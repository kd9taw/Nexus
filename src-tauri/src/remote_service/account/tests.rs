//! The station's account against a stand-in on loopback that is both the Remote service and its
//! issuer: the sign-in's redirect, the token exchange, the key set and `client/station`, each
//! checked as the real ones would. The tokens are tempo-client's account fixture: minted by
//! throwaway keys that never left a scratch process, so they prove nothing to anyone.
use super::*;
use crate::account_sign_in::tests::ask;
use crate::remote_service::vault::People;
use std::sync::{Arc, Mutex};
use tempo_client::account::{CachedDevice, Invite, Role};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const CASES: &str =
    include_str!("../../../../crates/tempo-client/tests/fixtures/account-proof-cases.json");
const STATION: &str = "20000000-0000-4000-8000-000000000001";
const ACCOUNT: &str = "40000000-0000-4000-8000-000000000004";
const ACCESS: &str = "access-token-never-to-be-shown";
const CREDENTIAL: &str = "station-credential-never-to-be-shown";

fn cases() -> Value {
    serde_json::from_str(CASES).unwrap()
}
fn now() -> u64 {
    cases()["now"].as_u64().unwrap()
}
fn key(name: &str) -> String {
    cases()["keys"][name].as_str().unwrap().to_string()
}
fn token(name: &str) -> String {
    cases()["tokens"][name].as_str().unwrap().to_string()
}

/// The account entries, in memory.
#[derive(Default)]
struct Store {
    owner: Mutex<Option<Owner>>,
    keys: Mutex<Option<AccountKeys>>,
    cache: Mutex<Option<DeviceCache>>,
    people: Mutex<Option<People>>,
    devices: Mutex<()>,
}
impl AccountVault for Store {
    fn save_people(&self, people: &People) -> Result<(), &'static str> {
        *self.people.lock().unwrap() = Some(people.clone());
        Ok(())
    }
    fn owner(&self) -> Result<Option<Owner>, &'static str> {
        Ok(self.owner.lock().unwrap().clone())
    }
    fn save_owner(&self, owner: &Owner) -> Result<(), &'static str> {
        *self.owner.lock().unwrap() = Some(owner.clone());
        Ok(())
    }
    fn account_keys(&self) -> Result<Option<AccountKeys>, &'static str> {
        Ok(self.keys.lock().unwrap().clone())
    }
    fn save_account_keys(&self, keys: &AccountKeys) -> Result<(), &'static str> {
        *self.keys.lock().unwrap() = Some(keys.clone());
        Ok(())
    }
    fn device_cache(&self) -> Result<Option<DeviceCache>, &'static str> {
        Ok(self.cache.lock().unwrap().clone())
    }
    fn save_device_cache(
        &self,
        _: &DevicesHeld<'_>,
        cache: &DeviceCache,
    ) -> Result<(), &'static str> {
        *self.cache.lock().unwrap() = Some(cache.clone());
        Ok(())
    }
    fn devices_lock(&self) -> &Mutex<()> {
        &self.devices
    }
    fn people(&self) -> Result<Option<People>, &'static str> {
        Ok(self.people.lock().unwrap().clone())
    }
    fn revokes(&self) -> Result<Option<crate::remote_service::vault::Revokes>, &'static str> {
        Ok(None)
    }
    fn save_revokes(&self, _: &crate::remote_service::vault::Revokes) -> Result<(), &'static str> {
        Ok(())
    }
    fn uninvites(&self) -> Result<Option<crate::remote_service::vault::Uninvites>, &'static str> {
        Ok(None)
    }
    fn save_uninvites(
        &self,
        _: &crate::remote_service::vault::Uninvites,
    ) -> Result<(), &'static str> {
        Ok(())
    }
    fn notified(&self) -> Result<Option<crate::remote_service::vault::Notified>, &'static str> {
        Ok(None)
    }
    fn save_notified(
        &self,
        _: &crate::remote_service::vault::Notified,
    ) -> Result<(), &'static str> {
        Ok(())
    }
    fn ties(&self) -> Result<Option<crate::remote_service::vault::Ties>, &'static str> {
        Ok(None)
    }
    fn save_ties(
        &self,
        _: &DevicesHeld<'_>,
        _: &crate::remote_service::vault::Ties,
    ) -> Result<(), &'static str> {
        Ok(())
    }
}

/// What the stand-in saw.
#[derive(Default)]
struct Seen {
    /// Each request: method and path.
    requests: Vec<(String, String)>,
    /// The body and the authorization of the `client/station` request.
    station: Option<(Value, String)>,
    /// The same of the `station-proof` request.
    proof: Option<(Value, String)>,
    /// The token form the issuer was sent.
    form: Option<String>,
}

/// One plain request: method, path, headers and body.
async fn read_request(stream: &mut TcpStream) -> Option<(String, String, String, Vec<u8>)> {
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
    let head = std::str::from_utf8(&bytes[..end]).ok()?.to_lowercase();
    let first = std::str::from_utf8(&bytes[..end])
        .ok()?
        .lines()
        .next()?
        .to_string();
    let mut first = first.split(' ');
    let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
    let header = |name: &str| {
        head.lines()
            .find_map(|l| l.strip_prefix(&format!("{name}:")))
            .map(|v| v.trim().to_string())
    };
    let length: usize = header("content-length")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    // The authorization exactly as sent (the head above is lowercased).
    let authorization = std::str::from_utf8(&bytes[..end])
        .ok()?
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(n, _)| n.eq_ignore_ascii_case("authorization"))
                .map(|(_, v)| v.trim().to_string())
        })
        .unwrap_or_default();
    let mut body = bytes[end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some((method, path, authorization, body))
}

fn reply(status: u16, body: &Value) -> Vec<u8> {
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// The stand-in on loopback, giving `id_token` from its token endpoint: its address and what it saw.
async fn stand_in(id_token: String) -> (String, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Seen::default()));
    let held = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let seen = held.clone();
            let id_token = id_token.clone();
            tokio::spawn(async move {
                let Some((method, path, authorization, body)) = read_request(&mut stream).await
                else {
                    return;
                };
                let answer = {
                    let mut seen = seen.lock().unwrap();
                    seen.requests.push((method.clone(), path.clone()));
                    match (method.as_str(), path.as_str()) {
                        ("GET", "/api/remote/config") => reply(
                            200,
                            &json!({"issuer": "https://elsewhere.test.invalid/", "nativeClientId": "not-ours", "audience": "https://api.test.invalid/"}),
                        ),
                        ("GET", "/.well-known/jwks.json") => reply(200, &cases()["jwks"]),
                        ("POST", "/oauth/token") => {
                            seen.form = Some(String::from_utf8_lossy(&body).to_string());
                            reply(
                                200,
                                &json!({"access_token": ACCESS, "token_type": "Bearer", "expires_in": 600, "id_token": id_token}),
                            )
                        }
                        ("POST", "/api/remote/client/station") => {
                            seen.station =
                                Some((serde_json::from_slice(&body).unwrap(), authorization));
                            reply(200, &json!({"stationId": STATION, "accountId": ACCOUNT}))
                        }
                        ("POST", p)
                            if p == format!(
                                "/api/remote/stations/{STATION}/client/station-proof"
                            ) =>
                        {
                            seen.proof =
                                Some((serde_json::from_slice(&body).unwrap(), authorization));
                            reply(200, &json!({"ok": true}))
                        }
                        _ => reply(404, &json!({"error": "notFound"})),
                    }
                };
                let _ = stream.write_all(&answer).await;
            });
        }
    });
    (at, seen)
}

/// The test tenant: the fixture's issuer and applications, asked at the stand-in.
fn service(at: &str) -> Service {
    let tenant: &'static Tenant = Box::leak(Box::new(Tenant {
        origin: Box::leak(at.to_string().into_boxed_str()),
        issuer: "https://issuer.test.invalid/",
        native_client_id: "test-native-client",
        spa_client_id: "test-spa-client",
    }));
    Service::with(at, tenant, &format!("{at}/")).unwrap()
}

/// Binds with the station's keys `station` and `lan` (fixture names): the result, the authorize
/// address the browser was sent to, and what the stand-in saw.
async fn bind_with(
    id_token: &str,
    station: &str,
    lan: &str,
    store: &Store,
) -> (Result<Bound, &'static str>, String, Arc<Mutex<Seen>>) {
    let (at, seen) = stand_in(id_token.to_string()).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = listener.local_addr().unwrap();
    let opened = Arc::new(Mutex::new(String::new()));
    let kept = opened.clone();
    let (station_key, lan_key) = (key(station), key(lan));
    let result = bind_on(
        &service(&at),
        store,
        &Station {
            name: "Shack",
            station_key: &station_key,
            lan_key: &lan_key,
            credential: CREDENTIAL,
        },
        listener,
        Duration::from_secs(10),
        move |url| {
            *kept.lock().unwrap() = url.to_string();
            let state = url
                .split(['?', '&'])
                .find_map(|pair| pair.strip_prefix("state="))
                .unwrap()
                .to_string();
            // The browser coming back with the code, as the issuer sends it.
            tokio::spawn(async move {
                ask(local, &format!("/callback?code=the-code&state={state}")).await;
            });
            Ok(())
        },
        now(),
    )
    .await;
    let opened = opened.lock().unwrap().clone();
    (result, opened, seen)
}

/// ★ Binding: the sign-in carries the hash of exactly this station's two keys, the station checks
/// the issuer's token itself, and only then sends it with the keys and its credential; it keeps the
/// owner the token names, under the station id the service gave.
#[tokio::test]
async fn the_station_binds_with_a_proof_for_exactly_its_keys() {
    let store = Store::default();
    let (bound, opened, seen) = bind_with(&token("station"), "station", "stationLan", &store).await;
    assert_eq!(
        bound,
        Ok(Bound {
            station_id: STATION.into(),
            account_id: ACCOUNT.into()
        })
    );
    let nonce = cases()["nonces"]["station"].as_str().unwrap().to_string();
    assert!(opened.ends_with(&format!("&nonce={nonce}")), "{opened}");
    // The built-in application, not the one the service's config names.
    assert!(opened.contains("client_id=test-native-client&"), "{opened}");
    assert!(opened.contains("audience=https%3A%2F%2Fapi.test.invalid%2F"));
    // Who signed in, and nothing more: the proof is listed to every computer of the account and
    // of each person the station invites, so it carries no address and no name,
    // nickname or picture.
    assert!(opened.contains("&scope=openid&"), "{opened}");
    let seen = seen.lock().unwrap();
    let (body, authorization) = seen.station.clone().unwrap();
    assert_eq!(authorization, format!("Bearer {ACCESS}"));
    assert_eq!(
        body,
        json!({"name": "Shack", "credential": CREDENTIAL, "stationKey": key("station"),
            "lanKey": key("stationLan"), "proof": token("station")})
    );
    assert!(seen.form.as_deref().unwrap().contains("code=the-code"));
    assert_eq!(
        store.owner().unwrap(),
        Some(Owner {
            station_id: STATION.into(),
            issuer: "https://issuer.test.invalid/".into(),
            subject: cases()["owner"].as_str().unwrap().into(),
            proof_kid: "test-p".into(),
            proof_issued_at: cases()["issuedAt"].as_u64().unwrap(),
        })
    );
    let kept = store.account_keys().unwrap().unwrap();
    assert_eq!(kept.issuer, "https://issuer.test.invalid/");
    assert!(kept.keys.knows("test-p"));
}

/// A token that does not name this station's keys (another LAN key; a device's sign-in) is refused
/// at the station: nothing is sent to the service and no owner is kept.
#[tokio::test]
async fn a_token_for_other_keys_binds_nothing() {
    for (id_token, lan) in [("station", "computerLan"), ("computer", "stationLan")] {
        let store = Store::default();
        let (bound, _, seen) = bind_with(&token(id_token), "station", lan, &store).await;
        assert_eq!(bound, Err("tokenNonce"), "{id_token}");
        let seen = seen.lock().unwrap();
        assert!(seen.station.is_none());
        assert!(!seen
            .requests
            .iter()
            .any(|(_, path)| path.contains("client/station")));
        assert_eq!(store.owner().unwrap(), None);
    }
    // An answer with no ID token at all: a sign-in that asked for one cannot go on.
    let store = Store::default();
    let (bound, _, seen) = bind_with("", "station", "stationLan", &store).await;
    assert_eq!(bound, Err("signInFailed"));
    assert!(seen.lock().unwrap().station.is_none());
    // A forged token: the issuer never signed it.
    let store = Store::default();
    let (bound, _, _) = bind_with(&token("forged"), "station", "stationLan", &store).await;
    assert_eq!(bound, Err("tokenSignature"));
}

/// The shared sign-in: one that sent a nonce cannot go on without the ID token it asked for, whoever
/// called it. CONTROL: with no nonce, the same answer is a sign-in (the internet road's today).
#[tokio::test]
async fn a_sign_in_that_asked_for_an_id_token_needs_one() {
    let (at, _) = stand_in(String::new()).await;
    let sign = SignIn {
        issuer: format!("{at}/"),
        client_id: "test-native-client".into(),
        audience: "https://api.test.invalid/".into(),
    };
    let http = service(&at).http;
    for (nonce, expected) in [
        (Some("n"), Err("signInFailed")),
        (None, Ok(ACCESS.to_string())),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local = listener.local_addr().unwrap();
        let signed = crate::account_sign_in::signed_in(
            &http,
            &sign,
            Scope::Device,
            nonce,
            listener,
            Duration::from_secs(10),
            move |url| {
                let state = url
                    .split(['?', '&'])
                    .find_map(|pair| pair.strip_prefix("state="))
                    .unwrap()
                    .to_string();
                tokio::spawn(async move {
                    ask(local, &format!("/callback?code=c&state={state}")).await;
                });
                Ok(())
            },
        )
        .await;
        assert_eq!(signed.map(|s| s.access), expected, "{nonce:?}");
    }
}

/// ★ No secret in anything binding hands back: neither the access token, the proof nor the
/// station's credential appears in the result, the owner kept or any refusal, which are all a
/// caller could log or show. CONTROL: the stand-in did receive all three.
#[tokio::test]
async fn binding_hands_back_no_secret() {
    for (id_token, lan) in [("station", "stationLan"), ("station", "computerLan")] {
        let store = Store::default();
        let (bound, _, seen) = bind_with(&token(id_token), "station", lan, &store).await;
        let shown = format!(
            "{bound:?} {:?} {:?}",
            store.owner().unwrap(),
            store.account_keys().unwrap()
        );
        for secret in [ACCESS, CREDENTIAL, token(id_token).as_str()] {
            assert!(!shown.contains(secret), "{secret} in {shown}");
        }
        // The signature alone, too: a token's tail.
        let signature = token(id_token).rsplit('.').next().unwrap().to_string();
        assert!(!shown.contains(&signature));
        if bound.is_ok() {
            let seen = seen.lock().unwrap();
            let (body, authorization) = seen.station.clone().unwrap();
            assert!(authorization.contains(ACCESS));
            assert_eq!(body["credential"], CREDENTIAL);
            assert_eq!(body["proof"], token(id_token));
        }
    }
}

/// The account signs in only at the service this build is for (staging, whose tenant is built in),
/// never at another origin, production's included.
#[test]
fn the_account_signs_in_only_where_this_build_has_a_configured_tenant() {
    for origin in [
        "https://elsewhere.test.invalid",
        "https://remote.hamradiotools.io",
    ] {
        assert!(
            matches!(Service::new(origin), Err("originDenied")),
            "{origin}"
        );
    }
    let service = Service::new(crate::remote_service::REMOTE_ORIGIN)
        .ok()
        .unwrap();
    assert_eq!(
        service.tenant(),
        Tenant::of("https://remote-staging.hamradiotools.io").unwrap()
    );
    assert_eq!(
        service.tenant().issuer,
        "https://dev-60ma72hxdqoohki1.us.auth0.com/"
    );
}

/// The service's config without its API is no sign-in: nothing is opened.
#[tokio::test]
async fn a_service_with_no_api_audience_is_not_ready() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let _ = read_request(&mut stream).await;
            let _ = stream.write_all(&reply(200, &json!({"issuer": "x"}))).await;
        }
    });
    let sign_in = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let opened = Arc::new(Mutex::new(false));
    let kept = opened.clone();
    let result = bind_on(
        &service(&at),
        &Store::default(),
        &Station {
            name: "Shack",
            station_key: &key("station"),
            lan_key: &key("stationLan"),
            credential: CREDENTIAL,
        },
        sign_in,
        Duration::from_secs(1),
        move |_| {
            *kept.lock().unwrap() = true;
            Ok(())
        },
        now(),
    )
    .await;
    assert_eq!(result, Err("accountNotReady"));
    assert!(!*opened.lock().unwrap());
}

/// An owner whose proof's key is `kid`.
fn owner(kid: &str) -> Owner {
    Owner {
        station_id: STATION.into(),
        issuer: "https://issuer.test.invalid/".into(),
        subject: cases()["owner"].as_str().unwrap().into(),
        proof_kid: kid.into(),
        proof_issued_at: cases()["issuedAt"].as_u64().unwrap(),
    }
}

/// ★ The key set is fetched from the issuer when none is kept, kept for that issuer, and says the
/// station must sign in again when its proof's key has left it. With the issuer unreachable, the
/// kept set is used; with none kept, there is nothing to check a proof with.
#[tokio::test]
async fn the_key_set_is_kept_and_asks_for_a_new_sign_in_when_the_proofs_key_leaves() {
    let (at, seen) = stand_in(String::new()).await;
    let service = service(&at);
    let store = Store::default();
    store.save_owner(&owner("test-p")).unwrap();
    let held = keys(&service, &store, None, now()).await.unwrap();
    assert!(!held.reproof && held.set.knows("test-q"));
    assert_eq!(seen.lock().unwrap().requests.len(), 1);
    // Kept and not due: not fetched again.
    keys(&service, &store, None, now() + 60).await.unwrap();
    assert_eq!(seen.lock().unwrap().requests.len(), 1);
    // A proof naming a key the set lacks fetches again once the retry wait has passed.
    keys(&service, &store, Some("test-new"), now() + 60)
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().requests.len(), 1);
    keys(&service, &store, Some("test-new"), now() + 301)
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().requests.len(), 2);

    // Due a day after its fetch: fetched again.
    keys(&service, &store, None, now() + 301 + 86_400)
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().requests.len(), 3);

    store.save_owner(&owner("test-retired")).unwrap();
    assert!(keys(&service, &store, None, now()).await.unwrap().reproof);

    // The issuer gone: the kept set still checks; with none kept, nothing does.
    let gone = Service::with(&at, service.tenant, "http://127.0.0.1:9/").unwrap();
    let due = now() + 2 * 86_400;
    assert!(keys(&gone, &store, None, due)
        .await
        .unwrap()
        .set
        .knows("test-p"));
    assert!(matches!(
        keys(&gone, &Store::default(), None, now()).await,
        Err("issuerUnavailable")
    ));
    // A set kept for another issuer is not this one's.
    let other = Store::default();
    let mut kept = store.account_keys().unwrap().unwrap();
    kept.issuer = "https://other.test.invalid/".into();
    other.save_account_keys(&kept).unwrap();
    assert!(matches!(
        keys(&gone, &other, None, now()).await,
        Err("issuerUnavailable")
    ));
}

fn computer_row(proof: &str) -> Value {
    json!({"id": "d1", "kind": "computer", "approved": 1, "publicKey": key("road"),
        "lanKey": key("computerLan"), "proof": proof})
}

fn pin_of(name: &str) -> String {
    pin(&tempo_stream::protocol::hex_bytes(&key(name)).unwrap())
}

/// ★ A listed device is vouched for only by its own proof, against the owner the station proved
/// and the people it invited, for exactly the keys the row names.
#[test]
fn a_listed_device_is_vouched_for_by_its_own_proof() {
    let store = Store::default();
    let tenant = Tenant {
        origin: "https://service.test.invalid",
        issuer: "https://issuer.test.invalid/",
        native_client_id: "test-native-client",
        spa_client_id: "test-spa-client",
    };
    let set = KeySet::fetched(&cases()["jwks"], now()).unwrap();
    let check = |row: &Value| vouch_listed(row, &store, &tenant, &set, now());
    assert_eq!(
        check(&computer_row(&token("computer"))).err(),
        Some("stationNotBound")
    );
    store.save_owner(&owner("test-p")).unwrap();
    let (vouched, pin) = check(&computer_row(&token("computer"))).unwrap();
    assert_eq!((vouched.role, vouched.guest), (Role::Operate, false));
    assert_eq!(pin, pin_of("computerLan"));
    // A browser is admitted by its one key.
    let browser = json!({"kind": "browser", "publicKey": key("browser"), "lanKey": null,
        "proof": token("browser")});
    assert_eq!(check(&browser).map(|(_, pin)| pin), Ok(pin_of("browser")));
    // Another device's key in the row, no proof, a guest not invited.
    let mut swapped = computer_row(&token("computer"));
    swapped["lanKey"] = key("stationLan").into();
    assert_eq!(check(&swapped).err(), Some("tokenNonce"));
    let mut bare = computer_row("");
    bare.as_object_mut().unwrap().remove("proof");
    assert_eq!(check(&bare).err(), Some("proofMissing"));
    assert_eq!(
        check(&computer_row(&token("guest"))).err(),
        Some("proofNotYours")
    );
    // Invited at another station: still not this one's guest. Invited here: the invitation's role.
    let invite = vec![Invite {
        mailbox: "k9.guest@example.test".into(),
        role: Role::Listen,
    }];
    store
        .save_people(&People {
            station_id: "other".into(),
            invites: invite.clone(),
        })
        .unwrap();
    assert_eq!(
        check(&computer_row(&token("guest"))).err(),
        Some("proofNotYours")
    );
    store
        .save_people(&People {
            station_id: STATION.into(),
            invites: invite,
        })
        .unwrap();
    let (guest, _) = check(&computer_row(&token("guest"))).unwrap();
    assert_eq!((guest.role, guest.guest), (Role::Listen, true));
}

fn cached(pin: String) -> CachedDevice {
    CachedDevice {
        pin,
        role: Role::Operate,
        guest: false,
        issued_at: 10,
        confirmed_at: 10,
    }
}

/// ★ Each list the station hears sweeps the LAN's devices: one no longer listed as approved is
/// taken out and its pin handed back; a cache kept for another station is dropped whole.
#[test]
fn each_list_sweeps_the_devices_kept_for_the_lan() {
    let store = Store::default();
    assert_eq!(sweep(&[], &store, STATION, 50), Ok(vec![]));
    let mut cache = DeviceCache::new(STATION);
    cache.keep(cached(pin_of("computerLan"))).unwrap();
    cache.keep(cached(pin_of("browser"))).unwrap();
    store
        .save_device_cache(&DevicesHeld::take(&store), &cache)
        .unwrap();
    let browser_waiting = json!({"kind": "browser", "approved": 0, "publicKey": key("browser")});
    let rows = [computer_row(&token("computer")), browser_waiting];
    assert_eq!(
        sweep(&rows, &store, STATION, 50),
        Ok(vec![pin_of("browser")])
    );
    let kept = store.device_cache().unwrap().unwrap();
    assert_eq!(kept.devices.len(), 1);
    assert_eq!(kept.devices[0].confirmed_at, 50);
    assert_eq!(
        sweep(&rows, &store, "another-station", 60),
        Ok(vec![pin_of("computerLan")])
    );
    assert_eq!(
        store.device_cache().unwrap(),
        Some(DeviceCache::new("another-station"))
    );
}

// ---- Signing in again ----------------------------------------------------------------------------

/// Proves the station bound as `STATION` again, with the station's keys `station` and `lan`
/// (fixture names), the store keeping `kept` as its owner: the result, the address the browser was
/// sent to (empty when it was never opened), and what the stand-in saw.
async fn reprove_with(
    id_token: &str,
    lan: &str,
    kept: Option<Owner>,
    store: &Store,
) -> (Result<(), &'static str>, String, Arc<Mutex<Seen>>) {
    *store.owner.lock().unwrap() = kept;
    let (at, seen) = stand_in(id_token.to_string()).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = listener.local_addr().unwrap();
    let opened = Arc::new(Mutex::new(String::new()));
    let was = opened.clone();
    let (station_key, lan_key) = (key("station"), key(lan));
    let result = reprove_on(
        &service(&at),
        store,
        &Station {
            name: "",
            station_key: &station_key,
            lan_key: &lan_key,
            credential: CREDENTIAL,
        },
        STATION,
        listener,
        Duration::from_secs(10),
        move |url| {
            *was.lock().unwrap() = url.to_string();
            let state = url
                .split(['?', '&'])
                .find_map(|pair| pair.strip_prefix("state="))
                .unwrap()
                .to_string();
            tokio::spawn(async move {
                ask(local, &format!("/callback?code=the-code&state={state}")).await;
            });
            Ok(())
        },
        now(),
    )
    .await;
    let opened = opened.lock().unwrap().clone();
    (result, opened, seen)
}

/// ★ Signing in again replaces the station's proof at the service: its owner's sign-in, for exactly
/// the station's keys, checked here first, then sent with its credential to `station-proof`; the
/// owner is kept with the new proof's key id and signing time, under the same station.
#[tokio::test]
async fn signing_in_again_replaces_the_stations_proof() {
    let store = Store::default();
    let retired = Owner {
        proof_kid: "test-retired".into(),
        proof_issued_at: 1,
        ..owner("test-retired")
    };
    let (result, opened, seen) =
        reprove_with(&token("station"), "stationLan", Some(retired), &store).await;
    assert_eq!(result, Ok(()));
    // The new proof is listed as the old one was: who signed in, and nothing more (rulings 547, 548).
    assert!(opened.contains("&scope=openid&"), "{opened}");
    // Nothing kept holds a secret: neither the access token, the proof, its signature nor the
    // credential. CONTROL: the stand-in was sent the last three.
    let kept = serde_json::to_string(&store.owner().unwrap()).unwrap();
    let signature = token("station").rsplit('.').next().unwrap().to_string();
    for secret in [ACCESS, &token("station"), &signature, CREDENTIAL] {
        assert!(!kept.contains(secret), "kept: {secret}");
    }
    let seen = seen.lock().unwrap();
    let sent = seen.proof.as_ref().unwrap().0.to_string();
    assert!(sent.contains(&signature) && sent.contains(CREDENTIAL));
    assert_eq!(seen.station, None, "nothing is bound again");
    let (body, authorization) = seen.proof.clone().unwrap();
    assert_eq!(authorization, format!("Bearer {ACCESS}"));
    assert_eq!(
        body,
        json!({"credential": CREDENTIAL, "stationKey": key("station"),
            "lanKey": key("stationLan"), "proof": token("station")})
    );
    assert_eq!(
        store.owner().unwrap(),
        Some(Owner {
            station_id: STATION.into(),
            issuer: "https://issuer.test.invalid/".into(),
            subject: cases()["owner"].as_str().unwrap().into(),
            proof_kid: "test-p".into(),
            proof_issued_at: cases()["issuedAt"].as_u64().unwrap(),
        })
    );
}

/// Signing in again as anyone but the station's owner, or with a token for other keys, replaces
/// nothing: nothing is sent and the owner kept is unchanged. With no owner kept for this station,
/// the browser is never opened.
#[tokio::test]
async fn signing_in_again_takes_only_the_owners_proof_for_the_stations_keys() {
    let someone_else = Owner {
        subject: "auth0|someone-else".into(),
        ..owner("test-retired")
    };
    let elsewhere = Owner {
        issuer: "https://elsewhere.test.invalid/".into(),
        ..owner("test-retired")
    };
    for (kept, lan, refusal) in [
        (someone_else, "stationLan", "proofNotYours"),
        (elsewhere, "stationLan", "proofNotYours"),
        (owner("test-retired"), "computerLan", "tokenNonce"),
    ] {
        let store = Store::default();
        let (result, opened, seen) =
            reprove_with(&token("station"), lan, Some(kept.clone()), &store).await;
        assert_eq!(result, Err(refusal), "{refusal}");
        assert!(!opened.is_empty());
        assert_eq!(seen.lock().unwrap().proof, None, "{refusal}: sent");
        assert_eq!(store.owner().unwrap(), Some(kept), "{refusal}: kept");
    }
    let other_station = Owner {
        station_id: "20000000-0000-4000-8000-0000000000ff".into(),
        ..owner("test-retired")
    };
    for kept in [None, Some(other_station)] {
        let store = Store::default();
        let (result, opened, _) = reprove_with(&token("station"), "stationLan", kept, &store).await;
        assert_eq!(result, Err("stationNotPaired"));
        assert!(opened.is_empty(), "the browser was opened");
    }
}

/// The sign-in dispatches on what it is given: a station not bound yet is bound (`client/station`),
/// one bound already is proven again (`station-proof`) and given back its binding, bound to nothing
/// new.
#[tokio::test]
async fn a_sign_in_binds_a_new_station_and_proves_a_bound_one_again() {
    for again in [
        None,
        Some(Bound {
            station_id: STATION.into(),
            account_id: ACCOUNT.into(),
        }),
    ] {
        let store = Store::default();
        *store.owner.lock().unwrap() = Some(owner("test-retired"));
        let (at, seen) = stand_in(token("station")).await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local = listener.local_addr().unwrap();
        let reproving = again.is_some();
        let result = sign_in_on(
            &service(&at),
            &store,
            Binds {
                name: "Shack".into(),
                station_key: key("station"),
                lan_key: key("stationLan"),
                credential: CREDENTIAL.into(),
                again,
            },
            listener,
            Duration::from_secs(10),
            move |url| {
                let state = url
                    .split(['?', '&'])
                    .find_map(|pair| pair.strip_prefix("state="))
                    .unwrap()
                    .to_string();
                tokio::spawn(async move {
                    ask(local, &format!("/callback?code=the-code&state={state}")).await;
                });
                Ok(())
            },
            now(),
        )
        .await;
        assert_eq!(
            result,
            Ok(Bound {
                station_id: STATION.into(),
                account_id: ACCOUNT.into()
            })
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.proof.is_some(), reproving, "station-proof");
        assert_eq!(seen.station.is_some(), !reproving, "client/station");
    }
}
