//! The audio road, this computer's end: a UDP socket of its own to the road the station named in
//! its welcome (`remote_service::lan::audio_road`), or on the internet a road of its own over ICE
//! (`crate::ice`), keyed from the command link's TLS session under the audio road's own label, and one thread that carries this computer's headset
//! (`tempo_codec::headset`) both ways. It shares nothing with Stop's road: its own socket, thread
//! and keys, and no lock a Stop takes.
//!
//! - **The station's audio.** While the link is open and this computer holds the lease, the thread
//!   says `listen` every [`LISTEN_EVERY`]. Each frame that comes back is decoded in the order of its
//!   number: an older one is dropped, a gap is played as the headset's quiet bed for as long as
//!   the missing frames would have lasted, and a new capture epoch starts afresh. Resampled to the
//!   headset's rate, it plays through the headset's cushion, muted, never ducked, while this
//!   computer's over may be on the air (M9: its press, the station saying its over is keyed, or the
//!   station's rig keyed; [`OnAir`]).
//! - **The microphone.** Only while a press is live at this computer ([`OnAir::pressing`]): the
//!   headset's microphone, resampled to 48 kHz and cut into 20 ms frames, each encoded
//!   (`MicEncoder`: voice, 32 kbit/s constant, no DTX, no FEC) and sent at once, one a datagram,
//!   numbered from the press's start on the capture's own clock, so the station places each frame
//!   on the over's clock by its number. A press that went on from the one speaking (an answered
//!   Lock, [`OnAir::went_on_from`]) carries on its numbers, its encoder and its clock. ⛔ NOTHING IS EVER QUEUED: a frame that cannot go now, or
//!   that is already later than the station's lead-in could still play ([`LATE`]), is dropped and
//!   its number skipped, so after a stall the station plays a hole where the stall was, never a
//!   burst. The press is client mode's, a held press on Stop's road (src-tauri's `client_mode`), and
//!   the station takes a frame only for the press its held PTT keyed for this computer.
//! - **The over's end, heard here.** When Stop's road hears an over end other than by the
//!   operator letting go, or a press is held with no word from the station for half a second, the
//!   headset plays a short tone. The tone goes to the headset's output alone, never into the
//!   microphone path.
//! - **A voice message, recorded here** ([`Recording`]): the same microphone, while the voice
//!   keyer records, folded to the keyer's 12 kHz on a thread of its own. Client mode sends it to
//!   the shack's slot as an import; nothing of it is transmitted from here.
//!
//! The keys are made from the link's session, held in memory only, never written, logged or shown,
//! and go when the road's thread ends, which it does as soon as its link has ended.
//!
//! What the road carries, either way, is [`wire`]'s, which the shack reads and writes too. This
//! computer's own sound devices behind the headset (`headset`, `devices`) are the `device`
//! feature's; without it the road runs on any [`Headset`].
/// What the audio road carries, as both ends read and write it.
pub mod wire;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tempo_codec::capture_resample::CaptureResampler;
use tempo_codec::headset::BED_LEVEL;
use tempo_codec::mic_encode::{MicEncoder, MIC_ENCODE_RATE_HZ, MIC_FRAME_MS, MIC_FRAME_SAMPLES};
use tempo_codec::receive_decode::{ReceiveDecoder, PLAY_FRAME_SAMPLES, PLAY_RATE_HZ};

use self::wire::{Down, Up};
use super::stop_road::OnAir;
use crate::datagram::{End, Keys, Opener, Sealer, Tag, AUDIO_MAX, EXPORTED};
use crate::ice::{IceRoad, Path};

pub use tempo_codec::headset::{
    sound, Rings, Sound, CUSHION_MS, GAIN_DB as VOLUME_DB, INTERNET_CUSHION_MS, VOLUME_DB_MAX,
    VOLUME_DB_MIN,
};
/// This computer's headset for client mode (`tempo_audio::headset::Headset`).
#[cfg(feature = "device")]
pub type ThisHeadset = tempo_audio::headset::Headset;

/// How often this computer says `listen` while it wants the station's audio: four in the
/// station's two seconds.
pub const LISTEN_EVERY: Duration = Duration::from_millis(500);
/// A microphone frame later than this, by the press's own clock, is dropped rather than sent: the
/// station's lead-in, past which it could no longer play it at its moment anyway.
pub const LATE: Duration = Duration::from_millis(60);
/// How long a send may take before its frame is dropped.
pub const SEND_WAIT: Duration = Duration::from_millis(1);
/// The longest the thread waits for the station before it looks at its own work again.
pub const TICK: Duration = Duration::from_millis(10);
/// The longest gap in the station's frames played as the bed; a longer one runs the cushion dry.
const BED_FRAMES: u32 = 8;

/// The longest voice message recorded here: a minute, for the keyer's short calls. Its import
/// stays well inside the command link's largest call.
pub const RECORD_MAX: Duration = Duration::from_secs(60);

/// A voice message being recorded on this computer's microphone for a slot of the shack's voice
/// keyer: the headset's input, kept open while it runs and taken every [`TICK`], folded to the
/// keyer's 12 kHz, for at most [`RECORD_MAX`]. Finished, the voice is the caller's to send; dropped,
/// nothing is kept.
pub struct Recording {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Vec<f32>>>,
}

impl Recording {
    /// A recording on `headset` from now, on a thread of its own; `None` if none could start.
    pub fn start(headset: Arc<dyn Headset>) -> Option<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::Builder::new()
            .name("nexus-client-record".into())
            .spawn(move || record(&*headset, &stopping))
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }

    /// The voice recorded, at 12 kHz, and the recording over.
    pub fn finish(mut self) -> Vec<f32> {
        self.stop.store(true, Ordering::Release);
        self.thread
            .take()
            .and_then(|thread| thread.join().ok())
            .unwrap_or_default()
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// A recorded message as the shack's voice keyer imports one: a 12 kHz mono WAV, whole.
pub fn voice_message(voice: &[f32]) -> std::io::Result<Vec<u8>> {
    tempo_codec::voice::wav_12k(voice)
}

/// The recording's thread: what the microphone captured from its start, at the keyer's rate,
/// until it is stopped or reaches [`RECORD_MAX`].
fn record(headset: &dyn Headset, stop: &AtomicBool) -> Vec<f32> {
    let rings = headset.rings();
    let most = tempo_codec::voice::VOICE_RATE as usize * RECORD_MAX.as_secs() as usize;
    // Nothing from before Record was pressed.
    let _ = rings.take_mic();
    let mut voice = Vec::new();
    let mut from: Option<(u64, u32, CaptureResampler)> = None;
    let mut take = |voice: &mut Vec<f32>| {
        let captured = rings.take_mic();
        let Some(rate) = rings.in_rate() else {
            return;
        };
        let opened = rings.opened();
        if from
            .as_ref()
            .is_none_or(|(at, was, _)| *at != opened || *was != rate)
        {
            from = Some((
                opened,
                rate,
                CaptureResampler::new(rate, tempo_codec::voice::VOICE_RATE),
            ));
        }
        if let Some((_, _, resampler)) = from.as_mut() {
            voice.extend(resampler.process(&captured));
        }
    };
    while !stop.load(Ordering::Acquire) && voice.len() < most {
        headset.want_input();
        take(&mut voice);
        std::thread::sleep(TICK);
    }
    take(&mut voice);
    voice.truncate(most);
    voice
}

/// This computer's headset, as an audio road uses it: its rings, and a direction kept open a
/// while longer. The real one in client mode (`tempo_audio::headset::Headset`); a stand-in in a
/// test, which opens no device.
pub trait Headset: Send + Sync + 'static {
    fn rings(&self) -> &Arc<Rings>;
    /// Keep the output open a while longer: the road is listening, or a sound is to play.
    fn want_output(&self);
    /// Keep the microphone open a while longer: a press is live.
    fn want_input(&self);
}

/// How long each want keeps a direction open: a want renewed every half second outlives it.
#[cfg(feature = "device")]
const WANT: Duration = Duration::from_millis(1500);

#[cfg(feature = "device")]
impl Headset for tempo_audio::headset::Headset {
    fn rings(&self) -> &Arc<Rings> {
        tempo_audio::headset::Headset::rings(self)
    }

    fn want_output(&self) {
        tempo_audio::headset::Headset::want_output(self, WANT);
    }

    fn want_input(&self) {
        tempo_audio::headset::Headset::want_input(self, WANT);
    }
}

/// This computer's headset for client mode, on the devices chosen ("" for the system default),
/// opening nothing until a road or the meter wants it. `None` if its thread would not start.
#[cfg(feature = "device")]
pub fn headset(output: &str, input: &str) -> Option<Arc<tempo_audio::headset::Headset>> {
    tempo_audio::headset::Headset::start(output, input).map(Arc::new)
}

/// Devices as This computer ▸ Audio lists them: each a name to keep and a label to show.
pub type Listed = Vec<(String, String)>;

/// This computer's audio devices, for This computer ▸ Audio: `(inputs, outputs)`.
#[cfg(feature = "device")]
pub fn devices() -> (Listed, Listed) {
    let (inputs, outputs) = tempo_audio::device::available_devices();
    let named = |list: Vec<tempo_audio::audiodev::AudioDevice>| {
        list.into_iter()
            .map(|device| (device.name, device.label))
            .collect::<Vec<_>>()
    };
    (named(inputs), named(outputs))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Shared {
    /// The road's way to the station: the LAN's socket, or its internet road.
    path: Path,
    /// The sending half of the keys, until the road's thread ends and drops it.
    send: Mutex<Option<Sealer>>,
    headset: Arc<dyn Headset>,
    on_air: Arc<OnAir>,
    lease: Mutex<Option<String>>,
    /// The link this road was made for has ended.
    ended: AtomicBool,
    /// How many turns the road's thread has finished: a test waits on it to know the road has
    /// acted on what changed.
    #[cfg(test)]
    turns: std::sync::atomic::AtomicU64,
    /// The press's clock, once a test holds it: each microphone frame is judged by this instant in
    /// place of the machine's, so a frame is late only by what the test makes it.
    #[cfg(test)]
    clock: Mutex<Option<Instant>>,
}

/// This computer's end of one link's audio road. Cheap to share: every holder is the same road.
pub struct AudioRoad {
    shared: Arc<Shared>,
}

impl AudioRoad {
    /// The road to `to`, tagged `tag`, under the keys the link's exporter gave (`exported`, under
    /// `datagram::AUDIO_LABEL` with the welcome's session as the context), on `headset`, with the
    /// link's [`OnAir`]. `None` if no socket could be had or no thread started.
    pub fn open(
        to: SocketAddr,
        tag: Tag,
        exported: &[u8; EXPORTED],
        headset: Arc<dyn Headset>,
        on_air: Arc<OnAir>,
    ) -> Option<Arc<Self>> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        // Connected: only the station's road is heard on it.
        socket.connect(to).ok()?;
        socket.set_read_timeout(Some(TICK)).ok()?;
        socket.set_write_timeout(Some(SEND_WAIT)).ok()?;
        Self::on(Path::Lan(socket), tag, exported, headset, on_air)
    }

    /// The road over the internet: on `ice` (bound with [`TICK`] and written with [`SEND_WAIT`], its
    /// pair found by ICE, and heard only there), tagged `tag`, under the keys the link's exporter
    /// gave for an internet audio road (`datagram::INTERNET_AUDIO_LABEL`), on `headset`.
    pub fn internet(
        ice: Arc<IceRoad>,
        tag: Tag,
        exported: &[u8; EXPORTED],
        headset: Arc<dyn Headset>,
        on_air: Arc<OnAir>,
    ) -> Option<Arc<Self>> {
        Self::on(Path::Internet(ice), tag, exported, headset, on_air)
    }

    fn on(
        path: Path,
        tag: Tag,
        exported: &[u8; EXPORTED],
        headset: Arc<dyn Headset>,
        on_air: Arc<OnAir>,
    ) -> Option<Arc<Self>> {
        let Keys { send, receive } = Keys::sized(exported, tag, End::Computer, AUDIO_MAX)?;
        let shared = Arc::new(Shared {
            path,
            send: Mutex::new(Some(send)),
            headset,
            on_air,
            lease: Mutex::new(None),
            ended: AtomicBool::new(false),
            #[cfg(test)]
            turns: Default::default(),
            #[cfg(test)]
            clock: Default::default(),
        });
        let theirs = shared.clone();
        std::thread::Builder::new()
            .name("nexus-client-audio-road".into())
            .spawn(move || run(theirs, receive))
            .ok()?;
        Some(Arc::new(Self { shared }))
    }

    /// The lease `listen` names, or none while this computer holds none (and then it listens not).
    pub fn lease(&self, lease: Option<String>) {
        *lock(&self.shared.lease) = lease;
    }

    /// The link this road was made for has ended: the road goes, its keys with it.
    pub fn link_ended(&self) {
        self.shared.ended.store(true, Ordering::Release);
    }

    /// How many turns the road's thread has finished.
    #[cfg(test)]
    fn turns(&self) -> u64 {
        self.shared.turns.load(Ordering::Acquire)
    }

    /// Holds the press's clock at `now`: every frame the road cuts from here is judged by it.
    #[cfg(test)]
    fn clock(&self, now: Instant) {
        *lock(&self.shared.clock) = Some(now);
    }
}

impl Shared {
    /// One message, sent now: whether it went on the socket in time.
    fn say(&self, up: &Up) -> bool {
        let sealed = lock(&self.send)
            .as_ref()
            .and_then(|send| send.seal(up.kind(), &up.body()));
        sealed.is_some_and(|datagram| self.path.send(&datagram))
    }
}

/// The station's audio, as this end plays it: the frame it expects next, and its decoder.
#[derive(Default)]
struct Playing {
    /// The capture epoch, and the number the next frame should carry.
    expect: Option<(u64, u32)>,
    decoder: Option<ReceiveDecoder>,
    /// To the headset's rate, for the headset as it is open now.
    to_headset: Option<(u64, u32, CaptureResampler)>,
    /// The bed's noise.
    noise: u32,
}

impl Playing {
    /// One frame from the station, played in its place, or dropped if it is not after the last.
    fn frame(&mut self, rings: &Rings, epoch: u64, seq: u32, packet: &[u8]) {
        let gap = match self.expect {
            Some((on, next)) if on == epoch => {
                if seq < next {
                    return; // older than what has played: dropped, never played late
                }
                seq - next
            }
            // A new capture: its own decoder, from its own first frame.
            _ => {
                self.decoder = ReceiveDecoder::new().ok();
                0
            }
        };
        self.expect = Some((epoch, seq.saturating_add(1)));
        let Some(samples) = self.decoder.as_mut().and_then(|d| d.decode(packet)) else {
            return;
        };
        let Some(rate) = rings.out_rate() else {
            return; // nowhere to play it yet
        };
        let opened = rings.opened();
        if self
            .to_headset
            .as_ref()
            .is_none_or(|(at, to, _)| *at != opened || *to != rate)
        {
            self.to_headset = Some((opened, rate, CaptureResampler::new(PLAY_RATE_HZ, rate)));
        }
        let Some((_, _, resampler)) = self.to_headset.as_mut() else {
            return;
        };
        // A known gap is the bed, for as long as the missing frames would have played.
        if gap > 0 && gap <= BED_FRAMES {
            let bed: Vec<f32> = (0..gap as usize * PLAY_FRAME_SAMPLES)
                .map(|_| {
                    self.noise = self
                        .noise
                        .wrapping_mul(1_664_525)
                        .wrapping_add(1_013_904_223);
                    ((self.noise >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * BED_LEVEL
                })
                .collect();
            rings.play(&resampler.process(&bed));
        }
        rings.play(&resampler.process(&samples));
    }
}

/// This computer's microphone in one press, as the thread sends it.
struct Speaking {
    press: u64,
    /// The number of the next frame; 1 for the press's first.
    frame: u32,
    /// When the press's first frame was cut: frame `n` belongs `n - 1` frames after it.
    started: Option<Instant>,
    encoder: MicEncoder,
    /// From the microphone's rate, for the microphone as it is open now.
    from_mic: Option<(u64, u32, CaptureResampler)>,
    /// 48 kHz samples not yet a whole frame.
    pending: Vec<f32>,
}

impl Speaking {
    fn new(press: u64) -> Option<Self> {
        Some(Self {
            press,
            frame: 1,
            started: None,
            encoder: MicEncoder::new().ok()?,
            from_mic: None,
            pending: Vec::new(),
        })
    }

    /// What the microphone captured since the last pass, cut into frames and each sent at once or
    /// dropped: never queued. Each frame is judged against the clock as it is about to go, so a
    /// stall anywhere before it reads as the lateness it is.
    fn pass(&mut self, shared: &Shared) {
        let rings = shared.headset.rings();
        let captured = rings.take_mic();
        let Some(rate) = rings.in_rate() else {
            return; // the microphone is not open yet
        };
        let opened = rings.opened();
        if self
            .from_mic
            .as_ref()
            .is_none_or(|(at, from, _)| *at != opened || *from != rate)
        {
            self.from_mic = Some((
                opened,
                rate,
                CaptureResampler::new(rate, MIC_ENCODE_RATE_HZ),
            ));
        }
        let Some((_, _, resampler)) = self.from_mic.as_mut() else {
            return;
        };
        self.pending.extend(resampler.process(&captured));
        while self.pending.len() >= MIC_FRAME_SAMPLES {
            let frame: Vec<f32> = self.pending.drain(..MIC_FRAME_SAMPLES).collect();
            let number = self.frame;
            self.frame = self.frame.saturating_add(1);
            let now = Instant::now();
            #[cfg(test)]
            let now = lock(&shared.clock).unwrap_or(now);
            let started = *self.started.get_or_insert(now);
            let due = started + Duration::from_millis(MIC_FRAME_MS * u64::from(number - 1));
            // Later than the station could still play: dropped, its number skipped, so the
            // station plays a hole where it was and nothing comes in a burst.
            if now.saturating_duration_since(due) > LATE {
                continue;
            }
            let Some(packet) = self.encoder.encode(&frame) else {
                continue;
            };
            // A frame the socket will not take now is dropped the same way.
            let _ = shared.say(&Up::Mic {
                press: self.press,
                frame: number,
                packet,
            });
        }
    }
}

/// The road's thread, until its link has ended (or nobody holds the road). The keys go with it.
fn run(shared: Arc<Shared>, receive: Opener) {
    serve(&shared, receive);
    lock(&shared.send).take();
    shared.headset.rings().listen(false);
}

fn serve(shared: &Arc<Shared>, mut receive: Opener) {
    let mut buffer = [0u8; 2048];
    let mut playing = Playing::default();
    let mut speaking: Option<Speaking> = None;
    let mut listened: Option<Instant> = None;
    let mut ends = shared.on_air.ended();
    let mut tone = false;
    // The press the station's silence was said for already, so it is said once a press.
    let mut no_word_for: Option<u64> = None;
    loop {
        let rings = shared.headset.rings();
        match shared.path.recv(&mut buffer) {
            Ok(length) => {
                let heard = receive
                    .open(&buffer[..length])
                    .and_then(|(kind, body)| Down::read(kind, &body));
                if let Some(Down::Rx { epoch, seq, packet }) = heard {
                    shared.on_air.heard();
                    playing.frame(rings, epoch, seq, &packet);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            // A refusal for an earlier datagram, or the socket's own trouble: a pause keeps an
            // error that repeats from spinning.
            Err(_) => std::thread::sleep(TICK),
        }
        let now = Instant::now();
        if shared.ended.load(Ordering::Acquire) || Arc::strong_count(shared) == 1 {
            return;
        }
        // M9: muted, never ducked, while the over may be on the air.
        rings.mute(shared.on_air.may_be_on_air());
        let lease = lock(&shared.lease).clone();
        rings.listen(lease.is_some());
        if let Some(lease) = lease {
            if listened.is_none_or(|at| now.saturating_duration_since(at) >= LISTEN_EVERY) {
                shared.say(&Up::Listen { lease });
                shared.headset.want_output();
                listened = Some(now);
            }
        }
        // The microphone, only while a press is live: a new press starts on fresh audio, and a
        // press that went on from the one speaking (an answered Lock) carries its audio on.
        #[cfg(any(test, feature = "test-util"))]
        let _sending = lock(&shared.on_air.sender);
        match shared.on_air.press() {
            Some(press) => {
                shared.headset.want_input();
                if speaking.as_ref().is_none_or(|s| s.press != press) {
                    let from = shared.on_air.went_on_from();
                    match speaking.as_mut().filter(|s| Some(s.press) == from) {
                        Some(going_on) => going_on.press = press,
                        None => {
                            let _ = rings.take_mic();
                            speaking = Speaking::new(press);
                            no_word_for = None;
                        }
                    }
                }
                if let Some(speaking) = speaking.as_mut() {
                    speaking.pass(shared);
                }
                if shared.on_air.silent_for(now) >= super::stop_road::NO_WORD
                    && no_word_for != Some(press)
                {
                    no_word_for = Some(press);
                    tone = true;
                }
            }
            None => speaking = None,
        }
        // An over Stop's road heard end: the tone, in this computer's headset only.
        let ended = shared.on_air.ended();
        if ended != ends {
            ends = ended;
            tone = true;
        }
        if tone {
            shared.headset.want_output();
            if let Some(rate) = rings.out_rate() {
                rings.sound(&sound(Sound::OverEnded, rate));
                tone = false;
            }
        }
        #[cfg(test)]
        shared.turns.fetch_add(1, Ordering::Release);
    }
}

/// A headset with no device behind it, for a test: rings open at 48 kHz both ways, and how often
/// each direction was wanted.
#[cfg(any(test, feature = "test-util"))]
pub struct StandIn {
    pub rings: Arc<Rings>,
    pub outputs: std::sync::atomic::AtomicU32,
    pub inputs: std::sync::atomic::AtomicU32,
}

#[cfg(any(test, feature = "test-util"))]
impl StandIn {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            rings: Arc::new(Rings::stand_in(48_000, 48_000)),
            outputs: Default::default(),
            inputs: Default::default(),
        })
    }
}

#[cfg(any(test, feature = "test-util"))]
impl Headset for StandIn {
    fn rings(&self) -> &Arc<Rings> {
        &self.rings
    }

    fn want_output(&self) {
        self.outputs.fetch_add(1, Ordering::SeqCst);
    }

    fn want_input(&self) {
        self.inputs.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stop_road::wire as shack_stop;
    use crate::stop_road::{Over, StopRoad};
    use tempo_base::mic::{MicEnded, MicStatus};
    use tempo_codec::headset::{gain, Cushion};
    use tempo_codec::mic_decode::{MicDecoder, FRAME_SAMPLES};
    use tempo_codec::receive_encode::encode_offline;

    const TAG: Tag = [4; 8];
    const LEASE: &str = "30000000-0000-4000-8000-000000000002";

    /// The station's end of an audio road: a socket, the keys the shack's end holds, and where the
    /// computer last spoke from.
    struct Station {
        socket: UdpSocket,
        keys: Keys,
        peer: Option<SocketAddr>,
    }

    /// A road to a station of the test's own, keyed alike at both ends, on a stand-in headset.
    fn road() -> (Arc<AudioRoad>, Station, Arc<StandIn>, Arc<OnAir>) {
        let exported = [9; EXPORTED];
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(5)))
            .unwrap();
        let headset = StandIn::new();
        let on_air = Arc::new(OnAir::default());
        let road = AudioRoad::open(
            socket.local_addr().unwrap(),
            TAG,
            &exported,
            headset.clone(),
            on_air.clone(),
        )
        .unwrap();
        let keys = Keys::sized(&exported, TAG, End::Shack, AUDIO_MAX).unwrap();
        let station = Station {
            socket,
            keys,
            peer: None,
        };
        (road, station, headset, on_air)
    }

    impl Station {
        /// Every message the road sent within `within`, with when it came.
        fn heard(&mut self, within: Duration) -> Vec<(Up, Instant)> {
            let until = Instant::now() + within;
            let mut heard = Vec::new();
            let mut buffer = [0u8; 2048];
            while Instant::now() < until {
                if let Ok((length, from)) = self.socket.recv_from(&mut buffer) {
                    let up = self
                        .keys
                        .receive
                        .open(&buffer[..length])
                        .and_then(|(kind, body)| Up::read(kind, &body));
                    if let Some(up) = up {
                        self.peer = Some(from);
                        heard.push((up, Instant::now()));
                    }
                }
            }
            heard
        }

        /// Everything the road has sent that the station has not read yet, with when it was read:
        /// read until nothing is waiting, so a reader a loaded machine wakes late misses nothing.
        fn drain(&mut self) -> Vec<(Up, Instant)> {
            let mut heard = Vec::new();
            let mut buffer = [0u8; 2048];
            while let Ok((length, from)) = self.socket.recv_from(&mut buffer) {
                let up = self
                    .keys
                    .receive
                    .open(&buffer[..length])
                    .and_then(|(kind, body)| Up::read(kind, &body));
                if let Some(up) = up {
                    self.peer = Some(from);
                    heard.push((up, Instant::now()));
                }
            }
            heard
        }

        /// Waits for the road's first word, so the station knows where the road is: its thread
        /// says `listen` at its next turn, which a loaded machine can wake late.
        fn met(&mut self) {
            assert!(
                until(|| {
                    let _ = self.heard(TICK);
                    self.peer.is_some()
                }),
                "the road never spoke"
            );
        }

        /// One frame of receive audio to the computer.
        fn send(&self, down: &Down) {
            let datagram = self.keys.send.seal(down.kind(), &down.body()).unwrap();
            self.socket
                .send_to(&datagram, self.peer.expect("the computer spoke first"))
                .unwrap();
        }
    }

    /// The microphone frames among what the road sent: their press, number and packet, and when.
    fn mics(heard: &[(Up, Instant)]) -> Vec<(u64, u32, Vec<u8>, Instant)> {
        heard
            .iter()
            .filter_map(|(up, at)| match up {
                Up::Mic {
                    press,
                    frame,
                    packet,
                } => Some((*press, *frame, packet.clone(), *at)),
                _ => None,
            })
            .collect()
    }

    /// A microphone, speaking a 1 kHz tone into `rings` 10 ms at a time until the returned flag is
    /// cleared.
    fn speak(rings: &Arc<Rings>) -> Arc<AtomicBool> {
        let speaking = Arc::new(AtomicBool::new(true));
        let (rings, on) = (rings.clone(), speaking.clone());
        std::thread::spawn(move || {
            let mut n = 0usize;
            while on.load(Ordering::SeqCst) {
                rings.captured((0..480).map(|i| {
                    let t = (n + i) as f32 / 48_000.0;
                    0.4 * (2.0 * std::f32::consts::PI * 1_000.0 * t).sin()
                }));
                n += 480;
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        speaking
    }

    /// The station's receive audio, as its encoder makes it: a quiet 700 Hz tone, frame by frame.
    fn band() -> Vec<Down> {
        let tone: Vec<f32> = (0..48_000)
            .map(|i| 0.01 * (2.0 * std::f32::consts::PI * 700.0 * i as f32 / 48_000.0).sin())
            .collect();
        encode_offline(48_000, &tone)
            .unwrap()
            .into_iter()
            .map(|frame| Down::Rx {
                epoch: frame.epoch,
                seq: frame.seq,
                packet: frame.packet,
            })
            .collect()
    }

    /// Waits for `done` as long as a loaded machine may take to get there: whether it did.
    fn until(mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !done() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }

    /// Waits until the road's thread has finished a whole turn begun after this call, so it has
    /// acted on everything that changed before it, however late a loaded machine woke it.
    fn turned(road: &AudioRoad) {
        let from = road.turns();
        assert!(
            until(|| road.turns() >= from + 2),
            "the road's thread took no turn"
        );
    }

    /// ★ `listen` goes every half second, naming the lease, only while this computer holds one,
    /// and wants the headset's output each time. CONTROL: once the lease is let go, and once the
    /// link has ended, nothing more goes.
    #[test]
    fn listen_goes_every_half_second_only_while_the_lease_is_held() {
        let (road, mut station, headset, _) = road();
        assert!(
            station.heard(Duration::from_millis(600)).is_empty(),
            "no lease, no listen"
        );
        road.lease(Some(LEASE.to_string()));
        let heard = station.heard(Duration::from_millis(1300));
        assert!((2..=4).contains(&heard.len()), "{heard:?}");
        let listen = Up::Listen {
            lease: LEASE.to_string(),
        };
        assert!(heard.iter().all(|(up, _)| *up == listen));
        for pair in heard.windows(2) {
            assert!(pair[1].1 - pair[0].1 >= Duration::from_millis(450));
        }
        assert!(headset.outputs.load(Ordering::SeqCst) >= 2);
        road.lease(None);
        let _ = station.heard(Duration::from_millis(30));
        assert!(
            station.heard(Duration::from_millis(700)).is_empty(),
            "let go"
        );
        road.lease(Some(LEASE.to_string()));
        assert!(!station.heard(Duration::from_millis(600)).is_empty());
        road.link_ended();
        let _ = station.heard(Duration::from_millis(30));
        assert!(
            station.heard(Duration::from_millis(700)).is_empty(),
            "the link ended"
        );
    }

    /// ★ The station's frames play in the order of their numbers, lifted for the headset: an
    /// older frame is dropped, never played late, and a known gap plays the quiet bed for exactly
    /// as long as the missing frames would have. CONTROL: what plays around the gap is the band.
    #[test]
    fn the_stations_frames_play_in_order_and_a_known_gap_plays_the_bed() {
        let (road, mut station, headset, _) = road();
        road.lease(Some(LEASE.to_string()));
        station.met();
        let frames = band();
        let rings = &headset.rings;
        let frame = PLAY_FRAME_SAMPLES;
        for each in &frames[..4] {
            station.send(each);
        }
        until(|| rings.waiting() == 4 * frame);
        assert_eq!(rings.waiting(), 4 * frame);
        // The road reads one datagram a turn.
        station.send(&frames[1]);
        turned(&road);
        assert_eq!(rings.waiting(), 4 * frame, "an older frame played");
        station.send(&frames[6]);
        turned(&road);
        assert_eq!(rings.waiting(), 7 * frame, "the gap's length");
        let mut cushion = Cushion::new(48_000);
        cushion.begin(rings);
        let played: Vec<f32> = (0..7 * frame).map(|_| cushion.next(rings)).collect();
        let loudest = |samples: &[f32]| samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let bed = &played[4 * frame..6 * frame];
        assert!(
            loudest(bed) <= BED_LEVEL * gain() * 1.5,
            "the gap was not the bed"
        );
        assert!(loudest(bed) > 0.0, "the gap was silence, not the bed");
        assert!(
            loudest(&played[2 * frame..4 * frame]) > 0.05,
            "the band before"
        );
        assert!(loudest(&played[6 * frame..]) > 0.05, "the band after");
    }

    /// ★ M9: muted, never ducked, while this computer's over may be on the air (its press, or the
    /// station's rig keyed): nothing the station sends then is queued. CONTROL: with neither, the
    /// same frames play.
    #[test]
    fn the_stations_audio_is_muted_while_the_over_may_be_on_the_air() {
        let (road, mut station, headset, on_air) = road();
        road.lease(Some(LEASE.to_string()));
        station.met();
        let (frames, rings) = (band(), &headset.rings);
        let mutes: [fn(&OnAir); 2] = [
            |on_air| on_air.pressing(Some(1)),
            |on_air| on_air.rig(Some(true)),
        ];
        for (n, mute) in mutes.iter().enumerate() {
            mute(&on_air);
            turned(&road);
            assert!(rings.muted(), "case {n}: not muted");
            for frame in &frames[10 * n..10 * n + 5] {
                station.send(frame);
            }
            // The road reads one datagram a turn.
            for _ in 0..5 {
                turned(&road);
            }
            assert_eq!(rings.waiting(), 0, "case {n}: queued while muted");
            on_air.pressing(None);
            on_air.rig(Some(false));
        }
        turned(&road);
        assert!(!rings.muted());
        for frame in &frames[30..35] {
            station.send(frame);
        }
        until(|| rings.waiting() == 5 * PLAY_FRAME_SAMPLES);
        assert_eq!(rings.waiting(), 5 * PLAY_FRAME_SAMPLES, "control: it plays");
    }

    /// ★ The microphone goes only while a press is live: one 20 ms frame a datagram, Opus at the
    /// constant 80 bytes, numbered from 1 under the press, and taken by the shack's own decoder as
    /// 20 ms at its 12 kHz. CONTROL: before the press, and after it, nothing goes.
    #[test]
    fn the_microphone_goes_only_while_a_press_is_live() {
        let (_road, mut station, headset, on_air) = road();
        let speaking = speak(&headset.rings);
        assert!(
            mics(&station.heard(Duration::from_millis(300))).is_empty(),
            "no press, no microphone"
        );
        on_air.pressing(Some(3));
        let sent = mics(&station.heard(Duration::from_millis(1000)));
        on_air.pressing(None);
        let _ = station.heard(Duration::from_millis(40));
        let after = mics(&station.heard(Duration::from_millis(300)));
        speaking.store(false, Ordering::SeqCst);
        assert!(
            (40..=55).contains(&sent.len()),
            "{} frames in a second",
            sent.len()
        );
        assert!(sent.iter().all(|(press, ..)| *press == 3));
        let numbers: Vec<u32> = sent.iter().map(|(_, n, ..)| *n).collect();
        assert_eq!(numbers[0], 1, "{numbers:?}");
        assert!(numbers.windows(2).all(|w| w[1] == w[0] + 1), "{numbers:?}");
        let mut decoder = MicDecoder::new().unwrap();
        for (_, _, packet, _) in &sent {
            assert_eq!(packet.len(), tempo_codec::mic_encode::MIC_PACKET_BYTES);
            assert_eq!(decoder.decode(packet).map(|s| s.len()), Some(FRAME_SAMPLES));
        }
        assert!(after.is_empty(), "the microphone went on after the press");
        assert!(headset.inputs.load(Ordering::SeqCst) > 0);
    }

    /// ★ A press that went on from the one speaking (an answered Lock) carries the microphone on:
    /// its frames go under the new press, numbered on from the old press's last, so the station
    /// places them on the same over's clock. CONTROL: the old press's frames were numbered from 1,
    /// and a press begun afresh after it starts again at frame 1.
    #[test]
    fn a_press_that_goes_on_carries_the_microphone_on() {
        let (_road, mut station, headset, on_air) = road();
        let speaking = speak(&headset.rings);
        on_air.pressing(Some(3));
        let before = mics(&station.heard(Duration::from_millis(400)));
        assert!(on_air.renew(3, 4));
        let after = mics(&station.heard(Duration::from_millis(400)));
        on_air.pressing(Some(9));
        let fresh = mics(&station.heard(Duration::from_millis(300)));
        speaking.store(false, Ordering::SeqCst);
        let numbers = |sent: &[(u64, u32, Vec<u8>, Instant)], press: u64| -> Vec<u32> {
            sent.iter()
                .filter(|(p, ..)| *p == press)
                .map(|(_, n, ..)| *n)
                .collect()
        };
        let old: Vec<u32> = [numbers(&before, 3), numbers(&after, 3)].concat();
        assert_eq!(old.first(), Some(&1), "CONTROL: {old:?}");
        let renewed = numbers(&after, 4);
        assert!(renewed.len() > 10, "{renewed:?}");
        assert_eq!(
            renewed[0],
            old.last().unwrap() + 1,
            "numbered on: {old:?} then {renewed:?}"
        );
        assert!(renewed.windows(2).all(|w| w[1] == w[0] + 1), "{renewed:?}");
        assert_eq!(
            numbers(&fresh, 9).first(),
            Some(&1),
            "CONTROL: a fresh press"
        );
    }

    /// ★ No burst after a stall: with the sender held while 300 ms of the microphone come in, the
    /// frames captured meanwhile are not sent when it is let go, only those the station could still
    /// play on time, and their numbers are skipped, so the station plays a hole where the stall
    /// was. CONTROL: before the stall the frames went one by one, numbered in turn. The press's
    /// clock is the test's, each frame cut at its own moment, so the stall is the only lateness
    /// there is: on the machine's clock a loaded machine can make frames late before the stall (a
    /// stand-in microphone falling behind, the road's thread starved) and move the press's start,
    /// which widens or narrows what a stall lets through.
    #[test]
    fn after_a_stall_nothing_comes_in_a_burst() {
        let (road, mut station, headset, on_air) = road();
        let rings = &headset.rings;
        // The microphone's `n`th 20 ms from the press's start, a 1 kHz tone, and the moment on the
        // press's clock it is all there.
        let tone = |n: usize| {
            (n * MIC_FRAME_SAMPLES..(n + 1) * MIC_FRAME_SAMPLES)
                .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 1_000.0 * i as f32 / 48_000.0).sin())
        };
        let start = Instant::now();
        let ready = |n: usize| start + Duration::from_millis(MIC_FRAME_MS * n as u64);
        on_air.pressing(Some(5));
        // The road starts a press on fresh audio: the microphone speaks once it has.
        turned(&road);
        for n in 0..25 {
            road.clock(ready(n));
            rings.captured(tone(n));
            turned(&road);
        }
        let before = mics(&station.drain());
        let stalled = lock(&on_air.sender);
        for n in 25..40 {
            rings.captured(tone(n));
        }
        // Let go the moment the last of it is all there: the oldest frame the station could still
        // play is then exactly `LATE` old, the most the rule ever lets through at once.
        road.clock(ready(39));
        let during = station.drain();
        let released = Instant::now();
        drop(stalled);
        turned(&road);
        let after = mics(&station.drain());
        assert!(before.windows(2).all(|w| w[1].1 == w[0].1 + 1), "control");
        let last_before = mics(&during)
            .last()
            .or(before.last())
            .map(|(_, n, ..)| *n)
            .unwrap();
        let first = after.first().expect("nothing after the stall");
        let together = after
            .iter()
            .filter(|(.., at)| at.saturating_duration_since(first.3) < Duration::from_millis(15))
            .count();
        eprintln!(
            "measured: after a 300 ms stall, {together} frames came together, numbers {} to {}",
            last_before, first.1
        );
        assert!(
            first.3 + Duration::from_millis(5) >= released,
            "premise: held"
        );
        assert!(together <= 4, "{together} frames in a burst");
        assert!(
            first.1 >= last_before + 10,
            "the stall's frames were not skipped: {last_before} then {}",
            first.1
        );
    }

    /// ★ When Stop's road hears an over end other than by the operator letting go, the headset
    /// plays the tone, through the mute, and it never reaches the microphone. A press held with no
    /// word from the station for half a second plays it once. CONTROL: a release plays nothing.
    #[test]
    fn an_overs_end_plays_the_tone_here_and_never_into_the_microphone() {
        let (road, mut station, headset, on_air) = road();
        let shack_keys = Keys::new(&[6; EXPORTED], [2; 8], End::Shack).unwrap();
        let stop_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        // The road's thread names the lease at its next turn, a tick away, but a loaded machine can
        // wake that thread later than any small multiple of a tick: the first heartbeat is waited
        // for as long as such a machine may take. Nothing else reads this socket.
        stop_socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let stop = StopRoad::open(
            stop_socket.local_addr().unwrap(),
            [2; 8],
            &[6; EXPORTED],
            on_air.clone(),
        )
        .unwrap();
        stop.lease(Some(LEASE.to_string()));
        stop.fetched(1);
        let mut buffer = [0u8; 256];
        let (_, computer) = stop_socket.recv_from(&mut buffer).expect("a heartbeat");
        // Stop's road takes each word on its own thread and the audio road acts on it at its own
        // next turn: a loaded machine can wake either later than any fixed wait, so both are
        // waited for.
        let say = |status: MicStatus| {
            let down = shack_stop::Down::Mic(status);
            let datagram = shack_keys.send.seal(down.kind(), &down.body()).unwrap();
            stop_socket.send_to(&datagram, computer).unwrap();
            assert!(
                until(|| stop.heard().mic == Some(status)),
                "Stop's road never heard {status:?}"
            );
            turned(&road);
        };
        let rings = &headset.rings;
        let sounded = |rings: &Rings| {
            let mut cushion = Cushion::new(48_000);
            cushion.begin(rings);
            (0..24_000).fold(0.0f32, |m, _| m.max(cushion.next(rings).abs()))
        };
        let keyed = MicStatus {
            armed: true,
            keyed: true,
            ..MicStatus::default()
        };
        let ended = |why| MicStatus {
            ended: Some(why),
            ..MicStatus::default()
        };
        say(keyed);
        assert_eq!(
            stop.over(Instant::now()),
            Some(Over::OnAir {
                holes: 0,
                hole_ms: 0
            })
        );
        assert!(rings.muted(), "the station's over keyed mutes it");
        say(ended(MicEnded::Released));
        assert!(sounded(rings) < 0.01, "a release played the tone");
        say(keyed);
        say(ended(MicEnded::AudioGap));
        assert!(sounded(rings) > 0.05, "no tone at the end");
        assert!(
            rings.take_mic().is_empty(),
            "the tone reached the microphone"
        );
        assert!(mics(&station.heard(Duration::from_millis(100))).is_empty());
        on_air.pressing(Some(8));
        std::thread::sleep(super::super::stop_road::NO_WORD + Duration::from_millis(100));
        assert_eq!(stop.over(Instant::now()), Some(Over::NoWord));
        turned(&road);
        assert!(sounded(rings) > 0.05, "no tone for the station's silence");
        std::thread::sleep(Duration::from_millis(400));
        turned(&road);
        assert!(
            sounded(rings) < 0.01,
            "the silence's tone again for one press"
        );
        drop(road);
    }
}
