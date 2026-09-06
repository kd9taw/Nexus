//! `js8::proto` — the message layer: [`crate::phy::Word87`] ⇄ frames, callsigns, commands.
//!
//! Contract: no audio, no DSP, no engine state — pure packing/unpacking with JS8Call's exact
//! wire layouts (varicode.cpp read as the spec). B1 lands the primitives (`bits`, `alphabet`,
//! `crc16`, `grid`, `callsign`, `command`); B4 lands `frame`, `huffman`, `jsc`, `compose`,
//! `reassembly`, `station`.

pub mod alphabet;
pub mod bits;
pub mod callsign;
pub mod crc16;
pub mod grid;
