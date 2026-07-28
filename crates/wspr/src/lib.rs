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
//! # DECODE ONLY — no encode, no gen_wave
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
