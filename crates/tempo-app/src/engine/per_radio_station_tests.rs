//! The station fields that are each radio's own: its PTT keying lines, keyers, devices, split,
//! scope and power caps (the CAT broker, JS8 API and TCI server stay the station's). A radio
//! switch brings the new radio's, a radio never takes another radio's keying lines, and a settings
//! file from before they were per radio keeps every radio's transmit power exactly where it was.

use super::*;
use crate::settings::{CwKeyerBackend, OperatingMode, Settings, SplitMode};

/// Radio 0, with this cable, keyer, ports and caps, set through its own (flat) form.
fn radio_a(e: &mut Engine) {
    let s = &mut e.settings;
    s.ptt_method = "rts".into();
    s.cat_rts_keys_ptt = true;
    s.cat_ptt_line_state = "low".into();
    s.cat_rts_state = "high".into();
    s.cat_dtr_state = "untouched".into();
    s.cat_serial_handshake = "none".into();
    s.cw_keyer = CwKeyerBackend::WinKeyer;
    s.winkeyer_port = "COM6".into();
    s.cw_key_port = "COM7".into();
    s.rtty_fsk_port = "COM8".into();
    s.rtty_backend = "fsk".into();
    s.monitor_device = "Headphones A".into();
    s.voice_mic_device = "Mic A".into();
    s.split_mode = SplitMode::Rig;
    s.scope_source = "sdrconnect".into();
    s.cat_broker_port = 4600;
    s.js8_api_tcp_port = 2443;
    s.max_power_digital = Some(0.8);
    s.max_power_phone = Some(0.9);
    s.tune_power_pct = Some(20);
    s.sstv_tx_power_pct = Some(40);
    s.sync_active_from_flat();
}

/// What a fresh radio's cable says: nothing of radio A's.
fn assert_no_cable_of_a(s: &Settings, why: &str) {
    let d = Settings::default();
    assert_eq!(s.ptt_method, "vox", "{why}: PTT method");
    assert!(!s.cat_rts_keys_ptt, "{why}: RTS keys PTT");
    assert_eq!(
        s.cat_ptt_line_state, d.cat_ptt_line_state,
        "{why}: keying line idle state"
    );
    assert_eq!(s.cat_rts_state, d.cat_rts_state, "{why}: RTS");
    assert_eq!(s.cat_dtr_state, d.cat_dtr_state, "{why}: DTR");
    assert_eq!(
        s.cat_serial_handshake, d.cat_serial_handshake,
        "{why}: handshake"
    );
    assert_eq!(s.cw_keyer, d.cw_keyer, "{why}: CW keyer");
    assert_eq!(s.winkeyer_port, "", "{why}: WinKeyer port");
    assert_eq!(s.cw_key_port, "", "{why}: CW key port");
    assert_eq!(s.rtty_fsk_port, "", "{why}: FSK port");
    assert_eq!(s.rtty_backend, d.rtty_backend, "{why}: RTTY backend");
}

/// ★ A SWITCH BRINGS THE RADIO'S OWN KEYING LINES, KEYERS AND CAPS. Radio A keys on RTS with its
/// WinKeyer on COM6 and its digital cap at 80 %; radio B is set up through its own form with other
/// values. Each switch puts the radio's own in force, both ways. (Flat-only,
/// these stayed radio A's on radio B, and B's edits overwrote A's: that is how this test was first
/// seen red.)
#[test]
fn a_switch_brings_the_radios_own_keying_lines_ports_and_caps() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.ensure_radio_profiles();
    radio_a(&mut e);
    let b = e.add_radio();
    e.set_active_radio(b);
    assert_no_cable_of_a(&e.settings, "radio B on its first switch");
    // B's own, through its own form.
    e.settings.ptt_method = "cat".into();
    e.settings.winkeyer_port = "COM9".into();
    // The station's, moved while B is in use.
    e.settings.cat_broker_port = 4700;
    e.settings.js8_api_tcp_port = 2444;
    e.settings.monitor_device = "Headphones B".into();
    e.settings.split_mode = SplitMode::FakeIt;
    e.settings.max_power_digital = Some(0.3);
    e.settings.tune_power_pct = Some(10);
    e.settings.sync_active_from_flat();

    e.set_active_radio(0);
    let s = &e.settings;
    assert_eq!(
        (
            s.ptt_method.as_str(),
            s.cat_rts_keys_ptt,
            s.cat_ptt_line_state.as_str()
        ),
        ("rts", true, "low"),
        "radio A's keying lines are back"
    );
    assert_eq!(s.winkeyer_port, "COM6");
    assert_eq!(
        (s.cat_broker_port, s.js8_api_tcp_port),
        (4700, 2444),
        "the CAT broker and JS8 API stay the station's: a switch does not move them"
    );
    assert_eq!(s.monitor_device, "Headphones A");
    assert_eq!(s.split_mode, SplitMode::Rig);
    assert_eq!(s.scope_source, "sdrconnect");
    assert_eq!(
        (
            s.max_power_digital,
            s.max_power_phone,
            s.tune_power_pct,
            s.sstv_tx_power_pct
        ),
        (Some(0.8), Some(0.9), Some(20), Some(40)),
        "radio A's caps"
    );

    e.set_active_radio(b);
    let s = &e.settings;
    assert_eq!((s.ptt_method.as_str(), s.cat_rts_keys_ptt), ("cat", false));
    assert_eq!(s.winkeyer_port, "COM9");
    assert_eq!(
        (s.cat_broker_port, s.js8_api_tcp_port),
        (4700, 2444),
        "still the station's"
    );
    assert_eq!(s.monitor_device, "Headphones B");
    assert_eq!(s.split_mode, SplitMode::FakeIt);
    assert_eq!(
        (s.max_power_digital, s.tune_power_pct),
        (Some(0.3), Some(10))
    );
}

/// A settings file written before these fields were per radio: two radios with none of them,
/// radio 0 in use, and the flat fields saying radio 0's cable keys RTS with its caps set.
fn old_file() -> Settings {
    let dir = std::env::temp_dir().join(format!(
        "nexus-per-radio-old-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let json = serde_json::json!({
        "mycall": "KD9TAW",
        "mygrid": "EN52",
        "activeRadio": 0,
        "pttMethod": "rts",
        "catRtsKeysPtt": true,
        "catPttLineState": "low",
        "catRtsState": "high",
        "winkeyerPort": "COM6",
        "catBrokerPtt": false,
        "catBrokerPort": 4600,
        "splitMode": "rig",
        "maxPowerPhone": 0.5,
        "maxPowerCw": 0.6,
        "maxPowerDigital": 0.2,
        "maxPowerAm": 0.25,
        "tunePowerPct": 30,
        "sstvTxPowerPct": 40,
        "ratedWatts": 100,
        "radios": [
            { "id": 0, "name": "FTdx10", "pttMethod": "rts", "ratedWatts": 100 },
            { "id": 1, "name": "IC-705", "pttMethod": "cat", "ratedWatts": 10 }
        ]
    });
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    let s = Settings::load(&path);
    let _ = std::fs::remove_dir_all(&dir);
    s
}

/// The settings with radio `id` in use, as a switch leaves them.
fn on_radio(mut s: Settings, id: u32) -> Settings {
    s.sync_active_from_flat();
    s.active_radio = id;
    s.sync_flat_from_active();
    s
}

/// ★ RADIO B NEVER TAKES RADIO A'S PTT. Neither a radio added to the roster nor one in a file from
/// before the fields were per radio: radio A's cable keys RTS (and holds it low, and lets Nexus
/// drop the handshake), and on radio B that would key B's transmitter from the moment its port
/// opened, or never key it.
#[test]
fn radio_b_never_inherits_radio_as_ptt_method_or_keying_lines() {
    // Added to the roster while radio A is set up.
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.ensure_radio_profiles();
    radio_a(&mut e);
    let b = e.add_radio();
    e.set_active_radio(b);
    assert_no_cable_of_a(&e.settings, "a radio added after radio A");

    // In a file from before the fields were per radio.
    let s = on_radio(old_file(), 1);
    assert_eq!(s.ptt_method, "cat", "radio 1 keeps its own PTT method");
    assert!(
        !s.cat_rts_keys_ptt,
        "radio 1 does not take radio 0's RTS keying"
    );
    assert_eq!(
        s.cat_ptt_line_state, "auto",
        "nor its keying line's idle state"
    );
    assert_eq!(
        s.cat_rts_state,
        Settings::default().cat_rts_state,
        "nor its RTS"
    );
    assert_eq!(s.winkeyer_port, "", "nor its WinKeyer port");
    // What is not its cable it takes from the station, as it had until now (and the CAT broker,
    // the station's, stays on its port).
    assert_eq!(
        (s.cat_broker_port, s.split_mode),
        (4600, SplitMode::Rig),
        "the station's broker port and split, as before"
    );

    // …while radio 0, the one the flat fields always described, keeps all of them.
    let s = on_radio(old_file(), 0);
    assert_eq!(
        (
            s.ptt_method.as_str(),
            s.cat_rts_keys_ptt,
            s.cat_ptt_line_state.as_str()
        ),
        ("rts", true, "low")
    );
    assert_eq!(
        (s.cat_rts_state.as_str(), s.winkeyer_port.as_str()),
        ("high", "COM6")
    );
}

/// The power a cap allows on a radio, in watts: the ceiling (a fraction of the radio's full
/// power) times the radio's rating.
fn watts(s: &Settings, mode: OperatingMode) -> f32 {
    let mut s = s.clone();
    s.operating_mode = mode;
    s.rf_power_ceiling() * s.rated_watts as f32
}

/// ★ THE MIGRATION KEEPS EVERY RADIO'S TRANSMIT POWER WHERE IT WAS. Before, one set of caps applied
/// to whichever radio was in use: 50 % phone, 60 % CW, 20 % digital, AM at a quarter, tune at
/// 30 % and SSTV at 40 %, of the 100 W FTdx10 and of the 10 W IC-705 alike. After the migration
/// each radio's own caps give the same watts on each radio.
#[test]
fn the_migration_keeps_each_radios_effective_watts() {
    for (id, rated) in [(0u32, 100.0f32), (1, 10.0)] {
        let s = on_radio(old_file(), id);
        assert_eq!(s.rated_watts as f32, rated, "premise: radio {id}'s rating");
        for (mode, before) in [
            (OperatingMode::Phone, 0.5),
            (OperatingMode::Cw, 0.6),
            (OperatingMode::Digital, 0.2),
            (OperatingMode::Rtty, 0.2),
        ] {
            assert_eq!(
                watts(&s, mode),
                before * rated,
                "radio {id}, {mode:?}: the watts its cap allowed before"
            );
        }
        let mut am = s.clone();
        am.operating_mode = OperatingMode::Phone;
        assert_eq!(
            am.rf_power_ceiling_am() * rated,
            0.25 * rated,
            "radio {id}, AM"
        );
        assert_eq!(
            (s.tune_power_pct, s.sstv_tx_power_pct),
            (Some(30), Some(40)),
            "radio {id}: tune and SSTV power"
        );
    }
}

/// ★ A RADIO WITH NO CAPS OF ITS OWN IS NEVER UNCAPPED BY IT. A profile from before the caps were
/// per radio, and a radio added to the roster, take the caps the station has now — the ones that
/// applied to them until now — and never a default of "no cap", which would be full power. The
/// switch that lets a program on the CAT broker key PTT is the station's: off stays off on them.
#[test]
fn a_radio_without_caps_of_its_own_takes_the_stricter_never_uncapped() {
    let s = on_radio(old_file(), 1);
    assert_eq!(
        (
            s.max_power_phone,
            s.max_power_cw,
            s.max_power_digital,
            s.max_power_am
        ),
        (Some(0.5), Some(0.6), Some(0.2), Some(0.25)),
        "a profile with no caps takes the station's, not none"
    );
    let mut digital = s.clone();
    digital.operating_mode = OperatingMode::Digital;
    assert_eq!(
        digital.rf_power_ceiling(),
        0.2,
        "and its ceiling is that cap, not 1.0"
    );
    assert!(!s.cat_broker_ptt, "the broker's PTT stays off");

    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.ensure_radio_profiles();
    e.settings.max_power_digital = Some(0.25);
    e.settings.cat_broker_ptt = false;
    e.settings.sync_active_from_flat();
    let b = e.add_radio();
    e.set_active_radio(b);
    assert_eq!(
        e.settings.max_power_digital,
        Some(0.25),
        "an added radio is capped too"
    );
    assert!(!e.settings.cat_broker_ptt, "and its broker PTT stays off");
}

/// ★ THE TRANSMIT CEILING IS THE RADIO IN USE'S. Radio A's digital cap is 80 %, radio B's 30 %:
/// the `set_rf_power` chokepoint holds each radio to its own, and a level commanded on radio A is
/// brought down to radio B's cap on the loop's next tick after a switch, before anything is keyed
/// on B, even with nothing read back from B yet.
#[test]
fn the_tx_power_ceiling_is_the_bound_radios_own_cap() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.ensure_radio_profiles();
    e.settings.max_power_digital = Some(0.8);
    e.settings.sync_active_from_flat();
    let b = e.add_radio();
    e.set_active_radio(b);
    e.settings.max_power_digital = Some(0.3);
    e.settings.sync_active_from_flat();
    e.set_operating_mode("digital", false);

    e.set_rf_power(1.0);
    assert_eq!(e.rf_power(), Some(0.3), "radio B: its own cap");
    e.set_active_radio(0);
    e.set_rf_power(1.0);
    assert_eq!(e.rf_power(), Some(0.8), "radio A: its own cap");

    // A's level, then a switch to B with nothing read back from B.
    e.rig_rf_power = None;
    e.set_active_radio(b);
    assert_eq!(
        e.rf_power_to_command(),
        Some((0.3, true)),
        "radio A's 80 % is never commanded to radio B"
    );
}

/// ★ A KEYER PICKED IN THE CW COCKPIT SURVIVES THE NEXT SAVE. The cockpit's keyer pick writes the
/// radio in use's keyer; the Settings form reads the radio's own (the read re-mirrors the active
/// profile) and saves it back with whatever else it changed. Written to the flat field alone, the
/// pick was in force until that save put the radio's old keyer back.
#[test]
fn a_keyer_picked_in_the_cockpit_survives_the_next_save() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.ensure_radio_profiles();
    e.settings.sync_active_from_flat();
    assert_eq!(e.settings.cw_keyer, CwKeyerBackend::Cat, "premise: CAT");
    e.set_cw_keyer("winkeyer", 0.0);
    // The form, as a settings read hands it over, saved with another change.
    let mut form = e.settings().clone();
    form.sync_flat_from_active();
    form.cw_wpm = 22;
    e.apply_settings(form);
    assert_eq!(e.settings.cw_wpm, 22, "the save landed");
    assert_eq!(
        e.settings.cw_keyer,
        CwKeyerBackend::WinKeyer,
        "the cockpit's keyer pick is still in force"
    );
    assert_eq!(
        e.settings.active_profile().map(|p| p.cw_keyer),
        Some(CwKeyerBackend::WinKeyer),
        "and is the radio's own"
    );
}
