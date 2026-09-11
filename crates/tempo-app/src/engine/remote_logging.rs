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

/// The existing journal DTO plus fields which the ordinary UI edit DTO does
/// not carry. Legacy journals remain readable. Nothing changes the log format.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingRecord {
    #[serde(flatten)]
    record: crate::dto::LoggedQso,
    #[serde(default)]
    time_off_unix: Option<u64>,
    #[serde(default)]
    freq_rx_mhz: Option<f64>,
}

pub(super) fn pending_qso_json(record: &QsoRecord) -> serde_json::Result<String> {
    serde_json::to_string(&PendingRecord {
        record: record.clone().into(),
        time_off_unix: record.time_off_unix,
        freq_rx_mhz: record.freq_rx_mhz,
    })
}

/// An exact hold, including its incarnation. Replacing a hold with an identical
/// record still invalidates an earlier confirmation or discard.
#[derive(Clone, Debug)]
pub struct PendingLogIdentity {
    identity: Arc<()>,
    record: Arc<QsoRecord>,
    path: Option<PathBuf>,
}

impl PendingLogIdentity {
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
        file.write_all(pending_qso_json(prepared.pending.record())?.as_bytes())?;
        file.sync_all()?;
        prepared.parent = journal_parent(prepared.pending.path.as_ref().expect("path checked"))?;
        Ok(prepared)
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
            let mut expected = native.engine.station.logbook.records()[0].clone();
            let actual = &remote.engine.station.logbook.records()[0];
            assert!(
                expected
                    .time_off_unix
                    .unwrap()
                    .abs_diff(actual.time_off_unix.unwrap())
                    <= 2
            );
            expected.time_off_unix = actual.time_off_unix;
            assert_eq!(&expected, actual);
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
    fn held_record_keeps_split_and_end_time_through_journal_and_confirmation() {
        let mut fixture = Fixture::new(Tier::Ft8, true);
        let CurrentQsoLogOutcome::Pending(_) = fixture.engine.log_current_qso_for_sync() else {
            panic!("hold")
        };
        let mut original = fixture.engine.pending_log.clone().unwrap();
        original.freq_rx_mhz = Some(14.0755);
        original.time_off_unix = Some(1_700_000_123);
        original.ota.their_program = Some("POTA".into());
        original.ota.their_ref = Some("K-1234".into());
        original
            .extra
            .push(("APP_TEST_CONTEXT".into(), "kept".into()));
        fixture.engine.replace_pending_log(Some(original.clone()));
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
        assert_eq!(restored.pending_log, Some(original.clone()));
        let mut changed = edits(&identity);
        changed.grid = Some("EN38".into());
        changed.rst_rcvd = Some("-09".into());
        let append = fixture
            .engine
            .confirm_pending_log_for_sync(identity.clone(), changed)
            .unwrap();
        assert_eq!(
            fixture.engine.pending_log,
            Some(original.clone()),
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
        assert!(fixture.engine.pending_log.is_none());
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
        fixture
            .engine
            .replace_pending_log(Some(identity.record().clone()));
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
        assert!(fixture.engine.pending_log.is_some());
        let current = fixture.engine.pending_log_identity().unwrap();
        fixture
            .engine
            .discard_pending_log_for_sync(&current)
            .unwrap()
            .sync()
            .unwrap();
        assert!(!fixture.dir.join("pending.json").exists());
        assert!(fixture.engine.pending_log.is_none());
    }

    #[test]
    fn old_prepared_journal_cannot_restore_a_discarded_or_replaced_hold() {
        let mut fixture = Fixture::new(Tier::Ft4, true);
        let CurrentQsoLogOutcome::Pending(write) = fixture.engine.log_current_qso_for_sync() else {
            panic!("hold")
        };
        let original = fixture.engine.pending_log.clone().unwrap();
        let prepared = write.prepare().unwrap();
        let temporary = prepared.temporary.clone();
        fixture.engine.discard_pending_log();
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
        let legacy = serde_json::to_string(&dto).unwrap();
        let mut restored = Engine::new("K2DEF", "FN31", 0);
        restored.load_pending_qso_json(&legacy);
        assert_eq!(restored.pending_log.as_ref().unwrap().call, "W9XYZ");
        assert_eq!(restored.pending_log.as_ref().unwrap().time_off_unix, None);
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
    pub(super) fn replace_pending_log(&mut self, record: Option<QsoRecord>) {
        self.pending_log = record;
        self.pending_log_identity = Arc::new(());
    }

    pub fn pending_log_identity(&self) -> Option<PendingLogIdentity> {
        Some(PendingLogIdentity {
            identity: self.pending_log_identity.clone(),
            record: Arc::new(self.pending_log.clone()?),
            path: self.station.pending_qso_path.clone(),
        })
    }

    fn matches_pending_log(&self, pending: &PendingLogIdentity) -> bool {
        Arc::ptr_eq(&self.pending_log_identity, &pending.identity)
            && self.pending_log.as_ref() == Some(pending.record())
            && self.station.pending_qso_path == pending.path
    }

    pub fn load_pending_qso_json(&mut self, text: &str) {
        if let Ok(pending) = serde_json::from_str::<PendingRecord>(text) {
            let mut record: QsoRecord = pending.record.into();
            record.time_off_unix = pending.time_off_unix;
            record.freq_rx_mhz = pending.freq_rx_mhz;
            self.load_pending_qso(record);
        }
    }

    /// The host must bind the displayed QSO and original controller context
    /// before calling. Eligibility and write-once behavior are shared with the
    /// local button. An existing confirmation is never replaced by this action.
    pub fn log_current_qso_for_sync(&mut self) -> CurrentQsoLogOutcome {
        if self.pending_log.is_some() {
            return CurrentQsoLogOutcome::PendingExists;
        }
        let Some(record) = self.take_current_qso_record() else {
            return CurrentQsoLogOutcome::NoEligibleContact;
        };
        if self.settings.prompt_to_log {
            self.replace_pending_log(Some(record));
            CurrentQsoLogOutcome::Pending(PendingJournalWrite {
                pending: self.pending_log_identity().expect("hold just installed"),
            })
        } else {
            CurrentQsoLogOutcome::Append(self.log_qso_for_sync(record))
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
