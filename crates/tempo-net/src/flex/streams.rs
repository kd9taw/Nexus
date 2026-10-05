//! The DAX audio streams: the refcounted broker that decides when a radio-side `dax_rx` stream
//! must exist, the DAX RX audio decoder, and the DAX TX packet builder.
//!
//! # The broker
//!
//! A DAX channel (1–8) carries one slice's receive audio. [`DaxBroker`] is the one table that
//! decides when a channel's `dax_rx` stream exists on the radio. Consumers ([`Holder`]s) acquire
//! and release channels; the broker answers with [`Action`]s for the command plane to carry out:
//!
//! - the first holder of a channel asks for `stream create type=dax_rx dax_channel=<n>`;
//! - the last holder's release removes the stream only after [`REMOVAL_GRACE_MS`], and a
//!   re-acquire inside that window cancels it, so the radio's transient unbind and rebind of a
//!   slice's DAX channel never tears a stream down;
//! - a stream the radio removed while it was still held is created again after
//!   [`RECREATE_DELAY_MS`];
//! - a refused or dropped create is tried again every [`CREATE_RETRY_MS`] while the channel is
//!   held, so one failure cannot wedge a channel.
//!
//! The stream's id comes from its own status, owned by this connection ([`DaxBroker::registered`]),
//! and its removal from the radio's removal status ([`DaxBroker::unregistered`]). Nothing else
//! moves the table: decisions are never made on status echoes, such as a slice's `dax=0` and
//! `dax=<n>` pair while the radio rebinds it. Time is passed in; [`DaxBroker::poll`] fires what is
//! due. A disconnect forgets everything without removals ([`DaxBroker::reset_for_disconnect`]),
//! because the radio reaps a departed client's streams itself.
//!
//! # DAX RX audio
//!
//! [`decode_dax_audio`] turns a DAX audio payload into interleaved stereo float32 at
//! [`DAX_RATE_HZ`]: class [`AUDIO_CLASS`] (`0x03E3`) is big-endian float32 stereo, and class
//! [`AUDIO_REDUCED_CLASS`] (`0x0123`, what the radio sends after `send_reduced_bw_dax=1`) is
//! big-endian int16 mono, duplicated to both sides. A payload that is not a whole number of
//! frames, or carries a value that is not finite, is refused whole: a partial packet is never
//! audio.
//!
//! # DAX TX packets
//!
//! [`dax_tx_packet`] builds the tested DAX TX format (port plan §4.4): packet type 1 (IF data with
//! a stream id), class id present, no trailer, TSI 3, TSF 1, the 4-bit packet count, zero
//! timestamps, and [`TX_FRAMES_PER_PACKET`] frames of big-endian float32 stereo in class
//! [`AUDIO_CLASS`]. They go to the radio's VITA-49 port [`VITA_PORT`] (4991). The native path
//! Nexus shipped before this sent int16 mono (`0x0123`), a format no tested client sends, to port
//! 4993, where an over keys with no audio.
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's), the DAX RX channel ownership
//! broker and the DAX RX audio decode of `src/core/backends/flex/PanadapterStream.h` and
//! `src/core/backends/flex/PanadapterStream.cpp` at commit
//! `32fa50e4896a846a6970fa3f443bd49d667c139d` (2026-10-03), translated from C++/Qt to Rust. The
//! stream registration rules (a status of ours, `type=dax_rx`, a channel 1–8; a removal routed by
//! id) and the create-failure report follow `src/models/RadioModel.cpp`, and the DAX TX packet
//! layout follows `src/core/AudioEngine.cpp`, both read as protocol facts; no code is taken from
//! either. Deliberate differences: a pure state machine with time passed in, so the grace,
//! recreate and retry timers are due times [`DaxBroker::poll`] fires instead of Qt single-shot
//! timers, each still voided by the channel's generation; actions are returned instead of
//! signalled, and there is no mutex, PCM producer or logging; a holder is a bit index rather than
//! a named consumer; releasing everything a holder holds covers all eight channels (upstream's
//! loop stops at four); a disconnect also forgets the stream ids; stream id 0 is never a stream.
//! Recorded in the repo-root NOTICE (AetherSDR entry).

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

/// The radio's DAX audio rate, both directions.
pub const DAX_RATE_HZ: u32 = 24_000;
/// DAX audio, big-endian float32 stereo.
pub const AUDIO_CLASS: u16 = 0x03E3;
/// DAX audio at reduced bandwidth, big-endian int16 mono.
pub const AUDIO_REDUCED_CLASS: u16 = 0x0123;
/// Frames in one DAX TX packet: 128 at 24 kHz, 5.333 ms of audio.
pub const TX_FRAMES_PER_PACKET: usize = 128;
/// The radio's VITA-49 receive port, where DAX TX goes (FlexRadio's API documentation,
/// `TCPIP-meter` and `Discovery-protocol`; the port AetherSDR sends DAX TX to).
pub const VITA_PORT: u16 = 4991;
/// The radio's UDP port for a client's one-byte registration datagram, sent after registration,
/// just before `client udpport` (port plan §4.2 and §4.5, step 7; [`super::handshake`]).
pub const UDP_REGISTRATION_PORT: u16 = 4992;

/// The last holder's release is acted on this long after it (upstream `kDaxRemovalGraceMs`, far
/// longer than the radio's ~80 ms transient rebind cycle).
pub const REMOVAL_GRACE_MS: u64 = 1_500;
/// A stream the radio removed while it was held is created again this long after
/// (`kDaxRecreateDelayMs`), so a teardown storm cannot turn into a create storm.
pub const RECREATE_DELAY_MS: u64 = 500;
/// A failed create is retried this often while the channel stays held (`kDaxCreateRetryMs`).
pub const CREATE_RETRY_MS: u64 = 2_000;
/// The DAX channels a radio numbers.
pub const CHANNELS: RangeInclusive<u8> = 1..=8;
/// The most frames one decoded packet may carry (upstream `PcmFrame::kMaxFrames`).
pub const MAX_FRAMES: usize = 65_536;

/// A consumer of a DAX channel's audio: one of eight, by bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Holder(u8);

impl Holder {
    /// Holder `index`, 0 to 7.
    pub const fn new(index: u8) -> Option<Holder> {
        if index < 8 {
            Some(Holder(index))
        } else {
            None
        }
    }

    fn bit(self) -> u8 {
        1 << self.0
    }
}

/// What the broker asks the command plane to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `stream create type=dax_rx dax_channel=<channel>`. A refusal, or a create that could not
    /// be sent, goes back through [`DaxBroker::create_failed`].
    Create { channel: u8 },
    /// `stream remove 0x<stream>`.
    Remove { stream: u32, channel: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Due {
    Removal,
    Recreate,
    CreateRetry,
}

#[derive(Debug, Clone, Copy)]
struct Timer {
    channel: u8,
    /// The channel's generation when the timer was set: any later change voids it.
    generation: u32,
    at_ms: u64,
    due: Due,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Channel {
    /// The radio's stream for this channel; 0 while there is none.
    stream: u32,
    create_pending: bool,
    holders: u8,
    generation: u32,
}

/// One channel, for diagnostics and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSnapshot {
    pub channel: u8,
    pub stream: Option<u32>,
    pub create_pending: bool,
    pub holders: Vec<Holder>,
}

/// Who holds which DAX channel, and when its stream must exist.
#[derive(Debug, Default)]
pub struct DaxBroker {
    /// Our registered streams, by id: the channel each carries.
    streams: BTreeMap<u32, u8>,
    channels: BTreeMap<u8, Channel>,
    /// Monotonic; a channel never reuses a generation.
    generations: u32,
    timers: Vec<Timer>,
}

impl DaxBroker {
    pub fn new() -> DaxBroker {
        DaxBroker::default()
    }

    fn next_generation(&mut self) -> u32 {
        self.generations = self.generations.wrapping_add(1);
        self.generations
    }

    fn schedule(&mut self, channel: u8, due: Due, now_ms: u64) {
        let Some(generation) = self.channels.get(&channel).map(|c| c.generation) else {
            return;
        };
        let delay = match due {
            Due::Removal => REMOVAL_GRACE_MS,
            Due::Recreate => RECREATE_DELAY_MS,
            Due::CreateRetry => CREATE_RETRY_MS,
        };
        self.timers.push(Timer {
            channel,
            generation,
            at_ms: now_ms.saturating_add(delay),
            due,
        });
    }

    /// Add `holder` to `channel`. The channel's first holder asks for its stream. Idempotent per
    /// holder. Returns the channel's stream, `None` while its create is in flight.
    pub fn acquire(&mut self, channel: u8, holder: Holder) -> (Option<u32>, Option<Action>) {
        if !CHANNELS.contains(&channel) {
            return (None, None);
        }
        let generation = self.next_generation();
        let st = self.channels.entry(channel).or_default();
        st.holders |= holder.bit();
        // Any acquire voids a pending removal.
        st.generation = generation;
        let create = st.stream == 0 && !st.create_pending;
        if create {
            st.create_pending = true;
        }
        (
            (st.stream != 0).then_some(st.stream),
            create.then_some(Action::Create { channel }),
        )
    }

    /// Drop `holder` from `channel`. When the last holder leaves, the stream is removed after the
    /// grace window unless someone acquires the channel again first.
    pub fn release(&mut self, channel: u8, holder: Holder, now_ms: u64) {
        if !CHANNELS.contains(&channel) {
            return;
        }
        if self
            .channels
            .get(&channel)
            .is_none_or(|st| st.holders & holder.bit() == 0)
        {
            return;
        }
        let generation = self.next_generation();
        let st = self.channels.get_mut(&channel).expect("checked above");
        st.holders &= !holder.bit();
        st.generation = generation;
        if st.holders == 0 {
            if st.stream != 0 {
                self.schedule(channel, Due::Removal, now_ms);
            } else if !st.create_pending {
                self.channels.remove(&channel);
            }
            // Else a create is in flight for a channel nobody wants any more: the entry stays, so
            // the registration finds no holder and schedules exactly one removal.
        }
    }

    /// Release every channel `holder` holds.
    pub fn release_all(&mut self, holder: Holder, now_ms: u64) {
        let held: Vec<u8> = self
            .channels
            .iter()
            .filter(|(_, st)| st.holders & holder.bit() != 0)
            .map(|(ch, _)| *ch)
            .collect();
        for channel in held {
            self.release(channel, holder, now_ms);
        }
    }

    /// Whether `holder` holds `channel`.
    pub fn held_by(&self, channel: u8, holder: Holder) -> bool {
        self.channels
            .get(&channel)
            .is_some_and(|st| st.holders & holder.bit() != 0)
    }

    /// The command plane reports a refused or dropped create for `channel`. The create latch
    /// clears and, while the channel is held, a retry is armed.
    pub fn create_failed(&mut self, channel: u8, now_ms: u64) {
        if !CHANNELS.contains(&channel) {
            return;
        }
        if self.channels.get(&channel).is_none_or(|st| st.stream != 0) {
            return;
        }
        let generation = self.next_generation();
        let st = self.channels.get_mut(&channel).expect("checked above");
        st.create_pending = false;
        st.generation = generation;
        if st.holders == 0 {
            self.channels.remove(&channel);
        } else {
            self.schedule(channel, Due::CreateRetry, now_ms);
        }
    }

    /// Our `dax_rx` stream for `channel` exists: the radio's status reported it as this
    /// connection's. One stream per channel: any other id on the channel is forgotten. A stream
    /// nobody holds is removed after the grace window.
    pub fn registered(&mut self, stream: u32, channel: u8, now_ms: u64) {
        if stream == 0 || !CHANNELS.contains(&channel) {
            return;
        }
        self.streams
            .retain(|id, ch| !(*ch == channel && *id != stream));
        self.streams.insert(stream, channel);
        let generation = self.next_generation();
        let st = self.channels.entry(channel).or_default();
        st.stream = stream;
        st.create_pending = false;
        st.generation = generation;
        if st.holders == 0 {
            self.schedule(channel, Due::Removal, now_ms);
        }
    }

    /// The radio removed `stream`. A channel still held gets its stream created again after the
    /// recreate delay; our own removals forget the stream first, so this fires only for the
    /// radio's.
    pub fn unregistered(&mut self, stream: u32, now_ms: u64) {
        if stream == 0 {
            return;
        }
        self.streams.remove(&stream);
        let Some(channel) = self
            .channels
            .iter()
            .find(|(_, st)| st.stream == stream)
            .map(|(ch, _)| *ch)
        else {
            return;
        };
        let generation = self.next_generation();
        let st = self.channels.get_mut(&channel).expect("found above");
        st.stream = 0;
        st.generation = generation;
        if st.holders != 0 {
            self.schedule(channel, Due::Recreate, now_ms);
        } else {
            self.channels.remove(&channel);
        }
    }

    /// Fire what is due at `now_ms`: removals past their grace, recreates and retries.
    pub fn poll(&mut self, now_ms: u64) -> Vec<Action> {
        let (mut due, later): (Vec<Timer>, Vec<Timer>) = std::mem::take(&mut self.timers)
            .into_iter()
            .partition(|t| t.at_ms <= now_ms);
        self.timers = later;
        due.sort_by_key(|t| t.at_ms);
        let mut actions = Vec::new();
        for t in due {
            let Some(st) = self.channels.get(&t.channel).copied() else {
                continue;
            };
            if st.generation != t.generation {
                continue; // the channel changed since: this timer is void
            }
            match t.due {
                Due::Removal => {
                    if st.holders != 0 {
                        continue;
                    }
                    self.channels.remove(&t.channel);
                    if st.stream != 0 {
                        self.streams.remove(&st.stream);
                        actions.push(Action::Remove {
                            stream: st.stream,
                            channel: t.channel,
                        });
                    }
                }
                Due::Recreate | Due::CreateRetry => {
                    if st.holders == 0 || st.stream != 0 || st.create_pending {
                        continue;
                    }
                    let generation = self.next_generation();
                    let st = self.channels.get_mut(&t.channel).expect("present above");
                    st.create_pending = true;
                    st.generation = generation;
                    actions.push(Action::Create { channel: t.channel });
                }
            }
        }
        actions
    }

    /// When [`DaxBroker::poll`] next has something to fire, if anything.
    pub fn next_due(&self) -> Option<u64> {
        self.timers.iter().map(|t| t.at_ms).min()
    }

    /// The connection is gone: forget everything, send nothing. The radio reaps a departed
    /// client's streams itself.
    pub fn reset_for_disconnect(&mut self) {
        let _ = self.next_generation();
        self.channels.clear();
        self.streams.clear();
        self.timers.clear();
    }

    /// The channel `stream` carries, if it is one of ours.
    pub fn channel_of(&self, stream: u32) -> Option<u8> {
        self.streams.get(&stream).copied()
    }

    /// Our registered streams and their channels, by id.
    pub fn streams(&self) -> impl Iterator<Item = (u32, u8)> + '_ {
        self.streams.iter().map(|(id, ch)| (*id, *ch))
    }

    /// The stream of `channel`, if it exists.
    pub fn stream_of(&self, channel: u8) -> Option<u32> {
        self.channels
            .get(&channel)
            .map(|st| st.stream)
            .filter(|s| *s != 0)
    }

    /// Every channel the broker knows, in channel order.
    pub fn snapshot(&self) -> Vec<ChannelSnapshot> {
        self.channels
            .iter()
            .map(|(ch, st)| ChannelSnapshot {
                channel: *ch,
                stream: (st.stream != 0).then_some(st.stream),
                create_pending: st.create_pending,
                holders: (0..8)
                    .filter_map(Holder::new)
                    .filter(|h| st.holders & h.bit() != 0)
                    .collect(),
            })
            .collect()
    }
}

/// A VITA-49 payload without its 4-byte trailer, when one is present.
pub fn without_trailer(payload: &[u8], has_trailer: bool) -> Option<&[u8]> {
    if has_trailer {
        payload.get(..payload.len().checked_sub(4)?)
    } else {
        Some(payload)
    }
}

/// Decode a DAX RX audio payload (trailer removed) to interleaved stereo float32. `None` for
/// another class, a payload that is not a whole number of frames or is too long, or any value that
/// is not finite.
pub fn decode_dax_audio(class: u16, payload: &[u8]) -> Option<Vec<f32>> {
    match class {
        AUDIO_CLASS => {
            if payload.len() < 8
                || !payload.len().is_multiple_of(8)
                || payload.len() / 8 > MAX_FRAMES
            {
                return None;
            }
            let mut out = Vec::with_capacity(payload.len() / 4);
            for c in payload.chunks_exact(4) {
                let s = f32::from_be_bytes([c[0], c[1], c[2], c[3]]);
                if !s.is_finite() {
                    return None;
                }
                out.push(s);
            }
            Some(out)
        }
        AUDIO_REDUCED_CLASS => {
            if payload.len() < 2
                || !payload.len().is_multiple_of(2)
                || payload.len() / 2 > MAX_FRAMES
            {
                return None;
            }
            Some(
                payload
                    .chunks_exact(2)
                    .flat_map(|c| {
                        let s = f32::from(i16::from_be_bytes([c[0], c[1]])) / 32768.0;
                        [s, s]
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

/// The 24-bit FlexRadio OUI in a VITA-49 class id.
const FLEX_OUI: u32 = 0x00_1C_2D;
/// FlexRadio's information class code ("SL").
const FLEX_INFO_CLASS: u32 = 0x534C;
/// Header words before the payload: word 0, the stream id, two class-id words, one integer and
/// two fractional timestamp words.
const TX_HEADER_WORDS: usize = 7;

/// One DAX TX packet for `stream`, with the 4-bit packet `count` and interleaved stereo `samples`
/// (normally [`TX_FRAMES_PER_PACKET`] frames). `None` for an odd sample count or a packet too long
/// for the header's 16-bit size field.
pub fn dax_tx_packet(stream: u32, count: u8, samples: &[f32]) -> Option<Vec<u8>> {
    if !samples.len().is_multiple_of(2) {
        return None;
    }
    let words = TX_HEADER_WORDS + samples.len();
    let size = u16::try_from(words).ok()?;
    let word0: u32 = (0x1 << 28) // packet type 1: IF data with a stream id
        | (1 << 27) // class id present; no trailer (bit 26 clear)
        | (0x3 << 22) // TSI 3: other
        | (0x1 << 20) // TSF 1: sample count
        | ((u32::from(count) & 0xF) << 16)
        | u32::from(size);
    let mut out = Vec::with_capacity(words * 4);
    out.extend_from_slice(&word0.to_be_bytes());
    out.extend_from_slice(&stream.to_be_bytes());
    out.extend_from_slice(&FLEX_OUI.to_be_bytes());
    out.extend_from_slice(&((FLEX_INFO_CLASS << 16) | u32::from(AUDIO_CLASS)).to_be_bytes());
    out.extend_from_slice(&[0u8; 12]); // timestamps: zero
    for s in samples {
        out.extend_from_slice(&s.to_be_bytes());
    }
    Some(out)
}

#[cfg(test)]
mod tests;
