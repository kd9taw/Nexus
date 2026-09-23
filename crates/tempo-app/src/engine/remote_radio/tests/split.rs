//! Remote split, RIT, XIT and VFO: the station rules beneath the browser grammar.
use super::*;
use crate::settings::{LicenseClass, RouteMode};

fn link(s: &Station) -> u64 {
    s.engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation
}

type Attempt = fn(&mut Engine, u64, &Permit) -> Result<(), Reason>;

fn split(e: &mut Engine, c: u64, p: &Permit) -> Result<(), Reason> {
    e.queue_remote_split(None, Some(14.076), c, p)
}
fn rit(e: &mut Engine, c: u64, p: &Permit) -> Result<(), Reason> {
    e.queue_remote_rit(0, 20, c, p)
}
fn xit(e: &mut Engine, c: u64, p: &Permit) -> Result<(), Reason> {
    e.queue_remote_xit(0, 20, c, p)
}
fn vfo(e: &mut Engine, c: u64, p: &Permit) -> Result<(), Reason> {
    e.queue_remote_vfo(false, true, c, p)
}
const ALL: [(&str, Attempt); 4] = [("split", split), ("rit", rit), ("xit", xit), ("vfo", vfo)];

fn attempt(s: &mut Station, run: Attempt) -> Result<(), Reason> {
    let connection = link(s);
    let permit = s.authority.permit(unexpired_deadline()).unwrap();
    run(&mut s.engine, connection, &permit)
}

#[test]
fn an_idle_station_accepts_each_request_without_arming_or_retuning() {
    for (name, run) in ALL {
        let mut s = Station::new(OperatingMode::Digital);
        let generation = s.engine.tx_gate_gen;
        let dial = s.engine.settings.dial_hz();
        attempt(&mut s, run).unwrap_or_else(|r| panic!("{name}: {r:?}"));
        assert!(!s.engine.tx_enabled(), "{name}");
        assert!(s.engine.tx_owner().is_none(), "{name}");
        assert_eq!(
            s.engine.tx_gate_gen, generation,
            "{name} leaves the TX gate alone"
        );
        assert_eq!(s.engine.settings.dial_hz(), dial, "{name}");
        assert!(s.engine.take_remote_radio().is_none(), "{name}");
        assert!(!s.path.exists(), "{name} saves nothing");
    }
}

#[test]
fn held_contexts_and_lapsed_authority_are_refused_before_any_request() {
    for context in 0..8 {
        for (name, run) in ALL {
            let mut s = Station::new(OperatingMode::Digital);
            let connection = link(&s);
            let permit = s.authority.permit(unexpired_deadline()).unwrap();
            match context {
                0 => s.engine.aprs_fm = true,
                1 => s.engine.fm_channel = true,
                2 => s.engine.sat_dial_owner = Some(("test pass".into(), 0)),
                3 => s.engine.machinery_park = Some("parked".into()),
                4 => s.engine.route_intent = Some(RouteMode::Fm),
                5 => s.engine.immediate_retune = true,
                // The browser's authority lapses after the permit was issued.
                6 => s.authority.revoke(),
                _ => s.engine.remote_settings_path = None,
            }
            let expected = match context {
                6 => Reason::AuthorityExpired,
                7 => Reason::UnsupportedAction,
                _ => Reason::StationBusy,
            };
            assert_eq!(
                run(&mut s.engine, connection, &permit),
                Err(expected),
                "{name} in context {context}"
            );
            assert_eq!(s.engine.take_split_request(), None);
            assert_eq!(s.engine.take_rit_apply(), None);
            assert_eq!(s.engine.take_xit_apply(), None);
            assert_eq!(s.engine.take_vfo_apply(), None);
        }
    }
}

#[test]
fn transmit_frequency_requests_wait_for_an_idle_transmitter_and_rit_does_not() {
    for (name, run) in ALL {
        let mut s = Station::new(OperatingMode::Digital);
        s.engine.set_tx_enabled(true);
        // Arming TX queues a retune one-shot the radio loop consumes on its next pass.
        s.engine.take_immediate_retune();
        let generation = s.engine.tx_gate_gen;
        let result = attempt(&mut s, run);
        if name == "rit" {
            assert_eq!(result, Ok(()));
        } else {
            assert_eq!(result, Err(Reason::StationBusy), "{name}");
        }
        assert!(s.engine.tx_enabled(), "{name} leaves the latch as it was");
        assert_eq!(s.engine.tx_gate_gen, generation, "{name}");
    }
}

#[test]
fn the_licence_judges_the_requested_transmit_frequency() {
    // A Technician may not transmit FT8 on 20 m at all: the split is refused before the verb.
    let mut s = Station::new(OperatingMode::Digital);
    s.engine.settings.license_class = LicenseClass::Technician;
    assert_eq!(attempt(&mut s, split), Err(Reason::OutsidePrivileges));
    assert_eq!(attempt(&mut s, xit), Err(Reason::OutsidePrivileges));
    assert_eq!(s.engine.split_tx_mhz(), None);
    assert_eq!(s.engine.take_xit_apply(), None);
    // Clearing a split never needs a privilege check.
    s.engine.split_tx_mhz = Some(14.076);
    let connection = link(&s);
    let permit = s.authority.permit(unexpired_deadline()).unwrap();
    assert_eq!(
        s.engine
            .queue_remote_split(Some(14.076), None, connection, &permit),
        Ok(())
    );
    assert_eq!(s.engine.split_tx_mhz(), None);
    // Positive control: a General on the same dial is allowed.
    let mut g = Station::new(OperatingMode::Digital);
    g.engine.settings.license_class = LicenseClass::General;
    assert_eq!(attempt(&mut g, split), Ok(()));
}

#[test]
fn values_the_page_did_not_display_or_cannot_reach_are_refused() {
    let mut s = Station::new(OperatingMode::Digital);
    let connection = link(&s);
    let permit = s.authority.permit(unexpired_deadline()).unwrap();
    let e = &mut s.engine;
    assert_eq!(
        e.queue_remote_split(Some(14.075), Some(14.076), connection, &permit),
        Err(Reason::ContextChanged)
    );
    assert_eq!(
        e.queue_remote_split(None, Some(7.076), connection, &permit),
        Err(Reason::InvalidAction)
    );
    assert_eq!(
        e.queue_remote_split(None, None, connection, &permit),
        Err(Reason::InvalidAction)
    );
    assert_eq!(
        e.queue_remote_rit(5, 20, connection, &permit),
        Err(Reason::ContextChanged)
    );
    assert_eq!(
        e.queue_remote_xit(0, 10_000, connection, &permit),
        Err(Reason::InvalidAction)
    );
    assert_eq!(
        e.queue_remote_vfo(true, false, connection, &permit),
        Err(Reason::ContextChanged)
    );
    // VFO selection under a split would change which frequency transmits.
    e.split_tx_mhz = Some(14.076);
    assert_eq!(
        e.queue_remote_vfo(false, true, connection, &permit),
        Err(Reason::StationBusy)
    );
    e.split_tx_mhz = None;
    e.observed_split = Some((true, Some(14_076_000), crate::engine::now_unix_secs()));
    assert_eq!(
        e.queue_remote_vfo(false, true, connection, &permit),
        Err(Reason::StationBusy)
    );
    assert_eq!(e.take_vfo_apply(), None);
}

/// A radio with no XIT refuses a browser's XIT as unsupported, and nothing is queued. The
/// IC-9700 has none (Icom A7508-3EX-4 lists RIT and no ΔTX); the same request on an IC-7610 is
/// admitted, which is what makes this a capability refusal rather than a station that refuses.
#[test]
fn a_radio_with_no_xit_refuses_a_browser_xit_and_queues_nothing() {
    for (model, want, queued) in [
        (3081, Err(Reason::UnsupportedAction), None), // IC-9700
        (3078, Ok(()), Some(20)),                     // IC-7610
    ] {
        let mut s = Station::new(OperatingMode::Digital);
        s.engine.settings.rig_model = model;
        assert_eq!(attempt(&mut s, xit), want, "model {model}");
        assert_eq!(s.engine.take_xit_apply(), queued, "model {model}");
    }
}
