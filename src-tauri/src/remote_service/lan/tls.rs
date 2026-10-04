//! The LAN road's TLS: TLS 1.3 only, with raw public keys both ways (RFC 7250), each end pinned to
//! the other. No certificate, no authority and no name: the shack presents its LAN key, a remote
//! PC presents the key it made for this shack, and the shack takes a client only when that key is
//! one a paired computer holds. rustls with ring, both already Nexus's (the relay socket and every
//! HTTPS client run on them), so nothing new is linked.
//!
//! Refused before a byte of application data is read:
//! - any version but TLS 1.3, and any client without a key;
//! - a certificate chain, or a key that is not a P-256 key;
//! - a key no paired computer holds ([`Paired`]), read at each handshake, so a computer revoked
//!   since the listener started is refused at its next connection;
//! - resumption and early data: no tickets and no session cache, so every connection is a full
//!   handshake that proves both keys again.
//!
//! **The second branch: an open pairing window ([`Pairing`]).** While it is open, a P-256 key no
//! paired computer holds is taken too, read at each handshake. Such a connection can only become a
//! pairing-only one (`super::pairing`): its first message must ask to pair, the session it gets
//! reaches nothing but the pairing, and a hello from it is refused as from any unpaired key.
//!
//! In TLS 1.3 the shack's key is proved before the client sends its own, so a machine posing as the
//! shack learns nothing about which key a computer holds.
use std::sync::Arc;

use ring::digest::{digest, SHA256};
use rustls::crypto::{verify_tls13_signature_with_raw_key, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, SubjectPublicKeyInfoDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::server::{AlwaysResolvesServerRawPublicKeys, NoServerSessionStorage, ServerConfig};
use rustls::sign::CertifiedKey;
use rustls::{
    CertificateError, DigitallySignedStruct, DistinguishedName, Error, PeerIncompatible,
    SignatureScheme,
};
use tempo_stream::protocol;

use super::super::station_key::Signer;

/// The one signature scheme either end may use: ECDSA over P-256 with SHA-256, the keys the
/// stream already signs with (A5, S3-M1).
const SCHEME: SignatureScheme = SignatureScheme::ECDSA_NISTP256_SHA256;

/// The paired computer that holds a key, by the SHA-256 of the key's SPKI: its device id. `None`
/// for a key no paired computer holds. Pairing keeps these; until it does, nothing is paired.
pub type Paired = Arc<dyn Fn(&[u8; 32]) -> Option<String> + Send + Sync>;

/// Whether the pairing window is open now: while it is, a key no paired computer holds is taken
/// into a pairing-only connection.
pub type Pairing = Arc<dyn Fn() -> bool + Send + Sync>;

/// SHA-256 of a key's SPKI: its pin, as the device records keep it.
pub(super) fn pin(spki: &[u8]) -> [u8; 32] {
    let mut out = [0; 32];
    out.copy_from_slice(digest(&SHA256, spki).as_ref());
    out
}

/// Is this the SPKI of a P-256 key, exactly as a device key is (`protocol::device_key`)?
fn p256(spki: &[u8]) -> bool {
    protocol::hex_bytes(protocol::P256_SPKI_PREFIX_HEX)
        .is_some_and(|prefix| spki.len() == prefix.len() + 65 && spki.starts_with(&prefix))
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The shack's LAN identity, ready to use: its key as TLS presents it, the signer its stream
/// answers carry, and the LAN station id a computer signs its offers over (A5).
///
/// Handled like the hosted station key (`station_key`): the private key is never logged, printed,
/// shown or serialised, and this type has no `Debug`, `Clone` or `Serialize`.
pub struct Identity {
    certified: Arc<CertifiedKey>,
    pub(super) signer: Arc<Signer>,
    pub(super) station_id: String,
}

impl Identity {
    /// The identity a LAN key's PKCS#8 document (lowercase hex) makes, or `None` for anything that
    /// is not a P-256 key: one key, both for TLS and for signing answers, so the key a computer
    /// pinned at the handshake is the key that signs what it is answered.
    pub fn new(pkcs8: &str, station_id: String) -> Option<Self> {
        let signer = Signer::restore(pkcs8)?;
        let document = protocol::hex_bytes(pkcs8)?;
        let key = provider()
            .key_provider
            .load_private_key(PrivateKeyDer::Pkcs8(document.into()))
            .ok()?;
        let spki = key.public_key()?.as_ref().to_vec();
        if !p256(&spki) || protocol::hex_bytes(signer.public_key())? != spki {
            return None;
        }
        Some(Self {
            certified: Arc::new(CertifiedKey::new(vec![CertificateDer::from(spki)], key)),
            signer: Arc::new(signer),
            station_id,
        })
    }

    /// The public half, SPKI as lowercase hex: what a remote PC pins at pairing.
    pub fn public_key(&self) -> &str {
        self.signer.public_key()
    }
}

/// The server half: TLS 1.3, the shack's raw key, a client key required and held to `paired` (or,
/// while `pairing` says the window is open, any P-256 key), and no way to resume or replay.
pub(super) fn server(
    identity: &Identity,
    paired: Paired,
    pairing: Pairing,
) -> Result<Arc<ServerConfig>, Error> {
    let provider = provider();
    let mut config = ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(Arc::new(PinnedClients {
            paired,
            pairing,
            provider,
        }))
        .with_cert_resolver(Arc::new(AlwaysResolvesServerRawPublicKeys::new(
            identity.certified.clone(),
        )));
    config.send_tls13_tickets = 0;
    config.session_storage = Arc::new(NoServerSessionStorage {});
    config.max_early_data_size = 0;
    Ok(Arc::new(config))
}

/// A client key is taken only when it is a P-256 key a paired computer holds, or, while the
/// pairing window is open, any P-256 key.
struct PinnedClients {
    paired: Paired,
    pairing: Pairing,
    provider: Arc<CryptoProvider>,
}

impl std::fmt::Debug for PinnedClients {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PinnedClients")
    }
}

impl ClientCertVerifier for PinnedClients {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        key: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        if !intermediates.is_empty() || !p256(key.as_ref()) {
            return Err(CertificateError::BadEncoding.into());
        }
        // The second branch: an unknown key gets as far as a pairing-only connection, and only
        // while the window is open.
        if (self.paired)(&pin(key.as_ref())).is_none() && !(self.pairing)() {
            return Err(CertificateError::ApplicationVerificationFailure.into());
        }
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _key: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, Error> {
        // Never reached: the server speaks TLS 1.3 only.
        Err(PeerIncompatible::Tls12NotOfferedOrEnabled.into())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        key: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, Error> {
        if dss.scheme != SCHEME {
            return Err(PeerIncompatible::NoSignatureSchemesInCommon.into());
        }
        verify_tls13_signature_with_raw_key(
            message,
            &SubjectPublicKeyInfoDer::from(key.as_ref()),
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SCHEME]
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

/// The remote PC's half (`crate::lan_client`): TLS 1.3, its own raw key for this shack, and the
/// shack's key pinned. No resumption and no early data, as on the shack's side, so every
/// connection proves both keys again.
pub(crate) mod client {
    use super::*;
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::client::{AlwaysResolvesClientRawPublicKeys, ClientConfig, Resumption};
    use rustls::pki_types::ServerName;

    /// The shack's key, pinned: anything else is refused before the client says a word (TLS 1.3
    /// proves the server's key before the client sends its own). `None` only for a pairing
    /// connection, which takes whatever P-256 key the shack presents and has the pairing's proofs
    /// bind it.
    #[derive(Debug)]
    struct PinnedShack {
        spki: Option<Vec<u8>>,
        provider: Arc<CryptoProvider>,
    }

    impl ServerCertVerifier for PinnedShack {
        fn verify_server_cert(
            &self,
            key: &CertificateDer<'_>,
            intermediates: &[CertificateDer<'_>],
            _name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            let pinned = match &self.spki {
                Some(spki) => key.as_ref() == spki.as_slice(),
                None => p256(key.as_ref()),
            };
            if intermediates.is_empty() && pinned {
                Ok(ServerCertVerified::assertion())
            } else {
                Err(CertificateError::ApplicationVerificationFailure.into())
            }
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _key: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Err(PeerIncompatible::Tls12NotOfferedOrEnabled.into())
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            key: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            if dss.scheme != SCHEME {
                return Err(PeerIncompatible::NoSignatureSchemesInCommon.into());
            }
            verify_tls13_signature_with_raw_key(
                message,
                &SubjectPublicKeyInfoDer::from(key.as_ref()),
                dss,
                &self.provider.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![SCHEME]
        }

        fn requires_raw_public_keys(&self) -> bool {
            true
        }
    }

    /// A remote PC holding `key` (PKCS#8, lowercase hex), with the shack's `shack` key (SPKI,
    /// lowercase hex) pinned. `None` for a key or a pin that is not a P-256 one.
    pub fn config(key: &str, shack: &str) -> Option<Arc<ClientConfig>> {
        let pinned = protocol::hex_bytes(shack).filter(|spki| p256(spki))?;
        made(key, Some(pinned))
    }

    /// A remote PC holding `key`, pairing: no shack key pinned yet, so it takes the one presented,
    /// for this one connection only.
    pub fn pairing(key: &str) -> Option<Arc<ClientConfig>> {
        made(key, None)
    }

    fn made(key: &str, shack: Option<Vec<u8>>) -> Option<Arc<ClientConfig>> {
        let provider = provider();
        let signing = provider
            .key_provider
            .load_private_key(PrivateKeyDer::Pkcs8(protocol::hex_bytes(key)?.into()))
            .ok()?;
        let spki = signing.public_key()?.as_ref().to_vec();
        if !p256(&spki) {
            return None;
        }
        let certified = Arc::new(CertifiedKey::new(vec![CertificateDer::from(spki)], signing));
        let mut config = ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .ok()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedShack {
                spki: shack,
                provider,
            }))
            .with_client_cert_resolver(Arc::new(AlwaysResolvesClientRawPublicKeys::new(certified)));
        config.resumption = Resumption::disabled();
        config.enable_early_data = false;
        Some(Arc::new(config))
    }
}
