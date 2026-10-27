//! An unkey pressed on a computer driving the station, on the roadside the road tests use: the
//! shack's real channel, command lane and engine over loopback, and the app's own handlers reached
//! as the main window reaches them. An unkey is the off half of a transmit switch, the contest
//! screen's S&P chip among them. From afar it does at the shack what the same press does at the
//! shack's own screen, and on the radio loop's next tick nothing more: what other starts from afar
//! began goes on. A Stop, the voice keyer's own included, still ends everything.
use super::from_afar::the_apps_handlers;
use super::*;
use crate::command_classes::Class;
use crate::command_dispatch::{off_args, Answer, OFF_HALVES};

/// Every unkey a computer sends: each switch's off half as the table reads it, and the contest
/// screen's S&P chip. The PTT's off half is also what Phone's Lock and leaving Phone send; Tune's,
/// what the setup wizard's Prove TX sends as it ends. JS8's switch names which one, as the screen
/// does: without it the press is answered with an error at both ends and turns nothing off.
fn unkeys() -> Vec<(&'static str, Value)> {
    OFF_HALVES
        .iter()
        .map(|(cmd, ..)| match *cmd {
            "js8_arm" => (*cmd, json!({"which": "autoreply", "on": false})),
            _ => (*cmd, off_args(cmd).unwrap()),
        })
        .chain([("set_mode", json!({"mode": "fieldday-sp"}))])
        .collect()
}

/// The station as the shack's own screen leaves it before the press: in the section the unkey is
/// pressed in, TX off.
fn set_up(e: &mut tempo_app::engine::Engine, cmd: &str) {
    match cmd {
        "js8_arm" | "js8_cq_repeat" => e.js8_enter(),
        "rtty_set_auto" | "rtty_set_latched" => e.set_operating_mode("rtty", false),
        "psk_set_latched" => e.set_operating_mode("keyboard", false),
        "set_mode" => {
            e.set_tier(tempo_app::dto::Tier::Ft8);
            // Field Day S&P asks for the exchange it would send.
            let mut settings = e.settings().clone();
            settings.fd_class = "1D".into();
            settings.fd_section = "IL".into();
            e.apply_settings(settings);
        }
        _ => e.set_operating_mode("phone", false),
    }
    e.set_tx_enabled(false);
    e.take_slot_tx_abort();
}

/// What the station holds after the radio loop's tick: TX armed, the tune carrier, the PTT, either
/// latch.
fn holds(e: &mut tempo_app::engine::Engine) -> (bool, bool, bool, bool, bool) {
    e.poll_remote_transmit(Instant::now());
    (
        e.tx_enabled(),
        e.tuning(),
        e.manual_ptt(),
        e.rtty_latched(),
        e.psk_latched(),
    )
}

/// [`holds`], and the unkey's own switch ([`own`]).
fn holds_and_own(
    e: &mut tempo_app::engine::Engine,
    cmd: &str,
) -> ((bool, bool, bool, bool, bool), bool) {
    (holds(e), own(e, cmd))
}

/// The switch an unkey is the off half of, turned on at the shack between TX On and the tune
/// carrier (a latch refuses under a carrier), so the unkey has something of its own to turn off.
fn switch_on(e: &mut tempo_app::engine::Engine, cmd: &str) {
    use tempo_app::engine::js8::Js8Switch;
    let on = match cmd {
        "aprs_arm" => {
            e.set_aprs_arm(tempo_app::engine::AprsArm::Explicit);
            Ok(())
        }
        "js8_arm" => e.js8_arm(Js8Switch::Autoreply, true),
        "js8_cq_repeat" => e.js8_set_cq_repeat(true),
        "psk_set_latched" => e.set_psk_latched(true),
        "rtty_set_auto" => e.set_rtty_auto(true),
        "rtty_set_latched" => e.set_rtty_latched(true),
        "set_beacon" => {
            e.set_beacon(true);
            Ok(())
        }
        "set_chat_cq" => e.set_chat_cq(true),
        "set_mode" => e.set_mode("qso-run"),
        "set_ptt" => {
            e.set_ptt(true);
            Ok(())
        }
        _ => Ok(()),
    };
    assert_eq!(
        on,
        Ok(()),
        "CONTROL {cmd}: its switch turns on at the shack"
    );
}

/// Whether the switch an unkey is the off half of is on.
fn own(e: &tempo_app::engine::Engine, cmd: &str) -> bool {
    match cmd {
        "aprs_arm" => e.aprs_arm_source() == tempo_app::engine::AprsArm::Explicit,
        "js8_arm" => e.js8_state().autoreply,
        "js8_cq_repeat" => e.js8_state().cq_on,
        "psk_set_latched" => e.psk_latched(),
        "rtty_set_auto" => e.rtty_state().auto,
        "rtty_set_latched" => e.rtty_latched(),
        "set_beacon" => e.beacon_enabled(),
        "set_chat_cq" => e.chat_cq_state() != "off",
        "set_mode" => e.snapshot().qso.is_some_and(|q| q.cq_running),
        "set_ptt" => e.manual_ptt(),
        "set_tune" => e.tuning(),
        "set_tx_enabled" => e.tx_enabled(),
        _ => unreachable!("{cmd}"),
    }
}

/// One call through the shack's own handlers, as its own screen makes it: no `afar`.
fn at_the_shack(r: &Roadside, cmd: &str, args: Value) -> Result<Value, String> {
    let (answer, answered) = std::sync::mpsc::channel();
    let reply: Answer = Box::new(move |result| {
        let _ = answer.send(result);
    });
    (r.s.shared.commands.runner)(cmd.to_string(), args, reply);
    answered
        .recv_timeout(Duration::from_secs(5))
        .expect("the handler answered")
}

/// ⛔ An unkey from afar ends its own over and nothing else. For every unkey: TX On, the unkey's own
/// switch and the tune carrier on, then the unkey from the computer driving the station, then the
/// radio loop's tick, leave the station, that switch included, as the same presses at the shack's
/// own screen leave it. CONTROL: no press from afar was refused or answered with an error; the
/// switch was on before the unkey and off after it at the shack, so a handler that did nothing from
/// afar would not match; the PTT let go and Tune off each leave TX on at the shack's own screen, so
/// this tells a halt from an unkey; and a real Stop from afar after all of that still halts
/// everything.
#[test]
fn an_unkey_from_afar_ends_its_own_over_and_nothing_else() {
    let _alone = alone();
    on_a_runtime(2, async {
        let mut wrong = Vec::new();
        for (cmd, args) in unkeys() {
            let what = format!("{cmd} {args}");
            // The shack's own screen.
            let app = App::new();
            let mut shack = roadside(&app);
            let _own_handlers = the_apps_handlers(&mut shack);
            set_up(&mut shack.s.shared.engine.lock().unwrap(), cmd);
            for (press, press_args) in [
                ("set_tx_enabled", json!({"enabled": true})),
                ("set_tune", json!({"on": true})),
            ] {
                if press == "set_tune" {
                    switch_on(&mut shack.s.shared.engine.lock().unwrap(), cmd);
                }
                let ran = at_the_shack(&shack, press, press_args);
                assert!(ran.is_ok(), "CONTROL {what}: {press} at the shack: {ran:?}");
            }
            assert!(
                own(&shack.s.shared.engine.lock().unwrap(), cmd),
                "CONTROL {what}: its switch is on before the unkey"
            );
            let ran = at_the_shack(&shack, cmd, args.clone());
            assert!(
                ran.is_ok(),
                "CONTROL {what}: the unkey at the shack: {ran:?}"
            );
            let expected = holds_and_own(&mut shack.s.shared.engine.lock().unwrap(), cmd);
            assert!(
                !expected.1,
                "CONTROL {what}: the unkey turns its own switch off at the shack"
            );
            if matches!(cmd, "set_ptt" | "set_tune") {
                assert!(expected.0 .0, "CONTROL {what}: TX stays on at the shack");
            }

            // The same presses from afar.
            let app = App::new();
            let mut r = roadside(&app);
            let _handlers = the_apps_handlers(&mut r);
            set_up(&mut r.s.shared.engine.lock().unwrap(), cmd);
            let mut seat = seated(&r).await;
            seat.renewed().await;
            for (id, (press, press_args)) in [
                ("set_tx_enabled", json!({"enabled": true})),
                ("set_tune", json!({"on": true})),
                (cmd, args.clone()),
            ]
            .into_iter()
            .enumerate()
            {
                if id == 1 {
                    switch_on(&mut r.s.shared.engine.lock().unwrap(), cmd);
                }
                let said = seat.call(id as u64 + 1, press, press_args).await;
                assert!(
                    said.get("refused").is_none() && said.get("error").is_none(),
                    "{what}: {press} from afar was refused: {said}"
                );
            }
            let got = holds_and_own(&mut r.s.shared.engine.lock().unwrap(), cmd);
            if got != expected {
                wrong.push(format!(
                    "{what}: (TX, tuning, PTT, RTTY latched, PSK latched, its own switch) from \
                     afar {got:?}, at the shack {expected:?}"
                ));
            }

            // CONTROL: a real Stop from afar still halts everything.
            let said = seat.call(9, "halt_tx", json!({})).await;
            assert!(said.get("refused").is_none(), "CONTROL {what}: {said}");
            let stopped = holds(&mut r.s.shared.engine.lock().unwrap());
            assert_eq!(
                stopped,
                (false, false, false, false, false),
                "CONTROL {what}: a Stop from afar halts everything"
            );
        }
        assert!(wrong.is_empty(), "an unkey from afar: {wrong:#?}");
    });
}

/// ⛔ A Stop is not an unkey: on the command link alone it still ends everything a computer started
/// from afar, on the radio loop's next tick. For every command that stops a transmission (the
/// voice keyer's Stop included): TX On and the tune carrier pressed from afar, then that Stop by the link alone (the road
/// never sent it), then the tick, and nothing is armed or keyed. CONTROL: TX On and the carrier were
/// up before the Stop; and each of those Stops but `halt_tx` leaves TX on at the shack's own screen,
/// so it is the Stop from afar, not its handler, that ends everything.
#[test]
fn a_stop_on_the_link_alone_ends_everything_started_from_afar() {
    let _alone = alone();
    on_a_runtime(2, async {
        let stops: Vec<&str> = crate::command_classes::classes()
            .filter(|(_, class)| *class == Class::StopsTx)
            .map(|(cmd, _)| cmd)
            .collect();
        assert_eq!(stops.len(), 9, "{stops:?}");
        let mut wrong = Vec::new();
        for stop in stops {
            if stop != "halt_tx" {
                let app = App::new();
                let mut own = roadside(&app);
                let _own_handlers = the_apps_handlers(&mut own);
                set_up(&mut own.s.shared.engine.lock().unwrap(), stop);
                assert!(at_the_shack(&own, "set_tx_enabled", json!({"enabled": true})).is_ok());
                let _ = at_the_shack(&own, stop, json!({}));
                assert!(
                    holds(&mut own.s.shared.engine.lock().unwrap()).0,
                    "CONTROL {stop}: TX stays on at the shack"
                );
            }
            let app = App::new();
            let mut r = roadside(&app);
            let _handlers = the_apps_handlers(&mut r);
            set_up(&mut r.s.shared.engine.lock().unwrap(), stop);
            let mut seat = seated(&r).await;
            seat.renewed().await;
            for (id, (press, args)) in [
                ("set_tx_enabled", json!({"enabled": true})),
                ("set_tune", json!({"on": true})),
            ]
            .into_iter()
            .enumerate()
            {
                let said = seat.call(id as u64 + 1, press, args).await;
                assert!(said.get("refused").is_none(), "{stop}: {press}: {said}");
            }
            let up = holds(&mut r.s.shared.engine.lock().unwrap());
            assert!(
                up.0 && up.1,
                "CONTROL {stop}: TX on and the carrier up: {up:?}"
            );
            let said = seat.call(3, stop, json!({})).await;
            assert!(said.get("refused").is_none(), "{stop}: {said}");
            let left = holds(&mut r.s.shared.engine.lock().unwrap());
            if left != (false, false, false, false, false) {
                wrong.push(format!(
                    "{stop}: (TX, tuning, PTT, RTTY latched, PSK latched) {left:?}"
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "a Stop on the link alone left what was started from afar: {wrong:#?}"
        );
    });
}

/// ⛔ Giving up the lease still ends everything started from afar, after an unkey left it running.
/// A JTTY live session, and the tune carrier, each with TX On, pressed from afar; the PTT let go
/// from afar, and the radio loop's tick leaves each running; then the computer hands the lease
/// back, and the next tick ends everything: TX off, no carrier, no live session. CONTROL: each was
/// running before the lease went.
#[test]
fn giving_up_the_lease_ends_what_an_unkey_left_running() {
    let _alone = alone();
    on_a_runtime(2, async {
        for over in ["jtty-live", "tune"] {
            let app = App::new();
            let mut r = roadside(&app);
            let _handlers = the_apps_handlers(&mut r);
            {
                let mut e = r.s.shared.engine.lock().unwrap();
                let section = if over == "tune" { "phone" } else { "jtty" };
                e.set_operating_mode(section, false);
                e.set_tx_enabled(false);
            }
            let mut seat = seated(&r).await;
            seat.renewed().await;
            let start = if over == "tune" {
                ("set_tune", json!({"on": true}))
            } else {
                ("jtty_live_start", json!({"typed": "", "hisCall": ""}))
            };
            for (id, (press, args)) in [
                ("set_tx_enabled", json!({"enabled": true})),
                start,
                ("set_ptt", json!({"on": false})),
            ]
            .into_iter()
            .enumerate()
            {
                let said = seat.call(id as u64 + 1, press, args).await;
                assert!(said.get("refused").is_none(), "{over}: {press}: {said}");
            }
            let running = |e: &mut tempo_app::engine::Engine| {
                let (tx, tuning, ..) = holds(e);
                (tx, tuning, e.jtty_live())
            };
            let before = running(&mut r.s.shared.engine.lock().unwrap());
            let expected = (true, over == "tune", over == "jtty-live");
            assert_eq!(before, expected, "{over}: running after the unkey");
            let lease = seat.lease();
            let released = seat.ask(json!({"type":"release","leaseId":lease})).await;
            assert!(released["error"].is_null(), "{over}: {released}");
            let after = running(&mut r.s.shared.engine.lock().unwrap());
            assert_eq!(
                after,
                (false, false, false),
                "{over}: (TX, tuning, JTTY live) after the lease went"
            );
        }
    });
}
