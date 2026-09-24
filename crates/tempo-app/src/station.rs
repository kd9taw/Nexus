//! Station-wide state — the half of the engine that belongs to the OPERATOR, not to a radio.
//!
//! [`Engine`](crate::engine::Engine) mixes two kinds of state: things that are true of the
//! STATION (one logbook, one ADIF file, one connector-upload funnel, one DXCC table, one
//! POTA activation, one PC clock) and things that are true of ONE RECEIVE/TRANSMIT CHAIN
//! (this waterfall, this slot clock, this dial, this TX queue). Only the second kind can
//! meaningfully exist more than once.
//!
//! `StationCore` is the first kind, lifted out verbatim. Today [`Engine`](crate::engine::Engine)
//! owns exactly one by value and the chain count is hard-capped at one, so this is a pure
//! relocation with no behavior change — the point is that the seam now EXISTS and the
//! compiler enforces which side each field is on. When a second chain arrives, N engines
//! share one core instead of each growing a divergent copy of the operator's log.
//!
//! What is deliberately NOT here: `settings`, `app` (identity/roster/conversations),
//! `pending_log`, `highlights`, `clear_tick`, `work_tick`, `broker_ptt` and `radio_live`.
//! Each is genuinely both-sided and needs a design ruling, not a default; they stay on
//! [`Engine`](crate::engine::Engine) untouched.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempo_core::logbook::writer::Change;
use tempo_core::logbook::{Logbook, OpClass, QsoRecord, WorkedSince};

use crate::logstore::{LogStore, Opened};

use crate::engine::{
    now_unix_secs, LotwResolver, PendingUpload, HUNT_TTL_SECS, MAX_UPLOAD_RETRIES, SSTV_GALLERY_CAP,
};

/// How many uploads the connector queue holds. See [`StationCore::enqueue_upload`] for what
/// goes when it is full.
const UPLOAD_QUEUE_CAP: usize = 256;

/// The shared `log.adi`'s freshness fingerprint — `(mtime, byte length)`, or `None`
/// if it cannot be statted. See [`StationCore::last_log_mtime`] for why the length
/// rides along; `None` never gates anything, because the recovery must never skip on
/// uncertainty.
fn log_file_stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok().map(|t| (t, m.len())))
}

/// One contact of a LoTW upload batch, as TQSL was handed it. See
/// [`StationCore::stamp_lotw_batch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LotwSigned {
    id: tempo_core::logbook::RecordId,
    fingerprint: u64,
}

/// What [`StationCore::stamp_lotw_batch`] did with a batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LotwStamped {
    /// Contacts stamped with the batch's outcome.
    pub stamped: usize,
    /// Contacts still in the log that changed after TQSL was handed them — not stamped.
    pub changed: usize,
    /// Contacts no longer in the log.
    pub gone: usize,
}

/// What the confirmation diagnostics read ([`StationCore::diagnostics_inputs`]), held apart
/// from the engine so the diagnosis — a pass over the whole log with a DXCC lookup per contact —
/// runs with the engine lock released, reading the log from the store (SPEC-2 v3 C14).
#[derive(Debug, Clone)]
pub struct DiagnosticsInputs {
    rows: crate::logstore::LogRows,
    recents: Vec<tempo_core::reconcile::ReconcileSummary>,
}

/// What the diagnosis reads of each contact: [`tempo_core::diagnostics::DiagRow`]'s fields —
/// the four confirmation channels are what `confirmed` and `award_confirmed` are read from.
const DIAGNOSIS: tempo_core::logbook::sqlite::Narrow = tempo_core::logbook::sqlite::Narrow {
    columns: &[
        "call",
        "band",
        "mode",
        "when_unix",
        "state",
        "qsl_card_rcvd_raw",
        "lotw_rcvd_raw",
        "eqsl_rcvd_raw",
        "qrz_status_raw",
        "credit_granted",
    ],
    uploads: true,
};

impl DiagnosticsInputs {
    /// How many contacts the log holds — a count, not a read of them.
    ///
    /// ⚠️ It reads the store: never under the Engine lock.
    pub fn log_len(&self) -> Result<usize, String> {
        self.rows
            .count()
            .map(|(n, _)| usize::try_from(n).unwrap_or(usize::MAX))
            .map_err(|e| e.to_string())
    }

    /// The diagnosis, exactly as [`StationCore::confirmation_diagnostics`] has always made it,
    /// and how many contacts it diagnosed — every index in the report is a position among them.
    ///
    /// ⚠️ It reads the store: never under the Engine lock.
    pub fn diagnose(
        &self,
        now: i64,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<(tempo_core::diagnostics::DiagnosticsReport, usize), String> {
        tempo_core::logbook::io_fence::whole_log_off_engine_lock("the confirmation diagnostics");
        let mut rows = Vec::new();
        let mut entities = Vec::new();
        self.rows
            .each(
                DIAGNOSIS,
                tempo_core::logbook::sqlite::Scope::All,
                tempo_core::logbook::sqlite::Order::Log,
                &mut |r| {
                    entities.push(resolve(&r.call));
                    rows.push(tempo_core::diagnostics::DiagRow::from(r));
                    std::ops::ControlFlow::Continue(())
                },
            )
            .map_err(|e| e.to_string())?;
        let recents: Vec<&tempo_core::reconcile::ReconcileSummary> = self.recents.iter().collect();
        let report = tempo_core::diagnostics::diagnose_rows(
            &rows,
            &entities,
            &recents,
            now,
            &tempo_core::diagnostics::DiagCfg::default(),
        );
        Ok((report, rows.len()))
    }
}

/// The catch-up sweep's pick (#290): the first `room` logged contacts, in log order, whose
/// upload on one of `legs` has NOT succeeded — never stamped, or stamped a failure — each with
/// the legs it is short of ([`unsent_legs`]). Read from the store: every contact's stamps, then
/// only the chosen contacts, whole, since it is the whole record an upload sends.
///
/// ⛔ Only the legs that leave a per-QSO upload stamp can be swept — see [`unsent_legs`] for
/// which three, and why the other four are refused. Asking for only those picks nothing.
///
/// ⚠️ It reads the store: never under the Engine lock. Take `rows` ([`StationCore::log_rows`])
/// and `room` ([`StationCore::catch_up_room`]) under it, and queue what this picks with
/// [`StationCore::requeue_catch_up`]. A contact another change removes between the two reads
/// is simply not picked; the next sweep looks again.
pub fn catch_up_records(
    rows: &crate::logstore::LogRows,
    legs: u8,
    room: usize,
) -> Result<Vec<(QsoRecord, u8)>, String> {
    use std::ops::ControlFlow;
    use tempo_core::logbook::sqlite::{Narrow, Order, Scope};
    const STAMPS: Narrow = Narrow {
        columns: &[],
        uploads: true,
    };
    if room == 0 {
        return Ok(Vec::new());
    }
    let mut owed: Vec<(tempo_core::logbook::RecordId, u8)> = Vec::new();
    rows.each(STAMPS, Scope::All, Order::Log, &mut |r| {
        match (unsent_legs(r, legs), r.id) {
            (0, _) | (_, None) => {}
            (legs, Some(id)) => owed.push((id, legs)),
        }
        if owed.len() >= room {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .map_err(|e| e.to_string())?;
    let ids: Vec<tempo_core::logbook::RecordId> = owed.iter().map(|(id, _)| *id).collect();
    let short_of: HashMap<tempo_core::logbook::RecordId, u8> = owed.into_iter().collect();
    let (whole, _) = rows.rows_by_ids(&ids).map_err(|e| e.to_string())?;
    Ok(whole
        .into_iter()
        .filter_map(|r| {
            let legs = r.id.and_then(|id| short_of.get(&id).copied())?;
            Some((r, legs))
        })
        .collect())
}

/// Fill a contact's country and state from the station's resolvers where it carries none —
/// what every insert does before it writes (SPEC-2 v3 D2-A), so the store holds exactly what
/// every screen shows. The record's own values always win, and a resolver that cannot place the
/// call leaves the field empty. The same rule `Engine::log_qso` applies to a contact it logs.
#[allow(clippy::type_complexity)]
pub(crate) fn fill_with(
    r: &mut QsoRecord,
    country: Option<&(dyn Fn(&str) -> Option<String> + Send + Sync)>,
    state: Option<&(dyn Fn(&str, Option<&str>) -> Option<String> + Send + Sync)>,
) {
    if r.country.is_none() {
        r.country = country.and_then(|resolve| resolve(&r.call));
    }
    if r.state.is_none() {
        r.state = state.and_then(|resolve| resolve(&r.call, r.grid.as_deref()));
    }
}

/// One fill the background job found for a stored contact that lacked it — see
/// [`StationCore::apply_log_fills`]. `None` is "nothing found", never "clear it".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFill {
    /// The contact.
    pub id: tempo_core::logbook::RecordId,
    /// Its country, if it had none and the resolver placed the call.
    pub country: Option<String>,
    /// Its state, if it had none and the resolver placed the call.
    pub state: Option<String>,
}

/// A LoTW batch's measure of "the contact that was signed": the row as the upload serialises
/// it — [`tempo_core::logbook::adif_record`], the outbound form, so a private note the upload
/// withholds is no part of it — hashed, WITHOUT its connector stamps: another connector on
/// auto-upload stamping the contact while TQSL runs changes nothing LoTW received. The station
/// fields a location-in-ADIF upload adds come from Settings, not from the row, and are no part
/// of it either.
fn lotw_fingerprint(r: &QsoRecord) -> u64 {
    use std::hash::Hasher;
    let mut row = r.clone();
    row.upload = Default::default();
    let mut h = std::hash::DefaultHasher::new();
    h.write(tempo_core::logbook::adif_record(&row).as_bytes());
    h.finish()
}

/// Canonical band key for the per-band worked indices.
///
/// Lower-cased, because the band spellings that actually reach the log differ by
/// source (Nexus writes "20m", a LoTW export "20M", and `parse_adif` passes BAND
/// through verbatim), and with the band-plan's FM channel suffix stripped
/// ("2m-fm" → "2m") since FM on 2 m is still 2 m for award purposes. The result is
/// byte-identical to `propagation::Band::label()` for every band that crate models,
/// so this side and the awards/needs side agree on what "2 m" is without tempo-app
/// having to depend on the propagation crate (it deliberately does not — see
/// [`StationCore::set_dxcc_resolver`]). A record with no BAND keys as `""`.
///
/// ⚠️ `""` IS A KEY IN THE INDEX, AND IT IS NOT A BAND. It arrives from any log row with no
/// BAND field — an ADIF import (`logbook.rs`: `f.remove("BAND").unwrap_or_default()`) is the
/// common source. The old wording here claimed it "matches no live band and so never
/// suppresses a need", and that was true only while the LIVE band was always a real label.
/// It is not: off the ham bands `settings.band` is `""` too, so asking a per-band question
/// with an empty band would match those phantom rows and answer CONFIRMED for a station the
/// operator has never worked on any band anyone can name.
///
/// The contract, therefore: **the per-band lookups are only ever ASKED with a named band.**
/// The one production caller (the decode-feed badges in `Engine::snapshot`) holds an
/// `Option<&str>` that is `None` off the bands and does not ask at all. Keep it that way —
/// a guard inside these methods cannot work, because their consumers want opposite
/// polarities of the answer (see the comment at that call site).
fn band_key(band: &str) -> String {
    // THE canonicaliser (bandplan::canonical_band), then lower-cased: the old
    // hand-rolled strip here handled only "-fm", so a legacy "6m-2"/"2m-call"
    // row keyed as its own phantom band and never suppressed a need.
    crate::bandplan::canonical_band(band).to_ascii_lowercase()
}

/// Which of `want`'s legs this record has NOT successfully uploaded — never stamped, or
/// stamped an outcome that is not [`UploadOutcome::is_sent`]. The catch-up sweep's
/// per-record question (#290); see [`catch_up_records`].
///
/// ⛔ **The three legs below are the whole list, and the omissions are deliberate.** They
/// are exactly the ones that leave a per-QSO stamp in
/// [`UploadState`](tempo_core::logbook::UploadState), so "has this contact already reached
/// the service?" has an answer. HRDLog, N3FJP, Cloudlog and WRL keep no such stamp: adding
/// one of them here could only answer "never sent" for every record in the log, and a
/// sweep would re-push the whole book on every credential save, forever. LoTW is not a leg
/// at all — it goes out as a TQSL-signed batch, not through this queue. A leg that is not
/// here contributes nothing rather than reading as unsent, so asking for only those
/// queues nothing.
pub(crate) fn unsent_legs(rec: &QsoRecord, want: u8) -> u8 {
    use crate::engine::upload_legs as legs;
    fn sent(s: &Option<tempo_core::logbook::UploadStatus>) -> bool {
        s.as_ref().is_some_and(|u| u.outcome.is_sent())
    }
    let mut owed = 0u8;
    for (leg, status) in [
        (legs::QRZ, &rec.upload.qrz),
        (legs::CLUBLOG, &rec.upload.clublog),
        (legs::EQSL, &rec.upload.eqsl),
    ] {
        if want & leg != 0 && !sent(status) {
            owed |= leg;
        }
    }
    owed
}

/// The worked-before (B4) sweeps `Engine::snapshot` reads, each with the log revision (and the
/// other inputs) it was built from. See [`StationCore::worked_sets`].
#[derive(Default)]
pub(crate) struct B4Cache {
    /// `(key_rev, fold_mode, calls, bands)`.
    #[allow(clippy::type_complexity)]
    lifetime: Option<(
        u64,
        bool,
        Arc<HashSet<String>>,
        Arc<HashSet<(String, String)>>,
    )>,
    /// `(key_rev, session start, rule, sweep)`.
    session: Option<(u64, u64, tempo_core::contest::DupeRule, Arc<WorkedSince>)>,
}

/// The operator's station: one log, one identity of record, one set of outbound
/// connector queues — shared by every receive/transmit chain the app runs.
pub struct StationCore {
    /// What the station knows about its own clock: the corroborated NTP offset
    /// it is steering by, its age and expiry, and any measurement guard 3
    /// refused. See [`crate::clocksync`] — this is NOT a bare number, because a
    /// bare number cannot say "measured 40 minutes ago and about to stop being
    /// worth applying", which is the whole of what an off-grid operator needs.
    pub(crate) clock: crate::clocksync::ClockState,
    /// One operator-facing line saying WHO owns this machine's clock and what
    /// state it is in — the third-party client that is doing the work, the OS
    /// service that is stopped, the Pi that has never checked its clock against
    /// anything. Produced by `tempo_audio::clockdiag`, which is the layer that
    /// can run OS commands; the engine only carries it to the UI. Empty until a
    /// detection pass has run.
    pub(crate) clock_owner_note: String,
    /// WSJT-X-format ALL.TXT decode lines pending flush to disk (when
    /// `settings.write_all_txt`). The engine is I/O-free, so the shell drains this via
    /// [`Self::take_all_txt_pending`] and appends to the log file. Capped so a
    /// never-draining shell can't grow it without bound.
    pub(crate) all_txt_pending: Vec<String>,
    /// Freshly-logged QSOs awaiting the shell's connector auto-upload worker
    /// (QRZ / ClubLog / eQSL). EVERY `Engine::log_qso` path queues here — the
    /// engine auto-log included — so "logged locally but never uploaded" can't
    /// happen for any log path. Drained by [`Self::take_pending_uploads`];
    /// bounded so a worker outage can't grow it without limit.
    pub(crate) pending_uploads: VecDeque<PendingUpload>,
    /// Uploads the full queue dropped (#290), held for the connector worker to report — see
    /// [`Self::enqueue_upload`]. Bounded like the queue.
    pub(crate) dropped_uploads: Vec<PendingUpload>,
    /// Earliest wall-clock second the NEXT catch-up upload may go out — the pacing slot
    /// allocator for [`UploadOrigin::CatchUp`] (#193). Bumped by
    /// [`CATCHUP_UPLOAD_SPACING_SECS`] every time a catch-up record is queued, so a scan
    /// that enqueues 81 records hands out 81 slots 15 s apart instead of 81 due-nows.
    /// Lives HERE, not in the caller, so every enqueue path is paced by construction —
    /// including the worker's transient-failure re-queue. Memory-only, like the queue it
    /// paces: nothing to restore at startup because nothing survives to be paced.
    pub(crate) catchup_slot_unix: i64,
    /// Last connector-upload outcome (operator-facing toast text) + whether it
    /// succeeded; `upload_tick` bumps on every note so the UI can toast changes.
    pub(crate) upload_note: Option<String>,
    pub(crate) upload_ok: bool,
    pub(crate) upload_tick: u32,
    /// Persistent QSO logbook (worked-before / ADIF), loaded from `log_path`.
    #[deprecated(
        note = "SPEC-2 retires the in-memory log: read the store (LogReader, StoreReads) off \
                the Engine lock. Each existing use carries #[allow(deprecated)] naming the step \
                that moves it"
    )]
    pub(crate) logbook: Logbook,
    /// The store that owns the log on disk, once [`Self::attach_store`] has run — see
    /// [`crate::logstore`]. `None` is the 1.13 path, where the log IS `log.adi`, appended to and
    /// rewritten whole: what a session falls back to when the store cannot be opened, and what
    /// the tests that pin `log.adi`'s own behaviour still drive through [`Self::set_log_path`].
    pub(crate) store: Option<LogStore>,
    /// Why the store is not in use this session, when it was asked for and could not be opened.
    pub(crate) store_problem: Option<crate::dto::LogStoreProblem>,
    /// `log_qso`'s duplicate guard, answered from the rows sharing a contact's base call —
    /// see [`tempo_core::logbook::dedup`]. Read against the in-memory log only: it has no
    /// store in scope and cannot wait on one.
    pub(crate) dedup: tempo_core::logbook::dedup::DedupIndex,
    /// Memory still holds a row another process deleted from the store: an in-place re-read
    /// (made while a caller held positions into the log) could not remove it. The next
    /// freshness poll does, whether or not anything else has changed.
    pub(crate) store_lingering: bool,
    /// The B4 sweeps, kept against the log's revision (see [`Self::worked_sets`]). A `Mutex`
    /// because the snapshot builds them through `&self`; it is only ever taken under the engine
    /// lock, so it never waits.
    pub(crate) b4_cache: std::sync::Mutex<B4Cache>,
    /// ADIF file the logbook is persisted to, if the shell set one.
    pub(crate) log_path: Option<PathBuf>,
    /// Last-seen (mtime, byte length) of the SHARED `log.adi` — the freshness
    /// fingerprint for the two-instance watcher and the pre-stamp recovery gate:
    /// re-read + reconcile only when another instance has touched the file. The
    /// length rides along because mtime granularity can be as coarse as 2 s on
    /// FAT/SMB shares (a NAS-shared log is supported), where a same-tick sibling
    /// write would otherwise be invisible.
    pub(crate) last_log_mtime: Option<(std::time::SystemTime, u64)>,
    /// ADIF journal for the Field Day contest log, if the shell set one — the
    /// in-memory FD log (which lives only inside `Mode::FieldDay`) is
    /// rewritten here on every logged contact and merged back in when a Field
    /// Day mode starts, so a mid-event restart loses nothing.
    pub(crate) fd_log_path: Option<PathBuf>,
    /// Durable journal for the single QSO held by the prompt-to-log popup.
    pub(crate) pending_qso_path: Option<PathBuf>,
    /// Journal path for the store-and-forward outbound queue (pending_msgs.json) —
    /// written on every queue mutation so held Tempo messages survive a restart.
    pub(crate) pending_msgs_path: Option<PathBuf>,
    /// The thread the two journals above are written on — the Field Day log's and the
    /// message queue's — and the JS8 inbox's (the engine's `js8_journal_path`), so the radio
    /// loop never waits on their fsync (see [`tempo_core::journal`]). No thread until the
    /// first write.
    pub(crate) journals: tempo_core::journal::JournalWriter,
    /// Callsign → DXCC entity resolver, injected by the command layer (which owns
    /// the cty.dat table) so tempo-app stays DXCC-free. `None` in headless tests
    /// (new-DXCC highlighting simply stays off). See [`Self::set_dxcc_resolver`].
    #[allow(clippy::type_complexity)]
    pub(crate) dxcc_resolve: Option<Box<dyn Fn(&str) -> Option<String> + Send + Sync>>,
    /// (Callsign, heard grid) → US state resolver, injected by the command layer (which owns
    /// the FCC callsign index) — same pattern as [`Self::set_dxcc_resolver`], and injected for
    /// the same reason: tempo-app has no `propagation` dependency, so it cannot reach the index
    /// directly. `None` in headless tests (state simply stays unresolved, never guessed).
    ///
    /// ⚠️ It MUST take the same inputs as the heard side's `us_state_hint(call, grid)`. Passing
    /// only the call would re-open the exact split this exists to close: a call the FCC file
    /// does not list still resolves on the heard side via its grid, so a call-only resolver
    /// would leave `worked_states` unable to match it and NewState would stick forever.
    #[allow(clippy::type_complexity)]
    pub(crate) state_resolve:
        Option<Box<dyn Fn(&str, Option<&str>) -> Option<String> + Send + Sync>>,
    /// Grid → rarity tier (0–3) resolver, injected by the command layer (which
    /// owns the geography table in the propagation crate) — same pattern as
    /// [`Self::set_dxcc_resolver`]. `None` in headless tests (gems stay off).
    #[allow(clippy::type_complexity)]
    pub(crate) grid_rarity_resolve: Option<Box<dyn Fn(&str) -> Option<u8> + Send + Sync>>,
    /// Injected "is this call an active LoTW uploader" check (the shell owns the
    /// ARRL user-activity file + recency window). Presentational only.
    pub(crate) lotw_resolve: Option<LotwResolver>,
    /// DXCC entities already worked (from the logbook), keyed PER BAND —
    /// `(entity, band_key)` — for new-entity decode highlighting. Rebuilt on log
    /// load + each log mutation. Per band because DXCC is a per-band award
    /// (Challenge slots, VHF DXCC): see [`worked_grids`](Self::worked_grids).
    pub(crate) worked_entities: HashSet<(String, String)>,
    /// DXCC entities CONFIRMED (award-grade: LoTW or card) per band — `(entity, band_key)`.
    /// Drives the decode panes' "hide confirmed on this band" filter (F4MQS): a station in a
    /// band-entity slot the operator has already confirmed can be filtered out, while one
    /// that is still NEW on the band always shows.
    pub(crate) confirmed_entities: HashSet<(String, String)>,
    /// Maidenhead grids already worked (uppercased), keyed PER BAND —
    /// `(grid, band_key)` — for new-grid highlighting. A grid square is an award
    /// slot on EACH band (VUCC is per band), so FN31 worked on 20 m is genuinely
    /// new again on 2 m, where a grid is a far rarer achievement.
    pub(crate) worked_grids: HashSet<(String, String)>,
    /// POTA/SOTA references already in the log (hunter side, `ota.their_ref`)
    /// — drives the NEW PARK badge like worked_entities drives new-DXCC.
    pub(crate) worked_parks: HashSet<String>,
    /// When the operator last worked each station, on any band or mode: base call
    /// (`W6A/P` in the log answers for `W6A` on a spot) → unix seconds of the most recent
    /// QSO. Drives the Spots panel's "worked within this window". Rebuilt with the rest of
    /// this index, so an edit, a delete or another instance's append moves it too.
    pub(crate) last_worked: HashMap<String, u64>,
    /// The log revision and row count the index above was last built at, so a log that only
    /// grew since is extended rather than rebuilt (see [`Self::refresh_worked_index`]). `None`
    /// forces a rebuild.
    pub(crate) worked_index_at: Option<(u64, usize)>,
    /// Park references the operator imported from their POTA "Hunted Parks.CSV"
    /// (uppercased). Unioned into `park_worked` so hunts made on CW — where the
    /// park ref is never in the exchange, so the log can't know it — still count
    /// as worked. Persisted by the shell; seeded on import + at startup.
    pub(crate) hunted_parks_import: HashSet<String>,
    /// Pending HUNT target (program, normalized ref, activator call, set-at
    /// unix): set by a one-click hunt; the next QSO logged with that call
    /// auto-tags SIG/SIG_INFO (their_*) and the pend clears. Expires after
    /// [`HUNT_TTL_SECS`] — activations end; a forgotten pend must never stamp
    /// a park on an unrelated contact hours later. Session-only.
    pub(crate) pending_hunt: Option<(String, String, String, u64)>,
    /// Per-launch salt for the hound pileup spread (stock re-randomizes each
    /// session; a pure callsign hash parked every operator on the same offset
    /// at every event).
    pub(crate) session_salt: u32,
    /// Directory for saved RX-period WAVs (settings.save_wav) — set by the shell.
    pub(crate) periods_dir: Option<String>,
    /// The latest reconcile summary from the last LoTW / eQSL sync (in-memory,
    /// this session) — its `orphans` drive the confirmation diagnostics. Per source
    /// so a later eQSL sync doesn't clobber the LoTW orphans. Resets on restart
    /// until the next sync.
    pub(crate) last_lotw_reconcile: Option<tempo_core::reconcile::ReconcileSummary>,
    pub(crate) last_eqsl_reconcile: Option<tempo_core::reconcile::ReconcileSummary>,
    /// Orphans from the last QRZ two-way sync (own its slot so an eQSL/LoTW sync
    /// doesn't clobber them). Resets on restart until the next sync.
    pub(crate) last_qrz_reconcile: Option<tempo_core::reconcile::ReconcileSummary>,
    /// Current Parks/Summits On The Air activation `(program, reference)` — when set,
    /// each logged QSO is tagged as your activation (POTA/SOTA). Transient (an
    /// activation ends), so not persisted. `None` = not activating.
    pub(crate) activation: Option<(String, String)>,
    /// Session gallery of saved SSTV images, newest last. Seeded from the
    /// persisted `gallery.json` at startup; the decode thread appends on each
    /// completed image. Capped at [`SSTV_GALLERY_CAP`].
    pub(crate) sstv_gallery: Vec<crate::dto::SstvGalleryEntry>,
}

impl StationCore {
    /// A fresh station: empty log, no paths, no injected resolvers. The shell wires the
    /// real ones in at startup (log path, cty.dat/rarity/LoTW resolvers, journals).
    #[allow(deprecated)] // SPEC-2 C19: the station is built around the in-memory log
    pub(crate) fn new() -> Self {
        Self {
            clock: crate::clocksync::ClockState::default(),
            clock_owner_note: String::new(),
            all_txt_pending: Vec::new(),
            pending_uploads: VecDeque::new(),
            dropped_uploads: Vec::new(),
            catchup_slot_unix: 0,
            upload_note: None,
            upload_ok: false,
            upload_tick: 0,
            logbook: Logbook::new(),
            store: None,
            store_problem: None,
            store_lingering: false,
            dedup: Default::default(),
            b4_cache: Default::default(),
            log_path: None,
            last_log_mtime: None,
            fd_log_path: None,
            pending_qso_path: None,
            pending_msgs_path: None,
            journals: Default::default(),
            dxcc_resolve: None,
            state_resolve: None,
            grid_rarity_resolve: None,
            lotw_resolve: None,
            worked_entities: HashSet::new(),
            confirmed_entities: HashSet::new(),
            worked_grids: HashSet::new(),
            worked_parks: HashSet::new(),
            last_worked: HashMap::new(),
            worked_index_at: None,
            hunted_parks_import: HashSet::new(),
            pending_hunt: None,
            session_salt: now_unix_secs() as u32,
            periods_dir: None,
            last_lotw_reconcile: None,
            last_eqsl_reconcile: None,
            last_qrz_reconcile: None,
            activation: None,
            sstv_gallery: Vec::new(),
        }
    }

    /// Where saved RX-period WAVs go (the shell passes `<recordings>/periods`).
    pub fn set_periods_dir(&mut self, dir: &str) {
        self.periods_dir = Some(dir.to_string());
    }

    pub fn periods_dir(&self) -> Option<String> {
        self.periods_dir.clone()
    }

    /// Point the logbook at an ADIF file and load any existing contacts from it.
    /// Called once by the shell at startup so `worked_before` highlighting and
    /// the log view reflect prior sessions, and auto-log appends to this file.
    ///
    /// This is the 1.13 path: `log.adi` IS the log. The shell takes it only when the store
    /// cannot be opened ([`Self::attach_store`] is the ordinary launch), and it is unchanged.
    #[allow(deprecated)] // SPEC-2 C19: the log.adi fallback (D1)
    pub fn set_log_path(&mut self, path: PathBuf) {
        self.store = None;
        self.logbook = Logbook::load(&path);
        self.log_path = Some(path);
        self.backfill_country();
        self.backfill_state();
        self.refresh_worked_index();
    }

    /// Make the store the owner of the log — the ordinary launch since the logbook moved into
    /// its database (see [`crate::logstore`]). The rows come from the store; `log.adi` is not
    /// read, and is from here on a mirror of them.
    ///
    /// ★ THE LOG IS THE STORE'S, FILLS INCLUDED (SPEC-2 v3 D2-A, the operator's choice). The
    /// country and state a contact's call places it in are written INTO the store: every
    /// insert fills both before it writes ([`Self::fill_record`]), and what an older build left
    /// unfilled is filled once, after the window is up, by a background job
    /// ([`crate::logfill`], [`Self::apply_log_fills`]) that runs again only when the resolvers'
    /// data changes. So this attach fills nothing and writes nothing, and memory, the store and
    /// every screen reading either hold the same rows. The one write an open itself can make is
    /// taking in a `log.adi` the store cannot account for ([`Self::take_in_log_file`]) —
    /// contacts that are in no store at all.
    #[allow(deprecated)] // SPEC-2 C19: the launch attach loads the in-memory log
    pub fn attach_store(&mut self, opened: Opened) {
        let Opened {
            store,
            records,
            foreign,
            outcome,
        } = opened;
        self.logbook = Logbook::from_store(records);
        self.log_path = Some(store.log_path().to_path_buf());
        self.last_log_mtime = None;
        self.store = Some(store);
        self.store_problem = None;
        self.store_lingering = false;
        if let Some(f) = foreign {
            self.take_in_log_file(&f.text, f.stamp);
        }
        // The conversion's last step, once: `log.adi` is still the file it read, which is not
        // the store's picture, and left so every launch until the first change would read it
        // and take it in again — an import, which can write (see `take_in_log_file`).
        if matches!(
            outcome,
            tempo_core::logbook::migrate::Outcome::Converted { .. }
        ) {
            if let Some(store) = &self.store {
                store.refresh_mirror(&self.logbook);
            }
        }
        self.refresh_worked_index();
    }

    /// Take the contacts of a `log.adi` the store does not account for into the log — a file a
    /// 1.13 instance wrote to, the operator's own log from before the store, a restore — so the
    /// mirror may replace it without taking anything with it.
    ///
    /// An IMPORT, with an import's rules: a contact the log lacks is added, one it holds is
    /// brought up to date monotonically (a confirmation, a stamp), and nothing is removed or
    /// un-confirmed. So a 1.13 instance's new contacts arrive; its edits arrive as the import
    /// rules have always treated a restated contact, and a contact it deleted stays — the
    /// visible trades a shared `log.adi` has always made, and never a contact lost.
    #[allow(deprecated)] // SPEC-2 C19: an import checks the whole log for what it already holds
    pub(crate) fn take_in_log_file(
        &mut self,
        text: &str,
        stamp: Option<tempo_core::logbook::mirror::FileStamp>,
    ) {
        let base = self.change_base();
        let (country, state) = (self.dxcc_resolve.as_deref(), self.state_resolve.as_deref());
        let (added, _, merged) = self
            .logbook
            .import_adif_with(text, |r| fill_with(r, country, state));
        self.persist_change(base, "take in log.adi");
        if !added.is_empty() || merged > 0 {
            tempo_core::applog::info(
                "logbook",
                &format!(
                    "log.adi held contacts the logbook database did not: {} added, {} brought up \
                     to date",
                    added.len(),
                    merged
                ),
            );
        }
        if let (Some(store), Some(stamp)) = (&self.store, stamp) {
            store.accept_log_file(stamp);
            // Its contacts are in: the mirror replaces it now, whether or not it held anything
            // new, so it is not read and taken in again at every launch.
            store.refresh_mirror(&self.logbook);
        }
    }

    /// The rows as they stand, for measuring a change against — taken only when a store will
    /// be told about the change. See [`Self::persist_change`].
    #[allow(deprecated)] // SPEC-2 C19: Stage 1's before-picture of every change
    fn change_base(&self) -> Option<Vec<Arc<QsoRecord>>> {
        self.store.as_ref().map(|_| self.logbook.records().to_vec())
    }

    /// Carry a change made in memory to disk. With the store, the rows whose contents differ
    /// from `base` go to the writer thread — a channel send, no I/O — and the log to the mirror
    /// lane. On the 1.13 path, the whole of `log.adi` is rewritten, as it always was.
    #[allow(deprecated)] // SPEC-2 C19: Stage 1's diff of every change
    fn persist_change(&mut self, base: Option<Vec<Arc<QsoRecord>>>, context: &str) {
        match (base, &self.store) {
            (Some(before), Some(store)) => {
                let change = Change::between(&before, &self.logbook, store.resolved());
                if let Some(store) = self.store.as_mut() {
                    store.submit(change, &self.logbook);
                }
            }
            _ => self.save_log(context),
        }
    }

    /// Carry the last `count` rows — just appended in memory — to disk. The one-contact case:
    /// no walk over the rest of the log.
    #[allow(deprecated)] // SPEC-2 C19: Stage 1's appended rows
    fn persist_appended(&mut self, count: usize) -> Option<tempo_core::logbook::writer::Ticket> {
        let change = Change::appended(&self.logbook, count, self.store.as_ref()?.resolved());
        self.store.as_mut()?.submit(change, &self.logbook)
    }

    /// Point the Field Day contest log at its durable ADIF journal. Called once
    /// by the shell at startup (beside [`Self::set_log_path`]); the journal is
    /// rewritten on every FD contact and restored when FD mode starts.
    pub fn set_fd_log_path(&mut self, path: PathBuf) {
        self.fd_log_path = Some(path);
    }

    /// Point the prompt-to-log hold at its durable journal. Called once by the shell at
    /// startup, beside [`Self::set_fd_log_path`].
    pub fn set_pending_qso_path(&mut self, path: PathBuf) {
        self.pending_qso_path = Some(path);
    }

    pub fn set_pending_msgs_path(&mut self, path: PathBuf) {
        self.pending_msgs_path = Some(path);
    }

    /// Inject the callsign → DXCC entity resolver (the command layer passes
    /// `propagation::dxcc::resolve`-backed closure). Rebuilds the worked-entity
    /// index so new-DXCC decode highlighting works from the next snapshot.
    pub fn set_dxcc_resolver(
        &mut self,
        resolve: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) {
        self.dxcc_resolve = Some(Box::new(resolve));
        self.backfill_country();
        // Every row's entity comes from the resolver, so a new one rebuilds the index.
        self.worked_index_at = None;
        self.refresh_worked_index();
    }

    /// Inject the (callsign, heard grid) → US state resolver (the command layer passes a
    /// closure over the FCC callsign index — the SAME `us_state_hint` the heard side uses).
    /// Backfills every record that lacks a STATE so the needed board, WAS and the awards
    /// matrix all see the states already worked.
    ///
    /// No `refresh_worked_index()` here, unlike [`Self::set_dxcc_resolver`]: worked STATES are
    /// not part of this struct's index (that holds entities/grids/parks). They are folded into
    /// `propagation::LogNeeds` from the records themselves, so backfilling the records is
    /// exactly what the needs side reads.
    pub fn set_state_resolver(
        &mut self,
        resolve: impl Fn(&str, Option<&str>) -> Option<String> + Send + Sync + 'static,
    ) {
        self.state_resolve = Some(Box::new(resolve));
        self.backfill_state();
    }

    /// Resolve a US state for any logged record that lacks one. No-op without a resolver;
    /// persists the log if anything changed.
    ///
    /// WHY THIS EXISTS: one question — "what state is this call in?" — used to be answered by
    /// two different resolvers on the two sides of the same comparison. The heard side resolved
    /// it from the FCC index; the worked side could only read a logged ADIF STATE, and every
    /// record Nexus generated hardcoded `state: None`. So a state could be worked and still
    /// report as needed forever. Filling the record is what makes both sides read the same
    /// source. See [`Self::backfill_country`] — same shape, same reasons, same ordering.
    fn backfill_state(&mut self) {
        if self.state_resolve.is_none() {
            return;
        }
        // Same ordering as backfill_country and for the same reason: pull in records a second
        // instance appended BEFORE the full-log rewrite below, or this silently drops them
        // (the M18 data-loss class). Doing it first also backfills the recovered records.
        self.recover_external_appends();
        let base = self.change_base();
        if self.fill_state() {
            self.persist_change(base, "backfill_state");
        }
    }

    /// Fill a US state into every record that lacks one and that the resolver can place — IN
    /// MEMORY, writing nothing. Returns whether anything was filled. [`Self::backfill_state`]
    /// persists it. The store's rows are filled another way: each insert fills its own, and the
    /// fill job the ones an older build left ([`Self::apply_log_fills`], SPEC-2 v3 D2-A).
    #[allow(deprecated)] // SPEC-2 C19: the log.adi fallback's backfill (D1)
    fn fill_state(&mut self) -> bool {
        let Some(resolve) = self.state_resolve.take() else {
            return false;
        };
        // Resolved first, written only if something resolved — see `backfill_country`.
        let fills: Vec<(usize, String)> = self
            .logbook
            .records()
            .iter()
            .enumerate()
            .filter(|(_, r)| r.state.is_none())
            .filter_map(|(i, r)| resolve(&r.call, r.grid.as_deref()).map(|st| (i, st)))
            .collect();
        self.state_resolve = Some(resolve);
        if fills.is_empty() {
            return false;
        }
        // An upgrade: a filled state is content a needs fold reads, and no row moves.
        let records = self.logbook.records_mut(OpClass::Upgrade);
        for (i, st) in fills {
            Arc::make_mut(&mut records[i]).state = Some(st);
        }
        true
    }

    /// Inject the grid → rarity-tier (0–3) resolver (the command layer passes a
    /// `propagation::gridrarity::tier_u8`-backed closure). Purely presentational
    /// — decodes/roster gain their rarity gems from the next snapshot.
    pub fn set_grid_rarity_resolver(
        &mut self,
        resolve: impl Fn(&str) -> Option<u8> + Send + Sync + 'static,
    ) {
        self.grid_rarity_resolve = Some(Box::new(resolve));
    }

    /// Rarity of a heard grid via the injected resolver; `None` when unwired,
    /// grid-less, or invalid.
    pub(crate) fn rarity_of(&self, grid: Option<&str>) -> Option<crate::dto::GridRarity> {
        let g = grid?.trim();
        if g.is_empty() {
            return None;
        }
        let f = self.grid_rarity_resolve.as_ref()?;
        f(g).map(crate::dto::GridRarity::from_tier)
    }

    /// Inject the callsign → active-LoTW-uploader check (the shell backs it with
    /// ARRL's lotw-user-activity.csv + the operator's recency window). Purely
    /// presentational — decodes/roster gain their LoTW marks from the next snapshot.
    pub fn set_lotw_resolver(&mut self, resolve: impl Fn(&str) -> bool + Send + Sync + 'static) {
        self.lotw_resolve = Some(Box::new(resolve));
    }

    /// Whether a heard call uploads to LoTW (via the injected resolver); `false`
    /// when unwired — the honest default is no highlight, never a guess.
    pub(crate) fn lotw_user(&self, call: Option<&str>) -> bool {
        let (Some(c), Some(f)) = (call, self.lotw_resolve.as_ref()) else {
            return false;
        };
        !c.trim().is_empty() && f(c.trim())
    }

    /// Resolve a DXCC country for any logged record that lacks one (e.g. a log
    /// loaded/imported from an ADIF without `COUNTRY`, or older Nexus records).
    /// No-op without a resolver; persists the log if anything changed. Run after
    /// load / import / resolver-set so the logbook + awards are country-complete.
    fn backfill_country(&mut self) {
        if self.dxcc_resolve.is_none() {
            return;
        }
        // Pull in any records a second instance appended BEFORE the full-log
        // rewrite below, so backfill can't silently drop them (the M18 data-loss
        // class). Doing it before the loop also backfills the recovered records.
        self.recover_external_appends();
        let base = self.change_base();
        if self.fill_country() {
            self.persist_change(base, "backfill_country");
        }
    }

    /// Fill a DXCC country into every record that lacks one and that the resolver can place —
    /// IN MEMORY, writing nothing. Returns whether anything was filled.
    /// [`Self::backfill_country`] persists it. The store's rows are filled another way: each
    /// insert fills its own, and the fill job the ones an older build left
    /// ([`Self::apply_log_fills`], SPEC-2 v3 D2-A).
    #[allow(deprecated)] // SPEC-2 C19: the log.adi fallback's backfill (D1)
    fn fill_country(&mut self) -> bool {
        let Some(resolve) = self.dxcc_resolve.take() else {
            return false;
        };
        // Resolved first, written only if something resolved. A mutable borrow of the records
        // marks the log REWRITTEN (`Logbook::revision`), which sends every log view a full
        // reload, and this runs after every import — in companion mode, once per contact
        // WSJT-X logs — where it usually has nothing to fill. A row the resolver cannot place
        // stays empty and is looked up again next time, as before.
        let fills: Vec<(usize, String)> = self
            .logbook
            .records()
            .iter()
            .enumerate()
            .filter(|(_, r)| r.country.is_none())
            .filter_map(|(i, r)| resolve(&r.call).map(|c| (i, c)))
            .collect();
        self.dxcc_resolve = Some(resolve);
        if fills.is_empty() {
            return false;
        }
        // Likewise: the entity index reads `country`, and no row moves.
        let records = self.logbook.records_mut(OpClass::Upgrade);
        for (i, c) in fills {
            Arc::make_mut(&mut records[i]).country = Some(c);
        }
        true
    }

    /// Write the fills the background job found (SPEC-2 v3 D2-A, [`crate::logfill`]) into the
    /// log and, as ONE change in the writer's bulk lane, into the store — with `fill_ver` on that
    /// change's last chunk, so the store names the resolver data its fills come from only once
    /// every one of them is on disk. Returns how many contacts gained a field.
    ///
    /// Each fill lands only in a field the contact still lacks: one edited, filled or deleted
    /// since the job read it keeps what it has now. No disk and no resolver call under the lock
    /// (the job resolved every call before it came here): one pass over the log, as any change
    /// to held rows makes before the store owns the write path (C19). Nothing on the 1.13 path,
    /// where the job never runs.
    #[allow(deprecated)] // SPEC-2 C19: the fill job's fills, applied to the in-memory log
    pub fn apply_log_fills(&mut self, fills: &[LogFill], fill_ver: i64) -> usize {
        if self.store.is_none() {
            return 0;
        }
        // Another window's commits first, as every change to rows the log holds takes them.
        self.recover_external_appends();
        let base = self.change_base();
        let by_id: HashMap<tempo_core::logbook::RecordId, &LogFill> =
            fills.iter().map(|f| (f.id, f)).collect();
        let hits: Vec<(usize, Option<String>, Option<String>)> = self
            .logbook
            .records()
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let fill = by_id.get(&r.id?)?;
                let country = fill.country.clone().filter(|_| r.country.is_none());
                let state = fill.state.clone().filter(|_| r.state.is_none());
                (country.is_some() || state.is_some()).then_some((i, country, state))
            })
            .collect();
        if !hits.is_empty() {
            // An upgrade, like the fills above: content a fold reads, and no row moves.
            let records = self.logbook.records_mut(OpClass::Upgrade);
            for (i, country, state) in &hits {
                let r = Arc::make_mut(&mut records[*i]);
                if let Some(c) = country {
                    r.country = Some(c.clone());
                }
                if let Some(s) = state {
                    r.state = Some(s.clone());
                }
            }
        }
        // Sent even when nothing was filled: `fill_ver` is how the next launch knows the job
        // has run over this data.
        if let (Some(before), Some(store)) = (base, &self.store) {
            let mut change = Change::between(&before, &self.logbook, store.resolved()).in_bulk();
            change.meta.push((crate::logfill::FILL_VER, fill_ver));
            if let Some(store) = self.store.as_mut() {
                store.submit(change, &self.logbook);
            }
        }
        hits.len()
    }

    /// One-click HUNT: remember the activator + park so the NEXT QSO logged
    /// with that call auto-tags `SIG`/`SIG_INFO` (POTA) / `SOTA_REF` — the
    /// hunter-side ADIF credit. Validates like [`Self::set_activation`].
    pub fn set_hunt_target(
        &mut self,
        call: &str,
        program: &str,
        reference: &str,
    ) -> Result<(), String> {
        let prog = tempo_core::pota::OtaProgram::from_code(program)
            .ok_or_else(|| format!("unknown program {program:?} (POTA/SOTA)"))?;
        let normalized = tempo_core::pota::normalize_ref(prog, reference)
            .ok_or_else(|| format!("invalid {} reference {reference:?}", prog.code()))?;
        let c = call.trim().to_uppercase();
        if c.is_empty() {
            return Err("no activator callsign".into());
        }
        self.pending_hunt = Some((prog.code().to_string(), normalized, c, now_unix_secs()));
        Ok(())
    }

    /// Drop the pending hunt target (operator cancelled / moved on).
    pub fn clear_hunt_target(&mut self) {
        self.pending_hunt = None;
    }

    /// The pending hunt (program, reference, activator call), for the UI chip.
    /// An expired pend reads as None (and is dropped lazily).
    pub fn hunt_target(&self) -> Option<(String, String, String)> {
        self.pending_hunt
            .as_ref()
            .filter(|(_, _, _, at)| now_unix_secs().saturating_sub(*at) <= HUNT_TTL_SECS)
            .map(|(p, r, c, _)| (p.clone(), r.clone(), c.clone()))
    }

    /// True when this POTA/SOTA reference is already worked — either in the log
    /// (hunter side) OR in the operator's imported POTA "Hunted Parks.CSV" (which
    /// covers CW hunts the log can't know about, since the park ref isn't exchanged).
    pub fn park_worked(&self, reference: &str) -> bool {
        let key = reference.trim().to_uppercase();
        self.worked_parks.contains(&key) || self.hunted_parks_import.contains(&key)
    }

    /// Unix seconds of the most recent QSO with this station, on any band or mode —
    /// matched on the base call, so `W6A/P` and `W6A` are one station. `None` when the log
    /// holds no contact with it.
    pub fn last_worked_unix(&self, call: &str) -> Option<u64> {
        self.last_worked
            .get(&tempo_core::message::base_call(call))
            .copied()
    }

    /// Seed the imported hunted-parks set from a POTA "Hunted Parks.CSV" (the shell
    /// parses the reference column). Replaces the set wholesale (a re-import is the
    /// full current picture). References are uppercased to match `park_worked`.
    pub fn set_hunted_parks_import(&mut self, refs: impl IntoIterator<Item = String>) {
        self.hunted_parks_import = refs
            .into_iter()
            .filter_map(|r| {
                let r = r.trim().to_uppercase();
                (!r.is_empty()).then_some(r)
            })
            .collect();
    }

    /// How many parks the operator has imported from their Hunted Parks.CSV.
    pub fn hunted_parks_import_count(&self) -> usize {
        self.hunted_parks_import.len()
    }

    /// What the snapshot reports as `log_tick`: the low 32 bits of the log's revision, so it
    /// moves on EVERY write to the records — an append, a rewrite (edit, delete, QSL mark,
    /// import, sync, upload stamp), another instance's appends folded in, or a load. (A write
    /// that turns out to change nothing can move it too; that costs a reload, never a miss.)
    /// The log views reload when it moves. It exists because Remote made the log a two-writer
    /// resource: the view used to load once and address rows by position, and a browser's
    /// delete shifted every later row under the operator's next click.
    ///
    /// It used to be a counter bumped by hand in each write path, and two paths had no bump: a
    /// log loaded by [`Self::set_log_path`], and a LoTW own-QSO echo that re-stamps rows already
    /// on file without counting them as promoted. Read off the revision, a write path cannot
    /// forget it.
    #[allow(deprecated)] // SPEC-2 C19: the watermarks outlive the in-memory log
    pub(crate) fn log_tick(&self) -> u32 {
        self.logbook.revision() as u32
    }

    /// The lifetime worked-before sets — every worked call, and every `(call, band)` pair
    /// (band·mode under `fold_mode`) — exactly as [`Logbook::worked_call_set`] and
    /// [`Logbook::worked_band_set`] build them, kept until the log's revision or `fold_mode`
    /// moves.
    ///
    /// `Engine::snapshot` reads these on every call: the UI polls it every 300 ms per window,
    /// the radio loop calls it at every slot boundary, and it runs under the engine lock. Two
    /// fresh sweeps per call held that lock for a time that grew with the log. The revision moves
    /// on every write to the records — one choke point in `tempo_core::logbook` — so no write
    /// path can leave these stale, and an unchanged log costs no sweep at all.
    #[allow(clippy::type_complexity)]
    #[allow(deprecated)] // SPEC-2 C13: the B4 sets, from the hot index
    pub(crate) fn worked_sets(
        &self,
        fold_mode: bool,
    ) -> (Arc<HashSet<String>>, Arc<HashSet<(String, String)>>) {
        // Keyed on key_rev, not the revision: these sets read call, band and mode only, so
        // an upload stamp or a country backfill cannot move them and must not cost a sweep.
        let revision = self.logbook.key_rev();
        let mut cache = self
            .b4_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((rev, fold, calls, bands)) = &cache.lifetime {
            if *rev == revision && *fold == fold_mode {
                return (calls.clone(), bands.clone());
            }
        }
        let calls = Arc::new(self.logbook.worked_call_set());
        let bands = Arc::new(self.logbook.worked_band_set(fold_mode));
        cache.lifetime = Some((revision, fold_mode, calls.clone(), bands.clone()));
        (calls, bands)
    }

    /// The contest session's sweep of the general log ([`Logbook::worked_keys_since`]), kept
    /// the same way as [`Self::worked_sets`] until the revision, the session start or the rule
    /// moves.
    #[allow(deprecated)] // SPEC-2 C13: the contest session, from the hot index
    pub(crate) fn worked_since(
        &self,
        cutoff: u64,
        rule: &tempo_core::contest::DupeRule,
    ) -> Arc<WorkedSince> {
        // key_rev for the same reason as `worked_sets`, with the exchange included: a dupe
        // key is call, band, mode class and the contest exchange, all of them row identity.
        let revision = self.logbook.key_rev();
        let mut cache = self
            .b4_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((rev, at, held, sweep)) = &cache.session {
            if *rev == revision && *at == cutoff && held == rule {
                return sweep.clone();
            }
        }
        let sweep = Arc::new(self.logbook.worked_keys_since(cutoff, rule));
        cache.session = Some((revision, cutoff, *rule, sweep.clone()));
        sweep
    }

    /// Recompute the worked-entity and worked-grid sets from the logbook; run on log load and
    /// after each log mutation.
    ///
    /// When the log only GREW since the last run — a logged contact, the common case, once per
    /// QSO and under the engine lock — only the new rows are indexed. Every set here is a union
    /// over the records and `last_worked` a maximum, so the old index plus the appended rows IS
    /// the rebuild, without a pass over the whole log and a DXCC lookup per row on every
    /// contact. Anything else — an edit, delete, merge, reload or a new resolver — rebuilds.
    #[allow(deprecated)] // SPEC-2 C13: the badge index, maintained per change
    pub(crate) fn refresh_worked_index(&mut self) {
        let from = match self.worked_index_at {
            Some((revision, rows))
                if self.logbook.appended_only_since(revision) && rows <= self.logbook.len() =>
            {
                rows
            }
            _ => {
                self.worked_grids.clear();
                self.worked_entities.clear();
                self.confirmed_entities.clear();
                self.worked_parks.clear();
                self.last_worked.clear();
                0
            }
        };
        self.worked_index_at = Some((self.logbook.revision(), self.logbook.len()));
        for r in &self.logbook.records()[from..] {
            // Any band, any mode: "worked recently" is about the station, not an award slot.
            let base = tempo_core::message::base_call(&r.call);
            if !base.is_empty() {
                let at = self.last_worked.entry(base).or_insert(r.when_unix);
                *at = (*at).max(r.when_unix);
            }
            // Parks are NOT per band: a POTA/SOTA reference is hunted once, on any
            // band, so this one stays a flat set. A two-fer ("US-0001,US-0002") is a contact
            // with each park, split on the separators the activator export reads.
            if let Some(refs) = &r.ota.their_ref {
                for p in refs.split([',', ';']) {
                    let p = p.trim();
                    if !p.is_empty() {
                        self.worked_parks.insert(p.to_uppercase());
                    }
                }
            }
            let band = band_key(&r.band);
            if let Some(g) = &r.grid {
                // A SATELLITE contact (PROP_MODE=SAT) earns Satellite-VUCC credit
                // only (the ARRL rule), so its grid must NOT enter the per-band
                // terrestrial index — a 2m sat-only FN31 muting the 2m NEW GRID
                // decode badge would hide a slot that is genuinely still open.
                // Mirrors the same exclusion in propagation's LogNeeds::add_qso.
                let sat = r
                    .prop_mode
                    .as_deref()
                    .is_some_and(|p| p.trim().eq_ignore_ascii_case("SAT"));
                // Index at 4-char granularity so a 6-char logged grid matches a 4-char decode.
                if !sat {
                    if let Some(g4) = Self::grid4(g) {
                        self.worked_grids.insert((g4, band.clone()));
                    }
                }
            }
            if let Some(resolve) = &self.dxcc_resolve {
                if let Some(entity) = resolve(&r.call) {
                    // award_confirmed = LoTW/card (not eQSL/QRZ) — the same grade the
                    // awards screens count, so the filter agrees with them.
                    if r.award_confirmed {
                        self.confirmed_entities
                            .insert((entity.clone(), band.clone()));
                    }
                    self.worked_entities.insert((entity, band));
                }
            }
        }
    }

    /// The 4-character Maidenhead field+square, upper-cased — the granularity grids are
    /// AWARDED at (VUCC counts squares, not subsquares).
    ///
    /// Both the index and the lookup MUST go through this. They did not: the log stores
    /// whatever was logged ("FN31PR" from a QRZ import), while a decode carries 4 characters
    /// ("FN31"), so the compare never matched and every such square reported NOT worked —
    /// a NEW GRID badge that fires forever. Mirrors `needalert::grid4`, which already did
    /// this correctly on the alerting side; only this index disagreed.
    fn grid4(grid: &str) -> Option<String> {
        let g: String = grid.trim().to_ascii_uppercase().chars().take(4).collect();
        (g.len() == 4).then_some(g)
    }

    /// Is this grid already worked ON THIS BAND? (`band` is the raw band label —
    /// canonicalized here.) A grid worked only on another band reads as NOT worked,
    /// which is the point: per-band is how grids are awarded.
    pub(crate) fn grid_worked_on(&self, grid: &str, band: &str) -> bool {
        Self::grid4(grid).is_some_and(|g4| self.worked_grids.contains(&(g4, band_key(band))))
    }

    /// Is this DXCC entity already worked ON THIS BAND? Per band, like
    /// [`grid_worked_on`](Self::grid_worked_on).
    pub(crate) fn entity_worked_on(&self, entity: &str, band: &str) -> bool {
        self.worked_entities
            .contains(&(entity.to_string(), band_key(band)))
    }

    /// Is this DXCC entity CONFIRMED (award-grade) ON THIS BAND? Drives the decode panes'
    /// hide-confirmed filter (F4MQS).
    pub(crate) fn entity_confirmed_on(&self, entity: &str, band: &str) -> bool {
        self.confirmed_entities
            .contains(&(entity.to_string(), band_key(band)))
    }

    /// Whether the entity has been worked on ANY band — a true ATNO check (all-time), as opposed
    /// to the per-band [`entity_worked_on`]. Distinguishes the decode feed's `DXCC` (all-time
    /// new) tag from its `BAND` (worked elsewhere, new on this band) tag.
    pub(crate) fn entity_worked_ever(&self, entity: &str) -> bool {
        self.worked_entities.iter().any(|(e, _)| e == entity)
    }

    /// Drain the freshly-logged QSOs awaiting connector auto-upload (FIFO).
    /// Called by the shell's upload worker; empty when nothing was logged.
    pub fn take_pending_uploads(&mut self) -> Vec<PendingUpload> {
        self.pending_uploads.drain(..).collect()
    }

    /// THE QUEUE'S ONE DOOR (#290). Every upload enters here — a live contact from
    /// `log_qso`, a catch-up record, a retry — so the cap has one rule and nothing it drops
    /// goes unreported.
    ///
    /// When the queue is full, what goes is the oldest CATCH-UP entry, the arriving one
    /// included; a live entry only when no catch-up entry is left. Catch-up is history that
    /// stays unsent in the log for the next sweep, and live is the contact at the key, so a
    /// catch-up can never cost a live contact its place.
    ///
    /// What is dropped waits in [`Self::take_dropped_uploads`] for the connector worker to
    /// name it in the Connections log.
    pub(crate) fn enqueue_upload(&mut self, upload: PendingUpload) {
        self.pending_uploads.push_back(upload);
        if self.pending_uploads.len() <= UPLOAD_QUEUE_CAP {
            return;
        }
        let oldest_catchup = self
            .pending_uploads
            .iter()
            .position(|u| u.origin == crate::engine::UploadOrigin::CatchUp);
        let Some(dropped) = self.pending_uploads.remove(oldest_catchup.unwrap_or(0)) else {
            return;
        };
        if self.dropped_uploads.len() < UPLOAD_QUEUE_CAP {
            self.dropped_uploads.push(dropped);
        } else {
            // A whole queue's worth of reports that nothing has collected: say it where it
            // can still be read, rather than hold more.
            tempo_core::applog::error(
                "upload",
                &format!(
                    "upload queue full — dropped the QSO with {} without sending it",
                    dropped.rec.call
                ),
            );
        }
    }

    /// Drain the uploads [`Self::enqueue_upload`] dropped since the last call.
    pub fn take_dropped_uploads(&mut self) -> Vec<PendingUpload> {
        std::mem::take(&mut self.dropped_uploads)
    }

    /// Re-queue an upload for ONLY the legs that transiently failed (network down,
    /// service busy), so the worker retries them without re-pushing the legs that
    /// already succeeded — a permanently-rejected or successful leg is never in
    /// `legs`. Dropped once past [`MAX_UPLOAD_RETRIES`] or with nothing owed.
    pub fn requeue_upload(&mut self, rec: tempo_core::logbook::QsoRecord, legs: u8, attempts: u8) {
        self.requeue_upload_at(rec, legs, attempts, 0, crate::engine::UploadOrigin::Live);
    }

    /// As [`requeue_upload`](Self::requeue_upload) but stamping when the record is next
    /// DUE — the exponential-backoff not-before time the worker honours. `now_unix + 0`
    /// means "due immediately"; the transient-retry path passes `now + upload_backoff_secs`.
    pub fn requeue_upload_at(
        &mut self,
        rec: tempo_core::logbook::QsoRecord,
        legs: u8,
        attempts: u8,
        retry_after_unix: i64,
        origin: crate::engine::UploadOrigin,
    ) {
        if legs == 0 || attempts >= MAX_UPLOAD_RETRIES {
            return;
        }
        // PACING, and only for catch-up (#193). An UNSTAMPED catch-up record (`0` = due
        // now, the sentinel this API already uses) is handed the next free slot — never
        // sooner than the last catch-up queued plus the spacing, never in the past — so
        // the drain worker's existing not-due check trickles them out one every
        // CATCHUP_UPLOAD_SPACING_SECS instead of pushing the lot in a single tick.
        //
        // ⚠️ ONLY when unstamped. The worker puts a not-yet-due record straight back on the
        // queue every 2 s tick; re-slotting one that already holds a due time would shove
        // every waiting record another spacing into the future on every tick, and a
        // catch-up of any size would march away from the present and never drain. A stamp
        // already set — a slot from this scan, or a transient-failure backoff — is kept.
        //
        // A Live record is untouched by all of it: it keeps the stamp it was given, which
        // for a fresh contact is 0 = go now. That is the whole point of realtime upload.
        let due = if origin == crate::engine::UploadOrigin::CatchUp && retry_after_unix == 0 {
            let slot = self.catchup_slot_unix.max(now_unix_secs() as i64);
            self.catchup_slot_unix = slot + crate::engine::CATCHUP_UPLOAD_SPACING_SECS;
            slot
        } else {
            retry_after_unix
        };
        self.enqueue_upload(crate::engine::PendingUpload {
            rec,
            origin,
            legs,
            attempts,
            retry_after_unix: due,
        });
    }

    /// How many records the catch-up sweep may queue now: the connector queue's FREE room
    /// (#290 — past the cap a catch-up record would only be dropped again, and what is not
    /// queued stays unsent in the log for the next sweep). No log read; take it under the lock
    /// with [`Self::log_rows`], pick with [`catch_up_records`] after releasing it, and queue with
    /// [`Self::requeue_catch_up`].
    pub fn catch_up_room(&self) -> usize {
        UPLOAD_QUEUE_CAP.saturating_sub(self.pending_uploads.len())
    }

    /// Re-queue the records the catch-up sweep picked ([`catch_up_records`]), each on the legs
    /// it is short of — the F4MQS "nothing retried after I fixed the password" gap. PACED:
    /// these go out as [`UploadOrigin::CatchUp`], one every [`CATCHUP_UPLOAD_SPACING_SECS`],
    /// because the log holds ADIF-imported history as well as this session's contacts and
    /// ClubLog objects to history arriving through the realtime endpoint in a burst (#193).
    /// Returns how many RECORDS were queued.
    ///
    /// Each record carries only what it is actually short of, so a contact that reached QRZ
    /// but not eQSL is re-pushed to eQSL alone rather than duplicated at QRZ.
    pub fn requeue_catch_up(&mut self, stale: Vec<(QsoRecord, u8)>) -> usize {
        let n = stale.len();
        for (rec, owed) in stale {
            self.requeue_upload_at(rec, owed, 0, 0, crate::engine::UploadOrigin::CatchUp);
        }
        n
    }

    /// Re-queue a record whose upload just FAILED, at no earlier than `earliest_due`.
    ///
    /// ⚠️ A FAILURE IS A FRESH SCHEDULING DECISION, which is why it does not go through
    /// [`Self::requeue_upload_at`]'s unstamped-only pacing. That guard exists so the drain
    /// worker's every-tick put-back of a not-yet-due record cannot shove the whole queue
    /// further into the future; it is not a statement that a stamped record is already
    /// paced. A transient failure stamps `now + backoff`, and taking that at face value let
    /// a catch-up record leave the pacing lane for good on its first blip — after which it
    /// was rationed only by the shared backoff, which flattens at 300 s.
    ///
    /// That is how the pacing turned back into a burst, and a worse one: ClubLog throttling
    /// us IS a transient failure, so an outage sends every due record down the ladder, and
    /// on recovery they are all due in the past and the worker pushes the lot inside one
    /// 2 s tick. Pacing here keeps a recovering catch-up a trickle instead of the very
    /// burst the pacing was added to prevent.
    ///
    /// A Live record keeps its backoff untouched: a just-worked contact that failed should
    /// retry as soon as the ladder allows.
    pub fn requeue_after_failure(
        &mut self,
        rec: tempo_core::logbook::QsoRecord,
        legs: u8,
        attempts: u8,
        earliest_due: i64,
        origin: crate::engine::UploadOrigin,
    ) {
        if origin != crate::engine::UploadOrigin::CatchUp {
            self.requeue_upload_at(rec, legs, attempts, earliest_due, origin);
            return;
        }
        // Never before its backoff, and never on top of another catch-up record.
        let slot = self.catchup_slot_unix.max(earliest_due);
        self.catchup_slot_unix = slot + crate::engine::CATCHUP_UPLOAD_SPACING_SECS;
        // Already stamped, so `requeue_upload_at` leaves the slot alone.
        self.requeue_upload_at(rec, legs, attempts, slot, origin);
    }

    /// Record a connector-upload outcome for the operator (toast text + level).
    /// Bumps `upload_tick` so the UI's snapshot poll notices the change.
    pub fn note_upload(&mut self, note: impl Into<String>, ok: bool) {
        self.upload_note = Some(note.into());
        self.upload_ok = ok;
        self.upload_tick = self.upload_tick.wrapping_add(1);
    }

    /// Begin a Parks/Summits On The Air activation — every QSO logged afterward is
    /// tagged as your activation until [`clear_activation`](Self::clear_activation).
    /// Validates + normalizes the reference; returns the normalized `(program, ref)`
    /// or an error string for an unknown program / malformed reference.
    pub fn set_activation(
        &mut self,
        program: &str,
        reference: &str,
    ) -> Result<(String, String), String> {
        let prog = tempo_core::pota::OtaProgram::from_code(program)
            .ok_or_else(|| format!("Unknown program '{program}' — use POTA or SOTA."))?;
        let normalized = tempo_core::pota::normalize_ref(prog, reference)
            .ok_or_else(|| format!("'{reference}' isn't a valid {} reference.", prog.code()))?;
        self.activation = Some((prog.code().to_string(), normalized.clone()));
        Ok((prog.code().to_string(), normalized))
    }

    /// End the current activation (subsequent QSOs are untagged).
    pub fn clear_activation(&mut self) {
        self.activation = None;
    }

    /// The current activation `(program, reference)`, if any.
    pub fn activation(&self) -> Option<(String, String)> {
        self.activation.clone()
    }

    /// How many logged QSOs carry the current activation reference (the live count
    /// for the activation panel). 0 when not activating.
    #[allow(deprecated)] // SPEC-2 C13: the activation count, from the hot index
    pub fn activation_qso_count(&self) -> usize {
        match &self.activation {
            Some((_, reference)) => self
                .logbook
                .records()
                .iter()
                .filter(|r| r.ota.my_ref.as_deref() == Some(reference.as_str()))
                .count(),
            None => 0,
        }
    }

    /// Before any full-log rewrite ([`Logbook::save`]), pull back any records that
    /// another writer — a second Nexus instance sharing this `log.adi`, since there
    /// is no single-instance guard — appended to the file after we loaded it. Our
    /// in-memory copy is otherwise stale, and `save` would `rename()` a truncated
    /// log over the file, silently discarding those QSOs.
    ///
    /// The reconcile ADDS the records we lack (appended to the end, leaving existing
    /// indices valid) and upgrades the shared ones monotonically — it never
    /// resurrects a record we just edited or deleted, PROVIDED callers run this
    /// BEFORE their mutation, while our copy still holds the record being changed.
    /// No-op without a log path or on a read error.
    /// Returns whether the disk was actually re-read (fingerprint moved).
    ///
    /// # What this costs: an EDIT made in the OTHER instance arrives as a DUPLICATE
    ///
    /// The contract above covers edits made HERE. It cannot cover an edit made
    /// THERE. A QSO has no stable id in the ADIF, so identity is
    /// (call, band, mode-class, contact second) — and a correction usually changes
    /// one of those. Instance A fixes a mis-logged time, 12:00 → 12:05, and rewrites
    /// the file; we still hold the 12:00 row; this recovery sees 12:05 as a contact
    /// we do not have and APPENDS it. From then on both instances hold two rows for
    /// one QSO — stably, permanently, and in the file — and both are eligible to be
    /// uploaded (`lotw_unsent_indices` counts them separately, so LoTW is offered two
    /// QSOs for one contact). Same for a corrected callsign or band. An edit that
    /// keeps all four key fields pairs normally, and OUR copy of the edited field is
    /// what the next rewrite writes back: that correction is silently reverted.
    ///
    /// **This is the chosen trade, not an oversight.** The only key that could
    /// recognise 12:05 as "the 12:00 row, edited" is a fuzzy one, and fuzz here is
    /// what mis-paired two distinct contacts with one station inside a day and
    /// destroyed one of them (see [`tempo_core::reconcile::merge_own_disk`]). A
    /// duplicate is on screen and one delete away; a silently reverted correction is
    /// invisible, and a mis-pairing is a QSO gone. Pinned by
    /// `merge_own_disk_leaves_a_cross_instance_edit_as_a_visible_duplicate`; told to
    /// the operator in the CHANGELOG. Making an edit detectable instead needs a
    /// per-record id persisted in the ADIF, which no existing log carries.
    ///
    /// # Two further cross-instance gaps, OPEN and deliberately left open
    ///
    /// Both are in the same family as the edit-becomes-a-duplicate trade above, both need the
    /// same missing ingredient, and both were looked at (and scoped out) when the whole-file
    /// rewrite gained its `fsync` in 2026-09. A partial fix to a log write path is worse than
    /// a named gap, so they are named here rather than half-closed.
    ///
    /// - **The recover→rewrite window.** This function reads the file; `save_log` renames a
    ///   full rewrite over it some time later. An append the other instance makes IN BETWEEN
    ///   is in neither our memory nor the file we publish, so it is lost. The fingerprint gate
    ///   narrows this to the width of one caller's work, and cannot close it: the two
    ///   operations are not one atomic step. Closing it properly means holding an exclusive
    ///   lock across read-and-rewrite — with its own crash story, because a lock file that
    ///   outlives a crashed instance locks the operator out of their own log.
    /// - **A delete can be resurrected.** `reconcile_disk` ADDS what the disk has and we lack;
    ///   it never removes what we have and the disk lacks — correctly, because "absent from
    ///   disk" is what an append-only log looks like to a stale reader. So if the OTHER
    ///   instance deletes a QSO and rewrites, we still hold it, and our next rewrite puts it
    ///   back. Distinguishing "deleted there" from "not yet written there" needs a persisted
    ///   per-record id and a tombstone — the same ingredient the edit trade above lacks, and
    ///   not something that can be added to an ADIF other loggers also read.
    #[allow(deprecated)] // SPEC-2 C19: another window's commits (D4)
    fn recover_external_appends(&mut self) -> bool {
        if self.store.is_some() {
            // IN PLACE: every caller of this is about to change a row it may be holding BY
            // POSITION, and the 1.13 recovery it stands in for only ever appended, so positions
            // held across it stayed true. A re-read here must not move a row either.
            return self.refresh_from_store(true);
        }
        let Some(path) = self.log_path.clone() else {
            return false;
        };
        // Fingerprint gate: the stamp/save paths run this up to three times per
        // logged QSO, and an unconditional read re-parsed the whole multi-MB
        // log each time. When the file's (mtime, len) matches what we last read
        // or WROTE (save_log records our own writes), the disk holds exactly
        // what we hold — skip the parse. A stat error falls through to the
        // full read: never skip on uncertainty.
        let stamp = log_file_stamp(&path);
        if stamp.is_some() && stamp == self.last_log_mtime {
            return false;
        }
        // A FAILED read records nothing: stamping the fingerprint here would
        // make the gate treat the failure as "reconciled", and the next full
        // rewrite would drop whatever the other instance wrote. Retry instead.
        //
        // BYTES, then a lossy decode — never `read_to_string`. A `log.adi` carrying CP1253 /
        // CP1252 text (Greek/German/French Windows write it into NAME/QTH/COMMENT routinely,
        // and it is what the 2026-08 Greek-Windows report turned up) is not valid UTF-8, so
        // `read_to_string` returns Err on a file that is perfectly present and readable. This
        // arm would then fire on EVERY call, and the other instance's QSOs would never be
        // reconciled in — permanently, for those operators. Same reasoning as
        // `Logbook::load`; the ADIF structure is ASCII, so the records still parse.
        let Ok(bytes) = std::fs::read(&path) else {
            return false;
        };
        let disk = String::from_utf8_lossy(&bytes);
        if !disk.is_empty() {
            // Field-level MERGE, not an additive import: fold in another instance's appends AND
            // upgrade shared records' confirmation/upload/QSL-sent state from disk, so this
            // instance's imminent full-file rewrite can't clobber what the other one wrote.
            self.logbook.reconcile_disk(&disk);
        }
        self.last_log_mtime = stamp;
        true
    }

    /// The store's answer to [`Self::recover_external_appends`]: if ANOTHER process — a second
    /// radio window on this data folder — has committed to the store since memory last matched
    /// it, re-read the store and lay this process's own changes still in flight over it (see
    /// [`crate::logstore::LogStore::reload`]). Whether anything was re-read.
    ///
    /// The question is an atomic read, so it costs nothing when nothing changed. The re-read
    /// happens only after another process wrote, at exactly the points the 1.13 path re-read
    /// `log.adi` — before a change to existing rows, and on the freshness poll — and it closes
    /// the gaps that path had to leave open: rows are matched by id, so an edit made in the
    /// other window arrives as an edit (not a duplicate), and a contact deleted there is gone
    /// here too (not resurrected by this window's next rewrite, because there is none).
    ///
    /// `in_place` keeps every row where it is — see [`crate::logstore::LogStore::reload`]: a
    /// row the other process deleted then lingers until a re-read that may move rows (the
    /// freshness poll) removes it.
    #[allow(deprecated)] // SPEC-2 C19: another window's commits (D4)
    fn refresh_from_store(&mut self, in_place: bool) -> bool {
        let Some(store) = self.store.as_mut() else {
            return false;
        };
        if !store.foreign_changed() && (in_place || !self.store_lingering) {
            return false;
        }
        match store.reload(self.logbook.records(), in_place) {
            Ok((rows, lingering)) => {
                self.store_lingering = lingering;
                // The store's rows carry their fills (SPEC-2 v3 D2-A: every insert fills before
                // it writes, and the fill job writes what an older build left), so the rows
                // re-read are the rows every screen shows, and nothing is filled here.
                self.logbook.replace_rows(rows);
                true
            }
            Err(e) => {
                tempo_core::applog::error(
                    "logbook",
                    &format!(
                        "another Nexus window changed the logbook, and this one could not read \
                         the change: {e}"
                    ),
                );
                false
            }
        }
    }

    /// THE way to APPEND to the logbook file: write the records on the end, then
    /// carry the freshness fingerprint forward so the recovery gate above does not
    /// re-parse a multi-MB log we extended ourselves.
    ///
    /// An append needs no recovery first — it cannot truncate anything — but it does
    /// move the file's `(mtime, len)`, and leaving the fingerprint behind made the
    /// gate MISS on every later call. That is a permanent per-call cost, not a
    /// one-time one: companion mode imports one ADIF per logged QSO, so a 26,000-QSO
    /// log was re-parsed from disk on every contact WSJT-X logged.
    ///
    /// Stamping is only sound while we can account for every byte on disk, so two
    /// checks fence it and BOTH are load-bearing:
    ///
    /// - the file must look exactly as we last read or WROTE it before we append —
    ///   otherwise our copy is already stale by another instance's records, and
    ///   nothing here would ever pull them back;
    /// - it must be exactly `written` bytes longer afterwards — otherwise another
    ///   instance appended in the window between our write and our stat, and the
    ///   post-write length is partly ITS bytes, which we do not hold.
    ///
    /// Fail either (or fail the write itself) and we drop the fingerprint rather than
    /// record it, so the next recovery re-reads. A stamp we cannot justify is exactly
    /// how a stale copy silently deletes a second instance's QSOs on the next full
    /// rewrite — the fault `recover_external_appends` exists to prevent. `written` is
    /// re-derived through the same [`adif_record_own_log`] the append writes; if the two ever
    /// drift the length check simply misses and we fall back to re-reading.
    ///
    /// # MEMORY FIRST — the caller's half of the contract
    ///
    /// `recs` must ALREADY be in `self.logbook`, as its last records, before this is
    /// called. The fingerprint recorded below says "the file holds exactly what we
    /// hold"; append a record we have not added to memory yet and that claim is false
    /// for the width of whatever runs next. Unwind in that window — `Engine::log_qso`
    /// spawns two threads there, and `std::thread::spawn` panics when the OS refuses
    /// one — and the contact is on disk, absent from memory, with the gate saying the
    /// two agree: the next full rewrite writes memory over the file and DELETES the
    /// contact just logged. Nothing pulls it back, because the gate is what would have
    /// re-read it. Memory is the source of truth for `save`, so memory is written
    /// first, always. Checked here in debug builds, where the invariant is relied on.
    ///
    /// [`adif_record_own_log`]: tempo_core::logbook::adif_record_own_log
    pub(crate) fn append_to_log(&mut self, recs: &[QsoRecord]) {
        let _ = self.append_to_log_checked(recs, false);
    }

    // The same memory-first append and fingerprint logic, with an explicit
    // receipt for Remote. No rewrite, rollback, retry or second upload path.
    #[allow(deprecated)] // SPEC-2 C19: Stage 1's append to the in-memory log
    pub(crate) fn append_to_log_checked(
        &mut self,
        recs: &[QsoRecord],
        receipt: bool,
    ) -> Option<Vec<tempo_core::logbook::LogAppendReceipt>> {
        if self.store.is_some() {
            // The rows are already the log's last (memory first — see above); the writer is
            // told, the mirror follows, and nothing here touches the disk. A receipt is the
            // change's ticket, redeemed after every lock is released.
            let ticket = self.persist_appended(recs.len())?;
            let writer = self.store.as_ref()?.writer();
            return Some(if receipt {
                vec![tempo_core::logbook::LogAppendReceipt::durable(
                    writer,
                    ticket,
                    crate::logstore::DURABLE_WAIT,
                )]
            } else {
                Vec::new()
            });
        }
        let path = self.log_path.clone()?;
        debug_assert!(
            {
                let held = self.logbook.records();
                held.len() >= recs.len()
                    && held[held.len() - recs.len()..]
                        .iter()
                        .zip(recs)
                        .all(|(held, rec)| **held == *rec)
            },
            "append_to_log: the records must already be in memory (see the contract above)"
        );
        let before = log_file_stamp(&path);
        let mut accountable = before.is_some() && before == self.last_log_mtime;
        let mut written = 0u64;
        let mut succeeded = true;
        let mut receipts = Vec::new();
        for r in recs {
            match if receipt {
                Logbook::append_for_sync(&path, r).map(Some)
            } else {
                Logbook::append(&path, r).map(|_| None)
            } {
                Ok(handle) => {
                    // MUST match `Logbook::append`'s own bytes (see the contract above),
                    // so it re-derives through the same own-log serializer.
                    written += tempo_core::logbook::adif_record_own_log(r).len() as u64;
                    receipts.extend(handle);
                }
                Err(e) => {
                    eprintln!("tempo: logbook append failed: {e}");
                    accountable = false;
                    succeeded = false;
                }
            }
        }
        self.last_log_mtime = match (accountable, before, log_file_stamp(&path)) {
            (true, Some((_, was)), Some((mtime, now))) if now == was + written => {
                Some((mtime, now))
            }
            _ => None,
        };
        succeeded.then_some(receipts)
    }

    /// THE way to persist the logbook: save, then record the file's fresh mtime
    /// so the recovery gate above doesn't re-parse our own write on the next
    /// stamp. Every full-log rewrite in this file funnels through here.
    #[allow(deprecated)] // SPEC-2 C19: the log.adi fallback (D1)
    fn save_log(&mut self, context: &str) {
        let Some(path) = self.log_path.clone() else {
            return;
        };
        match self.logbook.save(&path) {
            // The fingerprint comes from save() itself (statted pre-rename), so
            // a concurrent instance's rename can never be recorded as our write.
            Ok(stamp) => self.last_log_mtime = stamp,
            Err(e) => {
                eprintln!("tempo: {context} save failed: {e}");
                // Disk ≠ memory now: drop the gate so the next recovery
                // re-reads instead of trusting a stale fingerprint.
                self.last_log_mtime = None;
            }
        }
    }

    /// Two-instance freshness watcher: if the SHARED `log.adi`'s mtime moved since we last
    /// looked — i.e. the OTHER instance logged or confirmed something — fold its changes in and
    /// rebuild the worked-before/needs index. When nothing changed this is a single `stat`, so
    /// it is cheap to call on every Needed-board poll; that is what keeps a MONITORING radio
    /// (parked, not logging, so it never hits the stamp/save recovery path) from showing a DXCC
    /// as "needed" that the other radio just worked, with no save or restart required. Returns
    /// true when it actually re-read (so the caller can refresh anything derived downstream).
    pub fn sync_shared_log_if_changed(&mut self) -> bool {
        if self.store.is_some() {
            // Another window's commits, and — if the mirror has stopped because `log.adi` holds
            // something it cannot account for — that file, taken in so the mirror can resume.
            let reloaded = self.refresh_from_store(false);
            let took_in = self.take_in_refused_log_file();
            if reloaded || took_in {
                self.refresh_worked_index();
            }
            return reloaded || took_in;
        }
        let Some(path) = self.log_path.clone() else {
            return false;
        };
        // A missing file / stat error leaves us on last-good; the recovery owns
        // the mtime gate and records what it read.
        if std::fs::metadata(&path).and_then(|m| m.modified()).is_err() {
            return false;
        }
        if !self.recover_external_appends() {
            return false;
        }
        self.refresh_worked_index();
        true
    }

    /// If the mirror refused to replace `log.adi` because something other than Nexus wrote it
    /// mid-session — a 1.13 instance on the same folder — take that file in, so its contacts are
    /// in the log and the mirror can go back to keeping the file current. Whether it did.
    ///
    /// Reads `log.adi` under the engine lock, as the 1.13 path's recovery did; it happens only
    /// after a foreign write, and only on the freshness poll.
    fn take_in_refused_log_file(&mut self) -> bool {
        let Some(store) = self.store.as_ref() else {
            return false;
        };
        let status = store.mirror_status();
        if !status.foreign_write {
            return false;
        }
        let Some(path) = self.log_path.clone() else {
            return false;
        };
        let stamp = tempo_core::logbook::mirror::file_stamp(&path);
        // Only the file the mirror refused: one that has changed again since is refused again,
        // and taken in on the next poll — never replaced on the strength of an older read.
        if stamp.is_none() || stamp != status.foreign_stamp {
            return false;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            return false;
        };
        self.take_in_log_file(&String::from_utf8_lossy(&bytes), stamp);
        true
    }

    /// Edit an existing logbook entry (a correction — busted call, wrong band, etc).
    /// Sync-derived state is preserved by `Logbook::update_record`. Persists by
    /// rewriting the whole ADIF (an edit can't be an append). Returns false if
    /// `index` is out of range.
    #[allow(deprecated)] // SPEC-2 C16: an edit addressed by position
    pub fn update_qso(&mut self, index: usize, mut rec: QsoRecord) -> bool {
        // Keep country populated on edits (the edit form doesn't carry it).
        if rec.country.is_none() {
            if let Some(resolve) = &self.dxcc_resolve {
                rec.country = resolve(&rec.call);
            }
        }
        // Recover another instance's appends BEFORE applying the edit, so the
        // full-log rewrite below can't drop them (and so the pre-edit record is
        // still present to dedup against — no stale copy is re-added).
        self.recover_external_appends();
        let base = self.change_base();
        // A CALLSIGN correction is the one edit the services have to hear about again.
        // `Logbook::update_record` clears the upload stamps for exactly that reason — "the
        // services hold the OLD call, so clearing the upload stamps re-queues the corrected
        // QSO to every one of them" — but clearing a stamp queues nothing, and nothing else
        // ever re-scans for an unstamped record. So a busted call fixed in the log stayed
        // busted at QRZ/ClubLog/eQSL/… for good, with the stamps that were the only evidence
        // anything was owed now erased. Read the stored call BEFORE the update, under
        // `update_record`'s own trimmed, case-insensitive rule, so the two can never disagree
        // about what counts as a correction.
        let call_changed = self
            .logbook
            .records()
            .get(index)
            .is_some_and(|old| !rec.call.trim().eq_ignore_ascii_case(old.call.trim()));
        let ok = self.logbook.update_record(index, rec);
        if ok {
            self.persist_change(base, "update_qso");
            self.refresh_worked_index();
            if call_changed {
                // The STORED record, not the incoming payload: `update_record` merges the
                // fields the edit form does not carry (park refs, TIME_OFF, the split leg),
                // and the connectors must send the whole contact, not the form's half of it.
                if let Some(fixed) = self
                    .logbook
                    .records()
                    .get(index)
                    .map(|r| QsoRecord::clone(r))
                {
                    // Every leg: every stamp was just cleared, so every connector is owed.
                    // The disabled ones are dropped by the worker's own toggle check, the
                    // same way a freshly logged contact's are.
                    self.requeue_upload(fixed, crate::engine::upload_legs::ALL, 0);
                }
            }
        }
        ok
    }

    /// Record — or WITHDRAW — the operator's QSL-sent declaration for logbook entry `index`.
    /// `Some(via)` marks it sent (bureau/direct/electronic, dated now); `None` clears the mark,
    /// which until now had no path at all while the received side has taken a bool since #152.
    /// Never touches confirmation state in either direction. Persists by rewriting the ADIF —
    /// a clear MUST be written, since it carries the operator decision that keeps a later
    /// import from restoring the mark. Returns false if `index` is out of range.
    #[allow(deprecated)] // SPEC-2 C16: a mark addressed by position
    pub fn mark_qsl_sent(
        &mut self,
        index: usize,
        via: Option<tempo_core::logbook::QslVia>,
    ) -> bool {
        self.recover_external_appends();
        let base = self.change_base();
        let ok = self.logbook.mark_qsl_sent(index, via, now_unix_secs());
        if ok {
            self.persist_change(base, "mark_qsl_sent");
        }
        ok
    }

    /// Record whether a PAPER QSL card arrived for entry `index` (#152). Persists by rewriting
    /// the ADIF, and refreshes the worked index because a card is an award-eligible
    /// confirmation — the needs/awards model reads it.
    #[allow(deprecated)] // SPEC-2 C16: a mark addressed by position
    pub fn mark_qsl_card(&mut self, index: usize, received: bool) -> bool {
        self.recover_external_appends();
        let base = self.change_base();
        let ok = self.logbook.mark_qsl_card(index, received);
        if ok {
            self.persist_change(base, "mark_qsl_card");
            self.refresh_worked_index();
        }
        ok
    }

    /// Set — or REMOVE — the satellite tag on entry `index` (`PROP_MODE=SAT` + `SAT_NAME`).
    /// `Some(name)` tags the contact, `None` removes the tag; the name has already been
    /// gated against LoTW's accepted list by the command layer that owns the table.
    ///
    /// Persists by rewriting the ADIF — a removal MUST be written: the wrongly tagged record
    /// is what LoTW and the Satellite-VUCC fold are reading, and until the file says
    /// otherwise the contact keeps claiming a bird it was never worked through. Refreshes
    /// the worked index for the same reason `mark_qsl_card` does: `PROP_MODE=SAT` diverts a
    /// grid out of the per-band terrestrial sets into the band-independent satellite one, so
    /// the awards model has to be told. Returns false if `index` is out of range.
    #[allow(deprecated)] // SPEC-2 C16: a tag addressed by position
    pub fn set_sat_tag(&mut self, index: usize, sat_name: Option<&str>) -> bool {
        self.recover_external_appends();
        let base = self.change_base();
        let ok = self.logbook.set_sat_tag(index, sat_name);
        if ok {
            self.persist_change(base, "set_sat_tag");
            self.refresh_worked_index();
        }
        ok
    }

    /// Delete a logbook entry (a mis-logged contact). Persists by rewriting the
    /// ADIF. Returns false if `index` is out of range. Shifts later indices — the
    /// caller must reload the log afterward.
    #[allow(deprecated)] // SPEC-2 C16: a delete addressed by position
    pub fn delete_qso(&mut self, index: usize) -> bool {
        // Recover another instance's appends BEFORE the delete, so the rewrite
        // drops only THIS record (the deleted key is absent from our copy at save
        // time, so recovery can't re-add it) and keeps the other writer's QSOs.
        self.recover_external_appends();
        let base = self.change_base();
        let ok = self.logbook.delete(index);
        if ok {
            self.persist_change(base, "delete_qso");
            self.refresh_worked_index();
        }
        ok
    }

    /// Purge the ENTIRE logbook (operator-confirmed, destructive, irreversible).
    /// Clears every contact in memory, rewrites the ADIF file to an empty log, and
    /// recomputes the worked-entity/grid sets (so the roster B4 highlighting and
    /// the needs/awards model reset too). Returns the number of contacts removed.
    #[allow(deprecated)] // SPEC-2 C19: clears the in-memory log
    pub fn clear_logbook(&mut self) -> usize {
        let base = self.change_base();
        let n = self.logbook.clear();
        if n > 0 {
            self.persist_change(base, "clear_logbook");
            self.refresh_worked_index();
        }
        n
    }

    /// Import an external ADIF logbook: merge (deduped) into the persistent log,
    /// persist it, and return `(added, skipped, updated, total)` — where `updated`
    /// counts records ALREADY logged that the import upgraded. The next propagation
    /// snapshot derives real "needs" from the enlarged log (and roster B4
    /// highlighting updates).
    ///
    /// Two write paths, because the import has two effects. New contacts are
    /// APPENDED (cheap, and all an import used to do). But an import also upgrades
    /// records already in the log — the confirmations and credits a LoTW/eQSL/QRZ
    /// download restates — and an append cannot express a change to a record that
    /// is already in the file. Those need the full rewrite, or the confirmations
    /// live in memory until the next save and are lost outright if the app exits
    /// first.
    ///
    /// That second path is why this now recovers first, like every other full-log
    /// rewrite in this file. An append-only import was concurrency-safe by
    /// construction and was the one exemption from the contract above; the moment it
    /// could `rename()` a whole log over the file, the exemption stopped holding and
    /// a stale copy silently deleted a second instance's QSOs.
    ///
    /// ORDER — before the MERGE, not merely before the WRITE, matching every sibling
    /// site and the "BEFORE their mutation" requirement on
    /// [`Self::recover_external_appends`]. A confirmation report is about contacts
    /// already logged, and some of them may be logged only by the OTHER instance. Run
    /// first, and such a row matches the recovered record and confirms it. Run after,
    /// and the merge — looking at a log that does not contain it yet — reads the same
    /// row as a brand-new contact and logs it a second time. So the late order costs
    /// a duplicate and a lost confirmation even though it saves the file.
    ///
    /// A new row's COUNTRY and STATE are resolved BEFORE it joins the log, so it is appended
    /// complete — and an imported US contact carries its state from the moment it is imported,
    /// in the store as on every screen (SPEC-2 v3 D2-A). Companion mode imports one record per
    /// contact WSJT-X logs, and WSJT-X writes no COUNTRY: filled afterwards by the backfill,
    /// every such contact was an in-place write — a rewrite of the log's revision (a full
    /// reload for every log view) and a whole-log `save` of log.adi, fsync included, per
    /// contact.
    #[allow(deprecated)] // SPEC-2 C19: an import checks the whole log for what it already holds
    pub fn import_adif(&mut self, text: &str) -> (usize, usize, usize, usize) {
        self.recover_external_appends();
        let base = self.change_base();
        let (country, state) = (self.dxcc_resolve.as_deref(), self.state_resolve.as_deref());
        let (added, skipped, merged) = self
            .logbook
            .import_adif_with(text, |r| fill_with(r, country, state));
        if base.is_some() {
            // With the store the import and the rows it upgraded are ONE change of rows — no
            // whole-log rewrite, whatever the import touched. Its new rows arrived filled, and
            // the rows the log already held are the fill job's (D2-A), so nothing is backfilled
            // here. In companion mode this runs once per contact WSJT-X logs, from the radio
            // loop: it must not touch the disk, and it does not.
            self.persist_change(base, "import_adif");
        } else {
            if merged > 0 {
                self.save_log("import_adif"); // rewrites the whole log, `added` included
            } else {
                self.append_to_log(&added);
            }
            self.backfill_country();
        }
        self.refresh_worked_index();
        (added.len(), skipped, merged, self.logbook.len())
    }

    /// Reconcile a confirmation/credit report (ADIF — e.g. a LoTW export) INTO the
    /// existing log: monotonically upgrade matched QSOs' confirmation + credit
    /// (which a plain dedup-import would skip and lose), rewrite the ADIF file, and
    /// return the reconcile summary (newly confirmed/credited + unmatched orphans).
    #[allow(deprecated)] // SPEC-2 C19: a report merge plans on the whole log
    pub fn merge_lotw_report(&mut self, text: &str) -> tempo_core::reconcile::ReconcileSummary {
        self.recover_external_appends();
        let base = self.change_base();
        let summary = self.logbook.merge_report(text);
        self.last_lotw_reconcile = Some(summary.clone());
        self.persist_change(base, "merge_lotw_report");
        summary
    }

    /// Stamp POTA/SOTA park refs from a pota.app hunter/activator export onto matching
    /// existing QSOs (stamp-only: never creates records, never overwrites a ref — the
    /// reviewed-adds half is a separate feature). Returns (stamped, already, unmatched).
    #[allow(deprecated)] // SPEC-2 C19: a report merge plans on the whole log
    pub fn import_pota_log(&mut self, text: &str) -> (usize, usize, usize) {
        self.recover_external_appends();
        let base = self.change_base();
        let out = self.logbook.stamp_ota_refs(text);
        if out.0 > 0 {
            self.persist_change(base, "import_pota_log");
        }
        out
    }

    /// Merge a LoTW own-QSO report (`qso_qsl=no`) INTO the log: promote in-flight
    /// uploads (Pending / never-marked) to `Accepted` where LoTW confirms it holds
    /// your record — the step that turns a just-uploaded QSO into "waiting on the
    /// partner" (R2) and clears false "never uploaded" (R1) for out-of-band uploads.
    /// Persists the log on any change. Returns the count newly promoted.
    #[allow(deprecated)] // SPEC-2 C19: a report merge plans on the whole log
    pub fn merge_lotw_own_echo(&mut self, text: &str, when_unix: i64) -> usize {
        self.recover_external_appends();
        let base = self.change_base();
        let promoted = self.logbook.merge_own_echo(text, when_unix);
        if promoted > 0 {
            self.persist_change(base, "merge_lotw_own_echo");
        }
        promoted
    }

    /// Record a QRZ Logbook push outcome on the just-pushed QSO (`upload.qrz`), so
    /// the diagnostics can show "never uploaded to QRZ" (R1) / "QRZ upload bounced"
    /// (R9). Persists on change. Returns whether a record was stamped.
    #[allow(deprecated)] // SPEC-2 C16: the stamp finds its row in the whole log
    pub fn stamp_qrz_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) -> bool {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        self.recover_external_appends();
        let base = self.change_base();
        let changed = self.logbook.stamp_qrz_upload(pushed, status);
        if changed {
            self.persist_change(base, "stamp_qrz_upload");
        }
        changed
    }

    /// Record a ClubLog realtime push outcome on the just-pushed QSO
    /// (`upload.clublog`). Persists on change. Returns whether a record was stamped.
    #[allow(deprecated)] // SPEC-2 C16: the stamp finds its row in the whole log
    pub fn stamp_clublog_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) -> bool {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        self.recover_external_appends();
        let base = self.change_base();
        let changed = self.logbook.stamp_clublog_upload(pushed, status);
        if changed {
            self.persist_change(base, "stamp_clublog_upload");
        }
        changed
    }

    /// Record an eQSL ADIF-upload outcome on the just-pushed QSO (`upload.eqsl`).
    /// Persists on change. Returns whether a record was stamped.
    #[allow(deprecated)] // SPEC-2 C16: the stamp finds its row in the whole log
    pub fn stamp_eqsl_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) -> bool {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        self.recover_external_appends();
        let base = self.change_base();
        let changed = self.logbook.stamp_eqsl_upload(pushed, status);
        if changed {
            self.persist_change(base, "stamp_eqsl_upload");
        }
        changed
    }

    /// Merge an eQSL confirmation report into the log. Same generic reconcile path
    /// as [`Self::merge_lotw_report`]; the award-grade distinction lives in the
    /// ADIF (eQSL carries `EQSL_QSL_RCVD`, not `QSL_RCVD`/`LOTW_QSL_RCVD`), so an
    /// eQSL confirmation lands `confirmed` but NOT `award_confirmed` by construction.
    #[allow(deprecated)] // SPEC-2 C19: a report merge plans on the whole log
    pub fn merge_eqsl_report(&mut self, text: &str) -> tempo_core::reconcile::ReconcileSummary {
        self.recover_external_appends();
        let base = self.change_base();
        let summary = self.logbook.merge_report(text);
        self.last_eqsl_reconcile = Some(summary.clone());
        self.persist_change(base, "merge_eqsl_report");
        summary
    }

    /// Two-way QRZ Logbook sync: merge a QRZ **FETCH** ADIF (the operator's whole
    /// book) INTO the log. QRZ returns both QSOs the operator logged elsewhere (e.g.
    /// a phone app in the field) AND confirmation status, so this runs two passes:
    /// first import genuinely-new QSOs (deduped), then reconcile confirmations onto
    /// the QSOs already present. A QRZ-native confirmation (`APP_QRZLOG_STATUS`) lands
    /// `confirmed` but NOT `award_confirmed`, by construction of the `qrz` channel, so
    /// it can't inflate DXCC/WAS counts. Returns `(added, reconcile_summary)`.
    #[allow(deprecated)] // SPEC-2 C19: a report merge plans on the whole log
    pub fn merge_qrz_report(
        &mut self,
        text: &str,
    ) -> (usize, tempo_core::reconcile::ReconcileSummary) {
        self.recover_external_appends();
        // ONE consume-once pass: add the QSOs QRZ has that we lack AND upgrade
        // confirmations on the ones already present, keyed identically so a mode-
        // spelling difference (e.g. a phone QSO re-uploaded as USB vs our SSB) can't
        // double-log the same contact. A full save then captures both the appended
        // rows and the reconciled confirmations.
        let base = self.change_base();
        let before = self.logbook.len();
        let (added, summary) = self.logbook.merge_downloaded(text);
        self.last_qrz_reconcile = Some(summary.clone());
        if base.is_some() {
            // One change of rows: the merge's adds and upgrades. The merge appends the contacts
            // it adds, and each is filled here, before it is written (SPEC-2 v3 D2-A); the rows
            // the log already held are the fill job's.
            if self.logbook.len() > before {
                let (country, state) =
                    (self.dxcc_resolve.as_deref(), self.state_resolve.as_deref());
                for r in self
                    .logbook
                    .records_mut(OpClass::Upgrade)
                    .iter_mut()
                    .skip(before)
                {
                    fill_with(Arc::make_mut(r), country, state);
                }
            }
            self.persist_change(base, "merge_qrz_report");
        } else {
            self.save_log("merge_qrz_report");
            self.backfill_country();
        }
        self.refresh_worked_index();
        (added.len(), summary)
    }

    /// A clone of all logbook records (oldest-first / newest-last).
    #[allow(deprecated)] // SPEC-2 C17b: the whole log over IPC, deleted with get_log
    pub fn get_log(&self) -> Vec<QsoRecord> {
        self.logbook
            .records()
            .iter()
            .map(|r| QsoRecord::clone(r))
            .collect()
    }

    /// Run the silent match-failure diagnostics over the log (Phase 1a). `resolve`
    /// maps a callsign to its DXCC entity name (for R4d's US-family gate) — the
    /// command layer passes `propagation::dxcc::resolve`, keeping the entity table
    /// out of tempo-app. Reads the last LoTW + eQSL reconcile orphans (this session).
    ///
    /// A pass over the whole log, read from the store, with a DXCC lookup per contact (780 ms
    /// at 500,000), so a caller holding the engine lock takes [`Self::diagnostics_inputs`]
    /// instead and runs [`DiagnosticsInputs::diagnose`] after releasing it. This is the two in
    /// one breath, for a caller that holds no Engine guard.
    pub fn confirmation_diagnostics(
        &self,
        now: i64,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<tempo_core::diagnostics::DiagnosticsReport, String> {
        self.diagnostics_inputs()
            .diagnose(now, resolve)
            .map(|(report, _)| report)
    }

    /// The log's rows for a pass over them, taken under the lock this is called under and read
    /// after it is released — see [`crate::logstore::LogRows`]. The store's, whenever the store
    /// owns the log; on the 1.13 path, the log in memory, as a copy of pointers.
    ///
    /// The ONE reader of the in-memory log that the folds, lookups and sweeps share (SPEC-2 v3
    /// C14): every one of them reads through this, so the 1.13 path is served once, here.
    #[allow(deprecated)] // SPEC-2 C19: the log.adi fallback (D1) reads the rows in memory
    pub(crate) fn log_rows(&self) -> crate::logstore::LogRows {
        match &self.store {
            Some(store) => crate::logstore::LogRows::Store(store.reads()),
            None => crate::logstore::LogRows::Memory(self.logbook.records().to_vec()),
        }
    }

    /// What the confirmation diagnostics read: the log's rows ([`Self::log_rows`] — handles, or
    /// a copy of pointers) and the latest LoTW, eQSL and QRZ reconcile summaries. Cheap enough
    /// for the engine lock; the diagnosis itself reads and runs after it is released.
    pub fn diagnostics_inputs(&self) -> DiagnosticsInputs {
        DiagnosticsInputs {
            rows: self.log_rows(),
            // In this order — LoTW, eQSL, QRZ — as the diagnosis has always been handed them.
            recents: [
                &self.last_lotw_reconcile,
                &self.last_eqsl_reconcile,
                &self.last_qrz_reconcile,
            ]
            .into_iter()
            .flatten()
            .cloned()
            .collect(),
        }
    }

    /// Log indices (oldest-first) of QSOs not yet sent to LoTW: award-unconfirmed
    /// AND either never uploaded or a prior bounce. `UploadState` IS the per-QSO
    /// cursor — Pending/Accepted/Duplicate are excluded (don't re-send).
    #[allow(deprecated)] // SPEC-2 C16: LoTW's unsent rows by position
    pub fn lotw_unsent_indices(&self) -> Vec<usize> {
        self.logbook
            .records()
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.award_confirmed)
            .filter(|(_, r)| r.upload.lotw.as_ref().is_none_or(|s| !s.outcome.is_sent()))
            // LoTW matches on both operators' times agreeing (±30 min): a record
            // with NO known time can never match, so signing and sending it just
            // parks it at LoTW as unmatched forever — and, being re-sendable, it
            // kept the "Upload to LoTW (N)" count from ever clearing.
            .filter(|(_, r)| r.time_known)
            .map(|(i, _)| i)
            .collect()
    }

    /// Stamp `upload.lotw` on the given records after an upload attempt, then save.
    ///
    /// BY POSITION, so only for positions taken in the same hold of the engine lock (the
    /// operator's "already uploaded" declaration). An upload releases the lock while TQSL runs,
    /// and the positions it took may name other contacts by the time it is done: that is
    /// [`Self::stamp_lotw_batch`].
    #[allow(deprecated)] // SPEC-2 C16: a stamp addressed by position
    pub fn stamp_lotw_upload(
        &mut self,
        indices: &[usize],
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) {
        // Recover another instance's appends before the full-log rewrite; the
        // recovered records land at the end, so `indices` still address the same
        // rows.
        self.recover_external_appends();
        let base = self.change_base();
        // One classified write for the whole batch — a stamp nothing derived reads, and
        // taking it per row would move the revision once per stamped record.
        let records = self.logbook.records_mut(OpClass::Stamp);
        for &i in indices {
            if let Some(r) = records.get_mut(i) {
                Arc::make_mut(r).upload.lotw = Some(tempo_core::logbook::UploadStatus {
                    outcome,
                    when_unix,
                    detail,
                });
            }
        }
        self.persist_change(base, "lotw upload stamp");
    }

    /// The contacts at `indices` as a LoTW batch hands them to TQSL: each one's id, and its
    /// fingerprint as the upload serialises it. Taken in the same hold of the lock as the
    /// batch file, so the two describe one state of the log. A row without an id cannot be
    /// named later and is left out — every row the log holds carries one.
    #[allow(deprecated)] // SPEC-2 C16: LoTW's signed rows by position
    pub fn lotw_signed(&self, indices: &[usize]) -> Vec<LotwSigned> {
        let records = self.logbook.records();
        indices
            .iter()
            .filter_map(|&i| records.get(i))
            .filter_map(|r| {
                Some(LotwSigned {
                    id: r.id?,
                    fingerprint: lotw_fingerprint(r),
                })
            })
            .collect()
    }

    /// Stamp `upload.lotw` on the contacts of a batch TQSL signed — found BY ID, and only where
    /// the row is still the one that was signed.
    ///
    /// TQSL runs for tens of seconds with the engine lock released, and the log moves under it:
    /// the operator deletes or corrects a contact, another window's commit is folded in. A
    /// stamp addressed by position lands on whatever row slid into the gap — a contact LoTW
    /// never saw, marked sent, and so never uploaded at all. By id, a deleted contact is simply
    /// not found. A contact changed since it was signed is not stamped either: LoTW holds the
    /// version that was signed, so the row stays unsent and the next batch signs it as it now
    /// stands (LoTW dedupes what it already has). What is left unstamped is counted, for the
    /// caller to report.
    #[allow(deprecated)] // SPEC-2 C19: the stamp finds its rows in the whole log
    pub fn stamp_lotw_batch(
        &mut self,
        batch: &[LotwSigned],
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) -> LotwStamped {
        // Nothing is held by position across this, so the re-read may move rows — and should:
        // a contact another window deleted while TQSL ran is then gone before the stamp looks
        // for it, rather than lingering to be stamped and so written back into the store.
        if self.store.is_some() {
            self.refresh_from_store(false);
        } else {
            self.recover_external_appends();
        }
        let base = self.change_base();
        let mut signed: HashMap<tempo_core::logbook::RecordId, u64> =
            batch.iter().map(|s| (s.id, s.fingerprint)).collect();
        let mut report = LotwStamped::default();
        let mut hits = Vec::new();
        for (i, r) in self.logbook.records().iter().enumerate() {
            let Some(fingerprint) = r.id.and_then(|id| signed.remove(&id)) else {
                continue;
            };
            if lotw_fingerprint(r) == fingerprint {
                hits.push(i);
            } else {
                report.changed += 1;
            }
        }
        report.gone = signed.len();
        report.stamped = hits.len();
        if !hits.is_empty() {
            // One classified write for the whole batch, as `stamp_lotw_upload` makes.
            let records = self.logbook.records_mut(OpClass::Stamp);
            for i in hits {
                Arc::make_mut(&mut records[i]).upload.lotw =
                    Some(tempo_core::logbook::UploadStatus {
                        outcome,
                        when_unix,
                        detail,
                    });
            }
        }
        self.persist_change(base, "lotw upload stamp");
        report
    }

    /// Append a completed SSTV image to the session gallery (newest last),
    /// capped at [`SSTV_GALLERY_CAP`]. Decode-thread only.
    pub fn push_sstv_gallery(&mut self, entry: crate::dto::SstvGalleryEntry) {
        self.sstv_gallery.push(entry);
        if self.sstv_gallery.len() > SSTV_GALLERY_CAP {
            let excess = self.sstv_gallery.len() - SSTV_GALLERY_CAP;
            self.sstv_gallery.drain(0..excess);
        }
    }

    /// Attach a decoded FSK callsign ID to the newest gallery entry whose
    /// `path` matches (the image was just saved, so it's at/near the tail).
    /// Best-effort — a no-op if no entry matches (e.g. the gallery rolled past
    /// its cap before the burst decoded). Decode-thread only, like the other
    /// `sstv_gallery` mutators.
    /// Drop one image from the gallery by path. `true` when it was there. The FILE is the
    /// shell's business — this is only the in-memory index, and the two are kept in step by the
    /// single command that does both (`sstv_delete_image`), so they cannot drift the way the
    /// index and the directory used to.
    pub fn remove_sstv_gallery(&mut self, path: &str) -> bool {
        let before = self.sstv_gallery.len();
        self.sstv_gallery.retain(|e| e.path != path);
        self.sstv_gallery.len() != before
    }

    pub fn set_sstv_gallery_fsk_id(&mut self, path: &str, fsk_id: String) {
        if let Some(entry) = self.sstv_gallery.iter_mut().rev().find(|e| e.path == path) {
            entry.fsk_id = Some(fsk_id);
        }
    }

    /// Seed the session gallery from the persisted `gallery.json` (startup).
    pub fn load_sstv_gallery(&mut self, mut entries: Vec<crate::dto::SstvGalleryEntry>) {
        if entries.len() > SSTV_GALLERY_CAP {
            let excess = entries.len() - SSTV_GALLERY_CAP;
            entries.drain(0..excess);
        }
        self.sstv_gallery = entries;
    }

    /// The session SSTV gallery, oldest first.
    pub fn sstv_gallery(&self) -> &[crate::dto::SstvGalleryEntry] {
        &self.sstv_gallery
    }

    /// Adopt a corroborated NTP measurement (`tempo_net::sntp::measure`), sizing
    /// its hold window from this machine's own measured drift when the probe has
    /// one. Guard 3's 60 s ceiling is applied inside
    /// [`crate::clocksync::ClockState::publish`].
    pub fn publish_clock_offset(&mut self, offset_ms: i64, servers: u8, rate_ppm: Option<f64>) {
        self.clock
            .publish(offset_ms, servers, rate_ppm, std::time::Instant::now());
    }

    /// Drop the current offset. `reprobe` asks the probe thread to measure
    /// immediately rather than sleep out its interval — raised when the OS
    /// stepped the clock under us (resume from sleep), not when the operator
    /// merely turned the check off.
    pub fn clear_clock_offset(&mut self, reprobe: bool) {
        self.clock.clear(reprobe);
    }

    /// Take the pending re-probe request raised by [`Self::clear_clock_offset`].
    pub fn take_clock_reprobe(&mut self) -> bool {
        self.clock.take_reprobe()
    }

    /// The full clock picture for the UI: the held measurement (fresh or not)
    /// and any offset guard 3 refused to steer by.
    pub fn clock_state(&self) -> &crate::clocksync::ClockState {
        &self.clock
    }

    /// Record who owns this machine's clock (see [`Self::clock_owner_note`]).
    pub fn set_clock_owner_note(&mut self, note: String) {
        self.clock_owner_note = note;
    }

    /// The clock-ownership line, empty until a detection pass has run.
    pub fn clock_owner_note(&self) -> &str {
        &self.clock_owner_note
    }

    /// Set the offset directly, bypassing the probe. `Some` publishes it as a
    /// measurement taken now with the default hold window; `None` clears.
    ///
    /// ⚠️ Guard 3 still applies — a value beyond
    /// [`crate::clocksync::MAX_STEER_MS`] is refused here exactly as it is on the
    /// probe path, so a test cannot arrange a skew the shipped app would never
    /// steer by.
    pub fn set_clock_offset_ms(&mut self, ms: Option<i64>) {
        match ms {
            Some(ms) => self.publish_clock_offset(ms, 0, None),
            None => self.clear_clock_offset(false),
        }
    }

    /// The PC-clock-vs-UTC offset (ms) to steer by right now, `local − UTC`
    /// (positive = the PC clock is ahead of UTC). `None` when the NTP check is
    /// off, no round has ever agreed, the last measurement has aged out of its
    /// hold window, or guard 3 refused it. The radio loop subtracts this from the
    /// system clock so TX/RX slots land on the true UTC grid even when the OS
    /// clock is skewed.
    pub fn clock_offset_ms(&self) -> Option<i64> {
        self.clock.offset_ms(std::time::Instant::now())
    }

    /// Drain the WSJT-X-format ALL.TXT lines buffered since the last call (the shell
    /// appends them to the on-disk log). Empty when ALL.TXT logging is off.
    pub fn take_all_txt_pending(&mut self) -> Vec<String> {
        std::mem::take(&mut self.all_txt_pending)
    }

    /// Export the **general** logbook (Chat/QSO contacts, any mode) as ADIF or
    /// CSV. Independent of Field Day's contest log (`Engine::export_log`).
    /// `from_unix`/`to_unix` bound the QSO start time inclusively (#98); both
    /// `None` = the whole log, byte-identical to the unbounded export.
    #[allow(deprecated)] // SPEC-2 C15: an export
    pub fn export_logbook(
        &self,
        format: &str,
        from_unix: Option<u64>,
        to_unix: Option<u64>,
    ) -> String {
        match format.to_ascii_lowercase().as_str() {
            "csv" => self.logbook.csv_in_range(from_unix, to_unix),
            _ => self.logbook.adif_in_range(from_unix, to_unix),
        }
    }

    /// Distinct operators in the log (#25) — what a per-operator export offers to split by.
    #[allow(deprecated)] // SPEC-2 C15: an export's split
    pub fn log_operators(&self) -> Vec<String> {
        self.logbook.operators()
    }

    /// ADIF containing only `operator`'s contacts (#25).
    #[allow(deprecated)] // SPEC-2 C15: an export
    pub fn export_logbook_for_operator(&self, operator: &str) -> String {
        self.logbook.adif_for_operator(operator)
    }

    /// Distinct activations in the log — YOUR park × UTC day × the callsign it was worked
    /// under, newest first. What the per-activation export offers to split by, the way
    /// [`Self::log_operators`] drives the per-operator one.
    #[allow(deprecated)] // SPEC-2 C15: an export's split
    pub fn log_activations(&self) -> Vec<tempo_core::logbook::LoggedActivation> {
        self.logbook.activations()
    }

    /// ADIF containing only ONE activation's contacts — the three bounds an
    /// `Activation` carries, handed straight back.
    #[allow(deprecated)] // SPEC-2 C15: an export
    pub fn export_logbook_for_activation(
        &self,
        reference: &str,
        day_start_unix: u64,
        callsign: Option<&str>,
    ) -> String {
        self.logbook
            .adif_for_activation(reference, day_start_unix, callsign)
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;
    use tempo_core::logbook::QsoRecord;

    fn rec(call: &str, band: &str, grid: &str) -> QsoRecord {
        QsoRecord {
            id: None,
            call: call.into(),
            grid: Some(grid.into()),
            country: None,
            state: None,
            band: band.into(),
            freq_mhz: 14.074,
            freq_rx_mhz: None,
            mode: "FT8".into(),
            rst_sent: None,
            rst_rcvd: None,
            name: None,
            qth: None,
            comment: None,
            notes: None,
            tx_power: None,
            when_unix: 1_700_000_000,
            time_off_unix: None,
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: Default::default(),
            qsl_sent: Default::default(),
            credit_granted: Vec::new(),
            credit_submitted: Vec::new(),
            upload: Default::default(),
            ota: Default::default(),
            time_known: true,
            dxcc: None,
            prop_mode: None,
            sat_name: None,
            operator: None,
            my_grid: None,
            my_rig: None,
            station_callsign: None,
            extra: Vec::new(),
            contest: None,
        }
    }

    /// The worked index is EXTENDED, not rebuilt, when the log only grew: one DXCC lookup per
    /// logged contact instead of one per row of the whole log, under the engine lock, on every
    /// contact. The extended index must equal a rebuild from scratch, and anything but an
    /// append must still rebuild — or a NEW DXCC / NEW GRID / NEW PARK badge lies.
    #[test]
    fn an_appended_contact_extends_the_worked_index_to_exactly_a_rebuild() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        fn counting(
            lookups: Arc<AtomicUsize>,
        ) -> impl Fn(&str) -> Option<String> + Send + Sync + 'static {
            move |call| {
                lookups.fetch_add(1, Ordering::Relaxed);
                call.get(..2).map(str::to_string)
            }
        }
        type Index = (
            HashSet<(String, String)>,
            HashSet<(String, String)>,
            HashSet<(String, String)>,
            HashSet<String>,
            HashMap<String, u64>,
        );
        fn index(sc: &StationCore) -> Index {
            (
                sc.worked_grids.clone(),
                sc.worked_entities.clone(),
                sc.confirmed_entities.clone(),
                sc.worked_parks.clone(),
                sc.last_worked.clone(),
            )
        }
        fn rebuilt(sc: &StationCore) -> Index {
            let mut fresh = StationCore::new();
            fresh.set_dxcc_resolver(counting(Arc::new(AtomicUsize::new(0))));
            for r in sc.logbook.records() {
                fresh.logbook.add(r.as_ref().clone());
            }
            fresh.refresh_worked_index();
            index(&fresh)
        }
        let lookups = Arc::new(AtomicUsize::new(0));
        let mut sc = StationCore::new();
        sc.set_dxcc_resolver(counting(lookups.clone()));

        let mut confirmed = rec("DL1ABC", "20m", "JO31");
        confirmed.award_confirmed = true;
        let mut park = rec("K1ABC", "40m", "FN42");
        park.ota.their_ref = Some("US-0001".into());
        park.when_unix += 60;
        let rows = [
            rec("W1AW", "20m", "FN31PR"),
            confirmed,
            park,
            rec("W1AW", "40m", "FN31"),
        ];
        for (i, r) in rows.into_iter().enumerate() {
            let before = lookups.load(Ordering::Relaxed);
            sc.logbook.add(r);
            sc.refresh_worked_index();
            assert_eq!(
                lookups.load(Ordering::Relaxed) - before,
                1,
                "row {i}: one lookup for one appended row"
            );
            assert_eq!(
                index(&sc),
                rebuilt(&sc),
                "row {i}: the extended index IS the rebuild"
            );
        }

        // A rewrite rebuilds: a deleted row's slots leave the index.
        assert!(sc.logbook.delete(1));
        let before = lookups.load(Ordering::Relaxed);
        sc.refresh_worked_index();
        assert_eq!(
            lookups.load(Ordering::Relaxed) - before,
            3,
            "a delete re-indexes every row"
        );
        assert_eq!(index(&sc), rebuilt(&sc));
        assert!(
            sc.confirmed_entities.is_empty(),
            "the deleted row was the only confirmation"
        );
    }

    #[test]
    fn a_six_char_logged_grid_matches_a_four_char_decode() {
        // The bug this pins: QRZ/LoTW imports log "FN31PR" while a decode carries "FN31",
        // so the index and the lookup never met and NEW GRID fired forever on every such
        // square. Both sides now normalize to the 4-char square grids are awarded at.
        let mut sc = StationCore::new();
        sc.logbook.add(rec("W1AW", "20m", "FN31PR"));
        sc.refresh_worked_index();

        assert!(
            sc.grid_worked_on("FN31", "20m"),
            "a 6-char logged grid must satisfy a 4-char decode on the same band"
        );
        // And the per-band rule still holds on top of it.
        assert!(
            !sc.grid_worked_on("FN31", "2m"),
            "worked on 20m only — still NEW on 2m"
        );
    }

    #[test]
    fn a_two_fer_hunt_marks_both_parks_worked() {
        // NEW PARK asks whether a reference is anywhere on the hunter side of the log. A two-fer
        // record carries both parks in one field, and indexed whole it answered "never logged"
        // for both — a NEW PARK badge on a park the operator had just worked.
        let mut sc = StationCore::new();
        let mut r = rec("K1ABC", "20m", "FN31");
        r.ota.their_ref = Some("US-0001,US-0002".into());
        sc.logbook.add(r);
        sc.refresh_worked_index();
        assert!(sc.park_worked("US-0001"), "the first park");
        assert!(sc.park_worked("us-0002"), "…and the second");
        assert!(
            !sc.park_worked("US-0003"),
            "control: a park outside the pair"
        );
    }

    #[test]
    fn the_last_qso_with_a_station_is_indexed_by_base_call() {
        // The Spots panel asks the log one question per row: when did I last work this station,
        // on any band or mode? The log writes the call as it was worked — W6A/P for a
        // special-event portable — while the cluster spots W6A.
        let mut sc = StationCore::new();
        sc.refresh_worked_index();
        assert_eq!(sc.last_worked_unix("W6A"), None, "control: an empty log");

        let mut late = rec("W6A/P", "40m", "DM04");
        late.when_unix = 5_000;
        let mut early = rec("w6a", "20m", "DM04");
        early.when_unix = 1_000;
        // The later contact goes in FIRST, so "latest wins" cannot be "last row wins".
        sc.logbook.add(late);
        sc.logbook.add(early);
        sc.refresh_worked_index();
        assert_eq!(
            sc.last_worked_unix("W6A"),
            Some(5_000),
            "the most recent of two QSOs, on any band"
        );
        assert_eq!(
            sc.last_worked_unix("w6a/p"),
            Some(5_000),
            "the portable form names the same station"
        );
        assert_eq!(sc.last_worked_unix("K1ABC"), None, "a call never worked");

        // Deleting the later contact falls back to the earlier one — the map is rebuilt,
        // not accumulated.
        sc.logbook.delete(0);
        sc.refresh_worked_index();
        assert_eq!(sc.last_worked_unix("W6A"), Some(1_000));
    }

    #[test]
    fn a_malformed_grid_never_counts_as_worked() {
        let mut sc = StationCore::new();
        sc.logbook.add(rec("W1AW", "20m", "FN"));
        sc.refresh_worked_index();
        assert!(
            !sc.grid_worked_on("FN", "20m"),
            "a 2-char fragment is not a square"
        );
    }

    /// 57bd9dba put a `recover_external_appends` in front of `import_adif` — right,
    /// and it stays — but the APPEND branch below it left the freshness fingerprint
    /// pointing at the file as it was BEFORE our own append. So the gate missed on
    /// every later call, and companion mode (one `import_adif` per QSO WSJT-X logs)
    /// re-parsed the whole log from disk on every contact: measured 132.7 ms per QSO
    /// against a 26,007-record / 3.67 MB log, forever, not once.
    #[test]
    fn our_own_import_append_leaves_the_recovery_gate_shut() {
        use tempo_core::logbook::{adif_header, adif_record_own_log};
        let path =
            std::env::temp_dir().join(format!("nexus_append_stamp_{}.adi", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            format!(
                "{}{}",
                adif_header(),
                adif_record_own_log(&rec("W1AW", "20m", "FN31"))
            ),
        )
        .unwrap();

        let mut sc = StationCore::new();
        sc.set_log_path(path.clone());
        assert!(
            sc.recover_external_appends(),
            "the first look reads the file"
        );
        assert!(
            !sc.recover_external_appends(),
            "...and shuts the gate behind it"
        );

        // A companion-logged QSO: one contact we lack, nothing already held to
        // upgrade, so the append branch runs and not the full rewrite.
        let (added, _, merged, _) = sc.import_adif(
            "<EOH>\n<CALL:5>K5XYZ<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260804<TIME_ON:6>120000<EOR>\n",
        );
        assert_eq!((added, merged), (1, 0), "the append path, not the rewrite");

        assert!(
            !sc.recover_external_appends(),
            "our own append must not reopen the gate — every later call re-parses the whole log"
        );
        assert_eq!(
            sc.logbook.len(),
            2,
            "and nothing was re-read or double-counted"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The other half, and it is the half that must never be traded for the first:
    /// an append may carry the fingerprint forward ONLY while we can account for
    /// every byte on disk. Here a second instance wrote after our last look and we
    /// append without looking again — exactly the `Engine::log_qso` shape, which has
    /// no recovery in front of it. Stamping there would record a file we do not hold,
    /// the gate would skip, and the next full rewrite would delete the other
    /// instance's QSO: the fault 57bd9dba exists to prevent. The stamp must be
    /// DROPPED instead, so the next look re-reads.
    #[test]
    fn an_append_onto_a_file_that_moved_under_us_drops_the_fingerprint() {
        use tempo_core::logbook::{adif_header, adif_record_own_log};
        let path =
            std::env::temp_dir().join(format!("nexus_append_stale_{}.adi", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            format!(
                "{}{}",
                adif_header(),
                adif_record_own_log(&rec("W1AW", "20m", "FN31"))
            ),
        )
        .unwrap();

        let mut sc = StationCore::new();
        sc.set_log_path(path.clone());
        assert!(sc.recover_external_appends(), "we hold W1AW, gate shut");

        // Instance A appends a contact we never see in memory.
        Logbook::append(&path, &rec("W3CCC", "40m", "IO91")).unwrap();

        // We log our own contact — memory first, then the file, as log_qso does, carrying the
        // minted id back onto the copy we hand the writer so the two are the same record.
        let mut k = rec("K5XYZ", "20m", "FN31");
        k.id = Some(sc.logbook.add(k.clone()));
        sc.append_to_log(std::slice::from_ref(&k));
        assert!(
            sc.last_log_mtime.is_none(),
            "a file we cannot account for must not be stamped as ours"
        );
        assert!(
            sc.recover_external_appends(),
            "so the next look re-reads and folds A's QSO in"
        );

        // ...and the full rewrite that follows keeps it.
        sc.save_log("test");
        let on_disk = Logbook::load(&path);
        let calls: Vec<&str> = on_disk.records().iter().map(|r| r.call.as_str()).collect();
        assert!(
            calls.contains(&"W3CCC"),
            "another instance's append survives our rewrite (on disk: {calls:?})"
        );
        assert_eq!(on_disk.len(), 3, "...and nothing is double-logged");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn freshness_watcher_folds_in_another_instances_appends_and_no_ops_when_unchanged() {
        use tempo_core::logbook::{adif_header, adif_record_own_log};
        let path =
            std::env::temp_dir().join(format!("nexus_sync_watcher_{}.adi", std::process::id()));
        let write = |recs: &[QsoRecord]| {
            let mut s = adif_header();
            for r in recs {
                s.push_str(&adif_record_own_log(r));
            }
            std::fs::write(&path, s).unwrap();
        };
        // The shared log starts with just QSO X.
        let x = rec("DL1ABC", "20m", "JO31");
        write(std::slice::from_ref(&x));
        let mut sc = StationCore::new();
        sc.log_path = Some(path.clone());

        // First look (last mtime = None) folds X in and indexes it.
        assert!(
            sc.sync_shared_log_if_changed(),
            "first look reads the shared log"
        );
        assert_eq!(sc.logbook.len(), 1);
        assert!(sc.grid_worked_on("JO31", "20m"), "X is now worked-before");

        // The OTHER instance appends QSO Y. Force the gate (mtime granularity is coarse in a
        // fast test); the point under test is the reconcile+refresh, which the gate triggers.
        let y = rec("JA1XYZ", "40m", "PM95");
        write(&[x, y]);
        sc.last_log_mtime = None;
        assert!(sc.sync_shared_log_if_changed(), "a changed log is re-read");
        assert_eq!(
            sc.logbook.len(),
            2,
            "the other instance's new QSO is folded in"
        );
        assert!(
            sc.grid_worked_on("PM95", "40m"),
            "Y is now worked-before without a restart"
        );

        // Nothing changed → a cheap no-op (stat only), so it's safe on every Needed-board poll.
        assert!(
            !sc.sync_shared_log_if_changed(),
            "unchanged mtime → no re-read"
        );

        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod diagnostics_tests {
    //! The confirmation diagnosis runs with the engine lock released (SPEC-2 v3's C12): the
    //! engine hands out what it reads ([`StationCore::diagnostics_inputs`]) and the diagnosis
    //! runs on that ([`DiagnosticsInputs::diagnose`]). The report must be the one the old body
    //! made, reproduced below verbatim as the oracle.
    use super::*;
    use tempo_core::diagnostics::DiagnosticsReport;
    use tempo_core::reconcile::{OrphanConfirmation, ReconcileSummary};

    /// Award-confirmed, never uploaded, one to be told about three ways, eQSL-only, one
    /// uploaded an hour before `NOW` (lag, not yet a failure) and one uploaded long before it
    /// (waiting on the partner).
    const LOG: &str = "\
        <CALL:5>JA1AA<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260101<TIME_ON:6>010000<LOTW_QSL_RCVD:1>Y<EOR>\n\
        <CALL:5>DL1AB<BAND:3>40m<MODE:2>CW<QSO_DATE:8>20260102<TIME_ON:6>020000<EOR>\n\
        <CALL:5>W1ABC<BAND:3>20m<MODE:3>SSB<QSO_DATE:8>20260104<TIME_ON:6>030000<STATE:2>MA<EOR>\n\
        <CALL:6>PY2ABC<BAND:3>10m<MODE:3>FT8<QSO_DATE:8>20260106<TIME_ON:6>050000<EQSL_QSL_RCVD:1>Y<EOR>\n\
        <CALL:5>K5XYZ<BAND:3>40m<MODE:3>FT8<QSO_DATE:8>20260109<TIME_ON:6>230000<LOTW_QSL_SENT:1>Y<EOR>\n\
        <CALL:5>G4ABC<BAND:3>17m<MODE:4>RTTY<QSO_DATE:8>20251201<TIME_ON:6>070000<LOTW_QSL_SENT:1>Y<EOR>\n";

    /// 2026-01-10 00:00 UTC.
    const NOW: i64 = 1_768_003_200;

    /// A stand-in for the DXCC table, which tempo-app cannot name.
    fn entity(call: &str) -> Option<String> {
        let name = match call.as_bytes().first()? {
            b'W' | b'K' => "United States",
            b'J' => "Japan",
            b'D' => "Germany",
            b'P' => "Brazil",
            _ => return None,
        };
        Some(name.to_string())
    }

    /// A confirmation of the W1ABC contact that names the wrong band.
    fn orphan(band: &str) -> ReconcileSummary {
        ReconcileSummary {
            orphans: vec![OrphanConfirmation {
                call: "W1ABC".into(),
                band: band.into(),
                mode: "Phone".into(),
                when_unix: 1_767_495_600,
                reason: String::new(),
            }],
            ..Default::default()
        }
    }

    /// `StationCore::confirmation_diagnostics` as it was before C12, verbatim.
    fn old_diagnosis(
        sc: &StationCore,
        now: i64,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> DiagnosticsReport {
        let records = sc.logbook.records();
        let entities: Vec<Option<String>> = records.iter().map(|r| resolve(&r.call)).collect();
        let mut recents: Vec<&tempo_core::reconcile::ReconcileSummary> = Vec::new();
        if let Some(s) = &sc.last_lotw_reconcile {
            recents.push(s);
        }
        if let Some(s) = &sc.last_eqsl_reconcile {
            recents.push(s);
        }
        if let Some(s) = &sc.last_qrz_reconcile {
            recents.push(s);
        }
        tempo_core::diagnostics::diagnose(
            records,
            &entities,
            &recents,
            now,
            &tempo_core::diagnostics::DiagCfg::default(),
        )
    }

    /// ★ The diagnosis made from the handed-out inputs is the old one: the same rows in the
    /// same order, the same entity per row, the same clock, and the three services' reconcile
    /// summaries in the same order. The three disagree on purpose — each names a different
    /// wrong band for one contact, and the diagnosis keeps the first it is handed — so a
    /// reordering changes the report.
    #[test]
    fn the_diagnosis_off_the_lock_is_the_one_the_old_body_made() {
        let mut sc = StationCore::new();
        sc.import_adif(LOG);
        assert_eq!(sc.logbook.len(), 6, "premise: every contact imported");
        sc.last_lotw_reconcile = Some(orphan("15m"));
        sc.last_eqsl_reconcile = Some(orphan("17m"));
        sc.last_qrz_reconcile = Some(orphan("12m"));

        let old = old_diagnosis(&sc, NOW, entity);
        let old_text = format!("{old:?}");
        assert!(
            old_text.contains("expected: \"15m\""),
            "premise: the first summary's claim is the one kept"
        );
        assert!(
            old.pending_lag > 0 && old.waiting_on_partner > 0,
            "premise: the clock reaches the report, both ways"
        );
        assert!(
            !old.one_away.is_empty(),
            "premise: the entity lookup reaches the report: {old_text}"
        );
        assert!(
            old.diagnoses.len() > 1,
            "premise: several rows are diagnosed"
        );

        let inputs = sc.diagnostics_inputs();
        assert_eq!(inputs.log_len(), Ok(6));
        let (report, diagnosed) = inputs.diagnose(NOW, entity).expect("the log reads");
        assert_eq!(diagnosed, 6, "every contact diagnosed");
        assert_eq!(format!("{report:?}"), old_text);
        assert_eq!(
            format!(
                "{:?}",
                sc.confirmation_diagnostics(NOW, entity)
                    .expect("the log reads")
            ),
            old_text,
            "the two-in-one-breath form is the same diagnosis"
        );
    }

    /// ★ POSITIVE CONTROL: the diagnosis run while the engine lock is held is refused.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(
        expected = "io_fence: the confirmation diagnostics is a pass over the whole log"
    )]
    fn the_diagnosis_under_the_engine_lock_is_refused() {
        let engine = std::sync::Mutex::new(crate::engine::Engine::new("KD9TAW", "EN52", 0));
        let eng = crate::engine::engine_lock(&engine);
        let _ = eng.confirmation_diagnostics(NOW, entity);
    }
}
