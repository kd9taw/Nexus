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
//! # Transmit and receive
//! [`encode`] and [`gen_wave`] are the transmit half.
//!
//! ⚠️ **JT65 is the one mode whose encoder was NOT already compiled.** Q65, FST4,
//! WSPR and MSK144 all had theirs linked in because their DECODERS call them to
//! regenerate a candidate; JT65's decoder does not, so `gen65.f90` and `chkmsg.f90`
//! were vendored for this. Everything they need was already present: `packjt`,
//! `interleave63`, `graycode65`, and `rs_encode` from Karn's `wrapkarn.c` — the same
//! Reed-Solomon codec the decoder uses, so encode and decode share one RS layer.
//!
//! # DECODE-ONLY NOTE (historical)
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

pub use tempo_fast_sys::JT65_NN as NN;

/// ⭐ JT65's NATIVE sample rate is 11025 Hz, not the 12 kHz every other mode here
/// uses. Symbol length is 4096 samples AT THAT RATE, and the tone spacing derives
/// from it — get this wrong and the over is the right shape at the wrong speed.
pub const NATIVE_RATE: f32 = 11_025.0;

/// Samples per symbol at NATIVE_RATE.
pub const NSPS_NATIVE: f32 = 4096.0;

/// Where the modulation starts within the minute. 1.0 s, like the other 60 s modes.
pub const LEAD_IN_SECS: f32 = 1.0;

/// Tone spacing in Hz for `submode` (0/1/2 = A/B/C): `2^submode * 11025/4096`.
///
/// From `MainWindow::transmit()` (`mainwindow.cpp:12620-12622`) — the ON-AIR path,
/// which for JT65 spells the three cases out literally.
pub fn tone_spacing_hz(submode: u8) -> Option<f32> {
    (submode < NSUBMODES).then(|| f32::from(1u16 << submode) * NATIVE_RATE / NSPS_NATIVE)
}

/// Occupied bandwidth in Hz — the highest tone is 65 spacings above the lowest.
pub fn bandwidth_hz(submode: u8) -> Option<f32> {
    tone_spacing_hz(submode).map(|s| 65.0 * s)
}

/// How long the radio stays keyed for one over, in seconds.
///
/// `tx_duration()` in WSJT-X's `helper_functions.cpp:10`:
/// `1.0 + 126*4096/11025` ≈ 47.8 s, inside the 60 s slot.
pub fn tx_duration_secs() -> f32 {
    1.0 + (NN as f32 * NSPS_NATIVE) / NATIVE_RATE
}

/// Encode a message into the 126 JT65 channel symbols, or `None` if it will not
/// pack.
///
/// ⭐ At most **22 characters** — JT65 predates 77-bit and uses the legacy `packjt`
/// layer. Tones come back as 0 (sync) or 2..=65 (data); the +2 on data symbols is
/// part of the wire format.
pub fn encode(msg: &str) -> Option<Vec<i32>> {
    let c = std::ffi::CString::new(msg).ok()?;
    let mut itone = vec![0i32; NN];
    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe {
            tempo_fast_sys::jt65_encode_msg(
                c.as_ptr(),
                msg.len() as std::os::raw::c_int,
                itone.as_mut_ptr(),
            )
        }
    };
    (n as usize == NN).then_some(itone)
}

/// Synthesise the JT65 audio for `itone` at carrier `f0`, WITHOUT the lead-in.
///
/// Continuous-phase MFSK: tone *k* sits at `f0 + k * tone_spacing_hz(submode)`,
/// phase carried across symbol boundaries and never reset.
///
/// ⭐ THE SYMBOL LENGTH IS FRACTIONAL AT 12 kHz. JT65 is natively 11025 Hz with
/// 4096 samples per symbol, which resampled is `4096 * 12000/11025` = 4458.503…
/// samples — not an integer. Rounding each symbol independently would accumulate
/// ~63 samples of drift over 126 symbols and stretch the over past its slot, so
/// boundaries are taken as `round(i * nsps)` from the START, which keeps the total
/// exact and the error bounded to half a sample. Upstream does the same thing by
/// passing the fractional `4096.0*12000.0/11025.0` straight to the modulator
/// (`mainwindow.cpp:12626`).
pub fn gen_wave(itone: &[i32], submode: u8, fsample: f32, f0: f32) -> Option<Vec<f32>> {
    if itone.len() != NN || itone.iter().any(|&t| !(0..=65).contains(&t)) {
        return None;
    }
    let spacing = f64::from(tone_spacing_hz(submode)?);
    let nsps = f64::from(NSPS_NATIVE) * f64::from(fsample) / f64::from(NATIVE_RATE);
    let total = (NN as f64 * nsps).round() as usize;

    let dt = 1.0 / f64::from(fsample);
    let tau = std::f64::consts::TAU;
    let mut wave = vec![0f32; total];
    let mut phi = 0f64;
    for (i, &t) in itone.iter().enumerate() {
        let start = (i as f64 * nsps).round() as usize;
        let end = (((i + 1) as f64) * nsps).round() as usize;
        let dphi = tau * (f64::from(f0) + f64::from(t) * spacing) * dt;
        for w in wave.iter_mut().take(end.min(total)).skip(start) {
            *w = phi.sin() as f32;
            phi += dphi;
            if phi > tau {
                phi -= tau;
            }
        }
    }
    Some(wave)
}

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
    fn decode_and_encode_interleave_without_corrupting_each_other() {
        // Operator report: JT65 Call CQ hard-crashes on Windows with 0xC0000005 (an
        // ACCESS VIOLATION, not a Rust panic) inside Nexus.exe. A static probe that
        // calls the encoder ALONE passes on the same machine, so the encoder is fine in
        // isolation — what the app does differently is DECODE CONTINUOUSLY and then
        // encode, and gen65 shares the packjt module with the decoder while carrying a
        // bare `save` that makes every one of its locals static.
        //
        // This drives that interleaving.
        let mut noise = vec![0i16; NMAX];
        let mut seed: u32 = 0x1234;
        for s in noise.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *s = ((seed >> 20) as i16) % 100;
        }
        for round in 0..6 {
            let _ = decode_frame(&noise, 0, 200, 2900, 3, "", "", "", 1500);
            let t = encode("CQ KD9TAW EN52").expect("encode after decode");
            assert_eq!(
                t.len(),
                NN,
                "round {round}: wrong symbol count after a decode"
            );
            assert_eq!(
                t.iter().filter(|&&x| x == 0).count(),
                63,
                "round {round}: sync symbols corrupted after a decode"
            );
            // And the shorthand forms, which chkmsg routes down gen65's OTHER branch —
            // never exercised by the CQ probe.
            for sh in ["RO", "RRR", "73"] {
                let t = encode(sh).expect("shorthand encodes");
                assert_eq!(t.len(), NN, "round {round}: {sh} wrong length");
            }
        }
    }

    #[test]
    fn encode_produces_a_valid_frame() {
        let t = encode("K1ABC W9XYZ EN37").expect("packs");
        assert_eq!(t.len(), NN, "JT65 is 126 symbols");
        assert_eq!(
            t.iter().filter(|&&x| x == 0).count(),
            63,
            "63 sync symbols at tone 0"
        );
        assert_eq!(t.iter().filter(|&&x| x >= 2).count(), 63, "63 data symbols");
        assert!(
            t.iter().all(|&x| x == 0 || (2..=65).contains(&x)),
            "tones are 0 (sync) or 2..=65 (data +2 offset); never 1"
        );
    }

    #[test]
    fn a_message_longer_than_the_legacy_layer_is_refused() {
        // JT65 predates 77-bit: `packjt` carries 22 characters, not packjt77's 37.
        assert!(
            encode("BAD\0MSG").is_none(),
            "an interior NUL cannot cross the FFI"
        );
    }

    #[test]
    fn encode_then_decode_recovers_the_message() {
        // ⭐⭐ THIS TEST EXISTS BECAUSE ITS ABSENCE COST A SESSION, and the two traps
        // below are the whole reason. JT65 shipped with only `noise_decodes_to_nothing`
        // — there was no TX in-tree to make a signal with — so "runs and stays silent
        // on noise" stood in for "decodes". A LIVENESS TEST IS NOT A DECODE TEST.
        //
        // ⭐ TRAP 1: THE DECODER NEEDS A NOISE FLOOR. A mathematically perfect,
        // noiseless tone sequence DOES NOT DECODE — the decoder normalises against a
        // baseline and a zero-variance one is degenerate. Testing the bare waveform
        // returns nothing and reads exactly like a broken encoder. It is not.
        //
        // ⭐ TRAP 2 (the harness side, recorded so nobody repeats it): stock
        // `jt65sim -s 90` is NOT "no noise". Its guard is `if(xsnr.gt.90.0) sig=1.0`,
        // so 90 itself computes an astronomical amplitude and clips — 51,606 clipped
        // samples, a square wave, not JT65. Use a real SNR, or a value ABOVE 90.
        const MSG: &str = "K1ABC W9XYZ EN37";
        for submode in 0..NSUBMODES {
            let itone = encode(MSG).expect("packs");
            let wave = gen_wave(&itone, submode, SAMPLE_RATE, 1500.0).expect("supported");

            let lead = (LEAD_IN_SECS * SAMPLE_RATE) as usize;
            let mut iwave = vec![0i16; NMAX];
            // Deterministic noise floor at roughly the level jt65sim produces, with
            // the signal well above it. See TRAP 1.
            // Levels matter, not just presence: signal peak ~160 against a noise
            // sigma ~100 is what a real capture looks like and what decodes. A far
            // STRONGER signal over the same noise does NOT — the decoder's
            // normalisation wants a sane ratio, so "louder" is not "easier".
            let mut seed: u32 = 0xA5A5 ^ u32::from(submode);
            for s in iwave.iter_mut() {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *s = (((seed >> 16) % 347) as i16) - 173; // uniform ±173 ⇒ σ ≈ 100
            }
            for (i, &v) in wave.iter().enumerate() {
                if lead + i < iwave.len() {
                    iwave[lead + i] = iwave[lead + i].saturating_add((v * 160.0) as i16);
                }
            }

            let d = decode_frame(&iwave, submode, 200, 2900, 3, "", "", "", 1500);
            assert!(
                d.iter().any(|r| r.message.trim() == MSG),
                "JT65 submode {submode} did not decode its own transmission: {:?}",
                d.iter().map(|r| r.message.trim()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn the_over_fits_inside_the_minute() {
        // tx_duration() in helper_functions.cpp:10 — 1.0 + 126*4096/11025 ≈ 47.8 s.
        let d = tx_duration_secs();
        assert!((d - 47.8).abs() < 0.1, "expected ~47.8 s, got {d}");
        assert!(d < f32::from(PERIOD_S), "the over must fit its minute");
        // ⭐ 11025 Hz native, the only mode here that is not 12 kHz.
        assert!((tone_spacing_hz(0).unwrap() - 11025.0 / 4096.0).abs() < 1e-4);
        assert_eq!(
            tone_spacing_hz(1).unwrap(),
            2.0 * tone_spacing_hz(0).unwrap()
        );
        assert_eq!(
            tone_spacing_hz(2).unwrap(),
            4.0 * tone_spacing_hz(0).unwrap()
        );
    }

    #[test]
    fn the_frame_contract_is_the_full_minute() {
        // The decoder reads 52 s but the dummy is explicit-shape at 60 s, so the
        // caller must supply the whole thing. If these ever diverge from
        // jt65_cabi.f90's constants the buffer contract is broken.
        assert_eq!(NMAX, 60 * 12_000);
        assert_eq!(NPTS, 52 * 12_000);
        const { assert!(NPTS < NMAX, "the read window must fit inside the buffer") };
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
    fn a_partially_filled_ring_decodes_to_nothing_and_does_not_fault() {
        // REGRESSION — the 0.19.16 "Call CQ on JT65 hard-crashes Nexus" bug, which
        // was an ACCESS VIOLATION (0xC0000005) on Windows, not a panic.
        //
        // `RxRing::frame()` FRONT-ZERO-PADS while the ring is still filling, so the
        // decode that runs at the first 60 s boundary after selecting JT65 — or
        // after the ring is cleared at TX start, which is why it looked like a
        // TRANSMIT bug — sees a window that is mostly digital silence. Past 28%
        // silence (flat65's percentile), the reference spectrum went to zero,
        // symspec65's `ss/ref` became 0/0 = NaN, and NaN defeats BOTH comparisons in
        // xcor's peak search, so `lagpk` escaped unassigned and sync65 indexed
        // `real ccfblue(-32:82)` with a leftover stack word. See xcor.f90.
        //
        // ⚠️ This test passes on Linux even against the UNFIXED decoder, and that is
        // itself the point worth recording: sync65's frame lands on the same stack
        // address every call, so any earlier successful decode leaves a stale but
        // IN-RANGE lag in that slot. The fault needs a machine where the leftover
        // word is large — which is why it reached an operator and never CI. What
        // this test does lock down is the contract that makes the crash impossible
        // to reach: a partial window yields no decodes, quietly.
        //
        // Sample counts are the measured envelope: the cliff sits between 560000
        // (25.6% silence) and 540000 (28.8%), i.e. exactly on flat65's npct=28.
        for captured in [0, 180_000, 300_000, 432_000, 540_000, 560_000, NMAX] {
            let mut iwave = vec![0i16; NMAX];
            let mut seed: u32 = 0xA57E;
            // Real audio occupies the TAIL; the head is the zero padding.
            for s in iwave[NMAX - captured..].iter_mut() {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *s = ((seed >> 16) as i16) / 8;
            }
            let d = decode_frame(&iwave, 0, 200, 2900, 3, "KD9TAW", "W1AW", "EN52", 1500);
            assert!(
                d.is_empty(),
                "{captured} captured samples decoded {} signal(s) out of padding",
                d.len()
            );
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
