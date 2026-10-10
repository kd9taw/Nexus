//! The stream on the LAN road: the same admission, the same presence and the same bounds as
//! on the relay's road, under a lease bound to a LAN connection. Everything here runs on every
//! platform; what needs a WebRTC session is `tempo_stream::session`'s, on Windows.
use super::tests::DeviceKey;
use super::*;
use std::sync::Mutex;

const DEVICE: &str = "40000000-0000-4000-8000-000000000001";
const SESSION: &str = "50000000-0000-4000-8000-000000000001";
const STATION: &str = "60000000-0000-4000-8000-000000000001";

fn id() -> String {
    let hex = super::super::transport::random_secret().unwrap();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

struct Lan {
    station: Station,
    lease: String,
    key: DeviceKey,
}

/// A station whose one controller is a computer on the LAN road: station control granted at the
/// radio, its lease taken on its own LAN connection, its key pinned.
fn lan(now: Instant) -> Lan {
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
    let connection = authority.start_lan_connection();
    authority.permit_station(DEVICE, true).unwrap();
    let ask = |request: Request| {
        authority
            .handle_version(
                (connection, OPERATION_VERSION),
                SESSION,
                DEVICE,
                &request,
                &engine,
                now,
            )
            .unwrap()
    };
    let boot = ask(Request::State { request_id: id() })["stationBootId"]
        .as_str()
        .unwrap()
        .to_string();
    let lease = ask(Request::Acquire {
        request_id: id(),
        station_boot_id: boot,
    })["leaseId"]
        .as_str()
        .unwrap()
        .to_string();
    let key = DeviceKey::new();
    let pin = key.pin();
    Lan {
        station: Station {
            authority: authority.clone(),
            engine,
            connection,
            host: Host {
                ptt,
                mic,
                ..Host::default()
            },
            #[cfg(feature = "radio")]
            audio: None,
            station_id: STATION.into(),
            pinned: Arc::new(move |device: &str| (device == DEVICE).then_some(pin)),
            signer: None,
        },
        lease,
        key,
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

fn offer(l: &Lan, signer: &DeviceKey, ids: (&str, &str, &str)) -> Offer {
    let sdp = contract_offer();
    Offer {
        session: SESSION.into(),
        device: DEVICE.into(),
        lease: l.lease.clone(),
        public_key: Some(signer.public_key.clone()),
        signature: Some(signer.sign(&sdp, ids.0, ids.1, ids.2)),
        sdp,
    }
}

fn verified(l: &Lan, now: Instant) -> Streaming {
    let mut streaming = Streaming::new(
        l.station.clone(),
        offer(l, &l.key, (STATION, DEVICE, SESSION)),
        now,
    );
    streaming.verified = true;
    streaming
}

fn poll(l: &Lan, at: Instant) -> bool {
    tempo_app::engine::engine_lock(&l.station.engine).poll_remote_transmit(at)
}

fn heartbeat(lease: &str) -> Vec<u8> {
    format!(
        r#"{{"type":"heartbeat","requestId":"{}","leaseId":"{lease}","decodedFrameAt":1}}"#,
        id()
    )
    .into_bytes()
}

/// ★ The LAN road's offer goes through the same `admit`, and is refused exactly as the
/// relay's is: unsigned, signed by a key that is not the pinned one, or signed over another
/// session. CONTROL: the offer signed by the pinned key for this session gets past every check the
/// station makes, to the platform's own answer.
#[test]
fn the_lan_roads_offer_meets_the_same_admission() {
    let now = Instant::now();
    let l = lan(now);
    let other = DeviceKey::new();
    let mut unsigned = offer(&l, &l.key, (STATION, DEVICE, SESSION));
    unsigned.public_key = None;
    unsigned.signature = None;
    let cases = [
        ("unsigned", unsigned, Err(StreamReason::DeviceKeyMismatch)),
        (
            "another key",
            offer(&l, &other, (STATION, DEVICE, SESSION)),
            Err(StreamReason::DeviceKeyMismatch),
        ),
        (
            "another session",
            offer(
                &l,
                &l.key,
                (STATION, DEVICE, "50000000-0000-4000-8000-000000000009"),
            ),
            Err(StreamReason::DeviceKeyMismatch),
        ),
        (
            "control",
            offer(&l, &l.key, (STATION, DEVICE, SESSION)),
            if cfg!(windows) {
                Ok(())
            } else {
                Err(StreamReason::StreamUnavailable)
            },
        ),
    ];
    for (case, offer, expected) in cases {
        assert_eq!(admit(&l.station, &offer, now), expected, "{case}");
    }
}

/// ★ On the LAN road: a link that dies without a word is halted on the station's own clock. The
/// last fresh heartbeat on `control` is at 1 s; a phone over, a Tune carrier and an FT8 CQ are each
/// still up at 5.999 s and halted at 6.000 s, five seconds after it (Tune's measured 6 s). CONTROL:
/// up until then.
#[test]
fn a_silent_link_on_the_lan_road_halts_on_the_stations_clock() {
    for kind in ["phone", "tune", "ft8"] {
        let t0 = Instant::now();
        let l = lan(t0);
        let mut streaming = verified(&l, t0);
        streaming.presence.renew(&l.station, &streaming.offer, t0);
        let t1 = t0 + Duration::from_secs(1);
        let (answer, _) = streaming.control(&heartbeat(&l.lease), |_| true, t1);
        assert!(answer.is_some(), "{kind}: the heartbeat was not answered");
        {
            let mut e = tempo_app::engine::engine_lock(&l.station.engine);
            match kind {
                "phone" => {
                    e.set_operating_mode("phone", false);
                    e.set_ptt(true);
                    assert!(e.manual_ptt(), "premise: keyed");
                }
                "tune" => {
                    e.set_tune(true);
                    assert!(e.tuning(), "premise: the carrier is up");
                }
                // A CQ started through the picture is the shack's own, a local arm.
                _ => {
                    e.set_tx_enabled(true);
                    e.start_cq(None).unwrap();
                    assert!(e.tx_enabled(), "premise: on the air");
                }
            }
        }
        assert!(
            !poll(&l, t1 + Duration::from_millis(4999)),
            "{kind}: halted before presence lapsed"
        );
        assert!(
            poll(&l, t1 + Duration::from_secs(5)),
            "{kind}: still up when presence lapsed"
        );
    }
}

/// ★ The connection closing ends the stream's presence at once, before the stream's own
/// session notices, and the radio loop halts on its next tick. CONTROL: presence was live and the
/// station keyed until the close.
#[test]
fn a_lan_connection_that_closes_ends_its_streams_presence_at_once() {
    let now = Instant::now();
    let l = lan(now);
    let mut streaming = verified(&l, now);
    streaming.presence.renew(&l.station, &streaming.offer, now);
    {
        let mut e = tempo_app::engine::engine_lock(&l.station.engine);
        e.set_operating_mode("phone", false);
        e.set_ptt(true);
    }
    assert!(streaming.presence.live(now), "premise: presence");
    assert!(!poll(&l, now), "premise: keyed");
    l.station
        .authority
        .retire_lan_connection(l.station.connection);
    assert!(
        !streaming.presence.live(now),
        "presence outlived the connection"
    );
    assert!(
        poll(&l, now),
        "the station stayed keyed after the connection closed"
    );
    assert!(!tempo_app::engine::engine_lock(&l.station.engine).manual_ptt());
}

/// ★ One stream for the whole station, whichever road: while a stream on one road still holds
/// the slot, an offer on the other is answered `streamInUse` at once, and nothing is started.
/// CONTROL: once the first has finished ending, the same offer is taken to admission.
#[test]
fn one_stream_for_the_whole_station_whichever_road() {
    let now = Instant::now();
    let l = lan(now);
    let (to_peer, mut said) = tokio::sync::mpsc::unbounded_channel::<String>();
    let network = tempo_stream::lan::Network::new("192.168.1.20".parse().unwrap(), 24).unwrap();
    let payload = |l: &Lan| {
        let offer = offer(l, &l.key, (STATION, DEVICE, SESSION));
        BrowserSignal::Offer {
            sdp: offer.sdp,
            public_key: offer.public_key,
            signature: offer.signature,
        }
    };
    let ids = (SESSION.to_string(), DEVICE.to_string(), l.lease.clone());
    // The relay's road is streaming: its session holds the slot.
    let held = l
        .station
        .authority
        .claim_stream()
        .expect("the slot is free");
    let mut lane = StreamLane::default();
    let refused = lane.signal_lan(
        &l.station,
        &to_peer,
        ids.clone(),
        payload(&l),
        network,
        42075,
    );
    let refused: Value =
        serde_json::from_str(refused.as_deref().expect("an answer at once")).unwrap();
    assert_eq!(refused["reason"], "streamInUse", "{refused}");
    assert!(lane.live.is_none(), "a session was started beside another");
    // CONTROL: the relay's stream has finished ending.
    drop(held);
    assert_eq!(
        lane.signal_lan(&l.station, &to_peer, ids, payload(&l), network, 42075),
        None,
        "the offer was not taken to admission"
    );
    let ended: Value = serde_json::from_str(
        &said
            .blocking_recv()
            .expect("the session thread said how it stands"),
    )
    .unwrap();
    // Past every check of the offer, to the window this fixture has none of (or, off Windows, to
    // the platform's own answer): either way the same reason.
    assert_eq!(ended["streaming"], false);
    assert_eq!(ended["reason"], "streamUnavailable", "{ended}");
}

/// ★ What the LAN road's session socket hears and tries: only the shack's own subnet, a page's
/// host candidate there included, and nothing else (a reflexive candidate, loopback, another
/// network). CONTROL: the relay's road is not narrowed to a subnet (what the session tries, on
/// either road, is held to `tempo_stream::session::may_try` as well).
#[test]
fn the_lan_roads_socket_hears_and_tries_only_the_shacks_subnet() {
    let lan = Road::Lan {
        network: tempo_stream::lan::Network::new("192.168.1.20".parse().unwrap(), 24).unwrap(),
        port: 42075,
    };
    let on = "candidate:1 1 udp 2122260223 192.168.1.33 50000 typ host";
    let off = "candidate:2 1 udp 1686052607 203.0.113.9 50001 typ srflx raddr 0.0.0.0 rport 0";
    assert!(lan.hears("192.168.1.33:50000".parse().unwrap()));
    assert!(lan.tries(on));
    for source in ["203.0.113.9:50001", "127.0.0.1:50000", "192.168.2.33:50000"] {
        assert!(!lan.hears(source.parse().unwrap()), "heard {source}");
    }
    assert!(!lan.tries(off), "tried a reflexive candidate");
    assert!(Road::Relay.hears("203.0.113.9:50001".parse().unwrap()));
    assert!(Road::Relay.tries(off));
}
