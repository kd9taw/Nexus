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

use tempo_core::logbook::hot::{pairs_between, HotIndex, HotKeys, RowPair};
use tempo_core::logbook::writer::Change;
use tempo_core::logbook::{
    LogOp, Logbook, Minter, OpClass, QsoEdit, QsoRecord, RecordId, RowAfter, UploadService,
    Watermarks, WorkedSince,
};

use crate::logstore::{LogStore, Opened, SessionRows};

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

impl LotwSigned {
    /// The contact `r` as a batch hands it to TQSL: its id and its fingerprint. `None` for a row
    /// with no id, which could not be named later — every row the log holds carries one.
    pub fn of(r: &QsoRecord) -> Option<LotwSigned> {
        Some(LotwSigned {
            id: r.id?,
            fingerprint: lotw_fingerprint(r),
        })
    }
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
    /// Contacts whose result could not be recorded at all: the logbook could not be read, or
    /// kept changing under the stamp ([`RowRefusal::Busy`]). They stay unsent, and the next
    /// batch signs them again (LoTW dedupes what it already has).
    pub unrecorded: usize,
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
        let (report, rows, _) = self.diagnosis(now, resolve)?;
        Ok((report, rows.len()))
    }

    /// [`Self::diagnose`] for the desktop: every contact the report points at named by its id,
    /// and each diagnosed contact shown as its list shows it (SPEC-2 v2 §3, C17a), so the Awards
    /// view asks for a row and uploads by id with no log held. The Remote's report is sent as
    /// [`Self::diagnose`] makes it: its page admits no key it does not know.
    ///
    /// ⚠️ It reads the store: never under the Engine lock.
    pub fn diagnose_named(
        &self,
        now: i64,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<crate::dto::DiagnosticsReportDto, String> {
        let (report, rows, ids) = self.diagnosis(now, resolve)?;
        let mut named = crate::dto::DiagnosticsReportDto::from(report);
        named.name_rows(&rows, &ids);
        Ok(named)
    }

    /// The one pass both make: the report, and what it read of each contact — its row and its
    /// id — in log order, where every index in the report points.
    #[allow(clippy::type_complexity)] // one pass's three results, taken apart by its two callers
    fn diagnosis(
        &self,
        now: i64,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<
        (
            tempo_core::diagnostics::DiagnosticsReport,
            Vec<tempo_core::diagnostics::DiagRow>,
            Vec<Option<RecordId>>,
        ),
        String,
    > {
        tempo_core::logbook::io_fence::whole_log_off_engine_lock("the confirmation diagnostics");
        let mut rows = Vec::new();
        let mut ids = Vec::new();
        let mut entities = Vec::new();
        self.rows
            .each(
                DIAGNOSIS,
                tempo_core::logbook::sqlite::Scope::All,
                tempo_core::logbook::sqlite::Order::Log,
                &mut |r| {
                    entities.push(resolve(&r.call));
                    rows.push(tempo_core::diagnostics::DiagRow::from(r));
                    ids.push(r.id);
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
        Ok((report, rows, ids))
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

/// Whether a contact is owed to LoTW: award-unconfirmed AND either never uploaded or a prior
/// bounce. `UploadState` IS the per-QSO cursor — Pending/Accepted/Duplicate are excluded (don't
/// re-send).
///
/// LoTW matches on both operators' times agreeing (±30 min): a record with NO known time can
/// never match, so signing and sending it just parks it at LoTW as unmatched forever — and,
/// being re-sendable, it kept the "Upload to LoTW (N)" count from ever clearing.
///
/// THE rule, for the log in memory ([`StationCore::lotw_unsent_indices`]) and the store
/// ([`lotw_unsent`]) alike.
pub(crate) fn owed_to_lotw(r: &QsoRecord) -> bool {
    !r.award_confirmed
        && r.upload.lotw.as_ref().is_none_or(|s| !s.outcome.is_sent())
        && r.time_known
}

/// What [`owed_to_lotw`] reads of each contact: the four confirmation channels (which
/// `award_confirmed` is read from), whether its time is known, and the upload stamps.
const OWED_TO_LOTW: tempo_core::logbook::sqlite::Narrow = tempo_core::logbook::sqlite::Narrow {
    columns: &[
        "time_known",
        "qsl_card_rcvd_raw",
        "lotw_rcvd_raw",
        "eqsl_rcvd_raw",
        "qrz_status_raw",
    ],
    uploads: true,
};

/// The contacts owed to LoTW ([`owed_to_lotw`]), whole, in log order — the default batch an
/// upload signs, read from the store (SPEC-2 v3 C15) with the Engine lock released: a narrow
/// pass names them, and they are read whole by id.
///
/// ⚠️ It reads the store: never under the Engine lock. Take `rows` ([`StationCore::log_rows`])
/// under it. A contact another change removes between the two reads is simply not in the
/// batch; a change still on its way to the store is in the next batch.
pub fn lotw_unsent(rows: &crate::logstore::LogRows) -> Result<Vec<QsoRecord>, String> {
    use std::ops::ControlFlow;
    use tempo_core::logbook::sqlite::{Order, Scope};
    let mut owed = Vec::new();
    rows.each(OWED_TO_LOTW, Scope::All, Order::Log, &mut |r| {
        if let Some(id) = r.id.filter(|_| owed_to_lotw(r)) {
            owed.push(id);
        }
        ControlFlow::Continue(())
    })
    .map_err(|e| e.to_string())?;
    let (whole, _) = rows.rows_by_ids(&owed).map_err(|e| e.to_string())?;
    // The rows as they stand at the second read: one a change settled meanwhile is not owed.
    Ok(whole.into_iter().filter(owed_to_lotw).collect())
}

/// The contacts `ids` names, whole and read from the store, in the order `ids` names them — what
/// a LoTW batch chosen by id signs, as the log in memory answers the same ids: a contact the log
/// no longer holds is left out, and one named twice is there twice.
///
/// ⚠️ It reads the store: never under the Engine lock, as [`lotw_unsent`].
pub fn rows_named(
    rows: &crate::logstore::LogRows,
    ids: &[RecordId],
) -> Result<Vec<QsoRecord>, String> {
    let (found, _) = rows.rows_by_ids(ids).map_err(|e| e.to_string())?;
    let by_id: HashMap<RecordId, QsoRecord> =
        found.into_iter().filter_map(|r| Some((r.id?, r))).collect();
    Ok(ids.iter().filter_map(|id| by_id.get(id).cloned()).collect())
}

/// Whether any contact is owed to LoTW — the automatic batch's question before it starts TQSL,
/// answered from the store at the first one owed.
///
/// ⚠️ It reads the store: never under the Engine lock, as [`lotw_unsent`].
pub fn lotw_owed(rows: &crate::logstore::LogRows) -> Result<bool, String> {
    use std::ops::ControlFlow;
    use tempo_core::logbook::sqlite::{Order, Scope};
    let mut any = false;
    rows.each(OWED_TO_LOTW, Scope::All, Order::Log, &mut |r| {
        any = owed_to_lotw(r);
        if any {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .map_err(|e| e.to_string())?;
    Ok(any)
}

/// The ADIF upload payload for TQSL: the header, then `batch` in its order.
///
/// In ADIF-location mode (`adif_location`), each record is stamped with STATION_CALLSIGN +
/// MY_GRIDSQUARE so TQSL can sign from the ADIF (no named `-l` location). Named-location mode is
/// byte-identical to before (no MY_ fields), so existing uploads are unchanged.
///
/// `call` is the FALLBACK, not the answer: `adif_record_with_station` skips its own stamp when
/// the record carries a STATION_CALLSIGN of its own (which `adif_record` has already emitted),
/// so a batch uploaded after the operator sets their home call back still signs each contact
/// under the call that made it. Records written before that stamp existed carry none and sign
/// from the live setting, exactly as they always did.
pub fn lotw_batch_adif(batch: &[QsoRecord], adif_location: bool, call: &str, grid: &str) -> String {
    let mut out = tempo_core::logbook::adif_header();
    for r in batch {
        if adif_location {
            out.push_str(&tempo_core::logbook::adif_record_with_station(
                r, call, grid,
            ));
        } else {
            out.push_str(&tempo_core::logbook::adif_record(r));
        }
    }
    out
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

/// Up to this many rows, a debug build asks the old scan as well as the hot index for the two
/// answers a logged contact is built from — the duplicate guard and the partner's logged grid —
/// and stops on any difference (see [`StationCore::is_duplicate`]).
#[cfg(debug_assertions)]
const DUAL_EXECUTION_ROWS: usize = 1_000;

/// What the hot index needs from the station: the band a badge is keyed on, and the live DXCC
/// resolver. The store's launch build keys the index the same way
/// ([`crate::logstore::HotBuild`]).
pub(crate) struct StationKeys<'a>(pub(crate) Option<&'a DxccResolve>);

/// A callsign → DXCC entity resolver, as [`StationCore::dxcc_resolve`] holds it.
pub type DxccResolve = dyn Fn(&str) -> Option<String> + Send + Sync;

impl HotKeys for StationKeys<'_> {
    fn band_key(&self, band: &str) -> String {
        band_key(band)
    }
    fn entity(&self, call: &str) -> Option<String> {
        self.0.and_then(|resolve| resolve(call))
    }
}

/// The hot index ([`tempo_core::logbook::hot`]), caught up with the log, held for as long as
/// this lives — see [`StationCore::hot`]. Asked in the station's own terms: a band is the label
/// as logged or dialled, keyed here the way the index was.
pub(crate) struct Hot<'a>(std::sync::MutexGuard<'a, HotIndex>);

impl std::ops::Deref for Hot<'_> {
    type Target = HotIndex;
    fn deref(&self) -> &HotIndex {
        &self.0
    }
}

impl Hot<'_> {
    /// Is this grid already worked ON THIS BAND? (`band` is the raw band label — canonicalized
    /// here.) A grid worked only on another band reads as NOT worked, which is the point:
    /// per-band is how grids are awarded.
    pub(crate) fn grid_worked_on(&self, grid: &str, band: &str) -> bool {
        self.0.grid_worked_on(grid, &band_key(band))
    }

    /// Is this DXCC entity already worked ON THIS BAND? Per band, like
    /// [`grid_worked_on`](Self::grid_worked_on).
    pub(crate) fn entity_worked_on(&self, entity: &str, band: &str) -> bool {
        self.0.entity_worked_on(entity, &band_key(band))
    }

    /// Is this DXCC entity CONFIRMED (award-grade) ON THIS BAND? Drives the decode panes'
    /// hide-confirmed filter (F4MQS).
    pub(crate) fn entity_confirmed_on(&self, entity: &str, band: &str) -> bool {
        self.0.entity_confirmed_on(entity, &band_key(band))
    }
}

/// Whether two DXCC resolvers are the same one — both absent, or one shared `Arc`. A hot index
/// keyed by one is the index of the other only then: two closures cannot be compared.
fn same_resolver(a: &Option<Arc<DxccResolve>>, b: &Option<Arc<DxccResolve>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

/// The hot index behind its lock, for a write through `&mut` — which no guard can be holding. A
/// panic while one was held may have left it half-changed: it is let go of, and rebuilt at its
/// next catch-up.
fn hot_mut(hot: &mut std::sync::Mutex<HotIndex>) -> &mut HotIndex {
    match hot.get_mut() {
        Ok(hot) => hot,
        Err(poisoned) => {
            let hot = poisoned.into_inner();
            hot.invalidate();
            hot
        }
    }
}

/// What a connector's push must read of a row to tell whether it is the contact pushed
/// ([`tempo_core::logbook::same_push_key`]).
const PUSH_KEY: tempo_core::logbook::sqlite::Narrow = tempo_core::logbook::sqlite::Narrow {
    columns: &["call", "band", "mode", "when_unix"],
    uploads: false,
};

/// The row a connector's push names, as `plan` reads the log (SPEC-2 v3 C16's rule, read off
/// the store — C19):
///
/// ⛔ **A push of a row the log holds NAMES that row, by its id, and the stamp lands there** —
/// only while it is still the contact that was pushed (the same push key): one corrected since
/// holds a contact the service never received, and one deleted since holds nothing. A push that
/// names no row (from a page older than the ids) keeps the rule every push had, F9's fallback:
/// the NEWEST contact with its push key — the store's candidates for its call, then the Rust
/// match ([`LogPlan::newest`]).
///
/// ⚠️ It reads the store: never under the Engine lock.
pub fn push_target(plan: &LogPlan, pushed: &QsoRecord) -> Result<Option<Arc<QsoRecord>>, String> {
    use tempo_core::logbook::same_push_key;
    match pushed.id {
        Some(id) => Ok(plan.row(id)?.filter(|r| same_push_key(r, pushed))),
        None => plan.newest(&pushed.call, PUSH_KEY, |r| same_push_key(r, pushed)),
    }
}

/// `rows`, each with `service`'s leg stamped `status` — a planned change's pairs. `None` when
/// there is nothing to stamp.
pub(crate) fn stamped_rows(
    rows: &[Arc<QsoRecord>],
    service: UploadService,
    status: &tempo_core::logbook::UploadStatus,
) -> Option<Vec<MadeRow>> {
    let pairs: Vec<MadeRow> = rows
        .iter()
        .filter_map(|row| {
            let id = row.id?;
            let op = LogOp::Stamp {
                id,
                service,
                status: status.clone(),
            };
            let (_, after) = ops_on(row, &[op])?;
            Some((Arc::clone(row), after.map(Arc::new)))
        })
        .collect();
    (!pairs.is_empty()).then_some(pairs)
}

/// What a change that answers [`RowRefusal::Busy`] says where a string is the answer.
pub const LOG_BUSY: &str = "The logbook kept changing while this was being saved, so it was not \
                            saved. Try again.";

/// The contacts owed to LoTW ([`owed_to_lotw`]), by id, in log order — the "already uploaded"
/// declaration's pick, read from the store (SPEC-2 v3 C19) with the Engine lock released.
///
/// ⚠️ It reads the store: never under the Engine lock. Take `rows` ([`StationCore::log_rows`])
/// under it.
pub fn lotw_unsent_ids(rows: &crate::logstore::LogRows) -> Result<Vec<RecordId>, String> {
    use std::ops::ControlFlow;
    use tempo_core::logbook::sqlite::{Order, Scope};
    let mut owed = Vec::new();
    rows.each(OWED_TO_LOTW, Scope::All, Order::Log, &mut |r| {
        if let Some(id) = r.id.filter(|_| owed_to_lotw(r)) {
            owed.push(id);
        }
        ControlFlow::Continue(())
    })
    .map_err(|e| e.to_string())?;
    Ok(owed)
}

/// A LoTW batch's stamps, planned (SPEC-2 v3 C19): the contacts of the batch TQSL signed, found
/// BY ID as `plan` reads the log, each stamped only where the row is still the one that was
/// signed — and what that makes of the batch.
///
/// TQSL runs for tens of seconds with the engine lock released, and the log moves under it:
/// the operator deletes or corrects a contact, another window's commit is taken in. A stamp
/// addressed by position lands on whatever row slid into the gap — a contact LoTW never saw,
/// marked sent, and so never uploaded at all. By id, a deleted contact is simply not found. A
/// contact changed since it was signed is not stamped either: LoTW holds the version that was
/// signed, so the row stays unsent and the next batch signs it as it now stands (LoTW dedupes
/// what it already has). What is left unstamped is counted, for the caller to report.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub fn stamp_lotw_batch_plan(
    plan: &LogPlan,
    batch: &[LotwSigned],
    status: &tempo_core::logbook::UploadStatus,
) -> Result<(LotwStamped, Option<Vec<MadeRow>>), String> {
    let ids: Vec<RecordId> = batch.iter().map(|s| s.id).collect();
    let rows = plan.rows(&ids)?;
    let mut signed: HashMap<RecordId, u64> = batch.iter().map(|s| (s.id, s.fingerprint)).collect();
    let mut report = LotwStamped::default();
    let mut hits = Vec::new();
    // In the batch's order, each contact once.
    for s in batch {
        let Some(fingerprint) = signed.remove(&s.id) else {
            continue;
        };
        match rows.get(&s.id) {
            Some(row) if lotw_fingerprint(row) == fingerprint => hits.push(Arc::clone(row)),
            Some(_) => report.changed += 1,
            None => report.gone += 1,
        }
    }
    report.stamped = hits.len();
    Ok((report, stamped_rows(&hits, UploadService::Lotw, status)))
}

/// The fill job's fills (SPEC-2 v3 D2-A) laid over `rows`, the rows as a plan read them: a pair
/// for each row that still lacks a field the job found, with it filled. A fill lands only in a
/// field the row still lacks — one edited, filled or deleted since the job read it keeps what it
/// has now — and never clears one (`None` is "nothing found").
pub(crate) fn fill_pairs(
    rows: &HashMap<RecordId, Arc<QsoRecord>>,
    fills: &[LogFill],
) -> Vec<MadeRow> {
    fills
        .iter()
        .filter_map(|f| {
            let row = rows.get(&f.id)?;
            let country = f.country.clone().filter(|_| row.country.is_none());
            let state = f.state.clone().filter(|_| row.state.is_none());
            if country.is_none() && state.is_none() {
                return None;
            }
            let mut now = QsoRecord::clone(row);
            if country.is_some() {
                now.country = country;
            }
            if state.is_some() {
                now.state = state;
            }
            Some((Arc::clone(row), Some(Arc::new(now))))
        })
        .collect()
}

/// The class of a fill change: an upgrade — content a fold reads (a state, an entity's
/// country), and no row moves — or, for a change that fills nothing and only records that the
/// job has run, a stamp, which no fold is kept against.
pub(crate) fn fill_class(filled: usize) -> OpClass {
    if filled > 0 {
        OpClass::Upgrade
    } else {
        OpClass::Stamp
    }
}

/// Why a change addressed to one contact — by its id and the edit key of the version the caller
/// holds ([`QsoEdit::key`]) — was not made. See [`StationCore::fresh_row`].
#[derive(Debug, Clone, PartialEq)]
pub enum RowRefusal {
    /// No row carries that id: it was deleted, or was never in this log.
    Gone,
    /// The row is there, but changed since the caller read it — here as it now stands, for the
    /// caller to show and retry against.
    Changed(Arc<QsoRecord>),
    /// `LogBusy` (SPEC-1 v2 R5, SPEC-2 v3 §4.6): the row kept changing under the change — made
    /// so here, or in another window — every time it was planned ([`PLANS`] times), so nothing
    /// was changed. New with the write path's plan off the Engine lock, and rare: another writer
    /// has to change the row inside each plan's few milliseconds, four times running.
    Busy,
}

/// The log's rows as they stood just before a change: what the store is told the change
/// against, and what the change's pairs for the hot index are measured from. See
/// [`StationCore::change_base`].
pub(crate) struct ChangeBase {
    rows: Vec<Arc<QsoRecord>>,
}

/// How many times a change is planned before it answers [`RowRefusal::Busy`]: once, and three
/// times again (SPEC-2 v3 §4.6).
pub const PLANS: usize = 4;

/// Past this many calls, a bulk change's candidate rows are found in one pass over the log rather
/// than a look-up per call ([`LogPlan::candidates`]): a whole foreign `log.adi` names most of it.
const CANDIDATE_SEEKS: usize = 2_000;

/// This process's recent changes to the log, newest last, each by the revision it left the log
/// at — what a commit's precondition reads ([`StationCore::unchanged_since`], SPEC-2 v3 §4.6). A
/// change reaches it where every change reaches the hot index ([`StationCore::follow`]).
///
/// Kept as far back as [`RECENT_CHANGES`] changes: a plan taken before the oldest cannot be
/// checked, and plans again — a plan lives for milliseconds, so that is a plan that watched two
/// hundred changes go by.
#[derive(Debug, Default)]
struct Recent {
    /// `(revision, the rows it touched)` — each row as it stood before the change and as the
    /// change left it, by id and by call as the store keys one
    /// ([`tempo_core::logbook::sqlite::call_norm_of`]) — `None` for a change of more rows than
    /// [`RECENT_ROWS`] (a merge, an import, a purge, another window's commits taken in, a write
    /// the station did not make), which a plan reads as touching every row.
    changes: VecDeque<(u64, Option<Vec<Touched>>)>,
    /// The revision of the newest change that has left `changes`: a plan taken before it can no
    /// longer be checked.
    floor: u64,
}

/// One row a change touched, as [`Recent`] keeps it: its id, and its call as the store keys one.
type Touched = (Option<RecordId>, String);

/// How many changes [`Recent`] keeps.
const RECENT_CHANGES: usize = 256;
/// Past this many rows a change is kept as touching every row.
const RECENT_ROWS: usize = 64;

impl Recent {
    /// A change that left the log at `revision`, touching the rows of `pairs`.
    fn note(&mut self, revision: u64, pairs: &[RowPair]) {
        let rows = (pairs.len() <= RECENT_ROWS).then(|| {
            pairs
                .iter()
                .flat_map(|(b, a)| [b, a])
                .flatten()
                .map(|r| (r.id, tempo_core::logbook::sqlite::call_norm_of(&r.call)))
                .collect()
        });
        self.push(revision, rows);
    }

    /// A change that left the log at `revision`, touching rows nobody listed.
    fn note_any(&mut self, revision: u64) {
        self.push(revision, None);
    }

    fn push(&mut self, revision: u64, rows: Option<Vec<Touched>>) {
        self.changes.push_back((revision, rows));
        while self.changes.len() > RECENT_CHANGES {
            if let Some((gone, _)) = self.changes.pop_front() {
                self.floor = self.floor.max(gone);
            }
        }
    }

    /// Whether no change since `revision` touched any of `ids`.
    fn untouched_since(&self, revision: u64, ids: &[RecordId]) -> bool {
        self.untouched_since_by(revision, |(id, _)| id.is_some_and(|id| ids.contains(&id)))
    }

    /// Whether no change since `revision` touched any of `ids`, or a row whose call is one of
    /// `calls` (as the store keys a call: [`tempo_core::logbook::sqlite::call_norm_of`]) — a bulk
    /// change's precondition: a row added or changed under a call it read is a row it did not
    /// plan on.
    fn untouched_since_calls(
        &self,
        revision: u64,
        ids: &HashSet<RecordId>,
        calls: &std::collections::BTreeSet<String>,
    ) -> bool {
        self.untouched_since_by(revision, |(id, call)| {
            id.is_some_and(|id| ids.contains(&id)) || calls.contains(call)
        })
    }

    /// Whether no change since `revision` touched a row `hits` names.
    fn untouched_since_by(&self, revision: u64, hits: impl Fn(&Touched) -> bool) -> bool {
        revision >= self.floor
            && self
                .changes
                .iter()
                .filter(|(at, _)| *at > revision)
                .all(|(_, rows)| rows.as_ref().is_some_and(|rows| !rows.iter().any(&hits)))
    }
}

/// What a change planned off the Engine lock reads — the log's rows, and this process's own
/// changes the store may not hold yet — with where the log stood when it was taken: taken under
/// the lock by [`StationCore::log_plan`], read after it is released, and handed back to the
/// commit, which makes the change only if nothing it read has changed since (SPEC-2 v3 §4.6).
#[derive(Debug, Clone)]
pub struct LogPlan {
    rows: crate::logstore::LogRows,
    pending: crate::logstore::Pending,
    /// The station's revision when the plan was taken.
    rev: u64,
    /// The store's count of another process's commits then — `None` on the 1.13 path.
    foreign: Option<u64>,
}

impl LogPlan {
    /// The rows `ids` name, as this process knows them — the store's (on the 1.13 path, the log
    /// in memory's), with this process's changes the store may not hold yet laid over them
    /// ([`crate::logstore::Pending`]) — by id. An id no row carries is absent.
    ///
    /// It reads the store AS IT STANDS, without waiting for the writer to take this process's
    /// changes (SPEC-2 v3 P4's wait): every one it has not taken is laid over what it holds, so a
    /// plan never waits for a stalled write.
    ///
    /// ⚠️ It reads the store: never under the Engine lock (a debug build panics).
    pub fn rows(&self, ids: &[RecordId]) -> Result<HashMap<RecordId, Arc<QsoRecord>>, String> {
        let found = match &self.rows {
            crate::logstore::LogRows::Store(reads) => reads
                .read(std::time::Duration::ZERO, |db| db.rows_by_ids(ids))
                .map(|(found, _)| found),
            memory => memory.rows_by_ids(ids).map(|(found, _)| found),
        }
        .map_err(|e| e.to_string())?;
        let mut rows: HashMap<RecordId, Arc<QsoRecord>> = found
            .into_iter()
            .filter_map(|r| Some((r.id?, Arc::new(r))))
            .collect();
        for &id in ids {
            match self.pending.row(id) {
                Some(Some(row)) => {
                    rows.insert(id, row);
                }
                Some(None) => {
                    rows.remove(&id);
                }
                None => {}
            }
        }
        Ok(rows)
    }

    /// The row `id` names, as [`Self::rows`] reads it: `None` when no row carries it.
    ///
    /// ⚠️ It reads the store: never under the Engine lock.
    pub fn row(&self, id: RecordId) -> Result<Option<Arc<QsoRecord>>, String> {
        Ok(self.rows(&[id])?.remove(&id))
    }

    /// The NEWEST row, in log order, that `call` might name and `is` accepts — as this process
    /// knows the log, like [`Self::rows`]. The candidates are the store's rows whose `call_norm`
    /// is `call`'s ([`tempo_core::logbook::sqlite::Scope::CallNorm`]), so `is` must accept only
    /// rows whose call is `call` up to ASCII case and surrounding spaces — SQL narrows, Rust
    /// decides (SPEC-2 v3 P2). `fields` are the columns `is` reads.
    ///
    /// ⚠️ It reads the store: never under the Engine lock.
    pub fn newest(
        &self,
        call: &str,
        fields: tempo_core::logbook::sqlite::Narrow,
        is: impl Fn(&QsoRecord) -> bool,
    ) -> Result<Option<Arc<QsoRecord>>, String> {
        use std::ops::ControlFlow;
        use tempo_core::logbook::sqlite::{call_norm_of, Order, Scope};
        let norm = call_norm_of(call);
        // This process's own rows the store may not have yet: appends on their way are the
        // newest rows of all, and a row changed here is read as it now stands.
        if !self.pending.is_empty() {
            let mine = self
                .pending
                .rows_matching(|r| call_norm_of(&r.call) == norm);
            let ids: Vec<RecordId> = mine.iter().filter_map(|r| r.id).collect();
            let (stored, _) = self.rows.rows_by_ids(&ids).map_err(|e| e.to_string())?;
            let stored: HashSet<RecordId> = stored.iter().filter_map(|r| r.id).collect();
            if let Some(newest) = mine
                .iter()
                .rev()
                .find(|r| r.id.is_some_and(|id| !stored.contains(&id)) && is(r))
            {
                return Ok(Some(Arc::clone(newest)));
            }
        }
        let mut hit: Option<RecordId> = None;
        self.rows
            .each(
                fields,
                Scope::CallNorm(&norm),
                Order::NewestFirst,
                &mut |r| {
                    let Some(id) = r.id else {
                        return ControlFlow::Continue(());
                    };
                    let accepted = match self.pending.row(id) {
                        Some(Some(mine)) => is(&mine),
                        Some(None) => false,
                        None => is(r),
                    };
                    if accepted {
                        hit = Some(id);
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                },
            )
            .map_err(|e| e.to_string())?;
        match hit {
            Some(id) => self.row(id),
            None => Ok(None),
        }
    }

    /// Every merge identity the log holds (`APP_NEXUS_QID`, a contest row's `qid`) as this process
    /// knows the log ([`Self::rows`]): what the Field Day merge reads to skip a row already there
    /// — one narrow pass over the store's `contest_qid`, whatever the log's size.
    ///
    /// ⚠️ It reads the store: never under the Engine lock.
    pub(crate) fn merge_identities(&self) -> Result<HashSet<String>, String> {
        use std::ops::ControlFlow;
        use tempo_core::logbook::sqlite::{Narrow, Order, Scope};
        const QIDS: Narrow = Narrow {
            columns: &["contest_qid"],
            uploads: false,
        };
        let qid = |r: &QsoRecord| {
            r.contest
                .as_deref()
                .map(|c| c.qid.clone())
                .filter(|q| !q.is_empty())
        };
        let mut seen = HashSet::new();
        let mut keep = |r: &QsoRecord| {
            // This process's own version of the row, where one is on its way: a row it deleted
            // is not there, and one it changed carries what the change left.
            match r.id.and_then(|id| self.pending.row(id)) {
                Some(Some(mine)) => seen.extend(qid(&mine)),
                Some(None) => {}
                None => seen.extend(qid(r)),
            }
            ControlFlow::Continue(())
        };
        match &self.rows {
            crate::logstore::LogRows::Store(reads) => reads
                .read(std::time::Duration::ZERO, |db| {
                    db.each_narrow(QIDS, Scope::All, Order::Log, &mut keep)
                })
                .map(|_| ()),
            memory => memory
                .each(QIDS, Scope::All, Order::Log, &mut keep)
                .map(|_| ()),
        }
        .map_err(|e| e.to_string())?;
        // Rows on their way the store has not taken at all.
        seen.extend(
            self.pending
                .rows_matching(|r| qid(r).is_some())
                .iter()
                .filter_map(|r| qid(r)),
        );
        Ok(seen)
    }

    /// The rows of the log whose call is one `calls` names — each a call as the store keys it,
    /// trimmed and ASCII-uppercased ([`tempo_core::logbook::sqlite::call_norm_of`]) — as this
    /// process knows them ([`Self::rows`]), in log order: the CANDIDATE SUB-LOG a bulk change is
    /// planned on (SPEC-2 v3 §4.6, C19 Part B), read by the `qso_callhist` index.
    ///
    /// ★ It holds every row a bulk change can pair with a row it brings. Every matcher those
    /// changes use compares calls up to ASCII case — untrimmed (`reconcile`'s keys, the import's
    /// dedup key) or with `eq_ignore_ascii_case` (the POTA stamps) — and two calls equal under
    /// either are equal trimmed and ASCII-uppercased: they share a `call_norm`. A call with
    /// characters outside ASCII is no exception, since ASCII case leaves those alone on both
    /// sides alike.
    ///
    /// ⚠️ It reads the store: never under the Engine lock.
    pub(crate) fn candidates(
        &self,
        calls: &std::collections::BTreeSet<String>,
    ) -> Result<Vec<Arc<QsoRecord>>, String> {
        self.candidates_seeking(calls, CANDIDATE_SEEKS)
    }

    /// [`Self::candidates`], looking each call up in the store only while there are no more than
    /// `seeks` of them — a test's handle on the one-pass read.
    fn candidates_seeking(
        &self,
        calls: &std::collections::BTreeSet<String>,
        seeks: usize,
    ) -> Result<Vec<Arc<QsoRecord>>, String> {
        use std::ops::ControlFlow;
        use tempo_core::logbook::sqlite::{call_norm_of, Narrow, Order, Scope};
        const CALL_ONLY: Narrow = Narrow {
            columns: &["call"],
            uploads: false,
        };
        let mut ids: Vec<RecordId> = Vec::new();
        let mut keep = |r: &QsoRecord| {
            if calls.contains(&call_norm_of(&r.call)) {
                ids.extend(r.id);
            }
            ControlFlow::Continue(())
        };
        match &self.rows {
            crate::logstore::LogRows::Store(reads) => reads
                .read(std::time::Duration::ZERO, |db| {
                    if calls.len() > seeks {
                        // Too many calls to look each up (a whole foreign log.adi): one pass.
                        return db.each_narrow(CALL_ONLY, Scope::All, Order::Log, &mut keep);
                    }
                    for call in calls {
                        db.each_narrow(CALL_ONLY, Scope::CallNorm(call), Order::Log, &mut keep)?;
                    }
                    Ok(())
                })
                .map(|_| ()),
            // The 1.13 path: its log is in memory, and one pass finds the same rows.
            memory => memory
                .each(CALL_ONLY, Scope::All, Order::Log, &mut keep)
                .map(|_| ()),
        }
        .map_err(|e| e.to_string())?;
        // This process's rows on their way that carry one of these calls now: an append the store
        // has not taken yet, or a row an edit gave one of these calls — read by id below, so a row
        // the store holds keeps its place in the log.
        let mine = self
            .pending
            .rows_matching(|r| calls.contains(&call_norm_of(&r.call)));
        ids.extend(mine.iter().filter_map(|r| r.id));
        let stored = match &self.rows {
            crate::logstore::LogRows::Store(reads) => reads
                .read(std::time::Duration::ZERO, |db| db.rows_by_ids(&ids))
                .map(|(found, _)| found),
            memory => memory.rows_by_ids(&ids).map(|(found, _)| found),
        }
        .map_err(|e| e.to_string())?;
        let mut seen: HashSet<RecordId> = HashSet::with_capacity(stored.len());
        let mut rows = Vec::with_capacity(stored.len());
        for r in stored {
            let Some(id) = r.id else { continue };
            seen.insert(id);
            let row = match self.pending.row(id) {
                Some(Some(mine)) => mine,
                Some(None) => continue,
                None => Arc::new(r),
            };
            // A row an edit on its way took off these calls is not one of theirs any more.
            if calls.contains(&call_norm_of(&row.call)) {
                rows.push(row);
            }
        }
        // Appends the store has not taken: the newest rows of all, in the order they were made.
        rows.extend(
            mine.into_iter()
                .filter(|r| r.id.is_some_and(|id| seen.insert(id))),
        );
        Ok(rows)
    }
}

/// Why a planned change was not made: a row it read is not the row it read any more — changed
/// here, or in another window — and the caller plans again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stale;

/// One change to the log as the station makes it — THE shape of every write (SPEC-2 v3 C19):
/// the rows it takes out and puts in, as the hot index and the store are both told them.
pub(crate) struct RowChange {
    /// What kind of change it is: which watermarks it moves ([`OpClass`]).
    pub(crate) class: OpClass,
    /// The rows it takes out and puts in, in log order ([`RowPair`]).
    pub(crate) pairs: Vec<RowPair>,
    /// Every row goes first (a purge): its pairs name every row taken out.
    pub(crate) clear: bool,
    /// Written in the writer's bulk lane.
    pub(crate) bulk: bool,
    /// `log_meta` keys the store sets with its last chunk (the fill job's `fill_ver`).
    pub(crate) meta: Vec<(&'static str, i64)>,
}

/// A change to one row: the row as the plan read it, and as the change leaves it (`None`: the row
/// goes) — planned, or made.
pub type MadeRow = (Arc<QsoRecord>, Option<Arc<QsoRecord>>);

/// What a plan for one row decides, handed the row as it stands: `Ok(Some)` — the change's class
/// and the row as it leaves it (`None`: the row goes); `Ok(None)` — nothing to change;
/// `Err` — refused, changing nothing.
pub type Decided = Result<Option<(OpClass, Option<QsoRecord>)>, RowRefusal>;

/// What `ops`, made in order on `row`, leave: the widest class among them and the row as they
/// leave it (`None`: removed) — or `None` when none of them changes anything (a satellite tag
/// with a blank name). Each op's rule is its one implementation ([`LogOp::apply_to`]).
pub fn ops_on(row: &QsoRecord, ops: &[LogOp]) -> Option<(OpClass, Option<QsoRecord>)> {
    let mut now = Some(row.clone());
    let mut class: Option<OpClass> = None;
    for op in ops {
        let Some(r) = &now else { break };
        match op.apply_to(r) {
            RowAfter::Now(next) => now = Some(*next),
            RowAfter::Gone => now = None,
            RowAfter::Unchanged => continue,
        }
        class = Some(class.map_or(op.class(), |c| wider(c, op.class())));
    }
    class.map(|c| (c, now))
}

/// A bulk change planned on the candidate sub-log ([`plan_on_candidates`]): what it read, and what
/// it makes of it (SPEC-2 v3 §4.6, C19 Part B). Made under the Engine lock by
/// [`StationCore::commit_bulk`], only while nothing it read — and no row of a call it read — has
/// changed since.
#[derive(Debug)]
pub(crate) struct Planned {
    /// The rows it read, each as it read them.
    read: Vec<Arc<QsoRecord>>,
    /// The calls whose rows it read, as the store keys a call: a change since to a row of one of
    /// them — an append included — plans again.
    calls: std::collections::BTreeSet<String>,
    /// The ids rows it appends brought and keep, which the log held nowhere when it looked: one
    /// taken since plans again.
    kept: Vec<RecordId>,
    /// What it changes: each held row it upgrades (as it read it, as it leaves it), in log order,
    /// then each row it appends — whose id is `None` until the commit mints one.
    pairs: Vec<RowPair>,
    /// What a change to a held row is.
    class: OpClass,
}

impl Planned {
    /// Whether it changes nothing: no row upgraded and none appended.
    pub(crate) fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// How many held rows it upgrades.
    pub(crate) fn upgraded(&self) -> usize {
        self.pairs
            .iter()
            .filter(|(before, _)| before.is_some())
            .count()
    }
}

/// ★ Plan a bulk change — an import, a report merge, the POTA stamps — on the candidate sub-log:
/// the rows of the log whose call is one of `calls` (the calls of the rows it brings,
/// [`LogPlan::candidates`]), handed to `op`, THE implementation of the change as it has always run
/// on the whole log (`Logbook::import_adif_with`, `Logbook::merge_report`, …), on a log holding
/// only those rows. Every row the change can pair with is one of them, so it decides exactly
/// what it would decide over the whole log.
///
/// `ids` are the ids the rows it brings carry. A row keeps its own id only where the whole log
/// holds that id nowhere — the rule `op` applies, which the sub-log alone cannot see — and every
/// other row it appends is minted an id when the change is made. `class` is what a change to a
/// held row is. What `op` answered, and the plan.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn plan_on_candidates<R>(
    plan: &LogPlan,
    calls: impl IntoIterator<Item = String>,
    ids: &[RecordId],
    class: OpClass,
    op: impl FnOnce(&mut Logbook) -> R,
) -> Result<(R, Planned), String> {
    let calls: std::collections::BTreeSet<String> = calls
        .into_iter()
        .map(|c| tempo_core::logbook::sqlite::call_norm_of(&c))
        .collect();
    let read = plan.candidates(&calls)?;
    let held = plan.rows(ids)?;
    let free: HashSet<RecordId> = ids
        .iter()
        .filter(|id| !held.contains_key(id))
        .copied()
        .collect();
    let mut sub = Logbook::new();
    sub.replace_rows(read.clone());
    let out = op(&mut sub);
    let after = sub.records();
    let mut pairs: Vec<RowPair> = read
        .iter()
        .zip(after)
        .filter(|(b, a)| !Arc::ptr_eq(b, a) && ***b != ***a)
        .map(|(b, a)| (Some(Arc::clone(b)), Some(Arc::clone(a))))
        .collect();
    let mut kept = Vec::new();
    let mut taken = HashSet::new();
    for a in after.iter().skip(read.len()) {
        let mut row = QsoRecord::clone(a);
        match row.id {
            Some(id) if free.contains(&id) && taken.insert(id) => kept.push(id),
            _ => row.id = None,
        }
        pairs.push((None, Some(Arc::new(row))));
    }
    Ok((
        out,
        Planned {
            read,
            calls,
            kept,
            pairs,
            class,
        },
    ))
}

/// The calls and the ids of the records an ADIF text holds: what a bulk change bringing them is
/// planned by ([`plan_on_candidates`]).
fn calls_and_ids(text: &str) -> (Vec<String>, Vec<RecordId>) {
    let rows = tempo_core::logbook::parse_adif(text);
    let ids = rows.iter().filter_map(|r| r.id).collect();
    (rows.into_iter().map(|r| r.call).collect(), ids)
}

/// An ADIF import planned ([`plan_on_candidates`]): the contacts the log lacks appended, the ones
/// it holds upgraded from the rows restating them — `Logbook::import_adif`, on the rows of the
/// text's calls. `(added, skipped, merged)`, and the plan.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn plan_import(
    plan: &LogPlan,
    text: &str,
) -> Result<((usize, usize, usize), Planned), String> {
    let (calls, ids) = calls_and_ids(text);
    plan_on_candidates(plan, calls, &ids, OpClass::Upgrade, |log| {
        let (added, skipped, merged) = log.import_adif(text);
        (added.len(), skipped, merged)
    })
}

/// A confirmation report planned (LoTW's, eQSL's): the logged contacts it matches upgraded —
/// `Logbook::merge_report`, on the rows of the report's calls. Its summary, and the plan.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn plan_report(
    plan: &LogPlan,
    text: &str,
) -> Result<(tempo_core::reconcile::ReconcileSummary, Planned), String> {
    let (calls, _) = calls_and_ids(text);
    plan_on_candidates(plan, calls, &[], OpClass::Upgrade, |log| {
        log.merge_report(text)
    })
}

/// A downloaded logbook planned (QRZ's fetch): its contacts the log lacks appended, and the ones it
/// holds upgraded — `Logbook::merge_downloaded`, on the rows of the download's calls. How many it
/// added and its summary, and the plan.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn plan_download(
    plan: &LogPlan,
    text: &str,
) -> Result<((usize, tempo_core::reconcile::ReconcileSummary), Planned), String> {
    let (calls, ids) = calls_and_ids(text);
    plan_on_candidates(plan, calls, &ids, OpClass::Upgrade, |log| {
        let (added, summary) = log.merge_downloaded(text);
        (added.len(), summary)
    })
}

/// A pota.app export's park references planned onto the logged contacts they match —
/// `Logbook::stamp_ota_refs`, on the rows of the export's calls. `(stamped, already, unmatched)`,
/// and the plan.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn plan_ota_refs(
    plan: &LogPlan,
    text: &str,
) -> Result<((usize, usize, usize), Planned), String> {
    let (calls, _) = calls_and_ids(text);
    plan_on_candidates(plan, calls, &[], OpClass::Upgrade, |log| {
        log.stamp_ota_refs(text)
    })
}

/// LoTW's own-QSO report planned: the uploads it shows LoTW holds promoted to accepted, and the
/// ones already accepted stamped again with its time — `Logbook::merge_own_echo`, on the rows of
/// the report's calls. How many were newly promoted, and the plan.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn plan_own_echo(
    plan: &LogPlan,
    text: &str,
    when_unix: i64,
) -> Result<(usize, Planned), String> {
    let (calls, _) = calls_and_ids(text);
    plan_on_candidates(plan, calls, &[], OpClass::Stamp, |log| {
        log.merge_own_echo(text, when_unix)
    })
}

/// The wider of two classes of change to held rows — the one whose watermarks cover both. Every
/// class a change to one row can have is one of a chain (`IdOnly`/`Stamp` ⊂ `Upgrade` ⊂ `Key` ⊂
/// `Structural`), so one of the two is always it; an append, which is not on the chain, is taken
/// as the widest.
fn wider(a: OpClass, b: OpClass) -> OpClass {
    fn rank(c: OpClass) -> u8 {
        match c {
            OpClass::IdOnly | OpClass::Stamp => 0,
            OpClass::Upgrade => 1,
            OpClass::Key => 2,
            OpClass::Structural | OpClass::Append => 3,
        }
    }
    match if rank(a) >= rank(b) { a } else { b } {
        OpClass::Append => OpClass::Structural,
        w => w,
    }
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
    /// The log's watermarks: the revision at which each kind of change last happened
    /// ([`Watermarks`]), which every cache over the log is kept against. The station keeps them,
    /// so they outlive the in-memory log (SPEC-2 v3 C19), and they move as each change is
    /// followed ([`Self::follow`]).
    marks: Watermarks,
    /// Mints the ids of the contacts the station logs, under its position id
    /// ([`Self::set_posid`]). Each load's log hands over its own ([`Logbook::hand_over_minter`])
    /// and keeps a copy, under a nonce of its own, for the rows its imports and merges still add
    /// (SPEC-2 v3 C19, Part B).
    minter: Minter,
    /// This process's recent changes, by revision — what a commit's precondition reads.
    recent: Recent,
    /// The store that owns the log — see [`crate::logstore`]. A new station holds an empty one
    /// in memory ([`LogStore::in_memory`]) until the launch gives it the operator's
    /// ([`Self::attach_store`]). `None` is the 1.13 path, where the log IS `log.adi`, appended
    /// to and rewritten whole: what a session falls back to when the store cannot be opened, and
    /// what the tests that pin `log.adi`'s own behaviour still drive through
    /// [`Self::set_log_path`].
    pub(crate) store: Option<LogStore>,
    /// Why the store is not in use this session, when it was asked for and could not be opened.
    pub(crate) store_problem: Option<crate::dto::LogStoreProblem>,
    /// The hot index set ([`tempo_core::logbook::hot`]): the duplicate guard, the B4 sets, the
    /// badges, the partner's logged grid, "last worked", the activation count and the contest
    /// session's sweep, each followed row by row as the log changes instead of swept from it.
    /// A `Mutex` because the snapshot reads it through `&self` and it may catch up there; it is
    /// only ever taken under the engine lock, so it never waits — see [`Self::hot`].
    pub(crate) hot: std::sync::Mutex<HotIndex>,
    /// Memory still holds a row another process deleted from the store: an in-place re-read
    /// (made while a caller held positions into the log) could not remove it. The next
    /// freshness poll does, whether or not anything else has changed.
    pub(crate) store_lingering: bool,
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
    /// (new-DXCC highlighting simply stays off). See [`Self::set_dxcc_resolver`]. Shared, so the
    /// launch can key the hot index it builds off the lock with this very resolver
    /// ([`crate::logstore::HotBuild`]).
    pub(crate) dxcc_resolve: Option<Arc<DxccResolve>>,
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

/// The log a new station holds until the launch gives it the operator's: an empty store in memory
/// (SPEC-2 v3 C19, D1-A), so every station keeps its log in a store and a pass over it has one
/// home to read. It resolves no entity: the operator's store is opened with cty.dat's resolver,
/// and this one is replaced by it. Opening one costs about a millisecond. Should SQLite fail to
/// open even an in-memory database, the station starts store-less, as every station did before,
/// and says so in the diagnostic log.
fn empty_store() -> Option<LogStore> {
    let resolve: crate::logstore::StoreResolve =
        Arc::new(|_: &QsoRecord| tempo_core::logbook::sqlite::Resolved::default());
    match LogStore::in_memory(resolve) {
        Ok(store) => Some(store),
        Err(e) => {
            tempo_core::applog::error(
                "logbook",
                &format!("an empty logbook store could not be opened in memory: {e}"),
            );
            None
        }
    }
}

impl StationCore {
    /// A fresh station: an empty log in an empty store in memory, no paths, no injected
    /// resolvers. The shell wires the real ones in at startup (the operator's store, cty.dat /
    /// rarity / LoTW resolvers, journals).
    #[allow(deprecated)] // SPEC-2 C19: the station is built around the in-memory log
    pub(crate) fn new() -> Self {
        let mut logbook = Logbook::new();
        let minter = logbook.hand_over_minter();
        let marks = logbook.marks();
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
            logbook,
            marks,
            minter,
            recent: Recent::default(),
            store: empty_store(),
            store_problem: None,
            store_lingering: false,
            hot: Default::default(),
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

    /// Point the logbook at an ADIF file and load any existing contacts from it — the 1.13
    /// path, where `log.adi` IS the log's durable home. The shell takes it only when the store
    /// cannot be opened ([`Self::attach_store`] is the ordinary launch). With no entity resolver
    /// for the store: see [`Self::set_log_path_resolved`], which the launch uses.
    pub fn set_log_path(&mut self, path: PathBuf) {
        self.set_log_path_resolved(
            path,
            Arc::new(|_: &QsoRecord| tempo_core::logbook::sqlite::Resolved::default()),
        );
    }

    /// [`Self::set_log_path`], with cty.dat's answer for the columns the store writes beside each
    /// contact (its entity and CQ zone), as the operator's own store is opened with.
    ///
    /// ★ SPEC-2 v3 C19, D1-A — the operator's "Same code, in-memory database". The contacts are
    /// read as 1.13 read them ([`Logbook::load`]: the anchor copy, the sweep of the copies, the
    /// scrub, the ids), then loaded into a store in this process's memory
    /// ([`LogStore::fallback`]), so every reader of the log reads a store on either path. The
    /// store's lane keeps `log.adi` with 1.13's rules: a pure append appended, anything else
    /// rewritten whole, a change saved once it is in the file, and a file another machine
    /// changed taken in before it is replaced ([`Self::take_in_log_file_changes`]).
    ///
    /// Should the store not open even in memory, the session runs on `log.adi` exactly as 1.13
    /// did, with no store at all, and the diagnostic log says so.
    #[allow(deprecated)] // SPEC-2 C19: the log.adi fallback (D1)
    pub fn set_log_path_resolved(&mut self, path: PathBuf, resolve: crate::logstore::StoreResolve) {
        // The file as the load reads it — statted first, so a write that lands during the load
        // leaves it unaccounted, and it is read again before anything replaces it.
        let read = tempo_core::logbook::mirror::file_stamp(&path);
        let log = Logbook::load(&path);
        self.store = match LogStore::fallback(&path, read, log.records(), resolve) {
            Ok(store) => Some(store),
            Err(e) => {
                tempo_core::applog::error(
                    "logbook",
                    &format!(
                        "log.adi could not be loaded into a store in memory ({e}); this session \
                         writes it as 1.13 did"
                    ),
                );
                None
            }
        };
        self.logbook = log;
        self.minter = self.logbook.hand_over_minter();
        self.log_path = Some(path);
        self.last_log_mtime = None;
        self.backfill_country();
        self.backfill_state();
        self.sync_hot();
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
    ///
    /// Hands back the rows a contest session restored at launch is swept from, when the open set
    /// them aside ([`crate::logstore::HotBuild`]): they are this log's rows until it next
    /// changes.
    #[allow(deprecated)] // SPEC-2 C19: the launch attach loads the in-memory log
    pub fn attach_store(&mut self, opened: Opened) -> Option<SessionRows> {
        let Opened {
            store,
            records,
            foreign,
            hot,
            outcome,
        } = opened;
        self.logbook = Logbook::from_store(records);
        self.minter = self.logbook.hand_over_minter();
        // The hot index the open built of these very rows off the lock (SPEC-2 v3 C19), when it
        // was keyed by the resolver this station holds. Otherwise the catch-up below builds one
        // from the log, as it always has.
        let mut session_rows = None;
        if let Some(built) = hot.filter(|h| same_resolver(&h.entity, &self.dxcc_resolve)) {
            self.install_hot(built.index);
            session_rows = Some(SessionRows {
                rows: built.recent,
                bound: built.bound,
                marks: self.marks,
                exact: true,
                ready: true,
            });
        }
        self.log_path = store.log_path().map(Path::to_path_buf);
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
                store.refresh_mirror();
            }
        }
        self.sync_hot();
        session_rows
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
    #[allow(deprecated)] // SPEC-2 C19 (the cut): the take-in runs under the lock (the launch's attach, the freshness poll), so it checks the log in memory; it moves into the open path
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
            store.refresh_mirror();
        }
    }

    /// The rows as they stand, for measuring a change against: the store is told the change
    /// against them, and the hot index follows it by the pairs measured from them (see
    /// [`Self::persist_change`]) — a copy of pointers, never of a record. The index and the
    /// watermarks are brought up to the log first, so these are the rows the index holds and the
    /// pairs start from them.
    #[allow(deprecated)] // SPEC-2 C19 (C, the cut): the 1.13 backfills, the take-in and another window's reload measure their change against the log in memory
    fn change_base(&mut self) -> ChangeBase {
        self.sync_hot();
        ChangeBase {
            rows: self.logbook.records().to_vec(),
        }
    }

    /// Carry a change made in memory to disk. With the store, the rows whose contents differ
    /// from `base` go to the writer thread — a channel send, no I/O — and the mirror lane is
    /// told. On the 1.13 path, the whole of `log.adi` is rewritten, as it always was. Either
    /// way the hot index follows the change, row by row, from `base`.
    #[allow(deprecated)] // SPEC-2 C19 (C, the cut): the 1.13 backfills, the take-in and another window's reload measure their change against the log in memory
    fn persist_change(&mut self, base: ChangeBase, context: &str) {
        match &self.store {
            Some(store) => {
                let change = Change::between(&base.rows, &self.logbook, store.resolved());
                if let Some(store) = self.store.as_mut() {
                    store.submit(change);
                }
            }
            None => self.save_log(context),
        }
        self.catch_up_hot(Some(&base));
    }

    // ─── The write path: plan off the Engine lock, make under it (SPEC-2 v3 §4.6, C19 Part B) ───
    //
    // Every write the station makes is ONE `RowChange` — the rows it takes out and puts in — and
    // reaches the store, the hot index and (until the cut) the log in memory as that one change
    // ([`Self::commit`]). A change to rows the log already holds is PLANNED off the Engine lock,
    // on the rows read from the store ([`Self::log_plan`], [`LogPlan::rows`]), and MADE under it
    // only while those rows are still the rows it read ([`Self::unchanged_since`]); otherwise it
    // is planned again, and after [`PLANS`] plans it answers [`RowRefusal::Busy`]. An append reads
    // nothing: the FT auto-log builds its row, takes the duplicate guard's answer from the hot
    // index, and commits — no SQL, so the radio loop never waits on the disk ([`Self::append`]).
    //
    // Until the cut the log in memory is a FOLLOWER: each change is laid over it
    // ([`Logbook::follow`]), at the station's watermarks, so the tests and the parity oracle still
    // read a whole log. Nothing in the write path plans on it.

    /// What a change planned off the Engine lock reads: the log's rows ([`Self::log_rows`]), this
    /// process's changes the store may not hold yet, and where the log stands. Taken under the
    /// lock — handles and pointers, no I/O — and read after it is released ([`LogPlan::rows`]).
    ///
    /// Another window's commits are taken in first ([`Self::recover_external_appends`]), as
    /// before every change to rows the log holds, so the plan and the hot index start from the
    /// same picture.
    pub fn log_plan(&mut self) -> LogPlan {
        self.recover_external_appends();
        self.sync_hot();
        self.log_view()
    }

    /// [`Self::log_plan`] for a change that holds no position of any row across the re-read of
    /// another window's commits, which may then MOVE rows: a contact another window deleted is
    /// gone from the log in memory before the plan looks, rather than lingering until the next
    /// freshness poll. The LoTW batch's stamps, which have always re-read this way.
    pub fn log_plan_moving(&mut self) -> LogPlan {
        if self.store.is_some() {
            self.refresh_from_store(false);
        } else {
            self.recover_external_appends();
        }
        self.sync_hot();
        self.log_view()
    }

    /// [`Self::log_plan`] with nothing taken in first — what a READ of rows by id takes under the
    /// lock (the Logbook's answer to a change it just made, a test's look at a row), and the
    /// plan's handles. Pointers and no I/O; read after the lock is released.
    pub fn log_view(&self) -> LogPlan {
        LogPlan {
            rows: self.log_rows(),
            pending: self.store.as_ref().map(|s| s.pending()).unwrap_or_default(),
            rev: self.marks.revision,
            foreign: self.store.as_ref().map(|s| s.foreign_commits()),
        }
    }

    /// ★ The commit's precondition (SPEC-2 v3 §4.6): whether the rows a plan read — `rows`, as it
    /// read them — are still those rows. Under the Engine lock, and no I/O in the ordinary case.
    ///
    /// - **This process's changes**: none made since the plan was taken touched one of them
    ///   ([`Recent`]).
    /// - **Another window's**: none committed since — or, when one has, its commits are taken in
    ///   (as before every change) and each row the plan read is the row the log now holds.
    ///   Another window's change to OTHER rows then costs nothing but that take-in.
    ///
    /// ⚠️ The second half compares against the log in memory, which is kept current with the
    /// store's commits until the cut. After the cut this is what must answer it instead: the
    /// store's own word on those rows (A's D4-A, or a precondition the writer checks).
    #[allow(deprecated)] // SPEC-2 C19 (the cut): another window's change to a planned row, read in the log in memory
    pub(crate) fn unchanged_since(&mut self, plan: &LogPlan, rows: &[Arc<QsoRecord>]) -> bool {
        let ids: Vec<RecordId> = rows.iter().filter_map(|r| r.id).collect();
        if !self.recent.untouched_since(plan.rev, &ids) {
            return false;
        }
        let foreign = self.store.as_ref().map(|s| s.foreign_commits());
        if foreign == plan.foreign {
            return true;
        }
        self.recover_external_appends();
        let held: HashMap<RecordId, &Arc<QsoRecord>> = self
            .logbook
            .records()
            .iter()
            .filter_map(|r| r.id.filter(|id| ids.contains(id)).map(|id| (id, r)))
            .collect();
        rows.iter().all(|r| {
            r.id.and_then(|id| held.get(&id))
                .is_some_and(|h| ***h == **r)
        })
    }

    /// Whether the log has not changed at all since `plan` was taken — here or in another window:
    /// the precondition of a change whose plan read something no row by row check can hold (the
    /// Field Day merge's "already there", read off every row's merge identity).
    pub(crate) fn unchanged_at_all_since(&self, plan: &LogPlan) -> bool {
        self.marks.revision == plan.rev
            && self.store.as_ref().map(|s| s.foreign_commits()) == plan.foreign
    }

    /// ★ Make one change — THE way a row of the log changes (SPEC-2 v3 C19). Under the Engine
    /// lock, no I/O: the station's watermarks move by the change's class; the log in memory
    /// follows it (until the cut); the hot index follows its pairs ([`Self::follow`]); and it
    /// goes to the writer as one [`Change`] built from the same pairs — a channel send — or, on
    /// the 1.13 path, `log.adi` is rewritten (`context` names the change if that fails). The
    /// ticket, when the store took it.
    pub(crate) fn commit(
        &mut self,
        change: RowChange,
        context: &str,
    ) -> Option<tempo_core::logbook::writer::Ticket> {
        let marks = self.make(&change);
        let appends = !change.clear && change.pairs.iter().all(|(before, _)| before.is_none());
        match self.store.as_mut() {
            Some(store) => {
                let mut c = Change::of_pairs(marks, change.clear, &change.pairs, store.resolved());
                if change.bulk {
                    c = c.in_bulk();
                }
                c.meta = change.meta;
                // On the 1.13 path the store's lane keeps `log.adi` by the rule below: rows only
                // appended are appended to it (SPEC-2 v3 C19, D1-A). The database takes either
                // as any change.
                if appends {
                    store.submit_appended(c)
                } else {
                    store.submit(c)
                }
            }
            // The 1.13 path, as it always was: rows only appended are appended to `log.adi`;
            // any other change rewrites it whole; a change that touched no row (a purge of an
            // empty log) writes nothing.
            None => {
                if appends {
                    let rows: Vec<QsoRecord> = change
                        .pairs
                        .iter()
                        .filter_map(|(_, after)| after.as_deref().cloned())
                        .collect();
                    if !rows.is_empty() {
                        self.append_to_log_file(&rows, false);
                    }
                } else {
                    self.save_log(context);
                }
                None
            }
        }
    }

    /// A change, made in memory: the watermarks it moves the log to, the log in memory
    /// following it, and the hot index following its pairs — the half of [`Self::commit`] and
    /// [`Self::append`] that does not touch the disk.
    #[allow(deprecated)] // SPEC-2 C19 (the cut): the log in memory follows each change
    fn make(&mut self, change: &RowChange) -> Watermarks {
        // Up to the log as it stands, first: a write that reached it around the station (a
        // test's) is taken in, so the pairs start where the index does.
        self.sync_hot();
        let mut marks = self.marks;
        marks.mark(change.class);
        // A change that also appends rows after the rows it changed (an import's new contacts
        // beside the ones it upgraded) moves an append's watermarks too, after its own.
        if change.class != OpClass::Append
            && change
                .pairs
                .iter()
                .any(|(before, after)| before.is_none() && after.is_some())
        {
            marks.mark(OpClass::Append);
        }
        self.logbook.follow(&change.pairs, change.clear, marks);
        self.follow(&change.pairs, marks);
        marks
    }

    /// ★ Make a change planned off the Engine lock (SPEC-2 v3 §4.6) — under it. `rows` are the
    /// plan's pairs: each row as the plan read it, and as the change leaves it (`None`: it goes).
    /// [`Stale`], changing nothing, when one of the rows it read is no longer the row the plan
    /// read ([`Self::unchanged_since`]): the caller plans again.
    ///
    /// In a debug build the rows the plan read are held to the log in memory as well — the
    /// oracle the store's rows answer to until the cut (SPEC-2 v3 P6).
    pub(crate) fn commit_planned(
        &mut self,
        plan: &LogPlan,
        class: OpClass,
        rows: Vec<MadeRow>,
        bulk: bool,
        meta: Vec<(&'static str, i64)>,
        context: &str,
    ) -> Result<Option<tempo_core::logbook::writer::Ticket>, Stale> {
        let read: Vec<Arc<QsoRecord>> = rows.iter().map(|(b, _)| Arc::clone(b)).collect();
        if !self.unchanged_since(plan, &read) {
            return Err(Stale);
        }
        #[cfg(debug_assertions)]
        self.check_plan_against_memory(&read);
        let pairs = rows.into_iter().map(|(b, a)| (Some(b), a)).collect();
        Ok(self.commit(
            RowChange {
                class,
                pairs,
                clear: false,
                bulk,
                meta,
            },
            context,
        ))
    }

    /// ★ Make a bulk change planned off the Engine lock on the candidate sub-log
    /// ([`plan_on_candidates`]) — under it, in the writer's bulk lane. [`Stale`], changing
    /// nothing, when a row it read has changed since, a row of a call it read was added or
    /// changed, an id it keeps was taken, or another window committed: the caller plans again.
    ///
    /// Each row it appends is filled (its country and US state, SPEC-2 v3 D2-A) and given an id
    /// here, as every insert is — the station mints the ones it did not keep its own of. A change
    /// that only appends is an append; one that upgrades held rows is `planned`'s class, and its
    /// appends move an append's watermarks after it ([`Self::make`]). What it made: the ticket,
    /// and the rows it appended, as appended.
    pub(crate) fn commit_bulk(
        &mut self,
        plan: &LogPlan,
        planned: Planned,
        context: &str,
    ) -> Result<
        (
            Option<tempo_core::logbook::writer::Ticket>,
            Vec<Arc<QsoRecord>>,
        ),
        Stale,
    > {
        let mut ids: HashSet<RecordId> = planned.read.iter().filter_map(|r| r.id).collect();
        ids.extend(&planned.kept);
        if !self
            .recent
            .untouched_since_calls(plan.rev, &ids, &planned.calls)
            || self.store.as_ref().map(|s| s.foreign_commits()) != plan.foreign
        {
            return Err(Stale);
        }
        #[cfg(debug_assertions)]
        self.check_plan_against_memory(&planned.read);
        let class = if planned.upgraded() == 0 {
            OpClass::Append
        } else {
            planned.class
        };
        let (country, state) = (self.dxcc_resolve.as_deref(), self.state_resolve.as_deref());
        let mut appended = Vec::new();
        let mut pairs = Vec::with_capacity(planned.pairs.len());
        for (before, after) in planned.pairs {
            match (before, after) {
                (None, Some(row)) => {
                    let mut row = QsoRecord::clone(&row);
                    fill_with(&mut row, country, state);
                    if row.id.is_none() {
                        row.id = Some(self.minter.mint());
                    }
                    let row = Arc::new(row);
                    appended.push(Arc::clone(&row));
                    pairs.push((None, Some(row)));
                }
                pair => pairs.push(pair),
            }
        }
        let ticket = self.commit(
            RowChange {
                class,
                pairs,
                clear: false,
                bulk: true,
                meta: Vec::new(),
            },
            context,
        );
        Ok((ticket, appended))
    }

    /// The P6 oracle, in a debug build: every row a plan read off the store — this process's own
    /// changes still on their way laid over it — is the row the log in memory holds. Over a log
    /// small enough for a test to feel nothing, and only while no other process has ever
    /// committed to the store: another window's commit reaches the store at once and this
    /// window's memory only when it is taken in, so there the two may differ for a moment by
    /// design (the precondition's second half, [`Self::unchanged_since`]).
    #[cfg(debug_assertions)]
    #[allow(deprecated)] // SPEC-2 C19 (the cut): the debug build's oracle is the log in memory
    fn check_plan_against_memory(&self, read: &[Arc<QsoRecord>]) {
        if self.logbook.len() > DUAL_EXECUTION_ROWS
            || self.store.as_ref().is_some_and(|s| s.foreign_commits() > 0)
        {
            return;
        }
        for r in read {
            let held = self.logbook.records().iter().find(|h| h.id == r.id);
            assert_eq!(
                held.map(|h| h.as_ref()),
                Some(r.as_ref()),
                "the write path planned on a row the log in memory does not hold as it read it"
            );
        }
    }

    /// ★ Add contacts to the log — the append every logged contact makes (the FT auto-log, the
    /// cockpit's Log button, the manual form, Remote) and the contest merge's rows — and hand them
    /// back with their ids: the one each carries, or one the station mints.
    ///
    /// ⛔ **NO SQL, and no pass over the log** (SPEC-2 v3 §4.6; the FT gate): the rows are built
    /// in memory, taken by the hot index, and handed to the writer as they are
    /// ([`Change::appended`]) — a channel send — so the radio loop never waits on the disk. The
    /// duplicate guard was asked of the hot index before this, by the caller.
    ///
    /// With `receipt`, a receipt per change for a caller that proves it on disk after releasing
    /// the lock (Remote); `None` when nothing could be handed on (the 1.13 path's append
    /// failed, or no log is set).
    pub(crate) fn append(
        &mut self,
        mut recs: Vec<QsoRecord>,
        receipt: bool,
    ) -> (
        Vec<QsoRecord>,
        Option<Vec<tempo_core::logbook::LogAppendReceipt>>,
    ) {
        for r in &mut recs {
            if r.id.is_none() {
                r.id = Some(self.minter.mint());
            }
        }
        let rows: Vec<Arc<QsoRecord>> = recs.iter().cloned().map(Arc::new).collect();
        let marks = self.make(&RowChange {
            class: OpClass::Append,
            pairs: rows.iter().map(|r| (None, Some(Arc::clone(r)))).collect(),
            clear: false,
            bulk: false,
            meta: Vec::new(),
        });
        let receipts = match self.store.as_mut() {
            Some(store) => {
                // An append: on the 1.13 path its rows are appended to `log.adi`, as 1.13
                // appended them (SPEC-2 v3 C19, D1-A), where the database takes them as any
                // change.
                let ticket =
                    store.submit_appended(Change::appended(&rows, marks, store.resolved()));
                match (ticket, receipt) {
                    // Redeemed once the change is safe: committed, and on the 1.13 path in
                    // `log.adi` too.
                    (Some(ticket), true) => Some(vec![store.receipt(ticket)]),
                    (Some(_), false) => Some(Vec::new()),
                    (None, _) => None,
                }
            }
            // The 1.13 path: the rows are the log in memory's last (just followed), and are
            // appended to `log.adi` behind them.
            None => self.append_to_log_file(&recs, receipt),
        };
        (recs, receipts)
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
        self.set_dxcc_resolver_shared(Arc::new(resolve));
    }

    /// [`Self::set_dxcc_resolver`], with a resolver the caller shares — the launch, which keys
    /// the hot index it has the store build with the same one ([`crate::logstore::HotBuild`]).
    pub fn set_dxcc_resolver_shared(&mut self, resolve: Arc<DxccResolve>) {
        self.dxcc_resolve = Some(resolve);
        // Every row's entity comes from the resolver, so a new one re-keys the hot index: it is
        // rebuilt at its next catch-up — the backfill's, below.
        match self.hot.get_mut() {
            Ok(hot) => hot.invalidate(),
            Err(poisoned) => poisoned.into_inner().invalidate(),
        }
        self.backfill_country();
        self.sync_hot();
    }

    /// Inject the (callsign, heard grid) → US state resolver (the command layer passes a
    /// closure over the FCC callsign index — the SAME `us_state_hint` the heard side uses).
    /// Backfills every record that lacks a STATE so the needed board, WAS and the awards
    /// matrix all see the states already worked.
    ///
    /// No rebuild of the hot index here, unlike [`Self::set_dxcc_resolver`]: worked STATES are
    /// not part of it (that holds entities/grids/parks). They are folded into
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
        } else {
            // Nothing changed — though the write may still have moved the log's revision.
            self.catch_up_hot(Some(&base));
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
        } else {
            // Nothing changed — though the write may still have moved the log's revision.
            self.catch_up_hot(Some(&base));
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
    /// log, in one breath, as ONE change in the writer's bulk lane — with `fill_ver` on that
    /// change's last chunk, so the store names the resolver data its fills come from only once
    /// every one of them is on disk. Returns how many contacts gained a field. The job itself
    /// plans its fills with the Engine lock released ([`crate::logwrite::fill`]); this is the
    /// same plan and commit for an owner with the log in hand.
    ///
    /// Each fill lands only in a field the contact still lacks, as the plan reads it: one edited,
    /// filled or deleted since the job read it keeps what it has now ([`fill_pairs`]). Nothing on
    /// the 1.13 path, where the job never runs.
    pub fn apply_log_fills(&mut self, fills: &[LogFill], fill_ver: i64) -> usize {
        if self.store.is_none() {
            return 0;
        }
        let ids: Vec<RecordId> = fills.iter().map(|f| f.id).collect();
        for _ in 0..PLANS {
            let plan = self.log_plan();
            let rows = match plan.rows(&ids) {
                Ok(rows) => rows,
                Err(e) => {
                    tempo_core::applog::error(
                        "logbook",
                        &format!("the fill job could not read the logbook: {e}"),
                    );
                    return 0;
                }
            };
            let pairs = fill_pairs(&rows, fills);
            let n = pairs.len();
            let meta = vec![(crate::logfill::FILL_VER, fill_ver)];
            if self
                .commit_planned(&plan, fill_class(n), pairs, true, meta, "fill")
                .is_ok()
            {
                return n;
            }
        }
        tempo_core::applog::warn(
            "logbook",
            "the fill job's contacts kept changing under it; it runs again at the next launch",
        );
        0
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
        self.hot().park_in_log(&key) || self.hunted_parks_import.contains(&key)
    }

    /// Unix seconds of the most recent QSO with this station, on any band or mode —
    /// matched on the base call, so `W6A/P` and `W6A` are one station. `None` when the log
    /// holds no contact with it.
    pub fn last_worked_unix(&self, call: &str) -> Option<u64> {
        self.hot().last_worked(call)
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
    pub(crate) fn log_tick(&self) -> u32 {
        self.marks.revision as u32
    }

    /// The log's watermarks, as the station keeps them — see [`Watermarks`] and [`Self::follow`].
    pub(crate) fn marks(&self) -> Watermarks {
        self.marks
    }

    /// The station's position id, which the ids of the contacts it logs from now on carry (see
    /// [`RecordId`]); 0 until the profile has one.
    #[allow(deprecated)] // SPEC-2 C19 (the cut): the log in memory still mints the rows a take-in of log.adi and the 1.13 recovery add
    pub(crate) fn set_posid(&mut self, posid: u32) {
        self.minter.set_posid(posid);
        self.logbook.set_posid(posid);
    }

    /// The hot index set ([`tempo_core::logbook::hot`]), caught up with the log as it stands and
    /// held until the returned guard drops.
    ///
    /// ⚠️ **Take it once and pass it down.** The lock is only ever taken under the engine lock,
    /// by one thread, so it never waits — which means a second take on the same thread while
    /// the first guard lives can only be a re-entry, and waiting for it would hang the radio
    /// loop forever. It panics instead, naming the mistake.
    ///
    /// Every change the station makes reaches the index as it is made ([`Self::follow`]), so the
    /// catch-up here finds nothing to do. It is there for a write that reached the log some other
    /// way (a test's): with no pairs to follow, it takes in a stamp or an append and rebuilds
    /// after anything else ([`HotIndex::catch_up`]).
    #[allow(deprecated)] // SPEC-2 C19: the index catches up from the in-memory log
    pub(crate) fn hot(&self) -> Hot<'_> {
        let mut hot = match self.hot.try_lock() {
            Ok(hot) => hot,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                // A panic while it was held may have left it half-changed: start again.
                let mut hot = poisoned.into_inner();
                hot.invalidate();
                hot
            }
            Err(std::sync::TryLockError::WouldBlock) => panic!(
                "the hot index was asked for while this thread already holds it: take it once \
                 and pass it down (see StationCore::hot)"
            ),
        };
        hot.catch_up(&self.logbook, &StationKeys(self.dxcc_resolve.as_deref()));
        Hot(hot)
    }

    /// ★ THE way a change to the log reaches the hot index (SPEC-2 v3 C19): `pairs`, the rows it
    /// took out and put in ([`RowPair`]), and `now`, the watermarks it left the log at, which the
    /// station keeps from here. Every change the station makes ends here — the append
    /// ([`Self::append`]), every change it commits ([`Self::commit`]), and each change still made
    /// on the log in memory and measured against a [`ChangeBase`] (a take-in of `log.adi`, the
    /// 1.13 backfills, another window's reload: [`Self::persist_change`]) — so the index hears of
    /// each as it is made, row by row. For those last, `now` is the log's watermarks after it.
    #[allow(deprecated)] // SPEC-2 C19: a debug build checks the index against the in-memory log
    pub(crate) fn follow(&mut self, pairs: &[RowPair], now: Watermarks) {
        self.recent.note(now.revision, pairs);
        let keys = StationKeys(self.dxcc_resolve.as_deref());
        let hot = hot_mut(&mut self.hot);
        hot.follow(pairs, self.marks.revision, now.revision, &keys);
        self.marks = now;
        // The station brings the index up to the log before every change it makes, so the pairs
        // always describe the log the index holds: one it had to let go of is a bug here.
        debug_assert_eq!(
            hot.at(),
            Some(now.revision),
            "hot index: a change the station made could not be followed"
        );
        #[cfg(debug_assertions)]
        if self.logbook.len() <= tempo_core::logbook::hot::VERIFY_ROWS {
            hot.verify(&self.logbook, &keys);
        }
    }

    /// Make `index` — the index of the log as it stands, built somewhere else (the store, off
    /// the lock) — the one the station follows the log with from here.
    #[allow(deprecated)] // SPEC-2 C19: the index is installed as the in-memory log's
    fn install_hot(&mut self, index: HotIndex) {
        let rev = self.logbook.revision();
        let hot = hot_mut(&mut self.hot);
        *hot = index.holding(rev);
        self.marks = self.logbook.marks();
        // ⛔ The debug build's oracle: on a log small enough to afford it, the index built
        // elsewhere must answer as the one this station would have built from its own rows. The
        // oracle's pass is not the station's work, so the counters tests read are left as they
        // were.
        #[cfg(debug_assertions)]
        if self.logbook.len() <= tempo_core::logbook::hot::VERIFY_ROWS {
            use tempo_core::logbook::{hot::HOT_REBUILDS, LOG_SWEEPS};
            let keys = StationKeys(self.dxcc_resolve.as_deref());
            hot.verify(&self.logbook, &keys);
            let counts = (HOT_REBUILDS.with(|c| c.get()), LOG_SWEEPS.with(|c| c.get()));
            let own = HotIndex::build(&self.logbook, &keys);
            HOT_REBUILDS.with(|c| c.set(counts.0));
            LOG_SWEEPS.with(|c| c.set(counts.1));
            assert!(
                hot.answers_as(&own),
                "hot index: the index built elsewhere is not the log's"
            );
        }
    }

    /// Open the contest session's sweep — from `cutoff`, keyed by `rule` — from `rows`, the log's
    /// rows from a bound at or before `cutoff` read off the lock (SPEC-2 v3 C19). The caller has
    /// checked they are still this log's rows ([`crate::engine::Engine::open_session_from`]).
    #[allow(deprecated)] // SPEC-2 C19: a debug build holds the session to the in-memory log's sweep
    pub(crate) fn install_session(
        &mut self,
        cutoff: u64,
        rule: tempo_core::contest::DupeRule,
        rows: &[QsoRecord],
    ) {
        let hot = hot_mut(&mut self.hot);
        hot.install_session(cutoff, rule, rows);
        // ⛔ The debug build's oracle, on a log small enough to afford it: the session opened
        // from rows read elsewhere is the sweep of the log this station holds. The oracle's
        // sweep is not the station's work, so the counters tests read are left as they were.
        #[cfg(debug_assertions)]
        if self.logbook.len() <= tempo_core::logbook::hot::VERIFY_ROWS {
            use tempo_core::logbook::LOG_SWEEPS;
            let opened = hot.worked_since(&self.logbook, cutoff, &rule);
            let sweeps = LOG_SWEEPS.with(|c| c.get());
            let swept = self.logbook.worked_keys_since(cutoff, &rule);
            LOG_SWEEPS.with(|c| c.set(sweeps));
            assert_eq!(
                opened, swept,
                "hot index: the session opened from rows read elsewhere is not the log's sweep"
            );
        }
    }

    /// Bring the hot index, and the watermarks the station keeps, up to the log as it stands —
    /// after a load, or a write that reached the log around the station (a test's). After a
    /// change the station made, which it follows as it makes it ([`Self::follow`]), there is
    /// nothing to do.
    pub(crate) fn sync_hot(&mut self) {
        self.catch_up_hot(None);
    }

    /// Bring the hot index up to the log: by the pairs of the change made since `before` — the
    /// log as it stood when the change began ([`Self::follow`]) — or, with no `before`, by
    /// finding out from the log itself ([`HotIndex::catch_up`]).
    #[allow(deprecated)] // SPEC-2 C19: the index follows the in-memory log's changes
    fn catch_up_hot(&mut self, before: Option<&ChangeBase>) {
        match before {
            Some(base) => {
                let pairs = pairs_between(&base.rows, self.logbook.records());
                self.follow(&pairs, self.logbook.marks());
            }
            None => {
                let keys = StationKeys(self.dxcc_resolve.as_deref());
                hot_mut(&mut self.hot).catch_up(&self.logbook, &keys);
                let now = self.logbook.marks();
                // A write the station did not make, with no pairs to say which rows it touched:
                // a plan spanning it plans again.
                if now.revision != self.marks.revision {
                    self.recent.note_any(now.revision);
                }
                self.marks = now;
            }
        }
    }

    /// `log_qso`'s duplicate guard: whether `rec` is a contact the log already holds, from the
    /// hot index's station lists (see [`tempo_core::logbook::dedup`]).
    ///
    /// ⛔ The FT gate's guard, so a debug build asks the old scan too — over a log small enough
    /// that doing so costs nothing a test would feel — and stops on any difference.
    #[allow(deprecated)] // SPEC-2 C19: a debug build holds the answer to the in-memory log's scan
    pub(crate) fn is_duplicate(&self, rec: &QsoRecord) -> bool {
        let duplicate = self.hot().is_duplicate(rec);
        #[cfg(debug_assertions)]
        if self.logbook.len() <= DUAL_EXECUTION_ROWS {
            assert_eq!(
                duplicate,
                tempo_core::logbook::dedup::scan_for_duplicate(&self.logbook, rec),
                "hot index: the duplicate guard disagrees with the scan it replaced, for {:?}",
                rec.call
            );
        }
        duplicate
    }

    /// The grid logged for `call`'s station: its NEWEST row's, in log order, if that row has
    /// one — from the hot index's station lists. A debug build asks the old scan too, over a
    /// small log, and stops on any difference: this answer is written into logged contacts.
    #[allow(deprecated)] // SPEC-2 C19: a debug build holds the answer to the in-memory log's scan
    pub(crate) fn newest_logged_grid(&self, call: &str) -> Option<String> {
        let grid = self.hot().newest_grid(call);
        #[cfg(debug_assertions)]
        if self.logbook.len() <= DUAL_EXECUTION_ROWS {
            let scanned = self
                .logbook
                .records()
                .iter()
                .rev()
                .find(|r| tempo_core::message::same_call(&r.call, call))
                .and_then(|r| r.grid.clone())
                .filter(|g| !g.trim().is_empty());
            assert_eq!(
                grid, scanned,
                "hot index: the logged grid disagrees with the scan it replaced, for {call:?}"
            );
        }
        grid
    }

    /// The contest session's sweep of the general log ([`Logbook::worked_keys_since`]'s answer),
    /// from the hot index: swept once when a session opens or its start or rule changes, and
    /// followed row by row after that.
    #[allow(deprecated)] // SPEC-2 C19: a session opened is swept from the in-memory log once
    pub(crate) fn worked_since(
        &self,
        cutoff: u64,
        rule: &tempo_core::contest::DupeRule,
    ) -> WorkedSince {
        let mut hot = self.hot();
        hot.0.worked_since(&self.logbook, cutoff, rule)
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
    pub fn activation_qso_count(&self) -> usize {
        match &self.activation {
            Some((_, reference)) => self.hot().activation_count(reference),
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
    /// uploaded (`lotw_unsent_ids` counts them separately, so LoTW is offered two
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
    pub(crate) fn recover_external_appends(&mut self) -> bool {
        if self.on_log_file() {
            return self.take_in_log_file_changes();
        }
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
            let base = self.change_base();
            self.logbook.reconcile_disk(&disk);
            self.catch_up_hot(Some(&base));
        }
        self.last_log_mtime = stamp;
        true
    }

    /// Whether this session is on the 1.13 path with its log in a store in memory — `log.adi`
    /// kept by the store's lane ([`LogStore::fallback`]).
    pub(crate) fn on_log_file(&self) -> bool {
        self.store.as_ref().is_some_and(|s| s.lane().is_some())
    }

    /// [`Self::take_in_log_file_changes`] on the 1.13 path, and nothing on the store path —
    /// what a quit and the exit run before they wait for `log.adi`.
    pub(crate) fn take_in_log_file_if_changed(&mut self) -> bool {
        if !self.on_log_file() {
            return false;
        }
        let took = self.take_in_log_file_changes();
        if took {
            self.sync_hot();
        }
        took
    }

    /// The 1.13 path's recovery, on the store in memory that holds its log (SPEC-2 v3 C19,
    /// D1-A): when `log.adi` is not the file the lane last wrote or this station last took in —
    /// another Nexus sharing the data folder appended to it, or rewrote it — merge it in as 1.13
    /// did ([`Logbook::reconcile_disk`]: the other machine's contacts added, shared ones brought
    /// up to date, nothing removed), hand the merge to the store, and tell the lane the file is
    /// accounted for. Whether it read the file.
    ///
    /// Under the engine lock, as 1.13's was, and at the points 1.13 recovered: before a change
    /// to rows the log holds, and on the freshness poll ([`Self::sync_shared_log_if_changed`]).
    /// The merge's rows came from the file, so the lane writes nothing for them.
    ///
    /// ⚠️ One thing 1.13 never had to handle: the lane writes AFTER a change, so the file can
    /// lag the log. While the lane is writing this log's own changes the file is not looked at.
    /// Once the lane holds a rewrite because the file changed under it, the file is taken in
    /// without the rows it held when this log last accounted for it
    /// ([`LogFileWriter::holds`](tempo_core::logbook::logfile::LogFileWriter::holds)): such a row
    /// is this log's own as it stood before the changes still owed — a contact deleted or edited
    /// here since, which must not come back — and what another machine added is taken in. The
    /// trade: an upgrade the other machine made to one of those rows in that moment (a
    /// confirmation, a stamp) is not taken in; it comes back when that machine next rewrites the
    /// file. In the same race 1.13 lost the other machine's whole write, its new contacts too.
    /// (A contact the OTHER machine deleted still comes back with this machine's next rewrite,
    /// as in 1.13: that one this log still holds.)
    #[allow(deprecated)] // SPEC-2 C19: the log.adi fallback (D1)
    fn take_in_log_file_changes(&mut self) -> bool {
        let Some(path) = self.log_path.clone() else {
            return false;
        };
        let Some(lane) = self.store.as_ref().and_then(|s| s.lane()).cloned() else {
            return false;
        };
        let lane_now = lane.status();
        // The lane is writing this log's own changes: the file is in motion, and it is ours. It
        // is looked at once the lane is done — or once the lane finds it changed by something
        // else, and holds its rewrite for exactly this.
        if lane_now.pending() && !lane_now.foreign_write {
            return false;
        }
        // The fingerprint gate, as 1.13's, against the file the lane last wrote or accepted.
        let stamp = tempo_core::logbook::mirror::file_stamp(&path);
        if stamp.is_some() && stamp == lane_now.known {
            return false;
        }
        // Bytes, then a lossy decode, for 1.13's reason: a CP1252 `log.adi` is not UTF-8.
        let Ok(bytes) = std::fs::read(&path) else {
            return false;
        };
        let disk = String::from_utf8_lossy(&bytes);
        let mut ids = HashSet::new();
        if !disk.is_empty() {
            let base = self.change_base();
            // With the lane idle the file holds every change this log made, and the merge is
            // 1.13's. With the lane still owing it changes, the file lags this log: a row it
            // held when this log last accounted for it is this log's own, as it stood before
            // those changes — a contact since edited or deleted here — and not another
            // machine's news, so it is not merged back. What another machine added is.
            let lagging = lane_now.pending();
            ids = self
                .logbook
                .reconcile_disk_except(&disk, &|id| lagging && lane.holds(id));
            if let Some(store) = self.store.as_mut() {
                let change = Change::between(&base.rows, &self.logbook, store.resolved());
                store.submit_quietly(change);
            }
            self.catch_up_hot(Some(&base));
        }
        if let Some(stamp) = stamp {
            lane.accept(stamp, ids);
        }
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
        let Some(store) = self.store.as_ref() else {
            return false;
        };
        if !store.foreign_changed() && (in_place || !self.store_lingering) {
            return false;
        }
        let base = self.change_base();
        let Some(store) = self.store.as_mut() else {
            return false;
        };
        match store.reload(self.logbook.records(), in_place) {
            Ok((rows, lingering)) => {
                self.store_lingering = lingering;
                // The store's rows carry their fills (SPEC-2 v3 D2-A: every insert fills before
                // it writes, and the fill job writes what an older build left), so the rows
                // re-read are the rows every screen shows, and nothing is filled here.
                self.logbook.replace_rows(rows);
                self.catch_up_hot(Some(&base));
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

    /// THE way to APPEND to the logbook file on the 1.13 path (no store): write the records on
    /// the end, then carry the freshness fingerprint forward so the recovery gate above does not
    /// re-parse a multi-MB log we extended ourselves. With `receipt`, a receipt per record for a
    /// caller that syncs it after releasing the lock (Remote); `None` when an append failed or
    /// no log is set. No rewrite, rollback, retry or second upload path.
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
    #[allow(deprecated)] // SPEC-2 C19 (C, D1): the log.adi fallback's append checks the log in memory
    fn append_to_log_file(
        &mut self,
        recs: &[QsoRecord],
        receipt: bool,
    ) -> Option<Vec<tempo_core::logbook::LogAppendReceipt>> {
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
            "append_to_log_file: the records must already be in memory (see the contract above)"
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
        if self.store.is_some() && !self.on_log_file() {
            // Another window's commits, and — if the mirror has stopped because `log.adi` holds
            // something it cannot account for — that file, taken in so the mirror can resume.
            let reloaded = self.refresh_from_store(false);
            let took_in = self.take_in_refused_log_file();
            if reloaded || took_in {
                self.sync_hot();
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
        self.sync_hot();
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
        let Some(status) = store.mirror_status() else {
            return false;
        };
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

    /// ★ Plan and make a change to the contact `id` in one breath — for an owner holding the
    /// station with no Engine guard (a test's own station or engine): the plan's read of the
    /// store is made here, then the change, and the change is planned again while the row keeps
    /// changing under it, [`PLANS`] times in all. A command holding the Engine lock makes the same
    /// plan with the lock released and the same commit under it ([`crate::logwrite`]).
    ///
    /// `decide` is handed the row as it stands and answers what to make of it ([`Decided`]).
    /// `Err` when the store could not be read; `Ok(Err)` when no row carries `id`, `decide`
    /// refused, or the row kept changing ([`RowRefusal`]); `Ok(Ok(None))` when `decide` found
    /// nothing to change; otherwise the row as the plan read it and as the change left it.
    pub(crate) fn change_by_id(
        &mut self,
        id: RecordId,
        context: &str,
        mut decide: impl FnMut(&Self, &Arc<QsoRecord>) -> Decided,
    ) -> Result<Result<Option<MadeRow>, RowRefusal>, String> {
        for _ in 0..PLANS {
            let plan = self.log_plan();
            let Some(before) = plan.row(id)? else {
                return Ok(Err(RowRefusal::Gone));
            };
            let (class, after) = match decide(self, &before) {
                Ok(Some(made)) => made,
                Ok(None) => return Ok(Ok(None)),
                Err(refusal) => return Ok(Err(refusal)),
            };
            let after = after.map(Arc::new);
            let rows = vec![(Arc::clone(&before), after.clone())];
            if self
                .commit_planned(&plan, class, rows, false, Vec::new(), context)
                .is_ok()
            {
                return Ok(Ok(Some((before, after))));
            }
        }
        Ok(Err(RowRefusal::Busy))
    }

    /// Make `ops` on the contact `id`, in one breath ([`Self::change_by_id`]): whether a row
    /// carries `id` and took them. A row that kept changing under them is not changed, and says
    /// so in the diagnostic log.
    fn change_row(&mut self, id: RecordId, ops: &[LogOp], context: &str) -> bool {
        match self.change_by_id(id, context, |_, row| Ok(ops_on(row, ops))) {
            Ok(Ok(made)) => made.is_some(),
            Ok(Err(RowRefusal::Busy)) => {
                tempo_core::applog::warn(
                    "logbook",
                    &format!("{context}: the contact kept changing under the change; not made"),
                );
                false
            }
            Ok(Err(_)) => false,
            Err(e) => {
                tempo_core::applog::error(
                    "logbook",
                    &format!("{context}: the logbook could not be read: {e}"),
                );
                false
            }
        }
    }

    /// The contact `id` names, if it is still the version whose edit key is `edit_key` — the
    /// check every change addressed by a `RowRef` makes first, on the row as it stands.
    ///
    /// A key rather than the whole row, so the writes that happen on their own — an upload
    /// stamp, a confirmation from LoTW — never refuse an operator's edit ([`QsoEdit::key`]).
    pub fn fresh_row(row: &Arc<QsoRecord>, edit_key: &str) -> Result<(), RowRefusal> {
        if QsoEdit::project(row).key() == edit_key {
            Ok(())
        } else {
            Err(RowRefusal::Changed(Arc::clone(row)))
        }
    }

    /// Edit the contact `id` (a correction — busted call, wrong band, etc). Sync-derived
    /// state is preserved by the edit ([`tempo_core::logbook::LogOp::Edit`]). Returns false if
    /// no row carries `id`.
    pub fn update_qso(&mut self, id: RecordId, rec: QsoRecord) -> bool {
        self.update_row(id, rec).is_some()
    }

    /// [`Self::update_qso`], handing back the contact as the edit left it.
    pub(crate) fn update_row(&mut self, id: RecordId, rec: QsoRecord) -> Option<Arc<QsoRecord>> {
        let made = self.change_by_id(id, "update_qso", |sc, before| {
            Ok(ops_on(before, &[sc.edit_op(id, rec.clone())]))
        });
        match made {
            Ok(Ok(Some((before, Some(after))))) => {
                self.requeue_if_corrected(&before, &after);
                Some(after)
            }
            Ok(Ok(Some((_, None)))) | Ok(Ok(None)) | Ok(Err(_)) => None,
            Err(e) => {
                tempo_core::applog::error(
                    "logbook",
                    &format!("update_qso: the logbook could not be read: {e}"),
                );
                None
            }
        }
    }

    /// The edit `rec` of the contact `id` as an op — with its country filled from the
    /// station's resolver when the edit carries none (the edit form never does), which the edit
    /// then keeps over the stored one.
    pub(crate) fn edit_op(&self, id: RecordId, mut rec: QsoRecord) -> LogOp {
        if rec.country.is_none() {
            if let Some(resolve) = &self.dxcc_resolve {
                rec.country = resolve(&rec.call);
            }
        }
        LogOp::Edit {
            id,
            rec: Box::new(rec),
        }
    }

    /// A contact whose CALLSIGN a change just corrected — `before` as the row stood, `after` as
    /// the edit left it — queued to every connector again, as the edit left it.
    ///
    /// A CALLSIGN correction is the one edit the services have to hear about again. The edit
    /// clears the upload stamps for exactly that reason — "the services hold the OLD call, so
    /// clearing the upload stamps re-queues the corrected QSO to every one of them" — but
    /// clearing a stamp queues nothing, and nothing else ever re-scans for an unstamped record.
    /// So a busted call fixed in the log stayed busted at QRZ/ClubLog/eQSL/… for good, with the
    /// stamps that were the only evidence anything was owed now erased. Read under the edit's
    /// own trimmed, case-insensitive rule, so the two can never disagree about what counts as a
    /// correction. The STORED record, not the incoming payload: the edit merges the fields the
    /// form does not carry (park refs, TIME_OFF, the split leg), and the connectors must send
    /// the whole contact, not the form's half of it.
    pub(crate) fn requeue_if_corrected(&mut self, before: &QsoRecord, after: &QsoRecord) {
        if !after.call.trim().eq_ignore_ascii_case(before.call.trim()) {
            // Every leg: every stamp was just cleared, so every connector is owed. The disabled
            // ones are dropped by the worker's own toggle check, the same way a freshly logged
            // contact's are.
            self.requeue_upload(after.clone(), crate::engine::upload_legs::ALL, 0);
        }
    }

    /// The Logbook form's edit of the contact `id`, as ops on the row as it stands: the field
    /// edit, and the QSL-sent and paper-card marks where the edit changes them — what the form
    /// used to send as three commands, each its own write (`QsoEdit`'s header). Refused when the
    /// row is no longer the version whose key is `edit_key`; `Err` for an edit the caller must not
    /// send again as it is (its QSL-sent code is not one the form offers).
    pub(crate) fn edit_ops(
        &self,
        id: RecordId,
        edit_key: &str,
        edit: &QsoEdit,
        stored: &Arc<QsoRecord>,
    ) -> Result<Result<Vec<LogOp>, RowRefusal>, String> {
        if let Err(refusal) = Self::fresh_row(stored, edit_key) {
            return Ok(Err(refusal));
        }
        let sent = edit.qsl_sent_change(stored)?;
        let card = edit.qsl_card_change(stored);
        let mut rec = edit.record(stored);
        // Keep country populated on edits, as `update_qso` does: the form does not carry it.
        if let Some(resolve) = &self.dxcc_resolve {
            rec.country = resolve(&rec.call);
        }
        let mut ops = vec![LogOp::Edit {
            id,
            rec: Box::new(rec),
        }];
        if let Some(via) = sent {
            ops.push(LogOp::MarkQslSent {
                id,
                via,
                date_unix: now_unix_secs(),
            });
        }
        if let Some(received) = card {
            ops.push(LogOp::MarkQslCard { id, received });
        }
        Ok(Ok(ops))
    }

    /// The Logbook form's edit of the contact `id`, as ONE change ([`Self::edit_ops`]), in one
    /// breath ([`Self::change_by_id`]). Refused — changing nothing — when no row carries `id`,
    /// when the row is no longer the version whose key is `edit_key`, or when it kept changing.
    /// `Err` for an edit the caller must not send again as it is, or a store that could not be
    /// read. What it made: the row as it was, and as the edit left it.
    pub fn edit_qso(
        &mut self,
        id: RecordId,
        edit_key: &str,
        edit: &QsoEdit,
    ) -> Result<Result<MadeRow, RowRefusal>, String> {
        let mut bad = None;
        let mut edited = None;
        let made = self.change_by_id(id, "edit_qso", |sc, stored| {
            match sc.edit_ops(id, edit_key, edit, stored) {
                Ok(Ok(ops)) => {
                    // The row as the field edit alone leaves it — what a corrected call goes back
                    // out to the connectors as, below.
                    edited = ops.first().and_then(|op| match op.apply_to(stored) {
                        RowAfter::Now(row) => Some(*row),
                        RowAfter::Gone | RowAfter::Unchanged => None,
                    });
                    Ok(ops_on(stored, &ops))
                }
                Ok(Err(refusal)) => Err(refusal),
                Err(e) => {
                    bad = Some(e);
                    Ok(None)
                }
            }
        })?;
        if let Some(e) = bad {
            return Err(e);
        }
        match made {
            Ok(Some((before, after))) => {
                // A corrected call goes back out to the connectors as the EDIT left the contact
                // — as it did when the two marks were commands of their own that came after it.
                if let Some(edited) = edited {
                    self.requeue_if_corrected(&before, &edited);
                }
                Ok(Ok((before, after)))
            }
            Ok(None) => Ok(Err(RowRefusal::Gone)),
            Err(refusal) => Ok(Err(refusal)),
        }
    }

    /// Record — or WITHDRAW — the operator's QSL-sent declaration for the contact `id`.
    /// `Some(via)` marks it sent (bureau/direct/electronic, dated now); `None` clears the mark,
    /// which until now had no path at all while the received side has taken a bool since #152.
    /// Never touches confirmation state in either direction. A clear MUST be written, since it
    /// carries the operator decision that keeps a later import from restoring the mark. Returns
    /// false if no row carries `id`.
    pub fn mark_qsl_sent(
        &mut self,
        id: RecordId,
        via: Option<tempo_core::logbook::QslVia>,
    ) -> bool {
        self.change_row(
            id,
            &[LogOp::MarkQslSent {
                id,
                via,
                date_unix: now_unix_secs(),
            }],
            "mark_qsl_sent",
        )
    }

    /// Record whether a PAPER QSL card arrived for the contact `id` (#152) — an award-eligible
    /// confirmation, which the needs/awards model and the hot index both read.
    pub fn mark_qsl_card(&mut self, id: RecordId, received: bool) -> bool {
        self.change_row(id, &[LogOp::MarkQslCard { id, received }], "mark_qsl_card")
    }

    /// Set — or REMOVE — the satellite tag on the contact `id` (`PROP_MODE=SAT` + `SAT_NAME`).
    /// `Some(name)` tags the contact, `None` removes the tag; the name has already been
    /// gated against LoTW's accepted list by the command layer that owns the table.
    ///
    /// A removal MUST be written: the wrongly tagged record is what LoTW and the
    /// Satellite-VUCC fold are reading, and until the store says otherwise the contact keeps
    /// claiming a bird it was never worked through. `PROP_MODE=SAT` diverts a grid out of the
    /// per-band terrestrial sets into the band-independent satellite one, which the hot index
    /// follows. Returns false if no row carries `id`, and for a blank name.
    pub fn set_sat_tag(&mut self, id: RecordId, sat_name: Option<&str>) -> bool {
        self.change_row(
            id,
            &[LogOp::SetSatTag {
                id,
                sat_name: sat_name.map(str::to_string),
            }],
            "set_sat_tag",
        )
    }

    /// Delete the contact `id` (a mis-logged contact). Returns false if no row carries it.
    ///
    /// Another instance's commits are taken in BEFORE the delete is planned
    /// ([`Self::log_plan`]), so only THIS record goes and the other writer's QSOs stay.
    pub fn delete_qso(&mut self, id: RecordId) -> bool {
        self.change_row(id, &[LogOp::Delete(id)], "delete_qso")
    }

    /// Purge the ENTIRE logbook (operator-confirmed, destructive, irreversible).
    /// Clears every contact, empties the store (one statement) and `log.adi` with it, and
    /// resets the worked-entity/grid sets (so the roster B4 highlighting and the needs/awards
    /// model reset too). Returns the number of contacts removed.
    ///
    /// A purge is ONE change, with no plan to make: it reads nothing the store holds, and
    /// whatever the log holds when it runs is what goes.
    #[allow(deprecated)] // SPEC-2 C19 (the cut): the hot index is told the purge row by row, from the log in memory
    pub fn clear_logbook(&mut self) -> usize {
        self.sync_hot();
        let pairs: Vec<RowPair> = self
            .logbook
            .records()
            .iter()
            .map(|r| (Some(Arc::clone(r)), None))
            .collect();
        let n = pairs.len();
        self.commit(
            RowChange {
                class: OpClass::Structural,
                pairs,
                clear: n > 0,
                bulk: true,
                meta: Vec::new(),
            },
            "clear_logbook",
        );
        n
    }

    /// Import an external ADIF logbook: merge (deduped) into the persistent log,
    /// persist it, and return `(added, skipped, updated, total)` — where `updated`
    /// counts records ALREADY logged that the import upgraded. The next propagation
    /// snapshot derives real "needs" from the enlarged log (and roster B4
    /// highlighting updates).
    ///
    /// Two effects in ONE change: new contacts are APPENDED, and the records already in the
    /// log that the import restates — the confirmations and credits a LoTW/eQSL/QRZ download
    /// carries — are upgraded in place. Planned on the rows of the calls it brings
    /// ([`plan_import`]), in one breath here; a command plans it with the Engine lock released
    /// ([`crate::logwrite::import_adif`]).
    ///
    /// ORDER — another instance's commits are taken in before the import is planned
    /// ([`Self::log_plan`]), as before every change. A confirmation report is about contacts
    /// already logged, some of them perhaps only by the OTHER instance: taken in first, such a
    /// row matches the recovered record and confirms it; after, the import reads it as a
    /// brand-new contact and logs it a second time.
    ///
    /// A new row's COUNTRY and STATE are resolved BEFORE it joins the log, so it is appended
    /// complete — and an imported US contact carries its state from the moment it is imported,
    /// in the store as on every screen (SPEC-2 v3 D2-A). Companion mode imports one record per
    /// contact WSJT-X logs, and WSJT-X writes no COUNTRY: filled afterwards, every such contact
    /// was an in-place write — a rewrite of the log's revision (a full reload for every log view)
    /// and a whole-log `save` of log.adi, fsync included, per contact.
    pub fn import_adif(&mut self, text: &str) -> (usize, usize, usize, usize) {
        let (added, skipped, merged) =
            match self.bulk("import_adif", |plan| plan_import(plan, text)) {
                Ok((counts, _)) => counts,
                Err(e) => {
                    tempo_core::applog::error("logbook", &format!("an import was not made: {e}"));
                    (0, 0, 0)
                }
            };
        self.backfill_after_bulk();
        (added, skipped, merged, self.log_len())
    }

    /// Reconcile a confirmation/credit report (ADIF — e.g. a LoTW export) INTO the
    /// existing log: monotonically upgrade matched QSOs' confirmation + credit
    /// (which a plain dedup-import would skip and lose), and return the reconcile summary
    /// (newly confirmed/credited + unmatched orphans). Planned on the rows of the report's calls
    /// ([`plan_report`]).
    pub fn merge_lotw_report(&mut self, text: &str) -> tempo_core::reconcile::ReconcileSummary {
        let summary = self.bulk_or_log("merge_lotw_report", |plan| plan_report(plan, text));
        self.last_lotw_reconcile = Some(summary.clone());
        summary
    }

    /// Stamp POTA/SOTA park refs from a pota.app hunter/activator export onto matching
    /// existing QSOs (stamp-only: never creates records, never overwrites a ref — the
    /// reviewed-adds half is a separate feature). Returns (stamped, already, unmatched).
    pub fn import_pota_log(&mut self, text: &str) -> (usize, usize, usize) {
        self.bulk_or_log("import_pota_log", |plan| plan_ota_refs(plan, text))
    }

    /// Merge a LoTW own-QSO report (`qso_qsl=no`) INTO the log: promote in-flight
    /// uploads (Pending / never-marked) to `Accepted` where LoTW confirms it holds
    /// your record — the step that turns a just-uploaded QSO into "waiting on the
    /// partner" (R2) and clears false "never uploaded" (R1) for out-of-band uploads.
    /// Persists the log when one is promoted ([`plan_own_echo`]). Returns the count newly
    /// promoted.
    pub fn merge_lotw_own_echo(&mut self, text: &str, when_unix: i64) -> usize {
        self.bulk_or_log("merge_lotw_own_echo", |plan| {
            plan_own_echo(plan, text, when_unix)
        })
    }

    /// Record a QRZ Logbook push outcome on the just-pushed QSO (`upload.qrz`), so
    /// the diagnostics can show "never uploaded to QRZ" (R1) / "QRZ upload bounced"
    /// (R9). Returns whether a record was stamped. See [`Self::stamp_push`].
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
        self.stamp_push(pushed, UploadService::Qrz, status)
    }

    /// Record a ClubLog realtime push outcome on the just-pushed QSO
    /// (`upload.clublog`). Returns whether a record was stamped. See [`Self::stamp_push`].
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
        self.stamp_push(pushed, UploadService::Clublog, status)
    }

    /// Record an eQSL ADIF-upload outcome on the just-pushed QSO (`upload.eqsl`).
    /// Returns whether a record was stamped. See [`Self::stamp_push`].
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
        self.stamp_push(pushed, UploadService::Eqsl, status)
    }

    /// Stamp a connector's answer for the QSO it was pushed (`pushed`) on the row that push
    /// names ([`push_target`]), in one breath: planned on the store, made under the lock the
    /// caller holds, planned again while the row keeps changing. Whether a record was stamped.
    /// A command holding the Engine lock does the same with the lock released for the plan
    /// ([`crate::logwrite::stamp_push`]).
    pub(crate) fn stamp_push(
        &mut self,
        pushed: &QsoRecord,
        service: UploadService,
        status: tempo_core::logbook::UploadStatus,
    ) -> bool {
        for _ in 0..PLANS {
            let plan = self.log_plan();
            let target = match push_target(&plan, pushed) {
                Ok(Some(row)) => row,
                Ok(None) => return false,
                Err(e) => {
                    tempo_core::applog::error(
                        "logbook",
                        &format!("an upload stamp could not read the logbook: {e}"),
                    );
                    return false;
                }
            };
            let Some(rows) = stamped_rows(&[target], service, &status) else {
                return false;
            };
            if self
                .commit_planned(
                    &plan,
                    OpClass::Stamp,
                    rows,
                    false,
                    Vec::new(),
                    "upload stamp",
                )
                .is_ok()
            {
                return true;
            }
        }
        tempo_core::applog::warn(
            "logbook",
            "an upload stamp's contact kept changing under it; not stamped",
        );
        false
    }

    /// Merge an eQSL confirmation report into the log. Same generic reconcile path
    /// as [`Self::merge_lotw_report`]; the award-grade distinction lives in the
    /// ADIF (eQSL carries `EQSL_QSL_RCVD`, not `QSL_RCVD`/`LOTW_QSL_RCVD`), so an
    /// eQSL confirmation lands `confirmed` but NOT `award_confirmed` by construction.
    pub fn merge_eqsl_report(&mut self, text: &str) -> tempo_core::reconcile::ReconcileSummary {
        let summary = self.bulk_or_log("merge_eqsl_report", |plan| plan_report(plan, text));
        self.last_eqsl_reconcile = Some(summary.clone());
        summary
    }

    /// Two-way QRZ Logbook sync: merge a QRZ **FETCH** ADIF (the operator's whole
    /// book) INTO the log. QRZ returns both QSOs the operator logged elsewhere (e.g.
    /// a phone app in the field) AND confirmation status, so this adds the QSOs QRZ has that
    /// we lack AND upgrades confirmations on the ones already present, in ONE consume-once
    /// pass keyed identically, so a mode-spelling difference (e.g. a phone QSO re-uploaded as
    /// USB vs our SSB) can't double-log the same contact ([`plan_download`]). A QRZ-native
    /// confirmation (`APP_QRZLOG_STATUS`) lands `confirmed` but NOT `award_confirmed`, by
    /// construction of the `qrz` channel, so it can't inflate DXCC/WAS counts. Returns
    /// `(added, reconcile_summary)`.
    pub fn merge_qrz_report(
        &mut self,
        text: &str,
    ) -> (usize, tempo_core::reconcile::ReconcileSummary) {
        let (added, summary) =
            self.bulk_or_log("merge_qrz_report", |plan| plan_download(plan, text));
        self.last_qrz_reconcile = Some(summary.clone());
        self.backfill_after_bulk();
        (added, summary)
    }

    /// ★ Plan a bulk change on the candidate sub-log and make it, in one breath — for an owner
    /// holding the station with no Engine guard (a test's own station or engine; a command
    /// plans with the lock released, [`crate::logwrite`]). Planned again while what it read keeps
    /// changing, [`PLANS`] plans in all. What the plan answered and the rows it appended, or why
    /// not.
    pub(crate) fn bulk<R>(
        &mut self,
        context: &str,
        mut plan_it: impl FnMut(&LogPlan) -> Result<(R, Planned), String>,
    ) -> Result<(R, Vec<Arc<QsoRecord>>), String> {
        for _ in 0..PLANS {
            let plan = self.log_plan();
            let (out, planned) = plan_it(&plan)?;
            if planned.is_empty() {
                return Ok((out, Vec::new()));
            }
            if let Ok((_, appended)) = self.commit_bulk(&plan, planned, context) {
                return Ok((out, appended));
            }
        }
        Err(LOG_BUSY.into())
    }

    /// [`Self::bulk`] for a change whose answer is a report: one that could not be made says so
    /// in the diagnostic log and answers an empty report.
    fn bulk_or_log<R: Default>(
        &mut self,
        context: &str,
        plan_it: impl FnMut(&LogPlan) -> Result<(R, Planned), String>,
    ) -> R {
        match self.bulk(context, plan_it) {
            Ok((out, _)) => out,
            Err(e) => {
                tempo_core::applog::error("logbook", &format!("{context} was not made: {e}"));
                R::default()
            }
        }
    }

    /// After a bulk change on the 1.13 path, what it has always done next: the backfill of every
    /// record's country, from the resolver. The store's rows are filled another way (SPEC-2 v3
    /// D2-A): each insert fills its own, and the fill job the ones an older build left.
    pub(crate) fn backfill_after_bulk(&mut self) {
        if self.store.is_none() {
            self.backfill_country();
        }
    }

    /// How many contacts the log holds — read with no Engine guard, as a count of the store is.
    fn log_len(&self) -> usize {
        match self.log_rows().count() {
            Ok((n, _)) => n as usize,
            Err(e) => {
                tempo_core::applog::error("logbook", &format!("the log could not be counted: {e}"));
                0
            }
        }
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

    /// Stamp `upload.lotw` on the contacts `ids` names — the operator's "already uploaded"
    /// declaration — in one breath: planned on the store, made under the lock the caller holds,
    /// planned again while a contact keeps changing. By id, so a contact deleted since the ids
    /// were taken is simply not found; a batch TQSL signed goes through
    /// [`Self::stamp_lotw_batch`], which also refuses a contact changed since it was signed.
    /// How many were stamped; none — said in the diagnostic log — when the store could not be
    /// read or the contacts kept changing ([`RowRefusal::Busy`]).
    pub fn stamp_lotw_upload(
        &mut self,
        ids: &[RecordId],
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) -> usize {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        self.stamp_lotw_ids(ids, &status).unwrap_or_else(|e| {
            tempo_core::applog::warn("logbook", &format!("a LoTW upload stamp: {e}"));
            0
        })
    }

    /// [`Self::stamp_lotw_upload`]'s plan and commit.
    fn stamp_lotw_ids(
        &mut self,
        ids: &[RecordId],
        status: &tempo_core::logbook::UploadStatus,
    ) -> Result<usize, String> {
        for _ in 0..PLANS {
            let plan = self.log_plan();
            let found = plan.rows(ids)?;
            // In the order `ids` names them, each once.
            let mut seen = HashSet::new();
            let rows: Vec<Arc<QsoRecord>> = ids
                .iter()
                .filter(|id| seen.insert(**id))
                .filter_map(|id| found.get(id).cloned())
                .collect();
            let Some(pairs) = stamped_rows(&rows, UploadService::Lotw, status) else {
                return Ok(0);
            };
            let n = pairs.len();
            let bulk = n > tempo_core::logbook::writer::CHUNK_ROWS;
            if self
                .commit_planned(
                    &plan,
                    OpClass::Stamp,
                    pairs,
                    bulk,
                    Vec::new(),
                    "lotw upload stamp",
                )
                .is_ok()
            {
                return Ok(n);
            }
        }
        Err(LOG_BUSY.into())
    }

    /// Stamp `upload.lotw` on the contacts of a batch TQSL signed — found BY ID, and only where
    /// the row is still the one that was signed — in one breath (see [`stamp_lotw_batch_plan`]
    /// for the rule; a command holding the Engine lock plans with it released,
    /// [`crate::logwrite::stamp_lotw_batch`]).
    pub fn stamp_lotw_batch(
        &mut self,
        batch: &[LotwSigned],
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) -> LotwStamped {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        for _ in 0..PLANS {
            let plan = self.log_plan_moving();
            let (report, pairs) = match stamp_lotw_batch_plan(&plan, batch, &status) {
                Ok(planned) => planned,
                Err(e) => {
                    tempo_core::applog::error(
                        "logbook",
                        &format!("a LoTW upload's stamps could not read the logbook: {e}"),
                    );
                    return LotwStamped {
                        unrecorded: batch.len(),
                        ..LotwStamped::default()
                    };
                }
            };
            let Some(pairs) = pairs else {
                return report;
            };
            let bulk = pairs.len() > tempo_core::logbook::writer::CHUNK_ROWS;
            if self
                .commit_planned(
                    &plan,
                    OpClass::Stamp,
                    pairs,
                    bulk,
                    Vec::new(),
                    "lotw upload stamp",
                )
                .is_ok()
            {
                return report;
            }
        }
        LotwStamped {
            unrecorded: batch.len(),
            ..LotwStamped::default()
        }
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

    /// Every badge question the snapshot asks, for a fixed set of probes — what two stations can
    /// be compared by.
    fn badge_answers(sc: &StationCore) -> Vec<bool> {
        let hot = sc.hot();
        let mut out = Vec::new();
        for band in ["20m", "40m", "2m"] {
            for grid in ["FN31", "JO31", "FN42"] {
                out.push(hot.grid_worked_on(grid, band));
            }
            for entity in ["W1", "DL", "K1"] {
                out.push(hot.entity_worked_on(entity, band));
                out.push(hot.entity_confirmed_on(entity, band));
            }
        }
        for entity in ["W1", "DL", "K1"] {
            out.push(hot.entity_worked_ever(entity));
        }
        drop(hot);
        for park in ["US-0001", "US-0002"] {
            out.push(sc.park_worked(park));
        }
        out
    }

    /// The badge index follows the log row by row: ONE DXCC lookup per logged contact — not one
    /// per row of the whole log, under the engine lock, on every contact — and NONE to take a
    /// contact out, because a row leaves under the entity it was counted with. Either way the
    /// answers are a fresh station's over the same rows, or a NEW DXCC / NEW GRID / NEW PARK
    /// badge lies.
    #[test]
    fn the_badges_follow_each_contact_with_one_lookup_and_equal_a_rebuild() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        fn counting(
            lookups: Arc<AtomicUsize>,
        ) -> impl Fn(&str) -> Option<String> + Send + Sync + 'static {
            move |call| {
                lookups.fetch_add(1, Ordering::Relaxed);
                call.get(..2).map(str::to_string)
            }
        }
        fn rebuilt(sc: &StationCore) -> Vec<bool> {
            let mut fresh = StationCore::new();
            fresh.set_dxcc_resolver(counting(Arc::new(AtomicUsize::new(0))));
            for r in sc.logbook.records() {
                fresh.logbook.add(r.as_ref().clone());
            }
            fresh.sync_hot();
            badge_answers(&fresh)
        }
        let lookups = Arc::new(AtomicUsize::new(0));
        let mut sc = StationCore::new();
        sc.set_dxcc_resolver(counting(lookups.clone()));

        // Confirmed as a contact is: through a channel (LoTW), which is what makes it
        // award-confirmed — the store derives the flags from the channel it keeps.
        let mut confirmed = rec("DL1ABC", "20m", "JO31");
        confirmed.qsl_rcvd.lotw = true;
        confirmed.confirmed = true;
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
            sc.append(vec![r], false);
            sc.sync_hot();
            assert_eq!(
                lookups.load(Ordering::Relaxed) - before,
                1,
                "row {i}: one lookup for one appended row"
            );
            assert_eq!(
                badge_answers(&sc),
                rebuilt(&sc),
                "row {i}: the followed index IS the rebuild"
            );
        }
        assert!(sc.hot().entity_confirmed_on("DL", "20m"), "premise");

        // A delete, through the station: the deleted row's slots leave the index, and no row is
        // looked up again.
        let before = lookups.load(Ordering::Relaxed);
        assert!(sc.delete_qso(sc.logbook.records()[1].id.unwrap()));
        assert_eq!(
            lookups.load(Ordering::Relaxed) - before,
            0,
            "a delete looks nothing up"
        );
        assert_eq!(badge_answers(&sc), rebuilt(&sc));
        assert!(
            !sc.hot().entity_confirmed_on("DL", "20m"),
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
        sc.sync_hot();

        assert!(
            sc.hot().grid_worked_on("FN31", "20m"),
            "a 6-char logged grid must satisfy a 4-char decode on the same band"
        );
        // And the per-band rule still holds on top of it.
        assert!(
            !sc.hot().grid_worked_on("FN31", "2m"),
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
        sc.sync_hot();
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
        sc.sync_hot();
        assert_eq!(sc.last_worked_unix("W6A"), None, "control: an empty log");

        let mut late = rec("W6A/P", "40m", "DM04");
        late.when_unix = 5_000;
        let mut early = rec("w6a", "20m", "DM04");
        early.when_unix = 1_000;
        // The later contact goes in FIRST, so "latest wins" cannot be "last row wins".
        sc.logbook.add(late);
        sc.logbook.add(early);
        sc.sync_hot();
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

        // Deleting the later contact falls back to the earlier one — the answer follows the
        // rows, it does not accumulate.
        sc.logbook
            .apply(LogOp::Delete(sc.logbook.records()[0].id.unwrap()));
        sc.sync_hot();
        assert_eq!(sc.last_worked_unix("W6A"), Some(1_000));
    }

    #[test]
    fn a_malformed_grid_never_counts_as_worked() {
        let mut sc = StationCore::new();
        sc.logbook.add(rec("W1AW", "20m", "FN"));
        sc.sync_hot();
        assert!(
            !sc.hot().grid_worked_on("FN", "20m"),
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
        // The session with no store at all — the last resort, `log.adi` written by 1.13's own
        // code. The 1.13 path's own twin is the test after this one.
        sc.store = None;
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

    /// The same on the 1.13 path, its log in a store in memory (SPEC-2 v3 C19, D1-A): the file
    /// the launch loaded is the file the lane may replace, so nothing reads it again; a companion
    /// import of one contact is APPENDED to it, as 1.13's was — the file is the same file,
    /// grown — and leaves it accounted, so the look before the next change does not re-read the
    /// whole log either.
    #[cfg(unix)]
    #[test]
    fn a_companion_import_on_the_1_13_path_is_appended_and_leaves_the_gate_shut() {
        use std::os::unix::fs::MetadataExt;
        use tempo_core::logbook::{adif_header, adif_record_own_log};
        let dir =
            std::env::temp_dir().join(format!("nexus_append_stamp_d1a_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.adi");
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
        let lane = std::sync::Arc::clone(sc.store.as_ref().and_then(LogStore::lane).unwrap());
        assert!(
            !sc.take_in_log_file_if_changed(),
            "the file the launch read is not read again"
        );
        let file = std::fs::metadata(&path).unwrap().ino();
        let (added, _, merged, _) = sc.import_adif(
            "<EOH>\n<CALL:5>K5XYZ<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260804<TIME_ON:6>120000<EOR>\n",
        );
        assert_eq!(
            (added, merged),
            (1, 0),
            "fixture: one contact added, none upgraded"
        );
        let st = lane.flush(std::time::Duration::from_secs(60));
        assert!(!st.pending(), "{st:?}");
        assert_eq!(
            std::fs::metadata(&path).unwrap().ino(),
            file,
            "appended to the file, not a new file renamed over it"
        );
        let calls: Vec<String> = Logbook::load(&path)
            .records()
            .iter()
            .map(|r| r.call.clone())
            .collect();
        assert_eq!(calls, ["W1AW", "K5XYZ"], "the contact is in it");
        assert!(
            !sc.take_in_log_file_if_changed(),
            "our own append must not reopen the gate — every later call would re-read the log"
        );
        assert_eq!(
            sc.logbook.len(),
            2,
            "and nothing was re-read or double-counted"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
        // The session with no store at all: 1.13's own code (the 1.13 path's twin follows).
        sc.store = None;
        assert!(sc.recover_external_appends(), "we hold W1AW, gate shut");

        // Instance A appends a contact we never see in memory.
        Logbook::append(&path, &rec("W3CCC", "40m", "IO91")).unwrap();

        // We log our own contact through the station's append, as log_qso does: memory first,
        // then the file.
        sc.append(vec![rec("K5XYZ", "20m", "FN31")], false);
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

    /// The same on the 1.13 path, its log in a store in memory (SPEC-2 v3 C19, D1-A): our
    /// contact is appended by the lane onto a file another instance appended to since we last
    /// looked; the file is then not accounted, so the look before the next change reads it and
    /// takes the other contact in, and the rewrite that change makes keeps it.
    #[test]
    fn an_append_onto_a_file_that_moved_under_us_leaves_it_unaccounted_on_the_1_13_path() {
        use tempo_core::logbook::{adif_header, adif_record_own_log};
        let dir =
            std::env::temp_dir().join(format!("nexus_append_stale_d1a_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.adi");
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
        let lane = std::sync::Arc::clone(sc.store.as_ref().and_then(LogStore::lane).unwrap());
        let wait = std::time::Duration::from_secs(60);

        // Instance A appends a contact we never see in memory.
        Logbook::append(&path, &rec("W3CCC", "40m", "IO91")).unwrap();

        // We log our own contact through the station's append, as log_qso does.
        let _ = sc.append(vec![rec("K5XYZ", "20m", "FN31")], false);
        let st = lane.flush(wait);
        assert!(!st.pending(), "{st:?}");
        assert_eq!(
            st.known, None,
            "a file we cannot account for is not recorded as ours"
        );

        // The next change to a contact the log holds: the look before it takes A's in.
        let w1 = sc.logbook.records()[0].id.unwrap();
        assert!(sc.mark_qsl_sent(w1, Some(tempo_core::logbook::QslVia::Direct)));
        let st = lane.flush(wait);
        assert!(!st.pending(), "{st:?}");
        let on_disk = Logbook::load(&path);
        let calls: Vec<&str> = on_disk.records().iter().map(|r| r.call.as_str()).collect();
        assert!(
            calls.contains(&"W3CCC"),
            "another instance's append survives our rewrite (on disk: {calls:?})"
        );
        assert_eq!(on_disk.len(), 3, "...and nothing is double-logged");
        assert!(
            on_disk.records()[0].qsl_sent.sent,
            "and our change is in it"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
        // The 1.13 path: `log.adi` is the log, and there is no store.
        sc.store = None;
        sc.log_path = Some(path.clone());

        // First look (last mtime = None) folds X in and indexes it.
        assert!(
            sc.sync_shared_log_if_changed(),
            "first look reads the shared log"
        );
        assert_eq!(sc.logbook.len(), 1);
        assert!(
            sc.hot().grid_worked_on("JO31", "20m"),
            "X is now worked-before"
        );

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
            sc.hot().grid_worked_on("PM95", "40m"),
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

// Debug builds only: it reads the index's rebuild counter, which a release build compiles away.
#[cfg(all(test, debug_assertions))]
mod hot_parity_tests {
    //! SPEC-2's C13 at the station: the hot index, followed through the station's OWN write
    //! paths — every kind the app has — answers exactly as the code before C13 did, after every
    //! change, and is never rebuilt to do it. The oracles are that code, verbatim, over the
    //! in-memory log the station still holds.
    use super::*;
    use tempo_core::logbook::dedup::scan_for_duplicate;
    use tempo_core::logbook::hot::{HOT_CATCH_UPS, HOT_REBUILDS};
    use tempo_core::logbook::{adif_header, adif_record, QslVia, UploadOutcome};
    use tempo_core::message::{base_call, same_call};

    /// SplitMix64 — a deterministic stream, so a failing seed replays.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
            xs[self.below(xs.len())]
        }
        fn chance(&mut self, percent: u64) -> bool {
            self.next() % 100 < percent
        }
    }

    const CALLS: &[&str] = &[
        "W1AW",
        "w1aw",
        "W1AW/P",
        "KH6/W1AW",
        "<W1AW>",
        "K1ABC",
        "K1ABC/MM",
        "VP2E/AA9A",
        "AA9A",
        "DL1ZZZ/4",
        "dl1zzz",
        "JA1XYZ",
        "<...>",
        "N0OLD",
    ];
    const BANDS: &[&str] = &["20m", "20M", "40m", "2m", "6m", ""];
    const MODES: &[&str] = &["FT8", "ft8", "FT4", "CW", "SSB"];
    const GRIDS: &[Option<&str>] = &[
        None,
        Some(""),
        Some("FN31"),
        Some("fn31pr"),
        Some("JO3"),
        Some("EM12"),
        Some(" IO91 "),
    ];
    const PROPS: &[Option<&str>] = &[None, None, None, Some("SAT")];
    const THEIR: &[Option<&str>] = &[
        None,
        None,
        Some("US-0001"),
        Some("US-0001,US-0002"),
        Some(" k-0001 "),
    ];
    const MINE: &[Option<&str>] = &[None, None, Some("US-1234")];
    const T0: u64 = 1_788_000_000;
    const FD: tempo_core::contest::DupeRule = tempo_core::contest::DupeRule {
        by_call: true,
        by_band: true,
        by_mode_class: true,
        by_fields: &[],
        by_sent_fields: &[],
        mode_class_groups: &[],
        log_dupes: false,
        satellite_is_a_band: false,
        fm_satellite_once: false,
    };

    fn resolve(call: &str) -> Option<String> {
        let base = base_call(call);
        (base.len() >= 3).then(|| base[..2].to_string())
    }

    fn contact(rng: &mut Rng) -> QsoRecord {
        let mut r = QsoRecord {
            id: None,
            call: String::new(),
            grid: None,
            country: None,
            state: None,
            band: String::new(),
            freq_mhz: 14.074,
            freq_rx_mhz: None,
            mode: String::new(),
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
        };
        r.call = rng.pick(CALLS).to_string();
        r.band = rng.pick(BANDS).to_string();
        r.mode = rng.pick(MODES).to_string();
        r.when_unix = T0 + rng.below(4_000) as u64;
        r.grid = rng.pick(GRIDS).map(str::to_string);
        r.prop_mode = rng.pick(PROPS).map(str::to_string);
        r.ota.their_ref = rng.pick(THEIR).map(str::to_string);
        r.ota.my_ref = rng.pick(MINE).map(str::to_string);
        r
    }

    /// One row of the log, as a report or an export would restate it, with `extra` tags.
    fn restated(sc: &StationCore, rng: &mut Rng, extra: &str) -> Option<String> {
        let n = sc.logbook.len();
        (n > 0).then(|| {
            let row = adif_record(&sc.logbook.records()[rng.below(n)]);
            let at = row.rfind("<EOR>").expect("a record ends");
            format!("{}{}{extra}{}", adif_header(), &row[..at], &row[at..])
        })
    }

    /// The id of the contact at `at`: how the property names a row it picked by place.
    fn id_at(sc: &StationCore, at: usize) -> RecordId {
        sc.logbook.records()[at]
            .id
            .expect("every row the log holds carries an id")
    }

    /// Every change the station makes to its log, through the method the app calls.
    fn change(sc: &mut StationCore, rng: &mut Rng) -> &'static str {
        let n = sc.logbook.len();
        match rng.below(18) {
            0..=4 => {
                // `Engine::log_qso`'s append: the station's, one change.
                sc.append(vec![contact(rng)], false);
                "log a contact"
            }
            5 if n > 0 => {
                let id = id_at(sc, rng.below(n));
                sc.update_qso(id, contact(rng));
                "edit"
            }
            6 if n > 0 => {
                let id = id_at(sc, rng.below(n));
                sc.delete_qso(id);
                "delete"
            }
            7 if rng.chance(15) => {
                sc.clear_logbook();
                "purge"
            }
            8 => {
                let rows: Vec<QsoRecord> = (0..1 + rng.below(3)).map(|_| contact(rng)).collect();
                let mut text = adif_header();
                for r in &rows {
                    text.push_str(&adif_record(r));
                }
                sc.import_adif(&text);
                "import"
            }
            9 => match restated(sc, rng, "<LOTW_QSL_RCVD:1>Y") {
                Some(report) => {
                    sc.merge_lotw_report(&report);
                    "LoTW confirmation"
                }
                None => "nothing",
            },
            10 => match restated(sc, rng, "<EQSL_QSL_RCVD:1>Y") {
                Some(report) => {
                    sc.merge_eqsl_report(&report);
                    "eQSL confirmation"
                }
                None => "nothing",
            },
            11 => match restated(sc, rng, "<QSL_RCVD:1>Y") {
                Some(report) => {
                    sc.merge_qrz_report(&report);
                    "QRZ download"
                }
                None => "nothing",
            },
            12 if n > 0 => {
                // A POTA export naming a park for a contact that has none.
                let mut r = QsoRecord::clone(&sc.logbook.records()[rng.below(n)]);
                r.ota.their_program = Some("POTA".into());
                r.ota.their_ref = Some("US-0002".into());
                sc.import_pota_log(&format!("{}{}", adif_header(), adif_record(&r)));
                "POTA park stamp"
            }
            13 if n > 0 => {
                let pushed = QsoRecord::clone(&sc.logbook.records()[rng.below(n)]);
                match rng.below(3) {
                    0 => sc.stamp_qrz_upload(&pushed, UploadOutcome::Accepted, 1, None),
                    1 => sc.stamp_clublog_upload(&pushed, UploadOutcome::Accepted, 1, None),
                    _ => sc.stamp_eqsl_upload(&pushed, UploadOutcome::Accepted, 1, None),
                };
                "connector stamp"
            }
            14 if n > 0 => {
                let id = id_at(sc, rng.below(n));
                sc.stamp_lotw_upload(&[id], UploadOutcome::Accepted, 1, None);
                "LoTW stamp"
            }
            15 if n > 0 => {
                let id = id_at(sc, rng.below(n));
                if rng.chance(50) {
                    sc.mark_qsl_sent(id, Some(QslVia::Bureau));
                    "QSL sent"
                } else {
                    sc.mark_qsl_card(id, rng.chance(70));
                    "QSL card"
                }
            }
            16 if n > 0 => {
                let id = id_at(sc, rng.below(n));
                sc.set_sat_tag(id, rng.chance(50).then_some("SO-50"));
                "satellite tag"
            }
            17 => {
                sc.set_state_resolver(|call, _| call.starts_with('W').then(|| "MA".into()));
                "state backfill"
            }
            _ => "nothing",
        }
    }

    /// Every answer the hot index gives, against the code it replaced.
    fn assert_parity(sc: &StationCore, rng: &mut Rng, what: &str) {
        let log = &sc.logbook;
        // The oracles.
        let calls = log.worked_call_set();
        let bands = log.worked_band_set(false);
        let band_modes = log.worked_band_set(true);
        let mut old_grids = HashSet::new();
        let mut old_entities = HashSet::new();
        let mut old_confirmed = HashSet::new();
        let mut old_parks = HashSet::new();
        let mut old_last = HashMap::new();
        for r in log.records() {
            let base = base_call(&r.call);
            if !base.is_empty() {
                let at = old_last.entry(base).or_insert(r.when_unix);
                *at = (*at).max(r.when_unix);
            }
            if let Some(refs) = &r.ota.their_ref {
                for p in refs.split([',', ';']) {
                    let p = p.trim();
                    if !p.is_empty() {
                        old_parks.insert(p.to_uppercase());
                    }
                }
            }
            let band = band_key(&r.band);
            if let Some(g) = &r.grid {
                let sat = r
                    .prop_mode
                    .as_deref()
                    .is_some_and(|p| p.trim().eq_ignore_ascii_case("SAT"));
                let g4: String = g.trim().to_ascii_uppercase().chars().take(4).collect();
                if !sat && g4.len() == 4 {
                    old_grids.insert((g4, band.clone()));
                }
            }
            if let Some(entity) = resolve(&r.call) {
                if r.award_confirmed {
                    old_confirmed.insert((entity.clone(), band.clone()));
                }
                old_entities.insert((entity, band));
            }
        }

        let hot = sc.hot();
        // The duplicate guard: probes, and every row logged again.
        let mut probes: Vec<QsoRecord> = (0..6).map(|_| contact(rng)).collect();
        probes.extend(log.records().iter().map(|r| QsoRecord::clone(r)));
        for p in &probes {
            assert_eq!(
                hot.is_duplicate(p),
                scan_for_duplicate(log, p),
                "after {what}: dedup {:?} {} {} @{}",
                p.call,
                p.band,
                p.mode,
                p.when_unix
            );
        }
        for call in CALLS {
            let up = call.to_ascii_uppercase();
            assert_eq!(
                hot.worked_call(&up),
                calls.contains(&up),
                "after {what}: B4 {up}"
            );
            for band in BANDS {
                for mode in MODES {
                    for (fold, set) in [(false, &bands), (true, &band_modes)] {
                        let key = Logbook::band_key(band, mode, fold);
                        assert_eq!(
                            hot.worked_call_band(&up, &key, fold),
                            set.contains(&(up.clone(), key.clone())),
                            "after {what}: B4 {up} {key:?}"
                        );
                    }
                }
            }
            // The partner's grid: `dx_grid_resolved`'s scan, verbatim.
            let old_grid = log
                .records()
                .iter()
                .rev()
                .find(|r| same_call(&r.call, call))
                .and_then(|r| r.grid.clone())
                .filter(|g| !g.trim().is_empty());
            assert_eq!(
                hot.newest_grid(call),
                old_grid,
                "after {what}: grid of {call}"
            );
            assert_eq!(
                hot.last_worked(call),
                old_last.get(&base_call(call)).copied(),
                "after {what}: last worked {call}"
            );
        }
        for band in BANDS {
            for grid in ["FN31", "JO31", "EM12", "IO91", "fn31pr"] {
                let g4: String = grid.to_ascii_uppercase().chars().take(4).collect();
                assert_eq!(
                    hot.grid_worked_on(grid, band),
                    old_grids.contains(&(g4, band_key(band))),
                    "after {what}: grid {grid} on {band:?}"
                );
            }
            for entity in ["W1", "K1", "AA", "DL", "JA", "N0", "VP", "KH"] {
                let slot = (entity.to_string(), band_key(band));
                assert_eq!(
                    hot.entity_worked_on(entity, band),
                    old_entities.contains(&slot),
                    "after {what}: {entity} on {band:?}"
                );
                assert_eq!(
                    hot.entity_confirmed_on(entity, band),
                    old_confirmed.contains(&slot),
                    "after {what}: {entity} confirmed on {band:?}"
                );
                assert_eq!(
                    hot.entity_worked_ever(entity),
                    old_entities.iter().any(|(e, _)| e == entity),
                    "after {what}: {entity} ever"
                );
            }
        }
        drop(hot);
        for park in ["US-0001", "us-0002", "K-0001", "US-9999"] {
            let key = park.trim().to_uppercase();
            assert_eq!(
                sc.park_worked(park),
                old_parks.contains(&key) || sc.hunted_parks_import.contains(&key),
                "after {what}: park {park}"
            );
        }
        assert_eq!(
            sc.activation_qso_count(),
            log.records()
                .iter()
                .filter(|r| r.ota.my_ref.as_deref() == Some("US-1234"))
                .count(),
            "after {what}: activation count"
        );
        assert_eq!(
            sc.worked_since(T0 + 1_000, &FD),
            log.worked_keys_since(T0 + 1_000, &FD),
            "after {what}: the session sweep"
        );
    }

    /// ★ Seeded random runs of every write the station makes, with the oracles asked after each
    /// — and the index rebuilt NOT ONCE after the station set it up: every change, the stamps
    /// and the merges included, is followed row by row. And followed through ONE door
    /// ([`StationCore::follow`], SPEC-2 v3 C19): after every write the index already holds the
    /// log, before anything asks it, and no catch-up from the log is ever needed; the watermarks
    /// the station keeps are the log's own.
    #[test]
    fn the_station_answers_as_before_c13_through_every_write_path_without_a_rebuild() {
        for seed in 0..24u64 {
            let mut rng = Rng(seed);
            let mut sc = StationCore::new();
            sc.set_dxcc_resolver(resolve);
            sc.set_hunted_parks_import(["K-0001".to_string()]);
            sc.activation = Some(("POTA".into(), "US-1234".into()));
            sc.sync_hot();
            HOT_REBUILDS.with(|c| c.set(0));
            HOT_CATCH_UPS.with(|c| c.set(0));
            for step in 0..60 {
                let what = change(&mut sc, &mut rng);
                let at = format!("seed {seed} step {step}: {what}");
                assert_eq!(
                    sc.hot.lock().expect("not poisoned").at(),
                    Some(sc.logbook.revision()),
                    "{at}: the index does not hold the log — the write was not followed"
                );
                assert_eq!(
                    (sc.marks(), sc.log_tick()),
                    (sc.logbook.marks(), sc.logbook.revision() as u32),
                    "{at}: the station's watermarks, or the snapshot's tick, are not the log's"
                );
                assert_eq!(
                    (
                        HOT_REBUILDS.with(|c| c.get()),
                        HOT_CATCH_UPS.with(|c| c.get())
                    ),
                    (0, 0),
                    "{at}: the index was rebuilt, or caught up from the log — the station's \
                     own writes are followed row by row, through `follow`"
                );
                assert_parity(&sc, &mut rng, &at);
            }
        }
    }

    /// A write that reached the log around the station — a test's, or a path not yet taught — is
    /// taken in before the next change is walked, so the change is still followed row by row:
    /// the picture a change starts from is the one the index holds.
    #[test]
    fn a_write_around_the_station_is_taken_in_before_the_next_change() {
        let mut rng = Rng(7);
        let mut sc = StationCore::new();
        sc.set_dxcc_resolver(resolve);
        sc.activation = Some(("POTA".into(), "US-1234".into()));
        for _ in 0..5 {
            sc.append(vec![contact(&mut rng)], false);
        }
        HOT_REBUILDS.with(|c| c.set(0));
        HOT_CATCH_UPS.with(|c| c.set(0));
        // Around the station: an append straight onto the log.
        sc.logbook.add(contact(&mut rng));
        let second = id_at(&sc, 1);
        assert!(sc.update_qso(second, contact(&mut rng)));
        assert_eq!(
            HOT_REBUILDS.with(|c| c.get()),
            0,
            "the edit after it was followed, not rebuilt"
        );
        // The POSITIVE CONTROL for the property's "no catch-up": a write made around the station
        // is exactly what one counts.
        assert_eq!(
            HOT_CATCH_UPS.with(|c| c.get()),
            1,
            "the append around the station was caught up from the log, once"
        );
        assert_eq!(sc.marks(), sc.logbook.marks());
        assert_parity(&sc, &mut rng, "a write around the station, then an edit");
    }

    /// The station mints every id the log hands out (SPEC-2 v3 C19) — the contacts it logs and
    /// the rows an import adds (C19 Part B) — under its position id and from one sequence, so no
    /// two rows are ever handed one id.
    #[test]
    fn what_the_station_logs_and_what_an_import_adds_are_minted_under_one_position() {
        let mut rng = Rng(3);
        let mut sc = StationCore::new();
        sc.set_posid(0x00c0_ffee);
        let mut logged = contact(&mut rng);
        logged.call = "W1AW".into();
        let logged = sc.append(vec![logged], false).0[0]
            .id
            .expect("the station minted it");
        let mut imported = contact(&mut rng);
        imported.call = "K1ABC".into();
        sc.import_adif(&format!("{}{}", adif_header(), adif_record(&imported)));
        let imported = sc.logbook.records()[1]
            .id
            .expect("an import's row carries an id");
        match (logged, imported) {
            (
                RecordId::Minted {
                    posid: a,
                    nonce: x,
                    seq: m,
                },
                RecordId::Minted {
                    posid: b,
                    nonce: y,
                    seq: n,
                },
            ) => {
                assert_eq!((a, b), (0x00c0_ffee, 0x00c0_ffee), "the station's position");
                assert_eq!(x, y, "one minter");
                assert!(n > m, "the import's id is the next in its sequence");
            }
            other => panic!("both minted here: {other:?}"),
        }
    }

    /// ★ POSITIVE CONTROL for the debug build's dual execution: the partner's logged grid is the
    /// NEWEST row's — asked of the index and, in a debug build, of the scan it replaced. Two rows
    /// of one station with different grids are what tell "newest" from "oldest"; if the index
    /// ever answered otherwise, the scan's answer would stop the build here.
    #[test]
    fn the_logged_grid_is_the_newest_rows_and_the_scan_checks_it() {
        let mut sc = StationCore::new();
        let mut older = contact(&mut Rng(1));
        older.call = "W1AW".into();
        older.grid = Some("FN31".into());
        let mut newer = older.clone();
        newer.call = "W1AW/P".into();
        newer.grid = Some("EM12".into());
        sc.append(vec![older, newer], false);
        sc.sync_hot();
        assert_eq!(sc.newest_logged_grid("w1aw").as_deref(), Some("EM12"));
        assert!(sc.is_duplicate(&sc.logbook.records()[0].as_ref().clone()));
    }
}

#[cfg(test)]
mod bulk_tests;
