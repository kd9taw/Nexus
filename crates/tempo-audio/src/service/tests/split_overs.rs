//! SPLIT OPERATION OVER CONSECUTIVE FT8 OVERS, by value, on the radio's side of the link.
//!
//! WSJT-X's rule (3.0.2, `widgets/mainwindow.cpp:12355-12403`, `MainWindow::setXIT`): with Split
//! Operation on, in FT8, FT4 and FST4, the TX dial is the RX dial plus `XIT = (n/500)*500 - 1500`,
//! `n` the TX audio offset, and the audio goes out at `n - XIT`, from 1500 up to 2000 Hz. The TX dial
//! is worked out again before every over (`guiUpdate`, :7854, just ahead of `transceiver_ptt(true)`).
//! Rig split writes the TX VFO at every key (`Transceiver/TransceiverBase.cpp:160-171`, "or about to
//! tx split") and leaves the rig in split between overs. Fake It moves the one VFO to the TX dial for
//! the over and back after it (`Transceiver/EmulateSplitTransceiver.cpp:26-41`), and a read-back that
//! equals the TX dial is reported as the RX dial, never taken for a new one (:56-64).
//!
//! These scenes run the real `RadioLoop::step`, a 20 ms tick at a time, as a station calling CQ on
//! 20 m FT8 with its TX offset at 1441 Hz (a -500 Hz step) or 941 Hz (-1000 Hz), and record what
//! reaches the radio on the tick that keys each over, on the tick that unkeys it, and while it
//! receives, and where its VFOs are when the next over is due. The radios are an IC-7610 and an
//! IC-7300 behind Nexus's own CI-V daemon (the register file of `FakeRadio`, which also keys here),
//! and a rigctld that holds two VFOs and a split flag. They pin what the loop sends: on a radio
//! that takes every command, and on radios that do not take the dial write that puts the dial back
//! after a Fake It over (the write-back), which is the case WSJT-X's read-back rule exists for. The
//! write-back goes out 100 ms after the unkey, as WSJT-X waits after PTT off
//! (`TransceiverBase.cpp:142`), and again on the next ticks while the radio has not taken it, up to
//! the dial's three tries.
//!
//! The loop's clock runs on the real clock's timebase, a little ahead of it, because an over's hold
//! is measured on the real clock (`slot::slot_tx_phase`), as in `failed_unkey`.

use super::*;
use crate::civ::broker::CivDaemon;
use crate::civ::commands::IcomModel;
use crate::civ::engine::tests_support::{FakeRadio, Regs};
use crate::civ::frame::{bcd_to_freq, Frame, CONTROLLER};
use crate::rig::remote_tests::{retuning_peer, Peer};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use tempo_app::settings::SplitMode;

/// The 20 m FT8 dial.
const FT8_DIAL: u64 = 14_074_000;

/// What reached the radio around one over: its dial, mode, split, VFO, offset and PTT writes.
#[derive(Debug, Default, Clone, PartialEq)]
struct Over {
    /// The dial the engine held on the tick that keyed it (Hz).
    dial: u64,
    /// When it keyed, in ms into its 15 s period.
    key_ms: f64,
    /// When it unkeyed, in ms into its 15 s period.
    unkey_ms: f64,
    /// The writes on the tick that keyed it, up to and including the key.
    key: Vec<String>,
    /// The writes on the tick that unkeyed it, from the unkey on.
    unkey: Vec<String>,
    /// Every write while it received afterwards, up to the next over's tick.
    rx: Vec<String>,
    /// When each of `rx` went out, in ms after the tick that unkeyed it.
    rx_ms: Vec<f64>,
    /// The dials the engine held while it received, each once, in order (Hz).
    rx_dials: Vec<u64>,
}

/// The radio's side of the link, whichever it is.
trait Radio {
    /// Every command the controller has sent, in order, as text; all but those writes marked `?`.
    fn sent(&self) -> Vec<String>;
    /// The radio's own PTT.
    fn keyed(&self) -> bool;
}

/// The loop calling CQ against `radio`, and the over-by-over record.
struct Bench<R: Radio> {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    radio: R,
    /// The start of the first over's slot (ms), an even FT8 slot ahead of the real clock.
    slot: f64,
    t: f64,
}

/// A CQ on 20 m FT8 at `tx_hz`, Split Operation `split`, the snappy first over drained so the
/// first even slot keys it. `setup` sets anything else.
fn cq_engine(
    split: SplitMode,
    tx_hz: f32,
    setup: impl FnOnce(&mut Settings),
) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        e.set_license_class("extra");
        let mut s = e.settings().clone();
        s.split_mode = split;
        setup(&mut s);
        e.apply_settings(s);
        e.set_tier(Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB");
        e.set_tx_offset(tx_hz);
        e.start_cq(None).expect("Call CQ");
        let _ = e.take_immediate_tx();
    }
    engine
}

impl<R: Radio> Bench<R> {
    fn new(engine: Arc<Mutex<Engine>>, state: RadioLoop, rig: Rig, radio: R) -> Self {
        let mut state = state;
        // The two once-per-connection probes are taken as done: against a stand-in `\dump_state`
        // has no single-line answer and would eat the heavy poll's read budget.
        state.rx_ranges_probed = true;
        state.tuner_probed = true;
        // 25 to 55 s ahead, so the receive period before the first over is ahead of it too.
        let slot = ((now_unix_ms() + 25_000.0) / 30_000.0).ceil() * 30_000.0;
        Bench {
            engine,
            state,
            backend: MockBackend::new(),
            rig,
            radio,
            slot,
            // The odd slot before it: the loop receives (and polls) before it transmits.
            t: slot - 14_900.0,
        }
    }

    /// Run `n` overs and the receive period after the last, a tick at a time.
    fn overs(&mut self, n: usize) -> Vec<Over> {
        self.overs_with(n, |_| {})
    }

    /// [`Self::overs`], calling `on_key` with each over's index on the tick that keyed it.
    fn overs_with(&mut self, n: usize, on_key: impl FnMut(usize)) -> Vec<Over> {
        self.overs_hooked(n, on_key, |_, _| {})
    }

    /// [`Self::overs_with`], calling `before` with the loop's clock and state before every tick.
    fn overs_hooked(
        &mut self,
        n: usize,
        mut on_key: impl FnMut(usize),
        mut before: impl FnMut(f64, &mut RadioLoop),
    ) -> Vec<Over> {
        let sinks = Sinks {
            wsjtx: None,
            psk: None,
        };
        let rebuilt = Arc::new(AtomicBool::new(false));
        let flag = rebuilt.clone();
        let mut ra = mock_reopen_audio();
        let mut rr = move |_t: &Transport, _coexist: bool| {
            flag.store(true, Ordering::SeqCst);
            (Rig::vox(), None, CatProbe::status(None, ""))
        };
        let mut station = StationSinks::new();
        let mut overs: Vec<Over> = Vec::new();
        // Up to the tick before the over after the last one; bounded in case one never keys.
        let mut end = self.slot + 30_000.0 * (n as f64 + 1.0);
        let mut unkeyed_at = 0.0;
        while self.t < end {
            before(self.t, &mut self.state);
            let (before, was_keyed) = (self.radio.sent().len(), self.radio.keyed());
            let dial = self.engine.lock().unwrap().settings().dial_hz();
            self.state
                .step(
                    &self.engine,
                    &mut self.backend,
                    &mut self.rig,
                    &sinks,
                    self.t,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
            assert!(
                !rebuilt.load(Ordering::SeqCst),
                "the loop rebuilt its transport: the radio under test is gone"
            );
            let sent = self.radio.sent();
            let writes: Vec<String> = sent[before..]
                .iter()
                .filter(|l| !l.ends_with('?'))
                .cloned()
                .collect();
            let keyed = self.radio.keyed();
            if !was_keyed && keyed {
                let at = writes
                    .iter()
                    .position(|l| is_key(l))
                    .map_or(writes.len(), |i| i + 1);
                overs.push(Over {
                    dial,
                    key_ms: self.t.rem_euclid(15_000.0),
                    key: writes[..at].to_vec(),
                    ..Over::default()
                });
                on_key(overs.len() - 1);
                if overs.len() == n {
                    end = self.t + 29_900.0;
                }
            } else if let Some(over) = overs.last_mut() {
                if was_keyed && !keyed {
                    let at = writes.iter().position(|l| is_unkey(l)).unwrap_or(0);
                    over.unkey = writes[at..].to_vec();
                    over.unkey_ms = self.t.rem_euclid(15_000.0);
                    unkeyed_at = self.t;
                } else if !keyed {
                    over.rx_ms
                        .extend(writes.iter().map(|_| self.t - unkeyed_at));
                    over.rx.extend(writes);
                    let dial = self.dial();
                    if over.rx_dials.last() != Some(&dial) {
                        over.rx_dials.push(dial);
                    }
                }
            }
            self.t += 20.0;
        }
        assert_eq!(
            overs.len(),
            n,
            "premise: every even slot keyed an over: {overs:#?}"
        );
        overs
    }

    fn dial(&self) -> u64 {
        self.engine.lock().unwrap().settings().dial_hz()
    }
}

fn is_key(l: &str) -> bool {
    matches!(l, "1C 00 01" | "T 1" | "T 3")
}
fn is_unkey(l: &str) -> bool {
    matches!(l, "1C 00 00" | "T 0")
}

// ── Nexus's own CI-V daemon ──────────────────────────────────────────────────────────────────

/// `FakeRadio`'s register file as an Icom that also keys: `1C 00 v` is acked and held, a `1C 00`
/// read answers it, and every frame the controller sends is logged in order.
struct KeyingIcom {
    inner: FakeRadio,
    addr: u8,
    log: Arc<Mutex<Vec<Frame>>>,
    keyed: Arc<AtomicBool>,
    out: VecDeque<u8>,
}

impl Write for KeyingIcom {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut frame = Vec::new();
        for &b in buf {
            frame.push(b);
            if b != 0xFD {
                continue;
            }
            let bytes = std::mem::take(&mut frame);
            let Some(f) = Frame::parse(&bytes) else {
                continue;
            };
            self.log.lock().unwrap().push(f.clone());
            let reply = |cmd: u8, data: &[u8]| {
                Frame {
                    to: CONTROLLER,
                    from: self.addr,
                    cmd,
                    data: data.to_vec(),
                }
                .to_bytes()
            };
            match (f.cmd, f.data.as_slice()) {
                (0x1C, [0x00, v]) => {
                    self.keyed.store(*v != 0, Ordering::SeqCst);
                    let ack = reply(0xFB, &[]);
                    self.out.extend(ack);
                }
                (0x1C, [0x00]) => {
                    let on = u8::from(self.keyed.load(Ordering::SeqCst));
                    let answer = reply(0x1C, &[0x00, on]);
                    self.out.extend(answer);
                }
                _ => {
                    self.inner.write_all(&bytes)?;
                    let mut tmp = [0u8; 512];
                    if let Ok(n) = self.inner.read(&mut tmp) {
                        self.out.extend(&tmp[..n]);
                    }
                }
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for KeyingIcom {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.out.is_empty() {
            std::thread::sleep(Duration::from_millis(2));
            return Err(io::Error::new(io::ErrorKind::TimedOut, "no data"));
        }
        let n = buf.len().min(self.out.len());
        for (i, b) in self.out.drain(..n).enumerate() {
            buf[i] = b;
        }
        Ok(n)
    }
}

/// The CI-V radio as the bench sees it: the frames, the PTT, the register file.
struct Icom {
    log: Arc<Mutex<Vec<Frame>>>,
    keyed: Arc<AtomicBool>,
    regs: Arc<Mutex<Regs>>,
}

impl Radio for Icom {
    fn sent(&self) -> Vec<String> {
        self.log.lock().unwrap().iter().map(civ_text).collect()
    }
    fn keyed(&self) -> bool {
        self.keyed.load(Ordering::SeqCst)
    }
}

/// A CI-V frame as text, a frequency in Hz where it carries one. Everything but a write to the
/// dial, the mode, the split, the VFO selection, an offset or PTT is marked `?` and left out of
/// [`Over`]: the daemon's reads and the loop's level and meter traffic are not this file's subject.
fn civ_text(f: &Frame) -> String {
    let hex = |d: &[u8]| {
        d.iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let text = format!("{:02X} {}", f.cmd, hex(&f.data))
        .trim_end()
        .to_string();
    match (f.cmd, f.data.as_slice()) {
        (0x05, d) if d.len() >= 5 => format!("05 {}", bcd_to_freq(&d[..5])),
        (0x25, [band, rest @ ..]) if rest.len() >= 5 => {
            format!("25 {band:02X} {}", bcd_to_freq(&rest[..5]))
        }
        (0x06 | 0x07, _)
        | (0x0F, [_])
        | (0x1C, [0x00, _])
        | (0x21 | 0x26, [_, _, ..])
        | (0x16, [0x5A, _])
        | (0x1A, [0x06, _, ..]) => text,
        _ => format!("{text} ?"),
    }
}

/// The loop on Nexus's CI-V daemon over a fake `model` at `addr`, Split Operation `split`.
fn civ_bench(addr: u8, model: IcomModel, hamlib: u32, split: SplitMode, tx_hz: f32) -> Bench<Icom> {
    let engine = cq_engine(split, tx_hz, |s| {
        s.rig_model = hamlib;
        s.baud = 115_200;
        s.ptt_method = "cat".into();
    });
    let (inner, _push) = FakeRadio::new(addr);
    let regs = inner.regs();
    {
        let mut r = regs.lock().unwrap();
        r.main_hz = FT8_DIAL;
        r.sub_hz = FT8_DIAL;
        r.unselected_hz = FT8_DIAL;
        r.no_satmode = true; // neither radio has a satellite mode
    }
    let log = Arc::new(Mutex::new(Vec::new()));
    let keyed = Arc::new(AtomicBool::new(false));
    let radio = KeyingIcom {
        inner,
        addr,
        log: log.clone(),
        keyed: keyed.clone(),
        out: VecDeque::new(),
    };
    let daemon = CivDaemon::start_with_io(Box::new(radio), addr, 0, 1, Some(model)).unwrap();
    let rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
    let settings = engine.lock().unwrap().settings().clone();
    let cfg = RadioConfig {
        rig_model: settings.rig_model,
        ..RadioConfig::default()
    };
    let state = RadioLoop::new(
        Transport::from_settings(&settings),
        Some(CatDaemon::Native(daemon)),
        &cfg,
    );
    Bench::new(engine, state, rig, Icom { log, keyed, regs })
}

/// An IC-7610 on Nexus's CI-V daemon, Split Operation = Rig: before the first key `0F 01` (split
/// on, first, as this connection does not hold it yet), `25 01` (the SUB band's dial, by name, on
/// this radio) at the TX dial, `26 01 01 01` (that band's mode: USB, DATA mode D1, the bench's Data
/// mode) and `0F 01` (split on), last. Split is held between overs, as
/// WSJT-X holds it: nothing but the unkey at an unkey; the later overs, their TX dial and mode
/// unchanged and already confirmed, write only split on before the key, as WSJT-X does before every
/// split over. MAIN never written, nothing written while it receives, the engine's dial unmoved. At
/// 1441 Hz the step is -500 Hz; at 941 Hz, -1000 Hz.
///
/// The `26 01` is the loop's `X PKTUSB -1`, which the daemon used to refuse: its mode names
/// (`commands::Mode::from_name`) have no DATA submode, so nothing went on the wire, and the TX VFO
/// transmitted FT8 in whatever mode it was left in.
#[test]
fn an_ic7610_in_rig_split_gets_the_same_tx_dial_every_over_and_split_held_between() {
    for (tx_hz, tx_dial) in [(1441.0, 14_073_500u64), (941.0, 14_073_000)] {
        let mut b = civ_bench(0x98, IcomModel::Ic7610, 3078, SplitMode::Rig, tx_hz);
        let overs = b.overs(3);
        for (i, o) in overs.iter().enumerate() {
            eprintln!("IC-7610 Rig {tx_hz} Hz over {}: {o:?}", i + 1);
        }
        let key = vec!["0F 01".to_string(), "1C 00 01".to_string()];
        let first_key = vec![
            "0F 01".to_string(),
            format!("25 01 {tx_dial}"),
            "26 01 01 01".to_string(),
            "0F 01".to_string(),
            "1C 00 01".to_string(),
        ];
        let unkey = vec!["1C 00 00".to_string()];
        for (i, o) in overs.iter().enumerate() {
            let what = format!("{tx_hz} Hz, over {}", i + 1);
            assert_eq!(
                o.dial, FT8_DIAL,
                "{what}: the dial the shift was taken from"
            );
            let want = if i == 0 { &first_key } else { &key };
            assert_eq!(&o.key, want, "{what}: what goes out before the key");
            assert_eq!(o.unkey, unkey, "{what}: what goes out at the unkey");
            assert_eq!(
                o.rx,
                Vec::<String>::new(),
                "{what}: written while receiving"
            );
        }
        let r = b.radio.regs.lock().unwrap();
        assert_eq!(
            (r.main_hz, r.sub_hz, r.split),
            (FT8_DIAL, tx_dial, true),
            "{tx_hz} Hz: held in split"
        );
        drop(r);
        assert_eq!(b.dial(), FT8_DIAL, "{tx_hz} Hz: the engine's dial");
    }
}

/// An IC-7300 on Nexus's CI-V daemon, Split Operation = Fake It: before every key `05` to the TX
/// dial, and 100 ms after every unkey `05` back to the RX dial. The dial is put back exactly, the
/// engine's dial never moves, and nothing else is written.
#[test]
fn an_ic7300_in_fake_it_moves_the_dial_for_each_over_and_puts_it_back_exactly() {
    let mut b = civ_bench(0x94, IcomModel::Ic7300, 3073, SplitMode::FakeIt, 1441.0);
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("IC-7300 Fake It over {}: {o:?}", i + 1);
        assert_eq!(o.dial, FT8_DIAL, "over {}", i + 1);
        assert_eq!(o.key, ["05 14073500", "1C 00 01"], "over {}", i + 1);
        assert_eq!(o.unkey, ["1C 00 00"], "over {}", i + 1);
        assert_eq!(o.rx, ["05 14074000"], "over {}", i + 1);
        assert_eq!(o.rx_ms, [100.0], "over {}: after the unkey", i + 1);
        assert_eq!(o.rx_dials, [FT8_DIAL], "over {}", i + 1);
    }
    assert_eq!(b.radio.regs.lock().unwrap().main_hz, FT8_DIAL);
    assert_eq!(b.dial(), FT8_DIAL);
}

/// A Fake It write-back the radio does not take is sent again on the next tick, and the dial stays
/// on the channel. The IC-7300 says nothing to the one dial write that should put the dial back
/// after the first over (`drop_dial_writes`). That write used to go out once and never again; 750
/// ms later the dial read found the radio on the TX dial and `Engine::observe_rig_freq` took it for
/// the operator's QSY, so the second over was planned from 14.0735 and went out 500 Hz lower in RF,
/// and the radio was left off the 20 m FT8 channel for good (the band control read "custom").
/// WSJT-X's Fake It reports a read-back equal to the TX dial as the RX dial
/// (`EmulateSplitTransceiver.cpp:56-64`), so the same lost write leaves its dial, and the next
/// over, where they were.
#[test]
fn a_fake_it_write_back_the_radio_does_not_take_is_sent_again_and_the_dial_stays() {
    let mut b = civ_bench(0x94, IcomModel::Ic7300, 3073, SplitMode::FakeIt, 1441.0);
    let regs = b.radio.regs.clone();
    // Armed on the first over's key tick, after its `05`: the next dial write is its write-back.
    let overs = b.overs_with(3, |i| {
        if i == 0 {
            regs.lock().unwrap().drop_dial_writes = 1;
        }
    });
    for (i, o) in overs.iter().enumerate() {
        eprintln!(
            "IC-7300 Fake It, first write-back lost, over {}: {o:?}",
            i + 1
        );
    }
    assert_eq!(
        regs.lock().unwrap().drop_dial_writes,
        0,
        "premise: a dial write was dropped"
    );
    assert_eq!(
        overs[0].key,
        ["05 14073500", "1C 00 01"],
        "premise: the first over"
    );
    assert_eq!(
        overs[0].unkey,
        ["1C 00 00"],
        "nothing else on the unkey's tick"
    );
    assert_eq!(
        overs[0].rx,
        ["05 14074000", "05 14074000"],
        "the write-back the radio did not answer, and again"
    );
    assert_eq!(
        overs[0].rx_ms,
        [100.0, 120.0],
        "…100 ms after the unkey, then on the next tick"
    );
    for (i, o) in overs.iter().enumerate().skip(1) {
        assert_eq!(o.dial, FT8_DIAL, "over {}: the dial", i + 1);
        assert_eq!(
            o.key,
            ["05 14073500", "1C 00 01"],
            "over {}: the same TX dial",
            i + 1
        );
        assert_eq!(o.rx, ["05 14074000"], "over {}", i + 1);
    }
    for (i, o) in overs.iter().enumerate() {
        assert_eq!(
            o.rx_dials,
            [FT8_DIAL],
            "over {}: the engine's dial while receiving",
            i + 1
        );
    }
    assert_eq!(regs.lock().unwrap().main_hz, FT8_DIAL);
    assert_eq!(b.dial(), FT8_DIAL);
}

// ── Hamlib rigctld ───────────────────────────────────────────────────────────────────────────

/// VFO B and the split flag, as the rigctld below holds them (VFO A is the peer's dial).
#[derive(Default)]
struct Vfos {
    split: bool,
    b: u64,
    /// While above zero, a dial write (`F`) is answered `RPRT -9` and not taken.
    refuse_f: u32,
    /// A radio that cannot take a dial write in the moment after an unkey: the first this many
    /// `F` after every `T 0` are answered `RPRT -9` and not taken.
    refuse_f_after_unkey: u32,
    /// A read-back that lags a QSY: after the first `F` it takes after a `T 0`, the next dial
    /// read (`f`) answers the dial it left. This many times.
    lag_after_unkey: u32,
    /// The operator's knob: the first `F` taken after a `T 0` is answered `RPRT 0`, and VFO A is
    /// then turned to this dial.
    knob_after_unkey: Option<u64>,
    /// `F` since the last `T 0`, taken or not; `None` before the first.
    since_unkey: Option<u32>,
    /// What the next `f` answers in place of VFO A.
    stale: Option<u64>,
    /// Dial reads answered with anything but the 20 m FT8 dial.
    reads_off_the_channel: u32,
    /// While set, split off (`S 0`) is answered `RPRT -9` and not taken.
    refuse_split_off: bool,
    /// A radio with no split control: every `S` is answered `RPRT -11` (not available).
    no_split: bool,
    /// While set, split off (`S 0`) is answered `RPRT -11` (not available) and not taken.
    split_off_unavailable: bool,
    /// While set, every `S 0` records whether this engine's lock was free when it came.
    split_off_lock_probe: Option<Arc<Mutex<Engine>>>,
    split_off_lock_free: Vec<bool>,
    /// A TX VFO that exists only while split is on (Nexus's own Flex client, whose split slice
    /// `S 1` makes): the TX dial and mode (`I`, `X`) are answered `RPRT -1` and not taken while
    /// split is off.
    tx_vfo_only_in_split: bool,
    /// A radio that never answers a split write: every `S` is answered `RPRT -5` (Hamlib's "the
    /// rig did not answer") and not taken.
    split_no_answer: bool,
    /// The split read (`s`) answered `RPRT -5`, as a radio that never answers it.
    split_read_no_answer: bool,
    /// `\dump_caps` says the split can be read natively (`SplitDetect::Native`); otherwise it is
    /// answered `RPRT -1` and the split read is not trusted.
    split_read_native: bool,
    /// Split on (`S 1`) answered `RPRT -9` and not taken.
    refuse_split_on: bool,
    /// A K3 whose VFO B is in a mode it will not pair with VFO A's: split on (`S 1`) answered
    /// `RPRT -9` and not taken until a TX mode (`X`) is taken.
    split_on_needs_x: bool,
}

struct Hamlib {
    peer: Peer,
    keyed: Arc<AtomicBool>,
    vfos: Arc<Mutex<Vfos>>,
}

impl Radio for Hamlib {
    fn sent(&self) -> Vec<String> {
        self.peer
            .lines
            .lock()
            .unwrap()
            .iter()
            .map(|l| {
                // The same writes as the CI-V view: dial, mode, split (and its TX VFO's dial and
                // mode), VFO, the two offsets and PTT. rigctld's reads are lower-case.
                if l.starts_with(['F', 'M', 'S', 'I', 'X', 'V', 'J', 'Z', 'T']) {
                    l.clone()
                } else {
                    format!("{l} ?")
                }
            })
            .collect()
    }
    fn keyed(&self) -> bool {
        self.keyed.load(Ordering::SeqCst)
    }
}

/// The loop on a rigctld holding VFO A (the dial), VFO B and split, Split Operation `split`.
/// `own_split` is the radio's split before Nexus transmits: `Some(vfo_b)` = on, TX on VFO B there.
fn hamlib_bench(split: SplitMode, tx_hz: f32, own_split: Option<u64>) -> Bench<Hamlib> {
    let engine = cq_engine(split, tx_hz, |_| {});
    let keyed = Arc::new(AtomicBool::new(false));
    let vfos = Arc::new(Mutex::new(Vfos {
        split: own_split.is_some(),
        b: own_split.unwrap_or(FT8_DIAL),
        ..Vfos::default()
    }));
    let (k, v) = (keyed.clone(), vfos.clone());
    let peer = retuning_peer(FT8_DIAL, "PKTUSB", move |line, radio| {
        let mut v = v.lock().unwrap();
        let ok = || Some("RPRT 0\n".to_string());
        if line.starts_with("S 0") {
            if let Some(engine) = v.split_off_lock_probe.clone() {
                let free = engine.try_lock().is_ok();
                v.split_off_lock_free.push(free);
            }
        }
        match line {
            "T 1" | "T 3" => {
                radio.keyed = true;
                k.store(true, Ordering::SeqCst);
                ok()
            }
            "T 0" => {
                radio.keyed = false;
                k.store(false, Ordering::SeqCst);
                v.since_unkey = Some(0);
                ok()
            }
            "s" if v.split_read_no_answer => Some("RPRT -5\n".to_string()),
            "s" => Some(format!("{}\nVFOB\n", u8::from(v.split))),
            "\\dump_caps" if v.split_read_native => Some(
                "Can get Split VFO: Y\nCan get Split Freq: Y\nTargetable features: FREQ MODE\n"
                    .to_string(),
            ),
            _ if line.starts_with("S ") && v.split_no_answer => Some("RPRT -5\n".to_string()),
            _ if line.starts_with("S 1") && (v.refuse_split_on || v.split_on_needs_x) => {
                Some("RPRT -9\n".to_string())
            }
            "i" => Some(format!("{}\n", v.b)),
            "f" => {
                let hz = v.stale.take().unwrap_or(radio.dial);
                if hz != FT8_DIAL {
                    v.reads_off_the_channel += 1;
                }
                Some(format!("{hz}\n"))
            }
            _ if line.starts_with("S ") && v.no_split => Some("RPRT -11\n".to_string()),
            _ if line.starts_with("S 0") && v.split_off_unavailable => {
                Some("RPRT -11\n".to_string())
            }
            _ if line.starts_with("S 0") && v.refuse_split_off => Some("RPRT -9\n".to_string()),
            _ if line.starts_with("S ") => {
                v.split = line.split_whitespace().nth(1) == Some("1");
                ok()
            }
            _ if (line.starts_with("I ") || line.starts_with("X "))
                && v.tx_vfo_only_in_split
                && !v.split =>
            {
                Some("RPRT -1\n".to_string())
            }
            _ if line.starts_with("I ") => {
                v.b = line[2..].trim().parse().unwrap();
                ok()
            }
            _ if line.starts_with("X ") => {
                v.split_on_needs_x = false;
                ok()
            }
            _ if line.starts_with("F ") && v.refuse_f > 0 => {
                v.refuse_f -= 1;
                Some("RPRT -9\n".to_string())
            }
            _ if line.starts_with("F ") => {
                v.since_unkey = v.since_unkey.map(|n| n + 1);
                match v.since_unkey {
                    Some(n) if n <= v.refuse_f_after_unkey => Some("RPRT -9\n".to_string()),
                    // The first dial write the radio takes after the unkey.
                    Some(n) if n == v.refuse_f_after_unkey + 1 => {
                        if v.lag_after_unkey > 0 {
                            v.lag_after_unkey -= 1;
                            v.stale = Some(radio.dial);
                        }
                        let knob = v.knob_after_unkey.take()?;
                        radio.dial = knob;
                        ok()
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    });
    let rig = Rig::rigctld(&peer.address);
    Bench::new(engine, loop_state(), rig, Hamlib { peer, keyed, vfos })
}

/// Through Hamlib, Split Operation = Rig: `S 1 VFOB` first, then `I` at the TX dial and
/// `X PKTUSB -1`, before the first key only (the later overs' TX dial and mode are unchanged and
/// already confirmed), and `S 1 VFOB` last
/// before every key; nothing but `T 0` at an unkey, split held between overs as WSJT-X holds it.
/// VFO A is never written; VFO B holds the TX dial.
#[test]
fn through_hamlib_rig_split_is_engaged_before_the_first_key_and_held_between_overs() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("Hamlib Rig over {}: {o:?}", i + 1);
        assert_eq!(o.dial, FT8_DIAL, "over {}", i + 1);
        let key: &[&str] = if i == 0 {
            &["S 1 VFOB", "I 14073500", "X PKTUSB -1", "S 1 VFOB", "T 1"]
        } else {
            &["S 1 VFOB", "T 1"]
        };
        assert_eq!(o.key, key, "over {}", i + 1);
        assert_eq!(o.unkey, ["T 0"], "over {}", i + 1);
        assert_eq!(o.rx, Vec::<String>::new(), "over {}", i + 1);
    }
    let v = b.radio.vfos.lock().unwrap();
    assert_eq!((v.split, v.b), (true, 14_073_500));
    drop(v);
    assert_eq!(b.dial(), FT8_DIAL);
}

/// Through Hamlib, Split Operation = Rig, on a radio that was ALREADY in split before Nexus
/// transmitted, VFO B on the dial (a quick split): the receive-time split read takes that as the
/// operator's own split. Nexus holds its split between overs, as WSJT-X does, so VFO B stays on the
/// TX dial across them; the operator's own split, VFO B and all, is put back when Nexus lets go
/// (here, TX turned off).
#[test]
fn through_hamlib_a_radio_already_in_split_gets_its_own_vfo_b_back_when_nexus_lets_go() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, Some(FT8_DIAL));
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("Hamlib Rig, radio's own split on, over {}: {o:?}", i + 1);
        let key: &[&str] = if i == 0 {
            &["S 1 VFOB", "I 14073500", "X PKTUSB -1", "S 1 VFOB", "T 1"]
        } else {
            &["S 1 VFOB", "T 1"]
        };
        assert_eq!(o.key, key, "over {}", i + 1);
        assert_eq!(o.unkey, ["T 0"], "over {}", i + 1);
        assert_eq!(o.rx, Vec::<String>::new(), "over {}", i + 1);
    }
    assert_eq!(
        b.radio.vfos.lock().unwrap().b,
        14_073_500,
        "held on the TX dial"
    );
    b.engine.lock().unwrap().set_tx_enabled(false);
    let writes = b.ticks(3);
    assert_eq!(
        writes,
        ["S 1 VFOB", "I 14074000"],
        "the operator's own split put back"
    );
    let v = b.radio.vfos.lock().unwrap();
    assert_eq!((v.split, v.b), (true, FT8_DIAL));
}

/// Through Hamlib, Split Operation = Fake It: `F` to the TX dial before every key, and `F` back
/// 100 ms after every unkey; the dial is put back exactly.
#[test]
fn through_hamlib_fake_it_moves_the_dial_for_each_over_and_puts_it_back_exactly() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1441.0, None);
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("Hamlib Fake It over {}: {o:?}", i + 1);
        assert_eq!(o.dial, FT8_DIAL, "over {}", i + 1);
        assert_eq!(o.key, ["F 14073500", "T 1"], "over {}", i + 1);
        assert_eq!(o.unkey, ["T 0"], "over {}", i + 1);
        assert_eq!(o.rx, ["F 14074000"], "over {}", i + 1);
        assert_eq!(o.rx_ms, [100.0], "over {}: after the unkey", i + 1);
        assert_eq!(o.rx_dials, [FT8_DIAL], "over {}", i + 1);
    }
    assert_eq!(b.dial(), FT8_DIAL);
}

/// Through Hamlib: a Fake It write-back refused once (`RPRT -9`, an Icom's NG) is sent again on
/// the next tick and taken, and the next over keys from the channel. It used to go out once; the
/// dial read then took the TX dial for the dial, and every later over walked from it.
#[test]
fn through_hamlib_a_refused_fake_it_write_back_is_sent_again_and_the_dial_stays() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1441.0, None);
    let vfos = b.radio.vfos.clone();
    let overs = b.overs_with(3, |i| {
        if i == 0 {
            vfos.lock().unwrap().refuse_f = 1; // the write-back, and only it
        }
    });
    for (i, o) in overs.iter().enumerate() {
        eprintln!(
            "Hamlib Fake It, first write-back refused, over {}: {o:?}",
            i + 1
        );
    }
    assert_eq!(
        vfos.lock().unwrap().refuse_f,
        0,
        "premise: the write-back was refused"
    );
    assert_eq!(overs[0].unkey, ["T 0"]);
    assert_eq!(
        overs[0].rx,
        ["F 14074000", "F 14074000"],
        "refused, and sent again"
    );
    assert_eq!(overs[0].rx_ms, [100.0, 120.0]);
    for (i, o) in overs.iter().enumerate() {
        assert_eq!(o.dial, FT8_DIAL, "over {}: the dial", i + 1);
        assert_eq!(o.key, ["F 14073500", "T 1"], "over {}: the TX dial", i + 1);
        assert_eq!(o.rx_dials, [FT8_DIAL], "over {}", i + 1);
    }
    assert_eq!(b.dial(), FT8_DIAL);
}

/// Through Hamlib, on a radio that refuses every Fake It write-back (a dial write in the moment
/// after the unkey, all three of the dial's tries): each over's write-back goes out three times,
/// 100, 120 and 140 ms after the unkey, then the CAT status says the radio did not go back, and
/// the TX dial is still never taken for the dial, though every dial read finds the radio on it.
/// So every over keys the same TX dial from the channel. This station used to walk one step every
/// over: the TX dial and the dial went 500 Hz lower each cycle at 1441 Hz.
#[test]
fn through_hamlib_a_radio_that_refuses_every_write_back_gets_three_tries_and_never_walks() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1441.0, None);
    b.radio.vfos.lock().unwrap().refuse_f_after_unkey = 3;
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!(
            "Hamlib Fake It, every write-back refused, over {}: {o:?}",
            i + 1
        );
    }
    for (i, o) in overs.iter().enumerate() {
        let what = format!("over {}", i + 1);
        assert_eq!(o.dial, FT8_DIAL, "{what}: the dial");
        assert_eq!(o.key, ["F 14073500", "T 1"], "{what}: the TX dial");
        assert_eq!(o.unkey, ["T 0"], "{what}");
        assert_eq!(
            o.rx, ["F 14074000"; 3],
            "{what}: the write-back's three tries"
        );
        assert_eq!(o.rx_ms, [100.0, 120.0, 140.0], "{what}");
        assert_eq!(o.rx_dials, [FT8_DIAL], "{what}: never taken for a QSY");
    }
    assert!(
        b.radio.vfos.lock().unwrap().reads_off_the_channel > 0,
        "premise: the dial reads found the radio on the TX dial"
    );
    assert_eq!(b.dial(), FT8_DIAL);
    assert_eq!(
        b.engine.lock().unwrap().snapshot().radio.cat_detail,
        "Fake It split: the radio did not go back to 14.0740 MHz after the over (refused by the \
         rig, 3 tries); it may still be on its TX frequency, 14.0735 MHz"
    );
}

/// Through Hamlib, a radio that takes the write-back and then answers one dial read with the TX
/// dial it left (a read-back that lags the QSY): that read is not the operator's QSY. It used to be
/// taken for the dial until the next read 180 ms later put it back.
#[test]
fn through_hamlib_a_read_of_the_tx_dial_after_the_write_back_is_not_taken_for_a_qsy() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1441.0, None);
    b.radio.vfos.lock().unwrap().lag_after_unkey = 1;
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("Hamlib Fake It, one read lags, over {}: {o:?}", i + 1);
    }
    let v = b.radio.vfos.lock().unwrap();
    assert_eq!(
        (v.lag_after_unkey, v.reads_off_the_channel),
        (0, 1),
        "premise: one read answered the TX dial"
    );
    drop(v);
    for (i, o) in overs.iter().enumerate() {
        assert_eq!(o.dial, FT8_DIAL, "over {}: the dial", i + 1);
        assert_eq!(o.key, ["F 14073500", "T 1"], "over {}: the TX dial", i + 1);
        assert_eq!(o.rx, ["F 14074000"], "over {}", i + 1);
        assert_eq!(
            o.rx_dials,
            [FT8_DIAL],
            "over {}: never taken for a QSY",
            i + 1
        );
    }
    assert_eq!(b.dial(), FT8_DIAL);
}

/// ⚠️ POSITIVE CONTROL: the operator's knob is still followed. The radio takes the first over's
/// write-back, then the operator turns VFO A to 14.076 while it receives. That read is not the TX
/// dial, so it is the operator's QSY, and the next overs key from 14.076.
#[test]
fn through_hamlib_a_knob_move_while_receiving_is_still_followed() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1441.0, None);
    b.radio.vfos.lock().unwrap().knob_after_unkey = Some(14_076_000);
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("Hamlib Fake It, knob to 14.076, over {}: {o:?}", i + 1);
    }
    assert_eq!(
        b.radio.vfos.lock().unwrap().knob_after_unkey,
        None,
        "premise: the knob moved"
    );
    assert_eq!(overs[0].rx, ["F 14074000"]);
    assert_eq!(
        overs[0].rx_dials,
        [FT8_DIAL, 14_076_000],
        "the knob's dial is the dial"
    );
    for (i, o) in overs.iter().enumerate().skip(1) {
        assert_eq!(o.dial, 14_076_000, "over {}: the dial", i + 1);
        assert_eq!(o.key, ["F 14075500", "T 1"], "over {}: the TX dial", i + 1);
        assert_eq!(o.rx, ["F 14076000"], "over {}", i + 1);
        assert_eq!(o.rx_dials, [14_076_000], "over {}", i + 1);
    }
    assert_eq!(b.dial(), 14_076_000);
}

/// Stop TX during a Fake It over, then Call CQ again at once with the TX offset moved inside
/// 1500–2000 Hz, so the engine plans no shift and the new over keys in the same period (the snappy
/// first over): the radio goes back on the RX dial before that over keys, so it goes out from the
/// dial and not from the stopped over's TX dial. The write-back used to go out on the unkey's own
/// tick, ahead of any key; it now waits 100 ms after the unkey, and a key inside that wait sends it
/// first.
#[test]
fn through_hamlib_an_over_keyed_inside_the_write_back_wait_keys_from_the_rx_dial() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1441.0, None);
    let engine = b.engine.clone();
    // On the first over's key tick: Stop TX, the offset into the window, Call CQ.
    let overs = b.overs_with(1, |_| {
        let mut e = engine.lock().unwrap();
        e.halt_tx();
        e.set_tx_offset(1700.0);
        e.start_cq(None).expect("Call CQ again");
    });
    eprintln!("Hamlib Fake It, stopped and keyed again: {overs:?}");
    let writes: Vec<String> = b
        .radio
        .sent()
        .into_iter()
        .filter(|l| !l.ends_with('?'))
        .collect();
    let from = writes
        .iter()
        .position(|l| l == "F 14073500")
        .expect("the first over's TX dial");
    assert_eq!(
        writes[from..(from + 5).min(writes.len())],
        ["F 14073500", "T 1", "T 0", "F 14074000", "T 1"],
        "the stopped over, the write-back, then the new over from the RX dial: {writes:?}"
    );
    assert_eq!(b.dial(), FT8_DIAL);
}

/// A snappy first over started 3 s into its period (Stop TX before the period, Call CQ 3 s into
/// it) moves the dial like any other, and its write-back goes out 100 ms after its unkey. The loop
/// trims its head and its trailing silence, so it ends well before the period's boundary.
#[test]
fn through_hamlib_a_late_snappy_over_is_written_back_100_ms_after_its_unkey() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1441.0, None);
    let (engine, slot) = (b.engine.clone(), b.slot);
    // Stop TX before the even period, so its boundary keys nothing; Call CQ 3 s into it.
    let overs = b.overs_hooked(
        2,
        |_| {},
        |t, _| {
            if t == slot - 100.0 {
                engine.lock().unwrap().halt_tx();
            } else if t == slot + 3_000.0 {
                engine.lock().unwrap().start_cq(None).expect("Call CQ");
            }
        },
    );
    for (i, o) in overs.iter().enumerate() {
        eprintln!("Hamlib Fake It, late first over, over {}: {o:?}", i + 1);
    }
    assert_eq!(
        overs[0].key_ms, 3_000.0,
        "premise: the first over keyed 3 s into its period"
    );
    assert_eq!(
        overs[0].key,
        ["M PKTUSB -1", "F 14073500", "T 1"],
        "premise: the late over, after the mode Call CQ asked for"
    );
    eprintln!("late over unkeyed {} ms into its period", overs[0].unkey_ms);
    assert_eq!(overs[0].unkey, ["T 0"]);
    assert_eq!(overs[0].rx, ["F 14074000"]);
    assert_eq!(
        overs[0].rx_ms,
        [100.0],
        "the write-back, 100 ms after the unkey"
    );
    assert_eq!(overs[1].key, ["F 14073500", "T 1"]);
    assert_eq!(b.dial(), FT8_DIAL);
}

/// The operator retunes during a Fake It over (a band-plan pick of 14.076): the write-back after
/// it would put the old RX dial back over that, so it is dropped, and the new dial alone goes out
/// once the over ends.
#[test]
fn through_hamlib_a_retune_during_the_over_is_not_undone_by_the_write_back() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1441.0, None);
    let engine = b.engine.clone();
    let overs = b.overs_with(1, |_| {
        engine.lock().unwrap().set_frequency(14.076, "20m", "USB");
    });
    eprintln!("Hamlib Fake It, retuned during the over: {overs:?}");
    let dial_writes: Vec<&str> = overs[0]
        .rx
        .iter()
        .filter(|l| l.starts_with("F "))
        .map(String::as_str)
        .collect();
    assert_eq!(overs[0].unkey, ["T 0"]);
    assert_eq!(
        dial_writes,
        ["F 14076000"],
        "the new dial, and not the old one written back"
    );
    assert_eq!(b.dial(), 14_076_000);
}

/// The same Stop TX and Call CQ, on a radio that refuses the first dial write after every unkey:
/// the write-back sent ahead of the new over is refused, the new over keys anyway, and the next try
/// waits for that over's unkey and then 100 ms, like the first. Three tries in all.
#[test]
fn through_hamlib_a_write_back_refused_ahead_of_a_key_waits_for_the_next_unkey() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1441.0, None);
    b.radio.vfos.lock().unwrap().refuse_f_after_unkey = 1;
    let engine = b.engine.clone();
    let overs = b.overs_with(1, |_| {
        let mut e = engine.lock().unwrap();
        e.halt_tx();
        e.set_tx_offset(1700.0);
        e.start_cq(None).expect("Call CQ again");
    });
    eprintln!("Hamlib Fake It, refused ahead of the new over: {overs:?}");
    let writes: Vec<String> = b
        .radio
        .sent()
        .into_iter()
        .filter(|l| !l.ends_with('?'))
        .collect();
    let from = writes
        .iter()
        .position(|l| l == "F 14073500")
        .expect("the first over's TX dial");
    let tries = writes[from..]
        .iter()
        .filter(|l| l.as_str() == "F 14074000")
        .count();
    assert_eq!(tries, 3, "the write-back's three tries: {writes:?}");
    assert_eq!(
        overs[0].unkey,
        ["T 0"],
        "the new over's unkey, alone on its tick"
    );
    // The mode Call CQ asked for, held while the over was on the air; then the write-back,
    // refused and taken.
    assert_eq!(overs[0].rx, ["M PKTUSB -1", "F 14074000", "F 14074000"]);
    assert_eq!(overs[0].rx_ms, [20.0, 100.0, 120.0]);
    assert_eq!(b.dial(), FT8_DIAL);
}

/// A write-back still waiting when this station's period begins goes out ahead of that period's
/// key, so an over the engine does not shift (here the TX offset is 1700 Hz) goes out from the RX
/// dial. The wait is set up as a carrier let go 40 ms before the period leaves it: the write-back
/// due 100 ms later, the loop's own teardown starting the clock.
#[test]
fn through_hamlib_a_write_back_waiting_at_our_period_goes_ahead_of_its_key() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1700.0, None);
    let slot = b.slot;
    let overs = b.overs_hooked(
        1,
        |_| {},
        |t, st| {
            if t == slot - 40.0 {
                st.fake_it_restore = Some(FT8_DIAL);
            }
        },
    );
    eprintln!("Hamlib Fake It, write-back waiting at our period: {overs:?}");
    assert_eq!(overs[0].key_ms, 0.0, "premise: the over our period keys");
    assert_eq!(
        overs[0].key,
        ["F 14074000", "T 1"],
        "the write-back, then the unshifted over's key"
    );
    assert_eq!(b.dial(), FT8_DIAL);
}

/// The same wait set up 40 ms before the other station's period: no over keys at that boundary,
/// so the write-back keeps its 100 ms and goes out 15.06 s into this station's period.
#[test]
fn through_hamlib_a_write_back_waiting_at_their_period_keeps_its_100_ms() {
    let mut b = hamlib_bench(SplitMode::FakeIt, 1700.0, None);
    let slot = b.slot;
    let overs = b.overs_hooked(
        1,
        |_| {},
        |t, st| {
            if t == slot + 14_960.0 {
                st.fake_it_restore = Some(FT8_DIAL);
            }
        },
    );
    eprintln!("Hamlib Fake It, write-back waiting at their period: {overs:?}");
    assert_eq!(overs[0].key, ["T 1"], "premise: an unshifted over");
    assert_eq!(overs[0].rx, ["F 14074000"]);
    assert_eq!(
        overs[0].unkey_ms + overs[0].rx_ms[0],
        15_060.0,
        "the write-back, 100 ms after the wait began"
    );
    assert_eq!(b.dial(), FT8_DIAL);
}

// ── Rig split held between overs, as WSJT-X holds it ─────────────────────────────────────────
// WSJT-X turns split on once (`Transceiver/HamlibTransceiver.cpp:965-987`, the first
// `rig_set_split_vfo`) and writes only the TX frequency at every key after it
// (`TransceiverBase.cpp:160-171`, "or about to tx split"); `setXIT` keeps a TX frequency set while
// Split Operation is on (`widgets/mainwindow.cpp:12378-12387`), so nothing turns split off between
// overs. Nexus holds it the same way and lets go only when Split Operation is changed, the band or
// the radio changes, TX is turned off, or Nexus stops.

impl<R: Radio> Bench<R> {
    /// Step `n` ticks with no over recorded, returning every write the radio got.
    fn ticks(&mut self, n: usize) -> Vec<String> {
        let sinks = Sinks {
            wsjtx: None,
            psk: None,
        };
        let mut ra = mock_reopen_audio();
        let mut rr =
            move |_t: &Transport, _coexist: bool| (Rig::vox(), None, CatProbe::status(None, ""));
        let mut station = StationSinks::new();
        let before = self.radio.sent().len();
        for _ in 0..n {
            self.state
                .step(
                    &self.engine,
                    &mut self.backend,
                    &mut self.rig,
                    &sinks,
                    self.t,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
            self.t += 20.0;
        }
        self.radio.sent()[before..]
            .iter()
            .filter(|l| !l.ends_with('?'))
            .cloned()
            .collect()
    }
}

/// Through Hamlib, Split Operation = Rig, three overs of a CQ run: `S 1 VFOB` last before every
/// over, as WSJT-X writes it, and never split off between them; the first over turns split on
/// first and writes the TX dial and its mode before it, and the later ones, nothing changed, write neither; nothing is written at
/// an unkey or while receiving, and the radio is still in split, VFO B on the TX dial.
#[test]
fn through_hamlib_rig_split_is_put_on_before_every_over_and_held_across_overs() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("Hamlib Rig held, over {}: {o:?}", i + 1);
    }
    let writes = |p: &str| -> usize {
        overs
            .iter()
            .map(|o| o.key.iter().chain(&o.unkey).chain(&o.rx))
            .map(|w| w.filter(|l| l.starts_with(p)).count())
            .sum()
    };
    assert_eq!(
        writes("S 1"),
        4,
        "split on before each of three overs, and first on the first"
    );
    assert_eq!(writes("S 0"), 0, "split off between overs");
    assert_eq!(
        overs[0].key,
        ["S 1 VFOB", "I 14073500", "X PKTUSB -1", "S 1 VFOB", "T 1"]
    );
    for (i, o) in overs.iter().enumerate() {
        if i > 0 {
            assert_eq!(o.key, ["S 1 VFOB", "T 1"], "over {}", i + 1);
        }
        assert_eq!(o.unkey, ["T 0"], "over {}", i + 1);
        assert_eq!(o.rx, Vec::<String>::new(), "over {}", i + 1);
    }
    let v = b.radio.vfos.lock().unwrap();
    assert_eq!((v.split, v.b), (true, 14_073_500), "held in split");
}

/// What lets go of a held Rig split, each on its own: the band changing, TX turned off, Split
/// Operation changed. Each sends `S 0 VFOA` (no split of the radio's own to put back) while the
/// station receives, before anything keys. The control: none of them, and nothing is written.
#[test]
fn through_hamlib_a_held_rig_split_is_let_go_when_the_band_tx_or_split_mode_changes() {
    type Change = fn(&mut Engine);
    let cases: [(&str, Option<Change>); 4] = [
        (
            // TX turned on again after the band change's own halt: the band alone lets go.
            "the band changes",
            Some(|e| {
                e.set_frequency(7.074, "40m", "USB");
                e.set_tx_enabled(true);
            }),
        ),
        ("TX is turned off", Some(|e| e.set_tx_enabled(false))),
        (
            "Split Operation is set to None",
            Some(|e| {
                let mut s = e.settings().clone();
                s.split_mode = SplitMode::None;
                e.apply_settings(s);
            }),
        ),
        ("control: nothing changes", None),
    ];
    for (what, change) in cases {
        let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
        let _ = b.overs(1);
        assert!(b.radio.vfos.lock().unwrap().split, "{what}: premise, held");
        // The last ticks of the receive period after the over, before the next own boundary.
        if let Some(change) = change {
            change(&mut b.engine.lock().unwrap());
        }
        let writes = b.ticks(3);
        let sent: Vec<&str> = writes
            .iter()
            .map(String::as_str)
            .filter(|l| l.starts_with(['S', 'T']))
            .collect();
        match change {
            Some(_) => {
                assert_eq!(sent, ["S 0 VFOA"], "{what}: wire {writes:?}");
                assert!(
                    !b.radio.vfos.lock().unwrap().split,
                    "{what}: still in split"
                );
            }
            None => {
                assert_eq!(sent, Vec::<&str>::new(), "{what}: wire {writes:?}");
                assert!(b.radio.vfos.lock().unwrap().split, "{what}: let go");
            }
        }
    }
}

/// A held Rig split and an over whose TX offset needs no shift (1500-1999 Hz): the TX dial is the
/// RX dial, written before the key as WSJT-X writes `XIT` 0's TX frequency, never left on the last
/// shifted dial VFO B still holds. The mode, unchanged and already confirmed, is not written.
#[test]
fn through_hamlib_a_held_split_writes_the_rx_dial_for_an_over_with_no_shift() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let engine = b.engine.clone();
    // Moved while the second over is on the air, for the third.
    let overs = b.overs_with(3, |i| {
        if i == 1 {
            engine.lock().unwrap().set_tx_offset(1700.0);
        }
    });
    assert_eq!(overs[1].key, ["S 1 VFOB", "T 1"], "premise: held");
    assert_eq!(
        overs[2].key,
        ["I 14074000", "S 1 VFOB", "T 1"],
        "the TX dial back on the RX dial before the key"
    );
}

/// The radio changing lets go of a held Rig split through the outgoing link before it goes: a
/// saved CAT change rebuilds the link, and the old radio gets the unkey and then `S 0 VFOA`.
#[test]
fn through_hamlib_a_held_rig_split_is_let_go_when_the_radio_changes() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    assert!(b.radio.vfos.lock().unwrap().split, "premise: held");
    {
        let mut e = b.engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = 1035;
        e.apply_settings(s);
    }
    let writes = b.ticks(1);
    assert_eq!(
        writes,
        ["T 0", "S 0 VFOA"],
        "the old radio, before the link goes"
    );
    assert!(!b.radio.vfos.lock().unwrap().split, "left in split");
    assert!(!b.rig.tx_split_held, "the new link believes split is on");
}

/// A split release the radio does not take is still owed. TX turned off after a held Rig split,
/// the radio refusing `S 0` (`RPRT -9`): nothing keys while it is owed, even with TX turned on
/// again at the next own boundary (the over is refused with the split reason and TX halts); the
/// receive-time split read, which now finds the radio in split, never takes that for the
/// operator's own split; and once the radio takes `S 0` again, a retry at a quiet point lets go.
#[test]
fn through_hamlib_a_refused_split_release_is_retried_at_quiet_points_and_nothing_keys_meanwhile() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    assert!(b.radio.vfos.lock().unwrap().split, "premise: held");
    b.radio.vfos.lock().unwrap().refuse_split_off = true;
    b.engine.lock().unwrap().set_tx_enabled(false);
    let first = b.ticks(3);
    assert_eq!(first, ["S 0 VFOA"], "premise: the release, refused");
    assert!(
        b.rig.split_release_pending,
        "the refused release is not owed"
    );
    // TX on again just before the next own boundary: the over is refused, never keyed.
    b.engine.lock().unwrap().set_tx_enabled(true);
    let across = b.ticks(20);
    assert!(
        !across.iter().any(|l| l == "T 1"),
        "keyed while the release is owed: {across:?}"
    );
    {
        let e = b.engine.lock().unwrap();
        assert!(!e.tx_enabled(), "TX still on after the refused over");
        let refused = e
            .snapshot()
            .radio
            .slot_key_refused
            .expect("nothing says why the over was not sent");
        assert!(refused.split_failed, "not told as the split: {refused:?}");
    }
    // Several seconds of receiving: heavy polls and their split reads run, the release is sent
    // again (refused) at most every 2 s, and the radio's split is never taken for the operator's.
    let quiet = b.ticks(300);
    let retries = quiet.iter().filter(|l| *l == "S 0 VFOA").count();
    assert!((1..=4).contains(&retries), "retries in 6 s: {quiet:?}");
    assert!(!quiet.iter().any(|l| l == "T 1"), "keyed: {quiet:?}");
    assert!(
        !matches!(b.state.rig_split_restore, Some((true, _, _))),
        "Nexus's own split taken for the operator's: {:?}",
        b.state.rig_split_restore
    );
    // The radio takes it now: the next retry at a quiet point lets go.
    b.radio.vfos.lock().unwrap().refuse_split_off = false;
    let _ = b.ticks(150);
    assert!(!b.radio.vfos.lock().unwrap().split, "still in split");
    assert!(!b.rig.split_release_pending, "still owed");
}

impl<R: Radio> Bench<R> {
    /// [`Self::ticks`], a rebuilt link reopening through `addr` (the same radio, a new connection).
    fn ticks_reopening(&mut self, n: usize, addr: &str) -> Vec<String> {
        let sinks = Sinks {
            wsjtx: None,
            psk: None,
        };
        let mut ra = mock_reopen_audio();
        let addr = addr.to_string();
        let mut rr = move |_t: &Transport, _coexist: bool| {
            (Rig::rigctld(&addr), None, CatProbe::status(None, ""))
        };
        let mut station = StationSinks::new();
        let before = self.radio.sent().len();
        for _ in 0..n {
            self.state
                .step(
                    &self.engine,
                    &mut self.backend,
                    &mut self.rig,
                    &sinks,
                    self.t,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
            self.t += 20.0;
        }
        self.radio.sent()[before..]
            .iter()
            .filter(|l| !l.ends_with('?'))
            .cloned()
            .collect()
    }
}

/// A radio with no split control at all (every `S` answered `RPRT -11`, as a rig without split, an
/// OmniRig shim or Nexus's own Flex client answers it): the first Rig-split over is refused with the
/// split reason, and since split was never taken no release is owed, so Split Operation set to Fake
/// It then keys normally, with no restart. WSJT-X does not throw on a split it cannot leave on such a
/// rig (`Transceiver/HamlibTransceiver.cpp:1022-1033`) so fake split and no split keep working.
#[test]
fn through_hamlib_a_radio_with_no_split_refuses_rig_split_once_and_fake_it_then_keys() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    b.radio.vfos.lock().unwrap().no_split = true;
    // Across the first own boundary.
    let across = b.ticks(800);
    assert!(!across.iter().any(|l| l == "T 1"), "keyed: {across:?}");
    assert!(
        across.iter().any(|l| l == "S 1 VFOB"),
        "premise: {across:?}"
    );
    {
        let e = b.engine.lock().unwrap();
        assert!(!e.tx_enabled(), "TX still on after the refused over");
        let refused = e.snapshot().radio.slot_key_refused.expect("no reason");
        assert!(refused.split_failed, "{refused:?}");
    }
    assert!(
        !b.rig.split_release_pending,
        "a release owed for a split never taken"
    );
    {
        let mut e = b.engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.split_mode = SplitMode::FakeIt;
        e.apply_settings(s);
        e.set_tx_enabled(true);
        e.start_cq(None).expect("Call CQ");
        let _ = e.take_immediate_tx();
    }
    let overs = b.overs(1);
    assert_eq!(
        overs[0].key,
        ["F 14073500", "T 1"],
        "Fake It after the refusal"
    );
}

/// A split release the radio answers `RPRT -11` (not available) is done, as WSJT-X takes it
/// (`Transceiver/HamlibTransceiver.cpp:1022-1033`): nothing is owed and the next over keys.
#[test]
fn through_hamlib_a_release_answered_not_available_counts_as_done() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    b.radio.vfos.lock().unwrap().split_off_unavailable = true;
    b.engine.lock().unwrap().set_tx_enabled(false);
    assert_eq!(b.ticks(3), ["S 0 VFOA"], "premise: the release");
    assert!(!b.rig.split_release_pending, "owed after RPRT -11");
}

/// Split ON is put back before every Rig-split over, last before the key, as WSJT-X writes it
/// (`Transceiver/HamlibTransceiver.cpp:1010-1015`): a radio that left split between overs (the
/// front-panel SPLIT key, a Kenwood leaving split on a VFO switch, a closed split slice) is in split
/// again before the next over keys, so it goes out of VFO B and not 500 Hz off. Split is never
/// turned off between overs.
#[test]
fn through_hamlib_a_split_the_radio_left_between_overs_is_put_back_before_the_next_key() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let vfos = b.radio.vfos.clone();
    let overs = b.overs_with(2, |i| {
        if i == 0 {
            vfos.lock().unwrap().split = false;
        }
    });
    assert_eq!(
        overs[0].key,
        ["S 1 VFOB", "I 14073500", "X PKTUSB -1", "S 1 VFOB", "T 1"],
        "the first over: split on last"
    );
    assert_eq!(overs[1].key, ["S 1 VFOB", "T 1"], "split put back, last");
    assert!(
        !overs
            .iter()
            .any(|o| o.unkey.iter().chain(&o.rx).any(|l| l.starts_with("S 0"))),
        "split turned off between overs: {overs:?}"
    );
    let v = b.radio.vfos.lock().unwrap();
    assert_eq!((v.split, v.b), (true, 14_073_500));
}

/// A radio whose TX VFO exists only while split is on (Nexus's own Flex client makes its split
/// slice at `S 1` and refuses `I` or `X` before it). Split goes on FIRST whenever this connection
/// does not hold it, ahead of the TX dial and mode, as WSJT-X turns it on ahead of the TX
/// frequency (`Transceiver/HamlibTransceiver.cpp:960-987` in 3.0.2), and on again last before the
/// key; the later overs write split on alone. Every over keys. After a release the next over turns
/// split on first again.
#[test]
fn through_hamlib_a_tx_vfo_that_needs_split_on_gets_split_on_first_and_every_over_keys() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    b.radio.vfos.lock().unwrap().tx_vfo_only_in_split = true;
    let first = ["S 1 VFOB", "I 14073500", "X PKTUSB -1", "S 1 VFOB", "T 1"];
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("Hamlib Rig, TX VFO only in split, over {}: {o:?}", i + 1);
        let key: &[&str] = if i == 0 { &first } else { &["S 1 VFOB", "T 1"] };
        assert_eq!(o.key, key, "over {}", i + 1);
        assert_eq!(o.unkey, ["T 0"], "over {}", i + 1);
        assert_eq!(o.rx, Vec::<String>::new(), "over {}", i + 1);
    }
    assert_eq!(
        {
            let v = b.radio.vfos.lock().unwrap();
            (v.split, v.b)
        },
        (true, 14_073_500)
    );
    b.engine.lock().unwrap().set_tx_enabled(false);
    assert_eq!(b.ticks(3), ["S 0 VFOA"], "premise: the release");
    {
        let mut e = b.engine.lock().unwrap();
        e.set_tx_enabled(true);
        e.start_cq(None).expect("Call CQ");
        let _ = e.take_immediate_tx();
    }
    b.slot = (b.t / 30_000.0).ceil() * 30_000.0;
    let overs = b.overs(1);
    assert_eq!(overs[0].key, first, "split taken again: on first");
}

/// Only a split the radio took through this connection is owed a release. Nexus's split engaged
/// on the loop but `S 1` never taken here, and the release refused outright (`RPRT -9`, as a K3
/// refusing a cross-mode split answers): nothing is owed, so later overs are not refused until a
/// restart. The control: the same refusal with the split taken is owed.
#[test]
fn through_hamlib_a_refused_release_of_a_split_never_taken_owes_nothing() {
    for (what, taken) in [("never taken", false), ("taken", true)] {
        let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
        b.radio.vfos.lock().unwrap().refuse_split_off = true;
        let _ = b.ticks(3); // the receive-time dial and mode
        b.state.audio_rig_split = true;
        b.rig.tx_split_held = taken;
        b.engine.lock().unwrap().set_tx_enabled(false);
        assert_eq!(b.ticks(3), ["S 0 VFOA"], "{what}: premise: the release");
        assert_eq!(b.rig.split_release_pending, taken, "{what}: owed");
    }
}

/// An owed release is sent again outside the engine's lock: a radio that stops answering must not
/// hold the engine for a CAT deadline every few seconds.
#[test]
fn through_hamlib_an_owed_release_is_retried_without_the_engine_lock() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    {
        let mut v = b.radio.vfos.lock().unwrap();
        v.refuse_split_off = true;
        v.split_off_lock_probe = Some(b.engine.clone());
    }
    b.engine.lock().unwrap().set_tx_enabled(false);
    let _ = b.ticks(300);
    let v = b.radio.vfos.lock().unwrap();
    let retries = &v.split_off_lock_free[1..];
    assert!(
        !retries.is_empty(),
        "premise: retried: {:?}",
        v.split_off_lock_free
    );
    assert!(
        retries.iter().all(|free| *free),
        "a retry held the engine's lock: {:?}",
        v.split_off_lock_free
    );
}

/// A release owed on a link that is rebuilt to the same radio (a saved baud change) is still owed
/// on the new link: the radio may still be in Nexus's split. The control: a different radio model
/// starts with nothing owed.
#[test]
fn through_hamlib_an_owed_release_survives_a_rebuild_to_the_same_radio() {
    for (what, same) in [("the same radio", true), ("another radio", false)] {
        let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
        let _ = b.overs(1);
        b.radio.vfos.lock().unwrap().refuse_split_off = true;
        {
            let mut e = b.engine.lock().unwrap();
            let mut s = e.settings().clone();
            if same {
                s.baud = 19_200;
            } else {
                s.rig_model = 1035;
            }
            e.apply_settings(s);
        }
        let address = b.radio.peer.address.clone();
        let writes = b.ticks_reopening(1, &address);
        assert!(
            writes.iter().any(|l| l == "S 0 VFOA"),
            "{what}: premise, the release refused: {writes:?}"
        );
        assert!(b.rig.has_control(), "{what}: premise, a new link");
        assert_eq!(
            b.rig.split_release_pending, same,
            "{what}: owed on the new link"
        );
    }
}

/// Leaving the digital screens lets go of a held Rig split (a phone or CW transmission must never go
/// out of VFO B on the last FT TX dial), with TX turned on again so the screen alone decides; and
/// Nexus stopping lets go of it after the unkey.
#[test]
fn through_hamlib_a_held_rig_split_is_let_go_off_the_digital_screens_and_when_nexus_stops() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    {
        let mut e = b.engine.lock().unwrap();
        e.set_operating_mode("phone", false);
        e.set_tx_enabled(true);
    }
    let sent: Vec<String> = b
        .ticks(3)
        .into_iter()
        .filter(|l| l.starts_with('S'))
        .collect();
    assert_eq!(sent, ["S 0 VFOA"], "off the digital screens");
    assert!(!b.radio.vfos.lock().unwrap().split, "still in split");

    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    let before = b.radio.sent().len();
    b.state
        .stop_for_shutdown(&b.engine, &mut b.backend, &mut b.rig);
    let writes: Vec<String> = b.radio.sent()[before..]
        .iter()
        .filter(|l| !l.ends_with('?'))
        .cloned()
        .collect();
    assert_eq!(writes, ["T 0", "S 0 VFOA"], "Nexus stopping");
    assert!(!b.radio.vfos.lock().unwrap().split, "left in split");
}

// ── An owed split release the radio never confirms ───────────────────────────────────────────
// WSJT-X keeps no owed release: a split write that fails throws, and the rig is closed and opened
// again. Nexus keeps one so that no over goes out of VFO B while the radio may still be in its
// split, and it must not keep it for ever: a definite "off" from the radio's own split read ends
// it, and so does the operator disconnecting the radio.

impl Bench<Hamlib> {
    /// The first Rig-split over, refused because the radio did not answer split on, which may
    /// have been taken late: TX halted and the release owed. Returns the writes.
    fn refused_unanswered_split_over(&mut self) -> Vec<String> {
        self.radio.vfos.lock().unwrap().split_no_answer = true;
        let across = self.ticks(800);
        assert!(!across.iter().any(|l| l == "T 1"), "keyed: {across:?}");
        assert!(
            across.iter().any(|l| l == "S 1 VFOB") && across.iter().any(|l| l == "S 0 VFOA"),
            "premise, split on and its release: {across:?}"
        );
        assert!(!self.engine.lock().unwrap().tx_enabled(), "TX still on");
        assert!(self.rig.split_release_pending, "premise: the release owed");
        across
    }

    /// Split Operation set to Fake It and TX turned on again with a CQ.
    fn fake_it_cq(&mut self) {
        let mut e = self.engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.split_mode = SplitMode::FakeIt;
        e.apply_settings(s);
        e.set_tx_enabled(true);
        e.start_cq(None).expect("Call CQ");
        let _ = e.take_immediate_tx();
    }

    /// The refusal on screen, which must be the owed release's and say so.
    fn assert_refused_split_off_unconfirmed(&self, what: &str) {
        let refused = self
            .engine
            .lock()
            .unwrap()
            .snapshot()
            .radio
            .slot_key_refused
            .unwrap_or_else(|| panic!("{what}: nothing says why"));
        assert!(
            refused.split_failed && refused.split_off_unconfirmed,
            "{what}: not told the radio's SPLIT could not be confirmed off: {refused:?}"
        );
    }
}

/// A radio that never answers a split write (`RPRT -5` to every `S`) and reads its split natively.
/// The first Rig-split over is refused and its release owed; the retries are not taken, and a read
/// of the radio's split that says definitely off ends it, so Fake It then keys. The controls: the
/// read says split is ON (the radio took the `S 1` late), or the radio's split cannot be read
/// natively (a read Hamlib emulates or answers from its own cache): still owed, and the next over,
/// Fake It's included, is refused with the reason that says how to clear it.
#[test]
fn through_hamlib_a_split_never_answered_is_released_by_a_native_read_of_split_off() {
    for (what, native, in_split, freed) in [
        ("read says off", true, false, true),
        ("read says on", true, true, false),
        ("no native read", false, false, false),
    ] {
        let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
        b.radio.vfos.lock().unwrap().split_read_native = native;
        let _ = b.refused_unanswered_split_over();
        b.radio.vfos.lock().unwrap().split = in_split;
        let quiet = b.ticks(300);
        let retries = quiet.iter().filter(|l| *l == "S 0 VFOA").count();
        assert!(retries >= 1, "{what}: premise, retried: {quiet:?}");
        assert_eq!(b.rig.split_release_pending, !freed, "{what}: owed");
        b.fake_it_cq();
        if freed {
            let overs = b.overs(1);
            assert_eq!(overs[0].key, ["F 14073500", "T 1"], "{what}: Fake It");
        } else {
            let across = b.ticks(1300);
            assert!(!across.iter().any(|l| l == "T 1"), "{what}: keyed");
            b.assert_refused_split_off_unconfirmed(what);
        }
    }
}

/// The same radio, its split read never answered either: nothing can confirm the release, so it
/// stays owed through retries and through a rebuild to the same radio (a saved baud change). The
/// operator disconnecting the radio (Rig Model None, saved)
/// ends it, and with the radio chosen again Fake It keys.
#[test]
fn through_hamlib_an_unconfirmable_release_ends_when_the_operator_disconnects_the_radio() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let address = b.radio.peer.address.clone();
    let save = |b: &mut Bench<Hamlib>, edit: &dyn Fn(&mut Settings)| {
        let mut e = b.engine.lock().unwrap();
        let mut s = e.settings().clone();
        edit(&mut s);
        e.apply_settings(s);
    };
    // The radio chosen in Settings (an IC-7300), so None is a change.
    const MODEL: u32 = 3073;
    save(&mut b, &|s| s.rig_model = MODEL);
    let _ = b.ticks_reopening(1, &address);
    assert!(b.rig.has_control(), "premise: connected");
    {
        // The rebuild halted TX for the context change: the CQ again.
        let mut e = b.engine.lock().unwrap();
        e.set_tx_enabled(true);
        e.start_cq(None).expect("Call CQ");
        let _ = e.take_immediate_tx();
    }
    b.radio.vfos.lock().unwrap().split_read_no_answer = true;
    let _ = b.refused_unanswered_split_over();
    let _ = b.ticks(300);
    assert!(b.rig.split_release_pending, "a retry ended it");
    // The control: a rebuild to the same radio carries it.
    save(&mut b, &|s| s.baud = 19_200);
    let _ = b.ticks_reopening(1, &address);
    assert!(b.rig.has_control(), "premise: a new link");
    assert!(b.rig.split_release_pending, "a same-radio rebuild ended it");
    // The operator disconnects the radio, then chooses it again.
    save(&mut b, &|s| s.rig_model = 0);
    let _ = b.ticks(1);
    assert!(!b.rig.has_control(), "premise: disconnected");
    assert!(
        !b.rig.split_release_pending,
        "still owed after a disconnect"
    );
    save(&mut b, &|s| s.rig_model = MODEL);
    let _ = b.ticks_reopening(1, &address);
    assert!(b.rig.has_control(), "premise: connected again");
    assert!(
        !b.rig.split_release_pending,
        "owed on the radio chosen again"
    );
    b.fake_it_cq();
    let overs = b.overs(1);
    assert_eq!(overs[0].key, ["F 14073500", "T 1"], "Fake It");
}

/// A K3 refusing split while VFO B is in a mode it will not pair with VFO A's (WSJT-X 3.0.2 notes it
/// for its first split call, `Transceiver/HamlibTransceiver.cpp:963-970`): the first `S 1` refused
/// does not stop the over; the TX dial and mode go out, the mode fixes VFO B, the last `S 1` is
/// taken and the over keys. The control: a radio that refuses every `S 1` keys nothing, and since
/// split was never taken nothing is owed.
#[test]
fn through_hamlib_a_refused_first_split_on_leaves_the_last_to_decide() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    b.radio.vfos.lock().unwrap().split_on_needs_x = true;
    let overs = b.overs(2);
    assert_eq!(
        overs[0].key,
        ["S 1 VFOB", "I 14073500", "X PKTUSB -1", "S 1 VFOB", "T 1"],
        "the K3's first over"
    );
    assert_eq!(overs[1].key, ["S 1 VFOB", "T 1"], "held after it");

    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    b.radio.vfos.lock().unwrap().refuse_split_on = true;
    let across = b.ticks(800);
    assert!(!across.iter().any(|l| l == "T 1"), "keyed: {across:?}");
    assert_eq!(
        across
            .iter()
            .filter(|l| ["S ", "I ", "X "].iter().any(|v| l.starts_with(v)))
            .cloned()
            .collect::<Vec<_>>(),
        [
            "S 1 VFOB",
            "I 14073500",
            "X PKTUSB -1",
            "S 1 VFOB",
            "S 0 VFOA"
        ],
        "the writes, and the release putting VFO B's split back"
    );
    assert!(!b.engine.lock().unwrap().tx_enabled(), "TX still on");
    assert!(!b.rig.split_release_pending, "owed for a split never taken");
}

/// Test CAT takes the CAT port while a Rig split is held. The split is let go of through the link
/// before it goes, as a rebuild lets go of it; a release the radio does not take stays owed through
/// the hold (the Vox rig in the port's place sends nothing and settles nothing) and on the link
/// rebuilt after it, an automatic rebuild to the same radio, until the radio takes it. The control:
/// a release taken at the hold owes nothing and leaves the radio out of split.
#[test]
fn through_hamlib_test_cat_keeps_an_owed_split_release() {
    for (what, refused) in [("refused", true), ("taken", false)] {
        let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
        let _ = b.overs(1);
        assert!(b.radio.vfos.lock().unwrap().split, "{what}: premise: held");
        b.radio.vfos.lock().unwrap().refuse_split_off = refused;
        b.engine.lock().unwrap().hold_cat_port();
        let at_hold = b.ticks(1);
        assert!(
            at_hold.iter().any(|l| l == "S 0 VFOA"),
            "{what}: not let go before the port went: {at_hold:?}"
        );
        assert!(
            !b.rig.has_control(),
            "{what}: premise: the port handed over"
        );
        assert_eq!(
            b.rig.split_release_pending, refused,
            "{what}: owed at the hold"
        );
        let held = b.ticks(300);
        assert!(held.is_empty(), "{what}: written during the hold: {held:?}");
        assert_eq!(
            b.rig.split_release_pending, refused,
            "{what}: owed in the hold"
        );
        b.engine.lock().unwrap().release_cat_port();
        let address = b.radio.peer.address.clone();
        let _ = b.ticks_reopening(1, &address);
        assert!(b.rig.has_control(), "{what}: premise: the link back");
        assert_eq!(
            b.rig.split_release_pending, refused,
            "{what}: owed after it"
        );
        b.radio.vfos.lock().unwrap().refuse_split_off = false;
        let _ = b.ticks(150);
        assert!(!b.rig.split_release_pending, "{what}: still owed");
        assert!(!b.radio.vfos.lock().unwrap().split, "{what}: left in split");
    }
}
