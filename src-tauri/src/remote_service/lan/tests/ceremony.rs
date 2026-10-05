//! Pairing at the shack, end to end over real TCP: the ceremony as ruled on 2026-10-04 ("One
//! press"), the pairing-only connection, the book in its credential store, removing a computer,
//! resetting the key, and what comes back after a restart. Keys are made for each run and never
//! written anywhere but the in-memory store.
use super::super::book::{Checked, MAX_PAIRED, PAIRING_FOR, WRONG_PROOFS};
use super::super::pairing::{self as proofs, Side};
use super::*;

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn nonce() -> [u8; 32] {
    let mut bytes = [0; 32];
    ring::rand::SecureRandom::fill(&SystemRandom::new(), &mut bytes).unwrap();
    bytes
}

/// The code the shack shows while its window is open, as the operator reads it off the screen.
fn shown_code(book: &Book) -> [u8; 8] {
    let code = book
        .view(Instant::now())
        .pairing
        .expect("a window is open")
        .code;
    tempo_stream::protocol::hex_bytes(&code)
        .unwrap()
        .try_into()
        .unwrap()
}

/// A computer's side of a pairing connection: what the shack presented, and this session's
/// exporter.
pub(super) struct Pairing {
    pub(super) socket: Client,
    pub(super) shack: Vec<u8>,
    pub(super) exporter: [u8; 32],
}

/// TLS with `key` and no shack key pinned, then the upgrade.
pub(super) async fn open_pairing(
    stream: tokio::net::TcpStream,
    key: &str,
) -> Result<Pairing, String> {
    let connector = tokio_rustls::TlsConnector::from(tls::client::pairing(key).unwrap());
    let name = rustls::pki_types::ServerName::try_from("nexus-station").unwrap();
    let tls = connector
        .connect(name, stream)
        .await
        .map_err(|e| format!("tls: {e}"))?;
    let shack = tls.get_ref().1.peer_certificates().unwrap()[0]
        .as_ref()
        .to_vec();
    let exporter = tls
        .get_ref()
        .1
        .export_keying_material([0; 32], proofs::EXPORTER, None)
        .unwrap();
    let (socket, _) = tokio_tungstenite::client_async("ws://nexus-station/", tls)
        .await
        .map_err(|e| format!("upgrade: {e}"))?;
    Ok(Pairing {
        socket,
        shack,
        exporter,
    })
}

/// The station's `connection` for a computer at `peer`, pairing with `key`.
async fn dial_pairing(
    s: &Shack,
    key: &str,
    peer: &str,
    stop: watch::Receiver<bool>,
) -> (tokio::task::JoinHandle<()>, Result<Pairing, String>) {
    let (station, computer) = pair().await;
    let task = tokio::spawn(channel::connection(
        station,
        peer.parse().unwrap(),
        s.shared.clone(),
        stop,
    ));
    (task, open_pairing(computer, key).await)
}

/// The computer's half with `typed` as the code it was given: it asks to pair, checks the shack's
/// proof, and only then sends its own. `{"shackProofFailed":true}` if the shack's did not hold,
/// as a computer that stops there says; otherwise the shack's last word.
async fn prove_code(p: &mut Pairing, computer: &Computer, typed: &[u8; 8], name: &str) -> Value {
    let ours = nonce();
    send(
        &mut p.socket,
        json!({"type":"pair","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2,
            "name":name,"nonce":hex(&ours)}),
    )
    .await;
    let theirs = next(&mut p.socket).await;
    if theirs["type"] != "pairProof" {
        return theirs;
    }
    let shack_nonce = proofs::hex32(theirs["nonce"].as_str().unwrap()).unwrap();
    let t = proofs::transcript(&p.shack, &computer.spki, &ours, &shack_nonce, &p.exporter);
    let k = proofs::code_key(typed).unwrap();
    let shack_proof = proofs::hex32(theirs["proof"].as_str().unwrap()).unwrap();
    if !proofs::holds(&k, Side::Station, &t, &shack_proof) {
        return json!({"shackProofFailed": true});
    }
    send(
        &mut p.socket,
        json!({"type":"pairProof","proof":hex(&proofs::proof(&k, Side::Computer, &t))}),
    )
    .await;
    next(&mut p.socket).await
}

/// A hostile computer: it sends a proof made with `guess` whether or not the shack's proof held.
pub(super) async fn guess_code(p: &mut Pairing, computer: &Computer, guess: &[u8; 8]) -> Value {
    let ours = nonce();
    send(
        &mut p.socket,
        json!({"type":"pair","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2,
            "name":"Guesser","nonce":hex(&ours)}),
    )
    .await;
    let theirs = next(&mut p.socket).await;
    if theirs["type"] != "pairProof" {
        return theirs;
    }
    let shack_nonce = proofs::hex32(theirs["nonce"].as_str().unwrap()).unwrap();
    let t = proofs::transcript(&p.shack, &computer.spki, &ours, &shack_nonce, &p.exporter);
    let k = proofs::code_key(guess).unwrap();
    send(
        &mut p.socket,
        json!({"type":"pairProof","proof":hex(&proofs::proof(&k, Side::Computer, &t))}),
    )
    .await;
    next(&mut p.socket).await
}

/// A code that is not `code`.
fn other_than(code: &[u8; 8]) -> [u8; 8] {
    let mut wrong = *code;
    wrong[7] ^= 1;
    wrong
}

/// Welcomed on a connection of its own, with the shack's key pinned, then the state and an
/// acquire, with no grant given by the test: control only if the station gives it. The socket, the
/// session the station stamped, and the acquire's answer.
async fn acquire_as_paired(
    s: &Shack,
    computer: &Computer,
    peer: &str,
    stop: watch::Receiver<bool>,
) -> (Client, String, Value) {
    let (_task, socket) = dial(s, &computer.key, peer, stop).await;
    let mut socket = socket.expect("a paired computer gets through TLS");
    let welcome = hello(&mut socket, VERSIONS).await;
    assert_eq!(welcome["type"], "welcome", "{welcome}");
    let state = ask(&mut socket, json!({"type":"state","requestId":id()})).await;
    let boot = state["value"]["stationBootId"].clone();
    let acquired = ask(
        &mut socket,
        json!({"type":"acquire","requestId":id(),"stationBootId":boot}),
    )
    .await;
    let session = welcome["sessionId"].as_str().unwrap().to_string();
    (socket, session, acquired)
}

/// LAN on at this station, from `s`'s own book, with no socket of its own: the shack's grants and
/// presses, with connections dialled straight into `channel::connection`.
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

fn controls(s: &Shack, device: &str) -> bool {
    s.shared.authority.local_status()["controlDevices"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d == device)
}

// ----- The ceremony (D4 as ruled: one press) -----

/// ★ One press: the operator opens the window, a computer that types the code is paired there and
/// then, with no second press at the shack. It is told the device id its key gives and the LAN
/// station id; its record is in the credential store; the window has closed; and on a connection
/// of its own, with the shack's key pinned, it is welcomed as that device. CONTROL: before the
/// window, the same key is refused in the handshake.
#[tokio::test]
async fn a_good_code_pairs_the_computer_at_once() {
    let s = shack(home());
    let book = s.shared.desk.book.clone();
    let newcomer = Computer::new();
    let (_stop, stop) = watch::channel(false);
    let (_task, refused) = dial(&s, &newcomer.key, PEER, stop.clone()).await;
    let refused = match refused {
        Ok(mut socket) => hello(&mut socket, VERSIONS).await,
        Err(e) => json!({ "refused": e }),
    };
    assert!(
        refused["type"] != "welcome",
        "control: unpaired, and welcomed"
    );

    book.open(Instant::now(), 0).unwrap();
    let code = shown_code(&book);
    let (_task, p) = dial_pairing(&s, &newcomer.key, "192.168.1.40:50000", stop.clone()).await;
    let mut p = p.expect("an unknown key reaches the pairing while the window is open");
    let answer = prove_code(&mut p, &newcomer, &code, "Den PC").await;
    assert_eq!(
        answer,
        json!({"type":"paired","deviceId":newcomer.device,"stationId":STATION}),
        "paired with no second press"
    );
    assert!(
        newcomer.device.as_bytes()[14] == b'8',
        "{}",
        newcomer.device
    );
    assert_eq!(book.paired(&newcomer.pin), Some(newcomer.device.clone()));
    assert!(!book.pairing(Instant::now()), "one pairing per code");
    let kept = book_on(&s.store);
    assert_eq!(
        kept.paired(&newcomer.pin),
        Some(newcomer.device.clone()),
        "kept in the credential store"
    );
    let shown = kept.view(Instant::now()).devices;
    assert!(shown
        .iter()
        .any(|d| d.name == "Den PC" && d.key == hex(&newcomer.pin)));

    // The computer pins the key the shack presented while pairing, and connects as itself.
    assert_eq!(
        hex(&p.shack),
        s.public_key,
        "the shack presented its own key"
    );
    let (_task, socket) = dial(&s, &newcomer.key, "192.168.1.40:50001", stop).await;
    let welcome = hello(&mut socket.unwrap(), VERSIONS).await;
    assert_eq!(welcome["type"], "welcome", "{welcome}");
    assert_eq!(welcome["deviceId"], newcomer.device);
}

/// ★ Three wrong proofs close the window: each is told `wrongCode` and closed, and after the
/// third even the right code pairs nobody. CONTROL: two wrong proofs leave it open, and the right
/// code then pairs.
#[tokio::test]
async fn three_wrong_codes_close_the_window() {
    for wrongs in [WRONG_PROOFS - 1, WRONG_PROOFS] {
        let s = shack(home());
        let book = s.shared.desk.book.clone();
        let (_stop, stop) = watch::channel(false);
        book.open(Instant::now(), 0).unwrap();
        let code = shown_code(&book);
        for n in 0..wrongs {
            let guesser = Computer::new();
            let peer = format!("192.168.1.{}:50000", 50 + n);
            let (task, p) = dial_pairing(&s, &guesser.key, &peer, stop.clone()).await;
            let answer = guess_code(&mut p.unwrap(), &guesser, &other_than(&code)).await;
            assert_eq!(answer, json!({"type":"refused","reason":"wrongCode"}));
            task.await.unwrap();
            assert!(book.paired(&guesser.pin).is_none(), "a wrong code paired");
        }
        let newcomer = Computer::new();
        let (_task, p) = dial_pairing(&s, &newcomer.key, "192.168.1.60:50000", stop).await;
        if wrongs == WRONG_PROOFS {
            assert!(!book.pairing(Instant::now()), "the window outlived three");
            assert!(
                p.is_err(),
                "an unknown key got through TLS with the window closed"
            );
            assert!(book.paired(&newcomer.pin).is_none());
        } else {
            let answer = prove_code(&mut p.unwrap(), &newcomer, &code, "Den PC").await;
            assert_eq!(answer["type"], "paired", "control: {answer}");
        }
    }
}

/// ★ A code pairs one computer. Once used, another computer that has it is not taken in the
/// handshake, and the paired one asking again is told the window has closed. CONTROL: a new code
/// pairs the second computer.
#[tokio::test]
async fn a_used_code_pairs_nobody_else() {
    let s = shack(home());
    let book = s.shared.desk.book.clone();
    let (_stop, stop) = watch::channel(false);
    book.open(Instant::now(), 0).unwrap();
    let code = shown_code(&book);
    let first = Computer::new();
    let (_task, p) = dial_pairing(&s, &first.key, PEER, stop.clone()).await;
    assert_eq!(
        prove_code(&mut p.unwrap(), &first, &code, "First").await["type"],
        "paired"
    );

    let second = Computer::new();
    let (task, p) = dial_pairing(&s, &second.key, "192.168.1.41:50000", stop.clone()).await;
    assert!(p.is_err(), "the used code's window still took a new key");
    task.await.unwrap();
    let (task, p) = dial_pairing(&s, &first.key, "192.168.1.42:50000", stop.clone()).await;
    assert_eq!(
        prove_code(&mut p.unwrap(), &first, &code, "First").await,
        json!({"type":"refused","reason":"pairingClosed"})
    );
    task.await.unwrap();
    assert!(book.paired(&second.pin).is_none());

    book.open(Instant::now(), 0).unwrap();
    let fresh = shown_code(&book);
    assert_ne!(fresh, code, "a new window, a new code");
    let (_task, p) = dial_pairing(&s, &second.key, "192.168.1.43:50000", stop).await;
    assert_eq!(
        prove_code(&mut p.unwrap(), &second, &fresh, "Second").await["type"],
        "paired",
        "control"
    );
}

/// ★ A code past its ten minutes pairs nobody: an unknown key is not taken in the handshake, and a
/// proof against a window that closed while it was on its way is told the window has closed.
/// CONTROL: the same window, still inside its ten minutes, takes the same proof.
#[tokio::test]
async fn an_expired_code_pairs_nobody() {
    let s = shack(home());
    let book = s.shared.desk.book.clone();
    let (_stop, stop) = watch::channel(false);
    let opened = Instant::now()
        .checked_sub(PAIRING_FOR + Duration::from_secs(1))
        .expect("this box has been up more than ten minutes");
    book.open(opened, 0).unwrap();
    assert!(!book.pairing(Instant::now()));
    let newcomer = Computer::new();
    let (task, p) = dial_pairing(&s, &newcomer.key, PEER, stop).await;
    assert!(p.is_err(), "an expired window took an unknown key");
    task.await.unwrap();
    assert!(book.view(Instant::now()).pairing.is_none(), "still shown");

    // Closed on the way: the window lives ten minutes from `now`, and the proof lands after.
    let now = Instant::now();
    book.open(now, 0).unwrap();
    let code = shown_code(&book);
    let (serial, _) = book.proof_key(now).unwrap();
    let t = [7; 32];
    let right = proofs::proof(&proofs::code_key(&code).unwrap(), Side::Computer, &t);
    assert_eq!(
        book.check(serial, &t, &right, now + PAIRING_FOR),
        Checked::Closed
    );
    book.open(now, 0).unwrap();
    let code = shown_code(&book);
    let (serial, _) = book.proof_key(now).unwrap();
    let right = proofs::proof(&proofs::code_key(&code).unwrap(), Side::Computer, &t);
    assert_eq!(
        book.check(
            serial,
            &t,
            &right,
            now + PAIRING_FOR - Duration::from_millis(1)
        ),
        Checked::Right,
        "control"
    );
}

/// The window's own rules, without a socket: the right proof closes it for good, a proof from a
/// window since replaced is a closed one, the third wrong one closes it, and the station's proof is
/// never the computer's. CONTROL: the right proof under the right window holds.
#[test]
fn the_window_keeps_its_rules() {
    let s = shack(home());
    let book = s.shared.desk.book.clone();
    let now = Instant::now();
    let t = [9; 32];
    book.open(now, 0).unwrap();
    let code = shown_code(&book);
    let k = proofs::code_key(&code).unwrap();
    let (first, _) = book.proof_key(now).unwrap();
    assert_eq!(
        book.check(first, &t, &proofs::proof(&k, Side::Station, &t), now),
        Checked::Wrong,
        "the shack's own proof passed for the computer's"
    );
    assert_eq!(
        book.check(first, &[8; 32], &proofs::proof(&k, Side::Computer, &t), now),
        Checked::Wrong,
        "a proof over another transcript held"
    );
    // A window opened since: what was started under the first finishes under nothing.
    book.open(now, 0).unwrap();
    let k2 = proofs::code_key(&shown_code(&book)).unwrap();
    let (second, _) = book.proof_key(now).unwrap();
    assert_eq!(
        book.check(first, &t, &proofs::proof(&k2, Side::Computer, &t), now),
        Checked::Closed
    );
    assert_eq!(
        book.check(second, &t, &proofs::proof(&k2, Side::Computer, &t), now),
        Checked::Right,
        "control"
    );
    assert_eq!(
        book.check(second, &t, &proofs::proof(&k2, Side::Computer, &t), now),
        Checked::Closed,
        "a right proof twice"
    );
    assert!(!book.pairing(now));
}

/// ★ A pairing whose proof held just before a reset is refused when it comes to be kept: its proof
/// was made under the station id the reset retired, and the list after a reset admits nobody, nor
/// after a restart. CONTROL: with no reset between, the same pairing is kept, under the new id.
#[test]
fn a_pairing_proved_before_a_reset_pairs_nobody() {
    let store = LanStore::default();
    let book = book_on(&store);
    let (identity, _) = book.identity().unwrap();
    let computer = Computer::new();
    let (now, t) = (Instant::now(), [9; 32]);
    let proved = |book: &Book| {
        book.open(now, 0).unwrap();
        let k = proofs::code_key(&shown_code(book)).unwrap();
        let (serial, _) = book.proof_key(now).unwrap();
        book.check(serial, &t, &proofs::proof(&k, Side::Computer, &t), now)
    };
    assert_eq!(proved(&book), Checked::Right);
    book.forget_all();
    book.renew().unwrap();
    assert_eq!(
        book.add(computer.pin, "Den PC", &identity.station_id),
        Err("pairingClosed")
    );
    assert_eq!(book.paired(&computer.pin), None);
    assert_eq!(
        book_on(&store).paired(&computer.pin),
        None,
        "paired after a restart"
    );

    let (renewed, _) = book.identity().unwrap();
    assert_ne!(renewed.station_id, identity.station_id);
    assert_eq!(proved(&book), Checked::Right);
    assert_eq!(
        book.add(computer.pin, "Den PC", &renewed.station_id),
        Ok(computer.device.clone()),
        "the control"
    );
    assert_eq!(
        book_on(&store).paired(&computer.pin),
        Some(computer.device.clone())
    );
}

/// ★ A man in the middle, holding a key of its own, shows each end a different key, and each TLS
/// session it runs has its own exporter. The proofs are bound to both, each on its own: with the
/// keys alike they fail on the exporters, and with the exporters alike they fail on the keys, though
/// every side knows the code. CONTROL: the same keys and exporter on both sides hold.
#[test]
fn a_man_in_the_middle_cannot_pass_the_proofs_along() {
    let (shack, computer, middle) = (Computer::new(), Computer::new(), Computer::new());
    let k = proofs::code_key(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    let (a, b) = (nonce(), nonce());
    let (toward_shack, toward_computer) = ([3; 32], [4; 32]);
    let passes = |proved: [u8; 32], seen: [u8; 32]| {
        proofs::holds(
            &k,
            Side::Station,
            &seen,
            &proofs::proof(&k, Side::Station, &proved),
        )
    };
    // The keys alike, as if the middle could present the shack's: the sessions still differ.
    let keys_alike = (
        proofs::transcript(&shack.spki, &computer.spki, &a, &b, &toward_shack),
        proofs::transcript(&shack.spki, &computer.spki, &a, &b, &toward_computer),
    );
    assert!(
        !passes(keys_alike.0, keys_alike.1),
        "the exporter binds nothing"
    );
    // The sessions alike, the keys as the middle shows them: the shack proved its own and the
    // middle's, the computer checks the middle's and its own.
    let sessions_alike = (
        proofs::transcript(&shack.spki, &middle.spki, &a, &b, &toward_shack),
        proofs::transcript(&middle.spki, &computer.spki, &a, &b, &toward_shack),
    );
    assert!(
        !passes(sessions_alike.0, sessions_alike.1),
        "the keys bind nothing"
    );
    let honest = proofs::transcript(&shack.spki, &computer.spki, &a, &b, &toward_shack);
    assert!(passes(honest, honest), "control");
}

// ----- The pairing-only connection: no authority, no engine, no stream -----

/// ★ A pairing-only connection reaches nothing. Inside an open window, a key no paired computer
/// holds asks to pair, then tries what a paired computer would (a state, an acquire, a Stop, a
/// stream signal, a hello): each closes the connection, and no lease, grant or pairing appears,
/// the stream slot is free and the transmitter was never keyed. A connection that has just paired
/// is closed the same way. CONTROL: the computer, once paired, on a connection of its own, takes
/// control with no grant given by the test.
#[tokio::test]
async fn a_pairing_only_connection_reaches_nothing() {
    let s = shack(home());
    let scratch = Scratch::new();
    let _lan = lan_on(&s, &scratch);
    let book = s.shared.desk.book.clone();
    book.open(Instant::now(), 0).unwrap();
    let code = shown_code(&book);
    let (_stop, stop) = watch::channel(false);
    let stranger = Computer::new();
    for (n, attempt) in [
        json!({"type":"operationRequest","request":{"type":"state","requestId":id()}}),
        json!({"type":"operationRequest","request":{"type":"acquire","requestId":id(),
            "stationBootId":id()}}),
        json!({"type":"operationRequest","request":{"type":"stopTransmit","requestId":id(),
            "stationBootId":id(),"leaseId":id(),"stopToken":id()}}),
        json!({"type":"streamSignal","leaseId":id(),"payload":{"type":"close"}}),
        json!({"type":"hello","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2}),
    ]
    .into_iter()
    .enumerate()
    {
        let peer = format!("192.168.1.{}:50000", 70 + n);
        let (task, p) = dial_pairing(&s, &stranger.key, &peer, stop.clone()).await;
        let mut p = p.unwrap();
        send(
            &mut p.socket,
            json!({"type":"pair","protocol":VERSIONS.0,"stream":VERSIONS.1,
                "operation":VERSIONS.2,"name":"Stranger","nonce":hex(&nonce())}),
        )
        .await;
        assert_eq!(next(&mut p.socket).await["type"], "pairProof");
        send(&mut p.socket, attempt.clone()).await;
        let answer = next(&mut p.socket).await;
        assert!(
            answer["closed"].is_string(),
            "{attempt} was answered: {answer}"
        );
        task.await.unwrap();
    }
    // The window takes an unknown key into TLS, and only into a pairing: a hello from it, as its
    // first word, is refused like any unpaired key's.
    let (task, socket) = dial(&s, &stranger.key, "192.168.1.76:50000", stop.clone()).await;
    let answered = hello(
        &mut socket.expect("inside the window, TLS takes it"),
        VERSIONS,
    )
    .await;
    assert!(
        answered["closed"].is_string(),
        "an unpaired key was welcomed: {answered}"
    );
    task.await.unwrap();
    assert_eq!(
        s.shared.authority.local_status()["controller"],
        Value::Null,
        "a lease from a pairing connection"
    );
    assert!(
        !controls(&s, &stranger.device),
        "a grant from a pairing connection"
    );
    assert!(book.paired(&stranger.pin).is_none());
    assert!(
        book.pairing(Instant::now()),
        "premise: none of it spent the code"
    );
    assert!(
        s.shared.authority.claim_stream().is_some(),
        "the stream slot was taken"
    );
    assert!(!s.shared.engine.lock().unwrap().manual_ptt());

    // Paired on this one: the same connection still reaches nothing.
    let (_task, p) = dial_pairing(&s, &stranger.key, "192.168.1.79:50000", stop.clone()).await;
    let mut p = p.unwrap();
    assert_eq!(
        prove_code(&mut p, &stranger, &code, "Stranger").await["type"],
        "paired"
    );
    send(
        &mut p.socket,
        json!({"type":"operationRequest","request":{"type":"state","requestId":id()}}),
    )
    .await;
    assert!(next(&mut p.socket).await["closed"].is_string());
    assert_eq!(s.shared.authority.local_status()["controller"], Value::Null);

    let (_socket, _, acquired) = acquire_as_paired(&s, &stranger, "192.168.1.80:50000", stop).await;
    assert_eq!(
        acquired["value"]["phase"], "controlling",
        "control: {acquired}"
    );
}

// ----- The book in the credential store -----

/// ★ The key is made once, kept, and read back: the store holds it only in its own entry, and
/// the shack's status shows its fingerprint and never the key. A store that will not answer makes
/// none and writes nothing. CONTROL: a second read gives the same key, and a reset another one.
#[test]
fn the_key_is_made_once_and_kept() {
    let store = LanStore::default();
    let first = book_on(&store);
    let (identity, _) = first.identity().expect("a key is made");
    let public = identity.public_key().to_string();
    let kept: LanKey = serde_json::from_str(store.key.lock().unwrap().as_deref().unwrap()).unwrap();
    assert_eq!(identity.station_id, kept.station_id);
    let again = book_on(&store);
    let (read, generation) = again.identity().unwrap();
    assert_eq!(read.public_key(), public, "control: the same key");
    assert_eq!(read.station_id, kept.station_id);
    let fingerprint = hex(ring::digest::digest(
        &ring::digest::SHA256,
        &tempo_stream::protocol::hex_bytes(&public).unwrap(),
    )
    .as_ref());
    assert_eq!(again.view(Instant::now()).key, Some(fingerprint));
    let status = serde_json::to_string(&LanStatus {
        key: again.view(Instant::now()).key,
        devices: again.view(Instant::now()).devices,
        ..LanStatus::default()
    })
    .unwrap();
    assert!(!status.contains(&kept.pkcs8), "the key reached the status");
    assert!(
        store
            .devices
            .lock()
            .unwrap()
            .as_deref()
            .is_none_or(|d| !d.contains(&kept.pkcs8)),
        "the key reached the computers' entry"
    );

    let locked = LanStore::default();
    locked.locked.store(true, Ordering::SeqCst);
    assert!(
        book_on(&locked).identity().is_none(),
        "made a key it could not keep"
    );
    assert_eq!(locked.writes.load(Ordering::SeqCst), 0);

    again.renew().unwrap();
    let (renewed, later) = again.identity().unwrap();
    assert_ne!(renewed.public_key(), public, "a reset kept the key");
    assert_ne!(later, generation);
}

/// ★ Eight computers fit one credential entry, whatever names they gave (32 characters each, of
/// the widest kinds), and come back from the store as they were paired: the same ids, names and
/// fingerprints. A ninth is refused, and so is a window with eight paired. CONTROL: the store's
/// limit is real: a record over 2560 bytes of UTF-16 is refused by it.
#[test]
fn eight_computers_fit_the_credential_store_and_round_trip() {
    let store = LanStore::default();
    let book = book_on(&store);
    let (identity, _) = book.identity().unwrap();
    let widest = ["\u{1F4E1}".repeat(32), "\"".repeat(32), "\\".repeat(32)];
    let mut paired = Vec::new();
    for n in 0..MAX_PAIRED {
        let computer = Computer::new();
        let name = widest[n % widest.len()].clone();
        assert_eq!(
            book.add(computer.pin, &name, &identity.station_id),
            Ok(computer.device.clone()),
            "computer {n}"
        );
        paired.push((computer, name));
    }
    let value = store.devices.lock().unwrap().clone().unwrap();
    let bytes = value.encode_utf16().count() * 2;
    assert!(bytes <= 2560, "{bytes} bytes");
    let read = book_on(&store);
    for (computer, name) in &paired {
        assert_eq!(read.paired(&computer.pin), Some(computer.device.clone()));
        assert!(read
            .view(Instant::now())
            .devices
            .iter()
            .any(|d| d.id == computer.device && &d.name == name && d.key == hex(&computer.pin)));
    }
    assert_eq!(
        book.add(Computer::new().pin, "Ninth", &identity.station_id),
        Err("pairingFull")
    );
    assert_eq!(book.open(Instant::now(), 0), Err("lanFull"));

    let over = LanDevices {
        station_id: STATION.into(),
        devices: (0..12)
            .map(|_| LanDevice {
                pin: hex(&Computer::new().pin),
                name: widest[0].clone(),
            })
            .collect(),
    };
    assert!(store.save_lan_devices(&over).is_err(), "control: no limit");
}

/// A list the store holds for another key, or one out of shape, admits nobody; a store that will
/// not give the list up admits nobody and opens no window. CONTROL: the list for this key admits
/// its computer.
#[test]
fn a_list_that_is_not_this_keys_admits_nobody() {
    let computer = Computer::new();
    let key = fixture_key();
    let ours = LanStore::holding(&key, STATION, &[(&computer.pin, "Laptop")]);
    assert_eq!(
        book_on(&ours).paired(&computer.pin),
        Some(computer.device.clone()),
        "control"
    );
    let listed = |station: &str, pin: String| {
        let store = LanStore::holding(&key, STATION, &[]);
        store
            .save_lan_devices(&LanDevices {
                station_id: station.into(),
                devices: vec![LanDevice {
                    pin,
                    name: "Laptop".into(),
                }],
            })
            .unwrap();
        book_on(&store).paired(&computer.pin)
    };
    assert_eq!(
        listed("70000000-0000-4000-8000-000000000001", hex(&computer.pin)),
        None,
        "another key's list"
    );
    assert_eq!(
        listed(STATION, hex(&computer.pin).to_uppercase()),
        None,
        "a pin out of shape"
    );
    assert_eq!(
        listed(STATION, hex(&computer.pin)),
        Some(computer.device.clone()),
        "control"
    );

    let shut = LanStore::holding(&key, STATION, &[(&computer.pin, "Laptop")]);
    shut.locked.store(true, Ordering::SeqCst);
    let book = book_on(&shut);
    assert_eq!(book.paired(&computer.pin), None);
    assert_eq!(book.open(Instant::now(), 0), Err("noKey"));
}

/// ★ The two LAN entries read past a field a newer Nexus may have added: a key record with one more
/// field keeps its key and its computer, and a list with one more field, on the list and on its
/// computer, admits that computer, and nothing is written. CONTROL: the same records as this build
/// writes them read the same.
#[test]
fn the_lan_entries_read_past_a_field_a_newer_nexus_added() {
    let computer = Computer::new();
    let key = fixture_key();
    let public = tls::Identity::new(&key, STATION.into())
        .unwrap()
        .public_key()
        .to_string();
    let store = LanStore::holding(&key, STATION, &[(&computer.pin, "Laptop")]);
    let book = book_on(&store);
    assert_eq!(book.identity().unwrap().0.public_key(), public, "control");
    assert_eq!(book.paired(&computer.pin), Some(computer.device.clone()));
    let grow = |slot: &Mutex<Option<String>>, grown: &dyn Fn(&mut Value)| {
        let mut value: Value =
            serde_json::from_str(slot.lock().unwrap().as_deref().unwrap()).unwrap();
        grown(&mut value);
        *slot.lock().unwrap() = Some(value.to_string());
    };
    grow(&store.key, &|key| {
        key["createdAt"] = json!(1_759_536_000_000_u64)
    });
    grow(&store.devices, &|list| {
        list["revision"] = json!(2);
        list["devices"][0]["l"] = json!("2026-10-04");
    });
    let book = book_on(&store);
    assert_eq!(
        book.identity().unwrap().0.public_key(),
        public,
        "a key record with one more field was taken for none"
    );
    assert_eq!(
        book.paired(&computer.pin),
        Some(computer.device.clone()),
        "a list with one more field admitted nobody"
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), 0, "written over");
}

/// ★ A LAN entry that does not read is never taken for none, and nothing is made or written over
/// it. A key record that is not JSON, not this shape, or not a key leaves the station with no key,
/// so the listener says `noKey`, and the paired list stays as it was. A list that does not read,
/// or is out of shape, admits nobody, opens no window, keeps no pairing and is not written over.
/// Resetting the network identity at the shack is what replaces them. CONTROL: a store that holds
/// no key at all still gets one.
#[test]
fn a_lan_entry_that_does_not_read_is_kept_as_it_is() {
    let computer = Computer::new();
    let key = fixture_key();
    let not_a_key = json!({"stationId": STATION, "pkcs8": "00"}).to_string();
    for unread in ["not json", r#"{"station":"x"}"#, not_a_key.as_str()] {
        let store = LanStore::holding(&key, STATION, &[(&computer.pin, "Laptop")]);
        let list = store.devices.lock().unwrap().clone();
        *store.key.lock().unwrap() = Some(unread.into());
        let book = book_on(&store);
        assert!(book.identity().is_none(), "a key was made over {unread}");
        assert_eq!(store.key.lock().unwrap().as_deref(), Some(unread));
        assert_eq!(
            *store.devices.lock().unwrap(),
            list,
            "the list went: {unread}"
        );
        assert_eq!(store.writes.load(Ordering::SeqCst), 0, "{unread}");
        assert_eq!(book.open(Instant::now(), 0), Err("noKey"));
    }
    let pin = hex(&computer.pin);
    let twice = json!({"stationId": STATION, "devices": [{"p": pin, "n": "Laptop"},
        {"p": pin, "n": "Laptop"}]})
    .to_string();
    for unread in ["not json", twice.as_str()] {
        let store = LanStore::holding(&key, STATION, &[(&computer.pin, "Laptop")]);
        *store.devices.lock().unwrap() = Some(unread.into());
        let book = book_on(&store);
        assert!(book.identity().is_some(), "the key reads: {unread}");
        assert_eq!(book.paired(&computer.pin), None);
        assert_eq!(
            book.open(Instant::now(), 0),
            Err("credentialStoreUnavailable"),
            "{unread}"
        );
        assert_eq!(
            book.add(Computer::new().pin, "Den PC", STATION),
            Err("unavailable"),
            "{unread}"
        );
        assert_eq!(store.devices.lock().unwrap().as_deref(), Some(unread));
        assert_eq!(store.writes.load(Ordering::SeqCst), 0, "{unread}");
    }
    assert!(
        book_on(&LanStore::default()).identity().is_some(),
        "the control: no key made for a store that holds none"
    );
}

// ----- Removing a computer, and resetting the key -----

/// ★ Removing a computer at the shack ends it at once: its session closes, its over halts, its
/// station control is gone, its record leaves the credential store, and its next connection is
/// refused. CONTROL: another paired computer keeps its pairing and its control, and before the
/// removal the computer was in control and on the air.
#[tokio::test]
async fn removing_a_computer_ends_it_at_once() {
    let s = shack(home());
    let scratch = Scratch::new();
    let lan = lan_on(&s, &scratch);
    let other = Computer::new();
    s.shared
        .desk
        .book
        .add(other.pin, "Other", &s.shared.desk.station_id)
        .unwrap();
    let (_stop, stop) = watch::channel(false);
    let (mut socket, session, acquired) =
        acquire_as_paired(&s, &s.computer, PEER, stop.clone()).await;
    assert_eq!(
        acquired["value"]["phase"], "controlling",
        "control: {acquired}"
    );
    keyed_under_presence(&s, &session, &acquired["value"]);
    assert!(!halted(&s), "premise: on the air");

    lan.revoke(&s.device).unwrap();
    assert!(
        next(&mut socket).await["closed"].is_string(),
        "the session outlived the removal"
    );
    assert!(halted(&s), "the over outlived the removal");
    assert!(!controls(&s, &s.device), "control outlived the removal");
    assert_eq!(s.shared.desk.book.paired(&s.computer.pin), None);
    assert_eq!(
        book_on(&s.store).paired(&s.computer.pin),
        None,
        "still in the store"
    );
    let (_task, again) = dial(&s, &s.computer.key, "192.168.1.90:50000", stop).await;
    let again = match again {
        Ok(mut socket) => hello(&mut socket, VERSIONS).await,
        Err(e) => json!({ "refused": e }),
    };
    assert!(
        again["type"] != "welcome",
        "welcomed after its removal: {again}"
    );

    assert_eq!(
        book_on(&s.store).paired(&other.pin),
        Some(other.device.clone()),
        "control: the other left the store too"
    );
    assert!(
        controls(&s, &other.device),
        "control: the other lost control"
    );
    assert_eq!(
        lan.revoke(&s.device),
        Err("invalidRequest"),
        "removed twice"
    );
}

/// ★ Resetting the network identity: every paired computer out at once (its session closed, its
/// control gone), a new key in the credential store with nobody paired to it, and the new
/// fingerprint shown in place of the old. CONTROL: before the reset the computer was in control.
#[tokio::test]
async fn resetting_the_identity_pairs_nobody_with_a_new_key() {
    let s = shack(home());
    let scratch = Scratch::new();
    let lan = lan_on(&s, &scratch);
    let book = s.shared.desk.book.clone();
    let before = book.view(Instant::now()).key.unwrap();
    let (_stop, stop) = watch::channel(false);
    let (mut socket, _, acquired) = acquire_as_paired(&s, &s.computer, PEER, stop).await;
    assert_eq!(
        acquired["value"]["phase"], "controlling",
        "control: {acquired}"
    );

    lan.reset().unwrap();
    assert!(
        next(&mut socket).await["closed"].is_string(),
        "the session outlived the reset"
    );
    assert!(!controls(&s, &s.device), "control outlived the reset");
    let after = book.view(Instant::now());
    assert_ne!(after.key.as_deref(), Some(before.as_str()), "the same key");
    assert!(after.devices.is_empty());
    let kept = book_on(&s.store);
    assert_eq!(
        kept.view(Instant::now()).key,
        after.key,
        "not the stored key"
    );
    assert_eq!(kept.paired(&s.computer.pin), None);
    assert_eq!(lan.status().key, after.key, "the old fingerprint shown");
}

// ----- Restarts -----

/// ★ After a restart the paired computers come back from the credential store with their station
/// control, LAN on as it was left: a computer takes control with no grant given again. CONTROL: a
/// computer removed before the restart does not come back, and with LAN left off nobody holds
/// control though the computer is still paired.
#[tokio::test]
async fn paired_computers_come_back_after_a_restart() {
    let first = shack(home());
    let scratch = Scratch::new();
    let lan = lan_on(&first, &scratch);
    let removed = Computer::new();
    first
        .shared
        .desk
        .book
        .add(removed.pin, "Removed", &first.shared.desk.station_id)
        .unwrap();
    lan.revoke(&removed.device).unwrap();
    drop(lan);

    // Nexus starts again: a new authority and engine, the book read from the same store, the
    // switch from the same file.
    let s = shack_with(
        home(),
        first.station_key.clone(),
        first.computer.clone(),
        first.store.clone(),
    );
    let lan = switch_for(
        &s,
        &scratch,
        Arc::new(|_| only(Err(NoNetwork::Choose))),
        s.shared.desk.book.clone(),
    );
    eventually(&lan, "LAN came back off", |st| st.on).await;
    let (_stop, stop) = watch::channel(false);
    let (_socket, _, acquired) = acquire_as_paired(&s, &s.computer, PEER, stop).await;
    assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
    assert!(
        !controls(&s, &removed.device),
        "a removed computer came back"
    );
    assert_eq!(s.shared.desk.book.paired(&removed.pin), None);

    lan.turn_off();
    drop(lan);
    let off = shack_with(
        home(),
        first.station_key.clone(),
        first.computer.clone(),
        first.store.clone(),
    );
    let lan = switch_for(
        &off,
        &scratch,
        Arc::new(|_| only(Err(NoNetwork::Choose))),
        off.shared.desk.book.clone(),
    );
    eventually(&lan, "LAN came back on", |st| !st.on).await;
    assert_eq!(
        off.shared.desk.book.paired(&first.computer.pin),
        Some(first.device.clone()),
        "premise: still paired"
    );
    assert!(!controls(&off, &off.device), "control with LAN off");
}

// ----- Over the listener itself -----

/// ★ Over the listener, on this box's own private address: Pair a computer at the shack shows a
/// code and the key's fingerprint, the computer pairs with that code over the port (the key it was
/// shown is the one presented), then connects as itself with that key pinned. A reset ends its
/// session and restarts the listener on a new key, which the computer, pinned to the old one,
/// refuses. CONTROL: before the reset it was welcomed. Skipped, saying so, on a box with no private
/// address of its own.
#[tokio::test]
async fn pairing_over_the_listener_and_a_reset_restarting_it_on_a_new_key() {
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
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
        s.shared.desk.book.clone(),
    );
    assert_eq!(lan.pair(), Err("invalidRequest"), "a code with LAN off");
    lan.turn_on(None, Some(port)).unwrap();
    let at = SocketAddr::new(address.into(), port);
    eventually(&lan, "never listened", |st| {
        st.listening.as_deref() == Some(at.to_string().as_str())
    })
    .await;
    lan.pair().unwrap();
    let shown = lan.status();
    let code: [u8; 8] = tempo_stream::protocol::hex_bytes(&shown.pairing.as_ref().unwrap().code)
        .unwrap()
        .try_into()
        .unwrap();
    let newcomer = Computer::new();
    let mut p = open_pairing(
        tokio::net::TcpStream::connect(at).await.unwrap(),
        &newcomer.key,
    )
    .await
    .unwrap();
    assert_eq!(
        Some(hex(
            ring::digest::digest(&ring::digest::SHA256, &p.shack).as_ref()
        )),
        shown.key,
        "the key presented is not the one the shack shows"
    );
    assert_eq!(
        prove_code(&mut p, &newcomer, &code, "Den PC").await["type"],
        "paired"
    );
    assert!(
        lan.status().pairing.is_none(),
        "the window outlived its pairing"
    );

    let pinned = hex(&p.shack);
    let mut socket = open(
        tokio::net::TcpStream::connect(at).await.unwrap(),
        &newcomer.key,
        &pinned,
    )
    .await
    .unwrap();
    let welcome = hello(&mut socket, VERSIONS).await;
    assert_eq!(welcome["deviceId"], newcomer.device, "control: {welcome}");

    lan.reset().unwrap();
    assert!(
        next(&mut socket).await["closed"].is_string(),
        "the session outlived the reset"
    );
    let before = shown.key.clone();
    let after = eventually(&lan, "the book kept the old key", |st| st.key != before).await;
    assert!(after.devices.is_empty());
    // The listener restarts on the new key. With a window open, any key gets as far as the key
    // the port presents: wait until it is the new one the shack shows.
    lan.pair().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let presented = match open_pairing(up(at).await, &Computer::new().key).await {
            Ok(p) => Some(hex(
                ring::digest::digest(&ring::digest::SHA256, &p.shack).as_ref()
            )),
            Err(_) => None,
        };
        if presented.is_some() && presented == after.key {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the listener never presented the key the shack shows: {presented:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        open(up(at).await, &newcomer.key, &pinned).await.is_err(),
        "a computer pinned to the old key took the new one"
    );
}

/// A connection to `at` once something listens there, within 5 s.
async fn up(at: SocketAddr) -> tokio::net::TcpStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::net::TcpStream::connect(at).await {
            Ok(stream) => return stream,
            Err(e) => assert!(Instant::now() < deadline, "nothing listens at {at}: {e}"),
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
