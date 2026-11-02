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

/// ⛔ Cross-port, direction 1: the ACTIVE radio keys PTT on RTS of a dedicated port, and that port
/// is a MONITORED radio's CAT port. The monitor's daemon would raise RTS under flow control and key
/// the active radio's transmitter: the monitor is refused.
#[test]
fn a_monitor_whose_cat_port_is_the_active_radios_rts_ptt_port_is_refused() {
    let mut s = station("cat");
    s.radios[0].ptt_method = "rts".to_string();
    s.radios[0].ptt_serial_port = "COM99".to_string();
    s.ptt_method = "rts".to_string();
    s.ptt_serial_port = "COM99".to_string();
    assert_refused(&monitored(&s), "monitor on the active radio's RTS PTT port");
    // Control: the active radio's PTT port elsewhere leaves the monitor alone.
    s.radios[0].ptt_serial_port = "COM7".to_string();
    s.ptt_serial_port = "COM7".to_string();
    assert!(!hamlib_launch_refused(&monitored(&s)));
}

/// ⛔ Cross-port, direction 2: a MONITORED radio keys PTT on RTS of a dedicated port, and that port
/// is the ACTIVE radio's CAT port. The active radio's daemon would key the monitored radio: the
/// active launch is refused.
#[test]
fn the_active_radio_whose_cat_port_is_a_monitored_radios_rts_ptt_port_is_refused() {
    let mut s = station("rts");
    s.radios[1].ptt_serial_port = "COM98".to_string();
    s.serial_port = "COM98".to_string();
    s.rig_model = 2028;
    s.ptt_method = "cat".to_string();
    assert_refused(
        &Transport::from_settings(&s),
        "active on a monitored radio's RTS PTT port",
    );
    // Control: a DISABLED radio keys nothing.
    s.radios[1].enabled = false;
    assert!(!hamlib_launch_refused(&Transport::from_settings(&s)));
}

/// No spelling of the same port escapes the fact: a CW keyline stored as the Win32 device path,
/// another radio's PTT port in another case, the active radio's own PTT port as an alias of its CAT
/// port, and a mac tty.*/cu.* pair — read the same at the startup seed (healed port) and live
/// (stored port).
#[test]
fn no_spelling_of_the_same_port_escapes_the_rts_fact() {
    let mut s = station("cat");
    s.cw_keyer = tempo_app::settings::CwKeyerBackend::Serial;
    s.cw_key_line = "RTS".to_string();
    s.cw_key_port = "\\\\.\\com99".to_string();
    assert_refused(&monitored(&s), "CW keyline as \\\\.\\com99");

    let mut s = station("cat");
    s.radios[0].ptt_method = "rts".to_string();
    s.radios[0].ptt_serial_port = "com99".to_string();
    assert_refused(&monitored(&s), "another radio's PTT port in another case");

    let mut s = station("cat");
    s.serial_port = "COM98".to_string();
    s.rig_model = 2028;
    s.ptt_method = "rts".to_string();
    s.ptt_serial_port = "\\\\.\\COM98".to_string();
    assert_refused(
        &Transport::from_settings(&s),
        "own RTS PTT on an alias of the CAT port",
    );

    // The mac pair: the seed compares the healed transport port, the live path the stored one.
    let mut s = station("cat");
    s.cw_keyer = tempo_app::settings::CwKeyerBackend::Serial;
    s.cw_key_line = "rts".to_string();
    s.cw_key_port = "/dev/tty.usbserial-1".to_string();
    assert!(
        settings_key_rts_on(&s, "/dev/cu.usbserial-1"),
        "seed: healed cu.*"
    );
    assert!(
        settings_key_rts_on(&s, "/dev/tty.usbserial-1"),
        "live: stored tty.*"
    );
    let mut seeded = Transport::from_settings(&s);
    seeded.serial_port = "/dev/cu.usbserial-1".to_string();
    seeded.rts_keys_cat_port = false;
    seed_line_declarations(&mut seeded, &s);
    assert!(seeded.rts_keys_cat_port, "the startup seed sees it too");
}

/// A refused monitor's pill says why: the monitor open records the refusal, and the snapshot
/// carries it on that radio's summary — never on the active radio's, whose CAT line says it.
#[test]
fn a_refused_monitor_carries_its_reason_to_the_switcher() {
    let s = station("rts");
    let t = monitored(&s);
    let (rig, _, ok) = open_monitor(&t);
    let mut e = Engine::with_settings(s);
    e.observe_radio_cat(1, ok);
    e.observe_radio_cat_refused(1, !rig.has_control() && hamlib_launch_refused(&t));
    let radios = e.snapshot().radios;
    let summary = |id: u32| {
        radios
            .iter()
            .find(|r| r.id == id)
            .expect("radio in the switcher")
    };
    assert!(
        summary(1).cat_refused,
        "the monitored radio's pill gives the reason"
    );
    assert_eq!(summary(1).cat_ok, Some(false));
    assert!(!summary(0).cat_refused);
}

/// A radio with no CAT configured is never reported refused: its status says "no CAT".
#[test]
fn a_radio_with_no_cat_is_never_reported_refused() {
    let s = station("rts");
    let mut t = monitored(&s);
    assert!(
        hamlib_launch_refused(&t),
        "the premise: with a rig model it is refused"
    );
    t.rig_model = 0;
    assert!(!hamlib_launch_refused(&t));
}
