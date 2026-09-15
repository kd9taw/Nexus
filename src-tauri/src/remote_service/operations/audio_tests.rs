//! Who may listen to the station's receive audio.
//!
//! The one thing these assert is a separation the operator makes at the radio: the
//! logging grant and the control grant are given independently, and hearing the shack
//! belongs to the second one. Every case that PASSES here is paired with one that must
//! fail, because a permission test that only ever says yes proves nothing.

use super::*;

const LEASE_DEVICE: &str = DEVICE;

/// The live lease id for a browser that has acquired control.
fn controlling(f: &Fixture, now: Instant) -> String {
    acquire_controls_version(f, now, 4)["leaseId"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_logging_only_browser_may_not_listen() {
    let f = Fixture::new();
    let now = Instant::now();
    // The logging grant and a perfectly valid lease. This browser can write to the log.
    let lease = f.acquire(now)["leaseId"].as_str().unwrap().to_owned();
    assert_eq!(
        f.authority
            .audio_admitted(SESSION, LEASE_DEVICE, &lease, now),
        Err("notController"),
        "a browser allowed to log was also allowed to listen"
    );

    // The control, and it is the whole point: the SAME browser, the SAME session, with
    // the control grant added, is admitted. So the refusal above is about the grant and
    // not about some other thing being wrong with the fixture.
    let f = Fixture::new();
    let now = Instant::now();
    let lease = controlling(&f, now);
    assert_eq!(
        f.authority
            .audio_admitted(SESSION, LEASE_DEVICE, &lease, now),
        Ok(())
    );
}

#[test]
fn listening_needs_this_browsers_own_live_lease() {
    let f = Fixture::new();
    let now = Instant::now();
    let lease = controlling(&f, now);
    assert_eq!(
        f.authority
            .audio_admitted(SESSION, LEASE_DEVICE, &lease, now),
        Ok(())
    );

    // Another session cannot borrow the lease it can see.
    assert_eq!(
        f.authority.audio_admitted(OTHER, LEASE_DEVICE, &lease, now),
        Err("notController")
    );
    // Nor can a made-up one be presented.
    assert_eq!(
        f.authority.audio_admitted(
            SESSION,
            LEASE_DEVICE,
            "50000000-0000-4000-8000-000000000001",
            now
        ),
        Err("notController")
    );
    // Control FIRST, and the order is load-bearing: expiry is not a read, it retires the
    // lease. One second on, inside the five-second lease, the same call still succeeds.
    assert_eq!(
        f.authority
            .audio_admitted(SESSION, LEASE_DEVICE, &lease, now + Duration::from_secs(1)),
        Ok(())
    );
    // A lease that has lapsed stops the audio: this is the browser that closed its tab,
    // lost its link, or simply stopped sending heartbeats.
    assert_eq!(
        f.authority
            .audio_admitted(SESSION, LEASE_DEVICE, &lease, now + Duration::from_secs(6)),
        Err("notController")
    );
}

#[test]
fn revoking_station_control_stops_a_listener() {
    let f = Fixture::new();
    let now = Instant::now();
    let lease = controlling(&f, now);
    assert_eq!(
        f.authority
            .audio_admitted(SESSION, LEASE_DEVICE, &lease, now),
        Ok(())
    );
    // The operator withdraws station control at the radio.
    f.authority.permit_station(LEASE_DEVICE, false).unwrap();
    assert_eq!(
        f.authority
            .audio_admitted(SESSION, LEASE_DEVICE, &lease, now),
        Err("notController"),
        "audio outlived the permission it was given under"
    );
}

#[test]
fn a_malformed_identity_is_refused_before_anything_is_looked_up() {
    let f = Fixture::new();
    let now = Instant::now();
    let lease = controlling(&f, now);
    for (session, device, lease_id) in [
        ("not-a-session", LEASE_DEVICE, lease.as_str()),
        (SESSION, "not-a-device", lease.as_str()),
        (SESSION, LEASE_DEVICE, "not-a-lease"),
    ] {
        assert_eq!(
            f.authority.audio_admitted(session, device, lease_id, now),
            Err("invalidRequest")
        );
    }
}

#[test]
#[cfg(feature = "radio")]
fn the_listen_capability_is_advertised_under_station_control_and_not_under_logging() {
    let f = Fixture::new();
    let now = Instant::now();
    // Logging only: the browser is told about its logging capabilities and nothing about
    // audio, so a browser that knows the lane still never offers a listen control.
    f.authority.permit(DEVICE, true).unwrap();
    let logging = control_state_version(&f, now, 4);
    let capabilities = logging["controls"]["capabilities"].as_array().unwrap();
    assert!(
        capabilities.iter().any(|c| c == "qsoLogging"),
        "fixture: logging was not granted"
    );
    assert!(
        !capabilities.iter().any(|c| c == "audioListen"),
        "a logging-only browser was offered the listen control"
    );

    // With station control, the hint appears — the control that makes the absence above
    // mean something.
    let f = Fixture::new();
    let now = Instant::now();
    acquire_controls_version(&f, now, 4);
    let controlling = control_state_version(&f, now, 4);
    assert!(controlling["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c == "audioListen"));

    // And an older browser never sees it at all, because a v2/v3 station state carries no
    // v4 capability list: nothing is offered that the other end could not answer.
    let older = control_state_version(&f, now, 3);
    assert!(!older["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c == "audioListen"));
}
