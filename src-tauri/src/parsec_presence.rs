//! The Parsec presence watcher — the thread half of presence mode.
//!
//! Once a second it reads the setting (a brief engine lock), reads Parsec's host log with NO
//! lock held ([`tempo_app::presence::ParsecLog`]), and passes only the verdict in
//! ([`tempo_app::engine::Engine::apply_parsec_presence`], a second brief lock). The file I/O
//! never runs under the engine lock, so it cannot slow the radio loop, which shares that lock;
//! what the verdict means for the transmitter is decided in `engine/parsec_presence.rs`.
//!
//! Started only on Windows, where the Parsec host runs, but compiled and linted everywhere: the
//! platform test is `cfg!(windows)`, a plain boolean, so no part of this hides from the Linux
//! build the way a `#[cfg(windows)]` body would.
#![deny(warnings, clippy::all)]

use std::path::PathBuf;
use std::time::Duration;
use tempo_app::engine::engine_lock;
use tempo_app::presence::{log_candidates, ParsecLog};

/// How often Parsec's log is read — "about once a second" (the operator's spec). Parsec's own
/// drop detection is the slow term, not this.
const POLL: Duration = Duration::from_secs(1);

/// Start the watcher. Does nothing off Windows; on Windows it runs for the life of the app and
/// idles (no file access at all) while presence mode is switched off.
pub(crate) fn spawn(engine: crate::SharedEngine) {
    if !cfg!(windows) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("parsec-presence".into())
        .spawn(move || watch(&engine));
    if let Err(e) = spawned {
        eprintln!("[parsec] presence watcher did not start: {e}");
    }
}

fn watch(engine: &crate::SharedEngine) {
    let mut log: Option<ParsecLog> = None;
    loop {
        std::thread::sleep(POLL);
        let on = engine_lock(engine).settings().parsec_presence_stop;
        let verdict = if on {
            let log = log.get_or_insert_with(|| ParsecLog::new(candidates()));
            let found_before = log.path().is_some();
            let verdict = log.poll();
            if !found_before {
                if let Some(path) = log.path() {
                    tempo_core::applog::info("parsec", &format!("watching {}", path.display()));
                }
            }
            Some(verdict)
        } else {
            // Forget the file as well as the state: switched back on, it is read afresh.
            log = None;
            None
        };
        engine_lock(engine).apply_parsec_presence(verdict);
    }
}

/// `%APPDATA%\Parsec\log.txt` (a per-user install) and `%ProgramData%\Parsec\log.txt` (per
/// computer) — the two places Parsec's own documentation names.
fn candidates() -> Vec<PathBuf> {
    log_candidates(
        std::env::var_os("APPDATA").map(PathBuf::from),
        std::env::var_os("ProgramData").map(PathBuf::from),
    )
}
