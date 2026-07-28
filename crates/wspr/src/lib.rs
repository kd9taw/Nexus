//! Safe Rust wrapper over `libtempo`'s native WSPR decoder.
//!
//! WSPR — Weak Signal Propagation Reporter — is the **beacon** mode. A station
//! sends "CALL GRID DBM" as 50 bits over 110.6 s at about 1.4 baud inside a
//! 2-minute window, and everyone else reports what they heard. The point is not
//! to work anyone; it is to map which paths are open, which is why its decodes
//! feed propagation intelligence rather than a log.
//!
//! # ⭐ This decoder was a PROGRAM
//! WSJT-X has no library-shaped WSPR decoder — it runs the `wsprd` **executable**
//! as a subprocess. Nexus converted `main()` into `wspr_decode_core()`; every
//! piece of the program shell that was removed carries a `MODIFIED FOR NEXUS`
//! block in `wsprd.c`. That history matters when reading the state manifest: a
//! static a run-once-and-exit program could leave in any state is one a
//! repeatedly-called library has to answer for.
//!
//! # ⭐ Serialization is a requirement, not a tuning choice
//! This plans three FFTW transforms per call, and `fftwf_plan_dft_*` — the FFTW
//! **planner** — is not thread-safe. Two concurrent decodes corrupt FFTW's
//! internal plan list regardless of where the handles live. [`decode_frame`]
//! holds [`tempo_fast_sys::MODEM_LOCK`] for exactly this reason.
//!
//! # Transmit and receive
//! [`encode`] and [`gen_wave`] are the transmit half. The encoder is upstream's
//! `get_wspr_channel_symbols`, already compiled because the DECODER calls it to
//! subtract a decoded signal — so encode and decode share one generator.
//!
//! The WAVEFORM is synthesised here rather than in C, because upstream has none to
//! borrow: `wsprsim`'s `add_signal_vector` builds an I/Q pair for the simulator's
//! own noise model, not slot-positioned audio. WSPR's modulation is plain
//! continuous-phase 4-FSK, which is short enough to write and test directly.
//!
//! # ⭐ WSPR IS A BEACON, NOT A QSO MODE
//! The 50-bit payload is exactly callsign + grid + power, with no exchange, no
//! addressing and no free text. Putting it on the air is a SCHEDULING decision —
//! a percentage of 2-minute intervals, unattended — which is why the operating
//! layer routes these tiers through a beacon scheduler instead of the auto-sequencer.
//!
//! # DECODE-ONLY NOTE (historical)
//! `ModeKind::Wspr` reports `Capabilities { tx: false }`, so `modes::tx_mode()`
//! refuses to hand it to the transmit path. WSPR transmit is also an operating
//! decision, not just a code one: a beacon keys unattended on a schedule.
//!
//! # ⚠️ Hashed callsigns do not resolve
//! The hashed-callsign table is off (upstream's own `-H`), because it persisted
//! to a file on disk — a path for one radio chain's callsigns to reach another's
//! decoder, the same shape removed from Q65 and FST4W. Type-2 and type-3 messages
//! therefore report the `<...>` hash form instead of a previously-heard call.
//! Type-1 messages, which carry the callsign outright, are unaffected — and they
//! are the majority of WSPR traffic.

use tempo_fast_sys::MODEM_LOCK;

pub use tempo_fast_sys::{WSPR_NMAX as NMAX, WSPR_PERIOD_S as PERIOD_S};

/// WSJT-X audio sample rate (Hz).
pub const SAMPLE_RATE: f32 = 12_000.0;

/// WSPR channel symbols per transmission.
pub const NSYM: usize = 162;

/// Samples per symbol at 12 kHz. Upstream's WSPR symbol length.
pub const NSPS: usize = 8192;

/// Tone spacing in Hz — and also the baud rate, since WSPR's symbol length and tone
/// spacing are reciprocal: `12000/8192` ≈ 1.4648 Hz.
pub const TONE_SPACING_HZ: f32 = SAMPLE_RATE / NSPS as f32;

/// Where the modulation starts within the 2-minute window, in seconds.
///
/// ⭐ 1.0 s, confirmed in three independent places in WSJT-X 3.0.2:
/// `wsprsim.c:130` (`float f0=0.0, t0=1.0;`), the decoder's dt reference in
/// `wsprd.c` (`dt_print = shift1*dt - 1.0`), and the parity lab's own
/// `gen_wspr_wav.py`, which was validated against stock `wsprd`.
///
/// ⚠️ NOT the same as [`tx_duration_secs`], which is the PTT hold. WSJT-X's
/// `helper_functions.cpp` uses `2.0 + …` for WSPR; that 2.0 is keying allowance,
/// not the signal position. Conflating the two is exactly the bug that put FST4-15
/// half a second late.
pub const LEAD_IN_SECS: f32 = 1.0;

/// How long the radio stays keyed for one beacon, in seconds.
///
/// From `tx_duration()` in WSJT-X's `helper_functions.cpp:19`:
/// `2.0 + 162*8192/12000` ≈ 112.6 s, inside the 120 s window.
pub fn tx_duration_secs() -> f32 {
    2.0 + (NSYM * NSPS) as f32 / SAMPLE_RATE
}

/// Build the WSPR message text: `"CALL GRID DBM"`.
///
/// `power_dbm` is the transmitter's ERP in dBm. ⚠️ WSPR reports feed a PUBLIC
/// propagation database that other operators draw conclusions from, so a wrong
/// power figure corrupts other people's data, not just yours. The operating layer
/// requires it explicitly rather than defaulting it.
///
/// Only the first 4 characters of the grid are sent — the 50-bit payload has room
/// for no more.
pub fn message(callsign: &str, grid: &str, power_dbm: i32) -> String {
    let g: String = grid.chars().take(4).collect::<String>().to_uppercase();
    format!("{} {} {}", callsign.trim().to_uppercase(), g, power_dbm)
}

/// Encode `"CALL GRID DBM"` into the 162 channel symbols (0..3), or `None` if it
/// will not encode.
pub fn encode(msg: &str) -> Option<Vec<u8>> {
    let c = std::ffi::CString::new(msg).ok()?;
    let mut sym = vec![0u8; NSYM];
    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe { tempo_fast_sys::wspr_encode_msg(c.as_ptr(), sym.as_mut_ptr()) }
    };
    (n as usize == NSYM).then_some(sym)
}

/// Synthesise the WSPR audio for `symbols` at carrier `f0`, WITHOUT the lead-in.
///
/// Plain continuous-phase 4-FSK: tone *k* sits at `f0 + k * TONE_SPACING_HZ`, each
/// held for [`NSPS`] samples, with phase carried ACROSS symbol boundaries and never
/// reset. Resetting per symbol would splatter a mode whose whole point is occupying
/// ~6 Hz.
///
/// Returns `None` unless `symbols` is exactly [`NSYM`] values in 0..=3.
pub fn gen_wave(symbols: &[u8], fsample: f32, f0: f32) -> Option<Vec<f32>> {
    if symbols.len() != NSYM || symbols.iter().any(|&s| s > 3) {
        return None;
    }
    // Spacing is tied to the SYMBOL RATE, not the output rate: at any fsample a
    // symbol lasts NSPS/12000 s and the tones stay 12000/8192 Hz apart.
    let nsps_out = (NSPS as f32 * fsample / SAMPLE_RATE).round() as usize;
    let dt = 1.0 / f64::from(fsample);
    let mut wave = vec![0f32; NSYM * nsps_out];
    let mut phi = 0f64;
    let mut k = 0usize;
    for &s in symbols {
        let freq = f64::from(f0) + f64::from(s) * f64::from(TONE_SPACING_HZ);
        let dphi = std::f64::consts::TAU * freq * dt;
        for _ in 0..nsps_out {
            wave[k] = phi.sin() as f32;
            phi += dphi;
            if phi > std::f64::consts::TAU {
                phi -= std::f64::consts::TAU;
            }
            k += 1;
        }
    }
    Some(wave)
}

/// Which WSPR message form a decode came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Type 1 — `CALL GRID DBM`, the callsign sent in full. The common case.
    Standard,
    /// Type 2 — a compound callsign (portable/prefix), sent hashed.
    Compound,
    /// Type 3 — a 6-character grid, with the callsign sent hashed.
    HashedWithGrid,
    /// Anything else the decoder reports.
    Other(i32),
}

impl MessageType {
    fn from_code(c: i32) -> Self {
        match c {
            0 => MessageType::Standard,
            1 => MessageType::Compound,
            2 => MessageType::HashedWithGrid,
            n => MessageType::Other(n),
        }
    }

    /// True when this form carries a HASHED callsign, which this build cannot
    /// resolve (see the module docs) — such a decode shows `<...>` where the call
    /// should be, so a caller should not treat it as a station identification.
    pub fn is_hashed(self) -> bool {
        matches!(self, MessageType::Compound | MessageType::HashedWithGrid)
    }
}

/// A beacon report recovered by [`decode_frame`].
#[derive(Debug, Clone)]
pub struct Decode {
    /// Decoded message: `"CALL GRID DBM"`, at most 22 characters.
    pub message: String,
    /// ABSOLUTE RF frequency in MHz — dial plus audio offset, not an offset.
    /// Unlike every other mode here, which reports an audio frequency in Hz.
    pub freq_mhz: f64,
    /// Sync quality.
    pub sync: f32,
    /// SNR estimate (dB, 2500 Hz reference).
    pub snr: f32,
    /// Time offset in seconds.
    pub dt: f32,
    /// Frequency drift in Hz/minute — a WSPR-specific measure of how much the
    /// transmitter moved during its 110 s over. Large drift on an otherwise
    /// strong signal usually means an unstable transmitter, not a bad path.
    pub drift: f32,
    /// Which message form produced it.
    pub message_type: MessageType,
}

// WSPR's own `struct result decodes[50]` in wsprd.c. Raise both together.
const MAX_DECODES: usize = 50;

/// Decode every WSPR signal in one 2-minute reception interval.
///
/// `iwave` should hold [`NMAX`] int16 samples at 12 kHz (114 s of the window);
/// a shorter buffer is zero-padded by the decoder rather than refused, so a
/// capture that started late still decodes what it caught.
///
/// `dial_mhz` is the rig's dial frequency — WSPR reports ABSOLUTE frequency, so
/// this is what makes [`Decode::freq_mhz`] meaningful. Pass 0.0 and the reported
/// frequencies are audio offsets in MHz, which is almost never what you want.
///
/// `quick` skips the deep search for weak signals. `passes` is how many
/// subtraction passes to run — **upstream's default is 3**, and the third is the
/// weak-signal pass (wider blocksize, lower sync threshold). Passing 2 costs
/// 1-2 dB at the decode floor; the parity ladder caught exactly that. `subtract`
/// disables
/// signal subtraction between them; `more_candidates` and `stack_decoder` are
/// upstream's `-d` and `-J`.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame(
    iwave: &[i16],
    dial_mhz: f64,
    quick: bool,
    passes: i32,
    subtract: bool,
    more_candidates: bool,
    stack_decoder: bool,
) -> Vec<Decode> {
    let mut out = vec![tempo_fast_sys::WsprDecodeT::default(); MAX_DECODES];

    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe {
            tempo_fast_sys::wspr_decode_core(
                iwave.as_ptr(),
                iwave.len() as std::os::raw::c_long,
                dial_mhz,
                i32::from(quick),
                passes,
                i32::from(subtract),
                i32::from(more_candidates),
                i32::from(stack_decoder),
                out.as_mut_ptr(),
                out.len() as i32,
            )
        }
    };
    if n <= 0 {
        return Vec::new();
    }
    out.into_iter()
        .take(n as usize)
        .map(|r| Decode {
            message: cstr_field(&r.message),
            freq_mhz: r.freq,
            sync: r.sync,
            snr: r.snr,
            dt: r.dt,
            drift: r.drift,
            message_type: MessageType::from_code(r.decodetype),
        })
        .collect()
}

/// Read a NUL-padded fixed C char field into a trimmed `String`.
fn cstr_field(b: &[u8; 23]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_then_decode_recovers_the_beacon() {
        // Our waveform through our own decoder. Proves the WAVEFORM layer — symbol
        // rate, tone spacing, continuous phase, slot position — since the symbols
        // come from upstream's generator either way.
        const CALL: &str = "KD9TAW";
        const GRID: &str = "EN52";
        let msg = message(CALL, GRID, 30);
        let sym = encode(&msg).expect("encodes");
        let wave = gen_wave(&sym, SAMPLE_RATE, 1500.0).expect("valid");

        let lead = (LEAD_IN_SECS * SAMPLE_RATE) as usize;
        let mut iwave = vec![0i16; NMAX];
        for (i, &v) in wave.iter().enumerate() {
            if lead + i < iwave.len() {
                iwave[lead + i] = (v * 8000.0) as i16;
            }
        }

        let d = decode_frame(&iwave, 10.1387, false, 3, false, false, false);
        assert!(
            d.iter().any(|r| r.message.trim() == msg),
            "the beacon did not decode as {msg:?}: {:?}",
            d.iter().map(|r| r.message.trim()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn encode_produces_a_valid_4fsk_frame() {
        let sym = encode("KD9TAW EN52 30").expect("standard message encodes");
        assert_eq!(sym.len(), NSYM, "WSPR is 162 symbols");
        assert!(
            sym.iter().all(|&s| s <= 3),
            "WSPR is 4-FSK: symbols are 2*databit + sync, so 0..3"
        );
        // The sync vector is embedded in every symbol's low bit, so parity across
        // the frame is fixed by pr3 regardless of payload. Two different messages
        // must still differ somewhere.
        let other = encode("W1AW FN31 37").expect("encodes");
        assert_ne!(
            sym, other,
            "different messages must produce different symbols"
        );
    }

    #[test]
    fn the_message_builder_matches_the_wire_format() {
        assert_eq!(message("kd9taw", "en52tk", 30), "KD9TAW EN52 30");
        assert_eq!(message(" W1AW ", "FN31", 37), "W1AW FN31 37");
        // 4-character grid only — the 50-bit payload has room for no more.
        assert_eq!(message("K1ABC", "EN50AB", 23), "K1ABC EN50 23");
    }

    #[test]
    fn a_malformed_message_is_refused_rather_than_encoded() {
        assert!(
            encode("BAD\0MSG").is_none(),
            "an interior NUL cannot cross the FFI"
        );
        assert!(encode("").is_none(), "an empty message is not a beacon");
    }

    #[test]
    fn gen_wave_is_continuous_phase_and_correctly_sized() {
        let sym = encode("KD9TAW EN52 30").unwrap();
        let w = gen_wave(&sym, SAMPLE_RATE, 1500.0).expect("valid frame");
        assert_eq!(w.len(), NSYM * NSPS, "162 symbols x 8192 samples");

        // Continuity across every symbol boundary is the property that keeps WSPR
        // inside ~6 Hz. A phase reset would show up as a step; sample-to-sample
        // deltas must stay bounded by the highest tone's per-sample phase advance.
        let max_step = ((1500.0 + 3.0 * f64::from(TONE_SPACING_HZ)) * std::f64::consts::TAU
            / f64::from(SAMPLE_RATE))
        .sin()
            * 1.05;
        for i in 1..w.len() {
            let d = f64::from(w[i] - w[i - 1]).abs();
            assert!(
                d <= max_step.abs().max(0.9),
                "phase discontinuity at sample {i}: step {d}"
            );
        }
    }

    #[test]
    fn gen_wave_refuses_a_malformed_symbol_vector() {
        let sym = encode("KD9TAW EN52 30").unwrap();
        assert!(
            gen_wave(&sym[..161], SAMPLE_RATE, 1500.0).is_none(),
            "161 is not a frame"
        );
        let mut bad = sym.clone();
        bad[7] = 4; // 4-FSK has no symbol 4
        assert!(
            gen_wave(&bad, SAMPLE_RATE, 1500.0).is_none(),
            "symbol 4 does not exist"
        );
    }

    #[test]
    fn the_beacon_fits_inside_its_window() {
        // 2.0 + 162*8192/12000 = 112.6 s of keying inside a 120 s window, with the
        // modulation starting at 1.0 s. An overrun would key into the next window.
        let d = tx_duration_secs();
        assert!((d - 112.592).abs() < 0.01, "expected ~112.6 s, got {d}");
        assert!(d < f32::from(PERIOD_S), "the beacon must fit its window");
        assert!(LEAD_IN_SECS + (NSYM * NSPS) as f32 / SAMPLE_RATE < f32::from(PERIOD_S));
    }

    #[test]
    fn the_frame_is_114_seconds_of_the_two_minute_window() {
        assert_eq!(NMAX, 114 * 12_000);
        assert_eq!(PERIOD_S, 120);
        // The read window is SHORTER than the interval — WSPR's transmission is
        // 110.6 s and the decoder reads 114 s of the 120 s slot.
        assert!((NMAX as f32 / SAMPLE_RATE) < f32::from(PERIOD_S));
    }

    #[test]
    fn noise_decodes_to_nothing() {
        // Liveness + silence on noise. NOT a sensitivity test: with no WSPR TX
        // in-tree there is no way to synthesise a signal here, so sensitivity is
        // measured in the parity lab against stock wsprsim/wsprd.
        let mut iwave = vec![0i16; NMAX];
        let mut seed: u32 = 0x5EED;
        for s in iwave.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *s = ((seed >> 16) as i16) / 8;
        }
        let d = decode_frame(&iwave, 14.097, false, 2, true, false, false);
        assert!(d.is_empty(), "decoded {} beacon(s) from noise", d.len());
    }

    #[test]
    fn a_short_buffer_is_padded_not_refused() {
        // A capture that started late should still decode what it caught, so a
        // short buffer must not panic or error — the C side zero-pads.
        let iwave = vec![0i16; NMAX / 2];
        let d = decode_frame(&iwave, 14.097, true, 1, false, false, false);
        assert!(d.is_empty(), "silence decoded to {} beacon(s)", d.len());
    }

    #[test]
    fn hashed_message_types_are_flagged() {
        // A type-2/3 decode reports <...> rather than a callsign in this build,
        // so callers must be able to tell it apart from a real identification.
        assert!(!MessageType::from_code(0).is_hashed());
        assert!(MessageType::from_code(1).is_hashed());
        assert!(MessageType::from_code(2).is_hashed());
        assert_eq!(MessageType::from_code(9), MessageType::Other(9));
        assert!(!MessageType::Other(9).is_hashed());
    }
}
