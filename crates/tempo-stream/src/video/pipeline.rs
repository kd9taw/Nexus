//! The encoder thread: the newest captured picture in, VP8 frames out, on the station's terms.
//!
//! - **Drop, never queue.** The capture writes into a one-picture [`Mailbox`]; a picture the
//!   encoder has not reached yet is replaced by the next one, so a slow encoder shows the page the
//!   newest picture late rather than every picture later and later.
//! - **At most 30 frames a second** ([`FRAME_INTERVAL`]), however often the window changes.
//! - **A still window is sent at least twice a second** ([`STILL_FRAME_MS`]). The capture delivers
//!   a picture only when the window changes, and the page's freshness rule (S9) needs frames to
//!   echo, so the last picture is encoded again, stamped now. That is honest because nothing
//!   changed: the picture IS the window as of now. It stops the moment the window is not showing
//!   (minimized, or closed), so a page never sees an old picture presented as current.
//! - **A frame the transport cannot take is not silently lost.** The output holds a few frames;
//!   when it is full the frame is dropped and the next one is a keyframe, because every VP8 frame
//!   after a lost one decodes wrong until a keyframe arrives.
//! - **The capture instant rides with each frame**, strictly increasing, because it becomes the RTP
//!   timestamp the page echoes and the freshness rule reads (`frame_clock`).
//! - **Below the radio.** The thread lowers its own priority on Windows (the caller's `lower`).
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::picture::{self, Bgra, I420};
use crate::protocol::STILL_FRAME_MS;

/// The shortest time between two encoded frames: 30 a second.
pub const FRAME_INTERVAL: Duration = Duration::from_micros(33_334);
/// The longest a still window goes without a frame.
pub const STILL_FRAME: Duration = Duration::from_millis(STILL_FRAME_MS);
/// Frames the transport has not taken yet, before the next one is dropped.
const OUTPUT_DEPTH: usize = 4;

/// One encoded packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub data: Vec<u8>,
    pub keyframe: bool,
}

/// An encoder for one picture size.
pub trait Encode: Send {
    /// Encode `picture`, shown `at` after the encoder was opened. Returns the packets it made,
    /// which is none when the encoder chose to skip, or `None` if it failed.
    fn encode(&mut self, picture: &I420, at: Duration, keyframe: bool) -> Option<Vec<Packet>>;
}

/// Makes an encoder for a picture size. Called again whenever the size changes.
pub type MakeEncoder = Box<dyn FnMut(u32, u32) -> Option<Box<dyn Encode>> + Send>;

/// One encoded frame, ready for the transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoded {
    pub data: Vec<u8>,
    pub keyframe: bool,
    /// When the picture was captured (for a still window: when it was sent again).
    pub captured_at: Instant,
}

#[derive(Default)]
struct Slot {
    picture: Option<Bgra>,
    ended: bool,
}

/// What the capture hands the encoder: the newest picture, and whether there will be more.
#[derive(Default)]
pub struct Mailbox {
    slot: Mutex<Slot>,
    ready: Condvar,
}

/// What [`Mailbox::take`] found.
pub enum Taken {
    Picture(Bgra),
    /// Nothing new within the wait.
    Nothing,
    /// The capture is over and nothing is left.
    Ended,
}

impl Mailbox {
    /// A new picture, replacing one the encoder has not taken yet.
    pub fn put(&self, picture: Bgra) {
        let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        slot.picture = Some(picture);
        self.ready.notify_one();
    }

    /// The capture has ended; there will be no more pictures.
    pub fn end(&self) {
        let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        slot.ended = true;
        self.ready.notify_one();
    }

    pub fn ended(&self) -> bool {
        self.slot.lock().unwrap_or_else(|p| p.into_inner()).ended
    }

    /// The newest picture, waiting at most `wait` for one.
    pub fn take(&self, wait: Duration) -> Taken {
        let slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        let (mut slot, _) = self
            .ready
            .wait_timeout_while(slot, wait, |s| s.picture.is_none() && !s.ended)
            .unwrap_or_else(|p| p.into_inner());
        match slot.picture.take() {
            Some(picture) => Taken::Picture(picture),
            None if slot.ended => Taken::Ended,
            None => Taken::Nothing,
        }
    }
}

struct Shared {
    stop: AtomicBool,
    keyframe: AtomicBool,
}

/// The running encoder thread. Dropping it stops the thread and waits for it.
pub struct Pipeline {
    shared: Arc<Shared>,
    mailbox: Arc<Mailbox>,
    frames: Receiver<Encoded>,
    thread: Option<JoinHandle<()>>,
}

impl Pipeline {
    /// Start encoding what `mailbox` receives. `showing` says whether the window can be seen right
    /// now; `lower` runs first on the new thread (the Windows priority drop).
    pub fn start(
        mailbox: Arc<Mailbox>,
        showing: Box<dyn Fn() -> bool + Send>,
        make: MakeEncoder,
        lower: fn(),
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            keyframe: AtomicBool::new(false),
        });
        let (out, frames) = mpsc::sync_channel(OUTPUT_DEPTH);
        let thread = {
            let shared = shared.clone();
            let mailbox = mailbox.clone();
            std::thread::Builder::new()
                .name("nexus-stream-video".into())
                .spawn(move || {
                    lower();
                    Encoder::new(make).run(&shared, &mailbox, &*showing, &out);
                })?
        };
        Ok(Self {
            shared,
            mailbox,
            frames,
            thread: Some(thread),
        })
    }

    /// The page lost a frame and asked for a keyframe (PLI/FIR).
    pub fn request_keyframe(&self) {
        self.shared.keyframe.store(true, Ordering::Relaxed);
    }

    /// The frames encoded since the last call, oldest first.
    pub fn take(&self) -> Vec<Encoded> {
        self.frames.try_iter().collect()
    }

    /// Has the video ended for good: the capture is over, or the thread stopped?
    pub fn ended(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        // Wake the thread if it is waiting for a picture: nothing more is coming.
        self.mailbox.end();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The encoder thread's own state.
struct Encoder {
    make: MakeEncoder,
    current: Option<(Box<dyn Encode>, u32, u32, Instant)>,
    /// The last picture encoded, kept to send again while the window is still.
    last: Option<I420>,
    last_sent: Option<Instant>,
    last_captured: Option<Instant>,
    keyframe: bool,
}

impl Encoder {
    fn new(make: MakeEncoder) -> Self {
        Self {
            make,
            current: None,
            last: None,
            last_sent: None,
            last_captured: None,
            keyframe: true,
        }
    }

    fn run(
        mut self,
        shared: &Shared,
        mailbox: &Mailbox,
        showing: &dyn Fn() -> bool,
        out: &SyncSender<Encoded>,
    ) {
        while !shared.stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            // Pace: nothing sooner than a frame interval after the last one.
            if let Some(sent) = self.last_sent {
                let ready = sent + FRAME_INTERVAL;
                if now < ready {
                    std::thread::sleep(ready - now);
                    continue;
                }
            }
            let wait = match self.last_sent {
                Some(sent) => (sent + STILL_FRAME).saturating_duration_since(now),
                None => STILL_FRAME,
            };
            match mailbox.take(wait.max(Duration::from_millis(1))) {
                Taken::Picture(captured) => {
                    let Some(picture) = picture::to_i420(&captured) else {
                        continue;
                    };
                    self.last = Some(picture);
                    self.send(shared, captured.captured_at, out);
                }
                Taken::Nothing => {
                    // A still window, sent again, but only while it can be seen.
                    let due = self
                        .last_sent
                        .is_some_and(|sent| Instant::now() >= sent + STILL_FRAME);
                    if due && self.last.is_some() && showing() {
                        self.send(shared, Instant::now(), out);
                    }
                }
                Taken::Ended => break,
            }
        }
    }

    /// Encode the last picture as captured at `captured_at`, and hand it to the transport.
    fn send(&mut self, shared: &Shared, captured_at: Instant, out: &SyncSender<Encoded>) {
        let Some(picture) = self.last.as_ref() else {
            return;
        };
        // Strictly increasing capture instants: a still-window resend can be stamped a moment
        // after a picture that was captured just before it but taken just after.
        let captured_at = match self.last_captured {
            Some(previous) if captured_at <= previous => previous + Duration::from_millis(1),
            _ => captured_at,
        };
        let (width, height) = (picture.width(), picture.height());
        if !matches!(&self.current, Some((_, w, h, _)) if (*w, *h) == (width, height)) {
            self.current = (self.make)(width, height).map(|e| (e, width, height, captured_at));
            self.keyframe = true;
        }
        let Some((encoder, _, _, opened)) = self.current.as_mut() else {
            return;
        };
        // The page's request is consumed either way, so a frame that was already going to be a
        // keyframe answers it too.
        let requested = shared.keyframe.swap(false, Ordering::Relaxed);
        let keyframe = self.keyframe || requested;
        let at = captured_at.saturating_duration_since(*opened);
        let Some(packets) = encoder.encode(picture, at, keyframe) else {
            // A failed encode: start over with a fresh encoder and a keyframe.
            self.current = None;
            return;
        };
        self.keyframe = false;
        self.last_sent = Some(Instant::now());
        self.last_captured = Some(captured_at);
        for packet in packets {
            let frame = Encoded {
                keyframe: packet.keyframe,
                data: packet.data,
                captured_at,
            };
            match out.try_send(frame) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => self.keyframe = true,
                Err(TrySendError::Disconnected(_)) => shared.stop.store(true, Ordering::Relaxed),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An encoder that encodes nothing: one packet per picture, naming the picture's size, its
    /// first luma byte and whether it was asked for a keyframe.
    struct Fake;

    impl Encode for Fake {
        fn encode(&mut self, picture: &I420, _: Duration, keyframe: bool) -> Option<Vec<Packet>> {
            Some(vec![Packet {
                data: vec![
                    picture.width() as u8,
                    picture.height() as u8,
                    picture.planes()[0].0[0],
                ],
                keyframe,
            }])
        }
    }

    fn fake() -> MakeEncoder {
        Box::new(|_, _| Some(Box::new(Fake) as Box<dyn Encode>))
    }

    fn grey(v: u8, side: u32, at: Instant) -> Bgra {
        Bgra {
            width: side,
            height: side,
            stride: side as usize * 4,
            pixels: [v, v, v, 255].repeat((side * side) as usize),
            captured_at: at,
        }
    }

    fn no_priority() {}

    fn wait_for(pipeline: &Pipeline, n: usize, within: Duration) -> Vec<Encoded> {
        let until = Instant::now() + within;
        let mut got = Vec::new();
        while got.len() < n && Instant::now() < until {
            got.extend(pipeline.take());
            std::thread::sleep(Duration::from_millis(5));
        }
        got
    }

    /// The first frame is a keyframe, carries its capture instant, and a changed window is encoded.
    #[test]
    fn a_picture_is_encoded_and_the_first_frame_is_a_keyframe() {
        let mailbox = Arc::new(Mailbox::default());
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), fake(), no_priority).unwrap();
        let at = Instant::now();
        mailbox.put(grey(0, 32, at));
        let got = wait_for(&pipeline, 1, Duration::from_secs(2));
        assert_eq!(got.len(), 1);
        assert!(got[0].keyframe);
        assert_eq!(got[0].captured_at, at);
        assert_eq!(&got[0].data[..2], &[32, 32]);
    }

    /// Drop, never queue: pictures that arrive while the encoder is paced out are replaced, and
    /// only the newest is encoded.
    #[test]
    fn only_the_newest_picture_is_encoded() {
        let mailbox = Arc::new(Mailbox::default());
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), fake(), no_priority).unwrap();
        mailbox.put(grey(0, 32, Instant::now()));
        assert_eq!(wait_for(&pipeline, 1, Duration::from_secs(2)).len(), 1);
        // Three pictures inside one frame interval: black, mid, white.
        for v in [0u8, 128, 255] {
            mailbox.put(grey(v, 32, Instant::now()));
        }
        let got = wait_for(&pipeline, 1, Duration::from_secs(2));
        assert_eq!(got.len(), 1, "queued instead of replaced: {got:?}");
        assert!(got[0].data[2] >= 230, "not the newest picture: {got:?}");
    }

    /// A still window is sent again, at least twice a second, stamped as it is sent, and never
    /// faster than the frame interval. CONTROL for the next test: `showing` is true.
    #[test]
    fn a_still_window_is_sent_again_twice_a_second() {
        let mailbox = Arc::new(Mailbox::default());
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), fake(), no_priority).unwrap();
        let first = Instant::now();
        mailbox.put(grey(0, 32, first));
        let got = wait_for(&pipeline, 4, Duration::from_millis(2500));
        assert!(got.len() >= 4, "only {} frames in 2.5 s", got.len());
        for pair in got.windows(2) {
            let gap = pair[1].captured_at - pair[0].captured_at;
            // Twice a second, with room for a loaded test machine's scheduling.
            assert!(gap <= STILL_FRAME + Duration::from_millis(250), "{gap:?}");
            assert!(gap >= FRAME_INTERVAL.saturating_sub(Duration::from_millis(1)));
        }
        assert!(got[1].captured_at > first, "a resend kept the old stamp");
        assert!(!got[1].keyframe, "a resend is not a keyframe");
    }

    /// A window that cannot be seen (minimized) sends nothing more: an old picture is never
    /// presented as current.
    #[test]
    fn a_window_that_is_not_showing_is_not_sent_again() {
        let mailbox = Arc::new(Mailbox::default());
        let showing = Arc::new(AtomicBool::new(true));
        let seen = showing.clone();
        let pipeline = Pipeline::start(
            mailbox.clone(),
            Box::new(move || seen.load(Ordering::Relaxed)),
            fake(),
            no_priority,
        )
        .unwrap();
        mailbox.put(grey(0, 32, Instant::now()));
        assert_eq!(wait_for(&pipeline, 1, Duration::from_secs(2)).len(), 1);
        showing.store(false, Ordering::Relaxed);
        std::thread::sleep(STILL_FRAME * 3);
        assert!(pipeline.take().is_empty(), "a hidden window was sent again");
    }

    /// The page's keyframe request is honoured on the next frame, once.
    #[test]
    fn a_keyframe_request_is_answered_once() {
        let mailbox = Arc::new(Mailbox::default());
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), fake(), no_priority).unwrap();
        mailbox.put(grey(0, 32, Instant::now()));
        assert!(wait_for(&pipeline, 1, Duration::from_secs(2))[0].keyframe);
        std::thread::sleep(FRAME_INTERVAL);
        mailbox.put(grey(1, 32, Instant::now()));
        let second = wait_for(&pipeline, 1, Duration::from_secs(2));
        assert!(!second[0].keyframe, "control: an ordinary frame");
        pipeline.request_keyframe();
        std::thread::sleep(FRAME_INTERVAL);
        mailbox.put(grey(2, 32, Instant::now()));
        assert!(wait_for(&pipeline, 1, Duration::from_secs(2))[0].keyframe);
        std::thread::sleep(FRAME_INTERVAL);
        mailbox.put(grey(3, 32, Instant::now()));
        assert!(!wait_for(&pipeline, 1, Duration::from_secs(2))[0].keyframe);
    }

    /// A new window size opens a new encoder, and its first frame is a keyframe.
    #[test]
    fn a_new_size_opens_a_new_encoder_with_a_keyframe() {
        let mailbox = Arc::new(Mailbox::default());
        let opened = Arc::new(Mutex::new(Vec::new()));
        let log = opened.clone();
        let make: MakeEncoder = Box::new(move |w, h| {
            log.lock().unwrap().push((w, h));
            Some(Box::new(Fake) as Box<dyn Encode>)
        });
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), make, no_priority).unwrap();
        mailbox.put(grey(0, 32, Instant::now()));
        wait_for(&pipeline, 1, Duration::from_secs(2));
        std::thread::sleep(FRAME_INTERVAL);
        mailbox.put(grey(0, 64, Instant::now()));
        let got = wait_for(&pipeline, 1, Duration::from_secs(2));
        assert!(got[0].keyframe);
        assert_eq!(&got[0].data[..2], &[64, 64]);
        assert_eq!(*opened.lock().unwrap(), vec![(32, 32), (64, 64)]);
    }

    /// The transport stopped taking frames: the ones it cannot take are dropped, and the first it
    /// takes after that is a keyframe, so the page's decoder recovers instead of decoding garbage.
    #[test]
    fn a_frame_the_transport_could_not_take_makes_the_next_a_keyframe() {
        let mailbox = Arc::new(Mailbox::default());
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), fake(), no_priority).unwrap();
        // Ten pictures, a frame interval apart, and nothing taken: the output holds four, and at
        // least one more is encoded and dropped even if a busy machine merges a few pictures.
        for v in 0..10u8 {
            mailbox.put(grey(v, 32, Instant::now()));
            std::thread::sleep(FRAME_INTERVAL + Duration::from_millis(15));
        }
        let held = pipeline.take();
        assert_eq!(held.len(), OUTPUT_DEPTH, "{held:?}");
        std::thread::sleep(FRAME_INTERVAL);
        mailbox.put(grey(9, 32, Instant::now()));
        let next = wait_for(&pipeline, 1, Duration::from_secs(2));
        assert!(next[0].keyframe, "the decoder was left without a keyframe");
    }

    /// The capture ended: the thread ends too, and says so.
    #[test]
    fn the_pipeline_ends_with_its_capture() {
        let mailbox = Arc::new(Mailbox::default());
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), fake(), no_priority).unwrap();
        assert!(!pipeline.ended());
        mailbox.end();
        let until = Instant::now() + Duration::from_secs(2);
        while !pipeline.ended() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(pipeline.ended());
    }

    /// Capture instants go forward even when a resend was stamped after a picture that was
    /// captured before it.
    #[test]
    fn capture_instants_only_go_forward() {
        let mailbox = Arc::new(Mailbox::default());
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), fake(), no_priority).unwrap();
        let early = Instant::now();
        mailbox.put(grey(0, 32, early));
        wait_for(&pipeline, 1, Duration::from_secs(2));
        std::thread::sleep(FRAME_INTERVAL);
        // Captured "before" the first one, as a late-arriving picture can be.
        mailbox.put(grey(1, 32, early));
        let got = wait_for(&pipeline, 1, Duration::from_secs(2));
        assert!(got[0].captured_at > early);
    }
}
