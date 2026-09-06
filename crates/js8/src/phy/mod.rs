//! `js8::phy` — the physical layer: audio ⇄ [`Word87`].
//!
//! Contract: everything in here is a pure function of its arguments (no statics, no locks) so
//! the four JS8 speeds can decode concurrently. B1 lands the tables and the codec core
//! (`speed`, `costas`, `crc12`, `frame`, `ldpc`, `modulate`); B3 lands the receiver
//! (`sync`, `downsample`, `demod`, `subtract`, `decoder`) and the `decode` entry point.

pub mod costas;
pub mod crc12;
pub mod frame;
pub mod ldpc;
pub mod ldpc_tables;
pub mod modulate;
pub mod speed;

mod downsample;
mod params;
mod sync;
#[cfg(test)]
pub(crate) mod testutil;

pub use crc12::crc12;
pub use frame::{Payload72, Word87, I3};
pub use modulate::{encode_word, modulate};
pub use speed::Speed;

/// One CRC-verified decode from [`decode`] (B3) — phy never emits an unverified word.
#[derive(Debug, Clone, PartialEq)]
pub struct RawDecode {
    pub speed: Speed,
    /// Audio offset of the lowest tone, Hz.
    pub freq_hz: f32,
    /// WSJT-X convention: xdt = t − 0.5 s relative to the cycle start.
    pub dt_s: f32,
    /// 2500 Hz-bandwidth convention.
    pub snr_db: i32,
    pub sync: f32,
    pub word: Word87,
    pub nharderrors: u8,
    /// 1 − (nharderrors + dmin)/60, clamped to [0, 1] (JS8.cpp:2370).
    pub quality: f32,
}

/// Decoder knobs. `stock()` is what JS8Call runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecodeParams {
    /// Lowest search frequency, Hz.
    pub nfa: f32,
    /// Highest search frequency, Hz.
    pub nfb: f32,
    /// 1..=3 (JS8Call `-d`).
    pub depth: u8,
    /// Outer passes with signal subtraction (JS8Call: 2 of the 3 outer passes).
    pub subtract_passes: u8,
}

impl DecodeParams {
    /// JS8Call stock: nfa 100, nfb 4000, depth 3, subtract_passes 2.
    pub const fn stock() -> DecodeParams {
        DecodeParams {
            nfa: 100.0,
            nfb: 4000.0,
            depth: 3,
            subtract_passes: 2,
        }
    }
}
