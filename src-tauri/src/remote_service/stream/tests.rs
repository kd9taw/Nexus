//! The stream's station side, with a real operations authority and a real engine. What needs a
//! WebRTC session (the offer answered, DTLS, no LAN address) is tested in `tempo_stream::session`
//! on Windows; everything here runs on every platform.
use super::*;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use std::sync::{Mutex, OnceLock};

const DEVICE: &str = "10000000-0000-4000-8000-000000000001";
const SESSION: &str = "20000000-0000-4000-8000-000000000001";
const STATION: &str = "30000000-0000-4000-8000-000000000001";
const PRESS: &str = "10000000-0000-4000-8000-00000000000a";

/// A browser's device key (A5), made for the test run and never written down: the page's
/// non-extractable WebCrypto key, played by ring.
pub(in crate::remote_service) struct DeviceKey {
    pair: EcdsaKeyPair,
    /// SPKI, lowercase hex, as the page sends it.
    pub public_key: String,
}

impl DeviceKey {
    pub fn new() -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        let point: String = pair
            .public_key()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Self {
            public_key: format!("{}{point}", protocol::P256_SPKI_PREFIX_HEX),
            pair,
        }
    }

    /// What the station pins for it: SHA-256 of its SPKI.
    pub fn pin(&self) -> [u8; 32] {
        digest(&SHA256, &protocol::hex_bytes(&self.public_key).unwrap())
            .as_ref()
            .try_into()
            .unwrap()
    }

    /// Its signature over `sdp`'s fingerprint and the three ids, made the way the page makes it.
    pub fn sign(&self, sdp: &str, station: &str, device: &str, session: &str) -> String {
        let fingerprint = protocol::offer_fingerprint(sdp).unwrap();
        let hashed: [u8; 32] = digest(&SHA256, &fingerprint).as_ref().try_into().unwrap();
        let signed = protocol::offer_binding(&hashed, station, device, session);
        self.pair
            .sign(&SystemRandom::new(), &signed)
            .unwrap()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// The key every fixture's browser holds and every fixture's station pinned for it.
fn key() -> &'static DeviceKey {
    static KEY: OnceLock<DeviceKey> = OnceLock::new();
    KEY.get_or_init(DeviceKey::new)
}

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
    /// The keep-awake request as the OS would see it: every hold and release, in order.
    awake: Arc<Mutex<Vec<&'static str>>>,
}

/// The OS side of the keep-awake request, recorded.
struct AwakeRecorder(Arc<Mutex<Vec<&'static str>>>);

impl tempo_stream::keep_awake::Power for AwakeRecorder {
    fn hold(&mut self) -> Result<(), String> {
        self.0.lock().unwrap().push("hold");
        Ok(())
    }
    fn release(&mut self) -> Result<(), String> {
        self.0.lock().unwrap().push("release");
        Ok(())
    }
}

fn fixture(now: Instant) -> Fixture {
    let authority = Arc::new(Authority::default());
    let mut engine = tempo_app::engine::Engine::new("W9XYZ", "EN52", 0);
    engine.set_remote_transmit_revocation(authority.transmit_revocation());
    let ptt = PttHold::default();
    engine.set_remote_ptt_hold(ptt.clone());
    let mic = tempo_app::mic::MicFeed::default();
    engine.set_remote_mic_feed(mic.clone());
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
    let awake = Arc::new(Mutex::new(Vec::new()));
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
                mic,
                window: None,
                awake: tempo_stream::keep_awake::KeepAwake::new(
                    Box::new(AwakeRecorder(awake.clone())),
                    Arc::new(|_: &str| {}),
                ),
            },
            #[cfg(feature = "radio")]
            audio: None,
            station_id: STATION.into(),
            // The operator approved this browser at the radio, and pinned its key (A5).
            pinned: Arc::new(|device: &str| (device == DEVICE).then(|| key().pin())),
        },
        lease: acquired["leaseId"].as_str().unwrap().to_string(),
        delivered,
        awake,
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

/// The page's offer, signed by the fixture browser's key for this station, device and session.
fn offer(lease: &str) -> Offer {
    let sdp = contract_offer();
    Offer {
        session: SESSION.into(),
        device: DEVICE.into(),
        lease: lease.into(),
        public_key: Some(key().public_key.clone()),
        signature: Some(key().sign(&sdp, STATION, DEVICE, SESSION)),
        sdp,
    }
}

/// A streamed session whose page's certificate was the one it signed (A5 after DTLS): where the
/// tests of S7 to S11 start. A5's own tests start from `Streaming::new`.
fn verified_stream(f: &Fixture, now: Instant) -> Streaming {
    let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
    streaming.verified = true;
    streaming
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
    let mut streaming = verified_stream(&f, now);
    let tick = now + RECHECK;
    assert_eq!(
        streaming.still_admitted(tick),
        Ok(()),
        "control: admitted while granted"
    );
    f.station.authority.permit_station(DEVICE, false).unwrap();
    assert_eq!(
        streaming.still_admitted(tick + RECHECK),
        Err(StreamReason::NotController),
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

/// The operator's switch, turned off at the shack.
fn turn_streaming_off(f: &Fixture) {
    let mut e = tempo_app::engine::engine_lock(&f.station.engine);
    let mut s = e.settings().clone();
    s.remote_stream = false;
    e.apply_settings(s);
}

/// The operator's kill switch: turning streaming off at the shack ends the stream already running,
/// at its next recheck, under the reason the page shows as "Streaming is off at the shack". The
/// control is the same stream while the switch is on.
#[test]
fn turning_streaming_off_at_the_shack_ends_a_running_stream() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = verified_stream(&f, now);
    let tick = now + RECHECK;
    assert_eq!(
        streaming.still_admitted(tick),
        Ok(()),
        "control: admitted while the switch is on"
    );
    turn_streaming_off(&f);
    assert_eq!(
        streaming.still_admitted(tick + RECHECK),
        Err(StreamReason::StreamDisabled),
        "the stream outlived the operator's switch"
    );
}

/// The session thread never waits for the engine. A busy engine at the recheck is not the switch
/// turned off, and the switch is read again on the next tick, not a whole recheck later.
#[test]
fn a_busy_engine_is_not_the_switch_turned_off_and_is_asked_again_next_tick() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = verified_stream(&f, now);
    turn_streaming_off(&f);
    let tick = now + RECHECK;
    {
        let _busy = tempo_app::engine::engine_lock(&f.station.engine);
        assert_eq!(
            streaming.still_admitted(tick),
            Ok(()),
            "a busy engine read as the switch turned off"
        );
    }
    assert_eq!(
        streaming.still_admitted(tick + TICK),
        Err(StreamReason::StreamDisabled),
        "a busy engine put the switch off for a whole recheck"
    );
}

/// A settings document from 1.15.0, the release before the stream: the default document that
/// release's UI fixtures carried (`git show v1.15.0:ui/src/components/__fixtures__/defaultSettings.json`).
/// It predates the switch's key.
const SETTINGS_1_15_0: &str = include_str!("../../../tests/fixtures/settings-1.15.0.json");

/// ★ OFF BY DEFAULT, on a fresh install and on an update from 1.15.0 (the operator's ruling of
/// 2026-10-02: the stream ships "off by default"). Nothing here turns the switch off. Each install's
/// settings come through `Settings::load`, the path the app starts on (migrations included): no file
/// at all, as on a first run, and 1.15.0's document. Each leaves the switch off, and a browser that
/// is otherwise admitted (Remote on, approved for station controls at the radio, holding control
/// under a live lease) is refused `streamDisabled`, the check before anything of a session exists.
/// The CONTROL is the same offer once the operator turns the switch on.
#[test]
fn streaming_stays_off_on_a_fresh_install_and_after_an_update_until_it_is_turned_on() {
    let now = Instant::now();
    let f = fixture(now);
    let apply = |settings: tempo_app::settings::Settings| {
        tempo_app::engine::engine_lock(&f.station.engine).apply_settings(settings);
    };
    let dir = std::env::temp_dir().join(format!("nexus-stream-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // A fresh install: no settings file yet.
    let fresh = tempo_app::settings::Settings::load(&dir.join("settings.json"));
    assert!(!fresh.remote_stream, "a fresh install's switch is on");
    apply(fresh);
    assert_eq!(
        admit(&f.station, &offer(&f.lease), now),
        Err(StreamReason::StreamDisabled),
        "a fresh install streams before the switch is turned on"
    );

    // An update from 1.15.0: that release's settings file, loaded by this one.
    assert!(
        !SETTINGS_1_15_0.contains("remoteStream"),
        "premise: 1.15.0 had no switch"
    );
    let path = dir.join("settings-1.15.0.json");
    std::fs::write(&path, SETTINGS_1_15_0).unwrap();
    let updated = tempo_app::settings::Settings::load(&path);
    assert!(
        !updated.remote_stream,
        "the update from 1.15.0 turned the switch on"
    );
    apply(updated.clone());
    assert_eq!(
        admit(&f.station, &offer(&f.lease), now),
        Err(StreamReason::StreamDisabled),
        "an update from 1.15.0 streams before the switch is turned on"
    );

    // CONTROL: the operator turns it on at the shack, and the same offer gets past admission.
    let mut on = updated;
    on.remote_stream = true;
    apply(on);
    assert_eq!(admit(&f.station, &offer(&f.lease), now), passed());
    let _ = std::fs::remove_dir_all(&dir);
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
        let mut streaming = verified_stream(&f, t0);
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
    let mut streaming = verified_stream(&f, now);
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
    let mut streaming = verified_stream(&f, now);
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
    let mut streaming = verified_stream(&f, now);
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

/// A held PTT is taken only while presence is live, and ARMS the microphone over through the
/// engine (M1: the page's audio keys it, not the hold).
#[test]
fn a_held_ptt_is_taken_only_while_presence_is_live() {
    let now = Instant::now();
    let f = fixture(now);
    tempo_app::engine::engine_lock(&f.station.engine).set_operating_mode("phone", false);
    let mut streaming = verified_stream(&f, now);
    let hold = format!(r#"{{"type":"pttHold","holdId":"{PRESS}","seq":0}}"#);
    streaming.ptt(hold.as_bytes(), now);
    tempo_app::engine::engine_lock(&f.station.engine).poll_remote_transmit(now);
    assert!(
        !tempo_app::engine::engine_lock(&f.station.engine).mic_armed(),
        "armed with no presence"
    );
    // The engine would refuse it too (a second lock); this is the FIRST one: the hold never
    // reached the held-PTT state at all, so there is not even a refusal to report.
    assert!(
        streaming.ptt_reports().is_empty(),
        "a hold with no presence reached the PTT"
    );
    // CONTROL: with presence, a fresh press arms the microphone, and keys nothing by itself.
    streaming.presence.renew(&f.station, &streaming.offer, now);
    let other = "10000000-0000-4000-8000-00000000000b";
    let hold = format!(r#"{{"type":"pttHold","holdId":"{other}","seq":0}}"#);
    streaming.ptt(hold.as_bytes(), now);
    tempo_app::engine::engine_lock(&f.station.engine).poll_remote_transmit(now);
    {
        let e = tempo_app::engine::engine_lock(&f.station.engine);
        assert!(e.mic_armed(), "a press with presence did not arm");
        assert!(
            !e.mic_keyed() && !e.manual_ptt(),
            "the press alone keyed the station"
        );
    }
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
    let mut streaming = verified_stream(&f, now);
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
    let mut streaming = verified_stream(&f, now);
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

// ----- The relay ends a stream: Remote access switched off -----

/// A running stream's lane, as `StreamLane::signal` leaves it: a session thread waiting on its
/// inbox. The stand-in thread hands back the first signal it gets.
fn running_lane() -> (StreamLane, mpsc::Receiver<Signal>) {
    let (signals, inbox) = mpsc::channel::<Signal>();
    let (seen, told) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        if let Ok(signal) = inbox.recv() {
            let _ = seen.send(signal);
        }
    });
    let lane = StreamLane {
        live: Some(Live {
            session: SESSION.into(),
            signals,
            thread,
        }),
    };
    (lane, told)
}

/// ★ Remote access switched off mid-stream (operator, 2026-09-27: "Within about 2 s
/// (Recommended)", offered as "The relay tells the station to end that stream immediately"). The
/// relay's `streamEnd` ends the session's transmit presence on the relay's own thread, at once,
/// so a transmission halts on the radio loop's next poll whatever the session thread is doing,
/// and the session thread is told to tear down with the relay's reason. CONTROL: until the
/// message arrives the same transmission is not halted; and a `streamEnd` for another session
/// ends nothing.
#[test]
fn the_relays_stream_end_ends_presence_and_halts_a_transmission() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = verified_stream(&f, now);
    streaming.presence.renew(&f.station, &streaming.offer, now);
    {
        let mut e = tempo_app::engine::engine_lock(&f.station.engine);
        e.set_operating_mode("phone", false);
        e.set_ptt(true);
        assert!(e.manual_ptt(), "premise: keyed");
    }
    let tick = now + Duration::from_millis(20);
    assert!(
        !tempo_app::engine::engine_lock(&f.station.engine).poll_remote_transmit(tick),
        "control: halted before the relay said anything"
    );
    let (mut lane, told) = running_lane();
    // Another session's end touches nothing.
    lane.end(
        &f.station,
        "20000000-0000-4000-8000-000000000009",
        StreamReason::RemoteOff,
    );
    assert!(
        streaming.presence.live(tick),
        "another session's end ended presence"
    );
    lane.end(&f.station, SESSION, StreamReason::RemoteOff);
    assert!(
        !streaming.presence.live(tick),
        "presence outlived the relay's streamEnd"
    );
    assert!(
        tempo_app::engine::engine_lock(&f.station.engine).poll_remote_transmit(tick),
        "the station did not halt on the relay's streamEnd"
    );
    assert!(!tempo_app::engine::engine_lock(&f.station.engine).manual_ptt());
    assert_eq!(
        told.recv_timeout(Duration::from_secs(1)),
        Ok(Signal::Close(StreamReason::RemoteOff)),
        "the session thread was not told to end, with the relay's reason"
    );
}

/// S6 end to end on the station: a packet of the page's microphone is decoded on the session's
/// thread and reaches the engine, where the first one after the press keys the over (M1). A packet
/// that arrives before the press goes nowhere (M5): the over it would have keyed does not exist yet.
#[cfg(feature = "radio")]
#[test]
fn the_pages_microphone_keys_only_an_armed_over() {
    use tempo_app::mic::MicTick;
    let now = Instant::now();
    let f = fixture(now);
    tempo_app::engine::engine_lock(&f.station.engine).set_operating_mode("phone", false);
    let mut streaming = verified_stream(&f, now);
    streaming.presence.renew(&f.station, &streaming.offer, now);
    // 20 ms of Opus silence (CELT, fullband): what a browser sends between words.
    let packet = |seq: u64| tempo_stream::session::MicPacket {
        seq,
        rtp: 960 * seq,
        clock_hz: 48_000,
        arrived: now,
        payload: vec![0xF8, 0xFF, 0xFE],
    };
    let poll = |at: Instant| {
        let mut e = tempo_app::engine::engine_lock(&f.station.engine);
        e.poll_remote_transmit(at);
        e.poll_mic(at, 40.0, 0)
    };
    // Before any press: the packet goes nowhere, and nothing is armed to key.
    streaming.mic(&packet(1));
    assert_eq!(
        poll(now),
        MicTick::Idle,
        "audio with no press did something"
    );
    // The press arms; the packet that came before it is gone.
    let hold = format!(r#"{{"type":"pttHold","holdId":"{PRESS}","seq":0}}"#);
    streaming.ptt(hold.as_bytes(), now);
    assert_eq!(
        poll(now),
        MicTick::Armed,
        "a press with no new audio did something"
    );
    // The page's next packet keys it.
    streaming.mic(&packet(2));
    assert_eq!(
        poll(now + Duration::from_millis(20)),
        MicTick::Key,
        "the page's audio did not key the armed over"
    );
}

/// The page is told what the station's microphone over is doing (`micState`), when it changes and
/// only then: armed by the press, keyed by the voice, and its end with why. Nothing is said about
/// an over from before the session.
#[cfg(feature = "radio")]
#[test]
fn the_page_is_told_what_the_microphone_over_does() {
    let now = Instant::now();
    let f = fixture(now);
    tempo_app::engine::engine_lock(&f.station.engine).set_operating_mode("phone", false);
    let mut streaming = verified_stream(&f, now);
    streaming.presence.renew(&f.station, &streaming.offer, now);
    assert_eq!(
        streaming.mic_state(),
        None,
        "told of an over that does not exist"
    );
    let told = |streaming: &mut Streaming| -> Value {
        serde_json::from_str(
            &streaming
                .mic_state()
                .expect("a change the page was not told of"),
        )
        .unwrap()
    };
    let poll = |at: Instant| {
        let mut e = tempo_app::engine::engine_lock(&f.station.engine);
        e.poll_remote_transmit(at);
        e.poll_mic(at, 40.0, 0)
    };
    let hold = format!(r#"{{"type":"pttHold","holdId":"{PRESS}","seq":0}}"#);
    streaming.ptt(hold.as_bytes(), now);
    poll(now);
    assert_eq!(
        told(&mut streaming),
        serde_json::json!({"type":"micState","armed":true,"keyed":false,"noPowerOut":false})
    );
    assert_eq!(streaming.mic_state(), None, "told twice of one change");
    streaming.mic(&tempo_stream::session::MicPacket {
        seq: 1,
        rtp: 960,
        clock_hz: 48_000,
        arrived: now,
        payload: vec![0xF8, 0xFF, 0xFE],
    });
    poll(now + Duration::from_millis(20));
    assert_eq!(told(&mut streaming)["keyed"], true, "the key was not told");
    let release = format!(r#"{{"type":"pttRelease","holdId":"{PRESS}","seq":1}}"#);
    streaming.ptt(release.as_bytes(), now);
    poll(now + Duration::from_millis(40));
    assert_eq!(
        told(&mut streaming),
        serde_json::json!({"type":"micState","armed":false,"keyed":false,"noPowerOut":false,"ended":"released"})
    );
}

/// Every way an over ends reaches the page under its own name: the page's caption for each is
/// different (the audio design's §7), so two ends told as one would say the wrong thing.
#[test]
fn every_end_of_an_over_reaches_the_page_under_its_own_name() {
    use tempo_app::mic::MicEnded as Engine;
    for why in [
        Engine::Released,
        Engine::Stopped,
        Engine::AudioGap,
        Engine::Presence,
        Engine::Ceiling,
        Engine::Watchdog,
        Engine::RouteChanged,
    ] {
        let name = format!("{why:?}");
        let camel = name[..1].to_lowercase() + &name[1..];
        assert_eq!(
            serde_json::to_value(wire_ended(why)).unwrap(),
            Value::String(camel),
            "{why:?}"
        );
    }
}

/// The page's `held` set reaches the window as it came, and only while presence is live: it is
/// input. CONTROL: the same message with presence live is delivered.
#[test]
fn a_held_set_reaches_the_window_only_while_presence_is_live() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = verified_stream(&f, now);
    let held = br#"{"type":"held","keys":["Space"],"buttons":1,"seq":3}"#;
    streaming.ptt(held, now);
    assert!(f.delivered.lock().unwrap().is_empty(), "delivered blind");
    streaming.presence.renew(&f.station, &streaming.offer, now);
    streaming.ptt(held, now);
    let delivered = f.delivered.lock().unwrap().clone();
    assert_eq!(delivered.len(), 1, "control: delivered with presence");
    assert_eq!(
        serde_json::to_value(&delivered[0]).unwrap(),
        serde_json::from_slice::<Value>(held).unwrap(),
        "the window got something other than what the page sent"
    );
}

// ----- The lead's rulings: which heartbeat renews what, blind stops, B -----

fn keyed(f: &Fixture) {
    let mut e = tempo_app::engine::engine_lock(&f.station.engine);
    e.set_operating_mode("phone", false);
    e.set_ptt(true);
    assert!(e.manual_ptt(), "premise: keyed");
}

fn poll(f: &Fixture, at: Instant) -> bool {
    tempo_app::engine::engine_lock(&f.station.engine).poll_remote_transmit(at)
}

/// ★ Ruling 1: while a stream is attached, only a `control` heartbeat with a fresh picture
/// renews presence. The observe socket's own heartbeat, which the relay's operation lane
/// delivers exactly as below, keeps the LEASE and never presence, so a socket that lives while
/// the picture is frozen cannot keep the station transmitting. CONTROL: a fresh `control`
/// heartbeat at the same moment keeps both, and the station does not halt.
#[test]
fn a_socket_heartbeat_keeps_the_lease_and_not_presence() {
    for fresh_on_control in [false, true] {
        let t0 = Instant::now();
        let f = fixture(t0);
        let mut streaming = verified_stream(&f, t0);
        streaming.presence.renew(&f.station, &streaming.offer, t0);
        keyed(&f);
        let t1 = t0 + Duration::from_secs(3);
        if fresh_on_control {
            streaming.control(&heartbeat(&f.lease, Some(1)), |_| true, t1);
        } else {
            f.station
                .authority
                .handle_version(
                    (f.station.connection, OPERATION_VERSION),
                    SESSION,
                    DEVICE,
                    &Request::Heartbeat {
                        request_id: id(),
                        lease_id: f.lease.clone(),
                    },
                    &f.station.engine,
                    t1,
                )
                .expect("the socket heartbeat renews the lease");
        }
        let t2 = t0 + Duration::from_secs(5);
        assert_eq!(
            poll(&f, t2),
            !fresh_on_control,
            "fresh on control: {fresh_on_control}"
        );
        // The lease outlived t2 either way.
        assert_eq!(
            f.station
                .authority
                .stream_admitted(SESSION, DEVICE, &f.lease, t2),
            Ok(())
        );
    }
}

/// ★ Ruling A, and its exception: blind input is refused, and a blind Stop still stops. With no
/// presence (the picture never confirmed), a click is not delivered, but `stopTransmit` on
/// `control` is admitted and halts what is on the air. CONTROL: the click with presence live is
/// delivered (`input_is_delivered_only_while_presence_is_live`).
#[test]
fn blind_input_is_refused_and_a_blind_stop_still_stops() {
    let now = Instant::now();
    let f = fixture(now);
    let mut streaming = verified_stream(&f, now);
    keyed(&f);
    streaming.control(&click(), |_| true, now);
    assert!(
        f.delivered.lock().unwrap().is_empty(),
        "blind input delivered"
    );
    assert!(!streaming.presence.live(now), "premise: blind");
    let stop = stop_request(&mut streaming, &f.lease, now);
    let (answer, _) = streaming.control(&stop, |_| true, now);
    assert_eq!(
        reply_of(&answer.unwrap())["value"]["stop"],
        "accepted",
        "a blind Stop was refused"
    );
    assert!(
        !tempo_app::engine::engine_lock(&f.station.engine).manual_ptt(),
        "a blind Stop did not stop"
    );
}

/// The same with the lease lapsed too: a browser that held control still stops (the relay path's
/// ruling of 2026-09-15, which the stream inherits). It needs the current stop token, which
/// `state` hands an expired controller as it does a live one.
#[test]
fn a_stop_after_the_lease_lapsed_still_stops() {
    let t0 = Instant::now();
    let f = fixture(t0);
    let mut streaming = verified_stream(&f, t0);
    keyed(&f);
    let later = t0 + Duration::from_secs(8);
    assert!(
        f.station
            .authority
            .stream_admitted(SESSION, DEVICE, &f.lease, later)
            .is_err(),
        "premise: the lease has lapsed"
    );
    let stop = stop_request(&mut streaming, &f.lease, later);
    let (answer, _) = streaming.control(&stop, |_| true, later);
    assert_eq!(
        reply_of(&answer.unwrap())["value"]["stop"],
        "accepted",
        "a Stop with the lease lapsed was refused"
    );
    assert!(!tempo_app::engine::engine_lock(&f.station.engine).manual_ptt());
}

/// A `stopTransmit` as the page sends it, with the stop token from a `state` asked just before.
fn stop_request(streaming: &mut Streaming, lease: &str, now: Instant) -> Vec<u8> {
    let (answer, _) = streaming.control(
        &serde_json::to_vec(&serde_json::json!({"type":"state","requestId":id()})).unwrap(),
        |_| true,
        now,
    );
    let state = reply_of(&answer.unwrap())["value"].clone();
    let epoch = state["transmitEpoch"]
        .as_str()
        .expect("state hands the stop token")
        .to_string();
    serde_json::to_vec(&serde_json::json!({
        "type":"stopTransmit","requestId":id(),"stationBootId":state["stationBootId"],
        "leaseId":lease,"transmitEpoch":epoch,
    }))
    .unwrap()
}

/// ★ Question B, answered by the operator on 2026-09-27, verbatim: "Control grant is enough",
/// offered as "As the plan wrote it: the tick only mattered for the old page; any controller can
/// stream and transmit." So a browser with the station-control grant and a live lease, and
/// WITHOUT the FT8/FT4 transmit tick, is admitted to stream. Deliberate: change it only with the
/// operator. CONTROL: without the control grant the same browser is refused.
#[test]
fn a_controller_without_the_transmit_tick_is_admitted() {
    let now = Instant::now();
    let f = fixture(now);
    f.station.authority.permit_transmit(DEVICE, false).unwrap();
    assert_eq!(
        f.station
            .authority
            .stream_admitted(SESSION, DEVICE, &f.lease, now),
        Ok(())
    );
    assert_eq!(admit(&f.station, &offer(&f.lease), now), passed());
    f.station.authority.permit_station(DEVICE, false).unwrap();
    assert_eq!(
        admit(&f.station, &offer(&f.lease), now),
        Err(StreamReason::NotController)
    );
}

/// ★ S8 (d): a lease lapse ends presence. Presence is minted no later than the lease runs, so
/// renewing presence alone cannot outlive it, and once the lease has lapsed nothing renews
/// presence at all. CONTROL: up at 4.999 s, down at 5.000 s, the lease's own end.
#[test]
fn a_lapsed_lease_ends_presence() {
    let t0 = Instant::now();
    let f = fixture(t0);
    let mut streaming = verified_stream(&f, t0);
    streaming.presence.renew(&f.station, &streaming.offer, t0);
    keyed(&f);
    // Four seconds in, presence alone is renewed (no heartbeat, so the lease is not): it is capped
    // at the lease's end, five seconds after it was acquired.
    streaming
        .presence
        .renew(&f.station, &streaming.offer, t0 + Duration::from_secs(4));
    assert!(!poll(&f, t0 + Duration::from_millis(4999)), "halted early");
    assert!(
        poll(&f, t0 + Duration::from_secs(5)),
        "presence outlived the lease"
    );
    let after = t0 + Duration::from_secs(6);
    streaming
        .presence
        .renew(&f.station, &streaming.offer, after);
    assert!(
        !streaming.presence.live(after),
        "presence renewed with no lease"
    );
}

// ----- A5: the offer is the approved browser's own, and so is the certificate it connects with -----

/// The fixture offer, carrying `signer`'s key and its signature made for `ids` (station, device,
/// session).
fn signed_by(f: &Fixture, signer: &DeviceKey, ids: (&str, &str, &str)) -> Offer {
    let mut offer = offer(&f.lease);
    offer.public_key = Some(signer.public_key.clone());
    offer.signature = Some(signer.sign(&offer.sdp, ids.0, ids.1, ids.2));
    offer
}

const ELSEWHERE: &str = "40000000-0000-4000-8000-000000000001";

/// ★ A5's positive control: the offer the pinned key signed for this station, browser and session
/// is admitted; what is left is the platform's own answer.
#[test]
fn a5_the_correctly_signed_offer_is_admitted() {
    let now = Instant::now();
    let f = fixture(now);
    assert_eq!(
        admit(
            &f.station,
            &signed_by(&f, key(), (STATION, DEVICE, SESSION)),
            now
        ),
        passed()
    );
}

/// ★ A5: a signature from any key but the one the operator pinned is refused at admission: another
/// key's own offer, and the pinned key's public half carrying another key's signature. CONTROL: the
/// pinned key's own offer.
#[test]
fn a5_a_signature_from_a_different_key_is_refused() {
    let now = Instant::now();
    let f = fixture(now);
    let other = DeviceKey::new();
    assert_eq!(
        admit(
            &f.station,
            &signed_by(&f, &other, (STATION, DEVICE, SESSION)),
            now
        ),
        Err(StreamReason::DeviceKeyMismatch)
    );
    let mut borrowed = signed_by(&f, &other, (STATION, DEVICE, SESSION));
    borrowed.public_key = Some(key().public_key.clone());
    assert_eq!(
        admit(&f.station, &borrowed, now),
        Err(StreamReason::DeviceKeyMismatch)
    );
    assert_eq!(
        admit(
            &f.station,
            &signed_by(&f, key(), (STATION, DEVICE, SESSION)),
            now
        ),
        passed(),
        "control"
    );
}

/// ★ A5: the pinned key's own, correct signature, made for another session, another browser or
/// another station, is refused: a signature is good for the one offer it was made for.
#[test]
fn a5_a_correct_signature_over_another_session_device_or_station_is_refused() {
    let now = Instant::now();
    let f = fixture(now);
    for ids in [
        (STATION, DEVICE, ELSEWHERE),
        (STATION, ELSEWHERE, SESSION),
        (ELSEWHERE, DEVICE, SESSION),
    ] {
        assert_eq!(
            admit(&f.station, &signed_by(&f, key(), ids), now),
            Err(StreamReason::DeviceKeyMismatch),
            "{ids:?}"
        );
    }
    assert_eq!(
        admit(
            &f.station,
            &signed_by(&f, key(), (STATION, DEVICE, SESSION)),
            now
        ),
        passed(),
        "control"
    );
}

/// ★ A5: an offer whose DTLS fingerprint is not the one its signature covers is refused: whoever
/// swaps in a certificate of their own cannot keep the browser's signature.
#[test]
fn a5_an_offer_whose_fingerprint_differs_from_the_signed_one_is_refused() {
    let now = Instant::now();
    let f = fixture(now);
    let mut swapped = signed_by(&f, key(), (STATION, DEVICE, SESSION));
    let signed = protocol::offer_fingerprint(&swapped.sdp).unwrap();
    let line = swapped
        .sdp
        .lines()
        .find(|l| l.starts_with("a=fingerprint:sha-256 "))
        .unwrap()
        .to_string();
    let flipped: Vec<String> = signed.iter().map(|b| format!("{:02X}", b ^ 0xff)).collect();
    swapped.sdp = swapped.sdp.replace(
        &line,
        &format!("a=fingerprint:sha-256 {}", flipped.join(":")),
    );
    assert!(
        protocol::offer_fingerprint(&swapped.sdp).is_some_and(|moved| moved != signed),
        "premise: every section's fingerprint moved"
    );
    assert_eq!(
        admit(&f.station, &swapped, now),
        Err(StreamReason::DeviceKeyMismatch)
    );
}

/// ★ A5: a browser the operator has no pinned key for (approved before the key existed) is refused
/// by name, `deviceNotPinned`, signed or not, so the page can say "approve this browser again at
/// the radio". A pinned browser's unsigned offer is a key that does not match.
#[test]
fn a5_a_browser_with_no_pinned_key_is_refused_by_name() {
    let now = Instant::now();
    let mut f = fixture(now);
    let mut unsigned = offer(&f.lease);
    unsigned.public_key = None;
    unsigned.signature = None;
    assert_eq!(
        admit(&f.station, &unsigned, now),
        Err(StreamReason::DeviceKeyMismatch)
    );
    f.station.pinned = Arc::new(|_: &str| None);
    assert_eq!(
        admit(&f.station, &offer(&f.lease), now),
        Err(StreamReason::DeviceNotPinned)
    );
    assert_eq!(
        admit(&f.station, &unsigned, now),
        Err(StreamReason::DeviceNotPinned)
    );
}

/// ★ A5 after DTLS: a page whose DTLS presents a certificate other than the one it signed (or none)
/// is refused at `connected`, which the session loop turns into the end of the session. Before
/// that, and after it, nothing it sends on `control` or `ptt` is taken, even with presence forced
/// live. CONTROL: the signed certificate connects, and the same click and hold are taken.
#[test]
fn a5_after_dtls_a_mismatched_certificate_ends_the_session_before_any_input_or_ptt() {
    let now = Instant::now();
    let f = fixture(now);
    tempo_app::engine::engine_lock(&f.station.engine).set_operating_mode("phone", false);
    let signed = protocol::offer_fingerprint(&offer(&f.lease).sdp).unwrap();
    let mut other = signed;
    other[0] ^= 0xff;
    let hold = format!(r#"{{"type":"pttHold","holdId":"{PRESS}","seq":0}}"#);
    for presented in [Some(other), None] {
        let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
        assert_eq!(
            streaming.connected(presented, now),
            Err(StreamReason::DeviceKeyMismatch)
        );
        assert!(
            !streaming.presence.live(now),
            "presence for a page not checked"
        );
        // Presence forced live, as if something renewed it anyway: still nothing is taken.
        streaming.presence.renew(&f.station, &streaming.offer, now);
        assert!(streaming.presence.live(now), "premise: presence live");
        let (answer, _) = streaming.control(&click(), |_| true, now);
        assert!(answer.is_none());
        let (answer, _) = streaming.control(&heartbeat(&f.lease, Some(1)), |_| true, now);
        assert!(
            answer.is_none(),
            "a heartbeat answered for a page not checked"
        );
        streaming.ptt(hold.as_bytes(), now);
        tempo_app::engine::engine_lock(&f.station.engine).poll_remote_transmit(now);
        assert!(
            f.delivered.lock().unwrap().is_empty(),
            "input from a page not checked"
        );
        assert!(
            !tempo_app::engine::engine_lock(&f.station.engine).manual_ptt(),
            "keyed by a page not checked"
        );
        assert!(
            streaming.ptt_reports().is_empty(),
            "a hold from a page not checked reached the PTT"
        );
    }
    let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
    assert_eq!(streaming.connected(Some(signed), now), Ok(()), "control");
    assert!(
        streaming.presence.live(now),
        "control: presence from the moment it connected"
    );
    streaming.control(&click(), |_| true, now);
    assert_eq!(
        f.delivered.lock().unwrap().len(),
        1,
        "control: the click is taken"
    );
    streaming.ptt(hold.as_bytes(), now);
    tempo_app::engine::engine_lock(&f.station.engine).poll_remote_transmit(now);
    // Under M1 a hold ARMS the microphone over and the operator's voice keys it, so the control is
    // the same observable the unchecked page must leave empty: the hold reached the PTT.
    assert!(
        !streaming.ptt_reports().is_empty(),
        "control: the hold reaches the PTT"
    );
}

/// ★ P5: a streamed page hears the station while a browser Listens on the relay, both through the
/// station's one encoder. Before the fan-out, the stream's `audio` channel was refused `audioInUse`
/// while the Listen lane held the feed's one reader.
#[cfg(feature = "radio")]
#[test]
fn the_stream_hears_the_station_while_the_listen_lane_does() {
    use crate::remote_service::audio::{AudioLane, ReceiveFanout};
    use tempo_audio::receive_encode::{DetachedFeed, FRAME_MS};
    let now = Instant::now();
    let mut f = fixture(now);
    let feed = DetachedFeed::new(48_000);
    let receive = ReceiveFanout::new(feed.feed.clone());
    f.station.audio = Some(receive.clone());
    // The relay's Listen lane is already listening.
    let mut listen = AudioLane::default();
    listen
        .start(&receive, SESSION, DEVICE, &f.lease, now)
        .unwrap();
    let mut streaming = verified_stream(&f, now);
    let said: Value = serde_json::from_str(&streaming.audio_open(now)).unwrap();
    assert_eq!(
        said,
        serde_json::json!({"type": "audioState", "listening": true}),
        "the stream was refused while the Listen lane listened"
    );
    // Both are fed, bundle for bundle.
    let tick = 48_000 * FRAME_MS as usize / 1000;
    let (mut heard, mut streamed) = (0, 0);
    for k in 0..30 {
        let at = now + Duration::from_millis(FRAME_MS * k as u64);
        let tone: Vec<f32> = (0..tick)
            .map(|i| 0.3 * (((k * tick + i) as f32) * 0.09).sin())
            .collect();
        feed.publish(at, &tone);
        heard += usize::from(listen.poll(at, true).message.is_some());
        streamed += usize::from(streaming.audio_due(at).is_some());
    }
    assert!(
        heard >= 8 && streamed == heard,
        "the Listen lane sent {heard} bundles and the stream {streamed}"
    );
}

// ----- The shack kept awake while a stream is attached (the operator's pick, 2026-09-28) -----

/// A stream's connection holds the shack awake, once, and its state going, which is how every end
/// of the session loop ends a stream, releases it. CONTROL: a connection refused at A5 never
/// holds it.
#[test]
fn a_connected_stream_keeps_the_shack_awake_until_it_is_gone() {
    let now = Instant::now();
    let f = fixture(now);
    let requests = || f.awake.lock().unwrap().clone();
    let signed = protocol::offer_fingerprint(&offer(&f.lease).sdp).unwrap();
    let mut other = signed;
    other[0] ^= 0xff;
    let mut refused = Streaming::new(f.station.clone(), offer(&f.lease), now);
    assert_eq!(
        refused.connected(Some(other), now),
        Err(StreamReason::DeviceKeyMismatch)
    );
    drop(refused);
    assert!(
        requests().is_empty(),
        "a connection refused at A5 held the shack awake: {:?}",
        requests()
    );
    let mut streaming = Streaming::new(f.station.clone(), offer(&f.lease), now);
    assert!(requests().is_empty(), "held before the connection");
    assert_eq!(streaming.connected(Some(signed), now), Ok(()));
    assert_eq!(
        requests(),
        ["hold"],
        "connected, and the shack not held awake"
    );
    drop(streaming);
    assert_eq!(
        requests(),
        ["hold", "release"],
        "the stream's end did not release it"
    );
}

// ── Stop TX is never refused (operator ruling, 2026-10-03) ─────────────────────────────────────
// "Stop TX always stops whatever is transmitting, even if it was pressed before the last halt."
// Every halt at the station runs `Engine::halt_tx`, which retires the remote transmit generation
// (`remote_transmit_stand_down` is `Authority::transmit_revocation`), and that generation is the
// `transmitEpoch` the page's Stop carries. Until this ruling the Stop was refused `staleContext`
// for carrying the one it was shown before the halt, so a transmission started in the second
// before the page re-read its token stayed up through the press, and the page said nothing.

/// Exclusive use of the process-wide satellite track badge, which every Stop reaches (it ends a
/// live track): the crate's one implementation, as the operations tests take it.
fn alone() -> std::sync::MutexGuard<'static, ()> {
    crate::sat_track_alone()
}

/// The scene: a streamed operator holding control and presence, the stop token as the page holds
/// it from its last state reply, then a halt at the station (`cause`), then a Tune started through
/// the picture before the page has re-read the token. Returns the Stop the page sends.
fn tuned_after_a_halt(f: &Fixture, streaming: &mut Streaming, cause: u8, now: Instant) -> Vec<u8> {
    streaming.presence.renew(&f.station, &streaming.offer, now);
    // The page re-reads its token from the relay heartbeat's reply, about once a second.
    let held = stop_request(streaming, &f.lease, now);
    let mut e = tempo_app::engine::engine_lock(&f.station.engine);
    e.set_operating_mode("phone", false);
    match cause {
        // The shack's Esc, which an Esc typed over the picture is (Phone: halt_tx, then stop_voice).
        0 => {
            e.halt_tx();
            e.stop_voice();
        }
        // A band change through the picture or at the rig: the context halt every QSY runs.
        _ => e.halt_tx_for_context_change("band change"),
    }
    // The operator reaches for Tune at once, through the picture: a local press.
    e.set_tune(true);
    assert!(e.tuning(), "premise: the carrier is up");
    held
}

const HALTS_AT_THE_STATION: [(&str, u8); 2] = [
    (
        "an Esc over the picture (the shack's halt_tx + stop_voice)",
        0,
    ),
    ("a band change (halt_tx_for_context_change)", 1),
];

/// CONTROL: the same Stop carrying the token as re-read after the halt (the page's next heartbeat
/// reply) is accepted and the carrier comes down. It passed under the old rule too, so the test
/// below differs from it in the token's age alone.
#[test]
fn a_stop_with_the_token_reread_after_the_halt_stops_the_tune() {
    let _alone = alone();
    for (cause, halt) in HALTS_AT_THE_STATION {
        let now = Instant::now();
        let f = fixture(now);
        let mut streaming = verified_stream(&f, now);
        let _held = tuned_after_a_halt(&f, &mut streaming, halt, now);
        let fresh = stop_request(&mut streaming, &f.lease, now);
        let (answer, _) = streaming.control(&fresh, |_| true, now);
        assert_eq!(
            reply_of(&answer.unwrap())["value"]["stop"],
            "accepted",
            "{cause}"
        );
        assert!(
            !tempo_app::engine::engine_lock(&f.station.engine).tuning(),
            "{cause}: the carrier stayed up"
        );
    }
}

/// ★ Stop TX pressed after a halt at the station, and before the page has re-read its stop token,
/// stops the carrier started since. Both of the page's paths carry the same token; this is the
/// stream's. The only difference from the control above is the token's age.
#[test]
fn a_stop_pressed_after_a_station_side_halt_stops_what_started_since() {
    let _alone = alone();
    let mut failures = Vec::new();
    for (cause, halt) in HALTS_AT_THE_STATION {
        let now = Instant::now();
        let f = fixture(now);
        let mut streaming = verified_stream(&f, now);
        let held = tuned_after_a_halt(&f, &mut streaming, halt, now);
        let (answer, _) = streaming.control(&held, |_| true, now);
        let answer = reply_of(&answer.unwrap());
        let tuning = tempo_app::engine::engine_lock(&f.station.engine).tuning();
        if answer["value"]["stop"] != "accepted" || tuning {
            failures.push(format!(
                "{cause}: Stop TX left the carrier {}; the station answered {}",
                if tuning { "UP" } else { "down" },
                answer.get("error").unwrap_or(&answer["value"])
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
