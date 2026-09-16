//! The daemon gate shared by the two Hamlib integration suites.
//!
//! `rigctld_dummy.rs` and `rotctld_pass.rs` exist to test our CAT/rotator code
//! against a REAL daemon rather than against our beliefs about the protocol, so
//! a missing daemon is the one condition that empties them completely. It has
//! two very different meanings, and the bug this module fixes was treating them
//! alike — both arms returned, so both reported a pass.
//!
//! * **A contributor's box with no Hamlib.** Skipping is reasonable. But the
//!   skip must not READ as a pass, and the old one did: `cargo test` captures
//!   `eprintln!` from a passing test, so the `SKIP:` line went to a buffer
//!   nobody ever saw and `rigctld_dummy` reported *12 passed … finished in
//!   0.05 s* having executed nothing — the PTT-reaches-the-wire and
//!   failed-unkey-stays-latched transmit-path checks among them. (Measured
//!   2026-09-15, against 3.43 s for the same twelve on a real Hamlib 4.5.5
//!   dummy. The duration was the only tell, and reading a duration is not a
//!   gate.) The notice below goes to the process's real stderr, which libtest
//!   does not capture, so it is on screen in an ordinary run.
//! * **GitHub Actions.** There is no such thing as a reasonable skip here.
//!   `ci.yml`'s setup step installs `libhamlib-utils` precisely so these suites
//!   are real, and its own comment says dropping the package "silently removes
//!   the coverage". A missing daemon there therefore means that install broke —
//!   a condition that must go RED. So there this gate refuses to skip at all.
//!
//! **The discriminator is `GITHUB_ACTIONS`, deliberately not `CI`.** The rule is
//! "fail where the daemon was PROMISED", and it is `ci.yml`'s own apt line that
//! promises it. `CI` is a much weaker claim — anything that believes it is
//! gating sets it, `scripts/gates` included, and that runs on developer boxes
//! with no Hamlib. Keying on `CI` would make the local gate permanently red
//! there, and a gate that is always red is one everybody learns to read past:
//! it would trade this silent-green for a louder way to lose the same coverage.

use std::io::Write;
use std::process::{Command, Stdio};

/// The one environment whose own workflow guarantees the daemon is installed.
/// GitHub Actions sets `GITHUB_ACTIONS=true` in every step of every job.
fn daemon_was_promised() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some_and(|v| !v.is_empty())
}

/// Locate `bin`: `$env_override` (a path to the binary) first, then PATH,
/// probed with `--version` so a present-but-unrunnable file does not count.
fn locate(bin: &str, env_override: &str) -> Option<String> {
    if let Ok(p) = std::env::var(env_override) {
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    Command::new(bin)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        .then(|| bin.to_string())
}

/// Resolve the daemon, or decide what its absence means.
///
/// `Some(path)` → run the test. `None` → skip, with the reason already on the
/// real stderr. **On GitHub Actions this never returns `None`** — it panics,
/// because a suite that reports twelve passes having opened no socket is worse
/// than no suite at all.
pub fn require_daemon(bin: &str, env_override: &str) -> Option<String> {
    if let Some(found) = locate(bin, env_override) {
        return Some(found);
    }
    if daemon_was_promised() {
        panic!(
            "no `{bin}` on PATH, and ${env_override} is unset or names nothing that exists — \
             but this is GitHub Actions, where `ci.yml`'s setup step installs `libhamlib-utils` \
             precisely so this suite is real. Its absence means that install was dropped or \
             failed, and THAT is the bug this failure is reporting: every test in this suite \
             would otherwise report a pass having executed nothing, the transmit-path checks \
             included. To run against a binary elsewhere set ${env_override}=/path/to/{bin}."
        );
    }
    // Straight to the real stderr. `eprintln!` here is captured by libtest for a
    // passing test, which is exactly how the old notice went unread.
    let thread = std::thread::current();
    let test = thread.name().unwrap_or("<unnamed test>");
    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "!! NOT RUN, and NOT A PASS: {test} — no `{bin}` on this box, so it asserted nothing. \
         Install libhamlib-utils, or set ${env_override}=/path/to/{bin}."
    );
    let _ = err.flush();
    None
}
