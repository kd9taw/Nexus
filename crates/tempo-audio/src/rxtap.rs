//! A wait-free tee of the RX capture stream, so the waterfall can be produced somewhere
//! other than the radio loop.
//!
//! WHY THIS EXISTS. The waterfall row is an FFT of captured audio, but it was computed on the
//! RadioLoop thread — the same thread that issues every blocking CAT call (dial poll, heavy
//! poll, mode read, DSP-func probe, TX meters, and the slot boundary's PTT). A CAT read blocks
//! to 700 ms, or 2500 ms on slow serial, so any CAT stall meant NO row was produced for that
//! whole time. The UI kept polling at 8-20 Hz and redrew the cached row, which renders as
//! vertical STREAKING (operator report + screenshot, 2026-07-25).
//!
//! Two earlier attempts treated the symptom: 0.17.13 moved the row behind its own lock (that
//! fixed READER contention — the wrong half), and 0.17.14 backed off the worst-offending CAT
//! read's retry cadence (a frequency reducer that left every other blocking read exposed). The
//! first-order fix is that the row must not be produced on the blocking thread at all. See
//! `tasks/plan-spectrum-cat-split.md`.
//!
//! THE SPLIT THAT WAS CHOSEN, AND THE ONE THAT WAS NOT. Splitting the RADIO LOOP (moving CAT
//! telemetry or the transmitter itself onto another thread) was designed and rejected: a
//! blocking meter read on a second thread can sit in front of `rig.ptt(false)` and hold the
//! transmitter keyed PAST THE END OF AN OVER — far worse than a stuttering waterfall. The safe
//! decomposition moves the SAFE component (an FFT of audio, which needs nothing the radio loop
//! owns) off the dangerous thread, and leaves every transmit-critical statement exactly where
//! it is.
//!
//! THE SAFETY ARGUMENT IS TYPE-LEVEL. The consumer of this tap (see [`crate::rxdsp`]) captures
//! an `Arc<RxTap>` and the publish seam and NOTHING ELSE — no `Rig`, no CAT daemon, no
//! `CpalBackend`, no engine handle. There is no path from that thread to a CAT call, so no CAT
//! deadline can elapse on it. Reintroducing this bug would require adding a handle that has no
//! reason to be there.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::monitor::SpscRing;
use crate::receive_audio::{ReceiveAudioFeed, ReceiveOrigin, ReceiveSource};

/// One published capture source: the ring the audio callback pushes into, the device rate its
/// samples are at, and an epoch that changes whenever the source is replaced.
#[derive(Clone)]
pub struct RxSource {
    pub ring: Arc<SpscRing>,
    pub rate: u32,
    /// Bumped on every publish. The consumer rebuilds its resampler and clears its window when
    /// this changes, so an audio-device swap can never smear two sample rates together.
    pub epoch: u64,
}

/// The RX tap: what the capture callback tees into.
///
/// Every mutex here is held only long enough to clone an `Arc` or move a small value, so the
/// realtime callback and the radio loop can never wait measurably on the consumer.
///
/// (The RX LEVEL does not live here: it is measured by the consumer from the very samples it
/// drains and published on `tempo_app::engine::MeterFeed` — see `rxdsp.rs`. An earlier
/// `rx_level` cell here was never wired to a producer, which left readers believing a
/// stall-proof meter existed when it did not.)
#[derive(Default)]
pub struct RxTap {
    card: Mutex<Option<RxSource>>,
    epoch_seq: AtomicU64,
    audio: Arc<ReceiveAudioFeed>,
}

impl RxTap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish the sound-card capture source. Called as the LAST step of a successful audio
    /// open, so the consumer never adopts a half-built stream.
    ///
    /// The ring lives in the STREAM, not here: each open creates its own, so there is exactly
    /// one producer per ring. During a device rebuild the outgoing stream keeps pushing into
    /// ITS ring, which nobody drains any more — it fills, drops, and dies with the backend.
    pub fn publish_card(&self, ring: Arc<SpscRing>, rate: u32) {
        self.publish_card_with_origin(ring, rate, None);
    }

    /// The descriptor travels with the new ring, never with a later Settings
    /// read. Local-only/mocked sources deliberately have no origin.
    pub fn publish_card_with_origin(
        &self,
        ring: Arc<SpscRing>,
        rate: u32,
        origin: Option<ReceiveOrigin>,
    ) {
        let epoch = self
            .epoch_seq
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        if let Ok(mut g) = self.card.lock() {
            *g = Some(RxSource { ring, rate, epoch });
            self.audio
                .replace_source_with_origin(Some(ReceiveSource { epoch, rate }), origin);
        }
    }

    /// Drop the published source (audio closed). The consumer idles rather than spinning.
    pub fn clear_card(&self) {
        if let Ok(mut g) = self.card.lock() {
            *g = None;
            self.audio.replace_source(None);
        }
    }

    /// Retire only the optional media copy before capture teardown. Display and
    /// decode consumers keep their established lifecycle. Publishing a newly
    /// opened ring is the only way to make this media source available again.
    pub fn retire_receive_audio(&self) {
        self.audio.replace_source(None);
    }

    /// The source to drain right now, if any.
    pub fn current(&self) -> Option<RxSource> {
        self.card.lock().ok().and_then(|g| g.clone())
    }

    /// A separate bounded copy stream. The network consumer must not drain the
    /// single-consumer capture ring returned by current(). This feed alone does
    /// not authorize a Remote client or open any device or network connection.
    pub fn receive_audio(&self) -> Arc<ReceiveAudioFeed> {
        self.audio.clone()
    }

    pub(crate) fn publish_audio(&self, source: &RxSource, samples: &[f32]) {
        self.audio.publish(
            ReceiveSource {
                epoch: source.epoch,
                rate: source.rate,
            },
            std::time::Instant::now(),
            samples,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retiring_media_keeps_the_existing_display_ring_and_requires_a_new_open() {
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(4096));
        let origin = ReceiveOrigin::from_open(
            Some(3),
            "Radio codec",
            Some(crate::receive_audio::CaptureInput {
                device: "Resolved codec".into(),
                system_default: false,
            }),
        );
        tap.publish_card_with_origin(ring.clone(), 48_000, origin.clone());
        let feed = tap.receive_audio();
        let source = feed.describe().unwrap();
        assert_eq!(source.origin, origin);
        let reader = feed.subscribe(source.source).unwrap();
        tap.retire_receive_audio();
        assert!(feed.describe().is_none());
        assert!(matches!(
            reader.read(std::time::Instant::now()),
            Err(crate::receive_audio::ReceiveError::Ended)
        ));
        let display = tap.current().unwrap();
        assert!(Arc::ptr_eq(&display.ring, &ring));
        tap.publish_audio(&display, &[0.2; 960]);
        assert!(
            feed.source().is_none(),
            "remaining display samples cannot reopen media"
        );
        tap.publish_card_with_origin(Arc::new(SpscRing::new(4096)), 48_000, origin);
        assert_ne!(feed.source().unwrap(), source.source);
        let new = feed.subscribe(feed.source().unwrap()).unwrap();
        drop(reader);
        assert!(new.read(std::time::Instant::now()).is_ok());
    }

    #[test]
    fn publishing_a_source_bumps_the_epoch() {
        let tap = RxTap::new();
        assert!(tap.current().is_none(), "nothing published yet");
        tap.publish_card(Arc::new(SpscRing::new(64)), 48_000);
        let a = tap.current().expect("published");
        tap.publish_card(Arc::new(SpscRing::new(64)), 44_100);
        let b = tap.current().expect("republished");
        assert_ne!(a.epoch, b.epoch, "a new source must change the epoch");
        assert_eq!(b.rate, 44_100, "current() returns the NEWEST source");
    }

    #[test]
    fn the_orphaned_ring_is_not_drained_after_a_republish() {
        // A device rebuild opens the new stream BEFORE dropping the old one. The old stream
        // keeps pushing into its own ring; nobody may drain it, or two sample rates would
        // interleave into one window.
        let tap = RxTap::new();
        let old = Arc::new(SpscRing::new(64));
        tap.publish_card(old.clone(), 48_000);
        let new = Arc::new(SpscRing::new(64));
        tap.publish_card(new.clone(), 12_000);
        old.push(1.0); // the dying stream is still producing
        let cur = tap.current().expect("published");
        assert!(Arc::ptr_eq(&cur.ring, &new), "current() is the NEW ring");
        assert_eq!(
            cur.ring.len(),
            0,
            "the new ring is untouched by the old stream"
        );
    }

    #[test]
    fn clearing_the_card_idles_the_consumer() {
        let tap = RxTap::new();
        tap.publish_card(Arc::new(SpscRing::new(64)), 48_000);
        tap.clear_card();
        assert!(tap.current().is_none());
    }
}
