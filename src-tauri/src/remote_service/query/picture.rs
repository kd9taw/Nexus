//! One picture of the log for a Remote read — SPEC-2 v3's **C18** (§4.8), the station's half.
//!
//! A browser's questions about the log — the Log collection, recall, the award summary and the
//! statistics, the JS8 roster's history, the Connect and satellite coverage — were answered from
//! the whole copy of the log held in memory, 128 contacts at a time: each chunk took the Engine
//! lock, checked the log's read token, copied and let go, and a change between two chunks refused
//! the whole answer as busy, because a mixed log must never be reported as complete. They read the
//! logbook store now, and the chunks are gone: ONE read transaction is one picture of the log,
//! whatever commits while it runs.
//!
//! # What every Remote read of the log keeps to
//!
//! 1. **Handles under the lock, the read after it.** Under the Engine lock a reader takes the
//!    log's rows (`Engine::log_rows` — the store's handles, or a copy of pointers on the 1.13
//!    path) and the settings it answers for, nothing else; the pass runs once the lock is
//!    released. A debug build panics if it does not (`tempo_core::logbook::io_fence`).
//! 2. **One picture.** A pass over every contact and the whole records of the contacts it picked
//!    come from ONE read transaction ([`read`]), never from two reads a change could land
//!    between.
//! 3. **Never an answer older than the question (P4).** The read first waits, bounded, for the
//!    store to hold every change made before its rows were taken. If the writer is still behind
//!    when the wait runs out, the read is REFUSED as busy — the answer a page already retries —
//!    rather than served. The desktop's folds serve such an answer and do not keep it; a Remote
//!    capture is kept and shared with every browser, and one missing a contact logged a moment
//!    ago would call that station new.
//!
//! SQL narrows, Rust decides (SPEC-2 v3 P2): a reader names the columns it reads ([`Narrow`])
//! and runs its own test on every contact it is handed, so its answer is the one the log in
//! memory gave.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Arc;

use tempo_app::logstore::{Freshness, LogRows, READ_WAIT};
use tempo_core::logbook::sqlite::{call_norm_of, LogDb, Narrow, Order, Scope};
use tempo_core::logbook::{QsoRecord, RecordId};

/// What a read says when the store could not be read. Never an empty answer, which would call
/// every station on the air a new one.
const UNREADABLE: &str = "applicationUnavailable";

/// The log as one read sees it: the store inside one read transaction, or on the 1.13 path the
/// log in memory as a copy of its pointers. [`read`] makes it, for the length of one read.
pub(in crate::remote_service) enum Picture<'a> {
    Store(&'a LogDb),
    Memory(&'a [Arc<QsoRecord>]),
}

/// A contact a pass handed over: its place in log order in this picture (among the contacts its
/// pass visited), and its id — what its whole record is fetched by ([`Picture::whole`]). Two
/// picks of one pass are the same pick when they are the same place, which is the same contact.
#[derive(Debug, Clone, Copy)]
pub(in crate::remote_service) struct Pick {
    pub(super) at: usize,
    id: Option<RecordId>,
}

impl PartialEq for Pick {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at
    }
}
impl Eq for Pick {}
impl PartialOrd for Pick {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Pick {
    fn cmp(&self, other: &Self) -> Ordering {
        self.at.cmp(&other.at)
    }
}

impl Picture<'_> {
    /// Hand `each` every contact, in log order, with (at least) `narrow`'s fields filled, and its
    /// [`Pick`]. The first refusal `each` answers ends the pass, and is the answer.
    pub(in crate::remote_service) fn each(
        &self,
        narrow: Narrow,
        each: &mut dyn FnMut(Pick, &QsoRecord) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        self.each_in(Scope::All, narrow, each)
    }

    /// [`Self::each`] over only the contacts `scope` holds: on the store through the index that
    /// holds them, on the 1.13 path by the same test made here. A scope narrows, it never decides
    /// (P2): a reader still tests every contact it is handed.
    pub(in crate::remote_service) fn each_in(
        &self,
        scope: Scope<'_>,
        narrow: Narrow,
        each: &mut dyn FnMut(Pick, &QsoRecord) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        #[cfg(test)]
        seam(Seam::Each);
        match self {
            Picture::Store(db) => {
                let (mut at, mut answer) = (0, Ok(()));
                db.each_narrow(narrow, scope, Order::Log, &mut |q| {
                    let pick = Pick { at, id: q.id };
                    at += 1;
                    match each(pick, q) {
                        Ok(()) => ControlFlow::Continue(()),
                        Err(refused) => {
                            answer = Err(refused);
                            ControlFlow::Break(())
                        }
                    }
                })
                .map_err(|_| UNREADABLE)?;
                answer
            }
            Picture::Memory(rows) => rows
                .iter()
                .enumerate()
                .filter(|(_, q)| holds(scope, q))
                .try_for_each(|(at, q)| each(Pick { at, id: q.id }, q)),
        }
    }

    /// The whole records of `picks`, in the order given — from this same picture, so a contact
    /// edited or deleted since the pass that picked it is still the contact it picked.
    pub(in crate::remote_service) fn whole(
        &self,
        picks: &[Pick],
    ) -> Result<Vec<QsoRecord>, &'static str> {
        #[cfg(test)]
        seam(Seam::Whole);
        match self {
            Picture::Store(db) => {
                let ids = picks
                    .iter()
                    .map(|p| p.id.ok_or(UNREADABLE))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut found: HashMap<RecordId, QsoRecord> = db
                    .rows_by_ids(&ids)
                    .map_err(|_| UNREADABLE)?
                    .into_iter()
                    .filter_map(|r| Some((r.id?, r)))
                    .collect();
                // In one picture every contact a pass picked is there to be read.
                ids.iter()
                    .map(|id| found.remove(id).ok_or(UNREADABLE))
                    .collect()
            }
            Picture::Memory(rows) => picks
                .iter()
                .map(|p| {
                    rows.get(p.at)
                        .map(|r| QsoRecord::clone(r))
                        .ok_or(UNREADABLE)
                })
                .collect(),
        }
    }

    /// How many contacts the log holds.
    pub(super) fn count(&self) -> Result<usize, &'static str> {
        #[cfg(test)]
        seam(Seam::Count);
        match self {
            Picture::Store(db) => db
                .row_count()
                .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
                .map_err(|_| UNREADABLE),
            Picture::Memory(rows) => Ok(rows.len()),
        }
    }
}

/// Whether `scope` holds the contact `q`: the store's own test for each scope ([`Scope`]), made on
/// the 1.13 path's rows.
fn holds(scope: Scope<'_>, q: &QsoRecord) -> bool {
    match scope {
        Scope::All => true,
        Scope::Since(t) => q.when_unix >= t,
        Scope::CallNorm(norm) => call_norm_of(&q.call) == norm,
    }
}

/// The two seconds a Remote read of the log has always had, from before it takes the Engine
/// lock: checked every 128 contacts — once per chunk of the reads this replaced, which checked it
/// as they took the lock for each — a pass past it is refused as busy, as theirs were.
pub(super) fn within(deadline: std::time::Instant, pick: Pick) -> Result<(), &'static str> {
    if pick.at.is_multiple_of(128) && std::time::Instant::now() >= deadline {
        Err("applicationBusy")
    } else {
        Ok(())
    }
}

/// Run `f` against ONE picture of the log `rows` names — taken under the Engine lock by the
/// caller (`Engine::log_rows`), read here with the lock released. On the store it first waits for
/// every change made before `rows` was taken (P4), and refuses as busy if the writer is still
/// behind; then everything `f` reads comes from one read transaction.
///
/// ⚠️ Never call it holding the Engine lock; a debug build panics.
pub(in crate::remote_service) fn read<T>(
    rows: &LogRows,
    f: impl FnOnce(&Picture<'_>) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    tempo_core::logbook::io_fence::whole_log_off_engine_lock("a Remote read of the log");
    match rows {
        LogRows::Store(reads) => {
            let (answer, fresh) = reads
                .read(READ_WAIT, |db| Ok(f(&Picture::Store(db))))
                .map_err(|_| UNREADABLE)?;
            match fresh {
                Freshness::Current => answer,
                Freshness::Stale(_) => Err("applicationBusy"),
            }
        }
        LogRows::Memory(records) => f(&Picture::Memory(records)),
    }
}

/// Where a test may step into a read: as it counts the log, as its pass starts, and as it
/// fetches the whole records its pass picked — between two statements of one read transaction.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::remote_service) enum Seam {
    Count,
    Each,
    Whole,
}

#[cfg(test)]
type SeamHook = Box<dyn FnMut(Seam)>;

#[cfg(test)]
thread_local! {
    // A test's hook into the reads this thread makes ([`at_seams`]). Per thread: a Remote read
    // runs on the thread that asks, and the harness runs tests in parallel.
    static SEAM: std::cell::RefCell<Option<SeamHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn seam(at: Seam) {
    // Taken out while it runs, so a hook that reads the log itself cannot re-enter itself.
    if let Some(mut hook) = SEAM.with(|s| s.borrow_mut().take()) {
        hook(at);
        SEAM.with(|s| *s.borrow_mut() = Some(hook));
    }
}

/// Run `hook` at every [`Seam`] of every read this thread makes while `body` runs.
#[cfg(test)]
pub(in crate::remote_service) fn at_seams<T>(
    hook: impl FnMut(Seam) + 'static,
    body: impl FnOnce() -> T,
) -> T {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            SEAM.with(|s| *s.borrow_mut() = None);
        }
    }
    SEAM.with(|s| *s.borrow_mut() = Some(Box::new(hook)));
    let _clear = Clear;
    body()
}

#[cfg(test)]
mod tests {
    //! The three rules, each shown on the Log collection's window — the reader whose old form
    //! held the Engine lock across the whole log (SPEC-2 v3 F5).
    use super::super::log_tests::{launch, memory, parse_one, settle, synthetic_log, Dir};
    use super::super::log_window;
    use super::*;
    use std::time::Duration;
    use tempo_core::logbook::sqlite::WriteHold;

    /// A contact to log: ZD7AA, now, which no synthetic log holds.
    fn zd7() -> QsoRecord {
        let mut r = parse_one(
            "<CALL:5>ZD7AA<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260829<TIME_ON:6>030000<EOR>",
        );
        r.when_unix = 1_788_000_000;
        r
    }

    /// The window for ZD7AA, read as the Log collection reads it: the rows under the lock, the
    /// read after it is released.
    fn window(e: &crate::SharedEngine) -> Result<usize, &'static str> {
        let rows = e.lock().unwrap().log_rows();
        read(&rows, |log| log_window(log, "zd7aa", false)).map(|(_, total)| total)
    }

    /// ★ P4, AND THE ENGINE FREE: asked straight after a contact is logged — while its write is
    /// held up, as another process's write holds it — the read waits for it with the Engine lock
    /// free (checked from this thread while it waits), and then has it.
    #[test]
    fn a_read_waits_for_a_contact_just_logged_with_the_engine_lock_free() {
        let d = Dir::new("p4");
        std::fs::write(d.log(), synthetic_log(200, 0x000C_18A4)).unwrap();
        let e = launch(&d);
        assert_eq!(window(&e), Ok(0), "premise: ZD7AA is not in the log");
        let hold = WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(zd7());
        let found = std::thread::scope(|s| {
            let asked = s.spawn(|| window(&e));
            std::thread::sleep(Duration::from_millis(250));
            assert!(
                !asked.is_finished(),
                "the read waits for the contact's write"
            );
            for _ in 0..5 {
                assert!(
                    e.try_lock().is_ok(),
                    "the Engine lock is free while it waits"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(hold);
            asked.join().unwrap()
        });
        assert_eq!(
            found,
            Ok(1),
            "the read has the contact logged before it was asked"
        );
        settle(&e);
    }

    /// ★ NEVER OLDER THAN THE QUESTION: a writer still behind when the wait runs out — the
    /// store's lock held past it — makes the read a refusal the page retries, not an answer
    /// missing the contact. The control: the same read once the write has landed answers, with
    /// the contact.
    #[test]
    fn a_read_the_writer_has_not_caught_up_with_is_refused_as_busy() {
        let d = Dir::new("stale");
        std::fs::write(d.log(), synthetic_log(50, 0x000C_18A5)).unwrap();
        let e = launch(&d);
        let hold = WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(zd7());
        let stale = window(&e);
        drop(hold);
        assert_eq!(stale, Err("applicationBusy"));
        assert_eq!(window(&e), Ok(1), "control: once written, the read answers");
        settle(&e);
    }

    /// ★ POSITIVE CONTROL for the fence every Remote read of the log passes: made while this
    /// thread holds an Engine guard, it is a panic in a debug build.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "io_fence: a Remote read of the log is a pass over the whole log")]
    fn a_read_under_the_engine_lock_is_refused() {
        let e = memory(&synthetic_log(5, 0x000C_18A6));
        let eng = tempo_app::engine::engine_lock(&e);
        let rows = eng.log_rows();
        let _ = read(&rows, |log| log.count());
    }
}
