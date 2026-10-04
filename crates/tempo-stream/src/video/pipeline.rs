//! The encoder thread: the newest captured picture in, VP8 frames out, on the station's terms.
//!
//! - **Drop, never queue.** The capture writes into a one-picture [`Mailbox`]; a picture the
//!   encoder has not reached yet is replaced by the next one, so a slow encoder shows the page the
//!   newest picture late rather than every picture later and later.
//! - **At most 30 frames a second** ([`FRAME_INTERVAL`]), however often the window changes, and
//!   fewer for a picture so large that 30 would pass [`PIXEL_RATE`] ([`frame_interval`]): the most
//!   the encoder took before the picture followed the page's size, so a larger picture costs the
//!   station's CPU frames a second, not more of the CPU. Fewer again on a link too weak for them
//!   (`rate::PACES`).
//! - **A still window is sent at least twice a second** ([`STILL_FRAME_MS`]). The capture delivers
//!   a picture only when the window changes, and the page's freshness rule (S9) needs frames to
//!   echo, so the last picture is encoded again, stamped now. That is honest because nothing
//!   changed: the picture IS the window as of now. It stops the moment the window is not showing
//!   (minimized, or closed), or changes into something that gives no picture (dragged too small
//!   to encode, or a frame that could not be read: [`Mailbox::lost`]), so a page never sees an
//!   old picture presented as current.
//! - **A frame the transport cannot take is not silently lost.** The output holds a few frames;
//!   when it is full the frame is dropped and the next one is a keyframe, because every VP8 frame
//!   after a lost one decodes wrong until a keyframe arrives.
//! - **The capture instant rides with each frame**, strictly increasing, because it becomes the RTP
//!   timestamp the page echoes and the freshness rule reads (`frame_clock`).
//! - **At the size the page shows** ([`Pipeline::set_bound`], `picture::Bound`). Each picture is
//!   converted at the window's size, then scaled once to what the page's view and the path allow;
//!   the scaled copy is kept for a still window's resends. A new bound takes effect on the next
//!   frame, a still window's resend included. A new size opens a new encoder, whose first frame is
//!   a keyframe; a new bit rate retargets the running one ([`Encode::set_kbps`]), so a link that
//!   moves costs no keyframe (`rate`, which plans each frame from the bound and the session's
//!   estimate of the link). Nothing about the bound can stop the frames: the still-window floor
//!   holds through every change.
//! - **Below the radio.** The thread lowers its own priority on Windows (the caller's `lower`).
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::picture::{self, Bgra, Bound, I420};
use super::rate::Rate;
use crate::protocol::STILL_FRAME_MS;

/// The shortest time between two encoded frames: 30 a second.
pub const FRAME_INTERVAL: Duration = Duration::from_micros(33_334);
/// The most pixels a second the encoder is given: 2560×1600 at 30 frames a second.
pub const PIXEL_RATE: u64 = 2560 * 1600 * 30;

/// The shortest time between two frames of a `width`×`height` picture: [`FRAME_INTERVAL`], or
/// longer when 30 a second would pass [`PIXEL_RATE`]. A still window's floor is far inside it.
pub fn frame_interval(width: u32, height: u32) -> Duration {
    let pixels = u64::from(width) * u64::from(height);
    FRAME_INTERVAL.max(Duration::from_nanos(pixels * 1_000_000_000 / PIXEL_RATE))
}
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

    /// Give the running encoder a new bit rate, in kbit/s, from its next frame on and with no
    /// keyframe. False when it cannot, and then a new encoder is opened at that rate instead.
    fn set_kbps(&mut self, kbps: u32) -> bool {
        let _ = kbps;
        false
    }
}

/// Makes an encoder for a picture's width and height and a bit rate in kbit/s. Called again
/// whenever any of them changes.
pub type MakeEncoder = Box<dyn FnMut(u32, u32, u32) -> Option<Box<dyn Encode>> + Send>;

/// The width, height and bit rate an encoder was opened for.
type Opening = (u32, u32, u32);

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
    /// The window changed into something that gives no picture.
    lost: bool,
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
    /// The window changed, but gave no picture: the last one is no longer the window.
    Lost,
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
        slot.lost = false;
        self.ready.notify_one();
    }

    /// The window changed into something that cannot be sent (too small to encode, or a frame
    /// that could not be read). Whatever picture the encoder holds is no longer the window, so it
    /// is not sent again until a new one arrives.
    pub fn lost(&self) {
        let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        slot.picture = None;
        slot.lost = true;
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
            .wait_timeout_while(slot, wait, |s| s.picture.is_none() && !s.lost && !s.ended)
            .unwrap_or_else(|p| p.into_inner());
        match slot.picture.take() {
            Some(picture) => Taken::Picture(picture),
            None if slot.lost => {
                slot.lost = false;
                Taken::Lost
            }
            None if slot.ended => Taken::Ended,
            None => Taken::Nothing,
        }
    }
}

struct Shared {
    stop: AtomicBool,
    keyframe: AtomicBool,
    bound: Mutex<Bound>,
    /// The last plan's `wanted`, in kbit/s; 0 before the first.
    wanted: AtomicU32,
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
            bound: Mutex::new(Bound::default()),
            wanted: AtomicU32::new(0),
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

    /// What the picture may be encoded at from the next frame on: the page's view and the path's
    /// budget. Until the first call, the bound of a page whose view and path are not known yet.
    pub fn set_bound(&self, bound: Bound) {
        *self.shared.bound.lock().unwrap_or_else(|p| p.into_inner()) = bound;
    }

    /// The link the whole picture would need to be sent at its own budget, once a picture has
    /// been planned: what the session asks str0m's estimation to look for.
    pub fn wanted(&self) -> Option<u32> {
        match self.shared.wanted.load(Ordering::Relaxed) {
            0 => None,
            kbps => Some(kbps),
        }
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
    /// The encoder, what it was opened for, and when.
    current: Option<(Box<dyn Encode>, Opening, Instant)>,
    /// The last picture captured, at the window's size, kept to send again while the window is
    /// still.
    last: Option<I420>,
    /// `last` scaled to the size it was last sent at, when that is not the window's own.
    scaled: Option<I420>,
    last_sent: Option<Instant>,
    last_captured: Option<Instant>,
    keyframe: bool,
    /// What each frame is sent at, from the bound and the link.
    rate: Rate,
    /// The shortest time from the last frame sent to the next, as its plan said.
    interval: Duration,
}

impl Encoder {
    fn new(make: MakeEncoder) -> Self {
        Self {
            make,
            current: None,
            last: None,
            scaled: None,
            last_sent: None,
            last_captured: None,
            keyframe: true,
            rate: Rate::default(),
            interval: FRAME_INTERVAL,
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
            // Pace: nothing sooner than the last frame's interval after it.
            if let Some(sent) = self.last_sent {
                let ready = sent + self.interval;
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
                        // Nothing encodable (a window dragged too small): the old picture is not
                        // the window any more.
                        self.last = None;
                        continue;
                    };
                    self.last = Some(picture);
                    self.scaled = None;
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
                Taken::Lost => self.last = None,
                Taken::Ended => break,
            }
        }
    }

    /// Encode the last picture as captured at `captured_at`, at the size the bound allows, and hand
    /// it to the transport.
    fn send(&mut self, shared: &Shared, captured_at: Instant, out: &SyncSender<Encoded>) {
        let Some(source) = self.last.as_ref() else {
            return;
        };
        let bound = *shared.bound.lock().unwrap_or_else(|p| p.into_inner());
        let window = (source.width(), source.height());
        let Some(plan) = self.rate.plan(window, &bound, Instant::now()) else {
            return;
        };
        shared.wanted.store(plan.wanted, Ordering::Relaxed);
        let (width, height) = (plan.width, plan.height);
        let whole = (width, height) == (source.width(), source.height());
        if !whole
            && self
                .scaled
                .as_ref()
                .is_none_or(|s| (s.width(), s.height()) != (width, height))
        {
            self.scaled = Some(picture::scale(source, width, height));
        }
        let picture = match &self.scaled {
            Some(scaled) if !whole => scaled,
            _ => source,
        };
        // Strictly increasing capture instants: a still-window resend can be stamped a moment
        // after a picture that was captured just before it but taken just after.
        let captured_at = match self.last_captured {
            Some(previous) if captured_at <= previous => previous + Duration::from_millis(1),
            _ => captured_at,
        };
        // The same size goes on from the same encoder, given the new bit rate if there is one;
        // only a new size, or an encoder that cannot change its rate, opens another.
        let opening = (width, height, plan.kbps);
        let reuse = match self.current.as_mut() {
            Some((_, opened, _)) if *opened == opening => true,
            Some((encoder, opened, _)) if (opened.0, opened.1) == (width, height) => {
                let retargeted = encoder.set_kbps(plan.kbps);
                if retargeted {
                    opened.2 = plan.kbps;
                }
                retargeted
            }
            _ => false,
        };
        if !reuse {
            self.current =
                (self.make)(opening.0, opening.1, opening.2).map(|e| (e, opening, captured_at));
            self.keyframe = true;
        }
        self.interval = plan.interval;
        let Some((encoder, _, opened)) = self.current.as_mut() else {
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
    use crate::video::picture::{Path, MIN_KBPS};
    use crate::video::rate;

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
        Box::new(|_, _, _| Some(Box::new(Fake) as Box<dyn Encode>))
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

    /// A picture of `width`×`height`, all one grey.
    fn sized(v: u8, width: u32, height: u32, at: Instant) -> Bgra {
        Bgra {
            width,
            height,
            stride: width as usize * 4,
            pixels: [v, v, v, 255].repeat((width * height) as usize),
            captured_at: at,
        }
    }

    /// What every encoder a maker opened was opened for.
    type Opened = Arc<Mutex<Vec<Opening>>>;

    /// An encoder maker that records the width, height and bit rate of every encoder it opens.
    fn logged() -> (MakeEncoder, Opened) {
        let opened = Arc::new(Mutex::new(Vec::new()));
        let log = opened.clone();
        let make: MakeEncoder = Box::new(move |w, h, kbps| {
            log.lock().unwrap().push((w, h, kbps));
            Some(Box::new(Fake) as Box<dyn Encode>)
        });
        (make, opened)
    }

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

    /// The operator's case at the encoder (2026-10-03): a 3440×1440 window and a page on the shack's
    /// network that has not said its view. The encoder is opened for the whole window, at the
    /// network's bit rate, where it used to get half of each side.
    #[test]
    fn a_wide_window_reaches_the_encoder_whole_on_the_shacks_network() {
        let mailbox = Arc::new(Mailbox::default());
        let (make, opened) = logged();
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), make, no_priority).unwrap();
        pipeline.set_bound(Bound::new(Some(Path::Lan), None));
        mailbox.put(sized(0, 3440, 1440, Instant::now()));
        assert_eq!(wait_for(&pipeline, 1, Duration::from_secs(5)).len(), 1);
        assert_eq!(*opened.lock().unwrap(), vec![(3440, 1440, 4953)]);
    }

    /// The page says its view: the next frame, a still window's resend, is at the size that fits
    /// it, from a new encoder, and is a keyframe. The still-window floor holds through the change.
    /// CONTROL: the same bound again opens nothing, and the resends after it are ordinary frames.
    #[test]
    fn a_new_bound_resizes_the_next_frame_and_the_frames_keep_coming() {
        let mailbox = Arc::new(Mailbox::default());
        let (make, opened) = logged();
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), make, no_priority).unwrap();
        mailbox.put(sized(0, 960, 540, Instant::now()));
        let mut got = wait_for(&pipeline, 1, Duration::from_secs(2));
        assert!(got[0].keyframe);
        pipeline.set_bound(Bound::new(Some(Path::Lan), Some((480, 480))));
        let resized = wait_for(&pipeline, 1, STILL_FRAME * 3);
        assert!(
            resized[0].keyframe,
            "the first frame at a new size is not a keyframe"
        );
        pipeline.set_bound(Bound::new(Some(Path::Lan), Some((480, 480))));
        let still = wait_for(&pipeline, 2, STILL_FRAME * 3);
        assert!(
            still.len() >= 2 && still.iter().all(|f| !f.keyframe),
            "{still:?}"
        );
        assert_eq!(
            *opened.lock().unwrap(),
            vec![(960, 540, 518), (480, 270, MIN_KBPS)]
        );
        got.extend(resized);
        got.extend(still);
        for pair in got.windows(2) {
            let gap = pair[1].captured_at - pair[0].captured_at;
            assert!(gap <= STILL_FRAME + Duration::from_millis(250), "{gap:?}");
        }
    }

    /// An encoder that records every bit rate it is given after it was opened.
    struct Retargetable(Arc<Mutex<Vec<u32>>>);

    impl Encode for Retargetable {
        fn encode(&mut self, picture: &I420, at: Duration, keyframe: bool) -> Option<Vec<Packet>> {
            Fake.encode(picture, at, keyframe)
        }

        fn set_kbps(&mut self, kbps: u32) -> bool {
            self.0.lock().unwrap().push(kbps);
            true
        }
    }

    /// A maker of [`Retargetable`] encoders: what each was opened for, and the rates given after.
    fn retargetable() -> (MakeEncoder, Opened, Arc<Mutex<Vec<u32>>>) {
        let (opened, retargets) = (
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
        );
        let (log, given) = (opened.clone(), retargets.clone());
        let make: MakeEncoder = Box::new(move |w, h, kbps| {
            log.lock().unwrap().push((w, h, kbps));
            Some(Box::new(Retargetable(given.clone())) as Box<dyn Encode>)
        });
        (make, opened, retargets)
    }

    /// On the shack's network, with the session's estimate of the link.
    fn linked(estimate: u32) -> Bound {
        Bound::new(Some(Path::Lan), None).within(Some(estimate))
    }

    /// ★ The link narrows (2026-10-03): the next frame, a still window's resend, goes at the new
    /// bit rate from the same encoder, as an ordinary frame, so a weak link pays for no keyframe.
    /// CONTROL: a link so weak that the size steps down opens a new encoder at the new size, at the
    /// floor's bit rate, and its first frame is a keyframe.
    #[test]
    fn a_new_bit_rate_retargets_the_running_encoder_without_a_keyframe() {
        let mailbox = Arc::new(Mailbox::default());
        let (make, opened, retargets) = retargetable();
        let pipeline =
            Pipeline::start(mailbox.clone(), Box::new(|| true), make, no_priority).unwrap();
        pipeline.set_bound(linked(8000));
        mailbox.put(sized(0, 960, 540, Instant::now()));
        assert!(wait_for(&pipeline, 1, Duration::from_secs(2))[0].keyframe);
        pipeline.set_bound(linked(500));
        let next = wait_for(&pipeline, 1, STILL_FRAME * 3);
        assert!(!next[0].keyframe, "a new bit rate cost a keyframe");
        assert_eq!(*opened.lock().unwrap(), vec![(960, 540, 518)]);
        assert_eq!(*retargets.lock().unwrap(), vec![rate::share(500)]);
        // Once the link has been measured (the still window's resends taken meanwhile), a link
        // too weak for the whole picture's keyframes.
        wait_for(&pipeline, usize::MAX, rate::MEASURE);
        pipeline.set_bound(linked(100));
        // The first frame at the new size is a keyframe.
        let frames = wait_for(&pipeline, usize::MAX, STILL_FRAME * 3);
        let stepped = frames
            .iter()
            .find(|f| f.data[..2] == [(480u32 & 0xff) as u8, (270u32 & 0xff) as u8])
            .expect("the size never stepped down");
        assert!(
            stepped.keyframe,
            "the first frame at a new size is not a keyframe"
        );
        assert_eq!(
            opened.lock().unwrap().last(),
            Some(&(480, 270, rate::FLOOR_KBPS))
        );
        // What the session asks str0m to look for: the link the whole window would need.
        assert_eq!(pipeline.wanted(), Some(rate::link_for(518)));
    }

    /// A weak link lowers the frame rate: a window that changes all the time is encoded at most 15
    /// times a second when the share is under half the picture's budget (a small picture's budget
    /// is the 300 kbit/s floor, and a 200 kbit/s link's share is 116). CONTROL: on a wide link the
    /// same window goes faster.
    #[test]
    fn a_weak_link_lowers_the_frame_rate() {
        for (estimate, slow) in [(200, true), (8000, false)] {
            let mailbox = Arc::new(Mailbox::default());
            let pipeline =
                Pipeline::start(mailbox.clone(), Box::new(|| true), fake(), no_priority).unwrap();
            pipeline.set_bound(linked(estimate));
            let mut got = Vec::new();
            let until = Instant::now() + Duration::from_millis(1500);
            let mut v = 0u8;
            while Instant::now() < until {
                v = v.wrapping_add(1);
                mailbox.put(grey(v, 32, Instant::now()));
                std::thread::sleep(Duration::from_millis(5));
                got.extend(pipeline.take());
            }
            let gaps: Vec<Duration> = got
                .windows(2)
                .map(|p| p[1].captured_at - p[0].captured_at)
                .collect();
            let shortest = gaps.iter().min().copied().unwrap();
            if slow {
                assert!(
                    shortest >= rate::PACES[1] - Duration::from_millis(6),
                    "{estimate}: {gaps:?}"
                );
            } else {
                assert!(shortest < rate::PACES[1], "{estimate}: {gaps:?}");
            }
        }
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

    /// Up to 2560×1600, 30 frames a second; past it, fewer, so the encoder never takes more pixels
    /// a second than that. Every one of them far inside the still window's twice a second.
    #[test]
    fn a_picture_past_the_pixel_rate_gets_fewer_frames() {
        assert_eq!(frame_interval(1920, 1080), FRAME_INTERVAL);
        assert_eq!(frame_interval(2560, 1600), FRAME_INTERVAL);
        assert_eq!(frame_interval(3440, 1440), Duration::from_nanos(40_312_500));
        assert_eq!(frame_interval(3840, 2160), Duration::from_nanos(67_500_000));
        let lan = Bound::new(Some(Path::Lan), None);
        for (w, h) in [(3440, 1440), (3840, 2160), (7680, 4320)] {
            let (w, h) = picture::encoded_size(w, h, &lan).unwrap();
            assert!(frame_interval(w, h) * 4 < STILL_FRAME, "{w}x{h}");
        }
    }

    /// A new window size opens a new encoder, and its first frame is a keyframe.
    #[test]
    fn a_new_size_opens_a_new_encoder_with_a_keyframe() {
        let mailbox = Arc::new(Mailbox::default());
        let opened = Arc::new(Mutex::new(Vec::new()));
        let log = opened.clone();
        let make: MakeEncoder = Box::new(move |w, h, _| {
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

    /// The window changed into something that gives no picture (a frame that could not be
    /// read, or a window dragged too small to encode): the old picture is not sent again as if
    /// it were the window. CONTROL: a new picture brings the stream back.
    #[test]
    fn a_lost_or_unencodable_picture_is_not_sent_again() {
        for lose in [(|m: &Mailbox| m.lost()) as fn(&Mailbox), |m: &Mailbox| {
            m.put(grey(0, 8, Instant::now()))
        }] {
            let mailbox = Arc::new(Mailbox::default());
            let pipeline =
                Pipeline::start(mailbox.clone(), Box::new(|| true), fake(), no_priority).unwrap();
            mailbox.put(grey(0, 32, Instant::now()));
            assert_eq!(wait_for(&pipeline, 1, Duration::from_secs(2)).len(), 1);
            lose(&mailbox);
            std::thread::sleep(STILL_FRAME * 3);
            assert!(pipeline.take().is_empty(), "the old picture was sent again");
            mailbox.put(grey(1, 32, Instant::now()));
            assert_eq!(wait_for(&pipeline, 1, Duration::from_secs(2)).len(), 1);
        }
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
