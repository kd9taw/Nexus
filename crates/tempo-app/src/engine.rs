//! The live TX/RX engine: ties [`AppState`] + the FT1 modem + an operating mode
//! (chat / auto-QSO / Field Day) to a transport-agnostic slot loop.
//!
//! The engine does not own an audio device. Each slot the host calls
//! [`Engine::poll_tx`] (waveforms to play on TX slots) and [`Engine::ingest`]
//! (decode a captured frame). What gets transmitted depends on the [`mode`]:
//!   - **Chat** — beacons + presence-gated store-and-forward free-text frames.
//!   - **QSO** — the [`tempo_core::qso`] auto-sequencer (running CQ or answering).
//!   - **Field Day** — the [`tempo_core::fieldday`] exchange + dupe-checked log.
//!
//! In every mode, received decodes also update the roster / inbox / link so the
//! UI's Stations and Conversation panels stay populated.
//!
//! [`mode`]: Engine::set_mode

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tempo_core::fieldday::{Exchange, FieldDayStation};
use tempo_core::logbook::{Logbook, QsoRecord};
use tempo_core::qso::{State as QsoState, Station as QsoStation};
use tempo_core::qsy::{Directive, Roamer};
use tempo_core::{channel, spectrum, tempo_fast, tx};

/// The live waterfall row, published on its OWN lock so the UI can read it without
/// contending for the engine mutex.
///
/// The waterfall is display-only: the radio loop (and the Flex/CI-V scope workers) write it,
/// an 8-20 Hz UI poll reads it. Nothing about it needs app state. While it lived behind the
/// engine mutex, every long hold of that mutex froze the waterfall — see
/// [`Engine::spectrum_feed`] for the operator report that led here and the exact holder.
///
/// The lock is taken only to move a `Spectrum` in or clone one out, so a reader can never wait
/// on anything but another such move.
#[derive(Default)]
struct FeedRows {
    audio: Option<(Spectrum, Instant)>,
    rf: Option<(Spectrum, Instant)>,
}

#[derive(Clone, Default)]
pub struct SpectrumFeed {
    rows: Arc<Mutex<FeedRows>>,
}

impl SpectrumFeed {
    /// Publish the audio-FFT row (the rx-dsp thread).
    pub fn publish_audio(&self, row: Spectrum) {
        if let Ok(mut g) = self.rows.lock() {
            g.audio = Some((row, Instant::now()));
        }
    }
    /// Publish a NATIVE RF panadapter row (Flex VITA-49 / Icom CI-V scope).
    pub fn publish_rf(&self, row: Spectrum) {
        if let Ok(mut g) = self.rows.lock() {
            g.rf = Some((row, Instant::now()));
        }
    }
    /// Drop the native row so the audio FFT takes over immediately (DATA mode / scope off).
    pub fn clear_rf(&self) {
        if let Ok(mut g) = self.rows.lock() {
            g.rf = None;
        }
    }
    /// The row to display. This precedence is moved VERBATIM from the old
    /// `Engine::spectrum_row`: a native RF row wins while its rows are fresh (< 1 s), else the
    /// audio row while fresh (< 2 s). `None` only before anything has ever been published —
    /// the Companion/UDP path, which has no local capture and computes on demand.
    pub fn row(&self) -> Option<Spectrum> {
        let g = self.rows.lock().ok()?;
        if let Some((spec, at)) = &g.rf {
            if at.elapsed() < std::time::Duration::from_secs(1) && !spec.row.is_empty() {
                return Some(spec.clone());
            }
        }
        if let Some((spec, at)) = &g.audio {
            if at.elapsed() < std::time::Duration::from_secs(2) {
                return Some(spec.clone());
            }
            // STALE, but the capture path HAS published before: go quiet. Returning None here
            // would send the reader to the engine's Companion fallback, which computes a row
            // from the last decoded buffer — a non-empty row redrawn at 8-20 Hz, i.e. the
            // frozen-ghost streak this whole change exists to remove. An empty row stops the
            // waterfall cleanly, which is the honest failure.
            return Some(Spectrum {
                row: Vec::new(),
                lo_hz: spec.lo_hz,
                hi_hz: spec.hi_hz,
                source: spec.source.clone(),
            });
        }
        None
    }

    /// Test-only: backdate the audio stamp to simulate a capture stream that went silent.
    #[cfg(test)]
    pub fn backdate_audio_for_test(&self, by: std::time::Duration) {
        if let Ok(mut g) = self.rows.lock() {
            if let Some((_, at)) = g.audio.as_mut() {
                *at = Instant::now() - by;
            }
        }
    }
}

use crate::dto::{
    AppSnapshot, DecodeRow, FieldDayQso, FieldDayStatus, OpMode, QsoStatus, QsyStatus,
    RadioSummary, SourceKind, Spectrum, Tier,
};
use crate::settings::Settings;
use crate::station::StationCore;
use crate::AppState;

/// Live CAT read-back for one NON-active radio (dual-radio), fed by the monitor thread. `None` fields
/// mean "not read yet / not reported". `cat_ok` mirrors the active radio's `(Option<bool>)` health.
#[derive(Debug, Clone, Default)]
pub struct RadioLive {
    pub dial_mhz: Option<f64>,
    pub band: Option<String>,
    pub sideband: Option<String>,
    pub smeter_db: Option<i32>,
    pub cat_ok: Option<bool>,
}
use modes::{NativeSource, SignalSource, WsjtxUdpSource};
use std::sync::{Arc, Mutex};
use tempo_core::message::{same_call, Msg};

/// The decoder ([`SignalSource`]) behind its OWN lock, independent of the engine
/// mutex. The heavy per-slot decode runs on a persistent worker thread that locks
/// only this mutex — never the engine mutex — so the engine stays free for the UI
/// (`get_snapshot`) and the radio loop (waterfall feed) during the ~1–2 s decode.
///
/// The `Arc`/`Mutex` is created ONCE per [`Engine`] and never replaced; a tier /
/// source switch swaps the boxed contents *under the lock* (waiting for any decode
/// in flight). That one stable lock is the single serialization point for ALL
/// process-global decode FFI state (the WSJT-X a7 table, the packjt77 hash table,
/// and the FT1 IR-HARQ buffers), so nothing races the C decoder.
pub type SharedSource = Arc<Mutex<Box<dyn SignalSource>>>;

/// Which decode pass a [`DecodeJob`] is — selects the a7 cross-cycle flag and how
/// the result folds back in. Mirrors the three synchronous entry points exactly:
/// [`Engine::ingest`] (Boundary), [`Engine::ingest_early`] (Early), and
/// [`Engine::redecode`] (Redecode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodePass {
    /// Authoritative slot-boundary ingest: `a7_final = true` (save + replay).
    Boundary,
    /// WSJT-X-style early partial pass: `a7_final = false`.
    Early,
    /// F6 review re-decode over retained audio: `a7_final = false`.
    Redecode,
}

impl DecodePass {
    fn a7_final(self) -> bool {
        matches!(self, DecodePass::Boundary)
    }
}

/// Which decode path the job runs — decided under the engine lock in
/// [`Engine::build_decode_job`] so the worker needs no engine state.
enum DecodeBranch {
    /// Native FT1/FT8/FT4 through the [`SignalSource`] (AP context in the job).
    Native,
    /// DX1 robust full-band acquisition (`tempo_fast::deep::decode_band`, stateless).
    TempoDeep,
    /// Companion: drain the upstream WSJT-X/JTDX/MSHV UDP queue (audio ignored).
    Companion,
}

/// One decode job: an OWNED snapshot built under the engine lock, plus an `Arc`
/// clone of the decoder, handed to the worker thread. Self-contained — the worker
/// touches no engine state. The `frame` rides through and comes back in the
/// [`DecodeResult`] so the engine can fold it (`process_decodes` / WAV) with no copy.
pub struct DecodeJob {
    source: SharedSource,
    frame: Vec<f32>,
    branch: DecodeBranch,
    // A-priori request context — native branch only (tempodeep/companion ignore it).
    nfa: i32,
    nfb: i32,
    ndepth: i32,
    mycall: String,
    hiscall: String,
    nqso_progress: i32,
    nfqso: i32,
    frame_time_ms: i64,
    /// Native branch: clear FT1 IR-HARQ buffers before decoding (HARQ disabled).
    harq_reset: bool,
    pass: DecodePass,
    slot: u64,
    /// Decode-context generation this job was built in. If the engine's epoch has
    /// moved on (tier/source/context switch) by the time the result lands, the
    /// result is stale and dropped — it belongs to a decode context that no longer
    /// exists (slot indices / AP context are meaningless across the switch).
    epoch: u64,
    /// This radio chain's private copy of the modem's process-global decode state
    /// (a7 replay table, packjt77 callsign hashes, IR-HARQ pool, cached wideband
    /// spectrum), or `None` for the single-radio path.
    ///
    /// `None` is byte-for-byte the behavior that shipped: the decode runs against
    /// the process-global statics, untouched. `Some` swaps this chain's state in
    /// around the decode so a second chain on another band cannot feed it another
    /// band's callsigns — see [`run_decode_job`].
    ctx: Option<Arc<Mutex<tempo_fast::DecoderCtx>>>,
}

impl DecodeJob {
    /// Attach the owning radio chain's [`DecoderCtx`] to this job.
    ///
    /// Without it the job decodes against the shared process-global modem state,
    /// which is correct for one radio and silently wrong for two.
    ///
    /// [`DecoderCtx`]: tempo_fast::DecoderCtx
    pub fn with_ctx(mut self, ctx: Arc<Mutex<tempo_fast::DecoderCtx>>) -> Self {
        self.ctx = Some(ctx);
        self
    }
}

/// The worker's output for one [`DecodeJob`]: the decodes plus the round-tripped
/// `frame` and the bookkeeping (`pass`/`slot`/`epoch`) the engine needs to fold it.
pub struct DecodeResult {
    decodes: Vec<modes::Decode>,
    frame: Vec<f32>,
    pass: DecodePass,
    slot: u64,
    epoch: u64,
    /// The decode PANICKED and was contained — see [`run_decode_job`]. Carries no
    /// decodes and an empty frame, and exists only so the result still comes back:
    /// the radio loop clears its in-flight latch when a result lands, so a job that
    /// never returns one stops decoding forever.
    failed: bool,
}

impl DecodeResult {
    /// The decodes this job produced, in decoder order.
    pub fn decodes(&self) -> &[modes::Decode] {
        &self.decodes
    }
    /// Which pass produced this result (the loop routes Early vs Boundary).
    pub fn pass(&self) -> DecodePass {
        self.pass
    }
    /// The boundary slot this result belongs to.
    pub fn slot(&self) -> u64 {
        self.slot
    }
}

/// One journaled store-and-forward entry (`pending_msgs.json`). Internal format —
/// field-stable so an old journal still parses after upgrades.
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingMsgJournal {
    to: String,
    text: String,
    created_slot: u64,
    attempts: u32,
    last_attempt_slot: Option<u64>,
    id: char,
}

impl PendingMsgJournal {
    fn from(p: &tempo_core::store::Pending) -> Self {
        Self {
            to: p.to.clone(),
            text: p.text.clone(),
            created_slot: p.created_slot,
            attempts: p.attempts,
            last_attempt_slot: p.last_attempt_slot,
            id: p.id,
        }
    }
    fn into_pending(self) -> tempo_core::store::Pending {
        tempo_core::store::Pending {
            to: self.to,
            text: self.text,
            created_slot: self.created_slot,
            attempts: self.attempts,
            last_attempt_slot: self.last_attempt_slot,
            delivered: false,
            id: self.id,
            // Not journaled: a restored entry at attempts >= cap is re-flagged by the
            // next take_no_acks pass (self-healing), so the format stays field-stable.
            no_acked: false,
            // Not journaled either: re-resolves via resend → the peer's next reply.
            confirmed: false,
        }
    }
}

/// Outcome of folding a [`DecodeResult`] back into the engine.
pub enum DecodeApplied {
    /// The decode context changed while this was in flight — dropped, no effect.
    Stale,
    /// Early partial pass folded (`n` = decodes ingested this pass).
    Early { n: usize },
    /// Boundary pass folded; the loop now runs the deferred TX decision for `slot`
    /// (the retained `frame` comes back for the period-WAV save).
    Boundary {
        n: usize,
        slot: u64,
        frame: Vec<f32>,
    },
}

/// Run one [`DecodeJob`] — the heavy decode, off the engine mutex.
///
/// Shared by the persistent worker thread AND the synchronous [`Engine::ingest`] /
/// [`Engine::ingest_early`] / [`Engine::redecode`] paths, so both produce byte-for-byte
/// identical decodes. Locks ONLY the decoder ([`SharedSource`]) — never the engine
/// mutex — for the whole decode, serializing all process-global decode FFI state.
pub fn run_decode_job(job: DecodeJob) -> DecodeResult {
    // Identify the job before it is consumed — a panic destroys the job, but the
    // loop still needs to know which pass/slot/epoch failed.
    let (pass, slot, epoch) = (job.pass, job.slot, job.epoch);
    // ⭐ CONTAIN THE PANIC. Decoding runs on ONE persistent worker thread, and the
    // radio loop clears `decode_in_flight` only when a result arrives. Without this,
    // a panic anywhere in the decode chain — and the modem wrappers assert freely
    // (`jt65::decode_frame` panics on a short buffer or an out-of-range submode, and
    // the others match) — killed that thread, dropped the result channel, and left
    // the latch stuck true FOREVER. Every later dispatch went to a dead channel via
    // `let _ = tx.send(job)`, which discards the error. The app then decoded nothing
    // for the rest of the session, silently: the waterfall is fed separately, so it
    // kept painting and the app looked alive while it had gone completely deaf.
    //
    // `panic = "abort"` is NOT set for release, so the default unwind applies and the
    // process survives the dead thread — which is exactly what made it silent rather
    // than a visible crash.
    //
    // AssertUnwindSafe: on the panic path we return no decodes and touch no recovered
    // state, so a half-updated decoder cannot leak into the result. The decoder's own
    // global FFI state is re-seeded per job by the caller's context handling.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_decode_job_inner(job))) {
        Ok(r) => r,
        Err(_) => {
            eprintln!(
                "[decode] PANIC in {pass:?} decode for slot {slot} — decode contained, \
                 receive continues. This is a bug; please report it."
            );
            DecodeResult {
                decodes: Vec::new(),
                frame: Vec::new(),
                pass,
                slot,
                epoch,
                failed: true,
            }
        }
    }
}

fn run_decode_job_inner(job: DecodeJob) -> DecodeResult {
    let DecodeJob {
        source,
        frame,
        branch,
        nfa,
        nfb,
        ndepth,
        mycall,
        hiscall,
        nqso_progress,
        nfqso,
        frame_time_ms,
        harq_reset,
        pass,
        slot,
        epoch,
        ctx,
    } = job;
    // Hold the decoder lock across the ENTIRE decode: this is the single lock that
    // serializes the a7 table, the packjt77 hash table and the FT1 IR-HARQ buffers
    // against the engine thread's harq_reset / seed_hash_table / source swaps.
    let mut src = source.lock().unwrap();
    // The decode itself. Run directly when the job carries no per-chain context
    // (the single-radio path, unchanged), or inside `DecoderCtx::scoped` when it
    // does — see the `match ctx` below.
    let mut decode = || match branch {
        DecodeBranch::Companion => {
            // Decodes arrive over UDP; the captured audio is irrelevant.
            src.decode(&modes::DecodeRequest::full_band(&[]))
        }
        DecodeBranch::TempoDeep => {
            // Robust full-passband acquisition — no `modes::Mode`; normalize to the
            // unified Decode. (The lock is held purely to serialize FFI state.)
            tempo_fast::deep::decode_band(&frame, 200.0, 2900.0, tempo_fast::SAMPLE_RATE)
                .into_iter()
                .map(modes::Decode::from)
                .collect()
        }
        DecodeBranch::Native => {
            // IR-HARQ off (or non-FT1 mode): clear buffered RV0 so nothing
            // cross-frame-combines. Exactly where decode_frame reset it.
            if harq_reset {
                tempo_fast::harq_reset();
            }
            let iwave = channel::capture_to_i16(&frame);
            let req = modes::DecodeRequest {
                iwave: &iwave,
                nfa,
                nfb,
                ndepth,
                mycall: &mycall,
                hiscall: &hiscall,
                nqso_progress,
                nfqso,
                frame_time_ms,
            };
            src.decode_a7(&req, pass.a7_final())
        }
    };
    let decodes = match ctx {
        // Single radio: the process-global modem statics ARE this chain's state.
        None => decode(),
        // Two radios in one process: install this chain's modem state, decode,
        // capture the result back — inside the ONE `source.lock()` acquisition above, so no
        // other DECODE can land between the restore and the save. Without this, chain A's a7
        // replay list / callsign hash table / IR-HARQ buffers feed chain B a CRC-valid,
        // well-formed, WRONG decode.
        //
        // The guard is `source.lock()`, NOT `MODEM_LOCK` — engine.rs never takes MODEM_LOCK,
        // which is acquired deeper in crates/ft8|ft4|tempo-fast. Taking it here would deadlock
        // (std Mutex is not reentrant). `source.lock()` is the right guard anyway: this file
        // already documents it as "the single serialization point for ALL process-global
        // decode FFI state".
        //
        // KNOWN GAP, decode-vs-TX: a concurrent TX `encode()` on another thread takes only
        // MODEM_LOCK, which this path releases when `decode()` returns — so a TX can write
        // packjt77's hash table BETWEEN our restore and our save, and the save then captures
        // it into this chain's context. Not reachable today (nothing builds a second chain),
        // and pre-existing in shape, but it must be closed before two chains transmit.
        Some(ctx) => ctx.lock().unwrap().scoped(decode),
    };
    drop(src);
    DecodeResult {
        decodes,
        frame,
        pass,
        slot,
        epoch,
        failed: false,
    }
}

/// Default waterfall resolution (display bins). 512 over the 0–4000 Hz span ≈ 7.8 Hz/bin — ~3×
/// finer than the old 120-bin/22.5 Hz bank, over a wider band. The row is resampled from a
/// Hann-windowed 4096-point FFT (see `tempo_core::spectrum`).
pub const SPECTRUM_BINS: usize = 512;

/// The engine's current operating mode (holds the active sequencer).
///
/// `FieldDayStation` is boxed because it is much larger than the other variants
/// (it carries the whole dupe-checked log) — keeps `Mode` small.
enum Mode {
    Chat,
    Qso {
        station: Box<QsoStation>,
        running: bool,
    },
    FieldDay {
        station: Box<FieldDayStation>,
        running: bool,
    },
}

/// One of our own transmitted messages, recorded per TX slot so the decode feed
/// can show each of our calls (the WSJT-X own-TX rows / "I called them 4 times").
struct OwnTx {
    text: String,
    freq_hz: f32,
    when_unix: u64,
}

/// Drives transmit/receive against the modem and updates [`AppState`].
/// Callsign → "actively uploads to LoTW" (clippy type_complexity extraction).
pub(crate) type LotwResolver = Box<dyn Fn(&str) -> bool + Send + Sync>;

/// Connector legs a queued upload still owes, as a bitmask. A transient-failure
/// retry re-attempts ONLY the legs that failed (never a leg that already
/// succeeded), so a re-queue can't double-push to a connector that doesn't dedupe.
pub mod upload_legs {
    pub const QRZ: u8 = 1 << 0;
    pub const CLUBLOG: u8 = 1 << 1;
    pub const EQSL: u8 = 1 << 2;
    pub const HRDLOG: u8 = 1 << 3;
    pub const N3FJP: u8 = 1 << 4;
    pub const CLOUDLOG: u8 = 1 << 5;
    pub const ALL: u8 = QRZ | CLUBLOG | EQSL | HRDLOG | N3FJP | CLOUDLOG;
}

/// One QSO awaiting connector auto-upload, plus which legs it still owes and how
/// many times it has been retried.
#[derive(Clone)]
pub struct PendingUpload {
    pub rec: tempo_core::logbook::QsoRecord,
    /// Owed connector legs (see [`upload_legs`]); `ALL` on the first attempt.
    pub legs: u8,
    /// Retry count — a record is dropped once it hits [`MAX_UPLOAD_RETRIES`] so a
    /// perpetually-failing service can't pin it in the queue forever.
    pub attempts: u8,
}

/// Give up on a queued upload after this many transient-failure retries (~1 per
/// worker tick / 2 s), so a permanently-down service eventually stops retrying.
pub const MAX_UPLOAD_RETRIES: u8 = 20;

/// Where the STANDING N1MM broadcast should go, or `None` when it is off.
///
/// Two independent conditions, both required: the operator's toggle, and an
/// address to send to. Pulled out of [`Engine::push_to_n1mm`] so "am I
/// broadcasting?" is a pure, testable question rather than a condition buried in
/// the log path.
pub fn n1mm_broadcast_target(s: &Settings) -> Option<String> {
    let addr = s.n1mm_addr.trim();
    (s.n1mm_upload && !addr.is_empty()).then(|| addr.to_string())
}

/// Fill an N1MM `<contactinfo>` for an ORDINARY logged QSO.
///
/// The Field Day emitter in the radio loop fills the contest fields (class,
/// section, points, exchange) from the contest log. This is the other audience: a
/// live map/dashboard plotting each contact as it is logged, so what matters is
/// who, where, which band/frequency, which mode and when. The contest fields are
/// honestly blank — a ragchew claims no contest points and belongs to no section.
pub fn n1mm_contact_for(
    rec: &tempo_core::logbook::QsoRecord,
    mycall: &str,
) -> tempo_net::n1mm::N1mmContact {
    tempo_net::n1mm::N1mmContact {
        mycall: mycall.to_string(),
        call: rec.call.clone(),
        band: tempo_net::band_for_interop(&rec.band),
        // The logged mode token, passed through as-is. A Nexus-native tier
        // (TempoFast / FT1 / DX1) is reported under its real name rather than
        // mapped onto a mode it is not — a map that says "FT8" for an FT1 contact
        // is a wrong answer, where an unknown-but-true label is merely unfamiliar.
        mode: rec.mode.to_uppercase(),
        timestamp: tempo_net::n1mm::utc_timestamp(rec.when_unix),
        section: String::new(),
        gridsquare: rec.grid.clone().unwrap_or_default(),
        points: 0,
        contestname: tempo_net::n1mm::GENERAL_LOG.to_string(),
        freq_10hz: (rec.freq_mhz * 1e5) as u64,
        // A plain QSO's "exchange" is the signal report — what N1MM's own DX log
        // puts here.
        sent_exchange: rec.rst_sent.clone().unwrap_or_default(),
        // One station, one operator on this path (the rotating-operator field is
        // Field Day's, and Field Day does not come through here).
        operator: mycall.to_string(),
        id: tempo_net::n1mm::dedup_id(rec.when_unix, &rec.call, 0),
    }
}

pub struct Engine {
    pub app: AppState,
    settings: Settings,
    /// Station-wide state (logbook, connector queues, DXCC/rarity/LoTW resolvers, POTA
    /// activation, journal paths, PC-clock offset) — everything that is true of the
    /// OPERATOR rather than of this receive/transmit chain. See [`StationCore`].
    ///
    /// Owned by value: today there is exactly one chain, so an `Arc<Mutex<_>>` would add
    /// lock traffic to the single-radio TX path and buy nothing. Sharing arrives with the
    /// second chain.
    station: StationCore,
    /// Transmit audio offset (Hz) — where our signal sits in the SSB passband.
    tx_offset_hz: f32,
    /// Receive audio offset (Hz) — the green waterfall marker / where we focus.
    rx_offset_hz: f32,
    /// Keep TX offset fixed when RX changes (WSJT-X "Hold Tx Freq").
    hold_tx_freq: bool,
    tx_parity: u64,
    /// When true (default), answering a heard station in chat auto-picks the OPPOSITE
    /// T/R cycle (FT8-style); an explicit 1st/2nd selection clears it. A CQ run holds
    /// its current cycle regardless.
    tx_cycle_auto: bool,
    beacon_every: u64,
    tx_queue: VecDeque<String>,
    /// Open-broadcast (to-all) free-text frames, sent unconditionally on TX
    /// slots in Chat mode — no recipient presence required.
    broadcast_queue: VecDeque<String>,
    /// Rotating chunk message-id for broadcasts ('A'..'Z').
    broadcast_id: u8,
    /// Rolling history of our own transmitted messages (newest-last) — surfaced as
    /// `mine` rows in the decode feed so the operator sees each of their calls
    /// (WSJT-X "your message in Band Activity"). Capped; cleared on halt/mode change.
    own_tx: VecDeque<OwnTx>,
    /// The last decoded RX frame, retained for the F6 redecode and the waterfall row.
    ///
    /// ⭐ Stored as i16, the decoder's OWN input format — the same choice WSJT-X makes
    /// (`short int d2[NTMAX*RX_SAMPLE_RATE]`, commons.h). This is a permanent
    /// allocation sized by the T/R period, and f32 doubled it for nothing: the decode
    /// path immediately converts to i16 anyway. At FST4's 1800 s period that is 21.6 M
    /// samples — 86 MB as f32, 43 MB here.
    ///
    /// The round-trip is EXACT, not approximate. `capture_to_i16` is
    /// `round(x * 32767)`; dividing by the same constant and re-applying it returns
    /// the identical i16, so a redecode sees byte-for-byte what the live decode saw.
    last_rx: Option<Vec<i16>>,
    /// Decodes from the most recent [`Engine::ingest`] (for the network layer to
    /// emit over the WSJT-X UDP API / PSK Reporter).
    last_decodes: Vec<modes::Decode>,
    /// Session count of IR-HARQ rescues: decodes recovered by joint-combining
    /// retransmissions (rv > 0). Surfaced as a stats readout.
    harq_rescues: u32,
    /// Active RX signal source — the user-selectable native-vs-companion switch.
    /// For native tiers (FT1/FT8/FT4) this is a [`NativeSource`] whose mode tracks
    /// the selected [`Tier`]; DX1 decodes via its own robust path (see [`ingest`]).
    ///
    /// [`ingest`]: Engine::ingest
    ///
    /// Behind its own lock (see [`SharedSource`]) so the heavy decode runs on the
    /// decode-worker thread without ever taking the engine mutex. The `Arc`/`Mutex`
    /// is stable for the engine's lifetime; a tier/source switch swaps the boxed
    /// contents under the lock.
    source: SharedSource,
    /// Decode-context generation. Bumped by [`clear_decode_context`] (band QSY /
    /// tier / source / mode switch) so a decode that was in flight across the switch
    /// lands as stale and is dropped by [`apply_decode_result`] — its slot indices
    /// and AP context no longer mean anything.
    ///
    /// [`clear_decode_context`]: Engine::clear_decode_context
    /// [`apply_decode_result`]: Engine::apply_decode_result
    decode_epoch: u64,
    /// Which kind of source [`source`](Self::source) currently is. Tracked so
    /// [`set_tier`](Engine::set_tier) only re-points the *native* source and a
    /// live companion isn't clobbered, and so [`ingest`](Engine::ingest) routes
    /// companion decodes off the network regardless of the selected tier.
    source_kind: SourceKind,
    mode: Mode,
    /// Whether normal slot TX is enabled. False = Monitor-off (transmit muted):
    /// [`Engine::poll_tx`] returns nothing. Also forced false by the watchdog.
    tx_enabled: bool,
    /// Skip Tx1 (WSJT-X parity): when answering a CQ, open the QSO with the report
    /// (Tx2) instead of the grid (Tx1). A SESSION flag — deliberately not persisted, so
    /// it resets to off every launch exactly like WSJT-X's control. Set by the UI via
    /// `set_skip_tx1`; consumed in `call_station_ctx`.
    pub skip_tx1: bool,
    /// Whether the operator is holding a steady tune carrier. While true,
    /// [`Engine::poll_tx`] suppresses normal slot TX (the radio loop plays a
    /// continuous carrier instead).
    tuning: bool,
    /// Whether the transmit watchdog has tripped (auto-halted TX).
    tx_watchdog: bool,
    /// Unix-secs when the current unattended-transmit run began (first TX after the
    /// last operator action), or `None` if not transmitting. The watchdog trips on
    /// WALL-CLOCK elapsed since this (`tx_watchdog_min` minutes), like WSJT-X — not on
    /// TX air-time, which fired ~2x too late since FT8/FT4 transmit every other slot.
    tx_watchdog_start: Option<u64>,
    /// Recent decode `|dt|` magnitudes (seconds), most-recent-last, for the
    /// DT-derived time-sync health estimate.
    recent_dt: VecDeque<f32>,
    /// True once we've seen at least one decode (so time-sync is judged, not
    /// assumed-OK).
    seen_decode: bool,
    /// A rig read (freq) actually succeeded this session — the cockpit dial/mode are
    /// rig-confirmed rather than the persisted seed (read-only launch provenance).
    rig_confirmed: bool,
    /// Per-area tier memory: the structured tier (FT8/FT4) last used in the DX
    /// area and the chat tier (FT1/DX1) last used in MSG — so switching areas
    /// round-trips without losing the operator's pick (the "FT4 lost through
    /// msg" bug). None until an area has been left once.
    last_dx_tier: Option<Tier>,
    last_msg_tier: Option<Tier>,
    /// Work-a-spot navigation hint: bumped by [`Engine::work_spot_split`]; the
    /// UI (any window) navigates to `work_view`'s cockpit when the tick changes.
    work_tick: u64,
    /// The mode of the last worked spot ("digital" | "phone" | "cw").
    work_view: Option<String>,
    /// Callsign of the last worked spot — carried in the snapshot so a click in a pop-out
    /// window (band map / board) can prefill the MAIN window's log Call field. Cleared on a
    /// call-less work so a stale call never prefills.
    work_call: Option<String>,
    /// The signal report I last sent the current QSO's DX station (RST sent),
    /// captured from the sequencer's outgoing (R)Report. Reset per QSO.
    qso_report_sent: Option<i32>,
    /// A completed QSO held for the operator to confirm before it is logged
    /// (WSJT-X "Prompt me to log QSO"). `Some` only while `prompt_to_log` is on
    /// and a finished contact is awaiting confirm/discard.
    pending_log: Option<QsoRecord>,
    /// Whether the active QSO has already been auto-logged (so it logs exactly
    /// once when the sequencer reaches `Done`). Reset when a new QSO starts.
    qso_logged: bool,
    /// Unix-secs when the current QSO's exchange began (TIME_ON) — set when answering a
    /// station or when our CQ first gets a reply; `None` between QSOs. The log records
    /// this as the contact start, with the completion time as TIME_OFF.
    qso_start_unix: Option<u64>,
    /// Whether the operator is RUNNING (called CQ) vs answering a specific station.
    /// When running, a completed QSO returns to calling CQ (WSJT-X's run workflow).
    cq_running: bool,
    /// Transmit-percentage scheduler for the BEACON tiers (WSPR / FST4W). Carries
    /// the anti-consecutive carry state across intervals, so it must persist rather
    /// than be rebuilt per slot — rebuilding would reset the spread and skew the
    /// achieved duty cycle.
    beacon_sched: tempo_core::beacon::BeaconScheduler,
    /// The last beacon slot a transmit decision was taken for, so one decision is
    /// made per interval no matter how often `poll_tx` is called within it.
    beacon_decided_slot: Option<u64>,
    /// The decision itself, held for the length of the interval.
    beacon_tx_this_slot: bool,
    /// Chat-native CQ run (Tempo): while on, an idle own-parity TX slot re-sends the
    /// structured CQ — the WSJT-X-style "keep calling until someone answers" loop the
    /// one-shot Call CQ button couldn't do. Runtime-only (resets each launch, like
    /// Skip Tx1): a fresh session never keys on its own.
    chat_cq: bool,
    /// The run auto-pauses when the operator sends a directed message (sequential
    /// policy: work the answer, don't interleave CQ into the QSO) and auto-resumes
    /// after directed traffic has been idle for CHAT_CQ_RESUME_SLOTS.
    chat_cq_paused: bool,
    /// Slot of the last directed-traffic activity (send or owed-ACK) — drives the
    /// idle auto-resume above.
    chat_cq_last_directed: u64,
    /// Consecutive UNANSWERED chat-CQ transmissions. Resets on any directed activity or
    /// operator (re)start; at the budget (cq_max_calls, default 10) the run STOPS —
    /// bounded like every other chat transmission after the 2026-07 cadence rework.
    chat_cq_unanswered: u32,
    /// Yield-to-RX: set when a directed burst is released; once the burst finishes
    /// draining, the chat arm skips `chat_yield_slots` own-parity TX slots — a real
    /// listening window between overs, so a conversation alternates instead of the
    /// queue immediately grabbing the next slot. NEVER touches tx_parity (that's the
    /// collision-avoidance contract with the peer).
    chat_yield_pending: bool,
    chat_yield_slots: u8,
    /// The slot index of the most recent decoded frame — used to set TX parity to
    /// the OPPOSITE period when working a clicked station (WSJT-X double-click).
    last_decode_slot: Option<u64>,
    /// Set when a directed call is armed (double-click): the radio loop should key
    /// the CURRENT period immediately if it's our TX parity and the over still
    /// fits, instead of waiting a full T/R cycle for the next boundary. One-shot.
    immediate_tx: bool,
    /// Set when the operator clicks a section / works a Needed spot / QSYs: the radio
    /// loop must apply the dial + mode RIGHT NOW (single-click precision), CLEARING any
    /// `mode_giveup` so a click is never ignored — even if a prior attempt gave up on
    /// that mode. One-shot, drained by the loop.
    immediate_retune: bool,
    /// One-shot override for the mode class the NEXT `set_frequency` routes on, for callers whose
    /// mode intent isn't in `Settings`/engine state yet at QSY time. Only [`Engine::aprs_tune`]
    /// needs it: `set_frequency` clears `aprs_fm` (a QSY leaves the APRS context), so without the
    /// override an APRS tune would route on the mode class of the section the operator is LEAVING.
    /// Taken by the routing step; the normal path derives the class from [`Engine::route_mode`].
    route_intent: Option<crate::settings::RouteMode>,
    /// Rolling received-decode history (slot, row) across the last several T/R
    /// cycles — NOT just the last slot. This is what makes a roster/late click on
    /// a caller resolve to the RIGHT QSO step: `last_decodes` is replaced on every
    /// ingest, so by the time the operator clicks, the caller's message was gone
    /// and the sequencer silently fell back to Tx1 (the "sent my grid instead of a
    /// report" on-air bug). Also the source for answer parity.
    decode_history: std::collections::VecDeque<(u64, modes::Decode)>,
    /// Messages the WSJT-X-style EARLY decode pass already ingested for an
    /// upcoming boundary slot, so the boundary's full-window decode ingests only
    /// the stragglers it newly found (no double rows / double observe).
    early_seen: Option<(u64, std::collections::HashSet<String>)>,
    /// One-shot: drop Enable-Tx once the CURRENT over finishes playing — set
    /// when our final 73 goes out with "Disable Tx after sending 73" on. The
    /// radio loop consumes it AFTER tx_until expires; disabling immediately
    /// would trip the hard-stop path and cut the 73 itself mid-over.
    pending_tx_disable: bool,
    /// Deferred WSJT-X-style CW ID (armed when the final 73 leaves; consumed
    /// by the service on TX-idle so the ID never keys over the FT8 audio).
    pending_cw_id: bool,
    /// JTAlert-style callsign highlights from UDP HighlightCallsign (type 13):
    /// uppercased call → (bg, fg) CSS hex. None/None entries are removed.
    /// Value = (insertion sequence, bg, fg). The sequence exists so the
    /// at-cap eviction is OLDEST-FIRST: evicting `.keys().min()` was
    /// deterministic but age-blind and lexicographically biased — digit-prefixed
    /// DX calls (2E0, 9M, 5B…) sort below letters and were always the victims.
    highlights: std::collections::HashMap<String, (u64, Option<String>, Option<String>)>,
    /// Monotonic insertion counter for the highlight FIFO above.
    highlight_seq: u64,
    /// Bumped by an inbound UDP Clear (type 3) — the UI watches it and erases
    /// the matching pane(s). Visual-only: the engine's decode context (answer
    /// parity, history) is NOT a window and stays intact.
    clear_tick: u32,
    /// One-shot: the operator hit Erase — the radio loop tells cooperating
    /// apps via an outbound Clear (window byte; 0 = Band, 1 = Rx, 2 = both).
    pending_udp_clear: Option<u8>,
    /// The last ingest's decodes as they were ON AIR (pre hound-split/rewrite)
    /// — what UDP consumers receive. Identical to `last_decodes` outside Hound.
    last_wire_decodes: Vec<modes::Decode>,
    /// Directed-CQ token for CQ runs ("DX", "NA", "POTA", …): applied to the
    /// run's CQ message at start AND on the return-to-CQ after each pileup
    /// contact. None = plain CQ. Sticky until the operator starts a plain run
    /// (stock: the edited Tx6 text persists the same way).
    cq_dir: Option<String>,
    /// TX dial shift (Hz) for the over poll_tx just generated under WSJT-X
    /// Split Operation — the audio was reduced into 1500–2000 Hz and the loop
    /// must move the TX dial by this much before keying. 0 = no shift.
    tx_dial_shift_hz: i64,
    /// Desired rig SPLIT state: `Some(tx_dial_mhz)` = split on with that TX dial;
    /// `None` = simplex. Set by work_spot (pile-up "UP n" spots) and cleared by any
    /// plain QSY/work; the loop applies it via `split_dirty` (one-shot).
    split_tx_mhz: Option<f64>,
    /// One-shot "apply the split state now" flag for the radio loop.
    split_dirty: bool,
    /// RIT / XIT clarifier offsets in Hz (0 = off) and the active VFO — the CAT-panel controls.
    /// Write-only + optimistic (no read-back): the loop applies a change once and the snapshot
    /// mirrors the last commanded value, like the RF-power / filter-width path.
    rit_hz: i32,
    xit_hz: i32,
    active_vfo_b: bool, // false = VFO A, true = VFO B
    rit_dirty: bool,
    xit_dirty: bool,
    vfo_dirty: bool,
    /// Transient operator mode override for Phone (`"USB"`/`"LSB"`/`"FM"`), or `None` = AUTO
    /// (the band-derived sideband policy). NOT persisted — the cockpit mode picker sets it; a
    /// band change clears it so QSY re-asserts the auto sideband. FM as a persistent default
    /// still lives in `settings.phone_mode`.
    sideband_override: Option<String>,
    /// CW transmit queue (CAT keyer path): expanded CW text the radio loop drains and
    /// keys via `rig.send_morse`. Operator-initiated; gated by `tx_enabled` (Monitor).
    cw_queue: VecDeque<String>,
    /// Recent EXPANDED CW transmissions (macros resolved) — a TX echo the cockpit shows
    /// so the operator sees exactly what went out. Capped; cleared with the RX transcript.
    cw_sent: VecDeque<String>,
    /// A CW-keyer failure to surface (e.g. the rig rejected CAT send_morse), else None.
    /// Set by the radio loop; cleared when the operator switches keyer back-end.
    cw_keyer_error: Option<String>,
    /// Worked-station QRZ info for the `{HISNAME}`/`{HISSTATE}` CW-macro tokens, pushed by the
    /// frontend on a callbook lookup and keyed to the contact (`cw_peer_call`) so a stale
    /// lookup can't key the wrong name. All empty until a lookup resolves. See `set_cw_peer_info`.
    cw_peer_call: String,
    cw_peer_name: String,
    cw_peer_state: String,
    /// One-shot: the operator hit Abort — the radio loop calls `rig.stop_morse` and
    /// clears the queue, then resets this.
    cw_abort: bool,
    /// Manual PTT for live phone (operator push-to-talk): the radio loop keys the rig
    /// while true. Gated by `tx_enabled` (Monitor). Operator-initiated only.
    manual_ptt: bool,
    /// Desired RF output power as a 0.0–1.0 fraction; `None` = leave the rig's power
    /// alone. The radio loop applies it via `rig.set_power` when it changes.
    rf_power: Option<f32>,
    /// RF power as READ BACK from the rig (the knob's truth) — kept separate from
    /// the commanded `rf_power` so a 750 ms poll can never clobber a just-issued
    /// set that the radio loop hasn't applied yet.
    rig_rf_power: Option<f32>,
    /// Desired MIC GAIN (0.0–1.0); `None` = leave the rig's mic gain alone. Same
    /// commanded-vs-read-back split as `rf_power`/`rig_mic_gain` so an in-flight drag wins.
    mic_gain: Option<f32>,
    /// Mic gain as READ BACK from the rig (the poll's truth).
    rig_mic_gain: Option<f32>,
    /// Desired / read-back NOISE-REDUCTION level (0.0–1.0), same commanded-vs-observed split.
    nr_level: Option<f32>,
    rig_nr_level: Option<f32>,
    /// Desired / read-back AGC time constant as "fast"|"mid"|"slow" (the loop maps it to the
    /// rig's value). Commanded until the poll confirms; `None` when the rig doesn't report it.
    agc: Option<String>,
    rig_agc: Option<String>,
    /// Rig CAT S-meter (dB relative to S9) from the radio-loop poll; `None` when the
    /// rig doesn't report STRENGTH. Observed-only, RX-only.
    rig_smeter_db: Option<i32>,
    /// Rig CAT transmit meters from the radio-loop poll — the mirror image of the S-meter:
    /// read ONLY while keyed, `None` while receiving or when the rig doesn't report them.
    /// SWR ratio (1.0–6.0), ALC 0.0–1.0, Po in watts, COMP in dB. Observed-only.
    rig_tx_swr: Option<f32>,
    rig_tx_alc: Option<f32>,
    rig_tx_po_w: Option<f32>,
    rig_tx_comp_db: Option<f32>,
    /// Rig's actual mode read back over CAT (Hamlib name, e.g. "USB"/"LSB"/"FM").
    /// DISPLAY-ONLY — the cockpit flags a mismatch with the commanded mode, but this
    /// never overwrites the canonical commanded sideband (App-side invariant).
    rig_mode: Option<String>,
    /// Rig CAT DSP-function states, per `[nb, nr, notch(ANF), comp, vox]`, from the radio-loop
    /// poll. `None` = the rig doesn't support that func (hide the toggle); `Some(bool)` =
    /// supported + current on/off. Observed-only, same `None = can't do it` idiom as `rig_smeter_db`.
    rig_funcs: [Option<bool>; 5],
    /// Pending func toggles from the UI, per the same `[nb, nr, notch, comp, vox]` order; the
    /// radio loop drains + applies them next cycle (mirrors the split-request seam). Off the TCP path.
    pending_func: [Option<bool>; 5],
    /// Rig RX passband width (Hz) from the poll; `None` = unknown / rig default. Observed-only.
    rig_passband: Option<u32>,
    /// The radio's RECEIVE frequency ranges (Hz, inclusive), read once per CAT confirmation from
    /// Hamlib's capability table. `None` = unknown (not probed, no CAT, or an unparseable reply).
    ///
    /// This is what lets Nexus know a radio cannot reach 2 m BEFORE commanding it there — the
    /// FTdx10 report, where opening APRS auto-tuned 144.390 at an HF-only radio. **`None` must
    /// always fail OPEN** (see [`Self::rig_covers_mhz`]).
    rig_rx_ranges: Option<Vec<(u64, u64)>>,
    /// The last dial (MHz) the radio REFUSED, for the UI to explain itself. Set by the radio loop
    /// when it gives up on an out-of-range frequency; cleared by any accepted QSY.
    rig_refused_dial_mhz: Option<f64>,
    /// Pending RX filter-width set (Hz) from the UI; the loop drains + applies it via set_mode.
    pending_passband: Option<u32>,
    /// Pending native-scope control one-shots from the UI (native Icom CI-V only): span in Hz
    /// (± half-width), reference level in tenths of a dB, and center(false)/fixed(true) mode. The
    /// radio loop drains each and calls the CivDaemon while not keyed.
    pending_scope_span: Option<u32>,
    pending_scope_ref: Option<i32>,
    pending_scope_fixed: Option<bool>,
    /// FlexRadio native-panadapter controls (read continuously by the FlexSpectrum worker, which
    /// applies them via `display pan set`): the pan BANDWIDTH in Hz (default 200 kHz) and an
    /// optional reference level in dBm (`None` = leave the Flex on auto). Distinct from the Icom
    /// one-shots above — the Flex worker polls the current value, so these are live, not drained.
    flex_pan_span_hz: f64,
    flex_pan_ref_dbm: Option<i32>,
    /// A foreign CAT-broker client is holding PTT (arbitrated in `broker_ptt`).
    broker_ptt: bool,
    /// Phone voice-keyer: pending 12 kHz mono samples to transmit. The radio loop drains
    /// this (gated on `tx_enabled`), keys PTT, plays it, and drops PTT when it's out — the
    /// same path the soundcard CW keyer uses. Set by `send_voice`.
    voice_tx: Option<Vec<f32>>,
    /// One-shot voice-keyer abort: the loop flushes the output ring + unkeys, then clears it.
    voice_abort: bool,
    /// True while recording a voice message — the radio loop appends captured audio to
    /// `record_buf`. Set by `start_recording`, cleared by `stop_recording`.
    recording: bool,
    /// Accumulated 12 kHz mono capture for the in-progress recording.
    record_buf: Vec<f32>,
    /// QSO recording (audio bridge): while true the radio loop STREAMS live RX capture to
    /// the WAV at `qso_record_path` (no RAM buffer — the loop owns the file sink). Set by
    /// `start_qso_recording`. Unlike the voice-keyer record, this persists across UI nav.
    qso_recording: bool,
    /// Target WAV path for the in-progress QSO recording (set by `start_qso_recording`).
    qso_record_path: Option<String>,
    /// Rolling window of the most recent captured audio, fed continuously by the
    /// radio loop (independent of the decoder) so the waterfall reflects LIVE
    /// Cached waterfall row — recomputed once per audio feed in the radio loop, so `get_spectrum_row`
    /// (the ~20-30 Hz UI poll) just returns it instead of re-running the Goertzel under the engine
    /// lock on every call (the source of the "choppy" Phone scope under lock/IPC contention).
    /// The latest NATIVE RF panadapter row (Flex VITA / Icom CI-V) + when it arrived. Preferred
    /// over `spectrum_cache` while fresh (< 1 s); a stalled native source falls back to audio.
    /// Fed by `set_spectrum_rf`; `None` until a native scope worker is running.
    /// A longer rolling RX-audio ring (several seconds) — the batch per-channel decode
    /// behind the wideband CW skimmer (the 4096-sample waterfall window is too short).
    cw_audio: Vec<f32>,
    /// A 15 s rolling RX-audio ring for the AI CW decoder (the DeepCW model's window).
    /// Fed only while `settings.ai_cw_enabled` — empty (zero cost) otherwise.
    ai_cw_audio: Vec<f32>,
    /// The AI CW decoder's rolling transcript (window decodes stitched by absolute
    /// stream time — no re-emitted overlap). Bounded; oldest text drains off the front.
    ai_cw_text: String,
    /// Total samples ever fed to the AI-CW ring — the absolute stream clock the decode
    /// thread stitches windows with. Resets when the feature turns off.
    ai_cw_fed: u64,
    /// AI CW decoder status for the UI ("listening…", "model not installed", …).
    ai_cw_status: String,
    /// Streaming single-signal CW decoder at the operator's pitch. Fed incrementally so it
    /// accumulates a PERSISTENT transcript (the batch decoder re-read the ring each poll,
    /// so its text churned and the last characters vanished within seconds).
    cw_stream: tempo_core::cw_decode::CwStreamDecoder,
    /// The per-QSO WAV ring — the last ~60 s of RX audio, written to a file on log when
    /// `settings.save_qso_wav` is on (an automatic archive of each contact).
    qso_audio: Vec<f32>,
    /// Rig/CAT connection status surfaced to the UI: `(ok, detail)`. `ok` is
    /// `None` for VOX (no CAT), `Some(true/false)` for a CAT/serial rig. Written
    /// by the radio loop when it (re)opens or probes the rig.
    cat_status: (Option<bool>, String),
    /// Dual-radio: LIVE read-back state for the NON-active radios, keyed by radio id. The monitor
    /// thread (one CAT poll per non-active radio, read-only) feeds these via `observe_radio_*`; the
    /// snapshot's `radios[]` shows each radio's live freq/mode/S-meter/CAT-health from here instead of
    /// its stale profile `last_*`. The ACTIVE radio is NOT in this map — its live state is the flat
    /// `rig_*`/`settings.dial_mhz` block, driven by the existing `observe_rig_*`.
    radio_live: std::collections::HashMap<u32, RadioLive>,
    /// Set by `test_cat` to ask the radio loop to re-probe the current rig and
    /// refresh `cat_status`; the loop clears it via [`Engine::take_cat_reprobe`].
    cat_reprobe: bool,
    /// Set by the radio loop when the sound card failed to open, so the UI can
    /// explain a blank waterfall instead of failing silently.
    audio_error: Option<String>,
    /// Coordinated-QSY ("move together") state machine — a SEPARATE, opt-in
    /// function. Fully inert unless `settings.qsy_enabled` is true, so the primary
    /// Chat/QSO/Field-Day paths are unaffected when the operator hasn't enabled it.
    qsy: Roamer,
    /// RTTY RX decoder armed (session-only runtime state, never persisted — the
    /// decoder must never come up armed at launch). RX decode only; no TX path.
    rtty_armed: bool,
    /// Drain buffer for the RTTY decode thread: 12 kHz RX audio accumulates here
    /// while armed; the thread empties it via [`Engine::take_rtty_audio`]. Empty
    /// (zero cost) while disarmed.
    rtty_audio: Vec<f32>,
    /// Ring of decoded RTTY characters (+ per-char ATC confidence), capped at
    /// [`RTTY_TEXT_CAP`] — the cockpit transcript. Pushed by the decode thread.
    rtty_chars: VecDeque<tempo_core::rtty::DecodedChar>,
    /// Latest AFC offset (Hz) reported by the RTTY demodulator.
    rtty_afc_hz: f32,
    /// Whether the RTTY demodulator's AFC has acquired-then-frozen (locked).
    rtty_afc_locked: bool,
    /// One-shot: drop + rebuild the RX demodulator (the AFC-reset command — a
    /// wrong-neighbor acquire-then-freeze recovers by re-acquiring from scratch).
    rtty_afc_reset: bool,
    /// Audio center (Hz) the RTTY decoder is netted on — the mark/space midpoint.
    /// `None` = un-netted, which resolves to the nominal 2125 Hz mark
    /// (`center = 2125 + shift/2`) so the pair sits on today's 2125/2295 at any
    /// shift. A waterfall click sets it via [`Engine::rtty_net`], which rebuilds
    /// the demod around the new center. RX only; no TX path touches this.
    rtty_center: Option<f32>,
    /// Queued RTTY transmissions, one whole message per entry (already uppercased
    /// and ITA2-filtered). Filled ONLY by [`Engine::rtty_send_text`] — an explicit
    /// operator send — never on arm/launch. The radio loop keys ONE message at a
    /// time via [`Engine::poll_rtty_one`] (gated on tx_enabled + privileges +
    /// the Rtty operating mode + not tuning), pacing on the real bit-stream
    /// duration; Stop TX / halt clears this queue so the rest never keys.
    rtty_queue: VecDeque<String>,
    /// One-shot: abort the RTTY transmission in progress (the loop stops the FSK
    /// keying thread / flushes the AFSK audio ring and unkeys PTT).
    rtty_abort: bool,
    /// True while the radio loop is keying an RTTY over (stamped by the loop each
    /// tick; the cockpit's sending indicator).
    rtty_sending: bool,
    /// An RTTY keyer failure to surface (FSK port wouldn't open / PTT refused).
    rtty_keyer_error: Option<String>,
    /// The RTTY auto-sequencer, present ONLY while the operator has flipped Auto
    /// on (`None` = the manual-only behavior). Driven by the RX feed (in
    /// [`Engine::push_rtty_decode`]) + the service tick ([`Engine::rtty_auto_service`]);
    /// it NEVER transmits without an explicit CQ/Answer initiate (ARRL FD 6.4).
    rtty_seq: Option<tempo_core::rtty::RttySeq>,
    /// True from the moment an auto-sequencer `SendText` is enqueued until that
    /// over has fully keyed out — the edge the service tick turns into exactly one
    /// `on_tx_complete` (restarting the sequencer's reply timer).
    rtty_auto_over: bool,
    /// APRS (AFSK-1200 / AX.25) RX decoder arm state + HOW it was armed (session-only, never
    /// persisted). See [`AprsArm`] — the distinction gates the auto-ack, so it is TX-safety state.
    aprs_arm: AprsArm,
    /// The operator explicitly stopped the decoder this session, so entering the APRS view must
    /// not quietly start it again behind them. Session-only; a manual arm always still works.
    aprs_auto_arm_declined: bool,
    /// Drain buffer for the APRS decode thread (same pattern as `rtty_audio`). Empty while disarmed.
    aprs_audio: Vec<f32>,
    /// Recently-decoded APRS packets, newest last, capped at [`APRS_HEARD_CAP`]. Pushed by the
    /// decode thread; polled by the cockpit.
    aprs_heard: Vec<AprsHeard>,
    /// Decoder health — see [`AprsHealth`]. Written by the decode thread on every drain (not only
    /// when something decodes: "nothing arrived" is the reading that matters most). Reset on arm.
    aprs_health: AprsHealth,
    /// Pre-rendered APRS TX audio (12 kHz) — beacons, messages, acks. The radio loop keys ONE at a
    /// time via [`Engine::poll_aprs_tx`]; Stop TX / halt clears it.
    aprs_tx_queue: VecDeque<Vec<f32>>,
    /// Rolling APRS message line-number (001..999) for outgoing messages, so the recipient can ack.
    aprs_msg_seq: u16,
    /// Per-STATION state keyed by callsign-SSID — what the map and the station list read. Distinct
    /// from `aprs_heard`, which is a packet LOG: see [`AprsStation`] for why conflating the two
    /// made the map flash.
    aprs_stations: std::collections::HashMap<String, AprsStation>,
    /// RF-heard TNC2 lines awaiting contribution to APRS-IS. Filled ONLY by [`AprsSource::Rf`]
    /// pushes and only while the uplink setting is on; drained by the APRS-IS thread, which
    /// applies the gating rules. Empty (and free) with the iGate off.
    aprs_uplink_queue: VecDeque<String>,
    /// Live APRS-IS feed state, written by the socket thread. Session-only, never persisted.
    aprs_is_status: AprsIsStatus,
    /// APRS holds the rig in FM SIMPLEX (session-only, never persisted). Set by [`Engine::aprs_tune`];
    /// makes `rig_mode_effective` command FM and forces the repeater config to simplex (APRS is not
    /// a Phone/Digital operating section, so it can't ride the Phone-FM override). Cleared by any
    /// `set_frequency` (a normal QSY) or `set_operating_mode` — i.e. the moment the operator leaves
    /// APRS — so FM never follows them onto another band/mode.
    aprs_fm: bool,
    /// The rig is parked on an FM CHANNEL — a repeater or FM simplex frequency (session-only, never
    /// persisted). Set by [`Engine::repeater_tune`]; makes `rig_mode_effective` command FM and
    /// `route_mode` answer Fm, for the same reason `aprs_fm` above exists: the Program section is a
    /// workbench, not a Phone/Digital operating section, so a tune from it can't ride the Phone-FM
    /// override and would otherwise inherit the rig mode of whichever section the operator last
    /// operated — demodulating a repeater as packet-USB. UNLIKE `aprs_fm` it does NOT force simplex:
    /// carrying the machine's shift and tone is the whole point.
    ///
    /// Cleared by every QSY, section change and radio switch, exactly like `aprs_fm`, and gated at
    /// read time on the dial still being at/above 29 MHz — the same threshold `Settings::rig_mode`
    /// applies to Phone-FM, so a knob-QSY down to HF can never leave FM forced.
    fm_channel: bool,
    /// SSTV RX decoder armed (session-only runtime state, never persisted).
    sstv_armed: bool,
    /// Drain buffer for the SSTV decode thread (same pattern as `rtty_audio`).
    sstv_audio: Vec<f32>,
    /// In-flight SSTV decode progress (mode, lines, downscaled preview), pushed
    /// by the decode thread. `None` = no image in flight.
    sstv_progress: Option<SstvProgress>,
    /// An operator-initiated SSTV image TX waiting for the radio loop to key it:
    /// the whole pre-encoded 12 kHz buffer + its exact duration + mode label.
    /// Filled ONLY by [`Engine::sstv_send`] (an explicit operator send) — never on
    /// arm/launch/mode-select. The radio loop takes it via [`Engine::poll_sstv_tx`]
    /// behind every TX gate; one image in flight at a time (no queue). `None` = idle.
    sstv_tx: Option<SstvTxJob>,
    /// One-shot: abort the SSTV image in progress (the loop drops the feed, flushes
    /// the output ring and unkeys PTT). Mirrors `rtty_abort`.
    sstv_abort: bool,
    /// True while the radio loop is streaming an SSTV image (stamped by the loop each
    /// tick; the cockpit's TX indicator). Mirrors `rtty_sending`.
    sstv_sending: bool,
    /// Mode label of the image being (or last) transmitted, for the TX DTO. Set by
    /// [`Engine::sstv_send`]; cleared by Stop / halt / TX-disarm.
    sstv_tx_mode: Option<String>,
    /// In-flight SSTV TX progress `(played_ms, total_ms)`, stamped by the radio loop.
    /// `None` = no image queued or sending.
    sstv_tx_progress: Option<(f64, f64)>,
}

/// Samples of recent audio kept for the live waterfall spectrum (~0.34 s at
/// 12 kHz) — enough for a responsive, reasonably-resolved Goertzel bank.
/// CW-decode RX ring: ~6 s at 12 kHz — long enough for a full callsign exchange at the
/// speeds an operator reads, short enough that the per-poll decode stays cheap.
const CW_WINDOW: usize = 72_000;
/// The AI CW decoder's ring: 15 s at 12 kHz — exactly the DeepCW model's decode window.
const AI_CW_WINDOW: usize = 180_000;
/// Per-QSO WAV ring: ~60 s at 12 kHz — captures the exchange around a logged contact.
const QSO_WAV_WINDOW: usize = 720_000;
/// RTTY decoded-character ring cap: ~4000 chars ≈ tens of minutes of copy at 45.45 baud.
const RTTY_TEXT_CAP: usize = 4000;
/// AFSK RTTY mark tone (Hz) — mirrors `tempo_audio::rtty_afsk::MARK_HZ` (tempo-app can't
/// depend on tempo-audio); used only to judge where the AFSK emission lands vs the dial.
const RTTY_AFSK_MARK_HZ: f64 = 2125.0;
/// Cap on the armed RTTY/SSTV audio drain buffers (~10 s at 12 kHz). The decode
/// threads normally empty these every ~100 ms; the cap only bites if a thread
/// stalls, dropping oldest audio (garbling that decode) instead of growing RAM
/// without bound.
const RX_TAP_CAP: usize = 120_000;
/// Cap on the decoded-APRS list kept for the cockpit (oldest dropped past this).
const APRS_HEARD_CAP: usize = 300;

/// One decoded APRS packet, flattened for the cockpit list. Built from a
/// [`tempo_core::aprs::AprsPacket`] by the RX decode thread (kept out of the hot path).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AprsHeard {
    pub source: String,
    pub dest: String,
    pub path: Vec<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub symbol_table: char,
    pub symbol_code: char,
    /// "position" | "mice" | "message" | "status" | "other".
    pub kind: &'static str,
    /// Comment / message / status text, ready to show.
    pub text: String,
    pub speed_knots: Option<u16>,
    pub course_deg: Option<u16>,
    /// For a `message` kind: who it was addressed to (base call, no SSID). Lets the UI thread
    /// messages by conversation instead of collapsing them under the sender.
    pub addressee: Option<String>,
    /// For a `message` kind: the sender's line number, if any (used to ack).
    pub msg_id: Option<String>,
    /// Unix seconds the packet was decoded (drives the age column).
    pub at_unix: i64,
    /// WHERE this packet came from. Never inferred — see [`AprsSource`].
    pub source_kind: AprsSource,
    /// Weather readings, when this packet carried any.
    pub wx: Option<AprsWxDto>,
    /// The packet as a TNC2 monitor line, for the raw readout and (RF only) for the iGate uplink.
    /// Lossy UTF-8: this field is for DISPLAY. The uplink re-renders from the frame bytes so a
    /// non-UTF-8 information field is never corrupted on its way to the network.
    pub raw: String,
}

/// Where a packet reached us from — doctrine, not decoration.
///
/// "My antenna heard this" and "a server told me about it" are different facts with different
/// operational meaning: the first proves the RF chain works and the station is genuinely within
/// reach, the second proves neither. The operator must be able to tell them apart at a glance, so
/// this rides every station everywhere it is shown, and the internet feed can be hidden entirely.
///
/// It also gates the iGate: only [`AprsSource::Rf`] is ever offered for upload. That is the first
/// and least negotiable of the gating rules, and it is enforced here — structurally, by which
/// packets can reach the queue — rather than by a check that could be forgotten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AprsSource {
    /// Decoded off the air by this station's own receiver.
    #[default]
    Rf,
    /// Delivered by the APRS-IS internet feed.
    Inet,
    /// Heard both ways — the same packet arrived off-air and over the internet.
    Both,
}

impl AprsSource {
    /// Merge two sightings of the same packet. RF + internet = [`AprsSource::Both`]; anything else
    /// keeps what it had. Never downgrades: once our own antenna has heard a station, no later
    /// internet copy may erase that.
    pub fn merged(self, other: AprsSource) -> AprsSource {
        if self == other {
            self
        } else {
            AprsSource::Both
        }
    }
}

/// How the APRS decoder came to be armed.
///
/// ⚠️ TX SAFETY, not bookkeeping. Entering the APRS view arms the decoder so the operator does not
/// land on a dead screen — but that convenience must not also arm an UNATTENDED TRANSMITTER.
/// [`Engine::aprs_auto_ack`] keys the radio with nobody asking, so it stays behind TWO independent
/// operator acts: arming Monitor by hand AND enabling TX. Auto-arm supplies neither, so
/// [`AprsArm::Auto`] is receive-only, always. See the tests around
/// `auto_armed_never_auto_acks_even_with_tx_enabled`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AprsArm {
    /// Not decoding.
    #[default]
    Off,
    /// Armed by entering the APRS view. Decodes; NEVER auto-acks.
    Auto,
    /// Armed by an explicit operator act. The only state in which an ack may key — and even then
    /// only with TX enabled and every gate in [`Engine::aprs_tx_gate`] satisfied.
    Explicit,
}

impl AprsArm {
    /// Whether the decoder is running at all (either way of being armed).
    pub fn is_armed(self) -> bool {
        self != AprsArm::Off
    }
}

/// What the APRS decoder is actually HEARING — the readout that tells a silent screen apart from
/// a silent band.
///
/// Only frames that pass the AX.25 FCS ever become an [`AprsHeard`], and everything else is
/// dropped without a trace. That made three very different situations look identical: the app
/// listening to the wrong sound card, a mistuned/over-driven channel whose every frame fails CRC,
/// and a genuinely quiet band. All three showed an empty list and an empty map. These counters
/// separate them — audio level proves the tap is fed, and seen-vs-decoded proves whether the
/// demodulator is finding packets it cannot check.
// NOT `Copy`: carrying the radio's NAME means this owns a String. It is cloned once per cockpit
// poll, which is nothing next to the packet list beside it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AprsHealth {
    /// Armed-ness AND how it was armed — one field, so "is it running" and "may it ack" can never
    /// drift apart. The UI derives armed-ness from this rather than carrying a second flag.
    pub arm: AprsArm,
    /// Peak |sample| of the last drain that actually CARRIED audio.
    ///
    /// Deliberately not "the last drain": the decode thread polls every 100 ms while the radio
    /// loop, which feeds the tap, can take far longer than that per iteration (it issues blocking
    /// CAT — up to 2500 ms on slow serial). Empty drains are therefore normal and frequent, and
    /// letting one zero this would flap the readout to "no audio" on a perfectly healthy channel.
    pub audio_peak: f32,
    /// Unix seconds audio last arrived at the tap — samples ARRIVING, at any level.
    ///
    /// ⚠️ This and [`AprsHealth::audio_peak`] answer different questions and must never be merged
    /// back into one "no audio" test. A squelched USB codec streams a continuous run of digital
    /// ZEROS: samples keep arriving (this stays fresh) while the level is zero. Nothing arriving
    /// at all is a broken capture device; arriving-but-silent is an idle FM channel. Conflating
    /// them told an operator with a perfectly good setup to go fix their audio routing.
    pub last_audio_unix: Option<i64>,
    /// Drains the decode thread has reported since arming, carrying audio or not.
    ///
    /// Exists so "we have never seen audio" can be told apart from "we have not looked yet":
    /// arming resets this health, and the cockpit re-reads it immediately, so for a fraction of a
    /// second `last_audio_unix` is legitimately `None`. Without this the chip flashed a
    /// check-your-input-device alarm every time the operator armed Monitor.
    pub drains: u64,
    /// HDLC frames the deframer recovered since arming: candidates, BEFORE the FCS check.
    pub frames_seen: u64,
    /// Of those, how many passed the FCS and became packets. `frames_seen` climbing while this
    /// stays put is the signature of a channel being heard but not cleanly.
    pub frames_decoded: u64,
    /// Unix seconds of the most recent successful decode.
    pub last_decode_unix: Option<i64>,
    /// Unix seconds a frame candidate was last recovered. Pairs with `frames_seen` the way
    /// `last_decode_unix` pairs with `frames_decoded`: the counters are CUMULATIVE SINCE ARMING
    /// and the audio level is LIVE, so without a timestamp the UI cannot tell "packets are
    /// failing right now" from "two candidates turned up six minutes ago". Rendering those in
    /// one present tense produced "2 packets were heard ... peak -99 dBFS" — self-contradictory,
    /// because nothing is heard at -99 dBFS.
    pub last_frame_seen_unix: Option<i64>,
    /// Peak of the drain that carried the most recent frame candidate — the BURST-TIME level.
    ///
    /// `audio_peak` is the live drain peak, which between packets is the gap/hiss level. An
    /// operator reading it seconds after a burst sees the gap, not the burst: the field report
    /// "-62 dBFS" was read 9 s after the packet. Advice about burst level has to be based on the
    /// level *during a burst*, so it is captured when the candidate is counted.
    pub frame_peak: f32,
    /// Largest burst-time peak this listening session — the best look the decoder has had at a
    /// real packet, so a single quiet burst doesn't set the verdict forever.
    pub max_frame_peak: f32,
    /// Near-full-scale samples in the drain that carried the most recent frame candidate.
    /// Non-zero means the burst is hitting the rails somewhere in the rig→app path.
    pub frame_clipped_samples: u32,
    /// Name of the radio the decoder is listening to — the ACTIVE radio, since the APRS tap follows
    /// it. Empty when the roster has no profile for it.
    ///
    /// ⚠️ Exists because of a REGRESSION that cost a field session. A merge changed which of two
    /// 2 m-capable radios a band activation resolves to; the tap followed the new one, whose audio
    /// was set up for a different mode, and APRS went silent with nothing on screen naming the
    /// radio it was listening to. Wrong-radio silence is indistinguishable from a dead band unless
    /// the app says which radio it is on.
    pub radio_name: String,
    /// How many enabled radios explicitly cover the band in use. `> 1` means the activation had a
    /// real choice, so the operator needs told which way it went; `<= 1` means there was nothing to
    /// choose and saying so would just be noise.
    pub band_radio_count: u32,
}

/// Below this peak a recovered "frame" cannot be told from the deframer's flag-hunt locking onto
/// dither, so it is not counted as evidence (-80 dBFS).
///
/// ⚠️ MEASURED, not guessed, and deliberately far below any plausible signal. Two field data
/// points from KD9TAW's IC-9700 bracket it: squelch-closed dither reads **-99 dBFS** (19 dB below
/// this, so noise is rejected) and a real burst read **-62 dBFS** (18 dB above, so it counts).
///
/// It is NOT a decodability threshold. The Bell-202 discriminator compares mark ENERGY against
/// space energy, so absolute level cancels — measured decode holds down to -140 dBFS in the
/// absence of noise. Only SNR matters. Anything about "too quiet to decode" is therefore wrong;
/// see `APRS_HEALTHY_BURST_MIN` for what low level actually costs.
pub const APRS_NOISE_FLOOR_PEAK: f32 = 0.0001;

/// Peak below which a drain reads as silence for DISPLAY purposes (-60 dBFS). Mirrors
/// `SILENT_PEAK` in `AprsCockpit.tsx`; the two must move together.
///
/// Note this is 20 dB ABOVE [`APRS_NOISE_FLOOR_PEAK`], and that gap is deliberate: a burst at
/// -62 dBFS is quiet enough to display as near-silence but far too loud to dismiss as dither.
/// Gating frame counting on THIS value (as the first cut did) discarded real packets — the field
/// burst that failed CRC sat 2 dB below it.
pub const APRS_SILENT_PEAK: f32 = 0.001;

fn fmt_aprs_addr(a: &tempo_core::aprs::Address) -> String {
    if a.ssid == 0 {
        a.call.clone()
    } else {
        format!("{}-{}", a.call, a.ssid)
    }
}

/// Render an APRS frame to 12 kHz TX audio (~32 opening flags ≈ 0.2 s TXDELAY so the receiver's DCD
/// locks before the data). Shared by the beacon / message / ack senders.
fn render_aprs_frame(frame: &tempo_core::aprs::Frame) -> Vec<f32> {
    use tempo_core::aprs;
    aprs::modulate(&aprs::nrzi_encode(&aprs::encode_frame(
        &frame.encode(),
        32,
        3,
    )))
}

impl AprsHeard {
    /// Flatten a decoded packet for display, stamped with its decode time, where it came from, and
    /// the TNC2 line it was decoded from.
    pub fn from_packet(
        pkt: &tempo_core::aprs::AprsPacket,
        at_unix: i64,
        source_kind: AprsSource,
        raw: String,
    ) -> Self {
        use tempo_core::aprs::{AprsBody, AprsInfo};
        let pos = pkt.position();
        let mut addressee = None;
        let mut msg_id = None;
        let (kind, text, sym_t, sym_c, spd, crs) = match &pkt.body {
            AprsBody::MicE(m) => (
                "mice",
                m.comment.clone(),
                m.symbol_table,
                m.symbol_code,
                Some(m.speed_knots),
                Some(m.course_deg),
            ),
            AprsBody::Info(AprsInfo::Position(p)) => (
                "position",
                p.comment.clone(),
                p.symbol_table,
                p.symbol_code,
                None,
                None,
            ),
            AprsBody::Info(AprsInfo::Object {
                name,
                killed,
                position,
            }) => {
                let label = if *killed {
                    format!("{name} (killed) {}", position.comment)
                } else {
                    format!("{name} {}", position.comment)
                };
                (
                    "object",
                    label,
                    position.symbol_table,
                    position.symbol_code,
                    None,
                    None,
                )
            }
            AprsBody::Info(AprsInfo::Message(msg)) => {
                addressee = Some(msg.addressee.clone());
                msg_id = msg.id.clone();
                (
                    "message",
                    format!("\u{2192}{}: {}", msg.addressee, msg.text),
                    ' ',
                    ' ',
                    None,
                    None,
                )
            }
            AprsBody::Info(AprsInfo::Status { text, .. }) => {
                ("status", text.clone(), ' ', ' ', None, None)
            }
            AprsBody::Info(AprsInfo::Other { body, .. }) => {
                ("other", body.clone(), ' ', ' ', None, None)
            }
        };
        AprsHeard {
            source: fmt_aprs_addr(&pkt.source),
            dest: fmt_aprs_addr(&pkt.dest),
            path: pkt.path.iter().map(fmt_aprs_addr).collect(),
            lat: pos.map(|(la, _)| la),
            lon: pos.map(|(_, lo)| lo),
            symbol_table: sym_t,
            symbol_code: sym_c,
            kind,
            text,
            speed_knots: spd,
            course_deg: crs,
            addressee,
            msg_id,
            at_unix,
            source_kind,
            wx: pkt.weather().map(AprsWxDto::from),
            raw,
        }
    }

    /// The duplicate key for store-level merging: the same station saying the same thing. Matches
    /// the APRS-IS server's own rule — origin, destination, and payload, with the PATH EXCLUDED,
    /// because one transmission reaching us off-air and again through an iGate is one packet.
    fn dupe_key(&self) -> (&str, &str, &str) {
        (&self.source, self.kind, &self.text)
    }
}

/// How long after a packet lands a second copy of it (from the other source) still counts as the
/// same packet. Matches the APRS-IS server's own sliding duplicate window.
const APRS_DUPE_MERGE_SECS: i64 = 30;

/// Cap on the STATION store, by station count rather than packet count.
///
/// Sized so a busy filtered feed never churns visibly: a 150 km radius over a dense metro carries
/// a few hundred stations, so this is several times the worst realistic case. Eviction is by
/// oldest-last-heard, and the age window ([`Settings::aprs_station_ttl_min`]) does the real
/// retention work — the cap is a memory ceiling, not a policy.
pub const APRS_STATION_CAP: usize = 2000;

/// Cap on RF-heard lines waiting to be gated to APRS-IS. Bounded so a feed that is down (or an
/// uplink left on with no network) cannot grow the queue without limit; the oldest is dropped,
/// because a stale position is the least valuable thing to contribute.
const APRS_UPLINK_QUEUE_CAP: usize = 200;

/// A weather report on the wire — the serializable mirror of [`tempo_core::aprs::AprsWx`].
///
/// tempo-core carries no serde dependency, so turning its types into wire DTOs is this crate's job.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AprsWxDto {
    pub wind_dir_deg: Option<u16>,
    pub wind_mph: Option<u16>,
    pub gust_mph: Option<u16>,
    pub temp_f: Option<i16>,
    /// Hundredths of an inch.
    pub rain_1h_in100: Option<u16>,
    pub rain_24h_in100: Option<u16>,
    pub rain_midnight_in100: Option<u16>,
    pub humidity_pct: Option<u8>,
    /// Tenths of a hectopascal — 10156 is 1015.6 hPa.
    pub pressure_tenth_hpa: Option<u32>,
}

impl From<tempo_core::aprs::AprsWx> for AprsWxDto {
    fn from(w: tempo_core::aprs::AprsWx) -> Self {
        AprsWxDto {
            wind_dir_deg: w.wind_dir_deg,
            wind_mph: w.wind_mph,
            gust_mph: w.gust_mph,
            temp_f: w.temp_f,
            rain_1h_in100: w.rain_1h_in100,
            rain_24h_in100: w.rain_24h_in100,
            rain_midnight_in100: w.rain_midnight_in100,
            humidity_pct: w.humidity_pct,
            pressure_tenth_hpa: w.pressure_tenth_hpa,
        }
    }
}

/// One STATION, accumulated from every packet it has sent — the unit the map and the station list
/// are actually about.
///
/// ⭐ WHY THIS EXISTS. The packet log ([`Engine::aprs_heard`]) is a 300-entry ring capped by COUNT
/// with no age expiry. That is fine for a scrolling log and wrong for a map: with the APRS-IS feed
/// running, 300 packets is two to five minutes of traffic, so a station beaconing on a perfectly
/// normal ten-minute cycle was evicted before its next beacon — it vanished from the map and came
/// back, over and over. Operators saw the icons flashing. A map has to be about stations and their
/// age, not about the last N packets to arrive.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AprsStation {
    /// Callsign-SSID — the station identity, and the store key.
    pub call: String,
    /// Last known position. Sticky: see [`AprsStation::merge`] for why a later packet without one
    /// must never erase it.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub symbol_table: char,
    pub symbol_code: char,
    /// Kind of the most recent packet that carried real content.
    pub kind: &'static str,
    /// Most recent comment / status text.
    pub text: String,
    pub speed_knots: Option<u16>,
    pub course_deg: Option<u16>,
    /// Digipeater path of the most recent packet.
    pub path: Vec<String>,
    /// Most recent packet as a TNC2 line, for the raw readout.
    pub raw: String,
    /// Unix seconds of the most recent packet from this station, either source. Drives both the age
    /// column and the fade.
    pub last_heard_unix: i64,
    /// Unix seconds this station was last decoded OFF THE AIR by this receiver, if ever.
    pub last_rf_unix: Option<i64>,
    /// Unix seconds this station last arrived via APRS-IS, if ever.
    pub last_inet_unix: Option<i64>,
    /// Derived from the two timestamps above — so "how did this reach us" can never drift from the
    /// evidence for it.
    pub source_kind: AprsSource,
    /// Packets from this station this session.
    pub packets: u32,
    /// When this station was first heard this session.
    pub first_heard_unix: i64,
    /// Latest weather readings, when this station sends them. Sticky like the position: a status
    /// packet from a weather station must not blank its last reading.
    pub wx: Option<AprsWxDto>,
}

impl AprsStation {
    /// Start a station from its first packet.
    fn new(h: &AprsHeard) -> AprsStation {
        let mut st = AprsStation {
            call: h.source.clone(),
            lat: None,
            lon: None,
            symbol_table: ' ',
            symbol_code: ' ',
            kind: h.kind,
            text: String::new(),
            speed_knots: None,
            course_deg: None,
            path: Vec::new(),
            raw: String::new(),
            last_heard_unix: h.at_unix,
            last_rf_unix: None,
            last_inet_unix: None,
            source_kind: h.source_kind,
            packets: 0,
            first_heard_unix: h.at_unix,
            wx: None,
        };
        st.merge(h);
        st
    }

    /// Fold another packet from this station into its state.
    ///
    /// ⚠️ **A packet without a position must not erase the position we already have.** A moving
    /// station interleaves position reports with status and message packets, and the naive
    /// "latest packet wins" rule would blank its coordinates every time one arrived — dropping it
    /// off the map between beacons, which is the same bug in a different disguise. Same for the
    /// symbol: a message packet carries none.
    fn merge(&mut self, h: &AprsHeard) {
        self.packets = self.packets.saturating_add(1);
        self.last_heard_unix = self.last_heard_unix.max(h.at_unix);
        match h.source_kind {
            AprsSource::Rf => self.last_rf_unix = Some(h.at_unix),
            AprsSource::Inet => self.last_inet_unix = Some(h.at_unix),
            // A packet already merged as Both counts for both channels.
            AprsSource::Both => {
                self.last_rf_unix = Some(h.at_unix);
                self.last_inet_unix = Some(h.at_unix);
            }
        }
        self.source_kind = match (self.last_rf_unix, self.last_inet_unix) {
            (Some(_), Some(_)) => AprsSource::Both,
            (Some(_), None) => AprsSource::Rf,
            (None, Some(_)) => AprsSource::Inet,
            // Unreachable: the match above always set one.
            (None, None) => self.source_kind,
        };
        if h.lat.is_some() && h.lon.is_some() {
            self.lat = h.lat;
            self.lon = h.lon;
            // Course and speed belong to the position that carried them.
            self.speed_knots = h.speed_knots;
            self.course_deg = h.course_deg;
        }
        if h.symbol_table != ' ' || h.symbol_code != ' ' {
            self.symbol_table = h.symbol_table;
            self.symbol_code = h.symbol_code;
        }
        if !h.text.is_empty() {
            self.text = h.text.clone();
        }
        // Sticky, for the same reason the position is: a weather station also sends status and
        // message packets, and those must not blank its last reading.
        if h.wx.is_some() {
            self.wx = h.wx;
        }
        if !h.path.is_empty() {
            self.path = h.path.clone();
        }
        if !h.raw.is_empty() {
            self.raw = h.raw.clone();
        }
        self.kind = h.kind;
    }
}

/// The station roster plus the aging doctrine that produced it, so the UI cannot disagree with the
/// backend about when a station is stale.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AprsStationsView {
    /// Stations heard within the age window, newest-heard first.
    pub stations: Vec<AprsStation>,
    /// Minutes of silence after which a station is dropped entirely.
    pub ttl_min: u32,
    /// Minutes of silence after which a station starts to fade. Derived, never configured
    /// separately — see [`Engine::aprs_stations`].
    pub fade_after_min: u32,
}

/// What the APRS-IS internet feed is doing — the counterpart to [`AprsHealth`] for the other inlet.
///
/// Deliberately separate from `AprsHealth`: the two answer different questions and fail
/// independently. A green internet chip beside a silent RF chip is precisely the diagnostic the
/// feed was added to provide — internet stations plotting while the antenna hears nothing proves
/// the fault is in the RF chain, not in the app.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AprsIsStatus {
    /// The operator has the feed switched on. (Independent of the RF decoder's arm state.)
    pub enabled: bool,
    /// A session is up and the server answered the login.
    pub connected: bool,
    /// The login was accepted as VERIFIED — the precondition for any upload. A read-only feed
    /// (`pass -1`) is unverified by design and works perfectly.
    pub verified: bool,
    /// Packet lines received from the feed this session.
    pub packets: u64,
    /// Unix seconds of the most recent line — tells "connected but quiet" from "connected".
    pub last_packet_unix: Option<i64>,
    /// The operator has the RX iGate on.
    pub uplink_enabled: bool,
    /// Packets contributed to APRS-IS this session.
    pub uploaded: u64,
    /// RF-heard packets the gating rules refused. Not an error — it is mostly the loop guard doing
    /// its job — but a counter that only ever climbs is worth showing.
    pub gate_rejected: u64,
    /// The most recent refusal reason, for the tooltip.
    pub last_reject: Option<String>,
}
/// SSTV gallery session-list cap (mirrors the on-disk `gallery.json` cap).
pub(crate) const SSTV_GALLERY_CAP: usize = 200;
/// Hard cap on a single SSTV image's key-down (seconds), watchdog-independent. The
/// longest legitimate mode (PD290) is ~295 s; anything past this is a bug/abuse and is
/// refused before keying — defense-in-depth above the per-send TX-watchdog budget.
const SSTV_MAX_TX_SECS: f64 = 330.0;

/// Live progress of an in-flight SSTV decode, pushed by the decode thread and
/// read by the `get_sstv_state` poll.
#[derive(Debug, Clone, PartialEq)]
pub struct SstvProgress {
    /// Mode label, e.g. "Scottie 1".
    pub mode: String,
    /// Total scan lines in this mode's image.
    pub lines_total: u32,
    /// Scan lines decoded so far.
    pub lines_done: u32,
    /// Downscaled preview dimensions (0 until the first lines land).
    pub preview_w: u32,
    pub preview_h: u32,
    /// Raw RGB preview bytes (`preview_w × preview_h × 3`), nearest-neighbor
    /// downscale of the partial image.
    pub preview_rgb: Vec<u8>,
    /// Radio mistuning for this image, in Hz: `observed_leader_hz - 1900`, taken
    /// from the VIS header.
    ///
    /// Surfaced because the SSTV band view REPLACES the spectrum with decoded
    /// pixels while an image is coming in (operator's explicit choice), and the
    /// thing that costs you is exactly what the spectrum would have shown: whether
    /// you are tuned right. The decoder already computed it and used it only for
    /// diagnostics — stating the number outright is cheaper than making the
    /// operator infer it from a picture that is skewed.
    pub hedr_shift_hz: f64,
}

/// A ready-to-transmit SSTV image: the whole pre-encoded 12 kHz PCM buffer, the
/// mode's human label, and the exact key-down duration. Built in the command
/// layer (encode runs off the engine lock), handed to the engine by
/// [`Engine::sstv_send`], and taken by the radio loop via [`Engine::poll_sstv_tx`].
#[derive(Debug, Clone, PartialEq)]
pub struct SstvTxJob {
    /// Full over-the-air waveform (`f32` PCM at 12 kHz = `tempo_fast::SAMPLE_RATE`).
    pub samples: Vec<f32>,
    /// Human-readable mode label, e.g. "Scottie 1".
    pub mode_name: String,
    /// Exact duration of `samples` in milliseconds — the precomputed PTT hold.
    pub duration_ms: f64,
}

/// Compact RTTY state for the `get_rtty_state` poll: armed flag, AFC, the
/// decoded-text ring with per-character confidence (0–100, parallel to `text`'s
/// chars — render low values faint), plus the TX side (configured baud/shift/
/// backend, the live sending flag, and any keyer failure to surface).
#[derive(Debug, Clone, PartialEq)]
pub struct RttyRxState {
    pub armed: bool,
    pub afc_hz: f32,
    pub afc_locked: bool,
    /// Current mark/space audio tones (Hz) the demod is netted on — the waterfall
    /// mark/space cursor positions. Un-netted = the nominal 2125/2295 pair.
    pub mark_hz: f32,
    pub space_hz: f32,
    pub text: String,
    pub conf: Vec<u8>,
    pub baud: f64,
    pub shift_hz: u32,
    pub backend: String,
    pub sending: bool,
    pub keyer_error: Option<String>,
    // --- Auto-sequencer surface (meaningful only while `auto` is true) ---
    /// The RTTY auto-sequencer is active (the operator's Auto toggle is on).
    pub auto: bool,
    /// Live sequencer state: `idle` | `calling_cq` | `answering` |
    /// `exchange_sent` | `confirmed` | `done`.
    pub seq_state: String,
    /// The station currently being worked, once one is locked.
    pub peer: Option<String>,
    /// The peer's copied exchange in schema order, `(key, value)`.
    pub peer_exchange: Vec<(String, String)>,
    /// A CQ surfaced from the transcript for the operator to click-to-answer
    /// (only while Auto is on). Surfacing ONLY — clicking it is the human gate;
    /// it never transitions the machine on its own.
    pub heard_cq: Option<String>,
}

/// One operation applied to the RTTY auto-sequencer via [`Engine::rtty_drive`].
enum RttyOp {
    /// Operator initiates a CQ run (a human-initiate gate).
    StartCq,
    /// Operator answers a surfaced CQ — search & pounce (the other gate).
    Answer(String),
    /// Freshly decoded characters arrived from the RX thread.
    Feed(Vec<tempo_core::rtty::DecodedChar>),
    /// Clock tick — drives the timeout → AGN / repeat / abort discipline.
    Tick,
    /// The engine finished keying the last over (restarts the reply timer).
    TxComplete,
}

/// The `rtty_state` seq-state string the UI switches on.
fn seq_state_label(s: tempo_core::rtty::SeqState) -> &'static str {
    use tempo_core::rtty::SeqState;
    match s {
        SeqState::Idle => "idle",
        SeqState::CallingCq => "calling_cq",
        SeqState::Answering => "answering",
        SeqState::ExchangeSent => "exchange_sent",
        SeqState::Confirmed => "confirmed",
        SeqState::Done => "done",
    }
}

/// Window of recent decode DT samples used for the time-sync health median.
/// Map a UI DSP-func name to its `[nb, nr, notch, comp, vox]` slot index (the same order the
/// radio loop uses for the `["NB","NR","ANF","COMP","VOX"]` Hamlib tokens). `None` = unknown name.
fn func_index(func: &str) -> Option<usize> {
    match func.to_ascii_lowercase().as_str() {
        "nb" => Some(0),
        "nr" => Some(1),
        "notch" => Some(2),
        "comp" => Some(3),
        "vox" => Some(4),
        _ => None,
    }
}

/// A pending one-click POTA/SOTA hunt expires after this long — activations
/// end; a stale pend must never stamp a park on an unrelated contact.
pub(crate) const HUNT_TTL_SECS: u64 = 4 * 3600;

const DT_WINDOW: usize = 16;
/// Time-sync is considered OK while median(|dt|) is under this many seconds.
const DT_OK_THRESHOLD: f32 = 0.5;

/// The waveform an over needs, reduced to plain data.
///
/// Deliberately carries no borrow of the engine: this is the half of a transmit
/// that touches the MODEM, and the modem is serialized behind
/// `tempo_fast_sys::MODEM_LOCK`, which a running decode can hold for the length
/// of that decode. Building it while the ENGINE mutex is held made every Tauri
/// snapshot poll and every UI command queue up behind a decode — the window
/// went "not responding" for as long as the decoder took. Splitting the build
/// out lets the radio loop release the engine before it waits.
///
/// Nothing here changes WHEN we key: the wait is the same wait, in the same
/// place. Only the engine lock is no longer held across it.
pub enum TxWaveform {
    /// DX1: 8-FSK, non-coherent.
    Deep { text: String, f0: f32 },
    /// FT1: 4-CPM. `rv` is the IR-HARQ redundancy version (QSO retransmissions
    /// escalate it; Chat and Field Day always send RV0).
    Fast { text: String, f0: f32, rv: i32 },
    /// Everything driven by the `modes` trait — FT8, FT4, Q65, FST4, MSK144,
    /// JT65, and the WSPR/FST4W beacons.
    Native {
        kind: modes::ModeKind,
        text: String,
        f0: f32,
    },
}

impl TxWaveform {
    /// Synthesise the audio. **Takes the modem lock; call it with no other lock
    /// held.** Returns empty on any refusal, which every caller already treats
    /// as "do not key".
    pub fn build(&self) -> Vec<f32> {
        match self {
            TxWaveform::Deep { text, f0 } => {
                tempo_fast::deep::encode_wave(text, *f0, tempo_fast::SAMPLE_RATE)
            }
            TxWaveform::Fast { text, f0, rv } => {
                tx::build_rv(text, tempo_fast::SAMPLE_RATE, *f0, *rv).wave
            }
            TxWaveform::Native { kind, text, f0 } => {
                let Some(mode) = modes::tx_mode(*kind) else {
                    return Vec::new();
                };
                let tones = mode.encode(text);
                if tones.is_empty() {
                    return Vec::new();
                }
                mode.gen_wave(&tones, tempo_fast::SAMPLE_RATE, *f0)
            }
        }
    }
}

/// A decided over: what to send, and the slot/tier it was decided for.
///
/// `slot` and `tier` are carried so [`Engine::commit_tx`] can REFUSE a plan that
/// no longer matches the engine. The radio loop releases the engine mutex while
/// [`TxWaveform::build`] runs, so the operator can change tier or the slot can
/// roll over in between — keying a waveform built for the previous tier is
/// exactly the wrong-mode emission the transmit guards exist to prevent.
pub struct TxPlan {
    pub slot: u64,
    pub tier: Tier,
    pub waveform: TxWaveform,
    /// Beacon overs run the wall-clock watchdog at COMMIT, matching the order
    /// the single-shot path used (build, then check).
    beacon: bool,
}

impl Engine {
    /// Construct from explicit identity (back-compat; uses default settings).
    pub fn new(mycall: &str, mygrid: &str, tx_parity: u64) -> Self {
        let settings = Settings {
            mycall: mycall.to_string(),
            mygrid: mygrid.to_string(),
            tx_even: tx_parity.is_multiple_of(2),
            ..Settings::default()
        };
        Self::with_settings(settings)
    }

    /// Construct from full [`Settings`].
    pub fn with_settings(settings: Settings) -> Self {
        // Passive launch (safety): never auto-transmit on startup. The CQ beacon
        // is a deliberate, per-session opt-in — even if a saved settings file has
        // `beacon: true` (e.g. persisted from an earlier build), the app boots in
        // listen-only and the operator arms the beacon when ready. This is why
        // delaying the first beacon wasn't enough: at launch it must not call CQ
        // at all until the operator acts.
        let mut settings = settings;
        settings.beacon = false;
        let mut app = AppState::new(&settings.mycall, &settings.mygrid);
        app.set_radio(settings.dial_mhz, &settings.band, &settings.sideband);
        app.set_implicit_ack(settings.chat_implicit_ack);
        // Derive the TX-slot parity + audio offsets from settings (read before
        // `settings` is moved into the struct).
        let tx_parity = if settings.tx_even { 0 } else { 1 };
        let tx_offset_hz = settings.tx_offset_hz;
        let rx_offset_hz = settings.rx_offset_hz;
        let hold_tx_freq = settings.hold_tx_freq;
        // Coordinated QSY: configured from settings, enabled only if the operator
        // had it on (home = the current channel; partner is set when a peer is
        // selected). Inert while disabled.
        let mut qsy = Roamer::new();
        qsy.configure(settings.qsy_set.clone(), settings.qsy_cadence);
        if settings.qsy_enabled {
            let home = crate::bandplan::channel_for_dial(settings.dial_mhz)
                .map(|c| c.band)
                .unwrap_or_else(|| settings.band.clone());
            qsy.enable(home, None);
        }
        Self {
            app,
            settings,
            station: StationCore::new(),
            tx_offset_hz,
            rx_offset_hz,
            hold_tx_freq,
            tx_parity,
            tx_cycle_auto: true,
            beacon_every: 8,
            tx_queue: VecDeque::new(),
            broadcast_queue: VecDeque::new(),
            own_tx: VecDeque::new(),
            broadcast_id: 0,
            last_rx: None,
            last_decodes: Vec::new(),
            harq_rescues: 0,
            // Default native source = FT8 (matches the default link tier).
            source: Arc::new(Mutex::new(Box::new(NativeSource::from_kind(
                modes::ModeKind::Ft8,
            )))),
            decode_epoch: 0,
            source_kind: SourceKind::Native,
            mode: Mode::Chat,
            // Transmit DISARMED at launch — WSJT-X's "Enable Tx" latch, which is off
            // until the operator arms it. Passive monitor + beacon-off were not enough:
            // any path that leaves a pending message in the sequencer (a CQ-run state, a
            // restored/forced call) would key immediately if TX were pre-armed. With
            // this off, the rig can NEVER transmit on launch; Call CQ / double-click a
            // station / arming Monitor all set it true explicitly. (Fixes the reported
            // unsolicited call on open.)
            tx_enabled: false,
            skip_tx1: false,
            tuning: false,
            tx_watchdog: false,
            tx_watchdog_start: None,
            recent_dt: VecDeque::new(),
            seen_decode: false,
            rig_confirmed: false,
            last_dx_tier: None,
            last_msg_tier: None,
            work_tick: 0,
            work_view: None,
            work_call: None,
            qso_report_sent: None,
            pending_log: None,
            qso_logged: false,
            qso_start_unix: None,
            cq_running: false,
            // Seeded from the callsign so two stations do not share a draw
            // sequence; the percentage is applied from settings on each decision.
            beacon_sched: tempo_core::beacon::BeaconScheduler::new(0, 0x5EED),
            beacon_decided_slot: None,
            beacon_tx_this_slot: false,
            chat_cq: false,
            chat_cq_paused: false,
            chat_cq_last_directed: 0,
            chat_cq_unanswered: 0,
            chat_yield_pending: false,
            chat_yield_slots: 0,
            last_decode_slot: None,
            immediate_tx: false,
            immediate_retune: false,
            route_intent: None,
            decode_history: std::collections::VecDeque::new(),
            early_seen: None,
            pending_tx_disable: false,
            pending_cw_id: false,
            highlights: std::collections::HashMap::new(),
            highlight_seq: 0,
            clear_tick: 0,
            pending_udp_clear: None,
            cq_dir: None,
            last_wire_decodes: Vec::new(),
            tx_dial_shift_hz: 0,
            split_tx_mhz: None,
            split_dirty: false,
            rit_hz: 0,
            xit_hz: 0,
            active_vfo_b: false,
            rit_dirty: false,
            xit_dirty: false,
            vfo_dirty: false,
            sideband_override: None,
            cw_queue: VecDeque::new(),
            cw_sent: VecDeque::new(),
            cw_keyer_error: None,
            cw_peer_call: String::new(),
            cw_peer_name: String::new(),
            cw_peer_state: String::new(),
            cw_abort: false,
            manual_ptt: false,
            rf_power: None,
            rig_rf_power: None,
            mic_gain: None,
            rig_mic_gain: None,
            nr_level: None,
            rig_nr_level: None,
            agc: None,
            rig_agc: None,
            rig_smeter_db: None,
            rig_tx_swr: None,
            rig_tx_alc: None,
            rig_tx_po_w: None,
            rig_tx_comp_db: None,
            rig_mode: None,
            rig_funcs: [None; 5],
            rig_rx_ranges: None,
            rig_refused_dial_mhz: None,
            pending_func: [None; 5],
            rig_passband: None,
            pending_passband: None,
            pending_scope_span: None,
            pending_scope_ref: None,
            flex_pan_span_hz: 200_000.0,
            flex_pan_ref_dbm: None,
            pending_scope_fixed: None,
            broker_ptt: false,
            voice_tx: None,
            voice_abort: false,
            recording: false,
            record_buf: Vec::new(),
            qso_recording: false,
            qso_record_path: None,
            cw_audio: Vec::new(),
            ai_cw_audio: Vec::new(),
            ai_cw_text: String::new(),
            ai_cw_fed: 0,
            ai_cw_status: String::new(),
            cw_stream: tempo_core::cw_decode::CwStreamDecoder::new(tempo_fast::SAMPLE_RATE, 600.0),
            qso_audio: Vec::new(),
            cat_status: (None, String::new()),
            radio_live: std::collections::HashMap::new(),
            cat_reprobe: false,
            audio_error: None,
            qsy,
            rtty_armed: false,
            rtty_audio: Vec::new(),
            rtty_chars: VecDeque::new(),
            rtty_afc_hz: 0.0,
            rtty_afc_locked: false,
            rtty_afc_reset: false,
            rtty_center: None,
            rtty_queue: VecDeque::new(),
            rtty_abort: false,
            rtty_sending: false,
            rtty_keyer_error: None,
            rtty_seq: None,
            rtty_auto_over: false,
            aprs_arm: AprsArm::Off,
            aprs_stations: std::collections::HashMap::new(),
            aprs_uplink_queue: VecDeque::new(),
            aprs_is_status: AprsIsStatus::default(),
            aprs_auto_arm_declined: false,
            aprs_audio: Vec::new(),
            aprs_heard: Vec::new(),
            aprs_health: AprsHealth::default(),
            aprs_tx_queue: VecDeque::new(),
            aprs_msg_seq: 0,
            aprs_fm: false,
            fm_channel: false,
            sstv_armed: false,
            sstv_audio: Vec::new(),
            sstv_progress: None,
            sstv_tx: None,
            sstv_abort: false,
            sstv_sending: false,
            sstv_tx_mode: None,
            sstv_tx_progress: None,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The [`ModeKind`] a tier resolves to under the CURRENT settings.
    ///
    /// Q65 is the first tier whose mode is not determined by the tier alone — the
    /// operator's period and submode selection sets the frame length and the slot
    /// clock. Every caller must go through here rather than `Tier::mode_kind`
    /// directly, or a Q65 buffer gets sized for the wrong period.
    fn tier_mode_kind(&self, tier: Tier) -> Option<modes::ModeKind> {
        tier.mode_kind(
            self.settings.q65_period_s,
            self.settings.q65_submode,
            self.settings.fst4_period_s,
            self.settings.msk144_period_s,
            self.settings.jt65_submode,
        )
    }

    /// True when `tier` decodes but cannot transmit under the current settings.
    ///
    /// ⭐ `None` from [`Self::tier_mode_kind`] is NOT a refusal. TempoDeep (DX1)
    /// has no `ModeKind` — it transmits through FT1's own path — so treating an
    /// absent kind as receive-only would silence DX1. Only a mode that explicitly
    /// declares `tx: false` is receive-only.
    fn tier_is_rx_only(&self, tier: Tier) -> bool {
        self.tier_mode_kind(tier)
            .is_some_and(|k| modes::tx_mode(k).is_none())
    }

    /// True when `tier` is a BEACON — it transmits on a schedule and has no QSO
    /// sequence (WSPR, FST4W).
    ///
    /// Separate from [`Self::tier_is_rx_only`] and both are needed: a beacon CAN
    /// transmit, so the rx-only guards correctly let it through, but there is no
    /// exchange for the auto-sequencer to run. Without this the sequencer would arm
    /// a CQ run on WSPR and try to work the beacons it hears, which is meaningless.
    fn tier_is_beacon(&self, tier: Tier) -> bool {
        self.tier_mode_kind(tier)
            .is_some_and(|k| modes::make_mode(k).capabilities().beacon_only)
    }

    /// How many unanswered CQs before a run stops on its own, or `None` for stock
    /// WSJT-X behaviour (call until the operator or the watchdog stops it).
    ///
    /// ⭐ MSK144 RUNS UNCAPPED. WSJT-X has no per-call CQ budget in any mode — the
    /// wall-clock TX watchdog is the only bound — and on meteor scatter that
    /// matters: a CQ is expected to run for many minutes before a trail arrives.
    /// Our default of 10 calls would stop a run after ~5 minutes and read as the
    /// mode being broken. The watchdog still bounds it, as upstream intends.
    ///
    /// ⚠️ On MSK144 this OVERRIDES the operator's `cq_max_calls` rather than
    /// deferring to it. `Option<u32>` cannot distinguish "the operator chose 10"
    /// from "10 is the default", so honouring the value would silently reimpose the
    /// FT8 budget on every station that never touched the setting — the common
    /// case, and the one that reads as the mode being broken. Stopping a run is
    /// still one click, and the TX watchdog still bounds it.
    fn cq_call_budget(&self) -> Option<u32> {
        match self.app.tier() {
            Tier::Msk144 => None,
            _ => self.settings.cq_max_calls,
        }
    }

    /// How many unanswered overs to give an in-QSO step before abandoning it and
    /// returning to CQ. Mode-aware, because "the partner has gone" means completely
    /// different things on different modes.
    ///
    /// ⭐ MSK144 IS THE EXCEPTION, and the default was badly wrong for it. On
    /// meteor scatter, SILENCE IS THE NORMAL STATE: you transmit continuously and
    /// wait for a trail that may be a minute or more away. Our default of 3 overs
    /// is ~45 s at a 15 s period, which abandons live contacts.
    ///
    /// WSJT-X's own patience for MSK144 is the Wait-and-Reply timer,
    /// `stopWRTimer.start(int(12000.0*m_TRperiod))` (mainwindow.cpp:3031) — twelve
    /// T/R periods of transmitting, which is SIX own-transmit periods since the
    /// station keys every other one. That is the number matched here.
    ///
    /// The operator's explicit `cq_stall_overs` always wins; this only supplies the
    /// default, and `Some(0)` still disables abandonment entirely (stock WSJT-X
    /// behaviour: wait for the operator).
    fn qso_patience_overs(&self) -> u32 {
        if let Some(explicit) = self.settings.cq_stall_overs {
            return explicit;
        }
        match self.app.tier() {
            // 12 T/R periods of WSJT-X patience = 6 of our own overs.
            Tier::Msk144 => 6,
            _ => 3,
        }
    }

    /// `Err(reason)` when the ACTIVE tier has no QSO sequence to run — a beacon.
    /// Checked by the entry points that start or advance an EXCHANGE, alongside
    /// [`Self::require_tx_capable`], which asks the different question of whether
    /// the tier can key at all.
    fn require_qso_capable(&self) -> Result<(), String> {
        let tier = self.app.tier();
        if self.tier_is_beacon(tier) {
            return Err(format!(
                "{} is a beacon — it transmits your callsign, grid and power on a \
                 schedule, and has no QSO sequence. Set the transmit percentage in \
                 Settings ▸ Modes, or switch to FT8, FT4, Q65 or FST4 to work stations.",
                tier.label()
            ));
        }
        Ok(())
    }

    /// `Err(reason)` when the ACTIVE tier cannot transmit — the precondition every
    /// operator-initiated TX entry point checks.
    ///
    /// ⭐ WHY THIS EXISTS AT THE ENTRY POINTS AND NOT ONLY AT THE KEY.
    /// `modes::tx_mode()` already refuses in the wave builder, so a receive-only
    /// tier never keyed the radio — the safety property held. But the refusal
    /// happened ~800 lines downstream of the click, silently: Call CQ on MSK144
    /// armed the sequencer, enabled TX, queued a CQ and reported it as sent, and
    /// only the waveform quietly failed to exist. The operator watched a CQ run
    /// that transmitted nothing and had no way to tell from the UI. A guard that
    /// fires too late to inform is not enough; refuse where the operator asked.
    fn require_tx_capable(&self) -> Result<(), String> {
        let tier = self.app.tier();
        if self.tier_is_rx_only(tier) {
            return Err(format!(
                "{} is receive-only — Nexus decodes it but does not transmit it. \
                 Switch to FT8, FT4 or a Tempo tier to call.",
                tier.label()
            ));
        }
        Ok(())
    }

    /// Apply new settings. A change of callsign/grid rebinds identity IN PLACE
    /// (preserving roster + conversations + the `*` band feed — see
    /// [`AppState::set_identity`]); band/dial/Field-Day fields update in place. The
    /// operating mode returns to Chat.
    pub fn apply_settings(&mut self, s: Settings) {
        // What the active tier resolves to BEFORE the save — the per-mode period /
        // submode fields live in `Settings`, so this can change under us. See the
        // decoder rebuild at the end of this function.
        let kind_before = self.tier_mode_kind(self.tier());
        // Rebind identity IN PLACE (not a fresh AppState) so editing the callsign or
        // grid in Settings — or a GPS/UDP grid update routed through a save — does
        // NOT wipe conversation history, the `*` band feed, or the roster.
        if s.mycall != self.settings.mycall || s.mygrid != self.settings.mygrid {
            self.app.set_identity(&s.mycall, &s.mygrid);
        }
        // The signal source is owned by `set_source` (which binds the socket), not
        // the settings form — preserve the live choice so a stale form save can't
        // silently flip it.
        let live_source = self.source_kind;
        // The whole dual-radio roster (radios list + which radio is active + peg-lock + the active
        // radio's live tune) is LIVE operating state, owned by the dedicated verbs (`add_radio`/
        // `remove_radio`/`rename_radio`/`set_radio_bands`/`set_active_radio`/`set_radio_pegged`), NOT
        // the settings form — exactly like `source`. Capture it BEFORE the move so a Save from a form
        // loaded before a live roster/switch change can't revert it (drop a just-added radio, yank the
        // operator off the radio they switched to, or fold the flat CAT into the wrong profile).
        // `form_active` is the radio the flat rig form was editing — captured before
        // `ensure_radio_profiles` can remap a stale id (else the "removed radio" guard is dead code).
        let live_source_radios = std::mem::take(&mut self.settings.radios);
        let live_active = self.settings.active_radio;
        let live_pegged = self.settings.radio_pegged;
        // The routing rules + default radio are part of that same live roster state (they key on
        // `RadioProfile::id`), owned by `set_routing_rules`/`set_default_radio`. They MUST be
        // captured here too: while the operator is editing a NON-ACTIVE radio the panel saves via
        // `update_radio_profile` and never sends the form at all, so a later plain Save carries a
        // form loaded before the rules were written — and would silently revert them.
        let live_rules = std::mem::take(&mut self.settings.routing_rules);
        let live_default_radio = self.settings.default_radio;
        let (live_dial, live_band, live_sideband) = (
            self.settings.dial_mhz,
            self.settings.band.clone(),
            self.settings.sideband.clone(),
        );
        // Which radio the incoming flat fields describe. A P2-aware Settings form carries the roster
        // + its edited radio in `active_radio`. A LEGACY payload with no `radios` (an old settings.json
        // or a pre-P2 saved config profile) describes the LIVE active radio — fold its flat CAT there,
        // NOT into id 0, or loading a single-radio profile would clobber whatever radio is active.
        let form_active = if s.radios.is_empty() {
            live_active
        } else {
            s.active_radio
        };
        self.settings = s;
        // Implicit-ACK toggle lives app-side (the observe loop consumes it).
        self.app.set_implicit_ack(self.settings.chat_implicit_ack);
        self.settings.source = live_source;
        self.settings.radios = live_source_radios;
        self.settings.radio_pegged = live_pegged;
        self.settings.routing_rules = live_rules;
        self.settings.default_radio = live_default_radio;
        self.settings.ensure_radio_profiles();
        // Fold the form's flat rig/audio edits into the profile the FORM was editing — the flat fields
        // describe the radio SHOWN in the form, which may differ from the live active radio if a
        // switch happened after the form loaded. If that radio was removed live, drop its stale edits.
        if self.settings.radios.iter().any(|p| p.id == form_active) {
            self.settings.active_radio = form_active;
            self.settings.sync_active_from_flat();
            // The form's tune describes form_active too — persist it as that radio's last tune.
            if let Some(p) = self
                .settings
                .radios
                .iter_mut()
                .find(|p| p.id == form_active)
            {
                p.last_dial_mhz = self.settings.dial_mhz;
                p.last_band = self.settings.band.clone();
                p.last_sideband = self.settings.sideband.clone();
            }
        }
        // Restore the LIVE active radio and re-establish the invariant the service loop relies on:
        // the flat CAT/audio mirror + the tune MUST match the active radio (else it would command the
        // wrong hardware on the next `Transport::from_settings` rebuild). Common case (form edited the
        // live radio) keeps the form's tune; a diverged switch pins the mirror + tune back to the live
        // rig — the form's tune already went to form_active's profile above.
        self.settings.active_radio = live_active;
        self.settings.ensure_radio_profiles();
        // Two live rigctld daemons need distinct ports — de-conflict on EVERY save, not just at load,
        // else an in-session config (e.g. a flat-form port edit, or loading a pre-P2 profile) can
        // leave two radios sharing 4532; their monitors then cross-connect and command the wrong rig.
        self.settings.ensure_distinct_radio_ports();
        // A save can remove a radio (a legacy payload's roster is discarded, but `ensure_radio_profiles`
        // can still remap ids) — re-check that nothing routes at a radio that isn't there.
        self.settings.ensure_routing_targets();
        // `ensure_distinct_radio_ports` may have just BUMPED the active radio's profile rigctld/rotctld
        // port to de-conflict it. Re-pin the flat CAT/audio mirror to the active profile NOW, so the
        // active-radio loop (which reads the flat mirror via `Transport::from_settings`) and the
        // monitors (which read each profile via `Transport::from_profile`) agree on the daemon port.
        // Skipping this in the common (form==live) branch WAS the dual-radio CAT bug: a bumped port
        // lived only in the profile while the flat mirror kept the old, colliding port — so the active
        // rig and a monitor both bound the same port and the monitor's rigctld died. That is the
        // dead-CAT symptom that flipped to whichever radio was the monitor. `sync_flat_from_active`
        // copies rig/audio/rotator fields only (NOT dial/band/sideband), so the tune selection below is
        // unaffected.
        self.settings.sync_flat_from_active();
        let (app_dial, app_band, app_sideband) = if form_active == live_active {
            (
                self.settings.dial_mhz,
                self.settings.band.clone(),
                self.settings.sideband.clone(),
            )
        } else {
            self.settings.dial_mhz = live_dial;
            self.settings.band = live_band.clone();
            self.settings.sideband = live_sideband.clone();
            (live_dial, live_band, live_sideband)
        };
        self.app.set_radio(app_dial, &app_band, &app_sideband);
        // Re-derive the live timing/tuning state from the saved settings.
        self.tx_parity = if self.settings.tx_even { 0 } else { 1 };
        self.tx_offset_hz = self.settings.tx_offset_hz;
        self.rx_offset_hz = self.settings.rx_offset_hz;
        self.hold_tx_freq = self.settings.hold_tx_freq;
        // A settings save reconciles the operating mode with the Field Day
        // master switch `fd_active`, which is authoritative over whether the
        // engine operates in Field Day (spec §1). apply_settings is heavyweight
        // — it resets the mode to Chat and clears the TX queue — so this is the
        // one place a save re-enters (or leaves) FD to keep it in step with the
        // master.
        if self.settings.fd_active {
            // Master ON. PRESERVE an already-active FD session in place: the
            // Mode::FieldDay variant carries the whole dupe-checked contest log
            // in memory, and rebuilding it would drop everything logged since
            // the last journal flush (a solo entrant with no club logger has no
            // other live copy). The FD panel saves settings on every bonus-
            // checkbox toggle, so this preservation is load-bearing. If NOT yet
            // in FD, this save turned the master on (or followed a mode change):
            // reset the heavyweight state, then enter passive S&P so every
            // cockpit goes FD-aware — but only once class + section are set
            // (else `restore_field_day_if_enabled` leaves Chat so the UI can
            // prompt for them; the exchange goes on the air and `set_mode`
            // refuses a blank one).
            if !matches!(self.mode, Mode::FieldDay { .. }) {
                self.mode = Mode::Chat;
                self.cq_running = false;
                self.restore_field_day_if_enabled();
            }
        } else {
            // Master OFF. Field Day must be fully EXITED — reset to Chat
            // regardless of the current mode. This closes the gap (spec §1.3)
            // where flipping the master off left a lingering Mode::FieldDay, so
            // the operator was stranded in FD with the nav hidden. The durable
            // journal (written per contact) restores the log on the next
            // re-enable. Every non-FD mode is likewise safe to reset — Chat
            // holds nothing, a QSO is transient.
            self.mode = Mode::Chat;
            // Clear the CQ-run flag alongside the mode reset — otherwise a save
            // that drops out of a QSO run leaves `cq_running` stale-true, which
            // suppresses the smart auto-cycle on the next chat answer.
            self.cq_running = false;
        }
        // A save carries a `band` field; now that the Field Day log survives a
        // save (above), keep its frozen band in step with the saved band —
        // otherwise a QSY that reaches settings.band via a save (e.g. the FD
        // panel resending a stale-then-updated struct, or a knob QSY followed by
        // a bonus toggle) would leave log.band diverged, stamping later contacts
        // under the wrong band and busting dupe keys. No-op outside Field Day.
        self.sync_fd_band();
        self.tx_queue.clear();
        self.broadcast_queue.clear();
        // Reconcile the (separate) coordinated-QSY feature with the saved flags.
        self.qsy
            .configure(self.settings.qsy_set.clone(), self.settings.qsy_cadence);
        // ⭐ REBUILD THE DECODER when the save changed what the ACTIVE tier resolves
        // to. Q65, FST4/FST4W, MSK144 and JT65 take their period/submode from
        // Settings, so the same tier maps to different `ModeKind`s across a save —
        // and the boxed decoder was only ever rebuilt by `set_tier`/`set_source`.
        // Changing Q65 30 s → 60 s while sitting on Q65 therefore changed nothing
        // in the decode path (the Settings hint promises it takes effect on the
        // next slot), while `active_slot_secs`, `tx_over_secs` and the WSJT-X UDP
        // status all switched to the new period immediately — so the app disagreed
        // with itself about the running period until the operator toggled tiers.
        // A submode-only change had no visible symptom at all: decodes just stopped
        // appearing, on modes where that setting is the entire point.
        //
        // Compared BY KIND so an unrelated save is a no-op: rebuilding drops the
        // decoder's accumulated state, which for Q65/JT65 message averaging is real
        // progress to lose. Companion mode owns its own source — leave it alone.
        if self.source_kind == SourceKind::Native && self.tier_mode_kind(self.tier()) != kind_before
        {
            if let Some(kind) = self.tier_mode_kind(self.tier()) {
                // Swap UNDER the lock (waits out any decode in flight) and clear the
                // context, exactly as `set_tier` does — same reasons. The epoch bump
                // inside is the load-bearing part: a decode already dispatched at the
                // OLD period would otherwise land after the swap and be folded in
                // with slot indices that no longer mean anything.
                *self.source.lock().unwrap() = Box::new(NativeSource::from_kind(kind));
                self.clear_decode_context();
            }
        }
        if self.settings.qsy_enabled && !self.qsy.enabled {
            let home = self.qsy_token_for_current();
            let partner = self.app.active_peer().map(|s| s.to_string());
            self.qsy.enable(home, partner);
        } else if !self.settings.qsy_enabled && self.qsy.enabled {
            let _ = self.qsy.disable();
        }
    }

    /// Re-enter Field Day if the persisted master switch (`fd_active`) left it
    /// on. Called by the shell at startup (AFTER [`set_fd_log_path`](Self::set_fd_log_path),
    /// so the durable journal restores the contest log) and by
    /// [`apply_settings`](Self::apply_settings) when a save turns the master on.
    /// Enters passive S&P (`fieldday-sp`) so every cockpit goes FD-aware and the
    /// journal is merged in. This is the ONLY code path that auto-enters FD, and
    /// only because the operator left the master on — no date/default logic ever
    /// sets `fd_active` (spec §1.1). No-op unless the master is on with a
    /// non-blank class + section (the exchange goes on the air; `set_mode`
    /// refuses a blank one) and the engine is not already in FD (never rebuild a
    /// live in-memory FD log).
    pub fn restore_field_day_if_enabled(&mut self) {
        if self.settings.fd_active
            && !self.settings.fd_class.trim().is_empty()
            && !self.settings.fd_section.trim().is_empty()
            && !matches!(self.mode, Mode::FieldDay { .. })
        {
            let _ = self.set_mode("fieldday-sp");
        }
    }

    /// Advance the persisted LoTW incremental-sync cursor (`lotw_last_qsl`) WITHOUT
    /// the heavyweight side effects of [`Engine::apply_settings`] (no mode reset,
    /// no TX-queue clear, no QSY reconcile). Returns the updated [`Settings`] so the
    /// caller can persist them. The LoTW confirmation download must be invisible to
    /// live operation — a mid-QSO sync must not kick the operator back to Chat or
    /// drop queued TX.
    pub fn set_lotw_cursor(&mut self, high_water: String) -> Settings {
        self.settings.lotw_last_qsl = high_water;
        self.settings.clone()
    }

    /// Advance the persisted eQSL incremental-sync cursor (`eqsl_last_sync`) WITHOUT
    /// the side effects of [`Engine::apply_settings`] (see [`Engine::set_lotw_cursor`]).
    /// Returns the updated [`Settings`] for the caller to persist.
    pub fn set_eqsl_cursor(&mut self, cursor: String) -> Settings {
        self.settings.eqsl_last_sync = cursor;
        self.settings.clone()
    }

    /// Flip one connector auto-upload toggle (`Some(on)`) WITHOUT the side
    /// effects of [`Engine::apply_settings`] — saving a credential mid-QSO must
    /// not reset the operating mode or drop queued TX. `None` leaves a toggle
    /// unchanged. Returns the updated [`Settings`] for the caller to persist.
    pub fn set_upload_toggles(
        &mut self,
        qrz: Option<bool>,
        clublog: Option<bool>,
        eqsl: Option<bool>,
    ) -> Settings {
        if let Some(v) = qrz {
            self.settings.qrz_logbook_upload = v;
        }
        if let Some(v) = clublog {
            self.settings.clublog_upload = v;
        }
        if let Some(v) = eqsl {
            self.settings.eqsl_upload = v;
        }
        self.settings.clone()
    }

    /// Flip the HRDLog.net auto-upload toggle (persisted by the caller). Kept
    /// separate from [`set_upload_toggles`](Self::set_upload_toggles) so the three
    /// existing callers stay untouched; same rationale — a credential save must not
    /// reset the operating mode or drop queued TX (never `apply_settings`).
    pub fn set_hrdlog_upload(&mut self, on: bool) -> Settings {
        self.settings.hrdlog_upload = on;
        self.settings.clone()
    }

    /// Change band / dial frequency / mode **live** — without resetting the
    /// operating mode or queues (unlike [`Engine::apply_settings`]). Updates the
    /// settings + the UI radio readout; the radio loop re-tunes the rig from
    /// settings each slot. `mode` is "USB" (weak-signal) or "FM" (simplex).
    /// Keep an active Field Day log's band in step with the dial — it was
    /// frozen at mode-entry, so a mid-event QSY logged every later contact
    /// under the ENTRY band (wrong Cabrillo/N3FJP band, corrupted dupe keys).
    fn sync_fd_band(&mut self) {
        let band = self.settings.band.clone();
        if let Mode::FieldDay { station, .. } = &mut self.mode {
            station.log.band = band;
        }
    }

    /// Switch the ACTIVE radio (dual-radio). Persists the current radio's live tune into its
    /// profile, flips the active id, mirrors the new profile's CAT/audio into the flat fields (so
    /// the radio loop swaps to it on the next tick), and restores the new radio's last band/freq.
    /// The LIGHT path — deliberately does NOT touch Mode / TX queues / identity (unlike
    /// `apply_settings`), so swinging radios mid-session never resets the operator to Chat. No-op if
    /// `id` isn't a configured radio or is already active.
    pub fn set_active_radio(&mut self, id: u32) {
        if id == self.settings.active_radio || !self.settings.radios.iter().any(|p| p.id == id) {
            return;
        }
        // Switching rigs leaves the APRS FM-simplex context (the new radio has its own band/mode).
        // The band-gate in `rig_mode_effective` already prevents an FM leak onto a non-2 m band;
        // this clears the flag outright so it never lingers pointed at a different radio.
        self.aprs_fm = false;
        self.fm_channel = false;
        // Frequency coverage belongs to the RADIO, so it must not survive the switch. Dropping it to
        // "unknown" makes the window before the loop re-probes fail OPEN; keeping the old radio's
        // list would let an HF-only rig's ranges block a QSY on the VHF radio that just became
        // active — a fail-CLOSED bug, and the inverse of what the gate is for.
        self.rig_rx_ranges = None;
        self.rig_refused_dial_mhz = None;
        // Fold the OUTGOING radio's live flat CAT/audio edits into its own profile BEFORE we mirror
        // the new radio in — otherwise an unsaved flat change made while this radio was active (e.g. a
        // live Pwr/tx_level tweak) is discarded by `sync_flat_from_active` below. `active_radio` still
        // names the outgoing radio here, so this folds into the right profile.
        self.settings.sync_active_from_flat();
        // Defense-in-depth: folding the outgoing radio's flat mirror into its profile could carry a
        // colliding rigctld/rotctld port; de-conflict immediately so a switch can never leave two
        // radios sharing one daemon port (the flat mirror is re-pinned from the new active below).
        self.settings.ensure_distinct_radio_ports();
        // Persist the current radio's live tune into its profile before we leave it.
        let cur = self.settings.active_radio;
        if let Some(p) = self.settings.radios.iter_mut().find(|p| p.id == cur) {
            p.last_dial_mhz = self.settings.dial_mhz;
            p.last_band = self.settings.band.clone();
            p.last_sideband = self.settings.sideband.clone();
        }
        // A radio swap is a hard context change (different antenna/band): stop TX + drop the decode
        // context + roster, exactly like a band QSY.
        self.halt_tx();
        self.clear_decode_context();
        self.app.clear_stations();
        // The a7 cross-cycle AP table holds the OLD radio's decodes — replaying
        // them as AP hypotheses on the new radio's band would seed wrong-call decodes.
        modes::reset_ft8_a7();
        self.sideband_override = None;
        // Flip active + mirror the new profile's CAT/audio into the flat fields — Transport::
        // from_settings then differs and the loop's existing swap tears down the old rig + opens
        // the new one (unkey-first).
        self.settings.active_radio = id;
        self.settings.sync_flat_from_active();
        // Adopt the new radio's tune. Prefer its LIVE monitored dial (dual-radio: the radio has been
        // connected the whole time, so use where it ACTUALLY is — the operator may have hand-tuned it),
        // else its persisted last tune, else the mirrored dial. `band` follows the chosen dial.
        let live = self.radio_live.get(&id).cloned();
        if let Some(l) = live.filter(|l| l.dial_mhz.is_some()) {
            let dial = l.dial_mhz.unwrap();
            let band = l
                .band
                .filter(|b| !b.is_empty())
                .or_else(|| crate::bandplan::band_for_dial(dial).map(str::to_string))
                .unwrap_or_else(|| self.settings.band.clone());
            let sb = l.sideband.unwrap_or_else(|| self.settings.sideband.clone());
            self.settings.dial_mhz = dial;
            self.settings.band = band.clone();
            self.settings.sideband = sb.clone();
            self.app.set_radio(dial, &band, &sb);
        } else if let Some(p) = self.settings.active_profile().cloned() {
            let (dial, band, sb) = if p.last_band.is_empty() {
                (
                    self.settings.dial_mhz,
                    self.settings.band.clone(),
                    self.settings.sideband.clone(),
                )
            } else {
                (p.last_dial_mhz, p.last_band, p.last_sideband)
            };
            self.settings.dial_mhz = dial;
            self.settings.band = band.clone();
            self.settings.sideband = sb.clone();
            self.app.set_radio(dial, &band, &sb);
        }
        self.immediate_retune = true;
        self.sync_fd_band();
    }

    /// Peg-lock: when on, band selection never auto-switches the active radio (P4). Light setter.
    pub fn set_radio_pegged(&mut self, on: bool) {
        self.settings.radio_pegged = on;
    }

    /// Add a new (2nd/3rd…) radio to the roster and SWITCH TO IT (returns its id). Switching makes the
    /// flat Rig/Audio form edit the NEW radio, so the operator configures the radio they just added —
    /// NOT the previously-active radio (which is how a config could get clobbered). The new radio has
    /// no model yet, so it comes up as VOX/no-CAT until configured.
    pub fn add_radio(&mut self) -> u32 {
        let id = self.settings.add_radio_profile();
        self.set_active_radio(id);
        id
    }

    /// Remove a radio from the roster (no-op on the active or last radio). Pure roster edit.
    pub fn remove_radio(&mut self, id: u32) -> bool {
        self.settings.remove_radio_profile(id)
    }

    /// Rename a radio profile (operator-facing name / switcher label). Pure roster edit.
    pub fn rename_radio(&mut self, id: u32, name: &str) {
        if let Some(p) = self.settings.radios.iter_mut().find(|p| p.id == id) {
            p.name = name.trim().to_string();
        }
    }

    /// Set a radio's band-coverage set (empty = covers everything) for auto-routing (P4). Pure edit.
    pub fn set_radio_bands(&mut self, id: u32, bands: Vec<String>) {
        if let Some(p) = self.settings.radios.iter_mut().find(|p| p.id == id) {
            p.bands = bands;
        }
    }

    /// Replace the band+mode routing rules (first-match-wins order is the list order). Rules aimed
    /// at a radio that doesn't exist are dropped — a rule that can never fire is worse than no rule.
    /// Live verb, NOT part of the settings form: like the roster it must survive a stale-form Save.
    pub fn set_routing_rules(&mut self, rules: Vec<crate::settings::RoutingRule>) {
        self.settings.routing_rules = rules;
        self.settings.ensure_routing_targets();
    }

    /// Set (or clear) the fallback radio for band+mode combinations no rule and no band coverage
    /// claims. `None` = stay on the active radio. Live verb, same reason as `set_routing_rules`.
    pub fn set_default_radio(&mut self, id: Option<u32>) {
        self.settings.default_radio = id;
        self.settings.ensure_routing_targets();
    }

    /// Edit ONE radio's CAT/audio/PTT/rotator/native config IN PLACE — WITHOUT changing which radio
    /// is active. This is what lets the operator configure Radio 2 while operating on Radio 1: no
    /// live rig swap, no dropped carrier. If `id` happens to BE the active radio, the flat mirror is
    /// re-synced from it so the running loop picks the edits up; if it isn't, the active radio (and
    /// its flat mirror) are left completely untouched. `active_radio` is never changed here.
    pub fn update_radio_profile(&mut self, id: u32, patch: crate::settings::RadioProfilePatch) {
        if let Some(p) = self.settings.radios.iter_mut().find(|p| p.id == id) {
            patch.apply_to(p);
        }
        // Keep each radio's daemon ports distinct (a bumped port must not collide) and mirror the
        // ACTIVE profile back into the flat fields — a no-op for the flat mirror when id != active.
        self.settings.ensure_distinct_radio_ports();
        self.settings.sync_flat_from_active();
    }

    pub fn set_frequency(&mut self, dial_mhz: f64, band: &str, mode: &str) {
        // A fresh QSY request is a fresh attempt: drop any earlier refusal so a stale "the radio
        // refused 144.390" can't sit on screen next to a frequency that worked.
        self.rig_refused_dial_mhz = None;
        // Auto radio routing on (BAND, MODE CLASS): a commanded band pick (dropdown / manual entry /
        // Needed click / APRS tune) hands off to the radio the operator mapped for that band+mode
        // FIRST, then the tune below lands it on the requested dial. Mode class — not band alone —
        // is what lets ONE band go to two rigs: 2 m FT8 to the IC-9700, 2 m FM/APRS to the FT-991A.
        // Falls back to band-only coverage, then the default radio (see `Settings::route_radio`).
        // Peg-lock pins the active radio (no auto-switch). No-op for a single radio, or when the
        // routing decision already names the active one.
        //
        // The class is read BEFORE `aprs_fm` is cleared below, and `route_intent` lets a caller
        // whose intent isn't in state yet name it explicitly (aprs_tune).
        let route_mode = self
            .route_intent
            .take()
            .unwrap_or_else(|| self.route_mode(band, dial_mhz));
        if !self.settings.radio_pegged {
            if let Some(id) = self.settings.route_radio(band, route_mode) {
                self.set_active_radio(id);
            }
        }
        // A normal QSY leaves the APRS FM-simplex context (aprs_tune re-sets it right after its own
        // set_frequency call), so FM never lingers onto the next band the operator tunes. Same for
        // the FM-channel hold (repeater_tune re-sets it the same way).
        self.aprs_fm = false;
        self.fm_channel = false;
        // Band change invalidates the decode context: answering a HISTORY row from
        // the old band would target a station that isn't here and derive parity
        // from the old band's slots. The heard-stations roster goes with it —
        // those stations aren't on the new band (operator report: stale roster
        // entries lingered across QSY). (In-band QSY keeps both — same activity.)
        if !self.settings.band.eq_ignore_ascii_case(band) {
            self.clear_decode_context();
            self.app.clear_stations();
            // The a7 cross-cycle AP table holds the OLD band's decodes — replaying
            // them as AP hypotheses on the new band would seed wrong-call decodes.
            modes::reset_ft8_a7();
            // Band switch mid-QSO: without a halt the sequencer keeps calling a
            // station that isn't on the new band (operator report — directed
            // calls kept going out after a Needed-click QSY). Same semantics as
            // the Halt Tx button; working a new spot re-arms AFTER this, so the
            // click-a-needed → QSY → call flow is unaffected.
            self.halt_tx();
        }
        // A band change drops the transient mode override, so a QSY re-asserts the auto sideband
        // (LSB <10 MHz / USB above) — "manual mode, but don't impede band auto-select".
        if !self.settings.band.eq_ignore_ascii_case(band) {
            self.sideband_override = None;
        }
        // A QSY — band change OR a same-band dial move (band picker, MHz entry, or
        // working a needed spot, which funnels through here) — lands on a different
        // signal, so the CW copy from the old frequency is stale. Clear it.
        if !self.settings.band.eq_ignore_ascii_case(band)
            || (self.settings.dial_mhz - dial_mhz).abs() > 1e-9
        {
            self.clear_cw_decode();
        }
        self.settings.dial_mhz = dial_mhz;
        self.settings.band = band.to_string();
        self.settings.sideband = mode.to_string();
        self.app.set_radio(dial_mhz, band, mode);
        // Operator QSY → the radio loop must follow on the very next iteration, not
        // when it happens to notice the dial changed. (Single-click precision.)
        self.immediate_retune = true;
        // A plain QSY always returns the rig to SIMPLEX — leftover split from a
        // pile-up must never silently shift TX on the next frequency.
        if self.split_tx_mhz.take().is_some() {
            self.split_dirty = true;
        }
        self.sync_fd_band();
    }

    /// The rig reported a dial frequency we did NOT set — the operator turned the VFO knob
    /// (or another app moved it over the CAT broker). Adopt it as the live dial so the UI
    /// mirrors the knob. Updates the band from the frequency; leaves the operating mode +
    /// sideband to the rig-mode policy (the radio loop still owns what it commands).
    pub fn observe_rig_freq(&mut self, hz: u64) {
        let mhz = hz as f64 / 1_000_000.0;
        // A knob QSY returns the rig to simplex, exactly like an app-commanded retune
        // (set_frequency): otherwise a manual split's TX VFO would be left on the OLD dial —
        // transmitting off-frequency — and the SPLIT badge would lie about the offset.
        if self.split_tx_mhz.take().is_some() {
            self.split_dirty = true;
        }
        self.settings.dial_mhz = mhz;
        if let Some(band) = crate::bandplan::band_for_dial(mhz) {
            // Knob QSY across bands invalidates the decode context + roster too —
            // and halts TX for the same reason as set_frequency: the sequencer
            // must never keep calling across a band switch, however it happened.
            if !self.settings.band.eq_ignore_ascii_case(band) {
                self.clear_decode_context();
                self.app.clear_stations();
                // The a7 cross-cycle AP table holds the OLD band's decodes — replaying
                // them as AP hypotheses on the new band would seed wrong-call decodes.
                modes::reset_ft8_a7();
                self.halt_tx();
                self.clear_cw_decode(); // stale CW copy across a cross-band knob QSY
                                        // A knob QSY across bands drops the transient mode override too, exactly like an
                                        // app-commanded band change (set_frequency) — so the tooltip's "until you change
                                        // bands" holds however the QSY happened, and the auto sideband re-asserts.
                self.sideband_override = None;
            }
            self.settings.band = band.to_string();
        }
        self.app.set_radio(
            self.settings.dial_mhz,
            &self.settings.band,
            &self.settings.sideband,
        );
        self.sync_fd_band();
    }

    /// Seed the dial from the rig's OWN frequency at CAT open (read-only launch): update
    /// the app's belief — dial, derived band, snapshot radio state — and nothing else.
    ///
    /// Deliberately NOT `observe_rig_freq`: that models an operator knob-QSY and, on a
    /// band delta, clears the decode context/roster, resets the a7 table, halts TX and
    /// drops the sideband override. All no-ops on a fresh engine *today* — but a boot
    /// seed that depends on that coincidence breaks the day launch order changes. A seed
    /// must be provably side-effect-free.
    pub fn seed_rig_dial(&mut self, hz: u64) {
        let mhz = hz as f64 / 1_000_000.0;
        self.settings.dial_mhz = mhz;
        if let Some(band) = crate::bandplan::band_for_dial(mhz) {
            self.settings.band = band.to_string();
        }
        self.app.set_radio(
            self.settings.dial_mhz,
            &self.settings.band,
            &self.settings.sideband,
        );
        self.sync_fd_band();
    }

    /// A CAT read from the rig actually succeeded — the displayed dial/mode are the
    /// RIG's values, not the persisted seed. Set only from `has_control() && read Ok`
    /// (NEVER from `cat_ok`: a serial-PTT rig sharing the CAT port reports
    /// `cat_ok == true` while being structurally unreadable). Cleared with the other
    /// rig state when the CAT breaker trips.
    pub fn set_rig_confirmed(&mut self, confirmed: bool) {
        self.rig_confirmed = confirmed;
    }

    // --- Dual-radio: per-radio live read-back from the monitor thread (NON-active radios only) ---
    // These update `radio_live[id]` WITHOUT touching the active flat state / decode context / TX — a
    // monitor read must never move the operator's cockpit or key anything. Callers MUST pass a
    // non-active radio id (the active radio uses `observe_rig_*`).

    /// Record a non-active radio's live dial (Hz) + derived band. No cockpit/decode side effects.
    pub fn observe_radio_freq(&mut self, id: u32, hz: u64) {
        let mhz = hz as f64 / 1_000_000.0;
        let e = self.radio_live.entry(id).or_default();
        e.dial_mhz = Some(mhz);
        e.band = crate::bandplan::band_for_dial(mhz).map(str::to_string);
    }

    /// Record a non-active radio's live mode (Hamlib name, e.g. "USB"/"FM").
    pub fn observe_radio_mode(&mut self, id: u32, mode: String) {
        self.radio_live.entry(id).or_default().sideband = Some(mode);
    }

    /// Record a non-active radio's live S-meter (dB rel S9).
    pub fn observe_radio_smeter(&mut self, id: u32, db: i32) {
        self.radio_live.entry(id).or_default().smeter_db = Some(db);
    }

    /// Record a non-active radio's live CAT health (`Some(true/false)`), for its switcher pill.
    pub fn observe_radio_cat(&mut self, id: u32, ok: Option<bool>) {
        self.radio_live.entry(id).or_default().cat_ok = ok;
    }

    /// Drop a radio's live cache (it left the monitor pool — became active, was removed, or
    /// disconnected). The snapshot then falls back to its profile `last_*`.
    pub fn forget_radio_live(&mut self, id: u32) {
        self.radio_live.remove(&id);
    }

    /// The active tier's band plan with the operator's working-frequency
    /// overrides applied (WSJT-X Settings ▸ Frequencies): an override replaces
    /// the dial of the matching (band, mode) row; a band the stock table lacks
    /// is appended. Empty overrides = stock.
    pub fn band_plan(&self) -> Vec<crate::bandplan::BandChannel> {
        let tier = self.app.tier();
        let mode_name = match tier {
            Tier::Ft8 => "FT8",
            Tier::Ft4 => "FT4",
            _ => "",
        };
        let mut plan = crate::bandplan::band_plan_for(tier);
        if !mode_name.is_empty() {
            for wf in &self.settings.working_frequencies {
                if !wf.mode.eq_ignore_ascii_case(mode_name) || wf.mhz <= 0.0 {
                    continue;
                }
                if let Some(c) = plan
                    .iter_mut()
                    .find(|c| c.band.eq_ignore_ascii_case(&wf.band))
                {
                    c.dial_mhz = wf.mhz;
                } else {
                    // A band the stock table lacks (e.g. 60 m FT4): append it —
                    // silently dropping a saved override is a lie to the operator.
                    plan.push(crate::bandplan::BandChannel {
                        band: wf.band.clone(),
                        group: if wf.mhz >= 30.0 { "VHF" } else { "HF" }.into(),
                        dial_mhz: wf.mhz,
                        mode: "USB".into(),
                        label: format!("{} · {} (custom)", wf.band, mode_name),
                        note: "operator working-frequency override".into(),
                    });
                }
            }
        }
        plan
    }

    /// The "home" dial + sideband for `om` on the CURRENT band: Digital → the active tier's
    /// watering hole (FT8 14.074 / FT4 14.080 / FT1 native); Phone → the lowest phone freq the
    /// operator is licensed for; CW → the lowest CW freq. `None` when the operator has no
    /// privilege for that band+mode (Tech on 20 m phone, Tech digital off 10 m, 60 m, an FM /
    /// no-FT8 band) — the caller then leaves the dial alone and lets the TX lockout guard the
    /// air. Every returned dial is one the operator can legally TX on: the emission passband
    /// is checked the same way `tx_allowed` checks it.
    fn mode_home(&self, om: crate::settings::OperatingMode) -> Option<(f64, String)> {
        use crate::settings::OperatingMode;
        // SSB occupies ~2.8 kHz beside the carrier; an FT8/FT4 signal sits at the audio offset
        // above the dial. Keep these in step with `tx_allowed`'s passband model.
        const SSB_BW: f64 = 0.0028;
        let class = self.settings.license_class;
        let band = self.settings.band.clone();
        match om {
            OperatingMode::Digital => {
                let off = self.tx_offset_hz as f64 / 1_000_000.0;
                self.band_plan()
                    .into_iter()
                    .find(|c| c.band == band)
                    // FT8/FT4 are USB, so the emission sits `off` ABOVE the dial. Only adopt the
                    // watering hole if the operator may actually run data there (Tech: 10 m only).
                    .filter(|c| crate::privileges::tx_allowed(class, c.dial_mhz + off, om))
                    .map(|c| (c.dial_mhz, c.mode))
            }
            OperatingMode::Phone => {
                crate::privileges::segment_start(class, &band, om).map(|lo| {
                    // On LSB (<10 MHz) the passband is [dial-2.8 kHz, dial], so parking the dial
                    // AT the segment edge would push the lower 2.8 kHz out of band → TX locked
                    // out at the very home freq. Lift the LSB home so the whole passband clears
                    // the edge. USB extends upward from `lo`, so the edge is already fine.
                    if lo < 10.0 {
                        (lo + SSB_BW, "LSB".to_string())
                    } else {
                        (lo, "USB".to_string())
                    }
                })
            }
            OperatingMode::Cw => crate::privileges::segment_start(class, &band, om)
                // Park on the CW ACTIVITY frequency (20 m → 14.030), NOT the dead band edge
                // (14.000, where nobody works and your signal would fall out of band),
                // clamped to the licensed CW-segment start so it never drops below privileges.
                // This mirrors the band-dropdown path (getLicensedBandPlan) so switching TO CW
                // mode and picking a band from the dropdown both land in the same place.
                .map(|lo| {
                    let dial = crate::bandplan::cw_activity_mhz(&band).map_or(lo, |a| a.max(lo));
                    // Sideband is inert for CW (the rig-mode policy commands CW or a band-aware
                    // tone), but keep it self-consistent with the band's convention.
                    (
                        dial,
                        if dial < 10.0 {
                            "LSB".to_string()
                        } else {
                            "USB".to_string()
                        },
                    )
                }),
            // RTTY: park on the band's RTTY watering hole (the built-in plan the cockpit's
            // band selector serves) — only if the operator may key the RTTY emission there.
            OperatingMode::Rtty => crate::bandplan::rtty_band_plan()
                .into_iter()
                .find(|c| c.band == band)
                .filter(|c| self.rtty_emission_ok(c.dial_mhz))
                .map(|c| (c.dial_mhz, c.mode)),
        }
    }

    /// May the operator's class key the RTTY EMISSION at `dial`? Judges the real RF span
    /// per keying backend (keep in step with [`Self::tx_allowed`]'s Rtty arm): AFSK rides
    /// LSB, so the mark/space audio pair (2125 / 2125+shift Hz) lands BELOW the dial;
    /// true FSK keys the rig's RTTY mode where the dial reads the mark RF and space sits
    /// `shift` below. Both edges of the span must be privileged.
    fn rtty_emission_ok(&self, dial: f64) -> bool {
        use crate::settings::OperatingMode;
        let class = self.settings.license_class;
        let shift = self.settings.rtty_shift_hz as f64 / 1_000_000.0;
        let allow = |f: f64| crate::privileges::tx_allowed(class, f, OperatingMode::Rtty);
        if self.settings.rtty_backend.eq_ignore_ascii_case("fsk") {
            allow(dial - shift) && allow(dial)
        } else {
            let mark = RTTY_AFSK_MARK_HZ / 1_000_000.0;
            allow(dial - mark - shift) && allow(dial - mark)
        }
    }

    /// Set the per-section operating mode (Digital / Phone / CW) — the rig-mode policy.
    /// Digital → DATA-U/DATA-L; Phone forces USB/LSB by band; CW forces CW. Always flags an
    /// immediate retune so the radio loop applies the mode on its very next iteration,
    /// clearing any prior `mode_giveup` so a click is never ignored.
    ///
    /// `follow_freq` is the "go to this mode" gesture: when the operator clicks an actual
    /// operating-section tab (Phone / CW / Digital), QSY to that mode's home frequency on the
    /// current band — Phone lands in the phone segment, CW drops to the CW segment, Digital
    /// snaps to the tier's watering hole (14.074 etc.). It's `false` for incidental nav (so
    /// glancing at the map/logbook never moves the VFO) and for the Needed click (which sets
    /// the spot's exact frequency itself — re-homing would clobber it). The view-effect's
    /// `lastOpModeRef` guard means a `follow_freq` QSY only fires on a real mode change, so a
    /// manual tune within a mode survives non-operating nav.
    pub fn set_operating_mode(&mut self, mode: &str, follow_freq: bool) {
        use crate::settings::OperatingMode;
        // Entering a real operating section ends the APRS FM-simplex context, and the FM-channel
        // hold with it — that section's own mode policy takes over from here.
        self.aprs_fm = false;
        self.fm_channel = false;
        let om = match mode.to_ascii_lowercase().as_str() {
            "phone" => OperatingMode::Phone,
            "cw" => OperatingMode::Cw,
            "rtty" => OperatingMode::Rtty,
            _ => OperatingMode::Digital,
        };
        self.settings.operating_mode = om;
        // SAFETY re-clamp: entering a mode with a lower power ceiling (SSB → FT8) must bring the
        // rig DOWN to the cap now, not wait for the operator to touch the power slider. If a cap
        // exists for the new mode, command at most the cap — carrying the current level when it's
        // already under it. No cap ⇒ leave the commanded power untouched.
        let ceiling = self.settings.rf_power_ceiling();
        if ceiling < 1.0 {
            let cur = self.rf_power.or(self.rig_rf_power).unwrap_or(ceiling);
            self.rf_power = Some(cur.min(ceiling));
        }
        // Explicit operating-section entry → drop to that mode's home freq on the current
        // band. `mode_home` returns None when the operator has no privilege there (Tech on
        // 20 m phone, 60 m, a band with no FT8 channel) — then we leave the dial put and let
        // the TX lockout guard the air.
        if follow_freq {
            if let Some((dial, sideband)) = self.mode_home(om) {
                let band = self.settings.band.clone();
                self.set_frequency(dial, &band, &sideband); // also flags immediate_retune
            }
        }
        // Re-assert the rig mode now even if the dial didn't change (e.g. picking CW while
        // already on a CW freq must still command CW, not wait for a dial change).
        self.immediate_retune = true;
        // Phone, CW and RTTY are MANUAL modes — there's no auto-sequencer (poll_tx is
        // gated off for non-Digital), and the operator keys TX explicitly via PTT / the
        // voice keyer / CW / an RTTY send. Arm transmit on entry so those work, like a
        // rig's live mic/key. Arming keys NOTHING by itself — every one of those paths
        // still waits for an explicit operator send. (Digital is NOT auto-armed: the FT8
        // slot TX stays behind the Monitor / double-click / Call-CQ gate, so the app
        // never auto-keys FT8 on launch — the safety invariant.)
        if matches!(
            om,
            OperatingMode::Phone | OperatingMode::Cw | OperatingMode::Rtty
        ) {
            self.set_tx_enabled(true);
        }
    }

    /// Work a spotted station (the Needed click): set the operating MODE and QSY to the
    /// spot's EXACT frequency ATOMICALLY — both under the one engine lock the caller holds, so
    /// the radio loop never observes the new mode at the old dial (no wrong-mode flash) and a
    /// single command can't half-apply (no mode/freq desync if a later step failed). The mode
    /// is set with `follow_freq = false` so its own section-QSY can't override the spot's exact
    /// frequency, which is authoritative. Sideband is always "USB" here (→ PKTUSB for digital;
    /// ignored by the CW/phone policy).
    pub fn work_spot(&mut self, mode: &str, freq_mhz: f64, band: &str) {
        self.work_spot_split(mode, freq_mhz, band, None);
    }

    /// As [`work_spot`], optionally configuring rig SPLIT for a pile-up spot whose
    /// comment named a listening offset ("UP 2" → TX dial = spot + 2 kHz). The
    /// N1MM behavior: click the spot, the rig lands on the DX's frequency with TX
    /// already split to where they're listening. `None` = simplex (and clears any
    /// prior split — handled inside set_frequency).
    pub fn work_spot_split(
        &mut self,
        mode: &str,
        freq_mhz: f64,
        band: &str,
        split_up_khz: Option<f64>,
    ) {
        self.set_operating_mode(mode, false);
        self.set_frequency(freq_mhz, band, "USB"); // clears split + arms the retune
                                                   // Working a spot carries explicit mode intent — drop any manual override (even same-band)
                                                   // so the spot's band-auto sideband applies (10 m mixes FM + SSB, so a stale FM override
                                                   // must not key FM onto an SSB spot).
        self.sideband_override = None;
        if let Some(up) = split_up_khz {
            self.split_tx_mhz = Some(freq_mhz + up / 1000.0);
            self.split_dirty = true;
        }
        // Navigation hint: working a spot should land the operator IN the matching
        // cockpit, whichever window the click came from (a pop-out board can't
        // navigate the main window directly — the snapshot carries the request,
        // like clearTick/uploadTick).
        self.work_tick += 1;
        self.work_view = Some(mode.to_string());
        self.work_call = None; // the command that has the call sets it via note_work_call
    }

    /// Record the callsign of the just-worked spot, right after [`work_spot_split`] (same lock).
    /// This is the cross-window prefill hint: a click in a pop-out band map can't touch the main
    /// window's log directly, so the call rides the snapshot and the main window prefills from it.
    pub fn note_work_call(&mut self, call: Option<String>) {
        self.work_call = call.filter(|c| !c.trim().is_empty());
    }

    /// Request a RIT (receive incremental tuning) offset in Hz (0 = off); the radio loop applies it.
    pub fn request_rit(&mut self, hz: i32) {
        self.rit_hz = hz;
        self.rit_dirty = true;
    }
    /// One-shot: the pending RIT offset to write, if any.
    pub fn take_rit_apply(&mut self) -> Option<i32> {
        self.rit_dirty.then(|| {
            self.rit_dirty = false;
            self.rit_hz
        })
    }
    /// Request an XIT (transmit incremental tuning) offset in Hz (0 = off).
    pub fn request_xit(&mut self, hz: i32) {
        self.xit_hz = hz;
        self.xit_dirty = true;
    }
    pub fn take_xit_apply(&mut self) -> Option<i32> {
        self.xit_dirty.then(|| {
            self.xit_dirty = false;
            self.xit_hz
        })
    }
    /// Select VFO A (`false`) or B (`true`).
    pub fn request_vfo(&mut self, vfo_b: bool) {
        self.active_vfo_b = vfo_b;
        self.vfo_dirty = true;
    }
    /// Swap the active VFO (A↔B).
    pub fn request_swap_vfo(&mut self) {
        self.active_vfo_b = !self.active_vfo_b;
        self.vfo_dirty = true;
    }
    /// One-shot: the pending VFO selection to write (`true` = B), if any.
    pub fn take_vfo_apply(&mut self) -> Option<bool> {
        self.vfo_dirty.then(|| {
            self.vfo_dirty = false;
            self.active_vfo_b
        })
    }

    /// Consume the one-shot split request: `Some(Some(tx))` = set split TX dial,
    /// `Some(None)` = back to simplex, `None` = nothing to apply.
    pub fn take_split_request(&mut self) -> Option<Option<f64>> {
        if self.split_dirty {
            self.split_dirty = false;
            Some(self.split_tx_mhz)
        } else {
            None
        }
    }

    /// The desired split TX dial (MHz), `None` = simplex — the UI's SPLIT badge.
    pub fn split_tx_mhz(&self) -> Option<f64> {
        self.split_tx_mhz
    }

    /// The rig REJECTED the split command — drop the desired state so the SPLIT
    /// badge never claims a split the rig isn't running (the operator works the
    /// pile-up manually; the CAT note says so).
    pub fn split_rejected(&mut self) {
        self.split_tx_mhz = None;
        self.split_dirty = false;
    }

    /// Set (`Some(tx_mhz)`) or clear (`None`) the DESIRED split TX dial from the operator/UI;
    /// the radio loop applies it on the next cycle (same path the pile-up "UP n" uses). `None`
    /// returns to simplex. Marks the request dirty so `take_split_request` picks it up.
    pub fn request_split(&mut self, tx_mhz: Option<f64>) {
        self.split_tx_mhz = tx_mhz;
        self.split_dirty = true;
    }

    /// Set (`Some("USB"|"LSB"|"FM")`) or clear (`None` = AUTO) the transient Phone mode override
    /// from the cockpit picker; the radio loop applies it next cycle via `rig_mode_effective`. A
    /// band change clears it (see `set_frequency`), so a QSY re-asserts the band-auto sideband.
    pub fn request_sideband_override(&mut self, mode: Option<&str>) {
        // Whitelist the Phone voice modes — a broker/devtools caller can't smuggle "CW" etc. in.
        self.sideband_override = mode
            .map(|m| m.trim().to_ascii_uppercase())
            .filter(|m| matches!(m.as_str(), "USB" | "LSB" | "FM"));
        self.immediate_retune = true;
    }

    /// The mode the radio loop should COMMAND: the operator's transient Phone override when set,
    /// else the band-derived policy (`settings.rig_mode`). Write-side canon for the rig `M` verb.
    pub fn rig_mode_effective(&self) -> String {
        // APRS parks the rig in FM regardless of the operating section it was navigated from — a
        // 2 m packet signal demodulated as USB is garbage, so this is a correctness gate, not a
        // preference. Gated on STILL being on 2 m: a knob-QSY (`observe_rig_freq`) or a manual
        // radio switch (`set_active_radio`) that moves off 2 m without clearing the flag can then
        // never leave FM forced on another band (the `fm_does_not_follow_the_operator_down_to_hf`
        // invariant). The flag is also cleared by every QSY / section change / radio switch.
        if self.aprs_fm && self.settings.band.eq_ignore_ascii_case("2m") {
            return "FM".to_string();
        }
        // A repeater/FM-simplex channel parks the rig in FM the same way, from whatever section the
        // operator tuned it out of. Dial-gated (not band-gated like APRS) because an FM channel can
        // be on 10 m through 23 cm, and 29 MHz is the line below which FM isn't used — the same
        // threshold `Settings::rig_mode` applies to Phone-FM, so the two can't disagree.
        if self.fm_channel && self.settings.dial_mhz >= 29.0 {
            return "FM".to_string();
        }
        if self.settings.operating_mode == crate::settings::OperatingMode::Phone {
            // SSTV rides the Phone segment but transmits SOUNDCARD audio, so — exactly like
            // FT8 — it needs a DATA submode (PKTUSB/PKTLSB → Yaesu DATA / Icom D / Kenwood
            // DATA, rig-agnostically via Hamlib) to route the USB codec to the modulator;
            // plain USB/LSB takes TX audio from the MIC and radiates ZERO RF ("red light, no
            // signal"). Only while an image is queued or in flight, so live voice PTT keeps
            // plain SSB. Driving it through the continuous mode-apply commands DATA BEFORE the
            // SSTV PTT and restores plain SSB when the image ends — no Icom-only set_data_mode.
            if self.sstv_tx.is_some() || self.sstv_sending {
                let lsb = self
                    .sideband_override
                    .as_deref()
                    .map_or(self.settings.dial_mhz < 10.0, |m| {
                        m.eq_ignore_ascii_case("lsb")
                    });
                // Same per-radio opt-out as Digital and RTTY-AFSK: a mic-jack interface needs
                // PLAIN SSB here, because on that wiring a DATA submode routes TX audio to a
                // port the interface isn't on. Off by default — see
                // `RadioProfile::data_modes_plain_ssb`.
                return self.settings.plain_ssb_if_configured(if lsb {
                    "PKTLSB"
                } else {
                    "PKTUSB"
                });
            }
            if let Some(m) = &self.sideband_override {
                return m.clone();
            }
        }
        self.settings.rig_mode()
    }

    /// The mode CLASS radio routing decides on, for a QSY to `band` / `dial_mhz` (the TARGET, not
    /// where the rig is now — a routing decision is made before the tune lands).
    ///
    /// Reads the SAME state [`Engine::rig_mode_effective`] commands the rig from — `aprs_fm`,
    /// `operating_mode`, `phone_mode`, the dial — so the radio a band+mode routes to can never
    /// disagree with the mode that radio is then put in. In particular both FM gates are mirrored:
    /// APRS-FM only counts on 2 m, and Phone-FM only at/above 29 MHz (below it FM isn't used, and
    /// `phone_mode` is one station-wide field that nothing resets on a band change).
    pub fn route_mode(&self, band: &str, dial_mhz: f64) -> crate::settings::RouteMode {
        use crate::settings::{OperatingMode, RouteMode};
        if self.aprs_fm && band.eq_ignore_ascii_case("2m") {
            return RouteMode::Fm;
        }
        // Mirrors the `fm_channel` arm of `rig_mode_effective` (see the invariant in this doc
        // comment): a rig held on an FM channel routes on FM.
        if self.fm_channel && dial_mhz >= 29.0 {
            return RouteMode::Fm;
        }
        match self.settings.operating_mode {
            OperatingMode::Cw => RouteMode::Cw,
            OperatingMode::Rtty => RouteMode::Rtty,
            OperatingMode::Digital => RouteMode::Digital,
            // SSTV rides the Phone section, so it routes as SSB — see `RouteMode`'s note on why it
            // is not its own class.
            OperatingMode::Phone => {
                // The cockpit's explicit mode pick wins with NO band gate, matching
                // `rig_mode_effective` (which returns the override verbatim) — but only for a
                // SAME-BAND move, because `set_frequency` DROPS the override on a band change, so
                // on a cross-band QSY the override describes where we're leaving, not arriving.
                let override_fm = self
                    .sideband_override
                    .as_deref()
                    .filter(|_| self.settings.band.eq_ignore_ascii_case(band))
                    .is_some_and(|m| m.eq_ignore_ascii_case("fm"));
                // Otherwise the station-wide `phone_mode`, under `rig_mode`'s 29 MHz FM gate.
                let policy_fm =
                    self.settings.phone_mode.eq_ignore_ascii_case("fm") && dial_mhz >= 29.0;
                if override_fm || policy_fm {
                    RouteMode::Fm
                } else {
                    RouteMode::Ssb
                }
            }
        }
    }

    /// Where a given (band, mode class) WOULD route right now — the Settings "test" affordance, so
    /// the operator can check a rule table without QSYing a rig. Returns the resolved radio id,
    /// which is the ACTIVE radio when nothing claims that band+mode. Read-only: touches no state.
    pub fn route_preview(&self, band: &str, mode: crate::settings::RouteMode) -> u32 {
        self.settings
            .route_radio(band, mode)
            .unwrap_or(self.settings.active_radio)
    }

    /// The active Phone mode override for the cockpit picker (`None` = AUTO / band-derived).
    pub fn sideband_override(&self) -> Option<String> {
        self.sideband_override.clone()
    }

    /// The FM repeater plumbing (shift, offset Hz, CTCSS tone) the radio loop should command while
    /// the rig is in FM. APRS forces SIMPLEX / no tone so it never inherits the operator's
    /// Phone-section repeater shift (which would transmit the beacon off-frequency).
    pub fn fm_repeater_config(&self) -> (String, i64, f32) {
        if self.aprs_fm {
            ("simplex".to_string(), 0, 0.0)
        } else {
            (
                self.settings.rptr_shift.clone(),
                self.settings.rptr_offset_hz(),
                self.settings.ctcss_tone_hz,
            )
        }
    }

    /// Set the operator's amateur license class (drives the TX-privilege lockout + the
    /// licensed-segment band dropdown). Unknown strings fall back to Open (no lockout).
    pub fn set_license_class(&mut self, class: &str) {
        use crate::settings::LicenseClass;
        self.settings.license_class = match class.to_ascii_lowercase().as_str() {
            "technician" => LicenseClass::Technician,
            "general" => LicenseClass::General,
            "extra" => LicenseClass::Extra,
            _ => LicenseClass::Open,
        };
    }

    // ----- CW transmit (CAT keyer path) — operator-initiated, gated by Monitor -----

    /// Queue CW to transmit. `text` is an F-key macro template OR literal type-ahead;
    /// it's expanded with the current QSO context (mycall/name/grid + the worked call +
    /// a 599 report) and the radio loop keys it via `rig.send_morse`.
    /// Expand a CW macro template with the live QSO context (mycall/name/grid + the worked
    /// call via `!` + a 599 report). Shared by [`Self::send_cw`] and [`Self::preview_cw`] so
    /// the cockpit's "what F2 will send" preview matches exactly what gets sent.
    fn expand_cw(&self, text: &str) -> String {
        let hiscall = self
            .app
            .active_peer()
            .map(|s| s.to_string())
            .unwrap_or_default();
        // {HISNAME}/{HISSTATE} come from the frontend's QRZ lookup (the engine has no callbook).
        // Use them ONLY when the stored info belongs to the CURRENT worked call — otherwise a
        // stale lookup could key the wrong name at the wrong station.
        let peer_matches =
            !self.cw_peer_call.is_empty() && self.cw_peer_call.eq_ignore_ascii_case(&hiscall);
        let (hisname, hisstate): (&str, &str) = if peer_matches {
            (self.cw_peer_name.as_str(), self.cw_peer_state.as_str())
        } else {
            ("", "")
        };
        // Field Day exchange tokens ({CLASS}/{SECTION}/{EXCH}) are live only while the FD
        // master switch is on; outside FD they're empty so a stray token collapses cleanly.
        let (class, section): (&str, &str) = if self.settings.fd_active {
            (&self.settings.fd_class, &self.settings.fd_section)
        } else {
            ("", "")
        };
        let ctx = tempo_core::cw::CwContext {
            mycall: &self.settings.mycall,
            myname: &self.settings.op_name,
            mygrid: &self.settings.mygrid,
            mystate: &self.settings.op_state,
            hiscall: &hiscall,
            hisname,
            hisstate,
            rst: "599",
            class,
            section,
        };
        tempo_core::cw::expand(text, &ctx)
    }

    /// Record the worked station's QRZ name + US state for the `{HISNAME}`/`{HISSTATE}` CW
    /// macro tokens. Pushed by the frontend when a callbook lookup resolves; `call` keys it to
    /// the contact so a stale lookup never keys the wrong name (see `expand_cw`). Empty
    /// `call` clears it (the operator cleared the log field).
    pub fn set_cw_peer_info(&mut self, call: String, name: String, state: String) {
        self.cw_peer_call = call.trim().to_string();
        self.cw_peer_name = name.trim().to_string();
        self.cw_peer_state = state.trim().to_string();
    }

    /// Expand a CW macro WITHOUT queuing it — the cockpit's reply preview.
    pub fn preview_cw(&self, text: &str) -> String {
        self.expand_cw(text)
    }

    pub fn send_cw(&mut self, text: &str) {
        let expanded = self.expand_cw(text);
        if !expanded.trim().is_empty() {
            // A deliberate CW send (F-key macro or typed text) IS the transmit action, so RE-ARM
            // TX — exactly like entering a manual mode arms it (`set_operating_mode`). Without this,
            // a prior Stop TX / halt_tx (which disables tx_enabled to stop the FT8 auto-sequencer)
            // leaves `poll_cw_one` gated off, so the next F-key silently queues but never keys until
            // the operator re-enters the mode (a band/contact switch). CW is manual keying: hitting
            // the key must always transmit (privilege permitting — tx_allowed still gates). FT8 is
            // unaffected: it never calls send_cw and keeps its Monitor/double-click keying gate.
            self.tx_enabled = true;
            self.cw_abort = false; // a fresh send supersedes a pending one-shot abort
                                   // TX echo: show the operator what actually went out (tokens resolved).
            self.cw_sent.push_back(expanded.clone());
            while self.cw_sent.len() > 50 {
                self.cw_sent.pop_front();
            }
            // Queue WORD-BY-WORD so the radio loop feeds the rig one word at a time. That
            // keeps at most one word in the rig's CW keyer buffer, so Stop TX (which clears
            // this queue) drops the rest of the macro instead of the rig playing it all out.
            for word in expanded.split_whitespace() {
                self.cw_queue.push_back(word.to_string());
            }
        }
    }

    /// Recent expanded CW transmissions (oldest→newest) — the cockpit's SENT echo.
    pub fn cw_sent(&self) -> Vec<String> {
        self.cw_sent.iter().cloned().collect()
    }

    /// A CW-keyer failure to surface in the cockpit (rig rejected CAT keying), else None.
    pub fn cw_keyer_error(&self) -> Option<String> {
        self.cw_keyer_error.clone()
    }

    /// Record (or clear) a CW-keyer failure — the radio loop calls this after a send.
    pub fn set_cw_keyer_error(&mut self, e: Option<String>) {
        self.cw_keyer_error = e;
    }

    /// Set the CW keyer speed in WPM (clamped 5..=50).
    ///
    /// Lives in `settings` (not a duplicate engine field) exactly like `cw_pitch_hz`, so it
    /// persists and cannot drift from what gets saved. The CALLER decides whether to write
    /// settings to disk — the CW decoder's automatic speed-match also lands here, and the
    /// operator's stored preference must not be overwritten by whatever the last station sent.
    pub fn set_cw_wpm(&mut self, wpm: u32) {
        self.settings.cw_wpm = wpm.clamp(5, 50);
    }

    /// FT8/FT4 decode depth (1=Fast, 2=Normal, 3=Deep). Live-settable from the Operate cockpit so
    /// the operator can trade CPU/battery for weak-signal decodes mid-session (a POTA field lever),
    /// not only from Settings. The decoder reads `settings.decode_depth` on the next slot.
    pub fn set_decode_depth(&mut self, depth: u8) {
        self.settings.decode_depth = depth.clamp(1, 3);
    }

    /// Operator CW decode sensitivity in [0, 1] (0.5 = the original gates; higher catches
    /// weaker/off-pitch marks like the skimmer, lower rejects more noise).
    pub fn set_cw_sensitivity(&mut self, s: f32) {
        self.cw_stream.set_sensitivity(s);
    }

    /// Choose the CW keyer back-end ("cat" or "soundcard") + tone pitch (Hz; ignored
    /// if <= 0). Soundcard flips the CW rig-mode to USB; the radio loop re-applies it.
    pub fn set_cw_keyer(&mut self, backend: &str, pitch_hz: f32) {
        use crate::settings::CwKeyerBackend;
        self.cw_keyer_error = None; // a keyer change invalidates a prior keyer error
        self.settings.cw_keyer = match backend.to_ascii_lowercase().as_str() {
            "soundcard" => CwKeyerBackend::Soundcard,
            "winkeyer" => CwKeyerBackend::WinKeyer,
            "serial" => CwKeyerBackend::Serial,
            _ => CwKeyerBackend::Cat,
        };
        if pitch_hz > 0.0 {
            self.settings.cw_pitch_hz = pitch_hz.clamp(300.0, 1200.0);
        }
    }

    /// Abort CW in progress: the radio loop stops the rig and the queue is cleared.
    pub fn stop_cw(&mut self) {
        self.cw_queue.clear();
        self.cw_abort = true;
    }

    /// Pop the next queued CW WORD for the radio loop to key, or None if the queue is empty
    /// or TX is disabled (Monitor / outside privileges — the queue is then held, so a stray
    /// macro never keys unexpectedly). One word per call so the loop paces the send and Stop
    /// TX (which clears the queue) can drop the remainder before it reaches the rig.
    pub fn poll_cw_one(&mut self) -> Option<String> {
        if !self.tx_enabled || !self.tx_allowed() {
            return None;
        }
        self.cw_queue.pop_front()
    }

    /// Take + reset the one-shot CW abort flag (the loop calls `rig.stop_morse`).
    pub fn take_cw_abort(&mut self) -> bool {
        std::mem::take(&mut self.cw_abort)
    }

    /// Current CW keyer speed (WPM) — for the radio loop's `set_keyspd` + the snapshot.
    pub fn cw_wpm(&self) -> u32 {
        self.settings.cw_wpm
    }

    /// True if CW uses the SOUNDCARD keyer (the loop generates a keyed tone + PTT,
    /// rig in USB) vs the CAT keyer (`rig.send_morse`, rig in CW).
    pub fn cw_soundcard(&self) -> bool {
        matches!(
            self.settings.cw_keyer,
            crate::settings::CwKeyerBackend::Soundcard
        )
    }

    /// The WinKeyer serial port when the CW keyer backend is WinKeyer and a port is set —
    /// the radio loop opens/keys it. `None` for the CAT/soundcard keyers.
    pub fn cw_winkeyer_port(&self) -> Option<String> {
        if matches!(
            self.settings.cw_keyer,
            crate::settings::CwKeyerBackend::WinKeyer
        ) {
            let p = self.settings.winkeyer_port.trim();
            (!p.is_empty()).then(|| p.to_string())
        } else {
            None
        }
    }

    /// The serial keyline port when the CW keyer backend is Serial and a port is set — the
    /// radio loop opens it and toggles DTR/RTS (rig in CW). `None` for the other keyers.
    pub fn cw_serial_key_port(&self) -> Option<String> {
        if matches!(
            self.settings.cw_keyer,
            crate::settings::CwKeyerBackend::Serial
        ) {
            let p = self.settings.cw_key_port.trim();
            (!p.is_empty()).then(|| p.to_string())
        } else {
            None
        }
    }

    /// Which control line the serial keyline toggles ("dtr"/"rts").
    pub fn cw_serial_key_line(&self) -> String {
        self.settings.cw_key_line.clone()
    }

    /// CW tone pitch (Hz) for the soundcard keyer.
    pub fn cw_pitch_hz(&self) -> f32 {
        self.settings.cw_pitch_hz
    }

    // ----- Phone (voice) — manual PTT + RF power, applied by the radio loop -----

    /// Manually key (true) / unkey (false) the rig for live phone. Gated by Monitor:
    /// a key request is ignored while TX is disabled. The radio loop applies it.
    pub fn set_ptt(&mut self, on: bool) {
        self.manual_ptt = on && self.tx_enabled && self.tx_allowed();
    }

    /// Whether the operator is holding manual PTT (live phone) — read by the loop. Also
    /// masks on read, so a key that became out-of-privilege (knob turned to a locked
    /// segment while holding PTT) drops the next loop pass.
    pub fn manual_ptt(&self) -> bool {
        (self.manual_ptt || self.broker_ptt) && self.tx_enabled && self.tx_allowed()
    }

    /// PTT arbitration for a FOREIGN app on the CAT broker (WSJT-X/N1MM sharing
    /// the rig). Allowed ONLY when the operator opted in (settings.cat_broker_ptt),
    /// TX is enabled/legal, and Nexus itself is idle (no manual phone PTT held) —
    /// Nexus's own key always wins; a refused request returns false so the broker
    /// answers the client honestly. Un-key is always honored (safety).
    pub fn broker_ptt(&mut self, on: bool) -> bool {
        if !on {
            self.broker_ptt = false;
            return true; // un-key always succeeds
        }
        if !self.settings.cat_broker_ptt
            || !self.tx_enabled
            || !self.tx_allowed()
            || self.manual_ptt
            // A live tune carrier or an FT8 over in flight also owns the rig —
            // granting a foreign key mid-transmission double-keys it (review).
            || self.tuning
            || self.app.radio.transmitting
        {
            return false;
        }
        self.broker_ptt = true;
        true
    }

    /// Whether a broker client currently holds PTT (for tests/inspection).
    pub fn broker_ptt_active(&self) -> bool {
        self.broker_ptt
    }

    /// Set desired RF output power (0.0–1.0). The radio loop applies it via the rig.
    pub fn set_rf_power(&mut self, frac: f32) {
        // SAFETY CEILING: never command the rig above the current mode's per-mode cap (FT8/FT4/
        // RTTY duty-cycle protection). The single chokepoint — every power set, from any UI path,
        // passes through here — so the cap cannot be bypassed.
        let ceiling = self.settings.rf_power_ceiling();
        self.rf_power = Some(frac.clamp(0.0, ceiling));
    }

    /// The RF power the radio loop should COMMAND, plus whether to FORCE the command even if it
    /// hasn't changed. The per-mode cap is a HARD, continuous ceiling — not a one-shot on mode
    /// change: if the rig is observed running ABOVE the current mode's cap (the operator raised
    /// power on the rig's own knob, or re-entered a higher-cap mode still hot from a manual
    /// override), capture that as "run at the cap" — clamp `rf_power` down to the ceiling AND force
    /// the loop to command it down now. This is why the earlier version "only worked once": it
    /// clamped the commanded value but never re-asserted against the rig's actual level. Enforcement
    /// only ever LOWERS power, so it is safe-direction. Called each radio-loop tick.
    pub fn rf_power_to_command(&mut self) -> Option<(f32, bool)> {
        let ceiling = self.settings.rf_power_ceiling();
        let mut force = false;
        if ceiling < 1.0 {
            // 0.02 epsilon absorbs rig readback rounding so a settled cap doesn't re-command.
            if self
                .rig_rf_power
                .is_some_and(|observed| observed > ceiling + 0.02)
            {
                self.rf_power = Some(ceiling);
                force = true;
            }
        }
        self.rf_power.map(|p| (p, force))
    }

    /// Desired RF power, if the operator has set one (for the radio loop).
    pub fn rf_power(&self) -> Option<f32> {
        self.rf_power
    }

    /// Adopt the rig's reported RF power (radio-loop poll). Observed-only —
    /// never touches the commanded `rf_power`, so a user drag in flight wins.
    pub fn observe_rig_power(&mut self, frac: f32) {
        if frac.is_finite() {
            self.rig_rf_power = Some(frac.clamp(0.0, 1.0));
        }
    }

    /// Set desired mic gain (0.0–1.0). The radio loop applies it via the rig.
    pub fn set_mic_gain(&mut self, frac: f32) {
        self.mic_gain = Some(frac.clamp(0.0, 1.0));
    }
    /// Desired mic gain, if the operator has set one (for the radio loop).
    pub fn mic_gain(&self) -> Option<f32> {
        self.mic_gain
    }

    /// The roster's cached Maidenhead grid for a heard callsign (own decodes / PSK Reporter),
    /// if any — the same backfill the decode feed uses. Lets a caller resolve the station's US
    /// state or grid rarity for a cluster/RBN spot, which carries no grid of its own.
    pub fn roster_grid(&self, call: &str) -> Option<&str> {
        self.app
            .inbox
            .roster
            .get(call)
            .and_then(|h| h.grid.as_deref())
    }
    /// Adopt the rig's reported mic gain (radio-loop poll). Observed-only.
    pub fn observe_rig_mic_gain(&mut self, frac: f32) {
        if frac.is_finite() {
            self.rig_mic_gain = Some(frac.clamp(0.0, 1.0));
        }
    }

    /// Set desired noise-reduction level (0.0–1.0); the radio loop applies it.
    pub fn set_nr_level(&mut self, frac: f32) {
        self.nr_level = Some(frac.clamp(0.0, 1.0));
    }
    pub fn nr_level(&self) -> Option<f32> {
        self.nr_level
    }
    pub fn observe_rig_nr_level(&mut self, frac: f32) {
        if frac.is_finite() {
            self.rig_nr_level = Some(frac.clamp(0.0, 1.0));
        }
    }

    /// Set desired AGC speed ("fast"|"mid"|"slow"); the radio loop applies it on change.
    pub fn set_agc(&mut self, speed: &str) {
        if matches!(speed, "fast" | "mid" | "slow") {
            self.agc = Some(speed.to_string());
        }
    }
    pub fn agc(&self) -> Option<String> {
        self.agc.clone()
    }
    pub fn observe_rig_agc(&mut self, speed: String) {
        if matches!(speed.as_str(), "fast" | "mid" | "slow") {
            self.rig_agc = Some(speed);
        }
    }

    /// Adopt the rig's reported S-meter (dB relative to S9), from the radio-loop poll.
    /// Observed-only; the UI renders it as a calibrated S-unit meter. The poll reads
    /// STRENGTH only during RX, so this never carries a meaningless TX value.
    pub fn observe_rig_smeter(&mut self, db: i32) {
        self.rig_smeter_db = Some(db);
    }

    /// Adopt the rig's read-back mode (Hamlib name) for DISPLAY ONLY — the cockpit uses it
    /// to flag when the operator's mode knob disagrees with the commanded mode. Never touches
    /// the canonical commanded sideband.
    pub fn observe_rig_mode(&mut self, mode: String) {
        let m = mode.trim();
        if !m.is_empty() {
            self.rig_mode = Some(m.to_string());
        }
    }

    /// Drop the last S-meter reading so the UI reverts to "—" — called when CAT goes
    /// half-open (breaker trip) or the rig is found not to report STRENGTH, so a frozen
    /// needle never implies a signal that is no longer being measured.
    pub fn clear_rig_smeter(&mut self) {
        self.rig_smeter_db = None;
    }

    /// Adopt the rig's transmit meters (from the keyed-only poll): SWR ratio, ALC 0..1, Po
    /// watts, COMP dB. Each is independently `Some`/`None` so a rig that reports only some of
    /// them still shows those. The poll reads these ONLY while keyed, mirroring the S-meter.
    pub fn observe_rig_tx_meters(
        &mut self,
        swr: Option<f32>,
        alc: Option<f32>,
        po_w: Option<f32>,
        comp_db: Option<f32>,
    ) {
        if swr.is_some() {
            self.rig_tx_swr = swr;
        }
        if alc.is_some() {
            self.rig_tx_alc = alc;
        }
        if po_w.is_some() {
            self.rig_tx_po_w = po_w;
        }
        if comp_db.is_some() {
            self.rig_tx_comp_db = comp_db;
        }
    }

    /// Blank the transmit meters — called on unkey and on a CAT breaker trip, so the bars
    /// don't freeze at the last keyed reading while receiving.
    pub fn clear_rig_tx_meters(&mut self) {
        self.rig_tx_swr = None;
        self.rig_tx_alc = None;
        self.rig_tx_po_w = None;
        self.rig_tx_comp_db = None;
    }

    /// Drop the rig's read-back mode so the mismatch tag hides — called on a breaker trip
    /// and right after the app commands a retune, so a stale pre-change mode can't flash a
    /// false "rig: X" mismatch until the next poll reads the rig's true mode.
    pub fn clear_rig_mode(&mut self) {
        self.rig_mode = None;
    }

    /// Adopt the rig's CAT DSP-function states `[nb, nr, notch, comp, vox]` from the radio-loop
    /// poll. A `None` slot = the rig doesn't support that func (its toggle hides). Observed-only.
    pub fn observe_rig_funcs(&mut self, funcs: [Option<bool>; 5]) {
        self.rig_funcs = funcs;
    }

    /// Adopt the radio's RX frequency ranges (Hz) from the radio loop's capability probe.
    /// `None` = unknown, which every caller must treat as "allow" — see [`Self::rig_covers_mhz`].
    pub fn observe_rig_rx_ranges(&mut self, ranges: Option<Vec<(u64, u64)>>) {
        self.rig_rx_ranges = ranges;
    }

    /// Record (or clear) the dial the radio refused, so the UI can say WHICH frequency and why.
    pub fn set_rig_refused_dial(&mut self, mhz: Option<f64>) {
        self.rig_refused_dial_mhz = mhz;
    }

    /// Can the active radio RECEIVE `mhz`? `None` = **unknown, so don't block anything** — no CAT,
    /// caps not probed yet, or a `\dump_state` we couldn't parse.
    ///
    /// Fail-open is the whole safety property here. A capability probe that wrongly answered "no"
    /// would refuse legitimate QSYs and be far more damaging than the refused command it exists to
    /// prevent, so only a range list we positively parsed may ever say no.
    pub fn rig_covers_mhz(&self, mhz: f64) -> Option<bool> {
        let ranges = self.rig_rx_ranges.as_ref()?;
        if ranges.is_empty() {
            return None;
        }
        let hz = (mhz * 1_000_000.0).round() as u64;
        Some(ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&hz)))
    }

    /// Drop all rig func states (→ the toggles hide) — called on a breaker trip so a half-open
    /// CAT link never freezes stale NB/NR/… states in the cockpit.
    pub fn clear_rig_funcs(&mut self) {
        self.rig_funcs = [None; 5];
    }

    /// Queue a func toggle from the UI (`func` = "nb"|"nr"|"notch"|"comp"|"vox"); the radio loop
    /// applies it next cycle (off the TCP path). Also updates the observed state OPTIMISTICALLY so
    /// the snapshot returned to the UI reflects the click at once — the loop's next GET reconciles
    /// it (and reverts if the rig rejected the set). Unknown names are ignored.
    pub fn request_rig_func(&mut self, func: &str, on: bool) {
        if let Some(i) = func_index(func) {
            self.pending_func[i] = Some(on);
            self.rig_funcs[i] = Some(on);
        }
    }

    /// Drain the pending func requests for the radio loop to apply — `[nb, nr, notch, comp, vox]`,
    /// each `Some(on)` to apply then cleared. Mirrors `take_split_request`.
    pub fn take_func_requests(&mut self) -> [Option<bool>; 5] {
        std::mem::take(&mut self.pending_func)
    }

    /// Adopt the rig's RX passband width (Hz) from the poll. `None` (a split `m` read) keeps the
    /// last known width so the display doesn't flicker; a real value updates it.
    pub fn observe_rig_passband(&mut self, hz: Option<u32>) {
        if hz.is_some() {
            self.rig_passband = hz;
        }
    }

    /// Drop the RX passband width (→ display shows unknown) on a breaker trip.
    pub fn clear_rig_passband(&mut self) {
        self.rig_passband = None;
    }

    /// Queue an RX filter-width change (Hz) from the UI; the radio loop applies it via set_mode
    /// (Hamlib carries width as set_mode's 2nd arg). Off the TCP path. Also updates the observed
    /// width OPTIMISTICALLY so the snapshot reflects the click at once — rapid ± steps accumulate
    /// (rather than all reading the same stale value), and the loop's next read reconciles it.
    pub fn request_filter_width(&mut self, hz: u32) {
        self.pending_passband = Some(hz);
        self.rig_passband = Some(hz);
    }

    /// Drain a pending filter-width request for the radio loop.
    pub fn take_passband_request(&mut self) -> Option<u32> {
        self.pending_passband.take()
    }

    /// Queue a native-scope SPAN change (Hz, ± half-width) from the UI. Native Icom CI-V only;
    /// the loop drains it and drives the rig's real panadapter width while not keyed.
    pub fn request_scope_span(&mut self, span_hz: u32) {
        self.pending_scope_span = Some(span_hz);
    }
    /// Queue a native-scope REFERENCE-level change (tenths of a dB, −200..+200) from the UI.
    pub fn request_scope_ref(&mut self, ref_tenths_db: i32) {
        self.pending_scope_ref = Some(ref_tenths_db);
    }
    /// Queue a native-scope CENTER/FIXED mode change from the UI (`true` = fixed).
    pub fn request_scope_fixed(&mut self, fixed: bool) {
        self.pending_scope_fixed = Some(fixed);
    }

    /// FlexRadio panadapter controls, applied continuously by the FlexSpectrum worker (not
    /// drained). Set the pan BANDWIDTH (Hz) — the worker `display pan set … bw=` and re-labels the
    /// emitted RF span. Clamped to SmartSDR's practical range.
    pub fn set_flex_pan_span(&mut self, span_hz: f64) {
        self.flex_pan_span_hz = span_hz.clamp(5_000.0, 14_000_000.0);
    }
    /// Set the Flex pan REFERENCE level (dBm) — the worker sets the pan's dB window; `None` = auto.
    pub fn set_flex_pan_ref(&mut self, ref_dbm: Option<i32>) {
        self.flex_pan_ref_dbm = ref_dbm.map(|d| d.clamp(-160, 20));
    }
    /// The current Flex pan bandwidth (Hz) — polled by the FlexSpectrum worker.
    pub fn flex_pan_span_hz(&self) -> f64 {
        self.flex_pan_span_hz
    }
    /// The current Flex pan reference level (dBm), or `None` for auto — polled by the worker.
    pub fn flex_pan_ref_dbm(&self) -> Option<i32> {
        self.flex_pan_ref_dbm
    }
    /// Drain the pending native-scope control one-shots for the radio loop.
    pub fn take_scope_span_request(&mut self) -> Option<u32> {
        self.pending_scope_span.take()
    }
    pub fn take_scope_ref_request(&mut self) -> Option<i32> {
        self.pending_scope_ref.take()
    }
    pub fn take_scope_fixed_request(&mut self) -> Option<bool> {
        self.pending_scope_fixed.take()
    }

    // ----- Phone voice keyer — play recorded WAVs + record, via the radio loop -----

    /// Queue 12 kHz mono samples (a decoded voice-keyer WAV) for transmission. Ignored
    /// while TX is disabled (Monitor), so a stray F-key never keys unexpectedly. Replaces
    /// any still-pending message (one voice over at a time).
    pub fn send_voice(&mut self, samples: Vec<f32>) {
        if self.tx_enabled && self.tx_allowed() && !samples.is_empty() {
            self.voice_tx = Some(samples);
        }
    }

    /// Take the pending voice samples for the radio loop to play (gated on Monitor + privileges).
    pub fn poll_voice(&mut self) -> Option<Vec<f32>> {
        if !self.tx_enabled || !self.tx_allowed() {
            return None;
        }
        self.voice_tx.take()
    }

    /// Abort voice playback in progress: drop any pending message + raise the one-shot
    /// abort flag so the loop flushes the output ring and unkeys.
    pub fn stop_voice(&mut self) {
        self.voice_tx = None;
        self.voice_abort = true;
    }

    /// Take + reset the one-shot voice abort flag (the loop flushes output + unkeys).
    pub fn take_voice_abort(&mut self) -> bool {
        std::mem::take(&mut self.voice_abort)
    }

    /// Begin recording a voice message — the radio loop appends captured audio to the
    /// record buffer until `stop_recording`. Resets any prior buffer.
    pub fn start_recording(&mut self) {
        self.record_buf.clear();
        self.recording = true;
    }

    /// Whether a voice-message recording is in progress (read by the radio loop).
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// Append captured 12 kHz mono samples to the in-progress recording (radio loop).
    pub fn push_record_samples(&mut self, samples: &[f32]) {
        if self.recording {
            self.record_buf.extend_from_slice(samples);
        }
    }

    /// Stop recording and take the captured 12 kHz mono buffer (the command writes the WAV).
    pub fn stop_recording(&mut self) -> Vec<f32> {
        self.recording = false;
        std::mem::take(&mut self.record_buf)
    }

    /// The configured voice-keyer slots (for the UI list + the play command's path lookup).
    pub fn voice_messages(&self) -> &[crate::settings::VoiceMessage] {
        &self.settings.voice_messages
    }

    /// Bind a voice-keyer slot — set its label and/or file path (creates the slot if new).
    /// `None` leaves that field unchanged.
    pub fn set_voice_message(&mut self, slot: u8, label: Option<&str>, file: Option<&str>) {
        if let Some(m) = self
            .settings
            .voice_messages
            .iter_mut()
            .find(|m| m.slot == slot)
        {
            if let Some(l) = label {
                m.label = l.to_string();
            }
            if let Some(f) = file {
                m.file = f.to_string();
            }
        } else {
            self.settings
                .voice_messages
                .push(crate::settings::VoiceMessage {
                    slot,
                    label: label.unwrap_or_default().to_string(),
                    file: file.unwrap_or_default().to_string(),
                });
        }
    }

    /// Clear the recording bound to a slot (keeps its label).
    pub fn clear_voice_message(&mut self, slot: u8) {
        if let Some(m) = self
            .settings
            .voice_messages
            .iter_mut()
            .find(|m| m.slot == slot)
        {
            m.file.clear();
        }
    }

    // ----- QSO recording (audio bridge) — stream live RX capture to disk via the loop -----

    /// Begin streaming the live RX capture to `path` (a WAV the radio loop opens + writes).
    pub fn start_qso_recording(&mut self, path: &str) {
        self.qso_record_path = Some(path.to_string());
        self.qso_recording = true;
    }

    /// Stop QSO recording — the radio loop finalizes the WAV on its next iteration.
    pub fn stop_qso_recording(&mut self) {
        self.qso_recording = false;
        self.qso_record_path = None;
    }

    /// Whether a QSO recording is in progress (radio loop + snapshot REC badge).
    pub fn is_qso_recording(&self) -> bool {
        self.qso_recording
    }

    /// The target WAV path for the in-progress QSO recording (for the radio loop's sink).
    pub fn qso_record_path(&self) -> Option<String> {
        self.qso_record_path.clone()
    }

    // ----- coordinated QSY ("move together") ------------------------------
    //
    // A SEPARATE, opt-in function. Every method here is a no-op while the feature
    // is disabled, and none of it runs in the per-slot hooks unless
    // `settings.qsy_enabled` is true — so the primary modes are untouched.

    /// The band-plan channel token for the current dial (falls back to the band
    /// label if the dial isn't a known channel) — used as the home channel.
    fn qsy_token_for_current(&self) -> String {
        crate::bandplan::channel_for_dial(self.settings.dial_mhz)
            .map(|c| c.band)
            .unwrap_or_else(|| self.settings.band.clone())
    }

    /// Retune to a band-plan channel by its token (the QSY execution primitive),
    /// reusing the live [`Engine::set_frequency`] path the radio loop follows.
    fn execute_qsy_token(&mut self, token: &str) {
        if let Some(c) = crate::bandplan::band_plan()
            .into_iter()
            .find(|c| c.band.eq_ignore_ascii_case(token))
        {
            self.set_frequency(c.dial_mhz, &c.band, &c.mode);
        }
    }

    /// Enable / disable coordinated QSY. Enabling captures the current channel as
    /// home and the selected peer as the roaming partner; disabling returns the
    /// operator to the home channel. Persisted in settings so it survives restart.
    pub fn qsy_set_enabled(&mut self, on: bool) {
        if on {
            self.qsy
                .configure(self.settings.qsy_set.clone(), self.settings.qsy_cadence);
            let home = self.qsy_token_for_current();
            let partner = self.app.active_peer().map(|s| s.to_string());
            self.qsy.enable(home, partner);
            self.settings.qsy_enabled = true;
        } else {
            let home = self.qsy.disable();
            self.settings.qsy_enabled = false;
            if let Some(t) = home {
                self.execute_qsy_token(&t);
            }
        }
    }

    /// Update the QSY channel set + announce cadence (live + persisted).
    pub fn qsy_configure(&mut self, set: Vec<String>, cadence: u64) {
        self.settings.qsy_set = set.clone();
        self.settings.qsy_cadence = cadence.max(1);
        self.qsy.configure(set, cadence);
    }

    /// Set the roaming partner (defaults to the selected peer). Determines the
    /// initiator/follower roles.
    pub fn qsy_set_partner(&mut self, partner: Option<String>) {
        self.qsy.set_partner(partner);
    }

    /// Manual override: force the initiator to announce a move on its next over.
    pub fn qsy_move_now(&mut self) {
        if self.settings.qsy_enabled {
            self.qsy.request_move_now();
        }
    }

    /// Manual override: hold on the current channel (pause) or resume hopping.
    pub fn qsy_pause(&mut self, on: bool) {
        if self.settings.qsy_enabled {
            self.qsy.set_paused(on);
        }
    }

    /// Manual override: stop the feature and return to the home channel.
    pub fn qsy_stop(&mut self) {
        self.qsy_set_enabled(false);
    }

    /// Whether coordinated QSY is live: opt-in flag on AND we're in the
    /// conversational (Chat) flow. In the auto-QSO / Field-Day sequencer modes
    /// the whole feature stays dormant so it can't perturb their timing.
    fn qsy_active(&self) -> bool {
        self.settings.qsy_enabled && matches!(self.mode, Mode::Chat)
    }

    /// Execute a scheduled QSY move when it comes due (called every slot, any
    /// TX/RX state). No-op unless the feature is live (Chat + enabled).
    fn qsy_execute_due(&mut self, slot: u64) {
        if !self.qsy_active() {
            return;
        }
        if let Some(token) = self.qsy.take_due(slot) {
            self.execute_qsy_token(&token);
            // Moving frequency invalidates any in-progress IR-HARQ combine
            // (stale RV frames from the old channel must not combine here).
            self.harq_reset_locked();
        }
    }

    /// Enqueue an announced QSY directive as an open broadcast — to the FRONT of
    /// the broadcast queue so the current over carries it — and echo it into our
    /// own band feed (everything in the clear).
    fn enqueue_qsy_directive(&mut self, dir: &Directive) {
        let mycall = self.settings.mycall.clone();
        let body = dir.format();
        let full = tempo_core::inbox::broadcast_text(&mycall, &body);
        let id = (b'A' + self.broadcast_id) as char;
        self.broadcast_id = (self.broadcast_id + 1) % 26;
        for f in tempo_core::text::chunk(&full, id).into_iter().rev() {
            self.broadcast_queue.push_front(f);
        }
        self.app.note_broadcast(&body);
    }

    /// Project the current coordinated-QSY state into its status DTO.
    fn qsy_status(&self) -> QsyStatus {
        let role = if self.qsy.partner.is_none() {
            "idle"
        } else if self.qsy.is_initiator_now(&self.settings.mycall) {
            "initiator"
        } else {
            "follower"
        }
        .to_string();
        let (next_channel, next_slot) = match &self.qsy.pending {
            Some(p) => (Some(p.token.clone()), Some(p.at_slot)),
            None => (None, None),
        };
        QsyStatus {
            enabled: self.qsy.enabled,
            paused: self.qsy.paused,
            role,
            partner: self.qsy.partner.clone(),
            home: self.qsy.home.clone(),
            current: self.qsy.current.clone(),
            next_channel,
            next_slot,
            lost_sync: self.qsy.lost_sync,
        }
    }

    // ----- logbook --------------------------------------------------------

    /// Journal the store-and-forward queue (write-tmp + fsync + rename, like the
    /// pending-QSO journal). Written the MOMENT the queue changes — queue/release/
    /// ACK/archive — so a crash or power cut can't silently drop a held message the
    /// operator watched enter "waiting to send". An empty queue removes the file.
    pub fn persist_pending_msgs(&self) {
        let Some(path) = &self.station.pending_msgs_path else {
            return;
        };
        let items = self.app.export_pending();
        if items.is_empty() {
            let _ = std::fs::remove_file(path);
            return;
        }
        let dto: Vec<PendingMsgJournal> = items.iter().map(PendingMsgJournal::from).collect();
        let Ok(text) = serde_json::to_string(&dto) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        let res = std::fs::File::create(&tmp)
            .and_then(|mut f| {
                std::io::Write::write_all(&mut f, text.as_bytes())?;
                f.sync_all()
            })
            .and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = res {
            eprintln!("tempo: failed to journal pending messages: {e}");
        }
    }

    /// Restore the journaled queue at startup. Call AFTER `set_pending_msgs_path`
    /// and BEFORE `restore_conversations` — the held-vs-abandoned decision on each
    /// restored bubble reads the live queue.
    pub fn load_pending_msgs(&mut self, text: &str) {
        let Ok(items) = serde_json::from_str::<Vec<PendingMsgJournal>>(text) else {
            return;
        };
        self.app.restore_pending(
            items
                .into_iter()
                .map(PendingMsgJournal::into_pending)
                .collect(),
        );
    }

    /// Journal (or clear) the QSO held by the prompt-to-log popup.
    ///
    /// Written the MOMENT the hold changes, not at exit: a finished contact — exchange
    /// complete, the other station already logged it — sat only in memory while the popup
    /// waited, so a crash, a power cut, or an unattended reboot destroyed a real QSO with no
    /// trace. An exit hook would only have covered a clean quit. Mirrors the Field Day
    /// journal (`persist_fd_log`), which already got this right per contact.
    ///
    /// Same write-tmp + fsync + rename as the FD journal, so a crash mid-write cannot leave a
    /// truncated record. `None` removes the file — a confirmed or discarded QSO must not come
    /// back on the next launch.
    pub fn persist_pending_qso(&self) {
        let Some(path) = &self.station.pending_qso_path else {
            return;
        };
        let Some(rec) = &self.pending_log else {
            let _ = std::fs::remove_file(path);
            return;
        };
        let dto: crate::dto::LoggedQso = rec.clone().into();
        let Ok(text) = serde_json::to_string(&dto) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        let res = std::fs::File::create(&tmp)
            .and_then(|mut f| {
                std::io::Write::write_all(&mut f, text.as_bytes())?;
                f.sync_all() // on disk BEFORE the rename publishes it
            })
            .and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = res {
            eprintln!("tempo: failed to journal the pending QSO: {e}");
        }
    }

    /// Restore a QSO left in the prompt-to-log popup by a previous session (crash, power
    /// loss, or a quit with the popup open) so the operator can still log it. Called by the
    /// shell at startup AFTER [`Self::set_pending_qso_path`]. Ignored if a QSO is already
    /// held — a live hold outranks a restored one.
    pub fn load_pending_qso(&mut self, rec: QsoRecord) {
        if self.pending_log.is_none() {
            self.pending_log = Some(rec);
        }
    }

    /// Flush the in-memory Field Day contest log to `fd_log_path` as ADIF
    /// (write-tmp + fsync + rename, so neither a crash mid-write nor a power loss
    /// right after the rename can truncate the journal — each save REPLACES the
    /// whole file, so a torn publish would lose every contact, and Field Day runs
    /// on generators; same rationale as `Settings::save`).
    /// Best-effort: a write hiccup must never take down a logging path. No-op
    /// with no path set, outside FD mode, or on an empty log.
    pub fn persist_fd_log(&self) {
        let (Some(path), Some(text)) = (&self.station.fd_log_path, self.field_day_log_adif())
        else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("adi.tmp");
        let res = std::fs::File::create(&tmp)
            .and_then(|mut f| {
                std::io::Write::write_all(&mut f, text.as_bytes())?;
                f.sync_all() // data on disk BEFORE the rename publishes it
            })
            .and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = res {
            eprintln!("tempo: field day log save failed: {e}");
        }
    }

    /// Manually log a Field Day contact from the CW/Phone cockpits (the
    /// digital sequencer logs its own). `mode` = scoring class "CW" | "PH".
    /// Returns false on a band+mode dupe (nothing logged).
    pub fn fd_log_manual(
        &mut self,
        call: &str,
        class: &str,
        section: &str,
        mode: &str,
    ) -> Result<bool, String> {
        self.sync_fd_band(); // a knob-QSY between contacts must stamp the REAL band
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return Err("Field Day mode is not active".into());
        };
        let logged = station
            .log
            .log_mode_at(call, class, section, mode, 0, now_unix_secs());
        if logged {
            self.persist_fd_log(); // journal every contact — a crash loses nothing
        }
        Ok(logged)
    }

    /// Field Day score = QSO points × power multiplier + claimed bonuses.
    /// (Section count is the reported multiplier-equivalent; ARRL FD totals
    /// QSO-points × power, plus bonus points, and lists sections separately.)
    pub fn fd_score(&self) -> Option<(u32, u32, u32)> {
        let Mode::FieldDay { station, .. } = &self.mode else {
            return None;
        };
        let rs = tempo_core::fd_rules::ruleset(
            station.log.event,
            tempo_core::fd_rules::CURRENT_RULES_YEAR,
        );
        let (qso_pts, powered) = rs
            .scoring
            .qso_and_powered(&station.log, self.settings.fd_power_mult);
        let bonus = rs.bonus_points(&self.settings.fd_bonuses);
        Some((qso_pts, powered, bonus))
    }

    /// Manually add a contact to the logbook (the UI "Log QSO" button). Adds in
    /// memory and appends to the ADIF file if a log path is set.
    pub fn log_qso(&mut self, mut rec: QsoRecord) {
        // Duplicate-contact guard — the LAST line of defense against logging the same
        // QSO twice. The per-Station `qso_logged` latch only blocks a re-log within ONE
        // Station, and `call_station_ctx` resets it on every invocation, so one contact
        // seeded repeatedly (a double-click, or a companion auto-replying to a single
        // RR73/73 decode across cycles) would otherwise append identical records AND
        // enqueue an upload for each — the phantom-triplicate bug. Skip when the same
        // call+band+mode is already in the log within a few minutes: tight enough that
        // it can never block a legitimate later re-work (that's minutes/hours later, or
        // a different band), coarse enough to catch a burst of identical seeds. Covers
        // every path into the log (auto, cockpit button, manual Logbook, companion).
        const DEDUP_WINDOW_SECS: u64 = 300;
        let is_dup = self.station.logbook.records().iter().any(|r| {
            tempo_core::message::same_call(&r.call, &rec.call)
                && r.band.eq_ignore_ascii_case(&rec.band)
                && r.mode.eq_ignore_ascii_case(&rec.mode)
                && rec.when_unix.abs_diff(r.when_unix) <= DEDUP_WINDOW_SECS
        });
        if is_dup {
            return;
        }
        // Resolve the DXCC entity (country) if the record doesn't already carry one
        // — so manually-logged contacts get a country too, not just auto-QSOs.
        if rec.country.is_none() {
            if let Some(resolve) = &self.station.dxcc_resolve {
                rec.country = resolve(&rec.call);
            }
        }
        // Same treatment for the US state, and for the same reason: it is inferred from the
        // callsign, never exchanged on the air, and the needs/WAS side can ONLY learn it from
        // the logged record. Without this the auto-log path wrote `state: None` forever, so a
        // worked state could never enter `worked_states` and NewState (tier 60 — the lead pill
        // and the row colour) re-fired on every poll. An operator-supplied state always wins.
        if rec.state.is_none() {
            if let Some(resolve) = &self.station.state_resolve {
                rec.state = resolve(&rec.call, rec.grid.as_deref());
            }
        }
        // Tag with the current POTA/SOTA activation (your side) if one is set and the
        // record doesn't already carry one — so the contact exports with the right
        // MY_SIG/MY_SOTA_REF and counts toward your activation.
        if let Some((program, reference)) = &self.station.activation {
            if rec.ota.my_ref.is_none() {
                rec.ota.my_program = Some(program.clone());
                rec.ota.my_ref = Some(reference.clone());
            }
        }
        // Hunter side: a pending one-click hunt tags THIS contact with the
        // activator's park/summit (SIG/SIG_INFO / SOTA_REF) when the call
        // matches, then clears ON SUCCESS (an already-tagged record must not
        // consume the pend). Expired pends are dropped, never applied — an
        // activation is over within hours; stamping a park on an unrelated
        // same-call contact tomorrow would be a fabricated hunter credit.
        if let Some((program, reference, call, at)) = &self.station.pending_hunt {
            if now_unix_secs().saturating_sub(*at) > HUNT_TTL_SECS {
                self.station.pending_hunt = None;
            } else if tempo_core::message::same_call(&rec.call, call) && rec.ota.their_ref.is_none()
            {
                rec.ota.their_program = Some(program.clone());
                rec.ota.their_ref = Some(reference.clone());
                self.station.pending_hunt = None;
            }
        }
        if let Some(path) = &self.station.log_path {
            if let Err(e) = Logbook::append(path, &rec) {
                eprintln!("tempo: failed to append to logbook: {e}");
            }
        }
        self.push_to_hrd(&rec);
        self.push_to_n1mm(&rec);
        // Queue for the shell's connector auto-upload worker (QRZ/ClubLog/eQSL).
        // This is THE funnel: auto-logged FT8 QSOs, cockpit logs, and manual
        // Logbook entries all pass through here, so the Settings auto-upload
        // toggles can never be dead for one path again.
        if self.station.pending_uploads.len() >= 256 {
            self.station.pending_uploads.pop_front();
        }
        self.station.pending_uploads.push_back(PendingUpload {
            rec: rec.clone(),
            legs: upload_legs::ALL,
            attempts: 0,
        });
        self.station.logbook.add(rec);
        self.station.refresh_worked_index();
    }

    /// Push a logged QSO to Ham Radio Deluxe Logbook over its QSO-Forwarding UDP
    /// listener — one raw ADIF record per datagram, with the operator's station
    /// fields appended (HRD attributes the contact with these). This is the same
    /// standard network path WSJT-X / JTAlert / N1MM use (HRD's default port 2333);
    /// we deliberately use it rather than writing HRD's database. Best-effort and
    /// fire-and-forget — HRD need not be running, and a failed send is ignored.
    fn push_to_hrd(&self, rec: &QsoRecord) {
        if !self.settings.hrd_logging {
            return;
        }
        let mut adif = tempo_core::logbook::adif_record(rec);
        let tag = |name: &str, val: &str| -> String {
            let v = val.trim();
            if v.is_empty() {
                String::new()
            } else {
                format!("<{}:{}>{} ", name, v.len(), v)
            }
        };
        let station = format!(
            "{}{}",
            tag("STATION_CALLSIGN", &self.settings.mycall),
            tag("MY_GRIDSQUARE", &self.settings.mygrid),
        );
        // Insert the station fields before the record terminator.
        if let Some(pos) = adif.find("<EOR>") {
            adif.insert_str(pos, &station);
        }
        // OFF-THREAD like push_to_n1mm below, and for the same reason: the target
        // is a hostname, so `send_to`'s ToSocketAddrs resolves DNS INLINE — and
        // this runs from log_qso under the engine mutex. A slow resolver froze
        // every snapshot poll AND the slot loop for the timeout: a missed T/R
        // boundary right after logging.
        let addr = self.settings.hrd_udp_addr.trim().to_string();
        std::thread::spawn(move || {
            if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
                let _ = sock.send_to(adif.as_bytes(), &addr);
            }
        });
    }

    /// Broadcast a logged QSO as an N1MM `<contactinfo>` datagram — the STANDING
    /// output, on for every logged contact rather than only during a Field Day
    /// event. Live map/dashboard consumers (OpenHamClock, GridTracker) plot each
    /// contact from it as it is logged.
    ///
    /// Deliberately here, beside `push_to_hrd`, and NOT on the shell's upload
    /// worker: that worker DRAINS `pending_uploads`, and a record it drains with
    /// every HTTP connector switched off is consumed and dropped — so routing this
    /// through it would mean an operator running N1MM as their only integration
    /// silently lost the log-first-configure-later guarantee (enable QRZ an hour
    /// in, and this session's QSOs would no longer upload). Firing from the funnel
    /// itself also puts the packet on the wire in the same instant as the HRD
    /// datagram, which is how operators compare the two in Wireshark.
    ///
    /// Spawned, so a hostname that needs DNS can never stall a log write while the
    /// engine mutex is held. Best-effort: a dashboard that is not running is not an
    /// error worth interrupting an operator over, so it goes to stderr like the
    /// Field Day emitter's failures.
    fn push_to_n1mm(&self, rec: &QsoRecord) {
        let Some(addr) = n1mm_broadcast_target(&self.settings) else {
            return;
        };
        let contact = n1mm_contact_for(rec, &self.settings.mycall);
        std::thread::spawn(move || {
            if let Err(e) = tempo_net::n1mm::send_contact(&addr, &contact) {
                eprintln!("tempo: N1MM broadcast failed: {e}");
            }
        });
    }

    /// Build the ADIF upload payload (header + the records at `indices`) for TQSL.
    pub fn lotw_upload_adif(&self, indices: &[usize]) -> String {
        let recs = self.station.logbook.records();
        let mut out = tempo_core::logbook::adif_header();
        // In ADIF-location mode, stamp each record with STATION_CALLSIGN + MY_GRIDSQUARE so TQSL
        // can sign from the ADIF (no named `-l` location). Named-location mode is byte-identical
        // to before (no MY_ fields), so existing uploads are unchanged.
        let adif_loc = self.settings.lotw_use_adif_location;
        let call = self.settings.mycall.clone();
        let grid = self.settings.mygrid.clone();
        for &i in indices {
            if let Some(r) = recs.get(i) {
                if adif_loc {
                    out.push_str(&tempo_core::logbook::adif_record_with_station(
                        r, &call, &grid,
                    ));
                } else {
                    out.push_str(&tempo_core::logbook::adif_record(r));
                }
            }
        }
        out
    }

    /// Mark every QSO currently counted as un-uploaded to LoTW (the "Upload to LoTW (N)" set)
    /// as ALREADY on LoTW — the operator's declaration that an imported legacy log was uploaded
    /// through another tool (Ham2K Polo, TQSL, etc.). Stamps them `Accepted` so they drop out of
    /// the unsent count and a bulk upload never re-pushes them. Returns how many were marked.
    pub fn mark_lotw_uploaded_all(&mut self) -> usize {
        let indices = self.station.lotw_unsent_indices();
        let n = indices.len();
        if n > 0 {
            self.station.stamp_lotw_upload(
                &indices,
                tempo_core::logbook::UploadOutcome::Accepted,
                now_unix_secs() as i64,
                Some("marked already on LoTW".into()),
            );
        }
        n
    }

    /// Set the operating mode. `spec`: `chat` | `qso-run` | `qso-monitor` |
    /// `fieldday-run` | `fieldday-sp`.
    pub fn set_mode(&mut self, spec: &str) -> Result<(), String> {
        let mycall = self.settings.mycall.clone();
        let mygrid = self.settings.mygrid.clone();
        // A RUN mode auto-calls CQ and arms TX below — starting one on a tier that
        // cannot transmit produces a CQ run that reports overs it never sends. The
        // passive specs (monitor / S&P / chat) are decode views that only transmit
        // on a further operator action, and each of THOSE actions carries its own
        // check, so they stay reachable for listening.
        if matches!(spec, "qso-run" | "fieldday-run") {
            self.require_tx_capable()?;
            // A beacon transmits, so require_tx_capable passes — but there is no
            // exchange to run, so a CQ run on WSPR is nonsense. Refuse explicitly
            // rather than arm a sequencer with nothing to sequence.
            self.require_qso_capable()?;
        }
        // The Field Day exchange goes ON THE AIR — refuse to start the mode on a
        // blank class/section rather than transmit somebody else's defaults.
        if spec.starts_with("fieldday")
            && (self.settings.fd_class.trim().is_empty()
                || self.settings.fd_section.trim().is_empty())
        {
            return Err(
                "Set your Field Day class and ARRL/RAC section in Settings first —                  they are the exchange you transmit"
                    .to_string(),
            );
        }
        let exch = Exchange::new(&self.settings.fd_class, &self.settings.fd_section);
        let band = self.settings.band.clone();
        self.mode = match spec {
            "chat" => Mode::Chat,
            "qso-run" => Mode::Qso {
                station: Box::new({
                    let mut s = QsoStation::calling_cq(&mycall, &mygrid);
                    s.cq_call_cap = self.cq_call_budget();
                    if let Some(d) = &self.cq_dir {
                        // Directed run: "CQ DX <me> <grid>" instead of plain.
                        s.override_next(Msg::Cq {
                            de: mycall.clone(),
                            grid: mygrid.clone(),
                            dir: d.clone(),
                        });
                    }
                    s
                }),
                running: true,
            },
            "qso-monitor" => Mode::Qso {
                station: Box::new(QsoStation::monitoring(&mycall, &mygrid)),
                running: false,
            },
            "fieldday-run" => Mode::FieldDay {
                station: Box::new({
                    let mut st = FieldDayStation::running(&mycall, &mygrid, exch, &band);
                    st.log.event =
                        tempo_core::fieldday::FdEvent::from_code(&self.settings.fd_event);
                    st
                }),
                running: true,
            },
            "fieldday-sp" => Mode::FieldDay {
                station: Box::new({
                    let mut st = FieldDayStation::search_and_pounce(&mycall, &mygrid, exch, &band);
                    st.log.event =
                        tempo_core::fieldday::FdEvent::from_code(&self.settings.fd_event);
                    st
                }),
                running: false,
            },
            other => return Err(format!("unknown mode {other:?}")),
        };
        // Carry the operator's RR73/RRR preference into a fresh QSO sequencer.
        if let Mode::Qso { station, .. } = &mut self.mode {
            station.confirm_with_rrr = self.settings.prefer_rrr;
        }
        // Restore the durable FD journal into the fresh station — entering Field
        // Day used to build an EMPTY log, so a restart (or a run↔S&P toggle)
        // mid-event dropped every contact and the next exit flush overwrote the
        // backup with just the new session. Rows older than 4 days are a
        // previous event's journal and self-expire.
        if let Mode::FieldDay { station, .. } = &mut self.mode {
            if let Some(path) = &self.station.fd_log_path {
                station.log.merge_adif(
                    &std::fs::read_to_string(path).unwrap_or_default(),
                    now_unix_secs().saturating_sub(4 * 86_400),
                );
            }
        }
        // Mode↔tier invariant: free-text Chat needs an FT1/DX1 waveform — its
        // chunked free text does NOT fit FT8/FT4's 13-char packer (it would
        // silently transmit nothing). Snap to FT1 when entering Chat on a
        // structured tier, so Chat can never silently fail.
        //
        // Gated on `is_chat()` rather than an Ft8|Ft4 list: the list was written when
        // those were the only non-chat tiers, and the six receive-only ones slipped
        // straight through it. Chat on those was worse than on FT8 — composing armed
        // TX, the bubble said "sending", and it could never transmit at all. Same
        // outcome for FT8/FT4 as before (neither is `is_chat`).
        if matches!(self.mode, Mode::Chat) && !self.app.tier().is_chat() {
            self.set_tier(Tier::TempoFast);
        }
        // Running modes (Call CQ / Field-Day run) auto-call CQ, so entering one
        // must ENABLE TX like WSJT-X's run start — otherwise, after a prior Halt Tx
        // or a tripped watchdog, "Call CQ" would silently transmit nothing.
        // `cq_running` marks a CQ-run session so a completed QSO returns to CQ.
        self.cq_running = matches!(spec, "qso-run" | "fieldday-run");
        if self.cq_running {
            self.set_tx_enabled(true);
            // WSJT-X-snappy: Call CQ keys the CURRENT period when the over still
            // fits (the radio loop's immediate path owns the room/parity checks) —
            // without this a CQ clicked 1 s into your own period silently waited a
            // full T/R cycle.
            self.immediate_tx = true;
        }
        self.reset_tx_watchdog();
        self.tx_queue.clear();
        self.broadcast_queue.clear();
        self.own_tx.clear();
        // A new QSO (or mode change) starts a fresh auto-log window.
        self.qso_logged = false;
        self.qso_report_sent = None;
        self.qso_start_unix = None; // a fresh QSO stamps its own start time
                                    // Clear stale receive-side IR-HARQ buffers so a new exchange never
                                    // joint-combines with retransmissions from a previous one.
        self.harq_reset_locked();
        Ok(())
    }

    /// Switch the top-level operating AREA atomically (avoids a set_tier+set_mode
    /// command race). `"dx"` = FT8/FT4 structured operating; `"msg"` = FT1/DX1
    /// free-text (Chat). Idempotent + non-disruptive: it only changes the tier or
    /// mode when they're incompatible with the area, so re-entering an area never
    /// resets a live QSO or chat.
    pub fn set_area(&mut self, area: &str) {
        match area {
            "msg" => {
                // MSG = FT1/DX1 free-text paradigm. Remember which structured
                // tier we're leaving so a dx(FT4) → msg → dx round-trip returns
                // to FT4 (it used to default back to FT8 — the tier was lost).
                if !matches!(self.app.tier(), Tier::TempoFast | Tier::TempoDeep) {
                    self.last_dx_tier = Some(self.app.tier());
                    self.set_tier(self.last_msg_tier.unwrap_or(Tier::TempoFast));
                }
                if !matches!(self.mode, Mode::Chat) {
                    let _ = self.set_mode("chat");
                }
            }
            _ => {
                // DX = the structured/digital paradigm. Restore the remembered DX
                // tier (FT4 survives a trip through msg); default FT8. Only pull out
                // of Chat — leave a running QSO alone.
                //
                // The test is "are we on a CHAT tier", the exact mirror of the msg
                // branch above — not an Ft8|Ft4 list. That list predated the six
                // receive-only tiers, which are digital-area tiers too, so entering
                // Digital on Q65 used to file Q65 away as the remembered CHAT tier
                // and yank the operator to FT8. The next trip through msg then
                // restored Q65 as the chat tier — a chat client running on a mode
                // that cannot transmit.
                if self.app.tier().is_chat() {
                    self.last_msg_tier = Some(self.app.tier());
                    self.set_tier(self.last_dx_tier.unwrap_or(Tier::Ft8));
                }
                if matches!(self.mode, Mode::Chat) {
                    let _ = self.set_mode("qso-monitor");
                }
            }
        }
    }

    /// Initiate a directed QSO with a **specific** station (e.g. the operator
    /// double-clicked a heard station to work them, or a logger sent a WSJT-X
    /// Reply). Enters QSO mode answering `dxcall`, resets the TX watchdog, clears
    /// queues, and opens a fresh auto-log window (so the contact logs once when
    /// the sequence completes).
    pub fn call_station(&mut self, dxcall: &str) {
        self.call_station_ctx(dxcall, None, None, None, None);
    }

    /// Toggle Skip Tx1 (WSJT-X parity) — a session-only preference (see the field).
    pub fn set_skip_tx1(&mut self, on: bool) {
        self.skip_tx1 = on;
    }

    /// Current Skip Tx1 state (for the snapshot / UI to reflect the toggle).
    pub fn skip_tx1(&self) -> bool {
        self.skip_tx1
    }

    /// As [`call_station`], but pre-seeds the DX station's grid (e.g. the operator
    /// typed it into the directed-call entry, or it came from a roster/spot) so
    /// the contact logs with a grid even when we never decode the DX's own grid.
    ///
    /// [`call_station`]: Engine::call_station
    pub fn call_station_with_grid(&mut self, dxcall: &str, dxgrid: Option<&str>) {
        self.call_station_ctx(dxcall, dxgrid, None, None, None);
    }

    /// The faithful WSJT-X "double-click to work" entry point. `reply_msg` is the
    /// exact decoded line the operator double-clicked (with its `reply_snr`);
    /// WSJT-X parses *that* message to choose the next Tx, so clicking a station
    /// that already answered resumes the QSO mid-sequence instead of restarting at
    /// the grid. When `reply_msg` is `None` (a roster/spot/typed call with no
    /// specific line), we fall back to the most recent decode from `dxcall`
    /// addressed to us this slot. Working a station also **enables TX** (so a
    /// double-click transmits even if TX was toggled off) — matching WSJT-X.
    pub fn call_station_ctx(
        &mut self,
        dxcall: &str,
        dxgrid: Option<&str>,
        reply_msg: Option<&str>,
        reply_snr: Option<i32>,
        dx_freq: Option<f32>,
    ) {
        let mycall = self.settings.mycall.clone();
        let mygrid = self.settings.mygrid.clone();

        // Guard: a double-click on our OWN line (our CQ / our TX echo in the
        // band activity) must not start a QSO with ourselves — stock WSJT-X
        // ignores it; without this we'd key up calling our own callsign.
        if tempo_core::message::same_call(dxcall, &mycall) {
            return;
        }
        // Same reasoning on a receive-only tier: this arms TX, picks answer parity
        // and moves RX to the DX — a whole QSO the radio can never take part in.
        // JT65 decodes real QSO traffic into the band feed, so double-clicking a
        // heard station is the natural thing to try there; refusing here leaves it a
        // listening view instead of a QSO that never gets answered.
        //
        // Beacons are refused for the opposite reason: they CAN transmit, but a
        // WSPR spot is a propagation report, not a station calling. Answering one
        // is not a thing the mode does.
        let t = self.app.tier();
        if self.tier_is_rx_only(t) || self.tier_is_beacon(t) {
            return;
        }

        // Resolve the message we're answering → (parsed Msg, the report we send =
        // the SNR we decoded the DX at). Prefer the clicked line; recover its SNR
        // from this slot's decodes if the caller didn't pass one; else fall back to
        // the latest decode from dxcall addressed to me.
        let mut context_slot: Option<u64> = None;
        let context: Option<(Msg, i32)> = match reply_msg {
            Some(text) if !text.trim().is_empty() => {
                // Recover SNR (and the decode's SLOT, for answer parity) from the
                // rolling history — newest matching row wins.
                let hist = self
                    .decode_history
                    .iter()
                    .rev()
                    .find(|(_, d)| d.message.eq_ignore_ascii_case(text.trim()));
                context_slot = hist.map(|(s, _)| *s);
                let snr = reply_snr
                    .or_else(|| hist.map(|(_, d)| d.snr))
                    .unwrap_or(0)
                    .clamp(-30, 49);
                // Clicked text already aged out of the ring? The latest reply from
                // this station still beats the unrelated `last_decode_slot` below.
                if context_slot.is_none() {
                    context_slot = self.latest_reply_from(dxcall, &mycall).map(|(_, _, s)| s);
                }
                Some((Msg::parse(text), snr))
            }
            _ => self
                .latest_reply_from(dxcall, &mycall)
                .map(|(m, snr, slot)| {
                    context_slot = Some(slot);
                    (m, snr)
                }),
        };

        let mut station = QsoStation::start(
            &mycall,
            &mygrid,
            dxcall,
            context.as_ref().map(|(m, s)| (m, *s)),
            self.settings.prefer_rrr,
            self.skip_tx1,
        );
        // Hound (DXpedition) QSOs end on the Fox's RR73 with NO parting 73 —
        // a 73 would land in the Fox's own segment (see qso.rs quiet_finish).
        station.quiet_finish = matches!(
            self.settings.special_op,
            crate::settings::SpecialOp::Hound | crate::settings::SpecialOp::SuperHound
        );
        // Pre-seed the operator/spot grid only if we didn't capture one from the
        // message we're answering.
        if station.dxgrid.is_none() {
            station.dxgrid = dxgrid
                .map(|g| g.trim().to_uppercase())
                .filter(|g| !g.is_empty());
        }
        // The station we just built may ALREADY have a report armed — that's the CQ case: a
        // caller answered our CQ, `Station::observe` advanced CallingCq→AwaitRoger and queued
        // the report, and only then did the operator click to work them. Capture it here,
        // because our only other capture point (`observe_modes`) runs off a decode fold, and
        // no decode folds during our own TX slot. Miss it and the next fold has already
        // advanced the QSO to RR73, so the report is gone and the contact logs with a blank
        // RST_SENT. Operator report, 2026-07-25: "the log seems to have it right in almost
        // every case" — this is the "almost".
        let opening_report = report_in(station.outgoing());
        self.mode = Mode::Qso {
            station: Box::new(station),
            running: true,
        };
        // A directed call is S&P, not a CQ run: a completed QSO does NOT auto-resume
        // calling CQ.
        self.cq_running = false;
        // Set our TX period to the OPPOSITE of the period the DX was decoded in, so
        // we transmit while they listen — WSJT-X's auto-Tx-1st/2nd on double-click.
        // Use the ANSWERED decode's slot (the history row we resolved context
        // from); when the click carried no message (roster/station-card/spot click)
        // and the DX is calling CQ (not addressed to me), `context_slot` is None —
        // fall back to the DX's OWN latest decode (their CQ), like the chat path.
        // `last_decode_slot` is whatever slot decoded most recently from ANYONE and
        // is 50/50 WRONG on a two-cycle band — a wrong parity transmits while the
        // DX transmits: never heard, and reads as "waits multiple cycles".
        // (A decode's audio is from the slot before its ingest slot, so the
        // opposite of the DX's period is exactly `ingest_slot % 2`.) This is a DERIVED
        // parity (the sequencer answering), not a manual pick — keep auto-cycle on.
        if let Some(s) = context_slot
            .or_else(|| self.latest_decode_slot_from(dxcall))
            .or(self.last_decode_slot)
        {
            self.apply_cycle_parity(s % 2 == 0);
        }
        // Move our RX onto the DX's audio frequency (and TX with it, unless Hold Tx
        // Freq is on) — WSJT-X's double-click-to-work behavior. set_rx_offset clamps to
        // the passband and drags TX along when Hold is off. Ignore a non-positive offset
        // (absent/malformed) so we don't yank the rig to the band edge.
        if let Some(hz) = dx_freq.filter(|h| *h > 0.0) {
            self.set_rx_offset(hz);
        }
        // Hound rule (stock DXpedition mode): initial calls to the Fox must be
        // ABOVE 1000 Hz — the Fox listens there; 300–900 is the Fox's own
        // segment. Spread by callsign so a pileup doesn't stack at one offset.
        if matches!(
            self.settings.special_op,
            crate::settings::SpecialOp::Hound | crate::settings::SpecialOp::SuperHound
        ) && self.tx_offset_hz < 1000.0
        {
            // Per-SESSION spread (stock randomizes each launch): salt the call
            // hash so two hounds never collide deterministically every event,
            // and stay within the 2900 Hz offset ceiling (% 1900, not 2000 —
            // the clamp was stacking ~5% of calls at exactly 2900).
            let spread: u32 = self
                .settings
                .mycall
                .bytes()
                .fold(self.station.session_salt, |a, b| {
                    a.wrapping_mul(31).wrapping_add(b as u32)
                });
            self.set_tx_offset(1000.0 + (spread % 1900) as f32);
        }
        // Working a station implies transmit (stock "Double-click on call sets
        // Tx enable", default on): enable TX (also clears a tripped watchdog +
        // resets the continuous-TX count) so the auto-sequencer actually keys
        // even if TX was toggled off. Option off = the operator arms manually.
        if self.settings.double_click_sets_tx {
            self.set_tx_enabled(true);
        }
        // Any working click cancels a deferred after-73 disable from the
        // PREVIOUS contact — it must not disarm the new one a tick later.
        self.pending_tx_disable = false;
        // Key the current period immediately (if it's our parity and the over
        // fits) instead of waiting a full cycle for the next boundary.
        self.immediate_tx = true;
        self.tx_queue.clear();
        self.broadcast_queue.clear();
        self.qso_logged = false;
        self.qso_report_sent = opening_report;
        self.qso_start_unix = Some(now_unix_secs()); // working a station starts the QSO clock
        self.harq_reset_locked(); // fresh exchange: drop stale receive-side IR-HARQ state
    }

    /// Drop everything answer-context derives from: the decode history (message
    /// match + report SNR) and the parity slot. Called on band QSY / tier switch,
    /// where stale rows would answer a station that isn't in this activity.
    fn clear_decode_context(&mut self) {
        self.decode_history.clear();
        self.last_decode_slot = None;
        self.early_seen = None;
        // Advance the decode-context generation so any decode still in flight on the
        // worker (built in the OLD context) lands stale and is dropped — its slot
        // indices / AP context are meaningless after the switch.
        self.decode_epoch = self.decode_epoch.wrapping_add(1);
    }

    /// `tempo_fast::harq_reset()` serialized behind the decoder lock, so it can never race
    /// the worker thread's in-flight decode (which uses the same process-global FT1
    /// IR-HARQ buffers). Every engine-thread reset goes through here; the decode
    /// path's own reset already runs under the lock in [`run_decode_job`].
    fn harq_reset_locked(&self) {
        let _g = self.source.lock().unwrap();
        tempo_fast::harq_reset();
    }

    /// Clear the CW decode transcript + reset the stream decoder. A QSY (band change,
    /// commanded retune, or working a spot) moves onto a different signal, so the old
    /// CW copy is stale (operator report: the CW decode area lingered across a QSY).
    fn clear_cw_decode(&mut self) {
        self.ai_cw_text.clear();
        self.ai_cw_status.clear();
        self.ai_cw_audio.clear();
        self.ai_cw_fed = 0;
        self.cw_stream.clear();
    }

    /// The best resume point from `dxcall` addressed to `mycall` in this slot, for
    /// a roster/spot/typed call that carried no specific clicked line. Among this
    /// slot's decodes from the DX to me, pick the FURTHEST-progressed one (grid <
    /// report < R-report) so we resume at the right step deterministically, and
    /// IGNORE terminal messages (RRR/RR73/73): a roster click on a station that
    /// just signed off is a fresh call (grid start), not a lone 73. Matching is
    /// base-call/case-insensitive so portable calls still resolve.
    fn latest_reply_from(&self, dxcall: &str, mycall: &str) -> Option<(Msg, i32, u64)> {
        // Resume rank — only the non-terminal, addressed-to-me steps qualify.
        fn resume_rank(m: &Msg) -> Option<u8> {
            match m {
                Msg::Grid { .. } => Some(1),
                Msg::Report { .. } => Some(2),
                Msg::RReport { .. } => Some(3),
                _ => None, // CQ / RRR / RR73 / 73 / other → not a resume point
            }
        }
        // Search the HISTORY (several T/R cycles), not just the last slot's
        // decodes — the on-air bug: a roster click moments after the caller's
        // message scrolled out fell back to Tx1 and re-sent the grid.
        self.decode_history
            .iter()
            .filter_map(|(slot, d)| {
                let m = Msg::parse(&d.message);
                let from_dx = m.sender().map(|s| same_call(s, dxcall)).unwrap_or(false);
                let to_me = m.addressee().map(|a| same_call(a, mycall)).unwrap_or(false);
                if from_dx && to_me {
                    resume_rank(&m).map(|r| (r, m, d.snr.clamp(-30, 49), *slot))
                } else {
                    None
                }
            })
            // Newest first, then highest resume rank within the same slot.
            .max_by_key(|(r, _, _, slot)| (*slot, *r))
            .map(|(_, m, snr, slot)| (m, snr, slot))
    }

    /// Confirm-and-log a QSO held by the prompt-to-log popup. `rec` is the
    /// (possibly operator-edited) record; logs it and clears the pending hold.
    pub fn confirm_pending_log(&mut self, rec: QsoRecord) {
        self.pending_log = None;
        self.persist_pending_qso(); // clears the journal — it's in the log now
        self.log_qso(rec);
    }

    /// Discard a QSO held by the prompt-to-log popup without logging it.
    pub fn discard_pending_log(&mut self) {
        self.pending_log = None;
        self.persist_pending_qso(); // clears the journal — the operator said no
    }

    /// Operator "Resend": re-arm the current QSO message so a stalled (or just
    /// not-yet-copied) step transmits again on the next TX slot. No-op outside an
    /// active QSO. Also resets the TX watchdog so re-sending counts as activity.
    pub fn qso_resend(&mut self) {
        if let Mode::Qso { station, .. } = &mut self.mode {
            station.resend();
            self.reset_tx_watchdog();
        }
    }

    /// Operator "Log QSO" (the inline cockpit button / WSJT-X Log QSO): log the
    /// active QSO's DX contact now, from the sequencer's captured call/grid/report,
    /// even if the sequence hasn't reached the final 73. Marks the QSO logged so it
    /// isn't also auto-logged on completion. Returns false outside a QSO / no DX.
    pub fn log_current_qso(&mut self) -> bool {
        // Write-once: if this contact was already logged (manual double-click, or
        // it auto-logged on completion), don't log it again.
        if self.qso_logged {
            return false;
        }
        let (dxcall, dxgrid, rx_report) = match &self.mode {
            Mode::Qso { station, .. } => match &station.dxcall {
                Some(c) => (c.clone(), station.dxgrid.clone(), station.rx_report),
                None => return false,
            },
            _ => return false,
        };
        // Don't create a report-LESS record: a QSO isn't a contact until at least one
        // signal report has been exchanged (WSJT-X only logs after the report exchange).
        // Refuse a manual log while still calling CQ / awaiting the first report.
        if rx_report.is_none() && self.qso_report_sent.is_none() {
            return false;
        }
        let rec = self.qso_record(dxcall, dxgrid, rx_report);
        self.qso_logged = true;
        self.qso_start_unix = None; // contact logged — the next QSO stamps a fresh start
                                    // Respect prompt-to-log just like auto-log: hold for the confirm popup
                                    // instead of writing silently, so manual + auto behave the same.
        if self.settings.prompt_to_log {
            self.pending_log = Some(rec);
            self.persist_pending_qso(); // a real contact — journal it before the popup waits
        } else {
            self.log_qso(rec);
        }
        true
    }

    /// Operator in-QSO free text (WSJT-X Tx5): override the next transmission with
    /// `text`, addressed to the current DX station if one is known (`<DX> <ME>
    /// <text>`) else sent verbatim. No-op outside an active QSO or for empty text.
    pub fn qso_freetext(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let mycall = self.settings.mycall.clone();
        if let Mode::Qso { station, .. } = &mut self.mode {
            let msg = match &station.dxcall {
                Some(dx) => tempo_core::message::Msg::Other(format!("{dx} {mycall} {text}")),
                None => tempo_core::message::Msg::Other(text.to_string()),
            };
            station.override_next(msg);
            self.reset_tx_watchdog();
            // No record_own_tx here: QSO free text goes out as a SINGLE non-chunk frame,
            // which poll_tx already records (parse_chunk is None) — recording it here too
            // double-showed the row.
        }
    }

    // ----- command delegates ----------------------------------------------

    /// One-click resend of a terminal (no-ack / abandoned) chat bubble — same policy
    /// side-effects as a fresh send (parity, CQ-run pause, journal), fresh cycle budget.
    pub fn resend_message(&mut self, peer: &str, ack_id: Option<char>) -> bool {
        if !self.app.resend_message(peer, ack_id) {
            return false;
        }
        self.reset_tx_watchdog();
        if self.chat_cq {
            self.chat_cq_paused = true;
        }
        self.persist_pending_msgs(); // the re-queued entry must survive a restart
                                     // (Conversations are persisted by the shell's periodic exporter.)
        true
    }

    pub fn send_message(&mut self, peer: &str, text: &str) {
        // Same reasoning as `broadcast`: the entry point must refuse, not the wave
        // builder. Chat should not be reachable on these tiers at all now that
        // `set_mode("chat")` snaps off any non-chat tier, but this path is also
        // driven by companion datagrams, so it carries its own check.
        if self.tier_is_rx_only(self.app.tier()) {
            return;
        }
        self.reset_tx_watchdog();
        // Smart cycle (FT8-style): when auto and not in a CQ run, answer a heard station
        // on the OPPOSITE T/R parity to the one we decoded them in, so we key while they
        // listen (the 50/50-collision fix). A decode's audio is from the slot before its
        // ingest slot, so the opposite of the DX's period is `ingest_slot % 2`.
        if self.tx_cycle_auto && !self.cq_running {
            if let Some(s) = self.latest_decode_slot_from(peer).or(self.last_decode_slot) {
                self.apply_cycle_parity(s % 2 == 0);
            }
        }
        // Sequential CQ-run policy: answering/chatting pauses the run (never
        // interleave CQ into a QSO); it auto-resumes after directed traffic idles
        // (poll_tx) or via the operator's Resume.
        if self.chat_cq {
            self.chat_cq_paused = true;
        }
        self.app.send_message(peer, text);
        self.persist_pending_msgs(); // held message survives a crash from this instant
                                     // Sending a directed reply IS an explicit "put this on the air" action, exactly like
                                     // broadcast() — so arm TX, or the queued message never transmits without a separate
                                     // Enable-Tx click and the operator sees it sit in "waiting" forever (half of the
                                     // "reply won't send" bug). It stays store-and-forward gated: poll_tx still only
                                     // releases it once the peer is present (Roster::is_active), so arming here cannot put
                                     // a message on the air for an absent peer — it just opens the path for when they are.
        self.arm_tx_now();
        // NOTE: the own-TX band-activity row is recorded when the message is actually
        // RELEASED on the air (poll_tx, via due_frames' first-release body) — not here at
        // compose time, which showed a phantom row for a store-and-forward message held for
        // an absent peer. The outbound conversation bubble still appears immediately.
    }

    /// Queue an open broadcast (FT8-style "to all") of `text`. Unlike directed
    /// store-and-forward, a broadcast sends on the next TX slots in Chat mode
    /// *without* requiring any present recipient. The free text is prefixed
    /// `DE <MYCALL> ` and chunked so receivers can attribute it to us. Also
    /// echoed into the band-activity feed (conversation `*`) as outbound.
    /// Record a clean own-TX row for the band-activity decode feed — the LOGICAL chat
    /// message, NOT the raw wire chunk frames. Ring-capped. (poll_tx skips own-TX for
    /// free-text frames so they don't double / show "A13DE KD9TAW".)
    fn record_own_tx(&mut self, text: String) {
        const OWN_TX_RING: usize = 30;
        self.own_tx.push_back(OwnTx {
            text,
            freq_hz: self.tx_offset_hz,
            when_unix: now_unix_secs(),
        });
        if self.own_tx.len() > OWN_TX_RING {
            self.own_tx.pop_front();
        }
    }

    /// Send the delivery ACKs we owe: a 1-frame RR73 to each sender whose directed
    /// message we just fully received. Queued onto the chat broadcast path; the receiver
    /// (the original sender) sees it and marks the message delivered, ending the resend.
    fn send_pending_acks(&mut self, slot: u64) {
        // An ACK owed more than this many slots ago is stale — the sender has long since
        // moved on or purged the message (resend caps at ~8 attempts). Skip it so ACKs
        // banked while TX was off don't all flush as a rude burst when TX comes on.
        const ACK_TTL_SLOTS: u64 = 30;
        let mycall = self.settings.mycall.trim().to_uppercase();
        let owed = self.app.take_pending_acks();
        if mycall.is_empty() {
            return; // can't form a roger without our own call; drop (won't recur)
        }
        // One structured RR73 per peer this poll. A free-text id-tagged ACK can't fit (two
        // callsigns + id > the 13-char free-text frame), so the ACK stays the structured
        // RR73 (calls hashed into 77 bits) and the sender matches FIFO. The recovery win is
        // that we RE-owe on every heard resend (the inbox records owed ACKs on each
        // completion), so a lost RR73 is re-sent on the sender's next resend.
        let mut seen = std::collections::HashSet::new();
        for (peer, _id, incurred) in owed {
            if slot.saturating_sub(incurred) > ACK_TTL_SLOTS {
                continue; // stale — don't ack a conversation that's long over
            }
            if !seen.insert(peer.clone()) {
                continue;
            }
            let ack = Msg::Rr73 {
                to: peer,
                de: mycall.clone(),
            }
            .to_text(); // "<peer> <mycall> RR73"
            self.broadcast_queue.push_back(ack);
        }
    }

    pub fn broadcast(&mut self, text: &str) {
        if text.trim().is_empty() {
            return; // nothing to say — never put a bare "DE <MYCALL>" carrier on the air
        }
        // Receive-only tier: nothing to broadcast on. This is reachable WITHOUT any
        // Nexus UI action — a companion app's FreeText-with-send datagram (JTAlert,
        // GridTracker) lands straight here — so the guard has to be in the engine,
        // not only in the buttons. Without it the text is queued, echoed into the
        // band feed as if sent, and silently dropped at the wave builder.
        if self.tier_is_rx_only(self.app.tier()) {
            return;
        }
        self.reset_tx_watchdog();
        let mycall = self.settings.mycall.clone();
        let full = tempo_core::inbox::broadcast_text(&mycall, text);
        let sane = tempo_core::text::sanitize(&full);
        if sane.chars().count() <= tempo_core::text::FREETEXT_MAX {
            // Fast-path: a short line ("DE <CALL> 73"-length) fits ONE native free-text
            // frame — no chunk header, so it goes out in a single cycle (half the latency
            // of the 2-chunk path) and reads clean. The receiver's reassembler passes a
            // non-chunk frame straight through; poll_tx skips its own-TX record for
            // broadcast frames (the clean body is recorded once below).
            self.broadcast_queue.push_back(sane);
        } else {
            let id = (b'A' + self.broadcast_id) as char;
            self.broadcast_id = (self.broadcast_id + 1) % 26;
            for f in tempo_core::text::chunk(&full, id) {
                self.broadcast_queue.push_back(f);
            }
        }
        self.app.note_broadcast(text);
        self.record_own_tx(text.to_string()); // band activity shows the clean message
                                              // Broadcasting IS an explicit "put this on the air" action — arm TX so the
                                              // canned band macros key on the next over without a separate Enable-Tx click.
        self.arm_tx_now();
    }
    /// The current worked/active peer — drives the `!` CW macro + logging. `None` if unset.
    pub fn active_peer(&self) -> Option<String> {
        self.app.active_peer().map(str::to_string)
    }

    pub fn select_peer(&mut self, peer: &str) {
        self.app.select_peer(peer);
    }
    /// Archive (hide) a conversation thread (the recents-list hide affordance).
    pub fn archive_conversation(&mut self, peer: &str) {
        self.app.archive_conversation(peer);
        self.persist_pending_msgs(); // drop_for removed this peer's queued messages
    }
    /// Export conversation threads for on-disk persistence (bounded per thread).
    pub fn export_conversations(&self) -> Vec<crate::dto::Conversation> {
        self.app.export_conversations()
    }
    /// Restore persisted conversation threads at startup (chat history across restarts).
    pub fn load_conversations(&mut self, convs: Vec<crate::dto::Conversation>) {
        self.app.load_conversations(convs);
    }
    /// Clear the active peer (map/roster deselect).
    pub fn clear_peer(&mut self) {
        self.app.clear_peer();
    }
    pub fn set_tier(&mut self, tier: Tier) {
        // Tier switch changes the slot period (FT8 15 s / FT4 7.5 s) — slot indices
        // from the old tier are meaningless for answer parity. Flush the context.
        self.clear_decode_context();
        // Captured BEFORE the swap: the halt below asks what we are LEAVING.
        let from = self.app.tier();
        self.app.set_tier(tier);
        // ⭐ ANY TIER SWITCH WHILE AN OVER IS IN FLIGHT STANDS TRANSMIT DOWN.
        //
        // This was gated on `tier_is_rx_only`, which stopped covering the case that
        // matters once Q65/FST4/WSPR/FST4W gained transmitters — all four declare
        // `tx: true`, so none of them is rx-only and the halt never ran. The audio
        // service only cuts an in-flight over when `tx_enabled` goes false, so PTT
        // stayed held to `tx_until_ms`, sized from the whole generated waveform.
        //
        // On FT8 that window was ~13 s of a 15 s slot and reads as "the over
        // finishes". On WSPR it is ~93% of every interval and on FST4W-1800 it is
        // essentially always: the operator clicks off the tier, the UI and the pill
        // follow instantly, and the radio keeps sending the OLD mode for up to
        // 25 minutes. The retune to the new tier queues behind it too.
        //
        // ⚠️ SCOPED TO THE TIERS WHERE IT IS NEW. Halting on ANY armed tier switch
        // also disarms an FT8→FT4 change, which is shipped gold-standard behaviour
        // and not mine to alter — `tier_switch_keeps_message_layer` caught exactly
        // that. FT8/FT4/FT1/DX1 keep their existing behaviour (their overs are
        // 13 s or less, and trailing one out reads as "the over finishes"); the
        // four new tiers, whose overs run from 26 s to 30 MINUTES, stand down.
        let leaving_a_long_over = self.tx_enabled
            && !matches!(
                from,
                Tier::Ft8 | Tier::Ft4 | Tier::TempoFast | Tier::TempoDeep
            );
        if self.tier_is_rx_only(tier) || leaving_a_long_over {
            self.halt_tx();
            self.cq_running = false;
            if matches!(self.mode, Mode::Qso { .. } | Mode::FieldDay { .. }) {
                self.mode = Mode::Qso {
                    station: Box::new(QsoStation::monitoring(
                        &self.settings.mycall,
                        &self.settings.mygrid,
                    )),
                    running: false,
                };
            }
        }
        // Point the native signal source at the selected mode (FT1/FT8/FT4). DX1
        // decodes via its own robust path in `ingest`, so the source is left as-is.
        // In Companion mode the source is the upstream WSJT-X stream — never
        // clobber it; the tier still updates for TX / display.
        if self.source_kind == SourceKind::Native {
            if let Some(kind) = self.tier_mode_kind(tier) {
                // Swap the boxed decoder UNDER the lock (waits for any decode in
                // flight) so the stable serialization mutex is preserved and no
                // job can be reading the old mode as it's replaced.
                *self.source.lock().unwrap() = Box::new(NativeSource::from_kind(kind));
            }
        }
        // WSJT-X-style: switching the mode moves the rig to the NEW mode's dial for the
        // CURRENT band (FT8 14.074 → FT4 14.080; FT1 → the native plan), honoring any
        // Settings ▸ Frequencies overrides — so you land where the new mode actually calls
        // instead of being left on the old tier's frequency. In-band QSY (band unchanged),
        // so the decode context is preserved. Companion mode tracks the upstream rig, so
        // skip it there. (`band_plan()` is tier-aware off the just-set tier.)
        if self.source_kind == SourceKind::Native {
            let band = self.settings.band.clone();
            let plan = self.band_plan();
            // ⭐ FALL BACK TO THE MODE'S PRIMARY CHANNEL when the current band has
            // none. The tier-aware plans are not all all-band: MSK144 and Q65 are
            // VHF+ only, FST4/FST4W are 2200/630/160 m only. `find()` therefore
            // MISSES for the common case — an operator sitting on 20 m FT8 clicks
            // MSK144 — and the whole QSY used to be a silent no-op: tier, decoder
            // and slot clock all moved to MSK144 while the radio stayed on 14.074.
            // The mode then decodes an FT8 watering hole forever and reads as
            // broken, which is how this was found. WSJT-X moves you to the new
            // mode's own frequency in exactly this situation; the first plan entry
            // is that mode's calling channel (MSK144 → 6 m 50.260, FST4 → 2200 m).
            //
            // This is a BAND CHANGE, not an in-band nudge — `clear_decode_context`
            // at the top of this function has already flushed the stale context.
            let target = plan
                .iter()
                .find(|c| c.band.eq_ignore_ascii_case(&band))
                .or_else(|| plan.first())
                .cloned();
            if let Some(ch) = target {
                if (ch.dial_mhz - self.settings.dial_mhz).abs() > 0.0005 {
                    self.set_frequency(ch.dial_mhz, &ch.band, &ch.mode);
                }
            }
        }
    }

    /// The active RX signal source.
    pub fn source_kind(&self) -> SourceKind {
        self.source_kind
    }

    /// Switch the RX signal source between the native engine and a WSJT-X/JTDX/MSHV
    /// companion stream over UDP. Companion binds [`Settings::companion_addr`];
    /// returns `Err` (and stays on the previous source) if the socket can't bind.
    pub fn set_source(&mut self, kind: SourceKind) -> Result<(), String> {
        // Source switch invalidates the decode context — in particular a stale
        // Native early-pass marker would silently filter the first Companion
        // boundary's decodes, and history/parity belong to the old stream.
        self.clear_decode_context();
        match kind {
            SourceKind::Native => {
                let mode_kind = self
                    .tier_mode_kind(self.tier())
                    .unwrap_or(modes::ModeKind::TempoFast);
                *self.source.lock().unwrap() = Box::new(NativeSource::from_kind(mode_kind));
            }
            SourceKind::Companion => {
                let addr = &self.settings.companion_addr;
                let sock = WsjtxUdpSource::bind(addr)
                    .map_err(|e| format!("Can't listen on {addr} for WSJT-X UDP: {e}"))?;
                *self.source.lock().unwrap() = Box::new(sock);
            }
        }
        self.source_kind = kind;
        // Record the choice so the shell can persist it (restored at startup).
        self.settings.source = kind;
        Ok(())
    }

    /// Enable/disable the Chat-mode presence beacon ("CQ <call> <grid>"). Off by
    /// default so the app starts passive (hunt-and-pounce). When on, it announces
    /// presence periodically on the operator's TX slots.
    pub fn set_beacon(&mut self, on: bool) {
        self.settings.beacon = on;
    }

    /// Whether the presence beacon is currently enabled.
    pub fn beacon_enabled(&self) -> bool {
        self.settings.beacon
    }

    /// Enable/disable IR-HARQ (receive combining + TX redundancy escalation).
    /// On by default; off forces RV0-only behavior.
    pub fn set_harq_enabled(&mut self, on: bool) {
        self.settings.harq_enabled = on;
    }

    /// Whether IR-HARQ is currently enabled.
    pub fn harq_enabled(&self) -> bool {
        self.settings.harq_enabled
    }

    /// The active waveform tier (the radio loop reads this to pick the slot
    /// period + capture-window size: FT1 = 4 s, DX1 = 15 s).
    pub fn tier(&self) -> Tier {
        self.app.tier()
    }

    /// Whether the operator's identity is sufficient to transmit a STANDARD (FT8/FT4)
    /// message — a real callsign AND a valid Maidenhead grid, exactly what WSJT-X
    /// requires to build a CQ/Tx1. The command layer calls this before entering a
    /// keying FT8/FT4 mode (Call CQ / work a station) so the operator gets a concrete
    /// reason instead of a grid-less call or a silently-suppressed over. FT1/DX1
    /// free-text isn't grid-bound, so it returns `Ok`.
    pub fn structured_tx_ready(&self, needs_grid: bool) -> Result<(), String> {
        if !matches!(self.tier(), Tier::Ft8 | Tier::Ft4) {
            return Ok(());
        }
        if !tempo_core::message::is_callsign(&self.settings.mycall) {
            return Err("Set your callsign in Settings before transmitting FT8/FT4.".into());
        }
        // CQ/QSO messages carry the grid (Tx1); Field Day exchanges ("3A IL") do NOT,
        // so only require a grid when the message actually sends one.
        if needs_grid && !tempo_core::message::is_valid_grid(&self.settings.mygrid) {
            return Err(
                "Set your Maidenhead grid (e.g. EN52) in Settings before transmitting FT8/FT4."
                    .into(),
            );
        }
        Ok(())
    }

    /// Update the next-slot-boundary countdown (ms) shown in the UI TopBar.
    pub fn set_slot_timing(&mut self, next_slot_ms: u64) {
        self.app.set_slot_timing(next_slot_ms);
    }

    /// Stop transmitting now: drop any queued outbound frames + broadcasts and
    /// clear the TX indicator. Wired to the WSJT-X UDP "HaltTx" control so a
    /// logger / JTAlert can stop Tempo keying.
    pub fn halt_tx(&mut self) {
        // Stop transmitting AND stay stopped: disable TX so the auto-sequencer
        // doesn't immediately re-arm on the next slot (WSJT-X "Halt Tx" also
        // unchecks Enable Tx). Drop any tune carrier and queued audio too.
        self.tx_enabled = false;
        self.tuning = false;
        // Drop any LATCHED key intent too (manual PTT-lock / a broker client holding
        // T 1): Stop TX and a radio switch must release PTT, not merely mask it until
        // TX re-enables — else re-arming TX re-keys the radio with nobody holding it.
        self.manual_ptt = false;
        self.broker_ptt = false;
        // A pending snappy-TX request dies with the halt — otherwise the loop
        // consumes it against a disabled TX and a later re-arm has lost it.
        self.immediate_tx = false;
        // So does a deferred after-73 disable: TX is already off.
        self.pending_tx_disable = false;
        self.pending_cw_id = false; // halt = silence; no parting CW ID either
                                    // Cut any in-progress CW: the global Stop TX and the external UDP HaltTx
                                    // both route here, and CW keyed over CAT (`send_morse`) or a WinKeyer's
                                    // buffer keeps sending until explicitly aborted. Clearing cw_queue and
                                    // arming cw_abort makes the audio loop issue rig.stop_morse()/wk.clear()
                                    // on its next tick — otherwise "Stop TX" is a no-op mid-CW.
        self.cw_queue.clear();
        self.cw_abort = true;
        // Cut any in-progress RTTY the same way: drop every queued message and arm
        // the one-shot abort so the radio loop stops the FSK keying thread / flushes
        // the AFSK audio ring and unkeys on its next tick.
        self.rtty_queue.clear();
        self.rtty_abort = true;
        self.aprs_tx_queue.clear(); // drop any queued APRS beacon so Stop TX cancels it
        self.voice_tx = None; // drop any queued voice-keyer audio too
                              // Cut any in-progress SSTV image the same way: drop the queued job and arm the
                              // one-shot abort so the radio loop drops the feed, flushes the output ring and
                              // unkeys on its next tick.
        self.sstv_tx = None;
        self.sstv_abort = true;
        self.sstv_tx_mode = None;
        self.sstv_tx_progress = None;
        self.tx_queue.clear();
        self.broadcast_queue.clear();
        self.own_tx.clear();
        self.app.set_transmitting(false);
    }

    /// Enable/disable normal slot TX. `false` = Monitor-off (transmit muted):
    /// [`Engine::poll_tx`] returns nothing and any queued audio is dropped.
    /// `true` is an operator action: it clears a tripped watchdog and resets the
    /// continuous-TX count.
    /// Arm transmit for an operator-initiated broadcast / CQ: enable TX (which cancels
    /// a deferred after-73 disable and clears a tripped watchdog) and request the snappy
    /// "first-over" so it keys THIS period when the over still fits. Mirrors how every
    /// FT8 operator action arms; preserves the launch-safety invariant (TX is only ever
    /// armed by an explicit operator action, never on startup).
    fn arm_tx_now(&mut self) {
        self.set_tx_enabled(true);
        self.immediate_tx = true;
    }

    /// Call CQ from Tempo (chat-first) — emit ONE structured `CQ <mycall> <mygrid>`
    /// frame into the broadcast queue and arm TX, staying in Chat mode (unlike
    /// [`start_cq`](Self::start_cq), which switches to the Operate QSO sequencer). The
    /// frame is the clean WSJT-X i3=1 CQ ("CQ KD9TAW EN52"), packed structurally by the
    /// encoder — NOT the chunked `DE <CALL> …` free-text broadcast that leaked chunk
    /// headers ("A12…") and dropped the grid. `dir` is an optional directed-CQ token;
    /// only WSJT-X-packable tokens stay structured (an unsupported one would fall back
    /// to free text), so callers should validate or pass `None`.
    pub fn call_cq(&mut self, dir: Option<&str>) -> Result<(), String> {
        self.require_tx_capable()?;
        self.require_qso_capable()?;
        let mycall = self.settings.mycall.trim().to_string();
        let grid = self.settings.mygrid.trim();
        if mycall.is_empty() {
            return Err("Set your callsign in Settings before calling CQ".to_string());
        }
        // A COMPOUND call (W9XYZ/P, VP2E/W9XYZ) packs only as the grid-less i3=4 CQ —
        // a grid OR a directed-CQ token would force the packer to TRUNCATED free text
        // ("CQ W9XYZ/P EN" / "CQ DX W9XYZ/P"). Mirror `qso::compound_form`: clear BOTH.
        // A standard call needs its 4-char grid (i3=1) and may carry a directed token.
        let compound = tempo_core::message::is_compound(&mycall);
        if !compound && grid.len() < 4 {
            return Err("Set your 4-character grid in Settings before calling CQ".to_string());
        }
        let (grid, dir) = if compound {
            (String::new(), String::new())
        } else {
            (
                grid.chars().take(4).collect::<String>().to_uppercase(),
                dir.map(|d| d.trim().to_uppercase()).unwrap_or_default(),
            )
        };
        let msg = Msg::Cq {
            de: mycall,
            grid,
            dir,
        };
        let text = msg.to_text(); // "CQ KD9TAW EN52" — encoder packs it as one i3=1 frame
        self.reset_tx_watchdog();
        self.broadcast_queue.push_back(text.clone()); // structured frame, no DE/chunk
        self.arm_tx_now();
        self.app.note_broadcast(&text); // echo into the "*" band feed
        Ok(())
    }

    /// The structured chat-CQ text, or None when identity is invalid (same rules as
    /// [`Self::call_cq`] — compound calls drop grid/dir). Used by the CQ run's
    /// per-slot re-send, which must never emit a malformed frame silently.
    fn chat_cq_text(&self) -> Option<String> {
        let mycall = self.settings.mycall.trim().to_string();
        let grid = self.settings.mygrid.trim();
        if mycall.is_empty() {
            return None;
        }
        let compound = tempo_core::message::is_compound(&mycall);
        if !compound && grid.len() < 4 {
            return None;
        }
        let grid = if compound {
            String::new()
        } else {
            grid.chars().take(4).collect::<String>().to_uppercase()
        };
        Some(
            Msg::Cq {
                de: mycall,
                grid,
                dir: String::new(),
            }
            .to_text(),
        )
    }

    /// Toggle the chat-native CQ RUN (Tempo "keep calling" loop). Starting queues an
    /// immediate CQ + arms TX (via [`Self::call_cq`]); while running, every idle
    /// own-parity TX slot re-sends the CQ until someone answers (then it auto-pauses;
    /// see poll_tx) or the operator stops it.
    pub fn set_chat_cq(&mut self, on: bool) -> Result<(), String> {
        if on {
            self.call_cq(None)?; // identity-validated immediate first call
            self.chat_cq = true;
            self.chat_cq_paused = false;
            self.chat_cq_unanswered = 0; // an operator (re)start refills the budget
        } else {
            self.chat_cq = false;
            self.chat_cq_paused = false;
        }
        Ok(())
    }

    /// Resume a paused CQ run now (operator override of the idle auto-resume),
    /// re-calling immediately.
    pub fn resume_chat_cq(&mut self) -> Result<(), String> {
        if !self.chat_cq {
            return Err("CQ run is not on".to_string());
        }
        self.chat_cq_paused = false;
        self.call_cq(None)
    }

    /// Chat CQ run state for the UI: "off" | "calling" | "paused".
    pub fn chat_cq_state(&self) -> &'static str {
        if !self.chat_cq {
            "off"
        } else if self.chat_cq_paused {
            "paused"
        } else {
            "calling"
        }
    }

    pub fn set_tx_enabled(&mut self, on: bool) {
        // BACKSTOP for the receive-only tiers. Every arm path funnels here, so this
        // is where "armed but unable to transmit" is made unrepresentable — the UI
        // reads `tx_enabled`, so letting it latch true is exactly what made the app
        // claim it was transmitting. Note the `immediate_retune` below: arming also
        // sends the rig a mode/dial assert, so an ungated arm reaches CAT even on a
        // tier that can never key. DISARMING is always honoured.
        if on && self.tier_is_rx_only(self.app.tier()) {
            return;
        }
        // Read-only launch: arming TX is the moment the operator commits to
        // transmitting — arm a retune NOW so the mode assert runs a tick BEFORE the
        // key on the normal FT8 path (on a slow-serial rig an assert at the key
        // instant can burn most of a slot). The key-site latch remains the backstop
        // for Tune/manual PTT, which have no arm step.
        if on {
            self.immediate_retune = true;
        }
        if !on {
            self.broker_ptt = false; // a TX kill switch drops a foreign key too
        }
        // ANY operator arm cancels a deferred after-73 disable from the PREVIOUS
        // contact — every arm path funnels here (Enable-Tx button, Call CQ,
        // double-click, Tx-slot click, UDP Reply). Without this the stale
        // one-shot fired on the next loop tick and silently undid the arm
        // (worst case: a CQ run started right after an S&P contact never keyed).
        if on {
            self.pending_tx_disable = false;
        }
        self.tx_enabled = on;
        if on {
            // Re-enabling is an operator action: clear a tripped watchdog and
            // restart the watchdog timer on the next over.
            self.tx_watchdog = false;
            self.tx_watchdog_start = None;
        } else {
            // Muting transmit also drops anything queued — including CW. Without
            // clearing cw_queue + arming cw_abort, a macro fired while Monitor is
            // off (or a CQ still draining) survives and unexpectedly keys the rig
            // the moment TX is re-enabled (tx_enabled starts false at launch), and
            // any in-flight CW keeps sending. Mirror halt_tx's CW stop.
            self.cw_queue.clear();
            self.cw_abort = true;
            // Same for RTTY: a disarm must abort the over in flight AND drop the
            // queue, so nothing keys on a later re-enable.
            self.rtty_queue.clear();
            self.rtty_abort = true;
            // Same for SSTV: a disarm aborts the image in flight and drops the job.
            self.sstv_tx = None;
            self.sstv_abort = true;
            self.sstv_tx_mode = None;
            self.sstv_tx_progress = None;
            self.tx_queue.clear();
            self.broadcast_queue.clear();
            self.app.set_transmitting(false);
        }
    }

    /// Set the TX audio drive level (0.0–1.0), clamped. The radio loop reads
    /// `settings.tx_level` each slot and applies it to the audio backend, so this
    /// takes effect live (the "Pwr" slider — trim until ALC is just zero).
    pub fn set_tx_level(&mut self, level: f32) {
        self.settings.tx_level = level.clamp(0.0, 1.0);
    }

    /// Set the RX capture gain (≥1.0 multiplier on received audio before decode). Headroom for a
    /// quiet interface; clamped to 1.0–8.0 (+18 dB). Applied live by the audio service.
    pub fn set_rx_gain(&mut self, gain: f32) {
        self.settings.rx_gain = gain.clamp(1.0, 8.0);
    }

    /// Whether normal slot TX is currently enabled.
    pub fn tx_enabled(&self) -> bool {
        self.tx_enabled
    }

    /// Consume the one-shot "key the current period now" request set by a directed
    /// call (double-click). The radio loop honors it only if the current slot is
    /// our TX parity and the whole over still fits before the next boundary.
    pub fn take_immediate_tx(&mut self) -> bool {
        let v = self.immediate_tx;
        self.immediate_tx = false;
        v
    }

    /// Non-consuming check of the one-shot immediate-TX request — the radio loop
    /// peeks first and only TAKES when the over actually fits the current slot,
    /// so a click outside the fit window isn't silently swallowed (it then fires
    /// at the next boundary instead of waiting an extra full cycle).
    pub fn peek_immediate_tx(&self) -> bool {
        self.immediate_tx
    }

    /// Consume the one-shot "apply dial + mode right now" request set whenever the
    /// operator clicks a section, works a Needed spot, or QSYs. The radio loop honors
    /// it by clearing any `mode_giveup` and re-asserting the dial + mode immediately —
    /// so a single click is always followed, even on a mode a prior attempt gave up on.
    pub fn take_immediate_retune(&mut self) -> bool {
        let v = self.immediate_retune;
        self.immediate_retune = false;
        v
    }

    /// Approximate on-air duration of one transmit over for the active tier (s) —
    /// used to decide whether a late (mid-slot) first over still fits the slot.
    pub fn tx_over_secs(&self) -> f64 {
        match self.app.tier() {
            // FT8 = 0.5 s lead-in + 12.64 s tones (the slot-positioned wave the radio
            // loop actually plays). Must include the lead-in so the "snappy first over"
            // room check doesn't admit an over that overruns the next slot by 0.5 s.
            Tier::Ft8 => 13.14,
            // FST4 / FST4W / Q65 transmit; this returns the FULL T/R period, which is conservative means
            // modes::tx_mode() returns None and the wave builder abandons the over
            // before keying. A FULL SLOT is returned rather than 0.0 so that if
            // this value is ever consulted by a fit check, the answer is "does not
            // fit" and the over is skipped — the conservative direction. Returning
            // 0.0 would make every fit check pass and rely solely on tx_mode to
            // stop it; two guards pointing the same way is cheaper than one.
            //
            // The slot is now the operator's configured period, so the guard stays
            // conservative at every period rather than only at the one the ABI used
            // to pin. FST4 and FST4W share fst4_period_s: same decoder, same clock.
            Tier::Fst4 | Tier::Fst4w => f64::from(self.settings.fst4_period_s),
            Tier::Q65 => f64::from(self.settings.q65_period_s),
            Tier::Msk144 => f64::from(self.settings.msk144_period_s),
            Tier::Jt65 => 60.0,
            Tier::Wspr => 120.0,
            Tier::TempoDeep => 12.64, // no lead-in; a safe over-estimate of the ~9.9 s frame
            // FT4 = 0.5 s lead-in + 5.04 s tones (105 sym × 576 sa @ 12 kHz). The
            // generated buffer also carries ~1.0 s of TRAILING silence — that is
            // PTT-hold padding, not airtime, and the radio loop strips it on a
            // late (mid-slot) start so the over never bleeds into the next period.
            Tier::Ft4 => 5.54,
            Tier::TempoFast => 3.55,
        }
    }

    /// Hold (or release) a steady tune carrier. While tuning, normal slot TX is
    /// suppressed (the radio loop plays a continuous f0 sine for ATU/amp tuning).
    /// Turning tuning on is an operator action and resets the watchdog count.
    pub fn set_tune(&mut self, on: bool) {
        // Tune is the one keying path that bypasses poll_tx (the loop keys PTT directly), so
        // the privilege lockout must gate it here: never arm a tune carrier outside privileges.
        self.tuning = on && self.tx_allowed();
        if self.tuning {
            self.reset_tx_watchdog();
        }
    }

    /// Whether the operator is holding a steady tune carrier (read by the loop to key PTT).
    /// Masked by privileges on READ too, so if the dial moves into a locked segment while a
    /// tune is armed, the loop stops keying immediately.
    pub fn tuning(&self) -> bool {
        self.tuning && self.tx_allowed()
    }

    /// Set the RX input audio level (0.0–1.0) shown in the UI meter. Driven by
    /// the radio loop from the backend's input meter.
    pub fn set_rx_level(&mut self, level: f32) {
        self.app.set_rx_level(level);
    }

    /// Feed the latest captured audio for the **live** waterfall, independent of
    /// the decoder. The radio loop calls this every iteration so the waterfall
    /// updates at the loop cadence from real sound-card input, rather than only
    /// once per (4 s / 15 s) RX slot. Keeps a rolling [`SPECTRUM_WINDOW`] window.
    pub fn feed_rx_audio(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        // NOTE: the WATERFALL row is no longer computed here. It used to be, which meant the
        // radio loop — the only thread that issues blocking CAT (up to 2500 ms on slow serial)
        // — was also the only producer of spectrum rows, so any CAT stall froze the waterfall.
        // The row is now produced on the rx-dsp thread from a wait-free tee of the capture
        // stream (tempo-audio: rxtap.rs / rxdsp.rs). These mode taps DELIBERATELY stay on the
        // loop: moving them would need an engine handle on that thread, and a contended lock
        // would start DROPPING CW/RTTY audio during exactly the stall this fixed.
        // Feed the longer CW-decode ring (the 4096-sample waterfall window is far too
        // short for Morse — CW_WINDOW holds several seconds so a callsign fits).
        self.cw_audio.extend_from_slice(samples);
        if self.cw_audio.len() > CW_WINDOW {
            let drop = self.cw_audio.len() - CW_WINDOW;
            self.cw_audio.drain(0..drop);
        }
        // The AI CW decoder's longer ring (15 s — the model's window) fills only while
        // the feature is on, so everyone else pays nothing.
        if self.settings.ai_cw_enabled {
            self.ai_cw_audio.extend_from_slice(samples);
            self.ai_cw_fed += samples.len() as u64;
            if self.ai_cw_audio.len() > AI_CW_WINDOW {
                let drop = self.ai_cw_audio.len() - AI_CW_WINDOW;
                self.ai_cw_audio.drain(0..drop);
            }
        } else if !self.ai_cw_audio.is_empty() {
            self.ai_cw_audio.clear();
            self.ai_cw_fed = 0;
        }
        // The RTTY/SSTV RX taps: plain drain buffers the decode threads empty on
        // their own cadence (`take_rtty_audio`/`take_sstv_audio`). Session-armed
        // only — disarmed costs one branch, nothing else.
        if self.rtty_armed {
            self.rtty_audio.extend_from_slice(samples);
            if self.rtty_audio.len() > RX_TAP_CAP {
                let drop = self.rtty_audio.len() - RX_TAP_CAP;
                self.rtty_audio.drain(0..drop);
            }
        }
        if self.aprs_arm.is_armed() {
            self.aprs_audio.extend_from_slice(samples);
            if self.aprs_audio.len() > RX_TAP_CAP {
                let drop = self.aprs_audio.len() - RX_TAP_CAP;
                self.aprs_audio.drain(0..drop);
            }
        }
        if self.sstv_armed {
            self.sstv_audio.extend_from_slice(samples);
            if self.sstv_audio.len() > RX_TAP_CAP {
                let drop = self.sstv_audio.len() - RX_TAP_CAP;
                self.sstv_audio.drain(0..drop);
            }
        }
        // Feed the streaming CW decoder (retune first, cheaply, if the operator moved the
        // marker pitch) — it keeps a persistent transcript across polls + window slides.
        self.cw_stream.retune(self.settings.cw_pitch_hz);
        self.cw_stream.push(samples);
        // And the per-QSO WAV ring — the last ~60 s of RX, captured on log when enabled.
        self.qso_audio.extend_from_slice(samples);
        if self.qso_audio.len() > QSO_WAV_WINDOW {
            let drop = self.qso_audio.len() - QSO_WAV_WINDOW;
            self.qso_audio.drain(0..drop);
        }
    }

    /// The recent receive audio (the per-QSO WAV ring) as 16-bit PCM, for writing a WAV
    /// of a logged contact. Empty before any audio has arrived.
    pub fn recent_rx_pcm(&self) -> Vec<i16> {
        self.qso_audio
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect()
    }

    /// The live CW decode the cockpit renders. The AI decoder (DeepCW) is PRIMARY: its
    /// stitched transcript is the text. The classic streaming Goertzel decoder still runs
    /// quietly underneath — it supplies the WPM estimate (the model doesn't measure speed)
    /// and the transcript fallback when the AI is off or its model is missing.
    pub fn cw_decode(&self) -> tempo_core::cw_decode::CwDecode {
        let ai_live = self.settings.ai_cw_enabled
            && (!self.ai_cw_text.is_empty() || self.ai_cw_status.is_empty());
        tempo_core::cw_decode::CwDecode {
            text: if ai_live && !self.ai_cw_text.is_empty() {
                self.ai_cw_text.clone()
            } else if self.settings.ai_cw_enabled {
                String::new() // AI on but warming up / no copy yet — show idle, not stale Goertzel
            } else {
                self.cw_stream.transcript().to_string()
            },
            wpm: self.cw_stream.wpm(),
        }
    }

    /// A full 15 s AI-CW window when the ring has filled, else `None`. Returns the
    /// window plus the ABSOLUTE sample index of its end (the stream clock the decode
    /// thread stitches with). Brief lock + copy; the decode happens off-lock.
    pub fn ai_cw_window(&self) -> Option<(Vec<f32>, u64)> {
        (self.ai_cw_audio.len() >= AI_CW_WINDOW).then(|| (self.ai_cw_audio.clone(), self.ai_cw_fed))
    }

    /// Append newly-stitched AI-CW transcript text (already deduplicated by the decode
    /// thread's absolute-time cursor). Bounded; oldest text drains off the front.
    pub fn push_ai_cw_text(&mut self, text: &str) {
        self.ai_cw_text.push_str(text);
        const CAP: usize = 1500;
        if self.ai_cw_text.len() > CAP {
            let cut = self.ai_cw_text.len() - CAP;
            // Drain on a char boundary.
            let cut = (cut..self.ai_cw_text.len())
                .find(|&i| self.ai_cw_text.is_char_boundary(i))
                .unwrap_or(0);
            self.ai_cw_text.drain(0..cut);
        }
    }

    /// AI CW decoder status line for the cockpit ("listening…", "model not installed"…).
    pub fn set_ai_cw_status(&mut self, s: &str) {
        self.ai_cw_status = s.to_string();
    }

    /// Toggle the AI CW decoder (persisted setting; the decode thread + ring follow it).
    pub fn set_ai_cw_enabled(&mut self, on: bool) {
        self.settings.ai_cw_enabled = on;
        if !on {
            self.ai_cw_text.clear();
            self.ai_cw_status.clear();
            self.ai_cw_fed = 0;
        }
    }

    /// Clear the streaming CW decoder's accumulated transcript (the cockpit's Clear button).
    pub fn cw_clear(&mut self) {
        self.cw_stream.clear();
        self.cw_sent.clear();
        self.ai_cw_text.clear();
    }

    /// Wideband CW skim of the recent receive audio: every distinct keyed signal across
    /// the 300–1500 Hz CW passband, each as (pitch, text, WPM). The multi-signal sibling
    /// of [`Self::cw_decode`].
    pub fn cw_skim(&self) -> Vec<tempo_core::cw_decode::SkimHit> {
        tempo_core::cw_decode::skim_cw(&self.cw_audio, tempo_fast::SAMPLE_RATE, 300, 1500, 50)
    }

    // --- RTTY RX (armed decoder on the RX audio path; decode runs in the
    // tempo-audio `rttyrx` thread). RX ONLY — no TX path touches any of this. ---

    /// Arm/disarm the RTTY RX decoder. Session-only (never persisted, so the app
    /// never launches armed). Arming starts a fresh transcript; disarming keeps
    /// the transcript readable but stops the audio tap immediately.
    pub fn set_rtty_armed(&mut self, on: bool) {
        if on && !self.rtty_armed {
            self.rtty_chars.clear();
            self.rtty_afc_hz = 0.0;
            self.rtty_afc_locked = false;
        }
        if !on {
            self.rtty_audio.clear();
        }
        self.rtty_armed = on;
    }

    /// Whether the RTTY RX decoder is armed (read by the decode thread's gate).
    pub fn rtty_armed(&self) -> bool {
        self.rtty_armed
    }

    /// Drain the armed RTTY audio tap (12 kHz mono since the last take). The
    /// decode thread calls this under a brief lock; the demod runs off-lock.
    pub fn take_rtty_audio(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.rtty_audio)
    }

    /// Arm/disarm the APRS RX decoder by an EXPLICIT operator act. Session-only (never persisted).
    /// Disarming stops the audio tap immediately but keeps the heard list readable.
    ///
    /// [`AprsArm::Explicit`] is the only arm that permits an auto-ack. Passing [`AprsArm::Auto`]
    /// here would claim an operator act that never happened — use [`Engine::aprs_auto_arm`] for
    /// view entry, which applies the policy rather than trusting its caller.
    pub fn set_aprs_arm(&mut self, arm: AprsArm) {
        if arm == AprsArm::Off {
            self.aprs_audio.clear();
            // An operator who stopped the decoder has made a decision. Remember it for the rest of
            // the session so re-entering the view cannot restart it behind them.
            self.aprs_auto_arm_declined = true;
        }
        // Health counts what THIS listening session has heard, so arming starts them over —
        // otherwise a stale "0 decoded" from a previous session reads as a live fault.
        self.aprs_health = AprsHealth {
            arm,
            ..Default::default()
        };
        self.aprs_arm = arm;
    }

    /// Arm the decoder because the operator ENTERED the APRS view — receive-only, never
    /// ack-capable. Returns whether this call armed it.
    ///
    /// Only ever an upgrade from [`AprsArm::Off`]: it will not demote an explicit arm to RX-only,
    /// and it refuses outright once the operator has explicitly stopped the decoder this session.
    /// The policy lives here rather than in the UI so it survives a remount and is testable.
    pub fn aprs_auto_arm(&mut self) -> bool {
        if self.aprs_arm != AprsArm::Off || self.aprs_auto_arm_declined {
            return false;
        }
        self.aprs_health = AprsHealth {
            arm: AprsArm::Auto,
            ..Default::default()
        };
        self.aprs_arm = AprsArm::Auto;
        true
    }

    /// How the decoder was armed — the auto-ack gate, and what the cockpit shows the operator.
    pub fn aprs_arm_source(&self) -> AprsArm {
        self.aprs_arm
    }

    /// Record what the decode thread just heard: how many samples it consumed and their peak, how
    /// many HDLC frames the deframer recovered, and how many of those survived the FCS. Called on
    /// every drain while armed — including drains that carry nothing, which is the whole point.
    ///
    /// `samples == 0` records only that this drain was empty; it must NOT overwrite the level, or
    /// the ordinary case of the decode thread out-polling the radio loop reads as a dead input.
    #[allow(clippy::too_many_arguments)]
    pub fn note_aprs_rx(
        &mut self,
        samples: usize,
        audio_peak: f32,
        clipped_samples: usize,
        frames_seen: usize,
        frames_decoded: usize,
        at_unix: i64,
    ) {
        self.aprs_health.arm = self.aprs_arm;
        self.aprs_health.drains += 1;
        if samples > 0 {
            self.aprs_health.audio_peak = audio_peak;
            self.aprs_health.last_audio_unix = Some(at_unix);
        }
        // ⚠️ Frames recovered from a drain that carried SILENCE are not evidence of anything.
        // The demodulator slices noise into random bits, and the HDLC flag-hunt will eventually
        // find a 0x7E pattern in them — over minutes that is near-certain. A real AFSK burst is
        // never below the silence floor, so anything found there is the deframer locking onto
        // dither, and letting it drive an operator-facing "packets heard" claim manufactures
        // evidence for a problem that does not exist. Decodes are exempt: a frame that passed a
        // 16-bit FCS is real regardless of what the level meter said.
        let carried_signal = audio_peak >= APRS_NOISE_FLOOR_PEAK;
        if frames_seen > 0 && carried_signal {
            self.aprs_health.frames_seen += frames_seen as u64;
            self.aprs_health.last_frame_seen_unix = Some(at_unix);
            // Burst-time level, captured NOW — by the time the operator looks, the live peak is
            // the gap between packets.
            self.aprs_health.frame_peak = audio_peak;
            self.aprs_health.max_frame_peak = self.aprs_health.max_frame_peak.max(audio_peak);
            self.aprs_health.frame_clipped_samples = clipped_samples as u32;
        }
        self.aprs_health.frames_decoded += frames_decoded as u64;
        if frames_decoded > 0 {
            self.aprs_health.last_decode_unix = Some(at_unix);
        }
    }

    /// Snapshot of APRS decoder health for the cockpit poll.
    pub fn aprs_health(&self) -> AprsHealth {
        AprsHealth {
            arm: self.aprs_arm,
            // Read live rather than stamped at arm time: a radio hand-off mid-session must move
            // this, or the chip would name the radio the operator STARTED on.
            radio_name: self
                .settings
                .active_profile()
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            band_radio_count: self.settings.radios_covering(&self.settings.band),
            ..self.aprs_health.clone()
        }
    }

    /// Whether the APRS RX decoder is armed at all (read by the decode thread's gate).
    /// Deliberately source-agnostic: BOTH kinds of arm decode. Only the ack cares which.
    pub fn aprs_armed(&self) -> bool {
        self.aprs_arm.is_armed()
    }

    /// Drain the armed APRS audio tap (12 kHz mono since the last take). Called by the decode
    /// thread under a brief lock; the demod runs off-lock.
    pub fn take_aprs_audio(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.aprs_audio)
    }

    /// Record a freshly decoded APRS packet (newest last), capped at [`APRS_HEARD_CAP`]. Called by
    /// the RF decode thread and by the APRS-IS feed — one store, two inlets.
    ///
    /// **Merges duplicates rather than listing them twice.** A local station is routinely heard
    /// both ways: our own receiver decodes it, and moments later an iGate puts the same packet on
    /// APRS-IS. Appending both would double every nearby station in the list and stack two markers
    /// on the map. So a packet matching a recent one keeps the FIRST entry — which preserves when
    /// we actually heard it — and upgrades its source to [`AprsSource::Both`].
    ///
    /// This is also the only door to the iGate uplink, and it is the structural enforcement of the
    /// rule that only RF-heard packets may be gated: an [`AprsSource::Inet`] packet cannot reach
    /// the queue from here, so no later code path has to remember not to send it.
    pub fn push_aprs_heard(&mut self, heard: AprsHeard) {
        if let Some(prior) = self
            .aprs_heard
            .iter_mut()
            .rev()
            .take_while(|h| heard.at_unix - h.at_unix <= APRS_DUPE_MERGE_SECS)
            .find(|h| h.dupe_key() == heard.dupe_key())
        {
            let merged = prior.source_kind.merged(heard.source_kind);
            let newly_rf = merged != prior.source_kind && heard.source_kind == AprsSource::Rf;
            prior.source_kind = merged;
            // A packet the internet showed us first and our antenna then heard for itself is a
            // genuine RF reception, and the iGate should contribute it.
            if newly_rf {
                self.offer_aprs_uplink(&heard.raw);
            }
            // The packet log keeps one entry, but the STATION was genuinely heard again — and via
            // the other channel, which is exactly what upgrades its source tag.
            self.merge_aprs_station(&heard);
            return;
        }
        if heard.source_kind == AprsSource::Rf {
            self.offer_aprs_uplink(&heard.raw);
        }
        self.merge_aprs_station(&heard);
        self.aprs_heard.push(heard);
        if self.aprs_heard.len() > APRS_HEARD_CAP {
            let drop = self.aprs_heard.len() - APRS_HEARD_CAP;
            self.aprs_heard.drain(0..drop);
        }
    }

    /// Fold a packet into its station's state, creating the station if it is new.
    ///
    /// Expired stations are pruned and the cap enforced HERE rather than on a timer, and only when
    /// the store is actually over the cap — at feed rates a retain-per-packet would be pure waste.
    fn merge_aprs_station(&mut self, h: &AprsHeard) {
        if h.source.is_empty() {
            return;
        }
        match self.aprs_stations.get_mut(&h.source) {
            Some(st) => st.merge(h),
            None => {
                self.aprs_stations
                    .insert(h.source.clone(), AprsStation::new(h));
            }
        }
        if self.aprs_stations.len() <= APRS_STATION_CAP {
            return;
        }
        // Over the ceiling: drop anything already past its age window first, since those would not
        // have been shown anyway. (ttl 0 = keep-forever: skip straight to oldest-first eviction.)
        let ttl = self.station_ttl_min();
        if ttl > 0 {
            let cutoff = h.at_unix - i64::from(ttl) * 60;
            self.aprs_stations
                .retain(|_, st| st.last_heard_unix >= cutoff);
        }
        // Still over: evict the stations heard longest ago, which are the least useful to keep.
        while self.aprs_stations.len() > APRS_STATION_CAP {
            let Some(oldest) = self
                .aprs_stations
                .iter()
                // Timestamps are 1-second resolution, so same-second ties are the
                // NORMAL case — break on the callsign so which station vanishes
                // is stable, not HashMap order. min_by (borrowing comparator),
                // not min_by_key: a tuple key would clone every callsign per scan.
                .min_by(|(ka, sa), (kb, sb)| {
                    sa.last_heard_unix
                        .cmp(&sb.last_heard_unix)
                        .then_with(|| ka.cmp(kb))
                })
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.aprs_stations.remove(&oldest);
        }
    }

    /// The configured station age window in minutes, clamped to something sane.
    /// ZERO IS A SENTINEL: "keep stations forever" — no age filter, no age-based prune, no fade
    /// (the cap alone bounds memory). Added as a field diagnostic that earned its keep as a
    /// feature: with removal off, any station that still vanishes proves the fault is NOT the
    /// age machinery.
    fn station_ttl_min(&self) -> u32 {
        if self.settings.aprs_station_ttl_min == 0 {
            return 0;
        }
        self.settings.aprs_station_ttl_min.clamp(5, 24 * 60)
    }

    /// The station roster as of `now`, newest-heard first, with stations past the age window
    /// dropped — plus the aging thresholds themselves, so the UI's fade cannot disagree with the
    /// backend's retention.
    ///
    /// **The aging doctrine.** A station is shown while it has been heard within
    /// [`Settings::aprs_station_ttl_min`] (default 60) and starts to FADE after a third of that
    /// (20 minutes at the default). The numbers come from how APRS is actually operated: mobiles
    /// beacon every one to two minutes, fixed stations commonly every ten and as slowly as thirty,
    /// so a 60-minute window survives two missed beacons from the slowest legitimate beaconer —
    /// anything shorter reintroduces the flash for exactly those stations. The fade begins past the
    /// slowest MOBILE rhythm, so a moving station never dims while it is still beaconing, but a
    /// genuinely stale one visibly recedes instead of asserting it is still there. One setting, with
    /// the fade derived, so the two can never be configured into contradicting each other.
    pub fn aprs_stations(&self, now: i64) -> AprsStationsView {
        let ttl_min = self.station_ttl_min();
        // ttl 0 = keep forever: no cutoff at all.
        let cutoff = if ttl_min == 0 {
            i64::MIN
        } else {
            now - i64::from(ttl_min) * 60
        };
        let mut stations: Vec<AprsStation> = self
            .aprs_stations
            .values()
            .filter(|st| st.last_heard_unix >= cutoff)
            .cloned()
            .collect();
        // Same-second ties are the NORMAL case (1 s timestamps — see the
        // eviction above): without a final tiebreak the stable sort preserved
        // HashMap order and the rendered roster reshuffled between polls.
        stations.sort_by(|a, b| {
            b.last_heard_unix
                .cmp(&a.last_heard_unix)
                .then_with(|| a.call.cmp(&b.call))
        });
        AprsStationsView {
            stations,
            ttl_min,
            fade_after_min: (ttl_min / 3).max(1),
        }
    }

    /// Offer an RF-heard TNC2 line to the iGate uplink queue. No-op unless the operator has turned
    /// the uplink on. The APRS-IS thread drains this and applies the path guards, the duplicate
    /// window and the rate cap before anything reaches the network.
    fn offer_aprs_uplink(&mut self, raw: &str) {
        if !self.settings.aprs_is_uplink || raw.is_empty() {
            return;
        }
        // A bounded queue: if the feed is down, packets age out rather than growing without limit.
        if self.aprs_uplink_queue.len() >= APRS_UPLINK_QUEUE_CAP {
            self.aprs_uplink_queue.pop_front();
        }
        self.aprs_uplink_queue.push_back(raw.to_string());
    }

    /// Drain the iGate uplink queue (RF-heard TNC2 lines awaiting gating). Called by the APRS-IS
    /// thread; empty whenever the uplink is off.
    pub fn take_aprs_uplink(&mut self) -> Vec<String> {
        self.aprs_uplink_queue.drain(..).collect()
    }

    /// Record the APRS-IS feed's live state, written by the socket thread on each poll.
    pub fn set_aprs_is_status(&mut self, status: AprsIsStatus) {
        self.aprs_is_status = status;
    }

    /// Snapshot of the APRS-IS feed for the cockpit's internet status chip.
    pub fn aprs_is_status(&self) -> AprsIsStatus {
        AprsIsStatus {
            enabled: self.settings.aprs_is_enabled,
            uplink_enabled: self.settings.aprs_is_uplink,
            ..self.aprs_is_status.clone()
        }
    }

    /// Snapshot of the decoded-APRS list for the cockpit poll (newest last).
    pub fn aprs_heard(&self) -> Vec<AprsHeard> {
        self.aprs_heard.clone()
    }

    /// The up-front APRS-TX gate: every reason a beacon would be refused, checked before anything is
    /// queued so the operator learns WHY. No operating-mode gate (a beacon is a one-shot manual
    /// action on whatever the rig is set to — typically FM on 144.390); the `transmitting` check
    /// keeps it from ever keying over another mode's over.
    fn aprs_tx_gate(&self) -> Result<(), String> {
        if !self.tx_enabled {
            return Err("TX is off — enable TX first".to_string());
        }
        if !self.tx_allowed() {
            return Err(
                "TX locked — this frequency is outside your license privileges".to_string(),
            );
        }
        if self.tuning {
            return Err("Tune carrier is up — stop tuning first".to_string());
        }
        if self.app.radio.transmitting {
            return Err("Another transmission is in flight — stop it first".to_string());
        }
        Ok(())
    }

    /// Queue one APRS position beacon for transmission — an explicit operator send (the ONLY way
    /// APRS TX keys). Renders the frame to AFSK-1200 audio up front; the radio loop keys it via
    /// [`Engine::poll_aprs_tx`]. `path` is the digipeater aliases (e.g. `["WIDE1-1","WIDE2-1"]`).
    pub fn aprs_beacon(
        &mut self,
        lat: f64,
        lon: f64,
        symbol_table: char,
        symbol_code: char,
        comment: &str,
        path: &[String],
    ) -> Result<(), String> {
        use tempo_core::aprs;
        self.aprs_tx_gate()?;
        let mycall = self.settings.mycall.trim();
        if mycall.is_empty() {
            return Err("Set your callsign in Settings before beaconing".to_string());
        }
        let src = aprs::Address::parse(mycall)
            .ok_or_else(|| "Your callsign isn't a valid APRS source address".to_string())?;
        let mut digis = Vec::new();
        for p in path.iter().filter(|p| !p.trim().is_empty()) {
            digis.push(aprs::Address::parse(p).ok_or_else(|| format!("Invalid digipeater '{p}'"))?);
        }
        let frame = aprs::position_beacon(src, lat, lon, symbol_table, symbol_code, comment, digis);
        let audio = render_aprs_frame(&frame);
        if audio.is_empty() {
            return Err("Beacon rendered no audio".to_string());
        }
        self.reset_tx_watchdog();
        self.aprs_tx_queue.push_back(audio);
        Ok(())
    }

    /// Tune the rig for APRS: QSY to `dial_mhz` on 2 m in **FM simplex**, auto-routing to the radio
    /// mapped for **2 m FM** (the same routing every QSY uses — so a station with a VHF digital rig
    /// AND an FM rig lands on the FM one, not on whichever covers 2 m first). Distinct from the raw
    /// `set_frequency` the mode dropdowns use because APRS is not a Phone/Digital operating section:
    /// without this the rig kept the previous section's mode (DATA/USB) and a 2 m packet signal never
    /// demodulated. Cleared by the next QSY.
    /// ⚠️ CAPABILITY-GATED. Refuses when the radio provably cannot receive `dial_mhz` — the FTdx10
    /// report: opening the APRS cockpit auto-tuned 144.390 at an HF-only radio, the rig refused it,
    /// and the app was left showing a frequency the radio had never been on. Commanding a radio
    /// somewhere it cannot go has no upside, so the check comes BEFORE the QSY, not after the
    /// failure. Unknown coverage (no CAT, caps unreadable) still tunes — the loop's refusal
    /// handling is the backstop.
    pub fn aprs_tune(&mut self, dial_mhz: f64) -> Result<(), String> {
        // Judge the radio that would actually be USED. Under band+mode routing that is the
        // (2 m, FM) decision — `route_radio` returning Some means a DIFFERENT radio takes this
        // QSY, so the ACTIVE radio's coverage is the wrong question there; blocking on it would
        // break the exact HF+VHF setups the routing exists for. (Pure query: the one-shot
        // `route_intent` is set only after the gate passes, so a refusal cannot leave a stale
        // intent to misroute the next QSY.)
        let hands_off_to_a_vhf_radio = !self.settings.radio_pegged
            && self
                .settings
                .route_radio("2m", crate::settings::RouteMode::Fm)
                .is_some();
        if !hands_off_to_a_vhf_radio && self.rig_covers_mhz(dial_mhz) == Some(false) {
            return Err(format!(
                "This radio doesn't cover {dial_mhz:.3} MHz, so it can't receive RF APRS. \
                 RF APRS needs a VHF radio; the internet feed works without one."
            ));
        }
        // Name the mode class EXPLICITLY for the routing step. `aprs_fm` is still false here (we're
        // arriving from some other section, and set_frequency clears it anyway), so without this the
        // QSY would route on the class of the section being LEFT — sending an APRS tune to the FT8
        // radio when the operator happened to come from Operate.
        self.route_intent = Some(crate::settings::RouteMode::Fm);
        // set_frequency does the radio hand-off + dial + band and clears aprs_fm; re-arm FM after so
        // the loop commands FM (via rig_mode_effective) with simplex plumbing (via fm_repeater_config).
        self.set_frequency(dial_mhz, "2m", "FM");
        self.aprs_fm = true;
        self.immediate_retune = true;
        Ok(())
    }

    /// Tune the rig to an FM repeater: QSY to its OUTPUT on FM, with the machine's exact shift,
    /// offset magnitude and CTCSS tone, auto-routing to the radio mapped for that band on **FM**.
    ///
    /// One atomic step, and that is the point. The Program section's tune-now used to write the
    /// four FM settings fields and then QSY in a second call, which left two defects: the QSY
    /// routed on the mode class of the section being LEFT (a 2 m repeater tune started from
    /// Operate went to the FT8 radio — the same bug [`Self::aprs_tune`] exists to prevent), and
    /// between the two calls the rig could sit on the new output frequency still carrying the
    /// PREVIOUS machine's shift and tone.
    ///
    /// `shift` is `"simplex" | "plus" | "minus"`; `offset_hz` is the exact magnitude (0 = the band
    /// convention), so odd splits key the right input. `tone_hz` of 0 means no tone. An unknown
    /// shift is REFUSED rather than silently degraded to simplex: the Hamlib formatter maps
    /// anything it doesn't recognise to "no shift", which would transmit on the repeater's own
    /// output frequency the first time the operator keyed up.
    ///
    /// ⚠️ CAPABILITY-GATED like [`Self::aprs_tune`], and for the same reason — commanding a radio
    /// somewhere it cannot go has no upside, so the check comes before the QSY.
    pub fn repeater_tune(
        &mut self,
        output_mhz: f64,
        shift: &str,
        offset_hz: i64,
        tone_hz: f32,
    ) -> Result<(), String> {
        if !matches!(shift, "simplex" | "plus" | "minus") {
            return Err(format!("Unknown repeater shift '{shift}'"));
        }
        let band = crate::bandplan::band_for_dial(output_mhz)
            .ok_or_else(|| format!("{output_mhz:.4} MHz is outside the band plan"))?;
        // Judge the radio that would actually be USED — see aprs_tune for why the ACTIVE radio's
        // coverage is the wrong question when routing hands this QSY to a different rig.
        let hands_off_to_another_radio = !self.settings.radio_pegged
            && self
                .settings
                .route_radio(band, crate::settings::RouteMode::Fm)
                .is_some();
        if !hands_off_to_another_radio && self.rig_covers_mhz(output_mhz) == Some(false) {
            return Err(format!(
                "This radio doesn't cover {output_mhz:.4} MHz, so it can't work that repeater."
            ));
        }
        // The FM plumbing lands BEFORE the QSY so the radio loop's first retune after it already
        // carries this machine's shift and tone (fm_repeater_config reads these three fields).
        self.settings.phone_mode = "fm".to_string();
        self.settings.rptr_shift = shift.to_string();
        self.settings.rptr_offset_override_hz = offset_hz;
        self.settings.ctcss_tone_hz = tone_hz;
        // Name the mode class EXPLICITLY for the routing step: the operator may be arriving from
        // Program, Operate or CW, and `route_mode` would otherwise answer for the section being
        // LEFT. Set only after the gates pass, so a refusal can't leave a stale intent behind.
        self.route_intent = Some(crate::settings::RouteMode::Fm);
        // set_frequency clears the FM-channel hold (a QSY normally leaves one), so — exactly like
        // aprs_tune with aprs_fm — re-arm it after, and the loop then commands FM via
        // rig_mode_effective with this machine's shift/tone via fm_repeater_config.
        self.set_frequency(output_mhz, band, "FM");
        self.fm_channel = true;
        self.immediate_retune = true;
        Ok(())
    }

    /// Queue one APRS text message to `addressee` — an explicit operator send. Assigns a rolling
    /// line number (001..999) so the recipient can ack, and the sender can retry until acked. Same
    /// TX gate as the beacon; the path defaults to `WIDE1-1,WIDE2-1` so it digipeats like a beacon.
    pub fn aprs_send_message(&mut self, addressee: &str, text: &str) -> Result<(), String> {
        use tempo_core::aprs;
        self.aprs_tx_gate()?;
        let addressee = addressee.trim();
        if addressee.is_empty() {
            return Err("Enter a callsign to message".to_string());
        }
        let text = text.trim();
        if text.is_empty() {
            return Err("Enter a message".to_string());
        }
        // APRS caps message text at 67 chars (spec §14); reject rather than silently truncate.
        if text.chars().count() > 67 {
            return Err("APRS messages are limited to 67 characters".to_string());
        }
        let mycall = self.settings.mycall.trim();
        let src = aprs::Address::parse(mycall)
            .ok_or_else(|| "Set a valid callsign in Settings first".to_string())?;
        // Roll 001..999 (never 000 — a zero id reads as "no ack expected").
        self.aprs_msg_seq = self.aprs_msg_seq % 999 + 1;
        let id = format!("{:03}", self.aprs_msg_seq);
        let path = vec![
            aprs::Address::new("WIDE1", 1),
            aprs::Address::new("WIDE2", 1),
        ];
        let frame = aprs::message_frame(src, addressee, text, &id, path);
        let audio = render_aprs_frame(&frame);
        if audio.is_empty() {
            return Err("Message rendered no audio".to_string());
        }
        self.reset_tx_watchdog();
        self.aprs_tx_queue.push_back(audio);
        Ok(())
    }

    /// Auto-ack an incoming message addressed to us — called by the RX decode thread. An ACK is a
    /// message whose text is `ack<their-id>` sent back to `from`. Silent no-op unless the message
    /// truly targets our base callsign AND TX is enabled/allowed (we never key an ack on our own).
    pub fn aprs_auto_ack(&mut self, from: &str, addressee: &str, msg_id: &str) {
        use tempo_core::aprs;
        // ⚠️ THE AUTO-ARM GATE. An ack is an UNATTENDED transmission — nobody asked for it at the
        // moment it keys. It therefore requires two independent operator acts, and this is the
        // first: the operator armed Monitor THEMSELVES. A decoder armed merely by entering the
        // APRS view has no such act behind it and is receive-only, however the TX gates below
        // stand. Never relax this to `aprs_armed()`.
        if self.aprs_arm != AprsArm::Explicit {
            return;
        }
        if msg_id.is_empty() {
            return; // no id → sender isn't asking for an ack
        }
        // Match on base call, SSID-agnostic (the addressee field carries no SSID).
        let base = |c: &str| {
            c.split('-')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_uppercase()
        };
        let mine = base(&self.settings.mycall);
        if mine.is_empty() || base(addressee) != mine {
            return; // not addressed to us
        }
        if base(from) == mine {
            return; // never ack ourselves (a digipeated loopback)
        }
        if self.aprs_tx_gate().is_err() {
            return; // TX off / outside privileges / busy → don't auto-ack
        }
        let Some(src) = aprs::Address::parse(self.settings.mycall.trim()) else {
            return;
        };
        // The ack itself carries no id (an ack is never itself acked).
        let path = vec![
            aprs::Address::new("WIDE1", 1),
            aprs::Address::new("WIDE2", 1),
        ];
        let frame = aprs::message_frame(src, from, &format!("ack{msg_id}"), "", path);
        let audio = render_aprs_frame(&frame);
        if !audio.is_empty() {
            self.reset_tx_watchdog();
            self.aprs_tx_queue.push_back(audio);
        }
    }

    /// Pop the next queued APRS beacon audio for the radio loop to key, or `None` while any TX gate
    /// is down (Monitor off / outside privileges / a tune carrier up / another over in flight) —
    /// the queue is then HELD, so a beacon never keys unexpectedly.
    pub fn poll_aprs_tx(&mut self) -> Option<Vec<f32>> {
        if !self.tx_enabled || !self.tx_allowed() || self.tuning || self.app.radio.transmitting {
            return None;
        }
        self.aprs_tx_queue.pop_front()
    }

    /// Append freshly decoded RTTY characters + the demod's current AFC state.
    /// Called by the decode thread; the ring caps at [`RTTY_TEXT_CAP`].
    pub fn push_rtty_decode(
        &mut self,
        chars: &[tempo_core::rtty::DecodedChar],
        afc_hz: f32,
        afc_locked: bool,
    ) {
        self.rtty_chars.extend(chars.iter().copied());
        while self.rtty_chars.len() > RTTY_TEXT_CAP {
            self.rtty_chars.pop_front();
        }
        self.rtty_afc_hz = afc_hz;
        self.rtty_afc_locked = afc_locked;
        // Feed the auto-sequencer when Auto is on. In Idle the machine only
        // ACCUMULATES (the human-initiate gate), so this can never self-start.
        if self.rtty_seq.is_some() {
            self.rtty_drive(RttyOp::Feed(chars.to_vec()));
        }
    }

    /// The compact RTTY state the UI polls (`get_rtty_state`).
    pub fn rtty_state(&self) -> RttyRxState {
        let text: String = self.rtty_chars.iter().map(|c| c.ch).collect();
        // Auto-sequencer surface: the live state, the peer + their copied exchange,
        // and — only while Auto is on — a heard CQ the operator can click to answer.
        // `find_cq` SURFACES only; it never drives the machine (the human gate).
        let (auto, seq_state, peer, peer_exchange, heard_cq) = match &self.rtty_seq {
            Some(seq) => (
                true,
                seq_state_label(seq.state()).to_string(),
                seq.peer().map(|s| s.to_string()),
                seq.peer_exchange().to_vec(),
                tempo_core::rtty::seq::find_cq(&text),
            ),
            None => (false, "idle".to_string(), None, Vec::new(), None),
        };
        // Mark/space tones the demod is netted on (waterfall cursor positions) —
        // the same tone_pair the decode thread builds its filters from, so the
        // cursors and the actual decode tones can never disagree.
        let (mark_hz, space_hz) = tempo_core::rtty::tone_pair(
            self.rtty_center_hz(),
            self.rtty_shift_hz() as f32,
            self.rtty_reverse(),
        );
        RttyRxState {
            armed: self.rtty_armed,
            afc_hz: self.rtty_afc_hz,
            afc_locked: self.rtty_afc_locked,
            mark_hz,
            space_hz,
            conf: self
                .rtty_chars
                .iter()
                .map(|c| (c.confidence.clamp(0.0, 1.0) * 100.0).round() as u8)
                .collect(),
            baud: self.rtty_baud(),
            shift_hz: self.rtty_shift_hz(),
            backend: if self.rtty_fsk() { "fsk" } else { "afsk" }.to_string(),
            // Sending = an over on the air (stamped by the loop) OR messages still
            // queued behind it — the cockpit's TX indicator.
            sending: self.rtty_sending || !self.rtty_queue.is_empty(),
            keyer_error: self.rtty_keyer_error.clone(),
            text,
            auto,
            seq_state,
            peer,
            peer_exchange,
            heard_cq,
        }
    }

    /// Clear the decoded-RTTY transcript (the cockpit's Clear button). RX display
    /// only — the demodulator keeps running.
    pub fn rtty_clear(&mut self) {
        self.rtty_chars.clear();
    }

    /// Request an RX demodulator rebuild (AFC re-acquire): the decode thread drops
    /// its demod and builds a fresh one — the recovery for an acquire-then-freeze
    /// AFC frozen on the wrong neighbor. RX only; no TX path touches this.
    pub fn request_rtty_afc_reset(&mut self) {
        self.rtty_afc_reset = true;
        self.rtty_afc_hz = 0.0;
        self.rtty_afc_locked = false;
    }

    /// Take + reset the one-shot demod-rebuild request (decode-thread only).
    pub fn take_rtty_afc_reset(&mut self) -> bool {
        std::mem::take(&mut self.rtty_afc_reset)
    }

    /// The audio center (Hz) the RTTY demod nets around — the mark/space midpoint.
    /// Un-netted resolves to the nominal 2125 Hz mark (`2125 + shift/2`), so an
    /// un-netted decoder sits on today's 2125/2295 pair at any shift. RX only.
    pub fn rtty_center_hz(&self) -> f32 {
        self.rtty_center
            .unwrap_or(2125.0 + self.rtty_shift_hz() as f32 / 2.0)
    }

    /// Net the RTTY decoder onto a new audio center (Hz) — a waterfall click.
    /// Clamps into the audio passband, then rebuilds the demod around the new
    /// center the same clean acquire-then-freeze way an AFC reset does. RX-only
    /// decoder state, so this is safe during TX and needs no privilege gate.
    pub fn rtty_net(&mut self, hz: f32) {
        self.rtty_center = Some(hz.clamp(300.0, 3700.0));
        // Rebuild the demod around the new center (zeros AFC + arms the reset).
        self.request_rtty_afc_reset();
    }

    // ----- RTTY transmit — operator-initiated, mirroring the CW send path. -----
    // Launch-safety: nothing here runs on startup or on arm. The queue fills ONLY
    // via `rtty_send_text` (an explicit operator command) and the radio loop keys
    // ONLY what `poll_rtty_one` hands it, behind every TX gate.

    /// Queue RTTY text to transmit — validating every TX gate UP FRONT so a refused
    /// send tells the operator why (instead of silently holding a queue): TX must be
    /// armed (`tx_enabled`, the WSJT-X Enable-Tx discipline), the RTTY emission must
    /// sit inside license privileges at the current dial (`tx_allowed`), the RTTY
    /// section must own the rig (`operating_mode == Rtty` — this and the FT8/FT1
    /// slot sequencer are mutually exclusive by construction), no tune carrier may
    /// be up, and no foreign transmission may be in flight. Text is uppercased and
    /// filtered to the ITA2 charset, so exactly what's queued is what keys. Sending
    /// while RTTY itself is transmitting queues BEHIND the current over (type-ahead);
    /// Stop TX / halt drops the whole queue.
    pub fn rtty_send_text(&mut self, text: &str) -> Result<(), String> {
        self.rtty_tx_gate()?;
        self.rtty_enqueue(text, true)
    }

    /// The up-front RTTY TX gate: every reason a send would be refused, checked
    /// before anything is queued so the operator learns WHY (never a silent hold).
    /// Shared by the manual send path and the auto-sequencer's CQ/Answer initiate.
    fn rtty_tx_gate(&self) -> Result<(), String> {
        use crate::settings::OperatingMode;
        if self.settings.operating_mode != OperatingMode::Rtty {
            return Err("Not in the RTTY section — enter the RTTY cockpit first".to_string());
        }
        if !self.tx_enabled {
            return Err(
                "TX is off — enable TX first (Stop TX / the watchdog disarmed it)".to_string(),
            );
        }
        if !self.tx_allowed() {
            return Err(
                "TX locked — this frequency is outside your license privileges".to_string(),
            );
        }
        if self.tuning {
            return Err("Tune carrier is up — stop tuning first".to_string());
        }
        if self.app.radio.transmitting {
            return Err("Another transmission is in flight — stop it first".to_string());
        }
        // FSK line-conflict guard: the FSK DATA line and a serial PTT line must never
        // be the same physical line — PTT rides its own path (CAT PTT or the other
        // line/port), or the data bits would double as the key.
        if let Some(port) = self.rtty_fsk_port() {
            let ptt = self.settings.ptt_method.trim().to_ascii_lowercase();
            if ptt == "dtr" || ptt == "rts" {
                let ptt_port = if self.settings.ptt_serial_port.trim().is_empty() {
                    self.settings.serial_port.trim()
                } else {
                    self.settings.ptt_serial_port.trim()
                };
                if ptt_port.eq_ignore_ascii_case(&port)
                    && ptt == self.rtty_fsk_line().trim().to_ascii_lowercase()
                {
                    return Err(
                        "FSK keying and serial PTT share the same control line — give PTT its \
                         own path (CAT PTT, or the other of DTR/RTS) so the data bits can't \
                         double as the key"
                            .to_string(),
                    );
                }
            }
        }
        Ok(())
    }

    /// Uppercase + filter to the ITA2 charset, cap a single over, and enqueue.
    /// `reset_watchdog` restarts the TX-watchdog clock — TRUE for an operator send
    /// (and a genuine sequencer state advance, handled in `rtty_drive`), FALSE for
    /// an auto-over that merely repeats, so an unanswered auto-CQ still trips the
    /// watchdog ceiling. Assumes [`Engine::rtty_tx_gate`] has already passed.
    fn rtty_enqueue(&mut self, text: &str, reset_watchdog: bool) -> Result<(), String> {
        let up: String = text
            .chars()
            .map(|c| c.to_ascii_uppercase())
            .filter(|&c| tempo_core::rtty::encodable(c))
            .collect();
        if up.trim().is_empty() {
            return Err(
                "Nothing to send — RTTY carries A–Z, 0–9 and basic punctuation".to_string(),
            );
        }
        // Bound a SINGLE over: the TX watchdog is checked between queued messages
        // (poll_rtty_one), so one message must never be able to key longer than the
        // watchdog ceiling on its own. 1000 chars ≈ 2.75 min at 45.45 baud — well
        // under the 6-minute default, and far beyond any human RTTY over.
        const RTTY_MAX_SEND_CHARS: usize = 1000;
        if up.chars().count() > RTTY_MAX_SEND_CHARS {
            return Err(format!(
                "Message too long — RTTY sends are capped at {RTTY_MAX_SEND_CHARS} characters \
                 per over"
            ));
        }
        // An explicit operator send restarts the TX-watchdog clock (see poll_rtty_one).
        if reset_watchdog {
            self.reset_tx_watchdog();
        }
        self.rtty_queue.push_back(up);
        Ok(())
    }

    /// Stop RTTY: drop everything queued and abort the over in progress — the radio
    /// loop consumes the one-shot abort, stops the FSK keying thread / flushes the
    /// AFSK audio and unkeys PTT. The cockpit's Stop button (halt_tx also does this).
    pub fn rtty_stop(&mut self) {
        self.rtty_queue.clear();
        self.rtty_abort = true;
    }

    /// Pop the next queued RTTY MESSAGE for the radio loop to key, or `None` while
    /// any TX gate is down (Monitor off / outside privileges / not the RTTY section /
    /// a tune carrier up — the queue is then HELD, so nothing keys unexpectedly; the
    /// FT8/FT1 sequencer's `poll_tx` is gated off for non-Digital the same way, so
    /// the two can never key together). One message per call: the loop paces on the
    /// real bit-stream duration, so a Stop between messages drops the remainder
    /// before it reaches the rig. The wall-clock TX watchdog applies here exactly as
    /// it does to the FT8 slot — past the ceiling it trips BEFORE handing the
    /// message out: TX disarms, the queue drops, and the abort unkeys.
    pub fn poll_rtty_one(&mut self) -> Option<String> {
        use crate::settings::OperatingMode;
        if !self.tx_enabled
            || !self.tx_allowed()
            || self.tuning
            || self.settings.operating_mode != OperatingMode::Rtty
            || self.rtty_queue.is_empty()
        {
            return None;
        }
        // Wall-clock watchdog (WSJT-X semantics): timed from the last operator
        // action — every rtty_send_text resets it, so this only bites a runaway.
        let limit_secs = self.settings.tx_watchdog_min as u64 * 60;
        if limit_secs > 0 {
            let now = now_unix_secs();
            let start = *self.tx_watchdog_start.get_or_insert(now);
            if now.saturating_sub(start) >= limit_secs {
                self.tx_watchdog = true;
                self.tx_enabled = false;
                self.rtty_queue.clear();
                self.rtty_abort = true;
                return None;
            }
        }
        self.rtty_queue.pop_front()
    }

    /// Take + reset the one-shot RTTY abort flag (the loop stops the keyer, flushes
    /// queued audio and unkeys).
    pub fn take_rtty_abort(&mut self) -> bool {
        std::mem::take(&mut self.rtty_abort)
    }

    // ----- RTTY auto-sequencer — the pure `tempo_core::rtty::RttySeq` state
    // machine wired to the live TX path + logbook, gated behind the operator's
    // Auto toggle and a human CQ/Answer initiate. It NEVER transmits on launch or
    // on toggling Auto — only an explicit CQ/Answer keys up (ARRL FD 6.4). -----

    /// Turn the RTTY auto-sequencer on/off. On builds a fresh `RttySeq` from the
    /// operator's identity + the active exchange — Field Day class/section when the
    /// FD master switch is on, else casual RST/name/QTH (mirroring `set_mode`'s
    /// exchange selection). Off aborts any live session and stops TX. NEVER
    /// transmits: a session only ever starts from `rtty_auto_cq` / `rtty_auto_answer`.
    pub fn set_rtty_auto(&mut self, on: bool) {
        if on {
            let mycall = self.settings.mycall.clone();
            let seq = if self.settings.fd_active {
                let exch = [
                    ("CLASS", self.settings.fd_class.trim()),
                    ("SECTION", self.settings.fd_section.trim()),
                ];
                tempo_core::rtty::RttySeq::new(&mycall, tempo_core::rtty::seq::FIELD_DAY, &exch)
            } else {
                let exch = [
                    ("RST", "599"),
                    ("NAME", self.settings.op_name.trim()),
                    ("QTH", self.settings.op_state.trim()),
                ];
                tempo_core::rtty::RttySeq::new(&mycall, tempo_core::rtty::seq::CASUAL, &exch)
            };
            self.rtty_seq = Some(seq);
        } else {
            if let Some(seq) = self.rtty_seq.as_mut() {
                seq.abort();
            }
            self.rtty_stop();
            self.rtty_seq = None;
            self.rtty_auto_over = false;
        }
    }

    /// Operator starts an auto CQ run (a human-initiate gate). Errors if Auto is
    /// off or any TX gate is down.
    pub fn rtty_auto_cq(&mut self) -> Result<(), String> {
        if self.rtty_seq.is_none() {
            return Err("Turn on Auto first".to_string());
        }
        self.rtty_tx_gate()?;
        self.rtty_drive(RttyOp::StartCq);
        Ok(())
    }

    /// Operator answers a heard CQ — search & pounce (a human-initiate gate).
    /// Errors if Auto is off or any TX gate is down.
    pub fn rtty_auto_answer(&mut self, call: &str) -> Result<(), String> {
        if self.rtty_seq.is_none() {
            return Err("Turn on Auto first".to_string());
        }
        self.rtty_tx_gate()?;
        self.rtty_drive(RttyOp::Answer(call.to_string()));
        Ok(())
    }

    /// Operator kills the live auto session: abort the sequencer (silent), drop the
    /// TX queue + unkey the over in flight, and clear the pending-over latch.
    pub fn rtty_auto_abort(&mut self) {
        if let Some(seq) = self.rtty_seq.as_mut() {
            seq.abort();
        }
        self.rtty_stop();
        self.rtty_auto_over = false;
    }

    /// Service the auto-sequencer once per radio-loop tick (called under the engine
    /// lock, right before `poll_rtty_one`). Fires `on_tx_complete` EXACTLY ONCE per
    /// over — the instant our keyed over has fully played out (queue drained AND not
    /// sending) — then ticks the timeout clock. No-op when Auto is off.
    pub fn rtty_auto_service(&mut self) {
        if self.rtty_seq.is_none() {
            return;
        }
        if self.rtty_auto_over && !self.rtty_sending && self.rtty_queue.is_empty() {
            self.rtty_auto_over = false;
            self.rtty_drive(RttyOp::TxComplete);
        }
        self.rtty_drive(RttyOp::Tick);
    }

    /// Drive the auto-sequencer through one operation: capture the state, apply the
    /// op, and drain its actions — all inside a scoped borrow of `rtty_seq` that is
    /// dropped BEFORE the actions are applied (the borrow-checker seam). A genuine
    /// state advance resets the TX watchdog; each drained action is then applied.
    fn rtty_drive(&mut self, op: RttyOp) {
        let now = now_unix_millis();
        let (advanced, actions) = {
            let seq = match self.rtty_seq.as_mut() {
                Some(s) => s,
                None => return,
            };
            let before = seq.state();
            match op {
                RttyOp::StartCq => seq.start_cq(now),
                RttyOp::Answer(call) => seq.answer(&call, now),
                RttyOp::Feed(chars) => seq.feed(&chars, now),
                RttyOp::Tick => seq.tick(now),
                RttyOp::TxComplete => seq.on_tx_complete(now),
            }
            (seq.state() != before, seq.take_actions())
        };
        // Genuine progress resets the watchdog (WSJT-X discipline); a bare CQ-repeat
        // / AGN does NOT, so an unanswered auto-CQ still trips the ceiling.
        if advanced {
            self.reset_tx_watchdog();
        }
        for action in actions {
            self.apply_rtty_action(action);
        }
    }

    /// Apply one sequencer [`tempo_core::rtty::Action`] to the live engine.
    /// `SendText` enqueues the over WITHOUT resetting the watchdog (only a state
    /// advance does, in `rtty_drive`) and latches `rtty_auto_over`; a refused
    /// enqueue kills the session so it can't spin against a closed gate. `LogQso`
    /// writes a QSO record (honoring auto-log / prompt-to-log). `Abort` never keys.
    fn apply_rtty_action(&mut self, action: tempo_core::rtty::Action) {
        use tempo_core::rtty::Action;
        match action {
            Action::SendText(text) => {
                if self.rtty_enqueue(&text, false).is_ok() {
                    self.rtty_auto_over = true;
                } else {
                    self.set_rtty_keyer_error(Some(
                        "RTTY auto-sequencer stopped — a transmission was refused (the TX gate \
                         closed mid-QSO)."
                            .to_string(),
                    ));
                    if let Some(seq) = self.rtty_seq.as_mut() {
                        seq.abort();
                    }
                }
            }
            Action::LogQso { call, exchange } => {
                if self.settings.auto_log {
                    let rec = self.rtty_qso_record(&call, &exchange);
                    if self.settings.prompt_to_log {
                        // Hold for the operator's confirm-before-log popup.
                        self.pending_log = Some(rec);
                        self.persist_pending_qso(); // journal before the popup waits
                    } else {
                        self.log_qso(rec);
                    }
                }
            }
            Action::Abort => {}
        }
    }

    /// Build a [`QsoRecord`] for an auto-sequenced RTTY contact from the peer's
    /// copied exchange + the current band/dial/settings. Mode is always "RTTY"
    /// (award eligibility). Modeled on [`Engine::qso_record`]; the casual RST/name/
    /// QTH map to their ADIF columns, and any other exchange fields (Field Day
    /// class/section, a contest serial) ride the comment so nothing copied is lost.
    fn rtty_qso_record(&self, call: &str, exchange: &[(String, String)]) -> QsoRecord {
        let get = |key: &str| {
            exchange
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        let country = self
            .station
            .dxcc_resolve
            .as_ref()
            .and_then(|resolve| resolve(call));
        // On-air RF = dial + the TX audio offset, sideband-signed (WSJT-X convention).
        let off_mhz = self.tx_offset_hz as f64 / 1e6;
        let freq_mhz = if self.settings.sideband.eq_ignore_ascii_case("LSB") {
            self.settings.dial_mhz - off_mhz
        } else {
            self.settings.dial_mhz + off_mhz
        };
        // Exchange fields with no dedicated ADIF column (CLASS/SECTION/SERIAL) →
        // comment, so a Field Day / contest exchange survives in the log.
        let extras: Vec<String> = exchange
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "RST" | "NAME" | "QTH"))
            .map(|(_, v)| v.clone())
            .collect();
        let comment = (!extras.is_empty()).then(|| extras.join(" "));
        let now = now_unix_secs();
        QsoRecord {
            call: call.to_string(),
            grid: None,
            country,
            state: None,
            band: self.settings.band.clone(),
            freq_mhz,
            mode: "RTTY".to_string(),
            // RTTY reports are 599 by convention; the peer's copied report is rcvd.
            rst_sent: Some("599".to_string()),
            rst_rcvd: get("RST"),
            name: get("NAME"),
            qth: get("QTH"),
            comment,
            notes: None,
            tx_power: None,
            when_unix: now,
            time_off_unix: Some(now),
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: Default::default(),
            qsl_sent: Default::default(),
            credit_granted: Vec::new(),
            credit_submitted: Vec::new(),
            upload: Default::default(),
            ota: Default::default(),
        }
    }

    /// True if RTTY keys via the true-FSK serial keyline (vs the default soundcard
    /// AFSK) — also picks the rig mode (RTTY vs LSB, see `Settings::rig_mode`).
    pub fn rtty_fsk(&self) -> bool {
        self.settings.rtty_backend.eq_ignore_ascii_case("fsk")
    }

    /// The FSK keyline serial port when the RTTY backend is FSK: the dedicated
    /// `rtty_fsk_port` when set, else the CAT `serial_port` (the same fallback the
    /// RTS/DTR PTT line uses). `None` for AFSK, or when no port is configured at all.
    pub fn rtty_fsk_port(&self) -> Option<String> {
        if !self.rtty_fsk() {
            return None;
        }
        let p = self.settings.rtty_fsk_port.trim();
        let p = if p.is_empty() {
            self.settings.serial_port.trim()
        } else {
            p
        };
        (!p.is_empty()).then(|| p.to_string())
    }

    /// Which control line carries the FSK data bits ("dtr"/"rts").
    pub fn rtty_fsk_line(&self) -> String {
        self.settings.rtty_fsk_line.clone()
    }

    /// RTTY baud rate for TX + RX. Sanitized: a zero/negative stored value falls
    /// back to the 45.45 standard so a bit clock can never divide by zero.
    pub fn rtty_baud(&self) -> f64 {
        if self.settings.rtty_baud > 0.0 {
            self.settings.rtty_baud
        } else {
            45.45
        }
    }

    /// RTTY mark/space shift (Hz) for TX + RX (min 1 — a zero shift is no FSK).
    pub fn rtty_shift_hz(&self) -> u32 {
        self.settings.rtty_shift_hz.max(1)
    }

    /// Reversed mark/space sense (TX tone pair + RX demod).
    pub fn rtty_reverse(&self) -> bool {
        self.settings.rtty_reverse
    }

    /// Stamp whether the radio loop is keying an RTTY over right now (loop-only;
    /// feeds the cockpit's sending indicator via `rtty_state`).
    pub fn set_rtty_sending(&mut self, on: bool) {
        self.rtty_sending = on;
    }

    /// Record (or clear) an RTTY keyer failure — the radio loop calls this after a
    /// send (FSK port wouldn't open, rig refused PTT), so a silent no-TX has a cause.
    pub fn set_rtty_keyer_error(&mut self, e: Option<String>) {
        self.rtty_keyer_error = e;
    }

    // --- SSTV RX (armed decoder on the same tap; decode + image persistence run
    // in the tempo-audio `sstvrx` thread). RX ONLY. ---

    /// Arm/disarm the SSTV RX decoder. Session-only. Disarming drops the audio
    /// tap and any in-flight decode progress; the gallery is untouched.
    pub fn set_sstv_armed(&mut self, on: bool) {
        if !on {
            self.sstv_audio.clear();
            self.sstv_progress = None;
        }
        self.sstv_armed = on;
    }

    /// Whether the SSTV RX decoder is armed (read by the decode thread's gate).
    pub fn sstv_armed(&self) -> bool {
        self.sstv_armed
    }

    /// Drain the armed SSTV audio tap (12 kHz mono since the last take).
    pub fn take_sstv_audio(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.sstv_audio)
    }

    /// Publish (or clear) the in-flight SSTV decode progress. Decode-thread only.
    pub fn set_sstv_progress(&mut self, p: Option<SstvProgress>) {
        self.sstv_progress = p;
    }

    /// The in-flight SSTV decode progress, if an image is being received.
    pub fn sstv_progress(&self) -> Option<&SstvProgress> {
        self.sstv_progress.as_ref()
    }

    // ----- SSTV transmit — operator-initiated, mirroring the RTTY/voice-keyer send
    // path. Launch-safety: nothing here runs on startup, on arming RX, or on
    // selecting the SSTV/Phone section. `sstv_tx` fills ONLY via `sstv_send` (an
    // explicit operator command), and the radio loop keys ONLY what `poll_sstv_tx`
    // hands it, behind every TX gate. One image in flight, no queue. -----

    /// The up-front SSTV TX gate: every reason a send would be refused, checked before
    /// any image is queued so the operator learns WHY (never a silent hold). SSTV is USB
    /// **phone** audio (1200–2300 Hz), so it rides `OperatingMode::Phone` — whose
    /// `tx_allowed` arm already judges the whole SSB passband at the dial, exactly bounding
    /// the emission (no `Sstv` operating-mode variant needed). Unlike RTTY (which is
    /// mode-exclusive with every other keyer), SSTV SHARES the Phone segment with the voice
    /// keyer and live mic PTT — so mutual exclusion against those is checked explicitly here.
    ///
    /// Public (unlike `rtty_tx_gate`) because the SSTV encode runs in the command layer,
    /// OFF the engine lock — the command pre-flights this gate so a refused send (wrong
    /// frequency, TX off) fails fast BEFORE spending CPU encoding the image.
    pub fn sstv_tx_gate(&self) -> Result<(), String> {
        use crate::settings::OperatingMode;
        if self.settings.operating_mode != OperatingMode::Phone {
            return Err("Switch to Phone (USB) first — SSTV rides the phone segment".to_string());
        }
        if !self.tx_enabled {
            return Err(
                "TX is off — enable TX first (Stop TX / the watchdog disarmed it)".to_string(),
            );
        }
        if !self.tx_allowed() {
            return Err(
                "TX locked — this frequency is outside your license privileges".to_string(),
            );
        }
        if self.tuning {
            return Err("Tune carrier is up — stop tuning first".to_string());
        }
        if self.app.radio.transmitting {
            return Err("Another transmission is in flight — stop it first".to_string());
        }
        // Phone-shared TX sources: a live mic key (ours or a foreign broker client), a
        // queued/playing voice message, or a RTTY over must not overlap the image.
        if self.manual_ptt || self.broker_ptt {
            return Err("Mic PTT is held — release it first".to_string());
        }
        if self.voice_tx.is_some() {
            return Err("A voice message is transmitting — stop it first".to_string());
        }
        if self.rtty_sending || !self.rtty_queue.is_empty() {
            return Err("RTTY is transmitting — stop it first".to_string());
        }
        if self.sstv_tx.is_some() || self.sstv_sending {
            return Err("Already transmitting an image — stop it first".to_string());
        }
        Ok(())
    }

    /// Queue a fully-encoded SSTV image for transmission — validating every TX gate UP
    /// FRONT (so a refused send says why) plus a DURATION BUDGET the RTTY path doesn't
    /// need: one image is ONE continuous keyed over up to ~4.9 min (PD290), with no
    /// "between messages" for the wall-clock watchdog to bite. So the budget is enforced
    /// here, before anything keys: an image whose key-down would out-run the TX-watchdog
    /// ceiling (or the hard [`SSTV_MAX_TX_SECS`] cap) is refused UP FRONT rather than keyed
    /// then guillotined mid-image. On accept, the watchdog clock restarts (an explicit
    /// operator action, like `rtty_send_text`). `samples` are 12 kHz PCM from
    /// `tempo_sstv::encode::encode_image`; `mode_name` is the human label for the DTO.
    /// **Human-initiate only**: this is the ONLY writer of `sstv_tx`.
    pub fn sstv_send(&mut self, samples: Vec<f32>, mode_name: String) -> Result<(), String> {
        self.sstv_tx_gate()?;
        if samples.is_empty() {
            return Err("Nothing to send — the encoded image is empty".to_string());
        }
        let duration_secs = samples.len() as f64 / f64::from(tempo_fast::SAMPLE_RATE);
        // Hard cap, watchdog-independent: no legitimate SSTV image exceeds ~295 s (PD290),
        // so anything past 330 s is a bug or an abuse — refuse it outright.
        if duration_secs > SSTV_MAX_TX_SECS {
            return Err(format!(
                "Image too long — SSTV sends are capped at {SSTV_MAX_TX_SECS:.0} s of key-down"
            ));
        }
        // Duration budget vs the TX-watchdog ceiling: refuse a send that couldn't finish
        // before the wall-clock watchdog would trip (leave 15 s of head-room). The message
        // names the fix so the operator isn't left guessing.
        let ceiling_secs = f64::from(self.settings.tx_watchdog_min) * 60.0;
        if ceiling_secs > 0.0 && duration_secs + 15.0 > ceiling_secs {
            return Err(format!(
                "{mode_name} needs ≈{duration_secs:.0} s of key-down, past your TX watchdog \
                 ({} min) — raise Settings → TX watchdog or pick a faster mode",
                self.settings.tx_watchdog_min
            ));
        }
        let duration_ms = duration_secs * 1000.0;
        // An explicit operator send restarts the TX-watchdog clock (same as rtty_send_text).
        self.reset_tx_watchdog();
        self.sstv_tx_mode = Some(mode_name.clone());
        self.sstv_tx_progress = Some((0.0, duration_ms));
        self.sstv_tx = Some(SstvTxJob {
            samples,
            mode_name,
            duration_ms,
        });
        Ok(())
    }

    /// Take the queued SSTV image for the radio loop to stream, or `None` while any TX gate
    /// is down (Monitor off / outside privileges / not Phone / tuning) — the job is then
    /// HELD (not dropped), so nothing keys unexpectedly, mirroring `poll_rtty_one`'s
    /// hold-don't-drop. No wall-clock watchdog check here: the over's length is bounded UP
    /// FRONT by `sstv_send`'s budget, and the loop unkeys unconditionally at the precomputed
    /// `tx_until_ms`, so the watchdog never needs to bite mid-image.
    pub fn poll_sstv_tx(&mut self) -> Option<SstvTxJob> {
        use crate::settings::OperatingMode;
        if !self.tx_enabled
            || !self.tx_allowed()
            || self.tuning
            || self.settings.operating_mode != OperatingMode::Phone
            || self.sstv_tx.is_none()
        {
            return None;
        }
        self.sstv_tx.take()
    }

    /// Stop SSTV now: drop the queued image and abort the over in progress — the radio loop
    /// consumes the one-shot abort, drops the feed, flushes the output ring and unkeys PTT
    /// (the cockpit's Stop button; `halt_tx` and TX-disarm route here too).
    pub fn sstv_stop(&mut self) {
        self.sstv_tx = None;
        self.sstv_abort = true;
        self.sstv_tx_mode = None;
        self.sstv_tx_progress = None;
    }

    /// Take + reset the one-shot SSTV abort flag (the loop drops the feed, flushes output
    /// and unkeys).
    pub fn take_sstv_abort(&mut self) -> bool {
        std::mem::take(&mut self.sstv_abort)
    }

    /// Stamp whether the radio loop is currently streaming an SSTV image (loop-only).
    pub fn set_sstv_sending(&mut self, on: bool) {
        self.sstv_sending = on;
    }

    /// Publish the in-flight SSTV TX progress `(played_ms, total_ms)` (loop-only).
    pub fn set_sstv_tx_progress(&mut self, played_ms: f64, total_ms: f64) {
        self.sstv_tx_progress = Some((played_ms, total_ms));
    }

    /// Whether an SSTV image is queued or streaming — the cockpit's TX indicator (a queued
    /// but not-yet-keyed job counts, mirroring RTTY's `sending || !queue.is_empty()`).
    pub fn sstv_sending(&self) -> bool {
        self.sstv_sending || self.sstv_tx.is_some()
    }

    /// The mode label of the image being (or last) transmitted, for the TX DTO.
    pub fn sstv_tx_mode(&self) -> Option<&str> {
        self.sstv_tx_mode.as_deref()
    }

    /// The in-flight SSTV TX progress `(played_ms, total_ms)`, if an image is queued/sending.
    pub fn sstv_tx_progress(&self) -> Option<(f64, f64)> {
        self.sstv_tx_progress
    }

    /// Set the rig/CAT connection status the UI renders (and the Test-CAT result
    /// reads). `ok`: `None` = VOX/no CAT, `Some(true/false)` = CAT up/down.
    pub fn set_cat_status(&mut self, ok: Option<bool>, detail: String) {
        self.cat_status = (ok, detail);
    }

    /// Ask the radio loop to re-probe the current rig and refresh the CAT status
    /// (the "Test CAT" button). The loop consumes this via [`Self::take_cat_reprobe`].
    pub fn request_cat_reprobe(&mut self) {
        self.cat_reprobe = true;
    }

    /// Consume a pending re-probe request (returns true once per request).
    pub fn take_cat_reprobe(&mut self) -> bool {
        std::mem::take(&mut self.cat_reprobe)
    }

    /// Set (or clear) the sound-card error surfaced to the UI.
    pub fn set_audio_error(&mut self, err: Option<String>) {
        self.audio_error = err;
    }

    /// Set the operator's TX-slot parity live. `true` = transmit on even/"1st"
    /// slots, `false` = odd/"2nd". Read by `poll_tx` each slot. Two stations must
    /// use OPPOSITE periods to complete a QSO.
    /// Set the T/R cycle parity WITHOUT touching the auto flag (used by the smart
    /// auto-cycle and by the operator setter below).
    fn apply_cycle_parity(&mut self, even: bool) {
        self.tx_parity = if even { 0 } else { 1 };
        self.settings.tx_even = even;
    }

    /// Operator picks a fixed cycle (Tx 1st = even, Tx 2nd = odd). An explicit pick
    /// disables auto-cycle — the operator is now in manual control.
    pub fn set_tx_even(&mut self, even: bool) {
        self.apply_cycle_parity(even);
        self.tx_cycle_auto = false;
    }

    /// Toggle smart auto-cycle (FT8-style: answer on the opposite cycle of the station
    /// you reply to). On by default; turning it back on re-enables the auto pick.
    pub fn set_tx_cycle_auto(&mut self, auto: bool) {
        self.tx_cycle_auto = auto;
    }

    pub fn tx_cycle_auto(&self) -> bool {
        self.tx_cycle_auto
    }

    /// Slot of the most recent decode whose SENDER is `peer` — any frame (not just
    /// addressed-to-me), newest first. Drives the chat answer-cycle derivation.
    fn latest_decode_slot_from(&self, peer: &str) -> Option<u64> {
        self.decode_history
            .iter()
            .rev()
            .find(|(_, d)| {
                Msg::parse(&d.message)
                    .sender()
                    .map(|s| same_call(s, peer))
                    .unwrap_or(false)
            })
            .map(|(slot, _)| *slot)
    }
    /// Whether the operator transmits on even/"1st" slots.
    pub fn tx_even(&self) -> bool {
        self.tx_parity == 0
    }

    /// Set the transmit audio offset (Hz), clamped to the usable passband. Used
    /// for FT1 + DX1 TX modulation. Live — read by the next `poll_tx`.
    pub fn set_tx_offset(&mut self, hz: f32) {
        // Ceiling = the 4 kHz spectrum span (see the waterfall's HI_HZ), so the operator can
        // place a signal anywhere WSJT-X-callers do, well above the old 2.9 kHz cap.
        self.tx_offset_hz = hz.clamp(200.0, 4000.0);
        self.settings.tx_offset_hz = self.tx_offset_hz;
    }
    /// Set the receive audio offset (Hz) — the green waterfall marker. When
    /// "Hold Tx Freq" is off, the TX offset follows it (the common case).
    pub fn set_rx_offset(&mut self, hz: f32) {
        self.rx_offset_hz = hz.clamp(200.0, 4000.0);
        self.settings.rx_offset_hz = self.rx_offset_hz;
        if !self.hold_tx_freq {
            self.set_tx_offset(hz);
        }
    }
    /// Hold the TX offset fixed when the RX offset changes (WSJT-X "Hold Tx Freq").
    pub fn set_hold_tx_freq(&mut self, on: bool) {
        self.hold_tx_freq = on;
        self.settings.hold_tx_freq = on;
    }
    /// Current transmit / receive audio offsets (Hz).
    pub fn tx_offset_hz(&self) -> f32 {
        self.tx_offset_hz
    }
    pub fn rx_offset_hz(&self) -> f32 {
        self.rx_offset_hz
    }

    /// Whether the operator's license class permits transmitting at the CURRENT dial + mode.
    /// `Open` always permits. Judges the EMITTED RF, not the bare dial: for digital the signal
    /// sits at the dial + the TX audio offset (≈+1.5 kHz on USB), so a dial just below a
    /// higher-class-only edge can still emit inside it. Every TX path ANDs this in; the
    /// snapshot exposes it so the cockpit can show a lockout indicator. See `privileges.rs`.
    pub fn tx_allowed(&self) -> bool {
        use crate::settings::OperatingMode;
        let class = self.settings.license_class;
        let dial = self.settings.dial_mhz;
        let allow = |f: f64| crate::privileges::tx_allowed(class, f, self.settings.operating_mode);
        match self.settings.operating_mode {
            OperatingMode::Digital => {
                // Narrow data signal at the audio offset: ABOVE the dial on USB, BELOW on LSB.
                let off = self.tx_offset_hz as f64 / 1_000_000.0;
                let lsb = self.settings.sideband.eq_ignore_ascii_case("LSB");
                allow(if lsb { dial - off } else { dial + off })
            }
            OperatingMode::Phone => {
                // SSB occupies ~2.8 kHz above the carrier (USB) / below it (LSB). The WHOLE
                // passband must be in a privileged phone segment, so a dial within a passband
                // of a band edge can't bleed out of band. Phone sideband is band-aware (LSB <10 MHz).
                const SSB_BW: f64 = 0.0028;
                let (lo, hi) = if dial < 10.0 {
                    (dial - SSB_BW, dial)
                } else {
                    (dial, dial + SSB_BW)
                };
                allow(lo) && allow(hi)
            }
            // CW: the carrier sits at the dial.
            OperatingMode::Cw => allow(dial),
            // RTTY: judge the real mark/space RF span per keying backend (AFSK tones
            // below the LSB dial; FSK shift below the RTTY-mode dial) — both edges.
            OperatingMode::Rtty => self.rtty_emission_ok(dial),
        }
    }

    /// UDP HighlightCallsign (JTAlert): paint/clear a callsign in the decode
    /// panes. Both colors None = clear the entry.
    pub fn set_highlight(&mut self, call: &str, bg: Option<String>, fg: Option<String>) {
        let k = call.trim().to_uppercase();
        if k.is_empty() {
            return;
        }
        if bg.is_none() && fg.is_none() {
            self.highlights.remove(&k);
        } else {
            // Bounded: a chatty logger can paint thousands of calls over a
            // session, and the whole map rides every snapshot poll. Evict an
            // entry past the cap (newest paint wins). Deterministic AND
            // age-honest: evict the OLDEST insertion (callsign as the final
            // tiebreak), never an arbitrary HashMap entry and never the
            // lexicographic minimum — that one systematically blacked out
            // digit-prefixed DX calls, which sort below every letter.
            const MAX_HIGHLIGHTS: usize = 2048;
            if self.highlights.len() >= MAX_HIGHLIGHTS && !self.highlights.contains_key(&k) {
                let victim = self
                    .highlights
                    .iter()
                    .min_by(|(ka, va), (kb, vb)| va.0.cmp(&vb.0).then_with(|| ka.cmp(kb)))
                    .map(|(k, _)| k.clone());
                if let Some(old) = victim {
                    self.highlights.remove(&old);
                }
            }
            self.highlight_seq = self.highlight_seq.wrapping_add(1);
            self.highlights.insert(k, (self.highlight_seq, bg, fg));
        }
    }

    /// Inbound UDP Clear (type 3): bump the visual clear tick for the UI.
    pub fn apply_udp_clear(&mut self) {
        self.clear_tick = self.clear_tick.wrapping_add(1);
    }

    /// The operator hit Erase: queue an outbound UDP Clear so cooperating apps
    /// mirror it. `window`: 0 = Band Activity, 1 = Rx Frequency, 2 = both.
    pub fn notify_erase(&mut self, window: u8) {
        self.pending_udp_clear = Some(window.min(2));
    }

    /// Consume the queued outbound Clear (radio loop).
    pub fn take_pending_udp_clear(&mut self) -> Option<u8> {
        self.pending_udp_clear.take()
    }

    /// UDP Location (type 11): a GPS feeder updates our grid. Accepts a bare
    /// 4/6-char Maidenhead or the "GRID:XXnn[xx]" form; everything else is
    /// ignored (never let a malformed datagram corrupt the configured grid).
    pub fn apply_udp_location(&mut self, location: &str) {
        let g = location
            .trim()
            .strip_prefix("GRID:")
            .unwrap_or(location.trim())
            .trim()
            .to_uppercase();
        if tempo_core::message::is_valid_grid(&g) {
            self.settings.mygrid = g.clone();
            // Keep the displayed grid in step with what the sequencer now
            // transmits — settings alone left the UI showing the OLD grid.
            self.app.set_mygrid(&g);
        }
    }

    /// Start a CQ run, optionally DIRECTED ("DX"/"NA"/"POTA"/"TEST"/3-digit
    /// kHz). The token is validated by Msg::parse round-trip semantics at the
    /// UI layer; here it's stored verbatim (uppercased) and applied to the
    /// run's CQ — including the return-to-CQ after each pileup contact. A
    /// plain start clears a sticky token.
    pub fn start_cq(&mut self, dir: Option<&str>) -> Result<(), String> {
        self.cq_dir = dir
            .map(|d| d.trim().to_uppercase())
            .filter(|d| !d.is_empty());
        self.set_mode("qso-run")
    }

    /// WSJT-X Tx-slot click: force `text` as the next transmission to `dxcall`.
    /// Starts (or retargets) the QSO when needed; the auto-sequencer's observe
    /// still advances on the partner's matching reply, so a forced message
    /// rejoins the normal flow (Station::override_next semantics).
    pub fn override_next_tx(&mut self, dxcall: &str, dxgrid: Option<&str>, text: &str) {
        if tempo_core::message::same_call(dxcall, &self.settings.mycall) {
            return; // never a self-QSO
        }
        // Answering a decode is a TX commitment: it arms TX and asks for the
        // current period. On a receive-only tier there is no over to send, so do
        // not stage one — a queued reply the operator can see but the radio can
        // never send is the phantom this whole guard exists to prevent.
        if self.tier_is_rx_only(self.app.tier()) {
            return;
        }
        let on_dx = matches!(&self.mode, Mode::Qso { station, .. }
            if station.dxcall.as_deref().map(|c| tempo_core::message::same_call(c, dxcall)).unwrap_or(false));
        if !on_dx {
            self.call_station_ctx(dxcall, dxgrid, None, None, None);
        }
        if let Mode::Qso { station, .. } = &mut self.mode {
            station.override_next(Msg::parse(text));
        }
        self.reset_tx_watchdog();
        if self.settings.double_click_sets_tx {
            self.set_tx_enabled(true);
        }
        // Fire this period when it still fits (the snappy path).
        self.immediate_tx = true;
    }

    /// WSJT-X Split Operation: reduce a TX audio offset into the clean
    /// 1500–2000 Hz window (audio harmonics land outside the TX filter) and
    /// return the matching 500 Hz-step dial shift that keeps the RF frequency
    /// identical: `f0 = 1500 + (tx − 1500) mod 500`, `shift = tx − f0`.
    /// Inactive (raw offset, shift 0) when split is off, the tier isn't
    /// FT8/FT4, the offset is already in-window, or a cluster SPLIT-on-Work
    /// already owns the TX dial.
    fn split_reduce(&self, tx_hz: f32) -> (f32, i64) {
        use crate::settings::SplitMode;
        let t = tx_hz.round() as i64;
        if self.settings.split_mode == SplitMode::None
            || self.split_tx_mhz.is_some()
            || !matches!(self.app.tier(), Tier::Ft8 | Tier::Ft4)
            || (1500..=2000).contains(&t)
        {
            return (tx_hz, 0);
        }
        let f0 = 1500 + (t - 1500).rem_euclid(500);
        (f0 as f32, t - f0)
    }

    /// True while a cluster SPLIT-on-Work owns the TX dial (VFO B) — the audio
    /// split's teardown must not clear the rig split out from under it.
    pub fn cluster_split_active(&self) -> bool {
        self.split_tx_mhz.is_some()
    }

    /// Consume the dial shift for the over just generated (0 = none). The slot
    /// core applies it via the rig BEFORE keying PTT.
    pub fn take_tx_dial_shift(&mut self) -> i64 {
        std::mem::take(&mut self.tx_dial_shift_hz)
    }

    /// Consume the deferred "Disable Tx after sending 73" request — the radio
    /// loop calls this once the final over has fully played out (never mid-over).
    pub fn take_pending_tx_disable(&mut self) -> bool {
        std::mem::take(&mut self.pending_tx_disable)
    }

    /// One-shot: the final 73 finished and the operator wants a CW ID — the
    /// service consumes this on TX-idle and enqueues MYCALL through the normal
    /// CW keying path.
    pub fn take_pending_cw_id(&mut self) -> bool {
        std::mem::take(&mut self.pending_cw_id)
    }

    /// Reset the transmit-watchdog: clear the tripped flag and restart the wall-clock
    /// timer on the next over. Called on any operator-initiated action.
    fn reset_tx_watchdog(&mut self) {
        self.tx_watchdog = false;
        self.tx_watchdog_start = None;
    }

    /// T/R slot length (seconds) for the active tier: DX1 = 15 s; the native
    /// tiers read it from their [`modes::ModeKind`] (FT1 = 4, FT8 = 15, FT4 = 7.5).
    pub fn active_slot_secs(&self) -> f64 {
        match self.app.tier() {
            Tier::TempoDeep => tempo_core::timing::DX1_PERIOD_S,
            t => self
                .tier_mode_kind(t)
                .map(|k| k.slot_secs() as f64)
                .unwrap_or(tempo_core::timing::PERIOD_S),
        }
    }

    /// Number of int16 samples in the capture frame the active tier decodes:
    /// DX1 = its 15 s window; native tiers read it from their [`modes::ModeKind`]
    /// (FT8 = 180000, FT4 = 72576, FT1 = 48000). The radio loop sizes the RX ring
    /// + slot clock from this so a mode switch never mis-sizes the decode frame.
    pub fn active_frame_samples(&self) -> usize {
        match self.app.tier() {
            Tier::TempoDeep => tempo_fast::deep::capture_len(),
            t => self
                .tier_mode_kind(t)
                .map(|k| k.frame_samples())
                .unwrap_or(tempo_fast::NMAX),
        }
    }

    /// Samples the RX ring should CAPTURE per slot = the full T/R period. Equals
    /// [`active_frame_samples`] for FT8/FT1 (slot == decode frame) but is LARGER
    /// for FT4 (7.5 s slot > 6.048 s frame), so the ring holds the whole slot and
    /// the decoder reads its head (leading sync) instead of the amputated tail.
    ///
    /// [`active_frame_samples`]: Engine::active_frame_samples
    pub fn active_capture_samples(&self) -> usize {
        match self.app.tier() {
            Tier::TempoDeep => tempo_fast::deep::capture_len(),
            t => self
                .tier_mode_kind(t)
                .map(|k| k.capture_samples())
                .unwrap_or(tempo_fast::NMAX),
        }
    }

    /// DT-derived time-sync health: OK until we've seen decodes, then OK only
    /// while the median absolute DT of recent decodes is under the threshold.
    /// (No NTP — this is purely derived from how far off-grid heard signals are.)
    fn time_sync_ok(&self) -> bool {
        if !self.seen_decode || self.recent_dt.is_empty() {
            return true;
        }
        let mut v: Vec<f32> = self.recent_dt.iter().copied().collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = v.len() / 2;
        let median = if v.len().is_multiple_of(2) {
            (v[mid - 1] + v[mid]) / 2.0
        } else {
            v[mid]
        };
        median < DT_OK_THRESHOLD
    }

    /// Full snapshot, with mode + per-mode (QSO / Field Day) status filled in.
    pub fn snapshot(&self) -> AppSnapshot {
        let mut s = self.app.snapshot();
        // Mark roster stations worked-before (B4) from the persistent logbook, and
        // resolve each one's DXCC country (DX chasers scan the roster by country).
        //
        // Build the worked-call set ONCE (O(log)) instead of calling worked_before per station
        // (O(roster × log)). This snapshot runs under the engine mutex on the 300 ms UI poll and
        // again every slot boundary; the old multiplicative sweep held the lock long enough to
        // stall the waterfall's spectrum fetch (which needs the same lock) for 1–2 s at low CPU.
        let worked = self.station.logbook.worked_call_set();
        for st in &mut s.stations {
            st.worked = worked.contains(&st.call.to_ascii_uppercase());
            if let Some(resolve) = &self.station.dxcc_resolve {
                st.country = resolve(&st.call);
            }
            st.grid_rarity = self.station.rarity_of(st.grid.as_deref());
            st.lotw_user = self.station.lotw_user(Some(st.call.as_str()));
        }
        // Reflect transmit-enable / tuning / watchdog and the DT-derived
        // time-sync health into the radio status the UI renders.
        s.chat_cq = self.chat_cq_state().to_string();
        s.radio.tx_enabled = self.tx_enabled;
        s.radio.qso_recording = self.qso_recording;
        s.radio.tx_allowed = self.tx_allowed();
        s.radio.tuning = self.tuning;
        s.radio.tx_watchdog = self.tx_watchdog;
        s.radio.decode_depth = self.settings.decode_depth.clamp(1, 3);
        s.radio.rig_confirmed = self.rig_confirmed;
        s.radio.time_sync_ok = self.time_sync_ok();
        s.radio.cat_ok = self.cat_status.0;
        s.radio.cat_detail = self.cat_status.1.clone();
        // The radio's RX coverage, in MHz, for the UI to gate a "move the radio" control on. Empty
        // = unknown, which the UI must read as "allow" for the same fail-open reason the engine does.
        s.radio.rx_ranges_mhz = self
            .rig_rx_ranges
            .as_ref()
            .map(|r| {
                r.iter()
                    .map(|(lo, hi)| (*lo as f64 / 1_000_000.0, *hi as f64 / 1_000_000.0))
                    .collect()
            })
            .unwrap_or_default();
        s.radio.refused_dial_mhz = self.rig_refused_dial_mhz;
        s.radio.cw_wpm = self.settings.cw_wpm;
        s.radio.split_tx_mhz = self.split_tx_mhz;
        s.radio.cw_keyer = match self.settings.cw_keyer {
            crate::settings::CwKeyerBackend::Cat => "cat",
            crate::settings::CwKeyerBackend::Soundcard => "soundcard",
            crate::settings::CwKeyerBackend::WinKeyer => "winkeyer",
            crate::settings::CwKeyerBackend::Serial => "serial",
        }
        .to_string();
        s.radio.audio_error = self.audio_error.clone();
        s.radio.radio_config_warning =
            crate::settings::serial_port_conflicts(&self.settings.radios)
                .or_else(|| {
                    crate::settings::cw_key_port_conflict(
                        self.settings.cw_keyer,
                        &self.settings.cw_key_port,
                        &self.settings.radios,
                    )
                })
                .or_else(|| crate::settings::audio_device_conflicts(&self.settings.radios));
        s.radio.tx_even = self.tx_even();
        s.radio.tx_cycle_auto = self.tx_cycle_auto;
        s.radio.tr_period_secs = self.active_slot_secs();
        s.radio.beacon = self.beacon_enabled();
        s.radio.tx_offset_hz = self.tx_offset_hz;
        s.radio.rx_offset_hz = self.rx_offset_hz;
        s.radio.tx_level = self.settings.tx_level;
        // Rig read-back wins (the knob's truth); else the last commanded value.
        s.radio.rf_power = self.rig_rf_power.or(self.rf_power);
        s.radio.mic_gain = self.rig_mic_gain.or(self.mic_gain);
        s.radio.nr_level = self.rig_nr_level.or(self.nr_level);
        s.radio.agc = self.rig_agc.clone().or_else(|| self.agc.clone());
        s.radio.smeter_db = self.rig_smeter_db;
        s.radio.tx_swr = self.rig_tx_swr;
        s.radio.tx_alc = self.rig_tx_alc;
        s.radio.tx_po_w = self.rig_tx_po_w;
        s.radio.tx_comp_db = self.rig_tx_comp_db;
        s.radio.rig_mode = self.rig_mode.clone();
        s.radio.sideband_override = self.sideband_override.clone();
        // Phone sub-band the operator may legally use on the CURRENT band + class — the band-strip
        // shades it. None for no-phone-privilege / Open / off-plan bands (then the strip shows none).
        let (plo, phi) =
            crate::privileges::phone_segment(self.settings.license_class, &self.settings.band)
                .map_or((None, None), |(lo, hi)| (Some(lo), Some(hi)));
        s.radio.phone_seg_lo = plo;
        s.radio.phone_seg_hi = phi;
        // Rig DSP-func states [nb, nr, notch, comp, vox]; None = unsupported → the toggle hides.
        s.radio.nb = self.rig_funcs[0];
        s.radio.nr = self.rig_funcs[1];
        s.radio.notch = self.rig_funcs[2];
        s.radio.comp = self.rig_funcs[3];
        s.radio.vox = self.rig_funcs[4];
        s.radio.filter_width_hz = self.rig_passband;
        s.radio.rit_hz = self.rit_hz;
        s.radio.xit_hz = self.xit_hz;
        s.radio.active_vfo = if self.active_vfo_b { "B" } else { "A" }.to_string();
        s.radio.hold_tx_freq = self.hold_tx_freq;
        s.radio.clock_offset_ms = self.station.clock_offset_ms;
        s.radio.source = self.source_kind;
        s.radio.source_label = self.source.lock().unwrap().label();
        // Multi-radio switcher summaries (dual-radio). Left empty for a single-radio station (the
        // UI then renders no switcher). The active radio carries the live state we just filled into
        // `s.radio`; the others show their last-known tune (they're not connected in the active-only
        // model), so `catOk`/`smeterDb` are None for them.
        s.active_radio_id = self.settings.active_radio;
        s.radio_pegged = self.settings.radio_pegged;
        s.ai_cw = crate::dto::AiCwStatus {
            enabled: self.settings.ai_cw_enabled,
            status: self.ai_cw_status.clone(),
            text: self.ai_cw_text.clone(),
        };
        if self.settings.radios.len() > 1 {
            s.radios = self
                .settings
                .radios
                .iter()
                .map(|p| {
                    if p.id == self.settings.active_radio {
                        RadioSummary {
                            id: p.id,
                            name: p.name.clone(),
                            band: s.radio.band.clone(),
                            dial_mhz: s.radio.dial_mhz,
                            sideband: s.radio.sideband.clone(),
                            is_active: true,
                            cat_ok: self.cat_status.0,
                            smeter_db: self.rig_smeter_db,
                            transmitting: s.radio.transmitting,
                            bands: p.bands.clone(),
                        }
                    } else {
                        // Non-active radio: prefer the LIVE monitor read-back (dual-radio "both
                        // live"); fall back to the profile's last-known tune until the first read.
                        let live = self.radio_live.get(&p.id);
                        RadioSummary {
                            id: p.id,
                            name: p.name.clone(),
                            band: live
                                .and_then(|l| l.band.clone())
                                .filter(|b| !b.is_empty())
                                .unwrap_or_else(|| p.last_band.clone()),
                            dial_mhz: live.and_then(|l| l.dial_mhz).unwrap_or(p.last_dial_mhz),
                            sideband: live
                                .and_then(|l| l.sideband.clone())
                                .unwrap_or_else(|| p.last_sideband.clone()),
                            is_active: false,
                            cat_ok: live.and_then(|l| l.cat_ok),
                            smeter_db: live.and_then(|l| l.smeter_db),
                            transmitting: false,
                            bands: p.bands.clone(),
                        }
                    }
                })
                .collect();
        }
        // Coordinated-QSY status — only while the (opt-in) feature is enabled.
        if self.settings.qsy_enabled {
            s.qsy = Some(self.qsy_status());
        }
        match &self.mode {
            Mode::Chat => s.mode = OpMode::Chat,
            Mode::Qso { station, running } => {
                s.mode = OpMode::Qso;
                s.qso = Some(QsoStatus {
                    state: format!("{:?}", station.state),
                    dxcall: station.dxcall.clone(),
                    dxgrid: station
                        .dxcall
                        .as_deref()
                        .and_then(|c| self.dx_grid_resolved(c, station.dxgrid.clone())),
                    rx_report: station.rx_report,
                    running: *running,
                    tx_now: station.pending_text(),
                    stalled: station.stalled(),
                    tx_count: station.tx_count,
                });
            }
            // Master switch off (spec §1.3): defensively refuse to expose FD
            // chrome while `fd_active` is false, so a lingering `Mode::FieldDay`
            // can't strand the operator in Field Day after the master flips off.
            Mode::FieldDay { .. } if !self.settings.fd_active => s.mode = OpMode::Chat,
            Mode::FieldDay { station, running } => {
                s.mode = OpMode::FieldDay;
                let log = &station.log;
                let rs = tempo_core::fd_rules::ruleset(
                    log.event,
                    tempo_core::fd_rules::CURRENT_RULES_YEAR,
                );
                let (qso_pts, powered) =
                    rs.scoring.qso_and_powered(log, self.settings.fd_power_mult);
                let bonus = rs.bonus_points(&self.settings.fd_bonuses);
                s.field_day = Some(FieldDayStatus {
                    my_class: log.myexch.class.clone(),
                    my_section: log.myexch.section.clone(),
                    running: *running,
                    state: format!("{:?}", station.state),
                    dxcall: station.dxcall.clone(),
                    qso_count: log.qso_count(),
                    sections: log.sections(),
                    worked_sections: log.worked_sections(),
                    points: qso_pts,
                    event: if matches!(log.event, tempo_core::fieldday::FdEvent::WinterFd) {
                        "wfd".into()
                    } else {
                        "arrlfd".into()
                    },
                    powered_points: powered,
                    bonus_points: bonus,
                    total_score: powered + bonus,
                    log: log
                        .qsos()
                        .iter()
                        .map(|q| FieldDayQso {
                            call: q.call.clone(),
                            class: q.class.clone(),
                            section: q.section.clone(),
                            band: q.band.clone(),
                            mode: q.mode.clone(),
                            when_unix: q.when_unix,
                        })
                        .collect(),
                });
            }
        }

        // Project this slot's decodes into the live feed (alerts + coloring).
        let mycall = &self.settings.mycall;
        // The band these decodes arrived on — new-grid / new-DXCC are per-band
        // questions, and this chain's dial is the only honest answer to "on what
        // band did I just hear this?".
        let cur_band = self.settings.band.as_str();
        s.recent_decodes = self
            .last_decodes
            .iter()
            .map(|d| {
                let parsed = Msg::parse(&d.message);
                let is_cq = matches!(parsed, Msg::Cq { .. });
                let from = parsed.sender().map(|c| c.to_string());
                let directed_to_me = parsed
                    .addressee()
                    .map(|a| a.eq_ignore_ascii_case(mycall))
                    .unwrap_or(false);
                // Worked-before (B4): the decode's sender is in the logbook —
                // via the SAME prebuilt set as the roster above. The per-row
                // `worked_before()` this replaces was the exact O(rows × log)
                // sweep the set was built to end, re-grown 240 lines below the
                // comment explaining the 1–2 s waterfall stall it caused.
                let worked = from
                    .as_deref()
                    .map(|c| worked.contains(&c.to_ascii_uppercase()))
                    .unwrap_or(false);
                // New-grid (B3): the decode carries a Maidenhead grid we've never
                // worked ON THIS BAND. Grid is present on CQ/grid forms. Per band
                // because that is how grids are awarded (VUCC) — a grid worked on
                // 20 m is a genuinely new one on 2 m.
                let grid = match &parsed {
                    Msg::Cq { grid, .. } | Msg::Grid { grid, .. } => Some(grid.as_str()),
                    _ => None,
                };
                let new_grid = grid
                    .map(|g| !g.is_empty() && !self.station.grid_worked_on(g, cur_band))
                    .unwrap_or(false);
                // Country + New-DXCC (B3): resolve the sender's DXCC entity once.
                // `country` rides every decode (DX chasers scan by country); the
                // entity also drives new-DXCC. Needs the injected resolver; both
                // stay None/false in headless tests.
                let entity = match (&from, &self.station.dxcc_resolve) {
                    (Some(c), Some(resolve)) => resolve(c),
                    _ => None,
                };
                // Split the highlight (operator ruling, option 2): a TRUE all-time-new DXCC
                // entity (never worked on any band) reads as `DXCC` — matching the Needed
                // board's NEW ONE / ATNO — while an entity worked elsewhere but new on THIS band
                // (DXCC is awarded per band: Challenge slots, VHF DXCC) reads as the dimmer
                // `BAND`. The two are mutually exclusive by construction, so a chaser never
                // mistakes a band-slot for a new country.
                let (new_dxcc, new_band) = match &entity {
                    Some(e) => {
                        let ever = self.station.entity_worked_ever(e);
                        (!ever, ever && !self.station.entity_worked_on(e, cur_band))
                    }
                    None => (false, false),
                };
                // Rarity: prefer the grid on THIS frame, but on report/R/RR73/73
                // frames (which carry no grid) fall back to the sender's grid
                // remembered in the roster — so an ULTRA-rare station keeps its
                // badge through the whole QSO, not just on its CQ. (Backfills only
                // the rarity marker; the row's own `grid` text stays unchanged.)
                let grid_rarity = self.station.rarity_of(grid.or_else(|| {
                    from.as_deref()
                        .and_then(|c| self.app.inbox.roster.get(c))
                        .and_then(|h| h.grid.as_deref())
                }));
                DecodeRow {
                    lotw_user: self.station.lotw_user(from.as_deref()),
                    from,
                    snr: d.snr,
                    dt_sec: d.dt,
                    freq_hz: d.freq,
                    message: d.message.clone(),
                    is_cq,
                    directed_to_me,
                    worked,
                    country: entity,
                    new_dxcc,
                    new_band,
                    new_grid,
                    grid: grid.filter(|g| !g.is_empty()).map(str::to_string),
                    grid_rarity,
                    // WSJT-X decode markers: trailing 'a' = AP-assisted decode,
                    // '?' = low-confidence (qual below the stock 0.17 line).
                    ap: d.nap > 0,
                    low_conf: d.qual < 0.17,
                    // Label each decode by the mode that actually produced it
                    // (native source's mode, or a companion stream's per-decode
                    // WSJT-X mode); fall back to the selected tier when unknown
                    // (DX1's robust path, or an unrecognized companion mode).
                    tier: d.mode.map(Tier::from_mode_kind).unwrap_or(s.link.tier),
                    // DTO wire contract keeps rv as i32 (-1 = N/A); collapse the
                    // unified Option<i32> at this boundary.
                    rv: d.rv.unwrap_or(-1),
                    mine: false,
                    tx_at: None,
                }
            })
            .collect();
        // Append OUR OWN transmissions as `mine` rows so the operator sees each of
        // their calls in the decode feed (WSJT-X own-TX). The UI keys these by
        // cycle so repeated identical calls stack as distinct timestamped lines.
        let mycall = self.settings.mycall.clone();
        for tx in &self.own_tx {
            s.recent_decodes.push(DecodeRow {
                from: Some(mycall.clone()),
                snr: 0,
                dt_sec: 0.0,
                freq_hz: tx.freq_hz,
                message: tx.text.clone(),
                is_cq: false,
                directed_to_me: false,
                worked: false,
                country: None,
                new_dxcc: false,
                new_band: false,
                new_grid: false,
                grid: None,
                grid_rarity: None,
                lotw_user: false, // own-TX rows never mark
                rv: -1,
                mine: true,
                tx_at: Some(tx.when_unix),
                tier: s.link.tier,
                ap: false,
                low_conf: false,
            });
        }
        s.highlights = self
            .highlights
            .iter()
            .map(|(call, (_, bg, fg))| crate::dto::HighlightEntry {
                call: call.clone(),
                bg: bg.clone(),
                fg: fg.clone(),
            })
            .collect();
        s.clear_tick = self.clear_tick;
        s.work_tick = self.work_tick;
        s.work_view = self.work_view.clone();
        s.work_call = self.work_call.clone();
        s.hunt = self
            .station
            .hunt_target() // TTL-filtered: an expired pend never shows a chip
            .map(|(program, reference, call)| crate::dto::HuntDto {
                program,
                reference,
                call,
            });
        s.harq_rescues = self.harq_rescues;
        s.upload_note = self.station.upload_note.clone();
        s.upload_ok = self.station.upload_ok;
        s.upload_tick = self.station.upload_tick;
        s.pending_log = self.pending_log.clone().map(Into::into);
        s
    }

    // ----- the live loop --------------------------------------------------

    /// Audio waveform(s) to transmit at `slot` (empty unless it's our TX slot and
    /// the active mode has something to send). One frame per slot.
    /// The beacon message for the active identity, or `Err` with the reason.
    ///
    /// ⚠️ REFUSES ON AN UNSET POWER. WSPR reports feed a public propagation
    /// database, so a beacon claiming 0 dBm (1 mW) while actually running 5 W
    /// corrupts other operators' conclusions about the path, not just this
    /// station's own. Upstream defaults the field; we require it. A deliberate
    /// divergence, and it fails closed — no power, no transmission.
    fn beacon_message(&self) -> Result<String, String> {
        let call = self.settings.mycall.trim();
        let grid = self.settings.mygrid.trim();
        if call.is_empty() {
            return Err("Set your callsign before beaconing".to_string());
        }
        if grid.len() < 4 {
            return Err("Set your 4-character grid before beaconing".to_string());
        }
        if self.settings.beacon_power_dbm <= 0 {
            return Err(
                "Set your beacon transmit power (dBm) in Settings. WSPR reports \
                        are published, so the figure has to be real — 30 dBm = 1 W, \
                        37 = 5 W."
                    .to_string(),
            );
        }
        Ok(tempo_core::beacon::message(
            call,
            grid,
            self.settings.beacon_power_dbm,
        ))
    }

    /// Transmit decision + waveform for one BEACON interval.
    ///
    /// ⭐ ONE DECISION PER INTERVAL. `poll_tx` may be called many times inside a
    /// slot; re-drawing each time would break the anti-consecutive rule and make
    /// the achieved duty cycle meaningless.
    ///
    /// The TX-safety invariants still apply in full — this is only reached after
    /// `poll_tx`'s `tx_enabled` / `tx_allowed` / operating-mode guards, so a beacon
    /// keys unattended only once the operator has explicitly armed transmit.
    fn plan_beacon_tx(&mut self, slot: u64) -> Option<TxPlan> {
        // ⭐ 0% MEANS SILENT, WHICHEVER SCHEDULER IS SELECTED. This check used to
        // live only inside `BeaconScheduler::next_is_tx`, which the Round-Robin arm
        // below never calls — so a station with an RR slot left over from a
        // coordinated session kept keying every Nth interval after the operator set
        // Transmit % to 0 and believed they had stopped. Settings says "0 = listen
        // only" in as many words; it has to be true on both paths.
        if self.settings.beacon_tx_percent == 0 {
            self.beacon_decided_slot = Some(slot);
            self.beacon_tx_this_slot = false;
            self.app.set_transmitting(false);
            return None;
        }
        if self.beacon_decided_slot != Some(slot) {
            self.beacon_decided_slot = Some(slot);
            self.beacon_sched
                .set_tx_percent(self.settings.beacon_tx_percent);
            self.beacon_tx_this_slot = if self.settings.beacon_rr_slot > 0 {
                // FST4W Round Robin: a deterministic assignment so coordinated
                // stations never collide. A pure function of UTC, which `slot`
                // counts periods of.
                let period = self.active_slot_secs().max(1.0) as u32;
                let secs_of_day = (slot.wrapping_mul(u64::from(period)) % 86_400) as u32;
                tempo_core::beacon::fst4w_round_robin_is_tx(
                    secs_of_day,
                    period as u16,
                    self.settings.beacon_rr_slot.saturating_sub(1),
                    self.settings.beacon_rr_slots,
                )
            } else {
                self.beacon_sched.next_is_tx()
            };
        }
        if !self.beacon_tx_this_slot {
            self.app.set_transmitting(false);
            return None;
        }

        // Misconfigured (no power, no grid) ⇒ stay silent. The UI surfaces the
        // reason; publishing a wrong report is worse than not beaconing.
        let Ok(msg) = self.beacon_message() else {
            self.app.set_transmitting(false);
            return None;
        };
        let Some(kind) = self.tier_mode_kind(self.app.tier()) else {
            self.app.set_transmitting(false);
            return None;
        };
        // Resolved here rather than in the builder so a decode-only mode reaching
        // the beacon path is refused BEFORE any of the keying machinery runs.
        if modes::tx_mode(kind).is_none() {
            self.app.set_transmitting(false);
            return None;
        }
        Some(TxPlan {
            slot,
            tier: self.app.tier(),
            waveform: TxWaveform::Native {
                kind,
                text: msg,
                f0: self.tx_offset_hz(),
            },
            beacon: true,
        })
    }

    pub fn plan_tx(&mut self, slot: u64) -> Option<TxPlan> {
        // Coordinated QSY: execute a scheduled move the moment it comes due,
        // regardless of TX/RX/mute state (no-op while the feature is disabled).
        self.qsy_execute_due(slot);
        // Monitor-off (transmit muted), holding a tune carrier, or outside the operator's
        // license privileges at this dial/mode: no slot TX. The radio loop handles the
        // steady tune carrier separately (also privilege-gated at set_tune).
        if !self.tx_enabled || self.tuning || !self.tx_allowed() {
            self.app.set_transmitting(false);
            return None;
        }
        // RECEIVE-ONLY TIER: stop HERE, with the other "we are not transmitting"
        // guards — not at the wave builder below.
        //
        // ⭐ THIS ORDERING IS THE BUG, not a redundancy. `modes::tx_mode()` already
        // refused at the bottom of this function, so nothing ever keyed; but every
        // side effect of an over happens BETWEEN here and there, and none of it is
        // rolled back when the waveform turns out not to exist. On a receive-only
        // tier the old flow advanced the QSO sequencer and burned the CQ budget,
        // pushed an own-TX row into the band feed, wrote a `Tx` line to ALL.TXT
        // (which tailing loggers and GridTracker read as fact), armed the pending
        // CW ID, and started the TX watchdog that later reported halting a
        // transmission that never happened. Refusing at the top makes all of it
        // unrepresentable instead of merely harmless.
        //
        // Unreachable in normal operation now that arming is gated — this is the
        // backstop for any future path that latches `tx_enabled` directly.
        if self.tier_is_rx_only(self.app.tier()) {
            self.app.set_transmitting(false);
            return None;
        }
        // Phone / CW sections own the rig: the FT8/FT1 slot sequencer must NOT key the
        // radio while the operator is on voice or CW (the keyer/PTT drive those modes).
        // Without this gate a still-armed beacon/QSO would inject digital tones onto a
        // phone over (shared PTT + output ring) — a wrong-mode/spurious-emission bug.
        if self.settings.operating_mode != crate::settings::OperatingMode::Digital {
            self.app.set_transmitting(false);
            return None;
        }
        // BEACON TIER (WSPR / FST4W): a separate transmit path — there is no
        // exchange, so it never reaches the QSO sequencer below. The only decision
        // is WHETHER to key this interval.
        //
        // ⭐ THIS MUST STAY BELOW THE OPERATING-MODE GATE. It was above it, and that
        // was a live RF bug, not a style point: `set_operating_mode` ARMS TX when
        // the operator enters Phone/CW/RTTY (the comment there justifies it with
        // "poll_tx is gated off for non-Digital", which the early return had
        // quietly made false). A configured beacon therefore keyed itself on
        // schedule while the operator was on SSB — 111 s of WSPR tones into the
        // 20 m phone segment, on the shared PTT, with no action that reads as
        // "transmit". `tx_allowed` compounded it by validating the SSB passband
        // against PHONE privileges, so the data emission was checked against the
        // wrong rule set too.
        //
        // Everything above this line is a reason not to transmit AT ALL, so the
        // beacon must clear all of it. Only the QSO-specific machinery below —
        // identity packing, parity, the sequencer — is genuinely inapplicable.
        if self.tier_is_beacon(self.app.tier()) {
            return self.plan_beacon_tx(slot);
        }
        // Delivery ACKs we now owe (heard a directed message addressed to us) ride out on
        // the chat broadcast path — closing the store-and-forward loop. Only reached when
        // TX is enabled (never unsolicited), and ONLY in Chat mode — that's the only arm
        // that drains broadcast_queue, so queuing an ACK in QSO/Field Day would strand it
        // (and a later set_mode would wipe it). take_pending_acks preserves the owed ACKs
        // until we're back in Chat.
        if matches!(self.mode, Mode::Chat) {
            self.send_pending_acks(slot);
        }
        // Identity backstop for STANDARD (FT8/FT4) messages: never put a frame on the
        // air without the callsign + grid those messages require. WSJT-X refuses to
        // build a CQ/Tx1 without them; an empty/invalid grid here would otherwise emit a
        // grid-less call (the reported bug). FT1/DX1 free-text isn't grid-bound, so it's
        // exempt. This is the last line of defense — the command layer also blocks
        // entering a keying FT8/FT4 mode without a valid identity (with a clear message).
        // (structured_tx_ready returns Ok off the FT8/FT4 tier, so FT1/DX1 free-text is
        // exempt; Field Day exchanges carry no grid, so only the callsign is required.)
        let needs_grid = !matches!(self.mode, Mode::FieldDay { .. });
        if self.structured_tx_ready(needs_grid).is_err() {
            self.app.set_transmitting(false);
            return None;
        }

        if slot % 2 != self.tx_parity {
            self.app.set_transmitting(false);
            return None;
        }
        // Coordinated QSY (initiator): announce a hop on this over by queueing the
        // directive ahead of normal broadcasts. Live in Chat only — the auto-QSO /
        // Field-Day sequencers are never interrupted.
        if self.qsy_active() {
            let mycall = self.settings.mycall.clone();
            if let Some(dir) = self.qsy.poll_announce(&mycall, slot) {
                self.enqueue_qsy_directive(&dir);
            }
        }
        // IR-HARQ redundancy version for this transmission. Only the QSO
        // auto-sequencer escalates it (on unacknowledged retransmissions); Chat
        // and Field Day always send RV0.
        let mut tx_rv: i32 = 0;
        let text: Option<String> = match &mut self.mode {
            Mode::Chat => {
                // Open broadcasts have priority and send unconditionally — no
                // recipient presence gate, unlike directed store-and-forward.
                if let Some(f) = self.broadcast_queue.pop_front() {
                    Some(f)
                } else {
                    // The released burst has fully drained: arm the listening window
                    // (≈ one full over each way = 2 own-parity slots).
                    if self.tx_queue.is_empty() && self.chat_yield_pending {
                        self.chat_yield_pending = false;
                        self.chat_yield_slots = 2;
                    }
                    if self.tx_queue.is_empty() && self.chat_yield_slots > 0 {
                        // Yield-to-RX: spend this own-parity slot LISTENING instead of
                        // immediately grabbing it for more traffic (CQ resume + beacon
                        // included). Broadcast ACKs above still go out — the peer's
                        // receipt never waits. tx_parity is untouched: parity separation
                        // is the collision-avoidance contract with the peer.
                        self.chat_yield_slots -= 1;
                        None
                    } else {
                        if self.tx_queue.is_empty() {
                            // Cadence windows are WALL-CLOCK, converted to slots by the active
                            // tier's period (4 s TempoFast / 15 s TempoDeep): presence window
                            // 120 s, listening backoff 16 s after the burst's last frame. The
                            // cycle cap is the bounded-ARQ budget — TempoFast rides HARQ-era
                            // defaults (3); TempoDeep has no HARQ, so plain repeats get 5.
                            let period = self.active_slot_secs().max(1.0);
                            let window = ((120.0 / period).ceil() as u64).max(1);
                            let backoff = ((16.0 / period).ceil() as u64).max(1);
                            let cap =
                                self.settings
                                    .chat_max_cycles
                                    .unwrap_or(match self.app.tier() {
                                        Tier::TempoDeep => 5,
                                        _ => 3,
                                    });
                            let (frames, bodies) = self.app.due_frames(slot, window, backoff, cap);
                            for f in frames {
                                self.tx_queue.push_back(f);
                            }
                            if !self.tx_queue.is_empty() {
                                // A release recorded attempts/backoff — journal so a restart
                                // resumes the retry schedule instead of resetting it.
                                self.persist_pending_msgs();
                            }
                            // A directed message shows in band activity when it actually goes
                            // on the air (first release), not at compose time.
                            for b in bodies {
                                self.record_own_tx(b);
                            }
                            // Directed traffic released this slot: stamp the CQ run's idle
                            // clock so a paused run doesn't resume mid-conversation, refill
                            // the CQ budget (the run is conversing, not calling unanswered),
                            // and arm the yield-to-RX listening window for when it drains.
                            if !self.tx_queue.is_empty() {
                                self.chat_cq_last_directed = slot;
                                self.chat_cq_unanswered = 0;
                                self.chat_yield_pending = true;
                            }
                            // Chat CQ RUN: an idle own-parity TX slot re-sends the
                            // structured CQ (the "keep calling until answered" loop).
                            // Directed frames always win the slot (the queue check above);
                            // a pause set by answering auto-resumes once directed traffic
                            // has been quiet for CHAT_CQ_RESUME_SLOTS. Supersedes the
                            // presence beacon while running.
                            if self.chat_cq && self.tx_queue.is_empty() {
                                const CHAT_CQ_RESUME_SLOTS: u64 = 8; // ~32 s at FT1's 4 s slots
                                if self.chat_cq_paused
                                    && slot.saturating_sub(self.chat_cq_last_directed)
                                        >= CHAT_CQ_RESUME_SLOTS
                                {
                                    self.chat_cq_paused = false;
                                }
                                if !self.chat_cq_paused {
                                    // Budget: stop after N consecutive unanswered calls
                                    // (cq_max_calls; default 10 ≈ 80 s of calling at 4 s
                                    // slots). Any directed activity refills it; the operator
                                    // restarting CQ refills it. Bounded like everything else
                                    // after the cadence rework — no more calling forever.
                                    let budget = self.settings.cq_max_calls.unwrap_or(10).max(1);
                                    if self.chat_cq_unanswered >= budget {
                                        self.chat_cq = false; // run stops; snapshot shows it
                                    } else if let Some(cq) = self.chat_cq_text() {
                                        self.tx_queue.push_back(cq);
                                        self.chat_cq_unanswered += 1;
                                    }
                                }
                            }
                            // Presence beacon ("CQ <call> <grid>") only when the
                            // operator has enabled it. Default off → the app starts
                            // passive (hunt-and-pounce): it never calls CQ on its own.
                            // The CQ run supersedes it (never both in one slot).
                            if !self.chat_cq
                            && self.settings.beacon
                            && self.tx_queue.is_empty()
                            // Every Nth of OUR TX slots. The outer `slot%2 !=
                            // tx_parity` guard already restricts to TX slots;
                            // `slot/2` counts slot-pairs so this fires every Nth
                            // TX slot for BOTH even- and odd-parity operators
                            // (the old `slot % N == tx_parity` never fired for
                            // odd parity, since N's multiples are all even).
                            && (slot / 2).is_multiple_of(self.beacon_every)
                            {
                                self.tx_queue.push_back(self.app.beacon_text());
                            }
                        }
                        self.tx_queue.pop_front()
                    }
                }
            }
            Mode::Qso { station, .. } => {
                // Directed-call cap: a station that goes silent stops being called after
                // this many unanswered overs of a directed step (settings; default 8).
                // Refreshed here every TX slot so it always tracks the live setting,
                // regardless of how this station was created (CQ answer / S&P / monitoring).
                station.call_cap = self.settings.directed_max_calls;
                match station.outgoing_rv() {
                    Some((m, rv)) => {
                        station.after_tx();
                        // Stock "Disable Tx after sending 73": the over leaving
                        // NOW is our final 73 (after_tx just cleared pending at
                        // Done). S&P only — a CQ run returns to CQ instead.
                        if station.state == QsoState::Done && !self.cq_running {
                            if self.settings.disable_tx_after_73 {
                                self.pending_tx_disable = true;
                            }
                            // WSJT-X "CW ID after 73": queue the ID for AFTER
                            // this final over finishes (the service drains it
                            // on TX-idle, same timing as the deferred disable —
                            // keying CW mid-FT8-over would splatter both).
                            if self.settings.cw_id_after_73 {
                                self.pending_cw_id = true;
                            }
                        }
                        // IR-HARQ off: always transmit RV0 (no redundancy escalation).
                        tx_rv = if self.settings.harq_enabled {
                            rv as i32
                        } else {
                            0
                        };
                        Some(m.to_text())
                    }
                    None => None,
                }
            }
            Mode::FieldDay { station, .. } => {
                let m = station.outgoing();
                if m.is_some() {
                    station.after_tx();
                }
                m.map(|x| x.to_text())
            }
        };
        match text {
            Some(t) => {
                self.app.set_transmitting(true);
                // Wall-clock watchdog (WSJT-X): start the timer on the first over after
                // an operator action, then trip (auto-halt TX) once REAL elapsed time
                // exceeds the limit — counting the RX slots between overs too, so it
                // fires at the configured minutes, not 2x late (FT8/FT4 TX every other slot).
                let limit_secs = self.settings.tx_watchdog_min as u64 * 60;
                if limit_secs > 0 {
                    let now = now_unix_secs();
                    let start = *self.tx_watchdog_start.get_or_insert(now);
                    if now.saturating_sub(start) >= limit_secs {
                        self.tx_watchdog = true;
                        self.tx_enabled = false;
                        // ⭐ RETURN, don't fall through. This used to disarm TX and then
                        // build and return the wave anyway, so the loop keyed the radio
                        // and cut it ~20 ms later on the next tick — a brief spurious
                        // emission every time the watchdog fired.
                        self.app.set_transmitting(false);
                        return None;
                    }
                    // ⭐ AN OVER LONGER THAN THE WHOLE LIMIT CAN NEVER BE TRANSMITTED.
                    //
                    // The watchdog is evaluated when an over is BUILT, never during one,
                    // so it cannot interrupt a transmission already in flight. That is
                    // fine at 12.6 s (FT8) and wrong at 1800 s: FST4-1800 would key for
                    // THIRTY MINUTES with no duration bound whatsoever, then be refused
                    // at the next boundary — the operator's "TX watchdog: 6 min" setting
                    // silently not applying to the mode that most needs it.
                    //
                    // Refusing is the honest outcome: say so, and let the operator raise
                    // the limit deliberately. Same shape as the SSTV path, which already
                    // declines a send that could not finish in time.
                    //
                    // Scoped so FT modes are untouched: this fires only when the over
                    // exceeds the ENTIRE limit, which at 12.6 s against a 6-minute
                    // default cannot happen. It is not a "will it finish in the time
                    // remaining" test — that WOULD change FT8, by refusing an over
                    // starting a few seconds before the limit that today goes out.
                    let over_secs = self.tx_over_secs() as u64;
                    if over_secs > limit_secs {
                        self.tx_watchdog = true;
                        self.tx_enabled = false;
                        self.app.set_transmitting(false);
                        return None;
                    }
                }
                // Record this transmission so the decode feed shows our own calls
                // (WSJT-X own-TX). STRUCTURED frames (CQ, QSO exchanges, beacon) are
                // clean single overs → record as-is. CHUNK frames of a free-text chat
                // message are recorded ONCE as the LOGICAL message by broadcast()/
                // send_message()/qso_freetext() — showing the raw wire chunks here
                // ("A13DE KD9TAW") was the band-activity garble bug. Also skip a SINGLE
                // bare broadcast frame ("DE <CALL> 73", the S3 fast-path) — likewise
                // recorded once at source as the clean body.
                let is_chunk = tempo_core::text::parse_chunk(&t).is_some();
                let is_broadcast = tempo_core::inbox::parse_broadcast(&t).is_some();
                if !is_chunk && !is_broadcast {
                    self.record_own_tx(t.clone());
                    // ALL.TXT parity with WSJT-X: log OUR over as a `Tx` line too. The RX-only
                    // ALL.TXT left the operator's own CQ / reports / RR73 invisible — to tailing
                    // loggers and GridTracker, AND to us when a sequencing question comes up (an
                    // RX-only trace can't show whether WE sent the RR73). Pure logging, no on-air
                    // effect. Same clean-frame filter as record_own_tx; SNR/DT are 0 for our TX,
                    // audio = our TX offset.
                    if self.settings.write_all_txt {
                        let mode = format!("{:?}", self.app.tier()).to_uppercase();
                        self.station
                            .all_txt_pending
                            .push(crate::alltxt::all_txt_line(
                                now_unix_secs(),
                                self.settings.dial_mhz,
                                true,
                                &mode,
                                0,
                                0.0,
                                self.tx_offset_hz,
                                &t,
                            ));
                        let len = self.station.all_txt_pending.len();
                        if len > 5000 {
                            self.station.all_txt_pending.drain(0..len - 5000);
                        }
                    }
                }
                // Robust tier (DX1) modulates 8-FSK; fast tier (FT1) uses 4-CPM.
                // Both place the signal at the operator's TX audio offset.
                let waveform = match self.app.tier() {
                    // Robust tier: 8-FSK non-coherent.
                    Tier::TempoDeep => TxWaveform::Deep {
                        text: t,
                        f0: self.tx_offset_hz,
                    },
                    // FT1: 4-CPM. QSO mode escalates tx_rv for IR-HARQ
                    // retransmissions; Chat/Field Day keep tx_rv = 0 (RV0 = tx::build).
                    Tier::TempoFast => TxWaveform::Fast {
                        text: t,
                        f0: self.tx_offset_hz,
                        rv: tx_rv,
                    },
                    // FT8 / FT4: encode + synthesize via the active mode (no IR-HARQ).
                    // Split Operation reduces the audio into 1500–2000 Hz and
                    // leaves the matching dial shift for the slot core to apply
                    // before PTT — the on-air RF frequency is unchanged.
                    native => {
                        let kind = self
                            .tier_mode_kind(native)
                            .unwrap_or(modes::ModeKind::TempoFast);
                        // tx_mode, not make_mode: a receive-only mode yields None and this
                        // over is abandoned. Every mode shipped today declares tx, so None
                        // means a decode-only mode reached the TX path, which is a bug —
                        // refusing to key is the correct response to it.
                        //
                        // Checked HERE, in the planner, and not left to the builder: the
                        // builder returns a bare empty wave, which reads as "nothing to
                        // send" rather than "this mode must never key".
                        if modes::tx_mode(kind).is_none() {
                            self.app.set_transmitting(false);
                            return None;
                        }
                        // The split reduction is engine state (the slot core applies the
                        // matching dial shift before PTT), so it is decided here even
                        // though only the builder consumes f0.
                        let (f0, shift) = self.split_reduce(self.tx_offset_hz);
                        self.tx_dial_shift_hz = shift;
                        TxWaveform::Native { kind, text: t, f0 }
                    }
                };
                Some(TxPlan {
                    slot,
                    tier: self.app.tier(),
                    waveform,
                    beacon: false,
                })
            }
            None => {
                self.app.set_transmitting(false);
                None
            }
        }
    }

    /// Decide, build and commit an over in one call — the whole transmit decision
    /// for `slot`, exactly as it has always behaved.
    ///
    /// This is [`Self::plan_tx`] + [`TxWaveform::build`] + [`Self::commit_tx`] run
    /// back to back. The radio loop drives those three separately so it can RELEASE
    /// THE ENGINE MUTEX across the build, which takes the modem lock and can
    /// therefore wait out an entire decode; every other caller wants this one.
    pub fn poll_tx(&mut self, slot: u64) -> Vec<Vec<f32>> {
        let Some(plan) = self.plan_tx(slot) else {
            return Vec::new();
        };
        let wave = plan.waveform.build();
        self.commit_tx(&plan, wave)
    }

    /// Accept a built waveform and produce the over, or refuse it.
    ///
    /// Refuses a plan that no longer describes this engine. The radio loop builds
    /// with the engine mutex RELEASED, so between plan and commit the operator can
    /// change tier — keying a waveform built for the previous tier is a wrong-mode
    /// emission, precisely what the transmit guards exist to prevent. The check is
    /// cheap and makes the race unrepresentable rather than merely unlikely.
    pub fn commit_tx(&mut self, plan: &TxPlan, wave: Vec<f32>) -> Vec<Vec<f32>> {
        if plan.tier != self.app.tier() {
            self.app.set_transmitting(false);
            return Vec::new();
        }
        // Bail out on an EMPTY OUTER vec, never `vec![empty_wave]`. Callers gate
        // on `waves.is_empty()` and then index `waves.first()` / `waves.len()-1`
        // and call `rig.ptt(true)`; a zero-length wave passes that guard and
        // walks into the keying path with nothing to send. Returning no waves at
        // all is the only shape every downstream branch already handles.
        if wave.is_empty() {
            self.app.set_transmitting(false);
            return Vec::new();
        }
        // The BEACON path runs the wall-clock watchdog HERE rather than in the
        // planner, preserving the order the single-shot path used (build, then
        // check): a build that produces nothing still leaves the watchdog clock
        // unstarted, exactly as before.
        if plan.beacon {
            let limit_secs = u64::from(self.settings.tx_watchdog_min) * 60;
            if limit_secs > 0 {
                let now = now_unix_secs();
                let start = *self.tx_watchdog_start.get_or_insert(now);
                if now.saturating_sub(start) >= limit_secs {
                    self.tx_watchdog = true;
                    self.tx_enabled = false;
                    self.app.set_transmitting(false);
                    return Vec::new();
                }
            }
            self.app.set_transmitting(true);
        }
        vec![wave]
    }

    /// Decode a captured frame and fold it into the app *and* the active mode's
    /// sequencer. Returns the number of decodes.
    ///
    /// Synchronous: builds the job, runs the (heavy) decode inline, and folds the
    /// result — the same three steps the radio loop runs asynchronously across the
    /// decode-worker thread. The epoch can't move between build and apply here (one
    /// thread, no await), so the result always applies. Used by the headless test
    /// driver and as the in-process reference; the live loop uses the async split.
    pub fn ingest(&mut self, frame: &[f32], slot: u64) -> usize {
        let job = self.build_decode_job(frame.to_vec(), slot, DecodePass::Boundary);
        let result = run_decode_job(job);
        match self.apply_decode_result(result) {
            DecodeApplied::Boundary { n, .. } => n,
            _ => 0,
        }
    }

    /// Build the OWNED decode job for `frame`/`slot` under the engine lock: capture
    /// the branch (Native / DX1 / Companion), the AP request context, the HARQ-reset
    /// flag and the current decode epoch, plus an `Arc` clone of the decoder. No heavy
    /// work — the actual decode runs later in [`run_decode_job`] off the engine mutex.
    pub fn build_decode_job(&self, frame: Vec<f32>, slot: u64, pass: DecodePass) -> DecodeJob {
        let source = self.source.clone();
        let epoch = self.decode_epoch;
        // Companion: decodes arrive over UDP; the audio is irrelevant. Drain the
        // network source regardless of the selected tier.
        if self.source_kind == SourceKind::Companion {
            return DecodeJob {
                source,
                frame,
                branch: DecodeBranch::Companion,
                nfa: 0,
                nfb: 0,
                ndepth: 0,
                mycall: String::new(),
                hiscall: String::new(),
                nqso_progress: 0,
                nfqso: 0,
                frame_time_ms: 0,
                harq_reset: false,
                pass,
                slot,
                epoch,
                ctx: None,
            };
        }
        if self.app.tier() == Tier::TempoDeep {
            // DX1 full-passband acquisition — its own robust path, no AP context.
            return DecodeJob {
                source,
                frame,
                branch: DecodeBranch::TempoDeep,
                nfa: 0,
                nfb: 0,
                ndepth: 0,
                mycall: String::new(),
                hiscall: String::new(),
                nqso_progress: 0,
                nfqso: 0,
                frame_time_ms: 0,
                harq_reset: false,
                pass,
                slot,
                epoch,
                ctx: None,
            };
        }
        // Native tiers (FT1/FT8/FT4) decode through the active SignalSource with
        // the golden WSJT-X AP context. This block is the exact request-building
        // that used to live inline in `decode_frame` — moved here unchanged so the
        // heavy `decode_a7` call is all that crosses to the worker.
        //
        // IR-HARQ off (or a non-FT1 mode): the worker clears buffered FT1 RV0 so
        // nothing cross-frame-combines (each frame decoded RV0-only).
        let harq_reset = !self.settings.harq_enabled;
        // Monotonic ms timestamp for cross-frame IR-HARQ keying (FT1); only
        // differences (≤ 30 s) and the low 32 bits matter, so a slot-derived
        // counter at the active slot period suffices.
        let frame_time_ms = (slot as i64).wrapping_mul((self.active_slot_secs() * 1000.0) as i64);
        // A-priori (AP) context for the golden WSJT-X FT8/FT4 decoder: our callsign,
        // the station we're working, and the QSO-progress index (0..5). Only FT8/FT4
        // use WSJT-X AP; FT1 ignores these and stays on the empty/0 path.
        let (ap_mycall, ap_hiscall, ap_progress) = match (&self.mode, self.app.tier()) {
            (Mode::Qso { station, .. }, Tier::Ft8 | Tier::Ft4) => (
                self.settings.mycall.clone(),
                station.dxcall.clone().unwrap_or_default(),
                station.state.nqso_progress(),
            ),
            (Mode::FieldDay { .. }, Tier::Ft8 | Tier::Ft4) => {
                (self.settings.mycall.clone(), String::new(), 0)
            }
            _ => (String::new(), String::new(), 0),
        };
        // Operator decode controls (WSJT-X F Low / F High / depth), clamped to the
        // modem's real passband and kept ordered.
        let nfa = self.settings.decode_flow_hz.clamp(200, 3900) as i32;
        let nfb = self
            .settings
            .decode_fhigh_hz
            .clamp(300, 4000)
            .max(nfa as u32 + 100) as i32;
        let ndepth = self.settings.decode_depth.clamp(1, 3) as i32;
        DecodeJob {
            source,
            frame,
            branch: DecodeBranch::Native,
            nfa,
            nfb,
            ndepth,
            mycall: ap_mycall,
            hiscall: ap_hiscall,
            nqso_progress: ap_progress,
            // WSJT-X nfqso = the freq we're working/listening on. Centers the deep
            // AP passes + sync there so the gain follows the worked station.
            nfqso: self.rx_offset_hz as i32,
            frame_time_ms,
            harq_reset,
            pass,
            slot,
            epoch,
            // Single-radio today: the chain owner attaches its own context with
            // `DecodeJob::with_ctx`. `None` keeps the shipped path byte-identical.
            ctx: None,
        }
    }

    /// Fold a completed [`DecodeResult`] back into the engine — the back half of the
    /// async decode. Drops the result if the decode context changed while it was in
    /// flight (epoch moved), preserving exactly the semantics of the synchronous
    /// [`Engine::ingest`] / [`Engine::ingest_early`] for the case that survives.
    pub fn apply_decode_result(&mut self, result: DecodeResult) -> DecodeApplied {
        if result.epoch != self.decode_epoch {
            // Built in a decode context that no longer exists (tier/source/band
            // switch since dispatch) — its slot indices / AP context are meaningless.
            return DecodeApplied::Stale;
        }
        if result.failed {
            // A contained panic. Drop it exactly like a stale result: it carries no
            // decodes and an EMPTY frame, and letting that frame through would set
            // `last_rx` to nothing — which the F6 redecode would then hand to a
            // decoder whose buffer-length assert panics, killing the worker again.
            // The period is simply lost, which is what WSJT-X does with any decode
            // it cannot complete.
            return DecodeApplied::Stale;
        }
        let DecodeResult {
            decodes,
            frame,
            pass,
            slot,
            ..
        } = result;
        match pass {
            DecodePass::Boundary => {
                // If the early pass already ingested this boundary's messages, keep
                // only the stragglers the full-window decode newly found.
                let decodes = self.drop_early_dupes(decodes, slot);
                let n = self.process_decodes(&frame, decodes, slot);
                DecodeApplied::Boundary { n, slot, frame }
            }
            DecodePass::Early => {
                if decodes.is_empty() {
                    // Nothing heard yet: leave ALL state untouched (advancing
                    // last_decode_slot early would skew the parity fallback). The
                    // boundary pass redecodes the full window from scratch.
                    return DecodeApplied::Early { n: 0 };
                }
                self.early_seen = Some((
                    slot,
                    decodes
                        .iter()
                        .map(|d| d.message.trim().to_string())
                        .collect(),
                ));
                let n = self.process_decodes(&frame, decodes, slot);
                DecodeApplied::Early { n }
            }
            // The F6 redecode runs its own display-only fold in `redecode`, not here.
            DecodePass::Redecode => DecodeApplied::Stale,
        }
    }

    /// WSJT-X-style EARLY decode pass (FT8/FT4, native source only): decode the
    /// partial capture a few seconds before the boundary so callers appear while
    /// the period is still running — the operator (and the auto-sequencer) get
    /// 2–3 s of decision time instead of zero (stock decodes ~3×/period starting
    /// at ~11.8 s). `slot` is the UPCOMING boundary slot (audio slot + 1) so the
    /// parity / history conventions match the boundary ingest exactly; the
    /// boundary pass then ingests only what this pass missed.
    pub fn ingest_early(&mut self, frame: &[f32], slot: u64) -> usize {
        if self.source_kind != SourceKind::Native
            || !matches!(self.app.tier(), Tier::Ft8 | Tier::Ft4)
        {
            // Companion DRAINS a UDP queue (an early drain would steal the
            // boundary's decodes); FT1/DX1 decode full frames only.
            return 0;
        }
        let job = self.build_decode_job(frame.to_vec(), slot, DecodePass::Early);
        let result = run_decode_job(job);
        match self.apply_decode_result(result) {
            DecodeApplied::Early { n } => n,
            _ => 0,
        }
    }

    /// Seed the decoder's SESSION hash table with compound calls from the
    /// logbook. The vendored packjt77 table (how `<...>` i3=4 tokens resolve to
    /// full calls) is process-persistent but dies on app restart — WSJT-X
    /// reloads its equivalents from disk. Encoding "CQ <call>" populates the
    /// same table through pack28→save_hash_call without transmitting anything,
    /// so compound stations you've worked resolve immediately on relaunch.
    pub fn seed_hash_table(&self) {
        use tempo_core::message::is_compound;
        let mode = modes::make_mode(modes::ModeKind::Ft8);
        let mut seen = std::collections::HashSet::new();
        // Each `encode` writes the process-global packjt77 hash table via FFI — the
        // same table the worker's decode reads. Hold the decoder lock across the
        // whole seed so it can't race an in-flight decode (may briefly wait if one
        // is running; seeding is a one-shot startup task).
        let _g = self.source.lock().unwrap();
        // Newest first; cap the work — each encode is one FFI round-trip.
        for rec in self.station.get_log().into_iter().rev() {
            let call = rec.call.trim().to_uppercase();
            if !is_compound(&call) || !seen.insert(call.clone()) {
                continue;
            }
            let _ = mode.encode(&format!("CQ {call}"));
            if seen.len() >= 50 {
                break;
            }
        }
    }

    /// WSJT-X "Decode" button / F6: re-run the decoder over the LAST period's
    /// retained audio with the CURRENT settings (deeper depth, changed
    /// passband, fresh AP context) and ingest only the lines the original pass
    /// missed — re-observing an already-ingested message would double-advance
    /// the sequencer and duplicate rows.
    pub fn redecode(&mut self) -> usize {
        if self.source_kind != SourceKind::Native {
            // Companion decode() DRAINS the live UDP queue — a redecode would
            // steal the boundary's datagrams (same guard as the early pass).
            return 0;
        }
        // Back to f32 for the job. Transient and on an operator button press, not the
        // hot loop — and `build_decode_job`'s own `capture_to_i16` reverses this
        // exactly, so the decoder sees the identical samples it saw live.
        let Some(frame) = self.last_rx.as_ref().map(|q| {
            q.iter()
                .map(|&v| f32::from(v) / 32767.0)
                .collect::<Vec<f32>>()
        }) else {
            return 0;
        };
        let Some(slot) = self.last_decode_slot else {
            return 0;
        };
        // Re-run the decode over the retained audio (a7_final = false — review pass,
        // no a7 save/replay). Synchronous: an operator button press, not the hot loop.
        let job = self.build_decode_job(frame, slot, DecodePass::Redecode);
        let decodes = run_decode_job(job).decodes;
        let fresh: Vec<modes::Decode> = decodes
            .into_iter()
            .filter(|d| {
                !self
                    .decode_history
                    .iter()
                    .any(|(s, h)| *s == slot && h.message.trim() == d.message.trim())
            })
            .collect();
        if fresh.is_empty() {
            return 0;
        }
        // DISPLAY-ONLY ingest: rows + history + spotting, but NOT the QSO
        // sequencer — the redecoded period may be a full cycle old, and the
        // CallingCq auto-answer arm would otherwise commit the run to a caller
        // who has long moved on. The operator sees the row and clicks if the
        // station is still there (stock F6 is an operator review tool).
        let n = fresh.len();
        for d in &fresh {
            self.decode_history.push_back((slot, d.clone()));
        }
        while self.decode_history.len() > 240 {
            self.decode_history.pop_front();
        }
        let pend_before = self.app.pending_count();
        self.app.observe(&fresh, slot);
        // An inbound ACK may have marked queued messages delivered (or purged spent
        // ones) inside observe — journal only when the queue actually shrank.
        if self.app.pending_count() != pend_before {
            self.persist_pending_msgs();
        }
        self.last_wire_decodes = fresh.clone();
        self.last_decodes = fresh;
        n
    }

    /// Drop decodes the early pass already ingested for this boundary slot.
    /// Always consumes the marker — a leftover from a slot whose boundary never
    /// decoded (we transmitted) must not filter a later slot.
    fn drop_early_dupes(&mut self, decodes: Vec<modes::Decode>, slot: u64) -> Vec<modes::Decode> {
        match self.early_seen.take() {
            Some((s, seen)) if s == slot => decodes
                .into_iter()
                .filter(|d| !seen.contains(d.message.trim()))
                .collect(),
            _ => decodes,
        }
    }

    /// Fold a slot's decodes into the app/sequencer state (the shared back half
    /// of [`Engine::ingest`] / [`Engine::ingest_early`]).
    fn process_decodes(&mut self, frame: &[f32], decodes: Vec<modes::Decode>, slot: u64) -> usize {
        // Keep the ON-AIR text for UDP consumers BEFORE any hound rewriting —
        // JTAlert/GridTracker must never receive a message the Fox didn't send.
        let hound_active = matches!(
            self.settings.special_op,
            crate::settings::SpecialOp::Hound | crate::settings::SpecialOp::SuperHound
        );
        let wire_copy: Option<Vec<modes::Decode>> = hound_active.then(|| decodes.clone());
        let decodes = self.hound_split(decodes);
        let n = decodes.len();
        // Tally IR-HARQ rescues (messages recovered by combining retransmissions).
        self.harq_rescues += decodes
            .iter()
            .filter(|d| matches!(d.rv, Some(rv) if rv > 0))
            .count() as u32;
        // ALL.TXT decode log (off by default): append each decode in WSJT-X format for
        // loggers/GridTracker to tail. `process_decodes` is the single per-decode
        // chokepoint (early-vs-boundary dupes already dropped), so each logs once. The
        // engine is I/O-free → buffer here; the shell drains + writes.
        if self.settings.write_all_txt {
            let dial = self.settings.dial_mhz;
            let mode = format!("{:?}", self.app.tier()).to_uppercase();
            let now = now_unix_secs();
            for d in &decodes {
                self.station
                    .all_txt_pending
                    .push(crate::alltxt::all_txt_line(
                        now, dial, false, &mode, d.snr, d.dt, d.freq, &d.message,
                    ));
            }
            // Bound memory if the shell never drains (e.g. headless): keep newest 5000.
            let len = self.station.all_txt_pending.len();
            if len > 5000 {
                self.station.all_txt_pending.drain(0..len - 5000);
            }
        }
        // Track recent decode DT magnitudes for the time-sync health estimate.
        for d in &decodes {
            self.seen_decode = true;
            self.recent_dt.push_back(d.dt.abs());
            if self.recent_dt.len() > DT_WINDOW {
                self.recent_dt.pop_front();
            }
        }
        let pend_before = self.app.pending_count();
        self.app.observe(&decodes, slot);
        // An inbound ACK may have marked queued messages delivered (or purged spent
        // ones) inside observe — journal only when the queue actually shrank.
        if self.app.pending_count() != pend_before {
            self.persist_pending_msgs();
        }
        self.observe_modes(&decodes, slot);
        // Coordinated QSY (any role): reassemble + act on directives from our
        // partner, and track that the partner is still being heard. The directive
        // also flows through the inbox above and shows in the band feed as plain
        // text — this only *additionally* acts on it. Live in Chat only.
        if self.qsy_active() {
            let mycall = self.settings.mycall.clone();
            for d in &decodes {
                if let Some(sender) = Msg::parse(&d.message).sender() {
                    self.qsy.note_heard(sender);
                }
                self.qsy.accept_decode(&d.message, slot, &mycall);
            }
            self.qsy.on_rx_slot(slot);
        }
        self.last_rx = Some(tempo_core::channel::capture_to_i16(frame));
        for d in &decodes {
            self.decode_history.push_back((slot, d.clone()));
        }
        while self.decode_history.len() > 240 {
            self.decode_history.pop_front();
        }
        self.last_wire_decodes = wire_copy.unwrap_or_else(|| decodes.clone());
        self.last_decodes = decodes;
        self.last_decode_slot = Some(slot);
        n
    }

    /// Hound mode: a DXpedition Fox packs TWO payloads in one transmission
    /// ("K1ABC RR73; W9XYZ <FOX> -08"). Split them so everything downstream —
    /// rows, roster, the auto-sequencer — sees both halves as ordinary messages
    /// (the standard sequencer then handles the whole hound exchange). Gated on
    /// Hound so normal operation (where free text may carry ';') is untouched.
    fn hound_split(&self, decodes: Vec<modes::Decode>) -> Vec<modes::Decode> {
        if !matches!(
            self.settings.special_op,
            crate::settings::SpecialOp::Hound | crate::settings::SpecialOp::SuperHound
        ) || !matches!(self.app.tier(), Tier::Ft8 | Tier::Ft4)
        {
            // Fox multiplexing is an FT8 DXpedition construct — FT1/DX1 free
            // text may legitimately contain ';' and must never be split.
            return decodes;
        }
        // The Fox we're working (for reconstructing its implied sender below).
        let fox: Option<String> = match &self.mode {
            Mode::Qso { station, .. } => station.dxcall.clone(),
            _ => None,
        };
        // A Fox confirm half is the SENDER-LESS 2-token "K1ABC RR73" — re-add
        // the Fox's call so it parses as a standard Rr73 and passes the
        // sequencer's sender lock. Only exact 2-token <call> RR73/RRR/73 forms.
        let reattach = |m: String| -> String {
            let t: Vec<&str> = m.split_whitespace().collect();
            if let (Some(f), [to, fin]) = (&fox, t.as_slice()) {
                if matches!(*fin, "RR73" | "RRR" | "73") && tempo_core::message::is_callsign(to) {
                    return format!("{to} {f} {fin}");
                }
            }
            m
        };
        let mut out = Vec::with_capacity(decodes.len());
        for d in decodes {
            let halves = d
                .message
                .split_once(';')
                .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()));
            match halves {
                Some((a, b)) if !a.is_empty() && !b.is_empty() => {
                    let mut d1 = d.clone();
                    d1.message = reattach(a);
                    let mut d2 = d;
                    d2.message = reattach(b);
                    out.push(d1);
                    out.push(d2);
                }
                // NOT a multiplex: pass through UNTOUCHED. Reattaching here
                // fabricated a Fox sender for any bystander's 2-token
                // "W9XYZ 73" free text — which could falsely COMPLETE and log
                // our QSO. Only the halves of a real Fox multiplex ever drop
                // their sender; a standalone Fox message carries the full form.
                _ => out.push(d),
            }
        }
        out
    }

    /// Fold a slot's decodes into the active mode's sequencer and, in QSO mode,
    /// auto-log the contact once the sequence completes. Shared by [`ingest`] and
    /// the test driver so both exercise the same QSO/auto-log path.
    fn observe_modes(&mut self, decodes: &[modes::Decode], slot: u64) {
        // The completed contact to auto-log, gathered while `self.mode` is
        // borrowed and committed after (so building the record doesn't conflict
        // with the mutable borrow of the sequencer).
        let mut completed: Option<(String, Option<String>, Option<i32>)> = None;
        // Did the partner advance the sequence this slot? WSJT-X resets the Tx
        // watchdog on genuine QSO progress — on a marginal path where every step
        // needs several repeats, a progressing QSO must not be watchdog-killed
        // mid-exchange. (Operator actions reset it elsewhere, as before.)
        let mut sequence_advanced = false;
        // Did the Field Day sequencer log a contact this slot (→ journal it)?
        let mut fd_logged = false;
        // Hound: TX frequency to adopt for the R+report (the Fox's freq).
        let mut hound_move_tx: Option<f32> = None;
        match &mut self.mode {
            Mode::Chat => {}
            Mode::Qso { station, .. } => {
                let state_before = station.state;
                // W1.4 best-caller selection: when RUNNING CQ and several stations
                // answer in the same slot, reorder the decodes so the sequencer
                // locks onto the operator's preferred caller instead of stock
                // first-heard. The default ("first", no SNR floor) leaves the
                // slice untouched, so the existing sequencer tests stay identical.
                let picked;
                let observed: &[modes::Decode] = if self.cq_running
                    && state_before == QsoState::CallingCq
                    && (self.settings.best_caller != "first"
                        || self.settings.best_caller_min_snr.is_some())
                {
                    picked = best_caller_decodes(
                        decodes,
                        &self.settings.mycall,
                        &self.settings.best_caller,
                        self.settings.best_caller_min_snr,
                        &self.settings.mygrid,
                    );
                    &picked
                } else {
                    decodes
                };
                station.observe(observed);
                sequence_advanced = station.state != state_before;
                // Quiet-finish (hound) completion sends NOTHING after the Fox's
                // RR73 — so the stock "disable TX after 73" one-shot in the TX
                // path never fires. Arm it here instead: the service loop drops
                // Enable-Tx once TX is idle, matching WSJT-X hound behavior
                // (review catch — Enable-Tx stayed lit after every hound QSO).
                if sequence_advanced
                    && station.quiet_finish
                    && station.state == QsoState::Done
                    && self.settings.disable_tx_after_73
                {
                    self.pending_tx_disable = true;
                }
                // Hound rule: when the Fox answers with our report (we just
                // queued the R+report), move TX onto the FOX's frequency —
                // stock "your Tx frequency is moved to the Fox's". Find the
                // advancing decode (the report from the Fox addressed to us).
                // Gated on station.quiet_finish — the marker call_station sets
                // on TRUE hound QSOs — not just the persistent setting: a CQ run
                // with a stale Hound setting reaching AwaitRr73 must never have
                // its run frequency yanked onto a caller (review catch).
                if sequence_advanced && station.quiet_finish && station.state == QsoState::AwaitRr73
                {
                    if let Some(dx) = station.dxcall.clone() {
                        let mycall = self.settings.mycall.clone();
                        if let Some(d) = decodes.iter().find(|d| {
                            let m = Msg::parse(&d.message);
                            matches!(&m, Msg::Report { to, de, .. }
                                if tempo_core::message::same_call(to, &mycall)
                                    && tempo_core::message::same_call(de, &dx))
                        }) {
                            hound_move_tx = Some(d.freq);
                        }
                    }
                }
                // Remember the report we sent the DX (RST sent) — captured from
                // the sequencer's current outgoing (R)Report, which is replaced
                // by RR73/73 by the time the QSO reaches Done.
                if let Some(snr) = report_in(station.outgoing()) {
                    self.qso_report_sent = Some(snr);
                }
                // Stamp the QSO start (TIME_ON) the first time an exchange is actually
                // under way — when our CQ gets its first reply (answering a station
                // already stamped it in call_station_ctx). One-shot via the None guard;
                // skip once logged so a post-log over can't re-stamp a stale start.
                if self.qso_start_unix.is_none()
                    && !self.qso_logged
                    && station.dxcall.is_some()
                    && !matches!(station.state, QsoState::CallingCq | QsoState::Listening)
                {
                    self.qso_start_unix = Some(now_unix_secs());
                }
                // Auto-log exactly once when the contact is complete. The responder
                // reaches Done (it sent the final 73); the INITIATOR reaches Confirming
                // the moment it rogers with RR73 — and the partner very often never
                // sends a final 73 back, so waiting for Done dropped CQ-side QSOs from
                // the log entirely. But Confirming alone fires the instant the RR73 is
                // QUEUED, before it's actually transmitted; require tx_count >= 1 so we
                // only log once that closing roger has genuinely gone on the air (the
                // contact isn't real until your RR73 is sent).
                //
                // Done requires a REPORT to have been exchanged: `call_station` can
                // SYNTHESIZE a Done straight from a single decoded RR73/73 addressed to
                // us (qso.rs `Station::start`) with no exchange at all — a double-click
                // on that line, or a companion app auto-replying to it across several
                // cycles. Such a seed carries neither a received nor a sent report, so
                // without this each one auto-logged a phantom QSO that never happened
                // (the "3 identical calls I never worked" report). A real completion
                // always exchanged reports, so this never blocks one. Mirrors the manual
                // `log_current_qso` guard. (qso_logged guards the same-Station double;
                // `log_qso` dedups as the final net, since call_station resets it.)
                let report_exchanged =
                    station.rx_report.is_some() || self.qso_report_sent.is_some();
                let loggable = match station.state {
                    QsoState::Done => report_exchanged,
                    QsoState::Confirming => station.tx_count >= 1,
                    _ => false,
                };
                if loggable && !self.qso_logged {
                    // Only CLAIM the contact (which drives auto-log and the CQ-run
                    // resume) when Auto-log is on. With it off, leave qso_logged
                    // false so the completed QSO stays capturable by the cockpit
                    // "Log QSO" button (log_current_qso) — otherwise it is silently
                    // discarded and the button no-ops. Logging it there sets
                    // qso_logged, so a CQ run resumes on the next slot.
                    if self.settings.auto_log {
                        self.qso_logged = true;
                        completed = Some((
                            station.dxcall.clone().unwrap_or_default(),
                            station.dxgrid.clone(),
                            station.rx_report,
                        ));
                    }
                }
            }
            Mode::FieldDay { station, running } => {
                let state_before = station.state;
                let count_before = station.log.qso_count();
                station.observe(decodes, slot);
                sequence_advanced = station.state != state_before;
                // The FD sequencer logged a contact this slot → journal it
                // (after the match — persist_fd_log needs `self` unborrowed).
                fd_logged = station.log.qso_count() > count_before;
                // Run/S&P resilience: once the contact is fully closed (Done and
                // the closing frame has gone out, so `done()` is true), re-arm to
                // work the NEXT station instead of transmitting nothing forever —
                // the FD analogue of Mode::Qso's resume_cq. Keeps the in-station
                // contest log so dupes are still caught. Run re-arms instantly
                // (the happy path reaches Done with no pending); S&P re-arms on
                // the slot after its final RR73 is sent.
                if station.done() {
                    station.rearm(*running);
                    sequence_advanced = true;
                }
            }
        }
        if fd_logged {
            self.persist_fd_log();
        }
        if sequence_advanced {
            self.reset_tx_watchdog();
        }
        if let Some(hz) = hound_move_tx {
            self.set_tx_offset(hz);
        }
        if let Some((dxcall, dxgrid, rx_report)) = completed {
            if self.settings.auto_log {
                let rec = self.qso_record(dxcall, dxgrid, rx_report);
                if self.settings.prompt_to_log {
                    // Hold for the operator's confirm-before-log popup instead of
                    // writing it silently.
                    self.pending_log = Some(rec);
                    self.persist_pending_qso(); // journal before the popup waits
                } else {
                    self.log_qso(rec);
                }
            }
            // Contact closed — the next QSO (incl. a CQ-run's next caller) stamps a
            // fresh start time.
            self.qso_start_unix = None;
        }

        // Run workflow: after a completed QSO while RUNNING (we called CQ), return
        // to calling CQ to work the next caller in a pileup — WSJT-X's default Run
        // behavior. Only once our closing RR73/73 has actually gone out (Confirming
        // with ≥1 TX, or Done) so the DX still receives it; S&P/directed calls do
        // NOT auto-resume CQ (cq_running is false there).
        let resume_cq = self.cq_running
            && self.qso_logged
            && match &self.mode {
                Mode::Qso { station, .. } => {
                    // `done()` is Done AND nothing left to send — NOT a bare `state == Done`.
                    // CONFIRMED on air (KD9TAW ALL.TXT, 2026-07-23): when a caller answers our CQ
                    // with a bare REPORT instead of a grid, we become the RESPONDER while still
                    // running CQ (`cq_running` stays true — unlike a `call_station` click, which
                    // clears it). On their RR73 we reach Done with the closing `73` still QUEUED
                    // in `pending` (`after_tx` clears it only once it transmits). A bare-`Done`
                    // check resumed CQ that same tick and discarded the un-sent 73 — the operator
                    // rogered `KD2VRL R-11`, got `RR73`, and jumped straight to `CQ KD9TAW` with
                    // no 73. `done()` waits the one cycle for the 73 to go out. The INITIATOR
                    // (grid-reply caller) closes with its RR73 via the Confirming arm below, so it
                    // still resumes without waiting on the DX's optional final 73.
                    station.done()
                        || (matches!(station.state, QsoState::Confirming) && station.tx_count >= 1)
                }
                _ => false,
            };
        // Run resilience: a caller who answered but then went silent must not stall the
        // whole run. After `cq_stall_overs` unanswered overs of an in-QSO step (we keep
        // re-sending the same grid/report/roger and they never advance), abandon them and
        // resume CQ so the pileup keeps moving. Default 3 overs; `Some(0)` disables (wait
        // for the operator, stock WSJT-X). Confirming/Done are handled by `resume_cq` above
        // (they auto-log); this fires only for the mid-QSO waits, and never for a bare CQ.
        let stall_cap = self.qso_patience_overs();
        let abandon_stalled = self.cq_running
            && !self.qso_logged
            && stall_cap > 0
            && match &self.mode {
                Mode::Qso { station, .. } => {
                    station.dxcall.is_some()
                        && matches!(
                            station.state,
                            QsoState::AwaitReport | QsoState::AwaitRoger | QsoState::AwaitRr73
                        )
                        && station.tx_count >= stall_cap
                }
                _ => false,
            };
        // Tempo (chat tiers) S&P step cap — the QSO-side half of the 2026-07 cadence
        // rework. On FT8/FT4 the WSJT-X rule stands: an in-QSO step repeats until the
        // partner advances it or the wall-clock watchdog trips (the golden pins it). On a
        // 4 s chat-first mode that reads as "keeps sending and sending", so a chat-tier
        // S&P QSO step stops after TEMPO_QSO_STEP_CAP unanswered overs (≈ WSJT-X-patience
        // 6, NOT chat's 3) and returns cleanly to monitoring — the QSO strip clears, RX
        // continues, nothing hangs with a stuck pending. A chat-tier CQ RUN is already
        // covered by `abandon_stalled` above. Tier-gated at USE time: FT8 can never hit it.
        const TEMPO_QSO_STEP_CAP: u32 = 6;
        let tempo_step_capped = self.app.tier().is_chat()
            && !self.cq_running
            && match &self.mode {
                Mode::Qso { station, .. } => {
                    station.dxcall.is_some()
                        && matches!(
                            station.state,
                            QsoState::AwaitReport | QsoState::AwaitRoger | QsoState::AwaitRr73
                        )
                        && station.tx_count >= TEMPO_QSO_STEP_CAP
                }
                _ => false,
            };
        if tempo_step_capped {
            let mycall = self.settings.mycall.clone();
            let mygrid = self.settings.mygrid.clone();
            self.mode = Mode::Qso {
                station: Box::new(QsoStation::monitoring(&mycall, &mygrid)),
                running: false,
            };
            self.qso_report_sent = None;
            self.qso_start_unix = None;
            self.reset_tx_watchdog();
        }
        if resume_cq || abandon_stalled {
            let mycall = self.settings.mycall.clone();
            let mygrid = self.settings.mygrid.clone();
            let mut s = QsoStation::calling_cq(&mycall, &mygrid);
            s.confirm_with_rrr = self.settings.prefer_rrr;
            s.cq_call_cap = self.cq_call_budget();
            if let Some(d) = &self.cq_dir {
                // The directed run stays directed across the pileup (stock: the
                // edited Tx6 text persists).
                s.override_next(Msg::Cq {
                    de: mycall.clone(),
                    grid: mygrid.clone(),
                    dir: d.clone(),
                });
            }
            self.mode = Mode::Qso {
                station: Box::new(s),
                running: true,
            };
            // Fresh auto-log window for the next contact; keep TX enabled so CQ
            // keeps going out. Clear the QSO-start stamp too — the NEXT caller stamps
            // its own TIME_ON; otherwise this QSO's start would leak into the next
            // contact's logged TIME_ON (e.g. after a manual Log-QSO mid-run).
            self.qso_logged = false;
            self.qso_report_sent = None;
            self.qso_start_unix = None;
            self.reset_tx_watchdog();
        }
    }

    /// The DX's grid for a contact: what the exchange itself carried, else what we decoded
    /// from that station earlier this session.
    ///
    /// Grid backfill (operator report, 2026-07-23): a caller who answers our CQ with a bare
    /// report — `KD9TAW K0SSD +17`, the common FT8 form — carries NO grid in the QSO, so the
    /// contact logged with a blank GRIDSQUARE. But we almost always decoded that station's
    /// grid this session from their own CQ (`CQ HB9XBY JN47`) or a grid message, and the
    /// roster remembers it (the same source the rarity badge backfills from). Fill it in when
    /// the QSO itself gave us nothing — exactly as WSJT-X pre-fills its Log dialog from decode
    /// history. An empty/whitespace grid counts as nothing.
    ///
    /// Shared by the log record and the snapshot's `QsoStatus.dxgrid` so the cockpit's DX Grid
    /// box and the logged GRIDSQUARE resolve identically — they diverged before, and the
    /// operator saw a blank grid on screen for a contact that logged correctly.
    fn dx_grid_resolved(&self, dxcall: &str, dxgrid: Option<String>) -> Option<String> {
        dxgrid
            .filter(|g| !g.trim().is_empty())
            .or_else(|| {
                self.app
                    .inbox
                    .roster
                    .get(dxcall)
                    .and_then(|h| h.grid.clone())
                    .filter(|g| !g.trim().is_empty())
            })
            .or_else(|| {
                // Third and last: a grid we LOGGED for this station before. The session roster
                // only remembers stations heard since launch, so a station worked on a previous
                // day whose CQ we haven't decoded this session still logged with a blank grid —
                // and a blank grid means `worked_grids` never learns it, so NewGrid keeps firing
                // for a grid already worked. Same shape as the NewState defect, and fixed the
                // same way: read something we already own instead of leaving the field empty.
                //
                // Newest record first: a rover's most recent grid is the better answer.
                //
                // ⚠️ This is a LOOKUP, never a guess. Unlike state there is no callsign→grid
                // index, so a station whose grid we have genuinely never seen still logs blank
                // and still reports NewGrid — which is CORRECT: we cannot credit a grid we do
                // not know. Do not "fix" that by inventing one from the callsign prefix.
                self.station
                    .logbook
                    .records()
                    .iter()
                    .rev()
                    .find(|r| tempo_core::message::same_call(&r.call, dxcall))
                    .and_then(|r| r.grid.clone())
                    .filter(|g| !g.trim().is_empty())
            })
    }

    /// Build a [`QsoRecord`] for a completed auto-sequenced QSO from the current
    /// settings (band / dial / tier) and the station's exchanged reports.
    fn qso_record(
        &self,
        dxcall: String,
        dxgrid: Option<String>,
        rx_report: Option<i32>,
    ) -> QsoRecord {
        // ADIF mode must reflect the tier actually used — FT8/FT4 contacts log as
        // FT8/FT4 (award eligibility depends on it), not the native FT1 path.
        let mode = match self.app.tier() {
            Tier::TempoDeep => "TempoDeep",
            Tier::Ft8 => "FT8",
            Tier::Ft4 => "FT4",
            // "FST4" is the ADIF-registered mode name. Unreachable in practice while
            // FST4 is receive-only (no TX means no completed QSO to log), but the
            // right answer the moment that changes.
            Tier::Fst4 => "FST4",
            // FST4W is its own ADIF-registered mode name, not an FST4 submode.
            Tier::Fst4w => "FST4W",
            // "Q65" WITHOUT the submode: that is the ADIF-registered mode name,
            // and ADIF has no field for the period/submode. The tier displays as
            // "Q65-30A" because an operator needs to know which variant is being
            // decoded, but a log entry must carry the registered name or award
            // credit breaks. Same unreachable-while-receive-only note as FST4.
            Tier::Q65 => "Q65",
            // "MSK144" is the ADIF-registered mode name.
            Tier::Msk144 => "MSK144",
            Tier::Jt65 => "JT65",
            // ADIF-registered. Unreachable while receive-only, like the others.
            Tier::Wspr => "WSPR",
            Tier::TempoFast => "TempoFast",
        }
        .to_string();
        // Resolve the DXCC entity (country) at log time — the key field for a
        // DXer. Uses the injected resolver; None in headless tests.
        let country = self
            .station
            .dxcc_resolve
            .as_ref()
            .and_then(|resolve| resolve(&dxcall));
        // Logged FREQ is the actual on-air RF = dial + the TX audio offset (WSJT-X
        // convention), sideband-signed: USB adds the offset, LSB subtracts it. Bare
        // dial alone would log two stations at different audio offsets as identical.
        let off_mhz = self.tx_offset_hz as f64 / 1e6;
        let freq_mhz = if self.settings.sideband.eq_ignore_ascii_case("LSB") {
            self.settings.dial_mhz - off_mhz
        } else {
            self.settings.dial_mhz + off_mhz
        };
        let grid = self.dx_grid_resolved(&dxcall, dxgrid);
        QsoRecord {
            call: dxcall,
            grid,
            country,
            state: None,
            band: self.settings.band.clone(),
            freq_mhz,
            mode,
            // Digital dB SNR reports → ADIF string form ("-12").
            rst_sent: self.qso_report_sent.map(|v| v.to_string()),
            rst_rcvd: rx_report.map(|v| v.to_string()),
            name: None,
            qth: None,
            comment: None,
            notes: None,
            tx_power: None,
            // TIME_ON = when the exchange began (set on answer / first CQ reply);
            // TIME_OFF = now (the contact just completed). Fall back to now if we somehow
            // never stamped a start.
            when_unix: self.qso_start_unix.unwrap_or_else(now_unix_secs),
            time_off_unix: Some(now_unix_secs()),
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: Default::default(),
            qsl_sent: Default::default(),
            credit_granted: Vec::new(),
            credit_submitted: Vec::new(),
            upload: Default::default(),
            ota: Default::default(),
        }
    }

    /// Decodes from the most recent [`Engine::ingest`] (for the network layer).
    pub fn last_decodes(&self) -> &[modes::Decode] {
        &self.last_decodes
    }
    /// The last ingest's decodes as transmitted ON AIR (pre hound rewriting) —
    /// the only form that may leave over UDP.
    pub fn wire_decodes(&self) -> &[modes::Decode] {
        &self.last_wire_decodes
    }

    /// Every decode of the CURRENT period (for UDP Replay) — the early pass +
    /// boundary stragglers reassembled from history; `last_decodes` alone holds
    /// only the most recent ingest's batch.
    pub fn current_period_decodes(&self) -> Vec<modes::Decode> {
        let Some(slot) = self.last_decode_slot else {
            return Vec::new();
        };
        self.decode_history
            .iter()
            .filter(|(s, _)| *s == slot)
            .map(|(_, d)| d.clone())
            .collect()
    }

    /// Test-only: fold synthetic decodes through the same DT-tracking + observe
    /// path as [`Engine::ingest`], without needing a real audio frame.
    #[cfg(test)]
    fn app_pending_count_for_test(&self) -> usize {
        self.app.pending_count()
    }

    #[cfg(test)]
    fn ingest_decodes_for_test(&mut self, decodes: &[modes::Decode], slot: u64) {
        // Mirror the live path's Hound multi-payload split.
        let decodes: Vec<modes::Decode> = self.hound_split(decodes.to_vec());
        let decodes = &decodes[..];
        for d in decodes {
            self.seen_decode = true;
            self.recent_dt.push_back(d.dt.abs());
            if self.recent_dt.len() > DT_WINDOW {
                self.recent_dt.pop_front();
            }
        }
        let pend_before = self.app.pending_count();
        self.app.observe(decodes, slot);
        // An inbound ACK may have marked queued messages delivered (or purged spent
        // ones) inside observe — journal only when the queue actually shrank.
        if self.app.pending_count() != pend_before {
            self.persist_pending_msgs();
        }
        self.observe_modes(decodes, slot);
        // Mirror the live `ingest` QSY hook so tests exercise the same path.
        if self.qsy_active() {
            let mycall = self.settings.mycall.clone();
            for d in decodes {
                if let Some(sender) = Msg::parse(&d.message).sender() {
                    self.qsy.note_heard(sender);
                }
                self.qsy.accept_decode(&d.message, slot, &mycall);
            }
            self.qsy.on_rx_slot(slot);
        }
        // Mirror the live `ingest`: the projected feed + the reply-context history
        // read from the same stores the live path fills.
        for d in decodes.iter() {
            self.decode_history.push_back((slot, d.clone()));
        }
        while self.decode_history.len() > 240 {
            self.decode_history.pop_front();
        }
        self.last_decodes = decodes.to_vec();
    }

    /// Export the Field Day log in `format` ("cabrillo" or "adif"). Returns
    /// `None` unless currently in Field Day mode.
    pub fn export_log(&self, format: &str) -> Option<String> {
        match &self.mode {
            Mode::FieldDay { station, .. } => {
                let freq_khz = (self.settings.dial_mhz * 1000.0).round() as u32;
                match format.to_ascii_lowercase().as_str() {
                    "adif" => Some(station.log.adif()),
                    _ => Some(station.log.cabrillo(freq_khz)),
                }
            }
            _ => None,
        }
    }

    /// Serialize the in-progress Field Day contest log as ADIF for a durable
    /// flush-on-exit. Returns `None` when not in Field Day mode, or when the log
    /// is empty (nothing to persist).
    ///
    /// The FD contest log lives ONLY in memory inside [`Mode::FieldDay`] — there
    /// is no periodic save, and the sole exit hook persists conversation threads.
    /// So a normal quit (or a crash / SIGTERM) drops the whole session's
    /// class/section exchange log, which a solo entrant with no club logger has
    /// no other copy of. The shell's `ExitRequested` handler must call this and
    /// write the result to disk alongside `persist_conversations`. ADIF (not
    /// Cabrillo) because it carries the class/section exchange and is
    /// self-contained — it needs no operating-frequency argument.
    pub fn field_day_log_adif(&self) -> Option<String> {
        let Mode::FieldDay { station, .. } = &self.mode else {
            return None;
        };
        if station.log.qso_count() == 0 {
            return None;
        }
        Some(station.log.adif())
    }

    /// One waterfall row: the Goertzel power spectrum of the **live** captured
    /// audio (the rolling window fed by the radio loop), so the waterfall tracks
    /// real sound-card input continuously. Falls back to the last decoded frame,
    /// then to zeros before any audio has arrived.
    /// Compute one waterfall row (120-bin Goertzel over 200–2900 Hz audio). An EMPTY row for empty
    /// audio — not zeros — so the UI cleanly skips the tick (an all-zeros row scrolls a flat floor
    /// band that reads as broken).
    fn compute_spectrum(audio: &[f32]) -> Spectrum {
        // Widened from 200–2900: shows the full SSB/DATA passband (incl. wide DATA modes) and the
        // filter-slope shelf; the percentile AGC + gain/zero absorb the quiet edges. The 6 kHz
        // Nyquist (12 kHz capture) caps the top; 4000 covers every voice/data passband in use.
        const LO_HZ: f32 = 0.0;
        const HI_HZ: f32 = 4000.0;
        let row = if audio.is_empty() {
            Vec::new()
        } else {
            spectrum::power_spectrum(audio, tempo_fast::SAMPLE_RATE, LO_HZ, HI_HZ, SPECTRUM_BINS)
        };
        Spectrum {
            row,
            lo_hz: f64::from(LO_HZ),
            hi_hz: f64::from(HI_HZ),
            source: "audio".into(),
        }
    }

    /// The waterfall row for a source with NO local capture (the Companion/UDP path), computed
    /// on demand from the last decoded RX buffer.
    ///
    /// This is a FALLBACK ONLY now. Every real capture path publishes to [`SpectrumFeed`] from
    /// the rx-dsp thread (tempo-audio: `rxtap.rs` / `rxdsp.rs`), and `get_spectrum_row` reads
    /// the feed first; the source precedence that used to live here moved to
    /// [`SpectrumFeed::row`], where all three writers converge.
    ///
    /// It moved because the row used to be produced on the radio loop — the one thread that
    /// issues blocking CAT (700 ms, 2500 ms on slow serial). Any CAT stall meant no row was
    /// produced, so the UI redrew the cached row and the waterfall streaked vertically
    /// (operator report, 2026-07-25).
    pub fn spectrum_row(&self) -> Spectrum {
        // `power_spectrum` reads only the LAST FFT_N samples, so convert just that
        // tail rather than the whole retained frame — which at a 1800 s period would
        // be 21.6 M samples rebuilt on every UI tick.
        const TAIL: usize = 4096; // FFT_N in tempo_core::spectrum
        let tail: Vec<f32> = match self.last_rx.as_deref() {
            Some(q) => q[q.len().saturating_sub(TAIL)..]
                .iter()
                .map(|&v| f32::from(v) / 32767.0)
                .collect(),
            None => Vec::new(),
        };
        Self::compute_spectrum(&tail)
    }

    // ---------------------------------------------------------------------------
    // Station-wide delegates.
    //
    // These live on [`StationCore`]; the forwarders keep the Engine call surface
    // (Tauri commands, tests) byte-identical to before the split.
    // ---------------------------------------------------------------------------

    /// See [`StationCore::set_periods_dir`].
    pub fn set_periods_dir(&mut self, dir: &str) {
        self.station.set_periods_dir(dir)
    }

    /// See [`StationCore::periods_dir`].
    pub fn periods_dir(&self) -> Option<String> {
        self.station.periods_dir()
    }

    /// See [`StationCore::set_log_path`].
    pub fn set_log_path(&mut self, path: PathBuf) {
        self.station.set_log_path(path)
    }

    /// See [`StationCore::set_fd_log_path`].
    pub fn set_fd_log_path(&mut self, path: PathBuf) {
        self.station.set_fd_log_path(path)
    }

    /// See [`StationCore::set_pending_qso_path`].
    pub fn set_pending_qso_path(&mut self, path: PathBuf) {
        self.station.set_pending_qso_path(path)
    }

    /// See [`StationCore::set_pending_msgs_path`].
    pub fn set_pending_msgs_path(&mut self, path: PathBuf) {
        self.station.set_pending_msgs_path(path)
    }

    /// See [`StationCore::set_dxcc_resolver`].
    pub fn set_dxcc_resolver(
        &mut self,
        resolve: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) {
        self.station.set_dxcc_resolver(resolve)
    }

    /// See [`StationCore::set_state_resolver`].
    pub fn set_state_resolver(
        &mut self,
        resolve: impl Fn(&str, Option<&str>) -> Option<String> + Send + Sync + 'static,
    ) {
        self.station.set_state_resolver(resolve)
    }

    /// See [`StationCore::set_grid_rarity_resolver`].
    pub fn set_grid_rarity_resolver(
        &mut self,
        resolve: impl Fn(&str) -> Option<u8> + Send + Sync + 'static,
    ) {
        self.station.set_grid_rarity_resolver(resolve)
    }

    /// See [`StationCore::set_lotw_resolver`].
    pub fn set_lotw_resolver(&mut self, resolve: impl Fn(&str) -> bool + Send + Sync + 'static) {
        self.station.set_lotw_resolver(resolve)
    }

    /// See [`StationCore::set_hunt_target`].
    pub fn set_hunt_target(
        &mut self,
        call: &str,
        program: &str,
        reference: &str,
    ) -> Result<(), String> {
        self.station.set_hunt_target(call, program, reference)
    }

    /// See [`StationCore::clear_hunt_target`].
    pub fn clear_hunt_target(&mut self) {
        self.station.clear_hunt_target()
    }

    /// See [`StationCore::hunt_target`].
    pub fn hunt_target(&self) -> Option<(String, String, String)> {
        self.station.hunt_target()
    }

    /// See [`StationCore::park_worked`].
    pub fn park_worked(&self, reference: &str) -> bool {
        self.station.park_worked(reference)
    }

    /// See [`StationCore::set_hunted_parks_import`].
    pub fn set_hunted_parks_import(&mut self, refs: impl IntoIterator<Item = String>) {
        self.station.set_hunted_parks_import(refs)
    }

    /// See [`StationCore::hunted_parks_import_count`].
    pub fn hunted_parks_import_count(&self) -> usize {
        self.station.hunted_parks_import_count()
    }

    /// See [`StationCore::take_pending_uploads`].
    pub fn take_pending_uploads(&mut self) -> Vec<PendingUpload> {
        self.station.take_pending_uploads()
    }

    /// See [`StationCore::requeue_upload`].
    pub fn requeue_upload(&mut self, rec: tempo_core::logbook::QsoRecord, legs: u8, attempts: u8) {
        self.station.requeue_upload(rec, legs, attempts)
    }

    /// See [`StationCore::note_upload`].
    pub fn note_upload(&mut self, note: impl Into<String>, ok: bool) {
        self.station.note_upload(note, ok)
    }

    /// See [`StationCore::set_activation`].
    pub fn set_activation(
        &mut self,
        program: &str,
        reference: &str,
    ) -> Result<(String, String), String> {
        self.station.set_activation(program, reference)
    }

    /// See [`StationCore::clear_activation`].
    pub fn clear_activation(&mut self) {
        self.station.clear_activation()
    }

    /// See [`StationCore::activation`].
    pub fn activation(&self) -> Option<(String, String)> {
        self.station.activation()
    }

    /// See [`StationCore::activation_qso_count`].
    pub fn activation_qso_count(&self) -> usize {
        self.station.activation_qso_count()
    }

    /// See [`StationCore::update_qso`].
    pub fn update_qso(&mut self, index: usize, rec: QsoRecord) -> bool {
        self.station.update_qso(index, rec)
    }

    /// See [`StationCore::mark_qsl_sent`].
    pub fn mark_qsl_sent(&mut self, index: usize, via: tempo_core::logbook::QslVia) -> bool {
        self.station.mark_qsl_sent(index, via)
    }

    /// See [`StationCore::delete_qso`].
    pub fn delete_qso(&mut self, index: usize) -> bool {
        self.station.delete_qso(index)
    }

    /// See [`StationCore::clear_logbook`].
    pub fn clear_logbook(&mut self) -> usize {
        self.station.clear_logbook()
    }

    /// See [`StationCore::import_adif`].
    pub fn import_adif(&mut self, text: &str) -> (usize, usize, usize) {
        self.station.import_adif(text)
    }

    /// See [`StationCore::merge_lotw_report`].
    pub fn merge_lotw_report(&mut self, text: &str) -> tempo_core::reconcile::ReconcileSummary {
        self.station.merge_lotw_report(text)
    }

    /// See [`StationCore::import_pota_log`].
    pub fn import_pota_log(&mut self, text: &str) -> (usize, usize, usize) {
        self.station.import_pota_log(text)
    }

    /// See [`StationCore::merge_lotw_own_echo`].
    pub fn merge_lotw_own_echo(&mut self, text: &str, when_unix: i64) -> usize {
        self.station.merge_lotw_own_echo(text, when_unix)
    }

    /// See [`StationCore::oldest_pending_lotw_date`].
    pub fn oldest_pending_lotw_date(&self) -> Option<String> {
        self.station.oldest_pending_lotw_date()
    }

    /// See [`StationCore::stamp_qrz_upload`].
    pub fn stamp_qrz_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<String>,
    ) -> bool {
        self.station
            .stamp_qrz_upload(pushed, outcome, when_unix, detail)
    }

    /// See [`StationCore::stamp_clublog_upload`].
    pub fn stamp_clublog_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<String>,
    ) -> bool {
        self.station
            .stamp_clublog_upload(pushed, outcome, when_unix, detail)
    }

    /// See [`StationCore::stamp_eqsl_upload`].
    pub fn stamp_eqsl_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<String>,
    ) -> bool {
        self.station
            .stamp_eqsl_upload(pushed, outcome, when_unix, detail)
    }

    /// See [`StationCore::merge_eqsl_report`].
    pub fn merge_eqsl_report(&mut self, text: &str) -> tempo_core::reconcile::ReconcileSummary {
        self.station.merge_eqsl_report(text)
    }

    /// See [`StationCore::merge_qrz_report`].
    pub fn merge_qrz_report(
        &mut self,
        text: &str,
    ) -> (usize, tempo_core::reconcile::ReconcileSummary) {
        self.station.merge_qrz_report(text)
    }

    /// See [`StationCore::get_log`].
    pub fn get_log(&self) -> Vec<QsoRecord> {
        self.station.get_log()
    }

    /// See [`StationCore::confirmation_diagnostics`].
    pub fn confirmation_diagnostics(
        &self,
        now: i64,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> tempo_core::diagnostics::DiagnosticsReport {
        self.station.confirmation_diagnostics(now, resolve)
    }

    /// See [`StationCore::lotw_unsent_indices`].
    pub fn lotw_unsent_indices(&self) -> Vec<usize> {
        self.station.lotw_unsent_indices()
    }

    /// See [`StationCore::stamp_lotw_upload`].
    pub fn stamp_lotw_upload(
        &mut self,
        indices: &[usize],
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<String>,
    ) {
        self.station
            .stamp_lotw_upload(indices, outcome, when_unix, detail)
    }

    /// See [`StationCore::push_sstv_gallery`].
    pub fn push_sstv_gallery(&mut self, entry: crate::dto::SstvGalleryEntry) {
        self.station.push_sstv_gallery(entry)
    }

    /// See [`StationCore::set_sstv_gallery_fsk_id`].
    pub fn set_sstv_gallery_fsk_id(&mut self, path: &str, fsk_id: String) {
        self.station.set_sstv_gallery_fsk_id(path, fsk_id)
    }

    /// See [`StationCore::load_sstv_gallery`].
    pub fn load_sstv_gallery(&mut self, entries: Vec<crate::dto::SstvGalleryEntry>) {
        self.station.load_sstv_gallery(entries)
    }

    /// See [`StationCore::sstv_gallery`].
    pub fn sstv_gallery(&self) -> &[crate::dto::SstvGalleryEntry] {
        self.station.sstv_gallery()
    }

    /// See [`StationCore::set_clock_offset_ms`].
    pub fn set_clock_offset_ms(&mut self, ms: Option<i64>) {
        self.station.set_clock_offset_ms(ms)
    }

    /// See [`StationCore::clock_offset_ms`].
    pub fn clock_offset_ms(&self) -> Option<i64> {
        self.station.clock_offset_ms()
    }

    /// See [`StationCore::take_all_txt_pending`].
    pub fn take_all_txt_pending(&mut self) -> Vec<String> {
        self.station.take_all_txt_pending()
    }

    /// See [`StationCore::export_logbook`].
    pub fn export_logbook(&self, format: &str) -> String {
        self.station.export_logbook(format)
    }

    /// Two-instance freshness: re-read + reconcile the shared log iff another instance touched
    /// it (mtime-gated, so a no-op stat when unchanged). Call on the Needed-board poll so a
    /// monitoring radio's needs never go stale relative to the other radio. Returns true if it
    /// actually re-read. See [`StationCore::sync_shared_log_if_changed`].
    pub fn sync_shared_log_if_changed(&mut self) -> bool {
        self.station.sync_shared_log_if_changed()
    }
}

/// The signal report carried by an outgoing `Report` / `RReport` message, if
/// that's the message a station is about to send (used to record RST sent).
fn report_in(msg: Option<Msg>) -> Option<i32> {
    match msg {
        Some(Msg::Report { snr, .. }) | Some(Msg::RReport { snr, .. }) => Some(snr),
        _ => None,
    }
}

/// Current wall-clock time as Unix seconds (UTC), 0 before the epoch.
pub(crate) fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Current wall-clock time as Unix MILLISECONDS (UTC), 0 before the epoch. The
/// RTTY auto-sequencer's SINGLE clock epoch: the RX-feed thread (`feed`) and the
/// service tick (`tick` / `on_tx_complete`) MUST share it — never a service-loop
/// local millisecond clock, or the sequencer's timeouts would run off two epochs.
fn now_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One station answering my CQ this slot, distilled for best-caller ranking (W1.4).
#[derive(Debug, Clone)]
struct CqCaller {
    /// Position of this caller's decode in the slot's decode slice.
    decode_idx: usize,
    /// SNR we decoded them at (dB).
    snr: i32,
    /// Whether this same station ALSO produced a `CQ` decode this slot — i.e. they
    /// were themselves calling CQ (the `cq_first` strategy prefers such a caller).
    calling_cq: bool,
    /// Their Maidenhead grid when the answer carried one (a `Grid` reply with a
    /// non-empty locator); `None` for bare-report or grid-less answers.
    grid: Option<String>,
}

/// Pick which answering caller to work, per the operator's best-caller strategy.
/// Returns the index into `callers` of the winner, or `None` when the list is
/// empty or every caller is below the `min_snr` floor.
///
/// Strategies: `first` (earliest decode = stock WSJT-X), `strongest` (max SNR),
/// `farthest` (greatest great-circle distance from `mygrid`; grid-less callers
/// always rank last), `cq_first` (prefer a caller who was themselves calling CQ,
/// then the strongest of them, else fall back to strongest). Ties always break to
/// the earliest decode so the choice is deterministic. An unknown strategy string
/// behaves as `first`.
fn pick_caller(
    callers: &[CqCaller],
    strategy: &str,
    min_snr: Option<i32>,
    mygrid: &str,
) -> Option<usize> {
    let mut eligible: Vec<usize> = callers
        .iter()
        .enumerate()
        .filter(|(_, c)| min_snr.is_none_or(|floor| c.snr >= floor))
        .map(|(i, _)| i)
        .collect();
    if eligible.is_empty() {
        return None;
    }
    match strategy {
        // Loudest wins; ties fall to the earliest decode.
        "strongest" => {
            eligible.sort_by(|&a, &b| callers[b].snr.cmp(&callers[a].snr).then(a.cmp(&b)));
        }
        // Greatest great-circle distance from my grid; grid-less callers
        // (distance NEG_INFINITY) always sort last.
        "farthest" => {
            let me = maidenhead_center(mygrid);
            let dist = |c: &CqCaller| -> f64 {
                match (me, c.grid.as_deref().and_then(maidenhead_center)) {
                    (Some(m), Some(g)) => haversine_km(m, g),
                    _ => f64::NEG_INFINITY,
                }
            };
            eligible.sort_by(|&a, &b| {
                dist(&callers[b])
                    .total_cmp(&dist(&callers[a]))
                    .then(a.cmp(&b))
            });
        }
        // Prefer someone who was themselves calling CQ, then the strongest of
        // them; with no CQ-caller in the pileup this reduces to "strongest".
        "cq_first" => {
            eligible.sort_by(|&a, &b| {
                callers[b]
                    .calling_cq
                    .cmp(&callers[a].calling_cq)
                    .then(callers[b].snr.cmp(&callers[a].snr))
                    .then(a.cmp(&b))
            });
        }
        // "first" (and any unknown value): `eligible` is already in ascending
        // decode order, so the earliest caller stays at the front.
        _ => {}
    }
    eligible.first().copied()
}

/// Reorder a slot's decodes so the sequencer's first-heard auto-answer locks onto
/// the operator's best-caller pick (W1.4). Every non-answer decode is preserved in
/// place; among the stations answering my CQ, only the chosen one is kept (so the
/// `CallingCq` arms in [`tempo_core::qso`] commit to it). When no caller clears the
/// SNR floor, all answers are dropped and the run keeps calling CQ. Callers with no
/// answer at all yield the slice unchanged.
fn best_caller_decodes(
    decodes: &[modes::Decode],
    mycall: &str,
    strategy: &str,
    min_snr: Option<i32>,
    mygrid: &str,
) -> Vec<modes::Decode> {
    // Calls heard calling CQ this slot — input for the `cq_first` strategy.
    let cq_callers: Vec<String> = decodes
        .iter()
        .filter_map(|d| match Msg::parse(&d.message) {
            Msg::Cq { de, .. } => Some(de),
            _ => None,
        })
        .collect();
    // Distill the stations answering MY CQ this slot: a grid reply or a bare
    // report addressed to me — the two forms the sequencer auto-answers from
    // CallingCq.
    let mut callers = Vec::new();
    for (idx, d) in decodes.iter().enumerate() {
        let (de, grid) = match Msg::parse(&d.message) {
            Msg::Grid { to, de, grid } if same_call(&to, mycall) => {
                (de, (!grid.is_empty()).then_some(grid))
            }
            Msg::Report { to, de, .. } if same_call(&to, mycall) => (de, None),
            _ => continue,
        };
        callers.push(CqCaller {
            decode_idx: idx,
            snr: d.snr,
            calling_cq: cq_callers.iter().any(|c| same_call(c, &de)),
            grid,
        });
    }
    // No one answered → nothing to arbitrate; hand the slice back unchanged.
    if callers.is_empty() {
        return decodes.to_vec();
    }
    let keep = pick_caller(&callers, strategy, min_snr, mygrid).map(|i| callers[i].decode_idx);
    let answers: Vec<usize> = callers.iter().map(|c| c.decode_idx).collect();
    decodes
        .iter()
        .enumerate()
        .filter(|(i, _)| !answers.contains(i) || Some(*i) == keep)
        .map(|(_, d)| d.clone())
        .collect()
}

/// Maidenhead locator → (lat, lon) at the grid-square center; `None` when
/// malformed. A local 4/6-char copy so `tempo-app` needn't depend on the
/// propagation crate just to rank callers by distance.
fn maidenhead_center(grid: &str) -> Option<(f64, f64)> {
    let g = grid.trim().to_uppercase();
    let b = g.as_bytes();
    if b.len() < 4 || !b[2].is_ascii_digit() || !b[3].is_ascii_digit() {
        return None;
    }
    let f_lon = b[0].checked_sub(b'A')? as f64;
    let f_lat = b[1].checked_sub(b'A')? as f64;
    if f_lon > 17.0 || f_lat > 17.0 {
        return None;
    }
    let s_lon = (b[2] - b'0') as f64;
    let s_lat = (b[3] - b'0') as f64;
    let mut lon = -180.0 + f_lon * 20.0 + s_lon * 2.0;
    let mut lat = -90.0 + f_lat * 10.0 + s_lat * 1.0;
    if b.len() >= 6 {
        let ss_lon = b[4].checked_sub(b'A')? as f64;
        let ss_lat = b[5].checked_sub(b'A')? as f64;
        if ss_lon > 23.0 || ss_lat > 23.0 {
            return None;
        }
        lon += ss_lon * (5.0 / 60.0) + (2.5 / 60.0);
        lat += ss_lat * (2.5 / 60.0) + (1.25 / 60.0);
    } else {
        lon += 1.0;
        lat += 0.5;
    }
    Some((lat, lon))
}

/// Great-circle distance in km between two (lat, lon) points (haversine, R = 6371).
fn haversine_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    const R_KM: f64 = 6371.0;
    let (lat1, lon1) = (a.0.to_radians(), a.1.to_radians());
    let (lat2, lon2) = (b.0.to_radians(), b.1.to_radians());
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R_KM * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use modes::Decode;

    fn dec_dt(msg: &str, dt: f32) -> Decode {
        Decode {
            message: msg.to_string(),
            sync: 1.0,
            snr: -5,
            dt,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode: None,
        }
    }

    #[test]
    fn ttl_zero_keeps_every_station_forever() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let mut s = e.settings().clone();
        s.aprs_station_ttl_min = 0;
        e.apply_settings(s);
        e.push_aprs_heard(aprs_pkt(
            "W0OLD",
            "!4903.50N/07201.75W-",
            1000,
            AprsSource::Rf,
        ));
        // A year later the station is still in the view, and ttl_min = 0 travels to the UI
        // so the fade path knows to stand down too.
        let v = e.aprs_stations(1000 + 365 * 24 * 3600);
        assert_eq!(v.stations.len(), 1);
        assert_eq!(v.ttl_min, 0);
    }

    #[test]
    fn cw_queue_expands_gates_and_aborts() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        e.set_cw_wpm(28);
        assert_eq!(e.cw_wpm(), 28);
        e.set_cw_wpm(999); // out of range → clamps to 50
        assert_eq!(e.cw_wpm(), 50);

        // A macro expands using the operator's call and queues WORD-BY-WORD; the loop pops
        // one word per ready tick (so it can pace the send and Stop can drop the rest).
        e.send_cw("CQ CQ DE {MYCALL} K");
        for w in ["CQ", "CQ", "DE", "W9XYZ", "K"] {
            assert_eq!(e.poll_cw_one(), Some(w.to_string()));
        }
        assert_eq!(e.poll_cw_one(), None, "drained");

        // A deliberate CW send always transmits: even after Stop TX disabled it, send_cw RE-ARMS
        // TX and the word keys immediately (the F-key fidelity fix — manual keying is never held).
        e.set_tx_enabled(false);
        e.send_cw("TEST");
        assert!(e.tx_enabled(), "a deliberate CW send re-arms TX");
        assert_eq!(
            e.poll_cw_one(),
            Some("TEST".to_string()),
            "the send keys immediately — no held-queue trap after a halt"
        );

        // Abort clears the WHOLE remaining queue (the un-keyed words) and raises the one-shot
        // flag for the loop — this is what makes Stop TX interrupt a long macro.
        e.send_cw("A LONG MESSAGE");
        e.stop_cw();
        assert!(e.take_cw_abort());
        assert!(!e.take_cw_abort(), "abort is one-shot");
        assert_eq!(e.poll_cw_one(), None, "abort cleared the queue");
    }

    #[test]
    fn rtty_launches_silent_and_arming_rx_creates_no_tx_state() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // Launch state: nothing queued, nothing pollable, no abort pending — the
        // radio loop's RTTY dispatch can never key at startup.
        assert_eq!(e.poll_rtty_one(), None);
        assert!(!e.rtty_state().sending);
        assert!(!e.take_rtty_abort());
        // Arming the RX decoder is RX-only: still nothing pollable.
        e.set_rtty_armed(true);
        assert_eq!(e.poll_rtty_one(), None);
        assert!(!e.rtty_state().sending);
    }

    #[test]
    fn rtty_send_validates_every_tx_gate() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // Not in the RTTY section → refused: the section OWNS the rig for keying
        // (the FT8/FT1 sequencer runs only in Digital, RTTY only in Rtty).
        assert!(e.rtty_send_text("CQ TEST").is_err());
        // Entering the RTTY section arms TX (a manual mode, like CW) — arming
        // alone keys nothing; the queue stays empty until an explicit send.
        e.set_operating_mode("rtty", false);
        assert!(e.tx_enabled());
        assert_eq!(e.poll_rtty_one(), None);
        // Monitor off → refused with the reason (never a silent hold).
        e.set_tx_enabled(false);
        assert!(e
            .rtty_send_text("CQ TEST")
            .unwrap_err()
            .contains("TX is off"));
        e.set_tx_enabled(true);
        // Tune carrier up → refused.
        e.set_tune(true);
        assert!(e.rtty_send_text("CQ TEST").unwrap_err().contains("Tune"));
        e.set_tune(false);
        // License lockout: a Technician has no 20 m data privilege at all.
        e.set_license_class("technician");
        e.set_frequency(14.083, "20m", "LSB");
        assert!(!e.tx_allowed());
        assert!(e.rtty_send_text("CQ TEST").unwrap_err().contains("license"));
        // ...but 10 m data (28.080–28.100 window) is the Tech HF grant → allowed.
        // (The cross-band QSY halts TX — the standing band-change invariant — so
        // the operator re-arms, exactly like the TopBar TX button.)
        e.set_frequency(28.083, "10m", "LSB");
        assert!(
            !e.tx_enabled(),
            "a band change halts TX (existing invariant)"
        );
        e.set_tx_enabled(true);
        assert!(e.tx_allowed());
        e.rtty_send_text("CQ TEST").unwrap();
        assert_eq!(e.poll_rtty_one(), Some("CQ TEST".to_string()));
    }

    #[test]
    fn rtty_queue_uppercases_filters_and_stops() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_operating_mode("rtty", false);
        // Uppercased + filtered to the ITA2 charset ('%' has no mapping → dropped),
        // so exactly what's queued is what keys.
        e.rtty_send_text("cq de w9xyz 100%").unwrap();
        assert_eq!(e.poll_rtty_one(), Some("CQ DE W9XYZ 100".to_string()));
        assert_eq!(e.poll_rtty_one(), None, "drained");
        assert!(!e.rtty_state().sending, "drained queue is not 'sending'");
        // All-unmappable text refuses outright — nothing would key.
        assert!(e.rtty_send_text("%*+=").is_err());
        // A single over is length-bounded, so one pasted blob can never key past
        // the watchdog ceiling on its own (the watchdog checks between messages).
        assert!(e
            .rtty_send_text(&"A".repeat(1001))
            .unwrap_err()
            .contains("too long"));
        assert!(e.rtty_send_text(&"A".repeat(1000)).is_ok());
        assert_eq!(e.poll_rtty_one().unwrap().len(), 1000);
        // Disarming TX (Monitor off) aborts + DROPS the queue — nothing keys on a
        // later re-enable (mirrors the CW disarm semantics).
        e.rtty_send_text("TEST").unwrap();
        assert!(e.rtty_state().sending, "queued = sending indicator on");
        e.set_tx_enabled(false);
        assert!(e.take_rtty_abort(), "disarm aborts the over in flight");
        e.set_tx_enabled(true);
        assert_eq!(e.poll_rtty_one(), None, "disarm dropped the queue");
        // Stop drops the queue + arms the one-shot abort (the loop unkeys).
        e.rtty_send_text("A LONG MESSAGE").unwrap();
        e.rtty_stop();
        assert!(e.take_rtty_abort());
        assert!(!e.take_rtty_abort(), "abort is one-shot");
        assert_eq!(e.poll_rtty_one(), None, "stop cleared the queue");
    }

    #[test]
    fn rtty_halt_tx_aborts_clears_and_stays_stopped() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_operating_mode("rtty", false);
        e.rtty_send_text("CQ CQ DE W9XYZ").unwrap();
        e.halt_tx();
        assert!(
            e.take_rtty_abort(),
            "halt aborts the over in flight (unkey)"
        );
        assert!(!e.tx_enabled(), "halt disarms TX — stopped stays stopped");
        assert_eq!(e.poll_rtty_one(), None, "nothing keys while disarmed");
        e.set_tx_enabled(true);
        assert_eq!(e.poll_rtty_one(), None, "halt dropped the queued messages");
    }

    #[test]
    fn rtty_and_ft8_sequencers_never_key_together() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // RTTY owns the rig: the FT8/FT1 slot sequencer transmits NOTHING even
        // with TX armed and a CQ queued (poll_tx is gated off for non-Digital).
        e.set_operating_mode("rtty", false);
        e.call_cq(None).unwrap();
        for slot in 0..4 {
            assert!(
                e.poll_tx(slot).is_empty(),
                "no FT8 keying while RTTY owns the rig"
            );
        }
        // And vice versa: the RTTY queue is HELD while Digital owns the rig.
        e.rtty_send_text("CQ TEST").unwrap();
        e.set_operating_mode("digital", false);
        assert_eq!(
            e.poll_rtty_one(),
            None,
            "no RTTY keying while Digital owns the rig"
        );
        // Back in the RTTY section the held message keys normally.
        e.set_operating_mode("rtty", false);
        assert_eq!(e.poll_rtty_one(), Some("CQ TEST".to_string()));
    }

    #[test]
    fn rtty_fsk_refuses_ptt_and_data_on_the_same_line() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let mut s = e.settings().clone();
        s.rtty_backend = "fsk".into();
        s.rtty_fsk_port = "COM7".into();
        s.rtty_fsk_line = "dtr".into();
        s.ptt_method = "dtr".into();
        s.ptt_serial_port = "COM7".into();
        e.apply_settings(s);
        e.set_operating_mode("rtty", false);
        let err = e.rtty_send_text("CQ").unwrap_err();
        assert!(err.contains("same control line"), "{err}");
        // PTT on the OTHER line of the same port is the classic legal wiring
        // (DTR = FSK data, RTS = PTT).
        let mut s = e.settings().clone();
        s.ptt_method = "rts".into();
        e.apply_settings(s);
        e.set_operating_mode("rtty", false);
        e.rtty_send_text("CQ").unwrap();
    }

    #[test]
    fn rtty_fsk_port_falls_back_to_the_cat_serial_port() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert_eq!(e.rtty_fsk_port(), None, "AFSK never opens a keyline");
        let mut s = e.settings().clone();
        s.rtty_backend = "fsk".into();
        s.serial_port = "COM5".into();
        e.apply_settings(s);
        assert_eq!(
            e.rtty_fsk_port().as_deref(),
            Some("COM5"),
            "empty FSK port = the CAT serial port"
        );
        let mut s = e.settings().clone();
        s.rtty_fsk_port = "COM8".into();
        e.apply_settings(s);
        assert_eq!(e.rtty_fsk_port().as_deref(), Some("COM8"));
        // State the cockpit polls: backend + baud/shift ride rtty_state.
        let st = e.rtty_state();
        assert_eq!(st.backend, "fsk");
        assert_eq!(st.baud, 45.45);
        assert_eq!(st.shift_hz, 170);
    }

    // -- RTTY auto-sequencer (the pure state machine wired to TX + the logbook) --

    /// A headless RTTY engine armed for Auto: in the RTTY section (which arms TX),
    /// at the default frequency the default license can key.
    fn rtty_auto_engine() -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_operating_mode("rtty", false); // arms TX (a manual mode, like CW)
        assert!(e.tx_enabled() && e.tx_allowed(), "gate open for the tests");
        e.set_rtty_auto(true);
        e
    }

    fn rtty_decoded(text: &str) -> Vec<tempo_core::rtty::DecodedChar> {
        text.chars()
            .map(|ch| tempo_core::rtty::DecodedChar {
                ch,
                confidence: 0.9,
            })
            .collect()
    }

    #[test]
    fn rtty_auto_never_self_starts_on_a_heard_cq() {
        let mut e = rtty_auto_engine();
        // A CQ arrives while Idle — the machine only ACCUMULATES it (ARRL 6.4):
        // toggling Auto + feeding a CQ can NEVER key up.
        e.push_rtty_decode(&rtty_decoded("CQ CQ CQ DE W1AW W1AW K\n"), 0.0, true);
        let s = e.rtty_state();
        assert!(s.auto);
        assert_eq!(s.seq_state, "idle", "a heard CQ never transmits");
        assert!(!s.sending, "nothing queued");
        assert_eq!(e.poll_rtty_one(), None);
        // ...but the CQ IS surfaced for the operator to click.
        assert_eq!(s.heard_cq.as_deref(), Some("W1AW"));
    }

    #[test]
    fn rtty_auto_cq_enqueues_a_cq_and_calls() {
        let mut e = rtty_auto_engine();
        e.rtty_auto_cq().unwrap();
        assert_eq!(e.rtty_state().seq_state, "calling_cq");
        let msg = e.poll_rtty_one().expect("a CQ is queued");
        assert!(msg.contains("CQ") && msg.contains("W9XYZ"), "cq: {msg}");
        // Auto off → refused with a reason (never a silent no-op).
        let mut e2 = Engine::new("W9XYZ", "EN61", 0);
        e2.set_operating_mode("rtty", false);
        assert!(e2.rtty_auto_cq().unwrap_err().contains("Auto"));
    }

    #[test]
    fn rtty_auto_answer_then_exchange_enqueues_our_exchange() {
        let mut e = rtty_auto_engine();
        e.rtty_auto_answer("W1AW").unwrap();
        assert_eq!(e.rtty_state().seq_state, "answering");
        assert!(e.poll_rtty_one().unwrap().contains("W1AW DE W9XYZ"));
        // The runner comes back with his exchange → we send ours.
        e.push_rtty_decode(
            &rtty_decoded("W9XYZ DE W1AW UR RST 599 599 NAME BOB QTH BOSTON K\n"),
            0.0,
            true,
        );
        assert_eq!(e.rtty_state().seq_state, "exchange_sent");
        let msg = e.poll_rtty_one().expect("our exchange is queued");
        assert!(msg.contains("599"), "our exchange: {msg}");
    }

    #[test]
    fn rtty_auto_full_runner_qso_logs_a_record() {
        let mut e = rtty_auto_engine();
        e.rtty_auto_cq().unwrap();
        assert!(e.poll_rtty_one().is_some(), "drain the CQ");
        // W1AW answers directed → we send the exchange.
        e.push_rtty_decode(&rtty_decoded("W9XYZ DE W1AW W1AW K\n"), 0.0, true);
        assert_eq!(e.rtty_state().peer.as_deref(), Some("W1AW"));
        assert!(e.poll_rtty_one().is_some(), "drain our exchange");
        // His exchange comes back → log the contact + send the closing.
        e.push_rtty_decode(
            &rtty_decoded("W9XYZ DE W1AW R UR RST 599 599 NAME BOB QTH BOSTON K\n"),
            0.0,
            true,
        );
        assert_eq!(e.rtty_state().seq_state, "confirmed");
        let log = e.get_log();
        assert_eq!(log.len(), 1, "the QSO was auto-logged exactly once");
        assert_eq!(log[0].call, "W1AW");
        assert_eq!(log[0].mode, "RTTY", "logs as RTTY (award eligibility)");
        assert_eq!(log[0].rst_rcvd.as_deref(), Some("599"));
        assert_eq!(log[0].name.as_deref(), Some("BOB"), "copied exchange");
        assert_eq!(log[0].qth.as_deref(), Some("BOSTON"), "copied exchange");
    }

    #[test]
    fn rtty_auto_abort_clears_the_queue_and_returns_to_idle() {
        let mut e = rtty_auto_engine();
        e.rtty_auto_cq().unwrap();
        assert_eq!(e.rtty_state().seq_state, "calling_cq");
        e.rtty_auto_abort();
        assert_eq!(e.rtty_state().seq_state, "idle");
        assert!(e.take_rtty_abort(), "abort unkeys the over in flight");
        assert_eq!(e.poll_rtty_one(), None, "the queue was dropped");
    }

    #[test]
    fn voice_keyer_plays_gated_and_aborts_and_records() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
                                // Queue a voice message → the loop drains it once.
        e.send_voice(vec![0.1, 0.2, 0.3]);
        assert_eq!(e.poll_voice(), Some(vec![0.1, 0.2, 0.3]));
        assert!(e.poll_voice().is_none(), "drained");

        // Gated by Monitor: with TX disabled nothing is queued/played.
        e.set_tx_enabled(false);
        e.send_voice(vec![0.5]);
        assert!(
            e.poll_voice().is_none(),
            "no voice played while TX is disabled"
        );
        e.set_tx_enabled(true);

        // Abort drops the pending message + raises the one-shot flag.
        e.send_voice(vec![0.9; 100]);
        e.stop_voice();
        assert!(e.take_voice_abort());
        assert!(!e.take_voice_abort(), "abort is one-shot");
        assert!(
            e.poll_voice().is_none(),
            "abort dropped the pending message"
        );

        // Recording accumulates captured audio only while recording, and take resets it.
        assert!(!e.is_recording());
        e.push_record_samples(&[1.0]); // ignored — not recording
        e.start_recording();
        assert!(e.is_recording());
        e.push_record_samples(&[0.1, 0.2]);
        e.push_record_samples(&[0.3]);
        let rec = e.stop_recording();
        assert_eq!(
            rec,
            vec![0.1, 0.2, 0.3],
            "captured only what arrived while recording"
        );
        assert!(!e.is_recording());
        assert!(e.stop_recording().is_empty(), "buffer taken/reset");
    }

    #[test]
    fn qso_recording_signals_path_and_persists_in_snapshot() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert!(!e.is_qso_recording());
        assert!(e.qso_record_path().is_none());
        assert!(!e.snapshot().radio.qso_recording);

        e.start_qso_recording("/tmp/nexus/recordings/qso-1.wav");
        assert!(e.is_qso_recording());
        assert_eq!(
            e.qso_record_path().as_deref(),
            Some("/tmp/nexus/recordings/qso-1.wav")
        );
        assert!(
            e.snapshot().radio.qso_recording,
            "REC badge rides the snapshot"
        );

        e.stop_qso_recording();
        assert!(!e.is_qso_recording());
        assert!(e.qso_record_path().is_none(), "path cleared on stop");
        assert!(!e.snapshot().radio.qso_recording);
    }

    #[test]
    fn rtty_arm_gates_tap_accumulates_ring_and_caps() {
        use tempo_core::rtty::DecodedChar;
        let mut e = Engine::new("W9XYZ", "EN61", 0);

        // Disarmed = zero added work: the tap stays empty however much audio flows.
        e.feed_rx_audio(&[0.0; 256]);
        assert!(e.take_rtty_audio().is_empty(), "disarmed tap never fills");
        assert!(!e.rtty_state().armed);

        // Armed: RX audio accumulates; take() drains it for the decode thread.
        e.set_rtty_armed(true);
        e.feed_rx_audio(&[0.0; 256]);
        e.feed_rx_audio(&[0.0; 100]);
        assert_eq!(e.take_rtty_audio().len(), 356);
        assert!(e.take_rtty_audio().is_empty(), "take drains");

        // Synthesized decode push → state carries text + parallel confidence + AFC.
        let chars: Vec<DecodedChar> = "CQ DE W1ABC"
            .chars()
            .map(|ch| DecodedChar {
                ch,
                confidence: 0.9,
            })
            .collect();
        e.push_rtty_decode(&chars, -12.5, true);
        let s = e.rtty_state();
        assert!(s.armed);
        assert_eq!(s.text, "CQ DE W1ABC");
        assert_eq!(s.conf.len(), s.text.chars().count());
        assert_eq!(s.conf[0], 90, "0..1 confidence maps to 0..100");
        assert!((s.afc_hz + 12.5).abs() < 1e-6);
        assert!(s.afc_locked);

        // The ring caps at 4000 chars — oldest drop off the front.
        let many = vec![
            DecodedChar {
                ch: 'X',
                confidence: 1.0,
            };
            RTTY_TEXT_CAP + 500
        ];
        e.push_rtty_decode(&many, 0.0, true);
        assert_eq!(e.rtty_state().text.chars().count(), RTTY_TEXT_CAP);

        // Disarm: the tap stops immediately, but the transcript stays readable.
        e.set_rtty_armed(false);
        e.feed_rx_audio(&[0.0; 64]);
        assert!(e.take_rtty_audio().is_empty());
        let s = e.rtty_state();
        assert!(!s.armed);
        assert_eq!(
            s.text.chars().count(),
            RTTY_TEXT_CAP,
            "transcript survives disarm"
        );

        // Re-arming starts a fresh transcript (a new copy session).
        e.set_rtty_armed(true);
        assert!(e.rtty_state().text.is_empty());
    }

    #[test]
    fn sstv_arm_progress_and_gallery_cap() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);

        // Disarmed = no tap fill, no progress.
        e.feed_rx_audio(&[0.0; 128]);
        assert!(e.take_sstv_audio().is_empty());
        assert!(e.sstv_progress().is_none());

        // Armed: audio accumulates; the decode thread publishes progress.
        e.set_sstv_armed(true);
        e.feed_rx_audio(&[0.0; 128]);
        assert_eq!(e.take_sstv_audio().len(), 128);
        e.set_sstv_progress(Some(SstvProgress {
            mode: "Scottie 1".into(),
            lines_total: 256,
            lines_done: 40,
            preview_w: 2,
            preview_h: 1,
            preview_rgb: vec![1, 2, 3, 4, 5, 6],
            hedr_shift_hz: 0.0,
        }));
        assert_eq!(e.sstv_progress().unwrap().lines_done, 40);

        // A completed image lands in the session gallery (newest last), capped.
        for i in 0..(SSTV_GALLERY_CAP + 3) {
            e.push_sstv_gallery(crate::dto::SstvGalleryEntry {
                path: format!("/tmp/img{i}.bmp"),
                mode: "Robot 36".into(),
                finished_utc: "2026-07-17T00:00:00Z".into(),
                freq_mhz: 14.230,
                lines: 240,
                fsk_id: None,
            });
        }
        assert_eq!(e.sstv_gallery().len(), SSTV_GALLERY_CAP, "gallery caps");
        assert_eq!(
            e.sstv_gallery().last().unwrap().path,
            format!("/tmp/img{}.bmp", SSTV_GALLERY_CAP + 2),
            "newest kept"
        );

        // Disarm drops the tap + in-flight progress but keeps the gallery.
        e.set_sstv_armed(false);
        e.feed_rx_audio(&[0.0; 64]);
        assert!(e.take_sstv_audio().is_empty());
        assert!(e.sstv_progress().is_none());
        assert_eq!(e.sstv_gallery().len(), SSTV_GALLERY_CAP);

        // Startup seed replaces the session list (and caps defensively).
        e.load_sstv_gallery(vec![crate::dto::SstvGalleryEntry::default()]);
        assert_eq!(e.sstv_gallery().len(), 1);
    }

    /// A Phone-armed engine on a legal 20 m phone frequency — the precondition for
    /// an SSTV send. Extra class + 14.290 USB clears `tx_allowed` for the whole SSB
    /// passband; entering Phone arms TX (arming keys nothing by itself).
    fn phone_armed_engine() -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_license_class("extra");
        e.set_frequency(14.290, "20m", "USB");
        e.set_operating_mode("phone", false);
        assert!(e.tx_enabled(), "entering Phone arms TX");
        assert!(e.tx_allowed(), "14.290 USB is a phone segment for Extra");
        e
    }

    /// ~2 s of 12 kHz PCM — a stand-in for an encoded image, well under every budget.
    fn sstv_img_samples() -> Vec<f32> {
        vec![0.0; 24_000]
    }

    #[test]
    fn sstv_send_commands_a_data_submode_so_the_codec_modulates() {
        // SSTV rides Phone (plain USB/LSB = the rig takes TX audio from the MIC). While an
        // image is queued or on the air the rig must be commanded a DATA submode
        // (PKTUSB/PKTLSB) so the SOUNDCARD codec routes to the modulator — otherwise PTT keys
        // with zero RF. It must revert to plain SSB when idle so live voice still uses the mic.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_license_class("extra");
        e.set_frequency(14.290, "20m", "USB");
        e.set_operating_mode("phone", false);
        assert_eq!(
            e.rig_mode_effective(),
            "USB",
            "idle Phone → plain USB (voice = mic)"
        );
        // Queue an image → DATA submode commanded BEFORE the loop keys PTT.
        e.sstv_send(sstv_img_samples(), "PD-120".into()).unwrap();
        assert_eq!(
            e.rig_mode_effective(),
            "PKTUSB",
            "queued SSTV → DATA-USB (codec-routed)"
        );
        // Loop takes the job (sstv_tx → None) and marks it sending → still DATA on the air.
        let _ = e.poll_sstv_tx();
        e.set_sstv_sending(true);
        assert_eq!(
            e.rig_mode_effective(),
            "PKTUSB",
            "sending SSTV → still DATA"
        );
        // Image done → plain USB restores so the next voice PTT keys the mic, not the codec.
        e.set_sstv_sending(false);
        assert_eq!(e.rig_mode_effective(), "USB", "idle again → plain USB");
    }

    #[test]
    fn sstv_send_validates_every_tx_gate() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // Not in Phone → refused: SSTV rides the phone segment (USB voice audio).
        assert!(e
            .sstv_send(sstv_img_samples(), "PD-120".into())
            .unwrap_err()
            .contains("Phone"));

        // Enter Phone on a phone freq → arms TX; arming keys NOTHING (launch-safety).
        e.set_license_class("extra");
        e.set_frequency(14.290, "20m", "USB");
        e.set_operating_mode("phone", false);
        assert!(e.tx_enabled());
        assert!(
            e.poll_sstv_tx().is_none(),
            "arming a mode never queues an image"
        );

        // Monitor off → refused with the reason (never a silent hold).
        e.set_tx_enabled(false);
        assert!(e
            .sstv_send(sstv_img_samples(), "PD-120".into())
            .unwrap_err()
            .contains("TX is off"));
        e.set_tx_enabled(true);

        // Tune carrier up → refused.
        e.set_tune(true);
        assert!(e
            .sstv_send(sstv_img_samples(), "PD-120".into())
            .unwrap_err()
            .contains("Tune"));
        e.set_tune(false);

        // Outside privileges (drop to the 20 m data segment) → refused.
        e.set_frequency(14.074, "20m", "USB");
        e.set_tx_enabled(true); // a QSY halts TX (standing invariant) — re-arm
        assert!(!e.tx_allowed());
        assert!(e
            .sstv_send(sstv_img_samples(), "PD-120".into())
            .unwrap_err()
            .contains("license"));

        // Back on a legal phone freq → accepted; a SECOND send is refused (one in flight).
        e.set_frequency(14.290, "20m", "USB");
        e.set_tx_enabled(true);
        assert!(e.tx_allowed());
        e.sstv_send(sstv_img_samples(), "PD-120".into()).unwrap();
        assert!(e
            .sstv_send(sstv_img_samples(), "PD-120".into())
            .unwrap_err()
            .contains("Already"));
    }

    #[test]
    fn sstv_send_refuses_while_another_phone_tx_holds() {
        // SSTV shares the Phone segment with the voice keyer + live mic PTT (unlike RTTY,
        // which is mode-exclusive) — so those must be checked explicitly.
        let mut e = phone_armed_engine();

        // A queued voice-keyer message → refused.
        e.send_voice(vec![0.1; 100]);
        assert!(e
            .sstv_send(sstv_img_samples(), "PD-120".into())
            .unwrap_err()
            .contains("voice"));
        e.stop_voice();

        // Live mic PTT held → refused.
        e.set_ptt(true);
        assert!(e
            .sstv_send(sstv_img_samples(), "PD-120".into())
            .unwrap_err()
            .contains("PTT"));
        e.set_ptt(false);

        // A RTTY over on the air → refused (the loop stamps rtty_sending).
        e.set_rtty_sending(true);
        assert!(e
            .sstv_send(sstv_img_samples(), "PD-120".into())
            .unwrap_err()
            .contains("RTTY"));
        e.set_rtty_sending(false);

        // All clear → accepted.
        e.sstv_send(sstv_img_samples(), "PD-120".into()).unwrap();
    }

    #[test]
    fn sstv_send_enforces_the_duration_budget_and_hard_cap() {
        let mut e = phone_armed_engine();

        // Budget: a 200 s image can't finish before a 3-min (180 s) watchdog trips → refused
        // UP FRONT (not keyed then guillotined mid-image).
        e.settings.tx_watchdog_min = 3;
        let two_hundred_s = vec![0.0f32; 200 * 12_000];
        assert!(e
            .sstv_send(two_hundred_s, "PD-240".into())
            .unwrap_err()
            .contains("watchdog"));

        // Hard cap: past 330 s, refused regardless of a huge watchdog.
        e.settings.tx_watchdog_min = 30;
        let over_cap = vec![0.0f32; 340 * 12_000];
        assert!(e
            .sstv_send(over_cap, "PD-290".into())
            .unwrap_err()
            .contains("capped"));

        // A PD290-sized image (~290 s) fits under a generous watchdog → accepted.
        let pd290 = vec![0.0f32; 290 * 12_000];
        e.sstv_send(pd290, "PD-290".into()).unwrap();
    }

    #[test]
    fn sstv_send_restarts_the_watchdog_clock() {
        let mut e = phone_armed_engine();
        // Pretend a prior over started the watchdog clock a while ago.
        e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(30));
        e.sstv_send(sstv_img_samples(), "PD-120".into()).unwrap();
        assert!(
            e.tx_watchdog_start.is_none(),
            "an explicit send restarts the TX-watchdog clock"
        );
    }

    #[test]
    fn sstv_kill_switches_abort_and_unkey() {
        // Stop, halt_tx, and TX-disarm all drop the job + raise the one-shot abort the
        // radio loop turns into flush-output + unkey.
        let mut e = phone_armed_engine();

        // Stop button.
        e.sstv_send(sstv_img_samples(), "PD-120".into()).unwrap();
        assert!(e.sstv_sending(), "a queued image counts as sending");
        e.sstv_stop();
        assert!(e.take_sstv_abort(), "stop raises the abort");
        assert!(!e.sstv_sending(), "job dropped");
        assert!(!e.take_sstv_abort(), "abort is one-shot");

        // halt_tx (global Stop TX).
        e.set_operating_mode("phone", false); // re-arm
        e.sstv_send(sstv_img_samples(), "PD-120".into()).unwrap();
        e.halt_tx();
        assert!(e.take_sstv_abort(), "halt_tx aborts the image in flight");
        assert!(!e.sstv_sending());
        assert!(!e.tx_enabled(), "halt_tx also disarms TX");

        // TX disarm (Monitor off).
        e.set_operating_mode("phone", false); // re-arm
        e.sstv_send(sstv_img_samples(), "PD-120".into()).unwrap();
        e.set_tx_enabled(false);
        assert!(e.take_sstv_abort(), "disarm aborts the image in flight");
        assert!(!e.sstv_sending());
    }

    #[test]
    fn sstv_launch_safety_nothing_keys_on_arm_or_mode_select() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // Fresh engine: nothing queued.
        assert!(e.poll_sstv_tx().is_none());
        assert!(!e.sstv_sending());
        // Arming RX queues no TX.
        e.set_sstv_armed(true);
        assert!(
            e.poll_sstv_tx().is_none(),
            "arming RX never queues a TX image"
        );
        // Selecting Phone (which arms TX) still keys nothing — no image is queued.
        e.set_license_class("extra");
        e.set_frequency(14.290, "20m", "USB");
        e.set_operating_mode("phone", false);
        assert!(e.tx_enabled());
        assert!(
            e.poll_sstv_tx().is_none(),
            "arming a mode never queues an image — only an explicit sstv_send does"
        );
        assert!(!e.sstv_sending());
    }

    #[test]
    fn poll_sstv_tx_holds_the_job_while_a_gate_is_down() {
        // A queued image is HELD (not dropped) when a gate rung drops, so it can't
        // silently vanish — and can't key while the gate is down either.
        let mut e = phone_armed_engine();
        e.sstv_send(sstv_img_samples(), "PD-120".into()).unwrap();
        e.set_tune(true); // tune carrier up → poll holds
        assert!(e.poll_sstv_tx().is_none(), "held while tuning");
        assert!(e.sstv_sending(), "the job is still queued, not dropped");
        e.set_tune(false);
        assert!(e.poll_sstv_tx().is_some(), "released once the gate clears");
    }

    #[test]
    fn observe_rig_freq_mirrors_the_knob_into_the_snapshot() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // Operator turns the physical VFO to 14.213 MHz on the rig → the app adopts it.
        e.observe_rig_freq(14_213_000);
        let s = e.snapshot();
        assert!(
            (s.radio.dial_mhz - 14.213).abs() < 1e-6,
            "dial mirrors the knob"
        );
        assert_eq!(s.radio.band, "20m", "band derived from the observed freq");
        // The Hz→MHz→Hz round-trip is exact, so the retune block won't fight the knob.
        assert_eq!(
            e.settings().dial_hz(),
            14_213_000,
            "dial_hz round-trips exactly"
        );
    }

    #[test]
    fn a_qsy_clears_the_cw_decode_transcript() {
        // Operator report: the CW decode area lingered after a QSY. Changing bands
        // or working a needed spot moves onto a different signal, so the old CW
        // copy is stale and must clear.
        let mut e = Engine::new("W9XYZ", "EN61", 0); // default dial is 20 m
        e.ai_cw_text = "CQ CQ DE K1ABC K1ABC".into();
        e.ai_cw_status = "copying".into();
        // Band change (band picker, or a cross-band needed-click) clears the copy.
        e.set_frequency(7.030, "40m", "USB");
        assert!(
            e.ai_cw_text.is_empty(),
            "band-change QSY clears the CW decode"
        );
        assert!(e.ai_cw_status.is_empty());
        // Same-band QSY by working a needed spot must ALSO clear (a new signal on
        // the same band still means new copy).
        e.ai_cw_text = "W1AW 599 599".into();
        e.work_spot("CW", 7.025, "40m");
        assert!(
            e.ai_cw_text.is_empty(),
            "working a spot (same band, new freq) clears the CW decode"
        );
    }

    #[test]
    fn tx_lockout_blocks_all_paths_outside_license_privileges() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        e.set_beacon(true);
        // Default Open → no lockout, even on an Extra-only bottom + in any mode.
        e.set_frequency(14.005, "20m", "USB");
        assert!(e.tx_allowed());
        assert!(!e.poll_tx(0).is_empty(), "Open: beacon transmits");

        // Technician on 20 m → not authorized → blocked across every TX path.
        e.set_license_class("technician");
        assert!(!e.tx_allowed(), "Technician has no 20 m");
        assert!(e.poll_tx(0).is_empty(), "slot TX blocked");
        e.set_operating_mode("cw", false);
        e.send_cw("TEST");
        assert_eq!(e.poll_cw_one(), None, "CW blocked outside privileges");
        e.set_operating_mode("phone", false);
        e.set_ptt(true);
        assert!(!e.manual_ptt(), "manual PTT blocked outside privileges");
        e.set_tune(true);
        assert!(!e.tuning(), "tune refused outside privileges");

        // Move to a Technician-legal CW freq (40 m 7.030) → allowed again.
        e.set_operating_mode("cw", false);
        e.set_frequency(7.030, "40m", "USB");
        assert!(e.tx_allowed(), "Technician CW on 40 m is allowed");
    }

    #[test]
    fn tx_allowed_judges_emitted_rf_for_phone_passband_and_digital_sideband() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_license_class("general");

        // Phone (USB) upper-edge bleed: a 20 m dial within ~2.8 kHz of the 14.350 band top
        // emits ABOVE it → blocked; mid-segment is fine.
        e.set_operating_mode("phone", false);
        e.set_frequency(14.349, "20m", "USB");
        assert!(
            !e.tx_allowed(),
            "USB phone bleeding over the band top is blocked"
        );
        e.set_frequency(14.250, "20m", "USB");
        assert!(e.tx_allowed(), "mid-segment phone is fine");

        // Digital offset is sideband-signed: at the same dial just past the 80 m data ceiling
        // (3.600), USB emits ABOVE (blocked) but LSB emits BELOW, back inside the segment.
        e.set_operating_mode("digital", false);
        e.set_frequency(3.601, "80m", "USB");
        assert!(
            !e.tx_allowed(),
            "USB digital above the data ceiling is blocked"
        );
        e.set_frequency(3.601, "80m", "LSB");
        assert!(
            e.tx_allowed(),
            "LSB digital emits below the dial → back in the data segment"
        );
    }

    #[test]
    fn slot_tx_is_idle_in_phone_and_cw_modes() {
        // The FT8/FT1 slot sequencer must not key the rig on a phone/CW over, even with
        // a digital TX source armed — otherwise digital tones ride the voice/CW over.
        let mut e = Engine::new("W9XYZ", "EN61", 0); // tx_parity 0 → slot 0 is a TX slot
        e.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        e.set_beacon(true); // arm a digital TX source
                            // Digital: the slot path transmits as usual on a TX-parity slot.
        e.set_operating_mode("digital", false);
        assert!(
            !e.poll_tx(0).is_empty(),
            "digital slot TX still works with a beacon armed",
        );
        // Phone / CW: the slot path is silent (the keyer / PTT own the rig).
        e.set_operating_mode("phone", false);
        assert!(e.poll_tx(0).is_empty(), "no slot TX in Phone");
        e.set_operating_mode("cw", false);
        assert!(e.poll_tx(0).is_empty(), "no slot TX in CW");
    }

    #[test]
    fn section_tab_follow_freq_qsys_to_each_modes_home_and_arms_a_retune() {
        // The explicit operating-section tabs (follow_freq=true): "go to Phone" lands in the
        // phone segment, CW drops to the CW segment, Digital snaps to the FT8 watering hole.
        use crate::settings::OperatingMode;
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_license_class("extra");
        // Start on the 20 m FT8 watering hole (a digital freq); drain the QSY flag.
        e.set_frequency(14.074, "20m", "USB");
        assert!(e.take_immediate_retune(), "a QSY arms an immediate retune");

        // Phone tab → drop to the 20 m phone-segment start (Extra 14.150), arm the loop NOW.
        e.set_operating_mode("phone", true);
        assert_eq!(e.settings().operating_mode, OperatingMode::Phone);
        assert!(
            (e.settings().dial_mhz - 14.150).abs() < 1e-9,
            "Phone dropped to the phone segment, got {}",
            e.settings().dial_mhz
        );
        assert!(
            e.take_immediate_retune(),
            "a section switch arms an immediate retune"
        );

        // Digital tab → snap to the FT8 watering hole (default tier).
        e.set_operating_mode("digital", true);
        assert_eq!(e.settings().operating_mode, OperatingMode::Digital);
        assert!(
            (e.settings().dial_mhz - 14.074).abs() < 1e-9,
            "Digital snapped to the FT8 watering hole, got {}",
            e.settings().dial_mhz
        );

        // CW tab → the 20 m CW ACTIVITY freq (14.030), not the dead band edge (14.000).
        let _ = e.take_immediate_retune();
        e.set_operating_mode("cw", true);
        assert_eq!(e.settings().operating_mode, OperatingMode::Cw);
        assert!(
            (e.settings().dial_mhz - 14.030).abs() < 1e-9,
            "CW landed on the 20 m activity freq, got {}",
            e.settings().dial_mhz
        );
        assert!(
            e.take_immediate_retune(),
            "CW pick arms a retune to command CW"
        );
    }

    #[test]
    fn set_operating_mode_without_follow_freq_preserves_the_dial() {
        // The Needed-click + incidental-nav invariant: a mode write with follow_freq=false
        // sets ONLY the policy — it must never move the VFO (so a Needed spot keeps its exact
        // frequency, and glancing at the map doesn't yank you off-frequency). It still arms a
        // retune so the loop applies the new mode immediately.
        use crate::settings::OperatingMode;
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_license_class("extra");
        // A 20 m phone spot at 14.250 → set Phone (no follow) → stays exactly on 14.250.
        e.set_frequency(14.250, "20m", "USB");
        let _ = e.take_immediate_retune();
        e.set_operating_mode("phone", false);
        assert_eq!(e.settings().operating_mode, OperatingMode::Phone);
        assert!(
            (e.settings().dial_mhz - 14.250).abs() < 1e-9,
            "the exact spot freq is preserved (no QSY), got {}",
            e.settings().dial_mhz
        );
        assert!(
            e.take_immediate_retune(),
            "a mode change still arms a retune"
        );
    }

    #[test]
    fn phone_lsb_home_clears_the_band_edge_so_tx_is_legal() {
        // On LSB bands the SSB passband sits BELOW the dial; parking at the segment edge would
        // push 2.8 kHz out of band and lock TX out at the very home freq. The Phone home must
        // lift the dial so the whole passband stays in-segment.
        use crate::settings::OperatingMode;
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_license_class("general");
        e.set_frequency(7.074, "40m", "USB"); // start on the 40 m FT8 watering hole
        e.set_operating_mode("phone", true);
        assert_eq!(e.settings().operating_mode, OperatingMode::Phone);
        // General 40 m phone starts at 7.175; the home lifts it by the 2.8 kHz SSB passband.
        assert!(
            (e.settings().dial_mhz - 7.1778).abs() < 1e-9,
            "40 m phone home clears the LSB passband off the edge, got {}",
            e.settings().dial_mhz
        );
        // ...and at that home freq the operator may actually transmit (the whole point).
        assert!(
            e.tx_allowed(),
            "the LSB phone home freq is inside the privileged segment"
        );
    }

    #[test]
    fn digital_home_is_privilege_gated() {
        // A Technician has NO data privilege on 20 m → clicking Digital must NOT yank the dial
        // to 14.074 (where they can't operate); leave it put and let the lockout guard the air.
        use crate::settings::OperatingMode;
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_license_class("technician");
        e.set_frequency(14.205, "20m", "USB");
        let _ = e.take_immediate_retune();
        e.set_operating_mode("digital", true);
        assert_eq!(e.settings().operating_mode, OperatingMode::Digital);
        assert!(
            (e.settings().dial_mhz - 14.205).abs() < 1e-9,
            "Tech digital on 20 m leaves the dial put (no privilege), got {}",
            e.settings().dial_mhz
        );
        // Sanity: on 10 m — the one HF band a Tech may run data — it DOES snap to the hole.
        e.set_frequency(28.400, "10m", "USB");
        e.set_operating_mode("digital", true);
        assert!(
            (e.settings().dial_mhz - 28.074).abs() < 1e-9,
            "Tech digital on 10 m snaps to the FT8 watering hole, got {}",
            e.settings().dial_mhz
        );
    }

    #[test]
    fn split_set_by_pileup_work_and_cleared_by_any_qsy() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_license_class("extra");
        // Work a CW pile-up spot at 14.023 listening UP 2.
        e.work_spot_split("cw", 14.023, "20m", Some(2.0));
        assert_eq!(e.split_tx_mhz(), Some(14.025), "TX dial = spot + 2 kHz");
        assert_eq!(
            e.take_split_request(),
            Some(Some(14.025)),
            "loop gets a one-shot set-split request"
        );
        assert_eq!(e.take_split_request(), None, "one-shot consumed");
        // ANY plain QSY returns to simplex — leftover split must never shift TX
        // silently on the next frequency.
        e.set_frequency(7.074, "40m", "USB");
        assert_eq!(e.split_tx_mhz(), None);
        assert_eq!(
            e.take_split_request(),
            Some(None),
            "loop gets a one-shot split-OFF request"
        );
        // Already simplex → a simplex work issues NO redundant split-off command.
        e.work_spot_split("cw", 7.025, "40m", None);
        assert_eq!(e.split_tx_mhz(), None);
        assert_eq!(e.take_split_request(), None, "no-op when already simplex");
    }

    #[test]
    fn work_spot_sets_mode_and_exact_freq_atomically_without_override() {
        // The Needed click: mode + EXACT spot freq in one shot, no auto-QSY override even on a
        // freq that isn't a "home" channel.
        use crate::settings::OperatingMode;
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_license_class("extra");
        e.set_frequency(14.074, "20m", "USB"); // start on FT8
                                               // Work a 20 m phone spot at an arbitrary in-segment freq (NOT the home 14.150).
        e.work_spot("phone", 14.263, "20m");
        assert_eq!(e.settings().operating_mode, OperatingMode::Phone);
        assert!(
            (e.settings().dial_mhz - 14.263).abs() < 1e-9,
            "work_spot keeps the exact spot freq (no home override), got {}",
            e.settings().dial_mhz
        );
        assert!(
            e.take_immediate_retune(),
            "work_spot arms an immediate retune"
        );
    }

    #[test]
    fn working_a_station_moves_rx_to_its_audio_freq() {
        // Double-click-to-work with the decode's audio freq → RX moves onto it, and TX
        // follows unless Hold Tx Freq is set (WSJT-X behavior).
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.call_station_ctx("W1AW", None, None, None, Some(1200.0));
        assert_eq!(e.rx_offset_hz(), 1200.0, "RX moved to the DX's audio freq");
        assert_eq!(e.tx_offset_hz(), 1200.0, "TX follows when Hold Tx is off");

        // Hold Tx Freq on → only RX moves; TX stays put.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tx_offset(1500.0);
        e.set_hold_tx_freq(true);
        e.call_station_ctx("W1AW", None, None, None, Some(800.0));
        assert_eq!(e.rx_offset_hz(), 800.0, "RX moves");
        assert_eq!(e.tx_offset_hz(), 1500.0, "TX held with Hold Tx on");

        // No freq supplied (e.g. a roster click) → offsets unchanged.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_rx_offset(1000.0);
        e.call_station_ctx("W1AW", None, None, None, None);
        assert_eq!(e.rx_offset_hz(), 1000.0, "no freq → no move");
    }

    #[test]
    // The counter only exists in debug builds; without this gate a
    // `cargo test --release` fails to COMPILE the whole lib-test target.
    #[cfg(debug_assertions)]
    fn snapshot_performs_a_bounded_number_of_logbook_sweeps() {
        // The stranded-fix class, pinned as a SHAPE: worked_call_set() exists so
        // snapshot() sweeps the log once, yet a per-decode-row worked_before()
        // regrew 240 lines below it — O(rows × log) under the engine mutex, the
        // exact sweep whose comment documents a 1–2 s waterfall stall. This
        // bound fails the suite wherever a per-row sweep reappears.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        for i in 0..25 {
            let mut r = qrec(&format!("W{i}ABC"), "20m");
            r.mode = "FT8".into();
            e.log_qso(r);
        }
        // A busy period: 40 decode rows for the snapshot to fold.
        let rows: Vec<modes::Decode> = (0..40)
            .map(|i| dec_snr(&format!("CQ K{}AA FN3{}", i % 10, i % 10), -8))
            .collect();
        e.ingest_decodes_for_test(&rows, 2);

        tempo_core::logbook::LOG_SWEEPS.with(|c| c.set(0));
        let _snap = e.snapshot();
        let sweeps = tempo_core::logbook::LOG_SWEEPS.with(|c| c.get());
        assert!(
            sweeps <= 2,
            "snapshot() swept the logbook {sweeps} times for 40 decode rows — \
             a per-row sweep has regrown (use the prebuilt worked set)"
        );
    }

    #[test]
    fn ft8_tx_requires_callsign_and_grid() {
        // The reported bug: a standard FT8 message must NEVER go on the air without the
        // operator's callsign AND a valid grid (a grid-less call). The QSO sequencer
        // (Call CQ) is the real FT8 keying path.
        // No grid → Call CQ refused at the command boundary + the slot backstop blocks.
        let mut e = Engine::new("W9XYZ", "", 0);
        e.set_tier(Tier::Ft8);
        assert!(
            e.structured_tx_ready(true).is_err(),
            "FT8 not ready without a grid"
        );
        e.set_mode("qso-run").unwrap(); // engine set_mode doesn't gate; arms TX + CallingCq
        assert!(
            e.poll_tx(0).is_empty(),
            "backstop: no FT8 CQ without a grid"
        );

        // No callsign → blocked too.
        let mut e = Engine::new("", "EN52", 0);
        e.set_tier(Tier::Ft8);
        assert!(
            e.structured_tx_ready(true).is_err(),
            "FT8 not ready without a call"
        );
        e.set_mode("qso-run").unwrap();
        assert!(
            e.poll_tx(0).is_empty(),
            "backstop: no FT8 CQ without a call"
        );

        // Valid identity → ready, and the CQ keys.
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        e.set_tier(Tier::Ft8);
        assert!(
            e.structured_tx_ready(true).is_ok(),
            "valid call+grid → ready"
        );
        e.set_mode("qso-run").unwrap();
        assert!(
            !e.poll_tx(0).is_empty(),
            "FT8 CQ keys with a valid call+grid"
        );

        // A lowercase grid is valid (encoder is case-insensitive) — must NOT be blocked.
        let mut e = Engine::new("W9XYZ", "en52", 0);
        e.set_tier(Tier::Ft8);
        assert!(
            e.structured_tx_ready(true).is_ok(),
            "lowercase grid is accepted"
        );

        // Field Day on FT8: the exchange carries NO grid, so a valid call alone is enough
        // — a blank grid must NOT suppress the FD over.
        let mut e = Engine::new("W9XYZ", "", 0);
        e.set_tier(Tier::Ft8);
        // But a BLANK class/section must refuse the mode — the exchange goes on
        // the air, and the old "WI" default sent wrong exchanges outside Wisconsin.
        assert!(
            e.set_mode("fieldday-run").is_err(),
            "blank FD exchange must not start"
        );
        assert!(
            e.structured_tx_ready(false).is_ok(),
            "FD needs only a callsign"
        );
        {
            let mut s = e.settings().clone();
            s.fd_class = "3A".into();
            s.fd_section = "WI".into();
            e.apply_settings(s);
        }
        e.set_tier(Tier::Ft8);
        e.set_mode("fieldday-run").unwrap(); // arms TX
        assert!(
            !e.poll_tx(0).is_empty(),
            "FT8 Field Day keys without a grid"
        );

        // A settings save mid-event must NOT destroy the Field Day contest log
        // (the FD panel saves settings on every bonus-checkbox toggle). A real
        // FD session has the master switch on, so the save preserves FD in place
        // rather than exiting it (master off = not in FD, spec §1.3).
        {
            let mut e = Engine::new("W9XYZ", "EN61", 0);
            {
                let mut s = e.settings().clone();
                s.fd_active = true;
                s.fd_class = "3A".into();
                s.fd_section = "WI".into();
                e.apply_settings(s);
            }
            e.set_mode("fieldday-run").unwrap();
            assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
            assert!(e.fd_log_manual("W1AW", "1D", "CT", "PH").unwrap());
            let before = e.fd_score().expect("in Field Day mode");
            assert!(before.0 > 0, "logged contacts score points");
            // Toggle a bonus — this saves settings, the exact mid-event action.
            let mut s = e.settings().clone();
            s.fd_bonuses = vec!["w1aw".into()];
            e.apply_settings(s);
            let after = e
                .fd_score()
                .expect("still in Field Day after a settings save");
            assert_eq!(
                after.0, before.0,
                "the contest log must survive a settings save"
            );
        }

        // FT1 free-text is exempt from the standard-message grid contract.
        let mut e = Engine::new("W9XYZ", "", 0);
        e.set_tier(Tier::TempoFast);
        assert!(e.structured_tx_ready(true).is_ok(), "FT1 is not grid-bound");
    }

    #[test]
    fn callsign_is_single_source_of_truth_after_apply_settings() {
        // The DISPLAYED call (snapshot.mycall) and the TX call (settings.mycall, used
        // by BOTH the FT8 CQ and the Tempo broadcast wire) must never diverge —
        // changing the callsign in Settings updates both. Guards the user's "Tempo CQ
        // must use my configured callsign (the same one FT8 uses)" invariant.
        let mut e = Engine::new("OLDCALL", "EN52", 0);
        assert_eq!(e.snapshot().mycall, "OLDCALL");
        let mut s = e.settings().clone();
        s.mycall = "NEWCALL".into();
        e.apply_settings(s);
        // Display tracks the configured call…
        assert_eq!(e.snapshot().mycall, "NEWCALL");
        // …and TX reads the same settings field (what broadcast()/FT8 CQ use)…
        assert_eq!(e.settings().mycall, "NEWCALL");
        // …so the on-air broadcast prefix carries the new call.
        let wire = tempo_core::inbox::broadcast_text(&e.settings().mycall, "CQ EN52");
        assert!(wire.starts_with("DE NEWCALL "), "broadcast wire = {wire}");
    }

    #[test]
    fn apply_settings_grid_change_preserves_band_feed() {
        // Regression for "saving Settings blanks the band feed": a callsign/grid
        // change must rebind identity in place (not rebuild AppState), so the `*`
        // band feed (and all conversations) survive.
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.broadcast("CQ FN31");
        assert!(
            e.snapshot().conversations.iter().any(|c| c.peer == "*"),
            "broadcast seeded the band feed"
        );
        let mut s = e.settings().clone();
        s.mygrid = "FN42".into();
        e.apply_settings(s);
        assert!(
            e.snapshot().conversations.iter().any(|c| c.peer == "*"),
            "band feed survives a grid change"
        );
    }

    #[test]
    fn poll_tx_empty_when_tx_disabled() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        e.set_beacon(true); // there would be a beacon to send on slot 0
        assert!(!e.poll_tx(0).is_empty(), "baseline: a beacon is produced");

        e.set_tx_enabled(false);
        assert!(
            e.poll_tx(0).is_empty(),
            "no TX while transmit is disabled (Monitor-off)"
        );
        let snap = e.snapshot();
        assert!(!snap.radio.tx_enabled);
        assert!(!snap.radio.transmitting);
    }

    #[test]
    fn cw_id_after_73_arms_on_the_final_over_and_halt_clears_it() {
        // Mirrors disable_tx_after_73_is_deferred_and_snp_only's drive exactly.
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.settings.cw_id_after_73 = true;
        e.call_station("W9XYZ"); // S&P: we answer them
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        assert!(!e.poll_tx(4).is_empty(), "the closing 73 transmits");
        assert!(e.take_pending_cw_id(), "CW ID armed after the final 73");
        assert!(!e.take_pending_cw_id(), "one-shot");
        // Default-off: without the setting the flag never arms.
        let mut d = Engine::new("K2DEF", "FN31", 0);
        d.call_station("W9XYZ");
        d.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        d.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        let _ = d.poll_tx(4);
        assert!(!d.take_pending_cw_id(), "stock default: no CW ID");
        // Halt = silence: a queued ID dies with it.
        e.settings.cw_id_after_73 = true;
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 5);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 7);
        let _ = e.poll_tx(8);
        e.halt_tx();
        assert!(!e.take_pending_cw_id(), "halt clears a pending CW ID");
    }

    #[test]
    fn broker_ptt_refused_mid_tune_and_mid_over() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.settings.cat_broker_ptt = true;
        e.set_tx_enabled(true);
        assert!(e.broker_ptt(true), "idle → foreign key granted");
        assert!(e.broker_ptt(false));
        e.set_tune(true); // live tune carrier owns the rig
        assert!(!e.broker_ptt(true), "no foreign key mid-tune");
        e.set_tune(false);
        e.app.set_transmitting(true); // FT8 over in flight
        assert!(!e.broker_ptt(true), "no foreign key mid-over");
        e.app.set_transmitting(false);
        assert!(e.broker_ptt(true), "granted again once idle");
    }

    #[test]
    fn band_switch_halts_tx_but_in_band_qsy_does_not() {
        let mut eng = Engine::new("K2DEF", "FN31", 0);
        eng.set_frequency(14.074, "20m", "USB");
        eng.set_tx_enabled(true);
        assert!(eng.tx_enabled, "armed on 20m");
        // In-band dial move (FT8→FT4 dial): the QSO context survives — no halt.
        eng.set_frequency(14.080, "20m", "USB");
        assert!(eng.tx_enabled, "in-band QSY must not halt TX");
        // Band switch (the Needed-click case): the sequencer must STOP — a
        // directed call aimed at the old band's station must not key up here.
        eng.set_frequency(18.100, "17m", "USB");
        assert!(!eng.tx_enabled, "band switch must halt TX");
        // Knob-turned band change reported by the rig: same invariant.
        eng.set_tx_enabled(true);
        eng.observe_rig_freq(21_074_000);
        assert!(!eng.tx_enabled, "rig-knob band switch must halt TX");
    }

    #[test]
    fn halt_tx_stops_and_stays_stopped() {
        // Stop TX must actually stop: an armed QSO sequencer would otherwise
        // re-transmit on the very next slot, making the button look broken.
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.call_station("W9XYZ"); // arms the responder sequencer (running)
        assert!(!e.poll_tx(0).is_empty(), "baseline: the QSO transmits");

        e.halt_tx();
        assert!(!e.tx_enabled(), "halt_tx disables transmit");
        assert!(
            e.poll_tx(0).is_empty() && e.poll_tx(2).is_empty(),
            "stays stopped across slots — the sequencer does NOT re-arm"
        );
        assert!(!e.snapshot().radio.transmitting);
    }

    #[test]
    fn cw_send_re_arms_tx_after_a_halt() {
        // The reported CW-fidelity bug: after Stop TX, an F-key / typed CW send silently queued
        // but never keyed until a band/contact switch re-entered the mode. A deliberate CW send
        // must ALWAYS transmit (privilege permitting) — no ifs, ands, or buts.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_operating_mode("cw", false); // manual mode arms tx on entry
        e.set_frequency(7.03, "40m", "CW"); // a CW freq we have privileges on
        assert!(e.tx_allowed(), "on a CW freq we can key");

        e.halt_tx(); // operator hits Stop TX
        assert!(!e.tx_enabled(), "halt disarms");
        assert!(e.poll_cw_one().is_none(), "nothing keys while halted");

        e.send_cw("TEST"); // operator hits an F-key / types
        assert!(e.tx_enabled(), "a deliberate CW send re-arms TX");
        assert_eq!(
            e.poll_cw_one().as_deref(),
            Some("TEST"),
            "the queued word now keys — the F-key took effect"
        );
    }

    #[test]
    fn call_cq_emits_one_structured_cq_and_arms_tx() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::TempoFast);
        assert!(!e.tx_enabled(), "TX disarmed at start (launch safety)");

        e.call_cq(None).expect("call_cq with a valid grid");

        // ONE clean structured CQ — NOT a chunked "DE … A12…" free-text broadcast.
        assert_eq!(e.broadcast_queue.len(), 1, "exactly one frame");
        assert_eq!(e.broadcast_queue[0], "CQ KD9TAW EN52");
        // Auto-armed: keys without a separate Enable-Tx click.
        assert!(e.tx_enabled(), "call_cq arms TX");
        assert!(
            !e.poll_tx(0).is_empty(),
            "CQ keys on the next TX-parity slot"
        );
        // Echoed into the band feed as outbound.
        let feed = e.app.conversation("*").expect("band feed");
        assert!(feed
            .messages
            .iter()
            .any(|m| m.outbound && m.text == "CQ KD9TAW EN52"));
    }

    #[test]
    fn call_cq_compound_call_uses_grid_less_form() {
        // A compound call packs only as the grid-less i3=4 CQ — keeping a grid or a
        // directed token would force TRUNCATED free text (the bug). It must also be
        // allowed to call CQ WITHOUT a grid.
        let mut e = Engine::new("W9XYZ/P", "", 0);
        e.set_tier(Tier::TempoFast);
        e.call_cq(Some("DX"))
            .expect("compound CQ succeeds without a grid");
        assert_eq!(e.broadcast_queue.len(), 1);
        assert_eq!(
            e.broadcast_queue[0], "CQ W9XYZ/P",
            "compound packs grid-less + drops the dir token"
        );
    }

    #[test]
    fn call_cq_requires_a_grid_and_does_not_arm_on_error() {
        let mut e = Engine::new("KD9TAW", "", 0); // no grid
        assert!(e.call_cq(None).is_err(), "CQ needs a 4-char grid");
        assert!(!e.tx_enabled(), "a failed CQ must not arm TX");
        assert!(e.broadcast_queue.is_empty(), "nothing queued");
    }

    #[test]
    fn broadcast_arms_tx() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::TempoFast);
        assert!(!e.tx_enabled());
        e.broadcast("QRZ?");
        assert!(e.tx_enabled(), "an explicit broadcast arms TX too");
    }

    #[test]
    fn directed_send_arms_tx() {
        // A directed reply is an explicit "send this" action just like broadcast — it must
        // arm TX, or the queued message sits in "waiting" forever with no Enable-Tx click
        // (half of the "reply won't send" bug). It stays store-and-forward gated: arming does
        // not put it on the air until the peer is present.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::TempoFast);
        assert!(!e.tx_enabled());
        e.send_message("W9XYZ", "MEET AT NOON");
        assert!(e.tx_enabled(), "sending a directed reply arms TX");
    }

    #[test]
    fn band_activity_shows_logical_message_not_raw_chunks() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::TempoFast);
        e.broadcast("testing 123"); // -> "DE KD9TAW testing 123", chunked into wire frames
        for slot in [0u64, 2, 4, 6] {
            let _ = e.poll_tx(slot); // transmit the chunk frames over several TX slots
        }
        let mine: Vec<String> = e
            .snapshot()
            .recent_decodes
            .iter()
            .filter(|d| d.mine)
            .map(|d| d.message.clone())
            .collect();
        assert!(
            mine.iter().any(|m| m.contains("testing 123")),
            "band activity shows the clean message, got {mine:?}"
        );
        assert!(
            !mine
                .iter()
                .any(|m| tempo_core::text::parse_chunk(m).is_some()),
            "no raw 'A13…' chunk frames in band activity, got {mine:?}"
        );
    }

    #[test]
    fn short_broadcast_uses_one_bare_frame() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::TempoFast);
        e.broadcast("73"); // "DE KD9TAW 73" = 12 chars ≤ 13 → ONE bare frame (no header)
        assert_eq!(e.broadcast_queue.len(), 1, "single frame, not chunked");
        assert!(
            tempo_core::text::parse_chunk(&e.broadcast_queue[0]).is_none(),
            "fast-path frame carries no chunk header: {:?}",
            e.broadcast_queue[0]
        );
        for slot in [0u64, 2] {
            let _ = e.poll_tx(slot);
        }
        let mine: Vec<String> = e
            .snapshot()
            .recent_decodes
            .iter()
            .filter(|d| d.mine)
            .map(|d| d.message.clone())
            .collect();
        assert_eq!(
            mine.iter().filter(|m| m.contains("73")).count(),
            1,
            "exactly one clean own-TX row (the bare frame isn't double-recorded): {mine:?}"
        );
    }

    #[test]
    fn switching_tier_re_qsys_to_the_new_mode_dial() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB"); // 20m FT8 watering hole
                                               // FT8→FT4 must move the rig to the FT4 20m dial (14.080), like WSJT-X — not stay.
        e.set_tier(Tier::Ft4);
        assert!(
            (e.settings().dial_mhz - 14.080).abs() < 0.0005,
            "FT4 moved the dial to 14.080, got {}",
            e.settings().dial_mhz
        );
        // …and back.
        e.set_tier(Tier::Ft8);
        assert!(
            (e.settings().dial_mhz - 14.074).abs() < 0.0005,
            "FT8 moved back to 14.074, got {}",
            e.settings().dial_mhz
        );
    }

    fn cq_decode_from(call: &str) -> modes::Decode {
        modes::Decode {
            message: format!("CQ {call} EN37"),
            sync: 1.0,
            snr: -8,
            dt: 0.1,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode: None,
        }
    }

    #[test]
    fn chat_reply_auto_picks_the_opposite_cycle() {
        let mut e = Engine::new("K2DEF", "FN31", 0); // starts Tx 1st (even)
        e.set_tier(Tier::TempoFast);
        assert!(e.tx_even() && e.tx_cycle_auto(), "starts even + auto");
        // W9XYZ decoded at slot 7 → answer on tx_parity 7%2=1 (odd) = the OPPOSITE of
        // their period, so we key while they listen.
        e.decode_history.push_back((7, cq_decode_from("W9XYZ")));
        e.send_message("W9XYZ", "HI");
        assert!(!e.tx_even(), "auto-cycle flipped to the opposite cycle");
        assert!(
            e.tx_cycle_auto(),
            "a reply is not a manual pick — auto stays on"
        );
    }

    #[test]
    fn manual_cycle_pick_disables_auto_and_holds() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        assert!(e.tx_cycle_auto());
        e.set_tx_even(false); // operator picks Tx 2nd
        assert!(!e.tx_cycle_auto(), "a manual pick disables auto-cycle");
        assert!(!e.tx_even());
        // A later reply (whose slot would auto-pick EVEN) must NOT override the manual cycle.
        e.set_tier(Tier::TempoFast);
        e.decode_history.push_back((8, cq_decode_from("W9XYZ")));
        e.send_message("W9XYZ", "HI");
        assert!(!e.tx_even(), "manual cycle held — auto did not override it");
    }

    #[test]
    fn harq_enabled_by_default_and_toggles() {
        // IR-HARQ is ON by default (unlike beacon, it is NOT force-disabled at
        // boot), persists from settings, and the accessor toggles it.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        assert!(e.harq_enabled(), "IR-HARQ defaults on");
        e.set_harq_enabled(false);
        assert!(!e.harq_enabled());
        let off = Engine::with_settings(Settings {
            harq_enabled: false,
            ..Settings::default()
        });
        assert!(
            !off.harq_enabled(),
            "persisted harq_enabled=false is respected"
        );
    }

    #[test]
    fn decode_feed_labels_each_decode_by_its_own_mode() {
        // The active tier is FT1, but the feed must label each decode by the mode
        // that actually produced it (a companion WSJT-X stream, or post-switch
        // native decodes carry their own mode); an untagged decode (DX1's robust
        // path / unknown companion mode) falls back to the selected tier.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::TempoFast);

        let mut d_ft8 = dec_snr("CQ F5RXL IN94", -3);
        d_ft8.mode = Some(modes::ModeKind::Ft8);
        let mut d_ft4 = dec_snr("CQ RU N9OY EN43", -5);
        d_ft4.mode = Some(modes::ModeKind::Ft4);
        let d_unknown = dec_snr("CQ W6PQR CM87", -7); // mode: None

        e.ingest_decodes_for_test(&[d_ft8, d_ft4, d_unknown], 1);
        let rows = e.snapshot().recent_decodes;
        let tier_of = |m: &str| rows.iter().find(|r| r.message == m).unwrap().tier;

        assert_eq!(
            tier_of("CQ F5RXL IN94"),
            Tier::Ft8,
            "FT8-tagged decode labeled FT8"
        );
        assert_eq!(
            tier_of("CQ RU N9OY EN43"),
            Tier::Ft4,
            "FT4-tagged decode labeled FT4"
        );
        assert_eq!(
            tier_of("CQ W6PQR CM87"),
            Tier::TempoFast,
            "untagged decode falls back to the selected tier"
        );
    }

    #[test]
    fn source_toggle_native_companion() {
        // Companion binds an ephemeral loopback port so the test never contends
        // on the real :2237.
        let mut e = Engine::with_settings(Settings {
            companion_addr: "127.0.0.1:0".to_string(),
            ..Settings::default()
        });
        assert_eq!(e.source_kind(), SourceKind::Native);

        e.set_source(SourceKind::Companion)
            .expect("bind ephemeral companion socket");
        assert_eq!(e.source_kind(), SourceKind::Companion);
        assert_eq!(e.snapshot().radio.source, SourceKind::Companion);
        assert_eq!(e.snapshot().radio.source_label, "WSJT-X UDP");

        // Switching tier must NOT clobber a live companion source.
        e.set_tier(Tier::Ft8);
        assert_eq!(
            e.source_kind(),
            SourceKind::Companion,
            "a tier change keeps the live companion source"
        );
        assert_eq!(e.snapshot().radio.source_label, "WSJT-X UDP");

        // Back to native: the source follows the selected tier again.
        e.set_source(SourceKind::Native)
            .expect("native never fails");
        assert_eq!(e.source_kind(), SourceKind::Native);
        e.set_tier(Tier::Ft4);
        assert_eq!(e.snapshot().radio.source_label, "Native (FT4)");
    }

    #[test]
    fn source_choice_is_recorded_in_settings_and_survives_a_form_save() {
        let mut e = Engine::with_settings(Settings {
            companion_addr: "127.0.0.1:0".to_string(),
            ..Settings::default()
        });
        assert_eq!(e.settings().source, SourceKind::Native);

        e.set_source(SourceKind::Companion)
            .expect("bind ephemeral companion socket");
        // Recorded into settings so the shell can persist it across restart.
        assert_eq!(e.settings().source, SourceKind::Companion);

        // A settings-form save (whose payload carries the default Native source)
        // must NOT silently flip the live source — only set_source owns it.
        e.apply_settings(Settings {
            mycall: "K9ABC".to_string(),
            ..Settings::default()
        });
        assert_eq!(
            e.source_kind(),
            SourceKind::Companion,
            "form save preserves the live signal source"
        );
        assert_eq!(e.settings().source, SourceKind::Companion);
        assert_eq!(
            e.settings().mycall,
            "K9ABC",
            "other form fields still applied"
        );
    }

    #[test]
    fn poll_tx_empty_while_tuning() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_beacon(true);
        e.set_tune(true);
        assert!(
            e.poll_tx(0).is_empty(),
            "normal slot TX is suppressed while holding a tune carrier"
        );
        assert!(e.snapshot().radio.tuning);
    }

    #[test]
    fn chat_cq_run_repeats_pauses_on_send_and_resumes_when_idle() {
        let s = Settings {
            mycall: "KD9TAW".into(),
            mygrid: "EN52".into(),
            ..Settings::default()
        };
        let mut e = Engine::with_settings(s);
        e.set_tx_enabled(true);
        assert_eq!(e.chat_cq_state(), "off");
        e.set_chat_cq(true).unwrap();
        assert_eq!(e.chat_cq_state(), "calling");
        // Immediate CQ on start, then the run re-sends every idle own-parity slot —
        // the loop the one-shot Call CQ button couldn't do.
        assert!(!e.poll_tx(0).is_empty(), "immediate CQ on start");
        assert!(!e.poll_tx(2).is_empty(), "run re-sends on the next TX slot");
        assert!(!e.poll_tx(4).is_empty(), "and keeps calling");
        // Answering someone pauses the run (sequential policy: no CQ mid-QSO).
        e.send_message("W9XYZ", "hello there");
        assert_eq!(e.chat_cq_state(), "paused");
        assert!(
            e.poll_tx(6).is_empty(),
            "paused: no CQ while the conversation is fresh (peer absent → held)"
        );
        // Directed traffic idle for CHAT_CQ_RESUME_SLOTS → auto-resume.
        assert!(!e.poll_tx(8).is_empty(), "idle → the run resumes calling");
        assert_eq!(e.chat_cq_state(), "calling");
        // Stop ends it cleanly.
        e.set_chat_cq(false).unwrap();
        assert_eq!(e.chat_cq_state(), "off");
        assert!(e.poll_tx(10).is_empty(), "stopped: silent again");
    }

    #[test]
    fn launch_seed_does_not_halt_tx_or_clear_decode_context() {
        // Read-only launch (LAUNCH-SAFETY family): seed_rig_dial is the boot seed from
        // the rig's own frequency. It must be provably NOT observe_rig_freq — a
        // cross-band seed must not wipe the roster, halt TX, or touch split state,
        // even though those are coincidental no-ops on a fresh engine today.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tier(Tier::Ft8);
        // Populate state a knob-QSY would wipe: a heard station + armed TX.
        e.ingest_decodes_for_test(&[dec_snr("CQ K2DEF FN31", -7)], 1);
        e.set_tx_enabled(true);
        let stations_before = e.snapshot().stations.len();
        assert!(stations_before > 0, "precondition: roster populated");

        e.seed_rig_dial(7_074_000); // cross-band: 20 m default → 40 m
        let s = e.snapshot();
        assert_eq!(
            s.stations.len(),
            stations_before,
            "seed must not clear the roster"
        );
        assert!(s.radio.tx_enabled, "seed must not halt TX");
        assert_eq!(s.radio.band, "40m", "the dial/band did seed");
        assert!(
            (s.radio.dial_mhz - 7.074).abs() < 1e-9,
            "dial seeded exactly"
        );
    }

    #[test]
    fn launches_passive_even_if_beacon_persisted_on() {
        // A saved settings file with beacon=true must NOT auto-call CQ at launch:
        // the app boots listen-only until the operator arms the beacon this session.
        // Valid identity so the "fires once armed" leg isn't blocked by the standard-
        // message identity backstop (this test is about passivity, not identity).
        let s = Settings {
            beacon: true,
            mycall: "W9XYZ".into(),
            mygrid: "EN37".into(),
            ..Settings::default()
        };
        let mut e = Engine::with_settings(s);
        assert!(
            !e.beacon_enabled(),
            "beacon must be disarmed at launch even if persisted on"
        );
        assert!(
            e.poll_tx(0).is_empty(),
            "no auto-CQ on a TX beacon slot at launch"
        );
        // The operator arms it this session → now it beacons. TX is ALSO disarmed at
        // launch (WSJT-X Enable-Tx), so arming the beacon alone isn't enough — enabling
        // transmit is the second, deliberate gate.
        e.set_beacon(true);
        assert!(
            e.poll_tx(0).is_empty(),
            "beacon armed but TX still disarmed → still silent"
        );
        e.set_tx_enabled(true);
        assert!(
            !e.poll_tx(0).is_empty(),
            "beacon fires once the operator enables it AND arms transmit"
        );
    }

    #[test]
    fn tx_even_gates_opposite_slots() {
        // Tx-1st (even parity): beacons on an even TX slot, silent on odd slots.
        let mut a = Engine::new("W9XYZ", "EN37", 0);
        a.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        a.set_beacon(true);
        assert!(a.tx_even());
        assert!(!a.poll_tx(0).is_empty(), "Tx-1st transmits on even slot 0");
        assert!(a.poll_tx(1).is_empty(), "Tx-1st is silent on odd slot 1");
        assert!(a.snapshot().radio.tx_even);

        // Flip to Tx-2nd (odd parity) live: now transmits on odd slots only.
        a.set_tx_even(false);
        assert!(!a.tx_even());
        assert!(!a.poll_tx(1).is_empty(), "Tx-2nd transmits on odd slot 1");
        assert!(a.poll_tx(0).is_empty(), "Tx-2nd is silent on even slot 0");
        assert!(!a.snapshot().radio.tx_even);
    }

    #[test]
    fn set_offset_clamps_and_follows_hold() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tx_offset(1800.0);
        assert_eq!(e.tx_offset_hz(), 1800.0);
        // Hold off: setting RX drags TX along (the common case).
        e.set_rx_offset(1200.0);
        assert_eq!(e.rx_offset_hz(), 1200.0);
        assert_eq!(
            e.tx_offset_hz(),
            1200.0,
            "TX follows RX when Hold Tx is off"
        );
        // Hold on: RX no longer moves TX.
        e.set_hold_tx_freq(true);
        e.set_rx_offset(900.0);
        assert_eq!(e.rx_offset_hz(), 900.0);
        assert_eq!(e.tx_offset_hz(), 1200.0, "TX held when Hold Tx is on");
        // Clamp to the usable passband + surface in the snapshot.
        e.set_rx_offset(50.0);
        assert_eq!(e.rx_offset_hz(), 200.0, "clamped to the low edge");
        e.set_tx_offset(5000.0);
        assert_eq!(e.tx_offset_hz(), 4000.0, "clamped to the high edge");
        let snap = e.snapshot();
        assert_eq!(snap.radio.rx_offset_hz, 200.0);
        assert_eq!(snap.radio.tx_offset_hz, 4000.0);
        assert!(snap.radio.hold_tx_freq);
    }

    #[test]
    fn watchdog_trips_after_the_limit_then_clears() {
        // WALL-CLOCK watchdog: trips after `tx_watchdog_min` minutes of real elapsed
        // time since the first over (no real time passes in a test, so we backdate the
        // timer to simulate the limit being exceeded).
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        e.settings.tx_watchdog_min = 1; // 60 s limit
        for _ in 0..2 {
            e.broadcast("THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG MANY TIMES TODAY");
        }
        // First over starts the watchdog clock but doesn't trip it.
        assert!(!e.poll_tx(0).is_empty(), "an over is produced");
        assert!(!e.tx_watchdog, "a fresh timer doesn't trip immediately");
        assert!(
            e.tx_watchdog_start.is_some(),
            "the timer started on the first over"
        );

        // Backdate the start past the 60 s limit → the next over trips the watchdog.
        e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(61));
        e.poll_tx(2);
        assert!(e.tx_watchdog, "watchdog trips after the wall-clock limit");
        assert!(!e.tx_enabled, "watchdog auto-halts transmit");
        assert!(e.snapshot().radio.tx_watchdog);

        // Re-enabling TX is an operator action: clears the watchdog + restarts the timer.
        e.set_tx_enabled(true);
        assert!(e.tx_enabled);
        assert!(!e.tx_watchdog, "re-enabling TX clears the watchdog");
        assert!(
            e.tx_watchdog_start.is_none(),
            "the timer restarts on the next over"
        );
        assert!(!e.snapshot().radio.tx_watchdog);

        // An operator action also restarts the timer mid-stream.
        e.poll_tx(4); // re-arm the timer
        assert!(e.tx_watchdog_start.is_some());
        e.reset_tx_watchdog();
        assert!(
            e.tx_watchdog_start.is_none(),
            "operator action restarts the timer"
        );
    }

    #[test]
    fn time_sync_ok_flips_false_on_large_dt() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        // No decodes yet → assumed OK.
        assert!(e.snapshot().radio.time_sync_ok);

        // Small DT decodes keep us in sync.
        for s in 0..4u64 {
            e.ingest_decodes_for_test(&[dec_dt("CQ W9XYZ EN37", 0.1)], s);
        }
        assert!(
            e.snapshot().radio.time_sync_ok,
            "small DT keeps time-sync OK"
        );

        // A run of large-DT decodes pushes the median past the threshold.
        for s in 4..40u64 {
            e.ingest_decodes_for_test(&[dec_dt("CQ W9XYZ EN37", 1.2)], s);
        }
        assert!(
            !e.snapshot().radio.time_sync_ok,
            "large median DT flips time-sync to not-OK"
        );
    }

    /// Full engine loopback on the DX1 robust tier: station A's beacon is
    /// modulated as DX1 (non-coherent 8-FSK), placed in a 15 s capture window,
    /// and station B (also DX1) decodes it via the DX1 path — proving the
    /// engine's tier-aware TX (`poll_tx`) and RX (`ingest`) swap waveforms
    /// correctly and recover the message end-to-end.
    #[test]
    fn engine_dx1_tier_beacon_roundtrip() {
        let mut a = Engine::new("W9XYZ", "EN37", 0);
        a.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        a.set_tier(Tier::TempoDeep);
        a.set_beacon(true); // beacon is off by default; this test exercises it

        // Slot 0 is a TX slot (parity 0) and a beacon slot → "CQ W9XYZ EN37".
        let waves = a.poll_tx(0);
        assert!(!waves.is_empty(), "DX1 beacon produced a waveform");
        assert_eq!(
            waves[0].len(),
            tempo_fast::deep::frame_len(),
            "TX wave is one DX1 frame"
        );

        // Embed the frame in a full DX1 capture window at a non-zero offset so
        // the chirp sync must find it.
        let cap = tempo_fast::deep::capture_len();
        let mut window = vec![0f32; cap];
        let off = 6_000;
        window[off..off + waves[0].len()].copy_from_slice(&waves[0]);

        let mut b = Engine::new("K2DEF", "FN31", 1);
        b.set_tier(Tier::TempoDeep);
        let n = b.ingest(&window, 0);
        assert_eq!(n, 1, "DX1 station decoded the beacon");

        let snap = b.snapshot();
        assert!(
            snap.stations.iter().any(|s| s.call == "W9XYZ"),
            "roster learned the DX1 beacon's sender"
        );
    }

    /// DX1 full-passband acquisition (WS-B): three beacons at different carriers
    /// AND arrival times in one capture window are ALL decoded by a single
    /// `ingest` — the receiver is no longer limited to the one signal under the
    /// green RX marker. Proves `engine.ingest` routes DX1 through the full-band
    /// scan and the multi-decode feed carries each signal's own carrier.
    #[test]
    fn engine_dx1_band_multi_signal() {
        let beacons = [
            ("CQ W9XYZ EN37", 800.0f32, 3_000usize),
            ("CQ K2DEF FN20", 1500.0, 6_000),
            ("CQ AA1BB FN42", 2300.0, 9_000),
        ];
        let cap = tempo_fast::deep::capture_len();
        let mut window = vec![0f32; cap];
        for (msg, f0, off) in beacons {
            let w = tempo_fast::deep::encode_wave(msg, f0, tempo_fast::SAMPLE_RATE);
            for (i, &s) in w.iter().enumerate() {
                if off + i < cap {
                    window[off + i] += s;
                }
            }
        }

        let mut rx = Engine::new("N0CALL", "DM79", 7);
        rx.set_tier(Tier::TempoDeep);
        let n = rx.ingest(&window, 0);
        assert_eq!(n, 3, "all three band signals decoded in one slot");

        let snap = rx.snapshot();
        for call in ["W9XYZ", "K2DEF", "AA1BB"] {
            assert!(
                snap.stations.iter().any(|s| s.call == call),
                "roster learned DX1 band signal {call}"
            );
        }
        // Each decode carries its OWN carrier (the feed is offset-agnostic; not
        // pinned to rx_offset_hz anymore).
        let freqs: Vec<f32> = snap.recent_decodes.iter().map(|d| d.freq_hz).collect();
        assert!(
            freqs.iter().any(|&f| (f - 800.0).abs() <= 6.25)
                && freqs.iter().any(|&f| (f - 2300.0).abs() <= 6.25),
            "decodes carry their band carriers, got {freqs:?}"
        );
    }

    /// Switching tier changes nothing about the messaging layer: an FT1 beacon
    /// and a DX1 beacon from the same engine carry the same text, just different
    /// waveform lengths.
    #[test]
    fn tier_switch_keeps_message_layer() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        e.set_tier(Tier::TempoFast); // default is now FT8; this test compares FT1 vs DX1
        e.set_beacon(true); // beacon off by default; this test compares beacon waveforms
        let ft1_wave = e.poll_tx(0);
        e.set_tier(Tier::TempoDeep);
        let dx1_wave = e.poll_tx(0);
        assert!(!ft1_wave.is_empty() && !dx1_wave.is_empty());
        assert_eq!(ft1_wave[0].len(), tempo_fast::NMAX); // 4 s FT1 frame
        assert_eq!(dx1_wave[0].len(), tempo_fast::deep::frame_len()); // ~9.9 s DX1 frame
        assert_ne!(ft1_wave[0].len(), dx1_wave[0].len());
    }

    #[test]
    fn cq_run_auto_answers_a_caller_on_the_next_over() {
        // THE on-air complaint: "I called CQ over and over; someone was calling me
        // and the CQ just kept going." A caller's grid reply observed mid-run MUST
        // flip the next over to the report — WSJT-X auto-sequencing.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_mode("qso-run").unwrap();
        let cq = e.poll_tx(0);
        assert!(!cq.is_empty(), "CQ goes out on our slot");
        // A caller answers in the next (their) slot with a grid, addressed to us.
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC FN42", -8)], 1);
        // Our next over must be the REPORT to K1ABC, not another CQ.
        let qso = e.snapshot().qso.expect("QSO state after a caller");
        assert_eq!(qso.dxcall.as_deref(), Some("K1ABC"), "answering the caller");
        let next = e.snapshot().qso.unwrap().tx_now.unwrap_or_default();
        assert!(
            next.contains("K1ABC") && !next.to_uppercase().starts_with("CQ"),
            "next over answers the caller (report), got {next:?}"
        );
    }

    #[test]
    fn cq_run_abandons_a_silent_caller_and_resumes_cq() {
        // Run resilience: a caller answers our CQ, then vanishes. After the default 3
        // unanswered overs of our report, the run must DROP them and return to calling CQ
        // so a dead caller can't stall the whole run (the `abandon_stalled` path).
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_mode("qso-run").unwrap();
        e.poll_tx(0); // CQ out on our slot
                      // A caller answers with a grid → we switch to sending the report (AwaitRoger).
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC FN42", -8)], 1);
        assert_eq!(
            e.snapshot().qso.unwrap().dxcall.as_deref(),
            Some("K1ABC"),
            "answering the caller"
        );
        // The caller goes silent. Each of our overs re-sends the report (tx_count++), and
        // their slots are empty. After 3 unanswered overs → abandon and resume CQ.
        for slot in [2u64, 4, 6] {
            e.poll_tx(slot);
            e.ingest_decodes_for_test(&[], slot + 1);
        }
        let qso = e.snapshot().qso.expect("still running");
        assert_eq!(
            qso.state, "CallingCq",
            "abandoned the dead caller, back to CQ"
        );
        assert!(
            qso.dxcall.is_none(),
            "the silent caller was dropped, got {:?}",
            qso.dxcall
        );
    }

    #[test]
    fn cq_run_stall_abandon_can_be_disabled() {
        // `cq_stall_overs = Some(0)` disables auto-abandon (stock WSJT-X: wait for the
        // operator). The run stays locked on the silent caller.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.settings.cq_stall_overs = Some(0);
        e.set_mode("qso-run").unwrap();
        e.poll_tx(0);
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC FN42", -8)], 1);
        for slot in [2u64, 4, 6, 8, 10] {
            e.poll_tx(slot);
            e.ingest_decodes_for_test(&[], slot + 1);
        }
        assert_eq!(
            e.snapshot().qso.unwrap().dxcall.as_deref(),
            Some("K1ABC"),
            "abandon disabled → still working the (silent) caller"
        );
    }

    #[test]
    fn roster_click_long_after_the_decode_still_resumes_at_the_report() {
        // THE double-click bug: the caller's grid line decoded SLOTS ago (the
        // per-slot last_decodes was since replaced); a roster click passes no
        // message text. The HISTORY must still resolve the context → Tx2, never
        // re-send our grid.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC FN42", -8)], 5);
        // Later slots decode other traffic — last_decodes no longer holds K1ABC.
        e.ingest_decodes_for_test(&[dec_snr("CQ N0OTH EM48", -3)], 7);
        e.ingest_decodes_for_test(&[dec_snr("CQ W5MORE EM12", -1)], 9);
        // Roster click: call with NO message context.
        e.call_station("K1ABC");
        let qso = e.snapshot().qso.expect("QSO started");
        let next = qso.tx_now.unwrap_or_default();
        assert!(
            !next.contains("EN37"),
            "must NOT fall back to Tx1/grid, got {next:?}"
        );
        assert!(next.contains("K1ABC"), "answers the caller, got {next:?}");
        // Parity derives from the ANSWERED decode's slot (5 → odd ingest → their
        // audio slot 4/even → we TX on odd), not from the unrelated slot-9 decode.
        assert!(!e.tx_even(), "TX parity opposite the CALLER's period");
    }

    #[test]
    fn broker_ptt_is_arbitrated() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tx_enabled(true);
        // Default OFF: a foreign app may never key without the opt-in.
        assert!(!e.broker_ptt(true));
        assert!(!e.manual_ptt());
        // Opted in + idle → allowed; the radio loop sees the key via manual_ptt().
        e.settings.cat_broker_ptt = true; // same-module test access
        assert!(e.broker_ptt(true));
        assert!(e.manual_ptt());
        // Un-key always lands.
        assert!(e.broker_ptt(false));
        assert!(!e.manual_ptt());
        // Nexus's own manual PTT wins — a broker key while held is refused.
        e.set_ptt(true);
        assert!(!e.broker_ptt(true), "operator PTT beats a foreign key");
        e.set_ptt(false);
        // The TX kill switch drops a held broker key.
        assert!(e.broker_ptt(true));
        e.set_tx_enabled(false);
        assert!(!e.broker_ptt_active(), "Monitor-off drops the foreign key");
    }

    #[test]
    fn working_a_spot_stamps_the_navigation_hint() {
        // The hint rides the snapshot so a pop-out window's Needed click can
        // land the MAIN window in the right cockpit.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        let t0 = e.snapshot().work_tick;
        e.work_spot_split("cw", 14.030, "20m", None);
        let s = e.snapshot();
        assert_eq!(s.work_tick, t0 + 1);
        assert_eq!(s.work_view.as_deref(), Some("cw"));
    }

    #[test]
    fn grid_rarity_resolver_stamps_decodes_and_roster() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        // Fake resolver: water-world — every grid is ultra-rare (tier 3).
        e.set_grid_rarity_resolver(|_g| Some(3));
        e.ingest_decodes_for_test(&[dec_snr("CQ K1ABC FN42", -5)], 1);
        let s = e.snapshot();
        let row = s
            .recent_decodes
            .iter()
            .find(|d| d.from.as_deref() == Some("K1ABC"))
            .expect("decode row");
        assert_eq!(row.grid.as_deref(), Some("FN42"));
        assert_eq!(row.grid_rarity, Some(crate::dto::GridRarity::UltraRare));
        let st = s
            .stations
            .iter()
            .find(|s| s.call == "K1ABC")
            .expect("roster");
        assert_eq!(st.grid_rarity, Some(crate::dto::GridRarity::UltraRare));
    }

    #[test]
    fn no_rarity_resolver_means_no_gems() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.ingest_decodes_for_test(&[dec_snr("CQ K1ABC FN42", -5)], 1);
        let s = e.snapshot();
        let row = s
            .recent_decodes
            .iter()
            .find(|d| d.from.as_deref() == Some("K1ABC"))
            .unwrap();
        assert_eq!(row.grid_rarity, None);
        assert_eq!(row.grid.as_deref(), Some("FN42"), "grid still carried");
    }

    #[test]
    fn rarity_backfills_onto_gridless_frames_from_the_roster() {
        // An ultra-rare station's CQ carries its grid, but its follow-up
        // report/RR73 frames do not. The rarity badge must persist across the
        // whole QSO by falling back to the grid remembered in the roster — so the
        // ULTRA pill doesn't vanish on the row the operator is watching mid-QSO.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_grid_rarity_resolver(|_g| Some(3)); // water-world → ultra
        e.ingest_decodes_for_test(&[dec_snr("CQ K1ABC FN42", -5)], 1); // grid learned
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC -10", -5)], 2); // report, no grid
        let s = e.snapshot();
        let report = s
            .recent_decodes
            .iter()
            .find(|d| d.from.as_deref() == Some("K1ABC") && d.grid.is_none())
            .expect("gridless report row from K1ABC");
        assert_eq!(
            report.grid_rarity,
            Some(crate::dto::GridRarity::UltraRare),
            "rarity backfilled from the roster grid on a gridless frame"
        );
    }

    #[test]
    fn band_change_clears_the_roster_but_in_band_qsy_keeps_it() {
        // Operator report: after a band QSY the roster still showed the old
        // band's stations — stale presence (they aren't on the new frequency).
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.ingest_decodes_for_test(&[dec_snr("CQ K1ABC FN42", -5)], 1);
        assert!(!e.snapshot().stations.is_empty(), "heard station on roster");
        // In-band QSY (same band label) keeps the roster — same activity.
        let radio = e.snapshot().radio;
        e.set_frequency(radio.dial_mhz + 0.002, &radio.band, &radio.sideband);
        assert!(
            !e.snapshot().stations.is_empty(),
            "in-band QSY keeps roster"
        );
        // Cross-band QSY wipes it.
        let target = if radio.band.eq_ignore_ascii_case("40m") {
            "20m"
        } else {
            "40m"
        };
        e.set_frequency(7.074, target, &radio.sideband);
        assert!(
            e.snapshot().stations.is_empty(),
            "cross-band QSY clears roster"
        );
    }

    #[test]
    fn roster_click_on_a_cqing_station_picks_the_opposite_cycle() {
        // THE same-cycle bug (operator report, 6m): the DX is calling CQ (so
        // nothing is addressed to me), the roster click passes no message text,
        // and a LATER decode from an unrelated station sits on the opposite
        // parity. Parity must derive from the CLICKED station's own CQ — never
        // from the unrelated most-recent decode (that transmitted on top of them).
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        // PJ4DX's CQ ingests at slot 5 → their audio slot 4 (even): they TX even.
        e.ingest_decodes_for_test(&[dec_snr("CQ PJ4DX FK52", -10)], 5);
        // Unrelated traffic ingests at slot 6 — the bare last_decode_slot
        // fallback would put us on PJ4DX's own (even) cycle.
        e.ingest_decodes_for_test(&[dec_snr("CQ N0OTH EM48", -3)], 6);
        e.call_station("PJ4DX");
        assert_eq!(e.snapshot().qso.unwrap().dxcall.as_deref(), Some("PJ4DX"));
        assert!(!e.tx_even(), "TX parity opposite the CQing DX's period");
    }

    #[test]
    fn call_cq_arms_the_immediate_path() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        assert!(!e.peek_immediate_tx());
        e.set_mode("qso-run").unwrap();
        assert!(
            e.peek_immediate_tx(),
            "Call CQ requests the current period (WSJT-X-snappy)"
        );
    }

    #[test]
    fn band_qsy_forgets_the_old_bands_callers() {
        // A caller decoded on 20 m must NOT be auto-answered after a QSY to 40 m —
        // they aren't in this activity, and their slot parity belongs to the old run.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC FN42", -8)], 5);
        e.set_frequency(7.074, "40m", "USB");
        e.call_station("K1ABC");
        let next = e.snapshot().qso.unwrap().tx_now.unwrap_or_default();
        assert!(
            next.contains("EN37"),
            "no stale context → fresh grid start, got {next:?}"
        );
    }

    #[test]
    fn cq_cap_setting_flows_into_a_cq_run() {
        // The opt-in "stop CQ after N calls" must travel settings -> station.
        // (The field is a plain pub Option with a None default — without this
        // test a future construction site that forgets the assignment would
        // silently revert that path to uncapped.)
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.settings.cq_max_calls = Some(2);
        e.set_mode("qso-run").unwrap();
        assert!(!e.poll_tx(0).is_empty(), "CQ call 1");
        assert!(!e.poll_tx(2).is_empty(), "CQ call 2");
        assert!(
            e.poll_tx(4).is_empty(),
            "capped CQ stops after the configured budget"
        );
        // Default (None) stays stock-uncapped — pinned in tempo-core's
        // uncapped_cq_repeats_indefinitely_like_stock_wsjtx.
    }

    #[test]
    fn disable_tx_after_73_is_deferred_and_snp_only() {
        // Stock behavior option (default ON): after OUR final 73 of an S&P
        // contact goes out, Enable-Tx drops — but only via the DEFERRED flag
        // (an immediate drop would hard-stop the playing 73 mid-over).
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.call_station("W9XYZ"); // S&P: we answer them (arms TX)
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        // Our final 73 goes out on the next own-parity slot.
        assert!(!e.poll_tx(4).is_empty(), "the closing 73 transmits");
        assert!(e.tx_enabled(), "NOT disabled mid-over (deferred)");
        assert!(e.take_pending_tx_disable(), "deferral armed for the loop");
        // A CQ run must NOT arm it (Run returns to CQ instead).
        let mut r = Engine::new("W9XYZ", "EN37", 0);
        r.set_mode("qso-run").unwrap();
        let _ = r.poll_tx(0);
        r.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC FN42", -8)], 1);
        let _ = r.poll_tx(2); // report
        r.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC R-05", -8)], 3);
        let _ = r.poll_tx(4); // RR73 → resume CQ
        assert!(!r.take_pending_tx_disable(), "a run never self-disarms");
    }

    #[test]
    fn override_next_tx_forces_the_slot_and_rejoins_the_sequence() {
        // The WSJT-X Tx-slot click: force a specific message; the sequencer
        // resumes normally from the partner's matching reply.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.override_next_tx("K1ABC", Some("FN42"), "K1ABC W9XYZ -12");
        let qso = e.snapshot().qso.expect("QSO targeted");
        assert_eq!(qso.dxcall.as_deref(), Some("K1ABC"));
        assert_eq!(qso.tx_now.as_deref(), Some("K1ABC W9XYZ -12"), "forced Tx2");
        assert!(e.tx_enabled(), "Tx-slot click arms (stock default)");
        // Their R-report advances the forced flow exactly like a natural one.
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC R-08", -8)], 5);
        let next = e.snapshot().qso.unwrap().tx_now.unwrap_or_default();
        assert!(next.contains("RR73"), "sequence rejoined, got {next:?}");
        // Own call is guarded here too.
        let before = e.snapshot().qso;
        e.override_next_tx("W9XYZ", None, "W9XYZ W9XYZ 73");
        assert_eq!(e.snapshot().qso, before, "self-override ignored");
    }

    #[test]
    fn stale_after_73_disable_cannot_kill_the_next_arm() {
        // The review-found leak: the deferred disable armed by contact A's 73
        // must die the moment ANY new arm happens (Call CQ / Enable-Tx /
        // Tx-slot click) — otherwise the loop's consume tick silently disarmed
        // the brand-new run.
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        assert!(!e.poll_tx(4).is_empty(), "the 73 goes out, one-shot armed");
        e.set_mode("qso-run").unwrap(); // operator: Call CQ right away
        assert!(
            !e.take_pending_tx_disable(),
            "the new arm cancels the stale one-shot"
        );
        assert!(e.tx_enabled(), "the CQ run stays armed");
    }

    #[test]
    fn split_operation_reduces_audio_and_reports_the_dial_shift() {
        // WSJT-X Split: f0 = 1500 + (tx-1500) mod 500; shift = tx - f0 (500-Hz
        // steps), RF identical. Verified through poll_tx's side channel.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tier(Tier::Ft8);
        e.settings.split_mode = crate::settings::SplitMode::FakeIt;
        e.set_tx_enabled(true);
        // 750 Hz: audio must come up to 1750, dial down 1000.
        e.set_tx_offset(750.0);
        e.broadcast("CQ W9XYZ EN37");
        assert!(!e.poll_tx(0).is_empty());
        assert_eq!(e.take_tx_dial_shift(), -1000, "750 -> f0 1750, dial -1000");
        // 2600 Hz: f0 1600, dial +1000.
        e.set_tx_offset(2600.0);
        e.broadcast("CQ W9XYZ EN37");
        assert!(!e.poll_tx(2).is_empty());
        assert_eq!(e.take_tx_dial_shift(), 1000, "2600 -> f0 1600, dial +1000");
        // Already in the clean window: untouched.
        e.set_tx_offset(1700.0);
        e.broadcast("CQ W9XYZ EN37");
        assert!(!e.poll_tx(4).is_empty());
        assert_eq!(e.take_tx_dial_shift(), 0, "1500-2000 Hz needs no shift");
        // Extremes + the exact window edge.
        e.set_tx_offset(200.0); // passband floor → f0 1700, dial -1500
        e.broadcast("CQ W9XYZ EN37");
        assert!(!e.poll_tx(6).is_empty());
        assert_eq!(e.take_tx_dial_shift(), -1500, "200 -> f0 1700, dial -1500");
        e.set_tx_offset(2900.0); // high tone → f0 1900, dial +1000
        e.broadcast("CQ W9XYZ EN37");
        assert!(!e.poll_tx(8).is_empty());
        assert_eq!(e.take_tx_dial_shift(), 1000, "2900 -> f0 1900, dial +1000");
        e.set_tx_offset(2000.0); // window edge is INCLUSIVE — no spurious hop
        e.broadcast("CQ W9XYZ EN37");
        assert!(!e.poll_tx(10).is_empty());
        assert_eq!(e.take_tx_dial_shift(), 0, "2000 Hz is in-window");
        // Split off (stock default): raw offset, no shift.
        e.settings.split_mode = crate::settings::SplitMode::None;
        e.set_tx_offset(750.0);
        e.broadcast("CQ W9XYZ EN37");
        assert!(!e.poll_tx(12).is_empty());
        assert_eq!(e.take_tx_dial_shift(), 0, "None = stock raw-offset TX");
    }

    #[test]
    fn working_frequency_override_moves_the_watering_hole() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tier(Tier::Ft8);
        // Stock 20 m FT8 = 14.074; the operator overrides to 14.071.
        e.settings.working_frequencies = vec![crate::settings::WorkingFreq {
            band: "20m".into(),
            mode: "FT8".into(),
            mhz: 14.071,
        }];
        let plan = e.band_plan();
        let c = plan.iter().find(|c| c.band == "20m").unwrap();
        assert!((c.dial_mhz - 14.071).abs() < 1e-9, "override applied");
        // FT4 untouched by an FT8 override.
        e.set_tier(Tier::Ft4);
        let c = e.band_plan().into_iter().find(|c| c.band == "20m").unwrap();
        assert!((c.dial_mhz - 14.080).abs() < 1e-9, "FT4 stock kept");
    }

    #[test]
    fn directed_cq_run_persists_across_the_pileup() {
        // "CQ DX" must go out directed AND stay directed after a completed
        // contact returns the run to CQ (stock: the edited Tx6 text persists).
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.start_cq(Some("DX")).unwrap();
        let q = e.snapshot().qso.unwrap();
        assert_eq!(q.tx_now.as_deref(), Some("CQ DX W9XYZ EN37"));
        // Work a full pileup contact…
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC FN42", -8)], 1);
        let _ = e.poll_tx(2); // report
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC R-05", -8)], 3);
        let _ = e.poll_tx(4); // RR73 goes out
                              // Return-to-CQ is evaluated on the NEXT ingest (observe_modes), like a
                              // real period boundary following the RR73 over.
        e.ingest_decodes_for_test(&[dec_snr("CQ N0OTH EM48", -3)], 5);
        let q = e.snapshot().qso.unwrap();
        assert_eq!(
            q.tx_now.as_deref(),
            Some("CQ DX W9XYZ EN37"),
            "the run returns to the DIRECTED CQ"
        );
        // A plain start clears the sticky token.
        e.start_cq(None).unwrap();
        assert_eq!(
            e.snapshot().qso.unwrap().tx_now.as_deref(),
            Some("CQ W9XYZ EN37")
        );
    }

    #[test]
    fn redecode_ingests_only_newly_found_lines() {
        // F6 re-runs the decoder over the retained period audio; an already-
        // ingested message must NOT double-ingest (double-observe / dup rows).
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        let frame = native_frame_for(modes::ModeKind::Ft8, "CQ W1ABC FN42", 1500.0);
        assert!(e.ingest(&frame, 2) >= 1, "first pass decodes");
        let rows_before = e
            .decode_history
            .iter()
            .filter(|(_, d)| d.message == "CQ W1ABC FN42")
            .count();
        assert_eq!(e.redecode(), 0, "same audio, same settings: nothing new");
        let rows_after = e
            .decode_history
            .iter()
            .filter(|(_, d)| d.message == "CQ W1ABC FN42")
            .count();
        assert_eq!(rows_before, rows_after, "no duplicate history rows");
    }

    /// Decode at a specific audio frequency (Hound freq-rule tests).
    fn dec_at(msg: &str, snr: i32, freq: f32) -> Decode {
        let mut d = dec_snr(msg, snr);
        d.freq = freq;
        d
    }

    #[test]
    fn hound_works_a_fox_through_a_multi_payload_exchange() {
        // The full DXpedition hound exchange against a Fox that packs two
        // payloads per transmission ("<other> RR73; <us> <fox> rpt").
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tier(Tier::Ft8);
        e.settings.special_op = crate::settings::SpecialOp::Hound;
        e.set_tx_offset(450.0); // operator parked in the Fox's segment
                                // Double-click the Fox's CQ.
        e.ingest_decodes_for_test(&[dec_at("CQ DX PJ4DX", -10, 400.0)], 1);
        e.call_station_ctx("PJ4DX", None, Some("CQ DX PJ4DX"), Some(-10), Some(400.0));
        assert!(
            e.tx_offset_hz() >= 1000.0,
            "hound calls ABOVE 1000 Hz, got {}",
            e.tx_offset_hz()
        );
        // The Fox answers US inside a multi-payload transmission at 320 Hz.
        e.ingest_decodes_for_test(&[dec_at("K1ABC RR73; W9XYZ PJ4DX -08", -10, 320.0)], 3);
        let q = e.snapshot().qso.expect("hound QSO running");
        let next = q.tx_now.unwrap_or_default();
        assert!(
            next.contains("R-") || next.contains("R+"),
            "the R+report is queued, got {next:?}"
        );
        assert!(
            (e.tx_offset_hz() - 320.0).abs() < 1.0,
            "TX moved to the FOX's frequency for the R+report, got {}",
            e.tx_offset_hz()
        );
        // The Fox confirms us (again multiplexed) — contact complete + logged.
        e.ingest_decodes_for_test(&[dec_at("W9XYZ RR73; N0CALL PJ4DX +03", -10, 320.0)], 5);
        assert!(
            !e.get_log().is_empty(),
            "the hound contact logged on the Fox's RR73"
        );
        // And the hound goes SILENT: no parting 73 may key up in the Fox's
        // segment (stock WSJT-X hounds log and stop; ours transmitted one 73
        // at the Fox's own frequency until the quiet_finish fix).
        let after = e.snapshot().qso.and_then(|q| q.tx_now);
        assert!(
            after
                .as_deref()
                .is_none_or(|t| !t.contains("73") || t.contains("RR73")),
            "hound must not queue a parting 73 after the Fox's RR73, got {after:?}"
        );
    }

    #[test]
    fn bystander_73_never_fabricates_a_fox_confirmation() {
        // Review-found trap: while hounding, a third station's plain 2-token
        // "W9XYZ 73" free text must NOT get the Fox's call attached — that
        // falsely COMPLETED and logged the QSO. Only the halves of a real Fox
        // multiplex ever drop their sender.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tier(Tier::Ft8);
        e.settings.special_op = crate::settings::SpecialOp::Hound;
        e.ingest_decodes_for_test(&[dec_at("CQ DX PJ4DX", -10, 400.0)], 1);
        e.call_station_ctx("PJ4DX", None, Some("CQ DX PJ4DX"), Some(-10), Some(400.0));
        e.ingest_decodes_for_test(&[dec_at("K1ABC RR73; W9XYZ PJ4DX -08", -10, 320.0)], 3);
        // A bystander signs a free-text 73 that happens to carry OUR call.
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ 73", -3)], 5);
        assert!(
            e.get_log().is_empty(),
            "the QSO must NOT complete from a bystander's 73"
        );
        // The REAL multiplexed confirm still completes it.
        e.ingest_decodes_for_test(&[dec_at("W9XYZ RR73; N0CALL PJ4DX +03", -10, 320.0)], 7);
        assert!(!e.get_log().is_empty(), "the Fox's multiplexed RR73 logs");
    }

    #[test]
    fn fox_split_is_gated_to_hound_mode() {
        // Normal operation must never split on ';' (free text could carry it).
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.ingest_decodes_for_test(&[dec_snr("K1ABC RR73; W9XYZ PJ4DX -08", -10)], 1);
        assert_eq!(e.last_decodes().len(), 1, "no split outside Hound mode");
    }

    #[test]
    fn hunt_target_tags_only_the_matching_qso() {
        // One-click hunt: the NEXT logged QSO with the activator's call gets
        // SIG/SIG_INFO (hunter credit); other contacts never inherit the park,
        // and the pend clears once used.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_hunt_target("K1ABC", "POTA", "K-1234").unwrap();
        assert!(e.hunt_target().is_some());
        // A DIFFERENT station logged first must not get the park.
        let mut other = e.qso_record("N0OTH".into(), None, Some(-5));
        other.when_unix = 1;
        e.log_qso(other);
        let log = e.get_log();
        assert!(log
            .iter()
            .any(|r| r.call == "N0OTH" && r.ota.their_ref.is_none()));
        assert!(
            e.hunt_target().is_some(),
            "pend survives a non-matching QSO"
        );
        // The activator (portable suffix tolerated) gets tagged; pend clears.
        let mut rec = e.qso_record("K1ABC/P".into(), None, Some(-7));
        rec.when_unix = 2;
        e.log_qso(rec);
        let log = e.get_log();
        let hit = log.iter().find(|r| r.call == "K1ABC/P").unwrap();
        assert_eq!(hit.ota.their_program.as_deref(), Some("POTA"));
        assert_eq!(hit.ota.their_ref.as_deref(), Some("K-1234"));
        assert!(e.hunt_target().is_none(), "pend cleared after tagging");
        // The parks-worked index picked it up — the NEW PARK badge flips off.
        assert!(e.park_worked("k-1234"), "case-insensitive park index");
        assert!(!e.park_worked("K-9999"));
    }

    fn dec_snr(msg: &str, snr: i32) -> Decode {
        Decode {
            message: msg.to_string(),
            sync: 1.0,
            snr,
            dt: 0.1,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode: None,
        }
    }

    #[test]
    fn call_station_targets_the_dxcall() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.call_station("W9XYZ");
        let snap = e.snapshot();
        assert_eq!(snap.mode, OpMode::Qso);
        let qso = snap.qso.expect("QSO status present after call_station");
        assert_eq!(qso.dxcall.as_deref(), Some("W9XYZ"));
        assert!(qso.running, "directed call runs the responder side");
    }

    #[test]
    fn per_mode_power_ceiling_clamps_and_re_clamps_on_mode_change() {
        // SAFETY feature: FT8/FT4/RTTY run ~100% duty and can cook finals at a level fine for
        // SSB, so the operator caps power PER MODE. Two guarantees, both here.
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.settings.max_power_digital = Some(0.30); // 30% cap for the data modes
                                                   // No cap on phone/CW.

        // (1) The set chokepoint clamps. In a digital mode, a 100% request is held to 30%.
        e.set_operating_mode("digital", false);
        e.set_rf_power(1.0);
        assert_eq!(
            e.rf_power(),
            Some(0.30),
            "digital request clamped to the 30% cap"
        );
        // A request UNDER the cap is untouched.
        e.set_rf_power(0.10);
        assert_eq!(e.rf_power(), Some(0.10), "under-cap request passes through");

        // Uncapped mode → full power allowed.
        e.set_operating_mode("phone", false);
        e.set_rf_power(1.0);
        assert_eq!(e.rf_power(), Some(1.0), "phone is uncapped → full power");

        // (2) The mode-change re-clamp: at full power in SSB, switching to FT8 must bring the rig
        // DOWN to the cap immediately, not wait for a slider touch — the whole point of a
        // duty-cycle safety cap.
        e.set_operating_mode("digital", false);
        assert_eq!(
            e.rf_power(),
            Some(0.30),
            "SSB→FT8 re-clamped the commanded power down to the cap"
        );
        // And back to an uncapped mode leaves the (now-low) power put — it does not push it up.
        e.set_operating_mode("cw", false);
        assert_eq!(
            e.rf_power(),
            Some(0.30),
            "uncapped mode never RAISES power on its own"
        );
    }

    #[test]
    fn power_cap_is_a_continuous_ceiling_not_a_one_shot() {
        // Operator report (FTdx10, 2026-07-23): the cap "worked once", then a manual knob-up back
        // to 100% stuck and re-entering a mode didn't re-clamp. The cap must be a HARD, continuous
        // ceiling enforced against the rig's ACTUAL level, not just the commanded value.
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.settings.max_power_phone = Some(0.75);
        e.set_operating_mode("phone", false);
        e.set_rf_power(0.75); // commanded at the cap
        assert_eq!(
            e.rf_power_to_command(),
            Some((0.75, false)),
            "settled at the cap → no force"
        );

        // The operator turns the RIG's own knob up to 100% (OBSERVED, not commanded). The old loop
        // saw the commanded value unchanged and never re-commanded — the rig stayed hot. Now the
        // cap pulls it back down and FORCES the command even though our target didn't change.
        e.observe_rig_power(1.0);
        assert_eq!(
            e.rf_power_to_command(),
            Some((0.75, true)),
            "rig observed above the cap → clamp down and force"
        );
        assert_eq!(e.rf_power(), Some(0.75), "captured as run-at-the-cap");

        // Once the rig settles at the cap, no more forcing (no oscillation on readback).
        e.observe_rig_power(0.75);
        assert_eq!(e.rf_power_to_command(), Some((0.75, false)));

        // A knob-DOWN below the cap is the operator's choice — never forced back up.
        e.set_rf_power(0.40);
        e.observe_rig_power(0.40);
        assert_eq!(
            e.rf_power_to_command(),
            Some((0.40, false)),
            "below the cap is left alone"
        );

        // No cap on CW → never forces, even with the rig at 100%.
        e.set_operating_mode("cw", false);
        e.observe_rig_power(1.0);
        assert_eq!(
            e.rf_power_to_command().map(|(_, f)| f),
            Some(false),
            "uncapped mode never forces"
        );
    }

    #[test]
    fn working_a_cq_answerer_keeps_the_report_it_already_armed() {
        // Operator report, 2026-07-25: "grid, RST etc intermittently fail, but the log seems to
        // have it right in almost every case… calling CQ, then clicking Work or Call. That first
        // cycle seems more likely to fail."
        //
        // The CQ path is the one that loses it. A caller answers our CQ with their grid, so
        // `Station::observe` advances CallingCq→AwaitRoger and ARMS the report on its own. The
        // operator then clicks Work on that same station: `call_station_ctx` rebuilds the station
        // (landing back on AwaitRoger with the same report armed) and used to wipe
        // `qso_report_sent`. The only other place that captures it is `observe_modes`, which runs
        // off a decode fold — and no decode folds during our own TX slot. By the next fold the
        // QSO has advanced to RR73, `report_in` sees no report, and the contact logs with a blank
        // RST_SENT. S&P never hits this: there the advance CREATES the outgoing report, so the
        // same pass captures it.
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_tier(Tier::TempoFast);
        let _ = e.start_cq(None);

        // W9XYZ answers our CQ with their grid; we decode them at -7, so -7 is the report we owe.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ EM48", -7)], 1);
        assert_eq!(
            e.qso_report_sent,
            Some(-7),
            "the auto-answer armed the report and observe_modes captured it"
        );

        // The operator clicks Work on that same caller (the report is already armed).
        e.call_station("W9XYZ");
        assert_eq!(
            e.qso_report_sent,
            Some(-7),
            "the click must not discard the report the rebuilt station still has armed"
        );

        // They roger our report (→ we send RR73), then sign 73 and the QSO reaches Done.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ R-10", -7)], 3);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ 73", -7)], 5);
        let log = e.get_log();
        assert_eq!(log.len(), 1, "the QSO auto-logged");
        assert_eq!(
            log[0].rst_sent.as_deref(),
            Some("-7"),
            "RST_SENT survives working a station that answered our CQ"
        );
    }

    #[test]
    fn qso_snapshot_carries_the_same_grid_the_log_will() {
        // The cockpit's DX Grid box reads this field. It must resolve exactly as the logged
        // GRIDSQUARE does — the exchange's grid, else the roster's — or the operator sees a
        // blank grid on screen for a contact that logs correctly (operator report, 2026-07-25).
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_tier(Tier::TempoFast);
        // We hear W9XYZ's CQ first, so the roster learns EM48.
        e.ingest_decodes_for_test(&[dec_snr("CQ W9XYZ EM48", -7)], 1);
        let _ = e.start_cq(None);
        // They come back with a BARE REPORT — the common FT8 form, carrying no grid at all.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ +17", -7)], 3);

        let snap = e.snapshot();
        let qso = snap.qso.expect("a QSO is in progress");
        assert_eq!(qso.dxcall.as_deref(), Some("W9XYZ"));
        assert_eq!(
            qso.dxgrid.as_deref(),
            Some("EM48"),
            "the snapshot backfills the grid from the roster, exactly as the log does"
        );
    }

    /// REGRESSION (operator, on-air 2026-07-26): "things I have already worked are not being
    /// removed from the Needed section, and the pills that designate why I needed them do not
    /// get removed." Root cause was NOT a stale cache or a refresh gap — `log_qso` already
    /// refreshes the worked index, and the decode rows are rebuilt every snapshot. It was that
    /// ONE question ("what state is this call in?") had TWO resolvers on the two sides of the
    /// same comparison: the HEARD side used the FCC callsign index, while the WORKED side could
    /// only learn it from a logged ADIF STATE — and `qso_record` hardcoded `state: None`. So
    /// `LogNeeds.worked_states` never gained the state just worked and `NeedTag::NewState`
    /// (tier 60 — above NewGrid/NewBand, so it becomes tags[0]: the lead pill AND the row
    /// colour) re-fired forever. Fixed the same way `country` already was: an injected resolver
    /// filled at the one funnel every log path passes through.
    ///
    /// Uses the FT1 sequence because it is the known-good completion path in these tests; the
    /// record builder (`qso_record`) is shared with FT8/FT4, which is where the operator hit it.
    #[test]
    fn auto_logged_qso_carries_the_worked_station_state() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_state_resolver(|call, _grid| (call == "W9XYZ").then(|| "VT".to_string()));
        e.set_tier(Tier::TempoFast);

        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);

        let log = e.get_log();
        assert_eq!(log.len(), 1, "completed QSO auto-logs exactly one record");
        assert_eq!(
            log[0].state.as_deref(),
            Some("VT"),
            "an auto-logged QSO must carry the worked station's state — without it \
             worked_states can never learn it and NewState re-fires forever"
        );
    }

    /// Sibling of the NewState defect, same class: a QSO that logs with a BLANK grid can never
    /// teach `worked_grids` anything, so NewGrid keeps firing for a grid already worked. The
    /// session roster covers a station heard this session; this covers one worked on a previous
    /// day whose CQ we haven't decoded since.
    #[test]
    fn a_gridless_qso_falls_back_to_the_grid_we_logged_before() {
        let mut e = Engine::new("K2DEF", "FN31", 0);

        // A previous contact where they DID send a grid.
        let mut first = e.qso_record("W9XYZ".into(), Some("EN37".into()), None);
        first.grid = Some("EN37".into());
        e.log_qso(first);

        // Today they answer our CQ with a bare report — the common FT8 form, no grid in the
        // QSO at all, and nothing in this session's roster either.
        let rec = e.qso_record("W9XYZ".into(), None, None);
        assert_eq!(
            rec.grid.as_deref(),
            Some("EN37"),
            "a gridless QSO must reuse the grid we already logged for that call, or the grid \
             can never be credited and NewGrid fires forever"
        );
    }

    /// The honest limit, pinned deliberately. There is no callsign→grid index the way FCC gives
    /// callsign→state, so a station whose grid we have NEVER seen still logs blank — and still
    /// reports NewGrid, which is correct: we cannot credit a grid we do not know. If this test
    /// ever fails, someone has started inventing grids from callsign prefixes.
    #[test]
    fn an_unknown_grid_is_left_blank_rather_than_invented() {
        let e = Engine::new("K2DEF", "FN31", 0);
        let rec = e.qso_record("W9XYZ".into(), None, None);
        assert_eq!(rec.grid, None);
    }

    /// The funnel itself, mode-independent: every path (auto-log, cockpit Log button, manual
    /// form) goes through `log_qso`, so the fill belongs there and not in one mode's builder.
    #[test]
    fn log_qso_fills_a_missing_state_from_the_resolver() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_state_resolver(|call, _grid| (call == "W1ABC").then(|| "MA".to_string()));

        let mut rec = e.qso_record("W1ABC".into(), None, None);
        rec.state = None;
        e.log_qso(rec);

        assert_eq!(
            e.get_log()[0].state.as_deref(),
            Some("MA"),
            "log_qso must fill a missing state, like it already fills a missing country"
        );
    }

    /// The negative twin — pins that this is resolver-driven and never fabricated. With no
    /// resolver wired (headless, or FCC index absent) the state stays None rather than being
    /// guessed, and an explicitly-logged state is never overwritten.
    #[test]
    fn log_qso_never_invents_or_overwrites_a_state() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        let rec = e.qso_record("W1ABC".into(), None, None);
        e.log_qso(rec);
        assert_eq!(
            e.get_log()[0].state,
            None,
            "no resolver wired ⇒ no state, never a guess"
        );

        let mut e2 = Engine::new("K2DEF", "FN31", 0);
        e2.set_state_resolver(|_, _| Some("XX".to_string()));
        let mut rec2 = e2.qso_record("W1ABC".into(), None, None);
        rec2.state = Some("MA".into());
        e2.log_qso(rec2);
        assert_eq!(
            e2.get_log()[0].state.as_deref(),
            Some("MA"),
            "an operator-supplied state must win over the resolver"
        );
    }

    #[test]
    fn completed_qso_auto_logs_one_record() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_tier(Tier::TempoFast); // this test asserts the FT1 path (default is now FT8)
        assert!(e.settings().auto_log, "auto_log on by default");
        assert!(e.get_log().is_empty(), "log starts empty");

        // Operator works W9XYZ: send grid, get a report, get RR73 → QSO done.
        e.call_station("W9XYZ");
        // W9XYZ answers our grid with a report about our signal (-10) — we send
        // them back R<our report>, capturing rst_sent from our outgoing.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        // W9XYZ rogers → we send 73 and the QSO reaches Done.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);

        // Exactly one record was auto-logged.
        let log = e.get_log();
        assert_eq!(log.len(), 1, "completed QSO auto-logs exactly one record");
        let r = &log[0];
        assert_eq!(r.call, "W9XYZ");
        assert_eq!(r.band, "20m");
        assert_eq!(r.mode, "TempoFast");
        assert_eq!(
            r.rst_rcvd.as_deref(),
            Some("-10"),
            "report received about our signal"
        );
        assert_eq!(r.rst_sent.as_deref(), Some("-7"), "report we sent the DX");

        // worked_before now true (reflected in the snapshot's worked flag).
        let snap = e.snapshot();
        assert!(
            snap.stations.iter().any(|s| s.call == "W9XYZ" && s.worked),
            "W9XYZ shows worked-before after the logged QSO"
        );

        // Idempotent: re-observing in the Done state does not double-log.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 5);
        assert_eq!(e.get_log().len(), 1, "auto-log fires exactly once per QSO");
    }

    /// Operator report (2026-07-23): a caller who answers our CQ with a bare report — the common
    /// FT8 form, `KD9TAW K0SSD +17`, no grid — logged with a BLANK grid. But we'd decoded that
    /// station's grid from its own CQ, and the roster remembers it. The logged record must
    /// backfill the grid from the roster, exactly as WSJT-X fills its Log dialog from history.
    #[test]
    fn a_gridless_report_answer_backfills_the_grid_from_the_roster() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_tier(Tier::TempoFast);
        e.set_mode("qso-run").unwrap(); // we are CALLING CQ

        // We first hear W9XYZ's own CQ, carrying their grid — the roster records it. It is not
        // addressed to us, so it does not touch the sequencer; we keep calling CQ.
        e.ingest_decodes_for_test(&[dec_snr("CQ W9XYZ JN47", -6)], 1);

        // W9XYZ then answers OUR CQ with a bare report — no grid in this message at all.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 3);
        // They roger → we send 73 → the QSO completes and auto-logs.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 5);

        let log = e.get_log();
        assert_eq!(log.len(), 1, "the QSO auto-logged");
        assert_eq!(
            log[0].grid.as_deref(),
            Some("JN47"),
            "grid backfilled from the roster (the caller's earlier CQ), not left blank"
        );
    }

    /// CONFIRMED on-air bug (KD9TAW ALL.TXT, 2026-07-23): running CQ, a caller answers with a
    /// BARE REPORT (no grid), so we become the responder WHILE cq_running stays true. On their
    /// RR73 we must send the closing 73 before returning to CQ — the real log showed
    /// `KD2VRL KD9TAW R-11` → `KD9TAW KD2VRL RR73` → `CQ KD9TAW` with the 73 skipped, because
    /// `resume_cq` fired on a bare `Done` and clobbered the queued `Bye73`.
    #[test]
    fn a_cq_run_responder_sends_its_closing_73_before_resuming_cq() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_tier(Tier::TempoFast);
        e.set_mode("qso-run").unwrap(); // calling CQ, cq_running = true
        assert!(!e.poll_tx(0).is_empty(), "CQ goes out");

        // A caller answers our CQ with a bare report — no grid → we become the RESPONDER, and
        // (unlike a call_station click) cq_running stays true.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF KD2VRL -09", -7)], 1);
        assert_eq!(
            e.snapshot().qso.as_ref().and_then(|q| q.dxcall.as_deref()),
            Some("KD2VRL"),
            "locked onto the report-first caller"
        );
        // They confirm with RR73 → we owe them the final 73.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF KD2VRL RR73", -7)], 3);

        // THE FIX: `resume_cq` must NOT have replaced the QSO with a fresh CQ — our next queued
        // over is the 73 to KD2VRL. (Pre-fix, this was already `CQ K2DEF`.)
        let owed = e
            .snapshot()
            .qso
            .as_ref()
            .and_then(|q| q.tx_now.clone())
            .unwrap_or_default();
        assert!(
            owed.contains("KD2VRL") && !owed.to_uppercase().starts_with("CQ"),
            "closing 73 to KD2VRL is still queued, not a CQ, got {owed:?}"
        );
        assert!(!e.poll_tx(4).is_empty(), "the closing 73 transmits");

        // Only AFTER the 73 has gone out does the run resume calling CQ (one quiet slot later).
        e.ingest_decodes_for_test(&[], 5);
        let cq = e
            .snapshot()
            .qso
            .as_ref()
            .and_then(|q| q.tx_now.clone())
            .unwrap_or_default();
        assert!(
            cq.to_uppercase().starts_with("CQ"),
            "resumes CQ after the 73, got {cq:?}"
        );
    }

    /// The control: with NO grid ever heard for the caller, the record is honestly blank — the
    /// backfill invents nothing, it only recovers a grid we actually decoded.
    #[test]
    fn a_gridless_answer_with_no_known_grid_logs_blank_not_fabricated() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_tier(Tier::TempoFast);
        e.set_mode("qso-run").unwrap();
        // No prior CQ from W9XYZ → the roster has no grid for them.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 3);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 5);
        let log = e.get_log();
        assert_eq!(log.len(), 1);
        assert_eq!(
            log[0].grid.as_deref(),
            None,
            "no known grid → blank, not fabricated"
        );
    }

    /// THE connector-upload funnel: every log_qso path — the engine auto-log
    /// included — queues the record for the shell's QRZ/ClubLog/eQSL worker.
    /// (The original bug: auto-logged FT8 QSOs never reached any connector
    /// because pushes were wired only to UI-initiated log paths.)
    #[test]
    fn every_log_path_queues_a_pending_upload() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_tier(Tier::TempoFast);

        // Auto-logged QSO (the engine path that used to skip connectors).
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        let pending = e.take_pending_uploads();
        assert_eq!(pending.len(), 1, "auto-logged QSO queues for upload");
        assert_eq!(pending[0].rec.call, "W9XYZ");
        assert_eq!(
            pending[0].legs,
            upload_legs::ALL,
            "first attempt owes every leg"
        );

        // Drain is a real drain.
        assert!(e.take_pending_uploads().is_empty(), "queue empties on take");

        // Manual log path queues too.
        e.log_qso(QsoRecord {
            call: "N0CALL".into(),
            grid: Some("EN52".into()),
            country: None,
            state: None,
            band: "20m".into(),
            freq_mhz: 14.074,
            mode: "FT8".into(),
            rst_sent: None,
            rst_rcvd: None,
            name: None,
            qth: None,
            comment: None,
            notes: None,
            tx_power: None,
            when_unix: 0,
            time_off_unix: None,
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: Default::default(),
            qsl_sent: Default::default(),
            credit_granted: vec![],
            credit_submitted: vec![],
            upload: Default::default(),
            ota: Default::default(),
        });
        let pending = e.take_pending_uploads();
        assert_eq!(pending.len(), 1, "manual log_qso queues for upload");
        assert_eq!(pending[0].rec.call, "N0CALL");

        // The upload note bumps the tick for the UI toast.
        let t0 = e.snapshot().upload_tick;
        e.note_upload("Uploaded N0CALL to QRZ", true);
        let snap = e.snapshot();
        assert_eq!(snap.upload_tick, t0 + 1, "note bumps the tick");
        assert_eq!(snap.upload_note.as_deref(), Some("Uploaded N0CALL to QRZ"));
        assert!(snap.upload_ok);
    }

    /// THE NO-DOUBLE-SEND GUARANTEE for the interop broadcasts. Two emitters push
    /// contacts to the club network: the radio loop's Field-Day emitter (which
    /// reads the FD CONTEST log) and the shell's per-QSO forwarder (which drains
    /// `pending_uploads`, fed only by `log_qso`). One packet per contact holds
    /// only while those two sources stay disjoint — so a Field Day contact must
    /// never appear in the upload queue, even with an FD session live. If a
    /// future change routes FD contacts through `log_qso`, this fails, and the
    /// N1MM/N3FJP standing forwarders need a Field-Day exclusion before it lands.
    #[test]
    fn a_field_day_contact_never_enters_the_general_upload_queue() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        {
            let mut s = e.settings().clone();
            s.fd_active = true;
            s.fd_class = "3A".into();
            s.fd_section = "WI".into();
            e.apply_settings(s);
        }
        e.set_mode("fieldday-run").unwrap();
        assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
        assert!(e.fd_log_manual("W1AW", "1D", "CT", "PH").unwrap());
        assert_eq!(
            e.snapshot().field_day.map(|f| f.qso_count),
            Some(2),
            "both contacts are in the FD contest log"
        );
        assert!(
            e.take_pending_uploads().is_empty(),
            "FD contacts belong to the FD emitter alone — queueing them here would \
             broadcast every Field Day QSO twice"
        );

        // The converse: an ordinary contact logged DURING an FD session is a
        // genuinely different QSO (it is not in the contest log), so it still
        // reaches the general forwarder exactly once.
        e.log_qso(qrec("N0CALL", "20m"));
        let pending = e.take_pending_uploads();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].rec.call, "N0CALL");
    }

    // ---- N1MM+ standing broadcast (every logged QSO, event or not) ------------

    fn plain_qso() -> QsoRecord {
        QsoRecord {
            grid: Some("FN31pr".into()),
            freq_mhz: 14.074,
            mode: "FT8".into(),
            rst_sent: Some("-12".into()),
            when_unix: 1_782_583_500, // 2026-06-27 18:05:00 UTC
            ..qrec("W1AW", "20m")
        }
    }

    /// The standing toggle is the switch, the address is the destination, and BOTH
    /// are required — a toggle with nowhere to send is off, and an address alone
    /// must not opt an upgrading operator in.
    #[test]
    fn the_standing_n1mm_broadcast_needs_the_toggle_and_an_address() {
        let mut s = Settings {
            n1mm_addr: "127.0.0.1:12061".into(),
            ..Default::default()
        };
        assert_eq!(
            n1mm_broadcast_target(&s),
            None,
            "an address alone must never start broadcasting — that is the upgrade path"
        );
        s.n1mm_upload = true;
        assert_eq!(
            n1mm_broadcast_target(&s).as_deref(),
            Some("127.0.0.1:12061")
        );
        // Trimmed, so a pasted address with a stray space still resolves.
        s.n1mm_addr = "  127.0.0.1  ".into();
        assert_eq!(n1mm_broadcast_target(&s).as_deref(), Some("127.0.0.1"));
        // On, but nowhere to send.
        s.n1mm_addr = "   ".into();
        assert_eq!(n1mm_broadcast_target(&s), None);
    }

    /// What a map consumer (OpenHamClock / GridTracker) actually needs off an
    /// ORDINARY QSO: who, where, which band/frequency, which mode, when.
    #[test]
    fn an_ordinary_qso_broadcasts_the_fields_a_map_plots_from() {
        let c = n1mm_contact_for(&plain_qso(), "KD9TAW");
        let xml = tempo_net::n1mm::build_contactinfo(&c);
        for needle in [
            "<call>W1AW</call>",
            "<mycall>KD9TAW</mycall>",
            "<operator>KD9TAW</operator>",
            "<gridsquare>FN31pr</gridsquare>",
            "<band>20</band>", // meters, never MHz
            "<mode>FT8</mode>",
            "<timestamp>2026-06-27 18:05:00</timestamp>",
            "<rxfreq>1407400</rxfreq>", // 10 Hz units
            "<txfreq>1407400</txfreq>",
        ] {
            assert!(xml.contains(needle), "missing {needle} in {xml}");
        }
        // Not a contest contact: N1MM's general log is "DX", there is no section,
        // and claiming contest points for a ragchew would be fabricating a score.
        assert!(xml.contains("<contestname>DX</contestname>"), "{xml}");
        assert!(xml.contains("<section></section>"), "{xml}");
        assert!(xml.contains("<points>0</points>"), "{xml}");
        // A plain QSO's "exchange" is the signal report — what N1MM's DX log sends.
        assert!(xml.contains("<SentExchange>-12</SentExchange>"), "{xml}");
    }

    /// No grid in the record → no element (the consumer falls back to its own
    /// callsign→country lookup instead of plotting a contact at Null Island).
    #[test]
    fn a_gridless_qso_broadcasts_no_grid() {
        let rec = QsoRecord {
            grid: None,
            ..plain_qso()
        };
        let xml = tempo_net::n1mm::build_contactinfo(&n1mm_contact_for(&rec, "KD9TAW"));
        assert!(!xml.contains("gridsquare"), "{xml}");
    }

    /// THE REPORTED BUG, as the acceptance case. A tester configured the N1MM
    /// output on 127.0.0.1:12061 alongside HRD on 12060, logged QSOs outside any
    /// Field Day event, and Wireshark showed HRD sending and N1MM sending nothing.
    ///
    /// This drives the real log path — `log_qso` on a configured engine — and
    /// watches the datagram arrive on the port that was configured. Nothing is
    /// stubbed: the send is the same `push_to_n1mm` an operator's QSO fires.
    #[test]
    fn logging_an_ordinary_qso_broadcasts_to_the_configured_port() {
        let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        listener
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let configured = listener.local_addr().unwrap().to_string();

        let mut e = Engine::new("KD9TAW", "EN52", 0);
        {
            let mut s = e.settings().clone();
            s.n1mm_addr = configured;
            s.n1mm_upload = true;
            e.apply_settings(s);
        }
        // No Field Day event anywhere in sight — the tester's exact situation.
        assert!(e.snapshot().field_day.is_none());
        e.log_qso(plain_qso());

        let mut buf = [0u8; 4096];
        let (n, _) = listener
            .recv_from(&mut buf)
            .expect("logging a QSO broadcasts a contactinfo datagram");
        let xml = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(xml.starts_with("<?xml"), "{xml}");
        assert!(xml.contains("<call>W1AW</call>"), "{xml}");
        assert!(xml.contains("<gridsquare>FN31pr</gridsquare>"), "{xml}");
        assert!(xml.contains("<mycall>KD9TAW</mycall>"), "{xml}");

        // One QSO, one packet.
        listener
            .set_read_timeout(Some(std::time::Duration::from_millis(300)))
            .unwrap();
        assert!(
            listener.recv_from(&mut buf).is_err(),
            "a single QSO must not broadcast twice"
        );
    }

    /// The switch OFF is silence — not "sends to a default", not "sends anyway".
    #[test]
    fn the_broadcast_is_silent_while_the_toggle_is_off() {
        let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        listener
            .set_read_timeout(Some(std::time::Duration::from_millis(400)))
            .unwrap();
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        {
            let mut s = e.settings().clone();
            // Address configured, toggle never touched — the pre-change state, and
            // what every upgrading operator's settings.json looks like.
            s.n1mm_addr = listener.local_addr().unwrap().to_string();
            e.apply_settings(s);
        }
        e.log_qso(plain_qso());
        let mut buf = [0u8; 4096];
        assert!(
            listener.recv_from(&mut buf).is_err(),
            "an address alone must broadcast nothing outside a Field Day event"
        );
    }

    /// The reason this fires from the log funnel and NOT from the shell's upload
    /// worker: that worker DRAINS `pending_uploads`, and a record drained with every
    /// HTTP connector off is consumed and dropped. Broadcasting must therefore not
    /// make a record look "handled" — an operator running N1MM as their only
    /// integration still has this session's QSOs queued when they later enable QRZ.
    #[test]
    fn broadcasting_does_not_consume_the_pending_upload() {
        let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        {
            let mut s = e.settings().clone();
            s.n1mm_addr = listener.local_addr().unwrap().to_string();
            s.n1mm_upload = true;
            e.apply_settings(s);
        }
        e.log_qso(plain_qso());
        let pending = e.take_pending_uploads();
        assert_eq!(pending.len(), 1, "the QSO is still owed to the connectors");
        assert_eq!(pending[0].legs, upload_legs::ALL, "and owes every leg");
    }

    #[test]
    fn requeue_only_re_enqueues_owed_legs_and_caps_retries() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        let rec = qrec("W9XYZ", "20m");
        // A transient failure re-queues ONLY the failed leg — never a leg that
        // already succeeded, so the retry can't double-push a non-deduping service.
        e.requeue_upload(rec.clone(), upload_legs::EQSL, 1);
        let p = e.take_pending_uploads();
        assert_eq!(p.len(), 1);
        assert_eq!(
            p[0].legs,
            upload_legs::EQSL,
            "only the failed leg is owed on retry"
        );
        assert_eq!(p[0].attempts, 1);
        // Nothing owed, or retries exhausted → dropped (no infinite retry loop).
        e.requeue_upload(rec.clone(), 0, 1);
        e.requeue_upload(rec.clone(), upload_legs::QRZ, MAX_UPLOAD_RETRIES);
        assert!(
            e.take_pending_uploads().is_empty(),
            "nothing-owed / retry-exhausted uploads are dropped"
        );
    }

    /// A concise `QsoRecord` builder for the logbook tests (the struct has no
    /// `Default`; only call+band vary here).
    fn qrec(call: &str, band: &str) -> QsoRecord {
        QsoRecord {
            call: call.into(),
            grid: None,
            country: None,
            state: None,
            band: band.into(),
            freq_mhz: 14.074,
            mode: "FT8".into(),
            rst_sent: None,
            rst_rcvd: None,
            name: None,
            qth: None,
            comment: None,
            notes: None,
            tx_power: None,
            when_unix: 0,
            time_off_unix: None,
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: Default::default(),
            qsl_sent: Default::default(),
            credit_granted: vec![],
            credit_submitted: vec![],
            upload: Default::default(),
            ota: Default::default(),
        }
    }

    #[test]
    fn imported_hunted_parks_count_as_worked() {
        // A CW hunt never carries the park ref in the exchange, so the log can't know it —
        // the imported Hunted Parks.CSV set fills that gap for the NEW PARK badge.
        let mut e = Engine::new("K2DEF", "FN31", 0);
        assert!(!e.park_worked("US-1234"), "unknown park starts un-worked");
        e.set_hunted_parks_import(vec!["us-1234".into(), "US-5678".into(), "  ".into()]);
        assert_eq!(e.hunted_parks_import_count(), 2, "blank refs are dropped");
        assert!(e.park_worked("US-1234"), "case-insensitive match");
        assert!(e.park_worked(" us-5678 "), "trims + uppercases the query");
        assert!(!e.park_worked("US-9999"), "an un-imported park stays new");
        // A re-import replaces the set wholesale (the CSV is the full current picture).
        e.set_hunted_parks_import(vec!["US-9999".into()]);
        assert!(e.park_worked("US-9999") && !e.park_worked("US-1234"));
    }

    #[test]
    fn log_qso_dedups_a_repeated_identical_contact() {
        // The phantom-triplicate safety net: seeding the same contact repeatedly
        // (a double-click, or a companion auto-replying to one RR73/73 decode
        // across cycles) must append it ONCE, not once per seed.
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.log_qso(qrec("T22TT", "30m"));
        e.log_qso(qrec("T22TT", "30m"));
        e.log_qso(qrec("T22TT", "30m"));
        assert_eq!(e.get_log().len(), 1, "3 identical seeds → 1 logged QSO");

        // A different band, a different call, and the same call well outside the
        // dedup window are all legitimately distinct and MUST still log.
        e.log_qso(qrec("T22TT", "20m")); // different band
        e.log_qso(qrec("W1AW", "30m")); // different call
        let mut later = qrec("T22TT", "30m");
        later.when_unix = 3600; // an hour on — a genuine re-work, not a dupe
        e.log_qso(later);
        assert_eq!(
            e.get_log().len(),
            4,
            "distinct band / call / time still log"
        );
    }

    #[test]
    fn a_lone_rr73_seed_never_auto_logs_a_phantom_qso() {
        // The root-cause guard for the phantom bug: double-clicking (or a companion
        // auto-replying to) a decoded "<us> <dx> RR73" runs Station::start straight
        // into Done with NO transmission and no report exchanged. That must not
        // auto-log a contact we never actually made.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tier(Tier::Ft8);
        assert!(e.settings().auto_log, "auto_log on by default");
        // The DX's RR73 addressed to us appears in the decodes...
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ T22TT RR73", -10)], 1);
        // ...and we "work" it (double-click / companion Reply) — seeds Done, tx_count 0.
        e.call_station_ctx(
            "T22TT",
            None,
            Some("W9XYZ T22TT RR73"),
            Some(-10),
            Some(400.0),
        );
        assert!(e.snapshot().qso.is_some(), "the seed started a QSO station");
        // The next ingest runs the auto-log check.
        e.ingest_decodes_for_test(&[], 3);
        assert!(
            e.get_log().is_empty(),
            "a Done synthesized from a lone RR73 (no TX, no report) must not auto-log"
        );
    }

    /// PARITY GUARD for the `fd_rules` refactor: `fd_score` + the snapshot
    /// scoring block now compute via `fd_rules::ruleset(..).scoring`/bonuses
    /// instead of the old inline `legal_fd_power` + `fd_bonus_points`. The
    /// numbers must be byte-identical to what the inline math produced, so this
    /// pins an ARRL-FD fixture to values hand-computed from the OLD formula
    /// (`qso_pts × legal_fd_power(mult) + Σ bonus points`).
    #[test]
    fn fd_score_is_byte_identical_after_the_ruleset_refactor() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        {
            let mut s = e.settings().clone();
            s.fd_active = true;
            s.fd_class = "3A".into();
            s.fd_section = "WI".into();
            s.fd_power_mult = 5; // legal tier 5
            s.fd_bonuses = vec!["w1aw-bulletin".into(), "web-submission".into()]; // 100 + 50
            e.apply_settings(s);
        }
        e.set_mode("fieldday-run").unwrap();
        // 6 QSO points: CW 2 + CW 2 + PH 1 + PH 1.
        assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
        assert!(e.fd_log_manual("N0XYZ", "4A", "MN", "CW").unwrap());
        assert!(e.fd_log_manual("W1AW", "1D", "CT", "PH").unwrap());
        assert!(e.fd_log_manual("K5ABC", "3A", "STX", "PH").unwrap());

        // OLD formula: qso_pts=6, powered=6×5=30, bonus=100+50=150.
        assert_eq!(
            e.fd_score(),
            Some((6, 30, 150)),
            "fd_score matches the pre-refactor inline math exactly"
        );
        // The snapshot scoring block must agree (same ruleset path).
        let fd = e.snapshot().field_day.expect("master on → FD chrome");
        assert_eq!(fd.points, 6);
        assert_eq!(fd.powered_points, 30);
        assert_eq!(fd.bonus_points, 150);
        assert_eq!(fd.total_score, 180);
    }

    /// Master switch OFF (spec §1.3): even when the engine is still in
    /// `Mode::FieldDay`, a false `fd_active` must hide all FD chrome — the
    /// snapshot exposes no `field_day` so no cockpit shows the FD form.
    #[test]
    fn master_off_hides_field_day_chrome_in_the_snapshot() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        {
            let mut s = e.settings().clone();
            // fd_active stays false (the default) — the operator never flipped it.
            s.fd_class = "3A".into();
            s.fd_section = "WI".into();
            e.apply_settings(s);
        }
        e.set_mode("fieldday-run").unwrap();
        assert!(
            e.snapshot().field_day.is_none(),
            "master off → no FD chrome even in Mode::FieldDay"
        );
    }

    /// Master switch drives the operating mode (spec §1.2): a settings save with
    /// `fd_active` true and class/section set enters `Mode::FieldDay` (passive
    /// S&P), so every cockpit goes FD-aware without a separate `set_mode` — this
    /// is what the frontend toggle triggers through `apply_settings`.
    #[test]
    fn apply_settings_master_on_enters_field_day() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // Baseline: default off → no FD.
        assert!(e.snapshot().field_day.is_none(), "master off by default");
        {
            let mut s = e.settings().clone();
            s.fd_active = true;
            s.fd_class = "3A".into();
            s.fd_section = "WI".into();
            e.apply_settings(s);
        }
        let fd = e
            .snapshot()
            .field_day
            .expect("master on + class/section → engine enters Field Day S&P");
        assert!(!fd.running, "the master enters passive S&P, not a run");
        assert_eq!(fd.my_class, "3A");
        assert_eq!(fd.my_section, "WI");
    }

    /// Master ON but the exchange is incomplete: `apply_settings` must NOT enter
    /// FD on a blank class/section (the exchange goes on the air) — it leaves the
    /// engine non-FD so the setup screen (spec §1.2 #1) can prompt for them.
    #[test]
    fn apply_settings_master_on_without_exchange_stays_out_of_field_day() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        {
            let mut s = e.settings().clone();
            s.fd_active = true; // master flipped on, but class/section still blank
            e.apply_settings(s);
        }
        assert!(
            e.snapshot().field_day.is_none(),
            "no FD entry without a class/section to transmit"
        );
    }

    /// Master switch OFF forces FD exit (spec §1.3): once the engine is in Field
    /// Day, a save with `fd_active` false must leave the engine NOT in
    /// `Mode::FieldDay` (not merely hide the chrome) — otherwise the operator is
    /// stranded in FD with the nav hidden. `field_day_log_adif` reads the raw
    /// mode, so its `None` proves the engine truly left FD.
    #[test]
    fn apply_settings_master_off_exits_field_day() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        {
            let mut s = e.settings().clone();
            s.fd_active = true;
            s.fd_class = "3A".into();
            s.fd_section = "WI".into();
            e.apply_settings(s);
        }
        assert!(
            e.field_day_log_adif().is_some() || e.snapshot().field_day.is_some(),
            "precondition: the engine is in Field Day"
        );
        // Log a contact so the raw-mode accessor has something to report.
        assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
        assert!(e.field_day_log_adif().is_some(), "in FD with a contact");
        // The operator flips the master off.
        {
            let mut s = e.settings().clone();
            s.fd_active = false;
            e.apply_settings(s);
        }
        assert!(
            e.field_day_log_adif().is_none(),
            "master off → engine is no longer in Mode::FieldDay"
        );
        assert!(e.snapshot().field_day.is_none(), "and no FD chrome");
    }

    /// Restore-on-launch (spec §1.1): a relaunch with `fd_active` persisted true
    /// must re-enter Field Day so a crash/restart mid-contest comes back
    /// operating. `restore_field_day_if_enabled` is the shell's startup hook.
    #[test]
    fn restore_field_day_if_enabled_re_enters_on_relaunch() {
        // Simulate a relaunch: build the engine straight from persisted settings
        // (the shell's `Engine::with_settings` path) with the master left on. A
        // fresh engine boots in Chat — FD is NOT auto-entered by construction.
        let mut s = Engine::new("W9XYZ", "EN61", 0).settings().clone();
        s.fd_active = true;
        s.fd_class = "3A".into();
        s.fd_section = "WI".into();
        let mut e = Engine::with_settings(s);
        assert!(
            e.snapshot().field_day.is_none(),
            "freshly loaded engine is not yet in FD"
        );
        e.restore_field_day_if_enabled();
        assert!(
            e.snapshot().field_day.is_some(),
            "restore re-enters FD when the master was left on"
        );
        // Idempotent: a second restore never rebuilds a live FD log.
        assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
        e.restore_field_day_if_enabled();
        assert_eq!(
            e.snapshot().field_day.expect("still in FD").qso_count,
            1,
            "restore is a no-op once already in FD (never rebuilds the log)"
        );
    }

    /// M15: the Field Day contest log is memory-only, so the shell needs a way to
    /// serialize it for a flush-on-exit. `field_day_log_adif` yields the whole
    /// log — with the class/section exchange — as ADIF, and is `None` when there
    /// is nothing to persist.
    #[test]
    fn field_day_log_flush_accessor_serializes_the_contest_log() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // Outside Field Day there is no contest log to flush.
        assert!(
            e.field_day_log_adif().is_none(),
            "no FD log outside FD mode"
        );

        {
            let mut s = e.settings().clone();
            s.fd_class = "3A".into();
            s.fd_section = "WI".into();
            e.apply_settings(s);
        }
        e.set_mode("fieldday-run").unwrap();
        // In FD mode but empty → still nothing to persist.
        assert!(
            e.field_day_log_adif().is_none(),
            "empty FD log flushes nothing"
        );

        assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
        assert!(e.fd_log_manual("W1AW", "1D", "CT", "PH").unwrap());

        let adif = e
            .field_day_log_adif()
            .expect("FD log serializes for the exit flush");
        assert!(
            adif.contains("K1ABC") && adif.contains("W1AW"),
            "both worked stations are in the flush"
        );
        assert!(
            adif.contains("ARRL_SECT") && adif.contains("CLASS"),
            "the flush carries the FD exchange (class/section), unlike the plain QSO log"
        );
    }

    /// The FD contest log lived ONLY in `Mode::FieldDay`: every FD-mode entry
    /// built a fresh empty log, so a quit + relaunch mid-event dropped every
    /// contact — and the next exit flush OVERWROTE the backup with just the new
    /// session. With a journal path set, the log must survive quits/re-entry.
    #[test]
    fn field_day_log_survives_quit_and_relaunch() {
        let path =
            std::env::temp_dir().join(format!("nexus_fd_journal_{}.adi", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let fd_engine = || {
            let mut e = Engine::new("W9XYZ", "EN61", 0);
            {
                let mut s = e.settings().clone();
                s.fd_active = true; // master switch on — FD chrome visible in the snapshot
                s.fd_class = "3A".into();
                s.fd_section = "WI".into();
                e.apply_settings(s);
            }
            e.set_fd_log_path(path.clone());
            e.set_mode("fieldday-run").unwrap();
            e
        };

        // Session A logs two contacts and quits.
        let mut a = fd_engine();
        assert!(a.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
        assert!(a.fd_log_manual("W1AW", "1D", "CT", "PH").unwrap());
        drop(a);

        // Session B (a relaunch) restores both, still dupe-checks them, adds a third.
        let mut b = fd_engine();
        let snap = b.snapshot().field_day.expect("in Field Day mode");
        assert_eq!(snap.qso_count, 2, "a restart restores the contest log");
        assert!(
            !b.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap(),
            "restored contacts still dupe-check"
        );
        assert!(b.fd_log_manual("N0XYZ", "4A", "MN", "CW").unwrap());
        drop(b);

        // Session C: B's quit must not have overwritten the journal with only
        // B's session — all three contacts survive the second relaunch.
        let c = fd_engine();
        assert_eq!(
            c.snapshot().field_day.expect("in Field Day mode").qso_count,
            3,
            "the second quit keeps A's AND B's contacts"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// M18: a full-log ADIF rewrite from a stale in-memory copy must not silently
    /// drop QSOs that another Nexus instance (sharing the same file) appended.
    #[test]
    fn full_log_rewrite_preserves_another_instances_appends() {
        let path =
            std::env::temp_dir().join(format!("nexus_concurrent_mark_{}.adi", std::process::id()));
        let _ = std::fs::remove_file(&path);

        // Instance B loads the log and records two base contacts (append-only).
        let mut b = Engine::new("K2DEF", "FN31", 0);
        b.set_log_path(path.clone());
        b.log_qso(qrec("W1AAA", "20m"));
        b.log_qso(qrec("W2BBB", "20m"));
        assert_eq!(b.get_log().len(), 2);

        // Instance A (a second process on the same file) appends two more QSOs
        // that B never sees in memory.
        Logbook::append(&path, &qrec("W3CCC", "40m")).unwrap();
        Logbook::append(&path, &qrec("W4DDD", "15m")).unwrap();
        assert_eq!(Logbook::load(&path).len(), 4, "the file holds A's appends");

        // B does a full-log-rewrite action (mark QSL-sent) on its stale 2-record copy.
        assert!(b.mark_qsl_sent(0, tempo_core::logbook::QslVia::Direct));

        let on_disk = Logbook::load(&path);
        assert_eq!(
            on_disk.len(),
            4,
            "the rewrite from a stale copy must not drop another instance's appends"
        );
        let calls: Vec<&str> = on_disk.records().iter().map(|r| r.call.as_str()).collect();
        assert!(
            calls.contains(&"W3CCC") && calls.contains(&"W4DDD"),
            "A's appended QSOs survive B's full rewrite"
        );
        // B's own edit still landed.
        assert!(
            on_disk
                .records()
                .iter()
                .any(|r| r.call == "W1AAA" && r.qsl_sent.sent),
            "B's QSL-sent mark persisted"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// M18: a delete must still remove the targeted record (and not resurrect it)
    /// while preserving another instance's appends — the recover-before-mutation
    /// ordering that makes the reconcile safe for removals.
    #[test]
    fn delete_still_removes_target_and_keeps_other_instances_appends() {
        let path = std::env::temp_dir().join(format!(
            "nexus_concurrent_delete_{}.adi",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut b = Engine::new("K2DEF", "FN31", 0);
        b.set_log_path(path.clone());
        b.log_qso(qrec("W1AAA", "20m"));
        b.log_qso(qrec("W2BBB", "20m"));

        // Another instance appends a QSO B does not know about.
        Logbook::append(&path, &qrec("W3CCC", "40m")).unwrap();

        // B deletes its index 0 (W1AAA) on the stale copy.
        assert!(b.delete_qso(0));

        let on_disk = Logbook::load(&path);
        let calls: Vec<&str> = on_disk.records().iter().map(|r| r.call.as_str()).collect();
        assert!(
            !calls.contains(&"W1AAA"),
            "the deleted record is gone (not resurrected by the reconcile)"
        );
        assert!(
            calls.contains(&"W2BBB") && calls.contains(&"W3CCC"),
            "the survivor and the other instance's append both remain"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn prompt_to_log_holds_then_confirms() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.settings.prompt_to_log = true;
        // Seed a DX grid so the held record carries it (operator-typed call+grid).
        e.call_station_with_grid("W9XYZ", Some("en37"));
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);

        // Held, not logged.
        assert!(e.get_log().is_empty(), "prompt-to-log withholds the write");
        let snap = e.snapshot();
        let pending = snap.pending_log.expect("a QSO awaits confirm");
        assert_eq!(pending.call, "W9XYZ");
        assert_eq!(
            pending.grid.as_deref(),
            Some("EN37"),
            "DX grid captured + normalized"
        );

        // Confirm logs it and clears the hold.
        e.confirm_pending_log(pending.into());
        assert_eq!(e.get_log().len(), 1, "confirm writes exactly one record");
        assert!(
            e.snapshot().pending_log.is_none(),
            "hold cleared after confirm"
        );
    }

    #[test]
    fn a_held_qso_survives_a_crash_and_comes_back_on_next_launch() {
        // The failure this closes: a finished contact — exchange complete, the OTHER station
        // already logged it — sat only in memory while the confirm popup waited. A crash or
        // power cut in that window destroyed a real QSO with no trace anywhere.
        let dir = std::env::temp_dir().join(format!("nexus-pendingqso-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("pending_qso.json");

        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.settings.prompt_to_log = true;
        e.set_pending_qso_path(path.clone());
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        assert!(e.snapshot().pending_log.is_some(), "held for confirm");
        assert!(
            path.exists(),
            "journalled the MOMENT it was held — an exit hook would miss a crash"
        );

        // Simulate the crash: this engine simply vanishes, nothing is flushed.
        drop(e);

        // Next launch restores the hold, so the operator can still log the contact.
        let mut relaunched = Engine::new("K2DEF", "FN31", 0);
        relaunched.set_pending_qso_path(path.clone());
        let text = std::fs::read_to_string(&path).expect("journal readable after the crash");
        let q: crate::dto::LoggedQso = serde_json::from_str(&text).expect("journal parses");
        relaunched.load_pending_qso(q.into());
        let pending = relaunched
            .snapshot()
            .pending_log
            .expect("the contact came back");
        assert_eq!(pending.call, "W9XYZ", "the SAME station, not a blank hold");

        // Confirming logs it and clears the journal, so it cannot resurrect next launch.
        relaunched.confirm_pending_log(pending.into());
        assert_eq!(relaunched.get_log().len(), 1, "logged exactly once");
        assert!(
            !path.exists(),
            "journal removed on confirm — a logged QSO must not come back"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discarding_a_held_qso_clears_its_journal() {
        let dir = std::env::temp_dir().join(format!("nexus-pendingqso-d-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("pending_qso.json");

        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.settings.prompt_to_log = true;
        e.set_pending_qso_path(path.clone());
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        assert!(path.exists(), "journalled while held");
        e.discard_pending_log();
        assert!(
            !path.exists(),
            "a discarded QSO must not be restored on the next launch"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prompt_to_log_discard_drops_the_contact() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.settings.prompt_to_log = true;
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        assert!(e.snapshot().pending_log.is_some());
        e.discard_pending_log();
        assert!(e.get_log().is_empty(), "discard logs nothing");
        assert!(e.snapshot().pending_log.is_none());
    }

    #[test]
    fn new_grid_and_new_dxcc_highlight_in_decode_feed() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        // Stub resolver: first letter of the call is its "entity".
        e.set_dxcc_resolver(|call| call.chars().next().map(|c| c.to_string()));

        // Log a contact with W9XYZ in grid EN37 → entity "W" and grid EN37 worked.
        e.log_qso(QsoRecord {
            call: "W9XYZ".into(),
            grid: Some("EN37".into()),
            country: None,
            state: None,
            band: "20m".into(),
            freq_mhz: 14.074,
            mode: "FT8".into(),
            rst_sent: None,
            rst_rcvd: None,
            name: None,
            qth: None,
            comment: None,
            notes: None,
            tx_power: None,
            when_unix: 0,
            time_off_unix: None,
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: Default::default(),
            qsl_sent: Default::default(),
            credit_granted: vec![],
            credit_submitted: vec![],
            upload: Default::default(),
            ota: Default::default(),
        });

        // A CQ from a same-entity station in a NEW grid → new_grid, not new_dxcc.
        // A CQ from a different-entity station → new_dxcc.
        e.ingest_decodes_for_test(
            &[
                dec_snr("CQ W1AW EN37", -5),   // entity "W" worked; grid EN37 worked
                dec_snr("CQ W4ABC EM73", -8),  // entity "W" worked; grid EM73 NEW
                dec_snr("CQ DL1XYZ JO31", -9), // entity "D" NEW; grid JO31 NEW
            ],
            0,
        );
        let rows = e.snapshot().recent_decodes;
        let row = |c: &str| rows.iter().find(|r| r.from.as_deref() == Some(c)).unwrap();

        assert!(
            !row("W1AW").new_grid && !row("W1AW").new_dxcc && !row("W1AW").new_band,
            "all worked"
        );
        assert!(
            row("W4ABC").new_grid && !row("W4ABC").new_dxcc && !row("W4ABC").new_band,
            "new grid only (entity W already worked on this band)"
        );
        assert!(
            row("DL1XYZ").new_grid && row("DL1XYZ").new_dxcc && !row("DL1XYZ").new_band,
            "new grid + all-time-new entity (ATNO, so DXCC not BAND)"
        );
    }

    /// The operator's ruling (2026-07-22) on whether a 20 m contact counts for a 2 m
    /// chain: "Not on 2m it's a different band." Grids and DXCC are both awarded per
    /// band, and a 2 m grid is worth far more than the same square on HF — so the
    /// decode feed must light up again when the same grid/entity is heard on 2 m.
    /// The entity is not an ATNO though (it was worked on 20 m), so this is a `new_band`
    /// band-slot (the dimmer BAND tag), NOT `new_dxcc` (the all-time-new-one DXCC tag).
    #[test]
    fn worked_on_20m_is_new_again_on_2m_in_the_decode_feed() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_dxcc_resolver(|call| call.chars().next().map(|c| c.to_string()));
        // W9XYZ / EN37 worked on 20 m ONLY.
        e.log_qso(QsoRecord {
            grid: Some("EN37".into()),
            ..qrec("W9XYZ", "20m")
        });

        // Still on 20 m: nothing is new — the fix must not simply alert on everything.
        e.set_frequency(14.074, "20m", "USB");
        e.ingest_decodes_for_test(&[dec_snr("CQ W1AW EN37", -5)], 0);
        let on_20 = e.snapshot().recent_decodes;
        let r20 = on_20
            .iter()
            .find(|r| r.from.as_deref() == Some("W1AW"))
            .unwrap();
        assert!(
            !r20.new_grid && !r20.new_dxcc && !r20.new_band,
            "EN37 / entity W ARE worked on 20m → no badge there"
        );

        // Move to 2 m and hear the very same grid + entity → the grid is genuinely new,
        // and the entity is a new BAND-slot (worked on 20 m, never on 2 m) — not an ATNO.
        e.set_frequency(144.174, "2m", "USB");
        e.ingest_decodes_for_test(&[dec_snr("CQ W1AW EN37", -5)], 0);
        let on_2 = e.snapshot().recent_decodes;
        let r2 = on_2
            .iter()
            .find(|r| r.from.as_deref() == Some("W1AW"))
            .unwrap();
        assert!(
            r2.new_grid,
            "EN37 worked on 20m only → a NEW grid on 2m (VUCC is per band)"
        );
        assert!(
            r2.new_band && !r2.new_dxcc,
            "entity worked on 20m only → a new BAND-slot on 2m, not an all-time new one"
        );
    }

    /// Per-band keying is only correct if both sides spell a band the same way, and
    /// they don't: Nexus writes "20m" but a LoTW/QRZ export writes "20M", and
    /// `parse_adif` passes BAND through verbatim. An imported log must still suppress
    /// the badges it satisfies, or every HF decode would read as a new one.
    #[test]
    fn an_imported_logs_band_spelling_still_matches_the_live_band() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_dxcc_resolver(|call| call.chars().next().map(|c| c.to_string()));
        e.log_qso(QsoRecord {
            grid: Some("EN37".into()),
            ..qrec("W9XYZ", "20M") // as a LoTW export spells it
        });
        e.set_frequency(14.074, "20m", "USB");
        e.ingest_decodes_for_test(&[dec_snr("CQ W1AW EN37", -5)], 0);
        let rows = e.snapshot().recent_decodes;
        let r = rows
            .iter()
            .find(|x| x.from.as_deref() == Some("W1AW"))
            .unwrap();
        assert!(
            !r.new_grid && !r.new_dxcc,
            "\"20M\" in the log IS the \"20m\" we're tuned to"
        );
    }

    #[test]
    fn import_backfills_missing_country_via_resolver() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_dxcc_resolver(|call| {
            if call.starts_with("DL") {
                Some("Germany".to_string())
            } else if call.starts_with("F") {
                Some("France".to_string())
            } else {
                None
            }
        });
        // Import an ADIF that has NO COUNTRY fields (a typical old logbook export).
        let adif = "<EOH>\n\
            <CALL:6>DL1XYZ<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20240101<TIME_ON:6>120000<EOR>\n\
            <CALL:5>F5RXL<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20240101<TIME_ON:6>120100<EOR>\n";
        let (added, _skipped, _total) = e.import_adif(adif);
        assert_eq!(added, 2);
        let log = e.get_log();
        let country = |call: &str| {
            log.iter()
                .find(|r| r.call == call)
                .and_then(|r| r.country.clone())
        };
        assert_eq!(
            country("DL1XYZ").as_deref(),
            Some("Germany"),
            "country backfilled on import"
        );
        assert_eq!(country("F5RXL").as_deref(), Some("France"));
    }

    #[test]
    fn ft8_tier_logs_as_ft8_mode() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_tier(Tier::Ft8);
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        let log = e.get_log();
        assert_eq!(log.len(), 1);
        assert_eq!(
            log[0].mode, "FT8",
            "FT8 contacts log as FT8 (award eligibility)"
        );
    }

    #[test]
    fn own_transmissions_surface_as_mine_rows() {
        // Working a station, our transmitted message shows up as a `mine` decode
        // row so the operator sees each of their calls.
        let mut e = Engine::new("K2DEF", "FN31", 0); // parity 0 → TX on even slots
        e.call_station("W9XYZ"); // answering → pending Grid to W9XYZ
        let _ = e.poll_tx(0); // transmits on the even slot
        let snap = e.snapshot();
        let mine: Vec<_> = snap.recent_decodes.iter().filter(|d| d.mine).collect();
        assert_eq!(mine.len(), 1, "one own-TX row after one transmission");
        assert_eq!(mine[0].from.as_deref(), Some("K2DEF"));
        assert!(mine[0].message.contains("W9XYZ"), "shows what we sent");
        // A second over adds a second own-TX row (the repeated-call chronology).
        let _ = e.poll_tx(2);
        assert_eq!(
            e.snapshot()
                .recent_decodes
                .iter()
                .filter(|d| d.mine)
                .count(),
            2,
            "each call is its own row"
        );
        // Stop TX clears the own-TX history.
        e.halt_tx();
        assert_eq!(
            e.snapshot()
                .recent_decodes
                .iter()
                .filter(|d| d.mine)
                .count(),
            0,
            "halt_tx clears own-TX rows"
        );
    }

    #[test]
    fn area_round_trip_remembers_each_areas_tier() {
        // THE tier-lost bug: dx(FT4) -> msg -> dx used to default back to FT8.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_area("dx");
        e.set_tier(Tier::Ft4);
        e.set_area("msg");
        assert_eq!(e.tier(), Tier::TempoFast);
        e.set_area("dx");
        assert_eq!(
            e.tier(),
            Tier::Ft4,
            "FT4 survives the round-trip through msg"
        );
        // And the msg side remembers DX1 the same way.
        e.set_area("msg");
        e.set_tier(Tier::TempoDeep);
        e.set_area("dx");
        e.set_area("msg");
        assert_eq!(
            e.tier(),
            Tier::TempoDeep,
            "DX1 survives the round-trip through dx"
        );
    }

    #[test]
    fn set_area_binds_tier_and_mode_per_area() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        // MSG → FT1 + Chat.
        e.set_area("msg");
        assert_eq!(e.tier(), Tier::TempoFast);
        assert_eq!(e.snapshot().mode, OpMode::Chat);
        // DX → FT8 + out of Chat.
        e.set_area("dx");
        assert_eq!(e.tier(), Tier::Ft8);
        assert_ne!(e.snapshot().mode, OpMode::Chat);
        // Idempotent / non-disruptive: keep FT4 if already there on re-entering DX.
        e.set_tier(Tier::Ft4);
        e.set_area("dx");
        assert_eq!(e.tier(), Tier::Ft4, "DX keeps an already-structured tier");
        // MSG keeps DX1 if already there.
        e.set_tier(Tier::TempoDeep);
        e.set_area("msg");
        assert_eq!(
            e.tier(),
            Tier::TempoDeep,
            "MSG keeps an already-chat-capable tier"
        );
    }

    #[test]
    fn entering_chat_snaps_off_ft8_to_ft1() {
        // Chat free-text can't ride FT8 (13-char packer) — entering Chat on FT8
        // must snap the tier to FT1 so it never silently transmits nothing.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        assert_eq!(e.tier(), Tier::Ft8, "default is FT8");
        e.set_mode("chat").unwrap();
        assert_eq!(e.tier(), Tier::TempoFast, "Chat snapped to FT1");
        // DX1 is also chat-capable — entering Chat from DX1 leaves it on DX1.
        e.set_tier(Tier::TempoDeep);
        e.set_mode("chat").unwrap();
        assert_eq!(e.tier(), Tier::TempoDeep, "Chat keeps a chat-capable tier");
    }

    #[test]
    fn cq_initiator_autologs_at_rr73_without_a_final_73() {
        // Calling CQ: a station answers, we report, they roger — we send RR73 and
        // they vanish (no final 73). The contact must still auto-log (the bug was
        // it waited for a 73 that never came).
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        assert!(e.settings().auto_log);
        e.set_mode("qso-run").unwrap(); // CallingCq
                                        // K2DEF answers our CQ with a grid → we send a report (AwaitRoger).
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K2DEF FN31", -8)], 1);
        // K2DEF rogers our report → we queue RR73 (Confirming). No final 73 ever comes.
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K2DEF R-12", -8)], 3);
        // The RR73 is only QUEUED here — the contact must NOT log until it's on the air.
        assert!(
            e.get_log().is_empty(),
            "not logged before the RR73 is transmitted"
        );

        // We send the RR73 on our next TX slot (tx_count → 1)...
        assert!(!e.poll_tx(4).is_empty(), "RR73 goes out on a TX slot");
        // ...and the contact auto-logs once that closing roger has actually gone out.
        e.ingest_decodes_for_test(&[], 5); // an RX slot re-runs the auto-log check
        let log = e.get_log();
        assert_eq!(log.len(), 1, "CQ-side QSO auto-logs after RR73 is sent");
        assert_eq!(log[0].call, "K2DEF");
        assert_eq!(
            log[0].rst_rcvd.as_deref(),
            Some("-12"),
            "report they sent us"
        );
        // Idempotent: a later 73 (or re-observe) doesn't double-log.
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K2DEF 73", -8)], 7);
        assert_eq!(e.get_log().len(), 1, "no double-log on a late 73");
    }

    #[test]
    fn cq_run_does_not_leak_a_qso_start_into_the_next_contact() {
        // The review's critical catch: in a CQ run, manually logging QSO #1 before it
        // completes must NOT leave its start time stamped — the next caller stamps its
        // own TIME_ON. Exercises both the post-log stamp guard and the resume-CQ reset.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_mode("qso-run").unwrap(); // CallingCq, TX armed
                                        // QSO1: K2DEF answers our CQ → we report (AwaitRoger); start stamped.
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K2DEF FN31", -8)], 1);
        assert!(e.qso_start_unix.is_some(), "QSO1 stamped its start");
        // Operator manually logs QSO1 early (a report was sent → allowed).
        assert!(
            e.log_current_qso(),
            "manual log succeeds (report exchanged)"
        );
        assert!(e.qso_start_unix.is_none(), "manual log clears the start");
        // QSO1 then rogers; we send RR73 (tx_count→1) and resume to CQ.
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K2DEF R-12", -8)], 3);
        assert!(
            e.qso_start_unix.is_none(),
            "a post-log over must NOT re-stamp a start"
        );
        assert!(!e.poll_tx(4).is_empty(), "RR73 goes out");
        e.ingest_decodes_for_test(&[], 5); // resume to CQ
        assert_eq!(
            e.snapshot().qso.unwrap().state,
            "CallingCq",
            "resumed calling CQ"
        );
        assert!(
            e.qso_start_unix.is_none(),
            "no stale QSO1 start leaks into the next contact"
        );
        // The next caller stamps a FRESH start.
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ N0XYZ EM48", -8)], 7);
        assert!(e.qso_start_unix.is_some(), "QSO2 stamps its own start");
    }

    #[test]
    fn manual_log_then_completion_does_not_double_log() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        // Operator clicks "Log" mid-sequence.
        assert!(e.log_current_qso(), "manual log writes the contact");
        assert_eq!(e.get_log().len(), 1);
        // A second click is a no-op (write-once).
        assert!(!e.log_current_qso(), "second manual log is a no-op");
        assert_eq!(e.get_log().len(), 1);
        // The QSO then completes naturally — must NOT auto-log a duplicate.
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        assert_eq!(e.get_log().len(), 1, "completion does not double-log");
    }

    #[test]
    fn manual_log_respects_prompt_to_log() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.settings.prompt_to_log = true;
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        assert!(e.log_current_qso());
        // Held for confirm, not written, like auto-log.
        assert!(
            e.get_log().is_empty(),
            "prompt-to-log holds the manual log too"
        );
        let pending = e.snapshot().pending_log.expect("a QSO awaits confirm");
        e.confirm_pending_log(pending.into());
        assert_eq!(e.get_log().len(), 1);
    }

    #[test]
    fn completed_qso_logs_country_from_resolver() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        // Stub resolver: map the DX call to a country.
        e.set_dxcc_resolver(|call| {
            if call == "DL1XYZ" {
                Some("Germany".to_string())
            } else {
                None
            }
        });
        e.call_station("DL1XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF DL1XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF DL1XYZ RR73", -7)], 3);
        let log = e.get_log();
        assert_eq!(log.len(), 1);
        assert_eq!(
            log[0].country.as_deref(),
            Some("Germany"),
            "country resolved + logged at QSO completion"
        );
    }

    /// Engine-level coordinated-QSY end-to-end: an initiator engine announces a
    /// hop (visible as a plain-text broadcast in its own band feed), a follower
    /// engine fed those frames schedules the same move, and BOTH retune to the
    /// new channel on the target slot.
    #[test]
    fn qsy_initiator_announces_and_follower_moves_together() {
        use tempo_core::{inbox, qsy, text};

        // KA9AAA (Tx 1st / even) is the lexicographic initiator; KB9BBB follows.
        let mut a = Engine::new("KA9AAA", "EN52", 0);
        let mut b = Engine::new("KB9BBB", "EN52", 1);
        for e in [&mut a, &mut b] {
            e.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
            e.qsy_configure(vec!["20m".into(), "40m".into()], 1); // hop every over
        }
        a.select_peer("KB9BBB");
        b.select_peer("KA9AAA");
        a.qsy_set_enabled(true);
        b.qsy_set_enabled(true);
        assert_eq!(a.snapshot().qsy.unwrap().role, "initiator");
        assert_eq!(b.snapshot().qsy.unwrap().role, "follower");
        assert_eq!(a.snapshot().radio.band, "20m", "home channel");

        // A must hear B before it considers the partner present.
        a.ingest_decodes_for_test(&[dec_snr("KA9AAA KB9BBB EN52", -5)], 0);

        // A's over (even TX slot) auto-announces a hop to 40 m.
        let _ = a.poll_tx(2);
        let astat = a.snapshot().qsy.unwrap();
        assert_eq!(
            astat.next_channel.as_deref(),
            Some("40M"),
            "A scheduled a move"
        );
        let at = astat.next_slot.expect("A has a target slot");

        // The announce is in the clear in A's own band feed.
        let feed = a.snapshot();
        let band = feed.conversations.iter().find(|c| c.peer == "*").unwrap();
        assert!(
            band.messages.iter().any(|m| m.text.starts_with("QSY 40M")),
            "directive shown as plain text: {:?}",
            band.messages
        );

        // Reconstruct the on-air directive broadcast and feed it to B.
        let body = format!("QSY 40M {}", qsy::encode_slot(at));
        let frames = text::chunk(&inbox::broadcast_text("KA9AAA", &body), 'A');
        for f in &frames {
            b.ingest_decodes_for_test(&[dec_snr(f, -7)], 3);
        }
        assert_eq!(
            b.snapshot().qsy.unwrap().next_channel.as_deref(),
            Some("40M"),
            "B scheduled the same move"
        );

        // On the target slot both retune to 40 m together.
        let _ = a.poll_tx(at);
        let _ = b.poll_tx(at);
        assert_eq!(a.snapshot().radio.band, "40m", "A moved");
        assert_eq!(b.snapshot().radio.band, "40m", "B moved");
        assert_eq!(a.snapshot().radio.dial_mhz, 7.0430);
        assert_eq!(b.snapshot().radio.dial_mhz, 7.0430);
    }

    /// Isolation: with the feature disabled (the default), a decoded QSY directive
    /// is never acted on — no schedule, no retune — and no `qsy` status appears.
    #[test]
    fn qsy_disabled_ignores_directives() {
        use tempo_core::{inbox, qsy, text};
        let mut e = Engine::new("KB9BBB", "EN52", 1);
        assert!(e.snapshot().qsy.is_none(), "no QSY status while disabled");
        let body = format!("QSY 40M {}", qsy::encode_slot(100));
        let frames = text::chunk(&inbox::broadcast_text("KA9AAA", &body), 'A');
        for f in &frames {
            e.ingest_decodes_for_test(&[dec_snr(f, -7)], 1);
        }
        let _ = e.poll_tx(100);
        assert_eq!(
            e.snapshot().radio.band,
            "20m",
            "disabled feature never retunes"
        );
        assert!(e.snapshot().qsy.is_none());
    }

    #[test]
    fn auto_log_off_does_not_log() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.settings.auto_log = false;
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
        e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
        assert!(e.get_log().is_empty(), "no auto-log when auto_log is off");
    }

    /// Build a clean (noise-free) full frame for `kind` carrying `msg` at `f0`,
    /// scaled so the engine's `channel::capture_to_i16` (×32767) lands at ~±1000 i16.
    /// FT8 starts at the 0.5 s TX point; FT4 self-positions.
    fn native_frame_for(kind: modes::ModeKind, msg: &str, f0: f32) -> Vec<f32> {
        let mode = modes::make_mode(kind);
        let tones = mode.encode(msg);
        assert!(!tones.is_empty(), "{} encode failed", kind.as_str());
        let wave = mode.gen_wave(&tones, tempo_fast::SAMPLE_RATE, f0);
        let n = mode.frame_samples();
        // FT8/FT4 gen_wave is now slot-positioned (includes the 0.5 s lead-in), so place
        // it at the slot start — no manual offset (a stale +6000 here would push FT8 to
        // 1.0 s, double-offsetting it).
        let off = 0;
        let mut frame = vec![0f32; n];
        for (i, &s) in wave.iter().enumerate() {
            if off + i < n {
                frame[off + i] = s * 0.0305; // ×0.0305 here, ×32767 in capture_to_i16 → ~±1000 i16
            }
        }
        frame
    }

    /// FT8 decodes through the LIVE engine ingest path once the operator selects
    /// the FT8 tier — proving the SignalSource routing + mode-driven frame sizing
    /// the spine integration added (the path was hardcoded to FT1 before).
    #[test]
    fn ft8_live_ingest_decodes() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        assert_eq!(
            e.active_frame_samples(),
            modes::ModeKind::Ft8.frame_samples()
        );
        assert_eq!(e.active_slot_secs(), 15.0);
        let frame = native_frame_for(modes::ModeKind::Ft8, "CQ KD9TAW EN52", 1500.0);
        let n = e.ingest(&frame, 2);
        assert!(n >= 1, "FT8 frame should decode at least one signal");
        assert!(
            e.last_decodes()
                .iter()
                .any(|d| d.message == "CQ KD9TAW EN52"),
            "FT8 live ingest must recover the message; got {:?}",
            e.last_decodes()
        );
    }

    /// ALL.TXT decode log: off by default (nothing buffered), and when enabled the
    /// live ingest path buffers one WSJT-X-format line per decode, drained by the shell.
    #[test]
    fn all_txt_log_buffers_one_line_per_decode_when_enabled() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        let frame = native_frame_for(modes::ModeKind::Ft8, "CQ W1ABC FN42", 1500.0);
        e.ingest(&frame, 2);
        assert!(
            e.take_all_txt_pending().is_empty(),
            "off by default → no ALL.TXT lines"
        );

        e.settings.write_all_txt = true;
        let frame2 = native_frame_for(modes::ModeKind::Ft8, "CQ W1ABC FN42", 1500.0);
        e.ingest(&frame2, 3);
        let lines = e.take_all_txt_pending();
        assert_eq!(lines.len(), 1, "one decode → one ALL.TXT line: {lines:?}");
        assert!(
            lines[0].contains("Rx FT8") && lines[0].contains("CQ W1ABC FN42"),
            "line carries Rx/mode + message: {}",
            lines[0]
        );
        assert!(e.take_all_txt_pending().is_empty(), "drained after take");
    }

    /// The WSJT-X-style early pass: a period truncated at ~11.8 s (the capture
    /// the radio loop hands it mid-slot, tail zero-padded) must still decode, and
    /// the boundary's full-window ingest must NOT double-ingest the same message.
    #[test]
    fn early_pass_decodes_truncated_period_and_boundary_dedupes() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        let full = native_frame_for(modes::ModeKind::Ft8, "CQ W1ABC FN42", 1500.0);
        // What the early site captures by 11.8 s: audio so far, zero tail.
        let cut = (11.8 * tempo_fast::SAMPLE_RATE as f64) as usize;
        let mut early = full.clone();
        for x in &mut early[cut..] {
            *x = 0.0;
        }
        let n_early = e.ingest_early(&early, 3);
        assert!(n_early >= 1, "early (truncated) pass must decode");
        assert!(
            e.last_decodes()
                .iter()
                .any(|d| d.message == "CQ W1ABC FN42"),
            "early pass recovered the message; got {:?}",
            e.last_decodes()
        );
        assert_eq!(e.last_decode_slot, Some(3), "boundary-slot convention");
        // The boundary then decodes the FULL window — same message must be
        // filtered (no double row in history, no double observe).
        let n_boundary = e.ingest(&full, 3);
        assert_eq!(n_boundary, 0, "boundary ingests only stragglers");
        let rows = e
            .decode_history
            .iter()
            .filter(|(_, d)| d.message == "CQ W1ABC FN42")
            .count();
        assert_eq!(rows, 1, "exactly one history row for the message");
    }

    /// The early pass is FT8/FT4-native only: FT1/DX1 decode full frames, and a
    /// Companion source drains a UDP queue the boundary owns.
    #[test]
    fn early_pass_gated_to_native_ft8_ft4() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::TempoFast);
        assert_eq!(e.ingest_early(&[0.0f32; 48000], 1), 0);
    }

    /// The a7 cross-cycle flag plumb through the decode split
    /// ([`Engine::build_decode_job`] → [`run_decode_job`]): only the
    /// authoritative boundary ingest passes `a7_final = true`. The early
    /// partial pass and the F6 redecode pass `false` — an early save would
    /// double-book every decode in the a7 table (halving replay capacity) and
    /// waste a full replay against zero-padded audio.
    #[test]
    fn a7_final_true_only_on_boundary_ingest() {
        struct FlagRecorder(std::sync::Arc<std::sync::Mutex<Vec<bool>>>);
        impl SignalSource for FlagRecorder {
            fn label(&self) -> String {
                "flag-recorder".into()
            }
            fn mode_kind(&self) -> Option<modes::ModeKind> {
                Some(modes::ModeKind::Ft8)
            }
            fn decode(&mut self, _req: &modes::DecodeRequest) -> Vec<modes::Decode> {
                Vec::new()
            }
            fn decode_a7(
                &mut self,
                _req: &modes::DecodeRequest,
                a7_final: bool,
            ) -> Vec<modes::Decode> {
                self.0.lock().unwrap().push(a7_final);
                Vec::new()
            }
        }
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        let flags = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        *e.source.lock().unwrap() = Box::new(FlagRecorder(flags.clone()));
        let frame = vec![0.0f32; 1024];
        e.ingest(&frame, 3); // boundary: authoritative full-audio pass
        e.ingest_early(&frame, 4); // early partial pass
        e.last_rx = Some(tempo_core::channel::capture_to_i16(&frame));
        e.last_decode_slot = Some(4);
        e.redecode(); // F6 review of retained (old) audio
        assert_eq!(
            *flags.lock().unwrap(),
            vec![true, false, false],
            "a7_final must be: ingest=true, ingest_early=false, redecode=false"
        );
    }

    /// Epoch guard: a decode that was in flight across a decode-context switch
    /// (tier/source/band change) must land STALE and be dropped — its slot indices
    /// and AP context belong to a context that no longer exists.
    #[test]
    fn decode_result_dropped_when_epoch_advances() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        // Full-size capture (the real FT8 decoder asserts a minimum length).
        let frame = vec![0.0f32; e.active_capture_samples()];
        // Build + DECODE the job in the current context (as the worker would finish
        // it), THEN switch tier — the race the guard exists for: the operator switches
        // after the decode ran but before its result is folded.
        let job = e.build_decode_job(frame, 5, DecodePass::Boundary);
        let result = run_decode_job(job);
        e.set_tier(Tier::Ft4); // clear_decode_context bumps the epoch
        assert!(
            matches!(e.apply_decode_result(result), DecodeApplied::Stale),
            "a result from the pre-switch context must be dropped as stale"
        );
    }

    /// Control for the epoch guard: with no context switch, a boundary result folds
    /// normally (the guard must not over-drop live results).
    #[test]
    fn decode_result_applies_when_epoch_unchanged() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        let frame = vec![0.0f32; e.active_capture_samples()];
        let job = e.build_decode_job(frame, 5, DecodePass::Boundary);
        let result = run_decode_job(job);
        assert!(
            matches!(
                e.apply_decode_result(result),
                DecodeApplied::Boundary { .. }
            ),
            "an in-context boundary result folds (Boundary), even with zero decodes"
        );
    }

    /// Source swap: `set_tier` replaces the boxed decoder UNDER its lock (the stable
    /// serialization mutex is preserved). A decode built AFTER the swap must run
    /// through the NEW decoder, never the one that was swapped away.
    #[test]
    fn set_tier_swaps_the_decoder_under_its_lock() {
        struct Recorder(std::sync::Arc<std::sync::Mutex<u32>>);
        impl SignalSource for Recorder {
            fn label(&self) -> String {
                "recorder".into()
            }
            fn mode_kind(&self) -> Option<modes::ModeKind> {
                Some(modes::ModeKind::Ft8)
            }
            fn decode(&mut self, _r: &modes::DecodeRequest) -> Vec<modes::Decode> {
                *self.0.lock().unwrap() += 1;
                Vec::new()
            }
            fn decode_a7(&mut self, _r: &modes::DecodeRequest, _f: bool) -> Vec<modes::Decode> {
                *self.0.lock().unwrap() += 1;
                Vec::new()
            }
        }
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        let hits = std::sync::Arc::new(std::sync::Mutex::new(0u32));
        *e.source.lock().unwrap() = Box::new(Recorder(hits.clone()));
        // Switch tiers: the recorder is swapped out for a real NativeSource(FT4).
        e.set_tier(Tier::Ft4);
        let job = e.build_decode_job(
            vec![0.0f32; e.active_capture_samples()],
            2,
            DecodePass::Boundary,
        );
        let _ = run_decode_job(job);
        assert_eq!(
            *hits.lock().unwrap(),
            0,
            "the swapped-away decoder must never be invoked after the tier switch"
        );
    }

    /// Like [`native_frame_for`] but at a target SNR with seeded AWGN (the repo
    /// channel convention: `snr_to_scale` signal scale + unit-variance noise).
    fn noisy_frame_for(
        kind: modes::ModeKind,
        msg: &str,
        f0: f32,
        snr_db: f32,
        seed: u64,
    ) -> Vec<f32> {
        let mode = modes::make_mode(kind);
        let tones = mode.encode(msg);
        assert!(!tones.is_empty(), "{} encode failed", kind.as_str());
        let wave = mode.gen_wave(&tones, tempo_fast::SAMPLE_RATE, f0);
        let n = mode.frame_samples();
        // FT8/FT4 gen_wave is now slot-positioned (includes the 0.5 s lead-in), so place
        // it at the slot start; the wave already carries its own offset.
        let off = 0;
        let sig = tempo_core::channel::snr_to_scale(snr_db, tempo_fast::SAMPLE_RATE);
        let mut noise = tempo_core::channel::Awgn::new(seed);
        let mut frame = vec![0f32; n];
        for (i, &s) in wave.iter().enumerate() {
            if off + i < n {
                frame[off + i] = sig * s;
            }
        }
        for s in frame.iter_mut() {
            *s += noise.sample();
        }
        // The channel convention (unit-variance noise) is at the old ×100 int16 scale;
        // the real decode path now uses capture_to_i16 (×32767, matching real normalized
        // capture). Scale the whole frame (signal + noise together, so SNR is preserved)
        // to real-capture range so ×32767 reproduces the exact int16 levels the decoder
        // was validated against — no clipped noise.
        const HARNESS_TO_CAPTURE: f32 = 100.0 / 32767.0;
        for s in frame.iter_mut() {
            *s *= HARNESS_TO_CAPTURE;
        }
        frame
    }

    /// End-to-end proof that the LIVE engine feeds a-priori (AP) context from the
    /// active QSO into the golden FT8 decoder. At −22 dB the no-context FT8
    /// decoder recovers an RR73 0% of the time (see ft8's `ap_decode` tests), so
    /// recovery here can ONLY come from `ingest` passing our call + the worked
    /// station + nQSOProgress. The blanked-call control runs the identical path
    /// with AP disabled, isolating the wiring as the cause.
    #[test]
    fn engine_qso_feeds_ap_context_to_recover_marginal_frames() {
        let msg = "KD9TAW W1AW RR73"; // RR73 to me → iaptype 6 (all 77 ap bits)
        let seeds = 6u64;
        let (mut recovered, mut control) = (0u32, 0u32);
        for seed in 0..seeds {
            let frame = noisy_frame_for(modes::ModeKind::Ft8, msg, 1500.0, -22.0, seed);

            // AP path: FT8 QSO, our call KD9TAW, working W1AW, awaiting RR73.
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.call_station_with_grid("W1AW", Some("FN31"));
            if let Mode::Qso { station, .. } = &mut e.mode {
                station.state = QsoState::AwaitRr73; // → nQSOProgress 3 (deep AP)
            }
            e.ingest(&frame, 100);
            if e.last_decodes().iter().any(|d| d.message == msg) {
                recovered += 1;
            }

            // Control: identical engine/QSO, but blank MyCall → no AP possible.
            let mut c = Engine::new("KD9TAW", "EN52", 0);
            c.call_station_with_grid("W1AW", Some("FN31"));
            if let Mode::Qso { station, .. } = &mut c.mode {
                station.state = QsoState::AwaitRr73;
            }
            c.settings.mycall.clear();
            c.ingest(&frame, 100);
            if c.last_decodes().iter().any(|d| d.message == msg) {
                control += 1;
            }
        }
        assert_eq!(
            control, 0,
            "no operator call → no AP; the frame must stay undecoded ({control}/{seeds})"
        );
        assert!(
            recovered >= 4,
            "the QSO engine must feed AP context and recover the marginal frame ({recovered}/{seeds})"
        );
    }

    #[test]
    fn ft4_captures_the_full_slot_not_just_the_decode_frame() {
        // FT4's slot (7.5 s) is longer than its decode frame (6.048 s). The RX ring
        // must CAPTURE the whole slot so the decoder reads its head (leading sync);
        // capturing only the frame kept the slot tail and amputated sync.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft4);
        assert_eq!(
            e.active_frame_samples(),
            modes::ModeKind::Ft4.frame_samples()
        );
        assert_eq!(
            e.active_capture_samples(),
            90_000,
            "capture the full 7.5 s slot"
        );
        assert!(
            e.active_capture_samples() > e.active_frame_samples(),
            "FT4 captures more than it decodes (slot > frame)"
        );
        // FT8/FT1: slot == decode frame, so capture == frame (no change).
        e.set_tier(Tier::Ft8);
        assert_eq!(e.active_capture_samples(), e.active_frame_samples());
        e.set_tier(Tier::TempoFast);
        assert_eq!(e.active_capture_samples(), e.active_frame_samples());
    }

    /// FT4 decodes through the live engine ingest path on the FT4 tier (7.5 s,
    /// 72576-sample frame).
    #[test]
    fn ft4_live_ingest_decodes() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft4);
        assert_eq!(
            e.active_frame_samples(),
            modes::ModeKind::Ft4.frame_samples()
        );
        assert_eq!(e.active_slot_secs(), 7.5);
        let frame = native_frame_for(modes::ModeKind::Ft4, "CQ KD9TAW EN52", 1500.0);
        let n = e.ingest(&frame, 2);
        assert!(n >= 1, "FT4 frame should decode at least one signal");
        assert!(
            e.last_decodes()
                .iter()
                .any(|d| d.message == "CQ KD9TAW EN52"),
            "FT4 live ingest must recover the message; got {:?}",
            e.last_decodes()
        );
    }

    #[test]
    fn pick_caller_honors_each_strategy() {
        // A synthetic pileup answering my CQ (indices = decode order):
        //   idx0 K1ABC — weak, New England (FN42)
        //   idx1 W5FAR — loud, SoCal (DM13, farthest from EN37)
        //   idx2 N0CQ  — mid SNR, was itself calling CQ
        //   idx3 K9BARE — loudest but grid-less (bare-report answer)
        let callers = vec![
            CqCaller {
                decode_idx: 0,
                snr: -18,
                calling_cq: false,
                grid: Some("FN42".into()),
            },
            CqCaller {
                decode_idx: 1,
                snr: -3,
                calling_cq: false,
                grid: Some("DM13".into()),
            },
            CqCaller {
                decode_idx: 2,
                snr: -10,
                calling_cq: true,
                grid: Some("EN37".into()),
            },
            CqCaller {
                decode_idx: 3,
                snr: -1,
                calling_cq: false,
                grid: None,
            },
        ];
        let mygrid = "EN37";

        // "first" = earliest decode, stock WSJT-X.
        assert_eq!(pick_caller(&callers, "first", None, mygrid), Some(0));
        // "strongest" = max SNR (idx3 at -1 dB).
        assert_eq!(pick_caller(&callers, "strongest", None, mygrid), Some(3));
        // "farthest" = greatest distance from EN37 (DM13); the loud grid-less
        // caller (idx3) still ranks last for want of a grid.
        assert_eq!(pick_caller(&callers, "farthest", None, mygrid), Some(1));
        // "cq_first" = prefer the station that was itself calling CQ (idx2),
        // even though idx3 is louder.
        assert_eq!(pick_caller(&callers, "cq_first", None, mygrid), Some(2));

        // SNR floor drops everyone weaker than -5 → only idx1/idx3 survive; the
        // earliest survivor is idx1.
        assert_eq!(pick_caller(&callers, "first", Some(-5), mygrid), Some(1));
        // A floor above every caller → nobody to work.
        assert_eq!(pick_caller(&callers, "strongest", Some(0), mygrid), None);
        // With a floor, "strongest" still ranks within the survivors (idx3).
        assert_eq!(
            pick_caller(&callers, "strongest", Some(-5), mygrid),
            Some(3)
        );
        // Empty pileup → nothing to pick.
        assert_eq!(pick_caller(&[], "strongest", None, mygrid), None);
    }

    #[test]
    fn cq_run_defaults_to_first_caller_in_the_slot() {
        // Regression: the default strategy must remain stock first-heard — it
        // works the FIRST caller decoded regardless of SNR or distance.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_mode("qso-run").unwrap();
        e.poll_tx(0); // CQ out on our slot
        e.ingest_decodes_for_test(
            &[
                dec_snr("W9XYZ K1ABC FN42", -18),
                dec_snr("W9XYZ W5LOUD EM12", -3),
            ],
            1,
        );
        assert_eq!(
            e.snapshot().qso.unwrap().dxcall.as_deref(),
            Some("K1ABC"),
            "default best_caller=first works the earliest answerer"
        );
    }

    /// The source precedence that decides which row the waterfall shows.
    ///
    /// It used to live in `Engine::spectrum_row`, where all three writers had to reach the
    /// engine mutex to publish — and the radio loop holds that mutex across blocking CAT at the
    /// slot boundary, so a stall starved the audio row AND the Flex/CI-V panadapter together
    /// (operator report, 2026-07-25: the waterfall "hangs and stops moving"). The rule is
    /// unchanged; it moved to where the writers converge without a lock anyone else contends.
    #[test]
    fn a_fresh_native_row_wins_over_audio_and_falls_back_when_cleared() {
        let feed = SpectrumFeed::default();
        feed.publish_audio(Spectrum {
            row: vec![0.1, 0.2, 0.3],
            lo_hz: 0.0,
            hi_hz: 4000.0,
            source: "audio".into(),
        });
        assert_eq!(feed.row().expect("audio published").source, "audio");

        feed.publish_rf(Spectrum {
            row: vec![1.0, 2.0, 3.0],
            lo_hz: 144_000_000.0,
            hi_hz: 144_200_000.0,
            source: "civ".into(),
        });
        let row = feed.row().expect("rf published");
        assert_eq!(row.source, "civ", "a fresh native RF row wins");
        assert_eq!(row.row, vec![1.0, 2.0, 3.0]);

        // DATA mode drops the native row: the audio FFT must take over IMMEDIATELY, with no
        // window where the last panadapter row still wins.
        feed.clear_rf();
        assert_eq!(
            feed.row().expect("audio still there").source,
            "audio",
            "clearing RF falls straight back to the audio FFT"
        );
    }

    /// An EMPTY native row must never win — a panadapter that streams nothing would otherwise
    /// blank the waterfall instead of letting the audio FFT through.
    #[test]
    fn an_empty_native_row_never_wins() {
        let feed = SpectrumFeed::default();
        feed.publish_audio(Spectrum {
            row: vec![0.1, 0.2],
            lo_hz: 0.0,
            hi_hz: 4000.0,
            source: "audio".into(),
        });
        feed.publish_rf(Spectrum {
            row: Vec::new(),
            lo_hz: 0.0,
            hi_hz: 0.0,
            source: "flex".into(),
        });
        assert_eq!(feed.row().expect("published").source, "audio");
    }

    /// Nothing published at all → `None`, which routes the reader to the Companion/UDP fallback
    /// that computes a row on demand. That is the ONLY case allowed to reach the engine.
    #[test]
    fn an_unfed_feed_returns_none_so_the_companion_path_can_compute() {
        assert!(SpectrumFeed::default().row().is_none());
    }

    #[test]
    fn cq_run_strongest_works_the_loudest_caller() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.settings.best_caller = "strongest".to_string();
        e.set_mode("qso-run").unwrap();
        e.poll_tx(0);
        e.ingest_decodes_for_test(
            &[
                dec_snr("W9XYZ K1ABC FN42", -18),
                dec_snr("W9XYZ W5LOUD EM12", -3),
            ],
            1,
        );
        assert_eq!(
            e.snapshot().qso.unwrap().dxcall.as_deref(),
            Some("W5LOUD"),
            "best_caller=strongest works the loudest answerer, not the first"
        );
    }

    #[test]
    fn cq_run_min_snr_skips_a_too_weak_caller_and_keeps_calling() {
        // A caller below the SNR floor must NOT be worked; the run stays on CQ.
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.settings.best_caller_min_snr = Some(-10);
        e.set_mode("qso-run").unwrap();
        e.poll_tx(0);
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K1ABC FN42", -18)], 1);
        let qso = e.snapshot().qso.expect("still running");
        assert_eq!(
            qso.state, "CallingCq",
            "too-weak caller ignored — the run keeps calling CQ"
        );
        assert!(qso.dxcall.is_none(), "did not lock the below-floor caller");
    }

    /// A dead capture stream (device unplugged, DAX lost) must GO QUIET, not keep scrolling the
    /// last row as steady phantom carriers. Moved here with the rest of the precedence rule.
    #[test]
    fn a_stale_audio_row_goes_quiet_instead_of_scrolling_a_frozen_ghost() {
        let feed = SpectrumFeed::default();
        feed.publish_audio(Spectrum {
            row: vec![0.1, 0.2, 0.3],
            lo_hz: 0.0,
            hi_hz: 4000.0,
            source: "audio".into(),
        });
        assert!(
            !feed.row().expect("fresh").row.is_empty(),
            "fresh row draws"
        );

        feed.backdate_audio_for_test(std::time::Duration::from_secs(3));
        let row = feed.row().expect("still returns a row, deliberately");
        assert!(
            row.row.is_empty(),
            "a stale row must come back EMPTY so the waterfall stops"
        );
        // The critical half: it must NOT return None, which would route the reader to the
        // engine's on-demand fallback and redraw a non-empty ghost row at 8-20 Hz.
    }

    #[test]
    fn the_dedicated_ptt_port_follows_a_radio_switch() {
        // Operator report 2026-07-25 (U2R / SO2R interface): "each radio needs its own serial RTS
        // port for PTT. When switching between radios via the top left button, the PTT port does
        // not follow the selected radio. CAT works fine."
        //
        // Root cause: `ptt_serial_port` lived ONLY on the flat Settings, never on RadioProfile —
        // so `serial_port` (CAT) followed the switch and the keyline port did not. The radio loop
        // builds PttMode::Serial{port} from the flat mirror, so after a switch it keyed the
        // PREVIOUS radio's port. This pins that both ports now travel together.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.ensure_radio_profiles();

        // Radio 0: CAT on COM3, keyline on its own COM8 (the whole point of an SO2R box).
        e.settings.ptt_method = "rts".to_string();
        e.settings.serial_port = "COM3".to_string();
        e.settings.ptt_serial_port = "COM8".to_string();
        e.settings.sync_active_from_flat();

        // Radio 1: CAT on COM7, keyline on COM9.
        let r1 = e.add_radio();
        e.settings.ptt_method = "rts".to_string();
        e.settings.serial_port = "COM7".to_string();
        e.settings.ptt_serial_port = "COM9".to_string();
        e.settings.sync_active_from_flat();

        // Switch back to radio 0 — the flat mirror is what the radio loop keys from.
        e.set_active_radio(0);
        assert_eq!(
            e.settings.serial_port, "COM3",
            "CAT followed (it always did)"
        );
        assert_eq!(
            e.settings.ptt_serial_port, "COM8",
            "the KEYLINE must follow too — keying the other radio's port is the bug"
        );

        // ...and forward again.
        e.set_active_radio(r1);
        assert_eq!(e.settings.serial_port, "COM7");
        assert_eq!(
            e.settings.ptt_serial_port, "COM9",
            "the keyline follows in both directions"
        );
    }

    #[test]
    fn a_shared_keyline_port_is_still_allowed() {
        // The common single-cable case: no dedicated PTT port, so keying rides the CAT port.
        // Making the field per-radio must not force operators to fill it in.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.ensure_radio_profiles();
        e.settings.serial_port = "COM3".to_string();
        e.settings.ptt_serial_port = String::new();
        e.settings.sync_active_from_flat();
        let r1 = e.add_radio();
        e.set_active_radio(0);
        assert_eq!(
            e.settings.ptt_serial_port, "",
            "empty stays empty — falls back to the CAT port"
        );
        let _ = r1;
    }

    #[test]
    fn saving_a_stale_form_port_cannot_diverge_the_flat_mirror_from_the_active_profile() {
        // THE dual-radio dead-CAT bug (2026-07-10): the active-radio loop reads the rigctld port from
        // the flat mirror (Transport::from_settings) while monitors read it per-profile
        // (Transport::from_profile). If a Save carries a STALE/colliding flat port, apply_settings folds
        // it into the active profile, ensure_distinct_radio_ports BUMPS the profile back to a distinct
        // port — but if the flat mirror is NOT then re-synced, flat and profile DIVERGE: the active rig
        // and a monitor both try to bind the old port, the monitor's rigctld dies, and CAT is dead on
        // whichever radio is the monitor. This asserts flat == the (de-conflicted) active profile port.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.ensure_radio_profiles();
        e.settings.rig_model = 1042; // FTDX10 on radio 0 (rigctld 4532)
        e.settings.sync_active_from_flat();
        let r1 = e.add_radio(); // IC-9700 → distinct rigctld port (4534), now active
        e.settings.rig_model = 3081;
        e.settings.sync_active_from_flat();

        let r0_port = e
            .settings
            .radios
            .iter()
            .find(|p| p.id == 0)
            .unwrap()
            .rigctld_port;
        // Operator saves the IC-9700's config, but the form still shows radio 0's stale rigctld port
        // (the frontend keeps the previous flat fields on a live roster change) → a collision payload.
        let mut form = e.settings().clone();
        assert_eq!(
            form.active_radio, r1,
            "the form is editing the active (IC-9700)"
        );
        form.rigctld_port = r0_port; // the stale/colliding port
        e.apply_settings(form);

        let r1_port = e
            .settings
            .radios
            .iter()
            .find(|p| p.id == r1)
            .unwrap()
            .rigctld_port;
        assert_ne!(
            r1_port, r0_port,
            "ensure_distinct_radio_ports keeps the two radios' daemon ports distinct"
        );
        assert_eq!(
            e.settings.rigctld_port, r1_port,
            "flat mirror (active loop's Transport::from_settings) == the de-conflicted active profile \
             port (monitors' Transport::from_profile) — no divergence, so neither daemon collides"
        );
    }

    #[test]
    fn set_frequency_auto_routes_the_band_to_the_covering_radio() {
        // Dual-Radio P4: selecting 2 m (the band dropdown / manual entry, both via set_frequency) must
        // hand off to the IC-9700 (radio 1, which has 2 m configured) and land it on the 2 m dial —
        // and swinging back to an HF band returns to the FTDX10. Peg-lock pins the active radio.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.ensure_radio_profiles();
        e.settings.rig_model = 1042; // FTDX10 on radio 0 (catch-all: no band list)
        e.settings.band = "20m".into();
        e.settings.dial_mhz = 14.074;
        e.settings.sync_active_from_flat();
        let r1 = e.add_radio(); // IC-9700, now active
        e.settings.rig_model = 3081;
        e.settings.sync_active_from_flat();
        e.set_radio_bands(r1, vec!["2m".into()]); // IC-9700 explicitly covers 2 m
        e.set_active_radio(0); // back on the FTDX10

        // Select 2 m → routes to the IC-9700 AND parks it on the requested dial.
        e.set_frequency(144.200, "2m", "USB");
        assert_eq!(
            e.settings.active_radio, r1,
            "2 m selection switched to the IC-9700"
        );
        assert_eq!(e.settings.band, "2m");
        assert!(
            (e.settings.dial_mhz - 144.200).abs() < 1e-9,
            "landed on the 2 m dial"
        );

        // Select an HF band → swings back to the FTDX10 (catch-all beats the IC-9700's 2 m-only list).
        e.set_frequency(14.074, "20m", "USB");
        assert_eq!(
            e.settings.active_radio, 0,
            "HF selection switched back to the FTDX10"
        );

        // Peg-lock: no auto-switch. Selecting 2 m stays on the FTDX10 (out-of-range TX-lock handles it).
        e.set_radio_pegged(true);
        e.set_frequency(144.200, "2m", "USB");
        assert_eq!(
            e.settings.active_radio, 0,
            "pegged → band selection never switches radios"
        );
    }

    #[test]
    fn aprs_tune_switches_to_the_2m_radio_like_every_other_qsy() {
        // The operator's exact setup: FTDX10 (HF, active) + IC-9700 (2 m). APRS Tune must hand off
        // to the 9700 just like the band picker does — aprs_tune routes through the same
        // set_frequency/radio_for_band as every working mode.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.ensure_radio_profiles();
        e.settings.rig_model = 1042; // FTDX10 on radio 0 (catch-all coverage)
        e.settings.band = "20m".into();
        e.settings.dial_mhz = 14.074;
        e.settings.sync_active_from_flat();
        let r1 = e.add_radio(); // IC-9700, now active
        e.settings.rig_model = 3081;
        e.settings.sync_active_from_flat();
        e.set_radio_bands(r1, vec!["2m".into()]); // 9700 covers 2 m (operator confirmed this is ON)
        e.set_active_radio(0); // back on the FTDX10, as when the operator opens APRS

        e.aprs_tune(144.390).unwrap();
        assert_eq!(
            e.settings.active_radio, r1,
            "APRS Tune switched to the IC-9700"
        );
        assert!(
            (e.settings.dial_mhz - 144.390).abs() < 1e-9,
            "landed on the APRS dial"
        );
        assert_eq!(e.rig_mode_effective(), "FM");
    }

    /// The operator's actual three-radio shack, wired the way the app wires it (real profiles, real
    /// rig models, the routing rules from Settings). Returns `(ic9700, ft991a)`; radio 0 is the
    /// FTdx10 and is left ACTIVE, which is where an HF operator starts the day.
    fn three_radio_engine() -> (Engine, u32, u32) {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.ensure_radio_profiles();
        e.settings.rig_model = 1042; // FTdx10 on radio 0 — catch-all coverage (HF everything)
        e.settings.sync_active_from_flat();
        let ic9700 = e.add_radio();
        e.settings.rig_model = 3081;
        e.settings.sync_active_from_flat();
        let ft991a = e.add_radio();
        e.settings.rig_model = 1035;
        e.settings.sync_active_from_flat();
        e.rename_radio(0, "FTdx10");
        e.rename_radio(ic9700, "IC-9700");
        e.rename_radio(ft991a, "FT-991A");
        // Both VHF rigs cover 2 m / 70 cm — which is exactly why band-only routing can't decide.
        e.set_radio_bands(ic9700, vec!["2m".into(), "70cm".into()]);
        e.set_radio_bands(ft991a, vec!["2m".into(), "70cm".into()]);
        e.set_routing_rules(vec![
            crate::settings::RoutingRule {
                bands: vec!["2m".into(), "70cm".into()],
                mode: Some(crate::settings::RouteMode::Fm),
                radio: ft991a,
            },
            crate::settings::RoutingRule {
                bands: vec!["2m".into(), "70cm".into()],
                mode: Some(crate::settings::RouteMode::Digital),
                radio: ic9700,
            },
        ]);
        e.set_default_radio(Some(0));
        e.set_active_radio(0);
        e.settings.band = "20m".into();
        e.settings.dial_mhz = 14.074;
        (e, ic9700, ft991a)
    }

    #[test]
    fn working_a_2m_ft8_spot_routes_to_the_digital_radio() {
        // THE driving case. Clicking a 2 m FT8 spot must reach the IC-9700, not the FT-991A —
        // band-only routing cannot tell them apart (both list 2 m), so this is what mode-class
        // routing exists for.
        let (mut e, ic9700, _ft991a) = three_radio_engine();
        e.work_spot("operate", 144.174, "2m"); // the Needed-board click: mode + QSY, one lock
        assert_eq!(
            e.settings.active_radio, ic9700,
            "2 m FT8 routed to the IC-9700"
        );
        assert!((e.settings.dial_mhz - 144.174).abs() < 1e-9);
        assert_eq!(
            e.settings.operating_mode,
            crate::settings::OperatingMode::Digital
        );
    }

    #[test]
    fn aprs_tune_routes_to_the_fm_radio_not_merely_the_2m_radio() {
        // aprs_tune's OLD contract was "switch to the radio that covers 2 m". With two rigs on 2 m
        // that is ambiguous — and the ambiguity resolves the WRONG way here, because coverage-tier
        // ties go to the lowest id (the IC-9700). APRS must route on the FM mode class.
        let (mut e, ic9700, ft991a) = three_radio_engine();
        e.aprs_tune(144.390)
            .expect("aprs_tune should succeed in this scenario");
        assert_eq!(
            e.settings.active_radio, ft991a,
            "APRS routed to the FT-991A (the FM radio), not the IC-9700"
        );
        assert_ne!(e.settings.active_radio, ic9700);
        assert!((e.settings.dial_mhz - 144.390).abs() < 1e-9);
        assert_eq!(e.rig_mode_effective(), "FM", "still forces FM simplex");
    }

    #[test]
    fn aprs_tune_routes_on_fm_even_when_arriving_from_the_digital_section() {
        // The ordering trap: `set_frequency` CLEARS `aprs_fm` (a QSY leaves the APRS context), so a
        // routing decision derived from live state would see the section being LEFT. Arriving from
        // Operate (Digital) must still route APRS to the FM radio — that is what `route_intent` is.
        let (mut e, _ic9700, ft991a) = three_radio_engine();
        e.set_operating_mode("operate", false); // sitting in the FT8 cockpit
        e.aprs_tune(144.390)
            .expect("aprs_tune should succeed in this scenario");
        assert_eq!(e.settings.active_radio, ft991a);
    }

    #[test]
    fn a_2m_fm_repeater_qsy_routes_to_the_fm_radio() {
        // The other half of the operator's spec: a 2 m FM/repeater context (Phone + phone_mode=fm,
        // which is what the Memories FM channels set) goes to the FT-991A.
        let (mut e, _ic9700, ft991a) = three_radio_engine();
        e.settings.phone_mode = "fm".into();
        e.set_operating_mode("phone", false);
        e.set_frequency(146.520, "2m", "FM");
        assert_eq!(e.settings.active_radio, ft991a, "2 m FM → the FT-991A");

        // …and 2 m SSB (phone, NOT fm) matches neither mode rule, so it falls to band coverage,
        // where the tie breaks to the IC-9700. Recorded as the honest consequence of the operator's
        // rule set: they mapped 2 m digital and 2 m FM, not 2 m SSB.
        e.settings.phone_mode = "usb".into();
        e.set_active_radio(0);
        e.set_frequency(144.200, "2m", "USB");
        assert_eq!(e.settings.active_radio, 1);
    }

    #[test]
    fn repeater_tune_routes_on_fm_even_when_arriving_from_the_digital_section() {
        // The defect `repeater_tune` exists to fix. The Program section's tune-now wrote the FM
        // settings and then called `set_frequency`, which derives the mode class from live state —
        // so starting from Operate, a 2 m repeater tune routed on Digital and landed on the FT8
        // radio. Same trap as aprs_tune, same fix: name the class explicitly.
        let (mut e, ic9700, ft991a) = three_radio_engine();
        e.set_operating_mode("operate", false); // sitting in the FT8 cockpit
        e.repeater_tune(146.940, "minus", 600_000, 103.5).unwrap();
        assert_eq!(
            e.settings.active_radio, ft991a,
            "the repeater tune routed to the FM radio, not the FT8 radio"
        );
        assert_ne!(e.settings.active_radio, ic9700);
        assert!((e.settings.dial_mhz - 146.940).abs() < 1e-9);
        assert_eq!(e.settings.band, "2m", "band derived from the frequency");
        assert_eq!(e.rig_mode_effective(), "FM");
        // The machine's own plumbing reached the radio loop's read point.
        assert_eq!(
            e.fm_repeater_config(),
            ("minus".to_string(), 600_000, 103.5),
            "shift / offset / tone all present with the QSY"
        );
    }

    #[test]
    fn repeater_tune_keys_an_odd_split_on_its_exact_offset() {
        // A 2 m machine with a +1 MHz input: the band CONVENTION is 600 kHz, so anything that
        // ignores the exact offset transmits 400 kHz off the repeater's input and the operator
        // never gets in. This is what rptr_offset_override_hz is for.
        let (mut e, _ic9700, _ft991a) = three_radio_engine();
        e.repeater_tune(145.110, "plus", 1_000_000, 0.0).unwrap();
        let (shift, offset_hz, tone) = e.fm_repeater_config();
        assert_eq!(shift, "plus");
        assert_eq!(
            offset_hz, 1_000_000,
            "the exact split, not the 600 kHz default"
        );
        assert_eq!(tone, 0.0, "a toneless machine gets no tone");

        // A tone-less simplex channel is a legitimate repeater_tune target too (a calling freq).
        // Only the SHIFT and the tone are asserted: with no override, `rptr_offset_hz` reports the
        // band convention, and the offset register is don't-care under "R None".
        e.repeater_tune(146.520, "simplex", 0, 0.0).unwrap();
        let (shift, _offset_dont_care, tone) = e.fm_repeater_config();
        assert_eq!(shift, "simplex");
        assert_eq!(tone, 0.0);
    }

    #[test]
    fn repeater_tune_commands_fm_from_a_digital_section_and_lets_go_cleanly() {
        // Routing the QSY to the right radio is only half the job: the rig must also be told FM.
        // The mode policy is keyed on the operating SECTION, so without the `fm_channel` hold a
        // repeater tuned out of Operate was commanded PKTUSB — a repeater demodulated as packet.
        let (mut e, _ic9700, _ft991a) = three_radio_engine();
        e.set_operating_mode("operate", false);
        e.repeater_tune(146.940, "minus", 600_000, 103.5).unwrap();
        assert_eq!(
            e.rig_mode_effective(),
            "FM",
            "commanded FM, not the section's mode"
        );
        assert_eq!(
            e.settings.operating_mode,
            crate::settings::OperatingMode::Digital,
            "listening to a repeater must NOT move the operator into the Phone section \
             (set_operating_mode arms transmit — a tune may never do that)"
        );

        // …and it lets go. Every route out of the FM channel drops the hold, so FM can't linger.
        e.set_frequency(14.074, "20m", "USB");
        assert_ne!(e.rig_mode_effective(), "FM", "a later QSY escaped FM");

        // A knob-QSY down to HF doesn't route through set_frequency at all — the dial gate is the
        // backstop there (the aprs_fm band-gate lesson, restated for a multi-band hold).
        e.repeater_tune(146.940, "minus", 600_000, 103.5).unwrap();
        e.observe_rig_freq(14_074_000);
        assert_ne!(
            e.rig_mode_effective(),
            "FM",
            "FM followed the operator down to 20 m on a knob QSY"
        );
    }

    #[test]
    fn repeater_tune_refuses_a_bad_request_and_leaves_the_radio_alone() {
        let (mut e, _ic9700, _ft991a) = three_radio_engine();
        let (dial, band) = (e.settings.dial_mhz, e.settings.band.clone());

        // An unrecognised shift must be REFUSED, not degraded: rptr_shift_line maps anything it
        // doesn't know to "no shift", which would key the repeater's own OUTPUT frequency.
        assert!(e
            .repeater_tune(146.940, "negative", 600_000, 103.5)
            .is_err());
        // Outside the band plan there is no band to hand the routing step.
        assert!(e.repeater_tune(160.000, "minus", 600_000, 103.5).is_err());

        assert!(
            (e.settings.dial_mhz - dial).abs() < 1e-9,
            "refusal moved the dial"
        );
        assert_eq!(e.settings.band, band);
        assert_eq!(e.settings.active_radio, 0, "refusal switched radios");
        // A refusal must not leave a one-shot FM intent armed to misroute the NEXT QSY.
        e.set_operating_mode("operate", false);
        e.set_frequency(144.174, "2m", "USB");
        assert_eq!(
            e.settings.active_radio, 1,
            "the next 2 m digital QSY still routed on Digital"
        );
    }

    #[test]
    fn hf_actions_route_to_the_hf_radio_in_every_mode() {
        // "any HF action → FTdx10", from whichever VHF rig is active, in every mode class.
        let (mut e, ic9700, ft991a) = three_radio_engine();
        for (section, band, dial) in [
            ("operate", "20m", 14.074),
            ("phone", "20m", 14.250),
            ("cw", "40m", 7.030),
            ("rtty", "20m", 14.083),
        ] {
            for start in [ic9700, ft991a] {
                e.set_active_radio(start);
                e.set_operating_mode(section, false);
                e.set_frequency(dial, band, "USB");
                assert_eq!(
                    e.settings.active_radio, 0,
                    "{section} on {band} from radio {start} → the FTdx10"
                );
            }
        }
    }

    #[test]
    fn peg_lock_suppresses_mode_routing_too() {
        // Peg-lock is the operator's "don't move my radio" latch; adding a mode dimension must not
        // give routing a new way around it.
        let (mut e, _ic9700, _ft991a) = three_radio_engine();
        e.set_radio_pegged(true);
        e.work_spot("operate", 144.174, "2m");
        assert_eq!(e.settings.active_radio, 0, "pegged → no auto-switch");
        e.aprs_tune(144.390)
            .expect("aprs_tune should succeed in this scenario");
        assert_eq!(
            e.settings.active_radio, 0,
            "pegged → APRS doesn't switch either"
        );
    }

    #[test]
    fn route_mode_mirrors_the_rig_mode_policys_fm_gates() {
        // The routing class must never disagree with the mode the rig is then put in. Both FM gates
        // in `rig_mode`/`rig_mode_effective` are mirrored: APRS-FM only on 2 m, Phone-FM only ≥29 MHz.
        use crate::settings::{OperatingMode, RouteMode};
        let (mut e, _ic9700, _ft991a) = three_radio_engine();

        e.settings.operating_mode = OperatingMode::Phone;
        e.settings.phone_mode = "fm".into();
        assert_eq!(e.route_mode("2m", 146.520), RouteMode::Fm);
        assert_eq!(
            e.route_mode("20m", 14.250),
            RouteMode::Ssb,
            "phone_mode is station-wide and nothing resets it on a band change — FM must not \
             follow the operator down to HF, in routing OR in the commanded mode"
        );
        e.settings.phone_mode = "usb".into();
        assert_eq!(e.route_mode("2m", 144.200), RouteMode::Ssb);

        // The cockpit's explicit FM pick wins with no band gate — `rig_mode_effective` returns the
        // override verbatim, so routing must agree. But ONLY for a same-band move: `set_frequency`
        // drops the override on a band change, so on a cross-band QSY it describes the band being
        // LEFT and must not drag FM onto the new one.
        e.settings.band = "2m".into();
        e.sideband_override = Some("FM".into());
        assert_eq!(e.route_mode("2m", 146.520), RouteMode::Fm);
        assert_eq!(
            e.route_mode("20m", 14.250),
            RouteMode::Ssb,
            "a cross-band QSY drops the override, so it must not route on it"
        );
        e.sideband_override = None;

        e.settings.operating_mode = OperatingMode::Digital;
        assert_eq!(e.route_mode("2m", 144.174), RouteMode::Digital);
        e.settings.operating_mode = OperatingMode::Cw;
        assert_eq!(e.route_mode("40m", 7.030), RouteMode::Cw);
        e.settings.operating_mode = OperatingMode::Rtty;
        assert_eq!(e.route_mode("20m", 14.083), RouteMode::Rtty);

        // In the APRS context, a QSY that stays on 2 m routes FM; one that leaves 2 m routes on the
        // underlying section, because `aprs_fm` is meaningless off 2 m (same gate as rig_mode_effective).
        e.settings.operating_mode = OperatingMode::Digital;
        e.aprs_tune(144.390)
            .expect("aprs_tune should succeed in this scenario");
        assert_eq!(e.route_mode("2m", 144.800), RouteMode::Fm);
        assert_eq!(e.route_mode("20m", 14.074), RouteMode::Digital);
    }

    #[test]
    fn route_preview_answers_where_a_band_and_mode_would_go() {
        // The Settings "test" affordance: read-only, and it names a radio even when the answer is
        // "stay where you are" (so the operator sees a rig, never a blank).
        use crate::settings::RouteMode;
        let (e, ic9700, ft991a) = three_radio_engine();
        assert_eq!(e.route_preview("2m", RouteMode::Digital), ic9700);
        assert_eq!(e.route_preview("2m", RouteMode::Fm), ft991a);
        assert_eq!(e.route_preview("70cm", RouteMode::Fm), ft991a);
        assert_eq!(e.route_preview("20m", RouteMode::Ssb), 0);
        assert_eq!(e.route_preview("40m", RouteMode::Cw), 0);
        assert_eq!(
            e.settings.active_radio, 0,
            "preview is read-only — it must not move the radio"
        );
    }

    #[test]
    fn a_stale_form_save_cannot_revert_the_routing_rules() {
        // Same class as the roster: the rules are LIVE state owned by `set_routing_rules`, and while
        // the operator edits a non-active radio the panel never sends the form at all — so a later
        // plain Save carries a form loaded before the rules existed.
        let (mut e, _ic9700, ft991a) = three_radio_engine();
        let rules = e.settings.routing_rules.clone();
        let mut stale = Settings::default();
        stale.ensure_radio_profiles(); // a one-radio form with no rules
        e.apply_settings(stale);
        assert_eq!(
            e.settings.routing_rules, rules,
            "the stale form did not drop the routing rules"
        );
        assert_eq!(e.settings.default_radio, Some(0));
        assert_eq!(e.settings.radios.len(), 3, "…nor the third radio");
        // …and routing still works after the save.
        e.aprs_tune(144.390)
            .expect("aprs_tune should succeed in this scenario");
        assert_eq!(e.settings.active_radio, ft991a);
    }

    #[test]
    fn dxped_work_mode_routing_still_reaches_the_right_radio() {
        // The dxpedWorkMode invariant: a DXpedition card's mode string picks the cockpit, and that
        // cockpit's section name is what `work_spot` routes on. Regression guard that adding the
        // mode dimension didn't break the HF-DX path (which is the whole point of a dxped chase).
        let (mut e, ic9700, _ft991a) = three_radio_engine();
        for (work_mode, band, dial, want) in [
            ("operate", "20m", 14.074, 0u32),   // FT8 on 20 m → the FTdx10
            ("phone", "15m", 21.250, 0),        // SSB on 15 m → the FTdx10
            ("cw", "40m", 7.030, 0),            // CW on 40 m → the FTdx10
            ("operate", "2m", 144.174, ic9700), // a VHF dxped → the IC-9700
        ] {
            e.set_active_radio(if want == 0 { ic9700 } else { 0 });
            e.work_spot(work_mode, dial, band);
            assert_eq!(
                e.settings.active_radio, want,
                "{work_mode} on {band} routed wrong"
            );
            assert_eq!(
                e.work_view.as_deref(),
                Some(work_mode),
                "cockpit hint intact"
            );
        }
    }

    #[test]
    fn aprs_tune_refuses_a_radio_that_cannot_reach_2m() {
        // ⭐ THE FTdx10 REPORT. HF/6 m only radio, single-radio station. Opening the APRS cockpit
        // auto-tunes 144.390; commanding a radio somewhere it physically cannot go has no upside,
        // so the capability check must come BEFORE the QSY — not after the rig refuses it.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.ensure_radio_profiles();
        e.settings.band = "20m".into();
        e.settings.dial_mhz = 14.250;
        // Hamlib's capability table for an FTdx10: 30 kHz – 60 MHz receive, nothing above.
        e.observe_rig_rx_ranges(Some(vec![(30_000, 60_000_000)]));
        assert_eq!(e.rig_covers_mhz(144.390), Some(false));
        assert_eq!(e.rig_covers_mhz(14.250), Some(true));

        let err = e.aprs_tune(144.390).expect_err("must refuse, not command");
        // The message is what the operator reads, so it must say the useful thing: RF needs a VHF
        // radio, AND that the internet feed still works (an HF-only station is not shut out).
        assert!(err.contains("144.390"), "names the frequency: {err}");
        assert!(err.contains("VHF"), "names what is needed: {err}");
        assert!(
            err.to_lowercase().contains("internet"),
            "offers the path that DOES work on this station: {err}"
        );
        // And nothing moved: no dial pollution, no band change, no FM context left armed.
        assert!(
            (e.settings.dial_mhz - 14.250).abs() < 1e-9,
            "the dial must not adopt a frequency we never commanded"
        );
        assert_eq!(e.settings.band, "20m", "nor the band");
        assert_ne!(e.rig_mode_effective(), "FM", "nor the APRS FM context");
    }

    #[test]
    fn aprs_tune_allows_2m_when_coverage_is_unknown_or_present() {
        // FAIL OPEN. A capability probe that wrongly says "no" is worse than the refused command it
        // exists to prevent, so only a positively-parsed range list may ever block a QSY.
        let mut unknown = Engine::new("KD9TAW", "EN52", 0);
        unknown.settings.ensure_radio_profiles();
        assert_eq!(unknown.rig_covers_mhz(144.390), None, "nothing probed yet");
        assert!(
            unknown.aprs_tune(144.390).is_ok(),
            "unknown coverage must not block the tune"
        );

        // An empty list is also unknown, not "covers nothing".
        let mut empty = Engine::new("KD9TAW", "EN52", 0);
        empty.settings.ensure_radio_profiles();
        empty.observe_rig_rx_ranges(Some(vec![]));
        assert_eq!(empty.rig_covers_mhz(144.390), None);
        assert!(empty.aprs_tune(144.390).is_ok());

        // A radio that DOES cover 2 m tunes normally.
        let mut vhf = Engine::new("KD9TAW", "EN52", 0);
        vhf.settings.ensure_radio_profiles();
        vhf.observe_rig_rx_ranges(Some(vec![(144_000_000, 148_000_000)]));
        assert!(vhf.aprs_tune(144.390).is_ok());
        assert!((vhf.settings.dial_mhz - 144.390).abs() < 1e-9);
        assert_eq!(vhf.rig_mode_effective(), "FM");
    }

    #[test]
    fn aprs_tune_judges_coverage_on_the_radio_that_would_actually_be_used() {
        // The dual-radio case must not be caught by the gate: the ACTIVE radio is the HF-only rig,
        // but set_frequency hands off to the 2 m radio, so that is the radio to judge. Blocking here
        // would break the operator's real 9700 setup — the very thing aprs_tune was built for.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.ensure_radio_profiles();
        e.settings.band = "20m".into();
        e.settings.dial_mhz = 14.250;
        let r1 = e.add_radio();
        e.set_radio_bands(r1, vec!["2m".into()]); // the VHF radio
        e.set_active_radio(0); // operating HF when APRS opens
                               // The ACTIVE (HF) radio's caps say no 2 m…
        e.observe_rig_rx_ranges(Some(vec![(30_000, 60_000_000)]));
        assert_eq!(e.rig_covers_mhz(144.390), Some(false));
        // …but a hand-off is available, so the tune must proceed and switch radios.
        e.aprs_tune(144.390).expect("must hand off, not refuse");
        assert_eq!(e.settings.active_radio, r1, "handed off to the 2 m radio");
        assert!((e.settings.dial_mhz - 144.390).abs() < 1e-9);
    }

    #[test]
    fn aprs_tune_commands_fm_simplex_and_clears_on_the_next_qsy() {
        // APRS is not a Phone/Digital operating section, so the rig would keep the prior section's
        // mode (DATA/USB) and a 2 m packet never demodulates. aprs_tune must force FM + simplex,
        // and that context must evaporate the instant the operator QSYs elsewhere.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        // Seed a Phone-section repeater shift the way an operator with a local machine would — APRS
        // must NOT inherit it (an offset would transmit the beacon off-frequency).
        e.settings.operating_mode = crate::settings::OperatingMode::Digital;
        e.settings.rptr_shift = "minus".into();
        e.settings.ctcss_tone_hz = 100.0;

        e.aprs_tune(144.390).unwrap();
        assert_eq!(e.settings.band, "2m");
        assert!((e.settings.dial_mhz - 144.390).abs() < 1e-9);
        assert_eq!(e.rig_mode_effective(), "FM", "APRS parks the rig in FM");
        assert_eq!(
            e.fm_repeater_config(),
            ("simplex".to_string(), 0, 0.0),
            "APRS forces simplex / no CTCSS, ignoring the Phone-section repeater config"
        );

        // Leak guard (review finding): a VFO-knob QSY off 2 m goes through observe_rig_freq, which
        // does NOT clear aprs_fm — the band-gate in rig_mode_effective must still stop FM following
        // onto HF. aprs_fm is deliberately left set here; only the band moves.
        e.observe_rig_freq(14_074_000);
        assert_eq!(e.settings.band, "20m");
        assert_ne!(
            e.rig_mode_effective(),
            "FM",
            "FM must not follow a knob-QSY off 2 m even with aprs_fm still set"
        );

        // A normal QSY away from APRS also drops the flag outright.
        e.aprs_tune(144.390).unwrap(); // re-arm
        e.set_frequency(14.074, "20m", "USB");
        assert_ne!(
            e.rig_mode_effective(),
            "FM",
            "FM does not follow the operator off APRS onto HF"
        );
    }

    #[test]
    fn update_radio_profile_edits_a_nonactive_radio_without_touching_the_active_one() {
        use crate::settings::{RadioProfile, RadioProfilePatch};
        // Build a patch from a profile, changing only the serial port — the field we assert on.
        let patch_with_port = |p: &RadioProfile, port: &str| RadioProfilePatch {
            ptt_method: p.ptt_method.clone(),
            rig_model: p.rig_model,
            rig_model_name: p.rig_model_name.clone(),
            serial_port: port.to_string(),
            ptt_serial_port: p.ptt_serial_port.clone(),
            baud: p.baud,
            rig_conn: p.rig_conn.clone(),
            rig_addr: p.rig_addr.clone(),
            rigctld_port: p.rigctld_port,
            icom_native_cat: p.icom_native_cat,
            data_modes_plain_ssb: p.data_modes_plain_ssb,
            audio_in: p.audio_in.clone(),
            audio_out: p.audio_out.clone(),
            tx_level: p.tx_level,
            rx_gain: p.rx_gain,
            rotator_model: p.rotator_model,
            rotator_port: p.rotator_port.clone(),
            rotator_baud: p.rotator_baud,
            rotator_host: p.rotator_host.clone(),
            rotctld_port: p.rotctld_port,
            native_scope: p.native_scope.clone(),
        };

        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.ensure_radio_profiles();
        e.settings.serial_port = "COM_R0".into();
        e.settings.sync_active_from_flat(); // radio 0 profile now carries COM_R0
        let r0 = e.settings.active_radio;
        let r1 = e.add_radio(); // radio 1 — now the ACTIVE radio
        e.settings.serial_port = "COM_R1".into();
        e.settings.sync_active_from_flat(); // radio 1 profile + flat mirror carry COM_R1
        assert_eq!(e.settings.active_radio, r1);

        // Edit the NON-active radio (r0) — operating on r1 the whole time.
        let r0_profile = e
            .settings
            .radios
            .iter()
            .find(|p| p.id == r0)
            .unwrap()
            .clone();
        e.update_radio_profile(r0, patch_with_port(&r0_profile, "COM_R0_EDITED"));

        // r0's profile changed…
        let r0_now = e.settings.radios.iter().find(|p| p.id == r0).unwrap();
        assert_eq!(r0_now.serial_port, "COM_R0_EDITED");
        // …the active radio is STILL r1, its profile untouched, and the flat mirror still points at it.
        assert_eq!(e.settings.active_radio, r1, "active radio unchanged");
        let r1_now = e.settings.radios.iter().find(|p| p.id == r1).unwrap();
        assert_eq!(
            r1_now.serial_port, "COM_R1",
            "active radio's profile untouched"
        );
        assert_eq!(
            e.settings.serial_port, "COM_R1",
            "flat mirror still mirrors the active radio"
        );
    }

    #[test]
    fn dual_radio_switch_mirrors_flat_and_snapshot_lists_both() {
        // Switching the active radio must mirror the new radio's CAT into the flat fields (so the
        // service loop swaps rigs) and repoint the snapshot's `radio` at it, while listing both.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.ensure_radio_profiles();
        e.settings.rig_model = 1042;
        e.settings.rig_model_name = "Yaesu FTDX10".into();
        e.settings.band = "20m".into();
        e.settings.dial_mhz = 14.074;
        e.settings.sync_active_from_flat();
        e.rename_radio(0, "FTDX10");

        // "+ Add radio" now SWITCHES to the new radio, so its config form edits the NEW radio (not the
        // previously-active one — the clobber bug). Configure it via the flat form while it's active.
        let r1 = e.add_radio();
        assert_eq!(
            e.settings.active_radio, r1,
            "add_radio switches to the new radio"
        );
        e.settings.rig_model = 3081;
        e.settings.rig_model_name = "Icom IC-9700".into();
        e.settings.band = "2m".into();
        e.settings.dial_mhz = 144.2;
        e.settings.sync_active_from_flat();
        e.rename_radio(r1, "IC-9700");

        // Switch to radio 0 (mirrors the FTDX10), make a live flat edit, then switch back to r1 — the
        // switch under test. The outgoing flat edit must FOLD into radio 0's profile, not be discarded.
        e.set_active_radio(0);
        assert_eq!(
            e.settings.rig_model, 1042,
            "flat mirrors the FTDX10 after switching to it"
        );
        e.settings.tx_level = 0.42;
        e.set_active_radio(r1);
        assert_eq!(e.settings.active_radio, r1, "active flipped to r1");
        assert_eq!(
            e.settings.rig_model, 3081,
            "flat CAT mirrors radio 1 after switch"
        );
        assert_eq!(e.settings.dial_mhz, 144.2, "restored radio 1's last dial");
        assert_eq!(
            e.settings
                .radios
                .iter()
                .find(|p| p.id == 0)
                .unwrap()
                .tx_level,
            0.42,
            "outgoing radio's live flat edit was folded into its profile before the switch"
        );

        let snap = e.snapshot();
        assert_eq!(snap.active_radio_id, r1);
        assert_eq!(snap.radios.len(), 2, "both radios listed");
        let active = snap.radios.iter().find(|r| r.id == r1).unwrap();
        assert!(active.is_active);
        assert_eq!(active.band, "2m", "active summary shows the live band");
        let idle = snap.radios.iter().find(|r| r.id == 0).unwrap();
        assert!(!idle.is_active, "radio 0 now idle");
        assert_eq!(idle.band, "20m", "idle radio shows its last-known band");
        assert!(
            idle.cat_ok.is_none(),
            "idle radio is not connected (active-only model)"
        );

        // A Save from a STALE form (loaded before the 2nd radio existed / before the switch) must
        // NOT drop radio 1, revert the active radio, or clobber radio 1's CAT with the form's flat
        // (radio-0) fields — the roster is live state, preserved across apply_settings.
        let mut stale = Settings::default();
        stale.ensure_radio_profiles(); // one profile, id 0, default CAT
        stale.active_radio = 0;
        stale.rig_model = 1042; // the stale form still describes radio 0
        e.apply_settings(stale);
        assert_eq!(
            e.settings.active_radio, r1,
            "stale Save preserved the live active radio"
        );
        assert_eq!(
            e.settings.radios.len(),
            2,
            "stale Save did not drop the 2nd radio"
        );
        let p1 = e.settings.radios.iter().find(|p| p.id == r1).unwrap();
        assert_eq!(
            p1.rig_model, 3081,
            "radio 1's CAT was not clobbered by the stale form"
        );
    }

    #[test]
    fn qrz_two_way_sync_adds_new_and_confirms_existing_without_award() {
        // Seed a local unconfirmed QSO (as if logged in Nexus).
        let mut e = Engine::new("K2DEF", "FN31", 0);
        let mut local = e.qso_record("W1AW".into(), None, Some(-5));
        local.band = "20m".into();
        local.mode = "FT8".into();
        local.when_unix = 1_700_000_000;
        e.log_qso(local);
        assert_eq!(e.get_log().len(), 1);

        // A QRZ FETCH body: the same QSO now QRZ-confirmed, PLUS a brand-new QSO the
        // operator logged elsewhere (e.g. a phone app) that Nexus has never seen.
        let day = "20231114"; // matches 1_700_000_000's UTC day
        let adif = format!(
            "<EOH>\n\
             <CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>{day}<TIME_ON:6>223000\
             <APP_QRZLOG_STATUS:1>C<EOR>\n\
             <CALL:5>K5NEW<BAND:3>40m<MODE:2>CW<QSO_DATE:8>{day}<TIME_ON:6>010000\
             <APP_QRZLOG_STATUS:1>C<EOR>\n"
        );
        let (added, summary) = e.merge_qrz_report(&adif);

        // The new QSO was pulled down; the existing one was reconciled (not re-added).
        assert_eq!(added, 1, "only K5NEW is new");
        assert_eq!(e.get_log().len(), 2, "log grew by exactly one");

        // The existing QSO is now confirmed by QRZ — but NOT award-eligible.
        let w1aw = e.get_log().into_iter().find(|r| r.call == "W1AW").unwrap();
        assert!(w1aw.confirmed, "QRZ match confirms the contact");
        assert!(!w1aw.award_confirmed, "QRZ confirmation is NOT award-grade");
        assert!(w1aw.qsl_rcvd.qrz && !w1aw.qsl_rcvd.card);
        assert_eq!(summary.newly_confirmed, 0, "no award-grade upgrades");
        assert!(
            summary.newly_confirmed_any >= 1,
            "confirmed by some channel"
        );

        // The imported new QSO also carries the QRZ confirmation, non-award.
        let k5new = e.get_log().into_iter().find(|r| r.call == "K5NEW").unwrap();
        assert!(k5new.confirmed && !k5new.award_confirmed && k5new.qsl_rcvd.qrz);

        // Idempotent: a second identical sync adds nothing and re-confirms nothing new.
        let (added2, summary2) = e.merge_qrz_report(&adif);
        assert_eq!(added2, 0, "second sync adds no duplicates");
        assert_eq!(e.get_log().len(), 2);
        assert_eq!(summary2.newly_confirmed_any, 0, "already confirmed");
    }

    /// Capture the engine's transmitted FRAME TEXT per slot — the TX-schedule probe used by
    /// the FT8/FT4 golden below. Reads the own-TX rows the snapshot appends (mine == true),
    /// pairing each newly-appeared row with the slot whose poll_tx produced it.
    fn tx_schedule_for(tier: Tier) -> Vec<(u64, String)> {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_tier(tier);
        e.set_tx_cycle_auto(false); // deterministic: transmit on even slots
                                    // Hear K2DEF calling CQ, then answer (the WSJT-X double-click S&P entry).
        e.ingest_decodes_for_test(&[dec_snr("CQ K2DEF FN31", -5)], 1);
        e.call_station("K2DEF");
        let mut sched = Vec::new();
        let mut seen = 0usize;
        for slot in 2..=14u64 {
            let _ = e.poll_tx(slot);
            let mine: Vec<String> = e
                .snapshot()
                .recent_decodes
                .iter()
                .filter(|d| d.mine)
                .map(|d| d.message.clone())
                .collect();
            for m in mine.iter().skip(seen) {
                sched.push((slot, m.clone()));
            }
            seen = mine.len();
            // Scripted peer: NO reply for two of our overs (slots 2 + 4 — pins the
            // uncapped WSJT-X repeat), then the report, then RR73.
            if slot == 5 {
                e.ingest_decodes_for_test(&[dec_snr("W9XYZ K2DEF -10", -8)], slot);
            }
            if slot == 9 {
                e.ingest_decodes_for_test(&[dec_snr("W9XYZ K2DEF RR73", -8)], slot);
            }
        }
        sched
    }

    /// GOLDEN: the FT8/FT4 transmit schedule is WSJT-X behavior and MUST NOT change.
    /// Pinned from the tree at 2026-07-23 (pre-Tempo-cadence-rework): the S&P call repeats
    /// every own-parity slot while unanswered (uncapped — stock WSJT-X), the R-report repeats
    /// until RR73, then the single 73. If the Tempo cadence work (or anything else) perturbs
    /// these (slot, frame) pairs, THAT is the bug — do not re-pin without operator sign-off.
    #[test]
    fn ft8_ft4_tx_schedule_is_wsjtx_golden() {
        let expect: Vec<(u64, &str)> = vec![
            (3, "K2DEF W9XYZ EN61"),
            (5, "K2DEF W9XYZ EN61"),
            (7, "K2DEF W9XYZ R-08"),
            (9, "K2DEF W9XYZ R-08"),
            (11, "K2DEF W9XYZ 73"),
        ];
        for tier in [Tier::Ft8, Tier::Ft4] {
            let got = tx_schedule_for(tier);
            let got_ref: Vec<(u64, &str)> = got.iter().map(|(s, t)| (*s, t.as_str())).collect();
            assert_eq!(
                got_ref, expect,
                "{tier:?} TX schedule drifted from the WSJT-X golden"
            );
        }
    }

    /// Tempo S&P step cap: on a chat tier an unanswered in-QSO step stops after 6 overs
    /// and returns cleanly to monitoring (no stuck pending, no silent hang). The same
    /// script on FT8 keeps repeating — pinned by the TX-schedule golden.
    #[test]
    fn tempo_qso_step_caps_and_returns_to_monitoring() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_tier(Tier::TempoFast);
        e.set_tx_cycle_auto(false);
        e.ingest_decodes_for_test(&[dec_snr("CQ K2DEF FN31", -5)], 1);
        e.call_station("K2DEF"); // we call → AwaitReport; K2DEF never answers
        let mut sent = 0;
        for slot in 2..40u64 {
            if !e.poll_tx(slot).is_empty() {
                sent += 1;
            }
            // The cap check runs on the RX path (observe tail) — feed an unrelated
            // decode each slot so it evaluates, like real RX traffic would.
            e.ingest_decodes_for_test(&[dec_snr("CQ N0ABC EN52", -12)], slot);
        }
        assert!(
            (1..=6).contains(&sent),
            "chat-tier step capped at 6 overs, got {sent}"
        );
        let s = e.snapshot();
        let q = s
            .qso
            .expect("Mode::Qso projects a status even when monitoring");
        assert_eq!(q.dxcall, None, "the abandoned call is cleared");
        assert_eq!(q.tx_now, None, "nothing pending — no stuck step");
        assert!(!q.running, "not a run");
        assert!(!s.radio.transmitting);
        // And it STAYS silent — no zombie re-transmit on later slots.
        for slot in 40..48u64 {
            assert!(e.poll_tx(slot).is_empty(), "monitoring transmits nothing");
        }
    }

    /// Yield-to-RX: after a directed burst drains, the chat arm spends two own-parity
    /// slots LISTENING before any further traffic (CQ resume / beacon included) — the
    /// alternate-overs chat feel. TX parity itself is never touched.
    #[test]
    fn chat_burst_yields_to_rx_after_draining() {
        let mut e = Engine::new("W9XYZ", "EN61", 0); // TX on even slots
        e.set_mode("chat").expect("chat");
        e.set_tier(Tier::TempoFast);
        e.set_tx_enabled(true);
        e.set_tx_cycle_auto(false);
        e.ingest_decodes_for_test(&[dec_snr("CQ K2DEF FN31", -5)], 1);
        e.send_message("K2DEF", "HI");
        // Drain the burst: own-parity (even) slots transmit until the queue empties.
        let mut sent = 0;
        let mut slot = 2u64;
        while sent == 0 || !e.poll_tx(slot).is_empty() {
            if !e.poll_tx(slot).is_empty() {
                sent += 1;
            }
            if sent > 0 && e.poll_tx(slot + 2).is_empty() {
                slot += 2;
                break;
            }
            slot += 2;
            assert!(slot < 60, "burst never drained");
        }
        assert!(sent >= 1, "the burst transmitted");
        // The two slots after the drain stay SILENT even with the beacon armed.
        e.set_beacon(true);
        assert!(e.poll_tx(slot + 2).is_empty(), "yield slot 1 listens");
    }

    /// The chat CQ run is BUDGETED: after N consecutive unanswered calls it stops
    /// (snapshot flips to "off") instead of calling forever.
    #[test]
    fn chat_cq_run_stops_at_its_budget() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_mode("chat").expect("chat");
        e.set_tier(Tier::TempoFast);
        e.set_tx_cycle_auto(false);
        e.set_chat_cq(true).expect("cq run starts");
        let mut transmissions = 0;
        for slot in (2..120u64).step_by(2) {
            if !e.poll_tx(slot).is_empty() {
                transmissions += 1;
            }
        }
        // 1 immediate call (set_chat_cq → call_cq broadcast) + at most the budget (10).
        assert!(
            (2..=11).contains(&transmissions),
            "bounded CQ run, got {transmissions}"
        );
        assert_eq!(e.chat_cq_state(), "off", "run stopped at the budget");
    }

    /// TempoDeep delivery — the widened chat gate. The old `tier == TempoFast` gate left a
    /// TempoDeep chat message resending unbounded and permanently undeliverable: an inbound
    /// RR73 ACK was ignored, so "Delivered" never lit. Deep shares the whole Chat path.
    #[test]
    fn tempodeep_chat_message_can_be_marked_delivered() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_mode("chat").expect("chat mode");
        e.set_tier(Tier::TempoDeep);
        // Hear the peer (roster presence gates the store's release) then message them.
        e.ingest_decodes_for_test(&[dec_snr("CQ K2DEF FN31", -5)], 1);
        e.send_message("K2DEF", "HELLO DEEP");
        assert!(e.app_pending_count_for_test() > 0, "message queued");
        // Their id-bearing ACK arrives → the queue entry is marked delivered.
        e.ingest_decodes_for_test(&[dec_snr("W9XYZ K2DEF RR73", -8)], 3);
        assert_eq!(
            e.app_pending_count_for_test(),
            0,
            "the RR73 marked the Deep chat message delivered (old gate ignored it)"
        );
    }

    /// Invariant pin: ENTERING Chat mode via set_mode reconciles an FT tier down to
    /// TempoFast (the chat-capable tier) — the one direction the engine actively snaps.
    /// The reverse (tier set to FT8 while resting in Chat) is legitimate (boot state) and
    /// stays inert via Tier::is_chat() gating on every chat-cadence behavior.
    #[test]
    fn entering_chat_mode_snaps_an_ft_tier_to_tempofast() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_tier(Tier::Ft8);
        e.set_mode("chat").expect("chat mode");
        assert_eq!(
            e.snapshot().link.tier,
            Tier::TempoFast,
            "set_mode(chat) snaps Ft8 → TempoFast"
        );
        e.set_tier(Tier::Ft4);
        e.set_mode("chat").expect("chat mode");
        assert_eq!(
            e.snapshot().link.tier,
            Tier::TempoFast,
            "set_mode(chat) snaps Ft4 → TempoFast"
        );
        // TempoDeep is chat-capable — entering Chat must NOT clobber it to TempoFast.
        e.set_tier(Tier::TempoDeep);
        e.set_mode("chat").expect("chat mode");
        assert_eq!(
            e.snapshot().link.tier,
            Tier::TempoDeep,
            "a chat tier survives set_mode(chat)"
        );
    }

    /// GOLDEN companion: Chat mode at an FT8 tier (the engine's BOOT state, also recreated by
    /// apply_settings/set_tier) transmits NOTHING with no queued traffic — armed or not. The
    /// Tempo chat-cadence rework must keep this exact silence: new chat behaviors must be
    /// unreachable (or inert) when the tier is Ft8/Ft4.
    #[test]
    fn chat_mode_at_ft8_tier_stays_silent() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_tier(Tier::Ft8); // boot mode is Chat; tier explicitly FT8 (the probe)
        e.set_tx_enabled(true);
        for slot in 0..8u64 {
            assert!(
                e.poll_tx(slot).is_empty(),
                "Chat+Ft8 with nothing queued transmitted on slot {slot}"
            );
        }
        assert!(!e.snapshot().recent_decodes.iter().any(|d| d.mine));
    }

    /// One decoded packet, ready to push. `raw` doubles as the payload text so two calls with
    /// different `raw` are genuinely different packets.
    fn aprs_pkt(call: &str, raw: &str, at: i64, kind: AprsSource) -> AprsHeard {
        use tempo_core::aprs::{Address, AprsPacket, Frame};
        let frame = Frame::ui(
            Address::new("APRS", 0),
            Address::parse(call).expect("test callsign"),
            vec![Address::new("WIDE1", 1)],
            format!("!4903.50N/07201.75W-{raw}").as_bytes(),
        );
        AprsHeard::from_packet(&AprsPacket::from_frame(&frame), at, kind, raw.to_string())
    }

    /// ⭐ THE FLASHING BUG, measured. The packet ring is capped by COUNT with no age expiry, so a
    /// busy APRS-IS feed evicts stations that are still perfectly active — they vanish from the map
    /// and reappear on their next beacon. This test pins the mechanism.
    #[test]
    fn the_packet_ring_evicts_stations_that_are_still_active() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // A modest filtered feed: 200 stations in range, each beaconing once. That is already
        // under the 300-packet cap, so nothing is lost yet.
        for i in 0..200 {
            e.push_aprs_heard(aprs_pkt(
                &format!("W9AA-{}", i % 16),
                &format!("b{i}"),
                1000 + i,
                AprsSource::Inet,
            ));
        }
        // ...then the feed keeps running for another 300 packets, which is 2-5 MINUTES of real
        // traffic at 1-2 packets/second.
        for i in 200..500 {
            e.push_aprs_heard(aprs_pkt(
                "W9NEW-1",
                &format!("b{i}"),
                1000 + i,
                AprsSource::Inet,
            ));
        }
        let heard = e.aprs_heard();
        assert_eq!(heard.len(), APRS_HEARD_CAP, "the ring is full");
        // The station that beaconed at t=1000 is GONE, though only ~8 minutes of feed have passed
        // and it is still transmitting every 10 minutes like every fixed station does.
        assert!(
            !heard.iter().any(|h| h.text == "b0"),
            "the earliest beacon has been evicted purely by packet COUNT — this is the flash"
        );
    }

    /// ⭐ THE FIX for the test above: the same churn, but the STATION store keeps every station
    /// that is still active. This is the regression guard for the flashing bug.
    #[test]
    fn the_station_store_survives_the_ring_churn_that_evicted_them() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // 200 stations beacon once each...
        for i in 0..200 {
            e.push_aprs_heard(aprs_pkt(
                &format!("W{i:04}"),
                &format!("b{i}"),
                1000 + i,
                AprsSource::Inet,
            ));
        }
        // ...then 400 more packets pour in from one chatty station, cycling the packet ring twice.
        for i in 200..600 {
            e.push_aprs_heard(aprs_pkt(
                "W9NEW-1",
                &format!("c{i}"),
                1000 + i,
                AprsSource::Inet,
            ));
        }
        // The packet log has churned, exactly as before — that is what a log does.
        assert_eq!(e.aprs_heard().len(), APRS_HEARD_CAP);
        // But every one of the 200 stations is still on the map, because minutes, not packets,
        // decide whether a station is still there.
        let view = e.aprs_stations(1600);
        assert_eq!(view.stations.len(), 201, "200 beaconers + the chatty one");
        assert!(view.stations.iter().any(|s| s.call == "W0000"));
        assert!(view.stations.iter().any(|s| s.call == "W0199"));
    }

    #[test]
    fn a_station_is_dropped_only_once_it_has_been_silent_past_the_window() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.push_aprs_heard(aprs_pkt("W9AA-1", "beacon", 1000, AprsSource::Rf));
        let ttl = i64::from(e.settings().aprs_station_ttl_min) * 60;
        // A fixed station on a THIRTY-minute cycle: still shown while it is merely slow. This is
        // the case a shorter window would blink off, which is the whole point of an hour.
        assert_eq!(e.aprs_stations(1000 + 30 * 60).stations.len(), 1);
        assert_eq!(
            e.aprs_stations(1000 + ttl).stations.len(),
            1,
            "at the boundary"
        );
        assert!(
            e.aprs_stations(1000 + ttl + 1).stations.is_empty(),
            "and gone once genuinely stale"
        );
    }

    #[test]
    fn the_fade_threshold_is_derived_from_the_window_so_they_cannot_contradict() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let v = e.aprs_stations(0);
        assert_eq!(v.ttl_min, 60, "the default hour");
        assert_eq!(
            v.fade_after_min, 20,
            "a third of it — past the slowest mobile rhythm"
        );
        // An operator who shortens the window gets a proportionally earlier fade, never one that
        // outlives the retention it is supposed to warn about.
        let mut s = e.settings().clone();
        s.aprs_station_ttl_min = 15;
        e.apply_settings(s);
        let v = e.aprs_stations(0);
        assert_eq!(v.ttl_min, 15);
        assert!(v.fade_after_min < v.ttl_min);
    }

    #[test]
    fn a_weather_station_carries_its_readings_onto_the_station_record() {
        use tempo_core::aprs::AprsPacket;
        // Real captured CWOP packet. The readings used to sit unparsed in the comment.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let pkt = AprsPacket::from_tnc2(
            b"WB8HRV>APRS,WIDE1-1:@291813z3913.47N/08424.67W_220/004g011t085r000p000P000h68b10156.DsVP",
        )
        .unwrap();
        e.push_aprs_heard(AprsHeard::from_packet(
            &pkt,
            1000,
            AprsSource::Rf,
            String::new(),
        ));
        let wx = e.aprs_stations(1000).stations[0]
            .wx
            .expect("weather parsed");
        assert_eq!(wx.temp_f, Some(85));
        assert_eq!(wx.wind_dir_deg, Some(220));
        assert_eq!(wx.gust_mph, Some(11));
        assert_eq!(wx.humidity_pct, Some(68));
        assert_eq!(wx.pressure_tenth_hpa, Some(10156));
    }

    #[test]
    fn a_mobile_station_never_gains_phantom_weather() {
        use tempo_core::aprs::AprsPacket;
        // Its `098/065` course-and-speed slot is byte-identical to a wind slot.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let pkt = AprsPacket::from_tnc2(
            b"K9LGE-5>APDR16,WIDE1-1:=4153.96N/08857.08W$098/065/146.520MHz/A=000829 Blue Dodge",
        )
        .unwrap();
        e.push_aprs_heard(AprsHeard::from_packet(
            &pkt,
            1000,
            AprsSource::Rf,
            String::new(),
        ));
        assert!(e.aprs_stations(1000).stations[0].wx.is_none());
    }

    #[test]
    fn a_weather_stations_readings_survive_a_later_status_packet() {
        use tempo_core::aprs::AprsPacket;
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let wxp = AprsPacket::from_tnc2(
            b"WB8HRV>APRS,WIDE1-1:@291813z3913.47N/08424.67W_220/004g011t085r000p000P000h68b10156",
        )
        .unwrap();
        e.push_aprs_heard(AprsHeard::from_packet(
            &wxp,
            1000,
            AprsSource::Rf,
            String::new(),
        ));
        let status = AprsPacket::from_tnc2(b"WB8HRV>APRS,WIDE1-1:>station online").unwrap();
        e.push_aprs_heard(AprsHeard::from_packet(
            &status,
            1010,
            AprsSource::Rf,
            String::new(),
        ));
        let st = &e.aprs_stations(1010).stations[0];
        assert!(st.wx.is_some(), "readings are sticky, like the position");
        assert_eq!(st.wx.unwrap().temp_f, Some(85));
    }

    #[test]
    fn a_packet_without_a_position_never_erases_the_one_we_have() {
        // The other face of the same bug: a moving station interleaves positions with status and
        // message packets, and "latest packet wins" would blank its coordinates every time one
        // arrived — dropping it off the map between beacons.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.push_aprs_heard(aprs_pkt("W9AA-9", "rolling", 1000, AprsSource::Rf));
        let mut status = aprs_pkt("W9AA-9", "just a status", 1010, AprsSource::Rf);
        status.lat = None;
        status.lon = None;
        status.symbol_table = ' ';
        status.symbol_code = ' ';
        status.kind = "status";
        e.push_aprs_heard(status);
        let st = &e.aprs_stations(1010).stations[0];
        assert!(
            st.lat.is_some() && st.lon.is_some(),
            "the position is sticky"
        );
        assert_eq!(st.symbol_table, '/', "and so is the symbol");
        assert_eq!(st.text, "just a status", "but the text is the latest");
        assert_eq!(st.packets, 2);
    }

    #[test]
    fn a_station_source_is_derived_from_when_each_channel_last_carried_it() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.push_aprs_heard(aprs_pkt("W9AA-1", "p1", 1000, AprsSource::Inet));
        assert_eq!(
            e.aprs_stations(1000).stations[0].source_kind,
            AprsSource::Inet
        );
        // Our own antenna then hears it: the tag upgrades, and the evidence for it is visible.
        e.push_aprs_heard(aprs_pkt("W9AA-1", "p2", 1100, AprsSource::Rf));
        let st = &e.aprs_stations(1100).stations[0];
        assert_eq!(st.source_kind, AprsSource::Both);
        assert_eq!(st.last_rf_unix, Some(1100));
        assert_eq!(st.last_inet_unix, Some(1000));
        // A later internet-only packet must NOT demote it — we did hear it ourselves.
        e.push_aprs_heard(aprs_pkt("W9AA-1", "p3", 1200, AprsSource::Inet));
        assert_eq!(
            e.aprs_stations(1200).stations[0].source_kind,
            AprsSource::Both
        );
    }

    #[test]
    fn the_store_is_capped_by_station_and_evicts_the_longest_unheard() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // Every station within the age window, so only the cap can evict.
        for i in 0..(APRS_STATION_CAP + 100) {
            e.push_aprs_heard(aprs_pkt(
                &format!("W{i:04}"),
                "b",
                100_000 + i as i64,
                AprsSource::Inet,
            ));
        }
        let view = e.aprs_stations(100_000 + APRS_STATION_CAP as i64 + 100);
        assert!(view.stations.len() <= APRS_STATION_CAP, "bounded");
        // The newest survived; the very first, heard longest ago, did not.
        assert!(view
            .stations
            .iter()
            .any(|s| s.call == format!("W{:04}", APRS_STATION_CAP + 99)));
        assert!(!view.stations.iter().any(|s| s.call == "W0000"));
    }

    #[test]
    fn stations_come_back_newest_heard_first() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.push_aprs_heard(aprs_pkt("W9OLD-1", "a", 1000, AprsSource::Rf));
        e.push_aprs_heard(aprs_pkt("W9MID-1", "b", 1100, AprsSource::Rf));
        e.push_aprs_heard(aprs_pkt("W9NEW-1", "c", 1200, AprsSource::Rf));
        let calls: Vec<String> = e
            .aprs_stations(1200)
            .stations
            .iter()
            .map(|s| s.call.clone())
            .collect();
        assert_eq!(calls, vec!["W9NEW-1", "W9MID-1", "W9OLD-1"]);
    }

    #[test]
    fn a_station_that_keeps_beaconing_is_never_dropped_however_long_the_feed_runs() {
        // The operator-visible promise: an active station does not blink, ever.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let mut t = 1000i64;
        for round in 0..40 {
            // Our station beacons every 10 minutes, like a normal fixed station.
            e.push_aprs_heard(aprs_pkt(
                "W9ME-1",
                &format!("beacon {round}"),
                t,
                AprsSource::Rf,
            ));
            // ...while the feed pours 200 packets from other stations in between.
            for i in 0..200 {
                e.push_aprs_heard(aprs_pkt(&format!("X{i:04}"), "x", t + i, AprsSource::Inet));
            }
            t += 600;
            assert!(
                e.aprs_stations(t)
                    .stations
                    .iter()
                    .any(|s| s.call == "W9ME-1"),
                "round {round}: the station must never vanish"
            );
        }
    }

    #[test]
    fn the_same_packet_heard_both_ways_merges_instead_of_listing_twice() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.push_aprs_heard(aprs_pkt("N0CALL-9", "home", 1000, AprsSource::Rf));
        e.push_aprs_heard(aprs_pkt("N0CALL-9", "home", 1005, AprsSource::Inet));
        let heard = e.aprs_heard();
        assert_eq!(heard.len(), 1, "one packet, not two rows");
        assert_eq!(heard[0].source_kind, AprsSource::Both);
        assert_eq!(
            heard[0].at_unix, 1000,
            "the FIRST sighting is kept — when OUR antenna heard it"
        );
    }

    #[test]
    fn a_dupe_outside_the_merge_window_is_a_genuinely_new_packet() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.push_aprs_heard(aprs_pkt("N0CALL-9", "home", 1000, AprsSource::Rf));
        e.push_aprs_heard(aprs_pkt(
            "N0CALL-9",
            "home",
            1000 + APRS_DUPE_MERGE_SECS + 1,
            AprsSource::Rf,
        ));
        assert_eq!(e.aprs_heard().len(), 2, "a later beacon is not a duplicate");
    }

    #[test]
    fn different_stations_and_different_payloads_never_merge() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.push_aprs_heard(aprs_pkt("N0CALL-9", "home", 1000, AprsSource::Rf));
        e.push_aprs_heard(aprs_pkt("W9XYZ-1", "home", 1001, AprsSource::Inet));
        e.push_aprs_heard(aprs_pkt("N0CALL-9", "moving", 1002, AprsSource::Inet));
        assert_eq!(e.aprs_heard().len(), 3);
    }

    #[test]
    fn source_merge_never_downgrades_an_rf_sighting() {
        assert_eq!(AprsSource::Rf.merged(AprsSource::Inet), AprsSource::Both);
        assert_eq!(AprsSource::Inet.merged(AprsSource::Rf), AprsSource::Both);
        assert_eq!(AprsSource::Both.merged(AprsSource::Inet), AprsSource::Both);
        assert_eq!(AprsSource::Rf.merged(AprsSource::Rf), AprsSource::Rf);
    }

    #[test]
    fn the_uplink_queue_stays_empty_while_the_igate_is_off() {
        // The default. An operator who never enables the iGate must never queue a byte for it.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert!(!e.settings().aprs_is_uplink);
        e.push_aprs_heard(aprs_pkt("N0CALL-9", "home", 1000, AprsSource::Rf));
        assert!(e.take_aprs_uplink().is_empty());
    }

    #[test]
    fn only_rf_heard_packets_are_ever_offered_to_the_igate() {
        // THE gating rule, enforced structurally: an internet packet cannot reach the queue, so
        // no downstream code has to remember not to send it back where it came from.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let mut s = e.settings().clone();
        s.aprs_is_uplink = true;
        e.apply_settings(s);
        e.push_aprs_heard(aprs_pkt("N0CALL-9", "from-the-air", 1000, AprsSource::Rf));
        e.push_aprs_heard(aprs_pkt(
            "W9XYZ-1",
            "from-the-internet",
            1001,
            AprsSource::Inet,
        ));
        assert_eq!(e.take_aprs_uplink(), vec!["from-the-air".to_string()]);
        assert!(
            e.take_aprs_uplink().is_empty(),
            "draining consumes the queue"
        );
    }

    #[test]
    fn a_station_the_internet_showed_first_still_gates_once_we_hear_it_ourselves() {
        // The merge must not swallow the RF sighting's contribution: our antenna genuinely heard
        // this packet, which is exactly what an iGate exists to report.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let mut s = e.settings().clone();
        s.aprs_is_uplink = true;
        e.apply_settings(s);
        e.push_aprs_heard(aprs_pkt("N0CALL-9", "beacon", 1000, AprsSource::Inet));
        assert!(
            e.take_aprs_uplink().is_empty(),
            "the internet copy is not ours to gate"
        );
        e.push_aprs_heard(aprs_pkt("N0CALL-9", "beacon", 1002, AprsSource::Rf));
        assert_eq!(e.take_aprs_uplink(), vec!["beacon".to_string()]);
        assert_eq!(e.aprs_heard()[0].source_kind, AprsSource::Both);
    }

    #[test]
    fn the_uplink_queue_is_bounded_when_the_feed_is_down() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let mut s = e.settings().clone();
        s.aprs_is_uplink = true;
        e.apply_settings(s);
        for i in 0..(APRS_UPLINK_QUEUE_CAP + 50) {
            e.push_aprs_heard(aprs_pkt(
                "N0CALL-9",
                &format!("p{i}"),
                1000 + i as i64,
                AprsSource::Rf,
            ));
        }
        let queued = e.take_aprs_uplink();
        assert_eq!(queued.len(), APRS_UPLINK_QUEUE_CAP);
        assert_eq!(queued[0], "p50", "the oldest are dropped, not the newest");
    }

    #[test]
    fn the_internet_feed_runs_independently_of_the_rf_arm() {
        // The decision, asserted: an internet packet lands on the map with the RF decoder OFF.
        // Arming is about the receiver; the feed costs no RF resource and can key nothing. This
        // is also the diagnostic — internet stations plotting on a silent RF chip prove the fault
        // is in the radio path.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert_eq!(e.aprs_arm_source(), AprsArm::Off);
        e.push_aprs_heard(aprs_pkt("W9XYZ-1", "internet", 1000, AprsSource::Inet));
        assert_eq!(e.aprs_heard().len(), 1);
        assert!(!e.aprs_armed(), "and the decoder is still off");
    }

    #[test]
    fn aprs_is_status_reports_the_operator_switches_from_settings() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let mut s = e.settings().clone();
        s.aprs_is_enabled = true;
        s.aprs_is_uplink = true;
        e.apply_settings(s);
        let st = e.aprs_is_status();
        assert!(st.enabled && st.uplink_enabled);
        assert!(!st.connected, "no socket in a headless test");
    }

    #[test]
    fn aprs_heard_flattens_position_and_mice_packets() {
        use tempo_core::aprs::{Address, AprsPacket, Frame};
        // A position report.
        let pos = Frame::ui(
            Address::new("APRS", 0),
            Address::new("N0CALL", 9),
            vec![Address::new("WIDE1", 1)],
            b"!4903.50N/07201.75W-Home",
        );
        let h = AprsHeard::from_packet(
            &AprsPacket::from_frame(&pos),
            1000,
            AprsSource::Rf,
            String::from_utf8_lossy(&pos.to_tnc2()).into_owned(),
        );
        assert_eq!(h.source, "N0CALL-9");
        assert_eq!(h.source_kind, AprsSource::Rf);
        assert_eq!(h.raw, "N0CALL-9>APRS,WIDE1-1:!4903.50N/07201.75W-Home");
        assert_eq!(h.kind, "position");
        assert_eq!(h.text, "Home");
        assert_eq!(h.path, vec!["WIDE1-1".to_string()]);
        assert!(h.lat.unwrap() > 49.0 && h.lon.unwrap() < 0.0);
        assert_eq!(h.at_unix, 1000);
        // A Mic-E report (the hand-worked vector) — speed/course flatten through.
        let mice = Frame::ui(
            Address::new("SSRUVT", 0),
            Address::new("N0CALL", 7),
            vec![],
            &[0x60, b'(', b'#', b'H', 0x1e, 0x1e, b'O', b'>', b'/'],
        );
        let hm = AprsHeard::from_packet(
            &AprsPacket::from_frame(&mice),
            2000,
            AprsSource::Rf,
            String::new(),
        );
        assert_eq!(hm.kind, "mice");
        assert_eq!(hm.speed_knots, Some(20));
        assert_eq!(hm.course_deg, Some(251));
    }

    #[test]
    fn aprs_beacon_is_refused_when_tx_is_off() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        // Default is TX-disarmed → the beacon gate refuses with a clear reason, never a silent
        // queue (nothing can key without an explicit TX-enable).
        let err = e
            .aprs_beacon(41.9, -87.6, '/', '>', "test", &[])
            .unwrap_err();
        assert!(
            err.contains("TX is off"),
            "expected a TX-off refusal, got: {err}"
        );
        assert!(e.poll_aprs_tx().is_none(), "nothing queued");
    }

    // ---- Auto-arm is RX-ONLY: it must never confer the auto-ack ----
    //
    // Entering the APRS view arms the decoder so the operator does not face a dead screen. That
    // convenience must not also arm an UNATTENDED TRANSMITTER. An auto-ack keys the radio with
    // nobody asking, so it stays behind TWO independent operator acts: arming Monitor by hand AND
    // enabling TX. Auto-arm supplies neither. These pin that invariant — weakening any of them
    // means the app can transmit because someone navigated to a screen.

    /// Arm explicitly and enable TX — the only configuration in which an ack may key.
    fn armed_engine(arm: AprsArm) -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        match arm {
            AprsArm::Auto => {
                assert!(e.aprs_auto_arm(), "auto-arm should take from Off");
            }
            other => e.set_aprs_arm(other),
        }
        e.set_tx_enabled(true);
        e
    }

    #[test]
    fn auto_armed_never_auto_acks_even_with_tx_enabled() {
        let mut e = armed_engine(AprsArm::Auto);
        assert!(
            e.aprs_armed(),
            "the decoder IS running — this is RX-only, not off"
        );
        assert_eq!(e.aprs_arm_source(), AprsArm::Auto);
        e.aprs_auto_ack("N0CALL-7", "W9XYZ", "042");
        assert!(
            e.poll_aprs_tx().is_none(),
            "an auto-armed decoder must never queue an unattended ack"
        );
    }

    #[test]
    fn explicitly_armed_with_tx_on_does_auto_ack() {
        // The other side of the gate: with both operator acts present, the ack works as before.
        let mut e = armed_engine(AprsArm::Explicit);
        e.aprs_auto_ack("N0CALL-7", "W9XYZ", "042");
        assert!(
            e.poll_aprs_tx().is_some(),
            "explicit arm + TX enabled is the configuration that acks"
        );
    }

    #[test]
    fn explicit_arm_still_needs_tx_enabled_to_ack() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit); // TX left off
        e.aprs_auto_ack("N0CALL-7", "W9XYZ", "042");
        assert!(
            e.poll_aprs_tx().is_none(),
            "both acts are required, not either"
        );
    }

    #[test]
    fn auto_arm_only_upgrades_from_off_and_never_downgrades() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        assert!(
            !e.aprs_auto_arm(),
            "auto-arm must not touch an explicit arm"
        );
        assert_eq!(
            e.aprs_arm_source(),
            AprsArm::Explicit,
            "re-entering the view must not demote an explicit arm to RX-only"
        );
    }

    #[test]
    fn an_operator_who_stopped_the_decoder_is_not_overridden_on_re_entry() {
        // Explicit disarm is a decision, not a transient state. Re-entering the APRS view must not
        // quietly start the decoder up again behind them.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.set_aprs_arm(AprsArm::Off);
        assert!(!e.aprs_auto_arm(), "no re-arm after an explicit stop");
        assert!(!e.aprs_armed());
        // But the operator may always start it again by hand.
        e.set_aprs_arm(AprsArm::Explicit);
        assert_eq!(e.aprs_arm_source(), AprsArm::Explicit);
    }

    #[test]
    fn health_reports_how_the_decoder_was_armed() {
        // The UI cannot tell the operator that auto-armed is RX-only unless it can SEE the source.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert_eq!(e.aprs_health().arm, AprsArm::Off);
        e.aprs_auto_arm();
        assert_eq!(e.aprs_health().arm, AprsArm::Auto);
        e.set_aprs_arm(AprsArm::Explicit);
        assert_eq!(e.aprs_health().arm, AprsArm::Explicit);
    }

    // ---- A squelched codec is NOT a dead capture device (on-air report, 0.21.1) ----
    //
    // An FM rig's USB codec streams a continuous run of digital ZEROS while the squelch is closed.
    // Samples keep arriving; only the level is zero. The health struct has to keep those two facts
    // separate, because the UI turns them into opposite messages: "your audio device is broken"
    // versus "the channel is idle between packets".

    #[test]
    fn a_zero_filled_drain_still_counts_as_audio_arriving() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.note_aprs_rx(1200, 0.0, 0, 0, 0, 5_000);
        let h = e.aprs_health();
        assert_eq!(
            h.last_audio_unix,
            Some(5_000),
            "samples arrived — the capture device is alive and must not be blamed"
        );
        assert_eq!(h.audio_peak, 0.0, "and the level honestly reads silent");
    }

    #[test]
    fn a_drain_with_no_samples_leaves_the_arrival_stamp_alone() {
        // The genuinely-dead-capture case, and also the ordinary empty drain: the decode thread
        // out-polls the radio loop, so empty drains are routine and must not fake an arrival.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        for i in 0..20 {
            e.note_aprs_rx(0, 0.0, 0, 0, 0, 5_000 + i);
        }
        let h = e.aprs_health();
        assert_eq!(h.last_audio_unix, None, "nothing ever arrived");
        assert_eq!(h.drains, 20, "but we have definitely been looking");
    }

    #[test]
    fn drains_distinguish_never_looked_from_never_heard() {
        // Arming resets health; the cockpit re-reads it immediately. Before the decode thread has
        // reported anything, `None` means "not yet", not "your device is dead" — the UI needs to
        // tell those apart or it flashes an alarm on every arm.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        assert_eq!(e.aprs_health().drains, 0, "not looked yet");
        e.note_aprs_rx(0, 0.0, 0, 0, 0, 5_000);
        assert_eq!(e.aprs_health().drains, 1);
        // Re-arming starts the count over with the rest of the health.
        e.set_aprs_arm(AprsArm::Explicit);
        assert_eq!(e.aprs_health().drains, 0);
    }

    // ---- Frame candidates carry a timestamp, and noise does not count as evidence ----

    #[test]
    fn a_frame_recovered_from_signal_is_counted_and_dated() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.note_aprs_rx(1200, 0.4, 0, 1, 0, 9_000);
        let h = e.aprs_health();
        assert_eq!(h.frames_seen, 1);
        assert_eq!(
            h.last_frame_seen_unix,
            Some(9_000),
            "the UI needs to know WHEN, or a stale count claims the present"
        );
    }

    #[test]
    fn a_frame_found_in_silence_is_not_counted_as_evidence() {
        // The deframer's flag-hunt finds 0x7E patterns in sliced noise given enough minutes.
        // -99 dBFS dither (the operator's squelch-closed floor) is not a packet.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.note_aprs_rx(1200, 0.00001, 0, 2, 0, 9_000);
        let h = e.aprs_health();
        assert_eq!(
            h.frames_seen, 0,
            "noise must not manufacture 'packets heard'"
        );
        assert_eq!(h.last_frame_seen_unix, None);
    }

    #[test]
    fn a_decode_counts_however_quiet_the_meter_read() {
        // A frame that passed a 16-bit FCS is real by construction — the level meter does not
        // get a vote. Only the unverified candidates are gated.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.note_aprs_rx(1200, 0.00001, 0, 1, 1, 9_000);
        let h = e.aprs_health();
        assert_eq!(h.frames_decoded, 1, "a checksummed frame is always real");
        assert_eq!(h.last_decode_unix, Some(9_000));
    }

    #[test]
    fn arming_clears_the_frame_freshness_stamp_with_the_counters() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.note_aprs_rx(1200, 0.4, 0, 1, 0, 9_000);
        e.set_aprs_arm(AprsArm::Explicit);
        let h = e.aprs_health();
        assert_eq!(h.frames_seen, 0);
        assert_eq!(h.last_frame_seen_unix, None);
    }

    // ---- Burst-time level, and the corrected noise gate ----

    #[test]
    fn burst_time_level_is_captured_when_the_frame_is_counted() {
        // The field trap: the operator read the LIVE peak 9 s after the burst and saw the gap.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.note_aprs_rx(1200, 0.25, 0, 1, 0, 9_000); // the burst
        e.note_aprs_rx(1200, 0.00002, 0, 0, 0, 9_001); // quiet gap after it
        let h = e.aprs_health();
        assert_eq!(h.frame_peak, 0.25, "burst level, not the gap level");
        assert_eq!(h.max_frame_peak, 0.25);
        assert!(
            h.audio_peak < 0.001,
            "the LIVE peak has moved on, as it should"
        );
    }

    #[test]
    fn the_session_keeps_the_best_look_it_had_at_a_burst() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.note_aprs_rx(1200, 0.3, 0, 1, 0, 9_000);
        e.note_aprs_rx(1200, 0.01, 0, 1, 0, 9_100); // a weaker burst later
        assert_eq!(
            e.aprs_health().max_frame_peak,
            0.3,
            "one quiet burst must not set the verdict for the session"
        );
    }

    #[test]
    fn clipped_samples_in_the_burst_drain_are_reported() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.note_aprs_rx(1200, 0.999, 412, 1, 0, 9_000);
        assert_eq!(e.aprs_health().frame_clipped_samples, 412);
    }

    #[test]
    fn a_quiet_but_real_burst_is_no_longer_discarded_as_noise() {
        // ⚠️ THE REGRESSION THIS FIXES. The first gate discarded frames below -60 dBFS; the
        // operator's real burst measured -62 and would have been thrown away, showing "Silent"
        // during an audible packet. -62 dBFS is 18 dB above the measured dither floor.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.note_aprs_rx(1200, 0.0008, 0, 1, 0, 9_000); // -62 dBFS
        assert_eq!(
            e.aprs_health().frames_seen,
            1,
            "a real burst 18 dB above the dither floor must count"
        );
    }

    #[test]
    fn dither_far_below_any_signal_still_does_not_count() {
        // -99 dBFS: the operator's measured squelch-closed floor, 19 dB below the gate.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_aprs_arm(AprsArm::Explicit);
        e.note_aprs_rx(1200, 0.0000112, 0, 3, 0, 9_000);
        assert_eq!(
            e.aprs_health().frames_seen,
            0,
            "flag-matches in dither are not packets"
        );
    }

    // ---- The chip must be able to NAME the radio it is listening to ----
    //
    // The 0.21.4 regression in one sentence: a merge changed which of two 2 m-capable radios an APRS
    // activation resolves to, the tap followed the new one whose audio was set up for FT8, and the
    // section went silent with nothing on screen naming the radio. Wrong-radio silence and a dead
    // band look identical unless the app says which radio it is on.

    #[test]
    fn health_names_the_radio_the_decoder_is_listening_to() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let r1 = e.add_radio();
        e.rename_radio(r1, "FT-991A");
        e.set_radio_bands(r1, vec!["2m".into()]);
        e.set_active_radio(r1);
        e.set_aprs_arm(AprsArm::Explicit);
        assert_eq!(e.aprs_health().radio_name, "FT-991A");
    }

    #[test]
    fn the_named_radio_follows_a_handoff_mid_session() {
        // Read live, not stamped at arm time — otherwise the chip names the radio the operator
        // STARTED on, which is exactly the wrong answer after an automatic band hand-off.
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.settings.ensure_radio_profiles();
        e.rename_radio(0, "FTdx10");
        let r1 = e.add_radio();
        e.rename_radio(r1, "IC-9700");
        e.set_active_radio(0);
        e.set_aprs_arm(AprsArm::Explicit);
        assert_eq!(e.aprs_health().radio_name, "FTdx10");
        e.set_active_radio(r1);
        assert_eq!(
            e.aprs_health().radio_name,
            "IC-9700",
            "the name must track the active radio, not the one armed on"
        );
    }

    #[test]
    fn health_reports_whether_the_band_was_ambiguous() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.settings.ensure_radio_profiles();
        e.set_radio_bands(0, vec!["2m".into()]);
        e.set_frequency(144.390, "2m", "FM");
        assert_eq!(
            e.aprs_health().band_radio_count,
            1,
            "one radio covers 2 m — nothing to disambiguate, so the chip stays quiet"
        );
        let r1 = e.add_radio();
        e.set_radio_bands(r1, vec!["2m".into()]);
        e.set_frequency(144.390, "2m", "FM");
        assert_eq!(
            e.aprs_health().band_radio_count,
            2,
            "two radios cover 2 m — the operator needs told which one is listening"
        );
    }

    // ---- Receive-only tiers must never LOOK like they are transmitting ----
    //
    // `modes::tx_mode()` has always refused to build a waveform for these, so the
    // radio never keyed. The defect these cover is the other half: the app armed
    // TX, ran the sequencer, and reported overs it was silently discarding — the
    // operator saw a CQ run on MSK144 that produced no RF and no CAT.

    /// Every RX-only tier, in its DEFAULT settings resolution.
    ///
    /// The beacons are NOT here — they key the radio; they are in `BEACON_TIERS`
    /// below, which pins the different property that they refuse the QSO sequencer.
    /// ⭐ EMPTY — every shipped mode now transmits. The guards below are therefore
    /// vacuous today and are kept deliberately: they are the contract the next
    /// receive-only mode inherits, and the phantom-CQ bug they were written for is
    /// exactly what returns if a mode is added without them.
    const RX_ONLY_TIERS: [Tier; 0] = [];

    /// The BEACON tiers: transmit-capable, but with no QSO sequence. They must
    /// refuse every sequencer entry point while still being able to key.
    const BEACON_TIERS: [Tier; 2] = [Tier::Wspr, Tier::Fst4w];

    #[test]
    fn rx_only_tiers_refuse_to_start_a_cq_run() {
        for tier in RX_ONLY_TIERS {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(tier);
            let err = e
                .start_cq(None)
                .expect_err("{tier:?} is receive-only — a CQ run must be refused");
            assert!(
                err.contains("receive-only"),
                "{tier:?}: expected a receive-only refusal, got: {err}"
            );
            // The refusal has to be TOTAL: no armed TX, no running sequencer. A
            // refusal that still left the run armed would be the same phantom.
            assert!(!e.tx_enabled(), "{tier:?}: TX armed despite the refusal");
            assert!(
                !matches!(e.mode, Mode::Qso { running: true, .. }),
                "{tier:?}: sequencer running despite the refusal"
            );
        }
    }

    #[test]
    fn rx_only_tiers_refuse_to_arm_transmit() {
        // set_tx_enabled is the funnel EVERY arm path reaches (Enable-Tx button,
        // Call CQ, double-click, UDP Reply), and arming also fires a CAT retune —
        // so an ungated arm reaches the radio on a tier that can never key.
        for tier in RX_ONLY_TIERS {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(tier);
            e.set_tx_enabled(true);
            assert!(!e.tx_enabled(), "{tier:?}: TX armed on a receive-only tier");
        }
    }

    #[test]
    fn rx_only_tiers_refuse_to_answer_a_decode() {
        for tier in RX_ONLY_TIERS {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(tier);
            e.override_next_tx("W1AW", Some("FN31"), "W1AW KD9TAW EN52");
            assert!(!e.tx_enabled(), "{tier:?}: answering a decode armed TX");
            assert!(
                !e.take_immediate_tx(),
                "{tier:?}: requested the snappy key for an over that cannot be sent"
            );
        }
    }

    #[test]
    fn switching_into_an_rx_only_tier_stands_transmit_down() {
        // The likely real-world path: a CQ run is up on FT8 and the operator clicks
        // MSK144. Nothing about the run is tier-checked, so it used to survive the
        // switch intact — sequencer running, TX latched — and quietly stop keying.
        for tier in RX_ONLY_TIERS {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(Tier::Ft8);
            e.start_cq(None).unwrap();
            assert!(e.tx_enabled(), "precondition: the FT8 run armed TX");
            assert!(matches!(e.mode, Mode::Qso { running: true, .. }));

            e.set_tier(tier);
            assert!(
                !e.tx_enabled(),
                "{tier:?}: TX stayed armed across the switch"
            );
            assert!(
                !matches!(e.mode, Mode::Qso { running: true, .. }),
                "{tier:?}: the CQ run survived the switch"
            );
        }
    }

    #[test]
    fn transmit_capable_tiers_are_untouched_by_the_rx_only_guard() {
        // The guard may only ever SUBTRACT from the receive-only tiers. FT8/FT4 are
        // the gold-standard TX paths and TempoDeep (DX1) has NO ModeKind at all —
        // reading its absent kind as "cannot transmit" would silence DX1, which is
        // exactly the mistake this pins.
        for tier in [
            Tier::Ft8,
            Tier::Ft4,
            Tier::TempoFast,
            Tier::TempoDeep,
            Tier::Q65,
            Tier::Fst4,
            Tier::Msk144,
        ] {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(tier);
            e.set_tx_enabled(true);
            assert!(
                e.tx_enabled(),
                "{tier:?}: the guard blocked a TX-capable tier"
            );

            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(tier);
            e.start_cq(None)
                .unwrap_or_else(|err| panic!("{tier:?}: CQ run refused: {err}"));
            assert!(e.tx_enabled(), "{tier:?}: the CQ run did not arm TX");
            assert!(matches!(e.mode, Mode::Qso { running: true, .. }));
        }
    }

    // ---- The four pre-flight transmit-safety fixes ----
    //
    // Every one of these was found by a pre-flight review BEFORE the modes went on
    // the air, and three were reproduced with executable probes. They are grouped
    // because they share a cause: the beacon transmit path was bolted onto poll_tx
    // without walking the guard list it was skipping.

    #[test]
    fn the_watchdog_refuses_an_over_it_could_never_bound() {
        // The watchdog is evaluated when an over is BUILT, never during one, so it
        // cannot interrupt a transmission in flight. At FT8's 12.6 s that is fine.
        // At FST4-1800 it meant a THIRTY MINUTE key-down with no duration bound at
        // all, while Settings claimed a 6-minute watchdog was in force.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Fst4);
        let mut s = e.settings().clone();
        s.fst4_period_s = 1800; // a 30-minute over
        s.tx_watchdog_min = 6; // against a 6-minute limit
        e.apply_settings(s.clone());
        e.set_tx_enabled(true);
        e.set_mode("qso-run").unwrap();

        assert!(
            e.poll_tx(0).is_empty(),
            "an over longer than the whole watchdog limit must be refused, not keyed"
        );
        assert!(
            e.tx_watchdog,
            "and the refusal must be visible to the operator"
        );

        // Raising the limit past the over length makes it transmittable again —
        // the operator's decision, taken deliberately.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Fst4);
        s.tx_watchdog_min = 45;
        e.apply_settings(s);
        e.set_tx_enabled(true);
        e.set_mode("qso-run").unwrap();
        assert!(!e.tx_watchdog, "45 min > a 30 min over: no refusal");
    }

    #[test]
    fn a_watchdog_trip_emits_no_waveform() {
        // The trip used to disarm TX and then build and return the wave anyway, so
        // the radio loop keyed and cut it ~20 ms later — a brief spurious emission
        // every time the watchdog fired.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        let mut s = e.settings().clone();
        s.tx_watchdog_min = 1;
        e.apply_settings(s);
        e.set_tx_enabled(true);
        e.set_mode("qso-run").unwrap();
        e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(9_999));

        assert!(
            e.poll_tx(0).is_empty(),
            "a tripped watchdog must emit nothing"
        );
        assert!(e.tx_watchdog);
        assert!(!e.tx_enabled());
    }

    #[test]
    fn an_ft8_over_is_never_refused_by_the_length_rule() {
        // The length rule is scoped to overs exceeding the ENTIRE limit, which at
        // 12.6 s against any sane watchdog cannot happen. Deliberately NOT a "will
        // it finish in the time remaining" test — that would change FT8 by refusing
        // an over starting seconds before the limit that today goes out.
        for min in [1u32, 6, 30] {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(Tier::Ft8);
            let mut s = e.settings().clone();
            s.tx_watchdog_min = min;
            e.apply_settings(s);
            e.set_tx_enabled(true);
            e.set_mode("qso-run").unwrap();
            assert!(
                !e.tx_watchdog,
                "FT8 must never be refused by the length rule (watchdog {min} min)"
            );
        }
    }

    #[test]
    fn commit_tx_refuses_a_plan_whose_tier_changed_while_the_engine_was_unlocked() {
        // The radio loop plans an over, RELEASES the engine mutex to build the
        // waveform (the build takes MODEM_LOCK and can wait out a whole decode),
        // then re-acquires to commit. The operator can change tier inside that
        // window, and keying an FT8 waveform on Q65 is a wrong-mode emission —
        // exactly what the transmit guards exist to prevent.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        e.set_tx_enabled(true);
        e.start_cq(None).unwrap();

        // Plan on FT8, at a slot the parity accepts.
        let mut plan = None;
        for slot in 0..4u64 {
            if let Some(p) = e.plan_tx(slot) {
                plan = Some(p);
                break;
            }
        }
        let plan = plan.expect("FT8 CQ should plan an over");
        let wave = plan.waveform.build();
        assert!(!wave.is_empty(), "precondition: the FT8 over built");

        // The operator switches tier before the commit lands.
        e.set_tier(Tier::Q65);
        assert!(
            e.commit_tx(&plan, wave).is_empty(),
            "an over planned on FT8 must not key after a switch to Q65"
        );
        assert!(
            !e.snapshot().radio.transmitting,
            "the refusal must also clear the transmitting latch"
        );
    }

    #[test]
    fn jt65_call_cq_drives_the_whole_tx_path() {
        // Operator report 2026-07-28 (Windows, 0.19.16): "on jt65, call cq hard
        // crashes the program" — 0xC0000005 the instant the transmit cycle came
        // round, before PTT. TX was disabled in 0.19.17 as a mitigation and is
        // restored in 0.19.18: the cause was never on this path at all. It was the
        // DECODE dispatched at the same slot boundary, reading a front-zero-padded
        // window (xcor.f90's uninitialized `lagpk`), which is why it presented as a
        // transmit bug and why this test was clean on Linux throughout.
        //
        // Drives the ENTIRE engine-side Call CQ path — arm, start the run, key both
        // parities, then every call the UI polls afterwards — so an engine-side
        // regression fails in CI instead of on the air.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Jt65);
        e.set_tx_enabled(true);
        e.start_cq(None).expect("JT65 CQ run should start");

        let mut keyed = 0;
        for slot in 0..6u64 {
            let w = e.poll_tx(slot);
            if let Some(first) = w.first() {
                keyed += 1;
                // 126 symbols at 4458.5 samples + a 1.0 s lead-in, inside the minute.
                assert!(
                    first.len() > 500_000 && first.len() < 60 * 12_000,
                    "JT65 over is {} samples — outside the slot",
                    first.len()
                );
            }
        }
        assert!(keyed > 0, "JT65 CQ never produced a waveform in 6 slots");

        // Everything the UI polls afterwards. A panic in any of these crashes the app
        // at exactly the moment Call CQ is pressed.
        let snap = e.snapshot();
        assert_eq!(snap.link.tier, Tier::Jt65);
        let _ = e.spectrum_row();
        let _ = e.band_plan();
        let _ = e.active_frame_samples();
        let _ = e.active_capture_samples();
    }

    #[test]
    fn msk144_gets_wsjtx_meteor_scatter_patience_not_ft8_patience() {
        // ⭐ SILENCE IS THE NORMAL STATE ON METEOR SCATTER. You transmit continuously
        // and wait for a trail that may be a minute or more away, so FT8's "nobody
        // answered, move on" reflexes abandon live contacts.
        //
        // WSJT-X's own patience is the Wait-and-Reply timer,
        // stopWRTimer.start(int(12000.0*m_TRperiod)) (mainwindow.cpp:3031) — twelve
        // T/R periods of transmitting, which is SIX own overs since a station keys
        // every other period.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Msk144);
        assert_eq!(e.qso_patience_overs(), 6, "MSK144 waits 12 T/R periods");

        // And WSJT-X has no per-call CQ budget in ANY mode — the wall-clock watchdog
        // is the only bound. Our 10-call default would stop a run after ~5 minutes,
        // which on this mode reads as the mode being broken.
        assert_eq!(
            e.cq_call_budget(),
            None,
            "MSK144 CQ runs uncapped, like WSJT-X"
        );

        // The FT modes are untouched: 3 overs, and the 10-call default stands.
        for tier in [Tier::Ft8, Tier::Ft4] {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(tier);
            assert_eq!(e.qso_patience_overs(), 3, "{tier:?} keeps its patience");
            assert_eq!(
                e.cq_call_budget(),
                e.settings().cq_max_calls,
                "{tier:?} keeps its CQ budget"
            );
        }
    }

    #[test]
    fn an_explicit_stall_setting_still_wins_on_every_tier() {
        // The mode-aware value is only a DEFAULT. An operator who sets the number
        // gets the number, and Some(0) still disables abandonment entirely (stock
        // WSJT-X: wait for the operator).
        for tier in [Tier::Msk144, Tier::Ft8, Tier::Q65] {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(tier);
            let mut s = e.settings().clone();
            s.cq_stall_overs = Some(9);
            e.apply_settings(s.clone());
            assert_eq!(
                e.qso_patience_overs(),
                9,
                "{tier:?} must honour the operator"
            );

            s.cq_stall_overs = Some(0);
            e.apply_settings(s);
            assert_eq!(
                e.qso_patience_overs(),
                0,
                "{tier:?}: 0 disables abandonment"
            );
        }
    }

    #[test]
    fn a_beacon_never_keys_while_the_phone_or_cw_section_owns_the_rig() {
        // ⭐ THE RF BUG. The beacon branch returned ABOVE poll_tx's operating-mode
        // gate, and `set_operating_mode` ARMS TX when the operator enters Phone —
        // justified by a comment saying poll_tx is gated off for non-Digital, which
        // the early return had quietly made false. Net effect, from a probe: one
        // click on the Phone section, no transmit action at all, and the radio put
        // 111 s of WSPR tones into the 20 m phone segment on the beacon schedule.
        // `tx_allowed` compounded it by checking the SSB passband against PHONE
        // privileges, so the data emission was validated against the wrong rules.
        for tier in BEACON_TIERS {
            for op in ["phone", "cw", "rtty"] {
                let mut e = Engine::new("KD9TAW", "EN52", 0);
                e.set_tier(tier);
                let mut s = e.settings().clone();
                s.beacon_tx_percent = 100;
                s.beacon_power_dbm = 37;
                e.apply_settings(s);

                // The single operator action, exactly as the probe ran it.
                e.set_operating_mode(op, true);

                assert!(
                    e.poll_tx(0).is_empty(),
                    "{tier:?} beaconed while the {op} section owned the rig"
                );
            }
        }
    }

    #[test]
    fn a_beacon_is_bounded_by_the_tx_watchdog_like_everything_else() {
        // The wall-clock watchdog is implemented inline in poll_tx's QSO arm, ~240
        // lines BELOW the beacon branch, so beacons had no duration bound at all:
        // Settings offers "TX watchdog: 6 min" and it silently did not cover the two
        // modes most likely to be left running unattended.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Wspr);
        let mut s = e.settings().clone();
        s.beacon_tx_percent = 100;
        s.beacon_power_dbm = 37;
        s.tx_watchdog_min = 1;
        e.apply_settings(s);
        e.set_tx_enabled(true);

        assert!(!e.poll_tx(0).is_empty(), "precondition: the beacon keys");

        // Backdate the watchdog past its limit, as the probe did.
        e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(9_999));
        assert!(
            e.poll_tx(1).is_empty(),
            "the beacon kept keying past the watchdog limit"
        );
        assert!(e.tx_watchdog, "the trip must be visible to the operator");
        assert!(!e.tx_enabled(), "a trip disarms TX");
    }

    #[test]
    fn zero_percent_silences_the_beacon_on_both_schedulers() {
        // The 0%-never short-circuit lived only inside BeaconScheduler::next_is_tx,
        // which the Round-Robin arm never calls. A station with an RR slot left over
        // from a coordinated session kept keying every Nth interval after the
        // operator set Transmit % to 0 — which Settings describes, in as many words,
        // as "0 = listen only".
        for (rr_slot, rr_slots) in [(0u8, 0u8), (1, 2), (2, 3)] {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(Tier::Wspr);
            let mut s = e.settings().clone();
            s.beacon_tx_percent = 0;
            s.beacon_power_dbm = 37;
            s.beacon_rr_slot = rr_slot;
            s.beacon_rr_slots = rr_slots;
            e.apply_settings(s);
            e.set_tx_enabled(true);

            for slot in 0..8u64 {
                assert!(
                    e.poll_tx(slot).is_empty(),
                    "0% still keyed at slot {slot} with rr_slot={rr_slot}"
                );
            }
        }
    }

    #[test]
    fn leaving_a_long_over_tier_stands_transmit_down() {
        // set_tier halted only for RECEIVE-ONLY tiers, which stopped covering the
        // case that matters once these four gained transmitters — all declare
        // tx: true, so none is rx-only and the halt never ran. The audio service
        // cuts an in-flight over only when tx_enabled goes false, so PTT stayed held
        // for the rest of the waveform: ~93% of every WSPR interval, and up to 30
        // minutes on FST4W-1800.
        for from in [Tier::Wspr, Tier::Fst4w, Tier::Fst4, Tier::Q65] {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(from);
            e.set_tx_enabled(true);
            assert!(e.tx_enabled(), "precondition: armed on {from:?}");

            e.set_tier(Tier::Ft8);
            assert!(
                !e.tx_enabled(),
                "leaving {from:?} left transmit armed — the old mode keeps going out"
            );
        }
    }

    #[test]
    fn an_ft_tier_switch_keeps_its_shipped_behaviour() {
        // The fix above is deliberately SCOPED. Halting on any armed tier switch
        // would also disarm FT8→FT4, which is shipped gold-standard behaviour and
        // not ours to change — their overs are 13 s or less and trailing one out
        // reads as "the over finishes". This pins that they are untouched.
        for (from, to) in [
            (Tier::Ft8, Tier::Ft4),
            (Tier::Ft4, Tier::Ft8),
            (Tier::TempoFast, Tier::TempoDeep),
        ] {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(from);
            e.set_tx_enabled(true);
            e.set_tier(to);
            assert!(
                e.tx_enabled(),
                "{from:?} -> {to:?} disarmed TX; FT behaviour must not change"
            );
        }
    }

    #[test]
    fn beacon_tiers_transmit_but_refuse_every_qso_entry_point() {
        // WSPR and FST4W CAN key the radio, so the receive-only guards correctly let
        // them through — but there is no exchange to sequence. Without a separate
        // check the auto-sequencer would arm a CQ run on WSPR and try to work the
        // beacons it hears, which is meaningless.
        for tier in BEACON_TIERS {
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            e.set_tier(tier);

            // Transmit-capable: arming works, unlike a receive-only tier.
            e.set_tx_enabled(true);
            assert!(e.tx_enabled(), "{tier:?}: a beacon must be able to arm TX");

            // But no QSO.
            let err = e.start_cq(None).expect_err("{tier:?} has no CQ to run");
            assert!(
                err.contains("beacon"),
                "{tier:?}: expected a beacon refusal, got: {err}"
            );
            assert!(
                !matches!(e.mode, Mode::Qso { running: true, .. }),
                "{tier:?}: a sequencer was armed on a beacon"
            );

            // And answering a spot is not a thing the mode does.
            e.override_next_tx("W1AW", Some("FN31"), "W1AW KD9TAW EN52");
            assert!(
                !matches!(e.mode, Mode::Qso { .. }),
                "{tier:?}: answering a beacon spot started a QSO"
            );
        }
    }

    #[test]
    fn a_beacon_stays_silent_until_it_is_configured() {
        // FAIL CLOSED. WSPR reports are PUBLISHED, so a beacon with an unset power
        // would tell the world it runs 1 mW while actually running 5 W — corrupting
        // other operators' path conclusions, not just this station's. Default
        // settings must produce silence, not a wrong report.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Wspr);
        e.set_tx_enabled(true);

        assert_eq!(
            e.settings().beacon_tx_percent,
            0,
            "beaconing is off by default"
        );
        assert_eq!(e.settings().beacon_power_dbm, 0, "power has no default");
        assert!(
            e.beacon_message().is_err(),
            "an unset power must refuse to build a message"
        );

        // Even with the schedule turned fully on, an unset power stays silent.
        let mut s = e.settings().clone();
        s.beacon_tx_percent = 100;
        e.apply_settings(s.clone());
        e.set_tx_enabled(true);
        assert!(
            e.poll_tx(0).is_empty(),
            "a beacon with no configured power must not transmit"
        );

        // Configured: it builds the wire message and transmits.
        s.beacon_power_dbm = 30;
        e.apply_settings(s);
        e.set_tx_enabled(true);
        assert_eq!(e.beacon_message().unwrap(), "KD9TAW EN52 30");
    }

    #[test]
    fn a_beacon_decides_once_per_interval_not_once_per_poll() {
        // The radio loop calls poll_tx many times inside a slot. Re-drawing on each
        // call would break the anti-consecutive rule AND make the achieved duty
        // cycle meaningless — the operator would get roughly `percent` of POLLS
        // rather than of intervals.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Wspr);
        let mut s = e.settings().clone();
        s.beacon_tx_percent = 100;
        s.beacon_power_dbm = 30;
        e.apply_settings(s);
        e.set_tx_enabled(true);

        let first = e.poll_tx(4).len();
        for _ in 0..5 {
            assert_eq!(
                e.poll_tx(4).len(),
                first,
                "the same interval must yield the same decision"
            );
        }
    }

    #[test]
    fn a_period_change_in_settings_rebuilds_the_decoder() {
        // The per-mode period/submode live in Settings, so the SAME tier resolves to
        // a different ModeKind across a save. The decoder was only ever rebuilt by
        // set_tier, so changing Q65 30 s → 60 s while sitting on Q65 changed nothing
        // in the decode chain — while active_slot_secs, tx_over_secs and the WSJT-X
        // UDP tr_period all reported the new period immediately. The app disagreed
        // with itself until the operator happened to toggle tiers.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        let mut s = e.settings().clone();
        s.q65_period_s = 30;
        e.apply_settings(s.clone());
        e.set_tier(Tier::Q65);
        assert_eq!(e.active_slot_secs(), 30.0);
        let epoch_before = e.decode_epoch;

        s.q65_period_s = 60;
        e.apply_settings(s);
        assert_eq!(
            e.active_slot_secs(),
            60.0,
            "the reported period follows Settings"
        );
        assert_ne!(
            e.decode_epoch, epoch_before,
            "the decode context must be flushed — a decode dispatched at the OLD \
             period would otherwise fold in with meaningless slot indices"
        );
    }

    #[test]
    fn a_submode_change_rebuilds_the_decoder_even_though_no_size_changes() {
        // The nastiest arm: JT65 A→B (and Q65 30A→30C) change no buffer size at all,
        // so a stale decoder has NO symptom — the submode the operator asked for just
        // silently does not decode. Guard it via the epoch, which only bumps when the
        // resolved kind actually changed.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Jt65);
        let epoch_before = e.decode_epoch;
        let slot_before = e.active_slot_secs();

        let mut s = e.settings().clone();
        s.jt65_submode = 1; // JT65B — same 60 s period, different tone spacing
        e.apply_settings(s);

        assert_eq!(e.active_slot_secs(), slot_before, "JT65 has one period");
        assert_ne!(
            e.decode_epoch, epoch_before,
            "a submode-only change still has to reach the decoder"
        );
    }

    #[test]
    fn an_unrelated_save_does_not_disturb_the_decoder() {
        // The rebuild is compared BY KIND on purpose: it drops the decoder's
        // accumulated state, which for Q65/JT65 message averaging is real progress.
        // A save that does not change the resolved mode must be inert.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Q65);
        let epoch_before = e.decode_epoch;
        let mut s = e.settings().clone();
        s.prefer_rrr = !s.prefer_rrr;
        e.apply_settings(s);
        assert_eq!(
            e.decode_epoch, epoch_before,
            "an unrelated setting must not flush the decode context"
        );
    }

    #[test]
    fn selecting_a_tier_with_no_channel_on_this_band_moves_to_its_own_frequency() {
        // The bug that made MSK144 look broken: the tier-aware plans are not all
        // all-band (MSK144/Q65 are VHF+, FST4/FST4W are LF/MF), so the band-match
        // lookup MISSED for the ordinary case — 20 m FT8, click MSK144 — and the QSY
        // was a silent no-op. Tier, decoder and slot clock moved; the radio did not.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB");

        e.set_tier(Tier::Msk144);
        assert!(
            (e.settings().dial_mhz - 50.260).abs() < 1e-6,
            "MSK144 from 20 m must land on its own 6 m calling channel, got {}",
            e.settings().dial_mhz
        );
        assert_eq!(e.settings().band, "6m");

        // FST4 is 2200/630/160 m only — same fallback, different primary.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB");
        e.set_tier(Tier::Fst4);
        assert!(
            (e.settings().dial_mhz - 0.1360).abs() < 1e-6,
            "FST4 from 20 m must land on 2200 m, got {}",
            e.settings().dial_mhz
        );
    }

    #[test]
    fn a_tier_that_does_cover_this_band_stays_on_it() {
        // The fallback may only fire when the band is genuinely absent — FT8 → FT4 on
        // 20 m must still be the in-band nudge 14.074 → 14.080, not a jump to the
        // plan's first entry.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB");
        e.set_tier(Tier::Ft4);
        assert_eq!(e.settings().band, "20m", "still 20 m");
        assert!(
            (e.settings().dial_mhz - 14.080).abs() < 1e-6,
            "expected the 20 m FT4 channel, got {}",
            e.settings().dial_mhz
        );
    }

    // ---- Decode-worker robustness + retained-audio sizing ----

    /// A source that always panics — stands in for any assert in the modem wrappers
    /// (`jt65::decode_frame` panics on a short buffer or an out-of-range submode, and
    /// the others match).
    struct PanickingSource;
    impl modes::SignalSource for PanickingSource {
        fn label(&self) -> String {
            "panicking (test)".into()
        }
        fn mode_kind(&self) -> Option<modes::ModeKind> {
            Some(modes::ModeKind::Ft8)
        }
        fn decode(&mut self, _req: &modes::DecodeRequest) -> Vec<modes::Decode> {
            panic!("decoder exploded");
        }
    }

    #[test]
    fn a_panicking_decode_is_contained_and_still_returns_a_result() {
        // THE POINT IS THE RETURN, not the absence of a crash. Decoding runs on ONE
        // worker thread and the radio loop clears its in-flight latch only when a
        // result lands. A panic used to kill that thread and strand the latch true
        // forever — every later dispatch went to a dead channel via `let _ = send`,
        // which discards the error — so the app decoded nothing for the rest of the
        // session while the waterfall (fed separately) kept painting.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        *e.source.lock().unwrap() = Box::new(PanickingSource);
        let frame = vec![0.0f32; e.active_frame_samples()];

        // Silence the panic hook so the expected panic does not spam test output.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let job = e.build_decode_job(frame, 7, DecodePass::Boundary);
        let result = run_decode_job(job);
        std::panic::set_hook(prev);

        assert!(
            result.failed,
            "the panic must be reported, not swallowed silently"
        );
        assert!(result.decodes.is_empty());
        assert_eq!(
            result.slot, 7,
            "the failed result still identifies its slot"
        );
        assert_eq!(result.pass, DecodePass::Boundary);

        // And applying it must not poison engine state: dropped like a stale result,
        // so `last_rx` is never set to the empty frame a redecode would then hand to
        // a decoder whose buffer-length assert panics — killing the worker again.
        let applied = e.apply_decode_result(result);
        assert!(matches!(applied, DecodeApplied::Stale));
        assert!(
            e.last_rx.is_none(),
            "a failed decode must not clobber the retained audio"
        );
    }

    #[test]
    fn retained_audio_round_trips_through_i16_exactly() {
        // `last_rx` is stored in the decoder's own i16 format (WSJT-X does the same:
        // `short int d2[NTMAX*RX_SAMPLE_RATE]`), halving a permanent allocation that
        // is 86 MB as f32 at FST4's 1800 s period. The redecode must see byte-for-byte
        // what the live decode saw — `capture_to_i16` is round(x*32767), so dividing
        // and re-applying the same constant is exact, NOT approximate.
        let samples: Vec<f32> = vec![0.0, 1.0, -1.0, 0.5, -0.5, 0.000_03, 2.0, -2.0];
        let stored = tempo_core::channel::capture_to_i16(&samples);
        let restored: Vec<f32> = stored.iter().map(|&v| f32::from(v) / 32767.0).collect();
        let again = tempo_core::channel::capture_to_i16(&restored);
        assert_eq!(
            stored, again,
            "the i16 the decoder sees must survive the round-trip unchanged"
        );
        // Clipping is preserved rather than wrapping (±2.0 saturates, not overflows).
        assert_eq!(stored[6], i16::MAX);
        assert_eq!(stored[7], i16::MIN);
    }
}
