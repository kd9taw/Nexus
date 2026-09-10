//! The cloud runtime test invokes the ignored probe through piped stdin/stdout.
//! Only this test substitutes a vault and synthetic readings. No test mode, local
//! origin allowance, synthetic RF data or credential export exists in the app.
use super::*;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tempo_app::{dto::AmpStatusDto, engine::Engine, settings::Settings};

#[derive(Clone, Default)]
struct MemoryVault {
    values: Arc<Mutex<HashMap<String, String>>>,
    fail: Arc<AtomicBool>,
}
impl Vault for MemoryVault {
    fn binding(&self) -> Result<Option<Binding>, &'static str> {
        self.values
            .lock()
            .unwrap()
            .get("binding")
            .map(|v| serde_json::from_str(v).map_err(|_| "invalidResponse"))
            .transpose()
    }
    fn credential(&self, binding: &Binding) -> Result<String, &'static str> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("credentialStoreUnavailable");
        }
        self.values
            .lock()
            .unwrap()
            .get(&binding.station_id)
            .cloned()
            .ok_or("credentialStoreUnavailable")
    }
    fn stage(&self, binding: &Binding, credential: &str) -> Result<(), &'static str> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("credentialStoreUnavailable");
        }
        self.values
            .lock()
            .unwrap()
            .insert(binding.station_id.clone(), credential.to_string());
        Ok(())
    }
    fn save(&self, binding: &Binding, credential: &str) -> Result<(), &'static str> {
        self.stage(binding, credential)?;
        self.values
            .lock()
            .unwrap()
            .insert("binding".into(), serde_json::to_string(binding).unwrap());
        Ok(())
    }
    fn remove(&self, binding: &Binding) -> Result<(), &'static str> {
        let mut values = self.values.lock().unwrap();
        values.remove(&binding.station_id);
        values.remove("binding");
        Ok(())
    }
}

#[test]
fn remote_vault_lifecycle_is_write_only_and_keeps_unrelated_entries() {
    let vault = MemoryVault::default();
    vault
        .values
        .lock()
        .unwrap()
        .insert("unrelated-connector".into(), "unchanged".into());
    let binding = Binding {
        origin: REMOTE_ORIGIN.into(),
        station_id: "station".into(),
        account_id: "account".into(),
    };
    let first = transport::random_secret().unwrap();
    let second = transport::random_secret().unwrap();
    vault.stage(&binding, &first).unwrap();
    assert!(
        vault.binding().unwrap().is_none(),
        "staging cannot restore an unconfirmed pairing after restart"
    );
    vault.save(&binding, &first).unwrap();
    vault.save(&binding, &second).unwrap();
    assert!(vault.credential(&binding).unwrap() == second);
    vault.remove(&binding).unwrap();
    vault.remove(&binding).unwrap();
    assert!(vault.binding().unwrap().is_none());
    assert_eq!(vault.values.lock().unwrap().len(), 1);
}

#[test]
fn cloud_commands_and_remote_origins_have_no_generic_dispatch() {
    for data in [
        r#"{"type":"ptt","enabled":true}"#,
        r#"{"type":"invoke","command":"halt_tx"}"#,
        r#"{"type":"enable","command":"set_freq"}"#,
        r#"{"type":"approve","enrollmentId":"x"}"#,
    ] {
        assert!(serde_json::from_str::<Action>(data).is_err());
    }
    assert!(serde_json::from_str::<Action>(r#"{"type":"disable"}"#).is_ok());
    assert!(Client::new("https://untrusted.invalid").is_err());
    assert!(Client::new("http://127.0.0.1:1234/?token=hidden").is_err());
    assert!(Client::new(REMOTE_ORIGIN).is_ok());
}

#[test]
fn disable_preempts_a_full_command_queue() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (commands, _receiver) = mpsc::channel(1);
    let (cancel, mut cancellation) = watch::channel(false);
    let service = Service {
        commands,
        status: Arc::new(Mutex::new(Status::default())),
        control: Arc::new(Mutex::new(Control {
            enabled: true,
            cancel: Some(cancel),
            ..Default::default()
        })),
    };
    let (reply, _) = oneshot::channel();
    assert!(service
        .commands
        .try_send((Action::Refresh {}, 0, reply))
        .is_ok());
    assert!(matches!(
        runtime.block_on(service.action(Action::Disable {})),
        Err("remoteBusy")
    ));
    assert!(*cancellation.borrow_and_update());
    assert!(!service.control.lock().unwrap().enabled);
}

#[test]
fn memory_publication_obeys_enable_generation_and_disable_preemption() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (commands, _receiver) = mpsc::channel(1);
    let service = Service {
        commands,
        status: Default::default(),
        control: Default::default(),
    };
    let bank = service.control.lock().unwrap().memories.clone();
    let raw = include_str!("../../../ui/src/remote-web/__fixtures__/memories.json");
    assert!(!service.publish_memories("0", Some(raw)));
    assert!(service.status().unwrap().observation_generation.is_none());
    service.control.lock().unwrap().enabled = true;
    assert_eq!(
        service.status().unwrap().observation_generation.as_deref(),
        Some("0")
    );
    assert!(service.publish_memories("0", Some(raw)));
    assert!(query::memories::read(&bank).is_ok());
    assert!(!service.publish_memories("0", Some("{}")));
    assert!(query::memories::read(&bank).is_err());
    assert!(service.publish_memories("0", Some(raw)));
    let (reply, _) = oneshot::channel();
    service
        .commands
        .try_send((Action::Refresh {}, 0, reply))
        .unwrap();
    assert_eq!(
        runtime.block_on(service.action(Action::Disable {})).err(),
        Some("remoteBusy")
    );
    assert!(query::memories::read(&bank).is_err());
    service.control.lock().unwrap().enabled = true;
    assert!(
        !service.publish_memories("0", Some(raw)),
        "old enable replies cannot revive the bank"
    );
    assert!(query::memories::read(&bank).is_err());
    assert!(service.publish_memories("1", Some(raw)));
    assert!(query::memories::read(&bank).is_ok());
    assert!(!service.publish_memories("1", None));
    assert!(query::memories::read(&bank).is_err());
}

#[test]
fn refused_enable_does_not_change_local_authority() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (commands, _receiver) = mpsc::channel(1);
    let service = Service {
        commands,
        status: Arc::new(Mutex::new(Status::default())),
        control: Arc::new(Mutex::new(Control::default())),
    };
    let (reply, _) = oneshot::channel();
    assert!(service
        .commands
        .try_send((Action::Refresh {}, 0, reply))
        .is_ok());
    assert!(matches!(
        runtime.block_on(service.action(Action::Enable {})),
        Err("remoteBusy")
    ));
    assert!(!service.control.lock().unwrap().enabled);
}

#[test]
#[ignore = "invoked by remote/test/native.test.mjs with ephemeral account data over pipes"]
fn cloud_runtime_probe() {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let config: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    let origin = config["origin"].as_str().unwrap().to_string();
    assert!(origin.starts_with("http://127.0.0.1:"));
    let mut settings = Settings {
        mycall: "N0CALL".into(),
        mygrid: "AA00".into(),
        amp_model: "spe".into(),
        amp_port: "test-only".into(),
        ..Default::default()
    };
    settings.ensure_radio_profiles();
    let engine = Arc::new(Mutex::new(Engine::with_settings(settings)));
    {
        let mut e = engine.lock().unwrap();
        let chars: Vec<_> = "CQ W1AW"
            .chars()
            .map(|ch| tempo_core::textmode::DecodedChar {
                ch,
                confidence: 0.3,
            })
            .collect();
        e.set_rtty_armed(true);
        e.set_psk_armed(true);
        e.set_psk_mode(tempo_core::psk::PskModeKind::Qpsk31, true)
            .unwrap();
        e.push_rtty_decode(&chars, -12.5, true);
        e.push_psk_decode(&chars, 7.5, true);
    }
    let read_engine = engine.clone();
    let scope_feed = tempo_app::engine::SpectrumFeed::default();
    let read_scope = scope_feed.clone();
    let readings_stop = Arc::new(AtomicBool::new(false));
    let stop = readings_stop.clone();
    let readings = std::thread::spawn(move || {
        let (radio, amp) = {
            let mut e = read_engine.lock().unwrap();
            (e.remote_open_radio().unwrap(), e.remote_open_amp().unwrap())
        };
        while !stop.load(Ordering::Relaxed) {
            let mut e = read_engine.lock().unwrap();
            let read = e.remote_radio_read(&radio, Instant::now());
            e.remote_observe_cat(read.as_ref(), Some(true));
            e.remote_observe_dial(read.as_ref(), Some(14_074_000));
            e.remote_observe_mode(read.as_ref(), Some("USB"));
            e.remote_observe_ptt(read.as_ref(), Some(false));
            let read = e.remote_amp_read(&amp, Instant::now());
            e.remote_observe_amp(
                read.as_ref(),
                AmpStatusDto {
                    family: "spe".into(),
                    model: "13K".into(),
                    linked: true,
                    output_watts: Some(12),
                    operate: Some(false),
                    ..Default::default()
                },
            );
            drop(e);
            read_scope.publish_audio(tempo_app::dto::Spectrum {
                row: vec![0.25; 8],
                lo_hz: 0.0,
                hi_hz: 4000.0,
                source: "audio".into(),
            });
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let vault = MemoryVault::default();
    let prop_cache: crate::PropCache = Default::default();
    let sources = query::Sources {
        spots: Default::default(),
        live_paths: Default::default(),
        region_paths: crate::SharedRegionPaths(Default::default()),
        ota: Default::default(),
        parks: Default::default(),
        health: Default::default(),
        propagation: prop_cache.clone(),
        memories: Default::default(),
    };
    let ota_cache = sources.ota.clone();
    let mut service = Service::start(
        origin.clone(),
        Box::new(vault.clone()),
        engine.clone(),
        crate::remote_monitor::Publisher::default(),
        Some(scope_feed.clone()),
        Default::default(),
        Some(sources.clone()),
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    println!("REMOTE_TEST:{{\"ready\":true}}");
    std::io::stdout().flush().unwrap();
    for line in lines {
        let value: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        if value["type"] == "seedJs8" {
            let mut e = engine.lock().unwrap();
            let mut settings = e.settings().clone();
            settings.mycall = "N0CALL".into();
            e.apply_settings(settings);
            e.js8_load_journal(&value["journal"].to_string());
            e.js8_send(None, "TEST MESSAGE WITH MULTIPLE FRAMES".into())
                .unwrap();
            assert!(!e.snapshot().radio.tx_enabled);
            println!(
                "REMOTE_TEST:{}",
                json!({"state":e.js8_state(),"log":e.get_log().into_iter().map(|r| {
                    let mut q = tempo_app::dto::LoggedQso::from(r);
                    q.entity = propagation::dxcc::resolve(&q.call).map(|i| i.entity.to_string());
                    q
                }).collect::<Vec<_>>()})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "seedTempo" {
            let mut e = engine.lock().unwrap();
            e.set_tier(serde_json::from_value(value["tier"].clone()).unwrap());
            e.load_conversations(serde_json::from_value(value["conversations"].clone()).unwrap());
            let snapshot = e.snapshot();
            assert!(!snapshot.radio.tx_enabled);
            println!(
                "REMOTE_TEST:{}",
                json!({"conversations":snapshot.conversations,"tier":snapshot.link.tier})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "seedFieldDay" {
            let mut e = engine.lock().unwrap();
            let mut settings = e.settings().clone();
            settings.fd_active = true;
            settings.fd_event = "arrlfd".into();
            settings.fd_class = "1D".into();
            settings.fd_section = "EMA".into();
            settings.fd_operator = "W1AW".into();
            settings.fd_power_mult = 2;
            settings.fd_bonuses = vec!["emergency-power".into()];
            settings.fd_bonuses_planned = vec!["natural-power".into()];
            e.apply_settings(settings);
            e.restore_field_day_if_enabled();
            assert!(e.fd_log_manual("K1ABC", "2A", "WI", "CW").unwrap());
            assert!(e.fd_log_manual("K2ABC", "2A", "WI", "PH").unwrap());
            println!("REMOTE_TEST:{}", json!({"seeded":true}));
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "seedOta" {
            let fixture: serde_json::Value = serde_json::from_str(include_str!(
                "../../../ui/src/remote-web/__fixtures__/ota.json"
            ))
            .unwrap();
            let mut cache = ota_cache.lock().unwrap();
            cache.clear();
            for feed in fixture["feeds"].as_array().unwrap() {
                let program = feed["program"].as_str().unwrap();
                if value["missing"].as_str() == Some(program) {
                    continue;
                }
                let rows: Vec<propagation::OtaSpot> =
                    serde_json::from_value(feed["spots"].clone()).unwrap();
                cache.insert(
                    program.into(),
                    (crate::now_unix() - value["age"].as_i64().unwrap_or(0), rows),
                );
            }
            drop(cache);
            let mut e = engine.lock().unwrap();
            e.set_hunted_parks_import(vec!["US-0003".into()]);
            e.set_activation("POTA", "US-0001").unwrap();
            e.set_hunt_target("K2ABC", "POTA", "US-0004").unwrap();
            println!("REMOTE_TEST:{}", json!({ "seeded": true }));
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "publishMemories" {
            let generation = service
                .status()
                .unwrap()
                .observation_generation
                .unwrap_or_default();
            let accepted = service.publish_memories(&generation, value["bank"].as_str());
            println!("REMOTE_TEST:{}", json!({ "accepted": accepted }));
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "seedDxpeditions" {
            let mut e = engine.lock().unwrap();
            let mut settings = e.settings().clone();
            settings.mycall = "W1AW".into();
            e.apply_settings(settings);
            let mut snapshot = propagation::offline(
                crate::now_unix(),
                &e.settings().mycall,
                &e.settings().mygrid,
            );
            snapshot.source = "live".into();
            snapshot
                .dxpeditions
                .workable_now
                .push(propagation::WorkableCard {
                    call: "3Y0TEST".into(),
                    entity: "Bouvet Island".into(),
                    need: propagation::NeedKind::Atno,
                    band: "20m".into(),
                    bearing_deg: 145.,
                    octant: "SE".into(),
                    distance_km: 12500.,
                    status: propagation::WorkStatus::WorkNow,
                    likelihood: "Good".into(),
                    likelihood_score: 0.8,
                    live_confirmed: true,
                    how_to_call: "Synthetic test advice".into(),
                    ft8_mode: None,
                    window_hint: "1400–1700Z".into(),
                    priority: 100,
                    modes: vec!["CW".into()],
                });
            snapshot.dxpeditions.active.push("3Y0TEST".into());
            let context = crate::PropContext {
                call: e.settings().mycall.clone(),
                grid: e.settings().mygrid.clone(),
                log: e.log_read_token(),
            };
            let result = json!({ "boardJson": serde_json::to_string(&snapshot.dxpeditions).unwrap(), "source": snapshot.source, "asOf": snapshot.as_of });
            *prop_cache.lock().unwrap() = Some((Instant::now(), snapshot, context));
            println!("REMOTE_TEST:{result}");
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "keyboardState" {
            let e = engine.lock().unwrap();
            println!(
                "REMOTE_TEST:{}",
                json!({ "rtty": crate::rtty_state_dto(&e), "psk": crate::psk_state_dto(&e),
                "txEnabled": e.snapshot().radio.tx_enabled, "logCount": e.get_log().len() })
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        // Test-only input to the in-memory engine. The production controller has
        // no import action; the response supplies desktop truth for parity checks.
        if value["type"] == "seedRecallLog" {
            let mut e = engine.lock().unwrap();
            e.import_adif(value["adif"].as_str().unwrap());
            let my_call = e.settings().mycall.clone();
            let records = e.get_log();
            drop(e);
            let awards = crate::awards_for_records(&records, &my_call);
            let geography = propagation::compute_log_stats(
                &records.iter().map(|q| &q.call).collect::<Vec<_>>(),
                &my_call,
            );
            let mut log: Vec<_> = records
                .into_iter()
                .map(tempo_app::dto::LoggedQso::from)
                .collect();
            for q in &mut log {
                q.entity = propagation::dxcc::resolve(&q.call).map(|i| i.entity.to_string());
            }
            println!(
                "REMOTE_TEST:{}",
                json!({ "log": log, "awards": awards, "geography": geography })
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        let result = match value["type"].as_str() {
            Some("status") => service.status(),
            Some("vaultFailure") => {
                vault
                    .fail
                    .store(value["enabled"].as_bool().unwrap(), Ordering::Relaxed);
                service.status()
            }
            Some("restart") => {
                drop(service);
                service = Service::start(
                    origin.clone(),
                    Box::new(vault.clone()),
                    engine.clone(),
                    crate::remote_monitor::Publisher::default(),
                    Some(scope_feed.clone()),
                    Default::default(),
                    Some(sources.clone()),
                );
                std::thread::sleep(Duration::from_millis(100));
                service.status()
            }
            Some("exit") => break,
            _ => runtime.block_on(service.action(serde_json::from_value(value).unwrap())),
        };
        // This pipe is consumed by the parent test, never copied into a log.
        let response = match result {
            Ok(status) => json!({"ok":true,"status":status}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        println!("REMOTE_TEST:{response}");
        std::io::stdout().flush().unwrap();
    }
    drop(service);
    readings_stop.store(true, Ordering::Relaxed);
    readings.join().unwrap();
}
#[test]
fn obsolete_connection_cannot_change_newer_local_status() {
    let control = Arc::new(Mutex::new(Control {
        enabled: true,
        ..Default::default()
    }));
    let status = Arc::new(Mutex::new(Status {
        phase: "connected".into(),
        ..Default::default()
    }));
    let session = SessionStatus {
        control: control.clone(),
        status: status.clone(),
        generation: 0,
    };
    {
        let mut state = control.lock().unwrap();
        state.stop();
        state.enabled = true;
    }
    session.set("disabled", Some("accessDenied"));
    assert!(control.lock().unwrap().enabled);
    assert_eq!(status.lock().unwrap().phase, "connected");
    let current = SessionStatus {
        control: control.clone(),
        status: status.clone(),
        generation: 1,
    };
    current.set("disabled", Some("accessDenied"));
    assert!(!control.lock().unwrap().enabled);
    assert_eq!(status.lock().unwrap().phase, "disabled");
}

#[test]
fn actual_native_socket_refuses_cloud_commands_after_a_valid_publication() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        for command in [r#"{"type":"ptt","enabled":true}"#, r#"{"type":"invoke","command":"set_freq"}"#,
            r#"{"type":"watch","enabled":false,"command":"amp_command"}"#] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let client = Client::new(&origin).unwrap();
            let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
            let before = serde_json::to_value(engine.lock().unwrap().snapshot().radio).unwrap();
            let publisher = crate::remote_monitor::Publisher::default();
            let status = SessionStatus { status: Arc::new(Mutex::new(Status::default())),
                control: Arc::new(Mutex::new(Control { enabled: true, ..Default::default() })), generation: 0 };
            let (_cancel, cancellation) = watch::channel(false);
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                socket.send(Message::Text(r#"{"type":"watch","enabled":true,"requestId":"00000000-0000-4000-8000-000000000001"}"#.into())).await.unwrap();
                loop {
                    let next = tokio::time::timeout(Duration::from_secs(3), socket.next()).await.unwrap().unwrap().unwrap();
                    if let Message::Text(text) = next {
                        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                        assert_eq!(value["type"], "publication");
                        assert_eq!(value["frame"]["source"], "native");
                        break;
                    }
                }
                socket.send(Message::Text(command.into())).await.unwrap();
                // Keep the server alive until the native client refuses it.
                let _ = tokio::time::timeout(Duration::from_secs(3), socket.next()).await;
            });
            let token = transport::random_secret().unwrap();
            let result = transport::connected(&client, "00000000-0000-4000-8000-000000000001", &token,
                cancellation, &engine, &transport::Feeds { monitor: publisher.clone(), spectrum: None, meters: Default::default(), sources: None }, &status).await;
            assert_eq!(result, Err("invalidResponse"));
            server.await.unwrap();
            assert_eq!(serde_json::to_value(engine.lock().unwrap().snapshot().radio).unwrap(), before);
        }
    });
}
