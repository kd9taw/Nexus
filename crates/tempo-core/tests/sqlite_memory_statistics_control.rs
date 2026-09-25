//! The positive control for `sqlite_memory_statistics.rs`: in a process where a connection was
//! opened BEFORE the logbook's first, SQLite has initialised with its memory statistics on, the
//! logbook's attempt to turn them off comes too late, and the same measurement reads them on —
//! so a zero there is the setting, not a counter that never moves.

use tempo_core::logbook::sqlite::LogDb;

#[test]
fn a_connection_opened_before_the_logbook_s_leaves_the_statistics_on() {
    let first = rusqlite::Connection::open_in_memory().expect("a connection of its own, first");
    let db = LogDb::open(&LogDb::memory_name()).expect("an in-memory store opens");
    assert!(
        db.row_count().expect("it reads") == 0,
        "premise: SQLite has worked"
    );
    // SAFETY: a plain read of SQLite's statistics counter.
    let used = unsafe { rusqlite::ffi::sqlite3_memory_used() };
    assert!(
        used > 0,
        "SQLite counted {used} bytes: its statistics were on"
    );
    drop(first);
}
