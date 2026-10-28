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
use std::sync::atomic::{AtomicBool, Ordering};
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
        Self::keyed_from(Self::new(true), until)
    }

    /// [`Self::keyed`] from a scene already set up (another rig, an event log).
    fn keyed_from(mut s: Self, until: f64) -> Self {
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

/// The stops of a keyed over, each applied between two loop ticks.
type Stop = fn(&mut PcScene);
const STOPS: [(&str, Stop); 7] = [
    ("PTT release", |s| s.ptt(false)),
    ("Stop TX", |s| s.engine.lock().unwrap().halt_tx()),
    ("TX Off", |s| s.engine.lock().unwrap().set_tx_enabled(false)),
    ("the microphone went away", |s| {
        s.backend.pc_mic_lost = Some("device unplugged".into())
    }),
    ("a radio handoff", |s| s.state.handoff_deferred = true),
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

/// ⛔ UNKEY FIRST, THEN CLOSE THE MICROPHONE. Closing it takes the audio host lock (a device
/// enumeration on another thread can hold it) and drops a stream (slow on a device that just went
/// away), so it must never sit between a stop and the unkey. A CAT-keyed radio and the mock sound
/// card write into ONE log: for every stop, the radio's `T 0` and the flush come before the
/// microphone's close.
#[test]
fn every_stop_unkeys_before_the_microphone_is_closed() {
    for (name, stop) in STOPS {
        let (addr, log) = mock_rigctld_ptt(14_200_000, false, "0\n");
        let mut s = PcScene::new(true);
        s.rig = Rig::rigctld(&addr);
        s.backend.events = Some(Arc::clone(&log));
        let mut s = PcScene::keyed_from(s, 600.0);
        log.lock().unwrap().clear();
        stop(&mut s);
        s.tick(620.0, 0.5);
        let log = log.lock().unwrap().clone();
        let at = |what: &str| log.iter().position(|l| l == what);
        let unkey = at("T 0").unwrap_or_else(|| panic!("{name}: never unkeyed: {log:?}"));
        let flush = at("flush").unwrap_or_else(|| panic!("{name}: never flushed: {log:?}"));
        let close = at("pc_mic close")
            .unwrap_or_else(|| panic!("{name}: the microphone was not closed: {log:?}"));
        assert!(
            unkey < close && flush < close,
            "{name}: the microphone closed before the unkey: {log:?}"
        );
        assert!(!s.rig.keyed, "{name}: the rig stayed keyed");
    }
}

/// A release and a new press inside one loop tick: the old over is unkeyed and flushed, the
/// microphone is closed and opened afresh for the new one, nothing captured before the press goes
/// out, and the new over then keys on its own audio. It used to die silently: the abort ate the new
/// over's key and the next tick dropped it with nothing in the lane.
#[test]
fn a_quick_re_press_starts_a_fresh_over() {
    let mut s = PcScene::keyed(600.0);
    let opens = s.backend.pc_mic_calls.len();
    let flushes = s.backend.flush_calls;
    s.ptt(false);
    s.ptt(true);
    // This tick's delivery was captured before the press (0.9): it must never reach the air.
    s.tick(620.0, 0.9);
    assert!(
        s.backend.flush_calls > flushes,
        "the old over was not flushed"
    );
    assert!(!s.rig.keyed, "the old over was not unkeyed");
    assert_eq!(
        s.backend.pc_mic_calls[opens..],
        [None, Some(DEVICE.to_string())],
        "the microphone was not opened afresh for the new over"
    );
    let mut t = 620.0;
    for _ in 0..20 {
        t += 20.0;
        s.tick(t, 0.5);
    }
    assert!(s.rig.keyed, "the new over never keyed");
    assert!(
        !s.backend.played.contains(&0.9),
        "audio from before the press went out"
    );
    let e = s.engine.lock().unwrap();
    assert!(e.pc_mic_keyed(), "the new over was dropped");
    assert_eq!(e.snapshot().radio.pc_mic_ended, None);
}

/// ⛔ The input the sound card opened for receive (here the system default, none being set) is
/// never opened as the microphone: the over ends before the open, keys nothing, and says why.
/// CONTROL: a different receive input opens it.
#[test]
fn the_microphone_is_never_the_input_receive_opened() {
    let mut s = PcScene::new(true);
    s.backend.capture_input = Some(crate::receive_audio::CaptureInput {
        device: "desk microphone ".into(),
        system_default: true,
    });
    s.ptt(true);
    for k in 0..20 {
        s.tick(k as f64 * 20.0, 0.5);
    }
    assert!(
        s.backend.pc_mic_calls.is_empty(),
        "the receive input was opened as the microphone"
    );
    assert!(!s.rig.keyed && s.backend.played.is_empty());
    assert_eq!(
        s.engine
            .lock()
            .unwrap()
            .snapshot()
            .radio
            .pc_mic_ended
            .map(|x| x.cause),
        Some(tempo_app::dto::PcMicCause::SameAsReceive)
    );
    let mut s = PcScene::new(true);
    s.backend.capture_input = Some(crate::receive_audio::CaptureInput {
        device: "Radio Codec".into(),
        system_default: false,
    });
    s.ptt(true);
    s.tick(0.0, 0.5);
    assert_eq!(
        s.backend.pc_mic_calls,
        vec![Some(DEVICE.to_string())],
        "CONTROL: another receive input"
    );
}

/// A handoff (the rig not the operator's to key this tick) under a keyed over unkeys and says so.
#[test]
fn an_interrupted_over_is_named_in_the_lane() {
    let mut s = PcScene::keyed(600.0);
    s.state.handoff_deferred = true;
    s.tick(620.0, 0.5);
    assert!(!s.rig.keyed);
    assert_eq!(
        s.engine
            .lock()
            .unwrap()
            .snapshot()
            .radio
            .pc_mic_ended
            .map(|x| x.cause),
        Some(tempo_app::dto::PcMicCause::Interrupted)
    );
}

/// A CAT radio that runs `stop` on the engine ONCE, while the loop waits on its answer to the
/// over's key (`T 1`): a stop that lands inside the keying tick, as a short PTT tap or a Stop TX
/// clicked as the key goes out does. The radio and the mock sound card share one event log.
fn stop_during_the_key(
    stop: fn(&mut Engine),
) -> (PcScene, Arc<Mutex<Vec<String>>>, Arc<AtomicBool>) {
    let s = PcScene::new(true);
    let engine = Arc::clone(&s.engine);
    let fired = Arc::new(AtomicBool::new(false));
    let fired2 = Arc::clone(&fired);
    let (addr, log) = mock_rigctld_hooked(14_200_000, false, "0\n", move |line| {
        if line != "T 1" || fired2.swap(true, Ordering::SeqCst) {
            return;
        }
        // The loop holds no engine lock across the key; a bounded wait keeps a regression from
        // hanging the suite.
        let give_up = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(mut e) = engine.try_lock() {
                stop(&mut e);
                return;
            }
            assert!(
                Instant::now() < give_up,
                "the engine was locked across the key"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    let mut s = s;
    s.rig = Rig::rigctld(&addr);
    s.backend.events = Some(Arc::clone(&log));
    (s, log, fired)
}

impl PcScene {
    /// Press, and tick (the microphone delivering `level`) until the radio has been asked to key.
    fn press_until_keyed(&mut self, fired: &AtomicBool, level: f32) -> f64 {
        self.ptt(true);
        let mut t = 0.0;
        while !fired.load(Ordering::SeqCst) {
            assert!(
                t < 1_000.0,
                "premise: the over never asked the radio to key"
            );
            self.tick(t, level);
            t += 20.0;
        }
        t
    }
}

/// ⛔ UNKEY FIRST, THEN CLOSE THE MICROPHONE, for a stop that lands INSIDE a tick (the release,
/// Stop TX or TX Off while the radio is still answering the key). The keying tick leaves the
/// microphone alone, and the next tick sends `T 0` and flushes before it closes the microphone: a
/// close that blocks (the audio host lock held by a device listing, a stream slow to drop) can
/// only ever come after the unkey. It used to close the microphone on the keying tick, with the
/// radio keyed, and unkey a tick later.
#[test]
fn a_stop_during_the_key_unkeys_before_the_microphone_is_closed() {
    type EngineStop = fn(&mut Engine);
    let stops: [(&str, EngineStop); 3] = [
        ("PTT release", |e| e.set_ptt(false)),
        ("Stop TX", |e| e.halt_tx()),
        ("TX Off", |e| e.set_tx_enabled(false)),
    ];
    for (name, stop) in stops {
        let (mut s, log, fired) = stop_during_the_key(stop);
        let mut t = s.press_until_keyed(&fired, 0.5);
        // Stop TX's own path may already have unkeyed later in that tick; what may never happen
        // is a closed microphone under a keyed radio.
        assert!(
            !s.rig.keyed || s.backend.pc_mic_open,
            "{name}: the microphone was closed with the radio keyed"
        );
        s.tick(t, 0.5);
        t += 20.0;
        let events = log.lock().unwrap().clone();
        let key = events.iter().position(|l| l == "T 1").unwrap();
        let at = |what: &str| {
            events[key..]
                .iter()
                .position(|l| l == what)
                .map(|i| i + key)
                .unwrap_or_else(|| panic!("{name}: no {what:?}: {events:?}"))
        };
        let (unkey, flush, close) = (at("T 0"), at("flush"), at("pc_mic close"));
        assert!(
            unkey < close && flush < close,
            "{name}: the microphone closed before the unkey: {events:?}"
        );
        assert!(!s.rig.keyed, "{name}: the rig stayed keyed");
        assert!(!s.backend.pc_mic_open, "{name}: the microphone stayed open");
        for _ in 0..20 {
            s.tick(t, 0.5);
            t += 20.0;
            assert!(!s.rig.keyed, "{name}: re-keyed by itself");
        }
    }
}

/// A release and a new press while the radio is still answering the old over's key: the old over
/// is unkeyed and flushed on the next tick, only then is the microphone closed and opened afresh,
/// and the new over keys on its own audio with nothing in the lane. Nothing captured for the old
/// over (0.9) goes out. It used to reopen the microphone with the old over still keyed, and the
/// new over died a tick later as interrupted.
#[test]
fn a_re_press_during_the_key_starts_a_fresh_over() {
    let (mut s, log, fired) = stop_during_the_key(|e| {
        e.set_ptt(false);
        e.set_ptt(true);
    });
    let mut t = s.press_until_keyed(&fired, 0.9);
    let opens = s.backend.pc_mic_calls.len();
    assert!(
        s.backend.pc_mic_open,
        "the microphone was reopened with the old over keyed"
    );
    s.tick(t, 0.9);
    t += 20.0;
    let events = log.lock().unwrap().clone();
    let at = |what: &str| events.iter().rposition(|l| l == what);
    assert!(
        at("T 0").unwrap() < at("pc_mic close").unwrap(),
        "the old over was not unkeyed before the close: {events:?}"
    );
    assert!(!s.rig.keyed, "the old over was not unkeyed");
    assert_eq!(
        s.backend.pc_mic_calls[opens..],
        [None, Some(DEVICE.to_string())],
        "the microphone was not opened afresh for the new over"
    );
    for _ in 0..20 {
        s.tick(t, 0.5);
        t += 20.0;
    }
    assert!(s.rig.keyed, "the new over never keyed");
    assert!(
        s.backend.played.contains(&0.5),
        "the new over's voice never went out"
    );
    assert!(
        !s.backend.played.contains(&0.9),
        "the old over's audio went out"
    );
    let e = s.engine.lock().unwrap();
    assert!(e.pc_mic_keyed(), "the new over was dropped");
    assert_eq!(e.snapshot().radio.pc_mic_ended, None);
}
