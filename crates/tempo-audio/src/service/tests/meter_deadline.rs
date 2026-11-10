//! ⛔ A CHARACTERIZATION PIN, NOT A SPEC: the transmit meter read's deadline policy as it stands on
//! 2026-11-10, pinned on the real `RadioLoop::step` so the refactor that moves it out of the loop
//! into one pure function (`tx_meter_read_plan`) is proved to change nothing. Every value below is
//! what the loop does today, measured, not what it ought to do. A row that looks wrong is a
//! FINDING to raise with the maintainer, not something to fix here: change the policy in its own
//! change, with its own test, and only then move the row.
//!
//! The policy (service.rs, "AN OVER'S READ ENDS BY THE OVER'S NEXT END", and the constants above
//! [`TX_METER_OVER_END_SLACK_MS`]):
//!
//! - an over's read is asked only with at least [`TX_METER_OVER_MIN_LEFT_MS`] (150 ms) left to the
//!   over's next end, [`TX_METER_OVER_MIN_LEFT_SLOW_MS`] (600 ms) on a slow link;
//! - its deadline is the time left less [`TX_METER_OVER_END_SLACK_MS`] (20 ms), capped at the
//!   link's own deadline (`Rig::transport_deadline_ms`: 700 ms, 2.5 s on a slow link);
//! - a tune's read has [`TUNE_METER_DEADLINE_MS`] (120 ms);
//! - a hold with no end (live PTT, a radio keyed at the radio) is asked with NO deadline, which
//!   `Rig::command` reads as the link's own (700 ms, 2.5 s slow) checked only after each 500 ms
//!   socket read: a stall there holds the loop 1000 ms on a fast link, not 700 (measured
//!   2026-11-10, and pinned so: a pure function that turned `None` into `Some(700)` would change
//!   it to 700 and must fail here);
//! - the raw ALC read of a Yaesu that answered the probe is held to [`raw_alc_over_deadline_ms`]
//!   (300 ms, the link's deadline on a slow link), and to the over's own deadline when shorter.
//!
//! Each row drives ONE meter read through the loop twice, on a rigctld that answers it at once
//! (the base) and on one that never answers it (the stall), and pins whether the read went on the
//! wire at all and, when it did, that the stalled tick held the loop for the row's deadline: at
//! least the deadline less 10 ms, at most the deadline plus the base plus 120 ms (300 ms for a
//! deadline of a second or more, several socket reads end to end). The lower bound is the safe
//! direction (a socket timeout never fires early); the upper slack is a loaded machine's timer
//! lateness. Healthy, a stalled tick holds the deadline plus a few ms (measured
//! 2026-11-10: 700 pinned held 705, 2500 held 2510, 120 held 122, base 0-1 ms; the slow no-deadline
//! read, five socket timeouts end to end, held 2578, its slack the thinnest); a policy change shows
//! as hundreds of ms either way (300 against 700, 1000 against 700, 2500 against 700) or as a read
//! asked where none was.
//!
//! The over rows are an FT8 CQ keyed by the engine on its own boundary, the scene of
//! `ft_window::a_stalled_meter_read_near_an_overs_end_never_holds_the_loop_past_it`: its next end is
//! the audio stop at 13.64 s. The scene runs at a synthetic time 30 s ahead of the real clock.
use super::*;
use crate::rig::remote_tests::{retuning_peer, Peer};
use std::sync::atomic::{AtomicBool, Ordering};

/// What keys the transmitter in a row.
#[derive(Clone, Copy, Debug)]
enum Hold {
    /// An FT8 CQ over, read on the tick this many ms into its slot.
    Over(f64),
    /// The Tune button.
    Tune,
    /// Live PTT (the operator's PTT button): a hold with no end of its own.
    LivePtt,
    /// The radio keyed at the radio (its mic), seen by the loop's PTT poll: no end either.
    RadioKeyed,
}

/// One row's scene, run up to the tick that reads a meter. `slow` marks the link a slow
/// transport; `raw` sets the radio to an FT-991A (Hamlib 1035) whose link answered the raw ALC
/// probe, and reads the ALC slot.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    peer: Peer,
    t: f64,
}

impl Scene {
    fn new(
        hold: Hold,
        slow: bool,
        raw: bool,
        stall: &Arc<AtomicBool>,
        mic: &Arc<AtomicBool>,
    ) -> Scene {
        let (stall2, mic2) = (Arc::clone(stall), Arc::clone(mic));
        // Every meter read (`l NAME`, and the raw `w RM4;`) is answered at once, or not at all
        // while `stall` is set; the connection goes on serving meanwhile. `t` answers 1 while
        // `mic` is set (the radio's own mic key).
        let peer = retuning_peer(14_074_000, "PKTUSB", move |line, state| {
            let meter = line.starts_with("l ") || line.starts_with("w ");
            if meter && stall2.load(Ordering::SeqCst) {
                return Some(String::new());
            }
            if line.starts_with("w RM4;") {
                return Some("RM4064;\0".to_string());
            }
            if line.starts_with("l ") {
                return Some("0.500000\n".to_string());
            }
            if line == "t" && mic2.load(Ordering::SeqCst) {
                return Some("1\n".to_string());
            }
            (line == "T 1").then(|| {
                state.keyed = true;
                "RPRT 0\n".to_string()
            })
        });
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        {
            let mut e = engine.lock().unwrap();
            if raw {
                let mut s = e.settings().clone();
                s.rig_model = 1035;
                e.apply_settings(s);
            }
            e.set_license_class("extra");
            e.set_tier(Tier::Ft8);
            e.set_frequency(14.074, "20m", "USB");
        }
        let mut rig = Rig::rigctld(&peer.address);
        rig.set_slow_transport(slow);
        // An even FT8 boundary at least 30 s ahead of the real clock.
        let t = ((now_unix_ms() + 30_000.0) / 30_000.0).ceil() * 30_000.0;
        let mut s = Scene {
            state: loop_state_for(&engine),
            engine,
            backend: MockBackend::new(),
            rig,
            peer,
            t,
        };
        let t0 = s.t;
        match hold {
            Hold::Over(at) => {
                {
                    let mut e = s.engine.lock().unwrap();
                    e.broadcast("CQ KD9TAW EN52");
                    let _ = e.take_immediate_tx();
                }
                s.run_to(t0 + at - 20.0);
                assert!(
                    s.state.tx_until_ms.is_some(),
                    "{hold:?}: premise: the over is on"
                );
            }
            Hold::Tune => {
                {
                    let mut e = s.engine.lock().unwrap();
                    e.set_tx_enabled(true);
                    e.set_tune(true);
                }
                s.run_to(t0 + 400.0);
                assert!(s.state.tuning_keyed, "{hold:?}: premise: the tune is keyed");
            }
            Hold::LivePtt => {
                {
                    let mut e = s.engine.lock().unwrap();
                    e.set_tx_enabled(true);
                    e.set_ptt(true);
                }
                s.run_to(t0 + 400.0);
                assert!(
                    s.state.manual_ptt_applied && s.state.tx_until_ms.is_none(),
                    "{hold:?}: premise: live PTT held, with no end"
                );
            }
            Hold::RadioKeyed => {
                mic.store(true, Ordering::SeqCst);
                s.run_to(t0 + 2_400.0);
                assert!(
                    s.state.rig_keyed && s.state.tx_until_ms.is_none(),
                    "{hold:?}: premise: the loop sees the radio keyed, and holds nothing itself"
                );
            }
        }
        // The next tick reads one meter, from scratch: due, none parked, and the slot chosen.
        s.state.tx_meter_supported = [None; N_TX_METERS];
        s.state.tx_meter_misses = [0; N_TX_METERS];
        s.state.last_tx_meter_poll = s.t - 1_000.0;
        if raw {
            s.state.tx_meter_idx = 1; // ALC
            s.state.tx_alc_raw = Some(true);
        } else {
            s.state.tx_meter_idx = 0; // SWR
        }
        s
    }

    fn run_to(&mut self, until: f64) {
        step_to(
            &self.engine,
            &mut self.state,
            &mut self.backend,
            &mut self.rig,
            &mut self.t,
            until,
        );
    }

    /// Every meter line on the wire so far.
    fn meter_lines(&self) -> Vec<String> {
        let lines = self.peer.lines.lock().unwrap();
        lines
            .iter()
            .filter(|l| l.starts_with("l ") || l.starts_with("w "))
            .cloned()
            .collect()
    }
}

/// One row, the radio stalling on its read or answering it at once: how long the reading tick
/// held the loop (ms), and the meter lines it put on the wire.
fn tick(hold: Hold, slow: bool, raw: bool, stalled: bool) -> (f64, Vec<String>) {
    let (stall, mic) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let mut s = Scene::new(hold, slow, raw, &stall, &mic);
    let before = s.meter_lines().len();
    stall.store(stalled, Ordering::SeqCst);
    let started = Instant::now();
    let next = s.t;
    s.run_to(next); // exactly one tick
    let held = started.elapsed().as_secs_f64() * 1_000.0;
    (held, s.meter_lines()[before..].to_vec())
}

/// ⭐ THE TRANSMIT METER READ'S DEADLINE, ROW BY ROW, ON THE REAL LOOP (characterization pin; see
/// the module header). `None` is no read asked at all; `Some(ms)` is a read asked, which a radio
/// that never answers it holds the tick for `ms`.
#[test]
fn meter_deadline_policy_is_pinned_row_by_row() {
    // (row, hold, slow link, raw ALC read, the line asked, the deadline pinned)
    let rows = [
        // TUNE_METER_DEADLINE_MS.
        ("tune", Hold::Tune, false, false, "l SWR", Some(120.0)),
        // 2640 ms left: capped at the fast link's 700.
        (
            "FT8 far from its end, fast",
            Hold::Over(11_000.0),
            false,
            false,
            "l SWR",
            Some(700.0),
        ),
        // 2640 ms left: capped at the slow link's 2500 (2620 is more).
        (
            "FT8 far from its end, slow",
            Hold::Over(11_000.0),
            true,
            false,
            "l SWR",
            Some(2_500.0),
        ),
        // 340 ms left: 340 - 20.
        (
            "FT8 340 ms left, fast",
            Hold::Over(13_300.0),
            false,
            false,
            "l SWR",
            Some(320.0),
        ),
        // 80 ms left: under 150, no read.
        (
            "FT8 80 ms left, fast",
            Hold::Over(13_560.0),
            false,
            false,
            "l SWR",
            None,
        ),
        // 340 ms left: under the slow link's 600, no read.
        (
            "FT8 340 ms left, slow",
            Hold::Over(13_300.0),
            true,
            false,
            "l SWR",
            None,
        ),
        // 740 ms left: over 600 on the slow link, so read, bounded at 740 - 20.
        (
            "FT8 740 ms left, slow",
            Hold::Over(12_900.0),
            true,
            false,
            "l SWR",
            Some(720.0),
        ),
        // Raw ALC far from the end: RAW_ALC_OVER_DEADLINE_MS under the over's 700.
        (
            "raw ALC far from its end, fast",
            Hold::Over(11_000.0),
            false,
            true,
            "w RM4;",
            Some(300.0),
        ),
        // Raw ALC on a slow link: the transport's 2500 (raw_alc_over_deadline_ms).
        (
            "raw ALC far from its end, slow",
            Hold::Over(11_000.0),
            true,
            true,
            "w RM4;",
            Some(2_500.0),
        ),
        // Raw ALC with 180 ms left: the over's 160 under the raw read's 300. Far enough under it
        // that a strict bound which lost the over's own (~325 ms held) fails the upper bound
        // (160 + base + 120); healthy it holds ~160-210 (the miss drops the connection, and the
        // rest of the tick reconnects).
        (
            "raw ALC 180 ms left, fast",
            Hold::Over(13_460.0),
            false,
            true,
            "w RM4;",
            Some(160.0),
        ),
        // A hold with no end: no deadline of its own, so `Rig::command`'s, the link's 700 ms
        // checked after each 500 ms socket read: two reads, 1000 ms (see the module header).
        (
            "live PTT, fast",
            Hold::LivePtt,
            false,
            false,
            "l SWR",
            Some(1_000.0),
        ),
        (
            "radio keyed at the radio, fast",
            Hold::RadioKeyed,
            false,
            false,
            "l SWR",
            Some(1_000.0),
        ),
        // …and on a slow link its 2500, five whole reads.
        (
            "live PTT, slow",
            Hold::LivePtt,
            true,
            false,
            "l SWR",
            Some(2_500.0),
        ),
    ];
    let mut failed = Vec::new();
    for (row, hold, slow, raw, line, want) in rows {
        let (base, base_lines) = tick(hold, slow, raw, false);
        let (held, lines) = tick(hold, slow, raw, true);
        eprintln!("{row}: held {held:.0} ms, base {base:.0} ms, asked {lines:?}");
        let asked = want.map(|_| vec![line.to_string()]).unwrap_or_default();
        if base_lines != asked || lines != asked {
            failed.push(format!(
                "{row}: asked {lines:?} stalled, {base_lines:?} answered, expected {asked:?}"
            ));
        }
        if let Some(deadline) = want {
            // A deadline of a second or more is several socket reads end to end, each one late
            // by the kernel timer's slack, so it gets more room: the slow no-deadline row held
            // 2578 ms against 2500 healthy; a policy change moves a row by hundreds of ms.
            let slack = if deadline >= 1_000.0 { 300.0 } else { 120.0 };
            if held < deadline - 10.0 || held > deadline + base + slack {
                failed.push(format!(
                    "{row}: the stalled read held the tick {held:.0} ms, pinned {deadline} ms \
                     ({base:.0} ms answered)"
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}
