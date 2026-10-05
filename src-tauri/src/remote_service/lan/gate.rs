//! Who may even start a handshake with the shack, decided at accept, before a byte is read: the
//! caps and limits of the LAN road. Bounded state, clock passed in, nothing awaited, so a flood of
//! connections costs the listener a lock and a few comparisons each.
//!
//! The subnet check comes first and is the listener's (`Network::contains`): a source outside the
//! shack's own subnet never reaches the gate.
use std::collections::{BTreeMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Connections open at once, from everywhere.
pub(super) const MAX_OPEN: usize = 4;
/// Connections open at once from one address.
pub(super) const MAX_OPEN_PER_SOURCE: usize = 2;
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
    /// [`MAX_OPEN`] connections are open already, or the gate remembers [`SOURCES`] addresses.
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
pub(super) struct Gate {
    sources: Mutex<BTreeMap<IpAddr, Source>>,
}

/// One admitted connection's place under the caps, given back when it is dropped, however the
/// connection ends.
pub(super) struct Ticket {
    gate: Arc<Gate>,
    source: IpAddr,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if let Ok(mut sources) = self.gate.sources.lock() {
            if let Some(source) = sources.get_mut(&self.source) {
                source.open = source.open.saturating_sub(1);
            }
        }
    }
}

impl Gate {
    /// May `source` start a connection now? Every arrival counts toward its rate, a refused one
    /// included, so a source that hammers the port stays refused. Only the newest
    /// [`ARRIVALS_PER_MINUTE`] + 1 are kept, all the rule ever reads, so a flood costs one address
    /// eleven entries however fast it comes.
    pub fn admit(self: &Arc<Self>, source: IpAddr, now: Instant) -> Result<Ticket, Refused> {
        let mut sources = self.sources.lock().map_err(|_| Refused::Full)?;
        sources.retain(|_, s| {
            s.forget_before(now);
            !s.idle()
        });
        if !sources.contains_key(&source) && sources.len() >= SOURCES {
            return Err(Refused::Full);
        }
        let open: usize = sources.values().map(|s| s.open).sum();
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
        if open >= MAX_OPEN {
            return Err(Refused::Full);
        }
        entry.open += 1;
        Ok(Ticket {
            gate: self.clone(),
            source,
        })
    }

    /// How many of `source`'s arrivals the gate holds now.
    #[cfg(test)]
    pub fn arrivals(&self, source: IpAddr) -> usize {
        self.sources
            .lock()
            .map_or(0, |s| s.get(&source).map_or(0, |s| s.arrivals.len()))
    }

    /// A handshake from `source` failed: it never proved a paired key, spoke another protocol, or
    /// ran out of time. Enough of them in a minute and the address is ignored.
    pub fn failed(&self, source: IpAddr, now: Instant) {
        let Ok(mut sources) = self.sources.lock() else {
            return;
        };
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
