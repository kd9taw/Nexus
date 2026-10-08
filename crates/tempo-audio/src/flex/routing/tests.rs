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
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), DAX);
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
    assert_eq!(r.step(&view(true, "DIGU"), true, false, 0), None);
    // The operator flips it by hand: not fought while nothing they could change has changed.
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), None);
    // A change of mode is a new decision.
    assert_eq!(r.step(&view(false, "DIGL"), true, false, 0), DAX);
}

#[test]
fn phone_at_the_shack_goes_back_to_the_mic_and_the_browser_voice_to_dax() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert_eq!(r.step(&view(true, "USB"), true, false, 0), MIC);
    assert_eq!(
        r.written(false, Why::FollowMode, 0),
        None,
        "back on the operator's: forgotten"
    );
    // A streamed operator's presence: the browser's voice needs DAX.
    assert_eq!(r.step(&view(false, "USB"), true, true, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    // Presence ends: the mic again.
    assert_eq!(r.step(&view(true, "USB"), true, false, 0), MIC);
}

#[test]
fn cw_and_unknown_modes_leave_the_operators_setting_alone() {
    for mode in ["CW", "FDV", "WEIRD"] {
        let mut r = Routing::default();
        assert_eq!(r.step(&view(true, mode), true, false, 0), None, "{mode}");
        assert_eq!(r.step(&view(false, mode), true, true, 0), None, "{mode}");
    }
    // No mode reported yet: nothing.
    let mut r = Routing::default();
    let mut v = view(false, "DIGU");
    v.tx_mode = None;
    assert_eq!(r.step(&v, true, false, 0), None);
}

#[test]
fn never_beside_another_programs_dax() {
    let mut r = Routing::default();
    let mut v = view(false, "DIGU");
    v.other_feeder = true;
    assert_eq!(r.step(&v, true, false, 0), None);
    // Not even a restore owed from a previous session.
    let mut r = Routing::new(Some(false));
    let mut v = view(true, "USB");
    v.other_feeder = true;
    assert_eq!(r.step(&v, true, false, 0), None);
    assert_eq!(
        r.restore_on_disconnect(&v),
        Restore::Later,
        "not now: kept for a later connect"
    );
    // Control: the same views without the other program are written.
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), DAX);
}

#[test]
fn only_while_the_tx_slice_is_ours_and_the_radio_has_said_its_flag() {
    let mut r = Routing::default();
    let mut v = view(false, "DIGU");
    v.tx_slice = None;
    assert_eq!(r.step(&v, true, false, 0), None);
    // Our TX slice again: a new decision.
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), DAX);
    let mut r = Routing::default();
    let mut v = view(false, "DIGU");
    v.radio = None;
    assert_eq!(r.step(&v, true, false, 0), None);
    assert_eq!(r.operator(), None, "nothing is learnt from an unknown flag");
}

#[test]
fn dax_waits_for_our_transmit_stream() {
    let mut r = Routing::default();
    let mut v = view(false, "DIGU");
    v.dax_tx_stream = false;
    assert_eq!(r.step(&v, true, false, 0), Some(Step::CreateDaxTx));
    assert_eq!(
        r.step(&v, true, false, 0),
        Some(Step::CreateDaxTx),
        "until it exists"
    );
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), DAX);
    // The mic needs no stream.
    let mut r = Routing::default();
    let mut v = view(true, "USB");
    v.dax_tx_stream = false;
    assert_eq!(r.step(&v, true, false, 0), MIC);
}

/// A radio already taking DAX (SmartSDR's own DAX switch sets the same radio-wide flag) still gets
/// Nexus's transmit stream: there is nothing to write, but without the stream nothing carries the
/// over. Nothing is written then, so nothing is owed back.
#[test]
fn a_radio_already_on_dax_still_gets_our_transmit_stream() {
    let mut r = Routing::default();
    let mut v = view(true, "DIGU");
    v.dax_tx_stream = false;
    assert_eq!(r.step(&v, true, false, 0), Some(Step::CreateDaxTx));
    assert_eq!(
        r.step(&v, true, false, 0),
        Some(Step::CreateDaxTx),
        "until it exists"
    );
    assert_eq!(
        r.step(&view(true, "DIGU"), true, false, 0),
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
    assert_eq!(r.step(&v, false, false, 0), None);
}

#[test]
fn native_audio_off_puts_back_only_what_nexus_changed() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert_eq!(
        r.step(&view(true, "DIGU"), false, false, 0),
        Some(Step::Write {
            dax: false,
            why: Why::NativeAudioOff
        })
    );
    assert_eq!(r.written(false, Why::NativeAudioOff, 0), None);
    // Native audio never on: the flag is never touched.
    let mut r = Routing::default();
    assert_eq!(r.step(&view(true, "DIGU"), false, false, 0), None);
    assert_eq!(r.step(&view(true, "USB"), false, true, 0), None);
}

/// Nexus's own write leaves the radio on DAX over the operator's mic from that write until the
/// mic's write: through native audio going off, which is when nothing feeds that DAX. Never the
/// operator's own DAX, never once the operator has moved the flag, never beside another program's
/// DAX; a previous session's write until it is put back.
#[test]
fn nexus_leaves_the_radio_on_dax_from_its_own_write_until_the_mic_is_back() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), DAX);
    assert!(!r.leaves_dax(&view(false, "DIGU")), "nothing written yet");
    r.written(true, Why::FollowMode, 0);
    assert!(
        r.leaves_dax(&view(false, "DIGU")),
        "a write in flight stands"
    );
    assert!(r.leaves_dax(&view(true, "DIGU")));
    assert_eq!(
        r.step(&view(true, "DIGU"), false, false, 10),
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
    assert_eq!(r.step(&view(true, "DIGU"), true, false, 0), None);
    assert!(!r.leaves_dax(&view(true, "DIGU")));
    // A previous session's DAX over the operator's mic, until it is put back.
    let r = Routing::new(Some(false));
    assert!(r.leaves_dax(&view(true, "DIGU")));
    assert!(!r.leaves_dax(&view(false, "DIGU")));
}

#[test]
fn a_previous_sessions_change_is_put_back_at_the_next_connect_first() {
    // The last session wrote DAX over the operator's mic and never restored it (a crash).
    let mut r = Routing::new(Some(false));
    assert_eq!(
        r.step(&view(true, "DIGU"), true, false, 0),
        Some(Step::Write {
            dax: false,
            why: Why::RestoreAtConnect
        })
    );
    assert_eq!(r.written(false, Why::RestoreAtConnect, 0), None);
    // Then the ruling applies as usual, with the operator's setting the remembered one.
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), DAX);
    assert_eq!(r.written(true, Why::FollowMode, 0), Some(false));
    // Already back (the operator put it back by hand): nothing to restore.
    let mut r = Routing::new(Some(false));
    assert_eq!(r.step(&view(false, "USB"), true, false, 0), None);
    assert_eq!(
        r.restore_on_disconnect(&view(false, "USB")),
        Restore::Nothing
    );
}

#[test]
fn a_clean_disconnect_restores_only_a_flag_nexus_still_holds() {
    let mut r = Routing::default();
    r.step(&view(false, "DIGU"), true, false, 0);
    r.written(true, Why::FollowMode, 0);
    // Still in flight: it stands as written, whatever the stale flag says.
    assert_eq!(
        r.restore_on_disconnect(&view(false, "DIGU")),
        Restore::Write(false)
    );
    // Reported back.
    assert_eq!(r.step(&view(true, "DIGU"), true, false, 10), None);
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
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    // Not reported back yet: wait.
    assert_eq!(
        r.step(&view(false, "DIGU"), true, false, CONFIRM_MS - 1),
        None
    );
    // Never reported back (refused, or lost): the same decision, tried again.
    assert_eq!(r.step(&view(false, "DIGU"), true, false, CONFIRM_MS), DAX);
    r.written(true, Why::FollowMode, CONFIRM_MS);
    // Reported back, then flipped by hand: the operator's flip stands.
    assert_eq!(
        r.step(&view(true, "DIGU"), true, false, CONFIRM_MS + 10),
        None
    );
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 60_000), None);
}

#[test]
fn nothing_is_decided_against_a_flag_a_write_is_about_to_change() {
    let mut r = Routing::default();
    assert_eq!(r.step(&view(false, "DIGU"), true, false, 0), DAX);
    r.written(true, Why::FollowMode, 0);
    assert_eq!(
        r.step(&view(true, "DIGU"), true, false, 10),
        None,
        "confirmed"
    );
    assert_eq!(r.step(&view(true, "USB"), true, false, 20), MIC);
    r.written(false, Why::FollowMode, 20);
    // The browser voice goes live before the radio has reported the mic: its DAX must not be
    // taken for done because the stale flag still says DAX.
    assert_eq!(
        r.step(&view(true, "USB"), true, true, 30),
        None,
        "waits for the echo"
    );
    assert_eq!(r.step(&view(false, "USB"), true, true, 40), DAX);
}
