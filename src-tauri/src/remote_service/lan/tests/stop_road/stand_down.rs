//! Stop wins over a start that was in flight, on the roadside the road tests use: the shack's real
//! channel, command lane, Stop road and engine, over loopback, the app's handlers as
//! `held_on_the_engine` stands them in. A start is admitted under the lease, live presence and fresh
//! state, and waits its turn at the Engine (the gate stands in for the wait); a Stop is then taken,
//! or presence lapses and comes back, and the start reaches the Engine after it. It must not run.
use super::*;

/// Waits up to three seconds for `done`.
async fn until(what: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..600 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("never: {what}");
}

/// The result of call `id` on the link, whatever comes before it.
async fn result_of(seat: &mut Seat, id: u64) -> Value {
    loop {
        let said = seat.text().await;
        if said["type"] == "result" && said["id"] == id {
            return said;
        }
    }
}

/// The arguments the screen invokes `cmd` with, to turn it on.
fn on(cmd: &str) -> Value {
    match cmd {
        "set_tx_enabled" => json!({"enabled": true}),
        "play_voice_message" => json!({"slot": 1}),
        _ => json!({}),
    }
}

/// Opens a gate when the test ends, however it ends: a start left waiting at a shut one would hold
/// the test's runtime open.
struct Opens(Gate);

impl Drop for Opens {
    fn drop(&mut self) {
        set_gate(&self.0, false);
    }
}

/// ⛔ A start admitted before a Stop does not run after it. The computer arms TX, then a start (a
/// CQ, or Enable TX) is admitted and waits at the Engine; the operator's Stop is then taken, on the
/// road or on the link, and stops the station (TX off) while presence stays live. When the start
/// reaches the Engine it does not run: TX stays off and the start is answered `stopped`. CONTROL:
/// each premise is asserted (the start waited, the Stop ran first and disarmed TX, presence stayed
/// live).
#[test]
fn a_start_admitted_before_a_stop_does_not_run_after_it() {
    // A Stop on the road disarms the process-wide satellite badge: the road tests' guard.
    let _alone = alone();
    on_a_runtime(
        2,
        a_start_admitted_before_a_stop_does_not_run_after_it_on_a_runtime(),
    );
}

async fn a_start_admitted_before_a_stop_does_not_run_after_it_on_a_runtime() {
    let mut cases = Vec::new();
    for (start, arm) in [("call_cq", "set_tx_enabled"), ("set_tx_enabled", "call_cq")] {
        for by in ["road", "link"] {
            let app = App::new();
            let mut r = roadside(&app);
            let held = gate(true);
            let _open = Opens(held.clone());
            let record = held_on_the_engine(&mut r, held.clone(), start);
            let mut seat = seated(&r).await;
            seat.renewed().await;
            let said = seat.call(1, arm, on(arm)).await;
            assert_eq!(said["value"]["ran"], arm, "{start}/{by}: {said}");
            assert!(
                r.s.shared.engine.lock().unwrap().tx_enabled(),
                "premise: armed"
            );
            send(
                &mut seat.socket,
                json!({"type":"call","id":2,"cmd":start,"args":on(start)}),
            )
            .await;
            until("the start admitted, waiting at the Engine", || {
                ran(&record).len() == 2
            })
            .await;
            match by {
                "road" => assert!(
                    seat.road
                        .road_stop(1, Duration::from_secs(2))
                        .await
                        .is_some(),
                    "{start}/{by}: the Stop acknowledged"
                ),
                _ => {
                    let said = seat.call(3, "halt_tx", json!({})).await;
                    assert_eq!(said["value"]["ran"], "halt_tx", "{start}/{by}: {said}");
                }
            }
            until("the Stop ran: TX off", || {
                !r.s.shared.engine.lock().unwrap().tx_enabled()
            })
            .await;
            assert!(
                r.s.shared
                    .engine
                    .lock()
                    .unwrap()
                    .remote_presence_live(Instant::now()),
                "premise: a Stop leaves presence live"
            );
            set_gate(&held, false);
            let said = result_of(&mut seat, 2).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            let armed = r.s.shared.engine.lock().unwrap().tx_enabled();
            cases.push((format!("{start} (Stop by the {by})"), armed, said));
        }
    }
    let stood: Vec<_> = cases
        .iter()
        .filter(|(_, armed, said)| *armed || said["refused"] != "stopped")
        .collect();
    assert!(
        stood.is_empty(),
        "a start admitted before the Stop ran after it (TX armed, answer): {stood:#?}"
    );
}

/// The lapse that heals. ⛔ A start admitted before presence lapsed does not run after the lapse's
/// halt, even with presence back by then. A CQ is admitted under live presence and waits at the
/// Engine; the heartbeats stop for longer than presence lasts, then one comes again, and its
/// renewal first carries out the lapse (the station halted, TX off) before presence is live again.
/// When the CQ then reaches the Engine it does not run. CONTROL: each premise is asserted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_admitted_before_a_lapse_does_not_run_after_its_halt() {
    let app = App::new();
    let mut r = roadside(&app);
    let held = gate(true);
    let _open = Opens(held.clone());
    let record = held_on_the_engine(&mut r, held.clone(), "call_cq");
    let mut seat = seated(&r).await;
    seat.renewed().await;
    let renewed = Instant::now();
    let said = seat
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
    send(
        &mut seat.socket,
        json!({"type":"call","id":2,"cmd":"call_cq","args":{}}),
    )
    .await;
    until("the CQ admitted, waiting at the Engine", || {
        ran(&record).len() == 2
    })
    .await;
    while renewed.elapsed() < PRESENCE + Duration::from_millis(300) {
        tokio::time::sleep(Duration::from_millis(500)).await;
        seat.keep_the_lease().await;
    }
    seat.renewed().await;
    assert!(
        !r.s.shared.engine.lock().unwrap().tx_enabled(),
        "premise: the lapse halted the station"
    );
    assert!(
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .remote_presence_live(Instant::now()),
        "premise: presence is live again"
    );
    set_gate(&held, false);
    let said = result_of(&mut seat, 2).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let armed = r.s.shared.engine.lock().unwrap().tx_enabled();
    assert!(
        !armed && said["refused"] == "stopped",
        "a CQ admitted before presence lapsed ran after the lapse's halt: armed {armed}, {said}, \
         ran {:?}",
        ran(&record)
    );
}

/// ⛔ A start admitted before an unkey (the off half of a switch), or a Stop that names one over,
/// does not run after it either. An unkey halts nothing, so only the link moving the starts-voided
/// generation as it reads it voids what was waiting; the voice keyer's Stop moves the stand-down as
/// it is read. Each case on a link with no Stop on the road: Enable TX's off half behind a CQ (TX
/// stays off, no CQ armed); and behind the voice keyer's play (nothing queued to play), its Stop,
/// the PTT let go (also what Phone's Lock and leaving Phone send), Tune off (also the end of the
/// setup wizard's Prove TX) and the contest screen's S&P chip. The off half goes with an `afar` of
/// its own, which a computer never writes: it still runs, because nothing it carries can refuse a
/// Stop. CONTROL: each premise is asserted (the start waited, the unkey or the Stop ran first).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_admitted_before_an_off_half_or_the_keyers_stop_does_not_run_after_it() {
    let mut stood = Vec::new();
    for (start, stop, stop_args) in [
        (
            "call_cq",
            "set_tx_enabled",
            json!({"enabled": false, "afar": {"standDown": 0}}),
        ),
        ("play_voice_message", "stop_voice", json!({})),
        ("play_voice_message", "set_ptt", json!({"on": false})),
        ("play_voice_message", "set_tune", json!({"on": false})),
        (
            "play_voice_message",
            "set_mode",
            json!({"mode": "fieldday-sp"}),
        ),
    ] {
        let app = App::new();
        let mut r = roadside(&app);
        let held = gate(true);
        let _open = Opens(held.clone());
        let record = held_on_the_engine(&mut r, held.clone(), start);
        let mut seat = seated(&r).await;
        seat.renewed().await;
        let said = seat
            .call(1, "set_tx_enabled", json!({"enabled": true}))
            .await;
        assert_eq!(said["value"]["ran"], "set_tx_enabled", "{start}: {said}");
        send(
            &mut seat.socket,
            json!({"type":"call","id":2,"cmd":start,"args":on(start)}),
        )
        .await;
        until("the start admitted, waiting at the Engine", || {
            ran(&record).len() == 2
        })
        .await;
        let said = seat.call(3, stop, stop_args.clone()).await;
        assert_eq!(said["value"]["ran"], stop, "premise {start}: {said}");
        if stop == "set_tx_enabled" {
            assert!(
                !r.s.shared.engine.lock().unwrap().tx_enabled(),
                "premise: TX off"
            );
        }
        set_gate(&held, false);
        let said = result_of(&mut seat, 2).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut e = r.s.shared.engine.lock().unwrap();
        let on_air = match start {
            "call_cq" => e.tx_enabled(),
            _ => e.poll_voice().is_some(),
        };
        if on_air || said["refused"] != "stopped" {
            stood.push(format!("{start} behind {stop} {stop_args}: {said}"));
        }
    }
    assert!(
        stood.is_empty(),
        "a start admitted before the off half or the keyer's Stop ran after it: {stood:#?}"
    );
}

/// ⛔ A start is bound as the link reads it, not when it is admitted. The authority is held, as a
/// slow request would hold it, so a CQ read off the link waits to be admitted; the operator's Stop,
/// read after it, runs at once and stops the station. When the authority is free the CQ is
/// admitted and reaches the Engine, after the Stop: it does not run. CONTROL: the CQ waited (it had
/// not run when the Stop did), and the Stop disarmed TX.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_is_bound_as_the_link_reads_it_however_long_its_admission_waits() {
    let app = App::new();
    let mut r = roadside(&app);
    let record = held_on_the_engine(&mut r, gate(false), "none");
    let mut seat = seated(&r).await;
    seat.renewed().await;
    let said = seat
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
    let authority = r.s.shared.authority.clone();
    let waiting = authority.held();
    send(
        &mut seat.socket,
        json!({"type":"call","id":2,"cmd":"call_cq","args":{}}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let said = seat.call(3, "halt_tx", json!({})).await;
    assert_eq!(said["value"]["ran"], "halt_tx", "{said}");
    assert_eq!(
        ran(&record),
        ["set_tx_enabled", "halt_tx"],
        "premise: the CQ waits to be admitted"
    );
    assert!(
        !r.s.shared.engine.lock().unwrap().tx_enabled(),
        "premise: the Stop disarmed TX"
    );
    drop(waiting);
    let said = result_of(&mut seat, 2).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let armed = r.s.shared.engine.lock().unwrap().tx_enabled();
    assert!(
        !armed && said["refused"] == "stopped",
        "a CQ read before the Stop ran after it: armed {armed}, {said}, ran {:?}",
        ran(&record)
    );
}

/// ⛔ A start that runs after presence went is stopped at once (the signed rules' own words). A CQ
/// is admitted under live presence and waits at the Engine; the authority is held meanwhile, as a
/// slow request would hold it, so nothing at the station carries out the lapse when presence runs
/// out (here no radio loop polls presence; at a station one does, a tick later). The stand-down
/// still stands, so the CQ runs, and the station is stopped behind it. CONTROL: before the CQ runs,
/// TX is still armed and the engine holds no live presence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_that_runs_after_presence_ran_out_is_stopped_at_once() {
    let app = App::new();
    let mut r = roadside(&app);
    let held = gate(true);
    let _open = Opens(held.clone());
    let record = held_on_the_engine(&mut r, held.clone(), "call_cq");
    let mut seat = seated(&r).await;
    seat.renewed().await;
    let renewed = Instant::now();
    let said = seat
        .call(1, "set_tx_enabled", json!({"enabled": true}))
        .await;
    assert_eq!(said["value"]["ran"], "set_tx_enabled", "{said}");
    send(
        &mut seat.socket,
        json!({"type":"call","id":2,"cmd":"call_cq","args":{}}),
    )
    .await;
    until("the CQ admitted, waiting at the Engine", || {
        ran(&record).len() == 2
    })
    .await;
    let authority = r.s.shared.authority.clone();
    let busy = authority.held();
    tokio::time::sleep_until((renewed + PRESENCE + Duration::from_millis(300)).into()).await;
    {
        let e = r.s.shared.engine.lock().unwrap();
        assert!(e.tx_enabled(), "premise: nothing halted the station yet");
        assert!(
            !e.remote_presence_live(Instant::now()),
            "premise: presence ran out"
        );
    }
    set_gate(&held, false);
    let said = result_of(&mut seat, 2).await;
    drop(busy);
    assert_eq!(said["value"]["ran"], "call_cq", "{said}");
    until("the station stopped behind it", || ran(&record).len() == 3).await;
    assert_eq!(ran(&record), ["set_tx_enabled", "call_cq", "halt_tx"]);
    assert!(
        !r.s.shared.engine.lock().unwrap().tx_enabled(),
        "a CQ that ran after presence ran out stood"
    );
}
