//! Icom CI-V command table — pure encoders/decoders for the CAT-parity verb set, built on
//! [`super::frame`]. No I/O. Covers the 7300-family (IC-7300/7610/9700/705/905), whose
//! command numbers are shared; per-model differences are the CI-V **address** (below) and a
//! few band/mode specifics handled by the caller.
//!
//! A command builder returns a [`Frame`] (`.to_bytes()` for the wire); a decoder takes a
//! *reply* frame and extracts the value. Set commands are acknowledged with a bare
//! `FB`/`FA` ([`Frame::is_ack`]/[`Frame::is_nak`]).

use super::frame::{bcd_to_freq, freq_to_bcd, Frame};

/// The Icom rigs Nexus knows how to drive natively over CI-V, with their factory-default
/// CI-V bus address. (The address is user-changeable on the rig; the serial engine lets the
/// operator override it, defaulting to these.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcomModel {
    Ic7300,
    Ic7610,
    Ic9700,
    Ic705,
    Ic905,
}

impl IcomModel {
    /// The factory-default CI-V address for this model.
    pub fn default_civ_addr(self) -> u8 {
        match self {
            IcomModel::Ic7300 => 0x94,
            IcomModel::Ic7610 => 0x98,
            IcomModel::Ic9700 => 0xA2,
            IcomModel::Ic705 => 0xA4,
            IcomModel::Ic905 => 0xAC,
        }
    }

    /// This model's Hamlib model number — the key the dual-receiver capability table
    /// ([`crate::dualrx`]) and the rest of the curated catalogue are indexed by. The inverse
    /// of `rigmodels::icom_scope_model`; a test holds the two in step.
    pub fn hamlib_model(self) -> u32 {
        match self {
            IcomModel::Ic7300 => 3073,
            IcomModel::Ic7610 => 3078,
            IcomModel::Ic9700 => 3081,
            IcomModel::Ic705 => 3085,
            IcomModel::Ic905 => 3090,
        }
    }

    /// Recognize a model from a human/rig model name (e.g. "Icom IC-9700"). Case- and
    /// separator-insensitive on the `ic####` token.
    pub fn from_name(name: &str) -> Option<Self> {
        let n: String = name
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        // Longest/most-specific tokens first so "ic905" doesn't shadow nothing, etc.
        for (tok, m) in [
            ("ic7300", IcomModel::Ic7300),
            ("ic7610", IcomModel::Ic7610),
            ("ic9700", IcomModel::Ic9700),
            ("ic705", IcomModel::Ic705),
            ("ic905", IcomModel::Ic905),
        ] {
            if n.contains(tok) {
                return Some(m);
            }
        }
        None
    }
}

/// Operating modes as CI-V mode bytes (command `0x04`/`0x06` payload byte 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Lsb,
    Usb,
    Am,
    Cw,
    Rtty,
    Fm,
    CwR,
    RttyR,
}

impl Mode {
    pub fn to_byte(self) -> u8 {
        match self {
            Mode::Lsb => 0x00,
            Mode::Usb => 0x01,
            Mode::Am => 0x02,
            Mode::Cw => 0x03,
            Mode::Rtty => 0x04,
            Mode::Fm => 0x05,
            Mode::CwR => 0x07,
            Mode::RttyR => 0x08,
        }
    }
    pub fn from_byte(b: u8) -> Option<Mode> {
        Some(match b {
            0x00 => Mode::Lsb,
            0x01 => Mode::Usb,
            0x02 => Mode::Am,
            0x03 => Mode::Cw,
            0x04 => Mode::Rtty,
            0x05 => Mode::Fm,
            0x07 => Mode::CwR,
            0x08 => Mode::RttyR,
            _ => return None,
        })
    }
    /// The canonical uppercase name Nexus/Hamlib use.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Lsb => "LSB",
            Mode::Usb => "USB",
            Mode::Am => "AM",
            Mode::Cw => "CW",
            Mode::Rtty => "RTTY",
            Mode::Fm => "FM",
            Mode::CwR => "CWR",
            Mode::RttyR => "RTTYR",
        }
    }
    pub fn from_name(name: &str) -> Option<Mode> {
        Some(match name.to_ascii_uppercase().as_str() {
            "LSB" => Mode::Lsb,
            "USB" => Mode::Usb,
            "AM" => Mode::Am,
            "CW" => Mode::Cw,
            "RTTY" | "FSK" => Mode::Rtty,
            "FM" => Mode::Fm,
            "CWR" | "CW-R" => Mode::CwR,
            "RTTYR" | "RTTY-R" | "FSKR" => Mode::RttyR,
            _ => return None,
        })
    }
}

// ---- command builders (controller → radio) ----

/// Read the operating frequency (cmd `03`). Reply carries 5-byte BCD freq.
pub fn read_freq(radio: u8) -> Frame {
    Frame::command(radio, 0x03, &[])
}
/// Set the operating frequency (cmd `05`).
pub fn set_freq(radio: u8, hz: u64) -> Frame {
    Frame::command(radio, 0x05, &freq_to_bcd(hz))
}
/// Read the operating mode + filter (cmd `04`).
pub fn read_mode(radio: u8) -> Frame {
    Frame::command(radio, 0x04, &[])
}
/// Set the operating mode (cmd `06`), optionally selecting a filter (1/2/3).
pub fn set_mode(radio: u8, mode: Mode, filter: Option<u8>) -> Frame {
    match filter {
        Some(f) => Frame::command(radio, 0x06, &[mode.to_byte(), f]),
        None => Frame::command(radio, 0x06, &[mode.to_byte()]),
    }
}
/// Read PTT state (cmd `1C 00`). Reply data `[0x00, 0x00|0x01]`.
pub fn read_ptt(radio: u8) -> Frame {
    Frame::command(radio, 0x1C, &[0x00])
}
/// Set PTT (cmd `1C 00`): `tx=true` keys the transmitter.
pub fn set_ptt(radio: u8, tx: bool) -> Frame {
    Frame::command(radio, 0x1C, &[0x00, u8::from(tx)])
}
/// Read the S-meter (cmd `15 02`). Reply is a 2-byte big-endian BCD level 0000–0255.
pub fn read_smeter(radio: u8) -> Frame {
    Frame::command(radio, 0x15, &[0x02])
}
/// Read RF output power (cmd `14 0A`). Reply is a 2-byte BCD level 0000–0255.
pub fn read_rf_power(radio: u8) -> Frame {
    Frame::command(radio, 0x14, &[0x0A])
}
/// Set RF output power (cmd `14 0A`) as a percentage 0–100 (mapped to 0–255).
pub fn set_rf_power(radio: u8, percent: u8) -> Frame {
    let level = u16::from(percent.min(100)) * 255 / 100;
    let [hi, lo] = level_to_bcd2(level);
    Frame::command(radio, 0x14, &[0x0A, hi, lo])
}

// ---- extended verbs (split / VFO / RIT / CW / repeater / data mode) ----

/// Select a VFO (cmd `07`): `VFOA`/`VFOB` (also accepts `Main`/`Sub` for the IC-9700's
/// main/sub bands). `None` for a name the rig has no equivalent for.
pub fn select_vfo(radio: u8, vfo: &str) -> Option<Frame> {
    let b = match vfo.to_ascii_uppercase().as_str() {
        "VFOA" | "A" => 0x00,
        "VFOB" | "B" => 0x01,
        "MAIN" => 0xD0,
        "SUB" => 0xD1,
        _ => return None,
    };
    Some(Frame::command(radio, 0x07, &[b]))
}
/// Split on/off (cmd `0F`, data `00`/`01`).
pub fn set_split(radio: u8, on: bool) -> Frame {
    Frame::command(radio, 0x0F, &[u8::from(on)])
}
/// FM duplex (repeater shift) — shares cmd `0F`: `10` simplex, `11` DUP−, `12` DUP+.
pub fn set_duplex(radio: u8, shift: &str) -> Frame {
    let b = match shift {
        "+" => 0x12,
        "-" => 0x11,
        _ => 0x10,
    };
    Frame::command(radio, 0x0F, &[b])
}
/// Set the UNSELECTED VFO's frequency (cmd `25 01`) — the split/duplex TX dial on the
/// 7300 family without swapping VFOs.
pub fn set_unselected_freq(radio: u8, hz: u64) -> Frame {
    let mut data = vec![0x01];
    data.extend_from_slice(&freq_to_bcd(hz));
    Frame::command(radio, 0x25, &data)
}
/// Set the UNSELECTED VFO's MODE (cmd `26 01`) — the split TX VFO's own mode
/// register, which plain `06` cannot reach without swapping VFOs.
///
/// The `01` selects the unselected VFO exactly as it does for the frequency in
/// [`set_unselected_freq`] (`25 01`); then the mode byte, then Icom's DATA-mode
/// flag. That flag is always `00` here: the modes this carries are the
/// satellite uplink's (FM/USB/LSB/CW), never a soundcard DATA submode.
///
/// The manual allows a trailing FILTER byte. It is deliberately omitted so the
/// write says only what it means and the rig keeps the filter the operator
/// chose — the same restraint `06` shows with `filter: None`.
pub fn set_unselected_mode(radio: u8, mode: Mode) -> Frame {
    Frame::command(radio, 0x26, &[0x01, mode.to_byte(), 0x00])
}
/// RIT/ΔTX offset (cmd `21 00`): ±9.999 kHz as 2-byte little-endian BCD magnitude + sign
/// byte (`00` = +, `01` = −). The offset register is shared by RIT and ΔTX.
pub fn set_rit_offset(radio: u8, hz: i32) -> Frame {
    let mag = hz.unsigned_abs().min(9_999);
    let lo = ((((mag / 10) % 10) as u8) << 4) | ((mag % 10) as u8);
    let hi = ((((mag / 1000) % 10) as u8) << 4) | (((mag / 100) % 10) as u8);
    Frame::command(radio, 0x21, &[0x00, lo, hi, u8::from(hz < 0)])
}
/// RIT on/off (cmd `21 01`).
pub fn set_rit_on(radio: u8, on: bool) -> Frame {
    Frame::command(radio, 0x21, &[0x01, u8::from(on)])
}
/// ΔTX (Icom's XIT) on/off (cmd `21 02`).
pub fn set_dtx_on(radio: u8, on: bool) -> Frame {
    Frame::command(radio, 0x21, &[0x02, u8::from(on)])
}
/// Does this radio have ΔTX (Icom's XIT) at all?
///
/// ⛔ THE IC-9700 DOES NOT. Its CI-V Reference Guide (A7508-3EX-4) lists two `21` commands,
/// `21 00` (RIT frequency) and `21 01` (RIT on/off), and no `21 02`; the IC-7610's
/// (A7380-7EX-4) has all three. Hamlib says the same (`rigctl -m 3081 -u`, 4.5.5 and 4.7.1:
/// `Can set XIT: N`, and no XIT among its functions). So on a 9700 the offset register
/// [`set_rit_offset`] writes is the RIT offset and nothing else, and an XIT sent there lands
/// on the RECEIVER's clarifier.
///
/// The other four models keep ΔTX exactly as before. Hamlib gives each of them XIT; they have
/// not been checked here against Icom's own reference for each radio.
pub fn has_delta_tx(model: IcomModel) -> bool {
    !matches!(model, IcomModel::Ic9700)
}
/// Icom's per-frame CW text limit (cmd `17`) — longer messages are chunked.
pub const MORSE_CHUNK: usize = 30;
/// Key CW from text (cmd `17`, ASCII payload ≤ [`MORSE_CHUNK`] chars).
pub fn send_morse(radio: u8, text: &str) -> Frame {
    let ascii: Vec<u8> = text
        .bytes()
        .filter(u8::is_ascii)
        .take(MORSE_CHUNK)
        .collect();
    Frame::command(radio, 0x17, &ascii)
}
/// Abort CW keying in progress (cmd `17` with the single byte `FF`).
pub fn stop_morse(radio: u8) -> Frame {
    Frame::command(radio, 0x17, &[0xFF])
}
/// Keyer speed (cmd `14 0C`): WPM 6–48 mapped onto the 0–255 level scale.
pub fn set_keyer_speed_wpm(radio: u8, wpm: u32) -> Frame {
    let wpm = wpm.clamp(6, 48);
    let level = ((wpm - 6) * 255 / 42) as u16;
    let [hi, lo] = level_to_bcd2(level);
    Frame::command(radio, 0x14, &[0x0C, hi, lo])
}
/// Repeater (CTCSS) tone frequency (cmd `1B 00`), in tenths of Hz as 4-digit BCD
/// (88.5 Hz → 0885).
pub fn set_repeater_tone(radio: u8, tenths: u32) -> Frame {
    let [hi, lo] = level_to_bcd2(tenths.min(9999) as u16);
    Frame::command(radio, 0x1B, &[0x00, hi, lo])
}
/// TONE function on/off (cmd `16 42`) — transmit the repeater tone.
pub fn set_tone_func(radio: u8, on: bool) -> Frame {
    Frame::command(radio, 0x16, &[0x42, u8::from(on)])
}

/// Satellite mode (`16 5A`): Main = downlink/RX, Sub = uplink/TX, full duplex —
/// the IC-9700/IC-910H cross-band contract. TX always leaves on Sub while it is
/// on; a rig without a Sub band (IC-7300 family) NAKs the read, which is the
/// capability probe.
pub const FUNC_SATMODE: u8 = 0x5A;

/// ON-OFF function sub-commands under CI-V command `0x16` (Icom
/// IC-7300/9700/7610/705 generation share this 16-family table): the DSP/audio
/// set — Noise Blanker, Noise Reduction, Auto Notch, speech Compressor,
/// Monitor, VOX, MANUAL notch — plus satellite mode ([`FUNC_SATMODE`], the token
/// Hamlib calls `SATMODE`). The Hamlib func token maps to its sub-command byte here —
/// one place, so both the getter and setter agree.
///
/// ⚠️ `ANF` AND `MN` ARE TWO CONTROLS, not two names for one. `ANF` is the automatic
/// notch, which hunts a carrier down by itself; `MN` is the manual notch an operator
/// parks on a heterodyne by ear. A rig may have either, both or neither, and this table
/// held only `ANF` — so native CI-V answered `RPRT -11` for `MN`, the poll latched it
/// unsupported and the cockpit dropped the button with no error at all.
pub fn func_sub(token: &str) -> Option<u8> {
    Some(match token {
        "NB" => 0x22,   // Noise Blanker
        "NR" => 0x40,   // Noise Reduction
        "ANF" => 0x41,  // Auto Notch Filter (hunts a carrier by itself)
        "COMP" => 0x44, // Speech Compressor
        "MON" => 0x45,  // Monitor
        "VOX" => 0x46,  // VOX
        "MN" => 0x48,   // MANUAL notch — a different control from ANF, on its own register
        "SATMODE" => FUNC_SATMODE,
        _ => return None,
    })
}
/// Read a `16 <sub>` DSP-function state — the reply carries the on/off byte.
pub fn read_dsp_func(radio: u8, sub: u8) -> Frame {
    Frame::command(radio, 0x16, &[sub])
}
/// Set a `16 <sub>` DSP function on/off.
pub fn set_dsp_func(radio: u8, sub: u8, on: bool) -> Frame {
    Frame::command(radio, 0x16, &[sub, u8::from(on)])
}
/// Extract the on/off state from a `16 <sub>` DSP-function reply.
pub fn parse_dsp_func(f: &Frame, sub: u8) -> Option<bool> {
    if f.cmd == 0x16 && f.data.first() == Some(&sub) {
        f.data.get(1).map(|&b| b != 0)
    } else {
        None
    }
}
/// Microphone gain (cmd `14 0B`): percent 0–100 mapped onto the 0–255 level scale.
pub fn set_mic_gain(radio: u8, percent: u8) -> Frame {
    let level = u16::from(percent.min(100)) * 255 / 100;
    let [hi, lo] = level_to_bcd2(level);
    Frame::command(radio, 0x14, &[0x0B, hi, lo])
}
/// Read the microphone gain (cmd `14 0B`) — raw 0–255.
pub fn read_mic_gain(radio: u8) -> Frame {
    Frame::command(radio, 0x14, &[0x0B])
}
/// Extract the raw mic-gain level (0–255) from a `14 0B` reply.
pub fn parse_mic_gain_raw(f: &Frame) -> Option<u16> {
    if f.cmd == 0x14 && f.data.first() == Some(&0x0B) && f.data.len() >= 3 {
        Some(level_from_bcd2(f.data[1], f.data[2]))
    } else {
        None
    }
}

/// A `0x14` level sub-command: Noise-Reduction level = 0x06, Noise-Blanker level = 0x12.
pub const LVL_NR: u8 = 0x06;
pub const LVL_NB: u8 = 0x12;
/// The three ANALOG levels on the same `0x14` family: AF gain (volume), RF gain (receive
/// front-end gain, NOT transmit power) and squelch.
///
/// THESE BYTES WERE READ OFF THE WIRE, not off a table from memory. Hamlib 4.5.5's own Icom
/// backend was driven against a pty with `rigctl -m 3073` (IC-7300) and the frames captured:
/// `L AF 0.5` → `14 01 01 27`, `L RF 0.5` → `14 02 01 27`, `L SQL 0.5` → `14 03 01 27`, and
/// the reads `l AF`/`l RF`/`l SQL` → `14 01`/`14 02`/`14 03`. The same capture produced
/// `14 0a` for RFPOWER, `14 0b` for MICGAIN and `14 06` for NR — three constants this file
/// already held, which is the positive control that the capture reports real bytes rather
/// than an artefact of the harness. The `01 27` payload is BCD 127, exactly what
/// [`level_to_bcd2`] produces for 50%.
pub const LVL_AF: u8 = 0x01;
pub const LVL_RF: u8 = 0x02;
pub const LVL_SQL: u8 = 0x03;
/// SPEECH-PROCESSOR DEPTH — how hard the compressor works, a 0..1 fraction on the same
/// `0x14` family. From the same capture: `L COMP 0.25` → `14 0e 00 63`, `0.5` → `14 0e 01 27`,
/// `1.0` → `14 0e 02 55`.
///
/// ⚠️ NOT [`METER_COMP`]. This is the knob; that is the TX meter reading how many dB of
/// compression the rig is applying, on the `0x15` family, and the broker has served it for
/// a long time under `COMP_METER`. `16 44` ([`func_sub`]) switches the processor on; without
/// this level there was no way to say how hard it should work.
pub const LVL_COMP: u8 = 0x0E;
/// TRANSMIT-MONITOR GAIN — how loud the rig plays your own transmitted audio back to you
/// while you are talking. The on/off half of this control was already here as the `MON`
/// func (`16 45`, [`func_sub`]); this is its volume, on the `0x14` family like every other
/// fraction. From the same capture: `L MONITOR_GAIN 0.25` -> `14 15 00 63`, `0.5` ->
/// `14 15 01 27`, `1.0` -> `14 15 02 55`, read `l MONITOR_GAIN` -> `14 15`.
///
/// ⚠️ NOT the AF gain. [`LVL_AF`] is the receiver's volume; this one is only heard while
/// transmitting, and turning it down does not quieten the received audio a decoder listens
/// to. The two are adjacent knobs on the front panel and adjacent bytes on the bus.
pub const LVL_MONITOR_GAIN: u8 = 0x15;

/// Hamlib level token → its `0x14` sub-command, for the fractional 0..1 levels the generic
/// [`set_dsp_level`] / [`read_dsp_level`] pair serves. One table shared by the broker's
/// getter and setter so the two cannot disagree — exactly what [`func_sub`] does for the
/// `0x16` on/off family.
///
/// RFPOWER (`14 0A`) and MICGAIN (`14 0B`) are in the same CI-V family and deliberately NOT
/// here: they have their own named builders above, and the broker answers them from those.
///
/// ⛔ AND NEITHER IS `NOTCHF`, THE MANUAL-NOTCH FREQUENCY — DO NOT ADD IT HERE.
/// [`set_dsp_level`] is a PERCENT path (`percent.min(100) * 255 / 100`); Hamlib's `NOTCHF`
/// is Hz. An entry here would clamp every notch frequency above 100 Hz to full scale and
/// park the notch at the end of its range for every operator, silently. Nor is one needed
/// for parity: Hamlib 4.5.5's own Icom backend has no `NOTCHF` for ANY rig Nexus drives
/// natively — it carries `NOTCHF_RAW` (`14 0D`), a raw 0..255 notch POSITION — so a
/// Hamlib-served IC-7300 answers `NOTCHF` "not available" exactly as this daemon does.
/// Serving it in Hz needs the rig's Hz↔position curve, which is a bench measurement.
pub fn level_sub(token: &str) -> Option<u8> {
    Some(match token {
        "AF" => LVL_AF,
        "RF" => LVL_RF,
        "SQL" => LVL_SQL,
        "NR" => LVL_NR,
        "NB" => LVL_NB,
        "COMP" => LVL_COMP,
        "MONITOR_GAIN" => LVL_MONITOR_GAIN,
        _ => return None,
    })
}
/// Set a `14 <sub>` DSP level from a 0–100 percent (mapped onto the 0–255 scale), like mic gain.
pub fn set_dsp_level(radio: u8, sub: u8, percent: u8) -> Frame {
    let level = u16::from(percent.min(100)) * 255 / 100;
    let [hi, lo] = level_to_bcd2(level);
    Frame::command(radio, 0x14, &[sub, hi, lo])
}
/// Read a `14 <sub>` DSP level — raw 0–255.
pub fn read_dsp_level(radio: u8, sub: u8) -> Frame {
    Frame::command(radio, 0x14, &[sub])
}
/// Extract the raw level (0–255) from a `14 <sub>` reply.
pub fn parse_dsp_level_raw(f: &Frame, sub: u8) -> Option<u16> {
    if f.cmd == 0x14 && f.data.first() == Some(&sub) && f.data.len() >= 3 {
        Some(level_from_bcd2(f.data[1], f.data[2]))
    } else {
        None
    }
}

/// ATTENUATOR — the receiver's input pad, on its OWN CI-V command rather than the `0x14`
/// level family, one byte carrying the attenuation **in BCD decibels** (`0x00` = off).
///
/// ⚠️ BCD, NOT RAW HEX, AND NOT A FRACTION. `L ATT 12` puts `11 12` on the wire, where
/// `0x12` reads as twelve. A raw-hex encoder would send `11 0c` and pad the receiver by
/// some other amount; a 0..1 fraction (the shape every other level here has) would send
/// nothing meaningful at all. Captured from Hamlib 4.5.5's own Icom backend against a pty
/// at the IC-7610's address, whose three pads there are what tell the two encodings apart:
/// `L ATT 6` -> `11 06`, `L ATT 12` -> `11 12`, `L ATT 18` -> `11 18`, `L ATT 0` -> `11 00`.
pub const ATT_CMD: u8 = 0x11;
/// PREAMP — on the `0x16` family but **deliberately not a [`func_sub`] entry**: that table
/// feeds [`parse_dsp_func`], which answers a bool, and a preamp has three states (off,
/// P.AMP1, P.AMP2). The payload byte is the preamp's POSITION, not its decibels — see
/// [`preamp_index_for_db`].
pub const FUNC_PREAMP: u8 = 0x02;

/// The attenuator pads this rig actually has, in dB, ascending. `0` (off) is NOT in the
/// list: every rig has it, and it is the implicit first position.
///
/// Read off Hamlib 4.5.5's own backend for each rig (`rigctl -m <model> 1`, the
/// `Attenuator:` line — the `rig_caps.attenuator[]` array its `\dump_state` also emits),
/// not transcribed from a manual. An attenuator is a LIST, not a slider: offering a rig a
/// value it does not own gets the command NAKed or silently rounded to a neighbour.
///
/// ⭐ THE IC-7610 IS THE EXCEPTION, and its list is Icom's own: the CI-V Reference Guide
/// (A7380-7EX-4, Sep. 2025, p. 3) gives command `11` fifteen pads, 3 to 45 dB in 3 dB steps.
/// Hamlib's backend declares only 6, 12 and 18 of them — which is what Nexus offered before
/// — and leaves an operator who wants 3 dB, or more than 18, without it. NEEDS-BENCH
/// (IC-7610): each new pad against the radio's own ATT readout.
///
/// ⛔ The IC-905 is empty because Hamlib 4.5.5 has no IC-905 backend to read it from
/// (`-m 3095` returns no caps at all). An empty list renders no control, which degrades
/// honestly; a guessed one would move the operator's front end by the wrong amount.
/// NEEDS-BENCH (IC-905).
pub fn attenuator_steps_db(model: IcomModel) -> &'static [u8] {
    match model {
        IcomModel::Ic7300 | IcomModel::Ic705 => &[20],
        IcomModel::Ic9700 => &[10],
        IcomModel::Ic7610 => &[3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 39, 42, 45],
        IcomModel::Ic905 => &[],
    }
}

/// The preamps this rig has, by the label Hamlib gives each one, in list order. Same source
/// and same exclusion of `0` as [`attenuator_steps_db`].
///
/// ⚠️ The 7300-family labels are `1` and `2` because they are Icom's P.AMP1/P.AMP2
/// SELECTORS carried in Hamlib's dB-labelled array — they are names, not gains. The IC-7610
/// labels its two `12` and `20`, which really are decibels. Nothing here may treat a label
/// as a number to send: [`preamp_index_for_db`] looks it up.
pub fn preamp_steps_db(model: IcomModel) -> &'static [u8] {
    match model {
        IcomModel::Ic7300 | IcomModel::Ic705 | IcomModel::Ic9700 => &[1, 2],
        IcomModel::Ic7610 => &[12, 20],
        IcomModel::Ic905 => &[],
    }
}

/// Set the attenuator to `db` dB (0 = off). The caller is responsible for offering only
/// values from [`attenuator_steps_db`]; this encodes whatever it is given.
pub fn set_attenuator_db(radio: u8, db: u8) -> Frame {
    Frame::command(radio, ATT_CMD, &[to_bcd(db)])
}
/// Read the attenuator setting (`11`, no payload) — the reply carries the dB in BCD.
pub fn read_attenuator(radio: u8) -> Frame {
    Frame::command(radio, ATT_CMD, &[])
}
/// Extract the attenuation in dB from an `11` reply.
pub fn parse_attenuator_db(f: &Frame) -> Option<u8> {
    (f.cmd == ATT_CMD)
        .then(|| f.data.first().map(|&b| from_bcd(b)))
        .flatten()
}

/// Set the preamp by its POSITION in the rig's list (0 = off, 1 = first preamp, …).
/// Translate an operator-facing label with [`preamp_index_for_db`] first.
pub fn set_preamp_index(radio: u8, idx: u8) -> Frame {
    Frame::command(radio, 0x16, &[FUNC_PREAMP, idx])
}
/// Read the preamp selection (`16 02`) — the reply carries the position.
pub fn read_preamp(radio: u8) -> Frame {
    Frame::command(radio, 0x16, &[FUNC_PREAMP])
}
/// Extract the preamp position from a `16 02` reply.
pub fn parse_preamp_index(f: &Frame) -> Option<u8> {
    if f.cmd == 0x16 && f.data.first() == Some(&FUNC_PREAMP) {
        f.data.get(1).copied()
    } else {
        None
    }
}

/// A preamp LABEL (what the operator picked) → the POSITION to put on the wire, for this
/// rig. `0` is off on every rig; a label this rig does not have is `None` rather than a
/// nearest match — a preamp is a list of the rig's own choices, and quietly substituting a
/// neighbour changes the front end by an amount nobody asked for.
pub fn preamp_index_for_db(model: IcomModel, db: u8) -> Option<u8> {
    if db == 0 {
        return Some(0);
    }
    let i = preamp_steps_db(model).iter().position(|&v| v == db)?;
    u8::try_from(i + 1).ok()
}
/// The inverse of [`preamp_index_for_db`]: a position read back off the rig → the label to
/// show. `None` for a position this rig's list cannot explain.
pub fn preamp_db_for_index(model: IcomModel, idx: u8) -> Option<u8> {
    if idx == 0 {
        return Some(0);
    }
    preamp_steps_db(model).get(usize::from(idx) - 1).copied()
}

/// AGC time constant (`16 12`), one byte. On the IC-7300/9700/705 family: 00=off, 01=FAST,
/// 02=MID, 03=SLOW. Hamlib carries AGC as an enum int (OFF=0, FAST=2, SLOW=3, MEDIUM=5); these
/// two helpers translate at the CI-V boundary so the rigctld side speaks Hamlib and the wire
/// speaks Icom. Unknown values fall back to MID.
pub fn agc_civ_from_hamlib(hamlib: u8) -> u8 {
    match hamlib {
        0 => 0x00, // OFF
        2 => 0x01, // FAST
        3 => 0x03, // SLOW
        _ => 0x02, // MEDIUM(5) and anything else → MID
    }
}
pub fn agc_hamlib_from_civ(civ: u8) -> u8 {
    match civ {
        0x00 => 0,
        0x01 => 2,
        0x03 => 3,
        _ => 5, // 0x02 MID and anything else → MEDIUM
    }
}
/// Set the AGC time constant (`16 12`) from an Icom byte (01=FAST/02=MID/03=SLOW).
pub fn set_agc(radio: u8, civ_byte: u8) -> Frame {
    Frame::command(radio, 0x16, &[0x12, civ_byte])
}
/// Read the AGC time constant (`16 12`).
pub fn read_agc(radio: u8) -> Frame {
    Frame::command(radio, 0x16, &[0x12])
}
/// Extract the raw AGC Icom byte from a `16 12` reply.
pub fn parse_agc_civ(f: &Frame) -> Option<u8> {
    if f.cmd == 0x16 && f.data.first() == Some(&0x12) {
        f.data.get(1).copied()
    } else {
        None
    }
}
/// FM repeater offset frequency (cmd `0D` set / `0C` read): 3-byte little-endian BCD in
/// 100 Hz units (600 kHz → `00 60 00`). The 10 MHz digit only applies on 1200 MHz.
pub fn set_rptr_offset(radio: u8, hz: u64) -> Frame {
    let bcd = freq_to_bcd(hz / 100);
    Frame::command(radio, 0x0D, &bcd[..3])
}
/// DATA mode (cmd `1A 06`). The first data byte is `00` for off, and for ON it is **which**
/// data mode — which is the part that was wrong here.
///
/// ⚠️ ON A SINGLE-DATA-MODE ICOM (IC-7300) the byte is simply 1 = on. On the multi-data-mode
/// radios (IC-7610, IC-9700, IC-705, IC-905) it SELECTS D1/D2/D3, and passing a hard 1 is why
/// those radios always landed on D1 — an IC-7610 operator with USB audio wired to D2 found the
/// radio moved back under them on every mode assert (report, 2026-08-19). `mode` is clamped to
/// 1..=3 so a bad setting can never put an undefined value on the bus.
///
/// The filter byte keeps the rig's current selection when `None`.
pub fn set_data_mode_n(radio: u8, mode: u8, filter: Option<u8>) -> Frame {
    let m = mode.clamp(1, 3);
    let fil = filter.unwrap_or(0x01);
    Frame::command(radio, 0x1A, &[0x06, m, fil])
}

/// DATA mode on/off, selecting D1 when on — the long-standing behaviour, kept for the callers
/// that have no operator preference to apply (a tune restore putting the rig back the way it
/// was found).
pub fn set_data_mode(radio: u8, on: bool, filter: Option<u8>) -> Frame {
    if !on {
        let fil = filter.unwrap_or(0x00);
        return Frame::command(radio, 0x1A, &[0x06, 0x00, fil]);
    }
    set_data_mode_n(radio, 1, filter)
}
/// Read the DATA mode state (cmd `1A 06`).
pub fn read_data_mode(radio: u8) -> Frame {
    Frame::command(radio, 0x1A, &[0x06])
}
/// Extract the DATA-mode state from a `1A 06` reply.
pub fn parse_data_mode(f: &Frame) -> Option<bool> {
    if f.cmd == 0x1A && f.data.first() == Some(&0x06) {
        f.data.get(1).map(|&b| b != 0)
    } else {
        None
    }
}

// ---- BAND-DIRECTED commands (`29`) — name the Main or Sub receiver without selecting it ----

/// Icom's BAND-DIRECTED command: `29 <band> <command…>` performs `<command…>` on the named
/// receiver — "Regardless of active/inactive the Main or Sub band, you can directly specify
/// the Main or Sub band, and send/read the supported command settings" (IC-7610 CI-V Reference
/// Guide, Icom A7380-7EX-4, Sep. 2025, p. 9). The selection never moves, so the front panel
/// never flickers and the operator's choice of band is left exactly where they put it.
///
/// Format (same guide, p. 15, "Setting after directly specify the Main/Sub band"): the band
/// byte, then the supported command as it would otherwise be sent. "When you receive the OK
/// code (FB), or the NG code (FA), the Command 29 and Main/Sub specify (00 or 01) is
/// omitted" — an ack comes back bare; a data reply carries the prefix.
///
/// ⛔ PER MODEL, FROM THE VENDOR ONLY — see [`band_directed_form`]. The IC-9700 does NOT have
/// this command, whatever a wrapper's VFO list suggests: its own CI-V Reference Guide
/// (A7508-3EX-4, Mar. 2023) lists commands through `28` and stops.
pub const BAND_DIRECTED: u8 = 0x29;
/// The band byte of a [`BAND_DIRECTED`] command: `00` = MAIN, `01` = SUB (A7380-7EX-4 p. 9).
pub const BAND_MAIN: u8 = 0x00;
pub const BAND_SUB: u8 = 0x01;

/// Does Icom's own CI-V reference give `model` the band-directed form ([`BAND_DIRECTED`])?
///
/// | Model | Answer | Source |
/// |---|---|---|
/// | IC-7610 | yes | CI-V Reference Guide A7380-7EX-4 (Sep. 2025), p. 9 table row `29`, p. 15 format |
/// | IC-9700 | **no** | CI-V Reference Guide A7508-3EX-4 (Mar. 2023): the table ends at `28` (p. 12) |
/// | IC-7300 / IC-705 / IC-905 | no | not offered a Sub receiver at all ([`crate::dualrx`]); nothing to name |
///
/// For the record, since the programme's radio list names them: the IC-910H (Instruction
/// Manual, "CONTROL COMMAND", pp. 78–79: commands `00`–`1C`) and the IC-9100 (Instruction
/// Manual, ch. 18 "Command table", pp. 184–190: `00`–`20`) have no `29` either — and neither
/// is driven by this native daemon (`rigmodels::icom_scope_model`); both run on Hamlib.
pub fn band_directed_form(model: IcomModel) -> bool {
    matches!(model, IcomModel::Ic7610)
}

/// The IC-7610 commands Icom marks "Command 29 supported" (A7380-7EX-4 pp. 3–4 and 8), as
/// `(command, sub-command)`; `None` = a command with no sub-command, whose data byte is the
/// value itself (the attenuator `11`).
///
/// ⚠️ THE WHOLE MARKED SET, not only what the broker sends today, so the next verb routed
/// through here is checked against the vendor's table rather than against a subset of it. One
/// marked row is left out on purpose: the bare `07` ("Select the VFO mode", VFO as opposed to
/// memory) — none of `07`'s sub-command rows (`B0`, `D0`, `D1`…) carries the mark, and the
/// broker never sends the bare form.
///
/// NOT marked, so ALWAYS sent bare: PTT `1C`, CW `17`, RIT/ΔTX `21`, split `0F`, the
/// transmit-side levels `14 09/0A/0B/0C/0E/0F/14/15/16/17/19`, the transmit meters
/// `15 11`–`16`, and `16 44/45/46/47/50/58/5E/66/67` (compressor, monitor, VOX, break-in,
/// dial lock, TX bandwidth, Main/Sub tracking, TX inhibit, DPD). Every one of those is the
/// one transmitter's or the one radio's — the table agrees with the architecture.
const IC7610_BAND_DIRECTED: &[(u8, Option<u8>)] = &[
    (ATT_CMD, None), // attenuator
    (0x12, Some(0x00)),
    (0x12, Some(0x01)), // RX antenna ANT1/ANT2
    (0x14, Some(LVL_AF)),
    (0x14, Some(LVL_RF)),
    (0x14, Some(LVL_SQL)),
    (0x14, Some(0x05)), // APF position
    (0x14, Some(LVL_NR)),
    (0x14, Some(0x07)),
    (0x14, Some(0x08)), // twin PBT inner/outer
    (0x14, Some(0x0D)), // manual-notch position
    (0x14, Some(LVL_NB)),
    (0x14, Some(0x13)), // DIGI-SEL shift
    (0x15, Some(0x01)), // noise / S-meter squelch status
    (0x15, Some(0x02)), // S-meter
    (0x15, Some(0x05)), // tone-squelch status
    (0x15, Some(0x07)), // overflow
    (0x16, Some(FUNC_PREAMP)),
    (0x16, Some(0x12)), // AGC
    (0x16, Some(0x22)), // NB
    (0x16, Some(0x32)), // APF
    (0x16, Some(0x40)), // NR
    (0x16, Some(0x41)), // auto notch
    (0x16, Some(0x42)), // repeater tone
    (0x16, Some(0x43)), // tone squelch
    (0x16, Some(0x48)), // manual notch
    (0x16, Some(0x4E)), // DIGI-SEL
    (0x16, Some(0x4F)), // twin peak filter
    (0x16, Some(0x53)), // ANT-RX I/O
    (0x16, Some(0x56)), // DSP IF filter type
    (0x16, Some(0x57)), // manual-notch width
    (0x16, Some(0x65)), // IP+
    (0x1A, Some(0x03)), // IF filter width
    (0x1A, Some(0x04)), // AGC time constant
    (0x1A, Some(0x09)), // AF mute
    (0x1A, Some(0x0A)), // OVF indicator
    (0x1B, Some(0x00)), // repeater tone frequency
    (0x1B, Some(0x01)), // TSQL tone frequency
];

/// May `f` (a plain command for this radio) be sent in band-directed form on `model`? True
/// only where the model has the form ([`band_directed_form`]) AND Icom marks this command.
pub fn band_directed_supported(model: IcomModel, f: &Frame) -> bool {
    band_directed_form(model)
        && IC7610_BAND_DIRECTED
            .iter()
            .any(|&(cmd, sub)| f.cmd == cmd && (sub.is_none() || f.data.first().copied() == sub))
}

/// `f`, performed on `band` ([`BAND_MAIN`] / [`BAND_SUB`]): `29 <band> <cmd> <data…>`, to the
/// same radio. The caller has already checked [`band_directed_supported`].
pub fn band_directed(band: u8, f: &Frame) -> Frame {
    let mut data = Vec::with_capacity(2 + f.data.len());
    data.push(band);
    data.push(f.cmd);
    data.extend_from_slice(&f.data);
    Frame::command(f.to, BAND_DIRECTED, &data)
}

/// The reply to a band-directed READ with its `29 <band>` prefix taken off, so the ordinary
/// decoders read it unchanged. A reply that came back without the prefix is returned as it
/// arrived — see `engine::Expect::ReplyOnBand` for why both shapes are taken.
pub fn band_directed_reply(f: Frame) -> Frame {
    if f.cmd == BAND_DIRECTED && f.data.len() >= 2 {
        Frame {
            to: f.to,
            from: f.from,
            cmd: f.data[1],
            data: f.data[2..].to_vec(),
        }
    } else {
        f
    }
}

/// Does `model` read and write a band's DIAL and MODE by name — `25 <band>` / `26 <band>`,
/// where the band byte is `00` = MAIN and `01` = SUB whichever band the operator has selected?
///
/// | Model | Answer | Source |
/// |---|---|---|
/// | IC-7610 | yes | A7380-7EX-4 p. 13: "Main or Sub band's frequency settings" (`25`) and "Main or Sub band's operating mode and filter settings" (`26`), each `00: MAIN`, `01: SUB`; both "Send/read" (p. 9) |
/// | IC-9700 | **no** | A7508-3EX-4 p. 24: its `25`/`26` name the SELECTED or UNSELECTED VFO, so `25 00` follows the selection — the very thing this form exists to escape |
/// | IC-7300 / IC-705 / IC-905 | no | not offered a Sub receiver at all ([`crate::dualrx`]); nothing to name |
///
/// Neither command carries the band-directed mark (A7380-7EX-4 p. 9): the band is their OWN
/// first data byte. That is why this is a table of its own rather than a row of
/// [`band_directed_supported`] — `29 00 03` is not a command the radio lists.
pub fn dial_by_band_name(model: IcomModel) -> bool {
    matches!(model, IcomModel::Ic7610)
}

/// Read `band`'s dial by name (`25 <band>`, [`BAND_MAIN`] / [`BAND_SUB`]). The reply is
/// `25 <band>` and the 5-byte BCD frequency — see [`parse_band_freq`]. Only where
/// [`dial_by_band_name`]: on an IC-9700 the same bytes read the SELECTED VFO.
pub fn read_band_freq(radio: u8, band: u8) -> Frame {
    Frame::command(radio, 0x25, &[band])
}
/// Read `band`'s mode by name (`26 <band>`). The reply is `26 <band> <mode> <data> <filter>`
/// — see [`parse_band_mode`]. Same model rule as [`read_band_freq`].
pub fn read_band_mode(radio: u8, band: u8) -> Frame {
    Frame::command(radio, 0x26, &[band])
}
/// Set `band`'s dial by name: `25 <band>` and the 5-byte BCD frequency ([`set_freq`]'s
/// payload). Same model rule as [`read_band_freq`].
pub fn set_band_freq(radio: u8, band: u8, hz: u64) -> Frame {
    let mut data = vec![band];
    data.extend_from_slice(&freq_to_bcd(hz));
    Frame::command(radio, 0x25, &data)
}
/// Set `band`'s mode by name, in ONE frame that leaves the radio where [`set_mode`] followed by
/// the `1A 06` DATA write left it (A7380-7EX-4 pp. 10, 12, 13):
///
/// - `data` `None` → `26 <band> <mode>`. The DATA and filter bytes are skipped, which the
///   radio takes as "DATA OFF and the default filter setting of the operating mode" — what
///   `06 <mode>` (the mode's default filter) and then `1A 06 00 00` (DATA off) left.
/// - `data` `Some(n)` → `26 <band> <mode> <n> 01`: DATA mode `n` (clamped to D1–D3, as
///   [`set_data_mode_n`] clamps it) with FIL1, the two bytes `1A 06 <n> 01` sent.
pub fn set_band_mode(radio: u8, band: u8, mode: Mode, data: Option<u8>) -> Frame {
    match data {
        Some(n) => Frame::command(radio, 0x26, &[band, mode.to_byte(), n.clamp(1, 3), 0x01]),
        None => Frame::command(radio, 0x26, &[band, mode.to_byte()]),
    }
}

// ---- scope CONTROL (command 0x27 sub 14/15/19) — set the RIG's panadapter, not the stream ----
// Byte layouts verified against Hamlib (rigs/icom/icom.c). On dual-receiver rigs (IC-9700/7610)
// each of these takes a leading Main/Sub selector byte (`main_sub` = Some(0x00) for Main); single-
// scope rigs pass None and omit it. The caller (CivDaemon) supplies main_sub from scope_is_dual.

fn scope_ctrl_frame(radio: u8, sub: u8, main_sub: Option<u8>, payload: &[u8]) -> Frame {
    let mut data = Vec::with_capacity(2 + payload.len());
    data.push(sub);
    if let Some(ms) = main_sub {
        data.push(ms);
    }
    data.extend_from_slice(payload);
    Frame::command(radio, 0x27, &data)
}

/// Scope SPAN in center mode (`27 15`): the ± half-width in Hz as 5-byte little-endian BCD (the
/// frequency codec). Table values: 2500/5000/10000/25000/50000/100000/250000/500000.
pub fn set_scope_span(radio: u8, main_sub: Option<u8>, span_hz: u32) -> Frame {
    scope_ctrl_frame(radio, 0x15, main_sub, &freq_to_bcd(u64::from(span_hz)))
}

/// Scope REFERENCE level (`27 19`): magnitude as 2-byte big-endian BCD in 0.01 dB units (rounded
/// to 0.5 dB steps), then a trailing sign byte (00 = +, 01 = −). Range −20.0..+20.0 dB.
pub fn set_scope_ref(radio: u8, main_sub: Option<u8>, ref_tenths_db: i32) -> Frame {
    let tenths = ref_tenths_db.clamp(-200, 200);
    // Round to the nearest 0.5 dB (5 tenths), then to 0.01-dB hundredths for the wire.
    let hundredths = ((tenths.abs() + 2) / 5 * 5 * 10) as u16;
    let [hi, lo] = level_to_bcd2(hundredths);
    scope_ctrl_frame(radio, 0x19, main_sub, &[hi, lo, u8::from(tenths < 0)])
}

/// Scope CENTER/FIXED mode (`27 14`): one byte, 00 = center, 01 = fixed.
pub fn set_scope_center_mode(radio: u8, main_sub: Option<u8>, fixed: bool) -> Frame {
    scope_ctrl_frame(radio, 0x14, main_sub, &[u8::from(fixed)])
}

/// READ the scope CENTER/FIXED mode (`27 14` with no payload). The rig answers with the same
/// sub-command and a trailing mode byte.
///
/// ⚠️ THIS EXISTS SO NEXUS STOPS GUESSING WHY A SPAN WAS REFUSED. `set_scope_span` was reporting
/// every `NG` as "your scope is not in Center mode", which is only the MOST LIKELY cause and not a
/// fact — an operator whose scope WAS in Center (issue report 2026-09-18, IC-7300, photo showed
/// CENTER lit) was sent to check a setting that was already correct, and the real cause stayed
/// hidden behind our guess. A refusal is a fact about the radio; the reason for it is not, unless
/// we ask.
pub fn read_scope_center_mode(radio: u8, main_sub: Option<u8>) -> Frame {
    scope_ctrl_frame(radio, 0x14, main_sub, &[])
}

/// The mode byte from a `27 14` reply: `Some(true)` = fixed, `Some(false)` = center, `None` when
/// the frame is not that reply or carries no mode byte.
///
/// The byte is LAST rather than at a fixed index, because a dual-receiver rig puts a Main/Sub
/// selector between the sub-command and the payload and a single-receiver one does not.
pub fn parse_scope_center_mode(f: &Frame) -> Option<bool> {
    (f.cmd == 0x27 && f.data.first() == Some(&0x14) && f.data.len() >= 2)
        .then(|| f.data.last().is_some_and(|b| *b != 0))
}

// ---- reply decoders ----

/// Extract the frequency (Hz) from a `03` frequency report (or an unsolicited transceive
/// `00` report, which shares the 5-byte-BCD payload).
pub fn parse_freq(f: &Frame) -> Option<u64> {
    if (f.cmd == 0x03 || f.cmd == 0x00) && f.data.len() >= 5 {
        Some(bcd_to_freq(&f.data[..5]))
    } else {
        None
    }
}
/// Extract `(mode, filter)` from a `04` mode report (or transceive `01`).
pub fn parse_mode(f: &Frame) -> Option<(Mode, Option<u8>)> {
    if (f.cmd == 0x04 || f.cmd == 0x01) && !f.data.is_empty() {
        let mode = Mode::from_byte(f.data[0])?;
        Some((mode, f.data.get(1).copied()))
    } else {
        None
    }
}
/// The frequency (Hz) from a [`read_band_freq`] reply: `25 <band>` + 5-byte BCD. `None` for a
/// reply naming the OTHER band — the Sub's dial must never be read as Main's.
pub fn parse_band_freq(f: &Frame, band: u8) -> Option<u64> {
    (f.cmd == 0x25 && f.data.first() == Some(&band) && f.data.len() >= 6)
        .then(|| bcd_to_freq(&f.data[1..6]))
}
/// `(mode, filter)` from a [`read_band_mode`] reply: `26 <band> <mode> <data> <filter>`
/// (A7380-7EX-4 p. 13). `None` for a reply naming the other band.
///
/// ⚠️ THE DATA BYTE (`00` off, `01`–`03` = D1–D3) IS READ PAST, ON PURPOSE. The mode name the
/// broker reports is built exactly as it was from a `04` read — the plain mode, PKT only from a
/// received `1A 06` — and the cockpit compares that name against the mode Nexus commanded to
/// flag a mismatch. Starting to read the flag here would change that name in every data mode,
/// which is a different change from reading it off the right receiver.
pub fn parse_band_mode(f: &Frame, band: u8) -> Option<(Mode, Option<u8>)> {
    if f.cmd == 0x26 && f.data.first() == Some(&band) && f.data.len() >= 2 {
        let mode = Mode::from_byte(f.data[1])?;
        Some((mode, f.data.get(3).copied()))
    } else {
        None
    }
}
/// Extract PTT state from a `1C 00` reply (`true` = transmitting).
pub fn parse_ptt(f: &Frame) -> Option<bool> {
    if f.cmd == 0x1C && f.data.first() == Some(&0x00) {
        f.data.get(1).map(|&b| b != 0)
    } else {
        None
    }
}
/// Extract the raw S-meter level (0–255) from a `15 02` reply.
pub fn parse_smeter_raw(f: &Frame) -> Option<u16> {
    if f.cmd == 0x15 && f.data.first() == Some(&0x02) && f.data.len() >= 3 {
        Some(level_from_bcd2(f.data[1], f.data[2]))
    } else {
        None
    }
}
/// Extract the raw RF-power level (0–255) from a `14 0A` reply.
pub fn parse_rf_power_raw(f: &Frame) -> Option<u16> {
    if f.cmd == 0x14 && f.data.first() == Some(&0x0A) && f.data.len() >= 3 {
        Some(level_from_bcd2(f.data[1], f.data[2]))
    } else {
        None
    }
}

// ---- transmit meters (CI-V command 0x15 read family) ----
// Sub-commands: Po=0x11, SWR=0x12, ALC=0x13, COMP=0x14. Each reply is a 2-byte BCD raw
// value 0–255 (same codec as the S-meter). The raw→engineering-unit conversions below are
// the EXACT IC-9700 breakpoint tables Hamlib ships (rigs/icom/ic7300.c, the IC-9700 caps),
// linearly interpolated and clamped at the ends — so Nexus matches the reference reading.

/// Po (RF power) meter sub-command.
pub const METER_PO: u8 = 0x11;
/// SWR meter sub-command.
pub const METER_SWR: u8 = 0x12;
/// ALC meter sub-command.
pub const METER_ALC: u8 = 0x13;
/// Speech-compression meter sub-command.
pub const METER_COMP: u8 = 0x14;

/// Read a `15 <sub>` transmit meter.
pub fn read_meter(radio: u8, sub: u8) -> Frame {
    Frame::command(radio, 0x15, &[sub])
}
/// Extract the raw meter level (0–255) from a `15 <sub>` reply.
pub fn parse_meter_raw(f: &Frame, sub: u8) -> Option<u16> {
    if f.cmd == 0x15 && f.data.first() == Some(&sub) && f.data.len() >= 3 {
        Some(level_from_bcd2(f.data[1], f.data[2]))
    } else {
        None
    }
}

/// Linear interpolation over a raw→value breakpoint table, clamped at both ends.
fn interp_cal(raw: u16, table: &[(u16, f32)]) -> f32 {
    let x = f32::from(raw);
    if x <= f32::from(table[0].0) {
        return table[0].1;
    }
    for w in table.windows(2) {
        let (x0, y0) = (f32::from(w[0].0), w[0].1);
        let (x1, y1) = (f32::from(w[1].0), w[1].1);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    table[table.len() - 1].1
}

// IC-9700 calibration tables (Hamlib rigs/icom/ic7300.c: IC9700_*_CAL).
//
// ⚠️ THESE ARE ICOM CURVES AND THEY ARE APPLIED TO EVERY CI-V RIG. A Xiegu speaks CI-V but is
// not an Icom, and its meters use their own scale, so the converted figure is not the rig's.
// A G90 reading 1.2:1 on its own meter has been reported as 6:1 here - raw values land at or
// past the last SWR_CAL entry, and interp_cal clamps there, so the display pins at 6.0 rather
// than tracking anything. Documented for operators in docs/rigs/xiegu.md.
//
// Fixing it needs a bench capture of what each non-Icom model returns at a known SWR; building
// a second curve by guesswork would just be a different wrong answer. This is the
// vendor-spec-not-the-wrapper trap: these numbers came from Hamlib's reading of the protocol,
// which is not the same thing as the protocol.
const SWR_CAL: &[(u16, f32)] = &[(0, 1.0), (48, 1.5), (80, 2.0), (120, 3.0), (240, 6.0)];
const ALC_CAL: &[(u16, f32)] = &[(0, 0.0), (120, 1.0)];
const PO_WATTS_CAL: &[(u16, f32)] = &[
    (0, 0.0),
    (21, 5.0),
    (43, 10.0),
    (65, 15.0),
    (83, 20.0),
    (95, 25.0),
    (105, 30.0),
    (114, 35.0),
    (124, 40.0),
    (143, 50.0),
    (183, 75.0),
    (213, 100.0),
    (255, 120.0),
];
// IC-9700-specific (the IC-7300 tops out at 241→30 dB; the 9700 at 210→25.5 dB).
const COMP_DB_CAL: &[(u16, f32)] = &[(0, 0.0), (130, 15.0), (210, 25.5)];

/// SWR ratio (1.0–6.0) from the raw SWR meter.
pub fn swr_from_raw(raw: u16) -> f32 {
    interp_cal(raw, SWR_CAL)
}
/// ALC as a 0.0–1.0 fraction (full-scale "in the zone" edge = raw 120).
pub fn alc_frac_from_raw(raw: u16) -> f32 {
    interp_cal(raw, ALC_CAL)
}
/// Transmit power in WATTS from the Po meter (per the rig's own scale; the 9700 divides by
/// 10 above 1 GHz, which this table does not model — the low band is the common case).
pub fn po_watts_from_raw(raw: u16) -> f32 {
    interp_cal(raw, PO_WATTS_CAL)
}
/// Speech compression in dB from the COMP meter.
pub fn comp_db_from_raw(raw: u16) -> f32 {
    interp_cal(raw, COMP_DB_CAL)
}

/// Convert a raw Icom S-meter reading (0–255) to dB relative to S9, using Icom's nominal
/// scale (S9 ≈ raw 120; each S-unit ≈ 6 dB below S9; raw 120→241 ≈ 0→+60 dB over S9).
/// Approximate and per-rig-calibratable, but consistent and monotonic.
pub fn smeter_db_rel_s9(raw: u16) -> f32 {
    let raw = raw.min(255) as f32;
    if raw <= 120.0 {
        raw / 120.0 * 54.0 - 54.0 // S0 = -54 dB … S9 = 0 dB
    } else {
        (raw - 120.0) / (241.0 - 120.0) * 60.0 // S9 … S9+60
    }
}

// ---- 2-byte big-endian BCD level codec (S-meter, RF power: 0000–0255) ----

/// Encode a 0–255 level as Icom's 2-byte, big-endian, 4-digit BCD (e.g. 128 → `[0x01,0x28]`).
fn level_to_bcd2(v: u16) -> [u8; 2] {
    let v = v.min(9999);
    let hi = (v / 100) as u8; // 0–25 (two BCD digits: thousands+hundreds)
    let lo = (v % 100) as u8; // 0–99 (tens+ones)
    [to_bcd(hi), to_bcd(lo)]
}
/// Decode Icom's 2-byte big-endian BCD level back to 0–255.
fn level_from_bcd2(hi: u8, lo: u8) -> u16 {
    u16::from(from_bcd(hi)) * 100 + u16::from(from_bcd(lo))
}
/// Two decimal digits → one BCD byte (defensive: values > 99 wrap on the 100s).
fn to_bcd(v: u8) -> u8 {
    ((v / 10 % 10) << 4) | (v % 10)
}
/// One BCD byte → two decimal digits (non-decimal nibbles clamped to 0).
fn from_bcd(b: u8) -> u8 {
    let hi = b >> 4;
    let lo = b & 0x0F;
    (if hi > 9 { 0 } else { hi }) * 10 + (if lo > 9 { 0 } else { lo })
}

#[cfg(test)]
mod tests {

    /// THE BYTE THAT SENT AN IC-7610 TO D1 FOREVER.
    ///
    /// `1A 06`'s first data byte is `00` for off and, on the multi-data-mode Icoms, WHICH data
    /// mode for on. Nexus sent a hard `1`, so an operator with USB audio wired to D2 had the
    /// radio moved back under them on every mode assert (report, 2026-08-19).
    #[test]
    fn data_mode_selects_the_operators_d_number() {
        // D1 is what the old call produced — the byte is identical, so nothing moves for an
        // operator who never touches the setting.
        assert_eq!(
            set_data_mode_n(0x98, 1, None).data,
            set_data_mode(0x98, true, None).data,
            "D1 must be byte-identical to the long-standing behaviour"
        );
        assert_eq!(set_data_mode_n(0x98, 2, None).data, vec![0x06, 0x02, 0x01]);
        assert_eq!(set_data_mode_n(0x98, 3, None).data, vec![0x06, 0x03, 0x01]);
        // Off is off, whatever number is configured.
        assert_eq!(
            set_data_mode(0x98, false, None).data,
            vec![0x06, 0x00, 0x00]
        );
        // A nonsense setting can never reach the bus: 0 and 9 both clamp into range.
        assert_eq!(set_data_mode_n(0x98, 0, None).data[1], 0x01);
        assert_eq!(set_data_mode_n(0x98, 9, None).data[1], 0x03);
        // The filter byte is still honoured when the caller has one.
        assert_eq!(
            set_data_mode_n(0x98, 2, Some(0x02)).data,
            vec![0x06, 0x02, 0x02]
        );
    }
    use super::*;
    use crate::civ::frame::bcd_to_freq;

    #[test]
    fn model_addresses_and_name_detection() {
        assert_eq!(IcomModel::Ic9700.default_civ_addr(), 0xA2);
        assert_eq!(IcomModel::Ic7300.default_civ_addr(), 0x94);
        assert_eq!(
            IcomModel::from_name("Icom IC-9700"),
            Some(IcomModel::Ic9700)
        );
        assert_eq!(IcomModel::from_name("ic7300"), Some(IcomModel::Ic7300));
        assert_eq!(IcomModel::from_name("Yaesu FTDX10"), None);
    }

    /// The Hamlib number is how this daemon asks the capability table whether a model has a
    /// Sub; a drift from the catalogue's own mapping would silently route an IC-7610 as a
    /// single-receiver rig, or an IC-7300 as a dual one.
    #[test]
    fn the_hamlib_model_number_round_trips_the_catalogue_mapping() {
        for m in [
            IcomModel::Ic7300,
            IcomModel::Ic7610,
            IcomModel::Ic9700,
            IcomModel::Ic705,
            IcomModel::Ic905,
        ] {
            assert_eq!(
                crate::rigmodels::icom_scope_model(m.hamlib_model()),
                Some(m),
                "{m:?}"
            );
        }
    }

    /// ⭐ COMMAND `29`, AS ICOM PRINTS IT — and only where Icom prints it.
    ///
    /// IC-7610 CI-V Reference Guide A7380-7EX-4: p. 9 (the row), p. 15 (the format), and the
    /// "Command 29 supported" marks on pp. 3–4 and 8. IC-9700 CI-V Reference Guide
    /// A7508-3EX-4: no command `29` at all.
    #[test]
    fn the_band_directed_form_is_the_7610s_and_only_for_the_commands_icom_marks() {
        // The frame: `29 <band> <cmd> <data…>`, to the same radio.
        let af_set = set_dsp_level(0x98, LVL_AF, 50);
        let wrapped = band_directed(BAND_SUB, &af_set);
        assert_eq!(wrapped.to, 0x98);
        assert_eq!(wrapped.cmd, 0x29);
        assert_eq!(wrapped.data, vec![0x01, 0x14, 0x01, 0x01, 0x27]);
        assert_eq!(
            wrapped.to_bytes(),
            vec![0xFE, 0xFE, 0x98, 0xE0, 0x29, 0x01, 0x14, 0x01, 0x01, 0x27, 0xFD]
        );
        // A prefixed reply unwraps to the frame the ordinary decoders read; a bare one is
        // passed through as it arrived.
        let reply = Frame::parse(&[
            0xFE, 0xFE, 0xE0, 0x98, 0x29, 0x00, 0x15, 0x02, 0x01, 0x20, 0xFD,
        ])
        .unwrap();
        let inner = band_directed_reply(reply);
        assert_eq!(
            (inner.cmd, inner.data.clone()),
            (0x15, vec![0x02, 0x01, 0x20])
        );
        assert_eq!(parse_smeter_raw(&inner), Some(120));
        let bare = Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x15, 0x02, 0x01, 0x20, 0xFD]).unwrap();
        assert_eq!(band_directed_reply(bare.clone()), bare);

        // Marked on the IC-7610: the receive chain and the CTCSS tone.
        let marked = [
            read_smeter(0x98),
            read_dsp_level(0x98, LVL_AF),
            set_dsp_level(0x98, LVL_NB, 10),
            read_attenuator(0x98),
            set_attenuator_db(0x98, 12),
            read_preamp(0x98),
            read_agc(0x98),
            set_dsp_func(0x98, 0x22, true), // NB
            set_dsp_func(0x98, 0x48, true), // MN
            set_repeater_tone(0x98, 885),
            set_tone_func(0x98, true),
        ];
        for f in &marked {
            assert!(
                band_directed_supported(IcomModel::Ic7610, f),
                "{:02x} {:02x?} is marked on the 7610",
                f.cmd,
                f.data
            );
        }
        // NOT marked: the transmitter's and the radio's — and the keying path above all.
        let unmarked = [
            set_ptt(0x98, true),
            read_ptt(0x98),
            send_morse(0x98, "CQ"),
            stop_morse(0x98),
            set_rit_offset(0x98, 100),
            set_dtx_on(0x98, true),
            set_rf_power(0x98, 50),
            set_mic_gain(0x98, 50),
            set_dsp_func(0x98, 0x44, true), // speech compressor
            set_dsp_func(0x98, 0x46, true), // VOX
            read_meter(0x98, METER_SWR),
            set_split(0x98, true),
            select_vfo(0x98, "SUB").unwrap(),
            set_freq(0x98, 14_074_000),
        ];
        for f in &unmarked {
            assert!(
                !band_directed_supported(IcomModel::Ic7610, f),
                "{:02x} {:02x?} is not marked on the 7610",
                f.cmd,
                f.data
            );
        }
        // ⛔ THE IC-9700 HAS NO `29` — not for anything, whatever a wrapper suggests. And the
        // single-receiver rigs have nothing to name.
        for m in [
            IcomModel::Ic9700,
            IcomModel::Ic7300,
            IcomModel::Ic705,
            IcomModel::Ic905,
        ] {
            assert!(!band_directed_form(m), "{m:?}");
            assert!(!band_directed_supported(m, &read_smeter(0xA2)), "{m:?}");
        }
        assert!(band_directed_form(IcomModel::Ic7610));
    }

    #[test]
    fn set_freq_encodes_cmd_05_with_bcd() {
        let f = set_freq(0xA2, 145_000_000);
        assert_eq!(f.cmd, 0x05);
        assert_eq!(f.to, 0xA2);
        assert_eq!(bcd_to_freq(&f.data), 145_000_000);
    }

    #[test]
    fn dsp_func_frames_round_trip() {
        // Token → 0x16 sub-command mapping is the single source both get + set use.
        assert_eq!(func_sub("COMP"), Some(0x44));
        assert_eq!(func_sub("VOX"), Some(0x46));
        assert_eq!(func_sub("NB"), Some(0x22));
        // Satellite mode is Hamlib's RIG_FUNC_SATMODE token, so a Hamlib-served
        // 9700 answers the identical `U SATMODE 1` line. Engage = 16 5A 01.
        assert_eq!(func_sub("SATMODE"), Some(FUNC_SATMODE));
        assert_eq!(
            set_dsp_func(0xA2, FUNC_SATMODE, true).data,
            vec![0x5A, 0x01]
        );
        assert_eq!(func_sub("RIT"), None); // RIT is a separate register, not a 0x16 func
                                           // Set builds `16 <sub> <on>`.
        let on = set_dsp_func(0xA2, 0x44, true);
        assert_eq!(on.cmd, 0x16);
        assert_eq!(on.data, vec![0x44, 0x01]);
        // A reply `16 44 01` decodes to on=true, and a mismatched sub is rejected.
        let reply = Frame::parse(&[0xFE, 0xFE, 0xE0, 0xA2, 0x16, 0x44, 0x01, 0xFD]).unwrap();
        assert_eq!(parse_dsp_func(&reply, 0x44), Some(true));
        assert_eq!(parse_dsp_func(&reply, 0x46), None); // wrong sub-command
    }

    #[test]
    fn mic_gain_scales_percent_onto_0_255() {
        // 14 0B, percent → 0..255 BCD, mirroring set_rf_power / keyer-speed.
        let f = set_mic_gain(0xA2, 100);
        assert_eq!(f.cmd, 0x14);
        assert_eq!(f.data[0], 0x0B);
        assert_eq!(level_from_bcd2(f.data[1], f.data[2]), 255);
        assert_eq!(
            level_from_bcd2(set_mic_gain(0xA2, 0).data[1], set_mic_gain(0xA2, 0).data[2]),
            0
        );
    }

    #[test]
    fn scope_span_frame_matches_icom_reference() {
        // Hamlib ref: set Main scope span 25 kHz on an IC-9700 → 27 15 00 <25000 LE BCD>.
        // Dual-scope rig gets the leading Main byte (00); single-scope omits it.
        let dual = set_scope_span(0xA2, Some(0x00), 25_000);
        assert_eq!(dual.cmd, 0x27);
        assert_eq!(dual.data, vec![0x15, 0x00, 0x00, 0x50, 0x02, 0x00, 0x00]);
        let single = set_scope_span(0x94, None, 25_000);
        assert_eq!(single.data, vec![0x15, 0x00, 0x50, 0x02, 0x00, 0x00]); // no Main/Sub byte
    }

    #[test]
    fn scope_ref_encodes_level_then_sign() {
        // 27 19: [scope, 2-byte BE BCD magnitude in 0.01 dB, sign(00=+/01=-)].
        // +10.0 dB → 1000 → 10 00, sign 00.
        assert_eq!(
            set_scope_ref(0xA2, Some(0x00), 100).data,
            vec![0x19, 0x00, 0x10, 0x00, 0x00]
        );
        // −20.0 dB → 2000 → 20 00, sign 01. (Clamped range.)
        assert_eq!(
            set_scope_ref(0xA2, Some(0x00), -200).data,
            vec![0x19, 0x00, 0x20, 0x00, 0x01]
        );
        // +0.5 dB (5 tenths) → 50 → 00 50, sign 00.
        assert_eq!(
            set_scope_ref(0x94, None, 5).data,
            vec![0x19, 0x00, 0x50, 0x00]
        );
    }

    #[test]
    fn scope_center_mode_is_one_byte() {
        assert_eq!(
            set_scope_center_mode(0xA2, Some(0x00), true).data,
            vec![0x14, 0x00, 0x01]
        );
        assert_eq!(
            set_scope_center_mode(0x94, None, false).data,
            vec![0x14, 0x00]
        );
    }

    #[test]
    fn dsp_level_and_agc_frames() {
        // NR level 14 06, 50% → 127 → BE BCD 01 27.
        let nr = set_dsp_level(0xA2, LVL_NR, 50);
        assert_eq!(nr.cmd, 0x14);
        assert_eq!(nr.data[0], 0x06);
        assert_eq!(level_from_bcd2(nr.data[1], nr.data[2]), 127);
        // AGC 16 12: FAST byte is 0x01 on this family; Hamlib FAST=2.
        assert_eq!(set_agc(0xA2, 0x01).data, vec![0x12, 0x01]);
        assert_eq!(agc_civ_from_hamlib(2), 0x01); // FAST
        assert_eq!(agc_civ_from_hamlib(5), 0x02); // MEDIUM → MID
        assert_eq!(agc_civ_from_hamlib(3), 0x03); // SLOW
        assert_eq!(agc_hamlib_from_civ(0x02), 5); // MID → MEDIUM
        assert_eq!(agc_hamlib_from_civ(0x03), 3); // SLOW
    }

    /// THE ANALOG LEVELS REACH THE RIGHT REGISTER — and, just as importantly, not each
    /// other's. A transposed sub-command would move a control the operator never touched:
    /// `L AF 0` landing on `14 02` turns the receiver down instead of the speaker, and
    /// landing on `14 03` closes the squelch and goes deaf.
    ///
    /// ⚠️ EVERY VALUE BELOW DIFFERS. Driving all three with one number is inert — a table
    /// with AF and RF swapped produces byte-identical frames and passes. Distinct percentages
    /// make the payload identify which level it came from, so a swap shows up in both the sub
    /// and the value.
    #[test]
    fn the_analog_level_subs_are_the_bytes_hamlibs_icom_backend_puts_on_the_wire() {
        // Captured from Hamlib 4.5.5 `rigctl -m 3073` against a pty; see `LVL_AF`'s doc.
        for (token, sub, percent, bcd) in [
            ("AF", 0x01u8, 20u8, 51u16),
            ("RF", 0x02, 60, 153),
            ("SQL", 0x03, 90, 229),
        ] {
            assert_eq!(level_sub(token), Some(sub), "{token} sub-command");
            let f = set_dsp_level(0xA2, level_sub(token).unwrap(), percent);
            assert_eq!(f.cmd, 0x14);
            assert_eq!(
                f.data[0], sub,
                "{token} must not ride another level's register"
            );
            assert_eq!(
                level_from_bcd2(f.data[1], f.data[2]),
                bcd,
                "{token} payload"
            );
            assert_eq!(
                read_dsp_level(0xA2, sub).data,
                vec![sub],
                "{token} read frame"
            );
        }
        // The two in-repo constants the same wire capture reproduced — the positive control
        // that the captured table is the real Icom one and not this file talking to itself.
        assert_eq!(level_sub("NR"), Some(0x06));
        assert_eq!(level_sub("NB"), Some(0x12));
        // RFPOWER and MICGAIN are the same CI-V family but have their own builders, and the
        // broker answers them before it ever reaches this table.
        assert_eq!(level_sub("RFPOWER"), None);
        assert_eq!(level_sub("MICGAIN"), None);
        assert_eq!(level_sub("STRENGTH"), None);
        assert_eq!(level_sub(""), None);
    }

    /// THE MANUAL NOTCH REACHES THE RIG. `MN` is the notch an operator parks on a
    /// heterodyne by ear; `ANF` is the automatic one that hunts a carrier by itself. Two
    /// controls, two registers — and this table held only `ANF`, so on the native CI-V path
    /// the MN button latched unsupported and the cockpit dropped it, indistinguishable from
    /// a feature Nexus never wrote.
    ///
    /// Captured the way the analog levels were (see [`LVL_AF`]): Hamlib 4.5.5's own Icom
    /// backend driven against a pty at the IC-7300's address — `U MN 1` → `16 48 01`,
    /// `U MN 0` → `16 48 00`, `u MN` → `16 48`.
    #[test]
    fn the_manual_notch_func_is_the_byte_hamlibs_icom_backend_puts_on_the_wire() {
        assert_eq!(func_sub("MN"), Some(0x48), "MN sub-command");
        // THE TWO NOTCHES ARE NOT ONE REGISTER. A table answering `MN` with `ANF`'s byte
        // passes every "is MN supported" check while the button drives the automatic
        // notch — the exact confusion #95 was filed about.
        assert_ne!(
            func_sub("MN"),
            func_sub("ANF"),
            "the manual notch must not ride the automatic notch's register"
        );
        assert_eq!(set_dsp_func(0xA2, 0x48, true).data, vec![0x48, 0x01]);
        assert_eq!(set_dsp_func(0xA2, 0x48, false).data, vec![0x48, 0x00]);
        assert_eq!(read_dsp_func(0xA2, 0x48).data, vec![0x48]);
        // THE POSITIVE CONTROL: the same capture reproduced four bytes this table already
        // held, which is what makes the fifth evidence rather than recollection.
        for (token, sub) in [("ANF", 0x41u8), ("NB", 0x22), ("COMP", 0x44), ("MON", 0x45)] {
            assert_eq!(func_sub(token), Some(sub), "{token} sub-command");
        }
        // `NOTCHF` is where the manual notch SITS, not whether it is on; it is a level, and
        // a deliberately absent one — see the compressor-depth test below.
        assert_eq!(func_sub("NOTCHF"), None);
        assert_eq!(func_sub("MANUAL_NOTCH"), None); // not a Hamlib token
    }

    /// THE COMPRESSOR DEPTH REACHES THE RIG — and it is NOT the compression METER.
    /// `COMP` the level is how hard the speech processor works (`14 0E`, a 0..1 fraction).
    /// `COMP_METER` is how many dB of compression the rig shows while keyed, off the `0x15`
    /// meter family, and the broker already served that one. Conflating the two is the easy
    /// mistake here: the operator got a compressor they could switch on (`16 44`) and no way
    /// to say how hard it should work, which is the half that matters.
    ///
    /// ⛔ AND THE THIRD CONTROL, `NOTCHF`, IS DELIBERATELY NOT HERE. Hamlib 4.5.5's Icom
    /// backend has no `NOTCHF` for ANY rig Nexus drives natively — the same harness that
    /// captured the bytes below shows `NOTCHF` present on Yaesu (FT-991, FTDX-10) and absent
    /// on the IC-7300/7610/9700/705, which carry `NOTCHF_RAW` (`14 0D`) instead: a raw 0..255
    /// notch POSITION, where `NOTCHF` is Hz. So native CI-V is already at parity with a
    /// Hamlib-served Icom here, and [`set_dsp_level`] is a PERCENT path: a `NOTCHF` entry in
    /// this table would clamp every frequency above 100 Hz to full scale and silently park
    /// the notch at the end of its range. Placing it in Hz needs the rig's Hz↔position curve,
    /// which is a bench measurement. NEEDS-BENCH (IC-7300).
    #[test]
    fn the_compressor_depth_level_is_the_byte_hamlibs_icom_backend_puts_on_the_wire() {
        assert_eq!(LVL_COMP, 0x0E);
        assert_eq!(level_sub("COMP"), Some(LVL_COMP), "COMP sub-command");
        // Captured from Hamlib 4.5.5 `rigctl -m 3073` against a pty: `L COMP 0.25` →
        // `14 0e 00 63`, `L COMP 0.5` → `14 0e 01 27`, `L COMP 1.0` → `14 0e 02 55`.
        // ⚠️ THREE DIFFERENT DEPTHS, so the payload identifies the register it came from.
        // One value driven through all three is inert: COMP and NR transposed would produce
        // the identical frame and pass.
        for (percent, bcd) in [(25u8, 63u16), (50, 127), (100, 255)] {
            let f = set_dsp_level(0xA2, level_sub("COMP").unwrap(), percent);
            assert_eq!(f.cmd, 0x14);
            assert_eq!(
                f.data[0], 0x0E,
                "compressor depth must not ride another level's register"
            );
            assert_eq!(
                level_from_bcd2(f.data[1], f.data[2]),
                bcd,
                "COMP {percent}%"
            );
        }
        assert_eq!(read_dsp_level(0xA2, LVL_COMP).data, vec![0x0E]);
        // The TRANSMIT METER is a different thing on a different command family, and the
        // broker answers it by name before this table is ever consulted.
        assert_eq!(level_sub("COMP_METER"), None);
        // The ruling above, made executable: neither notch-frequency token may fall into
        // the percent path.
        assert_eq!(level_sub("NOTCHF"), None);
        assert_eq!(level_sub("NOTCHF_RAW"), None);
    }

    #[test]
    fn freq_report_round_trip() {
        // Radio replies to a read: FE FE E0 A2 03 <bcd> FD.
        let reply = Frame {
            to: 0xE0,
            from: 0xA2,
            cmd: 0x03,
            data: freq_to_bcd(432_100_000).to_vec(),
        };
        assert_eq!(parse_freq(&reply), Some(432_100_000));
        // A transceive (cmd 00) report is decoded the same way.
        let xcv = Frame {
            cmd: 0x00,
            ..reply.clone()
        };
        assert_eq!(parse_freq(&xcv), Some(432_100_000));
    }

    #[test]
    fn mode_table_is_a_bijection_on_known_modes() {
        for m in [
            Mode::Lsb,
            Mode::Usb,
            Mode::Am,
            Mode::Cw,
            Mode::Rtty,
            Mode::Fm,
            Mode::CwR,
            Mode::RttyR,
        ] {
            assert_eq!(Mode::from_byte(m.to_byte()), Some(m));
            assert_eq!(Mode::from_name(m.name()), Some(m));
        }
        assert_eq!(Mode::from_byte(0x7F), None);
    }

    #[test]
    fn set_mode_with_and_without_filter() {
        assert_eq!(set_mode(0xA2, Mode::Usb, None).data, vec![0x01]);
        assert_eq!(set_mode(0xA2, Mode::Cw, Some(2)).data, vec![0x03, 0x02]);
    }

    #[test]
    fn ptt_set_and_parse() {
        assert_eq!(set_ptt(0xA2, true).data, vec![0x00, 0x01]);
        assert_eq!(set_ptt(0xA2, false).data, vec![0x00, 0x00]);
        let rx = Frame {
            to: 0xE0,
            from: 0xA2,
            cmd: 0x1C,
            data: vec![0x00, 0x00],
        };
        let tx = Frame {
            data: vec![0x00, 0x01],
            ..rx.clone()
        };
        assert_eq!(parse_ptt(&rx), Some(false));
        assert_eq!(parse_ptt(&tx), Some(true));
    }

    #[test]
    fn level_bcd2_round_trips() {
        for v in [0u16, 1, 99, 100, 120, 128, 241, 255] {
            let [hi, lo] = level_to_bcd2(v);
            assert_eq!(level_from_bcd2(hi, lo), v, "level {v}");
        }
        // Known Icom byte layout: 128 → 0x01 0x28.
        assert_eq!(level_to_bcd2(128), [0x01, 0x28]);
    }

    #[test]
    fn smeter_raw_reply_and_db_curve() {
        // FE FE E0 A2 15 02 <2-byte BCD> FD, raw 120 = S9.
        let [hi, lo] = level_to_bcd2(120);
        let reply = Frame {
            to: 0xE0,
            from: 0xA2,
            cmd: 0x15,
            data: vec![0x02, hi, lo],
        };
        assert_eq!(parse_smeter_raw(&reply), Some(120));
        assert!(
            (smeter_db_rel_s9(120) - 0.0).abs() < 0.001,
            "raw 120 = S9 = 0 dB"
        );
        assert!(smeter_db_rel_s9(0) < smeter_db_rel_s9(120)); // S0 below S9
        assert!(smeter_db_rel_s9(241) > smeter_db_rel_s9(120)); // S9+60 above S9
        assert!(
            (smeter_db_rel_s9(0) + 54.0).abs() < 0.001,
            "raw 0 = S0 = -54 dB"
        );
    }

    #[test]
    fn repeater_offset_matches_the_official_example() {
        // IC-9700 CI-V reference: 600 kHz offset → cmd 0D data 00 60 00 (3-byte LE BCD,
        // 100 Hz units).
        let f = set_rptr_offset(0xA2, 600_000);
        assert_eq!(f.cmd, 0x0D);
        assert_eq!(f.data, vec![0x00, 0x60, 0x00]);
        // 5 MHz (70 cm convention) → 00 00 05.
        assert_eq!(
            set_rptr_offset(0xA2, 5_000_000).data,
            vec![0x00, 0x00, 0x05]
        );
    }

    #[test]
    fn rf_power_set_maps_percent_to_level() {
        // 100% → raw 255 → BCD 0x02 0x55.
        assert_eq!(set_rf_power(0xA2, 100).data, vec![0x0A, 0x02, 0x55]);
        assert_eq!(set_rf_power(0xA2, 0).data, vec![0x0A, 0x00, 0x00]);
        // Round-trips through the raw decoder.
        let f = set_rf_power(0xA2, 50);
        let reply = Frame {
            to: 0xE0,
            from: 0xA2,
            cmd: 0x14,
            data: f.data.clone(),
        };
        assert_eq!(parse_rf_power_raw(&reply), Some(127)); // 50% of 255
    }

    /// THE TRANSMIT MONITOR'S GAIN. `16 45` ([`func_sub`]) already switched the monitor on —
    /// it was in this table and nothing ever asked for it — but a monitor with no volume is
    /// a speaker you cannot turn down while you are talking into the microphone.
    ///
    /// Captured the way [`LVL_AF`] and [`LVL_COMP`] were: Hamlib 4.5.5's own Icom backend
    /// driven against a pty at the IC-7300's address (`rigctl -m 3073`), frames off the wire —
    /// `L MONITOR_GAIN 0.25` -> `14 15 00 63`, `0.5` -> `14 15 01 27`, `1.0` -> `14 15 02 55`,
    /// and the read `l MONITOR_GAIN` -> `14 15`. Reproduced at the IC-7610's address
    /// (`-m 3078`, `98` on the bus) so the register is the family's and not one rig's.
    #[test]
    fn the_monitor_gain_level_is_the_byte_hamlibs_icom_backend_puts_on_the_wire() {
        assert_eq!(LVL_MONITOR_GAIN, 0x15);
        assert_eq!(
            level_sub("MONITOR_GAIN"),
            Some(LVL_MONITOR_GAIN),
            "MONITOR_GAIN sub-command"
        );
        // ⚠️ THREE DIFFERENT GAINS, so the payload identifies the register it came from.
        // One value driven through all of them is inert: MONITOR_GAIN and COMP transposed
        // would produce an identical frame at every single setting and pass.
        for (percent, bcd) in [(25u8, 63u16), (50, 127), (100, 255)] {
            let f = set_dsp_level(0xA2, level_sub("MONITOR_GAIN").unwrap(), percent);
            assert_eq!(f.cmd, 0x14);
            assert_eq!(
                f.data[0], 0x15,
                "monitor gain must not ride another level's register"
            );
            assert_eq!(
                level_from_bcd2(f.data[1], f.data[2]),
                bcd,
                "MONITOR_GAIN {percent}%"
            );
        }
        assert_eq!(read_dsp_level(0xA2, LVL_MONITOR_GAIN).data, vec![0x15]);
        // THE POSITIVE CONTROL: the same capture reproduced the constants this file already
        // held — `14 0a` RFPOWER, `14 0b` MICGAIN, `14 06` NR, `14 0e` COMP and the four
        // `0x16` funcs — which is what makes the new byte evidence rather than recollection.
        assert_eq!(level_sub("NR"), Some(0x06));
        assert_eq!(level_sub("COMP"), Some(0x0E));
        assert_eq!(set_rf_power(0xA2, 50).data[0], 0x0A);
        assert_eq!(set_mic_gain(0xA2, 50).data[0], 0x0B);
        for (token, sub) in [("ANF", 0x41u8), ("NB", 0x22), ("COMP", 0x44), ("MON", 0x45)] {
            assert_eq!(func_sub(token), Some(sub), "{token} sub-command");
        }
        // The MONITOR is a FUNC and its GAIN is a LEVEL: two registers, two families. A
        // table answering one with the other's byte would move the wrong control silently.
        assert_ne!(
            u16::from(LVL_MONITOR_GAIN),
            u16::from(func_sub("MON").unwrap()),
            "the monitor's gain must not ride its on/off register"
        );
        assert_eq!(func_sub("MONITOR_GAIN"), None); // a level, never a func
    }

    /// ⚠️ THE ATTENUATOR IS DECIBELS IN BCD, AND THE OBVIOUS READING OF THE BYTE IS WRONG.
    /// It is not a 0..1 fraction and it is not the dB as a plain integer: `L ATT 12` puts
    /// `11 12` on the wire, where `0x12` is BCD twelve. A raw-hex encoder would send `11 0c`
    /// and a twelve-dB pad would arrive as something else entirely.
    ///
    /// Established by capture, not by recall: Hamlib 4.5.5's own Icom backend against a pty.
    /// ⭐ The IC-7610 (`-m 3078`) is what proves the encoding, because it is the one rig here
    /// with a THREE-STEP pad — `L ATT 6` -> `11 06`, `L ATT 12` -> `11 12`, `L ATT 18` ->
    /// `11 18`. Under raw hex those would have been `06`/`0c`/`12`; two of the three differ,
    /// so the three values discriminate the encodings where any one of them alone could not.
    /// `L ATT 0` -> `11 00` (off) and the read `l ATT` -> a bare `11`.
    ///
    /// The attenuator also has its OWN command (`0x11`), not a `0x14` level sub-command, so
    /// it cannot go through [`set_dsp_level`]'s percent path at all.
    #[test]
    fn the_attenuator_is_bcd_decibels_on_its_own_command() {
        // The three IC-7610 pads Hamlib's backend declares, and the byte each one produces.
        for (db, wire) in [(0u8, 0x00u8), (6, 0x06), (12, 0x12), (18, 0x18)] {
            let f = set_attenuator_db(0x98, db);
            assert_eq!(f.cmd, 0x11, "the attenuator has its own CI-V command");
            assert_eq!(f.data, vec![wire], "ATT {db} dB");
        }
        // ⭐ THE DISCRIMINATOR, stated as its own assertion: BCD and raw hex AGREE on 6 dB
        // and DISAGREE on 12 and 18. A test driven only at 6 dB would pass either way.
        assert_eq!(set_attenuator_db(0x98, 12).data, vec![0x12]);
        assert_ne!(
            set_attenuator_db(0x98, 12).data,
            vec![12],
            "12 dB is BCD 0x12, not raw 0x0c — a raw-hex encoder pads by the wrong amount"
        );
        // Read frame carries no payload, and a reply decodes back to the dB.
        assert_eq!(read_attenuator(0x98).data, Vec::<u8>::new());
        let reply = Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x11, 0x18, 0xFD]).unwrap();
        assert_eq!(parse_attenuator_db(&reply), Some(18));
        // A frame from a different register is not an attenuator reading.
        let other = Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x14, 0x06, 0x01, 0x27, 0xFD]).unwrap();
        assert_eq!(parse_attenuator_db(&other), None);
        // And it is NOT a 0x14 level: nothing may route it into the percent path, which
        // would turn 12 dB into "12%" and then into full scale.
        assert_eq!(level_sub("ATT"), None);
        assert_eq!(func_sub("ATT"), None);
    }

    /// ⚠️ THE PREAMP'S WIRE BYTE IS A LIST POSITION, NOT A DECIBEL COUNT — which is why the
    /// per-rig step list is load-bearing rather than decoration.
    ///
    /// Captured against a pty from Hamlib 4.5.5's own Icom backend at two addresses. On the
    /// IC-7300 (`-m 3073`) whose preamps Hamlib labels `1` and `2`: `L PREAMP 1` -> `16 02 01`,
    /// `L PREAMP 2` -> `16 02 02`, `L PREAMP 0` -> `16 02 00`. That capture alone is
    /// ambiguous — the label and the index are the same number. ⭐ The IC-7610 (`-m 3078`),
    /// whose preamps are labelled `12` and `20` dB, settles it: `L PREAMP 12` -> `16 02 01`
    /// and `L PREAMP 20` -> `16 02 02`. The rig is told WHICH preamp, and the operator's dB
    /// is only a label that must be looked up in that rig's own list.
    ///
    /// So the preamp rides the `0x16` family but is NOT a [`func_sub`] entry: that table
    /// feeds [`parse_dsp_func`], which answers a bool, and this control has three states.
    #[test]
    fn the_preamp_is_a_list_index_not_a_decibel_count() {
        for (idx, wire) in [(0u8, 0x00u8), (1, 0x01), (2, 0x02)] {
            let f = set_preamp_index(0x98, idx);
            assert_eq!(f.cmd, 0x16);
            assert_eq!(f.data, vec![FUNC_PREAMP, wire], "preamp index {idx}");
        }
        assert_eq!(read_preamp(0x98).data, vec![FUNC_PREAMP]);
        let reply = Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x16, 0x02, 0x02, 0xFD]).unwrap();
        assert_eq!(parse_preamp_index(&reply), Some(2));
        // ⭐ THE TRANSLATION, at the rig where label and index DISAGREE. 12 dB is the
        // IC-7610's FIRST preamp, so the wire byte is 1 — an implementation that sent the
        // decibels would ask for preamp twelve, which does not exist.
        assert_eq!(preamp_index_for_db(IcomModel::Ic7610, 12), Some(1));
        assert_eq!(preamp_index_for_db(IcomModel::Ic7610, 20), Some(2));
        assert_ne!(
            preamp_index_for_db(IcomModel::Ic7610, 12),
            Some(12),
            "the wire carries the preamp's POSITION, never its decibels"
        );
        // Off is off on every rig, and a label this rig does not have is refused rather
        // than rounded to a neighbour — a preamp is a list, not a slider.
        assert_eq!(preamp_index_for_db(IcomModel::Ic7610, 0), Some(0));
        assert_eq!(preamp_index_for_db(IcomModel::Ic7610, 10), None);
        // Round trip, so the getter and the setter cannot disagree about the same rig.
        assert_eq!(preamp_db_for_index(IcomModel::Ic7610, 1), Some(12));
        assert_eq!(preamp_db_for_index(IcomModel::Ic7610, 2), Some(20));
        assert_eq!(preamp_db_for_index(IcomModel::Ic7610, 0), Some(0));
        assert_eq!(preamp_db_for_index(IcomModel::Ic7610, 3), None);
        // The 7300's list makes label and index coincide; that is the case that hides the
        // bug, so it is asserted beside the one that exposes it.
        assert_eq!(preamp_index_for_db(IcomModel::Ic7300, 2), Some(2));
        assert_eq!(preamp_db_for_index(IcomModel::Ic7300, 2), Some(2));
        // It is not a bool func and not a 0x14 level.
        assert_eq!(func_sub("PREAMP"), None);
        assert_eq!(level_sub("PREAMP"), None);
    }

    /// ⭐ THE BY-NAME DIAL AND MODE READ THEIR OWN BAND AND NOTHING ELSE (A7380-7EX-4 p. 13).
    ///
    /// `25 <band>` / `26 <band>` carry the band in their first data byte and the reply names it
    /// back, so a reply naming the Sub must never read as Main's — that is the selection-
    /// following this read exists to remove, arriving by another door.
    #[test]
    fn the_by_name_dial_and_mode_read_their_own_band_and_nothing_else() {
        // Only the IC-7610 names MAIN/SUB this way; the IC-9700's same bytes name the SELECTED
        // VFO (A7508-3EX-4 p. 24), and the one-receiver rigs have nothing to name.
        assert!(dial_by_band_name(IcomModel::Ic7610));
        for m in [
            IcomModel::Ic9700,
            IcomModel::Ic7300,
            IcomModel::Ic705,
            IcomModel::Ic905,
        ] {
            assert!(!dial_by_band_name(m), "{m:?}");
        }
        assert_eq!(
            read_band_freq(0x98, BAND_MAIN).to_bytes(),
            vec![0xFE, 0xFE, 0x98, 0xE0, 0x25, 0x00, 0xFD]
        );
        assert_eq!(
            read_band_mode(0x98, BAND_MAIN).to_bytes(),
            vec![0xFE, 0xFE, 0x98, 0xE0, 0x26, 0x00, 0xFD]
        );

        // Frequency: `25 00` + 5-byte BCD, least significant pair first (p. 10).
        let main = Frame::parse(&[
            0xFE, 0xFE, 0xE0, 0x98, 0x25, 0x00, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD,
        ])
        .unwrap();
        assert_eq!(parse_band_freq(&main, BAND_MAIN), Some(14_074_000));
        let sub = Frame::parse(&[
            0xFE, 0xFE, 0xE0, 0x98, 0x25, 0x01, 0x00, 0x40, 0x07, 0x07, 0x00, 0xFD,
        ])
        .unwrap();
        assert_eq!(
            parse_band_freq(&sub, BAND_MAIN),
            None,
            "the Sub's dial is not Main's"
        );
        assert_eq!(parse_band_freq(&sub, BAND_SUB), Some(7_074_000));
        let plain = Frame::parse(&[
            0xFE, 0xFE, 0xE0, 0x98, 0x03, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD,
        ])
        .unwrap();
        assert_eq!(
            parse_band_freq(&plain, BAND_MAIN),
            None,
            "a `03` reply names no band"
        );
        let short = Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x25, 0x00, 0x00, 0x40, 0xFD]).unwrap();
        assert_eq!(
            parse_band_freq(&short, BAND_MAIN),
            None,
            "a truncated reply is no reading"
        );

        // Mode: `26 00 <mode> <data> <filter>`. The filter comes back; the DATA byte does not
        // change the answer (see `parse_band_mode`).
        let usb_d1 =
            Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x26, 0x00, 0x01, 0x01, 0x02, 0xFD]).unwrap();
        assert_eq!(
            parse_band_mode(&usb_d1, BAND_MAIN),
            Some((Mode::Usb, Some(0x02)))
        );
        let usb =
            Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x26, 0x00, 0x01, 0x00, 0x02, 0xFD]).unwrap();
        assert_eq!(
            parse_band_mode(&usb, BAND_MAIN),
            parse_band_mode(&usb_d1, BAND_MAIN)
        );
        let sub_lsb =
            Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x26, 0x01, 0x00, 0x00, 0x01, 0xFD]).unwrap();
        assert_eq!(
            parse_band_mode(&sub_lsb, BAND_MAIN),
            None,
            "the Sub's mode is not Main's"
        );
        assert_eq!(
            parse_band_mode(&sub_lsb, BAND_SUB),
            Some((Mode::Lsb, Some(0x01)))
        );
        // PSK (`12`) is a mode this table has no name for — no reading, exactly as from `04`.
        let psk =
            Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x26, 0x00, 0x12, 0x00, 0x01, 0xFD]).unwrap();
        assert_eq!(parse_band_mode(&psk, BAND_MAIN), None);
        assert_eq!(
            parse_mode(&Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x04, 0x12, 0x01, 0xFD]).unwrap()),
            None
        );
    }

    /// ⭐ THE BY-NAME WRITES CARRY WHAT `05`, `06` AND `1A 06` CARRIED (A7380-7EX-4 pp. 10, 12,
    /// 13) — the band byte is the only new thing on the wire.
    #[test]
    fn the_by_name_writes_carry_what_05_06_and_1a06_carried() {
        // The dial: `25 00` and the same five BCD bytes `05` carries.
        assert_eq!(
            set_band_freq(0x98, BAND_MAIN, 14_074_000).to_bytes(),
            vec![0xFE, 0xFE, 0x98, 0xE0, 0x25, 0x00, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD]
        );
        assert_eq!(
            set_band_freq(0x98, BAND_MAIN, 14_074_000).data[1..],
            set_freq(0x98, 14_074_000).data[..]
        );
        assert_eq!(set_band_freq(0x98, BAND_SUB, 7_074_000).data[0], 0x01);
        // A plain mode: the mode byte alone after the band, which the radio takes as DATA OFF
        // and the mode's default filter — the state `06 <mode>` + `1A 06 00 00` left.
        assert_eq!(
            set_band_mode(0x98, BAND_MAIN, Mode::Lsb, None).to_bytes(),
            vec![0xFE, 0xFE, 0x98, 0xE0, 0x26, 0x00, 0x00, 0xFD]
        );
        // A DATA mode: D<n> and FIL1, the two bytes `1A 06 <n> 01` carried, clamped the same way.
        for (n, d) in [(1u8, 0x01u8), (2, 0x02), (3, 0x03), (0, 0x01), (9, 0x03)] {
            let f = set_band_mode(0x98, BAND_MAIN, Mode::Usb, Some(n));
            assert_eq!(f.data, vec![0x00, 0x01, d, 0x01], "D{n}");
            assert_eq!(
                f.data[2..],
                set_data_mode_n(0x98, n, None).data[1..],
                "D{n}: the bytes `1A 06` carried"
            );
        }
    }

    /// THE STEP LISTS, AND WHERE EVERY NUMBER CAME FROM. An attenuator is not a slider: it
    /// is the handful of pads a particular radio actually has, and offering a value the rig
    /// does not own gets it NAKed or silently rounded.
    ///
    /// Each list is what Hamlib 4.5.5's own backend for that rig prints under `Attenuator:`
    /// and `Preamp:` in `rigctl -m <model> 1` (`dump_caps`) — the same `rig_caps.attenuator[]`
    /// / `.preamp[]` arrays its `\dump_state` emits, read back live rather than transcribed:
    ///
    /// ```text
    /// IC-7300  m=3073   Attenuator: 20dB            Preamp: 1dB 2dB
    /// IC-9700  m=3081   Attenuator: 10dB            Preamp: 1dB 2dB
    /// IC-705   m=3085   Attenuator: 20dB            Preamp: 1dB 2dB
    /// IC-7610  m=3078   Attenuator: 6dB 12dB 18dB   Preamp: 12dB 20dB
    /// ```
    ///
    /// ⭐ Except the IC-7610's ATTENUATOR, which is Icom's fifteen pads rather than Hamlib's
    /// three — `the_ic7610_attenuator_is_icoms_fifteen_three_db_pads` below pins it.
    ///
    /// ⚠️ The 7300-family "preamps" are `1` and `2` because they are Icom's P.AMP1/P.AMP2
    /// SELECTORS, which Hamlib carries in the same dB-labelled array; they are labels, not
    /// twelve-decibel-style gains, and [`preamp_index_for_db`] treats every entry as a label
    /// for exactly that reason.
    ///
    /// ⛔ THE IC-905 IS DELIBERATELY EMPTY. Hamlib 4.5.5 has no IC-905 backend at all
    /// (`rigctl -m 3095` returns no caps), so there is no source to read its pads off and
    /// nothing was invented for it: an empty list means the control does not render, which
    /// degrades honestly. NEEDS-BENCH (IC-905).
    #[test]
    fn the_step_lists_are_the_ones_hamlibs_own_backends_declare() {
        assert_eq!(attenuator_steps_db(IcomModel::Ic7300), &[20]);
        assert_eq!(attenuator_steps_db(IcomModel::Ic9700), &[10]);
        assert_eq!(attenuator_steps_db(IcomModel::Ic705), &[20]);
        assert_eq!(preamp_steps_db(IcomModel::Ic7300), &[1, 2]);
        assert_eq!(preamp_steps_db(IcomModel::Ic9700), &[1, 2]);
        assert_eq!(preamp_steps_db(IcomModel::Ic705), &[1, 2]);
        assert_eq!(preamp_steps_db(IcomModel::Ic7610), &[12, 20]);
        // ⚠️ The lists DIFFER between rigs, which is the whole reason they exist. A single
        // hard-coded ladder would pass a test that only ever looked at one model.
        assert_ne!(
            attenuator_steps_db(IcomModel::Ic7300),
            attenuator_steps_db(IcomModel::Ic7610)
        );
        assert_ne!(
            attenuator_steps_db(IcomModel::Ic7300),
            attenuator_steps_db(IcomModel::Ic9700)
        );
        assert_ne!(
            preamp_steps_db(IcomModel::Ic7300),
            preamp_steps_db(IcomModel::Ic7610)
        );
        // Unknown rig, empty list, no control — never a guessed one.
        assert!(attenuator_steps_db(IcomModel::Ic905).is_empty());
        assert!(preamp_steps_db(IcomModel::Ic905).is_empty());
        // 0 (off) is never IN a list; it is the implicit first position every rig has.
        for m in [
            IcomModel::Ic7300,
            IcomModel::Ic9700,
            IcomModel::Ic705,
            IcomModel::Ic7610,
        ] {
            assert!(!attenuator_steps_db(m).contains(&0), "{m:?} attenuator");
            assert!(!preamp_steps_db(m).contains(&0), "{m:?} preamp");
        }
    }

    /// ⭐ THE IC-7610'S ATTENUATOR IS ICOM'S LADDER: FIFTEEN 3 dB PADS, 3 TO 45 dB.
    ///
    /// Icom's CI-V Reference Guide for the IC-7610 (A7380-7EX-4, Sep. 2025, p. 3) gives command
    /// `11` sixteen data values — `00` (OFF), then `03`, `06`, `09` … `42`, `45`, each "Send/read
    /// the <n> dB attenuator setting" — and marks it band-directed. Hamlib's IC-7610 backend
    /// declares only 6, 12 and 18 (4.5.5, and its current source still does), which is where
    /// the old list came from: an operator who wanted 3 dB, or more than 18, could not get it
    /// from Nexus although the radio has it.
    ///
    /// The bytes below are typed from Icom's table, not computed. The data byte is the
    /// decibels in BCD, so 3, 6 and 9 read the same under BCD and raw hex, and every pad from
    /// 12 up does not — 45 dB is `0x45`, where raw hex would send `0x2D`, a value the table
    /// does not have.
    #[test]
    fn the_ic7610_attenuator_is_icoms_fifteen_three_db_pads() {
        let icom: [(u8, u8); 15] = [
            (3, 0x03),
            (6, 0x06),
            (9, 0x09),
            (12, 0x12),
            (15, 0x15),
            (18, 0x18),
            (21, 0x21),
            (24, 0x24),
            (27, 0x27),
            (30, 0x30),
            (33, 0x33),
            (36, 0x36),
            (39, 0x39),
            (42, 0x42),
            (45, 0x45),
        ];
        let pads: Vec<u8> = icom.iter().map(|&(db, _)| db).collect();
        assert_eq!(attenuator_steps_db(IcomModel::Ic7610), pads.as_slice());
        for (db, byte) in icom {
            assert_eq!(
                set_attenuator_db(0x98, db).to_bytes(),
                vec![0xFE, 0xFE, 0x98, 0xE0, 0x11, byte, 0xFD],
                "{db} dB on the wire"
            );
            let reply = Frame::parse(&[0xFE, 0xFE, 0xE0, 0x98, 0x11, byte, 0xFD]).unwrap();
            assert_eq!(
                parse_attenuator_db(&reply),
                Some(db),
                "{byte:02X} read back"
            );
        }
        // OFF is Icom's `00`, and stays the implicit first position, never in the list.
        assert_eq!(
            set_attenuator_db(0x98, 0).to_bytes(),
            vec![0xFE, 0xFE, 0x98, 0xE0, 0x11, 0x00, 0xFD]
        );

        // ⛔ EVERY OTHER RADIO'S LIST IS UNCHANGED — pinned beside the one that moved, so a
        // table edit that reached a neighbour's row fails here by name.
        assert_eq!(attenuator_steps_db(IcomModel::Ic7300), &[20]);
        assert_eq!(attenuator_steps_db(IcomModel::Ic705), &[20]);
        assert_eq!(attenuator_steps_db(IcomModel::Ic9700), &[10]);
        assert!(attenuator_steps_db(IcomModel::Ic905).is_empty());
        // The IC-7610's preamp is a different control and did not move either.
        assert_eq!(preamp_steps_db(IcomModel::Ic7610), &[12, 20]);
    }
}
