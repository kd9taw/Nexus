//! The computer's side of Remote over this network (`crate::lan_client`) against the shack's own
//! pieces: the production pairing client, the pinned road, the offer it signs and the answer it
//! checks, the versions (as ruled on 2026-10-04, "Refuse, say which"), control taken back by the
//! hosted road and acquired again, and one run end to end over the shack's real port, from the
//! window's page socket to a Stop. Keys are made for each run and never written anywhere but the
//! in-memory stores.
use super::*;
use crate::lan_client::road::{self, check_answer, Ids, ToPage};
use crate::lan_client::tests::{record, StationStore};
use crate::lan_client::{code, pairing, ComputerKey, Origin, Reach, Stations};
use crate::remote_service::vault::PairedStation;
use std::net::SocketAddrV4;
use tempo_stream::protocol::BrowserSignal;

/// The shack's `connection` behind a port on this box's loopback address: each connection
/// accepted there is handed to it as if it came from `PEER`, on the station's own subnet.
async fn behind_a_port(s: &Shack, stop: watch::Receiver<bool>) -> SocketAddrV4 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let SocketAddr::V4(at) = listener.local_addr().unwrap() else {
        unreachable!()
    };
    let shared = s.shared.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(channel::connection(
                stream,
                PEER.parse().unwrap(),
                shared.clone(),
                stop.clone(),
            ));
        }
    });
    at
}

/// This computer's record for `s`'s station, as pairing kept it: the paired computer's key, the
/// station's key pinned, the two ids, and `at`.
pub(super) fn paired_record(s: &Shack, at: SocketAddrV4) -> PairedStation {
    PairedStation {
        station_id: STATION.into(),
        device_id: s.device.clone(),
        station_key: s.public_key.clone(),
        pkcs8: s.computer.key.clone(),
        addresses: vec![at.to_string()],
    }
}

/// Another station's key: SPKI as lowercase hex.
fn another_key() -> String {
    tls::Identity::new(&fixture_key(), STATION.into())
        .unwrap()
        .public_key()
        .to_string()
}

/// LAN on at `s`'s station with no socket of its own: its grants and presses only, the paired
/// computers holding station control as they do while LAN is on.
fn lan_on(s: &Shack, scratch: &Scratch) -> Lan {
    let lan = switch_for(
        s,
        scratch,
        Arc::new(|_| only(Err(NoNetwork::Choose))),
        s.shared.desk.book.clone(),
    );
    lan.turn_on(None, None).unwrap();
    lan
}

/// The station's next word on the road other than its status line.
async fn heard(road: &mut road::Road) -> Value {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), road.next()).await {
            Ok(Some(ToPage::Station(text))) => {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] != "status" {
                    return value;
                }
            }
            Ok(Some(ToPage::AnswerRefused(reason))) => return json!({"answerRefused": reason}),
            Ok(None) => return json!({"closed": true}),
            Err(_) => return json!({"silent": true}),
        }
    }
}

/// One operation request on the road and the station's answer, asked again while the station is
/// merely busy, as a page does.
async fn ask_road(road: &mut road::Road, request: Value) -> Value {
    for _ in 0..50 {
        let mut request = request.clone();
        if request["type"] != "stopTransmit" {
            request["requestId"] = json!(id());
        }
        if let Some(answered) = road.operation(Some(4), &request).await.unwrap() {
            return serde_json::from_str(&answered).unwrap();
        }
        let answer = heard(road).await;
        if answer["error"] != "stationBusy" {
            return answer;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the station stayed busy")
}

/// The state, then an acquire: the lease's state, or the refusal.
async fn acquire(road: &mut road::Road) -> Value {
    let state = ask_road(road, json!({"type":"state"})).await;
    let boot = state["value"]["stationBootId"].clone();
    ask_road(road, json!({"type":"acquire","stationBootId":boot})).await
}

/// The contract's recorded offer and the answer str0m wrote for it.
pub(super) fn recorded() -> (String, String) {
    let file: Value = serde_json::from_str(include_str!(
        "../../../../../remote/test/fixtures/stream/station-answer.json"
    ))
    .unwrap();
    (
        file["offer"].as_str().unwrap().into(),
        file["answer"].as_str().unwrap().into(),
    )
}

// ----- The pinned road -----

/// ★ The pinned road refuses a station presenting any key but the one pinned while pairing, and it
/// does so before this computer sends its own: the station is never shown this computer's key
/// ("this station's key has changed"). CONTROL: pinned to the right key, the same computer is
/// welcomed under its device id, and the station was shown its key.
#[tokio::test]
async fn the_pinned_road_refuses_another_key_before_showing_its_own() {
    let mut s = shack(home());
    let shown = Arc::new(AtomicUsize::new(0));
    let (paired, pairing) = s.shared.desk.book.verifier();
    let counted = shown.clone();
    let counting: tls::Paired = Arc::new(move |pin: &[u8; 32]| {
        counted.fetch_add(1, Ordering::SeqCst);
        paired(pin)
    });
    let identity = tls::Identity::new(&s.station_key, STATION.into()).unwrap();
    s.shared.tls = tls::server(&identity, counting, pairing).unwrap();
    let (_stop, stop) = watch::channel(false);
    let at = behind_a_port(&s, stop).await;
    let mut pinned_elsewhere = paired_record(&s, at);
    pinned_elsewhere.station_key = another_key();
    assert_eq!(
        road::connect(&pinned_elsewhere, Some(at)).await.err(),
        Some("keyChanged")
    );
    assert_eq!(
        shown.load(Ordering::SeqCst),
        0,
        "the station was shown this computer's key"
    );
    let (opened, reached) = road::connect(&paired_record(&s, at), Some(at))
        .await
        .expect("the control: pinned to the station's own key");
    assert_eq!(reached, at);
    assert_eq!(opened.ids.device, s.device);
    assert_eq!(opened.ids.station, STATION);
    assert!(tempo_stream::protocol::identifier(&opened.ids.session));
    assert!(
        shown.load(Ordering::SeqCst) > 0,
        "the control's key was never shown"
    );
}

/// ★ A station that no longer has this computer's key paired refuses it in the handshake: removed
/// at the shack, the computer is told `notPaired` and never welcomed. CONTROL: before the
/// removal, the same record was welcomed.
#[tokio::test]
async fn a_computer_removed_at_the_shack_is_told_it_is_not_paired() {
    let s = shack(home());
    let scratch = Scratch::new();
    let lan = lan_on(&s, &scratch);
    let (_stop, stop) = watch::channel(false);
    let at = behind_a_port(&s, stop).await;
    let record = paired_record(&s, at);
    let (opened, _) = road::connect(&record, Some(at)).await.expect("the control");
    opened.close().await;
    lan.revoke(&s.device).unwrap();
    assert_eq!(
        road::connect(&record, Some(at)).await.err(),
        Some("notPaired")
    );
    // A key the station never paired is told the same.
    let mut stranger = paired_record(&s, at);
    stranger.pkcs8 = fixture_key();
    assert_eq!(
        road::connect(&stranger, Some(at)).await.err(),
        Some("notPaired")
    );
}

/// The shack's `connection` behind a port on `ip`, as [`behind_a_port`].
async fn behind_a_port_on(s: &Shack, ip: Ipv4Addr, stop: watch::Receiver<bool>) -> SocketAddrV4 {
    let listener = tokio::net::TcpListener::bind((ip, 0)).await.unwrap();
    let SocketAddr::V4(at) = listener.local_addr().unwrap() else {
        unreachable!()
    };
    let shared = s.shared.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(channel::connection(
                stream,
                PEER.parse().unwrap(),
                shared.clone(),
                stop.clone(),
            ));
        }
    });
    at
}

/// ★ Remembered addresses, on this box's own private address (a station's are private, and only a
/// private one is remembered): the one the operator typed is tried first, then the remembered ones
/// in their order, the last that worked first, and the first to welcome this computer is the
/// road. CONTROL: with only addresses where nothing listens, the answer is that nothing listens
/// there (`refused`); and an
/// address that told more (another key) is what the page is told over one that told nothing.
/// Skipped, saying so, on a box with no private address of its own.
#[tokio::test]
async fn the_remembered_addresses_are_tried_in_order() {
    let Some(private) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own");
        return;
    };
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    let live = behind_a_port_on(&s, private, stop).await;
    let dead = {
        let probe = std::net::TcpListener::bind((private, 0)).unwrap();
        let SocketAddr::V4(at) = probe.local_addr().unwrap() else {
            unreachable!()
        };
        at
    };
    // A second port of the same station: either welcomes this computer, so only the order decides.
    let (_stop_too, stop_too) = watch::channel(false);
    let also = behind_a_port_on(&s, private, stop_too).await;
    let mut kept = paired_record(&s, live);
    kept.addresses = vec![dead.to_string(), live.to_string(), also.to_string()];
    let (_, reached) = road::connect(&kept, None).await.unwrap();
    assert_eq!(
        reached, live,
        "the remembered order, past an address that is gone"
    );
    kept.addresses = vec![also.to_string(), live.to_string()];
    let (_, reached) = road::connect(&kept, None).await.unwrap();
    assert_eq!(
        reached, also,
        "the remembered order, the last that worked first"
    );
    let (_, reached) = road::connect(&kept, Some(live)).await.unwrap();
    assert_eq!(
        reached, live,
        "the typed address before the remembered ones"
    );
    kept.addresses = vec![dead.to_string()];
    assert_eq!(
        road::connect(&kept, None).await.err(),
        Some("refused"),
        "the control"
    );
    let mut pinned_elsewhere = paired_record(&s, live);
    pinned_elsewhere.station_key = another_key();
    pinned_elsewhere.addresses = vec![dead.to_string(), live.to_string()];
    assert_eq!(
        road::connect(&pinned_elsewhere, None).await.err(),
        Some("keyChanged")
    );
}

/// ★ Nothing answering is said as the window says it (`tempo_stream::lan::unreached`): a port where
/// nothing listens is `refused`, an address where nothing answers in time is `noAnswer`, and a
/// connection made and then dropped before the handshake was done (as the shack's gate drops a
/// source it will not hear) is `unreachable`, on the road as on a pairing. Across the addresses a
/// road tries, the most telling is told whatever their order: `refused`, then `otherNetwork`, then
/// `noAnswer`, then `unreachable`, which is also what nowhere to try says. An address on none of this
/// computer's networks is `otherNetwork` where that can be read (Windows) and `noAnswer` elsewhere.
/// CONTROL: past them, the station's own port welcomes this computer. Skipped, saying so, on a box
/// with no private address of its own.
#[tokio::test]
async fn nothing_answering_is_said_as_the_window_says_it() {
    let Some(private) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own");
        return;
    };
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    let live = behind_a_port_on(&s, private, stop).await;
    let dead = {
        let probe = std::net::TcpListener::bind((private, 0)).unwrap();
        let SocketAddr::V4(at) = probe.local_addr().unwrap() else {
            unreachable!()
        };
        at
    };
    let dropping = {
        let listener = tokio::net::TcpListener::bind((private, 0)).await.unwrap();
        let SocketAddr::V4(at) = listener.local_addr().unwrap() else {
            unreachable!()
        };
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        });
        at
    };
    // A documentation address (RFC 5737): nothing answers there, and it is on none of this
    // computer's networks, which only Windows can tell.
    let silent = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 42075);
    let silent_says = if cfg!(windows) {
        "otherNetwork"
    } else {
        "noAnswer"
    };
    let kept = paired_record(&s, live);
    let (at_dead, at_silent, at_dropping) = ([dead], [silent], [dropping]);
    let (refused, timed, dropped) = tokio::join!(
        road::connect_at(&kept, &at_dead),
        road::connect_at(&kept, &at_silent),
        road::connect_at(&kept, &at_dropping),
    );
    assert_eq!(refused.err(), Some("refused"));
    assert_eq!(timed.err(), Some(silent_says));
    assert_eq!(dropped.err(), Some("unreachable"));
    assert_eq!(
        road::connect_at(&kept, &[]).await.err(),
        Some("unreachable"),
        "nowhere to try"
    );
    let (tried_last, tried_first, no_refusal) = (
        [dropping, silent, dead],
        [dead, silent, dropping],
        [dropping, silent],
    );
    let (last, first, between) = tokio::join!(
        road::connect_at(&kept, &tried_last),
        road::connect_at(&kept, &tried_first),
        road::connect_at(&kept, &no_refusal),
    );
    assert_eq!(last.err(), Some("refused"), "the most telling, tried last");
    assert_eq!(
        first.err(),
        Some("refused"),
        "the most telling, tried first"
    );
    assert_eq!(
        between.err(),
        Some(silent_says),
        "nothing answering says more than a connection dropped"
    );
    let typed = [0x5e; 8];
    assert_eq!(
        pairing::pair(dead, typed, "Den PC", &[]).await.err(),
        Some("refused")
    );
    assert_eq!(
        pairing::pair(dropping, typed, "Den PC", &[]).await.err(),
        Some("unreachable")
    );
    let (_, reached) = road::connect_at(&kept, &[dead, dropping, live])
        .await
        .expect("the control");
    assert_eq!(reached, live);
}

// ----- Pairing, the computer's half -----

fn spaced_capitals(code: &str) -> String {
    code.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// ★ The production client pairs with the code the shack shows, typed in capitals with spaces: it
/// checks the shack's proof first, the shack checks its proof, and the record it keeps holds the
/// key the shack presented, the device id the shack gave it and the LAN station id. That record
/// opens a pinned road at once. CONTROL: before the window opens, the same pairing is refused in
/// the handshake (`pairingClosed`) and nothing is paired.
#[tokio::test]
async fn the_computer_pairs_with_the_code_the_shack_shows() {
    let s = shack(home());
    let scratch = Scratch::new();
    let lan = lan_on(&s, &scratch);
    let (_stop, stop) = watch::channel(false);
    let at = behind_a_port(&s, stop).await;
    assert_eq!(
        pairing::pair(at, [7; 8], "Den PC", &[]).await.err(),
        Some("pairingClosed"),
        "the control"
    );
    assert_eq!(lan.status().devices.len(), 1);
    lan.pair().unwrap();
    let shown = lan.status().pairing.unwrap().code;
    let typed = code(&spaced_capitals(&shown)).unwrap();
    let kept = pairing::pair(at, typed, "Den PC", &[]).await.unwrap();
    assert_eq!(
        kept.station_key, s.public_key,
        "not the key the shack presented"
    );
    assert_eq!(kept.station_id, STATION);
    assert_eq!(kept.addresses, [at.to_string()]);
    let paired = lan.status().devices;
    assert!(
        paired
            .iter()
            .any(|d| d.id == kept.device_id && d.name == "Den PC"),
        "{paired:?}"
    );
    assert!(
        lan.status().pairing.is_none(),
        "the window outlived its pairing"
    );
    let (opened, _) = road::connect(&kept, Some(at)).await.unwrap();
    assert_eq!(opened.ids.device, kept.device_id);
}

/// A computer that sends its proof whatever the shack's proof said, with `guess` as the code.
async fn guess(at: SocketAddrV4, guess: &[u8; 8]) -> Value {
    let (key, pkcs8) = ComputerKey::generate().unwrap();
    let connector = tokio_rustls::TlsConnector::from(tls::client::pairing(&pkcs8).unwrap());
    let name = rustls::pki_types::ServerName::try_from("nexus-station").unwrap();
    let stream = tokio::net::TcpStream::connect(at).await.unwrap();
    let tls = connector.connect(name, stream).await.unwrap();
    let station = tls.get_ref().1.peer_certificates().unwrap()[0]
        .as_ref()
        .to_vec();
    let exporter = tls
        .get_ref()
        .1
        .export_keying_material([0; 32], super::super::pairing::EXPORTER, None)
        .unwrap();
    let (mut socket, _) = tokio_tungstenite::client_async("ws://nexus-station/", tls)
        .await
        .unwrap();
    let ours = [9; 32];
    send(
        &mut socket,
        json!({"type":"pair","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2,
            "name":"Guesser","nonce":hex(&ours)}),
    )
    .await;
    let theirs = next(&mut socket).await;
    let nonce = super::super::pairing::hex32(theirs["nonce"].as_str().unwrap()).unwrap();
    let computer = tempo_stream::protocol::hex_bytes(key.public_key()).unwrap();
    let t = super::super::pairing::transcript(&station, &computer, &ours, &nonce, &exporter);
    let k = super::super::pairing::code_key(guess).unwrap();
    let proof = super::super::pairing::proof(&k, super::super::pairing::Side::Computer, &t);
    send(&mut socket, json!({"type":"pairProof","proof":hex(&proof)})).await;
    next(&mut socket).await
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// ★ The shack's proof is checked before this computer says a word of its own. A computer that
/// typed another code, or dialled a machine that does not hold the code it typed, stops with
/// `stationProofFailed` and never sends its proof: three such tries count nothing against the
/// code, and the window stays open for the right one. CONTROL: three computers that do send
/// their proofs with a wrong code close the window, as the shack's rules say.
#[tokio::test]
async fn the_shacks_proof_is_checked_before_the_computer_sends_its_own() {
    let s = shack(home());
    let scratch = Scratch::new();
    let lan = lan_on(&s, &scratch);
    let (_stop, stop) = watch::channel(false);
    let at = behind_a_port(&s, stop).await;
    lan.pair().unwrap();
    let shown = code(&lan.status().pairing.unwrap().code).unwrap();
    let mut wrong = shown;
    wrong[7] ^= 1;
    for _ in 0..super::super::book::WRONG_PROOFS {
        assert_eq!(
            pairing::pair(at, wrong, "Den PC", &[]).await.err(),
            Some("stationProofFailed")
        );
    }
    assert!(
        lan.status().pairing.is_some(),
        "a computer's departure was counted"
    );
    pairing::pair(at, shown, "Den PC", &[])
        .await
        .expect("the right code, after three that never left this computer");
    // The control: proofs that do leave with a wrong code are counted, and the third closes it.
    lan.pair().unwrap();
    let shown = code(&lan.status().pairing.unwrap().code).unwrap();
    let mut wrong = shown;
    wrong[0] ^= 1;
    for _ in 0..super::super::book::WRONG_PROOFS {
        assert_eq!(guess(at, &wrong).await["reason"], "wrongCode");
    }
    assert!(
        lan.status().pairing.is_none(),
        "the control: the window stayed open"
    );
    assert_eq!(
        pairing::pair(at, shown, "Den PC", &[]).await.err(),
        Some("pairingClosed")
    );
}

/// ★ A machine that is not the station cannot pair this computer, though it answers at the
/// station's address with a pairing window of its own: with the code the real station shows, its
/// proof does not hold, and this computer leaves (`stationProofFailed`). CONTROL: with that
/// machine's own code, the same client pairs with it, so the code alone decided.
#[tokio::test]
async fn a_machine_posing_as_the_station_learns_nothing() {
    let real = shack(home());
    let real_scratch = Scratch::new();
    let real_lan = lan_on(&real, &real_scratch);
    real_lan.pair().unwrap();
    let typed = code(&real_lan.status().pairing.unwrap().code).unwrap();
    let posing = shack(home());
    let posing_scratch = Scratch::new();
    let posing_lan = lan_on(&posing, &posing_scratch);
    posing_lan.pair().unwrap();
    let (_stop, stop) = watch::channel(false);
    let at = behind_a_port(&posing, stop).await;
    assert_eq!(
        pairing::pair(at, typed, "Den PC", &[]).await.err(),
        Some("stationProofFailed")
    );
    assert_eq!(
        posing_lan.status().devices.len(),
        1,
        "it paired this computer"
    );
    let own = code(&posing_lan.status().pairing.unwrap().code).unwrap();
    let kept = pairing::pair(at, own, "Den PC", &[])
        .await
        .expect("the control");
    assert_eq!(kept.station_key, posing.public_key);
}

// ----- The versions -----

/// ★ As ruled on 2026-10-04 ("Refuse, say which"): a station that speaks other versions refuses
/// this computer's hello saying which side to update, and the page is told exactly that, on the
/// road and on a pairing alike; this computer's hello names this build's versions, which are the
/// station's own. CONTROL: a station of this build welcomes it (every road above).
#[tokio::test]
async fn a_station_of_another_version_says_which_side_to_update() {
    for side in ["updateStation", "updateComputer"] {
        let key = fixture_key();
        let pinned = tls::Identity::new(&key, STATION.into())
            .unwrap()
            .public_key()
            .to_string();
        let refused = vec![vec![json!({"type":"refused","reason":side})]];
        let (at, mut heard) = scripted_station(Ipv4Addr::LOCALHOST, &key, refused.clone()).await;
        let mut kept = record(STATION, &["192.168.1.20"]);
        kept.station_key = pinned;
        assert_eq!(road::connect(&kept, Some(at)).await.err(), Some(side));
        let hello = heard.recv().await.unwrap();
        assert_eq!(
            hello,
            json!({"type":"hello","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2})
        );
        let (at, mut heard) = scripted_station(Ipv4Addr::LOCALHOST, &key, refused).await;
        assert_eq!(
            pairing::pair(at, [1; 8], "Den PC", &[]).await.err(),
            Some(side)
        );
        let asked = heard.recv().await.unwrap();
        assert_eq!(asked["type"], "pair");
        assert_eq!(
            (
                asked["protocol"].clone(),
                asked["stream"].clone(),
                asked["operation"].clone()
            ),
            (json!(VERSIONS.0), json!(VERSIONS.1), json!(VERSIONS.2))
        );
    }
    // What this build says is what the station's own check takes as its own.
    assert_eq!(VERSIONS, crate::remote_service::lan::VERSIONS);
}

// ----- The lane -----

/// ★ The road carries what the station's lane takes, as the station answers it: the state, an
/// acquire under the station's grant, a heartbeat, the status line, Stop and a release. Anything
/// else the page asks is answered here as the station would answer it (`stationUnsupported`) and
/// never sent, so the road stays open. CONTROL: after that, the lease still renews.
#[test]
fn the_road_carries_the_lease_the_status_line_and_stop() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let s = shack(home());
        let scratch = Scratch::new();
        let _lan = lan_on(&s, &scratch);
        let (_stop, stop) = watch::channel(false);
        let at = behind_a_port(&s, stop).await;
        let (mut opened, _) = road::connect(&paired_record(&s, at), Some(at))
            .await
            .unwrap();
        let status = loop {
            match opened.next().await {
                Some(ToPage::Station(text)) if text.contains("\"status\"") => break text,
                Some(_) => continue,
                None => panic!("the road closed"),
            }
        };
        assert_eq!(
            serde_json::from_str::<Value>(&status).unwrap(),
            json!({"type":"status","rigKeyed":null})
        );
        let acquired = acquire(&mut opened).await;
        assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
        let lease = acquired["value"]["leaseId"].clone();
        let refused = ask_road(&mut opened, json!({"type":"result","operationId":id()})).await;
        assert_eq!(refused["error"], "stationUnsupported");
        let renewed = ask_road(&mut opened, json!({"type":"heartbeat","leaseId":lease})).await;
        assert_eq!(
            renewed["value"]["phase"], "controlling",
            "the control: {renewed}"
        );
        let state = &renewed["value"];
        let stopped = ask_road(
            &mut opened,
            json!({"type":"stopTransmit","requestId":id(),"stationBootId":state["stationBootId"],
                "leaseId":state["leaseId"],"transmitEpoch":state["transmitEpoch"]}),
        )
        .await;
        assert_eq!(stopped["value"], json!({"stop":"accepted"}), "{stopped}");
        let released = ask_road(&mut opened, json!({"type":"release","leaseId":lease})).await;
        assert!(released.get("value").is_some(), "{released}");
        assert!(
            s.shared.authority.local_status()["controller"].is_null(),
            "{}",
            s.shared.authority.local_status()
        );
    });
}

/// ★ Control taken back by the hosted road and acquired again, with nothing at the station
/// changed: a hosted Turn on (or Turn off, or a browser revoked) ends this computer's lease as a
/// side effect, and the next heartbeat says so; the station gives the grant straight back, and
/// the page's state and acquire take control again. CONTROL: with LAN off, the grant is not given
/// back and the acquire is refused.
#[tokio::test]
async fn control_ended_by_the_hosted_road_is_acquired_again() {
    let s = shack(home());
    let scratch = Scratch::new();
    let lan = lan_on(&s, &scratch);
    let (_stop, stop) = watch::channel(false);
    let at = behind_a_port(&s, stop).await;
    let (mut opened, _) = road::connect(&paired_record(&s, at), Some(at))
        .await
        .unwrap();
    let first = acquire(&mut opened).await;
    assert_eq!(first["value"]["phase"], "controlling", "{first}");
    s.shared.authority.invalidate();
    let lost = ask_road(
        &mut opened,
        json!({"type":"heartbeat","leaseId":first["value"]["leaseId"]}),
    )
    .await;
    let reason = lost["error"].as_str().unwrap_or_default();
    assert!(
        reason == "notController" || reason == "leaseExpired",
        "the lease outlived the hosted road's decision: {lost}"
    );
    let again = acquire(&mut opened).await;
    assert_eq!(again["value"]["phase"], "controlling", "{again}");
    assert_ne!(again["value"]["leaseId"], first["value"]["leaseId"]);
    // The control.
    lan.turn_off();
    s.shared.authority.invalidate();
    let state = ask_road(&mut opened, json!({"type":"state"})).await;
    assert_ne!(state["value"]["phase"], "controlling", "{state}");
    assert_eq!(state["value"]["allowed"], false, "{state}");
}

// ----- The offer and the answer -----

/// The station's one stream slot, free again once a refused offer's session has finished ending.
pub(super) async fn slot_free(s: &Shack) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.shared.authority.claim_stream().is_none() {
        assert!(
            Instant::now() < deadline,
            "the stream slot was never given back"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// ★ The offer this computer signs passes the station's own admission (A5): through every check
/// the station makes, to the platform's own answer (this box captures no window). CONTROL, with
/// the same offer and the same key on a connection of the test's own: unsigned, signed by another
/// key, or signed for another session, the station refuses it `deviceKeyMismatch`.
#[tokio::test]
async fn the_offer_signed_here_passes_the_stations_admission() {
    let s = shack(home());
    let scratch = Scratch::new();
    let _lan = lan_on(&s, &scratch);
    let (_stop, stop) = watch::channel(false);
    let (offer, _) = recorded();
    // The controls first, on a connection of the test's own, under one lease.
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
    let mut socket = socket.unwrap();
    let welcome = hello(&mut socket, VERSIONS).await;
    let session = welcome["sessionId"].as_str().unwrap().to_string();
    let state = ask(&mut socket, json!({"type":"state","requestId":id()})).await;
    let acquired = ask(
        &mut socket,
        json!({"type":"acquire","requestId":id(),"stationBootId":state["value"]["stationBootId"]}),
    )
    .await;
    let lease = acquired["value"]["leaseId"].clone();
    let mine = ComputerKey::restore(&s.computer.key).unwrap();
    let other = ComputerKey::generate().unwrap().0;
    let controls = [
        ("unsigned", json!({"kind":"offer","sdp":offer})),
        (
            "another key",
            json!({"kind":"offer","sdp":offer,"publicKey":mine.public_key(),
                "signature":other.sign_offer(&offer, STATION, &s.device, &session).unwrap()}),
        ),
        (
            "another session",
            json!({"kind":"offer","sdp":offer,"publicKey":mine.public_key(),
                "signature":mine.sign_offer(&offer, STATION, &s.device, &id()).unwrap()}),
        ),
        (
            "the control's control: signed here for this session",
            json!({"kind":"offer","sdp":offer,"publicKey":mine.public_key(),
                "signature":mine.sign_offer(&offer, STATION, &s.device, &session).unwrap()}),
        ),
    ];
    for (who, payload) in controls {
        slot_free(&s).await;
        send(
            &mut socket,
            json!({"type":"streamSignal","leaseId":lease,"payload":payload}),
        )
        .await;
        let answered = next(&mut socket).await;
        let expected = if who.starts_with("the control") {
            "streamUnavailable"
        } else {
            "deviceKeyMismatch"
        };
        assert_eq!(answered["reason"], expected, "{who}: {answered}");
    }
    let released = ask(
        &mut socket,
        json!({"type":"release","requestId":id(),"leaseId":lease}),
    )
    .await;
    assert!(released.get("value").is_some(), "{released}");
    // The road: the page's offer goes unsigned to it, and is signed on its way.
    slot_free(&s).await;
    let at = behind_a_port(&s, stop).await;
    let (mut opened, _) = road::connect(&paired_record(&s, at), Some(at))
        .await
        .unwrap();
    let acquired = acquire(&mut opened).await;
    assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
    let lease = acquired["value"]["leaseId"].as_str().unwrap().to_string();
    opened
        .signal(
            lease,
            BrowserSignal::Offer {
                sdp: offer,
                public_key: None,
                signature: None,
            },
        )
        .await
        .unwrap();
    let ended = heard(&mut opened).await;
    assert_eq!(ended["type"], "streamState", "{ended}");
    assert_eq!(ended["reason"], "streamUnavailable", "{ended}");
}

/// ★ The answer is checked here against the key this computer pinned (S3-M1), for exactly this
/// offer, station, device and session: what the station's own signer writes holds. CONTROL:
/// under another key, for another session, device, station or offer, or with no signature, it
/// does not.
#[test]
fn the_answer_is_checked_against_the_pinned_key() {
    let (offer, answer) = recorded();
    let station = tls::Identity::new(&fixture_key(), STATION.into()).unwrap();
    let ids = Ids {
        station: STATION.into(),
        device: "40000000-0000-4000-8000-000000000001".into(),
        session: "50000000-0000-4000-8000-000000000001".into(),
    };
    let signed = station
        .signer
        .sign_answer(&offer, &answer, &ids.station, &ids.device, &ids.session)
        .unwrap();
    let key = station.public_key().to_string();
    assert_eq!(check_answer(&key, &signed, &offer, &ids), Ok(()));
    let other = "70000000-0000-4000-8000-000000000001".to_string();
    let cases = [
        ("another key", another_key(), offer.clone(), ids.clone()),
        (
            "another session",
            key.clone(),
            offer.clone(),
            Ids {
                session: other.clone(),
                ..ids.clone()
            },
        ),
        (
            "another device",
            key.clone(),
            offer.clone(),
            Ids {
                device: other.clone(),
                ..ids.clone()
            },
        ),
        (
            "another station",
            key.clone(),
            offer.clone(),
            Ids {
                station: other.clone(),
                ..ids.clone()
            },
        ),
        ("another offer", key.clone(), answer.clone(), ids.clone()),
    ];
    for (what, key, offered, ids) in cases {
        assert_eq!(
            check_answer(&key, &signed, &offered, &ids),
            Err("stationKeyMismatch"),
            "{what}"
        );
    }
    assert_eq!(
        check_answer(&key, &answer, &offer, &ids),
        Err("stationNotSigned")
    );
}

/// A machine on `on` holding `key` that answers each message it hears with the next group of
/// `replies` (TLS with any P-256 key taken, and the upgrade). What it heard comes back on the
/// channel.
pub(super) async fn scripted_station(
    on: Ipv4Addr,
    key: &str,
    replies: Vec<Vec<Value>>,
) -> (SocketAddrV4, tokio::sync::mpsc::UnboundedReceiver<Value>) {
    let identity = tls::Identity::new(key, STATION.into()).unwrap();
    let config = tls::server(&identity, Arc::new(|_: &[u8; 32]| None), Arc::new(|| true)).unwrap();
    let listener = tokio::net::TcpListener::bind((on, 0)).await.unwrap();
    let SocketAddr::V4(at) = listener.local_addr().unwrap() else {
        unreachable!()
    };
    let (told, heard) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let Ok(tls) = tokio_rustls::TlsAcceptor::from(config).accept(stream).await else {
            return;
        };
        let Ok(mut socket) = tokio_tungstenite::accept_async(tls).await else {
            return;
        };
        let mut replies = replies.into_iter();
        while let Some(Ok(message)) = socket.next().await {
            let Message::Text(text) = message else {
                continue;
            };
            let _ = told.send(serde_json::from_str::<Value>(&text).unwrap());
            for answer in replies.next().unwrap_or_default() {
                let _ = socket.send(Message::Text(answer.to_string().into())).await;
            }
        }
    });
    (at, heard)
}

/// Nothing more from the road for a moment.
async fn quiet(road: &mut road::Road) -> bool {
    tokio::time::timeout(Duration::from_millis(300), road.next())
        .await
        .is_err()
}

/// ★ The page never sees an answer that does not hold: the road refuses it (`answerRefused`), ends
/// the stream at the station (a `close` under the same lease), and passes on none of the
/// candidates that came before it. CONTROL: the station's own signature holds, and the road passes
/// the answer on, then the candidate that waited for it.
#[tokio::test]
async fn an_answer_that_does_not_hold_never_reaches_the_page() {
    for honest in [true, false] {
        let key = fixture_key();
        let identity = tls::Identity::new(&key, STATION.into()).unwrap();
        let mut kept = record(STATION, &["192.168.1.20"]);
        kept.station_key = identity.public_key().to_string();
        let session = "50000000-0000-4000-8000-000000000002";
        let (offer, answer) = recorded();
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
            Ipv4Addr::LOCALHOST,
            &key,
            vec![vec![welcome], vec![candidate, answered]],
        )
        .await;
        let (mut opened, _) = road::connect(&kept, Some(at)).await.unwrap();
        let lease = "30000000-0000-4000-8000-000000000003".to_string();
        opened
            .signal(
                lease.clone(),
                BrowserSignal::Offer {
                    sdp: offer,
                    public_key: None,
                    signature: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(there.recv().await.unwrap()["type"], "hello");
        let offered = there.recv().await.unwrap();
        assert_eq!(
            offered["payload"]["publicKey"],
            ComputerKey::restore(&kept.pkcs8).unwrap().public_key()
        );
        let first = heard(&mut opened).await;
        if honest {
            assert_eq!(first["payload"]["kind"], "answer", "the control: {first}");
            let then = heard(&mut opened).await;
            assert_eq!(then["payload"]["kind"], "candidate", "the control: {then}");
        } else {
            assert_eq!(first, json!({"answerRefused":"stationKeyMismatch"}));
            assert_eq!(
                there.recv().await.unwrap(),
                json!({"type":"streamSignal","leaseId":lease,"payload":{"kind":"close"}})
            );
            assert!(quiet(&mut opened).await, "something more reached the page");
        }
    }
}

// ----- End to end -----

/// The window's own page socket on `origin`: the secret in its path and the window's own origin.
pub(super) async fn page_socket(origin: &Origin) -> Client2 {
    let page = origin.page();
    let secret = page.split('/').nth(3).unwrap();
    let url = format!("ws://127.0.0.1:{}/{secret}/socket", origin.port());
    let mut request =
        tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
            url.as_str(),
        )
        .unwrap();
    request.headers_mut().insert(
        "Origin",
        format!("http://127.0.0.1:{}", origin.port())
            .parse()
            .unwrap(),
    );
    tokio_tungstenite::connect_async(request).await.unwrap().0
}

pub(super) type Client2 =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// What the page is told next, other than the station's status line.
pub(super) async fn told(page: &mut Client2) -> Value {
    loop {
        match tokio::time::timeout(Duration::from_secs(20), page.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] != "status" {
                    return value;
                }
            }
            Ok(Some(Ok(_))) => {}
            other => panic!("the page was told nothing: {other:?}"),
        }
    }
}

/// An operation request as the page's own client sends it, and its answer, asked again while
/// the station is merely busy.
pub(super) async fn page_ask(page: &mut Client2, request: Value) -> Value {
    for _ in 0..50 {
        let mut request = request.clone();
        if request["type"] != "stopTransmit" {
            request["requestId"] = json!(id());
        }
        let asked = json!({"type":"operationRequest","operationVersion":4,"request":request});
        page.send(Message::Text(asked.to_string().into()))
            .await
            .unwrap();
        let answer = told(page).await;
        if answer["error"] != "stationBusy" {
            return answer;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the station stayed busy")
}

/// ★ End to end, on this box's own private address: the shack's real listener, and this
/// computer's loopback origin with the window's own page socket. The page pairs with the code the
/// shack shows (typed in capitals, with spaces) and this computer keeps the station; the page
/// connects to it, reads the state, takes control on the station's own grant (LAN is on), the
/// station is keyed under the session's presence, and the page's Stop stops it; the page's
/// disconnect ends the lease at the station. CONTROL: on the air until the Stop. Skipped, saying
/// so, on a box with no private address of its own.
#[test]
fn pairing_then_control_then_stop_from_the_page_end_to_end() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
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
            s.shared.desk.book.clone(),
        );
        lan.turn_on(None, Some(port)).unwrap();
        let at = SocketAddr::new(address.into(), port);
        eventually(&lan, "never listened", |st| {
            st.listening.as_deref() == Some(at.to_string().as_str())
        })
        .await;
        lan.pair().unwrap();
        let shown = lan.status().pairing.unwrap().code;

        let store = StationStore::default();
        let origin = Origin::start(Reach {
            assets: Arc::new(|_: &str| None),
            stations: Arc::new(Stations::new(Arc::new(store.clone()))),
            find: Arc::new(|_| Ok(Vec::new())),
            name: "Den PC".into(),
        })
        .unwrap();
        let mut page = page_socket(&origin).await;
        let pair = json!({"type":"pair","address":at.to_string(),"code":spaced_capitals(&shown),
            "name":"Den PC"});
        page.send(Message::Text(pair.to_string().into()))
            .await
            .unwrap();
        let paired = told(&mut page).await;
        assert_eq!(paired["type"], "paired", "{paired}");
        assert_eq!(paired["station"]["id"], STATION);
        assert_eq!(paired["station"]["addresses"][0], at.to_string());
        assert_eq!(paired["station"]["key"], json!(lan.status().key.unwrap()));
        assert!(store.raw(&format!("lan-station-{STATION}")).is_some());

        let connect = json!({"type":"connect","stationId":STATION});
        page.send(Message::Text(connect.to_string().into()))
            .await
            .unwrap();
        let connected = told(&mut page).await;
        assert_eq!(connected["type"], "connected", "{connected}");
        assert_eq!(connected["stationKey"], s.public_key);
        let device = connected["deviceId"].as_str().unwrap().to_string();
        let session = connected["sessionId"].as_str().unwrap().to_string();
        assert!(lan.status().devices.iter().any(|d| d.id == device));

        let state = page_ask(&mut page, json!({"type":"state"})).await;
        let boot = state["value"]["stationBootId"].clone();
        let acquired = page_ask(&mut page, json!({"type":"acquire","stationBootId":boot})).await;
        assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
        let state = acquired["value"].clone();
        {
            let now = Instant::now();
            let permit = s
                .shared
                .authority
                .stream_presence(&session, &device, state["leaseId"].as_str().unwrap(), now)
                .unwrap();
            let mut e = s.shared.engine.lock().unwrap();
            e.hold_remote_presence(permit, now);
            e.set_operating_mode("phone", false);
            e.set_ptt(true);
            assert!(e.manual_ptt(), "the control: on the air");
        }
        let stopped = page_ask(
            &mut page,
            json!({"type":"stopTransmit","requestId":id(),"stationBootId":state["stationBootId"],
                "leaseId":state["leaseId"],"transmitEpoch":state["transmitEpoch"]}),
        )
        .await;
        assert_eq!(stopped["value"], json!({"stop":"accepted"}), "{stopped}");
        let deadline = Instant::now() + Duration::from_secs(2);
        while s.shared.engine.lock().unwrap().manual_ptt() {
            assert!(
                Instant::now() < deadline,
                "the page's Stop left the station keyed"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        page.send(Message::Text(
            json!({"type":"disconnect"}).to_string().into(),
        ))
        .await
        .unwrap();
        assert_eq!(
            told(&mut page).await,
            json!({"type":"closed","reason":"disconnected"})
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !s.shared.authority.local_status()["controller"].is_null() {
            assert!(Instant::now() < deadline, "the lease outlived the road");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
}

/// The page's connect to the station, at `typed` if the operator typed an address, and what the
/// page is told of it.
pub(super) async fn page_connect(page: &mut Client2, typed: Option<SocketAddrV4>) -> Value {
    let mut asked = json!({"type":"connect","stationId":STATION});
    if let Some(typed) = typed {
        asked["address"] = json!(typed.to_string());
    }
    page.send(Message::Text(asked.to_string().into()))
        .await
        .unwrap();
    told(page).await
}

/// ★ Found by name, for a station that is not where it was (as ruled on 2026-10-04, "By name, or
/// typed"), through the window's own page socket: the road is tried at the address the operator
/// typed, then at the remembered ones, the last that worked first, and only when none of them
/// answered, where a look by name finds this station (an advert whose key tag starts the key
/// pinned for it). The address that welcomed it is remembered first. CONTROL: a remembered address
/// that works is the road and no look is made; a typed one is tried first; an advert with another
/// station's key tag is never tried, though it names a port of this one, so with only that found
/// the answer stays the remembered address's own (`refused`). Skipped, saying so, on a box with no
/// private address of its own.
#[tokio::test]
async fn a_station_not_where_it_was_is_tried_where_it_is_found_by_name() {
    use ring::digest::{digest, SHA256};
    use tempo_stream::lan::dnssd::Found;
    let Some(private) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own");
        return;
    };
    let s = shack(home());
    let (_stop, stop) = watch::channel(false);
    let live = behind_a_port_on(&s, private, stop).await;
    let (_stop_too, stop_too) = watch::channel(false);
    let also = behind_a_port_on(&s, private, stop_too).await;
    let dead = {
        let probe = std::net::TcpListener::bind((private, 0)).unwrap();
        let SocketAddr::V4(at) = probe.local_addr().unwrap() else {
            unreachable!()
        };
        at
    };
    let spki = tempo_stream::protocol::hex_bytes(&s.public_key).unwrap();
    let tag = hex(digest(&SHA256, &spki).as_ref())[..16].to_string();
    let advert = |address: SocketAddrV4, key: &str| Found {
        name: "Nexus 3F2A 9B1C".into(),
        address,
        protocol: 1,
        key: key.into(),
    };
    // Another station's advert first, at a port of this one: were it tried, it would welcome.
    let other = advert(also, "00c0ffee00c0ffee");
    let adverts = Arc::new(Mutex::new(vec![other.clone(), advert(live, &tag)]));
    let looks = Arc::new(AtomicUsize::new(0));
    let store = StationStore::default();
    let stations = Stations::new(Arc::new(store.clone()));
    let origin = Origin::start(Reach {
        assets: Arc::new(|_: &str| None),
        stations: Arc::new(Stations::new(Arc::new(store.clone()))),
        find: {
            let (adverts, looks) = (adverts.clone(), looks.clone());
            Arc::new(move |_| {
                looks.fetch_add(1, Ordering::SeqCst);
                Ok(adverts.lock().unwrap().clone())
            })
        },
        name: "Den PC".into(),
    })
    .unwrap();
    let mut page = page_socket(&origin).await;
    let remembered = |addresses: &[SocketAddrV4]| {
        let mut kept = paired_record(&s, live);
        kept.addresses = addresses.iter().map(|a| a.to_string()).collect();
        stations.keep(&kept).unwrap();
    };

    remembered(&[dead]);
    let connected = page_connect(&mut page, None).await;
    assert_eq!(connected["type"], "connected", "{connected}");
    assert_eq!(
        connected["address"],
        live.to_string(),
        "where it is found by name, past another station's advert"
    );
    assert_eq!(looks.load(Ordering::SeqCst), 1);
    assert_eq!(
        stations.get(STATION).unwrap().unwrap().addresses,
        [live.to_string(), dead.to_string()],
        "remembered first for next time"
    );

    remembered(&[also, live]);
    let connected = page_connect(&mut page, None).await;
    assert_eq!(
        connected["address"],
        also.to_string(),
        "the last that worked, first"
    );
    let connected = page_connect(&mut page, Some(live)).await;
    assert_eq!(
        connected["address"],
        live.to_string(),
        "the typed one, first"
    );
    assert_eq!(
        looks.load(Ordering::SeqCst),
        1,
        "a look past one that worked"
    );

    *adverts.lock().unwrap() = vec![other];
    remembered(&[dead]);
    assert_eq!(
        page_connect(&mut page, None).await,
        json!({"type":"connectRefused","reason":"refused"}),
        "the control"
    );
    assert_eq!(looks.load(Ordering::SeqCst), 2);
}

/// ★ Another station at a remembered address (a DHCP lease moved the shack, and another Nexus with
/// LAN on took its old address) does not end the connect: past it, the remaining remembered
/// addresses are tried, then where a look by name finds this station, and the road opens there.
/// The page is told `keyChanged` only when no address yields the pinned key, and then also when
/// what was found by name told less (nothing listens there). CONTROL: with nothing found by name,
/// the remembered address's own answer is told (`keyChanged`).
#[tokio::test]
async fn another_station_at_a_remembered_address_does_not_hide_this_one() {
    use ring::digest::{digest, SHA256};
    use tempo_stream::lan::dnssd::Found;
    let Some(private) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own");
        return;
    };
    let s = shack(home());
    let other = shack(home());
    let (_stop, stop) = watch::channel(false);
    let live = behind_a_port_on(&s, private, stop.clone()).await;
    let taken = behind_a_port_on(&other, private, stop).await;
    let spki = tempo_stream::protocol::hex_bytes(&s.public_key).unwrap();
    let tag = hex(digest(&SHA256, &spki).as_ref())[..16].to_string();
    let found = vec![Found {
        name: "Nexus 3F2A 9B1C".into(),
        address: live,
        protocol: 1,
        key: tag,
    }];
    let adverts = Arc::new(Mutex::new(found));
    let looks = Arc::new(AtomicUsize::new(0));
    let store = StationStore::default();
    let stations = Stations::new(Arc::new(store.clone()));
    let origin = Origin::start(Reach {
        assets: Arc::new(|_: &str| None),
        stations: Arc::new(Stations::new(Arc::new(store.clone()))),
        find: {
            let (adverts, looks) = (adverts.clone(), looks.clone());
            Arc::new(move |_| {
                looks.fetch_add(1, Ordering::SeqCst);
                Ok(adverts.lock().unwrap().clone())
            })
        },
        name: "Den PC".into(),
    })
    .unwrap();
    let mut page = page_socket(&origin).await;
    let remembered = |addresses: &[SocketAddrV4]| {
        let mut kept = paired_record(&s, live);
        kept.addresses = addresses.iter().map(|a| a.to_string()).collect();
        stations.keep(&kept).unwrap();
    };

    remembered(&[taken]);
    let connected = page_connect(&mut page, None).await;
    assert_eq!(connected["type"], "connected", "{connected}");
    assert_eq!(connected["address"], live.to_string());
    assert_eq!(looks.load(Ordering::SeqCst), 1);

    remembered(&[taken, live]);
    let connected = page_connect(&mut page, None).await;
    assert_eq!(
        connected["address"],
        live.to_string(),
        "the remembered address past the other station's"
    );
    assert_eq!(
        looks.load(Ordering::SeqCst),
        1,
        "a look past one that worked"
    );

    adverts.lock().unwrap().clear();
    remembered(&[taken]);
    assert_eq!(
        page_connect(&mut page, None).await,
        json!({"type":"connectRefused","reason":"keyChanged"}),
        "the control"
    );
    assert_eq!(looks.load(Ordering::SeqCst), 2);

    let dead = {
        let probe = std::net::TcpListener::bind((private, 0)).unwrap();
        let SocketAddr::V4(at) = probe.local_addr().unwrap() else {
            unreachable!()
        };
        at
    };
    let spki = tempo_stream::protocol::hex_bytes(&s.public_key).unwrap();
    *adverts.lock().unwrap() = vec![Found {
        name: "Nexus 3F2A 9B1C".into(),
        address: dead,
        protocol: 1,
        key: hex(digest(&SHA256, &spki).as_ref())[..16].to_string(),
    }];
    remembered(&[taken]);
    assert_eq!(
        page_connect(&mut page, None).await,
        json!({"type":"connectRefused","reason":"keyChanged"}),
        "found by name where nothing listens"
    );
    assert_eq!(looks.load(Ordering::SeqCst), 3);
}

/// ★ Pairing again with a station this computer is paired with already keeps this computer's own
/// key for it (the operator's ruling of 2026-10-04, "Reuse the PC's own key"), so the station
/// replaces its one entry: it lists this computer once, under the same device id, and the key this
/// computer kept still opens the road. The first connection, which met the station's key and left
/// after the station's proof, spends nothing: the same code pairs on the second. CONTROL: this
/// computer's key for one station is never another's: knowing only another station's record, the
/// pairing makes a key of its own. Skipped, saying so, on a box with no private address of its own.
#[tokio::test]
async fn pairing_again_with_a_known_station_keeps_this_computers_key() {
    let Some(private) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own");
        return;
    };
    let s = shack(home());
    let scratch = Scratch::new();
    let lan = lan_on(&s, &scratch);
    let (_stop, stop) = watch::channel(false);
    let at = behind_a_port_on(&s, private, stop).await;
    let store = StationStore::default();
    let stations = Arc::new(Stations::new(Arc::new(store.clone())));
    let origin = Origin::start(Reach {
        assets: Arc::new(|_: &str| None),
        stations: stations.clone(),
        find: Arc::new(|_| Ok(Vec::new())),
        name: "Den PC".into(),
    })
    .unwrap();
    let mut page = page_socket(&origin).await;
    for _ in 0..2 {
        lan.pair().unwrap();
        let shown = lan.status().pairing.unwrap().code;
        let pair = json!({"type":"pair","address":at.to_string(),"code":shown,"name":"Den PC"});
        page.send(Message::Text(pair.to_string().into()))
            .await
            .unwrap();
        let paired = told(&mut page).await;
        assert_eq!(paired["type"], "paired", "{paired}");
    }
    let kept = stations.get(STATION).unwrap().unwrap();
    let devices = lan.status().devices;
    let ours: Vec<_> = devices.iter().filter(|d| d.name == "Den PC").collect();
    assert_eq!(
        ours.len(),
        1,
        "the station lists this computer {} times",
        ours.len()
    );
    assert_eq!(ours[0].id, kept.device_id);
    assert_eq!(devices.len(), 2, "{devices:?}");
    let (opened, _) = road::connect(&kept, Some(at)).await.unwrap();
    assert_eq!(opened.ids.device, kept.device_id);

    let mut elsewhere = paired_record(&s, at);
    elsewhere.station_key = another_key();
    elsewhere.station_id = "70000000-0000-4000-8000-000000000001".into();
    lan.pair().unwrap();
    let shown = code(&lan.status().pairing.unwrap().code).unwrap();
    let made = pairing::pair(at, shown, "Den PC", &[elsewhere])
        .await
        .unwrap();
    assert!(
        made.pkcs8 != s.computer.key && made.pkcs8 != kept.pkcs8,
        "another station's key, or this one's, was used"
    );
}
