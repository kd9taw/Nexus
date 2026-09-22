//! Native QSO logging with explicit persistence completion. Callers own Remote
//! admission; these methods grant no radio or transmit authority. Storage syncs
//! run after releasing Engine. A failed append is never replayed automatically.

use super::{Engine, LogWriteOutcome};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tempo_core::logbook::QsoRecord;

// Opaque observation identities, never authority. Exhaustion disables Remote
// identity matching instead of reusing an old value; local behavior continues.
pub(super) fn next_identity() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .unwrap_or(u64::MAX)
}
fn identity_key(id: u64) -> Option<String> {
    (id != u64::MAX).then(|| format!("{id:016x}"))
}

/// Where a held contact's GRIDSQUARE came from — the one thing a [`QsoRecord`] cannot say
/// about itself, and the thing the confirm popup's edits turn on.
///
/// ⭐ R2 follow-up (operator review, 2026-09-19). [`Engine::dx_grid_resolved`] has three
/// sources and only the first is the station's own; the other two are LOOKUPS KEYED ON THE
/// CALLSIGN, which is the BUSTED call when the operator corrects one — so they are another
/// station's grid, exactly as COUNTRY and NAME are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GridSource {
    /// The station SENT it, in the message we decoded. Correcting a mis-copied call does
    /// not make it wrong: it is still the grid that station put on the air.
    Transmitted,
    /// The session roster, or a grid logged for that call before — both keyed on the call.
    ///
    /// The DEFAULT, and so what a journal written by an older build restores as: a grid we
    /// cannot vouch for is dropped on a corrected call rather than logged against a station
    /// that may never have sent it. A missing grid can be filled in later; a confidently
    /// wrong one is exported, uploaded and believed.
    #[default]
    LookedUp,
}

impl GridSource {
    /// Which source [`Engine::dx_grid_resolved`] takes for a contact whose exchange carried
    /// `dxgrid` — the ONE predicate it and the hold share, so they cannot drift apart.
    pub(crate) fn of(dxgrid: Option<&str>) -> Self {
        match dxgrid {
            Some(grid) if !grid.trim().is_empty() => Self::Transmitted,
            _ => Self::LookedUp,
        }
    }
}

/// A completed contact waiting for the operator's confirm: the record, and where its grid
/// came from ([`GridSource`]).
#[derive(Clone, Debug)]
pub(crate) struct HeldQso {
    pub(crate) record: QsoRecord,
    pub(crate) grid_source: GridSource,
}

impl HeldQso {
    pub(crate) fn new(record: QsoRecord, grid_source: GridSource) -> Self {
        Self {
            record,
            grid_source,
        }
    }
}

/// The existing journal DTO plus fields which the ordinary UI edit DTO does
/// not carry. Legacy journals remain readable. Nothing changes the log format.
///
/// `timeOffUnix` used to be one of those extras and is not any more — #329 put it on
/// [`LoggedQso`](crate::dto::LoggedQso) so the Logbook can show and edit an end time, and
/// a sidecar of the same camelCase name beside a `flatten` writes the key twice and makes
/// the whole journal unreadable. A journal written by an older build spells it identically,
/// so it now lands in the flattened record instead and nothing on disk changes.
///
/// `freqRxMhz` has now gone exactly the same way, for the same reason and with the same
/// result on disk: it is on the DTO so the Logbook can SHOW a split contact's receive leg,
/// and a sidecar of that name beside the `flatten` would emit the key twice — which costs
/// every held contact in the journal, not just its own field. It is still restored by hand
/// below, because the DTO deliberately carries it one way only.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingRecord {
    #[serde(flatten)]
    record: crate::dto::LoggedQso,
    /// Absent from a journal written before this build — and then [`GridSource::LookedUp`],
    /// which is the safe answer for a grid whose provenance nobody recorded.
    #[serde(default)]
    grid_source: GridSource,
}

fn pending_record(held: &HeldQso) -> PendingRecord {
    PendingRecord {
        record: held.record.clone().into(),
        grid_source: held.grid_source,
    }
}

impl PendingRecord {
    fn into_held(self) -> HeldQso {
        let grid_source = self.grid_source;
        // Read off the DTO before the conversion, which drops it: `LoggedQso` carries the
        // split receive leg OUTBOUND only, so that a display column cannot become a write
        // path through the edit form. A held contact is this type's own round trip, not an
        // operator edit, so it restores what it wrote.
        let freq_rx_mhz = self.record.freq_rx_mhz;
        let mut record: QsoRecord = self.record.into();
        record.freq_rx_mhz = freq_rx_mhz;
        HeldQso::new(record, grid_source)
    }
}

pub(super) fn pending_qso_json(
    record: &QsoRecord,
    grid_source: GridSource,
) -> serde_json::Result<String> {
    serde_json::to_string(&pending_record(&HeldQso::new(record.clone(), grid_source)))
}

/// The WHOLE confirm-before-log queue, oldest first — what the desktop journals on every hold
/// change. A journal written by an older build holds one record and not an array;
/// [`Engine::load_pending_qso_json`] reads both.
pub(super) fn pending_qso_queue_json(
    held: &std::collections::VecDeque<HeldQso>,
) -> serde_json::Result<String> {
    let queue: Vec<PendingRecord> = held.iter().map(pending_record).collect();
    serde_json::to_string(&queue)
}

/// An exact hold, including its incarnation. Replacing a hold with an identical
/// record still invalidates an earlier confirmation or discard.
#[derive(Clone, Debug)]
pub struct PendingLogIdentity {
    identity: Arc<()>,
    record: Arc<QsoRecord>,
    grid_source: GridSource,
    path: Option<PathBuf>,
    epoch: u64,
}

impl PendingLogIdentity {
    pub fn key(&self) -> Option<String> {
        identity_key(self.epoch)
    }
    pub fn record(&self) -> &QsoRecord {
        &self.record
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFailure {
    StalePending,
    InvalidEdits,
    AlreadyPresent,
    PersistenceUnconfirmed,
}

#[derive(Debug)]
pub enum CurrentQsoLogOutcome {
    NoEligibleContact,
    PendingExists,
    Append(LogWriteOutcome),
    Pending(PendingJournalWrite),
}

/// Only the four fields editable in the existing confirmation dialog. All
/// other fields come from the station's original QsoRecord, not a DTO round trip.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingLogEdits {
    pub call: String,
    pub grid: Option<String>,
    pub rst_sent: Option<String>,
    pub rst_rcvd: Option<String>,
}

impl PendingLogEdits {
    fn valid(&self) -> bool {
        (3..=32).contains(&self.call.len())
            && self
                .call
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/')
            && self
                .grid
                .as_ref()
                .is_none_or(|s| s.len() <= 16 && s.bytes().all(|b| b.is_ascii_alphanumeric()))
            && [&self.rst_sent, &self.rst_rcvd].into_iter().all(|s| {
                s.as_ref().is_none_or(|s| {
                    s.len() <= 16 && s.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
                })
            })
    }
}

/// File and directory persistence after an Engine-owned rename/removal. This
/// proves the specific filesystem operation, not later unchanged station state.
#[derive(Debug)]
pub struct JournalSync {
    parent: Option<File>,
}
impl JournalSync {
    pub fn sync(self) -> io::Result<()> {
        if let Some(parent) = self.parent {
            parent.sync_all()?;
        }
        Ok(())
    }
}

fn journal_parent(path: &std::path::Path) -> io::Result<Option<File>> {
    #[cfg(unix)]
    {
        File::open(
            path.parent()
                .ok_or_else(|| io::Error::other("journal parent unavailable"))?,
        )
        .map(Some)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(None)
    }
}

#[derive(Debug)]
pub struct PendingJournalWrite {
    pending: PendingLogIdentity,
}

/// A synced temporary journal. Dropping an unpublished preparation removes only
/// its own file. Publication must recheck the held contact under Engine.
#[derive(Debug)]
pub struct PreparedPendingJournal {
    pending: PendingLogIdentity,
    temporary: PathBuf,
    parent: Option<File>,
}
impl Drop for PreparedPendingJournal {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temporary);
    }
}

impl PendingJournalWrite {
    /// Call without holding Engine. No rename or change to the live journal yet.
    pub fn prepare(self) -> io::Result<PreparedPendingJournal> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let text = pending_qso_json(self.pending.record(), self.pending.grid_source)?;
        let path = self
            .pending
            .path
            .as_ref()
            .ok_or_else(|| io::Error::other("journal unavailable"))?;
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("journal parent unavailable"))?;
        std::fs::create_dir_all(parent)?;
        let temporary = path.with_extension(format!(
            "remote-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        let mut prepared = PreparedPendingJournal {
            pending: self.pending,
            temporary,
            parent: None,
        };
        let synced = file
            .write_all(text.as_bytes())
            .and_then(|()| file.sync_all());
        // Close before cleanup, including on write failure (Windows refuses
        // removal of an open file).
        drop(file);
        synced?;
        prepared.parent = journal_parent(prepared.pending.path.as_ref().expect("path checked"))?;
        Ok(prepared)
    }
}

#[derive(Debug)]
pub struct PendingLogConfirmation {
    pending: PendingLogIdentity,
    outcome: LogWriteOutcome,
}

/// Only a successful sync can construct this proof. A duplicate in memory is
/// not evidence of a durable append and cannot clear the held contact.
#[derive(Debug)]
pub struct ConfirmedPendingLog {
    pending: PendingLogIdentity,
}
impl PendingLogConfirmation {
    pub fn sync(self) -> Result<ConfirmedPendingLog, LogFailure> {
        match self.outcome {
            LogWriteOutcome::PendingSync(receipts) if !receipts.is_empty() => {
                receipts
                    .into_iter()
                    .try_for_each(|receipt| receipt.sync())
                    .map_err(|_| LogFailure::PersistenceUnconfirmed)?;
                Ok(ConfirmedPendingLog {
                    pending: self.pending,
                })
            }
            LogWriteOutcome::Duplicate => Err(LogFailure::AlreadyPresent),
            _ => Err(LogFailure::PersistenceUnconfirmed),
        }
    }
}

impl Engine {
    /// Install `held` as the hold the popup is showing, or (with `None`) drop that hold and
    /// promote whatever was waiting behind it. Either way the front's identity is fresh, so a
    /// confirm or discard issued against the old front is refused.
    pub(super) fn replace_pending_log(&mut self, held: Option<HeldQso>) {
        match held {
            Some(held) if self.pending_logs.is_empty() => self.pending_logs.push_back(held),
            Some(held) => self.pending_logs[0] = held,
            None => {
                self.pending_logs.pop_front();
                // ⭐ R3 — ANSWERING THE POPUP IS THE ACKNOWLEDGEMENT. Every confirm and every
                // discard, desktop and Remote, reaches this one arm, so the unreviewed count
                // the popup shows clears here and nowhere else. A count that never cleared
                // would leave the warning up forever after one cap event.
                self.pending_logs_auto_logged = 0;
            }
        }
        self.pending_log_identity = Arc::new(());
        self.pending_log_epoch = next_identity();
    }

    pub(super) fn reset_qso_log_identity(&mut self) {
        self.qso_logged = false;
        self.qso_log_epoch = next_identity();
    }

    pub fn current_qso_log_key(&self) -> Option<String> {
        if matches!(self.mode, super::Mode::Qso { .. }) {
            // The native operating generation also retires away-and-back QSY,
            // profile, offset and settings changes. A newer command window
            // cannot make an older displayed contact valid in that context.
            Some(format!(
                "{}{}",
                identity_key(self.qso_log_epoch)?,
                identity_key(self.remote_log_context_generation())?
            ))
        } else {
            None
        }
    }

    pub fn pending_qso_log_key(&self) -> Option<String> {
        self.pending_log()
            .and_then(|_| identity_key(self.pending_log_epoch))
    }

    pub fn pending_log_identity(&self) -> Option<PendingLogIdentity> {
        let held = self.pending_logs.front()?;
        Some(PendingLogIdentity {
            identity: self.pending_log_identity.clone(),
            record: Arc::new(held.record.clone()),
            grid_source: held.grid_source,
            path: self.station.pending_qso_path.clone(),
            epoch: self.pending_log_epoch,
        })
    }

    fn matches_pending_log(&self, pending: &PendingLogIdentity) -> bool {
        Arc::ptr_eq(&self.pending_log_identity, &pending.identity)
            && self.pending_log() == Some(pending.record())
            && self.station.pending_qso_path == pending.path
    }

    /// Restore the journal: this build's queue, or the single record older builds wrote.
    /// Each restored contact keeps the grid provenance the journal recorded — a journal from
    /// an older build records none, and those restore as [`GridSource::LookedUp`].
    pub fn load_pending_qso_json(&mut self, text: &str) {
        if let Ok(queue) = serde_json::from_str::<Vec<PendingRecord>>(text) {
            for pending in queue {
                self.hold_pending_log(pending.into_held());
            }
        } else if let Ok(pending) = serde_json::from_str::<PendingRecord>(text) {
            self.hold_pending_log(pending.into_held());
        }
    }

    /// The host must bind the displayed QSO and original controller context
    /// before calling. Eligibility and write-once behavior are shared with the
    /// local button. An existing confirmation is never replaced by this action.
    pub fn log_current_qso_for_sync(&mut self) -> CurrentQsoLogOutcome {
        if self.pending_log().is_some() {
            return CurrentQsoLogOutcome::PendingExists;
        }
        let Some(held) = self.take_current_qso_record() else {
            return CurrentQsoLogOutcome::NoEligibleContact;
        };
        if self.settings.prompt_to_log {
            self.replace_pending_log(Some(held));
            CurrentQsoLogOutcome::Pending(PendingJournalWrite {
                pending: self.pending_log_identity().expect("hold just installed"),
            })
        } else {
            CurrentQsoLogOutcome::Append(self.log_qso_for_sync(held.record))
        }
    }

    /// Publish only if no local confirmation, discard, replacement or path
    /// change occurred while the temporary file was being synced.
    pub fn publish_pending_qso_journal(
        &mut self,
        mut prepared: PreparedPendingJournal,
    ) -> Result<JournalSync, LogFailure> {
        if !self.matches_pending_log(&prepared.pending) {
            return Err(LogFailure::StalePending);
        }
        std::fs::rename(
            &prepared.temporary,
            prepared
                .pending
                .path
                .as_ref()
                .ok_or(LogFailure::PersistenceUnconfirmed)?,
        )
        .map_err(|_| LogFailure::PersistenceUnconfirmed)?;
        // The temporary file holds the ONE contact this publication prepared, which was the
        // whole queue when it started (`log_current_qso_for_sync` refuses while a hold exists).
        // A local completion can have joined the queue since — the rename would then leave those
        // out of the journal — so rewrite it from the live queue.
        if self.pending_logs_waiting() > 0 {
            self.persist_pending_qso();
        }
        Ok(JournalSync {
            parent: prepared.parent.take(),
        })
    }

    /// Begin the shared append without clearing the hold or its recovery file.
    /// Neither this step nor its sync requires transmit permission.
    pub fn confirm_pending_log_for_sync(
        &mut self,
        pending: PendingLogIdentity,
        edits: PendingLogEdits,
    ) -> Result<PendingLogConfirmation, LogFailure> {
        if !self.matches_pending_log(&pending) {
            return Err(LogFailure::StalePending);
        }
        if !edits.valid() {
            return Err(LogFailure::InvalidEdits);
        }
        let mut record = pending.record().clone();
        record.call = edits.call;
        record.grid = edits.grid;
        record.rst_sent = edits.rst_sent;
        record.rst_rcvd = edits.rst_rcvd;
        // The same R2 re-derivation the desktop confirm runs: a corrected call must not carry
        // the busted call's COUNTRY, NAME or looked-up GRID into the log, whichever client the
        // operator confirmed from.
        self.rederive_after_edits(&mut record, pending.record(), pending.grid_source);
        let outcome = self.log_qso_for_sync(record);
        Ok(PendingLogConfirmation { pending, outcome })
    }

    pub fn finish_pending_log_confirmation(
        &mut self,
        confirmed: ConfirmedPendingLog,
    ) -> Result<JournalSync, LogFailure> {
        self.discard_pending_log_for_sync(&confirmed.pending)
    }

    /// The host must authorize an explicit discard or hold a synced append
    /// proof. A failed deletion retains the in-memory contact for recovery.
    pub fn discard_pending_log_for_sync(
        &mut self,
        pending: &PendingLogIdentity,
    ) -> Result<JournalSync, LogFailure> {
        if !self.matches_pending_log(pending) {
            return Err(LogFailure::StalePending);
        }
        let path = pending
            .path
            .as_ref()
            .ok_or(LogFailure::PersistenceUnconfirmed)?;
        let parent = journal_parent(path).map_err(|_| LogFailure::PersistenceUnconfirmed)?;
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(LogFailure::PersistenceUnconfirmed),
        }
        self.replace_pending_log(None);
        Ok(JournalSync { parent })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::Tier;
    use crate::engine::tests::dec_snr;

    struct Fixture {
        dir: PathBuf,
        engine: Engine,
    }
    impl Fixture {
        fn new(tier: Tier, prompt: bool) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "nexus-remote-qso-log-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&dir).unwrap();
            let mut engine = Engine::new("K2DEF", "FN31", 0);
            engine.set_tier(tier);
            engine.settings.prompt_to_log = prompt;
            engine.set_log_path(dir.join("log.adi"));
            engine.set_pending_qso_path(dir.join("pending.json"));
            engine.call_station_with_grid("W9XYZ", Some("EN37"));
            engine.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
            engine.qso_start_unix = Some(1_700_000_000);
            Self { dir, engine }
        }
        fn hold(&mut self) -> PendingLogIdentity {
            let CurrentQsoLogOutcome::Pending(write) = self.engine.log_current_qso_for_sync()
            else {
                panic!("contact must be held")
            };
            let prepared = write.prepare().unwrap();
            self.engine
                .publish_pending_qso_journal(prepared)
                .unwrap()
                .sync()
                .unwrap();
            self.engine.pending_log_identity().unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
    fn edits(pending: &PendingLogIdentity) -> PendingLogEdits {
        PendingLogEdits {
            call: pending.record().call.clone(),
            grid: pending.record().grid.clone(),
            rst_sent: pending.record().rst_sent.clone(),
            rst_rcvd: pending.record().rst_rcvd.clone(),
        }
    }

    #[test]
    fn current_qso_append_matches_local_fields_and_remains_write_once() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let mut native = Fixture::new(tier, false);
            let mut remote = Fixture::new(tier, false);
            let settings = serde_json::to_value(remote.engine.settings()).unwrap();
            assert!(native.engine.log_current_qso());
            let CurrentQsoLogOutcome::Append(LogWriteOutcome::PendingSync(receipts)) =
                remote.engine.log_current_qso_for_sync()
            else {
                panic!("expected append receipts")
            };
            assert!(!receipts.is_empty());
            for receipt in receipts {
                receipt.sync().unwrap();
            }
            let mut expected = native.engine.station.logbook.records()[0].as_ref().clone();
            let actual = &remote.engine.station.logbook.records()[0];
            assert!(
                expected
                    .time_off_unix
                    .unwrap()
                    .abs_diff(actual.time_off_unix.unwrap())
                    <= 2
            );
            expected.time_off_unix = actual.time_off_unix;
            // Two engines, two logs, so two minters: the id is the row's identity in ITS log,
            // never a field of the contact, and is the one thing that must NOT match.
            assert_ne!(expected.id, actual.id);
            expected.id = actual.id;
            assert_eq!(&expected, actual.as_ref());
            assert_eq!(actual.when_unix, 1_700_000_000);
            assert!(actual.freq_mhz > remote.engine.settings.dial_mhz);
            assert_eq!(actual.rst_rcvd.as_deref(), Some("-10"));
            assert!(std::fs::read_to_string(remote.dir.join("log.adi"))
                .unwrap()
                .contains("W9XYZ"));
            assert_eq!(remote.engine.station.pending_uploads.len(), 1);
            assert!(matches!(
                remote.engine.log_current_qso_for_sync(),
                CurrentQsoLogOutcome::NoEligibleContact
            ));
            remote
                .engine
                .ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
            assert_eq!(remote.engine.station.logbook.records().len(), 1);
            assert_eq!(remote.engine.station.pending_uploads.len(), 1);
            assert_eq!(
                serde_json::to_value(remote.engine.settings()).unwrap(),
                settings
            );
        }
    }

    #[test]
    fn current_qso_requires_native_report_eligibility() {
        let mut engine = Engine::new("K2DEF", "FN31", 0);
        engine.call_station("W9XYZ");
        assert!(matches!(
            engine.log_current_qso_for_sync(),
            CurrentQsoLogOutcome::NoEligibleContact
        ));
        assert!(!engine.qso_logged);
        assert!(engine.station.logbook.records().is_empty());
        assert!(engine.station.pending_uploads.is_empty());
    }

    #[test]
    fn observation_keys_follow_contact_lifetimes_not_snapshot_polls() {
        let mut fixture = Fixture::new(Tier::Ft8, true);
        let current = fixture.engine.current_qso_log_key().unwrap();
        fixture.engine.snapshot();
        fixture.engine.qso_resend();
        assert_eq!(
            fixture.engine.current_qso_log_key().as_deref(),
            Some(current.as_str())
        );
        // Same-contact re-arm preserves contact identity, but the deliberate
        // local gesture supersedes an older Remote operating context.
        let contact_epoch = fixture.engine.qso_log_epoch;
        fixture.engine.call_station("W9XYZ");
        assert_eq!(fixture.engine.qso_log_epoch, contact_epoch);
        assert_ne!(
            fixture.engine.current_qso_log_key().as_deref(),
            Some(current.as_str())
        );
        let held = fixture.hold();
        assert_eq!(fixture.engine.pending_qso_log_key(), held.key());
        fixture.engine.replace_pending_log(Some(HeldQso::new(
            held.record().clone(),
            GridSource::LookedUp,
        )));
        assert_ne!(fixture.engine.pending_qso_log_key(), held.key());
        fixture.engine.call_station("W1AW");
        assert_ne!(
            fixture.engine.current_qso_log_key().as_deref(),
            Some(current.as_str())
        );
        fixture.engine.qso_log_epoch = u64::MAX;
        fixture.engine.pending_log_epoch = u64::MAX;
        assert!(fixture.engine.current_qso_log_key().is_none());
        assert!(fixture.engine.pending_qso_log_key().is_none());
        assert!(fixture.engine.pending_log().is_some());
    }

    #[test]
    fn current_log_key_retires_after_qsy_even_if_the_same_contact_and_frequency_return() {
        let mut fixture = Fixture::new(Tier::Ft8, false);
        let key = fixture.engine.current_qso_log_key().unwrap();
        let qso = fixture.engine.snapshot().qso;
        fixture.engine.set_frequency(7.074, "40m", "USB");
        fixture.engine.set_frequency(14.074, "20m", "USB");
        assert_eq!(fixture.engine.snapshot().qso, qso);
        assert_ne!(
            fixture.engine.current_qso_log_key().as_deref(),
            Some(key.as_str())
        );
    }

    #[test]
    fn held_record_keeps_split_and_end_time_through_journal_and_confirmation() {
        let mut fixture = Fixture::new(Tier::Ft8, true);
        let CurrentQsoLogOutcome::Pending(_) = fixture.engine.log_current_qso_for_sync() else {
            panic!("hold")
        };
        let mut original = fixture.engine.pending_log().cloned().unwrap();
        original.freq_rx_mhz = Some(14.0755);
        original.time_off_unix = Some(1_700_000_123);
        original.ota.their_program = Some("POTA".into());
        original.ota.their_ref = Some("K-1234".into());
        original
            .extra
            .push(("APP_TEST_CONTEXT".into(), "kept".into()));
        fixture
            .engine
            .replace_pending_log(Some(HeldQso::new(original.clone(), GridSource::LookedUp)));
        let identity = fixture.engine.pending_log_identity().unwrap();
        let prepared = PendingJournalWrite {
            pending: identity.clone(),
        }
        .prepare()
        .unwrap();
        fixture
            .engine
            .publish_pending_qso_journal(prepared)
            .unwrap()
            .sync()
            .unwrap();
        let text = std::fs::read_to_string(fixture.dir.join("pending.json")).unwrap();
        let mut restored = Engine::new("K2DEF", "FN31", 0);
        restored.load_pending_qso_json(&text);
        assert_eq!(restored.pending_log(), Some(&original));
        let mut changed = edits(&identity);
        changed.grid = Some("EN38".into());
        changed.rst_rcvd = Some("-09".into());
        let append = fixture
            .engine
            .confirm_pending_log_for_sync(identity.clone(), changed)
            .unwrap();
        assert_eq!(
            fixture.engine.pending_log(),
            Some(&original),
            "hold stays until sync"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.dir.join("pending.json")).unwrap(),
            text
        );
        let confirmed = append.sync().unwrap();
        fixture
            .engine
            .finish_pending_log_confirmation(confirmed)
            .unwrap()
            .sync()
            .unwrap();
        assert!(fixture.engine.pending_log().is_none());
        assert!(!fixture.dir.join("pending.json").exists());
        let saved = &fixture.engine.station.logbook.records()[0];
        assert_eq!(saved.freq_rx_mhz, original.freq_rx_mhz);
        assert_eq!(saved.time_off_unix, original.time_off_unix);
        assert_eq!(saved.when_unix, original.when_unix);
        assert_eq!(saved.ota, original.ota);
        assert_eq!(saved.extra, original.extra);
        assert_eq!(saved.grid.as_deref(), Some("EN38"));
        assert_eq!(saved.rst_rcvd.as_deref(), Some("-09"));
        assert_eq!(fixture.engine.station.pending_uploads.len(), 1);
        assert_eq!(
            fixture
                .engine
                .confirm_pending_log_for_sync(identity.clone(), edits(&identity))
                .unwrap_err(),
            LogFailure::StalePending
        );
    }

    #[test]
    fn failed_append_and_memory_duplicate_never_clear_the_recovery_hold() {
        let mut fixture = Fixture::new(Tier::Ft4, true);
        let identity = fixture.hold();
        let before = std::fs::read(fixture.dir.join("pending.json")).unwrap();
        // A real filesystem failure: append cannot open a directory as a file.
        fixture.engine.set_log_path(fixture.dir.clone());
        let append = fixture
            .engine
            .confirm_pending_log_for_sync(identity.clone(), edits(&identity))
            .unwrap();
        assert_eq!(
            append.sync().unwrap_err(),
            LogFailure::PersistenceUnconfirmed
        );
        assert!(fixture.engine.matches_pending_log(&identity));
        assert_eq!(
            std::fs::read(fixture.dir.join("pending.json")).unwrap(),
            before
        );
        assert_eq!(fixture.engine.station.logbook.records().len(), 1);
        let duplicate = fixture
            .engine
            .confirm_pending_log_for_sync(identity.clone(), edits(&identity))
            .unwrap();
        assert_eq!(duplicate.sync().unwrap_err(), LogFailure::AlreadyPresent);
        assert!(fixture.engine.matches_pending_log(&identity));
        assert_eq!(fixture.engine.station.pending_uploads.len(), 1);
    }

    /// ⭐ R2 (operator review, 2026-09-19) ON THE REMOTE CONFIRM TOO. The edits arrive from a
    /// phone instead of the desktop popup, but they are the same four fields and they produce
    /// the same log record: a corrected call must not carry the busted call's COUNTRY or NAME
    /// in, and a grid the station itself TRANSMITTED is not made wrong by the correction.
    #[test]
    fn a_remote_confirm_rederives_a_corrected_call_like_the_desktop() {
        let mut fixture = Fixture::new(Tier::Ft8, true);
        fixture.engine.set_dxcc_resolver(|call| {
            Some(if call.starts_with("W9") {
                "United States".to_string()
            } else {
                "Canada".to_string()
            })
        });
        fixture
            .engine
            .note_callbook_name("W9XYZ", "the other station's operator");
        let identity = fixture.hold();
        assert_eq!(identity.record().country.as_deref(), Some("United States"));
        assert!(identity.record().name.is_some());

        let mut edits = edits(&identity);
        edits.call = "VE9XYZ".into();
        fixture
            .engine
            .confirm_pending_log_for_sync(identity, edits)
            .unwrap()
            .sync()
            .unwrap();

        let logged = &fixture.engine.station.logbook.records()[0];
        assert_eq!(logged.call, "VE9XYZ");
        assert_eq!(
            logged.country.as_deref(),
            Some("Canada"),
            "the country of the call actually worked"
        );
        assert_eq!(
            logged.name, None,
            "…and no name looked up for the other one"
        );
        assert_eq!(
            logged.grid.as_deref(),
            Some("EN37"),
            "the grid came off the air with the contact, so the correction leaves it alone"
        );
    }

    #[test]
    fn a_new_identical_hold_invalidates_in_flight_confirm_and_discard() {
        let mut fixture = Fixture::new(Tier::Ft8, true);
        let identity = fixture.hold();
        let confirmed = fixture
            .engine
            .confirm_pending_log_for_sync(identity.clone(), edits(&identity))
            .unwrap()
            .sync()
            .unwrap();
        fixture.engine.replace_pending_log(Some(HeldQso::new(
            identity.record().clone(),
            GridSource::LookedUp,
        )));
        fixture.engine.persist_pending_qso();
        assert_eq!(
            fixture
                .engine
                .finish_pending_log_confirmation(confirmed)
                .unwrap_err(),
            LogFailure::StalePending
        );
        assert_eq!(
            fixture
                .engine
                .discard_pending_log_for_sync(&identity)
                .unwrap_err(),
            LogFailure::StalePending
        );
        assert!(fixture.dir.join("pending.json").exists());
        assert!(fixture.engine.pending_log().is_some());
        let current = fixture.engine.pending_log_identity().unwrap();
        fixture
            .engine
            .discard_pending_log_for_sync(&current)
            .unwrap()
            .sync()
            .unwrap();
        assert!(!fixture.dir.join("pending.json").exists());
        assert!(fixture.engine.pending_log().is_none());
    }

    #[test]
    fn old_prepared_journal_cannot_restore_a_discarded_or_replaced_hold() {
        let mut fixture = Fixture::new(Tier::Ft4, true);
        let CurrentQsoLogOutcome::Pending(write) = fixture.engine.log_current_qso_for_sync() else {
            panic!("hold")
        };
        let original = fixture.engine.pending_log().cloned().unwrap();
        let prepared = write.prepare().unwrap();
        let temporary = prepared.temporary.clone();
        let key = fixture.engine.pending_qso_log_key().unwrap_or_default();
        assert!(fixture.engine.discard_pending_log(&key));
        fixture.engine.load_pending_qso(original);
        fixture.engine.persist_pending_qso();
        let before = std::fs::read(fixture.dir.join("pending.json")).unwrap();
        assert_eq!(
            fixture
                .engine
                .publish_pending_qso_journal(prepared)
                .unwrap_err(),
            LogFailure::StalePending
        );
        assert!(!temporary.exists());
        assert_eq!(
            std::fs::read(fixture.dir.join("pending.json")).unwrap(),
            before
        );
    }

    #[test]
    fn a_pending_hold_is_not_overwritten_by_another_log_click() {
        let mut fixture = Fixture::new(Tier::Ft8, true);
        let identity = fixture.hold();
        fixture.engine.call_station("K2ABC");
        fixture
            .engine
            .ingest_decodes_for_test(&[dec_snr("K2DEF K2ABC -12", -8)], 3);
        assert!(matches!(
            fixture.engine.log_current_qso_for_sync(),
            CurrentQsoLogOutcome::PendingExists
        ));
        assert!(!fixture.engine.qso_logged);
        assert!(fixture.engine.matches_pending_log(&identity));
    }

    #[test]
    fn legacy_pending_journals_remain_readable_and_live_holds_win() {
        let mut fixture = Fixture::new(Tier::Ft8, true);
        let identity = fixture.hold();
        let dto: crate::dto::LoggedQso = identity.record().clone().into();
        let ended = identity.record().time_off_unix;
        assert!(ended.is_some(), "fixture: the held contact has an end time");

        // The OLDEST shape: a bare record DTO, written before the journal grew any extras
        // at all. It still reads, and an end time nobody recorded stays absent.
        let mut bare = serde_json::to_value(&dto).unwrap();
        bare.as_object_mut().unwrap().remove("timeOffUnix");
        let legacy = serde_json::to_string(&bare).unwrap();
        let mut restored = Engine::new("K2DEF", "FN31", 0);
        restored.load_pending_qso_json(&legacy);
        assert_eq!(restored.pending_log().unwrap().call, "W9XYZ");
        assert_eq!(restored.pending_log().unwrap().time_off_unix, None);

        // And the shape the LAST build wrote: `timeOffUnix` beside the flattened record, as
        // the sidecar #329 removed put it. Identically spelled, so it now lands in the
        // record itself — a journal on disk when an operator upgrades keeps its end time.
        let mut sidecar = serde_json::to_value(&dto).unwrap();
        sidecar.as_object_mut().unwrap().remove("timeOffUnix");
        sidecar.as_object_mut().unwrap().insert(
            "timeOffUnix".into(),
            serde_json::json!(ended.expect("checked above")),
        );
        let mut upgraded = Engine::new("K2DEF", "FN31", 0);
        upgraded.load_pending_qso_json(&serde_json::to_string(&sidecar).unwrap());
        assert_eq!(
            upgraded.pending_log().unwrap().time_off_unix,
            ended,
            "a journal written before the sidecar was folded in keeps its end time"
        );

        // `freqRxMhz` went the same way one build later, and the on-disk spelling is again
        // unchanged: the sidecar sat at the top level of the object and the flattened field
        // lands in the same place. A journal holding a SPLIT contact when the operator
        // upgrades keeps its receive leg, which is a frequency nothing else can re-derive.
        let mut split = serde_json::to_value(&dto).unwrap();
        split
            .as_object_mut()
            .unwrap()
            .insert("freqRxMhz".into(), serde_json::json!(14.0755));
        let mut with_split = Engine::new("K2DEF", "FN31", 0);
        with_split.load_pending_qso_json(&serde_json::to_string(&split).unwrap());
        assert_eq!(
            with_split.pending_log().unwrap().freq_rx_mhz,
            Some(14.0755),
            "a journal written before the sidecar was folded in keeps its split receive leg"
        );

        fixture.engine.load_pending_qso_json(&legacy);
        assert!(fixture.engine.matches_pending_log(&identity));
    }

    #[test]
    fn journal_failure_retains_the_contact_and_cannot_claim_durability() {
        let mut fixture = Fixture::new(Tier::Ft8, true);
        let blocked = fixture.dir.join("not-a-directory");
        std::fs::write(&blocked, b"existing data").unwrap();
        fixture
            .engine
            .set_pending_qso_path(blocked.join("pending.json"));
        let CurrentQsoLogOutcome::Pending(write) = fixture.engine.log_current_qso_for_sync() else {
            panic!("hold")
        };
        assert!(write.prepare().is_err());
        let pending = fixture.engine.pending_log_identity().unwrap();
        assert_eq!(pending.record().call, "W9XYZ");
        assert_eq!(std::fs::read(&blocked).unwrap(), b"existing data");
        assert_eq!(
            fixture
                .engine
                .discard_pending_log_for_sync(&pending)
                .unwrap_err(),
            LogFailure::PersistenceUnconfirmed
        );
        assert!(fixture.engine.matches_pending_log(&pending));
        assert!(fixture.engine.station.logbook.records().is_empty());
        assert!(fixture.engine.station.pending_uploads.is_empty());
    }

    #[test]
    fn invalid_edits_and_a_changed_journal_path_refuse_before_append() {
        let mut fixture = Fixture::new(Tier::Ft4, true);
        let pending = fixture.hold();
        let mut invalid = edits(&pending);
        invalid.call = "W9XYZ\nK2ABC".into();
        assert_eq!(
            fixture
                .engine
                .confirm_pending_log_for_sync(pending.clone(), invalid)
                .unwrap_err(),
            LogFailure::InvalidEdits
        );
        assert!(fixture.engine.station.logbook.records().is_empty());
        fixture
            .engine
            .set_pending_qso_path(fixture.dir.join("other.json"));
        assert_eq!(
            fixture
                .engine
                .confirm_pending_log_for_sync(pending.clone(), edits(&pending))
                .unwrap_err(),
            LogFailure::StalePending
        );
        assert!(fixture.engine.station.pending_uploads.is_empty());
        assert!(fixture.dir.join("pending.json").exists());
    }
}
