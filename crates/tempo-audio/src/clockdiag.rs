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
//! the repair needs elevation, and it is only offered once a diagnosis says a
//! repair would actually help.
//!
//! **Nexus never calls a clock API.** On Windows the repair goes through
//! `sc`, `w32tm` and `reg` — the OS's own tools — so W32Time stays the sole
//! disciplinarian and there is no second writer to coexist with. On Linux and
//! macOS there is no repair at all: `systemd-timesyncd` polls at most every
//! 2048 s and `chrony` at most every 1024 s, both already tighter than anything
//! we would set, so the Windows problem simply does not exist there.
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

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Third-party Windows time clients. If one of these is running it owns the
/// clock and Nexus stands down entirely (guard 8) — a second disciplinarian is
/// how two programs fight over one clock and neither wins. On the machine this
/// was developed against, NetTime does 88% of the corrections.
const THIRD_PARTY_CLIENTS: [(&str, &str); 4] = [
    ("NetTimeService.exe", "NetTime"),
    ("ntpd.exe", "Meinberg NTP"),
    ("D4.exe", "Dimension 4"),
    ("chronyd.exe", "chrony"),
];

/// ⛔ THE REGISTRY VALUE NEXUS WRITES, AND THE ONLY ONE.
///
/// `SpecialPollInterval` under `TimeProviders\NtpClient` — how often W32Time
/// asks its configured server. Nothing else under `W32Time` is ever written.
const POLL_INTERVAL_KEY: &str =
    r"HKLM\SYSTEM\CurrentControlSet\Services\W32Time\TimeProviders\NtpClient";

/// ⛔ **NEVER WRITTEN.** `Parameters\NtpServer` holds the server W32Time polls,
/// and repointing it at the NTP Pool is the one change in this whole programme
/// that would do real harm outside this machine: at a 1024 s poll a host asks
/// **84 times a day**, and 1000+ installations of a shipped default would be
/// ~84,000 queries a day against volunteer infrastructure — from a MACHINE-WIDE
/// setting that outlives the app and survives its uninstall. The pool's vendor
/// policy is explicit: *"You must absolutely not use the default pool.ntp.org
/// zone names as the default configuration in your application or appliance."*
///
/// It is also not ours to touch for a second reason: whatever is in it, the
/// operator or their IT department put it there. Read it — the `0x1`
/// SpecialInterval flag decides whether our write does anything at all — and
/// never write it. Pinned by `never_writes_the_ntp_server_key`.
const NTP_SERVER_KEY: &str = r"HKLM\SYSTEM\CurrentControlSet\Services\W32Time\Parameters";

/// The poll interval Nexus asks for, in seconds — **the operator's decided
/// value** (2026-09-08).
///
/// 17 minutes. It sits exactly on Microsoft's own documented floor: this machine
/// reports `MinPollInterval = 0xa` → 2¹⁰ = 1024 s, and `SpecialPollInterval` *"is
/// contained by the MinPollInterval and MaxPollInterval registry values"*, so a
/// smaller number would be clamped up to this one anyway. Cross-check: 1024 s is
/// also chrony's default `maxpoll`.
///
/// ⚠️ It is not the feature, and must not be presented as one. On a healthy,
/// converged, always-on desktop it improves 27–79 ms of drift to 0.9–2.5 ms —
/// real, cheap, harmless, and far inside a tolerance that machine already met.
/// What this module is *for* is the machine whose time service is stopped, or
/// wedged, or which just resumed from sleep hours out of date.
pub const DESIRED_POLL_SECS: u32 = 1024;

/// Guard 3's ceiling, mirrored here so a diagnosis can say "too far out".
pub use tempo_app::clocksync::MAX_STEER_MS;

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
    /// Diagnosis 4 — healthy but polling too rarely. The one registry write.
    SetPollInterval(u32),
}

/// Everything a detection pass found, and what it concluded.
#[derive(Clone, Debug, PartialEq)]
pub struct ClockDiagnosis {
    pub owner: ClockOwner,
    pub state: ServiceState,
    /// The poll interval the OS service is actually using, in seconds.
    pub poll_secs: Option<u32>,
    /// Whether `NtpServer` carries the `0x1` SpecialInterval flag. **Without it
    /// the poll write is inert** — W32Time polls adaptively between Min and
    /// MaxPollInterval instead of honouring `SpecialPollInterval` — so a write
    /// would silently do nothing. `None` off Windows or when unreadable.
    pub special_interval_flag: Option<bool>,
    /// `2^MinPollInterval`, the floor `SpecialPollInterval` is clamped up to.
    /// Guard 11: if this exceeds what we asked for, we asked for the wrong thing.
    pub min_poll_secs: Option<u32>,
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
            poll_secs: None,
            special_interval_flag: None,
            min_poll_secs: None,
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
    pub poll_secs: Option<u32>,
    pub special_interval_flag: Option<bool>,
    pub min_poll_secs: Option<u32>,
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
/// third-party client needs nothing from us whatever else is true of it; "no
/// network" comes before any repair because a repair without a server to reach
/// is noise; and the poll write comes last because it is the weakest of the
/// four and must never shadow a real fault.
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
            poll_secs: f.poll_secs,
            special_interval_flag: f.special_interval_flag,
            min_poll_secs: f.min_poll_secs,
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
            poll_secs: f.poll_secs,
            special_interval_flag: f.special_interval_flag,
            min_poll_secs: f.min_poll_secs,
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
    // source was fine, it is the sample that is old.
    if f.just_stepped && f.repairs_available && f.service_running {
        return ClockDiagnosis {
            owner: ClockOwner::OsService(os_service_name().into()),
            state: if f.synced {
                ServiceState::Synced
            } else {
                ServiceState::NotSynced
            },
            poll_secs: f.poll_secs,
            special_interval_flag: f.special_interval_flag,
            min_poll_secs: f.min_poll_secs,
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
            poll_secs: f.poll_secs,
            special_interval_flag: f.special_interval_flag,
            min_poll_secs: f.min_poll_secs,
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

    // 2 — running but has never reached a server. `/rediscover` throws out the
    // accumulated error statistics along with the source.
    if f.service_running && !f.synced {
        return ClockDiagnosis {
            owner: ClockOwner::OsService(os_service_name().into()),
            state: ServiceState::NotSynced,
            poll_secs: f.poll_secs,
            special_interval_flag: f.special_interval_flag,
            min_poll_secs: f.min_poll_secs,
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

    // 5 — no SpecialInterval flag: the write would be INERT, so do not make it.
    // W32Time is already polling adaptively here, which is the better behaviour
    // of the two: a drifting machine shortens its own poll, where the `0x1` flag
    // would pin it at whatever we wrote no matter how badly it drifted.
    if f.special_interval_flag == Some(false) {
        return ClockDiagnosis {
            owner: ClockOwner::OsService(os_service_name().into()),
            state: ServiceState::Synced,
            poll_secs: f.poll_secs,
            special_interval_flag: Some(false),
            min_poll_secs: f.min_poll_secs,
            unvouched: false,
            repair: Repair::None,
            detail: "this machine's time server is set to adaptive polling, so the \
                     poll interval is not Nexus's to set — leaving it alone"
                .into(),
        };
    }

    // 4 — healthy, but polling far more rarely than it could. GUARD 11 IS HERE
    // AND NOT AT THE WRITE: `SpecialPollInterval` is clamped up to
    // `2^MinPollInterval`, so asking for less than the floor is asking for
    // something that cannot happen. Ask for the floor and report the floor.
    let target = f.min_poll_secs.unwrap_or(0).max(DESIRED_POLL_SECS);
    if f.repairs_available && f.poll_secs.is_some_and(|p| p > target) {
        return ClockDiagnosis {
            owner: ClockOwner::OsService(os_service_name().into()),
            state: ServiceState::Synced,
            poll_secs: f.poll_secs,
            special_interval_flag: f.special_interval_flag,
            min_poll_secs: f.min_poll_secs,
            unvouched: false,
            repair: Repair::SetPollInterval(target),
            detail: format!(
                "{} is healthy but only checks the time every {} s",
                os_service_name(),
                f.poll_secs.unwrap_or(0)
            ),
        };
    }

    // 4' / healthy — nothing to do. This is the common case and it must stay
    // silent: acting on a machine that is already right is how a feature earns
    // its reputation for meddling.
    ClockDiagnosis {
        owner: ClockOwner::OsService(os_service_name().into()),
        state: ServiceState::Synced,
        poll_secs: f.poll_secs,
        special_interval_flag: f.special_interval_flag,
        min_poll_secs: f.min_poll_secs,
        unvouched: false,
        repair: Repair::None,
        detail: format!("{} is keeping this clock right", os_service_name()),
    }
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

/// `Poll Interval: 15 (32768s)` from `w32tm /query /status` → 32768.
///
/// The parenthesised seconds, not the exponent — Microsoft prints both and the
/// bare `15` is a log2 that reads like a plausible number of seconds.
pub fn parse_poll_interval(status: &str) -> Option<u32> {
    let line = status
        .lines()
        .find(|l| l.trim_start().starts_with("Poll Interval:"))?;
    let inner = line.split('(').nth(1)?;
    inner
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()
}

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
    // "unspecified" here.
    let ever_synced = field("Last Successful Sync Time")
        .is_some_and(|v| !v.is_empty() && !v.to_ascii_lowercase().contains("unspecified"));
    state_ok && error_ok && ever_synced
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

/// The `0x1` SpecialInterval flag on the configured `NtpServer` entry.
///
/// `NtpServer    REG_SZ    time.windows.com,0x9` → `0x9 = 0x8 | 0x1` → true.
/// Several servers may be listed space-separated; the flag matters if ANY entry
/// carries it, since that entry is one W32Time will poll on our interval.
///
/// ⚠️ Reading this is the whole point. **Without the flag the poll write is
/// inert**, and writing anyway would leave us reporting a change that did not
/// happen. And the flag is never ADDED to an entry the operator configured —
/// that would change which behaviour their machine has, not just how often.
pub fn parse_special_interval_flag(reg_out: &str) -> Option<bool> {
    let line = reg_out
        .lines()
        .find(|l| l.split_whitespace().next() == Some("NtpServer"))?;
    let value = line
        .split_whitespace()
        .skip(2)
        .collect::<Vec<_>>()
        .join(" ");
    if value.is_empty() {
        return None;
    }
    Some(value.split_whitespace().any(|entry| {
        entry
            .rsplit_once(',')
            .and_then(|(_, flags)| {
                let hex = flags
                    .trim()
                    .strip_prefix("0x")
                    .or_else(|| flags.trim().strip_prefix("0X"))?;
                u32::from_str_radix(hex, 16).ok()
            })
            .is_some_and(|flags| flags & 0x1 != 0)
    }))
}

/// A `REG_DWORD` value, e.g. `MinPollInterval    REG_DWORD    0xa` → 10.
pub fn parse_reg_dword(reg_out: &str, name: &str) -> Option<u32> {
    let line = reg_out
        .lines()
        .find(|l| l.split_whitespace().next() == Some(name))?;
    let raw = line.split_whitespace().nth(2)?;
    let hex = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X"))?;
    u32::from_str_radix(hex, 16).ok()
}

/// `2^MinPollInterval` in seconds, saturating rather than overflowing on a
/// nonsense registry value.
pub fn min_poll_secs(exponent: u32) -> u32 {
    if exponent >= 31 {
        u32::MAX
    } else {
        1u32 << exponent
    }
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
    THIRD_PARTY_CLIENTS
        .iter()
        .find(|(image, _)| tasklist_out.contains(image))
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
fn capture(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
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
        f.synced = parse_synced(&status);
        f.poll_secs = parse_poll_interval(&status);
        f.special_interval_flag = capture("reg.exe", &["query", NTP_SERVER_KEY])
            .as_deref()
            .and_then(parse_special_interval_flag);
        f.min_poll_secs = capture(
            "reg.exe",
            &["query", NTP_SERVER_CONFIG_KEY, "/v", "MinPollInterval"],
        )
        .as_deref()
        .and_then(|o| parse_reg_dword(o, "MinPollInterval"))
        .map(min_poll_secs);
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
        // ⛔ NO REPAIR ON LINUX, DELIBERATELY. timesyncd caps its poll at 2048 s
        // and chrony at 1024 s — both at or tighter than what we would ask
        // Windows for, so there is nothing to improve. The broken case (neither
        // daemon installed) is fixed with a package manager, which is not ours
        // to run on someone's machine.
        f.repairs_available = false;
    }

    if !cfg!(windows) && !cfg!(target_os = "macos") && !f.service_present {
        return ClockDiagnosis::unknown(
            "no system time service found — install systemd-timesyncd or chrony",
        );
    }
    decide(&f)
}

/// `Config`, where `MinPollInterval` lives — read for guard 11's floor, never
/// written.
const NTP_SERVER_CONFIG_KEY: &str = r"HKLM\SYSTEM\CurrentControlSet\Services\W32Time\Config";

/// The commands one elevated helper run would execute for `repair`, in order.
///
/// Returned rather than run so the whole repair is **one** UAC prompt and so its
/// exact content is a unit test rather than a thing that happens on a stranger's
/// machine. Empty when there is nothing to do.
///
/// ⛔ Every command here targets `w32time`, `w32tm`, or [`POLL_INTERVAL_KEY`].
/// `NtpServer` never appears — see [`NTP_SERVER_KEY`] — and
/// `never_writes_the_ntp_server_key` checks that over every possible repair.
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
        Repair::SetPollInterval(secs) => vec![
            vec![
                "reg.exe".into(),
                "add".into(),
                POLL_INTERVAL_KEY.into(),
                "/v".into(),
                "SpecialPollInterval".into(),
                "/t".into(),
                "REG_DWORD".into(),
                "/d".into(),
                secs.to_string(),
                "/f".into(),
            ],
            s(&["w32tm.exe", "/resync"]),
        ],
    }
}

/// Guard 11: what the machine ACTUALLY ended up with, read back from
/// `w32tm /query /status` after a write.
///
/// The write returning success says the registry took the number, not that
/// W32Time is using it — `SpecialPollInterval` is clamped up to
/// `2^MinPollInterval`, and without the `0x1` flag it is ignored entirely.
/// Report the achieved value, never the requested one.
pub fn achieved_poll_secs(status_after: &str) -> Option<u32> {
    parse_poll_interval(status_after)
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
/// for at the moment it is needed instead, once, and only on a machine a
/// diagnosis has already said is broken. Every command in the repair goes into
/// **one** elevated invocation, because two invocations are two prompts.
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
    let script = format!(
        "Start-Process -FilePath cmd.exe -ArgumentList '/c',\"{joined}\" -Verb RunAs -Wait -WindowStyle Hidden"
    );
    // Absolute path: a GUI-launched process gets a minimal `PATH`, the trap this
    // tree documents in `rigctld_proc.rs`.
    Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .map(|st| st.success())
        .unwrap_or(false)
}

/// The operator-facing line after a repair attempt.
///
/// ⚠️ **GUARD 11 LIVES HERE.** For the poll write this re-reads
/// `w32tm /query /status` and reports the interval the machine ACTUALLY ended up
/// with, which can differ from the one requested in two documented ways: it is
/// clamped up to `2^MinPollInterval`, and without the `0x1` SpecialInterval flag
/// on the configured `NtpServer` entry it is ignored entirely. Reporting the
/// requested number would be reporting an intention as a result.
pub fn repair_outcome_note(diag: &ClockDiagnosis, ok: bool, terminal: bool) -> String {
    if terminal {
        return format!(
            "{} — Nexus tried to fix it and could not, and has stopped trying",
            diag.detail
        );
    }
    if !ok {
        return format!("{} — Nexus could not fix it", diag.detail);
    }
    match diag.repair {
        Repair::SetPollInterval(requested) => poll_write_note(
            requested,
            capture("w32tm.exe", &["/query", "/status"])
                .as_deref()
                .and_then(achieved_poll_secs),
        ),
        Repair::StartService => "started the Windows Time service".into(),
        Repair::Resync { .. } => "asked the Windows Time service to re-synchronise".into(),
        Repair::None => diag.detail.clone(),
    }
}

/// Guard 11's wording, split from the command that reads it back so the rule is
/// a unit test rather than a thing that only happens on Windows.
///
/// Three outcomes and they are genuinely different: the machine did what we
/// asked; the machine did something ELSE (clamped by `MinPollInterval`, or the
/// write ignored for want of the `0x1` flag), which the operator is told
/// verbatim; or we could not read it back, which is stated as the uncertainty it
/// is rather than smoothed into a success.
pub fn poll_write_note(requested: u32, achieved: Option<u32>) -> String {
    match achieved {
        Some(a) if a <= requested => {
            format!("the Windows Time service now checks the time every {a} s")
        }
        Some(a) => {
            format!("asked the Windows Time service for every {requested} s; it is using {a} s")
        }
        None => format!(
            "asked the Windows Time service for every {requested} s \
             (could not read back what it is using)"
        ),
    }
}

// ── Guard 9: the repair rate limit ───────────────────────────────────────────

/// Consecutive repair failures after which we stop trying (guard 9).
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Minimum interval between repair attempts (guard 9).
const REPAIR_COOLDOWN: Duration = Duration::from_secs(3_600);

/// Rate limiter for automatic repairs: at most one an hour, and a terminal stop
/// after three consecutive failures.
///
/// A repair that does not work will not work the fourth time either, and a
/// machine that fails it is exactly the machine where retrying forever means a
/// UAC prompt forever. Terminal means terminal until the operator acts.
#[derive(Debug, Default)]
pub struct RepairLimiter {
    last_attempt: Option<std::time::Instant>,
    consecutive_failures: u32,
}

impl RepairLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// May a repair be attempted at `now`?
    pub fn may_attempt(&self, now: std::time::Instant) -> bool {
        if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            return false;
        }
        match self.last_attempt {
            None => true,
            Some(t) => now.saturating_duration_since(t) >= REPAIR_COOLDOWN,
        }
    }

    /// Record an attempt and its outcome.
    pub fn record(&mut self, now: std::time::Instant, succeeded: bool) {
        self.last_attempt = Some(now);
        if succeeded {
            self.consecutive_failures = 0;
        } else {
            self.consecutive_failures += 1;
        }
    }

    /// Has this machine failed too many times to keep trying?
    pub fn is_terminal(&self) -> bool {
        self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

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

    const REG_PARAMETERS: &str = "\n\
HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\W32Time\\Parameters
    NtpServer    REG_SZ    time.windows.com,0x9
    ServiceDll    REG_EXPAND_SZ    %systemroot%\\system32\\w32time.dll
    ServiceDllUnloadOnStop    REG_DWORD    0x1
    ServiceMain    REG_SZ    SvchostEntry_W32Time
    Type    REG_SZ    NTP
";

    const REG_CONFIG_MINPOLL: &str = "\n\
HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\W32Time\\Config
    MinPollInterval    REG_DWORD    0xa
";

    const TASKLIST_WITH_NETTIME: &str = "\
Image Name                     PID Session Name        Session#    Mem Usage
========================= ======== ================ =========== ============
NetTimeService.exe            6240 Services                   0      3,960 K
";

    // ── Parsers, against that real output ─────────────────────────────────────

    #[test]
    fn the_poll_interval_is_the_seconds_not_the_exponent() {
        // `Poll Interval: 15 (32768s)` — 15 is a log2 that reads exactly like a
        // plausible number of seconds, which is the trap.
        assert_eq!(parse_poll_interval(W32TM_STATUS_VERBOSE), Some(32_768));
    }

    #[test]
    fn a_healthy_machine_parses_as_synced() {
        assert!(parse_synced(W32TM_STATUS_VERBOSE));
        assert!(parse_service_present(SC_QUERY_RUNNING));
        assert!(parse_service_running(SC_QUERY_RUNNING));
    }

    /// ⚠️ GUARD 12. The fixture above carries `Phase Offset: 0.0013088s`
    /// alongside `Time since Last Good Sync Time: 27995.9s` — the offset was
    /// measured 7.8 HOURS before it was read. A machine whose real error has
    /// since grown to a second must still diagnose as drifting, and reading that
    /// 1.3 ms as "the current error" is what would stop it.
    #[test]
    fn the_diagnosis_never_reads_phase_offset_as_the_current_error() {
        // Same fixture with a wildly stale Phase Offset and a huge real error:
        // the parse must be unchanged, because it never looked.
        let doctored =
            W32TM_STATUS_VERBOSE.replace("Phase Offset: 0.0013088s", "Phase Offset: 0.0000001s");
        assert_eq!(
            parse_synced(&doctored),
            parse_synced(W32TM_STATUS_VERBOSE),
            "Phase Offset must not move the verdict in either direction"
        );
        // And the diagnosis of a drifting machine comes from the POLL INTERVAL
        // plus Nexus's own probe, never from that field.
        let f = Findings {
            service_present: true,
            service_running: true,
            synced: true,
            poll_secs: parse_poll_interval(&doctored),
            special_interval_flag: Some(true),
            min_poll_secs: Some(1_024),
            probe_reached_network: true,
            measured_offset_ms: Some(900), // the REAL error, from our own probe
            repairs_available: true,
            ..Default::default()
        };
        assert_eq!(
            decide(&f).repair,
            Repair::SetPollInterval(1_024),
            "a stale 0.1 µs Phase Offset must not make a drifting machine look healthy"
        );
    }

    #[test]
    fn the_special_interval_flag_is_read_off_the_real_ntpserver_value() {
        // `time.windows.com,0x9` = 0x8 (Client) | 0x1 (SpecialInterval).
        assert_eq!(parse_special_interval_flag(REG_PARAMETERS), Some(true));
        // 0x8 alone: a Client entry with no SpecialInterval — the write would be
        // INERT, and this is the read that stops us making it.
        let no_flag = REG_PARAMETERS.replace("time.windows.com,0x9", "time.windows.com,0x8");
        assert_eq!(parse_special_interval_flag(&no_flag), Some(false));
        // Several servers, one flagged.
        let mixed = REG_PARAMETERS.replace(
            "time.windows.com,0x9",
            "ntp1.example.org,0x8 ntp2.example.org,0x9",
        );
        assert_eq!(parse_special_interval_flag(&mixed), Some(true));
        // A key with no NtpServer value at all.
        assert_eq!(parse_special_interval_flag("HKEY_LOCAL_MACHINE\\x\n"), None);
    }

    #[test]
    fn the_min_poll_floor_comes_off_the_registry() {
        assert_eq!(
            parse_reg_dword(REG_CONFIG_MINPOLL, "MinPollInterval"),
            Some(10)
        );
        assert_eq!(min_poll_secs(10), 1_024);
        assert_eq!(min_poll_secs(15), 32_768);
        assert_eq!(
            min_poll_secs(99),
            u32::MAX,
            "a nonsense exponent must not shift-overflow"
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
            poll_secs: Some(1_024),
            special_interval_flag: Some(true),
            min_poll_secs: Some(1_024),
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
        f.poll_secs = Some(32_768);
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

    #[test]
    fn a_drifting_machine_gets_the_poll_write() {
        let mut f = healthy_windows();
        f.poll_secs = Some(32_768); // the observed Windows default
        assert_eq!(decide(&f).repair, Repair::SetPollInterval(1_024));
    }

    /// ⚠️ Without the `0x1` SpecialInterval flag the write is INERT — W32Time
    /// polls adaptively and ignores `SpecialPollInterval` entirely. Detect that
    /// and report it; writing anyway would have us reporting a change that never
    /// happened. And the flag is never added to a server entry the operator set.
    #[test]
    fn no_special_interval_flag_means_no_write_at_all() {
        let mut f = healthy_windows();
        f.poll_secs = Some(32_768);
        f.special_interval_flag = Some(false);
        let d = decide(&f);
        assert_eq!(
            d.repair,
            Repair::None,
            "the write would do nothing — do not make it"
        );
        assert!(
            d.detail.contains("adaptive"),
            "and say why nothing happened: {}",
            d.detail
        );
    }

    /// ⚠️ GUARD 11. `SpecialPollInterval` is clamped up to `2^MinPollInterval`,
    /// so on a machine whose floor is ABOVE our target, asking for 1024 asks for
    /// something that cannot happen. Ask for the floor.
    #[test]
    fn a_higher_min_poll_floor_is_what_gets_requested() {
        let mut f = healthy_windows();
        f.poll_secs = Some(32_768);
        f.min_poll_secs = Some(4_096); // MinPollInterval = 0xc
        assert_eq!(
            decide(&f).repair,
            Repair::SetPollInterval(4_096),
            "requesting below the floor requests a number the machine cannot use"
        );
    }

    #[test]
    fn a_machine_already_at_or_below_the_target_is_not_written_to() {
        let mut f = healthy_windows();
        f.poll_secs = Some(1_024);
        assert_eq!(decide(&f).repair, Repair::None);
        f.poll_secs = Some(512);
        assert_eq!(
            decide(&f).repair,
            Repair::None,
            "faster than we would ask is fine"
        );
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
        // Linux and macOS: timesyncd caps at 2048 s and chrony at 1024 s, both
        // at or tighter than what we would ask Windows for.
        for (running, synced, poll) in [
            (false, false, None),
            (true, false, None),
            (true, true, Some(32_768)),
        ] {
            let f = Findings {
                service_present: true,
                service_running: running,
                synced,
                poll_secs: poll,
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
    /// queries a day against volunteer infrastructure, from a machine-wide
    /// setting that survives uninstalling Nexus.
    #[test]
    fn never_writes_the_ntp_server_key() {
        let every_repair = [
            Repair::None,
            Repair::StartService,
            Repair::Resync { rediscover: true },
            Repair::Resync { rediscover: false },
            Repair::SetPollInterval(1_024),
            Repair::SetPollInterval(4_096),
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
                // Nothing may write ANY W32Time key but the poll interval.
                if line.contains("reg.exe") {
                    assert!(
                        line.contains(POLL_INTERVAL_KEY) && line.contains("SpecialPollInterval"),
                        "{r:?} writes an unexpected registry value: {line}"
                    );
                }
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
            NTP_SERVER_KEY,
            "/v",
            "NtpServer",
            "/d",
            "pool.ntp.org,0x1",
        ];
        let line = forbidden.join(" ");
        assert!(line.contains("NtpServer") && line.to_lowercase().contains("pool.ntp.org"));
        // …and the real repairs are non-empty, so the sweep has something to scan.
        assert!(!repair_commands(Repair::SetPollInterval(1_024)).is_empty());
        assert!(!repair_commands(Repair::StartService).is_empty());
    }

    #[test]
    fn the_poll_write_asks_for_the_decided_value_and_then_resyncs() {
        let cmds = repair_commands(Repair::SetPollInterval(DESIRED_POLL_SECS));
        assert_eq!(cmds.len(), 2, "one write, one resync, one UAC prompt");
        assert_eq!(cmds[0][0], "reg.exe");
        assert!(cmds[0].contains(&"SpecialPollInterval".to_string()));
        assert!(
            cmds[0].contains(&"1024".to_string()),
            "the operator's decided value"
        );
        assert!(cmds[0].contains(&"REG_DWORD".to_string()));
        assert_eq!(cmds[1], vec!["w32tm.exe", "/resync"]);
    }

    /// ⚠️ GUARD 11's other half: report what was ACHIEVED, not what was asked.
    #[test]
    fn the_achieved_interval_is_read_back_from_the_service() {
        // A machine whose floor clamped our 1024 up to 4096.
        let after =
            W32TM_STATUS_VERBOSE.replace("Poll Interval: 15 (32768s)", "Poll Interval: 12 (4096s)");
        assert_eq!(achieved_poll_secs(&after), Some(4_096));
        // A machine that ignored the write entirely (no SpecialInterval flag)
        // still reports its old value — which is exactly why we read it back.
        assert_eq!(achieved_poll_secs(W32TM_STATUS_VERBOSE), Some(32_768));
    }

    /// ⚠️ GUARD 11's WORDING. "I asked for 1024" and "the machine is using 1024"
    /// are different claims, and the shipped app may only make the second one.
    #[test]
    fn the_outcome_reports_what_was_achieved_not_what_was_asked() {
        assert_eq!(
            poll_write_note(1_024, Some(1_024)),
            "the Windows Time service now checks the time every 1024 s"
        );
        // Clamped up by a higher `MinPollInterval` — say the real number.
        let clamped = poll_write_note(1_024, Some(4_096));
        assert!(
            clamped.contains("asked") && clamped.contains("4096"),
            "{clamped}"
        );
        // The write was ignored entirely (no `0x1` SpecialInterval flag), so the
        // service is still on the Windows default. The operator must not be told
        // this worked.
        let ignored = poll_write_note(1_024, Some(32_768));
        assert!(ignored.contains("32768"), "{ignored}");
        assert!(
            !ignored.contains("now checks"),
            "an ignored write is not a success: {ignored}"
        );
        // Could not read it back: state the uncertainty rather than smoothing it.
        let unknown = poll_write_note(1_024, None);
        assert!(unknown.contains("could not read back"), "{unknown}");
    }

    #[test]
    fn a_failed_or_terminal_repair_says_so_plainly() {
        let d = ClockDiagnosis {
            owner: ClockOwner::OsService("the Windows Time service".into()),
            state: ServiceState::Stopped,
            poll_secs: None,
            special_interval_flag: None,
            min_poll_secs: None,
            unvouched: false,
            repair: Repair::StartService,
            detail: "the Windows Time service is not running".into(),
        };
        assert!(repair_outcome_note(&d, false, false).contains("could not fix it"));
        let terminal = repair_outcome_note(&d, false, true);
        assert!(terminal.contains("stopped trying"), "{terminal}");
        assert!(
            terminal.contains("not running"),
            "and still says what is wrong: {terminal}"
        );
        assert_eq!(
            repair_outcome_note(&d, true, false),
            "started the Windows Time service"
        );
    }

    /// The elevated helper never runs anything off Windows, and never runs
    /// anything at all for a `Repair::None`. (On Linux this also proves the
    /// whole function is inert — nothing is spawned by the test suite.)
    #[test]
    fn the_elevated_helper_does_nothing_without_a_repair() {
        assert!(!run_repair_elevated(Repair::None));
        if !cfg!(windows) {
            assert!(!run_repair_elevated(Repair::SetPollInterval(1_024)));
            assert!(!run_repair_elevated(Repair::StartService));
        }
    }

    // ── Guard 9: the rate limit ───────────────────────────────────────────────

    #[test]
    fn ten_detections_in_ten_minutes_produce_one_repair() {
        let mut lim = RepairLimiter::new();
        let t0 = Instant::now();
        let mut attempts = 0;
        for i in 0..10 {
            let now = t0 + Duration::from_secs(i * 60);
            if lim.may_attempt(now) {
                attempts += 1;
                lim.record(now, true);
            }
        }
        assert_eq!(attempts, 1, "at most one repair an hour");
        // …and an hour later, one more.
        assert!(lim.may_attempt(t0 + Duration::from_secs(3_600)));
    }

    #[test]
    fn three_consecutive_failures_are_terminal() {
        let mut lim = RepairLimiter::new();
        let t0 = Instant::now();
        for i in 0..3 {
            let now = t0 + Duration::from_secs(i * 3_600);
            assert!(lim.may_attempt(now), "attempt {i} allowed");
            lim.record(now, false);
        }
        assert!(lim.is_terminal());
        assert!(
            !lim.may_attempt(t0 + Duration::from_secs(100 * 3_600)),
            "no fourth attempt, however long we wait"
        );
    }

    #[test]
    fn a_success_clears_the_failure_count() {
        let mut lim = RepairLimiter::new();
        let t0 = Instant::now();
        lim.record(t0, false);
        lim.record(t0 + Duration::from_secs(3_600), false);
        lim.record(t0 + Duration::from_secs(7_200), true);
        assert!(!lim.is_terminal());
        assert!(lim.may_attempt(t0 + Duration::from_secs(10_800)));
    }
}
