//! At most one TCI session per server and transceiver in this process, when the next one may start,
//! and a key a lost session may have left on the air.
//!
//! **One session per transceiver.** The radio loop, the monitor pool and a Remote selection can each
//! start CAT for a radio. A second start for a server and transceiver that already has a live
//! session here is refused at once, sending nothing. Two radios on one server's two transceivers are
//! two sessions, as TCI allows.
//!
//! **When to try again** is the reconnect ladder's (`tempo_net::tci::reconnect`): after a lost or
//! stale link, or a connect that found nothing, the next start waits 1, 2, 5 and 10 s, then every
//! 30 s; never once per tick. A link lost while a key of ours was up is tried again at once, once.
//! An address that answers but is not a TCI server, or a server that never says `ready;`, three
//! times in a row, waits for the operator ([`operator_acted`]: Test CAT, or a saved change to the
//! connection). A session that reaches `ready;` starts the ladder again; one closed on purpose
//! leaves nothing to wait for.
//!
//! **A key carried.** A session that ends with a key of ours possibly on the air (the link lost
//! mid-over, an unkey the server never confirmed, a teardown it did not confirm in time) leaves that
//! belief here, and the next session for the same server and transceiver takes it with its claim:
//! its stops go out first after `ready;`, and it holds the belief until the server reports the
//! transmitter off. Nothing else clears it: a connect that fails puts it back.
//!
//! Time is passed in, in milliseconds, so the ladder runs on a fake clock in the tests.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tempo_net::tci::reconnect::{End, Hold, Ladder, Step};
use tempo_net::tci::session::OurKey;
use tempo_net::tci::Trx;

/// A server and one of its transceivers.
pub type Key = (SocketAddr, Trx);

#[derive(Debug, Default)]
struct Entry {
    live: bool,
    ladder: Ladder,
    /// No start before this time.
    retry_at: Option<u64>,
    /// No start until the operator acts, and why.
    hold: Option<String>,
    /// A key of ours a session left possibly on the air, for the next session to stop first.
    carried: Option<OurKey>,
}

static SERVERS: Mutex<BTreeMap<Key, Entry>> = Mutex::new(BTreeMap::new());

fn servers() -> MutexGuard<'static, BTreeMap<Key, Entry>> {
    SERVERS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Why a start may not happen now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// This process already has a live session with this server's transceiver.
    Live,
    /// The ladder's wait has this long to run.
    Wait { remaining_ms: u64 },
    /// Waiting for the operator, after this.
    Operator(String),
}

impl Refusal {
    /// The operator's words.
    pub fn text(&self) -> String {
        match self {
            Refusal::Live => "Nexus already has a TCI session with this transceiver".into(),
            Refusal::Wait { remaining_ms } => format!(
                "Reconnecting to the SDR program in {} s",
                remaining_ms.div_ceil(1_000)
            ),
            Refusal::Operator(why) => format!(
                "{why}. Nexus will not reconnect by itself: check the SDR program's TCI server, \
                 then press Test CAT, or save the connection, to try again."
            ),
        }
    }
}

/// The right to run a session with one server's transceiver, until it ends. Dropping a claim that
/// was not ended gives the right back, and any key it carried, and changes nothing else.
#[derive(Debug)]
pub struct Claim {
    key: Key,
    carried: Option<OurKey>,
    open: bool,
}

/// Takes the transceiver's claim, with any key the last session left on the air, unless a session
/// with it is live, the operator is awaited, or the ladder says wait.
pub fn claim(key: Key, now: u64) -> Result<Claim, Refusal> {
    let mut servers = servers();
    let entry = servers.entry(key).or_default();
    if entry.live {
        return Err(Refusal::Live);
    }
    if let Some(why) = &entry.hold {
        return Err(Refusal::Operator(why.clone()));
    }
    if let Some(at) = entry.retry_at {
        if now < at {
            return Err(Refusal::Wait {
                remaining_ms: at - now,
            });
        }
    }
    entry.live = true;
    entry.retry_at = None;
    Ok(Claim {
        key,
        carried: entry.carried.take(),
        open: true,
    })
}

impl Claim {
    /// The key the last session left possibly on the air: the new session stops it first.
    pub fn carried(&self) -> Option<OurKey> {
        self.carried
    }

    /// The session reached `ready;`: the next loss starts the ladder again.
    pub fn connected(&self) {
        if let Some(entry) = servers().get_mut(&self.key) {
            entry.ladder.ready();
        }
    }

    /// The connect failed, or the session ended, at `now`, like `end`, with `carried` possibly on
    /// the air: what is carried is kept for the next session, and the ladder decides when that may
    /// start. Returns the ladder's steps, for the caller's alarm and log.
    pub fn ended(mut self, end: &End, carried: Option<OurKey>, now: u64) -> Vec<Step> {
        let mut servers = servers();
        let entry = servers.entry(self.key).or_default();
        entry.live = false;
        entry.carried = carried;
        let steps = entry.ladder.ended(end, carried);
        for step in &steps {
            match step {
                Step::Retry { after_ms } => entry.retry_at = Some(now.saturating_add(*after_ms)),
                Step::WaitForOperator(Hold::Refused(why)) => entry.hold = Some(why.clone()),
                Step::StillKeyed(_) => {}
            }
        }
        self.open = false;
        steps
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if self.open {
            if let Some(entry) = servers().get_mut(&self.key) {
                entry.live = false;
                entry.carried = entry.carried.or(self.carried);
            }
        }
    }
}

/// The operator acted (Test CAT, or a saved change to this connection): the next start may happen
/// at once, and the ladder starts again.
pub fn operator_acted(key: Key) {
    if let Some(entry) = servers().get_mut(&key) {
        entry.hold = None;
        entry.retry_at = None;
        entry.ladder.operator_connect();
    }
}

/// How long before a start may happen, or `None` when one may happen now (or only the operator can
/// make one happen).
pub fn retry_in(key: Key, now: u64) -> Option<u64> {
    let servers = servers();
    let entry = servers.get(&key)?;
    if entry.hold.is_some() {
        return None;
    }
    entry.retry_at.filter(|&at| at > now).map(|at| at - now)
}

/// Is this transceiver waiting for the operator?
pub fn held(key: Key) -> bool {
    servers().get(&key).is_some_and(|e| e.hold.is_some())
}

/// A key of ours a session left possibly on the air, while no session holds it: what the radio
/// loop counts as keyed between a lost session and the next (`was_keyed`), and why it keeps the
/// alarm.
pub fn carried(key: Key) -> Option<OurKey> {
    servers().get(&key).and_then(|e| e.carried)
}

/// Forgets a server's transceiver altogether: its hold, its wait, its ladder and what it carried.
/// For the tests, whose simulated servers listen on ephemeral loopback ports that the next test's
/// server can be given once one closes, so none may inherit what another left here.
#[cfg(test)]
pub(crate) fn forget(key: Key) {
    servers().remove(&key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_net::tci::admission::TxStart;
    use tempo_net::tci::session::{ConnectError, End as SessionEnd};

    fn key(port: u16) -> Key {
        (
            SocketAddr::from(([127, 0, 0, 1], port)),
            Trx::new(0).expect("trx 0"),
        )
    }

    fn keyed() -> OurKey {
        OurKey {
            trx: Trx::new(0).expect("trx 0"),
            kind: TxStart::Audio,
        }
    }

    /// One live session per transceiver; the ladder's wait; the hold until the operator acts.
    #[test]
    fn one_session_at_a_time_and_the_ladder_decides_the_next() {
        let k = key(1);
        forget(k);
        let first = claim(k, 0).expect("the first claim");
        assert_eq!(claim(k, 0).unwrap_err(), Refusal::Live);
        let lost = first.ended(&End::Ended(SessionEnd::Lost), None, 100);
        assert_eq!(lost, [Step::Retry { after_ms: 1_000 }]);
        assert_eq!(retry_in(k, 100), Some(1_000));
        assert_eq!(
            claim(k, 600).unwrap_err(),
            Refusal::Wait { remaining_ms: 500 }
        );
        // Three refusals in a row wait for the operator.
        let refused = End::Failed(ConnectError::Handshake("not a TCI server".into()));
        for n in 0..3 {
            let at = 10_000 * (n + 1);
            let c = claim(k, at).expect("the ladder's wait has run");
            c.ended(&refused, None, at);
        }
        assert!(held(k));
        assert_eq!(retry_in(k, 1_000_000), None);
        match claim(k, 1_000_000) {
            Err(Refusal::Operator(why)) => assert_eq!(why, "not a TCI server"),
            other => panic!("{other:?}"),
        }
        operator_acted(k);
        assert!(!held(k));
        claim(k, 1_000_000).expect("the operator acted");
        forget(k);
    }

    /// A key a session left on the air goes to the next claim, and a failed connect puts it back:
    /// only a session that ends with nothing of ours on the air clears it.
    #[test]
    fn a_carried_key_goes_to_the_next_session_and_survives_a_failed_connect() {
        let k = key(2);
        forget(k);
        let c = claim(k, 0).expect("a claim");
        let steps = c.ended(&End::Ended(SessionEnd::Lost), Some(keyed()), 0);
        assert_eq!(
            steps,
            [Step::StillKeyed(keyed()), Step::Retry { after_ms: 0 }],
            "lost while keyed: retried at once"
        );
        assert_eq!(carried(k), Some(keyed()));
        let c = claim(k, 0).expect("retried at once");
        assert_eq!(c.carried(), Some(keyed()));
        assert_eq!(carried(k), None, "the claim holds it now");
        let unreachable = End::Failed(ConnectError::Unreachable("refused".into()));
        c.ended(&unreachable, Some(keyed()), 0);
        assert_eq!(carried(k), Some(keyed()), "a failed connect puts it back");
        // A claim dropped without an end gives the key back too.
        let c = claim(k, 1_000).expect("past the ladder's wait");
        drop(c);
        assert_eq!(carried(k), Some(keyed()));
        // A session that ends with nothing of ours on the air clears it.
        let c = claim(k, 1_000).expect("a claim");
        c.ended(&End::Ended(SessionEnd::Closed), None, 1_000);
        assert_eq!(carried(k), None);
        forget(k);
    }
}
