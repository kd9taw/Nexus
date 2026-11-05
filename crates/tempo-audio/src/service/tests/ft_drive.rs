//! The FT drive remembered per band and mode (`tempo_app` `engine/ft_drive.rs`), on the real
//! `RadioLoop::step`: a band change made while an FT8 over is on the air brings the new band's
//! drive back only once the loop has unkeyed, never inside the over.
//!
//! The rig is a rigctld that keys and unkeys as asked; the loop's clock runs on the real clock's
//! timebase, a little ahead of it, because an over's hold is measured on the real clock.

use super::*;
use crate::rig::remote_tests::{retuning_peer, Peer};
use std::sync::atomic::{AtomicBool, Ordering};

struct Station {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    peer: Peer,
    /// The radio's own PTT, as the rigctld holds it.
    on_air: Arc<AtomicBool>,
    /// The start of the slot the first over is keyed in (ms).
    slot: f64,
    t: f64,
}

impl Station {
    /// A station calling CQ in FT8 on 20 m at drive 0.30, whose operator last drove 6 m FT8 at
    /// 0.80: both picks made with the drive slider, as the cockpit makes them.
    fn new() -> Station {
        Station::with(
            |_| {},
            |e| {
                e.set_frequency(50.313, "6m", "USB");
                e.set_tx_level(0.80);
                e.set_frequency(14.074, "20m", "USB");
                e.set_tx_level(0.30);
                e.start_cq(None).expect("Call CQ");
                // The snappy first over is drained, so the slot boundary keys it.
                let _ = e.take_immediate_tx();
            },
        )
    }

    /// The station in FT8 with `settings` as its file loads them, then `setup` done on the engine
    /// before the loop's first tick.
    fn with(configure: impl FnOnce(&mut Settings), setup: impl FnOnce(&mut Engine)) -> Station {
        let on_air = Arc::new(AtomicBool::new(false));
        let ptt = Arc::clone(&on_air);
        let peer = retuning_peer(14_074_000, "PKTUSB", move |line, radio| {
            match line {
                "T 1" | "T 3" => radio.keyed = true,
                "T 0" => radio.keyed = false,
                _ => return None,
            }
            ptt.store(radio.keyed, Ordering::SeqCst);
            Some("RPRT 0\n".into())
        });
        // The station's radio has its profile, as every settings file loads with one.
        let mut settings = Settings {
            mycall: "KD9TAW".into(),
            mygrid: "EN52".into(),
            tx_even: true,
            ..Settings::default()
        };
        settings.ensure_radio_profiles();
        configure(&mut settings);
        let engine = Arc::new(Mutex::new(Engine::with_settings(settings)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.set_tier(Tier::Ft8);
            setup(&mut e);
        }
        let slot = ((now_unix_ms() + 10_000.0) / 30_000.0).ceil() * 30_000.0;
        Station {
            engine,
            state: loop_state(),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&peer.address),
            peer,
            on_air,
            slot,
            t: slot + 100.0,
        }
    }

    /// One 20 ms tick of the loop.
    fn tick(&mut self) {
        let sinks = Sinks {
            wsjtx: None,
            psk: None,
        };
        let (mut ra, mut rr) = (mock_reopen_audio(), mock_reopen_rig());
        self.state
            .step(
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                &sinks,
                self.t,
                &mut ra,
                &mut rr,
                &mut StationSinks::new(),
            )
            .unwrap();
        self.t += 20.0;
    }

    fn keys(&self) -> usize {
        let lines = self.peer.lines.lock().unwrap();
        lines.iter().filter(|l| *l == "T 1" || *l == "T 3").count()
    }

    fn drive(&self) -> f32 {
        self.engine.lock().unwrap().settings().tx_level
    }
}

/// ⛔ A BAND CHANGE MID-OVER NEVER CHANGES THE DRIVE INSIDE THE OVER. The 6 m drive reaches the
/// audio only on a tick that began with the loop unkeyed and the radio's PTT down, and it does
/// reach it then.
#[test]
fn a_band_change_mid_over_brings_the_drive_back_only_after_the_unkey() {
    let mut s = Station::new();
    while s.t <= s.slot + 300.0 {
        s.tick();
    }
    assert_eq!(s.keys(), 1, "premise: the slot boundary keyed the over");
    assert!(s.state.tx_until_ms.is_some(), "premise: the over holds PTT");
    assert!((s.drive() - 0.30).abs() < 1e-6, "premise: 20 m's drive");

    // The operator clicks a 6 m spot while the over is on the air.
    s.engine.lock().unwrap().set_frequency(50.313, "6m", "USB");
    let mut applied_at = None;
    while s.t <= s.slot + 20_000.0 && applied_at.is_none() {
        let keyed_before = s.state.tx_until_ms.is_some() || s.on_air.load(Ordering::SeqCst);
        let calls = s.backend.tx_level_calls.len();
        s.tick();
        let restored = s.backend.tx_level_calls[calls..]
            .iter()
            .any(|l| (l - 0.80).abs() < 1e-6);
        if restored {
            assert!(
                !keyed_before,
                "the 6 m drive reached the audio on a tick that began keyed (t = {})",
                s.t
            );
            applied_at = Some(s.t);
        } else if s.state.tx_until_ms.is_some() {
            assert!(
                (s.drive() - 0.30).abs() < 1e-6,
                "the drive changed while the loop still held the key"
            );
        }
    }
    assert!(
        applied_at.is_some(),
        "the 6 m drive never came back after the unkey (drive {}, calls {:?}, keyed {:?}, band {})",
        s.drive(),
        s.backend.tx_level_calls,
        s.state.tx_until_ms,
        s.engine.lock().unwrap().settings().band
    );
    assert!(
        (s.drive() - 0.80).abs() < 1e-6,
        "6 m's drive is the live one"
    );
}

/// ⛔ THE RESTORE RUNS BEFORE THE TICK READS THE DRIVE. Nexus launches on 20 m FT8 with a stale
/// drive (0.30) beside the 20 m FT8 drive the operator last set (0.80), and its first tick is the
/// one that keys the first over. The 0.80 must reach the audio before the over's first sample and
/// the level must not change again inside the over. Restored after the tick has read the drive,
/// the over would start at 0.30 and switch to 0.80 partway through. Tried with the first tick at
/// several points of the slot's first half second.
#[test]
fn a_restore_on_the_keying_tick_lands_before_the_overs_first_sample() {
    for first_tick in [0.0, 20.0, 100.0, 500.0] {
        let mut s = Station::with(
            |st| {
                st.tx_level = 0.30;
                st.radios[0].tx_level = 0.30;
                st.radios[0].remember_ft_drive("20m", "FT8", 0.80);
            },
            |e| {
                e.set_frequency(14.074, "20m", "USB");
                e.start_cq(None).expect("Call CQ");
                let _ = e.take_immediate_tx();
            },
        );
        s.t = s.slot + first_tick;
        for _ in 0..300 {
            s.tick();
        }
        let at = &s.backend.tx_level_at;
        assert_eq!(s.keys(), 1, "{first_tick}: premise: the over keyed");
        assert!(
            !s.backend.played.is_empty(),
            "{first_tick}: premise: the over played"
        );
        assert!(
            at.iter().any(|(l, _)| (l - 0.80).abs() < 1e-6),
            "{first_tick}: the 20 m drive was never restored: {at:?}"
        );
        assert!(
            at.iter().all(|&(_, played)| played == 0),
            "{first_tick}: the drive changed after the over's audio was queued: {at:?}"
        );
    }
}

/// ⛔ THE LOOP'S OWN WORD THAT IT HOLDS THE KEY IS ENOUGH. A key the loop holds on a deadline of
/// its own (`tx_until_ms`: an APRS frame or a voice message it plays) is told to the engine only
/// after the tick that keyed it, so on that tick the engine sees nothing on the air. The loop's
/// argument alone must hold the restore off then. CONTROL: the same band change with nothing held
/// is restored on the first tick.
#[test]
fn a_key_only_the_loop_holds_keeps_the_drive() {
    let idle = || {
        Station::with(
            |_| {},
            |e| {
                e.set_frequency(50.313, "6m", "USB");
                e.set_tx_level(0.80);
                e.set_frequency(14.074, "20m", "USB");
                e.set_tx_level(0.30);
            },
        )
    };
    // CONTROL: nothing held.
    let mut s = idle();
    s.tick();
    s.engine.lock().unwrap().set_frequency(50.313, "6m", "USB");
    s.tick();
    assert!(
        (s.drive() - 0.80).abs() < 1e-6,
        "control: an idle band change brings 6 m's drive back"
    );

    let mut s = idle();
    s.tick();
    s.engine.lock().unwrap().set_frequency(50.313, "6m", "USB");
    // The loop holds the key on its own deadline; the engine has not been told.
    s.state.tx_until_ms = Some(s.t + 5_000.0);
    assert!(
        !s.engine.lock().unwrap().on_air(),
        "premise: the engine sees nothing on the air"
    );
    s.tick();
    assert!(
        (s.drive() - 0.30).abs() < 1e-6,
        "the drive changed under a key the loop held"
    );
    assert!(
        s.backend
            .tx_level_calls
            .iter()
            .all(|l| (l - 0.80).abs() > 1e-6),
        "the 6 m drive reached the audio under the key: {:?}",
        s.backend.tx_level_calls
    );
}
