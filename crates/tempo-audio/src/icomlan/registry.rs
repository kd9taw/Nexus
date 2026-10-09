//! At most one network session per radio in this process, and when the next one may start.
//!
//! **One session per radio.** The radio admits one network client, and three paths start CAT for a
//! radio: the radio loop, the monitor pool and a Remote selection. A second start for a radio that
//! already has a live session here is refused at once, sending nothing ("Nexus already has this
//! radio's network session"). A radio is known by its address and control port.
//!
//! **When to try again** is the retry ladder's (`tempo_net::icom::reconnect`): after a link that
//! went quiet or kept failing, or a connect the radio did not answer or refused as busy, the next
//! start waits 1, 2, 4, 8 and 16 s, then every 30 s; never once per tick, because each attempt
//! that fails part-way leaves the radio refusing new ones for longer. After the radio ended the
//! session itself (another program took it) or refused the login, nothing starts again until the
//! operator acts ([`operator_acted`]: Test CAT, or a saved change to the connection). A session
//! that connects starts the ladder again; one closed on purpose leaves nothing to wait for.
//!
//! Time is passed in, in milliseconds, so the ladder runs on a fake clock in the tests.

use std::collections::BTreeMap;
use std::net::SocketAddrV4;
use std::sync::{Mutex, PoisonError};

use tempo_net::icom::reconnect::{End, Ladder, Next};

#[derive(Debug, Default)]
struct Entry {
    live: bool,
    ladder: Ladder,
    /// No start before this time.
    retry_at: Option<u64>,
    /// No start until the operator acts, and why.
    hold: Option<String>,
}

static RADIOS: Mutex<BTreeMap<SocketAddrV4, Entry>> = Mutex::new(BTreeMap::new());

fn radios() -> std::sync::MutexGuard<'static, BTreeMap<SocketAddrV4, Entry>> {
    RADIOS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Why a start may not happen now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// This process already has a live session with the radio.
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
            Refusal::Live => "Nexus already has this radio's network session".into(),
            Refusal::Wait { remaining_ms } => format!(
                "Reconnecting to the radio in {} s (the radio may hold the old session for up to \
                 3 minutes)",
                remaining_ms.div_ceil(1_000)
            ),
            Refusal::Operator(why) => format!(
                "{why}. Nexus will not reconnect by itself: press Test CAT, or save the \
                 connection, to try again."
            ),
        }
    }
}

/// The right to run a session with one radio, until it ends. Dropping a claim that was not ended
/// gives the right back and changes nothing else.
#[derive(Debug)]
pub struct Claim {
    key: SocketAddrV4,
    open: bool,
}

/// Takes the radio's claim, unless a session with it is live or the ladder says wait.
pub fn claim(key: SocketAddrV4, now: u64) -> Result<Claim, Refusal> {
    let mut radios = radios();
    let entry = radios.entry(key).or_default();
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
    Ok(Claim { key, open: true })
}

impl Claim {
    /// The session connected: the next loss starts the ladder again.
    pub fn connected(&self) {
        if let Some(entry) = radios().get_mut(&self.key) {
            entry.ladder.connected();
        }
    }

    /// The session ended at `now`, like this; `why` is the operator's words for it. Decides when
    /// the next start may be.
    pub fn ended(mut self, end: &End, now: u64, why: &str) {
        let mut radios = radios();
        let entry = radios.entry(self.key).or_default();
        entry.live = false;
        match entry.ladder.after(end) {
            Next::RetryAfter(ms) => entry.retry_at = Some(now + ms),
            Next::WaitForOperator => entry.hold = Some(why.to_string()),
            Next::Stop => {}
        }
        self.open = false;
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if self.open {
            if let Some(entry) = radios().get_mut(&self.key) {
                entry.live = false;
            }
        }
    }
}

/// The operator acted (Test CAT, or a saved change to this connection): the next start may happen
/// at once.
pub fn operator_acted(key: SocketAddrV4) {
    if let Some(entry) = radios().get_mut(&key) {
        entry.hold = None;
        entry.retry_at = None;
    }
}

/// How long before a start may happen, or `None` when one may happen now (or only the operator
/// can make one happen).
pub fn retry_in(key: SocketAddrV4, now: u64) -> Option<u64> {
    let radios = radios();
    let entry = radios.get(&key)?;
    if entry.hold.is_some() {
        return None;
    }
    entry.retry_at.filter(|&at| at > now).map(|at| at - now)
}

/// Is this radio waiting for the operator?
pub fn held(key: SocketAddrV4) -> bool {
    radios().get(&key).is_some_and(|e| e.hold.is_some())
}

/// Forgets a radio altogether: its hold, its wait and its ladder. For the tests, whose simulated
/// radios listen on ephemeral loopback ports that the next test's radio can be given once one
/// closes, so none may inherit what another left here.
#[cfg(test)]
pub(crate) fn forget(key: SocketAddrV4) {
    radios().remove(&key);
}
