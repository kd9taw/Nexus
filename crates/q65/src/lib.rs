//! Safe Rust wrapper over `libtempo`'s native Q65 decoder.
//!
//! Q65 (WSJT-X) is the weak-signal mode for EME, VHF+ scatter and other paths where
//! the signal is buried and Doppler-smeared: 65-tone FSK carrying a 78-bit payload
//! through a Q-ary Repeat-Accumulate LDPC code over GF(64). This wraps the vendored
//! WSJT-X GPL decoder (`q65_decode_frame` in `q65_cabi.f90`), which drives the OO
//! `q65_decoder` (ana64 → q65_dec0 → q65_loops → q65_dec1/q65_dec2) down into Nico
//! Palermo's (IV3NWV) qracodes C layer.
//!
//! # Transmit and receive
//! [`encode`] and [`gen_wave`] are the transmit half. Q65 shipped decode-only
//! first; adding the encoder was cheap because `genq65` and the qracodes C layer
//! were already compiled into `libtempo` — the DECODER calls `genq65` to regenerate
//! a candidate's tone sequence, so encode and decode share one symbol generator and
//! cannot drift apart.
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
    Q65_NN as NN, Q65_NSPS as NSPS, Q65_NSUBMODES as NSUBMODES, Q65_PERIODS as PERIODS,
};

/// WSJT-X audio sample rate (Hz).
pub const SAMPLE_RATE: f32 = 12_000.0;

/// Symbol length in samples at 12 kHz for `period_s`, or `None` if unsupported.
fn nsps_for(period_s: u16) -> Option<usize> {
    PERIODS.iter().position(|&p| p == period_s).map(|i| NSPS[i])
}

/// The lead-in silence before the first symbol, in seconds.
///
/// ⭐ 0.5 s at 15/30 s, 1.0 s at 60 s and above — NOT a constant. Two independent
/// places in WSJT-X 3.0.2 agree: `q65sim.f90:164` (`k=(xdt+0.5)*12000`, and
/// `(xdt+1.0)` when `ntrperiod.ge.60`) and the over-length table in
/// `helper_functions.cpp:11` (`0.5 + 85*nsps/12000` for 15/30, `1.0 + …` for 60+).
pub fn lead_in_secs(period_s: u16) -> f32 {
    if period_s >= 60 {
        1.0
    } else {
        0.5
    }
}

/// Total on-air length of one Q65 over, in seconds: lead-in plus 85 symbols.
///
/// Matches `tx_duration()` in WSJT-X's `helper_functions.cpp` — the single upstream
/// source of truth for how long a transmission lasts. Ported rather than re-derived.
pub fn tx_duration_secs(period_s: u16) -> Option<f32> {
    let nsps = nsps_for(period_s)?;
    Some(lead_in_secs(period_s) + (NN * nsps) as f32 / SAMPLE_RATE)
}

/// Encode a message into the 85 Q65 channel symbols, or `None` if it will not pack.
///
/// Period and submode are deliberately absent: neither changes the symbol VALUES,
/// only how long each is held ([`NSPS`]) and how far apart the tones sit — both
/// applied by [`gen_wave`]. Upstream's `genq65` takes neither either.
pub fn encode(msg: &str) -> Option<Vec<i32>> {
    let c = std::ffi::CString::new(msg).ok()?;
    let mut itone = vec![0i32; NN];
    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe {
            tempo_fast_sys::q65_encode_msg(
                c.as_ptr(),
                msg.len() as std::os::raw::c_int,
                itone.as_mut_ptr(),
            )
        }
    };
    if n as usize != NN {
        return None; // pack77 refused the message
    }
    Some(itone)
}

/// Tone spacing in Hz for a period/submode, or `None` if unsupported.
///
/// `spacing = (12000 / nsps) * 2**submode` — WSJT-X's `lib/q65params.f90`
/// (`spacing=baud*2**(j-1)`), `q65sim.f90:176` and `mainwindow.cpp:12721` all agree.
pub fn tone_spacing_hz(period_s: u16, submode: u8) -> Option<f32> {
    if submode >= NSUBMODES {
        return None;
    }
    let nsps = nsps_for(period_s)?;
    Some((SAMPLE_RATE / nsps as f32) * f32::from(1u16 << submode))
}

/// Occupied bandwidth in Hz — 65 tones at [`tone_spacing_hz`].
///
/// ⭐ Q65 gets WIDE fast at short periods, and this is an operating constraint, not
/// trivia. Upstream's own table (`lib/q65params.f90`):
///
/// | | A | B | C | D | E |
/// |---|---|---|---|---|---|
/// | 15 s | 433 | 867 | 1733 | 3467 | **6933** |
/// | 30 s | 217 | 433 | 867 | 1733 | 3467 |
/// | 60 s | 108 | 217 | 433 | 867 | 1733 |
/// | 120 s | 49 | 98 | 195 | 390 | 780 |
/// | 300 s | 19 | 38 | 75 | 150 | 301 |
///
/// Q65-15E at 6933 Hz does not fit below the 6 kHz Nyquist of a 12 kHz audio path
/// at all — see [`gen_wave`], which refuses it.
pub fn bandwidth_hz(period_s: u16, submode: u8) -> Option<f32> {
    tone_spacing_hz(period_s, submode).map(|s| 65.0 * s)
}

/// Synthesise the Q65 audio for `itone` at carrier `f0`, WITHOUT the lead-in.
///
/// Returns `None` if the period or submode is unsupported, or `itone` is not 85
/// symbols long. The caller positions it in the slot — see `Q65Mode::gen_wave`.
///
/// ⭐ `submode` is not cosmetic: tone spacing is `(12000/nsps) << submode`, so
/// getting it wrong transmits at submode-A spacing and nobody can decode you. See
/// the Fortran for how easy that is to get wrong from WSJT-X's source.
pub fn gen_wave(
    itone: &[i32],
    period_s: u16,
    submode: u8,
    fsample: f32,
    f0: f32,
) -> Option<Vec<f32>> {
    if itone.len() != NN || submode >= NSUBMODES {
        return None;
    }
    let nsps = nsps_for(period_s)?;
    // ⭐ REFUSE A COMBINATION THAT CANNOT FIT IN THE AUDIO CHANNEL.
    //
    // The highest Q65 tone sits at f0 + 64*spacing. Above Nyquist it does not
    // simply sound wrong — it ALIASES back down into the passband as an emission
    // on a frequency we did not intend, which is a spurious-emission problem, not
    // a UX one. Q65-15E is 6933 Hz wide and cannot fit below the 6 kHz Nyquist of a
    // 12 kHz path at any carrier; 15D at 3467 Hz fits only near the bottom.
    //
    // ⚠️ DEVIATION FROM WSJT-X, deliberate and in the safe direction: upstream lets
    // the operator select any of the 25 combinations and does not check this. We
    // return None, which `Q65Mode::gen_wave` turns into an empty waveform, so the
    // radio loop keys nothing rather than splattering.
    let top = f0 + 64.0 * tone_spacing_hz(period_s, submode)?;
    if top >= fsample / 2.0 {
        return None;
    }
    let mut wave = vec![0f32; NN * nsps];
    let n = {
        let _guard = MODEM_LOCK.lock().unwrap();
        unsafe {
            tempo_fast_sys::q65_gen_wave(
                itone.as_ptr(),
                NN as std::os::raw::c_int,
                std::os::raw::c_int::from(period_s as i16),
                std::os::raw::c_int::from(submode),
                fsample,
                f0,
                wave.as_mut_ptr(),
                wave.len() as std::os::raw::c_int,
            )
        }
    };
    if n as usize != wave.len() {
        return None;
    }
    Some(wave)
}

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
    fn encode_then_decode_recovers_the_message_at_every_period_and_submode() {
        // THE round-trip that matters: our encoder's waveform, decoded by our own
        // decoder. Both sides come from the same vendored `genq65`, so this proves
        // the WAVEFORM layer — period → symbol duration, submode → tone spacing,
        // continuous phase — not the symbol packing.
        //
        // 15 s and 30 s only: the longer periods are the same code path with a
        // bigger buffer, and a 300 s frame is 3.6 M samples per case.
        const MSG: &str = "K1ABC W9XYZ EN37";
        for period_s in [15u16, 30] {
            for submode in 0..NSUBMODES {
                let itone = encode(MSG).expect("message packs");
                assert_eq!(itone.len(), NN);

                // Place the carrier low enough that the WHOLE 65-tone comb fits,
                // and search the band it actually occupies. Q65 gets wide fast:
                // 15D spans 3467 Hz, so the FT8-ish 300..2700 window that works for
                // the narrow combinations simply does not contain the signal — the
                // first run of this test read that as a broken decoder.
                let bw = bandwidth_hz(period_s, submode).unwrap();
                if 300.0 + bw >= SAMPLE_RATE / 2.0 {
                    // Q65-15E (6933 Hz) cannot fit below Nyquist at any carrier.
                    assert!(
                        gen_wave(&itone, period_s, submode, SAMPLE_RATE, 300.0).is_none(),
                        "a combination too wide for the audio channel must be refused"
                    );
                    continue;
                }
                let f0 = 300.0;
                let (nfa, nfb) = (100, (f0 + bw + 200.0) as i32);

                let wave = gen_wave(&itone, period_s, submode, SAMPLE_RATE, f0)
                    .expect("period and submode are supported");

                // Position it in the slot exactly as Q65Mode::gen_wave does, then
                // pad out to the decoder's frame contract.
                let lead = (lead_in_secs(period_s) * SAMPLE_RATE) as usize;
                let mut iwave = vec![0i16; nmax(period_s)];
                for (i, &s) in wave.iter().enumerate() {
                    if lead + i < iwave.len() {
                        iwave[lead + i] = (s * 8000.0) as i16;
                    }
                }

                let d = decode_frame(
                    &iwave, period_s, submode, nfa, nfb, 3, "", "", "", 0, f0 as i32,
                );
                assert!(
                    d.iter().any(|r| r.message.trim() == MSG),
                    "Q65-{period_s}{} did not decode its own transmission: {:?}",
                    (b'A' + submode) as char,
                    d.iter().map(|r| r.message.trim()).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn the_submode_actually_changes_the_signal() {
        // Guards the trap this mode's TX path is most likely to fall into. WSJT-X's
        // 48 kHz preview path computes `toneSpacing=fsample/nsps4`, which is submode
        // A whatever the operator picked; the on-air path at mainwindow.cpp:12721
        // applies `2**nSubMode`. If we ever regressed to the preview form, every
        // submode would produce identical audio and nobody could decode us.
        let itone = encode("K1ABC W9XYZ EN37").unwrap();
        let a = gen_wave(&itone, 30, 0, SAMPLE_RATE, 1500.0).unwrap();
        let b = gen_wave(&itone, 30, 1, SAMPLE_RATE, 1500.0).unwrap();
        assert_eq!(a.len(), b.len(), "same period ⇒ same duration");
        assert!(
            a.iter().zip(&b).any(|(x, y)| (x - y).abs() > 1e-6),
            "submodes A and B produced identical audio — the 2**submode tone \
             spacing is not being applied"
        );
    }

    #[test]
    fn an_unsupported_period_or_submode_is_refused_not_clamped() {
        let itone = encode("K1ABC W9XYZ EN37").unwrap();
        assert!(
            gen_wave(&itone, 45, 0, SAMPLE_RATE, 1500.0).is_none(),
            "45 s is not a Q65 period"
        );
        assert!(
            gen_wave(&itone, 30, 5, SAMPLE_RATE, 1500.0).is_none(),
            "submode F does not exist"
        );
        assert!(
            gen_wave(&itone[..84], 30, 0, SAMPLE_RATE, 1500.0).is_none(),
            "84 symbols is not a frame"
        );
    }

    #[test]
    fn the_over_fits_inside_its_period_with_the_documented_lead_in() {
        // Ported from WSJT-X's tx_duration() — an over that overruns its T/R period
        // transmits into the partner's receive window.
        for (period_s, expect_lead) in [
            (15u16, 0.5f32),
            (30, 0.5),
            (60, 1.0),
            (120, 1.0),
            (300, 1.0),
        ] {
            let d = tx_duration_secs(period_s).expect("supported period");
            assert_eq!(
                lead_in_secs(period_s),
                expect_lead,
                "Q65-{period_s} lead-in"
            );
            assert!(
                d < f32::from(period_s),
                "Q65-{period_s} over ({d} s) overruns its period"
            );
        }
        // Spot-check against the upstream table: 0.5 + 85*3600/12000 = 26.0 s.
        assert!((tx_duration_secs(30).unwrap() - 26.0).abs() < 1e-3);
        // 1.0 + 85*7200/12000 = 52.0 s.
        assert!((tx_duration_secs(60).unwrap() - 52.0).abs() < 1e-3);
    }

    #[test]
    fn a_message_that_cannot_pack_is_refused_rather_than_encoded() {
        // The safe failure: Mode::encode turns None into an empty tone vec, which
        // gen_wave turns into an empty waveform, so the radio loop keys nothing.
        // NOTE: arbitrary text does NOT fail — Q65 declares `free_text`, and pack77
        // legitimately falls back to the 13-character free-text form. The refusal
        // path is for input pack77 rejects outright, e.g. an interior NUL, which
        // cannot even reach the Fortran.
        assert!(
            encode("HELLO WORLD 73").is_some(),
            "free text is a valid Q65 message"
        );
        assert!(
            encode("BAD\0MSG").is_none(),
            "an interior NUL cannot cross the FFI"
        );
    }

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
            let d = decode_frame(
                &iwave, p, 0, 200, 2900, 3, "KD9TAW", "W1AW", "EN52", 0, 1500,
            );
            assert!(
                d.is_empty(),
                "Q65-{p} decoded {} signal(s) from noise",
                d.len()
            );
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
