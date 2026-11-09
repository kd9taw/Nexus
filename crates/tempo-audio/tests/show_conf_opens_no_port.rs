//! Asking Hamlib about the rig opens no serial port.
//!
//! Before CAT starts (and on every reconnect, every Auto-test rung, and Test CAT's own questions)
//! Nexus asks Hamlib for its show-conf dump. Asked with no `-r`, Hamlib 4.5.5's `rigctld -L` and
//! `rigctl -L`, and 4.6.5's `rigctl -L`, go on to `rig_open` the rig's DEFAULT port, `/dev/ttyS0`,
//! with no line held: on a station with a keying interface there (COM1, a Raspberry Pi's serial
//! port) that open could key the radio. Every ask now points the rig at a path no open reaches.
//!
//! This watches the default port with inotify while each of those paths runs, and requires zero
//! opens of it. The port must be watchable: run it with a file bind-mounted over `/dev/ttyS0`
//! (`unshare -rmn sh -c 'touch F; mount --bind F /dev/ttyS0; ip link set lo up; cargo test …'`),
//! or name another default with `NEXUS_TEST_DEFAULT_PORT`. Where it cannot be watched the test
//! says NOT RUN on the real stderr. Pick the Hamlib with `NEXUS_RIGCTLD` as the other suites do.
#![cfg(all(target_os = "linux", feature = "serial"))]

use std::io::Write;
use std::time::Duration;

use tempo_audio::baud_ladder::{ladder_kind, sweep_refusal, LadderGate};
use tempo_audio::rig::SerialLine;
use tempo_audio::rigctld_proc::{spawn_rigctld, ControlLines, KeyingFacts};

mod common;

/// Opens of `path` while `f` runs, as inotify sees them; `None` if it cannot be watched.
fn opens_during(path: &str, f: impl FnOnce()) -> Option<usize> {
    let c = std::ffi::CString::new(path).ok()?;
    // SAFETY: an inotify descriptor owned here and closed before returning; `read` writes at most
    // `buf.len()` bytes, and each event is read within what it returned.
    unsafe {
        let fd = libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC);
        if fd < 0 {
            return None;
        }
        if libc::inotify_add_watch(fd, c.as_ptr(), libc::IN_OPEN) < 0 {
            libc::close(fd);
            return None;
        }
        f();
        let mut buf = [0u8; 4096];
        let mut opens = 0;
        loop {
            let n = libc::read(fd, buf.as_mut_ptr().cast(), buf.len());
            if n <= 0 {
                break;
            }
            let mut at = 0usize;
            while at < n as usize {
                let ev = &*(buf.as_ptr().add(at) as *const libc::inotify_event);
                if ev.mask & libc::IN_OPEN != 0 {
                    opens += 1;
                }
                at += std::mem::size_of::<libc::inotify_event>() + ev.len as usize;
            }
        }
        libc::close(fd);
        Some(opens)
    }
}

fn not_run(why: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "!! NOT RUN, and NOT A PASS: show_conf_opens_no_port — {why}"
    );
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free loopback port")
}

#[test]
fn asking_hamlib_about_the_rig_opens_no_serial_port() {
    let Some(bin) = common::require_daemon("rigctld", "NEXUS_RIGCTLD") else {
        return;
    };
    if let Some(dir) = std::path::Path::new(&bin)
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
    {
        let path = format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        std::env::set_var("PATH", path);
    }
    let version = std::process::Command::new("rigctl")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let watch = std::env::var("NEXUS_TEST_DEFAULT_PORT").unwrap_or_else(|_| "/dev/ttyS0".into());
    // The control: an open of our own is counted. Without it, zero would prove nothing.
    match opens_during(&watch, || drop(std::fs::File::open(&watch))) {
        Some(1) => {}
        Some(n) => panic!("control: one open of {watch} counted as {n}"),
        None => {
            not_run(&format!(
                "{watch} cannot be watched here (bind-mount a file over it)"
            ));
            return;
        }
    }
    eprintln!("show_conf_opens_no_port: {version}, watching {watch}");
    // A port shaped like a real one that opens nothing: a path to nothing.
    let port = format!("/dev/nexus-test-no-such-port-{}", std::process::id());
    // The daemon stays up a moment inside the watch, so a late open of its own is seen too.
    let launch = |baud: u32| {
        let daemon = spawn_rigctld(
            3073,
            &port,
            baud,
            free_port(),
            false,
            KeyingFacts::default(),
            ControlLines::hold_low(),
        );
        std::thread::sleep(Duration::from_millis(500));
        drop(daemon);
    };
    let mut seen = Vec::new();
    for (path, run) in [
        ("CAT open", Box::new(|| launch(115_200)) as Box<dyn Fn()>),
        ("reconnect", Box::new(|| launch(115_200))),
        // What `port_prober::probe_cat_ports` launches for each rung: no declaration, hold-low.
        ("Auto-test rung", Box::new(|| launch(19_200))),
        (
            "Test CAT: which ladder",
            Box::new(|| drop(ladder_kind(3073))),
        ),
        (
            "Test CAT: may the sweep run",
            Box::new(|| {
                let _ = sweep_refusal(
                    3073,
                    LadderGate {
                        keying: Some(SerialLine::Rts),
                        rts_keys: true,
                    },
                );
            }),
        ),
    ] {
        let opens = opens_during(&watch, &run).expect("watchable");
        seen.push(format!("{path}: {opens}"));
    }
    assert!(
        seen.iter().all(|s| s.ends_with(": 0")),
        "{version}: opens of {watch}: {seen:?}"
    );
}
