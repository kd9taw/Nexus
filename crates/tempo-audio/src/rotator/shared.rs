//! One position read in flight per rotator, shared by everyone who wants to know where it points.
//!
//! ⭐ **Every surface used to ask rotctld for itself** (2026-11-04, rotators reached over the
//! internet). The rotor strip in each cockpit polled every 2 s, the Rotor pane every 2 s (a `p` and
//! a `\dump_state`), the Needed panel every 5 s, and a satellite or Moon track read the mast as
//! well. rotctld runs ONE command at a time for all of its clients, so over a slow link those reads
//! queued behind each other — and a read Nexus had already given up on still ran on the serial
//! line, so the queue grew instead of draining, and a STOP or a track's `P` waited behind all of it.
//!
//! Now a rotator has at most one position read from these callers in flight, and they share it:
//! - [`latest_position`]: the polls (strip, pane, Needed) and a track asking whether a rotator it
//!   gave up on answers again. A caller that finds a read already in flight takes THAT read's
//!   answer instead of starting its own; one that finds none reads. So N callers at once cost one
//!   read, and an answer is never older than the read that produced it — nothing is kept between
//!   reads, so a rotctld that has gone away is "not answering" on the very next poll.
//! - [`fresh_position`]: a read that STARTED after the call — never one already in flight, which
//!   may have left before the mast was sent somewhere. For a track that has just steered the mast
//!   and needs to know where it went. It still waits for a read in flight to finish rather than
//!   putting a second one beside it.
//! - [`latest_limits`]: the same sharing for `\dump_state`, which the pane asks after every
//!   position.
//!
//! Moves, STOP and the reads a manual move makes for itself ([`super::point_keeping`]) do NOT come
//! through here: they go straight to rotctld as they always did. A STOP waits for nothing in Nexus;
//! at rotctld it waits behind at most the one read this module lets out.
//!
//! The answer is shared, so an error is stored as its kind and message and rebuilt for each caller:
//! the kind is what callers branch on (`Unsupported` is a rotator with no position to give).

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use super::Limits;

/// A finished read: when it left, and what came back (an error as its kind and text).
struct Done<T> {
    started: Instant,
    result: Result<T, (std::io::ErrorKind, String)>,
}

struct State<T> {
    /// When the read in flight left, if one is.
    in_flight: Option<Instant>,
    last: Option<Done<T>>,
}

struct Slot<T> {
    state: Mutex<State<T>>,
    changed: Condvar,
}

/// The slots of one kind of read, one per rotctld address.
struct Shared<T> {
    slots: Mutex<Option<HashMap<String, Arc<Slot<T>>>>>,
}

/// Which read a caller will take.
#[derive(Clone, Copy)]
enum Want {
    /// The read in flight when the caller arrived, or, with none in flight, one of its own.
    Joined,
    /// A read that left after the caller arrived.
    Fresh,
}

impl<T: Clone> Shared<T> {
    const fn new() -> Self {
        Shared {
            slots: Mutex::new(None),
        }
    }

    fn slot(&self, addr: &str) -> Arc<Slot<T>> {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots
            .get_or_insert_with(HashMap::new)
            .entry(addr.to_string())
            .or_insert_with(|| {
                Arc::new(Slot {
                    state: Mutex::new(State {
                        in_flight: None,
                        last: None,
                    }),
                    changed: Condvar::new(),
                })
            })
            .clone()
    }

    /// The answer of a read that left at or after a point `want` sets, reading with `read` only
    /// when no such read has finished or is on its way. At most one `read` runs per address at
    /// any moment.
    fn get(
        &self,
        addr: &str,
        want: Want,
        read: impl FnOnce() -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        let slot = self.slot(addr);
        let mut st = slot.state.lock().unwrap_or_else(|e| e.into_inner());
        let arrived = Instant::now();
        // The oldest read this caller takes: a poll joins the one in flight; a fresh read wants
        // one that leaves after it asked.
        let since = match (want, st.in_flight) {
            (Want::Joined, Some(left)) => left,
            _ => arrived,
        };
        loop {
            if let Some(done) = st.last.as_ref().filter(|d| d.started >= since) {
                return rebuild(&done.result);
            }
            if st.in_flight.is_none() {
                break;
            }
            st = slot.changed.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        let started = Instant::now();
        st.in_flight = Some(started);
        drop(st);
        let result = read();
        let mut st = slot.state.lock().unwrap_or_else(|e| e.into_inner());
        st.in_flight = None;
        st.last = Some(Done {
            started,
            result: result
                .as_ref()
                .map(Clone::clone)
                .map_err(|e| (e.kind(), e.to_string())),
        });
        drop(st);
        slot.changed.notify_all();
        result
    }
}

fn rebuild<T: Clone>(result: &Result<T, (std::io::ErrorKind, String)>) -> std::io::Result<T> {
    result
        .clone()
        .map_err(|(kind, text)| std::io::Error::new(kind, text))
}

type Position = (f64, Option<f64>);

static POSITIONS: Shared<Position> = Shared::new();
static LIMITS: Shared<Option<Limits>> = Shared::new();

/// Where the rotator at `addr` points: the answer to the read already in flight, or to one of its
/// own when none is. What every poll asks. Errors as [`super::read_position`] gives them.
pub fn latest_position(addr: &str) -> std::io::Result<Position> {
    POSITIONS.get(addr, Want::Joined, || super::read_position(addr))
}

/// Where the rotator at `addr` points NOW: a read that left after this call. For a caller that has
/// just moved the mast.
pub fn fresh_position(addr: &str) -> std::io::Result<Position> {
    POSITIONS.get(addr, Want::Fresh, || super::read_position(addr))
}

/// What the rotctld at `addr` declares ([`super::read_limits`]), shared the way
/// [`latest_position`] is.
pub fn latest_limits(addr: &str) -> std::io::Result<Option<Limits>> {
    LIMITS.get(addr, Want::Joined, || super::read_limits(addr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;
    use std::time::Duration;

    /// A stand-in read: counts how many run at once and in all, and takes `ms` to answer with the
    /// number of reads so far, so a caller can tell which read it got.
    struct Probe {
        now: AtomicUsize,
        most: AtomicUsize,
        total: AtomicUsize,
    }

    impl Probe {
        fn new() -> Arc<Probe> {
            Arc::new(Probe {
                now: AtomicUsize::new(0),
                most: AtomicUsize::new(0),
                total: AtomicUsize::new(0),
            })
        }

        fn read(&self, ms: u64) -> std::io::Result<Position> {
            let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(n, Ordering::SeqCst);
            let k = self.total.fetch_add(1, Ordering::SeqCst) + 1;
            std::thread::sleep(Duration::from_millis(ms));
            self.now.fetch_sub(1, Ordering::SeqCst);
            Ok((k as f64, None))
        }
    }

    #[test]
    fn many_polls_at_once_share_one_read() {
        let shared: Arc<Shared<Position>> = Arc::new(Shared::new());
        let probe = Probe::new();
        let go = Arc::new(Barrier::new(8));
        let polls: Vec<_> = (0..8)
            .map(|_| {
                let (shared, probe, go) = (shared.clone(), probe.clone(), go.clone());
                std::thread::spawn(move || {
                    go.wait();
                    shared.get("a:1", Want::Joined, || probe.read(300))
                })
            })
            .collect();
        for p in polls {
            assert_eq!(p.join().expect("a poll").expect("a position"), (1.0, None));
        }
        assert_eq!(probe.most.load(Ordering::SeqCst), 1, "reads at once");
        assert_eq!(probe.total.load(Ordering::SeqCst), 1, "reads in all");
    }

    #[test]
    fn nothing_is_kept_between_reads() {
        // A poll with nothing in flight reads: a rotctld that has gone away since the last answer
        // is "not answering" at once, never shown at its old position.
        let shared: Shared<Position> = Shared::new();
        let probe = Probe::new();
        assert_eq!(
            shared.get("a:1", Want::Joined, || probe.read(0)).unwrap().0,
            1.0
        );
        let gone = shared.get("a:1", Want::Joined, || {
            Err(std::io::Error::from(std::io::ErrorKind::ConnectionRefused))
        });
        assert_eq!(
            gone.unwrap_err().kind(),
            std::io::ErrorKind::ConnectionRefused
        );
        // Another rotator is another slot.
        assert_eq!(
            shared.get("b:1", Want::Joined, || probe.read(0)).unwrap().0,
            2.0
        );
    }

    #[test]
    fn a_fresh_read_never_takes_an_answer_that_left_before_it_asked() {
        let shared: Arc<Shared<Position>> = Arc::new(Shared::new());
        let probe = Probe::new();
        // A read already in flight when it asks is not fresh: it waits for it (never two at
        // once), then reads for itself.
        let poll = {
            let (shared, probe) = (shared.clone(), probe.clone());
            std::thread::spawn(move || shared.get("a:1", Want::Joined, || probe.read(300)))
        };
        std::thread::sleep(Duration::from_millis(100));
        let fresh = shared
            .get("a:1", Want::Fresh, || probe.read(0))
            .expect("a position");
        assert_eq!(poll.join().unwrap().unwrap().0, 1.0);
        assert_eq!(fresh.0, 2.0, "the read in flight left before the ask");
        assert_eq!(probe.most.load(Ordering::SeqCst), 1, "reads at once");
    }

    #[test]
    fn a_poll_joins_a_fresh_read_in_flight() {
        let shared: Arc<Shared<Position>> = Arc::new(Shared::new());
        let probe = Probe::new();
        let fresh = {
            let (shared, probe) = (shared.clone(), probe.clone());
            std::thread::spawn(move || shared.get("a:1", Want::Fresh, || probe.read(300)))
        };
        std::thread::sleep(Duration::from_millis(100));
        let poll = shared.get("a:1", Want::Joined, || probe.read(0));
        assert_eq!(fresh.join().unwrap().unwrap().0, 1.0);
        assert_eq!(poll.unwrap().0, 1.0);
        assert_eq!(probe.total.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_error_is_shared_with_its_kind() {
        let shared: Arc<Shared<Position>> = Arc::new(Shared::new());
        let first = {
            let shared = shared.clone();
            std::thread::spawn(move || {
                shared.get("a:1", Want::Joined, || {
                    std::thread::sleep(Duration::from_millis(300));
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "the rotator does not report its position: \"RPRT -11\"",
                    ))
                })
            })
        };
        std::thread::sleep(Duration::from_millis(100));
        let joined = shared
            .get("a:1", Want::Joined, || Ok((1.0, None)))
            .unwrap_err();
        assert_eq!(joined.kind(), std::io::ErrorKind::Unsupported);
        assert!(joined.to_string().contains("RPRT -11"), "{joined}");
        assert_eq!(
            first.join().unwrap().unwrap_err().kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}
