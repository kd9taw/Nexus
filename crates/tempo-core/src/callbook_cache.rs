//! The callbook's answers, kept on disk: a call looked up once is answered from here until the
//! answer is older than the operator's expiry, and offline it is answered from here at any age.
//!
//! **What a row holds.** The call as asked (upper case), the answer as the caller serialised it
//! (the lookup DTO's JSON — the public record a callbook returned and nothing else), when it was
//! fetched, and when it was last used. Never a password, a session key or a request URL: the
//! caller hands in a parsed answer, never the response body or the request, so neither can
//! reach this file. The shell's tests scan the stored rows for a fixture's secrets to keep it so.
//!
//! **Why a file of its own, not a table in the log database.** The log is the operator's
//! irreplaceable record, with its own schema version, migration, backups and single writer the
//! FT auto-log depends on. A cache is disposable: "clear" must be a `DELETE` that cannot touch a
//! contact, a cache write must never queue behind (or ahead of) a contact's, and a log backup or
//! export has no business carrying a few thousand strangers' addresses. So it is a sibling
//! SQLite file, `callbook-cache.sqlite`, in the same directory as the settings.
//!
//! **The bound.** At most [`CAPACITY`] calls, least recently USED dropped first. "Used" is a
//! counter, not the clock: the clock steps (and steps backwards), and an LRU order taken from
//! it would evict the wrong row after a step. An answer with every field filled serialises to
//! 425 bytes, and a full cache of them measured 4.9 MB on disk
//! (`a_full_cache_stays_near_its_measured_size`).
//!
//! **Expiry** is the caller's: [`CallbookCache::get`] returns any row with its age and lets the
//! caller decide, because the same row is fresh for a lookup and still worth having offline.
//!
//! **The lookups a Logbook update spent** ([`CallbookCache::note_lookups`]). "Update from callbook"
//! looks many calls up at once, and QRZ counts an account's lookups over 24 hours. Each network
//! lookup it makes is noted here with its time, so its daily cap holds across runs and restarts.
//! Only a time: never the call, the answer or anything else. "Clear" forgets the answers, not
//! this tally, which would otherwise let a clear reset the cap.
//! QRZ's own count of the account's lookups, from the last answer that carried one, is kept the
//! same way ([`CallbookCache::note_qrz_count`]): the count and its time, nothing else. So is the
//! time until which QRZ is not asked after it refused service ([`CallbookCache::note_qrz_refused`]).

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// The most calls the cache keeps. See the module header for the measured size.
pub const CAPACITY: usize = 10_000;

/// The file name, beside `settings.json`.
pub const FILE_NAME: &str = "callbook-cache.sqlite";

/// One stored answer.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedAnswer {
    /// The answer as it was stored (the caller's JSON).
    pub answer: String,
    /// When it was fetched, Unix seconds.
    pub fetched_at: i64,
}

impl CachedAnswer {
    /// Whether the answer is younger than `max_age_secs` at `now`. A fetch time in the future
    /// (the clock stepped back) counts as age 0, not as expired.
    pub fn is_fresh(&self, now: i64, max_age_secs: i64) -> bool {
        now.saturating_sub(self.fetched_at).max(0) < max_age_secs
    }
}

/// The on-disk callbook cache. See the module header.
pub struct CallbookCache {
    conn: Connection,
    capacity: usize,
}

impl CallbookCache {
    /// Open (creating if needed) the cache at `path`.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?, CAPACITY)
    }

    /// An in-memory cache, for tests.
    pub fn open_in_memory(capacity: usize) -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?, capacity)
    }

    fn init(conn: Connection, capacity: usize) -> rusqlite::Result<Self> {
        // A cache never waits long for itself: the only other connection is a test's.
        conn.busy_timeout(std::time::Duration::from_millis(250))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS callbook_answer (
                 call       TEXT PRIMARY KEY NOT NULL,
                 answer     TEXT NOT NULL,
                 fetched_at INTEGER NOT NULL,
                 last_used  INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS callbook_answer_last_used
                 ON callbook_answer (last_used);
             CREATE TABLE IF NOT EXISTS lookup_spent (at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS qrz_count (at INTEGER NOT NULL, count INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS qrz_refused (until INTEGER NOT NULL);",
        )?;
        Ok(Self { conn, capacity })
    }

    fn key(call: &str) -> String {
        call.trim().to_ascii_uppercase()
    }

    fn next_use(&self) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT COALESCE(MAX(last_used), 0) + 1 FROM callbook_answer",
            [],
            |r| r.get(0),
        )
    }

    /// The stored answer for `call`, at any age, marking it used.
    pub fn get(&self, call: &str) -> rusqlite::Result<Option<CachedAnswer>> {
        let key = Self::key(call);
        let hit = self
            .conn
            .query_row(
                "SELECT answer, fetched_at FROM callbook_answer WHERE call = ?1",
                params![key],
                |r| {
                    Ok(CachedAnswer {
                        answer: r.get(0)?,
                        fetched_at: r.get(1)?,
                    })
                },
            )
            .optional()?;
        if hit.is_some() {
            let used = self.next_use()?;
            self.conn.execute(
                "UPDATE callbook_answer SET last_used = ?1 WHERE call = ?2",
                params![used, key],
            )?;
        }
        Ok(hit)
    }

    /// Store `answer` for `call`, fetched at `fetched_at`, replacing an earlier one, then drop
    /// the least recently used rows beyond the capacity.
    pub fn put(&self, call: &str, answer: &str, fetched_at: i64) -> rusqlite::Result<()> {
        let key = Self::key(call);
        if key.is_empty() {
            return Ok(());
        }
        let used = self.next_use()?;
        self.conn.execute(
            "INSERT INTO callbook_answer (call, answer, fetched_at, last_used)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(call) DO UPDATE SET
                 answer = excluded.answer,
                 fetched_at = excluded.fetched_at,
                 last_used = excluded.last_used",
            params![key, answer, fetched_at, used],
        )?;
        self.conn.execute(
            "DELETE FROM callbook_answer WHERE last_used <= (
                 SELECT last_used FROM callbook_answer
                 ORDER BY last_used DESC LIMIT 1 OFFSET ?1)",
            params![self.capacity as i64],
        )?;
        Ok(())
    }

    /// Forget `call`.
    pub fn remove(&self, call: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM callbook_answer WHERE call = ?1",
            params![Self::key(call)],
        )?;
        Ok(())
    }

    /// Forget every call.
    pub fn clear(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM callbook_answer", [])?;
        // Give the space back: a cleared cache should not keep a full one's file.
        self.conn.execute_batch("VACUUM")?;
        Ok(())
    }

    /// How many calls are stored.
    pub fn len(&self) -> rusqlite::Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM callbook_answer", [], |r| {
                r.get::<_, i64>(0)
            })
            .map(|n| n as usize)
    }

    /// Whether nothing is stored.
    pub fn is_empty(&self) -> rusqlite::Result<bool> {
        self.len().map(|n| n == 0)
    }

    /// Note `n` network lookups made at `at` (Unix seconds), forgetting those more than a day
    /// before it. See the module header.
    pub fn note_lookups(&self, n: usize, at: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM lookup_spent WHERE at <= ?1",
            params![at - 86_400],
        )?;
        for _ in 0..n {
            self.conn
                .execute("INSERT INTO lookup_spent (at) VALUES (?1)", params![at])?;
        }
        Ok(())
    }

    /// How many lookups [`Self::note_lookups`] noted in the 24 hours before `now`. One noted
    /// after `now` (the clock stepped back) still counts: a cap never loosens on a clock step.
    pub fn lookups_spent(&self, now: i64) -> rusqlite::Result<usize> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM lookup_spent WHERE at > ?1",
                params![now - 86_400],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as usize)
    }

    /// Keep `count`, QRZ's own count of the account's lookups in the 24 hours before `at`, in
    /// place of the one kept before. See the module header.
    pub fn note_qrz_count(&self, count: usize, at: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM qrz_count", [])?;
        self.conn.execute(
            "INSERT INTO qrz_count (at, count) VALUES (?1, ?2)",
            params![at, count as i64],
        )?;
        Ok(())
    }

    /// QRZ's count as [`Self::note_qrz_count`] kept it, while it is less than a day old at `now`.
    /// One kept after `now` (the clock stepped back) still counts, as for the tally.
    pub fn qrz_count(&self, now: i64) -> rusqlite::Result<Option<usize>> {
        self.conn
            .query_row(
                "SELECT count FROM qrz_count WHERE at > ?1",
                params![now - 86_400],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map(|n| n.map(|n| n.max(0) as usize))
    }

    /// Forget the `n` lookups noted last: lookups noted before they were made that turned out not
    /// to count (a service that was not answering).
    pub fn forget_lookups(&self, n: usize) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM lookup_spent WHERE rowid IN \
             (SELECT rowid FROM lookup_spent ORDER BY rowid DESC LIMIT ?1)",
            params![n as i64],
        )?;
        Ok(())
    }

    /// QRZ refused service: it is not asked until `until` (Unix seconds).
    pub fn note_qrz_refused(&self, until: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM qrz_refused", [])?;
        self.conn.execute(
            "INSERT INTO qrz_refused (until) VALUES (?1)",
            params![until],
        )?;
        Ok(())
    }

    /// The time QRZ may be asked again, while it is after `now`.
    pub fn qrz_refused_until(&self, now: i64) -> rusqlite::Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT until FROM qrz_refused WHERE until > ?1",
                params![now],
                |r| r.get(0),
            )
            .optional()
    }

    /// Every stored answer's text, for the tests that check what reached the disk.
    pub fn stored_text(&self) -> rusqlite::Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT call || char(31) || answer FROM callbook_answer")?;
        let rows = st.query_map([], |r| r.get(0))?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    /// A fresh directory of this test's own (pid and name), removed first.
    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexus-callbook-cache-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_answer_comes_back_for_the_call_in_any_case() {
        let c = CallbookCache::open_in_memory(10).unwrap();
        c.put("w1abc", r#"{"call":"W1ABC"}"#, 1_000).unwrap();
        let hit = c.get(" W1ABC ").unwrap().unwrap();
        assert_eq!(hit.answer, r#"{"call":"W1ABC"}"#);
        assert_eq!(hit.fetched_at, 1_000);
        assert_eq!(c.get("W1XYZ").unwrap(), None);
    }

    #[test]
    fn an_answer_is_fresh_until_its_expiry_and_kept_after_it() {
        let c = CallbookCache::open_in_memory(10).unwrap();
        c.put("W1ABC", "{}", 0).unwrap();
        let hit = c.get("W1ABC").unwrap().unwrap();
        assert!(hit.is_fresh(30 * DAY - 1, 30 * DAY));
        assert!(!hit.is_fresh(30 * DAY, 30 * DAY));
        // Expired is the caller's verdict: the row is still there for offline use.
        assert!(c.get("W1ABC").unwrap().is_some());
        // A clock that stepped back does not expire it.
        assert!(hit.is_fresh(-5, 30 * DAY));
    }

    #[test]
    fn a_new_answer_replaces_the_old_one() {
        let c = CallbookCache::open_in_memory(10).unwrap();
        c.put("W1ABC", "old", 1).unwrap();
        c.put("W1ABC", "new", 2).unwrap();
        assert_eq!(c.get("W1ABC").unwrap().unwrap().answer, "new");
        assert_eq!(c.len().unwrap(), 1);
    }

    #[test]
    fn the_least_recently_used_call_goes_first() {
        let c = CallbookCache::open_in_memory(3).unwrap();
        c.put("A1A", "a", 1).unwrap();
        c.put("B1B", "b", 2).unwrap();
        c.put("C1C", "c", 3).unwrap();
        // Using A makes B the oldest, though A was stored first.
        c.get("A1A").unwrap();
        c.put("D1D", "d", 4).unwrap();
        assert_eq!(c.len().unwrap(), 3);
        assert!(c.get("B1B").unwrap().is_none(), "B was least recently used");
        for kept in ["A1A", "C1C", "D1D"] {
            assert!(c.get(kept).unwrap().is_some(), "{kept} should be kept");
        }
        // Fetch time plays no part: an old fetch used recently stays.
        c.put("E1E", "e", 0).unwrap();
        assert!(c.get("E1E").unwrap().is_some());
        assert_eq!(c.len().unwrap(), 3);
    }

    #[test]
    fn the_cache_survives_a_restart() {
        let dir = scratch("restart");
        let path = dir.join(FILE_NAME);
        {
            let c = CallbookCache::open(&path).unwrap();
            c.put("W1ABC", r#"{"name":"Dave"}"#, 42).unwrap();
        }
        let c = CallbookCache::open(&path).unwrap();
        let hit = c.get("W1ABC").unwrap().unwrap();
        assert_eq!(
            (hit.answer.as_str(), hit.fetched_at),
            (r#"{"name":"Dave"}"#, 42)
        );
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_and_remove_forget() {
        let c = CallbookCache::open_in_memory(10).unwrap();
        c.put("W1ABC", "a", 1).unwrap();
        c.put("W2DEF", "b", 1).unwrap();
        c.remove("w1abc").unwrap();
        assert!(c.get("W1ABC").unwrap().is_none());
        assert_eq!(c.len().unwrap(), 1);
        c.clear().unwrap();
        assert!(c.is_empty().unwrap());
    }

    /// The tally of a day's lookups: counted for 24 hours, then gone; kept across a restart and
    /// across a clear of the answers; never moved by a clock that steps back.
    #[test]
    fn the_lookups_spent_count_for_a_day_and_survive_a_clear_and_a_restart() {
        let dir = scratch("spent");
        let path = dir.join(FILE_NAME);
        {
            let c = CallbookCache::open(&path).unwrap();
            c.note_lookups(1, 1_000).unwrap();
            c.note_lookups(2, 5_000).unwrap();
            assert_eq!(c.lookups_spent(5_000).unwrap(), 3);
            c.put("W1ABC", "a", 1).unwrap();
            c.clear().unwrap();
            assert_eq!(
                c.lookups_spent(5_000).unwrap(),
                3,
                "a clear keeps the tally"
            );
        }
        let c = CallbookCache::open(&path).unwrap();
        assert_eq!(c.lookups_spent(5_000).unwrap(), 3, "kept across a restart");
        assert_eq!(c.lookups_spent(1_000 + DAY - 1).unwrap(), 3);
        assert_eq!(c.lookups_spent(1_000 + DAY).unwrap(), 2, "a day old: gone");
        assert_eq!(c.lookups_spent(5_000 + DAY).unwrap(), 0);
        assert_eq!(c.lookups_spent(0).unwrap(), 3, "a clock stepped back");
        // A note forgets what is more than a day before it; the table does not grow.
        c.note_lookups(1, 5_000 + DAY + 1).unwrap();
        let rows: i64 = c
            .conn
            .query_row("SELECT COUNT(*) FROM lookup_spent", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert!(
            c.stored_text().unwrap().is_empty(),
            "the tally holds no call and no answer"
        );
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// QRZ's last count is kept with its time across a restart and a clear, the newest only,
    /// for a day; and it is a number and a time, nothing else.
    #[test]
    fn qrzs_count_is_kept_for_a_day_and_survives_a_restart() {
        let dir = scratch("qrz-count");
        let path = dir.join(FILE_NAME);
        {
            let c = CallbookCache::open(&path).unwrap();
            assert_eq!(c.qrz_count(1_000).unwrap(), None);
            c.note_qrz_count(240, 1_000).unwrap();
            c.note_qrz_count(245, 2_000).unwrap();
            c.clear().unwrap();
        }
        let c = CallbookCache::open(&path).unwrap();
        assert_eq!(
            c.qrz_count(2_000).unwrap(),
            Some(245),
            "the newest, after a restart"
        );
        assert_eq!(c.qrz_count(2_000 + DAY - 1).unwrap(), Some(245));
        assert_eq!(c.qrz_count(2_000 + DAY).unwrap(), None, "a day old: gone");
        assert_eq!(c.qrz_count(0).unwrap(), Some(245), "a clock stepped back");
        let rows: i64 = c
            .conn
            .query_row("SELECT COUNT(*) FROM qrz_count", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert!(c.stored_text().unwrap().is_empty());
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// QRZ's refusal holds until its time, across a restart and a clear; then it is gone. The
    /// last lookups noted can be forgotten, and only they.
    #[test]
    fn qrzs_refusal_holds_until_its_time_and_lookups_can_be_forgotten() {
        let dir = scratch("qrz-refused");
        let path = dir.join(FILE_NAME);
        {
            let c = CallbookCache::open(&path).unwrap();
            assert_eq!(c.qrz_refused_until(1_000).unwrap(), None);
            c.note_qrz_refused(1_000 + DAY).unwrap();
            c.clear().unwrap();
            c.note_lookups(3, 900).unwrap();
            c.note_lookups(2, 1_000).unwrap();
            c.forget_lookups(2).unwrap();
            assert_eq!(c.lookups_spent(1_000).unwrap(), 3, "the last two forgotten");
        }
        let c = CallbookCache::open(&path).unwrap();
        assert_eq!(c.qrz_refused_until(1_000).unwrap(), Some(1_000 + DAY));
        assert_eq!(
            c.qrz_refused_until(1_000 + DAY - 1).unwrap(),
            Some(1_000 + DAY)
        );
        assert_eq!(c.qrz_refused_until(1_000 + DAY).unwrap(), None, "over");
        assert!(c.stored_text().unwrap().is_empty());
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bound, measured: a full cache of full answers, and the file it makes. The answer is
    /// a QRZ answer with every field the lookup parses filled (425 bytes of JSON); measured at
    /// 4,894,720 bytes for the full cache.
    #[test]
    fn a_full_cache_stays_near_its_measured_size() {
        let dir = scratch("size");
        let path = dir.join(FILE_NAME);
        let c = CallbookCache::open(&path).unwrap();
        let answer = String::from(
            r#"{"call":"W1ABC","name":"David Smith","nickname":"Dave","qth":"Newington","grid":"FN31pr","state":"CT","country":"United States","dxcc":291,"cqZone":5,"ituZone":8,"image":"https://cdn-xml.qrz.com/a/w1abc/w1abc.jpg","lat":41.7147,"lon":-72.7272,"county":"Hartford","iota":"NA-001","email":"dave.smith@example.org","web":"https://www.example.org/w1abc","qslVia":"W1MGR","lotw":true,"eqsl":false,"paperQsl":true,"utcOffset":-5.0}"#,
        );
        c.conn.execute_batch("BEGIN").unwrap();
        for i in 0..(CAPACITY + 500) {
            c.put(&format!("K{i}ABC"), &answer, 1).unwrap();
        }
        c.conn.execute_batch("COMMIT").unwrap();
        assert_eq!(c.len().unwrap(), CAPACITY);
        let bytes = std::fs::metadata(&path).unwrap().len();
        eprintln!(
            "callbook cache: {CAPACITY} answers of {} bytes = {bytes} bytes on disk",
            answer.len()
        );
        assert!(bytes < 8 * 1024 * 1024, "{bytes} bytes for a full cache");
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
