//! Remote Sub-receiver levels: the station rules beneath the browser grammar.
//!
//! Every refusal the desktop's Sub row gets (`Engine::request_sub_level`) is the Remote page's too,
//! behind the admission every receive-display one-shot takes (the rig-scope settings are the
//! precedent): the permit, a native source, a fresh unkeyed CAT link to the radio the page
//! displayed, no owned transmitter, no tune carrier. None of it arms, keys, retunes or saves
//! anything.
use super::*;
use crate::engine::sub_controls::SubLevel;

fn link(s: &Station) -> u64 {
    s.engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation
}

/// A Phone station on Hamlib `model` whose radio loop has reported whether its CAT path can name
/// the Sub (`route`).
fn station(model: u32, route: bool) -> Station {
    let mut s = Station::new(OperatingMode::Phone);
    s.engine.settings.rig_model = model;
    assert!(
        s.engine.take_sub_level_requests(route).is_empty(),
        "nothing queued yet"
    );
    s
}

fn attempt(s: &mut Station, level: SubLevel, value: f32) -> Result<(), Reason> {
    let connection = link(s);
    let permit = s.authority.permit(unexpired_deadline()).unwrap();
    s.engine
        .queue_remote_sub_level(level, value, connection, &permit)
}

/// What the radio loop would be handed, as (level, value).
fn drained(s: &mut Station) -> Vec<(SubLevel, f32)> {
    s.engine
        .take_sub_level_requests(true)
        .into_iter()
        .map(|w| (w.level, w.value))
        .collect()
}

#[test]
fn a_sub_level_reaches_the_desktop_verb_without_arming_retuning_or_saving() {
    for level in SubLevel::ALL {
        let mut s = station(3078, true); // IC-7610: RF, AF and squelch are the Sub's own
        let generation = s.engine.tx_gate_gen;
        let dial = s.engine.settings.dial_hz();
        attempt(&mut s, level, 0.25).unwrap_or_else(|r| panic!("{level:?}: {r:?}"));
        assert_eq!(drained(&mut s), vec![(level, 0.25)], "{level:?}");
        assert!(!s.engine.tx_enabled(), "{level:?} leaves the latch down");
        assert!(s.engine.tx_owner().is_none(), "{level:?}");
        assert_eq!(
            s.engine.tx_gate_gen, generation,
            "{level:?} leaves the TX gate alone"
        );
        assert_eq!(
            s.engine.settings.dial_hz(),
            dial,
            "{level:?} does not retune"
        );
        assert!(s.engine.take_remote_radio().is_none(), "{level:?}");
        assert!(!s.path.exists(), "{level:?} saves nothing");
        // Main's own level is never the one that moves.
        assert_eq!(s.engine.af_gain(), None, "{level:?}");
        assert_eq!(s.engine.rf_gain(), None, "{level:?}");
        assert_eq!(s.engine.squelch(), None, "{level:?}");
    }
}

/// ⭐ THE DESKTOP'S REFUSALS ARE THE PAGE'S: the same engine verb answers both, and a refusal
/// queues nothing.
#[test]
fn every_refusal_the_desktop_gets_the_page_gets_and_nothing_is_queued() {
    for (name, model, route, level, value, expected) in [
        // No Sub offered: UNKNOWN (IC-7300) is never an offer.
        (
            "no Sub offered",
            3073u32,
            true,
            SubLevel::Rf,
            0.5f32,
            Reason::HardwareUnavailable,
        ),
        // D7: no vendor statement gives the IC-9700's Sub an audio stage of its own.
        (
            "IC-9700 AF",
            3081,
            true,
            SubLevel::Af,
            0.5,
            Reason::HardwareUnavailable,
        ),
        (
            "IC-9700 squelch",
            3081,
            true,
            SubLevel::Sql,
            0.5,
            Reason::HardwareUnavailable,
        ),
        // A CAT path that cannot name the Sub (Hamlib).
        (
            "no route",
            3078,
            false,
            SubLevel::Af,
            0.5,
            Reason::HardwareUnavailable,
        ),
        // Not a level: the station never clamps a value the page should not have sent.
        (
            "not a number",
            3078,
            true,
            SubLevel::Af,
            f32::NAN,
            Reason::InvalidAction,
        ),
        (
            "above 1",
            3078,
            true,
            SubLevel::Af,
            1.5,
            Reason::InvalidAction,
        ),
        (
            "below 0",
            3078,
            true,
            SubLevel::Af,
            -0.1,
            Reason::InvalidAction,
        ),
    ] {
        let mut s = station(model, route);
        assert_eq!(attempt(&mut s, level, value), Err(expected), "{name}");
        assert!(drained(&mut s).is_empty(), "{name}: nothing queued");
    }
    // Positive controls: the IC-9700's RF gain (its own front end) and the IC-7610's AF.
    let mut s = station(3081, true);
    assert_eq!(attempt(&mut s, SubLevel::Rf, 0.5), Ok(()));
    let mut s = station(3078, true);
    assert_eq!(attempt(&mut s, SubLevel::Af, 1.0), Ok(()));
}

#[test]
fn an_owned_transmitter_a_tune_carrier_lapsed_authority_a_stale_link_or_a_companion_source_refuses_it(
) {
    for context in [
        "manual PTT",
        "tune carrier",
        "lapsed authority",
        "stale link",
        "companion source",
        "idle",
    ] {
        let mut s = station(3078, true);
        let mut connection = link(&s);
        let permit = s.authority.permit(unexpired_deadline()).unwrap();
        match context {
            "manual PTT" => s.engine.manual_ptt = true,
            "tune carrier" => s.engine.tuning = true,
            "lapsed authority" => s.authority.revoke(),
            "stale link" => connection += 1,
            // Nexus is not the program driving this radio: nothing of its own reaches the Sub.
            "companion source" => s.engine.source_kind = crate::dto::SourceKind::Companion,
            _ => {}
        }
        let generation = s.engine.tx_gate_gen;
        let result = s
            .engine
            .queue_remote_sub_level(SubLevel::Af, 0.4, connection, &permit);
        let expected = match context {
            "manual PTT" | "tune carrier" => Err(Reason::StationBusy),
            "lapsed authority" => Err(Reason::AuthorityExpired),
            "stale link" => Err(Reason::ContextChanged),
            "companion source" => Err(Reason::UnsupportedAction),
            _ => Ok(()),
        };
        assert_eq!(result, expected, "{context}");
        assert_eq!(
            drained(&mut s),
            if context == "idle" {
                vec![(SubLevel::Af, 0.4)]
            } else {
                vec![]
            },
            "{context}"
        );
        assert_eq!(s.engine.tx_gate_gen, generation, "{context}");
        assert!(!s.engine.tx_enabled(), "{context}");
    }
}
