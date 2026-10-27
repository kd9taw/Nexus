//! Stop's second road at the shack, end to end over loopback: the shack's real channel, command lane
//! and road, and a computer's side of each, its road keys taken from its own TLS session as the
//! computer takes them. The shack's end of the command link runs behind a valve, so a test can make
//! the shack stop reading it.
//!
//! The design's proof (each premise is a test here, and each was watched red first): a Stop on the
//! road is acknowledged within 20 ms with a flood on another port and the command link full; while
//! the Engine is held three seconds; never refused for its lease, its age or its connection, a
//! minute past that connection's end; a datagram that does not verify is dropped unread; each road
//! alone stops each kind of over; presence is renewed only by a fresh echo; both roads dead, it
//! halts within five seconds and a tick.
//!
//! And the stream's transmit rules on this road, for what may now start from afar: a start on the
//! command link and a held press on the road each need the lease bound to the connection, presence
//! live on its road and the state its screen fetched under two seconds old; a press ends 200 ms
//! after its last re-assertion; either road alone stops each kind of over started from afar; a start
//! that ran after presence lapsed is stopped; after a reconnect nothing keys until it is armed.
//!
//! Beside it, the audio road (`super::super::audio_road`), on the same roadside: its own socket and
//! keys, the station's receive audio to a computer in control, and the microphone refused before
//! anything decodes it. The premise-1 flood goes to that real audio socket.
use super::super::audio_road::{self, AudioRoad};
use super::super::datagram::{End, Keys, Tag, AUDIO_LABEL, AUDIO_MAX, EXPORTED, STOP_LABEL};
use super::super::stop_road::{
    Down, Hears, Powers, Running, StateClock, StopRoad, Up, AFTERLIFE, ANSWERED, FRESH, PRESS_CAP,
};
use super::super::wire;
use super::commands::App;
use super::drops::{radio_loop, Over, TICK};
use super::*;
use crate::remote_service::audio::ReceiveFanout;
use crate::remote_service::operations::tests::transmit::{
    assert_stopped, key_locally, LOCAL_TRANSMISSIONS,
};
use crate::remote_service::operations::{RoadPresence, RoadStop};
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use tempo_app::mic::MicFeed;
use tempo_app::remote_control::ptt_hold::{HoldEnd, PttHold};
use tempo_audio::mic_encode::{MicEncoder, MIC_FRAME_SAMPLES};
use tempo_audio::receive_encode::DetachedFeed;
use tempo_client::stop_road::HEARTBEAT;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// The bench's road switch on this roadside, end to end (test builds only).
#[cfg(feature = "bench-road-switch")]
mod bench;
/// A press's end, said again.
mod ends;
/// A section change from afar arms nothing.
mod entry;
/// A CQ run, a JS8 message and a CW macro started from afar, ended by each road and by presence.
mod from_afar;
/// APRS, an SSTV picture and chat started from afar, ended by each road and by presence.
mod from_afar_aprs_sstv_chat;
/// Stop's road over the internet: ICE, through a TURN relay.
mod internet;
/// Phone's Lock from afar, from this computer's screen and microphone to the shack's loop.
mod lock;
/// A press's cap across renewals, a reconnect, re-assertions and the next press.
mod press_cap;
/// Stop's second road through a relay, as the internet stands between the two computers.
mod relayed;
/// Stop wins over a start that was in flight.
mod stand_down;
/// Stop wins over a start or a press that a later Stop overtook on the way.
mod stop_count;
/// An unkey from afar ends its own over and nothing else.
mod unkey;

/// The design's bound for a Stop on this road, flood or no flood.
const ACKNOWLEDGED: Duration = Duration::from_millis(20);
/// Presence's five seconds, written here so a change in the code cannot move them.
const PRESENCE: Duration = Duration::from_secs(5);
/// Long enough to say "nothing came": many times the road's own round trip on loopback.
const NOTHING: Duration = Duration::from_millis(500);

/// Runs `test` on a runtime of its own (`workers` threads, or 0 for this thread alone): the shape
/// of a test that holds the satellite badge's guard, which may not be held across an await point.
fn on_a_runtime(workers: usize, test: impl std::future::Future<Output = ()>) {
    let runtime = if workers == 0 {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
    } else {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .enable_all()
            .build()
    };
    runtime.unwrap().block_on(test);
}

// ----- The shack -----

/// A shack whose calls run on `app`, with Stop's second road on loopback beside it: the held PTT
/// and the microphone installed in its engine as the service installs them, and the road's powers
/// made from the shack's own authority, engine and book, as the listener makes them.
struct Roadside {
    s: Shack,
    ptt: PttHold,
    /// The microphone feed installed in its engine: its epoch moves at every drop of an over.
    mic: MicFeed,
    road: Option<Running>,
    /// The audio road beside it, and the receive feed its encoder reads.
    audio: Option<audio_road::Running>,
    feed: Arc<DetachedFeed>,
}

impl Drop for Roadside {
    fn drop(&mut self) {
        if let Some(road) = self.road.take() {
            drop(road.stop());
        }
        if let Some(audio) = self.audio.take() {
            drop(audio.stop());
        }
    }
}

impl Roadside {
    /// What the audio road has done so far.
    fn counts(&self) -> &audio_road::Counts {
        self.audio.as_ref().expect("an audio road").counts()
    }

    /// The station's band on its receive feed, 20 ms of a 700 Hz tone every 20 ms, until the flag
    /// it returns is cleared.
    fn on_the_air(&self) -> Arc<AtomicBool> {
        let playing = Arc::new(AtomicBool::new(true));
        let (feed, on) = (self.feed.clone(), playing.clone());
        std::thread::spawn(move || {
            let mut n = 0usize;
            while on.load(Ordering::SeqCst) {
                let block: Vec<f32> = (0..960)
                    .map(|i| {
                        let t = (n + i) as f32 / 48_000.0;
                        0.2 * (2.0 * std::f32::consts::PI * 700.0 * t).sin()
                    })
                    .collect();
                feed.publish(Instant::now(), &block);
                n += 960;
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        playing
    }
}

fn roadside_with(app: &App, afterlife: Duration, hears: Hears) -> Roadside {
    roadside_limited(app, afterlife, PRESS_CAP, hears)
}

/// The same, each press from afar held at most `cap`.
fn roadside_capped(app: &App, cap: Duration) -> Roadside {
    roadside_limited(app, AFTERLIFE, cap, Hears::Loopback)
}

fn roadside_limited(app: &App, afterlife: Duration, cap: Duration, hears: Hears) -> Roadside {
    let mut s = super::commands::running(app);
    let (ptt, mic) = (PttHold::default(), MicFeed::default());
    {
        let mut e = s.shared.engine.lock().unwrap();
        let mut settings = e.settings().clone();
        settings.save_qso_wav = false;
        e.apply_settings(settings);
        e.set_remote_ptt_hold(ptt.clone());
        e.set_remote_mic_feed(mic.clone());
    }
    let powers = Powers {
        stop: RoadStop::new(s.shared.authority.clone(), s.shared.engine.clone()).unwrap(),
        presence: RoadPresence::new(s.shared.authority.clone(), s.shared.engine.clone()),
        ptt: ptt.clone(),
        mic: mic.clone(),
        paired: s.shared.desk.as_ref().unwrap().book.verifier().0,
        pairings: {
            let book = s.shared.desk.as_ref().unwrap().book.clone();
            Arc::new(move || book.revisions())
        },
    };
    let at = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
    let road = StopRoad::start_with(at, hears, powers, afterlife, cap).unwrap();
    s.shared.stop_road = Some(road.road());
    // The audio road, as the listener makes it: the station's one encoder over a feed of the
    // test's own, the audio lane's admission from the shack's own authority.
    let feed = Arc::new(DetachedFeed::new(48_000));
    let authority = s.shared.authority.clone();
    let audio = AudioRoad::start(
        at,
        hears,
        audio_road::Powers {
            receive: Some(ReceiveFanout::new(feed.feed.clone())),
            admitted: Arc::new(move |session, device, lease, now| {
                authority.audio_admitted(session, device, lease, now)
            }),
            ptt: ptt.clone(),
            mic: mic.clone(),
        },
    )
    .unwrap();
    s.shared.audio_road = Some(audio.road());
    Roadside {
        s,
        ptt,
        mic,
        road: Some(road),
        audio: Some(audio),
        feed,
    }
}

fn roadside(app: &App) -> Roadside {
    roadside_with(app, AFTERLIFE, Hears::Loopback)
}

/// Shut, the shack reads nothing more from the computer, as a shack that has stopped reading its
/// socket: what the computer writes fills the connection and then waits.
#[derive(Default)]
struct Valve {
    shut: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Valve {
    fn shut(&self) {
        self.shut.store(true, Ordering::SeqCst);
    }
}

struct Valved {
    inner: tokio::net::TcpStream,
    valve: Arc<Valve>,
}

impl AsyncRead for Valved {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.valve.shut.load(Ordering::SeqCst) {
            *self.valve.waker.lock().unwrap() = Some(cx.waker().clone());
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for Valved {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

// ----- A computer -----

/// A computer's end of its road, as client mode's road makes it: a socket to the road the welcome
/// named, and the keys this computer's own TLS session exports. Each datagram is sealed and read
/// here, so a test can also keep one, change one, or send one under other keys.
struct Raw {
    udp: tokio::net::UdpSocket,
    keys: Keys,
    tag: Tag,
}

fn tag_of(offer: &Value) -> Tag {
    let hex = offer["tag"].as_str().expect("the welcome names a road");
    tempo_stream::protocol::hex_bytes(hex)
        .unwrap()
        .try_into()
        .unwrap()
}

/// This computer's export for `session`, from its own side of the TLS session.
fn exported(socket: &Client, session: &str) -> [u8; EXPORTED] {
    exported_under(socket, session, STOP_LABEL)
}

/// The same under any road's label.
fn exported_under(socket: &Client, session: &str, label: &[u8]) -> [u8; EXPORTED] {
    socket
        .get_ref()
        .get_ref()
        .1
        .export_keying_material([0; EXPORTED], label, Some(session.as_bytes()))
        .unwrap()
}

/// A computer's end of its audio road: a socket to the road the welcome named, and its keys.
struct AudioRaw {
    udp: tokio::net::UdpSocket,
    keys: Keys,
}

impl AudioRaw {
    async fn on(port: u16, keys: Keys) -> Self {
        let udp = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        udp.connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        Self { udp, keys }
    }

    async fn say(&self, up: &audio_road::Up) {
        let datagram = self.keys.send.seal(up.kind(), &up.body()).unwrap();
        self.udp.send(&datagram).await.unwrap();
    }

    /// Every frame the shack sent within `within`.
    async fn heard(&mut self, within: Duration) -> Vec<audio_road::Down> {
        let deadline = tokio::time::Instant::now() + within;
        let mut heard = Vec::new();
        loop {
            let mut buffer = [0u8; 2048];
            let Ok(Ok(length)) =
                tokio::time::timeout_at(deadline, self.udp.recv(&mut buffer)).await
            else {
                return heard;
            };
            assert!(length <= AUDIO_MAX, "a datagram of {length} bytes");
            let down = self
                .keys
                .receive
                .open(&buffer[..length])
                .and_then(|(kind, body)| audio_road::Down::read(kind, &body));
            heard.extend(down);
        }
    }
}

impl Raw {
    async fn on(port: u16, keys: Keys, tag: Tag) -> Self {
        let udp = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        udp.connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        Self { udp, keys, tag }
    }

    fn sealed(&self, up: &Up) -> Vec<u8> {
        self.keys.send.seal(up.kind(), &up.body()).unwrap()
    }

    async fn send(&self, datagram: &[u8]) {
        self.udp.send(datagram).await.unwrap();
    }

    async fn say(&self, up: &Up) {
        self.send(&self.sealed(up)).await;
    }

    /// The shack's next message on the road within `within`, if one comes.
    async fn heard(&mut self, within: Duration) -> Option<Down> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let mut buffer = [0u8; 256];
            let length = tokio::time::timeout_at(deadline, self.udp.recv(&mut buffer))
                .await
                .ok()?
                .ok()?;
            let down = self
                .keys
                .receive
                .open(&buffer[..length])
                .and_then(|(kind, body)| Down::read(kind, &body));
            if down.is_some() {
                return down;
            }
        }
    }

    /// When the Stop numbered `stop` was acknowledged, within `within`, whatever else came first.
    async fn stopped(&mut self, stop: u64, within: Duration) -> Option<Instant> {
        let deadline = Instant::now() + within;
        while let Some(down) = self
            .heard(deadline.saturating_duration_since(Instant::now()))
            .await
        {
            if down == (Down::Stopped { stop }) {
                return Some(Instant::now());
            }
        }
        None
    }

    /// When the shack's road sent its acknowledgement of the Stop numbered `stop` on this road, at
    /// or after `since`. The road notes it just after the acknowledgement goes, so this end, which
    /// has heard it, may look a moment early.
    fn answered(&self, stop: u64, since: Instant) -> Option<Instant> {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let noted = ANSWERED
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .find(|(tag, number, at)| *tag == self.tag && *number == stop && *at >= since)
                .map(|(_, _, at)| *at);
            if noted.is_some() || Instant::now() >= deadline {
                return noted;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// A Stop on the road, once this end has heard it acknowledged within `within`: how long the
    /// shack took, from its sending here to the acknowledgement leaving the shack. The time this
    /// end's own thread then takes to read it is the test machine's, not the road's: starved on a
    /// loaded box, it read acknowledgements 25 ms late that had left the shack in under a
    /// millisecond. Sealed, and the socket found writable, before the clock starts: the clock and
    /// the send then run together on this thread, with no yield to the runtime between them.
    async fn road_stop(&mut self, stop: u64, within: Duration) -> Option<Duration> {
        let datagram = self.sealed(&Up::Stop { stop });
        self.udp.writable().await.unwrap();
        let sent = Instant::now();
        self.udp.try_send(&datagram).unwrap();
        self.stopped(stop, within).await?;
        self.answered(stop, sent).map(|at| at - sent)
    }

    /// The next presence deadline the shack gives, within `within`, whatever else came first.
    async fn presence(&mut self, within: Duration) -> Option<u32> {
        let deadline = Instant::now() + within;
        while let Some(down) = self
            .heard(deadline.saturating_duration_since(Instant::now()))
            .await
        {
            if let Down::Presence { left_ms } = down {
                return Some(left_ms);
            }
        }
        None
    }
}

/// A computer on a roadside's station: its command link (the shack's end behind a valve), its lane
/// open and the snapshot watched, its lease if it took one, and its end of its road.
struct Seat {
    socket: Client,
    valve: Arc<Valve>,
    session: String,
    /// The acquire's answer, or `Null` for a computer that did not take the lease.
    state: Value,
    offer: Value,
    road: Raw,
    /// The welcome's audio road, and this computer's end of it.
    audio_offer: Value,
    audio: AudioRaw,
    /// The newest snapshot the lane pushed here: its number and when it arrived.
    snapshot: Option<(u64, Instant)>,
    _stop: watch::Sender<bool>,
}

/// Seated, with the lease taken.
async fn seated(r: &Roadside) -> Seat {
    seated_with(r, true).await
}

async fn seated_with(r: &Roadside, control: bool) -> Seat {
    let (station, computer) = pair().await;
    let valve = Arc::new(Valve::default());
    let (stop, stopped) = watch::channel(false);
    let shack_end = Valved {
        inner: station,
        valve: valve.clone(),
    };
    tokio::spawn(channel::connection(
        shack_end,
        PEER.parse().unwrap(),
        r.s.shared.clone(),
        stopped,
    ));
    let mut socket = open(computer, &r.s.computer.key, &r.s.public_key)
        .await
        .unwrap();
    r.s.shared
        .authority
        .permit_station(&r.s.device, true)
        .unwrap();
    let welcome = hello(&mut socket, VERSIONS).await;
    assert_eq!(welcome["type"], "welcome", "{welcome}");
    let session = welcome["sessionId"].as_str().unwrap().to_string();
    let offer = welcome["stopRoad"].clone();
    let keys = Keys::new(&exported(&socket, &session), tag_of(&offer), End::Computer).unwrap();
    let road = Raw::on(offer["port"].as_u64().unwrap() as u16, keys, tag_of(&offer)).await;
    let audio_offer = welcome["audioRoad"].clone();
    let audio_keys = Keys::sized(
        &exported_under(&socket, &session, AUDIO_LABEL),
        tag_of(&audio_offer),
        End::Computer,
        AUDIO_MAX,
    )
    .unwrap();
    let audio = AudioRaw::on(audio_offer["port"].as_u64().unwrap() as u16, audio_keys).await;
    let state = if control {
        let state = ask(&mut socket, json!({"type":"state","requestId":id()})).await;
        let boot = state["value"]["stationBootId"].clone();
        let acquired = ask(
            &mut socket,
            json!({"type":"acquire","requestId":id(),"stationBootId":boot}),
        )
        .await;
        assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
        acquired["value"].clone()
    } else {
        Value::Null
    };
    send(
        &mut socket,
        json!({"type":"open","profile":"desktop","app":wire::APP,"contract":wire::contract(),
            "run":1}),
    )
    .await;
    let mut seat = Seat {
        socket,
        valve,
        session,
        state,
        offer,
        road,
        audio_offer,
        audio,
        snapshot: None,
        _stop: stop,
    };
    assert_eq!(seat.text().await["type"], "opened");
    send(
        &mut seat.socket,
        json!({"type":"watch","feeds":["snapshot/1"]}),
    )
    .await;
    seat
}

impl Seat {
    fn lease(&self) -> String {
        self.state["leaseId"].as_str().unwrap().to_string()
    }

    /// This computer's audio road keys again, from its own side of the TLS session: a second
    /// sender under the same keys, for a test's own thread.
    fn audio_keys(&self) -> Keys {
        Keys::sized(
            &exported_under(&self.socket, &self.session, AUDIO_LABEL),
            tag_of(&self.audio_offer),
            End::Computer,
            AUDIO_MAX,
        )
        .unwrap()
    }

    fn feed(&mut self, frame: &[u8]) {
        if let Some(("snapshot/1", number, _)) = wire::read_feed(frame) {
            self.snapshot = Some((number, Instant::now()));
        }
    }

    /// The shack's next message of text on the link, the status line left out, every feed frame
    /// before it noted.
    async fn text(&mut self) -> Value {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), self.socket.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value["type"] != "status" {
                        return value;
                    }
                }
                Ok(Some(Ok(Message::Binary(frame)))) => self.feed(&frame),
                Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
                other => panic!("the link ended: {other:?}"),
            }
        }
    }

    /// One of the lease's requests on the link, asked again while the shack is busy for a moment.
    async fn ask(&mut self, request: Value) -> Value {
        for _ in 0..50 {
            let mut request = request.clone();
            let asked = id();
            request["requestId"] = json!(asked);
            send(
                &mut self.socket,
                json!({"type":"operationRequest","request":request}),
            )
            .await;
            let answer = loop {
                let said = self.text().await;
                if said["type"] == "operationResponse" && said["requestId"] == asked.as_str() {
                    break said;
                }
            };
            if answer["error"] != "stationBusy" && answer["error"] != "remoteBusy" {
                return answer;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the station stayed busy")
    }

    /// Whether the shack ended the link within five seconds.
    async fn closed(&mut self) -> bool {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            match tokio::time::timeout_at(deadline, self.socket.next()).await {
                Err(_) => return false,
                Ok(None | Some(Err(_)) | Some(Ok(Message::Close(_)))) => return true,
                Ok(Some(Ok(_))) => {}
            }
        }
    }

    /// The lease renewed by its own heartbeat, on the link.
    async fn keep_the_lease(&mut self) {
        let lease = self.lease();
        let answer = self.ask(json!({"type":"heartbeat","leaseId":lease})).await;
        assert_eq!(answer["value"]["phase"], "controlling", "{answer}");
    }

    /// The next snapshot the lane pushes: its number, and when it arrived here.
    async fn fresh_snapshot(&mut self) -> (u64, Instant) {
        let before = self.snapshot.map(|(number, _)| number);
        loop {
            match tokio::time::timeout(Duration::from_secs(5), self.socket.next()).await {
                Ok(Some(Ok(Message::Binary(frame)))) => {
                    self.feed(&frame);
                    if let Some(now) = self.snapshot.filter(|(n, _)| Some(*n) != before) {
                        return now;
                    }
                }
                Ok(Some(Ok(_))) => {}
                other => panic!("no snapshot came: {other:?}"),
            }
        }
    }

    /// A heartbeat on the road naming the snapshot numbered `state`, under this computer's lease.
    async fn heartbeat(&self, state: u64) {
        let lease = self.lease();
        self.road.say(&Up::Heartbeat { lease, state }).await;
    }

    /// The newest snapshot here: the next one, then any that waited in the socket behind it (the
    /// lane pushes one every 300 ms, so one that comes within 50 ms was waiting).
    async fn newest_snapshot(&mut self) -> (u64, Instant) {
        let mut newest = self.fresh_snapshot().await;
        while let Ok(Some(Ok(message))) =
            tokio::time::timeout(Duration::from_millis(50), self.socket.next()).await
        {
            if let Message::Binary(frame) = message {
                self.feed(&frame);
                newest = self.snapshot.unwrap_or(newest);
            }
        }
        newest
    }

    /// Presence renewed on the road by a fresh echo, and the deadline the shack gave. As this
    /// computer's road keeps it: a heartbeat naming the newest snapshot here, and another every
    /// [`HEARTBEAT`] until the shack answers, for as long as presence lasts. One heartbeat can go
    /// unanswered (the shack's authority busy that moment, or a snapshot too old to count after it
    /// waited in this socket while the test seated another computer); the shack takes the next,
    /// as it does from the real road.
    async fn renewed(&mut self) -> u32 {
        let deadline = Instant::now() + PRESENCE;
        loop {
            let (state, _) = self.newest_snapshot().await;
            self.heartbeat(state).await;
            let wait = HEARTBEAT.min(deadline.saturating_duration_since(Instant::now()));
            if let Some(left_ms) = self.road.presence(wait).await {
                return left_ms;
            }
            assert!(Instant::now() < deadline, "a fresh echo renews presence");
        }
    }

    /// One call on the command link, and its answer, whatever comes before it.
    async fn call(&mut self, id: u64, cmd: &str, args: Value) -> Value {
        let call = json!({"type":"call","id":id,"cmd":cmd,"args":args});
        if call.to_string().len() > wire::MESSAGE_BYTES {
            // Bigger than one message (an SSTV picture): in pieces, as the computer sends it.
            for frame in wire::split(wire::Whole::Text(call.to_string()), 1) {
                self.socket
                    .send(Message::Binary(frame.into()))
                    .await
                    .unwrap();
            }
        } else {
            send(&mut self.socket, call).await;
        }
        loop {
            let said = self.text().await;
            if said["type"] == "result" && said["id"] == id {
                return said;
            }
        }
    }

    /// A heartbeat under a lease that is not this computer's, naming the fresh snapshot `state`:
    /// the road reads the state its screen fetched, and renews nothing.
    async fn fresh_but_not_present(&self, state: u64) {
        self.road.say(&Up::Heartbeat { lease: id(), state }).await;
    }

    /// Everything the road says within `within`.
    async fn road_says(&mut self, within: Duration) -> Vec<Down> {
        let deadline = Instant::now() + within;
        let mut heard = Vec::new();
        while let Some(down) = self
            .road
            .heard(deadline.saturating_duration_since(Instant::now()))
            .await
        {
            heard.push(down);
        }
        heard
    }

    /// Writes on the command link until a write waits: the shack has stopped reading, and the link
    /// is full. How many messages it took.
    async fn fill_the_link(&mut self) -> usize {
        let call = Message::Text(
            json!({"type":"watch","feeds":["x".repeat(7000)]})
                .to_string()
                .into(),
        );
        let mut sent = 0;
        loop {
            match tokio::time::timeout(Duration::from_millis(300), self.socket.send(call.clone()))
                .await
            {
                Ok(Ok(())) => sent += 1,
                Ok(Err(error)) => panic!("the link failed instead of filling: {error}"),
                Err(_) => return sent,
            }
        }
    }
}

/// Waits, up to three seconds, until nothing at the station is on the air or armed: what every
/// stop leaves. The stop verbs run on the road's own stopper thread, as soon as the Engine is free.
async fn came_to_a_stop(s: &Shack) -> bool {
    for _ in 0..600 {
        {
            let e = s.shared.engine.lock().unwrap();
            if e.tx_owner().is_none() && !e.tx_enabled() && !e.manual_ptt() && !e.tuning() {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    false
}

fn key_the_mic(s: &Shack) {
    key_locally(&mut s.shared.engine.lock().unwrap(), "ptt");
}

fn keyed(s: &Shack) -> bool {
    s.shared.engine.lock().unwrap().manual_ptt()
}

// ----- The welcome -----

/// ★ The welcome names each connection's own road: the road's port, and a tag of its own, sixteen
/// lowercase hex characters. CONTROL: a shack whose road did not open names none, and a window's
/// welcome is otherwise as it was.
#[tokio::test]
async fn the_welcome_names_each_connections_own_road() {
    let app = App::new();
    let r = roadside(&app);
    let (one, two) = (seated(&r).await, seated_with(&r, false).await);
    assert_eq!(one.offer["port"], two.offer["port"]);
    assert_ne!(one.offer["tag"], two.offer["tag"]);
    for offer in [&one.offer, &two.offer] {
        let tag = offer["tag"].as_str().unwrap();
        assert_eq!(tag.len(), 16);
        assert!(tag
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    }
    let plain = super::commands::running(&app);
    let (_stop, stopped) = watch::channel(false);
    let (_task, socket) = dial(&plain, &plain.computer.key, PEER, stopped).await;
    let mut socket = socket.unwrap();
    plain
        .shared
        .authority
        .permit_station(&plain.device, true)
        .unwrap();
    let welcome = hello(&mut socket, VERSIONS).await;
    assert_eq!(welcome["type"], "welcome");
    assert!(welcome.get("stopRoad").is_none(), "{welcome}");
}

// ----- Stop is never refused, on this road -----

/// ★ The never-refused rule on this road (`a_controlling_browser_stops_any_local_transmission_…`
/// on the hosted road): a Stop from a computer in control stops any transmission at the station,
/// however it started. It is acknowledged, and the transmit generation is retired at once, before
/// the stop verbs run. CONTROL: each is on the air first.
#[test]
fn a_stop_on_the_road_stops_any_local_transmission() {
    let _alone = alone();
    on_a_runtime(0, async {
        for kind in LOCAL_TRANSMISSIONS {
            let app = App::new();
            let r = roadside(&app);
            let mut seat = seated(&r).await;
            key_locally(&mut r.s.shared.engine.lock().unwrap(), kind);
            let generation = r.s.shared.authority.transmit_revocation().generation();
            seat.road.say(&Up::Stop { stop: 1 }).await;
            assert!(
                seat.road.stopped(1, Duration::from_secs(2)).await.is_some(),
                "{kind}: acknowledged"
            );
            assert!(
                r.s.shared.authority.transmit_revocation().generation() > generation,
                "{kind}: the generation is retired"
            );
            assert!(came_to_a_stop(&r.s).await, "{kind}");
            assert_stopped(&mut r.s.shared.engine.lock().unwrap(), kind);
        }
    });
}

/// ★ Only two things are refused, and both stop nothing: a computer removed at the shack (its
/// connection ends, and its road's Stop changes nothing), and a datagram under keys no connection
/// was given. CONTROL: another computer still paired stops the same transmission on its own road.
#[test]
fn a_removed_computer_or_a_stranger_stops_nothing() {
    let _alone = alone();
    on_a_runtime(0, async {
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        key_the_mic(&r.s);
        // A stranger: this road's tag, keys from no session of the shack's.
        let stranger = Keys::new(&[9; EXPORTED], seat.road.tag, End::Computer).unwrap();
        let forged = stranger
            .send
            .seal(Up::Stop { stop: 1 }.kind(), &1u64.to_be_bytes())
            .unwrap();
        seat.road.send(&forged).await;
        assert_eq!(
            seat.road.heard(NOTHING).await,
            None,
            "a stranger is not answered"
        );
        assert!(keyed(&r.s), "a stranger stops nothing");
        // Removed at the shack.
        assert!(r.s.shared.desk.as_ref().unwrap().book.forget(&r.s.device));
        assert!(seat.closed().await, "its connection ends");
        assert_eq!(
            seat.road.road_stop(2, NOTHING).await,
            None,
            "a removed computer is not answered"
        );
        assert!(keyed(&r.s), "a removed computer stops nothing");
        drop(seat);

        // CONTROL: a paired computer's Stop stops it.
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        key_the_mic(&r.s);
        assert!(seat
            .road
            .road_stop(1, Duration::from_secs(2))
            .await
            .is_some());
        assert!(came_to_a_stop(&r.s).await);
    });
}

/// ★ No lease is asked: a computer that never took it, and one that released it, each stops the
/// station from its road (an expired lease's case on the hosted road). CONTROL: on the air first.
#[test]
fn a_stop_on_the_road_needs_no_lease() {
    let _alone = alone();
    on_a_runtime(0, async {
        let app = App::new();
        let r = roadside(&app);
        let mut idle = seated_with(&r, false).await;
        key_the_mic(&r.s);
        assert!(idle
            .road
            .road_stop(1, Duration::from_secs(2))
            .await
            .is_some());
        assert!(came_to_a_stop(&r.s).await, "never took the lease");

        let mut seat = seated(&r).await;
        let lease = seat.lease();
        let released = seat.ask(json!({"type":"release","leaseId":lease})).await;
        assert!(released.get("error").is_none(), "{released}");
        key_the_mic(&r.s);
        assert!(seat
            .road
            .road_stop(1, Duration::from_secs(2))
            .await
            .is_some());
        assert!(came_to_a_stop(&r.s).await, "released the lease");
    });
}

/// ★ Never refused for its age (`a_delayed_stop_still_stops_what_is_on_the_air_now` on the hosted
/// road): a Stop sealed before a transmission started, and delivered after, stops it; the same
/// Stop sent again later (a new datagram, the same number) stops what is on the air then. CONTROL:
/// the same bytes again are a replay, dropped, and what is on the air stays on.
#[test]
fn a_late_or_repeated_stop_stops_what_is_on_the_air_when_it_lands() {
    let _alone = alone();
    on_a_runtime(0, async {
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        let late = seat.road.sealed(&Up::Stop { stop: 1 });
        key_the_mic(&r.s);
        tokio::time::sleep(Duration::from_millis(200)).await;
        seat.road.send(&late).await;
        assert!(seat.road.stopped(1, Duration::from_secs(2)).await.is_some());
        assert!(came_to_a_stop(&r.s).await, "a late Stop");
        key_the_mic(&r.s);
        assert!(seat
            .road
            .road_stop(1, Duration::from_secs(2))
            .await
            .is_some());
        assert!(came_to_a_stop(&r.s).await, "the same Stop, sent again");
        key_the_mic(&r.s);
        seat.road.send(&late).await;
        assert_eq!(
            seat.road.heard(NOTHING).await,
            None,
            "a replay is not answered"
        );
        assert!(keyed(&r.s), "a replay stops nothing");
    });
}

/// ★ The road outlives its connection: thirty seconds after the computer's connection ended, its
/// Stop is still taken, and stops a transmission started at the shack since. CONTROL: the
/// connection had ended (its lease went with it).
#[test]
fn a_stop_thirty_seconds_after_its_connection_ended_is_still_taken() {
    let _alone = alone();
    on_a_runtime(0, async {
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        let _ = seat.socket.close(None).await;
        for _ in 0..200 {
            if r.s.shared.authority.local_status()["controller"].is_null() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            r.s.shared.authority.local_status()["controller"].is_null(),
            "the connection's lease went with it"
        );
        tokio::time::sleep(Duration::from_secs(30)).await;
        key_the_mic(&r.s);
        let took = seat.road.road_stop(1, Duration::from_secs(2)).await;
        eprintln!("measured: a Stop 30 s after its connection ended, acknowledged in {took:?}");
        assert!(took.is_some(), "acknowledged");
        assert!(came_to_a_stop(&r.s).await);
    });
}

/// ★ …and only for its afterlife: past it, the road's keys are gone, and its Stop changes nothing.
/// The afterlife is a second here, the product's minute being what the test above shows.
/// CONTROL: inside it, the same road's Stop is taken.
#[test]
fn a_road_past_its_afterlife_takes_nothing() {
    let _alone = alone();
    on_a_runtime(0, async {
        let app = App::new();
        let r = roadside_with(&app, Duration::from_secs(1), Hears::Loopback);
        let mut seat = seated(&r).await;
        let _ = seat.socket.close(None).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        key_the_mic(&r.s);
        assert!(seat
            .road
            .road_stop(1, Duration::from_secs(2))
            .await
            .is_some());
        assert!(came_to_a_stop(&r.s).await, "inside the afterlife");
        tokio::time::sleep(Duration::from_millis(1200)).await;
        key_the_mic(&r.s);
        assert_eq!(seat.road.road_stop(2, NOTHING).await, None);
        assert!(keyed(&r.s), "past the afterlife");
    });
}

/// ★ A datagram that does not verify is dropped before anything reads its body: one sealed under
/// another session's keys, one with another road's tag (the tag is in the authenticated header),
/// a changed byte, and a replay. Each is a well-formed Stop under the keys that sealed it, so a road
/// that read before verifying would act on it. A message of a kind the road does not take is
/// dropped too. CONTROL: the untouched Stop is taken.
#[test]
fn a_datagram_that_does_not_verify_is_dropped_unread() {
    let _alone = alone();
    on_a_runtime(0, async {
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        let other = seated_with(&r, false).await;
        key_the_mic(&r.s);
        let other_session = Keys::new(
            &exported(&other.socket, &other.session),
            seat.road.tag,
            End::Computer,
        )
        .unwrap();
        let mut changed = seat.road.sealed(&Up::Stop { stop: 2 });
        let last = changed.len() - 1;
        changed[last] ^= 1;
        let mut retagged = seat.road.sealed(&Up::Stop { stop: 3 });
        retagged[2..10].copy_from_slice(&other.road.tag);
        let wrong_kind = seat.road.keys.send.seal(16, &4u64.to_be_bytes()).unwrap();
        for (what, datagram) in [
            (
                "another session's keys",
                other_session.send.seal(1, &1u64.to_be_bytes()).unwrap(),
            ),
            ("a changed byte", changed),
            ("another road's tag", retagged),
            ("a kind the road does not take", wrong_kind),
        ] {
            seat.road.send(&datagram).await;
            assert_eq!(seat.road.heard(NOTHING).await, None, "{what}");
            assert!(keyed(&r.s), "{what} stops nothing");
        }
        let stop = seat.road.sealed(&Up::Stop { stop: 5 });
        seat.road.send(&stop).await;
        assert!(seat.road.stopped(5, Duration::from_secs(2)).await.is_some());
        assert!(came_to_a_stop(&r.s).await);
        key_the_mic(&r.s);
        seat.road.send(&stop).await;
        assert_eq!(seat.road.heard(NOTHING).await, None, "a replay");
        assert!(keyed(&r.s), "a replay stops nothing");
    });
}

/// ★ A datagram that verifies under the road's keys but carries a body the format does not write is
/// read as nothing and changes nothing: a Stop's body a byte short and a byte long, a hold that names
/// no Stops, a heartbeat whose lease the station could not have issued, and kinds the road does not
/// take. None is answered, and the local transmission stays on the air. CONTROL: a well-formed
/// Stop under the same keys, after them all, is taken.
#[test]
fn a_datagram_that_verifies_but_is_malformed_changes_nothing() {
    let _alone = alone();
    on_a_runtime(0, async {
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        key_the_mic(&r.s);
        let lease = seat.lease();
        let sealed = |kind: u8, body: &[u8]| seat.road.keys.send.seal(kind, body).unwrap();
        for (what, datagram) in [
            ("a Stop a byte short", sealed(1, &[0, 0, 0, 0, 0, 0, 1])),
            (
                "a Stop a byte long",
                sealed(1, &[0, 0, 0, 0, 0, 0, 0, 1, 0]),
            ),
            ("a hold that names no Stops", sealed(2, &1u64.to_be_bytes())),
            (
                "a heartbeat under a lease never issued",
                sealed(
                    4,
                    &[format!("g{}", &lease[1..]).as_bytes(), &1u64.to_be_bytes()].concat(),
                ),
            ),
            ("kind 0", sealed(0, &1u64.to_be_bytes())),
            ("kind 5", sealed(5, &1u64.to_be_bytes())),
        ] {
            seat.road.send(&datagram).await;
            assert_eq!(seat.road.heard(NOTHING).await, None, "{what}");
            assert!(keyed(&r.s), "{what} stops nothing");
        }
        seat.road.say(&Up::Stop { stop: 1 }).await;
        assert!(seat.road.stopped(1, Duration::from_secs(2)).await.is_some());
        assert!(came_to_a_stop(&r.s).await);
    });
}

// ----- Stop never waits -----

/// ★ Its own socket, port, thread and keys. The audio road's real socket flooded at a hundred
/// times its rate with frames that verify (each read, and refused, on the audio road's own
/// thread), its receive audio flowing to the same computer, and the command link full (the shack
/// has stopped reading it): a Stop on the road is still acknowledged within 20 ms, five times over.
/// CONTROL: the flood reached the audio road and was read there, the link really was full, and
/// the station came to a stop.
#[test]
fn a_stop_is_acknowledged_within_20_ms_beside_a_flood_and_a_full_command_link() {
    let _alone = alone();
    on_a_runtime(2, async {
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        key_the_mic(&r.s);
        // The audio road busy both ways: the station's band to this computer, and its socket
        // flooded with microphone frames under this computer's own audio keys.
        let band = r.on_the_air();
        seat.audio
            .say(&audio_road::Up::Listen {
                lease: seat.lease(),
            })
            .await;
        let at = SocketAddr::new(
            Ipv4Addr::LOCALHOST.into(),
            seat.audio_offer["port"].as_u64().unwrap() as u16,
        );
        let keys = seat.audio_keys();
        let flooding = Arc::new(AtomicBool::new(true));
        let flood = {
            let flooding = flooding.clone();
            std::thread::spawn(move || {
                let sender = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
                let mut frame = 0u32;
                // Audio is a datagram every 20 ms: fifty a second. This is five thousand.
                while flooding.load(Ordering::SeqCst) {
                    for _ in 0..50 {
                        frame += 1;
                        let up = audio_road::Up::Mic {
                            press: 1,
                            frame,
                            packet: vec![0x5a; 80],
                        };
                        let datagram = keys.send.seal(up.kind(), &up.body()).unwrap();
                        let _ = sender.send_to(&datagram, at);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
        };
        seat.valve.shut();
        let written = seat.fill_the_link().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut took = Vec::new();
        for stop in 1..=5u64 {
            took.push(seat.road.road_stop(stop, Duration::from_secs(1)).await);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        flooding.store(false, Ordering::SeqCst);
        band.store(false, Ordering::SeqCst);
        flood.join().unwrap();
        let read = r.counts().mic_read.load(Ordering::SeqCst);
        let sent = r.counts().rx_sent.load(Ordering::SeqCst);
        eprintln!(
            "measured: Stops on the road acknowledged in {took:?}, beside {read} flood frames read \
             on the audio road, {sent} receive frames sent on it, and a command link that took \
             {written} messages before it was full"
        );
        assert!(
            written > 0 && read > 100,
            "premise: link {written}, read {read}"
        );
        for (n, took) in took.iter().enumerate() {
            let took = took.unwrap_or_else(|| panic!("Stop {} was not acknowledged", n + 1));
            assert!(took < ACKNOWLEDGED, "Stop {} took {took:?}", n + 1);
        }
        assert!(came_to_a_stop(&r.s).await);
    });
}

/// ★ No shared lock. While a test holds the Engine's lock for three seconds, a Stop on
/// the road is acknowledged within 20 ms, and the station stops as soon as the lock is let go.
/// CONTROL: the lock was still held when the acknowledgement came, and the mic stayed keyed until
/// it was let go.
#[test]
fn a_stop_is_acknowledged_while_the_engine_is_held_three_seconds() {
    let _alone = alone();
    on_a_runtime(4, async {
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        key_the_mic(&r.s);
        let (held, holding) = std::sync::mpsc::channel();
        let engine = r.s.shared.engine.clone();
        let holder = std::thread::spawn(move || {
            let e = engine.lock().unwrap();
            held.send(Instant::now()).unwrap();
            std::thread::sleep(Duration::from_secs(3));
            let still_keyed = e.manual_ptt();
            drop(e);
            (Instant::now(), still_keyed)
        });
        let since = holding.recv().unwrap();
        let took = seat.road.road_stop(1, Duration::from_secs(1)).await;
        let acknowledged = Instant::now();
        let (let_go, still_keyed) = tokio::task::spawn_blocking(move || holder.join().unwrap())
            .await
            .unwrap();
        eprintln!("measured: a Stop acknowledged in {took:?} with the Engine held");
        let took = took.expect("acknowledged");
        assert!(took < ACKNOWLEDGED, "took {took:?}");
        assert!(acknowledged < let_go && acknowledged - since < Duration::from_secs(3));
        assert!(still_keyed, "nothing ran under the held lock");
        assert!(came_to_a_stop(&r.s).await, "stopped once the lock was free");
    });
}

/// ★ No lock on a Stop's path, not even the pairing book's: while a test holds the book's lock (the
/// one TLS takes at every handshake), a Stop on the road is acknowledged within 20 ms and the station
/// stops. CONTROL: the lock was still held when the acknowledgement came.
#[test]
fn a_stop_is_acknowledged_while_the_pairing_book_is_held() {
    let _alone = alone();
    on_a_runtime(4, async {
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        key_the_mic(&r.s);
        let book = r.s.shared.desk.as_ref().unwrap().book.clone();
        let (held, holding) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let guard = book.held();
            held.send(()).unwrap();
            std::thread::sleep(Duration::from_secs(1));
            drop(guard);
            Instant::now()
        });
        holding.recv().unwrap();
        let took = seat.road.road_stop(1, Duration::from_millis(900)).await;
        let acknowledged = Instant::now();
        let let_go = tokio::task::spawn_blocking(move || holder.join().unwrap())
            .await
            .unwrap();
        eprintln!("measured: a Stop acknowledged in {took:?} with the pairing book held");
        let took = took.expect("acknowledged");
        assert!(took < ACKNOWLEDGED, "took {took:?}");
        assert!(acknowledged < let_go, "while the lock was held");
        assert!(came_to_a_stop(&r.s).await);
    });
}

/// ★ The same with the book held across the shack's status tick, on a runtime of one worker that
/// the computer's end shares with the shack's: the session asks the book again at each tick, and
/// nothing at the shack waits for the book on a worker, which would leave no worker to read any
/// socket or fire any timer on the runtime (a slow PC's runtime, here the computer's own end). The
/// test above is a lottery for this (its Stop usually goes before a tick asks); this one is not.
/// CONTROL: the lock was still held when the acknowledgement came.
#[test]
fn a_stop_is_acknowledged_with_the_pairing_book_held_across_a_status_tick() {
    let _alone = alone();
    on_a_runtime(1, async {
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        key_the_mic(&r.s);
        let book = r.s.shared.desk.as_ref().unwrap().book.clone();
        let (held, holding) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let guard = book.held();
            held.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(1600));
            drop(guard);
            Instant::now()
        });
        holding.recv().unwrap();
        // Past a whole tick (`tempo_app::remote_monitor::POLL_MS`), on this thread: a runtime timer
        // would wait for the very worker this is about.
        std::thread::sleep(Duration::from_millis(
            tempo_app::remote_monitor::POLL_MS + 200,
        ));
        let took = seat.road.road_stop(1, Duration::from_millis(600)).await;
        let acknowledged = Instant::now();
        let let_go = tokio::task::spawn_blocking(move || holder.join().unwrap())
            .await
            .unwrap();
        eprintln!("measured: a Stop acknowledged in {took:?} with the pairing book held a tick");
        let took = took.expect("acknowledged");
        assert!(took < ACKNOWLEDGED, "took {took:?}");
        assert!(acknowledged < let_go, "while the lock was held");
        assert!(came_to_a_stop(&r.s).await);
    });
}

/// ★ Another computer's road opening while the book is held keeps no Stop waiting: its opener asks
/// the book, never the road's thread, which every road's Stop shares. CONTROL: the opening was
/// still waiting on the book when the acknowledgement came, and it opened once the book was free.
#[test]
fn a_stop_is_acknowledged_while_a_road_opens_with_the_pairing_book_held() {
    let _alone = alone();
    on_a_runtime(4, async {
        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        key_the_mic(&r.s);
        let book = r.s.shared.desk.as_ref().unwrap().book.clone();
        let road = r.s.shared.stop_road.clone().expect("a road");
        let (held, holding) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let guard = book.held();
            held.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(1600));
            drop(guard);
            Instant::now()
        });
        holding.recv().unwrap();
        let opener = std::thread::spawn(move || {
            let clock = Arc::new(StateClock::default());
            let opened = road.open(&[7; EXPORTED], "another", "another", [7; 32], clock);
            (Instant::now(), opened.is_some())
        });
        // Many of the road thread's ticks, so it has taken the opening if it would.
        std::thread::sleep(Duration::from_millis(200));
        let took = seat.road.road_stop(1, Duration::from_millis(600)).await;
        let acknowledged = Instant::now();
        let (opened_at, opened) = tokio::task::spawn_blocking(move || opener.join().unwrap())
            .await
            .unwrap();
        let let_go = tokio::task::spawn_blocking(move || holder.join().unwrap())
            .await
            .unwrap();
        eprintln!("measured: a Stop acknowledged in {took:?} while a road opened, the book held");
        let took = took.expect("acknowledged");
        assert!(took < ACKNOWLEDGED, "took {took:?}");
        assert!(acknowledged < let_go, "while the lock was held");
        assert!(
            acknowledged < opened_at && opened,
            "the opening waited on the book, then opened"
        );
        assert!(came_to_a_stop(&r.s).await);
    });
}

// ----- Each road alone -----

/// The commands a cockpit's Stop sends for each kind of over: the kind's own stop and the header's
/// Stop TX, as the desktop sends them.
fn cockpit_stop(kind: &str) -> Vec<&'static str> {
    let own = match kind {
        "cw" => Some("stop_cw"),
        "voice" => Some("stop_voice"),
        "rtty" => Some("rtty_stop"),
        "psk" => Some("psk_stop"),
        "jtty" => Some("jtty_stop"),
        "sstv" => Some("sstv_stop"),
        _ => None,
    };
    own.into_iter().chain(["halt_tx"]).collect()
}

/// ★ Either road alone stops each kind of over. The command link alone (no datagram
/// sent on the road), with the station's stop commands run as its handlers run them; then the road
/// alone, with the link stopped (the shack reads nothing from it). CONTROL: each is on the air
/// before its stop.
#[test]
fn each_road_alone_stops_each_kind_of_over() {
    let _alone = alone();
    on_a_runtime(0, async {
        for kind in LOCAL_TRANSMISSIONS {
            // The command link alone.
            let app = App::new();
            let mut r = roadside(&app);
            let engine = r.s.shared.engine.clone();
            let runner: crate::command_dispatch::Runner = Arc::new(
                move |cmd: String, _args: Value, answer: crate::command_dispatch::Answer| {
                    let mut e = engine.lock().unwrap();
                    match cmd.as_str() {
                        "halt_tx" => e.halt_tx(),
                        "stop_cw" => e.stop_cw(),
                        "stop_voice" => e.stop_voice(),
                        "rtty_stop" => e.rtty_stop(),
                        "psk_stop" => e.psk_stop(),
                        "jtty_stop" => e.jtty_stop(),
                        "sstv_stop" => e.sstv_stop(),
                        _ => {}
                    }
                    answer(Ok(json!({ "ran": cmd })))
                },
            );
            let voided = r.s.shared.engine.lock().unwrap().remote_starts_voided();
            r.s.shared.commands = super::super::commands::Commands::new(runner, voided);
            let mut seat = seated(&r).await;
            key_locally(&mut r.s.shared.engine.lock().unwrap(), kind);
            for (n, cmd) in cockpit_stop(kind).into_iter().enumerate() {
                let id = 100 + n as u64;
                send(
                    &mut seat.socket,
                    json!({"type":"call","id":id,"cmd":cmd,"args":{}}),
                )
                .await;
                loop {
                    let said = seat.text().await;
                    if said["type"] == "result" && said["id"] == id {
                        assert_eq!(said["value"]["ran"], cmd, "{said}");
                        break;
                    }
                }
            }
            assert_stopped(&mut r.s.shared.engine.lock().unwrap(), kind);

            // The road alone.
            let app = App::new();
            let r = roadside(&app);
            let mut seat = seated(&r).await;
            seat.valve.shut();
            key_locally(&mut r.s.shared.engine.lock().unwrap(), kind);
            assert!(
                seat.road
                    .road_stop(1, Duration::from_secs(2))
                    .await
                    .is_some(),
                "{kind}"
            );
            assert!(came_to_a_stop(&r.s).await, "{kind}");
            assert_stopped(&mut r.s.shared.engine.lock().unwrap(), kind);
        }
    });
}

// ----- Presence -----

/// ★ The state echo's clock takes only a number it sent, and only for two seconds after that
/// snapshot was taken: two seconds exactly is fresh, a millisecond more is not, a number never sent
/// is not, and a number the clock forgot (64 sends later) is not. CONTROL: a number it sent is
/// fresh at once.
#[test]
fn the_state_clock_knows_only_what_it_sent_under_two_seconds_ago() {
    let clock = StateClock::default();
    let at = Instant::now();
    clock.sent(5, at);
    assert!(clock.fresh(5, at));
    assert!(clock.fresh(5, at + FRESH));
    assert!(!clock.fresh(5, at + FRESH + Duration::from_millis(1)));
    assert!(!clock.fresh(6, at), "never sent");
    for number in 6..70 {
        clock.sent(number, at);
    }
    assert!(!clock.fresh(5, at), "forgotten");
    assert!(clock.fresh(69, at));
    assert_eq!(FRESH, Duration::from_secs(2));
}

/// ★ Presence is renewed only by a fresh echo. A heartbeat naming the snapshot the lane
/// just pushed renews it (the engine holds it, and the shack says the deadline); one naming a
/// number the lane never sent, one under a lease that is not this computer's, and one naming a
/// snapshot taken more than two seconds before renew nothing. CONTROL: at that same moment, a
/// fresh number renews it.
#[tokio::test]
async fn presence_is_renewed_only_by_a_fresh_echo() {
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    seat.keep_the_lease().await;
    let (old, arrived) = seat.fresh_snapshot().await;
    seat.heartbeat(old).await;
    let left = seat.road.presence(Duration::from_secs(2)).await;
    assert!(left.is_some_and(|ms| ms > 4500 && ms <= 5000), "{left:?}");
    assert!(
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .remote_presence_live(Instant::now()),
        "the engine holds it"
    );
    seat.heartbeat(old + 1_000_000).await;
    assert_eq!(
        seat.road.presence(NOTHING).await,
        None,
        "a number never sent"
    );
    let (fresh, _) = seat.fresh_snapshot().await;
    let stranger = id();
    seat.road
        .say(&Up::Heartbeat {
            lease: stranger,
            state: fresh,
        })
        .await;
    assert_eq!(
        seat.road.presence(NOTHING).await,
        None,
        "not this computer's lease"
    );
    // The old number, once its snapshot is more than two seconds old: it arrived here after it was
    // taken, so this is later than two seconds after the shack took it.
    seat.keep_the_lease().await;
    tokio::time::sleep_until((arrived + FRESH + Duration::from_millis(100)).into()).await;
    seat.heartbeat(old).await;
    assert_eq!(
        seat.road.presence(NOTHING).await,
        None,
        "older than two seconds"
    );
    seat.keep_the_lease().await;
    let left = seat.renewed().await;
    assert!(left > 4500, "a fresh echo, the same moment: {left}");
}

/// ★ A renewal never waits for the Engine, and is not lost to it either: a fresh heartbeat that
/// lands while a test holds the Engine is handed over once the Engine is free, with no second
/// heartbeat, as a stream's renewal waits for its next tick. A radio loop whose tick keeps meeting
/// the heartbeat would otherwise let presence lapse under an over that was never in doubt.
/// CONTROL: nothing was answered while the Engine was held, and the engine holds presence after.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_renewal_that_finds_the_engine_busy_is_handed_over_once_it_is_free() {
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    seat.keep_the_lease().await;
    let (state, _) = seat.fresh_snapshot().await;
    let held_for = Duration::from_millis(400);
    let (held, holding) = std::sync::mpsc::channel();
    let engine = r.s.shared.engine.clone();
    let holder = std::thread::spawn(move || {
        let e = engine.lock().unwrap();
        held.send(Instant::now()).unwrap();
        std::thread::sleep(held_for);
        drop(e);
    });
    let since = holding.recv().unwrap();
    seat.heartbeat(state).await;
    let left = seat.road.presence(Duration::from_secs(2)).await;
    let answered = Instant::now();
    tokio::task::spawn_blocking(move || holder.join().unwrap())
        .await
        .unwrap();
    eprintln!(
        "measured: a renewal that found the Engine held for {held_for:?}, answered {:?} after it \
         was taken",
        answered - since
    );
    assert!(left.is_some(), "renewed once the Engine was free");
    assert!(
        answered - since >= held_for,
        "nothing was answered while the Engine was held"
    );
    assert!(
        answered - since < held_for + Duration::from_millis(250),
        "handed over at the road's next looks: {:?}",
        answered - since
    );
    assert!(r
        .s
        .shared
        .engine
        .lock()
        .unwrap()
        .remote_presence_live(Instant::now()));
}

/// ★ Both roads dead. A computer in control keeps presence with a fresh echo on the
/// road every 500 ms and its lease on the link; FT8 is armed at the shack and the radio loop runs at
/// its own tick. Then nothing more comes by either road (neither is closed, as when Wi-Fi drops),
/// and presence halts the station within five seconds and a tick of its last renewal. CONTROL: on
/// the air until presence could have lapsed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn both_roads_dead_presence_halts_within_five_seconds_and_a_tick() {
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    seat.renewed().await;
    {
        let mut e = r.s.shared.engine.lock().unwrap();
        e.set_tx_enabled(true);
        e.start_cq(None).unwrap();
        assert!(e.tx_enabled(), "premise: armed");
    }
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Ft8,
        Arc::default(),
        Instant::now() + 3 * PRESENCE,
    );
    let (mut sent, mut renewed) = (Instant::now(), Instant::now());
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        seat.keep_the_lease().await;
        let (state, _) = seat.fresh_snapshot().await;
        sent = Instant::now();
        seat.heartbeat(state).await;
        assert!(seat.road.presence(Duration::from_secs(2)).await.is_some());
        renewed = Instant::now();
    }
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let on = ticks
        .iter()
        .position(|&(_, on)| on)
        .expect("premise: never on the air");
    let off = on
        + ticks[on..]
            .iter()
            .position(|&(_, on)| !on)
            .expect("never came off the air");
    let (before, off) = (ticks[off - 1].0, ticks[off].0);
    eprintln!(
        "measured: both roads silent, off the air {:?} after the last renewal (between {:?} and \
         {:?} after its heartbeat was sent); the bound is {PRESENCE:?} and one {TICK:?} tick",
        off.saturating_duration_since(renewed),
        off.saturating_duration_since(renewed),
        off.saturating_duration_since(sent),
    );
    assert!(
        before < renewed + PRESENCE,
        "still on the air at a tick {:?} after the last renewal",
        before.saturating_duration_since(renewed)
    );
    assert!(
        off + Duration::from_millis(500) >= sent + PRESENCE,
        "off the air {:?} after the last heartbeat, before presence could lapse",
        off.saturating_duration_since(sent)
    );
    drop(seat);
}

/// ★ Presence comes from the road alone, so whatever is on the air, the road has just worked. The
/// lease is kept on the command link every 500 ms, and after one renewal nothing more comes by the
/// road: presence halts the station within five seconds and a tick of that renewal, while this
/// computer still holds the lease. The link keeps control (watch, tune, log), never presence.
/// CONTROL: on the air until presence could have lapsed; the lease still answers controlling after.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_link_alone_keeps_no_presence() {
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    seat.keep_the_lease().await;
    let (state, _) = seat.fresh_snapshot().await;
    let sent = Instant::now();
    seat.heartbeat(state).await;
    assert!(seat.road.presence(Duration::from_secs(2)).await.is_some());
    let renewed = Instant::now();
    {
        let mut e = r.s.shared.engine.lock().unwrap();
        e.set_tx_enabled(true);
        e.start_cq(None).unwrap();
        assert!(e.tx_enabled(), "premise: armed");
    }
    let done = Arc::new(AtomicBool::new(false));
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Ft8,
        done.clone(),
        Instant::now() + 3 * PRESENCE,
    );
    while renewed.elapsed() < PRESENCE + Duration::from_secs(1) {
        tokio::time::sleep(Duration::from_millis(500)).await;
        seat.keep_the_lease().await;
    }
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let on = ticks
        .iter()
        .position(|&(_, on)| on)
        .expect("premise: never on the air");
    let off = on
        + ticks[on..]
            .iter()
            .position(|&(_, on)| !on)
            .expect("the link alone kept it on the air");
    let (before, off) = (ticks[off - 1].0, ticks[off].0);
    eprintln!(
        "measured: the lease kept on the link and the road silent, off the air {:?} after the \
         last renewal; the bound is {PRESENCE:?} and one {TICK:?} tick",
        off.saturating_duration_since(renewed),
    );
    assert!(
        before < renewed + PRESENCE,
        "still on the air at a tick {:?} after the last renewal",
        before.saturating_duration_since(renewed)
    );
    assert!(
        off + Duration::from_millis(500) >= sent + PRESENCE,
        "off the air {:?} after the last heartbeat, before presence could lapse",
        off.saturating_duration_since(sent)
    );
    seat.keep_the_lease().await;
}

// ----- The held press -----

/// Frame `frame` of a voice: 20 ms of an 800 Hz tone at the encoder's 48 kHz, encoded as this
/// computer's microphone encodes it.
fn voice(encoder: &mut MicEncoder, frame: u32) -> Vec<u8> {
    let samples: Vec<f32> = (0..MIC_FRAME_SAMPLES)
        .map(|i| {
            let t = (frame as usize * MIC_FRAME_SAMPLES + i) as f32 / 48_000.0;
            0.4 * (2.0 * std::f32::consts::PI * 800.0 * t).sin()
        })
        .collect();
    encoder.encode(&samples).unwrap()
}

/// This computer's press `press`, spoken into for `frames` frames of 20 ms on the audio road from
/// frame `first`, and held again on the road every 100 ms while `held` says so of the frame: what
/// a screen and its headset send while PTT is down. When each hold went.
///
/// Each frame goes at its own moment on the media clock, `start` plus 20 ms a frame, as a headset's
/// does, so a test starved for a while catches up rather than falling behind for good: voice that
/// drifts later than the station plays it is refused as late, and the over then ends on its 200 ms
/// audio gap, not on whatever the test was timing. Each hold is noted as it was about to go, so
/// the station can never have taken it before the moment noted.
async fn speak(
    seat: &Seat,
    encoder: &mut MicEncoder,
    press: u64,
    first: u32,
    frames: u32,
    held: impl Fn(u32) -> bool,
) -> Vec<Instant> {
    let mut holds = Vec::new();
    let start = tokio::time::Instant::now();
    for frame in first..first + frames {
        if (frame - first).is_multiple_of(5) && held(frame) {
            holds.push(Instant::now());
            seat.road.say(&Up::Hold { press, stops: 0 }).await;
        }
        let packet = voice(encoder, frame);
        seat.audio
            .say(&audio_road::Up::Mic {
                press,
                frame,
                packet,
            })
            .await;
        tokio::time::sleep_until(start + Duration::from_millis(20) * (frame - first + 1)).await;
    }
    holds
}

/// The presses among what the road said.
fn presses(said: &[Down]) -> Vec<&Down> {
    said.iter()
        .filter(|down| matches!(down, Down::Press { .. }))
        .collect()
}

/// ★ A held press from afar: a held press on the road arms the microphone over in Phone, and its voice
/// keys it, only under the stream's transmit rules. With no presence (its screen's state fresh, the
/// heartbeat under a lease that is not this computer's), and with stale state (presence live, the
/// newest heartbeat naming a snapshot more than two seconds old), a press is refused, said at each
/// hold, and never reaches the held PTT. With presence live and the state fresh it is taken: the road says
/// it is keyed (the held PTT's own report, relayed), the over keys on its voice, and frames are
/// taken. CONTROL: the refused presses came with the shack in Phone, TX armed, and the radio loop
/// ticking, where the same press is taken.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_press_arms_the_microphone_only_under_the_stream_rules() {
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    seat.keep_the_lease().await;
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
    let done = Arc::new(AtomicBool::new(false));
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Voice,
        done.clone(),
        Instant::now() + 3 * PRESENCE,
    );
    // No presence, the state fresh.
    let (state, _) = seat.fresh_snapshot().await;
    seat.fresh_but_not_present(state).await;
    for _ in 0..3 {
        seat.road.say(&Up::Hold { press: 1, stops: 0 }).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let said = seat.road_says(Duration::from_millis(200)).await;
    let refused = |press| Down::Press {
        press,
        keyed: false,
        end: Some(HoldEnd::Refused),
    };
    assert_eq!(
        presses(&said),
        [&refused(1); 3],
        "no presence: refused, said at each hold: {said:?}"
    );
    assert!(!r.ptt.keyed());
    // Presence live, the state stale.
    let (state, arrived) = seat.fresh_snapshot().await;
    seat.heartbeat(state).await;
    assert!(seat.road.presence(Duration::from_secs(2)).await.is_some());
    tokio::time::sleep_until((arrived + FRESH + Duration::from_millis(100)).into()).await;
    seat.keep_the_lease().await;
    seat.heartbeat(state).await;
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .remote_presence_live(Instant::now()),
        "premise: presence live"
    );
    for _ in 0..3 {
        seat.road.say(&Up::Hold { press: 2, stops: 0 }).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let said = seat.road_says(Duration::from_millis(200)).await;
    assert_eq!(
        presses(&said),
        [&refused(2); 3],
        "stale state: refused, said at each hold: {said:?}"
    );
    assert!(!r.ptt.keyed());
    assert!(!r.s.shared.engine.lock().unwrap().mic_armed());
    // Both: taken.
    seat.renewed().await;
    let mut encoder = MicEncoder::new().unwrap();
    speak(&seat, &mut encoder, 3, 1, 30, |_| true).await;
    let keyed = r.s.shared.engine.lock().unwrap().mic_keyed();
    seat.road.say(&Up::Release { press: 3 }).await;
    let said = seat.road_says(Duration::from_millis(300)).await;
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    assert!(keyed, "the voice keyed the over");
    assert!(
        said.contains(&Down::Press {
            press: 3,
            keyed: true,
            end: None
        }),
        "the road said it keyed: {said:?}"
    );
    assert!(r.counts().mic_taken.load(Ordering::SeqCst) > 0);
    assert!(ticks.iter().any(|&(_, on)| on), "on the air");
}

/// ⛔ A held press from afar outside the operator's licence is refused as the shack's own PTT is:
/// in Phone on 20 m with a Technician licence, the shack's own PTT does not key, and a press from
/// afar, held and spoken into under live presence and fresh state, is said refused, never arms the
/// microphone over, and none of its frames is taken. CONTROL: with an Extra licence on the same
/// dial, the same press keys by its voice and its frames are taken.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_press_from_afar_outside_the_licence_is_refused_as_at_the_shack() {
    use tempo_app::settings::LicenseClass;
    let licensed = |r: &Roadside, class| {
        let mut e = r.s.shared.engine.lock().unwrap();
        let mut settings = e.settings().clone();
        settings.license_class = class;
        e.apply_settings(settings);
    };
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    {
        let mut e = r.s.shared.engine.lock().unwrap();
        e.set_operating_mode("phone", false);
        e.set_frequency(14.250, "20m", "USB");
    }
    licensed(&r, LicenseClass::Technician);
    {
        let mut e = r.s.shared.engine.lock().unwrap();
        assert!(
            e.tx_enabled() && !e.tx_allowed(),
            "premise: armed, outside the licence"
        );
        e.set_ptt(true);
        assert!(!e.manual_ptt(), "premise: the shack's own PTT refused");
    }
    let mut encoder = MicEncoder::new().unwrap();
    for (press, class) in [(1, LicenseClass::Technician), (2, LicenseClass::Extra)] {
        licensed(&r, class);
        seat.renewed().await;
        let taken = r.counts().mic_taken.load(Ordering::SeqCst);
        let radio = radio_loop(
            r.s.shared.engine.clone(),
            Over::Voice,
            Arc::default(),
            Instant::now() + Duration::from_millis(900),
        );
        speak(&seat, &mut encoder, press, 1, 30, |_| true).await;
        let keyed = r.s.shared.engine.lock().unwrap().mic_keyed();
        seat.road.say(&Up::Release { press }).await;
        let said = seat.road_says(Duration::from_millis(300)).await;
        let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
            .await
            .unwrap();
        let taken = r.counts().mic_taken.load(Ordering::SeqCst) - taken;
        if class == LicenseClass::Technician {
            assert!(
                said.contains(&Down::Press {
                    press,
                    keyed: false,
                    end: Some(HoldEnd::Refused)
                }),
                "outside the licence, said refused: {said:?}"
            );
            assert!(
                !ticks.iter().any(|&(_, on)| on),
                "on the air outside the licence"
            );
            assert!(!keyed && taken == 0, "keyed {keyed}, {taken} frames taken");
        } else {
            assert!(
                keyed && taken > 0,
                "CONTROL: keyed {keyed}, {taken} frames taken"
            );
            assert!(ticks.iter().any(|&(_, on)| on), "CONTROL: on the air");
        }
    }
}

/// ★ The held press ends 200 ms after its last re-assertion. A press from afar, keyed and
/// spoken into, whose holds stop while its voice goes on: the over comes off the air no sooner than
/// 200 ms after the last hold and within a tick past it, the road says the press lapsed, and the
/// frames that follow are refused. CONTROL: on the air until then, its voice still arriving.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_held_press_ends_200_ms_after_its_last_reassertion() {
    const GAP: Duration = Duration::from_millis(200);
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
    seat.renewed().await;
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Voice,
        Arc::default(),
        Instant::now() + 2 * PRESENCE,
    );
    let mut encoder = MicEncoder::new().unwrap();
    // Held for the first 40 frames, spoken into for 60.
    let holds = speak(&seat, &mut encoder, 9, 1, 60, |frame| frame <= 40).await;
    let last = *holds.last().unwrap();
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let said = seat.road_says(Duration::from_millis(300)).await;
    let on = ticks
        .iter()
        .position(|&(_, on)| on)
        .expect("premise: never on the air");
    let off = on
        + ticks[on..]
            .iter()
            .position(|&(_, on)| !on)
            .expect("never came off the air");
    let (before, off) = (ticks[off - 1].0, ticks[off].0);
    eprintln!(
        "measured: the held press came off the air {:?} after its last hold (on at a tick {:?} \
         after it); the bound is {GAP:?} and one {TICK:?} tick",
        off.saturating_duration_since(last),
        before.saturating_duration_since(last),
    );
    // How it ended first: an over ended another way (its voice refused as late, then its audio
    // gap) is that, not an early lapse.
    assert!(
        said.contains(&Down::Press {
            press: 9,
            keyed: false,
            end: Some(HoldEnd::Lapsed)
        }),
        "the road said it lapsed: {said:?}"
    );
    assert!(off >= last + GAP, "ended before the gap");
    assert!(
        off < last + GAP + TICK + Duration::from_millis(30),
        "ended {:?} after the last hold",
        off.saturating_duration_since(last)
    );
    assert!(
        r.counts().mic_refused.load(Ordering::SeqCst) > 0,
        "the frames after it refused"
    );
}

/// The shack's radio loop on a thread of its own until `done`: each tick, when, and whether the
/// microphone over was armed or keyed. It goes on ticking after an over ends, so a press that keyed
/// again would be seen.
fn ticking(r: &Roadside, done: Arc<AtomicBool>) -> std::thread::JoinHandle<Vec<(Instant, bool)>> {
    let engine = r.s.shared.engine.clone();
    std::thread::spawn(move || {
        let mut ticks = Vec::new();
        while !done.load(Ordering::SeqCst) {
            let now = Instant::now();
            let mut e = engine.lock().unwrap();
            e.poll_remote_transmit(now);
            e.poll_mic(now, TICK.as_millis() as f64, 1);
            ticks.push((now, e.mic_armed() || e.mic_keyed()));
            drop(e);
            std::thread::sleep(TICK);
        }
        ticks
    })
}

/// ⛔ A press from afar ends at its cap on the station's own clock, though its computer never lets
/// go: held and spoken into well past the cap (a second and a half here; Phone's Lock cap, three
/// minutes, in a build), the over comes off the air no sooner than the cap after the press's first
/// hold and within a hold and a tick past it; the road says the press was let go, every hold after
/// it is answered so, and nothing keys again while the holds go on. CONTROL: the over was on the
/// air by its voice within half a second of the first hold, and stayed on until the cap.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_press_from_afar_ends_at_its_cap_though_its_computer_holds_on() {
    const CAP: Duration = Duration::from_millis(1500);
    assert_eq!(PRESS_CAP, Duration::from_secs(180), "the product's cap");
    let app = App::new();
    let r = roadside_capped(&app, CAP);
    let mut seat = seated(&r).await;
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
    seat.renewed().await;
    let done = Arc::new(AtomicBool::new(false));
    let radio = ticking(&r, done.clone());
    let mut encoder = MicEncoder::new().unwrap();
    // Held and spoken into for two and a half seconds.
    let holds = speak(&seat, &mut encoder, 9, 1, 125, |_| true).await;
    let said = seat.road_says(Duration::from_millis(300)).await;
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let first = holds[0];
    let on = ticks
        .iter()
        .position(|&(_, on)| on)
        .expect("premise: never on the air");
    let off = on
        + ticks[on..]
            .iter()
            .position(|&(_, on)| !on)
            .expect("still on the air past the cap");
    let off_at = ticks[off].0;
    eprintln!(
        "measured: the press came off the air {:?} after its first hold; the cap is {CAP:?}",
        off_at.saturating_duration_since(first)
    );
    assert!(
        ticks[on].0 < first + Duration::from_millis(500),
        "CONTROL: keyed {:?} after its first hold",
        ticks[on].0.saturating_duration_since(first)
    );
    assert!(off_at >= first + CAP, "ended before the cap");
    assert!(
        off_at < first + CAP + Duration::from_millis(100) + TICK + Duration::from_millis(100),
        "ended {:?} after its first hold",
        off_at.saturating_duration_since(first)
    );
    assert!(
        !ticks[off..].iter().any(|&(_, on)| on),
        "keyed again while its computer held on"
    );
    let ended = Down::Press {
        press: 9,
        keyed: false,
        end: Some(HoldEnd::Released),
    };
    let presses = presses(&said);
    assert!(
        presses.last() == Some(&&ended) && presses.iter().filter(|&&d| *d == ended).count() >= 5,
        "the holds after the cap answered with its end: {said:?}"
    );
    assert!(!r.ptt.keyed());
}

/// ★ Each press's reports go back by the road that made it, and the stream's page is told only its
/// own: a computer's press keyed and released is said on its road; the page's reader finds none of
/// it, and a page's own press is told to the page and never on the road. CONTROL: the page's press
/// was keyed, and its report was there to read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_press_report_goes_back_by_its_own_road_and_never_to_the_page() {
    const PAGE: &str = "10000000-0000-4000-8000-00000000000a";
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
    seat.renewed().await;
    let done = Arc::new(AtomicBool::new(false));
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Phone,
        done.clone(),
        Instant::now() + 2 * PRESENCE,
    );
    let mut page = Vec::new();
    for _ in 0..4 {
        seat.road.say(&Up::Hold { press: 5, stops: 0 }).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        page.extend(crate::remote_service::stream::page_reports(&r.ptt));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    seat.road.say(&Up::Release { press: 5 }).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    page.extend(crate::remote_service::stream::page_reports(&r.ptt));
    let said = seat.road_says(Duration::from_millis(200)).await;
    assert_eq!(
        presses(&said),
        [
            &Down::Press {
                press: 5,
                keyed: true,
                end: None
            },
            &Down::Press {
                press: 5,
                keyed: false,
                end: Some(HoldEnd::Released)
            }
        ],
        "{said:?}"
    );
    assert!(
        page.is_empty(),
        "the page was told a road's press: {page:?}"
    );
    r.ptt.hold(PAGE, Instant::now());
    let mut armed = false;
    for _ in 0..50 {
        armed |= r.s.shared.engine.lock().unwrap().mic_armed();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    r.ptt.release(PAGE);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let told = crate::remote_service::stream::page_reports(&r.ptt);
    done.store(true, Ordering::SeqCst);
    drop(tokio::task::spawn_blocking(move || radio.join().unwrap()).await);
    assert!(armed, "premise: the page's press was keyed");
    assert!(
        told.iter()
            .any(|report| report.hold_id == PAGE && report.keyed),
        "the page's own: {told:?}"
    );
    assert!(
        presses(&seat.road_says(Duration::from_millis(200)).await).is_empty(),
        "a page's press was said on the road"
    );
}

/// ★ The stream's page takes only its own presses' reports: with a road's and a page's press each
/// reported, the page's reader takes the page's and leaves the road's for its road. CONTROL: the
/// road's are still there to take.
#[test]
fn the_pages_reader_leaves_a_roads_presses_for_its_road() {
    const PAGE: &str = "10000000-0000-4000-8000-00000000000a";
    let ptt = PttHold::default();
    let road = super::super::stop_road::press_id(&[1; 8], 4);
    let t0 = Instant::now();
    ptt.hold(&road, t0);
    ptt.tick(t0, true, false, |_| true);
    ptt.release(&road);
    ptt.tick(t0, true, true, |_| true);
    ptt.hold(PAGE, t0);
    ptt.tick(t0, true, false, |_| true);
    let page: Vec<String> = crate::remote_service::stream::page_reports(&ptt)
        .into_iter()
        .map(|report| report.hold_id)
        .collect();
    assert_eq!(page, [PAGE]);
    let left: Vec<String> = ptt.reports().into_iter().map(|r| r.hold_id).collect();
    assert_eq!(left, [road.clone(), road]);
}

/// ★ The microphone over's state goes to a computer whose presence is live, and to no other.
/// CONTROL: before its presence, nothing came.
#[tokio::test]
async fn the_microphone_over_is_told_while_presence_is_live() {
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    let (state, _) = seat.fresh_snapshot().await;
    seat.road.say(&Up::Heartbeat { lease: id(), state }).await;
    assert_eq!(seat.road.heard(NOTHING).await, None, "no presence, no word");
    seat.heartbeat(state).await;
    let mut heard = Vec::new();
    while let Some(down) = seat.road.heard(Duration::from_millis(300)).await {
        heard.push(down);
    }
    assert!(
        heard.contains(&Down::Mic(Default::default())),
        "the over's state: {heard:?}"
    );
}

/// ★ The microphone over's state crosses the road whole, its holes on the air included (since the
/// road's format 2); a body of the first format, which had no holes, is not read as one. CONTROL:
/// each end, and both counts at their largest, come back as they went.
#[test]
fn the_overs_state_crosses_the_road_with_its_holes() {
    use tempo_app::mic::{MicEnded, MicStatus};
    for ended in [None, Some(MicEnded::AudioGap), Some(MicEnded::RouteChanged)] {
        let down = Down::Mic(MicStatus {
            armed: true,
            keyed: ended.is_none(),
            no_power_out: false,
            ended,
            holes: 3,
            hole_ms: 420,
        });
        assert_eq!(Down::read(down.kind(), &down.body()), Some(down.clone()));
    }
    let widest = Down::Mic(MicStatus {
        holes: u32::MAX,
        hole_ms: u32::MAX,
        ..MicStatus::default()
    });
    assert_eq!(
        Down::read(widest.kind(), &widest.body()),
        Some(widest.clone())
    );
    assert_eq!(
        Down::read(widest.kind(), &widest.body()[..4]),
        None,
        "a body without the holes"
    );
    assert_eq!(
        widest.body(),
        [0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255],
        "the bytes the frozen format writes"
    );
}

/// ★ A hold crosses the road with the Stops its press began under (the road's format 3), and a
/// hold of the older format, which named none, is not read as one; a Stop's body is as it always
/// was, so a Stop of every format is still read. CONTROL: the largest numbers come back as they
/// went.
#[test]
fn a_hold_crosses_the_road_with_the_stops_its_press_began_under() {
    // It names a Stop, and the badge census reads every test that names one as reaching the badge.
    let _alone = alone();
    for (press, stops) in [(1, 0), (7, 3), (u64::MAX, u64::MAX)] {
        let up = Up::Hold { press, stops };
        assert_eq!(up.body().len(), 16);
        assert_eq!(Up::read(up.kind(), &up.body()), Some(up.clone()));
        assert_eq!(
            Up::read(up.kind(), &up.body()[..8]),
            None,
            "a hold that names no Stops"
        );
    }
    let stop = Up::Stop { stop: 5 };
    assert_eq!(stop.body(), 5u64.to_be_bytes());
    assert_eq!(Up::read(stop.kind(), &5u64.to_be_bytes()), Some(stop));
}

// ----- The audio road -----

/// ★ The welcome names each connection's own audio road beside its Stop road: one port for every
/// connection, not Stop's, and a tag of its own, sixteen lowercase hex characters. CONTROL: a shack
/// with no audio road names none.
#[tokio::test]
async fn the_welcome_names_each_connections_own_audio_road() {
    let app = App::new();
    let r = roadside(&app);
    let (one, two) = (seated(&r).await, seated_with(&r, false).await);
    assert_eq!(one.audio_offer["port"], two.audio_offer["port"]);
    assert_ne!(
        one.audio_offer["port"], one.offer["port"],
        "Stop's own port"
    );
    assert_ne!(one.audio_offer["tag"], two.audio_offer["tag"]);
    for offer in [&one.audio_offer, &two.audio_offer] {
        let tag = offer["tag"].as_str().unwrap();
        assert_eq!(tag.len(), 16);
        assert!(tag
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    }
    let plain = super::commands::running(&app);
    let (_stop, stopped) = watch::channel(false);
    let (_task, socket) = dial(&plain, &plain.computer.key, PEER, stopped).await;
    let mut socket = socket.unwrap();
    plain
        .shared
        .authority
        .permit_station(&plain.device, true)
        .unwrap();
    let welcome = hello(&mut socket, VERSIONS).await;
    assert!(welcome.get("audioRoad").is_none(), "{welcome}");
}

/// ★ The audio road takes only its own keys: a frame sealed under the keys Stop's label exports,
/// with the audio road's own tag, is dropped unread. CONTROL: the same frame under the audio road's
/// own keys is read (and then refused, as every press from afar is).
#[tokio::test]
async fn the_audio_road_takes_only_its_own_keys() {
    let app = App::new();
    let r = roadside(&app);
    let seat = seated(&r).await;
    let frame = audio_road::Up::Mic {
        press: 1,
        frame: 1,
        packet: vec![0x5a; 80],
    };
    let stops_keys = Keys::sized(
        &exported(&seat.socket, &seat.session),
        tag_of(&seat.audio_offer),
        End::Computer,
        AUDIO_MAX,
    )
    .unwrap();
    let datagram = stops_keys.send.seal(frame.kind(), &frame.body()).unwrap();
    seat.audio.udp.send(&datagram).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(r.counts().mic_read.load(Ordering::SeqCst), 0, "Stop's keys");
    seat.audio.say(&frame).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        r.counts().mic_read.load(Ordering::SeqCst),
        1,
        "its own keys"
    );
    assert_eq!(r.counts().mic_refused.load(Ordering::SeqCst), 1);
}

/// ★ Receive audio goes to a computer that listens and controls the station, one 20 ms frame a
/// datagram in the order of its numbers, each decoding as 20 ms; and to no other: a computer whose
/// lease is another connection's hears nothing, and one that stops listening is sent nothing more
/// after two seconds. CONTROL: the frames are the station's band, decodable here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn receive_audio_goes_to_a_listening_computer_in_control_and_to_no_other() {
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    let mut watcher = seated_with(&r, false).await;
    let band = r.on_the_air();
    let lease = seat.lease();
    watcher
        .audio
        .say(&audio_road::Up::Listen {
            lease: lease.clone(),
        })
        .await;
    seat.audio
        .say(&audio_road::Up::Listen {
            lease: lease.clone(),
        })
        .await;
    let heard = seat.audio.heard(Duration::from_millis(1000)).await;
    assert!(
        watcher
            .audio
            .heard(Duration::from_millis(300))
            .await
            .is_empty(),
        "a lease of another connection's is no admission"
    );
    assert!(heard.len() >= 30, "{} frames in a second", heard.len());
    let mut decoder = tempo_audio::receive_decode::ReceiveDecoder::new().unwrap();
    let mut last: Option<u32> = None;
    for audio_road::Down::Rx { seq, packet, .. } in &heard {
        assert!(last.is_none_or(|last| *seq > last), "out of order");
        last = Some(*seq);
        let samples = decoder.decode(packet).expect("a frame did not decode");
        assert_eq!(
            samples.len(),
            tempo_audio::receive_decode::PLAY_FRAME_SAMPLES
        );
    }
    // No more `listen`: the frames stop within the lapse.
    tokio::time::sleep(audio_road::LISTEN_LAPSE + Duration::from_millis(300)).await;
    let _ = seat.audio.heard(Duration::from_millis(50)).await;
    let later = seat.audio.heard(Duration::from_millis(500)).await;
    band.store(false, Ordering::SeqCst);
    assert!(
        later.is_empty(),
        "{} frames after it stopped listening",
        later.len()
    );
}

/// ★ A frame belongs to a press, and is refused before anything decodes it unless its press is the
/// one the held PTT keyed for this computer: frames for a press this computer never held, with its
/// presence live and the station in Phone, never key, never arm the microphone over, and not one
/// reaches it. CONTROL: every frame verified and was read on the audio road; the radio loop ran
/// throughout.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_microphone_of_a_press_not_keyed_is_refused_before_anything_decodes_it() {
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
    seat.renewed().await;
    let done = Arc::new(AtomicBool::new(false));
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Voice,
        done.clone(),
        Instant::now() + 3 * PRESENCE,
    );
    let mut encoder = MicEncoder::new().unwrap();
    for frame in 1..=50u32 {
        let voice: Vec<f32> = (0..MIC_FRAME_SAMPLES)
            .map(|i| {
                let t = (frame as usize * MIC_FRAME_SAMPLES + i) as f32 / 48_000.0;
                0.4 * (2.0 * std::f32::consts::PI * 800.0 * t).sin()
            })
            .collect();
        let packet = encoder.encode(&voice).unwrap();
        seat.audio
            .say(&audio_road::Up::Mic {
                press: 7,
                frame,
                packet,
            })
            .await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!r.ptt.keyed(), "the press is never keyed");
        let e = r.s.shared.engine.lock().unwrap();
        assert!(!e.mic_armed(), "the microphone over is never armed");
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let counts = r.counts();
    assert_eq!(
        counts.mic_read.load(Ordering::SeqCst),
        50,
        "premise: every frame verified and was read"
    );
    assert_eq!(counts.mic_refused.load(Ordering::SeqCst), 50);
    assert_eq!(
        counts.mic_taken.load(Ordering::SeqCst),
        0,
        "a frame reached the microphone over"
    );
    assert!(ticks.len() > 20, "premise: the radio loop ran");
    assert!(ticks.iter().all(|&(_, on)| !on), "never on the air");
}

/// ★ A frame from afar never feeds an over another press armed. With the microphone over armed by
/// the held PTT's own press, as a page's press arms it, and its feed open, the frames of this
/// computer's press 7 are refused: the held press is wired from afar, but press 7 is not the press
/// the held PTT keyed. None keys the over. CONTROL: the over stayed armed with its feed open
/// throughout, and every frame verified and was read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_frame_from_afar_never_feeds_an_over_another_press_armed() {
    const PAGE: &str = "10000000-0000-4000-8000-00000000000a";
    let app = App::new();
    let r = roadside(&app);
    let mut seat = seated(&r).await;
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
    seat.renewed().await;
    r.ptt.hold(PAGE, Instant::now());
    let done = Arc::new(AtomicBool::new(false));
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Voice,
        done.clone(),
        Instant::now() + 3 * PRESENCE,
    );
    let mut armed = false;
    for _ in 0..100 {
        armed = r.s.shared.engine.lock().unwrap().mic_armed();
        if armed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(armed, "premise: another press armed the over");
    let mut encoder = MicEncoder::new().unwrap();
    for frame in 1..=50u32 {
        if frame % 5 == 1 {
            r.ptt.hold(PAGE, Instant::now());
        }
        let voice: Vec<f32> = (0..MIC_FRAME_SAMPLES)
            .map(|i| {
                let t = (frame as usize * MIC_FRAME_SAMPLES + i) as f32 / 48_000.0;
                0.4 * (2.0 * std::f32::consts::PI * 800.0 * t).sin()
            })
            .collect();
        let packet = encoder.encode(&voice).unwrap();
        seat.audio
            .say(&audio_road::Up::Mic {
                press: 7,
                frame,
                packet,
            })
            .await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let e = r.s.shared.engine.lock().unwrap();
        assert!(e.mic_armed(), "premise: the over stayed armed");
        assert!(
            !e.mic_keyed(),
            "a frame from afar keyed another press's over"
        );
    }
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let counts = r.counts();
    assert_eq!(
        counts.mic_read.load(Ordering::SeqCst),
        50,
        "premise: all read"
    );
    assert_eq!(counts.mic_refused.load(Ordering::SeqCst), 50);
    assert_eq!(counts.mic_taken.load(Ordering::SeqCst), 0);
    assert!(ticks.iter().all(|&(_, on)| !on), "never on the air");
}

/// ★ The audio road hears only the listener's subnet, as Stop's road does: a frame that verifies
/// under the right keys and names the right road, from off that subnet, is dropped unread.
/// CONTROL: the same computer's frame on a road that hears it is read.
#[test]
fn the_audio_road_hears_only_its_subnet() {
    on_a_runtime(0, async {
        let frame = audio_road::Up::Mic {
            press: 1,
            frame: 1,
            packet: vec![0x5a; 80],
        };
        let app = App::new();
        let r = roadside_with(&app, AFTERLIFE, Hears::Subnet(home()));
        let seat = seated(&r).await;
        seat.audio.say(&frame).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(r.counts().mic_read.load(Ordering::SeqCst), 0, "read");
        drop(seat);

        let app = App::new();
        let r = roadside(&app);
        let seat = seated(&r).await;
        seat.audio.say(&frame).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(r.counts().mic_read.load(Ordering::SeqCst), 1, "control");
    });
}

/// ★ The audio road names nothing of the station's engine and nothing of Stop's road: no handle of
/// either is in its thread, so nothing the audio road does can hold a Stop up, nor a Stop wait for
/// it. CONTROL: the scan reads the road's code.
#[test]
fn the_audio_road_names_nothing_of_the_engine_or_stops_road() {
    let code: String = include_str!("../audio_road.rs")
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        code.contains("ReceiveFanout") && code.contains("MicFeed"),
        "the scan reads the code"
    );
    for name in [
        "SharedEngine",
        "Engine",
        "engine",
        "RoadStop",
        "RoadPresence",
        "StopRoad",
        "Outbox",
        "Commands",
        "tokio",
        "Link",
        "lan_client",
    ] {
        assert!(!code.contains(name), "the audio road names `{name}`");
    }
}

// ----- The snapshot's numbers -----

/// ★ The shack pushes the snapshot every 300 ms whether or not it changed, each with a new number,
/// so a live link's state echo always moves. The station's snapshot here never changes. CONTROL:
/// every value is the same.
#[tokio::test]
async fn an_unchanged_snapshot_is_pushed_every_300_ms_with_a_new_number() {
    let app = App::new();
    let mut r = roadside(&app);
    let runner: crate::command_dispatch::Runner = Arc::new(
        |_cmd: String, _args: Value, answer: crate::command_dispatch::Answer| {
            answer(Ok(json!({"unchanged": true})))
        },
    );
    let voided = r.s.shared.engine.lock().unwrap().remote_starts_voided();
    r.s.shared.commands = super::super::commands::Commands::new(runner, voided);
    let mut seat = seated(&r).await;
    let started = Instant::now();
    let mut seen: Vec<(u64, Instant)> = Vec::new();
    while started.elapsed() < Duration::from_millis(3100) {
        seen.push(seat.fresh_snapshot().await);
    }
    let numbers: Vec<u64> = seen.iter().map(|(n, _)| *n).collect();
    let gaps: Vec<u128> = seen
        .windows(2)
        .map(|w| (w[1].1 - w[0].1).as_millis())
        .collect();
    eprintln!(
        "measured: {} snapshots in 3.1 s, gaps {gaps:?} ms",
        seen.len()
    );
    assert_eq!(
        numbers,
        (numbers[0]..numbers[0] + numbers.len() as u64).collect::<Vec<_>>()
    );
    assert!(seen.len() >= 9, "{} snapshots", seen.len());
}

// ----- Who is heard, and what the road names -----

/// ★ Only the listener's own subnet is heard. CONTROL: an address on it is.
#[test]
fn the_road_hears_only_its_subnet() {
    let hears = Hears::Subnet(home());
    for (ip, heard) in [
        ("192.168.1.33", true),
        ("192.168.1.20", true),
        ("192.168.2.1", false),
        ("10.0.0.1", false),
        ("127.0.0.1", false),
    ] {
        assert_eq!(hears.hears(ip.parse().unwrap()), heard, "{ip}");
    }
}

/// ★ …and the road's thread asks it before anything else: a road that hears only a subnet this
/// computer is not on drops its Stop unread, though it is sealed under the right keys and names
/// the right road, and the station stays on the air. CONTROL: the same computer's Stop on a road
/// that hears it is taken.
#[test]
fn a_datagram_from_outside_the_subnet_is_dropped_unread() {
    let _alone = alone();
    on_a_runtime(0, async {
        let app = App::new();
        let r = roadside_with(&app, AFTERLIFE, Hears::Subnet(home()));
        let mut seat = seated(&r).await;
        key_the_mic(&r.s);
        assert_eq!(seat.road.road_stop(1, NOTHING).await, None, "not answered");
        assert!(keyed(&r.s), "a Stop from outside the subnet stops nothing");
        drop(seat);

        let app = App::new();
        let r = roadside(&app);
        let mut seat = seated(&r).await;
        key_the_mic(&r.s);
        assert!(seat
            .road
            .road_stop(1, Duration::from_secs(2))
            .await
            .is_some());
        assert!(came_to_a_stop(&r.s).await);
    });
}

/// ★ The road's thread names nothing of the station but its powers: no Engine, command link,
/// outbox, runtime, audio or authority of its own, nor does what an internet road's thread runs
/// besides (the client core's ICE and TURN client). CONTROL: the scan reads the code (names each
/// file uses are found).
#[test]
fn road_names_nothing_of_the_station() {
    let files = [
        (
            "stop_road.rs",
            include_str!("../stop_road.rs"),
            ["RoadStop", "PttHold"],
        ),
        (
            "tempo-client's ice.rs",
            include_str!("../../../../../crates/tempo-client/src/ice.rs"),
            ["IceAgent", "Turn"],
        ),
        (
            "tempo-client's turn.rs",
            include_str!("../../../../../crates/tempo-client/src/turn.rs"),
            ["CHANNEL_BIND", "long_term_key"],
        ),
    ];
    for (file, text, used) in files {
        let code: String = text
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        let code = &code[..code.find("#[cfg(test)]\nmod tests").unwrap_or(code.len())];
        assert!(
            used.iter().all(|name| code.contains(name)),
            "the scan reads {file}'s code"
        );
        for name in [
            "SharedEngine",
            "Engine",
            "engine",
            "Outbox",
            "outbox",
            "Lane",
            "Commands",
            "Runner",
            "tokio",
            "TcpStream",
            "WebSocket",
            "audio",
            "Authority",
            "channel::",
            "Link",
            "lan_client",
        ] {
            assert!(!code.contains(name), "{file} names `{name}`");
        }
    }
}

// ----- Client mode on the road, end to end -----

/// ★ Client mode on the shack's road, end to end over loopback: the welcome names the road, this
/// computer makes the same keys from its own side of the TLS session, the main window's own poll
/// renews presence at the shack, and a Stop press stops the station by the road. The shack's
/// handler here stops nothing itself (it records the call), so the station stopping is the road's
/// doing. CONTROL: no presence before the main window fetched a snapshot; the call went by the link
/// too.
#[test]
fn client_mode_keeps_presence_and_stops_the_station_on_the_road() {
    use super::computer::{behind_a_port, paired_record};
    use crate::client_mode::tests::{ask, eventually, scratch, screens};
    use crate::client_mode::{Client, Find, Phase, ThisComputer};
    use crate::remote_service::vault::PairedStation;
    use tauri::Manager;
    let _alone = alone();
    let app = App::new();
    let r = roadside(&app);
    let (at, _listening) = tauri::async_runtime::block_on(async {
        r.s.shared
            .authority
            .permit_station(&r.s.device, true)
            .unwrap();
        let (stopper, stop) = watch::channel(false);
        (behind_a_port(&r.s, stop).await, stopper)
    });
    let record = paired_record(&r.s, at);
    let dir = scratch("road-end-to-end");
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let client = Arc::new(Client::new(STATION.to_string(), this));
    let screens = screens(&client);
    let (device, key, pkcs8) = (
        record.device_id.clone(),
        record.station_key.clone(),
        record.pkcs8.clone(),
    );
    let find: Find = Arc::new(move |_| {
        Ok(Some(PairedStation {
            station_id: STATION.into(),
            device_id: device.clone(),
            station_key: key.clone(),
            pkcs8: pkcs8.clone(),
            addresses: vec![at.to_string()],
        }))
    });
    let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
        screens.main.app_handle().clone(),
        find,
        Arc::new(|_| Ok(Vec::new())),
        Some(tempo_stream::lan::Typed::At(at)),
    ));
    let presence = || {
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .remote_presence_live(Instant::now())
    };
    eventually("control taken", || client.status().phase == Phase::Live);
    std::thread::sleep(Duration::from_millis(700));
    assert!(!presence(), "no presence before the main window fetched");
    eventually("presence from the main window's poll", || {
        let _ = ask(&screens.main, "get_snapshot", json!({}));
        std::thread::sleep(Duration::from_millis(100));
        presence()
    });
    key_the_mic(&r.s);
    assert!(ask(&screens.main, "halt_tx", json!({})).is_ok());
    eventually("the station stopped by the road", || {
        let e = r.s.shared.engine.lock().unwrap();
        !e.manual_ptt() && !e.tx_enabled()
    });
    assert!(
        app.ran().contains(&"halt_tx".to_string()),
        "and by the link"
    );
    driving.abort();
    tauri::async_runtime::block_on(client.let_go());
    std::fs::remove_dir_all(&dir).unwrap();
}

// ----- A start from afar -----

/// A gate the shack's handler holds a CQ at while it is shut: the stand-in for a start that waits
/// its turn at the Engine.
type Gate = Arc<(Mutex<bool>, std::sync::Condvar)>;

fn gate(shut: bool) -> Gate {
    Arc::new((Mutex::new(shut), std::sync::Condvar::new()))
}

fn set_gate(gate: &Gate, shut: bool) {
    *gate.0.lock().unwrap() = shut;
    gate.1.notify_all();
}

/// The shack's handler for these tests, on its own engine: the starts wired from afar and the
/// cockpits' own stops as the app's handlers drive them (Enable TX, a CQ, the voice keyer's play;
/// Stop TX and the keyer's Stop), each command recorded in order but the snapshot feed's own polls;
/// a CQ waits at `gate` while it is shut. Set before a computer is seated: its lane takes the
/// handler as it opens.
fn on_the_engine(r: &mut Roadside, gate: Gate) -> Arc<Mutex<Vec<String>>> {
    held_on_the_engine(r, gate, "call_cq")
}

/// [`on_the_engine`], with `held` the command that waits at `gate` while it is shut, and the
/// section entries as the app's handlers make them. Each start asks first what its handler asks first, under the Engine lock it then runs in: whether a Stop
/// came after the command lane read it (`command_dispatch::still_bound`, with what the lane wrote
/// under `afar`).
fn held_on_the_engine(r: &mut Roadside, gate: Gate, held: &'static str) -> Arc<Mutex<Vec<String>>> {
    use crate::command_dispatch::{still_bound, Afar, AFAR};
    let ran = Arc::new(Mutex::new(Vec::new()));
    let (engine, record) = (r.s.shared.engine.clone(), ran.clone());
    let runner: crate::command_dispatch::Runner = Arc::new(
        move |cmd: String, args: Value, answer: crate::command_dispatch::Answer| {
            if cmd == "get_snapshot" {
                return answer(Ok(json!({})));
            }
            record.lock().unwrap().push(cmd.clone());
            if cmd == held {
                let (shut, opened) = &*gate;
                let mut shut = shut.lock().unwrap();
                while *shut {
                    shut = opened.wait(shut).unwrap();
                }
            }
            let afar: Option<Afar> = args
                .get(AFAR)
                .and_then(|afar| serde_json::from_value(afar.clone()).ok());
            let mut e = engine.lock().unwrap();
            let done = match cmd.as_str() {
                "set_tx_enabled" => {
                    let on = args["enabled"].as_bool() == Some(true);
                    let bound = if on {
                        still_bound(&e, afar.as_ref())
                    } else {
                        Ok(())
                    };
                    bound.map(|()| e.set_tx_enabled(on))
                }
                "call_cq" => still_bound(&e, afar.as_ref()).and_then(|()| e.start_cq(None)),
                "play_voice_message" => {
                    still_bound(&e, afar.as_ref()).and_then(|()| e.send_voice(vec![0.1; 12_000]))
                }
                // `lib.rs` `set_operating_mode`: `set_operating_mode_with_arming`, arming nothing
                // from afar.
                "set_operating_mode" => {
                    e.set_operating_mode_with_arming(
                        args["mode"].as_str().unwrap_or_default(),
                        args["followFreq"].as_bool() == Some(true),
                        afar.is_none(),
                    );
                    Ok(())
                }
                // `lib.rs` `work_spot`: `work_spot_tiered_with_arming`, arming nothing from afar.
                "work_spot" => {
                    e.work_spot_tiered_with_arming(
                        None,
                        args["mode"].as_str().unwrap_or_default(),
                        args["freqMhz"].as_f64().unwrap_or_default(),
                        args["band"].as_str().unwrap_or_default(),
                        None,
                        afar.is_none(),
                    );
                    Ok(())
                }
                // `lib.rs` `sstv_tune`: `sstv_tune_with_arming`, arming nothing from afar.
                "sstv_tune" => {
                    e.sstv_tune_with_arming(
                        args["dialMhz"].as_f64().unwrap_or_default(),
                        args["band"].as_str().unwrap_or_default(),
                        args["mode"].as_str().unwrap_or_default(),
                        afar.is_none(),
                    );
                    Ok(())
                }
                "halt_tx" => {
                    e.halt_tx();
                    Ok(())
                }
                "stop_voice" => {
                    e.stop_voice();
                    Ok(())
                }
                _ => Ok(()),
            };
            answer(done.map(|()| json!({ "ran": cmd })))
        },
    );
    let voided = r.s.shared.engine.lock().unwrap().remote_starts_voided();
    r.s.shared.commands = super::super::commands::Commands::new(runner, voided);
    ran
}

fn ran(record: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    record.lock().unwrap().clone()
}

/// ★ On the command link, a wired start runs only under the stream's transmit rules. With the
/// lease and the state its screen fetched fresh but no presence (the heartbeat under a lease that is
/// not this computer's), it is refused `noPresence`; with presence live but the newest heartbeat
/// naming a snapshot more than two seconds old, `staleState`; and nothing runs. With both, Enable
/// TX, a CQ and the voice keyer's play each run, and TX is armed from afar. CONTROL: the refusals
/// came from the computer holding the lease, where the same starts run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wired_start_runs_only_under_the_stream_rules() {
    let app = App::new();
    let mut r = roadside(&app);
    let record = on_the_engine(&mut r, gate(false));
    let mut seat = seated(&r).await;
    seat.keep_the_lease().await;
    let (state, _) = seat.fresh_snapshot().await;
    seat.fresh_but_not_present(state).await;
    tokio::time::sleep(Duration::from_millis(60)).await;
    let said = seat
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["refused"], "noPresence", "{said}");
    let (state, arrived) = seat.fresh_snapshot().await;
    seat.heartbeat(state).await;
    assert!(seat.road.presence(Duration::from_secs(2)).await.is_some());
    tokio::time::sleep_until((arrived + FRESH + Duration::from_millis(100)).into()).await;
    seat.keep_the_lease().await;
    seat.heartbeat(state).await;
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .remote_presence_live(Instant::now()),
        "premise: presence live"
    );
    let said = seat
        .call(2, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["refused"], "staleState", "{said}");
    assert!(ran(&record).is_empty(), "ran: {:?}", ran(&record));
    assert!(!r.s.shared.engine.lock().unwrap().tx_enabled());
    seat.renewed().await;
    for (id, cmd, args) in [
        (3, "set_tx_enabled", json!({"enabled": true})),
        (4, "call_cq", json!({})),
        (5, "play_voice_message", json!({"slot": 1})),
    ] {
        let said = seat.call(id, cmd, args).await;
        assert_eq!(said["value"]["ran"], cmd, "{said}");
    }
    assert_eq!(
        ran(&record),
        ["set_tx_enabled", "call_cq", "play_voice_message"]
    );
    assert!(
        r.s.shared.engine.lock().unwrap().tx_enabled(),
        "armed from afar"
    );
}

/// ★ One controller, the lease bound to the connection that took it. A second connection
/// of the same paired computer, without the lease, is refused `notController` for a start, and its
/// road's press is refused and said at each hold, never reaching the held PTT, since only the lease holder's
/// own lease mints presence on its own road: in Phone, with the holder's presence live at the
/// station and the radio loop ticking, where the holder's own press arms the microphone.
/// CONTROL: the connection holding the lease starts the same CQ, and its press arms.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_from_a_connection_without_the_lease_is_refused() {
    let app = App::new();
    let mut r = roadside(&app);
    let record = on_the_engine(&mut r, gate(false));
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
    let mut holder = seated(&r).await;
    holder.renewed().await;
    let mut other = seated_with(&r, false).await;
    let said = other.call(1, "call_cq", json!({})).await;
    assert_eq!(said["refused"], "notController", "{said}");
    let done = Arc::new(AtomicBool::new(false));
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Voice,
        done.clone(),
        Instant::now() + 2 * PRESENCE,
    );
    let mut armed = false;
    for _ in 0..4 {
        other.road.say(&Up::Hold { press: 1, stops: 0 }).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        armed |= r.s.shared.engine.lock().unwrap().mic_armed();
    }
    let said = other.road_says(Duration::from_millis(200)).await;
    let refused = Down::Press {
        press: 1,
        keyed: false,
        end: Some(HoldEnd::Refused),
    };
    assert_eq!(presses(&said), [&refused; 4], "{said:?}");
    assert!(
        !armed && !r.ptt.keyed(),
        "another connection's press armed the over"
    );
    assert!(ran(&record).is_empty());
    // One run of Nexus on both connections: its call ids follow on.
    let said = holder.call(2, "call_cq", json!({})).await;
    assert_eq!(said["value"]["ran"], "call_cq", "{said}");
    holder.road.say(&Up::Hold { press: 1, stops: 0 }).await;
    let mut held = false;
    for _ in 0..20 {
        held |= r.s.shared.engine.lock().unwrap().mic_armed();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    holder.road.say(&Up::Release { press: 1 }).await;
    done.store(true, Ordering::SeqCst);
    drop(tokio::task::spawn_blocking(move || radio.join().unwrap()).await);
    assert!(held, "CONTROL: the lease holder's own press arms");
}

/// ★ A start that comes to the Engine after its computer's presence went does not run, and the
/// station is stopped behind it. A CQ admitted under live presence waits its turn at the Engine (the
/// gate) while the lease is handed back, and with it presence: the lease's end retires the transmit
/// generation, so the CQ is answered `stopped` without running, and the station is stopped behind
/// it with TX off. CONTROL: the same CQ with presence kept runs and stays armed, with no Stop behind
/// it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_that_ran_after_presence_went_is_stopped_at_once() {
    let app = App::new();
    let mut r = roadside(&app);
    let held = gate(false);
    let record = on_the_engine(&mut r, held.clone());
    let mut seat = seated(&r).await;
    seat.renewed().await;
    let said = seat
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
    let said = seat.call(2, "call_cq", json!({})).await;
    assert_eq!(said["value"]["ran"], "call_cq", "{said}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(ran(&record), ["set_tx_enabled", "call_cq"], "CONTROL");
    assert!(r.s.shared.engine.lock().unwrap().tx_enabled(), "CONTROL");
    set_gate(&held, true);
    send(
        &mut seat.socket,
        json!({"type":"call","id":3,"cmd":"call_cq","args":{}}),
    )
    .await;
    for _ in 0..200 {
        if ran(&record).len() == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(ran(&record).len(), 3, "premise: the CQ waits at the Engine");
    let lease = seat.lease();
    let released = seat.ask(json!({"type":"release","leaseId":lease})).await;
    assert!(released["error"].is_null(), "{released}");
    assert!(
        !r.s.shared
            .engine
            .lock()
            .unwrap()
            .remote_presence_live(Instant::now()),
        "premise: presence went with the lease"
    );
    set_gate(&held, false);
    let said = loop {
        let said = seat.text().await;
        if said["type"] == "result" && said["id"] == 3 {
            break said;
        }
    };
    assert_eq!(said["refused"], "stopped", "{said}");
    for _ in 0..200 {
        if ran(&record).len() == 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        ran(&record),
        ["set_tx_enabled", "call_cq", "call_cq", "halt_tx"]
    );
    assert!(!r.s.shared.engine.lock().unwrap().tx_enabled(), "stopped");
}

/// ★ After a reconnect nothing transmits by itself. A computer arms TX from afar and keys
/// a held press in Phone; its link drops, and the station comes off the air with TX off. Back on a
/// new connection, the lease taken again and presence live, TX is still off, nothing goes on the
/// air, and the computer's next press is refused at the station (TX off). CONTROL: armed again
/// from afar, the next press keys.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn after_a_reconnect_nothing_keys_until_it_is_armed() {
    let app = App::new();
    let mut r = roadside(&app);
    let record = on_the_engine(&mut r, gate(false));
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
    let mut first = seated(&r).await;
    first.renewed().await;
    let said = first
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Voice,
        Arc::default(),
        Instant::now() + 3 * PRESENCE,
    );
    let mut encoder = MicEncoder::new().unwrap();
    speak(&first, &mut encoder, 1, 1, 20, |_| true).await;
    assert!(
        r.s.shared.engine.lock().unwrap().mic_keyed(),
        "premise: on the air"
    );
    drop(first);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    assert!(
        ticks.iter().any(|&(_, on)| on) && !ticks.last().unwrap().1,
        "came off the air"
    );
    assert!(came_to_a_stop(&r.s).await, "the drop stopped the station");
    // The same run of Nexus on its new connection: its next call ids follow on, as client mode's do.
    let mut again = seated(&r).await;
    again.renewed().await;
    assert!(
        !r.s.shared.engine.lock().unwrap().tx_enabled(),
        "TX stays off"
    );
    let done = Arc::new(AtomicBool::new(false));
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Voice,
        done.clone(),
        Instant::now() + 2 * PRESENCE,
    );
    let holds = speak(&again, &mut encoder, 1, 21, 20, |_| true).await.len();
    let said = again.road_says(Duration::from_millis(200)).await;
    let refused = Down::Press {
        press: 1,
        keyed: false,
        end: Some(HoldEnd::Refused),
    };
    // Refused at its first tick, in the held PTT's own report, and said again at each hold that
    // reached the road after it (the first hold too, when the tick came between that hold and the
    // road's look at it): never more than one for each hold and that one report.
    let told = presses(&said);
    assert!(
        !told.is_empty() && told.len() <= holds + 1 && told.iter().all(|down| **down == refused),
        "refused at the station, {holds} holds: {said:?}"
    );
    assert!(!r.s.shared.engine.lock().unwrap().mic_armed());
    let armed_again = Instant::now();
    let said = again
        .call(2, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
    speak(&again, &mut encoder, 2, 41, 20, |_| true).await;
    let keyed = r.s.shared.engine.lock().unwrap().mic_keyed();
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    assert!(keyed, "CONTROL: armed again, the next press keys");
    assert!(
        ticks.iter().all(|&(at, on)| !on || at > armed_again),
        "on the air before it was armed again"
    );
    assert_eq!(
        ran(&record),
        ["set_tx_enabled", "set_tx_enabled"],
        "nothing else was asked"
    );
}

/// What was started from afar, for the Stop test: FT armed and a CQ called on the command link, the
/// voice keyer's play on it, or a held press spoken into. Each needs its computer's presence live.
async fn start_from_afar(r: &Roadside, seat: &mut Seat, kind: &str) {
    let said = seat
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "{kind}: {said}");
    match kind {
        "ft" => {
            let said = seat.call(2, "call_cq", json!({})).await;
            assert_eq!(said["value"]["ran"], "call_cq", "{kind}: {said}");
            assert!(r.s.shared.engine.lock().unwrap().tx_enabled(), "{kind}");
        }
        "voice" => {
            let said = seat.call(2, "play_voice_message", json!({"slot": 1})).await;
            assert_eq!(said["value"]["ran"], "play_voice_message", "{kind}: {said}");
            assert!(
                r.s.shared.engine.lock().unwrap().tx_owner().is_some(),
                "{kind}"
            );
        }
        _ => {
            let mut encoder = MicEncoder::new().unwrap();
            speak(seat, &mut encoder, 1, 1, 20, |_| true).await;
            assert!(r.s.shared.engine.lock().unwrap().mic_keyed(), "{kind}");
        }
    }
}

/// ★ For what may now start from afar, either road alone stops each kind of over a
/// computer started (FT armed with a CQ, the voice keyer's message, a held press keyed by its
/// voice). The command link alone (the cockpit's own stop on it, as the link test of locally
/// started overs sends it; no datagram on the road), then the road alone (the shack reading nothing
/// from the link). CONTROL: each is on the air, or armed, before its stop.
#[test]
fn each_road_alone_stops_each_kind_of_over_started_from_afar() {
    let _alone = alone();
    on_a_runtime(2, async {
        for kind in ["ft", "voice", "press"] {
            for road in ["link", "road"] {
                let app = App::new();
                let mut r = roadside(&app);
                let record = on_the_engine(&mut r, gate(false));
                if kind == "press" {
                    r.s.shared
                        .engine
                        .lock()
                        .unwrap()
                        .set_operating_mode("phone", false);
                }
                let mut seat = seated(&r).await;
                seat.renewed().await;
                let done = Arc::new(AtomicBool::new(false));
                let radio = radio_loop(
                    r.s.shared.engine.clone(),
                    Over::Voice,
                    done.clone(),
                    Instant::now() + 2 * PRESENCE,
                );
                start_from_afar(&r, &mut seat, kind).await;
                if road == "link" {
                    for (n, cmd) in cockpit_stop(kind).into_iter().enumerate() {
                        let said = seat.call(9 + n as u64, cmd, json!({})).await;
                        assert_eq!(said["value"]["ran"], cmd, "{kind}: {said}");
                    }
                } else {
                    seat.valve.shut();
                    assert!(
                        seat.road
                            .road_stop(1, Duration::from_secs(2))
                            .await
                            .is_some(),
                        "{kind}"
                    );
                }
                assert!(came_to_a_stop(&r.s).await, "{kind} by the {road}");
                done.store(true, Ordering::SeqCst);
                drop(tokio::task::spawn_blocking(move || radio.join().unwrap()).await);
                let mut e = r.s.shared.engine.lock().unwrap();
                assert!(!e.mic_armed(), "{kind} by the {road}: the microphone over");
                match kind {
                    "ft" | "voice" => assert_stopped(&mut e, kind),
                    _ => assert!(!e.tx_enabled(), "{kind} by the {road}"),
                }
                drop(e);
                if road == "road" {
                    assert!(
                        !ran(&record).contains(&"halt_tx".to_string()),
                        "{kind}: the link ran no Stop"
                    );
                }
            }
        }
    });
}

/// ★ A start after presence has lapsed is refused. Presence renewed once by a fresh echo, and the
/// lease kept on the link, then nothing more on the road for five seconds: the same Enable TX that
/// ran while presence was live is refused `noPresence`, and nothing runs. CONTROL: run while it was
/// live, and the lease still answers controlling after.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_after_presence_lapsed_is_refused() {
    let app = App::new();
    let mut r = roadside(&app);
    let record = on_the_engine(&mut r, gate(false));
    let mut seat = seated(&r).await;
    seat.renewed().await;
    let renewed = Instant::now();
    let said = seat
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "CONTROL: {said}");
    while renewed.elapsed() < PRESENCE + Duration::from_millis(300) {
        tokio::time::sleep(Duration::from_millis(500)).await;
        seat.keep_the_lease().await;
    }
    let said = seat
        .call(2, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["refused"], "noPresence", "{said}");
    assert_eq!(ran(&record), ["set_tx_enabled"]);
    seat.keep_the_lease().await;
}

/// ★ A start on a lease taken again waits for presence of its own. Presence renewed by a fresh
/// echo, then the lease handed back and taken again on the same link: the Enable TX that follows,
/// with the state its road named still under two seconds old and the lease held, is refused
/// `noPresence` and runs nothing, since a lease handed back takes its presence with it and only a
/// heartbeat mints presence for the new one. CONTROL: one heartbeat later the same Enable TX runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_on_a_lease_taken_again_waits_for_a_heartbeat() {
    let app = App::new();
    let mut r = roadside(&app);
    let record = on_the_engine(&mut r, gate(false));
    let mut seat = seated(&r).await;
    seat.renewed().await;
    let (_, fetched) = seat.snapshot.expect("a snapshot");
    let lease = seat.lease();
    let released = seat.ask(json!({"type":"release","leaseId":lease})).await;
    assert!(released["error"].is_null(), "{released}");
    let state = seat.ask(json!({"type":"state"})).await;
    let acquired = seat
        .ask(json!({"type":"acquire","stationBootId":state["value"]["stationBootId"]}))
        .await;
    assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
    seat.state = acquired["value"].clone();
    let said = seat
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert!(
        fetched.elapsed() < FRESH - Duration::from_millis(200),
        "premise: the state the road named was still fresh at the start ({:?})",
        fetched.elapsed()
    );
    assert_eq!(said["refused"], "noPresence", "{said}");
    assert!(ran(&record).is_empty(), "{:?}", ran(&record));
    seat.renewed().await;
    let said = seat
        .call(2, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "CONTROL: {said}");
    assert_eq!(ran(&record), ["set_tx_enabled"]);
}

/// ★ A press from a connection whose presence went is refused, even under another's live presence.
/// A computer holding the lease, with presence and fresh state, hands the lease back, and a second
/// connection takes it and renews presence of its own at the station, in Phone with TX armed. The
/// first computer's next press, its last state still under two seconds old, is refused by its own
/// road, said at each hold, and never reaches the held PTT: the presence live at the station is the other
/// connection's, and nothing else here would have refused it. CONTROL: the new holder's own press
/// arms.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_press_from_a_connection_whose_presence_went_is_refused_under_anothers() {
    let app = App::new();
    let mut r = roadside(&app);
    let record = on_the_engine(&mut r, gate(false));
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
    let mut first = seated(&r).await;
    let mut second = seated_with(&r, false).await;
    first.renewed().await;
    let (_, fetched) = first.snapshot.expect("a snapshot");
    let lease = first.lease();
    let released = first.ask(json!({"type":"release","leaseId":lease})).await;
    assert!(released["error"].is_null(), "{released}");
    let state = second.ask(json!({"type":"state"})).await;
    let acquired = second
        .ask(json!({"type":"acquire","stationBootId":state["value"]["stationBootId"]}))
        .await;
    assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
    second.state = acquired["value"].clone();
    second.renewed().await;
    let said = second
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
    let done = Arc::new(AtomicBool::new(false));
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Voice,
        done.clone(),
        Instant::now() + 2 * PRESENCE,
    );
    let mut armed = false;
    for _ in 0..3 {
        first.road.say(&Up::Hold { press: 1, stops: 0 }).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        armed |= r.s.shared.engine.lock().unwrap().mic_armed();
    }
    assert!(
        fetched.elapsed() < FRESH - Duration::from_millis(200),
        "premise: the first computer's state was still fresh at its press ({:?})",
        fetched.elapsed()
    );
    let said = first.road_says(Duration::from_millis(200)).await;
    let refused = Down::Press {
        press: 1,
        keyed: false,
        end: Some(HoldEnd::Refused),
    };
    assert_eq!(presses(&said), [&refused; 3], "{said:?}");
    assert!(
        !armed && !r.ptt.keyed(),
        "a press with no presence of its own armed the over"
    );
    second.road.say(&Up::Hold { press: 1, stops: 0 }).await;
    let mut held = false;
    for _ in 0..20 {
        held |= r.s.shared.engine.lock().unwrap().mic_armed();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    second.road.say(&Up::Release { press: 1 }).await;
    done.store(true, Ordering::SeqCst);
    drop(tokio::task::spawn_blocking(move || radio.join().unwrap()).await);
    assert!(held, "CONTROL: the new holder's own press arms");
    assert_eq!(ran(&record), ["set_tx_enabled"]);
}

/// ★ Take control from a second computer, end to end. The first computer holds the station in
/// Phone with TX on and its press keeping its over on the air; a second, kept out, takes control
/// on its own connection. At once the over ends with nothing keyed, TX is off for the computer that
/// took it, and the first computer's next presses are refused by its road. CONTROL: the press was
/// on the air before, and the new holder's own press arms once it turns TX on itself. Then the
/// first takes it back the same way.
#[test]
fn a_second_computer_takes_control_and_the_first_ones_over_ends() {
    let _alone = alone();
    on_a_runtime(2, async {
        let app = App::new();
        let mut r = roadside(&app);
        let record = on_the_engine(&mut r, gate(false));
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .set_operating_mode("phone", false);
        let mut first = seated(&r).await;
        let mut second = seated_with(&r, false).await;
        first.renewed().await;
        let said = first
            .call(1, "set_tx_enabled", json!({"enabled": true}))
            .await;
        assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
        let done = Arc::new(AtomicBool::new(false));
        let radio = radio_loop(
            r.s.shared.engine.clone(),
            Over::Voice,
            done.clone(),
            Instant::now() + 2 * PRESENCE,
        );
        let mut on_air = false;
        for _ in 0..20 {
            first.road.say(&Up::Hold { press: 1, stops: 0 }).await;
            tokio::time::sleep(Duration::from_millis(10)).await;
            on_air |= r.s.shared.engine.lock().unwrap().mic_armed();
        }
        assert!(on_air, "premise: the first computer's over is on the air");

        let state = second.ask(json!({"type":"state"})).await;
        assert_eq!(state["value"]["phase"], "occupied", "{state}");
        let taken = second
            .ask(json!({"type":"takeControl","stationBootId":state["value"]["stationBootId"]}))
            .await;
        assert_eq!(taken["value"]["phase"], "controlling", "{taken}");
        second.state = taken["value"].clone();
        {
            let e = r.s.shared.engine.lock().unwrap();
            assert_eq!(
                (e.mic_armed(), r.ptt.keyed(), e.tx_enabled()),
                (false, false, false),
                "(over armed, keyed, TX on) the moment the second computer took control"
            );
        }
        let mut armed = false;
        for _ in 0..3 {
            first.road.say(&Up::Hold { press: 1, stops: 0 }).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            armed |= r.s.shared.engine.lock().unwrap().mic_armed();
        }
        assert!(
            !armed && !r.ptt.keyed(),
            "the first computer keyed again after control was taken"
        );
        let beat = first
            .ask(json!({"type":"heartbeat","leaseId":first.lease()}))
            .await;
        assert_eq!(beat["error"], "notController", "{beat}");

        // CONTROL: the new holder's own press arms, once it turns TX on itself. (Both seats are one
        // paired computer's runs here, so its call takes a number of its own.)
        second.renewed().await;
        let said = second
            .call(2, "set_tx_enabled", json!({"enabled": true}))
            .await;
        assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
        let mut held = false;
        for _ in 0..20 {
            second.road.say(&Up::Hold { press: 1, stops: 0 }).await;
            tokio::time::sleep(Duration::from_millis(10)).await;
            held |= r.s.shared.engine.lock().unwrap().mic_armed();
        }
        second.road.say(&Up::Release { press: 1 }).await;
        assert!(held, "CONTROL: the new holder's own press arms");

        // The first takes it back the same way.
        let state = first.ask(json!({"type":"state"})).await;
        let back = first
            .ask(json!({"type":"takeControl","stationBootId":state["value"]["stationBootId"]}))
            .await;
        assert_eq!(back["value"]["phase"], "controlling", "{back}");
        let beat = second
            .ask(json!({"type":"heartbeat","leaseId":second.lease()}))
            .await;
        assert_eq!(beat["error"], "notController", "{beat}");
        done.store(true, Ordering::SeqCst);
        drop(tokio::task::spawn_blocking(move || radio.join().unwrap()).await);
        assert_eq!(ran(&record), ["set_tx_enabled", "set_tx_enabled"]);
    });
}
