//! Persistent QSO logbook (ADIF).
//!
//! Records completed contacts across sessions (so they survive restart, unlike
//! the live roster) and answers "worked before?" for dupe / B4 highlighting.
//! Stored as an ADIF file the operator can import into any logger. This is the
//! general logbook for Chat/QSO contacts — Field Day keeps its own contest log
//! ([`crate::fieldday`]).
//!
//! # The log is the one thing here that cannot be regenerated
//!
//! Two safety copies, and they answer different questions:
//!
//! - **The anchor** — `log.adi.bak`, beside the log, written by [`Logbook::backup_once`] the
//!   first time a non-empty log is loaded and **never touched again**. It answers "what did
//!   this log look like before this build ever wrote to it", which is the only question that
//!   helps when the PARSER is what lost the records. It is never rotated and never deleted.
//! - **The ring** — dated snapshots in a `backups/` folder beside the log, taken on the SAVE
//!   path by [`Logbook::snapshot_before_save`]. It answers "what did the log look like last
//!   week", and it is bounded three ways so it cannot grow without limit.
//!
//! **The snapshot trigger is SAVE, never LOAD, and that is deliberate.** [`Logbook::save`]
//! already rewrites the whole file, so one extra copy costs one more pass over a file the OS
//! is touching anyway — and launch pays nothing. Snapshotting at load time would put a
//! whole-file copy of a multi-MB log on the startup path, which is exactly what the operator
//! ruled out (2026-08: "a big log must not make launch slow"). `load` writes at most the
//! one-time anchor and, after that, nothing at all.
//!
//! **Why the snapshot is a COPY and not a hard link.** A link would be free, but
//! [`Logbook::append`] opens the log with `.append(true)` and mutates it **in place** — a
//! hard link is the same inode, so it would follow every later append instead of freezing
//! the bytes, and the "snapshot" would silently be a second name for the live file. Do not
//! "optimise" the copy into a link.

use std::path::Path;

// Whole-log sweep counter, DEBUG BUILDS ONLY — instrumentation for the
// traversal-bound test. A per-row `worked_before()` inside `snapshot()` once
// held the engine mutex long enough to stall the waterfall for 1–2 s, was
// fixed with a prebuilt set, and then regrew 240 lines below the fix's own
// comment. A test that pins "snapshot performs a bounded number of sweeps"
// stops the SHAPE from recurring, not just the instance. Release builds
// compile this away. PER-THREAD, not a process global: the test harness runs
// tests in parallel, and a shared counter would count every OTHER test's
// sweeps into the bound. (Plain comments: doc comments can't attach through
// the thread_local! macro.)
#[cfg(debug_assertions)]
thread_local! {
    pub static LOG_SWEEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[inline]
fn note_log_sweep() {
    #[cfg(debug_assertions)]
    LOG_SWEEPS.with(|c| c.set(c.get() + 1));
}

/// One logged contact.
#[derive(Debug, Clone, PartialEq)]
pub struct QsoRecord {
    pub call: String,
    pub grid: Option<String>,
    /// DXCC entity name (ADIF `COUNTRY`), resolved from the callsign at log time.
    /// The single most important derived field for a DXer — every award is keyed
    /// on it. `None` only when the call couldn't be resolved. Round-trips via ADIF.
    pub country: Option<String>,
    /// US state (ADIF `STATE`, 2-letter postal code, uppercased) — drives WAS.
    /// `None` for non-US contacts or when the report didn't carry it.
    pub state: Option<String>,
    pub band: String,
    pub freq_mhz: f64,
    /// Mode / tier label ("TempoFast" | "TempoDeep" | "FT8" | "CW" | "SSB" | "USB" | "LSB" | "FM" …).
    pub mode: String,
    /// Signal report SENT / RECEIVED, as a string (ADIF `RST_SENT`/`RST_RCVD` are
    /// type String). Holds a CW RST ("599"), a phone RS ("59"), OR a digital dB SNR
    /// ("-12") — the digital path's signed-int report is already a valid string, so
    /// this is a non-breaking generalization. Digital consumers parse the signed int
    /// back out (gated on mode), e.g. the Journey "strongest signal" stat.
    pub rst_sent: Option<String>,
    pub rst_rcvd: Option<String>,
    /// Operator's name (ADIF `NAME`) — callbook autofill / ragchew logging.
    pub name: Option<String>,
    /// QSO location / city (ADIF `QTH`).
    pub qth: Option<String>,
    /// Short, sharable remark about the contact (ADIF `COMMENT`).
    pub comment: Option<String>,
    /// Operator's own free-form, multi-line notes (ADIF `NOTES`) — rig/antenna/
    /// weather/conversation. The field ragchew operators love most.
    pub notes: Option<String>,
    /// Transmit power in watts (ADIF `TX_PWR`), if recorded.
    pub tx_power: Option<f64>,
    /// Contact START time, Unix seconds (UTC) — ADIF `QSO_DATE`/`TIME_ON`.
    pub when_unix: u64,
    /// Whether TIME_ON was actually KNOWN. `false` for imported records whose
    /// source carried no time of day: `when_unix` then holds the date at
    /// 00:00:00 UTC for ordering only, the ADIF writer emits NO fabricated
    /// TIME_ON, and the LoTW/eQSL batch builders exclude the record (both
    /// services match on time — asserting midnight parked such records as
    /// permanently unmatched, re-uploaded forever).
    pub time_known: bool,
    /// Contact END time, Unix seconds (UTC) — ADIF `QSO_DATE_OFF`/`TIME_OFF` (when the
    /// closing 73/RR73 completed). `None` for imported/legacy records with no off-time.
    pub time_off_unix: Option<u64>,
    /// Confirmed by ANY channel — LoTW, eQSL, or paper (`*_QSL_RCVD`). For
    /// general "has a confirmation" display only.
    pub confirmed: bool,
    /// **Award-eligible** confirmation: LoTW **or** paper QSL only. eQSL is NOT
    /// accepted for DXCC/WAZ/WPX/WAS, so award counting (DXCC, Challenge, …) must
    /// use this — not [`confirmed`](Self::confirmed) — or it over-counts.
    pub award_confirmed: bool,
    /// WHICH channel(s) confirmed (the per-source truth behind the two booleans).
    /// May be all-false on legacy in-memory records whose sync predates the
    /// split; the ADIF writer keeps a best-guess fallback for those.
    pub qsl_rcvd: QslRcvd,
    /// Operator-declared OUTBOUND QSL-request state (did I send a card, how, when).
    /// A *request*, NOT a confirmation — never promotes `confirmed`/`qsl_rcvd`.
    /// Round-trips via ADIF `QSL_SENT`/`QSL_SENT_VIA`/`QSLSDATE`. Default = not sent.
    pub qsl_sent: QslSent,
    /// Awards credit has been **granted** (ARRL credited it) — normalized ADIF
    /// award codes ("DXCC", "DXCC_BAND", "WAS"…), uppercased + sorted + deduped.
    /// Distinct from `award_confirmed`: a confirmation you hold vs credit you've
    /// been officially granted. From ADIF `CREDIT_GRANTED`.
    pub credit_granted: Vec<String>,
    /// Awards credit **applied/submitted** but not yet granted (ADIF
    /// `CREDIT_SUBMITTED`).
    pub credit_submitted: Vec<String>,
    /// Per-source OUTBOUND upload state (distinct from the inbound `confirmed`/
    /// `credit_*`): what WE pushed, so diagnostics can tell "never uploaded" from
    /// "uploaded, partner hasn't confirmed". Set by the LoTW/QRZ/ClubLog upload
    /// paths; round-trips via `APP_TEMPO_UL_*` ADIF app-fields. Default all-`None`.
    pub upload: UploadState,
    /// Parks/Summits On The Air context — your activation and/or the activator you
    /// hunted. Round-trips via standard ADIF (`MY_SIG`/`MY_SIG_INFO`/`SIG`/`SIG_INFO`
    /// for POTA, `MY_SOTA_REF`/`SOTA_REF` for SOTA), so exports upload cleanly to
    /// pota.app / the SOTA database. Default all-`None`.
    pub ota: Ota,
    /// ADIF `DXCC` — the NUMERIC canonical entity id. The award-grade identity
    /// (free-text COUNTRY spellings differ between cty.dat and QRZ), carried
    /// through imports so a master log keeps it.
    pub dxcc: Option<u32>,
    /// ADIF `PROP_MODE` / `SAT_NAME` — LoTW requires both for satellite award
    /// credit; stripping them at import made those QSOs uncreditable forever.
    pub prop_mode: Option<String>,
    pub sat_name: Option<String>,
    /// ADIF `OPERATOR` / `STATION_CALLSIGN` — who operated / which station
    /// logged it (multi-op and club logs depend on the distinction).
    pub operator: Option<String>,
    pub station_callsign: Option<String>,
    /// Every ADIF field this parser does NOT model, preserved verbatim
    /// (uppercased name, untouched value) and re-emitted on write — BY
    /// CONSTRUCTION (the parser CONSUMES what it models; the remainder lands
    /// here), so the preserved set can never drift from the code. Sorted for a
    /// deterministic write order.
    pub extra: Vec<(String, String)>,
}

/// Per-channel INBOUND confirmation state — which source(s) actually confirmed
/// this QSO. The derived [`QsoRecord::confirmed`]/[`QsoRecord::award_confirmed`]
/// booleans stay for cheap consumption, but THIS is the truth they derive from:
/// collapsing to two bools was lossy (the writer used to re-emit a paper-card
/// confirmation as `LOTW_QSL_RCVD`, silently rewriting the operator's QSL
/// history on every save).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QslRcvd {
    /// Paper/bureau/direct card (ADIF `QSL_RCVD`). Award-eligible.
    pub card: bool,
    /// Logbook of The World (ADIF `LOTW_QSL_RCVD`). Award-eligible.
    pub lotw: bool,
    /// eQSL.cc (ADIF `EQSL_QSL_RCVD`). NOT award-eligible for DXCC/WAZ/WPX/WAS.
    pub eqsl: bool,
    /// QRZ Logbook native confirmation (both ops have the QSO in their QRZ logs, ADIF
    /// `APP_QRZLOG_STATUS=C`). Like eQSL: it confirms the contact but is NOT award-eligible —
    /// keeping it out of `award()` is what stops a QRZ-only match inflating DXCC/WAS counts.
    pub qrz: bool,
}

impl QslRcvd {
    /// Any channel confirmed.
    pub fn any(self) -> bool {
        self.card || self.lotw || self.eqsl || self.qrz
    }

    /// Award-eligible (LoTW or paper — never eQSL or QRZ-native).
    pub fn award(self) -> bool {
        self.card || self.lotw
    }

    /// Monotonic per-source merge (confirmations only ever add).
    pub fn merge(&mut self, inc: QslRcvd) {
        self.card |= inc.card;
        self.lotw |= inc.lotw;
        self.eqsl |= inc.eqsl;
        self.qrz |= inc.qrz;
    }
}

/// How a paper/card QSL was sent (ADIF `QSL_SENT_VIA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QslVia {
    /// Bureau (ADIF `B`).
    Bureau,
    /// Direct — mailed to the operator (ADIF `D`).
    Direct,
    /// Electronic (ADIF `E`).
    Electronic,
}

impl QslVia {
    /// The single-letter ADIF `QSL_SENT_VIA` code.
    pub fn code(self) -> &'static str {
        match self {
            QslVia::Bureau => "B",
            QslVia::Direct => "D",
            QslVia::Electronic => "E",
        }
    }

    /// Parse an ADIF `QSL_SENT_VIA` code (case-insensitive). `None` for anything
    /// outside the B/D/E subset the operator can pick.
    pub fn from_code(s: &str) -> Option<QslVia> {
        match s.trim().to_ascii_uppercase().as_str() {
            "B" => Some(QslVia::Bureau),
            "D" => Some(QslVia::Direct),
            "E" => Some(QslVia::Electronic),
            _ => None,
        }
    }
}

/// Operator-declared OUTBOUND QSL-request state: whether the operator has sent a
/// QSL card/request for this contact, how, and when. This is a *request*, NOT a
/// confirmation — it is operator-declared truth that NEVER sets `confirmed` /
/// `qsl_rcvd` (a request is not a card in hand). Round-trips via the standard ADIF
/// `QSL_SENT` / `QSL_SENT_VIA` / `QSLSDATE` fields, with the same legacy-absent
/// tolerance as [`QslRcvd`] (all fields missing ⇒ default, `sent == false`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QslSent {
    /// A QSL was sent (ADIF `QSL_SENT` = `Y`). Operator-declared.
    pub sent: bool,
    /// How it was sent (ADIF `QSL_SENT_VIA`), when recorded.
    pub via: Option<QslVia>,
    /// Date sent, Unix seconds at UTC midnight (ADIF `QSLSDATE`, `YYYYMMDD`) — the
    /// field carries no time-of-day, so only the date round-trips.
    pub date_unix: Option<u64>,
}

impl QslSent {
    /// Adopt another instance's outbound QSL-sent mark when we don't already hold one. The
    /// operator declared "I sent a card" on the other instance; sharing one log, that truth
    /// must survive this instance's full-file rewrite. Monotonic — never un-sends.
    pub fn merge(&mut self, other: &QslSent) {
        if !self.sent && other.sent {
            *self = *other;
        }
    }
}

/// Parks/Summits On The Air tags on a contact: your activation (`my_*`) and/or the
/// activator you worked (hunter side). `program` is "POTA"/"SOTA"; `reference` is the
/// park/summit id (e.g. "K-1234" / "W7A/MN-001"). All-`None` = an ordinary contact.
///
/// Also carries the worked station's IOTA island-group ref — a separate award kept here
/// as a sibling on-the-air location reference (IOTA is the original "On The Air" program),
/// in its own field so a single QSO can be both a POTA park AND an island.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ota {
    pub my_program: Option<String>,
    pub my_ref: Option<String>,
    pub their_program: Option<String>,
    pub their_ref: Option<String>,
    /// IOTA island-group reference of the worked station (ADIF `IOTA`, e.g. "NA-001").
    pub iota: Option<String>,
}

/// Outbound upload outcome for one source (e.g. LoTW via TQSL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadOutcome {
    /// Dispatched (signed+sent), but not yet confirmed on file (no per-QSO ack).
    Pending,
    /// Confirmed on file at the service (e.g. echoed back in a LoTW download).
    Accepted,
    /// The service reports it was already uploaded (benign).
    Duplicate,
    /// The upload bounced (bad record / server rejection) — fix and re-send.
    Rejected,
    /// Credentials/cert/station-location rejected — re-authenticate, then re-send.
    AuthFail,
}

impl UploadOutcome {
    /// Lowercase wire/ADIF tag.
    pub fn code(self) -> &'static str {
        match self {
            UploadOutcome::Pending => "pending",
            UploadOutcome::Accepted => "accepted",
            UploadOutcome::Duplicate => "duplicate",
            UploadOutcome::Rejected => "rejected",
            UploadOutcome::AuthFail => "authfail",
        }
    }
    pub fn from_code(s: &str) -> Option<UploadOutcome> {
        Some(match s {
            "pending" => UploadOutcome::Pending,
            "accepted" => UploadOutcome::Accepted,
            "duplicate" => UploadOutcome::Duplicate,
            "rejected" => UploadOutcome::Rejected,
            "authfail" => UploadOutcome::AuthFail,
            _ => return None,
        })
    }
    /// Is this terminal "already sent" (excluded from the re-upload batch)?
    /// `Rejected`/`AuthFail` are re-sendable; the rest are not.
    pub fn is_sent(self) -> bool {
        matches!(
            self,
            UploadOutcome::Pending | UploadOutcome::Accepted | UploadOutcome::Duplicate
        )
    }
}

/// One source's last upload status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadStatus {
    pub outcome: UploadOutcome,
    pub when_unix: i64,
    /// Sanitized service/tool message (bounce reason); never a raw path/secret.
    pub detail: Option<String>,
}

/// One connector's last real outcome, read back off the persisted per-QSO stamps.
/// All-`None` = this connector has never been exercised, which is NOT the same as
/// healthy — the panel must show it as unverified rather than green.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceHealth {
    pub last_success_unix: Option<i64>,
    pub last_failure_unix: Option<i64>,
    /// Sanitized service message for the failure above (never a raw path/secret).
    pub last_failure_detail: Option<String>,
}

/// [`Logbook::upload_health`] for the four connectors that leave a per-QSO stamp.
/// HRDLog.net and Cloudlog are not here: they have no `UploadState` field, so their
/// health is session-only and lives in the orchestration layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UploadHealth {
    pub lotw: SourceHealth,
    pub eqsl: SourceHealth,
    pub qrz: SourceHealth,
    pub clublog: SourceHealth,
}

/// Per-source outbound upload state. Absent (`None`) = never attempted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UploadState {
    pub lotw: Option<UploadStatus>,
    pub eqsl: Option<UploadStatus>,
    pub qrz: Option<UploadStatus>,
    pub clublog: Option<UploadStatus>,
}

impl UploadState {
    /// Merge another copy of this record's upload state in, per source: keep whichever status
    /// is more recent (higher `when_unix`); the latest attempt is the current truth. This is
    /// what keeps two instances sharing one log from losing an upload stamp to the other's
    /// full-file rewrite — and lets the second instance see "already sent" so it doesn't
    /// re-upload the same QSO.
    pub fn merge_recent(&mut self, other: &UploadState) {
        fn pick(a: &mut Option<UploadStatus>, b: &Option<UploadStatus>) {
            if let Some(bs) = b {
                if a.as_ref().is_none_or(|as_| bs.when_unix > as_.when_unix) {
                    *a = Some(bs.clone());
                }
            }
        }
        pick(&mut self.lotw, &other.lotw);
        pick(&mut self.eqsl, &other.eqsl);
        pick(&mut self.qrz, &other.qrz);
        pick(&mut self.clublog, &other.clublog);
    }
}

/// An in-memory logbook backed by an ADIF file.
#[derive(Debug, Clone, Default)]
pub struct Logbook {
    records: Vec<QsoRecord>,
}

impl Logbook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn records(&self) -> &[QsoRecord] {
        &self.records
    }
    /// Mutable access to the records (for in-place upload-state stamping).
    pub fn records_mut(&mut self) -> &mut [QsoRecord] {
        &mut self.records
    }
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Add a record in memory.
    pub fn add(&mut self, rec: QsoRecord) {
        self.records.push(rec);
    }

    /// Replace the human-entered fields of the record at `index` (a correction —
    /// e.g. a busted call or wrong band). The sync-DERIVED state (confirmed /
    /// award_confirmed / credit / upload) is preserved from the existing record so
    /// an edit can never fabricate a confirmation; the next reconcile re-validates
    /// it against the corrected key. Returns false if `index` is out of range.
    pub fn update_record(&mut self, index: usize, mut rec: QsoRecord) -> bool {
        match self.records.get(index) {
            Some(old) => {
                // A field-edit must not wipe an operator-declared QSL-sent mark —
                // only `mark_qsl_sent` mutates it. (Kept even on a call fix: the
                // card WAS mailed; that's history, not credit.)
                rec.qsl_sent = old.qsl_sent;
                // TRIMMED comparison: an imported record can carry a padded
                // CALL, while the edit form always sends a trimmed one — an
                // untrimmed compare would read every ordinary edit of such a
                // record as a callsign correction and strip its confirmations.
                let call_changed = !rec.call.trim().eq_ignore_ascii_case(old.call.trim());
                if !call_changed {
                    // Ordinary edit (band/grid/name/…): derived state rides along.
                    rec.confirmed = old.confirmed;
                    rec.award_confirmed = old.award_confirmed;
                    rec.qsl_rcvd = old.qsl_rcvd;
                    rec.credit_granted = old.credit_granted.clone();
                    rec.credit_submitted = old.credit_submitted.clone();
                    rec.upload = old.upload.clone();
                } else {
                    // CALLSIGN correction — operator ruling (2026-07-30): the
                    // services hold the OLD call, so clearing the upload stamps
                    // re-queues the corrected QSO to every one of them; and a
                    // confirmation (or granted credit) matched against the busted
                    // call is credit this QSO never earned — stripped, never
                    // silently carried over. (LoTW itself still holds the
                    // old-call record; nothing we send can retract it.)
                    rec.confirmed = false;
                    rec.award_confirmed = false;
                    rec.qsl_rcvd = QslRcvd::default();
                    rec.credit_granted = Vec::new();
                    rec.credit_submitted = Vec::new();
                    rec.upload = UploadState::default();
                    // The RESURRECTION guard: an imported LOTW_QSL_SENT=Y rides
                    // in `extra`, and the parser's fallback would re-derive
                    // upload.lotw = Accepted from it on the next load — quietly
                    // undoing the clear above and excluding the corrected QSO
                    // from the LoTW batch forever. It described the BUSTED
                    // call's upload; it goes with the stamps.
                    rec.extra.retain(|(k, _)| k != "LOTW_QSL_SENT");
                    // Call-derived identity re-derives from the NEW call: the
                    // busted call's entity/state must not ride along. (country
                    // refills from the resolver on the save path; dxcc/state
                    // stay empty until a lookup supplies them.)
                    rec.dxcc = None;
                    rec.country = None;
                    rec.state = None;
                }
                // Never clobber a known country/state to None on an ORDINARY
                // edit (the form doesn't carry them) — but on a call correction
                // the old values describe the busted call and stay cleared.
                if !call_changed {
                    if rec.country.is_none() {
                        rec.country = old.country.clone();
                    }
                    if rec.state.is_none() {
                        rec.state = old.state.clone();
                    }
                }
                // The edit form doesn't carry the contact end time — preserve the
                // stored TIME_OFF rather than wiping it on a name/grid edit.
                if rec.time_off_unix.is_none() {
                    rec.time_off_unix = old.time_off_unix;
                }
                // Preserve the stored POTA/SOTA park refs when the edit leaves them empty (a
                // busted-call/RST fix must not silently drop the park from the record + ADIF).
                let incoming_ota_empty = rec.ota.my_program.is_none()
                    && rec.ota.my_ref.is_none()
                    && rec.ota.their_program.is_none()
                    && rec.ota.their_ref.is_none();
                if incoming_ota_empty {
                    rec.ota = old.ota.clone();
                }
                // The edit form carries none of the import-carried identity —
                // an edit must never bleach it off the record. (dxcc is
                // call-derived: preserved on an ordinary edit only.)
                if !call_changed && rec.dxcc.is_none() {
                    rec.dxcc = old.dxcc;
                }
                if rec.prop_mode.is_none() {
                    rec.prop_mode = old.prop_mode.clone();
                }
                if rec.sat_name.is_none() {
                    rec.sat_name = old.sat_name.clone();
                }
                if rec.operator.is_none() {
                    rec.operator = old.operator.clone();
                }
                if rec.station_callsign.is_none() {
                    rec.station_callsign = old.station_callsign.clone();
                }
                if rec.extra.is_empty() {
                    rec.extra = old.extra.clone();
                    if call_changed {
                        // The preservation must not undo the resurrection guard
                        // above: the busted call's LOTW_QSL_SENT goes, whether
                        // the extra set came from the payload or from `old`.
                        rec.extra.retain(|(k, _)| k != "LOTW_QSL_SENT");
                    }
                }
                // An edit that did not touch the TIME OF DAY must not fabricate
                // time-knowledge onto an imported, time-less record — keyed on
                // the time-of-day, not the whole timestamp, so a DATE fix on a
                // date-only import stays honestly time-unknown.
                if rec.when_unix % 86_400 == old.when_unix % 86_400 {
                    rec.time_known = old.time_known;
                }
                self.records[index] = rec;
                true
            }
            None => false,
        }
    }

    /// Mark the record at `index` as QSL-sent — operator-declared truth that you
    /// sent a card/request `via` (bureau/direct/electronic) on `date_unix`. Only
    /// ever ADDS a request; it never touches `confirmed`/`qsl_rcvd` (a request is
    /// not a confirmation). Returns false if `index` is out of range. Pure — call
    /// [`save`](Self::save) to persist.
    pub fn mark_qsl_sent(&mut self, index: usize, via: QslVia, date_unix: u64) -> bool {
        match self.records.get_mut(index) {
            Some(rec) => {
                rec.qsl_sent = QslSent {
                    sent: true,
                    via: Some(via),
                    date_unix: Some(date_unix),
                };
                true
            }
            None => false,
        }
    }

    /// Record that a PAPER card for `index` did or did not arrive (ADIF `QSL_RCVD`).
    ///
    /// The operator is the only possible authority here: LoTW, eQSL and QRZ report their own
    /// confirmations and Nexus syncs those, but nothing on the internet knows a card landed in
    /// somebody's letterbox. Until this existed a paper QSL could not be entered at all (#152),
    /// which mattered more than it sounds: `QslRcvd::award` counts card OR LoTW, so a card that
    /// makes a DXCC entity countable was unrecordable and the award view stayed wrong.
    ///
    /// Unlike [`QslRcvd::merge`], which is monotonic because a service only ever ADDS what it
    /// has matched, this can also clear — an operator who ticks the wrong row must be able to
    /// untick it. A later service sync cannot silently undo the correction either: merge ORs
    /// per source, and no service reports the card field.
    pub fn mark_qsl_card(&mut self, index: usize, received: bool) -> bool {
        match self.records.get_mut(index) {
            Some(rec) => {
                rec.qsl_rcvd.card = received;
                true
            }
            None => false,
        }
    }

    /// Remove the record at `index` (a mis-logged contact). Returns false if out of
    /// range. NOTE: this shifts the indices of all later records — callers that hold
    /// indices must reload after a delete.
    pub fn delete(&mut self, index: usize) -> bool {
        if index < self.records.len() {
            self.records.remove(index);
            true
        } else {
            false
        }
    }

    /// Remove EVERY record (the operator-confirmed "purge log" action). Returns the
    /// number removed. Persist with [`save`](Self::save) to truncate the ADIF file
    /// to an empty (header-only) log.
    pub fn clear(&mut self) -> usize {
        let n = self.records.len();
        self.records.clear();
        n
    }

    /// Merge external ADIF `text` into the log: ADD the contacts it has that we
    /// lack (deduped by call+band+mode+exact time) and monotonically UPGRADE the
    /// ones we already hold from the row that restates them. Returns the
    /// newly-added records (so the caller can persist exactly those), the count
    /// deduped, and the count of held records the import actually changed.
    ///
    /// # Why a dupe is not a discard
    ///
    /// "Already present" answers *don't log this contact twice* — it never meant
    /// *throw away what this row knows about the one we have*. It did: a dupe was
    /// dropped whole, taking its `QSL_RCVD`, its `CREDIT_GRANTED` and the
    /// STATE/COUNTRY detail with it. That is exactly the shape a LoTW download
    /// has (it restates contacts you already logged, and the confirmation rides
    /// on those rows), so importing one over a master log left an operator's
    /// award-eligible confirmations at 3% of his QSOs — worked counts perfect,
    /// confirmed counts empty, every award wrong at once. The upgrade uses the
    /// same [`crate::reconcile`] merge the sync path uses, which only ever ADDS:
    /// a re-import can never un-confirm or un-credit anything.
    pub fn import_adif(&mut self, text: &str) -> (Vec<QsoRecord>, usize, usize) {
        // Held records by dedup identity → index, so a dupe can be upgraded in
        // place and not merely counted. First index wins (log order), matching
        // the oldest-first consume order `reconcile` uses for repeated keys.
        let mut held: std::collections::HashMap<DedupKey, usize> =
            std::collections::HashMap::with_capacity(self.records.len());
        for (i, r) in self.records.iter().enumerate() {
            held.entry(dedup_key(r)).or_insert(i);
        }
        let mut added = Vec::new();
        let mut skipped = 0usize;
        let mut merged = 0usize;
        // Counters the import doesn't report; `apply_match` tallies into it.
        let mut tally = crate::reconcile::ReconcileSummary::default();
        for rec in parse_adif(text) {
            // LEGACY-MODE BRIDGE: rows this file imported under an earlier build
            // read MODE only, so a WSJT-X "MODE=MFSK + SUBMODE=FT4" row is stored
            // as "MFSK" — while the same source row now parses as "FT4". Without
            // this probe, re-importing an already-imported file would double-log
            // every FT4/Q65/FST4/FST4W row. The stored MFSK copy stays as-is;
            // the incoming promoted twin counts as the duplicate it is.
            let legacy_twin = promoted_submode(&rec.mode).is_some().then(|| {
                let mut k = dedup_key(&rec);
                k.2 = "MFSK".to_string();
                k
            });
            let key = dedup_key(&rec);
            let hit = legacy_twin
                .and_then(|t| held.get(&t).copied())
                .or_else(|| held.get(&key).copied());
            match hit {
                Some(i) => {
                    skipped += 1;
                    // Compare the whole record, not the summary counters: the
                    // enrichment a confirmation row carries (STATE, COUNTRY) is a
                    // real change the tallies don't name, and the caller has to
                    // persist it.
                    let before = self.records[i].clone();
                    crate::reconcile::apply_match(&mut self.records[i], &rec, &mut tally);
                    if self.records[i] != before {
                        merged += 1;
                    }
                }
                None => {
                    held.insert(key, self.records.len());
                    added.push(rec.clone());
                    self.records.push(rec);
                }
            }
        }
        (added, skipped, merged)
    }

    /// Reconcile the on-disk copy of this log back into memory — the two-instance-safe recovery.
    /// Unlike [`import_adif`] (additive only: it ADDS unseen records but never touches an
    /// existing one), this both ADDS the records disk has that memory lacks (another instance's
    /// appends) AND monotonically UPGRADES shared records' confirmation / credit / upload /
    /// QSL-sent state from disk. That makes the shared log a monotonic union: before this
    /// instance rewrites the whole file, it has folded in whatever the other instance wrote, so
    /// a confirmation or upload stamp is never clobbered. Call before any full-file `save`.
    ///
    /// Matched on EXACT identity (call / band / mode-class / the exact contact second),
    /// not on the report matcher's UTC-day key: these rows are our own `save()` output,
    /// so both sides carry the same timestamp, and the day key mis-paired two contacts
    /// with one station inside a day — see [`crate::reconcile::merge_own_disk`].
    pub fn reconcile_disk(&mut self, text: &str) {
        let incoming = parse_adif(text);
        crate::reconcile::merge_own_disk(&mut self.records, incoming);
    }

    /// Stamp park/summit references from an external OTA log (pota.app hunter or
    /// activator ADIF) onto MATCHING existing QSOs — the safe half of OTA pull-back.
    /// Never creates records (reviewed adds are a separate feature, per the
    /// anti-abuse rule) and never overwrites a ref already present. A row matches a
    /// local QSO on callsign + band + same UTC time within ±30 min (or the same UTC
    /// day when either side lacks a real time — some exports carry date only).
    /// Returns `(stamped, already_had, unmatched)`.
    pub fn stamp_ota_refs(&mut self, text: &str) -> (usize, usize, usize) {
        const WINDOW_SECS: u64 = 30 * 60;
        let mut stamped = 0usize;
        let mut already = 0usize;
        let mut unmatched = 0usize;
        for row in parse_adif(text) {
            let has_refs = row.ota.their_ref.is_some() || row.ota.my_ref.is_some();
            if !has_refs {
                continue; // nothing to stamp from this row
            }
            let row_day = row.when_unix / 86_400;
            let hit = self.records.iter_mut().find(|q| {
                if !q.call.eq_ignore_ascii_case(&row.call) {
                    return false;
                }
                if !q.band.eq_ignore_ascii_case(&row.band) {
                    return false;
                }
                // Date-only exports carry no time of day — `time_known` is the
                // explicit truth now (a REAL 00:00:00 QSO no longer reads as
                // date-only, which the old midnight-modulo heuristic got wrong).
                let row_timed = row.time_known;
                let q_timed = q.time_known;
                if row_timed && q_timed {
                    q.when_unix.abs_diff(row.when_unix) <= WINDOW_SECS
                } else {
                    q.when_unix / 86_400 == row_day
                }
            });
            match hit {
                Some(q) => {
                    let mut did = false;
                    if q.ota.their_ref.is_none() && row.ota.their_ref.is_some() {
                        q.ota.their_program = row.ota.their_program.clone();
                        q.ota.their_ref = row.ota.their_ref.clone();
                        did = true;
                    }
                    if q.ota.my_ref.is_none() && row.ota.my_ref.is_some() {
                        q.ota.my_program = row.ota.my_program.clone();
                        q.ota.my_ref = row.ota.my_ref.clone();
                        did = true;
                    }
                    if did {
                        stamped += 1;
                    } else {
                        already += 1;
                    }
                }
                None => unmatched += 1,
            }
        }
        (stamped, already, unmatched)
    }

    /// True if `call` appears anywhere in the log (worked on any band).
    /// The set of every worked callsign (uppercased), built in one O(n) pass. For a caller
    /// that tests MANY stations against the log at once — the roster snapshot — this turns an
    /// O(roster × log) sweep of [`worked_before`](Self::worked_before) into O(log + roster):
    /// build the set once, then O(1) membership per station. Rebuilt on each call from the
    /// live records, so there is no cached index to desync with edits/deletes/imports. This is
    /// the fix for the waterfall stall: `snapshot()` ran the multiplicative sweep under the
    /// engine mutex that the waterfall's spectrum fetch also needs.
    pub fn worked_call_set(&self) -> std::collections::HashSet<String> {
        note_log_sweep();
        self.records
            .iter()
            .map(|r| r.call.to_ascii_uppercase())
            .collect()
    }

    /// The worked-before index at BAND granularity, one sweep: `(CALL, BAND)` pairs, both
    /// uppercased — beside [`Self::worked_call_set`] for the same single-sweep reason (a
    /// per-row `worked_before_band` under the engine mutex is the waterfall stall).
    ///
    /// `fold_mode`: when set, the pair becomes `(CALL, BAND·MODE)` — WSJT-X's optional
    /// "highlight by mode" scope (its `HighlightByMode`, default off), where working a station
    /// on 40m FT8 does NOT mark them B4 for 40m phone. The raw uppercased ADIF mode is the
    /// key, exactly as WSJT-X compares its own mode strings.
    pub fn worked_band_set(&self, fold_mode: bool) -> std::collections::HashSet<(String, String)> {
        note_log_sweep();
        self.records
            .iter()
            .map(|r| {
                let band = if fold_mode {
                    format!(
                        "{}\u{1}{}",
                        r.band.to_ascii_uppercase(),
                        r.mode.to_ascii_uppercase()
                    )
                } else {
                    r.band.to_ascii_uppercase()
                };
                (r.call.to_ascii_uppercase(), band)
            })
            .collect()
    }

    /// The lookup key [`Self::worked_band_set`] stores for (`band`, `mode`) under `fold_mode`.
    pub fn band_key(band: &str, mode: &str, fold_mode: bool) -> String {
        if fold_mode {
            format!(
                "{}\u{1}{}",
                band.to_ascii_uppercase(),
                mode.to_ascii_uppercase()
            )
        } else {
            band.to_ascii_uppercase()
        }
    }

    pub fn worked_before(&self, call: &str) -> bool {
        note_log_sweep();
        self.records
            .iter()
            .any(|r| r.call.eq_ignore_ascii_case(call))
    }

    /// True if `call` was worked on `band` (band-specific dupe check).
    pub fn worked_before_band(&self, call: &str, band: &str) -> bool {
        note_log_sweep();
        self.records
            .iter()
            .any(|r| r.call.eq_ignore_ascii_case(call) && r.band.eq_ignore_ascii_case(band))
    }

    /// Load from an ADIF file. Missing/unreadable file → empty log.
    ///
    /// # Data-loss guard
    ///
    /// [`parse_adif`] drops any record it cannot assemble — a block with no `CALL`, and, more
    /// dangerously, a run of records after a malformed `<NAME:len>` length prefix desyncs the
    /// scan. Every [`save`](Self::save) then rewrites the WHOLE file from the parsed records, so
    /// a lossy load followed by any save **permanently truncates the file on disk**. The parse
    /// bug that drops the records is fixed at the source, but a parser can never promise it
    /// understands every third-party ADIF dialect, so this is the backstop: the FIRST time this
    /// build loads a non-empty log, the raw bytes are copied verbatim to a sibling `.bak` that
    /// is never overwritten. Whatever the parser did, the original survives.
    ///
    /// # Why this reads BYTES, not a String (the Greek-Windows report, 2026-08)
    ///
    /// `read_to_string` fails on a file that is not valid UTF-8 — and a `log.adi` written by a
    /// Greek/German/French Windows logger holds CP1253/CP1252 bytes in `NAME`, `QTH` and
    /// `COMMENT` routinely. `unwrap_or_default()` turned that `Err` into `""`: the logbook
    /// loaded as EMPTY, [`backup_once`](Self::backup_once) skipped (an empty body has nothing
    /// to lose), and the next [`save`](Self::save) rewrote the file from zero records. The
    /// operator's whole log, gone, with no copy — the single worst failure this module can
    /// have. So: read bytes, convert with [`String::from_utf8_lossy`], and never fail a load
    /// over an encoding.
    ///
    /// The conversion is lossy on purpose and it is bounded: the ADIF structure is ASCII, so
    /// every RECORD survives. Only the offending field's text degrades — `<NAME:7>` counts
    /// BYTES, and each bad byte widens to a 3-byte `U+FFFD`, so the length prefix now
    /// under-runs the value and the field reads back short or empty. The parser resyncs at the
    /// next `<` (which can never appear inside a UTF-8 multi-byte sequence), so the desync
    /// cannot spill into the following record. Losing the spelling of a name is a paper cut;
    /// losing the QSO is not. The anchor `.bak` keeps the original bytes either way, so nothing
    /// is destroyed.
    pub fn load(path: &Path) -> Self {
        let bytes = std::fs::read(path).unwrap_or_default();
        Self::backup_once(path, &bytes);
        Self {
            records: parse_adif(&String::from_utf8_lossy(&bytes)),
        }
    }

    /// Preserve the raw log bytes verbatim, exactly once, before any save can rewrite the file.
    /// Never overwrites an existing `.bak` (the earliest copy is the most complete — saves only
    /// ever shrink the file), and never fails the load: a backup that can't be written is logged
    /// and ignored, because refusing to open the logbook would be a worse failure than a missing
    /// safety copy.
    ///
    /// Takes BYTES, and every test here is on bytes: gating on a decoded `&str` is what made a
    /// non-UTF-8 log look empty and skip its own backup (see [`load`](Self::load)).
    fn backup_once(path: &Path, bytes: &[u8]) {
        // Protect only a file that actually carries records. The body is whatever follows
        // `<EOH>` (the whole file when there is no header) — the same split `parse_adif` uses,
        // so "has something to lose" here means exactly "the parser has something to read". A
        // missing file (read → empty) and a header-only log both have an empty body and skip.
        // "Empty" is ASCII whitespace only: a byte that is not valid UTF-8 is not whitespace,
        // it is content, and it is the case that must NOT skip.
        let body = body_after_eoh(bytes);
        if body.iter().all(|b| b.is_ascii_whitespace()) {
            return;
        }
        let bak = path.with_extension("adi.bak");
        if bak.exists() {
            return; // earliest = most complete; do not clobber with a later (possibly truncated) file
        }
        if let Err(e) = std::fs::write(&bak, bytes) {
            eprintln!("tempo: could not back up logbook to {}: {e}", bak.display());
        }
    }

    /// Take a dated snapshot of the log **as it is on disk right now**, into a `backups/`
    /// folder beside it, and rotate the folder back inside its bounds. Called from
    /// [`save`](Self::save) with the byte length the save is ABOUT to write.
    ///
    /// Everything here is best-effort: every failure path returns quietly, because a backup
    /// that cannot be written must never stop the operator saving their log.
    ///
    /// # When a snapshot is taken
    ///
    /// - **Unconditionally, before a save that SHRINKS the file.** This is the valuable one:
    ///   shrinking is precisely the failure that lost the operator's oldest QSOs (see
    ///   `a_lossy_load_then_save_cannot_destroy_the_original`), and a calendar rule would sail
    ///   straight past it — the truncating save usually lands on a day that already has its
    ///   snapshot.
    /// - **Otherwise at most once per UTC calendar day**, at the first qualifying save of that
    ///   day, and only when the bytes actually differ from the newest snapshot we hold.
    ///
    /// ## Why BYTES are the shrink signal, not record count
    ///
    /// Record count would be the more direct statement of "QSOs disappeared", but it is not
    /// honestly available here: we hold the new count in memory, and the OLD count could only
    /// come from re-parsing the file on disk — on **every save**, of a log that reaches tens of
    /// thousands of QSOs, and through the very parser whose lossiness is the hazard we are
    /// insuring against. Byte length comes from one `stat`, costs nothing, and is independent
    /// of the parser. It is also strictly wider: every dropped record shrinks the file (records
    /// are never empty), and it additionally catches field-level loss that leaves the count
    /// alone. The cost is the occasional false positive — a benign edit that shortens a
    /// COMMENT — which spends one snapshot out of a bounded ring.
    ///
    /// # The three bounds
    ///
    /// `keep` most recent snapshots, a `total_cap` byte ceiling over the folder, and the
    /// one-per-day rule that limits how fast the ring can turn over. Oldest go first. The
    /// ANCHOR is not in this folder at all — it is `log.adi.bak`, beside the log — so "the
    /// anchor is never eligible for rotation" holds by construction rather than by a check
    /// someone could delete.
    fn snapshot_before_save(path: &Path, new_len: u64, now_unix: u64, keep: usize, total_cap: u64) {
        // No file yet (first ever save) or an empty one: nothing to preserve.
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        let cur_len = meta.len();
        if cur_len == 0 {
            return;
        }
        let shrinking = new_len < cur_len;

        let dir = match path.parent() {
            Some(p) => p.join("backups"),
            None => return,
        };
        // Snapshot names are prefixed with the LOG's own stem, so a folder holding two logs
        // keeps two independent rings — and so rotation can never reach a file it did not
        // write.
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "log".to_string());
        let (y, mo, d, h, mi, s) = datetime_utc(now_unix);
        let day = format!("{y:04}{mo:02}{d:02}");

        let mut snaps = Self::snapshot_names(&dir, &stem);
        if !shrinking {
            // One per calendar day…
            let today = format!("{stem}-{day}-");
            if snaps.iter().any(|n| n.starts_with(&today)) {
                return;
            }
            // …and only if the log has actually changed since the newest one we hold.
            // Length first (a `stat` each); only equal lengths pay for a byte compare, and
            // this whole branch runs at most once a day.
            if let Some(newest) = snaps.last() {
                if same_file_contents(path, &dir.join(newest)) {
                    return;
                }
            }
        }

        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let name = if shrinking {
            // A distinct suffix so the operator can see WHICH copy was taken because the log
            // was about to get smaller — that is the one they will want.
            format!("{stem}-{day}-{h:02}{mi:02}{s:02}-shrink.adi")
        } else {
            format!("{stem}-{day}-{h:02}{mi:02}{s:02}.adi")
        };
        if std::fs::copy(path, dir.join(&name)).is_err() {
            return;
        }
        snaps.push(name);
        Self::rotate_snapshots(&dir, snaps, keep, total_cap);
    }

    /// The snapshot file names in `dir` for the log named `stem`, oldest first. The name
    /// carries `YYYYMMDD-HHMMSS`, so lexicographic order IS chronological order — deliberately
    /// not mtime, which a copy tool or a restore can rewrite.
    fn snapshot_names(dir: &Path, stem: &str) -> Vec<String> {
        let prefix = format!("{stem}-");
        let mut names: Vec<String> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with(&prefix) && n.ends_with(".adi"))
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        names
    }

    /// Drop the oldest snapshots until the folder is inside BOTH bounds. Best-effort.
    fn rotate_snapshots(dir: &Path, snaps: Vec<String>, keep: usize, total_cap: u64) {
        let mut live: Vec<(String, u64)> = snaps
            .into_iter()
            .map(|n| {
                let len = std::fs::metadata(dir.join(&n))
                    .map(|m| m.len())
                    .unwrap_or(0);
                (n, len)
            })
            .collect();
        let drop_oldest = |live: &mut Vec<(String, u64)>| {
            let (name, _) = live.remove(0);
            let _ = std::fs::remove_file(dir.join(name));
        };
        while live.len() > keep {
            drop_oldest(&mut live);
        }
        // The newest snapshot is never dropped, even if it ALONE exceeds the ceiling:
        // deleting the copy we just took to satisfy a disk-space rule would be strictly
        // worse than being over it, and the operator would have no snapshot at all.
        while live.len() > 1 && live.iter().map(|(_, n)| n).sum::<u64>() > total_cap {
            drop_oldest(&mut live);
        }
    }

    /// Append one record to the ADIF file (creating it with a header if new).
    /// Keeps the in-memory copy in sync — call after [`Logbook::add`].
    pub fn append(path: &Path, rec: &QsoRecord) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let new = !path.exists();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        if new {
            f.write_all(adif_header().as_bytes())?;
        }
        f.write_all(adif_record(rec).as_bytes())?;
        Ok(())
    }

    /// Rewrite the entire ADIF file from the in-memory records (write-tmp +
    /// rename, so a crash mid-write can't truncate the log). Needed after a
    /// [`merge_report`](Self::merge_report), which mutates existing records (unlike
    /// the append-only [`append`](Self::append)).
    /// Returns the persisted file's (mtime, byte length) — the recovery-gate
    /// fingerprint. Statted on the TMP, before the rename: rename preserves the
    /// mtime, and statting the final path AFTER the rename races another
    /// instance's own rename (recording THEIR stamp as ours would make the gate
    /// skip their write). The length rides along because mtime alone is only as
    /// fine as the filesystem's clock — 2 s on FAT/SMB shares, where a shared
    /// NAS log is a supported deployment — and a same-tick sibling write would
    /// otherwise be invisible.
    pub fn save(&self, path: &Path) -> std::io::Result<Option<(std::time::SystemTime, u64)>> {
        self.save_at(path, now_unix(), BACKUP_KEEP, BACKUP_TOTAL_BYTES)
    }

    /// [`save`](Self::save) with the clock and the backup bounds injected. The snapshot rules
    /// are CALENDAR-day rules over a bounded ring, and a test can neither wait a day nor write
    /// 64 MiB of fixtures to watch the ceiling bite.
    fn save_at(
        &self,
        path: &Path,
        now_unix: u64,
        keep: usize,
        total_cap: u64,
    ) -> std::io::Result<Option<(std::time::SystemTime, u64)>> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = self.adif();
        // Snapshot the file we are about to replace, BEFORE the rename publishes the new one.
        // This is the only trigger — load takes no snapshot, so launch stays free of it (see
        // the module header). Best-effort by construction: it returns `()`, so nothing here can
        // fail a save.
        Self::snapshot_before_save(path, body.len() as u64, now_unix, keep, total_cap);
        // Per-PROCESS tmp name: two instances sharing one log.adi can each be mid-save at the
        // same instant; a fixed "log.adi.tmp" would let them interleave writes into one tmp and
        // publish a corrupted file on rename. `log.adi.<pid>.tmp` gives each its own scratch, and
        // the rename onto the final path stays atomic (last writer wins the whole file, intact).
        let tmp = path.with_extension(format!("adi.{}.tmp", std::process::id()));
        std::fs::write(&tmp, &body)?;
        let stamp = std::fs::metadata(&tmp)
            .ok()
            .and_then(|m| m.modified().ok().map(|t| (t, m.len())));
        std::fs::rename(&tmp, path)?;
        Ok(stamp)
    }

    /// Merge a confirmation/credit report (ADIF — e.g. a LoTW export) into the
    /// log: monotonically upgrade matched QSOs' confirmation + credit state, and
    /// report confirmations that match no logged QSO. The fix for "re-importing a
    /// report drops new confirmations on already-logged QSOs". Pure merge — call
    /// [`save`](Self::save) to persist.
    pub fn merge_report(&mut self, text: &str) -> crate::reconcile::ReconcileSummary {
        let incoming = parse_adif(text);
        crate::reconcile::reconcile(&mut self.records, &incoming)
    }

    /// Two-way merge of a DOWNLOADED logbook (a QRZ Logbook FETCH — the operator's own
    /// book pulled back down). Unlike [`merge_report`] (confirmations only, unmatched
    /// rows become orphans), this ADDS the QSOs the download has that the local log
    /// lacks AND upgrades confirmations on the ones already present — in a single
    /// consume-once pass keyed at reconcile (mode-class) granularity, so a mode-spelling
    /// difference can't double-log the same contact. Returns `(added_records, summary)`;
    /// call [`save`](Self::save) to persist.
    pub fn merge_downloaded(
        &mut self,
        text: &str,
    ) -> (Vec<QsoRecord>, crate::reconcile::ReconcileSummary) {
        let incoming = parse_adif(text);
        crate::reconcile::merge_and_add(&mut self.records, incoming)
    }

    /// Merge a LoTW **own-QSO** report (`qso_qsl=no` ADIF — your records LoTW holds
    /// but the partner hasn't matched). Promotes matched QSOs' LoTW upload state to
    /// `Accepted` (your side is on file → "waiting on partner"). Returns the count
    /// newly promoted. Pure merge — call [`save`](Self::save) to persist.
    pub fn merge_own_echo(&mut self, text: &str, when_unix: i64) -> usize {
        let own = parse_adif(text);
        crate::reconcile::promote_own_echo(&mut self.records, &own, when_unix)
    }

    /// Index of the NEWEST logged QSO matching `pushed`'s key (call/band/mode-class/
    /// UTC-day) — the just-logged QSO in the auto-push-at-log-time flow. `None` if no
    /// match (e.g. the QSO isn't in this log).
    fn newest_match_index(&self, pushed: &QsoRecord) -> Option<usize> {
        let mc = crate::reconcile::mode_class(&pushed.mode);
        let day = pushed.when_unix / 86_400;
        self.records
            .iter()
            .enumerate()
            .rev()
            .find(|(_, r)| {
                r.call.eq_ignore_ascii_case(&pushed.call)
                    && r.band.eq_ignore_ascii_case(&pushed.band)
                    && crate::reconcile::mode_class(&r.mode) == mc
                    && r.when_unix / 86_400 == day
            })
            .map(|(i, _)| i)
    }

    /// Stamp a QRZ Logbook push outcome onto the newest matching QSO (the one just
    /// pushed). Returns whether a record was stamped. Pure — call `save` to persist.
    pub fn stamp_qrz_upload(&mut self, pushed: &QsoRecord, status: UploadStatus) -> bool {
        match self.newest_match_index(pushed) {
            Some(i) => {
                self.records[i].upload.qrz = Some(status);
                true
            }
            None => false,
        }
    }

    /// Stamp a ClubLog realtime push outcome onto the newest matching QSO. Returns
    /// whether a record was stamped. Pure — call `save` to persist.
    pub fn stamp_clublog_upload(&mut self, pushed: &QsoRecord, status: UploadStatus) -> bool {
        match self.newest_match_index(pushed) {
            Some(i) => {
                self.records[i].upload.clublog = Some(status);
                true
            }
            None => false,
        }
    }

    /// Stamp an eQSL ADIF-upload outcome onto the newest matching QSO. Returns
    /// whether a record was stamped. Pure — call `save` to persist.
    pub fn stamp_eqsl_upload(&mut self, pushed: &QsoRecord, status: UploadStatus) -> bool {
        match self.newest_match_index(pushed) {
            Some(i) => {
                self.records[i].upload.eqsl = Some(status);
                true
            }
            None => false,
        }
    }

    /// What actually happened, per connector, the last time Nexus talked to it.
    ///
    /// The Settings ▸ Connections panel used to answer a different question than the one
    /// it was built for: its dot came from a keychain read, so a revoked ClubLog password
    /// or a rotated QRZ key stayed green forever — the secret was still there. This is the
    /// answer it should always have been reading, and it costs nothing: the per-QSO
    /// `APP_TEMPO_UL_*` stamps are already written on every push and already round-trip
    /// through `log.adi`, so the history SURVIVES A RESTART and an upgrading operator sees
    /// real history immediately rather than a blank panel.
    ///
    /// One pass, borrowed — never clone the record vector for this; the panel polls it
    /// every 5 s under the engine lock.
    pub fn upload_health(&self) -> UploadHealth {
        let mut h = UploadHealth::default();
        for r in &self.records {
            for (src, status) in [
                (&mut h.lotw, &r.upload.lotw),
                (&mut h.eqsl, &r.upload.eqsl),
                (&mut h.qrz, &r.upload.qrz),
                (&mut h.clublog, &r.upload.clublog),
            ] {
                // `when_unix == 0` is not a date. The ADIF reader synthesises an
                // `Accepted` stamp for any imported record carrying `LOTW_QSL_SENT=Y`,
                // with no time to give it — counting those would tell every operator with
                // an imported legacy log that their last LoTW upload was 1 Jan 1970.
                let Some(s) = status.as_ref().filter(|s| s.when_unix > 0) else {
                    continue;
                };
                if s.outcome.is_sent() {
                    // `Duplicate` counts as a success on purpose: the service telling us
                    // it already has the QSO proves both the credentials and the record.
                    if src.last_success_unix.is_none_or(|w| s.when_unix > w) {
                        src.last_success_unix = Some(s.when_unix);
                    }
                } else if src.last_failure_unix.is_none_or(|w| s.when_unix > w) {
                    src.last_failure_unix = Some(s.when_unix);
                    src.last_failure_detail = s.detail.clone();
                }
            }
        }
        h
    }

    /// UTC date (`YYYY-MM-DD`) of the oldest QSO whose LoTW upload is awaiting the
    /// echo (`Pending`) — the lower bound for an own-QSO (`qso_qsl=no`) pull so a
    /// sync never scans the whole log. `None` when nothing is in flight (the caller
    /// then skips the own-pull entirely).
    pub fn oldest_pending_lotw_date(&self) -> Option<String> {
        self.records
            .iter()
            .filter(|r| {
                matches!(
                    r.upload.lotw.as_ref().map(|s| s.outcome),
                    Some(UploadOutcome::Pending)
                )
            })
            .map(|r| r.when_unix)
            .min()
            .map(|unix| {
                let (y, m, d, ..) = datetime_utc(unix);
                format!("{y:04}-{m:02}-{d:02}")
            })
    }

    /// The whole logbook as ADIF text (header + records).
    pub fn adif(&self) -> String {
        self.adif_in_range(None, None)
    }

    /// The logbook as ADIF, restricted to QSOs whose start time falls in
    /// `[from_unix, to_unix]` (inclusive; either bound absent = unbounded, both
    /// absent = the whole log, byte-identical to [`Self::adif`]). The date-range
    /// export (#98): an operator uploading "just this weekend's activation" was
    /// hand-editing the full file.
    pub fn adif_in_range(&self, from_unix: Option<u64>, to_unix: Option<u64>) -> String {
        let mut s = adif_header();
        for r in self.records_in_range(from_unix, to_unix) {
            s.push_str(&adif_record(r));
        }
        s
    }

    /// Records whose start time falls in `[from, to]` (inclusive; absent = unbounded).
    fn records_in_range(
        &self,
        from: Option<u64>,
        to: Option<u64>,
    ) -> impl Iterator<Item = &QsoRecord> {
        self.records.iter().filter(move |r| {
            from.is_none_or(|f| r.when_unix >= f) && to.is_none_or(|t| r.when_unix <= t)
        })
    }

    /// Every distinct operator in the log, uppercased and sorted (#25).
    ///
    /// POTA and Field Day both require each operator to submit their OWN log, and until the
    /// operator was stamped on the record there was nothing to split on — the choice was losing
    /// the second op's credit or hand-editing the exported ADIF. This is the list a per-operator
    /// export offers.
    ///
    /// Contacts with no operator are NOT represented here. That is the single-op case (the stamp
    /// is deliberately absent rather than defaulted to the station call), and inventing a bucket
    /// named after the station would claim someone said something they never did. Callers that
    /// need those records have the combined export, which is every record either way.
    pub fn operators(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .records
            .iter()
            .filter_map(|r| r.operator.as_deref())
            .map(|o| o.trim().to_ascii_uppercase())
            .filter(|o| !o.is_empty())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// The log as ADIF, containing ONLY the contacts `operator` made.
    ///
    /// Case- and whitespace-insensitive, because the operator is typed by a human mid-activation
    /// and `w1abc` and `W1ABC ` are the same person. A caller asking for an operator with no
    /// contacts gets a valid, EMPTY ADIF file (header and nothing else) rather than an error —
    /// an empty log is a real answer, and an export that silently produced the whole log instead
    /// would upload one operator's contacts under another's name.
    pub fn adif_for_operator(&self, operator: &str) -> String {
        let want = operator.trim().to_ascii_uppercase();
        let mut s = adif_header();
        for r in &self.records {
            let is_theirs = r
                .operator
                .as_deref()
                .map(|o| o.trim().to_ascii_uppercase() == want)
                .unwrap_or(false);
            if is_theirs {
                s.push_str(&adif_record(r));
            }
        }
        s
    }

    /// The whole logbook as RFC-4180 CSV (for spreadsheet / quick export).
    pub fn csv(&self) -> String {
        self.csv_in_range(None, None)
    }

    /// CSV restricted to `[from_unix, to_unix]` — same contract as [`Self::adif_in_range`].
    pub fn csv_in_range(&self, from_unix: Option<u64>, to_unix: Option<u64>) -> String {
        let mut s =
            String::from("Call,Grid,Band,Freq_MHz,Mode,RST_Sent,RST_Rcvd,Name,QTH,Comment,DateTimeUTC,Confirmed\n");
        for r in self.records_in_range(from_unix, to_unix) {
            let (y, mo, d, h, mi, se) = datetime_utc(r.when_unix);
            let dt = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{se:02}Z");
            let cells = [
                csv_cell(&r.call),
                csv_cell(r.grid.as_deref().unwrap_or("")),
                csv_cell(&r.band),
                format!("{:.6}", r.freq_mhz),
                csv_cell(&r.mode),
                csv_cell(r.rst_sent.as_deref().unwrap_or("")),
                csv_cell(r.rst_rcvd.as_deref().unwrap_or("")),
                csv_cell(r.name.as_deref().unwrap_or("")),
                csv_cell(r.qth.as_deref().unwrap_or("")),
                csv_cell(r.comment.as_deref().unwrap_or("")),
                dt,
                if r.confirmed { "Y" } else { "N" }.to_string(),
            ];
            s.push_str(&cells.join(","));
            s.push('\n');
        }
        s
    }
}

/// ADIF file header (`<EOH>`-terminated) — `pub` so an upload payload can be built
/// as `adif_header()` + N×`adif_record()` (TQSL needs a full ADIF file, not bare
/// records).
pub fn adif_header() -> String {
    "Nexus logbook\n<ADIF_VER:5>3.1.4\n<PROGRAMID:5>Nexus\n<EOH>\n".to_string()
}

/// One `<FIELD:len>value` tag.
fn field(name: &str, val: &str) -> String {
    format!("<{}:{}>{}", name, val.len(), val)
}

/// A `APP_TEMPO_UL_*` upload-state field as `"{outcome}|{when}|{detail}"` (or empty
/// if `None`). Length-prefixed, so a `|` in `detail` is fine; parsed via splitn(3).
fn upload_field(name: &str, st: &Option<UploadStatus>) -> String {
    match st {
        Some(s) => field(
            name,
            &format!(
                "{}|{}|{}",
                s.outcome.code(),
                s.when_unix,
                s.detail.as_deref().unwrap_or("")
            ),
        ),
        None => String::new(),
    }
}

/// Serialize a single QSO as one ADIF record (ending in `<eor>`) — used by the
/// full-log export and the QRZ Logbook push (one-record INSERT).
pub fn adif_record(r: &QsoRecord) -> String {
    let (y, mo, d, h, mi, s) = datetime_utc(r.when_unix);
    let mut out = String::new();
    out.push_str(&field("CALL", &r.call));
    if let Some(g) = &r.grid {
        out.push_str(&field("GRIDSQUARE", g));
    }
    if let Some(c) = &r.country {
        out.push_str(&field("COUNTRY", c));
    }
    if let Some(st) = &r.state {
        out.push_str(&field("STATE", st));
    }
    // BAND only when we have one: a record made on an off-band-table dial (a 47 GHz
    // transverter shot, a satellite leg above the table) carries band = "", and
    // `<BAND:0>` with an empty value is exactly the malformed field LoTW/TQSL reject —
    // FREQ below still identifies the RF. Same absent-not-guessed rule as FREQ.
    if !r.band.is_empty() {
        out.push_str(&field("BAND", &r.band));
    }
    // FREQ only when we actually have one. Imported QSOs (QRZ/LoTW give BAND, not frequency)
    // carry freq_mhz = 0, and `<FREQ:8>0.000000` is not a valid amateur frequency — Swisslog,
    // DXKeeper and other loggers REJECT a zero-frequency record on import, which is how an
    // operator's oldest imported contacts silently fail to land in the destination logbook.
    // BAND alone is valid ADIF (every logger derives the band from it), so omit FREQ rather
    // than emit a zero that gets the whole record thrown away.
    if r.freq_mhz.is_finite() && r.freq_mhz > 0.0 {
        out.push_str(&field("FREQ", &format!("{:.6}", r.freq_mhz)));
    }
    // A mode whose own spelling is not in the ADIF Mode enumeration rides as its REGISTERED
    // PARENT + a SUBMODE. The enumeration is CLOSED (47 values; "DATA" is not among them —
    // that exists only inside LoTW), so a bare <MODE:9>TempoFast is rejected outright by
    // TQSL: its cascade is MODE%SUBMODE -> SUBMODE -> MODE, all three miss, and the record
    // is dropped with "Invalid MODE". SUBMODE is data type String and is explicitly NOT
    // validated against its enumeration, so a parent MODE + an unregistered SUBMODE is
    // spec-legal today with no coordination.
    // The parent comes from the table, it is NOT always MFSK: MFSK is the honest family for
    // the Tempo protocols and FT2 (TempoFast is 4-CPM h=1/2 BT=0.3, the same continuous-phase
    // FSK family as FST4 (4-GFSK), which already lives under MFSK), but FreeDV's parent is
    // DIGITALVOICE and the VARA family's is DYNAMIC — see `adif_submode` for #68.
    // APP_TEMPO_MODE preserves the exact protocol for round-trip fidelity into our own log;
    // it is never the primary carrier, because an APP_-only mode is invisible to every
    // uploader.
    match adif_submode(&r.mode) {
        Some((parent, sub)) => {
            out.push_str(&field("MODE", parent));
            out.push_str(&field("SUBMODE", sub));
            out.push_str(&field("APP_TEMPO_MODE", &r.mode));
        }
        None => out.push_str(&field("MODE", &r.mode)),
    }
    out.push_str(&field("QSO_DATE", &format!("{y:04}{mo:02}{d:02}")));
    // NEVER assert a time nobody measured: an imported record with no time of
    // day used to re-emit <TIME_ON:6>000000 as fact on every export and upload,
    // and LoTW/eQSL (which match on time) then held it unmatched forever.
    if r.time_known {
        out.push_str(&field("TIME_ON", &format!("{h:02}{mi:02}{s:02}")));
    }
    // TIME_OFF / QSO_DATE_OFF — the contact's end (closing 73/RR73), when recorded.
    if let Some(off) = r.time_off_unix {
        let (oy, omo, od, oh, omi, os) = datetime_utc(off);
        out.push_str(&field("QSO_DATE_OFF", &format!("{oy:04}{omo:02}{od:02}")));
        out.push_str(&field("TIME_OFF", &format!("{oh:02}{omi:02}{os:02}")));
    }
    if let Some(rs) = &r.rst_sent {
        out.push_str(&field("RST_SENT", rs));
    }
    if let Some(rr) = &r.rst_rcvd {
        out.push_str(&field("RST_RCVD", rr));
    }
    if let Some(n) = &r.name {
        out.push_str(&field("NAME", n));
    }
    if let Some(q) = &r.qth {
        out.push_str(&field("QTH", q));
    }
    if let Some(c) = &r.comment {
        out.push_str(&field("COMMENT", c));
    }
    if let Some(n) = &r.notes {
        out.push_str(&field("NOTES", n));
    }
    if let Some(p) = r.tx_power {
        out.push_str(&field("TX_PWR", &format!("{p}")));
    }
    // Award-relevant identity carried through imports: the NUMERIC entity id,
    // the satellite pair LoTW requires for satellite credit, and who operated /
    // which station logged it.
    if let Some(x) = r.dxcc {
        out.push_str(&field("DXCC", &x.to_string()));
    }
    if let Some(p) = &r.prop_mode {
        out.push_str(&field("PROP_MODE", p));
    }
    if let Some(sn) = &r.sat_name {
        out.push_str(&field("SAT_NAME", sn));
    }
    if let Some(op) = &r.operator {
        out.push_str(&field("OPERATOR", op));
    }
    if let Some(sc) = &r.station_callsign {
        out.push_str(&field("STATION_CALLSIGN", sc));
    }
    // Emit each confirming channel FAITHFULLY (the old two-bool collapse
    // rewrote paper cards as LOTW_QSL_RCVD on every save). Legacy in-memory
    // records (bools set, per-source empty) keep the old best-guess emission
    // so their round-trip is unchanged until a sync refreshes them.
    if r.qsl_rcvd.any() {
        if r.qsl_rcvd.card {
            out.push_str(&field("QSL_RCVD", "Y"));
        }
        if r.qsl_rcvd.lotw {
            out.push_str(&field("LOTW_QSL_RCVD", "Y"));
        }
        if r.qsl_rcvd.eqsl {
            out.push_str(&field("EQSL_QSL_RCVD", "Y"));
        }
        if r.qsl_rcvd.qrz {
            // QRZ Logbook native confirmation. APP_-namespaced so other loggers ignore it and it
            // never masquerades as an award-grade QSL_RCVD; round-trips back to `qrz` on import.
            out.push_str(&field("APP_QRZLOG_STATUS", "C"));
        }
    } else if r.award_confirmed {
        out.push_str(&field("LOTW_QSL_RCVD", "Y"));
    } else if r.confirmed {
        out.push_str(&field("EQSL_QSL_RCVD", "Y"));
    }
    // Operator-declared OUTBOUND QSL request (I sent a card/request) — standard
    // ADIF so any logger imports it. Emitted only when actually sent; the via/date
    // ride along when recorded. NOT a confirmation.
    if r.qsl_sent.sent {
        out.push_str(&field("QSL_SENT", "Y"));
        if let Some(via) = r.qsl_sent.via {
            out.push_str(&field("QSL_SENT_VIA", via.code()));
        }
        if let Some(ts) = r.qsl_sent.date_unix {
            let (sy, smo, sd, ..) = datetime_utc(ts);
            out.push_str(&field("QSLSDATE", &format!("{sy:04}{smo:02}{sd:02}")));
        }
    }
    // Credit state round-trips so a reconciled log re-exports its granted/applied
    // awards (and re-imports back to the same state).
    if !r.credit_granted.is_empty() {
        out.push_str(&field("CREDIT_GRANTED", &r.credit_granted.join(",")));
    }
    if !r.credit_submitted.is_empty() {
        out.push_str(&field("CREDIT_SUBMITTED", &r.credit_submitted.join(",")));
    }
    // Outbound upload state (APP_-namespaced; other loggers ignore it).
    out.push_str(&upload_field("APP_TEMPO_UL_LOTW", &r.upload.lotw));
    out.push_str(&upload_field("APP_TEMPO_UL_EQSL", &r.upload.eqsl));
    out.push_str(&upload_field("APP_TEMPO_UL_QRZ", &r.upload.qrz));
    out.push_str(&upload_field("APP_TEMPO_UL_CLUBLOG", &r.upload.clublog));
    // Parks/Summits On The Air — standard ADIF so pota.app / the SOTA DB accept the
    // export. POTA (and WWFF) → SIG/SIG_INFO; SOTA → its dedicated *_SOTA_REF fields.
    out.push_str(&ota_fields(
        "MY_SIG",
        "MY_SIG_INFO",
        "MY_SOTA_REF",
        "MY_POTA_REF",
        &r.ota.my_program,
        &r.ota.my_ref,
    ));
    out.push_str(&ota_fields(
        "SIG",
        "SIG_INFO",
        "SOTA_REF",
        "POTA_REF",
        &r.ota.their_program,
        &r.ota.their_ref,
    ));
    // IOTA island-group reference — the standard ADIF `IOTA` field, so exports round-trip
    // and upload cleanly (LoTW/QRZ/ClubLog all recognize it).
    if let Some(iota) = r
        .ota
        .iota
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.push_str(&field("IOTA", iota));
    }
    // Fields this build does not model, preserved from import verbatim — see
    // [`QsoRecord::extra`]. Emitted last so modelled fields always lead.
    for (k, v) in &r.extra {
        out.push_str(&field(k, v));
    }
    out.push_str("<EOR>\n");
    out
}

/// Normalize + validate an ADIF `IOTA` reference ("NA-001") — a two-letter continent
/// (AN/AF/AS/EU/NA/OC/SA), a hyphen, then three digits. Returns the uppercased canonical
/// form, or `None` for a malformed value (so junk never counts toward the award).
fn valid_iota(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_uppercase();
    let (cont, num) = s.split_once('-')?;
    let cont_ok = matches!(cont, "AN" | "AF" | "AS" | "EU" | "NA" | "OC" | "SA");
    let num_ok = num.len() == 3 && num.bytes().all(|b| b.is_ascii_digit());
    (cont_ok && num_ok).then_some(s)
}

/// Like [`adif_record`] but with the operator's `STATION_CALLSIGN` + `MY_GRIDSQUARE` inserted —
/// required for LoTW to sign against the location EMBEDDED IN THE ADIF (TQSL's "use the location
/// in the ADIF file" mode), the traveling-operator workflow where no named TQSL Station Location
/// exists. Blank identity fields are skipped. Only used on the LoTW upload path, so ordinary ADIF
/// export is unchanged.
pub fn adif_record_with_station(r: &QsoRecord, station_call: &str, my_grid: &str) -> String {
    let base = adif_record(r);
    let mut extra = String::new();
    let call = station_call.trim();
    let grid = my_grid.trim();
    // The record may already carry its own STATION_CALLSIGN (imported multi-op
    // logs) — emitted by adif_record. Don't append a second one: TQSL takes the
    // record's own; a duplicate field is undefined territory.
    if !call.is_empty() && r.station_callsign.is_none() {
        extra.push_str(&field("STATION_CALLSIGN", call));
    }
    // Same duplicate-field guard for MY_GRIDSQUARE: it is not a modelled field,
    // so an imported record's own copy rides in `extra` and is already emitted
    // by adif_record — appending a second, conflicting one hands TQSL undefined
    // territory on the award-signing path.
    if !grid.is_empty() && !r.extra.iter().any(|(k, _)| k == "MY_GRIDSQUARE") {
        extra.push_str(&field("MY_GRIDSQUARE", grid));
    }
    if extra.is_empty() {
        return base;
    }
    // Insert the station fields just before the record terminator (`<EOR>` is ASCII, so the
    // byte offset from an uppercased search is valid on the original string).
    match base.to_ascii_uppercase().rfind("<EOR>") {
        Some(pos) => format!("{}{}{}", &base[..pos], extra, &base[pos..]),
        None => format!("{base}{extra}"),
    }
}

/// Emit the ADIF fields for one OTA side. SOTA uses its dedicated `*_SOTA_REF` field;
/// every other program (POTA, WWFF) uses the generic `SIG`/`SIG_INFO` pair. Empty
/// when not activating/hunting that side.
/// The ADIF `(MODE, SUBMODE)` pair for a mode whose OWN spelling is not in the ADIF Mode
/// enumeration, or `None` for anything already in it (FT8, CW, SSB, RTTY, ...), which is
/// emitted verbatim.
///
/// A PAIR, not a bare submode: the parent used to be hardcoded to `MODE=MFSK` at the call
/// site, which is right for the Tempo protocols and FT2 and wrong for everything else.
/// #68 (rogerloxton) is exactly that gap — FreeDV and VarAC QSOs exported as
/// `<MODE:6>FREEDV` / `<MODE:7>VARA HF`, spellings that miss the Mode enumeration and get
/// the record dropped by TQSL for the same reason a bare `<MODE:9>TempoFast` is, and
/// neither of their registered parents is MFSK.
///
/// Uppercase on the wire: TQSL uppercases everything anyway, ADIF enumeration values are
/// case-insensitive, and house style for new submodes is uppercase (FST4W, SCAMP_FAST).
fn adif_submode(mode: &str) -> Option<(&'static str, &'static str)> {
    match mode.trim().to_ascii_uppercase().as_str() {
        "TEMPOFAST" => Some(("MFSK", "TEMPOFAST")),
        "TEMPODEEP" => Some(("MFSK", "TEMPODEEP")),
        // ⭐ FT2 RIDES HERE FOR THE SAME REASON THE TEMPO MODES DO, and leaving it
        // out would have LOST CONTACTS. FT2 is Decodium's mode, not WSJT-X's, so it
        // is in neither the ADIF MODE enumeration nor TQSL's own mode table — a
        // bare <MODE:3>FT2 misses all three legs of the MODE%SUBMODE -> SUBMODE ->
        // MODE cascade and the record is DROPPED with "Invalid MODE", exactly as
        // the writer's note above describes for TempoFast.
        //
        // MFSK is the honest family here too, not a flag of convenience: FT2 is
        // 4-GFSK at 41.67 baud — the same continuous-phase FSK family as FST4,
        // which already lives under MFSK, and as FT4, whose symbol time it halves.
        "FT2" => Some(("MFSK", "FT2")),
        // -- #68 (rogerloxton): FreeDV and VarAC ------------------------------------
        // Neither program's mode name is a MODE value; both are SUBMODE values whose
        // parent IS in the enumeration. FreeDV's parent is DIGITALVOICE (it is digital
        // voice, and the awards/propagation side already classes FREEDV as Phone —
        // that classification reads `QsoRecord::mode`, which this does not touch).
        "FREEDV" | "FREE DV" => Some(("DIGITALVOICE", "FREEDV")),
        // Every VARA variant hangs off DYNAMIC. The registered submode spellings carry
        // the variant, and VARA FM carries its SPEED as part of the name — so an exact
        // typed spelling gets the exact registered submode.
        "VARA HF" | "VARAHF" => Some(("DYNAMIC", "VARA HF")),
        "VARA FM 1200" | "VARAFM1200" => Some(("DYNAMIC", "VARA FM 1200")),
        "VARA FM 9600" | "VARAFM9600" => Some(("DYNAMIC", "VARA FM 9600")),
        "VARA SATELLITE" | "VARASATELLITE" | "VARA SAT" => Some(("DYNAMIC", "VARA SATELLITE")),
        // Bare "VARA" / "VARA FM": the PARENT is certain (all of them are DYNAMIC), the
        // registered submode is not — bare "VARA" does not say HF, FM or satellite, and
        // "VARA FM" is registered only with a speed. Guessing one would put a bit rate
        // nobody measured in somebody else's database, so the operator's own words ride
        // as an unregistered SUBMODE instead: MODE=DYNAMIC is what makes the record land
        // (TQSL's cascade falls through an unmatched SUBMODE to it), which is the same
        // ground TEMPOFAST stands on.
        "VARA" => Some(("DYNAMIC", "VARA")),
        "VARA FM" | "VARAFM" => Some(("DYNAMIC", "VARA FM")),
        _ => None,
    }
}

/// Submodes the PARSER promotes to the record's mode — the reverse of
/// [`adif_submode`]'s cascade, plus the WSJT-X family submodes that are
/// first-class modes here (WSJT-X writes FT4/Q65/FST4/FST4W as MODE=MFSK +
/// SUBMODE). Returns the canonical in-app spelling.
///
/// Deliberately NOT every SUBMODE: Log4OM-style phone submodes (SSB+USB/LSB)
/// must not rename phone rows — SSB is the mode the log carries and the one we
/// should keep storing, and the sideband spellings already meet each other in
/// [`dedup_mode`], so promoting them would buy nothing.
fn promoted_submode(sub: &str) -> Option<&'static str> {
    match sub.trim().to_ascii_uppercase().as_str() {
        "TEMPOFAST" => Some("TempoFast"),
        "TEMPODEEP" => Some("TempoDeep"),
        "FT4" => Some("FT4"),
        // The read side of the FT2 cascade above — without it our own export
        // re-imports as bare "MFSK" and the mode is lost on the next full save.
        "FT2" => Some("FT2"),
        "Q65" => Some("Q65"),
        "FST4" => Some("FST4"),
        "FST4W" => Some("FST4W"),
        _ => None,
    }
}

fn ota_fields(
    sig: &str,
    sig_info: &str,
    sota: &str,
    pota: &str,
    program: &Option<String>,
    reference: &Option<String>,
) -> String {
    match (program.as_deref(), reference.as_deref()) {
        (Some(p), Some(r)) if p.eq_ignore_ascii_case("SOTA") => field(sota, r),
        // POTA emits BOTH conventions. SIG/SIG_INFO is what pota.app's own exports use and
        // is understood everywhere, but it is overloaded (WWFF and special events use it
        // too), which is exactly why ADIF 3.1.4 added the dedicated POTA_REF/MY_POTA_REF.
        // Loggers that key on the dedicated field — HRDLog among them — see no park at all
        // from SIG_INFO alone. Emitting both is safe: an ADIF reader ignores tags it does
        // not know. (Our own parser already READS POTA_REF; this closes the read/write gap.)
        (Some(p), Some(r)) if p.eq_ignore_ascii_case("POTA") => {
            field(sig, p) + &field(sig_info, r) + &field(pota, r)
        }
        (Some(p), Some(r)) => field(sig, p) + &field(sig_info, r),
        _ => String::new(),
    }
}

/// One RFC-4180 CSV cell (quote if it contains a comma, quote, or newline).
fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Import dedup identity: call (upper) + band (lower) + mode (canonical spelling,
/// see [`dedup_mode`]) + the exact contact second.
/// Needs-grade (preserves distinct QSOs, ignores re-imports), not award-grade.
type DedupKey = (String, String, String, u64);
/// Import-dedup identity: call + band + mode + the EXACT contact time. It once keyed
/// on the UTC *day* (`when_unix / 86_400`), which silently dropped genuinely distinct
/// QSOs that share a day — a rover working the same station from two grids hours apart,
/// or any second QSO after a band/opening change (measured: 35% of a real rover log,
/// 511 of an 11k master log). Keying on the exact second still collapses a true
/// re-import (identical timestamps) while preserving those distinct contacts. A
/// same-QSO pair whose sources disagree by a few seconds is now kept twice — benign
/// over-retention the operator can merge, versus the old silent data loss.
fn dedup_key(r: &QsoRecord) -> DedupKey {
    (
        r.call.to_ascii_uppercase(),
        r.band.to_ascii_lowercase(),
        dedup_mode(&r.mode),
        r.when_unix,
    )
}

/// Canonical mode SPELLING for [`dedup_key`] — the sideband names USB and LSB are
/// spellings of one mode, not modes of their own. ADIF says phone is `MODE=SSB` with
/// the sideband as a SUBMODE, but plenty of loggers write USB/LSB in the MODE field,
/// so a log round-tripped through one of them came back as a second copy of every
/// phone QSO. Folding them costs no discrimination: the key already carries the exact
/// second, and one station cannot be worked twice on one band in one second.
///
/// Nothing else is folded, on purpose. AM/FM/digital voice are genuinely different
/// modes, and the data modes are handled by the legacy-MFSK bridge in
/// [`Logbook::import_adif`] — keying on [`crate::reconcile::mode_class`] instead would
/// make every data mode one token, and since a row with no `TIME_ON` parks at
/// 00:00:00, two distinct digital QSOs with one station on one band on one date would
/// collapse and one real contact would vanish. Over-retention is the failure this
/// module prefers.
fn dedup_mode(mode: &str) -> String {
    let m = mode.trim().to_ascii_uppercase();
    match m.as_str() {
        "USB" | "LSB" => "SSB".to_string(),
        // Same provably-one-mode rule: BPSK31 is a common logger spelling of ADIF's
        // PSK31 (the B is the modulation the mode already means). The #31 record's last
        // unfolded example; the broad data-mode respellings stay handled by the
        // legacy-MFSK bridge in import_adif, and everything else stays unfolded —
        // over-retention remains the failure this module prefers.
        "BPSK31" => "PSK31".to_string(),
        _ => m,
    }
}

/// How many dated snapshots the `backups/` ring keeps. Ten is roughly a fortnight of an
/// active operator's saves — long enough that "my log looks wrong" is still recoverable when
/// they notice, short enough that the folder stays something a person can read. It is the
/// bound that normally binds; the byte ceiling below takes over for a very large log.
const BACKUP_KEEP: usize = 10;

/// Byte ceiling over the whole `backups/` ring. The biggest logs seen here are ~26,000 QSOs
/// at roughly 250 bytes a record — about 7 MB — so 64 MiB is ~9 snapshots of a log that
/// large: the ceiling binds before the count does exactly when a snapshot is expensive, and
/// the count binds first for the ordinary few-thousand-QSO log (~600 KB, well under). Either
/// way the folder is bounded by a number, not by "prune when it feels big".
const BACKUP_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// Wall clock, Unix seconds. `0` if the system clock is before the epoch — a nonsense stamp
/// is still a usable file name, and a backup must never fail over a clock.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether two files hold the same bytes. Length first (one `stat` each), so only a genuine
/// length match pays for the read. Any IO error answers `false` — "cannot prove they match"
/// must fall through to taking the snapshot, never to skipping it.
fn same_file_contents(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else {
        return false;
    };
    if ma.len() != mb.len() {
        return false;
    }
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Everything after the ADIF header's `<EOH>` — i.e. the QSO records — or the whole slice
/// when there is no header. Byte-level (and case-insensitive, as ADIF tags are) so it can
/// answer "does this file carry records?" for a log whose text is NOT valid UTF-8: that
/// question is what [`Logbook::backup_once`] gates on, and answering it on a decoded string
/// is what made a CP1253 log look empty and skip its own safety copy.
fn body_after_eoh(bytes: &[u8]) -> &[u8] {
    match bytes
        .windows(5)
        .position(|w| w.eq_ignore_ascii_case(b"<EOH>"))
    {
        Some(i) => &bytes[i + 5..],
        None => bytes,
    }
}

/// Minimal ADIF parser: reads `<NAME:len>value` tags, splitting records on
/// `<EOR>`. Tolerant of the header (everything up to `<EOH>` is skipped).
fn parse_adif(text: &str) -> Vec<QsoRecord> {
    let body = match text.to_ascii_uppercase().find("<EOH>") {
        Some(i) => &text[i + 5..],
        None => text,
    };
    let mut records = Vec::new();
    let mut cur: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let end = match body[i..].find('>') {
            Some(e) => i + e,
            None => break,
        };
        let tag = &body[i + 1..end];
        i = end + 1;
        let upper = tag.to_ascii_uppercase();
        if upper == "EOR" {
            if let Some(rec) = record_from(std::mem::take(&mut cur)) {
                records.push(rec);
            }
            continue;
        }
        // NAME:len or NAME:len:type
        let mut parts = tag.splitn(3, ':');
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        let len: usize = parts
            .next()
            .and_then(|l| l.trim().parse().ok())
            .unwrap_or(0);
        // `len` is attacker-controllable (from a crafted `<NAME:len>`); use saturating
        // arithmetic so a huge value can't overflow `i + len` (release: wrap → i jumps
        // backwards → infinite loop) — it just clamps past the end and stops the scan.
        let end = i.saturating_add(len);
        let val = body.get(i..end).unwrap_or("").to_string();
        i = end;
        cur.insert(name, val);
    }
    records
}

/// ADIF Time: 6 digits HHMMSS **or 4 digits HHMM** — both legal per the spec.
/// The 4-digit form used to be silently discarded to midnight. `.get()` slicing:
/// the value is arbitrary UTF-8 from the file, so a multibyte char inside the
/// fixed offsets must degrade, never panic.
fn parse_adif_time(t: &str) -> Option<(u32, u32, u32)> {
    let t = t.trim();
    let two = |a: usize| t.get(a..a + 2).and_then(|x| x.parse::<u32>().ok());
    match t.len() {
        4 => Some((two(0)?, two(2)?, 0)),
        n if n >= 6 => Some((
            two(0).unwrap_or(0),
            two(2).unwrap_or(0),
            two(4).unwrap_or(0),
        )),
        _ => None,
    }
}

/// Consume an ADIF Y/N flag ("Y" = true). `remove`, not `get`: consumed =
/// modelled, and whatever record_from leaves in the map becomes
/// [`QsoRecord::extra`].
fn take_yes(f: &mut std::collections::HashMap<String, String>, k: &str) -> bool {
    f.remove(k).is_some_and(|v| v.eq_ignore_ascii_case("Y"))
}

/// Consume a `*_QSL_RCVD` flag. ADIF's `QSL_Rcvd` enumeration spells a confirmation
/// the importer HOLDS two ways, not one: `Y` (received) and `V` (verified — an award
/// credit has been granted against it), the value Club Log and DXKeeper write for a
/// credited QSO in the logs they export. Reading only `Y` silently demoted exactly
/// the operator's best QSOs to unconfirmed. Everything else is NOT a confirmation
/// and must never promote one — `N`, `R` (requested: asked for, not received) and
/// `I` (ignore/invalid) all stay false. Trimmed, so a padded value from a sloppy
/// export still states what it states.
///
/// LoTW's own report emits only `Y`/`N` (ARRL's published field table), so `V` is
/// about third-party logs, not about a LoTW download.
fn take_confirmed(f: &mut std::collections::HashMap<String, String>, k: &str) -> bool {
    f.remove(k).is_some_and(|v| {
        let v = v.trim();
        v.eq_ignore_ascii_case("Y") || v.eq_ignore_ascii_case("V")
    })
}

/// Consume an `APP_TEMPO_UL_*` upload stamp: "{outcome}|{when}|{detail}" —
/// splitn(3) so a detail containing '|' survives intact.
fn take_upload(f: &mut std::collections::HashMap<String, String>, k: &str) -> Option<UploadStatus> {
    let v = f.remove(k)?;
    let mut it = v.splitn(3, '|');
    let outcome = UploadOutcome::from_code(it.next()?)?;
    let when_unix = it.next()?.parse::<i64>().ok()?;
    let detail = it.next().filter(|s| !s.is_empty()).map(|s| s.to_string());
    Some(UploadStatus {
        outcome,
        when_unix,
        detail,
    })
}

/// Consume one OTA side (my_* or their_*): a SOTA ref (dedicated field) takes
/// precedence; else a SIG=POTA/WWFF pair; else the ADIF 3.1.4 dedicated
/// POTA_REF (what pota.app's exports carry — may hold a comma list, verbatim).
fn take_ota_side(
    f: &mut std::collections::HashMap<String, String>,
    sig: &str,
    sig_info: &str,
    sota: &str,
    pota: &str,
) -> (Option<String>, Option<String>) {
    let sota_v = f.remove(sota).filter(|s| !s.is_empty());
    let (sig_v, sig_info_v) = (f.remove(sig), f.remove(sig_info));
    let pota_v = f.remove(pota).filter(|s| !s.is_empty());
    if let Some(r) = sota_v {
        (Some("SOTA".to_string()), Some(r.to_ascii_uppercase()))
    } else if let (Some(p), Some(r)) = (sig_v, sig_info_v) {
        (Some(p.to_ascii_uppercase()), Some(r.to_ascii_uppercase()))
    } else if let Some(r) = pota_v {
        (Some("POTA".to_string()), Some(r.to_ascii_uppercase()))
    } else {
        (None, None)
    }
}

/// Build a record from one ADIF record's fields, CONSUMING the map: every
/// modelled field is `remove`d, and the remainder becomes [`QsoRecord::extra`]
/// verbatim — losslessness by construction, not by a hand-kept census.
fn record_from(mut f: std::collections::HashMap<String, String>) -> Option<QsoRecord> {
    let f = &mut f;
    // TRIMMED at the boundary: a whitespace-padded CALL from a sloppy export
    // otherwise never compares equal to the UI's trimmed edit payload, and an
    // ordinary edit would then read as a callsign CORRECTION — stripping the
    // record's confirmations under the H19 ruling.
    let call = f
        .remove("CALL")
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())?;
    // `.get()` slicing (not `s[a..b]`): arbitrary UTF-8 from the file — a
    // multibyte char inside the fixed date offsets must degrade, never panic.
    let (y, mo, d) = f
        .remove("QSO_DATE")
        .filter(|s| s.len() >= 8)
        .map(|s| {
            (
                s.get(0..4).and_then(|x| x.parse().ok()).unwrap_or(1970),
                s.get(4..6).and_then(|x| x.parse().ok()).unwrap_or(1),
                s.get(6..8).and_then(|x| x.parse().ok()).unwrap_or(1),
            )
        })
        .unwrap_or((1970, 1, 1));
    // Time of day is OPTIONAL knowledge: absent (or unparseable) TIME_ON keeps
    // the date's midnight anchor for ordering but records `time_known = false`
    // — the writer then emits NO fabricated time and the LoTW/eQSL batches
    // exclude the record. (`time_known` is finalized at the record build below,
    // where the off-time is in scope — see the fabricated-midnight note there.)
    let time_on = f.remove("TIME_ON").as_deref().and_then(parse_adif_time);
    let (h, mi, s) = time_on.unwrap_or((0, 0, 0));
    // Per-source truth first; the two consumption booleans derive from it
    // (any-channel for display, LoTW+paper for award counting — never eQSL/QRZ).
    // A QRZ Logbook FETCH marks a native confirmation in APP_QRZLOG_STATUS=C (some exports use
    // Y). Map that to the QRZ channel — deliberately NOT to `card`, so a QRZ-only confirmation
    // never wrongly earns award credit. LOTW_QSL_RCVD / EQSL_QSL_RCVD that QRZ re-reports still
    // flow to their own award-grade channels.
    let qrz_status = f
        .remove("APP_QRZLOG_STATUS")
        .is_some_and(|v| v.eq_ignore_ascii_case("C") || v.eq_ignore_ascii_case("Y"));
    let qsl_rcvd = QslRcvd {
        card: take_confirmed(f, "QSL_RCVD"),
        lotw: take_confirmed(f, "LOTW_QSL_RCVD"),
        eqsl: take_confirmed(f, "EQSL_QSL_RCVD"),
        qrz: qrz_status,
    };
    let confirmed = qsl_rcvd.any();
    let award_confirmed = qsl_rcvd.award();
    // Operator-declared OUTBOUND QSL request. Absent fields ⇒ default (not sent),
    // matching the QslRcvd legacy tolerance. QSLSDATE is date-only → UTC midnight.
    let qsl_sent = QslSent {
        sent: take_yes(f, "QSL_SENT"),
        via: f.remove("QSL_SENT_VIA").and_then(|v| QslVia::from_code(&v)),
        date_unix: f.remove("QSLSDATE").filter(|s| s.len() >= 8).map(|s| {
            let (sy, smo, sd) = (
                s.get(0..4).and_then(|x| x.parse().ok()).unwrap_or(1970),
                s.get(4..6).and_then(|x| x.parse().ok()).unwrap_or(1),
                s.get(6..8).and_then(|x| x.parse().ok()).unwrap_or(1),
            );
            unix_from_ymdhms(sy, smo, sd, 0, 0, 0)
        }),
    };
    let credit_granted = f
        .remove("CREDIT_GRANTED")
        .map(|s| parse_credit(&s))
        .unwrap_or_default();
    let credit_submitted = f
        .remove("CREDIT_SUBMITTED")
        .map(|s| parse_credit(&s))
        .unwrap_or_default();
    let upload = UploadState {
        // Prefer Nexus's own upload record; otherwise honor the standard ADIF
        // `LOTW_QSL_SENT=Y` — the QSO was already uploaded to LoTW by whatever tool
        // wrote the ADIF, so an imported log isn't counted as needing a LoTW upload it
        // already had (the inflated "Upload to LoTW (N)" count on an imported log).
        // LOTW_QSL_SENT itself is only INSPECTED (get, not remove): it stays in
        // `extra` and round-trips verbatim for other loggers.
        lotw: take_upload(f, "APP_TEMPO_UL_LOTW").or_else(|| {
            f.get("LOTW_QSL_SENT")
                .is_some_and(|v| v.eq_ignore_ascii_case("Y"))
                .then_some(UploadStatus {
                    outcome: UploadOutcome::Accepted,
                    when_unix: 0,
                    detail: Some("LOTW_QSL_SENT (imported)".into()),
                })
        }),
        eqsl: take_upload(f, "APP_TEMPO_UL_EQSL"),
        qrz: take_upload(f, "APP_TEMPO_UL_QRZ"),
        clublog: take_upload(f, "APP_TEMPO_UL_CLUBLOG"),
    };
    let (my_program, my_ref) =
        take_ota_side(f, "MY_SIG", "MY_SIG_INFO", "MY_SOTA_REF", "MY_POTA_REF");
    let (their_program, their_ref) = take_ota_side(f, "SIG", "SIG_INFO", "SOTA_REF", "POTA_REF");
    let ota = Ota {
        my_program,
        my_ref,
        their_program,
        their_ref,
        iota: f.remove("IOTA").and_then(|s| valid_iota(&s)),
    };
    // TIME_OFF / QSO_DATE_OFF (optional contact end). Per ADIF, QSO_DATE_OFF falls back
    // to QSO_DATE when only TIME_OFF is present; the 4-digit HHMM form is legal here too.
    let off_date = f.remove("QSO_DATE_OFF");
    let time_off_unix = f
        .remove("TIME_OFF")
        .as_deref()
        .and_then(parse_adif_time)
        .map(|(oh, omi, os)| {
            let (oy, omo, od) = off_date
                .filter(|s| s.len() >= 8)
                .map(|s| {
                    (
                        s.get(0..4).and_then(|x| x.parse().ok()).unwrap_or(y),
                        s.get(4..6).and_then(|x| x.parse().ok()).unwrap_or(mo),
                        s.get(6..8).and_then(|x| x.parse().ok()).unwrap_or(d),
                    )
                })
                .unwrap_or((y, mo, d));
            unix_from_ymdhms(oy, omo, od, oh, omi, os)
        });
    // The WRITER's identity cascade, mirrored: APP_TEMPO_MODE (the exact
    // protocol, ours) → a promoted SUBMODE (see promoted_submode) → MODE.
    // Reading MODE alone collapsed every Tempo QSO to bare "MFSK" on the
    // next load, and the next full save wrote that collapse back to disk.
    // All three are consumed either way (the writer re-derives them from
    // `mode`); an UNpromoted submode (Log4OM's SSB+USB) goes back into the
    // map so it lands in `extra` and round-trips verbatim.
    let app_mode = f.remove("APP_TEMPO_MODE");
    let submode = f.remove("SUBMODE");
    let mode_field = f.remove("MODE");
    let mode = app_mode
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .or_else(|| {
            submode
                .as_deref()
                .and_then(promoted_submode)
                .map(str::to_string)
        })
        .or_else(|| mode_field.clone())
        .unwrap_or_default();
    // #68 (rogerloxton): "the writer re-derives them" is only true for a submode the
    // PROMOTION consumes. FREEDV / VARA are deliberately not promoted (see
    // `promoted_submode`), so their SUBMODE used to park in `extra` and be emitted a
    // SECOND time next to the one `adif_submode` regenerates — a two-SUBMODE record on
    // the re-export of our own file. Drop a submode the writer will re-derive from the
    // resolved mode; a foreign one (Log4OM's SSB+USB, an imported VarAC row whose MODE
    // we keep as DYNAMIC) is untouched and still round-trips verbatim.
    if let Some(sub) = submode.filter(|s| {
        promoted_submode(s).is_none()
            && !adif_submode(&mode).is_some_and(|(_, w)| w.eq_ignore_ascii_case(s.trim()))
    }) {
        f.insert("SUBMODE".to_string(), sub);
    }
    let rec = QsoRecord {
        call,
        grid: f.remove("GRIDSQUARE"),
        country: f
            .remove("COUNTRY")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        state: f
            .remove("STATE")
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty()),
        band: f.remove("BAND").unwrap_or_default(),
        freq_mhz: f.remove("FREQ").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        mode,
        // RST is a string (CW "599" / phone "59" / digital "-12") per ADIF.
        rst_sent: f
            .remove("RST_SENT")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        rst_rcvd: f
            .remove("RST_RCVD")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        name: f
            .remove("NAME")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        qth: f
            .remove("QTH")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        comment: f
            .remove("COMMENT")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        notes: f
            .remove("NOTES")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        tx_power: f.remove("TX_PWR").and_then(|s| s.trim().parse().ok()),
        when_unix: unix_from_ymdhms(y, mo, d, h, mi, s),
        // FABRICATED-MIDNIGHT MIGRATION: every date-only import that earlier
        // builds wrote carries an invented `<TIME_ON:6>000000` — reading that
        // back as a measured time would cement the fabrication for the whole
        // legacy corpus (11k+ rows in the reference log) and leave the LoTW
        // exclusion inert exactly where it matters. A midnight WITH an off-time
        // is a real, natively-logged 00:00 UTC contact (the native writer always
        // records both); a midnight with NO off-time is a date-only import and
        // reads as time-unknown. The false-negative (a genuine midnight QSO from
        // a source with no off-times) shows up honestly in the excluded count
        // and is one edit away from uploading.
        time_known: time_on.is_some() && !((h, mi, s) == (0, 0, 0) && time_off_unix.is_none()),
        time_off_unix,
        confirmed,
        award_confirmed,
        qsl_rcvd,
        qsl_sent,
        credit_granted,
        credit_submitted,
        upload,
        ota,
        dxcc: f.remove("DXCC").and_then(|s| s.trim().parse().ok()),
        prop_mode: f
            .remove("PROP_MODE")
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty()),
        sat_name: f
            .remove("SAT_NAME")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        operator: f
            .remove("OPERATOR")
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty()),
        station_callsign: f
            .remove("STATION_CALLSIGN")
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty()),
        // Whatever the parser did not consume is a field it does not model —
        // preserved verbatim, by construction. Sorted: deterministic writes.
        extra: {
            let mut extra: Vec<(String, String)> = f.drain().collect();
            extra.sort();
            extra
        },
    };
    Some(rec)
}

/// Parse an ADIF credit list (`CREDIT_GRANTED`/`CREDIT_SUBMITTED`): comma-separated
/// entries, each `AWARD` or `AWARD:source` (sources `&`-joined) — keep the award
/// code, drop the source, normalize (upper, sorted, deduped).
fn parse_credit(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s
        .split(',')
        .map(|t| {
            t.split(':')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_uppercase()
        })
        .filter(|t| !t.is_empty())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Unix seconds → (year, month, day, hour, min, sec) UTC, via Howard Hinnant's
/// civil-from-days algorithm (no external crates). `pub` so the ALL.TXT decode log
/// (tempo-app) can format WSJT-X-style UTC timestamps without a date dependency.
pub fn datetime_utc(unix: u64) -> (i32, u32, u32, u32, u32, u32) {
    let secs = unix as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    );
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (year, m, d, h, mi, s)
}

/// Inclusive Unix bounds of one UTC calendar day, from `"YYYY-MM-DD"` (the wire format of an
/// HTML date input) — `(00:00:00, 23:59:59)` of that day. `None` for anything unparseable,
/// and the CALLER must treat that as an error, never as "no bound": a malformed bound that
/// silently exported the whole log would ship a file the operator believes is filtered (#98).
pub fn day_bounds_utc(date: &str) -> Option<(u64, u64)> {
    let mut it = date.split('-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((
        unix_from_ymdhms(y, m, d, 0, 0, 0),
        unix_from_ymdhms(y, m, d, 23, 59, 59),
    ))
}

/// Inverse of [`datetime_utc`] — (y,m,d,h,mi,s) UTC → Unix seconds.
fn unix_from_ymdhms(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> u64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + (h as i64) * 3600 + (mi as i64) * 60 + s as i64;
    secs.max(0) as u64
}

#[cfg(test)]
mod tests {

    /// #31's last unfolded example: BPSK31 is a logger spelling of ADIF's PSK31 — one mode,
    /// two strings. Re-importing a log round-tripped through such a logger must not double it.
    #[test]
    fn a_bpsk31_respelling_does_not_double_the_qso_on_reimport() {
        let adif_a = "Nexus\n<EOH>\n<CALL:5>W9XYZ<BAND:3>20m<MODE:5>PSK31<QSO_DATE:8>20260801<TIME_ON:6>121500<EOR>";
        let adif_b = "Nexus\n<EOH>\n<CALL:5>W9XYZ<BAND:3>20m<MODE:6>BPSK31<QSO_DATE:8>20260801<TIME_ON:6>121500<EOR>";
        let mut lb = Logbook::new();
        let (added, _, _) = lb.import_adif(adif_a);
        assert_eq!(added.len(), 1, "control: the first import stores the QSO");
        let (added2, skipped2, _) = lb.import_adif(adif_b);
        assert_eq!(
            (added2.len(), skipped2),
            (0, 1),
            "the respelled twin is the duplicate it is — one contact, two spellings"
        );
    }

    use super::*;

    // Regression for the operator's "export drops oldest QSOs" report (2026-07-23). Root
    // cause was import dedup keying on the UTC DAY, which collapsed genuinely distinct QSOs
    // sharing a day — measured 35% loss on a real rover log (same station worked from two
    // grids hours apart). Two contacts an hour apart must BOTH survive; an exact re-import
    // of the same second must still collapse to one.
    #[test]
    fn import_keeps_distinct_qsos_same_day_but_dedups_identical_timestamp() {
        // Build an ADIF record for N7ABC on 6m FT8 at `secs`-past-midnight on 2025-07-19.
        let mk = |secs: u64| {
            let hhmmss = (secs / 3600) * 10_000 + ((secs % 3600) / 60) * 100 + (secs % 60);
            format!(
                "<CALL:5>N7ABC<BAND:2>6m<MODE:3>FT8<QSO_DATE:8>20250719<TIME_ON:6>{hhmmss:06}<EOR>\n"
            )
        };
        // Same station/band/mode/day, but 14:00:00 and 15:00:00 — two distinct contacts.
        let adif = format!("Nexus\n<EOH>\n{}{}", mk(14 * 3600), mk(15 * 3600));
        let mut lb = Logbook::new();
        let (added, skipped, _) = lb.import_adif(&adif);
        assert_eq!(
            added.len(),
            2,
            "two distinct-time QSOs must both import (was 1 — data loss)"
        );
        assert_eq!(skipped, 0);
        // An identical second (a true re-import of the very same QSO) still collapses to one.
        let same = format!("Nexus\n<EOH>\n{}{}", mk(14 * 3600), mk(14 * 3600));
        let mut lb2 = Logbook::new();
        let (added2, skipped2, _) = lb2.import_adif(&same);
        assert_eq!(
            added2.len(),
            1,
            "identical-timestamp duplicate must dedup to one"
        );
        assert_eq!(skipped2, 1);
    }

    fn rec(call: &str, band: &str, when: u64) -> QsoRecord {
        QsoRecord {
            call: call.into(),
            grid: Some("EN37".into()),
            country: None,
            state: None,
            band: band.into(),
            freq_mhz: 14.0905,
            mode: "TempoFast".into(),
            rst_sent: Some("-10".into()),
            rst_rcvd: Some("-12".into()),
            name: None,
            qth: None,
            comment: None,
            notes: None,
            tx_power: None,
            when_unix: when,
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
            station_callsign: None,
            extra: Vec::new(),
        }
    }

    // #98 — the date-range export. Three QSOs on three UTC days: the range keeps exactly the
    // in-range ones (bounds inclusive, whole-day), and NO range is byte-identical to the
    // unbounded export — the pre-#98 behavior, which callers with no dates must still get.
    #[test]
    fn export_range_keeps_exactly_the_in_range_days_and_no_range_is_the_whole_log() {
        let mut lb = Logbook::new();
        // 2026-08-10, -11, -12, each mid-day UTC.
        let (d10, _) = day_bounds_utc("2026-08-10").unwrap();
        let (d11, _) = day_bounds_utc("2026-08-11").unwrap();
        let (d12, _) = day_bounds_utc("2026-08-12").unwrap();
        lb.add(rec("W1AAA", "20m", d10 + 43_200));
        lb.add(rec("W2BBB", "20m", d11 + 43_200));
        lb.add(rec("W3CCC", "20m", d12 + 43_200));

        // Middle day only: exactly the one QSO, not its neighbours.
        let (from, to) = day_bounds_utc("2026-08-11").unwrap();
        let one = lb.adif_in_range(Some(from), Some(to));
        assert!(one.contains("W2BBB"), "the in-range QSO is kept");
        assert!(
            !one.contains("W1AAA") && !one.contains("W3CCC"),
            "out-of-range days are excluded"
        );

        // From-only and to-only bound one side each.
        let tail = lb.adif_in_range(Some(day_bounds_utc("2026-08-11").unwrap().0), None);
        assert!(!tail.contains("W1AAA") && tail.contains("W2BBB") && tail.contains("W3CCC"));
        let head = lb.adif_in_range(None, Some(day_bounds_utc("2026-08-11").unwrap().1));
        assert!(head.contains("W1AAA") && head.contains("W2BBB") && !head.contains("W3CCC"));

        // No range at all = the whole log, byte-identical to the unbounded export.
        assert_eq!(lb.adif_in_range(None, None), lb.adif());
        assert_eq!(lb.csv_in_range(None, None), lb.csv());

        // CSV takes the same filter (one header line + one row).
        let csv = lb.csv_in_range(Some(from), Some(to));
        assert_eq!(csv.trim_end().lines().count(), 2);
        assert!(csv.contains("W2BBB"));
    }

    // #98 — a malformed bound must parse to None (the command turns that into an ERROR;
    // silently exporting the whole log under a filter the operator believes applied is
    // the failure this guards).
    #[test]
    fn day_bounds_rejects_malformed_dates() {
        assert!(day_bounds_utc("2026-08-11").is_some());
        for bad in [
            "",
            "2026",
            "2026-13-01",
            "2026-00-10",
            "2026-08-32",
            "2026-08-11-05",
            "next-tuesday",
        ] {
            assert!(day_bounds_utc(bad).is_none(), "{bad:?} must not parse");
        }
        // The bounds cover the whole UTC day, inclusive.
        let (lo, hi) = day_bounds_utc("2023-11-14").unwrap();
        assert_eq!(hi - lo, 86_399);
        assert!(
            lo <= 1_700_000_000 && 1_700_000_000 <= hi,
            "22:13:20 UTC falls inside its day"
        );
    }

    #[test]
    fn reconcile_disk_unions_both_instances_state_never_clobbers() {
        // The two-instance shared-log invariant. This instance ("B") holds QSO X unconfirmed but
        // with its OWN ClubLog upload stamp. "Disk" (written by instance A) has the SAME QSO X
        // now award-confirmed + QRZ-uploaded, plus a NEW QSO Y that A logged.
        let mut mem = Logbook::new();
        mem.add(rec("DL1ABC", "20m", 1_700_000_000));
        mem.records_mut()[0].upload.clublog = Some(UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_700_000_050,
            detail: None,
        });

        let mut x = rec("DL1ABC", "20m", 1_700_000_000);
        x.award_confirmed = true;
        x.confirmed = true;
        x.qsl_rcvd.lotw = true;
        x.upload.qrz = Some(UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_700_000_100,
            detail: None,
        });
        let y = rec("JA1XYZ", "40m", 1_700_000_500);
        let disk = adif_header() + &adif_record(&x) + &adif_record(&y);

        mem.reconcile_disk(&disk);

        // Y appended; X upgraded IN PLACE (not skipped, not duplicated).
        assert_eq!(mem.len(), 2, "the other instance's new QSO Y is added");
        let gx = mem.records().iter().find(|r| r.call == "DL1ABC").unwrap();
        // UNION: B's ClubLog stamp AND A's LoTW confirmation AND A's QRZ stamp all survive.
        assert!(
            gx.award_confirmed && gx.qsl_rcvd.lotw,
            "A's LoTW confirmation folded in"
        );
        assert_eq!(
            gx.upload.qrz.as_ref().map(|u| u.outcome),
            Some(UploadOutcome::Accepted),
            "A's QRZ upload stamp survives B's rewrite"
        );
        assert_eq!(
            gx.upload.clublog.as_ref().map(|u| u.outcome),
            Some(UploadOutcome::Accepted),
            "B's own ClubLog stamp is NOT clobbered"
        );
    }

    /// Build the two-instance divergence the recovery has to survive: the shared file
    /// holds A's contact (award-confirmed) AND B's own later contact, while B's MEMORY
    /// holds only its own — B appended without re-reading. Returns (B's logbook, disk text).
    fn two_instance_same_station(a_when: u64, b_when: u64) -> (Logbook, String) {
        let mut a_row = rec("W1AW", "20m", a_when);
        a_row.mode = "FT8".into();
        a_row.award_confirmed = true;
        a_row.confirmed = true;
        a_row.qsl_rcvd.lotw = true;
        let mut b_row = rec("W1AW", "20m", b_when);
        b_row.mode = "FT8".into();

        let disk = adif_header() + &adif_record(&a_row) + &adif_record(&b_row);
        let mut mem = Logbook::new();
        mem.add(b_row); // B's memory: ONLY its own, later QSO
        (mem, disk)
    }

    fn assert_both_survive_confirmation_on_the_right_row(mem: &Logbook, a_when: u64, b_when: u64) {
        let mut times: Vec<u64> = mem.records().iter().map(|r| r.when_unix).collect();
        times.sort_unstable();
        assert_eq!(
            times,
            vec![a_when, b_when],
            "both QSOs survive with their OWN timestamps (day-keyed matching folded A's row \
             onto B's and re-appended B's as a second copy)"
        );
        let a = mem
            .records()
            .iter()
            .find(|r| r.when_unix == a_when)
            .expect("A's QSO");
        let b = mem
            .records()
            .iter()
            .find(|r| r.when_unix == b_when)
            .expect("B's QSO");
        assert!(
            a.award_confirmed && a.qsl_rcvd.lotw,
            "the confirmation stays on the contact it was earned by"
        );
        assert!(
            !b.award_confirmed && !b.qsl_rcvd.lotw,
            "and is NOT folded onto the other contact with that station"
        );
    }

    #[test]
    fn reconcile_disk_pairs_our_own_rows_by_exact_time_not_by_utc_day() {
        // Two QSOs with ONE station on ONE band inside ONE UTC day — routine FT8.
        // The report matcher buckets by UTC DAY, so recovering our own file paired A's
        // 06:00 row with B's 18:00 row: the confirmation landed on the wrong contact,
        // the 06:00 QSO vanished and the 18:00 QSO was duplicated.
        let day = 20_000u64 * 86_400;
        let (mut mem, disk) = two_instance_same_station(day + 6 * 3600, day + 18 * 3600);
        mem.reconcile_disk(&disk);
        assert_both_survive_confirmation_on_the_right_row(&mem, day + 6 * 3600, day + 18 * 3600);
    }

    #[test]
    fn reconcile_disk_pairs_our_own_rows_across_adjacent_days() {
        // Same defect one day apart: the ±1-day midnight tolerance (right for a report
        // whose timestamps legitimately differ) pairs two of OUR OWN rows that differ by
        // 30 hours. Nothing about our own file needs that tolerance — we wrote both.
        let a_when = 20_000u64 * 86_400 + 6 * 3600; // day N, 06:00
        let b_when = 20_001u64 * 86_400 + 12 * 3600; // day N+1, 12:00
        let (mut mem, disk) = two_instance_same_station(a_when, b_when);
        mem.reconcile_disk(&disk);
        assert_both_survive_confirmation_on_the_right_row(&mem, a_when, b_when);
    }

    #[test]
    fn reconcile_disk_keeps_two_date_only_contacts_from_one_day() {
        // The exact-second key assumes the second is a MEASURED one. A date-only source (a
        // paper log, an old export) carries QSO_DATE and no TIME_ON, so every row of that
        // day parks at 00:00:00 UTC — and two distinct contacts with one station on one band
        // share the key. The two-instance divergence the exact key was built for then came
        // straight back through it: B holds only its own row, disk holds both, and pairing by
        // ORDER folded A's confirmation onto B's contact and appended a second copy of B —
        // A's contact destroyed by the next full rewrite.
        let day = 20_000u64 * 86_400;
        let mut a = rec("W1AW", "20m", day);
        a.mode = "CW".into();
        a.time_known = false; // date-only: no TIME_ON in the source
        a.rst_sent = Some("599".into());
        a.award_confirmed = true;
        a.confirmed = true;
        a.qsl_rcvd.lotw = true;
        let mut b = rec("W1AW", "20m", day);
        b.mode = "CW".into();
        b.time_known = false;
        b.rst_sent = Some("339".into());

        let disk = adif_header() + &adif_record(&a) + &adif_record(&b);
        // The collision is the WRITER's and PARSER's, not the fixture's: round-tripped, both
        // rows really do come back time-unknown at the same fabricated midnight.
        assert!(
            parse_adif(&disk)
                .iter()
                .all(|r| !r.time_known && r.when_unix == day),
            "date-only rows round-trip as time-unknown at 00:00:00"
        );

        let mut mem = Logbook::new();
        mem.add(b); // B's memory: only its own contact
        mem.reconcile_disk(&disk);

        assert_eq!(mem.len(), 2, "both contacts survive");
        let with = |rst: &str| -> Vec<&QsoRecord> {
            mem.records()
                .iter()
                .filter(|r| r.rst_sent.as_deref() == Some(rst))
                .collect()
        };
        assert_eq!(with("599").len(), 1, "A's contact, once");
        assert_eq!(with("339").len(), 1, "B's contact, once");
        assert!(
            with("599")[0].award_confirmed && with("599")[0].qsl_rcvd.lotw,
            "the confirmation stays on the contact that earned it"
        );
        assert!(
            !with("339")[0].award_confirmed,
            "and is not folded onto the other contact"
        );
    }

    #[test]
    fn reconcile_disk_does_not_resurrect_a_deleted_qso() {
        // The operator deleted a mis-logged contact; the recovery runs BEFORE the rewrite
        // that persists the deletion, so the row is still on disk. Exact-identity matching
        // must not treat "gone from memory" as "new on disk" — the caller's contract is
        // that our copy still holds the record being changed, and the delete happens after.
        let day = 20_000u64 * 86_400;
        let (mut mem, disk) = two_instance_same_station(day + 6 * 3600, day + 18 * 3600);
        mem.reconcile_disk(&disk); // fold in A's row while we still hold ours
        assert_eq!(mem.len(), 2);
        let i = mem
            .records()
            .iter()
            .position(|r| r.when_unix == day + 18 * 3600)
            .unwrap();
        assert!(mem.delete(i));
        assert_eq!(mem.len(), 1, "the deleted QSO is gone and stays gone");
        assert_eq!(mem.records()[0].when_unix, day + 6 * 3600);
    }

    #[test]
    fn upload_state_merge_keeps_the_more_recent_per_source() {
        let older = UploadStatus {
            outcome: UploadOutcome::Pending,
            when_unix: 100,
            detail: None,
        };
        let newer = UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 200,
            detail: None,
        };
        let mut a = UploadState {
            lotw: Some(older.clone()),
            ..Default::default()
        };
        let b = UploadState {
            lotw: Some(newer.clone()),
            qrz: Some(newer.clone()),
            ..Default::default()
        };
        a.merge_recent(&b);
        assert_eq!(a.lotw, Some(newer.clone()), "newer LoTW status wins");
        assert_eq!(a.qrz, Some(newer.clone()), "absent-locally qrz is adopted");
        // A more-recent local status is NOT downgraded by an older incoming one.
        let mut c = UploadState {
            lotw: Some(newer.clone()),
            ..Default::default()
        };
        c.merge_recent(&UploadState {
            lotw: Some(older),
            ..Default::default()
        });
        assert_eq!(c.lotw.map(|u| u.outcome), Some(UploadOutcome::Accepted));
    }

    #[test]
    fn tempofast_rides_as_an_mfsk_submode_not_a_bare_invalid_mode() {
        // <MODE:9>TempoFast is rejected outright by TQSL — its cascade is MODE%SUBMODE ->
        // SUBMODE -> MODE, all three miss, "Invalid MODE", record dropped. MODE=MFSK resolves
        // to LoTW's DATA group and uploads.
        let mut r = rec("W1AW", "20m", 1_700_000_000);
        r.mode = "TempoFast".into();
        let adif = adif_record(&r);
        assert!(adif.contains("<MODE:4>MFSK"), "must ride as MFSK: {adif}");
        assert!(adif.contains("TEMPOFAST"), "submode missing: {adif}");
        assert!(
            !adif.contains("<MODE:9>TempoFast"),
            "must NOT emit the bare invalid mode: {adif}"
        );
        // Round-trip fidelity: our own log can still tell TempoFast from TempoDeep.
        assert!(adif.contains("APP_TEMPO_MODE"), "app field missing: {adif}");
    }

    /// #68 (rogerloxton): FreeDV and VarAC QSOs exported with an invalid ADIF mode.
    /// The typed spelling went straight into MODE — `<MODE:6>FREEDV`, `<MODE:7>VARA HF` —
    /// and neither is in the ADIF Mode enumeration, so TQSL drops the record exactly as it
    /// drops a bare `<MODE:9>TempoFast`. Both have a REGISTERED parent that is NOT MFSK:
    /// FreeDV is a DIGITALVOICE submode, the whole VARA family is DYNAMIC.
    #[test]
    fn freedv_and_vara_ride_as_their_registered_parent_mode_plus_submode() {
        for (typed, parent, submode) in [
            ("FreeDV", "DIGITALVOICE", "FREEDV"),
            ("FREEDV", "DIGITALVOICE", "FREEDV"),
            ("VARA HF", "DYNAMIC", "VARA HF"),
            ("VARAHF", "DYNAMIC", "VARA HF"),
            ("VARA FM 1200", "DYNAMIC", "VARA FM 1200"),
            ("VARA FM 9600", "DYNAMIC", "VARA FM 9600"),
            ("VARA SATELLITE", "DYNAMIC", "VARA SATELLITE"),
            // No registered submode says which VARA this was, so the operator's own
            // words ride as an unregistered SUBMODE — the parent is what makes the
            // record land, and nothing invents a speed or a band nobody measured.
            ("VARA", "DYNAMIC", "VARA"),
            ("VARA FM", "DYNAMIC", "VARA FM"),
        ] {
            let mut r = rec("W1AW", "20m", 1_700_000_000);
            r.mode = typed.into();
            let adif = adif_record(&r);
            assert!(
                adif.contains(&field("MODE", parent)),
                "{typed} must ride as MODE={parent}: {adif}"
            );
            assert!(
                adif.contains(&field("SUBMODE", submode)),
                "{typed} must carry SUBMODE={submode}: {adif}"
            );
            assert!(
                !adif.contains(&field("MODE", typed)),
                "{typed} must NOT emit the bare invalid mode: {adif}"
            );
            // Round-trip: our own file must still say what was actually worked, with the
            // operator's own spelling intact (APP_TEMPO_MODE, same cascade the Tempo modes use).
            let back = &parse_adif(&(adif_header() + &adif))[0];
            assert_eq!(back.mode, typed, "the typed mode must survive our own file");
            // ...and re-exporting that record must not emit the SUBMODE twice. FREEDV/VARA are
            // deliberately NOT in `promoted_submode`, so without the writer-derived filter in
            // `record_from` the parsed submode parks in `extra` and is re-emitted alongside the
            // one the writer regenerates — a malformed two-SUBMODE record on the second export.
            let again = adif_record(back);
            assert_eq!(
                again.matches("<SUBMODE:").count(),
                1,
                "{typed}: exactly one SUBMODE on re-export: {again}"
            );
        }
    }

    /// #68 guard: `adif_submode` returning a (MODE, SUBMODE) pair instead of a bare submode
    /// sits on the path EVERY export and EVERY upload crosses, so no mode that already worked
    /// may move a single byte. These goldens are the pre-#68 writer's exact output.
    #[test]
    fn existing_modes_emit_byte_identical_adif_mode_blocks() {
        for (mode, golden) in [
            ("FT8", "<MODE:3>FT8"),
            ("FT4", "<MODE:3>FT4"),
            ("CW", "<MODE:2>CW"),
            ("SSB", "<MODE:3>SSB"),
            ("RTTY", "<MODE:4>RTTY"),
            ("PSK31", "<MODE:5>PSK31"),
            ("SSTV", "<MODE:4>SSTV"),
            ("MFSK", "<MODE:4>MFSK"),
            ("Q65", "<MODE:3>Q65"),
            ("WSPR", "<MODE:4>WSPR"),
            (
                "TempoFast",
                "<MODE:4>MFSK<SUBMODE:9>TEMPOFAST<APP_TEMPO_MODE:9>TempoFast",
            ),
            (
                "TempoDeep",
                "<MODE:4>MFSK<SUBMODE:9>TEMPODEEP<APP_TEMPO_MODE:9>TempoDeep",
            ),
            ("FT2", "<MODE:4>MFSK<SUBMODE:3>FT2<APP_TEMPO_MODE:3>FT2"),
        ] {
            let mut r = rec("W1AW", "20m", 1_700_000_000);
            r.mode = mode.into();
            let adif = adif_record(&r);
            assert!(
                adif.contains(golden),
                "{mode}: the mode block moved — expected {golden} in {adif}"
            );
            // One MODE field, and a SUBMODE exactly when the golden has one (a mode that
            // never carried a submode must not grow one). `<APP_TEMPO_MODE:` does not
            // contain `<MODE:`, so the count is honest.
            assert_eq!(adif.matches("<MODE:").count(), 1, "{mode}: {adif}");
            assert_eq!(
                adif.matches("<SUBMODE:").count(),
                usize::from(golden.contains("<SUBMODE:")),
                "{mode}: {adif}"
            );
        }
    }

    #[test]
    fn every_loggable_mode_round_trips_through_its_own_adif() {
        // The old assertion here was write-side only — it checked the identity
        // fields were EMITTED and never parsed the record back, which is exactly
        // the half that was broken: every restart re-read a TempoFast QSO as
        // bare "MFSK" and the next full save made that permanent on disk.
        for mode in [
            "FT8",
            "FT4",
            "FT2",
            "TempoFast",
            "TempoDeep",
            "Q65",
            "FST4",
            "FST4W",
            "WSPR",
            "MSK144",
            "JT65",
            "CW",
            "SSB",
            "RTTY",
            "SSTV",
        ] {
            let mut r = rec("W1AW", "20m", 1_753_500_000);
            r.mode = mode.into();
            let adif = adif_header() + &adif_record(&r);
            let back = &parse_adif(&adif)[0];
            assert_eq!(
                back.mode, mode,
                "mode identity must survive the app's own file"
            );
        }
        // The WSJT-X shape for the family submodes (their FT4/Q65/FST4 ride
        // MODE=MFSK + SUBMODE) resolves to the first-class mode on import.
        let wsjtx = "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>012345\
                     <BAND:3>20m<MODE:4>MFSK<SUBMODE:3>FT4<EOR>";
        assert_eq!(parse_adif(wsjtx)[0].mode, "FT4");
        // A phone submode must NOT rename the row — the import dedup key is the
        // RAW mode + exact second, so promoting SSB→USB would duplicate every
        // phone row on the next re-import of the same log.
        let ssb = "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>012345\
                   <BAND:3>20m<MODE:3>SSB<SUBMODE:3>USB<EOR>";
        assert_eq!(parse_adif(ssb)[0].mode, "SSB");
    }

    #[test]
    fn unknown_time_is_never_fabricated_and_hhmm_is_accepted() {
        // ADIF's Time type is legally 6 digits (HHMMSS) OR 4 (HHMM). The old
        // parse discarded the 4-digit form to midnight, and an absent TIME_ON
        // became midnight too — then the writer re-asserted <TIME_ON:6>000000 as
        // fact on every export and upload. LoTW matches on the two operators'
        // times agreeing within 30 minutes, so those records could never
        // confirm, forever, on every service.
        //
        // 4-digit HHMM parses to the real time.
        let hhmm = "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:4>1423<BAND:3>20m<MODE:3>FT8<EOR>";
        let r = &parse_adif(hhmm)[0];
        assert!(r.time_known, "a 4-digit time IS a time");
        assert_eq!(r.when_unix % 86_400, 14 * 3600 + 23 * 60);

        // Absent TIME_ON → the date is kept for ordering, the time is UNKNOWN,
        // and the writer emits QSO_DATE with NO fabricated TIME_ON.
        let dateonly = "<CALL:4>K1JT<QSO_DATE:8>20260701<BAND:3>20m<MODE:3>FT8<EOR>";
        let r = &parse_adif(dateonly)[0];
        assert!(
            !r.time_known,
            "no TIME_ON → time unknown, not midnight-as-fact"
        );
        assert_eq!(r.when_unix % 86_400, 0, "date keeps its midnight anchor");
        let out = adif_record(r);
        assert!(
            !out.contains("TIME_ON"),
            "never assert a time nobody measured: {out}"
        );
        assert!(out.contains("<QSO_DATE:8>20260701"), "{out}");
        // And it round-trips as still-unknown.
        let back = &parse_adif(&(adif_header() + &out))[0];
        assert!(!back.time_known);

        // THE FABRICATED-MIDNIGHT MIGRATION: earlier builds wrote an invented
        // <TIME_ON:6>000000 on every date-only import — 11k+ rows in the
        // reference log — so a bare midnight reads back as time-UNKNOWN, or the
        // whole legacy corpus would cement its fabrication and the LoTW
        // exclusion would be inert exactly where it matters. A midnight WITH an
        // off-time is a real, natively-logged 00:00 UTC contact and stays known.
        let bare_midnight =
            "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>000000<BAND:3>20m<MODE:3>FT8<EOR>";
        assert!(
            !parse_adif(bare_midnight)[0].time_known,
            "a bare 000000 is the legacy fabrication, not a measurement"
        );
        let real_midnight = "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>000000\
                             <TIME_OFF:6>000130<BAND:3>20m<MODE:3>FT8<EOR>";
        assert!(
            parse_adif(real_midnight)[0].time_known,
            "a midnight with an off-time is a genuine 00:00 UTC contact"
        );

        // TIME_OFF accepts the 4-digit form too.
        let off4 = "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>012345\
                    <TIME_OFF:4>0130<BAND:3>20m<MODE:3>FT8<EOR>";
        let r = &parse_adif(off4)[0];
        assert_eq!(r.time_off_unix.map(|t| t % 86_400), Some(3600 + 30 * 60));
    }

    #[test]
    fn a_qso_nexus_logged_writes_neither_satellite_field() {
        // The ADIF half of the removed engine stamp. Nexus writes PROP_MODE /
        // SAT_NAME for no contact it logs itself — satellite tagging is not
        // done yet, and every push path (LoTW/eQSL/ClubLog/QRZ/Cloudlog)
        // serializes through this function, so a leak here tags an operator's
        // whole upload with a bird that was never in the sky. Records that
        // ARRIVE with the pair still carry it: that is the next assertion pair.
        let r = rec("W1AW", "20m", 1_700_000_000);
        assert_eq!(r.prop_mode, None, "a fresh record carries a prop mode");
        assert_eq!(r.sat_name, None, "a fresh record carries a satellite");
        let adif = adif_record(&r);
        assert!(!adif.contains("PROP_MODE"), "PROP_MODE on the wire: {adif}");
        assert!(!adif.contains("SAT_NAME"), "SAT_NAME on the wire: {adif}");

        // An operator-supplied / imported pair is written, so a record repaired
        // by hand round-trips and can still be uploaded for satellite credit.
        let mut sat = rec("W1AW", "70cm", 1_700_000_000);
        sat.prop_mode = Some("SAT".into());
        sat.sat_name = Some("RS-44".into());
        let adif = adif_record(&sat);
        assert!(adif.contains("<PROP_MODE:3>SAT"), "no PROP_MODE in {adif}");
        assert!(adif.contains("<SAT_NAME:5>RS-44"), "no SAT_NAME in {adif}");
    }

    #[test]
    fn foreign_adif_fields_survive_a_round_trip_verbatim() {
        // A third-party master log carries decades of fields this parser does
        // not model. They must ride through import → save → re-read untouched —
        // the old parser silently discarded them at import, permanently.
        let rich = "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>012345<BAND:3>20m\
                    <MODE:3>FT8<DXCC:3>291<PROP_MODE:3>SAT<SAT_NAME:5>RS-44\
                    <OPERATOR:6>KD9TAW<STATION_CALLSIGN:6>KD9TAW\
                    <CONTEST_ID:6>ARRL-B<SRX:3>042<QSL_VIA:6>BUREAU<EOR>";
        let r = &parse_adif(rich)[0];
        assert_eq!(r.dxcc, Some(291), "numeric DXCC is modelled now");
        assert_eq!(r.prop_mode.as_deref(), Some("SAT"));
        assert_eq!(r.sat_name.as_deref(), Some("RS-44"));
        assert_eq!(r.operator.as_deref(), Some("KD9TAW"));
        assert_eq!(r.station_callsign.as_deref(), Some("KD9TAW"));
        // The unmodelled remainder is preserved BY CONSTRUCTION (whatever the
        // parser didn't consume), not by a hand-kept list that drifts.
        let extra: std::collections::HashMap<_, _> = r.extra.iter().cloned().collect();
        assert_eq!(extra.get("CONTEST_ID").map(String::as_str), Some("ARRL-B"));
        assert_eq!(extra.get("SRX").map(String::as_str), Some("042"));
        assert_eq!(extra.get("QSL_VIA").map(String::as_str), Some("BUREAU"));
        // And the writer re-emits all of it, exactly once.
        let out = adif_header() + &adif_record(r);
        let back = &parse_adif(&out)[0];
        assert_eq!(back.dxcc, Some(291));
        assert_eq!(back.extra, r.extra, "foreign fields survive the round trip");
        assert_eq!(out.matches("CONTEST_ID").count(), 1, "no duplication");
    }

    #[test]
    fn tempodeep_gets_its_own_submode_not_tempofasts() {
        let mut r = rec("W1AW", "20m", 1_700_000_000);
        r.mode = "TempoDeep".into();
        let adif = adif_record(&r);
        assert!(adif.contains("TEMPODEEP"), "{adif}");
        assert!(
            !adif.contains("TEMPOFAST"),
            "must not collapse the two: {adif}"
        );
    }

    #[test]
    fn standard_modes_are_emitted_verbatim() {
        // FT8/CW/SSB are real ADIF enumeration values — they must NOT be rewritten to MFSK.
        for m in ["FT8", "CW", "SSB", "RTTY"] {
            let mut r = rec("W1AW", "20m", 1_700_000_000);
            r.mode = m.into();
            let adif = adif_record(&r);
            assert!(
                adif.contains(&format!("<MODE:{}>{m}", m.len())),
                "{m}: {adif}"
            );
            assert!(
                !adif.contains("SUBMODE"),
                "{m} must not gain a submode: {adif}"
            );
        }
    }

    #[test]
    fn pota_emits_both_sig_info_and_the_dedicated_pota_ref() {
        // HRDLog (and other loggers) key on the ADIF 3.1.4 dedicated POTA_REF and see
        // nothing from SIG_INFO alone — that was a real "my park is missing" bug. pota.app
        // still wants SIG/SIG_INFO, so both must go out.
        let mut r = rec("W1AW", "20m", 1_700_000_000);
        r.ota.their_program = Some("POTA".into());
        r.ota.their_ref = Some("K-1234".into());
        r.ota.my_program = Some("POTA".into());
        r.ota.my_ref = Some("K-5678".into());
        let adif = adif_record(&r);
        for tag in [
            "SIG:",
            "SIG_INFO:",
            "POTA_REF:",
            "MY_SIG:",
            "MY_SIG_INFO:",
            "MY_POTA_REF:",
        ] {
            assert!(adif.contains(tag), "missing {tag} in {adif}");
        }
        assert!(adif.contains("K-1234"), "their park ref missing");
        assert!(adif.contains("K-5678"), "my park ref missing");
    }

    #[test]
    fn sota_still_uses_only_its_dedicated_ref() {
        // SOTA must NOT gain a POTA_REF — the dedicated-field branch is per-program.
        let mut r = rec("W1AW", "20m", 1_700_000_000);
        r.ota.their_program = Some("SOTA".into());
        r.ota.their_ref = Some("W7A/MN-001".into());
        let adif = adif_record(&r);
        assert!(adif.contains("SOTA_REF:"), "SOTA ref missing");
        assert!(
            !adif.contains("POTA_REF:"),
            "SOTA must not emit POTA_REF: {adif}"
        );
    }

    #[test]
    fn adif_record_with_station_injects_my_fields_before_eor() {
        let r = rec("W1AW", "20m", 1_700_000_000);
        let out = adif_record_with_station(&r, "KD9TAW", "EN61");
        assert!(
            out.contains("<STATION_CALLSIGN:6>KD9TAW"),
            "station call emitted: {out}"
        );
        assert!(
            out.contains("<MY_GRIDSQUARE:4>EN61"),
            "operator grid emitted: {out}"
        );
        // The station fields go INSIDE the record (before its <EOR> terminator).
        let eor = out.to_ascii_uppercase().rfind("<EOR>").unwrap();
        assert!(
            out[..eor].contains("STATION_CALLSIGN"),
            "inside the record, not after"
        );
        assert_eq!(out.matches("<EOR>").count(), 1, "still exactly one record");
        // Blank identity → unchanged from the plain record (named-location mode).
        assert_eq!(adif_record_with_station(&r, "", ""), adif_record(&r));
    }

    #[test]
    fn import_honors_lotw_qsl_sent_so_uploaded_qsos_arent_recounted() {
        // The inflated "Upload to LoTW (N)" fix: a QSO the ADIF says was already sent to LoTW
        // (LOTW_QSL_SENT=Y) is marked already-uploaded on import, so it's not re-offered.
        let mut lb = Logbook::default();
        let adif = "<CALL:5>W1ABC<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20240101<TIME_ON:6>120000<LOTW_QSL_SENT:1>Y<EOR>\n\
                    <CALL:5>W2DEF<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20240101<TIME_ON:6>130000<EOR>\n";
        lb.import_adif(adif);
        let sent = lb.records().iter().find(|r| r.call == "W1ABC").unwrap();
        let unsent = lb.records().iter().find(|r| r.call == "W2DEF").unwrap();
        assert!(
            sent.upload
                .lotw
                .as_ref()
                .is_some_and(|s| s.outcome.is_sent()),
            "LOTW_QSL_SENT=Y → counts as already on LoTW"
        );
        assert!(
            unsent.upload.lotw.is_none(),
            "no field → still needs uploading"
        );
    }

    /// ⭐ THE CONNECTOR-HEALTH SOURCE. The Connections panel's dot used to come from a
    /// keychain read, so a revoked password stayed green forever. These stamps are what it
    /// should have been reading — and unlike the keychain they are per-connector, dated,
    /// and carry the service's own reason.
    #[test]
    fn upload_health_reads_the_last_real_outcome_per_connector() {
        fn stamped(call: &str, when: u64, outcome: UploadOutcome, at: i64) -> QsoRecord {
            let mut r = rec(call, "20m", when);
            r.upload.lotw = Some(UploadStatus {
                outcome,
                when_unix: at,
                detail: Some("station location not found".into()),
            });
            r
        }
        let mut lb = Logbook::new();
        // Never touched: all-None. This is the case the panel used to render GREEN.
        assert_eq!(lb.upload_health().lotw, SourceHealth::default());
        assert_eq!(lb.upload_health().qrz, SourceHealth::default());

        // Newest wins, per half, and the two halves are independent — a later failure must
        // not erase the earlier success, or "working until 3pm, broken since" is unsayable.
        lb.add(stamped("W1AW", 1_700_000_000, UploadOutcome::Accepted, 100));
        lb.add(stamped(
            "W2AAA",
            1_700_000_100,
            UploadOutcome::Duplicate,
            300,
        ));
        lb.add(stamped(
            "W3BBB",
            1_700_000_200,
            UploadOutcome::AuthFail,
            200,
        ));
        let h = lb.upload_health();
        assert_eq!(
            h.lotw.last_success_unix,
            Some(300),
            "Duplicate is a SUCCESS — the service confirming it already holds the QSO \
             proves both the credentials and the record"
        );
        assert_eq!(h.lotw.last_failure_unix, Some(200));
        assert_eq!(
            h.lotw.last_failure_detail.as_deref(),
            Some("station location not found"),
            "the service's own reason rides along, or the operator is sent to the log to guess"
        );
        // Untouched connectors stay untouched — one connector's history is not another's.
        assert_eq!(h.eqsl, SourceHealth::default());

        // THE 1970 TRAP. An imported legacy log carrying LOTW_QSL_SENT=Y synthesises a
        // stamp with when_unix = 0 (see the ADIF reader). Counting it would tell every
        // operator with an imported log that their last LoTW upload was 1 Jan 1970.
        let mut imported = Logbook::new();
        imported.import_adif(
            "<CALL:5>W1ABC<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20240101<TIME_ON:6>120000\
             <LOTW_QSL_SENT:1>Y<EOR>\n",
        );
        assert!(
            imported.records()[0]
                .upload
                .lotw
                .as_ref()
                .is_some_and(|s| s.when_unix == 0),
            "control: the import really does synthesise a zero-dated stamp, so the \
             assertion below is testing something"
        );
        assert_eq!(
            imported.upload_health().lotw.last_success_unix,
            None,
            "an undated import is not evidence that an upload ever happened"
        );
    }

    #[test]
    fn qsl_sent_round_trips_through_adif() {
        // Standard ADIF QSL_SENT / QSL_SENT_VIA / QSLSDATE, not APP_-fields.
        let mut r = rec("W1AW", "20m", 1_700_000_000);
        r.qsl_sent = QslSent {
            sent: true,
            via: Some(QslVia::Bureau),
            date_unix: Some(unix_from_ymdhms(2024, 3, 9, 0, 0, 0)),
        };
        let adif = adif_header() + &adif_record(&r);
        assert!(adif.contains("<QSL_SENT:1>Y"));
        assert!(adif.contains("<QSL_SENT_VIA:1>B"));
        assert!(adif.contains("<QSLSDATE:8>20240309"));
        let back = &parse_adif(&adif)[0];
        assert_eq!(
            back.qsl_sent, r.qsl_sent,
            "QSL-sent survives the round-trip"
        );
        // A request is NOT a confirmation.
        assert!(!back.confirmed && !back.award_confirmed);

        // Direct with no recorded date: sent + via survive, date stays None.
        let mut d = rec("K2DEF", "40m", 1_700_000_100);
        d.qsl_sent = QslSent {
            sent: true,
            via: Some(QslVia::Direct),
            date_unix: None,
        };
        let dback = &parse_adif(&(adif_header() + &adif_record(&d)))[0];
        assert_eq!(dback.qsl_sent.via, Some(QslVia::Direct));
        assert!(dback.qsl_sent.sent && dback.qsl_sent.date_unix.is_none());
    }

    #[test]
    fn qsl_sent_absent_fields_default_to_not_sent() {
        // Legacy record with no QSL_SENT tags (like every log before this feature)
        // parses back as the default — never spuriously "sent".
        let adif = "<EOH>\n<CALL:5>K2DEF<BAND:3>40m<MODE:3>FT8<EOR>\n";
        let back = &parse_adif(adif)[0];
        assert_eq!(back.qsl_sent, QslSent::default());
        assert!(!back.qsl_sent.sent);
        // And such a record emits NO QSL_SENT field on write-back.
        assert!(!adif_record(back).contains("QSL_SENT"));
    }

    #[test]
    fn mark_qsl_sent_declares_request_without_confirming() {
        let mut lb = Logbook::new();
        lb.add(rec("W1AW", "20m", 1_700_000_000));
        assert!(lb.mark_qsl_sent(0, QslVia::Electronic, 1_700_000_000));
        let r = &lb.records()[0];
        assert!(r.qsl_sent.sent);
        assert_eq!(r.qsl_sent.via, Some(QslVia::Electronic));
        // Marking a request must NEVER fabricate a confirmation.
        assert!(!r.confirmed && !r.award_confirmed && !r.qsl_rcvd.any());
        // Out-of-range is a no-op false.
        assert!(!lb.mark_qsl_sent(9, QslVia::Bureau, 1_700_000_000));
    }

    #[test]
    fn ota_round_trips_through_adif() {
        // POTA hunter contact while activating a SOTA summit (a P2P-ish mixed case):
        // my side = SOTA (dedicated ref field), their side = POTA (SIG/SIG_INFO).
        let mut r = rec("W1AW", "20m", 1_700_000_000);
        r.ota = Ota {
            my_program: Some("SOTA".into()),
            my_ref: Some("W7A/MN-001".into()),
            their_program: Some("POTA".into()),
            their_ref: Some("K-1234".into()),
            iota: Some("NA-001".into()),
        };
        let adif = adif_header() + &adif_record(&r);
        // Standard ADIF tags (so pota.app / SOTA DB accept the export), not APP_-fields.
        assert!(adif.contains("<MY_SOTA_REF:10>W7A/MN-001"));
        assert!(adif.contains("<SIG:4>POTA"));
        assert!(adif.contains("<SIG_INFO:6>K-1234"));
        assert!(adif.contains("<IOTA:6>NA-001"));
        let back = &parse_adif(&adif)[0];
        assert_eq!(back.ota, r.ota, "OTA context survives the ADIF round-trip");

        // A pure POTA activation (my side POTA via SIG, no hunter side).
        let mut p = rec("K2DEF", "40m", 1_700_000_100);
        p.ota.my_program = Some("POTA".into());
        p.ota.my_ref = Some("K-5678".into());
        let padif = adif_header() + &adif_record(&p);
        assert!(padif.contains("<MY_SIG:4>POTA"));
        assert!(padif.contains("<MY_SIG_INFO:6>K-5678"));
        let pback = &parse_adif(&padif)[0];
        assert_eq!(pback.ota.my_program.as_deref(), Some("POTA"));
        assert_eq!(pback.ota.my_ref.as_deref(), Some("K-5678"));
        assert_eq!(pback.ota.their_program, None);
    }

    #[test]
    fn a_call_correction_requeues_the_upload_and_strips_wrong_call_credit() {
        // Operator ruling (2026-07-30). Work a new one, mis-copy the call, it
        // uploads as W1AX; fix it to W1AW. The services still hold W1AX — the
        // record must RE-QUEUE (upload stamps cleared), and any confirmation
        // matched against the busted call is credit this QSO never earned, so
        // it goes too. The old behavior preserved both: Nexus showed W1AW,
        // LoTW held W1AX forever, and the QSO could never confirm.
        let mut lb = Logbook::new();
        let mut original = rec("W1AX", "20m", 1_700_000_000); // busted call: should be W1AW
        original.confirmed = true;
        original.award_confirmed = true;
        original.qsl_rcvd.lotw = true;
        original.credit_granted = vec!["DXCC".into()];
        original.qsl_sent = QslSent {
            sent: true,
            via: Some(QslVia::Direct),
            date_unix: Some(1_700_000_000),
        };
        original.upload.lotw = Some(UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1,
            detail: None,
        });
        lb.add(original);

        let fixed = rec("W1AW", "20m", 1_700_000_000);
        assert!(lb.update_record(0, fixed));

        let r = &lb.records()[0];
        assert_eq!(r.call, "W1AW", "human field corrected");
        assert!(
            r.upload.lotw.is_none(),
            "upload stamps cleared — the corrected QSO re-queues to every service"
        );
        assert!(
            !r.confirmed && !r.award_confirmed && !r.qsl_rcvd.lotw,
            "a confirmation matched on the busted call is stripped"
        );
        assert!(
            r.credit_granted.is_empty(),
            "granted credit rode the stripped confirmation"
        );
        assert!(
            r.qsl_sent.sent,
            "the operator's own outbound-card mark is history, not credit — kept"
        );
        assert!(
            !lb.update_record(9, rec("X", "20m", 1)),
            "out-of-range is false"
        );
    }

    #[test]
    fn a_call_correction_survives_a_save_and_reload_uncleared() {
        // The resurrection the adversarial review caught: an imported
        // LOTW_QSL_SENT=Y rode in `extra`, was re-emitted on save, and the
        // parser's fallback re-derived upload.lotw = Accepted on the next load
        // — quietly undoing the correction's clear and excluding the corrected
        // QSO from the LoTW batch forever.
        let adif = "<CALL:4>W1AX<QSO_DATE:8>20260101<TIME_ON:6>120000<BAND:3>20m\
                    <MODE:3>FT8<LOTW_QSL_RCVD:1>Y<LOTW_QSL_SENT:1>Y<EOR>";
        let mut lb = Logbook::new();
        lb.import_adif(&(adif_header() + adif));
        assert!(
            lb.records()[0].upload.lotw.is_some(),
            "precondition: imported as already-sent"
        );
        let mut fixed = lb.records()[0].clone();
        fixed.call = "W1AW".into();
        fixed.extra = Vec::new(); // the edit payload always arrives with extra empty
        assert!(lb.update_record(0, fixed));
        assert!(lb.records()[0].upload.lotw.is_none());
        // Save → reload: the clear must STICK.
        let back = parse_adif(&(adif_header() + &adif_record(&lb.records()[0])));
        assert!(
            back[0].upload.lotw.is_none(),
            "LOTW_QSL_SENT must not resurrect the cleared upload stamp"
        );
        assert!(!back[0].confirmed, "nor the stripped confirmation");

        // And a PADDED imported call is trimmed at the boundary, so an ordinary
        // edit of such a record never reads as a call correction.
        let padded =
            "<CALL:6> W1AW <QSO_DATE:8>20260101<TIME_ON:6>120000<BAND:3>20m<MODE:3>FT8<EOR>";
        assert_eq!(parse_adif(padded)[0].call, "W1AW");
    }

    #[test]
    fn reimporting_a_wsjtx_log_after_submode_promotion_adds_no_duplicates() {
        // Rows imported by a pre-promotion build are stored as bare "MFSK";
        // the same source row now parses as "FT4". The legacy-twin probe must
        // treat the promoted row as the duplicate it is.
        let legacy =
            "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>012345<BAND:3>20m<MODE:4>MFSK<EOR>";
        let promoted = "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>012345<BAND:3>20m\
                        <MODE:4>MFSK<SUBMODE:3>FT4<EOR>";
        let mut lb = Logbook::new();
        lb.import_adif(&(adif_header() + legacy));
        let (added, skipped, _) = lb.import_adif(&(adif_header() + promoted));
        assert!(added.is_empty(), "the promoted twin is the same QSO");
        assert_eq!(skipped, 1);
        assert_eq!(lb.records().len(), 1);
    }

    #[test]
    fn reimporting_a_phone_qso_under_another_sideband_spelling_adds_no_duplicate() {
        // One contact, two loggers: ADIF says the mode is SSB and the sideband is a
        // SUBMODE, but plenty of programs write USB/LSB as the MODE. Round-tripping a
        // log through one of them re-imported a second copy of every phone QSO.
        let row = |mode: &str| {
            format!(
                "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>012345<BAND:3>20m<MODE:{}>{mode}<EOR>",
                mode.len()
            )
        };
        for (first, second) in [("USB", "SSB"), ("SSB", "USB"), ("LSB", "SSB")] {
            let mut lb = Logbook::new();
            lb.import_adif(&(adif_header() + &row(first)));
            let (added, skipped, _) = lb.import_adif(&(adif_header() + &row(second)));
            assert!(
                added.is_empty(),
                "{first} then {second} is one QSO, not two"
            );
            assert_eq!(skipped, 1, "{first} then {second}");
            assert_eq!(lb.records().len(), 1, "{first} then {second}");
        }
        // The fold is the sideband spellings only — FM is a different mode, and
        // collapsing it into phone would lose a real contact.
        let mut lb = Logbook::new();
        lb.import_adif(&(adif_header() + &row("SSB")));
        let (added, _, _) = lb.import_adif(&(adif_header() + &row("FM")));
        assert_eq!(added.len(), 1, "FM is not a spelling of SSB");
    }

    #[test]
    fn import_keeps_two_distinct_digital_qsos_that_share_a_date_only_stamp() {
        // Why the dedup key must NOT be rebuilt on `reconcile::mode_class`: it calls
        // every data mode "Digital", and a row with no TIME_ON parks at 00:00:00, so
        // an FT8 and an RTTY contact with one station on one band on one date would
        // share a key and one real QSO would disappear on import.
        let row = |mode: &str| {
            format!(
                "<CALL:4>K1JT<QSO_DATE:8>20260701<BAND:3>20m<MODE:{}>{mode}<EOR>",
                mode.len()
            )
        };
        let mut lb = Logbook::new();
        let (added, skipped, _) = lb.import_adif(&(adif_header() + &row("FT8") + &row("RTTY")));
        assert_eq!(added.len(), 2, "two modes, two contacts");
        assert_eq!(skipped, 0);
        // The premise these two rows rest on: both parked at the same second.
        assert_eq!(lb.records()[0].when_unix, lb.records()[1].when_unix);
        assert!(lb.records().iter().all(|r| !r.time_known));
    }

    #[test]
    fn the_lotw_signing_path_never_emits_two_my_gridsquares() {
        // An imported record's own MY_GRIDSQUARE rides in `extra` and is emitted
        // by adif_record; the station appender must not add a second, conflicting
        // one — a duplicate field hands TQSL undefined territory.
        let rich = "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>012345<BAND:3>20m\
                    <MODE:3>FT8<MY_GRIDSQUARE:6>EN61AB<EOR>";
        let r = &parse_adif(rich)[0];
        let out = adif_record_with_station(r, "KD9TAW", "EN52XX");
        assert_eq!(out.matches("MY_GRIDSQUARE").count(), 1, "{out}");
        assert!(out.contains("EN61AB"), "the record's own grid wins: {out}");
        // A record WITHOUT its own grid still gets the station's.
        let bare = "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>012345<BAND:3>20m<MODE:3>FT8<EOR>";
        let r = &parse_adif(bare)[0];
        let out = adif_record_with_station(r, "KD9TAW", "EN52XX");
        assert_eq!(out.matches("MY_GRIDSQUARE").count(), 1);
        assert!(out.contains("EN52XX"));
    }

    #[test]
    fn an_ordinary_edit_preserves_derived_state() {
        // A band/grid/name fix (call unchanged) keeps confirmations, credit,
        // upload stamps, the QSL-sent mark — and the import-carried identity
        // the edit form never carries (extra fields, DXCC, satellite pair).
        let mut lb = Logbook::new();
        let mut original = rec("W1AW", "20m", 1_700_000_000);
        original.confirmed = true;
        original.award_confirmed = true;
        original.credit_granted = vec!["DXCC".into()];
        original.qsl_sent = QslSent {
            sent: true,
            via: Some(QslVia::Direct),
            date_unix: Some(1_700_000_000),
        };
        original.upload.lotw = Some(UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1,
            detail: None,
        });
        original.dxcc = Some(291);
        original.sat_name = Some("RS-44".into());
        original.extra = vec![("CONTEST_ID".into(), "ARRL-B".into())];
        original.time_known = false; // an imported, time-less record
        lb.add(original);

        // The edit form's payload: band fixed, call unchanged, none of the
        // derived/import-carried fields present, when_unix untouched.
        let mut edit = rec("W1AW", "40m", 1_700_000_000);
        edit.confirmed = false;
        edit.award_confirmed = false;
        assert!(lb.update_record(0, edit));

        let r = &lb.records()[0];
        assert_eq!(r.band, "40m");
        assert!(r.confirmed && r.award_confirmed, "confirmation preserved");
        assert_eq!(r.credit_granted, vec!["DXCC".to_string()]);
        assert_eq!(
            r.upload.lotw.as_ref().map(|s| s.outcome),
            Some(UploadOutcome::Accepted),
            "upload state preserved on an ordinary edit"
        );
        assert!(r.qsl_sent.sent && r.qsl_sent.via == Some(QslVia::Direct));
        assert_eq!(r.dxcc, Some(291), "import-carried DXCC survives an edit");
        assert_eq!(r.sat_name.as_deref(), Some("RS-44"));
        assert_eq!(r.extra.len(), 1, "foreign fields survive an edit");
        assert!(
            !r.time_known,
            "an unchanged time must not fabricate time-knowledge"
        );
    }

    #[test]
    fn update_record_preserves_country_and_state_when_edit_omits_them() {
        let mut lb = Logbook::new();
        let mut original = rec("DL1XYZ", "20m", 1_700_000_000);
        original.country = Some("Germany".into());
        original.state = Some("NY".into());
        lb.add(original);

        // Edit payload (from the UI form) carries neither country nor state.
        let mut edit = rec("DL1XYZ", "40m", 1_700_000_000);
        edit.country = None;
        edit.state = None;
        assert!(lb.update_record(0, edit));

        let r = &lb.records()[0];
        assert_eq!(r.band, "40m", "human field edited");
        assert_eq!(
            r.country.as_deref(),
            Some("Germany"),
            "country preserved, not clobbered"
        );
        assert_eq!(
            r.state.as_deref(),
            Some("NY"),
            "state preserved, not clobbered"
        );

        // An edit that DOES carry a new country overrides it.
        let mut edit2 = rec("DL1XYZ", "40m", 1_700_000_000);
        edit2.country = Some("Fed. Rep. of Germany".into());
        assert!(lb.update_record(0, edit2));
        assert_eq!(
            lb.records()[0].country.as_deref(),
            Some("Fed. Rep. of Germany")
        );
    }

    #[test]
    fn delete_removes_and_shifts() {
        let mut lb = Logbook::new();
        lb.add(rec("A", "20m", 1));
        lb.add(rec("B", "20m", 2));
        lb.add(rec("C", "20m", 3));
        assert!(lb.delete(1)); // remove B
        let calls: Vec<_> = lb.records().iter().map(|r| r.call.as_str()).collect();
        assert_eq!(calls, vec!["A", "C"], "B removed, C shifted down");
        assert!(!lb.delete(5), "out-of-range is false");
    }

    #[test]
    fn stamp_ota_refs_stamps_matches_and_never_creates_or_overwrites() {
        let mut lb = Logbook::new();
        // Local log: a QSO with no park ref (14:03Z), one with a ref already, and a
        // different band that must NOT match.
        let day = 1_752_000_000u64 - (1_752_000_000 % 86_400); // some UTC midnight
        lb.add(rec("K2DEF", "20m", day + 14 * 3600 + 3 * 60));
        let mut has = rec("W9XYZ", "40m", day + 9 * 3600);
        has.ota.their_program = Some("POTA".into());
        has.ota.their_ref = Some("US-0001".into());
        lb.add(has);
        lb.add(rec("K2DEF", "40m", day + 14 * 3600 + 3 * 60));
        let n_before = lb.len();

        // pota.app hunter export: K2DEF at 14:10Z (within the ±30 min window) on 20m
        // with a POTA_REF; W9XYZ row matches but the local already has the ref; a
        // third row matches nothing local.
        let d = {
            let dt = day + 14 * 3600 + 10 * 60;
            let days = dt / 86_400;
            // civil date for the ADIF stamp
            let (y, m, dd) = {
                // 1970-01-01 + days — reuse a simple civil conversion for the test
                let mut y = 1970i64;
                let mut rem = days as i64;
                loop {
                    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
                    let len = if leap { 366 } else { 365 };
                    if rem < len {
                        break;
                    }
                    rem -= len;
                    y += 1;
                }
                let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
                let ml = [
                    31,
                    if leap { 29 } else { 28 },
                    31,
                    30,
                    31,
                    30,
                    31,
                    31,
                    30,
                    31,
                    30,
                    31,
                ];
                let mut m = 0usize;
                while rem >= ml[m] {
                    rem -= ml[m];
                    m += 1;
                }
                (y, m + 1, rem + 1)
            };
            format!("{y:04}{m:02}{dd:02}")
        };
        let adif = format!(
            "<CALL:5>K2DEF<QSO_DATE:8>{d}<TIME_ON:6>141000<BAND:3>20m<MODE:3>SSB<POTA_REF:7>US-4566<EOR>\n\
             <CALL:5>W9XYZ<QSO_DATE:8>{d}<TIME_ON:6>090500<BAND:3>40m<MODE:3>SSB<POTA_REF:7>US-9999<EOR>\n\
             <CALL:5>N0CAL<QSO_DATE:8>{d}<TIME_ON:6>120000<BAND:3>20m<MODE:3>SSB<POTA_REF:7>US-1111<EOR>\n"
        );
        let (stamped, already, unmatched) = lb.stamp_ota_refs(&adif);
        assert_eq!(stamped, 1, "K2DEF 20m got the park stamped");
        assert_eq!(
            already, 1,
            "W9XYZ kept its existing ref (never overwritten)"
        );
        assert_eq!(unmatched, 1, "N0CAL matched nothing");
        assert_eq!(lb.len(), n_before, "stamp-only: no records created");
        let k = lb
            .records()
            .iter()
            .find(|q| q.call == "K2DEF" && q.band == "20m")
            .unwrap();
        assert_eq!(k.ota.their_ref.as_deref(), Some("US-4566"));
        assert_eq!(k.ota.their_program.as_deref(), Some("POTA"));
        let w = lb.records().iter().find(|q| q.call == "W9XYZ").unwrap();
        assert_eq!(
            w.ota.their_ref.as_deref(),
            Some("US-0001"),
            "existing ref untouched"
        );
        // And the 40 m K2DEF (same call, wrong band) stayed unstamped.
        let k40 = lb
            .records()
            .iter()
            .find(|q| q.call == "K2DEF" && q.band == "40m")
            .unwrap();
        assert!(k40.ota.their_ref.is_none(), "band mismatch never stamps");
    }

    /// A unique scratch path under the OS temp dir — no external tempfile crate, and no
    /// `Date`/random (a static counter keeps runs from colliding).
    fn scratch_adi() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("tempo_logtest_{}_{n}.adi", std::process::id()))
    }

    /// THE DATA-LOSS REGRESSION. A file with a record `parse_adif` cannot assemble (no `CALL`)
    /// loads lossily; a save then rewrites the file from the surviving records, which on the
    /// real pipeline is how the operator's oldest QSOs vanished from disk. The `.bak` written at
    /// load time must still hold the ORIGINAL bytes, so nothing is ever permanently destroyed.
    #[test]
    fn a_lossy_load_then_save_cannot_destroy_the_original() {
        let path = scratch_adi();
        let bak = path.with_extension("adi.bak");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bak);

        // Two records; the second has no CALL, so record_from drops it.
        let raw = format!(
            "{}{}<QSO_DATE:8>20240101<TIME_ON:6>120000<BAND:3>20m<MODE:3>FT8<EOR>\n",
            adif_header(),
            adif_record(&rec("W1AW", "20m", 1_700_000_000)),
        );
        std::fs::write(&path, &raw).unwrap();

        let lb = Logbook::load(&path);
        assert_eq!(
            lb.records().len(),
            1,
            "the CALL-less record was dropped on load"
        );

        // The save that would truncate the file on disk.
        lb.save(&path).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            !on_disk.contains("120000"),
            "precondition: the save DID drop the unparseable record from log.adi"
        );

        // …but the backstop preserved the original verbatim.
        let saved = std::fs::read_to_string(&bak).expect(".bak was written at load time");
        assert_eq!(saved, raw, ".bak holds the original bytes, loss and all");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bak);
    }

    #[test]
    fn the_backup_is_written_once_and_never_clobbered_by_a_shrinking_file() {
        let path = scratch_adi();
        let bak = path.with_extension("adi.bak");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bak);

        let full = adif_header()
            + &adif_record(&rec("W1AW", "20m", 1))
            + &adif_record(&rec("K2DEF", "40m", 2));
        std::fs::write(&path, &full).unwrap();
        let _ = Logbook::load(&path); // first load → backup captures the full file
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), full);

        // A later session loads a SHRUNKEN file (a truncating save already ran). The backup
        // must NOT be overwritten — the earliest copy is the most complete.
        let shrunk = adif_header() + &adif_record(&rec("W1AW", "20m", 1));
        std::fs::write(&path, &shrunk).unwrap();
        let _ = Logbook::load(&path);
        assert_eq!(
            std::fs::read_to_string(&bak).unwrap(),
            full,
            "the second load must not clobber the more-complete backup"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bak);
    }

    #[test]
    fn an_empty_or_missing_file_writes_no_backup() {
        let path = scratch_adi();
        let bak = path.with_extension("adi.bak");
        let _ = std::fs::remove_file(&bak);
        // Missing file.
        let _ = Logbook::load(&path);
        assert!(!bak.exists(), "a missing log needs no backup");
        // Header-only (no QSOs) file.
        std::fs::write(&path, adif_header()).unwrap();
        let _ = Logbook::load(&path);
        assert!(!bak.exists(), "an empty log needs no backup");
        let _ = std::fs::remove_file(&path);
    }

    /// ★ THE GREEK-WINDOWS DATA DESTROYER (operator report, 2026-08: a Greek Windows 11
    /// user could not launch at all, and the investigation found this on the way past).
    ///
    /// A `log.adi` holding ANY non-UTF-8 byte — exactly what a Greek/German/French Windows
    /// logger writes into `NAME`/`QTH`/`COMMENT` in CP1253/CP1252 — made `read_to_string`
    /// return `Err`. `unwrap_or_default()` turned that into `""`, so the logbook loaded as
    /// EMPTY, `backup_once` skipped (empty body → nothing to lose), and the next `save()`
    /// rewrote `log.adi` from zero records. Every QSO gone, silently, with no copy.
    ///
    /// Two things are pinned here, and both are load-bearing: the records still PARSE, and
    /// the anchor `.bak` still gets the ORIGINAL BYTES. The fixture is real CP1253 —
    /// `Γιώργος` / `Αθήνα` / `Καλή επιτυχία` in NAME/QTH/COMMENT of the middle QSO.
    #[test]
    fn a_non_utf8_log_loads_its_records_and_is_still_backed_up() {
        // Committed fixture, not a string literal: a `&str` in this file cannot hold the
        // invalid bytes that ARE the bug.
        const CP1253: &[u8] = include_bytes!("../tests/fixtures/logbook-cp1253.adi");
        // clippy's `invalid_from_utf8` fires because it can evaluate the fixture at compile
        // time and sees the call can only ever return Err. That is EXACTLY the assertion: this
        // is the positive control proving the fixture really is non-UTF-8, without which the
        // rest of the test would pass just as happily against a plain-ASCII file and prove
        // nothing. The lint is right in general and wrong here, so it is allowed at the one
        // call site with the reason, never crate-wide. (It is a rustc lint, not a clippy one —
        // `clippy::invalid_from_utf8` is not a real lint name and `-D warnings` rejects it.)
        #[allow(invalid_from_utf8)]
        let fixture_is_not_utf8 = std::str::from_utf8(CP1253).is_err();
        assert!(
            fixture_is_not_utf8,
            "positive control: the fixture must actually be non-UTF-8, or this test proves nothing"
        );

        let path = scratch_adi();
        let bak = path.with_extension("adi.bak");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bak);
        std::fs::write(&path, CP1253).unwrap();

        let lb = Logbook::load(&path);
        let calls: Vec<&str> = lb.records().iter().map(|r| r.call.as_str()).collect();
        assert_eq!(
            calls,
            ["W1AW", "SV1AB", "SV2XYZ"],
            "every QSO must survive a log with non-UTF-8 bytes in it"
        );

        // …and the untouched original is on disk, byte for byte, before any save can run.
        let saved = std::fs::read(&bak).expect(".bak was written at load time");
        assert_eq!(
            saved, CP1253,
            ".bak holds the ORIGINAL bytes — including the ones that are not UTF-8"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bak);
    }

    // ── The bounded backup ring (operator request, 2026-08: "periodic logbook backups, and
    //    they must not accumulate forever — and launch must not pay for a big log") ──────────

    /// A unique, EMPTY scratch DIRECTORY to hold one `log.adi`. The ring lives in a `backups/`
    /// folder beside the log, so each test needs its own parent or they would rotate each
    /// other's snapshots. (Under the OS temp dir; nothing here touches the checkout.)
    fn scratch_log_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("tempo_logbak_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 2026-01-01T00:00:00Z — a fixed epoch so "day N" in these tests is a real calendar day.
    const D0: u64 = 1_767_225_600;
    const DAY: u64 = 86_400;

    /// The snapshot file names in `<dir>/backups`, oldest first.
    fn snaps(dir: &Path) -> Vec<String> {
        Logbook::snapshot_names(&dir.join("backups"), "log")
    }

    /// A log seeded on disk with `n` records, loaded back (which writes the anchor `.bak`).
    fn seeded(dir: &Path, n: usize) -> (Logbook, std::path::PathBuf) {
        let path = dir.join("log.adi");
        let mut raw = adif_header();
        for i in 0..n {
            raw.push_str(&adif_record(&rec(
                &format!("W{i}AAA"),
                "20m",
                1_700_000_000 + i as u64,
            )));
        }
        std::fs::write(&path, &raw).unwrap();
        let lb = Logbook::load(&path);
        assert_eq!(lb.records().len(), n, "seed loaded");
        (lb, path)
    }

    /// ★ THE LOAD PATH WRITES NOTHING. The operator's constraint on this whole batch: a big
    /// log must not make launch slow, so no snapshot may be taken at load time. Once the
    /// one-time anchor exists, opening the logbook must touch the disk for reads only — and
    /// it must NEVER create the `backups/` ring, on any load.
    #[test]
    fn loading_the_logbook_writes_nothing_and_never_creates_the_ring() {
        let dir = scratch_log_dir();
        let (_, path) = seeded(&dir, 3); // first load writes the anchor, and only the anchor

        let after_first: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let mut sorted = after_first.clone();
        sorted.sort();
        assert_eq!(
            sorted,
            ["log.adi", "log.adi.bak"],
            "the first load writes the anchor and nothing else — no backups/ folder"
        );

        // A second (and third) load, with the anchor already there: nothing at all.
        let before = std::fs::metadata(&path).unwrap().len();
        let _ = Logbook::load(&path);
        let _ = Logbook::load(&path);
        let mut now: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        now.sort();
        assert_eq!(now, sorted, "a load with the anchor present writes no file");
        assert!(
            !dir.join("backups").exists(),
            "LOAD must never create the backup ring — that is the startup cost the operator ruled out"
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            before,
            "load does not rewrite log.adi either"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Rotation drops the OLDEST dated snapshot, and the anchor is untouchable. Twelve daily
    /// saves against the SHIPPED bounds: ten survive, and they are the ten most recent.
    #[test]
    fn the_ring_keeps_the_newest_and_never_touches_the_anchor() {
        let dir = scratch_log_dir();
        let (mut lb, path) = seeded(&dir, 1);
        let anchor = dir.join("log.adi.bak");
        let anchor_bytes = std::fs::read(&anchor).unwrap();

        // One save a day for twelve days, each adding a QSO so the content genuinely changes.
        for day in 0..12u64 {
            lb.add(rec(&format!("K{day}XX"), "40m", 1_800_000_000 + day));
            lb.save_at(&path, D0 + day * DAY, BACKUP_KEEP, BACKUP_TOTAL_BYTES)
                .unwrap();
        }

        let kept = snaps(&dir);
        assert_eq!(
            kept.len(),
            BACKUP_KEEP,
            "the ring is capped at {BACKUP_KEEP}"
        );
        // Days 0 and 1 snapshotted the file as it stood BEFORE those saves; they are the two
        // that must have been dropped, and every survivor is newer.
        assert!(
            !kept
                .iter()
                .any(|n| n.contains("20260101") || n.contains("20260102")),
            "the two oldest were dropped, not two arbitrary ones: {kept:?}"
        );
        assert!(
            kept.iter().any(|n| n.contains("20260112")),
            "the newest survives: {kept:?}"
        );
        assert_eq!(
            std::fs::read(&anchor).unwrap(),
            anchor_bytes,
            "the anchor is not in backups/ and is never eligible for rotation"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The byte ceiling bites: with a cap smaller than the ring would otherwise fill, the
    /// oldest go until the folder is back under it — and the newest is never the one dropped.
    #[test]
    fn the_total_size_ceiling_drops_the_oldest_until_it_fits() {
        let dir = scratch_log_dir();
        let (mut lb, path) = seeded(&dir, 4);
        // Cap chosen against a real snapshot size so the ceiling, not the count, is what bites.
        let one = std::fs::metadata(&path).unwrap().len();
        let cap = one * 3;

        for day in 0..8u64 {
            lb.add(rec(&format!("N{day}YY"), "15m", 1_900_000_000 + day));
            lb.save_at(&path, D0 + day * DAY, 100, cap).unwrap(); // count bound deliberately slack
        }

        let kept = snaps(&dir);
        let total: u64 = kept
            .iter()
            .map(|n| {
                std::fs::metadata(dir.join("backups").join(n))
                    .unwrap()
                    .len()
            })
            .sum();
        assert!(!kept.is_empty(), "the ceiling never empties the ring");
        assert!(
            total <= cap,
            "ring is {total} bytes, over the {cap}-byte cap: {kept:?}"
        );
        assert!(kept.len() < 8, "the ceiling actually bit: {kept:?}");
        assert!(
            kept.last().unwrap().contains("20260108"),
            "the survivors are the NEWEST: {kept:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A second save on the SAME calendar day takes no second snapshot — that is the rule that
    /// keeps a busy logging session from filling the ring in an afternoon.
    #[test]
    fn a_second_save_on_the_same_day_takes_no_second_snapshot() {
        let dir = scratch_log_dir();
        let (mut lb, path) = seeded(&dir, 2);

        lb.save_at(&path, D0, BACKUP_KEEP, BACKUP_TOTAL_BYTES)
            .unwrap();
        assert_eq!(snaps(&dir).len(), 1, "the day's first save snapshots");

        lb.add(rec("W9ZZZ", "20m", 1_700_500_000));
        lb.save_at(&path, D0 + 3 * 3600, BACKUP_KEEP, BACKUP_TOTAL_BYTES)
            .unwrap();
        assert_eq!(
            snaps(&dir).len(),
            1,
            "a later save the same day adds nothing"
        );

        // Positive control: the NEXT day does snapshot, so the rule is a day gate and not a
        // "one snapshot ever" bug.
        lb.add(rec("W8ZZZ", "20m", 1_700_600_000));
        lb.save_at(&path, D0 + DAY, BACKUP_KEEP, BACKUP_TOTAL_BYTES)
            .unwrap();
        assert_eq!(snaps(&dir).len(), 2, "a new day snapshots again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A new day whose content is IDENTICAL to the newest snapshot takes no snapshot — the
    /// ring holds versions, not days, so an idle week must not push real history out of it.
    #[test]
    fn an_unchanged_log_takes_no_second_snapshot() {
        let dir = scratch_log_dir();
        let (mut lb, path) = seeded(&dir, 2);

        lb.save_at(&path, D0, BACKUP_KEEP, BACKUP_TOTAL_BYTES)
            .unwrap();
        assert_eq!(snaps(&dir).len(), 1);

        // Three more days of saves with nothing changed.
        for day in 1..4u64 {
            lb.save_at(&path, D0 + day * DAY, BACKUP_KEEP, BACKUP_TOTAL_BYTES)
                .unwrap();
        }
        assert_eq!(
            snaps(&dir).len(),
            1,
            "identical bytes are never snapshotted twice"
        );

        // Positive control — and it says exactly what the ring holds. A snapshot preserves the
        // file the save is ABOUT TO REPLACE, so adding a QSO is not visible to the check until
        // that save has landed: day 4 still sees the old bytes on disk and skips, day 5 sees
        // the four-record file and copies it. The ring lags one save behind by design; the
        // shrink trigger is what makes that safe.
        lb.add(rec("VE3ABC", "40m", 1_700_700_000));
        lb.save_at(&path, D0 + 4 * DAY, BACKUP_KEEP, BACKUP_TOTAL_BYTES)
            .unwrap();
        assert_eq!(
            snaps(&dir).len(),
            1,
            "the day-4 save had nothing new ON DISK to preserve yet"
        );
        lb.save_at(&path, D0 + 5 * DAY, BACKUP_KEEP, BACKUP_TOTAL_BYTES)
            .unwrap();
        assert_eq!(snaps(&dir).len(), 2, "changed content does snapshot");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ THE MOST VALUABLE TRIGGER. A save that SHRINKS the log snapshots unconditionally —
    /// same day, same minute, doesn't matter. Shrinking is exactly the shape of the failure
    /// that lost the operator's oldest QSOs, and the calendar rule would have sailed past it
    /// because the day already had its snapshot.
    #[test]
    fn a_shrinking_save_snapshots_even_on_a_day_already_snapshotted() {
        let dir = scratch_log_dir();
        let (mut lb, path) = seeded(&dir, 5);

        lb.save_at(&path, D0, BACKUP_KEEP, BACKUP_TOTAL_BYTES)
            .unwrap();
        assert_eq!(snaps(&dir).len(), 1, "the day's ordinary snapshot");
        let before = std::fs::read(&path).unwrap();

        // The truncating save: four QSOs vanish from memory, and the rewrite takes them off
        // disk. Same calendar day as the snapshot above.
        lb.records.truncate(1);
        lb.save_at(&path, D0 + 600, BACKUP_KEEP, BACKUP_TOTAL_BYTES)
            .unwrap();

        let kept = snaps(&dir);
        assert_eq!(
            kept.len(),
            2,
            "the shrink is snapshotted regardless: {kept:?}"
        );
        let shrink = kept.iter().find(|n| n.contains("-shrink")).expect(
            "the shrink snapshot is named so the operator can see which copy is the interesting one",
        );
        assert_eq!(
            std::fs::read(dir.join("backups").join(shrink)).unwrap(),
            before,
            "and it holds the log as it was BEFORE the truncating save"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_purges_all_and_reports_count() {
        let mut lb = Logbook::new();
        lb.add(rec("A", "20m", 1));
        lb.add(rec("B", "40m", 2));
        lb.add(rec("C", "15m", 3));
        assert_eq!(lb.clear(), 3, "returns the number removed");
        assert!(lb.is_empty(), "every record gone");
        // ADIF of an empty log is header-only — saving truncates the file cleanly.
        assert!(
            !lb.adif().contains("<CALL:"),
            "no QSO records remain in the ADIF"
        );
        assert_eq!(lb.clear(), 0, "purging an empty log removes nothing");
    }

    /// An imported QSO (QRZ/LoTW give BAND, not frequency) has `freq_mhz = 0`. Emitting
    /// `<FREQ:8>0.000000` makes a downstream logger (Swisslog, DXKeeper) reject the whole
    /// record — the mechanism behind an operator's oldest imported contacts silently failing to
    /// land in the destination log. BAND alone is valid ADIF, so a zero freq must be OMITTED,
    /// not written as zero.
    #[test]
    fn a_zero_frequency_is_omitted_so_downstream_loggers_do_not_reject_the_record() {
        let mut r = rec("VO1KVT", "10m", 1_700_000_000);
        r.freq_mhz = 0.0;
        let adif = adif_record(&r);
        assert!(
            !adif.contains("<FREQ"),
            "a zero FREQ must not be written: {adif}"
        );
        assert!(
            adif.contains("<BAND:3>10m"),
            "BAND still carries the band: {adif}"
        );
        // And it still round-trips — the band survives, freq stays absent (parses to 0).
        let back = parse_adif(&(adif_header() + &adif));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].band, "10m");
        assert_eq!(back[0].freq_mhz, 0.0);

        // A real frequency is still written, unchanged (6-decimal format).
        let good = rec("W1AW", "20m", 1_700_000_000);
        assert!(adif_record(&good).contains("<FREQ:9>14.090500"));
    }

    #[test]
    fn upload_state_round_trips_through_adif() {
        let mut r = rec("W1AW", "20m", 1_700_000_000);
        r.upload.lotw = Some(UploadStatus {
            outcome: UploadOutcome::Rejected,
            when_unix: 1_700_000_500,
            detail: Some("bad record | line 3".into()), // detail with an embedded '|'
        });
        let adif = adif_header() + &adif_record(&r);
        let back = parse_adif(&adif);
        assert_eq!(back.len(), 1);
        let u = back[0]
            .upload
            .lotw
            .as_ref()
            .expect("lotw upload state survived");
        assert_eq!(u.outcome, UploadOutcome::Rejected);
        assert_eq!(u.when_unix, 1_700_000_500);
        assert_eq!(u.detail.as_deref(), Some("bad record | line 3")); // splitn(3) kept the '|'
        assert!(back[0].upload.eqsl.is_none());
    }

    #[test]
    fn worked_before_any_and_per_band() {
        let mut lb = Logbook::new();
        lb.add(rec("W9XYZ", "20m", 1_700_000_000));
        assert!(lb.worked_before("w9xyz")); // case-insensitive
        assert!(lb.worked_before_band("W9XYZ", "20m"));
        assert!(!lb.worked_before_band("W9XYZ", "40m"));
        assert!(!lb.worked_before("N0ABC"));
    }

    #[test]
    fn adif_round_trips() {
        let mut lb = Logbook::new();
        lb.add(rec("W9XYZ", "20m", 1_700_000_000));
        lb.add(rec("K2DEF", "40m", 1_700_003_600));
        let text = lb.adif();
        assert!(text.contains("<EOH>") && text.contains("<CALL:5>W9XYZ"));
        let back = parse_adif(&text);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].call, "W9XYZ");
        assert_eq!(back[0].band, "20m");
        assert_eq!(back[0].rst_rcvd.as_deref(), Some("-12"));
        assert!((back[0].freq_mhz - 14.0905).abs() < 1e-6);
        // time round-trips to the same unix second
        assert_eq!(back[0].when_unix, 1_700_000_000);
    }

    #[test]
    fn time_off_round_trips_through_adif() {
        // A record with a distinct end time emits TIME_OFF/QSO_DATE_OFF and parses back.
        let mut r = rec("W9XYZ", "20m", 1_700_000_000);
        r.time_off_unix = Some(1_700_000_075); // ~75 s later (the contact's end)
        let mut lb = Logbook::new();
        lb.add(r);
        let text = lb.adif();
        assert!(
            text.contains("TIME_OFF") && text.contains("QSO_DATE_OFF"),
            "emits TIME_OFF + QSO_DATE_OFF"
        );
        let back = parse_adif(&text);
        assert_eq!(back[0].when_unix, 1_700_000_000, "TIME_ON = start");
        assert_eq!(
            back[0].time_off_unix,
            Some(1_700_000_075),
            "TIME_OFF = end, round-trips to the same second"
        );

        // A record with no end time omits the fields and parses back None.
        let mut lb2 = Logbook::new();
        lb2.add(rec("K2DEF", "40m", 1_700_000_000));
        let back2 = parse_adif(&lb2.adif());
        assert_eq!(
            back2[0].time_off_unix, None,
            "no end time → no TIME_OFF emitted"
        );
    }

    #[test]
    fn confirmation_is_source_aware() {
        // eQSL is NOT award-eligible: confirmed=true but award_confirmed=false
        // (the bug fix — an eQSL-only QSO must NOT count toward DXCC/Challenge).
        let eqsl = "<EOH>\n<CALL:5>K2DEF<BAND:3>40m<MODE:3>FT8<EQSL_QSL_RCVD:1>Y<EOR>\n";
        let e = &parse_adif(eqsl)[0];
        assert!(e.confirmed, "eQSL is a confirmation...");
        assert!(!e.award_confirmed, "...but eQSL is NOT award-eligible");

        // LoTW and paper QSL both count toward awards.
        let lotw = "<EOH>\n<CALL:5>K2DEF<BAND:3>40m<MODE:3>FT8<LOTW_QSL_RCVD:1>Y<EOR>\n";
        assert!(
            parse_adif(lotw)[0].award_confirmed,
            "LoTW is award-eligible"
        );
        let paper = "<EOH>\n<CALL:5>K2DEF<BAND:3>40m<MODE:3>FT8<QSL_RCVD:1>Y<EOR>\n";
        assert!(
            parse_adif(paper)[0].award_confirmed,
            "paper QSL is award-eligible"
        );

        // Unconfirmed by default.
        let n = rec("N0ABC", "20m", 1_700_000_000);
        assert!(!n.confirmed && !n.award_confirmed);
    }

    #[test]
    fn qrz_native_confirmation_is_not_award_eligible() {
        // A QRZ Logbook FETCH marks a native match in APP_QRZLOG_STATUS=C. It must land
        // `confirmed` (both ops logged it) but NOT `award_confirmed` — and critically it
        // must NOT promote the paper `card` channel (which would wrongly earn DXCC/WAS).
        let qrz = "<EOH>\n<CALL:5>K2DEF<BAND:3>40m<MODE:3>FT8<APP_QRZLOG_STATUS:1>C<EOR>\n";
        let q = &parse_adif(qrz)[0];
        assert!(q.confirmed, "QRZ native match is a confirmation...");
        assert!(!q.award_confirmed, "...but QRZ is NOT award-eligible");
        assert!(q.qsl_rcvd.qrz, "the QRZ channel is set");
        assert!(
            !q.qsl_rcvd.card,
            "QRZ status must NOT promote the paper card channel"
        );
        assert!(!q.qsl_rcvd.lotw && !q.qsl_rcvd.eqsl);

        // It round-trips back to the QRZ channel (APP_-namespaced), not QSL_RCVD.
        let mut lb = Logbook::new();
        lb.add(q.clone());
        let text = lb.adif();
        assert!(text.contains("<APP_QRZLOG_STATUS:1>C"));
        assert!(
            !text.contains("<QSL_RCVD:"),
            "must not masquerade as a paper QSL"
        );
        let back = &parse_adif(&text)[0];
        assert!(back.qsl_rcvd.qrz && back.confirmed && !back.award_confirmed);
    }

    #[test]
    fn award_confirmation_round_trips() {
        // An award-confirmed (LoTW/paper) record re-emits a LoTW field and
        // parses back award-eligible; an eQSL-only one round-trips as eQSL.
        let mut r = rec("W9XYZ", "20m", 1_700_000_000);
        r.confirmed = true;
        r.award_confirmed = true;
        let mut lb = Logbook::new();
        lb.add(r);
        let text = lb.adif();
        assert!(text.contains("<LOTW_QSL_RCVD:1>Y"));
        let back = parse_adif(&text);
        assert!(back[0].confirmed && back[0].award_confirmed);

        // eQSL-only record → emits eQSL → round-trips confirmed but not award.
        let mut e = rec("K2DEF", "40m", 1_700_003_600);
        e.confirmed = true; // award_confirmed stays false
        let mut lb2 = Logbook::new();
        lb2.add(e);
        let t2 = lb2.adif();
        assert!(t2.contains("<EQSL_QSL_RCVD:1>Y"));
        let b2 = parse_adif(&t2);
        assert!(b2[0].confirmed && !b2[0].award_confirmed);
    }

    #[test]
    fn country_round_trips_through_adif() {
        // Parses COUNTRY; serialize re-emits it; re-parse preserves.
        let recs =
            parse_adif("<EOH>\n<CALL:6>DL1XYZ<BAND:3>20m<MODE:3>FT8<COUNTRY:7>Germany<EOR>\n");
        assert_eq!(recs[0].country.as_deref(), Some("Germany"));
        let mut lb = Logbook::new();
        lb.add(recs[0].clone());
        let text = lb.adif();
        assert!(
            text.contains("<COUNTRY:7>Germany"),
            "emits the country field"
        );
        assert_eq!(parse_adif(&text)[0].country.as_deref(), Some("Germany"));
        // No COUNTRY → no field emitted.
        let none = rec("K2DEF", "40m", 1_700_000_000);
        let mut lb2 = Logbook::new();
        lb2.add(none);
        assert!(!lb2.adif().contains("<COUNTRY"));
    }

    #[test]
    fn state_parses_uppercased_and_round_trips() {
        // Parse uppercases + trims; serialize re-emits <STATE>; re-parse preserves.
        let recs = parse_adif("<EOH>\n<CALL:5>W9XYZ<BAND:3>20m<MODE:3>FT8<STATE:2>ny<EOR>\n");
        assert_eq!(recs[0].state.as_deref(), Some("NY"));
        let mut lb = Logbook::new();
        lb.add(recs[0].clone());
        let text = lb.adif();
        assert!(text.contains("<STATE:2>NY"), "emits the state field");
        assert_eq!(parse_adif(&text)[0].state.as_deref(), Some("NY"));
        // No STATE → no field emitted, parses back None.
        let none = rec("K2DEF", "40m", 1_700_000_000);
        let mut lb2 = Logbook::new();
        lb2.add(none);
        assert!(!lb2.adif().contains("<STATE"));
    }

    #[test]
    fn adif_parser_is_panic_and_dos_safe() {
        // A2: a field length near usize::MAX must not overflow `i + len` (would panic in
        // debug / wrap into an infinite loop in release). Must simply terminate.
        let overflow = "<CALL:4>TEST<NOTE:18446744073709551615>x<EOR>";
        let _ = parse_adif(overflow);

        // A1: a multibyte char straddling a fixed TIME_ON byte offset must not panic.
        // "0é12345" is 8 bytes; the old t[0..2] slice cut through 'é' → panic.
        let multibyte = "<CALL:4>TEST<QSO_DATE:8>20240704<TIME_ON:8>0é12345<EOR>";
        let recs = parse_adif(multibyte);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].call, "TEST");

        // A1b: the same multibyte guard on the OFF pair — TIME_OFF/QSO_DATE_OFF parse the
        // same fixed offsets and are fed by the same untrusted inlets (file import, LoTW/
        // eQSL/QRZ merges, and a WSJT-X LoggedADIF datagram inside the radio loop).
        let off_time = "<CALL:4>TEST<QSO_DATE:8>20240704<TIME_ON:6>131500<TIME_OFF:7>1é3456<EOR>";
        let recs = parse_adif(off_time);
        assert_eq!(recs.len(), 1);
        let off_date = "<CALL:4>TEST<QSO_DATE:8>20240704<TIME_ON:6>131500\
                        <TIME_OFF:6>131622<QSO_DATE_OFF:9>202é0701<EOR>";
        let recs = parse_adif(off_date);
        assert_eq!(recs.len(), 1);

        // Regression: a normal record still parses cleanly.
        let ok = "<CALL:6>KD9TAW<QSO_DATE:8>20240704<TIME_ON:6>131500<BAND:3>20M<MODE:3>FT8<EOR>";
        let recs = parse_adif(ok);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].call, "KD9TAW");
    }

    #[test]
    fn adif_parser_survives_arbitrary_field_soup() {
        // Deterministic pseudo-fuzz (xorshift, fixed seed — reproducible, no dependency):
        // ADIF-shaped records whose values mix digits, multibyte chars and lying length
        // prefixes. A lying prefix hands later fields bytes from the middle of earlier
        // ones — exactly how a fixed-offset field picks up arbitrary UTF-8. Every parse
        // must degrade, never panic: this input is reachable from a hand-edited file or
        // an inbound LoggedADIF datagram.
        let mut s = 0x243F_6A88_85A3_08D3u64;
        let mut rng = move |m: usize| -> usize {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % m as u64) as usize
        };
        const CHARS: [&str; 12] = ["0", "1", "9", "é", "°", "☃", "z", "-", ".", " ", "<", ">"];
        const FIELDS: [&str; 9] = [
            "CALL",
            "QSO_DATE",
            "TIME_ON",
            "TIME_OFF",
            "QSO_DATE_OFF",
            "MODE",
            "BAND",
            "GRIDSQUARE",
            "FREQ",
        ];
        for _ in 0..2000 {
            let mut rec = String::new();
            for _ in 0..1 + rng(6) {
                let name = FIELDS[rng(FIELDS.len())];
                let mut val = String::new();
                for _ in 0..rng(12) {
                    val.push_str(CHARS[rng(CHARS.len())]);
                }
                let lie = (val.len() + rng(5)).saturating_sub(2);
                rec.push_str(&format!("<{name}:{lie}>{val}"));
            }
            rec.push_str("<EOR>");
            let _ = parse_adif(&rec);
        }
    }

    #[test]
    fn credit_fields_parse_and_round_trip() {
        // CREDIT_GRANTED with :source annotations → award codes only, normalized.
        let adif = "<EOH>\n<CALL:5>K2DEF<BAND:3>20m<MODE:3>FT8<LOTW_QSL_RCVD:1>Y\
                    <CREDIT_GRANTED:23>DXCC:lotw,WAS:card&lotw<CREDIT_SUBMITTED:4>IOTA<EOR>\n";
        let recs = parse_adif(adif);
        assert_eq!(
            recs[0].credit_granted,
            vec!["DXCC".to_string(), "WAS".to_string()]
        );
        assert_eq!(recs[0].credit_submitted, vec!["IOTA".to_string()]);
        // round-trips through serialize → parse.
        let mut lb = Logbook::new();
        lb.add(recs[0].clone());
        let back = parse_adif(&lb.adif());
        assert_eq!(
            back[0].credit_granted,
            vec!["DXCC".to_string(), "WAS".to_string()]
        );
        assert_eq!(back[0].credit_submitted, vec!["IOTA".to_string()]);
    }

    #[test]
    fn merge_report_upgrades_existing_qso_and_flags_orphan() {
        // The regression "clean sync" fixes: a report confirming an ALREADY-logged
        // QSO must upgrade it (plain dedup-import would skip and lose it).
        let mut lb = Logbook::new();
        lb.add(rec("W1AW", "20m", 1_700_000_000)); // logged, unconfirmed
        assert!(!lb.records()[0].award_confirmed);

        let (y, mo, d, ..) = datetime_utc(1_700_000_000);
        let date = format!("{y:04}{mo:02}{d:02}");
        // Report: confirms W1AW (submode differs MFSK→Digital) + DXCC credit, plus
        // a confirmation for a never-logged call.
        let report = format!(
            "<EOH>\n<CALL:4>W1AW<BAND:3>20m<MODE:4>MFSK<QSO_DATE:8>{date}<LOTW_QSL_RCVD:1>Y\
             <CREDIT_GRANTED:4>DXCC<EOR>\n\
             <CALL:5>K9ZZZ<BAND:3>40m<MODE:2>CW<QSO_DATE:8>{date}<LOTW_QSL_RCVD:1>Y<EOR>\n"
        );
        let s = lb.merge_report(&report);
        assert_eq!(s.newly_confirmed, 1);
        assert_eq!(s.newly_credited, 1);
        assert!(lb.records()[0].award_confirmed);
        assert_eq!(lb.records()[0].credit_granted, vec!["DXCC".to_string()]);
        assert_eq!(s.orphans.len(), 1, "K9ZZZ has no logged QSO");
        assert!(s.orphans[0].reason.contains("K9ZZZ"));
    }

    #[test]
    fn csv_has_header_and_quotes() {
        let mut lb = Logbook::new();
        lb.add(rec("W9XYZ", "20m", 1_700_000_000));
        let csv = lb.csv();
        let mut lines = csv.lines();
        assert_eq!(
            lines.next().unwrap(),
            "Call,Grid,Band,Freq_MHz,Mode,RST_Sent,RST_Rcvd,Name,QTH,Comment,DateTimeUTC,Confirmed"
        );
        let row = lines.next().unwrap();
        assert!(
            row.starts_with("W9XYZ,EN37,20m,14.090500,TempoFast,-10,-12,,,,2023-11-14T22:13:20Z,N")
        );
    }

    #[test]
    fn multimode_report_and_notes_round_trip_through_adif() {
        let mut r = rec("K2DEF", "20m", 1_700_000_000);
        r.mode = "SSB".into();
        r.rst_sent = Some("59".into()); // phone RS
        r.rst_rcvd = Some("599".into()); // (a CW-style RST, proving free strings)
        r.name = Some("Jim".into());
        r.qth = Some("Dayton, OH".into());
        r.comment = Some("nice signal".into());
        r.notes = Some("IC-7300, 100W, G5RV — talked antennas".into());
        r.tx_power = Some(100.0);
        let back = parse_adif(&(adif_header() + &adif_record(&r)));
        assert_eq!(back.len(), 1);
        let b = &back[0];
        assert_eq!(b.rst_sent.as_deref(), Some("59"));
        assert_eq!(b.rst_rcvd.as_deref(), Some("599"));
        assert_eq!(b.name.as_deref(), Some("Jim"));
        assert_eq!(b.qth.as_deref(), Some("Dayton, OH"));
        assert_eq!(b.comment.as_deref(), Some("nice signal"));
        assert_eq!(
            b.notes.as_deref(),
            Some("IC-7300, 100W, G5RV — talked antennas")
        );
        assert_eq!(b.tx_power, Some(100.0));
    }

    #[test]
    fn import_merges_dedups_and_reads_confirmations() {
        let mut lb = Logbook::new();
        let adif = "<EOH>\n\
            <CALL:5>C91RU<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20250101<EOR>\n\
            <CALL:5>JA1XX<BAND:3>40m<MODE:2>CW<QSO_DATE:8>20250101<LOTW_QSL_RCVD:1>Y<EOR>\n";
        let (added, skipped, _) = lb.import_adif(adif);
        assert_eq!(added.len(), 2);
        assert_eq!(skipped, 0);
        assert_eq!(lb.len(), 2);
        assert!(lb.worked_before("C91RU"));
        // JA1XX came in confirmed via LoTW → award-eligible.
        assert!(lb
            .records()
            .iter()
            .any(|r| r.call == "JA1XX" && r.confirmed && r.award_confirmed));

        // Re-importing the same text adds nothing (all dupes).
        let (added2, skipped2, _) = lb.import_adif(adif);
        assert_eq!(added2.len(), 0);
        assert_eq!(skipped2, 2);
        assert_eq!(lb.len(), 2);

        // A NEW band for an existing call is a distinct slot → imported.
        let more = "<EOH>\n<CALL:5>C91RU<BAND:3>40m<MODE:3>FT8<QSO_DATE:8>20250102<EOR>\n";
        let (added3, ..) = lb.import_adif(more);
        assert_eq!(added3.len(), 1);
        assert!(lb.worked_before_band("C91RU", "40m"));
    }

    #[test]
    fn date_conversion_is_correct() {
        // 2023-11-14 22:13:20 UTC = 1_700_000_000
        assert_eq!(datetime_utc(1_700_000_000), (2023, 11, 14, 22, 13, 20));
        assert_eq!(unix_from_ymdhms(2023, 11, 14, 22, 13, 20), 1_700_000_000);
        // epoch
        assert_eq!(datetime_utc(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn per_source_qsl_round_trips_faithfully() {
        // THE regression: a paper-card confirmation must survive a save/load
        // cycle AS a card — the old writer re-emitted it as LOTW_QSL_RCVD.
        let card = "<EOH>\n<CALL:5>K2DEF<BAND:3>40m<MODE:3>FT8<QSL_RCVD:1>Y<EOR>\n";
        let mut lb = Logbook::new();
        lb.import_adif(card);
        let r = &lb.records()[0];
        assert!(r.qsl_rcvd.card && !r.qsl_rcvd.lotw && !r.qsl_rcvd.eqsl);
        assert!(r.award_confirmed && r.confirmed);
        let out = lb.adif();
        assert!(out.contains("<QSL_RCVD:1>Y"), "card stays a card: {out}");
        assert!(
            !out.contains("LOTW_QSL_RCVD"),
            "never rewritten as LoTW: {out}"
        );

        // Multi-channel: LoTW + eQSL both emit; no card is fabricated.
        let both =
            "<EOH>\n<CALL:5>K2DEF<BAND:3>40m<MODE:3>FT8<LOTW_QSL_RCVD:1>Y<EQSL_QSL_RCVD:1>Y<EOR>\n";
        let mut lb2 = Logbook::new();
        lb2.import_adif(both);
        let out2 = lb2.adif();
        assert!(out2.contains("<LOTW_QSL_RCVD:1>Y") && out2.contains("<EQSL_QSL_RCVD:1>Y"));
        assert!(
            !out2.contains("<QSL_RCVD:1>Y"),
            "no fabricated card: {out2}"
        );
    }

    #[test]
    fn legacy_bools_without_sources_keep_the_old_emission() {
        // A record whose sync predates the per-source split (bools set, sources
        // empty) must round-trip exactly as before until a sync refreshes it.
        let mut r = rec("K2DEF", "40m", 1_700_000_000);
        r.confirmed = true;
        r.award_confirmed = true;
        let mut lb = Logbook::new();
        lb.add(r);
        let out = lb.adif();
        assert!(
            out.contains("<LOTW_QSL_RCVD:1>Y"),
            "legacy best-guess kept: {out}"
        );
    }

    // ---- LoTW download → confirmations (operator report, 2026-08-05) --------
    //
    // The fixtures below are shaped like LoTW's ACTUAL output, not like this
    // parser's expectations — that inversion is how the bug shipped. Layout and
    // field set follow ARRL's published response table
    // (lotw.arrl.org/lotw-help/developer-query-qsos-qsls): one field per line,
    // the `APP_LoTW_*` fields interleaved, UPPERCASE band labels, lowercase
    // `<eor>`, an `<APP_LoTW_EOF>` trailer — and, per that table, `QSL_RCVD` is
    // "Y" when LoTW matched the QSO and "N" when it did not, with the QSL-detail
    // block (DXCC / COUNTRY / STATE / GRIDSQUARE) present ONLY on a matched row.

    /// One ADIF field as LoTW writes it. The declared length is COMPUTED: a
    /// fixture that miscounts it desyncs the scan and would "prove" whatever the
    /// parser happened to do with the wreckage.
    fn fld(name: &str, value: &str) -> String {
        format!("<{name}:{}>{value}\n", value.len())
    }

    /// One `lotwreport.adi` QSO record.
    fn lotw_row(call: &str, band: &str, mode: &str, date: &str, st: &str, matched: bool) -> String {
        let mut s = String::new();
        s.push_str(&fld("STATION_CALLSIGN", "NT9E"));
        s.push_str(&fld("CALL", call));
        s.push_str(&fld("BAND", band));
        s.push_str(&fld("MODE", mode));
        s.push_str(&fld("APP_LoTW_MODEGROUP", "DATA"));
        s.push_str(&fld("QSO_DATE", date));
        s.push_str(&fld("TIME_ON", "010203"));
        s.push_str(&fld("APP_LoTW_RXQSO", "2024-02-01 00:00:00"));
        if matched {
            s.push_str(&fld("QSL_RCVD", "Y"));
            s.push_str(&fld("QSLRDATE", "20240210"));
            s.push_str(&fld("APP_LoTW_RXQSL", "2024-02-10 11:22:33"));
            // ADIF CreditList: each credit may carry a `:QSLMedium` qualifier.
            s.push_str(&fld("CREDIT_GRANTED", "DXCC:LOTW,DXCC_BAND:LOTW"));
            s.push_str(&fld("COUNTRY", "UNITED STATES OF AMERICA"));
            s.push_str(&fld("GRIDSQUARE", "FN31PR"));
            if !st.is_empty() {
                s.push_str(&fld("STATE", st));
            }
        } else {
            s.push_str(&fld("QSL_RCVD", "N"));
        }
        s.push_str("<eor>\n");
        s
    }

    /// (call, LoTW band label, mode, date, state, LoTW matched it)
    const LOTW_FIXTURE: [(&str, &str, &str, &str, &str, bool); 4] = [
        ("W1AW", "20M", "FT8", "20240101", "CT", true),
        ("K5XYZ", "40M", "SSB", "20240102", "TX", true),
        ("DL1ABC", "160M", "CW", "20240103", "", true),
        ("VK3AAA", "30M", "FT8", "20240104", "", false),
    ];

    fn lotw_download() -> String {
        let mut s = String::from("ARRL Logbook of the World Status Report\nfor nt9e\n");
        s.push_str(&fld("PROGRAMID", "LoTW"));
        s.push_str(&fld("APP_LoTW_LASTQSL", "2026-08-04 21:12:44"));
        s.push_str("<eoh>\n\n");
        for (c, b, m, d, st, ok) in LOTW_FIXTURE {
            s.push_str(&lotw_row(c, b, m, d, st, ok));
        }
        s.push_str("<APP_LoTW_EOF>\n");
        s
    }

    /// The same contacts as a third-party master log holds them: no QSL fields at
    /// all, lowercase band, same call/mode/second (this log is what was uploaded).
    fn master_log() -> String {
        let mut s = String::from("Some Other Logger\n<EOH>\n");
        for (c, b, m, d, _, _) in LOTW_FIXTURE {
            s.push_str(&fld("CALL", c));
            s.push_str(&fld("BAND", &b.to_ascii_lowercase()));
            s.push_str(&fld("MODE", m));
            s.push_str(&fld("QSO_DATE", d));
            s.push_str(&fld("TIME_ON", "010203"));
            s.push_str("<eor>\n");
        }
        s
    }

    /// ⭐ THE OPERATOR'S REPORT (2026-08-05): after a purge and a re-download from
    /// LoTW his QSO count is right and his confirmations are not — "816 of 26007".
    /// This is that shape at four records: the master log first, then the LoTW
    /// download through the SAME "Import ADIF" path. Every LoTW row deduped
    /// against the QSO already logged and was discarded WHOLE — confirmation,
    /// `CREDIT_GRANTED`, and the STATE/COUNTRY that only a matched row carries.
    /// A dupe means "don't add a second contact", never "throw away what this row
    /// knows about the one we have".
    #[test]
    fn a_lotw_download_confirms_the_qsos_already_logged() {
        let mut lb = Logbook::new();
        let (added, skipped, _) = lb.import_adif(&master_log());
        assert_eq!((added.len(), skipped), (4, 0), "the master log imports");

        let (added2, skipped2, merged) = lb.import_adif(&lotw_download());
        assert_eq!(added2.len(), 0, "LoTW re-states contacts we already hold");
        assert_eq!(skipped2, 4, "…so none is added");
        assert_eq!(merged, 3, "the three MATCHED rows upgrade what we hold");
        assert_eq!(lb.len(), 4, "and the QSO count is untouched");

        let by = |call: &str| {
            lb.records()
                .iter()
                .find(|r| r.call == call)
                .unwrap_or_else(|| panic!("{call} missing"))
        };
        // The three LoTW matched it: award-eligible, credited, and enriched with
        // the detail fields the confirmation row carries.
        for call in ["W1AW", "K5XYZ", "DL1ABC"] {
            let r = by(call);
            assert!(r.confirmed && r.award_confirmed, "{call} confirmed");
            assert_eq!(r.credit_granted, vec!["DXCC", "DXCC_BAND"], "{call} credit");
            assert_eq!(r.country.as_deref(), Some("UNITED STATES OF AMERICA"));
        }
        assert_eq!(by("W1AW").state.as_deref(), Some("CT"), "STATE drives WAS");
        assert_eq!(by("K5XYZ").state.as_deref(), Some("TX"));
        // …and the one LoTW has NOT matched stays unconfirmed. `QSL_RCVD:N` is a
        // statement, not an absence, and it must never read as a confirmation.
        let vk = by("VK3AAA");
        assert!(
            !vk.confirmed && !vk.award_confirmed,
            "QSL_RCVD=N is not a QSL"
        );
        assert!(vk.credit_granted.is_empty());
    }

    /// The over-correction guard. Everything here is a row that looks confirmation-
    /// adjacent and is NOT one; a merge that promotes any of them would hand the
    /// operator a DXCC application ARRL rejects, which is worse than under-counting.
    #[test]
    fn an_import_never_invents_a_confirmation() {
        let mut lb = Logbook::new();
        lb.import_adif(&master_log());
        // 1. An outbound QSL REQUEST (`QSL_SENT`) — a card we mailed, not one we hold.
        // 2. `QSL_RCVD:R` — requested. Not received.
        // 3. eQSL — a real confirmation, but never award-eligible.
        let sneaky = format!(
            "<EOH>\n{}{}{}",
            format_args!(
                "{}{}{}{}{}{}<eor>\n",
                fld("CALL", "W1AW"),
                fld("BAND", "20M"),
                fld("MODE", "FT8"),
                fld("QSO_DATE", "20240101"),
                fld("TIME_ON", "010203"),
                fld("QSL_SENT", "Y")
            ),
            format_args!(
                "{}{}{}{}{}{}<eor>\n",
                fld("CALL", "K5XYZ"),
                fld("BAND", "40M"),
                fld("MODE", "SSB"),
                fld("QSO_DATE", "20240102"),
                fld("TIME_ON", "010203"),
                fld("QSL_RCVD", "R")
            ),
            format_args!(
                "{}{}{}{}{}{}<eor>\n",
                fld("CALL", "DL1ABC"),
                fld("BAND", "160M"),
                fld("MODE", "CW"),
                fld("QSO_DATE", "20240103"),
                fld("TIME_ON", "010203"),
                fld("EQSL_QSL_RCVD", "Y")
            ),
        );
        let (added, _, merged) = lb.import_adif(&sneaky);
        assert_eq!(added.len(), 0);
        // Two rows change the record — the eQSL confirmation and the outbound
        // request, which is recorded AS a request. The `R` row changes nothing:
        // "I asked for a card" is not news about a card.
        assert_eq!(merged, 2);
        let by = |c: &str| lb.records().iter().find(|r| r.call == c).unwrap();
        assert!(!by("W1AW").confirmed, "a QSL we SENT is not a QSL we hold");
        assert!(
            by("W1AW").qsl_sent.sent,
            "…it is recorded as the request it is"
        );
        assert!(
            !by("K5XYZ").confirmed,
            "QSL_RCVD=R is requested, not received"
        );
        let dl = by("DL1ABC");
        assert!(dl.confirmed, "eQSL confirms the contact");
        assert!(!dl.award_confirmed, "…but never earns DXCC/WAZ/WAS credit");
    }

    /// A later import can only ever ADD. A LoTW pull taken before the partner
    /// uploaded says `QSL_RCVD:N` about a QSO a previous pull confirmed — and must
    /// leave it confirmed. (`reconcile` already guaranteed this for the sync path;
    /// the import path now shares it.)
    #[test]
    fn an_unconfirmed_row_never_clears_a_confirmation() {
        let mut lb = Logbook::new();
        lb.import_adif(&master_log());
        lb.import_adif(&lotw_download());
        assert!(lb.records().iter().filter(|r| r.award_confirmed).count() == 3);

        // The same download, but with LoTW having matched nothing.
        let mut stale = String::from("ARRL Logbook of the World Status Report\n<eoh>\n");
        for (c, b, m, d, st, _) in LOTW_FIXTURE {
            stale.push_str(&lotw_row(c, b, m, d, st, false));
        }
        let (_, _, merged) = lb.import_adif(&stale);
        assert_eq!(merged, 0, "nothing to add — and nothing to take away");
        assert_eq!(
            lb.records().iter().filter(|r| r.award_confirmed).count(),
            3,
            "confirmations are monotonic"
        );
        assert_eq!(lb.records()[0].credit_granted, vec!["DXCC", "DXCC_BAND"]);
    }

    /// ADIF's `QSL_Rcvd` enumeration has THREE confirmation-bearing spellings, not
    /// one: "Y", and "V" — verified, i.e. an award credit was granted against it —
    /// which Club Log and DXKeeper both write into the logs they export. LoTW's own
    /// report emits only Y/N (ARRL's field table), so this is not what the operator
    /// hit; it is the same class of miss, in the same predicate, for anyone whose
    /// master log came from one of those. A padded value must not read as "no"
    /// either.
    #[test]
    fn a_verified_qsl_is_a_confirmation_and_a_padded_one_still_parses() {
        let mut lb = Logbook::new();
        let adif = "<EOH>\n\
            <CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20240101<QSL_RCVD:1>V<EOR>\n\
            <CALL:5>K5XYZ<BAND:3>40m<MODE:3>SSB<QSO_DATE:8>20240102<LOTW_QSL_RCVD:1>V<EOR>\n\
            <CALL:6>DL1ABC<BAND:4>160m<MODE:2>CW<QSO_DATE:8>20240103<QSL_RCVD:2>Y <EOR>\n\
            <CALL:6>JA1XYZ<BAND:3>15m<MODE:3>FT8<QSO_DATE:8>20240104<QSL_RCVD:1>I<EOR>\n";
        lb.import_adif(adif);
        let by = |c: &str| lb.records().iter().find(|r| r.call == c).unwrap();
        assert!(by("W1AW").award_confirmed, "QSL_RCVD=V is a confirmation");
        assert!(by("K5XYZ").qsl_rcvd.lotw, "LOTW_QSL_RCVD=V likewise");
        assert!(by("DL1ABC").award_confirmed, "a padded Y is still a Y");
        assert!(!by("JA1XYZ").confirmed, "I = ignore/invalid is not a QSL");
    }
}

#[cfg(test)]
mod operator_split_tests {
    use super::*;

    fn rec(call: &str, operator: Option<&str>) -> QsoRecord {
        QsoRecord {
            call: call.into(),
            grid: None,
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
            operator: operator.map(|o| o.to_string()),
            station_callsign: None,
            extra: Vec::new(),
        }
    }

    /// #25: POTA and Field Day both want each operator to submit their own log. Before the
    /// operator was stamped there was nothing to split on.
    #[test]
    fn operators_lists_each_distinct_operator_once() {
        let lb = Logbook {
            records: vec![
                rec("W9AAA", Some("W1ABC")),
                rec("W9BBB", Some("G0PQR")),
                rec("W9CCC", Some("W1ABC")),
            ],
        };
        assert_eq!(
            lb.operators(),
            vec!["G0PQR".to_string(), "W1ABC".to_string()]
        );
    }

    /// The single-op case, and the one that must NOT grow a bucket: an unstamped contact belongs
    /// to nobody in particular, and naming a bucket after the station would claim the operator
    /// said something they never did.
    #[test]
    fn operators_invents_no_bucket_for_unstamped_contacts() {
        let lb = Logbook {
            records: vec![rec("W9AAA", None), rec("W9BBB", Some("  "))],
        };
        assert!(lb.operators().is_empty());
    }

    #[test]
    fn an_operators_export_carries_only_their_contacts() {
        let lb = Logbook {
            records: vec![
                rec("W9AAA", Some("W1ABC")),
                rec("W9BBB", Some("G0PQR")),
                rec("W9CCC", None),
            ],
        };
        let out = lb.adif_for_operator("W1ABC");
        assert!(out.contains("W9AAA"), "their own contact is missing");
        assert!(
            !out.contains("W9BBB"),
            "another operator's contact leaked into this export"
        );
        assert!(
            !out.contains("W9CCC"),
            "an unstamped contact was attributed to someone"
        );
    }

    /// Typed by a human, mid-activation, on a phone in a car park.
    #[test]
    fn matching_an_operator_ignores_case_and_stray_spaces() {
        let lb = Logbook {
            records: vec![rec("W9AAA", Some(" w1abc "))],
        };
        assert!(lb.adif_for_operator("W1ABC").contains("W9AAA"));
        assert_eq!(lb.operators(), vec!["W1ABC".to_string()]);
    }

    /// An operator with nothing logged gets a valid EMPTY file. The failure this forbids is
    /// falling back to the whole log, which would upload one operator's contacts under another's
    /// name — worse than an empty file, and silent.
    #[test]
    fn an_operator_with_no_contacts_gets_an_empty_file_not_everyone_elses() {
        let lb = Logbook {
            records: vec![rec("W9AAA", Some("W1ABC"))],
        };
        let out = lb.adif_for_operator("K9NOBODY");
        assert!(!out.contains("W9AAA"));
        assert!(
            out.contains("<EOH>"),
            "still a valid ADIF file, just an empty one"
        );
    }
}

#[cfg(test)]
mod qsl_card_tests {
    use super::*;

    /// A minimal record. `QsoRecord` has no `Default`, and spelling every field here would bury
    /// what these tests are about — the two QSL fields.
    fn rec() -> QsoRecord {
        QsoRecord {
            call: "K1ABC".into(),
            grid: None,
            country: None,
            state: None,
            band: "20m".into(),
            freq_mhz: 14.074,
            mode: "FT8".into(),
            rst_sent: Some("-10".into()),
            rst_rcvd: Some("-12".into()),
            name: None,
            comment: None,
            notes: None,
            qth: None,
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
            station_callsign: None,
            extra: Vec::new(),
        }
    }

    /// #152 (rgoiko): "I can't edit the QSL status in the Logbook. There's no field where I can
    /// select, for example, whether I received the QSL card on paper."
    ///
    /// He was right, and it cost more than a checkbox. `QslRcvd::award` is card OR LoTW, so a
    /// paper card is one of only two things that make a contact countable for DXCC — and it was
    /// the one channel no service can report and the operator could not enter.
    #[test]
    fn an_operator_can_record_a_paper_card_and_it_counts_for_awards() {
        let mut lb = Logbook::default();
        lb.records.push(rec());
        assert!(!lb.records[0].qsl_rcvd.card, "starts unconfirmed");
        assert!(!lb.records[0].qsl_rcvd.award(), "and unclaimable");

        assert!(lb.mark_qsl_card(0, true));
        assert!(lb.records[0].qsl_rcvd.card);
        assert!(
            lb.records[0].qsl_rcvd.award(),
            "a card is award-eligible — that is the whole point of being able to enter it"
        );
        assert!(lb.records[0].qsl_rcvd.any());
    }

    /// A mis-tick must be correctable. This is the one confirmation channel with no service
    /// behind it, so if the operator cannot undo it, nothing can.
    #[test]
    fn a_mistaken_card_can_be_taken_back() {
        let mut lb = Logbook::default();
        lb.records.push(rec());
        assert!(lb.mark_qsl_card(0, true));
        assert!(lb.mark_qsl_card(0, false));
        assert!(!lb.records[0].qsl_rcvd.card);
        assert!(!lb.records[0].qsl_rcvd.award());
    }

    /// It must touch ONLY the card, and only the row asked for. A confirmation is the operator's
    /// award evidence; a write that reached further than the row they clicked would be silent
    /// corruption of exactly the data they cannot reconstruct.
    #[test]
    fn it_touches_only_the_card_field_of_only_that_row() {
        let mut lb = Logbook::default();
        let mut with_lotw = rec();
        with_lotw.qsl_rcvd.lotw = true;
        lb.records.push(with_lotw);
        lb.records.push(rec());

        assert!(lb.mark_qsl_card(0, true));
        assert!(
            lb.records[0].qsl_rcvd.lotw,
            "LoTW's own confirmation is untouched"
        );
        assert!(!lb.records[0].qsl_rcvd.eqsl);
        assert!(!lb.records[1].qsl_rcvd.card, "the other row is untouched");
        // A card is not a request: marking one received must not claim we sent one.
        assert!(!lb.records[0].qsl_sent.sent);
    }

    /// Out of range is false, not a panic — the UI holds indices that shift under it.
    #[test]
    fn an_index_that_is_gone_reports_false() {
        let mut lb = Logbook::default();
        assert!(!lb.mark_qsl_card(0, true));
        lb.records.push(rec());
        assert!(!lb.mark_qsl_card(7, true));
        assert!(
            lb.mark_qsl_card(0, true),
            "control: a real index still works"
        );
    }
}
