//! Launching Hamlib's `rigctld` daemon ourselves.
//!
//! Instead of depending on `libhamlib` at build time, Tempo shells out to the
//! `rigctld` binary the operator already has installed and talks to it over TCP
//! (see [`crate::rig`]). [`rigctld_args`] builds the command line and is pure /
//! unit-tested; [`spawn_rigctld`] launches it and returns a kill-on-drop
//! [`Child`] so the daemon dies with Tempo.

use std::collections::VecDeque;
use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// What a serial control line is held at for the whole session (`-C rts_state=…` /
/// `-C dtr_state=…`), per line, as the operator chose it.
///
/// **Why any of this exists.** A serial port's driver raises RTS and DTR when the port is
/// opened, and Hamlib does not put them back down except on a line it is itself keying: `rig.c`
/// `rig_open` lowers a line only in the `RIG_PTT_SERIAL_RTS` / `_DTR` arms ("Needed on Linux
/// because the serial port driver sets RTS/DTR on open — only need to address the PTT line as we
/// offer config parameters to control the other"); the `RIG_PTT_NONE` / `_RIG` / `_RIG_MICDATA`
/// arms `break` without touching a pin. Its own force-low pair in `serial.c` is commented out
/// ("This fails on Linux and MacOS with hardware flow control"). Nexus's PTT default is `vox`, so
/// we pass no `-P` and land on exactly that do-nothing arm — and an interface that keys on RTS is
/// then **keyed by the act of connecting, for the whole session**. Hamlib's own answer to this is
/// these two config parameters (`rts_state` / `dtr_state`, values `Unset`/`ON`/`OFF`,
/// `src/serial_cfg_params.h` + `src/conf.c` `frontend_set_conf`), applied by `iofunc.c`
/// `port_open` AFTER `serial_setup` has finished — which is why using them is not the upstream
/// bug re-created: that one forced the pins low BEFORE termios was configured.
///
/// **[`Low`](LineState::Low) is Nexus's default** and is the fix. [`High`](LineState::High) is
/// for an accessory that draws its supply from the line (Hamlib's own wording for these
/// parameters is "for external powering"): an RS-232-era homebrew CI-V level converter, a
/// K1EL-style serial keyer. [`Untouched`](LineState::Untouched) says nothing at all and is
/// exactly 1.0.1 behaviour — the recovery hatch, and the value every failure path falls back to.
/// WSJT-X ("Force Control Lines: DTR/RTS = High|Low|blank") and fldigi both expose the same
/// three-way per line; they default to leave-alone, Nexus defaults to Low.
///
/// **PLATFORMS — this is deliberately NOT `cfg(unix)`.** Hamlib implements these parameters on
/// every platform (`ser_set_rts`/`ser_set_dtr` are `TIOCMBIC` on POSIX and, through Hamlib's own
/// `win32_serial_ioctl`, `EscapeCommFunction(CLRRTS/CLRDTR)` on Windows — `lib/termios.c`), so
/// the flags do real work on Windows, which is where nearly every Nexus operator is.
///
/// What differs is only how much was already true there, and it is NOT the same for the two
/// lines. Hamlib's Windows `tcsetattr` sets `fDtrControl = DTR_CONTROL_DISABLE` itself
/// (`lib/termios.c:2727`), so DTR is already driven low at open; it never touches
/// `fRtsControl`, which keeps whatever `GetCommState` returned from the driver — and RTS is
/// exactly the line the field report is about. On Linux the tty driver raises both and Hamlib's
/// own force-low pair is commented out. So: DTR-low is a documented no-op on Windows, RTS-low
/// is not, and setting an already-low line low costs nothing anywhere. Scoping this to Unix
/// would withhold the fix from the platform that needs it most.
///
/// ⚠️ NOT VERIFIED ELECTRICALLY, on any platform. Every Hamlib claim above is read from the
/// 4.7.1 sources and every refusal was re-observed by running the shipped `rigctld.exe`; nobody
/// here has watched a pin move. There is no serial rig on this machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineState {
    /// Emit nothing for this line. Exactly what Nexus did before 1.0.2, and the fallback for
    /// every case we cannot positively prove safe — see [`SettableLines`].
    #[default]
    Untouched,
    /// `-C <line>_state=OFF`: held low for the session, so opening the port cannot key.
    Low,
    /// `-C <line>_state=ON`: held high for the session, to power a line-fed accessory.
    High,
}

impl LineState {
    /// The exact Hamlib `<line>_state` value, or `None` for "say nothing". Hamlib parses these
    /// with `strcmp` in `conf.c` `frontend_set_conf` — case-sensitive, `"OFF"`/`"ON"` exactly —
    /// and a value it does not recognise is `Invalid parameter`, which exits rigctld with
    /// status 2 (observed). Kept in one place so the spelling cannot drift.
    fn value(self) -> Option<&'static str> {
        match self {
            LineState::Untouched => None,
            LineState::Low => Some("OFF"),
            LineState::High => Some("ON"),
        }
    }

    /// Parse the operator's stored setting (`Settings::cat_rts_state` / `cat_dtr_state`).
    /// Anything unrecognised — including the empty string an older settings file or a
    /// half-wired UI can produce — means the SAFE default, not "leave it high".
    pub fn from_setting(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "untouched" => LineState::Untouched,
            "high" => LineState::High,
            _ => LineState::Low,
        }
    }
}

/// What the operator wants each of the two control lines held at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ControlLines {
    pub rts: LineState,
    pub dtr: LineState,
    /// Emit `-C serial_handshake=None`, so RTS stops being flow control and CAN be held.
    ///
    /// Set only by [`resolve_lines`], and only when the hardware-handshake declaration is the
    /// ONE thing standing between us and holding RTS low — see [`SettableLines`] refusal #2.
    /// On ~50 backends (TS-2000, TS-590, FTDX10, FT-991, FT-891…) Hamlib refuses `rts_state`
    /// outright, so before this Nexus fell back to `Untouched` and the port came up with RTS
    /// asserted by flow control: on a Digirig-style cable, where RTS IS the PTT, that is the
    /// rig keyed from the moment you connect and for the whole session. Reported by vk6mo
    /// (#44, TS-2000 + Digirig Mobile) — and note his symptom, that it happened whatever the
    /// PTT setting was, which is exactly what a flow-control line does: it is not keying, so
    /// no PTT choice touches it.
    ///
    /// Legal because `rig_open` tests `rp->parm.serial.handshake`, the RUNTIME value, not
    /// `caps->serial_handshake` — verified against Hamlib 4.7.1 `src/rig.c`, so a handshake
    /// set through `-C` before open genuinely frees the line rather than merely disagreeing
    /// with the backend.
    ///
    /// ⚠️ THE TRADE, RE-SCOPED AFTER A FIELD REGRESSION (KD9TAW bench, 2026-08-09): the
    /// 2026-08-08 version dropped the handshake for EVERY hardware-handshake backend,
    /// because `cat_rts_state` defaults to "low" — and an FTDX10 / FT-991 at its factory
    /// default (CAT RTS = ENABLE) gates its CAT replies on the PC asserting RTS, so
    /// handshake=None + RTS held low made both bench Yaesus completely CAT-dead while the
    /// native-CI-V Icoms sailed on. The override is therefore gated on `rts_deliberate`
    /// ([`resolve_lines`]): RTS at this port must be POSITIVELY known to be wired to
    /// something — a recognised keying-interface cable (the Digirig class that keyed
    /// vk6mo's TS-2000, which stays protected) or a serial keying line declared on this
    /// port. The blanket safety default alone never costs a rig its handshake again.
    /// Residual, stated: an UNRECOGNISED RTS-wired cable on a hardware-handshake rig with
    /// non-serial PTT keeps the 1.0.5 keyed-on-connect hazard — the narrow price of not
    /// killing factory-default CAT on ~50 backends.
    pub handshake_none: bool,
}

impl ControlLines {
    /// The safety default: both lines held low.
    pub fn hold_low() -> Self {
        Self {
            rts: LineState::Low,
            dtr: LineState::Low,
            handshake_none: false,
        }
    }
}

/// Which control lines Hamlib will **accept** a `<line>_state` for on this rig at all —
/// established from the daemon itself by [`settable_lines_for`], never guessed.
///
/// **The two refusals, both re-observed against the bundled rigctld 4.7.1.** They apply to
/// `ON` exactly as to `OFF`: `rig_open` tests only that the state is *set*.
/// 1. `rig.c:1253-1272` — the line rigctld is *keying with*. Observed:
///    `rig_open: cannot set DTR with PTT by DTR "COM99"` on model 1007, and
///    `rig_open: cannot set RTS with PTT by RTS "COM99"` on model 1031, **with no `-P` passed**.
/// 2. `rig.c:1241-1249` — RTS on a backend whose serial handshake is hardware (RTS/CTS),
///    whatever the PTT type: `rig_open: cannot set RTS with hardware handshake "COM99"`.
///    ~50 backends declare it (FTDX10, FT-991, FT-891, TS-2000, TS-590, TS-990S…). DTR is
///    unaffected: it is not a flow-control line.
///
/// **A wrong flag fails in two different ways, and only one of them is loud** (both observed):
/// - A bad *value* — `-C rts_state=BOGUS` — is `Config parameter error: Invalid parameter` and
///   rigctld **exits with status 2** before it ever listens, so [`RigctldProc::is_alive`] is
///   false and the caller already refuses to connect. That is why [`LineState::value`] is the
///   one place either token is spelled.
/// - A refused *combination* — a legal value on a line `rig_open` will not have — is silent:
///   `rig_open` returns `-RIG_ECONF` and rigctld does **not** exit on it ("continue even if
///   opening the rig fails, because it may be powered off"), so it goes on serving a rig it
///   never opened. The operator sees CAT that connects and does nothing, and keying that is
///   dead too. (An unknown `-C` *token* is quieter still — one log line, daemon fine.)
///
/// That asymmetry is the whole design rule here: **`false` is always safe** (it means "say
/// nothing", i.e. 1.0.1), and `true` is only ever set from something the daemon positively told
/// us. Missing a hazard costs the operator nothing they did not already have; guessing wrong
/// costs them a working radio.
///
/// It has to be ASKED rather than tabulated: between 4.5.5 and 4.7.1 the FTDX10, TS-2000, TS-590
/// and FT-9000 all *gained* the hardware-handshake declaration, so a model list baked in at one
/// release would have killed CAT on the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettableLines {
    /// Hamlib accepts `rts_state`. False for a hardware-handshake backend, and for the
    /// keying line.
    pub rts: bool,
    /// Hamlib accepts `dtr_state`. False for the keying line.
    pub dtr: bool,
    /// `rts` is false for ONE recoverable reason: refusal #2, the hardware-handshake
    /// declaration — the line is offered and we are not keying with it. Distinguished from
    /// the other reasons because it is the only one an override can lift: keying with RTS is
    /// a real conflict, and a backend that offers no `rts_state` at all has nothing to set.
    /// Drives [`ControlLines::handshake_none`].
    pub rts_taken_by_handshake: bool,
}

/// Narrow the operator's wishes to what this rig's Hamlib will actually accept. A line we
/// cannot positively prove settable is left [`Untouched`](LineState::Untouched) — 1.0.1.
///
/// `rts_deliberate` — is RTS at this port KNOWN to be wired to something (a recognised
/// keying-interface cable, or a serial keying line declared on this very port)? The
/// handshake override is gated on it: see the ⚠️ in [`ControlLines::handshake_none`].
fn resolve_lines(
    settable: SettableLines,
    want: ControlLines,
    rts_deliberate: bool,
) -> ControlLines {
    // Refusal #2 is the one refusal we can lift: drop the rig's declared hardware handshake so
    // RTS stops being flow control, and the line we could not hold becomes holdable. Only when
    // the operator actually wants it held — an `Untouched` wish buys nothing and would trade
    // away someone's flow control for no gain — AND only when RTS at this port is a
    // DELIBERATE thing (`rts_deliberate`): the blanket hold-low safety default must never be
    // the reason a rig loses its declared handshake. See [`ControlLines::handshake_none`] —
    // the KD9TAW bench regression (2026-08-09) is why this gate exists.
    let free_rts = !settable.rts
        && settable.rts_taken_by_handshake
        && want.rts != LineState::Untouched
        && rts_deliberate;
    ControlLines {
        rts: if settable.rts || free_rts {
            want.rts
        } else {
            LineState::Untouched
        },
        dtr: if settable.dtr {
            want.dtr
        } else {
            LineState::Untouched
        },
        handshake_none: free_rts,
    }
}

/// Build the `rigctld` argument vector.
///
/// Produces e.g. `["-m", "3073", "-r", "COM5", "-s", "38400", "-t", "4532"]`:
/// - `-m <model>` Hamlib rig model number.
/// - `-r <addr>` the radio: a serial device (COM5, /dev/ttyUSB0) OR, when `network`, a
///   `host:port` (e.g. a FlexRadio's SmartSDR IP:4992) — Hamlib's network backends take the
///   host:port on `-r`. Omitted when `addr` is empty (Dummy / NET rigs that need no port).
/// - `-s <baud>` serial speed — omitted for a network rig (`network`) or an empty addr, since
///   a TCP rig has no baud rate.
/// - `-t <tcp_port>` TCP port for the daemon to listen on.
/// - `-P <RTS|DTR>` / `-p <addr>` ONLY when `ptt_line` is set: the daemon ALSO keys the
///   transmitter on the very same serial port it uses for CAT. This is the single-cable
///   interface case (Digirig Mobile), where keying and control share one port and we
///   therefore cannot open the line ourselves — rigctld holds it.
///
///   Safe because Hamlib explicitly SHARES the file descriptor rather than opening the port
///   twice (`rig.c`, `rig_open`): for `RIG_PTT_SERIAL_RTS`/`_DTR` it copies the rig pathname
///   when the PTT pathname is empty, then `if (!strcmp(pttp->pathname, rp->pathname))
///   { pttp->fd = rp->fd; }`. We pass `-p` explicitly anyway rather than relying on that
///   defaulting, so the intent is visible in the daemon's own log line.
///
///   ⚠️ The type token MUST be one Hamlib recognises — `RIG`, `DTR`, `RTS`, `PARALLEL`,
///   `NONE` (rigctld(1) of the bundled 4.7.x). Anything else does NOT error: rigctld prints
///   "Unrecognised PTT type, using NONE" and comes up with keying silently disabled, which
///   looks exactly like a dead rig. `ptt_type_token` is therefore derived from the enum, not
///   spelled at the call site, and is pinned by tests.
/// - `-C rts_state=…` / `-C dtr_state=…` per [`ControlLines`]: hold a control line at a fixed
///   level for the whole session, so opening the port cannot key the transmitter. `lines` must
///   already have been narrowed by [`resolve_lines`] to what this rig's Hamlib accepts — see
///   [`SettableLines`] for the two refusals and why a wrong flag is silently dead CAT.
/// - `-vvv` — **why the daemon says anything at all, and why it says the RIGHT thing.** rigctld
///   hands its `-v` count straight to `rig_set_debug`, so with no flag Hamlib runs at
///   `RIG_DEBUG_NONE` and prints NOTHING; the stderr pipe [`spawn_rigctld`] opens then drains an
///   empty stream, which is exactly how a failing FT-847 produced a support report saying
///   "nothing noteworthy". `-vv` is `RIG_DEBUG_ERR`, the first level that emits at all — but it
///   covers the OPEN (`serial_open: serial port COM99 does not exist`), and the commonest fault
///   behind that report is not an open: it is *the port opened and the rig never answered*, whose
///   explaining line is `RIG_DEBUG_WARN`. `-vvv` is that level, it costs nothing on a healthy rig
///   (measured: byte-identical output at `-vv` and `-vvv` over 10 serial polls and 30 network
///   polls that all succeeded), and `-vvvv` is where the per-transaction narration starts and is
///   the level this must never reach. Full measurements, both directions, on
///   `the_daemon_is_launched_at_the_verbosity_that_reports_a_rig_that_never_answered`.
pub fn rigctld_args(
    model: u32,
    addr: &str,
    baud: u32,
    tcp_port: u16,
    network: bool,
    ptt_line: Option<crate::rig::SerialLine>,
    lines: ControlLines,
) -> Vec<String> {
    // Errors AND the read timeouts that are the commonest fault — see the `-vvv` bullet above.
    // Unconditional: a fault the operator needs explaining is no more likely on a serial rig
    // than a network one.
    let mut args = vec!["-vvv".to_string(), "-m".to_string(), model.to_string()];
    if !addr.is_empty() {
        args.push("-r".to_string());
        args.push(addr.to_string());
        if !network {
            args.push("-s".to_string());
            args.push(baud.to_string());
        }
    }
    // Keying on the CAT port. Meaningless without a port to key (`addr` empty) or over a
    // network transport — a TCP rig has no RTS line — so it is gated on both.
    if let Some(line) = ptt_line {
        if !addr.is_empty() && !network {
            args.push("-P".to_string());
            args.push(ptt_type_token(line).to_string());
            args.push("-p".to_string());
            args.push(addr.to_string());
        }
    }
    // Hold the control lines at a fixed level for the session so that merely opening the port
    // cannot key the transmitter (see [`ControlLines`]). Gated on the same two things as keying
    // — no control lines exist over TCP, and an empty addr has no port.
    //
    // The `ptt_line` re-check is deliberate belt-and-braces. [`settable_lines_for`] already asks
    // the daemon with the very same `-P` we are about to pass, so the keying line comes back
    // unsettable; this second, independent guard means a caller that hand-builds `lines` (a
    // test, a future call site) still cannot ask Hamlib to hold the line it is keying with.
    if !addr.is_empty() && !network {
        // Drop the declared hardware handshake FIRST. Order is cosmetic to Hamlib — rigctld
        // applies every -C before rig_open, and rig_open is where both are read — but it reads
        // as the precondition it is, and the daemon's own log line then shows why RTS was
        // settable on a backend that declares otherwise.
        if lines.handshake_none {
            args.push("-C".to_string());
            args.push("serial_handshake=None".to_string());
        }
        if ptt_line != Some(crate::rig::SerialLine::Rts) {
            if let Some(v) = lines.rts.value() {
                args.push("-C".to_string());
                args.push(format!("rts_state={v}"));
            }
        }
        if ptt_line != Some(crate::rig::SerialLine::Dtr) {
            if let Some(v) = lines.dtr.value() {
                args.push("-C".to_string());
                args.push(format!("dtr_state={v}"));
            }
        }
    }
    // Bind LOOPBACK ONLY (#53). rigctld's default listen address is the wildcard, so every
    // install was exposing raw rig control to the LAN while the share UI promised "on this
    // computer". The test harness always passed `-T 127.0.0.1`; production now matches it.
    // Everything that talks to this daemon — Nexus itself, a shared local logger — is local
    // by design, and the CAT broker is the advertised share endpoint.
    args.push("-T".to_string());
    args.push("127.0.0.1".to_string());
    args.push("-t".to_string());
    args.push(tcp_port.to_string());
    args
}

/// The exact `--ptt-type` token Hamlib parses for a serial keying line. Kept as a function so
/// the spelling exists in ONE place: a typo here does not fail loudly, it silently disables
/// keying (see [`rigctld_args`]).
pub fn ptt_type_token(line: crate::rig::SerialLine) -> &'static str {
    match line {
        crate::rig::SerialLine::Rts => "RTS",
        crate::rig::SerialLine::Dtr => "DTR",
    }
}

/// A spawned `rigctld` that is killed when this handle is dropped — and, on
/// Windows, also when Tempo's *process* exits even if this handle is never
/// dropped (e.g. the detached radio thread is torn down at shutdown without
/// unwinding). On drop it kills + reaps the child; on Windows the daemon is also
/// placed in a Job Object with `KILL_ON_JOB_CLOSE`, so closing the job handle
/// (explicitly on drop, or implicitly by the OS at process exit) kills rigctld
/// and frees the serial/COM port. This is what prevents a stuck COM port after
/// closing Tempo.
pub struct RigctldProc {
    child: Child,
    /// What the daemon has printed on stderr, filled by the drain thread in [`spawn_rigctld`]:
    /// the newest lines, plus the best-ranked line of this whole connection attempt — see
    /// [`Said`] and [`RigctldProc::said`].
    said: Arc<Mutex<Said>>,
    /// Job-object handle (Windows) as an `isize`; 0 = none. Held for the daemon's
    /// lifetime so the kill-on-close guarantee is in force until Tempo exits.
    #[cfg(windows)]
    job: isize,
}

/// How many of the daemon's stderr lines are kept **as recent context**. Bounded because the
/// loudest case is a rig that is merely MUTE: at `-vvv` Hamlib reports the read timeout AND the
/// backend's own error on every poll, forever, so an unbounded buffer would grow for as long as
/// the operator leaves the app open. A healthy rig contributes nothing to push anything out (it
/// prints nothing at this level at all).
///
/// ⚠️ **This window is no longer what carries the diagnosis, and the round-three claim that it
/// was is FALSIFIED.** That round re-measured the per-failure burst sizes below and concluded "an
/// 8-line window still contains a cause line in every failure mode captured". It does not. Those
/// captures were all of a daemon whose ONLY traffic was the fault; a daemon on a rig that answers
/// mis-framed bytes narrates every poll on top of the fault, and the cause is printed once, at
/// `rig_open`, before any of it. Re-measured against the bundled rigctld 4.7.1 driven by a peer
/// that answers garbage and drops the link (`tests/fixtures/rigctld/wrong_baud_flood.log`).
/// Counted, rather than described: **549 lines, 26 of which name the fault, the first on line 1
/// and the last on line 517 — with 32 lines of bookkeeping after it and runs of up to 60 lines
/// between them.** Both numbers are larger than this window, which is the whole point: an 8-line
/// window at the end of that stream holds nothing but `handle_socket:` / `write_block()` /
/// `IO error`, and a probe that failed anywhere inside one of those runs saw the same.
///
/// So the window is now only the recent-context half of [`Said`]; what makes the diagnosis
/// survive is [`Said::best`], which is kept for the whole connection attempt and cannot be
/// pushed out at any volume. Choosing which of the retained lines to SHOW is
/// `service::with_daemon_error`'s job, and it is done by content.
///
/// Per-failure burst sizes measured (bundled rigctld 4.7.1, `-vvv`): missing serial port 3–4
/// lines per reconnect, busy port 4, FT-847 that never answers 5, FTDX10 that never answers ~26,
/// wrong baud 1 — and a wrong baud that keeps the link up, unbounded.
const SAID_KEPT: usize = 8;

/// How much of the fault a daemon line actually names. **The ordering is the rule** — a line
/// that names a cause beats anything, and rigctld's per-connection bookkeeping loses to
/// anything.
///
/// The middle rank is deliberate and is what makes this degrade rather than break: a line
/// neither list has seen still beats bookkeeping and still loses to a known cause, so a Hamlib
/// that rewords one of its messages costs precision, never the mechanism.
///
/// It lives here, beside the ring it now selects INTO, rather than beside the one consumer that
/// used to rank at read time: the ring has to know a cause when it sees one, because by the time
/// `service::with_daemon_error` looks, a cause it did not retain is gone.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Explains {
    /// rigctld narrating its own socket handling. True, and never the answer.
    Bookkeeping,
    /// Anything else — including every line neither list anticipated.
    Plain,
    /// Names the fault itself: the port, the host, the silence, the garbage.
    Cause,
}

/// Fragments that make a line a [`Explains::Cause`]. **Every one was observed coming out of the
/// bundled rigctld 4.7.1**, or is a format string in the `libhamlib-4.dll` beside it; the
/// captures are in `tests/fixtures/rigctld/`.
///
/// ⚠️ `"Timed out"` is CASE-SENSITIVE on purpose and the case is load-bearing. Hamlib's
/// diagnosis is `read_block_generic(): Timed out 1.41 seconds after 0 chars` — capital T, and
/// it carries the char count, which is the difference between "nothing came back at all" (0
/// chars: dead port, powered-off rig) and "something came back garbled" (N chars: wrong baud).
/// Its consequence line is the bare lower-case `Communication timed out`, which says nothing
/// and must not be mistaken for it.
const CAUSE_FRAGMENTS: &[&str] = &[
    "does not exist",           // serial_open: serial port COM99 does not exist
    "is already open",          // serial_open: serial port COM7 is already open  (port busy)
    "Unable to open",           // serial_open: Unable to open COM7 - Permission denied
    "cannot get host",          // network_open: cannot get host "rig.local"
    "Timed out",                // read_block_generic()/read_string_generic(): see above
    "not correctly terminated", // newcat_get_cmd: Command is not correctly terminated '…'
    "cmd validation failed",    // newcat_set_cmd_validate: 'AI0;'!='<the rig's garbage>'
    "rig power is off",         // newcat_get_cmd: rig power is off?
    "cannot set RTS",           // rig_open: cannot set RTS with PTT by RTS / hardware handshake
    "cannot set DTR",           // rig_open: cannot set DTR with PTT by DTR
    "Invalid parameter",        // a `-C` value Hamlib will not parse; rigctld then exits 2
];

/// Prefixes that make a line [`Explains::Bookkeeping`] — all observed, all content-free.
const NOISE_PREFIXES: &[&str] = &[
    "handle_socket:", // rigctld's per-connection rig_open/rig_close/i-o-error narration
    "rigctl_",        // rigctl_chk_vfo / rigctl_dump_state — the CLIENT handshake, not the rig
    "network_flush",  // "network data cleared" — housekeeping
    "*",              // Hamlib's ENTERFUNC/RETURNFUNC trace dump: `**2:newcat.c(2110):… entered`
];

/// Whole lines that are content-free. These are the tail fragments of Hamlib's multi-line error
/// prints — the subject was on the line before, which is the one worth showing.
const NOISE_EXACT: &[&str] = &["IO error", "Communication timed out", ")"];

pub(crate) fn explains(line: &str) -> Explains {
    if CAUSE_FRAGMENTS.iter().any(|f| line.contains(f)) {
        return Explains::Cause;
    }
    if NOISE_EXACT.contains(&line) || NOISE_PREFIXES.iter().any(|p| line.starts_with(p)) {
        return Explains::Bookkeeping;
    }
    Explains::Plain
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
fn said_line(raw: &[u8]) -> Option<String> {
    let line = String::from_utf8_lossy(raw).trim().to_string();
    (!line.is_empty()).then_some(line)
}

/// What the daemon has said about THIS connection attempt: a bounded window of the newest lines,
/// and — kept out of that window's reach — the best-ranked line of the whole attempt.
///
/// ⭐ **The two halves answer two different questions, and one buffer could not answer both.**
/// The window answers "what is happening now"; it has to be bounded, because a daemon on a mute
/// or mis-framing rig narrates for as long as the app is open. `best` answers "what went wrong",
/// and that line is printed ONCE, at `rig_open`, before the narration starts — so any bound at
/// all eventually drops it. Ranking at read time cannot recover it: by then it is not a
/// low-ranked line, it is a line that no longer exists.
///
/// One slot, not a second ring: the fault has one name, and the operator's status pill holds two
/// lines of which the second should be the live end of the stream. **Strictly-better replaces**,
/// so within a rank the FIRST line wins — Hamlib prints the cause first and its consequences
/// after, and a later line of the same rank is a repeat or a knock-on. That also makes this the
/// fallback rather than the answer: `service::with_daemon_error` scans newest-first within a
/// rank, so while the window still holds a line as good, the LIVE one is what the operator sees
/// and this slot only speaks when the window has nothing left to say.
#[derive(Default)]
pub(crate) struct Said {
    window: VecDeque<String>,
    best: Option<(Explains, String)>,
}

impl Said {
    fn push(&mut self, line: String) {
        let rank = explains(&line);
        if self.best.as_ref().is_none_or(|(best, _)| rank > *best) {
            self.best = Some((rank, line.clone()));
        }
        if self.window.len() == SAID_KEPT {
            self.window.pop_front();
        }
        self.window.push_back(line);
    }

    /// Everything retained, oldest first — the kept diagnosis, then the window. The retained
    /// line is omitted when the window still holds it, so a short-lived daemon reads exactly as
    /// it did before there was a second half.
    fn lines(&self) -> Vec<String> {
        self.best
            .iter()
            .map(|(_, l)| l)
            .filter(|l| !self.window.contains(l))
            .chain(self.window.iter())
            .cloned()
            .collect()
    }
}

/// How many lines of recent context the ring keeps ([`SAID_KEPT`]), for a test that needs to
/// prove a capture defeats a window of exactly that size rather than assume it does.
pub fn said_window_len() -> usize {
    SAID_KEPT
}

/// The ring the drain thread would be holding after `raw` — its own line handling and its own
/// bound, as a pure function, so a REAL captured `rigctld` stderr log can be replayed against
/// [`RigctldProc::said`]'s consumer without a daemon, a serial port or a rig.
pub fn said_ring(raw: &[u8]) -> Vec<String> {
    let mut said = Said::default();
    for line in raw.split_inclusive(|b| *b == b'\n') {
        if let Some(l) = said_line(line) {
            said.push(l);
        }
    }
    said.lines()
}

impl RigctldProc {
    /// The underlying child's process id, for logging.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// What Hamlib itself has said about this rig: the newest [`SAID_KEPT`] lines, and ahead of
    /// them the best-ranked line of the whole connection attempt when the window has dropped it
    /// (see [`Said`]).
    ///
    /// **This exists because the operator could not see it.** The daemon's stderr has been
    /// captured since the CI-V diagnostic was built, and it went to exactly one place: the
    /// CI-V log file, whose toggle the UI renders only for an Icom on the native CI-V path.
    /// Every Yaesu, Kenwood, Elecraft and Flex owner's Hamlib errors were captured and
    /// discarded — which is why an FT-847 owner whose rig works in WSJT-X reported "nothing
    /// noteworthy" while his daemon knew precisely what was wrong.
    pub fn said(&self) -> Vec<String> {
        self.said.lock().map(|s| s.lines()).unwrap_or_default()
    }

    /// True if the daemon is still running. rigctld that fails to bind its TCP port (e.g. the port is
    /// already taken by another radio's daemon or the CAT broker) exits immediately — so a `false`
    /// here means "did NOT get the port": the caller must NOT connect, or it would land on whatever
    /// FOREIGN daemon holds the port (the dual-radio crossed-CAT bug). Non-blocking.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for RigctldProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(windows)]
        if self.job != 0 {
            // SAFETY: `job` is a valid job-object HANDLE we created and have not
            // yet closed. Closing it triggers KILL_ON_JOB_CLOSE for any survivor.
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.job as *mut core::ffi::c_void);
            }
        }
    }
}

/// Place `child` in a new Job Object set to kill its processes when the job
/// handle closes, so rigctld dies with Tempo (clean exit, crash, or detached-
/// thread teardown). Returns the job HANDLE as an `isize` (0 on any failure, in
/// which case we just fall back to the Drop-time kill).
#[cfg(windows)]
fn assign_kill_on_close_job(child: &Child) -> isize {
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

/// Extra absolute directories to search for a Hamlib binary once it is neither bundled next to
/// the app nor found by name below — the last resort before giving up and handing `Command` the
/// bare name (which fails silently into a PATH lookup).
///
/// **Why this exists at all.** A GUI app launched from Finder/Dock/Spotlight is started by
/// launchd, which on every macOS version hands it the fixed environment
/// `PATH=/usr/bin:/bin:/usr/sbin:/sbin` — never the interactive shell's `PATH`, and so never
/// Homebrew's install prefix, no matter what the operator's own `.zshrc` says or how their
/// terminal resolves `rigctld`. `Command::new("rigctld")` from inside Nexus therefore fails even
/// on a station where `which rigctld` works fine in Terminal — confirmed on 2026-08-07 (Nexus's
/// own `PATH` inside a Finder-launched process was verified as exactly those four directories,
/// while `rigctld` sat in `/opt/homebrew/bin`, on `PATH` for every shell but invisible to Nexus).
/// Linux desktop launchers can hit the same gap for a Hamlib built from source into
/// `/usr/local/bin`, so this list is not Homebrew-only.
#[cfg(unix)]
const HAMLIB_SEARCH_DIRS: &[&str] = &[
    "/opt/homebrew/bin", // Homebrew, Apple Silicon
    "/usr/local/bin",    // Homebrew, Intel Mac; also a common manual/from-source install prefix
    "/opt/local/bin",    // MacPorts
];

/// Return the first `dirs` entry that contains a file named `bin_name`, absolute path — the
/// search core of [`resolve_rigctld`] / [`resolve_rotctld`] / [`resolve_rigctl`]'s fallback,
/// factored out so it can be driven by a fixture list in tests instead of the real
/// [`HAMLIB_SEARCH_DIRS`].
#[cfg(unix)]
fn find_in_dirs(dirs: &[&str], bin_name: &str) -> Option<std::ffi::OsString> {
    dirs.iter()
        .map(|dir| std::path::Path::new(dir).join(bin_name))
        .find(|p| p.is_file())
        .map(std::path::PathBuf::into_os_string)
}

/// Does `path_var` (a `PATH`-shaped, `:`-joined list of directories) resolve `bin_name`? The
/// testable core of [`on_path`] — takes the value as a parameter instead of reading the real
/// process environment, so a test can drive it without mutating global state that other tests
/// (the fixture stand-in below) already depend on being left alone.
#[cfg(unix)]
fn path_has(path_var: &std::ffi::OsStr, bin_name: &str) -> bool {
    std::env::split_paths(path_var).any(|dir| dir.join(bin_name).is_file())
}

/// Does the CURRENT process's own `PATH` already resolve `bin_name`? Read-only mirror of the
/// search `Command::new(bin_name)` is about to do itself.
///
/// **Why check this instead of just always trying [`HAMLIB_SEARCH_DIRS`] first.** An operator
/// (or a test) may deliberately put a specific `rigctld` earliest on `PATH` — a version pin, a
/// wrapper script, a fixture stand-in — and that intent must outrank Nexus's own guess at common
/// install directories. [`HAMLIB_SEARCH_DIRS`] exists for the case PATH has NOTHING, not to
/// second-guess a PATH that already has something.
#[cfg(unix)]
fn on_path(bin_name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| path_has(&path, bin_name))
}

/// Does `bin` actually RUN, or does it merely exist?
///
/// Existence is not usability, and the gap is not hypothetical: a Hamlib built from source and
/// installed under `~/.local` keeps the configured `--prefix` (`/usr/local/lib/libhamlib.4.dylib`)
/// as its dylib load path, so every `rigctl*` binary is executable, first on `PATH`, and dies at
/// `dyld` load with *"Library not loaded"* before `main`. Found on a real station on 2026-08-13:
/// CAT was dead with no usable diagnosis, because [`on_path`] said yes and the
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
fn runs_ok(bin: &std::ffi::OsStr) -> bool {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    let mut child = match Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false, // not executable at all (ENOENT / EACCES / bad arch)
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2_000);
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

/// Resolve a Hamlib binary on Unix: the operator's `PATH` first, then [`HAMLIB_SEARCH_DIRS`],
/// requiring at each step that the candidate actually runs ([`runs_ok`]).
///
/// The PATH-first order is deliberate and unchanged — an operator who puts a specific build
/// earliest on `PATH` (a version pin, a wrapper, a fixture) still outranks Nexus's guesses. What
/// changed is that a candidate which cannot execute no longer counts as a resolution and no
/// longer suppresses the fallback. Falling all the way through returns the bare name so
/// `Command` produces its own normal "not found" error, exactly as before.
#[cfg(unix)]
fn resolve_hamlib_bin(bin_name: &str) -> std::ffi::OsString {
    if on_path(bin_name) && runs_ok(std::ffi::OsStr::new(bin_name)) {
        return bin_name.into();
    }
    // One directory at a time rather than one `find_in_dirs` call over all of them: the first
    // directory that merely CONTAINS the binary is no longer necessarily the answer, so each
    // candidate has to clear `runs_ok` before the search stops.
    for dir in HAMLIB_SEARCH_DIRS {
        match find_in_dirs(std::slice::from_ref(dir), bin_name) {
            Some(p) if runs_ok(&p) => return p,
            _ => continue,
        }
    }
    bin_name.into()
}

/// Locate the `rigctld` binary. Prefers one **bundled next to the app** — the
/// Windows installer ships Hamlib under the install dir (with its DLLs), so CAT
/// works with no separate Hamlib install — then whatever `rigctld` the process's own `PATH`
/// already resolves, and only when PATH has NOTHING does it try a handful of common
/// package-manager install directories (see [`HAMLIB_SEARCH_DIRS`]) before giving up and handing
/// `Command` the bare name. Launching the bundled exe by full path lets Windows resolve its
/// co-located DLLs (libhamlib-4.dll etc.) from the exe's own directory.
fn resolve_rigctld() -> std::ffi::OsString {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [
                "hamlib/rigctld.exe",
                "resources/hamlib/rigctld.exe",
                "rigctld.exe",
                "hamlib/rigctld",
            ] {
                let p = dir.join(cand);
                if p.is_file() {
                    return p.into_os_string();
                }
            }
        }
    }
    #[cfg(unix)]
    {
        resolve_hamlib_bin("rigctld")
    }
    #[cfg(not(unix))]
    {
        std::ffi::OsString::from("rigctld")
    }
}

/// Build the `rotctld` argument vector — same shape as [`rigctld_args`] minus
/// the network-rig special case (rotators are serial devices to Hamlib).
pub fn rotctld_args(model: u32, port: &str, baud: u32, tcp_port: u16) -> Vec<String> {
    let mut args = vec!["-m".to_string(), model.to_string()];
    if !port.is_empty() {
        args.push("-r".to_string());
        args.push(port.to_string());
        args.push("-s".to_string());
        args.push(baud.to_string());
    }
    // Loopback-only, same defect class as rigctld_args: the wildcard default exposed
    // rotator control to the LAN. Every consumer of this daemon is on this machine.
    args.push("-T".to_string());
    args.push("127.0.0.1".to_string());
    args.push("-t".to_string());
    args.push(tcp_port.to_string());
    args
}

/// The bundled `rotctld` (ships beside rigctld in the Hamlib bundle), then the same
/// [`HAMLIB_SEARCH_DIRS`] fallback, then PATH — same resolution as [`resolve_rigctld`].
fn resolve_rotctld() -> std::ffi::OsString {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [
                "hamlib/rotctld.exe",
                "resources/hamlib/rotctld.exe",
                "rotctld.exe",
                "hamlib/rotctld",
            ] {
                let p = dir.join(cand);
                if p.is_file() {
                    return p.into_os_string();
                }
            }
        }
    }
    #[cfg(unix)]
    {
        resolve_hamlib_bin("rotctld")
    }
    #[cfg(not(unix))]
    {
        std::ffi::OsString::from("rotctld")
    }
}

/// Spawn `rotctld` for a ROTATOR `model` on `port`@`baud`, listening on
/// `tcp_port` — the rotator twin of [`spawn_rigctld`], with the same
/// kill-on-drop + Windows job-object lifetime guarantees (reuses
/// [`RigctldProc`]; the handle is daemon-agnostic).
pub fn spawn_rotctld(
    model: u32,
    port: &str,
    baud: u32,
    tcp_port: u16,
) -> std::io::Result<RigctldProc> {
    let args = rotctld_args(model, port, baud, tcp_port);
    let mut cmd = Command::new(resolve_rotctld());
    cmd.args(&args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let child = cmd.spawn()?;
    #[cfg(windows)]
    let job = assign_kill_on_close_job(&child);
    Ok(RigctldProc {
        child,
        // A rotator daemon's stderr is not piped, so it never says anything here. The field
        // exists because the handle is shared with rigctld, not because rotctld is silent.
        said: Arc::new(Mutex::new(Said::default())),
        #[cfg(windows)]
        job,
    })
}

/// Every `ptt_type` token Hamlib can report, split by whether it names a serial control line.
/// This is Hamlib's OWN vocabulary, printed on the `Combo:` line of every `--show-conf` dump
/// (`conf.c` `frontend_get_conf` `TOK_PTT_TYPE` is what writes the value; the combo list is its
/// mirror). Both halves are pinned so the parser can check that it recognises the WHOLE
/// vocabulary, not just the value it was handed — a Hamlib that added a third serial-line PTT
/// type would otherwise read as "not a line" and kill CAT on it.
const PTT_TOKENS_NOT_A_LINE: [&str; 7] = [
    "RIG",
    "RIGMICDATA",
    "Parallel",
    "CM108",
    "GPIO",
    "GPION",
    "None",
];
/// The `serial_handshake` tokens that leave RTS free for us, and the one that does not.
/// Same allow-list discipline: read the other way ("not Hardware"), a renamed or added token
/// in some future Hamlib would read as safe and kill CAT on every hardware-handshake rig.
const HANDSHAKE_RTS_FREE: [&str; 2] = ["None", "XONXOFF"];
const HANDSHAKE_RTS_TAKEN: [&str; 1] = ["Hardware"];

/// Which control lines Hamlib will accept a `<line>_state` for on `model`, asked of the very
/// `rigctld` we are about to launch, with the very `-P` we are about to pass.
///
/// **The oracle, and why it is `--show-conf` and not `--dump-caps`.** `rigctld -m <n> -u` prints
/// `PTT type: None` for all 309 models and always has: `rigctld.c:569` assigns
/// `my_rig->caps->ptt_type = ptt_type` — the command line's value, defaulting to `RIG_PTT_NONE` —
/// **before** `dumpcaps` runs at `rigctld.c:683`, and `dumpcaps.c:123` prints that clobbered
/// field. (`rigctl` has the identical clobber at `rigctl.c:512`.) So the caps dump is
/// structurally blind to the very thing refusal #1 tests, and reading `ctrl=` out of it can only
/// ever see refusal #2.
///
/// `rigctld -m <n> -L` (`--show-conf`) instead runs at `rigctld.c:668` — after `-P` has been
/// applied to `PTTPORT(rig)->type.ptt` at `:617`, before `rig_open` — and prints
/// `rig_get_conf2` values. `TOK_PTT_TYPE` there reads `pttp->type.ptt` (`conf.c:1117`), which is
/// **the exact field `rig_open`'s refusal tests**, seeded from `caps->ptt_type` in `rig_init`
/// (`rig.c:804`) and untouched by the clobber. `serial_handshake` likewise reads
/// `rp->parm.serial.handshake`, the exact field refusal #2 tests. One 27 ms invocation, exit 0,
/// no port opened (`-L` exits immediately when no `-r` was given, `rigctld.c:670-674`).
///
/// Observed against the bundled 4.7.1: model 1007 (FT-757GXII) → `ptt_type … Value: DTR`;
/// model 1031 (FT-980) → `Value: RTS`; 3073/1042/1/2 → `Value: RIG`/`RIGMICDATA`;
/// 1042 (FTDX-10) → `serial_handshake … Value: Hardware`; models 1 and 2 have no serial conf
/// params at all.
///
/// No binary, a non-zero exit, or output we cannot read at all yields
/// [`SettableLines::default`], i.e. touch nothing — 1.0.1.
///
/// ⚠️ Be precise about *partial* failure, because the claim this replaced ("any failure yields
/// the default") was not true of it. The two lines are decided INDEPENDENTLY, and each `true`
/// needs only its own proof: a dump whose `serial_handshake` is missing or unrecognised still
/// returns `dtr: true` if `dtr_state` is offered and `ptt_type` reads as something other than
/// DTR. That is correct rather than a leak — the handshake refusal is RTS-only
/// (`rig.c:1241-1249` tests `rts_state`) — but it means "unreadable" is a per-line question,
/// not a whole-dump one. What is guaranteed is the direction: no `true` is ever produced
/// without the specific evidence that line needs.
fn settable_lines_for(model: u32, ptt_line: Option<crate::rig::SerialLine>) -> SettableLines {
    let model = model.to_string();
    let mut args = vec!["-m", model.as_str(), "-L"];
    // Ask with the SAME keying override we are about to launch with, so the answer is the
    // effective `pttp->type.ptt` and not the backend default it may be overriding.
    if let Some(line) = ptt_line {
        args.push("-P");
        args.push(ptt_type_token(line));
    }
    daemon_dump(&args)
        .map(|d| parse_settable_lines(&d))
        .unwrap_or_default()
}

/// Run the daemon binary as a ONE-SHOT dump (`-L` show-conf, `-u` dump-caps) and hand back its
/// stdout. Opens no port and keys nothing: both flags print and exit (`rigctld.c:668-674`).
///
/// **This is the shape every "ask, do not tabulate" answer in Nexus takes.** A fact read out of
/// the operator's OWN Hamlib cannot go stale the way a table transcribed from a manual does —
/// which is the whole lesson of the baud-entry rounds. `None` on a missing binary, a non-zero
/// exit, or output we cannot read; every caller turns that into "claim nothing".
pub(crate) fn daemon_dump(args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(resolve_rigctld());
    cmd.args(args);
    cmd.stdin(Stdio::null());
    // Same no-console-window treatment as the daemon itself (Nexus is a GUI app).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.output() {
        Ok(out) if out.status.success() => Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        _ => None,
    }
}

/// The `rigctl` companion binary, resolved exactly like [`resolve_rigctld`]: bundled beside the
/// app first (the Windows installer ships the whole Hamlib tool set together), then `PATH`.
///
/// Used by the baud ladder, which needs ONE read from a rig at ONE rate and then to be gone —
/// a one-shot CLI, not a daemon holding a TCP port. Both binaries ship in the same Hamlib
/// package on every platform Nexus targets, so if `rigctld` is present `rigctl` is too.
///
/// Gated on `serial` because its one caller is (`baud_ladder::rigctl_read`); without the gate the
/// headless `cargo clippy --workspace --all-targets -- -D warnings` CI job fails it as dead code.
#[cfg(feature = "serial")]
pub(crate) fn resolve_rigctl() -> std::ffi::OsString {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [
                "hamlib/rigctl.exe",
                "resources/hamlib/rigctl.exe",
                "rigctl.exe",
                "hamlib/rigctl",
            ] {
                let p = dir.join(cand);
                if p.is_file() {
                    return p.into_os_string();
                }
            }
        }
    }
    #[cfg(unix)]
    {
        resolve_hamlib_bin("rigctl")
    }
    #[cfg(not(unix))]
    {
        std::ffi::OsString::from("rigctl")
    }
}

/// One conf parameter out of a `rigctld --show-conf` dump, as `(value, combo tokens)`.
///
/// The shape is fixed by `rigctl_parse.c` `print_conf_list`: a `<name>: "<tooltip>"` line, then
/// `\tDefault: <d>, Value: <v>`, then for a `RIG_CONF_COMBO` a `\tCombo: a, b, c` line.
/// `None` when the parameter is absent — which is itself load-bearing: Hamlib offers the serial
/// parameters only for `RIG_PORT_SERIAL` (`rig_confparam_lookup`), so their ABSENCE is how a
/// network or device backend is recognised, with no port-type string to parse.
pub(crate) fn conf_param<'a>(dump: &'a str, name: &str) -> Option<(&'a str, Vec<&'a str>)> {
    let mut lines = dump.lines();
    lines.find(|l| l.starts_with(name) && l[name.len()..].starts_with(':'))?;
    // `rfind` not `find`: a Default can itself contain ", Value: " (a pathname could), and the
    // real value is always the LAST such split.
    let value = lines.next()?.rsplit_once(", Value: ")?.1.trim();
    let combo = lines
        .next()
        .and_then(|l| l.trim_start().strip_prefix("Combo: "))
        .map(|c| c.split(", ").map(str::trim).collect())
        .unwrap_or_default();
    Some((value, combo))
}

/// Read one conf parameter as a member of a pinned, fully-enumerated vocabulary.
///
/// Returns `Some(value)` only when Hamlib printed a vocabulary at all, **every** token in it is
/// one we classify, AND the current value is too. A vocabulary that has grown, been reworded, or
/// stopped being printed — the three things a Hamlib bump does — comes back `None`, which every
/// caller turns into "say nothing about this line". That is the whole guard against the
/// enumeration going stale: it is checked against the operator's own Hamlib, at every connect,
/// not against a table written here.
fn known_conf_value<'a>(dump: &'a str, name: &str, vocabulary: &[&str]) -> Option<&'a str> {
    let (value, combo) = conf_param(dump, name)?;
    // An ABSENT combo is a failure, not a pass: without the vocabulary we cannot tell a token we
    // classify from one that merely has not been added yet, and this is the only guard that
    // notices a Hamlib change on its own.
    if combo.is_empty() || !combo.iter().all(|t| vocabulary.contains(t)) {
        return None;
    }
    vocabulary.contains(&value).then_some(value)
}

/// Read a `rigctld --show-conf` dump: which lines Hamlib will accept a `<line>_state` for.
///
/// Every `true` here is positive proof from the dump; everything else — a missing parameter, an
/// unreadable value, a vocabulary token we do not recognise — is `false`, which means 1.0.1.
fn parse_settable_lines(show_conf: &str) -> SettableLines {
    // Hamlib offers `rts_state`/`dtr_state` only for a serial backend, so their presence IS the
    // serial test. Absent → nothing to say (and the flags would be inert noise: rigctld would
    // just log "no such token as 'rts_state'").
    let rts_offered = conf_param(show_conf, "rts_state").is_some();
    let dtr_offered = conf_param(show_conf, "dtr_state").is_some();
    if !rts_offered && !dtr_offered {
        return SettableLines::default();
    }
    // Refusal #1: the keying line. Read from `pttp->type.ptt`, the exact field `rig_open` tests.
    let mut ptt_vocabulary = PTT_TOKENS_NOT_A_LINE.to_vec();
    ptt_vocabulary.push(ptt_type_token(crate::rig::SerialLine::Rts));
    ptt_vocabulary.push(ptt_type_token(crate::rig::SerialLine::Dtr));
    let Some(ptt) = known_conf_value(show_conf, "ptt_type", &ptt_vocabulary) else {
        return SettableLines::default();
    };
    // Refusal #2: RTS on a hardware-handshake backend. Read from `rp->parm.serial.handshake`,
    // the exact field `rig_open` tests — not the `ctrl=` string dumpcaps derives from it.
    let handshake_vocabulary: Vec<&str> = HANDSHAKE_RTS_FREE
        .iter()
        .chain(HANDSHAKE_RTS_TAKEN.iter())
        .copied()
        .collect();
    let handshake = known_conf_value(show_conf, "serial_handshake", &handshake_vocabulary);
    let rts_free = handshake.is_some_and(|h| HANDSHAKE_RTS_FREE.contains(&h));
    let not_keying_rts = ptt != ptt_type_token(crate::rig::SerialLine::Rts);
    SettableLines {
        rts: rts_offered && rts_free && not_keying_rts,
        dtr: dtr_offered && ptt != ptt_type_token(crate::rig::SerialLine::Dtr),
        // Positively Hardware, not merely "not free": an unreadable or unrecognised handshake
        // is the stale-vocabulary case, and the whole discipline here is that such a case
        // claims nothing. Overriding on a token we could not classify would be guessing with
        // someone's flow control.
        rts_taken_by_handshake: rts_offered
            && not_keying_rts
            && handshake.is_some_and(|h| HANDSHAKE_RTS_TAKEN.contains(&h)),
    }
}

/// Spawn `rigctld` for `model` on `serial_port`@`baud`, listening on
/// `tcp_port`. Returns a kill-on-drop handle. Uses the bundled Hamlib if present
/// (see [`resolve_rigctld`]), otherwise a `rigctld` on `PATH`.
///
/// `want` is the operator's per-line control-line choice ([`ControlLines`]); it is narrowed
/// to what this rig's Hamlib will actually accept before anything is emitted.
pub fn spawn_rigctld(
    model: u32,
    addr: &str,
    baud: u32,
    tcp_port: u16,
    network: bool,
    ptt_line: Option<crate::rig::SerialLine>,
    want: ControlLines,
) -> std::io::Result<RigctldProc> {
    // Ask the daemon about the rig BEFORE launching it, so it cannot come up holding the
    // transmitter keyed (see [`SettableLines`]). Skipped where there is no control line at
    // all: a TCP transport, or a model that needs no port.
    let lines = if network || addr.is_empty() {
        ControlLines::default()
    } else {
        // Is RTS at this port a deliberate thing? A serial keying line declared here, or
        // the port enumerating as a recognised keying-interface cable (Digirig class).
        let deliberate = ptt_line.is_some() || crate::usbrig::port_is_keying_interface(addr);
        resolve_lines(settable_lines_for(model, ptt_line), want, deliberate)
    };
    let args = rigctld_args(model, addr, baud, tcp_port, network, ptt_line, lines);
    let mut cmd = Command::new(resolve_rigctld());
    cmd.args(&args);
    // Capture the daemon's own stderr so Hamlib's connection errors (port open failed, read
    // timeout, wrong model) land IN the CI-V diagnostic — for a Hamlib/Icom rig the byte-level
    // tap can't see the serial link, so without this a "rig never answered" fault is invisible.
    cmd.stderr(Stdio::piped());
    // On Windows, don't pop a console window for the daemon (Tempo is a GUI app).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn()?;
    // Record the exact launch (model / port / baud) so a diagnostic capture names the rig config
    // even when logging is armed AFTER the daemon started. A no-op line when logging is off.
    crate::civ::diag::note(&format!(
        "rigctld spawn: {} (network={network})",
        args.join(" ")
    ));
    // Drain the daemon's stderr on a detached thread → the ring buffer AND the diagnostic.
    // `note` is a cheap relaxed load when logging is off, so this idles for free; the thread
    // ends when the daemon exits.
    //
    // The ring is the half that reaches EVERY operator: the CI-V log needs arming, and its
    // toggle is rendered only for an Icom on the native path, so for a Yaesu owner `note` has
    // always been a no-op ([`RigctldProc::said`]).
    let said = Arc::new(Mutex::new(Said::default()));
    if let Some(stderr) = child.stderr.take() {
        let sink = Arc::clone(&said);
        std::thread::Builder::new()
            .name("rigctld-stderr".into())
            .spawn(move || {
                let mut reader = std::io::BufReader::new(stderr);
                let mut raw = Vec::new();
                loop {
                    raw.clear();
                    // `read_until`, not `lines()`: a line Hamlib fills with the RIG's own bytes
                    // is not UTF-8, and `lines().map_while(Result::ok)` ended the whole drain
                    // there. See [`said_line`] — that line is the wrong-baud diagnosis.
                    match reader.read_until(b'\n', &mut raw) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let Some(line) = said_line(&raw) else {
                        continue;
                    };
                    crate::civ::diag::note(&format!("rigctld: {line}"));
                    if let Ok(mut q) = sink.lock() {
                        q.push(line);
                    }
                }
            })
            .ok();
    }
    // On Windows, bind the daemon to a kill-on-close Job Object so it can't
    // outlive Tempo and keep the COM port locked.
    #[cfg(windows)]
    let job = assign_kill_on_close_job(&child);
    Ok(RigctldProc {
        child,
        said,
        #[cfg(windows)]
        job,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write `body` as an executable shell script in a fresh temp dir and return its path.
    #[cfg(unix)]
    fn stub_script(tag: &str, body: &str) -> std::path::PathBuf {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "nexus-runs-ok-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let p = dir.join("rigctld");
        std::fs::File::create(&p)
            .and_then(|mut f| f.write_all(body.as_bytes()))
            .expect("write stub");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        p
    }

    /// [`runs_ok`] must separate "ran and said no" from "could not run at all".
    ///
    /// The bug this guards is a REGRESSION THAT ALREADY HAPPENED once, on 2026-08-13: written as
    /// `status.success()`, the probe rejected every binary that exits non-zero for an unknown
    /// flag — which is most wrapper scripts and, concretely, the `rigctld` stand-in in
    /// `service`'s `an_ordinary_connect_failure_carries_what_the_daemon_said` (it answers `-vvv`
    /// and exits 9 otherwise). Nexus then silently overrode a deliberately-chosen binary with its
    /// own `HAMLIB_SEARCH_DIRS` guess, exactly what [`resolve_rigctld`]'s contract forbids. Only
    /// death by SIGNAL — how `dyld` reports unloadable libraries, via `abort()` before `main` —
    /// means unusable. Anyone tempted to "simplify" this back to `.success()` fails here.
    #[cfg(unix)]
    #[test]
    fn runs_ok_accepts_a_nonzero_exit_and_rejects_only_a_signal() {
        // Exits 9 for an unknown flag — the shape of the service.rs fixture and of real wrappers.
        let picky = stub_script("picky", "#!/bin/sh\n[ \"$1\" = \"-vvv\" ] || exit 9\n");
        assert!(
            runs_ok(picky.as_os_str()),
            "a stand-in that exits non-zero for --version still RUNS; rejecting it lets Nexus \
             override an operator's or a test's deliberate choice of binary"
        );

        // Killed by SIGABRT — how a binary whose dylibs cannot be loaded dies.
        let aborts = stub_script("aborts", "#!/bin/sh\nkill -ABRT $$\n");
        assert!(
            !runs_ok(aborts.as_os_str()),
            "signal death is the unloadable-library signature and must be rejected"
        );

        // Not executable at all.
        assert!(
            !runs_ok(std::ffi::OsStr::new("/nonexistent-nexus-test-dir/rigctld")),
            "a path that cannot be spawned is not usable"
        );

        for p in [picky, aborts] {
            if let Some(d) = p.parent() {
                let _ = std::fs::remove_dir_all(d);
            }
        }
    }

    /// Repro for the 2026-08-07 macOS report: `rigctld` installed via Homebrew, `which rigctld`
    /// works in every terminal, and Nexus still can't spawn it — because a Finder-launched app's
    /// `PATH` is launchd's fixed `/usr/bin:/bin:/usr/sbin:/sbin`, which never includes
    /// `/opt/homebrew/bin`. [`find_in_dirs`] is the fix: an explicit directory search that does
    /// not depend on the process's inherited `PATH` at all.
    #[cfg(unix)]
    #[test]
    fn find_in_dirs_locates_a_binary_path_would_miss() {
        let dir = std::env::temp_dir().join(format!(
            "nexus-rigctld-proc-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("rigctld"), b"#!/bin/sh\n").unwrap();
        let dir_str = dir.to_str().unwrap();

        // Not present in a directory that doesn't have it.
        assert_eq!(
            find_in_dirs(&["/nonexistent-nexus-test-dir"], "rigctld"),
            None
        );
        // Found when its directory is in the search list, first hit wins, and the directories
        // that don't have it are skipped over rather than short-circuiting the search.
        let found = find_in_dirs(&["/nonexistent-nexus-test-dir", dir_str], "rigctld").unwrap();
        assert_eq!(std::path::Path::new(&found), dir.join("rigctld"));
        // A name that isn't in ANY of the search dirs stays a miss.
        assert_eq!(find_in_dirs(&[dir_str], "rotctld"), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The priority guard: [`HAMLIB_SEARCH_DIRS`] must never outrank a `rigctld` the operator (or
    /// a test) deliberately put on `PATH` — that's what a fixture stand-in prepended to `PATH`
    /// depends on, in [`crate::service::tests::an_ordinary_connect_failure_carries_what_the_daemon_said`].
    /// Drives [`path_has`] directly (never touches the real `PATH`) so it cannot race that test,
    /// which mutates the process's actual `PATH` and is never restored by design (its own comment:
    /// "this is the only test in this binary that resolves a rigctld").
    #[cfg(unix)]
    #[test]
    fn path_has_is_true_only_when_a_search_dir_actually_has_it() {
        let dir = std::env::temp_dir().join(format!(
            "nexus-rigctld-proc-onpath-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("rigctld"), b"#!/bin/sh\n").unwrap();

        // A PATH that does not mention our fixture dir at all.
        assert!(!path_has(
            std::ffi::OsStr::new("/nonexistent-nexus-test-dir"),
            "rigctld"
        ));
        // Prepended, matching how the fixture-stand-in test builds its own PATH — found.
        let path_var = format!("{}:/nonexistent-nexus-test-dir", dir.display());
        assert!(path_has(std::ffi::OsStr::new(&path_var), "rigctld"));
        // A name that isn't in the fixture dir stays a miss even though the dir IS on PATH.
        assert!(!path_has(std::ffi::OsStr::new(&path_var), "rotctld"));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// ⭐ The ring's own contract, at the volume that broke it. `said_ring` is replayed against a
    /// REAL capture in `service::tests::the_daemons_diagnosis_survives_a_stream_that_never_stops`;
    /// this pins the four properties that make it work, one at a time, where a failure names
    /// which one went.
    #[test]
    fn the_ring_keeps_the_attempts_diagnosis_and_still_tracks_the_live_end() {
        // 1. The cause is printed once and then buried under 500 lines of narration.
        let mut raw = String::from("serial_open: serial port COM7 does not exist\n");
        for i in 0..500 {
            raw.push_str(&format!(
                "handle_socket: rig_open reopened retcode=0 #{i}\n"
            ));
        }
        let ring = said_ring(raw.as_bytes());
        assert!(
            ring.iter().any(|l| l.contains("COM7 does not exist")),
            "a bounded window drops it; the whole point is that this does not: {ring:?}"
        );
        // 2. …without becoming a log: the window is still SAID_KEPT lines of the live end.
        assert_eq!(ring.len(), SAID_KEPT + 1, "{ring:?}");
        assert!(ring.last().is_some_and(|l| l.ends_with("#499")), "{ring:?}");

        // 3. Within a rank the FIRST wins (Hamlib prints the cause, then its knock-ons), but a
        //    BETTER rank arriving later still takes the slot.
        let mut raw = String::from("handle_socket: i/o error\n");
        raw.push_str("read_block_generic(): Timed out 1.4 seconds after 0 chars\n");
        raw.push_str("read_block_generic(): Timed out 9.9 seconds after 0 chars\n");
        for i in 0..20 {
            raw.push_str(&format!("handle_socket: rig_close retcode=0 #{i}\n"));
        }
        let ring = said_ring(raw.as_bytes());
        assert!(
            ring.iter().any(|l| l.contains("Timed out 1.4")),
            "the cause outranks the bookkeeping that preceded it, and it is the FIRST of its \
             rank that is kept: {ring:?}"
        );
        assert!(
            !ring.iter().any(|l| l.contains("Timed out 9.9")),
            "one slot, not a second ring: {ring:?}"
        );

        // 4. A short-lived daemon reads EXACTLY as it did before there was a second half: the
        //    retained line is in the window already, so it is not repeated.
        let short = b"serial_open: serial port COM7 does not exist\nhandle_socket: i/o error\n";
        assert_eq!(
            said_ring(short),
            vec![
                "serial_open: serial port COM7 does not exist".to_string(),
                "handle_socket: i/o error".to_string(),
            ],
            "no duplicate, no reordering, for the case that always worked"
        );
    }

    /// ⭐ THE ROOT CAUSE OF "nothing noteworthy" (FT-847 field report, 2026-08).
    ///
    /// [`spawn_rigctld`] pipes the daemon's stderr and drains it on a thread, and has since the
    /// CI-V diagnostic was built — so the shape looked right. It drains **an empty stream**:
    /// rigctld's `-v` count is passed straight to `rig_set_debug`, and with no `-v` at all that
    /// is `RIG_DEBUG_NONE`, at which Hamlib prints nothing whatsoever. The operator's daemon was
    /// silent, not ignored.
    ///
    /// Measured against the bundled rigctld 4.7.1, one run per level, `-r COM99`:
    /// - no flag → nothing;
    /// - `-v` (`RIG_DEBUG_BUG`) → nothing;
    /// - `-vv` (`RIG_DEBUG_ERR`) → `serial_open: serial port COM99 does not exist`;
    /// - `-vvv` (`RIG_DEBUG_WARN`) → the same one line;
    /// - `-vvvv` (`RIG_DEBUG_VERBOSE`) → 38 lines for that one failed open.
    ///
    /// **Why `-vvv` and not the `-vv` this shipped at.** `-vv` covers the OPEN: the port that
    /// is not there, the port that is busy. It does not cover the fault the field report is
    /// most likely to be — *the port opened and the rig never answered* — in the words that
    /// name it. Measured on a mute serial rig (a pty that accepts and never replies, FT-847,
    /// three `get_freq` polls):
    /// - `-vv` → 1 line per poll: `ft847: read_block returned -5`;
    /// - `-vvv` → 2 lines per poll: that, plus
    ///   `read_block_generic(): Timed out 1.109 seconds after 0 chars, direct=1`.
    ///
    /// The second line is the one an operator can act on, and `-vv` does not have it.
    ///
    /// **And the cost is nil, which is the part that had to be measured rather than assumed.**
    /// A HEALTHY rig prints nothing at either level — 10 polls on a pty rig that answers, and
    /// 30 polls through a real `rigctld -m 1` over one persistent connection, gave byte-identical
    /// output at `-vv` and `-vvv` (0 lines and 2 connect-time lines respectively). WARN adds
    /// warnings; it does not narrate. `-vvvv` is where the per-transaction trace starts (85
    /// lines for those same 10 healthy polls), and that is the level this must never reach.
    ///
    /// The doubled fault-case rate cannot displace the useful line, and after the round-four fix
    /// that no longer rests on the burst being small: the two mute-rig lines alternate inside the
    /// [`SAID_KEPT`] window, and either way [`Said::best`] holds the first `Timed out` of the
    /// attempt for as long as the daemon lives. `service::with_daemon_error` then reports the
    /// newest two DISTINCT lines of what survived.
    #[test]
    fn the_daemon_is_launched_at_the_verbosity_that_reports_a_rig_that_never_answered() {
        for args in [
            rigctld_args(
                1001,
                "COM5",
                57600,
                4532,
                false,
                None,
                ControlLines::default(),
            ),
            rigctld_args(1, "", 38400, 4532, false, None, ControlLines::default()),
            rigctld_args(
                23005,
                "192.168.1.50:4992",
                38400,
                4532,
                true,
                None,
                ControlLines::default(),
            ),
        ] {
            assert!(
                args.iter().any(|a| a == "-vvv"),
                "at `-vv` a mute rig says only `read_block returned -5`, and below that Hamlib \
                 is at RIG_DEBUG_NONE and the stderr drain has nothing to drain: {args:?}"
            );
            assert!(
                !args.iter().any(|a| a == "-vvvv"),
                "VERBOSE narrates every transaction (85 lines per 10 HEALTHY polls) and would \
                 push the fault out of an 8-line ring: {args:?}"
            );
        }
    }

    #[test]
    fn rotctld_args_mirror_the_rig_shape() {
        assert_eq!(
            rotctld_args(601, "COM7", 9600, 4533),
            vec![
                "-m",
                "601",
                "-r",
                "COM7",
                "-s",
                "9600",
                "-T",
                "127.0.0.1",
                "-t",
                "4533"
            ]
        );
        // No port (Dummy rotator for testing) → no -r/-s.
        assert_eq!(
            rotctld_args(1, "", 9600, 4533),
            vec!["-m", "1", "-T", "127.0.0.1", "-t", "4533"]
        );
    }

    #[test]
    fn args_with_serial_port() {
        let args = rigctld_args(
            3073,
            "COM5",
            38400,
            4532,
            false,
            None,
            ControlLines::default(),
        );
        assert_eq!(
            args,
            vec![
                "-vvv",
                "-m",
                "3073",
                "-r",
                "COM5",
                "-s",
                "38400",
                "-T",
                "127.0.0.1",
                "-t",
                "4532"
            ]
        );
    }

    #[test]
    fn args_for_network_rig_omit_baud() {
        // A FlexRadio over SmartSDR (or any TCP rig): host:port on -r, no baud.
        let args = rigctld_args(
            23005,
            "192.168.1.50:4992",
            38400,
            4532,
            true,
            None,
            ControlLines::default(),
        );
        assert_eq!(
            args,
            vec![
                "-vvv",
                "-m",
                "23005",
                "-r",
                "192.168.1.50:4992",
                "-T",
                "127.0.0.1",
                "-t",
                "4532"
            ]
        );
    }

    #[test]
    fn args_for_unix_serial_device() {
        let args = rigctld_args(
            1042,
            "/dev/ttyUSB0",
            19200,
            4533,
            false,
            None,
            ControlLines::default(),
        );
        assert_eq!(
            args,
            vec![
                "-vvv",
                "-m",
                "1042",
                "-r",
                "/dev/ttyUSB0",
                "-s",
                "19200",
                "-T",
                "127.0.0.1",
                "-t",
                "4533"
            ]
        );
    }

    #[test]
    fn args_without_serial_port_omit_port_and_baud() {
        // Dummy / NET rigs need no serial device.
        let args = rigctld_args(1, "", 38400, 4532, false, None, ControlLines::default());
        assert_eq!(
            args,
            vec!["-vvv", "-m", "1", "-T", "127.0.0.1", "-t", "4532"]
        );
    }

    /// The single-cable interface (Digirig Mobile): ONE port carries CAT and the RTS keying
    /// line, so rigctld is told to do both. Hamlib shares the fd for the second use rather than
    /// opening the port twice, which is what makes this legal at all.
    #[test]
    fn shared_port_keying_emits_ptt_type_and_file() {
        let args = rigctld_args(
            3073,
            "COM5",
            38400,
            4532,
            false,
            Some(crate::rig::SerialLine::Rts),
            ControlLines::default(),
        );
        assert_eq!(
            args,
            vec![
                "-vvv",
                "-m",
                "3073",
                "-r",
                "COM5",
                "-s",
                "38400",
                "-P",
                "RTS",
                "-p",
                "COM5",
                "-T",
                "127.0.0.1",
                "-t",
                "4532"
            ],
            "-P/-p must name the SAME device as -r; a mismatch opens a second port"
        );

        let dtr = rigctld_args(
            1042,
            "/dev/ttyUSB0",
            19200,
            4533,
            false,
            Some(crate::rig::SerialLine::Dtr),
            ControlLines::default(),
        );
        assert!(dtr.windows(2).any(|w| w == ["-P", "DTR"]));
        assert!(dtr.windows(2).any(|w| w == ["-p", "/dev/ttyUSB0"]));
    }

    /// ⚠️ THE SILENT-KILLER PIN. rigctld does NOT reject an unknown --ptt-type: it prints
    /// "Unrecognised PTT type, using NONE" and starts with keying disabled, so the rig tunes
    /// perfectly and never transmits. These two tokens are the ONLY serial ones Hamlib parses
    /// (rigctld(1), bundled 4.7.x: RIG, DTR, RTS, PARALLEL, NONE). If anyone "tidies" the
    /// spelling — lowercase, "SERIAL_RTS", the numeric enum — this test is the thing that
    /// stops it reaching a radio.
    #[test]
    fn ptt_type_tokens_are_exactly_what_hamlib_parses() {
        assert_eq!(ptt_type_token(crate::rig::SerialLine::Rts), "RTS");
        assert_eq!(ptt_type_token(crate::rig::SerialLine::Dtr), "DTR");
    }

    /// A TCP rig has no RTS line, and a rig with no serial device has nothing to key. Emitting
    /// -P/-p in either case would hand Hamlib a PTT path that cannot exist.
    #[test]
    fn shared_port_keying_is_dropped_when_there_is_no_serial_line_to_key() {
        let net = rigctld_args(
            23005,
            "192.168.1.50:4992",
            38400,
            4532,
            true,
            Some(crate::rig::SerialLine::Rts),
            ControlLines::default(),
        );
        assert!(
            !net.iter().any(|a| a == "-P" || a == "-p"),
            "a network rig has no serial keying line: {net:?}"
        );

        let no_dev = rigctld_args(
            1,
            "",
            38400,
            4532,
            false,
            Some(crate::rig::SerialLine::Rts),
            ControlLines::default(),
        );
        assert_eq!(
            no_dev,
            vec!["-vvv", "-m", "1", "-T", "127.0.0.1", "-t", "4532"]
        );
    }

    /// Every arg-shape test below reads the flag pairs rather than a whole vector, so adding an
    /// unrelated argument later cannot make a TX-safety test fail for the wrong reason.
    fn holds(args: &[String], line: &str, value: &str) -> bool {
        args.windows(2)
            .any(|w| w[0] == "-C" && w[1] == format!("{line}_state={value}"))
    }
    fn says_nothing_about(args: &[String], line: &str) -> bool {
        !args
            .windows(2)
            .any(|w| w[0] == "-C" && w[1].starts_with(&format!("{line}_state=")))
    }

    /// A `rigctld --show-conf` dump in the EXACT shape the bundled 4.7.1 prints, with the two
    /// values that decide everything substituted in. Every literal here — the tooltips, the
    /// `Default:`/`Value:`/`Combo:` layout, the tab indentation and both combo vocabularies — is
    /// verbatim from `rigctld.exe -m <model> -L`; [`SHOW_CONF_FT757GXII`] pins that.
    /// `handshake: None` builds a NON-serial backend: Hamlib offers the serial parameters only
    /// for `RIG_PORT_SERIAL`, so a network/device backend simply has no such lines.
    fn show_conf(ptt_value: &str, handshake: Option<&str>) -> String {
        let mut s = format!(
            "ptt_type: \"Push-To-Talk interface type override\"\n\
             \tDefault: RIG, Value: {ptt_value}\n\
             \tCombo: RIG, RIGMICDATA, DTR, RTS, Parallel, CM108, GPIO, GPION, None\n\
             ptt_pathname: \"Path to the device of the Push-To-Talk\"\n\
             \tDefault: /dev/rig, Value: \n\
             \tString.\n"
        );
        if let Some(h) = handshake {
            s.push_str(&format!(
                "serial_handshake: \"Serial port handshake\"\n\
                 \tDefault: None, Value: {h}\n\
                 \tCombo: None, XONXOFF, Hardware\n\
                 rts_state: \"Serial port set state of RTS signal for external powering\"\n\
                 \tDefault: Unset, Value: Unset\n\
                 \tCombo: Unset, ON, OFF\n\
                 dtr_state: \"Serial port set state of DTR signal for external powering\"\n\
                 \tDefault: Unset, Value: Unset\n\
                 \tCombo: Unset, ON, OFF\n"
            ));
        }
        s
    }

    /// ⚠️ THE REGRESSION THIS ROUND FIXES — verbatim `rigctld.exe 4.7.1 -m 1007 -L`, trimmed to
    /// the four parameters the parser reads. The Yaesu FT-757GXII declares
    /// `.ptt_type = RIG_PTT_SERIAL_DTR` (`rigs/yaesu/ft757gx.c:237`), so with **no `-P` passed —
    /// Nexus's `vox` default — Hamlib is already keying by DTR** and `rig_open` refuses
    /// `dtr_state`, serving a rig it never opened. The `--dump-caps` oracle cannot see this: it
    /// prints `PTT type: None` for all 309 models.
    const SHOW_CONF_FT757GXII: &str = "ptt_type: \"Push-To-Talk interface type override\"\n\
        \tDefault: RIG, Value: DTR\n\
        \tCombo: RIG, RIGMICDATA, DTR, RTS, Parallel, CM108, GPIO, GPION, None\n\
        serial_handshake: \"Serial port handshake\"\n\
        \tDefault: None, Value: None\n\
        \tCombo: None, XONXOFF, Hardware\n\
        rts_state: \"Serial port set state of RTS signal for external powering\"\n\
        \tDefault: Unset, Value: Unset\n\
        \tCombo: Unset, ON, OFF\n\
        dtr_state: \"Serial port set state of DTR signal for external powering\"\n\
        \tDefault: Unset, Value: Unset\n\
        \tCombo: Unset, ON, OFF\n";

    /// The same, for the Yaesu FT-980 (`rigs/yaesu/ft980.h:153`,
    /// `.ptt_type = RIG_PTT_SERIAL_RTS`) — the RTS half of the same defect.
    const SHOW_CONF_FT980: &str = "ptt_type: \"Push-To-Talk interface type override\"\n\
        \tDefault: RIG, Value: RTS\n\
        \tCombo: RIG, RIGMICDATA, DTR, RTS, Parallel, CM108, GPIO, GPION, None\n\
        serial_handshake: \"Serial port handshake\"\n\
        \tDefault: None, Value: None\n\
        \tCombo: None, XONXOFF, Hardware\n\
        rts_state: \"Serial port set state of RTS signal for external powering\"\n\
        \tDefault: Unset, Value: Unset\n\
        \tCombo: Unset, ON, OFF\n\
        dtr_state: \"Serial port set state of DTR signal for external powering\"\n\
        \tDefault: Unset, Value: Unset\n\
        \tCombo: Unset, ON, OFF\n";

    /// ⚠️ THE DEFECT, 1.0.1 and earlier. Nexus's PTT Method default is `vox`, so no `-P` is
    /// passed and Hamlib's `rig_open` takes the `RIG_PTT_NONE` arm, which `break`s without
    /// touching a pin — while the port's driver has just raised both. On an interface that keys
    /// on RTS that is the radio put into transmit BY CONNECTING, and held there for the session.
    /// Both lines are held low, because both key transmitters in the field: RTS is the reported
    /// one, DTR is the same defect on the equally common DTR-keyed interface (and on a DTR-keyed
    /// CW interface it is a continuous key-down).
    #[test]
    fn a_serial_rig_on_the_vox_default_holds_both_lines_low() {
        let args = rigctld_args(
            3073,
            "/dev/ttyUSB0",
            38400,
            4532,
            false,
            None,
            ControlLines::hold_low(),
        );
        assert!(
            holds(&args, "rts", "OFF"),
            "RTS left high on connect: {args:?}"
        );
        assert!(
            holds(&args, "dtr", "OFF"),
            "DTR left high on connect: {args:?}"
        );
    }

    /// ⚠️ THE INVARIANT THAT OUTRANKS THE FIX: an operator who keys by RTS or DTR must still
    /// key. Hamlib REFUSES to open a rig asked to set the state of the line it is keying with —
    /// `rig.c` `rig_open` returns `-RIG_ECONF`, observed from the bundled rigctld 4.7.1 as
    /// `rig_open: cannot set RTS with PTT by RTS "COM99"` and
    /// `rig_open: cannot set DTR with PTT by DTR "COM99"` — and it does NOT exit on that, it
    /// serves a rig it never opened. So this is not a preference: setting the keying line is
    /// silently dead CAT *and* dead keying at once. The OTHER line is still held, which the same
    /// binary confirms is accepted (`-m 1007 -P RTS -p COM99 -C dtr_state=OFF` reaches
    /// `serial_open`).
    #[test]
    fn the_keying_line_is_never_given_a_state() {
        for want in [ControlLines::hold_low(), high_both()] {
            let rts = rigctld_args(
                3073,
                "COM5",
                38400,
                4532,
                false,
                Some(crate::rig::SerialLine::Rts),
                want,
            );
            assert!(
                says_nothing_about(&rts, "rts"),
                "would ask Hamlib to set the RTS keying line — it refuses to open: {rts:?}"
            );
            assert!(
                !says_nothing_about(&rts, "dtr"),
                "DTR is not the keying line: {rts:?}"
            );

            let dtr = rigctld_args(
                3073,
                "COM5",
                38400,
                4532,
                false,
                Some(crate::rig::SerialLine::Dtr),
                want,
            );
            assert!(
                says_nothing_about(&dtr, "dtr"),
                "would ask Hamlib to set the DTR keying line — it refuses to open: {dtr:?}"
            );
            assert!(
                !says_nothing_about(&dtr, "rts"),
                "RTS is not the keying line: {dtr:?}"
            );
        }
    }

    fn high_both() -> ControlLines {
        ControlLines {
            rts: LineState::High,
            dtr: LineState::High,
            ..Default::default()
        }
    }

    /// A backend whose serial handshake is hardware owns RTS as a flow-control output; asking to
    /// set it is refused whatever the PTT type (`rig_open: cannot set RTS with hardware
    /// handshake "COM99"`, observed on model 1042). DTR is not a flow-control line and is
    /// unaffected — also observed.
    #[test]
    /// ⚠️ REVERSED 2026-08-08 (#44). This used to assert RTS was left alone on a
    /// hardware-handshake backend, and leaving it alone is what keyed vk6mo's TS-2000 the
    /// moment Nexus connected: RTS asserted as flow control, and a Digirig keys on RTS. We now
    /// drop the declared handshake so the line can be held low. The rest of the old assertion
    /// stands — DTR was never the problem.
    fn a_hardware_handshake_backend_gives_up_its_handshake_so_rts_can_be_held() {
        // deliberate=true: the port enumerates as a Digirig-class keying cable — the #44
        // wiring, where RTS IS the PTT and flow control is fiction.
        let lines = resolve_lines(
            parse_settable_lines(&show_conf("RIG", Some("Hardware"))),
            ControlLines::hold_low(),
            true,
        );
        let args = rigctld_args(1042, "COM5", 38400, 4532, false, None, lines);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-C" && w[1] == "serial_handshake=None"),
            "the handshake has to go first or Hamlib refuses rts_state: {args:?}"
        );
        assert!(
            holds(&args, "rts", "OFF"),
            "the whole point: RTS held low so connecting cannot key: {args:?}"
        );
        assert!(holds(&args, "dtr", "OFF"), "DTR is still free: {args:?}");
    }

    /// ⛔ THE KD9TAW BENCH REGRESSION (2026-08-09), pinned. An FTDX10 / FT-991 on its OWN
    /// USB port — no interface cable, no serial keying line — with nothing but the shipped
    /// hold-low default: the rig's declared hardware handshake must SURVIVE. These rigs at
    /// their factory default (CAT RTS = ENABLE) gate CAT replies on the PC asserting RTS;
    /// the 2026-08-08 blanket override (handshake=None + rts_state=OFF) made both bench
    /// Yaesus completely CAT-dead while the native-CI-V Icoms kept working. Written to fail
    /// against that build.
    #[test]
    fn a_bare_rig_on_the_default_hold_keeps_its_hardware_handshake() {
        let lines = resolve_lines(
            parse_settable_lines(&show_conf("RIG", Some("Hardware"))),
            ControlLines::hold_low(),
            false, // not a keying cable, no serial keying line — the default, undelibrate case
        );
        assert!(
            !lines.handshake_none,
            "the blanket safety default must never cost a rig its handshake"
        );
        let args = rigctld_args(1042, "COM5", 38400, 4532, false, None, lines);
        assert!(
            !args.iter().any(|a| a.starts_with("serial_handshake")),
            "an FTDX10 with CAT RTS = ENABLE goes CAT-dead the moment this flag appears: {args:?}"
        );
        assert!(
            says_nothing_about(&args, "rts"),
            "and RTS stays untouched — flow control keeps working: {args:?}"
        );
        // DTR is not flow control: the safety hold still applies there.
        assert!(holds(&args, "dtr", "OFF"), "{args:?}");
    }

    /// The `rts_deliberate` answer itself: a Digirig enumerates as a keying interface, a
    /// rig's own USB port does not, and an unknown port claims nothing.
    #[test]
    fn keying_interface_recognition_separates_cables_from_rigs() {
        use crate::ports::UsbPort;
        let ports = vec![
            UsbPort {
                port_name: "COM7".into(),
                vid: 0x10C4,
                pid: 0xEA60,
                product: "Digirig Mobile".into(),
                manufacturer: "Silicon Labs".into(),
            },
            UsbPort {
                port_name: "COM5".into(),
                vid: 0x10C4,
                pid: 0xEA60,
                product: "FTDX10".into(),
                manufacturer: "Yaesu".into(),
            },
        ];
        assert!(crate::usbrig::keying_interface_in(&ports, "COM7"));
        assert!(
            crate::usbrig::keying_interface_in(&ports, "com7"),
            "case-insensitive"
        );
        assert!(
            !crate::usbrig::keying_interface_in(&ports, "COM5"),
            "same VID/PID as the Digirig — the PRODUCT string is what separates them"
        );
        assert!(
            !crate::usbrig::keying_interface_in(&ports, "COM9"),
            "unknown port claims nothing"
        );
    }

    /// The override is not a blanket one. A backend that already leaves RTS free gets no
    /// handshake flag — without this, the test above passes just as well for a hook that
    /// emits it unconditionally, and every operator loses their flow control for nothing.
    #[test]
    fn a_handshake_free_backend_is_left_alone() {
        let lines = resolve_lines(
            parse_settable_lines(&show_conf("RIG", Some("None"))),
            ControlLines::hold_low(),
            false,
        );
        let args = rigctld_args(1042, "COM5", 38400, 4532, false, None, lines);
        assert!(
            !args.iter().any(|a| a.starts_with("serial_handshake")),
            "nothing to free — the line was already ours: {args:?}"
        );
        assert!(
            holds(&args, "rts", "OFF"),
            "and RTS is still held: {args:?}"
        );
    }

    /// Keying with RTS is refusal #1, and no handshake override can lift it — Hamlib refuses
    /// `rts_state` on the keying line whatever the flow control is. Trading the operator's
    /// handshake away here would buy nothing at all.
    #[test]
    fn keying_with_rts_is_not_something_the_override_can_rescue() {
        let settable = parse_settable_lines(&show_conf("RTS", Some("Hardware")));
        assert!(
            !settable.rts_taken_by_handshake,
            "the handshake is not why this line is unavailable — the keying is"
        );
        let lines = resolve_lines(settable, ControlLines::hold_low(), true);
        let args = rigctld_args(
            1042,
            "COM5",
            38400,
            4532,
            false,
            Some(crate::rig::SerialLine::Rts),
            lines,
        );
        assert!(
            !args.iter().any(|a| a.starts_with("serial_handshake")),
            "no handshake was given up for a line we still cannot set: {args:?}"
        );
        assert!(says_nothing_about(&args, "rts"), "{args:?}");
    }

    /// An operator who asked for `Untouched` keeps their flow control. The override exists to
    /// deliver a hold that was asked for, never as an opinion about handshakes.
    #[test]
    fn an_untouched_wish_gives_up_no_handshake() {
        let lines = resolve_lines(
            parse_settable_lines(&show_conf("RIG", Some("Hardware"))),
            ControlLines {
                rts: LineState::Untouched,
                dtr: LineState::Low,
                handshake_none: false,
            },
            true,
        );
        assert!(!lines.handshake_none, "nothing was asked for on RTS");
        let args = rigctld_args(1042, "COM5", 38400, 4532, false, None, lines);
        assert!(
            !args.iter().any(|a| a.starts_with("serial_handshake")),
            "{args:?}"
        );
    }

    /// A handshake token we cannot classify claims NOTHING — it must not be read as "not
    /// Hardware, therefore fine" NOR as "not free, therefore override". This is the
    /// stale-vocabulary guard the module is built around, applied to the new field.
    #[test]
    fn an_unclassifiable_handshake_buys_no_override() {
        let odd = show_conf("RIG", Some("Hardware")).replace("Hardware", "RtsCtsPlus");
        let settable = parse_settable_lines(&odd);
        assert!(!settable.rts, "unreadable means claim nothing");
        assert!(
            !settable.rts_taken_by_handshake,
            "and claiming nothing must not turn into overriding on a guess"
        );
    }

    /// A TCP transport has no control lines at all, and Hamlib does not even offer the config
    /// parameters for a non-serial backend. No line flags may appear — on either gate.
    #[test]
    fn a_network_rig_gets_no_line_flags() {
        let net = rigctld_args(
            23005,
            "192.168.1.50:4992",
            38400,
            4532,
            true,
            None,
            ControlLines::hold_low(),
        );
        assert!(
            !net.iter().any(|a| a == "-C"),
            "a TCP rig has no control lines: {net:?}"
        );

        let no_dev = rigctld_args(1, "", 38400, 4532, false, None, ControlLines::hold_low());
        assert_eq!(
            no_dev,
            vec!["-vvv", "-m", "1", "-T", "127.0.0.1", "-t", "4532"]
        );
    }

    /// ⚠️ THE P1 REGRESSION, by name. Both dumps are real `rigctld 4.7.1 -m <model> -L` output
    /// with NO `-P` — i.e. exactly what Nexus asks on its `vox` default. Round two of this fix
    /// exists because the first read `--dump-caps`, which prints `PTT type: None` for every one
    /// of the 309 models, and so suppressed the flag only when NEXUS had named the keying line.
    /// These two backends name it themselves.
    #[test]
    fn a_backend_that_keys_on_its_own_serial_line_is_never_given_that_line() {
        // Yaesu FT-757GXII (model 1007) — keys by DTR.
        assert_eq!(
            parse_settable_lines(SHOW_CONF_FT757GXII),
            SettableLines {
                rts: true,
                dtr: false,
                ..Default::default()
            },
            "FT-757GXII keys on DTR by default: `rig_open: cannot set DTR with PTT by DTR`"
        );
        // Yaesu FT-980 (model 1031) — keys by RTS.
        assert_eq!(
            parse_settable_lines(SHOW_CONF_FT980),
            SettableLines {
                rts: false,
                dtr: true,
                ..Default::default()
            },
            "FT-980 keys on RTS by default: `rig_open: cannot set RTS with PTT by RTS`"
        );
        // …and it must survive all the way to the command line, on the vox default.
        let args = rigctld_args(
            1007,
            "COM5",
            4800,
            4532,
            false,
            None,
            resolve_lines(
                parse_settable_lines(SHOW_CONF_FT757GXII),
                ControlLines::hold_low(),
                false,
            ),
        );
        assert!(
            says_nothing_about(&args, "dtr"),
            "dead CAT on an FT-757GXII: {args:?}"
        );
        assert!(holds(&args, "rts", "OFF"), "RTS is still free: {args:?}");
    }

    /// ENUMERATION, not sampling: every token Hamlib's own `ptt_type` vocabulary contains gets a
    /// decision, and the decision is checked for BOTH lines. The vocabulary is read off the
    /// `Combo:` line of a real dump, so this test fails the moment Hamlib grows a token — which
    /// is the only way a serial-line PTT type could slip past unclassified.
    #[test]
    fn every_ptt_type_token_hamlib_offers_gets_the_right_decision() {
        let (_, vocabulary) = conf_param(SHOW_CONF_FT757GXII, "ptt_type")
            .expect("the fixture is a real --show-conf dump");
        let mut classified: Vec<&str> = PTT_TOKENS_NOT_A_LINE.to_vec();
        classified.push("RTS");
        classified.push("DTR");
        classified.sort_unstable();
        let mut offered = vocabulary.clone();
        offered.sort_unstable();
        assert_eq!(
            offered, classified,
            "Hamlib's ptt_type vocabulary has changed — every token must be classified before \
             any line may be set again"
        );

        for token in vocabulary {
            let got = parse_settable_lines(&show_conf(token, Some("None")));
            assert_eq!(
                got,
                SettableLines {
                    rts: token != "RTS",
                    dtr: token != "DTR",
                    ..Default::default()
                },
                "wrong decision for ptt_type={token}"
            );
        }
    }

    /// The vocabulary check is the guard against a Hamlib bump, so it has to FAIL CLOSED: a
    /// token we do not classify — whether it is the current value or merely offered alongside it
    /// — must leave both lines alone. Guessing "probably not a serial line" is how this reopens.
    #[test]
    fn an_unclassified_token_leaves_both_lines_alone() {
        // A value we do not know.
        let unknown_value = show_conf("SERIAL_RTS", Some("None"));
        assert_eq!(
            parse_settable_lines(&unknown_value),
            SettableLines::default()
        );
        // A vocabulary that has GROWN — the value is one we know, but a sibling token is not,
        // so we can no longer prove we understand what "RIG" excludes.
        let grown = show_conf("RIG", Some("None")).replace(
            "Combo: RIG, RIGMICDATA, DTR",
            "Combo: RIG, RIGMICDATA, DTR, RS232RTS",
        );
        assert_eq!(parse_settable_lines(&grown), SettableLines::default());
        // A handshake vocabulary that has grown: RTS can no longer be proved free. DTR does not
        // depend on the handshake and is unaffected.
        let grown_handshake = show_conf("RIG", Some("None")).replace(
            "Combo: None, XONXOFF, Hardware",
            "Combo: None, XONXOFF, DTRDSR",
        );
        assert_eq!(
            parse_settable_lines(&grown_handshake),
            SettableLines {
                rts: false,
                dtr: true,
                ..Default::default()
            }
        );
        // A vocabulary that is no longer printed at all. Without it we cannot tell a token we
        // classify from one that has merely not been added yet, so there is nothing to prove
        // safety with — and this is the only guard that notices a Hamlib change unprompted.
        let no_combo = show_conf("RIG", Some("None"))
            .replace(
                "\tCombo: RIG, RIGMICDATA, DTR, RTS, Parallel, CM108, GPIO, GPION, None\n",
                "",
            )
            .replace("\tCombo: None, XONXOFF, Hardware\n", "");
        assert_eq!(parse_settable_lines(&no_combo), SettableLines::default());
        // Nothing readable at all.
        assert_eq!(parse_settable_lines(""), SettableLines::default());
        assert_eq!(
            parse_settable_lines("ptt_type: \"x\"\n\tDefault: RIG, Value: RIG\n"),
            SettableLines::default(),
            "no rts_state/dtr_state parameter means this is not a serial backend"
        );
    }

    /// `serial_handshake` is read as an ALLOW-LIST, from the same field `rig_open` tests
    /// (`rp->parm.serial.handshake`) rather than the `ctrl=` string `dumpcaps` derives from it.
    /// Observed values: 3073/1007/1031 → `None`; 1042 (FTDX-10) → `Hardware`.
    #[test]
    fn the_handshake_is_read_as_an_allow_list() {
        assert!(parse_settable_lines(&show_conf("RIG", Some("None"))).rts);
        assert!(parse_settable_lines(&show_conf("RIG", Some("XONXOFF"))).rts);
        assert!(!parse_settable_lines(&show_conf("RIG", Some("Hardware"))).rts);
        // DTR is not a flow-control line: the handshake never takes it away.
        for h in ["None", "XONXOFF", "Hardware"] {
            assert!(
                parse_settable_lines(&show_conf("RIG", Some(h))).dtr,
                "handshake {h} must not affect DTR"
            );
        }
    }

    /// A backend with no serial port (NET rigctl, Dummy — both observed) offers no serial conf
    /// parameters at all, so there is nothing to say and no port-type string to misread.
    #[test]
    fn a_non_serial_backend_offers_no_line_parameters() {
        assert_eq!(
            parse_settable_lines(&show_conf("RIGMICDATA", None)),
            SettableLines::default()
        );
        assert_eq!(
            parse_settable_lines(&show_conf("RIG", None)),
            SettableLines::default()
        );
    }

    /// The operator override (P2). Nexus's default is Low; `Untouched` is the recovery hatch
    /// that restores exactly 1.0.1, and `High` is for an accessory drawing its supply from the
    /// line. What the operator asks for is only ever NARROWED by what Hamlib accepts — a wish
    /// can never talk us into a flag the daemon would refuse.
    #[test]
    fn the_operator_override_is_narrowed_by_what_hamlib_accepts() {
        let free = SettableLines {
            rts: true,
            dtr: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_lines(free, ControlLines::hold_low(), false),
            ControlLines::hold_low()
        );
        assert_eq!(resolve_lines(free, high_both(), false), high_both());
        // Untouched is 1.0.1: nothing is emitted even where everything is settable.
        let untouched = rigctld_args(
            3073,
            "COM5",
            38400,
            4532,
            false,
            None,
            resolve_lines(free, ControlLines::default(), false),
        );
        assert!(!untouched.iter().any(|a| a == "-C"), "{untouched:?}");
        // High reaches the wire as Hamlib's own `ON`.
        let high = rigctld_args(
            3073,
            "COM5",
            38400,
            4532,
            false,
            None,
            resolve_lines(free, high_both(), false),
        );
        assert!(
            holds(&high, "rts", "ON") && holds(&high, "dtr", "ON"),
            "{high:?}"
        );
        // An unsettable line is Untouched whatever the operator asked for.
        let taken = SettableLines {
            rts: false,
            dtr: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_lines(taken, high_both(), false),
            ControlLines {
                rts: LineState::Untouched,
                dtr: LineState::High,
                ..Default::default()
            }
        );
    }

    /// The stored setting is a string on `Settings`, so it can arrive empty (an older settings
    /// file, a UI that has not been taught the control yet) or misspelled. Everything that is
    /// not positively `untouched` or `high` means the SAFE default — never "leave it high".
    #[test]
    fn an_unrecognised_stored_setting_means_the_safe_default() {
        assert_eq!(LineState::from_setting("untouched"), LineState::Untouched);
        assert_eq!(LineState::from_setting("Untouched"), LineState::Untouched);
        assert_eq!(LineState::from_setting("high"), LineState::High);
        assert_eq!(LineState::from_setting(" HIGH "), LineState::High);
        assert_eq!(LineState::from_setting("low"), LineState::Low);
        assert_eq!(LineState::from_setting(""), LineState::Low);
        assert_eq!(LineState::from_setting("leave alone"), LineState::Low);
        // The type's own default is the no-op, so every `?`/fallback path in this module lands
        // on 1.0.1 rather than on a flag.
        assert_eq!(LineState::default(), LineState::Untouched);
        assert_eq!(
            ControlLines::default(),
            ControlLines {
                rts: LineState::Untouched,
                dtr: LineState::Untouched,
                ..Default::default()
            }
        );
    }

    /// A `--show-conf` value may legitimately contain the separator we split on (a pathname
    /// Default does), so the split must take the LAST occurrence.
    #[test]
    fn a_value_is_read_from_the_last_separator_not_the_first() {
        let dump = "rig_pathname: \"Path name to the device file of the rig\"\n\
                    \tDefault: /dev/rig, Value: \\\\.\\COM1\n\tString.\n";
        assert_eq!(conf_param(dump, "rig_pathname").unwrap().0, "\\\\.\\COM1");
    }
    /// THE GUARD THAT MAKES THE SWEEP SAFE, and the one thing here that can be proven on a box
    /// with no Hamlib on it. A process that never exits must be killed, not waited on: that is
    /// exactly what wedged every CI run on this repo for six hours at a time, with no output
    /// naming the test responsible.
    #[test]
    fn run_bounded_kills_a_process_that_would_never_finish() {
        let t0 = std::time::Instant::now();
        let out = run_bounded(std::ffi::OsStr::new("sleep"), &["300"], 1);
        assert!(
            out.is_none(),
            "a process that outlives the deadline yields None"
        );
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(20),
            "it must give up at the deadline, not wait the process out: {:?}",
            t0.elapsed()
        );
    }

    /// ...and the other direction, so the guard is not passing by simply never working: a command
    /// that DOES finish comes back with its output intact.
    #[test]
    fn run_bounded_returns_the_output_of_a_command_that_finishes() {
        let out = run_bounded(std::ffi::OsStr::new("echo"), &["hello"], 10)
            .expect("a command that exits must produce output");
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
    }

    /// A binary that is not there is a skip, not a panic — the sweep is opportunistic.
    #[test]
    fn run_bounded_is_none_when_the_binary_does_not_exist() {
        assert!(run_bounded(std::ffi::OsStr::new("nexus-no-such-binary"), &["-l"], 5).is_none());
    }

    /// The daemon/client swap the sweep depends on.
    #[test]
    fn rigctl_is_found_beside_rigctld() {
        use std::ffi::OsString;
        assert_eq!(
            rigctl_beside(OsString::from("/usr/bin/rigctld")),
            OsString::from("/usr/bin/rigctl")
        );
        // The .exe arm, as a bare name: a Windows-style path with backslashes is a single
        // file_name on Unix (only `/` separates there), so spelling one here would test the host's
        // path rules rather than this function.
        assert_eq!(
            rigctl_beside(OsString::from("rigctld.exe")),
            OsString::from("rigctl.exe")
        );
        assert_eq!(
            rigctl_beside(OsString::from("/opt/hamlib/bin/rigctld.exe")),
            OsString::from("/opt/hamlib/bin/rigctl.exe")
        );
        // A bare name (nothing resolved) is left alone rather than rewritten into a guess.
        assert_eq!(
            rigctl_beside(OsString::from("rigctld")),
            OsString::from("rigctl")
        );
        assert_eq!(
            rigctl_beside(OsString::from("something-else")),
            OsString::from("something-else")
        );
    }

    /// `rigctl` sitting beside a resolved `rigctld` — same Hamlib package, same directory, so
    /// swapping the final component is enough. Done here rather than calling `resolve_rigctl`
    /// because that is `#[cfg(feature = "serial")]` and this test runs without it.
    fn rigctl_beside(rigctld: std::ffi::OsString) -> std::ffi::OsString {
        let p = std::path::PathBuf::from(&rigctld);
        match p.file_name().and_then(|f| f.to_str()) {
            Some("rigctld") => p.with_file_name("rigctl").into_os_string(),
            Some("rigctld.exe") => p.with_file_name("rigctl.exe").into_os_string(),
            _ => rigctld,
        }
    }

    /// Run a command and GIVE UP after `secs`, killing it. `None` for "could not run, or would not
    /// finish" — both of which mean the same thing to the sweep: skip this model.
    ///
    /// This exists because `Command::output()` has no timeout, and a process that never exits
    /// therefore hangs the whole test binary with no output to say which test it was. That is not
    /// hypothetical: it wedged every CI run on this repo for six hours at a time.
    fn run_bounded(
        bin: &std::ffi::OsStr,
        args: &[&str],
        secs: u64,
    ) -> Option<std::process::Output> {
        use std::process::Stdio;
        let mut child = Command::new(bin)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return child.wait_with_output().ok(),
                Ok(None) => {}
                Err(_) => return None,
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                eprintln!("run_bounded: {bin:?} {args:?} did not finish in {secs}s — killed");
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }

    /// THE ENUMERATION, against the operator's own Hamlib rather than a fixture: for EVERY
    /// model `rigctld -l` lists, the flags Nexus would emit must not name the line that model's
    /// backend keys with. This is the guard a Hamlib bump cannot walk past — 4.7.1 has exactly
    /// two such backends (1007 FT-757GXII / DTR and 1031 FT-980 / RTS, both found by reading
    /// `.ptt_type` out of `rigs/**` for that exact version), and a future release adding a third
    /// would fail here without anyone having to notice the release note.
    ///
    /// Skipped, loudly, when there is no rigctld to ask: CI has none, and the alternative — a
    /// baked-in model list — is the thing this whole module exists to avoid. Run it on a station
    /// machine, or point `NEXUS_RIGCTLD` at a bundled binary.
    #[test]
    fn no_model_is_ever_given_the_line_its_backend_keys_with() {
        // ⚠️ rigctl, THE CLIENT — never rigctld, the DAEMON. This asked the daemon to dump each
        // model's config and collected it with `.output()`, which waits for EOF. rigctld does not
        // exit, so on any machine that HAS Hamlib the first model blocked forever: `cargo test
        // --workspace` hung, and every CI run on this repo was killed at the six-hour limit having
        // never reported. It was invisible to anyone without Hamlib installed, because then the
        // binary is missing and the skip below fires instantly. `gen-hamlib-serial-speeds.mjs`
        // already uses `rigctl --dump-caps` for exactly this reason.
        let bin = std::env::var_os("NEXUS_RIGCTL")
            .or_else(|| std::env::var_os("NEXUS_RIGCTLD").map(rigctl_beside))
            .unwrap_or_else(|| rigctl_beside(resolve_rigctld()));
        // Bounded even so. The binary is right today; a future Hamlib could change what a flag
        // means, and the cost of being wrong here was six hours per run and a CI signal nobody
        // could trust.
        let Some(list) = run_bounded(&bin, &["-l"], 20) else {
            eprintln!(
                "SKIPPED: no usable rigctl to ask ({bin:?}). Set NEXUS_RIGCTL to run the full sweep."
            );
            return;
        };
        if !list.status.success() {
            eprintln!("SKIPPED: `rigctl -l` failed ({bin:?}).");
            return;
        }
        let models: Vec<u32> = String::from_utf8_lossy(&list.stdout)
            .lines()
            .filter_map(|l| l.split_whitespace().next()?.parse().ok())
            .filter(|&m| m > 0)
            .collect();
        assert!(
            models.len() > 100,
            "`rigctld -l` listed only {}",
            models.len()
        );
        let mut keyed_by_a_line = Vec::new();
        for m in models {
            let Some(out) = run_bounded(&bin, &["-m", &m.to_string(), "-L"], 10) else {
                continue;
            };
            let dump = String::from_utf8_lossy(&out.stdout);
            let Some((ptt, _)) = conf_param(&dump, "ptt_type") else {
                continue;
            };
            if ptt != "RTS" && ptt != "DTR" {
                continue;
            }
            keyed_by_a_line.push((m, ptt.to_string()));
            let lines = resolve_lines(parse_settable_lines(&dump), ControlLines::hold_low(), false);
            let args = rigctld_args(m, "COM5", 9600, 4532, false, None, lines);
            let line = if ptt == "RTS" { "rts" } else { "dtr" };
            assert!(
                says_nothing_about(&args, line),
                "model {m} keys by {ptt}: setting that line is -RIG_ECONF, i.e. dead CAT — {args:?}"
            );
        }
        eprintln!("swept every model; keyed by a serial line: {keyed_by_a_line:?}");
    }
}
