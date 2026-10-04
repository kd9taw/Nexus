//! The S4 drop tests over the LAN road, on the wall clock. An over is on the air at the shack under
//! a paired computer's stream presence, its lease taken on its connection to the shack's real
//! listener on this box's own private address; then, mid-over, the road goes silent (Wi-Fi off, a
//! cable pulled), the computer closes it, LAN is turned off at the shack, or the window on the
//! computer closes. The radio loop runs here at its own tick on a thread of its own, as in Nexus,
//! and each test measures when the over came off the air against the bound the design gives it, and
//! prints what it measured.
//!
//! What a stream carries itself (its heartbeats with a fresh picture, the held PTT and the
//! microphone) needs a WebRTC session, which this box's station cannot run: it captures no window.
//! Here the test hands them to the station as the stream's own channels would, and stops when the
//! road drops, since a network that drops carries the stream's channels with it. Each test is
//! skipped, saying so, on a box with no private IPv4 address of its own.
use super::computer::{page_ask, page_connect, page_socket, paired_record};
use super::*;
use crate::lan_client::tests::StationStore;
use crate::lan_client::{Origin, Reach, Stations};
use tempo_app::engine::Engine;
use tempo_app::mic::{MicFeed, MicFrame, MIC};
use tempo_app::remote_control::ptt_hold::PttHold;

/// The radio loop's tick (`tempo_audio::service`).
pub(super) const TICK: Duration = Duration::from_millis(20);
/// The bounds the design gives, written here rather than read from the code, so that a change
/// there cannot move them: presence lapses five seconds after the last fresh heartbeat; a voice
/// over ends 200 ms after its held PTT or its voice stops; a silent connection goes after 15 s.
const PRESENCE: Duration = Duration::from_secs(5);
const VOICE_GAP: Duration = Duration::from_millis(200);
const SILENCE: Duration = Duration::from_secs(15);
/// A healthy page's heartbeat with a fresh picture, and its held PTT's.
const HEARTBEAT: Duration = Duration::from_millis(500);
const HOLD_EVERY: Duration = Duration::from_millis(100);
/// "At once", for a road that closes or a switch turned off: the drop itself ends the over, not
/// presence's lapse, so it comes off well inside that. Each run prints what it measured.
const AT_ONCE: Duration = Duration::from_secs(1);
const PRESS: &str = "10000000-0000-4000-8000-00000000000a";

/// What is on the air, started through the picture, and how the station tells it has come off.
#[derive(Clone, Copy, Debug)]
pub(super) enum Over {
    /// The desktop's PTT, in Phone.
    Phone,
    /// A Tune carrier.
    Tune,
    /// An FT8 CQ: a local arm.
    Ft8,
    /// The page's own PTT over the streamed microphone: the held PTT arms it, and its voice keys.
    Voice,
}

impl Over {
    fn start(self, e: &mut Engine) {
        match self {
            Over::Phone => {
                e.set_operating_mode("phone", false);
                e.set_ptt(true);
            }
            Over::Tune => e.set_tune(true),
            Over::Ft8 => {
                e.set_tx_enabled(true);
                e.start_cq(None).unwrap();
            }
            // Pressed and spoken on the page: see `a_silent_road_ends_a_voice_over_at_its_gap`.
            Over::Voice => {}
        }
    }

    fn on_air(self, e: &Engine) -> bool {
        match self {
            Over::Phone => e.manual_ptt(),
            Over::Tune => e.tuning(),
            Over::Ft8 => e.tx_enabled(),
            Over::Voice => e.mic_keyed(),
        }
    }
}

/// The radio loop at its own tick, on a thread of its own as in Nexus: each tick takes the engine,
/// polls the stream's hold on the transmitter and the streamed microphone as `tempo_audio::service`
/// does, and notes whether `over` is on the air. It stops once the over has come off the air after
/// being on it, when `done` is set, or at `until`: every tick it ran, with what it found.
pub(super) fn radio_loop(
    engine: crate::SharedEngine,
    over: Over,
    done: Arc<AtomicBool>,
    until: Instant,
) -> std::thread::JoinHandle<Vec<(Instant, bool)>> {
    std::thread::spawn(move || {
        let mut ticks: Vec<(Instant, bool)> = Vec::new();
        loop {
            let now = Instant::now();
            let on = {
                let mut e = engine.lock().unwrap();
                e.poll_remote_transmit(now);
                e.poll_mic(now, TICK.as_millis() as f64, 1);
                over.on_air(&e)
            };
            let was_on = ticks.iter().any(|&(_, on)| on);
            ticks.push((now, on));
            if (was_on && !on) || done.load(Ordering::SeqCst) || now >= until {
                return ticks;
            }
            std::thread::sleep(TICK);
        }
    })
}

async fn ticks(radio: std::thread::JoinHandle<Vec<(Instant, bool)>>) -> Vec<(Instant, bool)> {
    tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap()
}

/// When the over came off the air: the tick before (which found it on) and the first tick that
/// found it off. From its first tick on the air it stayed there until `since`, the drop.
fn came_off(ticks: &[(Instant, bool)], since: Instant) -> (Instant, Instant) {
    let on = ticks
        .iter()
        .position(|&(_, on)| on)
        .expect("premise: never on the air");
    let off = on
        + ticks[on..]
            .iter()
            .position(|&(_, on)| !on)
            .expect("the over never came off the air");
    let (before, off) = (ticks[off - 1].0, ticks[off].0);
    assert!(
        off >= since,
        "off the air {:?} before the drop",
        since - off
    );
    (before, off)
}

/// The road between the computer and the shack, through a cable that can be pulled: once `cut`,
/// nothing crosses either way and neither end is told (both sockets stay open), as when Wi-Fi drops
/// or a cable is pulled at the computer. `gone` says when the shack gave up on its end.
struct Cable {
    at: SocketAddr,
    cut: watch::Sender<bool>,
    gone: tokio::sync::oneshot::Receiver<Instant>,
}

async fn cable(to: SocketAddr) -> Cable {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let at = listener.local_addr().unwrap();
    let (cut, mut pulled) = watch::channel(false);
    let (gave_up, gone) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (computer, _) = listener.accept().await.unwrap();
        let shack = tokio::net::TcpStream::connect(to).await.unwrap();
        let (mut from_computer, mut to_computer) = computer.into_split();
        let (mut from_shack, mut to_shack) = shack.into_split();
        let was_cut = tokio::select! {
            _ = async {
                let _ = tokio::join!(
                    tokio::io::copy(&mut from_computer, &mut to_shack),
                    tokio::io::copy(&mut from_shack, &mut to_computer),
                );
            } => false,
            _ = pulled.wait_for(|cut| *cut) => true,
        };
        if was_cut {
            // Nothing crosses now. The shack's end is still read, and thrown away, only to learn
            // when the shack gives up on it; the computer's end is left as it is.
            let _ = tokio::io::copy(&mut from_shack, &mut tokio::io::sink()).await;
            let _ = gave_up.send(Instant::now());
            std::future::pending::<()>().await;
        }
    });
    Cable { at, cut, gone }
}

/// The computer's road to `at` (the listener, or a cable in front of it): welcomed, in control, and
/// presence held as a live stream holds it. Its socket, its session and the acquire's state.
async fn controlling_at(s: &Shack, at: SocketAddr) -> (Client, String, Value) {
    let stream = tokio::net::TcpStream::connect(at).await.unwrap();
    let mut socket = open(stream, &s.computer.key, &s.public_key).await.unwrap();
    let (session, state) = in_control(s, &mut socket).await;
    renewed(s, &mut socket, &session, &state).await;
    (socket, session, state)
}

fn start(s: &Shack, over: Over) {
    let mut e = s.shared.engine.lock().unwrap();
    over.start(&mut e);
    assert!(over.on_air(&e), "premise: {over:?} on the air");
}

// ----- The road drops: silent -----

/// The road goes silent mid-over: a healthy stream for a second and a half (a fresh heartbeat
/// every 500 ms), then the cable is pulled between two heartbeats.
async fn silent_road(over: Over) {
    let Some(r) = listening().await else {
        return;
    };
    let cable = cable(r.at).await;
    let (mut socket, session, state) = controlling_at(&r.s, cable.at).await;
    start(&r.s, over);
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        over,
        Arc::default(),
        Instant::now() + 3 * PRESENCE,
    );
    let mut last = Instant::now();
    for _ in 0..3 {
        tokio::time::sleep(HEARTBEAT).await;
        last = renewed(&r.s, &mut socket, &session, &state).await;
    }
    tokio::time::sleep(HEARTBEAT / 2).await;
    let cut = Instant::now();
    cable.cut.send(true).unwrap();
    let (before, off) = came_off(&ticks(radio).await, cut);
    eprintln!(
        "measured: {over:?}, the road silent mid-over: off the air {:?} after the last fresh \
         heartbeat, {:?} after the cable was pulled; the bound is presence's {PRESENCE:?} plus one \
         {TICK:?} tick",
        off - last,
        off - cut
    );
    assert!(
        before < last + PRESENCE,
        "{over:?}: still on the air at a tick {:?} after the last fresh heartbeat",
        before - last
    );
    // The model, not the bound: nothing tells the shack of a silent road, so only presence ends it.
    assert!(
        off + HEARTBEAT >= last + PRESENCE,
        "{over:?}: off the air {:?} after the last fresh heartbeat, before presence lapsed",
        off - last
    );
    drop(socket);
}

/// ★ An FT8 CQ started through the picture, when the road goes silent (Wi-Fi off at the computer, a
/// cable pulled): nothing tells the shack, and presence lapses five seconds after the last fresh
/// heartbeat; the radio loop halts it on its next tick. CONTROL: on the air until then.
#[tokio::test]
async fn a_silent_road_halts_ft8_when_presence_lapses() {
    silent_road(Over::Ft8).await;
}

/// ★ A Tune carrier started through the picture, when the road goes silent: halted as FT8 is, five
/// seconds after the last fresh heartbeat and a tick (the design's "Tune halts at 6 s" is this, with
/// the last heartbeat a second in). CONTROL: on the air until then.
#[tokio::test]
async fn a_silent_road_halts_tune_when_presence_lapses() {
    silent_road(Over::Tune).await;
}

/// ★ A voice over, the page's own PTT over the streamed microphone, when the road goes silent: the
/// page's held PTT (re-asserted every 100 ms) and its voice (a frame every 20 ms) stop reaching the
/// station with the network, and the over unkeys when the first of the two 200 ms gaps runs out, on
/// the radio loop's next tick: long before presence would lapse. CONTROL: it keyed, and stayed keyed
/// through a second and a half of a healthy stream.
#[tokio::test]
async fn a_silent_road_ends_a_voice_over_at_its_gap() {
    let Some(r) = listening().await else {
        return;
    };
    let (hold, feed) = (PttHold::default(), MicFeed::default());
    {
        let mut e = r.s.shared.engine.lock().unwrap();
        e.set_remote_ptt_hold(hold.clone());
        e.set_remote_mic_feed(feed.clone());
        e.set_operating_mode("phone", false);
    }
    let cable = cable(r.at).await;
    let (mut socket, session, state) = controlling_at(&r.s, cable.at).await;
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Voice,
        Arc::default(),
        Instant::now() + PRESENCE,
    );
    // The page's held PTT and voice on a thread of their own, as the page's microphone and its
    // heartbeats are independent of each other: when each last reached the station.
    let fed = Arc::new(Mutex::new(None::<(Instant, Instant)>));
    let speaking = Arc::new(AtomicBool::new(true));
    let page = {
        let (hold, feed, fed, speaking) =
            (hold.clone(), feed.clone(), fed.clone(), speaking.clone());
        std::thread::spawn(move || {
            let samples = MIC.samples(20);
            let (mut held, mut k): (Option<Instant>, u64) = (None, 0);
            while speaking.load(Ordering::SeqCst) {
                let now = Instant::now();
                if held.is_none_or(|held| now >= held + HOLD_EVERY) {
                    hold.hold(PRESS, now);
                    held = Some(now);
                }
                // Refused until the press has armed the over, as the page's first frames are.
                let _ = feed.push(MicFrame {
                    seq: k + 1,
                    media: samples * k,
                    arrived: now,
                    samples: vec![0.5; samples as usize],
                });
                k += 1;
                *fed.lock().unwrap() = held.map(|held| (held, now));
                std::thread::sleep(TICK);
            }
        })
    };
    for _ in 0..3 {
        tokio::time::sleep(HEARTBEAT).await;
        renewed(&r.s, &mut socket, &session, &state).await;
    }
    let dropped = Instant::now();
    speaking.store(false, Ordering::SeqCst);
    cable.cut.send(true).unwrap();
    tokio::task::spawn_blocking(move || page.join().unwrap())
        .await
        .unwrap();
    let (held, spoke) = fed.lock().unwrap().expect("premise: the page spoke");
    let (before, off) = came_off(&ticks(radio).await, dropped);
    eprintln!(
        "measured: a voice over, the road silent mid-over: unkeyed {:?} after the last held PTT, \
         {:?} after the last voice frame; the bound is the {VOICE_GAP:?} gap plus one {TICK:?} \
         tick",
        off - held,
        off - spoke
    );
    assert!(
        before < held.min(spoke) + VOICE_GAP,
        "still keyed at a tick {:?} after the last held PTT",
        before - held
    );
    drop(socket);
}

/// ★ A silent road's connection itself: the shack drops it once nothing has come from the computer
/// for its silence limit (15 s; a healthy computer pings every 5 s), and the lease with it, though
/// presence ended the over long before. Measured from the computer's last word.
#[tokio::test]
async fn a_silent_road_is_dropped_at_its_silence_limit() {
    let Some(r) = listening().await else {
        return;
    };
    let cable = cable(r.at).await;
    let (mut socket, session, state) = controlling_at(&r.s, cable.at).await;
    start(&r.s, Over::Phone);
    renewed(&r.s, &mut socket, &session, &state).await;
    let last_word = Instant::now();
    cable.cut.send(true).unwrap();
    let gone = tokio::time::timeout(SILENCE + AT_ONCE * 5, cable.gone)
        .await
        .expect("the shack kept a silent connection")
        .unwrap();
    eprintln!(
        "measured: the road silent: the shack dropped the connection {:?} after the computer's \
         last word; the limit is {SILENCE:?}",
        gone - last_word
    );
    assert!(
        gone + HEARTBEAT >= last_word + SILENCE,
        "dropped {:?} after the last word, inside the silence limit",
        gone - last_word
    );
    assert!(
        gone <= last_word + SILENCE + AT_ONCE,
        "kept {:?} after the last word",
        gone - last_word
    );
    assert!(halted(&r.s), "the over outlived its connection");
    assert_eq!(
        r.s.shared.authority.local_status()["controller"],
        Value::Null
    );
    drop(socket);
}

// ----- The road drops: closed, LAN off, the window -----

/// ★ The computer closes the road mid-over (its Nexus closes the connection: TCP closes): the
/// session ends with its presence at the shack, and the radio loop halts the over on its next tick,
/// at once rather than when presence would have lapsed. For a Phone PTT, a Tune carrier and an FT8
/// CQ. CONTROL: each was on the air until the close.
#[tokio::test]
async fn a_road_closed_mid_over_ends_it_at_once() {
    for over in [Over::Phone, Over::Tune, Over::Ft8] {
        let Some(r) = listening().await else {
            return;
        };
        let (socket, _, _) = controlling_at(&r.s, r.at).await;
        start(&r.s, over);
        let radio = radio_loop(
            r.s.shared.engine.clone(),
            over,
            Arc::default(),
            Instant::now() + PRESENCE,
        );
        tokio::time::sleep(HEARTBEAT).await;
        let closed = Instant::now();
        drop(socket);
        let (_, off) = came_off(&ticks(radio).await, closed);
        eprintln!(
            "measured: {over:?}, the road closed mid-over: off the air {:?} after the close",
            off - closed
        );
        assert!(
            off - closed < AT_ONCE,
            "{over:?}: off the air only {:?} after the close",
            off - closed
        );
    }
}

/// ★ LAN turned off at the shack mid-over: the port closes and every session ends, each stream's
/// presence with it, and the radio loop halts the over on its next tick, at once. For a Phone PTT, a
/// Tune carrier and an FT8 CQ. CONTROL: each was on the air until LAN went off.
#[tokio::test]
async fn lan_off_at_the_shack_mid_over_ends_it_at_once() {
    for over in [Over::Phone, Over::Tune, Over::Ft8] {
        let Some(r) = listening().await else {
            return;
        };
        let (socket, _, _) = controlling_at(&r.s, r.at).await;
        start(&r.s, over);
        let radio = radio_loop(
            r.s.shared.engine.clone(),
            over,
            Arc::default(),
            Instant::now() + PRESENCE,
        );
        tokio::time::sleep(HEARTBEAT).await;
        let switched = Instant::now();
        r.lan.turn_off();
        let (_, off) = came_off(&ticks(radio).await, switched);
        eprintln!(
            "measured: {over:?}, LAN off at the shack mid-over: off the air {:?} after the switch",
            off - switched
        );
        assert!(
            off - switched < AT_ONCE,
            "{over:?}: off the air only {:?} after LAN off",
            off - switched
        );
        drop(socket);
    }
}

/// ★ The window on the computer closes mid-over: its origin stops (the window's Destroyed handler
/// drops it), which ends the page's session and its road to the shack, and the shack ends the lease
/// and the over at once. Through the window's real client and its own page socket, for a Phone PTT,
/// a Tune carrier and an FT8 CQ. CONTROL: each was on the air until the window closed.
#[tokio::test]
async fn the_window_closing_mid_over_ends_it_at_once() {
    for over in [Over::Phone, Over::Tune, Over::Ft8] {
        let Some(r) = listening().await else {
            return;
        };
        let SocketAddr::V4(at) = r.at else {
            unreachable!("the shack listens on IPv4 only")
        };
        let store = StationStore::default();
        Stations::new(Arc::new(store.clone()))
            .keep(&paired_record(&r.s, at))
            .unwrap();
        let origin = Origin::start(Reach {
            assets: Arc::new(|_: &str| None),
            stations: Arc::new(Stations::new(Arc::new(store.clone()))),
            find: Arc::new(|_| Ok(Vec::new())),
            name: "Den PC".into(),
        })
        .unwrap();
        let mut page = page_socket(&origin).await;
        let connected = page_connect(&mut page, None).await;
        assert_eq!(connected["type"], "connected", "{connected}");
        let state = page_ask(&mut page, json!({"type":"state"})).await;
        let acquired = page_ask(
            &mut page,
            json!({"type":"acquire","stationBootId":state["value"]["stationBootId"]}),
        )
        .await;
        assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
        let now = Instant::now();
        let permit =
            r.s.shared
                .authority
                .stream_presence(
                    connected["sessionId"].as_str().unwrap(),
                    &r.s.device,
                    acquired["value"]["leaseId"].as_str().unwrap(),
                    now,
                )
                .unwrap();
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .hold_remote_presence(permit, now);
        start(&r.s, over);
        let radio = radio_loop(
            r.s.shared.engine.clone(),
            over,
            Arc::default(),
            Instant::now() + PRESENCE,
        );
        tokio::time::sleep(HEARTBEAT).await;
        let closed = Instant::now();
        drop(origin);
        let (_, off) = came_off(&ticks(radio).await, closed);
        eprintln!(
            "measured: {over:?}, the window closed mid-over: off the air {:?} after it closed",
            off - closed
        );
        assert!(
            off - closed < AT_ONCE,
            "{over:?}: off the air only {:?} after the window closed",
            off - closed
        );
        drop(page);
    }
}
