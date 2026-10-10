//! Who owns this machine's clock, is it working, and is there anything Nexus
//! should do about it.
//!
//! [`crate::service`]'s probe measures the offset; [`tempo_app::clocksync`]
//! decides what may be steered by. This module answers the third question —
//! **why** the clock is wrong and **whether the machine can be repaired** —
//! because an offset alone cannot tell a laptop that just woke up from a PC
//! whose time service somebody disabled, and those need opposite responses.
//!
//! ## The shape, and the two rules that give it that shape
//!
//! **Detection is unprivileged and read-only.** Every query here runs as an
//! ordinary user; the diagnosis costs nothing and can run on any machine. Only
//! the repair needs elevation, it is only offered for a real fault (the time
//! service stopped, not synchronised, or the clock just jumped), and it runs
//! only when the operator presses **Repair clock** (the operator's rulings,
//! 2026-10-06). It used to run on its own, and operators met an administrator
//! prompt nobody had asked for. A report that does not confirm a sync, and a
//! clock step, are faults only while the clock measures out
//! ([`REPAIR_WORTH_MS`]): each is also what a working machine shows, and the
//! button came back every pass on clocks that were right.
//!
//! **Nexus never calls a clock API.** On Windows the repair goes through
//! `sc` and `w32tm` — the OS's own tools — so W32Time stays the sole
//! disciplinarian and there is no second writer to coexist with. On Linux and
//! macOS there is no repair at all ([`detect`] says why).
//!
//! ## Parsing is pure; running commands is not
//!
//! Every `parse_*` function below takes the captured stdout of a real command
//! and is unit-tested against output captured from a live Windows machine
//! (read-only, 2026-09-09 — the fixtures at the bottom are verbatim). The thin
//! wrappers that actually spawn a process are the only untestable part, and they
//! do nothing but hand their output to those functions.
//!
//! ⚠️ **NEEDS-BENCH.** Nothing here has been run against a *bare* Windows
//! machine. The one Windows box this was developed against has NetTime
//! installed and is therefore in the stand-down case by construction: it proves
//! the third-party detection and it proves the parsers, and it proves nothing
//! about what a machine with only W32Time does. Every diagnosis reports what it
//! FOUND rather than asserting what it means.

use std::process::Stdio;
use std::time::{Duration, Instant};

/// Third-party Windows time clients. If one of these is running it owns the
/// clock and Nexus stands down entirely (guard 8) — a second disciplinarian is
/// how two programs fight over one clock and neither wins. On the machine this
/// was developed against, NetTime does 88% of the corrections.
const THIRD_PARTY_CLIENTS: [(&str, &str); 5] = [
    ("nettimeservice.exe", "NetTime"),
    ("ntpd.exe", "Meinberg NTP"),
    ("d4.exe", "Dimension 4"),
    ("chronyd.exe", "chrony"),
    // Reported 2026-09-17: an operator's waterfall froze intermittently alongside this program's
    // "Invalid Server Time! Server: time.nist.gov" dialog. The dialog is that program's own window,
    // never Nexus's — but Nexus's clock probe DOES ask time.nist.gov too (`CLOCK_SERVERS` in
    // service.rs), and NIST refuses a client that asks more than once every 4 s, so two programs
    // asking from one address can make one of them see a refusal. A client that fails and retries
    // steps the clock more often than a healthy one, and a step is what stalls a wall-clock-driven
    // decode. Image confirmed as
    // `timesync.exe` in `C:\Program Files (x86)\VOVSOFT\Time Sync`.
    // ⚠️ Catches it only while it is RESIDENT. The vendor documents neither a service nor a tray
    // mode, so a scheduled run that steps the clock and exits is invisible to `tasklist` and this
    // guard will not fire for it. That gap is real and is not closed here; closing it needs the
    // "who last wrote the clock" evidence (the Windows event log), which this module does not read.
    ("timesync.exe", "VOVSOFT Time Sync"),
];

/// Guard 3's ceiling, mirrored here so a diagnosis can say "too far out".
pub use tempo_app::clocksync::MAX_STEER_MS;

/// How far off UTC Nexus's own probe must measure the clock before a time
/// service whose report does not confirm a sync, or a clock that just stepped,
/// is offered **Repair clock**: one second, where the clock chip turns amber
/// (`CLOCK_LOG_CONCERN_MS` in `ui/src/components/TopBar.tsx`) because logged QSO
/// times are then a second out. Under it the radio loop's own correction already
/// lands transmit and decode on the UTC grid, so a repair would ask for
/// administrator rights to change nothing the operator can see. A stopped
/// service, and one that has never synchronised, are offered whatever the clock
/// reads.
pub const REPAIR_WORTH_MS: i64 = 1_000;

/// Timeout for one detection command, and it is load-bearing rather than
/// decorative.
///
/// These are local queries that normally return in milliseconds — but
/// `w32tm /query /status` reaches the Windows Time service over RPC, and a
/// **wedged** time service is precisely the population this module exists to
/// find. Without a bound, the one machine most worth diagnosing is the one that
/// hangs the probe thread, and with it every clock measurement the transmitter
/// is steered by. `std::process` has no timeout of its own, so [`capture`]
/// builds one.
const CMD_TIMEOUT: Duration = Duration::from_secs(5);

/// Who is keeping this machine's clock right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClockOwner {
    /// A third-party client is doing the work. Guard 8: **report and do
    /// nothing.** Never become the second writer.
    ThirdParty(String),
    /// The operating system's own time service.
    OsService(String),
    /// Nothing found — no service running and no client installed.
    Nobody,
    /// We could not tell (an unsupported platform, or a query that failed).
    Unknown,
}

/// What the OS time service is doing, as far as an unprivileged query can see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceState {
    /// Running and reporting a successful synchronisation.
    Synced,
    /// Running but has never reached a server, or reports a sync error.
    NotSynced,
    /// Installed but not running (a "debloat" script, an optimisation guide).
    Stopped,
    /// Could not be determined.
    Unknown,
}

/// §3.6's decision table: what, if anything, to do about what we found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repair {
    /// Nothing to do, and nothing to say beyond the diagnosis: the machine is
    /// healthy, or the clock is not ours to touch (guard 8), or there is no
    /// network for a repair to use.
    None,
    /// Diagnosis 1 — the time service is stopped or disabled.
    StartService,
    /// Diagnoses 2 and 3 — running but not synced, or the clock just stepped.
    /// `w32tm /resync`, with `/rediscover` when the source itself is suspect.
    Resync { rediscover: bool },
}

impl Repair {
    /// Does this repair fix a real fault: the time service stopped, not
    /// synchronised, or the clock just jumped? Only those are offered, and only
    /// those run when the operator presses **Repair clock** (the operator's
    /// ruling, 2026-10-06: "Only for real faults").
    pub fn fixes_a_fault(self) -> bool {
        matches!(self, Repair::StartService | Repair::Resync { .. })
    }
}

/// Everything a detection pass found, and what it concluded.
#[derive(Clone, Debug, PartialEq)]
pub struct ClockDiagnosis {
    pub owner: ClockOwner,
    pub state: ServiceState,
    /// The clock is **unvouched**: nothing has confirmed it against an external
    /// reference and the OS says so too. A Raspberry Pi has no RTC — a Pi
    /// powered off for a week boots believing it is the moment it last ran, and
    /// nothing about that LOOKS wrong: a sensible date, a running clock. Only an
    /// active check surfaces it.
    pub unvouched: bool,
    pub repair: Repair,
    /// One operator-facing line naming what was found. Assembled here because
    /// the interesting part is always the combination.
    pub detail: String,
}

impl ClockDiagnosis {
    /// The honest answer on a platform or in a state we could not read.
    fn unknown(detail: impl Into<String>) -> Self {
        Self {
            owner: ClockOwner::Unknown,
            state: ServiceState::Unknown,
            unvouched: false,
            repair: Repair::None,
            detail: detail.into(),
        }
    }
}

// ── The decision (§3.6), as a pure function ───────────────────────────────────

/// Inputs to [`decide`], gathered by whichever platform probe ran.
#[derive(Clone, Debug, Default)]
pub struct Findings {
    pub third_party: Option<String>,
    pub service_present: bool,
    pub service_running: bool,
    pub synced: bool,
    /// The service's report neither confirms a sync nor says it never had one: a
    /// good sync is on record but the latest attempt did not take (a sync error,
    /// the state machine in Hold or Spike), or no report could be read (the query
    /// failed or timed out, an error line came back, or Windows printed the report
    /// in another language: only the English labels are read). A working service
    /// shows this between two of its polls as often as a broken one does, so the
    /// measured clock decides ([`REPAIR_WORTH_MS`]). Only Windows sets it.
    pub sync_unconfirmed: bool,
    /// Did Nexus's own SNTP probe reach a server this round?
    pub probe_reached_network: bool,
    /// The offset the probe measured, when it has one.
    pub measured_offset_ms: Option<i64>,
    /// True when this platform has no repair path at all (Linux, macOS).
    pub repairs_available: bool,
    /// The OS stepped the system clock since the last pass — resume from sleep
    /// or hibernate, or a time service that stepped rather than slewed. **This
    /// is the highest-value repair in the programme** and it is free: §8.2(a)'s
    /// jump detector already has to exist to stop a stale offset being applied,
    /// and a clock jump IS the resume signal. Microsoft names battery-powered
    /// portable devices as a population W32Time does not serve well, and on the
    /// machine this was studied on it took over six hours of uptime to
    /// synchronise after one such step.
    pub just_stepped: bool,
}

/// §3.6's decision table. Pure, so every row is a test rather than a machine.
///
/// The ORDER is the design. Guard 8 comes first because a machine with a
/// third-party client needs nothing from us whatever else is true of it, and "no
/// network" comes before any repair because a repair without a server to reach
/// is noise.
pub fn decide(f: &Findings) -> ClockDiagnosis {
    // 7 — a third-party client owns the clock. Report, stand down.
    if let Some(who) = &f.third_party {
        return ClockDiagnosis {
            owner: ClockOwner::ThirdParty(who.clone()),
            state: if f.synced {
                ServiceState::Synced
            } else {
                ServiceState::Unknown
            },
            unvouched: false,
            repair: Repair::None,
            detail: format!("{who} is managing this clock — Nexus is leaving it alone"),
        };
    }

    let unvouched = !f.probe_reached_network && !f.synced;

    // 6 — no network. A repair has nothing to reach; the held offset carries the
    // station and the operator is told which of the two situations they are in.
    if !f.probe_reached_network {
        let owner = if f.service_running {
            ClockOwner::OsService(os_service_name().into())
        } else if f.service_present {
            ClockOwner::Nobody
        } else {
            ClockOwner::Unknown
        };
        return ClockDiagnosis {
            owner,
            state: if f.synced {
                ServiceState::Synced
            } else {
                ServiceState::NotSynced
            },
            unvouched,
            repair: Repair::None,
            detail: if unvouched {
                "no time server reachable and this clock has never been checked \
                 against one — it may be wrong by days"
                    .into()
            } else {
                "no time server reachable — running on the last measured offset".into()
            },
        };
    }

    // 3 — the clock just STEPPED. Ordered above the stopped/unsynced rows only
    // because those two cannot be true at once with this one in practice, and
    // above the healthy row because that is the whole point: after a resume
    // W32Time reports a perfectly good last sync — from before the machine went
    // to sleep — so every other row in this table says "healthy, do nothing"
    // about a clock that is hours wrong. `/resync` without `/rediscover`: the
    // source was fine, it is the sample that is old. Only while the clock
    // measures out: a step that left it right is the clock being put right (the
    // time service stepping it, or Repair clock's own resync), not a fault.
    if f.just_stepped && f.repairs_available && f.service_running && clock_is_off(f) {
        return ClockDiagnosis {
            owner: ClockOwner::OsService(os_service_name().into()),
            state: if f.synced {
                ServiceState::Synced
            } else {
                ServiceState::NotSynced
            },
            unvouched: false,
            repair: Repair::Resync { rediscover: false },
            detail: "the system clock jumped — asking the Windows Time service to \
                     re-check it now rather than at its next poll"
                .into(),
        };
    }

    // 1 — the service is stopped or disabled.
    if f.service_present && !f.service_running {
        return ClockDiagnosis {
            owner: ClockOwner::Nobody,
            state: ServiceState::Stopped,
            unvouched,
            repair: if f.repairs_available {
                Repair::StartService
            } else {
                Repair::None
            },
            detail: format!(
                "{} is not running — nothing is keeping this clock right",
                os_service_name()
            ),
        };
    }

    // 2' — running, its report does not confirm a sync (`sync_unconfirmed`), and
    // the clock measures right: nothing to repair. A working service between two
    // polls reads this way, and so does every non-English Windows. Once the clock
    // is out, row 2 offers the repair.
    if f.service_running && f.sync_unconfirmed && !clock_is_off(f) {
        return ClockDiagnosis {
            owner: ClockOwner::OsService(os_service_name().into()),
            state: ServiceState::Unknown,
            unvouched: false,
            repair: Repair::None,
            detail: format!(
                "this clock is within a second of UTC; {} has not confirmed its latest check",
                os_service_name()
            ),
        };
    }

    // 2 — running but has never reached a server. `/rediscover` throws out the
    // accumulated error statistics along with the source.
    if f.service_running && !f.synced {
        return ClockDiagnosis {
            owner: ClockOwner::OsService(os_service_name().into()),
            state: ServiceState::NotSynced,
            unvouched,
            repair: if f.repairs_available {
                Repair::Resync { rediscover: true }
            } else {
                Repair::None
            },
            detail: format!(
                "{} is running but has not synchronised — its server may be blocked",
                os_service_name()
            ),
        };
    }

    // Healthy — nothing to do, and nothing to say about how often the service
    // checks the time: a default Windows PC checks every 32768 s, and that is not a
    // fault (the operator's ruling, 2026-10-06). This is the common case and it
    // must stay silent: acting on a machine that is already right is how a
    // feature earns its reputation for meddling.
    ClockDiagnosis {
        owner: ClockOwner::OsService(os_service_name().into()),
        state: ServiceState::Synced,
        unvouched: false,
        repair: Repair::None,
        detail: format!("{} is keeping this clock right", os_service_name()),
    }
}

/// Does Nexus's own probe measure this clock more than [`REPAIR_WORTH_MS`] off UTC?
fn clock_is_off(f: &Findings) -> bool {
    f.measured_offset_ms
        .is_some_and(|ms| ms.abs() > REPAIR_WORTH_MS)
}

/// The OS time service's name on this platform, for operator-facing text.
fn os_service_name() -> &'static str {
    if cfg!(windows) {
        "the Windows Time service"
    } else if cfg!(target_os = "macos") {
        "macOS timed"
    } else {
        "the system time service"
    }
}

// ── Parsers: pure, over real captured output ─────────────────────────────────

/// Is W32Time reporting a good synchronisation?
///
/// ⚠️ **GUARD 12: `Phase Offset` IS NOT AN INPUT HERE.** It is the offset of the
/// LAST SAMPLE, stored, not live — proved by two reads 50 s apart returning
/// byte-identical values while `Time since Last Good Sync Time` advanced. On a
/// machine polling every 9.1 hours it can be hours stale, so reading it as the
/// current error says "0.0013 s, all well" about a clock that has since drifted
/// anywhere at all. The current error comes from Nexus's OWN probe; this
/// function answers only "is the service working", from `State Machine`,
/// `Last Sync Error` and whether a good sync was ever recorded.
pub fn parse_synced(status: &str) -> bool {
    parse_sync_report(status) == SyncReport::Synced
}

/// The three things W32Time's report can say about synchronisation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyncReport {
    /// A good sync on record, the latest attempt took, and the state machine is
    /// in Sync.
    Synced,
    /// Neither confirmed nor denied ([`Findings::sync_unconfirmed`]): a good sync
    /// on record but a latest attempt that did not take, or no report this can
    /// read.
    Unconfirmed,
    /// The service has never synchronised.
    Never,
}

/// [`parse_synced`]'s reading, in its three parts. Guard 12 holds here too.
fn parse_sync_report(status: &str) -> SyncReport {
    let field = |name: &str| -> Option<String> {
        status
            .lines()
            .find(|l| l.trim_start().starts_with(name))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
    };
    // `State Machine: 2 (Sync)` is the healthy state; 0 (Unset) / 1 (Hold) are
    // not. Absent on a plain (non-verbose) query, so it is corroborating
    // evidence rather than the whole test.
    let state_ok = match field("State Machine") {
        Some(v) => v.starts_with('2'),
        None => true,
    };
    // `Last Sync Error: 0 (The command completed successfully.)`
    let error_ok = match field("Last Sync Error") {
        Some(v) => v.starts_with('0'),
        None => true,
    };
    // The load-bearing one: a machine that has never synchronised prints
    // "unspecified" here. No such line at all is no report: a query that failed
    // or timed out, an error line, or the labels in another language.
    match field("Last Successful Sync Time") {
        None => SyncReport::Unconfirmed,
        Some(v) if v.is_empty() || v.to_ascii_lowercase().contains("unspecified") => {
            SyncReport::Never
        }
        Some(_) if state_ok && error_ok => SyncReport::Synced,
        Some(_) => SyncReport::Unconfirmed,
    }
}

/// The sync findings from W32Time's status report ("" when the query could not run).
fn read_w32tm_status(f: &mut Findings, status: &str) {
    let report = parse_sync_report(status);
    f.synced = report == SyncReport::Synced;
    f.sync_unconfirmed = report == SyncReport::Unconfirmed;
}

/// `STATE : 4 RUNNING` from `sc query w32time`.
pub fn parse_service_running(sc_out: &str) -> bool {
    sc_out
        .lines()
        .find(|l| l.trim_start().starts_with("STATE"))
        .is_some_and(|l| l.contains("RUNNING"))
}

/// Does `sc query` describe a service that EXISTS? A missing service prints
/// `The specified service does not exist as an installed service.`
pub fn parse_service_present(sc_out: &str) -> bool {
    sc_out.contains("SERVICE_NAME") || sc_out.contains("STATE")
}

/// `NTPSynchronized=yes` / `NTP=yes` from `timedatectl show`.
pub fn parse_timedatectl(out: &str) -> (Option<bool>, Option<bool>) {
    let field = |k: &str| {
        out.lines()
            .find_map(|l| l.strip_prefix(k))
            .map(|v| v.eq_ignore_ascii_case("yes"))
    };
    (field("NTP="), field("NTPSynchronized="))
}

/// The first `THIRD_PARTY_CLIENTS` image found in `tasklist` output.
pub fn parse_third_party(tasklist_out: &str) -> Option<String> {
    // ⚠️ CASE-INSENSITIVE, and the entries above are lower-cased to match. Windows filenames are
    // case-insensitive, so `tasklist` prints whatever casing the image on disk happens to carry —
    // an installer, a rename or a repackage can change it without changing the program. A
    // case-SENSITIVE `contains` (what this was) therefore misses silently, and a miss here is not
    // cosmetic: guard 8 is what makes Nexus stand down instead of fighting another program for the
    // clock, and two disciplinarians is how a clock ends up stepping under a decode.
    let hay = tasklist_out.to_ascii_lowercase();
    THIRD_PARTY_CLIENTS
        .iter()
        .find(|(image, _)| hay.contains(image))
        .map(|(_, label)| (*label).to_string())
}

// ── Running the commands (the only untestable part) ──────────────────────────

/// Run a command and return its stdout, or `None` if it could not run or did not
/// finish inside [`CMD_TIMEOUT`].
///
/// ⚠️ **The reader runs on its own thread, and that is not tidiness.** The
/// obvious shape — `Command::output()`, or `try_wait` in a loop while nothing
/// drains the pipe — deadlocks whenever a child writes more than the OS pipe
/// buffer: the child blocks on the write, so it never exits, so the wait never
/// returns. `tasklist` on a busy machine is exactly that much output. Draining
/// concurrently while the parent keeps the `Child` is also what lets the timeout
/// KILL the thing it timed out on.
///
/// Absolute paths on Windows: a GUI-launched process gets a minimal `PATH`, a
/// trap this tree already documents in `rigctld_proc.rs`.
///
/// ⚠️ **No console window, and 1.12.0 shipped without that.** Every one of these
/// is a console program, and [`detect`] runs three of them back to back a few
/// seconds after launch and every ten minutes. Spawned with a bare
/// `Command::new`, each one flashed a command-prompt window on Windows, and
/// testers saw three or four at a time. [`tempo_core::process::command`] is what
/// prevents that.
fn capture(program: &str, args: &[&str]) -> Option<String> {
    let mut child = tempo_core::process::command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut pipe = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
        let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
    });
    let deadline = Instant::now() + CMD_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            // Timed out, or the wait itself failed. Kill it and reap it — an
            // orphaned query process holding a pipe is how a background thread
            // accumulates children over a long session.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    // The child is gone, so the pipe is at EOF and the reader is about to
    // finish; this wait is for the thread, not for the process.
    rx.recv_timeout(Duration::from_secs(1)).ok()
}

/// Detect the machine's clock situation. Unprivileged; safe to call any time.
///
/// `probe_reached_network` and `measured_offset_ms` come from Nexus's own SNTP
/// probe — the diagnosis needs them because "the OS says it is synced" and "we
/// can reach a time server" are different facts and their combination is what
/// distinguishes a healthy machine from an unvouched one. `just_stepped` comes from
/// the radio loop's jump detector and is what distinguishes a laptop that just
/// woke up — which W32Time will report as perfectly healthy, from a sync taken
/// before it went to sleep — from a machine that genuinely needs nothing.
pub fn detect(
    probe_reached_network: bool,
    measured_offset_ms: Option<i64>,
    just_stepped: bool,
) -> ClockDiagnosis {
    let mut f = Findings {
        probe_reached_network,
        measured_offset_ms,
        just_stepped,
        ..Default::default()
    };

    if cfg!(windows) {
        // `/NH /FO CSV` — one spawn for the whole list, without the table
        // header and padding. A per-image `/FI` filter would be four spawns.
        f.third_party = capture("tasklist.exe", &["/NH", "/FO", "CSV"])
            .as_deref()
            .and_then(parse_third_party);
        let sc = capture("sc.exe", &["query", "w32time"]).unwrap_or_default();
        f.service_present = parse_service_present(&sc);
        f.service_running = parse_service_running(&sc);
        let status = capture("w32tm.exe", &["/query", "/status", "/verbose"]).unwrap_or_default();
        read_w32tm_status(&mut f, &status);
        f.repairs_available = true;
    } else if cfg!(target_os = "macos") {
        // ⚠️ REPORT ONLY, AND LESS THAN ON THE OTHER TWO. `timed` owns the clock
        // and writing it needs a signed `SMAppService` helper under the hardened
        // runtime, which Nexus does not have. `systemsetup -getusingnetworktime`
        // is the documented query and it requires admin, so asking it would
        // trade a diagnosis for a password prompt on every probe — not worth it
        // for a platform where the answer changes nothing we do. Apple does not
        // document `timed`'s poll interval at all.
        f.service_present = true;
        f.service_running = true;
        f.synced = probe_reached_network;
        f.repairs_available = false;
    } else {
        // Linux, including the Raspberry Pi. `timedatectl show` is unprivileged
        // and answers both questions systemd knows about.
        let td = capture("timedatectl", &["show"]).unwrap_or_default();
        let (ntp, synced) = parse_timedatectl(&td);
        f.service_present = !td.is_empty();
        f.service_running = ntp.unwrap_or(false);
        f.synced = synced.unwrap_or(false);
        // ⛔ NO REPAIR ON LINUX, DELIBERATELY. A running timesyncd or chrony needs
        // nothing from us (they poll at most every 2048 s and 1024 s), and the
        // broken case (neither daemon installed) is fixed with a package manager,
        // which is not ours to run on someone's machine.
        f.repairs_available = false;
    }

    if !cfg!(windows) && !cfg!(target_os = "macos") && !f.service_present {
        return ClockDiagnosis::unknown(
            "no system time service found — install systemd-timesyncd or chrony",
        );
    }
    decide(&f)
}

/// The commands one elevated helper run would execute for `repair`, in order.
///
/// Returned rather than run so the whole repair is **one** UAC prompt and so its
/// exact content is a unit test rather than a thing that happens on a stranger's
/// machine. Empty when there is nothing to do.
///
/// ⛔ Every command here targets `w32time` or `w32tm`, and none writes the
/// registry: the time server W32Time asks (`Parameters\NtpServer`) is never
/// repointed, which `never_writes_the_ntp_server_key` checks over every possible
/// repair.
pub fn repair_commands(repair: Repair) -> Vec<Vec<String>> {
    let s = |v: &[&str]| v.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
    match repair {
        Repair::None => Vec::new(),
        // `start= auto` restores a Windows DEFAULT rather than imposing a Nexus
        // one, which is also why an uninstall leaves it in place: reverting a
        // time service to disabled would be actively harmful.
        Repair::StartService => vec![
            s(&["sc.exe", "config", "w32time", "start=", "auto"]),
            s(&["sc.exe", "start", "w32time"]),
            s(&["w32tm.exe", "/resync"]),
        ],
        Repair::Resync { rediscover } => vec![if rediscover {
            s(&["w32tm.exe", "/resync", "/rediscover"])
        } else {
            s(&["w32tm.exe", "/resync"])
        }],
    }
}

/// Run `repair` with the one elevation this programme spends, and report
/// whether it took. Returns `false` on any platform without a repair path, on a
/// `Repair::None`, and whenever the helper could not be launched or refused.
///
/// ## The one UAC prompt
///
/// Nexus installs per-user (`"installMode": "currentUser"`), which is
/// deliberate: switching the installer to per-machine to get an always-elevated
/// process would inherit a documented Tauri defect where the two install paths
/// can leave a machine carrying a duplicate install. So the elevation is asked
/// for at the moment it is needed instead: once for each press of **Repair
/// clock**, and only on a machine a diagnosis has already offered a repair for.
/// Nothing calls this on a timer (`service::repair_clock` is the one caller),
/// which is also why there is no rate limit: every prompt is one the operator
/// asked for. Every command in the repair goes into **one** elevated
/// invocation, because two invocations are two prompts.
///
/// ⚠️ There is no zero-prompt route. The technique that appears to offer one is
/// a catalogued UAC bypass and is deliberately not implemented.
///
/// ## Why PowerShell rather than `ShellExecuteW`
///
/// A deviation from the spec's letter, for the reason `CLAUDE.md` states
/// plainly: a `#[cfg(windows)]` FFI body is type-checked by neither a Linux
/// compile nor any test that can run here, so it would ship on the strength of
/// having been read. `Start-Process -Verb RunAs` is the same OS mechanism —
/// `runas` is exactly what `ShellExecuteW`'s verb parameter takes — reached
/// through `std::process::Command`, which compiles and type-checks everywhere,
/// adds no dependency, and needs no `unsafe`. PowerShell has shipped in every
/// supported Windows.
///
/// ⚠️ **NEEDS-BENCH.** This function has never been run: no write to a registry,
/// a clock or a service has been made from the machine this was developed on,
/// by policy. What is tested is the command list it would run
/// ([`repair_commands`]) and the decision that selects it ([`decide`]).
pub fn run_repair_elevated(repair: Repair) -> bool {
    let cmds = repair_commands(repair);
    if cmds.is_empty() || !cfg!(windows) {
        return false;
    }
    // One `cmd /c a && b && c` under a single elevation. `&&` so a failed
    // service start does not go on to resync a service that is not running, and
    // so the exit status describes the whole repair rather than its last step.
    let joined = cmds
        .iter()
        .map(|c| c.join(" "))
        .collect::<Vec<_>>()
        .join(" && ");
    let script = elevated_script(&joined);
    // Absolute path: a GUI-launched process gets a minimal `PATH`, the trap this
    // tree documents in `rigctld_proc.rs`.
    tempo_core::process::command(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .map(|st| st.success())
        .unwrap_or(false)
}

/// The PowerShell that runs `joined` under one elevation and exits with its
/// status. `-Wait` alone exits 0 whatever the command returned, so a repair
/// Windows refused part-way read as one that took
/// (`the_elevated_script_exits_with_the_repairs_own_status`). `Stop` keeps a
/// `Start-Process` that fails (a declined prompt) from running on to the `exit`,
/// which would exit 0 with no process to ask.
fn elevated_script(joined: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'; $p = Start-Process -FilePath cmd.exe -ArgumentList '/c',\"{joined}\" -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode"
    )
}

/// The operator-facing line after a repair attempt.
pub fn repair_outcome_note(diag: &ClockDiagnosis, ok: bool) -> String {
    if !ok {
        return format!("{} — Nexus could not fix it", diag.detail);
    }
    match diag.repair {
        Repair::StartService => "started the Windows Time service".into(),
        Repair::Resync { .. } => "asked the Windows Time service to re-synchronise".into(),
        Repair::None => diag.detail.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Fixtures: VERBATIM output from a live Windows machine, read-only,
    //    2026-09-09. Not hand-written approximations of what these tools print.

    const W32TM_STATUS_VERBOSE: &str = "\
Leap Indicator: 0(no warning)
Stratum: 5 (secondary reference - syncd by (S)NTP)
Precision: -23 (119.209ns per tick)
Root Delay: 0.0807901s
Root Dispersion: 0.4345451s
ReferenceId: 0xA83DD74A (source IP:  168.61.215.74)
Last Successful Sync Time: 9/9/2026 2:56:39 AM
Source: time.windows.com,0x9
Poll Interval: 15 (32768s)

Phase Offset: 0.0013088s
ClockRate: 0.0156248s
State Machine: 2 (Sync)
Time Source Flags: 0 (None)
Server Role: 0 (None)
Last Sync Error: 0 (The command completed successfully.)
Time since Last Good Sync Time: 27995.8688108s
";

    const SC_QUERY_RUNNING: &str = "\n\
SERVICE_NAME: w32time
        TYPE               : 30  WIN32
        STATE              : 4  RUNNING
                                (STOPPABLE, NOT_PAUSABLE, ACCEPTS_SHUTDOWN)
        WIN32_EXIT_CODE    : 0  (0x0)
        SERVICE_EXIT_CODE  : 0  (0x0)
        CHECKPOINT         : 0x0
        WAIT_HINT          : 0x0
";

    const TASKLIST_WITH_NETTIME: &str = "\
Image Name                     PID Session Name        Session#    Mem Usage
========================= ======== ================ =========== ============
NetTimeService.exe            6240 Services                   0      3,960 K
";

    /// VERBATIM, a live Windows 11 machine, read-only, 2026-10-10, as an ordinary user (the
    /// rights Nexus has). The service synchronised at 7:31:50 and is in Sync; its LATEST attempt
    /// found only stale data. The same report came back on every read for over an hour.
    const W32TM_STATUS_STALE_ATTEMPT: &str = "\
Leap Indicator: 0(no warning)
Stratum: 5 (secondary reference - syncd by (S)NTP)
Precision: -23 (119.209ns per tick)
Root Delay: 0.0567875s
Root Dispersion: 2.1708095s
ReferenceId: 0xA83DD74A (source IP:  168.61.215.74)
Last Successful Sync Time: 10/10/2026 7:31:50 AM
Source: time.windows.com,0x9
Poll Interval: 10 (1024s)

Phase Offset: 0.0047341s
ClockRate: 0.0156249s
State Machine: 2 (Sync)
Time Source Flags: 0 (None)
Server Role: 0 (None)
Last Sync Error: 2 (The computer did not resync because only stale time data was available.)
Time since Last Good Sync Time: 8426.0524863s
";

    /// VERBATIM, the same machine and day: what `w32tm` prints in place of a report it will not
    /// give an ordinary user (here `/query /configuration`).
    const W32TM_ACCESS_DENIED: &str =
        "The following error occurred: Access is denied. (0x80070005)\n";

    /// Clock offsets as Nexus's own probe measures them: right, and more than a second out.
    const CLOCK_RIGHT_MS: i64 = 30;
    const CLOCK_OFF_MS: i64 = 3_200;

    // ── Parsers, against that real output ─────────────────────────────────────

    #[test]
    fn a_healthy_machine_parses_as_synced() {
        assert!(parse_synced(W32TM_STATUS_VERBOSE));
        assert!(parse_service_present(SC_QUERY_RUNNING));
        assert!(parse_service_running(SC_QUERY_RUNNING));
    }

    /// ⚠️ GUARD 12. The fixture above carries `Phase Offset: 0.0013088s`
    /// alongside `Time since Last Good Sync Time: 27995.9s` — the offset was
    /// measured 7.8 HOURS before it was read. The current error comes from
    /// Nexus's own probe; reading that 1.3 ms as "the current error" would vouch
    /// for a clock that has since drifted anywhere at all.
    #[test]
    fn the_diagnosis_never_reads_phase_offset_as_the_current_error() {
        // Same fixture with a wildly stale Phase Offset: the parse must be
        // unchanged, because it never looked.
        let doctored =
            W32TM_STATUS_VERBOSE.replace("Phase Offset: 0.0013088s", "Phase Offset: 0.0000001s");
        assert_eq!(
            parse_synced(&doctored),
            parse_synced(W32TM_STATUS_VERBOSE),
            "Phase Offset must not move the verdict in either direction"
        );
    }

    #[test]
    fn a_missing_service_is_not_a_running_one() {
        let missing = "[SC] EnumQueryServicesStatus:OpenService FAILED 1060:\n\n\
                       The specified service does not exist as an installed service.\n";
        assert!(!parse_service_present(missing));
        assert!(!parse_service_running(missing));
        let stopped = SC_QUERY_RUNNING.replace("4  RUNNING", "1  STOPPED");
        assert!(
            parse_service_present(&stopped),
            "a stopped service still exists"
        );
        assert!(!parse_service_running(&stopped));
    }

    #[test]
    fn a_machine_that_never_synchronised_is_not_synced() {
        let never = W32TM_STATUS_VERBOSE
            .replace(
                "Last Successful Sync Time: 9/9/2026 2:56:39 AM",
                "Last Successful Sync Time: unspecified",
            )
            .replace("State Machine: 2 (Sync)", "State Machine: 0 (Unset)");
        assert!(!parse_synced(&never));
        // Each half on its own is also disqualifying.
        let bad_state =
            W32TM_STATUS_VERBOSE.replace("State Machine: 2 (Sync)", "State Machine: 1 (Hold)");
        assert!(!parse_synced(&bad_state));
        let errored = W32TM_STATUS_VERBOSE.replace(
            "Last Sync Error: 0 (The command",
            "Last Sync Error: 5 (The command",
        );
        assert!(!parse_synced(&errored));
    }

    #[test]
    fn timedatectl_answers_both_questions() {
        let out = "NTP=yes\nNTPSynchronized=yes\nTimeUSec=Tue 2026-09-09\n";
        assert_eq!(parse_timedatectl(out), (Some(true), Some(true)));
        let offline = "NTP=yes\nNTPSynchronized=no\n";
        assert_eq!(parse_timedatectl(offline), (Some(true), Some(false)));
        assert_eq!(parse_timedatectl(""), (None, None));
    }

    // ── §3.6's decision table ─────────────────────────────────────────────────

    fn healthy_windows() -> Findings {
        Findings {
            service_present: true,
            service_running: true,
            synced: true,
            probe_reached_network: true,
            measured_offset_ms: Some(30),
            repairs_available: true,
            ..Default::default()
        }
    }

    /// ⚠️ GUARD 7's real content, and the row most likely to be broken by a
    /// later change: a machine that is already right gets NO ACTION.
    #[test]
    fn a_healthy_machine_is_left_completely_alone() {
        let d = decide(&healthy_windows());
        assert_eq!(d.repair, Repair::None);
        assert!(repair_commands(d.repair).is_empty(), "and nothing is run");
        assert_eq!(d.state, ServiceState::Synced);
        assert!(!d.unvouched);
    }

    const TASKLIST_WITH_VOVSOFT: &str = "\
Image Name                     PID Session Name        Session#    Mem Usage
========================= ======== ================ =========== ============
TimeSync.exe                  9112 Console                    1     12,480 K
";

    /// An operator reported an intermittently freezing waterfall alongside VOVSOFT Time Sync's
    /// "Invalid Server Time!" dialog (2026-09-17). That dialog is the program's own window, not
    /// Nexus's (Nexus's clock probe asks time.nist.gov too, but shows no such dialog) — and the
    /// program owns the clock while it runs, so guard 8 has to see it.
    ///
    /// ⚠️ THE CASING IN THIS FIXTURE IS THE POINT. `tasklist` prints the image as it is named on
    /// disk, and the vendor ships `timesync.exe` while a repackage or a rename can just as easily
    /// print `TimeSync.exe`. The match used to be case-SENSITIVE, so this exact output found
    /// nothing and Nexus went on disciplining a clock another program was already steering.
    #[test]
    fn a_third_party_client_is_recognised_whatever_case_tasklist_prints() {
        assert_eq!(
            parse_third_party(TASKLIST_WITH_VOVSOFT).as_deref(),
            Some("VOVSOFT Time Sync")
        );
        // Every entry, in three casings, so no single one can rot back to case-sensitive.
        for (image, label) in THIRD_PARTY_CLIENTS {
            for cased in [image.to_string(), image.to_ascii_uppercase(), {
                let mut c = image.chars();
                c.next()
                    .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                    .unwrap_or_default()
            }] {
                let line = format!("Image Name\n=====\n{cased}   1234 Console   1   1,000 K\n");
                assert_eq!(
                    parse_third_party(&line).as_deref(),
                    Some(label),
                    "{cased} was not recognised"
                );
            }
        }
        // CONTROL: the check can still answer "nobody else owns the clock".
        // CONTROL: a machine with none of them running answers None, so the sweep above cannot
        // be passing by matching everything.
        assert_eq!(
            parse_third_party(
                "Image Name                     PID Session Name\n                 ========================= ======== ================\n                 explorer.exe                  4128 Console\n                 w32tm.exe                     6600 Console\n"
            ),
            None
        );
    }

    /// The table is matched against a lower-cased haystack, so an entry carrying an upper-case
    /// letter can never match — it would look present and be dead. Cheaper to pin than to debug.
    #[test]
    fn every_third_party_image_is_lower_case() {
        for (image, label) in THIRD_PARTY_CLIENTS {
            assert_eq!(
                image,
                image.to_ascii_lowercase(),
                "{label}'s image must be lower-case or it can never match"
            );
        }
    }

    /// ⚠️ GUARD 8, and this machine is the live case: NetTime is running on the
    /// box this was developed on and does 88% of its clock corrections.
    #[test]
    fn a_third_party_client_takes_the_clock_and_we_stand_down() {
        assert_eq!(
            parse_third_party(TASKLIST_WITH_NETTIME).as_deref(),
            Some("NetTime")
        );
        let mut f = healthy_windows();
        f.third_party = Some("NetTime".into());
        // …and make everything ELSE look like a machine that wants repairing.
        f.service_running = false;
        f.synced = false;
        let d = decide(&f);
        assert_eq!(d.owner, ClockOwner::ThirdParty("NetTime".into()));
        assert_eq!(
            d.repair,
            Repair::None,
            "never become the second writer, however broken W32Time looks"
        );
        assert!(
            d.detail.contains("NetTime"),
            "and say who owns it: {}",
            d.detail
        );
    }

    /// ⚠️ THE HIGHEST-VALUE REPAIR IN THE PROGRAMME, and the one every other row
    /// of the table gets wrong. After a resume from sleep W32Time reports a
    /// perfectly good synchronisation — taken before the machine went to sleep —
    /// so a machine hours out of date looks HEALTHY to every check here. Only
    /// the jump detector knows, and this is what it is for.
    #[test]
    fn a_clock_step_forces_a_resync_a_healthy_looking_machine_would_not_get() {
        let mut f = healthy_windows();
        // The resume left the clock out by its RTC's own error, which the probe measures.
        f.measured_offset_ms = Some(CLOCK_OFF_MS);
        // The control first: this exact machine, without the step, is left alone.
        assert_eq!(decide(&f).repair, Repair::None, "healthy without the step");

        f.just_stepped = true;
        let d = decide(&f);
        assert_eq!(
            d.repair,
            Repair::Resync { rediscover: false },
            "the source was fine — it is the SAMPLE that is stale, so no /rediscover"
        );
        assert!(d.detail.contains("jumped"), "say why: {}", d.detail);
        assert_eq!(
            repair_commands(d.repair),
            vec![vec!["w32tm.exe".to_string(), "/resync".to_string()]]
        );
    }

    #[test]
    fn a_clock_step_off_windows_still_proposes_nothing() {
        // A sleeping MacBook has the same RTC problem, and the jump detector runs
        // there too — but there is no repair to offer, so the honest answer is
        // the re-measurement B already does and nothing else.
        let f = Findings {
            service_present: true,
            service_running: true,
            synced: true,
            probe_reached_network: true,
            just_stepped: true,
            repairs_available: false,
            ..Default::default()
        };
        assert_eq!(decide(&f).repair, Repair::None);
    }

    #[test]
    fn a_clock_step_never_overrides_the_stand_down() {
        // Guard 8 outranks everything: a machine running NetTime gets nothing
        // from us even on the tick its clock jumps.
        let mut f = healthy_windows();
        f.just_stepped = true;
        f.third_party = Some("NetTime".into());
        assert_eq!(decide(&f).repair, Repair::None);
    }

    #[test]
    fn a_stopped_service_is_started() {
        let mut f = healthy_windows();
        f.service_running = false;
        f.synced = false;
        let d = decide(&f);
        assert_eq!(d.state, ServiceState::Stopped);
        assert_eq!(d.repair, Repair::StartService);
    }

    #[test]
    fn a_running_but_unsynced_service_is_resynced_with_rediscover() {
        let mut f = healthy_windows();
        f.synced = false;
        let d = decide(&f);
        assert_eq!(d.state, ServiceState::NotSynced);
        assert_eq!(d.repair, Repair::Resync { rediscover: true });
    }

    /// The machine's findings after detection read `status` as W32Time's report, with Nexus's
    /// probe measuring the clock `offset_ms` off UTC.
    fn windows_reading(status: &str, offset_ms: i64) -> Findings {
        let mut f = healthy_windows();
        f.measured_offset_ms = Some(offset_ms);
        read_w32tm_status(&mut f, status);
        f
    }

    /// ⛔ A WORKING SERVICE BETWEEN TWO POLLS IS NOT A FAULT. W32Time's `Last Sync Error` is its
    /// LATEST attempt, and this one synchronised and is in Sync. This report was read as "has not
    /// synchronised — its server may be blocked", so Repair clock was offered with the clock right
    /// and came back on the next pass after every press. The clock is what tells a lapse that
    /// matters from one that does not: more than a second out and the repair is offered again.
    #[test]
    fn a_service_whose_latest_poll_did_not_take_is_offered_nothing_while_the_clock_is_right() {
        let d = decide(&windows_reading(W32TM_STATUS_STALE_ATTEMPT, CLOCK_RIGHT_MS));
        assert_eq!(d.repair, Repair::None, "{}", d.detail);
        assert!(
            !d.detail.contains("blocked"),
            "nothing says it is blocked: {}",
            d.detail
        );

        let d = decide(&windows_reading(W32TM_STATUS_STALE_ATTEMPT, CLOCK_OFF_MS));
        assert_eq!(
            d.repair,
            Repair::Resync { rediscover: true },
            "a clock that is out is told"
        );
        let d = decide(&windows_reading(W32TM_STATUS_STALE_ATTEMPT, -CLOCK_OFF_MS));
        assert_eq!(
            d.repair,
            Repair::Resync { rediscover: true },
            "behind as well as ahead"
        );
    }

    /// ⛔ A REPORT THAT CANNOT BE READ SAYS NOTHING. A query that failed or ran past
    /// [`CMD_TIMEOUT`] reaches the parser as "" (one of 132 reads on the machine above came back
    /// with no report), and `w32tm` may print an error line in place of the report. Every
    /// non-English Windows is in the same place: it prints the report with its labels translated
    /// (German's "Letzte erfolgr. Synchronisierungszeit"), and only the English labels are read.
    /// Each was taken for a service that has never synchronised, so Repair clock was offered on
    /// every pass. The measured clock decides here too.
    #[test]
    fn a_report_that_cannot_be_read_is_not_a_fault_while_the_clock_is_right() {
        for unread in ["", W32TM_ACCESS_DENIED] {
            let d = decide(&windows_reading(unread, CLOCK_RIGHT_MS));
            assert_eq!(d.repair, Repair::None, "{unread:?}: {}", d.detail);
            let d = decide(&windows_reading(unread, CLOCK_OFF_MS));
            assert_eq!(
                d.repair,
                Repair::Resync { rediscover: true },
                "{unread:?}, the clock out"
            );
        }
    }

    /// The real faults are still told with the clock right: a service that has never
    /// synchronised, and one that is stopped. And the clean report is healthy either way.
    #[test]
    fn a_never_synchronised_or_stopped_service_is_offered_whatever_the_clock_reads() {
        let never = W32TM_STATUS_VERBOSE
            .replace(
                "Last Successful Sync Time: 9/9/2026 2:56:39 AM",
                "Last Successful Sync Time: unspecified",
            )
            .replace("State Machine: 2 (Sync)", "State Machine: 0 (Unset)");
        let d = decide(&windows_reading(&never, CLOCK_RIGHT_MS));
        assert_eq!(
            d.repair,
            Repair::Resync { rediscover: true },
            "{}",
            d.detail
        );

        let mut stopped = windows_reading(W32TM_ACCESS_DENIED, CLOCK_RIGHT_MS);
        stopped.service_running = false;
        assert_eq!(decide(&stopped).repair, Repair::StartService);

        for offset in [CLOCK_RIGHT_MS, CLOCK_OFF_MS] {
            let d = decide(&windows_reading(W32TM_STATUS_VERBOSE, offset));
            assert_eq!(d.repair, Repair::None, "the clean report at {offset} ms");
        }
    }

    /// ⛔ A STEP THAT LEAVES THE CLOCK RIGHT IS THE CLOCK BEING PUT RIGHT. Windows Time stepping
    /// a clock it found out, or the step Repair clock's own `w32tm /resync` makes, wakes the probe
    /// as a resume does, and the clock it measures is right. That step put Repair clock straight
    /// back after "Clock repaired". After a step the clock is out, the resync is offered as before.
    #[test]
    fn a_step_that_leaves_the_clock_right_is_not_a_fault() {
        let mut f = healthy_windows();
        f.just_stepped = true;
        f.measured_offset_ms = Some(CLOCK_RIGHT_MS);
        assert_eq!(decide(&f).repair, Repair::None);
        f.measured_offset_ms = Some(CLOCK_OFF_MS);
        assert_eq!(decide(&f).repair, Repair::Resync { rediscover: false });
    }

    /// ⛔ A DEFAULT WINDOWS PC IS HEALTHY, AND ITS NOTE SAYS ONLY THAT (the
    /// operator's ruling, 2026-10-06). It checks the time every 32768 s, the
    /// Windows default, and Nexus corrects its own timing whatever the PC clock
    /// does, so nothing is offered and the hover note says nothing about polling.
    /// It used to read "…is healthy but only checks the time every 32768 s", as
    /// if that were a fault. The diagnosis no longer reads the poll interval.
    #[test]
    fn a_default_windows_pc_is_healthy_and_its_note_says_nothing_about_polling() {
        let d = decide(&healthy_windows());
        assert_eq!(d.repair, Repair::None);
        assert_eq!(
            d.detail,
            format!("{} is keeping this clock right", os_service_name())
        );
        for word in ["poll", "checks the time", "32768"] {
            assert!(!d.detail.contains(word), "{word}: {}", d.detail);
        }
    }

    #[test]
    fn no_network_means_no_repair_and_no_noise() {
        let mut f = healthy_windows();
        f.probe_reached_network = false;
        let d = decide(&f);
        assert_eq!(d.repair, Repair::None);
        assert!(!d.unvouched, "the OS still vouches for this clock");
        assert!(d.detail.contains("last measured offset"), "{}", d.detail);
    }

    /// The Raspberry Pi case (§5.3(1)). No RTC: a Pi powered off for a week
    /// boots believing it is the moment it last ran, and NOTHING LOOKS WRONG —
    /// a sensible date, a running clock. Only the combination of "the OS has
    /// never synchronised" and "we cannot reach a server either" says so.
    #[test]
    fn an_offline_never_synced_clock_is_reported_unvouched() {
        let f = Findings {
            service_present: true,
            service_running: true,
            synced: false,
            probe_reached_network: false,
            repairs_available: false,
            ..Default::default()
        };
        let d = decide(&f);
        assert!(d.unvouched, "this clock could be days wrong");
        assert_eq!(
            d.repair,
            Repair::None,
            "and there is nothing to repair it with"
        );
        assert!(
            d.detail.contains("days"),
            "say how bad it could be: {}",
            d.detail
        );

        // The control: reachable network, and it is not unvouched even unsynced.
        let mut ok = f.clone();
        ok.probe_reached_network = true;
        assert!(!decide(&ok).unvouched);
    }

    #[test]
    fn a_platform_with_no_repair_path_never_proposes_one() {
        // Linux and macOS, whatever their time service is doing.
        for (running, synced) in [(false, false), (true, false), (true, true)] {
            let f = Findings {
                service_present: true,
                service_running: running,
                synced,
                probe_reached_network: true,
                repairs_available: false,
                ..Default::default()
            };
            assert_eq!(
                decide(&f).repair,
                Repair::None,
                "no repair exists off Windows (running={running} synced={synced})"
            );
        }
    }

    // ── The repair commands ───────────────────────────────────────────────────

    /// ⛔ GUARD 10, over EVERY repair the decision table can produce. Repointing
    /// `NtpServer` at the NTP Pool from a shipped default would be ~84,000
    /// queries a day against volunteer infrastructure (at a 1024 s poll, across
    /// 1000+ installations), from a machine-wide setting that survives
    /// uninstalling Nexus, and the pool's policy is explicit: *"You must
    /// absolutely not use the default pool.ntp.org zone names as the default
    /// configuration in your application or appliance."* Whatever server is
    /// there, the operator or their IT department put it there. No repair writes
    /// the registry at all.
    #[test]
    fn never_writes_the_ntp_server_key() {
        let every_repair = [
            Repair::None,
            Repair::StartService,
            Repair::Resync { rediscover: true },
            Repair::Resync { rediscover: false },
        ];
        for r in every_repair {
            for cmd in repair_commands(r) {
                let line = cmd.join(" ");
                assert!(
                    !line.contains("NtpServer"),
                    "{r:?} would write the operator's time server: {line}"
                );
                assert!(
                    !line.to_lowercase().contains("pool.ntp.org"),
                    "{r:?} would point a machine at the NTP Pool: {line}"
                );
                // …nor anything else in the registry.
                assert!(
                    !line.contains("reg.exe"),
                    "{r:?} writes the registry: {line}"
                );
            }
        }
    }

    /// The positive control for the test above: it must actually be capable of
    /// failing. Without this, "no command mentions NtpServer" would pass just as
    /// happily if `repair_commands` returned nothing at all.
    #[test]
    fn the_ntp_server_guard_can_fail() {
        let forbidden = [
            "reg.exe",
            "add",
            r"HKLM\SYSTEM\CurrentControlSet\Services\W32Time\Parameters",
            "/v",
            "NtpServer",
            "/d",
            "pool.ntp.org,0x1",
        ];
        let line = forbidden.join(" ");
        assert!(
            line.contains("reg.exe")
                && line.contains("NtpServer")
                && line.to_lowercase().contains("pool.ntp.org")
        );
        // …and the real repairs are non-empty, so the sweep has something to scan.
        assert!(!repair_commands(Repair::StartService).is_empty());
        assert!(!repair_commands(Repair::Resync { rediscover: true }).is_empty());
    }

    #[test]
    fn a_failed_repair_says_so_plainly() {
        let d = ClockDiagnosis {
            owner: ClockOwner::OsService("the Windows Time service".into()),
            state: ServiceState::Stopped,
            unvouched: false,
            repair: Repair::StartService,
            detail: "the Windows Time service is not running".into(),
        };
        let failed = repair_outcome_note(&d, false);
        assert!(failed.contains("could not fix it"), "{failed}");
        assert!(
            failed.contains("not running"),
            "and still says what is wrong: {failed}"
        );
        assert_eq!(
            repair_outcome_note(&d, true),
            "started the Windows Time service"
        );
    }

    /// ⚠️ A REPAIR WINDOWS REFUSED PART-WAY MUST READ AS ONE. PowerShell's own exit status is all
    /// [`run_repair_elevated`] sees, and `Start-Process -Wait` exits 0 whatever the command it
    /// waited for returned. Measured with Windows PowerShell on a Windows 11 machine (2026-10-10,
    /// unelevated, `cmd /c exit 5` in place of the repair): that shape exited 0, and this one
    /// (`-PassThru`, then `exit` with the command's own code) exited 5, and 0 for `exit 0`. A
    /// repair that failed said "Clock repaired", came off offer, and was offered again by the
    /// next pass. With `Start-Process` itself failing (a program that is not there, standing in
    /// for a declined prompt), `-PassThru` alone exited 0 and with `Stop` first exited 1.
    #[test]
    fn the_elevated_script_exits_with_the_repairs_own_status() {
        let s = elevated_script("w32tm.exe /resync");
        assert!(s.contains("-Verb RunAs"), "still one elevation: {s}");
        assert!(
            s.starts_with("$ErrorActionPreference = 'Stop'; $p = Start-Process "),
            "{s}"
        );
        assert!(s.contains(" -Wait -PassThru "), "{s}");
        assert!(s.ends_with("; exit $p.ExitCode"), "{s}");
    }

    /// The elevated helper never runs anything off Windows, and never runs
    /// anything at all for a `Repair::None`. (On Linux this also proves the
    /// whole function is inert — nothing is spawned by the test suite.)
    #[test]
    fn the_elevated_helper_does_nothing_without_a_repair() {
        assert!(!run_repair_elevated(Repair::None));
        if !cfg!(windows) {
            assert!(!run_repair_elevated(Repair::StartService));
        }
    }
}
