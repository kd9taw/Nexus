//! Raw FFI bindings to `libtempo` — the standalone FT1 4-CPM turbo modem.
//!
//! See `tempo/libtempo/include/libtempo.h` for the authoritative ABI documentation.
//! Use the safe wrapper in the `ft1` crate rather than these raw bindings.
#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_float, c_int, c_void};
use std::sync::{Mutex, MutexGuard};

/// Serializes ALL access to the non-thread-safe `libtempo` modem.
///
/// The Fortran code uses process-global `SAVE` state (CPM pulse tables,
/// downsample filter windows) and cached FFTW plans that are **shared across
/// FT1, FT8, FT4 and DX1** (they all link this one `libtempo`). Every safe wrapper
/// (`ft1`, `ft8`, `ft4`) must serialize behind this single mutex — a per-crate
/// lock would not prevent an FT1 decode from racing an FT8 decode on the shared
/// FFTW plan cache. It lives here because this crate owns the one native library.
///
/// ⚠️ **Acquire it with [`modem_lock`], never by calling `.lock().unwrap()` on this
/// static directly.** It stays public for documentation and for the tests that reason
/// about it; the unwrap is what turned one panic into a dead radio for the session.
pub static MODEM_LOCK: Mutex<()> = Mutex::new(());

/// Acquire [`MODEM_LOCK`], recovering rather than dying if it was POISONED.
///
/// # Why this exists rather than `.lock().unwrap()`
/// Every one of these call sites used to unwrap. A `Mutex` in Rust is poisoned
/// permanently the moment ANY thread panics while holding it, and every later
/// `.lock()` then returns `Err` forever — so a single panic anywhere in the modem
/// took out **every decode and every transmit for the rest of the session**, and did
/// it silently:
///
/// * on the decode worker the panic is swallowed by a `catch_unwind`, so the app
///   keeps running and the waterfall keeps painting while it has gone completely
///   deaf — one line on stderr is the only trace;
/// * on the radio loop it surfaces as the "RADIO ENGINE CRASHED" banner with dead
///   TX/RX until restart.
///
/// Recovering is the right call here specifically because of what this mutex
/// guards. It protects process-global Fortran state, and every guarded region is a
/// single FFI call with the argument checks done OUTSIDE the lock — so there is
/// almost nothing left that can panic under it, and if something does, the state is
/// no more suspect than the panic already made it. Weigh that against the
/// alternative, which is not "fail safe" but "the radio is dead until you notice and
/// restart". A stale decode is caught by the CRC; a deaf receiver is not caught by
/// anything.
///
/// This is the same recovery the audio device layer already uses
/// (`unwrap_or_else(|e| e.into_inner())`); the modem was the outlier.
pub fn modem_lock() -> MutexGuard<'static, ()> {
    MODEM_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Total channel symbols per FT1 frame.
pub const FT1_NN: usize = 99;
/// Raw audio samples per frame (4.0 s @ 12 kHz).
pub const FT1_NMAX: usize = 48000;
/// Downsample factor.
pub const FT1_NDOWN: usize = 54;
/// Downsampled complex samples.
pub const FT1_NDMAX: usize = 888;
/// Samples-per-symbol numerator.
pub const FT1_NSPS_NUM: c_int = 3000;
/// Samples-per-symbol denominator.
pub const FT1_NSPS_DEN: c_int = 7;
/// Decoded message bits (77 message + 14 CRC).
pub const FT1_MSG91: usize = 91;

/// Total channel symbols per FT8 frame.
pub const FT8_NN: usize = 79;
/// Raw audio samples per FT8 frame (15.0 s @ 12 kHz).
pub const FT8_NMAX: usize = 180_000;
/// Samples in the full 12.64 s FT8 waveform (NSPS*NN).
pub const FT8_NZ: usize = 151_680;

/// Sync + data channel symbols per FT4 frame (16 sync + 87 data).
pub const FT4_NN: usize = 103;
/// Raw audio samples per FT4 frame (21*3456, ~6.05 s window of the 7.5 s slot).
pub const FT4_NMAX: usize = 72_576;

/// The T/R periods FST4/FST4W support. Anything else is REJECTED with -1 rather
/// than clamped — a wrong period makes the modem read a different span of the
/// caller's buffer than the caller sized.
pub const FST4_PERIODS: [u16; 7] = [15, 30, 60, 120, 300, 900, 1800];

/// FST4 channel symbols per transmission: 120 data + 40 sync. Upstream's `NN`
/// (`lib/fst4/fst4_params.f90`). FST4 is 4-FSK, so tones are 0..3.
pub const FST4_NN: usize = 160;

/// Symbol length in samples at 12 kHz, indexed like [`FST4_PERIODS`]. Verbatim from
/// upstream, where the same table appears in `fst4_decode.f90:206` and both TX
/// blocks of `mainwindow.cpp` — 120 s is 8200, 900 s is 66560, not derived.
pub const FST4_NSPS: [usize; 7] = [720, 1680, 3888, 8200, 21504, 66560, 134400];
/// Ceiling on an FST4 frame: 1800 s @ 12 kHz. NOT the buffer contract — the actual
/// length is [`fst4_nmax`] of the period in use.
pub const FST4_NMAX_MAX: usize = 21_600_000;

/// Samples in one FST4/FST4W frame at `period_s` seconds, 12 kHz.
///
/// This is THE buffer contract for [`fst4_decode_frame`]. The routine reads
/// `nfft1`, which upstream's period table keeps ≤ this at every period.
pub const fn fst4_nmax(period_s: u16) -> usize {
    period_s as usize * 12_000
}

/// Whether `period_s` is one of the seven periods the modem supports.
pub const fn fst4_period_supported(period_s: u16) -> bool {
    matches!(period_s, 15 | 30 | 60 | 120 | 300 | 900 | 1800)
}

/// The T/R periods (seconds) Q65 supports. The C ABI accepts any of these and
/// REJECTS anything else with -1 rather than clamping — a wrong period makes the
/// modem read a different span of the caller's buffer than the caller sized, and a
/// decode off the wrong window is a plausible wrong answer rather than a crash.
pub const Q65_PERIODS: [u16; 5] = [15, 30, 60, 120, 300];
/// Number of Q65 submodes (A–E, passed as 0..=4). They differ in tone spacing.
pub const Q65_NSUBMODES: u8 = 5;

/// Q65 channel symbols per transmission: 63 data + 22 sync. Upstream's
/// `NUM_Q65_SYMBOLS`.
pub const Q65_NN: usize = 85;

/// Symbol length in samples at 12 kHz, indexed like [`Q65_PERIODS`]. Verbatim from
/// WSJT-X's `lib/q65params.f90` — NOT derived: 120 s is 16000, not the 16384 a
/// power-of-two guess gives, and 300 s is 41472.
pub const Q65_NSPS: [usize; 5] = [1800, 3600, 7200, 16000, 41472];
/// Ceiling on a Q65 frame: 300 s @ 12 kHz. NOT the buffer contract — the actual
/// length is [`q65_nmax`] of the period in use, and sizing everything at the max
/// would waste 20x on a 15 s decode.
pub const Q65_NMAX_MAX: usize = 3_600_000;

/// Samples in one Q65 frame at `period_s` seconds, 12 kHz.
///
/// This is THE buffer contract for [`q65_decode_frame`]: supply exactly this many
/// samples for the period being decoded.
pub const fn q65_nmax(period_s: u16) -> usize {
    period_s as usize * 12_000
}

/// Whether `period_s` is one of the five periods the modem supports.
pub const fn q65_period_supported(period_s: u16) -> bool {
    matches!(period_s, 15 | 30 | 60 | 120 | 300)
}

/// One decode from [`ft1_decode_frame`]. Layout matches `ft1_decode_t` in
/// `libtempo.h` (68 bytes, 4-byte aligned; `#[repr(C)]` reproduces the 2-byte pad
/// after `message`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Ft1DecodeT {
    pub sync: c_float,
    pub snr: c_int,
    /// Time offset in seconds, WSJT-X convention `xdt = t - 0.5`.
    pub dt: c_float,
    pub freq: c_float,
    pub message: [u8; 38],
    pub nap: c_int,
    pub qual: c_float,
    /// Redundancy version, or -1 (FT1's decode callback does not expose it).
    pub rv: c_int,
}

impl Default for Ft1DecodeT {
    fn default() -> Self {
        Self {
            sync: 0.0,
            snr: 0,
            dt: 0.0,
            freq: 0.0,
            message: [0; 38],
            nap: 0,
            qual: 0.0,
            rv: -1,
        }
    }
}

/// One decode from [`dx1_decode_band`] (the DX1 full-passband scan). Layout
/// matches `dx1_decode_t` in `libtempo.h` (52 bytes, 4-byte aligned; the 2-byte
/// tail pad after `message` is reproduced by `#[repr(C)]`). DX1 has no
/// dt/AP/RV, so it is leaner than [`Ft1DecodeT`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Dx1DecodeT {
    /// Resolved carrier (lower comb edge), Hz.
    pub freq: c_float,
    /// Chirp sync correlation metric.
    pub sync: c_float,
    /// SNR estimate, dB (rounded).
    pub snr: c_int,
    pub message: [u8; 38],
}

impl Default for Dx1DecodeT {
    fn default() -> Self {
        Self {
            freq: 0.0,
            sync: 0.0,
            snr: 0,
            message: [0; 38],
        }
    }
}

/// One decode from [`ft8_decode_frame`] / [`ft4_decode_frame`]. Layout matches
/// `ft8_decode_t` / `ft4_decode_t` in `libtempo.h` (64 bytes, 4-byte aligned;
/// `#[repr(C)]` reproduces the 2-byte pad after `message`). FT8 and FT4 share
/// the identical record; [`Ft4DecodeT`] is an alias.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Ft8DecodeT {
    pub sync: c_float,
    pub snr: c_int,
    /// Time offset in seconds, WSJT-X convention `xdt = t - 0.5`.
    pub dt: c_float,
    pub freq: c_float,
    pub message: [u8; 38],
    /// A-priori decode type used (iaptype; 0 = none).
    pub nap: c_int,
    pub qual: c_float,
}

impl Default for Ft8DecodeT {
    fn default() -> Self {
        Self {
            sync: 0.0,
            snr: 0,
            dt: 0.0,
            freq: 0.0,
            message: [0; 38],
            nap: 0,
            qual: 0.0,
        }
    }
}

/// FT4 decode record — byte-identical C-ABI layout to [`Ft8DecodeT`].
pub type Ft4DecodeT = Ft8DecodeT;

/// FST4 decode record — byte-identical C-ABI layout to [`Ft8DecodeT`].
pub type Fst4DecodeT = Ft8DecodeT;

/// The T/R periods MSK144 supports. 15 s is the 6 m meteor-scatter workhorse.
/// Anything else is REJECTED with -1 rather than clamped.
pub const MSK144_PERIODS: [u16; 4] = [5, 10, 15, 30];

/// MSK144 channel symbols per frame: 144 bits, which at 2000 baud is a 72 ms
/// message. Upstream's `NUM_MSK144_SYMBOLS`. The MSK40 shorthand is 40.
pub const MSK144_NN: usize = 144;

/// Samples per symbol at 12 kHz — 2000 baud. `msk144sim.f90:54`.
pub const MSK144_NSPS: usize = 6;

/// MSK144 baud rate. The tone separation is `baud/2` = 1000 Hz, which is what
/// makes the modulation minimum-shift keying rather than arbitrary FSK.
pub const MSK144_BAUD: f32 = 2000.0;
/// Ceiling on an MSK144 frame: 30 s @ 12 kHz.
pub const MSK144_NMAX_MAX: usize = 360_000;

/// Samples in one MSK144 frame at `period_s` seconds, 12 kHz.
pub const fn msk144_nmax(period_s: u16) -> usize {
    period_s as usize * 12_000
}

/// Whether `period_s` is one of the four periods MSK144 supports.
pub const fn msk144_period_supported(period_s: u16) -> bool {
    matches!(period_s, 5 | 10 | 15 | 30)
}

/// JT65 buffer contract: 60 s @ 12 kHz. The full frame must be supplied even
/// though only [`JT65_NPTS`] is read — `dd0` is an explicit-shape dummy.
pub const JT65_NMAX: usize = 720_000;
/// What the JT65 decoder actually reads: the first 52 s of the period.
pub const JT65_NPTS: usize = 624_000;
/// JT65 submodes A/B/C, passed as 0/1/2 (tone spacing 1x/2x/4x).
pub const JT65_NSUBMODES: u8 = 3;

/// JT65 channel symbols per transmission: 63 sync + 63 data. Upstream's
/// `NUM_JT65_SYMBOLS`.
pub const JT65_NN: usize = 126;

/// Samples the WSPR decoder reads: 114 s of the 120 s window, at 12 kHz.
pub const WSPR_NMAX: usize = 1_368_000;
/// WSPR's reception interval in seconds. Fixed — WSPR has one period.
pub const WSPR_PERIOD_S: u16 = 120;

/// One decode from [`wspr_decode_core`]. Layout matches `wspr_decode_t` in
/// `libtempo.h`.
///
/// NOT the 64-byte shape the FT8-family records share, deliberately: `freq` is an
/// ABSOLUTE frequency in MHz as an `f64` (not an audio offset in Hz), `drift` has
/// no analogue in the other modes, and the message is 22 characters from WSPR's
/// own 50-bit layer.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WsprDecodeT {
    /// Absolute RF frequency in MHz (dial + audio offset).
    pub freq: f64,
    pub sync: c_float,
    pub snr: c_float,
    pub dt: c_float,
    /// Frequency drift, Hz/minute.
    pub drift: c_float,
    pub message: [u8; 23],
    /// 0 = type 1, 1 = type 2, 2 = type 3.
    pub decodetype: c_int,
}

impl Default for WsprDecodeT {
    fn default() -> Self {
        Self {
            freq: 0.0,
            sync: 0.0,
            snr: 0.0,
            dt: 0.0,
            drift: 0.0,
            message: [0; 23],
            decodetype: 0,
        }
    }
}

/// One decode from [`jt65_decode_frame`]. Layout matches `jt65_decode_t` in
/// `libtempo.h`. NOT an alias of [`Ft8DecodeT`]: the last two fields are
/// `ft`/`qual` (int/int) where FT8 has `nap`/`qual` (int/float).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Jt65DecodeT {
    pub sync: c_float,
    pub snr: c_int,
    pub dt: c_float,
    pub freq: c_float,
    /// NUL-terminated. JT65 uses the legacy 72-bit layer, so only 22 bytes are
    /// ever filled.
    pub message: [u8; 38],
    /// Decode type: 1 = Reed-Solomon, 2 = deep search.
    pub ft: c_int,
    /// Deep-search confidence; 0 for a Reed-Solomon decode.
    pub qual: c_int,
}

impl Default for Jt65DecodeT {
    fn default() -> Self {
        Self {
            sync: 0.0,
            snr: 0,
            dt: 0.0,
            freq: 0.0,
            message: [0; 38],
            ft: 0,
            qual: 0,
        }
    }
}

/// One decode from [`msk144_decode_frame`]. Layout matches `msk144_decode_t` in
/// `libtempo.h` — the same 64 bytes as [`Ft8DecodeT`], NOT an alias: the last two
/// fields are `dtype`/`reserved` (int/int) where FT8 has `nap`/`qual`
/// (`int`/`float`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Msk144DecodeT {
    /// Always 0.0 — mskrtd reports no sync metric.
    pub sync: c_float,
    pub snr: c_int,
    /// Time offset within the T/R period, seconds.
    pub dt: c_float,
    pub freq: c_float,
    pub message: [u8; 38],
    /// 0 = frame-averaged, 1 = `&` single-ping (mskspd), 2 = `^` long average.
    pub dtype: c_int,
    /// Unused, always 0.
    pub reserved: c_int,
}

impl Default for Msk144DecodeT {
    // Hand-written rather than derived: `[u8; 38]` has no `Default` impl (the std
    // blanket stops at 32), which is why every other decode record here does the
    // same.
    fn default() -> Self {
        Self {
            sync: 0.0,
            snr: 0,
            dt: 0.0,
            freq: 0.0,
            message: [0; 38],
            dtype: 0,
            reserved: 0,
        }
    }
}

/// One decode from [`q65_decode_frame`]. Layout matches `q65_decode_t` in
/// `libtempo.h` — the same 64 bytes as [`Ft8DecodeT`], but NOT an alias: Q65's last
/// two fields are `idec`/`nused` (both `int`), where FT8 has `nap`/`qual`
/// (`int`/`float`). Aliasing would silently reinterpret `nused` as a float.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Q65DecodeT {
    /// snr1: sync-curve correlation metric.
    pub sync: c_float,
    /// SNR estimate, dB in 2500 Hz.
    pub snr: c_int,
    /// Time offset in seconds.
    pub dt: c_float,
    pub freq: c_float,
    pub message: [u8; 38],
    /// Decode type: 0=q0, 1=q1, 2=q2, 3=q3 (full-AP list decode).
    pub idec: c_int,
    /// T/R periods averaged. Always 1 — the ABI pins `lclearave` so each frame
    /// decodes independently.
    pub nused: c_int,
}

impl Default for Q65DecodeT {
    fn default() -> Self {
        Self {
            sync: 0.0,
            snr: 0,
            dt: 0.0,
            freq: 0.0,
            message: [0; 38],
            idec: 0,
            nused: 0,
        }
    }
}

extern "C" {
    /// Encode a message into 99 quaternary channel symbols {0,1,2,3} (RV0).
    pub fn ft1_encode(
        msg: *const c_char,
        msg_len: c_int,
        itone_out: *mut c_int, // [FT1_NN]
        nsym_out: *mut c_int,
    );

    /// Encode a message into 99 channel symbols for IR-HARQ redundancy version
    /// `irv` (0/1/2). `irv=0` is byte-identical to [`ft1_encode`]; `irv=1/2`
    /// emit the punctured retransmission frames with RV-specific Costas sync.
    pub fn ft1_encode_rv(
        msg: *const c_char,
        msg_len: c_int,
        irv: c_int,
        itone_out: *mut c_int, // [FT1_NN]
        nsym_out: *mut c_int,
    );

    /// Generate the real-valued 4-CPM audio waveform from channel symbols.
    pub fn ft1_gen_wave(
        itone: *const c_int,
        nsym: c_int,
        nsps_num: c_int,
        nsps_den: c_int,
        fsample: c_float,
        f0: c_float,
        wave_out: *mut c_float,
        nwave_out: *mut c_int, // in: capacity, out: samples produced
    );

    /// Decode a received frame (real-time / single-candidate, dt0 = 0 path).
    pub fn ft1_decode_rt(
        wave: *const c_float, // [FT1_NMAX]
        f0: c_float,
        snr_est: c_float,
        message91_out: *mut i8, // [FT1_MSG91]
        ntype_out: *mut c_int,  // 1=turbo, 2=OSD, -1=failed
        nharderror_out: *mut c_int,
    );

    /// Unpack the 77 message bits back to readable text.
    pub fn ft1_unpack(
        bits77: *const i8, // [77]
        msg_out: *mut c_char,
        msg_cap: c_int,
        success: *mut c_int,
    );

    /// Full RX acquisition decode of a 4-second int16 frame: Costas sync
    /// candidate search across time + frequency, then turbo decode / OSD / AP /
    /// SIC / IR-HARQ. Returns the number of decodes written to `out` (>=0), or
    /// -1 on error.
    ///
    /// `frame_time_ms` is a monotonic millisecond timestamp for THIS frame (need
    /// not be wall-clock — only monotonic and consistent across frames). It keys
    /// cross-frame IR-HARQ slot matching + 30 s expiry; call [`ft1_harq_reset`]
    /// on band/QSO change. `out[i].rv` carries the detected redundancy version.
    pub fn ft1_decode_frame(
        iwave: *const i16, // [FT1_NMAX]
        nfa: c_int,
        nfb: c_int,
        ndepth: c_int,
        mycall: *const c_char,
        hiscall: *const c_char,
        nqso_progress: c_int,
        frame_time_ms: c_int,
        out: *mut Ft1DecodeT,
        max_out: c_int,
    ) -> c_int;

    /// Clear all IR-HARQ soft-combining buffers. Call on band change, QSO
    /// change, or intentional QSY so a new exchange does not combine with stale
    /// RV frames. (Buffers otherwise persist across frames and self-expire
    /// after 30 s.)
    pub fn ft1_harq_reset();

    // ---- DX1: non-coherent M-FSK robust tier --------------------------------

    /// DX1 transmit-waveform length in samples (chirp sync + 58 8-FSK symbols).
    pub fn dx1_frame_len() -> c_int;

    /// DX1 receive capture-window length in samples (a full 15 s T/R slot).
    pub fn dx1_capture_len() -> c_int;

    /// Encode a message into a DX1 audio waveform. `wave_out` must hold at least
    /// `dx1_frame_len()` samples. Returns samples written (> 0), or -1 on error.
    pub fn dx1_encode_wave(
        msg: *const c_char,
        msg_len: c_int,
        f0: c_float,
        fsample: c_float,
        wave_out: *mut c_float,
        max_out: c_int,
    ) -> c_int;

    /// Non-coherently decode a DX1 capture window at carrier `f0`: chirp sync
    /// (searching sample offsets `idt_lo..idt_hi`) -> per-symbol FFT energies ->
    /// soft LDPC. Writes the message text + SNR/sync metrics. Returns the hard-
    /// error count (< 0 = decode/CRC failed).
    pub fn dx1_decode_buf(
        wave: *const c_float,
        nwave: c_int,
        f0: c_float,
        fsample: c_float,
        idt_lo: c_int,
        idt_hi: c_int,
        msg_out: *mut c_char,
        msg_cap: c_int,
        snr_out: *mut c_float,
        sync_out: *mut c_float,
    ) -> c_int;

    /// Decode EVERY DX1 signal in the audio passband in one slot (full-band
    /// acquisition, like [`ft1_decode_frame`] for FT1): a coarse chirp-
    /// correlation carrier scan over `f_lo..f_hi` -> peak-pick -> full decode
    /// per survivor (CRC-14 gated). Writes up to `min(found, max_out)` entries
    /// into `out`; returns the number of decodes (>= 0). NOT thread-safe.
    pub fn dx1_decode_band(
        wave: *const c_float,
        nwave: c_int,
        f_lo: c_float,
        f_hi: c_float,
        fsample: c_float,
        out: *mut Dx1DecodeT,
        max_out: c_int,
    ) -> c_int;

    // ---- FT8: native decode of the standard WSJT-X FT8 mode (15 s T/R) -------

    /// Encode a message into 79 FT8 channel tones {0..7}. `nsym_out` = 79 on
    /// success, or -1 on a bad message.
    pub fn ft8_encode(
        msg: *const c_char,
        msg_len: c_int,
        itone_out: *mut c_int, // [FT8_NN]
        nsym_out: *mut c_int,
    );

    /// Generate the real FT8 audio waveform (Gaussian BT=2.0) from tones.
    /// `nwave_out` is capacity in / samples produced out (`nsym*1920`), or -1.
    pub fn ft8_gen_wave(
        itone: *const c_int,
        nsym: c_int,
        fsample: c_float,
        f0: c_float,
        wave_out: *mut c_float,
        nwave_out: *mut c_int,
    );

    /// Decode every FT8 signal in a 180000-sample int16 frame: `ft8apset` ->
    /// `sync8` candidate search -> `ft8b` (with internal multi-pass subtraction),
    /// then the a7 cross-cycle replay (WSJT-X iaptype=7) on the authoritative
    /// pass. Returns decodes written (>=0) or -1. NOT thread-safe.
    pub fn ft8_decode_frame(
        iwave: *const i16, // [FT8_NMAX]
        nfa: c_int,
        nfb: c_int,
        ndepth: c_int,
        mycall: *const c_char,
        hiscall: *const c_char,
        nqso_progress: c_int,
        nfqso: c_int,    // QSO/RX freq (Hz); deep AP + sync center; 0/oob ⇒ band mid
        nutc: c_int,     // a7 slot key: slot UTC seconds-of-day (slot*15); see libtempo.h
        la7final: c_int, // 1 = authoritative pass (a7 save + replay); 0 = early pass
        out: *mut Ft8DecodeT,
        max_out: c_int,
    ) -> c_int;

    /// Clear the FT8 a7 cross-cycle decode table (prior-slot call pairs + slot
    /// tracker). Call on band/QSO change so stale prior-cycle pairs are not
    /// replayed as AP hypotheses. Mirrors [`ft1_harq_reset`].
    pub fn ft8_a7_reset();

    // ---- FT4: native decode of the standard WSJT-X FT4 mode (7.5 s T/R) ------

    /// Encode a message into 103 FT4 channel tones {0..3}. `nsym_out` = 103, or -1.
    pub fn ft4_encode(
        msg: *const c_char,
        msg_len: c_int,
        itone_out: *mut c_int, // [FT4_NN]
        nsym_out: *mut c_int,
    );

    /// Generate the full-length real FT4 audio frame (`FT4_NMAX` samples) from
    /// tones, exactly as `ft4sim` does. `nwave_out` is capacity in / `FT4_NMAX`
    /// out, or -1.
    pub fn ft4_gen_wave(
        itone: *const c_int,
        nsym: c_int,
        fsample: c_float,
        f0: c_float,
        wave_out: *mut c_float,
        nwave_out: *mut c_int,
    );

    /// Decode every FT4 signal in a 72576-sample int16 frame via the OO
    /// `ft4_decoder` (getcandidates4 -> sync4d -> get_ft4_bitmetrics ->
    /// decode174_91 -> subtract). Returns decodes written (>=0) or -1.
    pub fn ft4_decode_frame(
        iwave: *const i16, // [FT4_NMAX]
        nfa: c_int,
        nfb: c_int,
        ndepth: c_int,
        mycall: *const c_char,
        hiscall: *const c_char,
        nqso_progress: c_int,
        nfqso: c_int, // QSO/RX freq (Hz); deep AP center; 0/oob ⇒ band mid
        out: *mut Ft4DecodeT,
        max_out: c_int,
    ) -> c_int;

    /// Decode every FST4 signal in a 180000-sample (15 s) frame.
    ///
    /// DECODE ONLY — there is deliberately no `fst4_encode` / `fst4_gen_wave`.
    /// FST4 ships receive-only; see `Capabilities.tx` and `modes::tx_mode`.
    /// Encode a message into the 160 FST4 channel symbols (values 0..3).
    ///
    /// `iwspr`: 0 = FST4 (77-bit QSO message, LDPC(240,101)), 1 = FST4W (50-bit
    /// beacon, LDPC(240,74)). Returns `FST4_NN`, or -1 if the message will not pack.
    ///
    /// ⭐ `iwspr` selects the CODE, not just the message shape — both produce 160
    /// symbols, so a wrong value transmits a well-formed frame the other side's
    /// decoder cannot read.
    pub fn fst4_encode_msg(
        msg: *const c_char,
        msg_len: c_int,
        iwspr: c_int,
        itone_out: *mut c_int, // [FST4_NN]
    ) -> c_int;

    /// FST4 channel symbols → real audio at `fsample`, nominal carrier `f0`.
    ///
    /// `hmod` is upstream's tone-spacing multiplier (1 | 2 | 4). Returns samples
    /// produced (`160 * nsps`), or -1 on refusal.
    ///
    /// Unlike Q65's plain MFSK this is GFSK-shaped (BT=2.0) with raised-cosine
    /// ramps, via upstream's own `gen_fst4wave`. `f0` is where the signal is
    /// REPORTED — the ABI applies the 1.5-tone offset that upstream's callers do.
    pub fn fst4_gen_wave(
        itone: *const c_int,
        nsym: c_int,
        ntrperiod: c_int, // 15 | 30 | 60 | 120 | 300 | 900 | 1800
        hmod: c_int,      // 1 | 2 | 4
        fsample: c_float,
        f0: c_float,
        wave_out: *mut c_float,
        nwave_cap: c_int,
    ) -> c_int;

    pub fn fst4_decode_frame(
        iwave: *const i16, // [ntrperiod * 12000] — see `fst4_nmax`
        ntrperiod: c_int,  // 15|30|60|120|300|900|1800; anything else ⇒ -1
        iwspr: c_int,      // 0 = FST4 (QSO), 1 = FST4W (beacon); else ⇒ -1
        nfa: c_int,
        nfb: c_int,
        ndepth: c_int,
        mycall: *const c_char,
        hiscall: *const c_char,
        nqso_progress: c_int,
        nfqso: c_int, // QSO/RX freq (Hz); deep AP center; 0/oob ⇒ band mid
        out: *mut Fst4DecodeT,
        max_out: c_int,
    ) -> c_int;

    /// Decode every Q65 signal in a 360000-sample (30 s) frame.
    ///
    /// Takes `hisgrid` as well as the two callsigns: Q65's AP layer builds its
    /// candidate list from the grid too (`q65_set_list`), which FT8/FT4/FST4 do not.
    pub fn q65_decode_frame(
        iwave: *const i16, // [ntrperiod * 12000] — see `q65_nmax`
        ntrperiod: c_int,  // 15 | 30 | 60 | 120 | 300; anything else ⇒ -1
        nsubmode: c_int,   // 0..=4 for A..E; anything else ⇒ -1
        nfa: c_int,
        nfb: c_int,
        ndepth: c_int,
        mycall: *const c_char,
        hiscall: *const c_char,
        hisgrid: *const c_char,
        nqso_progress: c_int,
        nfqso: c_int, // QSO/RX freq (Hz); deep AP center; 0/oob ⇒ band mid
        out: *mut Q65DecodeT,
        max_out: c_int,
    ) -> c_int;

    /// Encode a message into the 85 Q65 channel symbols.
    ///
    /// Returns `Q65_NN` (85), or -1 when the message will not pack.
    ///
    /// ⭐ NOT `q65_encode` — that symbol belongs to upstream's qracodes codeword
    /// API (`q65.h:65`) and is already linked in. Period and submode are absent on
    /// purpose: neither changes the symbol VALUES, only their duration and spacing,
    /// which is [`q65_gen_wave`]'s job. Upstream splits it identically.
    pub fn q65_encode_msg(
        msg: *const c_char,
        msg_len: c_int,
        itone_out: *mut c_int, // [Q65_NN]
    ) -> c_int;

    /// Q65 channel symbols → real audio at `fsample`, carrier `f0`.
    ///
    /// Returns samples produced (`85 * nsps` for the period), or -1 if the period
    /// or submode is unsupported or the buffer is too small.
    ///
    /// ⭐ `nsubmode` genuinely matters: tone spacing is `(12000/nsps) << nsubmode`.
    /// See the Fortran for why the obvious reading of WSJT-X's source gets this
    /// wrong (its 48 kHz preview path is submode-A regardless of the selection).
    pub fn q65_gen_wave(
        itone: *const c_int,
        nsym: c_int,
        ntrperiod: c_int, // 15 | 30 | 60 | 120 | 300
        nsubmode: c_int,  // 0..=4 for A..E
        fsample: c_float,
        f0: c_float,
        wave_out: *mut c_float,
        nwave_cap: c_int,
    ) -> c_int;

    /// Decode every MSK144 signal in one T/R period.
    ///
    /// DECODE ONLY — receive-only; see `Capabilities.tx` and `modes::tx_mode`.
    ///
    /// ⭐ `nutc` MUST differ between periods. mskrtd's duplicate suppressor resets
    /// only when it changes, so a constant silently drops any message heard again
    /// in a later period.
    /// Encode a message into MSK144 channel symbols (BITS — 0 or 1).
    ///
    /// Returns 144 for a full frame, **40 for an MSK40 shorthand**, or -1 on
    /// failure. A caller that assumes 144 would transmit 104 symbols of whatever
    /// was in the buffer.
    pub fn msk144_encode_msg(
        msg: *const c_char,
        msg_len: c_int,
        itone_out: *mut c_int, // [MSK144_NN]
    ) -> c_int;

    pub fn msk144_decode_frame(
        iwave: *const i16, // [ntrperiod * 12000] — see `msk144_nmax`
        ntrperiod: c_int,  // 5 | 10 | 15 | 30; anything else ⇒ -1
        nutc: c_int,       // per-period label; MUST differ between periods
        nfa: c_int,
        nfb: c_int,
        ndepth: c_int,
        mycall: *const c_char,
        hiscall: *const c_char,
        nfqso: c_int,
        out: *mut Msk144DecodeT,
        max_out: c_int,
    ) -> c_int;

    /// Decode every JT65 signal in one 60 s T/R period.
    ///
    /// DECODE ONLY — receive-only; see `Capabilities.tx` and `modes::tx_mode`.
    ///
    /// `iwave` must hold [`JT65_NMAX`] samples even though only [`JT65_NPTS`] is
    /// read: the underlying dummy is explicit-shape.
    /// Encode a message into the 126 JT65 channel symbols.
    ///
    /// Returns 126, or -1 if the message will not pack.
    ///
    /// ⭐ `msg` is at most **22 characters** — JT65 predates 77-bit and uses the
    /// legacy `packjt` layer, not `packjt77`'s 37.
    ///
    /// ⭐ Tone values are 0 (sync) or 2..=65 (data). The +2 offset on data symbols
    /// is part of the wire format, not an off-by-two.
    pub fn jt65_encode_msg(
        msg: *const c_char,
        msg_len: c_int,
        itone_out: *mut c_int, // [JT65_NN]
    ) -> c_int;

    pub fn jt65_decode_frame(
        iwave: *const i16, // [JT65_NMAX] — the full 60 s
        nsubmode: c_int,   // 0 | 1 | 2 for A/B/C; anything else ⇒ -1
        nfa: c_int,
        nfb: c_int,
        ndepth: c_int,
        mycall: *const c_char,
        hiscall: *const c_char,
        hisgrid: *const c_char,
        nfqso: c_int,
        out: *mut Jt65DecodeT,
        max_out: c_int,
    ) -> c_int;

    /// Decode every WSPR signal in one 2-minute reception interval.
    ///
    /// DECODE ONLY — receive-only; see `Capabilities.tx` and `modes::tx_mode`.
    ///
    /// ⭐ Serialize. This plans three FFTW transforms per call, and the FFTW
    /// PLANNER is not thread-safe; the safe wrapper holds `MODEM_LOCK`.
    /// Encode `"CALL GRID DBM"` into the 162 WSPR channel symbols (values 0..3).
    ///
    /// Returns 162, or -1 if the message will not encode. Thin shim over upstream's
    /// `get_wspr_channel_symbols` — see `libtempo/wspr_cabi.c`.
    ///
    /// ⭐ There is no matching `wspr_gen_wave`: upstream has no library-shaped WSPR
    /// waveform generator (wsprsim builds I/Q for its own noise model). The
    /// waveform is plain continuous-phase 4-FSK and is synthesised in the `wspr`
    /// crate, where it is testable.
    pub fn wspr_encode_msg(
        msg: *const c_char,
        symbols_out: *mut u8, // [162]
    ) -> c_int;

    pub fn wspr_decode_core(
        iwave: *const i16, // [WSPR_NMAX]; short is zero-padded
        nsamples: std::os::raw::c_long,
        dialfreq: f64, // rig dial, MHz — reported freq is dial + offset
        quickmode: c_int,
        npasses: c_int,
        subtraction: c_int,
        more_candidates: c_int,
        stackdecoder: c_int,
        out: *mut WsprDecodeT,
        max_out: c_int,
    ) -> c_int;

    // ---- Per-chain decoder context (see `DecoderCtx`) ------------------------

    /// Bytes one per-chain decoder context needs. Sized by the library from its
    /// OWN declarations, so a vendor refresh that resizes a modem table cannot
    /// silently desync the buffer length here.
    pub fn tempo_ctx_size() -> usize;

    /// Write the modem's LOAD-TIME state into `ptr` (`tempo_ctx_size()` bytes).
    /// A fresh context is NOT a zeroed one — `ihash22` starts at -1, the callsign
    /// tables start space-filled — so this, not `memset`, is how one is made.
    /// Touches only the caller's buffer; no modem state, no lock needed.
    pub fn tempo_ctx_reset(ptr: *mut c_void);

    /// Copy the live modem statics OUT into `ptr`. Caller must hold [`MODEM_LOCK`].
    pub fn tempo_ctx_save(ptr: *mut c_void);

    /// Copy `ptr` IN over the live modem statics. Caller must hold [`MODEM_LOCK`].
    pub fn tempo_ctx_restore(ptr: *mut c_void);
}

/// One radio chain's private copy of the modem's process-global decode state.
///
/// Every statically-allocated Fortran symbol in `libtempo` is shared between
/// chains. Chain A's a7 replay table / IR-HARQ slot pool / callsign hash table /
/// cached wideband spectrum, consumed by chain B, does not crash — it yields a
/// CRC-valid, syntactically perfect, WRONG decode, logged and uploaded and
/// indistinguishable afterwards from a real QSO. Giving each chain a context and
/// swapping it around the decode is what makes two radios in one process safe.
///
/// Which symbols are in here is decided by `libtempo/modem-state-manifest.toml`
/// (the class-1 rows) and implemented in `libtempo/ft8_cabi.f90`; this type only
/// owns the bytes.
pub struct DecoderCtx {
    /// Opaque context storage. `u64`, not `u8`: the Fortran side maps a derived
    /// type containing `REAL`/`COMPLEX`/`INTEGER` onto this pointer, and a
    /// `Vec<u8>` allocation carries only a 1-byte alignment guarantee.
    buf: Vec<u64>,
    /// Byte length the library asked for (`buf` is rounded up to whole `u64`s).
    len: usize,
}

impl DecoderCtx {
    /// Allocate a fresh context holding the modem's load-time state.
    pub fn new() -> Self {
        let len = unsafe { tempo_ctx_size() };
        let mut ctx = Self {
            buf: vec![0u64; len.div_ceil(8)],
            len,
        };
        // Fills the caller's buffer only — no modem state is read or written.
        unsafe { tempo_ctx_reset(ctx.as_ptr()) };
        ctx
    }

    /// The context's raw bytes, for tests that need to assert on its CONTENT — chiefly that a
    /// fresh context carries the modem's load-time image rather than the zeroed allocation.
    pub fn as_bytes(&self) -> &[u8] {
        // Safe: `buf` is u64-backed and at least `len` bytes; we expose exactly `len`.
        unsafe { std::slice::from_raw_parts(self.buf.as_ptr() as *const u8, self.len) }
    }

    /// Context size in bytes, as the library reported it.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Always false — a context is never empty. (Present for `clippy::len_without_is_empty`.)
    pub fn is_empty(&self) -> bool {
        false
    }

    fn as_ptr(&mut self) -> *mut c_void {
        self.buf.as_mut_ptr().cast()
    }

    /// Install this context in the modem, run `f`, and capture the resulting
    /// state back into this context.
    ///
    /// The caller MUST already hold the one lock that serializes modem FFI calls
    /// for the whole of this call. A decode landing between the restore and the
    /// save would be decoded against this chain's state and then have ITS state
    /// captured here — exactly the corruption the context exists to prevent.
    ///
    /// If `f` panics the save is skipped, so the modem keeps whatever partial
    /// state the panic left. That is deliberate: a panic inside the decoder means
    /// the process is already unsound, and unwinding through the FFI to "tidy up"
    /// would write half-decoded state into the chain's context.
    pub fn scoped<R>(&mut self, f: impl FnOnce() -> R) -> R {
        let p = self.as_ptr();
        unsafe { tempo_ctx_restore(p) };
        let out = f();
        unsafe { tempo_ctx_save(p) };
        out
    }
}

impl Default for DecoderCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DecoderCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecoderCtx")
            .field("len", &self.len)
            .finish()
    }
}

/// The manifest gate's scanners, compiled into the TEST target only.
///
/// `manifest_gate.rs` is a build-script module (`#[path] mod manifest_gate;` in build.rs), and
/// cargo does not run `#[cfg(test)]` tests that live inside a build script — so every test in
/// that file silently never executed, which is worse than having none. Pulling it in here puts
/// those tests on the normal `cargo test -p tempo-fast-sys --lib` path. It is `cfg(test)` so
/// nothing is added to the shipped library.
///
/// `allow(dead_code)` because this target cannot see the file's real consumer: the
/// gate's entry point (`unclassified`) is called from `build.rs`, which is a separate
/// compilation unit, so from here it looks unused and `-D warnings` fails the build.
/// The allow covers the seam, not a genuinely dead function.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../manifest_gate.rs"]
mod manifest_gate;

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    /// Lock the C-ABI byte layout of `Ft1DecodeT` to `ft1_decode_t` in
    /// `libtempo.h` / `tempofast_cabi.f90`. A drift here would silently corrupt every
    /// FT1 decode marshalled across the FFI.
    #[test]
    fn ft1_decode_t_layout() {
        assert_eq!(size_of::<Ft1DecodeT>(), 68, "Ft1DecodeT size");
        assert_eq!(align_of::<Ft1DecodeT>(), 4, "Ft1DecodeT align");
        assert_eq!(offset_of!(Ft1DecodeT, sync), 0);
        assert_eq!(offset_of!(Ft1DecodeT, snr), 4);
        assert_eq!(offset_of!(Ft1DecodeT, dt), 8);
        assert_eq!(offset_of!(Ft1DecodeT, freq), 12);
        assert_eq!(offset_of!(Ft1DecodeT, message), 16);
        assert_eq!(offset_of!(Ft1DecodeT, nap), 56);
        assert_eq!(offset_of!(Ft1DecodeT, qual), 60);
        assert_eq!(offset_of!(Ft1DecodeT, rv), 64);
    }

    /// Lock the C-ABI byte layout of `Dx1DecodeT` to `dx1_decode_t` in
    /// `libtempo.h` / `tempofast_cabi.f90` (52 bytes; 2-byte tail pad after message).
    #[test]
    fn dx1_decode_t_layout() {
        assert_eq!(size_of::<Dx1DecodeT>(), 52, "Dx1DecodeT size");
        assert_eq!(align_of::<Dx1DecodeT>(), 4, "Dx1DecodeT align");
        assert_eq!(offset_of!(Dx1DecodeT, freq), 0);
        assert_eq!(offset_of!(Dx1DecodeT, sync), 4);
        assert_eq!(offset_of!(Dx1DecodeT, snr), 8);
        assert_eq!(offset_of!(Dx1DecodeT, message), 12);
    }

    /// Lock the C-ABI byte layout of `Ft8DecodeT` (and its `Ft4DecodeT` alias)
    /// to `ft8_decode_t` / `ft4_decode_t` in `libtempo.h` / `ft8_cabi.f90` /
    /// `ft4_cabi.f90` (64 bytes; 2-byte pad after message[38]).
    #[test]
    fn ft8_decode_t_layout() {
        assert_eq!(size_of::<Ft8DecodeT>(), 64, "Ft8DecodeT size");
        assert_eq!(align_of::<Ft8DecodeT>(), 4, "Ft8DecodeT align");
        assert_eq!(offset_of!(Ft8DecodeT, sync), 0);
        assert_eq!(offset_of!(Ft8DecodeT, snr), 4);
        assert_eq!(offset_of!(Ft8DecodeT, dt), 8);
        assert_eq!(offset_of!(Ft8DecodeT, freq), 12);
        assert_eq!(offset_of!(Ft8DecodeT, message), 16);
        assert_eq!(offset_of!(Ft8DecodeT, nap), 56);
        assert_eq!(offset_of!(Ft8DecodeT, qual), 60);
        // Ft4DecodeT is an alias — identical layout.
        assert_eq!(size_of::<Ft4DecodeT>(), size_of::<Ft8DecodeT>());
    }

    #[test]
    fn q65_decode_t_layout() {
        // Q65DecodeT is NOT an alias of Ft8DecodeT: the last two fields are
        // idec/nused (int/int) where FT8 has nap/qual (int/float). The offsets must
        // still line up byte-for-byte with q65_decode_t in libtempo.h, so this
        // pins them independently rather than leaning on the FT8 test.
        assert_eq!(size_of::<Q65DecodeT>(), 64, "Q65DecodeT size");
        assert_eq!(align_of::<Q65DecodeT>(), 4, "Q65DecodeT align");
        assert_eq!(offset_of!(Q65DecodeT, sync), 0);
        assert_eq!(offset_of!(Q65DecodeT, snr), 4);
        assert_eq!(offset_of!(Q65DecodeT, dt), 8);
        assert_eq!(offset_of!(Q65DecodeT, freq), 12);
        assert_eq!(offset_of!(Q65DecodeT, message), 16);
        assert_eq!(offset_of!(Q65DecodeT, idec), 56);
        assert_eq!(offset_of!(Q65DecodeT, nused), 60);
        // Same footprint as the FT8 record, which is what lets the two share a
        // results-buffer shape on the Fortran side.
        assert_eq!(size_of::<Q65DecodeT>(), size_of::<Ft8DecodeT>());
    }

    #[test]
    fn q65_frame_length_follows_the_period() {
        // The buffer contract: exactly period*12000 samples, for every supported
        // period. Getting this wrong reads the wrong span of the caller's audio.
        assert_eq!(q65_nmax(15), 180_000);
        assert_eq!(q65_nmax(30), 360_000);
        assert_eq!(q65_nmax(60), 720_000);
        assert_eq!(q65_nmax(120), 1_440_000);
        assert_eq!(q65_nmax(300), 3_600_000);
        // The ceiling must actually be the ceiling.
        for p in Q65_PERIODS {
            assert!(
                q65_nmax(p) <= Q65_NMAX_MAX,
                "period {p} exceeds the ceiling"
            );
            assert!(
                q65_period_supported(p),
                "period {p} in the table but rejected"
            );
        }
        assert_eq!(q65_nmax(300), Q65_NMAX_MAX);
        // And the unsupported ones must be rejected, not silently accepted.
        for p in [0u16, 1, 10, 20, 45, 90, 600] {
            assert!(!q65_period_supported(p), "period {p} must not be supported");
        }
    }

    /// REGRESSION — one panic under the modem lock used to kill the radio for the
    /// whole session.
    ///
    /// `Mutex` poisoning in Rust is PERMANENT and process-wide: the instant any
    /// thread panics while holding the lock, every later `.lock()` returns `Err`
    /// forever. All 29 acquisition sites unwrapped that, so a single panic anywhere
    /// in the modem made every subsequent decode AND transmit panic in turn — and
    /// silently, because the decode worker's `catch_unwind` swallows it and the app
    /// keeps running, waterfall painting, stone deaf.
    ///
    /// This deliberately poisons the real lock (there is only one, and poisoning
    /// cannot be undone) and then asserts the modem is still usable afterwards.
    #[test]
    fn a_panic_under_the_modem_lock_does_not_kill_the_modem() {
        // Poison it for real, the way a panicking decode would.
        let poisoned = std::thread::spawn(|| {
            let _guard = modem_lock();
            panic!("simulated panic while holding the modem lock");
        })
        .join();
        assert!(poisoned.is_err(), "the helper thread was supposed to panic");
        assert!(
            MODEM_LOCK.is_poisoned(),
            "precondition: the lock must actually be poisoned, or this proves nothing"
        );

        // The unwrap that used to be here would panic on every one of these.
        for _ in 0..3 {
            let _guard = modem_lock();
        }

        // And the modem still answers — a real FFI call through the recovered lock.
        let nmax = {
            let _guard = modem_lock();
            fst4_nmax(60)
        };
        assert_eq!(nmax, 720_000, "the modem is still usable after poisoning");
    }
}
