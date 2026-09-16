//! Exercise the real lease/heartbeat authority around native FT ownership.
//! Direct ownership tests and closed browser CQ/TX/Stop requests share native policy.
use super::*;

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
    f.authority.permit_transmit(DEVICE, true).unwrap();
    let permit = f.authority.transmit.permit(now + LEASE).unwrap();
    f.engine
        .lock()
        .unwrap()
        .start_remote_ft_cq(permit, None)
        .unwrap();
    (f, now, state)
}

fn heartbeat(
    f: &Fixture,
    state: &Value,
    session: &str,
    now: Instant,
) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, 3),
        session,
        DEVICE,
        &Request::Heartbeat {
            request_id: id(),
            lease_id: state["leaseId"].as_str().unwrap().into(),
        },
        &f.engine,
        now,
    )
}

#[test]
fn transmit_permission_is_separate_boot_scoped_and_never_arms() {
    let f = Fixture::new();
    assert_eq!(
        f.authority.permit_transmit(DEVICE, true),
        Err("localPermissionRequired")
    );
    f.authority.permit_station(DEVICE, true).unwrap();
    assert_eq!(f.authority.local_status()["transmitDevices"], json!([]));
    f.authority.permit_transmit(DEVICE, true).unwrap();
    assert_eq!(
        f.authority.local_status()["transmitDevices"],
        json!([DEVICE])
    );
    assert!(!f.engine.lock().unwrap().tx_enabled());
    f.authority.invalidate();
    assert_eq!(f.authority.local_status()["transmitDevices"], json!([]));
    assert_eq!(
        f.authority.permit_transmit(DEVICE, true),
        Err("localPermissionRequired")
    );
}

#[test]
fn transmit_heartbeat_renews_only_the_current_live_owner() {
    let (f, now, state) = armed();
    assert_eq!(
        heartbeat(&f, &state, OTHER, now + Duration::from_secs(1)),
        Err("notController")
    );
    heartbeat(&f, &state, SESSION, now + Duration::from_secs(2)).unwrap();
    let mut e = f.engine.lock().unwrap();
    assert!(!e.poll_remote_transmit(now + LEASE));
    assert!(e.tx_enabled());
    assert!(e.poll_remote_transmit(now + LEASE + Duration::from_secs(2)));
    assert!(!e.tx_enabled());
    drop(e);
    assert_eq!(
        heartbeat(&f, &state, SESSION, now + LEASE + Duration::from_secs(2)),
        Err("leaseExpired")
    );
}

#[test]
fn transmit_revocation_is_synchronous_even_while_engine_is_busy() {
    for cause in [
        "grant",
        "stationGrant",
        "loggingGrant",
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
            "grant" => f.authority.permit_transmit(DEVICE, false).unwrap(),
            "stationGrant" => {
                f.authority.permit_station(DEVICE, false).unwrap();
            }
            "loggingGrant" => {
                f.authority.permit(DEVICE, false).unwrap();
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
fn transmit_regrant_and_heartbeat_cannot_revive_old_tx_or_own_native_tx() {
    for local in [false, true] {
        let (f, now, state) = armed();
        if local {
            f.engine.lock().unwrap().set_tx_enabled(true);
        }
        f.authority.permit_transmit(DEVICE, false).unwrap();
        f.authority.permit_transmit(DEVICE, true).unwrap();
        heartbeat(&f, &state, SESSION, now + Duration::from_secs(1)).unwrap();
        let mut e = f.engine.lock().unwrap();
        assert_eq!(e.tx_enabled(), local);
        assert!(!e.poll_remote_transmit(now + Duration::from_secs(20)));
        assert_eq!(e.tx_enabled(), local);
    }
}

#[test]
fn transmit_release_ends_the_permit_and_other_device_revoke_does_not() {
    let (f, now, state) = armed();
    f.authority.permit_transmit(OTHER, false).unwrap();
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
    let (f, now, state) = armed();
    let request = stop_request(&f, &state);
    let mut engine = f.engine.lock().unwrap();
    let _pending_file_operation = f.authority.core.lock().unwrap();
    let result = f
        .authority
        .handle_version((f.connection, 4), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    assert_eq!(result, json!({"stop":"accepted"}));
    assert!(engine.poll_remote_transmit(now));
    assert!(!engine.tx_enabled());
    assert!(engine.take_slot_tx_abort());
}

#[test]
fn transmit_stop_rejects_a_replay_after_a_new_remote_arm() {
    let (f, now, state) = armed();
    let request = stop_request(&f, &state);
    f.authority
        .stop_transmit(f.connection, SESSION, DEVICE, &request, now)
        .unwrap();
    {
        let mut e = f.engine.lock().unwrap();
        assert!(e.poll_remote_transmit(now));
        e.take_immediate_retune();
        e.start_remote_ft_cq(f.authority.transmit.permit(now + LEASE).unwrap(), None)
            .unwrap();
    }
    assert_eq!(
        f.authority
            .stop_transmit(f.connection, SESSION, DEVICE, &request, now),
        Err("staleContext")
    );
    assert!(!f.engine.lock().unwrap().poll_remote_transmit(now));
    assert!(f.engine.lock().unwrap().tx_enabled());
    f.authority
        .stop_transmit(
            f.connection,
            SESSION,
            DEVICE,
            &stop_request(&f, &state),
            now,
        )
        .unwrap();
    assert!(f.engine.lock().unwrap().poll_remote_transmit(now));
}

// ── Stop anything (operator decision 2026-09-14) ────────────────────────────────────────────────
// A browser holding station control stops ANY transmission at the station, however it started: a
// keyed mic, a tune carrier, the CW keyer, the voice keyer, RTTY, PSK, SSTV or FT. The station runs
// every local stop verb (`transmit_stop::stop_station`). Transmit permission is needed only to START;
// a browser without station control is refused and changes nothing. This replaces
// `transmit_stop_cannot_stop_a_new_local_transmission`, which pinned the opposite rule.

const LOCAL_TRANSMISSIONS: [&str; 8] = ["ptt", "tune", "cw", "voice", "rtty", "psk", "sstv", "ft"];

/// A controlling v4 browser, with or without FT8/FT4 transmit permission, and nothing keyed.
fn controlling(transmit: bool) -> (Fixture, Instant, Value) {
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
    if transmit {
        f.authority.permit_transmit(DEVICE, true).unwrap();
    }
    let state = control_state_version(&f, now, 4);
    assert_eq!(state["phase"], "controlling");
    (f, now, state)
}

/// Start a transmission the way the shack does, through the local verbs, never a remote permit.
fn key_locally(e: &mut tempo_app::engine::Engine, kind: &str) {
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
            e.send_voice(vec![0.1; 1200]);
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

fn assert_stopped(e: &mut tempo_app::engine::Engine, kind: &str) {
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
fn a_controlling_browser_stops_any_local_transmission_with_or_without_transmit_permission() {
    for transmit in [false, true] {
        for kind in LOCAL_TRANSMISSIONS {
            let (f, now, state) = controlling(transmit);
            assert!(
                state["transmitEpoch"].is_string(),
                "station control alone holds the stop token (transmit={transmit})"
            );
            key_locally(&mut f.engine.lock().unwrap(), kind);
            assert_eq!(
                stop_v4(&f, DEVICE, &stop_request(&f, &state), now),
                Ok(json!({"stop":"accepted"})),
                "{kind} transmit={transmit}"
            );
            assert_stopped(&mut f.engine.lock().unwrap(), kind);
        }
    }
}

#[test]
fn a_browser_without_station_control_is_refused_and_stops_nothing() {
    for scene in ["noLease", "loggingOnly", "controlRevoked", "otherDevice"] {
        let f = Fixture::new();
        let now = Instant::now();
        let state = match scene {
            "noLease" => {
                f.authority.permit_station(DEVICE, true).unwrap();
                control_state_version(&f, now, 4)
            }
            "loggingOnly" => {
                f.authority.permit(DEVICE, true).unwrap();
                let boot = control_state_version(&f, now, 4)["stationBootId"].clone();
                f.authority
                    .handle_version(
                        (f.connection, 4),
                        SESSION,
                        DEVICE,
                        &Request::Acquire {
                            request_id: id(),
                            station_boot_id: boot.as_str().unwrap().into(),
                        },
                        &f.engine,
                        now,
                    )
                    .unwrap()
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
        if scene == "loggingOnly" {
            assert_eq!(
                state["phase"], "controlling",
                "positive control: a logging lease is held"
            );
            assert!(
                state["transmitEpoch"].is_null(),
                "logging alone holds no stop token"
            );
        }
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
    // `settled` is the ORDINARY case, not the exception: the browser polls state while it watches
    // the station transmit, so by the time the operator presses Stop the station has already
    // reconciled the expired lease away and taken the generation it was issued with.
    for settled in [false, true] {
        let (f, now, state) = controlling(false);
        key_locally(&mut f.engine.lock().unwrap(), "ptt");
        let expired = now + LEASE + Duration::from_secs(1);
        let target = if settled {
            let after = control_state_version(&f, expired, 4);
            assert_eq!(after["phase"], "available", "the lease really did run out");
            assert!(
                after["leaseId"].is_null(),
                "and it is no longer the controller"
            );
            // Re-issued to the browser that held control. Without a CURRENT token its Stop would be
            // refused `staleContext` — the expiry's own revocation retired the old one — and the
            // ruling would never reach the rig. Replay safety is untouched: see
            // `a_replayed_stop_transmit_has_no_further_effect`.
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
    for scene in ["released", "revokedAfterExpiry", "takenOverAfterExpiry"] {
        let (f, now, state) = controlling(false);
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

#[test]
fn a_replayed_stop_transmit_has_no_further_effect() {
    let (f, now, state) = controlling(false);
    key_locally(&mut f.engine.lock().unwrap(), "ptt");
    let request = stop_request(&f, &state);
    assert_eq!(
        stop_v4(&f, DEVICE, &request, now),
        Ok(json!({"stop":"accepted"}))
    );
    assert_stopped(&mut f.engine.lock().unwrap(), "ptt");
    // The shack keys up again; the same Stop delivered twice (a duplicated or delayed frame) is
    // refused and leaves the new transmission alone. It retires only the token it displayed.
    key_locally(&mut f.engine.lock().unwrap(), "ptt");
    assert_eq!(stop_v4(&f, DEVICE, &request, now), Err("staleContext"));
    assert!(f.engine.lock().unwrap().manual_ptt());
    // Positive control: a Stop against the current token does stop it.
    assert_eq!(
        stop_v4(&f, DEVICE, &stop_request(&f, &state), now),
        Ok(json!({"stop":"accepted"}))
    );
    assert_stopped(&mut f.engine.lock().unwrap(), "ptt");
}

#[test]
fn a_stop_is_accepted_at_once_while_the_engine_is_busy_and_stops_as_soon_as_it_is_free() {
    let (f, now, state) = controlling(false);
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
            // Station control is what Stop needs (transmit permission only starts): revoking it
            // refuses the Stop.
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
    // Revoking FT8/FT4 transmit keeps the stop token: station control alone may stop.
    f.authority.permit_transmit(DEVICE, false).unwrap();
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

fn ready_ft(tier: tempo_app::dto::Tier) -> (Fixture, Instant, Value) {
    ready_ft_link(tier, true)
}

fn ready_ft_link(tier: tempo_app::dto::Tier, connected: bool) -> (Fixture, Instant, Value) {
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        e.set_tier(tier);
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.set_tx_enabled(false);
        e.take_immediate_retune();
        e.take_slot_tx_abort();
        let radio = e.remote_open_radio().unwrap();
        let read = e.remote_radio_read(&radio, Instant::now());
        let hz = (e.settings().dial_mhz * 1e6).round() as u64;
        e.remote_observe_cat(read.as_ref(), Some(connected));
        e.remote_observe_dial(read.as_ref(), Some(hz));
        e.remote_observe_mode(read.as_ref(), Some("PKTUSB"));
        e.remote_observe_ptt(read.as_ref(), Some(false));
    }
    let now = Instant::now();
    acquire_controls_version(&f, now, 4);
    f.authority.permit_transmit(DEVICE, true).unwrap();
    let state = control_state_version(&f, now, 4);
    (f, now, state)
}
fn ft_command(state: &Value, action: Value) -> Request {
    serde_json::from_value(json!({"type":"stationControl","requestId":id(),"stationBootId":state["stationBootId"],
        "leaseId":state["leaseId"],"expectedRevision":state["revision"],"commandWindowId":state["commandWindowId"],
        "clientSequence":state["nextSequence"],"context":state["controls"]["context"],"action":action})).unwrap()
}
fn ft_run(f: &Fixture, request: &Request) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, 4),
        SESSION,
        DEVICE,
        request,
        &f.engine,
        Instant::now(),
    )
}

/// One approval (operator decision 2026-09-13): a transmit grant can now come back when Remote comes
/// on after a restart. Coming back arms nothing. Transmit without station control is not restored at
/// all. With it, the browser is offered FT operating, but the TX-enable latch stays off, taking the
/// lease and heartbeating arm nothing, and only the browser's own TX On does.
#[test]
fn a_restored_transmit_grant_arms_nothing_until_the_browser_presses_tx_on() {
    use tempo_app::dto::Tier;
    for tier in [Tier::Ft8, Tier::Ft4] {
        let f = Fixture::new();
        {
            let mut e = f.engine.lock().unwrap();
            e.set_tier(tier);
            e.configure_remote_settings_store(f.dir.join("settings.json"));
            e.set_tx_enabled(false);
            e.take_immediate_retune();
            e.take_slot_tx_abort();
            let radio = e.remote_open_radio().unwrap();
            let read = e.remote_radio_read(&radio, Instant::now());
            let hz = (e.settings().dial_mhz * 1e6).round() as u64;
            e.remote_observe_cat(read.as_ref(), Some(true));
            e.remote_observe_dial(read.as_ref(), Some(hz));
            e.remote_observe_mode(read.as_ref(), Some("PKTUSB"));
            e.remote_observe_ptt(read.as_ref(), Some(false));
        }
        let epoch = f.authority.epoch();
        let transmit_only = DurableGrants {
            transmit: vec![DEVICE.into()],
            ..Default::default()
        };
        assert_eq!(f.authority.restore(epoch, &transmit_only), Ok(true));
        assert_eq!(
            f.authority.local_status()["transmitDevices"],
            json!([]),
            "never restored without station control"
        );
        let grants = DurableGrants {
            control: vec![DEVICE.into()],
            transmit: vec![DEVICE.into()],
            ..Default::default()
        };
        assert_eq!(f.authority.restore(epoch, &grants), Ok(true));
        assert_eq!(
            f.authority.local_status()["transmitDevices"],
            json!([DEVICE])
        );
        assert!(
            !f.engine.lock().unwrap().tx_enabled(),
            "restoring leaves the latch off"
        );

        let now = Instant::now();
        let boot = control_state_version(&f, now, 4)["stationBootId"]
            .as_str()
            .unwrap()
            .to_string();
        let owner = f
            .authority
            .handle_version(
                (f.connection, 4),
                SESSION,
                DEVICE,
                &Request::Acquire {
                    request_id: id(),
                    station_boot_id: boot,
                },
                &f.engine,
                now,
            )
            .unwrap();
        f.authority
            .handle_version(
                (f.connection, 4),
                SESSION,
                DEVICE,
                &Request::Heartbeat {
                    request_id: id(),
                    lease_id: owner["leaseId"].as_str().unwrap().into(),
                },
                &f.engine,
                now,
            )
            .unwrap();
        let state = control_state_version(&f, now, 4);
        assert!(
            state["controls"]["capabilities"]
                .as_array()
                .unwrap()
                .contains(&json!("ftOperate")),
            "positive control: the restored grant is real"
        );
        assert_eq!(state["txArmed"], false);
        {
            let e = f.engine.lock().unwrap();
            assert!(
                !e.tx_enabled() && !e.remote_ft_tx_owned(),
                "a lease and a heartbeat arm nothing"
            );
        }
        let tx_on = ft_command(
            &state,
            json!({"action":"ft.txEnabled","expectedTier":tier,"transmitEpoch":state["transmitEpoch"],"on":true}),
        );
        assert_eq!(ft_run(&f, &tx_on).unwrap()["outcome"], "applied");
        assert!(
            f.engine.lock().unwrap().tx_enabled(),
            "positive control: the browser's TX On is what arms it"
        );
    }
}

#[test]
fn transmit_message_choice_uses_the_original_exchange_and_native_target() {
    use tempo_app::engine::remote_transmit::FtExchangeContext;
    for tier in [tempo_app::dto::Tier::Ft8, tempo_app::dto::Tier::Ft4] {
        let (f, _, state) = ready_ft(tier);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("ftMessages")));
        let cq = ft_command(
            &state,
            json!({"action":"ft.cq","expectedTier":tier,"transmitEpoch":state["transmitEpoch"],"direction":null}),
        );
        assert_eq!(ft_run(&f, &cq).unwrap()["outcome"], "applied");
        f.engine.lock().unwrap().take_immediate_retune();
        let expected = FtExchangeContext::from(&f.engine.lock().unwrap().snapshot().qso.unwrap());
        let current = control_state_version(&f, Instant::now(), 4);
        let command = ft_command(
            &current,
            json!({"action":"ft.message","expectedTier":tier,
            "transmitEpoch":current["transmitEpoch"],"expectedQso":expected,
            "call":"W1AW","grid":"FN31","text":"W1AW KD9TAW -12"}),
        );
        assert_eq!(ft_run(&f, &command).unwrap()["outcome"], "applied");
        let qso = f.engine.lock().unwrap().snapshot().qso.unwrap();
        assert_eq!(qso.dxcall.as_deref(), Some("W1AW"));
        assert_eq!(qso.tx_now.as_deref(), Some("W1AW KD9TAW -12"));
        let current = control_state_version(&f, Instant::now(), 4);
        let stale = ft_command(
            &current,
            json!({"action":"ft.message","expectedTier":tier,
            "transmitEpoch":current["transmitEpoch"],"expectedQso":expected,
            "call":"K2ABC","grid":null,"text":"K2ABC KD9TAW -10"}),
        );
        assert_eq!(ft_run(&f, &stale).unwrap()["outcome"], "rejected");
        assert_eq!(f.engine.lock().unwrap().snapshot().qso.unwrap(), qso);
    }
}

#[test]
fn transmit_exchange_controls_reuse_native_policy_and_reject_an_advanced_display() {
    use tempo_app::engine::remote_transmit::FtExchangeContext;
    for tier in [tempo_app::dto::Tier::Ft8, tempo_app::dto::Tier::Ft4] {
        let (f, _, state) = ready_ft(tier);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("ftExchange")));
        let cq = ft_command(
            &state,
            json!({"action":"ft.cq","expectedTier":tier,"transmitEpoch":state["transmitEpoch"],"direction":null}),
        );
        assert_eq!(ft_run(&f, &cq).unwrap()["outcome"], "applied");
        f.engine.lock().unwrap().take_immediate_retune();
        let original = FtExchangeContext::from(&f.engine.lock().unwrap().snapshot().qso.unwrap());
        for change in [
            json!({"kind":"freeText","text":"TNX 73"}),
            json!({"kind":"resend"}),
            json!({"kind":"monitor"}),
        ] {
            let expected =
                FtExchangeContext::from(&f.engine.lock().unwrap().snapshot().qso.unwrap());
            let current = control_state_version(&f, Instant::now(), 4);
            let command = ft_command(
                &current,
                json!({"action":"ft.exchange","expectedTier":tier,"transmitEpoch":current["transmitEpoch"],"expectedQso":expected,"change":change}),
            );
            assert_eq!(ft_run(&f, &command).unwrap()["outcome"], "applied");
            assert!(
                f.engine.lock().unwrap().tx_enabled(),
                "native S&P does not disarm the TX latch"
            );
        }
        let current = control_state_version(&f, Instant::now(), 4);
        let stale = ft_command(
            &current,
            json!({"action":"ft.exchange","expectedTier":tier,"transmitEpoch":current["transmitEpoch"],"expectedQso":original,"change":{"kind":"resend"}}),
        );
        assert_eq!(ft_run(&f, &stale).unwrap()["outcome"], "rejected");
    }
}

#[test]
fn transmit_browser_cq_tx_off_and_stop_preserve_native_ft_behavior() {
    use tempo_app::dto::Tier;
    for tier in [Tier::Ft8, Tier::Ft4] {
        let (f, _, state) = ready_ft(tier);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("ftOperate")));
        let command = ft_command(
            &state,
            json!({"action":"ft.cq","expectedTier":tier,"transmitEpoch":state["transmitEpoch"],"direction":"DX"}),
        );
        assert_eq!(ft_run(&f, &command).unwrap()["evidence"], "stationState");
        assert!(f.engine.lock().unwrap().tx_enabled());
        let armed = control_state_version(&f, Instant::now(), 4);
        assert_eq!(armed["txArmed"], true);
        let off = ft_command(
            &armed,
            json!({"action":"ft.txEnabled","expectedTier":tier,"transmitEpoch":armed["transmitEpoch"],"on":false}),
        );
        assert_eq!(ft_run(&f, &off).unwrap()["outcome"], "applied");
        assert!(!f.engine.lock().unwrap().take_slot_tx_abort());
        assert!(!f.engine.lock().unwrap().tx_enabled());
        let stop = stop_request(&f, &state);
        assert_eq!(ft_run(&f, &stop).unwrap(), json!({"stop":"accepted"}));
        // Stop anything (2026-09-14): the station halts at once rather than leaving the halt to
        // the radio loop's permit poll, so the poll finds nothing left to stop.
        let mut e = f.engine.lock().unwrap();
        assert!(!e.remote_ft_tx_owned());
        assert!(!e.poll_remote_transmit(Instant::now()));
        assert!(!e.tx_enabled());
        assert!(e.take_slot_tx_abort());
    }
}

#[test]
fn transmit_stop_invalidates_an_unsent_arm_even_if_its_command_window_still_matches() {
    let (f, _, state) = ready_ft(tempo_app::dto::Tier::Ft8);
    let command = ft_command(
        &state,
        json!({"action":"ft.cq","expectedTier":"FT8","transmitEpoch":state["transmitEpoch"],"direction":null}),
    );
    assert_eq!(
        ft_run(&f, &stop_request(&f, &state)).unwrap(),
        json!({"stop":"accepted"})
    );
    assert_eq!(ft_run(&f, &command), Err("staleContext"));
    assert!(!f.engine.lock().unwrap().tx_enabled());
    let fresh = control_state_version(&f, Instant::now(), 4);
    let command = ft_command(
        &fresh,
        json!({"action":"ft.cq","expectedTier":"FT8","transmitEpoch":fresh["transmitEpoch"],"direction":null}),
    );
    assert_eq!(ft_run(&f, &command).unwrap()["outcome"], "applied");
    assert!(f.engine.lock().unwrap().tx_enabled());
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
    let (f, _, _) = ready_ft(tempo_app::dto::Tier::Ft8);
    for _ in 0..200 {
        let state = control_state_version(&f, Instant::now(), 4);
        assert_eq!(
            ft_run(&f, &stop_request(&f, &state)).unwrap(),
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

#[test]
fn transmit_browser_arm_requires_its_own_grant_and_fresh_radio_context() {
    for cause in [
        "permission",
        "link",
        "tier",
        "generation",
        "direction",
        "local",
    ] {
        let (f, _, state) = ready_ft_link(tempo_app::dto::Tier::Ft8, cause != "link");
        let mut action = json!({"action":"ft.cq","expectedTier":"FT8","transmitEpoch":state["transmitEpoch"],"direction":null});
        match cause {
            "permission" => f.authority.permit_transmit(DEVICE, false).unwrap(),
            "link" => {}
            "tier" => action["expectedTier"] = json!("FT4"),
            "generation" => action["transmitEpoch"] = json!("ffffffffffffffff"),
            "direction" => action["direction"] = json!("CQ DX W1AW"),
            "local" => f.engine.lock().unwrap().set_tx_enabled(true),
            _ => unreachable!(),
        }
        let result = ft_run(&f, &ft_command(&state, action));
        assert!(
            result.is_err() || result.unwrap()["outcome"] == "rejected",
            "{cause}"
        );
        assert_eq!(
            f.engine.lock().unwrap().tx_enabled(),
            cause == "local",
            "{cause}"
        );
    }
}

#[test]
fn transmit_local_revocation_does_not_wait_for_a_pending_durable_write() {
    let (f, now, _) = armed();
    let core = f.authority.core.lock().unwrap();
    let result = f.authority.permit_transmit(DEVICE, false);
    assert_eq!(
        result,
        Ok(()),
        "a local revoke must not wait for a file sync"
    );
    assert!(f.engine.lock().unwrap().poll_remote_transmit(now));
    drop(core);
    let state = control_state_version(&f, Instant::now(), 4);
    // The stop token stays (station control alone may stop); starting FT does not.
    assert!(state["transmitEpoch"].is_string());
    assert!(!state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("ftOperate")));
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

#[test]
fn ft_preferences_require_transmit_grant_and_match_durable_native_state() {
    for tier in [tempo_app::dto::Tier::Ft8, tempo_app::dto::Tier::Ft4] {
        for change in [
            json!({"kind":"txOffset","hz":1800}),
            json!({"kind":"bothOffsets","hz":2200}),
            json!({"kind":"hold","on":true}),
            json!({"kind":"even","even":false}),
            json!({"kind":"auto","auto":false}),
        ] {
            let (f, _, state) = ready_ft(tier);
            assert!(state["controls"]["capabilities"]
                .as_array()
                .unwrap()
                .contains(&json!("ftSettings")));
            let expected = f.engine.lock().unwrap().remote_ft_settings().unwrap();
            let request = ft_command(
                &state,
                json!({"action":"ft.setting","expectedTier":tier,
                "transmitEpoch":state["transmitEpoch"],"expected":expected,"change":change}),
            );
            for version in [2, 3] {
                assert_eq!(
                    f.authority.handle_version(
                        (f.connection, version),
                        SESSION,
                        DEVICE,
                        &request,
                        &f.engine,
                        Instant::now()
                    ),
                    Err("stationUnsupported")
                );
            }
            let response = ft_run(&f, &request).unwrap();
            assert_eq!(response["outcome"], "applied", "{response}");
            assert_eq!(
                response["evidence"],
                if change["kind"] == "auto" {
                    "stationState"
                } else {
                    "settingsSaved"
                }
            );
            assert!(!f.engine.lock().unwrap().tx_enabled());
            let after = f.engine.lock().unwrap().remote_ft_settings().unwrap();
            assert_ne!(expected.key, after.key);
            match change["kind"].as_str().unwrap() {
                "txOffset" => assert_eq!(after.tx_offset_hz, 1800.0),
                "bothOffsets" => {
                    assert_eq!(after.tx_offset_hz, 2200.0);
                    assert_eq!(after.rx_offset_hz, 2200.0);
                }
                "hold" => assert!(after.hold_tx_freq),
                "even" => {
                    assert!(!after.tx_even);
                    assert!(!after.tx_cycle_auto);
                }
                "auto" => assert!(!after.tx_cycle_auto),
                _ => unreachable!(),
            }
            assert_eq!(ft_run(&f, &request).unwrap(), response);
            assert_eq!(f.engine.lock().unwrap().remote_ft_settings(), Some(after));
        }
    }
}

#[test]
fn ft_preferences_refuse_lost_grant_changed_context_and_unavailable_radio() {
    for cause in ["grant", "context", "radio"] {
        let (f, _, state) = ready_ft_link(tempo_app::dto::Tier::Ft8, cause != "radio");
        let expected = f.engine.lock().unwrap().remote_ft_settings().unwrap();
        let request = ft_command(
            &state,
            json!({"action":"ft.setting","expectedTier":"FT8","transmitEpoch":state["transmitEpoch"],
            "expected":expected,"change":{"kind":"txOffset","hz":1800}}),
        );
        if cause == "grant" {
            f.authority.permit_transmit(DEVICE, false).unwrap();
        }
        if cause == "context" {
            let mut e = f.engine.lock().unwrap();
            e.set_hold_tx_freq(!expected.hold_tx_freq);
            e.set_hold_tx_freq(expected.hold_tx_freq);
        }
        let before = f.engine.lock().unwrap().settings().clone();
        let result = ft_run(&f, &request);
        if cause == "grant" {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap()["outcome"], "rejected");
        }
        assert_eq!(f.engine.lock().unwrap().settings(), &before);
        assert!(!f.dir.join("settings.json").exists());
    }
}

#[test]
fn ft_runtime_rx_and_skip_work_through_the_current_lease_without_owning_local_tx() {
    for tier in [tempo_app::dto::Tier::Ft8, tempo_app::dto::Tier::Ft4] {
        for armed in [false, true] {
            let (f, _, state) = ready_ft(tier);
            assert!(state["controls"]["capabilities"]
                .as_array()
                .unwrap()
                .contains(&json!("ftRuntime")));
            if armed {
                let request = ft_command(
                    &state,
                    json!({"action":"ft.cq","expectedTier":tier,"transmitEpoch":state["transmitEpoch"],"direction":null}),
                );
                assert_eq!(ft_run(&f, &request).unwrap()["outcome"], "applied");
                f.engine.lock().unwrap().take_immediate_retune();
            }
            for change in [
                json!({"kind":"rxOffset","hz":900}),
                json!({"kind":"skipTx1","on":true}),
            ] {
                let state = control_state_version(&f, Instant::now(), 4);
                let expected = f.engine.lock().unwrap().remote_ft_runtime().unwrap();
                let request = ft_command(
                    &state,
                    json!({"action":"ft.runtime","expectedTier":tier,"transmitEpoch":state["transmitEpoch"],"expected":expected,"change":change}),
                );
                let response = ft_run(&f, &request).unwrap();
                assert_eq!(response["outcome"], "applied", "{response}");
                assert_eq!(
                    response["evidence"],
                    if change["kind"] == "rxOffset" {
                        "settingsSaved"
                    } else {
                        "stationState"
                    }
                );
                let after = f.engine.lock().unwrap().remote_ft_runtime().unwrap();
                assert_eq!(after.settings.tx_offset_hz, expected.settings.tx_offset_hz);
                assert_eq!(after.settings.rx_offset_hz, 900.0);
                if change["kind"] == "skipTx1" {
                    assert!(after.skip_tx1);
                }
                assert_eq!(f.engine.lock().unwrap().tx_enabled(), armed);
                assert_eq!(ft_run(&f, &request).unwrap(), response);
                assert_eq!(f.engine.lock().unwrap().remote_ft_runtime(), Some(after));
            }
        }
    }
}

#[test]
fn ft_runtime_refuses_local_skip_change_or_revoked_transmit_grant() {
    for revoked in [false, true] {
        let (f, _, state) = ready_ft(tempo_app::dto::Tier::Ft8);
        let expected = f.engine.lock().unwrap().remote_ft_runtime().unwrap();
        let request = ft_command(
            &state,
            json!({"action":"ft.runtime","expectedTier":"FT8","transmitEpoch":state["transmitEpoch"],"expected":expected,"change":{"kind":"skipTx1","on":true}}),
        );
        if revoked {
            f.authority.permit_transmit(DEVICE, false).unwrap();
        } else {
            let mut e = f.engine.lock().unwrap();
            e.set_skip_tx1(true);
            e.set_skip_tx1(false);
        }
        let result = ft_run(&f, &request);
        if revoked {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap()["outcome"], "rejected");
        }
        assert!(!f.engine.lock().unwrap().skip_tx1());
        assert!(!f.dir.join("settings.json").exists());
    }
}
