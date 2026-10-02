//! A streamed operator's held PTT: a STATE the page keeps re-asserting, never a keydown/keyup pair.
//!
//! The page sends a hold every 100 ms while its PTT control is held, over a channel that never
//! retransmits. The station keys on the first hold it accepts and releases the over when none has
//! arrived for [`GAP`]. So a lost release cannot leave the rig keyed: there is no release to lose,
//! only a silence, and silence unkeys. The FD-cockpit class in the transmit-safety notes (a guard
//! that swallowed a key-up and left the rig keyed under a button reading "release to stop") cannot
//! recur at the transport, because the transport has no key-up to swallow.
//!
//! Two sides share one [`PttHold`]:
//! - the stream (any thread) records what arrived: [`PttHold::hold`], [`PttHold::release`],
//!   [`PttHold::end`], and takes what the page should be told with [`PttHold::reports`];
//! - the engine decides, on every radio-loop tick, through [`PttHold::tick`]. Keying and unkeying
//!   happen only there, through the engine's own PTT verb and every guard it carries.
//!
//! ⛔ AN ENDED PRESS NEVER KEYS AGAIN. A hold that arrives for a press that has lapsed, been
//! released, been refused or been stopped is ignored, however it got there: a hold delayed in a
//! queue, reordered behind its own release, or sent by a page that missed the unkey. A new press
//! takes a new id. The last [`ENDED_KEPT`] ended ids are remembered, which covers any delay a
//! channel with no retransmission can produce by orders of magnitude.
//!
//! ⛔ A PRESS THAT WAS NOT KEYED NEVER KEYS LATER. A press refused on its first tick (transmit off,
//! outside the licence, no transmit presence) stays refused for its whole life, even if the reason
//! goes away while it is held: keying is always the answer to a fresh press, never to a condition
//! changing under an old one. Likewise a keyed press that something else unkeyed (a Stop at the
//! shack, a halt) ends as stopped and does not re-key.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a keyed press survives with no hold arriving.
pub const GAP: Duration = Duration::from_millis(200);
/// How many ended press ids are remembered, so a late hold for one is ignored.
pub const ENDED_KEPT: usize = 64;

/// Why a press ended, or was never keyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldEnd {
    /// The station would not key it.
    Refused,
    /// No hold arrived for [`GAP`].
    Lapsed,
    /// The page released it.
    Released,
    /// Something at the station unkeyed it: a Stop, a halt, the stream ending.
    Stopped,
}

/// One thing the page should be told about a press.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoldReport {
    pub hold_id: String,
    pub keyed: bool,
    pub end: Option<HoldEnd>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Accepted, not yet decided by the engine.
    Pending,
    Keyed,
    Refused,
}

struct Press {
    id: String,
    deadline: Instant,
    phase: Phase,
    /// Set by a release or by the stream ending: the next tick ends the press with this reason.
    ending: Option<HoldEnd>,
}

#[derive(Default)]
struct State {
    press: Option<Press>,
    ended: VecDeque<String>,
    reports: VecDeque<HoldReport>,
}

impl State {
    fn report(&mut self, id: &str, keyed: bool, end: Option<HoldEnd>) {
        // Bounded: a page that stops reading its reports costs this nothing.
        if self.reports.len() >= 16 {
            self.reports.pop_front();
        }
        self.reports.push_back(HoldReport {
            hold_id: id.to_string(),
            keyed,
            end,
        });
    }

    fn retire(&mut self, id: String) {
        if self.ended.len() >= ENDED_KEPT {
            self.ended.pop_front();
        }
        self.ended.push_back(id);
    }
}

/// The shared held-PTT state. Cheap to clone; every clone is the same press.
#[derive(Clone, Default)]
pub struct PttHold(Arc<Mutex<State>>);

impl PttHold {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // Nothing done under this lock can panic halfway through a change that matters, and a
        // poisoned hold must still be able to unkey, so a poisoned lock is taken as it stands.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A hold arrived for press `id`. Returns false when it was ignored: that press has already
    /// ended, or the station has stopped the key and nothing may take it over.
    pub fn hold(&self, id: &str, now: Instant) -> bool {
        let mut s = self.state();
        if s.ended.iter().any(|e| e == id) {
            return false;
        }
        let deadline = now + GAP;
        let Some(press) = s.press.as_mut() else {
            s.press = Some(Press {
                id: id.to_string(),
                deadline,
                phase: Phase::Pending,
                ending: None,
            });
            return true;
        };
        if press.id == id {
            if press.ending.is_none() {
                press.deadline = deadline;
            }
            return press.ending.is_none();
        }
        if press.ending == Some(HoldEnd::Stopped) {
            return false;
        }
        // A new press over one still in force: its release was lost, or the operator pressed
        // again inside the gap. A keyed press hands its key straight over, because the key then
        // stays up only while the NEW press keeps arriving and admission holds, and `tick` checks
        // both every radio tick. Anything else is replaced and decided afresh.
        let old = std::mem::replace(&mut press.id, id.to_string());
        let was = press.phase;
        press.deadline = deadline;
        press.ending = None;
        if was != Phase::Keyed {
            press.phase = Phase::Pending;
        }
        // A refused press was already reported as refused.
        if was != Phase::Refused {
            s.report(&old, false, Some(HoldEnd::Released));
        }
        s.retire(old);
        if was == Phase::Keyed {
            s.report(id, true, None);
        }
        true
    }

    /// The page released press `id`. The next tick unkeys it.
    pub fn release(&self, id: &str) {
        let mut s = self.state();
        if let Some(press) = s.press.as_mut().filter(|p| p.id == id) {
            press.ending.get_or_insert(HoldEnd::Released);
        } else if !s.ended.iter().any(|e| e == id) {
            // A release that overtook its own first hold: the press is over before it began.
            s.retire(id.to_string());
        }
    }

    /// The stream ended, or its presence lapsed. Any press ends now, as stopped.
    pub fn end(&self) {
        let mut s = self.state();
        if let Some(press) = s.press.as_mut() {
            press.ending.get_or_insert(HoldEnd::Stopped);
        }
    }

    /// What the page should be told since the last call, oldest first.
    pub fn reports(&self) -> Vec<HoldReport> {
        self.state().reports.drain(..).collect()
    }

    /// True while a press is keyed.
    pub fn keyed(&self) -> bool {
        self.state()
            .press
            .as_ref()
            .is_some_and(|p| p.phase == Phase::Keyed)
    }

    /// One radio-loop tick, on the engine. `may_key` is whether a remote key would be admitted
    /// right now; `key_up` is whether the engine's manual key is still up. `ptt` is the engine's
    /// own PTT verb: called with `true` to key (it answers whether the key is now up) and with
    /// `false` to unkey. It is called at most once per tick, and only from here.
    pub fn tick(
        &self,
        now: Instant,
        may_key: bool,
        key_up: bool,
        mut ptt: impl FnMut(bool) -> bool,
    ) {
        let mut s = self.state();
        let Some(press) = s.press.as_mut() else {
            return;
        };
        let ending = press
            .ending
            .or((now >= press.deadline).then_some(HoldEnd::Lapsed));
        if let Some(end) = ending {
            let press = s.press.take().expect("a press is in force");
            if press.phase == Phase::Keyed && key_up {
                ptt(false);
            }
            // A refused press has already been reported as refused.
            if press.phase != Phase::Refused {
                s.report(&press.id, false, Some(end));
            }
            s.retire(press.id);
            return;
        }
        match press.phase {
            Phase::Pending => {
                let keyed = may_key && ptt(true);
                press.phase = if keyed { Phase::Keyed } else { Phase::Refused };
                let id = press.id.clone();
                if keyed {
                    s.report(&id, true, None);
                } else {
                    s.report(&id, false, Some(HoldEnd::Refused));
                }
            }
            Phase::Keyed if !key_up || !may_key => {
                // Unkeyed under it (a Stop, a halt) or no longer admitted (presence lapsed, the
                // mode changed): the press is over, and it does not come back.
                let press = s.press.take().expect("a press is in force");
                if key_up {
                    ptt(false);
                }
                s.report(&press.id, false, Some(HoldEnd::Stopped));
                s.retire(press.id);
            }
            Phase::Keyed | Phase::Refused => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "10000000-0000-4000-8000-00000000000a";
    const B: &str = "10000000-0000-4000-8000-00000000000b";

    /// The engine's PTT as the tests see it: whether the key is up, and every call made.
    #[derive(Default)]
    struct Rig {
        up: bool,
        refuse: bool,
        calls: Vec<bool>,
    }
    impl Rig {
        fn tick(&mut self, hold: &PttHold, now: Instant, may_key: bool) {
            let up = self.up;
            let refuse = self.refuse;
            let mut calls = Vec::new();
            let mut result = up;
            hold.tick(now, may_key, up, |on| {
                calls.push(on);
                result = on && !refuse;
                result
            });
            if !calls.is_empty() {
                self.up = result;
            }
            self.calls.extend(calls);
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_hold_keys_on_the_next_tick_and_silence_unkeys_after_the_gap() {
        let (hold, mut rig, t0) = (PttHold::default(), Rig::default(), Instant::now());
        assert!(hold.hold(A, t0));
        rig.tick(&hold, t0 + ms(20), true);
        assert!(rig.up, "the first accepted hold keys");
        // No more holds: still keyed just inside the gap, released at it.
        rig.tick(&hold, t0 + ms(199), true);
        assert!(rig.up, "released before the gap");
        rig.tick(&hold, t0 + ms(200), true);
        assert!(!rig.up, "the gap did not release the key");
        assert_eq!(rig.calls, vec![true, false]);
        let reports = hold.reports();
        assert_eq!(reports.len(), 2);
        assert!(reports[0].keyed);
        assert_eq!(reports[1].end, Some(HoldEnd::Lapsed));
    }

    /// ★ A6's native half: re-asserted every 100 ms, the over holds for as long as the holds come.
    #[test]
    fn a_hold_reasserted_every_100_ms_keeps_the_key_up() {
        let (hold, mut rig, t0) = (PttHold::default(), Rig::default(), Instant::now());
        for i in 0..30u64 {
            let at = t0 + ms(i * 100);
            assert!(hold.hold(A, at));
            for tick in 0..5u64 {
                rig.tick(&hold, at + ms(tick * 20), true);
                assert!(rig.up, "dropped at {} ms", i * 100 + tick * 20);
            }
        }
        assert_eq!(rig.calls, vec![true], "keyed once, never re-keyed");
        // CONTROL: the same press stops being re-asserted, and the key comes down one gap later.
        rig.tick(&hold, t0 + ms(2900 + 200), true);
        assert!(!rig.up);
    }

    #[test]
    fn a_release_unkeys_on_the_next_tick() {
        let (hold, mut rig, t0) = (PttHold::default(), Rig::default(), Instant::now());
        hold.hold(A, t0);
        rig.tick(&hold, t0, true);
        hold.release(A);
        rig.tick(&hold, t0 + ms(20), true);
        assert!(!rig.up);
        assert_eq!(hold.reports().last().unwrap().end, Some(HoldEnd::Released));
    }

    /// ⛔ An ended press never keys again — late holds, reordered holds, a hold after the release.
    #[test]
    fn an_ended_press_never_keys_again() {
        let (hold, mut rig, t0) = (PttHold::default(), Rig::default(), Instant::now());
        hold.hold(A, t0);
        rig.tick(&hold, t0, true);
        rig.tick(&hold, t0 + ms(300), true);
        assert!(!rig.up, "baseline: lapsed");
        // Holds for the lapsed press arrive late, in a burst.
        for i in 0..5 {
            assert!(!hold.hold(A, t0 + ms(400 + i)), "a late hold was accepted");
            rig.tick(&hold, t0 + ms(420 + i), true);
            assert!(!rig.up, "a late hold re-keyed an ended press");
        }
        // A release that overtook its own first hold: the press never starts.
        hold.release(B);
        assert!(!hold.hold(B, t0 + ms(500)));
        rig.tick(&hold, t0 + ms(520), true);
        assert!(!rig.up);
        // CONTROL: a genuinely new press keys.
        let c = "10000000-0000-4000-8000-00000000000c";
        assert!(hold.hold(c, t0 + ms(600)));
        rig.tick(&hold, t0 + ms(620), true);
        assert!(rig.up);
    }

    /// ⛔ A refused press stays refused, even if what refused it goes away while it is held.
    #[test]
    fn a_refused_press_does_not_key_when_the_reason_goes_away() {
        let (hold, mut rig, t0) = (PttHold::default(), Rig::default(), Instant::now());
        hold.hold(A, t0);
        rig.tick(&hold, t0, false);
        assert!(!rig.up);
        assert_eq!(hold.reports()[0].end, Some(HoldEnd::Refused));
        for i in 1..10u64 {
            hold.hold(A, t0 + ms(i * 100));
            rig.tick(&hold, t0 + ms(i * 100 + 10), true);
            assert!(!rig.up, "a refused press keyed once admission allowed it");
        }
        assert!(rig.calls.is_empty(), "the PTT verb was never even asked");
        // …and a refusal by the engine itself (transmit off) is the same.
        let (hold, mut rig) = (
            PttHold::default(),
            Rig {
                refuse: true,
                ..Rig::default()
            },
        );
        hold.hold(B, t0);
        rig.tick(&hold, t0, true);
        rig.refuse = false;
        hold.hold(B, t0 + ms(100));
        rig.tick(&hold, t0 + ms(110), true);
        assert!(!rig.up);
        assert_eq!(rig.calls, vec![true], "asked once, never again");
    }

    /// Something at the station unkeyed a keyed press (a Stop, a halt): the press is over.
    #[test]
    fn a_press_unkeyed_under_it_ends_and_does_not_rekey() {
        let (hold, mut rig, t0) = (PttHold::default(), Rig::default(), Instant::now());
        hold.hold(A, t0);
        rig.tick(&hold, t0, true);
        rig.up = false; // halt_tx dropped the key
        hold.hold(A, t0 + ms(100));
        rig.tick(&hold, t0 + ms(110), true);
        assert!(!rig.up);
        hold.hold(A, t0 + ms(200));
        rig.tick(&hold, t0 + ms(210), true);
        assert!(!rig.up, "the press re-keyed after a stop");
        assert_eq!(rig.calls, vec![true]);
        assert_eq!(hold.reports().last().unwrap().end, Some(HoldEnd::Stopped));
    }

    /// Admission withdrawn while keyed (presence lapsed): the key comes down on that tick.
    #[test]
    fn losing_admission_while_keyed_unkeys() {
        let (hold, mut rig, t0) = (PttHold::default(), Rig::default(), Instant::now());
        hold.hold(A, t0);
        rig.tick(&hold, t0, true);
        hold.hold(A, t0 + ms(100));
        rig.tick(&hold, t0 + ms(110), false);
        assert!(!rig.up);
        assert_eq!(rig.calls, vec![true, false]);
    }

    #[test]
    fn the_stream_ending_unkeys_on_the_next_tick() {
        let (hold, mut rig, t0) = (PttHold::default(), Rig::default(), Instant::now());
        hold.hold(A, t0);
        rig.tick(&hold, t0, true);
        hold.end();
        rig.tick(&hold, t0 + ms(20), true);
        assert!(!rig.up);
        assert_eq!(hold.reports().last().unwrap().end, Some(HoldEnd::Stopped));
    }

    /// A new press while an old one is still keyed (its release lost, or a quick re-press): the
    /// key is handed over, and from then on it stays up only while the NEW press keeps coming.
    #[test]
    fn a_new_press_over_a_keyed_one_takes_the_key_and_the_old_one_is_dead() {
        let (hold, mut rig, t0) = (PttHold::default(), Rig::default(), Instant::now());
        hold.hold(A, t0);
        rig.tick(&hold, t0, true);
        assert!(hold.hold(B, t0 + ms(50)));
        rig.tick(&hold, t0 + ms(60), true);
        assert!(rig.up, "the handover dropped the key");
        // The old press is dead: its late holds neither extend nor re-key.
        assert!(!hold.hold(A, t0 + ms(150)));
        // The new press stops coming: down one gap after ITS last hold.
        rig.tick(&hold, t0 + ms(249), true);
        assert!(rig.up);
        rig.tick(&hold, t0 + ms(250), true);
        assert!(!rig.up, "the key outlived the press that held it");
        assert_eq!(
            rig.calls,
            vec![true, false],
            "one key, one unkey, no glitch"
        );
    }

    /// After a Stop at the station ends a press, nothing takes the key over until that stop has
    /// been carried out.
    #[test]
    fn a_stopped_press_cannot_be_taken_over() {
        let (hold, mut rig, t0) = (PttHold::default(), Rig::default(), Instant::now());
        hold.hold(A, t0);
        rig.tick(&hold, t0, true);
        hold.end();
        assert!(
            !hold.hold(B, t0 + ms(10)),
            "a new press took over a stopped key"
        );
        rig.tick(&hold, t0 + ms(20), true);
        assert!(!rig.up);
    }
}
