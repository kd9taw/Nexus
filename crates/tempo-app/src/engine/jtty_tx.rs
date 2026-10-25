//! JTTY transmit — WSJT-X's keyboard mode, sent.
//!
//! ⛔ Every send passes every transmit guard: [`Engine::jtty_tx_gate`] where it is planned, and
//! [`Engine::poll_jtty`] again before every frame the radio loop is handed, so a guard that goes
//! down with JTTY on the air drops what is queued and cuts it.
//!
//! THE SHAPE: WSJT-X 3.3.0-beta4's transmitter (`widgets/mainwindow_jtty.cpp`,
//! `lib/jtty/JttyTransmit.cpp`, `widgets/JttyMessages.hpp`, `widgets/JttyLiveEntry.hpp`), on
//! Nexus's keyboard machinery.
//! - **A send** is text the operator typed, or an F-key. Text is prepared as WSJT-X prepares it
//!   ([`crate::keyboard::kb_filter`] over [`crate::keyboard::JTTY`]), split at line breaks and
//!   then into segments of at most 80 characters at spaces (`nextTransmitTextSegment`), and each
//!   segment is encoded with end-of-message only where WSJT-X sets it: on the last segment of a
//!   line that ends the message, or that a line break ends (`Jtty::encodeTransmitText`). A
//!   segment that will not encode refuses the whole send; nothing of it is sent. The F-keys are WSJT-X's eight Contest templates, compiled as
//!   WSJT-X compiles them into call and exchange atoms (`Jtty::compileNativeMacro`): a call that
//!   is not a standard one REFUSES the key, it is never sent as literal text.
//! - **The queue** chains segments with no gap and no spacing frame; the Tx offset and the
//!   exchange profile are the ones in force when the send was made.
//! - **Rendering** (the encoder and the waveform both take the modem's `MODEM_LOCK`, which a
//!   decode can hold for seconds) runs with the engine lock RELEASED and off the radio loop: a
//!   send is planned under the lock, rendered without it, and committed under it again, refused
//!   if anything stopped JTTY in between ([`JttyTx::gen`]). FT8's plan/build/commit. A send from a
//!   computer driving the station is also asked, in both holds, whether it still holds
//!   ([`run_bound`]).
//! - **The radio loop** asks for audio a frame at a time and keeps at most ONE frame ahead
//!   ([`crate::keyboard::JTTY`]); every gate is re-checked on every tick while anything is queued
//!   or on the air, and a gate going down drops all of it and cuts the audio.
//! - **Live entry** (the operator, 2026-10-08: "Include live typing now") is a
//!   [`crate::keyboard::KeyboardLatch`]: continuous PTT, the filler `<<<<< <<<<<` whenever nothing
//!   typed is waiting, the per-tick gate re-check, the 10-minute ceiling and the watchdog. The
//!   ceiling bounds the whole over, from the session's start until what it left queued has
//!   drained: Send or the sign-off ending the session does not lift it. What is
//!   typed is committed as WSJT-X commits it: in batches once five words are complete beyond the
//!   three held back (an atom in progress is never split), everything after a pause, and the
//!   whole of it, with end-of-message, on Send or the sign-off (`autoAdvanceJttyLiveEntry`,
//!   `idleFlushJttyLiveEntry`, `commitJttyLiveEntry`, `insertJttyChatSignoff`).
//! - **Stop** (Stop TX, Esc, TX Off, the halt) cancels everything queued and cuts what is on the
//!   air, and leaving the JTTY screen does the same, as WSJT-X stops on a mode change
//!   (`noteTxModeChange`). Neither ever waits or is refused.
//!
//! Nexus's own, against WSJT-X's (the guards WSJT-X does not have, each only refusing more): the
//! TX-enable latch, the licence gate judged at the Tx offset's whole span, the callsign at the
//! keying boundary, the wall-clock watchdog (WSJT-X's is inert in JTTY), and a queue that must
//! fit the watchdog before anything keys.

use std::collections::VecDeque;
use std::sync::Mutex;

use super::{engine_lock, now_unix_millis, now_unix_secs, Engine, Mode, TxOwner};

/// WSJT-X's JTTY Tx offset: 1500 Hz, set again whenever JTTY is selected
/// (`on_actionJTTY_triggered`), and its spin box's range (`TxFreqSpinBox_2`).
pub const JTTY_TX_HZ: f32 = 1500.0;
pub const JTTY_TX_MIN_HZ: f32 = 200.0;
pub const JTTY_TX_MAX_HZ: f32 = 5000.0;

/// WSJT-X's serial number spin box for JTTY (`sbSerialNumber_2`): 1 to 5000, starting at 1.
pub const JTTY_SERIAL_MIN: u32 = 1;
pub const JTTY_SERIAL_MAX: u32 = 5000;

/// The keep-alive WSJT-X's live entry sends while the typist pauses (`Jtty::jttyFillerText`):
/// ordinary text, never shown, logged or counted at either end.
pub const FILLER: &str = "<<<<< <<<<<";

/// WSJT-X's encoder width: a segment is at most 80 characters (`Jtty::maxTransmitLength`).
const SEGMENT_CHARS: usize = 80;

/// The words a live commit holds back, so an atom still being typed is never split
/// (`Jtty::maxCompactAtomWords`), and the batch that commits early once that many are complete
/// beyond them (`kJttyAutoAdvanceBatchWords`).
const HOLDBACK_WORDS: usize = 3;
const BATCH_WORDS: usize = 5;

/// One frame on the air: 59 symbols at 12 kHz.
const FRAME_SAMPLES: usize = jtty::FRAME_SAMPLES;

/// JTTY's symbol rate, for [`crate::keyboard::JTTY`]'s look-ahead in milliseconds.
pub const JTTY_BAUD: f64 = jtty::BAUD as f64;

/// The signal's span above its Tx offset: tone 0 sits at the offset and the four tones span
/// four tone spacings (`jtty::gen_wave`). The licence gate judges both edges.
pub const JTTY_SPAN_HZ: f32 = 4.0 * jtty::BAUD;

/// How far either side of our own keyed time a decoded message is taken for our own echo: one
/// frame, the longest a message can start before or after any audio of ours it overlaps.
const ECHO_MARGIN_MS: i64 = 1888;

/// What the cockpit's notice line says: WSJT-X's own words where it has them.
const CANCELLED: &str = "JTTY transmission cancelled; some text may already have transmitted.";
const COMPLETE: &str = "JTTY transmission complete.";
const SUBSTITUTED: &str =
    "Unsupported characters were replaced. Hover over Send to review the text.";
const LEFT_SCREEN: &str = "JTTY stopped: you left the JTTY screen, so what was still queued was \
     dropped. Send it again when you are ready.";
const REFUSED_TX_OFF: &str = "JTTY stopped: transmit was turned off, so what was still queued \
     was dropped, not held for later. Send it again when you are ready.";
const REFUSED_PRIVILEGES: &str = "JTTY stopped: the Tx frequency is outside your license \
     privileges, so what was still queued was dropped. Send it again from inside them.";
const REFUSED_TUNE: &str = "JTTY stopped: a tune carrier took the transmitter, so what was \
     still queued was dropped. Send it again when you are ready.";
const KEY_REFUSED: &str = "JTTY keyer: the rig didn't accept PTT, so nothing was sent and \
     what was queued was dropped. Check your PTT method + that Nexus's audio output is routed to \
     the rig (like FT8).";
const REFUSED_LOOP: &str = "JTTY stopped: the radio was busy (switching, or held by CAT), so \
     what was still queued was dropped. Send it again when you are ready.";
const REFUSED_CLOCK: &str = "JTTY stopped: a clock repair started, so what was still queued was \
     dropped. Send it again when the repair has finished.";
const REFUSED_HOLD: &str = "JTTY stopped: a device signed in moments ago and holds transmit, so \
     what was still queued was dropped. Send it again once it is approved at the station.";
/// WSJT-X 3.3.0-beta4's words when a segment will not encode (`execute_jtty_tx`,
/// `widgets/mainwindow_jtty.cpp` 413-416): the whole send is refused.
const REFUSED_TOO_LONG: &str = "JTTY text not sent: a line does not fit in one message once \
     encoded; shorten it and send again.";
const WATCHDOG_TRIPPED: &str = "JTTY stopped by the TX watchdog: nothing was typed or sent for \
     its whole limit. Turn TX on again to carry on.";
const STOPPED_FOR_PREP: &str =
    "JTTY was stopped while the message was being prepared; nothing was sent.";

/// One segment ready for the transmitter: its audio, made at the Tx offset it was sent at.
#[derive(Debug, Clone)]
pub struct JttySegment {
    /// What the far end reads: the encoder's normalised text.
    pub text: String,
    /// 12 kHz audio, amplitude 1, whole frames.
    pcm: Vec<f32>,
    /// End-of-message on its last frame.
    pub last: bool,
    /// The keep-alive filler: never shown, logged or counted.
    pub filler: bool,
    /// The first segment of its send (the one WSJT-X's serial rule reads).
    first: bool,
    /// The Tx offset it was rendered at.
    tx_hz: f32,
}

impl JttySegment {
    /// Frames on the air.
    pub fn frames(&self) -> usize {
        self.pcm.len() / FRAME_SAMPLES
    }
}

/// The segment the radio loop is being fed, and how much of it has gone.
#[derive(Debug, Clone)]
struct Playing {
    seg: JttySegment,
    at: usize,
}

/// The engine's JTTY transmit state. Session-only except the serial number, which Settings
/// keeps (`jtty_serial`, WSJT-X's `SerialNumberJTTY`).
#[derive(Debug)]
pub struct JttyTx {
    /// The Tx offset (WSJT-X's `TxFreqSpinBox_2`).
    pub(super) tx_hz: f32,
    /// The contest serial number (`sbSerialNumber_2`).
    pub(super) serial: u32,
    /// Segments waiting, oldest first.
    queue: VecDeque<JttySegment>,
    /// The segment the loop is being fed.
    playing: Option<Playing>,
    /// Live entry: the operator's intent, the (unused) type-ahead and the ceiling's anchor — the
    /// three fields a [`crate::keyboard::KeyboardLatch`] borrows. JTTY's typed text is committed
    /// in segments by its planner, so the type-ahead stays empty.
    latched: bool,
    type_buf: VecDeque<char>,
    latch_start_ms: Option<u64>,
    /// The filler, rendered when the live session started (at its Tx offset).
    filler: Option<JttySegment>,
    /// The live session has put something on the air. WSJT-X tops a live session up with the
    /// filler only once a commit has gone out and drained (`finishJttyDrain`): arming alone
    /// keys nothing.
    live_keyed: bool,
    /// What is or was last on the air was stopped, not finished: no "complete" notice.
    stopped: bool,
    /// One-shot: the radio loop flushes JTTY's audio and unkeys.
    abort: bool,
    /// JTTY audio is on the air (the radio loop's per-tick truth).
    sending: bool,
    /// When the current keyed stretch began, UTC ms.
    keyed_since_ms: Option<i64>,
    /// WSJT-X's queue notice: cancelled, complete, replaced characters, or why it stopped.
    notice: Option<String>,
    /// Bumped by every stop, drop and Tx change: a send planned before one is not committed.
    pub(super) gen: u64,
}

impl JttyTx {
    pub(super) fn with_serial(serial: u32) -> Self {
        Self {
            tx_hz: JTTY_TX_HZ,
            serial: serial.clamp(JTTY_SERIAL_MIN, JTTY_SERIAL_MAX),
            queue: VecDeque::new(),
            playing: None,
            latched: false,
            type_buf: VecDeque::new(),
            latch_start_ms: None,
            filler: None,
            live_keyed: false,
            stopped: false,
            abort: false,
            sending: false,
            keyed_since_ms: None,
            notice: None,
            gen: 0,
        }
    }

    /// Anything of JTTY's is queued, on the air, or latched.
    fn active(&self) -> bool {
        self.sending
            || self.playing.is_some()
            || !self.queue.is_empty()
            || crate::keyboard::streaming(self.latched, &self.type_buf)
    }

    /// The segments still to go, the one being fed included (WSJT-X's "Send (3 left)"), and
    /// their text, one per line (its tooltip). The filler is not counted.
    fn pending(&self) -> (u32, String) {
        let playing = self
            .playing
            .as_ref()
            .filter(|p| p.at < p.seg.pcm.len())
            .map(|p| &p.seg);
        let segs: Vec<&JttySegment> = playing
            .into_iter()
            .chain(self.queue.iter())
            .filter(|s| !s.filler)
            .collect();
        let text = segs
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        (segs.len() as u32, text)
    }

    /// Air time still queued, the rest of the segment being fed included (ms).
    fn queued_ms(&self) -> f64 {
        let left = self
            .playing
            .as_ref()
            .map_or(0, |p| p.seg.pcm.len().saturating_sub(p.at));
        let queued: usize = self.queue.iter().map(|s| s.pcm.len()).sum();
        (left + queued) as f64 / 12.0
    }
}

/// What the radio loop does this tick on JTTY's behalf.
#[derive(Debug, Clone, PartialEq)]
pub enum JttyTick {
    /// Nothing of JTTY's is queued or on the air. Key nothing.
    Idle,
    /// Something is, every gate is up, and the loop is fed far enough ahead: stay keyed.
    Ahead,
    /// Key this audio next (12 kHz, at most one frame), chained after what was played.
    Frame(Vec<f32>),
}

/// How a live entry's typed text is committed (WSJT-X's three ways).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JttyCommit {
    /// While typing: only once five words are complete beyond the three held back
    /// (`autoAdvanceJttyLiveEntry`).
    Advance,
    /// After a pause: everything typed, the word in progress too, the message left open
    /// (`idleFlushJttyLiveEntry`).
    Flush,
}

/// What a send renders: decided under the engine lock, rendered without it.
#[derive(Debug, Clone)]
pub struct JttyPlan {
    gen: u64,
    tx_hz: f32,
    /// The exchange profile the text is packed for, the one in force when the send was made.
    profile: jtty::Profile,
    what: Planned,
    /// Start the live session when this commits.
    start_live: bool,
    /// End it (Send or the sign-off): the latch comes down once what is queued has gone.
    end_live: bool,
    /// How much of the text the operator typed this send takes (UTF-16 code units, as the
    /// cockpit counts), and what goes in its place (the macros expanded and the text prepared, as
    /// WSJT-X rewrites its box).
    pub consumed: usize,
    pub sent: String,
    /// Characters outside the JTTY alphabet were replaced.
    substituted: bool,
}

#[derive(Debug, Clone)]
enum Planned {
    /// Prepared text: lines, and whether the last one ends the message.
    Text { text: String, last: bool },
    /// An F-key's atoms, and the text they read as.
    Atoms {
        atoms: Vec<jtty::AtomDescriptor>,
        text: String,
    },
    /// Nothing to encode (a live session started with nothing typed).
    Nothing,
}

/// What a plan rendered to.
#[derive(Debug, Clone)]
pub struct JttyRendered {
    segments: Vec<JttySegment>,
    filler: Option<JttySegment>,
}

// ---------------------------------------------------------------------------------------------
// WSJT-X's text rules, described from its code (not copied).
// ---------------------------------------------------------------------------------------------

/// `Jtty::nextTransmitTextSegment`: the span of `line` from `offset`, at most `max` (and 80)
/// characters, ending after a space where the line goes on with more than the span (the last
/// space that leaves something before it), so a word is only split when it is longer than the
/// span. The span keeps its separating spaces, so every character is accounted for.
fn next_segment(line: &[char], offset: usize, max: usize) -> (usize, String) {
    if offset >= line.len() || max == 0 {
        return (0, String::new());
    }
    let mut length = (line.len() - offset).min(max).min(SEGMENT_CHARS);
    if offset + length < line.len() && line[offset + length] != ' ' {
        for i in (1..length).rev() {
            if line[offset + i] == ' '
                && line[offset..offset + i].iter().any(|c| !c.is_whitespace())
            {
                length = i + 1;
                break;
            }
        }
    }
    (length, line[offset..offset + length].iter().collect())
}

/// `Jtty::completeWordCount`: the space-terminated words; one still being typed does not count.
fn complete_words(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let words = text.split(' ').filter(|w| !w.is_empty()).count();
    if text.ends_with(' ') {
        words
    } else {
        words.saturating_sub(1)
    }
}

/// `Jtty::planIncrementalCommit`: how much of `text` to commit now. Forced, all of it;
/// otherwise up to the end of the space after the last complete word that is not one of the
/// `holdback` held back (nothing when none is). Assumes single spaces, as WSJT-X's does.
fn incremental_commit(text: &str, force: bool, holdback: usize) -> usize {
    if text.is_empty() {
        return 0;
    }
    if force {
        return text.len();
    }
    let commit = complete_words(text).saturating_sub(holdback);
    if commit == 0 {
        return 0;
    }
    let mut length = 0;
    for _ in 0..commit {
        match text[length..].find(' ') {
            Some(at) => length += at + 1,
            None => return length,
        }
    }
    length
}

/// `Jtty::formatSerialNumber`: at least three digits, zero-filled.
fn serial_text(serial: u32) -> String {
    format!("{serial:03}")
}

/// `Jtty::formatSnr`: a sign and at least two digits (`%+03d`).
fn snr_text(snr: i32) -> String {
    format!("{snr:+03}")
}

/// Who a send is from and to: what WSJT-X's macros read (`Jtty::NativeMacroContext`).
#[derive(Debug, Clone)]
struct MacroContext {
    my_call: String,
    his_call: String,
    serial: u32,
    /// The exchange profile, and the exchange it sends (Field Day's class and section; empty
    /// with no profile): [`Engine::jtty_exchange`].
    profile: jtty::Profile,
    exchange: String,
    grid: String,
    snr: i32,
}

/// `Jtty::nativeExchangeFieldText`, what `%E` reads: the serial with no exchange profile, else the
/// exchange.
fn exchange_text(ctx: &MacroContext) -> String {
    match ctx.profile {
        jtty::Profile::Unknown => serial_text(ctx.serial),
        _ => ctx.exchange.clone(),
    }
}

/// `Jtty::isCanonicalBase36Token`: two or three letters or digits, upper case, three not starting
/// with 0.
fn is_canonical_token(t: &str) -> bool {
    (t.len() == 2 || (t.len() == 3 && !t.starts_with('0')))
        && t.bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
}

/// `Jtty::nativeExchange`: the exchange as one atom, and the text it reads as. With no profile,
/// the serial and its report; in Field Day, the class and section (`COUNTCLASS SECTION`, the
/// count 1 to 32, the class A to F). WSJT-X's reason when the exchange will not fit the atom.
fn native_exchange(ctx: &MacroContext) -> Result<(jtty::AtomDescriptor, String), String> {
    if ctx.profile != jtty::Profile::FieldDay {
        return Ok((
            serial_atom(ctx.serial),
            format!("599 {}", serial_text(ctx.serial)),
        ));
    }
    let fields: Vec<&str> = ctx.exchange.split(' ').collect();
    let [count_class, section] = fields[..] else {
        return Err("the Field Day exchange must be COUNTCLASS SECTION".to_string());
    };
    let Some(class) = count_class
        .chars()
        .last()
        .filter(|_| count_class.len() >= 2)
    else {
        return Err("the Field Day exchange must be COUNTCLASS SECTION".to_string());
    };
    let count_text = &count_class[..count_class.len() - class.len_utf8()];
    let count = count_text
        .parse::<i32>()
        .ok()
        .filter(|n| count_text.bytes().all(|b| b.is_ascii_digit()) && (1..=32).contains(n))
        .ok_or("the Field Day transmitter count must be between 1 and 32")?;
    if !('A'..='F').contains(&class) {
        return Err("the Field Day class must be A through F".to_string());
    }
    if !is_canonical_token(section) {
        return Err("the Field Day section must be a canonical 2-3 character code".to_string());
    }
    let atom = jtty::atom(3, 1, (class as u8 - b'A') as i8, count, section)
        .expect("a section is at most three characters");
    Ok((atom, format!("{count}{class} {section}")))
}

/// `Jtty::normalizedFieldDayExchange`: upper case, single spaces, and a space put between the
/// class and the section when the two were written together (`3AWI`).
fn normalized_field_day_exchange(value: &str) -> String {
    let mut exchange = value.split_whitespace().collect::<Vec<_>>().join(" ");
    exchange.make_ascii_uppercase();
    if exchange.contains(' ') {
        return exchange;
    }
    let digits = exchange.bytes().take_while(u8::is_ascii_digit).count();
    let b = exchange.as_bytes();
    if digits > 0 && digits + 1 < b.len() && (b'A'..=b'F').contains(&b[digits]) {
        exchange.insert(digits + 1, ' ');
    }
    exchange
}

/// `Jtty::expandLiteralMacro`, the expansion WSJT-X gives typed text: `%M` my call, `%H` and
/// `%Q` the DX call, `%N` the serial, `%E` the exchange ([`exchange_text`]), `%G` my grid's first
/// four characters, `%R` the DX's SNR. Case-sensitive, as Qt's `replace` is.
fn expand_literal(text: &str, ctx: &MacroContext) -> String {
    let grid: String = ctx.grid.trim().to_uppercase().chars().take(4).collect();
    text.replace("%M", &ctx.my_call)
        .replace("%H", &ctx.his_call)
        .replace("%Q", &ctx.his_call)
        .replace("%N", &serial_text(ctx.serial))
        .replace("%E", &exchange_text(ctx))
        .replace("%G", &grid)
        .replace("%R", &snr_text(ctx.snr))
}

/// `Jtty::isNativeCall`: a call a JTTY call atom carries — 3 to 6 characters, not starting with
/// Q, its last digit second or third with a letter before it, and one to three letters after.
pub fn is_native_call(raw: &str) -> bool {
    let call: Vec<char> = raw.trim().to_uppercase().chars().collect();
    if call.len() < 3 || call.len() > 6 || call[0] == 'Q' {
        return false;
    }
    let Some(area) = (1..call.len()).rev().find(|&i| call[i].is_ascii_digit()) else {
        return false;
    };
    if area != 1 && area != 2 {
        return false;
    }
    let prefix = &call[..area];
    if !prefix
        .iter()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        || !prefix.iter().any(|c| c.is_ascii_uppercase())
        || area == call.len() - 1
    {
        return false;
    }
    call[area + 1..].iter().all(|c| c.is_ascii_uppercase()) && call.len() - area - 1 <= 3
}

/// WSJT-X's eight JTTY Contest templates (`Jtty::nativeMacroTemplate`), F1 to F8.
pub const FKEY_TEMPLATES: [&str; 8] = [
    "CQ %M CQ",
    "%H %E",
    "%H TU CQ %M CQ",
    "%M",
    "%H",
    "TU NOW %Q %E",
    "%H AGN?",
    "%E",
];

/// `Jtty::CallAction`, the call atom's subtype.
const CQ: i8 = 0;
const CALL: i8 = 1;
const CALL_TU: i8 = 3;
const CALL_AGN: i8 = 4;
const TU_NOW_CALL: i8 = 5;

fn call_atom(action: i8, call: &str) -> jtty::AtomDescriptor {
    jtty::atom(0, action, 0, 0, &call.trim().to_uppercase())
        .expect("a native call is at most six characters")
}

/// The serial as an EXCH_NUM SERIAL atom with its report (`Jtty::nativeSerialAtom`).
fn serial_atom(serial: u32) -> jtty::AtomDescriptor {
    jtty::atom(1, 0, 1, serial as i32, "").expect("no text")
}

/// F-key `n` (1..=8) compiled as WSJT-X compiles its Contest template (`Jtty::compileNativeMacro`):
/// the atoms and the text they read as, the exchange in the context's profile
/// ([`native_exchange`]). A call the template needs that is not a native one, or an exchange the
/// atom cannot carry, REFUSES the key — never literal text in its place.
fn compile_fkey(n: u8, ctx: &MacroContext) -> Result<(Vec<jtty::AtomDescriptor>, String), String> {
    let my = ctx.my_call.trim().to_uppercase();
    let his = ctx.his_call.trim().to_uppercase();
    let need_my = || -> Result<(), String> {
        if is_native_call(&my) {
            Ok(())
        } else {
            Err(format!(
                "F{n} sends your callsign as a JTTY call atom, and \"{my}\" is not a standard \
                 callsign. Nothing was sent."
            ))
        }
    };
    let need_his = || -> Result<(), String> {
        if is_native_call(&his) {
            Ok(())
        } else if his.is_empty() {
            Err(format!(
                "F{n} sends the other station's call: put it in the log strip's Call first. \
                 Nothing was sent."
            ))
        } else {
            Err(format!(
                "F{n} sends the other station's call as a JTTY call atom, and \"{his}\" is not a \
                 standard callsign. Nothing was sent."
            ))
        }
    };
    let exchange =
        || native_exchange(ctx).map_err(|why| format!("F{n} cannot send {why}. Nothing was sent."));
    Ok(match n {
        1 => {
            need_my()?;
            (vec![call_atom(CQ, &my)], format!("CQ {my} CQ"))
        }
        2 => {
            need_his()?;
            let (atom, text) = exchange()?;
            (vec![call_atom(CALL, &his), atom], format!("{his} {text}"))
        }
        3 => {
            need_his()?;
            need_my()?;
            (
                vec![call_atom(CALL_TU, &his), call_atom(CQ, &my)],
                format!("{his} TU CQ {my} CQ"),
            )
        }
        4 => {
            need_my()?;
            (vec![call_atom(CALL, &my)], my.clone())
        }
        5 => {
            need_his()?;
            (vec![call_atom(CALL, &his)], his.clone())
        }
        6 => {
            need_his()?;
            let (atom, text) = exchange()?;
            (
                vec![call_atom(TU_NOW_CALL, &his), atom],
                format!("TU NOW {his} {text}"),
            )
        }
        7 => {
            need_his()?;
            (vec![call_atom(CALL_AGN, &his)], format!("{his} AGN?"))
        }
        8 => {
            let (atom, text) = exchange()?;
            (vec![atom], text)
        }
        _ => return Err(format!("There is no F{n}: JTTY has F1 to F8.")),
    })
}

/// Render `text` as WSJT-X's `Jtty::encodeTransmitText` does: line by line, each line in segments
/// of at most 80 characters, end-of-message on the last segment of each line that ends a message —
/// every line but the last, and the last when `last`. A segment that will not encode refuses the
/// whole send, with no shorter retry (WSJT-X 3.3.0-beta4, `lib/jtty/JttyTransmit.cpp` 154-226;
/// its 3.3.0-beta1 retried the segment shorter until it encoded).
fn render_text(
    text: &str,
    last: bool,
    tx_hz: f32,
    profile: jtty::Profile,
) -> Result<Vec<JttySegment>, String> {
    let encode = |t: &str, final_: bool| jtty::encode_segment(t, profile, final_);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<JttySegment> = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let line_start = out.len();
        let mut texts: Vec<String> = Vec::new();
        let mut offset = 0;
        while offset < chars.len() {
            let (len, source) = next_segment(&chars, offset, SEGMENT_CHARS);
            if source.trim().is_empty() {
                offset += len;
                continue;
            }
            let encoded = encode(&source, false).map_err(|_| REFUSED_TOO_LONG.to_string())?;
            texts.push(source.clone());
            out.push(segment(encoded, false, false, tx_hz)?);
            offset += len;
        }
        if out.len() == line_start {
            continue;
        }
        let line_ends = li + 1 < lines.len() || last;
        if line_ends {
            let source = texts.last().expect("a segment was made");
            let encoded = encode(source, true).map_err(|_| REFUSED_TOO_LONG.to_string())?;
            let n = out.len() - 1;
            out[n] = segment(encoded, true, false, tx_hz)?;
        }
    }
    if out.is_empty() {
        return Err("Nothing to send.".to_string());
    }
    out[0].first = true;
    Ok(out)
}

fn segment(
    encoded: jtty::Encoded,
    last: bool,
    filler: bool,
    tx_hz: f32,
) -> Result<JttySegment, String> {
    let pcm = jtty::gen_wave(&encoded.tones, tx_hz)
        .ok_or_else(|| "JTTY could not make the waveform. Nothing was sent.".to_string())?;
    Ok(JttySegment {
        text: encoded.sent,
        pcm,
        last,
        filler,
        first: false,
        tx_hz,
    })
}

/// Render a plan: the modem's encoder and waveform, so call it with NO engine lock held (it
/// takes `MODEM_LOCK`, which a decode may hold for seconds).
pub fn render(plan: &JttyPlan) -> Result<JttyRendered, String> {
    let segments = match &plan.what {
        Planned::Text { text, last } => render_text(text, *last, plan.tx_hz, plan.profile)?,
        Planned::Atoms { atoms, text } => {
            let tones = jtty::encode_atoms(atoms).map_err(|e| match e {
                jtty::AtomError::UnknownSection => "The Field Day section is not in the ARRL/RAC \
                    table JTTY's exchange carries. Nothing was sent."
                    .to_string(),
                e => {
                    format!("JTTY could not encode the F-key's message ({e:?}). Nothing was sent.")
                }
            })?;
            let pcm = jtty::gen_wave(&tones, plan.tx_hz)
                .ok_or_else(|| "JTTY could not make the waveform. Nothing was sent.".to_string())?;
            vec![JttySegment {
                text: text.clone(),
                pcm,
                last: true,
                filler: false,
                first: true,
                tx_hz: plan.tx_hz,
            }]
        }
        Planned::Nothing => Vec::new(),
    };
    let filler = if plan.start_live {
        let encoded = jtty::encode_segment(FILLER, plan.profile, false)
            .map_err(|_| "JTTY could not encode its filler.".to_string())?;
        Some(segment(encoded, false, true, plan.tx_hz)?)
    } else {
        None
    };
    Ok(JttyRendered { segments, filler })
}

// ---------------------------------------------------------------------------------------------
// The engine's half.
// ---------------------------------------------------------------------------------------------

impl Engine {
    /// Every reason a JTTY send would be refused, checked before anything is encoded so the
    /// operator learns why.
    pub(super) fn jtty_tx_gate(&self) -> Result<(), String> {
        if !self.jtty_section() {
            return Err("Not in the JTTY section — open the JTTY screen first".to_string());
        }
        if self.clock_repair_holds_tx() {
            return Err(super::CLOCK_REPAIR_HOLDS_TX.to_string());
        }
        // ⛔ A SIGN-IN AUTO-GRANTED DEVICE HOLDS TRANSMIT (the 2026-10-23 sign-in decision): refused
        // while the hold stands, before anything is encoded ([`Engine::remote_transmit_held`]).
        if self.remote_transmit_held() {
            return Err(super::REMOTE_HOLD_HOLDS_TX.to_string());
        }
        if !self.tx_enabled {
            return Err(super::TX_OFF_TURN_IT_ON.to_string());
        }
        if let Some(why) = self.connection_tx_refusal() {
            return Err(why.to_string());
        }
        if !tempo_core::message::is_callsign(&self.settings.mycall) {
            return Err("Set your callsign in Settings before transmitting JTTY.".to_string());
        }
        if !self.tx_allowed() {
            return Err(
                "TX locked — the Tx frequency is outside your license privileges".to_string(),
            );
        }
        // Ownership from the one arbiter — everything but JTTY itself: a send while JTTY is
        // transmitting chains behind what is queued.
        if let Some(owner) = self.tx_owner().filter(|o| *o != TxOwner::Jtty) {
            return Err(owner.busy_reason());
        }
        Ok(())
    }

    /// The exchange profile JTTY packs for, and the exchange it sends, as WSJT-X takes them from
    /// its special operating activity (`jttyExchangeProfile`, `jttyNativeMacroContext`): Field Day,
    /// with the class and section, while ARRL Field Day is the contest running; none otherwise.
    /// Winter Field Day has none, as in WSJT-X (which has no activity for it): JTTY's class/section
    /// atom carries the classes A to F, and its H, I, O and M go as text. Nexus runs no contest
    /// WSJT-X's RTTY Roundup activity is for, so that profile never applies.
    pub(super) fn jtty_exchange(&self) -> (jtty::Profile, String) {
        use tempo_core::fieldday::FtMessages;
        let field_day = match &self.mode {
            Mode::FieldDay { station, .. } if self.settings.fd_active => {
                FtMessages::of_contest(&station.log.session.event_id) == FtMessages::FieldDay
            }
            _ => false,
        };
        match self.contest_sent_exchange().filter(|_| field_day) {
            Some(exchange) => (
                jtty::Profile::FieldDay,
                normalized_field_day_exchange(&exchange),
            ),
            None => (jtty::Profile::Unknown, String::new()),
        }
    }

    /// What the macros read. `%R` sends WSJT-X's -10 until the cockpit can say which line the DX
    /// call came from (WSJT-X takes the SNR of the decoded word the operator picked).
    fn jtty_macro_context(&self, his_call: &str) -> MacroContext {
        let (profile, exchange) = self.jtty_exchange();
        MacroContext {
            my_call: self.settings.mycall.trim().to_uppercase(),
            his_call: his_call.trim().to_uppercase(),
            serial: self.jtty_tx.serial,
            profile,
            exchange,
            grid: self.settings.mygrid.clone(),
            snr: -10,
        }
    }

    fn jtty_new_plan(&self, what: Planned) -> JttyPlan {
        JttyPlan {
            gen: self.jtty_tx.gen,
            tx_hz: self.jtty_tx.tx_hz,
            profile: self.jtty_exchange().0,
            what,
            start_live: false,
            end_live: false,
            consumed: 0,
            sent: String::new(),
            substituted: false,
        }
    }

    /// Plan a one-shot send of typed text — WSJT-X's Send (Ctrl+K): all of it, the macros in it
    /// expanded, ending the message (and a live session, if one is running).
    pub fn jtty_plan_send(&mut self, text: &str, his_call: &str) -> Result<JttyPlan, String> {
        self.jtty_tx_gate()?;
        let ctx = self.jtty_macro_context(his_call);
        let expanded = expand_literal(text, &ctx);
        let prepared = crate::keyboard::kb_filter(crate::keyboard::JTTY, &expanded);
        if prepared.trim().is_empty() {
            return Err("Nothing to send.".to_string());
        }
        let substituted = prepared != expanded.replace('\r', "\n");
        let mut plan = self.jtty_new_plan(Planned::Text {
            text: prepared.clone(),
            last: true,
        });
        plan.end_live = true;
        plan.consumed = text.encode_utf16().count();
        plan.sent = prepared;
        plan.substituted = substituted;
        Ok(plan)
    }

    /// Plan F-key `n` (1..=8) with `his_call` (the log strip's Call), compiled as WSJT-X compiles
    /// its Contest templates ([`FKEY_TEMPLATES`]): one message, ending there.
    pub fn jtty_plan_fkey(&mut self, n: u8, his_call: &str) -> Result<JttyPlan, String> {
        self.jtty_tx_gate()?;
        let ctx = self.jtty_macro_context(his_call);
        let (atoms, text) = compile_fkey(n, &ctx)?;
        let mut plan = self.jtty_new_plan(Planned::Atoms {
            atoms,
            text: text.clone(),
        });
        plan.sent = text;
        Ok(plan)
    }

    /// Plan the start of a live session (WSJT-X's Alt+J): what is typed so far goes out with
    /// the message left open, and the filler holds the transmitter between what follows.
    pub fn jtty_plan_live_start(
        &mut self,
        typed: &str,
        his_call: &str,
    ) -> Result<JttyPlan, String> {
        self.jtty_tx_gate()?;
        let mut plan = self.jtty_plan_typed(typed, his_call, true, false)?;
        plan.start_live = !self.jtty_tx.latched;
        Ok(plan)
    }

    /// Plan a live commit of `typed` (the text after what is already committed), as WSJT-X's
    /// live entry commits: refused unless a live session is running.
    pub fn jtty_plan_live(
        &mut self,
        typed: &str,
        his_call: &str,
        how: JttyCommit,
    ) -> Result<JttyPlan, String> {
        self.jtty_tx_gate()?;
        if !self.jtty_tx.latched {
            return Err("Live entry is off — start it first".to_string());
        }
        match how {
            JttyCommit::Advance => {
                if complete_words(typed).saturating_sub(HOLDBACK_WORDS) < BATCH_WORDS {
                    return Ok(self.jtty_new_plan(Planned::Nothing));
                }
                self.jtty_plan_typed(typed, his_call, false, false)
            }
            JttyCommit::Flush => self.jtty_plan_typed(typed, his_call, true, false),
        }
    }

    /// Plan the sign-off (WSJT-X's Alt+K): `DE %M K ` added where the typing ends — no space put
    /// before it, as WSJT-X inserts it at the cursor — and all of it sent, ending the message;
    /// the live session ends once it has gone.
    pub fn jtty_plan_sign_off(&mut self, typed: &str, his_call: &str) -> Result<JttyPlan, String> {
        self.jtty_tx_gate()?;
        if !self.jtty_tx.latched {
            return Err("Live entry is off — there is nothing to sign off".to_string());
        }
        let with = format!("{typed}DE %M K ");
        let mut plan = self.jtty_plan_typed(&with, his_call, true, true)?;
        plan.consumed = typed.encode_utf16().count();
        plan.end_live = true;
        Ok(plan)
    }

    /// `commitJttyLiveEntryPlan`: the part of `typed` to commit now (all of it when `force`),
    /// expanded and prepared.
    fn jtty_plan_typed(
        &mut self,
        typed: &str,
        his_call: &str,
        force: bool,
        last: bool,
    ) -> Result<JttyPlan, String> {
        let take = incremental_commit(typed, force, HOLDBACK_WORDS);
        let part = &typed[..take];
        if part.trim().is_empty() {
            let mut plan = self.jtty_new_plan(Planned::Nothing);
            plan.consumed = 0;
            return Ok(plan);
        }
        let ctx = self.jtty_macro_context(his_call);
        let expanded = expand_literal(part, &ctx);
        let prepared = crate::keyboard::kb_filter(crate::keyboard::JTTY, &expanded);
        let substituted = prepared != expanded.replace('\r', "\n");
        let mut plan = self.jtty_new_plan(Planned::Text {
            text: prepared.clone(),
            last,
        });
        plan.consumed = part.encode_utf16().count();
        plan.sent = prepared;
        plan.substituted = substituted;
        Ok(plan)
    }

    /// Commit a rendered plan: the gate again (anything may have changed while it rendered),
    /// nothing stopped since it was planned, and the queue still fitting the watchdog and what the
    /// over has left of its ceiling; then it
    /// is queued (and a live session started or ended as the plan says).
    pub fn jtty_commit(&mut self, plan: JttyPlan, rendered: JttyRendered) -> Result<(), String> {
        self.jtty_tx_gate()?;
        if plan.gen != self.jtty_tx.gen || plan.tx_hz != self.jtty_tx.tx_hz {
            return Err(STOPPED_FOR_PREP.to_string());
        }
        let adding: usize = rendered.segments.iter().map(|s| s.pcm.len()).sum();
        let total_ms = self.jtty_tx.queued_ms() + adding as f64 / 12.0;
        let limit_ms = self.jtty_queue_limit_ms();
        if total_ms > limit_ms {
            return Err(format!(
                "That would queue {:.0} s of JTTY, longer than it may run ({:.0} min: the TX \
                 watchdog, or JTTY's ceiling). Nothing was added; send less at a time.",
                total_ms / 1000.0,
                limit_ms / 60_000.0
            ));
        }
        if let Some(left_ms) = self.jtty_over_left_ms(now_unix_millis()) {
            if total_ms > left_ms as f64 {
                return Err(format!(
                    "That would queue {:.0} s of JTTY, more than this over has left ({:.0} s) \
                     before its {}-minute ceiling. Nothing was added; send less at a time.",
                    total_ms / 1000.0,
                    left_ms as f64 / 1000.0,
                    crate::keyboard::JTTY.max_latch_ms / 60_000
                ));
            }
        }
        if plan.start_live {
            let now = now_unix_millis();
            let JttyTx {
                latched,
                type_buf,
                latch_start_ms,
                ..
            } = &mut self.jtty_tx;
            crate::keyboard::KeyboardLatch::new(
                crate::keyboard::JTTY,
                latched,
                type_buf,
                latch_start_ms,
            )
            .latch_on(now);
            self.jtty_tx.filler = rendered.filler;
            self.jtty_tx.live_keyed = false;
        }
        if !rendered.segments.is_empty() {
            tempo_core::applog::info(
                "tx",
                &format!(
                    "JTTY queued: {:?} ({} segments)",
                    plan.sent,
                    rendered.segments.len()
                ),
            );
        }
        self.jtty_tx.queue.extend(rendered.segments);
        if plan.end_live && self.jtty_tx.latched {
            // Intent down; what is queued still goes, under the ordinary gates.
            self.jtty_tx.latched = false;
        }
        self.jtty_tx.notice = plan.substituted.then(|| SUBSTITUTED.to_string());
        self.jtty_tx.stopped = false;
        // An explicit operator send restarts the TX-watchdog clock.
        self.reset_tx_watchdog();
        Ok(())
    }

    /// While a live session's ceiling stands (from its start until the over drains or is
    /// stopped), the air time the over has left before it (ms; 0 once it is reached).
    fn jtty_over_left_ms(&self, now_ms: u64) -> Option<u64> {
        let start = self.jtty_tx.latch_start_ms?;
        Some(
            crate::keyboard::JTTY
                .max_latch_ms
                .saturating_sub(now_ms.saturating_sub(start)),
        )
    }

    /// The most air time JTTY may have queued: the TX watchdog's limit, and never more than a
    /// live session's ceiling (the watchdog can be turned off; the ceiling cannot).
    fn jtty_queue_limit_ms(&self) -> f64 {
        let ceiling = crate::keyboard::JTTY.max_latch_ms as f64;
        let watchdog = self.settings.tx_watchdog_min as f64 * 60_000.0;
        if watchdog > 0.0 {
            watchdog.min(ceiling)
        } else {
            ceiling
        }
    }

    /// Stop JTTY now: everything queued dropped, the live session ended, and what is on the air
    /// cut (the radio loop flushes and unkeys). Stop TX, Esc, TX Off, the halt and leaving the
    /// screen all come here; it never waits and is never refused.
    pub fn jtty_stop(&mut self) {
        self.jtty_drop(CANCELLED);
    }

    /// The one kill path: drop it all, say why, and arm the cut if anything was going.
    fn jtty_drop(&mut self, why: &str) {
        let was = self.jtty_tx.active();
        self.jtty_tx.queue.clear();
        self.jtty_tx.playing = None;
        self.jtty_tx.filler = None;
        self.jtty_tx.live_keyed = false;
        {
            let JttyTx {
                latched,
                type_buf,
                latch_start_ms,
                ..
            } = &mut self.jtty_tx;
            crate::keyboard::KeyboardLatch::new(
                crate::keyboard::JTTY,
                latched,
                type_buf,
                latch_start_ms,
            )
            .drop_latch();
        }
        self.jtty_tx.gen = self.jtty_tx.gen.wrapping_add(1);
        if was {
            tempo_core::applog::info("tx", &format!("JTTY stopped: {why}"));
            self.jtty_tx.stopped = true;
            self.jtty_tx.notice = Some(why.to_string());
            self.jtty_tx.abort = true;
            // …and the PTT tail past the last frame (see `psk_stop`).
            self.slot_tx_abort = true;
        }
    }

    /// Leaving the JTTY section: WSJT-X stops its transmitter on a mode change
    /// (`noteTxModeChange`), and so does this.
    pub(super) fn jtty_left(&mut self) {
        self.jtty_drop(LEFT_SCREEN);
    }

    /// JTTY was selected: the Tx offset goes back to 1500 Hz with the Rx frequency, as WSJT-X's
    /// selection sets both.
    pub(super) fn jtty_tx_selected(&mut self) {
        self.jtty_tx.tx_hz = JTTY_TX_HZ;
        self.jtty_tx.gen = self.jtty_tx.gen.wrapping_add(1);
    }

    /// The Tx offset (WSJT-X's `TxFreqSpinBox_2`): what UDP Status reports as `tx_df` in JTTY
    /// and where the licence gate judges the emission.
    pub fn jtty_tx_hz(&self) -> f32 {
        self.jtty_tx.tx_hz
    }

    /// Move the Tx offset. Refused while JTTY is transmitting or has anything queued, so what
    /// is on the air and queued is always at the offset the licence gate judges.
    pub fn set_jtty_tx(&mut self, hz: f32) -> Result<(), String> {
        if !(JTTY_TX_MIN_HZ..=JTTY_TX_MAX_HZ).contains(&hz) {
            return Err(format!(
                "JTTY Tx {hz} Hz is outside {JTTY_TX_MIN_HZ}-{JTTY_TX_MAX_HZ} Hz."
            ));
        }
        if self.jtty_tx.active() {
            return Err("Stop the JTTY transmission before moving Tx".to_string());
        }
        self.jtty_tx.tx_hz = hz;
        self.jtty_tx.gen = self.jtty_tx.gen.wrapping_add(1);
        Ok(())
    }

    /// The contest serial number (WSJT-X's `sbSerialNumber_2`; Settings keeps it).
    pub fn jtty_serial(&self) -> u32 {
        self.jtty_tx.serial
    }

    pub fn set_jtty_serial(&mut self, n: u32) -> Result<(), String> {
        if !(JTTY_SERIAL_MIN..=JTTY_SERIAL_MAX).contains(&n) {
            return Err(format!(
                "JTTY serial {n} is outside {JTTY_SERIAL_MIN}-{JTTY_SERIAL_MAX}."
            ));
        }
        self.jtty_tx.serial = n;
        self.settings.jtty_serial = n;
        Ok(())
    }

    /// A restored Settings bundle's serial, held to the spin box's range.
    pub(super) fn adopt_jtty_serial(&mut self, n: u32) {
        self.jtty_tx.serial = n.clamp(JTTY_SERIAL_MIN, JTTY_SERIAL_MAX);
    }

    /// Whether a live session is running (the cockpit's live control).
    pub fn jtty_live(&self) -> bool {
        self.jtty_tx.latched
    }

    /// Anything of JTTY's queued, latched or on the air: the transmitter's owner
    /// ([`Engine::tx_owner`]).
    pub(super) fn jtty_tx_active(&self) -> bool {
        self.jtty_tx.active()
    }

    /// Stamp whether JTTY audio is on the air (the radio loop, every tick). The edges are what
    /// the receive side needs: the reception ends while we transmit (WSJT-X ends it on
    /// Transmission), and a message decoded inside our own keyed time is our own echo.
    pub fn set_jtty_sending(&mut self, on: bool) {
        let now = now_unix_millis() as i64;
        match (self.jtty_tx.sending, on) {
            (false, true) => self.jtty_tx.keyed_since_ms = Some(now),
            (true, false) => {
                if let Some(since) = self.jtty_tx.keyed_since_ms.take() {
                    self.jtty
                        .note_keyed(since - ECHO_MARGIN_MS, now + ECHO_MARGIN_MS);
                }
                // A natural drain says so, as WSJT-X's `finishJttyDrain` does; a stop has said
                // its own piece.
                let spent = self
                    .jtty_tx
                    .playing
                    .as_ref()
                    .is_none_or(|p| p.at >= p.seg.pcm.len());
                if spent && self.jtty_tx.queue.is_empty() && !self.jtty_tx.latched {
                    // The over has ended: a later send is a new one, not under this ceiling.
                    self.jtty_tx.latch_start_ms = None;
                    if !self.jtty_tx.stopped {
                        self.jtty_tx.notice = Some(COMPLETE.to_string());
                    }
                }
            }
            _ => {}
        }
        self.jtty_tx.sending = on;
    }

    /// JTTY audio is on the air.
    pub fn jtty_sending(&self) -> bool {
        self.jtty_tx.sending
    }

    /// While JTTY is keyed, the stretch so far is our own (for the echo rule while it is still
    /// going).
    pub(super) fn jtty_keyed_since_ms(&self) -> Option<i64> {
        self.jtty_tx.keyed_since_ms.map(|s| s - ECHO_MARGIN_MS)
    }

    /// Take + reset the one-shot cut (the loop flushes JTTY's audio and unkeys).
    pub fn take_jtty_abort(&mut self) -> bool {
        std::mem::take(&mut self.jtty_tx.abort)
    }

    /// The radio loop cannot key this tick (a radio switch or a CAT hold): what JTTY has going
    /// is dropped and cut, as a latched keyboard mode's is — a JTTY message cannot resume after
    /// a hole.
    pub fn jtty_cannot_key(&mut self) {
        if self.jtty_tx.active() {
            self.jtty_drop(REFUSED_LOOP);
        }
    }

    /// The rig refused the key for a JTTY frame: nothing was played, and a JTTY message cannot
    /// go on after a hole, so what JTTY has going is dropped, with the reason.
    pub fn jtty_key_refused(&mut self) {
        self.jtty_drop(KEY_REFUSED);
    }

    /// ⚠️ THE RADIO LOOP'S ONE PATH TO JTTY AUDIO, every tick: every gate re-checked while
    /// anything is queued or on the air; a gate down drops it all and cuts it.
    /// With `want` (the loop has less than a frame queued ahead), the next frame — the segment
    /// being fed, the next one queued, or, in a live session with nothing typed waiting, the
    /// filler. `ahead_ms` is how much audio the loop already has queued: a segment's first frame
    /// starts on the air after it, and shows in the window and gets its ALL.TXT line stamped then.
    pub fn poll_jtty(&mut self, want: bool, ahead_ms: f64) -> JttyTick {
        use crate::keyboard::{KbDrop, KbGates, KbTick, KeyboardLatch};
        if !self.jtty_tx.active() {
            return JttyTick::Idle;
        }
        if self.clock_repair_holds_tx() {
            self.jtty_drop(REFUSED_CLOCK);
            return JttyTick::Idle;
        }
        // ⛔ A newly signed-in device holds transmit (the 2026-10-23 sign-in decision): dropped,
        // the same as under a clock repair ([`Engine::remote_transmit_held`]).
        if self.remote_transmit_held() {
            self.jtty_drop(REFUSED_HOLD);
            return JttyTick::Idle;
        }
        let gates = KbGates {
            tx_enabled: self.tx_enabled,
            tx_allowed: self.tx_allowed(),
            tuning: self.tuning,
            in_section: self.jtty_section(),
            now_ms: now_unix_millis(),
            now_secs: now_unix_secs(),
            watchdog_limit_secs: self.settings.tx_watchdog_min as u64 * 60,
            watchdog_start_secs: self.tx_watchdog_start,
        };
        let gate_down = |g: &KbGates| {
            if !g.in_section {
                Some(LEFT_SCREEN)
            } else if !g.tx_enabled {
                Some(REFUSED_TX_OFF)
            } else if g.tuning {
                Some(REFUSED_TUNE)
            } else if !g.tx_allowed {
                Some(REFUSED_PRIVILEGES)
            } else {
                None
            }
        };
        let streaming = crate::keyboard::streaming(self.jtty_tx.latched, &self.jtty_tx.type_buf);
        let tick = if streaming {
            // Live: the shared per-tick predicate, gate for gate (its table is on the method).
            let res = {
                let JttyTx {
                    latched,
                    type_buf,
                    latch_start_ms,
                    ..
                } = &mut self.jtty_tx;
                KeyboardLatch::new(crate::keyboard::JTTY, latched, type_buf, latch_start_ms)
                    .tick(gates, usize::from(want))
            };
            if let Some(start) = res.watchdog_start {
                self.tx_watchdog_start = Some(start);
            }
            match res.drop {
                Some(KbDrop::GateDown) => {
                    self.jtty_drop(gate_down(&gates).unwrap_or(REFUSED_TX_OFF));
                    return JttyTick::Idle;
                }
                Some(KbDrop::Ceiling) => {
                    let why = format!(
                        "JTTY live entry unkeyed at its {}-minute ceiling — start it again to \
                         carry on",
                        crate::keyboard::JTTY.max_latch_ms / 60_000
                    );
                    self.jtty_drop(&why);
                    return JttyTick::Idle;
                }
                Some(KbDrop::Watchdog) => {
                    self.jtty_watchdog_trip();
                    return JttyTick::Idle;
                }
                None => res.tick,
            }
        } else {
            // One-shot: the same gates, and the watchdog, on every tick while it drains.
            if let Some(why) = gate_down(&gates) {
                self.jtty_drop(why);
                return JttyTick::Idle;
            }
            // ⛔ The ceiling bounds the whole over: what a live session left queued when Send or
            // the sign-off ended it stops at the same ten minutes, from when the session started.
            if self.jtty_over_left_ms(gates.now_ms) == Some(0) {
                let why = format!(
                    "JTTY stopped at its {}-minute ceiling for one over, so what was still queued \
                     was dropped. Send it again when you are ready.",
                    crate::keyboard::JTTY.max_latch_ms / 60_000
                );
                self.jtty_drop(&why);
                return JttyTick::Idle;
            }
            if gates.watchdog_limit_secs > 0 {
                let start = *self.tx_watchdog_start.get_or_insert(gates.now_secs);
                if gates.now_secs.saturating_sub(start) >= gates.watchdog_limit_secs {
                    self.jtty_watchdog_trip();
                    return JttyTick::Idle;
                }
            }
            if want {
                KbTick::Diddle
            } else {
                KbTick::Ahead
            }
        };
        match tick {
            KbTick::Ahead => JttyTick::Ahead,
            KbTick::Idle => JttyTick::Idle,
            // JTTY's type-ahead stays empty, so the latch never hands out typed characters:
            // what is typed is committed in segments.
            KbTick::Text(_) | KbTick::Diddle => match self.jtty_next_frame(
                streaming,
                now_unix_millis() as i64 + ahead_ms.max(0.0) as i64,
            ) {
                Some(frame) => JttyTick::Frame(frame),
                None => JttyTick::Idle,
            },
        }
    }

    /// The wall-clock watchdog tripped: TX disarmed, everything dropped and cut.
    fn jtty_watchdog_trip(&mut self) {
        self.tx_watchdog = true;
        self.tx_enabled = false;
        self.jtty_drop(WATCHDOG_TRIPPED);
    }

    /// The next frame: of the segment being fed, else of the next one queued, else (live) of
    /// the filler. A segment's first frame is when it shows and is logged.
    fn jtty_next_frame(&mut self, live: bool, plays_at_ms: i64) -> Option<Vec<f32>> {
        let spent = self
            .jtty_tx
            .playing
            .as_ref()
            .is_none_or(|p| p.at >= p.seg.pcm.len());
        if spent {
            self.jtty_tx.playing = None;
            let bridge = live && self.jtty_tx.live_keyed;
            let next = match self.jtty_tx.queue.pop_front() {
                Some(next) => next,
                None if bridge => self.jtty_tx.filler.clone()?,
                None => return None,
            };
            if !next.filler {
                if live {
                    self.jtty_tx.live_keyed = true;
                }
                self.jtty_segment_started(&next, plays_at_ms);
            }
            self.jtty_tx.playing = Some(Playing { seg: next, at: 0 });
        }
        let p = self.jtty_tx.playing.as_mut()?;
        let end = (p.at + FRAME_SAMPLES).min(p.seg.pcm.len());
        let frame = p.seg.pcm[p.at..end].to_vec();
        p.at = end;
        Some(frame)
    }

    /// A segment starts on the air: WSJT-X's `updateJttyTransmitDisplay` (the sent text in the
    /// QSO Frequency pane and an ALL.TXT `Tx` line) and, for a send's first segment,
    /// `handleJttyContestSerial` (a message starting "TU " moves the serial on).
    fn jtty_segment_started(&mut self, seg: &JttySegment, at_ms: i64) {
        let dial_hz = (self.settings.dial_mhz * 1e6).round().max(0.0) as u64;
        self.jtty.push_sent(at_ms, dial_hz, seg.tx_hz, &seg.text);
        if self.settings.write_all_txt {
            let pending = &mut self.station.all_txt_pending;
            pending.push(crate::alltxt::jtty_tx_line(
                at_ms, dial_hz, seg.tx_hz, &seg.text,
            ));
            let len = pending.len();
            if len > 5000 {
                pending.drain(0..len - 5000);
            }
        }
        if seg.first
            && seg
                .text
                .get(..3)
                .is_some_and(|t| t.eq_ignore_ascii_case("TU "))
        {
            let next = (self.jtty_tx.serial + 1).min(JTTY_SERIAL_MAX);
            self.jtty_tx.serial = next;
            self.settings.jtty_serial = next;
        }
    }

    /// The cockpit's view of the transmitter.
    pub(super) fn jtty_tx_state(&self) -> crate::dto::JttyTxState {
        let (left, pending) = self.jtty_tx.pending();
        crate::dto::JttyTxState {
            refusal: self.jtty_tx_gate().err(),
            tx_hz: self.jtty_tx.tx_hz,
            serial: self.jtty_tx.serial,
            sending: self.jtty_tx.sending,
            live: self.jtty_tx.latched,
            left,
            pending,
            notice: self.jtty_tx.notice.clone(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The three steps together, for the commands and the tests: plan under the lock, render with it
// released, commit under it again.
// ---------------------------------------------------------------------------------------------

/// What a send did: how many characters of the typed text it took, and what went in their place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JttySent {
    pub consumed: usize,
    pub sent: String,
}

fn run(
    engine: &Mutex<Engine>,
    plan: impl FnOnce(&mut Engine) -> Result<JttyPlan, String>,
) -> Result<JttySent, String> {
    run_bound(engine, plan, |_| Ok(()), |_| {})
}

/// ⛔ A send whose caller binds it: `bound` is asked first in each of the two holds of the engine
/// lock, the plan's and the commit's, and refuses the send with its error, so nothing is queued
/// once it has stopped holding (a computer driving the station that pressed before a Stop or the
/// loss of its permission to transmit). `started` runs after the commit, in the same hold. A plan
/// that queues nothing commits nothing, and `started` is not run.
pub fn run_bound(
    engine: &Mutex<Engine>,
    plan: impl FnOnce(&mut Engine) -> Result<JttyPlan, String>,
    bound: impl Fn(&Engine) -> Result<(), String>,
    started: impl FnOnce(&mut Engine),
) -> Result<JttySent, String> {
    let plan = {
        let mut e = engine_lock(engine);
        bound(&e)?;
        plan(&mut e)?
    };
    let done = JttySent {
        consumed: plan.consumed,
        sent: plan.sent.clone(),
    };
    if matches!(plan.what, Planned::Nothing) && !plan.start_live && !plan.end_live {
        return Ok(done);
    }
    let rendered = render(&plan)?;
    let mut e = engine_lock(engine);
    bound(&e)?;
    e.jtty_commit(plan, rendered)?;
    started(&mut e);
    Ok(done)
}

/// Send typed text (WSJT-X's Send): see [`Engine::jtty_plan_send`].
pub fn send_text(engine: &Mutex<Engine>, text: &str, his_call: &str) -> Result<JttySent, String> {
    run(engine, |e| e.jtty_plan_send(text, his_call))
}

/// Send F-key `n`: see [`Engine::jtty_plan_fkey`].
pub fn send_fkey(engine: &Mutex<Engine>, n: u8, his_call: &str) -> Result<JttySent, String> {
    run(engine, |e| e.jtty_plan_fkey(n, his_call))
}

/// Start live entry with what is typed so far: see [`Engine::jtty_plan_live_start`].
pub fn live_start(engine: &Mutex<Engine>, typed: &str, his_call: &str) -> Result<JttySent, String> {
    run(engine, |e| e.jtty_plan_live_start(typed, his_call))
}

/// Commit live typing: see [`Engine::jtty_plan_live`].
pub fn live_commit(
    engine: &Mutex<Engine>,
    typed: &str,
    his_call: &str,
    how: JttyCommit,
) -> Result<JttySent, String> {
    run(engine, |e| e.jtty_plan_live(typed, his_call, how))
}

/// Sign off: see [`Engine::jtty_plan_sign_off`].
pub fn sign_off(engine: &Mutex<Engine>, typed: &str, his_call: &str) -> Result<JttySent, String> {
    run(engine, |e| e.jtty_plan_sign_off(typed, his_call))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::jtty_rx::JttyDriver;

    /// An engine on the JTTY screen, 20 m, TX armed, W9XYZ.
    fn jtty_engine() -> Mutex<Engine> {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_operating_mode("jtty", false);
        e.set_frequency(14.090, "20m", "USB");
        e.set_tx_enabled(true);
        Mutex::new(e)
    }

    /// Frame by frame, everything the radio loop would be handed until JTTY has nothing more.
    fn drain(engine: &Mutex<Engine>) -> Vec<f32> {
        let mut out = Vec::new();
        for _ in 0..1000 {
            let tick = engine_lock(engine).poll_jtty(true, 0.0);
            match tick {
                JttyTick::Frame(f) => out.extend(f),
                JttyTick::Ahead => panic!("a frame was asked for"),
                JttyTick::Idle => return out,
            }
        }
        panic!("JTTY never ran dry")
    }

    /// `n` frames, whatever they are.
    fn frames(engine: &Mutex<Engine>, n: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for _ in 0..n {
            let tick = engine_lock(engine).poll_jtty(true, 0.0);
            match tick {
                JttyTick::Frame(f) => out.extend(f),
                other => panic!("wanted a frame, got {other:?}"),
            }
        }
        out
    }

    /// What OUR OWN receiver makes of `audio`: the engine's tap and decode driver, as the app
    /// runs them, over two seconds of silence before and three after. The receive window's
    /// lines, `(text, state)`, each as the window shows it (filler gone, spaces collapsed).
    fn receive(audio: &[f32]) -> Vec<(String, String)> {
        let mut pcm = vec![0.0f32; 24_000];
        pcm.extend(audio.iter().map(|s| s * 0.3));
        pcm.extend(vec![0.0f32; 36_000]);
        let rx = Mutex::new(Engine::new("K1ABC", "FN42", 0));
        engine_lock(&rx).set_jtty_armed(true);
        let _contexts = crate::engine::jtty_rx::receive_contexts();
        let mut driver = JttyDriver::default();
        let t0 = 1_700_000_000_000i64;
        for (i, chunk) in pcm.chunks(1200).enumerate() {
            engine_lock(&rx).jtty_feed(chunk, t0 + (i as i64 + 1) * 100);
            driver.tick(&rx);
        }
        engine_lock(&rx).set_jtty_armed(false);
        driver.tick(&rx);
        let lines = engine_lock(&rx).jtty_state().lines;
        lines.into_iter().map(|l| (l.text, l.state)).collect()
    }

    fn complete(lines: &[(String, String)]) -> Vec<&str> {
        lines
            .iter()
            .filter(|(_, s)| s == "complete")
            .map(|(t, _)| t.as_str())
            .collect()
    }

    fn sent_lines(engine: &Mutex<Engine>) -> Vec<String> {
        engine_lock(engine)
            .jtty_state()
            .lines
            .into_iter()
            .filter(|l| l.tx)
            .map(|l| l.text)
            .collect()
    }

    /// ⛔ A BOUND SEND QUEUES NOTHING ONCE ITS BINDING LAPSES, even while it renders. Bound to the
    /// transmit stand-down as it was when pressed: the stand-down revoked after the plan, while the
    /// engine lock is released for rendering, refuses the commit with the binding's own error, and
    /// nothing is queued, owned or noted as started; revoked before the plan, nothing is planned.
    /// CONTROL: with the stand-down kept, the same send queues and is noted as started.
    #[test]
    fn a_bound_send_queues_nothing_once_its_binding_lapses_while_it_renders() {
        use crate::remote_control::transmit::TransmitAuthority;
        use std::sync::atomic::{AtomicBool, Ordering};
        const LAPSED: &str = "stopped since it was pressed";
        for lapse in ["while it renders", "before it is planned", "never"] {
            let e = jtty_engine();
            let transmit = TransmitAuthority::default();
            engine_lock(&e).set_remote_transmit_revocation(transmit.revocation());
            let pressed = engine_lock(&e).remote_transmit_stand_down_generation();
            let bound = |e: &Engine| {
                if e.remote_transmit_stand_down_generation() == pressed {
                    Ok(())
                } else {
                    Err(LAPSED.to_string())
                }
            };
            if lapse == "before it is planned" {
                transmit.revoke();
            }
            let planned = AtomicBool::new(false);
            let started = AtomicBool::new(false);
            let sent = run_bound(
                &e,
                |g| {
                    planned.store(true, Ordering::SeqCst);
                    let plan = g.jtty_plan_send("CQ W9XYZ", "");
                    if lapse == "while it renders" {
                        transmit.revoke();
                    }
                    plan
                },
                bound,
                |_| started.store(true, Ordering::SeqCst),
            );
            let mut g = engine_lock(&e);
            let left = g.jtty_tx_state().left;
            if lapse == "never" {
                assert!(sent.is_ok(), "CONTROL: {sent:?}");
                assert_eq!(left, 1, "CONTROL: queued");
                assert!(started.load(Ordering::SeqCst), "CONTROL: noted as started");
                continue;
            }
            assert_eq!(sent, Err(LAPSED.to_string()), "{lapse}");
            assert_eq!(
                planned.load(Ordering::SeqCst),
                lapse == "while it renders",
                "{lapse}: planned"
            );
            assert_eq!(left, 0, "{lapse}: queued");
            assert_eq!(g.tx_owner(), None, "{lapse}: the transmitter owned");
            assert_eq!(g.poll_jtty(true, 0.0), JttyTick::Idle, "{lapse}: keyed");
            assert!(!started.load(Ordering::SeqCst), "{lapse}: noted as started");
        }
    }

    /// ⛔ THE TX-ENABLE LATCH. Off, every way to send is refused with the latch's own words, and
    /// nothing is queued, owned or handed to the loop. The positive control: the same engine with
    /// TX on sends.
    #[test]
    fn with_tx_off_every_send_is_refused_and_nothing_reaches_the_loop() {
        let e = jtty_engine();
        engine_lock(&e).set_tx_enabled(false);
        let refused = |r: Result<JttySent, String>, what: &str| {
            let why = r.map(|_| ()).unwrap_err();
            assert!(why.starts_with("TX is off"), "{what}: {why}");
        };
        refused(send_text(&e, "CQ W9XYZ", ""), "Send");
        for n in 1..=8 {
            refused(send_fkey(&e, n, "W1AW"), &format!("F{n}"));
        }
        refused(live_start(&e, "HELLO ", ""), "Live");
        for how in [JttyCommit::Advance, JttyCommit::Flush] {
            refused(
                live_commit(&e, "A B C D E F G H I ", "", how),
                "a live commit",
            );
        }
        refused(sign_off(&e, "", ""), "Sign off");
        {
            let mut g = engine_lock(&e);
            assert_eq!(g.tx_owner(), None, "nothing owns the transmitter");
            assert_eq!(
                g.poll_jtty(true, 0.0),
                JttyTick::Idle,
                "nothing reaches the loop"
            );
            assert!(!g.take_jtty_abort());
            let st = g.jtty_tx_state();
            assert!(st.refusal.as_deref().unwrap().starts_with("TX is off"));
            assert_eq!((st.left, st.live, st.sending), (0, false, false));
        }
        // Control: TX on, the same send goes.
        engine_lock(&e).set_tx_enabled(true);
        assert!(send_text(&e, "CQ W9XYZ", "").is_ok());
        assert_eq!(engine_lock(&e).tx_owner(), Some(TxOwner::Jtty));
        assert!(matches!(
            engine_lock(&e).poll_jtty(true, 0.0),
            JttyTick::Frame(_)
        ));
    }

    /// ⛔ …AND THE GATES AGAIN BEFORE EVERY FRAME. A gate that goes down with JTTY on the air drops
    /// everything queued and cuts it at the loop's next hand-out, however it went down: the latch
    /// falling without TX Off's own stop, or the licence gate closing (here a class change that
    /// takes 20 m data away). The gate a send passed is not the only one.
    #[test]
    fn the_gates_are_read_again_before_every_frame() {
        type Down = fn(&mut Engine);
        let downs: [(&str, Down, &str); 2] = [
            ("the latch", |g| g.tx_enabled = false, REFUSED_TX_OFF),
            (
                "the licence",
                |g| {
                    g.set_license_class("technician");
                },
                REFUSED_PRIVILEGES,
            ),
        ];
        for (what, down, why) in downs {
            let e = jtty_engine();
            send_text(&e, "CQ CQ DE W9XYZ W9XYZ K", "").unwrap();
            let mut g = engine_lock(&e);
            assert!(
                matches!(g.poll_jtty(true, 0.0), JttyTick::Frame(_)),
                "{what}: control"
            );
            down(&mut g);
            assert_eq!(g.poll_jtty(true, 0.0), JttyTick::Idle, "{what}");
            assert!(g.take_jtty_abort(), "{what}: what was on the air is cut");
            assert_eq!(g.tx_owner(), None, "{what}: and nothing is left queued");
            assert_eq!(g.jtty_tx_state().notice.as_deref(), Some(why), "{what}");
        }
    }

    /// WSJT-X'S SEGMENTS (`execute_jtty_tx`): a line longer than the encoder's 80 characters is
    /// split at a space, every segment but a line's last sent OPEN (no end-of-message, so the
    /// receiver grows one message across them), and a line break ends a message. Each segment
    /// shows in the window and gets an ALL.TXT `Tx` line as it starts on the air.
    ///
    /// (What a receiver makes of a line longer than 80 characters is WSJT-X 3.3.0-beta4's own
    /// limit, not this sender's: its decoder keeps 16 frames and 80 characters of a message, ours
    /// and stock `rjtty` alike — measured in the lab, so it is not asserted here.)
    #[test]
    fn a_long_line_goes_out_in_wsjtx_segments() {
        let e = jtty_engine();
        engine_lock(&e).settings.write_all_txt = true;
        let first = "CQ CQ CQ DE W9XYZ W9XYZ W9XYZ PSE K THIS IS A LONGER LINE THAN EIGHTY CHARS \
                     SO IT SPLITS";
        assert_eq!(first.len(), 88);
        let done = send_text(&e, &format!("{first}\nTU 73"), "").unwrap();
        assert_eq!(done.consumed, first.len() + 6);
        let segs: Vec<(String, bool)> = engine_lock(&e)
            .jtty_tx
            .queue
            .iter()
            .map(|s| (s.text.clone(), s.last))
            .collect();
        assert_eq!(
            segs,
            vec![
                (
                    "CQ CQ CQ DE W9XYZ W9XYZ W9XYZ PSE K THIS IS A LONGER LINE THAN EIGHTY CHARS SO"
                        .to_string(),
                    false
                ),
                ("IT SPLITS".to_string(), true),
                ("TU 73".to_string(), true),
            ],
            "split after the last space that fits, open until the line ends"
        );
        let air = drain(&e);
        assert_eq!(
            air.len() % FRAME_SAMPLES,
            0,
            "whole frames, no gap, no padding"
        );
        assert_eq!(sent_lines(&e).len(), 3);
        let txt = engine_lock(&e).take_all_txt_pending();
        assert_eq!(txt.len(), 3, "{txt:?}");
        assert!(
            txt.iter().all(|l| l.contains(" Tx JTTY     0  0.0 1500 ")),
            "{txt:?}"
        );
        assert!(
            txt[2].ends_with(" Tx JTTY     0  0.0 1500 TU 73"),
            "{txt:?}"
        );
    }

    /// A LINE THAT WILL NOT ENCODE REFUSES THE WHOLE SEND, as WSJT-X 3.3.0-beta4 refuses it: no
    /// shorter retry, nothing queued, nothing keyed, and the operator is told why. Under RTTY
    /// Roundup's profile each `599 1` packs as `599 001`, so these thirteen (77 characters typed)
    /// overflow the encoder's 80 while ten of them fit: the line the old retry sent in two
    /// pieces. The control: a line that fits, under the same profile, sends.
    #[test]
    fn a_line_that_will_not_encode_refuses_the_whole_send() {
        let roundup = |text: &'static str| {
            move |g: &mut Engine| {
                let mut plan = g.jtty_plan_send(text, "")?;
                plan.profile = jtty::Profile::RttyRoundup;
                Ok(plan)
            }
        };
        let e = jtty_engine();
        let why = run(
            &e,
            roundup(
                "599 1 599 1 599 1 599 1 599 1 599 1 599 1 599 1 599 1 599 1 599 1 599 1 599 1",
            ),
        )
        .unwrap_err();
        assert_eq!(why, REFUSED_TOO_LONG);
        {
            let mut g = engine_lock(&e);
            assert_eq!(g.jtty_tx.queue.len(), 0, "nothing queued");
            assert_eq!(g.tx_owner(), None, "nothing owns the transmitter");
            assert_eq!(g.poll_jtty(true, 0.0), JttyTick::Idle, "nothing keyed");
        }
        // Control: a line that fits, under the same profile, goes.
        run(&e, roundup("599 1 599 2")).unwrap();
        let segs: Vec<String> = engine_lock(&e)
            .jtty_tx
            .queue
            .iter()
            .map(|s| s.text.clone())
            .collect();
        assert_eq!(segs, vec!["599 001 599 002".to_string()]);
        assert_eq!(engine_lock(&e).tx_owner(), Some(TxOwner::Jtty));
    }

    /// ENCODE → OUR OWN RECEIVE: what the transmitter puts out for a two-line send, read by the
    /// app's own receive path, is the two messages that were typed, in order.
    #[test]
    fn a_send_reads_back_as_typed() {
        let e = jtty_engine();
        send_text(&e, "CQ CQ CQ DE W9XYZ W9XYZ K\nqrz? 73", "").unwrap();
        let heard = receive(&drain(&e));
        assert_eq!(
            complete(&heard),
            vec!["CQ CQ CQ DE W9XYZ W9XYZ K", "QRZ? 73"],
            "{heard:?}"
        );
    }

    /// THE QUEUE: sends chain in the order they were made, with no gap, and what the receiver
    /// reads is in that order.
    #[test]
    fn queued_sends_chain_in_order() {
        let e = jtty_engine();
        send_text(&e, "CQ W9XYZ", "").unwrap();
        send_fkey(&e, 2, "W1AW").unwrap();
        send_text(&e, "QRZ?", "").unwrap();
        let st = engine_lock(&e).jtty_tx_state();
        assert_eq!(st.pending, "CQ W9XYZ\nW1AW 599 001\nQRZ?");
        let heard = receive(&drain(&e));
        assert_eq!(
            complete(&heard),
            vec!["CQ W9XYZ", "W1AW 599 001", "QRZ?"],
            "{heard:?}"
        );
    }

    /// THE F-KEYS: WSJT-X's eight Contest templates, compiled into the atoms WSJT-X sends (one
    /// frame each), read back by our receiver as WSJT-X's text. A call the template needs that is
    /// not a standard call REFUSES the key: never sent as literal text instead.
    #[test]
    fn the_f_keys_are_wsjtxs_contest_templates() {
        let e = jtty_engine();
        let want = [
            (1, "CQ W9XYZ CQ", 1),
            (2, "W1AW 599 007", 2),
            (3, "W1AW TU CQ W9XYZ CQ", 2),
            (4, "W9XYZ", 1),
            (5, "W1AW", 1),
            (6, "TU NOW W1AW 599 007", 2),
            (7, "W1AW AGN?", 1),
            (8, "599 007", 1),
        ];
        for (n, text, frames) in want {
            // F6 moves the serial on (its "TU "): each key starts from the same one.
            engine_lock(&e).set_jtty_serial(7).unwrap();
            let done = send_fkey(&e, n, "w1aw").unwrap();
            assert_eq!(done.sent, text, "F{n}");
            let air = drain(&e);
            assert_eq!(air.len(), frames * FRAME_SAMPLES, "F{n}: one frame an atom");
            let heard = receive(&air);
            assert_eq!(complete(&heard), vec![text], "F{n}: {heard:?}");
        }
        for (n, his, why) in [
            (2, "", "put it in the log strip's Call first"),
            (5, "W1AW/P", "is not a standard callsign"),
            (7, "Q1ABC", "is not a standard callsign"),
        ] {
            let err = send_fkey(&e, n, his).unwrap_err();
            assert!(err.contains(why), "F{n} with {his:?}: {err}");
        }
        assert_eq!(
            engine_lock(&e).tx_owner(),
            None,
            "a refused key queued nothing"
        );
        // My own call, when it is not a standard one.
        engine_lock(&e).settings.mycall = "W9XYZ/7".into();
        for n in [1, 3, 4] {
            let err = send_fkey(&e, n, "W1AW").unwrap_err();
            assert!(err.contains("your callsign"), "F{n}: {err}");
        }
        assert!(send_fkey(&e, 9, "W1AW").is_err());
    }

    /// THE SERIAL, as WSJT-X moves it (`handleJttyContestSerial`): on after a send whose first
    /// segment starts "TU " (either case), when it starts on the air — F6's "TU NOW" — and not
    /// after any other. Settings keeps it.
    #[test]
    fn the_serial_moves_on_after_a_tu_message() {
        let e = jtty_engine();
        send_fkey(&e, 2, "W1AW").unwrap();
        drain(&e);
        assert_eq!(engine_lock(&e).jtty_serial(), 1, "F2 keeps it");
        send_fkey(&e, 6, "W1AW").unwrap();
        assert_eq!(
            engine_lock(&e).jtty_serial(),
            1,
            "not before it is on the air"
        );
        frames(&e, 1);
        assert_eq!(engine_lock(&e).jtty_serial(), 2, "F6 moves it on");
        assert_eq!(
            engine_lock(&e).settings.jtty_serial,
            2,
            "and Settings keeps it"
        );
        drain(&e);
        send_text(&e, "tu 73", "").unwrap();
        drain(&e);
        assert_eq!(engine_lock(&e).jtty_serial(), 3, "typed, lower case");
        send_text(&e, "W1AW TU", "").unwrap();
        drain(&e);
        assert_eq!(engine_lock(&e).jtty_serial(), 3, "only at the start");
    }

    /// The JTTY screen in a contest session: `event` ("" is ARRL Field Day), class `class`,
    /// section WI, TX armed.
    fn jtty_in_contest(event: &str, class: &str) -> Mutex<Engine> {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        let mut s = e.settings().clone();
        s.fd_active = true;
        s.fd_event = event.into();
        s.fd_class = class.into();
        s.fd_section = "WI".into();
        e.apply_settings(s);
        e.set_mode("fieldday-run").unwrap();
        e.set_operating_mode("jtty", false);
        e.set_frequency(14.090, "20m", "USB");
        e.set_tx_enabled(true);
        Mutex::new(e)
    }

    /// THE EXCHANGE PROFILE, as WSJT-X takes it from its special operating activity
    /// (`jttyExchangeProfile`): in ARRL Field Day the exchange is the class and section, and the
    /// F-keys send it as JTTY's class/section atom (`nativeExchange`), which a receiver reads back
    /// as typed; `%E` in typed text is the same words. Outside a contest it is the serial, as
    /// before.
    #[test]
    fn arrl_field_day_sends_its_class_and_section() {
        let e = jtty_in_contest("arrlfd", "3A");
        assert_eq!(
            engine_lock(&e).jtty_exchange(),
            (jtty::Profile::FieldDay, "3A WI".to_string())
        );
        send_fkey(&e, 2, "W1AW").unwrap();
        assert_eq!(complete(&receive(&drain(&e))), ["W1AW 3A WI"]);
        assert_eq!(sent_lines(&e), ["W1AW 3A WI"]);
        send_fkey(&e, 6, "W1AW").unwrap();
        send_fkey(&e, 8, "").unwrap();
        assert_eq!(
            complete(&receive(&drain(&e))),
            ["TU NOW W1AW 3A WI", "3A WI"]
        );
        let typed = send_text(&e, "W1AW %E", "").unwrap();
        assert_eq!(typed.sent, "W1AW 3A WI");
        // The blank event is a fresh install's contest, ARRL Field Day.
        let e = jtty_in_contest("", "1D");
        assert_eq!(
            engine_lock(&e).jtty_exchange(),
            (jtty::Profile::FieldDay, "1D WI".to_string())
        );
        // Control: no contest running, the serial.
        let e = jtty_engine();
        assert_eq!(
            engine_lock(&e).jtty_exchange(),
            (jtty::Profile::Unknown, String::new())
        );
        assert_eq!(send_text(&e, "W1AW %E", "").unwrap().sent, "W1AW 001");
    }

    /// WINTER FIELD DAY HAS NO PROFILE (WSJT-X has no activity for it, and JTTY's class/section
    /// atom carries the classes A to F only, where Winter Field Day's are H, I, O and M): `%E` is
    /// the serial, and an exchange typed out goes as text, which a receiver reads back as typed.
    #[test]
    fn winter_field_day_exchanges_go_as_text() {
        let e = jtty_in_contest("wfd", "1O");
        assert_eq!(
            engine_lock(&e).jtty_exchange(),
            (jtty::Profile::Unknown, String::new())
        );
        send_text(&e, "W1AW 1O WI", "").unwrap();
        assert_eq!(complete(&receive(&drain(&e))), ["W1AW 1O WI"]);
    }

    /// A Field Day exchange the class/section atom cannot carry refuses the F-key with WSJT-X's
    /// reason, and nothing is sent.
    #[test]
    fn a_field_day_exchange_the_atom_cannot_carry_refuses_the_key() {
        let e = jtty_in_contest("arrlfd", "40A");
        let err = send_fkey(&e, 2, "W1AW").unwrap_err();
        assert!(err.contains("between 1 and 32"), "{err}");
        assert_eq!(
            engine_lock(&e).tx_owner(),
            None,
            "a refused key queued nothing"
        );
    }

    /// LIVE ENTRY: starting it keys nothing on its own; a commit goes out with the message
    /// left open; when it has gone the FILLER holds the transmitter, and goes on doing so; the
    /// sign-off ends the message and the session, and the transmitter then runs dry. Our receiver
    /// reads ONE message, the filler gone from it, and the window shows no filler line.
    #[test]
    fn live_entry_holds_the_carrier_with_the_filler_and_signs_off() {
        let e = jtty_engine();
        live_start(&e, "", "").unwrap();
        assert!(engine_lock(&e).jtty_live());
        assert_eq!(
            engine_lock(&e).poll_jtty(true, 0.0),
            JttyTick::Idle,
            "armed, nothing typed: nothing keyed (WSJT-X's Alt+J on an empty box)"
        );
        assert_eq!(engine_lock(&e).tx_owner(), Some(TxOwner::Jtty));
        let done = live_commit(&e, "HELLO THERE ", "", JttyCommit::Flush).unwrap();
        assert_eq!(done.consumed, 12);
        let mut air = frames(&e, 3); // "HELLO THERE" is three frames
        let filler_frames = {
            let f = jtty::encode_segment(FILLER, jtty::Profile::Unknown, false).unwrap();
            f.frames()
        };
        // Nothing typed waiting: the filler, and again, for as long as the session runs.
        air.extend(frames(&e, 2 * filler_frames));
        assert_eq!(
            engine_lock(&e).jtty_tx_state().left,
            0,
            "the filler is never counted"
        );
        sign_off(&e, "", "").unwrap();
        assert!(
            !engine_lock(&e).jtty_live(),
            "the sign-off ends the session"
        );
        air.extend(drain(&e));
        let heard = receive(&air);
        assert_eq!(
            complete(&heard),
            vec!["HELLO THERE DE W9XYZ K"],
            "{heard:?}"
        );
        assert_eq!(
            sent_lines(&e),
            vec!["HELLO THERE".to_string(), "DE W9XYZ K".to_string()],
            "the filler is never shown"
        );
    }

    /// A line break ends a message even when what is committed leaves the message open (a live
    /// commit): the line before it goes with end-of-message, the line after it open
    /// (`execute_jtty_tx`: every line but the last ends its message).
    #[test]
    fn a_line_break_ends_a_message_even_in_a_live_commit() {
        let e = jtty_engine();
        live_start(&e, "", "").unwrap();
        live_commit(&e, "LINE ONE\nLINE TWO", "", JttyCommit::Flush).unwrap();
        let segs: Vec<(String, bool)> = engine_lock(&e)
            .jtty_tx
            .queue
            .iter()
            .map(|s| (s.text.clone(), s.last))
            .collect();
        assert_eq!(
            segs,
            vec![
                ("LINE ONE".to_string(), true),
                ("LINE TWO".to_string(), false)
            ]
        );
    }

    /// WSJT-X's live commits: while typing, nothing until five words are complete beyond the
    /// three held back, then those five; after a pause, all of it, the word in progress too.
    #[test]
    fn live_commits_hold_back_an_atom_in_progress() {
        let e = jtty_engine();
        live_start(&e, "", "").unwrap();
        let seven = "ONE TWO THREE FOUR FIVE SIX SEVEN ";
        let done = live_commit(&e, seven, "", JttyCommit::Advance).unwrap();
        assert_eq!(done.consumed, 0, "seven complete words: nothing yet");
        let eight = "ONE TWO THREE FOUR FIVE SIX SEVEN EIGHT NI";
        let done = live_commit(&e, eight, "", JttyCommit::Advance).unwrap();
        assert_eq!(
            (done.consumed, done.sent.as_str()),
            (24, "ONE TWO THREE FOUR FIVE "),
            "eight complete words: the first five"
        );
        let rest = &eight[24..];
        let done = live_commit(&e, rest, "", JttyCommit::Flush).unwrap();
        assert_eq!(
            done.consumed,
            rest.len(),
            "a pause flushes the word in progress too"
        );
        assert_eq!(engine_lock(&e).jtty_tx_state().left, 2);
        // A commit needs a running session.
        engine_lock(&e).jtty_stop();
        assert!(live_commit(&e, "MORE ", "", JttyCommit::Flush).is_err());
    }

    /// STOP DRAIN: Stop (and the halt, and TX Off) drops everything queued, ends a live session
    /// and arms the cut, with WSJT-X's words; nothing more is handed to the loop.
    #[test]
    fn every_stop_drops_the_queue_and_cuts_the_air() {
        for stop in ["stop", "halt", "tx off"] {
            let e = jtty_engine();
            live_start(&e, "", "").unwrap();
            live_commit(&e, "CQ CQ DE W9XYZ ", "", JttyCommit::Flush).unwrap();
            send_text(&e, "AND MORE", "").unwrap();
            frames(&e, 1);
            {
                let mut g = engine_lock(&e);
                match stop {
                    "stop" => g.jtty_stop(),
                    "halt" => g.halt_tx(),
                    _ => g.set_tx_enabled(false),
                }
                assert!(g.take_jtty_abort(), "{stop}: the cut is armed");
                assert!(g.take_slot_tx_abort(), "{stop}: and the PTT tail's");
                assert_eq!(g.poll_jtty(true, 0.0), JttyTick::Idle, "{stop}");
                assert_eq!(g.tx_owner(), None, "{stop}: nothing left");
                assert!(!g.jtty_live(), "{stop}: the live session is over");
                assert_eq!(
                    g.jtty_tx_state().notice.as_deref(),
                    Some(CANCELLED),
                    "{stop}"
                );
            }
        }
        // A stop with nothing going arms nothing (a one-shot cut must not catch a later over).
        let e = jtty_engine();
        engine_lock(&e).jtty_stop();
        assert!(!engine_lock(&e).take_jtty_abort());
    }

    /// THE WATCHDOG: a live session left running on the filler trips it (TX disarmed, everything
    /// dropped and cut); so does a one-shot queue still going past the limit.
    #[test]
    fn the_watchdog_trips_what_is_left_running() {
        for live in [true, false] {
            let e = jtty_engine();
            engine_lock(&e).settings.tx_watchdog_min = 1;
            if live {
                live_start(&e, "HI ", "").unwrap();
            } else {
                send_text(&e, "CQ CQ CQ DE W9XYZ W9XYZ", "").unwrap();
            }
            frames(&e, 1);
            let mut g = engine_lock(&e);
            g.tx_watchdog_start = Some(now_unix_secs() - 59);
            assert!(
                matches!(g.poll_jtty(true, 0.0), JttyTick::Frame(_)),
                "under the limit"
            );
            g.tx_watchdog_start = Some(now_unix_secs() - 61);
            assert_eq!(g.poll_jtty(true, 0.0), JttyTick::Idle, "live {live}");
            assert!(
                g.tx_watchdog && !g.tx_enabled,
                "live {live}: tripped and disarmed"
            );
            assert!(g.take_jtty_abort(), "live {live}: cut");
            assert_eq!(g.tx_owner(), None);
            assert_eq!(g.jtty_tx_state().notice.as_deref(), Some(WATCHDOG_TRIPPED));
        }
    }

    /// THE CEILING: a live session ends at ten minutes however much is typed into it.
    #[test]
    fn the_ceiling_ends_a_live_session() {
        let e = jtty_engine();
        live_start(&e, "HI ", "").unwrap();
        frames(&e, 1);
        let mut g = engine_lock(&e);
        let max = crate::keyboard::JTTY.max_latch_ms;
        g.jtty_tx.latch_start_ms = Some(now_unix_millis() - max + 1_000);
        assert!(
            matches!(g.poll_jtty(true, 0.0), JttyTick::Frame(_)),
            "under it"
        );
        g.jtty_tx.latch_start_ms = Some(now_unix_millis() - max);
        assert_eq!(g.poll_jtty(true, 0.0), JttyTick::Idle);
        assert!(g.take_jtty_abort());
        assert!(!g.jtty_live());
        assert!(g.jtty_tx_state().notice.unwrap().contains("ceiling"));
    }

    /// ⛔ THE CEILING BOUNDS THE WHOLE OVER, not only the part with live entry up: once Send or
    /// the sign-off ends a live session, what it left queued is stopped at the same ten minutes,
    /// counted from when the session started. One second under it, the drain goes on.
    /// CONTROL: a session still in live entry, past the ceiling, is stopped as it always was.
    #[test]
    fn the_ceiling_bounds_what_a_live_session_left_queued() {
        let long = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG ".repeat(4);
        let max = crate::keyboard::JTTY.max_latch_ms;
        for ended_by in ["sign-off", "send", "still live"] {
            for under in [true, false] {
                let e = jtty_engine();
                live_start(&e, "", "").unwrap();
                live_commit(&e, "HELLO THERE ", "", JttyCommit::Flush).unwrap();
                frames(&e, 1);
                match ended_by {
                    "sign-off" => sign_off(&e, &long, "").map(|_| ()).unwrap(),
                    "send" => send_text(&e, &long, "").map(|_| ()).unwrap(),
                    _ => {}
                }
                let mut g = engine_lock(&e);
                assert_eq!(g.jtty_live(), ended_by == "still live", "{ended_by}");
                let elapsed = if under { max - 1_000 } else { max };
                g.jtty_tx.latch_start_ms = Some(now_unix_millis() - elapsed);
                let tick = g.poll_jtty(true, 0.0);
                if under {
                    assert!(
                        matches!(tick, JttyTick::Frame(_)),
                        "{ended_by}, under the ceiling: {tick:?}"
                    );
                    assert!(!g.take_jtty_abort(), "{ended_by}, under the ceiling: cut");
                    continue;
                }
                assert_eq!(tick, JttyTick::Idle, "{ended_by}: keyed past the ceiling");
                assert!(g.take_jtty_abort(), "{ended_by}: cut");
                assert_eq!(g.tx_owner(), None, "{ended_by}: the transmitter owned");
                assert_eq!(g.jtty_tx_state().left, 0, "{ended_by}: still queued");
                assert!(
                    g.jtty_tx_state()
                        .notice
                        .unwrap()
                        .contains("10-minute ceiling"),
                    "{ended_by}: the notice"
                );
                assert!(
                    !g.tx_watchdog && g.tx_enabled,
                    "{ended_by}: not the watchdog"
                );
            }
        }
    }

    /// A send that would run a live session's over past its ceiling is refused whole before
    /// anything of it is queued, with the reason; nothing already going is touched.
    /// CONTROL: the same sign-off with the session just started is queued.
    #[test]
    fn a_send_that_would_outrun_the_overs_ceiling_is_refused() {
        let long = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG ".repeat(4);
        let max = crate::keyboard::JTTY.max_latch_ms;
        for late in [true, false] {
            let e = jtty_engine();
            live_start(&e, "", "").unwrap();
            if late {
                engine_lock(&e).jtty_tx.latch_start_ms = Some(now_unix_millis() - max + 5_000);
            }
            let sent = sign_off(&e, &long, "");
            let mut g = engine_lock(&e);
            if !late {
                assert!(sent.is_ok(), "CONTROL: {sent:?}");
                assert!(g.jtty_tx_state().left > 0, "CONTROL: queued");
                continue;
            }
            let why = sent.map(|_| ()).unwrap_err();
            assert!(why.contains("10-minute ceiling"), "{why}");
            assert_eq!(g.jtty_tx_state().left, 0, "queued");
            assert!(g.jtty_live(), "the session is untouched");
            assert!(!g.take_jtty_abort(), "cut");
        }
    }

    /// The over ends when what a live session left queued has drained: a later send is a new
    /// over, not stopped or refused under the old one's ceiling.
    #[test]
    fn a_drained_over_leaves_no_ceiling_behind() {
        let e = jtty_engine();
        live_start(&e, "", "").unwrap();
        sign_off(&e, "HI ", "").unwrap();
        engine_lock(&e).set_jtty_sending(true);
        drain(&e);
        // Half a second of the ceiling left as the drain ends.
        let max = crate::keyboard::JTTY.max_latch_ms;
        engine_lock(&e).jtty_tx.latch_start_ms = Some(now_unix_millis() - max + 500);
        engine_lock(&e).set_jtty_sending(false);
        assert_eq!(
            engine_lock(&e).jtty_tx_state().notice.as_deref(),
            Some(COMPLETE)
        );
        send_text(&e, "CQ CQ DE W9XYZ", "").unwrap();
        assert!(matches!(
            engine_lock(&e).poll_jtty(true, 0.0),
            JttyTick::Frame(_)
        ));
    }

    /// MODE-SWITCH TX-OFF: leaving the JTTY screen — for PSK, the same section, or anywhere
    /// else — stops JTTY, what is on the air included (WSJT-X's `noteTxModeChange`). A view
    /// re-asserting JTTY leaves it running.
    #[test]
    fn leaving_the_jtty_screen_stops_it() {
        for to in ["keyboard", "digital", "phone"] {
            let e = jtty_engine();
            send_text(&e, "CQ CQ DE W9XYZ", "").unwrap();
            frames(&e, 1);
            engine_lock(&e).set_operating_mode("jtty", false);
            assert_eq!(
                engine_lock(&e).tx_owner(),
                Some(TxOwner::Jtty),
                "re-assert: kept"
            );
            let mut g = engine_lock(&e);
            g.set_operating_mode(to, false);
            assert!(g.take_jtty_abort(), "{to}: cut");
            assert_eq!(
                g.jtty_tx_state().notice.as_deref(),
                Some(LEFT_SCREEN),
                "{to}"
            );
            assert_eq!(g.poll_jtty(true, 0.0), JttyTick::Idle, "{to}");
            assert!(!g.jtty_tx_active(), "{to}");
        }
    }

    /// PSK IS REFUSED, AND ITS QUEUE DROPPED, WHEN JTTY BECOMES THE MODEM. PSK and JTTY are one
    /// section, so the old leave rule (on the section alone) let a PSK over queued on the PSK
    /// screen key on, out of sight, from the JTTY screen.
    #[test]
    fn psk_is_refused_and_its_queue_dropped_when_jtty_becomes_the_modem() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_operating_mode("keyboard", false);
        e.set_frequency(14.070, "20m", "USB");
        e.psk_send_text("CQ CQ DE W9XYZ").unwrap();
        assert!(!e.psk_queue.is_empty(), "control: PSK queued");
        e.set_operating_mode("jtty", false);
        assert!(
            e.psk_queue.is_empty(),
            "the PSK queue went with the PSK screen"
        );
        assert_eq!(
            e.psk_state().keyer_error.as_deref(),
            Some(super::super::PSK_LEFT_SECTION)
        );
        let refused =
            Err("JTTY is the keyboard modem — open the PSK screen to send PSK".to_string());
        assert_eq!(e.psk_send_text("CQ"), refused);
        assert_eq!(e.set_psk_latched(true), refused);
        assert!(e.psk_queue.is_empty() && !e.psk_latched());
        // The hand-out's own check, should anything reach the queue here: held, never keyed.
        e.psk_queue.push_back("CQ".to_string());
        assert_eq!(
            e.poll_psk_one(),
            None,
            "PSK handed out an over from the JTTY screen"
        );
        e.psk_queue.clear();
        // Back on the PSK screen it sends again.
        e.set_operating_mode("keyboard", false);
        assert!(e.psk_send_text("CQ").is_ok());
    }

    /// Opening JTTY arms TX as opening PSK does — the Keyboard section's one rule — and sends
    /// nothing: every send is still the operator's own Send, F-key or Live. Behind a stop it arms
    /// nothing either ("Stop holds"): after Stop TX the latch stays off until the operator's own act.
    #[test]
    fn opening_jtty_arms_tx_as_psk_does_but_never_behind_a_stop() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        assert!(!e.tx_enabled(), "launch: disarmed");
        e.set_operating_mode("jtty", false);
        assert!(e.tx_enabled(), "JTTY entry arms, as PSK's does");
        assert!(!e.jtty_tx_active(), "and sends nothing");
        e.halt_tx();
        assert!(!e.tx_enabled(), "premise: Stop TX disarmed");
        e.set_operating_mode("keyboard", false);
        e.set_operating_mode("jtty", false);
        assert!(!e.tx_enabled(), "a stop holds across entry");
    }

    /// THE TX OFFSET: the licence gate judges JTTY's ACTUAL emission — its four tones from the Tx
    /// offset up, both edges — not PSK's netted centre, which JTTY does not transmit on.
    #[test]
    fn the_licence_gate_judges_jttys_actual_tx_offset() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_license_class("general");
        e.set_operating_mode("keyboard", false);
        // A General's 20 m data starts at 14.025 MHz. PSK's centre (1000 Hz) on a 14.0236 dial
        // is below it; JTTY's 1500 Hz and its 125 Hz above it are not.
        e.set_frequency(14.0236, "20m", "USB");
        assert!(!e.tx_allowed(), "control: PSK at 1000 Hz is outside");
        e.set_operating_mode("jtty", false);
        assert_eq!(e.jtty_tx_hz(), 1500.0);
        assert!(e.tx_allowed(), "JTTY at 1500 Hz is inside");
        e.set_jtty_tx(200.0).unwrap();
        assert!(!e.tx_allowed(), "JTTY at 200 Hz is outside");
        // The top edge too: 14.150 is where a General's data ends.
        e.set_jtty_tx(1500.0).unwrap();
        e.set_frequency(14.1485, "20m", "USB");
        e.set_jtty_tx(1350.0).unwrap();
        assert!(e.tx_allowed(), "14.14985 to 14.149975: inside");
        e.set_jtty_tx(1400.0).unwrap();
        assert!(!e.tx_allowed(), "its top tone crosses 14.150");
    }

    /// THE DATA-FRAME RULE: the licence gate judges JTTY's whole frame against ONE privileged data
    /// range ([`crate::privileges::tx_span_allowed`]), as FT8, JS8 and RTTY are judged. Swept across
    /// every edge of every class's data ranges, the frame below, straddling and above each, at
    /// three Tx offsets: the gate says what the data-frame rule says, every time.
    ///
    /// (Two legal edges of a 125 Hz frame could only disagree with that rule across a gap
    /// narrower than the frame; the narrowest gap in the tables is 60 m's 2.1 kHz, so this pins
    /// the rule the gate uses rather than a case where the old per-edge check differed.)
    #[test]
    fn the_licence_gate_judges_the_whole_frame_in_one_range() {
        use crate::privileges::{allowed_spans, tx_span_allowed};
        use crate::settings::{LicenseClass, OperatingMode};
        for (class, name) in [
            (LicenseClass::Technician, "technician"),
            (LicenseClass::General, "general"),
            (LicenseClass::Extra, "extra"),
        ] {
            let mut e = Engine::new("W9XYZ", "EN37", 0);
            e.set_license_class(name);
            e.set_operating_mode("jtty", false);
            let mut checked = 0;
            for (lo, hi) in allowed_spans(class, OperatingMode::Keyboard).unwrap() {
                for edge in [lo, hi] {
                    for tx in [200.0f32, 1500.0, 2900.0] {
                        let (lo_hz, hi_hz) = (tx as f64, (tx + JTTY_SPAN_HZ) as f64);
                        // Never an edge exactly on a range's end, where the float sum decides.
                        for shift_hz in [-200.0, -126.0, -124.0, -60.0, -1.0, 1.0, 60.0, 200.0] {
                            let dial = edge - lo_hz / 1e6 + shift_hz / 1e6;
                            let band = crate::bandplan::band_for_dial(dial).unwrap_or("");
                            e.set_frequency(dial, band, "USB");
                            e.set_jtty_tx(tx).unwrap();
                            let want = tx_span_allowed(
                                class,
                                dial + lo_hz / 1e6,
                                dial + hi_hz / 1e6,
                                OperatingMode::Keyboard,
                            );
                            assert_eq!(
                                e.tx_allowed(),
                                want,
                                "{name}: dial {dial:.6} Tx {tx} at the edge {edge}"
                            );
                            checked += 1;
                        }
                    }
                }
            }
            assert!(checked > 0, "{name}: nothing swept");
        }
    }

    /// The Tx offset goes back to 1500 Hz whenever JTTY is selected (WSJT-X's selection sets it,
    /// with Rx), holds its range, and cannot move while anything is queued or on the air.
    #[test]
    fn the_tx_offset_is_wsjtxs_and_holds_still_while_transmitting() {
        let e = jtty_engine();
        let mut g = engine_lock(&e);
        g.set_jtty_tx(1800.0).unwrap();
        assert!(g.set_jtty_tx(199.0).is_err() && g.set_jtty_tx(5001.0).is_err());
        assert_eq!(g.jtty_tx_hz(), 1800.0);
        g.set_operating_mode("keyboard", false);
        g.set_operating_mode("jtty", false);
        assert_eq!(g.jtty_tx_hz(), 1500.0, "selected again: 1500");
        g.set_tx_enabled(true);
        drop(g);
        send_text(&e, "CQ", "").unwrap();
        assert_eq!(
            engine_lock(&e).set_jtty_tx(1700.0),
            Err("Stop the JTTY transmission before moving Tx".to_string())
        );
    }

    /// A send planned before a stop (or a Tx change) is not committed when its render finishes:
    /// the operator stopped JTTY while it was being prepared.
    #[test]
    fn a_send_planned_before_a_stop_is_not_committed() {
        let e = jtty_engine();
        let plan = engine_lock(&e).jtty_plan_send("CQ W9XYZ", "").unwrap();
        let rendered = render(&plan).unwrap();
        engine_lock(&e).jtty_stop();
        assert_eq!(
            engine_lock(&e).jtty_commit(plan, rendered),
            Err(STOPPED_FOR_PREP.to_string())
        );
        let plan = engine_lock(&e).jtty_plan_send("CQ W9XYZ", "").unwrap();
        let rendered = render(&plan).unwrap();
        engine_lock(&e).set_jtty_tx(1600.0).unwrap();
        assert!(engine_lock(&e).jtty_commit(plan, rendered).is_err());
        assert_eq!(engine_lock(&e).tx_owner(), None);
    }

    /// The whole queue must fit the TX watchdog (WSJT-X has none in JTTY; ours stands) before
    /// anything is added: a send that would leave more queued than the limit is refused whole.
    #[test]
    fn the_queue_must_fit_the_watchdog() {
        let e = jtty_engine();
        engine_lock(&e).settings.tx_watchdog_min = 1;
        let long = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG ".repeat(4);
        let err = send_text(&e, &long, "").unwrap_err();
        assert!(err.contains("longer than it may run (1 min"), "{err}");
        assert_eq!(engine_lock(&e).tx_owner(), None, "nothing was added");
        assert!(
            send_text(&e, "CQ W9XYZ", "").is_ok(),
            "control: a short one fits"
        );
    }

    /// The gates a send passes, each refusing with its reason.
    #[test]
    fn the_gates_refuse_with_their_reasons() {
        let e = jtty_engine();
        engine_lock(&e).set_tx_enabled(false);
        assert!(send_text(&e, "CQ", "")
            .unwrap_err()
            .starts_with("TX is off"));
        engine_lock(&e).set_tx_enabled(true);
        engine_lock(&e).settings.mycall = String::new();
        assert!(send_text(&e, "CQ", "").unwrap_err().contains("callsign"));
        engine_lock(&e).settings.mycall = "W9XYZ".into();
        {
            let mut g = engine_lock(&e);
            g.set_license_class("general");
            g.set_frequency(14.0200, "20m", "USB");
        }
        assert!(send_text(&e, "CQ", "")
            .unwrap_err()
            .starts_with("TX locked"));
        engine_lock(&e).set_frequency(14.090, "20m", "USB");
        engine_lock(&e).set_operating_mode("keyboard", false);
        assert!(send_text(&e, "CQ", "")
            .unwrap_err()
            .starts_with("Not in the JTTY section"));
    }
}
