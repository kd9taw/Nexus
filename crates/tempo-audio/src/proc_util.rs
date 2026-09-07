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

/// One stderr line as the ring keeps it — trimmed, `None` if there is nothing left.
///
/// **Bytes, not `&str`, and that signature is the fix.** The drain used
/// `BufRead::lines().map_while(Result::ok)`, and `lines()` yields `Err(InvalidData)` for a line
/// that is not UTF-8. `map_while` STOPS at the first `Err` — so such a line did not merely go
/// missing, it ended the drain thread and every line after it for the life of the daemon.
///
/// Hamlib emits exactly that line in the one case where it diagnoses a **wrong baud**: it
/// quotes the rig's own bytes back. Observed against the bundled rigctld 4.7.1 with mis-framed
/// replies — `newcat_get_cmd: Command is not correctly terminated '…'` followed by ~200 bytes
/// of the rig's garbage, which is arbitrary and very rarely valid UTF-8 (the capture is
/// `tests/fixtures/rigctld/wrong_baud.log`, and it is not a UTF-8 file). So the fault Nexus
/// most needed explaining was the fault that silenced the whole mechanism. `from_utf8_lossy`
/// keeps the sentence and marks the garbage; the marks are themselves the diagnosis — bytes
/// came back and they were rubbish, which is what a baud mismatch looks like from here.
pub fn said_line(raw: &[u8]) -> Option<String> {
    let line = String::from_utf8_lossy(raw).trim().to_string();
    (!line.is_empty()).then_some(line)
}

use std::collections::VecDeque;

/// A bounded window of the newest stderr lines, plus — kept out of that window's reach —
/// the single best-ranked line of the whole session. Generic over the rank type so a caller
/// supplies its own "what does this line explain" ordering; see `rigctld_proc::explains` for
/// the rig-error ranking and the long rationale on why one buffer cannot answer both
/// "what is happening now" and "what went wrong".
pub struct StderrRing<R: Ord + Copy> {
    window: VecDeque<String>,
    best: Option<(R, String)>,
    cap: usize,
    rank: fn(&str) -> R,
}

impl<R: Ord + Copy> StderrRing<R> {
    pub fn new(cap: usize, rank: fn(&str) -> R) -> Self {
        StderrRing {
            window: VecDeque::new(),
            best: None,
            cap,
            rank,
        }
    }

    pub fn push(&mut self, line: String) {
        let rank = (self.rank)(&line);
        if self.best.as_ref().is_none_or(|(best, _)| rank > *best) {
            self.best = Some((rank, line.clone()));
        }
        if self.window.len() == self.cap {
            self.window.pop_front();
        }
        self.window.push_back(line);
    }

    pub fn lines(&self) -> Vec<String> {
        self.best
            .iter()
            .map(|(_, l)| l)
            .filter(|l| !self.window.contains(l))
            .chain(self.window.iter())
            .cloned()
            .collect()
    }
}

/// Place `child` in a new Job Object set to kill its processes when the job
/// handle closes, so rigctld dies with Tempo (clean exit, crash, or detached-
/// thread teardown). Returns the job HANDLE as an `isize` (0 on any failure, in
/// which case we just fall back to the Drop-time kill).
#[cfg(windows)]
pub fn assign_kill_on_close_job(child: &std::process::Child) -> isize {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    unsafe {
        let job = CreateJobObjectW(core::ptr::null(), core::ptr::null());
        if job.is_null() {
            return 0;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = core::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set_ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            core::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set_ok == 0 || AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
            CloseHandle(job);
            return 0;
        }
        job as isize
    }
}

/// Startup half of the Unix orphan guarantee: remember where the PID ledger lives and kill
/// any rigctld/rotctld a PREVIOUS, now-dead Nexus left running (see [`orphan_ledger`]).
/// Call once, before the first daemon spawn, so a stale daemon's serial/TCP ports are free
/// again by the time this instance wants them. On Windows this is a no-op — the Job Object
/// above already guarantees no daemon outlives the process, however it dies.
pub fn init_orphan_ledger(dir: std::path::PathBuf) {
    #[cfg(unix)]
    orphan_ledger::init(dir);
    #[cfg(not(unix))]
    let _ = dir;
}

/// Quit-path half of the Unix orphan guarantee: TERM every daemon this process spawned and
/// has not dropped (see [`orphan_ledger::kill_leftovers`]). For the wedged quit, where the
/// radio thread is still blocked in a CAT read holding the [`RigctldProc`] when the process
/// exits and `Drop` therefore never runs. No-op on Windows (Job Object) and when nothing is
/// left (the ordinary case — `Drop` already deregistered everything).
pub fn kill_leftover_daemons() {
    #[cfg(unix)]
    orphan_ledger::kill_leftovers();
}

/// The Unix ledger of spawned daemons — what stands in for the Windows Job Object.
///
/// **The gap this closes** (mac QA audit, 2026-08-17): on Windows the daemon dies with the
/// process *however the process dies*, because the OS closes the kill-on-close job handle.
/// On macOS/Linux the only teardown was [`RigctldProc`]'s `Drop`, which never runs when
/// (a) the process dies on a signal — the Fortran-AV crash class presents as SIGSEGV here;
/// (b) the operator force-quits; (c) the quit path times out with the radio thread still
/// blocked in a CAT read holding the handle. The orphan then keeps the operator's serial
/// port open and its TCP port bound, and the NEXT launch can land on it — the crossed-CAT
/// shape `is_alive` warns about.
///
/// Two bounded mechanisms, deliberately NOT a supervisor:
/// * **The PID ledger.** Every spawn writes `<ledger>/<daemon-pid>.pid` containing
///   `"<our-pid> <binary-name>"`; `Drop` removes it. [`init`] sweeps the ledger at the next
///   launch and kills any recorded daemon whose spawning Nexus is gone — covering the three
///   no-`Drop` deaths above at the exact moment the collision would otherwise happen.
/// * **The in-process registry.** [`kill_leftovers`] (the quit path, after the radio loop
///   has been told to stop) TERMs anything not yet dropped — the wedged quit, handled
///   before the process exits rather than left for the next launch.
///
/// PID reuse is the hazard of any kill-by-recorded-pid, and both directions are covered:
/// * the registry holds only OUR direct children, none of them reaped when we signal
///   (`Drop` deregisters before it reaps), so those PIDs cannot have been recycled;
/// * the sweep kills a recorded PID only when `ps` says the process behind it is still the
///   recorded *binary*; a recycled PID reads as something else and the record is dropped
///   without a kill. A recycled PARENT pid makes the parent look alive, which merely keeps
///   the record for a later launch — the conservative failure, never a kill on a guess.
///
/// When [`init`] was never called (unit tests, headless tools) nothing is written and the
/// sweep never runs — behaviour is exactly pre-ledger.
#[cfg(unix)]
pub(crate) mod orphan_ledger {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    /// Where the records live. Set once by [`init`]; `None` = ledger disabled.
    static LEDGER_DIR: OnceLock<PathBuf> = OnceLock::new();
    /// Daemons this process has spawned and not yet dropped: `(daemon pid, binary name)`.
    static LIVE: Mutex<Vec<(u32, &'static str)>> = Mutex::new(Vec::new());

    /// The record body: `"<parent-pid> <binary-name>"`. The daemon's own pid is the
    /// FILENAME (`<pid>.pid`), so concurrent instances can never write the same record.
    fn format_record(parent_pid: u32, bin: &str) -> String {
        format!("{parent_pid} {bin}")
    }

    /// Parse a record body. Anything that is not exactly two fields with a numeric first
    /// is `None` — a garbled record must never produce a pid to kill.
    fn parse_record(s: &str) -> Option<(u32, String)> {
        let mut it = s.split_whitespace();
        let parent = it.next()?.parse().ok()?;
        let bin = it.next()?.to_string();
        if it.next().is_some() {
            return None;
        }
        Some((parent, bin))
    }

    /// The daemon pid a ledger filename names, or `None` for any file that is not ours.
    fn pid_of_filename(name: &str) -> Option<u32> {
        name.strip_suffix(".pid")?.parse().ok()
    }

    /// What the sweep does with one record. Pure — the whole kill decision in one place.
    #[derive(Debug, PartialEq, Eq)]
    enum Verdict {
        /// The spawning Nexus is still running (a live sibling instance, or a recycled
        /// parent pid): not ours to touch.
        Keep,
        /// The daemon is gone too (or its pid now belongs to some unrelated process):
        /// nothing to kill, drop the stale record.
        Drop,
        /// Parent dead, and the pid still runs the recorded binary: a true orphan.
        KillAndDrop,
    }

    /// `daemon_comm` is what `ps -o comm=` reports for the daemon's pid (`None` = no such
    /// process). Compared by basename because macOS prints the full executable path where
    /// Linux prints the bare name.
    fn record_verdict(parent_alive: bool, daemon_comm: Option<&str>, bin: &str) -> Verdict {
        if parent_alive {
            return Verdict::Keep;
        }
        match daemon_comm {
            Some(comm) if Path::new(comm).file_name().is_some_and(|f| f == bin) => {
                Verdict::KillAndDrop
            }
            _ => Verdict::Drop,
        }
    }

    /// One pass over the ledger. The probes and the kill are parameters so the decision
    /// path is drivable from a test with no processes involved; [`init`] passes the real
    /// ones. Files that are not records, and records that do not parse, are cleaned up or
    /// skipped — the ledger must not accrete junk, and junk must never cause a kill.
    fn sweep(
        dir: &Path,
        parent_alive: &dyn Fn(u32) -> bool,
        daemon_comm: &dyn Fn(u32) -> Option<String>,
        kill: &mut dyn FnMut(u32),
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(daemon_pid) = name.to_str().and_then(pid_of_filename) else {
                continue; // not a ledger record — leave foreign files alone
            };
            let parsed = std::fs::read_to_string(entry.path())
                .ok()
                .and_then(|s| parse_record(s.trim()));
            let Some((parent, bin)) = parsed else {
                let _ = std::fs::remove_file(entry.path()); // garbled: unusable, remove
                continue;
            };
            match record_verdict(
                parent_alive(parent),
                daemon_comm(daemon_pid).as_deref(),
                &bin,
            ) {
                Verdict::Keep => {}
                Verdict::Drop => {
                    let _ = std::fs::remove_file(entry.path());
                }
                Verdict::KillAndDrop => {
                    kill(daemon_pid);
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    /// What `ps` calls `pid`, or `None` when no such process exists. `ps -p <pid> -o comm=`
    /// answers liveness and identity in one portable call (macOS and Linux both have it in
    /// the launchd/systemd default PATH); a zombie still lists, which for the PARENT check
    /// is the conservative direction (its records wait for the next launch).
    fn proc_comm(pid: u32) -> Option<String> {
        let out = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// Send `sig` (a `kill(1)` signal argument, e.g. `"-TERM"`) to `pid`. Best-effort.
    fn send_signal(pid: u32, sig: &str) {
        let _ = std::process::Command::new("kill")
            .args([sig, &pid.to_string()])
            .status();
    }

    /// Kill a confirmed orphan: TERM (rigctld exits promptly and closes the serial port
    /// cleanly), then a bounded wait so OUR spawn moments later finds the ports actually
    /// free, then one KILL if it lingered. Bounded because this runs on the startup path.
    fn kill_stale(pid: u32) {
        send_signal(pid, "-TERM");
        for _ in 0..10 {
            if proc_comm(pid).is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        send_signal(pid, "-KILL");
    }

    /// Remember the ledger dir and sweep what a previous dead instance left. See the
    /// module doc; called via [`super::init_orphan_ledger`].
    pub(crate) fn init(dir: PathBuf) {
        let _ = std::fs::create_dir_all(&dir);
        sweep(
            &dir,
            &|pid| proc_comm(pid).is_some(),
            &proc_comm,
            &mut kill_stale,
        );
        let _ = LEDGER_DIR.set(dir);
    }

    /// A daemon was spawned: register it and write its ledger record.
    pub(crate) fn record(daemon_pid: u32, bin: &'static str) {
        if let Ok(mut live) = LIVE.lock() {
            live.push((daemon_pid, bin));
        }
        if let Some(dir) = LEDGER_DIR.get() {
            let _ = std::fs::write(
                dir.join(format!("{daemon_pid}.pid")),
                format_record(std::process::id(), bin),
            );
        }
    }

    /// A daemon is being dropped (and killed by its `Drop`): deregister + remove the record.
    pub(crate) fn forget(daemon_pid: u32) {
        if let Ok(mut live) = LIVE.lock() {
            live.retain(|(pid, _)| *pid != daemon_pid);
        }
        if let Some(dir) = LEDGER_DIR.get() {
            let _ = std::fs::remove_file(dir.join(format!("{daemon_pid}.pid")));
        }
    }

    /// TERM everything still registered — the quit path's backstop for handles whose `Drop`
    /// will never run. Safe against pid reuse (un-reaped children, see the module doc).
    /// Fire-and-forget: the process is exiting and must not block here; the ledger records
    /// deliberately STAY on disk, so if a TERM'd daemon somehow lingers, the next launch's
    /// sweep gets a second, identity-checked look instead of nothing.
    pub(crate) fn kill_leftovers() {
        let leftovers = LIVE
            .lock()
            .map(|mut live| std::mem::take(&mut *live))
            .unwrap_or_default();
        for (pid, _) in leftovers {
            send_signal(pid, "-TERM");
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The kill decision, exhaustively: a kill may happen only for a dead parent AND a
        /// pid that still runs the recorded binary. Everything else keeps or drops.
        #[test]
        fn a_kill_needs_a_dead_parent_and_a_matching_binary() {
            // Parent alive (live sibling instance): never touched, even if the daemon looks right.
            assert_eq!(
                record_verdict(true, Some("rigctld"), "rigctld"),
                Verdict::Keep
            );
            // True orphan — Linux (bare name) and macOS (full path) comm forms both match.
            assert_eq!(
                record_verdict(false, Some("rigctld"), "rigctld"),
                Verdict::KillAndDrop
            );
            assert_eq!(
                record_verdict(false, Some("/opt/homebrew/bin/rigctld"), "rigctld"),
                Verdict::KillAndDrop
            );
            // Daemon pid recycled by an unrelated process: drop the record, kill NOTHING.
            assert_eq!(
                record_verdict(false, Some("firefox"), "rigctld"),
                Verdict::Drop
            );
            // Daemon already gone: just tidy up.
            assert_eq!(record_verdict(false, None, "rigctld"), Verdict::Drop);
        }

        /// The record format round-trips, and garble parses to nothing (a garbled record
        /// must never yield a parent pid to test or a kill).
        #[test]
        fn records_round_trip_and_garble_parses_to_none() {
            assert_eq!(
                parse_record(&format_record(4242, "rotctld")),
                Some((4242, "rotctld".to_string()))
            );
            assert_eq!(parse_record(""), None);
            assert_eq!(parse_record("notanumber rigctld"), None);
            assert_eq!(parse_record("123"), None);
            assert_eq!(parse_record("123 rigctld extra"), None);
            // Filenames: only `<pid>.pid` is a record.
            assert_eq!(pid_of_filename("123.pid"), Some(123));
            assert_eq!(pid_of_filename("123.tmp"), None);
            assert_eq!(pid_of_filename("x.pid"), None);
        }

        /// The sweep against a real directory, with the probes faked: the orphan is killed
        /// and its record removed; the live sibling's record survives untouched; the
        /// recycled-pid record is removed without a kill; junk files are left alone.
        #[test]
        fn the_sweep_kills_only_the_true_orphan() {
            let dir = std::env::temp_dir().join(format!(
                "nexus-orphan-sweep-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&dir).expect("temp ledger dir");
            // parent 1000 dead, pid 11 still rigctld  -> kill + drop
            std::fs::write(dir.join("11.pid"), "1000 rigctld").unwrap();
            // parent 2000 alive (sibling instance)    -> keep
            std::fs::write(dir.join("22.pid"), "2000 rigctld").unwrap();
            // parent 1000 dead, pid 33 now firefox    -> drop, no kill
            std::fs::write(dir.join("33.pid"), "1000 rotctld").unwrap();
            // garbled record                          -> drop, no kill
            std::fs::write(dir.join("44.pid"), "what even is this").unwrap();
            // not a ledger file                       -> untouched
            std::fs::write(dir.join("README.txt"), "hands off").unwrap();

            let mut killed: Vec<u32> = Vec::new();
            sweep(
                &dir,
                &|parent| parent == 2000,
                &|pid| match pid {
                    11 => Some("/usr/bin/rigctld".to_string()),
                    33 => Some("firefox".to_string()),
                    _ => None,
                },
                &mut |pid| killed.push(pid),
            );

            assert_eq!(killed, vec![11], "exactly the one true orphan dies");
            assert!(!dir.join("11.pid").exists(), "the orphan's record is gone");
            assert!(
                dir.join("22.pid").exists(),
                "the live sibling's record survives"
            );
            assert!(
                !dir.join("33.pid").exists(),
                "the recycled pid's record is gone"
            );
            assert!(!dir.join("44.pid").exists(), "garble is cleaned up");
            assert!(
                dir.join("README.txt").exists(),
                "foreign files are not ours to touch"
            );
            std::fs::remove_dir_all(&dir).ok();
        }
    }
}
