//! The PSK Reporter reports of who heard this station, kept on disk.
//!
//! PSK Reporter keeps a reception report for about a day. Nexus already asks it, every five
//! minutes, for the last half hour of the operator's reports (the propagation nowcast's query);
//! this store keeps the ones where the operator was the SENDER, so the record of who heard the
//! station grows from the day it is installed instead of rolling off after a day. It adds no
//! query of its own: it is written from the answers the nowcast already fetched, and nothing in
//! it leaves the machine.
//!
//! **What a row holds.** The operator's call as sent, when the report was made (PSK Reporter's
//! `flowStartSeconds`), the receiving call, its grid when it gave one, the band, the exact
//! frequency in Hz when the report carries one, the mode, and the SNR when present. Nothing else
//! from the answer, and never the request. The strings come from other stations' reports and are
//! kept forever, so each has a limit past any real value: a call [`MAX_CALL`] bytes, a grid
//! [`MAX_GRID`], a mode [`MAX_MODE`], a band label [`MAX_BAND`]. A report with a longer field is
//! not kept at all; it is never truncated, which could merge two distinct reports into one.
//!
//! **Dedupe.** The query's half-hour window is wider than its five-minute cadence, so every
//! report comes back on about six polls running. A report is the same report when its sender,
//! time, receiver, band and mode are the same: that is the table's `UNIQUE` key, and a row that
//! is already there is ignored (`INSERT OR IGNORE`), so recording one answer twice changes
//! nothing. The mode is stored as `''` when the report has none, because SQLite's `UNIQUE` treats
//! every `NULL` as distinct and a `NULL` in the key would let a re-fetched report in again.
//!
//! **Retention.** Everything is kept: ten thousand reports measured 942,080 bytes on disk, the
//! dedupe index included (`ten_thousand_reports_stay_near_their_measured_size`), about 94 bytes a
//! report, so a station heard a thousand times a day adds about 34 MB a year.
//!
//! **Why a file of its own** (`heard-reports.sqlite`, beside the settings), not a table in the
//! log database: for the same reasons as [`crate::callbook_cache`]. The log is the operator's
//! record, with its own schema version, backups and single writer the FT auto-log depends on, and
//! a write here must never queue behind a contact's.
//!
//! **The schema version** is SQLite's `user_version`. Opening a file at an older version brings
//! it to [`SCHEMA_VERSION`] in place; a file written by a newer build is refused rather than
//! written under a schema this build does not know.

use rusqlite::{params, Connection};
use std::path::Path;

/// The file name, beside `settings.json`.
pub const FILE_NAME: &str = "heard-reports.sqlite";

/// The schema this build writes, in SQLite's `user_version`.
pub const SCHEMA_VERSION: i64 = 1;

/// The longest call kept, bytes.
pub const MAX_CALL: usize = 32;
/// The longest receiver grid kept, bytes.
pub const MAX_GRID: usize = 10;
/// The longest mode kept, bytes.
pub const MAX_MODE: usize = 16;
/// The longest band label kept, bytes.
pub const MAX_BAND: usize = 8;

/// One report of the operator's signal being heard.
#[derive(Debug, Clone, PartialEq)]
pub struct HeardReport {
    /// When the report was made, Unix seconds.
    pub time: i64,
    /// The operator's call as sent.
    pub my_call: String,
    /// The station that heard it.
    pub rx_call: String,
    /// The receiver's grid, when it gave one.
    pub rx_grid: Option<String>,
    /// The band label (`"20m"`).
    pub band: String,
    /// The exact frequency, Hz, when the report carries one.
    pub freq_hz: Option<i64>,
    /// The mode (`"FT8"`), when the report names one.
    pub mode: Option<String>,
    /// The signal report, dB, when present.
    pub snr: Option<f32>,
}

/// The store could not be opened.
#[derive(Debug)]
pub enum OpenError {
    Sqlite(rusqlite::Error),
    /// The file was written by a newer build.
    NewerSchema {
        found: i64,
    },
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "heard-reports store: {e}"),
            Self::NewerSchema { found } => write!(
                f,
                "heard-reports store is schema version {found}, this build writes {SCHEMA_VERSION}"
            ),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<rusqlite::Error> for OpenError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

/// The on-disk store. See the module header.
pub struct HeardReports {
    conn: Connection,
}

impl HeardReports {
    /// Open (creating or migrating if needed) the store at `path`.
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        Self::init(Connection::open(path)?)
    }

    /// An in-memory store, for tests.
    pub fn open_in_memory() -> Result<Self, OpenError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, OpenError> {
        conn.busy_timeout(std::time::Duration::from_millis(250))?;
        let found: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if found > SCHEMA_VERSION {
            return Err(OpenError::NewerSchema { found });
        }
        if found < 1 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS heard_report (
                     time     INTEGER NOT NULL,
                     my_call  TEXT NOT NULL,
                     rx_call  TEXT NOT NULL,
                     rx_grid  TEXT,
                     band     TEXT NOT NULL,
                     freq_hz  INTEGER,
                     mode     TEXT NOT NULL,
                     snr      REAL,
                     UNIQUE (my_call, time, rx_call, band, mode)
                 );
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        Ok(Self { conn })
    }

    /// Keep `reports`, ignoring any already kept. Returns how many were new.
    pub fn record(&mut self, reports: &[HeardReport]) -> rusqlite::Result<usize> {
        let tx = self.conn.transaction()?;
        let mut added = 0;
        {
            let mut insert = tx.prepare_cached(
                "INSERT OR IGNORE INTO heard_report
                     (time, my_call, rx_call, rx_grid, band, freq_hz, mode, snr)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for r in reports {
                let (my_call, rx_call) = (call_key(&r.my_call), call_key(&r.rx_call));
                if my_call.is_empty() || rx_call.is_empty() {
                    continue;
                }
                let rx_grid = r
                    .rx_grid
                    .as_deref()
                    .map(str::trim)
                    .filter(|g| !g.is_empty());
                let mode = r.mode.as_deref().unwrap_or("").trim();
                // Outside data kept forever: a field longer than any real one skips the row
                // (never truncated, which could merge two distinct reports into one key).
                if my_call.len() > MAX_CALL
                    || rx_call.len() > MAX_CALL
                    || rx_grid.is_some_and(|g| g.len() > MAX_GRID)
                    || mode.len() > MAX_MODE
                    || r.band.len() > MAX_BAND
                {
                    continue;
                }
                added += insert.execute(params![
                    r.time, my_call, rx_call, rx_grid, r.band, r.freq_hz, mode, r.snr,
                ])?;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// How many reports are kept.
    pub fn len(&self) -> rusqlite::Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM heard_report", [], |r| r.get(0))
    }

    /// Whether nothing is kept.
    pub fn is_empty(&self) -> rusqlite::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Every kept report, oldest first.
    pub fn all(&self) -> rusqlite::Result<Vec<HeardReport>> {
        let mut q = self.conn.prepare(
            "SELECT time, my_call, rx_call, rx_grid, band, freq_hz, mode, snr
               FROM heard_report ORDER BY time, rx_call, band, mode",
        )?;
        let rows = q.query_map([], |r| {
            let mode: String = r.get(6)?;
            Ok(HeardReport {
                time: r.get(0)?,
                my_call: r.get(1)?,
                rx_call: r.get(2)?,
                rx_grid: r.get(3)?,
                band: r.get(4)?,
                freq_hz: r.get(5)?,
                mode: (!mode.is_empty()).then_some(mode),
                snr: r.get(7)?,
            })
        })?;
        rows.collect()
    }
}

fn call_key(call: &str) -> String {
    call.trim().to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory of this test's own (pid and name), removed first.
    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexus-heard-reports-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn report(time: i64, rx: &str, band: &str, snr: Option<f32>) -> HeardReport {
        HeardReport {
            time,
            my_call: "W1AW".into(),
            rx_call: rx.into(),
            rx_grid: Some("FN31pr".into()),
            band: band.into(),
            freq_hz: Some(14_074_698),
            mode: Some("FT8".into()),
            snr,
        }
    }

    /// Two polls whose half-hour windows overlap: the shared reports are kept once, the new
    /// ones are added, and every field comes back as it went in.
    #[test]
    fn two_fetches_with_overlapping_reports_are_kept_once_each() {
        let mut s = HeardReports::open_in_memory().unwrap();
        let first = vec![
            report(1_000, "K1ABC", "20m", Some(-12.0)),
            report(1_015, "VE3XYZ", "20m", None),
        ];
        assert_eq!(s.record(&first).unwrap(), 2);
        // The next poll: the same two reports again (case/whitespace as the server sent them
        // may differ), plus one new one.
        let mut second = first.clone();
        second[0].rx_call = " k1abc ".into();
        second.push(report(1_300, "K1ABC", "20m", Some(-8.0)));
        assert_eq!(
            s.record(&second).unwrap(),
            1,
            "only the new report is added"
        );
        assert_eq!(
            s.record(&second).unwrap(),
            0,
            "recording an answer twice adds nothing"
        );

        let kept = s.all().unwrap();
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0], report(1_000, "K1ABC", "20m", Some(-12.0)));
        assert_eq!(kept[1], report(1_015, "VE3XYZ", "20m", None));
        assert_eq!(kept[2], report(1_300, "K1ABC", "20m", Some(-8.0)));
    }

    /// A field longer than a real one is refused, not truncated (truncating could merge two
    /// distinct reports into one key): the row is skipped, and the rest of the answer is kept.
    #[test]
    fn a_row_with_an_oversized_field_is_not_kept() {
        let mut s = HeardReports::open_in_memory().unwrap();
        let long_call = report(1_000, &"K".repeat(MAX_CALL + 1), "20m", None);
        let mut long_mode = report(1_001, "K1ABC", "20m", None);
        long_mode.mode = Some("M".repeat(MAX_MODE + 1));
        let mut long_grid = report(1_002, "K1ABC", "20m", None);
        long_grid.rx_grid = Some("G".repeat(MAX_GRID + 1));
        let long_band = report(1_003, "K1ABC", &"b".repeat(MAX_BAND + 1), None);
        let normal = report(1_004, "VE3XYZ", "20m", Some(-5.0));
        let n = s
            .record(&[long_call, long_mode, long_grid, long_band, normal.clone()])
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(s.all().unwrap(), vec![normal]);
        // At the limits exactly, a row is kept.
        let mut at_limit = report(1_005, &"K".repeat(MAX_CALL), "20m", None);
        at_limit.mode = Some("M".repeat(MAX_MODE));
        at_limit.rx_grid = Some("G".repeat(MAX_GRID));
        assert_eq!(s.record(&[at_limit]).unwrap(), 1);
    }

    /// A report with no mode, grid or frequency is still kept once, not once per poll: the
    /// missing mode must not become a `NULL` in the dedupe key.
    #[test]
    fn a_report_missing_its_optional_fields_still_dedupes() {
        let mut s = HeardReports::open_in_memory().unwrap();
        let bare = HeardReport {
            time: 2_000,
            my_call: "W1AW".into(),
            rx_call: "K1ABC".into(),
            rx_grid: None,
            band: "40m".into(),
            freq_hz: None,
            mode: None,
            snr: None,
        };
        assert_eq!(s.record(std::slice::from_ref(&bare)).unwrap(), 1);
        assert_eq!(s.record(std::slice::from_ref(&bare)).unwrap(), 0);
        assert_eq!(s.all().unwrap(), vec![bare]);
    }

    /// The same receiver hearing the station on two bands, or in two modes, is two reports.
    #[test]
    fn a_different_band_or_mode_is_a_different_report() {
        let mut s = HeardReports::open_in_memory().unwrap();
        let mut other_mode = report(1_000, "K1ABC", "20m", None);
        other_mode.mode = Some("FT4".into());
        let n = s
            .record(&[
                report(1_000, "K1ABC", "20m", None),
                report(1_000, "K1ABC", "40m", None),
                other_mode,
            ])
            .unwrap();
        assert_eq!(n, 3);
    }

    /// Reports survive a restart, and reopening an existing store neither loses nor duplicates
    /// them.
    #[test]
    fn an_existing_store_reopens_with_its_reports() {
        let dir = scratch("reopen");
        let path = dir.join(FILE_NAME);
        {
            let mut s = HeardReports::open(&path).unwrap();
            s.record(&[report(1_000, "K1ABC", "20m", Some(-3.0))])
                .unwrap();
        }
        let mut s = HeardReports::open(&path).unwrap();
        assert_eq!(s.len().unwrap(), 1);
        assert_eq!(
            s.record(&[report(1_000, "K1ABC", "20m", Some(-3.0))])
                .unwrap(),
            0
        );
        let version: i64 = s
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An existing database at version 0 (a file that predates the store, holding something
    /// else) is migrated in place: the table is added, what was there is untouched.
    #[test]
    fn an_existing_version_zero_database_is_migrated_in_place() {
        let dir = scratch("migrate");
        let path = dir.join(FILE_NAME);
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE other (x INTEGER); INSERT INTO other VALUES (7);")
                .unwrap();
        }
        let mut s = HeardReports::open(&path).unwrap();
        assert!(s.is_empty().unwrap());
        assert_eq!(s.record(&[report(1_000, "K1ABC", "20m", None)]).unwrap(), 1);
        let kept: i64 = s
            .conn
            .query_row("SELECT x FROM other", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, 7);
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file written by a newer build is refused, never written under an unknown schema.
    #[test]
    fn a_newer_schema_is_refused() {
        let dir = scratch("newer");
        let path = dir.join(FILE_NAME);
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch("PRAGMA user_version = 99;").unwrap();
        }
        match HeardReports::open(&path) {
            Err(OpenError::NewerSchema { found: 99 }) => {}
            other => panic!("expected a refusal, got {:?}", other.map(|_| ())),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The size, measured: ten thousand full reports, and the file they make (942,080 bytes).
    #[test]
    fn ten_thousand_reports_stay_near_their_measured_size() {
        let dir = scratch("size");
        let path = dir.join(FILE_NAME);
        let mut s = HeardReports::open(&path).unwrap();
        let reports: Vec<HeardReport> = (0..10_000)
            .map(|i| report(1_700_000_000 + i, &format!("K{i}ABC"), "20m", Some(-15.0)))
            .collect();
        assert_eq!(s.record(&reports).unwrap(), 10_000);
        drop(s);
        let bytes = std::fs::metadata(&path).unwrap().len();
        eprintln!("heard reports: 10000 rows = {bytes} bytes on disk");
        assert!(bytes < 1024 * 1024, "{bytes} bytes for 10k reports");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
