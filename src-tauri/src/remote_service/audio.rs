//! The station's receive-audio lane: what a listening browser is actually fed.
//!
//! One `ReceiveEncoder` (crates/tempo-audio) turned into wire messages, and nothing
//! else. The encoder knows nothing about a transport and this module knows nothing
//! about a codec, so replacing the relay with a direct connection later is an adapter
//! swap here and no change at all there.
//!
//! ## An idle station encodes nothing, and that is structural
//!
//! No encoder exists until a browser asks to listen. The encoder IS the
//! subscription — `ReceiveEncoder::start` takes the feed's single reader — and the feed
//! copies no samples at all while it has no reader. So the cost of this whole path on a
//! station nobody is listening to is zero, not "small": there is no timer to fire, no
//! buffer to fill and no copy to make. `idle_station_encodes_nothing` asserts it against
//! the real feed, with the control that a started lane does produce bytes on the same
//! instrument.
//!
//! ## One encoder, every listener (plan P5)
//!
//! A browser Listening on the relay's lane and a streamed page's `audio` channel hear the
//! station at once. The feed still has one reader, and it belongs to [`ReceiveFanout`]: the
//! station's one encoder, which encodes each 20 ms frame ONCE and hands a copy to every
//! listener subscribed to it ([`Subscription`]). The first listener to arrive starts it and
//! the last to leave stops it, so the idle rule above is unchanged.
//!
//! Each listener has its own backlog, bounded by [`MAX_PENDING_FRAMES`]. The encoder runs
//! when any listener polls, and a listener that stops taking frames (a stalled page, a
//! thread that is behind) loses ITS OWN oldest frames as a sequence gap; nobody else waits
//! for it or loses anything by it. A source change or a codec failure ends every listener,
//! each told once, and a listener's close or lease lapse removes only that listener.
//!
//! ## Only a controlling browser may listen
//!
//! Admission is [`super::operations::Authority::audio_admitted`]: the operator's local
//! **control** grant plus this browser's own live lease. A logging-only browser holds
//! the logging grant and a perfectly valid lease, and is refused — listening to a shack
//! is not a consequence of being allowed to write to its log. The lane re-checks on a
//! slow cadence as well as at the start, so a lapsed lease stops the audio rather than
//! leaving a stream running for a browser that no longer controls anything.
//!
//! ## Losing audio is the designed outcome, never delay
//!
//! Three bounds stand between a browser that stops reading and this station's memory,
//! and none of them is "wait":
//!
//! 1. `ReceiveAudioFeed` drops old blocks rather than growing (200 ms, upstream).
//! 2. This lane refuses to queue: when the socket is not writable the whole bundle is
//!    discarded, the sequence skips, and the listener hears a 60 ms gap. It never holds
//!    more than [`MAX_PENDING_FRAMES`] whatever the caller does.
//! 3. The relay gives up on a browser that has not taken a bundle in seconds, which ends
//!    the session — and the lane stops when nobody is listening.
//!
//! This is the same rule the feed states for its own seam, carried one layer out: media
//! is cheaper than a stalled producer, and here the producer shares a socket with Stop.
//!
//! ## Listening is not transmitting
//!
//! Nothing in this module can key a radio. It holds no Engine, no PTT, no transmit
//! authority and no microphone; it reads one bounded copy of already-captured receive
//! audio. The microphone direction is a separate batch behind its own grant.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempo_audio::receive_audio::{ReceiveAudioFeed, ReceiveError};
use tempo_audio::receive_encode::{EncodeError, EncodedFrame, ReceiveEncoder};

/// Frames per wire message. 3 x 20 ms: at one packet per message the TCP+TLS+WebSocket
/// header alone runs about 30 kbit/s against a 24 kbit/s payload, so bundling is not a
/// tidiness choice — unbundled, the headers cost more than the audio. At three the
/// header share falls to roughly 10 kbit/s and the added delay is 40 ms.
pub const BUNDLE_FRAMES: usize = 3;
/// Everything this lane will ever hold, in frames: five bundles, 300 ms. Reached only
/// if a caller keeps polling an unwritable socket; the oldest frames go first, because
/// old receive audio is worth less than new receive audio.
pub const MAX_PENDING_FRAMES: usize = BUNDLE_FRAMES * 5;
/// How often the lane re-asks whether its listener still controls the station. A lease
/// is five seconds and a heartbeat renews it well inside that, so a second is prompt
/// without putting the authority lock in the 60 ms path.
const RECHECK: Duration = Duration::from_secs(1);
/// Matches the browser and relay bound (`ui/src/remote-web/audio-protocol.ts`). Asserted
/// rather than trusted, because the arithmetic that says a bundle fits is exactly the
/// kind of claim that stays true until it quietly does not.
pub const MAX_MESSAGE_BYTES: usize = 1024;

/// The station's one receive-audio encoder, shared by every listener (plan P5): the relay's
/// Listen lane and each stream's `audio` channel. One per station, built beside the feed.
pub struct ReceiveFanout {
    feed: Arc<ReceiveAudioFeed>,
    state: Mutex<Fanout>,
}

#[derive(Default)]
struct Fanout {
    /// The feed's one reader, encoding each frame once for everyone. `None` while nobody
    /// listens, which is what keeps an idle station at zero cost.
    encoder: Option<ReceiveEncoder>,
    next: u64,
    sinks: Vec<Sink>,
}

/// One listener's share of the encoder's output.
struct Sink {
    id: u64,
    /// Frames encoded since this listener last took them. Bounded by [`MAX_PENDING_FRAMES`]:
    /// a listener that stops taking loses its own oldest frames, never anyone else's.
    frames: VecDeque<EncodedFrame>,
    /// Frames this listener lost to that bound, handed over with its next take.
    dropped: u64,
    /// The encoder ended under this listener, and why. Terminal for it; told once.
    ended: Option<&'static str>,
}

/// A listener's place on the fan-out. Dropping it leaves, and the last to leave releases the
/// feed's reader: the station goes back to encoding nothing.
pub struct Subscription {
    fanout: Arc<ReceiveFanout>,
    id: u64,
}

/// What one take produced for one listener.
struct Taken {
    frames: Vec<EncodedFrame>,
    /// Frames this listener lost to its own bound since its last take.
    dropped: u64,
}

impl ReceiveFanout {
    pub fn new(feed: Arc<ReceiveAudioFeed>) -> Arc<Self> {
        Arc::new(Self {
            feed,
            state: Mutex::new(Fanout::default()),
        })
    }

    /// A poisoned lock still holds a coherent state: queues and counters, nothing half-done
    /// that a listener could be harmed by.
    fn state(&self) -> std::sync::MutexGuard<'_, Fanout> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Join. The first listener starts the encoder (it takes the feed's reader); a later one
    /// shares it, from its next frame.
    fn subscribe(self: &Arc<Self>) -> Result<Subscription, &'static str> {
        let mut state = self.state();
        if state.encoder.is_none() {
            let source = self.feed.describe().ok_or("audioUnavailable")?.source;
            state.encoder =
                Some(
                    ReceiveEncoder::start(&self.feed, source).map_err(|error| match error {
                        EncodeError::Feed(ReceiveError::InUse) => "audioInUse",
                        EncodeError::Feed(_) => "audioUnavailable",
                        _ => "audioUnavailable",
                    })?,
                );
        }
        state.next += 1;
        let id = state.next;
        state.sinks.push(Sink {
            id,
            frames: VecDeque::with_capacity(MAX_PENDING_FRAMES),
            dropped: 0,
            ended: None,
        });
        Ok(Subscription {
            fanout: self.clone(),
            id,
        })
    }

    /// Frames encoded so far by the live encoder. Zero while nobody listens.
    #[cfg(test)]
    fn frames_encoded(&self) -> u32 {
        self.state()
            .encoder
            .as_ref()
            .map_or(0, ReceiveEncoder::frames_encoded)
    }
}

impl Fanout {
    /// Encode whatever the feed holds, once, and give every live listener its copy.
    fn pump(&mut self, now: Instant) {
        let Some(encoder) = self.encoder.as_mut() else {
            return;
        };
        match encoder.poll(now) {
            Ok(frames) => {
                for frame in frames {
                    for sink in self.sinks.iter_mut().filter(|sink| sink.ended.is_none()) {
                        if sink.frames.len() >= MAX_PENDING_FRAMES {
                            sink.frames.pop_front();
                            sink.dropped += 1;
                        }
                        sink.frames.push_back(frame.clone());
                    }
                }
            }
            // Every way the feed can end a live reader is a real END for every listener: the
            // capture source was replaced or closed (see `AudioLane::poll`). libopus refusing,
            // or 2^32 frames, is terminal for the encoder, so for all of them too. The encoder
            // goes now, so the next listener to arrive starts a fresh one on the new source.
            Err(error) => {
                let reason = match error {
                    EncodeError::Feed(_) => "sourceChanged",
                    EncodeError::Codec(_) | EncodeError::SequenceExhausted => "audioUnavailable",
                };
                self.encoder = None;
                for sink in &mut self.sinks {
                    sink.ended.get_or_insert(reason);
                    sink.frames.clear();
                }
            }
        }
    }
}

impl Subscription {
    /// Run the encoder (once, for everyone) and take this listener's frames.
    fn take(&self, now: Instant) -> Result<Taken, &'static str> {
        let mut state = self.fanout.state();
        state.pump(now);
        let sink = state
            .sinks
            .iter_mut()
            .find(|sink| sink.id == self.id)
            .ok_or("audioUnavailable")?;
        if let Some(reason) = sink.ended {
            return Err(reason);
        }
        Ok(Taken {
            frames: sink.frames.drain(..).collect(),
            dropped: std::mem::take(&mut sink.dropped),
        })
    }

    /// Payload bytes the shared encoder has produced.
    #[cfg(test)]
    fn encoded_bytes(&self) -> u64 {
        self.fanout
            .state()
            .encoder
            .as_ref()
            .map_or(0, ReceiveEncoder::encoded_bytes)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut state = self.fanout.state();
        state.sinks.retain(|sink| sink.id != self.id);
        // The last listener gone: dropping the encoder retires the feed's reader, which puts
        // the feed back to copying nothing.
        if state.sinks.is_empty() {
            state.encoder = None;
        }
    }
}

struct Listener {
    session: String,
    device: String,
    lease: String,
    /// This listener's share of the station's one encoder.
    audio: Subscription,
    /// Whole frames waiting for a bundle. Bounded by [`MAX_PENDING_FRAMES`].
    pending: Vec<EncodedFrame>,
    checked_at: Instant,
}

/// What one poll produced.
#[derive(Debug, Default)]
pub struct Pump {
    /// At most one bundle. Ready to send verbatim.
    pub message: Option<String>,
    /// The listener ended and the browser should be told why. Terminal for this listener.
    pub ended: Option<&'static str>,
}

#[derive(Default)]
pub struct AudioLane {
    listener: Option<Listener>,
    /// Frames discarded because the socket could not take them. Diagnostics; a listener
    /// learns about these as sequence gaps, which is the only signal that matters.
    dropped: u64,
    bundles: u64,
    /// The stream's lane, whose messages go straight to one page on its own data channel and
    /// so carry no relay address: exactly the bundle the relay would deliver, which is what the
    /// page's player parses (and it refuses a key it does not know).
    unaddressed: bool,
}

impl AudioLane {
    /// A lane for a streamed session's `audio` data channel.
    pub fn unaddressed() -> Self {
        Self {
            unaddressed: true,
            ..Self::default()
        }
    }

    /// True while a browser is being fed. Also the answer to "is this station encoding",
    /// because the encoder exists only inside a listener.
    pub fn listening(&self) -> bool {
        self.listener.is_some()
    }
    /// The session being fed, for addressing a message.
    pub fn session(&self) -> Option<&str> {
        self.listener.as_ref().map(|l| l.session.as_str())
    }
    /// Counters the tests read. Nothing in the shipped path consumes them: a listener
    /// learns about a drop as a sequence gap, which is the only signal that matters to
    /// it, and inventing an operator-facing number nobody asked for would be scope this
    /// batch does not have.
    #[cfg(test)]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
    #[cfg(test)]
    pub fn bundles(&self) -> u64 {
        self.bundles
    }
    /// Payload bytes encoded so far. Zero on a station nobody is listening to, and the
    /// measurement the idle assertion reads.
    #[cfg(test)]
    pub fn encoded_bytes(&self) -> u64 {
        self.listener
            .as_ref()
            .map_or(0, |l| l.audio.encoded_bytes())
    }

    /// Subscribe to the station's receive audio for `session`. The caller has already
    /// established that this browser may listen; this refuses only what the audio path
    /// itself can refuse.
    ///
    /// One listener per lane: the relay's lane feeds the browser that holds the lease, and a
    /// stream's lane feeds its own page. A second browser on the same lane is told
    /// `audioInUse` and the first is not disturbed. Lanes do not exclude each other: every
    /// lane hears the station through the one [`ReceiveFanout`].
    pub fn start(
        &mut self,
        fanout: &Arc<ReceiveFanout>,
        session: &str,
        device: &str,
        lease: &str,
        now: Instant,
    ) -> Result<(), &'static str> {
        if let Some(live) = &self.listener {
            // Asking again for a session that is already listening is not an error; a
            // browser may re-send after a reconnect.
            return if live.session == session {
                Ok(())
            } else {
                Err("audioInUse")
            };
        }
        let audio = fanout.subscribe()?;
        self.listener = Some(Listener {
            session: session.to_owned(),
            device: device.to_owned(),
            lease: lease.to_owned(),
            audio,
            pending: Vec::with_capacity(MAX_PENDING_FRAMES),
            checked_at: now,
        });
        Ok(())
    }

    /// Stop feeding. `session` of `None` stops whoever is listening — a disconnect, a
    /// socket teardown, Remote going off. Returns the session that was stopped.
    pub fn stop(&mut self, session: Option<&str>) -> Option<String> {
        let matches = match (&self.listener, session) {
            (Some(live), Some(id)) => live.session == id,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if !matches {
            return None;
        }
        // Dropping the subscription leaves the fan-out; the last listener to leave retires
        // the feed's reader, which puts the feed back to copying nothing. Nothing else has to
        // be told, and no other listener notices.
        self.listener.take().map(|l| l.session)
    }

    /// Drain the encoder and produce at most one bundle.
    ///
    /// `writable` is the caller's answer to "can the socket take a message right now
    /// without waiting". False means the browser is not keeping up: the bundle is
    /// dropped, never queued, and the listener hears a gap. Delaying it instead would
    /// put receive audio in front of an operation response on a shared writer, which is
    /// the one thing this lane must never be able to do.
    pub fn poll(&mut self, now: Instant, writable: bool) -> Pump {
        let Some(live) = self.listener.as_mut() else {
            return Pump::default();
        };
        match live.audio.take(now) {
            Ok(taken) => {
                live.pending.extend(taken.frames);
                // Lost to this listener's own bound on the fan-out: a gap it hears, like any drop.
                self.dropped += taken.dropped;
            }
            // Every way the feed can end a live reader is a real END, not a gap: the
            // capture source was replaced or closed, so the sample rate and the epoch
            // have both moved on (`sourceChanged`). Reporting it as a gap would let an
            // operator carry on believing they were still hearing the same receiver, which
            // is the one confusion this whole lane's failure vocabulary exists to prevent.
            //
            // The feed does not distinguish `Ended` from `SourceChanged` on a read (it
            // returns `Ended` for both once a reader is retired), and nothing here needs
            // it to: the distinction that matters to a listener is end-versus-gap, and
            // both of these are the end. libopus refusing, or 2^32 frames, is terminal for
            // the encoder too (`audioUnavailable`), and not something a listener can do
            // anything about beyond being told.
            Err(reason) => {
                self.listener = None;
                return Pump {
                    message: None,
                    ended: Some(reason),
                };
            }
        }
        // The hard bound. Reached only by a caller that keeps polling an unwritable
        // socket; the oldest go, because stale receive audio is the least valuable
        // thing here.
        if live.pending.len() > MAX_PENDING_FRAMES {
            let excess = live.pending.len() - MAX_PENDING_FRAMES;
            live.pending.drain(..excess);
            self.dropped += excess as u64;
        }
        if live.pending.len() < BUNDLE_FRAMES {
            return Pump::default();
        }
        let frames: Vec<EncodedFrame> = live.pending.drain(..BUNDLE_FRAMES).collect();
        if !writable {
            self.dropped += frames.len() as u64;
            return Pump::default();
        }
        let message = if self.unaddressed {
            bundle_value(&frames).to_string()
        } else {
            bundle(&live.session, &frames)
        };
        // Never send what the far end is bound to refuse. A bundle over the shared bound
        // would close the socket at the relay, so it is dropped here as loss instead.
        if message.len() > MAX_MESSAGE_BYTES {
            self.dropped += frames.len() as u64;
            return Pump::default();
        }
        self.bundles += 1;
        Pump {
            message: Some(message),
            ended: None,
        }
    }

    /// Re-ask the authority, on a slow cadence, whether the listener still controls the
    /// station. A lapsed lease or a withdrawn grant stops the audio; a *busy* authority
    /// does not, because contention is not a refusal.
    pub fn recheck(
        &mut self,
        now: Instant,
        admitted: impl Fn(&str, &str, &str) -> Result<(), &'static str>,
    ) -> Option<&'static str> {
        let live = self.listener.as_mut()?;
        if now.saturating_duration_since(live.checked_at) < RECHECK {
            return None;
        }
        live.checked_at = now;
        match admitted(&live.session, &live.device, &live.lease) {
            Ok(()) | Err("remoteBusy") => None,
            Err(reason) => {
                self.listener = None;
                Some(reason)
            }
        }
    }
}

/// One wire bundle. `count` and `frameMs` are stated, so a decoder never has to derive a
/// duration from a payload length; the epoch is the capture generation, formatted the
/// same way `transmitEpoch` already is.
fn bundle(session: &str, frames: &[EncodedFrame]) -> String {
    state(session, bundle_value(frames))
}

/// The bundle itself, before any address.
fn bundle_value(frames: &[EncodedFrame]) -> Value {
    let first = &frames[0];
    let mut payload = Vec::with_capacity(frames.iter().map(|f| f.packet.len() + 2).sum());
    for frame in frames {
        // Length-prefixed, not equal-sized: Opus is variable rate and a decoder must
        // never guess a packet boundary.
        let length = frame.packet.len() as u16;
        payload.extend_from_slice(&length.to_be_bytes());
        payload.extend_from_slice(&frame.packet);
    }
    json!({
        "type": "audioRx",
        "seq": first.seq,
        "epoch": format!("{:016x}", first.epoch),
        "firstFrameMs": first.capture_ms,
        "frameMs": first.frame_ms,
        "count": frames.len(),
        "payload": crate::b64_encode(&payload),
    })
}

/// Address a message to one browser. The relay strips this before delivery.
fn state(session: &str, mut value: Value) -> String {
    value["sessionId"] = json!(session);
    value.to_string()
}

/// Narrow a station-side refusal to the vocabulary the relay and the browser both know
/// (`ui/src/remote-web/audio-protocol.ts`).
///
/// This is not tidiness. The browser's parser refuses a state message carrying a reason
/// it does not recognise and closes its socket over it, so a code that leaked out of
/// this side — `remoteBusy`, `invalidRequest`, anything a future edit adds — would turn
/// a momentary refusal into a dropped session. An unknown code travels as
/// `audioUnavailable` instead, which is both true and harmless.
pub fn shared_reason(reason: &'static str) -> &'static str {
    match reason {
        "audioInUse" | "notController" | "sourceChanged" | "audioStopped" => reason,
        _ => "audioUnavailable",
    }
}

/// Tell one browser what the audio lane is doing. `reason` is a fixed code from the
/// shared vocabulary, never a message: a refusal string is attacker-adjacent input in
/// the other direction and this side keeps the same discipline.
pub fn audio_state(session: &str, listening: bool, reason: Option<&'static str>) -> String {
    state(session, audio_state_value(listening, reason))
}

/// The same, unaddressed: for a streamed session's own `audio` channel.
pub fn audio_state_value(listening: bool, reason: Option<&'static str>) -> Value {
    let mut value = json!({ "type": "audioState", "listening": listening });
    if let Some(reason) = reason {
        value["reason"] = json!(reason);
    }
    value
}

#[cfg(test)]
mod tests;
