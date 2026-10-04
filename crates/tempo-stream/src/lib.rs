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
//! - [`offer`] — what a page's offer must be before the station answers it: DTLS-SRTP only.
//! - [`frame_clock`] — how old the picture the page last showed is, on the station's own clock.
//! - [`lan`] — the last check that no LAN address leaves the shack, and where Remote over this
//!   network may listen and whom it may hear.
//! - [`stun`] — the one binding request that learns the station's reflexive address.
//! - [`session`] — one streamed session over WebRTC: answer, candidates, video, data channels.
//! - [`video`] — the picture: the station's window, captured and encoded as VP8.
//! - [`keep_awake`] — the shack kept awake, system and display, while a stream is attached.
pub mod frame_clock;
pub mod keep_awake;
pub mod lan;
pub mod offer;
pub mod protocol;
pub mod session;
pub mod stun;
pub mod video;
