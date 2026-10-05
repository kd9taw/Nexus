//! Remote over this network at the shack: the switch, the gate, and the ladder end to end over
//! real TCP with key pairs made for each run and never written down.
use super::super::vault::{LanDevice, LanDevices, LanKey, LanVault};
use super::channel::{self, Shared};
use super::gate::{Gate, Refused, ARRIVALS_PER_MINUTE, FAILURES_BEFORE_IGNORED, SOURCES};
use super::*;
use futures_util::{SinkExt, StreamExt};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use serde_json::{json, Value};
use std::net::IpAddr;
use std::sync::atomic::AtomicUsize;
use tokio_tungstenite::tungstenite::Message;

mod ceremony;
mod computer;
mod drops;
mod hostile;

const STATION: &str = "60000000-0000-4000-8000-000000000001";
const PEER: &str = "192.168.1.33:50000";
const VERSIONS: (u8, u8, u8) = (
    channel::PROTOCOL_VERSION,
    tempo_stream::protocol::STREAM_VERSION,
    channel::OPERATION_VERSION,
);

fn alone() -> std::sync::MutexGuard<'static, ()> {
    crate::sat_track_alone()
}

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

/// A P-256 key made for this test run and never written down: PKCS#8 as lowercase hex.
fn fixture_key() -> String {
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .unwrap();
    pkcs8.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// A computer with a key of its own: the key, its public half as the shack would pin it, and the
/// device id that pin gives.
#[derive(Clone)]
struct Computer {
    key: String,
    pin: [u8; 32],
    spki: Vec<u8>,
    device: String,
}

impl Computer {
    fn new() -> Self {
        let key = fixture_key();
        let spki = tls::Identity::new(&key, String::new())
            .unwrap()
            .public_key()
            .to_string();
        let spki = tempo_stream::protocol::hex_bytes(&spki).unwrap();
        let pin = tls::pin(&spki);
        Self {
            key,
            pin,
            spki,
            device: book::device_id(&pin),
        }
    }
}

/// The OS credential store, in memory: Remote over this network's two entries as the system's
/// would hold them (JSON, and Windows' size limit), a lock, and a count of the writes.
#[derive(Clone, Default)]
pub(crate) struct LanStore {
    pub(crate) key: Arc<Mutex<Option<String>>>,
    pub(crate) devices: Arc<Mutex<Option<String>>>,
    pub(crate) locked: Arc<AtomicBool>,
    pub(crate) writes: Arc<AtomicUsize>,
}

impl LanStore {
    /// A store holding the station key `pkcs8` for `station`, with these computers paired.
    pub(crate) fn holding(pkcs8: &str, station: &str, paired: &[(&[u8; 32], &str)]) -> Self {
        let store = Self::default();
        store
            .save_lan_key(&LanKey {
                station_id: station.into(),
                pkcs8: pkcs8.into(),
            })
            .unwrap();
        if !paired.is_empty() {
            store
                .save_lan_devices(&LanDevices {
                    station_id: station.into(),
                    devices: paired
                        .iter()
                        .map(|(pin, name)| LanDevice {
                            pin: pin.iter().map(|b| format!("{b:02x}")).collect(),
                            name: (*name).into(),
                        })
                        .collect(),
                })
                .unwrap();
        }
        store.writes.store(0, Ordering::SeqCst);
        store
    }

    fn put(&self, slot: &Mutex<Option<String>>, value: String) -> Result<(), &'static str> {
        if self.locked.load(Ordering::SeqCst) || value.encode_utf16().count() * 2 > 2560 {
            return Err("credentialStoreUnavailable");
        }
        self.writes.fetch_add(1, Ordering::SeqCst);
        *slot.lock().unwrap() = Some(value);
        Ok(())
    }

    fn get<T: serde::de::DeserializeOwned>(
        &self,
        slot: &Mutex<Option<String>>,
    ) -> Result<Option<T>, &'static str> {
        if self.locked.load(Ordering::SeqCst) {
            return Err("credentialStoreUnavailable");
        }
        match slot.lock().unwrap().as_deref() {
            Some(value) => super::super::vault::lan_entry(value),
            None => Ok(None),
        }
    }
}

impl LanVault for LanStore {
    fn lan_key(&self) -> Result<Option<LanKey>, &'static str> {
        self.get(&self.key)
    }
    fn save_lan_key(&self, key: &LanKey) -> Result<(), &'static str> {
        self.put(&self.key, serde_json::to_string(key).unwrap())
    }
    fn lan_devices(&self) -> Result<Option<LanDevices>, &'static str> {
        self.get(&self.devices)
    }
    fn save_lan_devices(&self, devices: &LanDevices) -> Result<(), &'static str> {
        self.put(&self.devices, serde_json::to_string(devices).unwrap())
    }
    fn remove_lan_devices(&self) -> Result<(), &'static str> {
        if self.locked.load(Ordering::SeqCst) {
            return Err("credentialStoreUnavailable");
        }
        self.writes.fetch_add(1, Ordering::SeqCst);
        *self.devices.lock().unwrap() = None;
        Ok(())
    }
}

/// A book on `store`, read.
fn book_on(store: &LanStore) -> Arc<Book> {
    let book = Arc::new(Book::new(Arc::new(store.clone())));
    book.load();
    book
}

fn home() -> Network {
    Network::new("192.168.1.20".parse().unwrap(), 24).unwrap()
}

/// A station with a LAN key, one paired computer, streaming on, and a fresh authority.
struct Shack {
    shared: Shared,
    computer: Computer,
    /// The paired computer's device id, as the station's book gives it.
    device: String,
    /// The station's LAN key (PKCS#8, hex) and its public half, as the computer pinned it.
    station_key: String,
    public_key: String,
    /// The credential store the station's book is kept in.
    store: LanStore,
}

fn shack(network: Network) -> Shack {
    let station_key = fixture_key();
    let computer = Computer::new();
    let store = LanStore::holding(&station_key, STATION, &[(&computer.pin, "Shack laptop")]);
    shack_with(network, station_key, computer, store)
}

/// The same, on a station whose LAN key, paired computer and credential store already exist: a
/// fresh authority and engine, and the book read from `store`, as Nexus starting again reads it.
fn shack_with(network: Network, station_key: String, computer: Computer, store: LanStore) -> Shack {
    let authority = Arc::new(Authority::default());
    let mut engine = tempo_app::engine::Engine::new("W9XYZ", "EN52", 0);
    engine.set_remote_transmit_revocation(authority.transmit_revocation());
    let mut settings = engine.settings().clone();
    settings.remote_stream = true;
    engine.apply_settings(settings);
    let engine: crate::SharedEngine = Arc::new(Mutex::new(engine));
    let identity = tls::Identity::new(&station_key, STATION.into()).unwrap();
    let public_key = identity.public_key().to_string();
    let book = book_on(&store);
    let (paired, pairing) = book.verifier();
    let shared = Shared {
        authority,
        engine,
        feeds: Feeds {
            monitor: crate::remote_monitor::Publisher::default(),
            spectrum: None,
            meters: Default::default(),
            sources: None,
            #[cfg(feature = "radio")]
            audio: None,
            stream: super::super::stream::Host::default(),
        },
        tls: tls::server(&identity, paired, pairing).unwrap(),
        desk: super::pairing::Desk::new(book, &identity).unwrap(),
        identity: Arc::new(identity),
        network,
        port: DEFAULT_PORT,
        gate: Arc::new(Gate::default()),
        handshake: channel::HANDSHAKE,
        silence: channel::SILENCE,
    };
    Shack {
        shared,
        device: computer.device.clone(),
        computer,
        station_key,
        public_key,
        store,
    }
}

type Client =
    tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>;

/// A loopback TCP pair: the shack's end, handed to `channel::connection` as if it came from any
/// address a test names, and the computer's end.
async fn pair() -> (tokio::net::TcpStream, tokio::net::TcpStream) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let computer = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (station, _) = listener.accept().await.unwrap();
    (station, computer)
}

/// TLS with `key`, pinning `station`, then the upgrade: the computer's side of a connection.
async fn open(stream: tokio::net::TcpStream, key: &str, station: &str) -> Result<Client, String> {
    let connector = tokio_rustls::TlsConnector::from(tls::client::config(key, station).unwrap());
    let name = rustls::pki_types::ServerName::try_from("nexus-station").unwrap();
    let tls = connector
        .connect(name, stream)
        .await
        .map_err(|e| format!("tls: {e}"))?;
    let (socket, _) = tokio_tungstenite::client_async("ws://nexus-station/", tls)
        .await
        .map_err(|e| format!("upgrade: {e}"))?;
    Ok(socket)
}

/// The station's `connection` for a computer at `peer` holding `key`.
async fn dial(
    s: &Shack,
    key: &str,
    peer: &str,
    stop: watch::Receiver<bool>,
) -> (tokio::task::JoinHandle<()>, Result<Client, String>) {
    let (station, computer) = pair().await;
    let task = tokio::spawn(channel::connection(
        station,
        peer.parse().unwrap(),
        s.shared.clone(),
        stop,
    ));
    (task, open(computer, key, &s.public_key).await)
}

async fn send(socket: &mut Client, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

/// The station's next message other than the status line; how the connection ended (`closed`);
/// or, if nothing came in 5 s, `silent`, which is not an end.
async fn next(socket: &mut Client) -> Value {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] != "status" {
                    return value;
                }
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            Ok(other) => return json!({"closed": format!("{other:?}")}),
            Err(_) => return json!({"silent": true}),
        }
    }
}

async fn hello(socket: &mut Client, (protocol, stream, operation): (u8, u8, u8)) -> Value {
    send(
        socket,
        json!({"type":"hello","protocol":protocol,"stream":stream,"operation":operation}),
    )
    .await;
    next(socket).await
}

/// One request and its answer. `stationBusy` is the authority's answer while the engine is held for
/// a moment (the radio loop, the status line's own read), and a page asks again: so does this, a
/// bounded number of times, with a fresh request id each time.
async fn ask(socket: &mut Client, request: Value) -> Value {
    for _ in 0..50 {
        let mut request = request.clone();
        if request["type"] != "stopTransmit" {
            request["requestId"] = json!(id());
        }
        send(socket, json!({"type":"operationRequest","request":request})).await;
        let answer = next(socket).await;
        if answer["error"] != "stationBusy" {
            return answer;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the station stayed busy")
}

/// Welcomed, then in control: the session it was given and the state the acquire answered.
async fn in_control(s: &Shack, socket: &mut Client) -> (String, Value) {
    s.shared.authority.permit_station(&s.device, true).unwrap();
    let welcome = hello(socket, VERSIONS).await;
    assert_eq!(welcome["type"], "welcome", "{welcome}");
    let state = ask(socket, json!({"type":"state","requestId":id()})).await;
    let boot = state["value"]["stationBootId"].clone();
    let acquired = ask(
        socket,
        json!({"type":"acquire","requestId":id(),"stationBootId":boot}),
    )
    .await;
    assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
    (
        welcome["sessionId"].as_str().unwrap().to_string(),
        acquired["value"].clone(),
    )
}

/// The station keyed (a phone over) under this session's own presence, as a stream would hold it.
fn keyed_under_presence(s: &Shack, session: &str, state: &Value) {
    let now = Instant::now();
    let permit = s
        .shared
        .authority
        .stream_presence(session, &s.device, state["leaseId"].as_str().unwrap(), now)
        .unwrap();
    let mut e = s.shared.engine.lock().unwrap();
    e.hold_remote_presence(permit, now);
    e.set_operating_mode("phone", false);
    e.set_ptt(true);
    assert!(e.manual_ptt(), "premise: keyed");
}

/// A live stream's heartbeat with a fresh picture, as the station takes it: the lease renewed on the
/// computer's own connection, then presence minted again from it. When presence was minted.
async fn renewed(s: &Shack, socket: &mut Client, session: &str, state: &Value) -> Instant {
    let answered = ask(
        socket,
        json!({"type":"heartbeat","requestId":id(),"leaseId":state["leaseId"]}),
    )
    .await;
    assert_eq!(answered["value"]["phase"], "controlling", "{answered}");
    let now = Instant::now();
    let permit = s
        .shared
        .authority
        .stream_presence(session, &s.device, state["leaseId"].as_str().unwrap(), now)
        .unwrap();
    s.shared
        .engine
        .lock()
        .unwrap()
        .hold_remote_presence(permit, now);
    now
}

fn halted(s: &Shack) -> bool {
    let mut e = s.shared.engine.lock().unwrap();
    e.poll_remote_transmit(Instant::now()) || !e.manual_ptt()
}

// ----- The switch -----

/// A folder of its own for one test, removed when it is dropped.
pub(crate) struct Scratch(std::path::PathBuf);
impl Scratch {
    pub(crate) fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("nexus-lan-{}-{}", std::process::id(), id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    pub(crate) fn path(&self) -> std::path::PathBuf {
        self.0.join("remote-lan.json")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// ★ Off by default. No file, an unreadable one, and one a newer Nexus wrote (a field this
/// version does not know) all read as off. CONTROL: what the operator left comes back as left,
/// under its wire keys.
#[test]
fn the_switch_ships_off_and_comes_back_as_it_was_left() {
    let scratch = Scratch::new();
    let path = scratch.path();
    assert!(!Switch::default().on, "ships on");
    assert_eq!(Switch::load(&path), Switch::default(), "no file");
    for unreadable in ["", "{", "[]", r#"{"on":"yes"}"#, r#"{"on":true,"newer":1}"#] {
        std::fs::write(&path, unreadable).unwrap();
        assert!(!Switch::load(&path).on, "read as on: {unreadable}");
    }
    let on = Switch {
        on: true,
        address: Some("192.168.1.20".parse().unwrap()),
        port: 42080,
        off: None,
    };
    on.save(&path).unwrap();
    assert_eq!(Switch::load(&path), on);
    let text = std::fs::read_to_string(&path).unwrap();
    for key in [
        r#""on": true"#,
        r#""address": "192.168.1.20""#,
        r#""port": 42080"#,
    ] {
        assert!(text.contains(key), "{key} missing from {text}");
    }
    let went_off = Switch {
        on: false,
        off: Some(Off::EndedAtShack),
        ..on
    };
    went_off.save(&path).unwrap();
    assert_eq!(Switch::load(&path), went_off);
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("endedAtShack"));
}

// ----- The gate -----

fn source(n: usize) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(10, 0, (n / 256) as u8, (n % 256) as u8))
}

/// ★ At most two connections from one address and four in all. CONTROL: a connection that
/// ends gives its place back.
#[test]
fn the_gate_holds_the_open_connection_caps() {
    let gate = Arc::new(Gate::default());
    let now = Instant::now();
    let _a1 = gate.admit(source(1), now).unwrap();
    let a2 = gate.admit(source(1), now).unwrap();
    assert_eq!(gate.admit(source(1), now).err(), Some(Refused::SourceFull));
    let _b1 = gate.admit(source(2), now).unwrap();
    let _b2 = gate.admit(source(2), now).unwrap();
    assert_eq!(gate.admit(source(3), now).err(), Some(Refused::Full));
    drop(a2);
    assert!(
        gate.admit(source(3), now).is_ok(),
        "an ended connection kept its place"
    );
}

/// ★ Ten new connections a minute from one address; the eleventh is refused, a refused one
/// counting too. Another address is untouched. CONTROL: a minute on, it may connect again.
#[test]
fn an_address_starting_too_many_connections_waits_out_the_minute() {
    let gate = Arc::new(Gate::default());
    let t0 = Instant::now();
    for n in 0..ARRIVALS_PER_MINUTE {
        let ticket = gate.admit(source(1), t0 + Duration::from_millis(n as u64));
        assert!(ticket.is_ok(), "arrival {n} refused");
    }
    let late = t0 + Duration::from_millis(100);
    assert_eq!(gate.admit(source(1), late).err(), Some(Refused::TooFast));
    assert!(
        gate.admit(source(2), late).is_ok(),
        "another address paid for it"
    );
    assert!(gate.admit(source(1), t0 + Duration::from_secs(61)).is_ok());
}

/// ★ Five failed handshakes in a minute get an address ignored for five minutes; the same
/// five spread over longer never add up. CONTROL: four are not enough, another address is never
/// ignored for it, and five minutes on it is heard again.
#[test]
fn failed_handshakes_get_an_address_ignored_for_five_minutes() {
    let s = |n: u64| Duration::from_secs(n);
    let gate = Arc::new(Gate::default());
    let t0 = Instant::now();
    for n in 0..FAILURES_BEFORE_IGNORED as u64 - 1 {
        gate.failed(source(1), t0 + s(n));
    }
    assert!(
        gate.admit(source(1), t0 + s(5)).is_ok(),
        "ignored after four"
    );
    gate.failed(source(1), t0 + s(6));
    assert_eq!(
        gate.admit(source(1), t0 + s(7)).err(),
        Some(Refused::Ignored)
    );
    assert_eq!(
        gate.admit(source(1), t0 + s(305)).err(),
        Some(Refused::Ignored)
    );
    assert!(
        gate.admit(source(2), t0 + s(7)).is_ok(),
        "another address was ignored"
    );
    assert!(
        gate.admit(source(1), t0 + s(306)).is_ok(),
        "ignored past five minutes"
    );
    let slow = Arc::new(Gate::default());
    for n in 0..10 {
        slow.failed(source(1), t0 + s(n * 20));
    }
    assert!(
        slow.admit(source(1), t0 + s(200)).is_ok(),
        "failures a minute apart added up"
    );
}

/// The gate remembers a bounded number of addresses: one more is refused until the oldest age
/// out. CONTROL: a minute on, the new one is heard.
#[test]
fn the_gate_remembers_a_bounded_number_of_addresses() {
    let gate = Arc::new(Gate::default());
    let t0 = Instant::now();
    for n in 0..SOURCES {
        gate.failed(source(n), t0);
    }
    assert_eq!(gate.admit(source(SOURCES), t0).err(), Some(Refused::Full));
    assert!(gate
        .admit(source(SOURCES), t0 + Duration::from_secs(61))
        .is_ok());
}

/// ★ A flood of connections from one address costs the gate eleven entries, whatever its rate:
/// a thousand a second for a minute, the first two held open, is two admitted and the rest
/// refused, as before, and never more than the newest eleven arrivals held. Arrivals at every
/// pace, bursts and lulls, get exactly the answer the whole minute counted out arrival by arrival
/// gives. CONTROL: that count holds every arrival of a burst, so the bound is the gate's own.
#[test]
fn a_flood_of_connections_costs_the_gate_eleven_entries() {
    let gate = Arc::new(Gate::default());
    let t0 = Instant::now();
    let (mut held, mut refused, mut most) = (Vec::new(), 0, 0);
    for n in 0..60_000 {
        match gate.admit(source(1), t0 + Duration::from_millis(n)) {
            Ok(ticket) => held.push(ticket),
            Err(_) => refused += 1,
        }
        most = most.max(gate.arrivals(source(1)));
    }
    assert_eq!((held.len(), refused), (2, 59_998));
    assert_eq!(
        most,
        ARRIVALS_PER_MINUTE + 1,
        "the arrivals held through a flood"
    );
    drop(held);

    // The minute counted out in full, against the gate, with each ticket given back at once.
    let gate = Arc::new(Gate::default());
    let mut every = std::collections::VecDeque::new();
    let (mut at, mut seed, mut largest) = (t0, 7u64, 0);
    let (mut admitted, mut too_fast) = (0, 0);
    for _ in 0..20_000 {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        // A burst a quarter of the time, otherwise a lull of up to twenty seconds.
        let gap = if seed >> 62 == 0 { 200 } else { 20_000 };
        at += Duration::from_millis((seed >> 20) % gap);
        every.push_back(at);
        while every
            .front()
            .is_some_and(|t| at.saturating_duration_since(*t) >= Duration::from_secs(60))
        {
            every.pop_front();
        }
        largest = largest.max(every.len());
        let refused = gate.admit(source(1), at).err();
        assert_eq!(
            refused == Some(Refused::TooFast),
            every.len() > ARRIVALS_PER_MINUTE,
            "{} arrivals in the minute to {at:?}, answered {refused:?}",
            every.len()
        );
        if refused.is_none() {
            admitted += 1;
        } else {
            too_fast += 1;
        }
        assert!(gate.arrivals(source(1)) <= ARRIVALS_PER_MINUTE + 1);
    }
    assert!(
        admitted > 1000 && too_fast > 1000,
        "{admitted} admitted, {too_fast} refused"
    );
    assert!(largest > ARRIVALS_PER_MINUTE + 1, "the control: {largest}");
}

// ----- The ladder, end to end over TCP -----

/// The next status line from the station, other messages passed over; `"none"` if none comes in
/// 3 s.
async fn status_line(socket: &mut Client) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        match tokio::time::timeout_at(deadline, socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] == "status" {
                    return value["rigKeyed"].clone();
                }
            }
            Ok(Some(Ok(_))) => {}
            _ => return json!("none"),
        }
    }
}

/// A CAT reading of the rig's PTT, through the station's own reading path.
fn rig_reads_keyed(
    s: &Shack,
    radio: &tempo_app::remote_monitor::provenance::Connection,
    keyed: bool,
) {
    let mut e = s.shared.engine.lock().unwrap();
    let read = e.remote_radio_read(radio, Instant::now()).unwrap();
    e.remote_observe_cat(Some(&read), Some(true));
    e.remote_observe_ptt(Some(&read), Some(keyed));
}

/// ★ The status line: whether the rig is keyed, as the station's own observation reads it, said once
/// after the welcome and again each time it changes. CONTROL: with no reading it says `null`, and a
/// reading that has not changed is not said again.
#[tokio::test]
async fn the_status_line_says_when_the_rig_is_keyed() {
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    assert_eq!(hello(&mut socket, VERSIONS).await["type"], "welcome");
    assert_eq!(
        status_line(&mut socket).await,
        Value::Null,
        "no reading yet"
    );
    let radio = s.shared.engine.lock().unwrap().remote_open_radio().unwrap();
    rig_reads_keyed(&s, &radio, true);
    assert_eq!(
        status_line(&mut socket).await,
        json!(true),
        "keyed was not said"
    );
    rig_reads_keyed(&s, &radio, true);
    let quiet = tokio::time::timeout(Duration::from_millis(1200), status_line(&mut socket)).await;
    assert!(
        quiet.is_err(),
        "an unchanged reading was said again: {quiet:?}"
    );
    rig_reads_keyed(&s, &radio, false);
    assert_eq!(
        status_line(&mut socket).await,
        json!(false),
        "unkeyed was not said"
    );
}

/// ★ Steps 1 to 3: a paired computer on the subnet is welcomed, and stamped by the station: the
/// device its key belongs to, a session id of the station's making (each connection its own), the
/// LAN station id. The status line follows.
#[tokio::test]
async fn a_paired_computer_is_welcomed_and_stamped_by_its_key() {
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
    let mut socket = socket.unwrap();
    send(
        &mut socket,
        json!({"type":"hello","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2}),
    )
    .await;
    let welcome = match socket.next().await {
        Some(Ok(Message::Text(text))) => serde_json::from_str::<Value>(&text).unwrap(),
        other => panic!("no welcome: {other:?}"),
    };
    assert_eq!(welcome["type"], "welcome", "{welcome}");
    assert_eq!(welcome["deviceId"], s.device);
    assert_eq!(welcome["stationId"], STATION);
    let session = welcome["sessionId"].as_str().unwrap().to_string();
    assert!(super::super::transport::identifier(&session), "{session}");
    let status = match socket.next().await {
        Some(Ok(Message::Text(text))) => serde_json::from_str::<Value>(&text).unwrap(),
        other => panic!("no status line: {other:?}"),
    };
    assert_eq!(status, json!({"type":"status","rigKeyed":null}));
    let (_task, other) = dial(&s, &s.computer.key, PEER, stop).await;
    let again = hello(&mut other.unwrap(), VERSIONS).await;
    assert_ne!(
        again["sessionId"],
        json!(session),
        "two connections, one session"
    );
}

/// ★ Step 2: a key no paired computer holds is refused in the handshake and never welcomed, and
/// its address pays at the gate: after five, even the paired key is not heard from there. CONTROL:
/// the paired key from another address is welcomed.
#[tokio::test]
async fn an_unpaired_key_is_refused_in_the_handshake() {
    let s = shack(home());
    let stranger = Computer::new();
    let (_stop, stop) = watch::channel(false);
    for _ in 0..FAILURES_BEFORE_IGNORED {
        let (task, socket) = dial(&s, &stranger.key, PEER, stop.clone()).await;
        let answered = match socket {
            Ok(mut socket) => hello(&mut socket, VERSIONS).await,
            Err(e) => json!({"refused": e}),
        };
        assert!(
            answered["type"] != "welcome",
            "an unpaired key was welcomed"
        );
        task.await.unwrap();
    }
    let (task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
    let answered = match socket {
        Ok(mut socket) => hello(&mut socket, VERSIONS).await,
        Err(e) => json!({"refused": e}),
    };
    assert!(
        answered["type"] != "welcome",
        "an ignored address was heard: {answered}"
    );
    task.await.unwrap();
    let (_task, socket) = dial(&s, &s.computer.key, "192.168.1.34:50000", stop).await;
    assert_eq!(
        hello(&mut socket.unwrap(), VERSIONS).await["type"],
        "welcome"
    );
}

/// What the window writes straight after its side of the handshake: its WebSocket upgrade.
const UPGRADE: &[u8] = b"GET / HTTP/1.1\r\nHost: nexus-station\r\nConnection: Upgrade\r\n\
    Upgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";

/// TLS with `key`, pinning `s`'s key: done on the computer's side once it has sent its own key,
/// which in TLS 1.3 is before the shack judges that key.
async fn tls_with(
    s: &Shack,
    stream: tokio::net::TcpStream,
    key: &str,
) -> tokio_rustls::client::TlsStream<tokio::net::TcpStream> {
    let connector =
        tokio_rustls::TlsConnector::from(tls::client::config(key, &s.public_key).unwrap());
    let name = rustls::pki_types::ServerName::try_from("nexus-station").unwrap();
    connector
        .connect(name, stream)
        .await
        .expect("the computer's side of TLS 1.3 ends before the shack judges its key")
}

/// ★ A computer whose key is refused (not paired, and no pairing window open) reads the refusal
/// and then the end of the stream, in that order and never a reset, whether it wrote its upgrade
/// straight after its side of the handshake, as the window does, or a moment later: the shack reads
/// what came behind the key and throws it away before it closes. Closed with the upgrade unread,
/// the connection ended in a reset (what Linux shows, with the upgrade at once); an upgrade that
/// arrived after the close was answered with one (what Windows shows, with it a moment later), and
/// Windows drops a refusal that a reset overtakes, so the window said nothing answered. Each
/// refusal still counts against its address. CONTROL: the refusal is AccessDenied, which the window
/// reads as `notPaired`, and after five the paired key is not heard from that address.
#[tokio::test]
async fn a_refused_key_reads_its_refusal_and_then_an_orderly_close() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let s = shack(home());
    let stranger = Computer::new();
    let (_stop, stop) = watch::channel(false);
    let late = Duration::from_millis(20);
    for (peer, after) in [(PEER, Duration::ZERO), ("192.168.1.35:50000", late)] {
        for n in 0..FAILURES_BEFORE_IGNORED {
            let (station, computer) = pair().await;
            let task = tokio::spawn(channel::connection(
                station,
                peer.parse().unwrap(),
                s.shared.clone(),
                stop.clone(),
            ));
            let mut tls = tls_with(&s, computer, &stranger.key).await;
            tokio::time::sleep(after).await;
            tls.write_all(UPGRADE).await.unwrap();
            tls.flush().await.unwrap();
            let mut read = [0; 512];
            let refusal = tls
                .read(&mut read)
                .await
                .expect_err("a refused key was answered");
            assert!(
                refusal.to_string().contains("AccessDenied"),
                "upgrade {after:?} after, try {n}: {refusal}"
            );
            // Promptly: the shack ends its side at once, never at the end of its one second.
            let (mut raw, _) = tls.into_inner();
            let ended = tokio::time::timeout(Duration::from_millis(500), raw.read(&mut read)).await;
            assert!(
                matches!(ended, Ok(Ok(0))),
                "upgrade {after:?} after, try {n}: after the refusal, {ended:?}"
            );
            drop(raw);
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .expect("held past the computer's own close")
                .unwrap();
        }
        let (task, socket) = dial(&s, &s.computer.key, peer, stop.clone()).await;
        assert!(socket.is_err(), "the refusals from {peer} were not counted");
        task.await.unwrap();
    }
}

/// ★ What comes behind a refused key is read for one second in all, never a second per read: a
/// computer dripping a byte every 200 ms is let go about a second after its refusal, though it
/// would drip for four; one that sends 64 KiB at once is let go as soon as 16 KiB of it is read.
/// Each is let go inside the handshake's deadline, and each counts against its address. CONTROL:
/// each was still connected, the first still sending, when it was let go.
#[tokio::test]
async fn a_refused_key_is_let_go_within_a_second_however_it_sends() {
    use tokio::io::AsyncWriteExt;
    let s = shack(home());
    let stranger = Computer::new();
    let (_stop, stop) = watch::channel(false);

    let (station, computer) = pair().await;
    let task = tokio::spawn(channel::connection(
        station,
        PEER.parse().unwrap(),
        s.shared.clone(),
        stop.clone(),
    ));
    let (mut raw, _) = tls_with(&s, computer, &stranger.key).await.into_inner();
    let started = Instant::now();
    let drip = tokio::spawn(async move {
        for _ in 0..20 {
            if raw.write_all(b"x").await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    tokio::time::timeout(channel::HANDSHAKE, task)
        .await
        .expect("a dripping computer was held past the handshake's deadline")
        .unwrap();
    let held = started.elapsed();
    assert!(
        held < Duration::from_millis(1800),
        "a dripping computer was held {held:?}"
    );
    assert!(!drip.is_finished(), "the control: it stopped dripping");
    drip.abort();

    let (station, computer) = pair().await;
    let task = tokio::spawn(channel::connection(
        station,
        PEER.parse().unwrap(),
        s.shared.clone(),
        stop.clone(),
    ));
    let (mut raw, _) = tls_with(&s, computer, &stranger.key).await.into_inner();
    let started = Instant::now();
    let _ = raw.write_all(&[0x17; 64 * 1024]).await;
    tokio::time::timeout(channel::HANDSHAKE, task)
        .await
        .expect("64 KiB behind a refused key was read past the handshake's deadline")
        .unwrap();
    let held = started.elapsed();
    assert!(
        held < Duration::from_millis(800),
        "64 KiB behind a refused key was read for {held:?}"
    );
    drop(raw);

    for _ in 2..FAILURES_BEFORE_IGNORED {
        let (task, socket) = dial(&s, &stranger.key, PEER, stop.clone()).await;
        assert!(socket.is_err(), "a stranger got through TLS");
        task.await.unwrap();
    }
    let (task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    assert!(
        socket.is_err(),
        "a refused key let go early was not counted"
    );
    task.await.unwrap();
}

/// ★ Only a refused key's connection is read before its close: a handshake that fails any other
/// way, here bytes that are no TLS at all, is closed as it arrives, though the computer keeps its
/// end open. CONTROL: the shack did not wait for that computer to close.
#[tokio::test]
async fn every_other_failed_handshake_is_closed_as_it_arrives() {
    use tokio::io::AsyncWriteExt;
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    let (station, mut computer) = pair().await;
    let started = Instant::now();
    let task = tokio::spawn(channel::connection(
        station,
        PEER.parse().unwrap(),
        s.shared.clone(),
        stop,
    ));
    computer.write_all(UPGRADE).await.unwrap();
    tokio::time::timeout(channel::HANDSHAKE, task)
        .await
        .expect("plain HTTP was held past the handshake's deadline")
        .unwrap();
    let held = started.elapsed();
    assert!(
        held < Duration::from_millis(400),
        "plain HTTP was held {held:?}"
    );
    drop(computer);
}

/// ★ Step 1, the subnet check: a source off the station's subnet, loopback and a public address
/// included, is dropped as it arrives, before TLS. CONTROL: the same computer on the subnet.
#[tokio::test]
async fn a_source_off_the_subnet_is_dropped_before_tls() {
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    for off in [
        "192.168.2.33:50000",
        "127.0.0.1:50000",
        "203.0.113.9:50000",
        "[::1]:50000",
    ] {
        let (task, socket) = dial(&s, &s.computer.key, off, stop.clone()).await;
        assert!(socket.is_err(), "{off} got through TLS");
        task.await.unwrap();
    }
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    assert_eq!(
        hello(&mut socket.unwrap(), VERSIONS).await["type"],
        "welcome"
    );
}

/// ★ Versions, as ruled on 2026-10-04: the hello must carry the station's own versions. A computer
/// that speaks newer is told to update the station, an older one to update itself, and either is
/// closed; it is a paired computer, so its address does not pay. CONTROL: the station's own
/// versions are welcomed from the same address after all of them.
#[tokio::test]
async fn the_hello_must_match_and_names_the_side_to_update() {
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    let (p, st, o) = VERSIONS;
    for (versions, side) in [
        ((p + 1, st, o), "updateStation"),
        ((p, st + 1, o), "updateStation"),
        ((p, st, o + 1), "updateStation"),
        ((p - 1, st, o), "updateComputer"),
        ((p, st - 1, o), "updateComputer"),
        ((p, st, o - 1), "updateComputer"),
    ] {
        let (task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
        let mut socket = socket.unwrap();
        assert_eq!(
            hello(&mut socket, versions).await,
            json!({"type":"refused","reason":side}),
            "{versions:?}"
        );
        assert!(
            next(&mut socket).await["closed"].is_string(),
            "{versions:?}: left open"
        );
        task.await.unwrap();
    }
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    assert_eq!(
        hello(&mut socket.unwrap(), VERSIONS).await["type"],
        "welcome"
    );
}

/// Bytes written on the computer's side of TLS, under the WebSocket layer.
async fn raw(socket: &mut Client, bytes: &[u8]) {
    use tokio::io::AsyncWrite;
    let stream = socket.get_mut();
    let mut written = 0;
    while written < bytes.len() {
        written += std::future::poll_fn(|cx| {
            std::pin::Pin::new(&mut *stream).poll_write(cx, &bytes[written..])
        })
        .await
        .unwrap();
    }
    std::future::poll_fn(|cx| std::pin::Pin::new(&mut *stream).poll_flush(cx))
        .await
        .unwrap();
}

/// ★ Framing, the size limit: a frame whose header announces more than 8 KiB closes the connection
/// as that header arrives, before a byte of its payload is held. CONTROL: a header announcing
/// exactly 8 KiB is waited on for its payload.
#[tokio::test]
async fn a_frame_over_8_kib_is_refused_at_its_header() {
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    for (length, refused) in [
        (channel::MESSAGE_BYTES, false),
        (channel::MESSAGE_BYTES + 1, true),
    ] {
        let (task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
        let mut socket = socket.unwrap();
        assert_eq!(hello(&mut socket, VERSIONS).await["type"], "welcome");
        let [high, low] = (length as u16).to_be_bytes();
        // A masked text frame's header, 16-bit length, then its mask, and no payload.
        raw(&mut socket, &[0x81, 0xFE, high, low, 1, 2, 3, 4]).await;
        let ended = tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                match socket.next().await {
                    Some(Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_))) => {}
                    _ => return,
                }
            }
        })
        .await
        .is_ok();
        assert_eq!(ended, refused, "a header announcing {length} bytes");
        drop(socket);
        task.await.unwrap();
    }
}

/// ★ Framing: the first message must be the hello, and after it only the lane's own messages, each
/// at most 8 KiB. Anything else closes the connection. CONTROL: a `state` request is answered.
#[tokio::test]
async fn the_lane_takes_only_its_own_messages() {
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    let first = json!({"type":"operationRequest","request":{"type":"state","requestId":id()}});
    let (task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
    let mut socket = socket.unwrap();
    send(&mut socket, first).await;
    assert!(
        next(&mut socket).await["closed"].is_string(),
        "spoke before its hello"
    );
    task.await.unwrap();
    let big = "x".repeat(channel::MESSAGE_BYTES + 1);
    for after_hello in [
        Message::Text(json!({"type":"state","padding":big}).to_string().into()),
        Message::Binary(vec![0u8; 16].into()),
        Message::Text(
            json!({"type":"hello","protocol":1,"stream":1,"operation":4})
                .to_string()
                .into(),
        ),
        Message::Text(
            json!({"type":"applicationQuery","requestId":id()})
                .to_string()
                .into(),
        ),
        Message::Text(
            json!({"type":"operationRequest","request":{"type":"state"}})
                .to_string()
                .into(),
        ),
    ] {
        let (task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
        let mut socket = socket.unwrap();
        assert_eq!(hello(&mut socket, VERSIONS).await["type"], "welcome");
        let shown = format!("{after_hello:?}")
            .chars()
            .take(60)
            .collect::<String>();
        socket.send(after_hello).await.unwrap();
        assert!(
            next(&mut socket).await["closed"].is_string(),
            "taken: {shown}"
        );
        task.await.unwrap();
    }
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    assert_eq!(hello(&mut socket, VERSIONS).await["type"], "welcome");
    let state = ask(&mut socket, json!({"type":"state","requestId":id()})).await;
    assert_eq!(state["type"], "operationResponse", "{state}");
    assert!(state["value"]["phase"].is_string(), "{state}");
}

/// ★ The relay's other lanes stay on the relay: on this road the lane answers the lease and Stop,
/// and refuses a logging or station-control request by name. CONTROL: `state` is answered.
#[tokio::test]
async fn the_lane_answers_the_lease_and_stop_only() {
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    let (_, state) = in_control(&s, &mut socket).await;
    let refused = ask(
        &mut socket,
        json!({"type":"result","requestId":id(),"operationId":id()}),
    )
    .await;
    assert_eq!(refused["error"], "stationUnsupported", "{refused}");
    let answered = ask(
        &mut socket,
        json!({"type":"heartbeat","requestId":id(),"leaseId":state["leaseId"]}),
    )
    .await;
    assert_eq!(answered["value"]["phase"], "controlling", "{answered}");
}

/// ★ The handshake's deadline: a computer that opens TCP and never finishes TLS is dropped when the
/// deadline passes, and pays at the gate. CONTROL: the production deadline is 5 s.
#[tokio::test]
async fn a_handshake_that_runs_out_of_time_is_dropped_and_counted() {
    assert_eq!(channel::HANDSHAKE, Duration::from_secs(5));
    let mut s = shack(home());
    s.shared.handshake = Duration::from_millis(150);
    let (_stop, stop) = watch::channel(false);
    for _ in 0..FAILURES_BEFORE_IGNORED {
        let (station, _computer) = pair().await;
        let started = Instant::now();
        tokio::time::timeout(
            Duration::from_secs(5),
            channel::connection(
                station,
                PEER.parse().unwrap(),
                s.shared.clone(),
                stop.clone(),
            ),
        )
        .await
        .expect("a silent computer was held past the deadline");
        assert!(
            started.elapsed() >= Duration::from_millis(150),
            "dropped before the deadline"
        );
    }
    let (task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
    assert!(socket.is_err(), "silent handshakes were not counted");
    task.await.unwrap();
}

/// ★ A connected computer that falls silent is gone after the silence limit, and its lease
/// goes with it. CONTROL: while it keeps pinging it stays, and the production limit is 15 s.
#[tokio::test]
async fn a_silent_computer_is_gone_and_takes_its_lease() {
    assert_eq!(channel::SILENCE, Duration::from_secs(15));
    let mut s = shack(home());
    s.shared.silence = Duration::from_millis(400);
    let (_stop, stop) = watch::channel(false);
    let (task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    in_control(&s, &mut socket).await;
    for _ in 0..8 {
        socket.send(Message::Ping(Vec::new().into())).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let state = ask(&mut socket, json!({"type":"state","requestId":id()})).await;
    assert_eq!(
        state["value"]["phase"], "controlling",
        "a pinging computer was dropped"
    );
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("a silent computer was kept")
        .unwrap();
    assert_eq!(
        s.shared.authority.local_status()["controller"],
        Value::Null,
        "the lease outlived its connection"
    );
}

/// ★ Both roads at once, end to end: a computer takes control on its own connection; the relay's
/// road moving leaves it in control and on the air; its connection closing ends its lease and halts
/// what it kept on the air, at once.
#[tokio::test]
async fn a_computers_lease_is_its_connections() {
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    let (task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    let (session, state) = in_control(&s, &mut socket).await;
    keyed_under_presence(&s, &session, &state);
    s.shared.authority.start_connection();
    s.shared
        .authority
        .disconnect_session("20000000-0000-4000-8000-000000000001");
    assert!(!halted(&s), "the relay's road halted the LAN controller");
    let answered = ask(
        &mut socket,
        json!({"type":"heartbeat","requestId":id(),"leaseId":state["leaseId"]}),
    )
    .await;
    assert_eq!(answered["value"]["phase"], "controlling", "{answered}");
    drop(socket);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("the connection outlived its socket")
        .unwrap();
    assert!(
        halted(&s),
        "the station stayed keyed after its controller left"
    );
    assert_eq!(s.shared.authority.local_status()["controller"], Value::Null);
}

/// ★ Stop's second road: a Stop on the LAN channel stops what the station is transmitting,
/// and is answered while the engine is held. CONTROL: the station was keyed until it.
#[test]
fn stop_on_the_lan_channel_stops_the_station_without_waiting() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let s = shack(home());
        let (_stop, stop) = watch::channel(false);
        let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
        let mut socket = socket.unwrap();
        let (_, state) = in_control(&s, &mut socket).await;
        {
            let mut e = s.shared.engine.lock().unwrap();
            e.set_operating_mode("phone", false);
            e.set_ptt(true);
            assert!(e.manual_ptt(), "premise: keyed");
        }
        // The engine held elsewhere, as the radio loop holds it for a tick.
        let engine = s.shared.engine.clone();
        let (holding, held) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let _engine = engine.lock().unwrap();
            holding.send(()).unwrap();
            let _ = released.recv();
        });
        held.recv().unwrap();
        let answered = ask(
            &mut socket,
            json!({"type":"stopTransmit","requestId":id(),"stationBootId":state["stationBootId"],
                "leaseId":state["leaseId"],"transmitEpoch":state["transmitEpoch"]}),
        )
        .await;
        assert_eq!(answered["value"], json!({"stop":"accepted"}), "{answered}");
        // The halt itself waits for the engine, on a thread of its own, and runs once it is free.
        release.send(()).unwrap();
        holder.join().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while s.shared.engine.lock().unwrap().manual_ptt() {
            assert!(Instant::now() < deadline, "the Stop left the station keyed");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
}

// ----- The listener, on this box's own private address -----

/// This box's own private IPv4 address, the one its route to the internet leaves by, or `None`.
fn own_private_address() -> Option<Ipv4Addr> {
    let probe = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    probe.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match probe.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if tempo_stream::lan::listenable(v4) => Some(v4),
        _ => None,
    }
}

struct Running {
    lan: Lan,
    s: Shack,
    at: SocketAddr,
    scratch: Scratch,
}

/// The station's own book: its LAN key and its one paired computer, in a store of its own.
fn book_of(s: &Shack) -> Arc<Book> {
    Arc::new(Book::new(Arc::new(LanStore::holding(
        &s.station_key,
        STATION,
        &[(&s.computer.pin, "Shack laptop")],
    ))))
}

/// A book whose credential store will not answer: no key is read, and none is made.
fn locked_book() -> Arc<Book> {
    let store = LanStore::default();
    store.locked.store(true, Ordering::SeqCst);
    Arc::new(Book::new(Arc::new(store)))
}

/// The switch, kept in `scratch`, serving `s`'s station from `book` (read on the listener's own
/// thread, as Nexus reads it), advertising and reading the firewall as Nexus does (off Windows,
/// neither does anything).
fn switch_for(s: &Shack, scratch: &Scratch, resolve: Resolve, book: Arc<Book>) -> Lan {
    switch_with(s, scratch, resolve, book, advertise(), firewall_says())
}

/// The same, with the advert and the firewall a test's own.
fn switch_with(
    s: &Shack,
    scratch: &Scratch,
    resolve: Resolve,
    book: Arc<Book>,
    advertise: Advertise,
    firewall: FirewallSays,
) -> Lan {
    Lan::start(
        scratch.path(),
        Deps {
            authority: s.shared.authority.clone(),
            engine: s.shared.engine.clone(),
            feeds: s.shared.feeds.clone(),
            book,
            resolve,
            advertise,
            firewall,
        },
    )
}

/// A look at a computer whose one network, if it has one, is `network`.
pub(crate) fn only(network: Result<Network, NoNetwork>) -> Look {
    Look {
        choices: network
            .iter()
            .map(|&network| Choice {
                network,
                name: "Wi-Fi".into(),
                index: 9,
                id: "{00000000-0000-0000-0000-000000000009}".into(),
            })
            .collect(),
        network,
    }
}

/// The port at `at` refuses a connection, within a moment of the listener being told to stop.
async fn closed(at: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while tokio::net::TcpStream::connect(at).await.is_ok() {
        assert!(Instant::now() < deadline, "the port at {at} is still open");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn eventually(lan: &Lan, what: &str, done: impl Fn(&LanStatus) -> bool) -> LanStatus {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = lan.status();
        if done(&status) {
            return status;
        }
        assert!(Instant::now() < deadline, "{what}: {status:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Listening on this box's own private address, LAN on, with nobody connected yet. `None`, said on
/// stderr, on a box with no private address of its own.
async fn listening() -> Option<Running> {
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return None;
    };
    let port = std::net::TcpListener::bind((address, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let network = Network::new(address, 32).unwrap();
    let s = shack(network);
    let scratch = Scratch::new();
    let lan = switch_for(
        &s,
        &scratch,
        Arc::new(move |_| only(Ok(network))),
        book_of(&s),
    );
    lan.turn_on(None, Some(port)).unwrap();
    let at = SocketAddr::new(address.into(), port);
    eventually(&lan, "never listened", |st| {
        st.listening.as_deref() == Some(at.to_string().as_str())
    })
    .await;
    Some(Running {
        lan,
        s,
        at,
        scratch,
    })
}

/// A computer that dialled `at` (the listener, or something in front of it), welcomed, in control
/// and keyed under its session's presence: its socket, its session and the acquire's state.
async fn keyed_at(s: &Shack, at: SocketAddr) -> (Client, String, Value) {
    let stream = tokio::net::TcpStream::connect(at).await.unwrap();
    let mut socket = open(stream, &s.computer.key, &s.public_key).await.unwrap();
    let (session, state) = in_control(s, &mut socket).await;
    keyed_under_presence(s, &session, &state);
    assert!(!halted(s), "premise: on the air");
    (socket, session, state)
}

/// Listening on this box's own private address, with a computer welcomed, in control and keyed.
/// `None`, said on stderr, on a box with no private address of its own.
async fn listening_and_keyed() -> Option<(Running, Client)> {
    let r = listening().await?;
    let (socket, _, _) = keyed_at(&r.s, r.at).await;
    Some((r, socket))
}

/// ★ LAN off mid-over: turning it off at the shack ends the session, its lease and the over,
/// and closes the port. CONTROL: it was listening, in control and keyed until then.
#[tokio::test]
async fn lan_off_mid_over_ends_the_over_and_closes_the_port() {
    let Some((r, mut socket)) = listening_and_keyed().await else {
        return;
    };
    r.lan.turn_off();
    eventually(&r.lan, "still listening", |st| {
        !st.on && st.listening.is_none()
    })
    .await;
    assert!(
        next(&mut socket).await["closed"].is_string(),
        "the session outlived LAN off"
    );
    assert!(halted(&r.s), "the over outlived LAN off");
    assert_eq!(
        r.s.shared.authority.local_status()["controller"],
        Value::Null
    );
    closed(r.at).await;
    assert_eq!(
        r.lan.status().reason,
        None,
        "turned off by the operator, with a reason"
    );
}

/// ★ "End remote control" turns it off by itself and ends every session and the over; the
/// station says why, and so does the switch kept on disk.
#[tokio::test]
async fn end_remote_control_turns_it_off_and_says_why() {
    let Some((r, mut socket)) = listening_and_keyed().await else {
        return;
    };
    r.s.shared.authority.invalidate();
    r.lan.end_at_shack();
    let status = eventually(&r.lan, "still on", |st| !st.on && st.listening.is_none()).await;
    assert_eq!(status.reason, Some("endedAtShack"));
    assert!(
        next(&mut socket).await["closed"].is_string(),
        "the session outlived it"
    );
    assert!(halted(&r.s), "the over outlived it");
    closed(r.at).await;
    let kept = Switch::load(&r.scratch.path());
    assert!(!kept.on);
    assert_eq!(kept.off, Some(Off::EndedAtShack));
}

/// ★ With no LAN key there is no listener: turned on with a credential store that will not
/// answer (so no key is read, and none is made), it goes off by itself and says why. With no
/// private network to listen on it waits, on, and says why, and so it does for a picked address
/// this computer does not have right now: only the shack can turn it on again, so going off would
/// lock a computer away from the shack out. CONTROL: the switch was on each time it was asked.
#[tokio::test]
async fn it_turns_itself_off_without_a_key_and_waits_for_a_network_or_its_address() {
    let network = home();
    let s = shack(network);
    let scratch = Scratch::new();
    let lan = switch_for(
        &s,
        &scratch,
        Arc::new(move |_| only(Ok(network))),
        locked_book(),
    );
    lan.turn_on(None, None).unwrap();
    let status = eventually(&lan, "listened without a key", |st| {
        !st.on && st.reason.is_some()
    })
    .await;
    assert_eq!(status.reason, Some("noKey"));
    assert_eq!(Switch::load(&scratch.path()).off, Some(Off::NoKey));

    let scratch = Scratch::new();
    let lan = switch_for(
        &s,
        &scratch,
        Arc::new(|_| only(Err(NoNetwork::Choose))),
        book_of(&s),
    );
    lan.turn_on(None, None).unwrap();
    let status = eventually(&lan, "no reason given", |st| st.on && st.reason.is_some()).await;
    assert!(status.on, "went off without a private network");
    assert_eq!(status.reason, Some("chooseAddress"));
    assert_eq!(status.listening, None);

    let scratch = Scratch::new();
    let lan = switch_for(
        &s,
        &scratch,
        Arc::new(|_| only(Err(NoNetwork::Gone))),
        book_of(&s),
    );
    lan.turn_on(Some("192.168.1.20"), None).unwrap();
    let status = eventually(&lan, "no reason given", |st| st.reason.is_some()).await;
    assert!(status.on, "went off without its address");
    assert_eq!(status.reason, Some("addressGone"));
    assert_eq!(status.listening, None);
    assert_eq!(status.picked.as_deref(), Some("192.168.1.20"));
    let kept = Switch::load(&scratch.path());
    assert!(kept.on, "kept off on disk");
    assert_eq!(kept.off, None);
    assert_eq!(kept.address, Some("192.168.1.20".parse().unwrap()));
}

/// ★ As ruled on 2026-10-04 ("any private network"): only a private IPv4 address can be picked, and
/// only a port that is not a system one. CONTROL: a private address and a high port are taken.
#[test]
fn only_a_private_address_can_be_picked() {
    let network = home();
    let s = shack(network);
    let scratch = Scratch::new();
    let lan = switch_for(
        &s,
        &scratch,
        Arc::new(move |_| only(Ok(network))),
        locked_book(),
    );
    for refused in [
        "127.0.0.1",
        "0.0.0.0",
        "8.8.8.8",
        "100.64.0.1",
        "169.254.1.1",
        "::1",
        "fe80::1",
        "nexus.local",
        "",
    ] {
        assert_eq!(
            lan.turn_on(Some(refused), None),
            Err("invalidRequest"),
            "{refused}"
        );
    }
    assert_eq!(lan.turn_on(None, Some(80)), Err("invalidRequest"));
    assert_eq!(lan.turn_on(Some("192.168.1.20"), Some(42080)), Ok(()));
}

// ----- The network picked, the advert by name, and the firewall -----

/// ★ The pick is the operator's to keep: turning off and on again keeps it, and so does a restart
/// (it is in the switch's file); only a private address can be picked, as for turning on; no pick
/// lets the shack choose again. CONTROL: each refusal leaves the pick as it was.
#[test]
fn a_pick_is_kept_through_off_and_on() {
    let s = shack(home());
    let scratch = Scratch::new();
    let lan = switch_for(
        &s,
        &scratch,
        Arc::new(|_| only(Err(NoNetwork::Choose))),
        book_of(&s),
    );
    let picked: Option<Ipv4Addr> = Some("10.0.0.5".parse().unwrap());
    lan.pick(Some("10.0.0.5")).unwrap();
    assert_eq!(lan.status().picked.as_deref(), Some("10.0.0.5"));
    lan.turn_on(None, None).unwrap();
    lan.turn_off();
    lan.turn_on(None, None).unwrap();
    assert_eq!(Switch::load(&scratch.path()).address, picked);
    for refused in [
        "127.0.0.1",
        "8.8.8.8",
        "100.64.0.1",
        "::1",
        "nexus.local",
        "",
    ] {
        assert_eq!(lan.pick(Some(refused)), Err("invalidRequest"), "{refused}");
        assert_eq!(Switch::load(&scratch.path()).address, picked, "{refused}");
    }
    lan.pick(None).unwrap();
    assert_eq!(Switch::load(&scratch.path()).address, None);
    assert_eq!(lan.status().picked, None);
}

/// ★ A switch's file is read back only if presses here could have written it: a pick that is a
/// private address, and a port that is not a system one. Anything else reads as off, as a file
/// this version does not understand does, and so does a reason an older build kept that this one
/// no longer gives. CONTROL: a private pick and a high port come back as they were.
#[test]
fn a_pick_reads_back_only_if_a_press_could_have_made_it() {
    let scratch = Scratch::new();
    let path = scratch.path();
    for written in [
        r#"{"on":true,"address":"8.8.8.8"}"#,
        r#"{"on":true,"address":"100.64.0.1"}"#,
        r#"{"on":true,"address":"127.0.0.1"}"#,
        r#"{"on":true,"address":"169.254.1.1"}"#,
        r#"{"on":true,"port":80}"#,
        r#"{"on":false,"off":"addressGone"}"#,
    ] {
        std::fs::write(&path, written).unwrap();
        assert_eq!(Switch::load(&path), Switch::default(), "{written}");
    }
    std::fs::write(&path, r#"{"on":true,"address":"10.0.0.5","port":42080}"#).unwrap();
    assert_eq!(
        Switch::load(&path),
        Switch {
            on: true,
            address: Some("10.0.0.5".parse().unwrap()),
            port: 42080,
            off: None,
        }
    );
}

fn choice(address: &str, name: &str) -> Choice {
    Choice {
        network: Network::new(address.parse().unwrap(), 24).unwrap(),
        name: name.into(),
        index: 9,
        id: String::new(),
    }
}

/// ★ While on, the shack lists the networks it may listen on, with their adapters' names, for the
/// operator to pick from; with two and no route through either, it says to choose. Off, it lists
/// none. CONTROL: before it was turned on, none were listed.
#[tokio::test]
async fn the_networks_to_pick_from_are_listed_while_on() {
    let s = shack(home());
    let scratch = Scratch::new();
    let two = Look {
        choices: vec![
            choice("192.168.1.20", "Wi-Fi"),
            choice("10.0.0.5", "Ethernet"),
        ],
        network: Err(NoNetwork::Choose),
    };
    // Each look takes a moment, as reading a computer's adapters does, so what a press answers
    // with is the switch's own word and not the next look's.
    let slow = move |_: Option<Ipv4Addr>| {
        std::thread::sleep(Duration::from_millis(100));
        two.clone()
    };
    let lan = switch_for(&s, &scratch, Arc::new(slow), book_of(&s));
    assert!(lan.status().networks.is_empty(), "listed while off");
    lan.turn_on(None, None).unwrap();
    let status = eventually(&lan, "no networks listed", |st| !st.networks.is_empty()).await;
    assert_eq!(status.reason, Some("chooseAddress"));
    assert_eq!(
        status.networks,
        vec![
            NetworkView {
                address: "192.168.1.20".into(),
                name: "Wi-Fi".into()
            },
            NetworkView {
                address: "10.0.0.5".into(),
                name: "Ethernet".into()
            },
        ]
    );
    // A pick answers with the list still there, so the picker does not vanish under the press.
    lan.pick(Some("10.0.0.5")).unwrap();
    assert_eq!(lan.status().networks, status.networks);
    lan.turn_off();
    eventually(&lan, "still listed when off", |st| {
        !st.on && st.networks.is_empty()
    })
    .await;
}

/// A port nothing listens on at `address` right now.
fn free_port(address: Ipv4Addr) -> u16 {
    std::net::TcpListener::bind((address, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// An advert a test holds: in `live` until it is withdrawn (dropped).
struct Advertised(Arc<Mutex<Vec<Record>>>, Record);
impl Advertised {
    fn standing(&self) -> Option<bool> {
        Some(true)
    }
}
impl Drop for Advertised {
    fn drop(&mut self) {
        let mut live = self.0.lock().unwrap();
        if let Some(at) = live.iter().position(|record| *record == self.1) {
            live.remove(at);
        }
    }
}

/// ★ While it listens, the station is advertised by name, from its own key and where it listens,
/// on that network's adapter, and the shack says what Windows' firewall says of that network;
/// when it stops, the advert is withdrawn and neither is said. CONTROL: nothing was advertised
/// before it was turned on.
#[tokio::test]
async fn the_station_is_named_while_it_listens_and_withdrawn_when_it_stops() {
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
    };
    let port = free_port(address);
    let network = Network::new(address, 32).unwrap();
    let s = shack(network);
    let scratch = Scratch::new();
    let live: Arc<Mutex<Vec<Record>>> = Arc::default();
    let held = live.clone();
    let advertise: Advertise = Arc::new(move |record| {
        held.lock().unwrap().push(record.clone());
        let advert = Advertised(held.clone(), record.clone());
        Some(Box::new(move || advert.standing()) as Advert)
    });
    let lan = switch_with(
        &s,
        &scratch,
        Arc::new(move |_| only(Ok(network))),
        book_of(&s),
        advertise,
        Arc::new(|_| Some("public")),
    );
    assert!(live.lock().unwrap().is_empty(), "advertised while off");
    lan.turn_on(None, Some(port)).unwrap();
    let at = SocketAddr::new(address.into(), port).to_string();
    let status = eventually(&lan, "never listened, named and read", |st| {
        st.listening.as_deref() == Some(at.as_str()) && st.named.is_some() && st.firewall.is_some()
    })
    .await;
    assert_eq!(status.named, Some(true));
    assert_eq!(status.firewall, Some("public"));
    let key = status.key.expect("no key while listening");
    assert_eq!(
        *live.lock().unwrap(),
        vec![Record::new(
            &key,
            channel::PROTOCOL_VERSION,
            SocketAddrV4::new(address, port),
            9
        )
        .unwrap()]
    );
    lan.turn_off();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !live.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "still advertised once off");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let status = eventually(&lan, "still said once off", |st| !st.on).await;
    assert_eq!((status.named, status.firewall), (None, None));
}

/// ★ A picked address that goes (sleep, a DHCP renewal, a cable out) is waited for, on, and
/// listened at again when it is back, with no press at the shack. CONTROL: while it was gone the
/// port was shut.
#[tokio::test]
async fn a_picked_address_that_goes_and_comes_back_is_listened_at_again() {
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
    };
    let port = free_port(address);
    let network = Network::new(address, 32).unwrap();
    let s = shack(network);
    let scratch = Scratch::new();
    let present = Arc::new(AtomicBool::new(true));
    let here = present.clone();
    let resolve: Resolve = Arc::new(move |_| {
        if here.load(Ordering::SeqCst) {
            only(Ok(network))
        } else {
            only(Err(NoNetwork::Gone))
        }
    });
    let lan = switch_for(&s, &scratch, resolve, book_of(&s));
    lan.turn_on(Some(&address.to_string()), Some(port)).unwrap();
    let at = SocketAddr::new(address.into(), port);
    let listening = |st: &LanStatus| st.listening.as_deref() == Some(at.to_string().as_str());
    eventually(&lan, "never listened", listening).await;
    present.store(false, Ordering::SeqCst);
    let status = eventually(&lan, "still listening without its address", |st| {
        st.listening.is_none()
    })
    .await;
    assert!(status.on, "went off without its address");
    assert_eq!(status.reason, Some("addressGone"));
    closed(at).await;
    present.store(true, Ordering::SeqCst);
    eventually(&lan, "not listening again", listening).await;
    assert!(tokio::net::TcpStream::connect(at).await.is_ok());
}

/// ★ Off never waits behind Windows' firewall: turned off while the firewall is being read (here a
/// read that does not come back), after the station has looked at its network again more than
/// once, the port closes at once and the status says off. CONTROL: the port was open while the
/// read was under way.
#[tokio::test]
async fn off_closes_the_port_at_once_while_the_firewall_is_read() {
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
    };
    let port = free_port(address);
    let network = Network::new(address, 32).unwrap();
    let s = shack(network);
    let scratch = Scratch::new();
    let (reading, mut read) = tokio::sync::mpsc::unbounded_channel();
    let (answer, answered) = std::sync::mpsc::channel::<()>();
    let answered = Mutex::new(answered);
    let firewall: FirewallSays = Arc::new(move |_| {
        let _ = reading.send(());
        let _ = answered
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(30));
        Some("public")
    });
    let lan = switch_with(
        &s,
        &scratch,
        Arc::new(move |_| only(Ok(network))),
        book_of(&s),
        advertise(),
        firewall,
    );
    lan.turn_on(None, Some(port)).unwrap();
    let at = SocketAddr::new(address.into(), port);
    tokio::time::timeout(Duration::from_secs(5), read.recv())
        .await
        .expect("the firewall was never read");
    tokio::time::sleep(LOOK_AGAIN + LOOK_AGAIN / 2).await;
    assert!(
        tokio::net::TcpStream::connect(at).await.is_ok(),
        "the control: not listening while the firewall was read"
    );
    let started = Instant::now();
    lan.turn_off();
    // Closed: a connection is no longer taken. Windows retries a refused connection to this
    // computer's own address for about two seconds before it says so, so a connection not taken
    // within a moment is what is asked, on every platform.
    let taken = || {
        tokio::time::timeout(
            Duration::from_millis(200),
            tokio::net::TcpStream::connect(at),
        )
    };
    while matches!(taken().await, Ok(Ok(_))) {
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the port at {at} is still open"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "closed only after {:?}",
        started.elapsed()
    );
    assert!(!lan.status().on);
    let _ = answer.send(());
}
