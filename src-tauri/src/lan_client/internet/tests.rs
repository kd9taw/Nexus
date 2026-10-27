//! The internet road's requests, against a stand-in for the Remote service on loopback that checks
//! each one as the service does (`signedBy`, `nativeDevice`): the challenge it gave, the device it
//! named, and the signature over the five lines with the key it holds for that device.
use super::*;
use futures_util::{SinkExt, StreamExt};
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;

const STATION: &str = "20000000-0000-4000-8000-000000000001";
const DEVICE: &str = "30000000-0000-4000-8000-000000000002";

/// What the stand-in holds and saw.
#[derive(Default)]
struct Seen {
    /// The device key it registered for [`DEVICE`], SPKI hex.
    device_key: Option<String>,
    /// The challenges it gave and has not yet taken.
    challenges: Vec<String>,
    /// Each request: method and path.
    requests: Vec<(String, String)>,
    /// A refusal it gives to the pipe, if any: status and code.
    refuse_pipe: Option<(u16, &'static str)>,
    /// What `client/session` lists.
    session: Option<Value>,
    /// The last `client/device` body it took.
    device_body: Option<Value>,
}

/// Whether `signature` is the key `spki`'s over the five lines for `challenge`, `method`, `path`
/// and `body`.
fn verifies(
    spki: &str,
    challenge: &str,
    method: &str,
    path: &str,
    body: &[u8],
    signature: &str,
) -> bool {
    let point = tempo_stream::protocol::hex_bytes(spki)
        .map(|der| der[der.len() - 65..].to_vec())
        .unwrap_or_default();
    let Some(signature) = tempo_stream::protocol::hex_bytes(signature) else {
        return false;
    };
    let lines = native::signed_lines(challenge, method, path, body);
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point)
        .verify(&lines, &signature)
        .is_ok()
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// The stand-in's checks for a signed request as the device: its id, a challenge it gave (taken),
/// and a good signature by the key it holds for that device.
fn signed(
    seen: &mut Seen,
    headers: &[(String, String)],
    method: &str,
    path: &str,
    body: &[u8],
) -> bool {
    let challenge = header(headers, "x-nexus-challenge")
        .unwrap_or_default()
        .to_string();
    let Some(at) = seen.challenges.iter().position(|c| *c == challenge) else {
        return false;
    };
    seen.challenges.remove(at);
    header(headers, "x-nexus-device-id") == Some(DEVICE)
        && seen.device_key.as_deref().is_some_and(|key| {
            verifies(
                key,
                &challenge,
                method,
                path,
                body,
                header(headers, "x-nexus-signature").unwrap_or_default(),
            )
        })
}

fn reply(status: u16, body: Value) -> Vec<u8> {
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// One plain request: method, path, headers and body.
async fn read_request(
    stream: &mut TcpStream,
) -> Option<(String, String, Vec<(String, String)>, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&bytes[..end]).ok()?.to_string();
    let mut lines = head.lines();
    let mut first = lines.next()?.split(' ');
    let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let length: usize = header(&headers, "content-length")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    let mut body = bytes[end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some((method, path, headers, body))
}

/// The stand-in on loopback: its origin, and what it saw.
async fn service() -> (String, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Seen::default()));
    let held = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let seen = held.clone();
            tokio::spawn(async move {
                let mut peeked = [0u8; 256];
                let n = stream.peek(&mut peeked).await.unwrap_or(0);
                let line = String::from_utf8_lossy(&peeked[..n]).to_string();
                if line.starts_with("GET ") && line.contains("/client/pipe ") {
                    pipe(stream, seen).await;
                    return;
                }
                let Some((method, path, headers, body)) = read_request(&mut stream).await else {
                    return;
                };
                let answer = answer_to(&seen, &method, &path, &headers, &body);
                let _ = stream.write_all(&answer).await;
            });
        }
    });
    (origin, seen)
}

/// The stand-in's answer to one plain request.
fn answer_to(
    seen: &Mutex<Seen>,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Vec<u8> {
    let mut seen = seen.lock().unwrap();
    seen.requests.push((method.to_string(), path.to_string()));
    let bearer = header(headers, "authorization") == Some("Bearer token-1");
    let station = |verb: &str| format!("/api/remote/stations/{STATION}/client/{verb}");
    if path == "/api/remote/client/challenge" || path == station("challenge") {
        // The account's with a token, or the device's own when it reopens.
        let reopening = path != "/api/remote/client/challenge";
        let ok = if reopening {
            header(headers, "x-nexus-device-id") == Some(DEVICE) && !bearer
        } else {
            bearer
        };
        if !ok {
            return reply(401, json!({"error":"signInRequired"}));
        }
        let challenge = format!("{:064x}", seen.challenges.len() + seen.requests.len());
        seen.challenges.push(challenge.clone());
        return reply(200, json!({"challenge":challenge,"expiresAt":1}));
    }
    if path == "/api/remote/client/session" && bearer {
        return reply(200, seen.session.clone().unwrap_or(json!({})));
    }
    if path == station("device") && bearer {
        let sent: Value = serde_json::from_slice(body).unwrap_or_default();
        let key = sent["publicKey"].as_str().unwrap_or_default().to_string();
        let challenge = header(headers, "x-nexus-challenge")
            .unwrap_or_default()
            .to_string();
        let good = seen.challenges.contains(&challenge)
            && verifies(
                &key,
                &challenge,
                "POST",
                path,
                body,
                header(headers, "x-nexus-signature").unwrap_or_default(),
            )
            && sent["kind"] == "computer";
        if !good {
            return reply(401, json!({"error":"invalidSignature"}));
        }
        seen.device_key = Some(key);
        seen.device_body = Some(sent);
        return reply(
            200,
            json!({"deviceId":DEVICE,"approved":false,"expiresAt":1}),
        );
    }
    if (path == station("turn") || path == station("renew") || path == station("forget")) && !bearer
    {
        if !signed(&mut seen, headers, "POST", path, body) {
            return reply(401, json!({"error":"invalidSignature"}));
        }
        if path == station("turn") {
            return reply(
                200,
                json!({"iceServers":[{"urls":["turns:turn.example.net:443?transport=tcp",
                "turn:127.0.0.1:3478?transport=udp"],"username":"u","credential":"p"}]}),
            );
        }
        return reply(200, json!({"ok":true}));
    }
    reply(404, json!({"error":"notFound"}))
}

/// The pipe: the upgrade's headers checked as a signed GET with no body, then each binary message
/// sent back as it came.
async fn pipe(stream: TcpStream, seen: Arc<Mutex<Seen>>) {
    let checked = seen.clone();
    let callback = move |request: &Request, response: Response| {
        let headers: Vec<(String, String)> = request
            .headers()
            .iter()
            .map(|(n, v)| {
                (
                    n.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let path = request.uri().path().to_string();
        let mut seen = checked.lock().unwrap();
        seen.requests.push(("GET".into(), path.clone()));
        let refusal = |status: u16, code: &str| {
            let mut refused = tokio_tungstenite::tungstenite::http::Response::new(Some(
                json!({"error":code}).to_string(),
            ));
            *refused.status_mut() = status.try_into().unwrap();
            refused
        };
        if let Some((status, code)) = seen.refuse_pipe {
            return Err(refusal(status, code));
        }
        if !signed(&mut seen, &headers, "GET", &path, b"") {
            return Err(refusal(401, "invalidSignature"));
        }
        Ok(response)
    };
    let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, callback).await else {
        return;
    };
    while let Some(Ok(message)) = socket.next().await {
        if let Message::Binary(bytes) = message {
            let _ = socket.send(Message::Binary(bytes)).await;
        }
    }
}

/// The road this computer keeps once registered with the stand-in.
/// The operator's pick when the account has more than one station: `pick`, or none; and every list
/// offered.
fn picking(pick: Option<&str>) -> (Choose, Arc<Mutex<Vec<Vec<Choice>>>>) {
    let offered: Arc<Mutex<Vec<Vec<Choice>>>> = Arc::default();
    let (kept, pick) = (offered.clone(), pick.map(str::to_string));
    let choose: Choose = Arc::new(move |stations| {
        kept.lock().unwrap().push(stations);
        let pick = pick.clone();
        Box::pin(async move { pick })
    });
    (choose, offered)
}

async fn registered(origin: &str, seen: &Arc<Mutex<Seen>>) -> InternetRoad {
    let (station_key, _) = ComputerKey::generate().unwrap();
    let service = Service::new(origin).unwrap();
    let (key, pkcs8) = ComputerKey::generate().unwrap();
    let (device_id, approved) = service
        .register("token-1", STATION, &key, None, "Den PC")
        .await
        .unwrap();
    assert!(!approved);
    assert_eq!(
        seen.lock().unwrap().device_key.as_deref(),
        Some(key.public_key())
    );
    InternetRoad {
        station_id: STATION.into(),
        device_id,
        station_key: station_key.public_key().to_string(),
        pkcs8,
    }
}

/// ★ Registering: this computer's road key as a device of the account's station, signed by that key
/// over a challenge the account was given, and the device id the service gave. With the sign-in's
/// proof, the LAN key and the proof ride along; without one, neither does. CONTROL: a road key the
/// signature is not by is refused (`accessDenied`).
#[tokio::test]
async fn registering_is_signed_by_the_road_key_and_carries_the_proof_when_given() {
    let (origin, seen) = service().await;
    let road = registered(&origin, &seen).await;
    assert_eq!(
        (road.station_id.as_str(), road.device_id.as_str()),
        (STATION, DEVICE)
    );
    let body = seen.lock().unwrap().device_body.clone().unwrap();
    let key = ComputerKey::restore(&road.pkcs8).unwrap();
    assert_eq!(
        body,
        json!({"name":"Den PC","kind":"computer","platform":platform(),"publicKey":key.public_key()})
    );

    let service = Service::new(&origin).unwrap();
    let (lan, _) = ComputerKey::generate().unwrap();
    let registered = service
        .register(
            "token-1",
            STATION,
            &key,
            Some((&lan, "the-proof")),
            "Den PC",
        )
        .await;
    assert_eq!(registered, Ok((DEVICE.to_string(), false)));
    assert_eq!(
        seen.lock().unwrap().device_body.clone().unwrap(),
        json!({"name":"Den PC","kind":"computer","platform":platform(),"publicKey":key.public_key(),
            "lanKey":lan.public_key(),"proof":"the-proof"})
    );
    // Not an id of the service's: nothing is sent.
    let before = seen.lock().unwrap().requests.len();
    assert_eq!(
        service
            .register("token-1", "not-an-id", &key, None, "Den PC")
            .await
            .err(),
        Some("invalidResponse")
    );
    assert_eq!(seen.lock().unwrap().requests.len(), before);
}

/// ★ The road kept says this computer's key for it as the station shows that key when it asks to
/// approve this computer: the shack's own fingerprint (`remote_service::fingerprint`) of the key the
/// service registered for the device, which is what the service lists to the station. Whether the
/// device still waits for that approval, and the road kept for the station, are the sign-in's
/// (`super::account`). CONTROL: a record with no key in it shows none.
#[tokio::test]
async fn the_road_kept_says_this_computers_key_as_the_station_shows_it() {
    let (origin, seen) = service().await;
    let road = registered(&origin, &seen).await;
    let listed = seen.lock().unwrap().device_key.clone().unwrap();
    let shown = shown_key(&road);
    assert!(shown.is_some(), "a key to show");
    assert_eq!(
        shown,
        crate::remote_service::fingerprint(&listed),
        "the key the station shows"
    );
    let mut broken = clone_road(&road);
    broken.pkcs8 = "00".into();
    assert_eq!(shown_key(&broken), None, "no key in it");
}

/// ★ A road registered without a proof is for the account's one station, or the one the operator
/// picks: each station's id and name offered, and the one picked is the one listed. CONTROL: no
/// pick (cancelled, or the window gone), or an id the service never listed, picks nothing
/// (`stationNotChosen`); an entitlement that does not take in the internet road picks nothing
/// (`internetNotIncluded`), nor does an account with no station.
#[tokio::test]
async fn a_road_without_a_proof_is_for_the_station_the_operator_picks() {
    const CLUB: &str = "20000000-0000-4000-8000-0000000000c1";
    let (club_key, _) = ComputerKey::generate().unwrap();
    let (shack_key, _) = ComputerKey::generate().unwrap();
    let session = json!({"entitlement":{"access":"internet"},"stations":[
        {"id":CLUB,"name":"Club station","stationKey":club_key.public_key()},
        {"id":STATION,"name":"Shack","stationKey":shack_key.public_key()}]});
    for pick in [None, Some("30000000-0000-4000-8000-0000000000ff")] {
        let (choose, offered) = picking(pick);
        assert_eq!(
            chosen(&session, choose).await.err(),
            Some("stationNotChosen"),
            "{pick:?}"
        );
        assert_eq!(offered.lock().unwrap().len(), 1);
    }
    let (choose, offered) = picking(Some(STATION));
    let station = chosen(&session, choose).await.unwrap();
    assert_eq!(
        offered.lock().unwrap().as_slice(),
        [vec![
            Choice {
                id: CLUB.into(),
                name: "Club station".into()
            },
            Choice {
                id: STATION.into(),
                name: "Shack".into()
            },
        ]]
    );
    assert_eq!(station["id"], STATION);
    assert_eq!(station["stationKey"], shack_key.public_key());

    let (choose, offered) = picking(None);
    let one = json!({"entitlement":{"enabled":true,"expiresAt":u64::MAX},
        "stations":[{"id":STATION,"name":"Shack","stationKey":shack_key.public_key()}]});
    assert_eq!(chosen(&one, choose).await.unwrap()["id"], STATION);
    assert!(offered.lock().unwrap().is_empty(), "asked with one station");
    let ended = json!({"entitlement":{"enabled":true,"expiresAt":1},"stations":one["stations"]});
    assert_eq!(
        chosen(&ended, picking(None).0).await.err(),
        Some("internetNotIncluded")
    );
    let none = json!({"entitlement":{"access":"internet"},"stations":[]});
    assert_eq!(
        chosen(&none, picking(None).0).await.err(),
        Some("stationUnavailable")
    );
}

/// ★ Reopening needs no sign-in: TURN credentials and a renewal are asked as the device, each
/// signed by the road's key over a challenge given for that device, and the UDP TURN server comes
/// back (the TLS one left out). CONTROL: a road holding another key is refused (`accessDenied`).
#[tokio::test]
async fn reopening_is_signed_by_the_roads_key_with_no_sign_in() {
    let (origin, seen) = service().await;
    let road = registered(&origin, &seen).await;
    let service = Service::new(&origin).unwrap();
    let servers = service.turn(&road).await.unwrap();
    let named: Vec<(String, u16)> = servers.iter().map(|s| (s.host.clone(), s.port)).collect();
    assert_eq!(named, vec![("127.0.0.1".to_string(), 3478)]);
    service.renew(&road).await.unwrap();
    let relay = relay(servers).await.expect("the relay");
    assert_eq!(relay.server, "127.0.0.1:3478".parse().unwrap());

    let (_, other) = ComputerKey::generate().unwrap();
    let impostor = InternetRoad {
        pkcs8: other,
        ..road
    };
    assert_eq!(service.renew(&impostor).await.err(), Some("accessDenied"));
}

/// ★ Forget asks the service, as the device and with no sign-in, to end this device's approval:
/// signed by the road's key over a challenge given for that device. CONTROL: a road holding another
/// key is refused (`accessDenied`).
#[tokio::test]
async fn forgetting_is_signed_by_the_roads_key_with_no_sign_in() {
    let (origin, seen) = service().await;
    let road = registered(&origin, &seen).await;
    let service = Service::new(&origin).unwrap();
    let before = seen.lock().unwrap().requests.len();
    service.forget(&road).await.unwrap();
    let asked = seen.lock().unwrap().requests[before..].to_vec();
    assert_eq!(
        asked.last(),
        Some(&(
            "POST".to_string(),
            format!("/api/remote/stations/{STATION}/client/forget")
        ))
    );
    let (_, other) = ComputerKey::generate().unwrap();
    let impostor = InternetRoad {
        pkcs8: other,
        ..road
    };
    assert_eq!(service.forget(&impostor).await.err(), Some("accessDenied"));
}

/// ★ The pipe is a WebSocket signed as the device (a GET with no body), and carries bytes both
/// ways. CONTROL: a pipe the service refuses for the entitlement says `internetNotIncluded`, and a
/// road holding another key is refused.
#[tokio::test]
async fn the_pipe_opens_signed_and_carries_bytes() {
    let (origin, seen) = service().await;
    let road = registered(&origin, &seen).await;
    let service = Service::new(&origin).unwrap();
    let mut socket = service.pipe(&road).await.expect("the pipe");
    socket
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    match tokio::time::timeout(Duration::from_secs(5), socket.next()).await {
        Ok(Some(Ok(Message::Binary(bytes)))) => assert_eq!(bytes.as_ref(), &[1, 2, 3]),
        other => panic!("nothing came back: {other:?}"),
    }
    let (_, other) = ComputerKey::generate().unwrap();
    let impostor = InternetRoad {
        pkcs8: other,
        ..clone_road(&road)
    };
    assert_eq!(service.pipe(&impostor).await.err(), Some("accessDenied"));
    seen.lock().unwrap().refuse_pipe = Some((403, "trialRequired"));
    assert_eq!(service.pipe(&road).await.err(), Some("internetNotIncluded"));
}

fn clone_road(road: &InternetRoad) -> InternetRoad {
    InternetRoad {
        station_id: road.station_id.clone(),
        device_id: road.device_id.clone(),
        station_key: road.station_key.clone(),
        pkcs8: road.pkcs8.clone(),
    }
}

/// The service's refusals as this computer names them: only those it knows, the rest by status.
#[test]
fn only_known_refusals_are_named() {
    let said = |code: &str| json!({"error":code});
    assert_eq!(
        named(403, Some(&said("trialRequired"))),
        "internetNotIncluded"
    );
    assert_eq!(
        named(401, Some(&said("signInRequired"))),
        "notApprovedThere"
    );
    assert_eq!(
        named(403, Some(&said("deviceNotApproved"))),
        "waitingApproval"
    );
    assert_eq!(named(403, Some(&said("<script>"))), "accessDenied");
    assert_eq!(named(429, None), "tryLater");
    assert_eq!(named(500, None), "serviceUnavailable");
    assert!(lasting("internetNotIncluded") && !lasting("serviceUnavailable"));
}
