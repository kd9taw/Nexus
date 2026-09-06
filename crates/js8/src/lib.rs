//! JS8 — JS8Call-compatible modem (`phy`) and message layer (`proto`), pure Rust.
//!
//! JS8Call (github.com/js8call/js8call @ a7ff1be0, GPLv3) was read as the on-air protocol
//! SPEC; every table here is a transcribed FACT cited to its upstream file (see NOTICE); no
//! code is copied. `phy` never sees text, `proto` never sees audio; the seam is
//! [`phy::Word87`] — 87 info bits (72 payload + 3 i3 + 12 CRC-12) MSB-first in 11 bytes.
//!
//! WHY one crate, two modules (judge.md ruling): a separate proto crate bought nothing once
//! the modem is Rust — no gfortran in the proto tests to keep out — and would add a manifest
//! plus a re-export layer that can drift. WHY pure Rust: every vendored WSJT-X routine hardcodes
//! NSPS=1920 at compile time, uses the flipped Costas, Gray-maps and fuses `unpack77`; none is
//! reusable for any JS8 speed, and a Fortran chain would serialise the four speeds on
//! `MODEM_LOCK`. This crate takes NO lock: every function is a pure function of its inputs so
//! four speeds may decode under `std::thread::scope` (B5). Do not add a lock "for symmetry".
//!
//! Re-exports grow with the batches: B1 lands the phy seam types and the proto primitives;
//! B3 adds `decode`; B4 adds `Command`-level `Frame`, `Reassembler`, `Station`.

pub mod phy;
pub mod proto;

pub use phy::{DecodeParams, Payload72, RawDecode, Speed, Word87, I3};
pub use proto::Command;
