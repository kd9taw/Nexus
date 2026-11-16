//! The three follow-ups to JS8Call 3.0.3's automatic replies on the engine's real path: the
//! decodes go in through `js8_ingest`, and what keys comes out of `plan_tx` and `commit_tx`,
//! checked BY VALUE (the words keyed, the offset, the period). A MSG that reads "MSG n" is filed
//! and acknowledged; a held message too long to hand over at the current speed is not offered and
//! the cockpit's state says why; a heartbeat's acknowledgement goes through the @ALLCALL cache.
use super::*;
use crate::dto::Js8InboxTooLong;
use ::js8::proto::compose::{frames_with_grid, ComposeError};
use ::js8::proto::frame::encode_frame;

/// KD9TAW in EN52 on 20 m, TX on at 1500 Hz, in @FUN, replies going by themselves (the
/// confirmation off) so each one keys; heartbeats acknowledged (heartbeat networking, HB ACK).
fn engine() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.js8_autoreply_confirmation = false;
    e.settings.js8_groups = vec!["@FUN".into()];
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

/// The radio loop for `secs` from now at the transmit speed: the once-a-second tick and, at each
/// period boundary, the transmit decision. Each over that keyed: (period start, offset, word).
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

/// The words KD9TAW keys for `text` at `speed`.
fn words(text: &str, speed: Js8Speed) -> Vec<::js8::Word87> {
    frames_with_grid("KD9TAW", "EN52", None, text, speed)
        .expect("composes")
        .iter()
        .map(|(f, i3)| encode_frame(f, *i3, speed).expect("packs"))
        .collect()
}

/// What keyed, by value: exactly `text`'s words at `speed`, one per period in a row, every one at
/// my 1500 Hz offset, or for a heartbeat's acknowledgement on one free spot in 500–950 Hz.
fn assert_keyed(keyed: &[(u64, f32, ::js8::Word87)], text: &str, speed: Js8Speed) {
    let got: Vec<::js8::Word87> = keyed.iter().map(|o| o.2).collect();
    assert_eq!(got, words(text, speed), "keyed {text:?}");
    let period_ms = u64::from(speed.period_s()) * 1000;
    for (i, (at, f0, _)) in keyed.iter().enumerate() {
        if text.contains(" HEARTBEAT SNR ") {
            assert!(
                (500.0..=950.0).contains(f0) && f0 % 50.0 == 0.0 && *f0 == keyed[0].1,
                "on a heartbeat spot: {f0}"
            );
        } else {
            assert_eq!(*f0, 1500.0, "on my offset");
        }
        assert_eq!(*at, keyed[0].0 + i as u64 * period_ms, "one frame a period");
    }
}

/// `KD9TAW MSG MSG 1` from K1ABC is a MSG reading "MSG 1": acknowledged on the air and filed for
/// the operator, while message 1, held for K1ABC, stays held. QUERY MSG 1 still hands it over.
#[test]
fn a_js8_msg_that_reads_msg_n_is_filed_and_acknowledged_on_the_air() {
    let mut e = engine();
    hear(
        &mut e,
        "W1AW",
        Some("KD9TAW"),
        "MSG TO:K1ABC T1",
        1750.0,
        -5,
    );
    assert_keyed(&overs(&mut e, 60), "W1AW ACK", Js8Speed::Normal);
    hear(&mut e, "K1ABC", Some("KD9TAW"), "MSG MSG 1", 1210.0, -7);
    assert_keyed(&overs(&mut e, 60), "K1ABC ACK", Js8Speed::Normal);
    let rows = |e: &Engine| {
        e.js8_state()
            .inbox
            .iter()
            .map(|m| (m.from.clone(), m.to.clone(), m.text.clone(), m.state))
            .collect::<Vec<_>>()
    };
    let row = |from: &str, to: &str, text: &str, state| {
        (from.to_string(), to.to_string(), text.to_string(), state)
    };
    assert_eq!(
        rows(&e),
        [
            row("W1AW", "K1ABC", "T1", InboxState::Store),
            row("K1ABC", "KD9TAW", "MSG 1", InboxState::Unread),
        ],
        "filed for the operator, and message 1 still held"
    );
    hear(&mut e, "K1ABC", Some("KD9TAW"), "QUERY MSG 1", 1210.0, -7);
    assert_keyed(
        &overs(&mut e, 60),
        "K1ABC MSG T1 FROM W1AW",
        Js8Speed::Normal,
    );
    assert_eq!(e.js8_state().inbox[0].state, InboxState::Delivered);
}

/// The shortest run of "QRV " whose hand-over to K1ABC from W1AW does not fit one transmission at
/// Slow and does at Normal, with the frames it needs at Slow.
fn too_long_at_slow() -> (String, usize) {
    let hand_over = |t: &str, speed| {
        frames_with_grid(
            "KD9TAW",
            "EN52",
            None,
            &format!("K1ABC MSG {t} FROM W1AW"),
            speed,
        )
    };
    (1..200)
        .map(|n| "QRV ".repeat(n).trim_end().to_string())
        .find_map(|t| match hand_over(&t, Js8Speed::Slow) {
            Err(ComposeError::TooLong { frames, .. }) => {
                assert!(hand_over(&t, Js8Speed::Normal).is_ok(), "fits at Normal");
                Some((t, frames))
            }
            _ => None,
        })
        .expect("some run is too long at Slow")
}

/// A message held for K1ABC whose hand-over needs more frames than one transmission holds at Slow:
/// at Slow, QUERY MSGS is answered NO on the air and the cockpit's state names the message, the
/// frames, the cap and the speed it goes at, and so does the Remote's sample (its page's parser
/// lists it). At Normal it is offered, by value, and the note is gone.
#[test]
fn a_js8_held_message_too_long_at_this_speed_is_not_offered_and_the_state_says_why() {
    let mut e = engine();
    let (long, frames) = too_long_at_slow();
    hear(
        &mut e,
        "W1AW",
        Some("KD9TAW"),
        &format!("MSG TO:K1ABC {long}"),
        1750.0,
        -5,
    );
    assert_keyed(&overs(&mut e, 60), "W1AW ACK", Js8Speed::Normal);
    e.js8_set_speed(Js8Speed::Slow.index()).expect("Slow");
    let st = e.js8_state();
    assert_eq!(
        st.inbox_too_long,
        [Js8InboxTooLong {
            id: 1,
            frames,
            max: 19,
            fits_at: Some(Js8Speed::Normal),
        }]
    );
    let json = serde_json::to_value(&st).expect("serialises");
    assert_eq!(
        json["inboxTooLong"],
        serde_json::json!([{ "id": 1, "frames": frames, "max": 19, "fitsAt": "normal" }]),
        "on the wire"
    );
    let remote = e.bounded_js8_state().expect("bounded");
    assert_eq!(
        serde_json::to_value(&remote).expect("serialises")["inboxTooLong"],
        json["inboxTooLong"],
        "the Remote's sample carries it"
    );
    hear(&mut e, "K1ABC", Some("KD9TAW"), "QUERY MSGS", 1210.0, -7);
    assert_keyed(&overs(&mut e, 90), "K1ABC NO", Js8Speed::Slow);
    assert_eq!(
        e.js8_state().inbox[0].state,
        InboxState::Store,
        "still held"
    );

    e.js8_set_speed(Js8Speed::Normal.index()).expect("Normal");
    let st = e.js8_state();
    assert!(st.inbox_too_long.is_empty());
    assert!(
        serde_json::to_value(&st)
            .expect("serialises")
            .get("inboxTooLong")
            .is_none(),
        "left out of the JSON while empty"
    );
    hear(&mut e, "K1ABC", Some("KD9TAW"), "QUERY MSGS", 1210.0, -7);
    assert_keyed(&overs(&mut e, 60), "K1ABC YES MSG ID 1", Js8Speed::Normal);
}

/// A heartbeat is acknowledged on the air; the same station's next heartbeat a moment later is
/// not, and neither is its @ALLCALL QUERY CALL: JS8Call's @ALLCALL cache holds it off for 15
/// minutes. Another station is answered on @ALLCALL meanwhile.
#[test]
fn a_js8_heartbeat_is_acknowledged_once_and_its_station_held_off_on_allcall() {
    let mut e = engine();
    hear(&mut e, "K1ABC", None, "HEARTBEAT FN31", 1100.0, -9);
    assert_keyed(
        &overs(&mut e, 60),
        "K1ABC HEARTBEAT SNR -09",
        Js8Speed::Normal,
    );
    hear(&mut e, "W1AW", None, "HEARTBEAT FN30", 900.0, -7);
    assert_keyed(
        &overs(&mut e, 60),
        "W1AW HEARTBEAT SNR -07",
        Js8Speed::Normal,
    );
    hear(&mut e, "W1AW", None, "HEARTBEAT FN30", 900.0, -7);
    assert!(overs(&mut e, 60).is_empty(), "its next heartbeat: held off");
    hear(
        &mut e,
        "W1AW",
        None,
        "@ALLCALL QUERY CALL K1ABC?",
        900.0,
        -7,
    );
    assert!(
        overs(&mut e, 60).is_empty(),
        "its @ALLCALL QUERY CALL: held off"
    );
    hear(
        &mut e,
        "N0XYZ",
        None,
        "@ALLCALL QUERY CALL K1ABC?",
        1300.0,
        -11,
    );
    assert_keyed(&overs(&mut e, 60), "N0XYZ YES -09 (NOW)", Js8Speed::Normal);
}
