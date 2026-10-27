//! Stop's second road through a relay, as the internet's UDP path and a TURN server stand between
//! the two computers. The computer's own road (`crate::lan_client::stop_road`), keyed from its side
//! of the TLS session as client mode keys it, speaks to the shack's real road through a relay on
//! loopback: the shack hears only the relay's address and answers it, and the relay can hold back a
//! datagram of any kind until told, or lose the next one of a kind (a datagram's kind is in its
//! clear header, so the relay reads no more of it than a network could). Presence, a held press, a
//! Stop and an end the station says again all cross it.
use super::stop_count::{counted, said, turned_on};
use super::*;
use crate::lan_client::stop_road::{OnAir, StopRoad as ComputerRoad};

/// Where a datagram's kind is, in its clear header (`datagram::HEADER`): after the version.
const KIND: usize = 1;

/// What the relay does to what crosses it.
#[derive(Default)]
struct Rules {
    /// Datagrams of this kind from the computer are held back until [`Relay::release`].
    holding: Option<u8>,
    held: Vec<Vec<u8>>,
    /// The next datagram of this kind from the shack is lost.
    lose: Option<u8>,
    lost: usize,
}

/// A relay between a computer's road and the shack's: a socket the computer sends to, and one that
/// sends on to the shack, each read by a thread of its own.
struct Relay {
    at: SocketAddr,
    inner: Arc<std::net::UdpSocket>,
    rules: Arc<Mutex<Rules>>,
    stopping: Arc<AtomicBool>,
}

impl Relay {
    /// A relay to the shack's road at `shack`, passing everything on as it comes.
    fn to(shack: SocketAddr) -> Self {
        let outer = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let inner = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        inner.connect(shack).unwrap();
        for socket in [&outer, &inner] {
            socket
                .set_read_timeout(Some(Duration::from_millis(5)))
                .unwrap();
        }
        let at = outer.local_addr().unwrap();
        let (outer, inner) = (Arc::new(outer), Arc::new(inner));
        let rules = Arc::new(Mutex::new(Rules::default()));
        let stopping = Arc::new(AtomicBool::new(false));
        let computer = Arc::new(Mutex::new(None::<SocketAddr>));
        {
            let (outer, inner, rules, stopping, computer) = (
                outer.clone(),
                inner.clone(),
                rules.clone(),
                stopping.clone(),
                computer.clone(),
            );
            std::thread::spawn(move || {
                let mut buffer = [0u8; 2048];
                while !stopping.load(Ordering::SeqCst) {
                    let Ok((length, from)) = outer.recv_from(&mut buffer) else {
                        continue;
                    };
                    *computer.lock().unwrap() = Some(from);
                    let datagram = buffer[..length].to_vec();
                    let mut rules = rules.lock().unwrap();
                    if rules.holding.is_some() && rules.holding == datagram.get(KIND).copied() {
                        rules.held.push(datagram);
                        continue;
                    }
                    drop(rules);
                    let _ = inner.send(&datagram);
                }
            });
        }
        {
            let (outer, inner, rules, stopping) =
                (outer, inner.clone(), rules.clone(), stopping.clone());
            std::thread::spawn(move || {
                let mut buffer = [0u8; 2048];
                while !stopping.load(Ordering::SeqCst) {
                    let Ok(length) = inner.recv(&mut buffer) else {
                        continue;
                    };
                    {
                        let mut rules = rules.lock().unwrap();
                        if rules.lose.is_some() && rules.lose == buffer.get(KIND).copied() {
                            rules.lose = None;
                            rules.lost += 1;
                            continue;
                        }
                    }
                    let to = *computer.lock().unwrap();
                    if let Some(to) = to {
                        let _ = outer.send_to(&buffer[..length], to);
                    }
                }
            });
        }
        Self {
            at,
            inner,
            rules,
            stopping,
        }
    }

    /// Hold back every datagram of `kind` from the computer, until [`Relay::release`].
    fn hold(&self, kind: u8) {
        self.rules.lock().unwrap().holding = Some(kind);
    }

    /// Let what was held back through, in the order it came, and hold nothing back from now.
    fn release(&self) {
        let held = {
            let mut rules = self.rules.lock().unwrap();
            rules.holding = None;
            std::mem::take(&mut rules.held)
        };
        for datagram in held {
            let _ = self.inner.send(&datagram);
        }
    }

    /// Lose the next datagram of `kind` from the shack.
    fn lose_next(&self, kind: u8) {
        self.rules.lock().unwrap().lose = Some(kind);
    }

    /// How many datagrams from the shack were lost.
    fn lost(&self) -> usize {
        self.rules.lock().unwrap().lost
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
    }
}

/// The kind of a hold, and of the shack's report on a press, as the relay reads them.
fn hold_kind() -> u8 {
    Up::Hold { press: 0, stops: 0 }.kind()
}

fn press_kind() -> u8 {
    Down::Press {
        press: 0,
        keyed: false,
        end: None,
    }
    .kind()
}

/// Whether the press `press`, whose hold the relay let through only now, keyed: waited for until
/// the station has answered it on the road, or ten seconds pass, not for a window of time, then
/// read from that answer, the held PTT and the microphone over. A hold the station refuses is
/// answered at once; one it keyed is reported as keyed at the road's next look.
pub(super) async fn keyed_once_answered(road: &ComputerRoad, r: &Roadside, press: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut answer = None;
    while answer.is_none() && Instant::now() < deadline {
        answer = road
            .heard()
            .presses
            .into_iter()
            .find(|down| matches!(down, Down::Press { press: p, .. } if *p == press));
        if answer.is_none() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    matches!(answer, Some(Down::Press { keyed: true, .. }))
        || r.ptt.keyed()
        || r.s.shared.engine.lock().unwrap().mic_armed()
}

/// Whether a press begun when this computer had pressed `stops` Stops keys: begun, and re-asserted
/// every 100 ms as the screen re-asserts it, until the station says on the road that it keyed, or
/// how it ended, then let go. A press the station let lapse before its radio loop keyed it (no hold
/// reached it for the gap, as when this process stalls) ends `Lapsed`, as it must, and a new press
/// from `first` on is tried, up to five; any other end, or none within ten seconds, is the
/// answer: not keyed.
pub(super) async fn a_press_keys_on(road: &ComputerRoad, first: u64, stops: u64) -> bool {
    for press in first..first + 5 {
        road.press(press, stops);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut ended = None;
        while ended.is_none() && Instant::now() < deadline {
            for down in road.heard().presses {
                match down {
                    Down::Press {
                        press: p,
                        keyed: true,
                        end: None,
                    } if p == press => {
                        road.release(press);
                        return true;
                    }
                    Down::Press { press: p, end, .. } if p == press && end.is_some() => {
                        ended = end;
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            road.hold(press);
        }
        road.release(press);
        if ended != Some(HoldEnd::Lapsed) {
            return false;
        }
    }
    false
}

/// A shack in Phone, its radio loop running, with a computer seated under the lease, and that
/// computer's own road to the shack's through a relay. The loop runs until the flag is set.
struct ThroughARelay {
    r: Roadside,
    seat: Seat,
    relay: Relay,
    road: Arc<ComputerRoad>,
    on_air: Arc<OnAir>,
    done: Arc<AtomicBool>,
    radio: Option<std::thread::JoinHandle<Vec<(Instant, bool)>>>,
}

impl ThroughARelay {
    async fn new(app: &App) -> Self {
        let mut r = roadside(app);
        // The app's handlers as the start tests stand them in, so Enable TX acts on the engine.
        let _record = on_the_engine(&mut r, gate(false));
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
        let port = seat.offer["port"].as_u64().unwrap() as u16;
        let relay = Relay::to(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
        let on_air = Arc::new(OnAir::default());
        let road = ComputerRoad::open(
            relay.at,
            tag_of(&seat.offer),
            &exported(&seat.socket, &seat.session),
            on_air.clone(),
        )
        .expect("the computer's road");
        road.lease(Some(seat.lease()));
        Self {
            r,
            seat,
            relay,
            road,
            on_air,
            done,
            radio: Some(radio),
        }
    }

    /// The screen fetched a snapshot just pushed, and the shack renewed presence from the road's
    /// next heartbeat naming it, through the relay: presence live, and the state fresh.
    async fn present(&mut self) {
        let (state, _) = self.seat.fresh_snapshot().await;
        self.road.fetched(state);
        let asked = Instant::now();
        for _ in 0..400 {
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

    async fn end(mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(radio) = self.radio.take() {
            let _ = tokio::task::spawn_blocking(move || radio.join()).await;
        }
    }
}

/// ⛔ Through a relay that delivers a press's first hold only after the Stop pressed after it, as the
/// internet can reorder two datagrams, the press never keys. Presence is live through the relay; a
/// press is begun when no Stop had been pressed (its hold names 0) and the relay holds that hold
/// back; the operator's Stop (number 1) crosses and stops the station; TX is armed again on the link
/// by a start made after the Stop; then the relay lets the hold through. Neither the held PTT nor
/// the microphone keys. CONTROL: a press begun after the Stop (naming 1), re-asserted through the
/// same relay, keys.
#[test]
fn through_a_relay_a_hold_delivered_after_the_stop_never_keys() {
    // A Stop on the road disarms the process-wide satellite badge: the road tests' guard.
    let _alone = alone();
    on_a_runtime(
        2,
        through_a_relay_a_hold_delivered_after_the_stop_never_keys_on_a_runtime(),
    );
}

async fn through_a_relay_a_hold_delivered_after_the_stop_never_keys_on_a_runtime() {
    let app = App::new();
    let mut t = ThroughARelay::new(&app).await;
    t.present().await;
    t.relay.hold(hold_kind());
    t.road.press(7, 0);
    t.road.stop(1);
    assert!(
        came_to_a_stop(&t.r.s).await,
        "premise: the Stop crossed the relay and stopped the station"
    );
    t.present().await;
    let armed = counted(
        &mut t.seat,
        1,
        "set_tx_enabled",
        turned_on("set_tx_enabled"),
        1,
    )
    .await;
    assert_eq!(said(&armed), "ran set_tx_enabled", "premise: armed again");
    t.relay.release();
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

/// ★ Through a relay that loses the station's report of a press's end, the end still reaches the
/// computer, at the next hold. A press begun through the relay keys; its screen then pauses, so the
/// station's held PTT ends it as lapsed after 200 ms, and the relay loses that report. When the
/// screen re-asserts the press, the station answers the hold with the end: the computer's press is
/// over, taken as one end, and no hold of it goes after. CONTROL: the relay lost exactly one report,
/// the press had keyed, and the computer had not heard the end before it held again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn through_a_relay_a_lost_end_is_said_again_at_the_next_hold() {
    let app = App::new();
    let mut t = ThroughARelay::new(&app).await;
    t.present().await;
    t.road.press(9, 0);
    let holding = t.reassert(9, 3);
    let keyed = t.keyed_within(Duration::from_millis(500)).await;
    let _ = holding.await;
    t.relay.lose_next(press_kind());
    tokio::time::sleep(Duration::from_millis(400)).await;
    let ended_before = t.on_air.ended();
    let held_again = t.road.hold(9);
    for _ in 0..200 {
        if t.on_air.ended() > ended_before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let ended_after = t.on_air.ended();
    let hold_after = t.road.hold(9);
    let lost = t.relay.lost();
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
        (true, 1, 0, true, 1, false),
        "(keyed, reports lost, ends heard before holding again, that hold went, ends heard after, \
         a hold after it went)"
    );
}
