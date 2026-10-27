//! The Operate strip's Call CQ, a JS8 message, the JS8 repeating CQ and every JS8 switch, a CW
//! macro, the tune carrier, RTTY and PSK continuous TX typed into, JTTY live entry typed into, and an
//! RTTY auto CQ run, pressed on a computer driving the station, on the roadside the road tests use: the shack's real channel, command lane, Stop road
//! and engine over loopback, and the app's own handlers reached as the main window reaches them
//! (`command_dispatch::tauri_runner`). The shack runs each with only the arguments the screen sent
//! and the generations the lane wrote, and each is ended by the link's Stop alone, by the road's
//! Stop alone, and by presence lapsing with the lease still held.
use super::*;
use crate::command_dispatch::{tauri_runner, Runner};

/// A JS8 message long enough to need several frames, so one is on the air with more still to go.
const LONG: &str = "A MESSAGE FROM AFAR LONG ENOUGH TO NEED SEVERAL FRAMES AT NORMAL SPEED";

/// The overs: the Operate strip's Call CQ (a CQ run), a JS8 message once its first frame is
/// keyed, the JS8 repeating CQ switched on, the automatic replies switched on with a reply asking,
/// the heartbeat, relay, heartbeat acks and heartbeat networking switched on, a yes to a reply, a
/// CW macro on the keyer, the tune carrier, RTTY and PSK continuous TX with typed text keying, JTTY
/// live entry with typed text keying, and an RTTY auto CQ run calling.
const OVERS: [&str; 15] = [
    "cq-run",
    "js8",
    "js8-cq-repeat",
    "js8-autoreply",
    "js8-hb",
    "js8-relay",
    "js8-hback",
    "js8-hbnet",
    "js8-reply",
    "cw-macro",
    "tune",
    "rtty-latched",
    "rtty-auto-cq",
    "psk-latched",
    "jtty-live",
];

/// The `js8_arm` switch an over turns on from afar, by its name on the wire.
fn switch(over: &str) -> Option<&'static str> {
    match over {
        "js8-autoreply" => Some("autoreply"),
        "js8-hb" => Some("hb"),
        "js8-relay" => Some("relay"),
        "js8-hback" => Some("hback"),
        "js8-hbnet" => Some("hbnet"),
        _ => None,
    }
}

/// What typed into continuous TX from afar keys.
const TYPED: &str = "CQ CQ DE W9XYZ";

/// The ways an over ends: the link's Stop alone (the road's datagrams never sent), the road's Stop
/// alone (the link no longer read), and presence lapsing while the link keeps the lease.
const ENDS: [&str; 3] = ["link", "road", "presence"];

/// The calls the screen makes for each over, after the shack's own set-up: what it sends, never a
/// time.
fn presses(over: &str) -> Vec<(&'static str, Value)> {
    match over {
        "cq-run" => vec![("start_running", json!({}))],
        "js8" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("js8_send", json!({"to": null, "text": LONG})),
        ],
        "js8-cq-repeat" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("js8_cq_repeat", json!({"on": true})),
        ],
        "js8-autoreply" | "js8-hb" | "js8-relay" | "js8-hback" | "js8-hbnet" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("js8_arm", json!({"which": switch(over), "on": true})),
        ],
        // The reply is named as the shack shows it, once it asks (filled in by the test).
        "js8-reply" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("js8_answer_reply", json!({"yes": true})),
        ],
        "cw-macro" => vec![("send_cw", json!({"text": "CQ {MYCALL} {MYCALL} TEST"}))],
        "tune" => vec![("set_tune", json!({"on": true}))],
        "rtty-latched" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("rtty_set_latched", json!({"on": true})),
            ("rtty_type", json!({"text": TYPED})),
        ],
        "rtty-auto-cq" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("rtty_auto_cq", json!({})),
        ],
        "psk-latched" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("psk_set_latched", json!({"on": true})),
            ("psk_type", json!({"text": TYPED})),
        ],
        // Live entry started with nothing typed, then the typing committed after a pause.
        "jtty-live" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("jtty_live_start", json!({"typed": "", "hisCall": ""})),
            (
                "jtty_live_commit",
                json!({"typed": TYPED, "hisCall": "", "flush": true}),
            ),
        ],
        _ => unreachable!(),
    }
}

/// The kind of over, as the cockpit's Stop and the shack's own Stop TX know it.
fn kind(over: &str) -> &'static str {
    match over {
        "cw-macro" => "cw",
        "tune" => "tune",
        _ if over.starts_with("rtty") => "rtty",
        "psk-latched" => "psk",
        "jtty-live" => "jtty",
        _ => "ft",
    }
}

/// Each call the shack's handlers got, with its arguments, in order.
pub(super) type Got = Arc<Mutex<Vec<(String, Value)>>>;

/// The shack's handlers for these commands, the app's own, on the roadside's engine, reached as
/// the main window reaches them. Every call the lane hands over is recorded with its arguments as
/// the handler got them; the snapshot feed's polls are answered here, as the other road tests do.
pub(super) fn the_apps_handlers(r: &mut Roadside) -> (crate::tests::ShackApp, Got) {
    let (app, main) = crate::tests::shack_app_for_starts_from_afar(r.s.shared.engine.clone());
    let runner: Runner = tauri_runner(app.handle().clone());
    let got = Arc::new(Mutex::new(Vec::new()));
    let record = got.clone();
    let recording: Runner = Arc::new(
        move |cmd: String, args: Value, answer: crate::command_dispatch::Answer| {
            if cmd == "get_snapshot" {
                return answer(Ok(json!({})));
            }
            record.lock().unwrap().push((cmd.clone(), args.clone()));
            runner(cmd, args, answer)
        },
    );
    let voided = r.s.shared.engine.lock().unwrap().remote_starts_voided();
    r.s.shared.commands = super::super::super::commands::Commands::new(recording, voided);
    ((app, main), got)
}

/// The station as the shack's own screen leaves it before the computer presses anything: FT8, the
/// JS8 tier, or the RTTY, PSK or JTTY cockpit (RTTY's Auto on for its run); TX off.
fn set_up_at_the_shack(e: &mut tempo_app::engine::Engine, over: &str) {
    if over.starts_with("js8") {
        use tempo_app::engine::js8::Js8Switch;
        let mut settings = e.settings().clone();
        // Off at the shack, so turning them on from afar is seen; the three that transmit by
        // themselves move only by their own switch.
        settings.js8_hb_networking = over != "js8-hbnet";
        e.apply_settings(settings);
        e.js8_arm(Js8Switch::Autoreply, over != "js8-autoreply")
            .unwrap();
        e.js8_arm(Js8Switch::Relay, over != "js8-relay").unwrap();
        e.js8_arm(Js8Switch::HbAck, over != "js8-hback").unwrap();
        e.js8_enter();
    } else if over.starts_with("rtty") {
        e.set_operating_mode("rtty", false);
        if over == "rtty-auto-cq" {
            e.set_rtty_auto(true).expect("premise: Auto on");
        }
    } else if over == "psk-latched" {
        e.set_operating_mode("keyboard", false);
    } else if over == "jtty-live" {
        e.set_operating_mode("jtty", false);
    } else {
        e.set_tier(tempo_app::dto::Tier::Ft8);
    }
    e.set_tx_enabled(false);
    e.take_slot_tx_abort();
}

/// Whether the over is on the air as the shack keys it: the run armed and calling; the message's
/// first frame keyed by the JS8 sender at its period with more still queued; the repeating CQ
/// armed to key at its period; the automatic replies armed, with a reply asking; the heartbeat,
/// relay and heartbeat acks armed, and heartbeat networking on; the reply queued; the macro holding
/// the transmitter on the keyer; the tune carrier up; continuous TX latched with the typed text
/// what the radio loop keys next; JTTY live entry up with the typed text queued and the radio loop
/// handed a frame; the RTTY run calling CQ with its over holding the transmitter.
fn on_the_air(e: &mut tempo_app::engine::Engine, over: &str) -> bool {
    match over {
        "cq-run" => e.tx_enabled(),
        "js8" => {
            let now = tempo_core::timing::now_unix_ms() as u64 / 1000 / 15;
            let keyed = (now..now + 3).any(|slot| !e.poll_tx(slot).is_empty());
            keyed && !e.js8_state().queue.is_empty()
        }
        "js8-cq-repeat" => e.js8_state().armed.cq,
        "js8-autoreply" => {
            let js8 = e.js8_state();
            js8.armed.autoreply && js8.pending_reply.is_some()
        }
        "js8-hb" => e.js8_state().armed.hb,
        "js8-relay" => e.js8_state().armed.relay,
        "js8-hback" => e.js8_state().armed.hb_ack,
        // Heartbeat networking keys nothing by itself: on, it is what the switch turned.
        "js8-hbnet" => e.settings().js8_hb_networking,
        "js8-reply" => !e.js8_state().queue.is_empty(),
        "cw-macro" => e.tx_owner().is_some(),
        "tune" => e.tuning(),
        "rtty-latched" => {
            e.rtty_latched()
                && e.poll_rtty_stream(64) == tempo_app::engine::RttyStreamTick::Text(TYPED.into())
        }
        "rtty-auto-cq" => e.rtty_state().seq_state == "calling_cq" && e.tx_owner().is_some(),
        "psk-latched" => {
            e.psk_latched()
                && e.poll_psk_stream(64) == tempo_app::engine::PskStreamTick::Text(TYPED.into())
        }
        "jtty-live" => {
            e.jtty_live()
                && e.jtty_state().tx.pending.contains(TYPED)
                && matches!(
                    e.poll_jtty(true, 0.0),
                    tempo_app::engine::jtty_tx::JttyTick::Frame(_)
                )
        }
        _ => unreachable!(),
    }
}

/// What every end leaves, as the shack's own Stop TX leaves it: the transmitter free, TX off, an
/// over in flight cut, the keyer aborted, and for JS8 the queue, the heartbeat and repeating CQ
/// schedules and any reply asking dropped and nothing armed, so nothing keys once TX is armed
/// again; for RTTY and PSK the latch down, nothing typed left to key and the auto run ended, so
/// once TX is armed again the loop keys nothing and the sequencer calls no CQ; for JTTY the live
/// session ended and nothing queued, so once TX is armed again the loop keys nothing.
fn assert_over(e: &mut tempo_app::engine::Engine, over: &str, end: &str) {
    let what = format!("{over}, ended by the {end}");
    assert_stopped(e, kind(over));
    if kind(over) == "rtty" || kind(over) == "psk" {
        use tempo_app::engine::{PskStreamTick, RttyStreamTick};
        assert!(
            !e.rtty_latched() && !e.psk_latched(),
            "{what}: the latch is down"
        );
        assert_eq!(e.rtty_state().seq_state, "idle", "{what}: the run ended");
        e.set_tx_enabled(true);
        e.rtty_auto_service();
        assert_eq!(
            e.poll_rtty_stream(64),
            RttyStreamTick::Idle,
            "{what}: RTTY keys"
        );
        assert_eq!(
            e.poll_psk_stream(64),
            PskStreamTick::Idle,
            "{what}: PSK keys"
        );
        assert_eq!(
            e.rtty_state().seq_state,
            "idle",
            "{what}: the run called again"
        );
        assert!(e.tx_owner().is_none(), "{what}: keyed again");
        e.set_tx_enabled(false);
    }
    if kind(over) == "jtty" {
        let tx = e.jtty_state().tx;
        assert!(!tx.live, "{what}: the live session ended");
        assert_eq!(tx.left, 0, "{what}: nothing queued");
        e.set_tx_enabled(true);
        assert_eq!(
            e.poll_jtty(true, 0.0),
            tempo_app::engine::jtty_tx::JttyTick::Idle,
            "{what}: JTTY keys"
        );
        assert!(e.tx_owner().is_none(), "{what}: keyed again");
        e.set_tx_enabled(false);
    }
    if over.starts_with("js8") {
        let js8 = e.js8_state();
        assert!(js8.queue.is_empty(), "{what}: the queue dropped");
        assert!(!js8.hb_on && !js8.cq_on, "{what}: the schedules ended");
        assert!(js8.pending_reply.is_none(), "{what}: no reply asking");
        let armed = js8.armed;
        assert!(
            !(armed.hb || armed.cq || armed.autoreply || armed.relay || armed.hb_ack),
            "{what}: nothing armed: {armed:?}"
        );
        e.set_tx_enabled(true);
        let now = tempo_core::timing::now_unix_ms() as u64 / 1000 / 15;
        for slot in now..now + 5 {
            assert!(e.poll_tx(slot).is_empty(), "{what}: keyed again at {slot}");
        }
        e.set_tx_enabled(false);
    }
}

/// ⛔ An over started from afar ends by each road alone and by presence. For the Operate strip's
/// Call CQ, a JS8 message, the JS8 repeating CQ and every JS8 switch turned on, a yes to a JS8
/// reply, a CW macro, the tune carrier, RTTY and PSK continuous TX typed into, JTTY live entry typed
/// into, and an RTTY auto CQ run: the computer presses it, the shack's own handler runs it
/// and the station keys it; then the link's Stop alone, the road's Stop alone, or presence lapsing
/// (the heartbeats stop, the lease still kept) ends it, as the shack's own Stop TX would. The shack
/// got only what the screen sent, and `afar` with the two generations alone: no time crossed.
/// CONTROL: each over is on the air before it ends, and each press was answered with no refusal.
#[test]
fn an_over_started_from_afar_ends_by_either_road_alone_and_by_presence() {
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
                let mut presses = presses(over);
                for (n, (cmd, args)) in presses.iter_mut().enumerate() {
                    if *cmd == "js8_answer_reply" {
                        // A query heard at the shack; its reply asks, and the yes names it.
                        let mut e = r.s.shared.engine.lock().unwrap();
                        e.js8_api_hear_snr_query("W1AW");
                        let reply = e.js8_state().pending_reply.expect("premise: a reply asks");
                        args["display"] = json!(reply.display);
                        args["firesAtMs"] = json!(reply.fires_at_ms);
                    }
                    let said = seat.call(n as u64 + 1, cmd, args.clone()).await;
                    assert!(
                        said.get("refused").is_none() && said.get("error").is_none(),
                        "{what}: {cmd} {said}"
                    );
                }
                if over == "js8-autoreply" {
                    // A query heard at the shack once the automatic replies are on.
                    r.s.shared
                        .engine
                        .lock()
                        .unwrap()
                        .js8_api_hear_snr_query("W1AW");
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
                        // What the cockpit's Stop TX sends: CW's stops the keyer, then halts.
                        for (n, stop) in cockpit_stop(kind(over)).into_iter().enumerate() {
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

/// The operator's licence at the shack.
fn licensed(e: &mut tempo_app::engine::Engine, class: tempo_app::settings::LicenseClass) {
    let mut settings = e.settings().clone();
    settings.license_class = class;
    e.apply_settings(settings);
}

/// ⛔ A start from afar outside the operator's licence is refused as the shack's own is: with a
/// Technician licence on 20 m, the tune carrier pressed on the shack's own screen stays down, and
/// pressed from afar, its call answered and run by the shack's own handler, it stays down too.
/// CONTROL: with an Extra licence on the same dial the same press from afar keys the carrier, and
/// the refused press reached the handler, so it was the licence that refused it, not the lane.
#[test]
fn a_start_from_afar_outside_the_licence_is_refused_as_at_the_shack() {
    use tempo_app::settings::LicenseClass;
    let _alone = alone();
    on_a_runtime(2, async {
        let app = App::new();
        let mut r = roadside(&app);
        let (_handlers, got) = the_apps_handlers(&mut r);
        {
            let mut e = r.s.shared.engine.lock().unwrap();
            set_up_at_the_shack(&mut e, "tune");
            e.set_frequency(14.074, "20m", "USB");
            licensed(&mut e, LicenseClass::Technician);
            assert!(!e.tx_allowed(), "premise: outside the licence");
            e.set_tune(true);
            assert!(!e.tuning(), "premise: the shack's own tune refused");
        }
        let mut seat = seated(&r).await;
        seat.renewed().await;
        let said = seat.call(1, "set_tune", json!({"on": true})).await;
        assert!(said.get("refused").is_none(), "the lane refused it: {said}");
        assert!(
            got.lock().unwrap().iter().any(|(cmd, _)| cmd == "set_tune"),
            "CONTROL: the press reached the shack's handler"
        );
        assert!(
            !r.s.shared.engine.lock().unwrap().tuning(),
            "a tune from afar keyed outside the licence"
        );

        licensed(&mut r.s.shared.engine.lock().unwrap(), LicenseClass::Extra);
        assert!(r.s.shared.engine.lock().unwrap().tx_allowed());
        seat.renewed().await;
        let said = seat.call(2, "set_tune", json!({"on": true})).await;
        assert!(said.get("refused").is_none(), "{said}");
        assert!(
            r.s.shared.engine.lock().unwrap().tuning(),
            "CONTROL: inside the licence the same press keys: {said}"
        );
        r.s.shared.engine.lock().unwrap().set_tune(false);
    });
}
