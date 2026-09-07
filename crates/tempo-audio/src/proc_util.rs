//! Reusable child-process discipline, lifted verbatim out of [`crate::rigctld_proc`].
//!
//! Everything here is generic over *which* daemon is being managed: the "does it actually
//! run" probe, the PATH / bundled-binary resolver, the Windows kill-on-job-close assignment,
//! the Unix orphan ledger, and the generic bounded stderr ring. `rigctld`, `rotctld` and
//! `rigctl` call through it (see [`crate::rigctld_proc`]); the Winlink ARDOP modem process
//! (`tempo_app::ardop_proc`, a later batch) will reuse the same machinery rather than
//! re-derive it.
//!
//! **This module carries no rig- or Hamlib-specific vocabulary.** The stderr *ranking*
//! (`Explains`, `CAUSE_FRAGMENTS`, …) stays in `rigctld_proc`; only the ring *mechanism*
//! ([`StderrRing`], [`said_line`]) lives here, rank-parameterised, so a different daemon's
//! stderr vocabulary plugs in without editing this file.

/// Return the first `dirs` entry that contains a file named `bin_name`, absolute path — the
/// search core of [`resolve_rigctld`] / [`resolve_rotctld`] / [`resolve_rigctl`]'s fallback,
/// factored out so it can be driven by a fixture list in tests instead of the real
/// [`HAMLIB_SEARCH_DIRS`].
#[cfg(unix)]
pub fn find_in_dirs(dirs: &[&str], bin_name: &str) -> Option<std::ffi::OsString> {
    dirs.iter()
        .map(|dir| std::path::Path::new(dir).join(bin_name))
        .find(|p| p.is_file())
        .map(std::path::PathBuf::into_os_string)
}

/// Does `path_var` (a `PATH`-shaped, `:`-joined list of directories) resolve `bin_name`? Takes
/// the value as a parameter instead of reading the real process environment, so a test can drive
/// it without mutating global state that other tests (the fixture stand-in below) already depend
/// on being left alone.
#[cfg(unix)]
pub fn path_has(path_var: &std::ffi::OsStr, bin_name: &str) -> bool {
    std::env::split_paths(path_var).any(|dir| dir.join(bin_name).is_file())
}

/// Does `bin` actually RUN, or does it merely exist?
///
/// Existence is not usability, and the gap is not hypothetical: a Hamlib built from source and
/// installed under `~/.local` keeps the configured `--prefix` (`/usr/local/lib/libhamlib.4.dylib`)
/// as its dylib load path, so every `rigctl*` binary is executable, first on `PATH`, and dies at
/// `dyld` load with *"Library not loaded"* before `main`. Found on a real station on 2026-08-13:
/// CAT was dead with no usable diagnosis, because the PATH existence check said yes and the
/// [`HAMLIB_SEARCH_DIRS`] fallback — which would have found a working Homebrew Hamlib one
/// directory later — was never consulted.
///
/// `--version` is the probe: it touches no serial port, binds no TCP port, and returns at once.
///
/// **A NON-ZERO EXIT IS A PASS — only a SIGNAL is a failure.** This is the whole subtlety, and
/// getting it wrong silently breaks the [`resolve_rigctld`] contract that an operator's or a
/// test's own binary outranks Nexus's guesses. A wrapper script, a version pin, or the stand-in
/// in `service`'s `an_ordinary_connect_failure_carries_what_the_daemon_said` need not implement
/// `--version` at all — that fixture answers only `-vvv` and exits 9 for anything else — and
/// rejecting them for it would hand Nexus's own guess a veto over a deliberate choice. A binary
/// whose libraries cannot be loaded fails differently: `dyld` calls `abort()` before `main`, so
/// the process is KILLED BY SIGABRT rather than returning an exit code (observed: 134, i.e.
/// 128+6, from the `~/.local` Hamlib above). Signal death, failure to exec at all, and a hang are
/// the three "this cannot run" verdicts; every ordinary exit status means it ran.
///
/// The wait is BOUNDED because this sits on the CAT-connect path: a candidate that hangs must not
/// hang Nexus, so it is killed and treated as unusable rather than waited on.
#[cfg(unix)]
pub fn runs_ok(bin: &std::ffi::OsStr) -> bool {
    runs_ok_within(bin, std::time::Duration::from_millis(2_000))
}

/// [`runs_ok`] with the wait budget supplied.
///
/// ⚠️ SPLIT OUT FOR THE TEST, AND THE PRODUCTION BUDGET IS UNCHANGED. The 2 s bound is right on
/// the CAT-connect path — a candidate that hangs must not hang Nexus — but it is a WALL CLOCK,
/// and a wall clock in a test is a race against the machine. Under a full `cargo test
/// --workspace`, with every core busy on other crates, spawning `/bin/sh` and collecting its exit
/// can take longer than two seconds; the child is then killed and a perfectly good binary reports
/// as unrunnable. That is what made
/// `runs_ok_accepts_a_nonzero_exit_and_rejects_only_a_signal` fail only in the full run and pass
/// alone, serially or in parallel — measured, after it went red on two consecutive workspace runs
/// while its own module passed every way it could be run on its own.
///
/// The test passes a generous budget so it measures the VERDICT LOGIC — ran vs died by signal vs
/// could not exec — which is the thing it exists to pin. Nothing about what ships changes.
#[cfg(unix)]
pub(crate) fn runs_ok_within(bin: &std::ffi::OsStr, budget: std::time::Duration) -> bool {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    // ⚠️ "COULD NOT EXEC RIGHT NOW" IS NOT "THIS BINARY IS UNUSABLE", and collapsing the two is
    // the same mistake EINTR was on the CAT read path. This function's verdict decides whether
    // Nexus ABANDONS an operator's deliberately chosen rigctld and substitutes its own guess, so
    // every transient must be retried rather than answered.
    //
    // Three are transient, and the third is the one that was actually biting:
    //   WouldBlock   EAGAIN from `fork` — the system is briefly out of process/thread slots.
    //   Interrupted  EINTR — a signal landed mid-call.
    //   ExecutableFileBusy  ETXTBSY — someone holds the file OPEN FOR WRITING. On Linux `execve`
    //                refuses that, and the writer does not have to be us: a fork in ANOTHER
    //                thread between a `File::create` and its close hands the child an inherited
    //                write descriptor, and the exec fails until that child's fd goes away. So it
    //                is a property of the moment, not of the binary — and it is exactly what a
    //                fresh install hits, where Nexus has just unpacked rigctld and is probing it.
    //
    // ETXTBSY was found by a flaky test rather than reasoned out: the assertion prints the raw
    // spawn error, and after 12 runs it said `kind=ExecutableFileBusy err=Text file busy`. An
    // earlier pass had guessed a fork/timeout cause and its own control DISPROVED it. Retrying
    // the wrong errno would have left this exactly as broken while looking fixed.
    //
    // ENOENT / EACCES / bad arch are real answers about the binary and still return immediately.
    let spawn = |_: ()| {
        Command::new(bin)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    };
    let mut child = match spawn(()) {
        Ok(c) => c,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::Interrupted
                    | std::io::ErrorKind::ExecutableFileBusy
            ) =>
        {
            // A few short retries, not one: ETXTBSY lasts as long as some other process holds
            // the write descriptor, which is a fork's worth of time and can outlast a single
            // 50 ms nap. Bounded so a genuinely busy file still answers quickly.
            let mut got = None;
            for _ in 0..5 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(c) = spawn(()) {
                    got = Some(c);
                    break;
                }
            }
            match got {
                Some(c) => c,
                None => return false,
            }
        }
        Err(_) => return false, // not executable at all (ENOENT / EACCES / bad arch)
    };
    let deadline = std::time::Instant::now() + budget;
    loop {
        match child.try_wait() {
            // Exited on its own terms — ANY code, see above. Only signal death disqualifies.
            Ok(Some(status)) => return status.signal().is_none(),
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(_) => return false,
        }
    }
}
