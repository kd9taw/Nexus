//! A streamed Remote operator's microphone (`tempo_app::engine::remote_mic`) on a Flex through
//! Nexus's own client with native DAX audio on. Their voice is played to the client's DAX and
//! nowhere else, so the radio must take that DAX for the whole over: a radio on its own mic would
//! put the shack's room on the air in their place. The key is marked as the voice keyer's is, so
//! the client keys it only on Nexus's DAX; a press it refuses keys nothing and ends with the reason
//! the page shows ("the station's audio changed"), and a keyed over ends the moment the radio no
//! longer takes Nexus's DAX. With native audio off, the over goes to the sound card as before.
//! A voice-keyer message sent during a Remote session rides the same DAX and is held to the same
//! rule: keyed only on Nexus's DAX, and ended the moment the radio leaves it.
//!
//! The radio is the SmartSDR simulator and the loop the real `RadioLoop::step`, as in the parent
//! module. What the radio saw is read by value, in its order ([`voice_trace`]).

use super::voice_keyer::{dax_in, run_until_seen, seen_at, send, shack, voice_trace};
use super::*;
use tempo_app::mic::{MicEnded, MicFeed, MicFrame, MIC};
use tempo_app::remote_control::ptt_hold::PttHold;
use tempo_app::remote_control::transmit::TransmitAuthority;

const PRESS: &str = "10000000-0000-4000-8000-00000000000b";

/// The streamed operator at the scene's station: presence held, the page's held PTT and the
/// microphone feed installed as `Service::start` installs them.
struct Remote {
    hold: PttHold,
    feed: MicFeed,
    _presence: TransmitAuthority,
    seq: u64,
}

/// A streamed operator joins `s`: from here the routing wants DAX for their voice in Phone.
fn join(s: &FlexScene) -> Remote {
    let hold = PttHold::default();
    let feed = MicFeed::default();
    let presence = TransmitAuthority::default();
    let now = Instant::now();
    {
        let mut e = s.engine.lock().unwrap();
        e.set_remote_ptt_hold(hold.clone());
        e.set_remote_mic_feed(feed.clone());
        let permit = presence.permit(now + Duration::from_secs(120)).unwrap();
        assert!(
            e.hold_remote_presence(permit, now),
            "premise: presence held"
        );
    }
    Remote {
        hold,
        feed,
        _presence: presence,
        seq: 0,
    }
}

/// The operator holds PTT and talks for `ms`: each loop tick the page re-asserts its hold and
/// sends a 20 ms frame, on the real clock.
fn talk(s: &mut FlexScene, r: &mut Remote, ms: u64) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(ms) {
        let now = Instant::now();
        r.hold.hold(PRESS, now);
        r.seq += 1;
        let _ = r.feed.push(MicFrame {
            seq: r.seq,
            media: MIC.samples(20) * r.seq,
            arrived: now,
            samples: vec![0.5; MIC.samples(20) as usize],
        });
        s.step(now_unix_ms());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Why the operator's last over ended, as the page is shown it.
fn ended(r: &Remote) -> Option<MicEnded> {
    r.feed.status().ended
}

/// The bundled radio refusing Nexus's write of its DAX source: it goes on taking its own mic.
fn refuses_dax() -> SimSession {
    let mut session = reports(&["USB"]);
    let rule = (
        Pattern::Exact("transmit set dax=1".to_string()),
        vec![Rule {
            code: "50000016".to_string(),
            message: String::new(),
            items: Vec::new(),
        }],
    );
    session.rules.retain(|(p, _)| *p != rule.0);
    session.rules.push(rule);
    session
}

/// ⛔ THE ROOM NEVER GOES ON THE AIR FOR A REMOTE OPERATOR. Phone at the shack, the radio on its own
/// mic, and the radio does not take the DAX Nexus writes for the streamed voice: it refuses the
/// write, or never answers it. The operator presses and talks: nothing keys (no `xmit 1`), the
/// over ends with the reason the page shows, and a PTT still held never keys later.
#[test]
fn a_remote_press_the_radio_has_not_switched_to_dax_for_keys_nothing_and_says_why() {
    for (name, mut s) in [
        ("refused", shack(refuses_dax())),
        (
            "unanswered",
            FlexScene::with_faults(
                true,
                reports(&["USB"]),
                vec![tempo_flexsim::Fault::Unanswered {
                    starting: "transmit set dax=1".into(),
                    then_silent: false,
                }],
                tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
            ),
        ),
    ] {
        if name == "unanswered" {
            super::voice_keyer::shack_on(&mut s);
        }
        let since = Instant::now();
        let mut r = join(&s);
        run_until_seen(&mut s, since, "transmit set dax=1");
        s.run(300);
        talk(&mut s, &mut r, 1_500);
        let trace = voice_trace(&s, since);
        assert_eq!(
            format!("{name}: keys={} ended={:?}", keys(&s), ended(&r)),
            format!("{name}: keys=0 ended=Some(RouteChanged)"),
            "{trace:?}"
        );
        assert!(
            !trace.iter().any(|t| t.starts_with("radio dax=1")),
            "premise: the radio never took DAX: {trace:?}"
        );
        assert!(!s.engine.lock().unwrap().mic_armed());
    }
}

/// CONTROL: the radio takes the DAX Nexus writes for the streamed voice. The press keys once, after
/// the radio reported DAX, and the operator's voice goes out over Nexus's DAX for as long as they
/// talk, with nothing said on the page.
#[test]
fn a_remote_press_on_nexuss_dax_keys_and_carries_the_voice() {
    let mut s = shack(reports(&["USB"]));
    let since = Instant::now();
    let mut r = join(&s);
    run_until_seen(&mut s, since, "radio dax=1");
    s.run(300);
    let pressed = Instant::now();
    talk(&mut s, &mut r, 1_500);
    assert!(
        s.engine.lock().unwrap().mic_keyed(),
        "the over ended: {:?} {:?}",
        ended(&r),
        voice_trace(&s, since)
    );
    assert_eq!(keys(&s), 1);
    assert_eq!(ended(&r), None);
    let keyed = seen_at(&s, since, "xmit 1").unwrap();
    assert!(keyed > seen_at(&s, since, "radio dax=1").unwrap());
    let frames: usize = dax_in(&s, pressed).iter().map(|(_, n)| n).sum();
    assert!(frames > 0, "the voice never reached Nexus's DAX");
}

/// The bundled radio, reporting its DAX source back to the mic `ms` after each `xmit 1`: the
/// operator pressing SmartSDR's own DAX button mid-over.
pub(super) fn leaves_dax_mid_over(ms: u64) -> SimSession {
    let mut session = reports(&["USB"]);
    let (_, rules) = session
        .rules
        .iter_mut()
        .find(|(p, _)| *p == Pattern::Exact("xmit 1".to_string()))
        .expect("the bundled session answers xmit 1");
    for rule in rules.iter_mut() {
        rule.items.push(Item::Wait(ms));
        rule.items.push(send("S0|transmit dax=0"));
    }
    session
}

/// ⛔ The radio leaving Nexus's DAX under a keyed Remote over (SmartSDR's own switch): its mic would
/// carry the rest of it, so the over ends at once with the reason on the page, and nothing re-keys
/// while the operator goes on talking.
#[test]
fn the_radio_leaving_dax_mid_remote_over_ends_it() {
    let mut s = shack(leaves_dax_mid_over(500));
    let since = Instant::now();
    let mut r = join(&s);
    run_until_seen(&mut s, since, "radio dax=1");
    s.run(300);
    talk(&mut s, &mut r, 2_000);
    let trace = voice_trace(&s, since);
    let left = seen_at(&s, since, "radio dax=0").expect("the radio left DAX");
    let unkey = seen_at(&s, since, "xmit 0").expect("the over was never unkeyed");
    assert!(left < unkey, "premise: the unkey followed the radio");
    assert!(
        unkey.duration_since(left) < Duration::from_millis(500),
        "the unkey waited {:?}: {trace:?}",
        unkey.duration_since(left)
    );
    assert_eq!(keys(&s), 1, "re-keyed: {trace:?}");
    assert_eq!(ended(&r), Some(MicEnded::RouteChanged));
    assert!(!s.engine.lock().unwrap().mic_armed());
}

/// CONTROL: with native audio off the voice goes to the sound card, which the operator has set the
/// radio to take, and the client leaves the radio's audio source alone: the press keys as it always
/// has, with no DAX source written for it.
#[test]
fn with_native_audio_off_a_remote_press_keys_as_before() {
    let mut s = FlexScene::with_session(false, reports(&["USB"]));
    {
        let mut e = s.engine.lock().unwrap();
        e.set_operating_mode("phone", false); // arms TX
        e.set_frequency(14.250, "20m", "USB");
    }
    s.run(500);
    let since = Instant::now();
    let mut r = join(&s);
    talk(&mut s, &mut r, 1_500);
    let trace = voice_trace(&s, since);
    assert_eq!(keys(&s), 1, "{trace:?} {:?}", ended(&r));
    assert!(s.engine.lock().unwrap().mic_keyed());
    assert!(
        !trace.iter().any(|t| t.starts_with("transmit set dax")),
        "{trace:?}"
    );
}

/// CONTROL: beside SmartSDR's DAX the route is not Nexus's to judge, so the watch leaves a keyed
/// Remote over alone.
#[test]
fn beside_smartsdrs_dax_a_remote_over_is_not_cut() {
    let mut s = FlexScene::with_faults(
        true,
        reports(&["USB"]),
        vec![tempo_flexsim::Fault::ForeignDaxTx(
            tempo_flexsim::ForeignDax::default(),
        )],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    {
        let mut e = s.engine.lock().unwrap();
        e.set_operating_mode("phone", false);
        e.set_frequency(14.250, "20m", "USB");
    }
    run_until(&mut s, "the client never saw the other program", |s| {
        s.state
            .rigctld_proc
            .as_ref()
            .and_then(CatDaemon::flex)
            .is_some_and(FlexDaemon::other_dax_feeder)
    });
    s.run(500);
    let since = Instant::now();
    let mut r = join(&s);
    s.run(300);
    talk(&mut s, &mut r, 2_000);
    let trace = voice_trace(&s, since);
    assert_eq!(
        format!(
            "keys={} ended={:?} keyed={}",
            keys(&s),
            ended(&r),
            s.engine.lock().unwrap().mic_keyed()
        ),
        "keys=1 ended=None keyed=true",
        "{trace:?}"
    );
}

/// A 3 s voice-keyer message, as the keyer's WAV reader hands it over.
fn three_seconds() -> Vec<f32> {
    vec![0.05; 36_000]
}

/// ⛔ A VOICE-KEYER MESSAGE DURING A REMOTE SESSION, THE RADIO LEAVING NEXUS'S DAX UNDER IT. With a
/// Remote operator on, the radio is already on Nexus's DAX for their voice, so the message needs no
/// switch around it; the operator then presses SmartSDR's own DAX button 500 ms into it. The
/// radio's mic would carry the rest and put the shack's room on the air: the message ends within a
/// few ticks of the radio's report, and nothing keys again.
#[test]
fn a_voice_message_in_a_remote_session_ends_when_the_radio_leaves_dax() {
    let mut s = shack(leaves_dax_mid_over(500));
    let since = Instant::now();
    let _r = join(&s);
    run_until_seen(&mut s, since, "radio dax=1");
    s.run(300);
    let sent = s.engine.lock().unwrap().send_voice(three_seconds());
    assert_eq!(sent, Ok(()));
    run_until_seen(&mut s, since, "radio dax=0");
    s.run(1_000);
    let trace = voice_trace(&s, since);
    let left = seen_at(&s, since, "radio dax=0").expect("the radio left DAX");
    let unkey = seen_at(&s, since, "xmit 0").expect("the message was never unkeyed");
    assert!(
        unkey.duration_since(left) < Duration::from_millis(500),
        "keyed {:?} after the radio left DAX: {trace:?}",
        unkey.saturating_duration_since(left)
    );
    assert_eq!(keys(&s), 1, "re-keyed: {trace:?}");
    assert!(!s.engine.lock().unwrap().snapshot().radio.voice_playing);
}

/// Hold Nexus's DAX transmit route down in `s`, or let it come back.
fn hold_route_out(s: &FlexScene, on: bool) {
    s.state
        .rigctld_proc
        .as_ref()
        .and_then(CatDaemon::flex)
        .expect("the Flex client")
        .hold_tx_route_out(on);
}

/// ⛔ A VOICE-KEYER MESSAGE BEFORE THE DAX TEE IS IN, DURING A REMOTE SESSION. The radio already
/// reports Nexus's DAX for the Remote operator, but the loop's transmit route trails the session
/// (the audio control thread's tick and its blocking stream requests: held here by a test-only
/// seam), so no tee carries the message. Keyed, its audio would go to the sound card, the radio
/// would send a silent over on DAX, and nothing would watch it: SmartSDR's DAX switched to the mic
/// 500 ms in would put the room on the air. It is dropped for DAX and keys nothing. CONTROL: once
/// the route is in, the next message keys, plays on DAX and ends when the radio leaves it.
#[test]
fn a_voice_message_before_the_dax_tee_is_in_keys_nothing() {
    let mut s = shack(leaves_dax_mid_over(500));
    let since = Instant::now();
    let _r = join(&s);
    run_until_seen(&mut s, since, "radio dax=1");
    s.run(300);
    hold_route_out(&s, true);
    run_until(&mut s, "the tee never went", |s| {
        s.state.dax_tee_id.is_none()
    });
    let sent = s.engine.lock().unwrap().send_voice(three_seconds());
    assert_eq!(sent, Ok(()));
    s.run(300);
    // The route comes in as it does in life, a tick or two late (mid-message, were it keyed).
    hold_route_out(&s, false);
    run_until(&mut s, "the tee never came in", |s| {
        s.state.dax_tee_id.is_some()
    });
    s.run(1_500);
    let trace = voice_trace(&s, since);
    let snap = serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap();
    assert_eq!(
        format!(
            "keys={} dropped={} left={:?}",
            keys(&s),
            snap["radio"]["voiceDropped"]["cause"],
            seen_at(&s, since, "radio dax=0").map(|left| seen_at(&s, since, "xmit 0")
                .map(|unkey| unkey.saturating_duration_since(left)))
        ),
        "keys=0 dropped=\"noDax\" left=None",
        "{trace:?}"
    );

    let again = Instant::now();
    let sent = s.engine.lock().unwrap().send_voice(three_seconds());
    assert_eq!(sent, Ok(()));
    run_until_seen(&mut s, again, "radio dax=0");
    s.run(1_000);
    let trace = voice_trace(&s, again);
    let key = seen_at(&s, again, "xmit 1").expect("the message never keyed with the tee in");
    let left = seen_at(&s, again, "radio dax=0").unwrap();
    let unkey = seen_at(&s, again, "xmit 0").expect("the message was never unkeyed");
    assert!(
        dax_in(&s, key).iter().map(|(_, n)| n).sum::<usize>() > 0,
        "the message never reached Nexus's DAX: {trace:?}"
    );
    assert!(
        unkey.duration_since(left) < Duration::from_millis(500),
        "keyed {:?} after the radio left DAX: {trace:?}",
        unkey.saturating_duration_since(left)
    );
    assert_eq!(keys(&s), 1, "re-keyed: {trace:?}");
}

/// CONTROL: the same message in the same session, with the radio staying on Nexus's DAX, plays to
/// its end.
#[test]
fn a_voice_message_in_a_remote_session_on_nexuss_dax_plays_to_its_end() {
    let mut s = shack(reports(&["USB"]));
    let since = Instant::now();
    let _r = join(&s);
    run_until_seen(&mut s, since, "radio dax=1");
    s.run(300);
    let sent = s.engine.lock().unwrap().send_voice(three_seconds());
    assert_eq!(sent, Ok(()));
    let keyed_since = Instant::now();
    run_until_seen(&mut s, keyed_since, "xmit 0");
    let trace = voice_trace(&s, since);
    let key = seen_at(&s, since, "xmit 1").expect("the message keyed");
    let unkey = seen_at(&s, since, "xmit 0").unwrap();
    assert!(
        unkey.duration_since(key) >= Duration::from_millis(2_900),
        "cut after {:?}: {trace:?}",
        unkey.duration_since(key)
    );
    assert_eq!(keys(&s), 1, "{trace:?}");
}

/// ⛔ A MESSAGE KEYED ONLY ON NEXUS'S DAX. A Remote operator on, the radio already on DAX by the
/// operator's own setting, and the radio refusing Nexus's DAX transmit stream: the radio does not
/// have the mic, so the message waits for no switch, but nothing of Nexus's reaches the radio's DAX
/// either. Its key is marked all the same, so the client keys nothing and the keyer says the
/// message was dropped for DAX.
#[test]
fn a_voice_message_off_nexuss_dax_in_a_remote_session_keys_nothing() {
    let mut s = FlexScene::with_faults(
        true,
        radio_on_dax(),
        vec![tempo_flexsim::Fault::DaxTxRefused],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    let since = Instant::now();
    let _r = join(&s);
    {
        let mut e = s.engine.lock().unwrap();
        e.set_operating_mode("phone", false); // arms TX
        e.set_frequency(14.250, "20m", "USB");
    }
    run_until(&mut s, "the client never settled", client_ready);
    s.run(1_000);
    assert!(
        !has_mic(&s),
        "premise: the radio does not have the mic: {:?}",
        voice_trace(&s, since)
    );
    let sent = s.engine.lock().unwrap().send_voice(three_seconds());
    assert_eq!(sent, Ok(()));
    s.run(1_500);
    let snap = serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap();
    assert_eq!(
        format!(
            "keys={} dropped={}",
            keys(&s),
            snap["radio"]["voiceDropped"]["cause"]
        ),
        "keys=0 dropped=\"noDax\"",
        "{:?}",
        voice_trace(&s, since)
    );
}
