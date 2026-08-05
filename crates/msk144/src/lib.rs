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
//! # TX since 0.20.0 (was decode-only)
//! `encode` is wired here and `ModeKind::Msk144` reports `Capabilities
//! { tx: true }`. The three deliberate steps the old decode-only note demanded —
//! C-ABI entry points, the capability flip, the FT-mode TX approval gate — all
//! happened; any further TX change still passes that gate.
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

use tempo_fast_sys::modem_lock;

pub use tempo_fast_sys::{
    msk144_nmax as nmax, msk144_period_supported as period_supported, MSK144_BAUD as BAUD,
    MSK144_NMAX_MAX as NMAX_MAX, MSK144_NN as NN, MSK144_NSPS as NSPS, MSK144_PERIODS as PERIODS,
};

/// WSJT-X audio sample rate (Hz).
pub const SAMPLE_RATE: f32 = 12_000.0;

/// The nominal audio CENTRE frequency for an MSK144 transmission, in Hz.
///
/// ⭐ FIXED, unlike every other mode here — MSK144 does not follow the operator's
/// TX offset. The signal is 1000 Hz wide (two tones at centre ±500), so it occupies
/// most of a normal SSB passband and there is nowhere to move it to. Upstream
/// hardcodes the same thing: `mainwindow.cpp:12763` sets `f0=1000.0` with a 1000 Hz
/// tone spacing, putting the tones at 1000 and 2000 Hz — a 1500 Hz centre.
pub const TX_CENTRE_HZ: f32 = 1500.0;

/// Tone separation in Hz: `BAUD/2` = 1000. Not a free parameter — an index of 0.5
/// is what makes this minimum-shift keying rather than arbitrary FSK.
pub const TONE_SPACING_HZ: f32 = BAUD / 2.0;

/// One frame's duration in seconds: 144 symbols at 2000 baud = 72 ms.
pub fn frame_secs() -> f32 {
    (NN * NSPS) as f32 / SAMPLE_RATE
}

/// How long the radio stays keyed for one period, in seconds — the TX WINDOW.
///
/// From `tx_duration()` in WSJT-X's `helper_functions.cpp:29`: `trPeriod - 0.25`.
/// ⭐ MSK144 KEYS FOR ESSENTIALLY THE WHOLE PERIOD, unlike every other mode here,
/// which sends one short over and listens. Meteor scatter works by transmitting
/// continuously and hoping one 72 ms frame finds a reflection.
///
/// ⚠️ **THIS IS THE WINDOW, NOT THE AUDIO LENGTH — see [`tx_audio_secs`].** Upstream
/// feeds this value straight into `tx2`, the end of the slot-relative window it
/// re-tests every tick (`mainwindow.cpp:5008-5012`). It is how long PTT may be held.
/// Budgeting the WAVEFORM from it over-generates by a quarter second, which is the
/// entire boundary margin — that was the bug, and the citation above was right about
/// what the function returns while being wrong about what it means.
pub fn tx_duration_secs(period_s: u16) -> f32 {
    f32::from(period_s) - 0.25
}

/// How much AUDIO one period carries, in seconds — always shorter than the window.
///
/// From `Modulator.cpp:275`, the fast-mode branch: `i1 = m_TRperiod*48000.0 - 24000.0`.
/// At 48 kHz those 24000 samples are **0.5 s**, so upstream's MSK144 waveform is
/// `period - 0.5` — a quarter second shorter than [`tx_duration_secs`].
///
/// That difference is the whole margin between the end of the audio and the slot
/// boundary. Measured with the waveform budgeted from the window instead: 70 / 30 /
/// 62 / 14 ms for the 5 / 10 / 15 / 30 s periods (10 ms for an MSK40 shorthand at any
/// period). **The radio loop ticks every 20 ms**, so MSK144-30 could not survive a
/// single tick, let alone a CAT exchange. Budgeted from here it is 286 / 318 / 278 /
/// 302 ms instead.
///
/// The cost is 3 of 204 frame repeats at 15 s — 1.5% fewer chances that a meteor
/// trail catches a frame, and exactly what upstream sends.
pub fn tx_audio_secs(period_s: u16) -> f32 {
    f32::from(period_s) - 0.5
}

/// Encode a message into MSK144 channel symbols (bits, 0 or 1).
///
/// Returns the symbols actually generated: **144 for a full frame, or 40 for an
/// MSK40 shorthand** (`<Call_1 Call2> Rpt`). `None` if the message will not pack.
///
/// ⚠️ Check the LENGTH. Assuming 144 on a shorthand frame would transmit 104
/// symbols of padding as if they were message.
pub fn encode(msg: &str) -> Option<Vec<i32>> {
    let c = std::ffi::CString::new(msg).ok()?;
    let mut itone = vec![0i32; NN];
    let n = {
        let _guard = modem_lock();
        unsafe {
            tempo_fast_sys::msk144_encode_msg(
                c.as_ptr(),
                msg.len() as std::os::raw::c_int,
                itone.as_mut_ptr(),
            )
        }
    };
    if n != 40 && n as usize != NN {
        return None;
    }
    itone.truncate(n as usize);
    Some(itone)
}

/// Synthesise one period of MSK144 audio: the frame REPEATED to fill the over.
///
/// `f0` is the CENTRE frequency; the two tones land at `f0 ± BAUD/4` (±500 Hz).
/// Returns `None` unless `itone` is 40 or 144 bits and the period is supported.
///
/// ⭐ THE REPETITION IS THE MODE. A 72 ms frame is sent over and over for
/// `period − 0.5 s` (the AUDIO budget, not the keying window) — ~201 copies in a 15 s
/// period. The far end may hear exactly
/// one of them, off a meteor trail lasting a tenth of a second. Sending the frame
/// once, as every other mode here does, would make the mode look like it worked
/// and essentially never complete a contact.
///
/// Whole frames only: a truncated trailing copy carries no decodable message, so
/// the over ends on a frame boundary and the remainder is silence.
pub fn gen_wave(itone: &[i32], period_s: u16, fsample: f32, f0: f32) -> Option<Vec<f32>> {
    if (itone.len() != 40 && itone.len() != NN) || itone.iter().any(|&t| !(0..=1).contains(&t)) {
        return None;
    }
    if !period_supported(period_s) {
        return None;
    }
    let nsps_out = (NSPS as f32 * fsample / SAMPLE_RATE).round().max(1.0) as usize;
    let frame_len = itone.len() * nsps_out;
    // The AUDIO budget, not the keying window — see `tx_audio_secs`.
    let budget = (tx_audio_secs(period_s) * fsample) as usize;
    let nreps = budget / frame_len;
    if nreps == 0 {
        return None;
    }

    // Continuous phase across symbols AND across frame repeats — the phase is never
    // reset, exactly as msk144sim.f90 builds it. A reset at each frame boundary
    // would splatter ~204 times per over.
    let dt = 1.0 / f64::from(fsample);
    let tau = std::f64::consts::TAU;
    let lo = tau * f64::from(f0 - BAUD / 4.0) * dt;
    let hi = tau * f64::from(f0 + BAUD / 4.0) * dt;

    let mut wave = vec![0f32; nreps * frame_len];
    let mut phi = 0f64;
    let mut k = 0usize;
    for _ in 0..nreps {
        for &bit in itone {
            let dphi = if bit == 0 { lo } else { hi };
            for _ in 0..nsps_out {
                wave[k] = phi.cos() as f32;
                phi = (phi + dphi) % tau;
                k += 1;
            }
        }
    }
    Some(wave)
}

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
/// docs.
///
/// `nfa..=nfb` is **accepted for ABI uniformity and otherwise ignored.** MSK144
/// does not search a band: the signal is 1000 Hz wide and always centred on
/// [`TX_CENTRE_HZ`] — [`gen_wave`] ignores `f0` for the same reason — so the
/// decoder pins that centre and searches only WSJT-X's ±50 Hz frequency-error
/// budget (`Ftol_MSK144`, mainwindow.cpp:1444). Sizing the tolerance from the
/// passband instead cost 2.55 s per 15 s period and would accept a signal a
/// kilohertz off frequency; see the note in `msk144_cabi.f90`.
///
/// `nfqso` is the QSO/RX audio frequency, honoured only inside upstream's
/// 1400..1600 clamp; anything else (including 0) falls back to the fixed centre.
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
        let _guard = modem_lock();
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
    fn encode_produces_a_valid_2fsk_frame() {
        let t = encode("K1ABC W9XYZ EN37").expect("packs");
        assert_eq!(t.len(), NN, "a full MSK144 frame is 144 symbols");
        assert!(
            t.iter().all(|&x| (0..=1).contains(&x)),
            "MSK144 is 2-FSK — every symbol is a BIT"
        );
    }

    #[test]
    fn the_frame_is_repeated_to_fill_the_over() {
        // ⭐ THE REPETITION IS THE MODE. A 72 ms frame goes out over and over for
        // period-0.25 s; the far end may hear exactly one copy off a meteor trail
        // lasting a tenth of a second. Sending it once — as every other mode here
        // does — would look correct in a file and essentially never complete a
        // contact on the air.
        let t = encode("K1ABC W9XYZ EN37").unwrap();
        for &p in PERIODS.iter() {
            let w = gen_wave(&t, p, SAMPLE_RATE, TX_CENTRE_HZ).expect("supported");
            let frames = w.len() / (NN * NSPS);
            assert!(frames > 50, "MSK144-{p} sent only {frames} frames");
            assert_eq!(w.len() % (NN * NSPS), 0, "MSK144-{p} ended mid-frame");
            // ⚠️ AGAINST THE AUDIO LENGTH, NOT THE KEYING WINDOW. This used to compare against
            // `tx_duration_secs` (period - 0.25), which is the PTT HOLD — so it passed while the
            // audio was over-generated by a quarter second, and that quarter second was the
            // entire boundary margin.
            assert!(
                w.len() as f32 / SAMPLE_RATE <= tx_audio_secs(p),
                "MSK144-{p} generates more audio than upstream: {:.3} s against a {:.3} s budget",
                w.len() as f32 / SAMPLE_RATE,
                tx_audio_secs(p),
            );
            // …and the margin that leaves before the slot boundary must survive the radio loop's
            // own 20 ms tick plus a CAT exchange. ⚠️ MEASURED AFTER THE PTT TAIL, which is what
            // actually decides whether the over crosses: the loop holds PTT for
            // `TX_TAIL_MS` = 250 ms past the audio (tempo-audio/src/slot.rs), so the real margin
            // is `period - audio - tail`, not `period - audio`. Measured before this fix:
            // 70 / 30 / 62 / 14 ms for 5 / 10 / 15 / 30 s — MSK144-30 could not survive a
            // single 20 ms tick.
            const TAIL_S: f32 = 0.25;
            let margin_ms = (f32::from(p) - w.len() as f32 / SAMPLE_RATE - TAIL_S) * 1000.0;
            assert!(
                margin_ms >= 250.0,
                "MSK144-{p} leaves only {margin_ms:.0} ms between the PTT tail and the boundary \
                 — the loop ticks every 20 ms and a CAT exchange can spend far more"
            );
        }
        let w15 = gen_wave(&t, 15, SAMPLE_RATE, TX_CENTRE_HZ).unwrap();
        assert_eq!(
            w15.len() / (NN * NSPS),
            201,
            // 201, not the 204 this asserted before: the waveform is now budgeted from the AUDIO
            // length (period - 0.5) rather than the keying window (period - 0.25), so three frame
            // repeats are given back to the boundary margin. That is 1.5% fewer chances a meteor
            // trail catches a frame — and it is exactly what upstream sends.
            "~201 frames in a 15 s interval"
        );
    }

    #[test]
    fn the_two_tones_sit_one_baud_half_apart() {
        // Minimum-shift keying is CPFSK at modulation index 0.5. The spacing is not
        // a free parameter: at 2000 baud the tones are 1000 Hz apart, centred on
        // TX_CENTRE_HZ, giving 1000 and 2000 Hz — exactly what upstream emits.
        assert_eq!(TONE_SPACING_HZ, BAUD / 2.0);
        assert_eq!(TONE_SPACING_HZ, 1000.0);
        assert_eq!(TX_CENTRE_HZ - BAUD / 4.0, 1000.0);
        assert_eq!(TX_CENTRE_HZ + BAUD / 4.0, 2000.0);
        assert!(
            (frame_secs() - 0.072).abs() < 1e-6,
            "144 bits at 2000 baud = 72 ms"
        );
    }

    #[test]
    fn an_unsupported_period_or_symbol_is_refused() {
        let t = encode("K1ABC W9XYZ EN37").unwrap();
        assert!(gen_wave(&t, 20, SAMPLE_RATE, TX_CENTRE_HZ).is_none());
        let mut bad = t.clone();
        bad[3] = 2;
        assert!(gen_wave(&bad, 15, SAMPLE_RATE, TX_CENTRE_HZ).is_none());
        assert!(gen_wave(&t[..100], 15, SAMPLE_RATE, TX_CENTRE_HZ).is_none());
    }

    #[test]
    fn encode_then_decode_recovers_the_message() {
        const MSG: &str = "K1ABC W9XYZ EN37";
        for &p in PERIODS.iter() {
            let t = encode(MSG).unwrap();
            let w = gen_wave(&t, p, SAMPLE_RATE, TX_CENTRE_HZ).unwrap();
            let mut iwave = vec![0i16; nmax(p)];
            for (i, &v) in w.iter().enumerate() {
                if i < iwave.len() {
                    iwave[i] = (v * 8000.0) as i16;
                }
            }
            let d = decode_frame(&iwave, p, 0, 300, 2700, 3, "", "", 1500);
            assert!(
                d.iter().any(|r| r.message.trim() == MSG),
                "MSK144-{p} did not decode its own transmission: {:?}",
                d.iter().map(|r| r.message.trim()).collect::<Vec<_>>()
            );
        }
    }

    /// Build one period of MSK144 centred on `f0` at a healthy level.
    fn one_period_at(msg: &str, period_s: u16, f0: f32) -> Vec<i16> {
        let t = encode(msg).unwrap();
        let w = gen_wave(&t, period_s, SAMPLE_RATE, f0).unwrap();
        let mut iwave = vec![0i16; nmax(period_s)];
        for (i, &v) in w.iter().enumerate() {
            if i < iwave.len() {
                iwave[i] = (v * 8000.0) as i16;
            }
        }
        iwave
    }

    #[test]
    fn a_signal_off_the_msk144_centre_is_not_reported() {
        // ⭐ THE TOLERANCE IS A FREQUENCY-ERROR BUDGET, NOT A BAND SEARCH. MSK144
        // lives at a FIXED centre — gen_wave ignores f0 for exactly this reason
        // (the signal is 1000 Hz wide and fills the passband, so there is nowhere
        // to move it), and upstream pins both spin boxes to 1500 with the RX one
        // clamped to 1400..1600 (mainwindow.cpp:8097-8099) and Ftol_MSK144
        // defaulting to 50 (mainwindow.cpp:1444). So ntol covers rig offset and
        // Doppler, nothing more.
        //
        // Deriving ntol from the caller's passband instead — max(20,(nfb-nfa)/2),
        // which is 1350 Hz at the default 200..2900 — was a category error twice
        // over: it made msk144sync's search `2*nint(ntol/delf)+1` bins wide, i.e.
        // 27x the work upstream does (the ~10 s decode), and it opened
        // msk144spd's plausibility gate `abs(detfer(il)) <= ntol` wide enough to
        // accept a detection a kilohertz off frequency. This is that second half:
        // a station 1000 Hz from the centre is not an MSK144 contact, and WSJT-X
        // would never report it.
        let iwave = one_period_at("K1ABC W9XYZ EN37", 15, 2500.0);
        let d = decode_frame(&iwave, 15, 0, 200, 2900, 3, "", "", 1500);
        assert!(
            !d.iter().any(|r| r.message.trim() == "K1ABC W9XYZ EN37"),
            "a signal 1000 Hz off the MSK144 centre was reported as a decode: {:?}",
            d.iter().map(|r| r.message.trim()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_centre_is_the_modes_own_not_the_operators_rx_offset() {
        // The other half, and the one that makes narrowing ntol safe: because our
        // TX ignores f0, the far end is always at TX_CENTRE_HZ. An operator whose
        // RX offset still carries an FT8 habit (1200 is typical) must not lose the
        // mode — so nfqso_in cannot be allowed to drag a +/-50 Hz window off the
        // only frequency the signal is ever on.
        const MSG: &str = "K1ABC W9XYZ EN37";
        let iwave = one_period_at(MSG, 15, TX_CENTRE_HZ);
        for nfqso in [0, 700, 1200, 1500, 2500] {
            let d = decode_frame(&iwave, 15, 0, 200, 2900, 3, "", "", nfqso);
            assert!(
                d.iter().any(|r| r.message.trim() == MSG),
                "an RX offset of {nfqso} Hz lost a signal on the MSK144 centre: {:?}",
                d.iter().map(|r| r.message.trim()).collect::<Vec<_>>()
            );
        }
    }

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
            let d = decode_frame(
                &iwave,
                p,
                i as i32 + 1,
                200,
                2900,
                3,
                "KD9TAW",
                "W1AW",
                1500,
            );
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
