//! Safe Rust wrapper over `libtempo`'s native FT2 codec.
//!
//! ⭐ **This is DECODIUM's FT2** (IU8LMC, decodium3-build — a GPL-3 fork of
//! WSJT-X 3.0.0-rc1), not the abandoned upstream experiment of the same name.
//! FT4 with a halved symbol time: LDPC(174,91)+CRC14, 4-GFSK at 41.67 baud
//! (~167 Hz occupied), 2.52 s of audio in a **3.75 s** T/R period, −10.8 to
//! −12 dB. The on-air FT2 population runs Decodium, so Decodium's behaviour is
//! the compatibility baseline — not WSJT-X's, which never shipped this mode.
//!
//! Wraps `ft2_decode_frame` / `ft2_encode_msg` / `ft2_gen_wave` in
//! `ft2_cabi.f90`, which drive the vendored Decodium sources. The decode entry
//! point is `ft2_triggered_decode` — the flat subroutine Decodium's own runtime
//! calls — over one fixed [`NMAX`]-sample window per period. No slide, no
//! period label; the simplest decode shape in the whole modem family.
//!
//! # Honest context (recorded at the operator's direction to build this)
//! TempoFast runs the same 3.75 s-class cadence ~3 dB deeper. FT2 is here for
//! interop with the existing FT2 community — stations Nexus could not otherwise
//! work because they are not running Nexus.
//!
//! # Thread safety
//! Not thread-safe; every entry serializes behind
//! [`tempo_fast_sys::MODEM_LOCK`]. The decode chain keeps process-global SAVE
//! state (init-once Costas/window tables, and `ft2_downsample`'s big-FFT
//! spectrum reused across candidates within one call) — classified in
//! `modem-state-manifest.toml`, GROUP N.

use tempo_fast_sys::modem_lock;

pub use tempo_fast_sys::{
    FT2_NMAX as NMAX, FT2_NN as NN, FT2_NN2 as NN2, FT2_NSPS as NSPS, FT2_NWAVE as NWAVE,
    FT2_TRPERIOD_S as TRPERIOD_S,
};

/// WSJT-X-family audio sample rate (Hz).
pub const SAMPLE_RATE: f32 = 12_000.0;

/// FT2 baud rate and tone spacing (they are equal: h=1 GFSK): 12000/288.
pub const BAUD: f32 = SAMPLE_RATE / NSPS as f32;

/// Audio seconds in one FT2 transmission: 105 symbols at 41.67 baud = 2.52 s.
pub fn tx_audio_secs() -> f32 {
    NWAVE as f32 / SAMPLE_RATE
}

/// A signal recovered by [`decode_frame`].
#[derive(Debug, Clone)]
pub struct Decode {
    /// Decoded message text.
    pub message: String,
    /// SNR estimate (dB).
    pub snr: i32,
    /// Time offset from the nominal start, seconds (about −1.0 .. +1.0).
    pub dt: f32,
    /// Audio frequency (Hz).
    pub freq: f32,
    /// 0 = ordinary decode; 1..=4 = a-priori decode of AP type n (the decoder's
    /// `a1`..`a4` annotation). An AP decode leaned on the known calls — same
    /// meaning as FT8's AP types, worth the same display treatment.
    pub ap_type: i32,
}

// Matches the practical ceiling in ft2_cabi.f90 (MAXHITS=50 in the decoder,
// one decode per hit). The ABI's outlines block is 100 either way.
const MAX_DECODES: usize = 64;

/// Decode every FT2 signal in one 3.75 s window.
///
/// `iwave` must hold at least [`NMAX`] int16 samples at 12 kHz — always the
/// full window; the Fortran dummy is explicit-shape and reads all of it.
///
/// `nfa..=nfb` is the candidate search band (Hz); `nfqso` orders candidates
/// around the QSO frequency. `ndepth` 1..=3 (3 loosens the sync/OSD thresholds
/// like Decodium's deep setting; `<=0` defaults to 3). An empty `hiscall`
/// disables the AP types that need the DX call — decodes still happen, they
/// just don't get the a-priori help.
///
/// # Panics
/// Panics if `iwave.len() < NMAX` — a caller contract violation; a short
/// buffer would let the decoder read past the slice.
pub fn decode_frame(
    iwave: &[i16],
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    nfqso: i32,
) -> Vec<Decode> {
    assert!(
        iwave.len() >= NMAX,
        "ft2::decode_frame needs {NMAX} samples (3.75 s @ 12 kHz), got {}",
        iwave.len()
    );
    let myc = std::ffi::CString::new(mycall).unwrap_or_default();
    let hisc = std::ffi::CString::new(hiscall).unwrap_or_default();
    let mut out = vec![tempo_fast_sys::Ft2DecodeT::default(); MAX_DECODES];

    let n = {
        let _guard = modem_lock();
        unsafe {
            tempo_fast_sys::ft2_decode_frame(
                iwave.as_ptr(),
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
        .map(|d| Decode {
            message: message_text(&d.message),
            snr: d.snr,
            dt: d.dt,
            freq: d.freq,
            ap_type: d.dtype,
        })
        .collect()
}

/// Encode a message into the [`NN`] FT2 channel tones (0..=3).
///
/// Returns `None` if the message will not pack into 77 bits.
pub fn encode(message: &str) -> Option<Vec<i32>> {
    let msg = std::ffi::CString::new(message).ok()?;
    let mut itone = vec![0i32; NN];
    let n = {
        let _guard = modem_lock();
        unsafe {
            tempo_fast_sys::ft2_encode_msg(msg.as_ptr(), message.len() as i32, itone.as_mut_ptr())
        }
    };
    if n as usize != NN {
        return None;
    }
    Some(itone)
}

/// Channel tones → real audio samples at 12 kHz.
///
/// Returns [`NWAVE`] samples (2.52 s) — the generator appends the
/// raised-cosine ramp-up/ramp-down symbols itself, which is why the output is
/// [`NN2`] symbols from an [`NN`]-tone input. `f0` is the audio carrier for
/// tone 0; the occupied width above it is ~3×[`BAUD`] ≈ 125 Hz plus shaping.
///
/// Returns `None` if `itone` is not exactly [`NN`] tones.
pub fn gen_wave(itone: &[i32], f0: f32) -> Option<Vec<f32>> {
    if itone.len() != NN {
        return None;
    }
    let mut wave = vec![0f32; NWAVE];
    let n = {
        let _guard = modem_lock();
        unsafe {
            tempo_fast_sys::ft2_gen_wave(
                itone.as_ptr(),
                NN as i32,
                f0,
                wave.as_mut_ptr(),
                NWAVE as i32,
            )
        }
    };
    if n as usize != NWAVE {
        return None;
    }
    Some(wave)
}

/// NUL/space-terminated fixed field → trimmed String.
fn message_text(raw: &[u8; 38]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TX→RX loopback at clean SNR: what we encode, we must decode.
    ///
    /// This is the parity harness's foundation — encode with the vendored
    /// generator, decode with the vendored triggered decoder, and the round
    /// trip must recover the exact message text. A decoder or encoder that
    /// drifts from Decodium's breaks here first.
    #[test]
    fn a_generated_message_decodes_back_to_itself() {
        let msg = "CQ KD9TAW EN52";
        let itone = encode(msg).expect("message must pack");
        assert_eq!(itone.len(), NN);
        assert!(
            itone.iter().all(|&t| (0..=3).contains(&t)),
            "tones are 0..=3"
        );

        let wave = gen_wave(&itone, 1200.0).expect("wave must generate");
        assert_eq!(wave.len(), NWAVE);

        // Place the 2.52 s transmission at the nominal start of the 3.75 s
        // window (dt 0), at moderate amplitude, no noise.
        let mut iwave = vec![0i16; NMAX];
        // The decoder measures dt as xibest/1333.33 - 0.5, so the nominal
        // "on time" start sits half a second into the window.
        let start = (0.5 * SAMPLE_RATE) as usize;
        for (i, &s) in wave.iter().enumerate() {
            iwave[start + i] = (s * 8000.0) as i16;
        }

        let decodes = decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "", 1200);
        assert!(
            decodes.iter().any(|d| d.message == msg),
            "expected {msg:?} among {decodes:?}"
        );
    }

    /// Negative control for the loopback: an empty window decodes nothing.
    /// Without this, a decoder that hallucinated messages from silence would
    /// make the test above pass for the wrong reason.
    #[test]
    fn silence_decodes_nothing() {
        let iwave = vec![0i16; NMAX];
        let decodes = decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "", 1200);
        assert!(decodes.is_empty(), "silence produced {decodes:?}");
    }

    /// pack77 does not refuse over-long text — it truncates to 13-char FREE
    /// TEXT, the WSJT-X-family behaviour every mode here shares. Pinned so
    /// nobody "fixes" encode to refuse and breaks free-text sends.
    #[test]
    fn overlong_text_packs_as_truncated_free_text() {
        let itone = encode("THIS MESSAGE IS FAR TOO LONG TO PACK INTO 77 BITS")
            .expect("free text must pack");
        let wave = gen_wave(&itone, 1200.0).expect("wave must generate");
        let mut iwave = vec![0i16; NMAX];
        let start = (0.5 * SAMPLE_RATE) as usize;
        for (i, &s) in wave.iter().enumerate() {
            iwave[start + i] = (s * 8000.0) as i16;
        }
        let decodes = decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "", 1200);
        assert!(
            decodes.iter().any(|d| d.message == "THIS MESSAG"
                || d.message == "THIS MESSAGE"
                || d.message == "THIS MESSAGE I"),
            "expected the truncated free text among {decodes:?}"
        );
    }
}
