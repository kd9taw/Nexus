//! Safe Rust wrapper over `libtempo`'s native FT8 decoder.
//!
//! FT8 (by K1JT/K9AN, in WSJT-X) is the dominant weak-signal HF digital mode:
//! 15 s T/R, 8-GFSK, 79 channel symbols at 6.25 Bd, LDPC(174,91) + CRC-14, ~−21 dB
//! AWGN threshold. This wraps the vendored WSJT-X GPL decoder (`ft8_decode_frame`
//! in `ft8_cabi.f90`): `ft8apset` → `sync8` candidate search → `ft8b` per-candidate
//! decode with internal multi-pass subtraction.
//!
//! # Thread safety
//! The underlying Fortran modem is **not** thread-safe (process-global `SAVE`
//! state + cached FFTW plans, shared with FT1/FT4/DX1). All entry points
//! serialize behind [`tempo_fast_sys::MODEM_LOCK`], the one lock shared across every
//! mode that links `libtempo`.

use tempo_fast_sys::modem_lock;

pub use tempo_fast_sys::{FT8_NMAX as NMAX, FT8_NN as NN, FT8_NZ as NZ};

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
// (and F8_MAXDEC in ft8_cabi.f90) so busy-band slots don't silently drop the
// weakest decodes. Also equals the vendored ft8_a7 table's MAXDEC.
const MAX_DECODES: usize = 200;

/// Encode a message (≤ 37 chars, standard 77-bit content) into FT8 channel tones
/// {0..7}. Returns the 79-tone vector, or empty on a bad message.
pub fn encode(msg: &str) -> Vec<i32> {
    let bytes = msg.as_bytes();
    let mut itone = vec![0i32; NN];
    let mut nsym: i32 = 0;
    let _guard = modem_lock();
    unsafe {
        tempo_fast_sys::ft8_encode(
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
    tempo_fast_sys::defer_encode(tempo_fast_sys::DeferredEncoder::Ft8, msg.as_bytes());
}

/// Generate the real FT8 audio waveform (Gaussian BT=2.0) for the given tones.
/// `fsample` is the sample rate (use [`SAMPLE_RATE`]); `f0` the audio carrier (Hz).
pub fn gen_wave(itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
    let cap = itone.len() * (NZ / NN); // NSPS per symbol = 1920
    let mut wave = vec![0f32; cap];
    let mut nwave: i32 = cap as i32;
    let _guard = modem_lock();
    unsafe {
        tempo_fast_sys::ft8_gen_wave(
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

/// Decode every FT8 signal in a 180000-sample ([`NMAX`]) int16 frame at 12 kHz.
///
/// `nfa..=nfb` is the audio search range (Hz); `ndepth` is decode aggressiveness
/// (≤ 0 ⇒ 3); `mycall`/`hiscall` enable a-priori decoding (pass `""` if unknown).
/// `nfqso` is the QSO/RX audio frequency (Hz) being worked — WSJT-X's nfqso:
/// the deep AP passes (MyCall+DxCall masks) only fire within ~75 Hz of it and
/// sync prioritizes near it; pass 0 (or out of `nfa..=nfb`) for band-center.
/// `ap` is WSJT-X's "Enable AP" (ft8b's `lft8apon`): `false` runs the regular
/// passes only AND skips the a7 cross-cycle replay (both consumers of the flag).
/// `ap_cq_only` restricts AP to the CQ hypothesis (`lapcqonly`, iaptype 1);
/// ignored while `ap` is `false`. `(true, false)` is stock WSJT-X behaviour.
///
/// A frame under 0.5 LSB rms decodes nothing and leaves the modem untouched, as in WSJT-X
/// (`decoder.f90:92-93`); every entry below shares that floor.
///
/// This is the a7-inert legacy entry: it delegates to [`decode_frame_a7`] with a constant
/// `nutc = 0` AND `a7_final = false`. Callers that thread real slot time (the engine) use
/// [`decode_frame_a7`] to get WSJT-X's iaptype=7 recovery. It also pins the two
/// pass-shape inputs at their unsplit/full-frame values (`nftx = nfqso`, `partial =
/// false`), so this entry decodes exactly as it did before those existed.
///
/// `a7_final = false` is load-bearing, not tidiness. A constant `nutc` alone is NOT inert:
/// it stops the prior-slot table rolling over (so replay no-ops, as intended), but with
/// `a7_final = true` `ft8_a7_save` still runs for every decode, and because `ndelta == 0`
/// is neither "new slot" nor "stale" the per-slot reset never fires — so `ndec` grows
/// without bound. Upstream's own guard against that case lives in `decoder.f90`
/// (`nzhsym==41 .or. nutc.ne.nutc0`), which we deliberately do not vendor.
///
/// The consequence is not cosmetic: `ndec` is incremented BEFORE the `i.gt.MAXDEC` guard
/// and never rolled back (`ft8_a7.f90:44-46`), and `msg0` is byte-adjacent to `jseq` in
/// .bss (verified with `nm -S`), so an unbounded counter eventually writes past the array
/// into the neighbouring symbol at `-O3` with no bounds checking. This path is currently
/// unreachable in production — `NativeSource::decode` is only called under
/// `DecodeBranch::Companion`, whose source is the UDP one — but that is an invariant three
/// files away, not a property of this function.
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
    ap: bool,
    ap_cq_only: bool,
) -> Vec<Decode> {
    decode_frame_a7(
        iwave,
        nfa,
        nfb,
        ndepth,
        mycall,
        hiscall,
        nqso_progress,
        nfqso,
        nfqso, // nftx: no split — the two deep-AP windows collapse to one
        0,
        true,
        false, // full frame, not the early partial pass
        ap,
        ap_cq_only,
    )
}

/// [`decode_frame`] plus WSJT-X's a7 cross-cycle a-priori path (iaptype=7).
///
/// `nutc` is the slot key — the slot's UTC seconds-of-day (`slot * 15` for
/// FT8). The decoder remembers each slot's decoded call pairs per even/odd
/// parity (`mod(nutc/5, 2)`); on the next same-parity slot it replays each
/// pair as ~206 QSO-continuation hypotheses (RR73/73/RRR/report/grid) against
/// the residual audio, recovering continuations a few dB below the direct
/// threshold. Recovered decodes report `nap == 7`. A `nutc` behind the last
/// seen slot (redecode of an older capture) leaves all a7 state untouched.
///
/// `a7_final` is `true` on the authoritative full-audio (slot-boundary) pass —
/// direct decodes are saved into the a7 table and the replay runs — and
/// `false` on an early partial pass (slot bookkeeping only).
///
/// `nftx` is the operator's TX audio offset — WSJT-X's `nftx`
/// (`mainwindow.cpp:3722`). Upstream's deep AP (iaptype ≥ 3) fires in TWO
/// windows, `nfqso ± napwid` **and** `nftx ± napwid` (`ft8b.f90:305` skips a
/// candidate only when it is outside both), because a station answering your CQ
/// usually answers on *your* transmit frequency. Pass `nfqso` (or 0) when RX and
/// TX are not split; the windows then coincide, which is what upstream computes
/// too.
///
/// `partial` marks WSJT-X's **early** decode (`nzhsym = 41`,
/// `mainwindow.cpp:1878`, `m_earlyDecode` at `mainwindow.h:533`) rather than the
/// full-frame pass (`nzhsym = 50`, `mainwindow.cpp:1877/1678`). Upstream makes
/// that pass deliberately cheap — sync threshold 2.0 instead of 1.3
/// (`ft8_decode.f90:178`) and the AP passes 5-8 switched off (`ft8b.f90:275`,
/// `if(nzhsym.lt.50) npasses=4`) — because the authoritative pass re-decodes the
/// same audio moments later. It does **not** change how much audio is read.
///
/// `partial` is NOT `!a7_final`: the F6 review re-decode is `a7_final = false`
/// over the retained FULL audio, and running it at `partial = true` would kill
/// its AP passes — a regression in the one path the operator invokes
/// deliberately to dig harder.
///
/// `ap` / `ap_cq_only` are the operator AP controls — see [`decode_frame`].
/// `ap = false` also disables the a7 replay (a7 is an a-priori technique;
/// ft8b's `lft8apon` gates both in the C-ABI wrapper), while the a7 slot
/// BOOKKEEPING still runs so re-enabling AP next slot has a live table.
///
/// Call [`a7_reset`] on band/QSO change to drop stale prior-cycle pairs.
///
/// # Panics
/// Panics if `iwave.len() < NMAX`.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame_a7(
    iwave: &[i16],
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    nqso_progress: i32,
    nfqso: i32,
    nftx: i32,
    nutc: i32,
    a7_final: bool,
    partial: bool,
    ap: bool,
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
        nftx,
        nutc,
        a7_final,
        partial,
        ap,
        ap_cq_only,
        None,
    )
}

/// What [`decode_frame_a7_streaming`] tells its caller while the pass runs.
pub enum Progress<'a> {
    /// A decode the pass has just accepted. It is also in the list the call returns, in the same
    /// place: the decodes come in the pass's own order, the sync candidates within 10 Hz of
    /// `nfqso` first (WSJT-X `sync8.f90:155-163`), then by sync, the a7 replay's last.
    Found(&'a Decode),
    /// A sync candidate of the direct passes has been tried. Answering `true` stops the pass there,
    /// with what it has found: no further candidate, pass or a7 replay. WSJT-X asks its early
    /// pass's give-up at exactly this point (`ft8_decode.f90:240-242`).
    Candidate,
    /// [`decode_frame_carry`]'s rescue pass, after a full pass from the residual: asked before it
    /// starts and after each of its sync candidates. Answering `true` gives the rescue up there,
    /// keeping what it found; before it starts, the pass's result is the full pass's alone.
    Rescue,
}

/// [`decode_frame_a7`], telling `progress` of each decode the moment the pass accepts it, as
/// WSJT-X's decoder hands each new decode to its callback (`ft8_decode.f90:233-238`) for jt9 to
/// write and flush at once. The answer to [`Progress::Found`] is ignored; to
/// [`Progress::Candidate`], `true` stops the pass.
///
/// `progress` runs on this thread inside the modem (the modem lock is held): it must not decode,
/// encode or otherwise call into the modem. A panic in it stops the pass and is raised again here
/// once the modem has returned.
///
/// # Panics
/// Panics if `iwave.len() < NMAX`, or if `progress` panicked.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame_a7_streaming(
    iwave: &[i16],
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    nqso_progress: i32,
    nfqso: i32,
    nftx: i32,
    nutc: i32,
    a7_final: bool,
    partial: bool,
    ap: bool,
    ap_cq_only: bool,
    progress: &mut dyn FnMut(Progress<'_>) -> bool,
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
        nftx,
        nutc,
        a7_final,
        partial,
        ap,
        ap_cq_only,
        Some(progress),
    )
}

/// The caller's [`Progress`] closure behind the modem's callback pointer, and a panic it raised.
struct Stream<'a> {
    progress: &'a mut dyn FnMut(Progress<'_>) -> bool,
    panic: Option<Box<dyn std::any::Any + Send>>,
}

/// The modem's callback (`tempo_decode_cb`). A panic must not unwind into the Fortran: it is kept
/// and the pass is asked to stop at its next candidate, and [`decode_frame_impl`] raises it again.
unsafe extern "C" fn on_progress(
    user: *mut std::os::raw::c_void,
    rec: *const tempo_fast_sys::Ft8DecodeT,
) -> std::os::raw::c_int {
    // SAFETY: `user` is the `Stream` `decode_frame_impl` passed for this call, alive and not
    // otherwise borrowed until the modem returns; `rec`, when not null, points at a record the
    // modem has just written.
    let stream = unsafe { &mut *user.cast::<Stream<'_>>() };
    if stream.panic.is_some() {
        return 1;
    }
    let found = (!rec.is_null()).then(|| from_record(unsafe { &*rec }));
    let progress = &mut *stream.progress;
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &found {
        Some(d) => {
            progress(Progress::Found(d));
            false
        }
        None => progress(Progress::Candidate),
    })) {
        Ok(stop) => std::os::raw::c_int::from(stop),
        Err(panic) => {
            stream.panic = Some(panic);
            1
        }
    }
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
    nftx: i32,
    nutc: i32,
    a7_final: bool,
    partial: bool,
    ap: bool,
    ap_cq_only: bool,
    progress: Option<&mut dyn FnMut(Progress<'_>) -> bool>,
) -> Vec<Decode> {
    decode_records(
        iwave,
        nfa,
        nfb,
        ndepth,
        mycall,
        hiscall,
        nqso_progress,
        nfqso,
        nftx,
        nutc,
        a7_final,
        partial,
        ap,
        ap_cq_only,
        progress,
        Carried::None,
        false,
    )
    .0
    .iter()
    .map(from_record)
    .collect()
}

/// What a pass is handed from the period's earlier passes ([`decode_records`]).
#[derive(Clone, Copy)]
enum Carried<'a> {
    /// Nothing: the raw audio.
    None,
    /// The early pass's decodes and the 47 step's residual: the full pass starts from it.
    Residual(&'a Early, &'a Residual),
    /// The rescue pass: the raw audio, and the records the period's passes already reported,
    /// which it dedupes against, never hands on and never saves into the a7 table again.
    Reported(&'a [tempo_fast_sys::Ft8DecodeT]),
}

/// The one call into the modem's decode (`ft8_decode_frame_dp`): the records it wrote, and, when
/// `want_subs`, what the full pass needs to subtract each one ([`Carry`]). `carried` is what the
/// period's earlier passes hand this one ([`Carried`]).
#[allow(clippy::too_many_arguments)]
fn decode_records(
    iwave: &[i16],
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    nqso_progress: i32,
    nfqso: i32,
    nftx: i32,
    nutc: i32,
    a7_final: bool,
    partial: bool,
    ap: bool,
    ap_cq_only: bool,
    progress: Option<&mut dyn FnMut(Progress<'_>) -> bool>,
    carried: Carried<'_>,
    want_subs: bool,
) -> (
    Vec<tempo_fast_sys::Ft8DecodeT>,
    Vec<tempo_fast_sys::Ft8SubT>,
) {
    assert!(
        iwave.len() >= NMAX,
        "decode_frame needs at least {NMAX} samples, got {}",
        iwave.len()
    );
    if below_level_floor(iwave) {
        return (Vec::new(), Vec::new());
    }
    let myc = std::ffi::CString::new(mycall).unwrap_or_default();
    let hisc = std::ffi::CString::new(hiscall).unwrap_or_default();
    let mut out = vec![tempo_fast_sys::Ft8DecodeT::default(); MAX_DECODES];
    let mut subs = if want_subs {
        vec![tempo_fast_sys::Ft8SubT::default(); MAX_DECODES]
    } else {
        Vec::new()
    };
    // WSJT-X's m_earlyDecode / m_hsymStop for FT8 (mainwindow.h:533,
    // mainwindow.cpp:1678). The wrapper treats anything but 41 as 50.
    let nzhsym = if partial { 41 } else { 50 };
    let (nearly, early, early_subs, resid) = match carried {
        Carried::Residual(e, r) => (
            e.records.len() as i32,
            e.records.as_ptr(),
            e.subs.as_ptr(),
            r.dd1.as_ptr(),
        ),
        Carried::Reported(records) => (
            records.len() as i32,
            records.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
        ),
        Carried::None => (0, std::ptr::null(), std::ptr::null(), std::ptr::null()),
    };
    // The modem's rescue mode (`ft8_cabi.f90`, la7final 2): the a7 replay runs, and only decodes
    // new to the period are saved.
    let la7final = match carried {
        Carried::Reported(_) if a7_final => 2,
        _ => a7_final as i32,
    };

    let mut stream = progress.map(|progress| Stream {
        progress,
        panic: None,
    });
    let (cb, user): (
        Option<tempo_fast_sys::DecodeCallback>,
        *mut std::os::raw::c_void,
    ) = match stream.as_mut() {
        Some(stream) => (Some(on_progress), (stream as *mut Stream<'_>).cast()),
        None => (None, std::ptr::null_mut()),
    };
    let n = {
        let _guard = modem_lock();
        // SAFETY: every pointer is to a live buffer of the length the modem reads: `iwave` holds
        // NMAX samples (asserted), `out` and `subs` MAX_DECODES records (or `subs` is null), the
        // carried early records and subs `nearly` each and the residual NMAX samples (`Carry`
        // builds them so), or the `nearly` reported records with null subs and residual, and
        // `user` is the `Stream` `on_progress` reads, alive until the modem returns.
        unsafe {
            tempo_fast_sys::ft8_decode_frame_dp(
                iwave.as_ptr(),
                nfa,
                nfb,
                ndepth,
                myc.as_ptr(),
                hisc.as_ptr(),
                nqso_progress,
                nfqso,
                nftx,
                nzhsym,
                nutc,
                la7final,
                ap as i32,
                ap_cq_only as i32,
                out.as_mut_ptr(),
                out.len() as i32,
                cb,
                user,
                if want_subs {
                    subs.as_mut_ptr()
                } else {
                    std::ptr::null_mut()
                },
                nearly,
                early,
                early_subs,
                resid,
            )
        }
    };
    if let Some(panic) = stream.and_then(|s| s.panic) {
        std::panic::resume_unwind(panic);
    }
    let n = n.max(0) as usize;
    out.truncate(n);
    subs.truncate(n.min(subs.len()));
    (out, subs)
}

/// The early pass's audio a carry is checked against: WSJT-X's early moment, 41 x 3456 samples
/// (11.808 s, `m_earlyDecode`).
const EARLY_SAMPLES: usize = 41 * 3456;

/// The samples WSJT-X's 47 step reads and its full pass takes from the residual: 47 x 3456
/// (13.536 s, `m_earlyDecode2`; `lib/ft8_decode.f90:153`).
pub const RESIDUAL_SAMPLES: usize = 47 * 3456;

/// What WSJT-X's FT8 decoder keeps between the passes of one period, so that its full pass starts
/// from audio with the early pass's decodes already subtracted, and neither searches for nor
/// reports them again (`lib/ft8_decode.f90`: the SAVEd `dd1`, `ndec_early`, `itone_save`,
/// `f1_save`, `xdt_save`, `lsubtracted`, `allmessages` at :72-73; the 47 step at :124-150; the full
/// pass at :121 and :152-162). A weak decode under a strong one then surfaces in the full pass's
/// first subtraction pass, and new decodes are handed on first.
///
/// WSJT-X keeps this in process-wide SAVE storage. Here the caller owns one per decoder (one per
/// radio chain), and nothing of it is kept in the modem. Its life, period by period:
/// - an early pass ([`decode_frame_carry`] with `partial`) replaces it with its decodes, what is
///   needed to subtract each, and the audio they were decoded from (none at depth 1, where WSJT-X
///   runs no early pass, `:102-105`, or when it decoded nothing);
/// - [`subtract_early`] turns that into the residual, only over audio that starts with the very
///   samples the early pass decoded;
/// - a full pass (`a7_final`, not `partial`) takes it, whatever it holds, and starts from the
///   residual only when its own first [`RESIDUAL_SAMPLES`] samples are the very samples the
///   residual was made from. Any other full pass decodes the raw audio, exactly as before: a carry
///   from another period, band or mode, or a step that never ran, can never be spliced onto audio
///   it was not made from;
/// - every other pass (the F6 re-decode) leaves it alone.
#[derive(Default)]
pub struct Carry {
    state: CarryState,
}

#[derive(Default)]
enum CarryState {
    #[default]
    Empty,
    Early(Early),
    Residual(Early, Residual),
}

/// An early pass's decodes, as it returned them.
struct Early {
    records: Vec<tempo_fast_sys::Ft8DecodeT>,
    subs: Vec<tempo_fast_sys::Ft8SubT>,
    /// The first [`EARLY_SAMPLES`] samples it decoded.
    audio: Vec<i16>,
}

/// The 47 step's residual.
struct Residual {
    /// [`NMAX`] samples; the full pass takes the first [`RESIDUAL_SAMPLES`].
    dd1: Vec<f32>,
    /// The first [`RESIDUAL_SAMPLES`] samples it was made from.
    audio: Vec<i16>,
}

impl Carry {
    /// Forget everything carried.
    pub fn clear(&mut self) {
        self.state = CarryState::Empty;
    }

    /// How many early decodes are carried.
    pub fn early_len(&self) -> usize {
        match &self.state {
            CarryState::Empty => 0,
            CarryState::Early(e) | CarryState::Residual(e, _) => e.records.len(),
        }
    }

    /// Whether the 47 step's residual is ready for a full pass.
    pub fn has_residual(&self) -> bool {
        matches!(self.state, CarryState::Residual(..))
    }

    /// How many of the carried early decodes the residual already has subtracted; the full pass
    /// subtracts the rest.
    pub fn early_subtracted(&self) -> usize {
        match &self.state {
            CarryState::Residual(e, _) => e.subs.iter().filter(|s| s.done != 0).count(),
            _ => 0,
        }
    }
}

/// [`decode_frame_a7_streaming`] for a caller that keeps WSJT-X's carry between the passes of a
/// period ([`Carry`]): an early pass (`partial`) leaves its decodes in `carry`; a full pass
/// (`a7_final`) takes `carry` and, when [`subtract_early`] made a residual from this very audio,
/// starts from it: the early decodes are subtracted and come first in the returned list, as the
/// early pass returned them, and only the decodes new to this pass reach `progress`, in the pass's
/// own order (the RX frequency's first). Without such a residual the full pass is exactly
/// [`decode_frame_a7_streaming`]'s. Any other pass is too, and leaves `carry` alone.
///
/// A full pass that started from the residual is followed by a rescue pass, unless the full pass
/// was given up or `progress` answers `true` to [`Progress::Rescue`] before it starts: the full
/// pass once more on the raw audio, as it ran before the carry, handing on only the messages the
/// period's passes have not reported, which follow them in the returned list. Subtracting the
/// early decodes at the 47 step damages a few signals under or beside them that the raw search
/// finds; the rescue keeps every decode the raw full pass finds, at the cost of a second full
/// pass. It saves only its new decodes into the a7 table, and runs the a7 replay. It asks
/// [`Progress::Rescue`] after each sync candidate, never [`Progress::Candidate`].
///
/// # Panics
/// Panics if `iwave.len() < NMAX`, or if `progress` panicked.
#[allow(clippy::too_many_arguments)]
pub fn decode_frame_carry(
    iwave: &[i16],
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: &str,
    hiscall: &str,
    nqso_progress: i32,
    nfqso: i32,
    nftx: i32,
    nutc: i32,
    a7_final: bool,
    partial: bool,
    ap: bool,
    ap_cq_only: bool,
    progress: &mut dyn FnMut(Progress<'_>) -> bool,
    carry: &mut Carry,
) -> Vec<Decode> {
    if partial {
        carry.clear();
        let (records, subs) = decode_records(
            iwave,
            nfa,
            nfb,
            ndepth,
            mycall,
            hiscall,
            nqso_progress,
            nfqso,
            nftx,
            nutc,
            a7_final,
            partial,
            ap,
            ap_cq_only,
            Some(progress),
            Carried::None,
            true,
        );
        let decodes = records.iter().map(from_record).collect();
        if effective_depth(ndepth) >= 2 && !records.is_empty() && subs.len() == records.len() {
            carry.state = CarryState::Early(Early {
                records,
                subs,
                audio: iwave[..EARLY_SAMPLES].to_vec(),
            });
        }
        return decodes;
    }
    let taken = if a7_final {
        std::mem::take(&mut carry.state)
    } else {
        CarryState::Empty
    };
    let carried = match &taken {
        CarryState::Residual(e, r)
            if iwave.len() >= NMAX && iwave[..RESIDUAL_SAMPLES] == r.audio[..] =>
        {
            Carried::Residual(e, r)
        }
        _ => Carried::None,
    };
    let from_residual = matches!(carried, Carried::Residual(..));
    let mut given_up = false;
    let (records, _) = decode_records(
        iwave,
        nfa,
        nfb,
        ndepth,
        mycall,
        hiscall,
        nqso_progress,
        nfqso,
        nftx,
        nutc,
        a7_final,
        partial,
        ap,
        ap_cq_only,
        Some(&mut |p: Progress<'_>| {
            let candidate = matches!(p, Progress::Candidate);
            let stop = progress(p);
            given_up |= candidate && stop;
            stop
        }),
        carried,
        false,
    );
    let mut decodes: Vec<Decode> = records.iter().map(from_record).collect();
    if !from_residual || given_up || records.is_empty() || progress(Progress::Rescue) {
        return decodes;
    }
    // The rescue: the raw audio, deduped against what the period has reported.
    let (rescued, _) = decode_records(
        iwave,
        nfa,
        nfb,
        ndepth,
        mycall,
        hiscall,
        nqso_progress,
        nfqso,
        nftx,
        nutc,
        a7_final,
        partial,
        ap,
        ap_cq_only,
        Some(&mut |p: Progress<'_>| match p {
            Progress::Candidate => progress(Progress::Rescue),
            p => progress(p),
        }),
        Carried::Reported(&records),
        false,
    );
    decodes.extend(rescued.iter().skip(records.len()).map(from_record));
    decodes
}

/// WSJT-X's 47 step (`lib/ft8_decode.f90:124-149`, run at 47 x 3456 samples, 13.536 s): subtract
/// the carried early decodes from the period's first [`RESIDUAL_SAMPLES`] samples of `iwave`, so
/// the full pass can start from the residual ([`Carry`]). Only those that end inside them (dt
/// under 0.396 s) are subtracted here, DT-refined at depth 3; the full pass subtracts the rest.
/// `give_up` is asked after each early decode and `true` stops there, keeping what was done:
/// WSJT-X bails out at 14.3 s into the period (`:140-145`).
///
/// Does nothing, and leaves no carry, unless `carry` holds an early pass's decodes and `iwave`
/// starts with the very samples that pass decoded. Returns how many early decodes were reached.
///
/// # Panics
/// Panics if `iwave.len() < NMAX`, or if `give_up` panicked.
pub fn subtract_early(
    iwave: &[i16],
    ndepth: i32,
    carry: &mut Carry,
    give_up: &mut dyn FnMut() -> bool,
) -> usize {
    assert!(
        iwave.len() >= NMAX,
        "subtract_early needs at least {NMAX} samples, got {}",
        iwave.len()
    );
    let CarryState::Early(mut early) = std::mem::take(&mut carry.state) else {
        return 0;
    };
    if iwave[..EARLY_SAMPLES] != early.audio[..] {
        return 0;
    }
    let mut dd1 = vec![0f32; NMAX];
    let mut asked = |p: Progress<'_>| matches!(p, Progress::Candidate) && give_up();
    let mut stream = Stream {
        progress: &mut asked,
        panic: None,
    };
    let n = {
        let _guard = modem_lock();
        // SAFETY: `iwave` holds NMAX samples (asserted), `dd1` NMAX, `early.subs` exactly the
        // `nearly` records passed, and `stream` is alive until the modem returns.
        unsafe {
            tempo_fast_sys::ft8_subtract_early(
                iwave.as_ptr(),
                ndepth,
                early.subs.len() as i32,
                early.subs.as_mut_ptr(),
                dd1.as_mut_ptr(),
                Some(on_progress),
                (&mut stream as *mut Stream<'_>).cast(),
            )
        }
    };
    if let Some(panic) = stream.panic {
        std::panic::resume_unwind(panic);
    }
    if n < 0 {
        return 0;
    }
    carry.state = CarryState::Residual(
        early,
        Residual {
            dd1,
            audio: iwave[..RESIDUAL_SAMPLES].to_vec(),
        },
    );
    n as usize
}

/// The decode depth the modem runs: anything at or below 0 is 3 (`ft8_cabi.f90`).
fn effective_depth(ndepth: i32) -> i32 {
    if ndepth <= 0 {
        3
    } else {
        ndepth
    }
}

/// WSJT-X's level floor: a frame under half an LSB rms is not decoded at all. WSJT-X 3.0.2,
/// `lib/decoder.f90:92-93`, at the top of `multimode_decoder`, before any decoder runs:
///
/// ```text
/// rms=sqrt(dot_product(float(id2(1:180000)),float(id2(1:180000)))/180000.0)
/// if(rms.lt.0.5) go to 800
/// ```
///
/// `id2` is the period's 12 kHz int16 audio, the frame each pass hands the decoder, as `iwave` is
/// here, and label 800 is past every decoder call: no pass, no a7 replay, no a7 bookkeeping. Same
/// samples (the first [`NMAX`]), same scale, same threshold. The sum is exact here; upstream's
/// single-precision one is exact below 2^24, far above the threshold's 45000, so the two agree on
/// every frame.
///
/// Without it a near-silent period (a muted rig, a dead cable) after a busy one gave the a7 replay
/// stations to "hear": its metrics are normalised, so one stray LSB scores like a signal.
fn below_level_floor(iwave: &[i16]) -> bool {
    let sum_sq: f64 = iwave[..NMAX]
        .iter()
        .map(|&s| f64::from(s) * f64::from(s))
        .sum();
    (sum_sq / NMAX as f64).sqrt() < 0.5
}

/// One modem record as a [`Decode`].
fn from_record(r: &tempo_fast_sys::Ft8DecodeT) -> Decode {
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

/// Clear the a7 cross-cycle decode table (prior-slot call pairs + slot
/// tracker). Call on band change / QSO change so a new band's audio is not
/// probed with stale prior-cycle hypotheses. Mirrors `tempo_fast::harq_reset`.
pub fn a7_reset() {
    A7ResetGuard(modem_lock()).reset();
}

/// How many decodes the slot decoded last has saved into the a7 table: each station once, however
/// many passes decoded it.
pub fn a7_saved() -> usize {
    let _guard = modem_lock();
    // SAFETY: reads one integer of the modem's table, under the modem lock.
    unsafe { tempo_fast_sys::ft8_a7_saved() }.max(0) as usize
}

/// Exclusive permission to reset FT8's prior-cycle decode table. This holds the
/// same process-wide modem mutex as every decode/encode call. It is not radio or
/// transmit authority, and must not be held across hardware I/O.
pub struct A7ResetGuard(std::sync::MutexGuard<'static, ()>);

impl A7ResetGuard {
    /// A remote context transition can refuse a busy decoder before changing
    /// native state, then recheck its own authority with serialization held.
    pub fn try_acquire() -> Option<Self> {
        tempo_fast_sys::try_modem_lock().map(Self)
    }

    /// Consume the held lock at the native reset point without locking again.
    pub fn reset(mut self) {
        self.reset_held();
    }

    /// Keep serialization across a native radio handoff followed by its QSY.
    /// Each native reset point still runs; neither point reacquires the lock.
    pub fn reset_held(&mut self) {
        let _guard = &self.0;
        unsafe {
            tempo_fast_sys::ft8_a7_reset();
        }
    }

    /// Tempo HARQ shares this same process-wide modem lock with FT8/FT4.
    /// A compound native workspace transition must not acquire it a second time.
    pub fn reset_tempo_harq_held(&mut self) {
        let _guard = &self.0;
        unsafe {
            tempo_fast_sys::ft1_harq_reset();
        }
    }
}

#[cfg(test)]
#[test]
fn repeated_native_a7_resets_keep_the_original_serialization_guard() {
    let mut guard = loop {
        if let Some(guard) = A7ResetGuard::try_acquire() {
            break guard;
        }
        std::thread::yield_now();
    };
    for _ in 0..2 {
        guard.reset_held();
        guard.reset_tempo_harq_held();
        guard.reset_held();
        assert!(
            A7ResetGuard::try_acquire().is_none(),
            "a native reset must retain serialization until the whole routed QSY commits"
        );
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

    /// Encode → waveform → place in a 15 s frame → int16 → full-frame decode
    /// recovers the message through the whole Rust/FFI stack (clean signal).
    #[test]
    fn ft8_encode_decode_roundtrip() {
        let msg = "CQ KD9TAW EN52";
        let tones = encode(msg);
        assert_eq!(tones.len(), NN, "FT8 encodes to 79 tones");

        let wave = gen_wave(&tones, SAMPLE_RATE, 1500.0);
        assert_eq!(wave.len(), NZ, "FT8 waveform is NSPS*NN samples");

        // Place at the 0.5 s FT8 TX start, scale into int16, no noise.
        let noff = 6_000usize;
        let mut iwave = vec![0i16; NMAX];
        for (i, &s) in wave.iter().enumerate() {
            if noff + i < NMAX {
                iwave[noff + i] = (s * 1000.0).clamp(-32768.0, 32767.0) as i16;
            }
        }

        let decs = decode_frame(&iwave, 200, 2900, 3, "", "", 0, 0, true, false);
        assert!(
            decs.iter().any(|d| d.message == msg),
            "FT8 must decode its own clean signal; got {decs:?}"
        );
    }

    /// Build a 15 s frame carrying `msg` at `f0` over a deterministic xorshift
    /// noise floor (±15 LSB). The noise gives every spectral bin real energy so
    /// baselines are well-defined even in bands with no signal; SNR ≈ +40 dB, so
    /// it never threatens a decode.
    fn frame_with_floor(msg: &str, f0: f32, mut seed: u32) -> Vec<i16> {
        let tones = encode(msg);
        assert_eq!(tones.len(), NN, "{msg} encodes to 79 tones");
        let wave = gen_wave(&tones, SAMPLE_RATE, f0);
        let mut iwave = vec![0i16; NMAX];
        for s in iwave.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *s = ((seed >> 8) % 31) as i16 - 15;
        }
        let noff = 6_000usize; // 0.5 s FT8 TX start
        for (i, &v) in wave.iter().enumerate() {
            if noff + i < NMAX {
                iwave[noff + i] = iwave[noff + i].saturating_add((v * 1000.0) as i16);
            }
        }
        iwave
    }

    /// Cross-cycle a7 recovery (WSJT-X iaptype=7) across two consecutive
    /// same-parity slots: slot A's decode seeds the a7 table; slot B's
    /// continuation is placed OUTSIDE the direct search band, so only the a7
    /// replay — which remembers the pair's frequency from slot A — can find
    /// it. Then `a7_reset` drops the table and the same recovery must fail.
    #[test]
    fn ft8_a7_cross_cycle_replay_recovers_continuation() {
        a7_reset();

        // Slot A (nutc=15, odd parity): W1AW answers KD9TAW with a grid.
        // Direct decode on the full band; the final pass seeds the a7 table.
        let msg_a = "KD9TAW W1AW FN31";
        let fa = frame_with_floor(msg_a, 1500.0, 0x2452_1057);
        let decs_a = decode_frame_a7(
            &fa, 200, 2900, 3, "", "", 0, 0, 0, 15, true, false, true, false,
        );
        assert!(
            decs_a.iter().any(|d| d.message == msg_a),
            "slot A must direct-decode; got {decs_a:?}"
        );

        // Slot B (nutc=45, the next odd slot): W1AW's R-report continuation at
        // the same 1500 Hz — but the direct search band is 2000..2900 Hz, so
        // sync8 cannot see it. Only the a7 replay reaches 1500 Hz.
        let msg_b = "KD9TAW W1AW R-10";
        let fb = frame_with_floor(msg_b, 1500.0, 0x0BAD_5EED);
        let decs_b = decode_frame_a7(
            &fb, 2000, 2900, 3, "", "", 0, 0, 0, 45, true, false, true, false,
        );
        let a7 = decs_b.iter().find(|d| d.message == msg_b);
        assert!(
            a7.is_some(),
            "a7 replay must recover the out-of-band continuation; got {decs_b:?}"
        );
        assert_eq!(a7.unwrap().nap, 7, "a7 recovery reports iaptype 7");
        // No duplicate rows: the replay dedups against the direct decodes.
        assert_eq!(
            decs_b.iter().filter(|d| d.message == msg_b).count(),
            1,
            "a7 decode appears exactly once"
        );

        // After a reset the table is empty: the same out-of-band continuation
        // at the next odd slot must NOT decode (nothing left to replay).
        a7_reset();
        let decs_c = decode_frame_a7(
            &fb, 2000, 2900, 3, "", "", 0, 0, 0, 75, true, false, true, false,
        );
        assert!(
            !decs_c.iter().any(|d| d.message == msg_b),
            "a7_reset must drop the prior-slot table; got {decs_c:?}"
        );

        // AP OFF disables the replay: re-seed the table from slot A's frame
        // (nutc=105, odd), then attempt the same out-of-band recovery at the
        // next odd slot with `ap = false`. The a7 replay is an a-priori
        // technique — WSJT-X's lft8apon gates it — so the continuation must
        // NOT appear. (This phase lives inside this test, not its own #[test]:
        // the a7 table is process-global state and a sibling test would
        // interleave with it.)
        let decs_d = decode_frame_a7(
            &fa, 200, 2900, 3, "", "", 0, 0, 0, 105, true, false, true, false,
        );
        assert!(
            decs_d.iter().any(|d| d.message == msg_a),
            "re-seed slot must direct-decode; got {decs_d:?}"
        );
        let decs_e = decode_frame_a7(
            &fb, 2000, 2900, 3, "", "", 0, 0, 0, 135, true, false, false, false,
        );
        assert!(
            !decs_e.iter().any(|d| d.message == msg_b),
            "ap = false must suppress the a7 cross-cycle replay; got {decs_e:?}"
        );
    }

    /// Several signals in one 15 s frame over the light noise floor: `(msg, f0, amplitude)`.
    fn frame_of(signals: &[(&str, f32, f32)]) -> Vec<i16> {
        let mut seed = 0x51A7_u32;
        let mut iwave: Vec<i16> = (0..NMAX)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                ((seed >> 8) % 31) as i16 - 15
            })
            .collect();
        for &(msg, f0, amp) in signals {
            let wave = gen_wave(&encode(msg), SAMPLE_RATE, f0);
            for (i, &v) in wave.iter().enumerate() {
                if let Some(s) = iwave.get_mut(6_000 + i) {
                    *s = s.saturating_add((v * amp) as i16);
                }
            }
        }
        iwave
    }

    /// Three loud stations and a weaker one on the RX frequency (1500 Hz).
    const BAND: [(&str, f32, f32); 4] = [
        ("CQ K1ABC FN31", 700.0, 3000.0),
        ("W9XYZ K2DEF EM79", 1100.0, 3000.0),
        ("CQ N3GHI FM19", 2200.0, 3000.0),
        ("W9XYZ K4JKL R-07", 1500.0, 600.0),
    ];

    /// Every decode the pass accepts is handed on at once, the RX frequency's first (WSJT-X's
    /// `sync8.f90:155-163` puts the candidates within 10 Hz of nfqso at the top), and the handed-on
    /// list IS the returned one; the streaming call returns exactly what the plain call returns.
    #[test]
    fn streaming_hands_on_each_decode_as_found_the_rx_frequency_first() {
        let iwave = frame_of(&BAND);
        // nutc 0 and no a7 save: the a7 table a sibling test drives is left alone.
        let plain = decode_frame_a7(
            &iwave, 200, 2900, 3, "W9XYZ", "", 0, 1500, 1500, 0, false, false, true, false,
        );
        let mut found = Vec::new();
        let mut candidates = 0;
        let streamed = decode_frame_a7_streaming(
            &iwave,
            200,
            2900,
            3,
            "W9XYZ",
            "",
            0,
            1500,
            1500,
            0,
            false,
            false,
            true,
            false,
            &mut |p| {
                match p {
                    Progress::Found(d) => found.push(d.clone()),
                    Progress::Candidate => candidates += 1,
                    Progress::Rescue => {}
                }
                false
            },
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
            "each decode handed on, in the returned order"
        );
        assert_eq!(found.len(), 4, "all four decoded: {found:?}");
        assert_eq!(
            found[0].message, "W9XYZ K4JKL R-07",
            "the RX frequency's decode is handed on first"
        );
        assert!(
            candidates > found.len(),
            "asked after every candidate, not only the decoded ones: {candidates}"
        );
        // The order follows the RX frequency, not the signals: moved to 2200 Hz, that station's
        // decode is the first one handed on.
        let mut first = None;
        decode_frame_a7_streaming(
            &iwave,
            200,
            2900,
            3,
            "W9XYZ",
            "",
            0,
            2200,
            2200,
            0,
            false,
            false,
            true,
            false,
            &mut |p| {
                if let (Progress::Found(d), None) = (p, &first) {
                    first = Some(d.message.clone());
                }
                false
            },
        );
        assert_eq!(
            first.as_deref(),
            Some("CQ N3GHI FM19"),
            "with the RX frequency on 2200 Hz, its decode is handed on first"
        );
    }

    /// A `true` answer to [`Progress::Candidate`] stops the pass there, with what it found: no
    /// further candidate or pass (WSJT-X's early-pass give-up, `ft8_decode.f90:240-242`).
    #[test]
    fn a_give_up_answer_stops_the_pass_at_that_candidate() {
        let iwave = frame_of(&BAND);
        let mut found = Vec::new();
        let mut candidates = 0;
        let got = decode_frame_a7_streaming(
            &iwave,
            200,
            2900,
            3,
            "W9XYZ",
            "",
            0,
            1500,
            1500,
            0,
            false,
            true,
            true,
            false,
            &mut |p| match p {
                Progress::Found(d) => {
                    found.push(d.message.clone());
                    false
                }
                Progress::Candidate => {
                    candidates += 1;
                    true
                }
                Progress::Rescue => false,
            },
        );
        assert_eq!(candidates, 1, "asked once, then stopped");
        assert_eq!(
            found,
            ["W9XYZ K4JKL R-07"],
            "the first candidate's decode, nothing after it"
        );
        assert_eq!(
            got.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(),
            ["W9XYZ K4JKL R-07"],
            "the pass returns what it found before it stopped"
        );
        // Control: never giving up runs the whole pass.
        let all = decode_frame_a7_streaming(
            &iwave,
            200,
            2900,
            3,
            "W9XYZ",
            "",
            0,
            1500,
            1500,
            0,
            false,
            true,
            true,
            false,
            &mut |_| false,
        );
        assert_eq!(all.len(), 4, "control: the whole pass: {all:?}");
    }

    /// A panic in the caller's closure never unwinds into the modem: the pass stops, the modem
    /// returns, and the panic is raised here. The modem works on after it.
    #[test]
    fn a_panic_in_progress_is_raised_once_the_modem_returns() {
        let iwave = frame_of(&BAND);
        let caught = std::panic::catch_unwind(|| {
            decode_frame_a7_streaming(
                &iwave,
                200,
                2900,
                3,
                "",
                "",
                0,
                1500,
                1500,
                0,
                false,
                false,
                true,
                false,
                &mut |p| match p {
                    Progress::Found(_) => panic!("the caller's bug"),
                    Progress::Candidate | Progress::Rescue => false,
                },
            )
        });
        assert!(caught.is_err(), "the panic reached the caller");
        let after = decode_frame(&iwave, 200, 2900, 3, "", "", 0, 1500, true, false);
        assert_eq!(after.len(), 4, "the modem decodes on: {after:?}");
    }

    /// The i3=4 nonstandard/compound-call forms Nexus generates must round-trip through
    /// the real modem INTACT — compound call in full, the hashed call's brackets
    /// preserved, the prefix NOT silently stripped. (A bare "PJ4/K1ABC W9XYZ -10" would
    /// strip to "K1ABC …"; these bracketed forms must not.)
    #[test]
    fn compound_call_forms_round_trip_intact() {
        let cases = [
            "CQ PJ4/K1ABC",           // compound CQ (full call, no grid)
            "<PJ4/K1ABC> W9XYZ",      // Tx1 to a compound DX (DX hashed)
            "<PJ4/K1ABC> W9XYZ R-10", // R-report (hashed-first keeps the number)
            "<PJ4/K1ABC> W9XYZ RR73", // roger
            "<W9XYZ> KD9TAW/P RRR",   // compound ME rogering a standard DX (i3=4, no number)
            "<W9XYZ> KD9TAW/P 73",    // compound ME signing off
            // Forms the OTHER station's modem delivers to us (what our sequencer consumes):
            "<KD9TAW> PJ4/K1ABC", // a compound DX's grid-less answer (no report)
            "<KD9TAW> PJ4/K1ABC RR73", // a compound DX's roger
            "<KD9TAW/P> W9XYZ -09", // a standard caller reporting our compound CQ
        ];
        for msg in cases {
            let tones = encode(msg);
            assert_eq!(tones.len(), NN, "{msg} encodes to 79 tones");
            let wave = gen_wave(&tones, SAMPLE_RATE, 1500.0);
            let noff = 6_000usize; // 0.5 s FT8 TX start
            let mut iwave = vec![0i16; NMAX];
            for (i, &s) in wave.iter().enumerate() {
                if noff + i < NMAX {
                    iwave[noff + i] = (s * 1000.0).clamp(-32768.0, 32767.0) as i16;
                }
            }
            let decs = decode_frame(&iwave, 200, 2900, 3, "", "", 0, 0, true, false);
            assert!(
                decs.iter().any(|d| d.message == msg),
                "{msg} must round-trip intact (no prefix strip / bracket loss); got {decs:?}"
            );
        }
    }
}
