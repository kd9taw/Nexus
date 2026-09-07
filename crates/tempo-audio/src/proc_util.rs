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

/// Every place a **bundled** Hamlib tool can sit, relative to the directory holding the Nexus
/// executable, most-specific first. `exe_name` is the executable's own file name, which is also
/// the directory Tauri names on Linux (`usr/lib/<productName>/`).
///
/// ⚠️ **THE LINUX AND macOS ENTRIES ARE NOT DECORATION — until 2026-08-24 they were missing and
/// the AppImage shipped Hamlib's five licence texts and no Hamlib.** The Windows installer has
/// always carried rigctld, and the entries above covered it because Tauri puts the Windows
/// resources beside the .exe. Every other platform puts them somewhere else, so the bundled copy
/// was unreachable even once the build staged it:
///
/// | bundle          | executable                  | resources                              |
/// |-----------------|-----------------------------|----------------------------------------|
/// | Windows NSIS    | `<root>/Nexus.exe`          | `<root>/resources/`                    |
/// | Linux deb + App | `usr/bin/Nexus`             | `usr/lib/Nexus/resources/`             |
/// | macOS .app      | `Contents/MacOS/Nexus`      | `Contents/Resources/`                  |
///
/// Kept as ONE list because the three resolvers (`rigctld`, `rotctld`, `rigctl`) held three
/// verbatim copies of it, which is how the two new entries would have gone into two of them.
pub fn bundled_candidates(exe_name: &str, tool: &str) -> Vec<String> {
    vec![
        format!("hamlib/{tool}.exe"),
        format!("resources/hamlib/{tool}.exe"),
        format!("{tool}.exe"),
        format!("hamlib/{tool}"),
        format!("resources/hamlib/{tool}"),
        // Linux .deb and AppImage: usr/bin/<exe> → usr/lib/<exe>/resources/
        format!("../lib/{exe_name}/resources/hamlib/{tool}"),
        // macOS .app: Contents/MacOS/<exe> → Contents/Resources/resources/
        //
        // ⚠️ THE `resources/` COMPONENT IS NOT OPTIONAL (#190). tauri.conf.json maps
        // "resources/hamlib/*" → "resources/hamlib/", and that destination is relative to the
        // bundle's own resource dir, so the tools land at Contents/Resources/resources/hamlib/.
        // Shipping only the shorter path meant the correctly-staged, correctly-signed Hamlib
        // inside every .app was never looked at: the resolver fell through to PATH and the
        // Homebrew/MacPorts dirs, and on a Mac that had never installed Hamlib the spawn
        // failed with "could not start its own rigctl" — i.e. NO CAT on 100% of fresh macOS
        // installs, while Windows and Linux (both of which carry the component) were fine.
        format!("../Resources/resources/hamlib/{tool}"),
        // The pre-#190 path, kept as a harmless fallback: it costs one stat, and an older or
        // hand-assembled bundle that really does put them here still resolves.
        format!("../Resources/hamlib/{tool}"),
    ]
}

/// The first bundled candidate for `tool` that exists under `dir`, or `None`.
///
/// Split from [`resolve_rigctld`] and its two siblings so the layouts above can be driven by a
/// fixture directory in tests — `current_exe()` cannot be pointed at one.
pub fn find_bundled_in(
    dir: &std::path::Path,
    exe_name: &str,
    tool: &str,
) -> Option<std::ffi::OsString> {
    for cand in bundled_candidates(exe_name, tool) {
        let p = dir.join(&cand);
        if p.is_file() {
            return Some(p.into_os_string());
        }
    }
    None
}

/// [`find_bundled_in`] against the running executable's own directory.
pub fn find_bundled(tool: &str) -> Option<std::ffi::OsString> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = exe.file_stem()?.to_str()?.to_string();
    find_bundled_in(dir, &name, tool)
}

/// Admit a bundled candidate only if it can actually RUN — existence is not resolution.
///
/// ⚠️ WHY — the mac 1.10.0 CAT regression. "Found the bundled copy" SUPPRESSES the
/// PATH/[`HAMLIB_SEARCH_DIRS`] fallback, so a bundled binary that dies before `main` costs
/// more than nothing: it silently discards the operator's own working Hamlib. Concretely:
/// fetch-hamlib-unix.sh repointed the libusb reference in the four TOOLS but not in
/// libhamlib.4.dylib itself, so the shipped library still named
/// /opt/homebrew/opt/libusb/lib/libusb-1.0.0.dylib — present on every CI runner (the #190
/// release gate ran `rigctld --version` there and stayed green) and absent on a Mac that
/// never installed Homebrew's libusb, where dyld killed rigctld AND every one-shot rigctl
/// ladder rung with SIGABRT before `main`. 1.9.2 had worked on the same machine because the
/// pre-#190 resolver never FOUND the bundled tools and fell through to the operator's
/// brew/MacPorts copy; #190 made it find them, and existence-only resolution turned a
/// packaging defect into "the rig never answered at any speed". PATH and search-dir
/// candidates have cleared [`runs_ok`] since 2026-08-13 for exactly this reason; this closes
/// the same hole for the bundled branch. (Unix-only by construction, like `runs_ok`: the
/// failure class — an absolute install-name into a package manager's prefix — does not exist
/// in PE loading, and Windows keeps its existence-only resolution untouched.)
#[cfg(unix)]
pub fn bundled_if_runnable(tool: &str, p: std::ffi::OsString) -> Option<std::ffi::OsString> {
    if runs_ok(&p) {
        return Some(p);
    }
    crate::civ::diag::note(&format!(
        "{tool}: the bundled copy at {} cannot run (killed by a signal — typically a library \
         it needs is missing or unloadable); trying PATH and the package-manager directories \
         instead",
        p.to_string_lossy()
    ));
    None
}
