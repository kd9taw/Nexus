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
    /// `\dump_caps` gets no answer at all, so Nexus could not ask whether the split read is native.
    caps_no_answer: bool,
    /// Split on (`S 1`) answered `RPRT -9` and not taken.
    refuse_split_on: bool,
    /// A K3 whose VFO B is in a mode it will not pair with VFO A's: split on (`S 1`) answered
    /// `RPRT -9` and not taken until a TX mode (`X`) is taken.
    split_on_needs_x: bool,
    /// The TX dial (`I`) answered `RPRT -9` and not taken.
    refuse_i: bool,
    /// The TX dial (`I`) taken, and answered this many ms late.
    i_delay_ms: u64,
    /// A radio slow to take VFO B to another band: an `I` that moves it off its band is taken,
    /// and answered this many ms late; one inside the band is answered at once.
    slow_band_change_ms: u64,
    /// The operator turning the radio's knob: each dial read (`f`) first moves VFO A to the last
    /// of these, until none is left.
    knob: Vec<u64>,
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
    hamlib_bench_on(cq_engine(split, tx_hz, |_| {}), FT8_DIAL, own_split)
}

/// [`hamlib_bench`] for `engine`, the radio's VFO A on `dial` (the engine's own dial).
fn hamlib_bench_on(engine: Arc<Mutex<Engine>>, dial: u64, own_split: Option<u64>) -> Bench<Hamlib> {
    let keyed = Arc::new(AtomicBool::new(false));
    let vfos = Arc::new(Mutex::new(Vfos {
        split: own_split.is_some(),
        b: own_split.unwrap_or(dial),
        ..Vfos::default()
    }));
    let (k, v) = (keyed.clone(), vfos.clone());
    let peer = retuning_peer(dial, "PKTUSB", move |line, radio| {
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
            "\\dump_caps" if v.caps_no_answer => Some(String::new()),
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
                if let Some(k) = v.knob.pop() {
                    radio.dial = k;
                }
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
            _ if line.starts_with("I ") && v.refuse_i => Some("RPRT -9\n".to_string()),
            _ if line.starts_with("I ") => {
                if v.i_delay_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(v.i_delay_ms));
                }
                let hz = line[2..].trim().parse().unwrap();
                if v.slow_band_change_ms > 0 && !crate::rig::tuning::same_named_band(v.b, hz) {
                    std::thread::sleep(std::time::Duration::from_millis(v.slow_band_change_ms));
                }
                v.b = hz;
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
/// (here, Split Operation set to None).
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
    let_go_by_split_none(&mut b);
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

/// What lets go of a held Rig split, each on its own: the band changing, Split Operation changed,
/// a mode Split Operation does not apply to. Each sends `S 0 VFOA` (no split of the radio's own to
/// put back) while the station receives, before anything keys. TX turned off is not one of them,
/// as in WSJT-X, which keeps split on while it only monitors
/// (`widgets/mainwindow.cpp:12378-12387`): nothing is written and the radio stays in split. The
/// control: nothing changes, nothing written.
#[test]
fn through_hamlib_a_held_rig_split_is_let_go_when_the_band_or_split_mode_changes_not_tx_off() {
    type Change = fn(&mut Engine);
    let cases: [(&str, Option<Change>, bool); 5] = [
        (
            // TX turned on again after the band change's own halt: the band alone lets go.
            "the band changes",
            Some(|e| {
                e.set_frequency(7.074, "40m", "USB");
                e.set_tx_enabled(true);
            }),
            true,
        ),
        (
            "Split Operation is set to None",
            Some(|e| {
                let mut s = e.settings().clone();
                s.split_mode = SplitMode::None;
                e.apply_settings(s);
            }),
            true,
        ),
        (
            "a mode Split Operation does not apply to",
            Some(|e| e.set_tier(Tier::TempoFast)),
            true,
        ),
        (
            "TX is turned off: held",
            Some(|e| e.set_tx_enabled(false)),
            false,
        ),
        ("control: nothing changes", None, false),
    ];
    for (what, change, lets_go) in cases {
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
        match lets_go {
            true => {
                assert_eq!(sent, ["S 0 VFOA"], "{what}: wire {writes:?}");
                assert!(
                    !b.radio.vfos.lock().unwrap().split,
                    "{what}: still in split"
                );
            }
            false => {
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

/// A split release the radio does not take is still owed. Split Operation set to None after a held
/// Rig split, the radio refusing `S 0` (`RPRT -9`): nothing keys while it is owed, even at the next
/// own boundary with TX on (the over is refused with the split reason and TX halts); the
/// receive-time split read, which now finds the radio in split, never takes that for the
/// operator's own split; and once the radio takes `S 0` again, a retry at a quiet point lets go.
#[test]
fn through_hamlib_a_refused_split_release_is_retried_at_quiet_points_and_nothing_keys_meanwhile() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    assert!(b.radio.vfos.lock().unwrap().split, "premise: held");
    b.radio.vfos.lock().unwrap().refuse_split_off = true;
    let_go_by_split_none(&mut b);
    let first = b.ticks(3);
    assert_eq!(first, ["S 0 VFOA"], "premise: the release, refused");
    assert!(
        b.rig.split_release_pending,
        "the refused release is not owed"
    );
    // The next own boundary, TX still on: the over is refused, never keyed.
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

// ── Keys that are not slot overs, with Rig split held ────────────────────────────────────────
//
// Split Operation = Rig holds split between overs, VFO B on the last over's TX dial (the RX dial
// plus a 500 Hz step). A tune, a CW word (the CW ID after 73 is one, queued by `send_cw` exactly
// as below), the operator's PTT, an APRS beacon and the radio's ATU tune-up are not slot overs:
// nothing wrote VFO B for them, so they keyed out of it at the shifted dial, a frequency the
// licence gate never judged (it judges them from the RX dial). WSJT-X writes the TX frequency
// before every key with split on, XIT 0's being the RX dial; before such a key Nexus puts VFO B
// back on the RX dial, or lets go of a hold that has ended.

/// The split, TX dial and PTT writes in `writes`, the rest left out.
fn split_and_keys(writes: &[String]) -> Vec<&str> {
    writes
        .iter()
        .map(String::as_str)
        .filter(|l| l.starts_with(['S', 'I', 'T']))
        .collect()
}

/// The 20 m dial Nexus's first over put a 500 Hz shift under, and VFO B's TX dial for it.
const SHIFTED_TX_DIAL: u64 = 14_073_500;

/// ⛔ A tune with Rig split held after a shifted over keys only once VFO B is back on the RX dial,
/// written just before the key (`I` at the dial, then the key). Split stays on, as WSJT-X holds it.
#[test]
fn a_tune_with_rig_split_held_keys_with_vfo_b_back_on_the_rx_dial() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    assert_eq!(
        b.radio.vfos.lock().unwrap().b,
        SHIFTED_TX_DIAL,
        "premise: VFO B on the over's TX dial"
    );
    b.engine.lock().unwrap().set_tune(true);
    let writes = b.ticks(3);
    let sent = split_and_keys(&writes);
    assert_eq!(sent.len(), 2, "wire {writes:?}");
    assert_eq!(
        sent[0], "I 14074000",
        "VFO B not put back before the key: {writes:?}"
    );
    assert!(is_key(sent[1]), "the tune did not key: {writes:?}");
    let v = b.radio.vfos.lock().unwrap();
    assert_eq!(
        (v.split, v.b),
        (true, FT8_DIAL),
        "split held, VFO B on the dial"
    );
}

/// ⛔ A CW word in the Digital section (what the CW ID after 73 sends, through `send_cw`) with Rig
/// split held: VFO B back on the RX dial before ANY keyer has it. The soundcard keyer's PTT comes
/// after the `I`; the radio's own keyer (CAT) is handed the word (`b`) only after it.
#[test]
fn a_cw_word_with_rig_split_held_keys_with_vfo_b_back_on_the_rx_dial() {
    use tempo_app::settings::CwKeyerBackend;
    for keyer in [CwKeyerBackend::Soundcard, CwKeyerBackend::Cat] {
        let engine = cq_engine(SplitMode::Rig, 1441.0, |s| s.cw_keyer = keyer);
        let mut b = hamlib_bench_on(engine, FT8_DIAL, None);
        let _ = b.overs(1);
        assert_eq!(
            b.radio.vfos.lock().unwrap().b,
            SHIFTED_TX_DIAL,
            "{keyer:?}: premise"
        );
        let lines_before = b.radio.peer.lines.lock().unwrap().len();
        {
            let mut e = b.engine.lock().unwrap();
            let mycall = e.settings().mycall.clone();
            e.send_cw(&mycall);
        }
        let writes = b.ticks(3);
        let raw: Vec<String> = b.radio.peer.lines.lock().unwrap()[lines_before..].to_vec();
        let vfo_b = raw.iter().position(|l| l == "I 14074000");
        let keyed = raw
            .iter()
            .position(|l| is_key(l) || l.starts_with("b "))
            .unwrap_or_else(|| panic!("{keyer:?}: the word was not keyed: {raw:?}"));
        assert!(
            vfo_b.is_some_and(|i| i < keyed),
            "{keyer:?}: VFO B not put back before the key: {raw:?}"
        );
        assert_eq!(
            b.radio.vfos.lock().unwrap().b,
            FT8_DIAL,
            "{keyer:?}: VFO B left on the shifted dial: {writes:?}"
        );
    }
}

/// ⛔ The operator's PTT (a Remote's too: the same latch) between two overs with Rig split held:
/// VFO B back on the RX dial before the key. And the slot overs are as they were: split is never
/// let go between them, and the next over writes its own TX dial again before its key.
#[test]
fn a_ptt_between_held_split_overs_keys_on_the_rx_dial_and_the_next_over_rewrites_its_tx_dial() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let engine = b.engine.clone();
    let slot = b.slot;
    // Two seconds into the receive period after the first over, held for two.
    let overs = b.overs_hooked(
        3,
        |_| {},
        move |t, _| {
            let mut e = engine.lock().unwrap();
            if (slot + 17_000.0..slot + 17_020.0).contains(&t) {
                e.set_ptt(true);
            }
            if (slot + 19_000.0..slot + 19_020.0).contains(&t) {
                e.set_ptt(false);
            }
        },
    );
    for (i, o) in overs.iter().enumerate() {
        eprintln!("PTT between held overs, key {}: {o:?}", i + 1);
    }
    assert_eq!(
        overs[0].key,
        ["S 1 VFOB", "I 14073500", "X PKTUSB -1", "S 1 VFOB", "T 1"],
        "premise: the first over"
    );
    let ptt = split_and_keys(&overs[1].key);
    assert_eq!(ptt.len(), 2, "the PTT's key: {:?}", overs[1].key);
    assert_eq!(ptt[0], "I 14074000", "VFO B not put back before the PTT");
    assert!(is_key(ptt[1]), "the PTT did not key: {:?}", overs[1].key);
    assert_eq!(
        overs[2].key,
        ["I 14073500", "S 1 VFOB", "T 1"],
        "the next over's TX dial written again before its key"
    );
    let let_go = overs
        .iter()
        .flat_map(|o| o.key.iter().chain(&o.unkey).chain(&o.rx))
        .filter(|l| l.starts_with("S 0"))
        .count();
    assert_eq!(let_go, 0, "split let go between overs");
    let v = b.radio.vfos.lock().unwrap();
    assert_eq!(
        (v.split, v.b),
        (true, SHIFTED_TX_DIAL),
        "held, on the TX dial"
    );
}

/// A CQ by a General on 20 m FT8, the dial 200 Hz below the bottom of the class's data privileges
/// (14.025 MHz) and the TX offset 200 Hz: the gate judges the over at 14.0250 MHz and allows it.
fn general_cq_near_the_edge(mhz: f64) -> Arc<Mutex<Engine>> {
    let engine = cq_engine(SplitMode::Rig, 200.0, |_| {});
    {
        let mut e = engine.lock().unwrap();
        assert!(e.set_license_class("general"), "premise: the class");
        e.set_frequency(mhz, "20m", "USB");
        e.set_tx_offset(200.0);
        e.start_cq(None).expect("Call CQ");
        let _ = e.take_immediate_tx();
    }
    engine
}

/// Whether a General may transmit data on `hz` (a single frequency, judged as a span of 1 Hz).
fn general_may_send_data_on(hz: u64) -> bool {
    use tempo_app::settings::{LicenseClass, OperatingMode};
    let mhz = hz as f64 / 1e6;
    tempo_app::privileges::tx_span_allowed(
        LicenseClass::General,
        mhz,
        mhz + 1e-6,
        OperatingMode::Digital,
    )
}

/// ⛔ THE LICENCE EDGE. The over's 200 Hz offset is shifted 1500 Hz down (audio at 1700 Hz, VFO B
/// at 14.0233 MHz). A tune after it, its tone at 1500 Hz, went out of VFO B there at 14.0248 MHz:
/// below the General's edge, where the gate (which judged 14.0250) never looked. With VFO B back
/// on the dial it goes out at 14.0263 MHz, inside, where the station's no-split tune goes.
///
/// The control: the dial moved 200 Hz down, the gate judges 14.0249 MHz, outside the class, and the
/// tune is refused before anything is written or keyed.
#[test]
fn a_tune_near_the_band_edge_goes_out_inside_the_privileges_the_gate_judged() {
    let mut b = hamlib_bench_on(general_cq_near_the_edge(14.0248), 14_024_800, None);
    let overs = b.overs(1);
    assert_eq!(
        overs[0].key,
        ["S 1 VFOB", "I 14023300", "X PKTUSB -1", "S 1 VFOB", "T 1"],
        "premise: the over's TX dial 1500 Hz down"
    );
    assert!(
        b.engine.lock().unwrap().tx_allowed(),
        "premise: the gate allows the dial"
    );
    b.engine.lock().unwrap().set_tune(true);
    let writes = b.ticks(3);
    assert!(
        split_and_keys(&writes).iter().any(|l| is_key(l)),
        "premise: the tune keyed: {writes:?}"
    );
    let tune_rf = b.radio.vfos.lock().unwrap().b + TUNE_FREQ_HZ as u64;
    assert_eq!(tune_rf, 14_026_300, "the tune's carrier: {writes:?}");
    assert!(
        general_may_send_data_on(tune_rf),
        "the tune went out outside the privileges at {tune_rf} Hz"
    );
    assert!(
        !general_may_send_data_on(SHIFTED_TX_DIAL_NEAR_EDGE + TUNE_FREQ_HZ as u64),
        "premise: on the shifted VFO B the tune was outside"
    );

    // The control.
    let mut c = hamlib_bench_on(general_cq_near_the_edge(14.0249), 14_024_900, None);
    let _ = c.overs(1);
    {
        let mut e = c.engine.lock().unwrap();
        e.set_frequency(14.0247, "20m", "USB");
        e.set_tx_enabled(true);
        assert!(!e.tx_allowed(), "premise: the gate refuses the moved dial");
        e.set_tune(true);
        assert!(!e.tuning(), "a tune armed outside the privileges");
    }
    let writes = c.ticks(5);
    assert!(
        !writes.iter().any(|l| is_key(l) || l.starts_with("I ")),
        "keyed or VFO B written outside the privileges: {writes:?}"
    );
}

/// VFO B's TX dial for the over at 14.0248 MHz with its 200 Hz offset shifted 1500 Hz down.
const SHIFTED_TX_DIAL_NEAR_EDGE: u64 = 14_023_300;

/// ⛔ FAIL CLOSED, AND NEVER LATE. VFO B's write for a tune the radio refuses, or confirms only
/// after the split writes' budget (500 ms), keys nothing: the tune ends, its line says why, VFO B
/// is where it was or where the radio put it, and nothing is keyed after the budget. The control:
/// a write confirmed inside the budget (100 ms) keys.
#[test]
fn a_tune_whose_vfo_b_write_is_refused_or_late_keys_nothing() {
    type Radio = fn(&mut Vfos);
    let cases: [(&str, Radio, bool); 3] = [
        ("refused", |v| v.refuse_i = true, false),
        ("answered after the budget", |v| v.i_delay_ms = 650, false),
        (
            "control: answered inside the budget",
            |v| v.i_delay_ms = 100,
            true,
        ),
    ];
    for (what, radio, keys) in cases {
        let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
        let _ = b.overs(1);
        radio(&mut b.radio.vfos.lock().unwrap());
        b.engine.lock().unwrap().set_tune(true);
        let writes = b.ticks(5);
        let sent = split_and_keys(&writes);
        assert_eq!(sent.first(), Some(&"I 14074000"), "{what}: wire {writes:?}");
        assert_eq!(
            sent.iter().any(|l| is_key(l)),
            keys,
            "{what}: keyed: {writes:?}"
        );
        let e = b.engine.lock().unwrap();
        assert_eq!(e.tuning(), keys, "{what}: the tune's state");
        if !keys {
            let line = e.snapshot().radio.audio_error;
            assert!(
                line.as_deref()
                    .is_some_and(|w| w.contains("Split Operation")),
                "{what}: nothing says why: {line:?}"
            );
        }
    }
}

/// ⛔ An APRS beacon and the radio's ATU tune-up from the Digital section with Rig split held: the
/// beacon's key and the tune-up's `U TUNER` (the radio keys its own carrier for it) come only after
/// VFO B is back on the RX dial.
#[test]
fn an_aprs_beacon_and_an_atu_tune_up_with_rig_split_held_go_out_with_vfo_b_on_the_rx_dial() {
    type Press = fn(&mut Engine);
    type Keys = fn(&str) -> bool;
    // What is pressed, how, what its key looks like on the wire, and how many keys the run sees
    // (the beacon's PTT is one; the tune-up is the radio's own and keys no PTT).
    let cases: [(&str, Press, Keys, usize); 2] = [
        (
            "APRS beacon",
            |e| {
                e.aprs_beacon(41.88, -87.63, '/', '>', "split", &[])
                    .expect("the beacon is queued");
            },
            is_key,
            3,
        ),
        (
            "ATU tune-up",
            |e| {
                // A radio whose tuner is in line and whose CAT can start a tune-up.
                e.observe_rig_tuner(Some(true), true);
                e.atu_tune().expect("the press is taken");
            },
            |l| l.starts_with("U TUNER "),
            2,
        ),
    ];
    for (what, press, keys, n) in cases {
        let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
        let engine = b.engine.clone();
        let lines = b.radio.peer.lines.clone();
        let pressed_at = Arc::new(Mutex::new(None));
        let at = pressed_at.clone();
        let slot = b.slot;
        // Two seconds into the receive period after the first over.
        let _ = b.overs_hooked(
            n,
            |_| {},
            move |t, _| {
                if (slot + 17_000.0..slot + 17_020.0).contains(&t) {
                    *at.lock().unwrap() = Some(lines.lock().unwrap().len());
                    press(&mut engine.lock().unwrap());
                }
            },
        );
        let from = pressed_at.lock().unwrap().expect("premise: pressed");
        let raw: Vec<String> = b.radio.peer.lines.lock().unwrap()[from..].to_vec();
        let keyed = raw
            .iter()
            .position(|l| keys(l))
            .unwrap_or_else(|| panic!("{what}: nothing keyed: {raw:?}"));
        assert!(
            raw[..keyed].iter().any(|l| l == "I 14074000"),
            "{what}: VFO B not put back before the key: {raw:?}"
        );
    }
}

/// ⛔ A PTT press whose VFO B write the radio refuses keys nothing, and, as any key the radio
/// refused, is not tried again while it is held: one `I`, no key, then only the release's `T 0`,
/// even with the radio taking the write again meanwhile. Let go and pressed again, it keys, VFO B
/// on the dial first. The next over is as it always was.
#[test]
fn a_ptt_press_refused_for_the_split_is_not_tried_again_while_it_is_held() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let engine = b.engine.clone();
    let vfos = b.radio.vfos.clone();
    let slot = b.slot;
    // In the receive period after the first over: pressed with the write refused, the radio taking
    // it again a moment later, let go; then pressed again and let go.
    let overs = b.overs_hooked(
        3,
        |_| {},
        move |t, _| {
            let at = |ms: f64| (slot + ms..slot + ms + 20.0).contains(&t);
            let mut e = engine.lock().unwrap();
            if at(17_000.0) {
                vfos.lock().unwrap().refuse_i = true;
                e.set_ptt(true);
            }
            if at(17_200.0) {
                vfos.lock().unwrap().refuse_i = false;
            }
            if at(18_000.0) {
                e.set_ptt(false);
            }
            if at(18_200.0) {
                e.set_ptt(true);
            }
            if at(19_000.0) {
                e.set_ptt(false);
            }
        },
    );
    for (i, o) in overs.iter().enumerate() {
        eprintln!("refused PTT, key {}: {o:?}", i + 1);
    }
    assert_eq!(
        split_and_keys(&overs[0].rx),
        ["I 14074000", "T 0"],
        "the refused press: one write, no key, then its release"
    );
    let again = split_and_keys(&overs[1].key);
    assert_eq!(again.len(), 2, "pressed again: {:?}", overs[1].key);
    assert_eq!(again[0], "I 14074000", "pressed again: {:?}", overs[1].key);
    assert!(is_key(again[1]), "pressed again: {:?}", overs[1].key);
    // The refused write forgot what the radio had confirmed, so its TX mode goes out again too.
    assert_eq!(
        overs[2].key,
        ["I 14073500", "X PKTUSB -1", "S 1 VFOB", "T 1"],
        "the next over's TX dial written again before its key"
    );
}

/// TX turned off holds the split, as WSJT-X does, so a tune then (the tune is the one key that
/// needs no TX latch) keys as with any held split: VFO B back on the RX dial first, then the key.
/// Split stays on.
#[test]
fn a_tune_after_tx_is_turned_off_keys_with_vfo_b_back_on_the_rx_dial() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    {
        let mut e = b.engine.lock().unwrap();
        e.set_tx_enabled(false);
        e.set_tune(true);
    }
    let writes = b.ticks(3);
    let sent = split_and_keys(&writes);
    assert_eq!(sent.len(), 2, "wire {writes:?}");
    assert_eq!(
        sent[0], "I 14074000",
        "VFO B not back on the RX dial before the key: {writes:?}"
    );
    assert!(is_key(sent[1]), "the tune did not key: {writes:?}");
    assert!(b.radio.vfos.lock().unwrap().split, "split let go");
}

/// What a key that is not a slot over finds on the first tick after the hold ends, before the quiet
/// point lets go of split later in that tick: the operator's PTT (readied at its own site) and an
/// RTTY over (keyed through `key_over`, the one site every such key passes), each pressed on the
/// tick the station leaves the digital screens. The licence is Open, so phone at the FT8 dial is
/// not refused before it reaches the radio loop.
type FirstTickKey = (&'static str, &'static str, fn(&mut Engine));
const FIRST_TICK_KEYS: [FirstTickKey; 2] = [
    ("PTT", "phone", |e| e.set_ptt(true)),
    ("RTTY", "rtty", |e| {
        e.rtty_send_text("CQ").expect("the RTTY over is queued");
    }),
];

/// ⛔ A HOLD THAT HAS ENDED IS LET GO BEFORE THE KEY, IN THE SAME TICK. The station leaves the
/// digital screens with Rig split held and keys at once: `S 0 VFOA` first, then the key, from
/// VFO A. Before the guard covered every key, the RTTY over keyed out of VFO B on the last over's
/// shifted TX dial, and the quiet point let go only after it.
#[test]
fn a_key_on_the_first_tick_after_leaving_digital_lets_go_of_the_split_first() {
    for (what, section, press) in FIRST_TICK_KEYS {
        let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
        let _ = b.overs(1);
        assert_eq!(
            b.radio.vfos.lock().unwrap().b,
            SHIFTED_TX_DIAL,
            "{what}: premise, VFO B on the over's TX dial"
        );
        {
            let mut e = b.engine.lock().unwrap();
            e.set_license_class("open");
            e.set_operating_mode(section, false);
            assert!(e.tx_enabled(), "{what}: premise, TX on in {section}");
            press(&mut e);
        }
        let writes = b.ticks(3);
        let sent = split_and_keys(&writes);
        assert_eq!(
            sent.first(),
            Some(&"S 0 VFOA"),
            "{what}: split not let go before the key: {writes:?}"
        );
        assert!(
            sent.get(1).is_some_and(|l| is_key(l)),
            "{what}: did not key after the release: {writes:?}"
        );
        assert!(
            !b.radio.vfos.lock().unwrap().split,
            "{what}: still in split"
        );
    }
}

/// ⛔ …AND A RELEASE THE RADIO REFUSES KEYS NOTHING, and keeps refusing every key while it is owed.
/// The same first tick, the radio answering `S 0` with `RPRT -9`: nothing keys, the PTT line names
/// Split Operation, the release is owed. Pressed again while it is owed (released and pressed, or
/// another over queued), still nothing keys. Once the radio takes the release at a quiet point, the
/// next press keys, from VFO A.
#[test]
fn a_key_whose_split_release_the_radio_refuses_keys_nothing_until_the_release_is_taken() {
    for (what, section, press) in FIRST_TICK_KEYS {
        let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
        let _ = b.overs(1);
        b.radio.vfos.lock().unwrap().refuse_split_off = true;
        {
            let mut e = b.engine.lock().unwrap();
            e.set_license_class("open");
            e.set_operating_mode(section, false);
            press(&mut e);
        }
        let writes = b.ticks(3);
        assert!(
            !writes.iter().any(|l| is_key(l)),
            "{what}: keyed with the release refused: {writes:?}"
        );
        assert_eq!(
            split_and_keys(&writes).first(),
            Some(&"S 0 VFOA"),
            "{what}: premise, the release was sent: {writes:?}"
        );
        assert!(b.rig.split_release_pending, "{what}: premise, owed");
        let line = b.engine.lock().unwrap().snapshot().radio.audio_error;
        assert!(
            line.as_deref()
                .is_some_and(|w| w.contains("Split Operation")),
            "{what}: nothing says why: {line:?}"
        );

        // Owed: pressed again (the PTT let go first), still nothing keys.
        {
            let mut e = b.engine.lock().unwrap();
            e.set_ptt(false);
        }
        let _ = b.ticks(2);
        {
            let mut e = b.engine.lock().unwrap();
            e.set_tx_enabled(true);
            press(&mut e);
        }
        let owed = b.ticks(3);
        assert!(
            !owed.iter().any(|l| is_key(l)),
            "{what}: keyed while the release is owed: {owed:?}"
        );

        // The radio takes it at a quiet point; then a press keys.
        {
            let mut e = b.engine.lock().unwrap();
            e.set_ptt(false);
        }
        b.radio.vfos.lock().unwrap().refuse_split_off = false;
        let _ = b.ticks(150);
        assert!(!b.rig.split_release_pending, "{what}: premise, taken");
        {
            let mut e = b.engine.lock().unwrap();
            e.set_tx_enabled(true);
            press(&mut e);
        }
        let after = b.ticks(3);
        assert!(
            after.iter().any(|l| is_key(l)),
            "CONTROL {what}: the release taken, the press keys: {after:?}"
        );
        assert!(
            !after.iter().any(|l| l.starts_with("I ")),
            "{what}: VFO B written after the release: {after:?}"
        );
    }
}

/// ⛔ A RELEASE STILL OWED REFUSES A TUNE TOO. Split Operation set to None after a held over, the
/// radio refusing `S 0`: the release is owed, split may still be on with VFO B on the shifted TX
/// dial. A tune (the one key that needs no TX latch) keys nothing while it is owed and its line
/// names Split Operation; it used to key out of VFO B. The control: once the radio takes the
/// release, a tune keys from VFO A.
#[test]
fn a_tune_while_a_split_release_is_owed_keys_nothing() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    b.radio.vfos.lock().unwrap().refuse_split_off = true;
    let_go_by_split_none(&mut b);
    let first = b.ticks(3);
    assert_eq!(first, ["S 0 VFOA"], "premise: the release, refused");
    assert!(b.rig.split_release_pending, "premise: owed");
    b.engine.lock().unwrap().set_tune(true);
    let writes = b.ticks(5);
    assert!(
        !writes.iter().any(|l| is_key(l)),
        "a tune keyed while the release is owed: {writes:?}"
    );
    {
        let e = b.engine.lock().unwrap();
        assert!(!e.tuning(), "the refused tune is still asked for");
        let line = e.snapshot().radio.audio_error;
        assert!(
            line.as_deref()
                .is_some_and(|w| w.contains("Split Operation")),
            "nothing says why: {line:?}"
        );
    }
    b.radio.vfos.lock().unwrap().refuse_split_off = false;
    let _ = b.ticks(150);
    assert!(!b.rig.split_release_pending, "premise: taken");
    b.engine.lock().unwrap().set_tune(true);
    let after = b.ticks(3);
    assert!(
        after.iter().any(|l| is_key(l)),
        "CONTROL: the release taken, the tune keys: {after:?}"
    );
}

/// ⛔ A KEY THAT ONLY `key_over` READIES, WITH THE HOLD STANDING: a voice-keyer message in the
/// Digital section (the Engine has no section backstop for it; an Open licence allows it there).
/// VFO B goes back on the RX dial before its key, as for the keys readied at their own site.
#[test]
fn a_voice_keyer_message_with_rig_split_held_keys_with_vfo_b_back_on_the_rx_dial() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    assert_eq!(b.radio.vfos.lock().unwrap().b, SHIFTED_TX_DIAL, "premise");
    {
        let mut e = b.engine.lock().unwrap();
        e.set_license_class("open");
        e.send_voice(vec![0.1; 2_400])
            .expect("the message is queued");
    }
    let writes = b.ticks(3);
    let sent = split_and_keys(&writes);
    assert_eq!(
        sent.first(),
        Some(&"I 14074000"),
        "VFO B not put back before the voice key: {writes:?}"
    );
    assert!(
        sent.get(1).is_some_and(|l| is_key(l)),
        "the message did not key: {writes:?}"
    );
    let v = b.radio.vfos.lock().unwrap();
    assert_eq!(
        (v.split, v.b),
        (true, FT8_DIAL),
        "split held, VFO B on the dial"
    );
    drop(v);

    // VFO B's write refused: the message keys nothing, and the PTT line names Split Operation,
    // not the generic PTT advice the voice keyer's own report would put over it.
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    b.radio.vfos.lock().unwrap().refuse_i = true;
    {
        let mut e = b.engine.lock().unwrap();
        e.set_license_class("open");
        e.send_voice(vec![0.1; 2_400])
            .expect("the message is queued");
    }
    let writes = b.ticks(3);
    assert!(
        !writes.iter().any(|l| is_key(l)),
        "keyed with VFO B's write refused: {writes:?}"
    );
    let line = b.engine.lock().unwrap().snapshot().radio.audio_error;
    assert!(
        line.as_deref()
            .is_some_and(|w| w.contains("Split Operation")),
        "the line does not name the split: {line:?}"
    );
}

/// ⛔ A CW ID INSIDE THE FAKE IT WRITE-BACK'S WAIT GOES OUT FROM THE DIAL. After a Fake It over
/// the write-back waits 100 ms, as WSJT-X waits after PTT off, and the CW ID after 73 lands in
/// exactly that wait. It used to key from VFO A still on the over's TX dial (14.0735 at a 1441 Hz
/// offset): the soundcard keyer's `T 1` and the radio's own keyer's word with no `F` before them,
/// 500 Hz from where the gate judged it. Now the write-back goes ahead of any keyer, as it does for
/// a tune.
#[test]
fn a_cw_id_inside_the_fake_it_write_back_wait_keys_after_the_write_back() {
    use tempo_app::settings::CwKeyerBackend;
    for keyer in [CwKeyerBackend::Soundcard, CwKeyerBackend::Cat] {
        let engine = cq_engine(SplitMode::FakeIt, 1441.0, |s| s.cw_keyer = keyer);
        let mut b = hamlib_bench_on(engine, FT8_DIAL, None);
        let _ = b.overs(1);
        // The write-back owed and its wait running, as the unkey leaves it.
        b.state.fake_it_restore = Some(FT8_DIAL);
        b.state.fake_it_tx_dial = Some(SHIFTED_TX_DIAL);
        b.state.fake_it_restore_tries = 0;
        b.state.fake_it_restore_at = Some(b.t + 100.0);
        b.state.last_freq_poll = b.t + 10_000.0;
        b.state.last_rig_poll = b.t + 10_000.0;
        let from = b.radio.peer.lines.lock().unwrap().len();
        {
            let mut e = b.engine.lock().unwrap();
            let mycall = e.settings().mycall.clone();
            e.send_cw(&mycall);
        }
        let _ = b.ticks(2);
        let raw: Vec<String> = b.radio.peer.lines.lock().unwrap()[from..].to_vec();
        let keyed = raw
            .iter()
            .position(|l| is_key(l) || l.starts_with("b "))
            .unwrap_or_else(|| panic!("{keyer:?}: the CW ID was not keyed: {raw:?}"));
        assert!(
            raw[..keyed].iter().any(|l| l == "F 14074000"),
            "{keyer:?}: keyed from the over's TX dial, no write-back first: {raw:?}"
        );
    }
}

/// ⛔ THE FAKE IT COUNTERPART OF AN OWED RELEASE. A Fake It over moved VFO A to its TX dial
/// (14.0735 at a 1441 Hz offset); the write-back after it was refused and is still owed, the radio
/// refusing every dial write meanwhile. A tune then used to key from the shifted VFO A, its tone
/// 500 Hz below where the gate judged it (`fake_it_write_back_before_key` leaves a refused try to
/// the teardown): it keys nothing now, the tune ends and the line names Split Operation. The
/// control: the same write-back owed but not yet tried goes out ahead of the tune's key, as before.
/// (Inside the over's own period the engine already refuses the press; this is the key that comes
/// after it, and any other key that is not a slot over takes the same guard.)
#[test]
fn a_tune_while_a_refused_fake_it_write_back_is_owed_keys_nothing() {
    for (what, tried, keys) in [
        ("a refused try owed", 1, false),
        ("control: not yet tried", 0, true),
    ] {
        let mut b = hamlib_bench(SplitMode::FakeIt, 1441.0, None);
        let _ = b.overs(1);
        // The write-back owed, as the teardown leaves it after a refused try, VFO A still on the
        // over's TX dial as far as anyone knows.
        b.state.fake_it_restore = Some(FT8_DIAL);
        b.state.fake_it_tx_dial = Some(SHIFTED_TX_DIAL);
        b.state.fake_it_restore_tries = tried;
        b.state.fake_it_restore_at = None;
        // No dial read in these few ticks: one would find where the radio really is and settle it.
        b.state.last_freq_poll = b.t + 10_000.0;
        b.state.last_rig_poll = b.t + 10_000.0;
        b.radio.vfos.lock().unwrap().refuse_f = if keys { 0 } else { 10 };
        b.engine.lock().unwrap().set_tune(true);
        let writes = b.ticks(3);
        let keyed = writes.iter().any(|l| is_key(l));
        assert_eq!(keyed, keys, "{what}: keyed: {writes:?}");
        if keys {
            let f = writes.iter().position(|l| l == "F 14074000");
            let k = writes.iter().position(|l| is_key(l));
            assert!(
                f < k && f.is_some(),
                "{what}: the write-back before the key: {writes:?}"
            );
        } else {
            let e = b.engine.lock().unwrap();
            assert!(!e.tuning(), "{what}: the refused tune is still asked for");
            let line = e.snapshot().radio.audio_error;
            assert!(
                line.as_deref()
                    .is_some_and(|w| w.contains("Split Operation")),
                "{what}: nothing says why: {line:?}"
            );
        }
    }
}

impl<R: Radio> Bench<R> {
    /// [`Self::ticks`], a rebuilt link reopening through `addr` (the same radio, a new connection).
    fn ticks_reopening(&mut self, n: usize, addr: &str) -> Vec<String> {
        self.ticks_reopening_probed(n, addr, None)
    }

    /// [`Self::ticks_reopening`], the reopen's probe answering `ok` (`Some(true)`: the open
    /// itself confirmed the CAT link, as a radio that answers at once does).
    fn ticks_reopening_probed(&mut self, n: usize, addr: &str, ok: Option<bool>) -> Vec<String> {
        let sinks = Sinks {
            wsjtx: None,
            psk: None,
        };
        let mut ra = mock_reopen_audio();
        let addr = addr.to_string();
        let mut rr = move |_t: &Transport, _coexist: bool| {
            (Rig::rigctld(&addr), None, CatProbe::status(ok, ""))
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
    let_go_by_split_none(&mut b);
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
    let_go_by_split_none(&mut b);
    assert_eq!(b.ticks(3), ["S 0 VFOA"], "premise: the release");
    save_settings(&mut b, |s| s.split_mode = SplitMode::Rig);
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
    let_go_by_split_none(&mut b);
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

/// Let go of a held Rig split while the station receives: Split Operation set to None. (TX turned
/// off no longer lets go of it, as in WSJT-X.)
fn let_go_by_split_none<R: Radio>(b: &mut Bench<R>) {
    save_settings(b, |s| s.split_mode = SplitMode::None);
}

fn save_settings<R: Radio>(b: &mut Bench<R>, edit: impl FnOnce(&mut Settings)) {
    let mut e = b.engine.lock().unwrap();
    let mut s = e.settings().clone();
    edit(&mut s);
    e.apply_settings(s);
}

fn call_cq_at_our_period<R: Radio>(b: &mut Bench<R>) {
    while b.t.rem_euclid(30_000.0) < 29_000.0 {
        let _ = b.ticks(1);
    }
    let mut e = b.engine.lock().unwrap();
    e.set_tx_enabled(true);
    e.start_cq(None).expect("Call CQ");
    let _ = e.take_immediate_tx();
}

/// ⛔ WHETHER THE SPLIT READ IS NATIVE IS A QUESTION FOR EACH RADIO. Radio A reads its split
/// natively; the operator then picks radio B, another model, whose split read is not native (a
/// cache stub: `s` answers 0 for a split it never confirmed). A release owed on B is not freed by
/// B's `s` = 0 on A's answer. CONTROL: when B reads natively too, its definite "off" frees it.
#[test]
fn through_hamlib_a_native_split_read_on_one_radio_is_not_believed_for_the_next() {
    for (what, b_native) in [("B not native", false), ("control: B native", true)] {
        let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
        let address = b.radio.peer.address.clone();
        save_settings(&mut b, |s| s.rig_model = 3073);
        let _ = b.ticks_reopening(1, &address);
        call_cq_at_our_period(&mut b);
        {
            let mut v = b.radio.vfos.lock().unwrap();
            v.split_read_native = true;
            v.split_no_answer = true;
        }
        let on_a = b.ticks(800);
        assert!(
            on_a.iter().any(|l| l == "S 0 VFOA") && !on_a.iter().any(|l| l == "T 1"),
            "{what}: premise: a release owed on A: {on_a:?}"
        );
        let _ = b.ticks(300);
        assert!(
            !b.rig.split_release_pending,
            "{what}: premise: A's native read freed it"
        );
        // The operator chooses radio B.
        b.radio.vfos.lock().unwrap().split_read_native = b_native;
        save_settings(&mut b, |s| s.rig_model = 1035);
        // B answers its open at once, so no later CAT confirmation asks it anything afresh.
        let _ = b.ticks_reopening_probed(1, &address, Some(true));
        assert!(b.rig.has_control(), "{what}: premise: B connected");
        call_cq_at_our_period(&mut b);
        let on_b = b.ticks(800);
        assert!(!on_b.iter().any(|l| l == "T 1"), "{what}: keyed: {on_b:?}");
        assert!(
            on_b.iter().any(|l| l == "S 1 VFOB") && on_b.iter().any(|l| l == "S 0 VFOA"),
            "{what}: premise: a release owed on B: {on_b:?}"
        );
        let _ = b.ticks(300);
        assert_eq!(b.rig.split_release_pending, !b_native, "{what}: owed on B");
    }
}

/// A radio whose split capability could not be asked (no answer to `\dump_caps`) is asked again
/// later, so a radio that reads its split natively is read once it answers. CONTROL: before it
/// answers, its split is not read.
#[test]
fn through_hamlib_a_split_capability_nexus_could_not_ask_is_asked_again() {
    let mut b = hamlib_bench(SplitMode::None, 1441.0, Some(14_075_000));
    save_settings(&mut b, |s| s.split_detect_enabled = true);
    {
        let mut v = b.radio.vfos.lock().unwrap();
        v.caps_no_answer = true;
        v.split_read_native = true;
    }
    let reads = |b: &Bench<Hamlib>, from: usize| -> (usize, usize) {
        let sent = b.radio.sent();
        let caps = sent[from..]
            .iter()
            .filter(|l| *l == "\\dump_caps ?")
            .count();
        let split = sent[from..].iter().filter(|l| *l == "s ?").count();
        (caps, split)
    };
    let _ = b.ticks(1000);
    let (caps, split) = reads(&b, 0);
    assert!(caps > 0, "premise: the capability was asked");
    assert_eq!(split, 0, "control: read before the radio answered");
    b.radio.vfos.lock().unwrap().caps_no_answer = false;
    let from = b.radio.sent().len();
    let _ = b.ticks(1000);
    let (_, split) = reads(&b, from);
    assert!(split > 0, "the split never read once the radio answered");
}

/// A dual-radio handoff makes another radio the active one, so the last radio's answer to "is the
/// split read native?" goes with it, as its `v` answer does.
#[test]
fn a_radio_handoff_asks_the_new_radio_whether_its_split_read_is_native() {
    let mut state = loop_state();
    state.split_detect = Some(crate::baud_ladder::SplitDetect::Native);
    state.vfo_read_native = Some(true);
    state.reset_for_handoff();
    assert_eq!(
        state.vfo_read_native, None,
        "premise: the handoff reset ran"
    );
    assert_eq!(state.split_detect, None, "the last radio's answer kept");
}

// ── Split Operation = Rig's TX dial put on the radio at a band change, while it receives ───────
// WSJT-X 3.0.2 writes the TX dial when the band changes, while it is only monitoring and whether
// or not TX is enabled: `MainWindow::band_changed` calls `setRig` and then `setXIT`
// (`widgets/mainwindow.cpp:12117-12118`), and `setXIT` sends `transceiver_tx_frequency` whenever
// split is on and it is monitoring (`:12378-12387`), through the same
// `HamlibTransceiver::do_tx_frequency` an over goes through (`Transceiver/HamlibTransceiver.cpp:
// 947-1033`). It writes nothing when the radio's own dial moves (`handle_transceiver_update`,
// `:12511-12545`). Nexus lets go of its split at a band change (the TX dial is the old band's) and
// takes the new band's TX dial at once, TX on or off, so the first over after TX is armed again
// finds VFO B there.

/// The 40 m FT8 dial, and the TX dial a 1441 Hz offset puts VFO B on there.
const FT8_40M_DIAL: u64 = 7_074_000;
const FT8_40M_TX_DIAL: u64 = 7_073_500;

/// The split, TX VFO and key writes among `writes`, in order.
fn split_writes(writes: &[String]) -> Vec<&str> {
    writes
        .iter()
        .map(String::as_str)
        .filter(|l| l.starts_with(['S', 'I', 'X', 'T']))
        .collect()
}

/// What the band change itself and the second after it write with the split held: the 20 m split
/// let go, then the 40 m TX dial and its mode taken, split on first and last.
const BAND_CHANGE_PRESET: [&str; 5] = [
    "S 0 VFOA",
    "S 1 VFOB",
    "I 7073500",
    "X PKTUSB -1",
    "S 1 VFOB",
];

impl<R: Radio> Bench<R> {
    /// Step to `at_ms` into the 30 s cycle (the bench's own slots start at 0), the next time it
    /// comes.
    fn step_to_cycle_ms(&mut self, at_ms: f64) -> Vec<String> {
        let mut writes = Vec::new();
        while !(at_ms..at_ms + 20.0).contains(&self.t.rem_euclid(30_000.0)) {
            writes.extend(self.ticks(1));
        }
        writes
    }

    /// The band changed to 40 m FT8 (which halts TX, as it does), with `at_change` done in the same
    /// moment; then a second of receiving. Returns the writes of both.
    fn qsy_to_40m(&mut self, at_change: impl FnOnce(&mut Engine)) -> Vec<String> {
        {
            let mut e = self.engine.lock().unwrap();
            e.set_frequency(7.074, "40m", "USB");
            at_change(&mut e);
        }
        self.ticks(50)
    }

    /// Call CQ with TX on, as the button does: the snappy first-over request is left pending, to
    /// key at once in this station's own slot or at its next boundary.
    fn call_cq_now(&mut self) {
        let mut e = self.engine.lock().unwrap();
        e.set_tx_enabled(true);
        e.start_cq(None).expect("Call CQ");
    }

    /// Up to `n` ticks, stopping on the one that keys; the writes.
    fn ticks_to_key(&mut self, n: usize) -> Vec<String> {
        let mut writes = Vec::new();
        for _ in 0..n {
            writes.extend(self.ticks(1));
            if self.radio.keyed() {
                break;
            }
        }
        writes
    }
}

/// A radio that takes 800 ms to move VFO B to another band, behind a link that waits for it.
fn slow_band_change_bench(split: SplitMode) -> Bench<Hamlib> {
    let mut b = hamlib_bench(split, 1441.0, None);
    b.rig.test_reply_deadline_ms = Some(2_000);
    b.radio.vfos.lock().unwrap().slow_band_change_ms = 800;
    b
}

/// A 20 m over with Rig split, then TX turned off (the split held, as WSJT-X holds it), on a radio
/// slow to move VFO B across bands.
fn held_and_tx_off() -> Bench<Hamlib> {
    let mut b = slow_band_change_bench(SplitMode::Rig);
    let _ = b.overs(1);
    b.engine.lock().unwrap().set_tx_enabled(false);
    assert_eq!(
        b.ticks(3),
        Vec::<String>::new(),
        "premise: TX off writes nothing"
    );
    assert!(b.radio.vfos.lock().unwrap().split, "premise: held");
    b
}

/// ⭐ THE BAND CHANGE'S FIRST OVER KEYS ON A RADIO SLOW TO MOVE VFO B, WHENEVER CALL CQ IS PRESSED.
/// The band changes 2 s into either period, TX off (the band change halts it in any case), and
/// the 40 m TX dial goes to the radio within the next second, while it receives. Call CQ a second
/// later leaves its first over pending as the button does: in the station's own period it keys at
/// once, in the other at the next own boundary. Either way VFO B is already on 40 m and the key
/// keys: no slow cross-band `I`, no refusal, TX still on.
/// Before, each of these keyed nothing and halted TX: the over wrote `S 1 VFOB, I 7073500` at its
/// key, took 800 ms, and was refused.
#[test]
fn a_band_change_in_either_period_puts_the_tx_dial_on_the_radio_and_the_called_over_keys() {
    for (what, at_ms) in [("own period", 2_000.0), ("other period", 17_000.0)] {
        let mut b = held_and_tx_off();
        let _ = b.step_to_cycle_ms(at_ms);
        let change = b.qsy_to_40m(|_| {});
        assert!(
            !b.engine.lock().unwrap().tx_enabled(),
            "{what}: premise: TX off"
        );
        assert_eq!(
            split_writes(&change),
            BAND_CHANGE_PRESET,
            "{what}: the 40 m TX dial not put on the radio at the band change"
        );
        b.call_cq_now();
        assert!(
            b.engine.lock().unwrap().peek_immediate_tx(),
            "{what}: premise: the first over pending"
        );
        assert_eq!(
            b.radio.vfos.lock().unwrap().b,
            FT8_40M_TX_DIAL,
            "{what}: VFO B not on 40 m before the over"
        );
        let key = b.ticks_to_key(800);
        assert!(b.radio.keyed(), "{what}: the over did not key: {key:?}");
        // Call CQ's own mode write (`M`) forgets what the radio confirmed for VFO B, so the key
        // may send the TX dial and mode again; VFO B is already on 40 m, so the radio takes them
        // at once and the key goes out inside its 500 ms.
        let key = split_writes(&key);
        assert_eq!(
            key[key.len() - 2..],
            ["S 1 VFOB", "T 1"],
            "{what}: the over's key: {key:?}"
        );
        assert!(
            key.iter()
                .all(|l| matches!(*l, "I 7073500" | "X PKTUSB -1" | "S 1 VFOB" | "T 1")),
            "{what}: the over's key: {key:?}"
        );
        assert!(b.engine.lock().unwrap().tx_enabled(), "{what}: TX halted");
        let v = b.radio.vfos.lock().unwrap();
        assert_eq!((v.split, v.b), (true, FT8_40M_TX_DIAL), "{what}");
        drop(v);
        assert_eq!(b.dial(), FT8_40M_DIAL, "{what}");
    }
}

/// ⛔ NOTHING IS PUT ON THE RADIO UNDER A CARRIER. The band changes on the tick an over keys: no
/// split, TX dial or mode write goes out while it is keyed; the band change ends the over, and the
/// 40 m TX dial goes after the unkey, while the station receives.
#[test]
fn a_band_change_during_an_over_is_put_on_the_radio_only_after_the_unkey() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    b.slot = (b.t / 30_000.0).ceil() * 30_000.0;
    let engine = b.engine.clone();
    let overs = b.overs_with(1, |_| {
        engine.lock().unwrap().set_frequency(7.074, "40m", "USB");
    });
    eprintln!("band change during the over: {:?}", overs[0]);
    let everywhere = b
        .radio
        .sent()
        .iter()
        .filter(|l| l.starts_with(['I', 'X']))
        .count();
    // The first over's I and X, and this one's recorded writes.
    let recorded = 2 + overs
        .iter()
        .flat_map(|o| o.key.iter().chain(&o.unkey).chain(&o.rx))
        .filter(|l| l.starts_with(['I', 'X']))
        .count();
    assert_eq!(
        everywhere,
        recorded,
        "a TX dial or mode written while the over was keyed: {:?}",
        b.radio.sent()
    );
    assert_eq!(
        split_writes(&overs[0].rx),
        BAND_CHANGE_PRESET,
        "the 40 m TX dial, after the unkey"
    );
}

/// ⛔ NOTHING IS PUT ON THE RADIO UNDER A TUNE CARRIER. The band changes and Tune is pressed in
/// the same moment: while the tune carrier is up nothing writes split, VFO B or its mode; the 40 m
/// TX dial goes once the tune ends.
#[test]
fn a_pre_set_due_during_a_tune_waits_for_the_tune_to_end() {
    let mut b = held_and_tx_off();
    let _ = b.step_to_cycle_ms(17_000.0);
    let tuning = b.qsy_to_40m(|e| e.set_tune(true));
    assert!(b.radio.keyed(), "premise: the tune keyed: {tuning:?}");
    assert!(
        !tuning
            .iter()
            .any(|l| l.starts_with("S 1") || l.starts_with('I') || l.starts_with('X')),
        "written under the tune carrier: {tuning:?}"
    );
    b.engine.lock().unwrap().set_tune(false);
    let after = b.ticks(100);
    assert!(!b.radio.keyed(), "premise: the tune ended");
    let at = after.iter().position(|l| l == "T 0");
    let preset = after.iter().position(|l| l == "I 7073500");
    assert!(
        matches!((at, preset), (Some(a), Some(p)) if p > a),
        "the TX dial not written after the tune ended: {after:?}"
    );
}

/// ⛔ NO PRE-SET INSIDE THE LAST SECONDS BEFORE A BOUNDARY. The band changes 3 s before the next
/// boundary: nothing goes until that boundary has passed, then the 40 m TX dial goes.
#[test]
fn a_band_change_just_before_a_boundary_pre_sets_after_it() {
    let mut b = held_and_tx_off();
    let _ = b.step_to_cycle_ms(12_000.0);
    let mut before = b.qsy_to_40m(|_| {});
    before.extend(b.step_to_cycle_ms(14_960.0));
    assert!(
        !before.iter().any(|l| l == "I 7073500"),
        "written inside the lead to the boundary: {before:?}"
    );
    let after = b.ticks(100);
    assert!(
        after.iter().any(|l| l == "I 7073500"),
        "not written after the boundary: {after:?}"
    );
}

/// The band button pressed twice in quick succession (40 m, then 30 m 100 ms later): one pre-set,
/// for the band it settled on.
#[test]
fn two_band_changes_inside_the_settle_pre_set_once_for_the_last() {
    let mut b = held_and_tx_off();
    let _ = b.step_to_cycle_ms(17_000.0);
    b.engine.lock().unwrap().set_frequency(7.074, "40m", "USB");
    let mut writes = b.ticks(5);
    b.engine.lock().unwrap().set_frequency(10.136, "30m", "USB");
    writes.extend(b.ticks(300));
    let tx_dials: Vec<&String> = writes.iter().filter(|l| l.starts_with("I ")).collect();
    assert_eq!(tx_dials, ["I 10135500"], "{writes:?}");
}

/// ⛔ THE RADIO'S KNOB WRITES NOTHING, AS IN WSJT-X. Split held on 20 m, TX on, receiving: ten dial
/// reads 100 Hz apart (the operator turning the knob) send no split, TX dial or mode write at all.
/// A band change then sends exactly one pre-set.
#[test]
fn a_turning_knob_pre_sets_nothing_and_a_band_change_pre_sets_once() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let _ = b.overs(1);
    let _ = b.step_to_cycle_ms(17_000.0);
    b.radio.vfos.lock().unwrap().knob = (1..=10).rev().map(|i| FT8_DIAL + 100 * i).collect();
    let spin = b.ticks(250);
    assert!(
        b.radio.vfos.lock().unwrap().knob.is_empty(),
        "premise: every knob step read"
    );
    assert_eq!(
        b.dial(),
        FT8_DIAL + 1_000,
        "premise: the dial followed the knob"
    );
    assert_eq!(
        split_writes(&spin),
        Vec::<&str>::new(),
        "written to the radio while the knob turned"
    );
    let _ = b.step_to_cycle_ms(17_000.0);
    let change = b.qsy_to_40m(|_| {});
    let more = b.ticks(250);
    let presets = change
        .iter()
        .chain(&more)
        .filter(|l| l.starts_with("I "))
        .count();
    assert_eq!(presets, 1, "{change:?} {more:?}");
}

/// ⛔ A CLUSTER SPLIT-ON-WORK OWNS VFO B: with one set at the band change, nothing of Split
/// Operation's is put on the radio. The control is the band change with none
/// (`a_band_change_in_either_period_puts_the_tx_dial_on_the_radio_and_the_called_over_keys`).
#[test]
fn a_cluster_split_on_work_keeps_vfo_b_from_the_pre_set() {
    let mut b = held_and_tx_off();
    let _ = b.step_to_cycle_ms(17_000.0);
    let mut rx = b.qsy_to_40m(|e| e.request_split(Some(7.080)));
    rx.extend(b.ticks(300));
    eprintln!("receive {rx:?}");
    assert!(
        !rx.iter().any(|l| l == "I 7073500" || l.starts_with("X ")),
        "Split Operation wrote VFO B under the cluster split: {rx:?}"
    );
    assert!(
        rx.iter().any(|l| l == "I 7080000"),
        "premise: the cluster split went to the radio: {rx:?}"
    );
    assert_eq!(
        b.radio.vfos.lock().unwrap().b,
        7_080_000,
        "VFO B moved off the cluster split"
    );
}

/// ⛔ A PRE-SET THE RADIO DOES NOT TAKE LEAVES THE KEY'S GUARD AS IT WAS. The radio refuses the TX
/// dial at the band change: it is tried once more, never a third time, and nothing is taken as
/// confirmed (the split the first try turned on is held). The radio then takes it, but slowly,
/// at the key: the over writes the TX dial again there, misses its 500 ms, and nothing keys, TX
/// halted.
#[test]
fn a_pre_set_the_radio_refuses_is_tried_once_more_and_the_key_still_guards() {
    let mut b = held_and_tx_off();
    let _ = b.step_to_cycle_ms(17_000.0);
    b.radio.vfos.lock().unwrap().refuse_i = true;
    let mut rx = b.qsy_to_40m(|_| {});
    rx.extend(b.ticks(300));
    eprintln!("receive {rx:?}");
    let tries = rx.iter().filter(|l| *l == "I 7073500").count();
    assert_eq!(tries, 2, "one try and one more: {rx:?}");
    assert_eq!(
        b.rig.split_confirmed, None,
        "a refused pre-set taken as confirmed"
    );
    b.radio.vfos.lock().unwrap().refuse_i = false;
    b.call_cq_now();
    let key = b.ticks(800);
    // The split the first try turned on is held, so the key's writes begin at the TX dial.
    let key = split_writes(&key);
    assert_eq!(key.first(), Some(&"I 7073500"), "the key's writes: {key:?}");
    assert!(
        !key.iter().any(|l| is_key(l)),
        "keyed past the budget: {key:?}"
    );
    assert!(!b.engine.lock().unwrap().tx_enabled(), "TX not halted");
}

/// ⛔ NO PRE-SET WHILE A SPLIT RELEASE IS OWED. The radio refuses the 20 m split's release at the
/// band change: while it is owed, nothing turns split on again or writes VFO B (the release
/// retries are all that goes out). The control: once the radio takes the release, the 40 m TX
/// dial goes.
#[test]
fn no_pre_set_goes_while_a_split_release_is_owed() {
    let mut b = held_and_tx_off();
    let _ = b.step_to_cycle_ms(17_000.0);
    b.radio.vfos.lock().unwrap().refuse_split_off = true;
    let mut owed = b.qsy_to_40m(|_| {});
    owed.extend(b.ticks(300));
    assert!(b.rig.split_release_pending, "premise: owed");
    assert!(
        split_writes(&owed).iter().all(|l| *l == "S 0 VFOA"),
        "written while the release is owed: {owed:?}"
    );
    b.radio.vfos.lock().unwrap().refuse_split_off = false;
    let after = b.ticks(1_500);
    assert!(
        after.iter().any(|l| l == "I 7073500"),
        "CONTROL: the release taken, the TX dial goes: {after:?}"
    );
}

/// ⛔ NO PRE-SET OFF THE DIGITAL SCREENS. On the Phone screens with TX armed and the tier still FT8,
/// a band change writes no split, TX dial or mode: a phone transmission must never go out of
/// VFO B. The control is the same band change on the digital screens.
#[test]
fn a_band_change_on_the_phone_screens_puts_nothing_on_vfo_b() {
    for (what, section, presets) in [
        ("Phone", "phone", false),
        ("control: Digital", "digital", true),
    ] {
        let mut b = held_and_tx_off();
        b.engine.lock().unwrap().set_operating_mode(section, false);
        b.engine.lock().unwrap().set_tx_enabled(true);
        let _ = b.step_to_cycle_ms(17_000.0);
        let mut rx = b.qsy_to_40m(|e| e.set_tx_enabled(true));
        rx.extend(b.ticks(300));
        let wrote = split_writes(&rx)
            .into_iter()
            .any(|l| l.starts_with("S 1") || l.starts_with('I') || l.starts_with('X'));
        assert_eq!(wrote, presets, "{what}: {rx:?}");
    }
}

/// ⛔ NO PRE-SET ON A RADIO THIS LOOP HAS NOT BEEN HANDED: the active radio switched and the loop
/// has not taken it over yet. Nothing goes to VFO B; the control, once the loop holds the radio
/// again, is the pre-set.
#[test]
fn no_pre_set_goes_to_a_radio_the_loop_does_not_hold() {
    let mut b = held_and_tx_off();
    let _ = b.step_to_cycle_ms(17_000.0);
    b.state.handed_over = Some(u32::MAX);
    let mut rx = b.qsy_to_40m(|_| {});
    rx.extend(b.ticks(200));
    assert!(
        !rx.iter().any(|l| l == "I 7073500" || l.starts_with("X ")),
        "written to a radio not handed over: {rx:?}"
    );
    b.state.handed_over = None;
    let after = b.ticks(1_500);
    assert!(
        after.iter().any(|l| l == "I 7073500"),
        "CONTROL: held again, the TX dial goes: {after:?}"
    );
}

/// Fake It and None move nothing at a band change, as before: no split, TX dial or mode write.
/// And Split Operation = Rig in a mode it does not apply to (Nexus's own fast tier) pre-sets
/// nothing either.
#[test]
fn fake_it_none_and_a_mode_without_split_put_nothing_on_the_radio_after_a_band_change() {
    type Setup = fn(&mut Engine);
    let cases: [(&str, SplitMode, Setup); 3] = [
        ("Fake It", SplitMode::FakeIt, |_| {}),
        ("None", SplitMode::None, |_| {}),
        ("Rig, fast tier", SplitMode::Rig, |e| {
            e.set_tier(Tier::TempoFast)
        }),
    ];
    for (what, split, setup) in cases {
        let mut b = slow_band_change_bench(split);
        let _ = b.overs(1);
        setup(&mut b.engine.lock().unwrap());
        let _ = b.step_to_cycle_ms(17_000.0);
        let mut rx = b.qsy_to_40m(|_| {});
        rx.extend(b.ticks(300));
        let wrote: Vec<&str> = split_writes(&rx)
            .into_iter()
            .filter(|l| !l.starts_with('T') && *l != "S 0 VFOA")
            .collect();
        assert_eq!(wrote, Vec::<&str>::new(), "{what}: {rx:?}");
    }
}

// ── What stands between a held split and a key out of VFO B ──────────────────────────────────
// TX off no longer lets go of the split, so these are what still does, or what keeps VFO B from
// being written.

/// ⛔ SWITCHING RADIO THROUGH THE POOL LETS GO OF THE SPLIT THROUGH THE OUTGOING LINK. Split held
/// with TX off, the operator picks a radio the pool already holds: the old radio gets `S 0 VFOA`
/// before the loop adopts the new one, and is left out of split. Otherwise a later key on the old
/// radio, the hand mic or a phone over, would go out of VFO B on the last TX dial.
#[test]
fn switching_radio_through_the_pool_with_split_held_lets_go_through_the_outgoing_link() {
    let mut b = held_and_tx_off();
    let _ = b.step_to_cycle_ms(17_000.0);
    let (cat, _, _) = mock_logging_rigctld();
    let (pool, id) = {
        let mut e = b.engine.lock().unwrap();
        let id = e.add_radio();
        let profile = e.settings().radios.iter().find(|p| p.id == id).unwrap();
        let monitor = live_monitor(id, Transport::from_profile(profile), &cat);
        let pool: MonitorPool = Arc::new(MonitorConnections::new(vec![monitor]));
        (pool, id)
    };
    b.engine.lock().unwrap().set_active_radio(id);
    let before = b.radio.sent().len();
    let mut last_active = 0u32;
    let pending = AtomicBool::new(false);
    handoff_if_switched(
        &b.engine,
        &pool,
        &mut b.rig,
        &mut b.state,
        &mut last_active,
        &pending,
    );
    assert_eq!(last_active, id, "premise: switched");
    assert!(
        pool.lock().unwrap().iter().any(|c| c.id == 0),
        "premise: the old radio's link went to the pool, not closed"
    );
    let writes: Vec<String> = b.radio.sent()[before..]
        .iter()
        .filter(|l| !l.ends_with('?'))
        .cloned()
        .collect();
    assert!(
        writes.iter().any(|l| l == "S 0 VFOA"),
        "not let go: {writes:?}"
    );
    assert!(
        !b.radio.vfos.lock().unwrap().split,
        "the outgoing radio left in split: {writes:?}"
    );
}

/// ⛔ A CLUSTER SPLIT-ON-WORK ENDS THE HOLD, SO NOTHING OF SPLIT OPERATION'S TOUCHES ITS VFO B. Set
/// while Split Operation's split is held, with TX off: the cluster's TX dial goes on VFO B, and a
/// tune then keys with nothing written, leaving it there. If the hold stood, the tune would put VFO
/// B back on the RX dial for Split Operation and overwrite the cluster's frequency.
#[test]
fn a_cluster_split_on_work_set_while_held_is_not_overwritten_by_a_tune() {
    let mut b = held_and_tx_off();
    let _ = b.step_to_cycle_ms(17_000.0);
    b.engine.lock().unwrap().request_split(Some(14.080));
    let set = b.ticks(100);
    // Read out of its lock first: a failed assert holding it poisons it for the mock's thread.
    let vfo_b = b.radio.vfos.lock().unwrap().b;
    assert_eq!(
        vfo_b, 14_080_000,
        "premise: the cluster split went to the radio: {set:?}"
    );
    b.engine.lock().unwrap().set_tune(true);
    let tune = b.ticks(5);
    assert!(b.radio.keyed(), "premise: the tune keyed: {tune:?}");
    let vfo_b = b.radio.vfos.lock().unwrap().b;
    assert_eq!(
        vfo_b, 14_080_000,
        "VFO B moved off the cluster split: {tune:?}"
    );
}

/// ⛔ NO PRE-SET ON A TICK THAT OPENED THE CAT LINK: opening it may already have cost a connect
/// timeout out of the lead to the next boundary. The pre-set is held off across the band change,
/// then the link is dropped as it comes due: the tick that reopens it writes no TX dial, and the
/// next one does.
#[test]
fn a_tick_that_reopens_the_cat_link_does_not_pre_set_and_the_next_does() {
    let mut b = held_and_tx_off();
    let _ = b.step_to_cycle_ms(17_000.0);
    b.state.handed_over = Some(u32::MAX);
    let _ = b.qsy_to_40m(|_| {});
    let _ = b.ticks(50);
    b.state.handed_over = None;
    b.rig.reconnect_for_test();
    assert!(!b.rig.connection_open(), "premise: the link closed");
    let reopening = b.ticks(1);
    assert!(b.rig.connection_open(), "premise: the tick reopened it");
    assert!(
        !reopening.iter().any(|l| l == "I 7073500"),
        "pre-set on the tick that reopened the link: {reopening:?}"
    );
    let next = b.ticks(1);
    assert!(
        next.iter().any(|l| l == "I 7073500"),
        "CONTROL: the next tick pre-sets: {next:?}"
    );
}

// ── A link that does not take the DATA word ──────────────────────────────────────────────────

/// What a rigctld does with a DATA word (`M PKT…`/`X PKT…`), as Hamlib's backends were seen to.
#[derive(Clone, Copy)]
enum DataAnswer {
    /// Refused (`RPRT -9`): a backend with no case for the word (Ten-Tec, Alinco, the FT-857's
    /// `PKTLSB`), which Hamlib refuses itself.
    Refuses,
    /// Taken (`RPRT 0`), but the radio stays in USB and reads back USB: SDR Console (2056), whose
    /// `MDD;` Hamlib reports as taken whether SDR Console answers it `?;` or not
    /// (`tests/rigctld_sdrconsole.rs`).
    Ignores,
    /// Taken, and read back as this: `PKTUSB` on an emulation answering ID904, piHPSDR, the
    /// QCX/QDX (Hamlib's `MD9;` = DIGU) and an Icom with a data flag (USB-D); `RTTYR` on the
    /// FT-1000MP, whose `PKTUSB` is RTTY-USB.
    Takes(&'static str),
}

/// FT8 CQ on 20 m against a rigctld whose radio starts in USB and answers a DATA word `answer`'s
/// way. Split writes are taken. The loop's transport is already this radio's, `model`.
fn data_word_bench(split: SplitMode, model: u32, answer: DataAnswer) -> Bench<Hamlib> {
    let engine = cq_engine(split, 1441.0, |s| s.rig_model = model);
    let keyed = Arc::new(AtomicBool::new(false));
    let vfos = Arc::new(Mutex::new(Vfos::default()));
    let k = keyed.clone();
    let peer = retuning_peer(FT8_DIAL, "USB", move |line, radio| {
        let ok = || Some("RPRT 0\n".to_string());
        let data = (line.starts_with("M ") || line.starts_with("X ")) && line.contains("PKT");
        match line {
            "T 1" | "T 3" => {
                radio.keyed = true;
                k.store(true, Ordering::SeqCst);
                ok()
            }
            "T 0" => {
                radio.keyed = false;
                k.store(false, Ordering::SeqCst);
                ok()
            }
            _ if data => match answer {
                DataAnswer::Refuses => Some("RPRT -9\n".to_string()),
                DataAnswer::Ignores => ok(),
                DataAnswer::Takes(reads) => {
                    if line.starts_with("M ") {
                        radio.mode = reads.to_string();
                    }
                    ok()
                }
            },
            _ if line.starts_with("S ") || line.starts_with("I ") || line.starts_with("X ") => ok(),
            "i" => Some(format!("{FT8_DIAL}\n")),
            _ => None,
        }
    });
    let rig = Rig::rigctld(&peer.address);
    let mut state = loop_state();
    state.applied.rig_model = model;
    Bench::new(engine, state, rig, Hamlib { peer, keyed, vfos })
}

/// Has the engine been told this link does not take `word`?
fn refused(b: &Bench<Hamlib>, model: u32, word: &str) -> bool {
    b.engine
        .lock()
        .unwrap()
        .settings()
        .link_refusals
        .refused(model, word)
}

/// Every mode and TX-mode write the radio was sent, in order.
fn mode_writes(b: &Bench<Hamlib>) -> Vec<String> {
    b.radio
        .peer
        .lines
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with("M ") || l.starts_with("X "))
        .cloned()
        .collect()
}

/// A LINK THAT REFUSES THE DATA WORD. The loop's ladder gives up on it while receiving and puts
/// the radio in USB, as it always did, and from then on this connection's word IS USB: no over is
/// keyed after another DATA word. Before, the key-time assert sent `M PKTUSB` again at every slot
/// boundary, just ahead of `TX;`. Every over keys at its slot.
#[test]
fn ft8_on_a_link_that_refuses_the_data_word_keys_every_over_in_usb() {
    const MODEL: u32 = 2056;
    let mut b = data_word_bench(SplitMode::None, MODEL, DataAnswer::Refuses);
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("refusing link, over {}: {o:?}", i + 1);
        assert_eq!(
            o.key.last().map(String::as_str),
            Some("T 1"),
            "over {}",
            i + 1
        );
        assert!(
            o.key.iter().all(|l| !l.contains("PKT")),
            "over {}: a DATA word on the key tick: {:?}",
            i + 1,
            o.key
        );
    }
    let modes = mode_writes(&b);
    let usb = modes
        .iter()
        .position(|l| l == "M USB -1")
        .expect("put in USB");
    assert!(
        modes[usb..].iter().all(|l| !l.contains("PKT")),
        "a DATA word after the link refused it: {modes:?}"
    );
    assert!(refused(&b, MODEL, "PKTUSB"));
}

/// …and with Split Operation = Rig, VFO B's mode is USB too. Before, the refused `X PKTUSB` halted
/// TX, so no over went out at all. The first over's key-time mode assert goes out where it always
/// did (the ladder's fallback never latches it); only its word is USB now.
#[test]
fn rig_split_on_a_link_that_refuses_the_data_word_writes_vfo_b_in_usb() {
    let mut b = data_word_bench(SplitMode::Rig, 990_301, DataAnswer::Refuses);
    let overs = b.overs(2);
    assert_eq!(
        overs[0].key,
        [
            "M USB -1",
            "S 1 VFOB",
            "I 14073500",
            "X USB -1",
            "S 1 VFOB",
            "T 1"
        ]
    );
    assert_eq!(overs[1].key, ["S 1 VFOB", "T 1"]);
}

/// ⭐ SDR CONSOLE (2056): Hamlib takes `PKTUSB` (`RPRT 0`) and sends `MDD;`, the radio never moves,
/// and it reads plain USB after EVERY set Nexus makes. One watch is never a verdict on its own (the
/// operator may have pressed USB inside it, delta R1), so Nexus sets the word again while receiving,
/// and the second consecutive plain watch condemns it: then the word is USB for the connection.
/// Every over still keys at its slot, and none is keyed behind a DATA word: each set is made while
/// receiving, and SDR Console stays in USB throughout. Before, `PKTUSB` was the word for good and
/// went out at every key.
#[test]
fn ft8_on_a_link_that_silently_ignores_the_data_word_moves_to_usb_while_receiving() {
    const MODEL: u32 = 990_302;
    let mut b = data_word_bench(SplitMode::None, MODEL, DataAnswer::Ignores);
    let overs = b.overs(4);
    for (i, o) in overs.iter().enumerate() {
        assert_eq!(
            o.key.last().map(String::as_str),
            Some("T 1"),
            "over {}",
            i + 1
        );
        assert!(o.key.iter().all(|l| !l.contains("PKT")), "over {}", i + 1);
    }
    eprintln!("silent link, mode writes: {:?}", mode_writes(&b));
    assert_eq!(
        mode_writes(&b),
        ["M PKTUSB -1", "M PKTUSB -1", "M USB -1"],
        "the set, the set again after one plain watch, then USB after the second"
    );
    assert!(refused(&b, MODEL, "PKTUSB"));
}

/// CONTROLS: a link whose radio TAKES the DATA word keeps it, however Hamlib's list reads.
/// Read back as `PKTUSB` (an emulation answering ID904, piHPSDR, the QCX/QDX: Hamlib's `MD9;`, DIGU;
/// an Icom's USB-D) or as another mode entirely (`RTTYR`, the FT-1000MP's RTTY-USB): the word stays
/// `PKTUSB` for every over, as today.
#[test]
fn a_link_that_takes_the_data_word_keeps_it() {
    for (model, reads) in [(990_303, "PKTUSB"), (990_304, "RTTYR")] {
        let mut b = data_word_bench(SplitMode::None, model, DataAnswer::Takes(reads));
        let _ = b.overs(2);
        assert_eq!(mode_writes(&b), ["M PKTUSB -1"], "reads back {reads}");
        assert!(!refused(&b, model, "PKTUSB"), "reads back {reads}");
    }
}

/// The judgement itself (TX re-review F1, delta D1). Only the reads after NEXUS'S OWN DATA set are
/// judged: the read on which the set is first seen is skipped (Hamlib's cache), as is any inside
/// `DATA_CACHE_MS`; the word read back latches it taken for the connection; a watch whose two reads
/// are the plain twin is NOT a verdict on its own: Nexus sets the word again (`last_mode` cleared for
/// the retune), and only two consecutive such watches condemn it; a watch whose set crossed a band
/// counts for nothing, and a band crossing inside a watch closes it; any other mode closes it; the
/// TS-590S/SG and TS-950 are never judged (F3); and a word that is not a DATA word opens no watch.
#[test]
fn the_data_word_is_judged_only_after_nexuss_own_set_and_latched_once_taken() {
    const T0: f64 = 1_000_000.0;
    /// One watch after Nexus's set number `writes` of `word`: `crossed_at_set` = that set crossed a
    /// band. Returns whether the watch asked for the word to be set again.
    fn watch(
        state: &mut RadioLoop,
        engine: &Mutex<Engine>,
        writes: u64,
        word: &str,
        crossed_at_set: bool,
        reads: &[(f64, &str)],
    ) -> bool {
        state.last_mode = word.into();
        if crossed_at_set {
            state.dial_band_crosses += 1;
        }
        let t = T0 + writes as f64 * 100_000.0;
        state.judge_data_word(engine, writes, t, Some(word)); // the set is seen: skipped
        for (at, read) in reads {
            state.judge_data_word(engine, writes, t + at, Some(read));
        }
        state.last_mode.is_empty()
    }
    let fresh = |model: u32| {
        let mut state = loop_state();
        state.applied.rig_model = model;
        (state, Mutex::new(Engine::new("KD9TAW", "EN52", 0)))
    };
    let refused = |engine: &Mutex<Engine>, model: u32, word: &str| {
        engine
            .lock()
            .unwrap()
            .settings()
            .link_refusals
            .refused(model, word)
    };
    let twice = [(3_000.0, "USB"), (6_000.0, "USB")];

    // SDR Console: plain USB after every one of Nexus's sets. The first watch asks for the set
    // again, the second condemns.
    let (mut st, eng) = fresh(2056);
    assert!(
        watch(&mut st, &eng, 1, "PKTUSB", false, &twice),
        "set it again"
    );
    assert!(!refused(&eng, 2056, "PKTUSB"), "one watch is not a verdict");
    watch(&mut st, &eng, 2, "PKTUSB", false, &twice);
    assert!(refused(&eng, 2056, "PKTUSB"), "two consecutive watches are");

    // R1: the operator's USB inside the first watch; the radio takes the word set again: latched.
    let (mut st, eng) = fresh(990_501);
    assert!(watch(&mut st, &eng, 1, "PKTUSB", false, &twice));
    watch(&mut st, &eng, 2, "PKTUSB", false, &[(3_000.0, "PKTUSB")]);
    assert!(!refused(&eng, 990_501, "PKTUSB") && st.data_taken == ["PKTUSB"]);
    watch(&mut st, &eng, 3, "PKTUSB", false, &twice);
    watch(&mut st, &eng, 4, "PKTUSB", false, &twice);
    assert!(
        !refused(&eng, 990_501, "PKTUSB"),
        "latched: never judged again"
    );

    // R2: the first set crossed a band (the band stack's USB behind Hamlib's cache): that watch
    // counts for nothing, so it takes two clean ones after it.
    let (mut st, eng) = fresh(990_502);
    assert!(
        watch(&mut st, &eng, 1, "PKTUSB", true, &twice),
        "set it again"
    );
    watch(&mut st, &eng, 2, "PKTUSB", false, &twice);
    assert!(
        !refused(&eng, 990_502, "PKTUSB"),
        "only one clean watch so far"
    );
    watch(&mut st, &eng, 3, "PKTUSB", false, &twice);
    assert!(refused(&eng, 990_502, "PKTUSB"));

    // A band crossing inside a watch closes it: no verdict, no set again.
    let (mut st, eng) = fresh(990_503);
    st.last_mode = "PKTUSB".into();
    st.judge_data_word(&eng, 1, T0, Some("PKTUSB"));
    st.judge_data_word(&eng, 1, T0 + 3_000.0, Some("USB"));
    st.dial_band_crosses += 1;
    st.judge_data_word(&eng, 1, T0 + 6_000.0, Some("USB"));
    assert!(st.data_watch.is_none() && !st.last_mode.is_empty());

    // The FT-1000MP's DATA-LSB reads PKTLSB: taken; later plain LSB changes nothing.
    let (mut st, eng) = fresh(1024);
    watch(
        &mut st,
        &eng,
        1,
        "PKTLSB",
        false,
        &[(3_000.0, "PKTLSB"), (6_000.0, "LSB"), (9_000.0, "LSB")],
    );
    assert!(!refused(&eng, 1024, "PKTLSB") && st.data_taken == ["PKTLSB"]);

    // A watch that judged nothing in between breaks the run: one plain watch, then another mode,
    // then one plain watch is not two consecutive.
    let (mut st, eng) = fresh(990_504);
    watch(&mut st, &eng, 1, "PKTUSB", false, &twice);
    watch(&mut st, &eng, 2, "PKTUSB", false, &[(3_000.0, "CW")]);
    watch(&mut st, &eng, 3, "PKTUSB", false, &twice);
    assert!(!refused(&eng, 990_504, "PKTUSB"));

    // One plain read is not two; reads inside the cache window are not judged.
    for reads in [
        &[(3_000.0, "USB")][..],
        &[(300.0, "USB"), (600.0, "USB")][..],
    ] {
        let (mut st, eng) = fresh(2056);
        assert!(
            !watch(&mut st, &eng, 1, "PKTUSB", false, reads),
            "{reads:?}"
        );
    }
    // F3: the TS-590S/SG and TS-950 read back through another field.
    for model in [2012, 2013, 2031, 2037] {
        let (mut st, eng) = fresh(model);
        assert!(!watch(&mut st, &eng, 1, "PKTUSB", false, &twice), "{model}");
        watch(&mut st, &eng, 2, "PKTUSB", false, &twice);
        assert!(!refused(&eng, model, "PKTUSB"), "{model}");
    }
    // Not a DATA word: no watch.
    let (mut st, _eng) = fresh(2056);
    assert!(!watch(&mut st, &_eng, 1, "USB", false, &twice));
}

/// ⛔ A NEW CONNECTION IS JUDGED AFRESH. What the last link showed is forgotten when the loop opens
/// the link, and the word is asked again in the same tick, so the new link is sent the DATA word
/// and judged on its own answers.
#[test]
fn the_tick_that_opens_a_link_forgets_the_last_links_judgement() {
    const MODEL: u32 = 2056;
    let engine = cq_engine(SplitMode::None, 1441.0, |s| s.rig_model = MODEL);
    engine
        .lock()
        .unwrap()
        .refuse_link_data_word(MODEL, "PKTUSB");
    assert_eq!(
        engine.lock().unwrap().rig_mode_effective(),
        "USB",
        "premise"
    );
    let peer = retuning_peer(FT8_DIAL, "USB", |_, _| None);
    let address = peer.address.clone();
    let mut state = loop_state();
    state.rx_ranges_probed = true;
    state.tuner_probed = true;
    let mut ra = mock_reopen_audio();
    let mut rr = move |_t: &Transport, _coexist: bool| {
        (
            Rig::rigctld(&address),
            None,
            CatProbe::status(Some(true), ""),
        )
    };
    let (mut backend, mut rig, mut station) = (MockBackend::new(), Rig::vox(), StationSinks::new());
    let sinks = Sinks {
        wsjtx: None,
        psk: None,
    };
    state
        .step(
            &engine,
            &mut backend,
            &mut rig,
            &sinks,
            now_unix_ms() + 2_000.0,
            &mut ra,
            &mut rr,
            &mut station,
        )
        .unwrap();
    let modes: Vec<String> = peer
        .lines
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with("M "))
        .cloned()
        .collect();
    assert_eq!(modes, ["M PKTUSB -1"], "the opening tick's mode write");
}

// ── TX re-review scenes (2026-11-08): a radio that TAKES the DATA word is never condemned ────

/// A rigctld whose radio TAKES the DATA word (reads back what it was told) and lets the test
/// change its mode behind the loop's back (the operator's front panel, or a band-stack recall).
/// `errs`: every `M`/`X` answers `RPRT -1` although the radio applies it (a link whose post-set
/// step fails, the Kenwood IF-length trap). Records the radio's mode at every key.
fn review_scene(
    model: u32,
    errs: bool,
    flip_at: Option<f64>,
    qsy_at: Option<f64>,
) -> (Vec<String>, Vec<String>) {
    review_scene_at(model, errs, flip_at, None, qsy_at)
}

/// [`review_scene`], the flip also moving the radio's dial to `flip_dial` (a band change on the
/// radio's own panel, its band stack recalling USB there).
fn review_scene_at(
    model: u32,
    errs: bool,
    flip_at: Option<f64>,
    flip_dial: Option<u64>,
    qsy_at: Option<f64>,
) -> (Vec<String>, Vec<String>) {
    let engine = cq_engine(SplitMode::None, 1441.0, |s| s.rig_model = model);
    let panel = Arc::new(AtomicBool::new(false));
    let at_key = Arc::new(Mutex::new(Vec::<String>::new()));
    let (p, k) = (panel.clone(), at_key.clone());
    let peer = retuning_peer(FT8_DIAL, "USB", move |line, radio| {
        if p.swap(false, Ordering::SeqCst) {
            radio.mode = "USB".into();
            if let Some(hz) = flip_dial {
                radio.dial = hz;
            }
        }
        let ok = || Some("RPRT 0\n".to_string());
        match line {
            "T 1" | "T 3" => {
                k.lock().unwrap().push(radio.mode.clone());
                radio.keyed = true;
                ok()
            }
            "T 0" => {
                radio.keyed = false;
                ok()
            }
            _ if line.starts_with("M ") => {
                radio.mode = line.split_whitespace().nth(1).unwrap().to_string();
                if errs {
                    Some("RPRT -1\n".into())
                } else {
                    ok()
                }
            }
            _ if line.starts_with("X ") => {
                if errs {
                    Some("RPRT -1\n".into())
                } else {
                    ok()
                }
            }
            _ if line.starts_with("S ") || line.starts_with("I ") => ok(),
            "i" => Some(format!("{FT8_DIAL}\n")),
            _ => None,
        }
    });
    let mut rig = Rig::rigctld(&peer.address);
    let mut state = loop_state();
    state.applied.rig_model = model;
    state.rx_ranges_probed = true;
    state.tuner_probed = true;
    let sinks = Sinks {
        wsjtx: None,
        psk: None,
    };
    let mut ra = mock_reopen_audio();
    let mut rr = move |_t: &Transport, _c: bool| (Rig::vox(), None, CatProbe::status(None, ""));
    let mut station = StationSinks::new();
    let mut backend = MockBackend::new();
    let slot = ((now_unix_ms() + 25_000.0) / 30_000.0).ceil() * 30_000.0;
    let mut t = slot - 14_900.0;
    let mut mark = 0;
    let mut flipped = false;
    let mut qsyd = false;
    let end = slot + 75_000.0;
    while t < end {
        if let Some(f) = flip_at {
            if !flipped && t >= slot + f {
                flipped = true;
                panel.store(true, Ordering::SeqCst);
                mark = peer.lines.lock().unwrap().len();
            }
        }
        if let Some(q) = qsy_at {
            if !qsyd && t >= slot + q {
                qsyd = true;
                let mut e = engine.lock().unwrap();
                e.set_frequency(14.0745, "20m", "USB"); // the operator clicks a spot: a QSY
                e.start_cq(None).expect("CQ again");
                let _ = e.take_immediate_tx();
            }
        }
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        t += 20.0;
    }
    let writes: Vec<String> = peer.lines.lock().unwrap()[mark..]
        .iter()
        .filter(|l| l.starts_with("M ") || l.starts_with("X "))
        .cloned()
        .collect();
    let keys = at_key.lock().unwrap().clone();
    eprintln!("REVIEW model {model} errs {errs}: mode writes after the flip {writes:?}; radio mode at each key {keys:?}");
    (writes, keys)
}

/// F-A: a radio that TAKES PKTUSB. The operator turns it to USB on the front panel (or a band-stack
/// recall does) during receive, and ~12 s later clicks a spot (QSY) and calls CQ again. The radio has
/// shown it takes the DATA word on this connection, so the over after the click must key in PKTUSB.
#[test]
fn review_front_panel_usb_then_a_spot_click_keys_in_the_data_word() {
    let (writes, keys) = review_scene(990_401, false, Some(16_000.0), Some(28_000.0));
    assert!(keys.len() >= 2, "premise: overs keyed: {keys:?}");
    assert_eq!(
        keys[0], "PKTUSB",
        "premise: the first over went out in the DATA word"
    );
    assert!(
        !writes.iter().any(|l| l.starts_with("M USB")),
        "a radio that took PKTUSB was commanded plain USB after a front-panel change: {writes:?}"
    );
    assert_eq!(
        keys.last().map(String::as_str),
        Some("PKTUSB"),
        "keyed in: {keys:?}"
    );
}

/// F-B: a link whose every mode set answers RPRT -1 although the radio applies it. The radio TAKES
/// PKTUSB on the wire; every over should key in it.
#[test]
fn review_a_link_that_errors_every_set_still_keys_in_what_the_radio_takes() {
    let (_writes, keys) = review_scene(990_402, true, None, None);
    assert!(keys.len() >= 2, "premise: overs keyed: {keys:?}");
    assert!(keys.iter().all(|m| m == "PKTUSB"), "keyed in: {keys:?}");
}

/// Q3: Split = Rig on a SILENT link (SDR Console): how many overs carry `X PKTUSB` before the verdict?
#[test]
fn review_rig_split_on_a_silent_link_sends_x_pkt_on_how_many_overs() {
    let mut b = data_word_bench(SplitMode::Rig, 990_405, DataAnswer::Ignores);
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!(
            "REVIEW split silent over {}: key {:?} rx-modes {:?}",
            i + 1,
            o.key,
            o.rx.iter()
                .filter(|l| l.starts_with("M ") || l.starts_with("X "))
                .collect::<Vec<_>>()
        );
    }
    let n = overs
        .iter()
        .filter(|o| o.key.iter().any(|l| l.contains("PKT")))
        .count();
    eprintln!("REVIEW overs keyed behind a DATA word: {n}");
    assert!(n <= 1, "{n}");
}

/// F-A, the band-stack variant: the operator changes band ON THE RADIO (40 m), its band stack
/// recalls USB there, and ~12 s later a spot click brings it back to 20 m FT8. The radio showed it
/// takes PKTUSB on this connection, so the over after the click keys in it.
#[test]
fn review_a_band_stack_usb_then_a_spot_click_keys_in_the_data_word() {
    let (writes, keys) = review_scene_at(
        990_403,
        false,
        Some(16_000.0),
        Some(7_074_000),
        Some(28_000.0),
    );
    assert!(keys.len() >= 2, "premise: overs keyed: {keys:?}");
    assert_eq!(
        keys[0], "PKTUSB",
        "premise: the first over went out in the DATA word"
    );
    assert!(
        !writes.iter().any(|l| l.starts_with("M USB")),
        "a radio that took PKTUSB was commanded plain USB after a band-stack recall: {writes:?}"
    );
    assert_eq!(
        keys.last().map(String::as_str),
        Some("PKTUSB"),
        "keyed in: {keys:?}"
    );
}

/// F3: a TS-590SG on a link that reads back plain USB after `PKTUSB` (Hamlib sets its DATA mode
/// through `SF` and may read the base mode there) is never judged by the read-back: today's word,
/// every over.
#[test]
fn a_ts590_is_never_read_back_judged() {
    let mut b = data_word_bench(SplitMode::None, 2037, DataAnswer::Ignores);
    let _ = b.overs(2);
    assert_eq!(mode_writes(&b), ["M PKTUSB -1"]);
    assert!(!refused(&b, 2037, "PKTUSB"));
}

/// ⛔ A RADIO HANDOFF IS JUDGED AFRESH, with no tick of the old verdict (F5): the loop's handoff
/// runs ahead of `step`, and the step's first engine lock forgets before it works out the word.
#[test]
fn a_radio_handoff_forgets_the_last_links_judgement_before_the_first_mode_write() {
    const MODEL: u32 = 2056;
    let engine = cq_engine(SplitMode::None, 1441.0, |s| s.rig_model = MODEL);
    engine
        .lock()
        .unwrap()
        .refuse_link_data_word(MODEL, "PKTUSB");
    let peer = retuning_peer(FT8_DIAL, "USB", |_, _| None);
    let mut rig = Rig::rigctld(&peer.address);
    let mut state = loop_state();
    state.applied.rig_model = MODEL;
    state.rx_ranges_probed = true;
    state.tuner_probed = true;
    state.data_taken.push("PKTLSB");
    state.reset_for_handoff(); // what `install_handoff_connection` runs
    assert!(
        state.data_taken.is_empty(),
        "nothing the last radio showed carries over"
    );
    let (mut ra, mut rr) = (mock_reopen_audio(), mock_reopen_rig());
    let (mut backend, mut station) = (MockBackend::new(), StationSinks::new());
    let sinks = Sinks {
        wsjtx: None,
        psk: None,
    };
    state
        .step(
            &engine,
            &mut backend,
            &mut rig,
            &sinks,
            now_unix_ms() + 2_000.0,
            &mut ra,
            &mut rr,
            &mut station,
        )
        .unwrap();
    let modes: Vec<String> = peer
        .lines
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with("M "))
        .cloned()
        .collect();
    assert_eq!(
        modes.first().map(String::as_str),
        Some("M PKTUSB -1"),
        "{modes:?}"
    );
}

/// R1 (delta review): the front-panel USB lands INSIDE the first watch, ~1 s after the loop's first
/// DATA set on this connection and before any read latched "taken". Then a spot click at +28 s.
#[test]
fn review_residual_front_panel_inside_the_first_watch() {
    let (writes, keys) = review_scene(990_406, false, Some(-13_900.0), Some(28_000.0));
    eprintln!("REVIEW residual writes {writes:?} keys {keys:?}");
    assert_eq!(
        keys.last().map(String::as_str),
        Some("PKTUSB"),
        "keyed in: {keys:?}"
    );
}

/// R2 (delta review): the connection's FIRST DATA set crosses a band. The radio sits on 40 m in USB;
/// Nexus restores 20 m FT8 (`M PKTUSB`, then `F 14074000`). The radio TAKES PKTUSB, but its band
/// stack then recalls USB for 20 m. Hamlib answers the re-assert's immediate `m` from its 500 ms
/// mode cache (measured on the wire, 4.6.5: no `MD;` sent, `PKTUSB` returned while the radio is in
/// `MD2`), so the re-assert sees nothing to fix. Later real reads say USB. At +28 s the operator
/// clicks a spot (no band cross): the over after it must key in PKTUSB, as on the base.
#[test]
fn review_r2_first_set_crossing_a_band_stack_radio() {
    let model = 990_407;
    let engine = cq_engine(SplitMode::None, 1441.0, |s| s.rig_model = model);
    let at_key = Arc::new(Mutex::new(Vec::<String>::new()));
    let cache: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let (k, c) = (at_key.clone(), cache.clone());
    let peer = retuning_peer(7_074_000, "USB", move |line, radio| {
        let ok = || Some("RPRT 0\n".to_string());
        match line {
            "T 1" | "T 3" => {
                k.lock().unwrap().push(radio.mode.clone());
                radio.keyed = true;
                ok()
            }
            "T 0" => {
                radio.keyed = false;
                ok()
            }
            "m" => c.lock().unwrap().take().map(|m| format!("{m}\n2400\n")), // Hamlib's mode cache, once
            _ if line.starts_with("M ") => {
                let m = line.split_whitespace().nth(1).unwrap().to_string();
                radio.mode = m.clone();
                *c.lock().unwrap() = Some(m);
                ok()
            }
            _ if line.starts_with("F ") => {
                let hz: u64 = line[2..].trim().parse().unwrap();
                if hz / 1_000_000 != radio.dial / 1_000_000 {
                    radio.mode = "USB".into(); // band stack recall
                }
                radio.dial = hz;
                ok()
            }
            _ if line.starts_with("S ") || line.starts_with("I ") || line.starts_with("X ") => ok(),
            "i" => Some(format!("{}\n", radio.dial)),
            _ => None,
        }
    });
    let mut rig = Rig::rigctld(&peer.address);
    let mut state = loop_state();
    state.applied.rig_model = model;
    state.rx_ranges_probed = true;
    state.tuner_probed = true;
    let sinks = Sinks {
        wsjtx: None,
        psk: None,
    };
    let mut ra = mock_reopen_audio();
    let mut rr = move |_t: &Transport, _c: bool| (Rig::vox(), None, CatProbe::status(None, ""));
    let mut station = StationSinks::new();
    let mut backend = MockBackend::new();
    let slot = ((now_unix_ms() + 25_000.0) / 30_000.0).ceil() * 30_000.0;
    let mut t = slot - 14_900.0;
    let mut qsyd = false;
    while t < slot + 75_000.0 {
        if !qsyd && t >= slot + 28_000.0 {
            qsyd = true;
            let mut e = engine.lock().unwrap();
            e.set_frequency(14.0745, "20m", "USB");
            e.start_cq(None).expect("CQ again");
            let _ = e.take_immediate_tx();
        }
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        t += 20.0;
    }
    let writes: Vec<String> = peer
        .lines
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with("M ") || l.starts_with("F "))
        .cloned()
        .collect();
    let keys = at_key.lock().unwrap().clone();
    eprintln!("REVIEW R2 writes {writes:?} keys {keys:?}");
    assert!(keys.len() >= 2, "premise: overs keyed: {keys:?}");
    assert_eq!(
        keys.last().map(String::as_str),
        Some("PKTUSB"),
        "keyed in: {keys:?}"
    );
}
