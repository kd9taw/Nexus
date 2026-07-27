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
//! # ⭐ Q65-30A only
//! Upstream supports 5 T/R periods × 5 submodes (A–E, tone spacing). `q65_decode`
//! sizes its frame from the period, so the C ABI pins **30 s / 360000 samples** and
//! **submode A**. Supporting more needs a per-period entry point plus a `ModeKind`
//! that can carry one, which is why [`NMAX`] is a constant here rather than a
//! function of anything.
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

pub use tempo_fast_sys::{Q65_NMAX as NMAX, Q65_NSUBMODE as NSUBMODE, Q65_NTRPERIOD as NTRPERIOD};

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

/// Decode every Q65 signal in a 360000-sample ([`NMAX`]) int16 frame at 12 kHz.
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
/// Panics if `iwave.len() < NMAX`.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame(
    iwave: &[i16],
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
        iwave.len() >= NMAX,
        "decode_frame needs at least {NMAX} samples, got {}",
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
    fn frame_is_the_pinned_30s_length() {
        // The ABI pins one period and one submode; if either changes, q65_cabi.f90's
        // Q65_NMAX / Q65_NTRPERIOD / Q65_NSUBMODE must change with it or the buffer
        // contract breaks.
        assert_eq!(NTRPERIOD, 30);
        assert_eq!(NSUBMODE, 0);
        assert_eq!(NMAX, 30 * 12_000);
    }

    #[test]
    fn noise_decodes_to_nothing() {
        // Same property the FST4 wrapper asserts: the decoder runs to completion on
        // pure noise and invents nothing. With no Q65 TX in-tree there is no way to
        // synthesise a signal here, so this is a liveness + silence check, NOT a
        // sensitivity test — sensitivity is measured in the parity lab against
        // stock q65sim.
        let mut iwave = vec![0i16; NMAX];
        let mut seed: u32 = 0x5EED;
        for s in iwave.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *s = ((seed >> 16) as i16) / 8;
        }
        let d = decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", "EN52", 0, 1500);
        assert!(d.is_empty(), "decoded {} signal(s) from noise", d.len());
    }

    #[test]
    fn repeated_frames_do_not_contaminate_each_other() {
        // The ABI pins lclearave=.true. so Q65's multi-period averaging cannot carry
        // one frame into the next. Decoding the SAME noise twice must therefore give
        // the same answer; if averaging were live, the second call would see
        // accumulated power from the first and could differ.
        let mut iwave = vec![0i16; NMAX];
        let mut seed: u32 = 0xC0FFEE;
        for s in iwave.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *s = ((seed >> 16) as i16) / 8;
        }
        let a = decode_frame(&iwave, 200, 2900, 3, "", "", "", 0, 1500);
        let b = decode_frame(&iwave, 200, 2900, 3, "", "", "", 0, 1500);
        assert_eq!(a.len(), b.len(), "same frame decoded differently on replay");
    }
}
