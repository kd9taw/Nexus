//! A contact another program logged leaves Nexus's own FT exchange alone, is held from the
//! services while contest mode is on, goes to none of the local loggers, and takes the running
//! activation only when it names no reference of its own.

use super::super::*;
use crate::test_util::StoredLog;
use std::sync::Mutex;

/// One raw ADIF contact as the contact port's importer hands it on (`crate::log_intake`).
fn intake_adif(call: &str, extra: &str) -> String {
    format!(
        "<CALL:{}>{call}<QSO_DATE:8>20261014<TIME_ON:6>010203<BAND:3>20m<MODE:3>FT8\
         <FREQ:9>14.075500{extra}<STATION_CALLSIGN:5>K2DEF<APP_NEXUS_VIA:6>WSJT-X<EOR>",
        call.len()
    )
}

fn ft8() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e
}

#[test]
fn an_intake_contact_leaves_the_ft_session_alone() {
    let mut e = ft8();
    // Nexus's own FT exchange with W1AW: a contact set aside for Log QSO, one a band change set
    // aside, the logged-contact tick the cockpit watches.
    e.stalled_qso = Some(e.set_aside(
        "W1AW".into(),
        None,
        Some(-4),
        ("20m".into(), (14.075_5, None)),
    ));
    e.band_change_qso = Some(e.set_aside(
        "W1AW".into(),
        None,
        Some(-4),
        ("20m".into(), (14.075_5, None)),
    ));
    let before = (
        format!("{:?}", e.stalled_qso),
        format!("{:?}", e.band_change_qso),
        e.logged_tick,
        format!("{:?}", e.snapshot().qso),
    );
    let e = Mutex::new(e);
    // Another program logs the very same station meanwhile.
    let (made, _) = crate::logwrite::intake_contact(&e, &intake_adif("W1AW", ""));
    assert_eq!(made, Ok(crate::logwrite::IntakeOutcome::Added));
    assert_eq!(e.stored_log().len(), 1, "premise: it was written");
    let e = e.into_inner().unwrap();
    let after = (
        format!("{:?}", e.stalled_qso),
        format!("{:?}", e.band_change_qso),
        e.logged_tick,
        format!("{:?}", e.snapshot().qso),
    );
    assert_eq!(after, before, "the FT session is exactly as it was");
}

#[test]
fn an_intake_contact_is_held_while_contest_mode_is_on() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    e.apply_settings(s);
    e.take_pending_uploads();
    let e = Mutex::new(e);
    let (made, _) = crate::logwrite::intake_contact(&e, &intake_adif("W1ABC", ""));
    assert_eq!(made, Ok(crate::logwrite::IntakeOutcome::Added));
    let mut e = e.into_inner().unwrap();
    assert!(
        e.take_pending_uploads().is_empty(),
        "nothing goes live during a contest"
    );
    assert_eq!(e.contest_held_uploads.len(), 1, "held for the end");
    assert_eq!(e.contest_held_uploads[0].rec.call, "W1ABC");
    // Control: with contest mode off, it is queued at once.
    let mut s = e.settings().clone();
    s.fd_active = false;
    e.apply_settings(s);
    e.take_pending_uploads();
    let e = Mutex::new(e);
    let _ = crate::logwrite::intake_contact(&e, &intake_adif("W2ABC", ""));
    let queued = engine_lock(&e).take_pending_uploads();
    assert!(queued.iter().any(|p| p.rec.call == "W2ABC"));
}

/// The local loggers Nexus forwards its own contacts to (WSJT-X UDP out, HRD, N1MM) get none of
/// another program's, and the upload worker's DXKeeper push leaves it out by its "via" mark. The
/// control is the operator's own contact, logged the same way with every forward on: it goes
/// to each.
#[test]
fn an_intake_contact_is_not_rebroadcast_to_wsjtx_hrd_n1mm_or_dxkeeper() {
    let n1mm = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    n1mm.set_read_timeout(Some(std::time::Duration::from_millis(400)))
        .unwrap();
    let mut e = Engine::new("K2DEF", "FN31", 0);
    let mut s = e.settings().clone();
    s.wsjtx_udp = true;
    s.hrd_logging = true;
    s.n1mm_upload = true;
    s.n1mm_addr = n1mm.local_addr().unwrap().to_string();
    e.apply_settings(s);
    let e = Mutex::new(e);
    let (made, _) = crate::logwrite::intake_contact(&e, &intake_adif("W1ABC", ""));
    assert_eq!(made, Ok(crate::logwrite::IntakeOutcome::Added));
    {
        let mut g = engine_lock(&e);
        assert!(g.take_pending_udp_qsos().is_empty(), "no WSJT-X QSO Logged");
        assert!(g.take_hrd_pending().is_empty(), "no HRD forward");
        let queued = g.take_pending_uploads();
        assert_eq!(queued.len(), 1, "the services, yes");
        assert!(
            crate::log_intake::from_another_program(&queued[0].rec),
            "the mark the worker leaves DXKeeper out by"
        );
    }
    let mut buf = [0u8; 2048];
    assert!(n1mm.recv_from(&mut buf).is_err(), "no N1MM broadcast");
    // Control: the operator's own contact goes to every one of them.
    let mine = tempo_core::logbook::parse_adif(
        "<CALL:5>W2ABC<QSO_DATE:8>20261014<TIME_ON:6>020203<BAND:3>20m<MODE:2>CW\
         <FREQ:6>14.025<EOR>",
    )
    .pop()
    .unwrap();
    let mut g = engine_lock(&e);
    g.log_qso(mine);
    assert_eq!(g.take_pending_udp_qsos().len(), 1);
    assert_eq!(g.take_hrd_pending().len(), 1);
    let queued = g.take_pending_uploads();
    assert!(!crate::log_intake::from_another_program(&queued[0].rec));
    drop(g);
    n1mm.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    assert!(n1mm.recv_from(&mut buf).is_ok(), "control: N1MM heard it");
}

#[test]
fn the_running_activation_is_stamped_only_on_a_contact_naming_no_reference() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_activation("POTA", "US-1234", vec![]).unwrap();
    let e = Mutex::new(e);
    let _ = crate::logwrite::intake_contact(&e, &intake_adif("W1ABC", ""));
    let _ = crate::logwrite::intake_contact(&e, &intake_adif("W2ABC", "<MY_SOTA_REF:9>W1/HA-001"));
    let log = e.stored_log();
    let mine = |call: &str| {
        log.iter()
            .find(|r| r.call == call)
            .map(|r| r.ota.my_ref.clone())
            .unwrap()
    };
    assert_eq!(mine("W1ABC").as_deref(), Some("US-1234"));
    assert_eq!(
        mine("W2ABC").as_deref(),
        Some("W1/HA-001"),
        "its own summit"
    );
}
