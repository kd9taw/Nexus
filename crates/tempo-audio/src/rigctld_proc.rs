//! Launching Hamlib's `rigctld` daemon ourselves.
//!
//! Instead of depending on `libhamlib` at build time, Tempo shells out to the
//! `rigctld` binary the operator already has installed and talks to it over TCP
//! (see [`crate::rig`]). [`rigctld_args`] builds the command line and is pure /
//! unit-tested; [`spawn_rigctld`] launches it and returns a kill-on-drop
//! [`Child`] so the daemon dies with Tempo.

use std::io::BufRead;
use std::process::{Child, Command, Stdio};

/// Which serial control lines rigctld is told to hold LOW for the whole session
/// (`-C rts_state=OFF` / `-C dtr_state=OFF`).
///
/// **Why any of this exists.** The Linux tty driver raises RTS and DTR when the port is opened,
/// and Hamlib does not put them back down except on a line it is itself keying: `rig.c`
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
/// **The two ways it bites, both re-observed against the bundled rigctld 4.7.1:**
/// 1. Ask it to hold the line it is *keying with* and `rig_open` returns `-RIG_ECONF`
///    (`rig.c`: "cannot set RTS with PTT by RTS" / "cannot set DTR with PTT by DTR").
/// 2. Ask it to hold RTS on a backend whose caps declare RTS/CTS hardware flow control and
///    `rig_open` returns `-RIG_ECONF` too ("cannot set RTS with hardware handshake"), whatever
///    the PTT type. ~50 backends declare it, including the FTDX10, FT-991, TS-2000 and TS-590.
///
/// Neither refusal kills the daemon — rigctld carries on serving a rig it never opened ("continue
/// even if opening the rig fails, because it may be powered off"), so a wrong flag here is not a
/// crash, it is **silently dead CAT**. That is why `false` is always the safe value: it means
/// "say nothing about this line", which is exactly what Nexus did before 1.0.2.
///
/// [`hold_low_for`] decides both from the caps of the very binary about to be launched, so it
/// cannot drift against the operator's Hamlib. It has to be asked rather than tabulated: between
/// 4.5.5 and 4.7.1 the FTDX10, TS-2000, TS-590 and FT-9000 all *gained* the hardware-handshake
/// declaration, so a model list baked in at one release would have killed CAT on the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HoldLow {
    /// Hold RTS low. Never set for a backend that uses RTS/CTS hardware flow control.
    pub rts: bool,
    /// Hold DTR low. DTR is not a flow-control line, so only the keying-line refusal applies.
    pub dtr: bool,
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
/// - `-C rts_state=OFF` / `-C dtr_state=OFF` per [`HoldLow`]: hold a control line LOW for the
///   whole session, so opening the port cannot key the transmitter. See [`HoldLow`] for the
///   Hamlib contract and the two ways it bites.
pub fn rigctld_args(
    model: u32,
    addr: &str,
    baud: u32,
    tcp_port: u16,
    network: bool,
    ptt_line: Option<crate::rig::SerialLine>,
    hold_low: HoldLow,
) -> Vec<String> {
    let mut args = vec!["-m".to_string(), model.to_string()];
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
    // Hold the control lines LOW for the session so that merely opening the port cannot key the
    // transmitter (see [`HoldLow`]). Gated on the same two things as keying — no control lines
    // exist over TCP, and an empty addr has no port — and NEVER emitted for the line rigctld is
    // itself keying with, which Hamlib refuses to open with `-RIG_ECONF`.
    if !addr.is_empty() && !network {
        if hold_low.rts && ptt_line != Some(crate::rig::SerialLine::Rts) {
            args.push("-C".to_string());
            args.push("rts_state=OFF".to_string());
        }
        if hold_low.dtr && ptt_line != Some(crate::rig::SerialLine::Dtr) {
            args.push("-C".to_string());
            args.push("dtr_state=OFF".to_string());
        }
    }
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
    /// Job-object handle (Windows) as an `isize`; 0 = none. Held for the daemon's
    /// lifetime so the kill-on-close guarantee is in force until Tempo exits.
    #[cfg(windows)]
    job: isize,
}

impl RigctldProc {
    /// The underlying child's process id, for logging.
    pub fn id(&self) -> u32 {
        self.child.id()
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

/// Locate the `rigctld` binary. Prefers one **bundled next to the app** — the
/// Windows installer ships Hamlib under the install dir (with its DLLs), so CAT
/// works with no separate Hamlib install — and falls back to `rigctld` on
/// `PATH`. Launching the bundled exe by full path lets Windows resolve its
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
    std::ffi::OsString::from("rigctld")
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
    args.push("-t".to_string());
    args.push(tcp_port.to_string());
    args
}

/// The bundled `rotctld` (ships beside rigctld in the Hamlib bundle), falling
/// back to PATH — same resolution as [`resolve_rigctld`].
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
    std::ffi::OsString::from("rotctld")
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
        #[cfg(windows)]
        job,
    })
}

/// Which control lines it is safe to hold low for `model`, asked of the very `rigctld` we are
/// about to launch (`rigctld -m <model> -u` dumps the backend's caps and exits without opening
/// the port). Asking beats tabulating: the answer then matches the operator's own Hamlib, not
/// the one this was written against (see [`HoldLow`]).
///
/// Any failure — no binary, a dump we cannot read, a non-serial backend — yields
/// [`HoldLow::default`], i.e. touch nothing. It can only ever leave us where 1.0.1 already was.
fn hold_low_for(model: u32) -> HoldLow {
    let model = model.to_string();
    let mut cmd = Command::new(resolve_rigctld());
    cmd.args(["-m", &model, "-u"]);
    cmd.stdin(Stdio::null());
    // Same no-console-window treatment as the daemon itself (Nexus is a GUI app).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.output() {
        Ok(out) if out.status.success() => parse_hold_low(&String::from_utf8_lossy(&out.stdout)),
        _ => HoldLow::default(),
    }
}

/// Read a `rigctld --dump-caps` dump: which lines may be held low for this backend.
///
/// The line that decides it is `dumpcaps.c`'s
/// `Serial speed: 4800..38400 baud, 8N2, ctrl=CTS/RTS`, printed only for `RIG_PORT_SERIAL`
/// backends. `ctrl=` is one of exactly `NONE`, `XONXOFF`, `CTS/RTS`.
///
/// ⚠️ Matched as an **allow-list** — RTS is held low only for a `ctrl=` we positively recognise
/// as leaving RTS free. Read the other way ("not CTS/RTS"), a renamed or reworded token in some
/// future Hamlib would read as "safe" and kill CAT on every hardware-handshake rig.
fn parse_hold_low(caps_dump: &str) -> HoldLow {
    let Some(serial) = caps_dump
        .lines()
        .find(|l| l.trim_start().starts_with("Serial speed:"))
    else {
        // No serial line in the caps at all: a network/none backend. Hamlib does not even offer
        // rts_state/dtr_state there (`rig_confparam_lookup` searches the serial params only for
        // RIG_PORT_SERIAL), so the flags would be inert noise.
        return HoldLow::default();
    };
    HoldLow {
        rts: serial.contains("ctrl=NONE") || serial.contains("ctrl=XONXOFF"),
        dtr: true,
    }
}

/// Spawn `rigctld` for `model` on `serial_port`@`baud`, listening on
/// `tcp_port`. Returns a kill-on-drop handle. Uses the bundled Hamlib if present
/// (see [`resolve_rigctld`]), otherwise a `rigctld` on `PATH`.
pub fn spawn_rigctld(
    model: u32,
    addr: &str,
    baud: u32,
    tcp_port: u16,
    network: bool,
    ptt_line: Option<crate::rig::SerialLine>,
) -> std::io::Result<RigctldProc> {
    // Ask the daemon about the rig BEFORE launching it, so it cannot come up holding the
    // transmitter keyed (see [`HoldLow`]). Skipped where there is no control line to hold:
    // a TCP transport, or a model that needs no port at all.
    let hold_low = if network || addr.is_empty() {
        HoldLow::default()
    } else {
        hold_low_for(model)
    };
    let args = rigctld_args(model, addr, baud, tcp_port, network, ptt_line, hold_low);
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
    // Drain the daemon's stderr on a detached thread → the diagnostic. `note` is a cheap relaxed
    // load when logging is off, so this idles for free; the thread ends when the daemon exits.
    if let Some(stderr) = child.stderr.take() {
        std::thread::Builder::new()
            .name("rigctld-stderr".into())
            .spawn(move || {
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let line = line.trim();
                    if !line.is_empty() {
                        crate::civ::diag::note(&format!("rigctld: {line}"));
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
        #[cfg(windows)]
        job,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotctld_args_mirror_the_rig_shape() {
        assert_eq!(
            rotctld_args(601, "COM7", 9600, 4533),
            vec!["-m", "601", "-r", "COM7", "-s", "9600", "-t", "4533"]
        );
        // No port (Dummy rotator for testing) → no -r/-s.
        assert_eq!(
            rotctld_args(1, "", 9600, 4533),
            vec!["-m", "1", "-t", "4533"]
        );
    }

    #[test]
    fn args_with_serial_port() {
        let args = rigctld_args(3073, "COM5", 38400, 4532, false, None, HoldLow::default());
        assert_eq!(
            args,
            vec!["-m", "3073", "-r", "COM5", "-s", "38400", "-t", "4532"]
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
            HoldLow::default(),
        );
        assert_eq!(
            args,
            vec!["-m", "23005", "-r", "192.168.1.50:4992", "-t", "4532"]
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
            HoldLow::default(),
        );
        assert_eq!(
            args,
            vec![
                "-m",
                "1042",
                "-r",
                "/dev/ttyUSB0",
                "-s",
                "19200",
                "-t",
                "4533"
            ]
        );
    }

    #[test]
    fn args_without_serial_port_omit_port_and_baud() {
        // Dummy / NET rigs need no serial device.
        let args = rigctld_args(1, "", 38400, 4532, false, None, HoldLow::default());
        assert_eq!(args, vec!["-m", "1", "-t", "4532"]);
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
            HoldLow::default(),
        );
        assert_eq!(
            args,
            vec![
                "-m", "3073", "-r", "COM5", "-s", "38400", "-P", "RTS", "-p", "COM5", "-t", "4532"
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
            HoldLow::default(),
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
            HoldLow::default(),
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
            HoldLow::default(),
        );
        assert_eq!(no_dev, vec!["-m", "1", "-t", "4532"]);
    }

    /// Every arg-shape test below reads the flag pairs rather than a whole vector, so adding an
    /// unrelated argument later cannot make a TX-safety test fail for the wrong reason.
    fn holds(args: &[String], line: &str) -> bool {
        args.windows(2)
            .any(|w| w[0] == "-C" && w[1] == format!("{line}_state=OFF"))
    }

    /// ⚠️ THE DEFECT, 1.0.1 and earlier. Nexus's PTT Method default is `vox`, so no `-P` is
    /// passed and Hamlib's `rig_open` takes the `RIG_PTT_NONE` arm, which `break`s without
    /// touching a pin — while the Linux tty driver has just raised both. On an interface that
    /// keys on RTS that is the radio put into transmit BY CONNECTING, and held there for the
    /// session. Both lines are now held low, because both key transmitters in the field: RTS is
    /// the reported one, DTR is the same defect on the equally common DTR-keyed interface (and
    /// on a DTR-keyed CW interface it is a continuous key-down).
    #[test]
    fn a_serial_rig_on_the_vox_default_holds_both_lines_low() {
        let args = rigctld_args(
            3073,
            "/dev/ttyUSB0",
            38400,
            4532,
            false,
            None,
            HoldLow {
                rts: true,
                dtr: true,
            },
        );
        assert!(holds(&args, "rts"), "RTS left high on connect: {args:?}");
        assert!(holds(&args, "dtr"), "DTR left high on connect: {args:?}");
    }

    /// ⚠️ THE INVARIANT THAT OUTRANKS THE FIX: an operator who keys by RTS or DTR must still
    /// key. Hamlib REFUSES to open a rig asked to hold the line it is keying with — `rig.c`
    /// `rig_open` returns `-RIG_ECONF`, observed from the bundled rigctld 4.7.1 as
    /// `rig_open: cannot set RTS with PTT by RTS "COM99"` and
    /// `rig_open: cannot set DTR with PTT by DTR "COM99"` — and it does NOT exit on that, it
    /// serves a rig it never opened. So this is not a preference: holding the keying line is
    /// silently dead CAT *and* dead keying at once. The OTHER line is still held, which the same
    /// binary confirms is accepted.
    #[test]
    fn the_keying_line_is_never_held_low() {
        let both = HoldLow {
            rts: true,
            dtr: true,
        };
        let rts = rigctld_args(
            3073,
            "COM5",
            38400,
            4532,
            false,
            Some(crate::rig::SerialLine::Rts),
            both,
        );
        assert!(
            !holds(&rts, "rts"),
            "would ask Hamlib to hold the RTS keying line — it refuses to open: {rts:?}"
        );
        assert!(holds(&rts, "dtr"), "DTR is not the keying line: {rts:?}");

        let dtr = rigctld_args(
            3073,
            "COM5",
            38400,
            4532,
            false,
            Some(crate::rig::SerialLine::Dtr),
            both,
        );
        assert!(
            !holds(&dtr, "dtr"),
            "would ask Hamlib to hold the DTR keying line — it refuses to open: {dtr:?}"
        );
        assert!(holds(&dtr, "rts"), "RTS is not the keying line: {dtr:?}");
    }

    /// A backend whose caps declare RTS/CTS hardware flow control owns RTS as a flow-control
    /// output; asking to hold it low is refused whatever the PTT type
    /// (`rig_open: cannot set RTS with hardware handshake "COM99"`, observed on model 1042).
    /// DTR is not a flow-control line and is unaffected — also observed.
    #[test]
    fn a_hardware_handshake_backend_keeps_its_rts_and_still_drops_dtr() {
        let args = rigctld_args(
            1042,
            "COM5",
            38400,
            4532,
            false,
            None,
            HoldLow {
                rts: false,
                dtr: true,
            },
        );
        assert!(
            !holds(&args, "rts"),
            "RTS is this backend's flow control — Hamlib refuses to open: {args:?}"
        );
        assert!(holds(&args, "dtr"), "DTR is still free: {args:?}");
    }

    /// A TCP transport has no control lines at all, and Hamlib does not even offer the config
    /// parameters for a non-serial backend. No line flags may appear — on either gate.
    #[test]
    fn a_network_rig_gets_no_line_flags() {
        let both = HoldLow {
            rts: true,
            dtr: true,
        };
        let net = rigctld_args(23005, "192.168.1.50:4992", 38400, 4532, true, None, both);
        assert!(
            !net.iter().any(|a| a == "-C"),
            "a TCP rig has no control lines: {net:?}"
        );

        let no_dev = rigctld_args(1, "", 38400, 4532, false, None, both);
        assert_eq!(no_dev, vec!["-m", "1", "-t", "4532"]);
    }

    /// The caps dumps are verbatim from the bundled `rigctld 4.7.1 -m <model> -u`. The `ctrl=`
    /// token is read as an allow-list: anything not positively known to leave RTS free must come
    /// back `rts: false`, because guessing wrong is dead CAT and guessing shy is only 1.0.1.
    #[test]
    fn parse_hold_low_reads_the_backend_caps() {
        // IC-7300: RS-232, no hardware handshake → both lines free.
        assert_eq!(
            parse_hold_low("Port type:\tRS-232\nSerial speed: 4800..115200 baud, 8N1, ctrl=NONE\n"),
            HoldLow {
                rts: true,
                dtr: true
            }
        );
        // FTDX-10: RS-232 with RTS/CTS → RTS is spoken for, DTR is not.
        assert_eq!(
            parse_hold_low(
                "Port type:\tRS-232\nSerial speed: 4800..38400 baud, 8N2, ctrl=CTS/RTS\n"
            ),
            HoldLow {
                rts: false,
                dtr: true
            }
        );
        // XON/XOFF is software flow control — it does not use RTS.
        assert!(parse_hold_low("Serial speed: 4800..9600 baud, 8N1, ctrl=XONXOFF\n").rts);
        // NET rigctl: no serial line in the caps at all → say nothing.
        assert_eq!(
            parse_hold_low("Model name:\tNET rigctl\nPort type:\tNetwork link\n"),
            HoldLow::default()
        );
        // An unreadable or unrecognised dump must fall back to 1.0.1 behaviour, never to
        // "safe to hold".
        assert_eq!(parse_hold_low(""), HoldLow::default());
        assert_eq!(
            parse_hold_low("Serial speed: 4800..38400 baud, 8N2, ctrl=SOMETHING_NEW\n"),
            HoldLow {
                rts: false,
                dtr: true
            }
        );
    }
}
