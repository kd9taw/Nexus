//! The Logbook's "Update from callbook": many calls looked up in the callbook, slowly, and their
//! answers handed to the fill ([`crate::logtools::callbook_fill`]), which writes each into the
//! EMPTY fields of the contacts logged with that call and nowhere else.
//!
//! This module is the lookups' loop and nothing more: which call next, when the next network
//! lookup may start, and when to stop. The lookup itself is the caller's ([`Callbook`]) — the
//! shell hands in the same cached lookup the callsign card makes, with the account it already
//! holds, so nothing here touches a password, a session or the network.
//!
//! # Paced, and capped per day
//!
//! - **A kept answer costs nothing.** A call the lookup cache answers fresh ([`Callbook::kept`])
//!   is taken at once: no wait, and not counted against the cap.
//! - **Every network lookup waits its turn first** ([`Limits::pace`]), the first one too, so `N`
//!   network lookups take at least `N × pace` and a run started straight after another cannot
//!   burst. The wait is cancellable.
//! - **The cap is per 24 hours, across runs** ([`Limits::daily_cap`]). Before each network
//!   lookup, what it would cost ([`Callbook::cost`]: the stroked call and its base are two) is
//!   added to what this station spent ([`Callbook::spent`], kept on disk), and a lookup that would
//!   pass the cap is not made: the run stops, [`Stop::Limit`]. QRZ publishes no daily number, so
//!   the cap is the operator's setting, 250 by default. ⚠️ **QRZ's own count of the account's
//!   lookups never holds the cap**: it counts every program on the account,
//!   so another program's day of lookups stopped every run before its first (a user report: QRZ
//!   said 16,191 against a cap of 6,000, above the setting's maximum). It is shown to the operator
//!   as information only; a QRZ that does refuse is caught by the refusal streak below.
//! - **A sign-in counts too**: a lookup that had to sign in again ([`Asked::logins`]) notes each
//!   sign-in against the cap once it is made, so the next lookup is held to it. Whether a lookup
//!   will sign in is not known before it is made, so the last lookup's sign-in can take the tally
//!   one past the cap; a session lasts hours, so that is one sign-in a run at most.
//! - **A call the callbook does not know** is reported ([`Lookups::unknown`]) and its contacts left
//!   alone. **A lookup that fails** (no sign-in, the network down) stops the run, [`Stop::Failed`]:
//!   the next lookup would only fail the same way, and each failure is still a request to QRZ.
//! - **A service down for a while** (callook.info is offline a few minutes each day) is not a
//!   failure: the call is left for another run ([`Looked::Later`]) and the run goes on, and a
//!   lookup that reached only that service is not counted against the cap.
//! - **QRZ refusing** stops the run, [`Stop::Refused`]. QRZ answers an unknown call and a refused
//!   lookup (its daily limit, a lapsed subscription, a suspended account) the same way, with no
//!   record, and only its wording differs, which Nexus does not read. One such answer is an
//!   unknown call. [`REFUSALS`] in a row, with no record from QRZ between them, is CHECKED
//!   ([`Callbook::own_call`]): the station's own call is looked up like any call, paced and
//!   counted against the cap. QRZ's record for it says QRZ is answering, so the streak was calls it
//!   does not know: they stay listed and the run goes on, the next check only after another
//!   streak. No record for it either is read as QRZ refusing, [`Refusal::OwnCallNoRecord`]: the
//!   run stops rather than go on asking, and the calls of that streak are left for another run
//!   instead of being listed as unknown. With no own call set the streak stops the run unchecked,
//!   [`Refusal::NoOwnCall`]. An operator whose own call is not on QRZ keeps that stop: Nexus
//!   cannot tell their streak from a refusal. (A user report: three unknown calls together
//!   stopped every run, read as QRZ refusing.)
//!
//! Whatever stops the run, the answers already in hand are the run's: the caller fills them as
//! ONE change, and a later run picks the rest up from the lookup cache at no cost.

use std::collections::HashMap;
use std::time::Duration;

use tempo_core::logbook::callbook::CallbookAnswer;

/// How fast and how far a run may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The least time before each network lookup.
    pub pace: Duration,
    /// The most network lookups in 24 hours, across runs.
    pub daily_cap: usize,
}

/// What one network lookup answered.
#[derive(Debug, Clone, PartialEq)]
pub enum Looked {
    Found(Box<CallbookAnswer>),
    /// The callbook was asked and holds no such call.
    Unknown,
    /// The lookup could not be made: the reason, as the operator reads it.
    Failed(String),
    /// A service that is down for a while (callook.info's daily maintenance) did not answer: the
    /// call is left for another run, and the run goes on.
    Later,
}

/// What one network lookup of a call answered, and what it says about the services.
#[derive(Debug, Clone, PartialEq)]
pub struct Asked {
    pub looked: Looked,
    /// QRZ's own count of the account's lookups in the last 24 hours, when its answer carried one.
    /// Shown to the operator; never counted against the cap (see the module header).
    pub qrz_count: Option<usize>,
    /// QRZ was asked for the call and gave no record for it: an unknown call, or a refusal.
    pub qrz_without_record: bool,
    /// QRZ itself answered with a record (for the call, or its base), whatever else answered.
    pub qrz_record: bool,
    /// The sign-ins the lookup made, beyond the lookups [`Callbook::cost`] counted.
    pub logins: usize,
    /// The lookup reached no service that counts ([`Looked::Later`] from callook.info alone): what
    /// was noted for it before it was made is taken back.
    pub uncounted: bool,
}

impl Asked {
    /// An answer with nothing more to say about the services.
    pub fn plain(looked: Looked) -> Self {
        Self {
            looked,
            qrz_count: None,
            qrz_without_record: false,
            qrz_record: false,
            logins: 0,
            uncounted: false,
        }
    }
}

/// QRZ answers with no record this many calls in a row: read as QRZ refusing lookups.
pub const REFUSALS: usize = 3;

/// The callbook as the loop uses it. The shell's is the cached lookup the callsign card makes;
/// the tests' a script with a fake clock.
pub trait Callbook {
    /// The answer the lookup cache holds for `call`, while it is fresh. Never the network.
    fn kept(&mut self, call: &str) -> Option<CallbookAnswer>;
    /// How many lookups a network lookup of `call` may make: the call, then its base for a
    /// stroked call. Counted against the cap before it is made.
    fn cost(&self, call: &str) -> usize {
        let _ = call;
        1
    }
    /// The network lookups this station noted in the last 24 hours.
    fn spent(&mut self) -> usize;
    /// Note `n` network lookups, made now.
    fn note_spent(&mut self, n: usize);
    /// Take back the last `n` noted: lookups that turned out not to count.
    fn forget_spent(&mut self, n: usize);
    /// Look `call` up over the network.
    fn live(&mut self, call: &str) -> Asked;
    /// Wait `d`. `false`: the run was cancelled while it waited.
    fn wait(&mut self, d: Duration) -> bool;
    /// Whether the operator cancelled the run.
    fn cancelled(&self) -> bool;
    /// The station's own call, looked up to check a streak of no-record answers; `None` when it
    /// is not set.
    fn own_call(&self) -> Option<String> {
        None
    }
}

/// Why a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// Every call was looked up.
    Done,
    Cancelled,
    /// The next network lookup would have passed the daily cap.
    Limit,
    /// A lookup failed: the reason.
    Failed(String),
    /// QRZ answered [`REFUSALS`] calls in a row with no record: read as a refusal.
    Refused(Refusal),
}

/// Why a streak of no-record answers was read as QRZ refusing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// QRZ had no record for the station's own call either.
    OwnCallNoRecord,
    /// No own call is set, so the streak could not be checked.
    NoOwnCall,
}

/// How far a run has got.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    /// Calls answered, from the cache or the network, or found unknown.
    pub done: usize,
    /// The calls the run was handed.
    pub total: usize,
    /// Of `done`, those the lookup cache answered.
    pub from_cache: usize,
    /// Network lookups made.
    pub looked_up: usize,
    /// Network lookups spent in the last 24 hours, as the cap reads them.
    pub spent: usize,
}

/// What a run found.
#[derive(Debug, Clone, PartialEq)]
pub struct Lookups {
    /// Each call answered, by its key ([`tempo_core::logbook::cleanup::call_key`]).
    pub answers: HashMap<String, CallbookAnswer>,
    /// The calls the callbook does not know.
    pub unknown: Vec<String>,
    pub stop: Stop,
    pub progress: Progress,
}

/// ★ Look `calls` up, kept answers first-hand and the rest over the network at `limits`' pace,
/// until every call is answered, the operator cancels, the cap is reached or a lookup fails.
/// `progress` is told after each call. See the module header.
pub fn look_up(
    calls: &[String],
    limits: Limits,
    callbook: &mut impl Callbook,
    progress: &mut dyn FnMut(&Progress),
) -> Lookups {
    let mut out = Lookups {
        answers: HashMap::new(),
        unknown: Vec::new(),
        stop: Stop::Done,
        progress: Progress {
            total: calls.len(),
            spent: callbook.spent(),
            ..Progress::default()
        },
    };
    // The calls QRZ answered with no record, in a row, and of those the ones listed as unknown.
    let mut streak = 0;
    let mut streak_unknown: Vec<String> = Vec::new();
    for call in calls {
        if callbook.cancelled() {
            out.stop = Stop::Cancelled;
            break;
        }
        if let Some(answer) = callbook.kept(call) {
            out.answers.insert(call.clone(), answer);
            out.progress.from_cache += 1;
            out.progress.done += 1;
            progress(&out.progress);
            continue;
        }
        let asked = match ask_network(call, limits, callbook, &mut out.progress) {
            Ok(asked) => asked,
            Err(stop) => {
                out.stop = stop;
                break;
            }
        };
        if asked.qrz_without_record {
            streak += 1;
        } else {
            streak = 0;
            streak_unknown.clear();
        }
        match asked.looked {
            Looked::Found(answer) => {
                out.answers.insert(call.clone(), *answer);
            }
            Looked::Unknown => {
                out.unknown.push(call.clone());
                if asked.qrz_without_record {
                    streak_unknown.push(call.clone());
                }
            }
            Looked::Failed(why) => {
                out.stop = Stop::Failed(why);
                break;
            }
            Looked::Later => {
                progress(&out.progress);
                continue;
            }
        }
        out.progress.done += 1;
        if streak >= REFUSALS {
            match check_streak(limits, callbook, &mut out.progress) {
                // Genuine misses: they stay listed, and the next check only after another streak.
                None => {
                    streak = 0;
                    streak_unknown.clear();
                }
                // Refused (or the check could not be made): not unknown, those calls are left for
                // another run.
                Some(stop) => {
                    out.unknown.retain(|c| !streak_unknown.contains(c));
                    out.progress.done -= streak_unknown.len();
                    out.stop = stop;
                    progress(&out.progress);
                    break;
                }
            }
        }
        progress(&out.progress);
    }
    out
}

/// One network lookup of `call`, held to the cap and the pace and noted in the tally, as every
/// network lookup is: `Err` is why the run stops before it is made.
fn ask_network(
    call: &str,
    limits: Limits,
    callbook: &mut impl Callbook,
    progress: &mut Progress,
) -> Result<Asked, Stop> {
    let spent = callbook.spent();
    progress.spent = spent;
    let cost = callbook.cost(call);
    if spent + cost > limits.daily_cap {
        return Err(Stop::Limit);
    }
    if !callbook.wait(limits.pace) {
        return Err(Stop::Cancelled);
    }
    // Noted before it is made: a lookup that fails was still a request.
    callbook.note_spent(cost);
    let asked = callbook.live(call);
    progress.looked_up += 1;
    if asked.uncounted {
        callbook.forget_spent(cost);
    }
    if asked.logins > 0 {
        callbook.note_spent(asked.logins);
    }
    progress.spent = callbook.spent();
    Ok(asked)
}

/// ★ A streak of [`REFUSALS`] no-record answers, checked: the station's own call is looked up
/// like any call (paced, capped, counted), and QRZ's own record for it says QRZ is answering, so
/// the streak was calls it does not know (`None`: go on). Anything else stops the run: no record
/// from QRZ (read as refusing, as before the check), no own call to check with, or the check's
/// own lookup failing, passing the cap or being cancelled.
fn check_streak(
    limits: Limits,
    callbook: &mut impl Callbook,
    progress: &mut Progress,
) -> Option<Stop> {
    let Some(own) = callbook.own_call() else {
        return Some(Stop::Refused(Refusal::NoOwnCall));
    };
    match ask_network(&own, limits, callbook, progress) {
        Err(stop) => Some(stop),
        Ok(Asked {
            looked: Looked::Failed(why),
            ..
        }) => Some(Stop::Failed(why)),
        Ok(asked) if asked.qrz_record => None,
        Ok(_) => Some(Stop::Refused(Refusal::OwnCallNoRecord)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A scripted callbook on a fake clock: `kept` answers from the cache, `known` over the
    /// network, `failing` fails; every other call is unknown.
    #[derive(Default)]
    struct Script {
        kept: Vec<&'static str>,
        known: Vec<&'static str>,
        failing: Vec<&'static str>,
        /// The fake clock, in milliseconds.
        now_ms: u64,
        /// When each network lookup started, by call.
        lookups: Vec<(String, u64)>,
        /// The station's tally: the time of each lookup noted.
        tally: Vec<u64>,
        /// Spent before the run, in the last day.
        spent_before: usize,
        /// QRZ's count, as its answers carry it: `Some(n)` reports `n + lookups made`.
        qrz_base: Option<usize>,
        /// Cancel once this many network lookups have been made.
        cancel_after: Option<usize>,
        /// Calls another service answered after QRZ gave no record.
        elsewhere: Vec<&'static str>,
        /// Sign-ins each lookup makes.
        logins: usize,
        /// Calls only a service that is down would answer: left for later, uncounted.
        down: Vec<&'static str>,
        /// The station's own call, the probe of a no-record streak; `None`: not set.
        own: Option<&'static str>,
        cancelled: Cell<bool>,
    }

    fn answer(call: &str) -> CallbookAnswer {
        CallbookAnswer {
            call: call.into(),
            name: Some("Op".into()),
            ..CallbookAnswer::default()
        }
    }

    impl Callbook for Script {
        fn kept(&mut self, call: &str) -> Option<CallbookAnswer> {
            self.kept.contains(&call).then(|| answer(call))
        }
        fn cost(&self, call: &str) -> usize {
            if call.contains('/') {
                2
            } else {
                1
            }
        }
        fn spent(&mut self) -> usize {
            self.spent_before + self.tally.len()
        }
        fn note_spent(&mut self, n: usize) {
            for _ in 0..n {
                self.tally.push(self.now_ms);
            }
        }
        fn forget_spent(&mut self, n: usize) {
            for _ in 0..n {
                self.tally.pop();
            }
        }
        fn live(&mut self, call: &str) -> Asked {
            self.lookups.push((call.to_string(), self.now_ms));
            if self.cancel_after.is_some_and(|n| self.lookups.len() >= n) {
                self.cancelled.set(true);
            }
            let count = self.qrz_base.map(|b| b + self.lookups.len());
            if self.down.contains(&call) {
                return Asked {
                    uncounted: true,
                    ..Asked::plain(Looked::Later)
                };
            }
            let looked = if self.failing.contains(&call) {
                Looked::Failed("QRZ could not be reached".into())
            } else if self.known.contains(&call) || self.elsewhere.contains(&call) {
                Looked::Found(Box::new(answer(call)))
            } else {
                Looked::Unknown
            };
            Asked {
                qrz_without_record: !self.known.contains(&call) && !self.failing.contains(&call),
                qrz_record: self.known.contains(&call),
                looked,
                qrz_count: count,
                logins: self.logins,
                uncounted: false,
            }
        }
        fn wait(&mut self, d: Duration) -> bool {
            if self.cancelled.get() {
                return false;
            }
            self.now_ms += d.as_millis() as u64;
            true
        }
        fn cancelled(&self) -> bool {
            self.cancelled.get()
        }
        fn own_call(&self) -> Option<String> {
            self.own.map(str::to_string)
        }
    }

    fn calls(c: &[&str]) -> Vec<String> {
        c.iter().map(|s| s.to_string()).collect()
    }

    const PACE: Duration = Duration::from_millis(1_500);

    fn limits(cap: usize) -> Limits {
        Limits {
            pace: PACE,
            daily_cap: cap,
        }
    }

    /// ★ PACING. `N` network lookups take at least `N × pace` on the clock, each starting at
    /// least `pace` after the one before (and the first `pace` after the start).
    #[test]
    fn network_lookups_are_paced() {
        let mut cb = Script {
            known: vec!["W1AW", "K1ABC", "N0XYZ", "G4ABC"],
            ..Script::default()
        };
        let run = look_up(
            &calls(&["W1AW", "K1ABC", "N0XYZ", "G4ABC"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(run.answers.len(), 4);
        let starts: Vec<u64> = cb.lookups.iter().map(|(_, t)| *t).collect();
        assert_eq!(starts, vec![1_500, 3_000, 4_500, 6_000]);
        assert!(cb.now_ms >= 4 * PACE.as_millis() as u64);
    }

    /// ★ THE CAP. A run never makes a lookup that would pass the day's cap, counting what was
    /// spent before it; it stops with what it has, and a stroked call costs two.
    #[test]
    fn the_cap_is_never_passed() {
        let all = calls(&["A1A", "B1B", "C1C", "D1D", "E1E"]);
        let mut cb = Script {
            known: vec!["A1A", "B1B", "C1C", "D1D", "E1E"],
            spent_before: 2,
            ..Script::default()
        };
        let run = look_up(&all, limits(5), &mut cb, &mut |_| {});
        assert_eq!(run.stop, Stop::Limit);
        assert_eq!(cb.lookups.len(), 3, "2 spent before + 3 = the cap of 5");
        assert_eq!(cb.spent(), 5);
        assert_eq!(run.answers.len(), 3);
        assert_eq!(run.progress.done, 3);

        // One left under the cap, and the next call is stroked: it would cost two. Not made.
        let mut cb = Script {
            known: vec!["A1A", "W1AW/P"],
            spent_before: 3,
            ..Script::default()
        };
        let run = look_up(&calls(&["A1A", "W1AW/P"]), limits(5), &mut cb, &mut |_| {});
        assert_eq!(run.stop, Stop::Limit);
        assert_eq!(cb.spent(), 4, "never past the cap");

        // At the cap already: nothing is asked at all.
        let mut cb = Script {
            known: vec!["A1A"],
            spent_before: 5,
            ..Script::default()
        };
        let run = look_up(&calls(&["A1A"]), limits(5), &mut cb, &mut |_| {});
        assert_eq!((run.stop, cb.lookups.len()), (Stop::Limit, 0));
    }

    /// ★ QRZ's own count never holds the cap: it counts every program on
    /// the account, so another program's lookups stopped every run at none (a user report: QRZ
    /// said 16,191 against a cap of 6,000). Only this station's own lookups count; a QRZ that
    /// refuses is still caught by the refusal streak.
    #[test]
    fn qrzs_own_count_does_not_hold_the_cap() {
        let mut cb = Script {
            known: vec!["A1A", "B1B", "C1C", "D1D"],
            qrz_base: Some(16_191),
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D"]),
            limits(10),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(cb.lookups.len(), 4);
        assert_eq!(run.answers.len(), 4);
        assert_eq!(
            run.progress.spent, 4,
            "the station's own tally, not QRZ's 16,195"
        );

        // The station's own tally still holds it, whatever QRZ says.
        let mut cb = Script {
            known: vec!["A1A", "B1B", "C1C", "D1D"],
            qrz_base: Some(0),
            spent_before: 8,
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D"]),
            limits(10),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!((run.stop, cb.lookups.len()), (Stop::Limit, 2));
    }

    /// ★ Cache hits cost nothing: no wait and no count, whatever the cap.
    #[test]
    fn kept_answers_neither_wait_nor_count() {
        let mut cb = Script {
            kept: vec!["A1A", "B1B", "C1C"],
            known: vec!["D1D"],
            spent_before: 0,
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D"]),
            limits(1),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(run.answers.len(), 4);
        assert_eq!((run.progress.from_cache, run.progress.looked_up), (3, 1));
        assert_eq!(cb.spent(), 1, "only the network lookup counted");
        assert_eq!(cb.now_ms, PACE.as_millis() as u64, "one wait, for D1D");
    }

    /// ★ Cancel: the run stops where it is, keeping every answer it had.
    #[test]
    fn cancel_keeps_what_was_found() {
        let mut cb = Script {
            known: vec!["A1A", "B1B", "C1C", "D1D"],
            cancel_after: Some(2),
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Cancelled);
        assert_eq!(cb.lookups.len(), 2);
        let mut got: Vec<&String> = run.answers.keys().collect();
        got.sort();
        assert_eq!(got, ["A1A", "B1B"]);
    }

    /// ★ A call the callbook does not know is reported and left alone; the run goes on.
    #[test]
    fn an_unknown_call_is_reported_and_the_run_goes_on() {
        let mut cb = Script {
            known: vec!["A1A", "C1C"],
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "Q9QQQ", "C1C"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(run.unknown, ["Q9QQQ"]);
        assert!(!run.answers.contains_key("Q9QQQ"));
        assert_eq!(run.answers.len(), 2);
        assert_eq!(run.progress.done, 3);
    }

    /// A lookup that fails stops the run: the next would fail the same way. What it found
    /// stands, and the failure was still a request, so it is counted.
    #[test]
    fn a_failed_lookup_stops_the_run() {
        let mut cb = Script {
            known: vec!["A1A", "C1C"],
            failing: vec!["B1B"],
            ..Script::default()
        };
        let mut seen = Vec::new();
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C"]),
            limits(250),
            &mut cb,
            &mut |p| seen.push(p.done),
        );
        assert_eq!(run.stop, Stop::Failed("QRZ could not be reached".into()));
        assert_eq!(cb.lookups.len(), 2, "C1C never asked");
        assert_eq!(cb.spent(), 2);
        assert_eq!(run.answers.len(), 1);
        assert_eq!(seen, [1], "progress after each call answered");
    }

    /// ★ QRZ REFUSING. QRZ answering [`REFUSALS`] calls in a row with no record stops the run,
    /// [`Stop::Refused`]: the next call is never asked. Those calls are left for another run, not
    /// listed as unknown; what was found stands, a record from another service among them too.
    #[test]
    fn qrz_answering_no_record_calls_in_a_row_stops_the_run() {
        let mut cb = Script {
            known: vec!["A1A", "F1F"],
            elsewhere: vec!["C1C"],
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D", "F1F"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Refused(Refusal::NoOwnCall));
        let asked: Vec<&str> = cb.lookups.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(asked, ["A1A", "B1B", "C1C", "D1D"], "F1F never asked");
        assert!(
            run.unknown.is_empty(),
            "refused, not unknown: {:?}",
            run.unknown
        );
        let mut got: Vec<&String> = run.answers.keys().collect();
        got.sort();
        assert_eq!(got, ["A1A", "C1C"]);
        assert_eq!(run.progress.done, 2, "B1B and D1D are left for another run");
    }

    /// ★ The control: a call QRZ does not know, between calls it answers, is listed and the run
    /// goes on — however many there are, while QRZ answers a call between them.
    #[test]
    fn unknown_calls_between_records_are_listed_and_the_run_goes_on() {
        let mut cb = Script {
            known: vec!["A1A", "D1D", "G1G"],
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D", "E1E", "F1F", "G1G"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(run.unknown, ["B1B", "C1C", "E1E", "F1F"]);
        assert_eq!(run.progress.done, 7);
    }

    /// A lookup's sign-ins count against the cap: the next lookup is held to it.
    #[test]
    fn sign_ins_count_against_the_cap() {
        let mut cb = Script {
            known: vec!["A1A", "B1B", "C1C"],
            logins: 1,
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C"]),
            limits(4),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Limit);
        assert_eq!(
            cb.lookups.len(),
            2,
            "1 + 1 sign-in each: the third would pass 4"
        );
        assert_eq!(cb.spent(), 4);
    }

    /// ★ A service down for a while is not a failure: the call is left for another run, the run
    /// goes on to the next, and the lookup is not counted against the cap.
    #[test]
    fn a_service_down_for_a_while_leaves_the_call_for_later_and_costs_nothing() {
        let mut cb = Script {
            known: vec!["A1A", "C1C"],
            down: vec!["W1AW"],
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "W1AW", "C1C"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(cb.lookups.len(), 3, "C1C asked after it");
        assert!(run.unknown.is_empty(), "not unknown");
        assert_eq!(run.answers.len(), 2);
        assert_eq!(run.progress.done, 2, "W1AW left for another run");
        assert_eq!(cb.spent(), 2, "W1AW's lookup not counted");
    }

    /// The operator's report: a few calls QRZ does not know, in a row, stopped the whole update
    /// as "QRZ refusing". QRZ words a miss and a refusal alike and Nexus does not read the words,
    /// so the streak is CHECKED instead: the station's own call is looked up, and a record for it
    /// says QRZ is answering, so the streak was genuine misses.
    fn probing(own: Option<&'static str>, known: Vec<&'static str>) -> Script {
        Script {
            own,
            known,
            ..Script::default()
        }
    }

    fn asked_calls(cb: &Script) -> Vec<&str> {
        cb.lookups.iter().map(|(c, _)| c.as_str()).collect()
    }

    /// ★ THREE GENUINE UNKNOWNS: the probe finds the station's own call, the calls are listed
    /// unknown and the run goes on to the end.
    #[test]
    fn a_streak_the_probe_shows_genuine_is_listed_and_the_run_goes_on() {
        let mut cb = probing(Some("W9OWN"), vec!["A1A", "F1F", "W9OWN"]);
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D", "F1F"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(
            asked_calls(&cb),
            ["A1A", "B1B", "C1C", "D1D", "W9OWN", "F1F"]
        );
        assert_eq!(run.unknown, ["B1B", "C1C", "D1D"]);
        assert_eq!(run.answers.len(), 2);
        assert_eq!(run.progress.done, 5);
    }

    /// ★ The probe is not made on every miss after it: the streak starts again, so the next probe
    /// comes only after another [`REFUSALS`].
    #[test]
    fn the_probe_comes_once_per_streak() {
        let mut cb = probing(Some("W9OWN"), vec!["W9OWN"]);
        let run = look_up(
            &calls(&["B1B", "C1C", "D1D", "E1E", "G1G", "H1H", "J1J"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(
            asked_calls(&cb),
            ["B1B", "C1C", "D1D", "W9OWN", "E1E", "G1G", "H1H", "W9OWN", "J1J"]
        );
        assert_eq!(run.unknown.len(), 7);
    }

    /// ★ THE PROBE FINDS NO RECORD for the station's own call either: QRZ is read as refusing and
    /// the run stops as before, the streak's calls left for another run.
    #[test]
    fn a_streak_the_probe_cannot_clear_stops_the_run() {
        let mut cb = probing(Some("W9OWN"), vec!["A1A", "F1F"]);
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D", "F1F"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Refused(Refusal::OwnCallNoRecord));
        assert_eq!(asked_calls(&cb), ["A1A", "B1B", "C1C", "D1D", "W9OWN"]);
        assert!(
            run.unknown.is_empty(),
            "left for another run: {:?}",
            run.unknown
        );
        assert_eq!(run.progress.done, 1);
    }

    /// A record from another callbook for the station's own call says nothing about QRZ: only
    /// QRZ's own record clears the streak.
    #[test]
    fn only_qrzs_record_for_the_own_call_clears_the_streak() {
        let mut cb = Script {
            own: Some("W9OWN"),
            elsewhere: vec!["W9OWN"],
            ..Script::default()
        };
        let run = look_up(
            &calls(&["B1B", "C1C", "D1D", "F1F"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Refused(Refusal::OwnCallNoRecord));
    }

    /// ★ THE PROBE COUNTS AGAINST THE CAP: noted like any lookup; and a probe that would pass the
    /// cap is not made, the run stopping at the limit.
    #[test]
    fn the_probe_counts_against_the_cap() {
        let mut cb = probing(Some("W9OWN"), vec!["W9OWN"]);
        let run = look_up(
            &calls(&["B1B", "C1C", "D1D", "E1E"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(cb.spent(), 5, "four calls and the probe");
        assert_eq!(run.progress.spent, 5);

        let mut cb = probing(Some("W9OWN"), vec!["W9OWN"]);
        let run = look_up(
            &calls(&["B1B", "C1C", "D1D", "E1E"]),
            limits(3),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Limit);
        assert_eq!(
            asked_calls(&cb),
            ["B1B", "C1C", "D1D"],
            "no probe past the cap"
        );
        assert_eq!(cb.spent(), 3);
    }

    /// ★ NO OWN CALL SET: nothing to probe with, so the streak stops the run as before, and the
    /// stop says it could not be checked.
    #[test]
    fn with_no_own_call_the_streak_stops_the_run_unchecked() {
        let mut cb = probing(None, vec!["A1A", "F1F"]);
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D", "F1F"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Refused(Refusal::NoOwnCall));
        assert_eq!(asked_calls(&cb), ["A1A", "B1B", "C1C", "D1D"]);
    }

    /// A probe that fails (a sign-in turned down, the network down) is the failure: the run
    /// stops on it as on any failed lookup.
    #[test]
    fn a_failed_probe_stops_the_run_as_a_failure() {
        let mut cb = Script {
            own: Some("W9OWN"),
            failing: vec!["W9OWN"],
            ..Script::default()
        };
        let run = look_up(
            &calls(&["B1B", "C1C", "D1D", "F1F"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Failed("QRZ could not be reached".into()));
        assert!(run.unknown.is_empty());
    }
}
