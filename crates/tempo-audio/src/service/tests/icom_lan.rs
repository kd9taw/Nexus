//! The Icom network connection, from the radio loop's side: which daemon a transport gets, that
//! nothing falls back from it to Hamlib or the serial CI-V daemon, that it keys on no CAT port,
//! that its scope needs no baud, and when the loop tries a lost radio again. Made-up addresses only
//! (the documentation range).
use super::*;
use crate::civ::commands::IcomModel;

/// An IC-7760 on the Icom network connection, as Settings stores one.
fn lan(model: u32) -> Transport {
    let mut t = cat_transport(4534, Some(4532));
    t.rig_model = model;
    t.rig_conn = "icomlan".into();
    t.icom_lan_host = "192.0.2.10".into();
    t.icom_lan_user = "test-user".into();
    t.radio_id = 3;
    t
}

/// ⭐ THE CONNECTION IS THE OPT-IN, FOR THE SIX ONLY: the target exists for each network Icom with
/// an IPv4 address, and for nothing else: not the IC-7300 (no network port), not a serial pick
/// with an address left over, not an address that is not IPv4.
#[test]
fn the_network_connection_targets_the_six_network_icoms_with_an_address() {
    for (model, icom) in [
        (3078, IcomModel::Ic7610),
        (3081, IcomModel::Ic9700),
        (3085, IcomModel::Ic705),
        (3090, IcomModel::Ic905),
        (3092, IcomModel::Ic7760),
        (3094, IcomModel::Ic7300Mk2),
    ] {
        let target = lan(model).icom_lan_target().expect("a target");
        assert_eq!(target.model, icom);
        assert_eq!(target.host, std::net::Ipv4Addr::new(192, 0, 2, 10));
        assert_eq!(target.control_port, 50001);
        assert_eq!(target.profile_id, 3, "the keychain entry is the profile's");
    }
    assert_eq!(
        lan(3073).icom_lan_target(),
        None,
        "the IC-7300 has no network port"
    );
    let mut serial = lan(3092);
    serial.rig_conn = "serial".into();
    assert_eq!(
        serial.icom_lan_target(),
        None,
        "an address left over is not the connection"
    );
    let mut bad = lan(3092);
    bad.icom_lan_host = "radio.local".into();
    assert_eq!(
        bad.icom_lan_target(),
        None,
        "IPv4 only: the protocol's ids are IPv4 addresses"
    );
    // The rule is the app's own, so Settings and the daemon cannot disagree.
    assert_eq!(
        tempo_app::settings::ICOM_LAN_RIGS
            .iter()
            .filter(|&&m| crate::rigmodels::icom_lan_model(m).is_some())
            .count(),
        6,
        "the app's six are the daemon's six"
    );
}

/// ⛔ NOTHING ON THIS CONNECTION OPENS A COM PORT: not the serial CI-V daemon (even with the
/// native toggle left on from a USB setup), not a keying line on the CAT port.
#[test]
fn the_network_connection_never_opens_the_serial_daemon_or_keys_a_cat_port() {
    let mut t = lan(3081);
    t.icom_native_cat = true;
    t.ptt_method = "rts".into();
    t.ptt_serial_port = t.serial_port.clone();
    assert_eq!(native_civ_model(&t), None);
    assert!(!keys_on_the_cat_port(&t));
    assert_eq!(cat_port_ptt_line(&t), None);
    // The control: the same profile on its USB serial port does open the serial daemon.
    let mut usb = t.clone();
    usb.rig_conn = "serial".into();
    usb.ptt_method = "cat".into();
    assert_eq!(native_civ_model(&usb), Some(IcomModel::Ic9700));
}

/// ⛔ NO FALLBACK: an Icom network connection that cannot serve this radio says why and starts
/// nothing else, rather than launching Hamlib at the serial port the profile last had.
#[test]
fn a_network_connection_that_cannot_serve_says_why_and_starts_nothing_else() {
    let mut t = lan(3073); // the IC-7300: no network port
    t.serial_port = "/dev/ttyUSB0".into();
    // On its own thread with a bound: a fallback would launch Hamlib at that serial port, which
    // does not come back promptly, and that has to fail here rather than hang the suite.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(
            spawn_cat_daemon(&t, "/dev/ttyUSB0", false, None)
                .err()
                .map(|e| e.to_string()),
        );
    });
    let err = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("refused at once, with nothing launched")
        .expect("refused");
    assert!(
        err.starts_with("The Icom network connection needs an IC-7610"),
        "{err}"
    );
}

/// The radio's scope over the network needs no baud: the USB 115200 rule does not apply there.
#[test]
fn the_scope_streams_over_the_network_at_any_baud() {
    let mut t = lan(3092);
    t.baud = 9600;
    assert!(link_carries_the_scope(&t));
    // The control: the USB rule stands for a serial radio.
    t.rig_conn = "serial".into();
    assert!(!link_carries_the_scope(&t));
    t.baud = 115_200;
    assert!(link_carries_the_scope(&t));
}

/// An address edit restarts only a radio on the Icom network connection: on a serial radio the
/// fields are not part of the transport at all.
#[test]
fn the_network_fields_rebuild_only_the_network_connection() {
    let a = lan(3092);
    let mut b = a.clone();
    b.icom_lan_host = "192.0.2.11".into();
    assert!(b.rig_differs(&a), "a new address is a new radio");
    let mut b = a.clone();
    b.icom_lan_user = "other-user".into();
    assert!(b.rig_differs(&a), "a new user logs in again");
    let mut serial_a = a.clone();
    serial_a.rig_conn = "serial".into();
    let mut serial_b = serial_a.clone();
    serial_b.icom_lan_host = "192.0.2.11".into();
    assert!(
        !serial_b.rig_differs(&serial_a),
        "nothing to restart on a serial radio"
    );
}

/// When the loop tries a radio that failed to open: on the ladder's wait, never while the radio
/// waits for the operator, and on the ordinary backoff when the ladder has nothing to say.
#[test]
fn the_loop_tries_a_lost_network_radio_on_the_ladder() {
    use crate::icomlan::{now_ms, registry};
    use tempo_net::icom::reconnect::End;
    use tempo_net::icom::session::LossReason;
    let key = std::net::SocketAddrV4::new(std::net::Ipv4Addr::new(198, 51, 100, 7), 50001);
    assert_eq!(
        icom_lan_reopen_at(key, 1_000.0, 11_000.0),
        11_000.0,
        "nothing to say"
    );
    let claim = registry::claim(key, now_ms()).expect("free");
    claim.ended(&End::Lost(LossReason::LinkTimeout), now_ms(), "lost");
    let at = icom_lan_reopen_at(key, 1_000.0, 11_000.0);
    assert!(
        (1_000.0..=2_000.0).contains(&at),
        "one second of the ladder: {at}"
    );
    registry::operator_acted(key);
    let claim = registry::claim(key, now_ms()).expect("free again");
    claim.ended(
        &End::Lost(LossReason::PeerDisconnect),
        now_ms(),
        "another program took the radio",
    );
    assert_eq!(icom_lan_reopen_at(key, 1_000.0, 11_000.0), f64::INFINITY);
    registry::operator_acted(key);
    assert_eq!(icom_lan_reopen_at(key, 1_000.0, 11_000.0), 11_000.0);
}
