use super::*;
use tempo_app::{dto::AmpStatusDto, settings::Settings};

fn station(follow: bool) -> Fixture {
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.amp_model = "spe".into();
        s.amp_port = "remote-follow-fixture".into();
        s.amp_follow_band = follow;
        s.sync_active_from_flat();
        let mut other = s.active_profile().unwrap().clone();
        other.id = 8;
        other.name = "Other radio".into();
        other.amp_port = "other-fixture".into();
        other.amp_follow_band = !follow;
        s.radios.push(other);
        s.ensure_distinct_radio_ports();
        s.sync_flat_from_active();
        // Start with the normal native loader's canonical configuration. It
        // seeds a default CW macro profile and resolves legacy satellite routes;
        // those migrations are independent of the follow-setting transaction.
        s.save(&f.dir.join("settings.json")).unwrap();
        *e = tempo_app::engine::Engine::with_settings(Settings::load(&f.dir.join("settings.json")));
        e.set_log_path(f.dir.join("contacts.adi"));
        assert_eq!(e.settings().radios.len(), 2);
        e.set_tx_enabled(false);
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.settings().save(&f.dir.join("settings.json")).unwrap();
    }
    sample(&f, Some(false), Some(false), Duration::ZERO);
    f
}

fn sample(f: &Fixture, ptt: Option<bool>, amp_ptt: Option<bool>, age: Duration) {
    let mut e = f.engine.lock().unwrap();
    let radio = e.remote_open_radio().unwrap();
    let r = e.remote_radio_read(&radio, Instant::now() - age).unwrap();
    e.remote_observe_cat(Some(&r), Some(true));
    e.remote_observe_ptt(Some(&r), ptt);
    let amp = e.remote_open_amp().unwrap();
    let r = e.remote_amp_read(&amp, Instant::now()).unwrap();
    e.remote_observe_amp(
        Some(&r),
        AmpStatusDto {
            family: "spe".into(),
            model: "15K".into(),
            linked: true,
            transmitting: amp_ptt,
            operate: Some(false),
            output_watts: Some(0),
            band_label: Some("20m".into()),
            ..Default::default()
        },
    );
}

fn action(f: &Fixture, follow: bool) -> Value {
    let e = f.engine.lock().unwrap();
    json!({"action":"amplifier.followBand", "radioId": e.settings().active_radio,
        "expectedSettingsRevision": crate::remote_service::query::settings_revision(e.settings()).unwrap(),
        "expectedFollow": e.settings().amp_follow_band, "follow": follow})
}

fn run(f: &Fixture, command: &Request) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, 3),
        SESSION,
        DEVICE,
        command,
        &f.engine,
        Instant::now(),
    )
}

fn command(f: &Fixture, intent: Value) -> Request {
    let state = acquire_controls_version(f, Instant::now(), 3);
    control_request(&state, intent)
}

#[test]
fn follow_save_preserves_every_other_setting_and_survives_reload_and_grant_loss() {
    for prior in [false, true] {
        let f = station(prior);
        let mut expected = f.engine.lock().unwrap().settings().clone();
        expected.amp_follow_band = !prior;
        let active = expected.active_radio;
        expected
            .radios
            .iter_mut()
            .find(|p| p.id == active)
            .unwrap()
            .amp_follow_band = !prior;
        let before_generation = f
            .engine
            .lock()
            .unwrap()
            .remote_receiver_context_generation();
        let request = command(&f, action(&f, !prior));
        let result = run(&f, &request).unwrap();
        assert_eq!(result["outcome"], "applied");
        assert_eq!(result["evidence"], "settingsSaved");
        let e = f.engine.lock().unwrap();
        assert_eq!(
            serde_json::to_value(e.settings()).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(e.remote_receiver_context_generation(), before_generation);
        drop(e);
        f.authority.permit_station(DEVICE, false).unwrap();
        f.authority.retire_connection(f.connection);
        assert_eq!(f.engine.lock().unwrap().settings().amp_follow_band, !prior);
        let loaded = Settings::load(&f.dir.join("settings.json"));
        assert_eq!(
            serde_json::to_value(&loaded).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(
            tempo_app::engine::Engine::with_settings(loaded)
                .settings()
                .amp_follow_band,
            !prior
        );
    }
}

#[test]
fn follow_duplicate_and_result_recovery_do_not_resave_after_a_later_local_choice() {
    let f = station(false);
    let request = command(&f, action(&f, true));
    let first = run(&f, &request).unwrap();
    assert_eq!(first["evidence"], "settingsSaved");
    {
        let mut e = f.engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.amp_follow_band = false;
        e.apply_settings(s);
        e.settings().save(&f.dir.join("settings.json")).unwrap();
    }
    let bytes = std::fs::read(f.dir.join("settings.json")).unwrap();
    assert_eq!(run(&f, &request).unwrap(), first);
    assert_eq!(
        run(
            &f,
            &Request::Result {
                request_id: id(),
                operation_id: request.id().into()
            }
        )
        .unwrap(),
        first
    );
    assert!(!f.engine.lock().unwrap().settings().amp_follow_band);
    assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), bytes);
}

#[test]
fn follow_refuses_a_changed_document_wrong_profile_and_mismatched_saved_choice() {
    for mismatch in ["revision", "radio", "follow"] {
        let f = station(false);
        let mut intent = action(&f, true);
        match mismatch {
            "revision" => intent["expectedSettingsRevision"] = "f".repeat(64).into(),
            "radio" => intent["radioId"] = 8.into(),
            _ => {
                intent["expectedFollow"] = true.into();
                intent["follow"] = false.into();
            }
        }
        let before = std::fs::read(f.dir.join("settings.json")).unwrap();
        let request = command(&f, intent);
        let result = run(&f, &request).unwrap();
        assert_eq!(result["outcome"], "rejected", "{mismatch}");
        assert_eq!(result["reason"], "contextChanged", "{mismatch}");
        assert!(!f.engine.lock().unwrap().settings().amp_follow_band);
        assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), before);
    }
    let f = station(false);
    let old_form = action(&f, true);
    {
        let mut e = f.engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.mygrid = "FN31".into();
        e.apply_settings(s);
    }
    // Fresh lease/window does not give an old form a new Settings revision.
    let request = command(&f, old_form);
    assert_eq!(run(&f, &request).unwrap()["reason"], "contextChanged");
    assert_eq!(f.engine.lock().unwrap().settings().mygrid, "FN31");
}

#[test]
fn follow_enable_requires_idle_fresh_current_hardware_but_disable_works_offline_and_armed() {
    for reason in ["armed", "ptt", "unknown", "old", "amp", "offline"] {
        let f = station(false);
        match reason {
            "armed" => f.engine.lock().unwrap().set_tx_enabled(true),
            "ptt" => sample(&f, Some(true), Some(false), Duration::ZERO),
            "unknown" => sample(&f, None, Some(false), Duration::ZERO),
            "old" => sample(&f, Some(false), Some(false), Duration::from_secs(2)),
            "amp" => sample(&f, Some(false), Some(true), Duration::ZERO),
            _ => {
                f.engine.lock().unwrap().remote_open_amp().unwrap();
            }
        }
        let request = command(&f, action(&f, true));
        let result = run(&f, &request).unwrap();
        assert_eq!(result["outcome"], "rejected", "{reason}");
        assert!(!f.engine.lock().unwrap().settings().amp_follow_band);
    }
    let f = station(true);
    {
        let mut e = f.engine.lock().unwrap();
        e.set_tx_enabled(true);
        e.remote_open_radio().unwrap();
        e.remote_open_amp().unwrap();
    }
    let request = command(&f, action(&f, false));
    let result = run(&f, &request).unwrap();
    assert_eq!(result["evidence"], "settingsSaved");
    let e = f.engine.lock().unwrap();
    assert!(e.tx_enabled());
    assert!(!e.settings().amp_follow_band);
}

#[test]
fn follow_disk_failure_preserves_file_and_memory_and_a_duplicate_never_retries() {
    let f = station(false);
    let before = std::fs::read(f.dir.join("settings.json")).unwrap();
    // A real failing native save, independent of Unix root permission behavior.
    let block = f.dir.join("settings.json.tmp");
    std::fs::create_dir(&block).unwrap();
    let request = command(&f, action(&f, true));
    let result = run(&f, &request).unwrap();
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "persistenceFailed");
    std::fs::remove_dir(&block).unwrap();
    assert_eq!(run(&f, &request).unwrap(), result);
    assert!(!f.engine.lock().unwrap().settings().amp_follow_band);
    assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), before);
    let fresh = control_request(
        &control_state_version(&f, Instant::now(), 3),
        action(&f, true),
    );
    assert_eq!(run(&f, &fresh).unwrap()["evidence"], "settingsSaved");
}

#[test]
fn follow_is_closed_v3_and_never_inherits_logging_only_or_expired_authority() {
    let f = station(false);
    let intent = action(&f, true);
    let mut extra = intent.clone();
    extra["settingsPath"] = "/untrusted/settings.json".into();
    assert!(serde_json::from_value::<station::Action>(extra).is_err());
    let request = command(&f, intent);
    assert_eq!(
        f.authority.handle_version(
            (f.connection, 2),
            SESSION,
            DEVICE,
            &request,
            &f.engine,
            Instant::now()
        ),
        Err("stationUnsupported")
    );
    assert!(
        !control_state_version(&f, Instant::now(), 2)["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("ampFollowBand"))
    );
    f.authority.permit_station(DEVICE, false).unwrap();
    f.authority.permit(DEVICE, true).unwrap();
    assert!(run(&f, &request).is_err());
    assert!(!f.engine.lock().unwrap().settings().amp_follow_band);
    let fresh = command(&f, action(&f, true));
    assert!(f
        .authority
        .handle_version(
            (f.connection, 3),
            SESSION,
            DEVICE,
            &fresh,
            &f.engine,
            Instant::now() + Duration::from_secs(30)
        )
        .is_err());
    assert!(!f.engine.lock().unwrap().settings().amp_follow_band);
}
