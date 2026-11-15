//! JS8Call 3.0.3's automatic replies and held messages on the engine's real path: the decodes go
//! in through `js8_ingest`, and what keys comes out of `plan_tx` and `commit_tx`, checked BY VALUE
//! (the words keyed, the offset, the period). QUERY CALL, AGN?, a group's message, "Store
//! message…", and the Inbox's notes of what a cap dropped, as the cockpit and the journal see them.
use super::*;
use ::js8::proto::command::Command;
use ::js8::proto::frame::encode_frame;
use ::js8::proto::station::DropReason;

const PERIOD_MS: u64 = 15_000;

/// KD9TAW in EN52 on 20 m, TX on at 1500 Hz, in @FUN, replies going by themselves (the
/// confirmation off) so each one keys.
fn engine() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.js8_autoreply_confirmation = false;
    e.settings.js8_groups = vec!["@FUN".into()];
    e.js8_apply_station_config();
    e.js8_enter();
    e.set_frequency(14.078, "20m", "USB");
    e.set_tx_offset(1500.0);
    e.set_tx_enabled(true);
    e
}

/// `from` sends `text` (to `to` when given), heard at `freq` with `snr`, frame by frame.
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

/// The radio loop for `secs` from now: the once-a-second tick and, at each period boundary, the
/// transmit decision. Each over that keyed: (period start, offset, word).
fn overs(e: &mut Engine, secs: u64) -> Vec<(u64, f32, ::js8::Word87)> {
    let t0 = tempo_core::timing::now_unix_ms() as u64;
    let mut last = t0 / PERIOD_MS;
    let mut out = Vec::new();
    for k in 0..=secs {
        let t = t0 + k * 1000;
        e.js8_tick(t);
        let slot = t / PERIOD_MS;
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
            out.push((slot * PERIOD_MS, f0, word));
        }
    }
    out
}

/// The words KD9TAW keys for `text`.
fn words(text: &str) -> Vec<::js8::Word87> {
    ::js8::proto::compose::frames_with_grid("KD9TAW", "EN52", None, text, Js8Speed::Normal)
        .expect("composes")
        .iter()
        .map(|(f, i3)| encode_frame(f, *i3, Js8Speed::Normal).expect("packs"))
        .collect()
}

/// What keyed, by value: exactly `text`'s words, every one at my 1500 Hz offset, one per period
/// in a row.
fn assert_keyed(keyed: &[(u64, f32, ::js8::Word87)], text: &str) {
    let got: Vec<::js8::Word87> = keyed.iter().map(|o| o.2).collect();
    assert_eq!(got, words(text), "keyed {text:?}");
    for (i, (at, f0, _)) in keyed.iter().enumerate() {
        assert_eq!(*f0, 1500.0, "on my offset");
        assert_eq!(*at, keyed[0].0 + i as u64 * PERIOD_MS, "one frame a period");
    }
}

#[test]
fn a_js8_query_call_is_answered_on_the_air_by_value() {
    let mut e = engine();
    hear(&mut e, "W1AW", None, "HEARTBEAT FN30", 900.0, -12);
    hear(
        &mut e,
        "K1ABC",
        Some("KD9TAW"),
        "QUERY CALL W1AW?",
        1210.0,
        -7,
    );
    assert_keyed(&overs(&mut e, 60), "K1ABC YES -12 (NOW)");
}

#[test]
fn a_js8_agn_sends_my_last_message_again_on_the_air() {
    let mut e = engine();
    e.js8_send_command("W1AW".into(), Command::Snr.id(), "-05".into())
        .expect("sends");
    let first = overs(&mut e, 30);
    assert_keyed(&first, "W1AW SNR -05");
    hear(&mut e, "W1AW", Some("KD9TAW"), "AGN?", 1210.0, -9);
    let again = overs(&mut e, 30);
    assert_keyed(&again, "W1AW SNR -05");
    assert_eq!(again[0].2, first[0].2, "the same word again");
}

#[test]
fn a_js8_group_message_is_offered_and_handed_over_once_on_the_air() {
    let mut e = engine();
    hear(
        &mut e,
        "W1AW",
        Some("KD9TAW"),
        "MSG TO:@FUN NET AT EIGHT",
        1750.0,
        -5,
    );
    assert_keyed(&overs(&mut e, 60), "W1AW ACK");
    hear(&mut e, "K1ABC", None, "@FUN QUERY MSGS", 1210.0, -7);
    assert_keyed(&overs(&mut e, 60), "K1ABC YES MSG ID 1");
    hear(&mut e, "K1ABC", Some("KD9TAW"), "QUERY MSG 1", 1210.0, -7);
    assert_keyed(&overs(&mut e, 120), "K1ABC MSG NET AT EIGHT FROM W1AW");
    let st = e.js8_state();
    assert_eq!(
        st.inbox
            .iter()
            .map(|m| (m.to.as_str(), m.state))
            .collect::<Vec<_>>(),
        [("@FUN", InboxState::Store)],
        "still held for the rest of the group"
    );
    hear(&mut e, "K1ABC", None, "@FUN QUERY MSGS", 1210.0, -7);
    assert_keyed(&overs(&mut e, 60), "K1ABC NO");
    hear(&mut e, "N0XYZ", None, "@FUN QUERY MSGS", 1210.0, -7);
    assert_keyed(&overs(&mut e, 60), "N0XYZ YES MSG ID 1");
}

/// "Store message…" keys nothing by itself; the message is held, journaled, and handed over on
/// the air when K1ABC asks for it.
#[test]
fn js8_store_message_is_held_journaled_and_handed_over_on_the_air() {
    let dir = std::env::temp_dir().join(format!("nexus-js8-store-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("js8_station.json");
    let _ = std::fs::remove_file(&path);
    let mut e = engine();
    e.set_js8_journal_path(path.clone());
    e.js8_store_message("k1abc".into(), "see you friday".into())
        .expect("stored");
    assert!(overs(&mut e, 30).is_empty(), "keys nothing by itself");
    let st = e.js8_state();
    let row = &st.inbox[0];
    assert_eq!(
        (
            row.from.as_str(),
            row.to.as_str(),
            row.text.as_str(),
            row.state
        ),
        ("KD9TAW", "K1ABC", "SEE YOU FRIDAY", InboxState::Store)
    );
    e.station.journals.settle();
    let mut fresh = engine();
    fresh.js8_load_journal(&std::fs::read_to_string(&path).expect("journaled"));
    assert_eq!(fresh.js8_state().inbox, st.inbox, "the journal holds it");
    hear(&mut e, "K1ABC", Some("KD9TAW"), "QUERY MSG 1", 1210.0, -7);
    assert_keyed(&overs(&mut e, 120), "K1ABC MSG SEE YOU FRIDAY FROM KD9TAW");
    assert_eq!(e.js8_state().inbox[0].state, InboxState::Delivered);
}

#[test]
fn js8_store_message_says_why_it_refuses() {
    let mut e = engine();
    for (to, text, said) in [
        ("@ALLCALL", "HI", "@ALLCALL is not a station's call."),
        ("K1ABC", "  ", "Nothing to store."),
    ] {
        assert_eq!(
            e.js8_store_message(to.into(), text.into()),
            Err(said.to_string())
        );
    }
    e.settings.mycall = String::new();
    e.js8_apply_station_config();
    assert_eq!(
        e.js8_store_message("K1ABC".into(), "HI".into()),
        Err("Set your callsign in Settings before storing a JS8 message.".to_string())
    );
    assert!(e.js8_state().inbox.is_empty());
}

/// The Inbox's notes reach the cockpit as `inboxDropped`, absent while there are none; the
/// Remote's bounded copy carries them; clearing them is the operator's act and is journaled.
#[test]
fn the_inbox_notes_reach_the_state_and_clear() {
    let mut e = engine();
    let json = serde_json::to_value(e.js8_state()).unwrap();
    assert!(
        json.get("inboxDropped").is_none(),
        "absent while there are none"
    );
    for i in 0..101u32 {
        e.js8_station
            .store_message(&format!("K{i}AA"), "HELLO", 1_000 + u64::from(i))
            .expect("stored");
    }
    let st = e.js8_state();
    assert_eq!(st.inbox_dropped.len(), 1);
    assert_eq!(
        (
            st.inbox_dropped[0].entry.to.as_str(),
            st.inbox_dropped[0].why
        ),
        ("K0AA", DropReason::Count)
    );
    let json = serde_json::to_value(&st).unwrap();
    assert_eq!(json["inboxDropped"][0]["why"], "count");
    assert_eq!(json["inboxDropped"][0]["entry"]["to"], "K0AA");
    assert_eq!(json["inboxDropped"][0]["atMs"], 1_100);
    // The Remote's sample is the desktop's state whole, `presence` and `controls` with it, and
    // the notes ride in it as they are.
    assert!(
        st.presence.is_some(),
        "control: the desktop's state carries presence"
    );
    assert!(
        st.controls.is_some(),
        "control: the desktop's state carries controls"
    );
    assert_eq!(
        e.bounded_js8_state(),
        Some(st),
        "the Remote's copy is the same"
    );
    e.js8_inbox_dropped_clear();
    assert!(e.js8_state().inbox_dropped.is_empty());
}
