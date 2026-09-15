//! The station's receive-audio lane: what a listening browser is actually fed.
//!
//! One `ReceiveEncoder` (crates/tempo-audio) turned into wire messages, and nothing
//! else. The encoder knows nothing about a transport and this module knows nothing
//! about a codec, so replacing the relay with a direct connection later is an adapter
//! swap here and no change at all there.
//!
//! ## An idle station encodes nothing, and that is structural
//!
//! [`AudioLane`] holds no encoder until a browser asks to listen. The encoder IS the
//! subscription — `ReceiveEncoder::start` takes the feed's single reader — and the feed
//! copies no samples at all while it has no reader. So the cost of this whole path on a
//! station nobody is listening to is zero, not "small": there is no timer to fire, no
//! buffer to fill and no copy to make. `idle_station_encodes_nothing` asserts it against
//! the real feed, with the control that a started lane does produce bytes on the same
//! instrument.
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

use std::sync::Arc;
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

struct Listener {
    session: String,
    device: String,
    lease: String,
    encoder: ReceiveEncoder,
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
}

impl AudioLane {
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
            .map_or(0, |l| l.encoder.encoded_bytes())
    }

    /// Subscribe to the feed for `session`. The caller has already established that this
    /// browser may listen; this refuses only what the audio path itself can refuse.
    ///
    /// One listener per station in v1. The feed has a single reader, so admitting a
    /// second browser would silently end the first one's stream — so the second is told
    /// `audioInUse` and the first is not disturbed.
    pub fn start(
        &mut self,
        feed: &Arc<ReceiveAudioFeed>,
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
        let source = feed.describe().ok_or("audioUnavailable")?.source;
        let encoder = ReceiveEncoder::start(feed, source).map_err(|error| match error {
            EncodeError::Feed(ReceiveError::InUse) => "audioInUse",
            EncodeError::Feed(_) => "audioUnavailable",
            _ => "audioUnavailable",
        })?;
        self.listener = Some(Listener {
            session: session.to_owned(),
            device: device.to_owned(),
            lease: lease.to_owned(),
            encoder,
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
        // Dropping the encoder retires the feed's reader, which puts the feed back to
        // copying nothing. Nothing else has to be told.
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
        match live.encoder.poll(now) {
            Ok(frames) => live.pending.extend(frames),
            // Every way the feed can end a live reader is a real END, not a gap: the
            // capture source was replaced or closed, so the sample rate and the epoch
            // have both moved on. Reporting it as a gap would let an operator carry on
            // believing they were still hearing the same receiver, which is the one
            // confusion this whole lane's failure vocabulary exists to prevent.
            //
            // The feed does not distinguish `Ended` from `SourceChanged` on a read (it
            // returns `Ended` for both once a reader is retired), and nothing here needs
            // it to: the distinction that matters to a listener is end-versus-gap, and
            // both of these are the end.
            Err(EncodeError::Feed(_)) => {
                self.listener = None;
                return Pump {
                    message: None,
                    ended: Some("sourceChanged"),
                };
            }
            // libopus refused, or 2^32 frames went by. Terminal for this encoder, and
            // not something a listener can do anything about beyond being told.
            Err(EncodeError::Codec(_) | EncodeError::SequenceExhausted) => {
                self.listener = None;
                return Pump {
                    message: None,
                    ended: Some("audioUnavailable"),
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
        let message = bundle(&live.session, &frames);
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
    let first = &frames[0];
    let mut payload = Vec::with_capacity(frames.iter().map(|f| f.packet.len() + 2).sum());
    for frame in frames {
        // Length-prefixed, not equal-sized: Opus is variable rate and a decoder must
        // never guess a packet boundary.
        let length = frame.packet.len() as u16;
        payload.extend_from_slice(&length.to_be_bytes());
        payload.extend_from_slice(&frame.packet);
    }
    state(
        session,
        json!({
            "type": "audioRx",
            "seq": first.seq,
            "epoch": format!("{:016x}", first.epoch),
            "firstFrameMs": first.capture_ms,
            "frameMs": first.frame_ms,
            "count": frames.len(),
            "payload": crate::b64_encode(&payload),
        }),
    )
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
    let mut value = json!({ "type": "audioState", "listening": listening });
    if let Some(reason) = reason {
        value["reason"] = json!(reason);
    }
    state(session, value)
}

#[cfg(test)]
mod tests;
