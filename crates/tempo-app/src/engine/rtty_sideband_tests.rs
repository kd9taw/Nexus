//! AFSK RTTY on either sideband keeps the standard on-air sense: mark HIGH on RF, space 170 Hz
//! below it. On LSB the mark is the lower audio tone (RF = dial − audio); on USB the audio maps
//! to RF the other way up (RF = dial + audio), so the tones swap. Reverse inverts whichever sense
//! that is, for an upside-down station.

use super::*;

fn station(side: &str, reverse: bool) -> Engine {
    let mut e = Engine::new("K1ABC", "EN50", 0);
    e.set_license_class("extra");
    let mut s = e.settings().clone();
    s.rtty_afsk_sideband = side.into();
    s.rtty_reverse = reverse;
    e.apply_settings(s);
    e.set_operating_mode("rtty", false);
    e
}

/// (mark, space) on RF, in Hz from the dial: the tone pair both TX sites in the radio loop build
/// (`tone_pair(rtty_center_hz, rtty_shift_hz, rtty_reverse)`), placed on the side of the dial the
/// commanded mode puts the audio.
fn emitted(e: &Engine) -> (f32, f32) {
    let (mark, space) = tempo_core::rtty::tone_pair(
        e.rtty_center_hz(),
        e.rtty_shift_hz() as f32,
        e.rtty_reverse(),
    );
    match e.rig_mode_effective().as_str() {
        "PKTUSB" => (mark, space),
        "PKTLSB" => (-mark, -space),
        other => panic!("AFSK RTTY commanded {other}"),
    }
}

#[test]
fn afsk_puts_mark_high_on_rf_on_either_sideband() {
    for (side, mode, rf) in [
        ("lsb", "PKTLSB", (-2125.0, -2295.0)),
        ("usb", "PKTUSB", (2295.0, 2125.0)),
    ] {
        let e = station(side, false);
        assert_eq!(e.rig_mode_effective(), mode, "{side}");
        assert_eq!(emitted(&e), rf, "{side}: (mark, space) on RF from the dial");
        assert!(rf.0 > rf.1, "{side}: mark is the higher RF");
        // The waterfall cursors are the same pair, and the privilege gate judges the same side.
        let st = e.rtty_state();
        assert_eq!(
            (st.mark_hz, st.space_hz),
            (rf.0.abs(), rf.1.abs()),
            "{side} cursors"
        );
        assert_eq!(
            e.rtty_afsk_usb_side(),
            Some(side == "usb"),
            "{side} gate side"
        );
    }
}

/// LSB + Reverse sends the audio pair USB sends, and USB + Reverse sends LSB's: Reverse inverts
/// the on-air sense on both sides, so mark lands LOW on RF.
#[test]
fn reverse_composes_with_the_sideband() {
    let audio = |e: &Engine| {
        tempo_core::rtty::tone_pair(
            e.rtty_center_hz(),
            e.rtty_shift_hz() as f32,
            e.rtty_reverse(),
        )
    };
    assert_eq!(audio(&station("lsb", true)), audio(&station("usb", false)));
    assert_eq!(audio(&station("usb", true)), audio(&station("lsb", false)));
    assert_eq!(audio(&station("lsb", true)), (2295.0, 2125.0));
    assert_eq!(audio(&station("usb", true)), (2125.0, 2295.0));
    for side in ["lsb", "usb"] {
        let (mark, space) = emitted(&station(side, true));
        assert!(
            mark < space,
            "{side} + Reverse: mark LOW on RF, got {mark} vs {space}"
        );
    }
}

/// True FSK ignores the AFSK sideband: the rig's own RTTY mode, and the operator's Reverse alone.
#[test]
fn fsk_ignores_the_afsk_sideband() {
    for side in ["lsb", "usb"] {
        let mut e = station(side, false);
        let mut s = e.settings().clone();
        s.rtty_backend = "fsk".into();
        e.apply_settings(s);
        assert_eq!(e.rig_mode_effective(), "RTTY", "{side}");
        assert!(!e.rtty_reverse(), "{side}");
    }
}

/// A station with a saved tone pair: the low tone in Settings, nothing netted.
fn saved_pair(low: u32) -> Engine {
    let mut e = station("lsb", false);
    let mut s = e.settings().clone();
    s.rtty_low_tone_hz = low;
    e.apply_settings(s);
    e
}

/// THE SAVED TONE PAIR IS WHERE RTTY SITS: the decoder's cursors, the AFSK transmitter's tones
/// and the privilege gate all read the one centre, and un-netted it is the saved pair.
#[test]
fn a_saved_1275_pair_is_where_the_decoder_and_the_transmitter_sit() {
    // CONTROL: nothing saved is the 2125/2295 standard, as before the setting existed.
    let e = station("lsb", false);
    let st = e.rtty_state();
    assert_eq!((st.mark_hz, st.space_hz), (2125.0, 2295.0));

    let e = saved_pair(1275);
    assert_eq!(e.rtty_center_hz(), 1360.0);
    let st = e.rtty_state();
    assert_eq!((st.mark_hz, st.space_hz), (1275.0, 1445.0), "the cursors");
    assert_eq!(
        emitted(&e),
        (-1275.0, -1445.0),
        "the AFSK tones, below the dial on LSB"
    );
    assert_eq!(
        e.rtty_tx_mark_hz(),
        1275.0,
        "what the privilege gate judges"
    );
    // On USB the same saved pair swaps, so mark stays high on RF — the low tone is not "mark".
    let mut s = e.settings().clone();
    s.rtty_afsk_sideband = "usb".into();
    let mut u = e;
    u.apply_settings(s);
    assert_eq!(
        emitted(&u),
        (1445.0, 1275.0),
        "USB: the same two tones, mark the upper"
    );
}

/// The privilege gate judges the SAVED pair, not 2125: a dial where the standard pair sits just
/// inside the top of the 20 m data segment puts a 1275 pair 850 Hz higher on RF, outside it.
#[test]
fn the_privilege_gate_judges_the_saved_pair() {
    let dial = 14.1514; // LSB: the 2125 pair tops out at 14.149275, the 1275 pair at 14.150125
    assert!(
        station("lsb", false).rtty_emission_ok(dial),
        "control: 2125 is inside"
    );
    assert!(
        !saved_pair(1275).rtty_emission_ok(dial),
        "a 1275 pair at this dial lands above 14.150 and must be refused"
    );
}

/// A waterfall net still moves RTTY for the session; a save that MOVES the saved pair re-homes
/// onto it, and a save that leaves the pair alone (any other setting) keeps the net.
#[test]
fn a_net_holds_until_a_save_moves_the_saved_pair() {
    let mut e = saved_pair(1275);
    e.rtty_net(1800.0).expect("unkeyed: the net is taken");
    assert_eq!(e.rtty_center_hz(), 1800.0);
    // An unrelated save keeps the net.
    let mut s = e.settings().clone();
    s.rtty_shift_hz = 170;
    s.cw_wpm = s.cw_wpm.saturating_add(1);
    e.apply_settings(s);
    assert_eq!(
        e.rtty_center_hz(),
        1800.0,
        "an unrelated save dropped the net"
    );
    // Moving the pair re-homes onto it.
    let mut s = e.settings().clone();
    s.rtty_low_tone_hz = 2125;
    e.apply_settings(s);
    assert_eq!(
        e.rtty_center_hz(),
        2210.0,
        "the new saved pair, not the old net"
    );
}

/// A saved low tone outside 300–2700 Hz (a cleared box, a hand-edited file, a Remote write) is
/// not used: RTTY sits on the 2125/2295 standard instead of a pair at the passband's edge.
#[test]
fn an_out_of_range_saved_tone_falls_back_to_the_standard_pair() {
    for bad in [0, 10, 2701, 9000] {
        let e = saved_pair(bad);
        assert_eq!(e.rtty_center_hz(), 2210.0, "stored {bad}");
        assert_eq!(
            e.rtty_tx_mark_hz(),
            2125.0,
            "what the gate judges, stored {bad}"
        );
    }
    // CONTROL: the range edges are used as saved.
    assert_eq!(saved_pair(300).rtty_center_hz(), 385.0);
    assert_eq!(saved_pair(2700).rtty_center_hz(), 2785.0);
}
