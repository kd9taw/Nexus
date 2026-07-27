//! Safe Rust wrapper over `libtempo`'s native FST4 decoder.
//!
//! FST4 (WSJT-X) is the slow weak-signal sibling of FT8: 4-GFSK, 160 channel
//! symbols, LDPC(240,101)+CRC-24 for QSO messages. This wraps the vendored WSJT-X
//! GPL decoder (`fst4_decode_frame` in `fst4_cabi.f90`), which drives the clean OO
//! `fst4_decoder` (get_candidates_fst4 → sync_fst4 → fst4_downsample →
//! get_fst4_bitmetrics → decode240_101/decode240_74 + OSD).
//!
//! # DECODE ONLY — no encode, no gen_wave
//! There is deliberately no `encode` or `gen_wave` here, and none in the C ABI.
//! FST4 ships receive-only: `ModeKind::Fst4` reports `Capabilities { tx: false }`,
//! so `modes::tx_mode()` refuses to hand it to the transmit path. Adding TX means
//! adding those entry points in `fst4_cabi.f90`, flipping that flag, AND passing
//! the FT-mode TX approval gate — three deliberate steps.
//!
//! # ⭐ Single T/R period
//! Upstream FST4 supports 15/30/60/120/300/900/1800 s, and `fst4_decode` sizes its
//! frame from the period. The C ABI pins **15 s / 180000 samples** so the buffer
//! contract is fixed. Supporting more periods needs a per-period entry point plus a
//! `ModeKind` that can carry one, which is why [`NMAX`] is a constant here rather
//! than a function of anything.
//!
//! # Thread safety
//! Not thread-safe; serializes behind [`tempo_fast_sys::MODEM_LOCK`] — the single
//! lock shared across every mode (FT1/FT8/FT4/FST4/DX1) that links `libtempo`.

use tempo_fast_sys::MODEM_LOCK;

pub use tempo_fast_sys::{FST4_NMAX as NMAX, FST4_NTRPERIOD as NTRPERIOD};

/// WSJT-X audio sample rate (Hz).
pub const SAMPLE_RATE: f32 = 12_000.0;

/// A signal recovered by [`decode_frame`].
#[derive(Debug, Clone)]
pub struct Decode {
    /// Decoded message text.
    pub message: String,
    /// Sync correlation metric.
    pub sync: f32,
    /// SNR estimate (dB, 2500 Hz BW).
    pub snr: i32,
    /// Time offset in seconds (WSJT-X convention `xdt = t − 0.5`).
    pub dt: f32,
    /// Audio carrier frequency (Hz).
    pub freq: f32,
    /// A-priori decode type used (iaptype; 0 = none).
    pub nap: i32,
    /// Decode quality (1.0 = perfect).
    pub qual: f32,
}

// Matches FST4_MAXDEC in fst4_cabi.f90, which in turn matches MAXCAND in
// fst4_decode.f90. Raise both together or the weakest decodes are dropped silently.
const MAX_DECODES: usize = 100;

/// Decode every FST4 signal in a 180000-sample ([`NMAX`]) int16 frame at 12 kHz.
///
/// `nfa..=nfb` is the audio search range (Hz); `ndepth` is decode aggressiveness
/// (≤ 0 ⇒ 3); `mycall`/`hiscall` enable a-priori decoding (pass `""` if unknown).
/// `nfqso` is the QSO/RX audio frequency (Hz) being worked; the deep AP passes fire
/// near it. Pass 0 (or out of `nfa..=nfb`) for band-center.
///
/// Runs FST4 QSO mode (iwspr=0). FST4W beacon mode is not wired up: it needs the
/// hashed-callsign table that the headless build's excised file read populated.
///
/// # Panics
/// Panics if `iwave.len() < NMAX`.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame(
    iwave: &[i16],
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    nqso_progress: i32,
    nfqso: i32,
) -> Vec<Decode> {
    assert!(
        iwave.len() >= NMAX,
        "decode_frame needs at least {NMAX} samples, got {}",
        iwave.len()
    );
    let myc = std::ffi::CString::new(mycall).unwrap_or_default();
    let hisc = std::ffi::CString::new(hiscall).unwrap_or_default();
    let mut out = vec![tempo_fast_sys::Fst4DecodeT::default(); MAX_DECODES];

    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe {
            tempo_fast_sys::fst4_decode_frame(
                iwave.as_ptr(),
                nfa,
                nfb,
                ndepth,
                myc.as_ptr(),
                hisc.as_ptr(),
                nqso_progress,
                nfqso,
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
            sync: r.sync,
            snr: r.snr,
            dt: r.dt,
            freq: r.freq,
            nap: r.nap,
            qual: r.qual,
        })
        .collect()
}

/// Read a NUL-padded fixed C char field into a trimmed `String`.
fn cstr_field(b: &[u8; 38]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_is_the_pinned_15s_length() {
        // The ABI pins one period; if this changes, fst4_cabi.f90's FST4_NMAX and
        // FST4_NTRPERIOD must change with it or the buffer contract breaks.
        assert_eq!(NTRPERIOD, 15);
        assert_eq!(NMAX, 15 * 12_000);
    }

    #[test]
    fn noise_decodes_to_nothing() {
        // Same property the C smoke test asserts, through the Rust surface: the
        // decoder runs to completion on pure noise and invents nothing. With no
        // FST4 TX in-tree there is no way to synthesise a signal, so this is a
        // liveness + silence check, NOT a sensitivity test.
        let mut iwave = vec![0i16; NMAX];
        let mut seed: u32 = 0x5EED;
        for s in iwave.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *s = ((seed >> 16) as i16) / 8;
        }
        let d = decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 0, 1500);
        assert!(d.is_empty(), "decoded {} signal(s) from noise", d.len());
    }
}
