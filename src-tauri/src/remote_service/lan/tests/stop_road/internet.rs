//! Stop's road over the internet, end to end over loopback: this computer's own command link
//! (`crate::lan_client::link`) asks for internet roads in its `open`; the shack's real channel opens
//! a Stop road of their own for that connection, on its own socket and ICE (`tempo_client::ice`);
//! and this computer reaches it only through a TURN relay (the client core's stand-in, where the
//! service's would stand), its relayed address the only one it offers. What the LAN's road holds,
//! this one holds: a Stop by the road stops the station and is noted where the command lane reads
//! it, so a start made before it never runs; a press the relay delivers after the Stop never keys;
//! an end the relay loses is said again at the next hold; and a datagram from an address ICE did not
//! select is dropped unread, the same bytes taken from the selected pair.
use super::super::super::channel::InternetRoads;
use super::super::computer::paired_record;
use super::relayed::{a_press_keys_on, keyed_once_answered};
use super::stop_count::turned_on;
use super::*;
use crate::lan_client::audio_road::{Headset, StandIn as Earphones};
use crate::lan_client::link::{Answer, Internet, Link};
use crate::lan_client::stop_road::StopRoad as ComputerRoad;
use tempo_client::ice::{Counter, Relay, Route};
use tempo_client::turn::stand_in::StandIn;

/// The relay's throwaway credential.
const USER: &str = "road";
const PASSWORD: &str = "throwaway-password";

/// The kind of a hold, a Stop and the shack's report on a press, as the relay reads them.
fn hold_kind() -> u8 {
    Up::Hold { press: 0, stops: 0 }.kind()
}

fn stop_kind() -> u8 {
    Up::Stop { stop: 0 }.kind()
}

fn press_kind() -> u8 {
    Down::Press {
        press: 0,
        keyed: false,
        end: None,
    }
    .kind()
}

/// What an answer on the link says in a word: the command it ran, or its refusal.
fn word(answer: &Answer) -> String {
    match answer {
        Answer::Value { value, .. } => format!("ran {}", value["ran"].as_str().unwrap_or("?")),
        Answer::Refused(refusal) => format!("refused {}", refusal.code()),
        other => format!("{other:?}"),
    }
}

/// One of the lease's operation requests on the link, asked again while the station is busy.
async fn operate(link: &Link, request: Value) -> Value {
    for _ in 0..100 {
        let mut request = request.clone();
        request["requestId"] = json!(id());
        let answer = link.operation(request).await.expect("the link carries it");
        if !["stationBusy", "remoteBusy", "controllerBusy"]
            .contains(&answer["error"].as_str().unwrap_or_default())
        {
            return answer;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the station stayed busy")
}

/// A shack in Phone, its radio loop running, whose roads are the internet's (each connection's own
/// socket, ICE and thread), and this computer's own link to it, holding the lease, with its internet
/// Stop road reaching the shack only through the TURN stand-in. The loop runs until the flag is set.
struct OverTheInternet {
    r: Roadside,
    turn: StandIn,
    link: Link,
    road: Arc<ComputerRoad>,
    record: Arc<Mutex<Vec<String>>>,
    done: Arc<AtomicBool>,
    radio: Option<std::thread::JoinHandle<Vec<(Instant, bool)>>>,
    _listening: watch::Sender<bool>,
}

/// The shack's `connection` behind a port on loopback, with `shared` as every connection's own.
async fn behind_a_port_with(shared: Shared, stop: watch::Receiver<bool>) -> SocketAddrV4 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let SocketAddr::V4(at) = listener.local_addr().unwrap() else {
        unreachable!()
    };
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
    at
}

impl OverTheInternet {
    async fn new(app: &App) -> Self {
        Self::beside(app, false).await.0
    }

    /// The same, with this computer's audio on `headset`: its internet audio road beside the Stop
    /// road, its own socket and ICE through the same relay.
    async fn with_headset(app: &App, headset: Arc<dyn Headset>) -> Self {
        Self::opened(app, false, Some(headset)).await.0
    }

    /// The same, and with `lan` the shack's own LAN listener as well, its roads the LAN's: another
    /// computer seated there, without the lease. Both listeners open their roads from the same
    /// Stop road, so its presses are counted and named from one count of tags.
    async fn beside(app: &App, lan: bool) -> (Self, Option<Seat>) {
        Self::opened(app, lan, None).await
    }

    async fn opened(
        app: &App,
        lan: bool,
        headset: Option<Arc<dyn Headset>>,
    ) -> (Self, Option<Seat>) {
        let mut r = roadside(app);
        let record = on_the_engine(&mut r, gate(false));
        r.s.shared
            .authority
            .permit_station(&r.s.device, true)
            .unwrap();
        // The internet's listener: no welcome names a road of the LAN's, and a connection that
        // asks in its `open` is given its own.
        let mut internet = r.s.shared.clone();
        let stop = internet.stop_road.take().expect("the roadside's Stop road");
        let audio = internet.audio_road.take();
        internet.internet = Some(InternetRoads {
            stop,
            audio,
            bind: Ipv4Addr::LOCALHOST.into(),
            stun: None,
        });
        let seat = match lan {
            true => Some(seated_with(&r, false).await),
            false => None,
        };
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
        let turn = StandIn::start(USER, PASSWORD);
        let (listening, stop) = watch::channel(false);
        let at = behind_a_port_with(internet, stop).await;
        let internet = Internet {
            bind: Ipv4Addr::LOCALHOST.into(),
            direct: false,
            stun: None,
            relay: Some(Relay {
                server: turn.address,
                username: USER.into(),
                password: PASSWORD.into(),
            }),
        };
        let (link, _) = Link::connect_over(
            &paired_record(&r.s, at),
            &[at],
            41,
            headset,
            Some(&internet),
        )
        .await
        .expect("the link");
        let road = link.stop_road().expect("the internet Stop road");
        let state = operate(&link, json!({"type":"state"})).await;
        let boot = state["value"]["stationBootId"].clone();
        let acquired = operate(&link, json!({"type":"acquire","stationBootId":boot})).await;
        assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
        road.lease(acquired["value"]["leaseId"].as_str().map(str::to_string));
        assert!(link.watch(&["snapshot/1"]));
        let t = Self {
            r,
            turn,
            link,
            road,
            record,
            done,
            radio: Some(radio),
            _listening: listening,
        };
        (t, seat)
    }

    /// The screen fetched the newest snapshot the link holds, and the shack renewed presence from
    /// the road's next heartbeat naming it, through the relay.
    async fn present(&self) {
        let asked = Instant::now();
        for _ in 0..600 {
            if let Some((state, _)) = self.link.latest("snapshot/1") {
                self.road.fetched(state);
            }
            if self.road.heard().presence.is_some_and(|(at, _)| at > asked) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("no renewal came back through the relay");
    }

    /// Whether the held PTT or the microphone over keyed at a look within `within`.
    async fn keyed_within(&self, within: Duration) -> bool {
        let until = Instant::now() + within;
        while Instant::now() < until {
            if self.r.ptt.keyed() || self.r.s.shared.engine.lock().unwrap().mic_armed() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        false
    }

    /// The press `press` re-asserted by the screen every 100 ms, `times` times, on a task.
    fn reassert(&self, press: u64, times: usize) -> tokio::task::JoinHandle<()> {
        let road = self.road.clone();
        tokio::spawn(async move {
            for _ in 0..times {
                road.hold(press);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
    }

    /// Whether the station acknowledged the Stop numbered `stop` on the road within `within`.
    async fn acknowledged(&self, stop: u64, within: Duration) -> bool {
        let until = Instant::now() + within;
        while Instant::now() < until {
            if self.road.heard().stopped.contains(&stop) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        false
    }

    async fn end(mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(radio) = self.radio.take() {
            let _ = tokio::task::spawn_blocking(move || radio.join()).await;
        }
        self.link.close();
    }
}

/// ⛔ Over the internet road, through the relay alone, a Stop by the road stops the station and is
/// noted where the command lane reads it: a start made before the Stop (Enable TX, carrying 0),
/// arriving on the link after it, is refused `stopped` and never reaches the Engine, while the same
/// start made after it (carrying 1) runs. The relay carried the road both ways (its own counts).
#[test]
fn over_the_internet_a_stop_by_the_road_stops_the_station_and_a_start_made_before_it_never_runs() {
    // A Stop on the road disarms the process-wide satellite badge: the road tests' guard.
    let _alone = alone();
    on_a_runtime(
        2,
        over_the_internet_a_stop_by_the_road_stops_the_station_and_a_start_made_before_it_never_runs_on_a_runtime(),
    );
}

async fn over_the_internet_a_stop_by_the_road_stops_the_station_and_a_start_made_before_it_never_runs_on_a_runtime(
) {
    let app = App::new();
    let t = OverTheInternet::new(&app).await;
    t.present().await;
    let crossed = t.turn.seen();
    assert!(
        crossed.sent_by_channel > 0 && crossed.delivered_by_channel > 0,
        "premise: the road crossed the relay both ways by its channel: {crossed:?}"
    );
    assert_eq!(crossed.allocations, 1, "{crossed:?}");
    let (_, armed) = t
        .link
        .call("set_tx_enabled", turned_on("set_tx_enabled"), None, 0)
        .await;
    assert_eq!(word(&armed), "ran set_tx_enabled", "premise: armed");
    t.road.stop(1);
    assert!(
        came_to_a_stop(&t.r.s).await,
        "the Stop by the internet road stopped the station"
    );
    assert!(t.acknowledged(1, Duration::from_secs(2)).await);
    t.present().await;
    let (_, before) = t
        .link
        .call("set_tx_enabled", turned_on("set_tx_enabled"), None, 0)
        .await;
    let (_, after) = t
        .link
        .call("set_tx_enabled", turned_on("set_tx_enabled"), None, 1)
        .await;
    let ran = ran(&t.record);
    t.end().await;
    assert_eq!(
        (word(&before), word(&after)),
        (
            "refused stopped".to_string(),
            "ran set_tx_enabled".to_string()
        ),
        "(the start made before the Stop, the same start made after it)"
    );
    assert_eq!(
        ran,
        ["set_tx_enabled", "set_tx_enabled"],
        "the start made before the Stop never reached the Engine"
    );
}

/// ⛔ Over the internet road, through a relay that delivers a press's first hold only after the Stop
/// pressed after it, the press never keys. Presence is live through the relay; a press is begun
/// when no Stop had been pressed (its hold names 0) and the relay holds that hold back; the
/// operator's Stop (number 1) crosses and stops the station; TX is armed again on the link by a
/// start made after the Stop; then the relay lets the hold through. Neither the held PTT nor the
/// microphone keys. CONTROL: a press begun after the Stop (naming 1), re-asserted through the same
/// relay, keys.
#[test]
fn over_the_internet_a_hold_the_relay_delivers_after_the_stop_never_keys() {
    let _alone = alone();
    on_a_runtime(
        2,
        over_the_internet_a_hold_the_relay_delivers_after_the_stop_never_keys_on_a_runtime(),
    );
}

async fn over_the_internet_a_hold_the_relay_delivers_after_the_stop_never_keys_on_a_runtime() {
    let app = App::new();
    let t = OverTheInternet::new(&app).await;
    t.present().await;
    t.turn.hold(hold_kind());
    t.road.press(7, 0);
    t.road.stop(1);
    assert!(
        came_to_a_stop(&t.r.s).await,
        "premise: the Stop crossed the relay and stopped the station"
    );
    t.present().await;
    let (_, armed) = t
        .link
        .call("set_tx_enabled", turned_on("set_tx_enabled"), None, 1)
        .await;
    assert_eq!(word(&armed), "ran set_tx_enabled", "premise: armed again");
    assert!(!t.turn.held().is_empty(), "premise: the hold was held back");
    t.turn.release();
    let late = keyed_once_answered(&t.road, &t.r, 7).await;
    t.present().await;
    let after = a_press_keys_on(&t.road, 8, 1).await;
    t.end().await;
    assert_eq!(
        (late, after),
        (false, true),
        "(the press begun before the Stop keyed, the press begun after it keyed)"
    );
}

/// ★ Over the internet road, through a relay that loses the station's report of a press's end, the
/// end still reaches this computer, at the next hold. A press begun through the relay keys; its
/// screen then pauses, so the station's held PTT ends it as lapsed after 200 ms, and the relay loses
/// that report. When the screen re-asserts the press, the station answers the hold with the end:
/// this computer's press is over, and no hold of it goes after. CONTROL: the relay lost exactly one
/// report, the press had keyed, and this computer had not heard the end before it held again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_the_internet_a_lost_end_is_said_again_at_the_next_hold() {
    let app = App::new();
    let t = OverTheInternet::new(&app).await;
    t.present().await;
    t.road.press(9, 0);
    let holding = t.reassert(9, 3);
    let keyed = t.keyed_within(Duration::from_millis(500)).await;
    let _ = holding.await;
    t.turn.lose_next(press_kind());
    tokio::time::sleep(Duration::from_millis(400)).await;
    let ended_before = t.road.heard().ended.is_some();
    let held_again = t.road.hold(9);
    let mut ended_after = false;
    for _ in 0..200 {
        if t.road.heard().ended.is_some() {
            ended_after = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let hold_after = t.road.hold(9);
    let lost = t.turn.lost();
    t.end().await;
    assert_eq!(
        (
            keyed,
            lost,
            ended_before,
            held_again,
            ended_after,
            hold_after
        ),
        (true, 1, false, true, true, false),
        "(keyed, reports lost, an end heard before holding again, that hold went, an end heard \
         after, a hold after it went)"
    );
}

/// ⛔ A datagram from an address ICE did not select is dropped unread at the shack's internet road,
/// though it is this computer's own sealed Stop: the relay holds back this computer's Stop, a
/// stranger's socket sends those very bytes straight to the shack's road, and nothing is taken (no
/// acknowledgement, and a start made before that Stop still runs). CONTROL: the same bytes from the
/// selected pair, the relay letting them through, are taken: acknowledged, and the same start is
/// then refused `stopped`.
#[test]
fn over_the_internet_a_stop_from_an_address_ice_did_not_select_is_dropped_unread() {
    let _alone = alone();
    on_a_runtime(
        2,
        over_the_internet_a_stop_from_an_address_ice_did_not_select_is_dropped_unread_on_a_runtime(
        ),
    );
}

async fn over_the_internet_a_stop_from_an_address_ice_did_not_select_is_dropped_unread_on_a_runtime(
) {
    let app = App::new();
    let t = OverTheInternet::new(&app).await;
    t.present().await;
    let shack_road = t
        .turn
        .seen()
        .channels
        .first()
        .expect("a channel to the shack")
        .1;
    t.turn.hold(stop_kind());
    t.road.stop(1);
    let mut held = Vec::new();
    for _ in 0..400 {
        held = t.turn.held();
        if !held.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let sealed = held.first().expect("the Stop, held back").clone();
    let stranger = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    stranger.send_to(&sealed, shack_road).unwrap();
    let taken_from_the_stranger = t.acknowledged(1, NOTHING).await;
    t.present().await;
    let (_, while_held) = t
        .link
        .call("set_tx_enabled", turned_on("set_tx_enabled"), None, 0)
        .await;
    // CONTROL: the same Stop, through the relay, by the selected pair.
    t.turn.release();
    let taken_from_the_pair = t.acknowledged(1, Duration::from_secs(2)).await;
    t.present().await;
    let (_, after) = t
        .link
        .call("set_tx_enabled", turned_on("set_tx_enabled"), None, 0)
        .await;
    t.end().await;
    assert_eq!(
        (
            taken_from_the_stranger,
            word(&while_held),
            taken_from_the_pair,
            word(&after)
        ),
        (
            false,
            "ran set_tx_enabled".to_string(),
            true,
            "refused stopped".to_string()
        ),
        "(the stranger's copy taken, a start made before the Stop while it was held, the pair's \
         copy taken, the same start after it)"
    );
}

/// ★ Each road takes only its own presses' reports, named under its own tag: with another computer
/// seated on the shack's LAN road (whose thread then reads the held PTT's reports too), every press
/// this computer makes over its internet road is reported to it keyed, by that road's own thread.
/// Six presses, each heard keyed here. (Taken by the LAN road's thread, a report would be dropped
/// there, as a press of no road of its own.)
#[test]
fn over_the_internet_each_press_is_reported_to_its_own_road_beside_a_lan_road() {
    let _alone = alone();
    on_a_runtime(
        2,
        over_the_internet_each_press_is_reported_to_its_own_road_beside_a_lan_road_on_a_runtime(),
    );
}

async fn over_the_internet_each_press_is_reported_to_its_own_road_beside_a_lan_road_on_a_runtime() {
    let app = App::new();
    let (t, seat) = OverTheInternet::beside(&app, true).await;
    let _seat = seat.expect("a computer on the LAN road");
    t.present().await;
    let keyed_report =
        |press: u64| {
            t.road.heard().presses.iter().any(
                |down| matches!(down, Down::Press { press: p, keyed: true, .. } if *p == press),
            )
        };
    let mut heard = Vec::new();
    for press in 1..=6u64 {
        t.road.press(press, 0);
        let holding = t.reassert(press, 4);
        let mut keyed = false;
        for _ in 0..200 {
            if keyed_report(press) {
                keyed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let _ = holding.await;
        t.road.release(press);
        heard.push(keyed);
        for _ in 0..200 {
            if !t.r.ptt.keyed() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        t.present().await;
    }
    t.end().await;
    assert_eq!(
        heard, [true; 6],
        "each press reported keyed to its own road"
    );
}

/// ★ A Stop by the internet road after its link has ended is still taken, as on the LAN: both ends
/// keep the road, its ICE and its relayed address, for the afterlife. The link closes; a moment
/// later this computer's Stop goes by the road, through the relay, and is acknowledged. CONTROL: the
/// link had ended.
#[test]
fn over_the_internet_a_stop_after_the_link_ended_is_still_taken() {
    let _alone = alone();
    on_a_runtime(
        2,
        over_the_internet_a_stop_after_the_link_ended_is_still_taken_on_a_runtime(),
    );
}

async fn over_the_internet_a_stop_after_the_link_ended_is_still_taken_on_a_runtime() {
    let app = App::new();
    let t = OverTheInternet::new(&app).await;
    t.present().await;
    t.link.close();
    for _ in 0..200 {
        if t.link.ended() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let ended = t.link.ended();
    tokio::time::sleep(Duration::from_millis(200)).await;
    t.road.stop(1);
    let taken = t.acknowledged(1, Duration::from_secs(2)).await;
    t.end().await;
    assert_eq!(
        (ended, taken),
        (true, true),
        "(the link ended, the Stop taken after it)"
    );
}

/// ★ The station's audio over the internet audio road, through the relay: with this computer's
/// headset, the link's `open` asks for an audio road beside the Stop road, the shack opens one of its
/// own (its own socket and ICE), and once this computer listens under its lease, the station's band
/// is played into its headset. CONTROL: nothing was played before it listened.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_the_internet_the_stations_audio_reaches_this_computers_headset() {
    let app = App::new();
    let earphones = Earphones::new();
    let t = OverTheInternet::with_headset(&app, earphones.clone()).await;
    let audio = t.link.audio_road().expect("the internet audio road");
    let band = t.r.on_the_air();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = earphones.rings.waiting();
    let lease = operate(&t.link, json!({"type":"state"})).await["value"]["leaseId"]
        .as_str()
        .map(str::to_string);
    audio.lease(lease);
    let mut played = 0;
    for _ in 0..400 {
        played = earphones.rings.waiting();
        if played > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    band.store(false, Ordering::SeqCst);
    let relayed = t.turn.seen().allocations;
    t.end().await;
    assert_eq!(before, 0, "played before it listened");
    assert!(played > 0, "the station's band never reached the headset");
    assert_eq!(relayed, 2, "a relayed address for each road");
}

// ----- Where an internet road's checks go -----

/// A listener whose roads are the internet's, bound at `bind`, as the roadside's Stop and audio
/// roads open them.
fn internet_at(r: &mut Roadside, bind: Ipv4Addr) -> Shared {
    r.s.shared
        .authority
        .permit_station(&r.s.device, true)
        .unwrap();
    let mut internet = r.s.shared.clone();
    let stop = internet.stop_road.take().expect("the roadside's Stop road");
    let audio = internet.audio_road.take();
    internet.internet = Some(InternetRoads {
        stop,
        audio,
        bind: bind.into(),
        stun: None,
    });
    internet
}

/// ⛔ Over the internet the shack never probes an address off the internet that a computer names:
/// a candidate on the shack's loopback for each road in the `open`, and one for each road on the
/// network the shack's own roads are on (the address its offer names), trickled after it, are read
/// and dropped, nothing ever reaches any of them, and the connection goes on. CONTROL: a shack
/// whose roads are on loopback, as a test's are, probes each such address it is given.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_the_internet_the_shack_never_probes_an_address_off_the_internet() {
    let app = App::new();
    for bind in [Ipv4Addr::LOCALHOST, Ipv4Addr::UNSPECIFIED] {
        let mut r = roadside(&app);
        let internet = internet_at(&mut r, bind);
        let (station, computer) = pair().await;
        let (_listening, stopped) = watch::channel(false);
        tokio::spawn(channel::connection(
            station,
            PEER.parse().unwrap(),
            internet,
            stopped,
        ));
        let mut socket = open(computer, &r.s.computer.key, &r.s.public_key)
            .await
            .unwrap();
        assert_eq!(hello(&mut socket, VERSIONS).await["type"], "welcome");
        let localhost = Ipv4Addr::LOCALHOST.into();
        let (stop_loopback, audio_loopback) = (Counter::on(localhost), Counter::on(localhost));
        let offer = |line: String| {
            json!({"ufrag":"0011223344556677","pwd":"00112233445566778899aabbccddeeff",
                "candidates":[line]})
        };
        send(
            &mut socket,
            json!({"type":"open","profile":"desktop","app":wire::APP,"contract":wire::contract(),
                "run":1,"roads":{"stop":offer(stop_loopback.line()),"audio":offer(audio_loopback.line())}}),
        )
        .await;
        let opened = next(&mut socket).await;
        assert_eq!(opened["type"], "opened", "{opened}");
        // The network the shack's own roads are on: the address its offer names.
        let own: std::net::IpAddr = opened["roads"]["stop"]["ice"]["candidates"][0]
            .as_str()
            .and_then(|line| line.split(' ').nth(4))
            .and_then(|ip| ip.parse().ok())
            .expect("the shack's host candidate");
        let (stop_own, audio_own) = (Counter::on(own), Counter::on(own));
        for (road, counter) in [("stop", &stop_own), ("audio", &audio_own)] {
            send(
                &mut socket,
                json!({"type":"candidate","road":road,"candidate":counter.line()}),
            )
            .await;
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let heard = [&stop_loopback, &audio_loopback, &stop_own, &audio_own].map(Counter::heard);
        let state = ask(&mut socket, json!({"type":"state"})).await;
        assert!(
            state["value"]["stationBootId"].is_string(),
            "the connection goes on: {state}"
        );
        if bind.is_loopback() {
            assert!(
                heard.iter().all(|&n| n > 0),
                "CONTROL: a shack on loopback probes each: {heard:?}"
            );
        } else {
            assert!(
                matches!(own, std::net::IpAddr::V4(v4) if v4.is_private()),
                "premise: the shack is on a private network ({own})"
            );
            assert_eq!(
                heard, [0; 4],
                "(each road's candidate on loopback, each road's on the shack's own network)"
            );
        }
    }
}

/// ⛔ Over the internet this computer never asks its relay to reach an address off the internet
/// that the shack names: a shack whose roads are on loopback, as a test's are, names their loopback
/// addresses in its `opened`, and this computer's two roads, on every interface as an internet road
/// is and relayed only, ask the TURN server for no permission and send nothing through it, so
/// neither selects a pair, while the link goes on. CONTROL: this computer's roads on loopback,
/// through the same kind of relay, are permitted the shack's address and each selects it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_the_internet_this_computer_never_asks_its_relay_for_an_address_off_the_internet() {
    let app = App::new();
    for bind in [Ipv4Addr::LOCALHOST, Ipv4Addr::UNSPECIFIED] {
        let mut r = roadside(&app);
        let internet = internet_at(&mut r, Ipv4Addr::LOCALHOST);
        let (_listening, stopped) = watch::channel(false);
        let at = behind_a_port_with(internet, stopped).await;
        let turn = StandIn::start(USER, PASSWORD);
        let headset: Arc<dyn Headset> = Earphones::new();
        let ours = Internet {
            bind: bind.into(),
            direct: false,
            stun: None,
            relay: Some(Relay {
                server: turn.address,
                username: USER.into(),
                password: PASSWORD.into(),
            }),
        };
        let (link, _) = Link::connect_over(
            &paired_record(&r.s, at),
            &[at],
            41,
            Some(headset),
            Some(&ours),
        )
        .await
        .expect("the link");
        let until = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < until
            && !link
                .internet_routes()
                .iter()
                .all(|(_, route)| route.is_some())
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let (routes, seen) = (link.internet_routes(), turn.seen());
        let state = operate(&link, json!({"type":"state"})).await;
        link.close();
        assert!(
            state["value"]["stationBootId"].is_string(),
            "the link goes on: {state}"
        );
        if bind.is_loopback() {
            assert!(
                routes.len() == 2
                    && routes
                        .iter()
                        .all(|(_, route)| matches!(route, Some(Route::Relayed { .. }))),
                "CONTROL: {routes:?}"
            );
            assert!(
                seen.permissions.contains(&Ipv4Addr::LOCALHOST.into()),
                "CONTROL: {:?}",
                seen.permissions
            );
        } else {
            assert_eq!(
                (
                    routes,
                    seen.allocations,
                    seen.permissions,
                    seen.sent_by_indication,
                    seen.sent_by_channel
                ),
                (vec![("stop", None), ("audio", None)], 2, Vec::new(), 0, 0),
                "(each road's pair, the relayed addresses, the permissions asked, what went through \
                 the relay)"
            );
        }
    }
}
