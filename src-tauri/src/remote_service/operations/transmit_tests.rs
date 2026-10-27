//! Exercise the real lease authority and Stop around native FT ownership.
use super::*;

/// A controlling browser's FT8 CQ on the air, as a streamed controller's Call CQ puts it there:
/// the native verb, under the session's presence minted from the lease.
fn armed() -> (Fixture, Instant, Value) {
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.set_tx_enabled(false);
        e.take_immediate_retune();
        e.take_slot_tx_abort();
    }
    let now = Instant::now();
    let state = acquire_controls_version(&f, now, 3);
    let presence = f
        .authority
        .stream_presence(SESSION, DEVICE, state["leaseId"].as_str().unwrap(), now)
        .unwrap();
    {
        let mut e = f.engine.lock().unwrap();
        e.start_cq(None).unwrap();
        assert!(e.hold_remote_presence(presence, now));
    }
    (f, now, state)
}

#[test]
fn transmit_revocation_is_synchronous_even_while_engine_is_busy() {
    for cause in [
        "stationGrant",
        "disconnect",
        "connection",
        "invalidate",
        "retire",
    ] {
        let (f, _, _) = armed();
        // These paths must revoke without waiting on Engine. This lock models
        // an in-flight modem or other native operation.
        let mut e = f.engine.lock().unwrap();
        match cause {
            "stationGrant" => {
                f.authority.permit_station(DEVICE, false).unwrap();
            }
            "disconnect" => f.authority.disconnect_session(SESSION),
            "connection" => {
                f.authority.start_connection();
            }
            "invalidate" => f.authority.invalidate(),
            "retire" => f.authority.retire_connection(f.connection),
            _ => unreachable!(),
        }
        assert!(e.poll_remote_transmit(Instant::now()), "{cause}");
        assert!(!e.tx_enabled(), "{cause}");
        assert!(e.take_slot_tx_abort(), "{cause}");
    }
}

#[test]
fn transmit_release_ends_the_permit_and_other_device_revoke_does_not() {
    let (f, now, state) = armed();
    f.authority.permit_station(OTHER, false).unwrap();
    assert!(!f.engine.lock().unwrap().poll_remote_transmit(now));
    f.authority
        .handle_version(
            (f.connection, 3),
            SESSION,
            DEVICE,
            &Request::Release {
                request_id: id(),
                lease_id: state["leaseId"].as_str().unwrap().into(),
            },
            &f.engine,
            now,
        )
        .unwrap();
    assert!(f.engine.lock().unwrap().poll_remote_transmit(now));
}

fn stop_request(f: &Fixture, state: &Value) -> Request {
    Request::StopTransmit {
        request_id: id(),
        station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        lease_id: state["leaseId"].as_str().unwrap().into(),
        transmit_epoch: format!("{:016x}", f.authority.transmit.generation()),
    }
}

#[test]
fn transmit_stop_does_not_wait_for_engine_or_a_pending_file_operation() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    let (f, now, state) = armed();
    let request = stop_request(&f, &state);
    let engine = f.engine.lock().unwrap();
    let _pending_file_operation = f.authority.core.lock().unwrap();
    let result = f
        .authority
        .handle_version((f.connection, 4), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    assert_eq!(result, json!({"stop":"accepted"}));
    assert!(
        engine.tx_enabled(),
        "the acceptance did not wait for the engine"
    );
    drop(engine);
    drop(_pending_file_operation);
    for _ in 0..200 {
        if !f.engine.lock().unwrap().tx_enabled() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut engine = f.engine.lock().unwrap();
    assert!(!engine.tx_enabled());
    assert!(engine.take_slot_tx_abort());
}

/// Stop TX is never refused (operator ruling, 2026-10-03): the same Stop delivered again after a
/// new CQ was armed stops that CQ too, although its token is from before the first Stop: the
/// station runs its stop (`transmit_stop::stop_station`) for the replay as for the first. The
/// CQ is the native Call CQ a streamed controller's click is. This replaced `transmit_stop_rejects_a_replay_after_a_new_remote_arm`, which pinned the
/// opposite (the replay refused `staleContext` and the new CQ left armed).
#[test]
fn a_replayed_stop_after_a_new_remote_arm_still_stops_it() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    let (f, now, state) = armed();
    let request = stop_request(&f, &state);
    assert_eq!(
        stop_v4(&f, DEVICE, &request, now),
        Ok(json!({"stop":"accepted"}))
    );
    {
        let mut e = f.engine.lock().unwrap();
        assert!(!e.tx_enabled(), "the first Stop left the CQ armed");
        e.take_immediate_retune();
        e.start_cq(None).unwrap();
        // Positive control: the new CQ is armed.
        assert!(e.tx_enabled());
    }
    assert_eq!(
        stop_v4(&f, DEVICE, &request, now),
        Ok(json!({"stop":"accepted"}))
    );
    let e = f.engine.lock().unwrap();
    assert!(!e.tx_enabled(), "the new CQ is still armed");
}

// ── Stop anything (operator decision 2026-09-14) ────────────────────────────────────────────────
// A browser holding station control stops ANY transmission at the station, however it started: a
// keyed mic, a tune carrier, the CW keyer, the voice keyer, RTTY, PSK, JTTY, SSTV or FT. The station runs
// every local stop verb (`transmit_stop::stop_station`). A browser without station control is
// refused and changes nothing. This replaces
// `transmit_stop_cannot_stop_a_new_local_transmission`, which pinned the opposite rule.

pub(crate) const LOCAL_TRANSMISSIONS: [&str; 9] = [
    "ptt", "tune", "cw", "voice", "rtty", "psk", "jtty", "sstv", "ft",
];

/// A controlling v4 browser, and nothing keyed.
fn controlling() -> (Fixture, Instant, Value) {
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.set_tx_enabled(false);
        e.take_immediate_retune();
        e.take_slot_tx_abort();
    }
    let now = Instant::now();
    acquire_controls_version(&f, now, 4);
    let state = control_state_version(&f, now, 4);
    assert_eq!(state["phase"], "controlling");
    (f, now, state)
}

/// Start a transmission the way the shack does, through the local verbs, never a remote permit.
pub(crate) fn key_locally(e: &mut tempo_app::engine::Engine, kind: &str) {
    match kind {
        "ptt" => {
            e.set_tx_enabled(true);
            e.set_ptt(true);
            assert!(e.manual_ptt(), "{kind}");
        }
        "tune" => {
            e.set_tune(true);
            assert!(e.tuning(), "{kind}");
        }
        "cw" => e.send_cw("CQ TEST"),
        "voice" => {
            e.set_tx_enabled(true);
            e.send_voice(vec![0.1; 1200]).unwrap();
        }
        "rtty" => {
            e.set_operating_mode("rtty", false);
            e.set_tx_enabled(true);
            e.rtty_send_text("CQ TEST").unwrap();
        }
        "psk" => {
            e.set_operating_mode("keyboard", false);
            e.set_tx_enabled(true);
            e.psk_send_text("CQ TEST").unwrap();
        }
        "jtty" => {
            e.set_operating_mode("jtty", false);
            e.set_frequency(14.090, "20m", "USB");
            e.set_tx_enabled(true);
            let plan = e.jtty_plan_send("CQ TEST", "", None).unwrap();
            let rendered = tempo_app::engine::jtty_tx::render(&plan).unwrap();
            e.jtty_commit(plan, rendered).unwrap();
        }
        "sstv" => {
            e.set_operating_mode("phone", false);
            e.set_tx_enabled(true);
            e.sstv_send(vec![0.1; 1200], "Robot 36".into()).unwrap();
        }
        // The FT sequencer armed at the shack (TX On); a slot over in flight is cut by the
        // one-shot slot abort, which every stop verb raises.
        "ft" => e.set_tx_enabled(true),
        _ => unreachable!(),
    }
    // Positive control: the local transmission really holds the transmitter (or, for FT, is armed).
    assert!(
        e.tx_owner().is_some() || (kind == "ft" && e.tx_enabled()),
        "{kind}: the local transmission must be live before the stop"
    );
    e.take_slot_tx_abort();
}

pub(crate) fn assert_stopped(e: &mut tempo_app::engine::Engine, kind: &str) {
    assert!(
        e.tx_owner().is_none(),
        "{kind}: nothing still owns the transmitter"
    );
    assert!(!e.manual_ptt(), "{kind}: the mic key is released");
    assert!(!e.tuning(), "{kind}: the tune carrier is down");
    // Exactly as the local Stop TX leaves it: disarmed, never re-armed.
    assert!(!e.tx_enabled(), "{kind}: the TX-enable latch is off");
    assert!(e.take_slot_tx_abort(), "{kind}: an over in flight is cut");
    match kind {
        "cw" => assert!(e.take_cw_abort(), "{kind}: the keyer is aborted"),
        "voice" => assert!(e.take_voice_abort(), "{kind}: playback is flushed"),
        "rtty" => assert!(e.take_rtty_abort(), "{kind}: the over is aborted"),
        "psk" => assert!(e.take_psk_abort(), "{kind}: the over is aborted"),
        "jtty" => assert!(e.take_jtty_abort(), "{kind}: what is on the air is cut"),
        "sstv" => assert!(e.take_sstv_abort(), "{kind}: the image is aborted"),
        _ => {}
    }
}

fn stop_v4(
    f: &Fixture,
    device: &str,
    request: &Request,
    now: Instant,
) -> Result<Value, &'static str> {
    f.authority
        .handle_version((f.connection, 4), SESSION, device, request, &f.engine, now)
}

#[test]
fn a_controlling_browser_stops_any_local_transmission() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    for kind in LOCAL_TRANSMISSIONS {
        let (f, now, state) = controlling();
        assert!(
            state["transmitEpoch"].is_string(),
            "station control alone holds the stop token"
        );
        key_locally(&mut f.engine.lock().unwrap(), kind);
        assert_eq!(
            stop_v4(&f, DEVICE, &stop_request(&f, &state), now),
            Ok(json!({"stop":"accepted"})),
            "{kind}"
        );
        assert_stopped(&mut f.engine.lock().unwrap(), kind);
    }
}

#[test]
fn a_browser_without_station_control_is_refused_and_stops_nothing() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    for scene in ["noLease", "controlRevoked", "otherDevice"] {
        let f = Fixture::new();
        let now = Instant::now();
        let state = match scene {
            "noLease" => {
                f.authority.permit_station(DEVICE, true).unwrap();
                control_state_version(&f, now, 4)
            }
            "controlRevoked" => {
                acquire_controls_version(&f, now, 4);
                let state = control_state_version(&f, now, 4);
                f.authority.permit_station(DEVICE, false).unwrap();
                state
            }
            "otherDevice" => {
                acquire_controls_version(&f, now, 4);
                control_state_version(&f, now, 4)
            }
            _ => unreachable!(),
        };
        key_locally(&mut f.engine.lock().unwrap(), "ptt");
        let request = Request::StopTransmit {
            request_id: id(),
            station_boot_id: state["stationBootId"].as_str().unwrap().into(),
            lease_id: state["leaseId"].as_str().map_or_else(id, Into::into),
            transmit_epoch: format!("{:016x}", f.authority.transmit.generation()),
        };
        let (device, expected) = match scene {
            "otherDevice" => (OTHER, "notController"),
            _ => (DEVICE, "localPermissionRequired"),
        };
        assert_eq!(stop_v4(&f, device, &request, now), Err(expected), "{scene}");
        {
            let e = f.engine.lock().unwrap();
            assert!(e.manual_ptt(), "{scene}: the local key is untouched");
            assert!(e.tx_enabled(), "{scene}: the latch is untouched");
        }
        // Positive control, EVERY scene: the refusal belongs to the browser, not to the station.
        // Give this device station control and a lease and the same keyed station stops at once —
        // so a refusal above is a real refusal, never a fixture that could not have stopped.
        f.authority.permit_station(DEVICE, true).unwrap();
        if control_state_version(&f, now, 4)["leaseId"].is_null() {
            acquire_controls_version(&f, now, 4);
        }
        let live = control_state_version(&f, now, 4);
        assert!(
            live["transmitEpoch"].is_string(),
            "{scene}: station control holds the stop token"
        );
        assert_eq!(
            stop_v4(&f, DEVICE, &stop_request(&f, &live), now),
            Ok(json!({"stop":"accepted"})),
            "{scene}"
        );
        assert_stopped(&mut f.engine.lock().unwrap(), "ptt");
    }
}

// ── The expired lease still stops (operator ruling, 2026-09-15) ─────────────────────────────────
// A browser that HELD station control stops the station even after its lease has run out. The
// failure this closes is a keyed rig and a Stop button that reported a refusal; an unnecessary
// unkey is the smaller harm. A browser that never held control is refused exactly as before, and
// nothing here can start, arm or re-arm anything.

#[test]
fn an_expired_lease_still_stops_and_starts_nothing() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
                          // `settled` is the ORDINARY case, not the exception: the browser polls state while it watches
                          // the station transmit, so by the time the operator presses Stop the station has already
                          // reconciled the expired lease away and taken the generation it was issued with.
    for settled in [false, true] {
        let (f, now, state) = controlling();
        key_locally(&mut f.engine.lock().unwrap(), "ptt");
        let expired = now + LEASE + Duration::from_secs(1);
        let target = if settled {
            let after = control_state_version(&f, expired, 4);
            assert_eq!(after["phase"], "available", "the lease really did run out");
            assert!(
                after["leaseId"].is_null(),
                "and it is no longer the controller"
            );
            // Re-issued to the browser that held control: the page sends a Stop only with a token
            // the station issued it. The token's age no longer matters (operator ruling,
            // 2026-10-03): see `a_delayed_stop_still_stops_what_is_on_the_air_now`.
            assert!(
                after["transmitEpoch"].is_string(),
                "the stop token stays with the browser that held control"
            );
            json!({"stationBootId":after["stationBootId"],"leaseId":state["leaseId"]})
        } else {
            state.clone()
        };
        assert_eq!(
            stop_v4(&f, DEVICE, &stop_request(&f, &target), expired),
            Ok(json!({"stop":"accepted"})),
            "settled={settled}"
        );
        // Nothing owns the transmitter and the TX-enable latch is off, exactly as a local Stop TX
        // leaves it. Stopping started nothing.
        assert_stopped(&mut f.engine.lock().unwrap(), "ptt");
        assert!(
            control_state_version(&f, expired, 4)["leaseId"].is_null(),
            "settled={settled}: the Stop did not hand the lease back"
        );
    }
}

#[test]
fn a_stop_token_ends_with_the_grant_and_with_any_lease_that_did_not_run_out() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    for scene in ["released", "revokedAfterExpiry", "takenOverAfterExpiry"] {
        let (f, now, state) = controlling();
        key_locally(&mut f.engine.lock().unwrap(), "ptt");
        let expired = now + LEASE + Duration::from_secs(1);
        let boot: String = state["stationBootId"].as_str().unwrap().into();
        let run = |session: &str, device: &str, request: &Request, at: Instant| {
            f.authority
                .handle_version((f.connection, 4), session, device, request, &f.engine, at)
        };
        match scene {
            // Given back, not run out: a browser that released control released the stop with it.
            "released" => {
                run(
                    SESSION,
                    DEVICE,
                    &Request::Release {
                        request_id: id(),
                        lease_id: state["leaseId"].as_str().unwrap().into(),
                    },
                    now,
                )
                .unwrap();
            }
            // Ran out, then the operator revoked station control at the radio.
            "revokedAfterExpiry" => {
                control_state_version(&f, expired, 4);
                f.authority.permit_station(DEVICE, false).unwrap();
            }
            // Ran out, then another browser took control: the token is the new controller's.
            "takenOverAfterExpiry" => {
                control_state_version(&f, expired, 4);
                f.authority.permit_station(OTHER, true).unwrap();
                run(
                    OTHER,
                    OTHER,
                    &Request::Acquire {
                        request_id: id(),
                        station_boot_id: boot.clone(),
                    },
                    expired,
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let at = if scene == "released" { now } else { expired };
        let expected = if scene == "takenOverAfterExpiry" {
            "notController"
        } else {
            "localPermissionRequired"
        };
        assert_eq!(
            run(SESSION, DEVICE, &stop_request(&f, &state), at),
            Err(expected),
            "{scene}"
        );
        {
            let e = f.engine.lock().unwrap();
            assert!(e.manual_ptt(), "{scene}: the local key is untouched");
            assert!(e.tx_enabled(), "{scene}: the latch is untouched");
        }
        // Positive control: the refusal is this browser's, not the station's. Whoever holds station
        // control now stops the same keyed station at once.
        let (session, device) = if scene == "takenOverAfterExpiry" {
            (OTHER, OTHER)
        } else {
            f.authority.permit_station(DEVICE, true).unwrap();
            run(
                SESSION,
                DEVICE,
                &Request::Acquire {
                    request_id: id(),
                    station_boot_id: boot.clone(),
                },
                at,
            )
            .unwrap();
            (SESSION, DEVICE)
        };
        let live = run(session, device, &Request::State { request_id: id() }, at).unwrap();
        assert!(
            live["transmitEpoch"].is_string(),
            "{scene}: the controller holds the stop token"
        );
        assert_eq!(
            run(session, device, &stop_request(&f, &live), at),
            Ok(json!({"stop":"accepted"})),
            "{scene}"
        );
        assert_stopped(&mut f.engine.lock().unwrap(), "ptt");
    }
}

// ── Stop TX is never refused (operator ruling, 2026-10-03) ─────────────────────────────────────
// "Stop TX always stops whatever is transmitting, even if it was pressed before the last halt."
// A Stop's token is the transmit generation its browser was last shown, and every halt at the
// station moves that generation, so refusing an older token refused fresh Stops for something the
// operator did not do. The token's age is no reason to refuse; WHO may stop is unchanged.

/// A Stop delivered late (a duplicated frame, or one carrying the token from before the last halt)
/// stops whatever is on the air when it lands. This replaced
/// `a_replayed_stop_transmit_has_no_further_effect`, which pinned the 2026-09-15 rule: the late
/// Stop refused `staleContext` and the new transmission left keyed. Authorization is not staleness:
/// the same late Stop from a browser that is not the controller is still refused and stops nothing.
#[test]
fn a_delayed_stop_still_stops_what_is_on_the_air_now() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    let (f, now, state) = controlling();
    key_locally(&mut f.engine.lock().unwrap(), "ptt");
    let request = stop_request(&f, &state);
    assert_eq!(
        stop_v4(&f, DEVICE, &request, now),
        Ok(json!({"stop":"accepted"}))
    );
    assert_stopped(&mut f.engine.lock().unwrap(), "ptt");
    // The shack keys up again, and the same Stop arrives a second time. Its token was retired by
    // its own first delivery, and the halt that ran then moved the generation again.
    key_locally(&mut f.engine.lock().unwrap(), "ptt");
    let Request::StopTransmit { transmit_epoch, .. } = &request else {
        unreachable!()
    };
    assert_ne!(
        *transmit_epoch,
        format!("{:016x}", f.authority.transmit.generation()),
        "premise: the token is not the current one"
    );
    // NEGATIVE CONTROL: from a browser that is not the controller, the late Stop is refused for
    // that, exactly as before, and the new transmission stays up.
    assert_eq!(stop_v4(&f, OTHER, &request, now), Err("notController"));
    assert!(
        f.engine.lock().unwrap().manual_ptt(),
        "a refused Stop stopped the new transmission"
    );
    // From the controller it stops the new transmission.
    assert_eq!(
        stop_v4(&f, DEVICE, &request, now),
        Ok(json!({"stop":"accepted"}))
    );
    assert_stopped(&mut f.engine.lock().unwrap(), "ptt");
}

#[test]
fn a_stop_is_accepted_at_once_while_the_engine_is_busy_and_stops_as_soon_as_it_is_free() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    let (f, now, state) = controlling();
    key_locally(&mut f.engine.lock().unwrap(), "ptt");
    let busy = f.engine.lock().unwrap();
    assert_eq!(
        stop_v4(&f, DEVICE, &stop_request(&f, &state), now),
        Ok(json!({"stop":"accepted"}))
    );
    assert!(
        busy.manual_ptt(),
        "the acceptance did not wait for the engine"
    );
    drop(busy);
    for _ in 0..200 {
        if !f.engine.lock().unwrap().manual_ptt() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_stopped(&mut f.engine.lock().unwrap(), "ptt");
}

#[test]
// The lease's IDENTITY is still required; its CLOCK is not. Expiry left this list on 2026-09-15 —
// see `an_expired_lease_still_stops_and_starts_nothing`.
fn transmit_stop_requires_the_exact_granted_lease_and_new_protocol() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    for scene in [
        "version",
        "device",
        "session",
        "lease",
        "boot",
        "grant",
        "connection",
    ] {
        let (f, now, state) = armed();
        let mut request = stop_request(&f, &state);
        let mut session = SESSION;
        let mut device = DEVICE;
        let mut version = 4;
        let expected = match scene {
            "version" => {
                version = 3;
                "stationUnsupported"
            }
            "device" => {
                device = OTHER;
                "notController"
            }
            "session" => {
                session = OTHER;
                "notController"
            }
            "lease" => {
                if let Request::StopTransmit { lease_id, .. } = &mut request {
                    *lease_id = id();
                }
                "notController"
            }
            "boot" => {
                if let Request::StopTransmit {
                    station_boot_id, ..
                } = &mut request
                {
                    *station_boot_id = id();
                }
                "staleStation"
            }
            // Station control is what Stop needs: revoking it refuses the Stop.
            "grant" => {
                f.authority.permit_station(DEVICE, false).unwrap();
                "localPermissionRequired"
            }
            "connection" => {
                f.authority.start_connection();
                "staleConnection"
            }
            _ => unreachable!(),
        };
        assert_eq!(
            f.authority.handle_version(
                (f.connection, version),
                session,
                device,
                &request,
                &f.engine,
                now
            ),
            Err(expected),
            "{scene}"
        );
        if !matches!(scene, "grant" | "connection") {
            assert!(
                !f.engine.lock().unwrap().poll_remote_transmit(now),
                "{scene}"
            );
            assert!(f.engine.lock().unwrap().tx_enabled(), "{scene}");
        }
    }
}

#[test]
fn transmit_stop_token_is_only_returned_to_the_granted_v4_owner() {
    let (f, now, _) = armed();
    for version in 1..=4 {
        let value = f
            .authority
            .handle_version(
                (f.connection, version),
                SESSION,
                DEVICE,
                &Request::State { request_id: id() },
                &f.engine,
                now,
            )
            .unwrap();
        if version < 4 {
            assert!(value.get("transmitEpoch").is_none());
        } else {
            assert_eq!(
                value["transmitEpoch"],
                format!("{:016x}", f.authority.transmit.generation())
            );
        }
    }
    let value = f
        .authority
        .handle_version(
            (f.connection, 4),
            OTHER,
            DEVICE,
            &Request::State { request_id: id() },
            &f.engine,
            now,
        )
        .unwrap();
    assert!(value["transmitEpoch"].is_null());
    let state = |f: &Fixture| {
        f.authority
            .handle_version(
                (f.connection, 4),
                SESSION,
                DEVICE,
                &Request::State { request_id: id() },
                &f.engine,
                now,
            )
            .unwrap()
    };
    assert_eq!(
        state(&f)["transmitEpoch"],
        format!("{:016x}", f.authority.transmit.generation())
    );
    // Revoking station control ends the lease, and the token with it.
    f.authority.permit_station(DEVICE, false).unwrap();
    assert!(state(&f)["transmitEpoch"].is_null());
}

/// A Stop must not leave the station fighting itself for Engine.
///
/// The satellite disarm briefly ran on a thread of its own, so the request the browser sends right
/// after pressing Stop — the State poll that repaints the rail, or a re-arm — could be refused
/// `stationBusy` by the Stop that preceded it. The Stop itself was never at risk (its admission
/// takes no Engine lock), which is exactly why nothing caught this: the failure lands on the NEXT
/// request. Looped because it is a race, and the count is MEASURED rather than guessed: against the
/// defect a single pass failed 33 of 40 runs but twenty passes only 8 of 10 (the outcome correlates
/// within a process, so iterations are not independent) — two hundred failed 20 of 20, and still
/// finish in hundredths of a second.
#[test]
fn transmit_stop_does_not_leave_the_station_busy_for_the_next_request() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    let (f, _, _) = controlling();
    for _ in 0..200 {
        let state = control_state_version(&f, Instant::now(), 4);
        assert_eq!(
            stop_v4(&f, DEVICE, &stop_request(&f, &state), Instant::now()).unwrap(),
            json!({"stop":"accepted"})
        );
        let after = f.authority.handle_version(
            (f.connection, 4),
            SESSION,
            DEVICE,
            &Request::State { request_id: id() },
            &f.engine,
            Instant::now(),
        );
        assert_ne!(
            after.err(),
            Some("stationBusy"),
            "the Stop's own work refused the request behind it"
        );
    }
}

// ── A stand-down AT THE SHACK retires the browser's transmit epoch ──────────────────────────────
// `d564adc5` made an FT gesture pressed during a confirming window HELD rather than refused, and
// the browser cancels a held gesture only on its OWN Stop or TX-off. Nothing at the shack moved
// the transmit generation: the desktop's Stop TX calls `Engine::halt_tx` and no more, so the epoch
// a held gesture carried still validated and `call_station_ctx` re-armed the rig up to a second and
// a half after the operator stood it down. Operator sign-off for this FT-gate change: 2026-09-17.
//
// The check is on `halt_tx` itself rather than on the Stop TX command, because every local stop
// funnels through that verb — the desktop button, WSJT-X's UDP HaltTx, a broker client. A fix
// hooked to the button alone would leave a logger's halt with the same hole.
#[test]
fn a_stand_down_at_the_shack_retires_a_held_browser_transmit_epoch() {
    let (f, now, state) = controlling();
    let epoch = state["transmitEpoch"]
        .as_str()
        .expect("a controlling browser is shown an epoch")
        .to_string();
    assert_eq!(
        u64::from_str_radix(&epoch, 16).unwrap(),
        f.authority.transmit.generation(),
        "the epoch a browser is shown is the transmit generation"
    );

    // The shack operator presses Stop TX. Not Remote's Stop — the desktop's own verb.
    f.engine.lock().unwrap().halt_tx();

    // The gesture the browser was holding now carries a generation that no longer exists, so the
    // permit it would be minted against must not be issued.
    assert_ne!(
        u64::from_str_radix(&epoch, 16).unwrap(),
        f.authority.transmit.generation(),
        "a local stand-down left the held gesture's transmit epoch still valid"
    );
    assert!(
        f.authority
            .transmit
            .permit_generation(u64::from_str_radix(&epoch, 16).unwrap(), now + LEASE)
            .is_none(),
        "a gesture held across the operator's own Stop TX was still admitted"
    );

    // CONTROL, so this cannot pass by refusing everything: the epoch the browser is shown AFTER
    // the stand-down still works, which is the deliberate re-arm the operator has to make.
    let fresh = f.authority.transmit.generation();
    assert!(
        f.authority
            .transmit
            .permit_generation(fresh, now + LEASE)
            .is_some(),
        "the browser could not re-arm after the stand-down"
    );
}

/// The device's own heartbeat, which re-asserts the engine's arm gate from its current state.
fn heartbeat(f: &Fixture, lease_id: &str, now: Instant) {
    f.authority
        .handle_version(
            (f.connection, 3),
            SESSION,
            DEVICE,
            &Request::Heartbeat {
                request_id: id(),
                lease_id: lease_id.into(),
            },
            &f.engine,
            now,
        )
        .unwrap();
}

/// Mark DEVICE as a sign-in-added device of the account (what the durable service list does).
fn signed_in(f: &Fixture) {
    f.authority
        .set_signin_devices(std::collections::BTreeSet::from([DEVICE.to_string()]));
}

/// ★ THE 2026-10-23 SIGN-IN DECISION: a device whose control came from a sign-in is refused
/// transmit while it controls, for its 15-minute window, yet keeps control and the receiver. The
/// station records the hold; the lease the device takes sets the engine's arm gate
/// (`apply_tx_hold`): the engine refuses an arm and nothing keys. A waiver — here an Approve at the
/// shack — reconciled on the next heartbeat, lets it transmit.
#[test]
fn a_sign_in_held_device_is_refused_transmit_while_it_controls() {
    let f = Fixture::new();
    f.engine.lock().unwrap().set_tx_enabled(false);
    let now = f.now();
    // The service lists it as sign-in-added, and the station recorded its 15-minute hold.
    signed_in(&f);
    f.authority
        .hold_transmit_until(DEVICE, now + Duration::from_secs(15 * 60));
    // It takes control with no shack press; the lease sets the engine's arm gate.
    let state = acquire_controls_version(&f, now, 3);
    {
        let mut e = f.engine.lock().unwrap();
        assert!(e.remote_transmit_held(), "the controlling device is held");
        e.set_tx_enabled(true);
        assert!(!e.tx_enabled(), "transmit arm refused while held");
    }
    // Approve at the shack waives it; the next heartbeat reconciles the engine gate.
    f.authority.waive_transmit(DEVICE);
    heartbeat(&f, state["leaseId"].as_str().unwrap(), now);
    let mut e = f.engine.lock().unwrap();
    assert!(!e.remote_transmit_held(), "the waiver lifted the hold");
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "control: it transmits once waived");
}

/// ★ FAIL-CLOSED: a device the service lists as sign-in-added but for which NO waiver record exists
/// is HELD from transmit while it controls — the default is held, never open.
#[test]
fn a_sign_in_device_with_no_record_is_held_fail_closed() {
    let f = Fixture::new();
    f.engine.lock().unwrap().set_tx_enabled(false);
    let now = f.now();
    // Listed as sign-in-added, but the station recorded no hold or waiver for it.
    signed_in(&f);
    let _ = acquire_controls_version(&f, now, 3);
    let mut e = f.engine.lock().unwrap();
    assert!(
        e.remote_transmit_held(),
        "a sign-in device with no record is held, not open"
    );
    e.set_tx_enabled(true);
    assert!(
        !e.tx_enabled(),
        "transmit arm refused with no waiver record"
    );
}

/// ★ A sign-in device with a positive WAIVED record transmits at once.
#[test]
fn a_waived_sign_in_device_may_transmit() {
    let f = Fixture::new();
    f.engine.lock().unwrap().set_tx_enabled(false);
    let now = f.now();
    signed_in(&f);
    f.authority.waive_transmit(DEVICE);
    let _ = acquire_controls_version(&f, now, 3);
    let mut e = f.engine.lock().unwrap();
    assert!(!e.remote_transmit_held(), "a waived device is not held");
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "it may arm when waived");
}

/// ★ A controller the LOADED service list CONFIRMS is not sign-in-added is never held, whatever
/// stale record it carries — the hold keys only to confirmed sign-in provenance.
#[test]
fn a_controller_confirmed_not_sign_in_is_never_held() {
    let f = Fixture::new();
    f.engine.lock().unwrap().set_tx_enabled(false);
    let now = f.now();
    // The list is LOADED (heard) and does not name DEVICE, even though a stale hold is recorded.
    f.authority
        .set_signin_devices(std::collections::BTreeSet::new());
    f.authority
        .hold_transmit_until(DEVICE, now + Duration::from_secs(15 * 60));
    let _ = acquire_controls_version(&f, now, 3);
    let mut e = f.engine.lock().unwrap();
    assert!(
        !e.remote_transmit_held(),
        "a controller the loaded list confirms is not sign-in-added is not held"
    );
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "it may arm");
}

/// ★ FAIL-CLOSED STARTUP: before the provenance list is heard (`None`, not an empty set), an
/// account-road controller is HELD — an empty/unknown list must never read as "not sign-in-added".
/// Once the list loads and confirms the device is not sign-in-added, it is freed.
#[test]
fn an_account_road_controller_is_held_until_the_list_loads() {
    let f = Fixture::new();
    f.engine.lock().unwrap().set_tx_enabled(false);
    let now = f.now();
    // Back to the pre-list state (as after a restart, before the first list sync).
    f.authority.forget_signin_devices();
    let state = acquire_controls_version(&f, now, 3);
    {
        let mut e = f.engine.lock().unwrap();
        assert!(
            e.remote_transmit_held(),
            "an account-road controller is held while the list is unknown"
        );
        e.set_tx_enabled(true);
        assert!(
            !e.tx_enabled(),
            "transmit arm refused before the list is heard"
        );
    }
    // The list loads and does not name this device: now it is confirmed not sign-in-added, and free.
    f.authority
        .set_signin_devices(std::collections::BTreeSet::new());
    heartbeat(&f, state["leaseId"].as_str().unwrap(), now);
    let mut e = f.engine.lock().unwrap();
    assert!(!e.remote_transmit_held(), "freed once the list confirms it");
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "it may arm once confirmed");
}

/// Confirm DEVICE's durable owner-notice record (what the service list's `notified` does).
fn notified(f: &Fixture) {
    f.authority
        .set_notified_devices(std::collections::BTreeSet::from([DEVICE.to_string()]));
}

/// ★ What the remote PC is told of its hold (`radio.txHold`, display only): held in its window,
/// the owner notice unconfirmed, no time is given, since only Allow to transmit lifts it then; once
/// the notice is confirmed, the time left in the window; with the provenance list unknown, held
/// but no time, since the window's end would not lift it; allowed at the shack, nothing. The
/// radio loop's tick carries it into the snapshot.
#[test]
fn the_hold_shown_to_the_remote_pc_gives_the_time_only_after_a_notice() {
    use tempo_app::dto::TxHold;
    let f = Fixture::new();
    f.wire_tx_hold();
    f.engine.lock().unwrap().set_tx_enabled(false);
    let now = f.now();
    signed_in(&f);
    f.authority
        .hold_transmit_until(DEVICE, now + Duration::from_secs(15 * 60));
    let _ = acquire_controls_version(&f, now, 3);
    let shown = || {
        let mut e = f.engine.lock().unwrap();
        e.poll_remote_transmit(now);
        (e.remote_transmit_held(), e.snapshot().radio.tx_hold)
    };
    let not_notified = TxHold {
        owner_notified: false,
        left_secs: None,
    };
    assert_eq!(shown(), (true, Some(not_notified)), "no notice: no time");

    notified(&f);
    let counting = TxHold {
        owner_notified: true,
        left_secs: Some(15 * 60),
    };
    assert_eq!(shown(), (true, Some(counting)), "notified: the time left");

    f.authority.forget_signin_devices();
    let unlisted = TxHold {
        owner_notified: true,
        left_secs: None,
    };
    assert_eq!(shown(), (true, Some(unlisted)), "list unknown: no time");

    f.authority.waive_transmit(DEVICE);
    assert_eq!(
        shown(),
        (false, None),
        "allowed: nothing held, nothing shown"
    );
}

/// ★ THE NOTIFICATION PRECONDITION (the 2026-10-23 sign-in decision, point 3): a sign-in device
/// whose owner-notice record the service has NOT confirmed stays HELD even once its 15-minute
/// window has elapsed. Only when the notice is confirmed does the elapsed window free it.
#[test]
fn a_held_device_stays_held_past_its_window_until_the_notice_is_confirmed() {
    let f = Fixture::new();
    f.engine.lock().unwrap().set_tx_enabled(false);
    let now = f.now();
    signed_in(&f);
    // A window already elapsed against the fixture clock (the real clock is an hour behind it).
    f.authority.hold_transmit_until(DEVICE, Instant::now());
    let state = acquire_controls_version(&f, now, 3);
    {
        let mut e = f.engine.lock().unwrap();
        assert!(
            e.remote_transmit_held(),
            "held past its window while the notice is unconfirmed"
        );
        e.set_tx_enabled(true);
        assert!(
            !e.tx_enabled(),
            "transmit arm refused until the notice is confirmed"
        );
    }
    // The service confirms the durable notice row; the next heartbeat lifts the elapsed hold.
    notified(&f);
    heartbeat(&f, state["leaseId"].as_str().unwrap(), now);
    let mut e = f.engine.lock().unwrap();
    assert!(
        !e.remote_transmit_held(),
        "control: the elapsed window frees it once the notice is confirmed"
    );
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "it transmits once confirmed and elapsed");
}

/// ★ A WAIVER KEPT ACROSS A RESTART WAITS FOR THE LIST: restored at launch, it frees the device only
/// once the service's list is heard and names it as a sign-in device; before that it is held, as
/// any account-road controller is. CONTROL: a waiver given in this run frees at once.
#[test]
fn a_restored_waiver_frees_only_once_the_list_is_heard() {
    let f = Fixture::new();
    f.engine.lock().unwrap().set_tx_enabled(false);
    let now = f.now();
    f.authority.forget_signin_devices();
    f.authority.restore_transmit_waivers([DEVICE.to_string()]);
    let state = acquire_controls_version(&f, now, 3);
    {
        let mut e = f.engine.lock().unwrap();
        assert!(e.remote_transmit_held(), "restored, list not heard: held");
        e.set_tx_enabled(true);
        assert!(!e.tx_enabled(), "transmit arm refused before the list");
    }
    signed_in(&f);
    heartbeat(&f, state["leaseId"].as_str().unwrap(), now);
    {
        let mut e = f.engine.lock().unwrap();
        assert!(!e.remote_transmit_held(), "the list names it: free");
        e.set_tx_enabled(true);
        assert!(e.tx_enabled(), "it may arm once the list is heard");
    }

    let control = Fixture::new();
    control.authority.forget_signin_devices();
    control.authority.waive_transmit(DEVICE);
    assert!(
        !control.authority.tx_held(DEVICE, control.now()),
        "CONTROL: a waiver given in this run frees at once"
    );
}

/// ★ A REVOKE ENDS A WAIVER: station control revoked and given back, a waived sign-in device (given
/// in this run or restored) is held again until it is waived again. CONTROL: given back with no
/// revoke between, it stays free.
#[test]
fn a_revoke_ends_a_waiver() {
    for (restored, revoke) in [(false, false), (false, true), (true, true)] {
        let f = Fixture::new();
        signed_in(&f);
        if restored {
            f.authority.restore_transmit_waivers([DEVICE.to_string()]);
        } else {
            f.authority.waive_transmit(DEVICE);
        }
        if revoke {
            f.authority.permit_station(DEVICE, false).unwrap();
        }
        f.authority.permit_station(DEVICE, true).unwrap();
        assert_eq!(
            f.authority.tx_held(DEVICE, f.now()),
            revoke,
            "restored {restored}, revoked {revoke}"
        );
    }
}

/// ★ NO OVER-HOLD OF A LOCAL CONTROLLER: a code-paired / LAN-enrolled controller (recorded in the
/// LAN book) is FREE even before the account list has loaded (`None`). Its own recorded provenance
/// clears it ahead of the fail-closed `None` arm — a normal local operator is never held in any
/// list state.
#[test]
fn a_lan_book_controller_is_free_even_before_the_list_loads() {
    let f = Fixture::new();
    f.engine.lock().unwrap().set_tx_enabled(false);
    let now = f.now();
    f.authority.forget_signin_devices(); // no account list heard yet
    f.authority
        .hold_lan_devices(Box::new(|| vec![DEVICE.to_string()])); // recorded on-network provenance
    let _ = acquire_controls_version(&f, now, 3);
    let mut e = f.engine.lock().unwrap();
    assert!(
        !e.remote_transmit_held(),
        "a LAN-book controller is free even with the list unknown"
    );
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "it may arm");
}

/// ★ A TUNNEL COMPUTER IS ASKED UNDER THE SERVICE'S ID FOR IT: the name the room's tunnel takes it
/// as is held exactly as that id is, and freed by that id's waiver; one standing for a device the
/// list names as not signed in is free. FAIL-CLOSED: a name of the shack's own whose service id is
/// not known is held, whatever the list says, on the tunnel's list or (just removed, its revoke not
/// yet queued) off it; a computer in the LAN book is not.
#[test]
fn a_tunnel_name_is_held_as_the_service_id_it_stands_for() {
    const TUNNEL: &str = "60000000-0000-8000-8000-000000000001";
    let f = Fixture::new();
    let now = f.now();
    signed_in(&f);
    f.authority
        .hold_tunnel_devices(Box::new(|| vec![TUNNEL.to_string()]));
    assert!(
        f.authority.tx_held(TUNNEL, now),
        "a tunnel name whose service id is not known"
    );
    let stands_for = |id: &str| {
        f.authority.set_tunnel_owners(BTreeMap::from([(
            TUNNEL.to_string(),
            BTreeSet::from([id.to_string()]),
        )]))
    };
    stands_for(OTHER);
    assert!(
        !f.authority.tx_held(TUNNEL, now),
        "CONTROL: one standing for a device not signed in"
    );
    stands_for(DEVICE);
    assert!(
        f.authority.tx_held(TUNNEL, now),
        "one standing for a sign-in device"
    );
    f.authority.waive_transmit(DEVICE);
    assert!(!f.authority.tx_held(TUNNEL, now), "its service id's waiver");

    // Removed: off the map, and (as the tunnel's list says next) off the tunnel.
    const OFF: &str = "60000000-0000-8000-8000-000000000002";
    const LAN: &str = "70000000-0000-8000-8000-000000000001";
    f.authority.set_tunnel_owners(BTreeMap::new());
    f.authority
        .hold_lan_devices(Box::new(|| vec![LAN.to_string()]));
    assert!(
        f.authority.tx_held(OFF, now),
        "a name of the shack's own no longer on the tunnel and not yet revoked"
    );
    assert!(
        !f.authority.tx_held(LAN, now),
        "CONTROL: one in the LAN book"
    );
}

/// A sign-in-held device in control, with the real radio-loop hold source wired (as `Service::start`
/// wires it) so a `poll_remote_transmit` tick re-derives the gate off the live lease. Asserts the
/// premise and the CONTROL: it is held while it controls, and a tick while it is STILL controlling
/// keeps it held — the local operator cannot arm, the hold never fails open. Returns the lease state
/// for the test to make the device DEPART.
fn held_controller(f: &Fixture, now: Instant) -> Value {
    f.wire_tx_hold();
    f.engine.lock().unwrap().set_tx_enabled(false);
    signed_in(f);
    f.authority
        .hold_transmit_until(DEVICE, now + Duration::from_secs(15 * 60));
    let state = acquire_controls_version(f, now, 3);
    let mut e = f.engine.lock().unwrap();
    assert!(e.remote_transmit_held(), "premise: held while it controls");
    // CONTROL: a tick while the held device is STILL controlling must keep the hold on.
    e.poll_remote_transmit(now);
    assert!(
        e.remote_transmit_held(),
        "a still-controlling held device stays held across a radio-loop tick"
    );
    e.set_tx_enabled(true);
    assert!(
        !e.tx_enabled(),
        "the local operator cannot arm while the held device controls"
    );
    e.set_tx_enabled(false);
    drop(e);
    state
}

/// The local operator can arm transmit at their own radio again, once the held controller is gone.
fn arms_locally(f: &Fixture) {
    let mut e = f.engine.lock().unwrap();
    assert!(
        !e.remote_transmit_held(),
        "the hold lifted once the held controller stopped controlling"
    );
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "the local operator arms at their own radio");
}

/// ★ B1 — REMOTE TURNED OFF. A held sign-in device is the controller; the operator turns Remote off
/// (`invalidate`, the single thing `Control::stop` does to the authority), which brings no further
/// request. The radio loop's next tick re-derives the gate off the now-cleared lease, and the local
/// operator can arm again. REDDENS before the fix: the gate stuck on and locked the local operator out.
#[test]
fn the_sign_in_hold_lifts_when_remote_is_turned_off() {
    let f = Fixture::new();
    let now = f.now();
    let _ = held_controller(&f, now);
    f.authority.invalidate();
    f.engine.lock().unwrap().poll_remote_transmit(now);
    arms_locally(&f);
}

/// ★ B1 — THE CONNECTION IS RETIRED. The held controller's socket drops (`retire_connection`) with
/// no further request; the next tick re-derives the gate off the ended lease.
#[test]
fn the_sign_in_hold_lifts_when_the_connection_is_retired() {
    let f = Fixture::new();
    let now = f.now();
    let _ = held_controller(&f, now);
    f.authority.retire_connection(f.connection);
    f.engine.lock().unwrap().poll_remote_transmit(now);
    arms_locally(&f);
}

/// ★ B1 — THE SESSION DISCONNECTS. The held controller's session goes (`disconnect_session`) with no
/// further request; the next tick re-derives the gate off the ended lease.
#[test]
fn the_sign_in_hold_lifts_when_the_session_disconnects() {
    let f = Fixture::new();
    let now = f.now();
    let _ = held_controller(&f, now);
    f.authority.disconnect_session(SESSION);
    f.engine.lock().unwrap().poll_remote_transmit(now);
    arms_locally(&f);
}

/// ★ B1 — THE CONTROLLER IS REPLACED (revoked at the shack). The held controller's station grant is
/// taken (`permit_station(.., false)`), which ends its lease but brings no request of its own; the
/// next tick re-derives the gate off the now-controllerless station.
#[test]
fn the_sign_in_hold_lifts_when_the_controller_is_revoked() {
    let f = Fixture::new();
    let now = f.now();
    let _ = held_controller(&f, now);
    f.authority.permit_station(DEVICE, false).unwrap();
    f.engine.lock().unwrap().poll_remote_transmit(now);
    arms_locally(&f);
}
