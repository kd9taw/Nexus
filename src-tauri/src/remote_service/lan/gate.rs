//! Who may even start a handshake with the shack, decided at accept, before a byte is read: the
//! caps and limits of the LAN road. Bounded state, clock passed in, nothing awaited, so a flood of
//! connections costs the listener a lock and a few comparisons each.
//!
//! The subnet check comes first and is the listener's (`Network::contains`): a source outside the
//! shack's own subnet never reaches the gate.
//!
//! ## Handshakes never hold a session's place
//!
//! A connection is a handshake until it has proved a paired key (or asked to pair or enrol), and
//! only then takes one of the [`MAX_OPEN`] places ([`Ticket::opened`]). Handshakes have places of
//! their own, [`HANDSHAKES`] of them, and a new arrival when all are taken cuts the oldest
//! ([`Ticket::cut`]): a computer's own handshake on the LAN takes milliseconds, so a host that
//! keeps slow handshakes open from many addresses holds no place a real computer needs, and an
//! open session, its Stop and its presence never wait on any of it.
use std::collections::{BTreeMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::Notify;

/// Sessions open at once, from everywhere: connections whose handshake has finished.
pub(super) const MAX_OPEN: usize = 4;
/// Connections open at once from one address, handshakes and sessions alike.
pub(super) const MAX_OPEN_PER_SOURCE: usize = 2;
/// Handshakes under way at once, from everywhere. One more cuts the oldest.
pub(super) const HANDSHAKES: usize = 8;
/// New connections one address may start in a minute.
pub(super) const ARRIVALS_PER_MINUTE: usize = 10;
/// Failed handshakes from one address in a minute that get it ignored for [`IGNORED_FOR`].
pub(super) const FAILURES_BEFORE_IGNORED: usize = 5;
pub(super) const IGNORED_FOR: Duration = Duration::from_secs(300);
/// The addresses the gate remembers at once. A network with more machines than this trying the
/// port is refused until some of them age out: LAN Remote waits, nothing else does.
pub(super) const SOURCES: usize = 256;
const MINUTE: Duration = Duration::from_secs(60);

/// Why a connection was dropped at accept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refused {
    /// This address failed too many handshakes, and is ignored for a while.
    Ignored,
    /// This address is starting connections faster than [`ARRIVALS_PER_MINUTE`].
    TooFast,
    /// [`MAX_OPEN_PER_SOURCE`] connections from this address are open already.
    SourceFull,
    /// [`MAX_OPEN`] sessions are open already, or the gate remembers [`SOURCES`] addresses.
    Full,
}

#[derive(Default)]
struct Source {
    open: usize,
    arrivals: VecDeque<Instant>,
    failures: VecDeque<Instant>,
    ignored_until: Option<Instant>,
}

impl Source {
    fn forget_before(&mut self, now: Instant) {
        let old = |at: &Instant| now.saturating_duration_since(*at) >= MINUTE;
        while self.arrivals.front().is_some_and(old) {
            self.arrivals.pop_front();
        }
        while self.failures.front().is_some_and(old) {
            self.failures.pop_front();
        }
        if self.ignored_until.is_some_and(|until| now >= until) {
            self.ignored_until = None;
        }
    }

    fn idle(&self) -> bool {
        self.open == 0
            && self.arrivals.is_empty()
            && self.failures.is_empty()
            && self.ignored_until.is_none()
    }
}

#[derive(Default)]
struct State {
    sources: BTreeMap<IpAddr, Source>,
    /// The handshakes under way, the oldest first: each one's number, and what cuts it.
    handshakes: VecDeque<(u64, Arc<Notify>)>,
    /// The last handshake's number.
    numbered: u64,
    /// Sessions open, under [`MAX_OPEN`].
    sessions: usize,
}

#[derive(Default)]
pub(super) struct Gate {
    state: Mutex<State>,
}

/// One admitted connection's place under the caps, given back when it is dropped, however the
/// connection ends: a handshake's place until [`Ticket::opened`], a session's after.
pub(super) struct Ticket {
    gate: Arc<Gate>,
    source: IpAddr,
    place: Place,
}

enum Place {
    /// Handshake number `.0`, cut by `.1`.
    Handshake(u64, Arc<Notify>),
    Session,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if let Ok(mut state) = self.gate.state.lock() {
            if let Some(source) = state.sources.get_mut(&self.source) {
                source.open = source.open.saturating_sub(1);
            }
            match &self.place {
                Place::Handshake(number, _) => state.handshakes.retain(|(n, _)| n != number),
                Place::Session => state.sessions = state.sessions.saturating_sub(1),
            }
        }
    }
}

impl Ticket {
    /// Resolves once a newer arrival has cut this handshake ([`HANDSHAKES`]); never for a session.
    pub async fn cut(&self) {
        match &self.place {
            Place::Handshake(_, cut) => cut.notified().await,
            Place::Session => std::future::pending().await,
        }
    }

    /// The handshake is done: its place goes to the next one, and the connection takes one of the
    /// [`MAX_OPEN`] sessions' places, or is refused [`Refused::Full`] while every one is taken.
    pub fn opened(&mut self) -> Result<(), Refused> {
        let mut state = self.gate.state.lock().map_err(|_| Refused::Full)?;
        if let Place::Handshake(number, _) = &self.place {
            let number = *number;
            state.handshakes.retain(|(n, _)| *n != number);
            if state.sessions >= MAX_OPEN {
                return Err(Refused::Full);
            }
            state.sessions += 1;
            self.place = Place::Session;
        }
        Ok(())
    }
}

impl Gate {
    /// May `source` start a handshake now? Every arrival counts toward its rate, a refused one
    /// included, so a source that hammers the port stays refused. Only the newest
    /// [`ARRIVALS_PER_MINUTE`] + 1 are kept, all the rule ever reads, so a flood costs one address
    /// eleven entries however fast it comes. Admitted while [`HANDSHAKES`] are under way, it cuts
    /// the oldest of them.
    pub fn admit(self: &Arc<Self>, source: IpAddr, now: Instant) -> Result<Ticket, Refused> {
        let mut state = self.state.lock().map_err(|_| Refused::Full)?;
        let state = &mut *state;
        let sources = &mut state.sources;
        sources.retain(|_, s| {
            s.forget_before(now);
            !s.idle()
        });
        if !sources.contains_key(&source) && sources.len() >= SOURCES {
            return Err(Refused::Full);
        }
        let entry = sources.entry(source).or_default();
        if entry.ignored_until.is_some() {
            return Err(Refused::Ignored);
        }
        entry.arrivals.push_back(now);
        while entry.arrivals.len() > ARRIVALS_PER_MINUTE + 1 {
            entry.arrivals.pop_front();
        }
        if entry.arrivals.len() > ARRIVALS_PER_MINUTE {
            return Err(Refused::TooFast);
        }
        if entry.open >= MAX_OPEN_PER_SOURCE {
            return Err(Refused::SourceFull);
        }
        entry.open += 1;
        if state.handshakes.len() >= HANDSHAKES {
            if let Some((_, oldest)) = state.handshakes.pop_front() {
                oldest.notify_one();
            }
        }
        state.numbered += 1;
        let cut = Arc::new(Notify::new());
        state.handshakes.push_back((state.numbered, cut.clone()));
        Ok(Ticket {
            gate: self.clone(),
            source,
            place: Place::Handshake(state.numbered, cut),
        })
    }

    /// How many of `source`'s arrivals the gate holds now.
    #[cfg(test)]
    pub fn arrivals(&self, source: IpAddr) -> usize {
        self.state.lock().map_or(0, |s| {
            s.sources.get(&source).map_or(0, |s| s.arrivals.len())
        })
    }

    /// How many of `source`'s connections are open now, under [`MAX_OPEN_PER_SOURCE`].
    #[cfg(test)]
    pub fn open(&self, source: IpAddr) -> usize {
        self.state
            .lock()
            .map_or(0, |s| s.sources.get(&source).map_or(0, |s| s.open))
    }

    /// How many of `source`'s failed handshakes the gate holds now.
    #[cfg(test)]
    pub fn failures(&self, source: IpAddr) -> usize {
        self.state.lock().map_or(0, |s| {
            s.sources.get(&source).map_or(0, |s| s.failures.len())
        })
    }

    /// A handshake from `source` failed: it never proved a paired key, spoke another protocol, or
    /// ran out of time. Enough of them in a minute and the address is ignored. A handshake cut to
    /// make room ([`Ticket::cut`]) is not one: newer arrivals cut it, not its address.
    pub fn failed(&self, source: IpAddr, now: Instant) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let sources = &mut state.sources;
        if !sources.contains_key(&source) && sources.len() >= SOURCES {
            return;
        }
        let entry = sources.entry(source).or_default();
        entry.forget_before(now);
        entry.failures.push_back(now);
        while entry.failures.len() > FAILURES_BEFORE_IGNORED {
            entry.failures.pop_front();
        }
        if entry.failures.len() >= FAILURES_BEFORE_IGNORED {
            entry.ignored_until = Some(now + IGNORED_FOR);
            entry.failures.clear();
        }
    }
}
