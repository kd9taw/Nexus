//! A HARDWARE SERIAL HANDSHAKE NEVER OPENS A PORT WHERE RTS KEYS THE RADIO — monitors included.
//!
//! Serial handshake is a station-wide setting, so every monitored radio's rigctld gets it. Under
//! RTS/CTS flow control the serial driver asserts RTS from the moment the port opens (on Windows
//! it also refuses Hamlib's un-key and reports success), so a radio whose PTT is RTS on its CAT
//! port is keyed for as long as the port is open. A monitor's daemon is read-only and told no
//! `-P`, so nothing would ever un-key it. The launch is refused instead, decided from the RADIO's
//! keying, never from the `-P` its daemon is given.

use super::*;

/// Radio 0 active (CAT PTT), radio 1 monitored: a TS-480 on COM99 keying with `ptt` — under
/// the station's Hardware serial handshake.
fn station(monitored_ptt: &str) -> Settings {
    let radio = |id: u32, ptt: &str, port: &str| tempo_app::settings::RadioProfile {
        id,
        enabled: true,
        rig_model: 2028,
        ptt_method: ptt.to_string(),
        serial_port: port.to_string(),
        rigctld_port: 4560 + id as u16,
        ..tempo_app::settings::RadioProfile::default()
    };
    Settings {
        radios: vec![radio(0, "cat", "COM98"), radio(1, monitored_ptt, "COM99")],
        active_radio: 0,
        cat_serial_handshake: "hardware".to_string(),
        ..Settings::default()
    }
}

/// The monitor's transport for radio 1, built exactly as the monitor thread builds it.
fn monitored(s: &Settings) -> Transport {
    let (_, want) = monitor_want(s, None, None);
    want.into_iter()
        .find(|(id, _)| *id == 1)
        .map(|(_, t)| t)
        .expect("radio 1 is monitored")
}

fn assert_refused(t: &Transport, what: &str) {
    assert!(hamlib_launch_refused(t), "{what}: the predicate");
    match spawn_cat_daemon(t, t.serial_port.as_str(), false, None) {
        Err(e) => assert!(
            crate::rigctld_proc::is_handshake_refusal(&e),
            "{what}: refused by type: {e}"
        ),
        Ok(_) => panic!("{what}: a daemon was launched on a port flow control would key"),
    }
}

/// ⛔ The monitor of a radio keying on RTS of its own CAT port is refused and launches nothing:
/// no daemon, no control channel — so a radio switch can never adopt it either (the handoff
/// adopts only a connection with `has_control()`, and never one for a radio that keys on its CAT
/// port: `keys_on_the_cat_port(&want_active)`).
#[test]
fn a_monitored_rts_keyed_radio_under_a_hardware_handshake_is_refused_and_never_launches() {
    let s = station("rts");
    let t = monitored(&s);
    assert_eq!(
        t.control_lines.handshake,
        crate::rigctld_proc::Handshake::Hardware,
        "the premise: the station's handshake reaches the monitor"
    );
    assert_refused(&t, "monitored RTS PTT");
    let (rig, daemon, ok) = open_monitor(&t);
    assert!(daemon.is_none(), "no daemon for a monitor it would key");
    assert!(!rig.has_control(), "nothing a switch could adopt");
    assert_eq!(ok, Some(false));
}

/// The negative control: the same monitored radio keying by CAT has nothing on RTS — it is not
/// refused, and its launch leaves RTS to the flow control (the #432 fix itself).
#[test]
fn a_monitored_cat_keyed_radio_under_a_hardware_handshake_is_not_refused() {
    let t = monitored(&station("cat"));
    assert!(!hamlib_launch_refused(&t));
    assert!(!keying_facts(&t, None).rts_keys);
}

/// ⛔ A CW serial keyline on RTS of the CAT port keys the KEY jack the moment flow control raises
/// RTS: refused on the active radio and on a monitor. On DTR, or on another port, it is not.
#[test]
fn a_cw_keyline_on_the_cat_ports_rts_is_refused_under_a_hardware_handshake() {
    let keyline = |line: &str, port: &str| {
        let mut s = station("cat");
        s.cw_keyer = tempo_app::settings::CwKeyerBackend::Serial;
        s.cw_key_line = line.to_string();
        s.cw_key_port = port.to_string();
        s
    };
    let s = keyline("rts", "com99");
    assert_refused(&monitored(&s), "monitor, CW keyline on its RTS");
    let mut active = s.clone();
    active.serial_port = "COM99".to_string();
    active.rig_model = 2028;
    active.ptt_method = "cat".to_string();
    assert_refused(
        &Transport::from_settings(&active),
        "active, CW keyline on its RTS",
    );
    // Controls: the keyline on DTR, or on a separate port, keys nothing on this port's RTS.
    assert!(!hamlib_launch_refused(&monitored(&keyline("dtr", "COM99"))));
    assert!(!hamlib_launch_refused(&monitored(&keyline("rts", "COM7"))));
}

/// The station-wide "Interface keys RTS on the CAT port" declaration reaches a monitor's refusal
/// too — without becoming the monitor's `cat_rts_keys_ptt`, which would move its Auto handshake.
#[test]
fn the_rts_keys_declaration_refuses_a_monitor_without_moving_its_auto_handshake() {
    let mut s = station("cat");
    s.cat_rts_keys_ptt = true;
    let t = monitored(&s);
    assert_refused(&t, "monitor under the declaration");
    assert!(
        !t.cat_rts_keys_ptt,
        "the Auto override's input is unchanged"
    );
    assert!(!keying_facts(&t, None).rts_declared);
}
