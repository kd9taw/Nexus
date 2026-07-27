//! Safe Rust wrapper over `libtempo`'s native Q65 decoder.
//!
//! Q65 (WSJT-X) is the weak-signal mode for EME, VHF+ scatter and other paths where
//! the signal is buried and Doppler-smeared: 65-tone FSK carrying a 78-bit payload
//! through a Q-ary Repeat-Accumulate LDPC code over GF(64). This wraps the vendored
//! WSJT-X GPL decoder (`q65_decode_frame` in `q65_cabi.f90`), which drives the OO
//! `q65_decoder` (ana64 → q65_dec0 → q65_loops → q65_dec1/q65_dec2) down into Nico
//! Palermo's (IV3NWV) qracodes C layer.
//!
//! # DECODE ONLY — no encode, no gen_wave
//! There is deliberately no `encode` or `gen_wave` here, and none in the C ABI. Q65
//! ships receive-only: `ModeKind::Q65` reports `Capabilities { tx: false }`, so
//! `modes::tx_mode()` refuses to hand it to the transmit path. Adding TX means
//! adding those entry points in `q65_cabi.f90`, flipping that flag, AND passing the
//! FT-mode TX approval gate — three deliberate steps.
//!
//! # ⭐ All 5 periods × all 5 submodes
//! Period and submode are arguments, not constants. The vendored Fortran was
//! always fully parametric — the single-period pin lived in the C ABI, not the
//! modem. That matters because Q65-30A is not the mode's main use: EME on VHF/UHF
//! runs **Q65-60A/B/C**, 6 m meteor/ionoscatter is where 30 belongs, and 15 is
//! troposcatter.
//!
//! **The frame length follows the period** — [`nmax`] of the period, from 180000
//! samples (15 s) to 3600000 (300 s). Supply exactly that many; an unsupported
//! period is rejected rather than clamped, because reading the wrong span of the
//! caller's audio yields a plausible wrong answer rather than an obvious failure.
//!
//! # ⭐ Every call is independent
//! Q65 supports multi-period message averaging, accumulating symbol-spectrum power
//! across calls. The ABI pins `lclearave` so those arrays are cleared at the top of
//! every decode: frame N is never influenced by frames 1..N-1, and [`Decode::nused`]
//! is always 1. That is deliberate — without it, a batch of frames contaminates
//! itself, which is the same trap that cost a calibration round on the FT8 side.
//!
//! # Thread safety
//! Not thread-safe; serializes behind [`tempo_fast_sys::MODEM_LOCK`] — the single
//! lock shared across every mode (FT1/FT8/FT4/FST4/Q65/DX1) that links `libtempo`.
//! Q65 adds process-global C state on top of the Fortran SAVE state: `codec` in
//! q65_subs.c and the q65_hist decode ring. See `modem-state-manifest.toml` GROUP H.

use tempo_fast_sys::MODEM_LOCK;

pub use tempo_fast_sys::{
    q65_nmax as nmax, q65_period_supported as period_supported, Q65_NMAX_MAX as NMAX_MAX,
    Q65_NSUBMODES as NSUBMODES, Q65_PERIODS as PERIODS,
};

/// WSJT-X audio sample rate (Hz).
pub const SAMPLE_RATE: f32 = 12_000.0;

/// A signal recovered by [`decode_frame`].
#[derive(Debug, Clone)]
pub struct Decode {
    /// Decoded message text.
    pub message: String,
    /// Sync-curve correlation metric (`snr1` upstream).
    pub sync: f32,
    /// SNR estimate (dB, 2500 Hz BW).
    pub snr: i32,
    /// Time offset in seconds.
    pub dt: f32,
    /// Audio carrier frequency (Hz).
    pub freq: f32,
    /// Decode type: 0 = q0, 1 = q1, 2 = q2, 3 = q3 (full-AP list decode).
    ///
    /// ⭐ `idec == 3` is worth treating differently from the rest. A q3 decode is
    /// not an independent recovery of the message: it is a match against a list of
    /// candidate messages pre-built from `mycall`/`hiscall`/`hisgrid`, so its
    /// "checksum" is the fact that the message was on the list. Upstream treats it
    /// as valid, and so does Nexus, but anything ranking decode confidence should
    /// know the difference.
    pub idec: i32,
    /// T/R periods averaged. Always 1 here — see the module docs.
    pub nused: i32,
}

// Matches Q65_MAXDEC in q65_cabi.f90, which in turn matches decodes(100) in
// q65_decode.f90. Raise both together or the weakest decodes are dropped silently.
const MAX_DECODES: usize = 100;

/// Decode every Q65 signal in one T/R period.
///
/// `iwave` must hold [`nmax`]`(period_s)` int16 samples at 12 kHz — 180000 at 15 s,
/// 3600000 at 300 s. `period_s` must be one of [`PERIODS`] and `submode` 0..=4
/// (A..E); anything else returns an empty result rather than decoding the wrong
/// window.
///
/// `nfa..=nfb` is the audio search range (Hz); `ndepth` is decode aggressiveness
/// (≤ 0 ⇒ 3); `mycall`/`hiscall`/`hisgrid` enable a-priori decoding (pass `""` if
/// unknown). `nfqso` is the QSO/RX audio frequency (Hz) being worked; the deep AP
/// passes fire near it. Pass 0 (or out of `nfa..=nfb`) for band-center.
///
/// Unlike the FT8/FT4/FST4 wrappers this takes `hisgrid`: Q65's AP layer builds its
/// candidate list from the grid as well as the callsigns.
///
/// Runs the full-band search (`nqd=0`) and never the contest path — the contest
/// mode depends on caller history that used to arrive through a file the headless
/// build removed, precisely because it let one chain's callers reach another.
///
/// # Panics
/// Panics if `iwave.len() < nmax(period_s)`, or if `period_s`/`submode` are not
/// supported. These are contract violations by the caller, not runtime conditions:
/// the buffer length and the period must be chosen together, and a silent
/// short-read would decode a window the caller never intended.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame(
    iwave: &[i16],
    period_s: u16,
    submode: u8,
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    hisgrid: &str,
    nqso_progress: i32,
    nfqso: i32,
) -> Vec<Decode> {
    assert!(
        period_supported(period_s),
        "Q65 period {period_s}s is not supported (must be one of {PERIODS:?})"
    );
    assert!(
        submode < NSUBMODES,
        "Q65 submode {submode} out of range (0..={} for A..E)",
        NSUBMODES - 1
    );
    let need = nmax(period_s);
    assert!(
        iwave.len() >= need,
        "decode_frame needs at least {need} samples for a {period_s}s period, got {}",
        iwave.len()
    );
    let myc = std::ffi::CString::new(mycall).unwrap_or_default();
    let hisc = std::ffi::CString::new(hiscall).unwrap_or_default();
    let hisg = std::ffi::CString::new(hisgrid).unwrap_or_default();
    let mut out = vec![tempo_fast_sys::Q65DecodeT::default(); MAX_DECODES];

    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe {
            tempo_fast_sys::q65_decode_frame(
                iwave.as_ptr(),
                i32::from(period_s),
                i32::from(submode),
                nfa,
                nfb,
                ndepth,
                myc.as_ptr(),
                hisc.as_ptr(),
                hisg.as_ptr(),
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
            idec: r.idec,
            nused: r.nused,
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
        // The buffer contract. Getting this wrong reads the wrong span of audio,
        // which decodes a window the caller never meant to hand over.
        for p in PERIODS {
            assert_eq!(nmax(p), p as usize * 12_000);
            assert!(period_supported(p));
        }
        assert_eq!(nmax(300), NMAX_MAX);
        // EME's actual working period must be reachable — the whole point of
        // making this parametric.
        assert!(period_supported(60), "Q65-60 is the EME workhorse");
    }

    #[test]
    fn unsupported_period_is_refused() {
        for p in [0u16, 1, 10, 20, 45, 90, 600] {
            assert!(!period_supported(p), "period {p} must not be supported");
        }
    }

    #[test]
    #[should_panic(expected = "not supported")]
    fn decoding_at_an_unsupported_period_panics() {
        // Refusing loudly beats decoding the wrong window: the modem would read
        // ntrperiod*12000 samples regardless of what the caller sized.
        let iwave = vec![0i16; nmax(30)];
        let _ = decode_frame(&iwave, 45, 0, 200, 2900, 3, "", "", "", 0, 1500);
    }

    #[test]
    #[should_panic(expected = "samples for a 60s period")]
    fn a_buffer_sized_for_the_wrong_period_panics() {
        // A 30 s buffer handed to a 60 s decode is exactly the short-read the
        // assert exists to catch.
        let iwave = vec![0i16; nmax(30)];
        let _ = decode_frame(&iwave, 60, 0, 200, 2900, 3, "", "", "", 0, 1500);
    }

    #[test]
    fn noise_decodes_to_nothing_at_every_period() {
        // Liveness + silence on noise, across the whole period range rather than
        // just the one that used to be pinned. NOT a sensitivity test — with no
        // Q65 TX in-tree there is no way to synthesise a signal here; sensitivity
        // is measured in the parity lab against stock q65sim.
        //
        // 300 s is skipped: it is a 3.6 M-sample decode and would dominate the
        // suite's runtime for no extra coverage of the code path.
        for p in [15u16, 30, 60] {
            let mut iwave = vec![0i16; nmax(p)];
            let mut seed: u32 = 0x5EED ^ u32::from(p);
            for s in iwave.iter_mut() {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *s = ((seed >> 16) as i16) / 8;
            }
            let d = decode_frame(&iwave, p, 0, 200, 2900, 3, "KD9TAW", "W1AW", "EN52", 0, 1500);
            assert!(d.is_empty(), "Q65-{p} decoded {} signal(s) from noise", d.len());
        }
    }

    #[test]
    fn every_submode_runs() {
        // A–E differ in tone spacing (mode_q65 drives LL=64*(2+mode_q65)), so each
        // takes a different path through the demodulator. All five must at least
        // run to completion and stay silent on noise.
        let mut iwave = vec![0i16; nmax(15)];
        let mut seed: u32 = 0xBEEF;
        for s in iwave.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *s = ((seed >> 16) as i16) / 8;
        }
        for sm in 0..NSUBMODES {
            let d = decode_frame(&iwave, 15, sm, 200, 2900, 3, "", "", "", 0, 1500);
            assert!(d.is_empty(), "submode {sm} decoded {} from noise", d.len());
        }
    }

    #[test]
    fn repeated_frames_do_not_contaminate_each_other() {
        // The ABI pins lclearave=.true. so Q65's multi-period averaging cannot carry
        // one frame into the next. Decoding the SAME noise twice must therefore give
        // the same answer; if averaging were live, the second call would see
        // accumulated power from the first and could differ.
        let mut iwave = vec![0i16; nmax(30)];
        let mut seed: u32 = 0xC0FFEE;
        for s in iwave.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *s = ((seed >> 16) as i16) / 8;
        }
        let a = decode_frame(&iwave, 30, 0, 200, 2900, 3, "", "", "", 0, 1500);
        let b = decode_frame(&iwave, 30, 0, 200, 2900, 3, "", "", "", 0, 1500);
        assert_eq!(a.len(), b.len(), "same frame decoded differently on replay");
    }
}
