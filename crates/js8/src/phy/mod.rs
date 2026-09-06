//! `js8::phy` — the physical layer: audio ⇄ [`Word87`].
//!
//! Contract: everything in here is a pure function of its arguments (no statics, no locks) so
//! the four JS8 speeds can decode concurrently. B1 lands the tables and the codec core
//! (`speed`, `costas`, `crc12`, `frame`, `ldpc`, `modulate`); B3 lands the receiver
//! (`sync`, `downsample`, `demod`, `subtract`, `decoder`) and the `decode` entry point.

pub mod costas;
pub mod crc12;
pub mod speed;

pub use crc12::crc12;
pub use speed::Speed;
