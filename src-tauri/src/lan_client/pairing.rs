//! Pairing this computer with a station over its network: the computer's half of the shack's
//! ceremony (`remote_service::lan::pairing`, "One press" as ruled on 2026-10-04), with the shack's
//! own transcript, key and proof functions, so the two halves cannot drift apart.
//!
//! 1. A new key for this station ([`ComputerKey`]), made here and kept only if the pairing
//!    succeeds, unless this computer is paired with the station already (step 4).
//! 2. TLS 1.3 with that key and NO station key pinned: for this one connection, the computer takes
//!    the P-256 key the station presents (`tls::client::pairing`). The proofs bind it.
//! 3. `pair`, with the versions, this computer's name and a nonce. The station answers with its
//!    nonce and its proof, HMAC(k, "station" ‖ T).
//! 4. **The station's proof is checked before this computer says anything else.** A machine posing
//!    as the station cannot make it without the code, and learns nothing: this computer leaves
//!    without its own proof, and the station does not count that against the code. A station this
//!    computer is paired with already (the key it presented is the one pinned for it) is left the
//!    same way, and paired again on a second connection with this computer's own key for it,
//!    pinned to that station's key so the key is shown to it alone: the station then replaces its
//!    one entry for this computer instead of adding a second (the operator's ruling of
//!    2026-10-04, "Reuse the PC's own key"). The code is not spent by the first connection.
//! 5. This computer's proof, HMAC(k, "client" ‖ T); then `paired`, with the device id the station
//!    worked out from this computer's key and its LAN station id, or a refusal by name.
//!
//! The record (this computer's key, the station's key as it presented it, the two ids and the
//! address) is the caller's to keep (`super::Stations::keep`); nothing is kept for a pairing that
//! did not finish.
use std::net::SocketAddrV4;
use std::time::Duration;

use futures_util::StreamExt;
use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use super::road::{self, Failed, Opened, Socket};
use super::{hex, ComputerKey};
use crate::remote_service::lan::pairing::{code_key, hex32, holds, proof, transcript, Side};
use crate::remote_service::lan::{device_id, tls, valid_name, VERSIONS};
use crate::remote_service::vault::PairedStation;

/// The whole ceremony, from the connection to `paired`: the station's own deadlines (its handshake
/// and its pairing session) with room to spare.
const PAIRING: Duration = Duration::from_secs(15);

/// What the station says on a pairing connection. Parsed whole and exactly.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum FromStation {
    PairProof {
        nonce: String,
        proof: String,
    },
    Paired {
        #[serde(rename = "deviceId")]
        device_id: String,
        #[serde(rename = "stationId")]
        station_id: String,
    },
    Refused {
        reason: String,
    },
}

/// A station's refusal, as the page names it. The station's own `unavailable` (it could not keep
/// the pairing) is not this computer's: it is `stationUnavailable` here. Anything the station
/// never says means it is not one.
fn refusal(reason: &str) -> &'static str {
    match reason {
        "wrongCode" => "wrongCode",
        "pairingClosed" => "pairingClosed",
        "pairingFull" => "pairingFull",
        "unavailable" => "stationUnavailable",
        "updateStation" => "updateStation",
        "updateComputer" => "updateComputer",
        _ => "notStation",
    }
}

/// The station's next word on a pairing connection. Gone without one is `unreachable`: what a
/// station does when its pairing session runs out of time.
async fn heard(socket: &mut Socket) -> Result<FromStation, &'static str> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                return serde_json::from_str(&text).map_err(|_| "notStation");
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            _ => return Err("unreachable"),
        }
    }
}

/// Pair with the station at `at` with the code the operator typed, as `name`, knowing the
/// stations this computer is paired with already (`known`): the record to keep, or why not, by
/// name.
pub(crate) async fn pair(
    at: SocketAddrV4,
    code: [u8; 8],
    name: &str,
    known: &[PairedStation],
) -> Result<PairedStation, &'static str> {
    if !valid_name(name) {
        return Err("badName");
    }
    tokio::time::timeout(PAIRING, ceremony(at, code, name.trim(), known))
        .await
        .unwrap_or(Err("unreachable"))
}

/// How one pairing connection ended, when it did not end in a refusal.
enum Ceremony<'a> {
    Paired(PairedStation),
    /// The station is one this computer is paired with already: this record's.
    Known(&'a PairedStation),
}

async fn ceremony(
    at: SocketAddrV4,
    code: [u8; 8],
    name: &str,
    known: &[PairedStation],
) -> Result<PairedStation, &'static str> {
    let (key, pkcs8) = ComputerKey::generate().ok_or("unavailable")?;
    let config = tls::client::pairing(&pkcs8).ok_or("unavailable")?;
    let own = match with(at, code, name, &key, pkcs8, config, known).await? {
        Ceremony::Paired(record) => return Ok(record),
        Ceremony::Known(own) => own,
    };
    let key = ComputerKey::restore(&own.pkcs8).ok_or("unavailable")?;
    let config = tls::client::config(&own.pkcs8, &own.station_key).ok_or("unavailable")?;
    match with(at, code, name, &key, own.pkcs8.clone(), config, &[]).await? {
        // The same station by its id as well as its key.
        Ceremony::Paired(record) if record.station_id == own.station_id => Ok(record),
        _ => Err("notStation"),
    }
}

/// One pairing connection, with `key` (its PKCS#8 document `pkcs8`) on TLS `config`, knowing the
/// stations in `known`.
async fn with<'a>(
    at: SocketAddrV4,
    code: [u8; 8],
    name: &str,
    key: &ComputerKey,
    pkcs8: String,
    config: std::sync::Arc<rustls::ClientConfig>,
    known: &'a [PairedStation],
) -> Result<Ceremony<'a>, &'static str> {
    let computer = tempo_stream::protocol::hex_bytes(key.public_key()).ok_or("unavailable")?;
    let Opened {
        mut socket,
        station,
        exporter,
    } = road::open(at, config)
        .await
        .map_err(|failed| match failed {
            Failed::Unreachable => "unreachable",
            // Nothing answered at the address: why, in the words the card uses for it.
            Failed::NotConnected(how) => {
                tempo_stream::lan::unreached(&how.into(), tempo_stream::lan::here(*at.ip()))
            }
            // A station takes a key it does not know only while its pairing window is open.
            Failed::Refused => "pairingClosed",
            // On a pairing connection any P-256 key is taken: one that is not is no station's.
            Failed::KeyChanged => "notStation",
        })?;
    let mut ours = [0; 32];
    SystemRandom::new()
        .fill(&mut ours)
        .map_err(|_| "unavailable")?;
    let (protocol, stream, operation) = VERSIONS;
    let asked = json!({"type":"pair","protocol":protocol,"stream":stream,"operation":operation,
        "name":name,"nonce":hex(&ours)});
    road::say(&mut socket, asked.to_string())
        .await
        .map_err(|_| "unreachable")?;
    let (theirs, station_proof) = match heard(&mut socket).await? {
        FromStation::PairProof { nonce, proof } => (
            hex32(&nonce).ok_or("notStation")?,
            hex32(&proof).ok_or("notStation")?,
        ),
        FromStation::Refused { reason } => return Err(refusal(&reason)),
        FromStation::Paired { .. } => return Err("notStation"),
    };
    let t = transcript(&station, &computer, &ours, &theirs, &exporter);
    let k = code_key(&code).ok_or("unavailable")?;
    if !holds(&k, Side::Station, &t, &station_proof) {
        // Not the code the station holds, or not the station: leave without a word of ours.
        let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
        return Err("stationProofFailed");
    }
    // A station this computer is paired with already: left the same way (see the header).
    if let Some(own) = known.iter().find(|own| own.station_key == hex(&station)) {
        let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
        return Ok(Ceremony::Known(own));
    }
    let ours_proof = json!({"type":"pairProof","proof":hex(&proof(&k, Side::Computer, &t))});
    road::say(&mut socket, ours_proof.to_string())
        .await
        .map_err(|_| "unreachable")?;
    let (device, station_id) = match heard(&mut socket).await? {
        FromStation::Paired {
            device_id,
            station_id,
        } => (device_id, station_id),
        FromStation::Refused { reason } => return Err(refusal(&reason)),
        FromStation::PairProof { .. } => return Err("notStation"),
    };
    // The station works the device id out of this computer's key, as it does for every computer
    // it pairs: an id it did not work out so is not a station's answer.
    let pin: [u8; 32] = digest(&SHA256, &computer)
        .as_ref()
        .try_into()
        .map_err(|_| "unavailable")?;
    if device != device_id(&pin) || !tempo_stream::protocol::identifier(&station_id) {
        return Err("notStation");
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
    Ok(Ceremony::Paired(PairedStation {
        station_id,
        device_id: device,
        station_key: hex(&station),
        pkcs8,
        addresses: vec![at.to_string()],
    }))
}
