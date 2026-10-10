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
            cfg_dial_hz: FT8_DIAL,
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

/// An IC-7610 on Nexus's CI-V daemon, Split Operation = Rig: before every key `0F 01` (split on)
/// and `25 01` (the SUB band's dial, by name, on this radio) at the TX dial; after every unkey
/// `0F 00` (split off). The same TX dial every over, MAIN never written, nothing written while it
/// receives, the engine's dial unmoved. At 1441 Hz the step is -500 Hz; at 941 Hz, -1000 Hz.
///
/// No mode reaches the TX VFO: the loop's `X PKTUSB -1` is refused by the daemon, whose mode names
/// (`commands::Mode::from_name`) have no DATA submode, so nothing goes on the wire for it.
#[test]
fn an_ic7610_in_rig_split_gets_the_same_tx_dial_every_over_and_split_off_after_each() {
    for (tx_hz, tx_dial) in [(1441.0, 14_073_500u64), (941.0, 14_073_000)] {
        let mut b = civ_bench(0x98, IcomModel::Ic7610, 3078, SplitMode::Rig, tx_hz);
        let overs = b.overs(3);
        for (i, o) in overs.iter().enumerate() {
            eprintln!("IC-7610 Rig {tx_hz} Hz over {}: {o:?}", i + 1);
        }
        let key = vec![
            "0F 01".to_string(),
            format!("25 01 {tx_dial}"),
            "1C 00 01".to_string(),
        ];
        let unkey = vec!["1C 00 00".to_string(), "0F 00".to_string()];
        for (i, o) in overs.iter().enumerate() {
            let what = format!("{tx_hz} Hz, over {}", i + 1);
            assert_eq!(
                o.dial, FT8_DIAL,
                "{what}: the dial the shift was taken from"
            );
            assert_eq!(o.key, key, "{what}: what goes out before the key");
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
            (FT8_DIAL, tx_dial, false),
            "{tx_hz} Hz"
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
            "s" => Some(format!("{}\nVFOB\n", u8::from(v.split))),
            "i" => Some(format!("{}\n", v.b)),
            "f" => {
                let hz = v.stale.take().unwrap_or(radio.dial);
                if hz != FT8_DIAL {
                    v.reads_off_the_channel += 1;
                }
                Some(format!("{hz}\n"))
            }
            _ if line.starts_with("S ") => {
                v.split = line.split_whitespace().nth(1) == Some("1");
                ok()
            }
            _ if line.starts_with("I ") => {
                v.b = line[2..].trim().parse().unwrap();
                ok()
            }
            _ if line.starts_with("X ") => ok(),
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

/// Through Hamlib, Split Operation = Rig: before every key `S 1 VFOB`, `I` at the TX dial and
/// `X PKTUSB -1`; after every unkey `S 0 VFOA`. VFO A is never written; VFO B holds the TX dial.
#[test]
fn through_hamlib_rig_split_is_engaged_at_every_key_and_released_at_every_unkey() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, None);
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("Hamlib Rig over {}: {o:?}", i + 1);
        assert_eq!(o.dial, FT8_DIAL, "over {}", i + 1);
        assert_eq!(
            o.key,
            ["S 1 VFOB", "I 14073500", "X PKTUSB -1", "T 1"],
            "over {}",
            i + 1
        );
        assert_eq!(o.unkey, ["T 0", "S 0 VFOA"], "over {}", i + 1);
        assert_eq!(o.rx, Vec::<String>::new(), "over {}", i + 1);
    }
    let v = b.radio.vfos.lock().unwrap();
    assert_eq!((v.split, v.b), (false, 14_073_500));
    drop(v);
    assert_eq!(b.dial(), FT8_DIAL);
}

/// Through Hamlib, Split Operation = Rig, on a radio that was ALREADY in split before Nexus
/// transmitted, VFO B on the dial (a quick split): the receive-time split read takes that as the
/// operator's own split, and after every over the teardown puts it back. So VFO B goes to the TX
/// dial for each over and back to the dial after it, every cycle, where WSJT-X leaves it on the TX
/// dial.
#[test]
fn through_hamlib_a_radio_already_in_split_gets_its_own_vfo_b_back_after_every_over() {
    let mut b = hamlib_bench(SplitMode::Rig, 1441.0, Some(FT8_DIAL));
    let overs = b.overs(3);
    for (i, o) in overs.iter().enumerate() {
        eprintln!("Hamlib Rig, radio's own split on, over {}: {o:?}", i + 1);
        assert_eq!(
            o.key,
            ["S 1 VFOB", "I 14073500", "X PKTUSB -1", "T 1"],
            "over {}",
            i + 1
        );
        assert_eq!(o.unkey, ["T 0", "S 1 VFOB", "I 14074000"], "over {}", i + 1);
        assert_eq!(o.rx, Vec::<String>::new(), "over {}", i + 1);
    }
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
