//! Nexus Remote as a stream: the station's side of a streamed session.
//!
//! A streamed session shows the remote operator the station's own Nexus window, over WebRTC, and
//! carries their input back into that window. The station stays the authority: the same sign-in,
//! pairing, lease and stop chain admits a stream as admits the Remote page, and the station decides
//! everything about what it transmits.
//!
//! - [`protocol`] — the wire contract with the relay and the page. Its fixtures, and the prose that
//!   defines each field, live in `remote/test/fixtures/stream/`, where the relay and the page test
//!   against the same files.
pub mod protocol;
