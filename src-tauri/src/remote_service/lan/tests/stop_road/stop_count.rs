//! Stop wins over a start that a later Stop overtook on the way, on the roadside the road tests use:
//! the shack's real channel, command lane, Stop road and engine, over loopback. Every call carries
//! how many Stops its computer had pressed when it was made (a Stop its own number, which its road
//! carries too), and every hold the count its press began under. A Stop pressed after a start can
//! reach the shack first: by the road, the shorter way on the internet, or ahead of the start on the
//! link, whose outbox writes a Stop before anything waiting. The start, or the press, must then not
//! run. Each computer here speaks the wire by hand, so each count is the test's own, by value.
use super::*;

/// The arguments the screen invokes `cmd` with, to turn it on.
pub(super) fn turned_on(cmd: &str) -> Value {
    match cmd {
        "set_tx_enabled" => json!({"enabled": true}),
        "play_voice_message" => json!({"slot": 1}),
        _ => json!({}),
    }
}

/// One call on the link carrying `stops`, the count of Stops its computer had pressed when it made
/// it, and the shack's answer to it ([`answer_to`]).
pub(super) async fn counted(seat: &mut Seat, id: u64, cmd: &str, args: Value, stops: u64) -> Value {
    let call = json!({"type":"call","id":id,"cmd":cmd,"args":args,"stops":stops});
    answer_to(seat, id, call).await
}

/// `call` sent on the link, and the shack's answer to the call numbered `id`, whatever comes before;
/// `{"unanswered": true}` when none comes within three seconds.
async fn answer_to(seat: &mut Seat, id: u64, call: Value) -> Value {
    send(&mut seat.socket, call).await;
    let answer = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let said = seat.text().await;
            if said["type"] == "result" && said["id"] == id {
                return said;
            }
        }
    })
    .await;
    answer.unwrap_or_else(|_| json!({"unanswered": true}))
}

/// What an answer says in a word: the command it ran, or its refusal.
pub(super) fn said(answer: &Value) -> String {
    match (answer["value"]["ran"].as_str(), answer["refused"].as_str()) {
        (Some(ran), _) => format!("ran {ran}"),
        (_, Some(refused)) => format!("refused {refused}"),
        _ => answer.to_string(),
    }
}

/// ⛔ A start made before a Stop that the road delivered first never runs. The computer's first Stop
/// (number 1) goes by the road and is taken, stopping what was on the air; the operator arms TX
/// again (a start made after that Stop, so it carries 1); then a start made before the Stop, when
/// no Stop had been pressed (it carries 0), arrives on the link. It is refused `stopped` and never
/// reaches the Engine, although TX is armed and presence live. CONTROL: the same start made after
/// the Stop (carrying 1) runs, for each wired kind (a CQ, Enable TX, the voice keyer).
#[test]
fn a_start_made_before_a_stop_the_road_delivered_first_never_runs() {
    let _alone = alone();
    on_a_runtime(
        2,
        a_start_made_before_a_stop_the_road_delivered_first_never_runs_on_a_runtime(),
    );
}

async fn a_start_made_before_a_stop_the_road_delivered_first_never_runs_on_a_runtime() {
    let starts = ["call_cq", "set_tx_enabled", "play_voice_message"];
    let mut cases = Vec::new();
    for start in starts {
        let app = App::new();
        let mut r = roadside(&app);
        let record = on_the_engine(&mut r, gate(false));
        let mut seat = seated(&r).await;
        seat.renewed().await;
        // On the air, so the Stop's verbs show when they have run: the road acknowledges the Stop
        // at once and runs them once the Engine is free, and their halt voids a start read before
        // then, even one made after the Stop.
        key_the_mic(&r.s);
        assert!(
            seat.road
                .road_stop(1, Duration::from_secs(2))
                .await
                .is_some(),
            "{start}: premise: the Stop taken"
        );
        assert!(came_to_a_stop(&r.s).await, "{start}: premise: stopped");
        let armed = counted(
            &mut seat,
            1,
            "set_tx_enabled",
            turned_on("set_tx_enabled"),
            1,
        )
        .await;
        assert_eq!(
            said(&armed),
            "ran set_tx_enabled",
            "{start}: premise: armed again"
        );
        let before = counted(&mut seat, 2, start, turned_on(start), 0).await;
        let after = counted(&mut seat, 3, start, turned_on(start), 1).await;
        cases.push((start, said(&before), said(&after), ran(&record)));
    }
    let want: Vec<_> = starts
        .iter()
        .map(|&start| {
            (
                start,
                "refused stopped".to_string(),
                format!("ran {start}"),
                vec!["set_tx_enabled".to_string(), start.to_string()],
            )
        })
        .collect();
    assert_eq!(
        cases, want,
        "(the start, made before the Stop, made after it, what reached the Engine)"
    );
}

/// ⛔ A start that a later Stop overtook on the link never runs. The computer arms TX. Its first Stop
/// is pressed while a start made before it still waits in the link's outbox, and the outbox writes
/// the Stop first, carrying 1. The shack reads the Stop; TX is armed again by a start made after it;
/// then comes the start that waited, carrying 0: refused `stopped`. For a Stop TX, the off half of
/// Enable TX, and the voice keyer's Stop. CONTROL: a CQ made after the Stop (carrying 1) runs.
#[test]
fn a_start_a_later_stop_overtook_on_the_link_never_runs() {
    let _alone = alone();
    on_a_runtime(
        2,
        a_start_a_later_stop_overtook_on_the_link_never_runs_on_a_runtime(),
    );
}

async fn a_start_a_later_stop_overtook_on_the_link_never_runs_on_a_runtime() {
    let stops = [
        ("halt_tx", json!({})),
        ("set_tx_enabled", json!({"enabled": false})),
        ("stop_voice", json!({})),
    ];
    let mut cases = Vec::new();
    for (stop, args) in stops.clone() {
        let app = App::new();
        let mut r = roadside(&app);
        let record = on_the_engine(&mut r, gate(false));
        let mut seat = seated(&r).await;
        seat.renewed().await;
        let armed = counted(
            &mut seat,
            1,
            "set_tx_enabled",
            turned_on("set_tx_enabled"),
            0,
        )
        .await;
        assert_eq!(said(&armed), "ran set_tx_enabled", "{stop}: premise: armed");
        let stopped = counted(&mut seat, 2, stop, args, 1).await;
        assert_eq!(said(&stopped), format!("ran {stop}"), "{stop}: premise");
        let again = counted(
            &mut seat,
            3,
            "set_tx_enabled",
            turned_on("set_tx_enabled"),
            1,
        )
        .await;
        assert_eq!(
            said(&again),
            "ran set_tx_enabled",
            "{stop}: premise: armed again"
        );
        let before = counted(&mut seat, 4, "call_cq", json!({}), 0).await;
        let after = counted(&mut seat, 5, "call_cq", json!({}), 1).await;
        cases.push((stop, said(&before), said(&after), ran(&record)));
    }
    let want: Vec<_> = stops
        .iter()
        .map(|(stop, _)| {
            (
                *stop,
                "refused stopped".to_string(),
                "ran call_cq".to_string(),
                ["set_tx_enabled", stop, "set_tx_enabled", "call_cq"]
                    .map(str::to_string)
                    .to_vec(),
            )
        })
        .collect();
    assert_eq!(
        cases, want,
        "(the Stop, the CQ made before it, the CQ made after it, what reached the Engine)"
    );
}

/// ⛔ A press made before a Stop the shack has taken never keys. The shack is in Phone with its radio
/// loop running and presence live; the computer's Stop (number 1) is taken, by the road or by the
/// link, and TX is armed again by a start made after it. A hold the network delivers only now, for
/// a press begun before that Stop (it names 0), is refused at each hold and the microphone never
/// arms. CONTROL: a press begun after the Stop (naming 1) keys, as its road says, and is let go.
#[test]
fn a_press_made_before_a_stop_the_shack_took_never_keys() {
    // A Stop on the road disarms the process-wide satellite badge: the road tests' guard.
    let _alone = alone();
    on_a_runtime(
        2,
        a_press_made_before_a_stop_the_shack_took_never_keys_on_a_runtime(),
    );
}

async fn a_press_made_before_a_stop_the_shack_took_never_keys_on_a_runtime() {
    let mut cases = Vec::new();
    for by in ["road", "link"] {
        let app = App::new();
        let mut r = roadside(&app);
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
        seat.renewed().await;
        match by {
            "road" => assert!(
                seat.road
                    .road_stop(1, Duration::from_secs(2))
                    .await
                    .is_some(),
                "{by}: premise: the Stop taken"
            ),
            _ => {
                let stopped = counted(&mut seat, 1, "halt_tx", json!({}), 1).await;
                assert_eq!(said(&stopped), "ran halt_tx", "{by}: premise");
            }
        }
        // The road's Stop runs its verbs as soon as the Engine is free: TX off, as Phone armed it.
        assert!(came_to_a_stop(&r.s).await, "{by}: premise: stopped");
        let armed = counted(
            &mut seat,
            2,
            "set_tx_enabled",
            turned_on("set_tx_enabled"),
            1,
        )
        .await;
        assert_eq!(
            said(&armed),
            "ran set_tx_enabled",
            "{by}: premise: armed again"
        );
        seat.renewed().await;
        let late = answers_to_holds(&mut seat, 7, 0, 3).await;
        let mic_after_late = r.s.shared.engine.lock().unwrap().mic_armed();
        seat.renewed().await;
        let keyed = a_press_keys(&mut seat, 8, 1).await;
        done.store(true, Ordering::SeqCst);
        let _ = tokio::task::spawn_blocking(move || radio.join().unwrap()).await;
        cases.push((by, late, mic_after_late, keyed));
    }
    let refused = Down::Press {
        press: 7,
        keyed: false,
        end: Some(HoldEnd::Refused),
    };
    let want: Vec<_> = ["road", "link"]
        .map(|by| (by, vec![refused.clone(); 3], false, true))
        .to_vec();
    assert_eq!(
        cases, want,
        "(the Stop's road, what each late hold was answered, the microphone armed after them, \
         the press begun after the Stop keyed)"
    );
}

/// What the road answered the first `answers` holds of press `press`, made when its computer had
/// pressed `stops` Stops: held every 100 ms, as the computer's road holds it, until that many have
/// been answered or ten seconds pass. Each hold is waited on, not a window of time, so an answer
/// the shack sent late still counts as that hold's; a hold the shack could not judge that moment
/// is answered at the next.
async fn answers_to_holds(seat: &mut Seat, press: u64, stops: u64, answers: usize) -> Vec<Down> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut answered = Vec::new();
    while answered.len() < answers && Instant::now() < deadline {
        seat.road.say(&Up::Hold { press, stops }).await;
        let held = Instant::now();
        while answered.len() < answers {
            let wait = Duration::from_millis(100).saturating_sub(held.elapsed());
            let Some(down) = seat.road.heard(wait).await else {
                break;
            };
            if matches!(down, Down::Press { press: p, .. } if p == press) {
                answered.push(down);
            }
        }
    }
    answered
}

/// Whether a press begun when its computer had pressed `stops` Stops keys: held every 100 ms until
/// the road says it keyed, or how it ended, then let go. A press the shack let lapse before its
/// radio loop keyed it (the station heard no hold for the gap, as when this process stalls) ends
/// `Lapsed`, as it must, and a new press from `first` on is tried, up to five; any other end, or
/// none within ten seconds, is the answer: not keyed.
async fn a_press_keys(seat: &mut Seat, first: u64, stops: u64) -> bool {
    for press in first..first + 5 {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut ended = None;
        while ended.is_none() && Instant::now() < deadline {
            seat.road.say(&Up::Hold { press, stops }).await;
            let held = Instant::now();
            loop {
                let wait = Duration::from_millis(100).saturating_sub(held.elapsed());
                match seat.road.heard(wait).await {
                    None => break,
                    Some(Down::Press {
                        press: p,
                        keyed: true,
                        end: None,
                    }) if p == press => {
                        seat.road.say(&Up::Release { press }).await;
                        return true;
                    }
                    Some(Down::Press { press: p, end, .. }) if p == press && end.is_some() => {
                        ended = end;
                        break;
                    }
                    Some(_) => {}
                }
            }
        }
        seat.road.say(&Up::Release { press }).await;
        if ended != Some(HoldEnd::Lapsed) {
            return false;
        }
    }
    false
}

/// ⛔ A Stop is taken whatever number it carries: its number refuses only a start or a press made
/// before it, never a Stop. After the computer's Stop number 5 is taken, a Stop numbered 2 (a late
/// repeat), one numbered 0, a Stop TX on the link carrying 0, and one carrying no number at all,
/// each with a transmission keyed at the shack before it: each is taken, acknowledged or answered,
/// and stops it. And none of them took back the newest: a CQ made before Stop 5 (carrying 4) is
/// still refused `stopped`. CONTROL: each transmission was on the air when its Stop was sent, and a
/// CQ made after Stop 5 runs.
#[test]
fn a_stop_is_taken_whatever_number_it_carries() {
    let _alone = alone();
    on_a_runtime(2, async {
        let app = App::new();
        let mut r = roadside(&app);
        let _record = on_the_engine(&mut r, gate(false));
        let mut seat = seated(&r).await;
        let mut cases = Vec::new();
        // Stop 5's verbs run once the Engine is free, after its acknowledgement: on the air first,
        // so this waits for them rather than the first case meeting them on its own transmission.
        key_the_mic(&r.s);
        assert!(seat
            .road
            .road_stop(5, Duration::from_secs(2))
            .await
            .is_some());
        assert!(came_to_a_stop(&r.s).await, "Stop 5 stopped");
        for (stop, by) in [
            (2, "road"),
            (0, "road"),
            (0, "link"),
            (9, "link, no number"),
        ] {
            key_the_mic(&r.s);
            let on_the_air = keyed(&r.s);
            let taken = match by {
                "road" => seat
                    .road
                    .road_stop(stop, Duration::from_secs(2))
                    .await
                    .is_some(),
                "link" => {
                    said(&counted(&mut seat, 10 + stop, "halt_tx", json!({}), stop).await)
                        == "ran halt_tx"
                }
                _ => {
                    let id = 10 + stop;
                    let call = json!({"type":"call","id":id,"cmd":"halt_tx","args":{}});
                    said(&answer_to(&mut seat, id, call).await) == "ran halt_tx"
                }
            };
            cases.push((stop, by, on_the_air, taken, came_to_a_stop(&r.s).await));
        }
        assert_eq!(
            cases,
            [
                (2, "road", true, true, true),
                (0, "road", true, true, true),
                (0, "link", true, true, true),
                (9, "link, no number", true, true, true),
            ],
            "(the number, its road, on the air before it, taken, stopped)"
        );
        seat.renewed().await;
        let armed = counted(
            &mut seat,
            30,
            "set_tx_enabled",
            turned_on("set_tx_enabled"),
            5,
        )
        .await;
        assert_eq!(said(&armed), "ran set_tx_enabled", "premise: armed again");
        let before = said(&counted(&mut seat, 31, "call_cq", json!({}), 4).await);
        let after = said(&counted(&mut seat, 32, "call_cq", json!({}), 5).await);
        assert_eq!(
            (before.as_str(), after.as_str()),
            ("refused stopped", "ran call_cq"),
            "(a CQ made before Stop 5, one made after it)"
        );
    });
}
