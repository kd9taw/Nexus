//! The stream's station side, with a real operations authority and a real engine. What needs a
//! WebRTC session (the offer answered, DTLS, no LAN address) is tested in `tempo_stream::session`
//! on Windows; everything here runs on every platform.
use super::*;
use std::sync::Mutex;

const DEVICE: &str = "10000000-0000-4000-8000-000000000001";
const SESSION: &str = "20000000-0000-4000-8000-000000000001";
const PRESS: &str = "10000000-0000-4000-8000-00000000000a";

fn id() -> String {
    super::super::transport::random_secret().unwrap()[..32]
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if [8, 12, 16, 20].contains(&i) {
                format!("-{c}")
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// A station a streamed browser controls: the control grant, a live lease acquired at operation
/// version 4, the engine wired to the authority and the held PTT as `Service::start` wires them,
/// and the operator's streaming switch on.
struct Fixture {
    station: Station,
    lease: String,
    delivered: Arc<Mutex<Vec<WebviewInput>>>,
}

fn fixture(now: Instant) -> Fixture {
    let authority = Arc::new(Authority::default());
    let mut engine = tempo_app::engine::Engine::new("W9XYZ", "EN52", 0);
    engine.set_remote_transmit_revocation(authority.transmit_revocation());
    let ptt = PttHold::default();
    engine.set_remote_ptt_hold(ptt.clone());
    let mut settings = engine.settings().clone();
    settings.remote_stream = true;
    engine.apply_settings(settings);
    let engine: crate::SharedEngine = Arc::new(Mutex::new(engine));
    let connection = authority.start_connection();
    authority.permit_station(DEVICE, true).unwrap();
    let boot = authority
        .handle_version(
            (connection, OPERATION_VERSION),
            SESSION,
            DEVICE,
            &Request::State { request_id: id() },
            &engine,
            now,
        )
        .unwrap()["stationBootId"]
        .as_str()
        .unwrap()
        .to_string();
    let acquired = authority
        .handle_version(
            (connection, OPERATION_VERSION),
            SESSION,
            DEVICE,
            &Request::Acquire {
                request_id: id(),
                station_boot_id: boot,
            },
            &engine,
            now,
        )
        .unwrap();
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let sink = delivered.clone();
    let input: InputSink = Arc::new(move |i: &WebviewInput| sink.lock().unwrap().push(i.clone()));
    Fixture {
        station: Station {
            authority,
            engine,
            connection,
            host: Host {
                input: Some(input),
                ptt,
                window: None,
            },
            #[cfg(feature = "radio")]
            audio: None,
        },
        lease: acquired["leaseId"].as_str().unwrap().to_string(),
        delivered,
    }
}

fn contract_offer() -> String {
    let file: Value = serde_json::from_str(include_str!(
        "../../../../remote/test/fixtures/stream/signal.json"
    ))
    .unwrap();
    file["roomToStation"][0]["message"]["payload"]["sdp"]
        .as_str()
        .unwrap()
        .to_string()
}

fn offer(lease: &str) -> Offer {
    Offer {
        session: SESSION.into(),
        device: DEVICE.into(),
        lease: lease.into(),
        sdp: contract_offer(),
    }
}

/// What the platform answers once everything the station checks has passed: a session on
/// Windows, and "unavailable" elsewhere, where there is no WebRTC backend to offer.
fn passed() -> Result<(), StreamReason> {
    if cfg!(windows) {
        Ok(())
    } else {
        Err(StreamReason::StreamUnavailable)
    }
}

// ----- A3: the same admission chain, before anything of the session exists -----

/// ★ A3: an offer without station control, with a lapsed lease, or from a revoked device is
/// refused `notController` by the first check, before the station opens a socket. The control is
/// the same offer under a live lease and the control grant, which gets past admission.
#[test]
fn an_offer_without_control_or_a_live_lease_is_refused_first() {
    let now = Instant::now();
    let f = fixture(now);
    // CONTROL: admitted; what is left is the platform's own answer.
    assert_eq!(admit(&f.station, &offer(&f.lease), now), passed());
    // Someone else's lease, or a made-up one.
    assert_eq!(
        admit(
            &f.station,
            &offer("50000000-0000-4000-8000-000000000001"),
            now
        ),
        Err(StreamReason::NotController)
    );
    // The lease lapsed: no heartbeat for five seconds.
    let later = now + Duration::from_secs(6);
    assert_eq!(
        admit(&f.station, &offer(&f.lease), later),
        Err(StreamReason::NotController)
    );
    // A logging-only browser: a perfectly good lease, and no control grant.
    let f = fixture(now);
    f.station.authority.permit_station(DEVICE, false).unwrap();
    assert_eq!(
        admit(&f.station, &offer(&f.lease), now),
        Err(StreamReason::NotController)
    );
}

/// "A stream never survives revoke-device": the running stream's own recheck refuses once the
/// device's control grant is withdrawn at the shack.
#[test]
fn revoking_the_device_ends_a_running_stream() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
    let tick = now + RECHECK;
    assert!(
        streaming.still_admitted(tick),
        "control: admitted while granted"
    );
    f.station.authority.permit_station(DEVICE, false).unwrap();
    assert!(
        !streaming.still_admitted(tick + RECHECK),
        "the stream outlived its device's revocation"
    );
}

/// The operator's switch: with streaming off at the station, an admitted browser is refused
/// `streamDisabled`.
#[test]
fn an_offer_to_a_station_with_streaming_off_is_refused() {
    let now = Instant::now();
    let f = fixture(now);
    {
        let mut e = tempo_app::engine::engine_lock(&f.station.engine);
        let mut s = e.settings().clone();
        s.remote_stream = false;
        e.apply_settings(s);
    }
    assert_eq!(
        admit(&f.station, &offer(&f.lease), now),
        Err(StreamReason::StreamDisabled)
    );
}

/// A4's first lock, reached through admission: an admitted browser's plain-RTP offer is refused
/// as an invalid offer.
#[test]
fn an_admitted_plain_rtp_offer_is_refused_as_invalid() {
    let now = Instant::now();
    let f = fixture(now);
    let mut plain = offer(&f.lease);
    plain.sdp = plain.sdp.replace("UDP/TLS/RTP/SAVPF", "RTP/AVP");
    assert_eq!(
        admit(&f.station, &plain, now),
        Err(StreamReason::InvalidOffer)
    );
}

// ----- S8 and S9 (A7): presence, and a picture fresh enough to hold it -----

fn heartbeat(lease: &str, decoded: Option<u32>) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "type": "heartbeat", "requestId": id(), "leaseId": lease, "decodedFrameAt": decoded,
    }))
    .unwrap()
}

fn reply_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// ★ A7: a heartbeat whose picture is 2.5 s old renews the lease and leaves presence to lapse
/// (and with it everything at the station stops); the control, a 0.5 s-old picture, renews both.
#[test]
fn a_stale_picture_renews_the_lease_and_not_presence() {
    for fresh in [false, true] {
        let t0 = Instant::now();
        let f = fixture(t0);
        let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), t0);
        // Connected: presence from the moment the session is live.
        streaming.presence.renew(&f.station, &streaming.offer, t0);
        assert!(streaming.presence.live(t0), "premise: presence installed");
        {
            // Something is on the air, started by the streamed operator.
            let mut e = tempo_app::engine::engine_lock(&f.station.engine);
            e.set_operating_mode("phone", false);
            e.set_ptt(true);
            assert!(e.manual_ptt());
        }
        // Three seconds on, a heartbeat. `fresh` stands for the picture's age: the session's
        // clock says 0.5 s old when true, 2.5 s old when false (tempo_stream's frame_clock tests).
        let t1 = t0 + Duration::from_secs(3);
        let (answer, _) = streaming.control(&heartbeat(&f.lease, Some(1)), |_| fresh, t1);
        let answer = reply_of(&answer.unwrap());
        assert_eq!(
            answer["value"]["phase"], "controlling",
            "the lease was not renewed"
        );
        // Held either way at this moment: a stale picture does not END presence, it stops
        // renewing it, and the first permit still has two seconds to run.
        assert_eq!(answer["presence"], true);
        // Past the first presence deadline, inside the renewed lease.
        let t2 = t0 + Duration::from_secs(5);
        let halted = tempo_app::engine::engine_lock(&f.station.engine).poll_remote_transmit(t2);
        assert_eq!(halted, !fresh, "fresh: {fresh}");
        let e = tempo_app::engine::engine_lock(&f.station.engine);
        assert_eq!(e.manual_ptt(), fresh, "fresh: {fresh}: the key");
        // The lease itself outlived t2 either way: the operator is still connected.
        drop(e);
        assert_eq!(
            f.station
                .authority
                .stream_admitted(SESSION, DEVICE, &f.lease, t2),
            Ok(())
        );
    }
}

/// A Stop at the station ends the transmission and leaves presence, so what the streamed operator
/// starts next is still guarded; releasing the lease ends presence, and the station halts.
#[test]
fn a_stop_keeps_presence_and_a_release_ends_it() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
    streaming.presence.renew(&f.station, &streaming.offer, now);
    tempo_app::engine::engine_lock(&f.station.engine).halt_tx();
    assert!(streaming.presence.live(now), "a local stop ended presence");
    let (answer, released) = streaming.control(
        &serde_json::to_vec(&serde_json::json!({
            "type":"release","requestId":id(),"leaseId":f.lease,
        }))
        .unwrap(),
        |_| true,
        now,
    );
    assert!(released, "the page's release ends the stream");
    assert!(reply_of(&answer.unwrap())["value"].is_object());
    assert!(!streaming.presence.live(now), "presence outlived the lease");
    {
        let mut e = tempo_app::engine::engine_lock(&f.station.engine);
        e.set_operating_mode("phone", false);
        e.set_ptt(true);
    }
    assert!(
        tempo_app::engine::engine_lock(&f.station.engine)
            .poll_remote_transmit(now + Duration::from_millis(20)),
        "the station did not halt when the stream's lease ended"
    );
}

// ----- S11 and S7: input and the held PTT only while presence is live -----

fn click() -> Vec<u8> {
    br#"{"type":"pointer","action":"down","x":0.5,"y":0.25,"button":0,"buttons":1,"modifiers":0,"pointerType":"mouse","clicks":1}"#.to_vec()
}

/// ★ Input reaches the station's own window, and only while presence is live (blind means no
/// authority). CONTROL: the same click with presence live is delivered.
#[test]
fn input_is_delivered_only_while_presence_is_live() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
    streaming.control(&click(), |_| true, now);
    assert!(
        f.delivered.lock().unwrap().is_empty(),
        "delivered with no presence"
    );
    streaming.presence.renew(&f.station, &streaming.offer, now);
    streaming.control(&click(), |_| true, now);
    assert_eq!(
        f.delivered.lock().unwrap().len(),
        1,
        "control: delivered with presence"
    );
    // Lapsed: blind again.
    streaming.control(&click(), |_| true, now + Duration::from_secs(5));
    assert_eq!(
        f.delivered.lock().unwrap().len(),
        1,
        "delivered after presence lapsed"
    );
}

/// When presence lapses, the window is told to let go of whatever the operator held down, once
/// per lapse; a renewal re-arms it. CONTROL: nothing is reset while presence is live.
#[test]
fn a_lapse_of_presence_resets_the_window_once() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
    let resets = || {
        f.delivered
            .lock()
            .unwrap()
            .iter()
            .filter(|i| matches!(i, WebviewInput::Reset {}))
            .count()
    };
    streaming.presence.renew(&f.station, &streaming.offer, now);
    streaming.watch_presence(now);
    assert_eq!(resets(), 0, "control: reset while presence was live");
    // A stale picture keeps the lease and lets presence lapse.
    let stale = now + Duration::from_secs(3);
    streaming.control(&heartbeat(&f.lease, Some(1)), |_| false, stale);
    let lapsed = now + Duration::from_secs(5);
    streaming.watch_presence(lapsed);
    assert_eq!(resets(), 1, "the lapse did not reset the window");
    streaming.watch_presence(lapsed + Duration::from_millis(500));
    assert_eq!(resets(), 1, "one lapse, one reset");
    // A fresh picture again: presence is back, and its next lapse is a second reset.
    let again = now + Duration::from_secs(6);
    streaming.control(&heartbeat(&f.lease, Some(1)), |_| true, again);
    assert!(streaming.presence.live(again), "premise: presence renewed");
    streaming.watch_presence(again);
    assert_eq!(resets(), 1);
    streaming.watch_presence(again + Duration::from_secs(5));
    assert_eq!(resets(), 2);
}

/// A held PTT is taken only while presence is live, and keys through the engine.
#[test]
fn a_held_ptt_is_taken_only_while_presence_is_live() {
    let now = Instant::now();
    let f = fixture(now);
    tempo_app::engine::engine_lock(&f.station.engine).set_operating_mode("phone", false);
    let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
    let hold = format!(r#"{{"type":"pttHold","holdId":"{PRESS}","seq":0}}"#);
    streaming.ptt(hold.as_bytes(), now);
    tempo_app::engine::engine_lock(&f.station.engine).poll_remote_transmit(now);
    assert!(
        !tempo_app::engine::engine_lock(&f.station.engine).manual_ptt(),
        "keyed with no presence"
    );
    // The engine would refuse it too (a second lock); this is the FIRST one: the hold never
    // reached the held-PTT state at all, so there is not even a refusal to report.
    assert!(
        streaming.ptt_reports().is_empty(),
        "a hold with no presence reached the PTT"
    );
    // CONTROL: with presence, a fresh press keys.
    streaming.presence.renew(&f.station, &streaming.offer, now);
    let other = "10000000-0000-4000-8000-00000000000b";
    let hold = format!(r#"{{"type":"pttHold","holdId":"{other}","seq":0}}"#);
    streaming.ptt(hold.as_bytes(), now);
    tempo_app::engine::engine_lock(&f.station.engine).poll_remote_transmit(now);
    assert!(tempo_app::engine::engine_lock(&f.station.engine).manual_ptt());
    // …and the page is told so.
    let reports: Vec<Value> = streaming
        .ptt_reports()
        .iter()
        .map(|r| reply_of(r))
        .collect();
    assert!(reports
        .iter()
        .any(|r| r["holdId"] == other && r["keyed"] == true));
}

// ----- S10: the existing protocol on the data channel -----

/// Today's `state` and `stopTransmit` answer on the data channel exactly as on the relay, at
/// operation version 4, stamped with the session's own identity.
#[test]
fn state_and_stop_answer_as_they_do_on_the_relay() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
    let (answer, _) = streaming.control(
        &serde_json::to_vec(&serde_json::json!({"type":"state","requestId":id()})).unwrap(),
        |_| true,
        now,
    );
    let state = reply_of(&answer.unwrap())["value"].clone();
    assert_eq!(state["phase"], "controlling");
    let epoch = state["transmitEpoch"]
        .as_str()
        .expect("the stop token")
        .to_string();
    let (answer, _) = streaming.control(
        &serde_json::to_vec(&serde_json::json!({
            "type":"stopTransmit","requestId":id(),"stationBootId":state["stationBootId"],
            "leaseId":f.lease,"transmitEpoch":epoch,
        }))
        .unwrap(),
        |_| true,
        now,
    );
    assert_eq!(reply_of(&answer.unwrap())["value"]["stop"], "accepted");
}

/// The contract's own control messages all parse on the station; a malformed one is dropped
/// unanswered rather than taking the stream down.
#[test]
fn the_contract_messages_are_taken_and_a_malformed_one_is_dropped() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
    let (answer, released) = streaming.control(b"{not json", |_| true, now);
    assert!(answer.is_none() && !released);
    let file: Value = serde_json::from_str(include_str!(
        "../../../../remote/test/fixtures/stream/channel.json"
    ))
    .unwrap();
    for case in file["controlBrowserToStation"].as_array().unwrap() {
        let bytes = serde_json::to_vec(&case["message"]).unwrap();
        assert!(protocol::parse_control(&bytes).is_ok(), "{}", case["name"]);
    }
}
