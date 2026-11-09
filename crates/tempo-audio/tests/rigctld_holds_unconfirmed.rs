//! A Hamlib that cannot confirm the RTS/DTR holds does not get a real port opened without them:
//! the launch is refused, with the operator's sentence, before any daemon runs.
//!
//! That is a `rigctl` (asked first, every time) that gives no whole dump. The `rigctld` beside it
//! is a 4.5.x one when one is on hand (`NEXUS_RIGCTLD`, or the first on PATH, linked into the
//! stand-in's directory, since the `rigctl` asked is the one beside the `rigctld` that runs),
//! otherwise a stand-in that does what 4.5.5 does after `-L`: print, then stay up; it is never
//! asked once `rigctl` ran. The `rigctl` is always a stand-in that fails. A pseudo-terminal, which
//! has no lines to hold, is launched all the same.
#![cfg(all(target_os = "linux", feature = "serial"))]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::FromRawFd;

use tempo_audio::rigctld_proc::{
    is_launch_refusal, spawn_rigctld, ControlLines, KeyingFacts, HAMLIB_CANNOT_CONFIRM_HOLDS,
};

fn script(dir: &std::path::Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::File::create(&p)
        .and_then(|mut f| f.write_all(body.as_bytes()))
        .expect("write stand-in");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// A 4.5.x rigctld to use, if there is one: its directory.
fn hamlib_45_dir() -> Option<std::path::PathBuf> {
    let candidate = std::env::var("NEXUS_RIGCTLD").unwrap_or_else(|_| "rigctld".into());
    let out = std::process::Command::new(&candidate)
        .arg("--version")
        .output()
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout);
    if !v.contains("Hamlib 4.5.") {
        return None;
    }
    eprintln!("rigctld_holds_unconfirmed: using {}", v.trim());
    std::path::Path::new(&candidate)
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.to_path_buf())
        .or_else(|| {
            std::env::var("PATH")
                .ok()?
                .split(':')
                .map(std::path::PathBuf::from)
                .find(|d| d.join("rigctld").exists())
        })
}

/// A pseudo-terminal: its master end, and the slave's path.
fn open_pty() -> (std::fs::File, String) {
    // SAFETY: plain libc calls on a descriptor this function owns; `ptsname_r` writes at most
    // `buf.len()` bytes, NUL-terminated.
    unsafe {
        let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(m >= 0, "posix_openpt");
        assert_eq!(libc::grantpt(m), 0);
        assert_eq!(libc::unlockpt(m), 0);
        let mut buf = [0 as libc::c_char; 128];
        assert_eq!(libc::ptsname_r(m, buf.as_mut_ptr(), buf.len()), 0);
        let name = std::ffi::CStr::from_ptr(buf.as_ptr())
            .to_string_lossy()
            .into_owned();
        (std::fs::File::from_raw_fd(m), name)
    }
}

#[test]
fn a_hamlib_that_cannot_confirm_the_holds_is_refused_on_a_real_port_only() {
    let dir = std::env::temp_dir().join(format!("nexus-unconfirmed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let log = dir.join("launches");
    // The failing rigctl, always.
    script(&dir, "rigctl", "#!/bin/sh\necho usage >&2\nexit 1\n");
    let real_45 = hamlib_45_dir();
    if real_45.is_none() {
        eprintln!("rigctld_holds_unconfirmed: no Hamlib 4.5.x here; a stand-in rigctld");
        // Print, then stay up after -L, as 4.5.5 does; any other launch is logged and stays up.
        script(
            &dir,
            "rigctld",
            &format!(
                "#!/bin/sh\ncase \" $* \" in *' -L '*|*' -L') echo 'rig_pathname: x'; exec sleep 60;; \
                 *' --version '*) echo 'rigctld Hamlib 4.5.5 (stand-in)'; exit 0;; esac\n\
                 echo \"$*\" >> {log}\nexec sleep 60\n",
                log = log.display()
            ),
        );
    }
    // The real rigctld goes in the SAME directory as the failing rigctl, as a link: Nexus asks
    // the rigctl beside the rigctld it will run, so the two have to sit together to be paired.
    if let Some(d) = &real_45 {
        std::os::unix::fs::symlink(d.join("rigctld"), dir.join("rigctld")).expect("link rigctld");
    }
    std::env::set_var(
        "PATH",
        format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );

    let tcp = || {
        std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .expect("free port")
    };
    let real_port = format!("/dev/nexus-test-no-such-port-{}", std::process::id());
    let refused = spawn_rigctld(
        3073,
        &real_port,
        115_200,
        tcp(),
        false,
        KeyingFacts::default(),
        ControlLines::hold_low(),
    );
    let e = match refused {
        Ok(_) => panic!("a real port was launched without confirmed holds"),
        Err(e) => e,
    };
    assert!(is_launch_refusal(&e), "refused by type: {e}");
    assert_eq!(e.to_string(), HAMLIB_CANNOT_CONFIRM_HOLDS);
    if real_45.is_none() {
        assert!(!log.exists(), "a daemon was launched before the refusal");
    }

    // A pseudo-terminal has no lines to hold: it is launched.
    let (_master, slave) = open_pty();
    let launched = spawn_rigctld(
        3073,
        &slave,
        115_200,
        tcp(),
        false,
        KeyingFacts::default(),
        ControlLines::hold_low(),
    );
    assert!(
        launched.is_ok(),
        "the pty was refused: {:?}",
        launched.err()
    );
    drop(launched);
    let _ = std::fs::remove_dir_all(&dir);
}
