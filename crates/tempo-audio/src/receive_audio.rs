//! Optional, bounded copies of received sound-card audio for a media consumer.
//!
//! The existing RX DSP thread owns the capture ring. This feed never drains that
//! ring and owns no Engine, CAT, microphone, playback device or transmit handle.
//! An idle feed copies nothing; a slow or contended consumer loses media instead
//! of delaying the spectrum or decoder. Opening this local seam is not a Remote
//! permission: a network adapter must separately authorize and retire its reader.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_BLOCK_MS: usize = 100;
const MAX_BUFFER_MS: usize = 200;
const MAX_BLOCKS: usize = 16;
/// Age since publication by the RX DSP thread, not a hardware capture timestamp.
pub const MAX_MEDIA_AGE: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiveSource {
    pub epoch: u64,
    pub rate: u32,
}
impl ReceiveSource {
    fn valid(self) -> bool {
        self.epoch > 0 && (8_000..=192_000).contains(&self.rate)
    }
    fn samples_for(self, ms: usize) -> usize {
        self.rate as usize * ms / 1000
    }
}

/// The resolved capture stream's device label, obtained by its backend at open.
/// This identifies the opened OS endpoint, not the physical cable or upstream
/// routing of a virtual device. Kept local; not part of any browser DTO.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureInput {
    pub device: String,
    pub system_default: bool,
}

/// Native ownership at capture open. The requested choice is retained even when
/// startup recovery opened System default instead. A descriptor is not a grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiveOrigin {
    pub radio_id: u32,
    pub requested_input: String,
    pub input: CaptureInput,
}
impl ReceiveOrigin {
    pub fn from_open(
        radio_id: Option<u32>,
        requested_input: &str,
        input: Option<CaptureInput>,
    ) -> Option<Self> {
        let input = input.filter(|input| !input.device.trim().is_empty())?;
        Some(Self {
            radio_id: radio_id?,
            requested_input: requested_input.to_owned(),
            input,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiveDescription {
    pub source: ReceiveSource,
    pub origin: Option<ReceiveOrigin>,
}

#[derive(Debug)]
pub struct ReceiveBlock {
    pub source: ReceiveSource,
    pub published_at: Instant,
    /// Device-rate mono samples, after the existing phase-coherent fold and RX gain.
    pub samples: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveError {
    SourceUnavailable,
    SourceChanged,
    InUse,
    Ended,
}

#[derive(Default)]
struct State {
    source: Option<ReceiveSource>,
    origin: Option<ReceiveOrigin>,
    next_reader: u64,
    reader: Option<u64>,
    primed: bool,
    blocks: VecDeque<ReceiveBlock>,
    samples: usize,
}
impl State {
    fn clear(&mut self) {
        self.blocks.clear();
        self.samples = 0;
    }
    fn retire(&mut self) {
        self.reader = None;
        self.primed = false;
        self.clear();
    }
    fn pop(&mut self) -> Option<ReceiveBlock> {
        let block = self.blocks.pop_front()?;
        self.samples -= block.samples.len();
        Some(block)
    }
}

#[derive(Default)]
pub struct ReceiveAudioFeed {
    active: AtomicBool,
    state: Mutex<State>,
}
impl ReceiveAudioFeed {
    pub fn source(&self) -> Option<ReceiveSource> {
        self.state.lock().ok().and_then(|state| state.source)
    }

    /// One atomic snapshot of source generation and native capture ownership.
    /// An unnamed/mock backend can serve local DSP without media provenance.
    pub fn describe(&self) -> Option<ReceiveDescription> {
        let state = self.state.lock().ok()?;
        Some(ReceiveDescription {
            source: state.source?,
            origin: state.origin.clone(),
        })
    }

    /// One reader per local feed. Source identity must come from this station;
    /// neither a browser-supplied rate nor a stale epoch can select another input.
    /// Subscription starts empty and discards its first producer batch, including
    /// samples the DSP thread might already have drained before the subscription.
    pub fn subscribe(
        self: &Arc<Self>,
        expected: ReceiveSource,
    ) -> Result<ReceiveAudioReader, ReceiveError> {
        let mut state = self.state.lock().map_err(|_| ReceiveError::Ended)?;
        let source = state.source.ok_or(ReceiveError::SourceUnavailable)?;
        if source != expected {
            return Err(ReceiveError::SourceChanged);
        }
        if state.reader.is_some() {
            return Err(ReceiveError::InUse);
        }
        let reader = state
            .next_reader
            .checked_add(1)
            .ok_or(ReceiveError::Ended)?;
        state.next_reader = reader;
        state.reader = Some(reader);
        state.primed = false;
        state.clear();
        self.active.store(true, Ordering::Release);
        Ok(ReceiveAudioReader {
            feed: self.clone(),
            reader,
            source,
        })
    }

    /// Called with the capture-source publication, never from an audio callback.
    pub(crate) fn replace_source(&self, source: Option<ReceiveSource>) {
        self.replace_source_with_origin(source, None);
    }

    pub(crate) fn replace_source_with_origin(
        &self,
        source: Option<ReceiveSource>,
        origin: Option<ReceiveOrigin>,
    ) {
        if let Ok(mut state) = self.state.lock() {
            state.retire();
            state.source = source.filter(|source| source.valid());
            state.origin = state.source.and(origin);
            self.active.store(false, Ordering::Release);
        } else {
            self.active.store(false, Ordering::Release);
        }
    }

    /// RX DSP producer only. No network/consumer can make this call wait. The
    /// largest copied block and the total retained samples are both bounded.
    pub(crate) fn publish(&self, source: ReceiveSource, at: Instant, samples: &[f32]) {
        if !self.active.load(Ordering::Acquire) || samples.is_empty() {
            return;
        }
        let Ok(mut state) = self.state.try_lock() else {
            return;
        };
        if state.source != Some(source) || state.reader.is_none() {
            return;
        }
        if !state.primed {
            state.primed = true;
            return;
        }
        if samples.len() > source.samples_for(MAX_BLOCK_MS)
            || samples.iter().any(|sample| !sample.is_finite())
        {
            state.clear();
            return;
        }
        while state.blocks.front().is_some_and(|block| {
            at.checked_duration_since(block.published_at)
                .is_none_or(|age| age >= MAX_MEDIA_AGE)
        }) {
            state.pop();
        }
        while state.blocks.len() >= MAX_BLOCKS
            || state.samples + samples.len() > source.samples_for(MAX_BUFFER_MS)
        {
            state.pop();
        }
        state.blocks.push_back(ReceiveBlock {
            source,
            published_at: at,
            samples: samples.to_vec(),
        });
        state.samples += samples.len();
    }
}

pub struct ReceiveAudioReader {
    feed: Arc<ReceiveAudioFeed>,
    reader: u64,
    source: ReceiveSource,
}
impl ReceiveAudioReader {
    /// Empty/stale media is silence, never a replay backlog. A source change
    /// ends this reader permanently, even if the former input is selected again.
    pub fn read(&self, now: Instant) -> Result<Option<ReceiveBlock>, ReceiveError> {
        let mut state = self.feed.state.lock().map_err(|_| ReceiveError::Ended)?;
        if state.reader != Some(self.reader) || state.source != Some(self.source) {
            return Err(ReceiveError::Ended);
        }
        while let Some(block) = state.pop() {
            if now
                .checked_duration_since(block.published_at)
                .is_some_and(|age| age < MAX_MEDIA_AGE)
            {
                return Ok(Some(block));
            }
        }
        Ok(None)
    }
}
impl Drop for ReceiveAudioReader {
    fn drop(&mut self) {
        if let Ok(mut state) = self.feed.state.lock() {
            if state.reader == Some(self.reader) {
                state.retire();
                self.feed.active.store(false, Ordering::Release);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Arc<ReceiveAudioFeed>, ReceiveSource, Instant) {
        let feed = Arc::new(ReceiveAudioFeed::default());
        let source = ReceiveSource {
            epoch: 1,
            rate: 48_000,
        };
        feed.replace_source(Some(source));
        (feed, source, Instant::now())
    }

    #[test]
    fn descriptions_keep_the_requested_choice_and_actual_open_together_without_starting_media() {
        let (feed, source, now) = fixture();
        let actual = CaptureInput {
            device: "System microphone".into(),
            system_default: true,
        };
        let origin =
            ReceiveOrigin::from_open(Some(7), "Radio codec", Some(actual.clone())).unwrap();
        feed.replace_source_with_origin(Some(source), Some(origin.clone()));
        assert_eq!(
            feed.describe(),
            Some(ReceiveDescription {
                source,
                origin: Some(origin)
            })
        );
        feed.publish(source, now, &[0.5; 960]);
        assert!(!feed.active.load(Ordering::Acquire));
        assert!(feed.state.lock().unwrap().blocks.is_empty());
        assert!(ReceiveOrigin::from_open(None, "Radio codec", Some(actual.clone())).is_none());
        assert!(ReceiveOrigin::from_open(Some(7), "Radio codec", None).is_none());
        assert!(ReceiveOrigin::from_open(
            Some(7),
            "Radio codec",
            Some(CaptureInput {
                device: " ".into(),
                system_default: false
            })
        )
        .is_none());
        let next = ReceiveSource {
            epoch: source.epoch + 1,
            rate: 44_100,
        };
        let reader = feed.subscribe(source).unwrap();
        feed.replace_source_with_origin(Some(next), None);
        assert!(matches!(reader.read(now), Err(ReceiveError::Ended)));
        assert_eq!(
            feed.describe(),
            Some(ReceiveDescription {
                source: next,
                origin: None
            })
        );
        feed.replace_source_with_origin(
            Some(ReceiveSource { epoch: 0, rate: 0 }),
            ReceiveOrigin::from_open(Some(7), "", Some(actual)),
        );
        assert!(feed.describe().is_none());
        assert!(feed.state.lock().unwrap().origin.is_none());
    }

    #[test]
    fn subscription_starts_with_new_samples_and_a_second_reader_cannot_steal_them() {
        let (feed, source, now) = fixture();
        feed.publish(source, now, &[0.1; 960]);
        assert!(feed.state.lock().unwrap().blocks.is_empty());
        let reader = feed.subscribe(source).unwrap();
        assert!(matches!(feed.subscribe(source), Err(ReceiveError::InUse)));
        feed.publish(source, now, &[0.2; 960]); // could have been drained before subscribe
        assert!(reader.read(now).unwrap().is_none());
        feed.publish(source, now, &[0.3; 960]);
        let block = reader.read(now).unwrap().unwrap();
        assert_eq!(block.source, source);
        assert_eq!(block.samples, vec![0.3; 960]);
        assert!(reader.read(now).unwrap().is_none());
    }

    #[test]
    fn source_swap_and_close_end_old_readers_without_letting_their_drop_close_a_new_one() {
        let (feed, source, now) = fixture();
        let old = feed.subscribe(source).unwrap();
        let next = ReceiveSource {
            epoch: 2,
            rate: 44_100,
        };
        feed.replace_source(Some(next));
        assert!(matches!(old.read(now), Err(ReceiveError::Ended)));
        assert!(matches!(
            feed.subscribe(source),
            Err(ReceiveError::SourceChanged)
        ));
        let reader = feed.subscribe(next).unwrap();
        drop(old);
        feed.publish(source, now, &[0.1; 960]);
        feed.publish(next, now, &[0.2; 882]);
        feed.publish(next, now, &[0.3; 882]);
        assert_eq!(reader.read(now).unwrap().unwrap().samples, vec![0.3; 882]);
        feed.replace_source(None);
        assert!(matches!(reader.read(now), Err(ReceiveError::Ended)));
        assert!(matches!(
            feed.subscribe(next),
            Err(ReceiveError::SourceUnavailable)
        ));
    }

    #[test]
    fn slow_readers_drop_oldest_audio_and_expire_the_rest_at_the_publication_deadline() {
        let (feed, source, now) = fixture();
        let reader = feed.subscribe(source).unwrap();
        feed.publish(source, now, &[0.0; 960]);
        for i in 1..=20 {
            feed.publish(source, now, &[i as f32; 960]);
        }
        assert_eq!(feed.state.lock().unwrap().samples, 9_600);
        assert_eq!(reader.read(now).unwrap().unwrap().samples, vec![11.0; 960]);
        assert!(reader.read(now + MAX_MEDIA_AGE).unwrap().is_none());
        feed.publish(source, now + MAX_MEDIA_AGE, &[0.4; 960]);
        assert_eq!(
            reader.read(now + MAX_MEDIA_AGE).unwrap().unwrap().samples,
            vec![0.4; 960]
        );
    }

    #[test]
    fn excessive_backlog_invalid_samples_and_future_publications_never_replay() {
        let (feed, source, now) = fixture();
        let reader = feed.subscribe(source).unwrap();
        feed.publish(source, now, &[0.0; 960]);
        for samples in [
            vec![0.1; 4_801],
            vec![f32::NAN; 960],
            vec![f32::INFINITY; 960],
        ] {
            feed.publish(source, now, &[0.2; 960]);
            feed.publish(source, now, &samples);
            assert!(reader.read(now).unwrap().is_none());
        }
        feed.publish(source, now + Duration::from_secs(1), &[0.3; 960]);
        assert!(reader.read(now).unwrap().is_none());
    }

    #[test]
    fn a_contended_consumer_cannot_block_publication_and_a_closed_reader_retains_nothing() {
        let (feed, source, now) = fixture();
        let reader = feed.subscribe(source).unwrap();
        feed.publish(source, now, &[0.0; 960]);
        let held = feed.state.lock().unwrap();
        let (completed, waiting) = std::sync::mpsc::channel();
        let producer = feed.clone();
        let publishing = std::thread::spawn(move || {
            producer.publish(source, now, &[0.1; 960]);
            completed.send(()).unwrap();
        });
        let returned = waiting.recv_timeout(Duration::from_secs(2));
        // Release before asserting so a regressed blocking producer can finish;
        // the negative control must fail promptly instead of hanging the suite.
        drop(held);
        publishing.join().unwrap();
        assert!(returned.is_ok(), "publication waited for the reader lock");
        assert!(reader.read(now).unwrap().is_none());
        feed.publish(source, now, &[0.2; 960]);
        drop(reader);
        assert!(!feed.active.load(Ordering::Acquire));
        let next = feed.subscribe(source).unwrap();
        assert!(next.read(now).unwrap().is_none());
        feed.publish(source, now, &[0.3; 960]);
        feed.publish(source, now, &[0.4; 960]);
        assert_eq!(next.read(now).unwrap().unwrap().samples, vec![0.4; 960]);
    }

    #[test]
    fn invalid_sources_and_reader_counter_exhaustion_cannot_create_a_subscription() {
        let (feed, source, _) = fixture();
        for invalid in [
            ReceiveSource { rate: 0, ..source },
            ReceiveSource { epoch: 0, ..source },
            ReceiveSource {
                rate: 192_001,
                ..source
            },
        ] {
            feed.replace_source(Some(invalid));
            assert_eq!(feed.source(), None);
            assert!(matches!(
                feed.subscribe(invalid),
                Err(ReceiveError::SourceUnavailable)
            ));
        }
        feed.replace_source(Some(source));
        feed.state.lock().unwrap().next_reader = u64::MAX;
        assert!(matches!(feed.subscribe(source), Err(ReceiveError::Ended)));
    }
}
