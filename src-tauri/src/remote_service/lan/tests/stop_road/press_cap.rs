//! A press from afar ends at its cap, on the station's own clock from the hold that began it, and
//! nothing a computer goes on doing carries that one press past it: presence renewed on its road and
//! its lease on the link all through the press, a reconnect in the middle of it, the press held
//! again faster, or released and held again. A NEW press is a new over: after the cap it keys afresh
//! with a cap of its own, and pressed over an over still on the air (the answer to "Still talking?")
//! it carries that same over on, never dropping and arming it again. Each on the roadside the road
//! tests use, the cap shortened through the road's own seam, the shack's radio loop ticking.
use super::*;

/// The cap for these tests, short enough to wait for: Phone's Lock cap, three minutes, in a build.
const CAP: Duration = Duration::from_millis(1500);
/// How late past the cap the over may come off the air: the hold that finds the cap passed comes
/// up to 100 ms after it, then the loop's tick, and a margin for a loaded machine.
const PAST_CAP: Duration = Duration::from_millis(220);

/// What the road and the link said while a press was held, each as it was read here.
#[derive(Default)]
struct Heard {
    /// Each message on the road, and when it was read.
    road: Vec<(Instant, Down)>,
    /// The lease's answer to each heartbeat sent on the link.
    leases: Vec<Value>,
}

impl Heard {
    /// When each presence renewal was answered on the road.
    fn renewals(&self) -> Vec<Instant> {
        self.road
            .iter()
            .filter(|(_, down)| matches!(down, Down::Presence { .. }))
            .map(|&(at, _)| at)
            .collect()
    }

    /// What the road said of press `press`, in order, from `since`.
    fn of_press(&self, press: u64, since: Instant) -> Vec<&Down> {
        self.road
            .iter()
            .filter(|(at, down)| {
                *at >= since && matches!(down, Down::Press { press: p, .. } if *p == press)
            })
            .map(|(_, down)| down)
            .collect()
    }
}

/// This computer's press `press`, spoken into for `frames` frames of 20 ms from frame `first` on the
/// media clock as [`speak`] does, and held on the road every `every` frames; and, all through it,
/// its presence renewed on the road every 200 ms (a heartbeat naming the newest snapshot its screen
/// holds) and its lease on the link every second: what a computer keeping a long press on the air
/// sends. When each hold went, and everything the road and the link said meanwhile.
async fn held(
    seat: &mut Seat,
    encoder: &mut MicEncoder,
    press: u64,
    first: u32,
    frames: u32,
    every: u32,
) -> (Vec<Instant>, Heard) {
    let lease = seat.lease();
    let mut holds = Vec::new();
    let mut heard = Heard::default();
    let start = tokio::time::Instant::now();
    for frame in first..first + frames {
        let n = frame - first;
        if n.is_multiple_of(every) {
            holds.push(Instant::now());
            seat.road.say(&Up::Hold { press, stops: 0 }).await;
        }
        if n.is_multiple_of(10) {
            if let Some((state, _)) = seat.snapshot {
                seat.heartbeat(state).await;
            }
        }
        if n % 50 == 25 {
            let request = json!({"type":"heartbeat","leaseId":lease,"requestId":id()});
            send(
                &mut seat.socket,
                json!({"type":"operationRequest","request":request}),
            )
            .await;
        }
        let packet = voice(encoder, frame);
        seat.audio
            .say(&audio_road::Up::Mic {
                press,
                frame,
                packet,
            })
            .await;
        // What the road has said so far, then the link until this frame's moment is over: its
        // snapshots, so the next heartbeat names the newest, and the lease's answers.
        while let Some(down) = seat.road.heard(Duration::ZERO).await {
            heard.road.push((Instant::now(), down));
        }
        let deadline = start + Duration::from_millis(20) * (n + 1);
        while let Ok(Some(Ok(message))) =
            tokio::time::timeout_at(deadline, seat.socket.next()).await
        {
            match message {
                Message::Binary(frame) => seat.feed(&frame),
                Message::Text(text) => {
                    let said: Value = serde_json::from_str(&text).unwrap();
                    if said["type"] == "operationResponse" {
                        heard.leases.push(said);
                    }
                }
                _ => {}
            }
        }
    }
    (holds, heard)
}

/// The shack's radio loop on a thread of its own until `done`, as [`ticking`]: each tick, when,
/// whether the microphone over was armed or keyed, and the microphone feed's epoch, which moves
/// at every drop of an over, so one over is one epoch from its arm to its end.
fn ticking_epochs(
    r: &Roadside,
    done: Arc<AtomicBool>,
) -> std::thread::JoinHandle<Vec<(Instant, bool, u64)>> {
    let (engine, mic) = (r.s.shared.engine.clone(), r.mic.clone());
    std::thread::spawn(move || {
        let mut ticks = Vec::new();
        while !done.load(Ordering::SeqCst) {
            let now = Instant::now();
            let mut e = engine.lock().unwrap();
            e.poll_remote_transmit(now);
            e.poll_mic(now, TICK.as_millis() as f64, 1);
            let on = e.mic_armed() || e.mic_keyed();
            drop(e);
            ticks.push((now, on, mic.epoch()));
            std::thread::sleep(TICK);
        }
        ticks
    })
}

/// One stretch on the air: from its first tick on to its first tick off after it (`None` while
/// still on at the last tick), and the feed's epochs along it.
#[derive(Debug)]
struct Run {
    on: Instant,
    off: Option<Instant>,
    epochs: Vec<u64>,
}

/// Every stretch on the air among `ticks`, in order.
fn runs(ticks: &[(Instant, bool, u64)]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut was = false;
    for &(at, on, epoch) in ticks {
        match (was, on) {
            (false, true) => runs.push(Run {
                on: at,
                off: None,
                epochs: vec![epoch],
            }),
            (true, true) => {
                let run = runs.last_mut().unwrap();
                if run.epochs.last() != Some(&epoch) {
                    run.epochs.push(epoch);
                }
            }
            (true, false) => runs.last_mut().unwrap().off = Some(at),
            (false, false) => {}
        }
        was = on;
    }
    runs
}

fn phone(r: &Roadside) {
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
}

/// The press `press` let go at its cap, as the road answers a hold for it.
fn let_go(press: u64) -> Down {
    Down::Press {
        press,
        keyed: false,
        end: Some(HoldEnd::Released),
    }
}

/// `run` keyed within half a second of `first`, the first hold of its press, and came off the air
/// at its cap from that hold: no sooner, and within [`PAST_CAP`] after it.
fn ends_at_its_cap(run: &Run, first: Instant, what: &str) {
    assert!(
        run.on >= first && run.on < first + Duration::from_millis(500),
        "{what}: keyed {:?} after its first hold",
        run.on.saturating_duration_since(first)
    );
    let off = run
        .off
        .unwrap_or_else(|| panic!("{what}: still on the air past its cap"));
    eprintln!(
        "measured: {what} came off the air {:?} after its first hold; the cap is {CAP:?}",
        off.saturating_duration_since(first)
    );
    assert!(off >= first + CAP, "{what}: ended before its cap");
    assert!(
        off < first + CAP + PAST_CAP,
        "{what}: ended {:?} after its first hold",
        off.saturating_duration_since(first)
    );
}

/// ⛔ A press's cap counts from its first hold, however often its computer's presence and lease are
/// renewed under it. Held and spoken into for two and a half seconds, presence renewed on the road
/// every 200 ms and the lease on the link every second, the press comes off the air at its cap from
/// its first hold, is said let go, and nothing keys again while the holds and the renewals go on.
/// CONTROL: renewals were answered before the cap and after it, every lease heartbeat found this
/// computer still controlling, and presence is live at the end: it was the cap that ended the over,
/// not a lapse.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_press_cap_counts_from_its_first_hold_however_often_presence_is_renewed() {
    let app = App::new();
    let r = roadside_capped(&app, CAP);
    let mut seat = seated(&r).await;
    phone(&r);
    seat.renewed().await;
    let done = Arc::new(AtomicBool::new(false));
    let radio = ticking_epochs(&r, done.clone());
    let mut encoder = MicEncoder::new().unwrap();
    let (holds, heard) = held(&mut seat, &mut encoder, 9, 1, 125, 5).await;
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let first = holds[0];
    let runs = runs(&ticks);
    assert_eq!(runs.len(), 1, "on the air once: {runs:?}");
    ends_at_its_cap(&runs[0], first, "the press");
    let off = runs[0].off.unwrap();
    let renewals = heard.renewals();
    assert!(
        renewals
            .iter()
            .filter(|&&at| at < first + CAP - Duration::from_millis(200))
            .count()
            >= 2,
        "CONTROL: presence renewed during the press, {} renewals: {renewals:?}",
        renewals.len()
    );
    assert!(
        renewals.iter().any(|&at| at > off),
        "CONTROL: presence renewed after the cap"
    );
    assert!(
        heard.leases.len() >= 2
            && heard
                .leases
                .iter()
                .all(|said| said["value"]["phase"] == "controlling"),
        "CONTROL: the lease renewed on the link: {:?}",
        heard.leases
    );
    assert!(
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .remote_presence_live(Instant::now()),
        "CONTROL: presence live at the end"
    );
    let said = heard.of_press(9, off);
    assert!(
        said.len() >= 5 && said.iter().all(|&down| *down == let_go(9)),
        "the holds after the cap answered with its end: {said:?}"
    );
    assert!(!r.ptt.keyed());
}

/// ⛔ A capped press never comes back, however its computer re-asserts it. Past its cap, the press
/// is held again five times as often, then released and held again under the same number, its
/// voice going on all the while and its presence renewed: nothing keys, no frame of it is taken,
/// and every hold is answered with its end. CONTROL: the press was on the air until its cap, and
/// presence is live at the end.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_capped_press_held_again_or_released_and_held_again_never_keys_again() {
    let app = App::new();
    let r = roadside_capped(&app, CAP);
    let mut seat = seated(&r).await;
    phone(&r);
    seat.renewed().await;
    let done = Arc::new(AtomicBool::new(false));
    let radio = ticking_epochs(&r, done.clone());
    let mut encoder = MicEncoder::new().unwrap();
    // Held to past the cap and its margin.
    let (holds, _) = held(&mut seat, &mut encoder, 9, 1, 100, 5).await;
    let capped = Instant::now();
    let taken = r.counts().mic_taken.load(Ordering::SeqCst);
    // Held at every frame.
    let (_, faster) = held(&mut seat, &mut encoder, 9, 101, 30, 1).await;
    // Released, and held again under the same number.
    seat.road.say(&Up::Release { press: 9 }).await;
    let released = Instant::now();
    let (again, after) = held(&mut seat, &mut encoder, 9, 131, 40, 5).await;
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let runs = runs(&ticks);
    assert_eq!(runs.len(), 1, "on the air again after its cap: {runs:?}");
    ends_at_its_cap(&runs[0], holds[0], "the press");
    assert!(runs[0].off.unwrap() < capped, "premise: capped before");
    assert_eq!(
        r.counts().mic_taken.load(Ordering::SeqCst),
        taken,
        "a frame of the capped press was taken"
    );
    let faster = faster.of_press(9, capped);
    assert!(
        faster.len() >= 25 && faster.iter().all(|&down| *down == let_go(9)),
        "each hold after the cap answered with its end: {faster:?}"
    );
    let after: Vec<_> = after
        .road
        .iter()
        .filter(|(at, down)| *at >= released && matches!(down, Down::Press { .. }))
        .map(|(_, down)| down)
        .collect();
    assert!(
        after.len() + 1 >= again.len() && after.iter().all(|&down| *down == let_go(9)),
        "each hold after its release answered with its end, {} holds: {after:?}",
        again.len()
    );
    assert!(!r.ptt.keyed());
    assert!(
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .remote_presence_live(Instant::now()),
        "CONTROL: presence live at the end"
    );
}

/// ⛔ A reconnect cannot carry a press past its cap. A computer keys its press; its link drops a
/// little way in, and its old road goes on holding the press through the road's afterlife: the over
/// comes off the air at the drop, well before the cap, and nothing keys while those holds go on.
/// Back on a new connection, TX armed again from afar, the same number held on the new road is a
/// new press: a new over, ended at the cap from its own first hold, no later. CONTROL: the first
/// over was on the air before the drop, and the drop stopped the station.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reconnect_mid_press_ends_its_over_and_the_next_one_has_its_own_cap() {
    let app = App::new();
    let mut r = roadside_capped(&app, CAP);
    let _record = on_the_engine(&mut r, gate(false));
    phone(&r);
    let mut first = seated(&r).await;
    first.renewed().await;
    let said = first
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
    let done = Arc::new(AtomicBool::new(false));
    let radio = ticking_epochs(&r, done.clone());
    let mut encoder = MicEncoder::new().unwrap();
    let (before, _) = held(&mut first, &mut encoder, 9, 1, 30, 5).await;
    assert!(
        r.s.shared.engine.lock().unwrap().mic_keyed(),
        "premise: on the air"
    );
    // The link drops; the computer's old road goes on holding its press.
    let Seat {
        road: old,
        socket,
        audio,
        _stop: stop,
        ..
    } = first;
    drop((socket, audio, stop));
    let dropped = Instant::now();
    for _ in 0..6 {
        old.say(&Up::Hold { press: 9, stops: 0 }).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(came_to_a_stop(&r.s).await, "the drop stopped the station");
    let mut again = seated(&r).await;
    again.renewed().await;
    let said = again
        .call(2, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
    let (after, heard) = held(&mut again, &mut encoder, 9, 31, 125, 5).await;
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let runs = runs(&ticks);
    assert_eq!(runs.len(), 2, "on the air twice: {runs:?}");
    let off = runs[0].off.expect("the first over came off the air");
    eprintln!(
        "measured: the first over came off the air {:?} after the drop, {:?} after its first hold",
        off.saturating_duration_since(dropped),
        off.saturating_duration_since(before[0])
    );
    assert!(
        off >= dropped && off < before[0] + CAP,
        "the first over ended {:?} after its first hold, the link dropped {:?} after it",
        off.saturating_duration_since(before[0]),
        dropped.saturating_duration_since(before[0])
    );
    assert!(
        runs[1].on >= after[0],
        "on the air before the new connection's press"
    );
    ends_at_its_cap(&runs[1], after[0], "the press on the new road");
    assert_ne!(
        runs[0].epochs, runs[1].epochs,
        "the press on the new road carried the first over on"
    );
    let off = runs[1].off.unwrap();
    let said = heard.of_press(9, off);
    assert!(
        !said.is_empty() && said.iter().all(|&down| *down == let_go(9)),
        "the new road's holds after its cap answered with its end: {said:?}"
    );
}

/// ★ A new press is a new over. After a press's cap the next press keys afresh, a drop of the
/// microphone over between them, and runs to its own cap from its own first hold. Pressed while an
/// over is still on the air, as an answer to "Still talking?" presses, a new press carries that
/// same over on, its frames numbered on from the old press's: it is never dropped and armed again
/// (the feed's epoch does not move), stays on the air through the old press's cap, and ends at the
/// new press's own cap. CONTROL: each press keyed within half a second of its first hold.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn after_its_cap_a_new_press_is_a_new_over_and_over_a_live_one_carries_it_on() {
    let app = App::new();
    let r = roadside_capped(&app, CAP);
    let mut seat = seated(&r).await;
    phone(&r);
    seat.renewed().await;
    let done = Arc::new(AtomicBool::new(false));
    let radio = ticking_epochs(&r, done.clone());
    let mut encoder = MicEncoder::new().unwrap();
    let (capped, _) = held(&mut seat, &mut encoder, 9, 1, 100, 5).await;
    let (next, _) = held(&mut seat, &mut encoder, 10, 101, 100, 5).await;
    let (old, _) = held(&mut seat, &mut encoder, 11, 201, 35, 5).await;
    let (new, _) = held(&mut seat, &mut encoder, 12, 236, 100, 5).await;
    done.store(true, Ordering::SeqCst);
    let ticks = tokio::task::spawn_blocking(move || radio.join().unwrap())
        .await
        .unwrap();
    let runs = runs(&ticks);
    assert_eq!(runs.len(), 3, "three overs: {runs:?}");
    ends_at_its_cap(&runs[0], capped[0], "the first press");
    ends_at_its_cap(&runs[1], next[0], "the press after its cap");
    assert!(
        runs[0].epochs.len() == 1 && runs[1].epochs.len() == 1,
        "an over dropped under its press: {runs:?}"
    );
    assert_ne!(
        runs[0].epochs, runs[1].epochs,
        "the press after the cap was not a new over"
    );
    // The press over a live one: one over, from the old press's first hold to the new press's cap.
    let carried = &runs[2];
    assert!(
        carried.on >= old[0] && carried.on < old[0] + Duration::from_millis(500),
        "the old press keyed {:?} after its first hold",
        carried.on.saturating_duration_since(old[0])
    );
    assert_eq!(
        carried.epochs.len(),
        1,
        "the over was dropped and armed again under the new press: {carried:?}"
    );
    let off = carried
        .off
        .expect("still on the air past the new press's cap");
    eprintln!(
        "measured: the carried over came off the air {:?} after the new press's first hold, {:?} \
         after the old press's",
        off.saturating_duration_since(new[0]),
        off.saturating_duration_since(old[0])
    );
    assert!(
        off > old[0] + CAP + Duration::from_millis(500),
        "the over ended at the old press's cap"
    );
    assert!(off >= new[0] + CAP, "ended before the new press's cap");
    assert!(
        off < new[0] + CAP + PAST_CAP,
        "ended {:?} after the new press's first hold",
        off.saturating_duration_since(new[0])
    );
}
