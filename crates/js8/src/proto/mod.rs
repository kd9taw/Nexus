//! `js8::proto` — the message layer: [`crate::phy::Word87`] ⇄ frames, callsigns, commands.
//!
//! Contract: no audio, no DSP, no engine state — pure packing/unpacking with JS8Call's exact
//! wire layouts (varicode.cpp read as the spec). B1 lands the primitives (`bits`, `alphabet`,
//! `crc16`, `grid`, `callsign`, `command`); B4 lands `frame`, `huffman`, `jsc`, `compose`,
//! `reassembly`, `station`.

pub mod alphabet;
pub mod bits;
pub mod callsign;
pub mod command;
pub mod compose;
pub mod crc16;
pub mod frame;
pub mod grid;
pub mod huffman;
pub mod jsc;

pub use command::Command;
pub use compose::{frame_count_estimate, frames, max_frames, ComposeError};
pub use frame::{decode_word, encode_frame, Frame, FrameError, FrameType};
