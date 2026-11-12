//! A program's audio over through Nexus's TCI server on the real radio loop (`RadioLoop::step`): it
//! keys on the program's first audio with Nexus's own key and plays that audio and no other, plays
//! out its tail on its owner's release, and ends hard (unkeyed, its queued audio flushed) within a
//! tick of every stop, its 300 ms gap, its 10-minute ceiling and its program's loss; a CAT rebuild
//! under it unkeys it through both links; a loop rebuilt under it plays none of it; and programs'
//! slot-timed overs stay refused, a program's data-mode over with them on any screen, and so does
//! the over of a program that asked no voice mode. The program is
//! the engine's own API as the server's backend calls it (`tci_key`, `tci_tx_audio`, `tci_release`,
//! `tci_end_own`, `tci_disconnected`, `tci_note_modulation`); the server and its wire are the app's,
//! tested there. The loop's monotonic clock is the test's (`mono_origin`), so a program's audio
//! arrives at the test's moments.

use super::cw_stop::logging_rigctld;
use super::udp_requests::halt_tx_datagram;
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempo_net::tci::server::{KeyKind, Mic, Refusal};
use tempo_net::tci::wire::Word;

/// The program that keys, and another one: both asked `usb` (`Scene::with`).
const P1: u64 = 1;
const P2: u64 = 2;
/// A program that has asked no mode, as WSJT-X, JTDX and MSHV ask none in their default setup.
const P3: u64 = 3;

/// The radio loop's tick in these scenes, ms.
const TICK: f64 = 20.0;

/// One 20 ms frame of a program's audio at the route's 12 kHz.
const FRAME: usize = TCI_OVER.samples(20) as usize;

/// The radio loop at a station in Phone on 20 m with every HF privilege, TX armed as entering Phone
/// arms it, "Programs on TCI may transmit" on, and P1 and P2 programs that asked `usb` (a program's
/// own audio goes out only after it asks a voice mode); a mock sound card that keeps what is played.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    origin: Instant,
    /// What a rebuild of the CAT link opens (a rigctld at this address, else a VOX rig), and how
    /// many times the loop rebuilt it.
    reopen: Option<String>,
    rebuilds: Arc<AtomicUsize>,
    /// A logger on the WSJT-X UDP feed, its socket and the loop's server for it.
    wsjtx: Option<(std::net::UdpSocket, WsjtxServer)>,
}

impl Scene {
    /// On a VOX rig.
    fn new() -> Scene {
        Scene::with(Rig::vox(), |_| {})
    }

    /// On `rig`, the station's settings changed by `tweak` first. The loop is built on the
    /// station's own transport, so it rebuilds its link only when a scene changes the settings.
    fn with(rig: Rig, tweak: impl FnOnce(&mut tempo_app::settings::Settings)) -> Scene {
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        let applied = {
            let mut e = engine.lock().unwrap();
            let mut s = e.settings().clone();
            s.tci_server_tx = true;
            tweak(&mut s);
            e.apply_settings(s);
            e.set_license_class("extra");
            e.set_operating_mode("phone", false);
            e.set_frequency(14.250, "20m", "USB");
            assert!(e.tx_enabled(), "premise: entering Phone arms TX");
            let usb = Word::new("usb").expect("a mode word");
            e.tci_note_modulation(P1, &usb);
            e.tci_note_modulation(P2, &usb);
            Transport::from_settings(e.settings())
        };
        let cfg = RadioConfig {
            rig_model: applied.rig_model,
            ..RadioConfig::default()
        };
        let origin = Instant::now();
        let mut state = RadioLoop::new(applied, None, &cfg);
        state.mono_origin = Some(origin);
        Scene {
            engine,
            state,
            backend: MockBackend::new(),
            rig,
            origin,
            reopen: None,
            rebuilds: Arc::default(),
            wsjtx: None,
        }
    }

    /// The same scene with a logger on the loop's WSJT-X UDP feed.
    fn with_logger(mut self) -> Scene {
        let logger = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let server =
            WsjtxServer::new("127.0.0.1:0".parse().unwrap(), logger.local_addr().unwrap()).unwrap();
        self.wsjtx = Some((logger, server));
        self
    }

    fn engine(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap()
    }

    fn mono(&self, t: f64) -> Instant {
        self.origin + Duration::from_secs_f64(t / 1000.0)
    }

    /// `trx:0,true,tci` from `program`, its audio stream started.
    fn key(&self, program: u64) -> Result<(), Refusal> {
        self.engine().tci_key(program, KeyKind::Tci, true)
    }

    /// 20 ms of `program`'s audio at `level`, arriving at `t`: whether the engine took it.
    fn audio(&self, program: u64, t: f64, level: f32) -> bool {
        let at = self.mono(t);
        self.engine().tci_tx_audio(program, &[level; FRAME], at)
    }

    /// The loop's step at `t` ms.
    fn step(&mut self, t: f64) {
        let sinks = Sinks {
            wsjtx: self.wsjtx.as_ref().map(|(_, server)| server),
            ..no_sinks()
        };
        let (reopen, rebuilds) = (self.reopen.clone(), self.rebuilds.clone());
        let mut reopen_rig = move |_t: &Transport, _coexist: bool| -> RigOpen {
            rebuilds.fetch_add(1, Ordering::SeqCst);
            match &reopen {
                Some(addr) => (
                    Rig::rigctld(addr),
                    None,
                    CatProbe::status(Some(true), "reopened"),
                ),
                None => (Rig::vox(), None, CatProbe::status(None, "")),
            }
        };
        let mut reopen_audio = mock_reopen_audio();
        let mut station = StationSinks::new();
        self.state
            .step(
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                &sinks,
                t,
                &mut reopen_audio,
                &mut reopen_rig,
                &mut station,
            )
            .unwrap();
    }

    /// One tick: P1's 20 ms of audio at `level`, when there is some, then the step.
    fn tick(&mut self, t: f64, level: Option<f32>) {
        if let Some(level) = level {
            self.audio(P1, t, level);
        }
        self.step(t);
    }

    /// P1 admitted at 0 ms and its audio from 20 ms on, every tick to `until`: a keyed over, fed.
    fn keyed(mut self, until: f64) -> Scene {
        assert_eq!(self.key(P1), Ok(()), "premise: P1's key is admitted");
        self.step(0.0);
        let mut t = TICK;
        while t <= until {
            self.tick(t, Some(0.5));
            t += TICK;
        }
        assert!(self.rig.keyed, "premise: P1's audio keyed the rig");
        assert!(self.state.program_over_keyed, "premise: the loop keyed it");
        self
    }
}

/// A rigctld that logs every line, on the scene's dial, so the loop's reads never move the station.
fn rigctld_on_the_dial() -> (String, Arc<Mutex<Vec<String>>>) {
    logging_rigctld(|line| match line {
        "f" => Some("14250000\n".to_string()),
        "m" => Some("USB\n2400\n".to_string()),
        _ => None,
    })
}

/// The key-downs on a rigctld's wire, in order.
fn key_downs(log: &Mutex<Vec<String>>) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with("T ") && l.as_str() != "T 0")
        .cloned()
        .collect()
}

/// ⭐ A PROGRAM'S OVER KEYS ON ITS FIRST AUDIO AND PLAYS THAT AUDIO. Admitted (`trx:0,true,tci`), it
/// keys nothing and plays nothing for two seconds with no audio. Its first frame keys the rig;
/// nothing is fed inside the 60 ms lead-in; then 40 ms of silent pre-roll, then the program's own
/// audio at its own level, and nothing else.
#[test]
fn a_programs_over_keys_on_its_first_audio_and_plays_it() {
    let mut s = Scene::new();
    assert_eq!(s.key(P1), Ok(()), "premise: admitted");
    let mut t = 0.0;
    for _ in 0..100 {
        s.step(t);
        assert!(
            !s.rig.keyed,
            "the admission keyed the rig at {t} ms with no audio"
        );
        t += TICK;
    }
    assert!(
        s.backend.played.is_empty(),
        "samples reached the transmitter with no audio"
    );
    assert!(s.engine().tci_over_armed(), "premise: the over is armed");
    s.tick(t, Some(0.5));
    assert!(s.rig.keyed, "the first audio did not key the rig");
    let keyed_at = t;
    while t < keyed_at + 40.0 {
        t += TICK;
        s.tick(t, Some(0.5));
        assert!(
            s.backend.played.is_empty(),
            "audio was fed {} ms into the lead-in",
            t - keyed_at
        );
    }
    for _ in 0..20 {
        t += TICK;
        s.tick(t, Some(0.5));
    }
    let played = &s.backend.played;
    assert!(!played.is_empty(), "nothing was fed after the lead-in");
    // 40 ms at 12 kHz.
    let pre_roll = 480;
    assert!(
        played[..pre_roll].iter().all(|&x| x == 0.0),
        "the pre-roll carried audio"
    );
    assert!(
        played[pre_roll..].contains(&0.5),
        "the program's audio never followed"
    );
    assert!(
        played.iter().all(|&x| x == 0.0 || x == 0.5),
        "audio that was not the program's was played"
    );
}

/// ⛔ ITS AUDIO STOPS, AND THE OVER ENDS 300 MS LATER: PTT released and the queued audio flushed
/// between 300 and 320 ms after the program's last frame (the gap by value, and one tick). CONTROL:
/// a pause of 80 ms, inside the over's lead, does not end it.
#[test]
fn a_programs_over_ends_300_ms_after_its_audio_stops_and_a_pause_does_not() {
    let mut s = Scene::new().keyed(600.0);
    s.backend.flush_calls = 0;
    let last = 600.0;
    let mut released = None;
    let mut t = last + TICK;
    while t <= last + 600.0 {
        s.tick(t, None);
        if !s.rig.keyed && released.is_none() {
            released = Some(t - last);
        }
        t += TICK;
    }
    let after = released.expect("PTT stayed up with no audio arriving");
    assert!(
        (300.0..=320.0).contains(&after),
        "PTT released {after} ms after the last frame"
    );
    assert!(
        s.backend.flush_calls > 0,
        "the queued audio was not flushed"
    );
    assert!(!s.engine().tci_over_armed(), "the over survived its gap");

    let mut s = Scene::new().keyed(600.0);
    let mut t = 620.0;
    while t < 600.0 + 80.0 {
        s.tick(t, None);
        t += TICK;
    }
    while t <= 1_400.0 {
        s.tick(t, Some(0.5));
        assert!(s.rig.keyed, "an 80 ms pause unkeyed the rig (at {t} ms)");
        t += TICK;
    }
}

/// ⛔ THE 10-MINUTE CEILING ENDS A PROGRAM'S OVER WITHIN A TICK, its audio still arriving on every
/// tick: aged to 9 minutes 50 seconds from its key it goes on (CONTROL), and on the first tick past
/// 10 minutes (by value) the rig is unkeyed, what was queued is flushed, the over is gone, and
/// nothing keys or plays after it. The age comes from the engine's test hook, which moves only the
/// over's key time back (`Engine::age_tci_over_for_test`); the ceiling and its clock are the real
/// ones.
#[test]
fn a_programs_over_ends_at_its_ten_minute_ceiling_within_a_tick() {
    let mut s = Scene::new().keyed(600.0);
    s.engine().age_tci_over_for_test(590_000);
    let mut t = 620.0;
    for _ in 0..5 {
        s.tick(t, Some(0.5));
        assert!(
            s.rig.keyed,
            "control: the over ended at 9 minutes 50 seconds (at {t} ms)"
        );
        t += TICK;
    }
    s.backend.flush_calls = 0;
    s.engine().age_tci_over_for_test(10_000);
    s.tick(t, Some(0.5));
    assert!(!s.rig.keyed, "still keyed a tick past 10 minutes");
    assert!(
        s.backend.flush_calls > 0,
        "the queued audio was not flushed"
    );
    assert!(s.state.tx_until_ms.is_none(), "the PTT hold survived");
    assert!(
        !s.engine().tci_over_armed(),
        "the over survived its ceiling"
    );
    let played = s.backend.played.len();
    for _ in 0..20 {
        t += TICK;
        s.tick(t, Some(0.5));
        assert!(!s.rig.keyed, "keyed again at {t} ms");
    }
    assert_eq!(
        s.backend.played.len(),
        played,
        "audio fed after the ceiling"
    );
}

/// ⛔ A WEDGED LOOP UNKEYS: on every tick of a long over the audio fed is never more than 40 ms
/// past real time, and the unkey deadline never more than 350 ms out (the lead-in, the look-ahead
/// and the tail, by value), so a loop that stops ticking lets go of the transmitter. CONTROL: the
/// over keeps up, fed at the loop's rate.
#[test]
fn a_programs_over_is_fed_40_ms_ahead_and_its_deadline_follows_it() {
    let mut s = Scene::new().keyed(20.0);
    let first_feed = 20.0 + 60.0;
    let mut t = 40.0;
    while t <= 3_000.0 {
        s.tick(t, Some(0.5));
        let fed_ms = s.backend.played.len() as f64 / 12.0;
        let real_ms = (t - first_feed).max(0.0);
        assert!(
            fed_ms <= real_ms + 40.0 + 1e-6,
            "at {t} ms, {fed_ms:.1} ms fed against {real_ms:.1} ms of real time"
        );
        let hold = s
            .state
            .tx_until_ms
            .expect("a keyed over holds PTT to a deadline")
            - t;
        assert!(
            hold <= 350.0,
            "at {t} ms the unkey deadline was {hold:.0} ms out"
        );
        t += TICK;
    }
    let fed_ms = s.backend.played.len() as f64 / 12.0;
    assert!(
        fed_ms >= 3_000.0 - first_feed - 40.0,
        "only {fed_ms:.0} ms fed in 3 s"
    );
}

/// ⛔ AUDIO ONLY FROM THE OVER'S OWNER: while P1's over plays, P2 sends audio at a level of its own
/// on every tick, and none of it reaches the transmitter (the engine takes none of it either).
/// CONTROL: P1's does.
#[test]
fn no_other_programs_audio_reaches_the_transmitter() {
    let mut s = Scene::new().keyed(200.0);
    let mut taken = Vec::new();
    let mut t = 220.0;
    while t <= 1_200.0 {
        if s.audio(P2, t, 0.9) {
            taken.push(t);
        }
        s.tick(t, Some(0.5));
        t += TICK;
    }
    assert!(
        !s.backend.played.contains(&0.9),
        "P2's audio reached the transmitter"
    );
    assert!(taken.is_empty(), "the engine took P2's audio at {taken:?}");
    assert!(s.rig.keyed, "premise: P1's over went on");
    assert!(s.backend.played.contains(&0.5), "control: P1's audio did");
}

/// ⛔ EVERY STOP ENDS A PROGRAM'S OVER HARD, WITHIN A TICK: on the loop's next tick the rig is
/// unkeyed, what was queued is flushed and the PTT hold is gone; for twenty ticks after, with the
/// program still sending, nothing keys and nothing is played; and the program's next key gets the
/// answer the stop leaves (a Stop holds until the operator arms; a band or radio change holds
/// programs' keys; the program's own end, a new route or the loop's rig changing hands hold
/// nothing; its loss forgets its mode ask, so its number is a program that asked no voice mode).
#[test]
fn every_stop_ends_a_programs_over_within_a_tick() {
    type Tweak = fn(&mut tempo_app::settings::Settings);
    type Stop = fn(&mut Scene);
    let as_is: Tweak = |_| {};
    let rows: [(&str, Tweak, Stop, Result<(), Refusal>); 13] = [
        ("its stop;", as_is, |s| s.engine().tci_end_own(P1), Ok(())),
        (
            "its lost connection",
            as_is,
            |s| s.engine().tci_disconnected(P1),
            Err(Refusal::VoiceMode),
        ),
        (
            "another program's trx:0,false",
            as_is,
            |s| {
                assert!(s.engine().tci_release(P2), "premise: it halted");
            },
            Err(Refusal::StopHold),
        ),
        (
            "Stop TX, Esc and the Remote's Stop (each is halt_tx)",
            as_is,
            |s| s.engine().halt_tx(),
            Err(Refusal::StopHold),
        ),
        (
            "TX Off",
            as_is,
            |s| s.engine().set_tx_enabled_by_operator(false),
            Err(Refusal::StopHold),
        ),
        (
            "a logger's UDP HaltTx",
            as_is,
            |s| {
                let (logger, server) = s.wsjtx.as_ref().expect("the scene's logger");
                logger
                    .send_to(&halt_tx_datagram(), server.local_addr().unwrap())
                    .unwrap();
                std::thread::sleep(Duration::from_millis(20));
            },
            Err(Refusal::StopHold),
        ),
        (
            "the high-SWR cutoff",
            |st| {
                // An IC-7300 on Nexus's own CI-V, whose SWR scale is verified, cutting off over 2.5:1.
                st.rig_model = 3073;
                st.rig_conn = "serial".into();
                st.serial_port = "/dev/tempo-test-ic7300".into();
                st.icom_native_cat = true;
                st.swr_stop_enabled = true;
                st.swr_stop_threshold = 2.5;
            },
            |s| {
                let mut e = s.engine();
                assert!(
                    e.settings().swr_stop_ratio().is_some(),
                    "premise: the cutoff is in force"
                );
                // Two readings over the cutoff, as the keyed-only meter poll delivers them.
                e.observe_rig_tx_meters(Some(3.4), None, None, None);
                e.observe_rig_tx_meters(Some(3.4), None, None, None);
            },
            Err(Refusal::StopHold),
        ),
        (
            "a band change",
            as_is,
            |s| s.engine().set_frequency(7.200, "40m", "LSB"),
            Err(Refusal::ContextHold),
        ),
        (
            "a radio switch (every one halts through the engine's context change)",
            as_is,
            |s| s.engine().halt_tx_for_context_change("radio handoff"),
            Err(Refusal::ContextHold),
        ),
        (
            "Programs on TCI may transmit switched off",
            as_is,
            |s| {
                let mut e = s.engine();
                let mut st = e.settings().clone();
                st.tci_server_tx = false;
                e.apply_settings(st);
            },
            Err(Refusal::Switch),
        ),
        (
            "the transmit route replaced",
            as_is,
            |s| s.state.tx_route_gen += 1,
            Ok(()),
        ),
        (
            "the radio took its own microphone",
            as_is,
            |s| s.engine().observe_flex_radio_has_mic(true),
            Err(Refusal::RadioMic),
        ),
        (
            "a deferred radio switch: the loop's rig is the radio being left",
            as_is,
            |s| s.state.handoff_deferred = true,
            Ok(()),
        ),
    ];
    for (name, tweak, stop, next_key) in rows {
        let mut s = Scene::with(Rig::vox(), tweak).with_logger().keyed(600.0);
        s.backend.flush_calls = 0;
        stop(&mut s);
        s.tick(620.0, Some(0.5));
        assert!(!s.rig.keyed, "{name}: still keyed a tick later");
        assert!(
            s.backend.flush_calls > 0,
            "{name}: the queued audio was not flushed"
        );
        assert!(
            s.state.tx_until_ms.is_none(),
            "{name}: the PTT hold survived"
        );
        assert!(!s.engine().tci_over_armed(), "{name}: the over survived");
        let played = s.backend.played.len();
        let mut t = 640.0;
        for _ in 0..20 {
            s.tick(t, Some(0.5));
            assert!(!s.rig.keyed, "{name}: keyed again at {t} ms");
            t += TICK;
        }
        assert_eq!(
            s.backend.played.len(),
            played,
            "{name}: audio fed after the stop"
        );
        assert_eq!(s.key(P1), next_key, "{name}: P1's next key");
        assert_eq!(
            s.rebuilds.load(Ordering::SeqCst),
            0,
            "{name}: premise: the loop kept its link"
        );
    }
}

/// ⭐ ITS OWNER'S RELEASE PLAYS OUT ITS TAIL (`trx:0,false` from the program that keyed, the graceful
/// end): every frame it sent before the release reaches the transmitter, its last one included;
/// nothing is flushed while the rig stays keyed; its audio after the release is not taken; and the
/// loop's deadline unkeys after the last of it, between 250 and 400 ms after that last frame was
/// fed (the tail, by value). CONTROL: Stop TX at the same moment unkeys and flushes on the next
/// tick, and the last frame never goes out.
#[test]
fn the_owners_release_plays_out_its_tail_and_the_deadline_unkeys_after_it() {
    let level = |t: f64| 0.1 + 0.001 * (t / TICK).round() as f32;
    for release in [true, false] {
        let what = if release { "the release" } else { "Stop TX" };
        let mut s = Scene::new();
        assert_eq!(s.key(P1), Ok(()), "premise: admitted");
        s.step(0.0);
        let mut t = TICK;
        while t <= 600.0 {
            s.audio(P1, t, level(t));
            s.step(t);
            t += TICK;
        }
        assert!(s.rig.keyed, "premise: keyed");
        let last = level(600.0);
        assert!(
            !s.backend.played.contains(&last),
            "premise: the last frame is still to go out"
        );
        s.backend.flush_calls = 0;
        if release {
            s.engine().tci_release(P1);
        } else {
            s.engine().halt_tx();
        }
        let (mut last_fed, mut unkeyed, mut flushed_keyed, mut taken) = (None, None, false, false);
        while t <= 2_000.0 {
            taken |= s.audio(P1, t, 0.9);
            s.step(t);
            if last_fed.is_none() && s.backend.played.contains(&last) {
                last_fed = Some(t);
            }
            flushed_keyed |= s.rig.keyed && s.backend.flush_calls > 0;
            if unkeyed.is_none() && !s.rig.keyed {
                unkeyed = Some(t);
            }
            t += TICK;
        }
        let unkeyed = unkeyed.unwrap_or_else(|| panic!("{what}: the rig was never unkeyed"));
        assert!(!taken, "{what}: audio after it was taken");
        assert!(
            !s.backend.played.contains(&0.9),
            "{what}: audio after it was played"
        );
        if release {
            let last_fed =
                last_fed.expect("the release cut the over: its last frame never went out");
            assert!(!flushed_keyed, "the release flushed what was queued");
            assert!(
                (250.0..=400.0).contains(&(unkeyed - last_fed)),
                "unkeyed {} ms after the last frame was fed",
                unkeyed - last_fed
            );
        } else {
            assert_eq!(last_fed, None, "control: Stop TX let the last frame out");
            assert_eq!(unkeyed, 620.0, "control: Stop TX unkeyed late");
            assert!(
                s.backend.flush_calls > 0,
                "control: Stop TX flushed nothing"
            );
        }
    }
}

/// ⛔ ONCE A PROGRAM'S OVER HAS PLAYED OUT, THE LOOP HOLDS NOTHING OF IT: the operator's own PTT,
/// pressed in that over's tail (the deadline not yet passed, the rig still keyed for it), stays keyed
/// through the deadline and after it. CONTROL: the operator's release unkeys.
#[test]
fn the_operators_ptt_pressed_in_a_played_out_overs_tail_stays_keyed() {
    let mut s = Scene::new().keyed(600.0);
    s.engine().tci_release(P1);
    let mut t = 620.0;
    while s.engine().tci_over_armed() {
        assert!(t < 1_200.0, "premise: the over never played out");
        s.tick(t, None);
        t += TICK;
    }
    assert!(
        s.rig.keyed && s.state.tx_until_ms.is_some(),
        "premise: inside the over's tail"
    );
    s.engine().set_ptt(true);
    let until = t + 1_000.0;
    while t <= until {
        s.tick(t, None);
        assert!(s.rig.keyed, "the operator's PTT was let go at {t} ms");
        t += TICK;
    }
    assert!(
        s.state.manual_ptt_applied,
        "premise: the operator's PTT keyed"
    );
    s.engine().set_ptt(false);
    s.tick(t, None);
    assert!(
        !s.rig.keyed,
        "control: the operator's release did not unkey"
    );
}

/// ⛔ NEXUS'S OWN KEY WINS: the operator's PTT pressed during a program's over takes the transmitter
/// on the next tick. The over ends, what it queued is flushed, none of the program's audio is
/// played after it, and the rig stays keyed for the operator.
#[test]
fn the_operators_ptt_takes_the_transmitter_from_a_programs_over() {
    let mut s = Scene::new().keyed(600.0);
    s.backend.flush_calls = 0;
    s.engine().set_ptt(true);
    s.tick(620.0, Some(0.5));
    assert!(!s.engine().tci_over_armed(), "the over went on");
    assert!(
        s.backend.flush_calls > 0,
        "its queued audio was not flushed"
    );
    assert!(
        s.state.manual_ptt_applied && s.rig.keyed,
        "the operator's PTT did not key"
    );
    let played = s.backend.played.len();
    let mut t = 640.0;
    for _ in 0..20 {
        s.tick(t, Some(0.5));
        t += TICK;
    }
    assert_eq!(
        s.backend.played.len(),
        played,
        "the program's audio played under the operator's PTT"
    );
    assert!(s.rig.keyed, "the operator's PTT was let go");
}

/// ⛔ THE LOOP POLLS THE PROGRAMS ON EVERY TICK, AN OVER ARMED OR NOT: a program's microphone key
/// (`trx:0,true`, the radio's own microphone) that a gate ended does not come back when the gate
/// goes away. Here the gate is the operator's own PTT pressed on top of it: released, the radio
/// unkeys and stays unkeyed, and the program holds nothing. CONTROL: before the press, its key
/// keys the radio.
#[test]
fn a_programs_microphone_key_a_gate_ended_never_comes_back() {
    let mut s = Scene::new();
    assert_eq!(
        s.engine().tci_key(P1, KeyKind::Mic(Mic::Selected), false),
        Ok(()),
        "premise: its microphone key"
    );
    s.step(0.0);
    assert!(s.rig.keyed, "control: the program's microphone key keyed");
    s.engine().set_ptt(true);
    s.step(20.0);
    assert!(s.rig.keyed, "premise: the operator's PTT holds the radio");
    s.engine().set_ptt(false);
    let mut t = 40.0;
    for _ in 0..20 {
        s.step(t);
        assert!(
            !s.rig.keyed,
            "the program's microphone key came back at {t} ms"
        );
        t += TICK;
    }
    assert_eq!(s.engine().tci_owner(), None, "the program kept its key");
}

/// ⛔ A PROGRAM'S AUDIO OVER IS NEXUS'S AUDIO, NEVER THE RADIO'S MICROPHONE: on a radio whose
/// transmit audio Nexus plays into the rear DATA input (`T 3` keys it, `T 1` the front microphone),
/// the over keys `T 3`, and no `T 1` goes out while it plays. CONTROL: the same program's microphone
/// key (`trx:0,true`, asking for the radio's own microphone) keys `T 1`.
#[test]
fn a_programs_over_keys_the_radios_data_input_never_its_microphone() {
    let (addr, log) = rigctld_on_the_dial();
    let mut s = Scene::with(Rig::with_control(Some(addr), PttMode::CatData), |_| {}).keyed(400.0);
    assert_eq!(key_downs(&log), ["T 3"], "the over's key-down");
    s.engine().tci_end_own(P1);
    s.tick(420.0, None);
    assert!(!s.rig.keyed, "premise: the over ended");
    assert_eq!(
        s.engine().tci_key(P1, KeyKind::Mic(Mic::Selected), false),
        Ok(()),
        "premise: its microphone key"
    );
    s.tick(440.0, None);
    assert!(s.rig.keyed, "premise: the microphone key keyed");
    assert_eq!(
        key_downs(&log),
        ["T 3", "T 1"],
        "control: the microphone key"
    );
}

/// ⛔ PROGRAMS' SLOT-TIMED OVERS STAY REFUSED THROUGH NEXUS'S SERVER (the operator's rule, until the
/// delay Nexus adds is measured at 0.2 s or less): in Digital, armed by the operator on FT8's 20 m
/// dial, a program's `trx:0,true,tci` is refused as slot-timed, none of its audio is taken, and two
/// seconds of it key nothing and play nothing. CONTROL: the same program in Phone keys and plays.
#[test]
fn a_programs_slot_timed_over_stays_refused_and_keys_nothing() {
    let mut s = Scene::new();
    {
        let mut e = s.engine();
        e.set_operating_mode("digital", false);
        e.set_frequency(14.074, "20m", "USB");
        e.set_tx_enabled_by_operator(true);
        assert!(e.tx_enabled(), "premise: the operator armed Digital");
    }
    let key = s.key(P1);
    let (mut taken, mut keyed_at) = (false, None);
    let mut t = 0.0;
    while t <= 2_000.0 {
        taken |= s.audio(P1, t, 0.5);
        s.step(t);
        if s.rig.keyed && keyed_at.is_none() {
            keyed_at = Some(t);
        }
        t += TICK;
    }
    assert_eq!(keyed_at, None, "the program's over keyed the rig");
    assert!(s.backend.played.is_empty(), "its audio was played");
    assert!(!taken, "its audio was taken");
    assert_eq!(key, Err(Refusal::SlotTimed), "the key");
    let s = Scene::new().keyed(200.0);
    assert!(
        s.backend.played.contains(&0.5),
        "control: in Phone it plays"
    );
}

/// ⛔ A DATA-MODE OVER FROM A PROGRAM IS HELD LIKE FT8 ON THE PHONE SCREEN TOO (the operator's ruling
/// of 2026-10-20): at 14.074 with the licence class Open, where no other gate refuses it, a program
/// whose last `modulation:` asked `digu` (WSJT-X's shape before its FT8) is refused as slot-timed,
/// and two seconds of its audio key nothing, play nothing and are not taken. CONTROL: once the same
/// program asks `usb`, its voice over keys and plays.
#[test]
fn a_programs_data_mode_over_on_the_phone_screen_keys_nothing() {
    let word = |w: &str| Word::new(w).expect("a mode word");
    let mut s = Scene::new();
    {
        let mut e = s.engine();
        e.set_license_class("open");
        e.set_frequency(14.074, "20m", "USB");
        e.tci_note_modulation(P1, &word("digu"));
    }
    let key = s.key(P1);
    let (mut taken, mut keyed_at) = (false, None);
    let mut t = 0.0;
    while t <= 2_000.0 {
        taken |= s.audio(P1, t, 0.5);
        s.step(t);
        if s.rig.keyed && keyed_at.is_none() {
            keyed_at = Some(t);
        }
        t += TICK;
    }
    assert_eq!(keyed_at, None, "the program's data-mode over keyed the rig");
    assert!(s.backend.played.is_empty(), "its audio was played");
    assert!(!taken, "its audio was taken");
    assert_eq!(key, Err(Refusal::SlotTimed), "the key");
    s.engine().tci_note_modulation(P1, &word("usb"));
    assert_eq!(s.key(P1), Ok(()), "control: its key after usb");
    while t <= 2_600.0 {
        s.tick(t, Some(0.5));
        t += TICK;
    }
    assert!(
        s.rig.keyed && s.backend.played.contains(&0.5),
        "control: after usb its voice over keys and plays"
    );
}

/// ⛔ A PROGRAM'S OWN AUDIO GOES OUT ONLY AFTER IT ASKS A VOICE MODE (the operator's ruling of
/// 2026-10-20): WSJT-X's default shape (its Radio mode "None": no `modulation:` at all, then
/// `trx:0,true,tci`) at 14.074 on the Phone screen with the licence class Open, where no other gate
/// refuses it, is refused as asking no voice mode, and two seconds of its audio key nothing, play
/// nothing and are not taken. CONTROL: once the same program asks `usb`, its voice over keys and
/// plays.
#[test]
fn a_programs_over_with_no_voice_mode_asked_keys_nothing_on_the_phone_screen() {
    let mut s = Scene::new();
    {
        let mut e = s.engine();
        e.set_license_class("open");
        e.set_frequency(14.074, "20m", "USB");
        assert!(
            e.tx_allowed(),
            "premise: the Open class keys on 14.074 in Phone"
        );
    }
    let key = s.key(P3);
    let (mut taken, mut keyed_at) = (false, None);
    let mut t = 0.0;
    while t <= 2_000.0 {
        taken |= s.audio(P3, t, 0.5);
        s.step(t);
        if s.rig.keyed && keyed_at.is_none() {
            keyed_at = Some(t);
        }
        t += TICK;
    }
    assert_eq!(keyed_at, None, "the program's over keyed the rig");
    assert!(s.backend.played.is_empty(), "its audio was played");
    assert!(!taken, "its audio was taken");
    assert_eq!(key, Err(Refusal::VoiceMode), "the key");
    s.engine()
        .tci_note_modulation(P3, &Word::new("usb").expect("a mode word"));
    assert_eq!(s.key(P3), Ok(()), "control: its key after usb");
    while t <= 2_600.0 {
        s.audio(P3, t, 0.5);
        s.step(t);
        t += TICK;
    }
    assert!(
        s.rig.keyed && s.backend.played.contains(&0.5),
        "control: after usb its voice over keys and plays"
    );
}

/// ⛔ NEVER REPLACE THE RIG WHILE KEYED (the stuck-TX rules): the operator saves another CAT port
/// while a program's over is keyed. The loop unkeys through the old link before letting it go, the
/// new link is unkeyed and never keyed after, the over is over, and its program's next key waits
/// for the operator's arm (the rebuild is a context change, which holds programs' keys in Phone).
#[test]
fn a_cat_rebuild_under_a_programs_over_unkeys_it_through_both_links() {
    let (old_addr, old_log) = rigctld_on_the_dial();
    let (new_addr, new_log) = rigctld_on_the_dial();
    let mut s = Scene::with(Rig::with_control(Some(old_addr), PttMode::Cat), |st| {
        st.rig_model = 2014;
        st.ptt_method = "cat".into();
        st.serial_port = "/dev/tempo-test-a".into();
    });
    s.reopen = Some(new_addr);
    let mut s = s.keyed(600.0);
    assert_eq!(
        key_downs(&old_log),
        ["T 1"],
        "premise: keyed on the old link"
    );
    {
        let mut e = s.engine();
        let mut st = e.settings().clone();
        st.serial_port = "/dev/tempo-test-b".into();
        e.apply_settings(st);
    }
    s.tick(620.0, Some(0.5));
    assert_eq!(
        s.rebuilds.load(Ordering::SeqCst),
        1,
        "premise: the loop rebuilt its link"
    );
    let old = old_log.lock().unwrap().clone();
    let key = old.iter().position(|l| l == "T 1").unwrap();
    assert!(
        old[key..].iter().any(|l| l == "T 0"),
        "the old link was let go keyed: {old:?}"
    );
    assert!(
        new_log.lock().unwrap().iter().any(|l| l == "T 0"),
        "the new link was not unkeyed"
    );
    assert!(
        !s.engine().tci_over_armed(),
        "the over survived the rebuild"
    );
    assert!(!s.rig.keyed, "the loop believes the rig keyed");
    let mut t = 640.0;
    for _ in 0..20 {
        s.tick(t, Some(0.5));
        t += TICK;
    }
    assert!(
        key_downs(&new_log).is_empty(),
        "the new link was keyed: {:?}",
        new_log.lock().unwrap()
    );
    assert_eq!(s.key(P1), Err(Refusal::ContextHold), "P1's next key");
}

/// ⛔ A LOOP REBUILT UNDER A KEYED OVER NEVER PLAYS IT: a fresh radio loop whose first tick finds the
/// engine's program over keyed (the radio thread started again under it) ends the over through its
/// one kill path, plays none of it, and lets go of the rig it finds keyed.
#[test]
fn a_loop_rebuilt_under_a_keyed_over_ends_it_and_plays_none_of_it() {
    let mut s = Scene::new().keyed(600.0);
    let applied = Transport::from_settings(s.engine().settings());
    s.state = RadioLoop::new(applied, None, &RadioConfig::default());
    s.state.mono_origin = Some(s.origin);
    let played = s.backend.played.len();
    s.tick(620.0, Some(0.5));
    assert!(
        !s.engine().tci_over_armed(),
        "the over went on under the new loop"
    );
    assert!(!s.rig.keyed, "the rig was left keyed");
    let mut t = 640.0;
    for _ in 0..20 {
        s.tick(t, Some(0.5));
        assert!(!s.rig.keyed, "keyed again at {t} ms");
        t += TICK;
    }
    assert_eq!(
        s.backend.played.len(),
        played,
        "the new loop played an over it never keyed"
    );
}

/// A program's over's hard end, behind `program_over_keyed`, pinned for the cut-site refactor
/// (`cut_sites`): Stop TX mid-over flushes, then unkeys, and drops the hold.
#[test]
fn stop_tx_mid_programs_over_flushes_then_unkeys_and_drops_the_hold() {
    let (addr, log) = rigctld_on_the_dial();
    let mut s = Scene::with(Rig::rigctld(&addr), |_| {});
    s.backend.events = Some(Arc::clone(&log));
    let mut s = s.keyed(600.0);
    let (from, flushes) = (log.lock().unwrap().len(), s.backend.flush_calls);
    s.engine().halt_tx();
    s.tick(620.0, Some(0.5));
    super::cut_sites::assert_cut(
        "TCI program",
        &log,
        from,
        flushes,
        &s.backend,
        s.state.tx_until_ms,
        &["flush", "T 0"],
    );
    assert!(!s.rig.keyed);
}
