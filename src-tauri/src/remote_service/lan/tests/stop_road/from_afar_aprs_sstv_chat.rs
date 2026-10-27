//! APRS, an SSTV picture, chat (Nexus to Nexus) and a CQ run entered as a mode, pressed on a
//! computer driving the station, on the roadside `from_afar` uses: the shack's real channel,
//! command lane, Stop road and engine over loopback, and the app's own handlers reached as the main
//! window reaches them. Each is ended by the link's Stop alone, by the road's Stop alone, and by
//! presence lapsing with the lease still held; what transmits by itself after its press (APRS's
//! automatic ack, the presence beacon, the chat CQ run) is turned off by the lapse.
use super::from_afar::the_apps_handlers;
use super::*;

/// A chat message or broadcast long enough to need several frames.
const LONG: &str = "A MESSAGE FROM AFAR LONG ENOUGH TO NEED SEVERAL FRAMES ON THE AIR";

/// The station heard on the air, whom a chat message goes to and an APRS message is for.
const PEER: &str = "W1AW";

/// The overs: an APRS beacon and an APRS message, each on the air with a second still queued;
/// APRS Monitor armed by hand with an automatic ack on the air; an SSTV picture; a chat message and
/// a broadcast, each with its first frame keyed; the chat CQ run, turned on and resumed; the
/// presence beacon; and a CQ run entered as a mode.
const OVERS: [&str; 10] = [
    "aprs-beacon",
    "aprs-message",
    "aprs-ack",
    "sstv",
    "chat-message",
    "chat-broadcast",
    "chat-cq",
    "chat-cq-resumed",
    "chat-beacon",
    "qso-run",
];

/// The ways an over ends, as in `from_afar`.
const ENDS: [&str; 3] = ["link", "road", "presence"];

/// What transmits by itself after its press, and so is turned off when presence lapses.
fn on_its_own(over: &str) -> bool {
    matches!(
        over,
        "aprs-ack" | "chat-cq" | "chat-cq-resumed" | "chat-beacon"
    )
}

/// The calls the screen makes for each over, after the shack's own set-up: never a time.
fn presses(over: &str) -> Vec<(&'static str, Value)> {
    let tx_on = ("set_tx_enabled", json!({"enabled": true}));
    let beacon = (
        "aprs_send_beacon",
        json!({"lat": 41.9, "lon": -87.6, "symbolTable": "/", "symbolCode": ">",
               "comment": "FROM AFAR", "path": ["WIDE1-1"]}),
    );
    let message = (
        "aprs_send_message",
        json!({"addressee": PEER, "text": "HELLO FROM AFAR"}),
    );
    match over {
        "aprs-beacon" => vec![tx_on, beacon.clone(), beacon],
        "aprs-message" => vec![tx_on, message.clone(), message],
        "aprs-ack" => vec![tx_on, ("aprs_arm", json!({"on": true}))],
        "sstv" => vec![tx_on, ("sstv_send", crate::tests::sstv_picture())],
        "chat-message" => vec![tx_on, ("send_message", json!({"peer": PEER, "text": LONG}))],
        "chat-broadcast" => vec![tx_on, ("broadcast", json!({"text": LONG}))],
        "chat-cq" => vec![("set_chat_cq", json!({"on": true}))],
        "chat-cq-resumed" => vec![("resume_chat_cq", json!({}))],
        "chat-beacon" => vec![tx_on, ("set_beacon", json!({"on": true}))],
        "qso-run" => vec![("set_mode", json!({"mode": "qso-run"}))],
        _ => unreachable!(),
    }
}

/// The station as the shack's own screen leaves it before the computer presses anything: the
/// section the over belongs to, nothing of APRS or chat on, TX off. For a resumed run, the run on
/// at the shack and Stopped; for a chat message, its peer heard on the air.
fn set_up_at_the_shack(e: &mut tempo_app::engine::Engine, over: &str) {
    match over {
        "sstv" => e.set_operating_mode("phone", false),
        "qso-run" => e.set_tier(tempo_app::dto::Tier::Ft8),
        _ if over.starts_with("aprs") => {}
        _ => {
            e.set_mode("chat").unwrap();
            e.hear_for_test(&format!("CQ {PEER} FN31"), -5, slot_now());
        }
    }
    if over == "chat-cq-resumed" {
        e.set_chat_cq(true).unwrap();
        e.halt_tx();
    }
    e.set_tx_enabled(false);
    // The latch going down arms the aborts; taken, so an end must arm them again.
    e.take_slot_tx_abort();
    e.take_sstv_abort();
}

/// The slot now, as the shack counts its four-second chat slots.
fn slot_now() -> u64 {
    tempo_core::timing::now_unix_ms() as u64 / 4000
}

/// Whether something keys in a few slots `ahead` of now, as the shack's slots key it: each check
/// asks about slots of its own, none asked before.
fn keys(e: &mut tempo_app::engine::Engine, ahead: u64) -> bool {
    let from = slot_now() + ahead;
    (from..from + 6).any(|slot| !e.poll_tx(slot).is_empty())
}

/// Whether the over is on the air as the shack keys it.
fn on_the_air(e: &mut tempo_app::engine::Engine, over: &str) -> bool {
    match over {
        // The first frame keyed, by the loop's own poll; the second is still queued.
        "aprs-beacon" | "aprs-message" => e.poll_aprs_tx().is_some(),
        // A message for this station heard on the air: the ack keys by itself.
        "aprs-ack" => {
            let mine = e.settings().mycall.clone();
            e.aprs_auto_ack("N0CALL-7", &mine, "042");
            e.aprs_arm_source() == tempo_app::engine::AprsArm::Explicit
                && e.poll_aprs_tx().is_some()
        }
        "sstv" => e.tx_owner() == Some(tempo_app::engine::TxOwner::Sstv),
        "chat-message" | "chat-broadcast" => keys(e, 0),
        "chat-cq" | "chat-cq-resumed" => e.chat_cq_state() == "calling" && keys(e, 0),
        "chat-beacon" => e.beacon_enabled(),
        "qso-run" => e.tx_enabled(),
        _ => unreachable!(),
    }
}

/// What every end leaves, as the shack's own Stop TX leaves it: the transmitter free, TX off, an
/// over in flight cut, an SSTV picture aborted, and what APRS and chat had queued dropped. What
/// transmits by itself is turned off too, so once TX is armed again nothing keys: no ack, no CQ,
/// no beacon, no message.
fn assert_over(e: &mut tempo_app::engine::Engine, over: &str, end: &str) {
    let what = format!("{over}, ended by the {end}");
    assert_stopped(e, if over == "sstv" { "sstv" } else { "ft" });
    assert!(e.poll_aprs_tx().is_none(), "{what}: APRS's queue dropped");
    assert!(!keys(e, 100), "{what}: keyed with TX off");
    if on_its_own(over) {
        let on = [
            e.aprs_arm_source() == tempo_app::engine::AprsArm::Explicit,
            e.beacon_enabled(),
            e.chat_cq_state() != "off",
        ];
        assert_eq!(on, [false; 3], "{what}: each turned off");
    }
    e.set_tx_enabled(true);
    if over.starts_with("aprs") {
        let mine = e.settings().mycall.clone();
        e.aprs_auto_ack("N0CALL-7", &mine, "043");
        assert!(e.poll_aprs_tx().is_none(), "{what}: APRS keyed again");
    } else if over != "sstv" && over != "qso-run" {
        assert!(!keys(e, 200), "{what}: keyed again");
    }
    e.set_tx_enabled(false);
}

/// ⛔ APRS, an SSTV picture, chat and a CQ run entered as a mode, started from afar, end by each
/// road alone and by presence. The computer presses it, the shack's own handler runs it and the
/// station keys it; then the link's Stop alone, the road's Stop alone, or presence lapsing (the
/// heartbeats stop, the lease still kept) ends it, as the shack's own Stop TX would, and each turns
/// off what transmits by itself. The shack got only what the screen sent, and `afar`
/// with the two generations alone: no time crossed. CONTROL: each over is on the air before it
/// ends, and each press was answered with no refusal.
#[test]
fn aprs_sstv_and_chat_started_from_afar_end_by_either_road_alone_and_by_presence() {
    let _alone = alone();
    on_a_runtime(2, async {
        for over in OVERS {
            for end in ENDS {
                let what = format!("{over}, ended by the {end}");
                let app = App::new();
                let mut r = roadside(&app);
                let (_handlers, got) = the_apps_handlers(&mut r);
                set_up_at_the_shack(&mut r.s.shared.engine.lock().unwrap(), over);
                let mut seat = seated(&r).await;
                seat.renewed().await;
                let presses = presses(over);
                for (n, (cmd, args)) in presses.iter().enumerate() {
                    let said = seat.call(n as u64 + 1, cmd, args.clone()).await;
                    assert!(
                        said.get("refused").is_none() && said.get("error").is_none(),
                        "{what}: {cmd} {said}"
                    );
                }
                assert!(
                    on_the_air(&mut r.s.shared.engine.lock().unwrap(), over),
                    "{what}: premise, on the air"
                );
                // No time crossed: each press reached the handler with what the screen sent and
                // the generations the lane wrote, nothing else.
                let got = got.lock().unwrap().clone();
                for (cmd, args) in &presses {
                    let (_, reached) = got
                        .iter()
                        .find(|(name, _)| name == cmd)
                        .unwrap_or_else(|| panic!("{what}: {cmd} never reached the shack"));
                    let mut sent = args.as_object().unwrap().clone();
                    let afar = reached["afar"].clone();
                    sent.insert("afar".into(), afar.clone());
                    assert_eq!(reached, &Value::Object(sent), "{what}: {cmd}");
                    let keys: Vec<&String> = afar.as_object().unwrap().keys().collect();
                    assert_eq!(keys, ["standDown", "startsVoided"], "{what}: {cmd} {afar}");
                }
                match end {
                    "link" => {
                        let kind = if over == "sstv" { "sstv" } else { "ft" };
                        for (n, stop) in cockpit_stop(kind).into_iter().enumerate() {
                            let said = seat.call(90 + n as u64, stop, json!({})).await;
                            assert!(said.get("error").is_none(), "{what}: {said}");
                        }
                    }
                    "road" => {
                        seat.valve.shut();
                        assert!(
                            seat.road
                                .road_stop(1, Duration::from_secs(2))
                                .await
                                .is_some(),
                            "{what}: the Stop acknowledged"
                        );
                        assert!(came_to_a_stop(&r.s).await, "{what}");
                    }
                    _ => {
                        let renewed = Instant::now();
                        while renewed.elapsed() < PRESENCE + Duration::from_millis(300) {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            seat.keep_the_lease().await;
                        }
                        // The radio loop's tick, which carries out a lapse.
                        r.s.shared
                            .engine
                            .lock()
                            .unwrap()
                            .poll_remote_transmit(Instant::now());
                    }
                }
                assert_over(&mut r.s.shared.engine.lock().unwrap(), over, end);
            }
        }
    });
}
