//! ONE WINDOW PER RADIO: a process bound to one radio never monitors another.
//!
//! Under simultaneous radios each radio runs in its own Nexus process (`--profile r<id>`). The
//! monitor's read-only connection used to take every enabled radio regardless, so each window
//! started a rigctld for the OTHER window's radio, on that radio's COM port and TCP port: the
//! second window adopted the first window's daemon (and lost it whenever the first recycled its
//! monitor), and the first kept retrying a COM port the second held. The single band-following
//! window (no bound radio) still monitors every other radio, exactly as before.

use super::*;

/// Three enabled radios with rig models, each on its own COM port and rigctld port; radio 0 active.
fn three_radios() -> Settings {
    Settings {
        radios: (0..3)
            .map(|id| tempo_app::settings::RadioProfile {
                id,
                enabled: true,
                rig_model: 1035,
                serial_port: format!("COM{}", id + 4),
                rigctld_port: 4540 + id as u16,
                ..tempo_app::settings::RadioProfile::default()
            })
            .collect(),
        active_radio: 0,
        ..Settings::default()
    }
}

fn ids(want: &[(u32, Transport)]) -> Vec<u32> {
    want.iter().map(|(id, _)| *id).collect()
}

#[test]
fn a_bound_process_monitors_no_other_radio() {
    let s = three_radios();
    let (active, want) = monitor_want(&s, None, Some(0));
    assert_eq!((active, ids(&want)), (0, vec![]));
}

/// A bound window whose operator moved off its own radio still watches its own, and only that.
#[test]
fn a_bound_process_switched_away_watches_only_its_own_radio() {
    let mut s = three_radios();
    s.active_radio = 2;
    let (_, want) = monitor_want(&s, None, Some(0));
    assert_eq!(ids(&want), vec![0]);
}

/// A `--profile r<id>` that names no radio in the roster opens nothing at all.
#[test]
fn a_process_bound_to_a_missing_radio_monitors_nothing() {
    let s = three_radios();
    let (_, want) = monitor_want(&s, None, Some(9));
    assert_eq!(ids(&want), Vec::<u32>::new());
}

/// The single band-following window: every enabled, non-active radio with a rig model, as before.
#[test]
fn an_unbound_process_monitors_every_other_radio() {
    let s = three_radios();
    let (active, want) = monitor_want(&s, None, None);
    assert_eq!((active, ids(&want)), (0, vec![1, 2]));
}

/// The hop from the process's config to its monitor thread: a window launched for radio 0 hands
/// its monitor a scope that takes no other radio. Without it the monitor fell back to every
/// enabled radio and each window started a rigctld for the other window's radio again.
#[test]
fn the_monitor_takes_its_scope_from_the_process_config() {
    let s = three_radios();
    let bound = RadioConfig {
        bound_radio: Some(0),
        ..RadioConfig::default()
    };
    assert_eq!(
        ids(&MonitorScope::of(&bound).want(&s, None).1),
        Vec::<u32>::new()
    );
    // Control: the single band-following window still monitors every other radio.
    assert_eq!(
        ids(&MonitorScope::of(&RadioConfig::default()).want(&s, None).1),
        vec![1, 2]
    );
}

/// The two hops the test above cannot reach: the spawn in `run_radio` and the snapshot inside
/// `monitor_loop`, a thread that never returns and opens real rig daemons. Source-scanned,
/// because what is pinned is what each one hands on: the process's scope, and nothing else.
#[test]
fn the_monitor_thread_is_spawned_with_and_reads_through_the_process_scope() {
    fn body_of<'a>(src: &'a str, sig: &str) -> &'a str {
        let rest = &src[src.find(&format!("\n{sig}")).expect(sig) + 1..];
        &rest[..rest.find("\n}\n").expect("the end of the function")]
    }
    let src = include_str!("../../service.rs");
    assert!(
        body_of(src, "pub fn run_radio(").contains("let scope = MonitorScope::of(&cfg);"),
        "run_radio hands the monitor its own config's scope"
    );
    let monitor = body_of(src, "fn monitor_loop(");
    assert!(
        monitor.contains("scope.want(") && !monitor.contains("monitor_want("),
        "the monitor's want-list goes through its scope"
    );
}

/// ONE RADIO, ONE WINDOW, for the monitor too: a radio another live Nexus window drives (by its
/// port) is never watched here, so this window never starts a rigctld for it, the daemon the
/// other window could adopt and then lose (the field crash class). Once that window lets go, the
/// radio is watched again.
#[test]
fn the_monitor_skips_a_radio_another_window_drives() {
    let dir = std::env::temp_dir().join(format!("nexus-monitor-claims-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let here = tempo_app::radio_claim::Claims::new(dir.clone());
    let other_window = tempo_app::radio_claim::Claims::new(dir.clone());
    let s = three_radios();
    other_window
        .claim("serial:COM5")
        .expect("the other window drives radio 1, on COM5");

    let (_, want) = monitor_want(&s, None, None);
    assert_eq!(
        ids(&want),
        vec![1, 2],
        "premise: unbound, both others wanted"
    );
    let want = watch_only_free(want, Some(&here));
    assert_eq!(
        ids(&want),
        vec![2],
        "COM5 is the other window's: not watched"
    );

    // CONTROL: with no claims set up, nothing changes.
    let (_, want) = monitor_want(&s, None, None);
    assert_eq!(ids(&watch_only_free(want, None)), vec![1, 2]);

    // The other window closes: COM5 is free to watch again. (A child another test forks at that
    // instant can hold a copy of the handle until its exec, a few ms.)
    drop(other_window);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while here.held_elsewhere("serial:COM5") && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (_, want) = monitor_want(&s, None, None);
    assert_eq!(ids(&watch_only_free(want, Some(&here))), vec![1, 2]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The order that makes the monitor's hand-off safe, source-scanned because what is under test is
/// the order the loop calls things in: the monitor drops a radio another window drives BEFORE it
/// reconciles (so the pool closes that daemon), lets go of the watch only AFTER (so a window
/// waiting on the watch never finds the daemon still on the port), and every open waits for other
/// windows' watches before it looks at the port at all.
#[test]
fn the_monitor_lets_go_only_after_closing_and_every_open_waits_for_it() {
    fn body_of<'a>(src: &'a str, sig: &str) -> &'a str {
        let rest = &src[src.find(&format!("\n{sig}")).expect(sig) + 1..];
        &rest[..rest.find("\n}\n").expect("the end of the function")]
    }
    let at = |hay: &str, needle: &str| hay.find(needle).unwrap_or_else(|| panic!("{needle}"));
    let src = include_str!("../../service.rs");
    let monitor = body_of(src, "fn monitor_loop(");
    assert!(at(monitor, "watch_only_free(want, claims)") < at(monitor, "reconcile_pool("));
    assert!(at(monitor, "reconcile_pool(") < at(monitor, "unwatch_all_but("));
    let open = body_of(src, "fn open_rig(");
    assert!(
        at(open, "wait_unwatched(&endpoint, MONITOR_LET_GO)")
            < at(open, "keys_through_the_tci_client(t)")
    );
}
