//! #500: Tune in the CW section keys a real CW carrier through the CW keyer when that keyer can
//! hold a key down (a WinKeyer's Key Immediate, the serial keyline held), and every path that ends
//! the tune lets the key up BEFORE the unkey, since on a break-in radio the key alone keys the
//! transmitter. The field report: a Flex-8600 in CW with a WinKeyer went into transmit on Tune and
//! put out nothing, because Tune was CAT PTT plus a sound-card tone the radio in CW ignores.
//!
//! Each test runs the real `RadioLoop::step` against a rigctld that logs every line. The WinKeyer
//! is a pty standing in for the keyer (Linux only, #148, as the speed tests); at each `T 0` the
//! rigctld reads what the keyer has been sent by then, so the order across the two wires is
//! measured, not assumed. The serial keyline is the real keyer thread over a recording line.

use super::cw_stop::{lines_since, logging_rigctld, step};
use super::*;
use std::time::{Duration, Instant};

/// An engine in the CW section on 40 m, Extra class, with `keyer` chosen and `set` applied.
fn tune_engine(
    keyer: &str,
    set: impl FnOnce(&mut tempo_app::settings::Settings),
) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        set(&mut s);
        e.apply_settings(s);
        e.set_license_class("extra");
        e.set_cw_keyer(keyer, 600.0);
        e.set_operating_mode("cw", false);
        e.set_frequency(7.03, "40m", "CW");
        e.set_cw_wpm(20);
    }
    engine
}

/// The last Key Immediate (`0B nn`) on `wire`: `Some(true)` key down, `Some(false)` key up.
fn last_key_immediate(wire: &[u8]) -> Option<bool> {
    wire.windows(2)
        .rev()
        .find(|w| w[0] == 0x0B && w[1] <= 1)
        .map(|w| w[1] == 1)
}

#[cfg(all(target_os = "linux", feature = "serial"))]
mod winkeyer {
    use super::*;
    use serialport::SerialPort;
    use std::io::{Read, Write};

    /// A pty standing in for a WinKeyer: it answers Host Open as a real WK does, and keeps every
    /// byte Nexus sends it in `sent`.
    pub(super) struct Wire {
        pub port: String,
        master: Arc<Mutex<serialport::TTYPort>>,
        sent: Arc<Mutex<Vec<u8>>>,
        _slave: serialport::TTYPort,
    }

    impl Wire {
        pub fn new() -> Self {
            let (mut master, slave) = serialport::TTYPort::pair().expect("a pty pair");
            let port = slave.name().expect("the pts side has a path");
            // Short reads: a pull from inside the rigctld's `T 0` answer must not hold the unkey.
            master.set_timeout(Duration::from_millis(20)).unwrap();
            let mut responder = master.try_clone().expect("clone the pty master");
            responder.set_timeout(Duration::from_millis(3000)).unwrap();
            std::thread::spawn(move || {
                let mut host_open = [0u8; 2];
                if responder.read_exact(&mut host_open).is_ok() {
                    let _ = responder.write_all(&[0x17]); // WK2, revision 23
                    let _ = responder.flush();
                }
            });
            Self {
                port,
                master: Arc::new(Mutex::new(master)),
                sent: Arc::new(Mutex::new(Vec::new())),
                _slave: slave,
            }
        }

        /// A handle the rigctld's hook can pull with.
        pub fn puller(&self) -> impl Fn() -> Vec<u8> + Send + 'static {
            let (master, sent) = (Arc::clone(&self.master), Arc::clone(&self.sent));
            move || pull(&master, &sent)
        }

        /// Everything the keyer has been sent so far.
        pub fn sent(&self) -> Vec<u8> {
            pull(&self.master, &self.sent)
        }
    }

    fn pull(master: &Mutex<serialport::TTYPort>, sent: &Mutex<Vec<u8>>) -> Vec<u8> {
        let mut m = master.lock().unwrap();
        let mut buf = [0u8; 64];
        while let Ok(n) = m.read(&mut buf) {
            if n == 0 {
                break;
            }
            sent.lock().unwrap().extend_from_slice(&buf[..n]);
        }
        sent.lock().unwrap().clone()
    }

    /// A WinKeyer tune scene: the engine, the keyer's wire, the rigctld log, and the key state the
    /// keyer was in at each `T 0` the radio was sent.
    pub(super) struct Scene {
        pub engine: Arc<Mutex<Engine>>,
        pub wire: Wire,
        pub log: Arc<Mutex<Vec<String>>>,
        pub at_unkey: Arc<Mutex<Vec<Option<bool>>>>,
        pub at_key: Arc<Mutex<Vec<Option<bool>>>>,
        pub rig: Rig,
        pub backend: MockBackend,
        pub state: RadioLoop,
    }

    impl Scene {
        pub fn new(set: impl FnOnce(&mut tempo_app::settings::Settings)) -> Self {
            let wire = Wire::new();
            let port = wire.port.clone();
            let engine = tune_engine("winkeyer", move |s| {
                s.winkeyer_port = port;
                set(s);
            });
            let at_unkey = Arc::new(Mutex::new(Vec::new()));
            let at_key = Arc::new(Mutex::new(Vec::new()));
            let (unkeys, keys, pull) = (Arc::clone(&at_unkey), Arc::clone(&at_key), wire.puller());
            let (addr, log) = logging_rigctld(move |l| {
                match l {
                    "T 0" => unkeys.lock().unwrap().push(last_key_immediate(&pull())),
                    "T 1" => keys.lock().unwrap().push(last_key_immediate(&pull())),
                    _ => {}
                }
                None
            });
            let state = loop_state_for(&engine);
            Self {
                engine,
                wire,
                log,
                at_unkey,
                at_key,
                rig: Rig::rigctld(&addr),
                backend: MockBackend::new(),
                state,
            }
        }

        pub fn step(&mut self, t: f64) {
            step(
                &mut self.state,
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                t,
            );
        }

        /// Press Tune and run the tick that keys it; check it keyed through the keyer.
        pub fn tune(&mut self, t: f64) {
            self.engine.lock().unwrap().set_tune(true);
            self.step(t);
            assert!(self.state.tuning_keyed, "control: the tune keyed");
            assert_eq!(
                last_key_immediate(&self.wire.sent()),
                Some(true),
                "control: the keyer was keyed down: {:02x?}",
                self.wire.sent()
            );
        }

        /// The tune has ended keyed up: the keyer's last Key Immediate is up, it was up when the
        /// radio was sent its unkey, and the radio was unkeyed.
        pub fn assert_ended_keyed_up(&self, how: &str) {
            let wire = self.wire.sent();
            assert_eq!(
                last_key_immediate(&wire),
                Some(false),
                "{how}: the keyer must be keyed up (0B 00): {wire:02x?}"
            );
            assert_eq!(
                self.at_unkey.lock().unwrap().last(),
                Some(&Some(false)),
                "{how}: the keyer must already be up when the radio is unkeyed (T 0): {:?}",
                lines_since(&self.log, 0)
            );
            assert!(!self.rig.keyed, "{how}: the radio is unkeyed");
            assert!(!self.state.tuning_keyed, "{how}: the tune is over");
        }
    }

    /// ⭐ THE REPORT: Tune in CW with a WinKeyer keys the keyer down (Key Immediate `0B 01`) after
    /// the radio's PTT, and plays no tone (the radio in CW would not transmit it).
    #[test]
    fn a_cw_tune_on_a_winkeyer_keys_a_carrier_through_the_keyer() {
        let mut s = Scene::new(|_| {});
        s.tune(100.0);
        assert!(
            lines_since(&s.log, 0).iter().any(|l| l == "T 1"),
            "the radio is keyed as for every tune"
        );
        assert_eq!(
            s.at_key.lock().unwrap().last(),
            Some(&None),
            "the keyer is keyed down after the PTT, never before it"
        );
        s.step(120.0);
        s.step(140.0);
        assert!(
            s.backend.played.iter().all(|x| *x == 0.0),
            "no tune tone is played into a radio in CW"
        );
        assert_eq!(
            s.wire
                .sent()
                .windows(2)
                .filter(|w| *w == [0x0B, 0x01])
                .count(),
            1,
            "one key-down, held, not re-sent every tick"
        );
        assert!(s.state.tuning_keyed, "the tune is still up");
    }

    #[test]
    fn tune_off_keys_the_winkeyer_up_before_the_unkey() {
        let mut s = Scene::new(|_| {});
        s.tune(100.0);
        s.engine.lock().unwrap().set_tune(false);
        s.step(140.0);
        s.assert_ended_keyed_up("Tune off");
    }

    #[test]
    fn stop_tx_keys_the_winkeyer_up_before_the_unkey() {
        let mut s = Scene::new(|_| {});
        s.tune(100.0);
        s.engine.lock().unwrap().halt_tx();
        s.step(140.0);
        s.assert_ended_keyed_up("Stop TX");
    }

    /// The operator's Tune timeout (WSJT-X "Tune after t s") ends the carrier, keyed up.
    #[test]
    fn the_tune_timeout_keys_the_winkeyer_up_before_the_unkey() {
        let mut s = Scene::new(|s| s.tune_timeout_secs = 2);
        s.tune(100.0);
        s.step(1_900.0);
        assert!(s.state.tuning_keyed, "control: still inside the timeout");
        s.step(2_200.0);
        s.assert_ended_keyed_up("the Tune timeout");
        assert!(!s.engine.lock().unwrap().tuning(), "the tune is released");
    }

    /// The hard ceiling (`MAX_TUNE_MS`, 60 s) holds whatever the setting says: the tune's
    /// watchdog, and it ends the carrier keyed up too. (WK's own 100 s tune watchdog, which
    /// cannot be disabled, is beyond it.)
    #[test]
    fn the_tune_ceiling_keys_the_winkeyer_up_before_the_unkey() {
        let mut s = Scene::new(|s| s.tune_timeout_secs = 600);
        s.tune(100.0);
        s.step(100.0 + MAX_TUNE_MS - 100.0);
        assert!(s.state.tuning_keyed, "control: still inside the ceiling");
        s.step(100.0 + MAX_TUNE_MS + 100.0);
        s.assert_ended_keyed_up("the 60 s tune ceiling");
    }

    /// A keyer that will not open refuses the tune with no PTT at all, and says why.
    #[test]
    fn a_winkeyer_that_will_not_open_refuses_the_tune_without_keying() {
        let mut s = Scene::new(|_| {});
        {
            let mut e = s.engine.lock().unwrap();
            let mut st = e.settings().clone();
            st.winkeyer_port = "NO_SUCH_KEYER_PORT".into();
            e.apply_settings(st);
            e.set_tune(true);
        }
        s.step(100.0);
        s.step(140.0);
        assert!(
            !lines_since(&s.log, 0).iter().any(|l| l == "T 1"),
            "no PTT for a tune whose keyer is not there: {:?}",
            lines_since(&s.log, 0)
        );
        assert!(!s.engine.lock().unwrap().tuning(), "the tune is refused");
        assert!(!s.state.tuning_keyed);
        let said = s
            .engine
            .lock()
            .unwrap()
            .cw_keyer_error()
            .unwrap_or_default();
        assert!(said.contains("WinKeyer"), "the cockpit says why: {said:?}");
    }

    /// A keyer error mid-tune (the key-down will not go out) ends the tune, unkeyed, and says why.
    #[test]
    fn a_winkeyer_key_down_that_fails_ends_the_tune_unkeyed() {
        let mut s = Scene::new(|_| {});
        s.step(50.0); // the keyer opens, as it does once configured
        assert!(s.state.winkeyer.is_some(), "control: the keyer is open");
        // The keyer goes away under the open port: its far end closes, and every write fails.
        let (master, _) = serialport::TTYPort::pair().unwrap();
        *s.wire.master.lock().unwrap() = master;
        s.engine.lock().unwrap().set_tune(true);
        s.step(100.0);
        s.step(140.0);
        let said = s
            .engine
            .lock()
            .unwrap()
            .cw_keyer_error()
            .unwrap_or_default();
        assert!(said.contains("WinKeyer"), "the cockpit says why: {said:?}");
        assert!(!s.engine.lock().unwrap().tuning(), "the tune ends");
        assert!(!s.rig.keyed, "the radio is unkeyed");
        assert!(!s.state.tuning_keyed);
    }

    /// The keyer's port closes mid-tune (its setting cleared): the dropped keyer sends Clear
    /// Buffer, which cancels Tune (WK3 manual p.11), the tune ends and the radio is unkeyed.
    #[test]
    fn the_winkeyer_port_closing_mid_tune_ends_it_keyed_up() {
        let mut s = Scene::new(|_| {});
        s.tune(100.0);
        {
            let mut e = s.engine.lock().unwrap();
            let mut st = e.settings().clone();
            st.winkeyer_port = String::new();
            e.apply_settings(st);
        }
        s.step(140.0);
        s.step(160.0);
        let wire = s.wire.sent();
        let down = wire.windows(2).rposition(|w| w == [0x0B, 0x01]).unwrap();
        assert!(
            wire[down..].contains(&crate::winkeyer::CLEAR_BUFFER[0]),
            "Clear Buffer after the key-down: {wire:02x?}"
        );
        assert!(!s.engine.lock().unwrap().tuning(), "the tune ends");
        assert!(!s.rig.keyed, "the radio is unkeyed");
        assert!(!s.state.tuning_keyed);
    }

    /// App shutdown mid-tune: the keyer is let up before the shutdown's unkey.
    #[test]
    fn shutdown_keys_the_winkeyer_up_before_the_unkey() {
        let mut s = Scene::new(|_| {});
        s.tune(100.0);
        s.state
            .stop_for_shutdown(&s.engine, &mut s.backend, &mut s.rig);
        s.state.release_keyers_for_shutdown();
        // The shutdown leaves the loop's tune flags to the dropped state; the wire is the proof.
        let wire = s.wire.sent();
        assert_eq!(
            last_key_immediate(&wire),
            Some(false),
            "keyed up: {wire:02x?}"
        );
        assert_eq!(
            s.at_unkey.lock().unwrap().last(),
            Some(&Some(false)),
            "the keyer is up before the radio's unkey"
        );
        assert!(!s.rig.keyed, "the radio is unkeyed");
    }

    /// A CAT rebuild, an audio rebuild or the Test-CAT hold mid-tune (they clear the loop's TX
    /// state without reaching the tune's release): the keyer is let up before their unkey.
    #[test]
    fn a_context_change_keys_the_winkeyer_up_before_the_unkey() {
        let mut s = Scene::new(|_| {});
        s.tune(100.0);
        s.state
            .clear_loop_tx_for_context(&mut s.backend, &mut s.rig, "test: a rebuild");
        s.assert_ended_keyed_up("a rebuild");
    }

    /// The amplifier TX delay: the keyer keys down only once the delay has passed after PTT, as
    /// the tone waits out its queued silence.
    #[test]
    fn the_winkeyer_key_down_waits_out_the_amplifier_tx_delay() {
        let mut s = Scene::new(|s| s.amp_tx_delay_ms = 100);
        s.engine.lock().unwrap().set_tune(true);
        s.step(100.0);
        assert!(s.state.tuning_keyed, "control: the tune keyed");
        assert_eq!(
            last_key_immediate(&s.wire.sent()),
            None,
            "not down inside the delay"
        );
        s.step(150.0);
        assert_eq!(
            last_key_immediate(&s.wire.sent()),
            None,
            "not down inside the delay"
        );
        s.step(210.0);
        assert_eq!(
            last_key_immediate(&s.wire.sent()),
            Some(true),
            "down once it passed"
        );
    }

    /// Outside the CW section the WinKeyer is not the tune's carrier: the tone, as before.
    #[test]
    fn outside_the_cw_section_the_tune_does_not_touch_the_winkeyer() {
        let mut s = Scene::new(|_| {});
        s.engine
            .lock()
            .unwrap()
            .set_operating_mode("digital", false);
        s.engine.lock().unwrap().set_frequency(14.074, "20m", "USB");
        s.engine.lock().unwrap().set_tune(true);
        s.step(100.0);
        assert!(s.state.tuning_keyed, "control: the tune keyed");
        assert!(!s.backend.played.is_empty(), "the tone plays");
        assert_eq!(last_key_immediate(&s.wire.sent()), None, "no Key Immediate");
    }
}

#[cfg(feature = "serial")]
mod keyline {
    use super::*;

    /// The engine, the key line's recorded levels, the radio, the audio and the loop.
    type Scene = (
        Arc<Mutex<Engine>>,
        Arc<Mutex<Vec<bool>>>,
        Rig,
        MockBackend,
        RadioLoop,
    );

    /// A serial-keyline tune scene: the real keyer thread over a recording line, already open on
    /// the configured port and line so the loop keeps it.
    fn scene() -> Scene {
        let engine = tune_engine("serial", |s| {
            s.cw_key_port = "KEYLINE".into();
            s.cw_key_line = "dtr".into();
        });
        let levels = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&levels);
        let keyer = crate::serial_keyer::SerialKeyer::keying(move |d| seen.lock().unwrap().push(d));
        let mut state = loop_state_for(&engine);
        state.serial_keyer = Some(("KEYLINE".into(), "dtr".into(), keyer));
        let (addr, _log) = logging_rigctld(|_| None);
        (
            engine,
            levels,
            Rig::rigctld(&addr),
            MockBackend::new(),
            state,
        )
    }

    fn wait_last(levels: &Mutex<Vec<bool>>, down: bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            if levels.lock().unwrap().last() == Some(&down) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }

    #[test]
    fn a_cw_tune_on_the_serial_keyline_holds_the_key_down_until_tune_off() {
        let (engine, levels, mut rig, mut backend, mut state) = scene();
        engine.lock().unwrap().set_tune(true);
        step(&mut state, &engine, &mut backend, &mut rig, 100.0);
        assert!(wait_last(&levels, true), "the key line is held down");
        step(&mut state, &engine, &mut backend, &mut rig, 140.0);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(levels.lock().unwrap().last(), Some(&true), "and stays down");
        assert!(
            backend.played.iter().all(|x| *x == 0.0),
            "no tone into a radio in CW"
        );
        engine.lock().unwrap().set_tune(false);
        step(&mut state, &engine, &mut backend, &mut rig, 180.0);
        assert!(wait_last(&levels, false), "Tune off lets the key up");
        assert!(!rig.keyed && !state.tuning_keyed, "and unkeys");
    }

    #[test]
    fn stop_tx_lets_the_serial_keyline_tune_up() {
        let (engine, levels, mut rig, mut backend, mut state) = scene();
        engine.lock().unwrap().set_tune(true);
        step(&mut state, &engine, &mut backend, &mut rig, 100.0);
        assert!(wait_last(&levels, true), "control: held down");
        engine.lock().unwrap().halt_tx();
        step(&mut state, &engine, &mut backend, &mut rig, 140.0);
        assert!(wait_last(&levels, false), "Stop TX lets the key up");
        assert!(!rig.keyed && !state.tuning_keyed, "and unkeys");
    }

    #[test]
    fn the_tune_timeout_lets_the_serial_keyline_up() {
        let (engine, levels, mut rig, mut backend, mut state) = scene();
        {
            let mut e = engine.lock().unwrap();
            let mut s = e.settings().clone();
            s.tune_timeout_secs = 1;
            e.apply_settings(s);
            e.set_tune(true);
        }
        step(&mut state, &engine, &mut backend, &mut rig, 100.0);
        assert!(wait_last(&levels, true), "control: held down");
        step(&mut state, &engine, &mut backend, &mut rig, 1_200.0);
        assert!(wait_last(&levels, false), "the timeout lets the key up");
        assert!(!rig.keyed && !state.tuning_keyed, "and unkeys");
    }

    #[test]
    fn the_serial_keyline_port_closing_mid_tune_lets_it_up_and_ends_the_tune() {
        let (engine, levels, mut rig, mut backend, mut state) = scene();
        engine.lock().unwrap().set_tune(true);
        step(&mut state, &engine, &mut backend, &mut rig, 100.0);
        assert!(wait_last(&levels, true), "control: held down");
        {
            let mut e = engine.lock().unwrap();
            let mut s = e.settings().clone();
            s.cw_key_port = String::new();
            e.apply_settings(s);
        }
        step(&mut state, &engine, &mut backend, &mut rig, 140.0);
        step(&mut state, &engine, &mut backend, &mut rig, 160.0);
        assert_eq!(
            levels.lock().unwrap().last(),
            Some(&false),
            "the dropped keyer is up"
        );
        assert!(!engine.lock().unwrap().tuning(), "the tune ends");
        assert!(!rig.keyed && !state.tuning_keyed, "and unkeys");
    }
}

/// Where the CW keyer cannot hold a key down (the radio's own keyer over CAT), Tune stays PTT and
/// the tone, and the cockpit says plainly why that may be no carrier in CW.
#[test]
fn a_cw_tune_on_the_cat_keyer_says_why_there_may_be_no_carrier() {
    let engine = tune_engine("cat", |_| {});
    let (addr, log) = logging_rigctld(|_| None);
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    engine.lock().unwrap().set_tune(true);
    step(&mut state, &engine, &mut backend, &mut rig, 100.0);
    assert!(state.tuning_keyed, "the tune keys as before");
    assert!(lines_since(&log, 0).iter().any(|l| l == "T 1"));
    assert!(!backend.played.is_empty(), "with its tone, as before");
    assert_eq!(
        engine.lock().unwrap().cw_keyer_error().as_deref(),
        Some(TUNE_IN_CW_NO_CARRIER),
        "the cockpit says why there may be no carrier"
    );
}

/// …and outside CW, where the tone is the carrier, it says nothing.
#[test]
fn a_tune_outside_cw_says_nothing_about_cw() {
    let engine = tune_engine("cat", |_| {});
    {
        let mut e = engine.lock().unwrap();
        e.set_operating_mode("digital", false);
        e.set_frequency(14.074, "20m", "USB");
        e.set_tune(true);
    }
    let (addr, _log) = logging_rigctld(|_| None);
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    step(&mut state, &engine, &mut backend, &mut rig, 100.0);
    assert!(state.tuning_keyed, "control: the tune keyed");
    assert_eq!(engine.lock().unwrap().cw_keyer_error(), None);
}
