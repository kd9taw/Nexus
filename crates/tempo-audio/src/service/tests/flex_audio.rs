//! Nexus's own Flex client in the radio loop: native audio on the client's one session, the
//! transmit audio source written only at a quiet point, the FT-timing measurement of DAX TX
//! against the sound card route (the operator signs off on the numbers before native audio can
//! become anything but opt-in), and what the client must tell the operator about the transmitter:
//! its alarms on the CAT status, and an APRS key it refuses.
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
    /// A rebuild of the CAT link starts a fresh client on the same radio, as `open_cat` does, in
    /// place of failing the test.
    replace_on_rebuild: bool,
}

impl FlexScene {
    /// `native`: the operator's `flex_native_audio`. The simulator streams DAX receive audio once
    /// a receive stream is created, so the receive floor never starves native audio.
    fn new(native: bool) -> FlexScene {
        FlexScene::with_session(native, SimSession::v4_gui_client())
    }

    /// The same scene on a simulated radio that follows `session`.
    fn with_session(native: bool, session: SimSession) -> FlexScene {
        FlexScene::with_faults(
            native,
            session,
            Vec::new(),
            tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
        )
    }

    /// The same scene on a simulated radio with `faults`, its client on `config`.
    fn with_faults(
        native: bool,
        session: SimSession,
        faults: Vec<tempo_flexsim::Fault>,
        config: tempo_net::flex::session::Config,
    ) -> FlexScene {
        FlexScene::on(FlexScene::radio(session, faults), native, config)
    }

    /// The scene's simulated radio, before any client: a test can act on it first.
    fn radio(session: SimSession, faults: Vec<tempo_flexsim::Fault>) -> Simulator {
        Simulator::start(
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
                faults,
            },
        )
        .expect("the simulator starts")
    }

    /// The scene on `sim`, its client on `config`.
    fn on(sim: Simulator, native: bool, config: tempo_net::flex::session::Config) -> FlexScene {
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
            config,
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
            replace_on_rebuild: false,
        }
    }

    fn step(&mut self, now: f64) {
        let sinks = no_sinks();
        let mut station = StationSinks::new();
        let backend = self.backend.clone();
        let mut reopen_audio = move |_t: &Transport| Ok::<_, String>(backend.clone());
        let rebuilt = self.rebuilt.clone();
        let (replace, radio) = (self.replace_on_rebuild, self.sim.tcp_addr());
        let mut reopen_rig = move |t: &Transport, _coexist: bool| {
            rebuilt.store(true, std::sync::atomic::Ordering::Relaxed);
            if !replace {
                return (Rig::vox(), None, CatProbe::status(None, ""));
            }
            // What `open_cat` does for a radio Nexus's own client serves: a fresh client, which
            // remembers the handles of the sessions it replaces, probed as every open is.
            match FlexDaemon::start(radio, 0) {
                Ok(d) => {
                    let mut rig = Rig::rigctld(&format!("127.0.0.1:{}", d.local_addr().port()));
                    let probe = finish_cat_open(&mut rig, t);
                    (rig, Some(CatDaemon::Flex(d)), probe)
                }
                Err(e) => (
                    Rig::vox(),
                    None,
                    CatProbe::status(Some(false), e.to_string()),
                ),
            }
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
            self.replace_on_rebuild || !self.rebuilt.load(std::sync::atomic::Ordering::Relaxed),
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
    // The DAX source is written at the loop's next quiet point (`RadioLoop::tx_routing_quiet`),
    // not with the tee: a transmit stream created in the last tick before a slot boundary's guard
    // puts the tee in inside the guard, and the flag waits for the guard after the boundary, about
    // two seconds later. A digital over in between is refused on the mic (`flex::shim`).
    run_until(&mut s, "the radio was never told to take DAX", |s| {
        s.log()
            .iter()
            .any(|(_, e)| command(e, "transmit set dax=1"))
    });
    s.run(200);
    let log = s.log();
    let connections = log
        .iter()
        .filter(|(_, e)| matches!(e, SimEvent::Connected { .. }))
        .count();
    assert_eq!(connections, 1, "one session per radio");
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

/// The bundled session, with the radio's transmitter already taking its audio from DAX when Nexus
/// connects, as SmartSDR's own DAX switch leaves it (the flag is radio-wide).
fn radio_on_dax() -> SimSession {
    let mut s = SimSession::v4_gui_client();
    for (pattern, rules) in &mut s.rules {
        if *pattern == Pattern::Exact("sub tx all".to_string()) {
            for item in rules.iter_mut().flat_map(|r| r.items.iter_mut()) {
                if let Item::Send(line) = item {
                    if line.starts_with("S0|transmit ") {
                        *line = line.replace(" dax=0 ", " dax=1 ");
                    }
                }
            }
        }
    }
    s
}

/// ⭐ A RADIO ALREADY ON DAX GETS NEXUS'S TRANSMIT STREAM. The flag the routing follows the mode
/// with is radio-wide, and SmartSDR's own DAX switch sets it, so a radio can already take its
/// transmit audio from DAX when Nexus connects. Nothing needs writing then, but Nexus's own
/// transmit stream still has to exist: without it the tee never goes in and the client refuses
/// every digital over. One create, the tee in, a queued over keys and its audio leaves as DAX TX,
/// and the flag is never written (the operator's own setting stands, so nothing is owed back).
#[test]
fn a_radio_already_on_dax_gets_the_transmit_stream_and_keys() {
    let mut s = FlexScene::with_session(true, radio_on_dax());
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.backend.tee.lock().unwrap().is_none() && Instant::now() < deadline {
        s.run(100);
    }
    let tee = s.backend.tee.lock().unwrap().is_some();
    let (_, played) = one_over(
        &mut s,
        first_boundary(),
        1_500.0,
        false,
        1_500.0,
        &mut |_, _| {},
    );
    let log = s.log();
    let count = |want: &str| log.iter().filter(|(_, e)| command(e, want)).count();
    let dax_tx_audio = log
        .iter()
        .any(|(_, e)| matches!(e, SimEvent::UdpIn { bytes, .. } if bytes.len() > 1));
    let writes = log
        .iter()
        .filter(|(_, e)| matches!(e, SimEvent::Command { text, .. } if text.starts_with("transmit set dax")))
        .count();
    let tx_enabled = s.engine.lock().unwrap().tx_enabled();
    assert_eq!(
        format!(
            "dax_tx_creates={} tee={tee} xmit1={} played={} dax_tx_audio={dax_tx_audio} \
             dax_writes={writes} tx_enabled={tx_enabled}",
            count("stream create type=dax_tx"),
            count("xmit 1"),
            played.is_some(),
        ),
        "dax_tx_creates=1 tee=true xmit1=1 played=true dax_tx_audio=true dax_writes=0 \
         tx_enabled=true"
    );
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

// ── What the client must tell the operator about the transmitter ─────────────────────────────

/// The CAT status the operator reads (the cockpit's CAT chip): its verdict and its line.
fn cat_status(s: &FlexScene) -> (Option<bool>, String) {
    let radio = s.engine.lock().unwrap().snapshot().radio;
    (radio.cat_ok, radio.cat_detail)
}

/// Whether a CAT status line carries one of the client's alarms about the transmitter.
fn names_an_alarm(detail: &str) -> bool {
    [
        "may still be transmitting",
        "still holds the transmitter",
        "stopped answering",
    ]
    .iter()
    .any(|words| detail.contains(words))
}

/// Queue a position beacon, as the APRS cockpit's Beacon button does.
fn beacon(s: &FlexScene) {
    s.engine
        .lock()
        .unwrap()
        .aprs_beacon(41.88, -87.63, '/', '>', "", &[])
        .expect("the beacon is queued");
}

/// How many `xmit 0` the simulated radio has been sent.
fn unkeys(s: &FlexScene) -> usize {
    s.log().iter().filter(|(_, e)| command(e, "xmit 0")).count()
}

/// Run the loop a tick at a time until the tick that rebuilds the CAT link, and read the CAT
/// status that tick left: what the operator reads once the client has been restarted.
fn status_after_the_rebuild(s: &mut FlexScene) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        s.step(now_unix_ms());
        if s.rebuilt.load(std::sync::atomic::Ordering::Relaxed) {
            return cat_status(s).1;
        }
        assert!(
            Instant::now() < deadline,
            "the loop never rebuilt the CAT link"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The client's session, its unkey deadline shortened as the client's own tests shorten it.
fn short_deadline() -> tempo_net::flex::session::Config {
    let mut config = tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap());
    config.unkey_deadline_ms = 1_500;
    config
}

/// The scene with the beacon's over lost: the radio drops the client's connection 100 ms into the
/// over and stays keyed under the lost session's handle. A rebuild starts a fresh client.
fn lost_mid_over() -> FlexScene {
    let mut s = FlexScene::with_faults(
        false,
        SimSession::v4_gui_client(),
        vec![tempo_flexsim::Fault::DisconnectMidOver {
            after: Duration::from_millis(100),
            radio_stays_keyed: true,
        }],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    s.replace_on_rebuild = true;
    s
}

/// ⭐ THE ALARM LEADS THE RESTART THAT FOLLOWS IT. The radio never confirms the unkey after a
/// beacon's over: the client says the radio may still be transmitting and its session closes, and
/// the loop, in one tick, reads that alarm and restarts the client. The restart's report goes on
/// the same status line, so after that tick the operator must read the alarm first, then the
/// restart.
#[test]
fn a_flex_alarm_leads_the_restart_that_follows_it() {
    let mut s = FlexScene::with_faults(
        false,
        SimSession::v4_gui_client(),
        vec![tempo_flexsim::Fault::StuckTransmit],
        short_deadline(),
    );
    s.replace_on_rebuild = true;
    beacon(&s);
    let detail = status_after_the_rebuild(&mut s);
    assert_eq!(keys(&s), 1, "the beacon's over was keyed");
    assert!(
        detail.starts_with(
            "the radio did not confirm the unkey — it may still be transmitting. Check the radio \
             now. The CAT helper (rigctld) stopped and was restarted."
        ),
        "{detail}"
    );
}

/// ⭐ …and a session LOST during the over: nothing confirmed the unkey, so the alarm is the same,
/// the radio may still be transmitting, and it leads the restart's report as well.
#[test]
fn a_session_lost_mid_over_is_the_alarm_that_leads_the_restart() {
    let mut s = lost_mid_over();
    beacon(&s);
    let detail = status_after_the_rebuild(&mut s);
    assert_eq!(keys(&s), 1, "the beacon's over was keyed");
    assert!(
        detail.starts_with(
            "the connection to the radio was lost during a transmission — it may still be \
             transmitting. Check the radio now."
        ),
        "{detail}"
    );
}

/// ⭐ A LIVE CLIENT'S ALARM REACHES THE CAT STATUS. The client that replaces the lost one finds the
/// lost session still holding the transmitter: it will not key under it, and says so while it
/// runs. The loop read a client's alarm only once the client was dead, so this reached only the
/// app log. It is shown once, in the lane the loop's transmit notices use, with the link's verdict
/// as it was: a transmit fault, not a CAT fault.
#[test]
fn a_live_clients_alarm_reaches_the_cat_status_once() {
    let mut s = lost_mid_over();
    beacon(&s);
    status_after_the_rebuild(&mut s);
    run_until(&mut s, "the operator was never told", |s| {
        cat_status(s).1.contains("still holds the transmitter")
    });
    let (ok, detail) = cat_status(&s);
    assert!(
        detail.starts_with("an earlier Nexus session (0x"),
        "{detail}"
    );
    assert_eq!(ok, Some(true), "the link's verdict, kept");
    let shown = s.engine.lock().unwrap().cat_probe_gen();
    s.run(500);
    assert_eq!(
        s.engine.lock().unwrap().cat_probe_gen(),
        shown,
        "shown once, not written again every tick"
    );
}

/// The control: an over whose unkey the radio confirms puts no alarm on the CAT status.
#[test]
fn a_confirmed_unkey_puts_no_alarm_on_the_cat_status() {
    let mut s = FlexScene::new(false);
    beacon(&s);
    run_until(&mut s, "the beacon never keyed", |s| keys(s) == 1);
    run_until(&mut s, "the beacon's over never ended", idle);
    s.run(1_000);
    let (_, detail) = cat_status(&s);
    assert!(!names_an_alarm(&detail), "{detail}");
}

/// The control: a session lost while NOTHING is keyed (the radio stops answering pings while the
/// station is idle, and the client's keepalive ends the session) raises no alarm: the restart is
/// all the status says.
#[test]
fn a_session_lost_while_idle_raises_no_alarm() {
    let mut s = FlexScene::with_faults(
        false,
        SimSession::v4_gui_client(),
        vec![tempo_flexsim::Fault::DropPings {
            first: 1,
            count: 1_000,
        }],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    s.replace_on_rebuild = true;
    let detail = status_after_the_rebuild(&mut s);
    assert_eq!(keys(&s), 0, "nothing was keyed");
    assert!(
        detail.starts_with("the CAT helper (rigctld) stopped and was restarted."),
        "{detail}"
    );
    assert!(!names_an_alarm(&detail), "{detail}");
}

// ── The transmitter alarm stays on screen until the operator dismisses it ────────────────────

/// The transmitter alarms on screen, in the order shown, as `(id, text)`: read off the snapshot in
/// the form the screen receives it.
fn tx_alarms(s: &FlexScene) -> Vec<(u64, String)> {
    let snap = serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap();
    snap["txAlarms"]
        .as_array()
        .map(|alarms| {
            alarms
                .iter()
                .map(|a| {
                    (
                        a["id"].as_u64().unwrap_or_default(),
                        a["text"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// ⭐ THE ALARM OUTLIVES THE CAT LINE (operator ruling, 2026-10-04: "Sticky until dismissed"). The
/// radio never confirms the unkey after a beacon's over, and the alarm reaches the CAT status. That
/// line is latest-wins: the next CAT message replaced the alarm, and it could scroll off unseen.
/// It stays on screen until the operator dismisses it.
#[test]
fn a_flex_tx_alarm_stays_on_screen_after_a_later_cat_message() {
    let unconfirmed =
        "the radio did not confirm the unkey — it may still be transmitting. Check the radio now.";
    let mut s = FlexScene::with_faults(
        false,
        SimSession::v4_gui_client(),
        vec![tempo_flexsim::Fault::StuckTransmit],
        short_deadline(),
    );
    s.replace_on_rebuild = true;
    beacon(&s);
    let detail = status_after_the_rebuild(&mut s);
    assert!(
        detail.starts_with(unconfirmed),
        "premise: the CAT status carried it: {detail}"
    );
    // The next CAT message, through the setter every one of them takes, then the loop's own.
    s.engine
        .lock()
        .unwrap()
        .set_cat_status(Some(true), "Connected — 14.074 MHz".to_string());
    s.run(500);
    let (_, detail) = cat_status(&s);
    assert!(
        !detail.contains(unconfirmed),
        "premise: the CAT line moved on: {detail}"
    );
    let shown = tx_alarms(&s);
    assert_eq!(
        shown.first().map(|(_, text)| text.as_str()),
        Some(unconfirmed),
        "the alarm left the screen with the CAT line: {shown:?}"
    );
}

/// ⭐ …AND A RECONNECT KEEPS IT. A session lost mid-over: the alarm is raised as the session ends,
/// and the loop restarts the client in the same tick. The fresh client finds the lost session
/// still holding the transmitter and says so too. A newer alarm queues behind the one on screen
/// and never replaces it, so the first stays first, through the reconnect and after it.
#[test]
fn a_reconnect_keeps_the_tx_alarm() {
    let lost = "the connection to the radio was lost during a transmission — it may still be \
                transmitting. Check the radio now.";
    let mut s = lost_mid_over();
    beacon(&s);
    status_after_the_rebuild(&mut s);
    let shown = tx_alarms(&s);
    assert_eq!(
        shown.first().map(|(_, text)| text.as_str()),
        Some(lost),
        "the reconnect took the alarm off the screen: {shown:?}"
    );
    run_until(
        &mut s,
        "the fresh client never said the transmitter is held",
        |s| tx_alarms(s).len() == 2,
    );
    let shown = tx_alarms(&s);
    assert_eq!(shown[0].1, lost, "{shown:?}");
    assert!(
        shown[1].1.starts_with("an earlier Nexus session (0x"),
        "{shown:?}"
    );
}

/// The control: an over whose unkey the radio confirms raises no transmitter alarm, so there is
/// nothing on screen to dismiss.
#[test]
fn a_confirmed_unkey_raises_no_tx_alarm() {
    let mut s = FlexScene::new(false);
    beacon(&s);
    run_until(&mut s, "the beacon never keyed", |s| keys(s) == 1);
    run_until(&mut s, "the beacon's over never ended", idle);
    s.run(1_000);
    assert!(unkeys(&s) >= 1, "premise: the over was unkeyed");
    assert!(tx_alarms(&s).is_empty(), "{:?}", tx_alarms(&s));
}

// ── Every Flex client is read for alarms, the ones in the monitor pool too ─────────────────────

/// The client's words when the radio never confirms an unkey.
const UNCONFIRMED: &str =
    "the radio did not confirm the unkey — it may still be transmitting. Check the radio now.";

/// The transmitter alarms on screen as `(radio id, radio name, text)`, in the order shown.
fn tx_alarms_by_radio(s: &FlexScene) -> Vec<(u64, String, String)> {
    let snap = serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap();
    snap["txAlarms"]
        .as_array()
        .map(|alarms| {
            alarms
                .iter()
                .map(|a| {
                    (
                        a["radioId"].as_u64().unwrap_or(u64::MAX),
                        a["radioName"].as_str().unwrap_or_default().to_string(),
                        a["text"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A second radio for the operator to switch the Flex radio (radio 0) away to: connected,
/// read-only and waiting in the monitor pool, as the monitor thread keeps one. Both radios are
/// named, so an alarm can be read for the radio it names. Returns the pool and the radio's id.
fn second_radio_in_the_pool(s: &FlexScene) -> (MonitorPool, u32) {
    let (cat, _, _) = mock_logging_rigctld();
    let mut e = s.engine.lock().unwrap();
    e.rename_radio(0, "FLEX-6400");
    let id = e.add_radio();
    e.rename_radio(id, "IC-7300");
    let profile = e.settings().radios.iter().find(|p| p.id == id).unwrap();
    let monitor = live_monitor(id, Transport::from_profile(profile), &cat);
    (Arc::new(MonitorConnections::new(vec![monitor])), id)
}

/// The operator switches the active radio from `from` to `to`, and the radio loop's handoff takes
/// the switch, as it does before each step.
fn switch(s: &mut FlexScene, pool: &MonitorPool, from: u32, to: u32) {
    s.engine.lock().unwrap().set_active_radio(to);
    let mut last_active = from;
    let pending = std::sync::atomic::AtomicBool::new(false);
    handoff_if_switched(
        &s.engine,
        pool,
        &mut s.rig,
        &mut s.state,
        &mut last_active,
        &pending,
    );
    assert_eq!(last_active, to, "premise: the handoff took the switch");
}

/// The Flex radio's client in the pool, if it is there: its session.
fn pooled_flex<T>(pool: &MonitorPool, read: impl Fn(&FlexDaemon) -> T) -> Option<T> {
    pool.lock()
        .unwrap()
        .iter()
        .find(|c| c.id == 0)
        .and_then(|c| c.rigctld_proc.as_ref().and_then(CatDaemon::flex))
        .map(read)
}

/// What the monitor thread wants kept for the Flex radio: the connection the switch handed it.
fn want_the_flex_radio(pool: &MonitorPool) -> Vec<(u32, Transport)> {
    let p = pool.lock().unwrap();
    let conn = p.iter().find(|c| c.id == 0);
    let conn = conn.expect("premise: the switch handed the Flex radio's client to the pool");
    vec![(0, conn.transport.clone())]
}

/// What the monitor thread opens for the Flex radio once its connection has died: a fresh client
/// of its own on the same radio, which remembers the sessions before it.
fn reopen_flex(
    radio: std::net::SocketAddr,
) -> impl FnMut(&Transport) -> (Rig, Option<CatDaemon>, Option<bool>) {
    move |_| match FlexDaemon::start(radio, 0) {
        Ok(d) => {
            let cat = format!("127.0.0.1:{}", d.local_addr().port());
            let rig = Rig::with_control(Some(cat), PttMode::Vox);
            (rig, Some(CatDaemon::Flex(d)), Some(true))
        }
        Err(_) => (Rig::vox(), None, Some(false)),
    }
}

/// Run the monitor thread's pass over the pool, as `monitor_loop` does (the pool reconciled, then
/// polled), until `done` holds, or fail with `what`.
fn monitor_until(
    s: &FlexScene,
    pool: &MonitorPool,
    active: u32,
    want: &[(u32, Transport)],
    mut reopen: impl FnMut(&Transport) -> (Rig, Option<CatDaemon>, Option<bool>),
    what: &str,
    done: impl Fn(&FlexScene) -> bool,
) {
    let pending = std::sync::atomic::AtomicBool::new(false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(s) {
        assert!(Instant::now() < deadline, "{what}");
        reconcile_pool_with_open(pool, want, active, &s.engine, now_unix_ms(), &mut reopen);
        poll_monitors(pool, active, &s.engine, &pending);
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// The Flex radio on air with a beacon, its unkey never confirmed, and the operator switching to
/// the second radio mid-over: the handoff unkeys the Flex radio and hands its client to the pool.
fn switched_away_mid_over() -> (FlexScene, MonitorPool, u32) {
    let mut s = FlexScene::with_faults(
        false,
        SimSession::v4_gui_client(),
        vec![tempo_flexsim::Fault::StuckTransmit],
        short_deadline(),
    );
    let (pool, other) = second_radio_in_the_pool(&s);
    beacon(&s);
    run_until(&mut s, "the beacon never keyed", |s| keys(s) == 1);
    switch(&mut s, &pool, 0, other);
    (s, pool, other)
}

/// ⭐ A CLIENT HANDED TO THE MONITOR POOL IS STILL READ. The operator switches radios while a beacon
/// is on the air: the handoff unkeys the Flex radio and hands its client to the pool, where the
/// monitor keeps it. The radio never confirms the unkey, and the client says the radio may still be
/// transmitting, in the pool, where nothing read it: only the active client's alarms reached the
/// operator. The alarm reaches the screen, and it names the radio it is about, not the one now
/// active.
#[test]
fn a_pooled_clients_alarm_reaches_the_screen_naming_its_radio() {
    let (s, pool, other) = switched_away_mid_over();
    let want = want_the_flex_radio(&pool);
    monitor_until(
        &s,
        &pool,
        other,
        &want,
        reopen_flex(s.sim.tcp_addr()),
        "the pooled client's alarm never reached the screen",
        |s| !tx_alarms_by_radio(s).is_empty(),
    );
    assert_eq!(
        tx_alarms_by_radio(&s)[0],
        (0, "FLEX-6400".to_string(), UNCONFIRMED.to_string())
    );
}

/// ⭐ …AND A CLIENT THE POOL OPENS IS READ FROM ITS START. The pooled client's session has ended, so
/// the monitor opens a fresh client for the radio. It finds the earlier session still holding the
/// transmitter, and says so while it lives in the pool: that too reaches the screen, naming the
/// radio, behind the first alarm.
#[test]
fn a_client_the_pool_opens_is_read_from_its_start() {
    let (s, pool, other) = switched_away_mid_over();
    let want = want_the_flex_radio(&pool);
    monitor_until(
        &s,
        &pool,
        other,
        &want,
        reopen_flex(s.sim.tcp_addr()),
        "the fresh client's alarm never reached the screen",
        |s| tx_alarms_by_radio(s).len() == 2,
    );
    let shown = tx_alarms_by_radio(&s);
    assert_eq!(shown[0].2, UNCONFIRMED, "{shown:?}");
    assert_eq!((shown[1].0, shown[1].1.as_str()), (0, "FLEX-6400"));
    assert!(
        shown[1].2.starts_with("an earlier Nexus session (0x"),
        "{shown:?}"
    );
    assert!(
        pooled_flex(&pool, FlexDaemon::is_alive) == Some(true),
        "premise: the alarm came from the live client in the pool"
    );
}

/// ⭐ …AND A CLIENT THE HANDOFF LETS GO IS READ FIRST. The pooled client's session ended before the
/// monitor looked at it again, and the operator switches back to the Flex radio. The handoff will
/// not adopt a dead connection, so it lets it go and the radio is opened afresh: the alarm the
/// client raised as it ended was dropped with it.
#[test]
fn a_dead_pooled_client_is_read_before_the_handoff_lets_it_go() {
    let (mut s, pool, other) = switched_away_mid_over();
    let deadline = Instant::now() + Duration::from_secs(10);
    while pooled_flex(&pool, FlexDaemon::is_alive) != Some(false) {
        assert!(
            Instant::now() < deadline,
            "premise: the pooled session never ended"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    switch(&mut s, &pool, other, 0);
    assert!(
        pooled_flex(&pool, FlexDaemon::is_alive).is_none(),
        "premise: the handoff let the dead client go"
    );
    assert_eq!(
        tx_alarms_by_radio(&s),
        vec![(0, "FLEX-6400".to_string(), UNCONFIRMED.to_string())]
    );
}

/// The control: a switch made mid-over on a radio that confirms the unkey leaves a client in the
/// pool with nothing to say, so nothing reaches the screen.
#[test]
fn a_switch_after_a_confirmed_unkey_puts_no_alarm_on_screen() {
    let mut s = FlexScene::new(false);
    let (pool, other) = second_radio_in_the_pool(&s);
    beacon(&s);
    run_until(&mut s, "the beacon never keyed", |s| keys(s) == 1);
    switch(&mut s, &pool, 0, other);
    let want = want_the_flex_radio(&pool);
    let confirmed = |_: &FlexScene| pooled_flex(&pool, |d| !d.session().snapshot().keyed);
    monitor_until(
        &s,
        &pool,
        other,
        &want,
        |_: &Transport| panic!("the pooled client's session ended"),
        "premise: the radio never confirmed the unkey",
        |s| confirmed(s) == Some(true),
    );
    // A second and a half more of the monitor's passes, where the alarm would have reached it.
    let end = Instant::now() + Duration::from_millis(1_500);
    monitor_until(
        &s,
        &pool,
        other,
        &want,
        |_: &Transport| panic!("the pooled client's session ended"),
        "the monitor's passes never ended",
        |_| Instant::now() >= end,
    );
    assert!(unkeys(&s) >= 1, "premise: the over was unkeyed");
    assert!(
        tx_alarms_by_radio(&s).is_empty(),
        "{:?}",
        tx_alarms_by_radio(&s)
    );
}

/// Whether the client would admit a key: nothing of ours keyed, and its readback has seen the
/// radio idle.
fn client_ready(s: &FlexScene) -> bool {
    s.state
        .rigctld_proc
        .as_ref()
        .and_then(CatDaemon::flex)
        .is_some_and(|d| {
            let session = d.session().snapshot();
            !session.keyed && session.transmit_ready
        })
}

/// The bundled radio letting go of the transmitter `ms` after an unkey (the interlock's last
/// READY, naming no client) instead of 430 ms.
fn slow_release(ms: u64) -> SimSession {
    let mut s = SimSession::v4_gui_client();
    for (pattern, rules) in &mut s.rules {
        if *pattern == Pattern::Exact("xmit 0".to_string()) {
            for item in rules.iter_mut().flat_map(|r| r.items.iter_mut()) {
                if let Item::Wait(wait) = item {
                    *wait = ms;
                }
            }
        }
    }
    s
}

/// ⭐ A REFUSED APRS KEY PLAYS NOTHING AND IS REPORTED. A beacon queued the moment the last one's
/// over has unkeyed meets a radio still letting go of the transmitter (three seconds here, so the
/// window is certain): the client refuses the key, so the packet never goes out. The loop played
/// it into the receiving radio anyway and said nothing. It is not played, the APRS status line
/// says the radio did not accept the key, and nothing sends the packet later. The control, in the
/// same scene: once the radio has let go, the next beacon keys.
#[test]
fn a_refused_aprs_key_plays_nothing_and_is_reported() {
    let mut s = FlexScene::with_session(false, slow_release(3_000));
    beacon(&s);
    run_until(&mut s, "the beacon never keyed", |s| keys(s) == 1);
    run_until(&mut s, "the beacon's over never unkeyed", |s| {
        unkeys(s) == 1 && s.state.tx_until_ms.is_none()
    });
    beacon(&s);
    run_until(&mut s, "the refused beacon was never reported", |s| {
        s.engine.lock().unwrap().aprs_tx_notice().is_some()
    });
    s.run(300);
    assert_eq!(
        keys(&s),
        1,
        "the radio was never keyed for the second beacon"
    );
    assert_eq!(
        s.backend.played.lock().unwrap().len(),
        1,
        "the refused frame was played into the receiving radio"
    );
    let notice = s
        .engine
        .lock()
        .unwrap()
        .aprs_tx_notice()
        .map(str::to_string);
    assert!(
        notice
            .as_deref()
            .is_some_and(|n| n.starts_with("APRS not sent: the radio did not accept the key")),
        "{notice:?}"
    );

    // The refused key still meets the loop's unkey (its idle self-heal, on the same tick), which
    // here lands while the client waits for the first one's proof, so the radio starts its
    // three-second release again. Wait for the client itself to read the radio idle.
    run_until(&mut s, "the client never read the radio idle", |s| {
        idle(s) && client_ready(s)
    });
    s.run(300);
    assert_eq!(keys(&s), 1, "the refused packet was not sent later");
    beacon(&s);
    run_until(&mut s, "the next beacon never keyed", |s| keys(s) == 2);
    assert_eq!(
        s.engine.lock().unwrap().aprs_tx_notice(),
        None,
        "a beacon that keys clears the notice"
    );
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

/// The engine's refusal on screen, if any, and whether TX is on.
fn refusal(s: &FlexScene) -> (Option<tempo_app::dto::SlotKeyRefused>, bool) {
    let e = s.engine.lock().unwrap();
    (e.snapshot().radio.slot_key_refused, e.tx_enabled())
}

/// ⭐ THE CLIENT'S OWN REASON REACHES THE SCREEN. A digital over the client keeps off the air for
/// the audio route halts TX like any refused key, but the rigctld wire carries only `RPRT -1`:
/// the refusal on screen carries the client's reason instead. Native audio comes on 900 ms before
/// a boundary, inside the routing's guard, so at the boundary the radio still takes its transmit
/// audio from its mic input and Nexus's transmit stream does not exist yet.
#[test]
fn a_refusal_for_the_audio_route_says_why_on_screen() {
    let mut s = FlexScene::new(false);
    s.run(1_500);
    let mut toggled = false;
    let mut during = |sc: &mut FlexScene, rel: f64| {
        if !toggled && rel >= -900.0 {
            toggled = true;
            sc.set_native_audio(true);
            // A Settings save clears the queued over (`apply_settings`); the CQ run goes on.
            let mut e = sc.engine.lock().unwrap();
            e.broadcast("CQ TEST W9XYZ EN37");
            let _ = e.take_immediate_tx();
        }
    };
    let (_, played) = one_over(
        &mut s,
        first_boundary(),
        1_500.0,
        false,
        1_000.0,
        &mut during,
    );
    let (refused, tx_enabled) = refusal(&s);
    assert!(
        played.is_none() && keys(&s) == 0 && !tx_enabled,
        "the premise: the client refused the key, nothing played, TX off"
    );
    assert_eq!(
        refused.map(|r| (r.why, r.flex_audio)),
        Some((
            "not keying a DIGU over: the radio takes its transmit audio from its mic input and \
             Nexus's DAX transmit stream does not exist yet"
                .to_string(),
            Some(tempo_app::dto::FlexAudioRefusal {
                mode: "DIGU".to_string(),
                cause: tempo_app::dto::FlexAudioCause::NotYetDax,
            })
        ))
    );
}

/// The scene with native audio on, the DAX source written for the slice's digital mode and the
/// tee in: the station a run of DAX overs starts from.
fn on_dax() -> FlexScene {
    let mut s = FlexScene::new(true);
    run_until(&mut s, "settle: the tee and the DAX source", |s| {
        s.backend.tee.lock().unwrap().is_some()
            && s.log()
                .iter()
                .any(|(_, e)| command(e, "transmit set dax=1"))
    });
    s.run(300);
    s
}

/// What came of an over, read by value: whether it keyed and played, whether DAX TX audio reached
/// the radio, whether TX is on, whether the operator's mic was back before the `boundary`, and the
/// refusal on screen (its words, and its cause as the UI receives it).
fn over_outcome(s: &FlexScene, boundary: Instant, played: Option<Instant>) -> String {
    let log = s.log();
    let mic_back = log
        .iter()
        .find(|(_, e)| command(e, "transmit set dax=0"))
        .is_some_and(|(t, _)| *t < boundary);
    let dax_tx_audio = log
        .iter()
        .any(|(_, e)| matches!(e, SimEvent::UdpIn { bytes, .. } if bytes.len() > 1));
    let (refused, tx_enabled) = refusal(s);
    let flex_audio = serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap()["radio"]
        ["slotKeyRefused"]["flexAudio"]
        .clone();
    format!(
        "xmit1={} played={} dax_tx_audio={dax_tx_audio} tx_enabled={tx_enabled} \
         mic_back_before_boundary={mic_back} why={:?} flex_audio={flex_audio}",
        keys(s),
        played.is_some(),
        refused.map(|r| r.why),
    )
}

/// The over a station on DAX keys after its operator turned native audio off `offset` ms from the
/// boundary, with the CQ queued again (a Settings save clears the queued over).
fn native_audio_off_at(offset: f64) -> String {
    let mut s = on_dax();
    let mut toggled = false;
    let mut during = |sc: &mut FlexScene, rel: f64| {
        if !toggled && rel >= offset {
            toggled = true;
            sc.set_native_audio(false);
            let mut e = sc.engine.lock().unwrap();
            e.broadcast("CQ TEST W9XYZ EN37");
            let _ = e.take_immediate_tx();
        }
    };
    let (at, played) = one_over(
        &mut s,
        first_boundary(),
        1_500.0,
        false,
        1_000.0,
        &mut during,
    );
    over_outcome(&s, at, played)
}

/// The refusal for an over the radio would have taken from a DAX nothing feeds, as
/// [`over_outcome`] reads it.
const DAX_UNFED: &str = "xmit1=0 played=false dax_tx_audio=false tx_enabled=false \
     mic_back_before_boundary=false why=Some(\"not keying a DIGU over: native audio is off, and \
     the radio still takes its transmit audio from the DAX Nexus set, which nothing feeds until \
     its mic input is back\") flex_audio={\"cause\":\"daxUnfed\",\"mode\":\"DIGU\"}";

/// ⭐⭐ NEVER A SILENT OVER (operator ruling, 2026-10-07, "Refuse that over"). Native audio goes
/// off inside the routing's guard before a boundary: the tee comes out at once, but the operator's
/// own setting (the mic) comes back only at the next quiet point, after the boundary, so the radio
/// would key that over on the DAX Nexus wrote with nothing feeding it. The client refuses the key,
/// in its own words: nothing keys, nothing is played to the sound card or the radio, and TX is off,
/// as for any refused key. The control: turned off before the guard, the mic is back first and
/// the over keys on the sound card.
#[test]
fn native_audio_off_inside_the_guard_refuses_the_over_it_would_leave_silent() {
    let offsets = [-900.0, -400.0, -40.0, 0.0, -1_300.0];
    let got: Vec<String> = offsets
        .iter()
        .map(|o| format!("off at {o:+}: {}", native_audio_off_at(*o)))
        .collect();
    let want: Vec<String> = offsets
        .iter()
        .map(|o| {
            let outcome = if *o < -ROUTING_GUARD_MS {
                "xmit1=1 played=true dax_tx_audio=false tx_enabled=true \
                 mic_back_before_boundary=true why=None flex_audio=null"
            } else {
                DAX_UNFED
            };
            format!("off at {o:+}: {outcome}")
        })
        .collect();
    assert_eq!(got.join("\n"), want.join("\n"));
}

/// ⭐ The receive floor's fallback is the other way native audio goes off (no DAX receive audio
/// for `DAX_STARVE_AFTER`: back to the sound card, said on screen), and inside the guard it leaves
/// the radio on the DAX Nexus wrote in the same way: the over is refused for the same reason, and
/// the Phone cockpit's "mic disconnected" (`flex_dax_tx`) holds until the radio has the mic back.
/// The receive stream is withheld here, and the floor's clock kept fresh until the chosen moment,
/// 900 ms before the boundary.
#[test]
fn a_receive_floor_fallback_inside_the_guard_refuses_the_over_too() {
    let mut s = FlexScene::with_faults(
        true,
        SimSession::v4_gui_client(),
        vec![tempo_flexsim::Fault::Vita {
            stream_id: 0x0400_0001,
            drop: (0..8_000).collect(),
            swap: Vec::new(),
        }],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !(s.backend.tee.lock().unwrap().is_some()
        && s.log()
            .iter()
            .any(|(_, e)| command(e, "transmit set dax=1")))
    {
        assert!(
            Instant::now() < deadline,
            "settle: the tee and the DAX source"
        );
        s.state.dax_last_audio = Some(Instant::now());
        s.run(100);
    }
    let mut fired = false;
    let mut mic_off: Vec<(f64, bool)> = Vec::new();
    let mut during = |sc: &mut FlexScene, rel: f64| {
        if fired {
        } else if rel >= -900.0 {
            fired = true;
            sc.state.dax_last_audio = Some(Instant::now() - DAX_STARVE_AFTER);
        } else {
            sc.state.dax_last_audio = Some(Instant::now());
        }
        mic_off.push((rel, sc.engine.lock().unwrap().snapshot().radio.flex_dax_tx));
    };
    let (at, played) = one_over(
        &mut s,
        first_boundary(),
        1_500.0,
        false,
        1_000.0,
        &mut during,
    );
    let mic_off_at = |when: f64| mic_off.iter().find(|(rel, _)| *rel >= when).map(|m| m.1);
    assert_eq!(
        format!(
            "{} audio_error={} mic_off: -800={:?} -100={:?} +900={:?}",
            over_outcome(&s, at, played),
            s.engine
                .lock()
                .unwrap()
                .snapshot()
                .radio
                .audio_error
                .is_some(),
            mic_off_at(-800.0),
            mic_off_at(-100.0),
            mic_off_at(900.0),
        ),
        format!(
            "{DAX_UNFED} audio_error=true mic_off: -800=Some(true) -100=Some(true) \
             +900=Some(false)"
        )
    );
}

/// The Phone cockpit's "mic disconnected" (`flex_dax_tx`) follows the radio, not the toggle:
/// native audio off 900 ms before a boundary leaves the radio on DAX until the operator's own
/// setting comes back after it, and the mic reads disconnected until then.
#[test]
fn the_mic_reads_disconnected_until_the_radio_has_it_back() {
    let mut s = on_dax();
    let mut toggled = false;
    let mut seen: Vec<(f64, bool, Option<bool>)> = Vec::new();
    let mut during = |sc: &mut FlexScene, rel: f64| {
        if !toggled && rel >= -900.0 {
            toggled = true;
            sc.set_native_audio(false);
        }
        let mic_off = sc.engine.lock().unwrap().snapshot().radio.flex_dax_tx;
        let dax = sc
            .state
            .rigctld_proc
            .as_ref()
            .and_then(CatDaemon::flex)
            .and_then(|d| d.session().snapshot().model.transmit.dax);
        seen.push((rel, mic_off, dax));
    };
    one_over(
        &mut s,
        first_boundary(),
        1_500.0,
        false,
        1_500.0,
        &mut during,
    );
    let at = |when: f64| {
        seen.iter()
            .find(|(rel, _, _)| *rel >= when)
            .map(|(_, mic_off, dax)| format!("mic_off={mic_off} dax={dax:?}"))
    };
    assert_eq!(
        format!("{:?} {:?}", at(-100.0), at(1_400.0)),
        "Some(\"mic_off=true dax=Some(true)\") Some(\"mic_off=false dax=Some(false)\")"
    );
}

/// The scene of [`on_dax`] on a radio whose DAX receive audio never arrives: the receive floor's
/// clock is kept fresh by hand, so the floor gives up on DAX only when a test lets it go stale.
fn on_dax_without_receive_audio() -> FlexScene {
    let mut s = FlexScene::with_faults(
        true,
        SimSession::v4_gui_client(),
        vec![tempo_flexsim::Fault::Vita {
            stream_id: 0x0400_0001,
            drop: (0..8_000).collect(),
            swap: Vec::new(),
        }],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !(s.backend.tee.lock().unwrap().is_some()
        && s.log()
            .iter()
            .any(|(_, e)| command(e, "transmit set dax=1")))
    {
        assert!(
            Instant::now() < deadline,
            "settle: the tee and the DAX source"
        );
        s.state.dax_last_audio = Some(Instant::now());
        s.run(100);
    }
    s
}

/// When a test takes an over's audio route away, from the virtual times of its key and its
/// deadline.
type When = fn(f64, f64) -> f64;

/// An over on DAX, keyed at an even boundary, whose audio route goes at the virtual time
/// `when(key, deadline)` names: the operator turns native audio off, or (`floor`) the receive
/// floor gives up on DAX. Read by value: whether it keyed and its audio reached the radio as DAX,
/// whether any DAX audio reached the radio 100 ms or more after the route went, when it unkeyed
/// (`tick+N`: N loop ticks after the first tick that ran with the route gone; `own end`: at its
/// own deadline, keyed until then), whether TX is on, what the status lane got (as the UI receives
/// it), and whether the receive floor's banner is up.
fn route_gone_mid_over(floor: bool, when: When) -> String {
    let mut s = if floor {
        on_dax_without_receive_audio()
    } else {
        on_dax()
    };
    {
        let mut e = s.engine.lock().unwrap();
        e.broadcast("CQ TEST W9XYZ EN37");
        // The boundary path, as every over after the first in a run keys.
        let _ = e.take_immediate_tx();
    }
    s.state.prev_slot_was_tx = true;
    let boundary = first_boundary();
    let w0 = Instant::now();
    let start_v = boundary - 1_500.0;
    let mut keyed: Option<f64> = None;
    let mut deadline = 0.0;
    let mut gone: Option<(usize, Instant, f64)> = None;
    let mut unkeyed: Option<(usize, f64)> = None;
    for tick in 0.. {
        let v = start_v + w0.elapsed().as_secs_f64() * 1000.0;
        if gone.is_none() {
            if keyed.is_some_and(|k| v >= when(k, deadline)) {
                gone = Some((tick, Instant::now(), v));
                if floor {
                    s.state.dax_last_audio = Some(Instant::now() - DAX_STARVE_AFTER);
                } else {
                    s.set_native_audio(false);
                }
            } else if floor {
                s.state.dax_last_audio = Some(Instant::now());
            }
        }
        s.step(v);
        if keyed.is_none() && s.state.tx_until_ms.is_some() {
            // From the key on the wire: the boundary's tick can take a while to return.
            keyed = Some(start_v + w0.elapsed().as_secs_f64() * 1000.0);
            deadline = s.state.slot_tx_until_ms;
        } else if keyed.is_some() && unkeyed.is_none() && s.state.tx_until_ms.is_none() {
            unkeyed = Some((tick, v));
        }
        let settled = gone
            .zip(unkeyed)
            .is_some_and(|((_, _, g), _)| v > g + 300.0);
        if settled || v > boundary + 14_000.0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (gone_tick, gone_at, _) = gone.expect("the over keyed and its route went");
    let log = s.log();
    let dax: Vec<Instant> = log
        .iter()
        .filter(|(_, e)| matches!(e, SimEvent::UdpIn { bytes, .. } if bytes.len() > 1))
        .map(|(t, _)| *t)
        .collect();
    let unkey = match unkeyed {
        Some((_, v)) if v >= deadline => "own end".to_string(),
        Some((tick, _)) => format!("tick+{}", tick - gone_tick),
        None => "none".to_string(),
    };
    let e = s.engine.lock().unwrap();
    let lane = serde_json::to_value(e.snapshot()).unwrap()["radio"]["slotAudioLost"]["at"].is_u64();
    format!(
        "xmit1={} audio_on_dax={} dax_audio_after={} unkey={unkey} tx_enabled={} lane={} \
         floor_banner={}",
        log.iter().filter(|(_, e)| command(e, "xmit 1")).count(),
        dax.iter().any(|t| *t < gone_at),
        dax.iter()
            .any(|t| t.saturating_duration_since(gone_at) >= Duration::from_millis(100)),
        e.tx_enabled(),
        if lane { "slotAudioLost" } else { "none" },
        e.snapshot().radio.audio_error.is_some(),
    )
}

/// ⭐⭐ NEVER A SILENT OVER, PART WAY THROUGH EITHER (operator ruling, 2026-10-08, "End the over").
/// Native audio going off in the middle of an over takes the DAX tee out with the over's audio
/// in it, and the radio stays keyed with nothing to send: the rest of the over was dead air, ending
/// only at its own deadline, 11–13 s later, with TX still on. Now the over ends in the tick after
/// the one that saw the route go, TX is off, and the status lane says why: at 100, 509 and 2,000 ms
/// into the over when the operator turns native audio off, and at 2,000 ms when the receive floor
/// gives up on DAX (its own banner stays up beside the lane). DAX audio stops either way, at once.
#[test]
fn native_audio_off_mid_over_ends_the_over() {
    let trials: [(&str, bool, When); 4] = [
        ("off +100", false, |k, _| k + 100.0),
        ("off +509", false, |k, _| k + 509.0),
        ("off +2000", false, |k, _| k + 2_000.0),
        ("floor +2000", true, |k, _| k + 2_000.0),
    ];
    let got: Vec<String> = trials
        .iter()
        .map(|(name, floor, when)| format!("{name}: {}", route_gone_mid_over(*floor, *when)))
        .collect();
    let want: Vec<String> = trials
        .iter()
        .map(|(name, floor, _)| {
            format!(
                "{name}: xmit1=1 audio_on_dax=true dax_audio_after=false unkey=tick+{} \
                 tx_enabled=false lane=slotAudioLost floor_banner={floor}",
                // The receive floor turns native audio off in the tick that takes the tee out;
                // the toggle is read after that tick's tee is settled, so the tee goes a tick on.
                if *floor { 0 } else { 1 }
            )
        })
        .collect();
    assert_eq!(got.join("\n"), want.join("\n"));
}

/// The control: native audio off once the over's audio has ended, 100 ms into the PTT tail before
/// its unkey, changes nothing. The over unkeys at its own deadline, as every over does, TX stays
/// on, and the lane gets nothing: the over was whole.
#[test]
fn native_audio_off_in_an_overs_tail_changes_nothing() {
    assert_eq!(
        route_gone_mid_over(false, |_, deadline| deadline - crate::slot::TX_TAIL_MS
            + 100.0),
        "xmit1=1 audio_on_dax=true dax_audio_after=false unkey=own end tx_enabled=true \
         lane=none floor_banner=false"
    );
}

/// The rule above ends a SLOT over (FT8, FT4, JS8 and the other timed-slot modes), as the
/// refused-key and failed-unkey halts are a slot over's. Another over Nexus plays over DAX, here a
/// recorded message, is left as it was when native audio goes off under it: TX stays on and the
/// lane gets nothing. Halting TX there would leave the Phone screen, where the voice keyer lives,
/// with no TX switch to turn it back on.
#[test]
fn native_audio_off_mid_message_leaves_the_voice_keyer_as_it_was() {
    let mut s = on_dax();
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    run_until(&mut s, "the message never keyed", |s| keys(s) == 1);
    s.run(300);
    assert!(
        s.log()
            .iter()
            .any(|(_, e)| matches!(e, SimEvent::UdpIn { bytes, .. } if bytes.len() > 1)),
        "premise: the message went out as DAX"
    );
    s.set_native_audio(false);
    s.run(1_500);
    let e = s.engine.lock().unwrap();
    let lane = serde_json::to_value(e.snapshot()).unwrap()["radio"]["slotAudioLost"].is_object();
    assert_eq!(
        format!("tx_enabled={} lane={lane}", e.tx_enabled()),
        "tx_enabled=true lane=false"
    );
}

/// The operator's Phone key on a station on DAX for FT8, made as the loop sees the switch to
/// Phone (`ticks_between` 0: in the same tick) or that many ticks after it, with native audio
/// turned off at the same moment as the switch when `native_off`. Read by value, for that press and
/// for a second one once it is let go and the radio has its mic back: the source each `xmit 1`
/// went out on (DAX until a `transmit set dax=0` goes before it), and after each press the status
/// lane's refusal (its mode, as the UI receives it) and whether the PTT banner is up.
fn phone_key_after_ft8(native_off: bool, ticks_between: usize) -> String {
    let mut s = FlexScene::with_session(true, reports(&["USB"]));
    run_until(&mut s, "settle: the tee and the DAX source", |s| {
        s.backend.tee.lock().unwrap().is_some()
            && s.log()
                .iter()
                .any(|(_, e)| command(e, "transmit set dax=1"))
    });
    s.run(300);
    let switched = Instant::now();
    if native_off {
        s.set_native_audio(false);
    }
    {
        let mut e = s.engine.lock().unwrap();
        e.set_operating_mode("phone", false); // arms TX
        e.set_frequency(14.250, "20m", "USB");
        if ticks_between == 0 {
            e.set_ptt(true);
        }
    }
    for _ in 0..ticks_between {
        s.step(now_unix_ms());
        std::thread::sleep(Duration::from_millis(20));
    }
    if ticks_between > 0 {
        s.engine.lock().unwrap().set_ptt(true);
    }
    s.run(500);
    let after = |s: &FlexScene| {
        let e = s.engine.lock().unwrap();
        format!(
            "lane={} banner={}",
            serde_json::to_value(e.snapshot()).unwrap()["radio"]["pttRefused"]["mode"],
            e.snapshot().radio.audio_error.is_some()
        )
    };
    let first = after(&s);
    s.engine.lock().unwrap().set_ptt(false);
    // Its mic back, and a first press that keyed let go of, as the client's readback proves it:
    // its admission refuses a key until then.
    run_until(&mut s, "the radio never had its mic back", |s| {
        idle(s)
            && s.state
                .rigctld_proc
                .as_ref()
                .and_then(CatDaemon::flex)
                .is_some_and(|d| {
                    let snap = d.session().snapshot();
                    snap.transmit_ready && snap.model.transmit.dax == Some(false)
                })
    });
    s.run(200);
    s.engine.lock().unwrap().set_ptt(true);
    s.run(400);
    let second = after(&s);
    s.engine.lock().unwrap().set_ptt(false);
    s.run(300);
    let mut dax = true;
    let mut keyed_on = Vec::new();
    for (_, e) in s.log().iter().filter(|(t, _)| *t >= switched) {
        match e {
            SimEvent::Command { text, .. } if text == "transmit set dax=0" => dax = false,
            SimEvent::Command { text, .. } if text == "transmit set dax=1" => dax = true,
            SimEvent::Command { text, .. } if text == "xmit 1" => {
                keyed_on.push(if dax { "DAX" } else { "mic" })
            }
            _ => {}
        }
    }
    format!("keyed on {keyed_on:?}; first press: {first}; second press: {second}")
}

/// ⭐⭐ NOR A SILENT PHONE OVER (operator ruling, 2026-10-08, "Same rule for Phone"). A station on
/// DAX for FT8 switches to Phone: the radio takes its transmit audio from the DAX Nexus set until
/// the routing's quiet point at the end of the tick puts the operator's mic back. A key the loop
/// sees in that same tick (a press made with the switch, or held through it) went out on DAX, and
/// with the key held no quiet point came, so the whole over was silent, the mic unused, with no
/// word on screen. It is now refused, with the reason on the status lane in place of the PTT advice,
/// until the radio has its mic back; the next press keys on the mic. The same with native audio
/// turned off at the same moment. The control: a press the loop sees a tick later already went
/// out after the mic's write, before as now, and nothing is refused.
#[test]
fn a_phone_key_is_refused_while_the_radio_is_still_on_the_dax_nexus_set() {
    let cases = [
        ("native on, same tick", false, 0),
        ("native off, same tick", true, 0),
        ("native on, next tick", false, 1),
        ("native off, next tick", true, 1),
    ];
    let got: Vec<String> = cases
        .iter()
        .map(|(name, off, ticks)| format!("{name}: {}", phone_key_after_ft8(*off, *ticks)))
        .collect();
    let want: Vec<String> = cases
        .iter()
        .map(|(name, _, ticks)| {
            let outcome = if *ticks == 0 {
                "keyed on [\"mic\"]; first press: lane=\"USB\" banner=false; second press: \
                 lane=null banner=false"
            } else {
                "keyed on [\"mic\", \"mic\"]; first press: lane=null banner=false; second press: \
                 lane=null banner=false"
            };
            format!("{name}: {outcome}")
        })
        .collect();
    assert_eq!(got.join("\n"), want.join("\n"));
}

/// The lane above is the Phone key's alone. A key the client keeps off the air for another
/// cause, here a manual key in FT8 on the radio's mic as native audio comes on, keeps the PTT
/// banner it always had, and the lane says nothing of a radio on DAX in place of its mic.
#[test]
fn a_key_kept_off_the_air_for_another_cause_keeps_the_ptt_banner() {
    let mut s = FlexScene::new(true);
    s.engine.lock().unwrap().set_ptt(true);
    s.run(300);
    let (lane, banner) = {
        let e = s.engine.lock().unwrap();
        (
            serde_json::to_value(e.snapshot()).unwrap()["radio"]["pttRefused"].clone(),
            e.snapshot().radio.audio_error.is_some(),
        )
    };
    assert_eq!(
        format!("xmit1={} lane={lane} banner={banner}", keys(&s)),
        "xmit1=0 lane=null banner=true"
    );
}

/// ⭐⭐ THE KEYING PATH NEVER CARRIES THE FLAG (operator ruling, 2026-10-03: "write the flag when
/// the mode or the TX slice changes, never between the slot boundary and `xmit 1`"). FT8 with TX
/// enabled and an over queued every TX period, while the operator turns native audio off and on at
/// moments chosen to land just before, at and just after the boundary, so the flag must change
/// across overs. Every `transmit set dax` the radio receives is checked against every over: never
/// between that over's boundary and its `xmit 1`, never while it is keyed (from `xmit 1` to the
/// radio's READY naming no transmitter), and never within the loop's guard of a boundary, either
/// side. The flag does change: the writes happen, between overs. And a toggle inside the guard
/// before a boundary has that over refused, whichever way it went: never keyed on the mic, never
/// keyed on a DAX nothing feeds; the late toggle that turns native audio off ends its over there
/// (operator ruling, 2026-10-08, "End the over").
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
    // The boundaries of the overs ended there because native audio went off under them.
    let mut ended: Vec<Instant> = Vec::new();
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
        let (at, played) = one_over(&mut s, boundary, pre, false, 1_000.0, &mut during);
        let xmit = s
            .log()
            .iter()
            .filter(|(_, e)| command(e, "xmit 1"))
            .map(|(t, _)| *t)
            .nth(xmits_before);
        // A toggle inside the guard before the boundary leaves the radio's source as it was until
        // the quiet point after the boundary, so the client refuses that over's key: native audio
        // just on, the radio still takes its transmit audio from its mic input; just off, from
        // DAX, which nothing feeds then. A refused slot key plays nothing and halts TX, as WSJT-X
        // halts on a rig failure (`slot::slot_key_failure`); the next trial's CQ arms TX again.
        // After the boundary the over may have keyed first.
        let (refused, tx_enabled) = refusal(&s);
        if (-ROUTING_GUARD_MS..=0.0).contains(offset) {
            let cause = format!("{:?}", refused.and_then(|r| r.flex_audio).map(|f| f.cause));
            let want = if native {
                "Some(NotYetDax)"
            } else {
                "Some(DaxUnfed)"
            };
            assert_eq!(
                (xmit.is_none(), played.is_none(), tx_enabled, cause.as_str()),
                (true, true, false, want),
                "toggle {offset:+} (native audio {}): the client must refuse the key, play \
                 nothing and leave TX off",
                if native { "on" } else { "off" }
            );
        } else if xmit.is_none() {
            assert!(
                played.is_none() && !tx_enabled,
                "toggle {offset:+}: the client refused the key, and the over was played or TX \
                 left on"
            );
        }
        // Native audio turned off after the over keyed takes its audio with the tee: the over ends
        // there and TX halts. Only that toggle: off, after the key.
        if s.engine
            .lock()
            .unwrap()
            .snapshot()
            .radio
            .slot_audio_lost
            .is_some()
        {
            assert!(
                xmit.is_some() && !native && !tx_enabled && *offset > 0.0,
                "toggle {offset:+}: an over was ended for its lost audio"
            );
            ended.push(at);
        }
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
    // The six toggles inside the guard are refused (asserted above); the one before it and the
    // two well after the boundary always key, and the one at +15 ms when the boundary's tick
    // comes first.
    assert!(
        keyed_overs >= 3,
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
        // The guard either side of every boundary an over keyed at, less the session's delivery
        // (≤ one read poll). After a refused key, or an over ended for its lost audio, TX is
        // halted (asserted above), and with no slot transmission armed the quiet point holds no
        // write off a boundary (`RadioLoop::tx_routing_quiet`): nothing keys after it.
        for (at, _) in overs
            .iter()
            .filter(|(at, xmit)| xmit.is_some() && !ended.contains(at))
        {
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
    assert_eq!(
        ended.len(),
        1,
        "the one late toggle that turns native audio off ends its over\n{}",
        trace.join("\n")
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

// ── The client's belief in the loop's keyed set ──────────────────────────────────────────────

/// The bundled session with the radio reported idle when Nexus subscribes and no interlock on the
/// new slice, so the transmitter's holder the simulator remembers (its answer to `sub tx all`) is
/// the last word.
fn holder_kept() -> SimSession {
    let mut session = SimSession::v4_gui_client();
    for (pattern, rules) in &mut session.rules {
        match pattern {
            Pattern::Prefix(p) if p == "slice create " => {
                for r in rules.iter_mut() {
                    r.items
                        .retain(|i| !matches!(i, Item::Send(l) if l.contains("|interlock ")));
                }
            }
            Pattern::Exact(p) if p == "sub tx all" => {
                for r in rules.iter_mut() {
                    for item in r.items.iter_mut() {
                        if matches!(item, Item::Send(l) if l.contains("state=RECEIVE")) {
                            *item = Item::Send(
                                "S0|interlock tx_client_handle=0x00000000 state=READY reason= \
                                 source= tx_allowed=1 amplifier="
                                    .to_string(),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    session
}

/// An earlier session of ours that started the radio's tune carrier and was lost, played by a raw
/// session: `transmit tune 1`, then gone. Under StuckTune the carrier stays up under its handle.
fn tune_and_leave(sim: &Simulator) -> u32 {
    use std::io::{BufRead, BufReader, Write};
    let stream = std::net::TcpStream::connect(sim.tcp_addr()).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    let handle = loop {
        line.clear();
        reader.read_line(&mut line).expect("the prologue");
        if let Some(h) = line.trim().strip_prefix('H') {
            break u32::from_str_radix(h, 16).expect("a handle");
        }
    };
    (&stream).write_all(b"C1|transmit tune 1\n").unwrap();
    loop {
        line.clear();
        reader.read_line(&mut line).expect("the reply");
        if line.starts_with("R1|") {
            assert_eq!(line.trim(), "R1|0|");
            break handle;
        }
    }
}

/// The scene with an earlier session's tune carrier still up: the client knows that session's
/// handle, as a client that replaces it does, and believes something of ours is on the air; the
/// loop's own state does not.
fn with_a_lost_tune() -> FlexScene {
    let sim = FlexScene::radio(holder_kept(), vec![tempo_flexsim::Fault::StuckTune]);
    let lost = tune_and_leave(&sim);
    crate::flex::remember_handle(sim.tcp_addr(), lost);
    let mut config = tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap());
    config.previous_handles = vec![lost];
    let s = FlexScene::on(sim, false, config);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !s.state.flex_keyed() {
        assert!(
            Instant::now() < deadline,
            "the client never saw the lost tune"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!s.rig.keyed, "the loop never keyed it");
    s
}

/// The commands one connection has sent so far, pings left out.
fn sent(log: &[tempo_flexsim::Logged], conn: usize) -> Vec<String> {
    log.iter()
        .filter_map(|l| match &l.event {
            SimEvent::Command { conn: c, text, .. } if *c == conn && text != "ping" => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

/// [`sent`], once `want` is among them (or the wait ends).
fn sent_on(s: &FlexScene, conn: usize, want: &str) -> Vec<String> {
    s.sim.wait_for(Duration::from_secs(10), |log| {
        sent(log, conn).iter().any(|c| c == want)
    });
    sent(&s.sim.log(), conn)
}

/// A saved CAT change: the transport differs, so the next tick rebuilds the link.
fn save_a_cat_change(s: &FlexScene) {
    let mut e = s.engine.lock().unwrap();
    let mut settings = e.settings().clone();
    settings.rig_addr = "127.0.0.1:5003".into();
    e.apply_settings(settings);
}

/// ⭐ A REBUILD WHILE A NATIVE TUNE IS UP STOPS IT THROUGH THE NEW SESSION. The radio still holds
/// an earlier session's tune carrier; the loop rebuilds the link and its unkey through the fresh
/// client is `T 0`, which must end the tune with the tune's own stop: an `xmit 0` alone would leave
/// the carrier up. The loop believes the radio keyed until that unkey has gone through.
#[test]
fn a_rebuild_while_a_native_tune_is_keyed_stops_it_through_the_new_session() {
    let mut s = with_a_lost_tune();
    s.replace_on_rebuild = true;
    save_a_cat_change(&s);
    s.step(now_unix_ms());
    assert!(s.rebuilt.load(std::sync::atomic::Ordering::Relaxed));
    // The connections: the lost session, the scene's client, the fresh client.
    let fresh = sent_on(&s, 2, "xmit 0");
    let at = |text: &str| {
        fresh
            .iter()
            .position(|c| c == text)
            .unwrap_or_else(|| panic!("no {text:?} in {fresh:?}"))
    };
    assert!(at("transmit tune 0") < at("cwx clear"), "{fresh:?}");
    assert!(at("cwx clear") < at("xmit 0"), "{fresh:?}");
    assert!(!s.rig.keyed, "the unkey went through the fresh client");
}

/// ⭐ …and the belief is carried onto the fresh rig when its channel refuses that unkey, so the idle
/// self-heal goes on sending it: the client's belief is part of the loop's keyed set, which a
/// rebuild must never forget.
#[test]
fn a_rebuild_carries_the_flex_clients_belief_onto_the_fresh_rig() {
    let mut s = with_a_lost_tune();
    save_a_cat_change(&s);
    let refuse = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let (addr, _, log) = mock_logging_rigctld_refusing_unkey(refuse);
    let sinks = no_sinks();
    let mut station = StationSinks::new();
    let backend = s.backend.clone();
    let mut reopen_audio = move |_t: &Transport| Ok::<_, String>(backend.clone());
    let mut reopen_rig = move |_t: &Transport, _coexist: bool| {
        (
            Rig::rigctld(&addr),
            None,
            CatProbe::status(Some(true), "reopened"),
        )
    };
    s.state
        .step(
            &s.engine,
            &mut s.backend,
            &mut s.rig,
            &sinks,
            now_unix_ms(),
            &mut reopen_audio,
            &mut reopen_rig,
            &mut station,
        )
        .unwrap();
    assert!(
        log.lock().unwrap().iter().any(|l| l == "T 0"),
        "the post-reopen unkey reached the fresh channel: {:?}",
        log.lock().unwrap()
    );
    assert!(
        s.rig.keyed,
        "the fresh channel refused the unkey: the loop still believes the radio keyed"
    );
}

/// ⭐ STOP TX REACHES A FLEX TRANSMISSION THE LOOP DID NOT KEY. The loop's own state is idle, but
/// the client believes something of ours is on the air (here an earlier session's tune carrier):
/// the hard stop sends `T 0`, and the client ends it with the tune's own stop. Stop TX's CW stop
/// (`\stop_morse`) goes first, as it always has; it reaches the earlier session's CWX buffer.
#[test]
fn stop_tx_reaches_a_flex_transmission_the_loop_did_not_key() {
    let mut s = with_a_lost_tune();
    s.step(now_unix_ms());
    let before = sent(&s.sim.log(), 1).len();
    s.engine.lock().unwrap().halt_tx();
    s.step(now_unix_ms());
    let after: Vec<String> = sent_on(&s, 1, "xmit 0")[before..]
        .iter()
        .filter(|c| c.starts_with("xmit") || c.starts_with("transmit tune") || c.starts_with("cwx"))
        .cloned()
        .collect();
    assert_eq!(
        after,
        ["cwx clear", "transmit tune 0", "cwx clear", "xmit 0"],
        "the CW stop, then the hard stop's T 0"
    );
}

// ── CW through the radio's keyer: switched on in tests only ─────────────────────────────────
//
// Admission refuses every CWX word in production (`tempo_net::flex::admission::BENCHED`) until a
// tester's bench confirms its readback. These tests open the door the way only tests can
// (tempo-net's `flex-unbenched`), so the loop runs the path behind it on the checks it will run on
// once switched on.

/// The client's configuration with the door past the bench's list open.
fn unbenched() -> tempo_net::flex::session::Config {
    let mut c = tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap());
    c.unbenched = true;
    c
}

/// The served slice's mode, as the client reports it in rigctld words.
fn slice_mode(s: &FlexScene) -> Option<String> {
    let d = s.state.rigctld_proc.as_ref().and_then(CatDaemon::flex)?;
    d.slices().into_iter().find(|r| r.index == 0)?.mode
}

/// The loop's CW section, the CAT keyer at 20 WPM, on a radio that reports its slice's mode
/// changes, its client on `config`; the slice in CW.
fn cw_scene(
    faults: Vec<tempo_flexsim::Fault>,
    config: tempo_net::flex::session::Config,
) -> FlexScene {
    cw_scene_on(reports(&["CW", "DIGU"]), faults, config)
}

/// [`cw_scene`] on a radio that follows `session`, which must report the slice's mode changes.
fn cw_scene_on(
    session: SimSession,
    faults: Vec<tempo_flexsim::Fault>,
    config: tempo_net::flex::session::Config,
) -> FlexScene {
    let mut s = FlexScene::with_faults(false, session, faults, config);
    {
        let mut e = s.engine.lock().unwrap();
        e.set_cw_keyer("cat", 600.0);
        e.set_cw_wpm(20);
        e.set_operating_mode("cw", false);
        e.set_frequency(14.03, "20m", "CW");
    }
    run_until(&mut s, "the slice in CW", |s| {
        slice_mode(s).as_deref() == Some("CW")
    });
    s
}

/// The CW keyer's commands on the radio's wire, and the unkey, in order.
fn cw_wire(s: &FlexScene) -> Vec<String> {
    s.log()
        .into_iter()
        .filter_map(|(_, e)| match e {
            SimEvent::Command { text, .. } if text.starts_with("cwx ") || text == "xmit 0" => {
                Some(text)
            }
            _ => None,
        })
        .collect()
}

/// What the CW screen's warning line says.
fn cw_line(s: &FlexScene) -> Option<String> {
    s.engine.lock().unwrap().cw_keyer_error()
}

/// Whether the client believes something of ours is on the air.
fn flex_on_air(s: &FlexScene) -> bool {
    s.state.flex_keyed()
}

/// ⭐ A message goes out one word per `cwx send`, at the classic pacing: each word is handed over
/// once the one before has had its keying time plus a word space (7 dits), so at most one word sits
/// in the radio's buffer and Stop TX can drop the rest. On the loop's own clock, 20 ms a step, each
/// gap is that time to within one step. The radio's break-in ends the message: no stop, no alarm,
/// nothing on the CW line.
#[test]
fn a_macro_goes_out_one_word_per_cwx_send_at_the_classic_pacing() {
    let mut s = cw_scene(vec![], unbenched());
    s.engine.lock().unwrap().send_cw("CQ TEST DE W9XYZ");
    let words = ["CQ", "TEST", "DE", "W9XYZ"];
    // The loop's clock steps 20 ms at the real clock's pace: the client and the radio keep real
    // time. A word is handed over on the step whose clock it was paced to.
    let (start, zero) = (Instant::now(), now_unix_ms());
    let mut handed = Vec::new();
    for i in 0..600u32 {
        let now = zero + 20.0 * f64::from(i);
        let busy = s.state.cw_busy_until;
        s.step(now);
        if s.state.cw_busy_until != busy {
            handed.push(now - zero);
        }
        let next = start + Duration::from_millis(20 * u64::from(i + 1));
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
        if handed.len() == words.len() && !flex_on_air(&s) {
            break;
        }
    }
    assert_eq!(handed.len(), words.len(), "{handed:?}");
    let dit = 1200.0 / 20.0;
    for (i, pair) in handed.windows(2).enumerate() {
        let paced = tempo_core::cw::morse_duration_ms(words[i], 20) + 7.0 * dit;
        let gap = pair[1] - pair[0];
        assert!(
            gap >= paced && gap < paced + 20.0,
            "{}: {gap} ms after it, paced {paced}",
            words[i + 1]
        );
    }
    assert_eq!(
        cw_wire(&s),
        [
            "cwx send \"CQ\" 1",
            "cwx send \"TEST\" 2",
            "cwx send \"DE\" 3",
            "cwx send \"W9XYZ\" 4"
        ]
    );
    assert!(!flex_on_air(&s), "the radio's break-in ended it");
    assert_eq!(cw_line(&s), None);
    assert_eq!(tx_alarms(&s), Vec::<(u64, String)>::new());
}

/// ⭐ Stop TX in the middle of a message (the CW screen's: the CW abort and Halt) clears the
/// radio's CW buffer at once, `cwx clear` from `\stop_morse` and again from the hard stop's `T 0`,
/// and nothing goes out after it. The radio's break-in ends the word that was going out: no unkey
/// follows. On a radio that still shows our CWX transmitting after the clear, the unkey does, and
/// its release ends it. No alarm either way.
#[test]
fn stop_tx_mid_macro_clears_and_sends_nothing_after() {
    for held in [false, true] {
        let faults = if held {
            vec![tempo_flexsim::Fault::HoldsCwx]
        } else {
            Vec::new()
        };
        let mut s = cw_scene(faults, unbenched());
        s.engine.lock().unwrap().send_cw("CQ TEST DE W9XYZ");
        run_until(&mut s, "the first word", |s| !cw_wire(s).is_empty());
        {
            let mut e = s.engine.lock().unwrap();
            e.stop_cw();
            e.halt_tx();
        }
        run_until(&mut s, "the end proven", |s| !flex_on_air(s));
        s.run(3_000);
        let mut want = vec!["cwx send \"CQ\" 1", "cwx clear", "cwx clear"];
        if held {
            want.push("xmit 0");
        }
        assert_eq!(cw_wire(&s), want, "held: {held}");
        assert_eq!(tx_alarms(&s), Vec::<(u64, String)>::new(), "held: {held}");
    }
}

/// ⭐ The CW ID after an FT 73 stays unsent on a DIGU slice. It is queued from the FT section with
/// the slice in DIGU (what the loop does once the 73 has left the air), the client refuses it
/// there, nothing reaches the radio's keyer, and the CW line says why. That is today's outcome,
/// kept: sending it would take a mode change inside FT's QSO handling. The control: the same send
/// from the CW section goes out.
#[test]
fn the_cw_id_after_73_stays_refused_on_a_digu_slice() {
    let mut s = FlexScene::with_faults(false, reports(&["CW", "DIGU"]), Vec::new(), unbenched());
    run_until(&mut s, "the slice in DIGU", |s| {
        slice_mode(s).as_deref() == Some("PKTUSB")
    });
    {
        let mut e = s.engine.lock().unwrap();
        let mycall = e.settings().mycall.clone();
        e.send_cw(&mycall);
    }
    s.run(1_500);
    assert_eq!(cw_wire(&s), Vec::<String>::new());
    assert_eq!(
        cw_line(&s).as_deref(),
        Some(
            "CW not sent: the radio's transmit slice is in DIGU, not CW. Nexus sends CW to the \
             radio's keyer only in CW."
        )
    );
    let mut s = cw_scene(Vec::new(), unbenched());
    s.engine.lock().unwrap().send_cw("W9XYZ");
    run_until(&mut s, "the control's word", |s| !cw_wire(s).is_empty());
    assert_eq!(cw_wire(&s), ["cwx send \"W9XYZ\" 1"]);
}

/// ⭐ A word the client refuses drops the rest of the message, as a refused key does, so the
/// message cannot resume part way once the radio would take a word: here the first word is
/// refused on a DIGU slice, the slice is in CW by the time the second word is due, and nothing goes
/// out. As it ships, the CW line says the client does not send CW yet, in place of the Hamlib
/// advice, which does not apply to it.
#[test]
fn a_refused_word_drops_the_rest_of_the_message_and_says_why() {
    let mut s = FlexScene::with_faults(false, reports(&["CW", "DIGU"]), Vec::new(), unbenched());
    run_until(&mut s, "the slice in DIGU", |s| {
        slice_mode(s).as_deref() == Some("PKTUSB")
    });
    s.engine.lock().unwrap().send_cw("CQ TEST DE W9XYZ");
    run_until(&mut s, "the first word refused", |s| cw_line(s).is_some());
    {
        let mut e = s.engine.lock().unwrap();
        e.set_cw_keyer("cat", 600.0);
        e.set_operating_mode("cw", false);
    }
    run_until(&mut s, "the slice in CW", |s| {
        slice_mode(s).as_deref() == Some("CW")
    });
    s.run(3_000);
    assert_eq!(cw_wire(&s), Vec::<String>::new());

    // As it ships.
    let mut s = cw_scene(
        Vec::new(),
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    s.engine.lock().unwrap().send_cw("CQ TEST");
    run_until(&mut s, "the word refused", |s| cw_line(s).is_some());
    assert_eq!(
        cw_line(&s).as_deref(),
        Some(
            "CW not sent: the Flex native client does not send CW yet. For CW, turn the Flex \
             native client off (SmartSDR CAT sends it), or use the WinKeyer or Soundcard keyer."
        )
    );
    s.run(3_000);
    assert_eq!(cw_wire(&s), Vec::<String>::new());
}

/// The client's configuration as it ships: the door past the bench's list shut.
fn shipped() -> tempo_net::flex::session::Config {
    tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap())
}

/// The scene's Flex client.
fn client(s: &FlexScene) -> &FlexDaemon {
    s.state
        .rigctld_proc
        .as_ref()
        .and_then(CatDaemon::flex)
        .expect("the scene's client")
}

/// The bundled session, its radio reporting `wpm` once it is told `cw wpm <wpm>`.
fn speed_reported(mut session: SimSession, wpm: u32) -> SimSession {
    let rule = (
        Pattern::Exact(format!("cw wpm {wpm}")),
        vec![Rule {
            code: "0".to_string(),
            message: String::new(),
            items: vec![Item::Send(format!("S0|transmit speed={wpm}"))],
        }],
    );
    session.rules.retain(|(p, _)| *p != rule.0);
    session.rules.push(rule);
    session
}

/// The CW keyer's speed commands on the radio's wire.
fn speed_wire(s: &FlexScene) -> Vec<String> {
    s.log()
        .into_iter()
        .filter_map(|(_, e)| match e {
            SimEvent::Command { text, .. } if text.starts_with("cw wpm") => Some(text),
            _ => None,
        })
        .collect()
}

/// ⭐ A SPEED ANOTHER PROGRAM SETS ON THE RADIO IS FOLLOWED, NEVER FOUGHT. The radio's keyer sends
/// each word at the speed it reports. With the CW screen at 20 WPM, another program sets the radio
/// to 30 (here a command no part of the loop sent): the WPM control becomes 30, so the CW screen
/// shows what the radio keys at; the next message is paced at 30, its second word handed over the
/// first word's keying time at 30 plus a word space after it, to within one 20 ms step; and Nexus
/// sends no speed back, so the only `cw wpm` on the wire is the other program's. Nexus's own speed
/// is never taken back either: the operator sets 25, the next word carries it to the radio, the
/// operator moves on to 28 before the radio's report of 25 is read, and the control stays at 28. As
/// it ships, with the client's CW off, the radio's speed leaves the control alone.
#[test]
fn a_speed_another_program_sets_on_the_radio_is_followed_never_fought() {
    let other_program_sets_30 = |s: &mut FlexScene| {
        s.run(500);
        let reply = client(s).session().request(
            tempo_net::flex::encode::Command::CwSpeed { wpm: 30 },
            Duration::from_secs(3),
        );
        assert!(matches!(reply, Ok(r) if r.code == 0), "the radio took 30");
        s.run(500);
        s.engine.lock().unwrap().cw_wpm()
    };
    let session = speed_reported(speed_reported(reports(&["CW", "DIGU"]), 30), 25);
    let mut s = cw_scene_on(session.clone(), Vec::new(), unbenched());
    let shown = other_program_sets_30(&mut s);
    s.engine.lock().unwrap().send_cw("CQ TEST");
    let (start, zero) = (Instant::now(), now_unix_ms());
    let mut handed = Vec::new();
    for i in 0..400u32 {
        let now = zero + 20.0 * f64::from(i);
        let busy = s.state.cw_busy_until;
        s.step(now);
        if s.state.cw_busy_until != busy {
            handed.push(now - zero);
        }
        let next = start + Duration::from_millis(20 * u64::from(i + 1));
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
        if handed.len() == 2 && !flex_on_air(&s) {
            break;
        }
    }
    let paced = tempo_core::cw::morse_duration_ms("CQ", 30) + 7.0 * 1200.0 / 30.0;
    let gap = handed.get(1).zip(handed.first()).map(|(b, a)| b - a);
    assert_eq!(
        format!(
            "shown={shown} paced_at_30={} speed_wire={:?} words={:?}",
            gap.is_some_and(|g| g >= paced && g < paced + 20.0),
            speed_wire(&s),
            cw_wire(&s)
        ),
        "shown=30 paced_at_30=true speed_wire=[\"cw wpm 30\"] words=[\"cwx send \\\"CQ\\\" 1\", \
         \"cwx send \\\"TEST\\\" 2\"]",
        "the second word {gap:?} ms after the first, paced {paced}"
    );
    s.engine.lock().unwrap().set_cw_wpm(25);
    s.engine.lock().unwrap().send_cw("K");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cw_wire(&s).iter().any(|c| c.starts_with("cwx send \"K\"")) {
        assert!(Instant::now() < deadline, "the word never went");
        s.step(now_unix_ms());
        std::thread::sleep(Duration::from_millis(20));
    }
    s.engine.lock().unwrap().set_cw_wpm(28);
    let deadline = Instant::now() + Duration::from_secs(10);
    while client(&s).cw_speed() != Some(25) {
        assert!(Instant::now() < deadline, "the radio never reported 25");
        std::thread::sleep(Duration::from_millis(20));
    }
    s.run(500);
    let held = s.engine.lock().unwrap().cw_wpm();
    assert_eq!(
        (held, speed_wire(&s)),
        (28, vec!["cw wpm 30".to_string(), "cw wpm 25".to_string()]),
        "Nexus's own speed taken back"
    );

    // As it ships.
    let mut s = cw_scene_on(session, Vec::new(), shipped());
    assert_eq!(other_program_sets_30(&mut s), 20);
}

// ── Tune with the radio's own carrier: switched on in tests only ─────────────────────────────
//
// Admission refuses the radio's tune carrier in production (`tempo_net::flex::admission::BENCHED`)
// until a tester's bench confirms its readback, and the loop then tunes as over every CAT link:
// PTT and Nexus's tone, over the client's audio route. These tests open the door the way only tests
// can ([`unbenched`]), so the loop runs the path behind it; each "as it ships" half is today's tune.

/// A scene on the bundled radio, its client on `config`, settled: with `native` audio, the tee and
/// the radio's DAX source in place, so a tone tune would go out over DAX; without, the readback
/// idle.
fn tuning_scene(
    native: bool,
    faults: Vec<tempo_flexsim::Fault>,
    config: tempo_net::flex::session::Config,
) -> FlexScene {
    tuning_scene_on(SimSession::v4_gui_client(), native, faults, config)
}

/// [`tuning_scene`] on a radio that follows `session`.
fn tuning_scene_on(
    session: SimSession,
    native: bool,
    faults: Vec<tempo_flexsim::Fault>,
    config: tempo_net::flex::session::Config,
) -> FlexScene {
    let mut s = FlexScene::with_faults(native, session, faults, config);
    run_until(&mut s, "settle", |s| {
        let ready = client(s).session().snapshot().transmit_ready;
        ready
            && (!native
                || (s.backend.tee.lock().unwrap().is_some()
                    && s.log()
                        .iter()
                        .any(|(_, e)| command(e, "transmit set dax=1"))))
    });
    s.run(300);
    s
}

/// The keying commands on the radio's wire: the key, its unkey and the tune carrier's.
fn tune_wire(s: &FlexScene) -> Vec<String> {
    s.log()
        .into_iter()
        .filter_map(|(_, e)| match e {
            SimEvent::Command { text, .. }
                if text.starts_with("xmit ") || text.starts_with("transmit tune") =>
            {
                Some(text)
            }
            _ => None,
        })
        .collect()
}

/// Whether the radio reports its tune carrier up (`transmit tune=1`).
fn radio_tuned(s: &FlexScene) -> bool {
    client(s).session().snapshot().model.transmit.tune == Some(true)
}

/// Whether DAX transmit audio reached the radio at or after `from`.
fn dax_tx_audio_since(s: &FlexScene, from: Instant) -> bool {
    s.log()
        .iter()
        .any(|(t, e)| *t >= from && matches!(e, SimEvent::UdpIn { bytes, .. } if bytes.len() > 1))
}

/// ⭐ A NATIVE TUNE KEYS THE RADIO'S OWN CARRIER AND PLAYS NO TONE. With native audio on, Tune is
/// `transmit tune 1`: the radio makes the carrier at its own tune power, the loop plays nothing,
/// DAX carries nothing, no power is written (the Tune power setting, here 10 %, is not for this
/// carrier), and the screens are given the radio's tune power (the bundled radio's 10 %) and its
/// transmit timeout (off: 0) to show beside Tune. As it ships, Tune is today's: PTT (`xmit 1`) and
/// Nexus's tone, over DAX, at the Tune power setting, with nothing beside it.
#[test]
fn a_native_tune_keys_the_radios_carrier_and_plays_no_tone() {
    let outcome = |config| {
        let mut s = tuning_scene(true, Vec::new(), config);
        {
            let mut e = s.engine.lock().unwrap();
            let mut settings = e.settings().clone();
            settings.tune_power_pct = Some(10);
            e.apply_settings(settings);
        }
        let from = Instant::now();
        s.engine.lock().unwrap().set_tune(true);
        run_until(&mut s, "the tune keyed", |s| {
            s.state.tuning_keyed && flex_on_air(s)
        });
        s.run(1_000);
        let played = s.backend.played.lock().unwrap().iter().any(|t| *t >= from);
        let power_written = s.log().iter().any(|(t, e)| {
            *t >= from
                && matches!(e, SimEvent::Command { text, .. } if text.starts_with("transmit set rfpower"))
        });
        let flex_tune = s.engine.lock().unwrap().snapshot().radio.flex_tune;
        format!(
            "wire={:?} radio_tuned={} played={played} dax_tx_audio={} power_written={power_written} \
             beside_tune={flex_tune:?}",
            tune_wire(&s),
            radio_tuned(&s),
            dax_tx_audio_since(&s, from)
        )
    };
    assert_eq!(
        outcome(unbenched()),
        "wire=[\"transmit tune 1\"] radio_tuned=true played=false dax_tx_audio=false \
         power_written=false beside_tune=Some(FlexTune { power_pct: Some(10), tx_timeout_ms: \
         Some(0) })"
    );
    assert_eq!(
        outcome(shipped()),
        "wire=[\"xmit 1\"] radio_tuned=false played=true dax_tx_audio=true power_written=true \
         beside_tune=None"
    );
}

/// ⭐ THE TUNE TIMEOUT AND THE RELEASE EACH END THE RADIO'S CARRIER WITH ITS OWN STOP. On the loop's
/// own clock, held: the carrier is still up 11,980 ms into the 12-second tune timeout, and
/// `transmit tune 0` goes out on the first tick past 12,000 ms. Released by the operator: it goes
/// out on the next tick. Each time the tune's own stop ends it (never `xmit 0`), the radio's end is
/// proven, Tune is off and there is no alarm. In Phone the release's stop is the only one. In FT's
/// section the end of a tune also stands the sequencer down, as WSJT-X's does (a halt), and the
/// halt's hard stop asks the client again while the tune's end is not yet proven: the same stop
/// twice, ms apart, the second one the hard stop's `T 0`.
#[test]
fn the_tune_timeout_and_the_release_each_send_transmit_tune_0() {
    let ended = |s: &mut FlexScene, now: f64| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while flex_on_air(s) && Instant::now() < deadline {
            s.step(now);
            std::thread::sleep(Duration::from_millis(20));
        }
        // One engine lock at a time: `tx_alarms` takes its own.
        let tuning = s.engine.lock().unwrap().tuning();
        format!(
            "wire={:?} tuning={tuning} on_air={} alarms={:?}",
            tune_wire(s),
            flex_on_air(s),
            tx_alarms(s)
        )
    };
    let up = |s: &mut FlexScene, now: f64| {
        s.engine.lock().unwrap().set_tune(true);
        s.step(now);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !radio_tuned(s) {
            assert!(Instant::now() < deadline, "the radio never tuned");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let scene = |phone: bool| {
        let mut s = tuning_scene(false, Vec::new(), unbenched());
        if phone {
            s.engine.lock().unwrap().set_operating_mode("phone", false);
            s.run(500);
        }
        s
    };
    for (phone, stops) in [
        (true, "\"transmit tune 0\""),
        (false, "\"transmit tune 0\", \"transmit tune 0\""),
    ] {
        let want =
            format!("wire=[\"transmit tune 1\", {stops}] tuning=false on_air=false alarms=[]");
        // The timeout.
        let mut s = scene(phone);
        assert_eq!(s.engine.lock().unwrap().settings().tune_timeout_secs, 12);
        let t0 = now_unix_ms();
        up(&mut s, t0);
        s.step(t0 + 11_980.0);
        assert_eq!(
            tune_wire(&s),
            ["transmit tune 1"],
            "held at 11,980 ms, phone: {phone}"
        );
        s.step(t0 + 12_020.0);
        assert_eq!(
            ended(&mut s, t0 + 12_040.0),
            want,
            "the timeout, phone: {phone}"
        );

        // The release.
        let mut s = scene(phone);
        let t0 = now_unix_ms();
        up(&mut s, t0);
        s.engine.lock().unwrap().set_tune(false);
        s.step(t0 + 500.0);
        assert_eq!(
            ended(&mut s, t0 + 520.0),
            want,
            "the release, phone: {phone}"
        );
    }
}

/// ⭐ STOP TX ENDS A NATIVE TUNE WITH THE TUNE'S OWN STOP. Halt drops Tune, the loop's release sends
/// `transmit tune 0`, and the hard stop's `T 0` asks the client to end what is ours, which is that
/// tune again: the same stop a second time, nothing else. The end is proven, Tune is off and the
/// loop holds nothing, no alarm.
#[test]
fn stop_tx_ends_a_native_tune_with_its_own_stop() {
    let mut s = tuning_scene(false, Vec::new(), unbenched());
    s.engine.lock().unwrap().set_tune(true);
    run_until(&mut s, "the radio tuned", radio_tuned);
    s.engine.lock().unwrap().halt_tx();
    run_until(&mut s, "the end proven", |s| !flex_on_air(s));
    s.run(1_000);
    let tuning = s.engine.lock().unwrap().tuning();
    assert_eq!(
        format!(
            "wire={:?} tuning={tuning} tuning_keyed={} radio_tuned={} alarms={:?}",
            tune_wire(&s),
            s.state.tuning_keyed,
            radio_tuned(&s),
            tx_alarms(&s)
        ),
        "wire=[\"transmit tune 1\", \"transmit tune 0\", \"transmit tune 0\"] tuning=false \
         tuning_keyed=false radio_tuned=false alarms=[]"
    );
}

/// ⭐ A STALLED LOOP STILL ENDS THE RADIO'S CARRIER. The loop hands the client its tune timeout
/// with the start, and the client's session ends the carrier by itself at that plus its 2-second
/// margin: here a 1-second timeout and a loop that never steps again once the tune is up, and
/// `transmit tune 0` goes out 3 s after `transmit tune 1` (from 10 ms before, as the session's
/// clock counts whole milliseconds, to 250 ms after), with no stop from the loop.
#[test]
fn a_stalled_loop_still_ends_the_native_tune_at_its_timeout_and_margin() {
    let mut s = tuning_scene(false, Vec::new(), unbenched());
    {
        let mut e = s.engine.lock().unwrap();
        let mut settings = e.settings().clone();
        settings.tune_timeout_secs = 1;
        e.apply_settings(settings);
        e.set_tune(true);
    }
    s.step(now_unix_ms());
    s.sim.wait_for(Duration::from_secs(8), |log| {
        sent(log, 0).iter().any(|c| c == "transmit tune 0")
    });
    let at = |text: &str| {
        s.sim.log().iter().find_map(|l| match &l.event {
            SimEvent::Command { text: t, .. } if t == text => Some(l.at),
            _ => None,
        })
    };
    let gap = at("transmit tune 0")
        .zip(at("transmit tune 1"))
        .map(|(off, on)| off.saturating_sub(on).as_millis());
    assert!(
        gap.is_some_and(|g| (2_990..3_250).contains(&g)),
        "transmit tune 0 {gap:?} ms after transmit tune 1: {:?}",
        tune_wire(&s)
    );
}

/// ⭐ NO TRANSMIT TIMEOUT ON THE RADIO: TUNE STILL STARTS, AND THE SCREENS ARE TOLD (operator ruling,
/// 2026-10-08, "Warn only"). The bundled radio reports its transmit timeout off
/// (`interlock timeout=0`): the native tune keys, and the radio's 0 reaches the snapshot for the
/// screens to warn beside Tune. With a 30-second timeout reported, the same tune keys and the
/// snapshot carries 30,000 ms.
#[test]
fn a_native_tune_without_the_radios_tx_timeout_starts_and_says_so() {
    for (timeout, want) in [(0, "Some(0)"), (30_000, "Some(30000)")] {
        let mut session = SimSession::v4_gui_client();
        for item in session
            .rules
            .iter_mut()
            .flat_map(|(_, rules)| rules.iter_mut())
            .flat_map(|r| r.items.iter_mut())
        {
            if let Item::Send(line) = item {
                *line = line.replace(
                    "S0|interlock timeout=0 ",
                    &format!("S0|interlock timeout={timeout} "),
                );
            }
        }
        let mut s = tuning_scene_on(session, false, Vec::new(), unbenched());
        s.engine.lock().unwrap().set_tune(true);
        run_until(&mut s, "the radio tuned", radio_tuned);
        let beside = s.engine.lock().unwrap().snapshot().radio.flex_tune;
        assert_eq!(
            format!(
                "wire={:?} timeout={:?}",
                tune_wire(&s),
                beside.map(|t| t.tx_timeout_ms)
            ),
            format!("wire=[\"transmit tune 1\"] timeout=Some({want})")
        );
    }
}

/// ⭐ NATIVE AUDIO OFF MID-TUNE CHANGES NOTHING. The radio makes the carrier, so native audio going
/// off under a native tune takes nothing from it: the carrier stays up, Tune stays on, nothing that
/// keys or routes the transmitter is written until the operator releases Tune, and the release ends
/// it with the tune's own stop (twice in FT's section: see the timeout and release test). As it
/// ships, the tune is PTT and Nexus's tone over DAX: native audio off takes the tone off DAX, and
/// the radio is keyed with nothing to send.
#[test]
fn native_audio_off_mid_tune_changes_nothing() {
    let outcome = |config| {
        let mut s = tuning_scene(true, Vec::new(), config);
        s.engine.lock().unwrap().set_tune(true);
        run_until(&mut s, "the tune keyed", |s| {
            s.state.tuning_keyed && flex_on_air(s)
        });
        s.run(500);
        let transmit_side = |s: &FlexScene| -> Vec<String> {
            s.log()
                .into_iter()
                .filter_map(|(_, e)| match e {
                    SimEvent::Command { text, .. }
                        if [
                            "xmit",
                            "transmit",
                            "cwx",
                            "atu",
                            "stream create type=dax_tx",
                        ]
                        .iter()
                        .any(|p| text.starts_with(p)) =>
                    {
                        Some(text)
                    }
                    _ => None,
                })
                .collect()
        };
        let before = transmit_side(&s).len();
        let off = Instant::now();
        s.set_native_audio(false);
        s.run(1_500);
        let written = transmit_side(&s)[before..].to_vec();
        // Past what the DAX pacer may still hold when the route goes.
        let dax_after = dax_tx_audio_since(&s, off + Duration::from_millis(700));
        let held = format!(
            "radio_tuned={} tuning={} tuning_keyed={}",
            radio_tuned(&s),
            s.engine.lock().unwrap().tuning(),
            s.state.tuning_keyed
        );
        let keying = tune_wire(&s).len();
        s.engine.lock().unwrap().set_tune(false);
        run_until(&mut s, "the end proven", |s| !flex_on_air(s));
        // The keying commands only: once the tune has ended, the next quiet point puts the
        // operator's mic back (`transmit set dax=0`), as it does after any transmission.
        let released = tune_wire(&s)[keying..].to_vec();
        format!(
            "written={written:?} {held} dax_after={dax_after} released={released:?} alarms={:?}",
            tx_alarms(&s)
        )
    };
    assert_eq!(
        outcome(unbenched()),
        "written=[] radio_tuned=true tuning=true tuning_keyed=true dax_after=false \
         released=[\"transmit tune 0\", \"transmit tune 0\"] alarms=[]"
    );
    assert_eq!(
        outcome(shipped()),
        "written=[] radio_tuned=false tuning=true tuning_keyed=true dax_after=false \
         released=[\"xmit 0\", \"xmit 0\"] alarms=[]"
    );
}

/// ⭐ A CONNECTION LOST MID-TUNE. The session drops while the radio's carrier is up, and the radio
/// keeps it up (the worse answer, which a tester's bench settles). The loop reads the client's alarm,
/// that the radio may still be transmitting, ahead of its restart; Tune goes off; and the fresh
/// client it starts ends the lost session's carrier with the tune's own stop, `transmit tune 0`,
/// before the rest of `T 0` (the earlier session's `cwx clear` and `xmit 0`). Nothing keys again.
#[test]
fn a_connection_lost_mid_tune_alarms_and_the_fresh_client_ends_the_carrier() {
    // The radio's answer to `sub tx all` is the last interlock word a fresh client hears
    // ([`holder_kept`]): the lost session still tuning.
    let sim = FlexScene::radio(
        holder_kept(),
        vec![tempo_flexsim::Fault::DisconnectMidOver {
            after: Duration::from_millis(300),
            radio_stays_keyed: true,
        }],
    );
    let mut s = FlexScene::on(sim, false, unbenched());
    run_until(&mut s, "settle", |s| {
        client(s).session().snapshot().transmit_ready
    });
    s.replace_on_rebuild = true;
    s.engine.lock().unwrap().set_tune(true);
    let detail = status_after_the_rebuild(&mut s);
    assert!(
        detail.starts_with(
            "the connection to the radio was lost during a transmission — it may still be \
             transmitting. Check the radio now."
        ),
        "{detail}"
    );
    // The connections: the scene's client (lost), then the fresh client.
    let fresh = sent_on(&s, 1, "xmit 0");
    let at = |text: &str| {
        fresh
            .iter()
            .position(|c| c == text)
            .unwrap_or_else(|| panic!("no {text:?} in {fresh:?}"))
    };
    assert!(at("transmit tune 0") < at("xmit 0"), "{fresh:?}");
    s.run(1_000);
    let keyed_again = sent(&s.sim.log(), 1)
        .iter()
        .any(|c| c == "xmit 1" || c == "transmit tune 1");
    assert_eq!(
        (
            s.engine.lock().unwrap().tuning(),
            s.state.tuning_keyed,
            keyed_again
        ),
        (false, false, false)
    );
}

// ── The radio's own ATU: switched on in tests only ──────────────────────────────────────────
//
// Admission refuses an ATU start in production (`tempo_net::flex::admission::BENCHED`) until a
// tester's bench confirms its readback, and the client then answers no `u TUNER`, so no ATU button
// is offered on it. These tests open the door the way only tests can ([`unbenched`]); each "as it
// ships" half is today's. They run in the Phone section: in the Digital section the stand-down
// that follows a tune-up the radio takes halts TX, which is its own matter.

/// A scene on a radio that follows `session`, its client on `config`, in the Phone section,
/// settled, the radio's tuner probed.
fn atu_scene(
    session: SimSession,
    faults: Vec<tempo_flexsim::Fault>,
    config: tempo_net::flex::session::Config,
) -> FlexScene {
    let mut s = FlexScene::with_faults(false, session, faults, config);
    s.engine.lock().unwrap().set_operating_mode("phone", false);
    run_until(&mut s, "settle", |s| {
        client(s).session().snapshot().transmit_ready && s.state.tuner_probed
    });
    s.run(300);
    s
}

/// The ATU's commands on the radio's wire, and both stops a cycle can take, in order.
fn atu_wire(s: &FlexScene) -> Vec<String> {
    s.log()
        .into_iter()
        .filter_map(|(_, e)| match e {
            SimEvent::Command { text, .. }
                if text.starts_with("atu ")
                    || text == "xmit 0"
                    || text.starts_with("transmit tune") =>
            {
                Some(text)
            }
            _ => None,
        })
        .collect()
}

/// What the screens are given beside the ATU.
fn beside_atu(s: &FlexScene) -> Option<tempo_app::dto::FlexAtu> {
    s.engine.lock().unwrap().snapshot().radio.flex_atu
}

/// Whether the radio reports the client's ATU cycle keyed.
fn cycle_keyed(s: &FlexScene) -> bool {
    client(s)
        .session()
        .snapshot()
        .model
        .interlock
        .sample
        .is_some_and(|i| i.state == "TRANSMITTING" && i.source == "TUNE")
}

/// Whether the radio has reported the cycle's result and the client believes nothing of ours on the
/// air.
fn cycle_ended(s: &FlexScene) -> bool {
    client(s).session().snapshot().model.atu.status.as_deref() == Some("TUNE_SUCCESSFUL")
        && !flex_on_air(s)
}

/// The bundled radio with its `atu start` rules in place of its own.
fn atu_rules(rules: Vec<Rule>) -> SimSession {
    let mut session = SimSession::v4_gui_client();
    let group = session.lookup("atu start").expect("a rule");
    session.rules[group].1 = rules;
    session
}

/// The bundled radio's own ATU cycle, taking `ms` from TRANSMITTING to its result (500 ms in the
/// bundled session): room for Stop TX mid-cycle on the loop's real clock.
fn cycle_of(ms: u64) -> Rule {
    let mut cycle = SimSession::v4_gui_client().rules[SimSession::v4_gui_client()
        .lookup("atu start")
        .expect("a rule")]
    .1[0]
        .clone();
    for item in &mut cycle.items {
        if *item == Item::Wait(500) {
            *item = Item::Wait(ms);
        }
    }
    cycle
}

/// A radio with no tuner fitted: its answer to `sub atu all` carries no `atu` status.
fn no_tuner() -> SimSession {
    let mut session = SimSession::v4_gui_client();
    let group = session.lookup("sub atu all").expect("a rule");
    for rule in session.rules[group].1.iter_mut() {
        rule.items.clear();
    }
    session
}

/// ⭐ ON A FLEX PROFILE THE ATU BUTTON IS THE CLIENT'S TO OFFER. The scene's profile is Hamlib model
/// 2036, SmartSDR CAT's, whose CAT path cannot start a tune ("press TUNER on the radio itself").
/// Behind the door, with a tuner the radio reports fitted, the screens get an ATU button that
/// starts one, bypassed until a cycle matches: the client answers for its own path. With no tuner
/// fitted, no button. As it ships, no button either, as before.
#[test]
fn the_atu_button_on_a_flex_profile_is_the_clients_to_offer() {
    let probed = |session, config| {
        let s = atu_scene(session, Vec::new(), config);
        let radio = s.engine.lock().unwrap().snapshot().radio;
        (radio.atu, radio.atu_start_tune_unsupported)
    };
    assert_eq!(
        probed(SimSession::v4_gui_client(), unbenched()),
        (Some(false), false)
    );
    assert_eq!(probed(no_tuner(), unbenched()), (None, true));
    assert_eq!(probed(SimSession::v4_gui_client(), shipped()), (None, true));
}

/// ⭐ AN ATU PRESS RUNS ONE CYCLE OF THE RADIO'S OWN TUNER, AND ITS RESULT SHOWS BESIDE IT. Behind the
/// door: one `atu start` and nothing else on the wire; the client believes nothing of ours on the
/// air once the radio has reported the result and the idle interlock; beside the ATU the screens
/// are given the radio's tune power and transmit timeout (the bundled radio's 10 % and off, so the
/// warning shows) and the cycle's result in the radio's word; nothing alarms. As it ships there is
/// no ATU button, a press is refused with its reason, and nothing reaches the radio.
#[test]
fn an_atu_press_runs_one_cycle_and_its_result_shows_beside_it() {
    let mut s = atu_scene(SimSession::v4_gui_client(), Vec::new(), unbenched());
    s.engine
        .lock()
        .unwrap()
        .atu_tune()
        .expect("the press is taken");
    run_until(&mut s, "the cycle's result", cycle_ended);
    s.run(500);
    assert_eq!(
        format!(
            "wire={:?} on_air={} beside_atu={:?} alarms={:?}",
            atu_wire(&s),
            flex_on_air(&s),
            beside_atu(&s),
            tx_alarms(&s)
        ),
        "wire=[\"atu start\"] on_air=false beside_atu=Some(FlexAtu { tune: FlexTune { \
         power_pct: Some(10), tx_timeout_ms: Some(0) }, status: Some(\"TUNE_SUCCESSFUL\"), \
         refused: None }) alarms=[]"
    );

    let mut s = atu_scene(SimSession::v4_gui_client(), Vec::new(), shipped());
    assert_eq!(
        s.engine.lock().unwrap().atu_tune(),
        Err("This radio doesn't report an antenna tuner over CAT — nothing to run".to_string())
    );
    s.run(500);
    assert_eq!(
        format!("wire={:?} beside_atu={:?}", atu_wire(&s), beside_atu(&s)),
        "wire=[] beside_atu=None"
    );
}

/// ⭐ STOP TX DURING AN ATU CYCLE SENDS BOTH STOPS. No command that ends a cycle part way is
/// documented, so Stop TX (the hard stop's `T 0`) sends the radio's unkey and its tune-off, `xmit 0`
/// and `transmit tune 0`, and nothing alarms once the radio reports the cycle's result and the idle
/// interlock. A radio whose cycle goes on (the simulator's AtuNeverEnds) gets both again once the
/// stop's deadline passes (shortened here to 1.5 s, as the client's own tests shorten it), and the
/// operator reads that it may still be transmitting, ahead of the client's restart.
#[test]
fn stop_tx_mid_cycle_sends_both_stops_and_a_cycle_that_goes_on_alarms() {
    let mut s = atu_scene(atu_rules(vec![cycle_of(3_000)]), Vec::new(), unbenched());
    s.engine
        .lock()
        .unwrap()
        .atu_tune()
        .expect("the press is taken");
    run_until(&mut s, "the cycle keyed", cycle_keyed);
    s.engine.lock().unwrap().halt_tx();
    run_until(&mut s, "the end proven", |s| !flex_on_air(s));
    s.run(500);
    assert_eq!(
        format!("wire={:?} alarms={:?}", atu_wire(&s), tx_alarms(&s)),
        "wire=[\"atu start\", \"xmit 0\", \"transmit tune 0\"] alarms=[]"
    );

    let mut config = unbenched();
    config.unkey_deadline_ms = 1_500;
    let mut s = atu_scene(
        SimSession::v4_gui_client(),
        vec![tempo_flexsim::Fault::AtuNeverEnds],
        config,
    );
    s.replace_on_rebuild = true;
    s.engine
        .lock()
        .unwrap()
        .atu_tune()
        .expect("the press is taken");
    run_until(&mut s, "the cycle keyed", cycle_keyed);
    s.engine.lock().unwrap().halt_tx();
    let detail = status_after_the_rebuild(&mut s);
    assert!(
        detail.starts_with(
            "the radio did not confirm the unkey — it may still be transmitting. Check the radio \
             now."
        ),
        "{detail}"
    );
    let first: Vec<String> = sent(&s.sim.log(), 0)
        .into_iter()
        .filter(|c| c.starts_with("atu ") || c == "xmit 0" || c.starts_with("transmit tune"))
        .collect();
    assert_eq!(
        first,
        [
            "atu start",
            "xmit 0",
            "transmit tune 0",
            "xmit 0",
            "transmit tune 0"
        ]
    );
}

/// ⭐ AN ATU START THE RADIO REFUSES ENDS QUIETLY, WITH ITS REASON BESIDE THE ATU. The radio answers
/// the first `atu start` with an error and keys nothing: no stop, no alarm, the client kept (no
/// restart), nothing of ours believed on the air, and the ATU line says why, with the radio's code.
/// The next press clears it and runs a cycle. (Which code a radio refuses with is not established:
/// the simulator's error code stands in.)
#[test]
fn a_refused_atu_start_ends_quietly_with_its_reason_beside_the_atu() {
    let refused = Rule {
        code: tempo_flexsim::fault::REFUSED.to_string(),
        message: String::new(),
        items: Vec::new(),
    };
    let mut s = atu_scene(
        atu_rules(vec![refused, cycle_of(500)]),
        Vec::new(),
        unbenched(),
    );
    // So that a restart is a value here, not the scene's panic.
    s.replace_on_rebuild = true;
    s.engine
        .lock()
        .unwrap()
        .atu_tune()
        .expect("the press is taken");
    s.run(2_000);
    assert_eq!(
        format!(
            "wire={:?} on_air={} refused={:?} alarms={:?} restarted={}",
            atu_wire(&s),
            flex_on_air(&s),
            beside_atu(&s).and_then(|a| a.refused),
            tx_alarms(&s),
            s.rebuilt.load(std::sync::atomic::Ordering::Relaxed)
        ),
        "wire=[\"atu start\"] on_air=false refused=Some(\"ATU not started: the radio refused it \
         (0x5000002C).\") alarms=[] restarted=false"
    );

    s.engine
        .lock()
        .unwrap()
        .atu_tune()
        .expect("the press is taken");
    run_until(&mut s, "the cycle's result", cycle_ended);
    s.run(300);
    assert_eq!(
        beside_atu(&s).map(|a| (a.status, a.refused)),
        Some((Some("TUNE_SUCCESSFUL".to_string()), None))
    );
    assert_eq!(atu_wire(&s), ["atu start", "atu start"]);
}
