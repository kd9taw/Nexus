//! The computer's side of Remote over this network on its own: the typed code and address, the
//! station records in a credential store that keeps Windows' size limit, and the loopback origin's
//! three gates over real TCP, each refusing a planted request on its own. What needs a station is
//! with the station's own test pieces (`remote_service::lan::tests::computer`).
use super::origin::{parse_head, Gates, Refused, SECRET_CHARS};
use super::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

const STATION: &str = "60000000-0000-4000-8000-000000000001";
const DEVICE: &str = "1d2b7c3a-4e5f-8a6b-9c7d-0e1f2a3b4c5d";

/// The OS credential store, in memory, as `StationVault` sees it: JSON in each entry, Windows'
/// size limit on every write, a lock, and a count of the writes.
#[derive(Clone, Default)]
pub(crate) struct StationStore {
    pub(crate) entries: Arc<Mutex<std::collections::BTreeMap<String, String>>>,
    pub(crate) locked: Arc<AtomicBool>,
    pub(crate) writes: Arc<AtomicUsize>,
}

impl StationStore {
    fn put(&self, name: String, value: String) -> Result<(), &'static str> {
        if self.locked.load(Ordering::SeqCst) || value.encode_utf16().count() * 2 > 2560 {
            return Err("credentialStoreUnavailable");
        }
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.entries.lock().unwrap().insert(name, value);
        Ok(())
    }

    fn get<T: serde::de::DeserializeOwned>(&self, name: &str) -> Result<Option<T>, &'static str> {
        if self.locked.load(Ordering::SeqCst) {
            return Err("credentialStoreUnavailable");
        }
        Ok(self
            .entries
            .lock()
            .unwrap()
            .get(name)
            .and_then(|v| serde_json::from_str(v).ok()))
    }

    /// The raw text of entry `name`, as the store holds it.
    pub(crate) fn raw(&self, name: &str) -> Option<String> {
        self.entries.lock().unwrap().get(name).cloned()
    }
}

impl StationVault for StationStore {
    fn paired_stations(&self) -> Result<Option<PairedStations>, &'static str> {
        self.get("lan-stations")
    }
    fn save_paired_stations(&self, list: &PairedStations) -> Result<(), &'static str> {
        self.put("lan-stations".into(), serde_json::to_string(list).unwrap())
    }
    fn paired_station(&self, id: &str) -> Result<Option<PairedStation>, &'static str> {
        self.get(&format!("lan-station-{id}"))
    }
    fn save_paired_station(&self, station: &PairedStation) -> Result<(), &'static str> {
        self.put(
            format!("lan-station-{}", station.station_id),
            serde_json::to_string(station).unwrap(),
        )
    }
    fn remove_paired_station(&self, id: &str) -> Result<(), &'static str> {
        if self.locked.load(Ordering::SeqCst) {
            return Err("credentialStoreUnavailable");
        }
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.entries
            .lock()
            .unwrap()
            .remove(&format!("lan-station-{id}"));
        Ok(())
    }
}

fn id(n: u8) -> String {
    format!("60000000-0000-4000-8000-0000000000{n:02x}")
}

/// A sound record for station `station`, with a key made for this run and never written down.
pub(crate) fn record(station: &str, addresses: &[&str]) -> PairedStation {
    let (_, pkcs8) = ComputerKey::generate().unwrap();
    let (station_key, _) = ComputerKey::generate().unwrap();
    PairedStation {
        station_id: station.into(),
        device_id: DEVICE.into(),
        station_key: station_key.public_key().to_string(),
        pkcs8,
        addresses: addresses.iter().map(|a| a.to_string()).collect(),
    }
}

/// ★ The code as typed: sixteen hexadecimal characters, either case, spaces anywhere, gives the
/// eight bytes the shack holds. CONTROL: the shack's own grouping and plain lowercase read the
/// same; anything that is not sixteen hex characters is refused.
#[test]
fn the_code_is_read_as_the_operator_types_it() {
    let bytes = [0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f, 0x60, 0x71];
    for typed in [
        "0a1b2c3d4e5f6071",
        "0A1B 2C3D 4E5F 6071",
        "0a1b2c3d4e5f6071 ",
        "  0a 1b 2c 3d 4e 5f 60 71",
        "0a1B\t2c3D\n4e5F6071",
        "0A1B\u{3000}2C3D4E5F6071",
    ] {
        assert_eq!(code(typed), Some(bytes), "{typed:?}");
    }
    for refused in [
        "",
        "0a1b2c3d4e5f607",
        "0a1b2c3d4e5f60711",
        "0a1b2c3d4e5f607g",
        "0a1b-2c3d-4e5f-6071",
        "0x0a1b2c3d4e5f6071",
        "０a1b2c3d4e5f6071",
    ] {
        assert_eq!(code(refused), None, "{refused:?}");
    }
}

/// ★ A station's address is a private IPv4 address, with or without its port: the one rule both
/// ends keep (`tempo_stream::lan::typed`). CONTROL: the station's own default port is filled in;
/// everything a station never listens on is refused.
#[test]
fn only_a_private_address_is_a_stations() {
    use tempo_stream::lan::typed;
    assert_eq!(
        typed("192.168.1.20"),
        Some("192.168.1.20:42075".parse().unwrap())
    );
    assert_eq!(
        typed(" 10.0.0.7:5000 "),
        Some("10.0.0.7:5000".parse().unwrap())
    );
    assert_eq!(
        typed("172.16.4.1:42075"),
        Some("172.16.4.1:42075".parse().unwrap())
    );
    for refused in [
        "127.0.0.1",
        "8.8.8.8",
        "100.85.1.2",
        "169.254.1.1",
        "0.0.0.0",
        "192.168.1.20:80",
        "192.168.1.20:0",
        "[fe80::1]:42075",
        "shack.local",
        "",
        "192.168.1",
    ] {
        assert_eq!(typed(refused), None, "{refused:?}");
    }
}

/// ★ The largest record this computer writes fits the smallest credential blob, and so does the
/// list of eight stations; a ninth is refused before anything is written. CONTROL: the store's
/// limit is real: an entry over 2560 bytes of UTF-16 is refused by it.
#[test]
fn every_record_fits_the_credential_store() {
    let store = StationStore::default();
    let stations = Stations::new(Arc::new(store.clone()));
    let widest = [
        "192.168.100.200:65535",
        "192.168.100.201:65535",
        "192.168.100.202:65535",
        "192.168.100.203:65535",
    ];
    for n in 0..MAX_STATIONS as u8 {
        stations.keep(&record(&id(n), &widest)).unwrap();
    }
    for n in 0..MAX_STATIONS as u8 {
        let text = store.raw(&format!("lan-station-{}", id(n))).unwrap();
        assert!(text.encode_utf16().count() * 2 <= 2560, "{}", text.len());
    }
    assert_eq!(stations.list().unwrap().len(), MAX_STATIONS);
    let writes = store.writes.load(Ordering::SeqCst);
    assert_eq!(
        stations.keep(&record(&id(99), &widest)),
        Err("stationsFull")
    );
    assert_eq!(
        store.writes.load(Ordering::SeqCst),
        writes,
        "a write for the ninth"
    );
    // The same station paired again replaces its record, with room or without.
    stations.keep(&record(&id(3), &["10.0.0.3"])).unwrap();
    assert_eq!(stations.list().unwrap().len(), MAX_STATIONS);
    assert_eq!(
        stations.get(&id(3)).unwrap().unwrap().addresses,
        vec!["10.0.0.3".to_string()]
    );
    // The control.
    assert!(store.put("too-big".into(), "x".repeat(1281)).is_err());
    assert!(store.put("fits".into(), "x".repeat(1280)).is_ok());
}

/// ★ The address that worked goes first, so it is the first tried next time; at most four are
/// kept. CONTROL: an address already first writes nothing.
#[test]
fn the_address_that_worked_is_tried_first_next_time() {
    let store = StationStore::default();
    let stations = Stations::new(Arc::new(store.clone()));
    stations
        .keep(&record(
            STATION,
            &["192.168.1.20:42075", "192.168.1.21:42075"],
        ))
        .unwrap();
    stations
        .worked(STATION, "192.168.1.21:42075".parse().unwrap())
        .unwrap();
    assert_eq!(
        stations.get(STATION).unwrap().unwrap().addresses,
        ["192.168.1.21:42075", "192.168.1.20:42075"]
    );
    for n in 30..36u8 {
        stations
            .worked(STATION, format!("10.0.0.{n}:42075").parse().unwrap())
            .unwrap();
    }
    assert_eq!(
        stations.get(STATION).unwrap().unwrap().addresses,
        [
            "10.0.0.35:42075",
            "10.0.0.34:42075",
            "10.0.0.33:42075",
            "10.0.0.32:42075"
        ]
    );
    let writes = store.writes.load(Ordering::SeqCst);
    stations
        .worked(STATION, "10.0.0.35:42075".parse().unwrap())
        .unwrap();
    assert_eq!(store.writes.load(Ordering::SeqCst), writes);
}

/// ★ Forgetting a station removes its record, this computer's key for it with it, and then takes
/// it off the list. CONTROL: another station's record stays.
#[test]
fn forgetting_a_station_removes_its_key() {
    let store = StationStore::default();
    let stations = Stations::new(Arc::new(store.clone()));
    stations.keep(&record(&id(1), &["10.0.0.1"])).unwrap();
    stations.keep(&record(&id(2), &["10.0.0.2"])).unwrap();
    stations.forget(&id(1)).unwrap();
    assert!(store.raw(&format!("lan-station-{}", id(1))).is_none());
    assert_eq!(
        stations
            .list()
            .unwrap()
            .iter()
            .map(|v| v.id.clone())
            .collect::<Vec<_>>(),
        [id(2)]
    );
    assert!(store.raw(&format!("lan-station-{}", id(2))).is_some());
}

/// ★ A record this module could not have written reads as no pairing, and the page sees only the
/// station's fingerprint, never this computer's key. CONTROL: the sound record reads back.
#[test]
fn a_record_that_is_not_sound_is_no_pairing() {
    let store = StationStore::default();
    let stations = Stations::new(Arc::new(store.clone()));
    let kept = record(STATION, &["10.0.0.1"]);
    let pkcs8 = kept.pkcs8.clone();
    stations.keep(&kept).unwrap();
    let view = stations.list().unwrap().remove(0);
    let shown = serde_json::to_string(&view).unwrap();
    assert!(
        !shown.contains(&pkcs8),
        "the page was shown this computer's key"
    );
    assert_eq!(view.key.len(), 64);
    let name = format!("lan-station-{STATION}");
    let sound: Value = serde_json::from_str(&store.raw(&name).unwrap()).unwrap();
    for (field, bad) in [
        ("stationId", json!("not-an-id")),
        ("deviceId", json!("40000000-0000-4000-8000-00000000000G")),
        ("stationKey", json!("04ab")),
        ("pkcs8", json!("00")),
        ("addresses", json!([])),
        ("addresses", json!(["8.8.8.8:42075"])),
    ] {
        let mut tampered = sound.clone();
        tampered[field] = bad;
        store
            .entries
            .lock()
            .unwrap()
            .insert(name.clone(), tampered.to_string());
        assert!(stations.get(STATION).unwrap().is_none(), "{field}");
        assert!(stations.list().unwrap().is_empty(), "{field}");
    }
    store
        .entries
        .lock()
        .unwrap()
        .insert(name, sound.to_string());
    assert!(stations.get(STATION).unwrap().is_some(), "the control");
}

/// A locked store refuses, and the caller is told so by name. CONTROL: unlocked, the same calls
/// answer.
#[test]
fn a_locked_store_says_so() {
    let store = StationStore::default();
    let stations = Stations::new(Arc::new(store.clone()));
    store.locked.store(true, Ordering::SeqCst);
    assert_eq!(stations.list(), Err("storeUnavailable"));
    assert_eq!(
        stations.keep(&record(STATION, &["10.0.0.1"])),
        Err("storeUnavailable")
    );
    store.locked.store(false, Ordering::SeqCst);
    assert_eq!(stations.list(), Ok(Vec::new()));
}

/// The key a station is pinned with is the key that signs for this computer: generated, restored
/// from its document, never another's. CONTROL: another key is another public half.
#[test]
fn a_computer_key_is_restored_from_its_record() {
    let (made, document) = ComputerKey::generate().unwrap();
    assert!(protocol::device_key(made.public_key()));
    assert_eq!(
        ComputerKey::restore(&document).unwrap().public_key(),
        made.public_key()
    );
    assert_ne!(
        ComputerKey::generate().unwrap().0.public_key(),
        made.public_key()
    );
    assert!(ComputerKey::restore("00").is_none());
}

// ----- The loopback origin's three gates -----

const SECRET: &str = "5ec2e75ec2e75ec2e75ec2e75ec2e75ec2e75ec2e75ec2e75ec2e75ec2e75ec2";

fn head(text: &str) -> origin::Head {
    parse_head(text.as_bytes()).expect("a head")
}

/// ★ Each gate, on its own: a request right in every way but one is refused by that one. CONTROL:
/// right in every way, the page and the socket pass, and a page's own request with no `Origin`
/// passes for a file but never for the socket.
#[test]
fn each_gate_refuses_on_its_own() {
    assert_eq!(SECRET.len(), SECRET_CHARS);
    let gates = Gates::new(42076, SECRET);
    let file = |target: &str, host: &str, origin: Option<&str>| {
        let origin = origin.map_or(String::new(), |o| format!("Origin: {o}\r\n"));
        head(&format!(
            "GET {target} HTTP/1.1\r\nHost: {host}\r\n{origin}Accept: */*"
        ))
    };
    let page = format!("/{SECRET}/lan.html");
    let socket = format!("/{SECRET}/socket");
    // The controls.
    assert_eq!(
        gates.admit(&file(&page, "127.0.0.1:42076", None), false),
        Ok("lan.html")
    );
    assert_eq!(
        gates.admit(
            &file(&page, "127.0.0.1:42076", Some("http://127.0.0.1:42076")),
            false
        ),
        Ok("lan.html")
    );
    assert_eq!(
        gates.admit(
            &file(&socket, "127.0.0.1:42076", Some("http://127.0.0.1:42076")),
            true
        ),
        Ok("socket")
    );
    // Gate 1, the secret: none, another, a prefix of it, it with no slash after.
    for target in [
        "/lan.html".to_string(),
        format!("/{}/lan.html", SECRET.replace('5', "6")),
        format!("/{}/lan.html", &SECRET[..63]),
        format!("/{SECRET}"),
        format!("/{SECRET}lan.html"),
        format!("//{SECRET}/lan.html"),
    ] {
        assert_eq!(
            gates.admit(&file(&target, "127.0.0.1:42076", None), false),
            Err(Refused::Secret),
            "{target}"
        );
    }
    // Gate 2, Host: a rebound name, another port, none, two.
    for host in [
        "evil.example:42076",
        "localhost:42076",
        "127.0.0.1:42077",
        "127.0.0.1",
    ] {
        assert_eq!(
            gates.admit(&file(&page, host, None), false),
            Err(Refused::Host),
            "{host}"
        );
    }
    assert_eq!(
        gates.admit(&head(&format!("GET {page} HTTP/1.1\r\nAccept: */*")), false),
        Err(Refused::Host)
    );
    assert_eq!(
        gates.admit(
            &head(&format!(
                "GET {page} HTTP/1.1\r\nHost: 127.0.0.1:42076\r\nHost: evil.example"
            )),
            false
        ),
        Err(Refused::Host)
    );
    // Gate 3, Origin: another origin on anything, and none, or "null", on the socket.
    for origin in [
        "https://evil.example",
        "http://127.0.0.1:42077",
        "http://localhost:42076",
        "null",
        "http://tauri.localhost",
    ] {
        assert_eq!(
            gates.admit(&file(&page, "127.0.0.1:42076", Some(origin)), false),
            Err(Refused::Origin),
            "{origin}"
        );
        assert_eq!(
            gates.admit(&file(&socket, "127.0.0.1:42076", Some(origin)), true),
            Err(Refused::Origin),
            "{origin}"
        );
    }
    assert_eq!(
        gates.admit(&file(&socket, "127.0.0.1:42076", None), true),
        Err(Refused::Origin)
    );
}

/// A store, a page and its asset, for an origin under test.
fn reach(store: StationStore) -> Reach {
    let assets: origin::Assets = Arc::new(|path: &str| match path {
        "lan.html" => Some((b"<!doctype html><p>lan</p>".to_vec(), "text/html".into())),
        "assets/lan-1.js" => Some((b"export {}".to_vec(), "text/javascript".into())),
        _ => None,
    });
    Reach {
        assets,
        stations: Arc::new(Stations::new(Arc::new(store))),
        find: Arc::new(|_| Ok(Vec::new())),
        name: "Den PC".into(),
    }
}

/// One raw request to the origin, and what it answered: all of it, or a switch to a WebSocket's
/// head (the socket then stays open, and is dropped here).
async fn ask(port: u16, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut answer = Vec::new();
    let mut chunk = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let read = tokio::time::timeout_at(deadline, stream.read(&mut chunk)).await;
        let Ok(Ok(n)) = read else { break };
        if n == 0 {
            break;
        }
        answer.extend_from_slice(&chunk[..n]);
        if answer.starts_with(b"HTTP/1.1 101 ") && answer.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&answer).to_string()
}

fn secret_of(origin: &Origin) -> String {
    let page = origin.page();
    page.split('/').nth(3).unwrap().to_string()
}

/// ★ The origin itself, over real TCP on the loopback address: a planted request that fails one
/// gate is refused with that gate's answer, for the page, its files and its socket alike, and
/// nothing is served or upgraded. CONTROL: the window's own requests are answered (the page with
/// its policy, an asset, and a socket that answers `stations`).
#[tokio::test]
async fn the_origin_answers_only_through_its_three_gates() {
    let origin = Origin::start(reach(StationStore::default())).unwrap();
    let port = origin.port();
    let secret = secret_of(&origin);
    assert_eq!(secret.len(), SECRET_CHARS);
    assert!(origin
        .page()
        .starts_with(&format!("http://127.0.0.1:{port}/")));
    let host = format!("127.0.0.1:{port}");
    let own = format!("http://127.0.0.1:{port}");
    let get = |target: &str, host: &str, extra: &str| {
        format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n{extra}\r\n")
    };
    let upgrade = |origin: &str| {
        format!(
            "Origin: {origin}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n"
        )
    };
    // The controls.
    let page = ask(
        port,
        &get(&format!("/{secret}/lan.html?lang=de"), &host, ""),
    )
    .await;
    assert!(page.starts_with("HTTP/1.1 200 "), "{page}");
    assert!(
        page.contains("Content-Security-Policy: default-src 'none'"),
        "{page}"
    );
    assert!(page.contains(&format!("connect-src 'self' ws://127.0.0.1:{port};")));
    assert!(page.ends_with("<p>lan</p>"));
    let asset = ask(port, &get(&format!("/{secret}/assets/lan-1.js"), &host, "")).await;
    assert!(asset.starts_with("HTTP/1.1 200 "), "{asset}");
    let socket = ask(
        port,
        &get(&format!("/{secret}/socket"), &host, &upgrade(&own)),
    )
    .await;
    assert!(socket.starts_with("HTTP/1.1 101 "), "{socket}");
    assert!(socket.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
    // Gate 1 alone.
    for target in [
        "/lan.html".to_string(),
        format!("/{}/lan.html", "0".repeat(64)),
    ] {
        let refused = ask(port, &get(&target, &host, "")).await;
        assert!(refused.starts_with("HTTP/1.1 404 "), "{refused}");
    }
    let refused = ask(port, &get("/socket", &host, &upgrade(&own))).await;
    assert!(refused.starts_with("HTTP/1.1 404 "), "{refused}");
    // Gate 2 alone: a name rebound to this address.
    let rebound = format!("evil.example:{port}");
    let refused = ask(port, &get(&format!("/{secret}/lan.html"), &rebound, "")).await;
    assert!(refused.starts_with("HTTP/1.1 421 "), "{refused}");
    let refused = ask(
        port,
        &get(&format!("/{secret}/socket"), &rebound, &upgrade(&own)),
    )
    .await;
    assert!(refused.starts_with("HTTP/1.1 421 "), "{refused}");
    // Gate 3 alone: another page's socket, a socket naming no origin, a file for another origin.
    let refused = ask(
        port,
        &get(
            &format!("/{secret}/socket"),
            &host,
            &upgrade("https://evil.example"),
        ),
    )
    .await;
    assert!(refused.starts_with("HTTP/1.1 403 "), "{refused}");
    let bare = upgrade(&own).replace(&format!("Origin: {own}\r\n"), "");
    let refused = ask(port, &get(&format!("/{secret}/socket"), &host, &bare)).await;
    assert!(refused.starts_with("HTTP/1.1 403 "), "{refused}");
    let refused = ask(
        port,
        &get(
            &format!("/{secret}/lan.html"),
            &host,
            "Origin: https://evil.example\r\n",
        ),
    )
    .await;
    assert!(refused.starts_with("HTTP/1.1 403 "), "{refused}");
    // Past the gates, only the page's own files.
    for target in [
        format!("/{secret}/index.html"),
        format!("/{secret}/assets/../lan.html"),
        format!("/{secret}/assets/.hidden"),
        format!("/{secret}/assets/"),
    ] {
        let refused = ask(port, &get(&target, &host, "")).await;
        assert!(refused.starts_with("HTTP/1.1 404 "), "{target}: {refused}");
    }
}

/// The window's own page on its socket: `stations` answers with this computer's name and its paired
/// stations, and a word the page never says ends the session. CONTROL: the stations are the
/// store's.
#[tokio::test]
async fn the_pages_socket_answers_its_own_words_only() {
    let store = StationStore::default();
    Stations::new(Arc::new(store.clone()))
        .keep(&record(STATION, &["192.168.1.20:42075"]))
        .unwrap();
    let origin = Origin::start(reach(store)).unwrap();
    let port = origin.port();
    let url = format!("ws://127.0.0.1:{port}/{}/socket", secret_of(&origin));
    let mut request =
        tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
            url.as_str(),
        )
        .unwrap();
    request.headers_mut().insert(
        "Origin",
        format!("http://127.0.0.1:{port}").parse().unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    socket
        .send(Message::Text(json!({"type":"stations"}).to_string().into()))
        .await
        .unwrap();
    let Some(Ok(Message::Text(text))) = socket.next().await else {
        panic!("no answer")
    };
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answer["type"], "stations");
    assert_eq!(answer["computer"], "Den PC");
    assert_eq!(answer["stations"][0]["id"], STATION);
    assert_eq!(answer["stations"][0]["addresses"][0], "192.168.1.20:42075");
    assert!(answer["stations"][0].get("pkcs8").is_none());
    socket
        .send(Message::Text(
            json!({"type":"runCommand","name":"set_ptt"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let ended = tokio::time::timeout(Duration::from_secs(5), socket.next()).await;
    assert!(
        matches!(ended, Ok(None | Some(Ok(Message::Close(_))) | Some(Err(_)))),
        "the session outlived a word the page never says: {ended:?}"
    );
}

/// The window's own page socket on `origin`: the secret in its path and the window's own origin.
async fn page_socket(
    origin: &Origin,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let port = origin.port();
    let url = format!("ws://127.0.0.1:{port}/{}/socket", secret_of(origin));
    let mut request =
        tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
            url.as_str(),
        )
        .unwrap();
    request.headers_mut().insert(
        "Origin",
        format!("http://127.0.0.1:{port}").parse().unwrap(),
    );
    tokio_tungstenite::connect_async(request).await.unwrap().0
}

/// ★ Found by name, for the pairing dialog's address field: `find` answers `found` with each
/// station a look found (its advertised name, address, protocol and key tag), and says whether
/// the look could be made at all. The look runs beside the session: while it waits, the page's
/// other words are answered, and a second `find` starts no second look. CONTROL: where Windows'
/// name service cannot be used the list is empty and says so; a name longer than the page reads
/// is left out, the rest kept.
#[tokio::test]
async fn the_pages_find_offers_the_stations_found_by_name() {
    use tempo_stream::lan::dnssd::{Found, Unavailable};
    let shack = Found {
        name: "Nexus 3F2A 9B1C".into(),
        address: "192.168.1.20:42075".parse().unwrap(),
        protocol: 1,
        key: "3f2a9b1c00c0ffee".into(),
    };
    let long = Found {
        name: "N".repeat(65),
        ..shack.clone()
    };
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = Arc::new(Mutex::new(held));
    let looks = Arc::new(AtomicUsize::new(0));
    let (counted, found) = (looks.clone(), vec![long, shack.clone()]);
    let origin = Origin::start(Reach {
        find: Arc::new(move |wait| {
            assert_eq!(wait, origin::FIND_FOR);
            counted.fetch_add(1, Ordering::SeqCst);
            let _ = held.lock().unwrap().recv();
            Ok(found.clone())
        }),
        ..reach(StationStore::default())
    })
    .unwrap();
    let mut socket = page_socket(&origin).await;
    for word in ["find", "find", "stations"] {
        socket
            .send(Message::Text(json!({ "type": word }).to_string().into()))
            .await
            .unwrap();
    }
    fn told(next: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>) -> Value {
        let Some(Ok(Message::Text(text))) = next else {
            panic!("no answer: {next:?}")
        };
        serde_json::from_str(&text).unwrap()
    }
    let first = tokio::time::timeout(Duration::from_secs(5), socket.next()).await;
    let first = told(first.expect("a look in progress held up the page's other words"));
    assert_eq!(first["type"], "stations", "{first}");
    release.send(()).unwrap();
    release.send(()).unwrap();
    let answer = tokio::time::timeout(Duration::from_secs(5), socket.next()).await;
    assert_eq!(
        told(answer.expect("the look found nothing to say")),
        json!({"type":"found","available":true,"shacks":[
            {"name":"Nexus 3F2A 9B1C","address":"192.168.1.20:42075","protocol":1,
             "key":"3f2a9b1c00c0ffee"}]})
    );
    assert_eq!(looks.load(Ordering::SeqCst), 1, "a second look at once");
    drop(origin);

    let origin = Origin::start(Reach {
        find: Arc::new(|_| Err(Unavailable)),
        ..reach(StationStore::default())
    })
    .unwrap();
    let mut socket = page_socket(&origin).await;
    socket
        .send(Message::Text(json!({"type":"find"}).to_string().into()))
        .await
        .unwrap();
    let answer = tokio::time::timeout(Duration::from_secs(5), socket.next()).await;
    assert_eq!(
        told(answer.unwrap()),
        json!({"type":"found","available":false,"shacks":[]})
    );
}

/// ★ Why a pairing reached nobody, as the pairing dialog says it (`tempo_stream::lan::unreached`,
/// in the card's own words): an address whose computer answers that nothing listens there is
/// `refused`, and one where nothing answers at all is `noAnswer`. Off Windows every address counts
/// as on this computer's network. On Windows, where this computer's networks are read, neither
/// address is on one of them (loopback is no adapter's network, and TEST-NET-1 is routed nowhere),
/// so both are `otherNetwork` there: that function asks it first.
#[tokio::test]
async fn a_pairing_that_reaches_nobody_says_why() {
    let (refused, nobody_answers) = if cfg!(windows) {
        ("otherNetwork", "otherNetwork")
    } else {
        ("refused", "noAnswer")
    };
    let nothing_listens = {
        let probe = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let std::net::SocketAddr::V4(at) = probe.local_addr().unwrap() else {
            unreachable!()
        };
        at
    };
    assert_eq!(
        pairing::pair(nothing_listens, [7; 8], "Den PC", &[])
            .await
            .err(),
        Some(refused)
    );
    // TEST-NET-1 (RFC 5737), routed nowhere: nothing answers.
    let nobody = "192.0.2.1:42075".parse().unwrap();
    assert_eq!(
        pairing::pair(nobody, [7; 8], "Den PC", &[]).await.err(),
        Some(nobody_answers)
    );
}
