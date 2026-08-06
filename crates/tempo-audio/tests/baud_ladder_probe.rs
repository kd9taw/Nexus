//! Does the Hamlib baud ladder actually TELL THE FOUR STATES APART when it runs?
//!
//! The unit tests in `baud_ladder` cover the two ends — that `probe_args` puts `-vvv` on the
//! line, and that `open_failure_line`/`parse_rigctl_read` read the right stream from the right
//! offset. Neither can see the middle, and the middle is where the bug was: `probe_rate_via_hamlib`
//! is private, it is the only place the two halves meet, and it was handing `open_failure_line`
//! output that was empty by construction because the flag that produces it was missing.
//!
//! So this runs the real `run_hamlib` against a stand-in `rigctl` that replays, verbatim, what
//! the bundled rigctl 4.7.1 printed in each of the four states — **including printing nothing at
//! all when it is not asked with `-vvv`**, which is the behaviour that made a busy COM port look
//! like a dead rig. Hamlib is deliberately not involved: the mechanism under test is the argument
//! vector, the two streams and the classification, and a test that needed Hamlib installed would
//! self-skip on the machines where this regressed.
//!
//! The captures are the same `tests/fixtures/rigctld/probe_*` files the unit tests use, taken
//! from `rigctl.exe -vvv -m 1001 -r <port> -s 9600 f` against: a COM port that does not exist, a
//! Windows named pipe whose one instance another process was holding, a stand-in rig that accepts
//! and stays mute, and a stand-in rig that answers 14.074 MHz / USB.
#![cfg(all(unix, feature = "serial"))]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;

use tempo_audio::baud_ladder::{run_hamlib, BaudProbe, LadderGate, RigCaps};

/// Which state the stand-in plays is taken from the PORT it is handed, so the four cases need no
/// process-global switch and can run in parallel.
const STANDIN: &str = r#"#!/bin/sh
vvv=0
port=
prev=
for a in "$@"; do
  [ "$a" = "-vvv" ] && vvv=1
  [ "$prev" = "-r" ] && port="$a"
  prev="$a"
done
case "$port" in
  MISSING)
    # rc 2, nothing on either stream unless asked to speak.
    [ "$vvv" = 1 ] && echo 'serial_open: serial port COM99 does not exist' >&2
    exit 2 ;;
  BUSY)
    [ "$vvv" = 1 ] && {
      echo 'WinErrorShow: serial error on CreateFileA:  failed with error 231: All pipe instances are busy.' >&2
      echo 'serial_open: serial port \\.\pipe\nexuscom is already open' >&2
    }
    exit 2 ;;
  MUTE)
    [ "$vvv" = 1 ] && echo "Opened rig model 1001, 'FT-847'"
    echo 'error = rig_get_freq: cache miss age=10005ms, cached_vfo=Main, asked_vfo=Main'
    echo 'read_block_generic(): Timed out 1.38724 seconds after 0 chars, direct=1'
    echo '2200'
    echo 'Communication timed out'
    [ "$vvv" = 1 ] && {
      echo 'read_block_generic(): Timed out 1.36928 seconds after 0 chars, direct=1' >&2
      echo 'ft847: read_block returned -5' >&2
    }
    exit 0 ;;
  WORKING)
    [ "$vvv" = 1 ] && echo "Opened rig model 1001, 'FT-847'"
    echo '14074000'
    for c in "$@"; do
      [ "$c" = "m" ] && { echo 'USB'; echo '2200'; }
    done
    exit 0 ;;
esac
exit 9
"#;

/// Put the stand-in on PATH under the name `resolve_rigctl` falls back to. A test binary has no
/// bundled Hamlib beside it, so PATH is the branch it takes.
fn install_standin() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("nexus-ladder-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let bin = dir.join("rigctl");
        std::fs::File::create(&bin)
            .and_then(|mut f| f.write_all(STANDIN.as_bytes()))
            .expect("write stand-in");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let path = format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        std::env::set_var("PATH", path);
    });
}

/// The FT-847's real bounds and coverage, so the rungs and the plausibility test are the ones
/// that ship: 4800..57600, and 14.074 MHz is inside its 20 m range.
fn ft847_caps() -> RigCaps {
    tempo_audio::baud_ladder::parse_caps(include_str!("fixtures/rigctld/caps_ft847.log"))
}

fn sweep_report(port: &str) -> tempo_audio::baud_ladder::LadderReport {
    install_standin();
    run_hamlib(port, 9600, 1001, LadderGate { keying: None }, &ft847_caps())
}

fn sweep(port: &str) -> Vec<(u32, BaudProbe)> {
    sweep_report(port).outcomes
}

/// ⭐ A port another program is holding must NOT walk the whole sweep and come back "your rig
/// never answered" — the verdict is "close WSJT-X", and it is a completely different cure.
///
/// This is the defect, executed: the stand-in prints its open failure only when it is asked with
/// `-vvv`, exactly as the real binary does, so a probe that does not ask sees zero bytes and
/// classifies every rung as silence.
#[test]
fn a_port_another_program_is_holding_is_reported_as_held_not_as_a_silent_rig() {
    let report = sweep_report("BUSY");
    for (baud, outcome) in &report.outcomes {
        match outcome {
            BaudProbe::OpenFailed(e) => assert!(
                e.contains("is already open"),
                "the operator has to be told WHICH fault: {e}"
            ),
            other => panic!(
                "a busy port came back as {other:?} at {baud} — that verdict says 'check the cable'"
            ),
        }
    }
    // …and the verdict the operator reads has to be the one with the right cure on it.
    let m = tempo_audio::baud_ladder::compose_hamlib_ladder_message(&report, "Yaesu FT-847");
    assert!(m.contains("is already open"), "{m}");
    assert!(m.contains("WSJT-X"), "{m}");
    assert!(
        !m.contains("never answered"),
        "a held port is not a silent rig: {m}"
    );
}

/// The same, for a port that is not there at all: also an open failure, also not silence.
#[test]
fn a_port_that_is_not_there_is_reported_as_not_there() {
    match &sweep("MISSING")[0].1 {
        BaudProbe::OpenFailed(e) => assert!(e.contains("does not exist"), "{e}"),
        other => panic!("expected an open failure, got {other:?}"),
    }
}

/// A rig that OPENS and never answers is the one state that really is silence — and it must stay
/// silence with the flag on, walking every rate the backend can drive.
#[test]
fn a_rig_that_opens_and_stays_mute_is_still_silence_at_every_rate() {
    let outcomes = sweep("MUTE");
    assert!(
        outcomes.iter().all(|(_, o)| *o == BaudProbe::Silence),
        "the `2200` three lines into the failure blob is a passband, not a frequency: {outcomes:?}"
    );
    // Bounded by the FT-847's declared 4800..57600, configured rate first.
    assert_eq!(
        outcomes.iter().map(|(b, _)| *b).collect::<Vec<_>>(),
        vec![9600, 4800, 57600, 19200, 38400]
    );
}

/// ⭐ And the half that cannot ship on its own: with `-vvv` the answer moves to stdout line 2, so
/// a probe that adds the flag without moving the read turns every healthy rung into silence.
#[test]
fn a_rig_that_answers_is_still_a_hit_once_the_probe_asks_hamlib_to_speak() {
    let outcomes = sweep("WORKING");
    assert_eq!(
        outcomes,
        vec![(
            9600,
            BaudProbe::Reply {
                freq_hz: Some(14_074_000)
            }
        )],
        "the configured rate answered, so the sweep is over"
    );
}
