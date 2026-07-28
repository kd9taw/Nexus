//! Safe Rust wrapper over `libtempo`'s native JT65 decoder.
//!
//! JT65 (WSJT) is the classic weak-signal and **EME** mode: 65-tone MFSK, one tone
//! per 372 ms, carrying a 72-bit message through a (63,12) Reed-Solomon code. It is
//! the mode that made amateur moonbounce routine, and it is still worked on 2 m and
//! up alongside Q65.
//!
//! This wraps `jt65_decode_frame` in `jt65_cabi.f90`, which drives the vendored
//! WSJT-X `jt65_decoder`.
//!
//! # DECODE ONLY — no encode, no gen_wave
//! `ModeKind::Jt65` reports `Capabilities { tx: false }`, so `modes::tx_mode()`
//! refuses to hand it to the transmit path. Adding TX means adding those entry
//! points in the C ABI, flipping that flag, AND passing the FT-mode TX approval
//! gate.
//!
//! # ⭐ The legacy message layer
//! JT65 predates 77-bit. Decodes come back as **22 characters** from the old
//! `packjt`, not the 37 that `packjt77` modes produce. Nothing downstream needs to
//! care — [`Decode::message`] is just shorter — but anything assuming a 37-char
//! ceiling is assuming the wrong thing about this mode.
//!
//! # ⭐ 60 s frame, 52 s decoded
//! `iwave` must hold [`NMAX`] (720000) samples — the full minute — because the
//! underlying Fortran dummy is explicit-shape. The decoder reads only [`NPTS`]
//! (624000, the first 52 s), matching upstream. The tail is buffer it never
//! touches; supplying it is the contract, not waste.
//!
//! # ⭐ Every call is independent
//! JT65 supports multi-period message averaging. The ABI pins `clearave` so those
//! arrays are cleared at the top of every decode and frame N is never influenced by
//! frames 1..N-1 — the same reasoning as Q65, and for the same reason: without it a
//! batch of frames contaminates itself.
//!
//! # Not KVASD
//! JT65's historical Reed-Solomon decoder was the non-free KVASD binary, shelled
//! out to as a subprocess. This build uses **ftrsd**, the Franke-Taylor
//! soft-decision decoder written to replace it, and neither ships nor invokes
//! KVASD. The ftrsd RS codec is Phil Karn's (KA9Q) under a separate, explicitly
//! stated GPL grant — see `NOTICE`.
//!
//! # Thread safety
//! Not thread-safe; serializes behind [`tempo_fast_sys::MODEM_LOCK`]. `jt65_mod`
//! holds the shared symbol-spectra state (`s1` alone is 258 KB) and the decoder
//! keeps SAVE state besides; see `modem-state-manifest.toml`.

use tempo_fast_sys::MODEM_LOCK;

pub use tempo_fast_sys::{JT65_NMAX as NMAX, JT65_NPTS as NPTS, JT65_NSUBMODES as NSUBMODES};

/// WSJT-X audio sample rate (Hz).
pub const SAMPLE_RATE: f32 = 12_000.0;

/// JT65's T/R period, in seconds. Fixed — unlike Q65 and FST4, JT65 has only one.
pub const PERIOD_S: u16 = 60;

/// How a decode was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeType {
    /// Recovered by the Reed-Solomon decoder — an independent decode.
    ReedSolomon,
    /// Recovered by DEEP SEARCH: matched against candidate messages built from
    /// the callsigns in play, rather than decoded on its own merits. Worth
    /// distinguishing for the same reason Q65's q3 is — its confidence comes from
    /// the candidate list, which is what [`Decode::qual`] scores.
    DeepSearch,
    /// Anything else the decoder reports.
    Other(i32),
}

impl DecodeType {
    fn from_code(c: i32) -> Self {
        match c {
            1 => DecodeType::ReedSolomon,
            2 => DecodeType::DeepSearch,
            n => DecodeType::Other(n),
        }
    }
}

/// A signal recovered by [`decode_frame`].
#[derive(Debug, Clone)]
pub struct Decode {
    /// Decoded message text. At most 22 characters — see the module docs.
    pub message: String,
    /// Sync correlation metric.
    pub sync: f32,
    /// SNR estimate (dB).
    pub snr: i32,
    /// Time offset in seconds.
    pub dt: f32,
    /// Audio carrier frequency (Hz).
    pub freq: f32,
    /// How it was obtained.
    pub dtype: DecodeType,
    /// Deep-search confidence. 0 for a Reed-Solomon decode, which needs none.
    pub qual: i32,
}

// Matches JT65_MAXDEC in jt65_cabi.f90, which matches dec(50) in jt65_decode.f90.
const MAX_DECODES: usize = 50;

/// Decode every JT65 signal in one 60 s T/R period.
///
/// `iwave` must hold [`NMAX`] int16 samples at 12 kHz (the full minute).
/// `submode` is 0/1/2 for JT65A/B/C — wider spacing survives more Doppler spread
/// at the cost of sensitivity.
///
/// `nfa..=nfb` is the audio search range (Hz); `ndepth` is decode aggressiveness
/// (≤ 0 ⇒ 3) and also scales the Reed-Solomon trial budget.
/// `mycall`/`hiscall`/`hisgrid` enable a-priori and deep-search decoding; pass `""`
/// to run blind.
///
/// # Panics
/// Panics if `iwave.len() < NMAX` or `submode >= NSUBMODES`. Both are caller
/// contract violations: a short buffer would let the explicit-shape Fortran dummy
/// read past the end.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame(
    iwave: &[i16],
    submode: u8,
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    hisgrid: &str,
    nfqso: i32,
) -> Vec<Decode> {
    assert!(
        submode < NSUBMODES,
        "JT65 submode {submode} out of range (0..={} for A..C)",
        NSUBMODES - 1
    );
    assert!(
        iwave.len() >= NMAX,
        "decode_frame needs at least {NMAX} samples (the full 60 s), got {}",
        iwave.len()
    );
    let myc = std::ffi::CString::new(mycall).unwrap_or_default();
    let hisc = std::ffi::CString::new(hiscall).unwrap_or_default();
    let hisg = std::ffi::CString::new(hisgrid).unwrap_or_default();
    let mut out = vec![tempo_fast_sys::Jt65DecodeT::default(); MAX_DECODES];

    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe {
            tempo_fast_sys::jt65_decode_frame(
                iwave.as_ptr(),
                i32::from(submode),
                nfa,
                nfb,
                ndepth,
                myc.as_ptr(),
                hisc.as_ptr(),
                hisg.as_ptr(),
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
            dtype: DecodeType::from_code(r.ft),
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
    fn the_frame_contract_is_the_full_minute() {
        // The decoder reads 52 s but the dummy is explicit-shape at 60 s, so the
        // caller must supply the whole thing. If these ever diverge from
        // jt65_cabi.f90's constants the buffer contract is broken.
        assert_eq!(NMAX, 60 * 12_000);
        assert_eq!(NPTS, 52 * 12_000);
        assert!(NPTS < NMAX, "the read window must fit inside the buffer");
        assert_eq!(PERIOD_S, 60);
    }

    #[test]
    #[should_panic(expected = "submode 3 out of range")]
    fn an_out_of_range_submode_panics() {
        let iwave = vec![0i16; NMAX];
        let _ = decode_frame(&iwave, 3, 200, 2900, 3, "", "", "", 1500);
    }

    #[test]
    #[should_panic(expected = "the full 60 s")]
    fn a_short_buffer_panics() {
        // A 52 s buffer is exactly the mistake the explicit-shape dummy would turn
        // into a read past the end.
        let iwave = vec![0i16; NPTS];
        let _ = decode_frame(&iwave, 0, 200, 2900, 3, "", "", "", 1500);
    }

    #[test]
    fn noise_decodes_to_nothing_in_every_submode() {
        // Liveness + silence on noise across A/B/C. NOT a sensitivity test — with
        // no JT65 TX in-tree there is no way to synthesise a signal here.
        for sm in 0..NSUBMODES {
            let mut iwave = vec![0i16; NMAX];
            let mut seed: u32 = 0x5EED ^ u32::from(sm);
            for s in iwave.iter_mut() {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *s = ((seed >> 16) as i16) / 8;
            }
            let d = decode_frame(&iwave, sm, 200, 2900, 3, "KD9TAW", "W1AW", "EN52", 1500);
            assert!(d.is_empty(), "submode {sm} decoded {} from noise", d.len());
        }
    }

    #[test]
    fn decode_type_distinguishes_reed_solomon_from_deep_search() {
        // The distinction matters the same way Q65's idec==3 does: a deep-search
        // decode was matched against candidates built from the callsigns in play,
        // not recovered on its own merits.
        assert_eq!(DecodeType::from_code(1), DecodeType::ReedSolomon);
        assert_eq!(DecodeType::from_code(2), DecodeType::DeepSearch);
        assert_eq!(DecodeType::from_code(7), DecodeType::Other(7));
    }
}
