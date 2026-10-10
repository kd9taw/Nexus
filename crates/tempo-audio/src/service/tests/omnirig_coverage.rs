//! AN OMNIRIG RADIO'S RANGE IS UNKNOWN TO THE ENGINE, as a native CI-V radio's is, never the wide
//! row the CAT broker's `\dump_state` carries for WSJT-X (135.7 kHz to 1.3 GHz). Checked through
//! the real `RadioLoop::step`, a `Rig` over TCP and Nexus's OmniRig shim in front of the COM
//! boundary's mock (`crate::omnirig::tests::MockOmni`): what the loop's capability probe hands the
//! engine.
use super::*;
use crate::omnirig::tests::MockOmni;
use crate::omnirig::{OmniDaemon, OmniRigClient, OmniStatus, RigSlot};

/// ⭐ THE SHIM DECLARES NO RANGE, so its `\dump_state` carries an empty RX list and the probe hands
/// the engine "unknown": every caller fails open and the radio answers for itself. The shim reads
/// no range through its COM boundary, so it has none to declare. It served the broker's wide row,
/// which the probe took as this radio's range, 2 m included whatever the radio. The pads and
/// preamps further down the same reply are the control.
#[test]
fn an_omnirig_radios_range_is_unknown_to_the_engine() {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    let settings = {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_conn = "omnirig".into();
        s.ptt_method = "cat".into();
        e.apply_settings(s);
        e.settings().clone()
    };
    let mock = Arc::new(MockOmni::online());
    let daemon = OmniDaemon::start_with(
        Box::new(move || Ok(Box::new(mock) as Box<dyn OmniRigClient>)),
        RigSlot::Rig1,
        0,
    )
    .unwrap();
    let mut rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
    let cfg = RadioConfig {
        rig_model: settings.rig_model,
        ..RadioConfig::default()
    };
    // The same settings the transport is built from, so the loop never rebuilds the link.
    let mut state = RadioLoop::new(
        Transport::from_settings(&settings),
        Some(CatDaemon::Omni(daemon)),
        &cfg,
    );
    let mut backend = MockBackend::new();
    let rebuilt = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !state.rx_ranges_probed && Instant::now() < deadline {
        let sinks = no_sinks();
        let mut station = StationSinks::new();
        let mut reopen_audio = mock_reopen_audio();
        let flag = rebuilt.clone();
        let mut reopen_rig = move |_t: &Transport, _coexist: bool| {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            (Rig::vox(), None, CatProbe::status(None, ""))
        };
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                now_unix_ms(),
                &mut reopen_audio,
                &mut reopen_rig,
                &mut station,
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    let e = engine.lock().unwrap();
    let snap = e.snapshot();
    assert_eq!(
        (
            rebuilt.load(std::sync::atomic::Ordering::Relaxed),
            state.rx_ranges_probed,
            state.rx_ranges.clone(),
            e.rig_covers_mhz(145.0),
            snap.radio.rx_ranges_mhz,
            snap.radio.att_steps_db,
            snap.radio.preamp_steps_db,
        ),
        (false, true, None, None, vec![], Some(vec![]), Some(vec![]))
    );
}

/// The radio loop on Nexus's OmniRig shim in front of `mock`, settled: its range probed, ten ticks more.
struct OmniScene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    backend: MockBackend,
    mock: Arc<MockOmni>,
    rebuilt: Arc<std::sync::atomic::AtomicBool>,
}

impl OmniScene {
    fn new() -> OmniScene {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let settings = {
            let mut e = engine.lock().unwrap();
            let mut s = e.settings().clone();
            s.rig_conn = "omnirig".into();
            s.ptt_method = "cat".into();
            e.apply_settings(s);
            e.set_operating_mode("phone", false);
            e.set_frequency(14.074, "20m", "USB");
            e.settings().clone()
        };
        let mock = Arc::new(MockOmni::online());
        let m = mock.clone();
        let daemon = OmniDaemon::start_with(
            Box::new(move || Ok(Box::new(m) as Box<dyn OmniRigClient>)),
            RigSlot::Rig1,
            0,
        )
        .unwrap();
        let rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
        let cfg = RadioConfig {
            rig_model: settings.rig_model,
            ..RadioConfig::default()
        };
        // The same settings the transport is built from, so the loop never rebuilds the link.
        let state = RadioLoop::new(
            Transport::from_settings(&settings),
            Some(CatDaemon::Omni(daemon)),
            &cfg,
        );
        let mut s = OmniScene {
            engine,
            state,
            rig,
            backend: MockBackend::new(),
            mock,
            rebuilt: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        while !s.state.rx_ranges_probed && Instant::now() < deadline {
            s.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        for _ in 0..10 {
            s.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        s
    }

    fn step(&mut self) {
        let sinks = no_sinks();
        let mut station = StationSinks::new();
        let mut reopen_audio = mock_reopen_audio();
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
                now_unix_ms(),
                &mut reopen_audio,
                &mut reopen_rig,
                &mut station,
            )
            .unwrap();
        assert!(
            !self.rebuilt.load(std::sync::atomic::Ordering::Relaxed),
            "the loop rebuilt its transport: the shim under test is gone"
        );
    }

    /// The CAT status line and the dial the radio refused.
    fn told(&self) -> (String, Option<f64>) {
        let snap = self.engine.lock().unwrap().snapshot();
        (snap.radio.cat_detail, snap.radio.refused_dial_mhz)
    }
}

/// ⭐ A DIAL TO A RIG OMNIRIG SAYS IS NOT RESPONDING IS NOT CALLED REFUSED. The radio goes quiet,
/// OmniRig says so, and the operator QSYs to 21.074 MHz. The loop sends the dial three times, as
/// it did when the shim answered the silence as a refusal, says each time that the rig did not
/// reply, then gives the dial up without recording a refusal. Nothing reaches the radio either
/// way. It was "the radio refused 21.0740 MHz — it does not cover that frequency".
#[test]
fn a_dial_to_a_rig_omnirig_says_is_not_responding_is_not_called_refused() {
    let mut s = OmniScene::new();
    let written = s.mock.calls.lock().unwrap().len();
    *s.mock.status.lock().unwrap() = (
        OmniStatus::NotResponding,
        "RIG 1 is not responding".to_string(),
    );
    s.engine.lock().unwrap().set_frequency(21.074, "15m", "USB");
    let (mut said, mut held_ms) = (Vec::new(), Vec::new());
    let mut last = s.told().0;
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.state.dial_giveup.is_none() && Instant::now() < deadline {
        let started = Instant::now();
        s.step();
        held_ms.push(started.elapsed().as_millis());
        let line = s.told().0;
        if line != last {
            said.push(line.clone());
            last = line;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    eprintln!(
        "a dial to a rig OmniRig says is not responding: the steps held the loop {held_ms:?} ms"
    );
    let said: Vec<String> = said.into_iter().filter(|l| l.contains("21.0740")).collect();
    let written = s.mock.calls.lock().unwrap()[written..].to_vec();
    assert_eq!(
        (
            said.clone(),
            said.iter().any(|l| l.contains("refused")),
            written,
            s.told().1,
            s.state.dial_giveup,
        ),
        (
            vec![
                "21.0740 MHz not sent — no reply from the rig (1/3)".to_string(),
                "21.0740 MHz not sent — no reply from the rig (2/3)".to_string(),
                "21.0740 MHz not sent — no reply from the rig after 3 tries".to_string(),
            ],
            false,
            Vec::<String>::new(),
            None,
            Some(21_074_000),
        )
    );
}

/// ⭐ A MODE TO A RIG OMNIRIG SAYS IS NOT RESPONDING GIVES UP IN WORDS THAT FIT OMNIRIG. The radio
/// goes quiet, OmniRig says so, and the operator moves Phone to Digital; past the switch's own tick
/// the dial is left given up, so only the mode goes out, and the steady loop's budget starts two
/// short, so it gives up in two tries, with nothing written. The give-up says the rig did not answer
/// and points at the radio and OmniRig's own setup. It told the operator to raise the rig's CI-V
/// baud and turn CI-V Transceive off, settings an OmniRig link does not have in Nexus.
#[test]
fn a_mode_to_a_rig_omnirig_says_is_not_responding_gives_up_in_words_that_fit_omnirig() {
    let mut s = OmniScene::new();
    let written = s.mock.calls.lock().unwrap().len();
    *s.mock.status.lock().unwrap() = (
        OmniStatus::NotResponding,
        "RIG 1 is not responding".to_string(),
    );
    s.engine
        .lock()
        .unwrap()
        .set_operating_mode("digital", false);
    s.step();
    s.state.dial_giveup = Some(14_074_000);
    s.state.mode_fail_count = MODE_SET_MAX_TRIES - 2;
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.state.mode_giveup.is_none() && Instant::now() < deadline {
        s.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    let written = s.mock.calls.lock().unwrap()[written..].to_vec();
    assert_eq!(
        (s.state.mode_giveup.clone(), s.told().0, written),
        (
            Some("PKTUSB".to_string()),
            "couldn't set PKTUSB: no reply over CAT — OmniRig says the rig is not responding; \
             check the radio is on and its CAT settings match OmniRig — gave up"
                .to_string(),
            Vec::<String>::new(),
        )
    );
}
