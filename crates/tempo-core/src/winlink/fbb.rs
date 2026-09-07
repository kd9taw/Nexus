//! The FBB forwarding protocol — `FC` proposals, the `F>` block checksum, `FS` answers, and the
//! SOH/STX/EOT binary framer.
//!
//! Two halves of one wire language. The **proposal** half is line-oriented ASCII: `FC` lines offer
//! messages by MID with their uncompressed and compressed sizes, `F>` closes the block with a
//! two-hex-digit checksum over it, and the peer's `FS` line answers each proposal in order —
//! including the `!offset` form that resumes a transfer already partly held. The **framing** half
//! is binary: SOH/STX records with a length byte (`0x00` meaning 256), terminated by EOT carrying
//! a two's-complement checksum over the data, and a frame whose checksum fails must deliver
//! nothing at all rather than deliver something corrupt.
//!
//! Bytes, never `String`: a length byte, a checksum byte and an LZHUF body are all free to be any
//! value at all, and decoding them as UTF-8 corrupts them inbound and — far worse — outbound.
//!
//! Not yet implemented — the module exists so the rest of `winlink` can name it.
