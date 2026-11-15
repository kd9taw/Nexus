//! WSJT-X 3.0.2's multithreaded FT8 decoder (`lib/ft8var`), the WSJT-X port of JTDX's decoder.
//!
//! WSJT-X runs it when Decode > "Use multithreaded FT8 decoder" is on, for the full pass of a
//! period only; its early passes stay the standard decoder (`mainwindow.cpp:5559-5562`). Here
//! [`decode_frame_var`] is that full pass, through `libtempo/ft8var_cabi.f90`, which re-expresses
//! WSJT-X's `lib/decoder.f90` dispatch.
//!
//! # Threads and stack
//! The decoder is compiled with OpenMP, which keeps its local arrays on the stack (megabytes of
//! them), so every call into it here runs on one long-lived thread of its own with
//! [`tempo_fast_sys::OPENMP_THREAD_STACK`] (`on_decoder_thread`); any thread may call these
//! functions. With one thread ([`VarRequest::threads`] = 1) a decode is deterministic; with
//! several, the threads share the audio they subtract from (upstream's design) and the result
//! varies run to run.
//!
//! # State
//! ft8var keeps one radio's QSO across periods (message tables, saved signals, average DT, its own
//! callsign hash tables). It is process-global Fortran state, swapped per radio chain by
//! [`tempo_fast_sys::DecoderCtx`] like the standard decoder's; [`var_reset`] clears what WSJT-X
//! clears on a mode change.

use std::path::Path;

use tempo_fast_sys::{modem_lock, Ft8VarDecodeT, Ft8VarParams};

use crate::{from_record, Decode};

/// One full pass's inputs: WSJT-X's `dec_data.params` for the multithreaded decoder
/// (`mainwindow.cpp:5505-5592`), in Rust types. Callsigns are blank-padded to their field.
#[derive(Debug, Clone)]
pub struct VarRequest<'a> {
    /// Samples decoded = `nzhsym * 3456`. WSJT-X's full pass at its default decoder start: 49.
    pub nzhsym: i32,
    /// The period's start, UTC seconds of day.
    pub utc_seconds: i32,
    /// Search band, Hz.
    pub nfa: i32,
    pub nfb: i32,
    /// RX and TX audio frequency, Hz.
    pub nfqso: i32,
    pub nftx: i32,
    /// `nQSOProgress`, as the standard decoder gets it.
    pub nqso_progress: i32,
    /// AP window, Hz: 5 below 30 MHz, 15 below 100 MHz, else 50 (`mainwindow.cpp:5511-5513`);
    /// see [`napwid_for_dial`].
    pub napwid: i32,
    /// Threads: 0 = WSJT-X's automatic count, 1..=12.
    pub threads: i32,
    /// Decoding cycles 1..=3 (`nft8cycles`).
    pub cycles: i32,
    /// FT8 sensitivity 1..=3: 1 = minimum, 2 = low thresholds, 3 = low thresholds and the
    /// metric sub-pass (`mainwindow.cpp:5524-5527`).
    pub sensitivity: i32,
    /// QSO RX-frequency sensitivity 1..=3 (`nft8rxfsens`; WSJT-X forces 1 as a Hound).
    pub rx_freq_sensitivity: i32,
    /// Candidate thinning, percent; 100 keeps every candidate (`ncandthin`).
    pub cand_thin: i32,
    /// Enable AP (`lft8apon`).
    pub ap: bool,
    /// Hide duplicate decodes (`lhideft8dupes`).
    pub hide_dupes: bool,
    pub mycall: &'a str,
    pub hiscall: &'a str,
    /// Base calls, without prefix or suffix (`Radio::base_callsign`).
    pub mybcall: &'a str,
    pub hisbcall: &'a str,
    pub hisgrid: &'a str,
    /// Whether each call is a standard call (`MainWindow::stdCall`).
    pub mycall_std: bool,
    pub hiscall_std: bool,
    /// Hound (`lhound`; also `ncontest` 7, which turns the a8 pass off).
    pub hound: bool,
    /// A DX call is set, transmit is off and the call is not yet logged (`lenabledxcsearch`).
    pub dx_search: bool,
    /// The band-wide DX-call search (`lwidedxcsearch`).
    pub wide_dx_search: bool,
    /// JTDX's SWL mode.
    pub swl: bool,
}

/// WSJT-X's AP window for the dial frequency (`mainwindow.cpp:5511-5513`).
pub fn napwid_for_dial(dial_hz: u64) -> i32 {
    if dial_hz < 30_000_000 {
        5
    } else if dial_hz < 100_000_000 {
        15
    } else {
        50
    }
}

fn field<const N: usize>(s: &str) -> [u8; N] {
    let mut out = [b' '; N];
    for (o, b) in out.iter_mut().zip(s.bytes()) {
        *o = b;
    }
    out
}

impl VarRequest<'_> {
    /// The C-ABI parameter block, with WSJT-X 3.0.2's constants where Nexus has no setting:
    /// `ndtcenter` 0, `nstophint` 0, `lcommonft8b` true, and `ltxing`, `lapmyc`, `nlasttx`,
    /// `lskiptx1`, `lmultinst` all 0 (`mainwindow.cpp` never sets the first three non-zero).
    pub fn params(&self) -> Ft8VarParams {
        let (lowth, subpass) = match self.sensitivity {
            1 => (false, false),
            2 => (true, false),
            _ => (true, true),
        };
        let t = self.utc_seconds.rem_euclid(86_400);
        Ft8VarParams {
            nzhsym: self.nzhsym,
            nutc: (t / 3600) * 10_000 + (t / 60 % 60) * 100 + t % 60,
            nfa: self.nfa,
            nfb: self.nfb,
            nfqso: self.nfqso,
            nftx: self.nftx,
            nqso_progress: self.nqso_progress,
            napwid: self.napwid,
            nmt: self.threads,
            nft8cycles: self.cycles,
            nft8rxfsens: if self.hound {
                1
            } else {
                self.rx_freq_sensitivity
            },
            ncandthin: self.cand_thin,
            ncontest: if self.hound { 7 } else { 0 },
            lft8lowth: lowth.into(),
            lft8subpass: subpass.into(),
            lft8apon: self.ap.into(),
            lhideft8dupes: self.hide_dupes.into(),
            lmycallstd: self.mycall_std.into(),
            lhiscallstd: self.hiscall_std.into(),
            lhound: self.hound.into(),
            lcommonft8b: 1,
            lenabledxcsearch: self.dx_search.into(),
            lwidedxcsearch: self.wide_dx_search.into(),
            lswl: self.swl.into(),
            mycall: field(self.mycall),
            hiscall: field(self.hiscall),
            mybcall: field(self.mybcall),
            hisbcall: field(self.hisbcall),
            hisgrid: field(self.hisgrid),
            ..Ft8VarParams::default()
        }
    }
}

/// What the decoder thread sends back while a decode runs.
enum Reply {
    /// A record the decoder has just accepted.
    Found(Decode),
    /// The decode has returned: its records, in the decoder's order.
    Done(Vec<Decode>),
}

/// The C callback, on the decoder thread or one of its OpenMP workers (one at a time, under the
/// decoder's `critical(ft8var_decoded)`): each accepted record goes to the caller at once.
unsafe extern "C" fn on_var_decode(
    user: *mut std::os::raw::c_void,
    rec: *const Ft8VarDecodeT,
) -> std::os::raw::c_int {
    if !rec.is_null() {
        // SAFETY: `user` is the job's reply sender, alive until the decode returns; `rec` is a
        // record the modem has just written.
        let reply = unsafe { &*user.cast::<std::sync::mpsc::Sender<Reply>>() };
        // The caller stops listening only after `Done`, so a send cannot fail mid-decode.
        let _ = reply.send(Reply::Found(from_var_record(unsafe { &*rec })));
    }
    0
}

fn from_var_record(r: &Ft8VarDecodeT) -> Decode {
    Decode {
        unchecked: r.unchecked != 0,
        ..from_record(&r.rec)
    }
}

/// Run `f` on the one thread every call into ft8var runs on, for the life of the process, as
/// WSJT-X's `jt9` decodes from one thread for its whole life, and wait for it.
///
/// Why a thread of its own rather than the caller's:
/// - the decoder keeps megabytes of local arrays on the stack (it is compiled -fopenmp; a decode
///   needs 4-6 MiB measured, the callsign-database load over 1 MiB), so the thread needs
///   [`tempo_fast_sys::OPENMP_THREAD_STACK`], which a caller's thread (a test's, at 2 MiB) need
///   not have;
/// - its OpenMP thread pool belongs to the thread that starts parallel regions, and its FFT
///   plans are keyed on stack addresses: one long-lived thread keeps both, where each new calling
///   thread would build them again;
/// - its `!$omp threadprivate` state (packjt77's recent calls) lives on the thread that decodes.
///
/// `f` may send to the caller while it runs (a decode streams its records); the caller holds the
/// modem lock across the whole call.
fn on_decoder_thread(f: Box<dyn FnOnce() + Send>) {
    type Work = Box<dyn FnOnce() + Send>;
    static EXEC: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<Work>>> =
        std::sync::OnceLock::new();
    let exec = EXEC.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Work>();
        std::thread::Builder::new()
            .name("ft8var-decoder".into())
            .stack_size(tempo_fast_sys::OPENMP_THREAD_STACK)
            .spawn(move || {
                for work in rx {
                    work();
                }
            })
            .expect("spawn the ft8var decoder thread");
        std::sync::Mutex::new(tx)
    });
    exec.lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(f)
        .expect("the ft8var decoder thread is gone");
}

/// Run `f` on ft8var's decoder thread (see `on_decoder_thread`) and return its result: the way
/// into code that shares ft8var's state and needs its stack (the JTDX FT4 decoder's, which uses
/// ft8var's modules). The caller holds the modem lock across the call.
pub fn on_decoder_thread_wait<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    on_decoder_thread(Box::new(move || {
        let _ = tx.send(f());
    }));
    rx.recv().expect("the ft8var decoder thread died")
}

/// [`decode_frame_var`] with the parameter block given as is (a test that must reproduce
/// `jt9`'s exact parameters uses this).
///
/// # Panics
/// Panics if `iwave.len() < NMAX`, or if `found` panicked (after the decode has finished: the
/// modem is never released mid-decode).
pub fn decode_frame_var_raw(
    iwave: &[i16],
    p: &Ft8VarParams,
    found: &mut dyn FnMut(&Decode),
) -> Vec<Decode> {
    assert!(
        iwave.len() >= crate::NMAX,
        "decode_frame_var needs at least {} samples, got {}",
        crate::NMAX,
        iwave.len()
    );
    let _guard = modem_lock();
    let (tx, rx) = std::sync::mpsc::channel();
    let iwave = iwave[..crate::NMAX].to_vec();
    let params = *p;
    on_decoder_thread(Box::new(move || {
        let mut reply = tx;
        let mut out = vec![Ft8VarDecodeT::default(); crate::MAX_DECODES];
        // SAFETY: iwave holds NMAX samples, `out` MAX_DECODES records; the callback's `user` is
        // `reply`, alive across the call; the caller holds the modem lock until it has `Done`.
        let n = unsafe {
            tempo_fast_sys::ft8var_decode_frame(
                iwave.as_ptr(),
                &params,
                out.as_mut_ptr(),
                out.len() as i32,
                Some(on_var_decode),
                (&mut reply as *mut std::sync::mpsc::Sender<Reply>).cast(),
            )
        };
        out.truncate(n.clamp(0, crate::MAX_DECODES as i32) as usize);
        let _ = reply.send(Reply::Done(out.iter().map(from_var_record).collect()));
    }));
    let mut panic = None;
    loop {
        match rx
            .recv()
            .expect("the ft8var decoder thread died mid-decode")
        {
            Reply::Found(d) => {
                if panic.is_none() {
                    if let Err(e) =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| found(&d)))
                    {
                        panic = Some(e);
                    }
                }
            }
            Reply::Done(all) => {
                if let Some(e) = panic {
                    std::panic::resume_unwind(e);
                }
                return all;
            }
        }
    }
}

/// One period's full pass through the multithreaded decoder: every decode, in the order the
/// decoder accepted them, and each handed to `found`, on this thread, the moment the decoder
/// accepted it (as WSJT-X prints and flushes each). Any thread may call it: the decode itself
/// runs on the decoder's own thread (see `decoder_thread`). There is no give-up point: ft8var
/// runs to the end of its passes.
///
/// # Panics
/// Panics if `iwave.len() < NMAX`, or if `found` panicked.
pub fn decode_frame_var(
    iwave: &[i16],
    req: &VarRequest<'_>,
    found: &mut dyn FnMut(&Decode),
) -> Vec<Decode> {
    decode_frame_var_raw(iwave, &req.params(), found)
}

/// Tell the decoder where `ALLCALL7.TXT` is, and load it (once per process; later calls only
/// report). Returns whether the callsign database is valid: without it ft8var filters nothing
/// against it.
pub fn var_load_callsign_db(dir: &Path) -> bool {
    let path = dir.to_string_lossy().into_owned();
    let _guard = modem_lock();
    on_decoder_thread_wait(move || {
        // SAFETY: a byte buffer and its length; the caller holds the modem lock.
        unsafe {
            tempo_fast_sys::ft8var_set_share_dir(path.as_ptr().cast(), path.len() as i32);
            tempo_fast_sys::ft8var_load_callsign_db() == 1
        }
    })
}

/// What WSJT-X resets when the mode changes (`decoder.f90:190-196`): the average DT and the
/// decoder's callsign hash tables. Call where the standard decoder's a7 table is reset.
pub fn var_reset() {
    let _guard = modem_lock();
    // SAFETY: no arguments; the caller holds the modem lock.
    on_decoder_thread_wait(|| unsafe { tempo_fast_sys::ft8var_reset() })
}

/// The thread count WSJT-X would use for `requested` (0 = automatic) on this machine.
pub fn var_threads(requested: i32) -> i32 {
    // SAFETY: reads the core count; no modem state.
    unsafe { tempo_fast_sys::ft8var_threads(requested) }
}

/// The frequency slice each of `n` threads decodes, as WSJT-X splits `nfa..=nfb`.
pub fn var_slices(nfa: i32, nfb: i32, n: usize) -> Vec<(i32, i32)> {
    let n = n.clamp(1, 12);
    let (mut lo, mut hi) = ([0i32; 12], [0i32; 12]);
    // SAFETY: two 12-element buffers, n <= 12; no modem state.
    unsafe { tempo_fast_sys::ft8var_slices(nfa, nfb, n as i32, lo.as_mut_ptr(), hi.as_mut_ptr()) };
    lo[..n]
        .iter()
        .copied()
        .zip(hi[..n].iter().copied())
        .collect()
}
