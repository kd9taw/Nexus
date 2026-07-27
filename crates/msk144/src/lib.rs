//! Safe Rust wrapper over `libtempo`'s native MSK144 decoder.
//!
//! MSK144 (WSJT-X) is the **meteor-scatter** mode. It sends 72 ms frames (864
//! samples at 12 kHz) continuously through the T/R period, so a single ionised
//! meteor trail lasting a tenth of a second can carry an entire message. On 6 m and
//! 2 m that turns sporadic pings into workable contacts.
//!
//! This wraps `msk144_decode_frame` in `msk144_cabi.f90`, which drives the vendored
//! WSJT-X `mskrtd` decoder.
//!
//! # DECODE ONLY — no encode, no gen_wave
//! There is deliberately no `encode` or `gen_wave` here, and none in the C ABI.
//! `ModeKind::Msk144` reports `Capabilities { tx: false }`, so `modes::tx_mode()`
//! refuses to hand it to the transmit path. Adding TX means adding those entry
//! points in the C ABI, flipping that flag, AND passing the FT-mode TX approval
//! gate — three deliberate steps.
//!
//! # ⭐ A sliding-window decoder behind a frame-shaped API
//! `mskrtd` analyses ONE 7168-sample block per call and is driven at half-block
//! (~0.3 s) steps across the period — about 50 calls per 15 s. The C ABI owns that
//! slide, so [`decode_frame`] keeps the same "hand over one period, get every
//! decode back" shape as every other mode here. The cost of that convenience is
//! that MSK144 cannot report partial results mid-period through this API; a
//! streaming variant would need a different entry point.
//!
//! # ⭐ `nutc` is the period label — it must differ between periods
//! `mskrtd` suppresses duplicate decodes against the previous message and resets
//! that suppressor on `nutc00 != nutc0 || tsec < tsec0`. Both halves are live:
//! `tsec0` advances on every call via a *labelled* assignment at `mskrtd.f90:238`
//! that every exit path reaches, so the suppressor also self-clears at a period
//! boundary when `tsec` restarts near zero.
//!
//! Passing a distinct value per period is still required — it is the UTC field of
//! the decoder's output line and the other half of the reset condition. Pass the
//! period's UTC, or any per-period-distinct value.
//!
//! # Thread safety
//! Not thread-safe; serializes behind [`tempo_fast_sys::MODEM_LOCK`] — the single
//! lock shared across every mode that links `libtempo`. `mskrtd` keeps
//! process-global SAVE state (the analytic-signal ring, the duplicate-check pair,
//! the MSK40 hashed-callsign table and the recent-shorthand ring); see
//! `modem-state-manifest.toml`.

use tempo_fast_sys::MODEM_LOCK;

pub use tempo_fast_sys::{
    msk144_nmax as nmax, msk144_period_supported as period_supported,
    MSK144_NMAX_MAX as NMAX_MAX, MSK144_PERIODS as PERIODS,
};

/// WSJT-X audio sample rate (Hz).
pub const SAMPLE_RATE: f32 = 12_000.0;

/// How a decode was recovered — worth surfacing, because the two mean different
/// things to an operator watching a band open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeType {
    /// Frame-averaged: stacked across the period.
    Averaged,
    /// `&` — a single-frame "fast" decode off one bright ping (mskspd).
    SinglePing,
    /// `^` — a long average across many frames.
    LongAverage,
}

impl DecodeType {
    fn from_code(c: i32) -> Self {
        match c {
            1 => DecodeType::SinglePing,
            2 => DecodeType::LongAverage,
            _ => DecodeType::Averaged,
        }
    }
}

/// A signal recovered by [`decode_frame`].
#[derive(Debug, Clone)]
pub struct Decode {
    /// Decoded message text.
    pub message: String,
    /// SNR estimate (dB).
    pub snr: i32,
    /// Time offset within the T/R period (seconds) — when in the period the ping
    /// landed, not a WSJT-X-style `xdt`.
    pub dt: f32,
    /// Audio carrier frequency (Hz).
    pub freq: f32,
    /// How it was recovered.
    pub dtype: DecodeType,
}

// Matches MSK144_MAXDEC in msk144_cabi.f90. Raise both together or the weakest
// decodes are dropped silently.
const MAX_DECODES: usize = 100;

/// Decode every MSK144 signal in one T/R period.
///
/// `iwave` must hold [`nmax`]`(period_s)` int16 samples at 12 kHz. `period_s` must
/// be one of [`PERIODS`] (5/10/15/30; 15 is the 6 m workhorse).
///
/// `nutc` labels the period and **must differ between periods** — see the module
/// docs. `nfa..=nfb` is the audio search range (Hz); MSK144 searches a centre plus
/// tolerance internally, both derived from that range. `nfqso` is the QSO/RX audio
/// frequency; pass 0 (or out of range) for band-center.
///
/// # Panics
/// Panics if `iwave.len() < nmax(period_s)` or `period_s` is unsupported. These are
/// caller contract violations: buffer length and period must be chosen together,
/// and a silent short-read would decode a window the caller never intended.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame(
    iwave: &[i16],
    period_s: u16,
    nutc: i32,
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    nfqso: i32,
) -> Vec<Decode> {
    assert!(
        period_supported(period_s),
        "MSK144 period {period_s}s is not supported (must be one of {PERIODS:?})"
    );
    let need = nmax(period_s);
    assert!(
        iwave.len() >= need,
        "decode_frame needs at least {need} samples for a {period_s}s period, got {}",
        iwave.len()
    );
    let myc = std::ffi::CString::new(mycall).unwrap_or_default();
    let hisc = std::ffi::CString::new(hiscall).unwrap_or_default();
    let mut out = vec![tempo_fast_sys::Msk144DecodeT::default(); MAX_DECODES];

    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe {
            tempo_fast_sys::msk144_decode_frame(
                iwave.as_ptr(),
                i32::from(period_s),
                nutc,
                nfa,
                nfb,
                ndepth,
                myc.as_ptr(),
                hisc.as_ptr(),
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
            snr: r.snr,
            dt: r.dt,
            freq: r.freq,
            dtype: DecodeType::from_code(r.dtype),
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
        assert_eq!(nmax(30), NMAX_MAX);
        // 15 s is the period 6 m meteor scatter actually runs on.
        assert!(period_supported(15));
        for p in [0u16, 1, 20, 45, 60, 120] {
            assert!(!period_supported(p), "period {p} must not be supported");
        }
    }

    #[test]
    #[should_panic(expected = "not supported")]
    fn decoding_at_an_unsupported_period_panics() {
        let iwave = vec![0i16; nmax(15)];
        let _ = decode_frame(&iwave, 60, 1, 200, 2900, 3, "", "", 1500);
    }

    #[test]
    #[should_panic(expected = "samples for a 30s period")]
    fn a_buffer_sized_for_the_wrong_period_panics() {
        let iwave = vec![0i16; nmax(15)];
        let _ = decode_frame(&iwave, 30, 1, 200, 2900, 3, "", "", 1500);
    }

    #[test]
    fn noise_decodes_to_nothing() {
        // Liveness + silence on noise across the period range. NOT a sensitivity
        // test — with no MSK144 TX in-tree there is no way to synthesise a signal
        // here; sensitivity is measured in the parity lab against stock msk144sim.
        for (i, p) in [5u16, 15].into_iter().enumerate() {
            let mut iwave = vec![0i16; nmax(p)];
            let mut seed: u32 = 0x5EED ^ u32::from(p);
            for s in iwave.iter_mut() {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *s = ((seed >> 16) as i16) / 8;
            }
            // Distinct nutc per call, as the contract requires.
            let d = decode_frame(&iwave, p, i as i32 + 1, 200, 2900, 3, "KD9TAW", "W1AW", 1500);
            assert!(d.is_empty(), "MSK144-{p} decoded {} from noise", d.len());
        }
    }

    #[test]
    fn decode_type_codes_map_to_their_symbols() {
        // The C ABI encodes mskrtd's decsym; a mis-mapping would label a
        // single-ping decode as a patient long average, which is exactly backwards
        // for judging whether a band is open.
        assert_eq!(DecodeType::from_code(0), DecodeType::Averaged);
        assert_eq!(DecodeType::from_code(1), DecodeType::SinglePing);
        assert_eq!(DecodeType::from_code(2), DecodeType::LongAverage);
        // Anything unexpected degrades to the neutral reading rather than panicking
        // mid-decode.
        assert_eq!(DecodeType::from_code(99), DecodeType::Averaged);
    }
}
