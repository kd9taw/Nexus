//! AN OMNIRIG RADIO'S RANGE IS UNKNOWN TO THE ENGINE, as a native CI-V radio's is, never the wide
//! row the CAT broker's `\dump_state` carries for WSJT-X (135.7 kHz to 1.3 GHz). Checked through
//! the real `RadioLoop::step`, a `Rig` over TCP and Nexus's OmniRig shim in front of the COM
//! boundary's mock (`crate::omnirig::tests::MockOmni`): what the loop's capability probe hands the
//! engine.
use super::*;
use crate::omnirig::tests::MockOmni;
use crate::omnirig::{OmniDaemon, OmniRigClient, RigSlot};

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
