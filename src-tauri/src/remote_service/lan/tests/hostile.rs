//! The hostile-network kit: each refusal the security review relies on, tried against the shack's
//! real listener on this box's own private address and, for the computer's side, the window's real
//! client, in process. Each case shows its refusal, and that it reached nothing past it
//! ([`reached_nothing`]): the operations authority opened no connection for it, control and the
//! grants did not move, no setting changed, the stream's one slot stayed free, and what was on the
//! air stayed as it was. Where a case leaves room under the gate's two connections per address
//! (every connection here comes from this box's one address), a paired computer keeps an over on the
//! air on its own connection through it ([`Controller`]), and the over must still be there after.
//!
//! Keys are made for each run and never written anywhere but the in-memory stores. Each case is
//! skipped, saying so, on a box with no private IPv4 address of its own.
use super::super::book::WRONG_PROOFS;
use super::super::pairing::{self as proofs, Side};
use super::ceremony::{guess_code, hex, nonce, open_pairing};
use super::computer::{
    page_connect, page_socket, paired_record, recorded, scripted_station, slot_free, told, Client2,
};
use super::drops::{radio_loop, Over, TICK};
use super::*;
use crate::lan_client::tests::{record, StationStore};
use crate::lan_client::{code, pairing, road, ComputerKey, Origin, Reach, Stations};
use std::net::SocketAddrV4;
use tempo_stream::lan::dnssd::{self, Found};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn v4(at: SocketAddr) -> SocketAddrV4 {
    match at {
        SocketAddr::V4(at) => at,
        SocketAddr::V6(_) => unreachable!("the shack listens on IPv4 only"),
    }
}

// ----- What a hostile try must leave as it found it -----

struct Snapshot {
    /// The id the operations authority would give the next LAN connection it opens.
    next: u64,
    control: Value,
    settings: Value,
    air: (bool, bool, bool),
}

/// The id the operations authority gives the next LAN connection it opens, taken by one opened and
/// closed at once that holds nothing. Two of them a hostile try apart show whether the try opened a
/// connection there in between.
fn probe(s: &Shack) -> u64 {
    let id = s.shared.authority.start_lan_connection();
    s.shared.authority.retire_lan_connection(id);
    id
}

/// Who holds control, and the grants, as the authority says when it is free (busy, it says none).
fn control(s: &Shack) -> Value {
    for _ in 0..200 {
        let status = s.shared.authority.local_status();
        if status.get("controlDevices").is_some() {
            return status;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the operations authority stayed busy")
}

fn settings(s: &Shack) -> Value {
    serde_json::to_value(s.shared.engine.lock().unwrap().settings()).unwrap()
}

/// What is on the air after the radio loop's own poll: the PTT, a Tune carrier, transmit armed.
fn on_air(s: &Shack) -> (bool, bool, bool) {
    let mut e = s.shared.engine.lock().unwrap();
    e.poll_remote_transmit(Instant::now());
    (e.manual_ptt(), e.tuning(), e.tx_enabled())
}

fn snapshot(s: &Shack) -> Snapshot {
    Snapshot {
        next: probe(s) + 1,
        control: control(s),
        settings: settings(s),
        air: on_air(s),
    }
}

/// ★ Each case's other half: nothing past its refusal was reached. The operations authority opened
/// no connection for it, control and the grants are as they were, no setting changed, the stream's
/// one slot is free, and what is on the air is as it was.
fn reached_nothing(s: &Shack, before: &Snapshot, what: &str) {
    assert_eq!(
        probe(s),
        before.next,
        "{what}: the operations authority opened a connection for it"
    );
    assert_eq!(
        control(s),
        before.control,
        "{what}: control or a grant moved"
    );
    assert_eq!(settings(s), before.settings, "{what}: a setting changed");
    assert!(
        s.shared.authority.claim_stream().is_some(),
        "{what}: the stream's slot was taken"
    );
    assert_eq!(on_air(s), before.air, "{what}: what was on the air changed");
}

/// The paired computer in control on its own connection, an over on the air under its session's
/// presence.
struct Controller {
    socket: Client,
    session: String,
    state: Value,
}

impl Controller {
    async fn on(s: &Shack, at: SocketAddr) -> Self {
        let (socket, session, state) = keyed_at(s, at).await;
        Self {
            socket,
            session,
            state,
        }
    }

    /// The live stream's heartbeat: the paired computer still answers and still holds control, and
    /// its presence is renewed.
    async fn renewed(&mut self, s: &Shack) {
        renewed(s, &mut self.socket, &self.session, &self.state).await;
    }
}

// ----- The cases -----

/// ★ An unpinned key. A computer holding a key the shack never paired, and pinning the shack's own,
/// is refused in the TLS handshake itself: the shack's verifier answers AccessDenied, and no
/// application data is read; the computer's real client says `notPaired`. Its address pays at the
/// gate: after five, the
/// address is ignored, and a new connection from there with the paired key is dropped before TLS too
/// (`unreachable`), a denial of service of LAN Remote's new connections and nothing else. It reached
/// nothing. CONTROL: the paired computer was welcomed from this address before, and its own
/// connection keeps control and the over through all of it.
#[tokio::test]
async fn an_unpinned_key_is_refused_in_the_handshake_and_reaches_nothing() {
    let Some(r) = listening().await else {
        return;
    };
    let at = v4(r.at);
    let mut controller = Controller::on(&r.s, r.at).await;
    controller.renewed(&r.s).await;
    let before = snapshot(&r.s);
    let mut stranger = paired_record(&r.s, at);
    stranger.pkcs8 = Computer::new().key;
    let refused = open(
        TcpStream::connect(r.at).await.unwrap(),
        &stranger.pkcs8,
        &r.s.public_key,
    )
    .await
    .expect_err("an unpinned key got through the handshake");
    assert!(refused.contains("AccessDenied"), "{refused}");
    for n in 2..=FAILURES_BEFORE_IGNORED {
        assert_eq!(
            road::connect_at(&stranger, &[at]).await.err(),
            Some("notPaired"),
            "try {n}"
        );
    }
    assert_eq!(
        road::connect_at(&paired_record(&r.s, at), &[at])
            .await
            .err(),
        Some("unreachable"),
        "a new connection from an ignored address was heard"
    );
    reached_nothing(&r.s, &before, "an unpinned key");
    controller.renewed(&r.s).await;
}

/// ★ Plain HTTP to the TLS port, as a browser on the network sends it: a page's request, and a
/// WebSocket upgrade from a web page's origin (CSRF, or DNS rebinding to the shack's address). Each
/// is answered with no HTTP at all, at most a TLS alert, and closed as it arrives rather than at the
/// handshake's deadline: nothing past the TLS record layer reads it, and it reached nothing.
/// CONTROL: the same port takes the paired computer's TLS and welcomes it; and the check sees HTTP
/// where there is some, from this computer's own window origin.
#[tokio::test]
async fn plain_http_to_the_tls_port_is_answered_with_no_http_and_reaches_nothing() {
    let Some(r) = listening().await else {
        return;
    };
    let at = v4(r.at);
    let mut controller = Controller::on(&r.s, r.at).await;
    controller.renewed(&r.s).await;
    let before = snapshot(&r.s);
    for request in [
        format!("GET / HTTP/1.1\r\nHost: {at}\r\nAccept: text/html\r\n\r\n"),
        format!(
            "GET /socket HTTP/1.1\r\nHost: {at}\r\nOrigin: http://example.com\r\nUpgrade: \
             websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n"
        ),
    ] {
        let mut tcp = TcpStream::connect(r.at).await.unwrap();
        let sent = Instant::now();
        tcp.write_all(request.as_bytes()).await.unwrap();
        let mut answer = Vec::new();
        let ended =
            tokio::time::timeout(Duration::from_secs(10), tcp.read_to_end(&mut answer)).await;
        let took = sent.elapsed();
        assert!(ended.is_ok(), "held open: {request:?}");
        assert!(
            took < channel::HANDSHAKE / 5,
            "closed only after {took:?}: {request:?}"
        );
        assert!(
            !answer.windows(4).any(|w| w == b"HTTP"),
            "answered in HTTP: {answer:02x?}"
        );
        assert!(
            answer.is_empty() || (answer[0] == 0x15 && answer.len() <= 7),
            "more than a TLS alert: {answer:02x?}"
        );
    }
    reached_nothing(&r.s, &before, "plain HTTP");
    assert!(
        road::connect_at(&paired_record(&r.s, at), &[at])
            .await
            .is_ok(),
        "the control"
    );
    controller.renewed(&r.s).await;
    let origin = Origin::start(Reach {
        assets: Arc::new(|_: &str| None),
        stations: Arc::new(Stations::new(Arc::new(StationStore::default()))),
        find: Arc::new(|_| Ok(Vec::new())),
        name: "Den PC".into(),
    })
    .unwrap();
    let mut tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, origin.port()))
        .await
        .unwrap();
    tcp.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .await
        .unwrap();
    let mut answer = vec![0; 64];
    let read = tokio::time::timeout(Duration::from_secs(5), tcp.read(&mut answer))
        .await
        .unwrap()
        .unwrap();
    assert!(
        answer[..read].starts_with(b"HTTP/1.1 "),
        "the control of the check: {:02x?}",
        &answer[..read]
    );
}

/// ★ A flood, of frames and of connections, from a machine on the network. Inside an open pairing
/// window, where TLS takes any key, a pairing connection's flood of WebSocket frames is cut at its
/// first frame that is not `pair`, and the code is not spent. Bytes that are no TLS are closed at
/// their first record. Then forty connections at once, each with a key of its own: the gate lets one
/// through at a time beside the paired computer's (two per address), counts every arrival, refused
/// ones too (ten a minute), and ignores an address for five minutes once it has failed five
/// handshakes, so at most one of the forty reaches TLS; every other is dropped as it arrives, before
/// a byte is read, and so is a new connection from there with the paired key. Then three seconds
/// more of it, sixteen connections every tick,
/// each dropped as it arrives. Through all of it the radio loop keeps its tick and the over stays on
/// the air, the paired computer's own connection keeps answering, and nothing was reached. The radio
/// loop's longest gap is printed.
#[tokio::test]
async fn a_flood_of_frames_and_connections_reaches_nothing_and_never_the_over() {
    let Some(r) = listening().await else {
        return;
    };
    let at = v4(r.at);
    let mut controller = Controller::on(&r.s, r.at).await;
    controller.renewed(&r.s).await;
    let before = snapshot(&r.s);
    let done = Arc::new(AtomicBool::new(false));
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Phone,
        done.clone(),
        Instant::now() + Duration::from_secs(30),
    );

    r.lan.pair().unwrap();
    let shown = r.lan.status().pairing.unwrap().code;
    let tcp = TcpStream::connect(r.at).await.unwrap();
    let mut p = open_pairing(tcp, &Computer::new().key)
        .await
        .expect("inside the window, TLS takes any key");
    let mut sent = 0;
    while sent < 1000 {
        let frame = if sent % 2 == 0 {
            Message::Ping(vec![0; 125].into())
        } else {
            Message::Text("x".repeat(1024).into())
        };
        if p.socket.send(frame).await.is_err() {
            break;
        }
        sent += 1;
    }
    assert!(
        next(&mut p.socket).await["closed"].is_string(),
        "the frames were taken"
    );
    let still = r.lan.status();
    assert_eq!(
        still.pairing.map(|p| p.code),
        Some(shown),
        "the flood spent the code"
    );
    assert_eq!(still.devices.len(), 1, "the flood paired a computer");
    r.lan.cancel_pairing();

    for junk in [
        // A handshake record announcing more than TLS allows.
        [&[0x16, 0x03, 0x01, 0xff, 0xff][..], &[0xaa; 4096][..]].concat(),
        // Application data before any handshake.
        [&[0x17, 0x03, 0x03, 0x40, 0x00][..], &[0x55; 16384][..]].concat(),
        // Bytes that are no record at all.
        (0..65536u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect::<Vec<u8>>(),
    ] {
        let mut tcp = TcpStream::connect(r.at).await.unwrap();
        let sent = Instant::now();
        let _ = tcp.write_all(&junk).await;
        let mut answer = Vec::new();
        let ended =
            tokio::time::timeout(Duration::from_secs(10), tcp.read_to_end(&mut answer)).await;
        assert!(ended.is_ok(), "held open: {:02x?}", &junk[..5]);
        assert!(
            sent.elapsed() < channel::HANDSHAKE / 5,
            "closed only at the deadline: {:02x?}",
            &junk[..5]
        );
        assert!(
            answer.is_empty() || answer[0] == 0x15,
            "more than a TLS alert: {answer:02x?}"
        );
    }

    controller.renewed(&r.s).await;
    let (to, public) = (r.at, r.s.public_key.clone());
    let tries = (0..40).map(|_| {
        let public = public.clone();
        async move {
            let tcp = TcpStream::connect(to)
                .await
                .map_err(|e| format!("tcp: {e}"))?;
            open(tcp, &Computer::new().key, &public).await
        }
    });
    let answered = futures_util::future::join_all(tries).await;
    assert!(
        answered.iter().all(Result::is_err),
        "a stranger was welcomed"
    );
    let reached = answered
        .iter()
        .filter(|a| a.as_ref().is_err_and(|e| e.contains("AccessDenied")))
        .count();
    assert!(
        reached <= FAILURES_BEFORE_IGNORED - 4,
        "{reached} of 40 reached TLS"
    );
    assert_eq!(
        road::connect_at(&paired_record(&r.s, at), &[at])
            .await
            .err(),
        Some("unreachable"),
        "the flooding address is still heard"
    );
    let (flooding, mut beat, mut more) = (Instant::now(), Instant::now(), 0);
    while flooding.elapsed() < Duration::from_secs(3) {
        let wave = (0..16).map(|_| async move { TcpStream::connect(to).await.is_ok() });
        more += futures_util::future::join_all(wave)
            .await
            .into_iter()
            .filter(|&made| made)
            .count();
        if beat.elapsed() >= Duration::from_secs(1) {
            controller.renewed(&r.s).await;
            beat = Instant::now();
        }
        tokio::time::sleep(TICK).await;
    }
    controller.renewed(&r.s).await;
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let worst = ticks
        .windows(2)
        .map(|w| w[1].0 - w[0].0)
        .max()
        .unwrap_or_default();
    eprintln!(
        "measured: the flood: {sent} frames sent into the pairing window before it was closed, \
         {reached} of 40 connections reached TLS, then {more} more connections in {:?}; over {} \
         radio loop ticks the longest gap was {worst:?} (its tick is {TICK:?})",
        flooding.elapsed(),
        ticks.len()
    );
    assert!(
        ticks.iter().all(|&(_, on)| on),
        "the over came off the air during the flood"
    );
    reached_nothing(&r.s, &before, "the flood");
}

/// A machine on the network answering as the station with a key of its own: how many connections
/// it took, how many computers' keys it was shown, and how many handshakes it finished.
struct Forger {
    at: SocketAddrV4,
    taken: Arc<AtomicUsize>,
    shown: Arc<AtomicUsize>,
    finished: Arc<AtomicUsize>,
}

async fn forger(on: Ipv4Addr) -> Forger {
    let identity = tls::Identity::new(&fixture_key(), STATION.into()).unwrap();
    let (taken, shown, finished) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let counted = shown.clone();
    let config = tls::server(
        &identity,
        Arc::new(move |_: &[u8; 32]| {
            counted.fetch_add(1, Ordering::SeqCst);
            None
        }),
        Arc::new(|| true),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind((on, 0)).await.unwrap();
    let at = v4(listener.local_addr().unwrap());
    let (took, done) = (taken.clone(), finished.clone());
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            took.fetch_add(1, Ordering::SeqCst);
            let accepted = tokio_rustls::TlsAcceptor::from(config.clone())
                .accept(stream)
                .await;
            if accepted.is_ok() {
                done.fetch_add(1, Ordering::SeqCst);
            }
        }
    });
    Forger {
        at,
        taken,
        shown,
        finished,
    }
}

/// ★ A forged discovery reply. Discovery supplies only an address, and the window's own client
/// connects only to the key it pinned at pairing. The station is not where this computer remembers
/// it, so the window looks for it by name, and a machine on the network answers first, with the
/// shack's own key tag (copied from the shack's advert, as anyone on the network can) and its own
/// address. The client tries it and refuses its key in the handshake before showing its own: the
/// forger's verifier never sees this computer's key, and it finishes no handshake. The client then
/// tries the shack's own advert and is welcomed there, so the page is told only of the real station.
/// A reply naming an address the shack never listens at (public, loopback, link-local, carrier NAT)
/// or a system port is not read as a station's at all. At the shack it reached nothing: the one
/// connection opened there is the honest road. CONTROL: with the forged reply alone, nothing
/// connects, and the page is told only that a station answered by name that is not the one this
/// computer paired with (`notThisStation`), never that this station's key changed.
#[tokio::test]
async fn a_forged_discovery_reply_costs_one_handshake_and_learns_nothing() {
    use ring::digest::{digest, SHA256};
    let Some(r) = listening().await else {
        return;
    };
    let at = v4(r.at);
    let mut controller = Controller::on(&r.s, r.at).await;
    let spki = tempo_stream::protocol::hex_bytes(&r.s.public_key).unwrap();
    let tag = hex(digest(&SHA256, &spki).as_ref())[..16].to_string();
    let instance = format!("Nexus 3F2A 9B1C.{}", dnssd::SERVICE);
    let txt = |a: &str| {
        vec![
            ("v".to_string(), "1".to_string()),
            ("k".to_string(), tag.clone()),
            ("a".to_string(), a.to_string()),
        ]
    };
    let own = at.ip().to_string();
    for (address, port) in [
        ("203.0.113.9", at.port()),
        ("127.0.0.1", at.port()),
        ("169.254.7.7", at.port()),
        ("100.64.0.9", at.port()),
        (own.as_str(), 80),
    ] {
        assert!(
            dnssd::found(&instance, port, &txt(address)).is_none(),
            "{address}:{port} read as a station's advert"
        );
    }
    assert!(
        dnssd::found(&instance, at.port(), &txt(&own)).is_some(),
        "the shack's own advert"
    );

    let forger = forger(*at.ip()).await;
    let dead = v4(std::net::TcpListener::bind((*at.ip(), 0))
        .unwrap()
        .local_addr()
        .unwrap());
    let advert = |address: SocketAddrV4| Found {
        name: "Nexus 3F2A 9B1C".into(),
        address,
        protocol: 1,
        key: tag.clone(),
    };
    let adverts = Arc::new(Mutex::new(vec![advert(forger.at), advert(at)]));
    let store = StationStore::default();
    let stations = Stations::new(Arc::new(store.clone()));
    let mut kept = paired_record(&r.s, at);
    kept.addresses = vec![dead.to_string()];
    stations.keep(&kept).unwrap();
    let origin = Origin::start(Reach {
        assets: Arc::new(|_: &str| None),
        stations: Arc::new(Stations::new(Arc::new(store.clone()))),
        find: {
            let adverts = adverts.clone();
            Arc::new(move |_| Ok(adverts.lock().unwrap().clone()))
        },
        name: "Den PC".into(),
    })
    .unwrap();
    let mut page = page_socket(&origin).await;
    controller.renewed(&r.s).await;
    let mut before = snapshot(&r.s);
    let connected = page_connect(&mut page, None).await;
    assert_eq!(connected["type"], "connected", "{connected}");
    assert_eq!(
        connected["address"],
        at.to_string(),
        "the page was handed the forger"
    );
    assert_eq!(
        forger.taken.load(Ordering::SeqCst),
        1,
        "the forged reply was not tried"
    );
    assert_eq!(
        forger.shown.load(Ordering::SeqCst),
        0,
        "this computer showed the forger its key"
    );
    assert_eq!(
        forger.finished.load(Ordering::SeqCst),
        0,
        "the forger finished a handshake"
    );
    // The honest road: the one connection the shack opened.
    before.next += 1;
    reached_nothing(&r.s, &before, "a forged discovery reply");

    page.send(Message::Text(
        json!({"type":"disconnect"}).to_string().into(),
    ))
    .await
    .unwrap();
    assert_eq!(
        told(&mut page).await,
        json!({"type":"closed","reason":"disconnected"})
    );
    *adverts.lock().unwrap() = vec![advert(forger.at)];
    stations.keep(&kept).unwrap();
    assert_eq!(
        page_connect(&mut page, None).await,
        json!({"type":"connectRefused","reason":"notThisStation"}),
        "the control"
    );
    assert_eq!(forger.taken.load(Ordering::SeqCst), 2);
    assert_eq!(forger.shown.load(Ordering::SeqCst), 0, "the control");
    controller.renewed(&r.s).await;
}

/// A port on `on` that hands its first connection to `first` and every later one to `then`, byte
/// for byte: a machine taking a station's address over between two connections.
async fn switching(on: Ipv4Addr, first: SocketAddr, then: SocketAddr) -> SocketAddrV4 {
    let listener = tokio::net::TcpListener::bind((on, 0)).await.unwrap();
    let at = v4(listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut to = first;
        while let Ok((mut inbound, _)) = listener.accept().await {
            let target = std::mem::replace(&mut to, then);
            tokio::spawn(async move {
                if let Ok(mut outbound) = TcpStream::connect(target).await {
                    let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                }
            });
        }
    });
    at
}

/// ★ Pairing again with a station this computer knows shows this computer's own key for that
/// station to it alone (the operator's ruling of 2026-10-04, "Reuse the PC's own key"): the second
/// connection pins the key the station presented on the first, so a machine answering in its
/// place between the two (here the forger, at the same address) is refused in the handshake
/// before it is shown this computer's key, and nothing is paired; the code is not spent, and it
/// reached nothing. CONTROL: the first connection reached the station and its proof held, since
/// the pairing went on to a second connection, which the forger took.
#[tokio::test]
async fn pairing_again_shows_this_computers_key_to_its_station_alone() {
    let Some(r) = listening().await else {
        return;
    };
    let on = *v4(r.at).ip();
    let forger = forger(on).await;
    let at = switching(on, r.at, SocketAddr::V4(forger.at)).await;
    let before = snapshot(&r.s);
    r.lan.pair().unwrap();
    let shown = code(&r.lan.status().pairing.unwrap().code).unwrap();
    let known = paired_record(&r.s, v4(r.at));
    let paired = pairing::pair(at, shown, "Den PC", &[known]).await;
    assert_eq!(
        forger.shown.load(Ordering::SeqCst),
        0,
        "this computer showed the forger its key for the station"
    );
    assert_eq!(paired.err(), Some("notStation"));
    assert_eq!(forger.taken.load(Ordering::SeqCst), 1, "the control");
    assert!(r.lan.status().pairing.is_some(), "the code was spent");
    reached_nothing(&r.s, &before, "pairing again through a forger");
}

/// A machine in the middle holding `key`: it takes a computer's connection as if it were the
/// station, with a pairing window of its own open (so it takes any computer's key), opens its own to
/// the station at `to` with its key, and passes every message along both ways. How many keys
/// computers showed it, and every message it passed on from the computer.
async fn middle(
    key: String,
    on: Ipv4Addr,
    to: SocketAddr,
) -> (
    SocketAddrV4,
    Arc<AtomicUsize>,
    tokio::sync::mpsc::UnboundedReceiver<Value>,
) {
    let identity = tls::Identity::new(&key, STATION.into()).unwrap();
    let shown = Arc::new(AtomicUsize::new(0));
    let counted = shown.clone();
    let as_station = tls::server(
        &identity,
        Arc::new(move |_: &[u8; 32]| {
            counted.fetch_add(1, Ordering::SeqCst);
            None
        }),
        Arc::new(|| true),
    )
    .unwrap();
    let as_computer = tls::client::pairing(&key).unwrap();
    let listener = tokio::net::TcpListener::bind((on, 0)).await.unwrap();
    let at = v4(listener.local_addr().unwrap());
    let (told, heard) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (as_station, as_computer, told) =
                (as_station.clone(), as_computer.clone(), told.clone());
            tokio::spawn(async move {
                let Ok(tls) = tokio_rustls::TlsAcceptor::from(as_station)
                    .accept(stream)
                    .await
                else {
                    return;
                };
                let Ok(computer) = tokio_tungstenite::accept_async(tls).await else {
                    return;
                };
                let Ok(tcp) = TcpStream::connect(to).await else {
                    return;
                };
                let name = rustls::pki_types::ServerName::try_from("nexus-station").unwrap();
                let Ok(tls) = tokio_rustls::TlsConnector::from(as_computer)
                    .connect(name, tcp)
                    .await
                else {
                    return;
                };
                let Ok((station, _)) =
                    tokio_tungstenite::client_async("ws://nexus-station/", tls).await
                else {
                    return;
                };
                let (mut to_computer, mut from_computer) = computer.split();
                let (mut to_station, mut from_station) = station.split();
                let up = async {
                    while let Some(Ok(Message::Text(text))) = from_computer.next().await {
                        let _ = told.send(serde_json::from_str::<Value>(&text).unwrap_or_default());
                        if to_station.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                };
                let down = async {
                    while let Some(Ok(Message::Text(text))) = from_station.next().await {
                        if to_computer.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                };
                tokio::select! {
                    _ = up => {}
                    _ = down => {}
                }
            });
        }
    });
    (at, shown, heard)
}

/// ★ A man in the middle with his own key. Toward the computer it cannot be the shack: the
/// computer's real client pins the shack's key and refuses the middle's in the handshake, before
/// showing its own (`keyChanged`; the middle never sees this computer's key). Toward the shack it
/// cannot be the computer: its key is not paired, and the shack refuses it (AccessDenied). Inside an
/// open pairing window, where each end takes an unknown key, it finishes both handshakes and passes
/// every word along, and still fails: the shack's proof is bound to its own session and the keys in
/// it, so the computer's real pairing client finds it does not hold (`stationProofFailed`) and
/// leaves without sending its own. Nothing is paired at the shack, the code is not spent, and it
/// reached nothing. CONTROL: the same computer pairs directly with the same code.
#[tokio::test]
async fn a_man_in_the_middle_with_his_own_key_is_refused_at_both_ends() {
    let Some(r) = listening().await else {
        return;
    };
    let at = v4(r.at);
    let before = snapshot(&r.s);
    let key = fixture_key();
    let (middle_at, shown, mut heard) = middle(key.clone(), *at.ip(), r.at).await;
    assert_eq!(
        road::connect_at(&paired_record(&r.s, at), &[middle_at])
            .await
            .err(),
        Some("keyChanged")
    );
    assert_eq!(
        shown.load(Ordering::SeqCst),
        0,
        "this computer showed the middle its key"
    );
    let refused = open(
        TcpStream::connect(r.at).await.unwrap(),
        &key,
        &r.s.public_key,
    )
    .await
    .expect_err("the shack took the middle's key");
    assert!(refused.contains("AccessDenied"), "{refused}");

    r.lan.pair().unwrap();
    let shown_code = r.lan.status().pairing.unwrap().code;
    let typed = code(&shown_code).unwrap();
    assert_eq!(
        pairing::pair(middle_at, typed, "Den PC", &[]).await.err(),
        Some("stationProofFailed")
    );
    let mut passed = Vec::new();
    while let Ok(word) = heard.try_recv() {
        passed.push(word["type"].clone());
    }
    assert_eq!(
        passed,
        [json!("pair")],
        "this computer said more than its request"
    );
    let status = r.lan.status();
    assert_eq!(
        status.devices.len(),
        1,
        "a computer was paired through the middle"
    );
    assert_eq!(
        status.pairing.map(|p| p.code),
        Some(shown_code),
        "the middle spent the code"
    );
    reached_nothing(&r.s, &before, "a man in the middle");
    let kept = pairing::pair(at, typed, "Den PC", &[])
        .await
        .expect("the control: the same code, direct");
    assert_eq!(kept.station_key, r.s.public_key);
}

/// A machine on the path that passes one connection along untouched and keeps a copy of every byte
/// the computer sent; `done` once both directions have ended.
async fn tap(
    on: Ipv4Addr,
    to: SocketAddr,
) -> (
    SocketAddrV4,
    Arc<Mutex<Vec<u8>>>,
    tokio::sync::oneshot::Receiver<()>,
) {
    let listener = tokio::net::TcpListener::bind((on, 0)).await.unwrap();
    let at = v4(listener.local_addr().unwrap());
    let kept = Arc::new(Mutex::new(Vec::new()));
    let (finished, done) = tokio::sync::oneshot::channel();
    let keeping = kept.clone();
    tokio::spawn(async move {
        let Ok((computer, _)) = listener.accept().await else {
            return;
        };
        let Ok(shack) = TcpStream::connect(to).await else {
            return;
        };
        let (mut from_computer, mut to_computer) = computer.into_split();
        let (mut from_shack, mut to_shack) = shack.into_split();
        let up = async {
            let mut buffer = vec![0; 16 * 1024];
            loop {
                match from_computer.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        keeping.lock().unwrap().extend_from_slice(&buffer[..n]);
                        if to_shack.write_all(&buffer[..n]).await.is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = to_shack.shutdown().await;
        };
        let down = async {
            let _ = tokio::io::copy(&mut from_shack, &mut to_computer).await;
            let _ = to_computer.shutdown().await;
        };
        tokio::join!(up, down);
        let _ = finished.send(());
    });
    (at, kept, done)
}

/// ★ Replayed pairing proofs. On the wire: everything a computer sent while it paired, recorded byte
/// for byte by a machine on the path and played back to the shack inside a new pairing window, is
/// refused in the TLS handshake (TLS 1.3 with no tickets and no early data: the shack's fresh
/// randoms leave the recorded flight undecryptable), so it pairs nothing and spends no code. In a
/// session of its own: a computer's proof for its session, sent in another one by a machine with a
/// key of its own and the same nonce, is a wrong code (the proof is bound to the session's exporter,
/// both keys and both nonces). Once the code is used, it pairs nobody: a further try is refused in
/// the handshake. It reached nothing. CONTROL: the recorded pairing paired, and the proof that was
/// replayed pairs the computer it was made for, in its own session.
#[tokio::test]
async fn replayed_pairing_proofs_pair_nobody() {
    let Some(r) = listening().await else {
        return;
    };
    let at = v4(r.at);
    r.lan.pair().unwrap();
    let typed = code(&r.lan.status().pairing.unwrap().code).unwrap();
    let (tap_at, kept, done) = tap(*at.ip(), r.at).await;
    pairing::pair(tap_at, typed, "Den PC", &[])
        .await
        .expect("the control: the recorded pairing pairs");
    tokio::time::timeout(Duration::from_secs(5), done)
        .await
        .expect("the recorded connection stayed open")
        .unwrap();
    let recorded = kept.lock().unwrap().clone();
    assert_eq!(r.lan.status().devices.len(), 2);
    let before = snapshot(&r.s);

    r.lan.pair().unwrap();
    let shown = r.lan.status().pairing.unwrap().code;
    let mut tcp = TcpStream::connect(r.at).await.unwrap();
    let _ = tcp.write_all(&recorded).await;
    let mut answer = Vec::new();
    let ended = tokio::time::timeout(Duration::from_secs(10), tcp.read_to_end(&mut answer)).await;
    assert!(ended.is_ok(), "the replay was held open");
    let status = r.lan.status();
    assert_eq!(status.devices.len(), 2, "the replay paired a computer");
    assert_eq!(
        status.pairing.map(|p| p.code),
        Some(shown.clone()),
        "the replay spent the code"
    );

    let typed = code(&shown).unwrap();
    let honest = Computer::new();
    let mut a = open_pairing(TcpStream::connect(r.at).await.unwrap(), &honest.key)
        .await
        .unwrap();
    let ours = nonce();
    let asked = json!({"type":"pair","protocol":VERSIONS.0,"stream":VERSIONS.1,
        "operation":VERSIONS.2,"name":"Den PC","nonce":hex(&ours)});
    send(&mut a.socket, asked.clone()).await;
    let theirs = next(&mut a.socket).await;
    assert_eq!(theirs["type"], "pairProof", "{theirs}");
    let shack_nonce = proofs::hex32(theirs["nonce"].as_str().unwrap()).unwrap();
    let t = proofs::transcript(&a.shack, &honest.spki, &ours, &shack_nonce, &a.exporter);
    let k = proofs::code_key(&typed).unwrap();
    let proved = json!({"type":"pairProof","proof":hex(&proofs::proof(&k, Side::Computer, &t))});
    let replayer = Computer::new();
    let mut b = open_pairing(TcpStream::connect(r.at).await.unwrap(), &replayer.key)
        .await
        .unwrap();
    send(&mut b.socket, asked).await;
    assert_eq!(next(&mut b.socket).await["type"], "pairProof");
    send(&mut b.socket, proved.clone()).await;
    assert_eq!(
        next(&mut b.socket).await,
        json!({"type":"refused","reason":"wrongCode"}),
        "a proof made for another session"
    );
    assert!(next(&mut b.socket).await["closed"].is_string());
    assert_eq!(
        r.lan.status().pairing.map(|p| p.code),
        Some(shown),
        "the window"
    );
    reached_nothing(&r.s, &before, "replayed pairing proofs");

    send(&mut a.socket, proved).await;
    assert_eq!(
        next(&mut a.socket).await["type"],
        "paired",
        "the control: the proof in its own session"
    );
    drop((a, b));
    let again = open_pairing(TcpStream::connect(r.at).await.unwrap(), &replayer.key)
        .await
        .err()
        .expect("a used code's window took an unknown key");
    assert!(again.contains("AccessDenied"), "{again}");
}

/// ★ Three wrong codes. Inside an open pairing window, a machine that sends a proof made with a
/// guessed code, whatever the shack's own proof said, is refused `wrongCode` each time; after the
/// third the window is closed, and the right code from the computer's real pairing client is then
/// refused in the handshake (`pairingClosed`): the code is spent, nobody is paired, and it reached
/// nothing. CONTROL: after one and after two, the window was still open, with the same code.
#[tokio::test]
async fn three_wrong_codes_close_the_window_and_reach_nothing() {
    let Some(r) = listening().await else {
        return;
    };
    let at = v4(r.at);
    let before = snapshot(&r.s);
    r.lan.pair().unwrap();
    let shown = r.lan.status().pairing.unwrap().code;
    let right = code(&shown).unwrap();
    let mut wrong = right;
    wrong[7] ^= 1;
    for n in 1..=WRONG_PROOFS {
        let guesser = Computer::new();
        let mut p = open_pairing(TcpStream::connect(r.at).await.unwrap(), &guesser.key)
            .await
            .expect("inside the window, TLS takes any key");
        assert_eq!(
            guess_code(&mut p, &guesser, &wrong).await,
            json!({"type":"refused","reason":"wrongCode"}),
            "guess {n}"
        );
        assert!(
            next(&mut p.socket).await["closed"].is_string(),
            "left open after guess {n}"
        );
        let open = r.lan.status().pairing.map(|p| p.code);
        if n < WRONG_PROOFS {
            assert_eq!(open, Some(shown.clone()), "the control: closed after {n}");
        } else {
            assert_eq!(open, None, "still open after {n}");
        }
    }
    assert_eq!(
        pairing::pair(at, right, "Den PC", &[]).await.err(),
        Some("pairingClosed"),
        "the right code, after three wrong ones"
    );
    assert_eq!(r.lan.status().devices.len(), 1);
    reached_nothing(&r.s, &before, "three wrong codes");
}

/// ★ An offer signed by another key, on the paired computer's own connection: unsigned, signed by a
/// key that is not the one its connection proved, carrying another key and that key's signature, or
/// signed for another session. Each is refused at admission (`deviceKeyMismatch`), before a stream
/// exists: the stream's slot is free, and no socket was opened at the stream's address and port (one
/// binds there now). The over stays on the air, and it reached nothing. CONTROL: the offer the
/// computer signs for this session passes every check the station makes, to the platform's own
/// answer (this box captures no window: `streamUnavailable`).
#[tokio::test]
async fn an_offer_signed_by_another_key_is_refused_at_admission_and_reaches_nothing() {
    let Some(r) = listening().await else {
        return;
    };
    let mut controller = Controller::on(&r.s, r.at).await;
    controller.renewed(&r.s).await;
    let before = snapshot(&r.s);
    let (offer, _) = recorded();
    let mine = ComputerKey::restore(&r.s.computer.key).unwrap();
    let other = ComputerKey::generate().unwrap().0;
    let session = controller.session.clone();
    let signed = |by: &ComputerKey, session: &str| {
        by.sign_offer(&offer, STATION, &r.s.device, session)
            .unwrap()
    };
    for (what, payload) in [
        ("unsigned", json!({"kind":"offer","sdp":offer})),
        (
            "signed by another key",
            json!({"kind":"offer","sdp":offer,"publicKey":mine.public_key(),
                "signature":signed(&other, &session)}),
        ),
        (
            "another key and its signature",
            json!({"kind":"offer","sdp":offer,"publicKey":other.public_key(),
                "signature":signed(&other, &session)}),
        ),
        (
            "signed for another session",
            json!({"kind":"offer","sdp":offer,"publicKey":mine.public_key(),
                "signature":signed(&mine, &id())}),
        ),
    ] {
        let lease = controller.state["leaseId"].clone();
        send(
            &mut controller.socket,
            json!({"type":"streamSignal","leaseId":lease,"payload":payload}),
        )
        .await;
        let answered = next(&mut controller.socket).await;
        assert_eq!(
            answered["reason"], "deviceKeyMismatch",
            "{what}: {answered}"
        );
        slot_free(&r.s).await;
        assert!(
            std::net::UdpSocket::bind(r.at).is_ok(),
            "{what}: a stream socket was opened"
        );
    }
    reached_nothing(&r.s, &before, "offers signed by another key");
    let lease = controller.state["leaseId"].clone();
    send(
        &mut controller.socket,
        json!({"type":"streamSignal","leaseId":lease,"payload":{"kind":"offer","sdp":offer,
            "publicKey":mine.public_key(),"signature":signed(&mine, &session)}}),
    )
    .await;
    let answered = next(&mut controller.socket).await;
    assert_eq!(
        answered["reason"], "streamUnavailable",
        "the control: {answered}"
    );
}

/// Nothing more reaches the page for a moment, but the station's status line.
async fn page_quiet(page: &mut Client2) -> bool {
    loop {
        match tokio::time::timeout(Duration::from_millis(300), page.next()).await {
            Err(_) => return true,
            Ok(Some(Ok(Message::Text(text))))
                if serde_json::from_str::<Value>(&text).is_ok_and(|v| v["type"] == "status") => {}
            Ok(_) => return false,
        }
    }
}

/// ★ An answer signed by another key, from a station that holds the pinned key (so the road is up):
/// the computer's real client checks the answer against the key it pinned before the page sees it.
/// Through the window's own page socket, the page is told only that the answer did not hold
/// (`answerRefused`, `stationKeyMismatch`): never the answer, nor the candidate that came before it,
/// so its WebRTC has nothing to connect with, and the stream is ended at the station (`close`, under
/// the same lease). CONTROL: the station's own signature reaches the page, then the candidate.
#[tokio::test]
async fn an_answer_signed_by_another_key_never_reaches_the_page() {
    let Some(private) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own");
        return;
    };
    for honest in [true, false] {
        let key = fixture_key();
        let identity = tls::Identity::new(&key, STATION.into()).unwrap();
        let session = "50000000-0000-4000-8000-000000000002";
        let lease = "30000000-0000-4000-8000-000000000003";
        let (offer, answer) = recorded();
        let mut kept = record(STATION, &[]);
        kept.station_key = identity.public_key().to_string();
        let signer = if honest {
            identity.signer.clone()
        } else {
            tls::Identity::new(&fixture_key(), STATION.into())
                .unwrap()
                .signer
        };
        let signed = signer
            .sign_answer(&offer, &answer, STATION, &kept.device_id, session)
            .unwrap();
        let welcome = json!({"type":"welcome","sessionId":session,"deviceId":kept.device_id,
            "stationId":STATION});
        let candidate = json!({"type":"streamSignal","payload":{"kind":"candidate",
            "candidate":"candidate:1 1 udp 2130706431 192.168.1.20 42075 typ host","sdpMid":"0"}});
        let answered = json!({"type":"streamSignal","payload":{"kind":"answer","sdp":signed}});
        let (at, mut there) = scripted_station(
            private,
            &key,
            vec![vec![welcome], vec![candidate, answered]],
        )
        .await;
        kept.addresses = vec![at.to_string()];
        let store = StationStore::default();
        Stations::new(Arc::new(store.clone())).keep(&kept).unwrap();
        let origin = Origin::start(Reach {
            assets: Arc::new(|_: &str| None),
            stations: Arc::new(Stations::new(Arc::new(store.clone()))),
            find: Arc::new(|_| Ok(Vec::new())),
            name: "Den PC".into(),
        })
        .unwrap();
        let mut page = page_socket(&origin).await;
        let connected = page_connect(&mut page, None).await;
        assert_eq!(connected["type"], "connected", "{connected}");
        let offered = json!({"type":"streamSignal","leaseId":lease,
            "payload":{"kind":"offer","sdp":offer}});
        page.send(Message::Text(offered.to_string().into()))
            .await
            .unwrap();
        assert_eq!(there.recv().await.unwrap()["type"], "hello");
        assert_eq!(there.recv().await.unwrap()["payload"]["kind"], "offer");
        let first = told(&mut page).await;
        if honest {
            assert_eq!(first["payload"]["kind"], "answer", "the control: {first}");
            let then = told(&mut page).await;
            assert_eq!(then["payload"]["kind"], "candidate", "the control: {then}");
        } else {
            assert_eq!(
                first,
                json!({"type":"answerRefused","reason":"stationKeyMismatch"})
            );
            assert_eq!(
                there.recv().await.unwrap(),
                json!({"type":"streamSignal","leaseId":lease,"payload":{"kind":"close"}})
            );
            assert!(
                page_quiet(&mut page).await,
                "more of that negotiation reached the page"
            );
        }
    }
}
