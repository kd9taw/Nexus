//! The CI-V serial engine — ONE thread owns the CI-V byte stream and multiplexes three
//! traffics over it:
//!
//! 1. **Command/reply** — requests arrive on a channel ([`CivHandle::transact`]), are written
//!    to the port strictly one at a time, and the matching reply (or `FB`/`FA` ack) resolves
//!    the caller. A per-request serial deadline keeps a dead radio from wedging the queue.
//! 2. **Unsolicited transceive** — the radio pushes frequency (`00`) / mode (`01`) reports
//!    when the operator touches the front panel; they fold into the shared [`CivState`]
//!    (instant dial tracking, no polling).
//! 3. **Scope waveform** (`27`) — routed to the [`ScopeAssembler`]; each completed sweep
//!    lands in a latest-wins slot the radio loop drains into the waterfall.
//!
//! The engine is generic over [`CivIo`] (`Read + Write`), so the whole protocol path is
//! unit-tested against an in-memory fake radio — only the constructor that opens a real
//! serial port needs the `serial` feature (see [`super::broker`]).

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::frame::{Frame, FrameSplitter};
use super::scope::{scope_stream_frames, ScopeAssembler, ScopeSweep};
use super::state::CivState;

/// The byte transport the engine drives: a real serial port in production (both
/// `serialport`'s port types and our test fakes implement `Read + Write`). Reads must
/// TIME OUT rather than block forever (`ErrorKind::TimedOut`/`WouldBlock` = "no data
/// yet") — the engine's loop interleaves reads with the command queue.
pub trait CivIo: Read + Write + Send {}
impl<T: Read + Write + Send> CivIo for T {}

/// How long the engine waits on the wire for one request's reply before failing it.
/// CI-V at 115200 answers in ~10–20 ms; even 19200 stays well under this.
const REQUEST_DEADLINE: Duration = Duration::from_millis(300);
/// Read chunk timeout the loop expects `CivIo` reads to observe (the real port is opened
/// with this; the loop just treats timeouts as "no data").
pub const READ_TIMEOUT: Duration = Duration::from_millis(30);

/// What reply resolves a request.
#[derive(Debug, Clone, Copy)]
pub enum Expect {
    /// A set command → bare `FB` (ok) / `FA` (rejected).
    Ack,
    /// A read → a frame with this command byte (and this first data byte, for
    /// sub-commanded reads like `15 02`).
    Reply { cmd: u8, sub: Option<u8> },
    /// A read sent in Icom's BAND-DIRECTED form (`29 <band> <cmd> <sub>…`, see
    /// `commands::band_directed`) → the reply naming the same band and command.
    ///
    /// Icom's text says only that an ACK/NAK drops the `29 <band>` prefix (IC-7610 CI-V
    /// Reference Guide A7380-7EX-4, p. 15). A data reply is taken WITH the prefix, as that
    /// sentence implies, and also without it — so a wording ambiguity can never become a meter
    /// that never moves on a real radio. The bare shape is safe to take: the bus carries one
    /// request at a time, so a bare `<cmd> <sub>` reply arriving while this is pending is this
    /// request's answer. A prefixed reply naming the OTHER band is never taken.
    ReplyOnBand { band: u8, cmd: u8, sub: Option<u8> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CivError {
    /// No matching reply within the deadline (radio off / wrong address / wrong baud).
    Timeout,
    /// The radio rejected the command (`FA`).
    Nak,
    /// The engine thread is gone.
    Gone,
}

struct CivRequest {
    frame: Frame,
    expect: Expect,
    /// `None` for the engine's own housekeeping commands (scope enable/disable): they
    /// still occupy the pending slot — every write MUST, or their acks would resolve a
    /// later caller's command on the half-duplex bus — but nobody awaits the result.
    reply_to: Option<mpsc::SyncSender<Result<Frame, CivError>>>,
}

impl CivRequest {
    fn resolve(self, r: Result<Frame, CivError>) {
        if let Some(tx) = self.reply_to {
            let _ = tx.try_send(r);
        }
    }
}

/// Cloneable client handle to the engine: transact commands, read the live state.
#[derive(Clone)]
pub struct CivHandle {
    tx: mpsc::Sender<CivRequest>,
    state: Arc<Mutex<CivState>>,
    alive: Arc<AtomicBool>,
}

impl CivHandle {
    /// Send one CI-V command and wait for its reply/ack. Serialized with every other
    /// caller — the engine owns the half-duplex bus.
    pub fn transact(&self, frame: Frame, expect: Expect) -> Result<Frame, CivError> {
        // Fail in microseconds when the engine thread is dead — a wedged daemon must
        // never serialize callers behind full recv timeouts (the UI-hang convoy).
        if !self.alive.load(Ordering::Relaxed) {
            return Err(CivError::Gone);
        }
        let (rtx, rrx) = mpsc::sync_channel(1);
        self.tx
            .send(CivRequest {
                frame,
                expect,
                reply_to: Some(rtx),
            })
            .map_err(|_| CivError::Gone)?;
        // The engine enforces REQUEST_DEADLINE per request; the extra headroom here covers
        // requests queued behind others.
        rrx.recv_timeout(REQUEST_DEADLINE * 4 + Duration::from_millis(100))
            .map_err(|_| CivError::Timeout)?
    }

    /// A snapshot of the live state (freq/mode/PTT/meters folded from replies + transceive).
    pub fn state(&self) -> CivState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// The running engine. Dropping it stops the thread (and the port closes with it).
pub struct CivEngine {
    handle: CivHandle,
    scope_row: Arc<Mutex<Option<ScopeSweep>>>,
    scope_enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CivEngine {
    /// Start the engine on `io`, talking to the radio at CI-V address `radio_addr`.
    pub fn start(io: Box<dyn CivIo>, radio_addr: u8) -> CivEngine {
        let (tx, rx) = mpsc::channel::<CivRequest>();
        let state = Arc::new(Mutex::new(CivState::default()));
        let scope_row = Arc::new(Mutex::new(None));
        let scope_enabled = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let thread = {
            let state = state.clone();
            let scope_row = scope_row.clone();
            let scope_enabled = scope_enabled.clone();
            let stop = stop.clone();
            let alive = alive.clone();
            std::thread::Builder::new()
                .name("civ-engine".into())
                .spawn(move || {
                    engine_loop(
                        io,
                        radio_addr,
                        rx,
                        state,
                        scope_row,
                        scope_enabled,
                        stop,
                        alive,
                    )
                })
                .expect("spawn civ-engine")
        };
        CivEngine {
            handle: CivHandle {
                tx,
                state,
                alive: alive.clone(),
            },
            scope_row,
            scope_enabled,
            stop,
            alive,
            thread: Some(thread),
        }
    }

    pub fn handle(&self) -> CivHandle {
        self.handle.clone()
    }

    /// Take the newest completed scope sweep, if one arrived since the last take.
    pub fn take_scope_row(&self) -> Option<ScopeSweep> {
        self.scope_row.lock().ok().and_then(|mut s| s.take())
    }

    /// Enable/disable the radio's scope waveform stream. The engine sends the CI-V
    /// enable/disable commands on the transition (idempotent per state).
    pub fn set_scope_enabled(&self, on: bool) {
        self.scope_enabled.store(on, Ordering::Relaxed);
    }

    /// False once the engine thread has exited (I/O error — port unplugged/denied).
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
}

impl Drop for CivEngine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// One frame write with bounded retries for transient stalls.
enum WriteOutcome {
    Ok,
    /// Timed out repeatedly — the request fails, the engine lives.
    Transient,
    /// Hard I/O error — the port is gone.
    Fatal,
}

fn write_frame(io: &mut Box<dyn CivIo>, frame: &Frame) -> WriteOutcome {
    let bytes = frame.to_bytes();
    super::diag::log(super::diag::Dir::Tx, &bytes);
    for _ in 0..3 {
        match io.write_all(&bytes).and_then(|_| io.flush()) {
            Ok(()) => return WriteOutcome::Ok,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {} // retry
            Err(_) => return WriteOutcome::Fatal,
        }
    }
    WriteOutcome::Transient
}

/// True when `f` resolves a request expecting `expect`. `FA` rejects both kinds.
fn resolves(expect: Expect, f: &Frame) -> Option<Result<Frame, CivError>> {
    if f.is_nak() {
        return Some(Err(CivError::Nak));
    }
    match expect {
        Expect::Ack => f.is_ack().then(|| Ok(f.clone())),
        Expect::Reply { cmd, sub } => {
            let sub_ok = sub.is_none_or(|s| f.data.first() == Some(&s));
            (f.cmd == cmd && sub_ok).then(|| Ok(f.clone()))
        }
        Expect::ReplyOnBand { band, cmd, sub } => {
            let d = &f.data;
            let prefixed = f.cmd == super::commands::BAND_DIRECTED
                && d.first() == Some(&band)
                && d.get(1) == Some(&cmd)
                && sub.is_none_or(|s| d.get(2) == Some(&s));
            let bare = f.cmd == cmd && sub.is_none_or(|s| d.first() == Some(&s));
            (prefixed || bare).then(|| Ok(f.clone()))
        }
    }
}

#[allow(clippy::too_many_arguments)] // one private loop, one call site
fn engine_loop(
    mut io: Box<dyn CivIo>,
    radio_addr: u8,
    rx: mpsc::Receiver<CivRequest>,
    state: Arc<Mutex<CivState>>,
    scope_row: Arc<Mutex<Option<ScopeSweep>>>,
    scope_enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
) {
    let mut splitter = FrameSplitter::new();
    let mut assembler = ScopeAssembler::new();
    let mut pending: Option<(CivRequest, Instant)> = None;
    // The engine's own housekeeping commands, queued ahead of caller traffic. They flow
    // through the SAME pending slot as user requests — every write must, or their acks
    // would resolve a later caller's command on the half-duplex bus.
    let mut internal: std::collections::VecDeque<CivRequest> = std::collections::VecDeque::new();
    let mut scope_sent: Option<bool> = None; // last commanded waveform-output state
    let mut buf = [0u8; 512];
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        // Keep the radio's waveform-output state in sync with the wanted flag.
        let want_scope = scope_enabled.load(Ordering::Relaxed);
        if scope_sent != Some(want_scope) {
            for f in scope_stream_frames(radio_addr, want_scope) {
                internal.push_back(CivRequest {
                    frame: f,
                    expect: Expect::Ack,
                    reply_to: None,
                });
            }
            scope_sent = Some(want_scope);
        }
        // Start the next queued request when idle (housekeeping first, then callers).
        if pending.is_none() {
            let next = internal.pop_front().map(Ok).unwrap_or_else(|| {
                rx.try_recv().map_err(|e| match e {
                    mpsc::TryRecvError::Empty => false,
                    mpsc::TryRecvError::Disconnected => true,
                })
            });
            match next {
                Ok(req) => {
                    // A transient write stall (USB hiccup) fails THIS REQUEST, never the
                    // engine — killing the engine over one stall would take down all
                    // native CAT including the ability to unkey a keyed radio.
                    match write_frame(&mut io, &req.frame) {
                        WriteOutcome::Ok => {
                            pending = Some((req, Instant::now() + REQUEST_DEADLINE));
                        }
                        WriteOutcome::Transient => {
                            req.resolve(Err(CivError::Timeout));
                        }
                        WriteOutcome::Fatal => {
                            req.resolve(Err(CivError::Gone));
                            break; // port gone for real
                        }
                    }
                }
                Err(true) => break, // all handles dropped
                Err(false) => {}
            }
        }
        // Read whatever arrived (short timeout keeps the loop responsive).
        let frames = match io.read(&mut buf) {
            Ok(0) => {
                // A pipe-like fake returns Ok(0) at EOF; a serial port never does. Treat
                // as "no data" so tests can drain, but yield so we don't spin.
                std::thread::sleep(Duration::from_millis(1));
                Vec::new()
            }
            Ok(n) => {
                super::diag::log(super::diag::Dir::Rx, &buf[..n]);
                splitter.push(&buf[..n])
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                Vec::new()
            }
            Err(_) => break, // hard I/O error — port unplugged
        };
        for f in frames {
            // Scope WAVEFORM frames go to the assembler, never to request matching.
            //
            // ⚠️ THE SUB-COMMAND IS PART OF THE TEST, and leaving it out meant NO `27`-family READ
            // could ever resolve. `27 00` is the waveform (`parse_waveform` requires data[0]==0x00);
            // `27 14`, `27 15` and the rest are ordinary command replies. This arm used to swallow
            // EVERY `27` frame and `continue`, so a reply to a scope read never reached request
            // matching and `Expect::Reply { cmd: 0x27, .. }` was unsatisfiable by construction.
            // Nothing noticed because every `27` call in the tree writes and expects an Ack (`FB`),
            // which is a different command byte and slipped past. Found while adding the first
            // `27` READ — the scope centre/fixed mode, so a refused span stops being explained by
            // a guess.
            if f.cmd == 0x27 && f.data.first() == Some(&0x00) {
                if let Some(sweep) = assembler.push(&f) {
                    if let Ok(mut slot) = scope_row.lock() {
                        *slot = Some(sweep); // latest wins
                    }
                }
                continue;
            }
            // Everything else refreshes the live state (replies AND transceive pushes).
            if let Ok(mut s) = state.lock() {
                s.apply(&f);
            }
            // Resolve the in-flight request if this frame answers it.
            if let Some((req, _)) = &pending {
                if let Some(result) = resolves(req.expect, &f) {
                    let (req, _) = pending.take().unwrap();
                    req.resolve(result);
                }
            }
        }
        // Fail a request the radio never answered.
        if let Some((_, deadline)) = &pending {
            if Instant::now() > *deadline {
                let (req, _) = pending.take().unwrap();
                req.resolve(Err(CivError::Timeout));
            }
        }
    }
    // Fail everything still queued so callers unblock immediately.
    if let Some((req, _)) = pending.take() {
        req.resolve(Err(CivError::Gone));
    }
    while let Ok(req) = rx.try_recv() {
        req.resolve(Err(CivError::Gone));
    }
    alive.store(false, Ordering::Relaxed);
}

/// The in-memory fake IC-9700 the engine + daemon tests drive — kept out of `mod tests`
/// so the broker's end-to-end test reuses it.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::super::frame::{bcd_to_freq, freq_to_bcd, Frame, CONTROLLER};
    use std::collections::VecDeque;
    use std::io::{self, Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// The fake rig's REGISTER FILE, shared out via [`FakeRadio::regs`] so a
    /// test can assert what the wire ACTUALLY did after the radio has been
    /// moved into the engine. Models the IC-9700's dual-band reality: Main and
    /// Sub each hold a frequency and a mode; `07 D0/D1` moves the selection;
    /// `03/05/04/06` act on the SELECTED band; and — deliberately — `25 01`
    /// writes the unselected VFO *of the current band*, a register nothing else
    /// reads, because that is exactly where the parity batch's uplink went.
    #[derive(Debug)]
    pub struct Regs {
        pub main_hz: u64,
        pub sub_hz: u64,
        /// Selected band: false = Main, true = Sub (`07 D0`/`07 D1`).
        pub sel_sub: bool,
        /// Satellite mode (`16 5A`) — TX fixed on Sub, full duplex.
        pub satmode: bool,
        /// Same-band A/B split (`0F 01`) — cross-band on a 9700 is NOT this.
        pub split: bool,
        /// The unselected VFO of the CURRENT band (`25 01`). See above.
        pub unselected_hz: u64,
        /// CI-V mode bytes per band (`06` on the selected band).
        pub main_mode: u8,
        pub sub_mode: u8,
        /// The mode of the UNSELECTED VFO of the current band (`26 01`) — its
        /// OWN register, which is the whole point: `06` cannot reach it, so a
        /// split TX VFO keeps whatever mode was last put here.
        pub unselected_mode: u8,
        /// DATA mode (`1A 06`) — the rear-jack/soundcard flag that turns USB into USB-D and
        /// FM into FM-D. Separate from the mode byte on a real Icom, and separate here, which
        /// is the whole point: "FM" and "FM-D" differ ONLY in this bit.
        pub data_mode: bool,
        /// NAK `16 5A` like a single-band rig (IC-7300) — honest "no such mode".
        pub no_satmode: bool,
        /// Fault injection — NAK the next N Main selects (`07 D0`): the
        /// failed-restore case, which strands the selection on Sub.
        pub nak_main_select: u32,
        /// Fault injection — NAK the next N `16 5A` SETs (a satmode change
        /// the rig refuses to take).
        pub nak_satmode_set: u32,
        /// Fault injection — swallow the next N `16 5A` READ replies: a lost
        /// CI-V reply (the request times out while the rig's state stands).
        pub drop_satmode_reads: u32,
        /// Scope CENTER/FIXED position (`27 14`): false = Center, true = Fixed. Modelled so a
        /// scope SPAN (`27 15`) can be REFUSED — issue #275 reports an IC-7300 rejecting a span
        /// while its scope is in Fixed mode, and a fixture that acks everything cannot exercise
        /// what Nexus does with a refusal. The rule here is the field report's, not a reading of
        /// Icom's CI-V document; what the tests assert is the handling of a NAK.
        pub scope_fixed: bool,
        /// ATTENUATOR (`0x11`) — THE RAW WIRE BYTE, not a decoded dB, deliberately. The
        /// real register is BCD decibels, so a fixture that decoded on the way in would
        /// accept a raw-hex encoder and hand a test back the number it asked for: 12 dB
        /// would store 12 whether the wire said `0x12` or `0x0c`. Keeping the byte makes
        /// the encoding itself assertable.
        pub att_raw: u8,
        /// THE `0x16` REGISTER FILE, sub-command → its byte. One map rather than a field
        /// per control, because that is what the family is on a real Icom: NB (`22`), NR
        /// (`40`), ANF (`41`), COMP (`44`), MON (`45`), VOX (`46`), MN (`48`), the PREAMP
        /// selector (`02`) and AGC (`12`) are all "read the sub-command, write the
        /// sub-command plus a byte".
        ///
        /// ⚠️ It holds a BYTE, not a bool. Most of these are on/off, but the preamp is a
        /// 3-state POSITION and AGC is a speed enum, and a fixture that stored `bool` would
        /// quietly turn "select preamp 2" into "preamp on" — and then read back a 1 where
        /// the radio would have said 2. Satellite mode (`5A`) keeps its own field above: it
        /// has fault injection this map has no place for.
        pub funcs: std::collections::BTreeMap<u8, u8>,
        /// THE `0x14` LEVEL FILE, sub-command → the level as 0..255, decoded from the
        /// 2-byte BCD the wire carries: RFPOWER (`0A`), MICGAIN (`0B`), NR (`06`), COMP
        /// (`0E`), MONITOR_GAIN (`15`), AF/RF/SQL (`01`/`02`/`03`).
        ///
        /// ⚠️ THE BCD IS DECODED HERE BY HAND, on purpose. Calling the encoder's own
        /// [`super::super::commands`] helper would make every round trip tautological — a
        /// transposed nibble would encode and decode symmetrically and the test would pass.
        /// A second, independent implementation is what makes the read-back evidence.
        pub levels: std::collections::BTreeMap<u8, u16>,
        /// ⭐ THE SUB BAND'S OWN RECEIVE REGISTERS. `levels`, `funcs`, `att_raw` and
        /// `smeter_raw` above are the MAIN band's; these are the Sub's. Two files, not one,
        /// because a single register file cannot tell "the command acted on Main" from
        /// "it acted on Sub" — a read would come back with the same number either way.
        ///
        /// A plain (unaddressed) `0x14`/`0x15 02`/`0x11`/`0x16` command acts on the band
        /// that is SELECTED when it arrives (`sel_sub`); a band-directed one (`29 <band>
        /// …`) on the band it names. ⚠️ That the IC-9700 applies these to its selected band
        /// is this fixture's MODEL, taken from Icom's own wording ("Some functions can only
        /// be applied to the selected band", IC-9700 Basic Manual, "Selecting the Main and
        /// Sub bands") — not a bench measurement. NEEDS-BENCH, like everything here.
        pub sub_levels: std::collections::BTreeMap<u8, u16>,
        pub sub_funcs: std::collections::BTreeMap<u8, u8>,
        pub sub_att_raw: u8,
        /// Raw 0..255 S-meter per band (`15 02`). Main defaults to 120 (S9, what every
        /// existing test expects); Sub to a DIFFERENT value, so a reading taken from the
        /// wrong receiver is visible by its number rather than by luck.
        pub smeter_raw: u16,
        pub sub_smeter_raw: u16,
        /// Does this radio answer Icom's BAND-DIRECTED command `29` ("regardless of
        /// active/inactive the Main or Sub band, you can directly specify the Main or Sub
        /// band", IC-7610 CI-V Reference Guide A7380-7EX-4, p. 9)? True only at the IC-7610's
        /// address: the IC-9700's reference (A7508-3EX-4) has no command `29` at all, so
        /// at `0xA2` the fixture refuses it the way that table says the rig would.
        pub cmd29: bool,
        /// Does this radio read and write a band's dial and mode BY NAME — `25 <band>` /
        /// `26 <band>` with `00` = MAIN and `01` = SUB, whichever band is selected (IC-7610 CI-V
        /// Reference Guide A7380-7EX-4, p. 13)? True only at the IC-7610's address. The
        /// IC-9700's `25`/`26` name the SELECTED or UNSELECTED VFO instead (A7508-3EX-4,
        /// p. 24): its reads are not modelled and are NAKed here, because nothing sends them to
        /// that radio, and its `25 01` / `26 01` writes land in the unselected-VFO registers.
        pub dial_by_band: bool,
        /// Fault injection — swallow the next N by-name dial/mode READ replies (`25`/`26`
        /// above): a lost CI-V reply, so the read times out while the rig's state stands.
        pub drop_dial_reads: u32,
        /// Which band each command ACTED on, in arrival order: `(on_sub, cmd, data)`. A
        /// `29`-wrapped command is recorded UNWRAPPED, under the band it named; every other
        /// command under the selection at the moment it arrived. This is the witness for
        /// "the command acted on Main" — `log` below says what was SENT, this says where
        /// it LANDED.
        pub acted: Vec<(bool, u8, Vec<u8>)>,
        /// Every controller frame exactly as it arrived on the wire, `FE FE … FD`. The
        /// byte-for-byte witness the single-receiver identity test compares against.
        pub wire: Vec<Vec<u8>>,
        /// Every command frame received, as (cmd, data) — lets a test assert a
        /// verb was NOT sent (e.g. "no `0F` under the satellite-mode contract").
        pub log: Vec<(u8, Vec<u8>)>,
    }

    /// The (command, sub-command) pairs Icom's IC-7610 CI-V Reference Guide (A7380-7EX-4,
    /// Sep. 2025, pp. 3–4 and 8) marks "Command 29 supported", for the band-directed
    /// handler below. `None` = a command with no sub-command, whose whole data is the value
    /// (the attenuator, `11`).
    ///
    /// ⚠️ Deliberately a SECOND transcription, typed separately from the broker's table in
    /// `commands`: a fixture that asked the code under test which commands it may wrap
    /// would agree with any mistake that code made.
    const IC7610_CMD29: &[(u8, Option<u8>)] = &[
        (0x11, None),
        (0x12, Some(0x00)),
        (0x12, Some(0x01)),
        (0x14, Some(0x01)),
        (0x14, Some(0x02)),
        (0x14, Some(0x03)),
        (0x14, Some(0x05)),
        (0x14, Some(0x06)),
        (0x14, Some(0x07)),
        (0x14, Some(0x08)),
        (0x14, Some(0x0D)),
        (0x14, Some(0x12)),
        (0x14, Some(0x13)),
        (0x15, Some(0x01)),
        (0x15, Some(0x02)),
        (0x15, Some(0x05)),
        (0x15, Some(0x07)),
        (0x16, Some(0x02)),
        (0x16, Some(0x12)),
        (0x16, Some(0x22)),
        (0x16, Some(0x32)),
        (0x16, Some(0x40)),
        (0x16, Some(0x41)),
        (0x16, Some(0x42)),
        (0x16, Some(0x43)),
        (0x16, Some(0x48)),
        (0x16, Some(0x4E)),
        (0x16, Some(0x4F)),
        (0x16, Some(0x53)),
        (0x16, Some(0x56)),
        (0x16, Some(0x57)),
        (0x16, Some(0x65)),
        (0x1A, Some(0x03)),
        (0x1A, Some(0x04)),
        (0x1A, Some(0x09)),
        (0x1A, Some(0x0A)),
        (0x1B, Some(0x00)),
        (0x1B, Some(0x01)),
    ];

    /// What the fake radio does with ONE command — `cmd` + `data` acting on the Main band
    /// (`on_sub` false) or the Sub band. A plain frame acts on whichever band is selected when
    /// it arrives; a band-directed one ([`band_directed`]) on the band it names. Returns the
    /// reply to send: `None` = ack, `Some((0xFA, _))` = NAK, [`SILENT`] = say nothing.
    fn act(r: &mut Regs, addr: u8, cmd: u8, data: &[u8], on_sub: bool) -> Option<(u8, Vec<u8>)> {
        r.acted.push((on_sub, cmd, data.to_vec()));
        match (cmd, data.first().copied()) {
            (0x03, _) => {
                let hz = if r.sel_sub { r.sub_hz } else { r.main_hz };
                Some((0x03, freq_to_bcd(hz).to_vec()))
            }
            (0x05, _) => {
                let hz = bcd_to_freq(data);
                if r.sel_sub {
                    r.sub_hz = hz;
                } else {
                    r.main_hz = hz;
                }
                None // ack
            }
            (0x04, _) => {
                let m = if r.sel_sub { r.sub_mode } else { r.main_mode };
                Some((0x04, vec![m, 0x01]))
            }
            (0x06, _) => {
                let m = data[0];
                if r.sel_sub {
                    r.sub_mode = m;
                } else {
                    r.main_mode = m;
                }
                None
            }
            // [MAIN/SUB] band selection; the A/B forms just ack.
            (0x07, Some(0xD0)) => {
                if r.nak_main_select > 0 {
                    r.nak_main_select -= 1;
                    Some((0xFA, Vec::new()))
                } else {
                    r.sel_sub = false;
                    None
                }
            }
            (0x07, Some(0xD1)) => {
                r.sel_sub = true;
                None
            }
            (0x07, Some(0x00 | 0x01)) => None,
            // Same-band split / duplex — the 9700 accepts these.
            (0x0F, Some(v @ (0x00 | 0x01))) => {
                r.split = v != 0;
                None
            }
            (0x0F, _) => None, // duplex shift
            // A band's dial or mode BY NAME (`25 <band>` / `26 <band>`): the IC-7610's form, `00`
            // MAIN / `01` SUB whichever band is selected (A7380-7EX-4 p. 13). No value reads, and
            // the reply names the band back; a value writes that band. See [`Regs::dial_by_band`].
            (0x25 | 0x26, Some(band @ (0x00 | 0x01))) if r.dial_by_band => {
                let sub = band == 0x01;
                // Recorded under the band it NAMED, like a `29`-wrapped command — the entry
                // pushed above assumed the selection, which this command does not use.
                if let Some(last) = r.acted.last_mut() {
                    last.0 = sub;
                }
                match (cmd, data.len()) {
                    (_, 1) if r.drop_dial_reads > 0 => {
                        r.drop_dial_reads -= 1;
                        Some((SILENT, Vec::new()))
                    }
                    (0x25, 1) => {
                        let mut d = vec![band];
                        d.extend_from_slice(&freq_to_bcd(if sub { r.sub_hz } else { r.main_hz }));
                        Some((0x25, d))
                    }
                    (0x26, 1) => {
                        let m = if sub { r.sub_mode } else { r.main_mode };
                        Some((0x26, vec![band, m, u8::from(r.data_mode), 0x01]))
                    }
                    (0x25, n) if n >= 6 => {
                        let hz = bcd_to_freq(&data[1..6]);
                        if sub {
                            r.sub_hz = hz;
                        } else {
                            r.main_hz = hz;
                        }
                        None // ack
                    }
                    // `26 <band> <mode> [<data> <filter>]`. A skipped DATA byte is "DATA OFF"
                    // (p. 13); the fixture keeps one DATA register, like the `1A 06` arm below,
                    // and does not model the filter.
                    (0x26, _) => {
                        if sub {
                            r.sub_mode = data[1];
                        } else {
                            r.main_mode = data[1];
                        }
                        r.data_mode = data.get(2).is_some_and(|&d| d != 0);
                        None // ack
                    }
                    _ => Some((0xFA, Vec::new())), // a `25` with a short frequency
                }
            }
            // The unselected VFO of the CURRENT band — write-only here.
            (0x25, Some(0x01)) if data.len() >= 6 => {
                r.unselected_hz = bcd_to_freq(&data[1..]);
                None
            }
            // …and its MODE (`26 01 <mode> <data>`), a register of
            // its own. Separate from `main_mode`/`sub_mode` here for
            // the same reason it is separate on the radio: that is
            // exactly what made an unwritten TX VFO keep the last
            // pass's sideband.
            (0x26, Some(0x01)) if data.len() >= 2 => {
                r.unselected_mode = data[1];
                None
            }
            // Satellite mode: read (1-byte data) / set (2-byte data).
            (0x16, Some(0x5A)) if !r.no_satmode => match data.get(1) {
                Some(&v) => {
                    if r.nak_satmode_set > 0 {
                        r.nak_satmode_set -= 1;
                        Some((0xFA, Vec::new()))
                    } else {
                        r.satmode = v != 0;
                        None
                    }
                }
                None => {
                    if r.drop_satmode_reads > 0 {
                        r.drop_satmode_reads -= 1;
                        Some((SILENT, Vec::new()))
                    } else {
                        Some((0x16, vec![0x5A, u8::from(r.satmode)]))
                    }
                }
            },
            // DATA mode set (`1A 06 <on> <filter>`) / read (`1A 06`). A real
            // IC-7300/9700 answers both; without them this fake NAKed every
            // DATA-submode set, so `M PKTUSB`/`M PKTFM` could not be tested at
            // all against it — the daemon's `set_mode` requires the DATA ack.
            (0x1A, Some(0x06)) => match data.get(1) {
                Some(&on) => {
                    r.data_mode = on != 0;
                    None // ack
                }
                None => Some((0x1A, vec![0x06, u8::from(r.data_mode), 0x01])),
            },
            // S-meter, from the band the command acts on (Main S9 by default — see
            // [`Regs::smeter_raw`]).
            (0x15, Some(0x02)) => {
                let [hi, lo] = bcd2(if on_sub {
                    r.sub_smeter_raw
                } else {
                    r.smeter_raw
                });
                Some((0x15, vec![0x02, hi, lo]))
            }
            // THE `0x14` LEVEL FAMILY — see [`Regs::levels`]. A bare
            // sub-command reads; a sub-command plus two BCD bytes writes.
            (0x14, Some(sub)) => {
                let file = if on_sub {
                    &mut r.sub_levels
                } else {
                    &mut r.levels
                };
                match (data.get(1), data.get(2)) {
                    (Some(&hi), Some(&lo)) => {
                        // Two decimal digits per byte, big-endian: `01 27` = 127.
                        let dig = |b: u8| {
                            let (h, l) = (b >> 4, b & 0x0F);
                            u16::from(if h > 9 { 0 } else { h }) * 10
                                + u16::from(if l > 9 { 0 } else { l })
                        };
                        file.insert(sub, dig(hi) * 100 + dig(lo));
                        None // ack
                    }
                    _ => {
                        let [hi, lo] = bcd2(file.get(&sub).copied().unwrap_or(0));
                        Some((0x14, vec![sub, hi, lo]))
                    }
                }
            }
            // ATTENUATOR (`0x11`): a bare frame reads, one payload byte writes.
            // The byte is stored and echoed UNTOUCHED — see [`Regs::att_raw`].
            (0x11, None) => Some((0x11, vec![if on_sub { r.sub_att_raw } else { r.att_raw }])),
            (0x11, Some(v)) => {
                if on_sub {
                    r.sub_att_raw = v;
                } else {
                    r.att_raw = v;
                }
                None // ack
            }
            // CW text (`17 …`, `17 FF` = stop), RIT / ΔTX (`21 00` offset, `21 01`/`21 02`
            // on-off), the repeater and TSQL tone frequencies (`1B 00`/`1B 01`) and the
            // duplex offset (`0D`): write-only here, and accepted, because a real IC-9700 takes
            // them (CI-V Reference Guide A7508-3EX-4, command table). Where each one LANDED is
            // what the tests read, and [`Regs::acted`] records that for every command.
            (0x17, _) | (0x21, _) | (0x1B, Some(0x00 | 0x01)) | (0x0D, _) => None,
            // THE REST OF THE `0x16` FAMILY — see [`Regs::funcs`]. Read is the
            // bare sub-command, write carries the byte after it, exactly as
            // satellite mode does above (and this arm sits AFTER it, so `5A`
            // keeps its own field and its fault injection).
            //
            // ⚠️ Keeping read and write apart is load-bearing, not tidiness: a
            // read swallowed as a write would set the register to the
            // sub-command's own value and answer nothing — switching the preamp
            // or the monitor to something nobody asked for, in the fixture that
            // is supposed to be the witness.
            // ⛔ `5A` NEVER reaches here. A rig configured `no_satmode` must
            // answer satellite mode with a NAK — an honest "this rig has no such
            // mode", which is the capability probe the 7300-family relies on.
            // Without this exclusion the arm above falls through on exactly that
            // configuration and the generic map ACKS it, turning a single-band
            // rig into one that claims satellite mode.
            (0x16, Some(sub)) if sub != 0x5A => {
                let file = if on_sub {
                    &mut r.sub_funcs
                } else {
                    &mut r.funcs
                };
                match data.get(1) {
                    Some(&v) => {
                        file.insert(sub, v);
                        None // ack
                    }
                    None => Some((0x16, vec![sub, file.get(&sub).copied().unwrap_or(0)])),
                }
            }
            // Scope CENTER/FIXED (`27 14`) — remembered so the span below can be
            // refused. The mode is the LAST payload byte on both frame shapes
            // (`27 14 <fixed>` single-scope, `27 14 <main_sub> <fixed>` dual).
            // ⚠️ READ vs WRITE, and the distinction is load-bearing. A CI-V READ is
            // the bare sub-command (`27 14`, len 1); a WRITE carries the mode after it
            // (`27 14 <fixed>`, or `27 14 <main_sub> <fixed>` on a dual-scope rig). The
            // arm below used to take `data.last()` unconditionally, so a READ would have
            // been swallowed as a write of `0x14` — setting the mode to Center and
            // answering nothing. A test built on that would have "passed" while the
            // fixture quietly rewrote the state under it.
            // A READ carries no mode byte; a WRITE does. Length alone cannot tell
            // them apart, because a dual-scope rig puts a Main/Sub selector between the
            // sub-command and the payload: a single-rig WRITE and a dual-rig READ are
            // both two bytes. So ask the SAME question the real code asks — is this
            // address dual — instead of guessing from the length.
            (0x27, Some(0x14))
                if data.len() == 1 + usize::from(crate::civ::scope::scope_is_dual(addr)) =>
            {
                Some((0x27, {
                    let mut d = vec![0x14];
                    if crate::civ::scope::scope_is_dual(addr) {
                        d.push(0x00);
                    }
                    d.push(u8::from(r.scope_fixed));
                    d
                }))
            }
            (0x27, Some(0x14)) => {
                r.scope_fixed = data.last().is_some_and(|&b| b == 0x01);
                None
            }
            // Scope SPAN (`27 15`) — NAKed while the scope is in Fixed mode, which
            // is the #275 refusal this fixture exists to produce.
            (0x27, Some(0x15)) if r.scope_fixed => Some((0xFA, Vec::new())),
            (0x27, _) => None, // scope enable/disable, span in centre
            // Voice TX memory (`28 00`, `00` = stop): acked. Whether a real rig acks
            // a stop with nothing playing is a bench question; tests assert the frame.
            (0x28, Some(0x00)) => None,
            _ => Some((0xFA, Vec::new())), // NAK anything unknown
        }
    }

    /// Icom's BAND-DIRECTED form, `29 <band> <command…>` (`band` 00 = MAIN, 01 = SUB), as the
    /// IC-7610 CI-V Reference Guide A7380-7EX-4 describes it: p. 9, "Regardless of
    /// active/inactive the Main or Sub band, you can directly specify the Main or Sub band";
    /// p. 15, "When you receive the OK code (FB), or the NG code (FA), the Command 29 and
    /// Main/Sub specify (00 or 01) is omitted" — so an ack is a bare `FB`, and a DATA reply
    /// carries the prefix back.
    ///
    /// A radio without the command (`cmd29` false — the IC-9700 fixture) NAKs it, and so does
    /// the IC-7610 for any command its table does not mark. The selection is never touched.
    fn band_directed(r: &mut Regs, addr: u8, data: &[u8]) -> Option<(u8, Vec<u8>)> {
        let nak = Some((0xFA, Vec::new()));
        let (Some(&band), Some(&cmd)) = (data.first(), data.get(1)) else {
            return nak;
        };
        let inner = &data[2..];
        let marked = IC7610_CMD29
            .iter()
            .any(|&(c, s)| c == cmd && (s.is_none() || s == inner.first().copied()));
        if !r.cmd29 || band > 0x01 || !marked {
            return nak;
        }
        match act(r, addr, cmd, inner, band == 0x01) {
            // Acks and refusals come back bare (p. 15); a lost reply stays lost.
            reply @ (None | Some((0xFA | SILENT, _))) => reply,
            Some((rcmd, rdata)) => {
                let mut d = vec![band, rcmd];
                d.extend_from_slice(&rdata);
                Some((0x29, d))
            }
        }
    }

    /// Two decimal digits per byte, big-endian: `raw` 0..=9999 → the 2-byte BCD Icom's
    /// level and meter replies carry. By hand, like the decoder in the `0x14` arm.
    fn bcd2(raw: u16) -> [u8; 2] {
        let d = |v: u16| {
            let v = (v % 100) as u8;
            ((v / 10) << 4) | (v % 10)
        };
        let v = raw.min(9999);
        [d(v / 100), d(v % 100)]
    }

    /// Pseudo-reply cmd meaning "say NOTHING" (a lost reply on the wire).
    /// Deliberately not a real CI-V command byte.
    const SILENT: u8 = 0xFF;

    /// An in-memory fake IC-9700: scripted replies keyed by (cmd, first data byte).
    /// Reads time out when nothing is queued, like a real serial port.
    pub struct FakeRadio {
        addr: u8,
        outgoing: VecDeque<u8>,
        /// Unsolicited bytes injected before the next read (transceive, scope).
        push_next: Arc<Mutex<Vec<u8>>>,
        /// The register file — see [`Regs`]; share it out with [`FakeRadio::regs`].
        regs: Arc<Mutex<Regs>>,
        /// When true, drop every command silently (a dead radio).
        pub mute: bool,
        /// When true, every read/write fails hard (a yanked cable) — the engine
        /// classifies it Fatal and exits, driving `is_alive()` false.
        pub dead: bool,
    }
    impl FakeRadio {
        pub fn new(addr: u8) -> (Self, Arc<Mutex<Vec<u8>>>) {
            let push = Arc::new(Mutex::new(Vec::new()));
            (
                FakeRadio {
                    addr,
                    outgoing: VecDeque::new(),
                    push_next: push.clone(),
                    regs: Arc::new(Mutex::new(Regs {
                        main_hz: 145_000_000,
                        sub_hz: 435_000_000,
                        sel_sub: false,
                        satmode: false,
                        split: false,
                        unselected_hz: 0,
                        main_mode: 0x01, // USB
                        sub_mode: 0x05,  // FM — the 9700's Sub-band default
                        // LSB, and deliberately: the field report's TX VFO was
                        // left in LSB by an earlier inverting linear pass.
                        unselected_mode: 0x00,
                        data_mode: false,
                        no_satmode: false,
                        nak_main_select: 0,
                        nak_satmode_set: 0,
                        drop_satmode_reads: 0,
                        scope_fixed: false,
                        att_raw: 0,
                        funcs: std::collections::BTreeMap::new(),
                        levels: std::collections::BTreeMap::new(),
                        sub_levels: std::collections::BTreeMap::new(),
                        sub_funcs: std::collections::BTreeMap::new(),
                        sub_att_raw: 0,
                        smeter_raw: 120, // S9 — what every existing reading expects
                        sub_smeter_raw: 60,
                        cmd29: addr == 0x98, // the IC-7610 has `29`; the IC-9700 does not
                        dial_by_band: addr == 0x98, // `25`/`26` name MAIN/SUB on the IC-7610 only
                        drop_dial_reads: 0,
                        acted: Vec::new(),
                        wire: Vec::new(),
                        log: Vec::new(),
                    })),
                    mute: false,
                    dead: false,
                },
                push,
            )
        }
        /// Clone out the register handle BEFORE moving the radio into the engine.
        pub fn regs(&self) -> Arc<Mutex<Regs>> {
            self.regs.clone()
        }
        fn reply(&mut self, cmd: u8, data: &[u8]) {
            let f = Frame {
                to: CONTROLLER,
                from: self.addr,
                cmd,
                data: data.to_vec(),
            };
            self.outgoing.extend(f.to_bytes());
        }
        fn ack(&mut self) {
            self.reply(0xFB, &[]);
        }
    }
    impl Write for FakeRadio {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.dead {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "dead"));
            }
            // FrameSplitter drops controller-originated frames as "echo", so parse raw.
            let mut raw = Vec::new();
            let mut cur = Vec::new();
            for &b in buf {
                cur.push(b);
                if b == 0xFD {
                    raw.push(std::mem::take(&mut cur));
                }
            }
            for bytes in raw {
                let Some(f) = Frame::parse(&bytes) else {
                    continue;
                };
                if self.mute {
                    continue;
                }
                let addr = self.addr;
                let action = {
                    // Poison-tolerant: a test asserting under the regs lock may
                    // panic; the engine thread must not cascade after it.
                    let mut r = self
                        .regs
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    r.log.push((f.cmd, f.data.clone()));
                    r.wire.push(bytes.clone());
                    // Decide the reply under the register lock, emit after.
                    if f.cmd == 0x29 {
                        band_directed(&mut r, addr, &f.data)
                    } else {
                        let on_sub = r.sel_sub;
                        act(&mut r, addr, f.cmd, &f.data, on_sub)
                    }
                };
                match action {
                    None => self.ack(),
                    Some((SILENT, _)) => {} // lost reply — the request times out
                    Some((0xFA, _)) => self.reply(0xFA, &[]),
                    Some((cmd, data)) => self.reply(cmd, &data),
                }
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Read for FakeRadio {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.dead {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "dead"));
            }
            if let Ok(mut p) = self.push_next.lock() {
                if !p.is_empty() {
                    self.outgoing.extend(p.drain(..));
                }
            }
            if self.outgoing.is_empty() {
                std::thread::sleep(Duration::from_millis(2));
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no data"));
            }
            let n = buf.len().min(self.outgoing.len());
            for (i, b) in self.outgoing.drain(..n).enumerate() {
                buf[i] = b;
            }
            Ok(n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::commands::{read_freq, read_smeter, set_freq};
    use super::super::frame::{freq_to_bcd, Frame};
    use super::tests_support::FakeRadio;
    use super::*;

    #[test]
    fn transact_read_and_set_against_a_fake_radio() {
        let (radio, _push) = FakeRadio::new(0xA2);
        let eng = CivEngine::start(Box::new(radio), 0xA2);
        let h = eng.handle();
        // Read the frequency.
        let f = h
            .transact(
                read_freq(0xA2),
                Expect::Reply {
                    cmd: 0x03,
                    sub: None,
                },
            )
            .expect("freq read");
        assert_eq!(super::super::commands::parse_freq(&f), Some(145_000_000));
        // Set a new one (ack), read it back.
        h.transact(set_freq(0xA2, 144_200_000), Expect::Ack)
            .expect("freq set acked");
        let f = h
            .transact(
                read_freq(0xA2),
                Expect::Reply {
                    cmd: 0x03,
                    sub: None,
                },
            )
            .expect("freq re-read");
        assert_eq!(super::super::commands::parse_freq(&f), Some(144_200_000));
        // The engine folded replies into the shared state too.
        assert_eq!(h.state().freq_hz, Some(144_200_000));
    }

    #[test]
    fn sub_commanded_read_matches_on_the_sub_byte() {
        let (radio, _push) = FakeRadio::new(0xA2);
        let eng = CivEngine::start(Box::new(radio), 0xA2);
        let f = eng
            .handle()
            .transact(
                read_smeter(0xA2),
                Expect::Reply {
                    cmd: 0x15,
                    sub: Some(0x02),
                },
            )
            .expect("smeter read");
        assert_eq!(super::super::commands::parse_smeter_raw(&f), Some(120));
    }

    #[test]
    fn a_dead_radio_times_out_instead_of_wedging() {
        let (mut radio, _push) = FakeRadio::new(0xA2);
        radio.mute = true;
        let eng = CivEngine::start(Box::new(radio), 0xA2);
        let t0 = Instant::now();
        let r = eng.handle().transact(
            read_freq(0xA2),
            Expect::Reply {
                cmd: 0x03,
                sub: None,
            },
        );
        assert_eq!(r.unwrap_err(), CivError::Timeout);
        assert!(t0.elapsed() < Duration::from_secs(3), "bounded, not wedged");
        // And the engine still answers later requests (a NAK-ing radio here).
        // (mute stays on — a second request also times out but doesn't panic.)
        let r = eng.handle().transact(
            read_freq(0xA2),
            Expect::Reply {
                cmd: 0x03,
                sub: None,
            },
        );
        assert_eq!(r.unwrap_err(), CivError::Timeout);
    }

    #[test]
    fn unsolicited_transceive_folds_into_state_without_a_request() {
        let (radio, push) = FakeRadio::new(0xA2);
        let eng = CivEngine::start(Box::new(radio), 0xA2);
        // The operator turns the knob: the radio pushes cmd 00 with the new freq.
        let f = Frame {
            to: 0x00, // transceive broadcasts to address 00
            from: 0xA2,
            cmd: 0x00,
            data: freq_to_bcd(146_520_000).to_vec(),
        };
        push.lock().unwrap().extend(f.to_bytes());
        // Wait for the engine to pick it up.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if eng.handle().state().freq_hz == Some(146_520_000) {
                break;
            }
            assert!(Instant::now() < deadline, "transceive folded into state");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A band-directed read resolves on its OWN band's reply — prefixed (`29 <band> <cmd>
    /// <sub> <data>`) or bare — and never on the other band's.
    #[test]
    fn a_band_directed_read_takes_its_own_bands_reply_in_either_shape() {
        let want = Expect::ReplyOnBand {
            band: 0x00,
            cmd: 0x14,
            sub: Some(0x01),
        };
        let from = |cmd: u8, data: &[u8]| Frame {
            to: 0xE0,
            from: 0x98,
            cmd,
            data: data.to_vec(),
        };
        let prefixed = from(0x29, &[0x00, 0x14, 0x01, 0x01, 0x27]);
        assert_eq!(resolves(want, &prefixed), Some(Ok(prefixed.clone())));
        let bare = from(0x14, &[0x01, 0x01, 0x27]);
        assert_eq!(resolves(want, &bare), Some(Ok(bare.clone())));
        // The Sub's answer is not Main's, and another level is not this one.
        assert_eq!(
            resolves(want, &from(0x29, &[0x01, 0x14, 0x01, 0x00, 0x50])),
            None
        );
        assert_eq!(
            resolves(want, &from(0x29, &[0x00, 0x14, 0x02, 0x00, 0x50])),
            None
        );
        assert_eq!(resolves(want, &from(0x14, &[0x02, 0x00, 0x50])), None);
        // A refusal is a refusal, whatever was asked.
        assert_eq!(resolves(want, &from(0xFA, &[])), Some(Err(CivError::Nak)));
    }

    #[test]
    fn nak_resolves_as_nak_not_timeout() {
        let (radio, _push) = FakeRadio::new(0xA2);
        let eng = CivEngine::start(Box::new(radio), 0xA2);
        // The fake NAKs unknown commands — 0x1C PTT isn't scripted.
        let r = eng
            .handle()
            .transact(super::super::commands::set_ptt(0xA2, true), Expect::Ack);
        assert_eq!(r.unwrap_err(), CivError::Nak);
    }
}
