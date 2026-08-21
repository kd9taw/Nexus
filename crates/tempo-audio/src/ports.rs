//! Serial-port enumeration for the rig-control UI (feature `serial`).
//!
//! [`available_ports`] is public in both builds so UI code can call it
//! unconditionally. With the `serial` feature it lists the OS serial ports via
//! `serialport`; without it (the headless build, which has no libudev) it
//! returns an empty list so a port can still be typed in by hand.

/// Names of the serial ports currently present (e.g. `"COM5"`, `"/dev/ttyUSB0"`).
///
/// Returns an empty `Vec` when built without the `serial` feature, or if the
/// platform enumeration fails.
#[cfg(feature = "serial")]
pub fn available_ports() -> Vec<String> {
    // serialport's Windows enumeration walks the registry / SetupAPI and can panic on some driver
    // setups (Flex/virtual COM ports). This runs when the Settings tab opens, so isolate it — a
    // panic yields an empty list (the operator can still type a COM port) instead of crashing.
    std::panic::catch_unwind(|| {
        // `mut` is only exercised by the Linux virtual-port union below; on other
        // targets that block compiles away (this was a warning in every Windows
        // cross-build — noise that trains eyes to skip warnings).
        #[cfg_attr(not(any(target_os = "linux", windows)), allow(unused_mut))]
        let mut names: Vec<String> = match serialport::available_ports() {
            Ok(ports) => ports.into_iter().map(|p| p.port_name).collect(),
            Err(_) => Vec::new(),
        };
        // On Windows, union in the registry's own list — SetupAPI cannot see com0com-family
        // virtual pairs, and upstream's fallback is gated on a count comparison that discards
        // them whenever real adapters outnumber them (#117). Dedup by name.
        #[cfg(windows)]
        {
            names = union_ports(
                names,
                windows_registry_ports_in("HARDWARE\\DEVICEMAP\\SERIALCOMM"),
            );
        }
        // On Linux, union in the virtual ports udev structurally cannot report (see
        // `linux_virtual_ports`). Dedup by path — a node reported both ways must appear once.
        #[cfg(target_os = "linux")]
        for v in linux_virtual_ports(std::path::Path::new("/dev")) {
            if !names.contains(&v) {
                names.push(v);
            }
        }
        #[cfg(target_os = "macos")]
        let names = collapse_tty_twin_names(names);
        names
    })
    .unwrap_or_else(|_| {
        // NEVER swallow silently: a per-poll panic here is invisible but costs real
        // CPU (unwind + panic hook each time) — the "sluggish laptop" failure mode.
        // Rate-limited so a storm doesn't also flood stderr.
        use std::sync::atomic::{AtomicU32, Ordering};
        static CAUGHT: AtomicU32 = AtomicU32::new(0);
        let n = CAUGHT.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 || n.is_multiple_of(100) {
            eprintln!(
                "nexus: serial-port enumeration panicked (caught; occurrence {n}) — \
                 a driver/udev issue on this system; ports list returned empty"
            );
        }
        Vec::new()
    })
}

/// Virtual serial ports on Linux that `serialport`'s udev enumeration cannot see.
///
/// `serialport` 4.9 asks udev for the `tty` subsystem and then keeps a device only when
/// `parent.is_some() || is_rfcomm(..)` (posix/enumerate.rs). PTY-backed virtual ports have no
/// udev parent, and `/dev/pts/N` gets no persistent `/sys/class/tty` entry at all, so they are
/// invisible to that API BY DESIGN — not a bug we can fix upstream-side. Hams hit this whenever
/// a virtual pair bridges Nexus to another program (a rigctld/flrig bridge, WSJT-X interop, a
/// GPS feed). Reported symptom: "CAT works but no ports are listed" — CAT works because it
/// connects to a typed path or a network host and never needs enumeration.
///
/// We deliberately do NOT sweep `/dev/pts/*`: those are ordinary terminal sessions (every open
/// shell is one), and listing them would bury the real ports under junk. We match only what
/// virtual-serial tooling actually creates:
///   * a symlink in `/dev` resolving to a pts node — socat's `PTY,link=/dev/ttyV0` convention,
///     i.e. a path a human deliberately created to BE a serial port;
///   * `tnt*` — tty0tty's kernel-module nodes (the com0com equivalent).
///
/// Takes the directory so it is testable without root or a real virtual port.
#[cfg(all(feature = "serial", target_os = "linux"))]
fn linux_virtual_ports(dev: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dev) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            // tty0tty (and lookalikes): a real char device named tnt0..tnt7.
            if name.starts_with("tnt") {
                return Some(path.to_string_lossy().into_owned());
            }
            // socat-style: a symlink someone made to stand in for a serial port. Resolving to a
            // pts node is what distinguishes it from the many other symlinks in /dev.
            let meta = std::fs::symlink_metadata(&path).ok()?;
            if !meta.file_type().is_symlink() {
                return None;
            }
            let target = std::fs::read_link(&path).ok()?;
            target
                .to_string_lossy()
                .contains("pts/")
                .then(|| path.to_string_lossy().into_owned())
        })
        .collect();
    out.sort(); // stable order — the picker must not reshuffle between polls
    out
}

/// COM ports named in `HKLM\\HARDWARE\\DEVICEMAP\\SERIALCOMM` — the Windows twin of
/// [`linux_virtual_ports`], and for the same reason.
///
/// ⚠️ WHY THIS EXISTS. `serialport`'s Windows enumeration asks SetupAPI for the `Ports` and
/// `Modem` setup classes only, and com0com-family virtual pairs install under their OWN class,
/// so they are invisible to it. Upstream does read SERIALCOMM as a fallback — but gates it on
/// `raw_ports_set.len() > ports.len()`, a COUNT comparison rather than a union: with three real
/// FTDI adapters present, registry names are discarded unless SERIALCOMM holds strictly more
/// than three. A station with three adapters and two virtual ports therefore sees the three and
/// never the two (issue #117, SmartSDR CAT's virtual pair).
///
/// SERIALCOMM is where every serial driver publishes its port name, physical or virtual, so
/// UNIONING it can only ever add a port that genuinely exists. Nexus does not filter what it
/// finds: a name here is a name the operator can open.
///
/// Takes the key so it is testable without touching the real registry.
#[cfg(all(feature = "serial", windows))]
fn windows_registry_ports_in(key_path: &str) -> Vec<String> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    // Hand-rolled rather than adding a registry crate for one read: this is ~40 lines against a
    // new dependency on the operator's serial path, and dependencies here are permanent.
    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExW(key: isize, sub: *const u16, opts: u32, sam: u32, out: *mut isize) -> i32;
        fn RegEnumValueW(
            key: isize,
            index: u32,
            name: *mut u16,
            name_len: *mut u32,
            reserved: *mut u32,
            kind: *mut u32,
            data: *mut u8,
            data_len: *mut u32,
        ) -> i32;
        fn RegCloseKey(key: isize) -> i32;
    }
    const HKEY_LOCAL_MACHINE: isize = -2147483646; // 0x80000002 as isize
    const KEY_READ: u32 = 0x2_0019;
    const ERROR_SUCCESS: i32 = 0;

    let wide: Vec<u16> = std::ffi::OsStr::new(key_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut key: isize = 0;
    // A missing key is the ordinary answer on a machine with no serial ports at all.
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide.as_ptr(), 0, KEY_READ, &mut key) }
        != ERROR_SUCCESS
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0u32;
    loop {
        let mut name = [0u16; 256];
        let mut name_len = name.len() as u32;
        let mut data = [0u8; 512];
        let mut data_len = data.len() as u32;
        let rc = unsafe {
            RegEnumValueW(
                key,
                i,
                name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                data.as_mut_ptr(),
                &mut data_len,
            )
        };
        if rc != ERROR_SUCCESS {
            break; // ERROR_NO_MORE_ITEMS, or anything else — stop rather than guess
        }
        // The VALUE is the port name ("COM6"); the value's NAME is the device path.
        let bytes = &data[..data_len as usize];
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .collect();
        let port = std::ffi::OsString::from_wide(&u16s)
            .to_string_lossy()
            .trim()
            .to_string();
        if !port.is_empty() {
            out.push(port);
        }
        i += 1;
    }
    unsafe { RegCloseKey(key) };
    out.sort(); // stable order — the picker must not reshuffle between polls
    out
}

/// Add `extra` names that `have` does not already contain, case-insensitively.
///
/// ⚠️ THIS IS THE RULE UPSTREAM GOT WRONG, and the whole of issue #117. `serialport`'s Windows
/// path compares COUNTS — it keeps the registry names only when there are strictly more of them
/// than SetupAPI found — so a station with three real adapters and two virtual ports sees three
/// and never the two. A union cannot do that: every name either enumeration knows about is
/// offered exactly once.
///
/// Case-insensitive because Windows writes `COM6` and nothing guarantees the two sources agree
/// on case; matching case-sensitively would list the same port twice.
///
/// Pure string logic, compiled everywhere so the test runs on every platform; only the Windows
/// enumeration calls it.
#[cfg_attr(not(all(feature = "serial", windows)), allow(dead_code))]
fn union_ports(mut have: Vec<String>, extra: Vec<String>) -> Vec<String> {
    for v in extra {
        if !have.iter().any(|n| n.eq_ignore_ascii_case(&v)) {
            have.push(v);
        }
    }
    have
}

/// Names of the serial ports currently present.
///
/// Without the `serial` feature there is no enumeration backend, so this
/// returns an empty `Vec`; the operator can still type a port name manually.
#[cfg(not(feature = "serial"))]
pub fn available_ports() -> Vec<String> {
    Vec::new()
}

/// Name-level twin collapse for the plain string list ([`available_ports`]).
///
/// Same rule as [`collapse_tty_twins`]: a `/dev/tty.X` whose `/dev/cu.X` twin is present is
/// dropped; a lone `tty.*` is kept (it is then the only node there is). The #92 collapse was
/// applied only to [`available_usb_ports`], which feeds detection and the auto-test sweep —
/// but the Settings rig picker reads THIS list, so it still offered the tty twin, and a
/// picked `tty.*` HANGS on carrier detect instead of failing (Mac field report, 2026-08-17).
///
/// Pure string logic, compiled everywhere so the test runs on every platform; only the
/// macOS enumeration calls it.
#[cfg_attr(not(all(feature = "serial", target_os = "macos")), allow(dead_code))]
fn collapse_tty_twin_names(names: Vec<String>) -> Vec<String> {
    let callouts: std::collections::HashSet<&str> = names
        .iter()
        .filter_map(|n| n.strip_prefix("/dev/cu."))
        .collect();
    let drop: Vec<String> = names
        .iter()
        .filter(|n| {
            n.strip_prefix("/dev/tty.")
                .is_some_and(|rest| callouts.contains(rest))
        })
        .cloned()
        .collect();
    names.into_iter().filter(|n| !drop.contains(n)).collect()
}

/// The stored-port heal, pure core: the cu twin to USE INSTEAD when `port` is a macOS
/// `/dev/tty.*` name whose `/dev/cu.*` twin `exists`; `None` = use the port as stored.
///
/// The twin collapses above are ENUMERATION-ONLY — they filter what the picker and detection
/// OFFER — but during 1.5.0–1.6.1 the picker offered the tty twin as an equal row, so real
/// Mac configs still HOLD one, and every consumer of the stored value (rigctld's `-r`, the
/// native CI-V open, the baud ladder, serial PTT) passed it verbatim: CAT kept hanging on
/// carrier detect after the upgrade that fixed the picker (mac QA audit, 2026-08-17). Same
/// rule as [`collapse_tty_twins`], applied at the consuming end: a lone `tty.*` with no cu
/// twin is kept — it is then the only node there is, and "healing" it to a node that does
/// not exist would turn a working port into a vanished one.
///
/// The existence check is a parameter so this is pure string logic, compiled and tested on
/// every platform (the [`collapse_tty_twin_names`] discipline); only the macOS wrapper below
/// consults the filesystem.
pub fn heal_tty_twin_with(port: &str, cu_twin_exists: impl Fn(&str) -> bool) -> Option<String> {
    let rest = port.strip_prefix("/dev/tty.")?;
    let cu = format!("/dev/cu.{rest}");
    cu_twin_exists(&cu).then_some(cu)
}

/// The open-time heal for a STORED serial-port name: on macOS, substitute the `/dev/cu.*`
/// twin for a saved `/dev/tty.*` when the twin is present ([`heal_tty_twin_with`]); identity
/// everywhere else and for every other name. Called where the stored value is about to be
/// consumed (`service::Transport`, the Test-CAT ladder) — the startup settings migration
/// rewrites the file, but a rig that was unplugged at launch is only ever healed here.
///
/// Logs the substitution once per session (the open path re-runs every settings tick).
pub fn heal_stored_port(port: String) -> String {
    #[cfg(target_os = "macos")]
    if let Some(cu) = heal_tty_twin_with(&port, |p| std::path::Path::new(p).exists()) {
        static NOTED: std::sync::Once = std::sync::Once::new();
        NOTED.call_once(|| {
            eprintln!(
                "nexus: configured serial port {port} is a /dev/tty.* node (a 1.5.0–1.6.1 mac \
                 picker offered it) — using its callout twin {cu} instead; tty.* hangs CAT on \
                 carrier detect"
            );
        });
        return cu;
    }
    port
}

/// A USB serial port plus the descriptor fields zero-config setup reads to identify
/// the radio (model from `product`) and the bridge chip / driver (from `vid`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbPort {
    pub port_name: String,
    pub vid: u16,
    pub pid: u16,
    pub product: String,
    pub manufacturer: String,
}

/// USB serial ports currently present, with their USB descriptor fields. Non-USB
/// ports (legacy RS-232, Bluetooth SPP, …) are omitted — zero-config only reasons
/// about USB. Empty without the `serial` feature, or if enumeration fails or PANICS
/// ([`guarded_usb_ports`] — this runs when the Settings tab opens; #132).
#[cfg(feature = "serial")]
pub fn available_usb_ports() -> Vec<UsbPort> {
    guarded_usb_ports(|| {
        use serialport::SerialPortType;
        let ports = match serialport::available_ports() {
            Ok(ports) => ports
                .into_iter()
                .filter_map(|p| match p.port_type {
                    SerialPortType::UsbPort(info) => Some(UsbPort {
                        port_name: p.port_name,
                        vid: info.vid,
                        pid: info.pid,
                        product: info.product.unwrap_or_default(),
                        manufacturer: info.manufacturer.unwrap_or_default(),
                    }),
                    _ => None,
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        #[cfg(target_os = "macos")]
        let ports = collapse_tty_twins(ports);
        ports
    })
}

/// Panic containment for [`available_usb_ports`] — the same guard [`available_ports`] and
/// `device::available_devices` carry, and for the same reason: a driver DLL that panics while
/// enumerating must not take the window down when the operator opens Settings.
///
/// #132 (K4KCX) found this one UNGUARDED while both its siblings were wrapped, even though all
/// three run on the Settings MOUNT path (`SettingsPanel` → `get_serial_ports_detailed`) — so
/// the click that opened the gear ran two guarded enumerations and one bare one.
///
/// ⚠️ A native ACCESS VIOLATION inside a driver DLL is not a Rust panic and is NOT caught here
/// (see `src-tauri/src/main.rs`); this closes a real hole but does not prove it was #132's.
///
/// The enumeration is an argument for the same reason [`linux_virtual_ports`] takes its
/// directory and [`heal_tty_twin_with`] takes its existence check: the part that touches the
/// machine is the untestable part, so it goes outside. That makes the guard exercisable on
/// every platform without a broken driver to hand.
#[cfg_attr(not(feature = "serial"), allow(dead_code))]
fn guarded_usb_ports(
    enumerate: impl FnOnce() -> Vec<UsbPort> + std::panic::UnwindSafe,
) -> Vec<UsbPort> {
    std::panic::catch_unwind(enumerate).unwrap_or_else(|_| {
        // NEVER swallow silently: detection and the auto-test sweep poll this, so a per-poll
        // panic is invisible but costs real CPU (unwind + panic hook every time) — the
        // "sluggish laptop" failure mode. Rate-limited so a storm doesn't also flood stderr.
        use std::sync::atomic::{AtomicU32, Ordering};
        static CAUGHT: AtomicU32 = AtomicU32::new(0);
        let n = CAUGHT.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 || n.is_multiple_of(100) {
            eprintln!(
                "nexus: USB serial-port enumeration panicked (caught; occurrence {n}) — \
                 a driver issue on this system; USB port list returned empty"
            );
        }
        Vec::new()
    })
}

/// macOS lists every serial port TWICE: `/dev/cu.X` and `/dev/tty.X` are the same hardware.
///
/// They are offered side by side, differ by four characters, and look interchangeable — but only
/// `cu.*` (callout) is usable for a rig. `tty.*` (dial-in) blocks on carrier detect, so choosing it
/// does not fail, it HANGS, which is the hardest kind of wrong answer to diagnose. Listing it as an
/// equal option is offering the operator a choice where one branch is always wrong.
///
/// Measured on a two-radio station: 22 rows for 2 radios, half of them tty twins.
///
/// A `tty.*` with NO `cu.*` twin is KEPT: it is then the only node there is, and dropping a real
/// port would look exactly like a rig that stopped existing.
#[cfg(all(feature = "serial", target_os = "macos"))]
fn collapse_tty_twins(ports: Vec<UsbPort>) -> Vec<UsbPort> {
    let callouts: std::collections::HashSet<&str> = ports
        .iter()
        .filter_map(|p| p.port_name.strip_prefix("/dev/cu."))
        .collect();
    let drop: Vec<String> = ports
        .iter()
        .filter(|p| {
            p.port_name
                .strip_prefix("/dev/tty.")
                .is_some_and(|rest| callouts.contains(rest))
        })
        .map(|p| p.port_name.clone())
        .collect();
    ports
        .into_iter()
        .filter(|p| !drop.contains(&p.port_name))
        .collect()
}

/// USB serial ports — empty without the `serial` feature (no enumeration backend).
#[cfg(not(feature = "serial"))]
pub fn available_usb_ports() -> Vec<UsbPort> {
    Vec::new()
}

#[cfg(test)]
mod tests {

    /// ISSUE #117: SmartSDR CAT's virtual COM ports never appeared in the rig picker.
    ///
    /// Nexus filters nothing — the gap is upstream. `serialport`'s Windows enumeration asks
    /// SetupAPI for the Ports/Modem classes only, and com0com-family virtual pairs install under
    /// their own class. Its SERIALCOMM fallback exists but is gated on a COUNT comparison, so
    /// with three real FTDI adapters present the registry names are thrown away unless there are
    /// strictly more than three of them. The reporter had exactly that: three adapters, and his
    /// virtual pair invisible.
    ///
    /// The rule is a UNION, and this is it. (Nexus already does the same thing on Linux for the
    /// same reason — `linux_virtual_ports` — and the Windows side was never mirrored.)
    #[test]
    fn the_registry_ports_are_unioned_not_count_compared() {
        // The reporter's machine: three real ports, two virtual ones only the registry knows.
        let setupapi = vec!["COM3".to_string(), "COM4".to_string(), "COM5".to_string()];
        let registry = vec!["COM3".to_string(), "COM6".to_string(), "COM7".to_string()];
        let got = union_ports(setupapi.clone(), registry);
        assert_eq!(
            got,
            vec!["COM3", "COM4", "COM5", "COM6", "COM7"],
            "every port either source knows about, exactly once — a COUNT test would have kept \
             none of the virtual ones here"
        );

        // A port both sources report appears ONCE, whatever the case.
        assert_eq!(
            union_ports(vec!["COM6".to_string()], vec!["com6".to_string()]),
            vec!["COM6"],
            "Windows case must not duplicate a port"
        );

        // Nothing new to add is not an error, and order is preserved.
        assert_eq!(union_ports(setupapi.clone(), Vec::new()), setupapi);
        // …and with nothing enumerated at all, the registry IS the list.
        assert_eq!(
            union_ports(Vec::new(), vec!["COM9".to_string()]),
            vec!["COM9"]
        );
    }

    /// macOS offers every serial port twice. `tty.*` blocks on carrier detect and HANGS rather
    /// than failing, so it is never the right node for a rig — but it sits beside its `cu.*` twin
    /// in the picker and looks interchangeable. A two-radio station showed 22 rows for 2 radios.
    #[cfg(all(feature = "serial", target_os = "macos"))]
    #[test]
    fn tty_twins_collapse_but_a_lone_tty_survives() {
        let mk = |n: &str| UsbPort {
            port_name: n.to_string(),
            vid: 0x10c4,
            pid: 0xea70,
            product: "CP2105".into(),
            manufacturer: "Silicon Labs".into(),
        };
        let got = collapse_tty_twins(vec![
            mk("/dev/cu.usbserial-A"),
            mk("/dev/tty.usbserial-A"),
            mk("/dev/tty.lonelyport"),
            mk("/dev/cu.usbserial-B"),
        ]);
        let names: Vec<&str> = got.iter().map(|p| p.port_name.as_str()).collect();
        assert!(
            names.contains(&"/dev/cu.usbserial-A"),
            "the callout survives: {names:?}"
        );
        assert!(
            names.contains(&"/dev/cu.usbserial-B"),
            "and so does the other one: {names:?}"
        );
        assert!(
            !names.contains(&"/dev/tty.usbserial-A"),
            "the dial-in twin must go: {names:?}"
        );
        assert!(
            names.contains(&"/dev/tty.lonelyport"),
            "a tty with NO cu twin is the only node there is and must be KEPT: {names:?}"
        );
        assert_eq!(got.len(), 3);
    }
    use super::*;

    #[cfg(all(feature = "serial", target_os = "linux"))]
    #[test]
    fn linux_virtual_ports_finds_the_real_ones_and_ignores_terminals() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("nexus-devscan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pts = dir.join("pts");
        std::fs::create_dir_all(&pts).unwrap();

        // Terminal sessions — every open shell is one of these. They must NEVER be listed:
        // burying the operator's real ports under a dozen tty sessions is worse than the bug.
        for n in ["0", "1", "2"] {
            std::fs::write(pts.join(n), b"").unwrap();
        }
        // A socat-style virtual port: a symlink a human deliberately created to BE a port.
        symlink("pts/2", dir.join("ttyV0")).unwrap();
        // tty0tty's kernel node.
        std::fs::write(dir.join("tnt0"), b"").unwrap();
        // Decoys that must not match: a plain file, and a symlink pointing somewhere else.
        std::fs::write(dir.join("null"), b"").unwrap();
        symlink("../tmp", dir.join("shm")).unwrap();

        let found = linux_virtual_ports(&dir);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.rsplit('/').next().unwrap().to_string())
            .collect();

        assert!(
            names.contains(&"ttyV0".to_string()),
            "socat PTY link: {names:?}"
        );
        assert!(
            names.contains(&"tnt0".to_string()),
            "tty0tty node: {names:?}"
        );
        assert_eq!(names.len(), 2, "nothing else may be listed: {names:?}");
        assert!(
            !names.iter().any(|n| n == "0" || n == "1" || n == "2"),
            "raw pts terminal sessions must never reach the port picker: {names:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(all(feature = "serial", target_os = "linux"))]
    #[test]
    fn linux_virtual_ports_is_quiet_when_dev_is_unreadable() {
        // A missing/unreadable /dev must yield an empty list, never a panic — this runs every
        // time the Settings tab opens.
        assert!(linux_virtual_ports(std::path::Path::new("/nonexistent-xyz")).is_empty());
    }

    /// The rig picker's string list gets the same twin collapse as detection — the #92 fix
    /// filtered [`available_usb_ports`] only, so the picker kept offering `/dev/tty.*` rows
    /// that hang a CAT probe on carrier detect. Non-mac names pass through untouched.
    #[test]
    fn the_pickers_name_list_collapses_tty_twins_too() {
        let got = collapse_tty_twin_names(vec![
            "/dev/cu.usbserial-A".into(),
            "/dev/tty.usbserial-A".into(),
            "/dev/tty.lonelyport".into(),
            "COM5".into(),
            "/dev/ttyUSB0".into(),
        ]);
        assert_eq!(
            got,
            vec![
                "/dev/cu.usbserial-A".to_string(),
                "/dev/tty.lonelyport".into(),
                "COM5".into(),
                "/dev/ttyUSB0".into(),
            ],
            "the cu twin survives, its tty twin goes, everything else is untouched"
        );
    }

    /// ⭐ FAILING-FIRST for the stored-port heal (mac QA audit, 2026-08-17). The twin collapse
    /// above is ENUMERATION-ONLY — it filters what the picker and detection OFFER — so a
    /// `/dev/tty.*` saved by a 1.5.0–1.6.1 install (whose picker offered the twin as an equal
    /// row) still reaches rigctld/native-CI-V/PTT verbatim after an upgrade and hangs CAT on
    /// carrier detect. The heal is the same rule at the CONSUMING end: substitute the cu twin
    /// when it is present, keep a lone tty.* (it is then the only node there is).
    #[test]
    fn a_stored_tty_twin_heals_to_cu_and_a_lone_tty_is_kept() {
        // The cu twin is live → substitute.
        assert_eq!(
            heal_tty_twin_with("/dev/tty.usbserial-1420", |p| p == "/dev/cu.usbserial-1420"),
            Some("/dev/cu.usbserial-1420".to_string())
        );
        // No cu twin → the tty node is the only node there is; use it as-is.
        assert_eq!(
            heal_tty_twin_with("/dev/tty.usbserial-1420", |_| false),
            None
        );
        // Everything that is not a /dev/tty.* name passes through untouched — and the
        // existence check must not even be consulted (it stats the filesystem).
        for name in ["/dev/cu.usbserial-1420", "COM5", "/dev/ttyUSB0", ""] {
            assert_eq!(
                heal_tty_twin_with(name, |_| panic!("no disk check for {name}")),
                None
            );
        }
    }

    /// The public wrapper is IDENTITY for every name the heal does not apply to, on every
    /// platform — a Linux `/dev/ttyUSB0` or a Windows `COM5` must never be rewritten, and a
    /// mac tty.* whose twin is absent stays as-is (off macOS the whole heal is compiled out).
    #[test]
    fn heal_stored_port_never_touches_a_port_it_cannot_improve() {
        for name in ["COM5", "/dev/ttyUSB0", "/dev/cu.usbserial-1420", ""] {
            assert_eq!(heal_stored_port(name.to_string()), name);
        }
        // A twin that does not exist (on any platform) is left alone.
        let lone = "/dev/tty.nexus-test-no-such-twin";
        assert_eq!(heal_stored_port(lone.to_string()), lone);
    }

    /// ⭐ FAILING-FIRST for #132 (K4KCX, "Settings gear crashes Nexus on Windows 10 and 11").
    ///
    /// [`available_usb_ports`] is on the Settings MOUNT path (`SettingsPanel` →
    /// `get_serial_ports_detailed`) and was the ONE of the three enumerations there with no
    /// `catch_unwind` — its two siblings, [`available_ports`] and `device::available_devices`,
    /// both carry one, and both comments say the guard exists BECAUSE this runs when the
    /// Settings tab opens. Unguarded, a driver that panics while enumerating unwinds out of a
    /// Tauri command thread and takes the window down on the click of the gear.
    ///
    /// ⚠️ This does NOT prove #132's cause: a native access violation inside a driver DLL is
    /// not a Rust panic and `catch_unwind` cannot see it.
    #[test]
    fn a_panicking_usb_enumeration_is_contained() {
        let ports = guarded_usb_ports(|| panic!("driver DLL panicked while enumerating"));
        assert!(
            ports.is_empty(),
            "a panicking enumerator must yield an empty list (the operator can still TYPE a \
             COM port), never unwind into the caller: {ports:?}"
        );
        // The other direction — the guard must not be a blanket swallow. A normal enumeration
        // reaches the picker unchanged, or "no crash" would be indistinguishable from "no ports".
        let port = UsbPort {
            port_name: "COM5".into(),
            vid: 0x10c4,
            pid: 0xea60,
            product: "CP2102 USB to UART Bridge Controller".into(),
            manufacturer: "Silicon Labs".into(),
        };
        assert_eq!(
            guarded_usb_ports(|| vec![port.clone()]),
            vec![port],
            "a healthy enumeration must pass through the guard untouched"
        );
    }

    #[test]
    fn available_ports_is_callable() {
        // We can't assert hardware is present; just prove the function exists
        // and returns a Vec in either build configuration.
        let _ports: Vec<String> = available_ports();
    }
}
