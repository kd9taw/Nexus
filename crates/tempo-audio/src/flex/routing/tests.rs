//! The ruling's rules as decisions, one by one, with no radio behind them. The same rules against
//! the simulator, through the daemon and its admission, are in `flex::tests`.

use super::*;

fn view(radio: bool, mode: &str) -> View {
    View {
        radio: Some(radio),
        tx_slice: Some(0),
        tx_mode: Some(mode.to_string()),
        other_feeder: false,
        dax_tx_stream: true,
    }
}

const DAX: Option<Step> = Some(Step::Write {
    dax: true,
    why: Why::FollowMode,
});
const MIC: Option<Step> = Some(Step::Write {
    dax: false,
    why: Why::FollowMode,
});

#[test]
fn the_mode_decides_the_source() {
    for m in ["DIGU", "DIGL", "RTTY", "DFM", "digu"] {
        assert_eq!(mode_class(m), ModeClass::Digital, "{m}");
        assert_eq!(wanted_source(Some(m), false), Some(true), "{m}");
    }
    for m in ["USB", "LSB", "AM", "SAM", "FM", "NFM", "DSB"] {
        assert_eq!(mode_class(m), ModeClass::Phone, "{m}");
        assert_eq!(
            wanted_source(Some(m), false),
            Some(false),
            "Phone at the shack: {m}"
        );
        assert_eq!(
            wanted_source(Some(m), true),
            Some(true),
            "the browser voice: {m}"
        );
    }
    for m in ["CW", "FDV", "NT", ""] {
        assert_eq!(mode_class(m), ModeClass::Other, "{m}");
        assert_eq!(wanted_source(Some(m), true), None, "left alone: {m}");
    }
    assert_eq!(wanted_source(None, false), None);
}

#[test]
fn a_digital_tx_slice_asks_for_dax_once_and_then_leaves_the_flag_alone() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
    assert_eq!(
        r.operator(),
        Some(false),
        "the operator's setting is what the radio had"
    );
    assert_eq!(
        r.written(true, Why::FollowMode, 0),
        Some(false),
        "kept: Nexus changed it"
    );
    // The radio echoes it: nothing more.
    assert_eq!(r.step(&view(true, "DIGU"), true, false, false, 0), None);
    // The operator flips it by hand: not fought while nothing they could change has changed.
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), None);
    // A change of mode is a new decision.
    assert_eq!(r.step(&view(false, "DIGL"), true, false, false, 0), DAX);
}

#[test]
fn phone_at_the_shack_goes_back_to_the_mic_and_the_browser_voice_to_dax() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert_eq!(r.step(&view(true, "USB"), true, false, false, 0), MIC);
    assert_eq!(
        r.written(false, Why::FollowMode, 0),
        None,
        "back on the operator's: forgotten"
    );
    // A streamed operator's presence: the browser's voice needs DAX.
    assert_eq!(r.step(&view(false, "USB"), true, true, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    // Presence ends: the mic again.
    assert_eq!(r.step(&view(true, "USB"), true, false, false, 0), MIC);
}

#[test]
fn cw_and_unknown_modes_leave_the_operators_setting_alone() {
    for mode in ["CW", "FDV", "WEIRD"] {
        let mut r = Routing::default();
        assert_eq!(
            r.step(&view(true, mode), true, false, false, 0),
            None,
            "{mode}"
        );
        assert_eq!(
            r.step(&view(false, mode), true, true, false, 0),
            None,
            "{mode}"
        );
    }
    // No mode reported yet: nothing.
    let mut r = Routing::default();
    let mut v = view(false, "DIGU");
    v.tx_mode = None;
    assert_eq!(r.step(&v, true, false, false, 0), None);
}

#[test]
fn never_beside_another_programs_dax() {
    let mut r = Routing::default();
    let mut v = view(false, "DIGU");
    v.other_feeder = true;
    assert_eq!(r.step(&v, true, false, false, 0), None);
    // Not even a restore owed from a previous session.
    let mut r = Routing::new(Some(false));
    let mut v = view(true, "USB");
    v.other_feeder = true;
    assert_eq!(r.step(&v, true, false, false, 0), None);
    assert_eq!(
        r.restore_on_disconnect(&v),
        Restore::Later,
        "not now: kept for a later connect"
    );
    // Control: the same views without the other program are written.
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
}

#[test]
fn only_while_the_tx_slice_is_ours_and_the_radio_has_said_its_flag() {
    let mut r = Routing::default();
    let mut v = view(false, "DIGU");
    v.tx_slice = None;
    assert_eq!(r.step(&v, true, false, false, 0), None);
    // Our TX slice again: a new decision.
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
    let mut r = Routing::default();
    let mut v = view(false, "DIGU");
    v.radio = None;
    assert_eq!(r.step(&v, true, false, false, 0), None);
    assert_eq!(r.operator(), None, "nothing is learnt from an unknown flag");
}

#[test]
fn dax_waits_for_our_transmit_stream() {
    let mut r = Routing::default();
    let mut v = view(false, "DIGU");
    v.dax_tx_stream = false;
    assert_eq!(r.step(&v, true, false, false, 0), Some(Step::CreateDaxTx));
    assert_eq!(
        r.step(&v, true, false, false, 0),
        Some(Step::CreateDaxTx),
        "until it exists"
    );
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
    // The mic needs no stream.
    let mut r = Routing::default();
    let mut v = view(true, "USB");
    v.dax_tx_stream = false;
    assert_eq!(r.step(&v, true, false, false, 0), MIC);
}

/// A radio already taking DAX (SmartSDR's own DAX switch sets the same radio-wide flag) still gets
/// Nexus's transmit stream: there is nothing to write, but without the stream nothing carries the
/// over. Nothing is written then, so nothing is owed back.
#[test]
fn a_radio_already_on_dax_still_gets_our_transmit_stream() {
    let mut r = Routing::default();
    let mut v = view(true, "DIGU");
    v.dax_tx_stream = false;
    assert_eq!(r.step(&v, true, false, false, 0), Some(Step::CreateDaxTx));
    assert_eq!(
        r.step(&v, true, false, false, 0),
        Some(Step::CreateDaxTx),
        "until it exists"
    );
    assert_eq!(
        r.step(&view(true, "DIGU"), true, false, false, 0),
        None,
        "already on DAX: nothing to write"
    );
    assert_eq!(r.operator(), Some(true), "the operator's own setting");
    assert_eq!(
        r.restore_on_disconnect(&view(true, "DIGU")),
        Restore::Nothing
    );
    // Native audio off: Nexus feeds nothing over DAX, so it asks for no stream.
    let mut r = Routing::default();
    assert_eq!(r.step(&v, false, false, false, 0), None);
}

#[test]
fn native_audio_off_puts_back_only_what_nexus_changed() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert_eq!(
        r.step(&view(true, "DIGU"), false, false, false, 0),
        Some(Step::Write {
            dax: false,
            why: Why::NativeAudioOff
        })
    );
    assert_eq!(r.written(false, Why::NativeAudioOff, 0), None);
    // Native audio never on: the flag is never touched.
    let mut r = Routing::default();
    assert_eq!(r.step(&view(true, "DIGU"), false, false, false, 0), None);
    assert_eq!(r.step(&view(true, "USB"), false, true, false, 0), None);
}

/// Nexus's own write leaves the radio on DAX over the operator's mic from that write until the
/// mic's write: through native audio going off, which is when nothing feeds that DAX. Never the
/// operator's own DAX, never once the operator has moved the flag, never beside another program's
/// DAX; a previous session's write until it is put back.
#[test]
fn nexus_leaves_the_radio_on_dax_from_its_own_write_until_the_mic_is_back() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
    assert!(!r.leaves_dax(&view(false, "DIGU")), "nothing written yet");
    r.written(true, Why::FollowMode, 0);
    assert!(
        r.leaves_dax(&view(false, "DIGU")),
        "a write in flight stands"
    );
    assert!(r.leaves_dax(&view(true, "DIGU")));
    assert_eq!(
        r.step(&view(true, "DIGU"), false, false, false, 10),
        Some(Step::Write {
            dax: false,
            why: Why::NativeAudioOff
        })
    );
    assert!(
        r.leaves_dax(&view(true, "DIGU")),
        "native audio off, not yet put back"
    );
    let mut other = view(true, "DIGU");
    other.other_feeder = true;
    assert!(!r.leaves_dax(&other), "another program's DAX feeds it");
    assert!(!r.leaves_dax(&view(false, "DIGU")), "the operator moved it");
    r.written(false, Why::NativeAudioOff, 10);
    assert!(
        !r.leaves_dax(&view(true, "DIGU")),
        "the mic's write is on its way"
    );
    // The operator's own DAX: nothing of Nexus's.
    let mut r = Routing::default();
    assert_eq!(r.step(&view(true, "DIGU"), true, false, false, 0), None);
    assert!(!r.leaves_dax(&view(true, "DIGU")));
    // A previous session's DAX over the operator's mic, until it is put back.
    let r = Routing::new(Some(false));
    assert!(r.leaves_dax(&view(true, "DIGU")));
    assert!(!r.leaves_dax(&view(false, "DIGU")));
}

/// A voice over keyed while Nexus's own write has the radio on DAX would leave on DAX, not on the
/// operator's mic: right after the transmit slice goes from a digital mode to Phone, with native
/// audio on or off, until the mic's write. Not while the stream's browser voice is live with native
/// audio on: Phone wants DAX then, and that voice rides it.
#[test]
fn a_voice_over_would_leave_on_the_dax_nexus_set_until_the_mics_write() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert_eq!(
        r.step(&view(true, "USB"), true, false, false, 10),
        Some(Step::Write {
            dax: false,
            why: Why::FollowMode
        })
    );
    assert!(r.leaves_voice_on_dax(&view(true, "USB"), true));
    assert!(
        r.leaves_voice_on_dax(&view(true, "USB"), false),
        "native audio off"
    );
    r.written(false, Why::FollowMode, 10);
    assert!(
        !r.leaves_voice_on_dax(&view(true, "USB"), true),
        "the mic's write is on its way"
    );
    // The stream's browser voice: Phone wants DAX, and that voice rides it.
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, true, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert_eq!(r.step(&view(true, "USB"), true, true, false, 10), None);
    assert!(!r.leaves_voice_on_dax(&view(true, "USB"), true));
    assert!(
        r.leaves_voice_on_dax(&view(true, "USB"), false),
        "native audio off: nothing feeds DAX, browser voice or not"
    );
}

#[test]
fn a_previous_sessions_change_is_put_back_at_the_next_connect_first() {
    // The last session wrote DAX over the operator's mic and never restored it (a crash).
    let mut r = Routing::new(Some(false));
    assert_eq!(
        r.step(&view(true, "DIGU"), true, false, false, 0),
        Some(Step::Write {
            dax: false,
            why: Why::RestoreAtConnect
        })
    );
    assert_eq!(r.written(false, Why::RestoreAtConnect, 0), None);
    // Then the ruling applies as usual, with the operator's setting the remembered one.
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
    assert_eq!(r.written(true, Why::FollowMode, 0), Some(false));
    // Already back (the operator put it back by hand): nothing to restore.
    let mut r = Routing::new(Some(false));
    assert_eq!(r.step(&view(false, "USB"), true, false, false, 0), None);
    assert_eq!(
        r.restore_on_disconnect(&view(false, "USB")),
        Restore::Nothing
    );
}

#[test]
fn a_clean_disconnect_restores_only_a_flag_nexus_still_holds() {
    let mut r = Routing::default();
    r.step(&view(false, "DIGU"), true, false, false, 0);
    r.written(true, Why::FollowMode, 0);
    // Still in flight: it stands as written, whatever the stale flag says.
    assert_eq!(
        r.restore_on_disconnect(&view(false, "DIGU")),
        Restore::Write(false)
    );
    // Reported back.
    assert_eq!(r.step(&view(true, "DIGU"), true, false, false, 10), None);
    assert_eq!(
        r.restore_on_disconnect(&view(true, "DIGU")),
        Restore::Write(false)
    );
    // The operator changed it since: their latest choice stands.
    assert_eq!(
        r.restore_on_disconnect(&view(false, "DIGU")),
        Restore::Nothing
    );
    // Unknown: keep the memory for the next connect.
    let mut v = view(true, "DIGU");
    v.radio = None;
    assert_eq!(r.restore_on_disconnect(&v), Restore::Later);
    // Nexus never wrote: nothing.
    let r = Routing::default();
    assert_eq!(
        r.restore_on_disconnect(&view(true, "DIGU")),
        Restore::Nothing
    );
}

#[test]
fn the_memory_survives_a_new_process_through_its_file() {
    let dir = std::env::temp_dir().join(format!(
        "nexus-flex-routing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(MEMORY_FILE);
    let a = FileMemory::new(Some(path.clone()));
    a.keep("192.0.2.20:4992", Some(false));
    a.keep("192.0.2.21:4992", Some(true));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "192.0.2.20:4992 0\n192.0.2.21:4992 1\n"
    );
    // "A new process": a fresh memory reading the same file.
    let b = FileMemory::new(Some(path.clone()));
    assert_eq!(b.recall("192.0.2.20:4992"), Some(false));
    assert_eq!(b.recall("192.0.2.21:4992"), Some(true));
    assert_eq!(b.recall("192.0.2.22:4992"), None);
    b.keep("192.0.2.20:4992", None);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "192.0.2.21:4992 1\n"
    );
    // A line it cannot read is skipped, not trusted.
    std::fs::write(&path, "junk\n192.0.2.30:4992 2\n192.0.2.31:4992 0\n").unwrap();
    let c = FileMemory::new(Some(path));
    assert_eq!(c.recall("192.0.2.30:4992"), None);
    assert_eq!(c.recall("192.0.2.31:4992"), Some(false));
    // In process only.
    let d = FileMemory::new(None);
    d.keep("r", Some(true));
    assert_eq!(d.recall("r"), Some(true));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_write_never_reported_back_is_tried_again_and_a_hand_flip_after_one_is_not() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    // Not reported back yet: wait.
    assert_eq!(
        r.step(&view(false, "DIGU"), true, false, false, CONFIRM_MS - 1),
        None
    );
    // Never reported back (refused, or lost): the same decision, tried again.
    assert_eq!(
        r.step(&view(false, "DIGU"), true, false, false, CONFIRM_MS),
        DAX
    );
    r.written(true, Why::FollowMode, CONFIRM_MS);
    // Reported back, then flipped by hand: the operator's flip stands.
    assert_eq!(
        r.step(&view(true, "DIGU"), true, false, false, CONFIRM_MS + 10),
        None
    );
    assert_eq!(
        r.step(&view(false, "DIGU"), true, false, false, 60_000),
        None
    );
}

#[test]
fn nothing_is_decided_against_a_flag_a_write_is_about_to_change() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert_eq!(
        r.step(&view(true, "DIGU"), true, false, false, 10),
        None,
        "confirmed"
    );
    assert_eq!(r.step(&view(true, "USB"), true, false, false, 20), MIC);
    r.written(false, Why::FollowMode, 20);
    // The browser voice goes live before the radio has reported the mic: its DAX must not be
    // taken for done because the stale flag still says DAX.
    assert_eq!(
        r.step(&view(true, "USB"), true, true, false, 30),
        None,
        "waits for the echo"
    );
    assert_eq!(r.step(&view(false, "USB"), true, true, false, 40), DAX);
}

/// The voice keyer (operator ruling, 2026-10-12, "Change it now"): in Phone at the shack a message
/// waiting asks for DAX, its going asks for the mic back, and the DAX between is the keyer's until
/// the radio reports its mic again. That DAX does not lift the voice rule for Nexus's own PTT,
/// which carries the operator's voice: only the browser voice rides DAX.
#[test]
fn a_voice_keyer_message_asks_for_dax_and_its_going_for_the_mic() {
    let mut r = Routing::default();
    assert_eq!(
        r.step(&view(false, "USB"), true, false, false, 0),
        None,
        "the mic, as it is"
    );
    assert_eq!(r.step(&view(false, "USB"), true, false, true, 10), DAX);
    assert_eq!(
        r.written(true, Why::FollowMode, 10),
        Some(false),
        "kept: Nexus changed it"
    );
    assert!(
        !r.dax_for_keyer(&view(false, "USB")),
        "not before the radio reports it"
    );
    assert!(r.dax_for_keyer(&view(true, "USB")));
    assert!(
        r.leaves_voice_on_dax(&view(true, "USB"), true),
        "Nexus's own PTT still waits for the mic"
    );
    assert_eq!(
        r.step(&view(true, "USB"), true, false, true, 20),
        None,
        "confirmed"
    );
    // The message taken: the mic again.
    assert_eq!(r.step(&view(true, "USB"), true, false, false, 30), MIC);
    assert_eq!(
        r.written(false, Why::FollowMode, 30),
        None,
        "back on the operator's"
    );
    assert!(
        r.dax_for_keyer(&view(true, "USB")),
        "the keyer's until the radio reports its mic"
    );
    assert_eq!(r.step(&view(false, "USB"), true, false, false, 40), None);
    assert!(
        !r.dax_for_keyer(&view(true, "USB")),
        "the mic reported: DAX after it is not the keyer's"
    );
}

/// DAX for anything but the keyer's message alone is never the keyer's: the browser voice live
/// beside a message, a digital mode, or the keyer's DAX taken over by a digital mode before the
/// mic came back. Nor beside another program's DAX, nor outside a voice mode.
#[test]
fn dax_for_anything_else_is_never_the_keyers() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "USB"), true, true, true, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert!(!r.dax_for_keyer(&view(true, "USB")), "the browser voice's");

    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, true, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert!(!r.dax_for_keyer(&view(true, "USB")), "the digital mode's");

    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "USB"), true, false, true, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert!(r.dax_for_keyer(&view(true, "USB")));
    let mut other = view(true, "USB");
    other.other_feeder = true;
    assert!(!r.dax_for_keyer(&other), "beside another program's DAX");
    assert!(!r.dax_for_keyer(&view(true, "CW")), "outside a voice mode");
    // The slice goes digital before the mic is back: DAX is that mode's now, nothing to write.
    assert_eq!(r.step(&view(true, "DIGU"), true, false, false, 10), None);
    assert!(
        !r.dax_for_keyer(&view(true, "USB")),
        "taken over by the digital mode"
    );
}

/// The keyer's DAX is Nexus's change like any other: a disconnect in the middle of a message puts
/// the mic back, and the shim's silent-over rules see it.
#[test]
fn a_disconnect_mid_message_puts_the_mic_back() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "USB"), true, false, true, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert_eq!(
        r.restore_on_disconnect(&view(true, "USB")),
        Restore::Write(false)
    );
    assert!(r.leaves_dax(&view(true, "USB")));
}

/// For keying, Nexus's write stays in flight until the radio reports it or refuses it, however late:
/// the routing's confirm window decides again, but never lets a key through on the value the radio
/// still reports. A report later than that window has the decision taken again on the real value.
#[test]
fn a_write_stays_in_flight_for_keys_until_the_radio_reports_or_refuses_it() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(true, "USB"), true, false, false, 0), MIC);
    r.written(false, Why::FollowMode, 0);
    r.sent(7);
    assert!(r.write_in_flight(&view(true, "USB")));
    assert!(r.mic_write_in_flight(&view(true, "USB")));
    // A press asks for DAX after the confirm window: the stale flag already says DAX, so nothing is
    // written, and the write of the mic is still in flight for keys.
    assert_eq!(
        r.step(&view(true, "USB"), true, false, true, CONFIRM_MS + 100),
        None
    );
    assert!(r.write_in_flight(&view(true, "USB")), "past the window");
    assert!(r.mic_write_in_flight(&view(true, "USB")));
    // CONTROL: another command's refusal changes nothing.
    r.refused(8);
    assert!(r.write_in_flight(&view(true, "USB")));
    // The late report lands: no longer in flight, and DAX is written for the press after all.
    assert_eq!(
        r.step(&view(false, "USB"), true, false, true, CONFIRM_MS + 600),
        DAX
    );
    assert!(!r.write_in_flight(&view(false, "USB")));
    // A write of DAX is in flight, but never counts as the radio having its mic.
    r.written(true, Why::FollowMode, CONFIRM_MS + 600);
    r.sent(9);
    assert!(r.write_in_flight(&view(false, "USB")));
    assert!(!r.mic_write_in_flight(&view(false, "USB")));
    // The radio refuses it: the flag it reports stands.
    r.refused(9);
    assert!(!r.write_in_flight(&view(false, "USB")));
    // A new connection starts with nothing in flight.
    assert!(!Routing::new(Some(false)).write_in_flight(&view(true, "USB")));
}
