//! Client mode over the internet road, end to end: this computer's client (`crate::client_mode`)
//! on Tauri's mock runtime, its station's addresses on this network answering nothing, and the
//! station reached instead through the shack's real tunnel behind an in-memory room, by a stand-in
//! for the Remote service ([`Afar`]) that opens the pipe, counts each renewal and can refuse one.
//! The shack is the real one, its commands running; its internet roads bind loopback, and this
//! computer's go through a TURN stand-in, or through nothing at all where this network is said
//! to block UDP.
use super::commands::{running, App};
use super::computer::{behind_a_port, paired_record};
use super::tunnel::{approved_for, piped, through_record, tunnel_of, APPROVED};
use super::*;
use crate::client_mode::tests::{ask, eventually, scratch, screens};
use crate::client_mode::{Afar, Client, Find, Phase, ThisComputer, Timing};
use crate::lan_client::audio_road::{Headset, CUSHION_MS, INTERNET_CUSHION_MS};
use crate::lan_client::link::{Internet, Link};
use crate::lan_client::tests::where_nothing_listens;
use crate::remote_service::tunnel::Approved;
use crate::remote_service::vault::PairedStation;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use tauri::Manager;
use tempo_client::ice::Relay;
use tempo_client::turn::stand_in::StandIn;

const USER: &str = "road";
const PASSWORD: &str = "throwaway-password";

/// The Remote service as client mode meets it: a pipe to the shack's tunnel each time it is asked
/// to open, its roads through the TURN stand-in at `relay` (or, with none, gathering nothing: a
/// network that lets no UDP through), and each renewal counted and answered as `renewal` says.
struct Service {
    tunnel: Tunnel,
    stop: watch::Receiver<bool>,
    station_key: String,
    pkcs8: String,
    relay: Option<std::net::SocketAddr>,
    road_kept: AtomicBool,
    opens: AtomicUsize,
    renewals: AtomicUsize,
    renewal: Mutex<Result<(), &'static str>>,
    room: Arc<Room>,
}

/// What the room (or the internet on the way to it) does to the pipe's bytes once, or from now on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    /// One byte changed in the next piece this computer sends.
    Tamper,
    /// One byte changed in the next piece the shack sends.
    TamperDown,
    /// The next piece this computer sends, delivered twice.
    Duplicate,
    /// The next piece this computer sends, held back and delivered after the one behind it.
    Reorder,
}

/// The room between this computer and the shack, in the computer's pipe: it passes the bytes
/// either way as they come, but can act on them once ([`Act`]), or go silent both ways for good
/// (the internet gone, nobody told).
#[derive(Default)]
struct Room {
    act: Mutex<Option<Act>>,
    acted: AtomicBool,
    silent: AtomicBool,
}

impl Room {
    fn arm(&self, act: Act) {
        self.acted.store(false, Ordering::SeqCst);
        *self.act.lock().unwrap() = Some(act);
    }

    /// The act armed for a piece going this way, taken: it is done once.
    fn take(&self, up: bool) -> Option<Act> {
        let mut act = self.act.lock().unwrap();
        let mine = act.filter(|act| (*act == Act::TamperDown) != up);
        if mine.is_some() {
            *act = None;
            self.acted.store(true, Ordering::SeqCst);
        }
        mine
    }

    /// The computer's end of `pipe`, through this room.
    fn between(self: &Arc<Self>, pipe: tokio::io::DuplexStream) -> tokio::io::DuplexStream {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (computer, room) = tokio::io::duplex(256 * 1024);
        let (mut from_computer, mut to_computer) = tokio::io::split(room);
        let (mut from_shack, mut to_shack) = tokio::io::split(pipe);
        let down = Arc::clone(self);
        tokio::spawn(async move {
            let mut buf = vec![0u8; 64 * 1024];
            while let Ok(n @ 1..) = from_shack.read(&mut buf).await {
                if down.silent.load(Ordering::SeqCst) {
                    continue;
                }
                if down.take(false).is_some() {
                    buf[n - 1] ^= 0x01;
                }
                if to_computer.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
            let _ = to_computer.shutdown().await;
        });
        let up = Arc::clone(self);
        tokio::spawn(async move {
            let mut buf = vec![0u8; 64 * 1024];
            let mut held: Option<Vec<u8>> = None;
            while let Ok(n @ 1..) = from_computer.read(&mut buf).await {
                if up.silent.load(Ordering::SeqCst) {
                    continue;
                }
                let mut piece = buf[..n].to_vec();
                let mut out = Vec::new();
                match up.take(true) {
                    Some(Act::Tamper) => {
                        piece[n - 1] ^= 0x01;
                        out.push(piece);
                    }
                    Some(Act::Duplicate) => out.extend([piece.clone(), piece]),
                    Some(Act::Reorder) => held = Some(piece),
                    _ => {
                        out.push(piece);
                        out.extend(held.take());
                    }
                }
                for piece in out {
                    if to_shack.write_all(&piece).await.is_err() {
                        return;
                    }
                }
            }
            let _ = to_shack.shutdown().await;
        });
        computer
    }
}

impl Afar for Service {
    fn has_road(&self, _: &str) -> bool {
        self.road_kept.load(Ordering::SeqCst)
    }

    fn open(
        &self,
        _: String,
        headset: Option<Arc<dyn Headset>>,
    ) -> Pin<Box<dyn Future<Output = Result<Link, &'static str>> + Send>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let (tunnel, stop, relay) = (self.tunnel.clone(), self.stop.clone(), self.relay);
        let room = Arc::clone(&self.room);
        let record = PairedStation {
            addresses: Vec::new(),
            station_id: STATION.into(),
            device_id: APPROVED.into(),
            station_key: self.station_key.clone(),
            pkcs8: self.pkcs8.clone(),
        };
        Box::pin(async move {
            let internet = Internet {
                bind: Ipv4Addr::LOCALHOST.into(),
                direct: false,
                stun: None,
                relay: relay.map(|server| Relay {
                    server,
                    username: USER.into(),
                    password: PASSWORD.into(),
                }),
            };
            let pipe = room.between(piped(&tunnel, stop).await);
            Link::connect_tunnel(pipe, &record, 41, headset, &internet).await
        })
    }

    fn renew(&self, _: String) -> Pin<Box<dyn Future<Output = Result<(), &'static str>> + Send>> {
        self.renewals.fetch_add(1, Ordering::SeqCst);
        let said = *self.renewal.lock().unwrap();
        Box::pin(async move { said })
    }

    fn heard_key(&self, _: &str, _: &str) {}
}

/// The shack, running, with its tunnel taking this computer; client mode on this computer with its
/// station's address on this network at `lan` (shared, so a test can move it), the stand-in
/// service, the clocks a test can wait for, and a stand-in headset.
struct Afield {
    app: App,
    s: Shack,
    client: Arc<Client>,
    screens: crate::client_mode::tests::Screens,
    service: Arc<Service>,
    /// The station's address on this network, as the operator typed it: nothing listens there
    /// until a test puts the station's listener there ([`Afield::on_this_network`]).
    lan: SocketAddrV4,
    headset: Arc<tempo_client::audio_road::StandIn>,
    driving: tauri::async_runtime::JoinHandle<()>,
    dir: std::path::PathBuf,
    _turn: Option<StandIn>,
    _listening: watch::Sender<bool>,
}

impl Afield {
    fn new(label: &str, udp: bool, timing: Timing) -> Self {
        Self::kept(label, udp, timing, true)
    }

    /// The same, with an internet road kept for the station or none.
    fn kept(label: &str, udp: bool, timing: Timing, kept: bool) -> Self {
        Self::with(label, udp, timing, kept, true)
    }

    /// The same, with this network's record of the station kept or none: none for a station kept by
    /// its service id, which this computer has never reached on its network.
    fn with(label: &str, udp: bool, timing: Timing, kept: bool, on_this_network: bool) -> Self {
        let app = App::new();
        let s = running(&app);
        // The tunnel grants no control of its own: the shack gives it, as to any computer.
        s.shared.authority.permit_station(APPROVED, true).unwrap();
        let approved = Arc::new(Approved::default());
        approved.set(approved_for(Some(&s.computer)));
        let tunnel = tunnel_of(&s, approved.roster(), &approved);
        let (listening, stop) = watch::channel(false);
        let turn = udp.then(|| StandIn::start(USER, PASSWORD));
        let record = through_record(&s, &s.public_key);
        let service = Arc::new(Service {
            tunnel,
            stop,
            station_key: record.station_key.clone(),
            pkcs8: record.pkcs8.clone(),
            relay: turn.as_ref().map(|turn| turn.address),
            road_kept: AtomicBool::new(kept),
            opens: AtomicUsize::new(0),
            renewals: AtomicUsize::new(0),
            renewal: Mutex::new(Ok(())),
            room: Arc::default(),
        });
        let dir = scratch(label);
        let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
        let client = Arc::new(Client::new(STATION.to_string(), this));
        let headset = tempo_client::audio_road::StandIn::new();
        client.record_from(Arc::clone(&headset) as Arc<dyn Headset>);
        client.reach_afar(Arc::clone(&service) as Arc<dyn Afar>);
        client.time_with(timing);
        let screens = screens(&client);
        let lan = where_nothing_listens(Ipv4Addr::LOCALHOST);
        let find: Find = {
            // This network's own record: the device id the station's LAN listener gave.
            let (key, pkcs8, device) = (
                record.station_key.clone(),
                record.pkcs8.clone(),
                s.device.clone(),
            );
            Arc::new(move |_| {
                Ok(on_this_network.then(|| PairedStation {
                    station_id: STATION.into(),
                    device_id: device.clone(),
                    station_key: key.clone(),
                    pkcs8: pkcs8.clone(),
                    addresses: vec![lan.to_string()],
                }))
            })
        };
        let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
            screens.main.app_handle().clone(),
            find,
            Arc::new(|_| Ok(Vec::new())),
            Some(tempo_stream::lan::Typed::At(lan)),
        ));
        Self {
            app,
            s,
            client,
            screens,
            service,
            lan,
            headset,
            driving,
            dir,
            _turn: turn,
            _listening: listening,
        }
    }

    fn live_over_the_internet(&self) {
        eventually("control taken over the internet", || {
            let status = self.client.status();
            status.phase == Phase::Live && status.internet
        });
    }

    /// The station's listener on this network, at the address the operator typed.
    fn on_this_network(&self, stop: watch::Receiver<bool>) {
        let shared = self.s.shared.clone();
        let at = self.lan;
        tauri::async_runtime::block_on(async move {
            let listener = tokio::net::TcpListener::bind(at).await.unwrap();
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(channel::connection(
                        stream,
                        PEER.parse().unwrap(),
                        shared.clone(),
                        stop.clone(),
                    ));
                }
            });
        });
    }

    fn end(self) {
        self.driving.abort();
        tauri::async_runtime::block_on(self.client.let_go());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The clocks a test can wait for: the datagram roads' grace, a try of this network, "Still
/// there?" and its end, and the pipe's renewal, each far shorter than the app's.
fn quick() -> Timing {
    Timing {
        udp_grace: Duration::from_millis(1500),
        lan_probe: Duration::from_millis(300),
        idle_prompt: Duration::from_secs(60),
        idle_end: Duration::from_secs(61),
        renew: Duration::from_millis(200),
        ..Timing::default()
    }
}

/// ★ With nothing answering at the station's address on this network, client mode drives it over
/// the internet road: control taken through the room, the shack's snapshot answering the screens'
/// poll, a station command run at the shack, the receive cushion at the internet's 120 ms, the
/// pipe renewed while the link lives, and the banner told it is the internet. CONTROL: with no
/// internet road kept for the station, nothing is opened over the internet and the station is said
/// to be out of reach, as before.
#[test]
fn off_this_network_client_mode_drives_the_station_over_the_internet() {
    let _alone = alone();
    let t = Afield::new("afar", true, quick());
    t.live_over_the_internet();
    eventually("the shack's snapshot here", || {
        ask(&t.screens.main, "get_snapshot", json!({})).is_ok_and(|s| s["n"].is_u64())
    });
    assert!(ask(&t.screens.main, "get_settings", json!({})).is_ok());
    assert!(t.app.ran().contains(&"get_settings".to_string()));
    assert_eq!(t.headset.rings().cushion_ms(), INTERNET_CUSHION_MS);
    eventually("the pipe renewed", || {
        t.service.renewals.load(Ordering::SeqCst) >= 2
    });
    assert!(!t.client.status().udp_blocked, "{:?}", t.client.status());
    assert_eq!(t.service.opens.load(Ordering::SeqCst), 1);
    t.end();

    let t = Afield::kept("afar-none", true, quick(), false);
    eventually("said to be out of reach", || {
        let reason = t.client.status().reason;
        reason.is_some_and(|reason| crate::lan_client::road::UNREACHED.contains(&reason.as_str()))
    });
    std::thread::sleep(Duration::from_millis(1000));
    let status = t.client.status();
    assert_eq!(
        t.service.opens.load(Ordering::SeqCst),
        0,
        "opened over the internet with no road kept"
    );
    assert!(
        !status.internet && status.phase != Phase::Live,
        "{status:?}"
    );
    t.end();
}

/// ★ Signed in from elsewhere: a station kept by its service id (this computer has no record of it on this
/// network, never having reached it there) is driven over the internet road from the first try, as
/// a station off this network is: control taken through the room, the shack's snapshot answering
/// the screens' poll, the banner told the internet is the station's only road. Operate needs no
/// LAN record for it. CONTROL: with no internet road kept either, the station is unknown here and
/// nothing is opened.
#[test]
fn a_station_kept_by_its_service_id_is_driven_over_the_internet_road() {
    let _alone = alone();
    let t = Afield::with("afar-away", true, quick(), true, false);
    t.live_over_the_internet();
    eventually("the shack's snapshot here", || {
        ask(&t.screens.main, "get_snapshot", json!({})).is_ok_and(|s| s["n"].is_u64())
    });
    let status = t.client.status();
    assert!(status.away, "{status:?}");
    assert_eq!(t.service.opens.load(Ordering::SeqCst), 1);
    t.end();

    let t = Afield::with("afar-away-none", true, quick(), false, false);
    eventually("said to be unknown here", || {
        t.client.status().reason.as_deref() == Some("unknownStation")
    });
    let status = t.client.status();
    assert!(!status.away && status.phase == Phase::Refused, "{status:?}");
    assert_eq!(t.service.opens.load(Ordering::SeqCst), 0);
    t.end();
}

/// ⛔ Over the internet on a network that lets no UDP through, the datagram roads never find a
/// path: the banner says so, a start and a press are refused here as `udpBlocked` with nothing
/// sent, and the shack never keys; Stop still goes over the link and runs at the shack. CONTROL:
/// the same link with a relay is not said to be blocked (the test above).
#[test]
fn with_udp_blocked_over_the_internet_nothing_starts_and_stop_still_runs() {
    let _alone = alone();
    let t = Afield::new("afar-blocked", false, quick());
    t.live_over_the_internet();
    eventually("said to be blocked", || t.client.status().udp_blocked);
    assert_eq!(
        ask(&t.screens.main, "call_cq", json!({})),
        Err(json!("nexusClient:udpBlocked"))
    );
    assert_eq!(
        ask(&t.screens.main, "set_ptt", json!({"on": true})),
        Err(json!("nexusClient:udpBlocked"))
    );
    assert!(ask(&t.screens.main, "halt_tx", json!({})).is_ok());
    let ran = t.app.ran();
    assert!(!ran.contains(&"call_cq".to_string()), "{ran:?}");
    assert!(ran.contains(&"halt_tx".to_string()), "{ran:?}");
    assert!(!t.s.shared.engine.lock().unwrap().mic_armed());
    t.end();
}

/// ★ "Still there?" on the internet: with nothing from the operator the banner asks, then the
/// link ends, control is handed back at once (another computer finds it free), and nothing
/// connects again until the operator answers; then it does. CONTROL: input reported before the
/// prompt keeps the link live past the end.
#[test]
fn nobody_still_there_ends_the_internet_link_until_the_operator_answers() {
    let _alone = alone();
    let timing = Timing {
        idle_prompt: Duration::from_millis(2500),
        idle_end: Duration::from_millis(4000),
        ..quick()
    };
    let t = Afield::new("afar-idle", true, timing);
    t.live_over_the_internet();
    // CONTROL: the operator at work keeps it live past the end.
    for _ in 0..12 {
        t.client.still_here();
        std::thread::sleep(Duration::from_millis(500));
    }
    let status = t.client.status();
    assert!(
        status.phase == Phase::Live && !status.still_there,
        "{status:?}"
    );
    eventually("asked", || t.client.status().still_there);
    eventually("ended", || {
        let status = t.client.status();
        status.phase == Phase::Refused && status.reason.as_deref() == Some("idle")
    });
    let opened = t.service.opens.load(Ordering::SeqCst);
    // Control was handed back: another computer on this network, given control, finds it free.
    t.s.shared
        .authority
        .permit_station(&t.s.device, true)
        .unwrap();
    let (_stopper, stop) = watch::channel(false);
    let at = tauri::async_runtime::block_on(behind_a_port(&t.s, stop));
    let free = tauri::async_runtime::block_on(async {
        let (other, _) = Link::connect(&paired_record(&t.s, at), Some(at), 7, None)
            .await
            .expect("another link");
        let mut phase = Value::Null;
        for _ in 0..100 {
            let answer = other
                .operation(json!({"type":"state","requestId":id()}))
                .await
                .expect("an answer");
            if answer["error"] != "stationBusy" && answer["error"] != "remoteBusy" {
                phase = answer["value"]["phase"].clone();
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        other.close();
        phase
    });
    assert_eq!(free, "available");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        t.service.opens.load(Ordering::SeqCst),
        opened,
        "connected again with nobody there"
    );
    t.client.still_here();
    eventually("connected again", || {
        t.service.opens.load(Ordering::SeqCst) > opened
    });
    t.live_over_the_internet();
    t.end();
}

/// ★ LAN first: while on the internet, the station answering on this network again ends the
/// internet link for this network's road, with the LAN cushion back. CONTROL: while it answers
/// nothing here, the link stays on the internet across several tries.
#[test]
fn the_station_on_this_network_again_takes_this_networks_road() {
    let _alone = alone();
    let t = Afield::new("afar-lan", true, quick());
    t.live_over_the_internet();
    std::thread::sleep(Duration::from_millis(1500));
    let status = t.client.status();
    assert!(status.internet && status.phase == Phase::Live, "{status:?}");
    assert_eq!(t.service.opens.load(Ordering::SeqCst), 1);
    let (_stopper, stop) = watch::channel(false);
    t.s.shared
        .authority
        .permit_station(&t.s.device, true)
        .unwrap();
    t.on_this_network(stop);
    let at = t.lan;
    let mut last = t.client.status();
    for _ in 0..100 {
        last = t.client.status();
        if last.phase == Phase::Live && !last.internet && last.station == at.to_string() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        last.phase == Phase::Live && !last.internet && last.station == at.to_string(),
        "never on this network at {at}: {last:?}, opened over the internet {} times",
        t.service.opens.load(Ordering::SeqCst)
    );
    assert_eq!(t.headset.rings().cushion_ms(), CUSHION_MS);
    t.end();
}

/// ★ A renewal the service refuses for good (the device no longer approved there) ends the link,
/// and the next open's refusal is said. CONTROL: renewals answered keep it live (the first test).
#[test]
fn a_renewal_refused_for_good_ends_the_internet_link() {
    let _alone = alone();
    let t = Afield::new("afar-renew", true, quick());
    t.live_over_the_internet();
    *t.service.renewal.lock().unwrap() = Err("notApprovedThere");
    eventually("the link ended", || t.client.status().phase != Phase::Live);
    t.end();
}

// ----- The internet road dropping, and a room that is not honest -----

/// "At once", for a link the shack has ended: the end itself ends the press, not a lapse.
const AT_ONCE: Duration = Duration::from_secs(1);

/// Each tick of the shack's loop, and whether the microphone over was armed at it.
type Ticks = Vec<(Instant, bool)>;
/// What the screen was answered at each re-assertion of its press.
type Said = Vec<Result<Value, Value>>;

/// The shack's radio loop at its own tick, on a thread of its own as in Nexus: each tick polls the
/// press's hold on the transmitter and the streamed microphone as `tempo_audio::service` does, and
/// notes whether the microphone over is armed, until `done`.
fn shack_loop(
    engine: crate::SharedEngine,
    done: Arc<AtomicBool>,
) -> std::thread::JoinHandle<Ticks> {
    std::thread::spawn(move || {
        let mut ticks = Vec::new();
        while !done.load(Ordering::SeqCst) {
            let now = Instant::now();
            let armed = {
                let mut e = engine.lock().unwrap();
                e.poll_remote_transmit(now);
                e.poll_mic(now, super::drops::TICK.as_millis() as f64, 1);
                e.mic_armed()
            };
            ticks.push((now, armed));
            std::thread::sleep(super::drops::TICK);
        }
        ticks
    })
}

/// When the over first came off the air after `since`, having been on until then.
fn off_after(ticks: &[(Instant, bool)], since: Instant) -> Option<Instant> {
    assert!(
        ticks.iter().any(|&(at, armed)| armed && at < since),
        "premise: never on the air before {since:?}"
    );
    ticks
        .iter()
        .find(|&&(at, armed)| at >= since && !armed)
        .map(|&(at, _)| at)
}

/// A press held on this computer's screen, as the screen holds one: the shack in Phone, the first
/// press taken, then re-asserted every 100 ms until `done`, each answer kept, while the screen
/// keeps reading the shack's state (presence wants it fresh); and the shack's loop.
///
/// The screen keeps its two timers apart, as the screen's are: the press is re-asserted every
/// 100 ms from the last re-assertion's start, and the state is read every 300 ms on a thread of
/// its own. So the time a call takes, the state's included, never lengthens the gap between two
/// holds, which the shack ends a press at (200 ms, `ptt_hold::GAP`). Paced by a sleep after its
/// calls, with the state read in line every third time, a loaded run stretched one gap past that
/// and the shack, rightly, ended the press under a screen still holding it.
///
/// The first re-assertion is due 100 ms after the press that took was sent, as the screen's timer
/// starts with the press itself. Timed from the screen thread's start instead, the first gap also
/// held the press's answer on its way back and the thread starting, and a starved run stretched it
/// past 200 ms the same way.
struct Held {
    done: Arc<AtomicBool>,
    screen: std::thread::JoinHandle<Said>,
    shack: std::thread::JoinHandle<Ticks>,
}

impl Held {
    fn on(t: &Afield) -> Self {
        {
            // The engine holds the stream's press and microphone as the service gives them it.
            let mut e = t.s.shared.engine.lock().unwrap();
            e.set_remote_ptt_hold(t.s.shared.feeds.stream.ptt.clone());
            e.set_remote_mic_feed(t.s.shared.feeds.stream.mic.clone());
            e.set_operating_mode("phone", false);
        }
        let done = Arc::new(AtomicBool::new(false));
        let shack = shack_loop(t.s.shared.engine.clone(), done.clone());
        let mut pressed = Instant::now();
        eventually("presence live for a press", || {
            let _ = ask(&t.screens.main, "get_snapshot", json!({}));
            pressed = Instant::now();
            ask(&t.screens.main, "set_ptt", json!({"on": true})).is_ok()
        });
        let (window, until) = (t.screens.main.clone(), done.clone());
        let poll = {
            let (window, until) = (window.clone(), until.clone());
            std::thread::spawn(move || {
                while !until.load(Ordering::SeqCst) {
                    let _ = ask(&window, "get_snapshot", json!({}));
                    std::thread::sleep(Duration::from_millis(300));
                }
            })
        };
        let screen = std::thread::spawn(move || {
            let mut said = Vec::new();
            let mut last = pressed;
            loop {
                std::thread::sleep(
                    (last + Duration::from_millis(100)).saturating_duration_since(Instant::now()),
                );
                if until.load(Ordering::SeqCst) {
                    break;
                }
                last = Instant::now();
                said.push(ask(&window, "set_ptt", json!({"on": true})));
            }
            poll.join().unwrap();
            said
        });
        eventually("the press armed at the shack", || {
            t.s.shared.engine.lock().unwrap().mic_armed()
        });
        Self {
            done,
            screen,
            shack,
        }
    }

    fn end(self) -> (Said, Ticks) {
        self.done.store(true, Ordering::SeqCst);
        (self.screen.join().unwrap(), self.shack.join().unwrap())
    }
}

/// ★ A relay alone (no direct path is even tried, as behind a network that allows no other): the
/// road's datagrams cross the relay both ways, a press held on the screen arms the shack's
/// microphone over, it stays armed while held, and letting go disarms it.
#[test]
fn over_a_relay_alone_a_held_press_keys_the_shack_and_letting_go_unkeys_it() {
    let _alone = alone();
    let t = Afield::new("afar-relay", true, quick());
    t.live_over_the_internet();
    let held = Held::on(&t);
    std::thread::sleep(Duration::from_millis(1000));
    assert!(
        t.s.shared.engine.lock().unwrap().mic_armed(),
        "a held press stayed armed"
    );
    let crossed = t._turn.as_ref().unwrap().seen();
    assert!(
        crossed.sent_by_channel + crossed.sent_by_indication > 0
            && crossed.delivered_by_channel + crossed.delivered_by_indication > 0,
        "the road never crossed the relay both ways: {crossed:?}"
    );
    let (said, _) = held.end();
    assert!(said.iter().all(Result::is_ok), "{said:?}");
    assert!(ask(&t.screens.main, "set_ptt", json!({"on": false})).is_ok());
    eventually("let go at the shack", || {
        !t.s.shared.engine.lock().unwrap().mic_armed()
    });
    t.end();
}

/// ⛔ The internet road drops mid-over. The room ends the pipe (the shack ends the session, and
/// the press with it at once), or the whole internet goes silent both ways with nobody told (the
/// pipe and the relay alike: the press ends at its hold's gap). Either way the shack's microphone
/// over comes off, the screen's next re-assertions are refused (`pressEnded`), and nothing at the
/// shack keys again. CONTROL: each press was armed and held until the drop.
#[test]
fn over_the_internet_a_link_dropped_mid_press_unkeys_the_shack_and_nothing_stays_keyed() {
    let _alone = alone();
    for silent in [false, true] {
        let t = Afield::new("afar-drop", true, quick());
        t.live_over_the_internet();
        let held = Held::on(&t);
        std::thread::sleep(Duration::from_millis(500));
        let dropped = Instant::now();
        if silent {
            t.service.room.silent.store(true, Ordering::SeqCst);
            t._turn.as_ref().unwrap().mute(true);
        } else {
            t._listening.send(true).unwrap();
        }
        // Silent, presence as this computer heard it lapses only at its deadline (5 s), and the
        // screen's press ends there.
        std::thread::sleep(Duration::from_millis(if silent { 6500 } else { 2500 }));
        let (said, ticks) = held.end();
        let off = off_after(&ticks, dropped).expect("the press never came off the air");
        eprintln!(
            "measured: silent={silent}: off the air {:?} after the drop",
            off - dropped
        );
        assert!(
            off - dropped < AT_ONCE,
            "silent={silent}: off the air only {:?} after the drop",
            off - dropped
        );
        let after = ticks.iter().filter(|&&(at, _)| at > off);
        assert!(
            after.clone().all(|&(_, armed)| !armed),
            "silent={silent}: keyed again after the drop"
        );
        assert_eq!(
            said.last(),
            Some(&Err(json!("nexusClient:pressEnded"))),
            "silent={silent}: {said:?}"
        );
        let e = t.s.shared.engine.lock().unwrap();
        assert!(!e.mic_armed() && !e.manual_ptt() && !e.tx_enabled());
        drop(e);
        t.end();
    }
}

/// ⛔ A room that is not honest: one byte changed on the way to the shack or from it, a piece
/// delivered twice, or two pieces delivered out of order, mid-press. Each is refused: the TLS at
/// the end that reads it ends the session, so a station command sent after it never runs (and a
/// duplicate never runs twice), the press comes off at once, the screen's re-assertions are
/// refused, and client mode opens a new link. CONTROL: the same press and commands through an
/// honest room: both commands run, the press stays armed, and no new link is opened.
#[test]
fn over_the_internet_a_tampered_duplicated_or_reordered_piece_is_refused() {
    let _alone = alone();
    for act in [
        None,
        Some(Act::Tamper),
        Some(Act::TamperDown),
        Some(Act::Duplicate),
        Some(Act::Reorder),
    ] {
        let t = Afield::new("afar-room", true, quick());
        t.live_over_the_internet();
        let held = Held::on(&t);
        let runs = || {
            t.app
                .ran()
                .iter()
                .filter(|cmd| *cmd == "get_settings")
                .count()
        };
        let (before, opens) = (runs(), t.service.opens.load(Ordering::SeqCst));
        let acted = Instant::now();
        if let Some(act) = act {
            t.service.room.arm(act);
        }
        let asked = [
            ask(&t.screens.main, "get_settings", json!({})),
            ask(&t.screens.main, "get_settings", json!({})),
        ];
        if act.is_some() {
            eventually("the room acted", || {
                t.service.room.acted.load(Ordering::SeqCst)
            });
            eventually("a new link", || {
                t.service.opens.load(Ordering::SeqCst) > opens
            });
        } else {
            std::thread::sleep(Duration::from_millis(1500));
        }
        let (said, ticks) = held.end();
        let ran = runs() - before;
        match act {
            None => {
                assert!(asked.iter().all(Result::is_ok), "{asked:?}");
                assert_eq!(ran, 2);
                assert_eq!(t.service.opens.load(Ordering::SeqCst), opens);
                assert!(said.iter().all(Result::is_ok), "{said:?}");
                assert!(
                    ticks.last().is_some_and(|&(_, armed)| armed),
                    "CONTROL: still armed"
                );
            }
            Some(act) => {
                match act {
                    Act::Tamper | Act::Reorder => assert_eq!(ran, 0, "{act:?}: ran after it"),
                    Act::Duplicate => assert!(ran <= 1, "{act:?}: ran {ran} times"),
                    // Read at the shack before the answer was changed on its way back.
                    Act::TamperDown => assert!(ran <= 2),
                }
                let off = off_after(&ticks, acted).expect("the press never came off the air");
                eprintln!(
                    "measured: {act:?}: off the air {:?} after the act",
                    off - acted
                );
                assert!(
                    off - acted < AT_ONCE + Duration::from_secs(1),
                    "{act:?}: {:?}",
                    off - acted
                );
                assert!(
                    ticks
                        .iter()
                        .filter(|&&(at, _)| at > off)
                        .all(|&(_, armed)| !armed),
                    "{act:?}: keyed again"
                );
                assert_eq!(
                    said.last(),
                    Some(&Err(json!("nexusClient:pressEnded"))),
                    "{act:?}"
                );
            }
        }
        t.end();
    }
}

/// ★ A held PTT is the operator at work: "Still there?" is never asked while the screen holds a
/// press, however long, and nothing else is reported. CONTROL: once it is let go, with nothing
/// else from the operator, the prompt comes.
#[test]
fn a_held_press_keeps_still_there_from_asking() {
    let _alone = alone();
    let timing = Timing {
        idle_prompt: Duration::from_millis(2500),
        idle_end: Duration::from_millis(4000),
        ..quick()
    };
    let t = Afield::new("afar-held-idle", true, timing);
    t.live_over_the_internet();
    let held = Held::on(&t);
    let mut asked = false;
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(100));
        asked |= t.client.status().still_there;
    }
    let (said, _) = held.end();
    assert!(said.iter().all(Result::is_ok), "{said:?}");
    let status = t.client.status();
    assert!(
        !asked && status.phase == Phase::Live,
        "asked while held: {status:?}"
    );
    assert!(ask(&t.screens.main, "set_ptt", json!({"on": false})).is_ok());
    eventually("asked once let go", || t.client.status().still_there);
    t.end();
}
