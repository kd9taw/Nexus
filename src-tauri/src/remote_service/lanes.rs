//! The relay's older lanes, held to the approved browser's own key (security review S1-M1,
//! 2026-10-03).
//!
//! The relay stamps `sessionId` and `deviceId` on every message it hands the station, so on its word
//! alone a relay that is not the one the operator trusts could act as any approved browser: take
//! station control, keep a lease (and an FT transmission with it) alive, work FT where it is granted,
//! write the log, listen. The stream's offer has carried the browser's own signature since A5. These
//! lanes now carry it too: every operation request but two, and every Listen, comes with a proof made
//! with the device key the operator pinned at the radio, and anything else is refused here, before
//! the operations authority or the audio lane sees it.
//!
//! **What a proof covers**: the lane's label ([`OPERATION`], [`LISTEN`]), SHA-256 of the message's own
//! bytes (the operation request exactly as the relay handed it on; Listen's [`listen_body`]), the
//! station's, device's and session's ids, and `seq`: a number the page counts up within its session
//! and never uses twice. A proof is good for that one message, from that browser, in that session,
//! to this station, once. [`Lanes`] keeps which numbers each session has spent.
//!
//! **Two requests need none, on purpose.** `stopTransmit`: a forged Stop only stops. `state`: it
//! grants nothing, and it is how a page learns what Stop is composed from, so Stop never waits on a
//! key. A stream's own data channel carries neither proofs nor the relay's stamp: DTLS bound it to
//! the browser's key when it was admitted.
//!
//! **Refusals.** A message with no proof is refused in words every page already reads
//! (`stationUnsupported`, `audioUnavailable`): only a page from before proofs, or a relay, sends one.
//! A proof that fails is refused by name (`deviceNotPinned`, `deviceKeyMismatch`), which only a page
//! that signs is ever told. A number already spent is not answered at all: only a replay sends one.
//! Nothing a relay forges here can fail open, and nothing here holds up Stop.
use ring::digest::{digest, SHA256};
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};
use serde::Deserialize;
use tempo_stream::protocol;

/// What an operation request's proof begins with.
pub const OPERATION: &[u8] = b"nexus-operation/1";
/// What a Listen's proof begins with.
pub const LISTEN: &[u8] = b"nexus-listen/1";
/// The largest number a page can count to (JavaScript's safe integer).
const MAX_SEQ: u64 = 9_007_199_254_740_991;
/// How far behind a session's highest number another may still arrive: the relay can deliver a
/// command after a heartbeat sent behind it (it awaits the database for one), never by more.
const WINDOW: u64 = 64;
/// The most sessions whose spent numbers one connection remembers. The relay admits a handful.
const SESSIONS: usize = 64;

/// A browser's proof on one lane message, as the relay hands it on.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Proof {
    /// The browser's device key: SPKI, lowercase hex.
    public_key: String,
    seq: u64,
    /// ECDSA P-256 over SHA-256 of [`binding`], IEEE P1363 `r‖s`, lowercase hex.
    signature: String,
}

/// Why a lane message was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// No proof at all.
    Unsigned,
    /// The operator pinned no key for this browser: approved before keys existed, so approved again.
    NotPinned,
    /// Not the pinned key, or not a signature over this message, browser, session and station.
    Mismatch,
    /// A number this session has already spent.
    Replayed,
}

impl Refused {
    /// The operation lane's answer: in words every page already reads when no proof came, by name
    /// when one failed, and none at all for a replay.
    pub fn operation_error(self) -> Option<&'static str> {
        match self {
            Self::Unsigned => Some("stationUnsupported"),
            Self::NotPinned => Some("deviceNotPinned"),
            Self::Mismatch => Some("deviceKeyMismatch"),
            Self::Replayed => None,
        }
    }
    /// Listen's answer, on the same terms.
    pub fn listen_reason(self) -> Option<&'static str> {
        match self {
            Self::Unsigned => Some("audioUnavailable"),
            Self::NotPinned => Some("deviceNotPinned"),
            Self::Mismatch => Some("deviceKeyMismatch"),
            Self::Replayed => None,
        }
    }
}

/// The bytes a proof's signature covers.
pub fn binding(
    label: &[u8],
    body: &[u8],
    station: &str,
    device: &str,
    session: &str,
    seq: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(label.len() + 32 + 3 * 36 + 8);
    bytes.extend_from_slice(label);
    bytes.extend_from_slice(digest(&SHA256, body).as_ref());
    for id in [station, device, session] {
        bytes.extend_from_slice(id.as_bytes());
    }
    bytes.extend_from_slice(&seq.to_be_bytes());
    bytes
}

/// What a Listen's proof covers: what it asks, and under which lease, in one fixed spelling.
pub fn listen_body(listening: bool, lease: &str) -> String {
    format!(r#"{{"listening":{listening},"leaseId":"{lease}"}}"#)
}

/// The numbers one session has spent: the highest, and which of the [`WINDOW`] below it.
#[derive(Default)]
struct Spent {
    top: u64,
    below: u64,
}

impl Spent {
    fn fresh(&self, seq: u64) -> bool {
        seq > self.top || (self.top - seq < WINDOW && self.below & (1 << (self.top - seq)) == 0)
    }
    fn spend(&mut self, seq: u64) {
        if seq > self.top {
            let gap = seq - self.top;
            self.below = if gap >= WINDOW { 0 } else { self.below << gap };
            self.below |= 1;
            self.top = seq;
        } else {
            self.below |= 1 << (self.top - seq);
        }
    }
}

/// The checks for one relay connection: this station's id, the keys the operator pinned (read at
/// each message, so a pin or revoke made since the socket opened counts), and the numbers spent.
pub struct Lanes {
    station: String,
    pinned: super::stream::PinnedKeys,
    spent: Vec<(String, Spent)>,
}

impl Lanes {
    pub fn new(station: String, pinned: super::stream::PinnedKeys) -> Self {
        Self {
            station,
            pinned,
            spent: Vec::new(),
        }
    }

    /// Is `body`, on the lane `label`, the pinned key's own word for `device` in `session`? A
    /// message that passes has spent its number.
    pub fn check(
        &mut self,
        label: &[u8],
        (session, device): (&str, &str),
        body: &[u8],
        proof: Option<&Proof>,
    ) -> Result<(), Refused> {
        let proof = proof.ok_or(Refused::Unsigned)?;
        let pinned = (self.pinned)(device).ok_or(Refused::NotPinned)?;
        if !protocol::identifier(&self.station)
            || !protocol::device_key(&proof.public_key)
            || !(1..=MAX_SEQ).contains(&proof.seq)
            || proof.signature.len() != protocol::SIGNATURE_HEX_CHARS
        {
            return Err(Refused::Mismatch);
        }
        let key = protocol::hex_bytes(&proof.public_key).ok_or(Refused::Mismatch)?;
        let signature = protocol::hex_bytes(&proof.signature).ok_or(Refused::Mismatch)?;
        if digest(&SHA256, &key).as_ref() != pinned {
            return Err(Refused::Mismatch);
        }
        let signed = binding(label, body, &self.station, device, session, proof.seq);
        // An SPKI ends with the uncompressed point, which is what ring verifies against.
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &key[key.len() - 65..])
            .verify(&signed, &signature)
            .map_err(|_| Refused::Mismatch)?;
        let at = match self.spent.iter().position(|(s, _)| s == session) {
            Some(at) => at,
            None => {
                if self.spent.len() >= SESSIONS {
                    self.spent.remove(0);
                }
                self.spent.push((session.to_owned(), Spent::default()));
                self.spent.len() - 1
            }
        };
        let spent = &mut self.spent[at].1;
        if !spent.fresh(proof.seq) {
            return Err(Refused::Replayed);
        }
        spent.spend(proof.seq);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_is_spent_once_and_a_late_one_is_taken_within_the_window() {
        let mut spent = Spent::default();
        for seq in [1, 2, 5] {
            assert!(spent.fresh(seq));
            spent.spend(seq);
            assert!(!spent.fresh(seq), "{seq} spent");
        }
        // 3 and 4 were skipped: each is still good once, behind 5.
        assert!(spent.fresh(3));
        spent.spend(3);
        assert!(!spent.fresh(3));
        assert!(spent.fresh(4));
        // A jump past the window forgets what was below it, and refuses anything that old.
        spent.spend(5 + WINDOW);
        assert!(!spent.fresh(5 + WINDOW));
        assert!(!spent.fresh(5), "outside the window");
        assert!(spent.fresh(6), "inside it and never spent");
        assert!(!spent.fresh(4), "older than the window");
    }

    #[test]
    fn the_binding_is_the_label_the_body_digest_the_ids_and_the_number() {
        let (station, device, session) = (
            "30000000-0000-4000-8000-0000000000c1",
            "10000000-0000-4000-8000-0000000000c1",
            "20000000-0000-4000-8000-0000000000c1",
        );
        let bytes = binding(OPERATION, b"{}", station, device, session, 258);
        assert_eq!(bytes.len(), 17 + 32 + 108 + 8);
        assert_eq!(&bytes[..17], OPERATION);
        assert_eq!(&bytes[17..49], digest(&SHA256, b"{}").as_ref());
        assert_eq!(
            &bytes[49..157],
            format!("{station}{device}{session}").as_bytes()
        );
        assert_eq!(&bytes[157..], &[0, 0, 0, 0, 0, 0, 1, 2]);
        // Controls: the other lane, another body, another number are all other bytes.
        assert_ne!(bytes, binding(LISTEN, b"{}", station, device, session, 258));
        assert_ne!(
            bytes,
            binding(OPERATION, b"{ }", station, device, session, 258)
        );
        assert_ne!(
            bytes,
            binding(OPERATION, b"{}", station, device, session, 259)
        );
        assert_eq!(
            listen_body(true, session),
            format!(r#"{{"listening":true,"leaseId":"{session}"}}"#)
        );
    }
}
