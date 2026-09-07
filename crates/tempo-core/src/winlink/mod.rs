//! Winlink B2F protocol — pure, offline, byte-oriented (spec Layer A).
//!
//! Everything under `winlink/` is protocol only: no sockets, no clock, no filesystem policy
//! beyond the mailbox blob store. The session engine is `b2f::Session::feed(&[u8]) -> Vec<Action>`,
//! the same shape as `tempo_net::cluster` / `tempo_net::aprsis`. **Lines are bytes, not `String`**
//! (see `tempo_net::aprsis`'s module header): FBB carries SOH/STX/EOT binary and LZHUF bodies, and
//! decoding the wire as UTF-8 would corrupt them inbound and — worse — outbound.
//!
//! This placement is forced rather than chosen: `tempo-net` is a leaf over `std` sockets with no
//! `tempo-core` dependency, while the app depends on both, so the protocol cannot live beside the
//! transports. The transports (CMS telnet, ARDOP) are `tempo-net`'s; nothing here can key a radio,
//! open a port, or read a clock.

pub mod b2f;
pub mod fbb;
pub mod lzhuf;
pub mod mailbox;
pub mod message;
pub mod secure;
pub mod sid;

/// Credentials and identity for a Winlink session.
///
/// **Deliberately derives neither `Debug` nor `Serialize`.** The Winlink password is a real
/// account secret, and both derives are ways it reaches somewhere it must never be: a derived
/// `Debug` puts it in every log line, panic message and `assert_eq!` failure that happens to
/// carry a config; a derived `Serialize` puts it in every settings blob, telemetry payload and
/// crash report. The hand-written [`Debug`] impl below redacts it, and there is no `Serialize` at
/// all — persistence is the OS keychain's job, not this struct's.
pub struct ClientConfig {
    /// The operator's callsign, used as the Winlink account name in the SID and login exchange.
    pub callsign: String,
    /// The Winlink account password. Never logged, never serialized; consumed only by
    /// [`secure::pr_response`](secure) to answer a `;PQ:` challenge.
    pub password: String,
}

impl std::fmt::Debug for ClientConfig {
    /// Redacts [`password`](ClientConfig::password). The marker is printed unconditionally — a
    /// blank or absent field would read as "no password set" and send someone debugging the wrong
    /// way, and printing its length would leak the one fact about it worth guessing from.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientConfig")
            .field("callsign", &self.callsign)
            .field("password", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_the_password() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "hunter2secret".into(),
        };
        let shown = format!("{cfg:?}");
        assert!(
            !shown.contains("hunter2secret"),
            "password leaked in Debug: {shown}"
        );
        assert!(
            shown.contains("N0CALL"),
            "callsign should still be visible: {shown}"
        );
        assert!(
            shown.contains("<redacted>"),
            "redaction marker missing: {shown}"
        );
    }
}
