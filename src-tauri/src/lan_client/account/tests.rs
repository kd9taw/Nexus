//! Signing in on this computer, against a stand-in on loopback that is both the Remote service and
//! its issuer (`Cloud`): the sign-in's redirect, the token exchange, the key set, the account's
//! stations and the device registration, each checked as the real ones would check it.
//!
//! The issuer's ID token here is unsigned: this computer reads its own token without checking a
//! signature (it came straight from the issuer), and no stand-in station checks one. A station's own
//! proof IS checked with its signature where a test runs the real check (`listed_station`), with
//! tempo-client's account fixture, minted by throwaway keys that never left a scratch process. The
//! flows that need a station on the network are with the station's own test pieces
//! (`remote_service::lan::tests::signing`).
use super::*;
use crate::lan_client::tests::StationStore;
use crate::remote_service::vault::StationVault;
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tempo_client::native::base64url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const CASES: &str =
    include_str!("../../../../crates/tempo-client/tests/fixtures/account-proof-cases.json");
/// The fixture's tenant: its issuer and its applications.
pub(crate) const ISSUER: &str = "https://issuer.test.invalid/";
pub(crate) const NATIVE: &str = "test-native-client";
/// The service's id for the account's station.
pub(crate) const SERVICE_STATION: &str = "20000000-0000-4000-8000-000000000001";
pub(crate) const ACCESS: &str = "access-token-never-to-be-shown";
/// The station's id on its network, as a stand-in station gives it.
pub(crate) const LAN_STATION: &str = "60000000-0000-4000-8000-0000000000aa";

pub(crate) fn cases() -> Value {
    serde_json::from_str(CASES).unwrap()
}
pub(crate) fn fixture_now() -> u64 {
    cases()["now"].as_u64().unwrap()
}
pub(crate) fn fixture_key(name: &str) -> String {
    cases()["keys"][name].as_str().unwrap().to_string()
}
pub(crate) fn fixture_token(name: &str) -> String {
    cases()["tokens"][name].as_str().unwrap().to_string()
}

/// What the stand-in holds, and what it saw.
pub(crate) struct Seen {
    /// Each request: method and path.
    pub(crate) requests: Vec<(String, String)>,
    /// The nonce of the last authorize address the browser was sent to.
    pub(crate) nonce: Option<String>,
    /// Give a token with this nonce instead of the one asked for.
    pub(crate) wrong_nonce: Option<String>,
    /// Who the issuer says signed in, and when.
    pub(crate) subject: String,
    pub(crate) issued_at: u64,
    /// What `client/session` lists.
    pub(crate) session: Value,
    /// Each `client/device` body taken, with the authorization it came with.
    pub(crate) devices: Vec<(Value, String)>,
    /// The road keys registered, in order: a device's id is its key's place here.
    pub(crate) keys: Vec<String>,
    /// Whether a registration is approved.
    pub(crate) approve: bool,
    /// The devices that ended their own approval (`client/forget`, signed by their keys).
    pub(crate) forgotten: Vec<String>,
    challenges: Vec<String>,
}

impl Default for Seen {
    fn default() -> Self {
        Self {
            requests: Vec::new(),
            nonce: None,
            wrong_nonce: None,
            subject: cases()["owner"].as_str().unwrap().into(),
            issued_at: fixture_now() - 30,
            session: json!({"stations": [], "entitlement": {"access": "internet"}}),
            devices: Vec::new(),
            keys: Vec::new(),
            approve: true,
            forgotten: Vec::new(),
            challenges: Vec::new(),
        }
    }
}

impl Seen {
    /// The paths asked, in order.
    pub(crate) fn paths(&self) -> Vec<String> {
        self.requests.iter().map(|(_, path)| path.clone()).collect()
    }
}

/// The device id the stand-in gives the road key at `place` in its list.
pub(crate) fn device_at(place: usize) -> String {
    format!("30000000-0000-4000-8000-{place:012x}")
}

/// An ID token as the stand-in issuer gives one: the claims this computer reads, and no signature
/// anyone could check.
pub(crate) fn unsigned_token(subject: &str, issued_at: u64, nonce: &str) -> String {
    let part = |value: Value| base64url(value.to_string().as_bytes());
    format!(
        "{}.{}.{}",
        part(json!({"alg": "RS256", "kid": "test-p"})),
        part(
            json!({"iss": ISSUER, "aud": NATIVE, "sub": subject, "iat": issued_at,
            "exp": issued_at + 36_000, "nonce": nonce, "email": "owner@example.test",
            "email_verified": true})
        ),
        base64url(b"not-a-signature")
    )
}

fn reply(status: u16, body: &Value) -> Vec<u8> {
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// One plain request: method, path, headers (names in lowercase) and body.
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

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// Whether `signature` is the key `spki`'s over the five lines for `challenge`, `method`, `path`
/// and `body`, as the service checks a device's request.
fn verifies(spki: &str, challenge: &str, path: &str, body: &[u8], signature: &str) -> bool {
    if spki.len() < 130 {
        return false;
    }
    let point = tempo_stream::protocol::hex_bytes(spki)
        .map(|der| der[der.len() - 65..].to_vec())
        .unwrap_or_default();
    let Some(signature) = tempo_stream::protocol::hex_bytes(signature) else {
        return false;
    };
    let lines = tempo_client::native::signed_lines(challenge, "POST", path, body);
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point)
        .verify(&lines, &signature)
        .is_ok()
}

/// The stand-in's answer to one request.
fn answer_to(
    seen: &Mutex<Seen>,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Vec<u8> {
    let mut seen = seen.lock().unwrap();
    seen.requests.push((method.to_string(), path.to_string()));
    let bearer = header(headers, "authorization") == Some(&format!("Bearer {ACCESS}"));
    match (method, path) {
        ("GET", "/api/remote/config") => reply(
            200,
            &json!({"issuer": "https://elsewhere.test.invalid/", "nativeClientId": "not-ours",
                "audience": "https://api.test.invalid/"}),
        ),
        ("GET", "/.well-known/jwks.json") => reply(200, &cases()["jwks"]),
        ("POST", "/oauth/token") => {
            let form = String::from_utf8_lossy(body);
            if !form.contains("code=the-code") || !form.contains(&format!("client_id={NATIVE}")) {
                return reply(400, &json!({"error": "invalid_grant"}));
            }
            let nonce = seen
                .wrong_nonce
                .clone()
                .or_else(|| seen.nonce.clone())
                .unwrap_or_default();
            let id_token = unsigned_token(&seen.subject, seen.issued_at, &nonce);
            reply(
                200,
                &json!({"access_token": ACCESS, "token_type": "Bearer", "expires_in": 600,
                    "id_token": id_token}),
            )
        }
        ("POST", "/api/remote/client/session") if bearer => reply(200, &seen.session),
        ("POST", "/api/remote/client/challenge") if bearer => {
            let challenge = format!("{:064x}", seen.requests.len());
            seen.challenges.push(challenge.clone());
            reply(200, &json!({"challenge": challenge, "expiresAt": 1}))
        }
        ("POST", p) if bearer && p.ends_with("/client/device") => {
            let sent: Value = serde_json::from_slice(body).unwrap_or_default();
            let key = sent["publicKey"].as_str().unwrap_or_default().to_string();
            let challenge = header(headers, "x-nexus-challenge")
                .unwrap_or_default()
                .to_string();
            let signature = header(headers, "x-nexus-signature").unwrap_or_default();
            if !seen.challenges.contains(&challenge)
                || !verifies(&key, &challenge, p, body, signature)
            {
                return reply(401, &json!({"error": "invalidSignature"}));
            }
            seen.challenges.retain(|c| *c != challenge);
            let place = match seen.keys.iter().position(|k| *k == key) {
                Some(place) => place,
                None => {
                    seen.keys.push(key);
                    seen.keys.len() - 1
                }
            };
            let authorization = header(headers, "authorization")
                .unwrap_or_default()
                .to_string();
            seen.devices.push((sent, authorization));
            reply(
                200,
                &json!({"deviceId": device_at(place), "approved": seen.approve, "expiresAt": 1}),
            )
        }
        // A device reopening, with no sign-in: a challenge for the device it names, then its own
        // approval ended, signed by its key.
        ("POST", p) if !bearer && p.ends_with("/client/challenge") => {
            let device = header(headers, "x-nexus-device-id").unwrap_or_default();
            if !(0..seen.keys.len()).any(|place| device_at(place) == device) {
                return reply(401, &json!({"error": "signInRequired"}));
            }
            let challenge = format!("{:064x}", seen.requests.len());
            seen.challenges.push(challenge.clone());
            reply(200, &json!({"challenge": challenge, "expiresAt": 1}))
        }
        ("POST", p) if !bearer && p.ends_with("/client/forget") => {
            let device = header(headers, "x-nexus-device-id")
                .unwrap_or_default()
                .to_string();
            let challenge = header(headers, "x-nexus-challenge")
                .unwrap_or_default()
                .to_string();
            let signature = header(headers, "x-nexus-signature").unwrap_or_default();
            let key = (0..seen.keys.len())
                .find(|&place| device_at(place) == device)
                .map(|place| seen.keys[place].clone())
                .unwrap_or_default();
            if !seen.challenges.contains(&challenge)
                || !verifies(&key, &challenge, p, body, signature)
            {
                return reply(401, &json!({"error": "invalidSignature"}));
            }
            seen.challenges.retain(|c| *c != challenge);
            seen.forgotten.push(device);
            reply(200, &json!({"ok": true}))
        }
        _ => reply(404, &json!({"error": "notFound"})),
    }
}

/// The stand-in on loopback: its address, and what it holds and saw.
pub(crate) struct Cloud {
    pub(crate) at: String,
    pub(crate) seen: Arc<Mutex<Seen>>,
}

impl Cloud {
    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Seen::default()));
        let held = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let seen = held.clone();
                tokio::spawn(async move {
                    let Some((method, path, headers, body)) = read_request(&mut stream).await
                    else {
                        return;
                    };
                    let answer = answer_to(&seen, &method, &path, &headers, &body);
                    let _ = stream.write_all(&answer).await;
                });
            }
        });
        Self { at, seen }
    }

    /// The account lists `stations`.
    pub(crate) fn lists(&self, stations: Value) {
        self.seen.lock().unwrap().session =
            json!({"stations": stations, "entitlement": {"access": "internet"}});
    }
}

/// A station as the account lists it, with `proof` for its keys, and the account's `role` there.
pub(crate) fn row(station_key: &str, lan_key: &str, proof: Option<&str>, role: &str) -> Value {
    json!({"id": SERVICE_STATION, "name": "Shack", "stationKey": station_key, "lanKey": lan_key,
        "proof": proof, "role": role})
}

/// Trusts a listed station on its row alone, as the real check does once a proof holds (and as it
/// does refuse a row with no proof): for a test whose station is a stand-in with a key made for the
/// run, which no fixture proof can name. The real check runs in this file's own tests and in
/// tempo-client's.
pub(crate) fn trusting(
    row: &Value,
    _: &KeySet,
    _: &Tenant,
    _: &str,
    _: u64,
) -> Result<Listed, &'static str> {
    if row["proof"].is_null() || row["lanKey"].is_null() {
        return Err("stationNotProven");
    }
    Ok(Listed {
        id: row["id"].as_str().unwrap().into(),
        name: row["name"].as_str().unwrap().into(),
        station_key: row["stationKey"].as_str().unwrap().into(),
        lan_key: row["lanKey"].as_str().unwrap().into(),
        own: row["role"] == "owner",
        role: if row["role"] == "listen" {
            Role::Listen
        } else {
            Role::Operate
        },
    })
}

/// A look by name that finds `found`, and counts how often it was asked.
pub(crate) fn finding(found: Vec<tempo_stream::lan::dnssd::Found>) -> (Find, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = asked.clone();
    let find: Find = Arc::new(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(found.clone())
    });
    (find, asked)
}

/// Signing in at `cloud` with this computer's `stations`, looking by name with `find`, checking
/// listed stations with `trust`.
pub(crate) fn env(cloud: &Cloud, stations: Arc<Stations>, find: Find, trust: Trust) -> Env {
    let tenant: &'static Tenant = Box::leak(Box::new(Tenant {
        origin: Box::leak(cloud.at.clone().into_boxed_str()),
        issuer: ISSUER,
        native_client_id: NATIVE,
        spa_client_id: "test-spa-client",
    }));
    Env {
        stations,
        account: Arc::new(Account::with(&cloud.at, tenant, &format!("{}/", cloud.at)).unwrap()),
        service: internet::Service::new(&cloud.at).unwrap(),
        find,
        lookup: Arc::new(|_, _| Vec::new()),
        name: "Den PC".into(),
        trust,
        hear: nowhere,
        now_s: fixture_now,
    }
}

/// A station heard nowhere on this network: its key for the internet road is not heard here.
pub(crate) fn nowhere<'a>(_: &'a PairedStation, _: Vec<std::net::SocketAddrV4>) -> Heard<'a> {
    Box::pin(async { Err("unreachable") })
}

/// A store for this computer's stations, with the signed file in a scratch directory of its own.
pub(crate) fn stations(store: &StationStore, dir: &std::path::Path) -> Arc<Stations> {
    Arc::new(
        Stations::new(Arc::new(store.clone()))
            .typed_in(dir.join("lan-typed.json"))
            .signed_in(dir.join("lan-account.json")),
    )
}

/// The operator's picks, none; every list offered kept.
pub(crate) fn no_pick() -> Choose {
    Arc::new(|_| Box::pin(async { None }))
}

/// Each question the operator was asked while signing in: the station's name, and why the last
/// address did not take this computer.
pub(crate) type Asked = Arc<Mutex<Vec<(String, Option<&'static str>)>>>;

/// The operator asked where the station is: each answer in turn, then none; every question kept.
pub(crate) fn answering(answers: Vec<String>) -> (Ask, Asked) {
    let asked: Asked = Arc::default();
    let (kept, answers) = (asked.clone(), Arc::new(Mutex::new(answers)));
    let ask: Ask = Arc::new(move |name, why| {
        kept.lock().unwrap().push((name, why));
        let next = {
            let mut answers = answers.lock().unwrap();
            (!answers.is_empty()).then(|| answers.remove(0))
        };
        Box::pin(async move { next })
    });
    (ask, asked)
}

/// One sign-in at `cloud`: the browser goes to the issuer and comes back with the code, as the
/// issuer sends it. What the sign-in did, and the address the browser was sent to.
pub(crate) async fn signing_in(
    env: &Env,
    cloud: &Cloud,
    here: Option<String>,
    choose: Choose,
    ask: Ask,
) -> (Result<Signed, &'static str>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = listener.local_addr().unwrap();
    let opened = Arc::new(Mutex::new(String::new()));
    let (kept, seen) = (opened.clone(), cloud.seen.clone());
    let open = move |url: &str| {
        *kept.lock().unwrap() = url.to_string();
        let field = |name: &str| {
            url.split(['?', '&'])
                .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
                .map(str::to_string)
        };
        seen.lock().unwrap().nonce = field("nonce");
        let state = field("state").unwrap();
        tokio::spawn(async move {
            crate::account_sign_in::tests::ask(
                local,
                &format!("/callback?code=the-code&state={state}"),
            )
            .await;
        });
        Ok(())
    };
    let signed = sign_in_on(
        env,
        here,
        listener,
        Duration::from_secs(10),
        open,
        choose,
        ask,
    )
    .await;
    let opened = opened.lock().unwrap().clone();
    (signed, opened)
}

/// The nonce a sign-in for these two keys carries: what the issuer's token must answer.
pub(crate) fn nonce_for(road_pkcs8: &str, lan_pkcs8: &str) -> String {
    let spki = |pkcs8: &str| {
        let key = ComputerKey::restore(pkcs8).unwrap();
        tempo_stream::protocol::hex_bytes(key.public_key()).unwrap()
    };
    proofs::device_nonce("computer", &[&spki(road_pkcs8), &spki(lan_pkcs8)])
}

/// This computer's key for its road as the station shows it: the station's own fingerprint of the
/// road key a device registration (`client/device`) gave the service, which it lists to the station.
pub(crate) fn shown(registered: &Value) -> String {
    crate::remote_service::fingerprint(registered["publicKey"].as_str().unwrap()).unwrap()
}

/// A scratch directory for one test's files, gone with it.
pub(crate) struct Scratch(pub(crate) std::path::PathBuf);
impl Scratch {
    pub(crate) fn new() -> Self {
        let mut bytes = [0u8; 8];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).unwrap();
        let dir = std::env::temp_dir().join(format!("nexus-signing-{}", tempo_client::hex(&bytes)));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// ★ A station is trusted only on its own proof, checked here with the issuer's signature, for the
/// two keys listed beside it, and for a station listed as the account's own, by the account's own
/// sign-in. One whose proof does not hold is neither registered at the service nor looked for on
/// the network, nor kept, and the keys made for it stay for the next try. CONTROL: the same station
/// with its own keys beside its proof is trusted and registered; found nowhere on this network, it
/// is kept by the service's id with exactly the two keys its proof vouched for pinned.
#[tokio::test]
async fn a_station_whose_proof_does_not_hold_is_neither_registered_nor_looked_for() {
    let station = fixture_key("station");
    let proof = fixture_token("station");
    for (lan, subject, trusted) in [
        // Another LAN key beside the proof: a service standing in another station's network key.
        ("computerLan", "auth0|test-owner", false),
        // The owner's station, but the account signing in is someone else's.
        ("stationLan", "auth0|test-stranger", false),
        ("stationLan", "auth0|test-owner", true),
    ] {
        let (cloud, scratch, store) = (
            Cloud::start().await,
            Scratch::new(),
            StationStore::default(),
        );
        cloud.lists(json!([row(
            &station,
            &fixture_key(lan),
            Some(&proof),
            "owner"
        )]));
        cloud.seen.lock().unwrap().subject = subject.into();
        let (find, looked) = finding(Vec::new());
        let kept = stations(&store, &scratch.0);
        let env = env(&cloud, kept.clone(), find, proofs::listed_station);
        let staged = kept.staged().unwrap();
        let (signed, _) = signing_in(&env, &cloud, None, no_pick(), answering(Vec::new()).0).await;
        let registered = cloud
            .seen
            .lock()
            .unwrap()
            .paths()
            .iter()
            .filter(|path| path.ends_with("/client/device"))
            .count();
        if trusted {
            // Registered; then nowhere on this network, and the operator said nothing: kept by its
            // service id, the LAN key its proof vouched for pinned, its road beside it. The service
            // approves it by default here, so the road is granted on its sign-in (the 2026-10-23 sign-in decision) and
            // does not wait for Approve at the station.
            let key = (cloud.seen.lock().unwrap().devices.first())
                .map(|(body, _)| shown(body))
                .unwrap_or_default();
            assert_eq!(
                signed,
                Ok(Signed {
                    station: SERVICE_STATION.into(),
                    waiting: false,
                    away: true,
                    key
                }),
                "{lan} {subject}"
            );
            assert_eq!(registered, 1);
            assert_eq!(looked.load(Ordering::SeqCst), 1);
            let away = kept.away(SERVICE_STATION).unwrap().expect("kept");
            assert_eq!(
                (away.station_key, away.pkcs8),
                (fixture_key("stationLan"), staged.lan.clone())
            );
            let road = kept.internet(SERVICE_STATION).unwrap().expect("its road");
            assert_eq!(
                (road.station_key, road.device_id, road.pkcs8),
                (station.clone(), device_at(0), staged.road.clone())
            );
            assert!(
                store.staged_keys().unwrap().is_none(),
                "the keys are the station's now"
            );
        } else {
            assert_eq!(signed, Err("stationProofFailed"), "{lan} {subject}");
            assert_eq!(registered, 0, "{lan} {subject}: registered");
            assert_eq!(
                looked.load(Ordering::SeqCst),
                0,
                "{lan} {subject}: looked for"
            );
            assert!(kept.away(SERVICE_STATION).unwrap().is_none());
            assert!(
                store.staged_keys().unwrap().is_some(),
                "the keys stay for the next try"
            );
        }
    }
}

/// ★ Signed in from elsewhere: a computer that has never been on the station's network (a laptop away, a guest
/// who lives elsewhere) keeps a station its sign-in vouched for, found nowhere here and with no
/// address from the operator, by the service's id: registered with its LAN key and proof as on the
/// network, its road kept under that id for Operate, and this computer's two keys with it; no time
/// is kept, since no admission on the station's network rests on this sign-in. A second sign-in
/// for the account finds it here (`alreadyHere`) and registers nothing: one device at the service.
/// A guest invited to operate is kept the same way; one invited only to listen is not kept; and a
/// computer holding eight stations, one of them kept this way, adds no ninth: refused before the
/// browser opens. A road the service APPROVED no longer waits for Approve at the station (the
/// 2026-10-23 sign-in decision: the station grants control on a sign-in,
/// `remote_service::account::GRANTS_ON_SIGN_IN`); one not yet approved still waits.
#[tokio::test]
async fn a_station_found_nowhere_here_is_kept_by_its_service_id_for_the_internet_road() {
    let (lan_key, _) = ComputerKey::generate().unwrap();
    let station = fixture_key("station");
    for (role, approve) in [("owner", true), ("operate", true), ("owner", false)] {
        let (cloud, scratch, store) = (
            Cloud::start().await,
            Scratch::new(),
            StationStore::default(),
        );
        cloud.lists(json!([row(
            &station,
            lan_key.public_key(),
            Some("p"),
            role
        )]));
        cloud.seen.lock().unwrap().approve = approve;
        let kept = stations(&store, &scratch.0);
        let env = env(&cloud, kept.clone(), finding(Vec::new()).0, trusting);
        let staged = kept.staged().unwrap();
        let (ask, asked) = answering(Vec::new());
        let (signed, _) = signing_in(&env, &cloud, None, no_pick(), ask).await;
        let key = (cloud.seen.lock().unwrap().devices.first())
            .map(|(body, _)| shown(body))
            .unwrap_or_default();
        assert_eq!(
            signed,
            Ok(Signed {
                station: SERVICE_STATION.into(),
                waiting: !approve,
                away: true,
                key
            }),
            "{role}"
        );
        assert_eq!(*asked.lock().unwrap(), vec![("Shack".to_string(), None)]);
        {
            let seen = cloud.seen.lock().unwrap();
            let (body, _) = seen.devices[0].clone();
            assert_eq!(
                (&body["publicKey"], &body["lanKey"]),
                (
                    &json!(ComputerKey::restore(&staged.road).unwrap().public_key()),
                    &json!(ComputerKey::restore(&staged.lan).unwrap().public_key())
                ),
                "{role}"
            );
            assert!(body["proof"].is_string(), "{body}");
        }
        assert!(kept.list().unwrap().is_empty(), "a station on this network");
        let shown = kept.list_away().unwrap();
        assert_eq!(
            shown.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            ["Shack"]
        );
        let away = kept.away(SERVICE_STATION).unwrap().unwrap();
        assert_eq!(
            (away.station_key, away.pkcs8),
            (lan_key.public_key().to_string(), staged.lan.clone())
        );
        assert_eq!(kept.signed(SERVICE_STATION), None, "a time kept");
        // Signed in again for the same account: it is here, and nothing is registered twice.
        let (again, _) = signing_in(&env, &cloud, None, no_pick(), answering(Vec::new()).0).await;
        assert_eq!(again, Err("alreadyHere"), "{role}");
        assert_eq!(cloud.seen.lock().unwrap().devices.len(), 1, "{role}");
        assert_eq!(cloud.seen.lock().unwrap().keys.len(), 1, "{role}");
    }
    // Eight on this computer, one of them kept by its service id: a ninth is refused before
    // anything is registered.
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    cloud.lists(json!([row(
        &station,
        lan_key.public_key(),
        Some("p"),
        "owner"
    )]));
    let kept = stations(&store, &scratch.0);
    for n in 0..7u8 {
        let (_, pkcs8) = ComputerKey::generate().unwrap();
        let (station_key, _) = ComputerKey::generate().unwrap();
        kept.keep(&PairedStation {
            station_id: format!("60000000-0000-4000-8000-0000000000{n:02x}"),
            device_id: device_at(9),
            station_key: station_key.public_key().into(),
            pkcs8,
            addresses: vec![format!("192.168.1.{}:42075", 20 + n)],
        })
        .unwrap();
    }
    let elsewhere =
        crate::lan_client::tests::away_record("20000000-0000-4000-8000-0000000000a9", "Club");
    let road = crate::lan_client::tests::road_to("20000000-0000-4000-8000-0000000000a9");
    kept.keep_away(&elsewhere, &road).unwrap();
    let full = env(&cloud, kept.clone(), finding(Vec::new()).0, trusting);
    let (signed, opened) =
        signing_in(&full, &cloud, None, no_pick(), answering(Vec::new()).0).await;
    assert_eq!(signed, Err("stationsFull"));
    assert_eq!(opened, "", "the browser opened");
    assert!(cloud.seen.lock().unwrap().devices.is_empty());
    // Invited only to listen: not registered, not kept.
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    cloud.lists(json!([row(
        &station,
        lan_key.public_key(),
        Some("p"),
        "listen"
    )]));
    let kept = stations(&store, &scratch.0);
    let env = env(&cloud, kept.clone(), finding(Vec::new()).0, trusting);
    let (signed, _) = signing_in(&env, &cloud, None, no_pick(), answering(Vec::new()).0).await;
    assert_eq!(signed, Err("listenOnly"));
    assert!(kept.list_away().unwrap().is_empty());
    assert!(cloud.seen.lock().unwrap().devices.is_empty());
}

/// ★ The sign-in carries the hash of exactly the two keys this computer made for the station, to
/// this build's own tenant and Native application (never the ones the service's config names), and
/// the token the issuer gives must answer that hash. CONTROL: a token naming another nonce goes
/// nowhere (`signInFailed`): the account's stations are not even asked for.
#[tokio::test]
async fn the_sign_in_names_exactly_this_computers_two_keys() {
    for wrong in [false, true] {
        let (cloud, scratch, store) = (
            Cloud::start().await,
            Scratch::new(),
            StationStore::default(),
        );
        if wrong {
            cloud.seen.lock().unwrap().wrong_nonce = Some("another-nonce".into());
        }
        let env = env(
            &cloud,
            stations(&store, &scratch.0),
            finding(Vec::new()).0,
            trusting,
        );
        let (signed, opened) =
            signing_in(&env, &cloud, None, no_pick(), answering(Vec::new()).0).await;
        let staged = store.staged_keys().unwrap().unwrap();
        let nonce = nonce_for(&staged.road, &staged.lan);
        assert!(opened.ends_with(&format!("&nonce={nonce}")), "{opened}");
        assert!(opened.contains(&format!("client_id={NATIVE}&")), "{opened}");
        assert!(opened.contains("audience=https%3A%2F%2Fapi.test.invalid%2F"));
        // Who signed in, and the address the issuer verified: how a station knows a person it
        // invited. Never a name, a nickname or a picture.
        assert!(opened.contains("&scope=openid%20email&"), "{opened}");
        let paths = cloud.seen.lock().unwrap().paths();
        if wrong {
            assert_eq!(signed, Err("signInFailed"));
            assert!(!paths.iter().any(|p| p.contains("client/")), "{paths:?}");
        } else {
            // Signed in; the account lists no station.
            assert_eq!(signed, Err("stationUnavailable"));
            assert!(paths.contains(&"/api/remote/client/session".to_string()));
        }
    }
}

/// ★ A station listed with no proof (its owner never signed in at the shack), one the account is
/// invited only to listen at, and one already on this computer are not added: each said by name,
/// and nothing registered. CONTROL: an empty list says the account has no station for this computer.
#[tokio::test]
async fn a_station_this_computer_cannot_be_added_to_is_said_by_name() {
    let (lan_key, _) = ComputerKey::generate().unwrap();
    let station = fixture_key("station");
    for (rows, why) in [
        (
            json!([row(&station, lan_key.public_key(), None, "owner")]),
            "stationNotProven",
        ),
        (
            json!([row(&station, lan_key.public_key(), Some("p"), "listen")]),
            "listenOnly",
        ),
        (
            json!([row(&station, lan_key.public_key(), Some("p"), "owner")]),
            "alreadyHere",
        ),
        (json!([]), "stationUnavailable"),
    ] {
        let (cloud, scratch, store) = (
            Cloud::start().await,
            Scratch::new(),
            StationStore::default(),
        );
        cloud.lists(rows);
        let stations = stations(&store, &scratch.0);
        if why == "alreadyHere" {
            let (_, pkcs8) = ComputerKey::generate().unwrap();
            stations
                .keep(&PairedStation {
                    station_id: LAN_STATION.into(),
                    device_id: device_at(0),
                    station_key: lan_key.public_key().into(),
                    pkcs8,
                    addresses: vec!["192.168.1.20:42075".into()],
                })
                .unwrap();
        }
        let env = env(&cloud, stations, finding(Vec::new()).0, trusting);
        let (signed, _) = signing_in(&env, &cloud, None, no_pick(), answering(Vec::new()).0).await;
        assert_eq!(signed, Err(why));
        let paths = cloud.seen.lock().unwrap().paths();
        assert!(
            !paths.iter().any(|p| p.ends_with("/client/device")),
            "{why}: {paths:?}"
        );
    }
}

/// ★ A station paired here with a code whose owner never signed in at the shack: the account lists
/// it with no proof, so the internet road alone is registered, as "Add the internet road" did:
/// without a proof or a LAN key, for the station the account lists, waiting for Approve at the
/// station. Nothing is looked for on the network, and no sign-in time is kept. CONTROL: an
/// entitlement that does not take in the internet road registers nothing; a station that has its
/// road already is told it has nothing to add (`stationNotProven`).
#[tokio::test]
async fn a_code_paired_station_whose_owner_never_signed_in_gets_its_internet_road_alone() {
    let (shack_lan, _) = ComputerKey::generate().unwrap();
    let station = fixture_key("station");
    for case in ["road", "noInternet", "hasRoad"] {
        let (cloud, scratch, store) = (
            Cloud::start().await,
            Scratch::new(),
            StationStore::default(),
        );
        let entitlement = if case == "noInternet" {
            json!({"enabled": true, "expiresAt": 1})
        } else {
            json!({"access": "internet"})
        };
        cloud.seen.lock().unwrap().session = json!({"entitlement": entitlement,
            "stations": [row(&station, shack_lan.public_key(), None, "owner")]});
        cloud.seen.lock().unwrap().approve = false;
        let stations = stations(&store, &scratch.0);
        let (_, pkcs8) = ComputerKey::generate().unwrap();
        stations
            .keep(&PairedStation {
                station_id: LAN_STATION.into(),
                device_id: device_at(9),
                station_key: shack_lan.public_key().into(),
                pkcs8,
                addresses: vec!["192.168.1.20:42075".into()],
            })
            .unwrap();
        if case == "hasRoad" {
            let (_, road) = ComputerKey::generate().unwrap();
            stations
                .keep_internet(
                    LAN_STATION,
                    &InternetRoad {
                        station_id: SERVICE_STATION.into(),
                        device_id: device_at(5),
                        station_key: station.clone(),
                        pkcs8: road,
                    },
                )
                .unwrap();
        }
        let (find, looked) = finding(Vec::new());
        let env = env(&cloud, stations.clone(), find, trusting);
        let (signed, _) = signing_in(
            &env,
            &cloud,
            Some(LAN_STATION.into()),
            no_pick(),
            answering(Vec::new()).0,
        )
        .await;
        let seen = cloud.seen.lock().unwrap();
        assert_eq!(looked.load(Ordering::SeqCst), 0, "{case}: looked for");
        assert_eq!(stations.signed(LAN_STATION), None, "{case}");
        match case {
            "road" => {
                // Waiting for Approve at the station, which shows this key when it asks.
                let key = seen.devices.last().map(|(body, _)| shown(body));
                assert_eq!(
                    signed,
                    Ok(Signed {
                        station: LAN_STATION.into(),
                        waiting: true,
                        away: false,
                        key: key.unwrap_or_default()
                    })
                );
                let (body, authorization) = seen.devices.last().unwrap().clone();
                assert_eq!(authorization, format!("Bearer {ACCESS}"));
                assert!(
                    body.get("proof").is_none() && body.get("lanKey").is_none(),
                    "{body}"
                );
                let road = stations.internet(LAN_STATION).unwrap().unwrap();
                assert_eq!(
                    (road.station_id.as_str(), road.station_key.as_str()),
                    (SERVICE_STATION, station.as_str())
                );
                assert_eq!(
                    body["publicKey"],
                    ComputerKey::restore(&road.pkcs8).unwrap().public_key()
                );
                assert!(
                    store.staged_keys().unwrap().is_none(),
                    "the staged road key is kept"
                );
            }
            "noInternet" => {
                assert_eq!(signed, Err("internetNotIncluded"));
                assert!(seen.devices.is_empty());
                assert!(stations.internet(LAN_STATION).unwrap().is_none());
            }
            _ => {
                assert_eq!(signed, Err("stationNotProven"));
                assert!(seen.devices.is_empty());
            }
        }
    }
}

/// ★ Security review (2026-10-20): the service may list a station whose own proof holds under any
/// id it likes, since the proof vouches for the station's keys and not for the service's id. Listed
/// under the id of a station this computer holds on its network (another key), the station found
/// nowhere here is not kept (`notStation`), and the station held under that id keeps its record,
/// its keys and its road as they were; nothing is kept under that id. CONTROL: the same station
/// listed under an id of its own is kept by it (`a_station_found_nowhere_here_is_kept_by_…`).
#[tokio::test]
async fn a_station_listed_under_another_stations_id_replaces_nothing() {
    let (lan_key, _) = ComputerKey::generate().unwrap();
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let kept = stations(&store, &scratch.0);
    let (held_key, _) = ComputerKey::generate().unwrap();
    let (_, held_pkcs8) = ComputerKey::generate().unwrap();
    kept.keep(&PairedStation {
        station_id: LAN_STATION.into(),
        device_id: device_at(7),
        station_key: held_key.public_key().into(),
        pkcs8: held_pkcs8,
        addresses: vec!["192.168.1.99:42075".into()],
    })
    .unwrap();
    kept.keep_internet(
        LAN_STATION,
        &crate::lan_client::tests::road_to("20000000-0000-4000-8000-0000000000b0"),
    )
    .unwrap();
    let held = |store: &StationStore| {
        (
            store.raw(&format!("lan-station-{LAN_STATION}")),
            store.raw(&format!("lan-internet-{LAN_STATION}")),
            store.raw("lan-stations"),
        )
    };
    let before = held(&store);
    let mut listed = row(
        &fixture_key("station"),
        lan_key.public_key(),
        Some("p"),
        "owner",
    );
    listed["id"] = json!(LAN_STATION);
    cloud.lists(json!([listed]));
    let env = env(&cloud, kept.clone(), finding(Vec::new()).0, trusting);
    let (signed, _) = signing_in(&env, &cloud, None, no_pick(), answering(Vec::new()).0).await;
    assert_eq!(signed, Err("notStation"));
    assert_eq!(held(&store), before, "the station held changed");
    assert!(store.raw(&format!("lan-away-{LAN_STATION}")).is_none());
    assert!(kept.list_away().unwrap().is_empty());
}

/// A station on this computer it does not know is no sign-in: the browser is never opened.
#[tokio::test]
async fn an_unknown_station_signs_in_nothing() {
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let env = env(
        &cloud,
        stations(&store, &scratch.0),
        finding(Vec::new()).0,
        trusting,
    );
    let (signed, opened) = signing_in(
        &env,
        &cloud,
        Some(LAN_STATION.into()),
        no_pick(),
        answering(Vec::new()).0,
    )
    .await;
    assert_eq!(signed, Err("unknownStation"));
    assert_eq!(opened, "");
    assert!(cloud.seen.lock().unwrap().requests.is_empty());
}

/// The station's refusals of an enrolment, as the page names them: only those it knows, the proof's
/// own failures as one, and anything else as no station's answer.
#[test]
fn a_stations_refusals_are_named_as_the_page_says_them() {
    for (said, named) in [
        ("updateStation", "updateStation"),
        ("updateComputer", "updateComputer"),
        ("enrolClosed", "stationNotSignedIn"),
        ("proofNotYours", "proofNotYours"),
        ("listenNative", "listenOnly"),
        ("pairingFull", "pairingFull"),
        ("deviceCacheFull", "pairingFull"),
        ("unavailable", "stationCouldNotKeep"),
        ("credentialStoreUnavailable", "stationCouldNotKeep"),
        ("tokenNonce", "proofRefused"),
        ("tokenIssuedAt", "proofRefused"),
        ("ownerIssuer", "proofRefused"),
        ("deviceKind", "proofRefused"),
        ("<script>", "notStation"),
    ] {
        assert_eq!(refusal(said), named, "{said}");
    }
}
