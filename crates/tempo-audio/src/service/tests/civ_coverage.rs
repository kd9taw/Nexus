//! A NATIVE CI-V RADIO'S RANGE IS UNKNOWN TO THE ENGINE, never the wide row the CAT broker's
//! `\dump_state` carries for WSJT-X (135.7 kHz to 1.3 GHz). Checked through the real
//! `RadioLoop::step`, a `Rig` over TCP and Nexus's own CI-V daemon in front of a fake radio: what
//! the loop's capability probe hands the engine, what that does to a satellite pick, and what the
//! operator is told when the radio itself refuses a frequency, or does not answer at all.
//!
//! The fake radio takes every dial unless a scene gives it `covers_hz`; then it answers NG (`FA`)
//! outside those ranges. That a real radio NGs a dial it cannot tune is the fixture's model of
//! CI-V, not a bench measurement. Its `drop_dial_writes` makes it say nothing to a dial write: a
//! radio that went quiet, which is not a refusal. `drop_mode_writes` and `drop_split_writes` do the
//! same to a mode and a split, and `off` to everything.
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

/// A QSY from 20 m to 28.400 MHz on an IC-7300 that never answers the dial write, stepped as
/// `run_radio` steps for `window`. `off` switches the whole radio off; otherwise it answers
/// everything but a dial write. Returns what the operator was told about 28.400 MHz (each change
/// of the CAT status line naming it, in order), whether any status line said "refused", the `05`
/// writes on the wire, the dial the radio refused, the dial given up on, and the dials the engine
/// and the radio are left on. How long each step that carried a write held the loop goes to
/// stderr (`--nocapture`): every write to a silent radio holds it for the CI-V deadline.
#[allow(clippy::type_complexity)]
fn unanswered_dial(
    off: bool,
    window: Duration,
) -> (Vec<String>, bool, usize, Option<f64>, Option<u64>, f64, u64) {
    let mut s = Scene::new(IcomModel::Ic7300, 3073, (14.074, "20m"), &[]);
    {
        let mut r = s.regs.lock().unwrap();
        r.drop_dial_writes = u32::MAX;
        r.off = off;
    }
    let n = s.wire_len();
    s.engine.lock().unwrap().set_frequency(28.4, "10m", "USB");
    let (mut said, mut held_ms) = (Vec::new(), Vec::new());
    let mut last = s.told().0;
    let end = Instant::now() + window;
    while Instant::now() < end {
        let writes = s.dial_writes(n).len();
        let started = Instant::now();
        s.step();
        if s.dial_writes(n).len() > writes {
            held_ms.push(started.elapsed().as_millis());
        }
        let line = s.told().0;
        if line != last {
            said.push(line.clone());
            last = line;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let writes = s.dial_writes(n).len();
    eprintln!(
        "unanswered dial, radio {}: {writes} dial writes in {window:?}; the steps that carried \
         one held the loop {held_ms:?} ms",
        if off { "off" } else { "answering reads only" }
    );
    (
        said.iter()
            .filter(|l| l.contains("28.4000"))
            .cloned()
            .collect(),
        said.iter().any(|l| l.contains("refused")),
        writes,
        s.told().1,
        s.state.dial_giveup,
        s.judged().0,
        s.main_hz(),
    )
}

/// ⭐ A DIAL THE RADIO DOES NOT ANSWER IS SENT THREE TIMES, AND IS NOT CALLED REFUSED. The IC-7300
/// answers everything but the dial write. The loop sends the dial three times, as it did when it
/// counted the silence as a refusal, says each time that the rig did not reply, then gives the
/// dial up without recording a refusal and shows the dial the radio is really on. The silence was
/// "the radio refused 28.4000 MHz — it does not cover that frequency"; and once it stopped being a
/// refusal, the dial went out on every tick for as long as the radio stayed quiet.
#[test]
fn a_dial_the_radio_does_not_answer_is_sent_three_times_and_not_called_refused() {
    assert_eq!(
        unanswered_dial(false, Duration::from_secs(5)),
        (
            vec![
                "28.4000 MHz not sent — no reply from the rig (1/3)".to_string(),
                "28.4000 MHz not sent — no reply from the rig (2/3)".to_string(),
                "28.4000 MHz not sent — no reply from the rig after 3 tries; still on 14.0740 MHz"
                    .to_string(),
            ],
            false,
            3,
            None,
            Some(28_400_000),
            14.074,
            14_074_000,
        )
    );
}

/// ⭐ …AND THE SAME FOR A RADIO THAT IS SWITCHED OFF. Every frame goes unanswered, reads too, so the
/// circuit breaker trips once the daemon's cached dial runs out. The dial still goes out three
/// times, and no more across the trip; the heal reads the daemon's cached dial.
#[test]
fn a_dial_sent_to_a_radio_that_is_off_is_sent_three_times_and_not_called_refused() {
    assert_eq!(
        unanswered_dial(true, Duration::from_secs(8)),
        (
            vec![
                "28.4000 MHz not sent — no reply from the rig (1/3)".to_string(),
                "28.4000 MHz not sent — no reply from the rig (2/3)".to_string(),
                "28.4000 MHz not sent — no reply from the rig after 3 tries; still on 14.0740 MHz"
                    .to_string(),
            ],
            false,
            3,
            None,
            Some(28_400_000),
            14.074,
            14_074_000,
        )
    );
}

/// ⭐ AN UNANSWERED DIAL WRITE FEEDS THE CIRCUIT BREAKER AND THE GIVE-UP; AN NG THE GIVE-UP ALONE.
/// Counted at the loop's second write, its first retry, where only the dial goes out: the QSY's own
/// write comes with the mode, whose ack proves the link and clears the breaker's misses. A silence
/// is then one miss and two tries toward giving the dial up. An NG, the control, is two tries and
/// no miss. The silence was two tries and no miss when it counted as a refusal, and one miss and no
/// tries when it stopped counting at all.
#[test]
fn an_unanswered_dial_write_counts_toward_the_breaker_and_the_give_up_and_an_ng_toward_the_give_up()
{
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
        ((2, 1, 2), (2, 0, 2))
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
                    "28.4000 MHz not sent — no reply from the rig (1/3)".to_string(),
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

/// A switch from Phone (USB) to Digital (PKTUSB) on an IC-7300 on 20 m that answers everything but
/// a mode write, after the switch's own tick. `ng` answers each mode write NG (`FA`); otherwise it
/// says nothing to one. The scene starts the steady loop's try budget two short of
/// MODE_SET_MAX_TRIES, so it reaches the give-up in two tries; that every try before those counts
/// the same, silence or NG, is the loop's own arithmetic, unchanged here. Returns each change of
/// the CAT status line naming PKTUSB, in order, from the switch on, whether any of them called it
/// rejected or refused, the mode writes (`06`) on the wire after the switch's tick, the mode given
/// up on, and the radio's mode byte and DATA flag.
#[allow(clippy::type_complexity)]
fn a_mode_given_up(ng: bool) -> (Vec<String>, bool, usize, Option<String>, (u8, bool)) {
    let mut s = Scene::new(IcomModel::Ic7300, 3073, (14.074, "20m"), &[]);
    {
        let mut r = s.regs.lock().unwrap();
        if ng {
            r.nak_mode_writes = u32::MAX;
        } else {
            r.drop_mode_writes = u32::MAX;
        }
    }
    s.engine
        .lock()
        .unwrap()
        .set_operating_mode("digital", false);
    let mut said = Vec::new();
    let mut last = s.told().0;
    let mut note = |s: &Scene| {
        let line = s.told().0;
        if line != last {
            said.push(line.clone());
            last = line;
        }
    };
    s.step();
    note(&s);
    let n = s.wire_len();
    s.state.mode_fail_count = MODE_SET_MAX_TRIES - 2;
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.state.mode_giveup.is_none() && Instant::now() < deadline {
        s.step();
        note(&s);
        std::thread::sleep(Duration::from_millis(20));
    }
    let mode_writes = s.regs.lock().unwrap().wire[n..]
        .iter()
        .filter(|f| f.get(4) == Some(&0x06))
        .count();
    let r = s.regs.lock().unwrap();
    let radio = (r.main_mode, r.data_mode);
    drop(r);
    let said: Vec<String> = said.into_iter().filter(|l| l.contains("PKTUSB")).collect();
    let blamed = said
        .iter()
        .any(|l| l.contains("rejected") || l.contains("refused"));
    (
        said,
        blamed,
        mode_writes,
        s.state.mode_giveup.clone(),
        radio,
    )
}

/// ⭐ A MODE THE RADIO DOES NOT ANSWER IS NOT CALLED REJECTED, AND ITS GIVE-UP SENDS NO FALLBACK.
/// The IC-7300 answers everything but a mode write. Each try says the rig did not reply, the
/// give-up says the same and blames the link, and nothing more is sent: the two tries before it,
/// and the radio still in USB with DATA off. The silence was "rig rejected PKTUSB", and the give-up
/// sent the plain-USB fallback (a third `06`) to a radio that was not answering, then said "rig
/// refused PKTUSB". An NG, the control, keeps its words and its fallback.
#[test]
fn a_mode_the_radio_does_not_answer_is_not_called_rejected_and_its_give_up_sends_no_fallback() {
    let silent = [
        "no reply from the rig over CAT — couldn't set PKTUSB: rigctld mode: the rig did not \
         answer (Hamlib RPRT -5)",
        "no reply from the rig over CAT — couldn't set PKTUSB: rigctld mode: the rig did not \
         answer (Hamlib RPRT -5) (29/30)",
        "couldn't set PKTUSB: no reply over CAT — link too slow or rig mute; try raising the \
         CI-V baud to 115200 on the rig and in Settings ▸ Radio ▸ Rig & CAT, and turning CI-V \
         Transceive off — gave up",
    ];
    let ng = [
        "rig rejected PKTUSB: rigctld mode error: \"RPRT -1\\n\"",
        "rig rejected PKTUSB: rigctld mode error: \"RPRT -1\\n\" (29/30)",
        "rig refused PKTUSB — couldn't set DATA mode; select USB-D/DATA on the rig by hand — gave \
         up",
    ];
    assert_eq!(
        (a_mode_given_up(false), a_mode_given_up(true)),
        (
            (
                silent.map(String::from).to_vec(),
                false,
                2,
                Some("PKTUSB".to_string()),
                (0x01, false),
            ),
            (
                ng.map(String::from).to_vec(),
                true,
                3,
                Some("PKTUSB".to_string()),
                (0x01, false),
            ),
        )
    );
}

/// ⭐ AN UNANSWERED MODE WRITE FEEDS THE CIRCUIT BREAKER AND THE GIVE-UP; AN NG THE GIVE-UP ALONE.
/// Counted at the steady loop's second mode write after a switch to Digital, with the dial already
/// given up so that only the mode goes out (a dial the radio takes would prove the link and clear
/// the misses) and no heavy poll in between (its good read would clear them too). A silence is then
/// two misses and two tries; an NG, the control, two tries and no miss. The silence was two tries
/// and no miss: it counted as a refusal.
#[test]
fn an_unanswered_mode_write_counts_toward_the_breaker_and_the_give_up_and_an_ng_toward_the_give_up()
{
    let second_write = |ng: bool| {
        let mut s = Scene::new(IcomModel::Ic7300, 3073, (14.074, "20m"), &[]);
        {
            let mut r = s.regs.lock().unwrap();
            if ng {
                r.nak_mode_writes = u32::MAX;
            } else {
                r.drop_mode_writes = u32::MAX;
            }
        }
        s.engine
            .lock()
            .unwrap()
            .set_operating_mode("digital", false);
        s.step();
        let n = s.wire_len();
        s.state.dial_giveup = Some(14_074_000);
        let writes = |s: &Scene| {
            s.regs.lock().unwrap().wire[n..]
                .iter()
                .filter(|f| f.get(4) == Some(&0x06))
                .count()
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        while writes(&s) < 2 && Instant::now() < deadline {
            s.state.last_rig_poll = now_unix_ms() + 60_000.0;
            s.step();
        }
        (writes(&s), s.state.freq_misses, s.state.mode_fail_count)
    };
    assert_eq!(
        (second_write(false), second_write(true)),
        ((2, 2, 2), (2, 0, 2))
    );
}

/// ⭐ NO MODE WRITE REACHES A RADIO THAT IS OFF WHILE THE CIRCUIT BREAKER IS TRIPPED, nor a dial,
/// and the mode lands once the radio answers. An IC-7300 switched off as the operator moves Phone
/// to Digital: the mode goes out until the breaker trips on the dial read (the daemon's cached dial
/// runs out first), and the trip is on the link, so nothing more in the next three seconds. The
/// radio comes back, the breaker's next probe finds it, and PKTUSB lands. The trip was read as a
/// refusal (the daemon's `f` answers 0 for a radio that has stopped answering), so neither was
/// withheld, and the mode went out on every step of the trip, each one holding the loop for the
/// CI-V deadline of both its frames.
#[test]
fn no_mode_write_reaches_a_radio_that_is_off_while_the_breaker_is_tripped_and_it_lands_when_it_answers(
) {
    let mut s = Scene::new(IcomModel::Ic7300, 3073, (14.074, "20m"), &[]);
    s.regs.lock().unwrap().off = true;
    s.engine
        .lock()
        .unwrap()
        .set_operating_mode("digital", false);
    let mode_writes = |s: &Scene, n: usize| {
        s.regs.lock().unwrap().wire[n..]
            .iter()
            .filter(|f| f.get(4) == Some(&0x06))
            .count()
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    while s.state.cat_ok != Some(false) && Instant::now() < deadline {
        s.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(s.state.cat_ok, Some(false), "premise: the breaker tripped");
    let on_the_link = s.state.cat_down_link_fault;
    let n = s.wire_len();
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        s.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    let while_tripped = mode_writes(&s, n);
    // The radio answers again, and the breaker's next probe is due now.
    s.regs.lock().unwrap().off = false;
    s.state.cat_retry_at = 0.0;
    let landed = |s: &Scene| {
        let r = s.regs.lock().unwrap();
        (r.main_mode, r.data_mode) == (0x01, true)
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !landed(&s) && Instant::now() < deadline {
        s.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        (on_the_link, while_tripped, landed(&s), s.state.cat_ok),
        (true, 0, true, Some(true))
    );
}

/// ⭐ A SPLIT THE RADIO DOES NOT ANSWER IS NOT CALLED REJECTED. A pile-up's split up 2 kHz to an
/// IC-7300 that answers everything but the split: split on goes out once and its TX dial not at
/// all, as when the radio refuses it, and the operator is told the rig did not reply. An NG, the
/// control, keeps "rig rejected split". The silence was "rig rejected split" too.
#[test]
fn a_split_the_radio_does_not_answer_is_not_called_rejected() {
    let split = |knob: fn(&mut Regs)| {
        let mut s = Scene::new(IcomModel::Ic7300, 3073, (14.074, "20m"), &[]);
        knob(&mut s.regs.lock().unwrap());
        let n = s.wire_len();
        s.engine.lock().unwrap().request_split(Some(14.076));
        let said = s.said_until(|s| s.told().0.contains("split"));
        let r = s.regs.lock().unwrap();
        let sent = |cmd: u8| {
            r.wire[n..]
                .iter()
                .filter(|f| f.get(4) == Some(&cmd))
                .count()
        };
        (
            said.into_iter()
                .filter(|l| l.contains("split"))
                .collect::<Vec<_>>(),
            sent(0x0F),
            sent(0x25),
        )
    };
    assert_eq!(
        (
            split(|r| r.drop_split_writes = u32::MAX),
            split(|r| r.nak_split_writes = u32::MAX),
        ),
        (
            (
                vec![
                    "no reply from the rig — split not set; work the pile-up manually".to_string()
                ],
                1,
                0,
            ),
            (
                vec!["rig rejected split — work the pile-up manually".to_string()],
                1,
                0,
            ),
        )
    );
}
