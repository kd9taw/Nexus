//! Does what the CAT daemon says actually REACH us?
//!
//! The unit tests around this cover the two ends — that `-vvv` is on the command line
//! (`rigctld_proc`) and that a captured line is folded into the CAT status
//! (`service::with_daemon_error`). Neither can see the middle, and the middle is where this
//! bug lived for a year: `spawn_rigctld` has piped stderr and drained it on a thread since the
//! CI-V diagnostic was built, so the SHAPE was right the whole time. What it drained was an
//! empty stream (Hamlib at `RIG_DEBUG_NONE` prints nothing), and what it drained it INTO was
//! `civ::diag::note`, a no-op unless an Icom owner has armed a log file. An FT-847 owner's
//! support report therefore said "nothing noteworthy".
//!
//! So this runs the real `spawn_rigctld` against a stand-in daemon and asks the handle what it
//! heard. Hamlib is deliberately not involved: the mechanism under test is the pipe, the drain
//! thread and the ring buffer, and a test that needs Hamlib installed would self-skip on the
//! machines where this regressed.
#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use tempo_audio::rigctld_proc::{spawn_rigctld, ControlLines};

/// The exact line the bundled rigctld 4.7.1 prints at `-vvv` for a port that is not there
/// (observed: `rigctld.exe -vvv -m 1001 -r COM99 -s 57600`). At `-v` and with no flag it prints
/// nothing at all, which is the whole reason the drain had nothing to drain.
const HAMLIB_SAYS: &str = "serial_open: serial port COM99 does not exist";

#[test]
fn the_daemons_own_words_reach_the_handle_the_operator_is_told_from() {
    let dir = std::env::temp_dir().join(format!("nexus-rigctld-said-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let bin = dir.join("rigctld");
    {
        let mut f = std::fs::File::create(&bin).expect("write stand-in");
        // `exit 9` unless -vvv leads: `spawn_rigctld`'s own `--show-conf` probe runs first with
        // different arguments and is EXPECTED to fail here (it falls back to "say nothing about
        // the control lines"), while a launch that lost `-vvv` would be launching a daemon that
        // cannot report a rig that never answered — so the stand-in refuses to play one.
        f.write_all(
            format!(
                "#!/bin/sh\n\
                 [ \"$1\" = \"-vvv\" ] || exit 9\n\
                 echo '{HAMLIB_SAYS}' >&2\n\
                 sleep 30\n"
            )
            .as_bytes(),
        )
        .expect("write stand-in");
    }
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    // `resolve_rigctld` prefers a bundled binary beside the executable and falls back to PATH;
    // a test binary has no bundle, so this is the path it takes.
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    std::env::set_var("PATH", path);

    let proc = spawn_rigctld(
        1001, // FT-847 — the field report's rig
        "COM99",
        57600, // …at the rate his radio is actually on
        45_123,
        false,
        // no keying declaration — this test is about stderr capture, not line states
        Default::default(),
        ControlLines::hold_low(),
    )
    .expect("the stand-in daemon must launch");

    // The drain runs on its own thread; give it a moment to see the first line.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let said = loop {
        let said = proc.said();
        if !said.is_empty() || std::time::Instant::now() > deadline {
            break said;
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    assert!(
        said.iter().any(|l| l.contains(HAMLIB_SAYS)),
        "the daemon's diagnosis must survive as far as the handle — this is the ONLY \
         make-agnostic route to the operator, and it was a dead end for every non-Icom. \
         got: {said:?}"
    );
    drop(proc);
    let _ = std::fs::remove_dir_all(&dir);
}
