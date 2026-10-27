//! Safe Rust wrapper over `libtempo`'s native FT4 decoder.
//!
//! FT4 (WSJT-X) is the fast contest sibling of FT8: 7.5 s T/R, 4-GFSK, 103
//! channel symbols, LDPC(174,91) + CRC-14. This wraps the vendored WSJT-X GPL
//! decoder (`ft4_decode_frame` in `ft4_cabi.f90`), which drives the clean OO
//! `ft4_decoder` (getcandidates4 → sync4d → get_ft4_bitmetrics → decode174_91 →
//! subtract).
//!
//! # Thread safety
//! Not thread-safe; serializes behind [`tempo_fast_sys::MODEM_LOCK`] — the single lock
//! shared across every mode (FT1/FT8/FT4/DX1) that links `libtempo`.

use tempo_fast_sys::modem_lock;

pub use tempo_fast_sys::{FT4_NMAX as NMAX, FT4_NN as NN};

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

// Stock WSJT-X allows 200 decodes per period (decodedtext MAXDEC); matching it
// (and F4_MAXDEC in ft4_cabi.f90) so busy-band slots don't silently drop the
// weakest decodes.
const MAX_DECODES: usize = 200;

/// Encode a message (≤ 37 chars, standard 77-bit content) into FT4 channel tones
/// {0..3}. Returns the 103-tone vector, or empty on a bad message.
pub fn encode(msg: &str) -> Vec<i32> {
    let bytes = msg.as_bytes();
    let mut itone = vec![0i32; NN];
    let mut nsym: i32 = 0;
    let _guard = modem_lock();
    unsafe {
        tempo_fast_sys::ft4_encode(
            bytes.as_ptr() as *const _,
            bytes.len() as i32,
            itone.as_mut_ptr(),
            &mut nsym,
        );
    }
    if nsym < 0 {
        return Vec::new();
    }
    if (nsym as usize) <= itone.len() {
        itone.truncate(nsym as usize);
    }
    itone
}

/// Run [`encode`] on `msg` once the modem is free, for what it writes into the callsign tables
/// the decoder reads (every call `pack77` packs, the recent calls and hash memos `unpack77` keeps);
/// its tones are not wanted. The lock-free encoder (`modes::wsjt77`) makes the tones and calls
/// this, so a hashed call this station sends still resolves when it comes back. Never waits: the
/// encode runs now if the modem is free, otherwise at the start of the next modem call, before
/// that call can read a table (`tempo_fast_sys::defer_encode`).
pub fn encode_later(msg: &str) {
    tempo_fast_sys::defer_encode(tempo_fast_sys::DeferredEncoder::Ft4, msg.as_bytes());
}

/// Generate the full-length real FT4 audio frame ([`NMAX`] samples) for the given
/// tones (the shaped/ramped signal is positioned by the modem, as in `ft4sim`).
pub fn gen_wave(itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
    let mut wave = vec![0f32; NMAX];
    let mut nwave: i32 = NMAX as i32;
    let _guard = modem_lock();
    unsafe {
        tempo_fast_sys::ft4_gen_wave(
            itone.as_ptr(),
            itone.len() as i32,
            fsample,
            f0,
            wave.as_mut_ptr(),
            &mut nwave,
        );
    }
    if nwave >= 0 && (nwave as usize) <= wave.len() {
        wave.truncate(nwave as usize);
    }
    wave
}

/// Decode every FT4 signal in a 72576-sample ([`NMAX`]) int16 frame at 12 kHz.
///
/// `nfa..=nfb` is the audio search range (Hz); `ndepth` is decode aggressiveness
/// (≤ 0 ⇒ 3); `mycall`/`hiscall` enable a-priori decoding (pass `""` if unknown).
/// `nfqso` is the QSO/RX audio frequency (Hz) being worked — WSJT-X's nfqso; the
/// deep AP passes fire near it. Pass 0 (or out of `nfa..=nfb`) for band-center.
///
/// `ap_cq_only` restricts a-priori decoding to the CQ hypothesis (the vendored
/// decoder's `lapcqonly`: iaptype forced 1, one AP pass); `false` is stock.
/// There is deliberately NO `ap` on/off here: the vendored FT4 decoder has no
/// such flag — its AP passes run whenever `ndepth > 1` — so an off-switch at
/// this level would be a placebo. CQ-only is FT4's only decoder-honest AP
/// restriction.
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
    nqso_progress: i32,
    nfqso: i32,
    ap_cq_only: bool,
) -> Vec<Decode> {
    decode_frame_impl(
        iwave,
        nfa,
        nfb,
        ndepth,
        mycall,
        hiscall,
        nqso_progress,
        nfqso,
        ap_cq_only,
        None,
    )
}

/// [`decode_frame`], handing each decode to `found` the moment the decoder finds it, as WSJT-X's
/// FT4 decoder hands each to its callback (`ft4_decode.f90:456`). The decodes come in the
/// decoder's own order, the candidates within 20 Hz of `nfqso` first (`getcandidates4.f90:70-81`),
/// and each is also in the list the call returns, in the same place. FT4 has no give-up.
///
/// `found` runs on this thread inside the modem (the modem lock is held): it must not decode,
/// encode or otherwise call into the modem. A panic in it is raised again here once the modem has
/// returned; the decodes after it are not handed on.
///
/// # Panics
/// Panics if `iwave.len() < NMAX`, or if `found` panicked.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame_streaming(
    iwave: &[i16],
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    nqso_progress: i32,
    nfqso: i32,
    ap_cq_only: bool,
    found: &mut dyn FnMut(&Decode),
) -> Vec<Decode> {
    decode_frame_impl(
        iwave,
        nfa,
        nfb,
        ndepth,
        mycall,
        hiscall,
        nqso_progress,
        nfqso,
        ap_cq_only,
        Some(found),
    )
}

/// The caller's closure behind the modem's callback pointer, and a panic it raised.
struct Stream<'a> {
    found: &'a mut dyn FnMut(&Decode),
    panic: Option<Box<dyn std::any::Any + Send>>,
}

/// The modem's callback (`tempo_decode_cb`). A panic must not unwind into the Fortran: it is kept,
/// and [`decode_frame_impl`] raises it again.
unsafe extern "C" fn on_found(
    user: *mut std::os::raw::c_void,
    rec: *const tempo_fast_sys::Ft4DecodeT,
) -> std::os::raw::c_int {
    // SAFETY: `user` is the `Stream` `decode_frame_impl` passed for this call, alive and not
    // otherwise borrowed until the modem returns; `rec` points at a record the modem has just
    // written (FT4 never passes null, and a null is ignored anyway).
    let stream = unsafe { &mut *user.cast::<Stream<'_>>() };
    if stream.panic.is_some() || rec.is_null() {
        return 0;
    }
    let d = from_record(unsafe { &*rec });
    let found = &mut *stream.found;
    if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| found(&d))) {
        stream.panic = Some(panic);
    }
    0
}

#[allow(clippy::too_many_arguments)]
fn decode_frame_impl(
    iwave: &[i16],
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    nqso_progress: i32,
    nfqso: i32,
    ap_cq_only: bool,
    found: Option<&mut dyn FnMut(&Decode)>,
) -> Vec<Decode> {
    assert!(
        iwave.len() >= NMAX,
        "decode_frame needs at least {NMAX} samples, got {}",
        iwave.len()
    );
    let myc = std::ffi::CString::new(mycall).unwrap_or_default();
    let hisc = std::ffi::CString::new(hiscall).unwrap_or_default();
    let mut out = vec![tempo_fast_sys::Ft4DecodeT::default(); MAX_DECODES];

    let mut stream = found.map(|found| Stream { found, panic: None });
    let n = {
        let _guard = modem_lock();
        match stream.as_mut() {
            None => unsafe {
                tempo_fast_sys::ft4_decode_frame(
                    iwave.as_ptr(),
                    nfa,
                    nfb,
                    ndepth,
                    myc.as_ptr(),
                    hisc.as_ptr(),
                    nqso_progress,
                    nfqso,
                    ap_cq_only as i32,
                    out.as_mut_ptr(),
                    out.len() as i32,
                )
            },
            Some(stream) => unsafe {
                tempo_fast_sys::ft4_decode_frame_cb(
                    iwave.as_ptr(),
                    nfa,
                    nfb,
                    ndepth,
                    myc.as_ptr(),
                    hisc.as_ptr(),
                    nqso_progress,
                    nfqso,
                    ap_cq_only as i32,
                    out.as_mut_ptr(),
                    out.len() as i32,
                    Some(on_found),
                    (stream as *mut Stream<'_>).cast(),
                )
            },
        }
    };
    if let Some(panic) = stream.and_then(|s| s.panic) {
        std::panic::resume_unwind(panic);
    }
    if n <= 0 {
        return Vec::new();
    }
    out.iter().take(n as usize).map(from_record).collect()
}

/// One modem record as a [`Decode`].
fn from_record(r: &tempo_fast_sys::Ft4DecodeT) -> Decode {
    Decode {
        message: cstr_field(&r.message),
        sync: r.sync,
        snr: r.snr,
        dt: r.dt,
        freq: r.freq,
        nap: r.nap,
        qual: r.qual,
    }
}

/// Read a NUL-/space-padded fixed C char field into a trimmed String.
fn cstr_field(buf: &[u8]) -> String {
    let bytes: Vec<u8> = buf.iter().take_while(|&&b| b != 0).copied().collect();
    String::from_utf8_lossy(&bytes).trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every decode is handed on the moment the decoder finds it, the RX frequency's first (WSJT-X's
    /// `getcandidates4.f90:70-81` tries the candidates within 20 Hz of nfqso first), and the
    /// handed-on list IS the returned one; the streaming call returns exactly what the plain call
    /// returns.
    #[test]
    fn streaming_hands_on_each_decode_as_found_the_rx_frequency_first() {
        let mut seed = 0x51A7_u32;
        let mut iwave: Vec<i16> = (0..NMAX)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                ((seed >> 8) % 31) as i16 - 15
            })
            .collect();
        for (msg, f0, amp) in [
            ("CQ K1ABC FN31", 700.0, 3000.0),
            ("W9XYZ K2DEF EM79", 1100.0, 3000.0),
            ("CQ N3GHI FM19", 2200.0, 3000.0),
            ("W9XYZ K4JKL R-07", 1500.0, 600.0),
        ] {
            let wave = gen_wave(&encode(msg), SAMPLE_RATE, f0);
            for (s, &v) in iwave.iter_mut().zip(&wave) {
                *s = s.saturating_add((v * amp) as i16);
            }
        }
        let plain = decode_frame(&iwave, 200, 2900, 3, "W9XYZ", "", 0, 1500, false);
        let mut found = Vec::new();
        let streamed = decode_frame_streaming(
            &iwave,
            200,
            2900,
            3,
            "W9XYZ",
            "",
            0,
            1500,
            false,
            &mut |d| found.push(d.clone()),
        );
        let fields = |v: &[Decode]| -> Vec<String> { v.iter().map(|d| format!("{d:?}")).collect() };
        assert_eq!(
            fields(&streamed),
            fields(&plain),
            "the same decodes, field for field"
        );
        assert_eq!(
            fields(&found),
            fields(&streamed),
            "each handed on, in the returned order"
        );
        assert_eq!(found.len(), 4, "all four decoded: {found:?}");
        assert_eq!(
            found[0].message, "W9XYZ K4JKL R-07",
            "the RX frequency's decode is handed on first"
        );
        // The order follows the RX frequency, not the signals: moved to 2200 Hz, that station's
        // decode is the first one handed on.
        let mut first = None;
        decode_frame_streaming(
            &iwave,
            200,
            2900,
            3,
            "W9XYZ",
            "",
            0,
            2200,
            false,
            &mut |d| {
                first.get_or_insert_with(|| d.message.clone());
            },
        );
        assert_eq!(
            first.as_deref(),
            Some("CQ N3GHI FM19"),
            "with the RX frequency on 2200 Hz, its decode is handed on first"
        );
    }

    /// Encode → full-frame waveform → int16 → full-frame decode recovers the
    /// message through the whole Rust/FFI stack (clean signal).
    #[test]
    fn ft4_encode_decode_roundtrip() {
        let msg = "CQ KD9TAW EN52";
        let tones = encode(msg);
        assert_eq!(tones.len(), NN, "FT4 encodes to 103 tones");

        let wave = gen_wave(&tones, SAMPLE_RATE, 1500.0);
        assert_eq!(wave.len(), NMAX, "FT4 gen_wave fills the full frame");

        // gen_wave already positions the signal; just scale into int16, no noise.
        let mut iwave = vec![0i16; NMAX];
        for (i, &s) in wave.iter().enumerate() {
            iwave[i] = (s * 1000.0).clamp(-32768.0, 32767.0) as i16;
        }

        let decs = decode_frame(&iwave, 200, 2900, 3, "", "", 0, 0, false);
        assert!(
            decs.iter().any(|d| d.message == msg),
            "FT4 must decode its own clean signal; got {decs:?}"
        );

        // With AP restricted to CQ-only the direct (non-AP) passes are
        // untouched, so a clean signal still decodes. This pins MARSHALLING
        // ONLY — a bug that misaligned out/max_out around the new argument
        // fails loudly here — and would still pass if the flag were hard-wired
        // in ft4_cabi.f90. The behavioural pin (a frame only the deep AP passes
        // can recover, which CQ-only must NOT recover) lives in
        // tests/ap_decode.rs, where the marginal SNR it needs belongs.
        let decs_cq = decode_frame(&iwave, 200, 2900, 3, "", "", 0, 0, true);
        assert!(
            decs_cq.iter().any(|d| d.message == msg),
            "CQ-only AP must not affect direct decodes; got {decs_cq:?}"
        );
    }
}
