//! Pairing a computer with the shack over its own network (the operator's rulings of 2026-10-04,
//! "One press" and "Until revoked"). Making the code at the shack is the approval: a computer that
//! proves it knows the code is paired at once, with no second press, and stays paired until it is
//! removed at the shack.
//!
//! ## The ceremony
//!
//! 1. **At the shack**, Pair a computer opens the window (`Book::open`): a code of sixteen
//!    hexadecimal characters (64 random bits), good for ten minutes and one pairing, and closed by
//!    the third wrong proof.
//! 2. **The computer connects over TLS 1.3.** It has no key of the shack's to pin yet, so for this
//!    one connection it takes the one the shack presents. The shack takes a key no paired computer
//!    holds only while the window is open (`super::tls`), and only into this pairing-only
//!    connection.
//! 3. **Both prove the code** over a transcript T: SHA-256 over [`LABEL`], both keys as this TLS
//!    session presented them, a nonce from each side and this session's TLS exporter
//!    ([`EXPORTER`]) ([`transcript`]). With k from the code by HKDF-SHA256 ([`code_key`]), the
//!    shack sends HMAC(k, "station" ‖ T) first; the computer checks it before it says anything
//!    else, then sends HMAC(k, "client" ‖ T).
//! 4. **The shack checks the computer's proof** (`Book::check`). Right: the window closes for good,
//!    the computer's key is kept in the credential store and admitted (`Book::add`), and the shack
//!    tells it the device id it gave it. Wrong: it is told so and closed, and the third wrong proof
//!    closes the window.
//!
//! The computer then connects again, on a connection of its own with the shack's key pinned, as
//! any paired computer does.
//!
//! ## Why it holds (the design's security review, as ruled)
//!
//! - A machine in the middle shows each side a different key. Both proofs cover both keys and the
//!   TLS exporter, so it cannot pass them along: it would have to know the code.
//! - The code cannot be searched in its ten minutes: not offline, and not online in three tries.
//! - A machine posing as the shack learns nothing: it cannot make the shack's proof, and the
//!   computer sends its own only after checking that one.
//! - What one press gives up, knowingly: anyone who reads the code within its ten minutes is
//!   paired, with no second look at the shack.
//!
//! ## What a pairing-only connection can reach
//!
//! A [`Desk`]: the book with its window, and the shack's public key. No operations authority, no
//! engine and no stream, so it takes no lease, reads no state and keys nothing. It reads one
//! message, the proof, and closes after its answer, inside one deadline.
//!
//! ## The wire (WebSocket text frames of JSON, after TLS)
//!
//! - computer: `{"type":"pair","protocol":1,"stream":1,"operation":4,"name":"…","nonce":"<64 hex>"}`
//! - shack: `{"type":"pairProof","nonce":"<64 hex>","proof":"<64 hex>"}`
//! - computer: `{"type":"pairProof","proof":"<64 hex>"}`
//! - shack: `{"type":"paired","deviceId":"…","stationId":"…"}`, or `{"type":"refused","reason":…}`
//!   with `wrongCode`, `pairingClosed` (no window, or it has closed), `pairingFull`, `unavailable`
//!   (the credential store would not keep it), or the versions' `updateStation` and
//!   `updateComputer` (`super::channel`).
//!
//! Every hex value is lowercase. The name is the computer's own, up to 32 characters, shown at the
//! shack beside its key's fingerprint.
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use ring::digest::{Context, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use ring::{hkdf, hmac};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

use super::book::{valid_name, Book, Checked};
use super::channel::Socket;
use super::tls;

/// What the transcript and the code's key begin with.
pub const LABEL: &[u8] = b"nexus-lan-pairing/1";
/// The TLS exporter label: 32 bytes, no context, from each side of this one session.
pub const EXPORTER: &[u8] = b"EXPORTER-nexus-lan-pairing";

/// Which side a proof is from.
#[derive(Clone, Copy)]
pub enum Side {
    Station,
    Computer,
}

impl Side {
    fn label(self) -> &'static [u8] {
        match self {
            Side::Station => b"station",
            Side::Computer => b"client",
        }
    }
}

/// T: SHA-256 over [`LABEL`], the shack's key and the computer's (SPKI, each after its length),
/// the computer's nonce, the shack's, and this TLS session's exporter.
pub fn transcript(
    station: &[u8],
    computer: &[u8],
    computer_nonce: &[u8; 32],
    station_nonce: &[u8; 32],
    exporter: &[u8; 32],
) -> [u8; 32] {
    let mut hash = Context::new(&SHA256);
    hash.update(LABEL);
    for key in [station, computer] {
        hash.update(&(key.len() as u16).to_be_bytes());
        hash.update(key);
    }
    hash.update(computer_nonce);
    hash.update(station_nonce);
    hash.update(exporter);
    let mut out = [0; 32];
    out.copy_from_slice(hash.finish().as_ref());
    out
}

/// k: the key a code gives, by HKDF-SHA256 with [`LABEL`] as the salt.
pub fn code_key(code: &[u8; 8]) -> Option<hmac::Key> {
    hkdf::Salt::new(hkdf::HKDF_SHA256, LABEL)
        .extract(code)
        .expand(&[b"proof"], hmac::HMAC_SHA256)
        .ok()
        .map(hmac::Key::from)
}

/// One side's proof: HMAC-SHA256 under k over the side's label and T.
pub fn proof(key: &hmac::Key, side: Side, transcript: &[u8; 32]) -> [u8; 32] {
    let mut mac = hmac::Context::with_key(key);
    mac.update(side.label());
    mac.update(transcript);
    let mut out = [0; 32];
    out.copy_from_slice(mac.sign().as_ref());
    out
}

/// Is `proof` that side's proof over T? In constant time.
pub fn holds(key: &hmac::Key, side: Side, transcript: &[u8; 32], proof: &[u8]) -> bool {
    let mut message = side.label().to_vec();
    message.extend_from_slice(transcript);
    hmac::verify(key, &message, proof).is_ok()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Exactly 32 bytes, as 64 lowercase hexadecimal characters.
pub fn hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    tempo_stream::protocol::hex_bytes(text)?.try_into().ok()
}

/// All a pairing-only connection can reach: the book with its window, and the shack's own key.
#[derive(Clone)]
pub(super) struct Desk {
    pub book: Arc<Book>,
    /// The shack's key, SPKI, exactly as its TLS presents it.
    pub station: Arc<[u8]>,
    /// The LAN station id, which the computer keeps beside the shack's key.
    pub station_id: String,
}

impl Desk {
    /// The desk for a listener presenting `identity`.
    pub(super) fn new(book: Arc<Book>, identity: &tls::Identity) -> Option<Self> {
        Some(Self {
            book,
            station: tempo_stream::protocol::hex_bytes(identity.public_key())?.into(),
            station_id: identity.station_id.clone(),
        })
    }
}

/// What a pairing connection's first message asked, with what its TLS session proved.
pub(super) struct Asked {
    name: String,
    nonce: [u8; 32],
    /// The computer's key, SPKI, as this session presented it.
    key: Vec<u8>,
    exporter: [u8; 32],
}

impl Asked {
    /// `None` for a name or a nonce out of shape.
    pub(super) fn new(name: &str, nonce: &str, key: Vec<u8>, exporter: [u8; 32]) -> Option<Self> {
        if !valid_name(name) {
            return None;
        }
        Some(Self {
            name: name.trim().to_string(),
            nonce: hex32(nonce)?,
            key,
            exporter,
        })
    }
}

/// How a pairing connection ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    /// Paired, and told its device id.
    Paired,
    /// Not paired, through no fault of its own (no window, a full book, the store), or it left
    /// before proving anything, which is what a computer does when the shack's proof fails: not
    /// counted against its address.
    Turned,
    /// A wrong proof, or anything but a proof: counted against its address at the gate.
    Failed,
}

/// The only message a pairing-only connection acts on.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum FromComputer {
    PairProof { proof: String },
}

/// One pairing-only connection, from the shack's proof to its close, inside `deadline`.
pub(super) async fn session<S>(
    mut socket: Socket<S>,
    asked: Asked,
    desk: Desk,
    deadline: Duration,
    mut stop: watch::Receiver<bool>,
) -> Outcome
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let outcome = tokio::select! {
        biased;
        _ = stop.changed() => Outcome::Turned,
        done = tokio::time::timeout(deadline, prove(&mut socket, asked, &desk)) => {
            done.unwrap_or(Outcome::Failed)
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
    outcome
}

async fn say<S>(socket: &mut Socket<S>, value: serde_json::Value) -> bool
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .is_ok()
}

async fn refuse<S>(socket: &mut Socket<S>, reason: &str, outcome: Outcome) -> Outcome
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    say(socket, json!({"type":"refused","reason":reason})).await;
    outcome
}

async fn prove<S>(socket: &mut Socket<S>, asked: Asked, desk: &Desk) -> Outcome
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let Some((window, key)) = desk.book.proof_key(Instant::now()) else {
        return refuse(socket, "pairingClosed", Outcome::Turned).await;
    };
    let mut nonce = [0; 32];
    if SystemRandom::new().fill(&mut nonce).is_err() {
        return refuse(socket, "unavailable", Outcome::Turned).await;
    }
    let t = transcript(
        &desk.station,
        &asked.key,
        &asked.nonce,
        &nonce,
        &asked.exporter,
    );
    let ours = json!({"type":"pairProof","nonce":hex(&nonce),
        "proof":hex(&proof(&key, Side::Station, &t))});
    if !say(socket, ours).await {
        return Outcome::Turned;
    }
    let theirs = loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => break text,
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            // Gone without a proof: the shack's did not hold for it.
            _ => return Outcome::Turned,
        }
    };
    let Ok(FromComputer::PairProof { proof }) = serde_json::from_str(&theirs) else {
        return Outcome::Failed;
    };
    // A proof out of shape is a wrong one, and counts as one.
    let proof = hex32(&proof).map_or_else(Vec::new, |p| p.to_vec());
    match desk.book.check(window, &t, &proof, Instant::now()) {
        Checked::Closed => refuse(socket, "pairingClosed", Outcome::Turned).await,
        Checked::Wrong => refuse(socket, "wrongCode", Outcome::Failed).await,
        Checked::Right => {
            let (book, pin, name) = (desk.book.clone(), tls::pin(&asked.key), asked.name);
            let under = desk.station_id.clone();
            // The store can block: off this connection's thread.
            match tokio::task::spawn_blocking(move || book.add(pin, &name, &under)).await {
                Ok(Ok(device)) => {
                    say(
                        socket,
                        json!({"type":"paired","deviceId":device,"stationId":desk.station_id}),
                    )
                    .await;
                    Outcome::Paired
                }
                Ok(Err(reason)) => refuse(socket, reason, Outcome::Turned).await,
                Err(_) => refuse(socket, "unavailable", Outcome::Turned).await,
            }
        }
    }
}
