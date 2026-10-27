//! Stop's second road, this computer's end: a UDP socket of its own to the road the station named
//! in its welcome (`remote_service::lan::stop_road`), or on the internet a road of its own over ICE
//! (`crate::ice`), keyed from the command link's TLS session, and a thread that reads it and does
//! nothing else. The command link knows nothing of it, and it knows nothing of the link: either road
//! alone stops the station.
//!
//! - **A Stop press** ([`StopRoad::stop`]) is written to the socket by the caller itself, on the
//!   caller's own thread, before anything else is asked: client mode's Stop handler, beside the
//!   Stop it sends on the command link, and under that Stop's number (how many Stops this run has
//!   pressed, itself included). The road's thread only sends it again, every [`REPEAT`]
//!   until the station acknowledges it, for up to [`GIVE_UP`]. Only then is this computer's own
//!   press ended, and that is an atomic store: nothing a Stop does waits for the audio road.
//! - **The held press** ([`StopRoad::press`], [`StopRoad::hold`], [`StopRoad::release`]): each
//!   time this computer's screen re-asserts its PTT, one `hold`, written by the caller on its own
//!   thread; the road never repeats one on a timer of its own, so a screen that stops re-asserting
//!   lets the station's held PTT end the press 200 ms later. A press the station says has ended
//!   ends here too, so the microphone stops and it never keys again. The station says that end
//!   again at each hold that reaches it after, so a lost report is heard at the next hold; it is
//!   taken once, for the press live here. Every hold names how many Stops had been pressed when its
//!   press began, so the station never keys a press the network delivers after a Stop pressed after
//!   it. The station holds one press for at most Phone's Lock cap, so a Lock its operator answers
//!   goes on under a new press ([`StopRoad::renew`]): the station hands the key straight over to
//!   it, and the microphone goes on as it was.
//! - **The heartbeat**, every [`HEARTBEAT`] while the link is open: the lease, and the number of
//!   the last snapshot this computer's main window fetched (client mode keeps it, [`StopRoad::fetched`]).
//!   The number comes from the screen's own poll, so a window that hangs stops moving it, and
//!   presence lapses at the station: Rust here keeps time, never freshness.
//! - **What the station says** comes back on the same socket: each Stop acknowledged, a press's
//!   report, the microphone over with its holes on the air, presence's deadline ([`Heard`]). From
//!   it come the screens' words for this computer's over ([`StopRoad::over`]) and what the audio
//!   road reads of it without a lock ([`OnAir`]): whether the station says the over is keyed, when
//!   the station was last heard, and each end of an over the operator did not make by letting go.
//! - **After the link ends** the road stays for [`AFTERLIFE`], as the station keeps its keys, so a
//!   Stop pressed as the link drops, or just after, still goes. No heartbeat goes then.
//! - **In a test build** a road opened under the bench's road switch drops every datagram both ways
//!   while the switch blocks it, as a firewall here would (`crate::bench_road_switch`).
//!
//! The keys are made from the link's session and held in memory only; nothing here writes, logs or
//! shows them. They go with the afterlife: the road's thread drops them as it ends, whoever still
//! holds the road.
//!
//! What the road carries, either way, is [`wire`]'s, which the shack reads and writes too.
/// What Stop's second road carries, as both ends read and write it.
pub mod wire;
use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tempo_base::mic::{MicEnded, MicStatus};
use tempo_base::ptt_hold::HoldEnd;

use self::wire::{Down, Up, AFTERLIFE};
use crate::datagram::{End, Keys, Opener, Sealer, Tag, EXPORTED};
use crate::ice::{IceRoad, Path};

/// How often an unacknowledged Stop is sent again.
pub const REPEAT: Duration = Duration::from_millis(50);
/// How long a Stop is sent again before it is given up: by then presence has lapsed at the
/// station, which halts it anyway.
pub const GIVE_UP: Duration = Duration::from_secs(5);
/// How often the heartbeat goes.
pub const HEARTBEAT: Duration = Duration::from_millis(500);
/// The longest the thread waits for the station before it looks at its own work again: the read
/// timeout an internet road's ICE road is bound with too.
pub const TICK: Duration = Duration::from_millis(10);
/// What is kept of what the station said, each kind.
const KEPT: usize = 16;
/// How long a press may be held with no word from the station before the screens say the over may
/// have ended.
pub const NO_WORD: Duration = Duration::from_millis(500);
/// How long an ended over's words stay on the screens.
pub const ENDED_SHOWN: Duration = Duration::from_secs(10);

/// Whether this computer's over may be on the air, and what the station last said of it, in
/// atomics only: Stop's road and the command link write it, the audio road and the screens read
/// it, and none of them ever waits for another. M9 reads it: the station's audio is muted, never
/// ducked, while the over may be on the air.
pub struct OnAir {
    /// The press live at this computer, 0 for none.
    press: AtomicU64,
    /// The press the live one went on from ([`StopRoad::renew`]), its microphone with it; 0 for
    /// none.
    went_on_from: AtomicU64,
    /// The station's microphone over, as Stop's road last heard it: keyed.
    over_keyed: AtomicBool,
    /// The station's rig, as the command link's status line last said: keyed.
    rig_keyed: AtomicBool,
    /// How many overs Stop's road has heard end, other than by the operator letting go.
    ended: AtomicU64,
    /// When the station was last heard on either road, in ms from `epoch`.
    heard: AtomicU64,
    epoch: Instant,
    /// A test's hold on the audio road's sender: the stand-in for a sender stuck mid-send.
    #[cfg(any(test, feature = "test-util"))]
    pub sender: Mutex<()>,
}

impl Default for OnAir {
    fn default() -> Self {
        Self {
            press: AtomicU64::new(0),
            went_on_from: AtomicU64::new(0),
            over_keyed: AtomicBool::new(false),
            rig_keyed: AtomicBool::new(false),
            ended: AtomicU64::new(0),
            heard: AtomicU64::new(0),
            epoch: Instant::now(),
            #[cfg(any(test, feature = "test-util"))]
            sender: Mutex::new(()),
        }
    }
}

impl OnAir {
    /// M9: this computer's press, the station saying its over is keyed, or its rig keyed.
    pub fn may_be_on_air(&self) -> bool {
        self.press().is_some()
            || self.over_keyed.load(Ordering::Acquire)
            || self.rig_keyed.load(Ordering::Acquire)
    }

    /// The press live at this computer.
    pub fn press(&self) -> Option<u64> {
        Some(self.press.load(Ordering::Acquire)).filter(|press| *press != 0)
    }

    /// A press is live (its number, never 0), or none is. An atomic store: never waits.
    pub fn pressing(&self, press: Option<u64>) {
        self.went_on_from.store(0, Ordering::Release);
        self.press.store(press.unwrap_or(0), Ordering::Release);
    }

    /// The press `press` goes on as `renewed`, if it is the live one: whether it was. Never waits.
    pub(crate) fn renew(&self, press: u64, renewed: u64) -> bool {
        self.went_on_from.store(press, Ordering::Release);
        self.press
            .compare_exchange(press, renewed, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// The press the live one went on from, when it went on from one: the microphone carries on
    /// from that press's frames, numbered on, rather than starting afresh.
    pub fn went_on_from(&self) -> Option<u64> {
        Some(self.went_on_from.load(Ordering::Acquire)).filter(|press| *press != 0)
    }

    /// The press `press` is over here, if it is the live one: another press, or none, is left as
    /// it is. Whether it was the live one. Never waits.
    fn end_press(&self, press: u64) -> bool {
        self.press
            .compare_exchange(press, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// The station's rig, as its status line says (`None` when it cannot tell).
    pub fn rig(&self, keyed: Option<bool>) {
        self.rig_keyed
            .store(keyed.unwrap_or(false), Ordering::Release);
    }

    /// How many overs have ended other than by the operator letting go.
    pub fn ended(&self) -> u64 {
        self.ended.load(Ordering::Acquire)
    }

    /// The station was heard just now, on either road.
    pub fn heard(&self) {
        let ms = Instant::now()
            .saturating_duration_since(self.epoch)
            .as_millis();
        self.heard
            .store(u64::try_from(ms).unwrap_or(u64::MAX), Ordering::Release);
    }

    /// How long since the station was last heard, on either road, at `now`.
    pub fn silent_for(&self, now: Instant) -> Duration {
        let last = self.epoch + Duration::from_millis(self.heard.load(Ordering::Acquire));
        now.saturating_duration_since(last)
    }
}

/// What the screens say of this computer's over: on the air with the station's count of its holes,
/// ended and why, or a press held with no word from the station.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "phase")]
pub enum Over {
    /// Keyed at the station: the holes on the air in it so far.
    #[serde(rename_all = "camelCase")]
    OnAir { holes: u32, hole_ms: u32 },
    /// Ended, and why: `audioGap`, `pttGap`, `stopped`, `presence`, `ceiling`, `watchdog`,
    /// `routeChanged`, or `refused` for a press the station would not take. Said for
    /// [`ENDED_SHOWN`].
    Ended { why: &'static str },
    /// A press held, and no word from the station for [`NO_WORD`]: the over may have ended.
    NoWord,
}

/// The words for an over the microphone ended, or none for one the operator let go.
fn ended_why(ended: MicEnded) -> Option<&'static str> {
    Some(match ended {
        MicEnded::Released => return None,
        MicEnded::Stopped => "stopped",
        MicEnded::AudioGap => "audioGap",
        MicEnded::Presence => "presence",
        MicEnded::Ceiling => "ceiling",
        MicEnded::Watchdog => "watchdog",
        MicEnded::RouteChanged => "routeChanged",
    })
}

/// What the station has said on the road. The screens read the over's state and its end from it
/// ([`StopRoad::over`]); the rest is for the held press, once a press from afar is allowed, and
/// until then only the tests read it.
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heard {
    /// The Stops it acknowledged, by their numbers, the newest last.
    pub stopped: VecDeque<u64>,
    /// Its reports on presses, the newest last.
    pub presses: VecDeque<Down>,
    /// The microphone over, as it last said.
    pub mic: Option<MicStatus>,
    /// The last presence deadline it gave: when it came, and how long it had to run.
    pub presence: Option<(Instant, Duration)>,
    /// The last over it said had ended other than by the operator letting go: when, and why.
    pub ended: Option<(Instant, &'static str)>,
}

/// A Stop the station has not acknowledged yet.
struct Pending {
    stop: u64,
    first: Instant,
    last: Instant,
}

struct Shared {
    /// The road's way to the station: the LAN's socket, or its internet road.
    path: Path,
    /// Whether this computer's over may be on the air, shared with the audio road and the link.
    on_air: Arc<OnAir>,
    /// The sending half of the keys, until the road's thread ends and drops it.
    send: Mutex<Option<Sealer>>,
    /// How long the road outlives its link.
    afterlife: Duration,
    stops: Mutex<Vec<Pending>>,
    /// How many Stops had been pressed when the press live here began: what each of its holds names.
    press_stops: AtomicU64,
    lease: Mutex<Option<String>>,
    /// The number of the last snapshot the main window fetched, 0 before the first.
    state: AtomicU64,
    /// When the link this road was made for ended.
    ended: Mutex<Option<Instant>>,
    heard: Mutex<Heard>,
    /// A test build's road switch, when the road was opened under one.
    #[cfg(feature = "bench-road-switch")]
    switch: Option<Arc<crate::bench_road_switch::Switch>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// This computer's end of one link's Stop road. Cheap to share: every holder is the same road.
pub struct StopRoad {
    shared: Arc<Shared>,
}

impl StopRoad {
    /// The road to `to`, tagged `tag`, under the keys the link's exporter gave (`exported`, under
    /// `datagram::STOP_LABEL` with the welcome's session as the context), with its thread. `None`
    /// if no socket could be had or no thread started.
    pub fn open(
        to: SocketAddr,
        tag: Tag,
        exported: &[u8; EXPORTED],
        on_air: Arc<OnAir>,
    ) -> Option<Arc<Self>> {
        Self::open_with(to, tag, exported, on_air, AFTERLIFE)
    }

    pub fn open_with(
        to: SocketAddr,
        tag: Tag,
        exported: &[u8; EXPORTED],
        on_air: Arc<OnAir>,
        afterlife: Duration,
    ) -> Option<Arc<Self>> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        // Connected: only the station's road is heard on it.
        socket.connect(to).ok()?;
        socket.set_read_timeout(Some(TICK)).ok()?;
        Self::on(Path::Lan(socket), tag, exported, on_air, afterlife)
    }

    /// The road over the internet: on `ice` (bound with [`TICK`], its pair found by ICE, and heard
    /// only there), tagged `tag`, under the keys the link's exporter gave for an internet Stop road
    /// (`datagram::INTERNET_STOP_LABEL`), with its thread.
    pub fn internet(
        ice: Arc<IceRoad>,
        tag: Tag,
        exported: &[u8; EXPORTED],
        on_air: Arc<OnAir>,
    ) -> Option<Arc<Self>> {
        Self::on(Path::Internet(ice), tag, exported, on_air, AFTERLIFE)
    }

    fn on(
        path: Path,
        tag: Tag,
        exported: &[u8; EXPORTED],
        on_air: Arc<OnAir>,
        afterlife: Duration,
    ) -> Option<Arc<Self>> {
        let Keys { send, receive } = Keys::new(exported, tag, End::Computer)?;
        let shared = Arc::new(Shared {
            path,
            on_air,
            send: Mutex::new(Some(send)),
            afterlife,
            stops: Mutex::new(Vec::new()),
            press_stops: AtomicU64::new(0),
            lease: Mutex::new(None),
            state: AtomicU64::new(0),
            ended: Mutex::new(None),
            heard: Mutex::new(Heard::default()),
            #[cfg(feature = "bench-road-switch")]
            switch: crate::bench_road_switch::Switch::opening(),
        });
        let theirs = shared.clone();
        std::thread::Builder::new()
            .name("nexus-client-stop-road".into())
            .spawn(move || run(theirs, receive))
            .ok()?;
        Some(Arc::new(Self { shared }))
    }

    /// A Stop press, numbered `stop` by the caller (how many Stops this run has pressed, itself
    /// included: the number the command link's Stop carries): written to the socket now, on this
    /// thread, and then sent again by the road's thread until the station acknowledges it; then
    /// this computer's own press is over. Returns at once.
    pub fn stop(&self, stop: u64) {
        let now = Instant::now();
        self.shared.say(&Up::Stop { stop });
        lock(&self.shared.stops).push(Pending {
            stop,
            first: now,
            last: now,
        });
        // The microphone stops sending: an atomic store, which waits for nothing.
        self.shared.on_air.pressing(None);
    }

    /// What the screens say of this computer's over at `now`, if anything.
    pub fn over(&self, now: Instant) -> Option<Over> {
        let on_air = &self.shared.on_air;
        let pressing = on_air.press().is_some();
        if pressing && on_air.silent_for(now) >= NO_WORD {
            return Some(Over::NoWord);
        }
        let heard = lock(&self.shared.heard);
        if let Some(status) = heard.mic.filter(|status| status.keyed) {
            return Some(Over::OnAir {
                holes: status.holes,
                hole_ms: status.hole_ms,
            });
        }
        heard
            .ended
            .filter(|(at, _)| now.saturating_duration_since(*at) < ENDED_SHOWN)
            .map(|(_, why)| Over::Ended { why })
    }

    /// One message, sent now: whether it went on the socket. For the tests, which speak on the
    /// road by hand; the app speaks through the calls below.
    #[cfg(test)]
    pub fn say(&self, up: &Up) -> bool {
        self.shared.say(up)
    }

    /// A new press, numbered `press`, begun when this run had pressed `stops` Stops: its first hold
    /// written now, on this thread, and the press live here (the microphone sends, and the station's
    /// audio here is muted). Each hold of it names the same `stops`.
    pub fn press(&self, press: u64, stops: u64) {
        self.shared.press_stops.store(stops, Ordering::SeqCst);
        self.shared.say(&Up::Hold { press, stops });
        self.shared.on_air.pressing(Some(press));
    }

    /// The press `press` held again, as the screen re-asserted it: one hold, written now, while
    /// the press is still live here. `false`, and nothing sent, once it has ended: a Stop pressed
    /// here, or the station saying it ended.
    pub fn hold(&self, press: u64) -> bool {
        if self.shared.on_air.press() != Some(press) {
            return false;
        }
        let stops = self.shared.press_stops.load(Ordering::SeqCst);
        self.shared.say(&Up::Hold { press, stops });
        true
    }

    /// The live press `press` goes on as the new press `renewed`, begun when this run had pressed
    /// `stops` Stops, as an answer to "Still talking?" keeps a Lock on: the new press's first hold
    /// written now, and it is the press live here. The station hands its key straight over from
    /// the old press, and the microphone goes on, its frames numbered on, so the over carries on as
    /// it was. `false`, and nothing sent, once `press` has ended here.
    pub fn renew(&self, press: u64, renewed: u64, stops: u64) -> bool {
        if self.shared.on_air.press() != Some(press) {
            return false;
        }
        self.shared.press_stops.store(stops, Ordering::SeqCst);
        self.shared.say(&Up::Hold {
            press: renewed,
            stops,
        });
        self.shared.on_air.renew(press, renewed)
    }

    /// The press `press` let go: its release written now, and the press over here.
    pub fn release(&self, press: u64) {
        self.shared.say(&Up::Release { press });
        self.shared.on_air.end_press(press);
    }

    /// Whether presence is live as the station last said it on this road: the deadline it gave at
    /// its last renewal not past.
    pub fn presence_live(&self, now: Instant) -> bool {
        lock(&self.shared.heard)
            .presence
            .is_some_and(|(at, left)| now < at + left)
    }

    /// The lease the heartbeat names, or none while this computer holds none.
    pub fn lease(&self, lease: Option<String>) {
        *lock(&self.shared.lease) = lease;
    }

    /// The main window fetched the snapshot numbered `state` on this road's link.
    pub fn fetched(&self, state: u64) {
        self.shared.state.store(state, Ordering::SeqCst);
    }

    /// The link this road was made for has ended: no more heartbeats, and the road goes once its
    /// afterlife is over.
    pub fn link_ended(&self) {
        lock(&self.shared.ended).get_or_insert_with(Instant::now);
    }

    /// Is the road past its afterlife, so a Stop pressed now would find no keys at the station?
    pub fn gone(&self, now: Instant) -> bool {
        self.shared.gone(now)
    }

    /// What the station has said on the road so far.
    // See `Heard`.
    #[allow(dead_code)]
    pub fn heard(&self) -> Heard {
        lock(&self.shared.heard).clone()
    }
}

impl Shared {
    fn say(&self, up: &Up) -> bool {
        let sealed = lock(&self.send)
            .as_ref()
            .and_then(|send| send.seal(up.kind(), &up.body()));
        // Blocked by a test build's switch: sealed, and dropped, as a firewall here would drop it.
        #[cfg(feature = "bench-road-switch")]
        if self.blocked() {
            return sealed.is_some();
        }
        sealed.is_some_and(|datagram| self.path.send(&datagram))
    }

    /// Whether a test build's road switch blocks this road now.
    #[cfg(feature = "bench-road-switch")]
    fn blocked(&self) -> bool {
        self.switch
            .as_ref()
            .is_some_and(|switch| switch.blocks(crate::bench_road_switch::Road::StopRoad))
    }

    fn gone(&self, now: Instant) -> bool {
        lock(&self.ended).is_some_and(|at| now.saturating_duration_since(at) >= self.afterlife)
    }

    fn heard_one(&self, down: Down, now: Instant) {
        fn keep<T>(list: &mut VecDeque<T>, item: T) {
            if list.len() >= KEPT {
                list.pop_front();
            }
            list.push_back(item);
        }
        self.on_air.heard();
        if let Down::Stopped { stop } = down {
            lock(&self.stops).retain(|pending| pending.stop != stop);
        }
        let mut heard = lock(&self.heard);
        // An over's end, taken once: by the microphone over's state as it leaves armed, or by the
        // press's report when its holds stopped arriving (the over then reads as let go).
        let ended = |heard: &mut Heard, why: &'static str| {
            heard.ended = Some((now, why));
            self.on_air.ended.fetch_add(1, Ordering::AcqRel);
        };
        match down {
            Down::Stopped { stop } => keep(&mut heard.stopped, stop),
            press @ Down::Press { .. } => {
                // A press the station says has ended is over here too: the microphone stops, and
                // the screen's next re-assertion finds it ended. Its end is taken once, while the
                // press is live here: the station says it again for each hold that reached it
                // after, and a press let go here is the operator's own, with no words or tone.
                let taken = match press {
                    Down::Press {
                        press: number,
                        end: Some(end),
                        ..
                    } if self.on_air.end_press(number) => Some(end),
                    _ => None,
                };
                match taken {
                    Some(HoldEnd::Lapsed) => ended(&mut heard, "pttGap"),
                    // Not an over, so no tone: the words alone.
                    Some(HoldEnd::Refused) => heard.ended = Some((now, "refused")),
                    _ => {}
                }
                keep(&mut heard.presses, press);
            }
            Down::Mic(status) => {
                let was_armed = heard.mic.is_some_and(|was| was.armed);
                if was_armed && !status.armed {
                    if let Some(why) = status.ended.and_then(ended_why) {
                        ended(&mut heard, why);
                    }
                }
                self.on_air
                    .over_keyed
                    .store(status.keyed, Ordering::Release);
                heard.mic = Some(status);
            }
            Down::Presence { left_ms } => {
                heard.presence = Some((now, Duration::from_millis(u64::from(left_ms))))
            }
        }
    }
}

/// The road's thread: what the station says, the Stops sent again, and the heartbeat, until the
/// road is past its afterlife (or nobody holds it) with no Stop left to send. The keys go with it.
fn run(shared: Arc<Shared>, receive: Opener) {
    serve(&shared, receive);
    lock(&shared.send).take();
}

fn serve(shared: &Arc<Shared>, mut receive: Opener) {
    let mut buffer = [0u8; 256];
    let mut beat: Option<Instant> = None;
    loop {
        match shared.path.recv(&mut buffer) {
            // Blocked by a test build's switch: dropped unread, as a firewall here would drop it.
            #[cfg(feature = "bench-road-switch")]
            Ok(_) if shared.blocked() => {}
            Ok(length) => {
                let heard = receive
                    .open(&buffer[..length])
                    .and_then(|(kind, body)| Down::read(kind, &body));
                if let Some(down) = heard {
                    shared.heard_one(down, Instant::now());
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            // A refusal for an earlier datagram (the station's port closed a moment, say), or the
            // socket's own trouble: nothing here ends the road but its afterlife, and a pause keeps
            // an error that repeats from spinning.
            Err(_) => std::thread::sleep(TICK),
        }
        let now = Instant::now();
        let again: Vec<u64> = {
            let mut stops = lock(&shared.stops);
            stops.retain(|pending| {
                let keep = now.saturating_duration_since(pending.first) < GIVE_UP;
                if !keep {
                    tempo_base::applog::warn(
                        "client",
                        "a Stop on the second road was not acknowledged within five seconds",
                    );
                }
                keep
            });
            stops
                .iter_mut()
                .filter(|pending| now.saturating_duration_since(pending.last) >= REPEAT)
                .map(|pending| {
                    pending.last = now;
                    pending.stop
                })
                .collect()
        };
        for stop in again {
            shared.say(&Up::Stop { stop });
        }
        let open = lock(&shared.ended).is_none();
        if open && beat.is_none_or(|at| now.saturating_duration_since(at) >= HEARTBEAT) {
            let lease = lock(&shared.lease).clone();
            let state = shared.state.load(Ordering::SeqCst);
            if let (Some(lease), true) = (lease, state > 0) {
                shared.say(&Up::Heartbeat { lease, state });
                beat = Some(now);
            }
        }
        let idle = lock(&shared.stops).is_empty();
        if idle && (shared.gone(now) || Arc::strong_count(shared) == 1) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: Tag = [1; 8];
    const LEASE: &str = "30000000-0000-4000-8000-000000000001";

    /// The station's end of a road, for a test: a socket, and the keys the shack's end holds.
    struct Station {
        socket: UdpSocket,
        keys: Keys,
    }

    /// A road to a station of the test's own, keyed alike at both ends.
    fn road() -> (Arc<StopRoad>, Station) {
        road_with(AFTERLIFE)
    }

    fn road_with(afterlife: Duration) -> (Arc<StopRoad>, Station) {
        let exported = [3; EXPORTED];
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(5)))
            .unwrap();
        let road = StopRoad::open_with(
            socket.local_addr().unwrap(),
            TAG,
            &exported,
            Arc::default(),
            afterlife,
        )
        .unwrap();
        let keys = Keys::new(&exported, TAG, End::Shack).unwrap();
        (road, Station { socket, keys })
    }

    impl Station {
        /// Every message the road sent within `within`: what, when it came, and from where.
        fn heard(&mut self, within: Duration) -> Vec<(Up, Instant, SocketAddr)> {
            let until = Instant::now() + within;
            let mut heard = Vec::new();
            let mut buffer = [0u8; 256];
            while Instant::now() < until {
                if let Ok((length, from)) = self.socket.recv_from(&mut buffer) {
                    let up = self
                        .keys
                        .receive
                        .open(&buffer[..length])
                        .and_then(|(kind, body)| Up::read(kind, &body));
                    if let Some(up) = up {
                        heard.push((up, Instant::now(), from));
                    }
                }
            }
            heard
        }

        fn answer(&self, to: SocketAddr, down: &Down) {
            let datagram = self.keys.send.seal(down.kind(), &down.body()).unwrap();
            self.socket.send_to(&datagram, to).unwrap();
        }
    }

    /// ★ A Stop goes at once, from the caller's own thread, and the road's thread sends it again
    /// every 50 ms until the station acknowledges it; then no more. CONTROL: the acknowledgement is
    /// heard, under the same number.
    #[test]
    fn a_stop_goes_at_once_and_again_every_50_ms_until_it_is_acknowledged() {
        let (road, mut station) = road();
        let pressed = Instant::now();
        let stop = 1;
        road.stop(stop);
        let heard = station.heard(Duration::from_millis(180));
        assert!(heard.len() >= 3, "{heard:?}");
        assert!(heard.iter().all(|(up, ..)| *up == Up::Stop { stop }));
        assert!(heard[0].1 - pressed < Duration::from_millis(5), "at once");
        for pair in heard.windows(2) {
            let between = pair[1].1 - pair[0].1;
            assert!(between >= Duration::from_millis(45), "{between:?}");
        }
        station.answer(heard[0].2, &Down::Stopped { stop });
        let _ = station.heard(Duration::from_millis(30));
        assert!(
            station.heard(Duration::from_millis(200)).is_empty(),
            "no more once acknowledged"
        );
        assert_eq!(road.heard().stopped, [stop]);
    }

    /// ★ A Stop nobody acknowledges is sent again for five seconds and then given up: by then
    /// presence has lapsed at the station. CONTROL: it was sent again all that while.
    #[test]
    fn a_stop_nobody_acknowledges_is_given_up_after_five_seconds() {
        let (road, mut station) = road();
        road.stop(1);
        let heard = station.heard(Duration::from_millis(5600));
        let (first, last) = (heard[0].1, heard[heard.len() - 1].1);
        assert!(
            heard.len() >= 60,
            "sent again while unacknowledged: {}",
            heard.len()
        );
        assert!(
            last - first < GIVE_UP + Duration::from_millis(100),
            "{:?}",
            last - first
        );
        assert!(
            last - first > GIVE_UP - Duration::from_millis(200),
            "{:?}",
            last - first
        );
    }

    /// ★ The heartbeat names the lease and the snapshot the screen fetched, every 500 ms, only while
    /// it has both, and only while the link is open. CONTROL: a new number is the next one named.
    #[test]
    fn the_heartbeat_names_the_lease_and_the_fetched_snapshot_while_the_link_is_open() {
        let (road, mut station) = road();
        assert!(
            station.heard(Duration::from_millis(700)).is_empty(),
            "nothing to name"
        );
        road.lease(Some(LEASE.to_string()));
        assert!(
            station.heard(Duration::from_millis(700)).is_empty(),
            "no snapshot fetched"
        );
        road.fetched(9);
        let heard = station.heard(Duration::from_millis(1300));
        let beats: Vec<&Instant> = heard.iter().map(|(_, at, _)| at).collect();
        assert!((2..=4).contains(&beats.len()), "{heard:?}");
        assert!(heard.iter().all(|(up, ..)| *up
            == Up::Heartbeat {
                lease: LEASE.to_string(),
                state: 9
            }));
        for pair in beats.windows(2) {
            assert!(*pair[1] - *pair[0] >= Duration::from_millis(450));
        }
        road.fetched(10);
        let next = station.heard(Duration::from_millis(600));
        assert_eq!(
            next.first().map(|(up, ..)| up.clone()),
            Some(Up::Heartbeat {
                lease: LEASE.to_string(),
                state: 10
            })
        );
        road.link_ended();
        let _ = station.heard(Duration::from_millis(20));
        assert!(
            station.heard(Duration::from_millis(700)).is_empty(),
            "the link ended"
        );
    }

    /// ★ After its link ends the road stays for its afterlife, as the station keeps its keys: a Stop
    /// pressed then still goes. CONTROL: past the afterlife, the road is gone.
    #[test]
    fn a_stop_still_goes_after_the_link_ended_until_its_afterlife() {
        let (road, mut station) = road();
        road.link_ended();
        let stop = 1;
        road.stop(stop);
        let heard = station.heard(Duration::from_millis(30));
        assert_eq!(
            heard.first().map(|(up, ..)| up.clone()),
            Some(Up::Stop { stop })
        );
        assert!(!road.gone(Instant::now()));
        assert!(road.gone(Instant::now() + AFTERLIFE));
    }

    /// ★ The over's words follow what the station says: keyed with its holes on the air; ended, and
    /// why, once, for ten seconds; a release says nothing;
    /// a press the station refused is words alone, never counted as an over's end; a press held
    /// with no word from the station for half a second is "no word". CONTROL: before anything is
    /// said, there are no words.
    #[test]
    fn the_overs_words_follow_what_the_station_says() {
        let (road, mut station) = road();
        let on_air = road.shared.on_air.clone();
        road.lease(Some(LEASE.to_string()));
        road.fetched(1);
        let peer = station.heard(Duration::from_millis(700))[0].2;
        assert_eq!(road.over(Instant::now()), None);
        let say = |down: Down| {
            station.answer(peer, &down);
            std::thread::sleep(Duration::from_millis(40));
        };
        let status = |armed, keyed, ended, holes, hole_ms| {
            Down::Mic(MicStatus {
                armed,
                keyed,
                no_power_out: false,
                ended,
                holes,
                hole_ms,
            })
        };
        say(status(true, true, None, 1, 100));
        assert_eq!(
            road.over(Instant::now()),
            Some(Over::OnAir {
                holes: 1,
                hole_ms: 100
            })
        );
        assert!(on_air.may_be_on_air(), "the station's over keyed");
        say(status(false, false, Some(MicEnded::AudioGap), 0, 0));
        assert_eq!(
            road.over(Instant::now()),
            Some(Over::Ended { why: "audioGap" })
        );
        assert_eq!(on_air.ended(), 1);
        assert!(!on_air.may_be_on_air());
        assert_eq!(
            road.over(Instant::now() + ENDED_SHOWN),
            None,
            "ten seconds on"
        );
        say(status(true, false, None, 0, 0));
        say(status(false, false, Some(MicEnded::Released), 0, 0));
        assert_eq!(on_air.ended(), 1, "a release is the operator's own");
        // A press's end is this computer's own over only while the press is live here.
        on_air.pressing(Some(4));
        say(Down::Press {
            press: 4,
            keyed: false,
            end: Some(HoldEnd::Lapsed),
        });
        assert_eq!(
            road.over(Instant::now()),
            Some(Over::Ended { why: "pttGap" })
        );
        assert_eq!(on_air.ended(), 2);
        on_air.pressing(Some(5));
        say(Down::Press {
            press: 5,
            keyed: false,
            end: Some(HoldEnd::Refused),
        });
        assert_eq!(
            road.over(Instant::now()),
            Some(Over::Ended { why: "refused" })
        );
        assert_eq!(on_air.ended(), 2, "a refused press was no over");
        on_air.pressing(Some(6));
        assert_ne!(road.over(Instant::now()), Some(Over::NoWord), "just heard");
        assert_eq!(
            road.over(Instant::now() + NO_WORD),
            Some(Over::NoWord),
            "held, and silence"
        );
    }

    /// ⛔ An over's end is taken once per press, while the press is still live here: the station
    /// says it again for every hold that reaches it after (one lost datagram must not leave the
    /// over on), so the words and the tone come once. A press this computer let go is the
    /// operator's own: an end said for it after the release brings no tone. CONTROL: the first end
    /// said for the live press is taken, and the press is over here.
    #[test]
    fn an_end_said_again_is_taken_once_per_press() {
        let (road, mut station) = road();
        let on_air = road.shared.on_air.clone();
        road.lease(Some(LEASE.to_string()));
        road.fetched(1);
        let peer = station.heard(Duration::from_millis(700))[0].2;
        let say = |down: Down| {
            station.answer(peer, &down);
            std::thread::sleep(Duration::from_millis(40));
        };
        let lapsed = |press| Down::Press {
            press,
            keyed: false,
            end: Some(HoldEnd::Lapsed),
        };
        road.press(7, 0);
        say(lapsed(7));
        assert_eq!(on_air.ended(), 1, "CONTROL: the live press's end is taken");
        assert_eq!(on_air.press(), None, "CONTROL: the press is over here");
        say(lapsed(7));
        say(lapsed(7));
        road.press(8, 0);
        road.release(8);
        say(lapsed(8));
        assert_eq!(
            on_air.ended(),
            1,
            "an end said again, or said for a press let go, taken as an over's end"
        );
    }

    /// ★ A Stop press ends this computer's own press, after the Stop is on the socket. CONTROL:
    /// the press was live before.
    #[test]
    fn a_stop_ends_this_computers_press_after_it_goes() {
        let (road, mut station) = road();
        road.shared.on_air.pressing(Some(9));
        assert_eq!(road.shared.on_air.press(), Some(9));
        let stop = 1;
        road.stop(stop);
        assert_eq!(road.shared.on_air.press(), None);
        assert_eq!(
            station
                .heard(Duration::from_millis(30))
                .first()
                .map(|(up, ..)| up.clone()),
            Some(Up::Stop { stop })
        );
    }

    /// ★ The keys go with the afterlife, whoever still holds the road: once it is over the road's
    /// thread has dropped them, so nothing more is sealed and nothing more goes, a Stop included.
    /// The afterlife is 300 ms here, the product's minute being the test above's. CONTROL: inside
    /// it, a Stop is sealed and goes.
    #[test]
    fn the_keys_go_when_the_afterlife_is_over() {
        let (road, mut station) = road_with(Duration::from_millis(300));
        road.link_ended();
        assert!(road.say(&Up::Stop { stop: 1 }), "inside the afterlife");
        assert_eq!(
            station
                .heard(Duration::from_millis(50))
                .first()
                .map(|(up, ..)| up.clone()),
            Some(Up::Stop { stop: 1 })
        );
        std::thread::sleep(Duration::from_millis(400));
        assert!(road.gone(Instant::now()));
        assert!(
            !road.say(&Up::Stop { stop: 2 }),
            "nothing is sealed past it"
        );
        assert!(station.heard(Duration::from_millis(100)).is_empty());
    }

    /// ★ In a test build, a road opened under the bench's road switch carries nothing either way
    /// while the switch blocks it: a Stop pressed, a held press and the heartbeat never reach the
    /// station, and what the station says (an acknowledgement, presence) is dropped unread. Opened
    /// again, the Stop still unacknowledged is sent again, and its acknowledgement is heard.
    /// CONTROL: before the block the same road's Stop reached the station at once and its answer
    /// was heard; a road opened under no switch is never blocked.
    #[cfg(feature = "bench-road-switch")]
    #[test]
    fn a_road_the_switch_blocks_carries_nothing_either_way() {
        use crate::bench_road_switch::{Position, Switch};
        let switch = Arc::new(Switch::default());
        let (switched, mut station) = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(switch.over(async { road() }));
        switched.lease(Some(LEASE.to_string()));
        switched.fetched(9);
        let first = 1;
        switched.stop(first);
        let heard = station.heard(Duration::from_millis(100));
        assert!(
            heard.iter().any(|(up, ..)| *up == Up::Stop { stop: first }),
            "CONTROL: {heard:?}"
        );
        let from = heard[0].2;
        station.answer(from, &Down::Stopped { stop: first });
        let until = Instant::now() + Duration::from_secs(1);
        while switched.heard().stopped.is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            switched.heard().stopped,
            [first],
            "CONTROL: the answer heard"
        );

        assert_eq!(
            switch.set(Position::BlockStopRoad),
            Ok(Position::BlockStopRoad)
        );
        let _ = station.heard(Duration::from_millis(30));
        let second = 2;
        switched.stop(second);
        switched.press(7, second);
        assert!(switched.hold(7), "the press is live here");
        let carried = station.heard(Duration::from_millis(700));
        assert!(carried.is_empty(), "carried while blocked: {carried:?}");
        station.answer(from, &Down::Stopped { stop: second });
        station.answer(from, &Down::Presence { left_ms: 5_000 });
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            switched.heard().stopped,
            [first],
            "an answer taken while blocked"
        );
        assert!(
            !switched.presence_live(Instant::now()),
            "presence taken while blocked"
        );

        assert_eq!(switch.set(Position::Normal), Ok(Position::Normal));
        let after = station.heard(Duration::from_millis(200));
        assert!(
            after
                .iter()
                .any(|(up, ..)| *up == Up::Stop { stop: second }),
            "the Stop unacknowledged goes again once open: {after:?}"
        );
        station.answer(from, &Down::Stopped { stop: second });
        let until = Instant::now() + Duration::from_secs(1);
        while switched.heard().stopped.len() < 2 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(switched.heard().stopped, [first, second]);

        let (free, mut its_station) = road();
        assert_eq!(
            switch.set(Position::BlockStopRoad),
            Ok(Position::BlockStopRoad)
        );
        let stop = 1;
        free.stop(stop);
        assert!(
            its_station
                .heard(Duration::from_millis(100))
                .iter()
                .any(|(up, ..)| *up == Up::Stop { stop }),
            "a road opened under no switch was blocked"
        );
    }
}
