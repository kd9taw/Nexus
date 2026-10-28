//! The computer microphone at the transmitter (`tempo_app::engine::pc_mic`, the radio loop's block
//! beside the Remote microphone's): the real `RadioLoop::step`, a VOX rig and a mock sound card whose
//! microphone delivers 20 ms of audio a tick while it is open.
//!
//! What is held here: the microphone is open only while the operator's PTT is held; nothing keys
//! before its audio; its audio reaches the transmitter only between the press and the release; and
//! every stop (the release, Stop TX, TX Off, a lost microphone, leaving Phone, a watchdog trip)
//! unkeys, flushes and closes the microphone within ONE tick. Off (the default) the press keys the
//! radio's own microphone exactly as before, and the computer's microphone is never opened.

use super::*;
use tempo_app::mic::PC_MIC;

const DEVICE: &str = "Desk Microphone";

struct PcScene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    origin: Instant,
}

impl PcScene {
    /// Phone, TX armed, the computer's microphone switched on (`on`) for the radio.
    fn new(on: bool) -> Self {
        let origin = Instant::now();
        let settings = Settings {
            pc_mic: on,
            pc_mic_device: DEVICE.into(),
            ..Settings::default()
        };
        let engine = Arc::new(Mutex::new(Engine::with_settings(settings)));
        engine.lock().unwrap().set_operating_mode("phone", false);
        let mut state = loop_state();
        state.mono_origin = Some(origin);
        Self {
            engine,
            state,
            backend: MockBackend::new(),
            rig: Rig::vox(),
            origin,
        }
    }

    /// One loop tick at `t` ms. While the microphone is open it delivers 20 ms of `level` first.
    fn tick(&mut self, t: f64, level: f32) {
        if self.backend.pc_mic_open {
            self.backend
                .queue_pc_mic(vec![level; PC_MIC.samples(20) as usize]);
        }
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        let _ = self.origin;
        self.state
            .step(
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
    }

    fn ptt(&mut self, on: bool) {
        self.engine.lock().unwrap().set_ptt(on);
    }

    /// Press at 0 ms and run to `until`: a keyed over, fed.
    fn keyed(until: f64) -> Self {
        let mut s = Self::new(true);
        s.ptt(true);
        let mut t = 0.0;
        while t <= until {
            s.tick(t, 0.5);
            t += 20.0;
        }
        assert!(s.rig.keyed, "premise: the microphone's audio keyed the rig");
        assert!(
            s.backend.played.contains(&0.5),
            "premise: the operator's voice reached the transmitter"
        );
        s
    }
}

#[test]
fn the_microphone_is_open_and_on_the_air_only_while_ptt_is_held() {
    let mut s = PcScene::new(true);
    // No press: the microphone is never opened and nothing keys.
    for k in 0..20 {
        s.tick(k as f64 * 20.0, 0.5);
    }
    assert!(s.backend.pc_mic_calls.is_empty(), "opened with no press");
    assert!(!s.rig.keyed && s.backend.played.is_empty());
    // The press opens the named microphone; its first audio keys the rig, then the voice plays.
    s.ptt(true);
    let mut t = 400.0;
    s.tick(t, 0.5);
    assert_eq!(s.backend.pc_mic_calls, vec![Some(DEVICE.to_string())]);
    for _ in 0..20 {
        t += 20.0;
        s.tick(t, 0.5);
    }
    assert!(s.rig.keyed, "the microphone's audio did not key the rig");
    assert!(
        s.backend.played.contains(&0.5),
        "no voice reached the transmitter"
    );
    // Nothing on the air comes back to the operator: the headphone monitor is muted for the over.
    assert!(
        s.backend.monitor_tx_muted,
        "the headphone monitor plays on under the microphone"
    );
    // The release: ONE tick later the rig is unkeyed, the queue flushed and the microphone closed.
    s.ptt(false);
    let flushes = s.backend.flush_calls;
    let played = s.backend.played.len();
    t += 20.0;
    s.tick(t, 0.5);
    assert!(!s.rig.keyed, "PTT stayed up after the release");
    assert!(
        s.backend.flush_calls > flushes,
        "the queued voice was not flushed"
    );
    assert!(
        !s.backend.pc_mic_open,
        "the microphone stayed open after the release"
    );
    assert_eq!(
        s.backend.played.len(),
        played,
        "audio played after the release"
    );
    assert!(s.state.tx_until_ms.is_none());
    assert!(
        !s.backend.monitor_tx_muted,
        "the monitor stayed muted after the release"
    );
}

#[test]
fn every_stop_unkeys_flushes_and_closes_the_microphone_within_a_tick() {
    type Stop = fn(&mut PcScene);
    let stops: [(&str, Stop); 6] = [
        ("PTT release", |s| s.ptt(false)),
        ("Stop TX", |s| s.engine.lock().unwrap().halt_tx()),
        ("TX Off", |s| s.engine.lock().unwrap().set_tx_enabled(false)),
        ("the microphone went away", |s| {
            s.backend.pc_mic_lost = Some("device unplugged".into())
        }),
        ("leaving Phone for FT8", |s| {
            s.engine
                .lock()
                .unwrap()
                .set_operating_mode("digital", false);
        }),
        ("a watchdog trip", |s| {
            s.engine.lock().unwrap().trip_tx_watchdog_for_test()
        }),
    ];
    for (name, stop) in stops {
        let mut s = PcScene::keyed(600.0);
        let flushes = s.backend.flush_calls;
        stop(&mut s);
        let played = s.backend.played.len();
        s.tick(620.0, 0.5);
        assert!(!s.rig.keyed, "{name}: the rig stayed keyed");
        assert!(
            s.backend.flush_calls > flushes,
            "{name}: nothing was flushed"
        );
        assert!(!s.backend.pc_mic_open, "{name}: the microphone stayed open");
        assert_eq!(s.backend.played.len(), played, "{name}: audio played on");
        // …and nothing keys again while the operator's press is still "held" in the cockpit.
        for k in 1..20 {
            s.tick(620.0 + k as f64 * 20.0, 0.5);
            assert!(!s.rig.keyed, "{name}: re-keyed by itself");
        }
    }
}

#[test]
fn a_microphone_that_will_not_open_keys_nothing() {
    let mut s = PcScene::new(true);
    s.backend.pc_mic_fail = true;
    s.ptt(true);
    for k in 0..20 {
        s.tick(k as f64 * 20.0, 0.5);
    }
    assert!(!s.rig.keyed && s.backend.played.is_empty());
    let e = s.engine.lock().unwrap();
    assert!(
        !e.pc_mic_armed(),
        "the over stayed armed on a dead microphone"
    );
    assert_eq!(
        e.snapshot().radio.pc_mic_ended.map(|x| x.cause),
        Some(tempo_app::dto::PcMicCause::OpenFailed)
    );
}

#[test]
fn ft_never_opens_the_microphone() {
    let mut s = PcScene::new(true);
    s.engine
        .lock()
        .unwrap()
        .set_operating_mode("digital", false);
    s.ptt(true);
    for k in 0..20 {
        s.tick(k as f64 * 20.0, 0.5);
    }
    assert!(
        s.backend.pc_mic_calls.is_empty(),
        "FT opened the microphone"
    );
    assert!(
        !s.backend.played.contains(&0.5),
        "the microphone reached an FT over"
    );
}

#[test]
fn off_by_default_the_press_keys_the_radios_own_microphone_as_before() {
    let mut s = PcScene::new(false);
    s.ptt(true);
    for k in 0..10 {
        s.tick(k as f64 * 20.0, 0.5);
    }
    assert!(s.rig.keyed, "the press no longer keys the radio");
    assert!(
        s.backend.pc_mic_calls.is_empty(),
        "the computer microphone was opened"
    );
    assert!(
        s.backend.played.is_empty(),
        "audio was played to a radio on its own microphone"
    );
    s.ptt(false);
    s.tick(220.0, 0.5);
    assert!(!s.rig.keyed);
}
