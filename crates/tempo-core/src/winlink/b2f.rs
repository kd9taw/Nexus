//! The B2F session engine — `Session::feed(&[u8]) -> Vec<Action>`.
//!
//! The whole handshake as one accumulate-and-drain state machine, in the shape
//! `tempo_net::cluster` and `tempo_net::aprsis` already use: bytes in, actions out, an internal
//! buffer holding whatever partial unit the last chunk ended mid-way through. SID exchange →
//! secure login → FC proposals → FS answers → SOH/STX/EOT transfer → LZHUF decompress → parsed
//! message, driven entirely by what the buffer now holds.
//!
//! **The session never learns which transport it is on.** It has no socket, no clock and no
//! filesystem: telnet to a CMS and ARDOP over the air feed it the identical bytes, which is why
//! the telnet path can settle the undecidable wire questions with no radio involved, and why the
//! entire handshake unit-tests from two `Vec<u8>`.
//!
//! Not yet implemented — the module exists so the rest of `winlink` can name it.
