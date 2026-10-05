//! The station's own signing key (security review S3-M1, 2026-10-03).
//!
//! The browser's device key (A5) proves to the station which approved browser is offering a stream.
//! Nothing proved the station to the browser: a relay that answered a page's offer itself became
//! "the station" for that browser, and was sent every key typed on the picture, every paste and the
//! microphone, and could aim the browser's ICE checks at the operator's own network. This key is the
//! other half.
//!
//! - **Made once for each pairing**, here, with ring (ECDSA P-256): when the operator approves the
//!   pairing at the radio, or, for a pairing made before the key existed, the first time Nexus
//!   starts with that pairing.
//! - **Kept the way Nexus keeps its other Remote secrets**: in the OS credential store, in an entry
//!   of its own beside the pairing's, bound to that pairing (`vault::StationKey`). The private key
//!   exists there and in this process, nowhere else: it is never logged, printed, shown or sent, and
//!   [`Signer`] has no `Debug`, `Clone` or `Serialize`.
//! - **Pinned in the pairing record at the service.** Its public half goes to the service before
//!   every connection (`native/key`). The service keeps the first key a station sends it and refuses
//!   any other by name (`stationKeyPinned`), which the shack shows. A page checks each answer against
//!   the key the service lists for its station, and refuses an answer without a good signature.
//! - **Kept by each page, and shown at both ends** (security review S3-L1). A page keeps the first
//!   key the service lists for the station and refuses the stream while the service lists another.
//!   The shack shows the SHA-256 of the public half as "This station's key" (`Status::station_key`),
//!   and the page shows the key it kept the same way, the first 128 bits in eight groups of four, so
//!   the operator can compare them.
//! - **Lost or rotated means pairing again.** A key that cannot be read back is replaced by a new
//!   one, which the service refuses. Revoke station access, then pair again: a new pairing is a new
//!   station with a new key. Nothing else changes it, so neither a relay nor anyone holding only the
//!   pairing's credential can put a key of their own in its place once one is recorded.
//! - **What it signs**: each stream answer, over `tempo_stream::protocol::answer_binding` (both DTLS
//!   fingerprints and the station, device and session ids), on one SDP line. Nothing else.
use ring::digest::{digest, SHA256};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use tempo_stream::protocol;

/// The station's key, ready to sign. See the module header for what may never happen to it.
pub struct Signer {
    pair: EcdsaKeyPair,
    /// The public half: SPKI DER, lowercase hex, the device key's shape (182 characters).
    public_key: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Signer {
    /// A new key, and its PKCS#8 document as lowercase hex for the vault entry: the one copy of the
    /// private key that ever leaves this type, and it goes only there.
    pub fn generate() -> Option<(Self, String)> {
        let pkcs8 =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
                .ok()?;
        let document = hex(pkcs8.as_ref());
        Some((Self::restore(&document)?, document))
    }

    /// The key a vault entry holds, or `None` for anything that is not one.
    pub fn restore(pkcs8: &str) -> Option<Self> {
        let pair = EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            &protocol::hex_bytes(pkcs8)?,
            &SystemRandom::new(),
        )
        .ok()?;
        let public_key = format!(
            "{}{}",
            protocol::P256_SPKI_PREFIX_HEX,
            hex(pair.public_key().as_ref())
        );
        Some(Self { pair, public_key })
    }

    /// The public half, SPKI as lowercase hex: what the service pins and a page verifies with.
    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    /// `answer`, signed for the negotiation it answers: `offer`, from `device`, in `session`, to this
    /// station. `None` when either side has no single SHA-256 fingerprint, which no page or station
    /// would take anyway.
    pub fn sign_answer(
        &self,
        offer: &str,
        answer: &str,
        station: &str,
        device: &str,
        session: &str,
    ) -> Option<String> {
        let hashed = |sdp: &str| -> Option<[u8; 32]> {
            digest(&SHA256, &protocol::offer_fingerprint(sdp)?)
                .as_ref()
                .try_into()
                .ok()
        };
        let signed =
            protocol::answer_binding(&hashed(answer)?, &hashed(offer)?, station, device, session);
        let signature = self.pair.sign(&SystemRandom::new(), &signed).ok()?;
        protocol::signed_answer(answer, &hex(signature.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};
    use serde_json::Value;

    const STATION: &str = "30000000-0000-4000-8000-0000000000b1";
    const DEVICE: &str = "10000000-0000-4000-8000-0000000000b1";
    const SESSION: &str = "20000000-0000-4000-8000-0000000000b1";

    /// The contract's recorded pair: the page's offer and the answer the station wrote for it.
    fn recorded() -> (String, String) {
        let file: Value = serde_json::from_str(include_str!(
            "../../../remote/test/fixtures/stream/station-answer.json"
        ))
        .unwrap();
        (
            file["offer"].as_str().unwrap().into(),
            file["answer"].as_str().unwrap().into(),
        )
    }

    /// Does the signature on `signed` hold for this negotiation, under `public_key`?
    fn holds(public_key: &str, signed: &str, offer: &str, ids: [&str; 3]) -> bool {
        let line = signed
            .lines()
            .find_map(|line| line.strip_prefix(protocol::ANSWER_SIGNATURE_ATTRIBUTE))
            .expect("the answer carries a signature line");
        let hashed = |sdp: &str| -> [u8; 32] {
            digest(&SHA256, &protocol::offer_fingerprint(sdp).unwrap())
                .as_ref()
                .try_into()
                .unwrap()
        };
        let bound =
            protocol::answer_binding(&hashed(signed), &hashed(offer), ids[0], ids[1], ids[2]);
        let spki = protocol::hex_bytes(public_key).unwrap();
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &spki[spki.len() - 65..])
            .verify(&bound, &protocol::hex_bytes(line).unwrap())
            .is_ok()
    }

    #[test]
    fn a_key_is_made_once_and_restored_from_its_vault_document() {
        let (made, document) = Signer::generate().unwrap();
        assert!(protocol::device_key(made.public_key()), "a P-256 SPKI");
        let restored = Signer::restore(&document).expect("the document is the key");
        assert_eq!(restored.public_key(), made.public_key());
        // Controls: another key is another public half; a document that is not one is no key.
        assert_ne!(
            Signer::generate().unwrap().0.public_key(),
            made.public_key()
        );
        assert!(Signer::restore("00").is_none());
        assert!(Signer::restore("not hex").is_none());
    }

    #[test]
    fn the_answer_is_signed_for_this_offer_session_device_and_station_only() {
        let (offer, answer) = recorded();
        let (key, _) = Signer::generate().unwrap();
        let signed = key
            .sign_answer(&offer, &answer, STATION, DEVICE, SESSION)
            .unwrap();
        let ids = [STATION, DEVICE, SESSION];
        assert!(holds(key.public_key(), &signed, &offer, ids));
        // Controls: none of it holds for another session, browser or station, another offer, or
        // under another key.
        let other = "40000000-0000-4000-8000-0000000000b1";
        assert!(!holds(
            key.public_key(),
            &signed,
            &offer,
            [STATION, DEVICE, other]
        ));
        assert!(!holds(
            key.public_key(),
            &signed,
            &offer,
            [STATION, other, SESSION]
        ));
        assert!(!holds(
            key.public_key(),
            &signed,
            &offer,
            [other, DEVICE, SESSION]
        ));
        assert!(!holds(key.public_key(), &signed, &answer, ids));
        assert!(!holds(
            Signer::generate().unwrap().0.public_key(),
            &signed,
            &offer,
            ids
        ));
        // The answer is otherwise the one str0m wrote.
        let line = signed
            .lines()
            .find(|line| line.starts_with(protocol::ANSWER_SIGNATURE_ATTRIBUTE))
            .unwrap();
        assert_eq!(signed.replacen(&format!("{line}\r\n"), "", 1), answer);
        // An offer with no single fingerprint is answered by no signature at all.
        let unfingerprinted = offer.replace("a=fingerprint:", "a=not-a-fingerprint:");
        assert_eq!(
            key.sign_answer(&unfingerprinted, &answer, STATION, DEVICE, SESSION),
            None
        );
    }
}
