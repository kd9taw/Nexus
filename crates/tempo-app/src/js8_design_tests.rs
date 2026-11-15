//! JS8Call 3.0.3's own design on the engine's real path, where Nexus had departed from it: the
//! decodes go in through `js8_ingest`, and what keys comes out of `plan_tx` and `commit_tx`,
//! checked BY VALUE (the words keyed, the offset, the period). A QUERY CALL about a portable call
//! heard as such is answered, and the heard list the cockpit and the Remote read keeps the call as
//! heard; the 55-minute rule's stations go into the operator's "never acknowledge" list in Settings
//! and in `settings.json`.
use super::*;
use crate::engine::{engine_lock, Engine};
use crate::settings::writer::{Flushed, SettingsWriter};
use ::js8::proto::compose::frames_with_grid;
use ::js8::proto::frame::encode_frame;
use std::sync::{Arc, Mutex};

/// KD9TAW in EN52 on 20 m, TX on at 1500 Hz, replies going by themselves (the confirmation off)
/// so each one keys; heartbeats acknowledged (heartbeat networking, HB ACK).
fn engine() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.js8_autoreply_confirmation = false;
    e.settings.js8_hb_networking = true;
    e.settings.js8_hb_ack = true;
    e.js8_apply_station_config();
    e.js8_enter();
    e.set_frequency(14.078, "20m", "USB");
    e.set_tx_offset(1500.0);
    e.set_tx_enabled(true);
    e
}

/// `from` sends `text` (to `to` when given) at Normal, heard at `freq` with `snr`, frame by frame.
fn hear(e: &mut Engine, from: &str, to: Option<&str>, text: &str, freq: f32, snr: i32) {
    let to = to.map(|t| CallRef::Base(t.to_string()));
    let frames =
        ::js8::proto::compose::frames(from, to.as_ref(), text, Js8Speed::Normal).expect("composes");
    for (f, i3) in frames {
        let word = encode_frame(&f, i3, Js8Speed::Normal).expect("packable");
        let d = modes::Decode {
            message: f.render(),
            sync: 5.0,
            snr,
            dt: 0.1,
            freq,
            nap: 0,
            qual: 0.9,
            rv: None,
            mode: Some(modes::ModeKind::Js8 {
                speed: Js8Speed::Normal,
            }),
            raw: Some(*word.as_bytes()),
            unchecked: false,
        };
        e.js8_ingest(&[d], 0);
    }
}

/// A heartbeat from `call` (a `/P` one too: a heartbeat frame carries its call whole), heard at
/// `freq` with `snr`.
fn hear_heartbeat(e: &mut Engine, call: &str, freq: f32, snr: i32) {
    let frame = ::js8::Frame::Heartbeat {
        call: call.to_string(),
        grid: Some("FN30".to_string()),
        is_cq: false,
        idx: 0,
    };
    let whole = ::js8::I3 {
        first: true,
        last: true,
        data: false,
    };
    let word = encode_frame(&frame, whole, Js8Speed::Normal).expect("packable");
    let d = modes::Decode {
        message: frame.render(),
        sync: 5.0,
        snr,
        dt: 0.1,
        freq,
        nap: 0,
        qual: 0.9,
        rv: None,
        mode: Some(modes::ModeKind::Js8 {
            speed: Js8Speed::Normal,
        }),
        raw: Some(*word.as_bytes()),
        unchecked: false,
    };
    e.js8_ingest(&[d], 0);
}

/// The radio loop for `secs` from now: the once-a-second tick and, at each period boundary, the
/// transmit decision. Each over that keyed: (period start, offset, word).
fn overs(e: &mut Engine, secs: u64) -> Vec<(u64, f32, ::js8::Word87)> {
    let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
    let t0 = tempo_core::timing::now_unix_ms() as u64;
    let mut last = t0 / period_ms;
    let mut out = Vec::new();
    for k in 0..=secs {
        let t = t0 + k * 1000;
        e.js8_tick(t);
        let slot = t / period_ms;
        if slot == last {
            continue;
        }
        last = slot;
        let Some(plan) = e.plan_tx(slot) else {
            continue;
        };
        let TxWaveform::Js8 { f0, word, .. } = &plan.waveform else {
            panic!("a JS8 plan carries the typed waveform");
        };
        let (f0, word) = (*f0, *word);
        let wave = plan.waveform.build();
        if !e.commit_tx(&plan, wave, slot).is_empty() {
            out.push((slot * period_ms, f0, word));
        }
    }
    out
}

/// The words KD9TAW keys for `text` at Normal.
fn words(text: &str) -> Vec<::js8::Word87> {
    frames_with_grid("KD9TAW", "EN52", None, text, Js8Speed::Normal)
        .expect("composes")
        .iter()
        .map(|(f, i3)| encode_frame(f, *i3, Js8Speed::Normal).expect("packs"))
        .collect()
}

/// The words of `keyed`, run together.
fn keyed_words(keyed: &[(u64, f32, ::js8::Word87)]) -> Vec<::js8::Word87> {
    keyed.iter().map(|o| o.2).collect()
}

/// QUERY CALL about W1AW/P, heard as W1AW/P, keys `K1ABC YES -12 (NOW)` on my offset, as JS8Call
/// answers it; and the heard list the cockpit and the Remote read (`Js8State.stations`) holds
/// W1AW/P as heard.
#[test]
fn a_js8_query_call_about_a_portable_call_heard_is_answered_on_the_air() {
    let mut e = engine();
    // No acknowledgement of the heartbeat in the way: the QUERY CALL's answer is the over.
    e.settings.js8_hb_ack = false;
    e.js8_apply_station_config();
    hear_heartbeat(&mut e, "W1AW/P", 900.0, -12);
    let calls: Vec<String> = e
        .js8_state()
        .stations
        .iter()
        .map(|h| h.call.clone())
        .collect();
    assert_eq!(calls, ["W1AW/P"], "kept as heard");
    hear(
        &mut e,
        "K1ABC",
        Some("KD9TAW"),
        "QUERY CALL W1AW/P?",
        1210.0,
        -7,
    );
    let keyed = overs(&mut e, 60);
    assert_eq!(keyed_words(&keyed), words("K1ABC YES -12 (NOW)"));
    assert!(keyed.iter().all(|o| o.1 == 1500.0), "on my offset");
}

/// The 55-minute rule's station goes into Settings' "Never acknowledge heartbeats from these
/// callsigns", after the operator's own, and into `settings.json`; the next launch, built from
/// that file, does not acknowledge it; the operator taking it off the list (a Settings save) is
/// how it is acknowledged again, as on JS8Call's list.
#[test]
fn the_55_minute_rule_writes_its_station_into_settings_and_settings_json() {
    let dir = std::env::temp_dir().join(format!("nexus-js8-hbdeny-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let _ = std::fs::remove_file(&path);
    let shared = Arc::new(Mutex::new(engine()));
    // The cache JS8Call holds a second acknowledgement off with for 15 minutes, shortened so the
    // rule sees two heartbeats a moment apart on the wall clock.
    let no_cache = |e: &mut Engine| {
        let mut c = e.js8_station.config().clone();
        c.allcall_reply_interval_ms = 0;
        e.js8_station.set_config(c);
    };
    {
        let mut e = engine_lock(&shared);
        e.configure_remote_settings_store(path.clone());
        e.settings.js8_hb_ack_rate_limit = true;
        e.settings.js8_hb_ack_deny = vec!["N0XYZ".into()];
        e.js8_apply_station_config();
        no_cache(&mut e);
        hear(&mut e, "W1AW", None, "HEARTBEAT FN30", 900.0, -7);
        assert_eq!(
            keyed_words(&overs(&mut e, 60)),
            words("W1AW HEARTBEAT SNR -07"),
            "the first heartbeat is acknowledged"
        );
        no_cache(&mut e);
        hear(&mut e, "W1AW", None, "HEARTBEAT FN30", 900.0, -7);
        assert!(overs(&mut e, 60).is_empty(), "the second is not");
        assert_eq!(
            e.settings().js8_hb_ack_deny,
            ["N0XYZ", "W1AW"],
            "on the operator's list, after their own"
        );
        assert_eq!(
            e.js8_state().presence.map(|p| p.hb_ack_rate_blocked),
            Some(vec!["W1AW".to_string()]),
            "and named on the HB ACK chip as the rule's"
        );
    }
    assert_eq!(
        SettingsWriter::of(&path).flush(std::time::Duration::from_secs(30)),
        Flushed::Saved
    );
    let saved = crate::settings::Settings::load(&path);
    assert_eq!(
        saved.js8_hb_ack_deny,
        ["N0XYZ", "W1AW"],
        "settings.json holds it"
    );
    // The next launch, from that file.
    let mut next = Engine::with_settings(saved);
    next.settings.js8_autoreply_confirmation = false;
    next.js8_apply_station_config();
    next.js8_enter();
    next.set_frequency(14.078, "20m", "USB");
    next.set_tx_offset(1500.0);
    next.set_tx_enabled(true);
    hear(&mut next, "W1AW", None, "HEARTBEAT FN30", 900.0, -7);
    hear(&mut next, "K1ABC", None, "HEARTBEAT FN30", 900.0, -9);
    assert_eq!(
        keyed_words(&overs(&mut next, 60)),
        words("K1ABC HEARTBEAT SNR -09"),
        "after the launch: K1ABC acknowledged, W1AW not"
    );
    // The operator takes W1AW off the list: acknowledged again.
    let mut s = next.settings().clone();
    s.js8_hb_ack_deny = vec!["N0XYZ".into()];
    next.apply_settings(s);
    no_cache(&mut next);
    hear(&mut next, "W1AW", None, "HEARTBEAT FN30", 900.0, -7);
    assert_eq!(
        keyed_words(&overs(&mut next, 60)),
        words("W1AW HEARTBEAT SNR -07"),
        "off the list: acknowledged again"
    );
    drop((shared, next));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The JS8 frame planned for the next period at Normal, with its slot: (slot, word, offset).
fn plan_frame(e: &mut Engine) -> (u64, ::js8::Word87, f32) {
    let slot = tempo_core::timing::now_unix_ms() as u64 / 15_000 + 1;
    let plan = e.plan_tx(slot).expect("a frame planned");
    let TxWaveform::Js8 { word, f0, .. } = plan.waveform else {
        panic!("a JS8 plan carries the typed waveform");
    };
    (slot, word, f0)
}

/// "Allow TX offset changes while sending" on: a move of the TX offset while a frame keyed on it
/// is on the air hands the radio loop the rest of that frame at the new offset, once per move,
/// BY VALUE: given how much of the frame has not played, that many samples of its wave moved
/// `JS8_RETUNE_LEAD` into them, every move before kept, as JS8Call's Modulator takes a new
/// frequency mid-frame (Modulator.cpp:217-229).
#[test]
fn with_offset_changes_on_a_move_hands_over_the_rest_of_the_frame_on_the_air() {
    use ::js8::phy::{encode_word, modulate, modulate_moved};
    let mut e = engine();
    e.settings.js8_tx_offset_while_sending = true;
    e.js8_send(None, "HELLO FROM THE PARK".into())
        .expect("sends");
    let (slot, word, f0) = plan_frame(&mut e);
    assert_eq!(f0, 1500.0, "keyed on my offset");
    assert!(e.js8_retune_on_air(slot).is_none(), "not moved: nothing");
    let tones = encode_word(&word, Js8Speed::Normal);
    let at_1500 = modulate(&tones, Js8Speed::Normal, 1500.0, 12_000.0);
    let len = at_1500.len();
    let lead = JS8_RETUNE_LEAD;
    e.set_tx_offset(1800.0);
    let mut rest = e.js8_retune_on_air(slot).expect("moved: the rest");
    let played = 60_000;
    let first = played + lead;
    let want = modulate_moved(
        &tones,
        Js8Speed::Normal,
        &[(0, 1500.0), (first, 1800.0)],
        12_000.0,
    );
    assert_eq!(
        rest(len - played, 0),
        want[played..],
        "moved 100 ms into the rest"
    );
    assert_eq!(
        want[..first],
        at_1500[..first],
        "the frame as it was, up to the move"
    );
    assert_eq!(
        rest(len - played, 63),
        want[played - 63..],
        "with the 63 samples before it, which the sound card asks for too"
    );
    assert!(e.js8_retune_on_air(slot).is_none(), "handed over once");
    // A second move carries on from the first.
    e.set_tx_offset(1650.0);
    let mut again = e.js8_retune_on_air(slot).expect("moved again");
    let second = 90_000 + lead;
    let want = modulate_moved(
        &tones,
        Js8Speed::Normal,
        &[(0, 1500.0), (first, 1800.0), (second, 1650.0)],
        12_000.0,
    );
    assert_eq!(again(len - 90_000, 0), want[90_000..], "every move kept");
    assert!(again(0, 0).is_empty(), "nothing left to play: nothing");
}

/// The move is handed over only where it applies: the option on, the frame on the air (its slot),
/// keyed on the operator's offset, TX on, and the new offset inside the licence's privileges. A
/// frame keyed on a heartbeat spot keeps it.
#[test]
fn a_move_is_handed_over_only_for_a_frame_on_the_offset_with_tx_on_inside_the_privileges() {
    let fresh_at = |option: bool, dial_mhz: f64| {
        let mut e = engine();
        e.set_license_class("extra");
        e.settings.js8_tx_offset_while_sending = option;
        e.set_frequency(dial_mhz, "20m", "USB");
        e.js8_send(None, "HELLO FROM THE PARK".into())
            .expect("sends");
        let (slot, _, _) = plan_frame(&mut e);
        (e, slot)
    };
    let fresh = |option: bool| fresh_at(option, 14.078);
    let (mut e, slot) = fresh(true);
    e.set_tx_offset(1800.0);
    assert!(e.js8_retune_on_air(slot + 1).is_none(), "another slot's");
    assert!(e.js8_retune_on_air(slot).is_some(), "control: this slot's");

    let (mut e, slot) = fresh(false);
    e.set_tx_offset(1800.0);
    assert_eq!(e.tx_offset_hz(), 1500.0, "the option off: the move refused");
    assert!(e.js8_retune_on_air(slot).is_none(), "the option off");

    // Moved with the option on, then the option turned off before the loop asks: the frame stays.
    let (mut e, slot) = fresh(true);
    e.set_tx_offset(1800.0);
    let mut s = e.settings().clone();
    s.js8_tx_offset_while_sending = false;
    e.apply_settings(s);
    assert_eq!(e.tx_offset_hz(), 1800.0, "premise: moved while it was on");
    assert!(
        e.js8_retune_on_air(slot).is_none(),
        "the option off by the time the loop asks"
    );

    let (mut e, slot) = fresh(true);
    e.set_tx_offset(1800.0);
    e.set_tx_enabled(false);
    assert!(e.js8_retune_on_air(slot).is_none(), "TX off");

    // 14.1475 MHz USB at Extra: 1500 Hz up is 14.149 MHz, 2800 Hz up is past the top of 20 m's
    // data privileges (14.150 MHz).
    let (mut e, slot) = fresh_at(true, 14.1475);
    e.set_tx_offset(2800.0);
    assert!(!e.tx_allowed(), "premise: out of the privileges");
    assert!(
        e.js8_retune_on_air(slot).is_none(),
        "out of the privileges: the frame stays"
    );
    e.set_tx_offset(2000.0);
    assert!(e.tx_allowed(), "premise: back inside");
    assert!(
        e.js8_retune_on_air(slot).is_some(),
        "control: inside, it moves"
    );

    // A heartbeat on its own spot in 500–1000 Hz keeps it.
    let mut e = engine();
    e.settings.js8_tx_offset_while_sending = true;
    e.js8_heartbeat_now().expect("a heartbeat");
    let (slot, _, f0) = plan_frame(&mut e);
    assert!(
        (500.0..=1000.0).contains(&f0) && f0 != 1500.0,
        "premise: on a heartbeat spot, {f0}"
    );
    e.set_tx_offset(1800.0);
    assert!(
        e.js8_retune_on_air(slot).is_none(),
        "a heartbeat on its spot keeps it"
    );
}
