//! Safe Rust wrapper over `libtempo`'s native FST4 decoder.
//!
//! FST4 (WSJT-X) is the slow weak-signal sibling of FT8: 4-GFSK, 160 channel
//! symbols, LDPC(240,101)+CRC-24 for QSO messages. This wraps the vendored WSJT-X
//! GPL decoder (`fst4_decode_frame` in `fst4_cabi.f90`), which drives the clean OO
//! `fst4_decoder` (get_candidates_fst4 → sync_fst4 → fst4_downsample →
//! get_fst4_bitmetrics → decode240_101/decode240_74 + OSD).
//!
//! # Transmit and receive (FST4)
//! [`encode`] and [`gen_wave`] are the transmit half. As with Q65, the encoder
//! needed no new sources: `genfst4` and `gen_fst4wave` were ALREADY compiled,
//! because the decoder calls both — `get_fst4_tones_from_bits` for candidate tones
//! and `gen_fst4wave` to subtract a decoded signal from the spectrum.
//!
//! **FST4W (the beacon variant) also transmits**, on a schedule rather than through
//! the QSO sequencer — see `tempo_core::beacon`. `Capabilities` distinguishes them
//! with `beacon_only`, not with `tx`: both key the radio, but only FST4 has an
//! exchange to sequence.
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
    FST4_NN as NN, FST4_NSPS as NSPS, FST4_PERIODS as PERIODS,
};

/// WSJT-X audio sample rate (Hz).
pub const SAMPLE_RATE: f32 = 12_000.0;

/// Symbol length in samples at 12 kHz for `period_s`, or `None` if unsupported.
fn nsps_for(period_s: u16) -> Option<usize> {
    PERIODS.iter().position(|&p| p == period_s).map(|i| NSPS[i])
}

/// Tone spacing in Hz — `hmod * 12000 / nsps`. `hmod` is upstream's x1/x2/x4 Tone
/// Spacing option; 1 is standard.
pub fn tone_spacing_hz(period_s: u16, hmod: u8) -> Option<f32> {
    let nsps = nsps_for(period_s)?;
    Some(f32::from(hmod) * SAMPLE_RATE / nsps as f32)
}

/// Occupied bandwidth in Hz — 4 tones, so 3 spacings wide plus the shaped skirts.
///
/// FST4 is narrow by design: at 1800 s / hmod 1 the spacing is 0.089 Hz and the
/// whole signal is under half a hertz. That is the point of the mode on 2200 m.
pub fn bandwidth_hz(period_s: u16, hmod: u8) -> Option<f32> {
    tone_spacing_hz(period_s, hmod).map(|s| 3.0 * s)
}

/// How long the radio stays keyed for one over, in seconds.
///
/// Ported from `tx_duration()` in WSJT-X's `helper_functions.cpp:20`, which uses
/// `1.0 + 160*nsps/12000` at EVERY period.
///
/// ⚠️ This is the PTT hold, and it is NOT the same as [`lead_in_secs`]. At
/// FST4-15 the two genuinely differ: the key is held from t=0 for 10.6 s, but the
/// modulation starts at 0.5 s. Reading the `1.0` in this table as the signal
/// position is the mistake — see [`lead_in_secs`].
pub fn tx_duration_secs(period_s: u16) -> Option<f32> {
    let nsps = nsps_for(period_s)?;
    Some(1.0 + (NN * nsps) as f32 / SAMPLE_RATE)
}

/// Where the modulation starts within the slot, in seconds.
///
/// ⭐ 1.0 s at every period EXCEPT 15 s, which is **0.5 s**. Two independent places
/// in WSJT-X 3.0.2 agree, and both are unambiguous:
///
/// ```text
/// fst4sim.f90:116     k=nint((xdt+1.0)/dt)
///                     if(nsec.eq.15) k=nint((xdt+0.5)/dt)
///
/// fst4_decode.f90:390 xdt=(isbest-nspsec)/fs2
///                     if(ntrperiod.eq.15) xdt=(isbest-real(nspsec)/2.0)/fs2
/// ```
///
/// The transmit simulator places it and the decoder references its `dt` to the same
/// split, so a flat 1.0 s puts FST4-15 exactly 0.5 s late. It still decodes — which
/// is what makes this worth a citation rather than an eyeball. Stock `jt9` reported
/// our FST4-15 at `dt 0.5` while every other period read `0.0`, which is how it was
/// caught.
pub fn lead_in_secs(period_s: u16) -> f32 {
    if period_s == 15 {
        0.5
    } else {
        1.0
    }
}

/// Encode a message into the 160 FST4 channel symbols (0..3), or `None` if it will
/// not pack.
///
/// `wspr` selects FST4W's 50-bit beacon payload instead of FST4's 77-bit QSO
/// message. ⭐ It picks the LDPC code, not just the message shape — both yield 160
/// symbols, so a wrong value transmits a frame the far end cannot decode.
pub fn encode(msg: &str, wspr: bool) -> Option<Vec<i32>> {
    let c = std::ffi::CString::new(msg).ok()?;
    let mut itone = vec![0i32; NN];
    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe {
            tempo_fast_sys::fst4_encode_msg(
                c.as_ptr(),
                msg.len() as std::os::raw::c_int,
                std::os::raw::c_int::from(wspr),
                itone.as_mut_ptr(),
            )
        }
    };
    (n as usize == NN).then_some(itone)
}

/// Synthesise the FST4 audio for `itone` at nominal carrier `f0`, WITHOUT the
/// lead-in. `hmod` is the tone-spacing multiplier (1 | 2 | 4); 1 is standard.
///
/// GFSK-shaped with raised-cosine ramps, via upstream's own `gen_fst4wave` — not a
/// plain MFSK synthesis like Q65's. `f0` is where the signal is REPORTED; the ABI
/// applies the 1.5-tone offset that upstream's callers apply.
pub fn gen_wave(itone: &[i32], period_s: u16, hmod: u8, fsample: f32, f0: f32) -> Option<Vec<f32>> {
    if itone.len() != NN || !matches!(hmod, 1 | 2 | 4) {
        return None;
    }
    let nsps = nsps_for(period_s)?;
    let mut wave = vec![0f32; NN * nsps];
    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe {
            tempo_fast_sys::fst4_gen_wave(
                itone.as_ptr(),
                NN as std::os::raw::c_int,
                std::os::raw::c_int::from(period_s as i16),
                std::os::raw::c_int::from(hmod),
                fsample,
                f0,
                wave.as_mut_ptr(),
                wave.len() as std::os::raw::c_int,
            )
        }
    };
    (n as usize == wave.len()).then_some(wave)
}

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
    fn fst4_15_starts_half_a_second_earlier_than_every_other_period() {
        // A REAL BUG this caught, not a hypothetical. A flat 1.0 s lead-in still
        // decoded at every period — stock jt9 just reported FST4-15 at `dt 0.5`
        // while the others read 0.0. Upstream splits it in two places:
        //   fst4sim.f90:117      if(nsec.eq.15) k=nint((xdt+0.5)/dt)
        //   fst4_decode.f90:391  if(ntrperiod.eq.15) xdt=(isbest-nspsec/2.0)/fs2
        assert_eq!(lead_in_secs(15), 0.5, "FST4-15 starts at 0.5 s");
        for &p in PERIODS.iter().filter(|&&p| p != 15) {
            assert_eq!(lead_in_secs(p), 1.0, "FST4-{p} starts at 1.0 s");
        }
        // The PTT hold is a SEPARATE number and uses 1.0 at every period. The two
        // genuinely disagree at 15 s; conflating them is what caused the bug.
        assert!((tx_duration_secs(15).unwrap() - 10.6).abs() < 1e-3);
        assert_ne!(
            lead_in_secs(15),
            tx_duration_secs(15).unwrap() - 160.0 * 720.0 / 12000.0
        );
    }

    #[test]
    fn encode_produces_a_valid_4fsk_frame() {
        let t = encode("K1ABC W9XYZ EN37", false).expect("message packs");
        assert_eq!(t.len(), NN, "FST4 is 160 symbols");
        assert!(
            t.iter().all(|&x| (0..=3).contains(&x)),
            "FST4 is 4-FSK — every tone must be 0..3"
        );
    }

    #[test]
    fn the_wspr_flag_selects_a_different_code_not_just_a_message_shape() {
        // iwspr picks LDPC(240,101)+77-bit vs LDPC(240,74)+50-bit. BOTH yield 160
        // symbols, so a wrong flag transmits a well-formed frame the far end cannot
        // read — there is no length check that would catch it. Pin that they differ.
        let qso = encode("K1ABC W9XYZ EN37", false).expect("FST4 packs");
        let beacon = encode("K1ABC EN37 30", true).expect("FST4W packs");
        assert_eq!(qso.len(), beacon.len(), "both are 160 symbols");
        assert_ne!(
            qso, beacon,
            "the two codes must not produce identical tones"
        );
    }

    #[test]
    fn encode_then_decode_recovers_the_message() {
        // Our waveform through our own decoder, across the short periods. The long
        // ones are the same code path with a far bigger buffer (1800 s is 21.6 M
        // samples), so they are exercised by the stock-jt9 parity run instead.
        const MSG: &str = "K1ABC W9XYZ EN37";
        for period_s in [15u16, 30, 60] {
            let itone = encode(MSG, false).expect("packs");
            let wave = gen_wave(&itone, period_s, 1, SAMPLE_RATE, 1500.0).expect("supported");

            let lead = (lead_in_secs(period_s) * SAMPLE_RATE) as usize;
            let mut iwave = vec![0i16; nmax(period_s)];
            for (i, &v) in wave.iter().enumerate() {
                if lead + i < iwave.len() {
                    iwave[lead + i] = (v * 8000.0) as i16;
                }
            }

            let d = decode_frame(&iwave, period_s, false, 100, 2900, 3, "", "", 0, 1500);
            assert!(
                d.iter().any(|r| r.message.trim() == MSG),
                "FST4-{period_s} did not decode its own transmission: {:?}",
                d.iter().map(|r| r.message.trim()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn an_unsupported_period_or_hmod_is_refused_not_clamped() {
        let t = encode("K1ABC W9XYZ EN37", false).unwrap();
        assert!(
            gen_wave(&t, 45, 1, SAMPLE_RATE, 1500.0).is_none(),
            "45 s is not an FST4 period"
        );
        assert!(
            gen_wave(&t, 60, 3, SAMPLE_RATE, 1500.0).is_none(),
            "hmod 3 does not exist"
        );
        assert!(
            gen_wave(&t[..159], 60, 1, SAMPLE_RATE, 1500.0).is_none(),
            "159 symbols is not a frame"
        );
    }

    #[test]
    fn the_over_fits_inside_its_period_at_every_length() {
        // From WSJT-X's tx_duration(): 1.0 + 160*nsps/12000 at every period. That
        // is the PTT HOLD, not the modulation start — see lead_in_secs, where 15 s
        // differs. An over that overruns its period transmits into the partner's
        // receive window.
        for &period_s in PERIODS.iter() {
            let d = tx_duration_secs(period_s).expect("supported");
            assert!(
                d < f32::from(period_s),
                "FST4-{period_s} over ({d} s) overruns its period"
            );
        }
        // Spot-checks against the upstream table.
        assert!((tx_duration_secs(15).unwrap() - (1.0 + 160.0 * 720.0 / 12000.0)).abs() < 1e-3);
        assert!(
            (tx_duration_secs(1800).unwrap() - (1.0 + 160.0 * 134400.0 / 12000.0)).abs() < 1e-2
        );
    }

    #[test]
    fn fst4_is_extraordinarily_narrow_at_long_periods() {
        // The reason the mode exists on 2200 m. Worth pinning: if the nsps table
        // were ever mis-transcribed the bandwidth would move by orders of magnitude.
        assert!((tone_spacing_hz(15, 1).unwrap() - 12000.0 / 720.0).abs() < 1e-6);
        let widest = bandwidth_hz(15, 1).unwrap();
        let narrowest = bandwidth_hz(1800, 1).unwrap();
        assert!(
            widest > 40.0 && widest < 60.0,
            "FST4-15 ≈ 50 Hz, got {widest}"
        );
        assert!(narrowest < 0.3, "FST4-1800 is sub-hertz, got {narrowest}");
    }

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
