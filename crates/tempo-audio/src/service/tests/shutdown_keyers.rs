//! Quitting mid-message lets go of a keyline the keyer thread is holding down, and the line is
//! back up BEFORE the shutdown step returns: the app may exit the moment `SHUTDOWN_DONE` is set,
//! and on a break-in CW rig a serial key line left asserted is a carrier with no PTT involved.
//! A stop alone is not enough: it is taken by the keying thread within about 8 ms, asynchronously,
//! so the shutdown has to join the thread. The lines are recorders; the keyers' threads, channels
//! and stops are the real ones.
#![cfg(feature = "serial")]

use super::*;
use std::time::{Duration, Instant};

/// A line that records each level it is put at (`true` = key down / FSK space).
fn recorder() -> (Arc<Mutex<Vec<bool>>>, impl FnMut(bool) + Send + 'static) {
    let levels = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&levels);
    (levels, move |level| seen.lock().unwrap().push(level))
}

/// Wait (at most 5 s) until the line's last level is `true`.
fn wait_down(levels: &Mutex<Vec<bool>>) {
    let start = Instant::now();
    while levels.lock().unwrap().last() != Some(&true) && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        levels.lock().unwrap().last(),
        Some(&true),
        "control: the line must be down"
    );
}

#[test]
fn shutdown_returns_with_the_serial_cw_key_line_up() {
    let mut state = loop_state();
    let (levels, set) = recorder();
    let keyer = crate::serial_keyer::SerialKeyer::keying(set);
    keyer.send("TTTT", 5); // four 720 ms dahs: the line is down for most of 3 s
    state.serial_keyer = Some(("port".into(), "dtr".into(), keyer));
    wait_down(&levels);
    state.release_keyers_for_shutdown();
    let line = levels.lock().unwrap().clone();
    assert_eq!(
        line.last(),
        Some(&false),
        "the key line is up when the shutdown step returns; it went {line:?}"
    );
}

#[test]
fn shutdown_returns_with_the_fsk_line_at_mark() {
    let mut state = loop_state();
    let (levels, set) = recorder();
    let keyer = crate::rtty_fsk::FskKeyer::keying(set);
    keyer.send(vec![false; 50], 5.0); // 200 ms bits: long space runs
    state.rtty_keyer = Some(("port".into(), "dtr".into(), keyer));
    wait_down(&levels);
    state.release_keyers_for_shutdown();
    let line = levels.lock().unwrap().clone();
    assert_eq!(
        line.last(),
        Some(&false),
        "the FSK line is at mark when the shutdown step returns; it went {line:?}"
    );
}
