//! Does what the ROTATOR daemon says reach us — and do we notice when it has already died?
//!
//! The rig's daemon got stderr capture when the CI-V diagnostic was built. rotctld never did:
//! `spawn_rotctld` left the pipe closed and wrote "A rotator daemon's stderr is not piped, so
//! it never says anything here" on the field where the diagnosis would have gone. And rotctld
//! fails harder than rigctld does — measured against the bundled 4.7.1, `rotctld -m 901 -r
//! COM99` prints `serial_open: serial port COM99 does not exist` / `IO error` and **exits**,
//! where rigctld deliberately stays up ("the rig may be powered off"). So the two halves of the
//! commonest rotator failure were both invisible: the daemon was gone ~27 ms after launch, and
//! Nexus logged "rotctld launched".
//!
//! This runs the real `spawn_rotctld` against a stand-in that behaves the way the real one
//! does, and asks the handle both questions. Hamlib is deliberately not involved — the
//! mechanism under test is the pipe, the drain thread, the ring and `is_alive`, and a test that
//! needed Hamlib installed would self-skip on the machines where this regressed.
#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use tempo_audio::rigctld_proc::spawn_rotctld;

/// What the bundled rotctld 4.7.1 actually prints for a port that is not there, observed at
/// `src-tauri/resources/hamlib/rotctld.exe -m 901 -r COM99 -T 127.0.0.1 -t 4599`.
const HAMLIB_SAYS: &str = "serial_open: serial port COM99 does not exist";

fn stand_in(dir: &std::path::Path, script: &str) {
    std::fs::create_dir_all(dir).expect("temp dir");
    let bin = dir.join("rotctld");
    let mut f = std::fs::File::create(&bin).expect("write stand-in");
    f.write_all(script.as_bytes()).expect("write stand-in");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    // `resolve_rotctld` prefers a bundled binary beside the executable and falls back to PATH;
    // a test binary has no bundle, so this is the path it takes.
    std::env::set_var(
        "PATH",
        format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default()),
    );
}

// ONE test, both directions. `PATH` is process-global and cargo runs a file's tests in
// parallel, so two stand-ins would race to be the `rotctld` the other one resolves — which is
// exactly how this first went green against the wrong daemon.
#[test]
fn a_rotctld_that_dies_at_launch_is_heard_and_then_noticed_and_a_live_one_is_not() {
    let dir = std::env::temp_dir().join(format!("nexus-rotctld-said-{}", std::process::id()));
    // `exit 9` unless -vvv leads: without it Hamlib runs at RIG_DEBUG_NONE and the pipe drains
    // an empty stream, so a launch that lost the flag is launching a daemon that cannot explain
    // the commonest rotator fault (a port that opened and a controller that never answered).
    stand_in(
        &dir,
        &format!(
            "#!/bin/sh\n\
             [ \"$1\" = \"-vvv\" ] || exit 9\n\
             echo '{HAMLIB_SAYS}' >&2\n\
             echo 'IO error' >&2\n\
             exit 2\n"
        ),
    );

    let mut proc = spawn_rotctld(901, "COM99", 600, 45_533).expect("the stand-in must launch");

    // The drain runs on its own thread; give it a moment to see the lines.
    let deadline = Instant::now() + Duration::from_secs(3);
    let said = loop {
        let said = proc.said();
        if !said.is_empty() || Instant::now() > deadline {
            break said;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        said.iter().any(|l| l.contains(HAMLIB_SAYS)),
        "Hamlib's own diagnosis is the only thing that names the cause — wrong model, bad port, \
         port busy, refused baud all land here. got: {said:?}"
    );

    // …and the handle must be able to say the daemon is GONE. `spawn_rotctld` returning Ok only
    // means the fork succeeded; the exit came milliseconds later.
    let deadline = Instant::now() + Duration::from_secs(3);
    while proc.is_alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !proc.is_alive(),
        "a rotctld that exited must not read as running — logging 'rotctld launched' for it is \
         the whole defect"
    );
    let _ = std::fs::remove_dir_all(&dir);

    // The other direction — a guard shown to fire one way only is half a test.
    let live = std::env::temp_dir().join(format!("nexus-rotctld-live-{}", std::process::id()));
    stand_in(&live, "#!/bin/sh\nsleep 30\n");
    let mut proc = spawn_rotctld(1, "", 9600, 45_534).expect("the stand-in must launch");
    std::thread::sleep(Duration::from_millis(200));
    assert!(proc.is_alive(), "a running daemon must not read as dead");
    drop(proc);
    let _ = std::fs::remove_dir_all(&live);
}
