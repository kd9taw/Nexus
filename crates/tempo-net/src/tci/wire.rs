//! The TCI text codec: a WebSocket text message split into commands, each read into a typed value,
//! and every typed value rendered back to wire text.
//!
//! # The grammar (§3.1)
//!
//! `name:arg1,arg2,…;`, or `name;` with no arguments; `:`, `,` and `;` are reserved. A message may
//! carry several commands (`vfo:0,0,14074000;modulation:0,usb;`), and whitespace between commands
//! is skipped. The case of letters does not matter: names and protocol words ([`Word`]) are read
//! in any case and rendered in lower case; callsigns ([`Call`]) read in any case and are kept in
//! upper case; names of things ([`Token`]: a device, a program, a version, a spot's mode) keep the
//! case they came in. An invalid command is ignored by its receiver (§3.1); here it is a typed
//! [`WireError`] that the session counts and drops, and the commands around it still parse.
//!
//! Free text (a CW macro, the parts of a CW message, a spot's comment) carries the reserved
//! characters escaped: `:` as `^`, `,` as `~`, `;` as `*` (§3.2.1). [`Text`] holds the plain text
//! and does the escaping; text that holds `^`, `~` or `*` itself cannot be carried, and is refused
//! rather than sent changed. The spec states the escaping for CW macros; this codec escapes every
//! free-text argument the same way, since none may carry a raw reserved character.
//!
//! A number is a decimal integer, or for a level, a power or an SWR a decimal fraction ([`Real`]);
//! spaces around an argument are allowed (the spec's own `RX_SENSORS:0, -71.5;`), a sign other
//! than `-`, an exponent or a separator inside a number are not.
//!
//! # The four kinds
//!
//! - [`Notice`]: what a server sends: initialization (§4.1), the state of every parameter, stream
//!   acknowledgements, notifications and meters (§4.4–§4.5). [`Notice::parse`],
//!   [`Notice::render`], and [`notices`] for a whole message.
//! - [`Command`]: what a client sends that cannot key the transmitter: parameter writes
//!   ([`Setting`]) and reads ([`Query`]), stream control ([`StreamSetting`]), spots, meter
//!   subscriptions. [`Command::render`].
//! - [`Stop`]: what ends a transmission (`trx:<trx>,false;`, `tune:<trx>,false;`,
//!   `cw_macros_stop;`, `cw_terminal:false;`, a key-up). Never gated: [`Stop::render`].
//! - [`Start`]: what can start or extend a transmission (`trx:<trx>,true[,<source>];`,
//!   `tune:<trx>,true;`, `cw_macros`, `cw_msg`, `cw_terminal:true;`, a key-down). Its renderer
//!   is private to the `tci` module, where the admission a key must pass is meant to be its only
//!   caller; nothing outside the module can produce the text of a key.
//!
//! [`Command::render`] and [`Stop::render`] return [`ClientText`], and [`Notice::render`]
//! [`ServerText`]. Neither has a public constructor, so a client's outbox that takes only
//! `ClientText` cannot carry a notice (`trx:0,true;` is both a server's report and a key) or any
//! raw string. A server reads a client's commands as [`Request`]s ([`requests`]). Nexus's own
//! server reads them with [`inbound`], which adds the forms real clients send that the spec gives
//! no client ([`Extra`]: WSJT-X's `split_enable:false;`, MSHV's `rx_enable` and `tx_enable`, the
//! recorder commands it refuses), and answers with [`Notice`]s and [`ExtraNotice`]s.
//!
//! # Bounds
//!
//! Nothing a server sends can make this module panic or allocate without bound. A message over
//! [`MAX_MESSAGE`] bytes is refused whole; at most [`MAX_COMMANDS`] commands are read from one; a
//! command is at most [`MAX_COMMAND`] bytes with a name of at most [`MAX_NAME`], at most
//! [`MAX_ARGS`] arguments of at most [`MAX_ARG`] bytes each. Receiver and channel numbers,
//! frequencies and real values are bounded by their types. Every error names the command it is
//! about when the name was read ([`WireError::command`]): a session that cannot read back a
//! value it relies on (`trx`, `drive`, `tx_enable`, `tx_sensors`) treats that value as unknown
//! rather than keeping the last one.
//!
//! # Dialects
//!
//! Every spelling a known server uses is read, whatever the dialect: `drive` with or without a
//! receiver, `channel_count` and `channels_count`, `tx_footswitch` and the spec's
//! `tx_footcwitch`, a `modulation` with a channel, the 1.x meters (`rx_sensors`, `rx_smeter`,
//! `tx_power`, `tx_swr`). Rendering follows the [`Dialect`]: [`Setting::drive`] and
//! [`Query::drive`] build the drive form it reads, and a key that names a source is not rendered
//! for a dialect without one. A server reading a client's `drive:<n>;` needs its own dialect to
//! know whether that is a write (1.x) or a read of receiver `n` (2.0), so [`requests`] takes one.
//!
//! Not typed, so not sent or read: the `LINE_OUT_RECORDER_*` commands, which make the SDR
//! program's computer write a file at a path the client names (§4.3).

use std::fmt::{self, Write as _};

use super::frame::{AudioRate, Channels, IqRate, SampleFormat};
use super::{Channel, Dialect, DriveForm, Trx};

/// The longest text message read, in bytes: AetherSDR's TCI server refuses messages over the same
/// 64 KiB. A server's whole initialization burst is a few hundred bytes.
pub const MAX_MESSAGE: usize = 64 * 1024;

/// The most commands read from one message; the rest of the message is refused.
pub const MAX_COMMANDS: usize = 1024;

/// The longest command, in bytes, from its name to its `;`.
pub const MAX_COMMAND: usize = 4096;

/// The longest command name. The longest in TCI 2.0 is `tx_stream_audio_buffering` (25).
pub const MAX_NAME: usize = 32;

/// The most arguments one command carries. The longest list is `modulations_list`, about a dozen.
pub const MAX_ARGS: usize = 64;

/// The longest argument, in bytes, and the longest [`Text`].
pub const MAX_ARG: usize = 1024;

/// The longest [`Token`] or [`Call`].
pub const MAX_TOKEN: usize = 64;

/// The longest [`Word`].
pub const MAX_WORD: usize = 16;

/// The largest magnitude of a frequency or an offset, in Hz: 1 THz.
pub const MAX_HZ: i64 = 1_000_000_000_000;

/// The largest magnitude of a [`Real`], in its own units (dBm, W, SWR).
pub const MAX_REAL: i64 = 1_000_000_000;

const RESERVED: [u8; 3] = *b":,;";
const ESCAPES: [u8; 3] = *b"^~*";

fn printable(b: u8) -> bool {
    (0x20..=0x7e).contains(&b)
}

// ── Values ──────────────────────────────────────────────────────────────────────────────────

/// A frequency or an offset in Hz, within ±[`MAX_HZ`]. Negative values are ordinary: an IF
/// offset, an XIT, a filter edge below the carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hz(i64);

impl Hz {
    pub fn new(hz: i64) -> Option<Hz> {
        (-MAX_HZ..=MAX_HZ).contains(&hz).then_some(Hz(hz))
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for Hz {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A decimal value (a level in dBm, a power in W, an SWR), held exactly in millionths, within
/// ±[`MAX_REAL`]. Read from up to nine decimals, rounded to six; rendered with at least one
/// decimal (`47.0`, never `47`: WSJT-X splits power and SWR at the `.`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Real(i64);

impl Real {
    const SCALE: i64 = 1_000_000;

    pub fn from_micros(micros: i64) -> Option<Real> {
        let max = MAX_REAL * Real::SCALE;
        (-max..=max).contains(&micros).then_some(Real(micros))
    }

    /// The nearest value in millionths; `None` if not finite or out of range.
    pub fn from_f64(value: f64) -> Option<Real> {
        let micros = (value * Real::SCALE as f64).round();
        let max = (MAX_REAL * Real::SCALE) as f64;
        (micros.is_finite() && micros.abs() <= max).then_some(Real(micros as i64))
    }

    pub fn micros(self) -> i64 {
        self.0
    }

    pub fn to_f64(self) -> f64 {
        self.0 as f64 / Real::SCALE as f64
    }

    fn parse(arg: &str) -> Option<Real> {
        let s = arg.trim_matches(' ');
        let (negative, body) = match s.strip_prefix('-') {
            Some(body) => (true, body),
            None => (false, s),
        };
        let (whole, fraction) = match body.split_once('.') {
            Some((w, f)) => (w, Some(f)),
            None => (body, None),
        };
        if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let whole: i64 = whole.parse().ok()?;
        let micros = match fraction {
            None => 0,
            Some(f) => {
                if f.is_empty() || f.len() > 9 || !f.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let digits: i64 = f.parse().ok()?;
                let scale = 10i64.pow(f.len() as u32);
                // Half a millionth and up rounds away from zero (the sign is applied after).
                (digits * Real::SCALE + scale / 2) / scale
            }
        };
        let magnitude = whole.checked_mul(Real::SCALE)?.checked_add(micros)?;
        Real::from_micros(if negative { -magnitude } else { magnitude })
    }
}

impl fmt::Display for Real {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let magnitude = self.0.unsigned_abs();
        let scale = Real::SCALE.unsigned_abs();
        let mut fraction = format!("{:06}", magnitude % scale);
        while fraction.len() > 1 && fraction.ends_with('0') {
            fraction.pop();
        }
        write!(f, "{sign}{}.{fraction}", magnitude / scale)
    }
}

/// A protocol word: a modulation (`usb`, `digu`, `nfm`, …) or a signal source. Letters, digits,
/// `_`, `-` and `+`, at most [`MAX_WORD`]; held in lower case.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Word(String);

impl Word {
    pub fn new(text: &str) -> Option<Word> {
        let ok = !text.is_empty()
            && text.len() <= MAX_WORD
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+'));
        ok.then(|| Word(text.to_ascii_lowercase()))
    }

    fn parse(arg: &str) -> Option<Word> {
        Word::new(arg.trim_matches(' '))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The name of a thing (a device, a program, a version, a spot's mode), kept as given: printable
/// ASCII without the reserved or escape characters, no space at either end, 1 to [`MAX_TOKEN`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token(String);

impl Token {
    pub fn new(text: &str) -> Option<Token> {
        let ok = !text.is_empty()
            && text.len() <= MAX_TOKEN
            && !text.starts_with(' ')
            && !text.ends_with(' ')
            && text
                .bytes()
                .all(|b| printable(b) && !RESERVED.contains(&b) && !ESCAPES.contains(&b));
        ok.then(|| Token(text.to_string()))
    }

    fn parse(arg: &str) -> Option<Token> {
        Token::new(arg.trim_matches(' '))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A callsign, as spots and clicks carry it: a [`Token`] held in upper case, so a click matches
/// the spot it is on whatever case the server echoes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Call(String);

impl Call {
    pub fn new(text: &str) -> Option<Call> {
        Token::new(text).map(|t| Call(t.0.to_ascii_uppercase()))
    }

    fn parse(arg: &str) -> Option<Call> {
        Call::new(arg.trim_matches(' '))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Call {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Free text: printable ASCII without `^`, `~` or `*`, at most [`MAX_ARG`] characters, kept
/// exactly (the spaces of a CW macro are part of it). `:`, `,` and `;` are carried escaped.
///
/// A CW macro gives `<`, `>` (speed) and `|…|` (prosigns) meaning, and a CW message's callsign
/// `$<n>` (repeat): this type does not police them. Whoever builds CW text filters it to what it
/// can time.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Text(String);

impl Text {
    pub fn new(plain: &str) -> Option<Text> {
        let ok =
            plain.len() <= MAX_ARG && plain.bytes().all(|b| printable(b) && !ESCAPES.contains(&b));
        ok.then(|| Text(plain.to_string()))
    }

    /// The plain text of a wire argument. A raw reserved character (a `:` past the name) means the
    /// sender did not escape, so the text is refused rather than guessed.
    fn from_wire(arg: &str) -> Option<Text> {
        if arg.len() > MAX_ARG || !arg.bytes().all(|b| printable(b) && !RESERVED.contains(&b)) {
            return None;
        }
        let plain = arg
            .chars()
            .map(|c| match c {
                '^' => ':',
                '~' => ',',
                '*' => ';',
                c => c,
            })
            .collect();
        Some(Text(plain))
    }

    fn escape_into(&self, out: &mut String) {
        out.extend(self.0.chars().map(|c| match c {
            ':' => '^',
            ',' => '~',
            ';' => '*',
            c => c,
        }));
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A spot's colour (`SPOT` `arg4`, §4.3), ARGB. The specification writes it as a decimal number
/// (`SPOT:RN6LHF,CW,7100000,16711680,ANY_TEXT;`). AetherSDR's TCI server keeps the argument as text
/// and draws a spot in it only when it starts with `#` (`TciProtocol::cmdSpot` and
/// `SpectrumWidget`, at `8394342`), so it is sent `#AARRGGBB` there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotColour {
    /// A decimal number: `4294065856`.
    Decimal(u32),
    /// `#AARRGGBB`: `#FFF23EC0`.
    Hash(u32),
}

impl SpotColour {
    /// The colour, ARGB, in whichever form it came.
    pub fn argb(self) -> u32 {
        match self {
            SpotColour::Decimal(argb) | SpotColour::Hash(argb) => argb,
        }
    }

    fn parse(arg: &str) -> Option<SpotColour> {
        let s = arg.trim_matches(' ');
        match s.strip_prefix('#') {
            Some(hex) if hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
                u32::from_str_radix(hex, 16).ok().map(SpotColour::Hash)
            }
            Some(_) => None,
            None => uint(s).map(SpotColour::Decimal),
        }
    }
}

impl fmt::Display for SpotColour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpotColour::Decimal(argb) => write!(f, "{argb}"),
            SpotColour::Hash(argb) => write!(f, "#{argb:08X}"),
        }
    }
}

/// Where a transmission takes its audio (`trx` `arg3`, §4.2). Absent, the SDR program's selected
/// microphone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    /// The client's TCI audio stream. Taken only while that stream is enabled; otherwise the
    /// microphone is (§4.2, TRX note).
    Tci,
    Mic1,
    Mic2,
    MicPc,
    /// The E-Coder2 panel.
    Ecoder2,
    /// A source this codec does not know, as a server reported it; never one of the five above.
    Other(Word),
}

impl Source {
    pub fn from_word(word: Word) -> Source {
        match word.as_str() {
            "tci" => Source::Tci,
            "mic1" => Source::Mic1,
            "mic2" => Source::Mic2,
            "micpc" => Source::MicPc,
            "ecoder2" => Source::Ecoder2,
            _ => Source::Other(word),
        }
    }

    pub fn word(&self) -> &str {
        match self {
            Source::Tci => "tci",
            Source::Mic1 => "mic1",
            Source::Mic2 => "mic2",
            Source::MicPc => "micpc",
            Source::Ecoder2 => "ecoder2",
            Source::Other(w) => w.as_str(),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// A receiver's AGC mode (§4.2 `AGC_MODE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgcMode {
    Normal,
    Fast,
    Off,
}

impl AgcMode {
    pub const ALL: [AgcMode; 3] = [AgcMode::Normal, AgcMode::Fast, AgcMode::Off];

    pub fn word(self) -> &'static str {
        match self {
            AgcMode::Normal => "normal",
            AgcMode::Fast => "fast",
            AgcMode::Off => "off",
        }
    }

    fn parse(arg: &str) -> Option<AgcMode> {
        let arg = arg.trim_matches(' ');
        AgcMode::ALL
            .into_iter()
            .find(|m| m.word().eq_ignore_ascii_case(arg))
    }
}

impl fmt::Display for AgcMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// The per-receiver on/off parameters (§4.2): each is `<name>:<trx>,<bool>;`, read with
/// `<name>:<trx>;`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flag {
    Rit,
    Xit,
    Split,
    RxMute,
    NoiseBlanker,
    Binaural,
    NoiseReduction,
    NoiseCanceller,
    AutoNotch,
    PeakFilter,
    Surround,
    NotchFilters,
    /// The tuning lock (`lock`).
    Lock,
    Squelch,
}

impl Flag {
    pub const ALL: [Flag; 14] = [
        Flag::Rit,
        Flag::Xit,
        Flag::Split,
        Flag::RxMute,
        Flag::NoiseBlanker,
        Flag::Binaural,
        Flag::NoiseReduction,
        Flag::NoiseCanceller,
        Flag::AutoNotch,
        Flag::PeakFilter,
        Flag::Surround,
        Flag::NotchFilters,
        Flag::Lock,
        Flag::Squelch,
    ];

    /// The command name.
    pub fn name(self) -> &'static str {
        match self {
            Flag::Rit => "rit_enable",
            Flag::Xit => "xit_enable",
            Flag::Split => "split_enable",
            Flag::RxMute => "rx_mute",
            Flag::NoiseBlanker => "rx_nb_enable",
            Flag::Binaural => "rx_bin_enable",
            Flag::NoiseReduction => "rx_nr_enable",
            Flag::NoiseCanceller => "rx_anc_enable",
            Flag::AutoNotch => "rx_anf_enable",
            Flag::PeakFilter => "rx_apf_enable",
            Flag::Surround => "rx_dse_enable",
            Flag::NotchFilters => "rx_nf_enable",
            Flag::Lock => "lock",
            Flag::Squelch => "sql_enable",
        }
    }

    fn from_name(name: &str) -> Option<Flag> {
        Flag::ALL.into_iter().find(|f| f.name() == name)
    }
}

/// A list of modulation names (`modulations_list`, §4.1): 1 to [`MAX_ARGS`] of them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Modes(Vec<Word>);

impl Modes {
    pub fn new(modes: Vec<Word>) -> Option<Modes> {
        (!modes.is_empty() && modes.len() <= MAX_ARGS).then_some(Modes(modes))
    }

    pub fn as_slice(&self) -> &[Word] {
        &self.0
    }
}

// ── Commands ────────────────────────────────────────────────────────────────────────────────

/// A parameter's value, as a client writes it and a server reports it (§4.2). Keying state (`trx`,
/// `tune`) is not here: a client starts those only as a [`Start`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Setting {
    /// The receiver's centre (panorama) frequency.
    Dds {
        trx: Trx,
        hz: Hz,
    },
    /// The IF filter's offset within the panorama.
    If {
        trx: Trx,
        channel: Channel,
        hz: Hz,
    },
    /// A channel's (VFO's) frequency: channel 0 is A, 1 is B.
    Vfo {
        trx: Trx,
        channel: Channel,
        hz: Hz,
    },
    /// The mode, by a name `modulations_list` gave. `channel` is read when a server sends one
    /// (`modulation:<trx>,<ch>,<mode>;`, which WSJT-X reads from ExpertSDR3 and Thetis,
    /// `TCITransceiver.cpp:707-711`); the spec's form has none.
    Modulation {
        trx: Trx,
        channel: Option<Channel>,
        mode: Word,
    },
    /// Output power, 0 to 100 in the spec. `trx` is absent in TCI 1.x ([`DriveForm::Global`]).
    /// The level is read whatever its value: a readback above the ceiling must be seen, not dropped.
    Drive {
        trx: Option<Trx>,
        level: u32,
    },
    /// Output power in Tune.
    TuneDrive {
        trx: Trx,
        level: u32,
    },
    Flag {
        flag: Flag,
        trx: Trx,
        on: bool,
    },
    RitOffset {
        trx: Trx,
        hz: Hz,
    },
    XitOffset {
        trx: Trx,
        hz: Hz,
    },
    /// Channel B on or off (A is always on).
    RxChannelEnable {
        trx: Trx,
        channel: Channel,
        on: bool,
    },
    /// The receive filter's edges, relative to the carrier.
    RxFilterBand {
        trx: Trx,
        low: Hz,
        high: Hz,
    },
    /// CW macro speed, WPM.
    CwMacrosSpeed(u32),
    /// CW macro delay before transmitting, ms.
    CwMacrosDelay(u32),
    /// CW keyer speed, WPM.
    CwKeyerSpeed(u32),
    /// Main volume, dB (-60 to 0).
    Volume(i32),
    Mute(bool),
    RxVolume {
        trx: Trx,
        channel: Channel,
        db: i32,
    },
    RxBalance {
        trx: Trx,
        channel: Channel,
        db: i32,
    },
    /// Monitor volume in transmit, dB.
    MonVolume(i32),
    MonEnable(bool),
    AgcMode {
        trx: Trx,
        mode: AgcMode,
    },
    AgcGain {
        trx: Trx,
        db: i32,
    },
    /// Noise blanker threshold (1–100) and pulse duration (1–300).
    NbParam {
        trx: Trx,
        threshold: u32,
        duration: u32,
    },
    SqlLevel {
        trx: Trx,
        db: i32,
    },
    /// The audio offset of DIGL, Hz (0–4000).
    DiglOffset(u32),
    /// The audio offset of DIGU, Hz (0–4000).
    DiguOffset(u32),
}

impl Setting {
    /// The drive write `dialect` reads.
    pub fn drive(trx: Trx, level: u32, dialect: &Dialect) -> Setting {
        Setting::Drive {
            trx: match dialect.drive {
                DriveForm::PerTrx => Some(trx),
                DriveForm::Global => None,
            },
            level,
        }
    }

    fn line(&self) -> String {
        match self {
            Setting::Dds { trx, hz } => Line::new("dds").arg(trx).arg(hz).end(),
            Setting::If { trx, channel, hz } => Line::new("if").arg(trx).arg(channel).arg(hz).end(),
            Setting::Vfo { trx, channel, hz } => {
                Line::new("vfo").arg(trx).arg(channel).arg(hz).end()
            }
            Setting::Modulation { trx, channel, mode } => Line::new("modulation")
                .arg(trx)
                .opt(channel.as_ref())
                .arg(mode)
                .end(),
            Setting::Drive { trx, level } => Line::new("drive").opt(trx.as_ref()).arg(level).end(),
            Setting::TuneDrive { trx, level } => Line::new("tune_drive").arg(trx).arg(level).end(),
            Setting::Flag { flag, trx, on } => Line::new(flag.name()).arg(trx).arg(on).end(),
            Setting::RitOffset { trx, hz } => Line::new("rit_offset").arg(trx).arg(hz).end(),
            Setting::XitOffset { trx, hz } => Line::new("xit_offset").arg(trx).arg(hz).end(),
            Setting::RxChannelEnable { trx, channel, on } => Line::new("rx_channel_enable")
                .arg(trx)
                .arg(channel)
                .arg(on)
                .end(),
            Setting::RxFilterBand { trx, low, high } => Line::new("rx_filter_band")
                .arg(trx)
                .arg(low)
                .arg(high)
                .end(),
            Setting::CwMacrosSpeed(wpm) => Line::new("cw_macros_speed").arg(wpm).end(),
            Setting::CwMacrosDelay(ms) => Line::new("cw_macros_delay").arg(ms).end(),
            Setting::CwKeyerSpeed(wpm) => Line::new("cw_keyer_speed").arg(wpm).end(),
            Setting::Volume(db) => Line::new("volume").arg(db).end(),
            Setting::Mute(on) => Line::new("mute").arg(on).end(),
            Setting::RxVolume { trx, channel, db } => {
                Line::new("rx_volume").arg(trx).arg(channel).arg(db).end()
            }
            Setting::RxBalance { trx, channel, db } => {
                Line::new("rx_balance").arg(trx).arg(channel).arg(db).end()
            }
            Setting::MonVolume(db) => Line::new("mon_volume").arg(db).end(),
            Setting::MonEnable(on) => Line::new("mon_enable").arg(on).end(),
            Setting::AgcMode { trx, mode } => Line::new("agc_mode").arg(trx).arg(mode).end(),
            Setting::AgcGain { trx, db } => Line::new("agc_gain").arg(trx).arg(db).end(),
            Setting::NbParam {
                trx,
                threshold,
                duration,
            } => Line::new("rx_nb_param")
                .arg(trx)
                .arg(threshold)
                .arg(duration)
                .end(),
            Setting::SqlLevel { trx, db } => Line::new("sql_level").arg(trx).arg(db).end(),
            Setting::DiglOffset(hz) => Line::new("digl_offset").arg(hz).end(),
            Setting::DiguOffset(hz) => Line::new("digu_offset").arg(hz).end(),
        }
    }
}

/// A read of a parameter (§4.2 "Read"): the server answers with the parameter's [`Setting`] (or,
/// for `trx`, `tune` and `vfo_lock`, the matching [`Notice`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Query {
    Dds(Trx),
    If {
        trx: Trx,
        channel: Channel,
    },
    Vfo {
        trx: Trx,
        channel: Channel,
    },
    Modulation(Trx),
    /// Whether the receiver is transmitting.
    Trx(Trx),
    Tune(Trx),
    /// `drive:<trx>;`, or `drive;` in TCI 1.x.
    Drive(Option<Trx>),
    TuneDrive(Trx),
    Flag {
        flag: Flag,
        trx: Trx,
    },
    RitOffset(Trx),
    XitOffset(Trx),
    RxChannelEnable {
        trx: Trx,
        channel: Channel,
    },
    RxFilterBand(Trx),
    CwMacrosSpeed,
    CwMacrosDelay,
    Volume,
    Mute,
    RxVolume {
        trx: Trx,
        channel: Channel,
    },
    RxBalance {
        trx: Trx,
        channel: Channel,
    },
    MonVolume,
    MonEnable,
    AgcMode(Trx),
    AgcGain(Trx),
    NbParam(Trx),
    SqlLevel(Trx),
    DiglOffset,
    DiguOffset,
    VfoLock {
        trx: Trx,
        channel: Channel,
    },
    /// The polled S-meter of servers before 2.0 (not in the spec; WSJT-X asks it).
    RxSmeter {
        trx: Trx,
        channel: Channel,
    },
}

impl Query {
    /// The drive read `dialect` understands.
    pub fn drive(trx: Trx, dialect: &Dialect) -> Query {
        Query::Drive(match dialect.drive {
            DriveForm::PerTrx => Some(trx),
            DriveForm::Global => None,
        })
    }

    fn line(&self) -> String {
        match self {
            Query::Dds(trx) => Line::new("dds").arg(trx).end(),
            Query::If { trx, channel } => Line::new("if").arg(trx).arg(channel).end(),
            Query::Vfo { trx, channel } => Line::new("vfo").arg(trx).arg(channel).end(),
            Query::Modulation(trx) => Line::new("modulation").arg(trx).end(),
            Query::Trx(trx) => Line::new("trx").arg(trx).end(),
            Query::Tune(trx) => Line::new("tune").arg(trx).end(),
            Query::Drive(trx) => Line::new("drive").opt(trx.as_ref()).end(),
            Query::TuneDrive(trx) => Line::new("tune_drive").arg(trx).end(),
            Query::Flag { flag, trx } => Line::new(flag.name()).arg(trx).end(),
            Query::RitOffset(trx) => Line::new("rit_offset").arg(trx).end(),
            Query::XitOffset(trx) => Line::new("xit_offset").arg(trx).end(),
            Query::RxChannelEnable { trx, channel } => {
                Line::new("rx_channel_enable").arg(trx).arg(channel).end()
            }
            Query::RxFilterBand(trx) => Line::new("rx_filter_band").arg(trx).end(),
            Query::CwMacrosSpeed => Line::new("cw_macros_speed").end(),
            Query::CwMacrosDelay => Line::new("cw_macros_delay").end(),
            Query::Volume => Line::new("volume").end(),
            Query::Mute => Line::new("mute").end(),
            Query::RxVolume { trx, channel } => Line::new("rx_volume").arg(trx).arg(channel).end(),
            Query::RxBalance { trx, channel } => {
                Line::new("rx_balance").arg(trx).arg(channel).end()
            }
            Query::MonVolume => Line::new("mon_volume").end(),
            Query::MonEnable => Line::new("mon_enable").end(),
            Query::AgcMode(trx) => Line::new("agc_mode").arg(trx).end(),
            Query::AgcGain(trx) => Line::new("agc_gain").arg(trx).end(),
            Query::NbParam(trx) => Line::new("rx_nb_param").arg(trx).end(),
            Query::SqlLevel(trx) => Line::new("sql_level").arg(trx).end(),
            Query::DiglOffset => Line::new("digl_offset").end(),
            Query::DiguOffset => Line::new("digu_offset").end(),
            Query::VfoLock { trx, channel } => Line::new("vfo_lock").arg(trx).arg(channel).end(),
            Query::RxSmeter { trx, channel } => Line::new("rx_smeter").arg(trx).arg(channel).end(),
        }
    }
}

/// The streams a client turns on and shapes (§4.3); a server acknowledges each with the same
/// command (WSJT-X waits for `audio_start:<trx>;`, `TCITransceiver.cpp:761-769`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamSetting {
    AudioStart(Trx),
    AudioStop(Trx),
    IqStart(Trx),
    IqStop(Trx),
    LineOutStart(Trx),
    LineOutStop(Trx),
    AudioSampleRate(AudioRate),
    IqSampleRate(IqRate),
    /// `audio_stream_sample_type`; float32 by default.
    AudioSampleType(SampleFormat),
    /// `audio_stream_channels`; two by default.
    AudioChannels(Channels),
    /// `audio_stream_samples`: the `length` of each audio frame (100–2048 in the spec).
    AudioSamples(u32),
    /// `tx_stream_audio_buffering`, ms (50–500 in the spec, 50 by default).
    TxAudioBuffering(u32),
}

impl StreamSetting {
    fn line(&self) -> String {
        match self {
            StreamSetting::AudioStart(trx) => Line::new("audio_start").arg(trx).end(),
            StreamSetting::AudioStop(trx) => Line::new("audio_stop").arg(trx).end(),
            StreamSetting::IqStart(trx) => Line::new("iq_start").arg(trx).end(),
            StreamSetting::IqStop(trx) => Line::new("iq_stop").arg(trx).end(),
            StreamSetting::LineOutStart(trx) => Line::new("line_out_start").arg(trx).end(),
            StreamSetting::LineOutStop(trx) => Line::new("line_out_stop").arg(trx).end(),
            StreamSetting::AudioSampleRate(r) => Line::new("audio_samplerate").arg(r).end(),
            StreamSetting::IqSampleRate(r) => Line::new("iq_samplerate").arg(r).end(),
            StreamSetting::AudioSampleType(f) => {
                Line::new("audio_stream_sample_type").arg(f.word()).end()
            }
            StreamSetting::AudioChannels(c) => Line::new("audio_stream_channels").arg(c).end(),
            StreamSetting::AudioSamples(n) => Line::new("audio_stream_samples").arg(n).end(),
            StreamSetting::TxAudioBuffering(ms) => {
                Line::new("tx_stream_audio_buffering").arg(ms).end()
            }
        }
    }
}

/// What a server sends.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Notice {
    /// The device's frequency range (§4.1).
    VfoLimits {
        low: Hz,
        high: Hz,
    },
    /// The IF filter's range (§4.1).
    IfLimits {
        low: Hz,
        high: Hz,
    },
    TrxCount(u32),
    /// `channel_count` in the spec, `channels_count` from every server read; rendered plural,
    /// the only spelling a common client accepts (AetherSDR's notes).
    ChannelCount(u32),
    Device(Token),
    ReceiveOnly(bool),
    ModulationsList(Modes),
    /// The program and the TCI version it speaks. Not an identity: piHPSDR and AetherSDR announce
    /// `ExpertSDR3`.
    Protocol {
        program: Token,
        version: Token,
    },
    /// The end of the initialization burst and the state after it (§4.1).
    Ready,
    /// The SDR program started (§4.2 `START`).
    RadioStart,
    /// The SDR program stopped (§4.2 `STOP`).
    RadioStop,
    State(Setting),
    /// Transmitting or not. `source` when the server names one (the spec's reply has none).
    Trx {
        trx: Trx,
        on: bool,
        source: Option<Source>,
    },
    Tune {
        trx: Trx,
        on: bool,
    },
    /// Whether the transmitter may transmit, at connect and on a band change (§4.3).
    TxEnable {
        trx: Trx,
        on: bool,
    },
    /// The frequency the transmitter is on (§4.4).
    TxFrequency(Hz),
    /// Whether a channel's tuning is locked (§4.5).
    VfoLock {
        trx: Trx,
        channel: Channel,
        on: bool,
    },
    Stream(StreamSetting),
    /// The legacy click on a spot (§4.4).
    ClickedOnSpot {
        call: Call,
        hz: Hz,
    },
    RxClickedOnSpot {
        trx: Trx,
        channel: Channel,
        call: Call,
        hz: Hz,
    },
    /// The PTT footswitch (`TX_FOOTSWITCH` in the heading, `TX_FOOTCWITCH` in its own lines; both
    /// read, the heading's rendered).
    TxFootswitch {
        trx: Trx,
        pressed: bool,
    },
    AppFocus(bool),
    /// The receive level, dBm (§4.4; deprecated by 2.0).
    RxSensors {
        trx: Trx,
        dbm: Real,
    },
    /// The receive level per channel, dBm (§4.5).
    RxChannelSensors {
        trx: Trx,
        channel: Channel,
        dbm: Real,
    },
    /// The transmitter's readings: microphone level dBm, RMS power W, peak power W, SWR (§4.4).
    TxSensors {
        trx: Trx,
        mic_dbm: Real,
        rms_w: Real,
        peak_w: Real,
        swr: Real,
    },
    /// The polled S-meter's answer (not in the spec).
    RxSmeter {
        trx: Trx,
        channel: Channel,
        dbm: Real,
    },
    /// Forward power, W (not in the spec; WSJT-X maps it).
    TxPower(Real),
    /// SWR (not in the spec; WSJT-X maps it).
    TxSwr(Real),
    /// In CW terminal mode, the last character in the queue has started (§3.2.1).
    CwMacrosEmpty,
    /// The callsign a CW message finally sent (§3.2.2).
    CallsignSend(Call),
}

impl Notice {
    /// One command, read as a server's.
    pub fn parse(raw: &Raw<'_>) -> Result<Notice, WireError> {
        notice(raw.name(), &Args(raw.args())).map_err(|fault| WireError::Command {
            name: raw.name.clone(),
            fault,
        })
    }

    /// The wire text a server sends.
    pub fn render(&self) -> ServerText {
        ServerText(self.line())
    }

    fn line(&self) -> String {
        match self {
            Notice::VfoLimits { low, high } => Line::new("vfo_limits").arg(low).arg(high).end(),
            Notice::IfLimits { low, high } => Line::new("if_limits").arg(low).arg(high).end(),
            Notice::TrxCount(n) => Line::new("trx_count").arg(n).end(),
            Notice::ChannelCount(n) => Line::new("channels_count").arg(n).end(),
            Notice::Device(name) => Line::new("device").arg(name).end(),
            Notice::ReceiveOnly(on) => Line::new("receive_only").arg(on).end(),
            Notice::ModulationsList(modes) => modes
                .as_slice()
                .iter()
                .fold(Line::new("modulations_list"), Line::arg)
                .end(),
            Notice::Protocol { program, version } => {
                Line::new("protocol").arg(program).arg(version).end()
            }
            Notice::Ready => Line::new("ready").end(),
            Notice::RadioStart => Line::new("start").end(),
            Notice::RadioStop => Line::new("stop").end(),
            Notice::State(setting) => setting.line(),
            Notice::Trx { trx, on, source } => {
                Line::new("trx").arg(trx).arg(on).opt(source.as_ref()).end()
            }
            Notice::Tune { trx, on } => Line::new("tune").arg(trx).arg(on).end(),
            Notice::TxEnable { trx, on } => Line::new("tx_enable").arg(trx).arg(on).end(),
            Notice::TxFrequency(hz) => Line::new("tx_frequency").arg(hz).end(),
            Notice::VfoLock { trx, channel, on } => {
                Line::new("vfo_lock").arg(trx).arg(channel).arg(on).end()
            }
            Notice::Stream(stream) => stream.line(),
            Notice::ClickedOnSpot { call, hz } => {
                Line::new("clicked_on_spot").arg(call).arg(hz).end()
            }
            Notice::RxClickedOnSpot {
                trx,
                channel,
                call,
                hz,
            } => Line::new("rx_clicked_on_spot")
                .arg(trx)
                .arg(channel)
                .arg(call)
                .arg(hz)
                .end(),
            Notice::TxFootswitch { trx, pressed } => {
                Line::new("tx_footswitch").arg(trx).arg(pressed).end()
            }
            Notice::AppFocus(on) => Line::new("app_focus").arg(on).end(),
            Notice::RxSensors { trx, dbm } => Line::new("rx_sensors").arg(trx).arg(dbm).end(),
            Notice::RxChannelSensors { trx, channel, dbm } => Line::new("rx_channel_sensors")
                .arg(trx)
                .arg(channel)
                .arg(dbm)
                .end(),
            Notice::TxSensors {
                trx,
                mic_dbm,
                rms_w,
                peak_w,
                swr,
            } => Line::new("tx_sensors")
                .arg(trx)
                .arg(mic_dbm)
                .arg(rms_w)
                .arg(peak_w)
                .arg(swr)
                .end(),
            Notice::RxSmeter { trx, channel, dbm } => {
                Line::new("rx_smeter").arg(trx).arg(channel).arg(dbm).end()
            }
            Notice::TxPower(w) => Line::new("tx_power").arg(w).end(),
            Notice::TxSwr(swr) => Line::new("tx_swr").arg(swr).end(),
            Notice::CwMacrosEmpty => Line::new("cw_macros_empty").end(),
            Notice::CallsignSend(call) => Line::new("callsign_send").arg(call).end(),
        }
    }
}

/// What a client sends that cannot key the transmitter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Command {
    /// Start the SDR program (`start;`, §4.2): receive, not transmit.
    RadioStart,
    /// Stop the SDR program (`stop;`): the radio stops altogether.
    RadioStop,
    Set(Setting),
    Read(Query),
    Stream(StreamSetting),
    /// A spot on the SDR program's panorama (§4.3): callsign, mode, frequency, colour and a
    /// comment.
    Spot {
        call: Call,
        mode: Token,
        hz: Hz,
        colour: SpotColour,
        text: Text,
    },
    /// A spot taken off the panorama: by its callsign (§4.3), or by its callsign and its frequency,
    /// the form AetherSDR's TCI server takes (`TciProtocol::cmdSpotDelete` at `8394342` ignores a
    /// delete that has no frequency, and takes off the spot of that call within 100 Hz of it).
    SpotDelete {
        call: Call,
        hz: Option<Hz>,
    },
    SpotClear,
    /// Receive levels pushed every `interval_ms` (30–1000; the server's default when absent).
    RxSensorsEnable {
        on: bool,
        interval_ms: Option<u32>,
    },
    /// Transmit readings pushed every `interval_ms`.
    TxSensorsEnable {
        on: bool,
        interval_ms: Option<u32>,
    },
    CwMacrosSpeedUp(u32),
    CwMacrosSpeedDown(u32),
    /// Bring the SDR program's window to the front.
    SetInFocus,
}

impl Command {
    /// The wire text a client sends.
    pub fn render(&self) -> ClientText {
        ClientText(self.line())
    }

    fn line(&self) -> String {
        match self {
            Command::RadioStart => Line::new("start").end(),
            Command::RadioStop => Line::new("stop").end(),
            Command::Set(setting) => setting.line(),
            Command::Read(query) => query.line(),
            Command::Stream(stream) => stream.line(),
            Command::Spot {
                call,
                mode,
                hz,
                colour,
                text,
            } => Line::new("spot")
                .arg(call)
                .arg(mode)
                .arg(hz)
                .arg(colour)
                .text(text)
                .end(),
            Command::SpotDelete { call, hz } => {
                Line::new("spot_delete").arg(call).opt(hz.as_ref()).end()
            }
            Command::SpotClear => Line::new("spot_clear").end(),
            Command::RxSensorsEnable { on, interval_ms } => Line::new("rx_sensors_enable")
                .arg(on)
                .opt(interval_ms.as_ref())
                .end(),
            Command::TxSensorsEnable { on, interval_ms } => Line::new("tx_sensors_enable")
                .arg(on)
                .opt(interval_ms.as_ref())
                .end(),
            Command::CwMacrosSpeedUp(wpm) => Line::new("cw_macros_speed_up").arg(wpm).end(),
            Command::CwMacrosSpeedDown(wpm) => Line::new("cw_macros_speed_down").arg(wpm).end(),
            Command::SetInFocus => Line::new("set_in_focus").end(),
        }
    }
}

/// What ends a transmission. Never gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stop {
    /// `trx:<trx>,false;`. Where the server has no notion of an owner this ends any client's
    /// transmission, which is the safe direction.
    Trx(Trx),
    /// `tune:<trx>,false;`.
    Tune(Trx),
    /// `cw_macros_stop;`: the CW macro or message being sent, at once (§3.2).
    CwMacros,
    /// `cw_terminal:false;`: the transmitter returns to receive when the macro ends (§3.2.1).
    CwTerminal,
    /// `keyer:<trx>,false[,<ms>];`: a straight key released; `ms` is the length of the character
    /// just keyed (§4.4 `KEYER`).
    KeyUp { trx: Trx, ms: Option<u32> },
}

impl Stop {
    /// The wire text a client sends.
    pub fn render(&self) -> ClientText {
        ClientText(self.line())
    }

    fn line(&self) -> String {
        match self {
            Stop::Trx(trx) => Line::new("trx").arg(trx).arg(false).end(),
            Stop::Tune(trx) => Line::new("tune").arg(trx).arg(false).end(),
            Stop::CwMacros => Line::new("cw_macros_stop").end(),
            Stop::CwTerminal => Line::new("cw_terminal").arg(false).end(),
            Stop::KeyUp { trx, ms } => Line::new("keyer")
                .arg(trx)
                .arg(false)
                .opt(ms.as_ref())
                .end(),
        }
    }
}

/// What can start or extend a transmission. Rendered only inside the `tci` module
/// ([`Start::render`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Start {
    /// `trx:<trx>,true[,<source>];`. With [`Source::Tci`] the over carries the client's audio
    /// stream, but only while the server has that stream enabled: otherwise it takes the
    /// microphone (§4.2, TRX note). Without a source, the SDR program's selected microphone.
    Trx { trx: Trx, source: Option<Source> },
    /// `tune:<trx>,true;`: a carrier at the tune drive.
    Tune(Trx),
    /// `cw_macros:<trx>,<text>;`: queued after anything being sent (§3.5).
    CwMacros { trx: Trx, text: Text },
    /// `cw_msg:<trx>,<prefix>,<call>,<suffix>;`: replaces a message being sent (§3.2.2, §3.5).
    CwMsg {
        trx: Trx,
        prefix: Text,
        call: Text,
        suffix: Text,
    },
    /// `cw_msg:<call>;`: corrects the callsign of the message being sent, for the characters not
    /// yet sent. A correction changes what goes on the air, so it is a start.
    CwMsgCall(Text),
    /// `cw_terminal:true;`: the transmitter stays in transmit after a macro until terminal mode
    /// is turned off (§3.2.1).
    CwTerminal,
    /// `keyer:<trx>,true[,<ms>];`: a straight key pressed (§4.4 `KEYER`).
    KeyDown { trx: Trx, ms: Option<u32> },
}

/// Why a start cannot be rendered for a dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unrenderable {
    /// The dialect's `trx` takes no source; sent without one, the over would take the SDR's
    /// microphone.
    SourceNotInDialect,
}

impl Start {
    /// The wire text of this start for `dialect`. Private to the `tci` module: the admission a key
    /// must pass ([`super::admission::Admitted::render`]) is its only caller outside the tests.
    pub(in crate::tci) fn render(&self, dialect: &Dialect) -> Result<ClientText, Unrenderable> {
        let line = match self {
            Start::Trx { trx, source } => {
                if source.is_some() && !dialect.trx_source {
                    return Err(Unrenderable::SourceNotInDialect);
                }
                Line::new("trx")
                    .arg(trx)
                    .arg(true)
                    .opt(source.as_ref())
                    .end()
            }
            Start::Tune(trx) => Line::new("tune").arg(trx).arg(true).end(),
            Start::CwMacros { trx, text } => Line::new("cw_macros").arg(trx).text(text).end(),
            Start::CwMsg {
                trx,
                prefix,
                call,
                suffix,
            } => Line::new("cw_msg")
                .arg(trx)
                .text(prefix)
                .text(call)
                .text(suffix)
                .end(),
            Start::CwMsgCall(call) => Line::new("cw_msg").text(call).end(),
            Start::CwTerminal => Line::new("cw_terminal").arg(true).end(),
            Start::KeyDown { trx, ms } => {
                Line::new("keyer").arg(trx).arg(true).opt(ms.as_ref()).end()
            }
        };
        Ok(ClientText(line))
    }
}

/// A client's command, as a server reads it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Request {
    Command(Command),
    Stop(Stop),
    Start(Start),
}

impl Request {
    /// One command, read as a client's in `dialect`.
    pub fn parse(raw: &Raw<'_>, dialect: &Dialect) -> Result<Request, WireError> {
        request(raw.name(), &Args(raw.args()), dialect).map_err(|fault| WireError::Command {
            name: raw.name.clone(),
            fault,
        })
    }
}

/// Wire text a client sends: one command, `;` included. Made only by this module.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientText(String);

impl ClientText {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for ClientText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Wire text a server sends: one command, `;` included. Made only by this module.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerText(String);

impl ServerText {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for ServerText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Errors ──────────────────────────────────────────────────────────────────────────────────

/// What is wrong with a command whose name was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// A byte outside printable ASCII among the arguments (a command is an ASCII string, §3.1).
    NotAscii,
    /// More than [`MAX_ARGS`] arguments; how many.
    TooManyArgs(usize),
    /// The argument at this index is longer than [`MAX_ARG`].
    ArgTooLong(usize),
    /// A name this codec does not know. The protocol grows, and an unknown command is ignored.
    Unknown,
    /// A command the other side sends: a client's from a server, or a server's from a client.
    Direction,
    /// An argument count the command never has; the count.
    Arity(usize),
    /// The argument at this index is not what the command carries there.
    BadArg(usize),
}

/// Why a command is not a value. The command is dropped; the session counts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// The message is longer than [`MAX_MESSAGE`]; its length. Nothing in it is read.
    MessageTooLong(usize),
    /// The message holds more than [`MAX_COMMANDS`] commands; the rest is not read.
    TooManyCommands,
    /// The message ends in text with no `;`.
    Unterminated,
    /// A command longer than [`MAX_COMMAND`]; its length.
    CommandTooLong(usize),
    /// A name that is empty, longer than [`MAX_NAME`], or not letters, digits and `_`.
    BadName,
    /// The named command is malformed.
    Command { name: String, fault: Fault },
}

impl WireError {
    /// The command this error is about, in lower case, when its name was read.
    pub fn command(&self) -> Option<&str> {
        match self {
            WireError::Command { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn fault(&self) -> Option<Fault> {
        match self {
            WireError::Command { fault, .. } => Some(*fault),
            _ => None,
        }
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for WireError {}

// ── Splitting ───────────────────────────────────────────────────────────────────────────────

/// One command as it arrived: its name, in lower case, and its arguments, split and bounded but
/// not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raw<'a> {
    name: String,
    args: Vec<&'a str>,
}

impl<'a> Raw<'a> {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The arguments as they arrived: none for `name;`, one empty one for `name:;`.
    pub fn args(&self) -> &[&'a str] {
        &self.args
    }
}

/// The commands of one text message, in order ([`split`]).
#[derive(Debug, Clone)]
pub struct Split<'a> {
    rest: &'a str,
    taken: usize,
    whole: usize,
    done: bool,
}

/// Split one text message into its commands. Lazy, and bounded by [`MAX_MESSAGE`],
/// [`MAX_COMMANDS`], [`MAX_COMMAND`], [`MAX_NAME`], [`MAX_ARGS`] and [`MAX_ARG`]. An empty command
/// (`;;`) is skipped; the text after the last `;` must be whitespace.
pub fn split(message: &str) -> Split<'_> {
    Split {
        rest: message,
        taken: 0,
        whole: message.len(),
        done: false,
    }
}

impl<'a> Iterator for Split<'a> {
    type Item = Result<Raw<'a>, WireError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if self.whole > MAX_MESSAGE {
            self.done = true;
            return Some(Err(WireError::MessageTooLong(self.whole)));
        }
        loop {
            let rest = self
                .rest
                .trim_start_matches(|c: char| c.is_ascii_whitespace());
            if rest.is_empty() {
                self.done = true;
                return None;
            }
            if self.taken >= MAX_COMMANDS {
                self.done = true;
                return Some(Err(WireError::TooManyCommands));
            }
            let Some((command, after)) = rest.split_once(';') else {
                self.done = true;
                return Some(Err(WireError::Unterminated));
            };
            self.rest = after;
            self.taken += 1;
            if !command.is_empty() {
                return Some(lex(command));
            }
        }
    }
}

/// One command, from its name (leading whitespace already skipped) to just before its `;`.
fn lex(command: &str) -> Result<Raw<'_>, WireError> {
    if command.len() > MAX_COMMAND {
        return Err(WireError::CommandTooLong(command.len()));
    }
    let (name, args) = match command.split_once(':') {
        Some((name, args)) => (name, Some(args)),
        None => (command, None),
    };
    let name = name.trim_end_matches(' ');
    if name.is_empty()
        || name.len() > MAX_NAME
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(WireError::BadName);
    }
    let name = name.to_ascii_lowercase();
    let Some(args) = args else {
        return Ok(Raw {
            name,
            args: Vec::new(),
        });
    };
    if !args.bytes().all(printable) {
        return Err(WireError::Command {
            name,
            fault: Fault::NotAscii,
        });
    }
    let count = args.bytes().filter(|&b| b == b',').count() + 1;
    if count > MAX_ARGS {
        return Err(WireError::Command {
            name,
            fault: Fault::TooManyArgs(count),
        });
    }
    let args: Vec<&str> = args.split(',').collect();
    if let Some(index) = args.iter().position(|a| a.len() > MAX_ARG) {
        return Err(WireError::Command {
            name,
            fault: Fault::ArgTooLong(index),
        });
    }
    Ok(Raw { name, args })
}

/// Every notice in one message from a server, in order.
pub fn notices(message: &str) -> impl Iterator<Item = Result<Notice, WireError>> + '_ {
    split(message).map(|raw| raw.and_then(|raw| Notice::parse(&raw)))
}

/// Every request in one message from a client, in order, read in `dialect`.
pub fn requests<'a>(
    message: &'a str,
    dialect: &'a Dialect,
) -> impl Iterator<Item = Result<Request, WireError>> + 'a {
    split(message).map(move |raw| raw.and_then(|raw| Request::parse(&raw, dialect)))
}

// ── Reading ─────────────────────────────────────────────────────────────────────────────────

/// Names only a client sends: §3.2's CW commands and §4.3–§4.4's "Sent only by the TCI client".
const CLIENT_ONLY: [&str; 13] = [
    "spot",
    "spot_delete",
    "spot_clear",
    "cw_macros",
    "cw_msg",
    "cw_macros_stop",
    "cw_terminal",
    "keyer",
    "rx_sensors_enable",
    "tx_sensors_enable",
    "cw_macros_speed_up",
    "cw_macros_speed_down",
    "set_in_focus",
];

/// Names only a server sends: §4.1's initialization, §4.3–§4.5's "Sent only by the TCI server",
/// and the 1.x meters.
const SERVER_ONLY: [&str; 24] = [
    "vfo_limits",
    "if_limits",
    "trx_count",
    "channel_count",
    "channels_count",
    "device",
    "receive_only",
    "modulations_list",
    "protocol",
    "ready",
    "tx_enable",
    "tx_frequency",
    "clicked_on_spot",
    "rx_clicked_on_spot",
    "tx_footswitch",
    "tx_footcwitch",
    "app_focus",
    "rx_sensors",
    "rx_channel_sensors",
    "tx_sensors",
    "tx_power",
    "tx_swr",
    "cw_macros_empty",
    "callsign_send",
];

/// A command's arguments, read by position. Each reader trims spaces where its kind allows them.
struct Args<'r, 'a>(&'r [&'a str]);

impl<'a> Args<'_, 'a> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn arity(&self, n: usize) -> Result<(), Fault> {
        if self.len() == n {
            Ok(())
        } else {
            Err(Fault::Arity(self.len()))
        }
    }

    fn read<T>(&self, i: usize, f: impl FnOnce(&'a str) -> Option<T>) -> Result<T, Fault> {
        let arg = self.0.get(i).copied().ok_or(Fault::Arity(self.len()))?;
        f(arg).ok_or(Fault::BadArg(i))
    }

    fn trx(&self, i: usize) -> Result<Trx, Fault> {
        self.read(i, |a| uint(a).and_then(Trx::new))
    }

    fn channel(&self, i: usize) -> Result<Channel, Fault> {
        self.read(i, |a| uint(a).and_then(Channel::new))
    }

    fn hz(&self, i: usize) -> Result<Hz, Fault> {
        self.read(i, |a| int(a).and_then(Hz::new))
    }

    fn real(&self, i: usize) -> Result<Real, Fault> {
        self.read(i, Real::parse)
    }

    fn uint(&self, i: usize) -> Result<u32, Fault> {
        self.read(i, uint)
    }

    fn int(&self, i: usize) -> Result<i32, Fault> {
        self.read(i, |a| int(a).and_then(|v| i32::try_from(v).ok()))
    }

    fn boolean(&self, i: usize) -> Result<bool, Fault> {
        self.read(i, boolean)
    }

    fn word(&self, i: usize) -> Result<Word, Fault> {
        self.read(i, Word::parse)
    }

    fn token(&self, i: usize) -> Result<Token, Fault> {
        self.read(i, Token::parse)
    }

    fn call(&self, i: usize) -> Result<Call, Fault> {
        self.read(i, Call::parse)
    }

    fn text(&self, i: usize) -> Result<Text, Fault> {
        self.read(i, Text::from_wire)
    }

    fn source(&self, i: usize) -> Result<Source, Fault> {
        self.read(i, |a| Word::parse(a).map(Source::from_word))
    }

    /// An optional trailing unsigned argument at `i`.
    fn opt_uint(&self, i: usize) -> Result<Option<u32>, Fault> {
        if self.len() > i {
            self.uint(i).map(Some)
        } else {
            Ok(None)
        }
    }
}

/// `[0-9]+`, spaces around it allowed, as a u32.
fn uint(arg: &str) -> Option<u32> {
    let s = arg.trim_matches(' ');
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `-?[0-9]+`, spaces around it allowed, as an i64.
fn int(arg: &str) -> Option<i64> {
    let s = arg.trim_matches(' ');
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `true` or `false`, in any case.
fn boolean(arg: &str) -> Option<bool> {
    let s = arg.trim_matches(' ');
    if s.eq_ignore_ascii_case("true") {
        Some(true)
    } else if s.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// Which side's text is being read.
#[derive(Debug, Clone, Copy)]
enum Side {
    Server,
    /// A client in a dialect: its `drive:<n>;` is a write in 1.x and a read in 2.0.
    Client(DriveForm),
}

enum Param {
    Set(Setting),
    Read(Query),
}

/// The parameters of §4.2, written, reported or (by a client) read. `Ok(None)`: not one of them.
fn param(name: &str, a: &Args<'_, '_>, side: Side) -> Result<Option<Param>, Fault> {
    use Param::{Read, Set};
    let n = a.len();
    let reads = matches!(side, Side::Client(_));
    let arity = Err(Fault::Arity(n));
    if let Some(flag) = Flag::from_name(name) {
        return Ok(Some(match n {
            1 if reads => Read(Query::Flag {
                flag,
                trx: a.trx(0)?,
            }),
            2 => Set(Setting::Flag {
                flag,
                trx: a.trx(0)?,
                on: a.boolean(1)?,
            }),
            _ => return arity,
        }));
    }
    let param = match name {
        "dds" => match n {
            1 if reads => Read(Query::Dds(a.trx(0)?)),
            2 => Set(Setting::Dds {
                trx: a.trx(0)?,
                hz: a.hz(1)?,
            }),
            _ => return arity,
        },
        "if" => match n {
            2 if reads => Read(Query::If {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
            }),
            3 => Set(Setting::If {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
                hz: a.hz(2)?,
            }),
            _ => return arity,
        },
        "vfo" => match n {
            2 if reads => Read(Query::Vfo {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
            }),
            3 => Set(Setting::Vfo {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
                hz: a.hz(2)?,
            }),
            _ => return arity,
        },
        "modulation" => match n {
            1 if reads => Read(Query::Modulation(a.trx(0)?)),
            2 => Set(Setting::Modulation {
                trx: a.trx(0)?,
                channel: None,
                mode: a.word(1)?,
            }),
            3 => Set(Setting::Modulation {
                trx: a.trx(0)?,
                channel: Some(a.channel(1)?),
                mode: a.word(2)?,
            }),
            _ => return arity,
        },
        "drive" => match (n, side) {
            (0, Side::Client(_)) => Read(Query::Drive(None)),
            (1, Side::Client(DriveForm::PerTrx)) => Read(Query::Drive(Some(a.trx(0)?))),
            (1, _) => Set(Setting::Drive {
                trx: None,
                level: a.uint(0)?,
            }),
            (2, _) => Set(Setting::Drive {
                trx: Some(a.trx(0)?),
                level: a.uint(1)?,
            }),
            _ => return arity,
        },
        "tune_drive" => match n {
            1 if reads => Read(Query::TuneDrive(a.trx(0)?)),
            2 => Set(Setting::TuneDrive {
                trx: a.trx(0)?,
                level: a.uint(1)?,
            }),
            _ => return arity,
        },
        "rit_offset" => match n {
            1 if reads => Read(Query::RitOffset(a.trx(0)?)),
            2 => Set(Setting::RitOffset {
                trx: a.trx(0)?,
                hz: a.hz(1)?,
            }),
            _ => return arity,
        },
        "xit_offset" => match n {
            1 if reads => Read(Query::XitOffset(a.trx(0)?)),
            2 => Set(Setting::XitOffset {
                trx: a.trx(0)?,
                hz: a.hz(1)?,
            }),
            _ => return arity,
        },
        "rx_channel_enable" => match n {
            2 if reads => Read(Query::RxChannelEnable {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
            }),
            3 => Set(Setting::RxChannelEnable {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
                on: a.boolean(2)?,
            }),
            _ => return arity,
        },
        "rx_filter_band" => match n {
            1 if reads => Read(Query::RxFilterBand(a.trx(0)?)),
            3 => Set(Setting::RxFilterBand {
                trx: a.trx(0)?,
                low: a.hz(1)?,
                high: a.hz(2)?,
            }),
            _ => return arity,
        },
        "cw_macros_speed" => match n {
            0 if reads => Read(Query::CwMacrosSpeed),
            1 => Set(Setting::CwMacrosSpeed(a.uint(0)?)),
            _ => return arity,
        },
        "cw_macros_delay" => match n {
            0 if reads => Read(Query::CwMacrosDelay),
            1 => Set(Setting::CwMacrosDelay(a.uint(0)?)),
            _ => return arity,
        },
        "cw_keyer_speed" => match n {
            1 => Set(Setting::CwKeyerSpeed(a.uint(0)?)),
            _ => return arity,
        },
        "volume" => match n {
            0 if reads => Read(Query::Volume),
            1 => Set(Setting::Volume(a.int(0)?)),
            _ => return arity,
        },
        "mute" => match n {
            0 if reads => Read(Query::Mute),
            1 => Set(Setting::Mute(a.boolean(0)?)),
            _ => return arity,
        },
        "rx_volume" => match n {
            2 if reads => Read(Query::RxVolume {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
            }),
            3 => Set(Setting::RxVolume {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
                db: a.int(2)?,
            }),
            _ => return arity,
        },
        "rx_balance" => match n {
            2 if reads => Read(Query::RxBalance {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
            }),
            3 => Set(Setting::RxBalance {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
                db: a.int(2)?,
            }),
            _ => return arity,
        },
        "mon_volume" => match n {
            0 if reads => Read(Query::MonVolume),
            1 => Set(Setting::MonVolume(a.int(0)?)),
            _ => return arity,
        },
        "mon_enable" => match n {
            0 if reads => Read(Query::MonEnable),
            1 => Set(Setting::MonEnable(a.boolean(0)?)),
            _ => return arity,
        },
        "agc_mode" => match n {
            1 if reads => Read(Query::AgcMode(a.trx(0)?)),
            2 => Set(Setting::AgcMode {
                trx: a.trx(0)?,
                mode: a.read(1, AgcMode::parse)?,
            }),
            _ => return arity,
        },
        "agc_gain" => match n {
            1 if reads => Read(Query::AgcGain(a.trx(0)?)),
            2 => Set(Setting::AgcGain {
                trx: a.trx(0)?,
                db: a.int(1)?,
            }),
            _ => return arity,
        },
        "rx_nb_param" => match n {
            1 if reads => Read(Query::NbParam(a.trx(0)?)),
            3 => Set(Setting::NbParam {
                trx: a.trx(0)?,
                threshold: a.uint(1)?,
                duration: a.uint(2)?,
            }),
            _ => return arity,
        },
        "sql_level" => match n {
            1 if reads => Read(Query::SqlLevel(a.trx(0)?)),
            2 => Set(Setting::SqlLevel {
                trx: a.trx(0)?,
                db: a.int(1)?,
            }),
            _ => return arity,
        },
        "digl_offset" => match n {
            0 if reads => Read(Query::DiglOffset),
            1 => Set(Setting::DiglOffset(a.uint(0)?)),
            _ => return arity,
        },
        "digu_offset" => match n {
            0 if reads => Read(Query::DiguOffset),
            1 => Set(Setting::DiguOffset(a.uint(0)?)),
            _ => return arity,
        },
        _ => return Ok(None),
    };
    Ok(Some(param))
}

/// §4.3's stream commands, the same from either side, one argument each. `Ok(None)`: not one of
/// them.
fn stream(name: &str, a: &Args<'_, '_>) -> Result<Option<StreamSetting>, Fault> {
    type Read = fn(&Args<'_, '_>) -> Result<StreamSetting, Fault>;
    let read: Read = match name {
        "audio_start" => |a| Ok(StreamSetting::AudioStart(a.trx(0)?)),
        "audio_stop" => |a| Ok(StreamSetting::AudioStop(a.trx(0)?)),
        "iq_start" => |a| Ok(StreamSetting::IqStart(a.trx(0)?)),
        "iq_stop" => |a| Ok(StreamSetting::IqStop(a.trx(0)?)),
        "line_out_start" => |a| Ok(StreamSetting::LineOutStart(a.trx(0)?)),
        "line_out_stop" => |a| Ok(StreamSetting::LineOutStop(a.trx(0)?)),
        "audio_samplerate" => |a| {
            let rate = a.read(0, |s| uint(s).and_then(AudioRate::new))?;
            Ok(StreamSetting::AudioSampleRate(rate))
        },
        "iq_samplerate" => |a| {
            let rate = a.read(0, |s| uint(s).and_then(IqRate::new))?;
            Ok(StreamSetting::IqSampleRate(rate))
        },
        "audio_stream_sample_type" => |a| {
            let format = a.read(0, |s| SampleFormat::from_word(s.trim_matches(' ')))?;
            Ok(StreamSetting::AudioSampleType(format))
        },
        "audio_stream_channels" => |a| {
            let channels = a.read(0, |s| uint(s).and_then(Channels::from_count))?;
            Ok(StreamSetting::AudioChannels(channels))
        },
        "audio_stream_samples" => |a| Ok(StreamSetting::AudioSamples(a.uint(0)?)),
        "tx_stream_audio_buffering" => |a| Ok(StreamSetting::TxAudioBuffering(a.uint(0)?)),
        _ => return Ok(None),
    };
    a.arity(1)?;
    read(a).map(Some)
}

fn notice(name: &str, a: &Args<'_, '_>) -> Result<Notice, Fault> {
    let n = a.len();
    Ok(match name {
        "vfo_limits" | "if_limits" => {
            a.arity(2)?;
            let (low, high) = (a.hz(0)?, a.hz(1)?);
            if name == "vfo_limits" {
                Notice::VfoLimits { low, high }
            } else {
                Notice::IfLimits { low, high }
            }
        }
        "trx_count" => {
            a.arity(1)?;
            Notice::TrxCount(a.uint(0)?)
        }
        "channel_count" | "channels_count" => {
            a.arity(1)?;
            Notice::ChannelCount(a.uint(0)?)
        }
        "device" => {
            a.arity(1)?;
            Notice::Device(a.token(0)?)
        }
        "receive_only" => {
            a.arity(1)?;
            Notice::ReceiveOnly(a.boolean(0)?)
        }
        "modulations_list" => {
            if n == 0 {
                return Err(Fault::Arity(0));
            }
            let modes = (0..n).map(|i| a.word(i)).collect::<Result<Vec<_>, _>>()?;
            Notice::ModulationsList(Modes::new(modes).ok_or(Fault::TooManyArgs(n))?)
        }
        "protocol" => {
            a.arity(2)?;
            Notice::Protocol {
                program: a.token(0)?,
                version: a.token(1)?,
            }
        }
        "ready" => {
            a.arity(0)?;
            Notice::Ready
        }
        "start" => {
            a.arity(0)?;
            Notice::RadioStart
        }
        "stop" => {
            a.arity(0)?;
            Notice::RadioStop
        }
        "trx" => match n {
            2 | 3 => Notice::Trx {
                trx: a.trx(0)?,
                on: a.boolean(1)?,
                source: if n == 3 { Some(a.source(2)?) } else { None },
            },
            _ => return Err(Fault::Arity(n)),
        },
        "tune" => {
            a.arity(2)?;
            Notice::Tune {
                trx: a.trx(0)?,
                on: a.boolean(1)?,
            }
        }
        "tx_enable" => {
            a.arity(2)?;
            Notice::TxEnable {
                trx: a.trx(0)?,
                on: a.boolean(1)?,
            }
        }
        "tx_frequency" => {
            a.arity(1)?;
            Notice::TxFrequency(a.hz(0)?)
        }
        "vfo_lock" => {
            a.arity(3)?;
            Notice::VfoLock {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
                on: a.boolean(2)?,
            }
        }
        "clicked_on_spot" => {
            a.arity(2)?;
            Notice::ClickedOnSpot {
                call: a.call(0)?,
                hz: a.hz(1)?,
            }
        }
        "rx_clicked_on_spot" => {
            a.arity(4)?;
            Notice::RxClickedOnSpot {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
                call: a.call(2)?,
                hz: a.hz(3)?,
            }
        }
        "tx_footswitch" | "tx_footcwitch" => {
            a.arity(2)?;
            Notice::TxFootswitch {
                trx: a.trx(0)?,
                pressed: a.boolean(1)?,
            }
        }
        "app_focus" => {
            a.arity(1)?;
            Notice::AppFocus(a.boolean(0)?)
        }
        "rx_sensors" => {
            a.arity(2)?;
            Notice::RxSensors {
                trx: a.trx(0)?,
                dbm: a.real(1)?,
            }
        }
        "rx_channel_sensors" => {
            a.arity(3)?;
            Notice::RxChannelSensors {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
                dbm: a.real(2)?,
            }
        }
        "tx_sensors" => {
            a.arity(5)?;
            Notice::TxSensors {
                trx: a.trx(0)?,
                mic_dbm: a.real(1)?,
                rms_w: a.real(2)?,
                peak_w: a.real(3)?,
                swr: a.real(4)?,
            }
        }
        "rx_smeter" => {
            a.arity(3)?;
            Notice::RxSmeter {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
                dbm: a.real(2)?,
            }
        }
        "tx_power" => {
            a.arity(1)?;
            Notice::TxPower(a.real(0)?)
        }
        "tx_swr" => {
            a.arity(1)?;
            Notice::TxSwr(a.real(0)?)
        }
        "cw_macros_empty" => {
            a.arity(0)?;
            Notice::CwMacrosEmpty
        }
        "callsign_send" => {
            a.arity(1)?;
            Notice::CallsignSend(a.call(0)?)
        }
        _ => {
            if let Some(s) = stream(name, a)? {
                return Ok(Notice::Stream(s));
            }
            return match param(name, a, Side::Server)? {
                Some(Param::Set(setting)) => Ok(Notice::State(setting)),
                // A read is a client's.
                Some(Param::Read(_)) => Err(Fault::Direction),
                None if CLIENT_ONLY.contains(&name) => Err(Fault::Direction),
                None => Err(Fault::Unknown),
            };
        }
    })
}

fn request(name: &str, a: &Args<'_, '_>, dialect: &Dialect) -> Result<Request, Fault> {
    let n = a.len();
    let command = |c| Ok(Request::Command(c));
    match name {
        "start" => {
            a.arity(0)?;
            command(Command::RadioStart)
        }
        "stop" => {
            a.arity(0)?;
            command(Command::RadioStop)
        }
        "trx" => match n {
            1 => command(Command::Read(Query::Trx(a.trx(0)?))),
            2 | 3 => {
                let trx = a.trx(0)?;
                if a.boolean(1)? {
                    let source = if n == 3 { Some(a.source(2)?) } else { None };
                    Ok(Request::Start(Start::Trx { trx, source }))
                } else {
                    // A stop is read whatever source it names: refusing one is the unsafe way.
                    Ok(Request::Stop(Stop::Trx(trx)))
                }
            }
            _ => Err(Fault::Arity(n)),
        },
        "tune" => match n {
            1 => command(Command::Read(Query::Tune(a.trx(0)?))),
            2 => {
                let trx = a.trx(0)?;
                Ok(if a.boolean(1)? {
                    Request::Start(Start::Tune(trx))
                } else {
                    Request::Stop(Stop::Tune(trx))
                })
            }
            _ => Err(Fault::Arity(n)),
        },
        "keyer" => match n {
            2 | 3 => {
                let trx = a.trx(0)?;
                let ms = a.opt_uint(2)?;
                Ok(if a.boolean(1)? {
                    Request::Start(Start::KeyDown { trx, ms })
                } else {
                    Request::Stop(Stop::KeyUp { trx, ms })
                })
            }
            _ => Err(Fault::Arity(n)),
        },
        "cw_macros" => {
            a.arity(2)?;
            Ok(Request::Start(Start::CwMacros {
                trx: a.trx(0)?,
                text: a.text(1)?,
            }))
        }
        "cw_msg" => match n {
            1 => Ok(Request::Start(Start::CwMsgCall(a.text(0)?))),
            4 => Ok(Request::Start(Start::CwMsg {
                trx: a.trx(0)?,
                prefix: a.text(1)?,
                call: a.text(2)?,
                suffix: a.text(3)?,
            })),
            _ => Err(Fault::Arity(n)),
        },
        "cw_terminal" => {
            a.arity(1)?;
            Ok(if a.boolean(0)? {
                Request::Start(Start::CwTerminal)
            } else {
                Request::Stop(Stop::CwTerminal)
            })
        }
        "cw_macros_stop" => {
            a.arity(0)?;
            Ok(Request::Stop(Stop::CwMacros))
        }
        "spot" => {
            a.arity(5)?;
            command(Command::Spot {
                call: a.call(0)?,
                mode: a.token(1)?,
                hz: a.hz(2)?,
                colour: a.read(3, SpotColour::parse)?,
                text: a.text(4)?,
            })
        }
        "spot_delete" => {
            if !(1..=2).contains(&n) {
                return Err(Fault::Arity(n));
            }
            command(Command::SpotDelete {
                call: a.call(0)?,
                hz: if n == 2 { Some(a.hz(1)?) } else { None },
            })
        }
        "spot_clear" => {
            a.arity(0)?;
            command(Command::SpotClear)
        }
        "rx_sensors_enable" | "tx_sensors_enable" => {
            if !(1..=2).contains(&n) {
                return Err(Fault::Arity(n));
            }
            let (on, interval_ms) = (a.boolean(0)?, a.opt_uint(1)?);
            command(if name == "rx_sensors_enable" {
                Command::RxSensorsEnable { on, interval_ms }
            } else {
                Command::TxSensorsEnable { on, interval_ms }
            })
        }
        "cw_macros_speed_up" => {
            a.arity(1)?;
            command(Command::CwMacrosSpeedUp(a.uint(0)?))
        }
        "cw_macros_speed_down" => {
            a.arity(1)?;
            command(Command::CwMacrosSpeedDown(a.uint(0)?))
        }
        "set_in_focus" => {
            a.arity(0)?;
            command(Command::SetInFocus)
        }
        // A client reads these; a server sets them.
        "vfo_lock" => match n {
            2 => command(Command::Read(Query::VfoLock {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
            })),
            3 => Err(Fault::Direction),
            _ => Err(Fault::Arity(n)),
        },
        "rx_smeter" => match n {
            2 => command(Command::Read(Query::RxSmeter {
                trx: a.trx(0)?,
                channel: a.channel(1)?,
            })),
            3 => Err(Fault::Direction),
            _ => Err(Fault::Arity(n)),
        },
        _ => {
            if let Some(s) = stream(name, a)? {
                return command(Command::Stream(s));
            }
            match param(name, a, Side::Client(dialect.drive))? {
                Some(Param::Set(setting)) => command(Command::Set(setting)),
                Some(Param::Read(query)) => command(Command::Read(query)),
                None if SERVER_ONLY.contains(&name) => Err(Fault::Direction),
                None => Err(Fault::Unknown),
            }
        }
    }
}

// ── A server's reading of a client ──────────────────────────────────────────────────────────

/// A client's command as Nexus's TCI server reads it: a [`Request`], or one of the forms real
/// clients send that the specification gives no client ([`Extra`]). The tokenizer is the one
/// [`notices`] and [`requests`] use; [`inbound`] reads a whole message.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Inbound {
    Request(Request),
    Extra(Extra),
}

impl Inbound {
    /// One command, read as a client's in `dialect`: an [`Extra`] when it is one, otherwise what
    /// [`Request::parse`] reads.
    pub fn parse(raw: &Raw<'_>, dialect: &Dialect) -> Result<Inbound, WireError> {
        match extra(raw.name(), &Args(raw.args())) {
            Ok(Some(e)) => Ok(Inbound::Extra(e)),
            Ok(None) => Request::parse(raw, dialect).map(Inbound::Request),
            Err(fault) => Err(WireError::Command {
                name: raw.name.clone(),
                fault,
            }),
        }
    }
}

/// What real clients send a server that a [`Request`] does not read. None of them keys: a server
/// answers each with the value in force, or refuses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Extra {
    /// `split_enable:<bool>;` with no receiver. WSJT-X 3.0.2 sends `split_enable:false;` two
    /// seconds after it connects (`Transceiver/TCITransceiver.cpp:432-435`).
    Split(bool),
    /// `rx_enable:<trx>,<bool>;`, TCI 1.6's receiver switch: not in the 2.0 document, but in
    /// eesdr-tci's command table (`tci.py`). MSHV writes `rx_enable:<trx>,true;` on the first
    /// `vfo` it reads (`network.cpp:2482-2491`).
    RxEnable { trx: Trx, on: bool },
    /// `rx_enable:<trx>;`, its read.
    RxEnableRead(Trx),
    /// `tx_enable:<trx>,<bool>;` from a client. The spec gives it to the server alone, to tell
    /// clients whether the transmitter may transmit (§4.3); MSHV writes `tx_enable:<trx>,true;` on
    /// the first `vfo` (`network.cpp:2482-2491`). A server never arms on it.
    TxEnable { trx: Trx, on: bool },
    /// `line_out_recorder_start`, `line_out_recorder_save` or `line_out_recorder_break` (§4.3):
    /// they make the server's computer record, and write a file at a path the client names. Read
    /// only so that the refusal can name them; no argument is kept.
    LineOutRecorder,
}

/// Every command in one message from a client, in order, as Nexus's server reads it.
pub fn inbound<'a>(
    message: &'a str,
    dialect: &'a Dialect,
) -> impl Iterator<Item = Result<Inbound, WireError>> + 'a {
    split(message).map(move |raw| raw.and_then(|raw| Inbound::parse(&raw, dialect)))
}

/// The [`Extra`] forms. `Ok(None)`: not one of them, so the command is read as a [`Request`].
fn extra(name: &str, a: &Args<'_, '_>) -> Result<Option<Extra>, Fault> {
    let n = a.len();
    Ok(Some(match name {
        // Only the form with no receiver: `split_enable:<trx>;` (a read) and
        // `split_enable:<trx>,<bool>;` (a write) are requests.
        "split_enable" if n == 1 && a.0.first().copied().and_then(boolean).is_some() => {
            Extra::Split(a.boolean(0)?)
        }
        "rx_enable" => match n {
            1 => Extra::RxEnableRead(a.trx(0)?),
            2 => Extra::RxEnable {
                trx: a.trx(0)?,
                on: a.boolean(1)?,
            },
            _ => return Err(Fault::Arity(n)),
        },
        "tx_enable" if n == 2 => Extra::TxEnable {
            trx: a.trx(0)?,
            on: a.boolean(1)?,
        },
        "line_out_recorder_start" | "line_out_recorder_save" | "line_out_recorder_break" => {
            Extra::LineOutRecorder
        }
        _ => return Ok(None),
    }))
}

/// What Nexus's server sends that [`Notice`] does not carry: its answers to [`Extra::RxEnable`]
/// and to `cw_terminal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtraNotice {
    /// `rx_enable:<trx>,<bool>;`
    RxEnable { trx: Trx, on: bool },
    /// `cw_terminal:<bool>;`: whether the transmitter stays keyed after a CW macro (§3.2.1).
    CwTerminal(bool),
}

impl ExtraNotice {
    /// The wire text a server sends.
    pub fn render(&self) -> ServerText {
        ServerText(match self {
            ExtraNotice::RxEnable { trx, on } => Line::new("rx_enable").arg(trx).arg(on).end(),
            ExtraNotice::CwTerminal(on) => Line::new("cw_terminal").arg(on).end(),
        })
    }
}

// ── Rendering ───────────────────────────────────────────────────────────────────────────────

/// One command's wire text: the name, then `:` and the arguments joined by `,`, then `;`.
struct Line {
    text: String,
    args: usize,
}

impl Line {
    fn new(name: &str) -> Line {
        Line {
            text: name.to_string(),
            args: 0,
        }
    }

    fn separator(&mut self) {
        self.text.push(if self.args == 0 { ':' } else { ',' });
        self.args += 1;
    }

    fn arg(mut self, value: impl fmt::Display) -> Line {
        self.separator();
        // Writing into a String cannot fail.
        let _ = write!(self.text, "{value}");
        self
    }

    fn opt(self, value: Option<impl fmt::Display>) -> Line {
        match value {
            Some(v) => self.arg(v),
            None => self,
        }
    }

    fn text(mut self, text: &Text) -> Line {
        self.separator();
        text.escape_into(&mut self.text);
        self
    }

    fn end(mut self) -> String {
        self.text.push(';');
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tci::{DIALECTS, EXPERT_SDR2, EXPERT_SDR3, MAX_CHANNELS, MAX_TRX};
    use proptest::prelude::*;

    fn trx(n: u32) -> Trx {
        Trx::new(n).expect("a receiver number below MAX_TRX")
    }

    fn one_notice(text: &str) -> Result<Notice, WireError> {
        let mut all: Vec<_> = notices(text).collect();
        assert_eq!(all.len(), 1, "{text:?} holds one command: {all:?}");
        all.remove(0)
    }

    fn one_request(text: &str, dialect: &Dialect) -> Result<Request, WireError> {
        let mut all: Vec<_> = requests(text, dialect).collect();
        assert_eq!(all.len(), 1, "{text:?} holds one command: {all:?}");
        all.remove(0)
    }

    fn fault(name: &str, fault: Fault) -> WireError {
        WireError::Command {
            name: name.to_string(),
            fault,
        }
    }

    fn request_line(r: &Request, d: &Dialect) -> String {
        match r {
            Request::Command(c) => c.render().into_string(),
            Request::Stop(s) => s.render().into_string(),
            Request::Start(s) => s.render(d).expect("renders in its dialect").into_string(),
        }
    }

    // ── The spec's examples ──

    const CORPUS: &str = include_str!("../../tests/fixtures/tci-2.0-examples.txt");

    /// One line of the fixture: where the example is from, who sends it, the example, and what
    /// reading it must give.
    struct Example<'a> {
        place: &'a str,
        sides: &'a str,
        text: &'a str,
        expect: Expect<'a>,
    }

    enum Expect<'a> {
        /// Read, and rendered back as itself (case aside).
        Same,
        /// Read, and rendered as this.
        Renders(&'a str),
        /// Refused for this reason.
        Refused(&'a str),
    }

    fn corpus() -> Vec<Example<'static>> {
        CORPUS
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|l| {
                let parts: Vec<&str> = l.split('`').collect();
                assert!(parts.len() == 3 || parts.len() == 5, "fixture line {l:?}");
                let head: Vec<&str> = parts[0].split_whitespace().collect();
                assert_eq!(head.len(), 2, "fixture line {l:?}");
                let tail = parts[2].trim();
                let expect = if parts.len() == 5 {
                    assert_eq!(tail, "=>", "fixture line {l:?}");
                    Expect::Renders(parts[3])
                } else if let Some(reason) = tail.strip_prefix('!') {
                    Expect::Refused(reason.trim())
                } else {
                    assert_eq!(tail, "", "fixture line {l:?}");
                    Expect::Same
                };
                Example {
                    place: head[0],
                    sides: head[1],
                    text: parts[1],
                    expect,
                }
            })
            .collect()
    }

    fn fault_named(reason: &str) -> Fault {
        let mut words = reason.split_whitespace();
        let kind = words.next().unwrap_or("");
        let n = words
            .next()
            .and_then(|w| w.parse().ok())
            .unwrap_or(usize::MAX);
        match kind {
            "unknown" => Fault::Unknown,
            "direction" => Fault::Direction,
            "arity" => Fault::Arity(n),
            "badarg" => Fault::BadArg(n),
            other => panic!("fixture reason {other:?}"),
        }
    }

    /// Every example in the TCI 2.0 document (and each dialect form a known server sends)
    /// reads as the side that sends it reads it, renders back, and is refused by the other side.
    #[test]
    fn every_spec_example_reads_as_its_sender_meant_and_renders_back() {
        let examples = corpus();
        assert!(examples.len() > 150, "{} examples", examples.len());
        for ex in &examples {
            let at = format!("{} {:?}", ex.place, ex.text);
            let server = ex.sides.contains('s');
            let client = ex.sides.contains('c');
            assert!(server || client, "{at}: sides {:?}", ex.sides);
            if server {
                let got = one_notice(ex.text);
                match &ex.expect {
                    Expect::Refused(reason) => {
                        assert_eq!(
                            got.as_ref().err().and_then(WireError::fault),
                            Some(fault_named(reason)),
                            "{at} as a notice: {got:?}"
                        );
                    }
                    Expect::Same | Expect::Renders(_) => {
                        let n = got.unwrap_or_else(|e| panic!("{at} as a notice: {e:?}"));
                        let line = n.render().into_string();
                        let want = match ex.expect {
                            Expect::Renders(r) => r,
                            _ => ex.text,
                        };
                        assert!(
                            line.eq_ignore_ascii_case(want),
                            "{at}: renders {line:?}, not {want:?}"
                        );
                        assert_eq!(one_notice(&line), Ok(n), "{at}: its rendering reads back");
                    }
                }
            } else {
                assert!(
                    one_notice(ex.text).is_err(),
                    "{at}: a client's, refused as a notice"
                );
            }
            if client {
                let got = one_request(ex.text, &EXPERT_SDR3);
                match &ex.expect {
                    Expect::Refused(reason) => {
                        assert_eq!(
                            got.as_ref().err().and_then(WireError::fault),
                            Some(fault_named(reason)),
                            "{at} as a request: {got:?}"
                        );
                    }
                    Expect::Same | Expect::Renders(_) => {
                        let r = got.unwrap_or_else(|e| panic!("{at} as a request: {e:?}"));
                        let line = request_line(&r, &EXPERT_SDR3);
                        let want = match ex.expect {
                            Expect::Renders(r) => r,
                            _ => ex.text,
                        };
                        assert!(
                            line.eq_ignore_ascii_case(want),
                            "{at}: renders {line:?}, not {want:?}"
                        );
                        assert_eq!(
                            one_request(&line, &EXPERT_SDR3),
                            Ok(r),
                            "{at}: its rendering reads back"
                        );
                    }
                }
            } else {
                assert!(
                    one_request(ex.text, &EXPERT_SDR3).is_err(),
                    "{at}: a server's, refused as a request"
                );
            }
        }
    }

    #[test]
    fn the_corpus_covers_every_command_this_codec_reads() {
        let examples = corpus();
        let named = |name: &str| {
            examples.iter().any(|ex| {
                ex.text
                    .split([':', ';'])
                    .next()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
        };
        let mut missing: Vec<&str> = CLIENT_ONLY
            .iter()
            .chain(SERVER_ONLY.iter())
            .copied()
            .chain(Flag::ALL.iter().map(|f| f.name()))
            .chain([
                "start",
                "stop",
                "dds",
                "if",
                "vfo",
                "modulation",
                "trx",
                "tune",
                "drive",
                "tune_drive",
                "rit_offset",
                "xit_offset",
                "rx_channel_enable",
                "rx_filter_band",
                "cw_macros_speed",
                "cw_macros_delay",
                "cw_keyer_speed",
                "volume",
                "mute",
                "rx_volume",
                "rx_balance",
                "mon_volume",
                "mon_enable",
                "agc_mode",
                "agc_gain",
                "rx_nb_param",
                "sql_level",
                "digl_offset",
                "digu_offset",
                "vfo_lock",
                "rx_smeter",
                "audio_start",
                "audio_stop",
                "iq_start",
                "iq_stop",
                "line_out_start",
                "line_out_stop",
                "audio_samplerate",
                "iq_samplerate",
                "audio_stream_sample_type",
                "audio_stream_channels",
                "audio_stream_samples",
                "tx_stream_audio_buffering",
            ])
            .filter(|n| !named(n))
            .collect();
        missing.sort_unstable();
        assert!(missing.is_empty(), "no fixture names {missing:?}");
    }

    // ── Splitting ──

    #[test]
    fn a_message_carries_several_commands_and_whitespace_between_them() {
        let got: Vec<_> = notices("vfo:0,0,14074000;modulation:0,USB;\r\n  ready;  \n").collect();
        assert_eq!(
            got,
            [
                Ok(Notice::State(Setting::Vfo {
                    trx: trx(0),
                    channel: Channel::A,
                    hz: Hz(14_074_000),
                })),
                Ok(Notice::State(Setting::Modulation {
                    trx: trx(0),
                    channel: None,
                    mode: Word("usb".into()),
                })),
                Ok(Notice::Ready),
            ]
        );
        // Empty commands are skipped; nothing at all is no commands.
        assert_eq!(notices(";;ready;;").count(), 1);
        assert_eq!(notices("").count(), 0);
        assert_eq!(notices(" \r\n\t").count(), 0);
    }

    #[test]
    fn a_bad_command_is_refused_and_the_ones_around_it_still_read() {
        let got: Vec<_> = notices("ready;vfo:0,0,abc;trx:0,false;").collect();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], Ok(Notice::Ready));
        assert_eq!(got[1], Err(fault("vfo", Fault::BadArg(2))));
        assert_eq!(
            got[2],
            Ok(Notice::Trx {
                trx: trx(0),
                on: false,
                source: None
            })
        );
    }

    #[test]
    fn names_are_read_in_any_case_and_rendered_in_lower_case() {
        let n = one_notice("TrX:0,TRUE,TCI;").expect("reads");
        assert_eq!(
            n,
            Notice::Trx {
                trx: trx(0),
                on: true,
                source: Some(Source::Tci)
            }
        );
        assert_eq!(n.render().as_str(), "trx:0,true,tci;");
        let raw = split("VFO_LIMITS:10000,30000000;").next().unwrap().unwrap();
        assert_eq!(raw.name(), "vfo_limits");
    }

    #[test]
    fn text_after_the_last_semicolon_is_unterminated() {
        let got: Vec<_> = notices("ready;trx:0,true").collect();
        assert_eq!(got, [Ok(Notice::Ready), Err(WireError::Unterminated)]);
        assert_eq!(
            notices("ready").collect::<Vec<_>>(),
            [Err(WireError::Unterminated)]
        );
    }

    #[test]
    fn a_malformed_name_is_refused() {
        for text in [":0;", " :0;", "vf o:0;", "vfo-x:0;", "vfé:0;", "\0;"] {
            assert_eq!(
                split(text).next(),
                Some(Err(WireError::BadName)),
                "{text:?}"
            );
        }
        // A space before the colon is allowed; the name is what precedes it.
        assert_eq!(split("ready ;").next().unwrap().unwrap().name(), "ready");
    }

    #[test]
    fn a_byte_outside_printable_ascii_in_the_arguments_is_refused() {
        for text in [
            "vfo:0,0,140\t74000;",
            "device:Sun\u{e9}SDR;",
            "device:a\rb;",
        ] {
            assert_eq!(
                split(text).next(),
                Some(Err(WireError::Command {
                    name: text.split(':').next().unwrap().to_string(),
                    fault: Fault::NotAscii
                })),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_raw_reserved_character_inside_an_argument_is_refused() {
        // `:` past the first is not a separator; a sender that did not escape it is not guessed at.
        assert_eq!(
            one_request("cw_macros:0,TU:599;", &EXPERT_SDR3),
            Err(fault("cw_macros", Fault::BadArg(1)))
        );
        assert_eq!(
            one_notice("device:Sun:SDR;"),
            Err(fault("device", Fault::BadArg(0)))
        );
        assert_eq!(
            one_notice("vfo:0,0:1,7000000;"),
            Err(fault("vfo", Fault::BadArg(1)))
        );
    }

    // ── One test per bound ──

    #[test]
    fn a_message_over_max_message_is_refused_whole() {
        let mut message = "ready;".repeat(MAX_MESSAGE / 6);
        message.push_str("ready;");
        assert!(message.len() > MAX_MESSAGE);
        assert_eq!(
            notices(&message).collect::<Vec<_>>(),
            [Err(WireError::MessageTooLong(message.len()))]
        );
        // At the bound it is read.
        let at = " ".repeat(MAX_MESSAGE - 6) + "ready;";
        assert_eq!(notices(&at).collect::<Vec<_>>(), [Ok(Notice::Ready)]);
    }

    #[test]
    fn a_message_stops_after_max_commands() {
        let message = "ready;".repeat(MAX_COMMANDS + 5);
        let got: Vec<_> = notices(&message).collect();
        assert_eq!(got.len(), MAX_COMMANDS + 1);
        assert!(got[..MAX_COMMANDS].iter().all(|r| *r == Ok(Notice::Ready)));
        assert_eq!(got[MAX_COMMANDS], Err(WireError::TooManyCommands));
        // Empty commands count, so `;;;…` cannot spin past the bound either.
        let message = ";".repeat(MAX_COMMANDS) + "ready;";
        assert_eq!(
            notices(&message).collect::<Vec<_>>(),
            [Err(WireError::TooManyCommands)]
        );
        let message = ";".repeat(MAX_COMMANDS - 1) + "ready;";
        assert_eq!(notices(&message).collect::<Vec<_>>(), [Ok(Notice::Ready)]);
    }

    #[test]
    fn a_command_over_max_command_is_refused() {
        // Five arguments, each within MAX_ARG, together past MAX_COMMAND: no command that long is
        // handed on, whatever its parts.
        let long = format!("x:{};", vec!["a".repeat(1000); 5].join(","));
        assert_eq!(
            split(&long).next(),
            Some(Err(WireError::CommandTooLong(5006)))
        );
        let at = format!("x:{};", "a".repeat(MAX_COMMAND - 2));
        assert!(matches!(
            split(&at).next(),
            Some(Err(WireError::Command { .. }))
        ));
        // The next command still reads.
        let got: Vec<_> = notices(&(long + "ready;")).collect();
        assert_eq!(got[1], Ok(Notice::Ready));
    }

    #[test]
    fn a_name_over_max_name_is_refused() {
        let at = "a".repeat(MAX_NAME) + ";";
        assert_eq!(
            one_notice(&at),
            Err(fault(&"a".repeat(MAX_NAME), Fault::Unknown))
        );
        let over = "a".repeat(MAX_NAME + 1) + ";";
        assert_eq!(split(&over).next(), Some(Err(WireError::BadName)));
    }

    #[test]
    fn more_than_max_args_are_refused() {
        // No command with more than MAX_ARGS arguments is handed on (nor its slices collected).
        let at = format!("x:{};", ",".repeat(MAX_ARGS - 1));
        assert_eq!(split(&at).next().unwrap().unwrap().args().len(), MAX_ARGS);
        let over = format!("x:{};", ",".repeat(MAX_ARGS));
        assert_eq!(
            split(&over).next(),
            Some(Err(fault("x", Fault::TooManyArgs(MAX_ARGS + 1))))
        );
        let modes = vec!["usb"; MAX_ARGS].join(",");
        let at = format!("modulations_list:{modes};");
        match one_notice(&at) {
            Ok(Notice::ModulationsList(m)) => assert_eq!(m.as_slice().len(), MAX_ARGS),
            other => panic!("{other:?}"),
        }
        let over = format!("modulations_list:{modes},lsb;");
        assert_eq!(
            one_notice(&over),
            Err(fault("modulations_list", Fault::TooManyArgs(MAX_ARGS + 1)))
        );
    }

    #[test]
    fn an_argument_over_max_arg_is_refused() {
        let at = format!("cw_macros:0,{};", "E".repeat(MAX_ARG));
        assert!(matches!(
            one_request(&at, &EXPERT_SDR3),
            Ok(Request::Start(Start::CwMacros { .. }))
        ));
        let over = format!("cw_macros:0,{};", "E".repeat(MAX_ARG + 1));
        assert_eq!(
            one_request(&over, &EXPERT_SDR3),
            Err(fault("cw_macros", Fault::ArgTooLong(1)))
        );
        // No argument longer than MAX_ARG is handed on, whatever the command.
        let over = format!("x:{};", "a".repeat(MAX_ARG + 1));
        assert_eq!(
            split(&over).next(),
            Some(Err(fault("x", Fault::ArgTooLong(0))))
        );
    }

    #[test]
    fn a_receiver_or_channel_number_past_its_bound_is_refused() {
        let last = MAX_TRX - 1;
        assert!(one_notice(&format!("trx:{last},false;")).is_ok());
        for n in [MAX_TRX, 20, 256, 300, u32::MAX] {
            assert_eq!(
                one_notice(&format!("trx:{n},false;")),
                Err(fault("trx", Fault::BadArg(0))),
                "{n}"
            );
        }
        let last = MAX_CHANNELS - 1;
        assert!(one_notice(&format!("vfo:0,{last},7000000;")).is_ok());
        for n in [MAX_CHANNELS, 9, 260] {
            assert_eq!(
                one_notice(&format!("vfo:0,{n},7000000;")),
                Err(fault("vfo", Fault::BadArg(1))),
                "{n}"
            );
        }
    }

    #[test]
    fn a_frequency_past_max_hz_is_refused() {
        assert!(one_notice(&format!("tx_frequency:{MAX_HZ};")).is_ok());
        assert!(one_notice(&format!("tx_frequency:-{MAX_HZ};")).is_ok());
        for hz in [MAX_HZ + 1, i64::MAX] {
            assert_eq!(
                one_notice(&format!("tx_frequency:{hz};")),
                Err(fault("tx_frequency", Fault::BadArg(0))),
                "{hz}"
            );
        }
        assert_eq!(
            one_notice("tx_frequency:-9223372036854775808;"),
            Err(fault("tx_frequency", Fault::BadArg(0)))
        );
        assert_eq!(
            one_notice("tx_frequency:99999999999999999999999;"),
            Err(fault("tx_frequency", Fault::BadArg(0)))
        );
        assert_eq!(Hz::new(MAX_HZ + 1), None);
        assert_eq!(Hz::new(i64::MIN), None);
    }

    #[test]
    fn a_real_past_max_real_or_with_more_than_nine_decimals_is_refused() {
        assert_eq!(
            Real::parse("1000000000"),
            Real::from_micros(MAX_REAL * 1_000_000)
        );
        assert_eq!(Real::parse("1000000000.000001"), None, "past MAX_REAL");
        assert_eq!(Real::parse("-1000000000.1"), None, "past MAX_REAL");
        assert_eq!(Real::parse("9999999999"), None, "past MAX_REAL");
        assert_eq!(Real::parse("99999999999"), None, "past MAX_REAL");
        assert_eq!(Real::parse(&"9".repeat(40)), None, "past i64");
        assert_eq!(Real::parse("1.123456789"), Real::from_micros(1_123_457));
        assert_eq!(Real::parse("1.1234567890"), None, "ten decimals");
        assert_eq!(Real::from_micros(MAX_REAL * 1_000_000 + 1), None);
        assert_eq!(Real::from_f64(1.0e10), None);
        assert_eq!(Real::from_f64(f64::NAN), None);
        assert_eq!(Real::from_f64(-71.5), Real::from_micros(-71_500_000));
    }

    #[test]
    fn a_token_word_or_call_past_its_bound_is_refused() {
        assert!(Token::new(&"a".repeat(MAX_TOKEN)).is_some());
        assert_eq!(Token::new(&"a".repeat(MAX_TOKEN + 1)), None);
        assert!(Call::new(&"A".repeat(MAX_TOKEN)).is_some());
        assert_eq!(Call::new(&"A".repeat(MAX_TOKEN + 1)), None);
        assert!(Word::new(&"a".repeat(MAX_WORD)).is_some());
        assert_eq!(Word::new(&"a".repeat(MAX_WORD + 1)), None);
        assert_eq!(
            one_notice(&format!("device:{};", "a".repeat(MAX_TOKEN + 1))),
            Err(fault("device", Fault::BadArg(0)))
        );
        assert_eq!(
            one_notice(&format!("modulation:0,{};", "u".repeat(MAX_WORD + 1))),
            Err(fault("modulation", Fault::BadArg(1)))
        );
    }

    #[test]
    fn a_text_past_max_arg_cannot_be_made() {
        assert!(Text::new(&"E".repeat(MAX_ARG)).is_some());
        assert_eq!(Text::new(&"E".repeat(MAX_ARG + 1)), None);
        // Nor read, even past the lexer's own bound.
        assert!(Text::from_wire(&"E".repeat(MAX_ARG)).is_some());
        assert_eq!(Text::from_wire(&"E".repeat(MAX_ARG + 1)), None);
    }

    /// A spot's colour in either form (the specification's decimal, AetherSDR's `#AARRGGBB`) and a
    /// delete with or without the spot's frequency each read back as they were sent. Anything else
    /// in the colour, or a third argument to a delete, is refused.
    #[test]
    fn a_spot_colour_and_a_delete_read_in_either_form() {
        let call = || Call::new("K1ABC").expect("a call");
        let spot = |colour| Command::Spot {
            call: call(),
            mode: Token::new("CW").expect("a mode"),
            hz: Hz::new(14_025_000).expect("a frequency"),
            colour,
            text: Text::new("New one").expect("text"),
        };
        let cases = [
            (
                "spot:K1ABC,CW,14025000,4294065856,New one;",
                spot(SpotColour::Decimal(0xFFF2_3EC0)),
            ),
            (
                "spot:K1ABC,CW,14025000,#FFF23EC0,New one;",
                spot(SpotColour::Hash(0xFFF2_3EC0)),
            ),
            (
                "spot_delete:K1ABC;",
                Command::SpotDelete {
                    call: call(),
                    hz: None,
                },
            ),
            (
                "spot_delete:K1ABC,14025000;",
                Command::SpotDelete {
                    call: call(),
                    hz: Hz::new(14_025_000),
                },
            ),
        ];
        for (text, command) in cases {
            assert_eq!(command.render().as_str(), text);
            let read: Vec<_> = requests(text, &EXPERT_SDR3).collect();
            assert_eq!(read, [Ok(Request::Command(command))], "{text}");
        }
        assert_eq!(
            SpotColour::parse("#fff23ec0"),
            Some(SpotColour::Hash(0xFFF2_3EC0))
        );
        for bad in [
            "spot:K1ABC,CW,14025000,#FFF23EC,x;",
            "spot:K1ABC,CW,14025000,#FFF23EC0A,x;",
            "spot:K1ABC,CW,14025000,#GGF23EC0,x;",
            "spot:K1ABC,CW,14025000,-1,x;",
            "spot:K1ABC,CW,14025000,red,x;",
            "spot_delete:K1ABC,14025000,1;",
        ] {
            let read: Vec<_> = requests(bad, &EXPERT_SDR3).collect();
            assert!(read.len() == 1 && read[0].is_err(), "{bad}: {read:?}");
        }
    }

    #[test]
    fn a_mode_list_past_max_args_cannot_be_made() {
        let usb = Word::new("usb").unwrap();
        assert!(Modes::new(vec![usb.clone(); MAX_ARGS]).is_some());
        assert_eq!(Modes::new(vec![usb; MAX_ARGS + 1]), None);
        assert_eq!(Modes::new(Vec::new()), None);
    }

    // ── Values ──

    #[test]
    fn free_text_escapes_the_reserved_characters_and_refuses_the_escapes() {
        let text = Text::new("CQ: K1ABC, QRZ; 73").expect("printable, no escapes");
        let line = Start::CwMacros {
            trx: trx(0),
            text: text.clone(),
        }
        .render(&EXPERT_SDR3)
        .unwrap();
        assert_eq!(line.as_str(), "cw_macros:0,CQ^ K1ABC~ QRZ* 73;");
        assert_eq!(
            one_request(line.as_str(), &EXPERT_SDR3),
            Ok(Request::Start(Start::CwMacros { trx: trx(0), text }))
        );
        // The spec's own example (§3.2.1) keeps its speed marks and prosign.
        assert_eq!(
            one_request("cw_macros:0,>TU >599 004 |SK|;", &EXPERT_SDR3),
            Ok(Request::Start(Start::CwMacros {
                trx: trx(0),
                text: Text::new(">TU >599 004 |SK|").unwrap()
            }))
        );
        // Text holding an escape character itself cannot be carried.
        for bad in ["a^b", "a~b", "a*b", "tab\there", "\u{e9}"] {
            assert_eq!(Text::new(bad), None, "{bad:?}");
        }
        // Spaces at either end are kept: they are part of a CW macro.
        assert_eq!(
            Start::CwMacros {
                trx: trx(0),
                text: Text::new(" E ").unwrap()
            }
            .render(&EXPERT_SDR3)
            .unwrap()
            .as_str(),
            "cw_macros:0, E ;"
        );
    }

    #[test]
    fn numbers_are_strict_but_spaces_around_them_are_allowed() {
        assert_eq!(
            one_notice("rx_sensors:0, -71.5;"),
            Ok(Notice::RxSensors {
                trx: trx(0),
                dbm: Real(-71_500_000)
            })
        );
        assert_eq!(
            one_notice(" vfo : 0 , 1 , 7100000 ;"),
            Ok(Notice::State(Setting::Vfo {
                trx: trx(0),
                channel: Channel::B,
                hz: Hz(7_100_000)
            }))
        );
        for bad in [
            "+5", "5.", ".5", "1e3", "0x10", "--5", "5 5", "", "-", "1,5",
        ] {
            assert_eq!(uint(bad), None, "{bad:?}");
            assert_eq!(int(bad), None, "{bad:?}");
            assert_eq!(Real::parse(bad), None, "{bad:?}");
        }
        assert_eq!(int("-0"), Some(0));
        assert_eq!(uint("007"), Some(7));
        assert_eq!(uint("4294967296"), None);
    }

    #[test]
    fn a_real_renders_with_a_decimal_point_and_rounds_to_millionths() {
        for (text, rendered) in [
            ("47", "47.0"),
            ("-71.5", "-71.5"),
            ("0", "0.0"),
            ("-0.0", "0.0"),
            ("1.7", "1.7"),
            ("1.0000005", "1.000001"),
            ("1.000000499", "1.0"),
            ("-1.0000005", "-1.000001"),
            ("0.9999995", "1.0"),
        ] {
            let r = Real::parse(text).unwrap_or_else(|| panic!("{text:?}"));
            assert_eq!(r.to_string(), rendered, "{text:?}");
            assert_eq!(Real::parse(&r.to_string()), Some(r), "{text:?}");
        }
        assert_eq!(Real::parse("-71.5").map(Real::to_f64), Some(-71.5));
    }

    #[test]
    fn a_source_the_codec_does_not_know_still_reads_the_key() {
        // A refused `trx` notice would leave the keyed state stale; an unknown source must not.
        assert_eq!(
            one_notice("trx:0,true,line2;"),
            Ok(Notice::Trx {
                trx: trx(0),
                on: true,
                source: Some(Source::Other(Word("line2".into())))
            })
        );
        assert_eq!(
            Source::from_word(Word::new("MicPC").unwrap()),
            Source::MicPc
        );
    }

    #[test]
    fn a_readback_is_read_whatever_its_value_and_an_error_names_its_command() {
        // A drive above 100 is the server's truth: the session must see it to refuse a key.
        assert_eq!(
            one_notice("drive:0,250;"),
            Ok(Notice::State(Setting::Drive {
                trx: Some(trx(0)),
                level: 250
            }))
        );
        // What cannot be read names its command, so the session can forget that value.
        for (text, name) in [
            ("drive:0,abc;", "drive"),
            ("trx:0,maybe;", "trx"),
            ("tx_enable:0;", "tx_enable"),
            ("tx_sensors:0,-27.2,47.4.67.5,1.7;", "tx_sensors"),
        ] {
            assert_eq!(
                one_notice(text).err().as_ref().and_then(WireError::command),
                Some(name),
                "{text:?}"
            );
        }
    }

    // ── Dialects ──

    #[test]
    fn the_drive_form_follows_the_dialect() {
        let d = |dialect: &Dialect| {
            (
                Command::Set(Setting::drive(trx(1), 40, dialect))
                    .render()
                    .into_string(),
                Command::Read(Query::drive(trx(1), dialect))
                    .render()
                    .into_string(),
            )
        };
        assert_eq!(d(&EXPERT_SDR3), ("drive:1,40;".into(), "drive:1;".into()));
        assert_eq!(d(&EXPERT_SDR2), ("drive:40;".into(), "drive;".into()));
        // A server reading `drive:1;` needs its own dialect to know what it is.
        assert_eq!(
            one_request("drive:1;", &EXPERT_SDR3),
            Ok(Request::Command(Command::Read(Query::Drive(Some(trx(1))))))
        );
        assert_eq!(
            one_request("drive:1;", &EXPERT_SDR2),
            Ok(Request::Command(Command::Set(Setting::Drive {
                trx: None,
                level: 1
            })))
        );
        // A notice is never a read: one number is the 1.x drive.
        assert_eq!(
            one_notice("drive:30;"),
            Ok(Notice::State(Setting::Drive {
                trx: None,
                level: 30
            }))
        );
    }

    #[test]
    fn a_dialect_without_a_source_does_not_render_a_sourced_key() {
        let keyed = Start::Trx {
            trx: trx(0),
            source: Some(Source::Tci),
        };
        assert_eq!(
            keyed.render(&EXPERT_SDR3).map(ClientText::into_string),
            Ok("trx:0,true,tci;".into())
        );
        assert_eq!(
            keyed.render(&EXPERT_SDR2),
            Err(Unrenderable::SourceNotInDialect)
        );
        let plain = Start::Trx {
            trx: trx(0),
            source: None,
        };
        for d in DIALECTS {
            assert_eq!(
                plain.render(&d).map(ClientText::into_string),
                Ok("trx:0,true;".into()),
                "{}",
                d.name
            );
        }
    }

    #[test]
    fn a_stop_is_read_whatever_source_it_names() {
        assert_eq!(
            one_request("trx:0,false,tci;", &EXPERT_SDR3),
            Ok(Request::Stop(Stop::Trx(trx(0))))
        );
        assert_eq!(Stop::Trx(trx(0)).render().as_str(), "trx:0,false;");
    }

    // ── Strategies ──

    fn a_trx() -> impl Strategy<Value = Trx> {
        (0..MAX_TRX).prop_map(trx)
    }

    fn a_channel() -> impl Strategy<Value = Channel> {
        (0..MAX_CHANNELS).prop_map(|n| Channel::new(n).unwrap())
    }

    fn a_hz() -> impl Strategy<Value = Hz> {
        (-MAX_HZ..=MAX_HZ).prop_map(Hz)
    }

    fn a_real() -> impl Strategy<Value = Real> {
        let max = MAX_REAL * 1_000_000;
        prop_oneof![
            (-max..=max).prop_map(Real),
            (-200_000i64..=200_000).prop_map(|tenths| Real(tenths * 100_000)),
        ]
    }

    fn a_word() -> impl Strategy<Value = Word> {
        "[a-zA-Z0-9_+-]{1,16}".prop_map(|s| Word::new(&s).unwrap())
    }

    /// Printable ASCII without the reserved (`:,;`) or escape (`^~*`) characters.
    const TOKEN_CHAR: &str = "[!-)+\\--9<-\\]_-}]";
    /// The same, or a space.
    const TOKEN_CHAR_OR_SPACE: &str = "[ !-)+\\--9<-\\]_-}]";

    fn regex(pattern: &str) -> impl Strategy<Value = String> {
        proptest::string::string_regex(pattern).expect("a valid pattern")
    }

    fn a_token() -> impl Strategy<Value = Token> {
        prop_oneof![
            regex(&format!("{TOKEN_CHAR}{{1,64}}")),
            regex(&format!(
                "{TOKEN_CHAR}{TOKEN_CHAR_OR_SPACE}{{0,20}}{TOKEN_CHAR}"
            )),
        ]
        .prop_map(|s| Token::new(&s).unwrap())
    }

    fn a_call() -> impl Strategy<Value = Call> {
        prop_oneof![
            regex("[A-Za-z0-9/]{1,12}"),
            regex(&format!("{TOKEN_CHAR}{{1,64}}")),
        ]
        .prop_map(|s| Call::new(&s).unwrap())
    }

    fn a_text() -> impl Strategy<Value = Text> {
        prop_oneof![
            "[ -)+-\\]_-}]{0,40}",
            "[A-Z0-9 ,:;<>|$]{0,40}",
            "[ -)+-\\]_-}]{1000,1024}",
        ]
        .prop_map(|s| Text::new(&s).unwrap())
    }

    fn a_colour() -> impl Strategy<Value = SpotColour> {
        prop_oneof![
            any::<u32>().prop_map(SpotColour::Decimal),
            any::<u32>().prop_map(SpotColour::Hash),
        ]
    }

    fn a_source() -> impl Strategy<Value = Source> {
        prop_oneof![
            Just(Source::Tci),
            Just(Source::Mic1),
            Just(Source::Mic2),
            Just(Source::MicPc),
            Just(Source::Ecoder2),
            a_word().prop_map(Source::from_word),
        ]
    }

    fn a_flag() -> impl Strategy<Value = Flag> {
        prop::sample::select(Flag::ALL.to_vec())
    }

    fn an_agc() -> impl Strategy<Value = AgcMode> {
        prop::sample::select(AgcMode::ALL.to_vec())
    }

    fn a_setting() -> impl Strategy<Value = Setting> {
        prop_oneof![
            (a_trx(), a_hz()).prop_map(|(trx, hz)| Setting::Dds { trx, hz }),
            (a_trx(), a_channel(), a_hz()).prop_map(|(trx, channel, hz)| Setting::If {
                trx,
                channel,
                hz
            }),
            (a_trx(), a_channel(), a_hz()).prop_map(|(trx, channel, hz)| Setting::Vfo {
                trx,
                channel,
                hz
            }),
            (a_trx(), prop::option::of(a_channel()), a_word())
                .prop_map(|(trx, channel, mode)| Setting::Modulation { trx, channel, mode }),
            (prop::option::of(a_trx()), any::<u32>())
                .prop_map(|(trx, level)| Setting::Drive { trx, level }),
            (a_trx(), any::<u32>()).prop_map(|(trx, level)| Setting::TuneDrive { trx, level }),
            (a_flag(), a_trx(), any::<bool>()).prop_map(|(flag, trx, on)| Setting::Flag {
                flag,
                trx,
                on
            }),
            (a_trx(), a_hz()).prop_map(|(trx, hz)| Setting::RitOffset { trx, hz }),
            (a_trx(), a_hz()).prop_map(|(trx, hz)| Setting::XitOffset { trx, hz }),
            (a_trx(), a_channel(), any::<bool>())
                .prop_map(|(trx, channel, on)| { Setting::RxChannelEnable { trx, channel, on } }),
            (a_trx(), a_hz(), a_hz()).prop_map(|(trx, low, high)| Setting::RxFilterBand {
                trx,
                low,
                high
            }),
            any::<u32>().prop_map(Setting::CwMacrosSpeed),
            any::<u32>().prop_map(Setting::CwMacrosDelay),
            any::<u32>().prop_map(Setting::CwKeyerSpeed),
            any::<i32>().prop_map(Setting::Volume),
            any::<bool>().prop_map(Setting::Mute),
            (a_trx(), a_channel(), any::<i32>()).prop_map(|(trx, channel, db)| Setting::RxVolume {
                trx,
                channel,
                db
            }),
            (a_trx(), a_channel(), any::<i32>())
                .prop_map(|(trx, channel, db)| Setting::RxBalance { trx, channel, db }),
            any::<i32>().prop_map(Setting::MonVolume),
            any::<bool>().prop_map(Setting::MonEnable),
            (a_trx(), an_agc()).prop_map(|(trx, mode)| Setting::AgcMode { trx, mode }),
            (a_trx(), any::<i32>()).prop_map(|(trx, db)| Setting::AgcGain { trx, db }),
            (a_trx(), any::<u32>(), any::<u32>()).prop_map(|(trx, threshold, duration)| {
                Setting::NbParam {
                    trx,
                    threshold,
                    duration,
                }
            }),
            (a_trx(), any::<i32>()).prop_map(|(trx, db)| Setting::SqlLevel { trx, db }),
            any::<u32>().prop_map(Setting::DiglOffset),
            any::<u32>().prop_map(Setting::DiguOffset),
        ]
    }

    fn a_query() -> impl Strategy<Value = Query> {
        prop_oneof![
            a_trx().prop_map(Query::Dds),
            (a_trx(), a_channel()).prop_map(|(trx, channel)| Query::If { trx, channel }),
            (a_trx(), a_channel()).prop_map(|(trx, channel)| Query::Vfo { trx, channel }),
            a_trx().prop_map(Query::Modulation),
            a_trx().prop_map(Query::Trx),
            a_trx().prop_map(Query::Tune),
            prop::option::of(a_trx()).prop_map(Query::Drive),
            a_trx().prop_map(Query::TuneDrive),
            (a_flag(), a_trx()).prop_map(|(flag, trx)| Query::Flag { flag, trx }),
            a_trx().prop_map(Query::RitOffset),
            a_trx().prop_map(Query::XitOffset),
            (a_trx(), a_channel())
                .prop_map(|(trx, channel)| Query::RxChannelEnable { trx, channel }),
            a_trx().prop_map(Query::RxFilterBand),
            Just(Query::CwMacrosSpeed),
            Just(Query::CwMacrosDelay),
            Just(Query::Volume),
            Just(Query::Mute),
            (a_trx(), a_channel()).prop_map(|(trx, channel)| Query::RxVolume { trx, channel }),
            (a_trx(), a_channel()).prop_map(|(trx, channel)| Query::RxBalance { trx, channel }),
            Just(Query::MonVolume),
            Just(Query::MonEnable),
            a_trx().prop_map(Query::AgcMode),
            a_trx().prop_map(Query::AgcGain),
            a_trx().prop_map(Query::NbParam),
            a_trx().prop_map(Query::SqlLevel),
            Just(Query::DiglOffset),
            Just(Query::DiguOffset),
            (a_trx(), a_channel()).prop_map(|(trx, channel)| Query::VfoLock { trx, channel }),
            (a_trx(), a_channel()).prop_map(|(trx, channel)| Query::RxSmeter { trx, channel }),
        ]
    }

    fn a_stream() -> impl Strategy<Value = StreamSetting> {
        prop_oneof![
            a_trx().prop_map(StreamSetting::AudioStart),
            a_trx().prop_map(StreamSetting::AudioStop),
            a_trx().prop_map(StreamSetting::IqStart),
            a_trx().prop_map(StreamSetting::IqStop),
            a_trx().prop_map(StreamSetting::LineOutStart),
            a_trx().prop_map(StreamSetting::LineOutStop),
            prop::sample::select(AudioRate::HZ.to_vec())
                .prop_map(|hz| StreamSetting::AudioSampleRate(AudioRate::new(hz).unwrap())),
            prop::sample::select(IqRate::HZ.to_vec())
                .prop_map(|hz| StreamSetting::IqSampleRate(IqRate::new(hz).unwrap())),
            prop::sample::select(SampleFormat::ALL.to_vec())
                .prop_map(StreamSetting::AudioSampleType),
            prop::sample::select(vec![Channels::One, Channels::Two])
                .prop_map(StreamSetting::AudioChannels),
            any::<u32>().prop_map(StreamSetting::AudioSamples),
            any::<u32>().prop_map(StreamSetting::TxAudioBuffering),
        ]
    }

    fn a_notice() -> impl Strategy<Value = Notice> {
        prop_oneof![
            (a_hz(), a_hz()).prop_map(|(low, high)| Notice::VfoLimits { low, high }),
            (a_hz(), a_hz()).prop_map(|(low, high)| Notice::IfLimits { low, high }),
            any::<u32>().prop_map(Notice::TrxCount),
            any::<u32>().prop_map(Notice::ChannelCount),
            a_token().prop_map(Notice::Device),
            any::<bool>().prop_map(Notice::ReceiveOnly),
            prop::collection::vec(a_word(), 1..=MAX_ARGS)
                .prop_map(|m| Notice::ModulationsList(Modes::new(m).unwrap())),
            (a_token(), a_token())
                .prop_map(|(program, version)| Notice::Protocol { program, version }),
            Just(Notice::Ready),
            Just(Notice::RadioStart),
            Just(Notice::RadioStop),
            a_setting().prop_map(Notice::State),
            (a_trx(), any::<bool>(), prop::option::of(a_source()))
                .prop_map(|(trx, on, source)| Notice::Trx { trx, on, source }),
            (a_trx(), any::<bool>()).prop_map(|(trx, on)| Notice::Tune { trx, on }),
            (a_trx(), any::<bool>()).prop_map(|(trx, on)| Notice::TxEnable { trx, on }),
            a_hz().prop_map(Notice::TxFrequency),
            (a_trx(), a_channel(), any::<bool>()).prop_map(|(trx, channel, on)| Notice::VfoLock {
                trx,
                channel,
                on
            }),
            a_stream().prop_map(Notice::Stream),
            (a_call(), a_hz()).prop_map(|(call, hz)| Notice::ClickedOnSpot { call, hz }),
            (a_trx(), a_channel(), a_call(), a_hz()).prop_map(|(trx, channel, call, hz)| {
                Notice::RxClickedOnSpot {
                    trx,
                    channel,
                    call,
                    hz,
                }
            }),
            (a_trx(), any::<bool>())
                .prop_map(|(trx, pressed)| Notice::TxFootswitch { trx, pressed }),
            any::<bool>().prop_map(Notice::AppFocus),
            (a_trx(), a_real()).prop_map(|(trx, dbm)| Notice::RxSensors { trx, dbm }),
            (a_trx(), a_channel(), a_real())
                .prop_map(|(trx, channel, dbm)| Notice::RxChannelSensors { trx, channel, dbm }),
            (a_trx(), a_real(), a_real(), a_real(), a_real()).prop_map(
                |(trx, mic_dbm, rms_w, peak_w, swr)| Notice::TxSensors {
                    trx,
                    mic_dbm,
                    rms_w,
                    peak_w,
                    swr
                }
            ),
            (a_trx(), a_channel(), a_real()).prop_map(|(trx, channel, dbm)| Notice::RxSmeter {
                trx,
                channel,
                dbm
            }),
            a_real().prop_map(Notice::TxPower),
            a_real().prop_map(Notice::TxSwr),
            Just(Notice::CwMacrosEmpty),
            a_call().prop_map(Notice::CallsignSend),
        ]
    }

    fn a_command() -> impl Strategy<Value = Command> {
        prop_oneof![
            Just(Command::RadioStart),
            Just(Command::RadioStop),
            a_setting().prop_map(Command::Set),
            a_query().prop_map(Command::Read),
            a_stream().prop_map(Command::Stream),
            (a_call(), a_token(), a_hz(), a_colour(), a_text()).prop_map(
                |(call, mode, hz, colour, text)| Command::Spot {
                    call,
                    mode,
                    hz,
                    colour,
                    text
                }
            ),
            (a_call(), prop::option::of(a_hz()))
                .prop_map(|(call, hz)| Command::SpotDelete { call, hz }),
            Just(Command::SpotClear),
            (any::<bool>(), prop::option::of(any::<u32>()))
                .prop_map(|(on, interval_ms)| Command::RxSensorsEnable { on, interval_ms }),
            (any::<bool>(), prop::option::of(any::<u32>()))
                .prop_map(|(on, interval_ms)| Command::TxSensorsEnable { on, interval_ms }),
            any::<u32>().prop_map(Command::CwMacrosSpeedUp),
            any::<u32>().prop_map(Command::CwMacrosSpeedDown),
            Just(Command::SetInFocus),
        ]
    }

    fn a_stop() -> impl Strategy<Value = Stop> {
        prop_oneof![
            a_trx().prop_map(Stop::Trx),
            a_trx().prop_map(Stop::Tune),
            Just(Stop::CwMacros),
            Just(Stop::CwTerminal),
            (a_trx(), prop::option::of(any::<u32>())).prop_map(|(trx, ms)| Stop::KeyUp { trx, ms }),
        ]
    }

    fn a_start() -> impl Strategy<Value = Start> {
        prop_oneof![
            (a_trx(), prop::option::of(a_source()))
                .prop_map(|(trx, source)| Start::Trx { trx, source }),
            a_trx().prop_map(Start::Tune),
            (a_trx(), a_text()).prop_map(|(trx, text)| Start::CwMacros { trx, text }),
            (a_trx(), a_text(), a_text(), a_text()).prop_map(|(trx, prefix, call, suffix)| {
                Start::CwMsg {
                    trx,
                    prefix,
                    call,
                    suffix,
                }
            }),
            a_text().prop_map(Start::CwMsgCall),
            Just(Start::CwTerminal),
            (a_trx(), prop::option::of(any::<u32>()))
                .prop_map(|(trx, ms)| Start::KeyDown { trx, ms }),
        ]
    }

    fn a_dialect() -> impl Strategy<Value = Dialect> {
        prop::sample::select(DIALECTS.to_vec())
    }

    /// A drive in the form `dialect` reads: the one place a request's text depends on it.
    fn in_dialect(c: Command, dialect: &Dialect) -> Command {
        match c {
            Command::Set(Setting::Drive { trx, level }) => {
                Command::Set(Setting::drive(trx.unwrap_or(Trx(0)), level, dialect))
            }
            Command::Read(Query::Drive(trx)) => {
                Command::Read(Query::drive(trx.unwrap_or(Trx(0)), dialect))
            }
            c => c,
        }
    }

    /// What every value the decoders hand out must satisfy, whatever the input.
    fn within_bounds(n: &Notice) -> bool {
        match n {
            Notice::ModulationsList(m) => (1..=MAX_ARGS).contains(&m.as_slice().len()),
            Notice::Device(t) => t.as_str().len() <= MAX_TOKEN,
            Notice::Protocol { program, version } => {
                program.as_str().len() <= MAX_TOKEN && version.as_str().len() <= MAX_TOKEN
            }
            _ => true,
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]

        /// Every notice renders to text that reads back as itself.
        #[test]
        fn every_notice_round_trips(n in a_notice()) {
            let line = n.render().into_string();
            prop_assert_eq!(one_notice(&line), Ok(n));
        }

        /// Every command, stop and start reads back as itself in the dialect it was made for.
        #[test]
        fn every_request_round_trips(
            dialect in a_dialect(),
            c in a_command(),
            s in a_stop(),
            k in a_start(),
        ) {
            let c = in_dialect(c, &dialect);
            prop_assert_eq!(
                one_request(c.render().as_str(), &dialect),
                Ok(Request::Command(c))
            );
            prop_assert_eq!(one_request(s.render().as_str(), &dialect), Ok(Request::Stop(s)));
            match k.render(&dialect) {
                Ok(line) => {
                    prop_assert_eq!(one_request(line.as_str(), &dialect), Ok(Request::Start(k)));
                }
                Err(Unrenderable::SourceNotInDialect) => {
                    prop_assert!(!dialect.trx_source);
                    prop_assert!(
                        matches!(k, Start::Trx { source: Some(_), .. }),
                        "only a sourced key is refused"
                    );
                }
            }
        }

        /// THE TRANSMIT PARTITION AT THE TEXT LEVEL: nothing a `Command` or a `Stop` renders is
        /// read by a server, in any dialect, as a start. Only a `Start` renders a key.
        #[test]
        fn no_command_or_stop_renders_a_key(c in a_command(), s in a_stop()) {
            for d in DIALECTS {
                for line in [c.render().into_string(), s.render().into_string()] {
                    let read: Vec<_> = requests(&line, &d).collect();
                    prop_assert_eq!(read.len(), 1);
                    prop_assert!(
                        !matches!(read[0], Ok(Request::Start(_))),
                        "{:?} reads as a start in {}", line, d.name
                    );
                }
            }
        }

        /// And every start renders a key.
        #[test]
        fn every_start_renders_a_key(k in a_start()) {
            for d in DIALECTS {
                if let Ok(line) = k.render(&d) {
                    prop_assert!(matches!(
                        one_request(line.as_str(), &d),
                        Ok(Request::Start(_))
                    ));
                }
            }
        }
    }

    /// Text a fuzzer is more likely to make protocol-shaped than arbitrary unicode.
    fn protocol_ish() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop_oneof![
                Just(":".to_string()),
                Just(",".to_string()),
                Just(";".to_string()),
                Just(" ".to_string()),
                Just("-".to_string()),
                Just(".".to_string()),
                "[0-9]{1,12}",
                prop::sample::select(
                    CLIENT_ONLY
                        .iter()
                        .chain(SERVER_ONLY.iter())
                        .copied()
                        .chain(["trx", "tune", "drive", "vfo", "modulation", "audio_start"])
                        .map(String::from)
                        .collect::<Vec<_>>()
                ),
                "(true|false|TRUE|tci|usb)",
                "[\\x00-\\x7f]{1,3}",
                any::<char>().prop_map(String::from),
            ],
            0..40,
        )
        .prop_map(|parts| parts.concat())
    }

    /// One spec example, edited: a character dropped, doubled, swapped or inserted.
    fn edited_example() -> impl Strategy<Value = String> {
        let texts: Vec<String> = corpus().iter().map(|e| e.text.to_string()).collect();
        (
            prop::sample::select(texts),
            prop::collection::vec((any::<prop::sample::Index>(), 0u8..4, any::<char>()), 1..4),
        )
            .prop_map(|(text, edits)| {
                let mut chars: Vec<char> = text.chars().collect();
                for (at, op, c) in edits {
                    if chars.is_empty() {
                        chars.push(c);
                        continue;
                    }
                    let i = at.index(chars.len());
                    match op {
                        0 => {
                            chars.remove(i);
                        }
                        1 => {
                            let twin = chars[i];
                            chars.insert(i, twin);
                        }
                        2 => chars[i] = c,
                        _ => chars.insert(i, c),
                    }
                }
                chars.into_iter().collect()
            })
    }

    /// Read every way there is, and check what comes out.
    fn read_all(text: &str) -> Result<(), TestCaseError> {
        for raw in split(text).flatten() {
            prop_assert!((1..=MAX_NAME).contains(&raw.name().len()));
            prop_assert!(raw.args().len() <= MAX_ARGS);
            prop_assert!(raw.args().iter().all(|a| a.len() <= MAX_ARG));
            let length = raw.name().len() + raw.args().iter().map(|a| a.len() + 1).sum::<usize>();
            prop_assert!(length <= MAX_COMMAND);
        }
        let read: Vec<_> = notices(text).collect();
        prop_assert!(read.len() <= MAX_COMMANDS + 1);
        for n in read.iter().flatten() {
            prop_assert!(within_bounds(n), "{:?}", n);
        }
        for d in DIALECTS {
            prop_assert!(requests(text, &d).count() <= MAX_COMMANDS + 1);
        }
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 4096, ..ProptestConfig::default() })]

        /// The fuzz: any string. Never a panic, never more than the bounds allow.
        #[test]
        fn any_text_never_panics(text in any::<String>()) {
            read_all(&text)?;
        }

        /// Any bytes a server could put in a text frame (tungstenite hands over valid UTF-8; the
        /// lossy form stands in for what a broken server's bytes become).
        #[test]
        fn any_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..400)) {
            read_all(&String::from_utf8_lossy(&bytes))?;
        }

        /// The fuzz, aimed at the grammar.
        #[test]
        fn protocol_shaped_text_never_panics(text in protocol_ish()) {
            read_all(&text)?;
        }

        /// The spec's examples, edited.
        #[test]
        fn edited_spec_examples_never_panic(text in edited_example()) {
            read_all(&text)?;
        }
    }
}

/// The server's reading ([`inbound`], [`Extra`], [`ExtraNotice`]).
#[cfg(test)]
mod server_tests {
    use super::*;
    use crate::tci::EXPERT_SDR3;
    use proptest::prelude::*;

    fn trx(n: u32) -> Trx {
        Trx::new(n).expect("a receiver number below MAX_TRX")
    }

    fn one(text: &str) -> Result<Inbound, WireError> {
        let mut all: Vec<_> = inbound(text, &EXPERT_SDR3).collect();
        assert_eq!(all.len(), 1, "{text:?} holds one command: {all:?}");
        all.remove(0)
    }

    #[test]
    fn wsjtx_split_without_a_receiver_reads_as_an_extra() {
        assert_eq!(
            one("split_enable:false;"),
            Ok(Inbound::Extra(Extra::Split(false)))
        );
        assert_eq!(
            one("SPLIT_ENABLE:TRUE;"),
            Ok(Inbound::Extra(Extra::Split(true)))
        );
        // The forms with a receiver are the spec's read and write, untouched.
        assert_eq!(
            one("split_enable:0;"),
            Ok(Inbound::Request(Request::Command(Command::Read(
                Query::Flag {
                    flag: Flag::Split,
                    trx: trx(0),
                }
            ))))
        );
        assert_eq!(
            one("split_enable:0,false;"),
            Ok(Inbound::Request(Request::Command(Command::Set(
                Setting::Flag {
                    flag: Flag::Split,
                    trx: trx(0),
                    on: false,
                }
            ))))
        );
        // A word that is neither a receiver nor a boolean is still the read's bad argument.
        assert_eq!(
            one("split_enable:maybe;").map_err(|e| e.fault()),
            Err(Some(Fault::BadArg(0)))
        );
    }

    #[test]
    fn mshv_rx_enable_and_tx_enable_read_as_extras() {
        assert_eq!(
            one("rx_enable:0,true;"),
            Ok(Inbound::Extra(Extra::RxEnable {
                trx: trx(0),
                on: true
            }))
        );
        assert_eq!(
            one("RX_ENABLE:1;"),
            Ok(Inbound::Extra(Extra::RxEnableRead(trx(1))))
        );
        assert_eq!(
            one("tx_enable:0,true;"),
            Ok(Inbound::Extra(Extra::TxEnable {
                trx: trx(0),
                on: true
            }))
        );
        // Malformed, each refused by name rather than guessed at.
        for (text, fault) in [
            ("rx_enable;", Fault::Arity(0)),
            ("rx_enable:0,1,true;", Fault::Arity(3)),
            ("rx_enable:0,yes;", Fault::BadArg(1)),
            ("rx_enable:99,true;", Fault::BadArg(0)),
            ("tx_enable:0,maybe;", Fault::BadArg(1)),
            // A read of `tx_enable` is still nobody's: the spec has no client read of it.
            ("tx_enable:0;", Fault::Direction),
        ] {
            let got = one(text);
            assert_eq!(
                got.as_ref().err().and_then(WireError::fault),
                Some(fault),
                "{text}: {got:?}"
            );
        }
    }

    #[test]
    fn the_recorder_commands_are_read_only_to_be_refused() {
        // The spec's own examples (p33-34), and the forms a hostile client might try.
        for text in [
            "LINE_OUT_RECORDER_START:0,250;",
            "LINE_OUT_RECORDER_SAVE:0,home/user_name/record_dir/file_name.wav;",
            "LINE_OUT_RECORDER_SAVE:0,D|\\record_dir\\file_name.mp3;",
            "LINE_OUT_RECORDER_BREAK:0;",
            "line_out_recorder_save;",
            "line_out_recorder_save:0,..,..,..,etc,passwd;",
        ] {
            assert_eq!(
                one(text),
                Ok(Inbound::Extra(Extra::LineOutRecorder)),
                "{text}"
            );
        }
        // The client's reading is unchanged: as a request they stay unknown.
        assert_eq!(
            requests("line_out_recorder_break:0;", &EXPERT_SDR3)
                .next()
                .and_then(|r| r.err())
                .and_then(|e| e.fault()),
            Some(Fault::Unknown)
        );
    }

    #[test]
    fn extra_notices_render_as_a_server_sends_them() {
        assert_eq!(
            ExtraNotice::RxEnable {
                trx: trx(0),
                on: true
            }
            .render()
            .as_str(),
            "rx_enable:0,true;"
        );
        assert_eq!(
            ExtraNotice::CwTerminal(false).render().as_str(),
            "cw_terminal:false;"
        );
        // And they read back as the extras a client would write.
        assert_eq!(
            one("rx_enable:0,true;"),
            Ok(Inbound::Extra(Extra::RxEnable {
                trx: trx(0),
                on: true
            }))
        );
        assert_eq!(
            one("cw_terminal:false;"),
            Ok(Inbound::Request(Request::Stop(Stop::CwTerminal)))
        );
    }

    /// Every name a client or server sends, the extras' among them, for the property below.
    fn a_name() -> impl Strategy<Value = String> {
        let mut names: Vec<&'static str> = CLIENT_ONLY
            .iter()
            .chain(SERVER_ONLY.iter())
            .copied()
            .chain(Flag::ALL.iter().map(|f| f.name()))
            .chain([
                "start",
                "stop",
                "dds",
                "if",
                "vfo",
                "modulation",
                "trx",
                "tune",
                "drive",
                "tune_drive",
                "rit_offset",
                "xit_offset",
                "audio_start",
                "audio_samplerate",
                "audio_stream_samples",
                "rx_enable",
                "line_out_recorder_start",
                "line_out_recorder_save",
                "line_out_recorder_break",
                "no_such_command",
            ])
            .collect();
        names.sort_unstable();
        names.dedup();
        prop::sample::select(names).prop_map(str::to_string)
    }

    fn an_arg() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("0".to_string()),
            Just("1".to_string()),
            Just("true".to_string()),
            Just("FALSE".to_string()),
            Just("tci".to_string()),
            Just("14074000".to_string()),
            Just("-1".to_string()),
            Just(String::new()),
            "[a-z0-9 .^~*/-]{0,6}",
        ]
    }

    fn a_command() -> impl Strategy<Value = String> {
        (
            a_name(),
            prop::option::of(prop::collection::vec(an_arg(), 0..5)),
        )
            .prop_map(|(name, args)| match args {
                None => format!("{name};"),
                Some(args) => format!("{name}:{};", args.join(",")),
            })
    }

    const EXTRA_NAMES: [&str; 6] = [
        "split_enable",
        "rx_enable",
        "tx_enable",
        "line_out_recorder_start",
        "line_out_recorder_save",
        "line_out_recorder_break",
    ];

    proptest! {
        #![proptest_config(ProptestConfig { cases: 2048, ..ProptestConfig::default() })]

        /// The server's reading adds forms; it changes nothing the client's reading reads. A
        /// command whose name is not an extra's reads exactly as [`requests`] reads it, and one
        /// whose name is either reads as an [`Extra`] or as [`requests`] reads it.
        #[test]
        fn inbound_reads_every_other_command_as_requests_does(text in a_command()) {
            let ours: Vec<_> = inbound(&text, &EXPERT_SDR3).collect();
            let theirs: Vec<_> = requests(&text, &EXPERT_SDR3).collect();
            prop_assert_eq!(ours.len(), theirs.len());
            let name = text.split([':', ';']).next().unwrap_or("").to_ascii_lowercase();
            for (o, t) in ours.into_iter().zip(theirs) {
                match o {
                    Ok(Inbound::Extra(_)) => prop_assert!(
                        EXTRA_NAMES.contains(&name.as_str()),
                        "{} read as an extra", text
                    ),
                    Ok(Inbound::Request(r)) => prop_assert_eq!(Ok(r), t),
                    Err(e) => {
                        if !EXTRA_NAMES.contains(&name.as_str()) {
                            prop_assert_eq!(Err(e), t);
                        }
                    }
                }
            }
        }

        /// The fuzz: any string, never a panic.
        #[test]
        fn inbound_never_panics(text in any::<String>()) {
            for item in inbound(&text, &EXPERT_SDR3) {
                let _ = item;
            }
        }
    }
}
