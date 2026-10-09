//! A NATIVE CI-V RADIO'S RANGE IS UNKNOWN TO THE ENGINE, never the wide row the CAT broker's
//! `\dump_state` carries for WSJT-X (135.7 kHz to 1.3 GHz). Checked through the real
//! `RadioLoop::step`, a `Rig` over TCP and Nexus's own CI-V daemon in front of a fake radio: what
//! the loop's capability probe hands the engine, what that does to a satellite pick, and what the
//! operator is told when the radio itself refuses a frequency, or does not answer at all.
//!
//! The fake radio takes every dial unless a scene gives it `covers_hz`; then it answers NG (`FA`)
//! outside those ranges. That a real radio NGs a dial it cannot tune is the fixture's model of
//! CI-V, not a bench measurement. Its `drop_dial_writes` makes it say nothing to a dial write: a
//! radio that went quiet, which is not a refusal.
use super::*;
use crate::civ::broker::CivDaemon;
use crate::civ::commands::IcomModel;
use crate::civ::engine::tests_support::{FakeRadio, Regs};
use tempo_app::settings::LicenseClass;
use tempo_core::doppler::{DownlinkClass, Transponder};

/// The radio loop on a fake Icom behind Nexus's own CI-V daemon: CAT PTT, Phone, a Technician.
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
    /// `hamlib` is the model number the profile carries; the engine starts on `start`.
    fn new(model: IcomModel, hamlib: u32, start: (f64, &str), covers_hz: &[(u64, u64)]) -> Self {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let settings = {
            let mut e = engine.lock().unwrap();
            let mut s = e.settings().clone();
            s.rig_model = hamlib;
            s.ptt_method = "cat".into();
            s.license_class = LicenseClass::Technician;
            e.apply_settings(s);
            e.set_operating_mode("phone", false);
            e.set_frequency(start.0, start.1, "USB");
            e.settings().clone()
        };
        let addr = model.default_civ_addr();
        let (radio, _push) = FakeRadio::new(addr);
        let regs = radio.regs();
        regs.lock().unwrap().covers_hz = covers_hz.to_vec();
        let daemon = CivDaemon::start_with_io(Box::new(radio), addr, 0, 1, Some(model)).unwrap();
        let rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
        let cfg = RadioConfig {
            rig_model: settings.rig_model,
            ..RadioConfig::default()
        };
        // The same settings the transport is built from, so the loop never rebuilds the link.
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
        // A test starts from a settled link: the engine's dial on the radio, read back, and the
        // once-per-confirmation capability probe done.
        let n = s.wire_len();
        s.settle(|s| s.write_then_read(n) && s.state.rx_ranges_probed);
        assert!(
            s.write_then_read(n) && s.state.rx_ranges_probed,
            "the link never settled"
        );
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

    /// Step the loop as `run_radio` does (a step, then 20 ms) until `done` or 3 s, then ten ticks
    /// more so the loop has acted on what it last read. It does not fail: each test asserts what
    /// happened by value, so a red names the values rather than a timeout.
    fn settle(&mut self, done: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !done(self) && Instant::now() < deadline {
            self.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        for _ in 0..10 {
            self.step();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Step as [`Self::settle`] does, noting the CAT status line each time it changes: the words
    /// the operator was shown, in order, from the first step on.
    fn said_until(&mut self, done: impl Fn(&Self) -> bool) -> Vec<String> {
        let mut said = Vec::new();
        let mut last = self.told().0;
        let mut step = |s: &mut Self| {
            s.step();
            let line = s.told().0;
            if line != last {
                said.push(line.clone());
                last = line;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        while !done(self) && Instant::now() < deadline {
            step(self);
        }
        for _ in 0..10 {
            step(self);
        }
        said
    }

    fn wire_len(&self) -> usize {
        self.regs.lock().unwrap().wire.len()
    }

    fn main_hz(&self) -> u64 {
        self.regs.lock().unwrap().main_hz
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

    /// Hold a fixed-downlink transponder and pick it, as the satellite pane does.
    fn pick(&self, down_hz: u64) -> tempo_app::engine::SatBinding {
        let mut e = self.engine.lock().unwrap();
        e.set_sat_transponder(Some((
            "QO-100|NB transponder".into(),
            0,
            Transponder::channel(0, down_hz),
        )));
        e.sat_tune_nominal(DownlinkClass::Usb, now_unix_ms() as u64)
    }

    /// What the operator is shown about the dial: the CAT status line, the dial the radio refused,
    /// and the satellite rail's note.
    fn told(&self) -> (String, Option<f64>, Option<String>) {
        let e = self.engine.lock().unwrap();
        let snap = e.snapshot();
        (
            snap.radio.cat_detail,
            snap.radio.refused_dial_mhz,
            e.sat_binding().and_then(|b| b.note.clone()),
        )
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

/// ⭐ THE IC-905'S RANGE IS UNKNOWN, NOT 135.7 kHz TO 1.3 GHz. The probe ran, and what it handed the
/// engine is "unknown", so the 13, 6 and 3 cm bands are not refused before the radio is asked. With
/// the CAT broker's wide row read as this radio's list, all three answered "doesn't cover".
#[test]
fn an_ic905s_range_is_unknown_to_the_engine() {
    let s = Scene::new(IcomModel::Ic905, 3090, (144.2, "2m"), &[]);
    let e = s.engine.lock().unwrap();
    assert_eq!(
        (
            s.state.rx_ranges_probed,
            s.state.rx_ranges.clone(),
            [2304.1, 5760.1, 10368.15].map(|mhz| e.rig_covers_mhz(mhz)),
        ),
        (true, None, [None, None, None])
    );
}

/// ⭐ AN IC-7300 IS NO LONGER BELIEVED TO COVER 2 m. The wide row said it did. The pads and preamps
/// ride the same reply, so they are the control: still the IC-7300's own, 20 dB and P.AMP 1/2.
#[test]
fn an_ic7300_is_not_believed_to_cover_2_m() {
    let s = Scene::new(IcomModel::Ic7300, 3073, (14.074, "20m"), &[]);
    let e = s.engine.lock().unwrap();
    let snap = e.snapshot();
    assert_eq!(
        (
            s.state.rx_ranges_probed,
            s.state.rx_ranges.clone(),
            e.rig_covers_mhz(145.0),
            snap.radio.att_steps_db,
            snap.radio.preamp_steps_db,
        ),
        (true, None, None, Some(vec![20]), Some(vec![1, 2]))
    );
}

/// ⭐ A QO-100 DOWNLINK PICK, 10489.500 MHz, REACHES THE IC-905: queued with no note, written in the
/// 10 GHz band's 12 digits, taken, and confirmed on the rail. It was refused before it was tried,
/// with "This radio doesn't cover 10489.5000 MHz".
#[test]
fn a_qo100_downlink_pick_reaches_the_ic905() {
    let mut s = Scene::new(IcomModel::Ic905, 3090, (144.2, "2m"), &[]);
    let n = s.wire_len();
    let picked = s.pick(10_489_500_000);
    s.settle(|s| s.write_then_read(n));
    let confirmed = s
        .engine
        .lock()
        .unwrap()
        .sat_binding()
        .and_then(|b| b.downlink_mhz);
    assert_eq!(
        (
            picked.note,
            picked.pending_downlink_mhz,
            s.dial_writes(n),
            s.main_hz(),
            confirmed,
        ),
        (
            None,
            Some(10489.5),
            vec!["FE FE AC E0 05 00 00 50 89 04 01 FD".to_string()],
            10_489_500_000,
            Some(10489.5),
        )
    );
}

/// ⭐ THE RADIO'S OWN REFUSAL REACHES THE OPERATOR. An IC-905 that takes no dial on 10 GHz answers NG
/// to the QO-100 downlink: the loop tries it three times, then stops, and the CAT status, the
/// refused-dial field and the satellite rail all say the RADIO refused 10489.5000 MHz. The dial
/// stays where the radio is. Before, the radio was never asked, and the rail carried Nexus's guess.
#[test]
fn a_downlink_the_radio_refuses_is_reported_as_the_radios_refusal() {
    let mut s = Scene::new(
        IcomModel::Ic905,
        3090,
        (144.2, "2m"),
        &[(144_000_000, 5_925_000_000)],
    );
    let n = s.wire_len();
    s.pick(10_489_500_000);
    s.settle(|s| s.told().1.is_some());
    assert_eq!(
        (s.dial_writes(n), s.main_hz(), s.told(), s.judged().0),
        (
            vec!["FE FE AC E0 05 00 00 50 89 04 01 FD".to_string(); 3],
            144_200_000,
            (
                "the radio refused 10489.5000 MHz — it does not cover that frequency; still on \
                 144.2000 MHz"
                    .to_string(),
                Some(10489.5),
                Some("The radio refused 10489.5000 MHz — the downlink was not tuned.".to_string()),
            ),
            144.2,
        )
    );
}

/// ⭐ A 2 m QSY AN IC-7300 REFUSES IS REPORTED AS REFUSED, not dropped: three tries, then the CAT
/// status and the refused-dial field name 145.0000 MHz, and the dial heals to the radio's own. The
/// ordinary QSY never consulted the range, so this held before the fix as well; it is the path every
/// pick the range used to refuse now takes.
#[test]
fn a_2_m_qsy_the_ic7300_refuses_is_reported_as_refused() {
    let mut s = Scene::new(
        IcomModel::Ic7300,
        3073,
        (14.074, "20m"),
        &[(30_000, 74_800_000)],
    );
    let n = s.wire_len();
    s.engine.lock().unwrap().set_frequency(145.0, "2m", "USB");
    s.settle(|s| s.told().1.is_some());
    assert_eq!(
        (s.dial_writes(n), s.main_hz(), s.told(), s.judged().0),
        (
            vec!["FE FE 94 E0 05 00 00 00 45 01 FD".to_string(); 3],
            14_074_000,
            (
                "the radio refused 145.0000 MHz — it does not cover that frequency; still on \
                 14.0740 MHz"
                    .to_string(),
                Some(145.0),
                None,
            ),
            14.074,
        )
    );
}

/// ⭐ THE LICENCE GATE DOES NOT READ THE RANGE. A Technician in Phone on the IC-905: 144.200 MHz is
/// all-mode and allowed, and a QSY to 144.050 MHz, in the 2 m CW-only segment, reaches the radio and
/// is TX-locked. The gate's own verdict on the two numbers is the other half.
#[test]
fn the_licence_gate_still_refuses_phone_in_the_2_m_cw_segment_on_an_ic905() {
    let mut s = Scene::new(IcomModel::Ic905, 3090, (144.2, "2m"), &[]);
    let before = s.judged();
    let n = s.wire_len();
    s.engine.lock().unwrap().set_frequency(144.05, "2m", "USB");
    s.settle(|s| s.write_then_read(n));
    let gate = |mhz: f64| {
        tempo_app::privileges::tx_allowed(
            LicenseClass::Technician,
            mhz,
            tempo_app::settings::OperatingMode::Phone,
        )
    };
    assert_eq!(
        (
            before,
            s.dial_writes(n),
            s.main_hz(),
            s.judged(),
            (gate(144.2), gate(144.05)),
        ),
        (
            (144.2, "2m".to_string(), true),
            vec!["FE FE AC E0 05 00 00 05 44 01 FD".to_string()],
            144_050_000,
            (144.05, "2m".to_string(), false),
            (true, false),
        )
    );
}

/// ⭐ A RADIO THAT DOES NOT ANSWER A DIAL WRITE IS NOT REFUSING IT. An IC-7300 on 20 m is asked for
/// 28.400 MHz, a dial it takes, and says nothing to the write. The operator is told the dial was
/// not sent because the rig did not reply, the loop keeps asking, and nothing records a refusal or
/// gives the dial up; when the radio answers again, the dial lands. The silence was counted as
/// three refusals: "the radio refused 28.4000 MHz — it does not cover that frequency", the dial
/// given up and healed back to 20 m, and the radio never asked again.
#[test]
fn a_dial_write_the_radio_does_not_answer_is_not_reported_as_refused() {
    let mut s = Scene::new(IcomModel::Ic7300, 3073, (14.074, "20m"), &[]);
    s.regs.lock().unwrap().drop_dial_writes = u32::MAX;
    let n = s.wire_len();
    s.engine.lock().unwrap().set_frequency(28.4, "10m", "USB");
    let past_the_budget = |s: &Scene| s.dial_writes(n).len() > DIAL_SET_MAX_TRIES as usize;
    let said = s.said_until(|s| past_the_budget(s) || s.told().1.is_some());
    let quiet = (said, past_the_budget(&s), s.told().1, s.state.dial_giveup);
    // The radio answers again.
    s.regs.lock().unwrap().drop_dial_writes = 0;
    s.settle(|s| s.main_hz() == 28_400_000);
    assert_eq!(
        (quiet, s.main_hz(), s.judged().0),
        (
            (
                vec!["28.4000 MHz not sent — no reply from the rig".to_string()],
                true,
                None,
                None,
            ),
            28_400_000,
            28.4,
        )
    );
}

/// ⭐ AN UNANSWERED DIAL WRITE FEEDS THE CIRCUIT BREAKER, NOT THE GIVE-UP. Counted at the loop's
/// second write, its first retry, where only the dial goes out: the QSY's own write comes with the
/// mode, whose ack proves the link and clears the breaker's misses. A silence is then one miss and
/// no refusal toward giving the dial up. An NG, the control, is two refusals and no miss. The
/// silence counted as a refusal.
#[test]
fn an_unanswered_dial_write_counts_toward_the_breaker_and_an_ng_toward_the_give_up() {
    let second_write = |covers_hz: &[(u64, u64)], silent: u32, to: (f64, &str)| {
        let mut s = Scene::new(IcomModel::Ic7300, 3073, (14.074, "20m"), covers_hz);
        s.regs.lock().unwrap().drop_dial_writes = silent;
        let n = s.wire_len();
        s.engine.lock().unwrap().set_frequency(to.0, to.1, "USB");
        let deadline = Instant::now() + Duration::from_secs(3);
        while s.dial_writes(n).len() < 2 && Instant::now() < deadline {
            s.step();
            // No heavy poll between the writes: its good read would clear the miss being counted.
            s.state.last_rig_poll = now_unix_ms() + 60_000.0;
        }
        (
            s.dial_writes(n).len(),
            s.state.freq_misses,
            s.state.dial_fail_count,
        )
    };
    assert_eq!(
        (
            second_write(&[], u32::MAX, (28.4, "10m")),
            second_write(&[(30_000, 74_800_000)], 0, (145.0, "2m")),
        ),
        ((2, 1, 0), (2, 0, 2))
    );
}

/// ⭐ A DIAL NOTE COMES DOWN WHEN THE DIAL LANDS. A dial write the radio did not answer, and one it
/// refused, are each followed by a write it takes: the note gives way to the confirmation, and the
/// radio is on the dial. A line published after the note (here a Test CAT result) is newer than
/// the dial and stays. The note stayed up over a radio that had taken the dial: "28.4000 MHz not
/// sent — no reply from the rig" with the radio on 28.400 MHz.
#[test]
fn a_dial_note_comes_down_when_the_dial_lands_and_a_newer_line_stays() {
    let confirmed = "CAT confirmed — rig accepted a command".to_string();
    // A silence, then the retry lands.
    let mut s = Scene::new(IcomModel::Ic7300, 3073, (14.074, "20m"), &[]);
    s.regs.lock().unwrap().drop_dial_writes = 1;
    s.engine.lock().unwrap().set_frequency(28.4, "10m", "USB");
    let silent = (s.said_until(|s| s.main_hz() == 28_400_000), s.main_hz());
    // An NG, then the radio takes the same dial on the retry; `newer` is published in between.
    let refused_then_taken = |newer: Option<&str>| {
        let mut s = Scene::new(
            IcomModel::Ic7300,
            3073,
            (14.074, "20m"),
            &[(30_000, 74_800_000)],
        );
        s.engine.lock().unwrap().set_frequency(145.0, "2m", "USB");
        let deadline = Instant::now() + Duration::from_secs(3);
        while !s.told().0.contains("(1/3)") && Instant::now() < deadline {
            s.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        let note = s.told().0;
        if let Some(line) = newer {
            s.engine
                .lock()
                .unwrap()
                .set_cat_status(Some(true), line.to_string());
        }
        s.regs.lock().unwrap().covers_hz.clear();
        let said = s.said_until(|s| s.main_hz() == 145_000_000);
        (note, said, s.told().0, s.main_hz())
    };
    assert_eq!(
        (
            silent,
            refused_then_taken(None),
            refused_then_taken(Some("Connected — 14.074 MHz")),
        ),
        (
            (
                vec![
                    "28.4000 MHz not sent — no reply from the rig".to_string(),
                    confirmed.clone(),
                ],
                28_400_000,
            ),
            (
                "145.0000 MHz refused by the rig (1/3)".to_string(),
                vec![confirmed.clone()],
                confirmed,
                145_000_000,
            ),
            (
                "145.0000 MHz refused by the rig (1/3)".to_string(),
                vec![],
                "Connected — 14.074 MHz".to_string(),
                145_000_000,
            ),
        )
    );
}
