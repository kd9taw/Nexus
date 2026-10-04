//! Nexus's own Flex client in the radio loop: native audio on the client's one session, the
//! transmit audio source written only at a quiet point, and the FT-timing measurement of DAX TX
//! against the sound card route (the operator signs off on the numbers before native audio can
//! become anything but opt-in).
//!
//! The radio is the SmartSDR simulator (`tempo-flexsim`) on loopback; the loop is the real
//! `RadioLoop::step`, driving the real `Rig` into the client's rigctld shim, as `run_radio` does.

use super::*;
use crate::flex::routing::FileMemory;
use crate::flex::{FlexDaemon, Options};
use tempo_flexsim::server::Event as SimEvent;
use tempo_flexsim::session::{Item, Pattern, Rule};
use tempo_flexsim::{
    vita as sim_vita, Config as SimConfig, Content, Session as SimSession, Simulator, Start, Stream,
};
use tempo_net::flex::encode::Station;

/// An audio backend that notes when transmit audio starts and, as the sound card does, hands it
/// to an installed tee instead of the device.
#[derive(Clone, Default)]
struct TeeBackend {
    played: Arc<Mutex<Vec<Instant>>>,
    tee: Arc<Mutex<Option<crate::backend::TxTeeHandle>>>,
}

impl AudioBackend for TeeBackend {
    fn capture(&mut self) -> Vec<f32> {
        Vec::new()
    }
    fn play(&mut self, samples: &[f32]) {
        self.played.lock().unwrap().push(Instant::now());
        if let Some(tee) = self.tee.lock().unwrap().as_ref() {
            tee.feed(samples);
        }
    }
    fn set_tx_tee(&mut self, tee: Option<crate::backend::TxTeeHandle>) {
        *self.tee.lock().unwrap() = tee;
    }
    fn flush_output(&mut self) -> usize {
        self.tee.lock().unwrap().as_ref().map_or(0, |t| t.flush())
    }
}

/// The radio loop on a simulated FLEX-6400 behind Nexus's own client, in FT8 with TX enabled.
struct FlexScene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    backend: TeeBackend,
    sim: Simulator,
    rebuilt: Arc<std::sync::atomic::AtomicBool>,
}

impl FlexScene {
    /// `native`: the operator's `flex_native_audio`. The simulator streams DAX receive audio once
    /// a receive stream is created, so the receive floor never starves native audio.
    fn new(native: bool) -> FlexScene {
        FlexScene::with_session(native, SimSession::v4_gui_client())
    }

    /// The same scene on a simulated radio that follows `session`.
    fn with_session(native: bool, session: SimSession) -> FlexScene {
        let sim = Simulator::start(
            session,
            SimConfig {
                streams: vec![Stream {
                    stream_id: 0x0400_0001,
                    content: Content::DaxAudio {
                        class: sim_vita::class::AUDIO_F32_STEREO,
                        tone_hz: 1000.0,
                        amplitude: 0.1,
                    },
                    period: Stream::dax_period(),
                    ticks: None,
                    start: Start::After("stream create type=dax_rx".into()),
                    until: Some("stream remove 0x04000001".into()),
                }],
                keepalive_timeout: Duration::from_secs(600),
                ..SimConfig::default()
            },
        )
        .expect("the simulator starts");
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let settings = {
            let mut e = engine.lock().unwrap();
            let mut s = e.settings().clone();
            s.rig_model = 2036;
            s.rig_conn = "network".into();
            // What a Flex profile carries (SmartSDR CAT's address): without one the transport is
            // not a network one, and no Flex path of the loop is reached at all.
            s.rig_addr = "127.0.0.1:5002".into();
            s.ptt_method = "cat".into();
            s.flex_native_cat = true;
            s.flex_radio_ip = "127.0.0.1".into();
            s.flex_native_audio = native;
            e.apply_settings(s);
            e.set_tier(Tier::Ft8);
            e.set_tx_enabled(true);
            e.settings().clone()
        };
        let daemon = FlexDaemon::start_full(
            sim.tcp_addr(),
            0,
            tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
            Options {
                vita: sim.udp_addr(),
                registration: sim.udp_addr(),
                memory: Arc::new(FileMemory::new(None)),
            },
        )
        .expect("the client starts");
        let rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
        let cfg = RadioConfig {
            rig_model: settings.rig_model,
            ..RadioConfig::default()
        };
        // The transport the loop compares its settings against: the same settings, so it never
        // rebuilds the link (which would replace the client under test).
        let state = RadioLoop::new(
            Transport::from_settings(&settings),
            Some(CatDaemon::Flex(daemon)),
            &cfg,
        );
        FlexScene {
            engine,
            state,
            rig,
            backend: TeeBackend::default(),
            sim,
            rebuilt: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn step(&mut self, now: f64) {
        let sinks = no_sinks();
        let mut station = StationSinks::new();
        let backend = self.backend.clone();
        let mut reopen_audio = move |_t: &Transport| Ok::<_, String>(backend.clone());
        let rebuilt = self.rebuilt.clone();
        let mut reopen_rig = move |_t: &Transport, _coexist: bool| {
            rebuilt.store(true, std::sync::atomic::Ordering::Relaxed);
            (Rig::vox(), None, CatProbe::status(None, ""))
        };
        self.state
            .step(
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                &sinks,
                now,
                &mut reopen_audio,
                &mut reopen_rig,
                &mut station,
            )
            .unwrap();
        assert!(
            !self.rebuilt.load(std::sync::atomic::Ordering::Relaxed),
            "the loop rebuilt its transport: the client under test is gone"
        );
    }

    /// Run the loop as `run_radio` does, a step then 20 ms, on the real clock, for `ms`.
    fn run(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            self.step(now_unix_ms());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The simulator's log on this test's clock.
    fn log(&self) -> Vec<(Instant, SimEvent)> {
        let zero = self.sim.started();
        self.sim
            .log()
            .into_iter()
            .map(|l| (zero + l.at, l.event))
            .collect()
    }

    fn set_native_audio(&self, on: bool) {
        let mut e = self.engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.flex_native_audio = on;
        e.apply_settings(s);
    }
}

fn command(e: &SimEvent, want: &str) -> bool {
    matches!(e, SimEvent::Command { text, .. } if text == want)
}

/// ⭐ Native audio through the client, both ways, on its ONE session: the decoder takes the
/// client's DAX receive audio in place of the sound card, the over leaves through its DAX tee, the
/// radio carries no second Nexus session, and the Phone cockpit is told the mic is off
/// exactly while the radio takes DAX (the TX slice is digital here).
#[test]
fn native_audio_on_the_client_carries_both_ways_on_one_session() {
    let mut s = FlexScene::new(true);
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.backend.tee.lock().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the DAX tee never went in");
        s.run(100);
    }
    s.run(500);
    assert!(
        s.state.dax_last_audio.is_some(),
        "DAX receive audio reached the decoder"
    );
    let log = s.log();
    let connections = log
        .iter()
        .filter(|(_, e)| matches!(e, SimEvent::Connected { .. }))
        .count();
    assert_eq!(connections, 1, "one session per radio");
    assert!(log.iter().any(|(_, e)| command(e, "transmit set dax=1")));
    assert!(s.engine.lock().unwrap().snapshot().radio.flex_dax_tx);
    // Off: back to the sound card, the operator's mic back on the radio, the cockpit told.
    s.set_native_audio(false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.backend.tee.lock().unwrap().is_some()
        || !s
            .log()
            .iter()
            .any(|(_, e)| command(e, "transmit set dax=0"))
    {
        assert!(Instant::now() < deadline, "native audio did not let go");
        s.run(100);
    }
    s.run(200);
    assert!(!s.engine.lock().unwrap().snapshot().radio.flex_dax_tx);
}

/// The control for the test above: native audio off, nothing about DAX on the wire, the sound
/// card keeps both directions, and the loop runs as before.
#[test]
fn without_native_audio_the_client_leaves_audio_to_the_sound_card() {
    let mut s = FlexScene::new(false);
    s.run(1_500);
    assert!(s.backend.tee.lock().unwrap().is_none());
    assert!(s.state.dax_last_audio.is_none());
    assert!(!s
        .log()
        .iter()
        .any(|(_, e)| matches!(e, SimEvent::Command { text, .. } if text.starts_with("stream create") || text.starts_with("transmit set dax"))));
}

/// The bundled session, except that the radio reports a slice it switched to one of `modes`, as a
/// radio does (the bundled session answers a mode change without a status).
fn reports(modes: &[&str]) -> SimSession {
    let mut s = SimSession::v4_gui_client();
    for mode in modes {
        let rule = (
            Pattern::Exact(format!("slice set 0 mode={mode}")),
            vec![Rule {
                code: "0".to_string(),
                message: String::new(),
                items: vec![Item::Send(format!("S{{h}}|slice 0 mode={mode}"))],
            }],
        );
        s.rules.retain(|(p, _)| *p != rule.0);
        s.rules.push(rule);
    }
    s
}

/// A second of a recorded message, as the keyer's WAV reader hands it over.
fn message() -> Vec<f32> {
    vec![0.05; 12_000]
}

/// How many `xmit 1` the simulated radio has been sent.
fn keys(s: &FlexScene) -> usize {
    s.log().iter().filter(|(_, e)| command(e, "xmit 1")).count()
}

/// Run the loop until `done` holds, or fail with `what`.
fn run_until(s: &mut FlexScene, what: &str, done: impl Fn(&FlexScene) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(s) {
        assert!(Instant::now() < deadline, "{what}");
        s.run(100);
    }
}

/// ⭐ THE VOICE KEYER CAN'T PLAY WHILE THE RADIO HAS THE MIC (operator ruling, 2026-10-04,
/// "Refuse with a message"). Phone at the shack with native audio on: the TX slice is in USB, so
/// the radio takes its transmit audio from its own mic, and a recorded message, which Nexus sends
/// as DAX, would not reach the air. The send is refused with the reason before anything is
/// queued, so nothing keys and nothing plays. The control, in the same scene: native audio off,
/// and the same message keys the radio and plays, as it always has.
#[test]
fn the_voice_keyer_refuses_in_phone_at_the_shack_with_native_audio_on() {
    let mut s = FlexScene::with_session(true, reports(&["USB"]));
    {
        let mut e = s.engine.lock().unwrap();
        e.set_operating_mode("phone", false); // arms TX
        e.set_frequency(14.250, "20m", "USB");
    }
    run_until(&mut s, "the radio never had the mic", |s| {
        s.engine.lock().unwrap().snapshot().radio.flex_radio_has_mic
    });
    let sent = s.engine.lock().unwrap().send_voice(message());
    s.run(1_000);
    assert_eq!(keys(&s), 0, "nothing was keyed");
    assert!(
        s.backend.played.lock().unwrap().is_empty(),
        "nothing played"
    );
    assert!(s.state.tx_until_ms.is_none());
    let why = sent.expect_err("the voice keyer refuses while the radio has the mic");
    assert!(
        why.contains("the radio has the mic"),
        "the refusal says why: {why}"
    );

    // The client's own answer follows native audio at once, whatever the radio last reported.
    let d = s
        .state
        .rigctld_proc
        .as_ref()
        .and_then(CatDaemon::flex)
        .expect("the client serves the radio");
    assert!(d.radio_has_mic());
    d.set_native_audio(false);
    assert!(
        !d.radio_has_mic(),
        "with native audio off the radio's mic is the operator's own"
    );

    // Native audio off: the radio is the operator's again, and the message plays.
    s.set_native_audio(false);
    run_until(&mut s, "the radio kept the mic for native audio", |s| {
        !s.engine.lock().unwrap().snapshot().radio.flex_radio_has_mic
    });
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    run_until(&mut s, "the message never keyed", |s| keys(s) == 1);
    assert_eq!(
        s.backend.played.lock().unwrap().len(),
        1,
        "the message played"
    );
}

/// …and the client in a digital mode with native audio on (the TX slice in DIGU, the radio on
/// DAX): the radio does not have the mic, and the message keys and goes out over DAX.
#[test]
fn the_voice_keyer_plays_while_the_radio_takes_dax() {
    let mut s = FlexScene::new(true);
    run_until(&mut s, "the DAX tee never went in", |s| {
        s.backend.tee.lock().unwrap().is_some()
            && s.log()
                .iter()
                .any(|(_, e)| command(e, "transmit set dax=1"))
    });
    s.run(300);
    assert!(!s.engine.lock().unwrap().snapshot().radio.flex_radio_has_mic);
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    run_until(&mut s, "the message never keyed", |s| keys(s) == 1);
    assert_eq!(
        s.backend.played.lock().unwrap().len(),
        1,
        "the message played"
    );
    run_until(&mut s, "no DAX transmit audio reached the radio", |s| {
        s.log()
            .iter()
            .any(|(_, e)| matches!(e, SimEvent::UdpIn { bytes, .. } if bytes.len() > 1))
    });
}

/// Whether the radio has the mic, as the engine was last told.
fn has_mic(s: &FlexScene) -> bool {
    s.engine.lock().unwrap().snapshot().radio.flex_radio_has_mic
}

/// Whether nothing holds the transmitter: no over on the air or in its tail, and the radio has
/// let go of it since the last unkey (its interlock READY naming no client). An over keyed
/// before that can be refused by the client's admission, which made the next key a race.
fn idle(s: &FlexScene) -> bool {
    let log = s.log();
    let released = log
        .iter()
        .rposition(|(_, e)| command(e, "xmit 0"))
        .is_none_or(|i| {
            log[i..].iter().any(|(_, e)| {
                matches!(e, SimEvent::Sent { line, .. }
                    if line.contains("tx_client_handle=0x00000000 state=READY"))
            })
        });
    released && s.state.tx_until_ms.is_none() && s.engine.lock().unwrap().tx_owner().is_none()
}

/// The scene on APRS in FM with native audio on, once the radio has the mic: FM is a voice mode,
/// so the radio takes its transmit audio from its own mic. TX is on, as the APRS cockpit's TX
/// strip turns it on: the tune is a context change, and from Digital that leaves the latch down.
fn aprs_in_fm() -> FlexScene {
    let mut s = FlexScene::with_session(true, reports(&["FM"]));
    s.engine
        .lock()
        .unwrap()
        .aprs_tune(144.390)
        .expect("the APRS channel tunes");
    run_until(&mut s, "the radio never had the mic", has_mic);
    s.engine.lock().unwrap().set_tx_enabled(true);
    s
}

/// A second of a picture, as the SSTV encoder hands it over.
fn picture() -> Vec<f32> {
    vec![0.05; 12_000]
}

/// ⭐ APRS CAN'T SEND WHILE THE RADIO HAS THE MIC (operator ruling, 2026-10-04, "Refuse like the
/// voice keyer"). APRS in FM with native audio on: a packet, which Nexus sends as DAX, would not
/// reach the air, and the mic would carry the over in its place. A beacon and a message are
/// refused with the reason before anything is queued, so nothing keys and nothing plays. The
/// control, in the same scene: native audio off, and the same beacon keys the radio.
#[test]
fn an_aprs_send_refuses_in_fm_with_native_audio_on() {
    let mut s = aprs_in_fm();
    let sent = {
        let mut e = s.engine.lock().unwrap();
        [
            e.aprs_beacon(41.88, -87.63, '/', '>', "", &[]),
            e.aprs_send_message("N0CALL", "test"),
        ]
    };
    s.run(1_000);
    assert_eq!(keys(&s), 0, "nothing was keyed");
    assert!(
        s.backend.played.lock().unwrap().is_empty(),
        "nothing played"
    );
    for sent in sent {
        let why = sent.expect_err("APRS refuses while the radio has the mic");
        assert!(
            why.contains("the radio has the mic"),
            "the refusal says why: {why}"
        );
    }

    s.set_native_audio(false);
    run_until(&mut s, "the radio kept the mic for native audio", |s| {
        !has_mic(s)
    });
    // TX On again, as the strip would: native audio going off is a context change, and from
    // Digital that leaves the latch down.
    s.engine.lock().unwrap().set_tx_enabled(true);
    s.engine
        .lock()
        .unwrap()
        .aprs_beacon(41.88, -87.63, '/', '>', "", &[])
        .unwrap();
    run_until(&mut s, "the beacon never keyed", |s| keys(s) == 1);
}

/// …and the UNATTENDED half: an automatic ack, with both operator acts behind it (Monitor armed by
/// hand, TX on), is skipped while the radio has the mic, so nothing keys; it is not held to go out
/// later either. The control: native audio off, and the next ack keys.
#[test]
fn an_aprs_auto_ack_is_skipped_in_fm_with_native_audio_on() {
    let mut s = aprs_in_fm();
    {
        let mut e = s.engine.lock().unwrap();
        e.set_aprs_arm(tempo_app::engine::AprsArm::Explicit);
        e.aprs_auto_ack("N0CALL", "W9XYZ", "001");
    }
    s.run(1_000);
    assert_eq!(keys(&s), 0, "nothing was keyed");
    assert!(
        s.backend.played.lock().unwrap().is_empty(),
        "nothing played"
    );

    s.set_native_audio(false);
    run_until(&mut s, "the radio kept the mic for native audio", |s| {
        !has_mic(s)
    });
    // TX On again, as the strip would: native audio going off is a context change, and from
    // Digital that leaves the latch down.
    s.engine.lock().unwrap().set_tx_enabled(true);
    s.run(500);
    assert_eq!(keys(&s), 0, "the skipped ack was not held for later");
    s.engine
        .lock()
        .unwrap()
        .aprs_auto_ack("N0CALL", "W9XYZ", "002");
    run_until(&mut s, "the ack never keyed", |s| keys(s) == 1);
}

/// ⭐ SSTV CAN'T SEND WHILE THE RADIO HAS THE MIC (the same ruling). SSTV in plain SSB (a radio
/// set for plain SSB instead of the DATA submode) with native audio on: the TX slice stays in USB,
/// so the radio takes its own mic, and the picture, which Nexus sends as DAX, would not reach the
/// air. The send is refused with the reason before anything waits, so nothing keys and nothing
/// plays. The control, in the same scene: native audio off, and the same picture keys the radio.
#[test]
fn an_sstv_send_refuses_in_plain_ssb_with_native_audio_on() {
    let mut s = FlexScene::with_session(true, reports(&["USB"]));
    {
        let mut e = s.engine.lock().unwrap();
        let mut settings = e.settings().clone();
        settings.data_modes_plain_ssb = true;
        e.apply_settings(settings);
        e.set_operating_mode("phone", false); // arms TX
        e.set_frequency(14.230, "20m", "USB");
    }
    run_until(&mut s, "the radio never had the mic", has_mic);
    let sent = s
        .engine
        .lock()
        .unwrap()
        .sstv_send(picture(), "Scottie 1".to_string());
    s.run(1_000);
    assert_eq!(keys(&s), 0, "nothing was keyed");
    assert!(
        s.backend.played.lock().unwrap().is_empty(),
        "nothing played"
    );
    let why = sent.expect_err("SSTV refuses while the radio has the mic");
    assert!(
        why.contains("the radio has the mic"),
        "the refusal says why: {why}"
    );

    s.set_native_audio(false);
    run_until(&mut s, "the radio kept the mic for native audio", |s| {
        !has_mic(s)
    });
    s.engine
        .lock()
        .unwrap()
        .sstv_send(picture(), "Scottie 1".to_string())
        .unwrap();
    run_until(&mut s, "the picture never keyed", |s| keys(s) == 1);
}

/// …and the client in a digital mode with native audio on (the TX slice in DIGU, the radio on
/// DAX): the radio does not have the mic, and a beacon, an automatic ack and a picture each key as
/// before, the packets going out over DAX. The picture rides Phone with the data submode held
/// while the SSTV receiver runs, which keeps the slice digital.
#[test]
fn aprs_and_sstv_key_while_the_radio_takes_dax() {
    let mut s = FlexScene::new(true);
    run_until(&mut s, "the DAX tee never went in", |s| {
        s.backend.tee.lock().unwrap().is_some()
            && s.log()
                .iter()
                .any(|(_, e)| command(e, "transmit set dax=1"))
    });
    s.run(300);
    assert!(!has_mic(&s));
    {
        let mut e = s.engine.lock().unwrap();
        e.set_aprs_arm(tempo_app::engine::AprsArm::Explicit);
        e.aprs_beacon(41.88, -87.63, '/', '>', "", &[]).unwrap();
    }
    run_until(&mut s, "the beacon never keyed", |s| keys(s) == 1);
    // An ack that arrives while the transmitter is busy is not sent (as before), so the next
    // message arrives after the beacon's over.
    run_until(&mut s, "the beacon's over never ended", idle);
    s.engine
        .lock()
        .unwrap()
        .aprs_auto_ack("N0CALL", "W9XYZ", "001");
    run_until(&mut s, "the ack never keyed", |s| keys(s) == 2);
    run_until(&mut s, "no DAX transmit audio reached the radio", |s| {
        s.log()
            .iter()
            .any(|(_, e)| matches!(e, SimEvent::UdpIn { bytes, .. } if bytes.len() > 1))
    });

    {
        let mut e = s.engine.lock().unwrap();
        let mut settings = e.settings().clone();
        settings.sstv_hold_data_submode = true;
        e.apply_settings(settings);
        e.set_sstv_armed(true);
        e.set_operating_mode("phone", false); // arms TX
        e.set_frequency(14.230, "20m", "USB");
    }
    s.run(500);
    run_until(&mut s, "the ack's over never ended", idle);
    assert!(!has_mic(&s), "the slice stayed digital");
    s.engine
        .lock()
        .unwrap()
        .sstv_send(picture(), "Scottie 1".to_string())
        .unwrap();
    run_until(&mut s, "the picture never keyed", |s| keys(s) == 3);
}

/// The percentiles of a sample, in ms.
fn summary(mut v: Vec<f64>) -> String {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    let pick = |p: f64| v[((p * (n - 1) as f64).round() as usize).min(n - 1)];
    let mean = v.iter().sum::<f64>() / n as f64;
    format!(
        "n={n} p50={:.1} p90={:.1} p95={:.1} p99={:.1} max={:.1} mean={:.1}",
        pick(0.5),
        pick(0.9),
        pick(0.95),
        pick(0.99),
        v[n - 1],
        mean
    )
}

fn median(v: &[f64]) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Milliseconds from `at` to `t`, signed.
fn ms(t: Instant, at: Instant) -> f64 {
    if t >= at {
        t.duration_since(at).as_secs_f64() * 1000.0
    } else {
        -(at.duration_since(t).as_secs_f64() * 1000.0)
    }
}

/// A fixed-seed generator, so every run uses the same pre-rolls.
fn rng(seed: u64) -> impl FnMut() -> f64 {
    let mut s = seed;
    move || {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((s >> 33) as f64) / ((1u64 << 31) as f64)
    }
}

/// One over: the loop crosses an even FT8 boundary (our TX period, an over queued) on a virtual
/// clock that runs in real time, from `pre` ms before it, until the over keys (`stop_at_key`) or
/// `until` ms past the boundary. `during` runs before every tick with the virtual time relative to
/// the boundary. Returns the boundary's real instant and the instant of the tick that keyed.
///
/// Every trial starts as a run of overs does after its first: the loop's last boundary was ours,
/// so this one keys at the boundary tick without waiting on a decode, in both routes alike (the
/// DAX route's receive ring holds audio, the sound card route's does not; an early-decode boundary
/// keys in the same tick through the same `key_boundary_tx`).
fn one_over(
    s: &mut FlexScene,
    boundary: f64,
    pre: f64,
    stop_at_key: bool,
    until: f64,
    during: &mut dyn FnMut(&mut FlexScene, f64),
) -> (Instant, Option<Instant>) {
    {
        let mut e = s.engine.lock().unwrap();
        e.broadcast("CQ TEST W9XYZ EN37");
        // The boundary path, as every over after the first in a run keys.
        let _ = e.take_immediate_tx();
    }
    s.state.prev_slot_was_tx = true;
    let plays = s.backend.played.lock().unwrap().len();
    let w0 = Instant::now();
    let at_boundary = w0 + Duration::from_secs_f64(pre / 1000.0);
    let start_v = boundary - pre;
    let mut keyed = None;
    loop {
        let v = start_v + w0.elapsed().as_secs_f64() * 1000.0;
        during(s, v - boundary);
        let t = Instant::now();
        s.step(v);
        if keyed.is_none() && s.backend.played.lock().unwrap().len() > plays {
            keyed = Some(t);
            if stop_at_key {
                return (at_boundary, keyed);
            }
        }
        if v > boundary + until {
            return (at_boundary, keyed);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The first even FT8 boundary comfortably ahead of the real clock.
fn first_boundary() -> f64 {
    let period = 15_000.0;
    let mut first = ((now_unix_ms() / period).floor() + 8.0) * period;
    if (first / period) as u64 % 2 == 1 {
        first += period;
    }
    first
}

/// ⭐⭐ THE KEYING PATH NEVER CARRIES THE FLAG (operator ruling, 2026-10-03: "write the flag when
/// the mode or the TX slice changes, never between the slot boundary and `xmit 1`"). FT8 with TX
/// enabled and an over queued every TX period, while the operator turns native audio off and on at
/// moments chosen to land just before, at and just after the boundary, so the flag must change
/// across overs. Every `transmit set dax` the radio receives is checked against every over: never
/// between that over's boundary and its `xmit 1`, never while it is keyed (from `xmit 1` to the
/// radio's READY naming no transmitter), and never within the loop's guard of a boundary, either
/// side. The flag does change: the writes happen, between overs.
#[test]
fn the_dax_source_is_written_only_at_quiet_points() {
    let mut s = FlexScene::new(true);
    // Settle: the DAX source written for the slice's digital mode, the route up.
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.backend.tee.lock().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the DAX tee never went in");
        s.run(100);
    }
    let period = 15_000.0;
    let mut boundary = first_boundary();
    // Where, relative to each boundary, the toggle lands (ms; negative is before it).
    let offsets = [
        -1_200.0, -900.0, -400.0, -120.0, -40.0, -5.0, 0.0, 15.0, 60.0, 300.0,
    ];
    let mut overs: Vec<(Instant, Option<Instant>)> = Vec::new();
    let mut native = true;
    for offset in &offsets {
        let mut toggled = false;
        let mut during = |sc: &mut FlexScene, rel: f64| {
            if !toggled && rel >= *offset {
                toggled = true;
                native = !native;
                sc.set_native_audio(native);
                // A Settings save clears the queued over (`apply_settings`); the CQ run goes on.
                let mut e = sc.engine.lock().unwrap();
                e.broadcast("CQ TEST W9XYZ EN37");
                let _ = e.take_immediate_tx();
            }
        };
        let pre = 1_500.0;
        let xmits_before = s.log().iter().filter(|(_, e)| command(e, "xmit 1")).count();
        // Run on a second past the boundary: the late toggles land while the over is keyed.
        let (at, _) = one_over(&mut s, boundary, pre, false, 1_000.0, &mut during);
        let xmit = s
            .log()
            .iter()
            .filter(|(_, e)| command(e, "xmit 1"))
            .map(|(t, _)| *t)
            .nth(xmits_before);
        overs.push((at, xmit));
        // Let the over run its course before the next period's pre-roll: the unkey, the readback
        // and any quiet-point write all happen here, between overs.
        boundary += 2.0 * period;
        let resume = boundary - pre - 2_500.0;
        let w0 = Instant::now();
        while w0.elapsed() < Duration::from_millis(2_000) {
            let v = resume + w0.elapsed().as_secs_f64() * 1000.0;
            s.step(v);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let log = s.log();
    let writes: Vec<Instant> = log
        .iter()
        .filter(|(_, e)| matches!(e, SimEvent::Command { text, .. } if text.starts_with("transmit set dax=")))
        .map(|(t, _)| *t)
        .collect();
    // Every keyed window: from each `xmit 1` to the next READY naming no transmitter.
    let mut keyed: Vec<(Instant, Instant)> = Vec::new();
    for (i, (t, e)) in log.iter().enumerate() {
        if command(e, "xmit 1") {
            let end = log[i..]
                .iter()
                .find(|(_, e)| matches!(e, SimEvent::Sent { line, .. } if line.contains("tx_client_handle=0x00000000 state=READY")))
                .map(|(t, _)| *t)
                .unwrap_or_else(Instant::now);
            keyed.push((*t, end));
        }
    }
    let keyed_overs = overs.iter().filter(|(_, x)| x.is_some()).count();
    let trace: Vec<String> = overs
        .iter()
        .zip(&offsets)
        .map(|((at, x), off)| {
            let near: Vec<String> = log
                .iter()
                .filter(|(t, e)| {
                    *t + Duration::from_millis(2_000) >= *at
                        && *t <= *at + Duration::from_millis(1_500)
                        && matches!(e, SimEvent::Command { text, .. } if text != "ping")
                })
                .map(|(t, e)| match e {
                    SimEvent::Command { text, .. } => format!("{:+.0} {text}", ms(*t, *at)),
                    _ => String::new(),
                })
                .collect();
            format!(
                "toggle {off:+}: keyed {:?}; {near:?}",
                x.map(|x| ms(x, *at))
            )
        })
        .collect();
    assert!(
        keyed_overs >= 4,
        "only {keyed_overs} overs keyed: the scene is not exercising the path\n{}",
        trace.join("\n")
    );
    for w in &writes {
        for (at, xmit) in &overs {
            if let Some(x) = xmit {
                assert!(
                    !(*w >= *at && *w <= *x),
                    "the DAX source was written {:.1} ms after a boundary, before that over's xmit 1 ({:.1} ms)",
                    ms(*w, *at),
                    ms(*x, *at)
                );
            }
        }
        for (from, to) in &keyed {
            assert!(
                !(*w >= *from && *w <= *to),
                "the DAX source was written while keyed ({:.1} ms into the over)",
                ms(*w, *from)
            );
        }
        // The guard either side of every boundary, less the session's delivery (≤ one read poll).
        for (at, _) in &overs {
            let off = ms(*w, *at);
            assert!(
                off.abs() >= ROUTING_GUARD_MS - 50.0,
                "the DAX source was written {off:.1} ms from a boundary\n{}",
                trace.join("\n")
            );
        }
    }
    assert!(
        writes.len() >= offsets.len() / 2,
        "the flag must follow the toggles: only {} writes",
        writes.len()
    );
    // The evidence, for the record (`--nocapture`).
    let nearest = |w: &Instant| {
        overs
            .iter()
            .map(|(at, _)| ms(*w, *at))
            .min_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap_or(f64::NAN)
    };
    println!(
        "QUIET POINTS: {} trials, {keyed_overs} overs keyed, {} writes; nearest boundary per write (ms): {:?}",
        offsets.len(),
        writes.len(),
        writes.iter().map(|w| nearest(w).round()).collect::<Vec<_>>()
    );
}

/// THE EVIDENCE FOR THE FT-TIMING SIGN-OFF: an FT8 over's keying and audio start on
/// DAX TX against the sound card route, measured. Not an assertion about the timing: the operator
/// decides what a shift is worth.
///
/// Two configurations, each its own loop, client and simulated radio, taken in turn trial by
/// trial so the box's load falls on both alike. The sound card route is the client with native
/// audio off (today's path on the client: the over is handed to the sound card, which on a Flex
/// is SmartSDR's DAX device); the DAX TX route is the client with native audio on. Each trial
/// crosses an even FT8 boundary on a virtual clock that runs in real time, from a random 0.6–1.5 s
/// before it, ticking as `run_radio` does. Measured from the boundary's real instant: the start of
/// the tick that keyed; `xmit 1` arriving at the radio (PTT on the wire); the over handed to the
/// audio route (`play`); and, on DAX, the over's first DAX TX packet arriving at the radio's
/// VITA-49 port. The sound card route's own latency after `play` (the device buffer, SmartSDR's
/// DAX driver, the network) is not modelled here; DAX TX's is, up to the radio's socket.
///
/// `FLEX_DAX_TRIALS` (default 40) sets the trials per configuration; `FLEX_DAX_LATENCY_OUT` names a
/// file for the raw samples (CSV). Run alone, optimised as shipped: `cargo test --release -p
/// tempo-audio --features device,serial --lib flex_dax_tx_ft_timing -- --ignored --nocapture`.
#[test]
#[ignore = "measurement: minutes of real time; run explicitly for the FT-timing sign-off"]
fn flex_dax_tx_ft_timing_measurement() {
    let trials: usize = std::env::var("FLEX_DAX_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let mut rnd = rng(0x2026_1004);
    let configs = [("sound card", false), ("DAX TX", true)];
    let mut scenes: Vec<FlexScene> = configs
        .iter()
        .map(|&(_, native)| FlexScene::new(native))
        .collect();
    // Settle: the read-only latch asserted, and on DAX the source written and the route up.
    for (s, &(name, native)) in scenes.iter_mut().zip(&configs) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            s.run(100);
            let ready = s.backend.tee.lock().unwrap().is_some();
            if ready == native && s.state.last_mode == "PKTUSB" {
                break;
            }
            assert!(Instant::now() < deadline, "{name}: never settled");
        }
    }
    let period = 15_000.0;
    let mut boundary = [first_boundary(); 2];
    // Per configuration: (tick, ptt on the wire, play, audio on the wire), ms from the boundary.
    let mut samples: Vec<Vec<[f64; 4]>> = vec![Vec::new(); 2];
    let mut csv = String::from("route,trial,pre_ms,tick_ms,ptt_wire_ms,play_ms,audio_wire_ms\n");
    // One warm-up over each: the loop's first boundary decides from a decode; every later one keys
    // at the boundary, as a run of overs does.
    for trial in 0..=trials {
        for (i, &(name, native)) in configs.iter().enumerate() {
            let pre = 600.0 + 900.0 * rnd();
            let s = &mut scenes[i];
            let packets_before = s
                .sim
                .log()
                .iter()
                .filter(|l| matches!(&l.event, SimEvent::UdpIn { bytes, .. } if bytes.len() > 1))
                .count();
            let plays = s.backend.played.lock().unwrap().len();
            let xmits_before = s
                .sim
                .log()
                .iter()
                .filter(|l| command(&l.event, "xmit 1"))
                .count();
            let (at, tick) = one_over(
                s,
                boundary[i],
                pre,
                true,
                2_000.0,
                &mut |_: &mut FlexScene, _: f64| {},
            );
            let tick = tick.unwrap_or_else(|| panic!("{name} trial {trial}: nothing keyed"));
            // Let the first DAX packets arrive.
            std::thread::sleep(Duration::from_millis(60));
            let log = s.log();
            // This trial's key: the first `xmit 1` after those already on the log.
            let ptt = log
                .iter()
                .filter(|(_, e)| command(e, "xmit 1"))
                .map(|(t, _)| *t)
                .nth(xmits_before)
                .unwrap_or_else(|| panic!("{name} trial {trial}: no xmit 1 on the wire"));
            let play = s.backend.played.lock().unwrap()[plays];
            let audio = if native {
                log.iter()
                    .filter(|(_, e)| matches!(e, SimEvent::UdpIn { bytes, .. } if bytes.len() > 1))
                    .nth(packets_before)
                    .map(|(t, _)| *t)
                    .unwrap_or_else(|| panic!("{name} trial {trial}: no DAX TX packet"))
            } else {
                // Control, every trial: the sound card route sends nothing over DAX.
                let packets = log
                    .iter()
                    .filter(|(_, e)| matches!(e, SimEvent::UdpIn { bytes, .. } if bytes.len() > 1))
                    .count();
                assert_eq!(
                    packets, 0,
                    "{name} trial {trial}: DAX TX on the sound card route"
                );
                play
            };
            // The over has started; its remaining audio is of no use to the measurement and only
            // fills the simulator's log. The loop unkeys it at the next trial's first tick.
            s.backend.flush_output();
            boundary[i] += 2.0 * period;
            if trial == 0 {
                continue; // the warm-up
            }
            let row = [ms(tick, at), ms(ptt, at), ms(play, at), ms(audio, at)];
            csv.push_str(&format!(
                "{name},{trial},{pre:.1},{:.2},{:.2},{:.2},{:.2}\n",
                row[0], row[1], row[2], row[3]
            ));
            samples[i].push(row);
        }
    }
    for (i, &(name, _)) in configs.iter().enumerate() {
        let col = |k: usize| samples[i].iter().map(|r| r[k]).collect::<Vec<f64>>();
        let ptt_to_audio = samples[i].iter().map(|r| r[3] - r[1]).collect::<Vec<f64>>();
        println!(
            "FLEX DAX TIMING route={name}\n  tick start    : {}\n  ptt on wire   : {}\n  play          : {}\n  audio on wire : {}\n  ptt->audio    : {}",
            summary(col(0)),
            summary(col(1)),
            summary(col(2)),
            summary(col(3)),
            summary(ptt_to_audio)
        );
    }
    // The median shift DAX − sound card, with a 95 % bootstrap interval, for PTT and audio.
    let mut boot = rng(0x0B00_7570);
    for (k, what) in [(1usize, "ptt on wire"), (3usize, "audio start")] {
        let a: Vec<f64> = samples[0].iter().map(|r| r[k]).collect();
        let b: Vec<f64> = samples[1].iter().map(|r| r[k]).collect();
        let shift = median(&b) - median(&a);
        let mut shifts: Vec<f64> = (0..2_000)
            .map(|_| {
                let pick = |v: &[f64], r: &mut dyn FnMut() -> f64| -> Vec<f64> {
                    (0..v.len())
                        .map(|_| v[((r() * v.len() as f64) as usize).min(v.len() - 1)])
                        .collect()
                };
                median(&pick(&b, &mut boot)) - median(&pick(&a, &mut boot))
            })
            .collect();
        shifts.sort_by(|x, y| x.total_cmp(y));
        println!(
            "MEDIAN SHIFT (DAX TX - sound card) {what}: {shift:+.1} ms [{:+.1}, {:+.1}]",
            shifts[50], shifts[1_949]
        );
    }
    if let Ok(path) = std::env::var("FLEX_DAX_LATENCY_OUT") {
        std::fs::write(&path, csv).expect("write the samples");
        println!("samples: {path}");
    }
}
