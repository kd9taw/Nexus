//! ★ SQLite's memory statistics are off in a process that opens the logbook through `LogDb`.
//!
//! With them on, every allocation SQLite makes takes one process-wide mutex to count itself,
//! and connections working at once on different threads queue on it: 960 empty in-memory stores
//! opened on 32 threads cost 35 ms of CPU each (30 s of system time in all) against 3 ms without
//! (see `quiet_memory_statistics` in `logbook/sqlite.rs`). Every engine holds a store, so a test
//! run opens hundreds of them at once.
//!
//! Its own test binary, because the setting belongs to the process and must come before
//! SQLite's first use. The positive control is `sqlite_memory_statistics_control.rs`: the same
//! measurement, in a process where a connection was opened before the logbook's, reads the
//! statistics on.

use tempo_core::logbook::sqlite::LogDb;

#[test]
fn the_logbook_s_first_open_turns_sqlite_s_memory_statistics_off() {
    let db = LogDb::open(&LogDb::memory_name()).expect("an in-memory store opens");
    assert!(
        db.row_count().expect("it reads") == 0,
        "premise: SQLite has worked"
    );
    // SAFETY: a plain read of SQLite's statistics counter.
    let used = unsafe { rusqlite::ffi::sqlite3_memory_used() };
    assert_eq!(
        used, 0,
        "SQLite counted {used} bytes: its memory statistics are on"
    );
}
