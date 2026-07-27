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
//! # ⭐ All 7 periods, and both modes
//! Period and mode are arguments. **The frame length follows the period** —
//! [`nmax`] of it, 180000 samples (15 s) to 21600000 (1800 s).
//!
//! `wspr = false` is FST4, the QSO mode (77-bit messages, AP decoding).
//! `wspr = true` is **FST4W**, the WSPR-like beacon mode (50-bit messages, no AP).
//! FST4W is why the period had to become an argument: its standard beacon
//! intervals are 120/300/900/1800 s, so a 15 s-only wrapper could not do it.
//!
//! ⚠️ **FST4W hashed callsigns do not resolve.** The k50 lookup table is populated
//! upstream from `fst4w_calls.txt`, a GUI-side file the headless build removed.
//! With an empty table the decoder reports the `<...>` hash form — the same result
//! an empty file produced upstream. Beacon reception, SNR and grid all work; only
//! resolving a previously-heard hashed call is missing.
//!
//! # Thread safety
//! Not thread-safe; serializes behind [`tempo_fast_sys::MODEM_LOCK`] — the single
//! lock shared across every mode (FT1/FT8/FT4/FST4/DX1) that links `libtempo`.

use tempo_fast_sys::MODEM_LOCK;

pub use tempo_fast_sys::{
    fst4_nmax as nmax, fst4_period_supported as period_supported, FST4_NMAX_MAX as NMAX_MAX,
    FST4_PERIODS as PERIODS,
};

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

/// Decode every FST4 (or FST4W) signal in one T/R period.
///
/// `iwave` must hold [`nmax`]`(period_s)` int16 samples at 12 kHz. `period_s` must
/// be one of [`PERIODS`]. `wspr` selects FST4W beacon mode over FST4 QSO mode.
///
/// `nfa..=nfb` is the audio search range (Hz); `ndepth` is decode aggressiveness
/// (≤ 0 ⇒ 3); `mycall`/`hiscall` enable a-priori decoding (pass `""` if unknown).
/// `nfqso` is the QSO/RX audio frequency (Hz) being worked; the deep AP passes fire
/// near it. Pass 0 (or out of `nfa..=nfb`) for band-center.
///
/// # Panics
/// Panics if `iwave.len() < nmax(period_s)` or `period_s` is unsupported. These are
/// caller contract violations: buffer length and period must be chosen together,
/// and a silent short-read would decode a window the caller never intended.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame(
    iwave: &[i16],
    period_s: u16,
    wspr: bool,
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    nqso_progress: i32,
    nfqso: i32,
) -> Vec<Decode> {
    assert!(
        period_supported(period_s),
        "FST4 period {period_s}s is not supported (must be one of {PERIODS:?})"
    );
    let need = nmax(period_s);
    assert!(
        iwave.len() >= need,
        "decode_frame needs at least {need} samples for a {period_s}s period, got {}",
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
                i32::from(period_s),
                i32::from(wspr),
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
    fn frame_length_follows_the_period() {
        for p in PERIODS {
            assert_eq!(nmax(p), p as usize * 12_000);
            assert!(period_supported(p));
        }
        assert_eq!(nmax(1800), NMAX_MAX);
        // FST4W's standard beacon intervals must all be reachable — the reason
        // this became parametric.
        for p in [120u16, 300, 900, 1800] {
            assert!(period_supported(p), "FST4W needs {p}s");
        }
        for p in [0u16, 1, 10, 20, 45, 600, 3600] {
            assert!(!period_supported(p), "period {p} must not be supported");
        }
    }

    #[test]
    fn noise_decodes_to_nothing() {
        // Same property the C smoke test asserts, through the Rust surface: the
        // decoder runs to completion on pure noise and invents nothing. With no
        // FST4 TX in-tree there is no way to synthesise a signal, so this is a
        // liveness + silence check, NOT a sensitivity test.
        // Both modes, at the shortest period so the suite stays quick.
        for wspr in [false, true] {
            let mut iwave = vec![0i16; nmax(15)];
            let mut seed: u32 = 0x5EED ^ u32::from(wspr);
            for s in iwave.iter_mut() {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *s = ((seed >> 16) as i16) / 8;
            }
            let d = decode_frame(&iwave, 15, wspr, 200, 2900, 3, "KD9TAW", "W1AW", 0, 1500);
            assert!(
                d.is_empty(),
                "wspr={wspr} decoded {} signal(s) from noise",
                d.len()
            );
        }
    }
}
