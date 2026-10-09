//! THE IC-905'S 10 GHz DIAL, FROM THE RADIO LOOP'S SIDE: the frequency the licence gate, the band
//! and the logbook are handed while the radio is on its 10 GHz band. There its dial is 12 digits
//! (6 bytes), not 10 (IC-905 CI-V Reference Guide A7711-9EX-2, PDF p. 17). Checked by value on the
//! wire of a fake IC-905 behind Nexus's own CI-V daemon, driven by the real `RadioLoop::step`, with
//! a Technician's licence so the gate has something to judge.
//!
//! What the fake knows of the radio is that page's byte layout: it takes any dial it is sent and
//! refuses none. Whether a real IC-905 on 10 GHz refuses a 10-digit write is for the bench.
use super::*;
use crate::civ::broker::CivDaemon;
use crate::civ::commands::IcomModel;
use crate::civ::engine::tests_support::{FakeRadio, Regs};
use tempo_app::settings::{LicenseClass, OperatingMode};

/// The IC-905's default CI-V address.
const ADDR: u8 = 0xAC;

/// The radio loop on a fake IC-905 behind Nexus's own CI-V daemon: CAT PTT, Phone, a Technician.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    backend: MockBackend,
    regs: Arc<Mutex<Regs>>,
    /// Set if the loop ever tried to rebuild the transport (it would drop the daemon under test).
    rebuilt: Arc<std::sync::atomic::AtomicBool>,
}

impl Scene {
    fn new() -> Self {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let settings = {
            let mut e = engine.lock().unwrap();
            let mut s = e.settings().clone();
            s.rig_model = 3090; // IC-905
            s.ptt_method = "cat".into();
            s.license_class = LicenseClass::Technician;
            e.apply_settings(s);
            e.set_operating_mode("phone", false);
            e.settings().clone()
        };
        let (radio, _push) = FakeRadio::new(ADDR);
        let regs = radio.regs();
        let daemon =
            CivDaemon::start_with_io(Box::new(radio), ADDR, 0, 1, Some(IcomModel::Ic905)).unwrap();
        let rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
        let cfg = RadioConfig {
            rig_model: settings.rig_model,
            ..RadioConfig::default()
        };
        // The transport the loop compares its settings against: the same settings, so the loop never
        // rebuilds the link (which would replace the daemon under test with a stub).
        let state = RadioLoop::new(
            Transport::from_settings(&settings),
            Some(CatDaemon::Native(daemon)),
            &cfg,
        );
        let mut s = Scene {
            engine,
            state,
            rig,
            backend: MockBackend::new(),
            regs,
            rebuilt: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        // The loop's first ticks put the engine's own dial on the radio and read it back. A test
        // starts from that settled link.
        let n = s.wire_len();
        s.run_until("the first dial write and a read after it", |s| {
            s.write_then_read(n)
        });
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
            "the loop rebuilt its transport: the scene's daemon is gone"
        );
    }

    /// Step the loop as `run_radio` does (a step, then 20 ms) until `done`, then ten ticks more so
    /// the loop has acted on what it last read. Fails, naming `what`, after 5 s.
    fn run_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done(self) {
            assert!(Instant::now() < deadline, "never happened: {what}");
            self.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        for _ in 0..10 {
            self.step();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wire_len(&self) -> usize {
        self.regs.lock().unwrap().wire.len()
    }

    /// The dial writes (`05`) on the wire since frame `n`, as hex.
    fn dial_writes(&self, n: usize) -> Vec<String> {
        self.regs.lock().unwrap().wire[n..]
            .iter()
            .filter(|f| f.get(4) == Some(&0x05))
            .map(|f| {
                f.iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    /// Since frame `n`, a dial write (`05`) and a dial read (`03`) after it.
    fn write_then_read(&self, n: usize) -> bool {
        let r = self.regs.lock().unwrap();
        let cmds: Vec<u8> = r.wire[n..]
            .iter()
            .filter_map(|f| f.get(4).copied())
            .collect();
        cmds.iter()
            .position(|c| *c == 0x05)
            .is_some_and(|i| cmds[i..].contains(&0x03))
    }

    /// What the engine holds: the dial (MHz), its band, and the licence gate's verdict on it.
    fn judged(&self) -> (f64, String, bool) {
        let e = self.engine.lock().unwrap();
        (
            e.settings().dial_mhz,
            e.settings().band.clone(),
            e.tx_allowed(),
        )
    }
}

/// ⭐ A QSY TO 10368.150 MHz REACHES THE RADIO AS 10368.150 MHz, and the dial the loop reads back is
/// the one the licence gate judges: 3 cm, all-mode for a Technician. In 10 digits the radio was sent
/// 368.150 MHz, the loop read that back as a move at the radio, and the gate judged 368.150 MHz,
/// which is no amateur allocation, with no band for the log.
#[test]
fn a_qsy_to_10_ghz_reaches_the_ic905_and_the_licence_gate_judges_3_cm() {
    let mut s = Scene::new();
    let n = s.wire_len();
    s.engine
        .lock()
        .unwrap()
        .set_frequency(10368.15, "3cm", "USB");
    s.run_until("the QSY's write and a read after it", |s| {
        s.write_then_read(n)
    });
    let (dial, band, allowed) = s.judged();
    assert_eq!(
        (
            s.dial_writes(n),
            s.regs.lock().unwrap().main_hz,
            dial,
            band.as_str(),
            allowed
        ),
        (
            vec!["FE FE AC E0 05 00 00 15 68 03 01 FD".to_string()],
            10_368_150_000,
            10368.15,
            "3cm",
            true
        )
    );
}

/// ⭐ THE RADIO'S OWN 10 GHz DIAL IS READ AS 10 GHz. The operator puts the radio on 10368.150 MHz at
/// its front panel, and the loop's next read hands the engine that dial: the band the log carries,
/// and the frequency the gate judges. In 10 digits it was 368.150 MHz, off every band, and refused.
/// The gate's own verdict on the two numbers, under the operator's class, is the other half: the
/// gate is the one it was, and only its input moved.
#[test]
fn the_ic905s_own_10_ghz_dial_is_what_the_licence_gate_judges() {
    let mut s = Scene::new();
    let before = s.judged().0;
    s.regs.lock().unwrap().main_hz = 10_368_150_000;
    s.run_until("the loop adopts the radio's dial", |s| {
        s.judged().0 != before
    });
    let (dial, band, allowed) = s.judged();
    assert_eq!((dial, band.as_str(), allowed), (10368.15, "3cm", true));
    let gate = |mhz: f64| {
        tempo_app::privileges::tx_allowed(LicenseClass::Technician, mhz, OperatingMode::Phone)
    };
    assert_eq!((gate(10368.15), gate(368.15)), (true, false));
}
