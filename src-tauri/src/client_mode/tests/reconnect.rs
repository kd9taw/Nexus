//! A cold launch (the icon, no `--client` argument) connects again to the station this computer
//! last drove, unless Leave cleared the target or the station is no longer known here.
use super::*;

/// An address the operator typed for the station, kept with the target.
const AT: &str = "192.168.1.30:51800";

/// (a) After this computer enters client mode for a station, a cold start resolves to client mode
/// for that same station, at the address that was typed for it.
#[test]
fn a_cold_start_after_entering_resolves_to_the_station() {
    let path = scratch("reconnect-enter").join("client-target.json");
    let typed = Typed::read(AT).expect("a typed address");
    keep_target_at(&path, STATION, Some(&typed));
    // The cold start reads the kept target; the station is still known here.
    assert_eq!(
        resolve_target(None, read_target_at(&path), |id| id == STATION),
        Some((STATION.to_string(), Some(typed))),
        "a cold start did not resolve to the station last driven"
    );
}

/// (b) After Leave clears the target, a cold start resolves to local — this computer's own station.
#[test]
fn a_cold_start_after_leaving_resolves_to_local() {
    let path = scratch("reconnect-leave").join("client-target.json");
    keep_target_at(&path, STATION, None);
    clear_target_at(&path);
    assert_eq!(
        resolve_target(None, read_target_at(&path), |_| true),
        None,
        "a cold start after Leave entered client mode"
    );
}

/// (c) A kept target whose station this computer no longer knows (removed or forgotten) resolves
/// to local: a cold launch never drives a station the operator can no longer see.
#[test]
fn a_cold_start_for_a_forgotten_station_resolves_to_local() {
    let path = scratch("reconnect-forgotten").join("client-target.json");
    let typed = Typed::read(AT).expect("a typed address");
    keep_target_at(&path, STATION, Some(&typed));
    assert_eq!(
        resolve_target(None, read_target_at(&path), |_| false),
        None,
        "a cold start drove a station this computer no longer knows"
    );
}

/// (d) A radio window (`--profile`) opened from the picker drives its own radio: even with a kept
/// target whose station is still known, it never reconnects to that station.
#[test]
fn a_radio_profile_window_never_reconnects() {
    let path = scratch("reconnect-profile").join("client-target.json");
    keep_target_at(&path, STATION, None);
    assert_eq!(
        resolve_target(Some("r2"), read_target_at(&path), |_| true),
        None,
        "a radio window was taken over by the station the default window drove"
    );
    // Control: the default window, with the same kept target, does reconnect.
    assert!(resolve_target(None, read_target_at(&path), |_| true).is_some());
}

/// (e) A station that last refused this computer as one it no longer knows, or with another key,
/// is not driven again by a cold start: that refusal passes only when the operator pairs again. The
/// next cold start offers to pair again with it instead, once. CONTROL: a station that refused for
/// another reason, and one that took this computer again since, are driven as before.
#[test]
fn a_cold_start_after_the_station_refused_this_computer_offers_to_pair_again() {
    for reason in ["notPaired", "keyChanged"] {
        let path = scratch(&format!("reconnect-refused-{reason}")).join("client-target.json");
        keep_target_at(&path, STATION, None);
        note_refusal_at(&path, STATION, Some(reason));
        assert_eq!(
            resolve_target(None, read_target_at(&path), |_| true),
            None,
            "{reason}: a cold start drove a station that refused this computer"
        );
        assert_eq!(
            pair_again_at(&path, None, |_| true),
            Some(STATION.to_string()),
            "{reason}: the next start does not offer to pair again"
        );
        assert_eq!(
            pair_again_at(&path, None, |_| true),
            None,
            "{reason}: offered on every start, not once"
        );
        assert_eq!(
            resolve_target(None, read_target_at(&path), |_| true),
            None,
            "{reason}: offered, then driven anyway"
        );
    }
    // CONTROL: a refusal that is not about the pairing, and one cleared by a link that opened.
    let path = scratch("reconnect-refused-control").join("client-target.json");
    keep_target_at(&path, STATION, None);
    note_refusal_at(&path, STATION, Some("updateStation"));
    assert!(resolve_target(None, read_target_at(&path), |_| true).is_some());
    note_refusal_at(&path, STATION, Some("notPaired"));
    note_refusal_at(&path, STATION, None);
    assert!(resolve_target(None, read_target_at(&path), |_| true).is_some());
    assert_eq!(pair_again_at(&path, None, |_| true), None);
    // A refusal by another station than the one kept says nothing of the one kept.
    note_refusal_at(
        &path,
        "60000000-0000-4000-8000-0000000000ff",
        Some("notPaired"),
    );
    assert!(resolve_target(None, read_target_at(&path), |_| true).is_some());
}

/// (f) A radio window never offers to pair again, and never takes the offer from the default
/// window; nor is a station this computer no longer knows offered.
#[test]
fn only_the_default_window_offers_to_pair_again_with_a_known_station() {
    let path = scratch("reconnect-pair-again-scope").join("client-target.json");
    keep_target_at(&path, STATION, None);
    note_refusal_at(&path, STATION, Some("notPaired"));
    assert_eq!(pair_again_at(&path, Some("r2"), |_| true), None);
    assert_eq!(pair_again_at(&path, None, |_| false), None);
    assert_eq!(
        pair_again_at(&path, None, |_| true),
        Some(STATION.to_string()),
        "CONTROL: the default window offers it"
    );
}
