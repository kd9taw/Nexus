//! Amplifier link — the wire formats for SPE Expert and Elecraft KPA, and nothing else.
//!
//! Pure codec: builds request bytes, parses reply bytes, and owns no port, no thread and no
//! state. The I/O half lives with the poller, exactly as the rotator splits its framing from its
//! transport, so every rule below is testable without an amplifier on the bench.
//!
//! # Provenance — the vendor's own document
//!
//! The SPE half is written from **"EXPERT 1.3K-FA / EXPERT 2K-FA Application Programmer's Guide,
//! Rev. 1.1" (S.P.E. s.r.l., 15.10.2015)** — the manufacturer's published protocol specification,
//! section references below are to it. No Hamlib code, expression or table is involved: this is
//! the "documented protocol" arm of the OSS-integration checklist in its cleanest form, a vendor
//! spec implemented from scratch.
//!
//! Reading the real document corrected two things an earlier reading of Hamlib's source had got
//! wrong here, and both would have broken the link on the amplifier's FIRST reply:
//!
//! - **Replies open with `0xAA`, not `0x55`** (§3). `0x55` is the host→amplifier sync only, and
//!   matching the request preamble against a reply rejects every frame the amplifier sends.
//!   Hamlib is where that error came from: `expert.c:159` says *"read the 4-byte header
//!   x55x55x55xXX"* and then never checks the bytes, so it mislabels rather than rejects — and
//!   a reader who takes the comment at its word inherits a bug Hamlib itself does not hit.
//! - **`CNT` counts the DATA bytes, checksum excluded** (§3) — it is not a whole-frame length.
//!   The spec's own worked ACK is `AA AA AA 01 0D 0D`: four header bytes, then `CNT` data and
//!   one checksum byte. Both prior readings break on real frames. `CNT - 4`, written here,
//!   underflows on that ACK. Hamlib's `bytes - 3` (`expert.c:174`) under-reads a status reply by
//!   seven — 67 data + 2 checksum + CRLF is 71 bytes after the header, and it takes 64 — leaving
//!   the tail to be read as the head of the next frame; on the 1-byte ACK it asks for a length
//!   of −2.
//!
//! # Serial settings — there is nothing for the operator to set
//!
//! 8 data bits, 1 stop bit, no parity (§1). The maximum speed is 115200 and **the amplifier
//! adapts itself to lower speeds automatically**, so no baud field belongs in Settings. The USB
//! and RS-232 ports work independently but never simultaneously (RS-232 is on 2K-FA s/n > …102).
//!
//! # Hamlib's SPE backend, and why it is not going in front of a 2 kW amplifier
//!
//! Hamlib has three amplifier backends in total (`amplifiers/{elecraft,expert,gemini}`), and
//! Gemini is Ethernet-only — from a serial-port picker that is ONE usable amplifier, for the
//! price of a third daemon and a third port to manage. Its SPE backend calls itself "Initial
//! prototype" and carries this:
//!
//! ```text
//! case RIG_POWER_OFF:     cmd[0] = 0x0a;
//! case RIG_POWER_STANDBY: cmd[0] = 0x0a;   // the identical byte
//! ```
//!
//! The command set (§4) settles what that byte is: **`0x0A` is SWITCH OFF.** There is no standby
//! keycode at all — standby is reached with `0x0D`, the OPERATE key, which toggles. So Hamlib's
//! "standby" does not merely collide with off, it *is* off; and `expert_reset()` calls
//! `set_powerstat(STANDBY)`, so a Hamlib reset switches the amplifier off mid-session.
//!
//! # v1 is READ-ONLY, and that is a safety decision
//!
//! SPE's whole command set is front-panel KEYSTROKES (§4: INPUT, BAND ±, ANTENNA, L ±, C ±,
//! TUNE, SWITCH OFF, POWER, DISPLAY, OPERATE, CAT, arrows, SET) — relative steps and toggles,
//! no absolute setters. Every write's meaning depends on a state we learn a poll late, exactly
//! as the OPERATE toggle above shows. Only `STATUS` (0x90) is ever sent from here.
//!
//! And to say it once: putting an amplifier in standby is NOT a way to stop a transmission — the
//! exciter keeps keying and the drive passes straight through — so nothing here may ever appear
//! in a cockpit's stop-line census.

/// Which amplifier family a link speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmpModel {
    /// SPE Expert 1.3K-FA / 1.5K-FA / 2K-FA — one binary protocol, three amplifiers.
    ///
    /// The 1.5K-FA is CONFIRMED ON HARDWARE (2026-08-29): it identifies itself as `15K` and
    /// answers the documented status request in this protocol. SPE's own programming guide
    /// never names it — "1.5" appears zero times in it — so its support was a hypothesis until
    /// an operator's amplifier answered. The model id is still kept raw rather than matched
    /// against a list, so an amplifier nobody here has seen reports itself rather than being
    /// rejected.
    SpeExpert,
    /// Elecraft KPA500 / KPA1500 — line-oriented ASCII, from Elecraft's own published
    /// programmer's references (KPA500 Rev. A2; KPA1500 Rev. 2.03).
    ///
    /// The KPA500's 22-verb set is a **strict subset** of the KPA1500's 80-odd — checked verb
    /// by verb, not assumed — so a poller built on KPA500 verbs drives both amplifiers from one
    /// code path, and Hamlib registers neither of them under this name.
    ElecraftKpa,
}

// ─────────────────────────── SPE Expert ───────────────────────────

/// Sync byte opening a host→amplifier frame, three times (§3).
const SPE_HOST_SYNC: u8 = 0x55;
/// Sync byte opening an amplifier→host frame, three times (§3). **Not the same byte.**
const SPE_AMP_SYNC: u8 = 0xAA;
/// The one command this module sends: request the status string (§4, §5).
const SPE_CMD_STATUS: u8 = 0x90;
/// Every SPE frame opens with three sync bytes and a count byte.
const SPE_HEADER_LEN: usize = 4;
/// A status reply's trailer after its data: two checksum bytes, a comma, then CR and LF (§5).
///
/// ⭐ FIVE, NOT FOUR, AND THE DOCUMENT CONTRADICTS ITSELF ABOUT IT. §5's field table says the
/// string is "terminated with bytes CR (13) and LF (10)" — four trailer bytes. §5's own worked
/// example prints `…,N,N,%^,CRLF`, with a comma between the checksum pair and the terminator —
/// five. A real EXPERT 1.5K-FA settles it: it sends the comma. Reading the table rather than
/// the example cost us a link that could never parse a single frame, on ANY model, because the
/// length check rejected the frame before anything else ran — and it left one byte unread in
/// the port buffer, desyncing the next poll too.
const SPE_STATUS_TRAILER_LEN: usize = 5;
/// The status string is 19 comma-separated fields (§5).
const SPE_STATUS_FIELDS: usize = 19;

/// Build an SPE request frame: `55 55 55 <cnt> <cmd…> <checksum>`.
///
/// `cnt` is the number of command bytes, checksum excluded, and the checksum is the mod-256 sum
/// of those bytes alone — not the preamble and not the count (§3). Returns `None` for an empty
/// command or one longer than the count byte can express, rather than emitting a frame the
/// amplifier would have to reject.
pub fn spe_request(cmd: &[u8]) -> Option<Vec<u8>> {
    if cmd.is_empty() || cmd.len() > u8::MAX as usize {
        return None;
    }
    let mut f = Vec::with_capacity(cmd.len() + SPE_HEADER_LEN + 1);
    f.extend_from_slice(&[SPE_HOST_SYNC; 3]);
    f.push(cmd.len() as u8);
    f.extend_from_slice(cmd);
    // Wrapping sum: the checksum is a byte, and a multi-byte command overflows it by design.
    f.push(cmd.iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    Some(f)
}

/// The status request, whole: `55 55 55 01 90 90` (§5).
pub fn spe_status_request() -> Vec<u8> {
    spe_request(&[SPE_CMD_STATUS]).expect("a one-byte command always frames")
}

/// The ONLY commands Nexus will ever send an SPE amplifier beyond asking for status.
///
/// ⛔ THE DANGEROUS BYTE IS UNREPRESENTABLE, NOT MERELY UNUSED. §4's keystroke table puts
/// `SWITCH OFF` at `0x0A`, immediately after `TUNE` at `0x09`, so an off-by-one anywhere in a
/// command table turns a tune-up into powering the operator's amplifier off mid-session. That
/// is not hypothetical: Hamlib maps its own "standby" to `0x0A` and switches SPE amplifiers
/// OFF when an operator asks for standby. There is no variant here that can produce `0x0A`,
/// `0x09`, or any of the menu/antenna/L/C keys — so no future edit, no arithmetic slip and no
/// mis-mapping can reach them. Adding one means adding a variant, in this file, on purpose.
///
/// ⚠️ AND NONE OF THESE IS A STOP. Putting an amplifier in standby does not end a
/// transmission — the exciter keeps keying and the drive passes straight through — so no
/// control built on this enum may ever appear in a cockpit's stop-line census.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeCommand {
    /// `0x02` — step DOWN one band. Sequential: there is no "set band" in this protocol.
    BandDown,
    /// `0x03` — step UP one band.
    BandUp,
    /// `0x0D` — OPERATE. ⭐ A TOGGLE, NOT A SET. The protocol offers no idempotent "go to
    /// operate": this flips whatever state the amplifier is in. So a lost or duplicated frame
    /// inverts it silently, and any control built on it must drive its own appearance from the
    /// STATUS string's Standby/Operate field and reconcile against it — never from what it
    /// believes it just sent.
    Operate,
}

impl SpeCommand {
    /// The single keystroke byte this command is (§4).
    fn code(self) -> u8 {
        match self {
            SpeCommand::BandDown => 0x02,
            SpeCommand::BandUp => 0x03,
            SpeCommand::Operate => 0x0D,
        }
    }
}

/// Frame one command for the wire: `55 55 55 01 <code> <code>` (§3 — a one-byte checksum is
/// the byte itself).
pub fn spe_command(cmd: SpeCommand) -> Vec<u8> {
    spe_request(&[cmd.code()]).expect("a one-byte command always frames")
}

/// The SPE band ladder — index as the status string reports it (field 6) to a band name.
///
/// ⚠️ DERIVED, AND THE DERIVATION IS THE WHOLE VALUE. §5 publishes only the two ends: `00` is
/// 160m, and the top is `11` = 4m on the 1.3K-FA / `10` = 6m on the 2K-FA. Three things pin
/// the middle. A real 1.5K-FA reported `01` while its operator was on 80m. And 60m MUST be in
/// the ladder for the arithmetic to close: with it there are twelve bands and 4m lands exactly
/// on 11; without it there are eleven and 4m would land on 10, contradicting the published
/// endpoint. The 2K-FA then falls out as the same ladder minus 4m, putting 6m on 10 as stated.
///
/// Returns `None` for an index outside the ladder rather than guessing — a band an amplifier
/// reports and we cannot name is a newer model, not a bad frame.
pub fn spe_band_label(index: u8) -> Option<&'static str> {
    Some(match index {
        0 => "160m",
        1 => "80m",
        2 => "60m",
        3 => "40m",
        4 => "30m",
        5 => "20m",
        6 => "17m",
        7 => "15m",
        8 => "12m",
        9 => "10m",
        10 => "6m",
        11 => "4m",
        _ => return None,
    })
}

/// How many bytes follow the 4-byte header of a STATUS reply — `cnt` data bytes plus the
/// two-byte checksum and CRLF (§5).
///
/// Note this differs from an ACK, whose trailer is a single checksum byte. Only STATUS is
/// requested here, so only its shape is decoded; an ACK arriving on a shared port is not
/// something this module claims to frame.
///
/// `None` when the header is short or not an amplifier→host frame.
pub fn spe_status_reply_len(header: &[u8]) -> Option<usize> {
    if header.len() < SPE_HEADER_LEN || header[..3] != [SPE_AMP_SYNC; 3] {
        return None;
    }
    Some(header[3] as usize + SPE_STATUS_TRAILER_LEN)
}

/// ATU state for the selected TX antenna — the second byte of field 7 (§5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeAtu {
    /// `t` — a tunable antenna.
    Tunable,
    /// `b` — ATU bypassed.
    Bypassed,
    /// `a` — ATU enabled.
    Enabled,
    /// A code this firmware reports and the spec does not list.
    Unknown(char),
}

/// Front-panel power level (§5, field 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpePowerLevel {
    Low,
    Mid,
    High,
    Unknown(char),
}

/// Warnings (§5, and the WARNING table). `Unknown` is deliberate — see [`SpeAlarm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeWarning {
    None,
    AlarmAmplifier,
    NoSelectedAntenna,
    SwrAntenna,
    NoValidBand,
    PowerLimitExceeded,
    Overheating,
    AtuNotAvailable,
    TuningWithNoPower,
    AtuBypassed,
    PowerSwitchHeldByRemote,
    CombinerOverheating,
    CombinerFault,
    Unknown(char),
}

/// Alarms (§5, and the ALARMS table).
///
/// ⚠️ An unrecognised code becomes `Unknown`, never `None`. A later firmware adding an alarm
/// letter must not read to this application as "no alarm" — the failure direction of a status
/// decoder in front of a kilowatt has to be toward reporting a fault, not toward silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeAlarm {
    None,
    SwrExceedingLimits,
    AmplifierProtection,
    InputOverdriving,
    ExcessOverheating,
    CombinerFault,
    Unknown(char),
}

impl SpeWarning {
    /// `true` for anything other than an explicit "no warnings".
    pub fn is_raised(self) -> bool {
        self != SpeWarning::None
    }
}

impl SpeAlarm {
    /// `true` for anything other than an explicit "no alarms" — an unknown code counts.
    pub fn is_raised(self) -> bool {
        self != SpeAlarm::None
    }
}

/// One decoded status string (§5).
#[derive(Debug, Clone, PartialEq)]
pub struct SpeStatus {
    /// `20K` (2K-FA) or `13K` (1.3K-FA), kept raw: an id we do not recognise is a newer
    /// amplifier, not a bad frame.
    pub model: String,
    /// `true` = OPERATE, `false` = STANDBY.
    pub operate: bool,
    /// `true` = the amplifier sees the exciter keyed.
    pub transmitting: bool,
    /// Memory bank `A`/`B` on the 1.3K-FA. The 2K-FA always reports `x`, which becomes `None`.
    pub bank: Option<char>,
    /// Selected input port, 1 or 2.
    pub input: u8,
    /// The amplifier's own band index: 0 = 160 m … 10 = 6 m (2K-FA) or 11 = 4 m (1.3K-FA).
    ///
    /// ⚠️ NEEDS-BENCH: the spec gives only those endpoints, so the intermediate mapping is
    /// derived, not quoted. 160/80/60/40/30/20/17/15/12/10/6/4 is the only ladder that lands
    /// both endpoints where §5 puts them — but it is an inference until a real amplifier is
    /// asked, so this stays an index here and no band name is put on screen from it yet.
    pub band_index: u8,
    /// Selected TX antenna.
    pub tx_antenna: u8,
    /// ATU state for that antenna.
    pub atu: SpeAtu,
    /// A separate RX-only antenna where one is set; `0r` on the wire means none.
    pub rx_antenna: Option<u8>,
    pub power_level: SpePowerLevel,
    /// Measured output power in watts. Zero on receive.
    pub output_watts: u16,
    /// VSWR measured before the ATU. Zero on receive.
    pub swr_atu: f32,
    /// VSWR measured at the antenna. Zero on receive.
    pub swr_antenna: f32,
    /// PA supply voltage — 48.0 on a 2K-FA in operate at high power. Zero on receive.
    pub volts: f32,
    /// PA supply current. Zero on receive.
    pub amps: f32,
    /// Heatsink temperature, upper (the only one on a 1.3K-FA).
    ///
    /// ⚠️ **THE UNIT IS NOT ON THE WIRE.** §5 says "Temp in °C or F" — the amplifier reports
    /// whatever its own display is set to, and the status string does not say which. This number
    /// therefore cannot be labelled with a unit from the protocol alone.
    pub temp_upper: i16,
    /// Lower heatsink; a 1.3K-FA always reports 000.
    pub temp_lower: i16,
    /// Power-combiner temperature; a 1.3K-FA always reports 000.
    pub temp_combiner: i16,
    pub warning: SpeWarning,
    pub alarm: SpeAlarm,
}

/// Parse a complete status frame — header, data, checksum and CRLF (§5).
///
/// The frame is validated before a field is believed: amplifier sync bytes, a length that
/// matches what arrived, the 16-bit checksum, and the CRLF terminator.
///
/// Two deliberate choices about strictness, both from the document itself:
///
/// - **Fields are split on commas, never read at fixed offsets.** §5 states the string is 67
///   characters, and its own worked example is 65 — every individual field length in the table
///   is right, the total is not. A parser keyed to offsets would be built on the wrong one of
///   two numbers that the vendor's own document disagrees about.
/// - **At least 19 fields, not exactly 19.** A firmware that appends a field still decodes.
pub fn parse_spe_status(frame: &[u8]) -> Option<SpeStatus> {
    if frame.len() < SPE_HEADER_LEN || frame[..3] != [SPE_AMP_SYNC; 3] {
        return None;
    }
    let cnt = frame[3] as usize;
    // Accept BOTH trailers the document describes — 5 bytes as its worked example and real
    // hardware show, 4 as its field table says. Being strict about the example would reject an
    // amplifier that followed the table, and we have no frame from one to know either way.
    let trailer_len = frame.len().checked_sub(SPE_HEADER_LEN + cnt)?;
    if trailer_len != SPE_STATUS_TRAILER_LEN && trailer_len != SPE_STATUS_TRAILER_LEN - 1 {
        return None;
    }
    let data = &frame[SPE_HEADER_LEN..SPE_HEADER_LEN + cnt];
    let trailer = &frame[SPE_HEADER_LEN + cnt..];

    // CHK byte0 = SUM % 256, byte1 = SUM / 256 — a little-endian 16-bit sum over the data only,
    // not the mod-256 byte a command frame carries.
    let sum: u32 = data.iter().map(|b| u32::from(*b)).sum();
    if trailer[0] != (sum % 256) as u8 || trailer[1] != (sum / 256) as u8 {
        return None;
    }
    // CRLF terminates, wherever it falls; a separator before it must be the documented comma.
    if trailer[trailer_len - 2] != b'\r' || trailer[trailer_len - 1] != b'\n' {
        return None;
    }
    if trailer_len == SPE_STATUS_TRAILER_LEN && trailer[2] != b',' {
        return None;
    }

    let text = std::str::from_utf8(data).ok()?;
    // ⭐ THE DATA OPENS WITH A COMMA (§5's example, confirmed on a real 1.5K-FA), so a plain
    // split yields an empty first part and EVERY field index below shifts by one — the model
    // reads back blank and each reading belongs to its neighbour. Silent and total.
    let text = text.strip_prefix(',').unwrap_or(text);
    let f: Vec<&str> = text.split(',').collect();
    if f.len() < SPE_STATUS_FIELDS {
        return None;
    }
    // A blank model is the visible symptom of a shifted split. Refuse rather than report it.
    if f[0].trim().is_empty() {
        return None;
    }

    // Fields are space-padded to a fixed width; trim before parsing every one of them.
    let num = |i: usize| f[i].trim();
    let first = |i: usize| f[i].trim().chars().next();

    Some(SpeStatus {
        model: f[0].trim().to_string(),
        operate: first(1) == Some('O'),
        transmitting: first(2) == Some('T'),
        bank: first(3).filter(|c| *c == 'A' || *c == 'B'),
        input: num(4).parse().ok()?,
        band_index: num(5).parse().ok()?,
        tx_antenna: first(6).and_then(|c| c.to_digit(10))? as u8,
        atu: match f[6].trim().chars().nth(1) {
            Some('t') => SpeAtu::Tunable,
            Some('b') => SpeAtu::Bypassed,
            Some('a') => SpeAtu::Enabled,
            Some(c) => SpeAtu::Unknown(c),
            None => SpeAtu::Unknown(' '),
        },
        rx_antenna: first(7)
            .and_then(|c| c.to_digit(10))
            .filter(|n| *n != 0)
            .map(|n| n as u8),
        power_level: match first(8) {
            Some('L') => SpePowerLevel::Low,
            Some('M') => SpePowerLevel::Mid,
            Some('H') => SpePowerLevel::High,
            Some(c) => SpePowerLevel::Unknown(c),
            None => SpePowerLevel::Unknown(' '),
        },
        output_watts: num(9).parse().ok()?,
        swr_atu: num(10).parse().ok()?,
        swr_antenna: num(11).parse().ok()?,
        volts: num(12).parse().ok()?,
        amps: num(13).parse().ok()?,
        temp_upper: num(14).parse().ok()?,
        temp_lower: num(15).parse().ok()?,
        temp_combiner: num(16).parse().ok()?,
        warning: match first(17) {
            Some('N') => SpeWarning::None,
            Some('M') => SpeWarning::AlarmAmplifier,
            Some('A') => SpeWarning::NoSelectedAntenna,
            Some('S') => SpeWarning::SwrAntenna,
            Some('B') => SpeWarning::NoValidBand,
            Some('P') => SpeWarning::PowerLimitExceeded,
            Some('O') => SpeWarning::Overheating,
            Some('Y') => SpeWarning::AtuNotAvailable,
            Some('W') => SpeWarning::TuningWithNoPower,
            Some('K') => SpeWarning::AtuBypassed,
            Some('R') => SpeWarning::PowerSwitchHeldByRemote,
            Some('T') => SpeWarning::CombinerOverheating,
            Some('C') => SpeWarning::CombinerFault,
            Some(c) => SpeWarning::Unknown(c),
            None => SpeWarning::Unknown(' '),
        },
        alarm: match first(18) {
            Some('N') => SpeAlarm::None,
            Some('S') => SpeAlarm::SwrExceedingLimits,
            Some('A') => SpeAlarm::AmplifierProtection,
            Some('D') => SpeAlarm::InputOverdriving,
            Some('H') => SpeAlarm::ExcessOverheating,
            Some('C') => SpeAlarm::CombinerFault,
            Some(c) => SpeAlarm::Unknown(c),
            None => SpeAlarm::Unknown(' '),
        },
    })
}
/// Does this reply look like an **EXPERT 1K-FA**, which speaks a different protocol?
///
/// SPE ships two incompatible amplifier protocols under one brand, and they share only the frame
/// layout above. The 1K-FA ("Communication Protocol Specifications Rev. 2.0") runs at 9600 baud,
/// answers with a 30-byte BINARY record of bitfields rather than the 1.3K/2K-FA's ASCII string,
/// wraps keystrokes in a `KEY_PRESSED` opcode, and needs an RCU on/off handshake first.
///
/// Nothing here decodes it. This exists so the poller can tell an operator *which* amplifier it
/// found instead of reporting nothing at all — a 1K-FA owner seeing "no amplifier" would file a
/// fault against a link that is working perfectly and simply speaking the other dialect. The
/// tell is that document's own status record: `CNT` of 0x1E, and a `STATUS_CODE` of 0xA0 or 0xA1
/// in the first data byte.
pub fn spe_looks_like_1k_fa(frame: &[u8]) -> bool {
    frame.len() > SPE_HEADER_LEN
        && frame[..3] == [SPE_AMP_SYNC; 3]
        && frame[3] == 0x1E
        && matches!(frame[SPE_HEADER_LEN], 0xA0 | 0xA1)
}

// ────────────────────────── Elecraft KPA ──────────────────────────

/// Build an Elecraft query: `^<verb>;`.
///
/// ⚠️ **Verbs are two OR THREE upper-case letters.** This function used to require exactly two,
/// which silently made five documented commands unaskable — `^RVM` (firmware version), `^BRP`
/// and `^BRX` (the port data rates), `^DMO` (demo mode) and `^FLC` (clear fault). The examples
/// in this comment were `FR` and `AE`, which the **KPA500 does not implement at all**; they are
/// KPA1500-only, so the one worked example here was of a command half the supported amplifiers
/// would ignore. Both errors came from reading Hamlib rather than Elecraft's own reference.
///
/// The verbs a status poll actually wants, all present on both amplifiers: `OS` (operate or
/// standby), `ON` (power), `BN` (band), `TM` (PA temperature, °C), `VI` (volts and current),
/// `WS` (power and SWR), `FL` (current fault).
///
/// Anything else is refused rather than sent: the amplifier ignores a malformed command in
/// silence, and a silent amplifier is indistinguishable from an unplugged one.
pub fn kpa_query(verb: &str) -> Option<String> {
    let ok = matches!(verb.len(), 2 | 3) && verb.bytes().all(|b| b.is_ascii_uppercase());
    ok.then(|| format!("^{verb};"))
}

/// The null command — a bare `;`, which the KPA echoes back as `;`.
///
/// Elecraft documents this as the way to confirm the PC is talking to the amplifier, and it is
/// the only probe here that asks for nothing and changes nothing. That makes it the honest
/// presence check, and the one safe way to sweep for the port's data rate: the KPA's speed is
/// operator-selectable (`^BRP`: 4800 / 9600 / 19200 / 38400) and is remembered in the amplifier,
/// so Nexus cannot assume one — but it can send a semicolon at each and see which answers.
pub fn kpa_ping() -> &'static str {
    ";"
}

/// Is `reply` the KPA's answer to [`kpa_ping`]?
pub fn kpa_ping_ok(reply: &str) -> bool {
    reply.trim() == ";"
}

/// Extract the payload of a KPA reply for `verb` — `^WS250 015;` with verb `WS` yields
/// `250 015`, which the caller reads as 250 W at 1.5:1 (Elecraft puts an implied decimal point
/// before the last digit of the SWR, and after the second digit of each `^VI` value).
///
/// Returns `None` when the reply is for a DIFFERENT verb, which matters more than it looks: the
/// KPA sends unsolicited status, so a reply arriving after a query is not necessarily the answer
/// TO it. Matching the verb is what stops a temperature being read as an output power.
///
/// The payload is returned whole, spaces and all — `^VI` and `^WS` both pack two values into one
/// reply, and splitting them is the caller's business, not the framing's.
pub fn kpa_payload<'a>(reply: &'a str, verb: &str) -> Option<&'a str> {
    let body = reply.trim().strip_prefix('^')?.strip_suffix(';')?;
    let rest = body.strip_prefix(verb)?;
    Some(rest)
}

/// The four data rates a KPA can be set to (`^BRP`), fastest first.
///
/// The amplifier remembers its rate, so Nexus cannot assume one — it sweeps them with
/// [`kpa_ping`], which is the only probe that asks for nothing and changes nothing.
pub const KPA_BAUDS: [u32; 4] = [38_400, 19_200, 9_600, 4_800];

/// Decode a `^VI` payload — `"480 325"` is 48.0 V and 32.5 A.
///
/// Elecraft puts an **implied decimal point after the second digit** of both values, so the
/// three digits are hundredths-of-a-hundred, not an integer. Reading `480` as 480 volts would
/// put a plausible, wildly wrong number in front of the operator, which is why this is a parser
/// with a test and not an inline `parse()`.
pub fn kpa_parse_vi(payload: &str) -> Option<(f32, f32)> {
    let (v, i) = payload.split_once(' ')?;
    Some((implied_tenth(v)?, implied_tenth(i.trim_start())?))
}

/// Decode a `^WS` payload — `"250 015"` is 250 W at 1.5:1.
///
/// The SWR carries the same implied decimal. Elecraft documents its range as 1.0–99.0 and says
/// it **reads `000` when not transmitting**, so a zero is "no reading", not a 0:1 match — it
/// comes back as `None` rather than as a number no antenna can produce.
pub fn kpa_parse_ws(payload: &str) -> Option<(u16, Option<f32>)> {
    let (w, s) = payload.split_once(' ')?;
    let watts = w.trim().parse().ok()?;
    let swr = implied_tenth(s.trim_start())?;
    Some((watts, (swr > 0.0).then_some(swr)))
}

/// Three digits with an implied decimal before the last: `"480"` → `48.0`.
fn implied_tenth(digits: &str) -> Option<f32> {
    let d = digits.trim();
    if d.len() != 3 || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(d.parse::<u16>().ok()? as f32 / 10.0)
}

/// One decoded KPA reading. Every field comes from a verb present on **both** the KPA500 and the
/// KPA1500, so this struct is the same for either amplifier.
#[derive(Debug, Clone, PartialEq)]
pub struct KpaStatus {
    /// `^OS` — `true` = operate, `false` = standby.
    pub operate: bool,
    /// `^BN` — 0 = 160 m … 10 = 6 m. Elecraft publishes this ladder in full and notes it matches
    /// the K3S/K3, so unlike the SPE index this one needs no bench confirmation.
    pub band_index: u8,
    /// `^TM` — PA temperature. Documented as degrees **Celsius**, 0–150.
    pub temp_c: u16,
    /// `^VI` — PA supply voltage and current.
    pub volts: f32,
    pub amps: f32,
    /// `^WS` — output power in watts.
    pub output_watts: u16,
    /// `^WS` — SWR, or `None` when the amplifier is not transmitting.
    pub swr: Option<f32>,
    /// `^FL` — current fault identifier; `0` means no fault is active.
    pub fault: u8,
}

impl KpaStatus {
    /// `true` when the amplifier is reporting a fault.
    pub fn has_fault(self: &KpaStatus) -> bool {
        self.fault != 0
    }
}

// ─────────────────────── SPE transport (serial) ───────────────────────

/// The link itself, gated exactly as the WinKeyer's is: the codec above compiles everywhere and
/// is tested everywhere; only the half that opens a port needs the `serial` feature.
#[cfg(feature = "serial")]
mod imp {
    use super::{
        kpa_parse_vi, kpa_parse_ws, kpa_payload, kpa_ping, kpa_ping_ok, kpa_query,
        parse_spe_status, spe_command, spe_looks_like_1k_fa, spe_status_reply_len,
        spe_status_request, KpaStatus, SpeCommand, SpeStatus, KPA_BAUDS, SPE_HEADER_LEN,
    };
    use serialport::SerialPort;
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    /// 115200 8N1, per §1 — and the amplifier adapts downward by itself, so this is the only
    /// speed Nexus ever needs to offer and there is no baud setting to get wrong.
    const SPE_BAUD: u32 = 115_200;

    /// Turn a `serialport::Error` into an `io::Error` **keeping its kind**.
    ///
    /// ⭐ THE KIND IS THE ONLY THING THAT DISTINGUISHES A BUSY PORT FROM AN ABSENT ONE, and the
    /// poller has to tell an operator which. Flattening every open failure to
    /// `io::Error::other` — which this did — threw that away and left only an English sentence
    /// to parse, so "the amplifier is on your CAT port" and "the amplifier is switched off"
    /// arrived indistinguishable.
    fn io_from_serial(e: serialport::Error) -> std::io::Error {
        match e.kind {
            serialport::ErrorKind::Io(k) => std::io::Error::new(k, e.description),
            // "The device is not available. This could indicate that the device is in use by
            // another process or was disconnected" — the crate's own words, and it does not
            // separate the two. NotFound is the honest reading of an ambiguous answer: it is
            // the one that does NOT accuse another program of holding the port.
            serialport::ErrorKind::NoDevice => {
                std::io::Error::new(std::io::ErrorKind::NotFound, e.description)
            }
            _ => std::io::Error::other(e.description),
        }
    }

    /// One status string is ~71 bytes — under a millisecond of wire time at 115200. This bounds
    /// an amplifier that accepted the connection and then went quiet, not the transfer.
    const SPE_TIMEOUT: Duration = Duration::from_millis(500);

    /// An open SPE Expert link. Owns the port and nothing else; all framing is the pure codec.
    pub struct SpeLink {
        port: Box<dyn SerialPort>,
    }

    impl SpeLink {
        /// Open `port` at the documented line settings.
        pub fn open(port: &str) -> std::io::Result<Self> {
            let sp = serialport::new(port, SPE_BAUD)
                .data_bits(serialport::DataBits::Eight)
                .stop_bits(serialport::StopBits::One)
                .parity(serialport::Parity::None)
                .flow_control(serialport::FlowControl::None)
                .timeout(SPE_TIMEOUT)
                .open()
                .map_err(io_from_serial)?;
            Ok(Self { port: sp })
        }

        /// Ask for one status string and decode it.
        ///
        /// ⭐ **A POLL THAT GOT NO ANSWER IS A FAILURE**, and so is one that got an answer this
        /// module cannot vouch for — the rotator's worst defect was an empty reply read as an
        /// acknowledgement, and an amplifier is not a place to repeat it. Every early return
        /// here is an `Err`; none of them is a default reading. In particular a frame whose
        /// checksum fails is an error, never a status with plausible-looking numbers in it.
        ///
        /// On any framing failure the input buffer is drained, so a desync costs one poll rather
        /// than every poll after it.
        /// Send ONE keystroke command (§4). Fire-and-forget by design.
        ///
        /// ⭐ NO REPLY IS READ, AND THAT IS DELIBERATE. §3 says a keystroke draws either an ACK
        /// or a STATUS, and which one is not stated per command. Reading a reply here would
        /// mean guessing its shape, and a wrong guess desyncs the stream for the poll that
        /// follows — turning one uncertain command into every reading after it being wrong. The
        /// next poll is 1 s away and reports the amplifier's ACTUAL state, which is the only
        /// confirmation worth having: `OPERATE` is a toggle, so what matters is where the
        /// amplifier ended up, never what we believe we sent. Anything the amplifier volunteers
        /// in the meantime is drained by the poll's own framing recovery.
        ///
        /// ⛔ The caller must have established that the amplifier is NOT transmitting. This
        /// method does not check — it cannot, having no reading of its own — and it is private
        /// to the poll thread, which holds a status frame from one moment earlier.
        pub fn send_command(&mut self, cmd: SpeCommand) -> std::io::Result<()> {
            self.port.write_all(&spe_command(cmd))?;
            self.port.flush()
        }

        pub fn poll(&mut self) -> std::io::Result<SpeStatus> {
            self.port.write_all(&spe_status_request())?;
            self.port.flush()?;

            let mut header = [0u8; SPE_HEADER_LEN];
            if let Err(e) = self.port.read_exact(&mut header) {
                self.drain();
                return Err(e);
            }

            let body_len = match spe_status_reply_len(&header) {
                Some(n) => n,
                None => {
                    self.drain();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "not an SPE reply: the frame did not open with the amplifier sync bytes",
                    ));
                }
            };

            let mut frame = header.to_vec();
            frame.resize(SPE_HEADER_LEN + body_len, 0);
            if let Err(e) = self.port.read_exact(&mut frame[SPE_HEADER_LEN..]) {
                self.drain();
                return Err(e);
            }

            // Named before it is called corrupt: this is the other SPE protocol, on a link that
            // is working perfectly. Telling the operator which amplifier answered is the whole
            // reason this check exists.
            if spe_looks_like_1k_fa(&frame) {
                self.drain();
                // `Unsupported` is what makes this reach the operator as "wrong model"
                // rather than as "no amplifier": the link is working perfectly and the
                // amplifier is answering — in the other dialect.
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "this is an EXPERT 1K-FA, which speaks a different protocol Nexus does not \
                     support yet — the 1.3K-FA, 1.5K-FA and 2K-FA are the models Nexus speaks to",
                ));
            }

            parse_spe_status(&frame).ok_or_else(|| {
                self.drain();
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "SPE status frame failed its checksum or was malformed",
                )
            })
        }

        /// Discard whatever is still inbound, so the next poll starts on a frame boundary.
        fn drain(&mut self) {
            let _ = self.port.clear(serialport::ClearBuffer::Input);
        }
    }

    /// Per-verb reply budget. The KPA answers immediately; this bounds a link that went quiet.
    const KPA_TIMEOUT: Duration = Duration::from_millis(400);
    /// A shorter budget while sweeping rates, so four wrong guesses do not cost two seconds.
    const KPA_PROBE_TIMEOUT: Duration = Duration::from_millis(250);
    /// How many replies to skip while looking for the one matching the verb we asked. The KPA
    /// sends unsolicited status, so the next line is not necessarily our answer.
    const KPA_MAX_SKIPPED: usize = 8;
    /// A reply is `^XXdddd;` — a few dozen bytes. This bounds a port emitting noise.
    const KPA_MAX_REPLY: usize = 64;

    /// An open Elecraft KPA500 or KPA1500. One type for both: the KPA500's verbs are a strict
    /// subset of the KPA1500's, so every command this issues works on either amplifier.
    pub struct KpaLink {
        port: Box<dyn SerialPort>,
    }

    impl KpaLink {
        /// Open `port`, discovering its data rate.
        ///
        /// The KPA's speed is operator-selectable and remembered in the amplifier, so unlike the
        /// SPE there is nothing to assume — but there is also nothing to ask the operator. Each
        /// rate gets a bare `;`, which Elecraft documents the amplifier as echoing back, and the
        /// one that answers is the one it is set to. The probe reads nothing and changes nothing,
        /// which is what makes sweeping with it safe.
        ///
        /// Returns the link and the rate that answered.
        pub fn open(port: &str) -> std::io::Result<(Self, u32)> {
            let mut last: Option<std::io::Error> = None;
            for baud in KPA_BAUDS {
                let mut sp = match serialport::new(port, baud)
                    .data_bits(serialport::DataBits::Eight)
                    .stop_bits(serialport::StopBits::One)
                    .parity(serialport::Parity::None)
                    .flow_control(serialport::FlowControl::None)
                    .timeout(KPA_PROBE_TIMEOUT)
                    .open()
                {
                    Ok(sp) => sp,
                    Err(e) => {
                        last = Some(io_from_serial(e));
                        continue;
                    }
                };
                let _ = sp.clear(serialport::ClearBuffer::All);
                if sp.write_all(kpa_ping().as_bytes()).is_err() || sp.flush().is_err() {
                    continue;
                }
                if matches!(read_framed(&mut *sp, KPA_PROBE_TIMEOUT), Ok(r) if kpa_ping_ok(&r)) {
                    let _ = sp.set_timeout(KPA_TIMEOUT);
                    return Ok((Self { port: sp }, baud));
                }
            }
            // Every open SUCCEEDED and nothing answered: the port is fine and the amplifier
            // is not talking. `TimedOut` says that; `other` would have looked like a fault.
            Err(last.unwrap_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "no KPA answered on any of its four data rates — check the port, and that \
                     the amplifier is powered on",
                )
            }))
        }

        /// Read one full status set.
        ///
        /// Six queries rather than one: Elecraft has no combined status command, so a reading is
        /// assembled. Every one of these verbs exists on both amplifiers.
        pub fn poll(&mut self) -> std::io::Result<KpaStatus> {
            let operate = self.ask("OS")? == "1";
            let band_index = self.ask("BN")?.parse().map_err(bad("^BN band"))?;
            let temp_c = self.ask("TM")?.parse().map_err(bad("^TM temperature"))?;
            let (volts, amps) =
                kpa_parse_vi(&self.ask("VI")?).ok_or_else(|| malformed("^VI volts/current"))?;
            let (output_watts, swr) =
                kpa_parse_ws(&self.ask("WS")?).ok_or_else(|| malformed("^WS power/SWR"))?;
            let fault = self.ask("FL")?.parse().map_err(bad("^FL fault"))?;

            Ok(KpaStatus {
                operate,
                band_index,
                temp_c,
                volts,
                amps,
                output_watts,
                swr,
                fault,
            })
        }

        /// Send one query and return the payload of the reply **to that verb**.
        ///
        /// ⭐ The KPA sends unsolicited status, so the next line to arrive is not necessarily the
        /// answer to what was just asked. Replies for other verbs are skipped rather than
        /// returned — without this a temperature of `042` lands in the pane as 42 watts. The skip
        /// is bounded, so a chattering amplifier ends as an error rather than a stall.
        fn ask(&mut self, verb: &str) -> std::io::Result<String> {
            let q = kpa_query(verb).ok_or_else(|| malformed("an unknown verb"))?;
            self.port.write_all(q.as_bytes())?;
            self.port.flush()?;

            read_matching(&mut *self.port, verb, KPA_TIMEOUT)
        }
    }

    /// Read replies until one is the answer to `verb`, skipping the rest.
    ///
    /// ⭐ Split out of `ask` so it can be tested against an in-memory reader — this is the logic
    /// that stops a `^TM042;` temperature being returned as the answer to `^WS` and landing in
    /// the pane as 42 watts, and it was previously reachable only with an amplifier on the desk.
    ///
    /// The skip is bounded: a chattering amplifier ends as an error, never a stall.
    fn read_matching<R: Read + ?Sized>(
        r: &mut R,
        verb: &str,
        budget: Duration,
    ) -> std::io::Result<String> {
        for _ in 0..KPA_MAX_SKIPPED {
            let reply = read_framed(r, budget)?;
            if let Some(payload) = kpa_payload(&reply, verb) {
                return Ok(payload.to_string());
            }
        }
        Err(std::io::Error::other(format!(
            "the amplifier never answered ^{verb} — {KPA_MAX_SKIPPED} other replies arrived first"
        )))
    }

    /// Read one `;`-terminated reply against an overall deadline.
    ///
    /// The deadline bounds the whole reply, not each read: a reply arriving in two pieces is a
    /// split reply, not a foreign one — the same rule `rig.rs` states for CAT, and the reason a
    /// partial answer is never treated as a complete one.
    fn read_framed<R: Read + ?Sized>(port: &mut R, budget: Duration) -> std::io::Result<String> {
        let deadline = Instant::now() + budget;
        let mut out = Vec::new();
        let mut b = [0u8; 1];
        while Instant::now() < deadline {
            match port.read(&mut b) {
                // End of stream — an unplugged USB serial adapter returns this, and returns it
                // forever. Yield before looking again: without the pause this arm spins flat out
                // until the deadline (measured at ~2M iterations in 50 ms), which costs nothing
                // on a one-shot call and a whole core on a polling thread. The deadline still
                // bounds the reply, so no timing contract moves.
                Ok(0) => std::thread::sleep(Duration::from_millis(1)),
                Ok(_) => {
                    out.push(b[0]);
                    if b[0] == b';' {
                        return String::from_utf8(out).map_err(|_| malformed("a non-ASCII reply"));
                    }
                    if out.len() >= KPA_MAX_REPLY {
                        return Err(malformed("a reply with no terminator"));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "the amplifier stopped answering",
        ))
    }

    /// `InvalidData`, not `other`: the poller maps the KIND to the operator-facing reason, and
    /// "the amplifier said something we cannot read" is a different fact from "it said nothing".
    fn malformed(what: &str) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("the amplifier sent {what} this cannot read"),
        )
    }

    fn bad<E>(what: &'static str) -> impl Fn(E) -> std::io::Error {
        move |_| malformed(what)
    }

    #[cfg(test)]
    mod transport_tests {
        use super::*;
        use std::io::Cursor;

        /// A short budget: these readers hit EOF, and the timeout paths should not cost 400 ms.
        const T: Duration = Duration::from_millis(50);

        #[test]
        fn a_reply_is_read_up_to_its_semicolon() {
            let mut r = Cursor::new(b"^OS1;".to_vec());
            assert_eq!(read_framed(&mut r, T).unwrap(), "^OS1;");

            // A second reply is left in the stream, not swallowed with the first.
            let mut r = Cursor::new(b"^OS1;^TM042;".to_vec());
            assert_eq!(read_framed(&mut r, T).unwrap(), "^OS1;");
            assert_eq!(read_framed(&mut r, T).unwrap(), "^TM042;");
        }

        #[test]
        fn an_unterminated_reply_is_an_error_not_a_partial_reading() {
            // No semicolon ever arrives — this must time out, never return what it managed to get.
            let mut r = Cursor::new(b"^OS1".to_vec());
            assert!(
                read_framed(&mut r, T).is_err(),
                "a partial answer is not an answer"
            );

            // POSITIVE CONTROL: the same reader WITH its terminator succeeds.
            let mut r = Cursor::new(b"^OS1;".to_vec());
            assert!(read_framed(&mut r, T).is_ok());
        }

        /// ⭐ THE ONE THAT MATTERS. Elecraft documents unsolicited status, so the next reply to
        /// arrive is not necessarily the answer to what was just asked.
        #[test]
        fn unsolicited_status_is_skipped_rather_than_returned_as_the_answer() {
            // Three unsolicited lines arrive before the ^WS we asked for.
            let mut r = Cursor::new(b"^OS1;^TM042;^BN05;^WS250 015;".to_vec());
            assert_eq!(read_matching(&mut r, "WS", T).unwrap(), "250 015");

            // Without the skip, the FIRST line would have been the answer — and a temperature of
            // 042 would have been read as 42 watts. Pin that it is not.
            let mut r = Cursor::new(b"^TM042;^WS250 015;".to_vec());
            let got = read_matching(&mut r, "WS", T).unwrap();
            assert_eq!(got, "250 015");
            assert_ne!(got, "042", "a temperature must never answer a power query");
        }

        /// ⭐ A DEAD PORT MUST NOT BURN A CORE. `read()` returning `Ok(0)` is end-of-stream —
        /// an unplugged USB serial adapter does exactly this, and does it forever. With no
        /// pause in that arm the loop spins flat out until the deadline, which is harmless for
        /// a one-shot call and is a pegged core once this runs on a polling thread.
        ///
        /// `Err(TimedOut) => continue` is NOT the same thing and must stay as it is: the OS has
        /// already blocked for the budget by the time it returns.
        #[test]
        fn an_end_of_stream_port_does_not_spin_the_cpu() {
            struct Dead(std::cell::Cell<usize>);
            impl Read for Dead {
                fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                    self.0.set(self.0.get() + 1);
                    Ok(0)
                }
            }
            let d = Dead(std::cell::Cell::new(0));
            let mut d = d;
            assert!(read_framed(&mut d, T).is_err(), "a dead port is an error");
            let calls = d.0.get();
            assert!(
                calls < 500,
                "spun {calls} times in {T:?} — a dead port is burning a core"
            );
        }

        #[test]
        fn a_chattering_amplifier_ends_as_an_error_not_a_stall() {
            // More non-matching replies than the bound allows.
            let noise = "^OS1;".repeat(KPA_MAX_SKIPPED + 2);
            let mut r = Cursor::new(noise.into_bytes());
            assert!(
                read_matching(&mut r, "WS", T).is_err(),
                "bounded, not infinite"
            );

            // POSITIVE CONTROL: just inside the bound still succeeds, so the limit is not
            // rejecting ordinary traffic.
            let mut ok = "^OS1;".repeat(KPA_MAX_SKIPPED - 1);
            ok.push_str("^WS250 015;");
            let mut r = Cursor::new(ok.into_bytes());
            assert_eq!(read_matching(&mut r, "WS", T).unwrap(), "250 015");
        }
    }
}

#[cfg(feature = "serial")]
pub use imp::{KpaLink, SpeLink};

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrap a status data string in a valid frame, computing the length and checksum the way
    /// §5 specifies. Used to build fixtures — the checksum rule itself is asserted separately.
    fn status_frame(data: &str) -> Vec<u8> {
        let sum: u32 = data.bytes().map(u32::from).sum();
        let mut f = vec![SPE_AMP_SYNC; 3];
        f.push(data.len() as u8);
        f.extend_from_slice(data.as_bytes());
        f.push((sum % 256) as u8);
        f.push((sum / 256) as u8);
        // The comma §5's worked example shows between the checksum and the terminator, and
        // that a real 1.5K-FA sends. This helper once emitted a bare CRLF, which made every
        // fixture in this module agree with the parser and disagree with every amplifier.
        f.extend_from_slice(b",\r\n");
        f
    }

    /// The spec's own worked example (§5), verbatim — INCLUDING the comma at each end.
    ///
    /// ⭐ THIS CONSTANT USED TO BE WRONG, AND ITS COMMENT EXPLAINED THE WRONGNESS AWAY. It was
    /// transcribed without the opening and closing commas, giving 65 characters against §5's
    /// stated 67, and the old comment called that "the document disagrees with itself". It does
    /// not: 65 + 2 commas = 67 exactly. Reading the shortfall as a document defect rather than a
    /// transcription slip is what let the leading-comma field shift survive.
    ///
    /// The count is now self-checking (below), and the checksum is checkable against the
    /// document too: §5 prints the low byte as `%`, and 0x25 is what this string sums to.
    const SPEC_EXAMPLE: &str =
        ",20K,S,R,x,1,00,1a,0r,L,0000, 0.00, 0.00, 0.0, 0.0, 33,  0,  0,N,N,";

    #[test]
    fn spe_frames_the_documented_request_examples() {
        // §3's worked example: OPERATE is `55 55 55 01 0D 0D`.
        assert_eq!(
            spe_request(&[0x0D]).expect("a frame"),
            vec![0x55, 0x55, 0x55, 0x01, 0x0D, 0x0D]
        );
        // §5's: STATUS is `55 55 55 01 90 90`.
        assert_eq!(
            spe_status_request(),
            vec![0x55, 0x55, 0x55, 0x01, 0x90, 0x90]
        );

        // The checksum sums the COMMAND bytes only — not the sync bytes, not the count.
        let f = spe_request(&[0x10, 0x20, 0x30]).expect("a frame");
        assert_eq!(f, vec![0x55, 0x55, 0x55, 0x03, 0x10, 0x20, 0x30, 0x60]);
        // And it WRAPS rather than saturating: it is one byte and a long command overflows it.
        assert_eq!(
            *spe_request(&[0xFF, 0x02]).expect("a frame").last().unwrap(),
            0x01
        );

        // Refused rather than sent malformed.
        assert!(
            spe_request(&[]).is_none(),
            "an empty command is not a frame"
        );
        assert!(spe_request(&[0u8; 256]).is_none(), "cnt is one byte");
    }

    /// ⭐ THE CORRECTION. Both halves of this were wrong before the vendor spec was read, and
    /// both would have desynced the link on the amplifier's very first reply.
    #[test]
    fn spe_reply_framing_uses_the_amplifier_sync_and_counts_data_only() {
        // §5: reply sync is 0xAA. The request sync 0x55 is NOT a reply header — the previous
        // code matched on 0x55 and would have rejected every frame the amplifier ever sent.
        assert_eq!(spe_status_reply_len(&[0x55, 0x55, 0x55, 0x43]), None);
        assert!(
            spe_status_reply_len(&[0xAA, 0xAA, 0xAA, 0x43]).is_some(),
            "0xAA is what an amplifier actually sends"
        );

        // §3: cnt counts DATA bytes, checksum excluded. A status reply's trailer is 2 checksum
        // bytes + CRLF, so 0x43 = 67 data bytes means 71 more bytes to read — not 63 (cnt-4,
        // which underflows on a short frame) and not 64 (Hamlib's cnt-3).
        assert_eq!(
            spe_status_reply_len(&[0xAA, 0xAA, 0xAA, 0x43]),
            Some(67 + 5),
            "trailer is checksum pair + comma + CRLF; reading 4 leaves a byte in the port \
             buffer and desyncs the next poll"
        );

        // And a small count must not UNDERFLOW: the old cnt-4 returned None for every frame
        // with fewer than four data bytes, which is most of them.
        assert_eq!(spe_status_reply_len(&[0xAA, 0xAA, 0xAA, 0x01]), Some(6));

        // Not a frame at all.
        assert_eq!(spe_status_reply_len(&[0xAA, 0xAA, 0xAA]), None);
        assert_eq!(spe_status_reply_len(&[]), None);
    }

    /// ⭐ REAL HARDWARE. The exact 76 bytes an **EXPERT 1.5K-FA** put on the wire on
    /// 2026-08-29, captured with the standalone probe. Until this landed, every fixture in this
    /// module was built by `status_frame` from our own reading of §5 — so the tests agreed with
    /// the parser and neither agreed with an amplifier. This is the first frame here that a
    /// machine actually sent, and it is the arbiter.
    ///
    /// Two things it settles, both of which the document states ambiguously:
    /// - the 67 data bytes OPEN AND CLOSE WITH A COMMA, and
    /// - a comma sits between the checksum pair and the CRLF.
    ///
    /// It also answers the question the model list could not: **a 1.5K-FA calls itself `15K`**.
    #[test]
    fn a_real_1_5k_fa_frame_off_the_wire_parses() {
        let mut frame = vec![0xAA, 0xAA, 0xAA, 0x43];
        let data = b",15K,S,R,A,1,01,1a,0r,M,0000, 0.00, 0.00, 0.0, 0.0,081,000,000,N,N,";
        assert_eq!(
            data.len(),
            0x43,
            "the CNT byte counts these, commas and spaces included"
        );
        frame.extend_from_slice(data);
        // Checksum as the amplifier sent it: lo = SUM % 256, hi = SUM / 256.
        frame.extend_from_slice(&[0x47, 0x0D]);
        // The trailer the field table omits and the worked example shows.
        frame.extend_from_slice(b",\r\n");
        assert_eq!(frame.len(), 76, "what actually arrived");

        let s = parse_spe_status(&frame).expect("a real amplifier's own status frame must parse");

        assert_eq!(
            s.model, "15K",
            "the 1.5K-FA names itself; no document says this"
        );
        assert!(!s.operate, "S = standby");
        assert!(!s.transmitting, "R = receive");
        assert_eq!(s.bank, Some('A'));
        assert_eq!(s.input, 1);
        assert_eq!(s.band_index, 1);
        assert_eq!(s.tx_antenna, 1);
        assert_eq!(s.atu, SpeAtu::Enabled, "the 'a' of \"1a\"");
        assert_eq!(s.rx_antenna, None, "\"0r\" = no RX-only antenna");
    }

    /// ⛔ THE BYTE THAT MUST NEVER GO OUT. `SWITCH OFF` is `0x0A` and `TUNE` is `0x09` — the two
    /// keystrokes either side of the commands we do send. This walks every variant the enum can
    /// express and asserts none of them frames either byte. It is a guard against a future edit,
    /// not against today's code: today's is obviously fine, and that is exactly when such a rule
    /// gets broken.
    #[test]
    fn no_command_nexus_can_express_switches_the_amplifier_off() {
        let all = [
            SpeCommand::BandDown,
            SpeCommand::BandUp,
            SpeCommand::Operate,
        ];
        for c in all {
            let f = spe_command(c);
            assert!(
                !f[4..].contains(&0x0A),
                "{c:?} framed SWITCH OFF (0x0A) — an operator's amplifier would power down"
            );
            assert!(!f[4..].contains(&0x09), "{c:?} framed TUNE (0x09) unasked");
        }

        // The control: the framer CAN emit those bytes, so the assertions above are testing the
        // enum's restraint and not a framer that is incapable of the mistake.
        let danger = spe_request(&[0x0A]).expect("a frame");
        assert!(
            danger[4..].contains(&0x0A),
            "the framer is not what protects us"
        );
    }

    /// The three commands frame exactly as §3/§4 specify, checksum included.
    #[test]
    fn spe_commands_frame_as_the_document_specifies() {
        // §3: a one-byte command's checksum is the byte itself.
        assert_eq!(
            spe_command(SpeCommand::BandDown),
            vec![0x55, 0x55, 0x55, 0x01, 0x02, 0x02]
        );
        assert_eq!(
            spe_command(SpeCommand::BandUp),
            vec![0x55, 0x55, 0x55, 0x01, 0x03, 0x03]
        );
        // OPERATE is the same byte §3's own worked example uses for STANDBY→OPERATE.
        assert_eq!(
            spe_command(SpeCommand::Operate),
            vec![0x55, 0x55, 0x55, 0x01, 0x0D, 0x0D]
        );
    }

    /// The band ladder, checked against every anchor that exists rather than against itself.
    #[test]
    fn the_band_ladder_matches_every_published_and_measured_anchor() {
        // §5's two published endpoints.
        assert_eq!(spe_band_label(0), Some("160m"), "§5: 00 is 160m");
        assert_eq!(
            spe_band_label(11),
            Some("4m"),
            "§5: the 1.3K-FA's top index is 4m"
        );
        assert_eq!(
            spe_band_label(10),
            Some("6m"),
            "§5: the 2K-FA's top index is 6m"
        );

        // Measured: a real 1.5K-FA reported 01 with its operator on 80m (2026-08-29).
        assert_eq!(
            spe_band_label(1),
            Some("80m"),
            "measured on hardware, not inferred"
        );

        // The arithmetic that forces 60m in: without it the ladder is 11 long and 4m could not
        // land on 11. This asserts the consequence, so dropping 60m fails here loudly.
        assert_eq!(
            spe_band_label(2),
            Some("60m"),
            "60m must be present for 4m to reach 11"
        );
        assert_eq!(
            (0..=11).filter(|i| spe_band_label(*i).is_some()).count(),
            12,
            "twelve bands, 160m through 4m"
        );

        // An index past the ladder is unknown, not guessed.
        assert_eq!(spe_band_label(12), None);
        assert_eq!(spe_band_label(255), None);
    }

    #[test]
    fn spe_parses_the_specs_own_example_string() {
        let s = parse_spe_status(&status_frame(SPEC_EXAMPLE)).expect("the spec's example parses");

        assert_eq!(s.model, "20K");
        assert!(!s.operate, "S = standby");
        assert!(!s.transmitting, "R = receive");
        assert_eq!(s.bank, None, "a 2K-FA reports x, which is not a bank");
        assert_eq!(s.input, 1);
        assert_eq!(s.band_index, 0);
        assert_eq!(s.tx_antenna, 1);
        assert_eq!(s.atu, SpeAtu::Enabled, "the 'a' of \"1a\"");
        assert_eq!(s.rx_antenna, None, "\"0r\" means no RX-only antenna is set");
        assert_eq!(s.power_level, SpePowerLevel::Low);
        assert_eq!(s.output_watts, 0);
        assert_eq!(s.swr_atu, 0.0);
        assert_eq!(s.swr_antenna, 0.0);
        assert_eq!(s.volts, 0.0);
        assert_eq!(s.amps, 0.0);
        assert_eq!(s.temp_upper, 33);
        assert_eq!(s.temp_lower, 0);
        assert_eq!(s.temp_combiner, 0);
        assert_eq!(s.warning, SpeWarning::None);
        assert_eq!(s.alarm, SpeAlarm::None);
        assert!(!s.alarm.is_raised() && !s.warning.is_raised());
    }

    #[test]
    fn spe_parses_a_transmitting_amplifier() {
        // A 1.3K-FA in operate, keyed, 1250 W into a 1.4:1 antenna on 20 m, bank B, RX antenna 2.
        let s = parse_spe_status(&status_frame(
            "13K,O,T,B,2,05,3t,2r,H,1250, 1.10, 1.40,48.0,32.5, 47,000,000,N,N",
        ))
        .expect("parses");

        assert_eq!(s.model, "13K");
        assert!(s.operate && s.transmitting);
        assert_eq!(s.bank, Some('B'));
        assert_eq!(s.input, 2);
        assert_eq!(s.band_index, 5);
        assert_eq!(s.tx_antenna, 3);
        assert_eq!(s.atu, SpeAtu::Tunable);
        assert_eq!(s.rx_antenna, Some(2));
        assert_eq!(s.power_level, SpePowerLevel::High);
        assert_eq!(s.output_watts, 1250);
        assert_eq!(s.swr_atu, 1.10);
        assert_eq!(s.swr_antenna, 1.40);
        assert_eq!(s.volts, 48.0);
        assert_eq!(s.amps, 32.5);
        assert_eq!(s.temp_upper, 47);
    }

    /// ⭐ THE FAILURE DIRECTION THAT MATTERS. A status decoder in front of a kilowatt must never
    /// turn a code it does not know into "no alarm".
    #[test]
    fn an_unrecognised_alarm_code_is_not_silence() {
        let raised = |data: &str| {
            let s = parse_spe_status(&status_frame(data)).expect("parses");
            (s.warning, s.alarm)
        };

        // Documented codes decode.
        let (w, a) = raised("20K,O,T,x,1,05,1a,0r,H,1250, 3.10, 3.40,48.0,32.5, 47,  0,  0,S,S");
        assert_eq!(w, SpeWarning::SwrAntenna);
        assert_eq!(a, SpeAlarm::SwrExceedingLimits);
        assert!(a.is_raised(), "an SWR alarm is raised");

        // A letter no firmware in this spec emits becomes Unknown — and Unknown IS raised.
        let (w, a) = raised("20K,O,T,x,1,05,1a,0r,H,1250, 1.10, 1.40,48.0,32.5, 47,  0,  0,Z,Q");
        assert_eq!(w, SpeWarning::Unknown('Z'));
        assert_eq!(a, SpeAlarm::Unknown('Q'));
        assert!(
            a.is_raised() && w.is_raised(),
            "an unknown code must read as a fault, never as quiet"
        );

        // POSITIVE CONTROL for the assertion above: the same predicate must go the other way on
        // the documented no-alarm letter, or `is_raised` proves nothing.
        let (w, a) = raised(SPEC_EXAMPLE);
        assert!(!a.is_raised() && !w.is_raised(), "N really is quiet");
    }

    #[test]
    fn spe_rejects_a_frame_it_cannot_trust() {
        let good = status_frame(SPEC_EXAMPLE);
        assert!(
            parse_spe_status(&good).is_some(),
            "control: this one parses"
        );

        // Wrong sync — a host frame is not a reply.
        let mut f = good.clone();
        f[0] = SPE_HOST_SYNC;
        assert_eq!(parse_spe_status(&f), None);

        // A corrupted data byte fails the checksum rather than decoding to a wrong reading.
        let mut f = good.clone();
        f[SPE_HEADER_LEN + 10] ^= 0x01;
        assert_eq!(parse_spe_status(&f), None, "checksum catches a flipped bit");

        // A count that does not match what arrived.
        let mut f = good.clone();
        f[3] = f[3].wrapping_sub(1);
        assert_eq!(parse_spe_status(&f), None);

        // Missing terminator.
        let mut f = good.clone();
        let n = f.len();
        f[n - 1] = b'\0';
        assert_eq!(parse_spe_status(&f), None, "CRLF terminates a status frame");

        // Truncated, and empty.
        assert_eq!(parse_spe_status(&good[..good.len() - 3]), None);
        assert_eq!(parse_spe_status(&[]), None);

        // Too few fields — a truncated string that still checksums must not half-decode.
        assert_eq!(parse_spe_status(&status_frame("20K,S,R,x,1,00")), None);
    }

    /// The parser must key off the wire's own count byte, not a length assumed from the page.
    ///
    /// ⭐ THIS TEST USED TO ASSERT THE BUG. It pinned `SPEC_EXAMPLE.len() == 65` and called the
    /// gap to §5's stated 67 a contradiction in the document. The document was right and the
    /// transcription was wrong — the two absent characters are the commas that open and close
    /// the string, and the missing leading one shifted every field index by one.
    #[test]
    fn spe_status_length_comes_from_the_wire_not_from_the_document() {
        // §5 states 67 characters "included commas and spaces", and its example is 67 once
        // transcribed whole. The document agrees with itself.
        assert_eq!(
            SPEC_EXAMPLE.len(),
            67,
            "§5's stated count, and its example, agree"
        );
        assert!(parse_spe_status(&status_frame(SPEC_EXAMPLE)).is_some());

        // The low checksum byte the DOCUMENT prints for that example is `%`. Computing it from
        // the transcription above and getting the same character is what proves the string was
        // copied correctly — the check the 65-character version could never have passed.
        let sum: u32 = SPEC_EXAMPLE.bytes().map(u32::from).sum();
        assert_eq!(
            (sum % 256) as u8,
            b'%',
            "§5 prints the low checksum byte as '%'"
        );

        // A same-length variant with different padding and no leading comma — the field table's
        // reading. Still 19 fields, still parses, so neither form is rejected.
        let padded = "20K,S,R,x,1,00,1a,0r,L,0000, 0.00, 0.00,  0.0,  0.0, 33,  0,  0,N,N";
        assert_eq!(padded.len(), 67, "and the length §5 states");
        let s = parse_spe_status(&status_frame(padded)).expect("parses too");
        assert_eq!(s.temp_upper, 33);
        assert_eq!(s.volts, 0.0);

        // A firmware that appends a 20th field still decodes the 19 that are specified.
        let extended = format!("{SPEC_EXAMPLE},7");
        assert!(parse_spe_status(&status_frame(&extended)).is_some());
    }

    /// A 1K-FA on the port is a DIFFERENT protocol, not a broken link — and saying so is the
    /// difference between a clear answer and a fault report against working hardware.
    #[test]
    fn a_1k_fa_reply_is_recognised_rather_than_read_as_junk() {
        // That family's status record: cnt 0x1E, then STATUS_CODE 0xA0/0xA1.
        let mut f = vec![SPE_AMP_SYNC; 3];
        f.push(0x1E);
        f.push(0xA0);
        f.extend_from_slice(&[0u8; 29]);
        assert!(
            spe_looks_like_1k_fa(&f),
            "the other SPE dialect is identified"
        );
        // It is emphatically NOT decodable as an FA-family status string.
        assert_eq!(parse_spe_status(&f), None);

        // POSITIVE CONTROL, both ways: the amplifier this module DOES speak must not trip it,
        // or the check would report every 2K-FA as an unsupported model.
        let fa = status_frame(SPEC_EXAMPLE);
        assert!(!spe_looks_like_1k_fa(&fa), "a 2K-FA is not a 1K-FA");
        assert!(parse_spe_status(&fa).is_some());
        // And a host-sync frame is neither.
        assert!(!spe_looks_like_1k_fa(&[0x55, 0x55, 0x55, 0x1E, 0xA0]));
    }

    /// ⭐ THE SECOND CORRECTION FROM A VENDOR DOCUMENT. Requiring exactly two letters made five
    /// documented commands unaskable, and the old worked example (`FR`) is a verb the KPA500
    /// does not implement.
    #[test]
    fn kpa_queries_are_built_for_two_and_three_letter_verbs() {
        // The status verbs, all present on BOTH amplifiers.
        for v in ["OS", "ON", "BN", "TM", "VI", "WS", "FL"] {
            assert_eq!(kpa_query(v).as_deref(), Some(format!("^{v};").as_str()));
        }
        // Three-letter verbs are real and must frame — this is the regression.
        for v in ["RVM", "BRP", "BRX", "DMO", "FLC"] {
            assert_eq!(
                kpa_query(v).as_deref(),
                Some(format!("^{v};").as_str()),
                "{v} is a documented KPA command and was previously unaskable"
            );
        }

        // Refused rather than sent — the KPA ignores a malformed command in silence, and
        // silence is indistinguishable from a dead link.
        assert!(kpa_query("F").is_none(), "one letter is not a verb");
        assert!(kpa_query("FRQX").is_none(), "four is not a verb either");
        assert!(kpa_query("fr").is_none(), "we always send upper case");
        assert!(kpa_query("R2").is_none(), "digits are data, not the verb");
        assert!(kpa_query("").is_none());
    }

    /// The null command is Elecraft's own documented "is anyone there" — and the only probe that
    /// asks for nothing and changes nothing, which is what makes it safe to sweep baud rates with.
    /// ⭐ THE IMPLIED DECIMAL POINT. Elecraft packs `48.0 V` as `480`, so a plain integer parse
    /// puts a plausible and wildly wrong number in front of the operator — 480 volts on a supply
    /// that runs at 48, or 250 W shown as a 15:1 SWR.
    #[test]
    fn kpa_readings_honour_elecrafts_implied_decimal_point() {
        // ^VI480 325; — 48.0 volts, 32.5 amps.
        let (v, i) = kpa_parse_vi("480 325").expect("parses");
        assert_eq!(v, 48.0);
        assert_eq!(i, 32.5);
        assert_ne!(v, 480.0, "reading it as an integer is the bug this pins");

        // ^WS250 015; — 250 watts at 1.5:1.
        let (w, swr) = kpa_parse_ws("250 015").expect("parses");
        assert_eq!(w, 250);
        assert_eq!(swr, Some(1.5));

        // Documented top of the SWR range.
        assert_eq!(kpa_parse_ws("999 990").unwrap().1, Some(99.0));
    }

    /// Elecraft documents SWR reading `000` when not transmitting. Zero is "no reading", not a
    /// 0:1 match — no antenna produces that, and showing it would be a lie with a number on it.
    #[test]
    fn kpa_swr_is_absent_rather_than_zero_on_receive() {
        let (w, swr) = kpa_parse_ws("000 000").expect("parses");
        assert_eq!(w, 0);
        assert_eq!(
            swr, None,
            "not transmitting is no reading, not a perfect match"
        );

        // POSITIVE CONTROL: a real transmitting reading DOES come back as a number, or the
        // check above would be hiding every SWR the amplifier ever reports.
        assert_eq!(kpa_parse_ws("100 012").unwrap().1, Some(1.2));
    }

    #[test]
    fn kpa_readings_refuse_malformed_payloads() {
        // Both parsers need exactly three digits per field — anything else is not a reading.
        assert_eq!(kpa_parse_vi("48 325"), None, "two digits is not the format");
        assert_eq!(kpa_parse_vi("4800 325"), None, "four is not either");
        assert_eq!(kpa_parse_vi("480"), None, "^VI carries two values");
        assert_eq!(kpa_parse_vi("abc def"), None);
        assert_eq!(kpa_parse_vi(""), None);
        assert_eq!(kpa_parse_ws("250"), None, "^WS carries two values");
        assert_eq!(kpa_parse_ws("25O 015"), None, "letter O is not a zero");

        // POSITIVE CONTROL: the well-formed case still parses, or these prove nothing.
        assert!(kpa_parse_vi("480 325").is_some());
        assert!(kpa_parse_ws("250 015").is_some());
    }

    #[test]
    fn kpa_fault_zero_is_the_only_quiet_value() {
        let ok = KpaStatus {
            operate: true,
            band_index: 5,
            temp_c: 42,
            volts: 48.0,
            amps: 32.5,
            output_watts: 250,
            swr: Some(1.5),
            fault: 0,
        };
        assert!(!ok.has_fault());
        assert!(KpaStatus {
            fault: 7,
            ..ok.clone()
        }
        .has_fault());
        assert!(
            KpaStatus { fault: 1, ..ok }.has_fault(),
            "any non-zero is a fault"
        );
    }

    #[test]
    fn kpa_ping_is_a_bare_semicolon_echoed_back() {
        assert_eq!(kpa_ping(), ";");
        assert!(kpa_ping_ok(";"));
        assert!(kpa_ping_ok("  ;\r\n"), "framing whitespace is tolerated");

        // POSITIVE CONTROL: silence and junk must NOT read as a live amplifier, or the probe
        // would report every unplugged port as an amplifier.
        assert!(!kpa_ping_ok(""), "silence is not an answer");
        assert!(!kpa_ping_ok("^OS1;"), "some other reply is not the echo");
        assert!(!kpa_ping_ok(";;"), "two is not one");
    }

    #[test]
    fn kpa_payload_matches_the_verb_it_was_asked_for() {
        // The real status replies, in Elecraft's documented formats.
        assert_eq!(kpa_payload("^OS1;", "OS"), Some("1"), "operate");
        assert_eq!(kpa_payload("^TM042;", "TM"), Some("042"), "42 °C");
        assert_eq!(kpa_payload("^FL00;", "FL"), Some("00"), "no fault");
        // Two values in one reply come back whole — splitting them is the caller's job.
        assert_eq!(kpa_payload("^WS250 015;", "WS"), Some("250 015"));
        assert_eq!(kpa_payload("^VI480 325;", "VI"), Some("480 325"));
        // Three-letter verbs parse too.
        assert_eq!(kpa_payload("^RVM01.23;", "RVM"), Some("01.23"));
        // Whitespace around a framed reply is tolerated.
        assert_eq!(kpa_payload("  ^OS1;\r\n", "OS"), Some("1"));

        // ⭐ THE ONE THAT MATTERS. The KPA sends unsolicited status, so the next thing to arrive
        // after a query is not necessarily its answer. A temperature must NEVER be read as an
        // output power — 042 °C would land in the pane as 42 watts.
        assert_eq!(
            kpa_payload("^TM042;", "WS"),
            None,
            "wrong verb is not an answer"
        );

        // Unframed junk is not a reply.
        assert_eq!(kpa_payload("WS250 015", "WS"), None, "no ^ and no ;");
        assert_eq!(kpa_payload("^WS250 015", "WS"), None, "unterminated");
        assert_eq!(kpa_payload("", "WS"), None);
    }
}
