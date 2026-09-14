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
    // A locked store refuses the remembered state the same way it refuses the credential.
    fn state(&self) -> Result<Option<vault::State>, &'static str> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("credentialStoreUnavailable");
        }
        Ok(self
            .values
            .lock()
            .unwrap()
            .get("state")
            .and_then(|v| serde_json::from_str(v).ok()))
    }
    fn save_state(&self, state: &vault::State) -> Result<(), &'static str> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("credentialStoreUnavailable");
        }
        self.values
            .lock()
            .unwrap()
            .insert("state".into(), serde_json::to_string(state).unwrap());
        Ok(())
    }
    fn remove_state(&self) -> Result<(), &'static str> {
        self.values.lock().unwrap().remove("state");
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
        e.configure_remote_settings_store(
            std::path::Path::new(config["configurationRoot"].as_str().unwrap())
                .join("settings.json"),
        );
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
        let (mut radio, mut amp) = {
            let mut e = read_engine.lock().unwrap();
            (e.remote_open_radio().unwrap(), e.remote_open_amp().unwrap())
        };
        while !stop.load(Ordering::Relaxed) {
            let mut e = read_engine.lock().unwrap();
            // Synthetic owner tick: exercise permit expiry/Stop without RF or device I/O.
            e.poll_remote_transmit(Instant::now());
            // Consume the modeled retune signal; this fixture owns no CAT device.
            e.take_immediate_retune();
            let read = e.remote_radio_read(&radio, Instant::now()).or_else(|| {
                radio = e.remote_open_radio().unwrap();
                e.remote_radio_read(&radio, Instant::now())
            });
            e.remote_observe_cat(read.as_ref(), Some(true));
            e.remote_observe_dial(read.as_ref(), Some(14_074_000));
            e.remote_observe_mode(read.as_ref(), Some("USB"));
            e.remote_observe_ptt(read.as_ref(), Some(false));
            let read = e.remote_amp_read(&amp, Instant::now()).or_else(|| {
                amp = e.remote_open_amp().unwrap();
                e.remote_amp_read(&amp, Instant::now())
            });
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
    let sstv_files = sstv::fixture::Gallery::new();
    let sources = query::Sources {
        spots: Default::default(),
        live_paths: Default::default(),
        region_paths: crate::SharedRegionPaths(Default::default()),
        ota: Default::default(),
        parks: Default::default(),
        health: Default::default(),
        propagation: prop_cache.clone(),
        memories: Default::default(),
        navigation: Default::default(),
        sstv: sstv_files.source(),
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
        if value["type"] == "seedFtQso"
            || value["type"] == "ftQsoStep"
            || value["type"] == "ftQsoEvidence"
        {
            let root = std::path::Path::new(config["configurationRoot"].as_str().unwrap());
            let mut e = engine.lock().unwrap();
            let mut samples = 0;
            if value["type"] == "seedFtQso" {
                e.halt_tx();
                let mut settings = e.settings().clone();
                settings.mycall = "K2DEF".into();
                settings.mygrid = "FN31".into();
                settings.auto_log = false;
                settings.prompt_to_log = value["prompt"].as_bool().unwrap();
                settings.save_qso_wav = false;
                e.apply_settings(settings);
                e.set_tier(serde_json::from_value(value["tier"].clone()).unwrap());
                e.set_log_path(root.join("ft-contacts.adi"));
                e.set_pending_qso_path(root.join("ft-pending.json"));
                e.take_immediate_retune();
                e.take_slot_tx_abort();
            } else if value["type"] == "ftQsoStep" {
                // Only this ignored pipe-driven test can supply a simulated
                // peer or advance a native slot. All audio remains in memory.
                let slot = value["slot"].as_u64().unwrap();
                if let Some(text) = value["message"].as_str() {
                    let mut peer = Engine::new("W1AW", "FN31", 0);
                    peer.set_tier(e.tier());
                    peer.override_next_tx("K2DEF", Some("FN31"), text);
                    let tx_slot = if peer.tx_even() { 0 } else { 1 };
                    let mut frame: Vec<f32> = peer.poll_tx(tx_slot).into_iter().flatten().collect();
                    samples = frame.len();
                    assert!(samples > 0, "peer must produce actual native waveform");
                    frame.resize((e.active_slot_secs() * 12000.0) as usize, 0.0);
                    assert!(
                        e.ingest(&frame, slot) > 0,
                        "station must decode the actual peer waveform"
                    );
                } else {
                    let tx_slot = if (slot % 2 == 0) == e.tx_even() {
                        slot
                    } else {
                        slot + 1
                    };
                    e.take_immediate_retune();
                    e.take_slot_tx_abort();
                    samples = e.poll_tx(tx_slot).into_iter().flatten().count();
                    assert!(
                        samples > 0,
                        "remote-owned native sequencer must generate its over"
                    );
                }
            }
            let snapshot = e.snapshot();
            println!(
                "REMOTE_TEST:{}",
                json!({"qso":snapshot.qso,"currentQsoLogKey":e.current_qso_log_key(),
                "pendingQsoLogKey":e.pending_qso_log_key(),"pendingLog":snapshot.pending_log,
                "records":e.get_log().into_iter().map(tempo_app::dto::LoggedQso::from).collect::<Vec<_>>(),"adif":std::fs::read_to_string(root.join("ft-contacts.adi")).unwrap_or_default(),
                "journal":root.join("ft-pending.json").is_file(),"samples":samples,"txEnabled":e.tx_enabled()})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "seedLogging" || value["type"] == "loggingEvidence" {
            let path = std::path::Path::new(config["configurationRoot"].as_str().unwrap())
                .join("remote-manual.adi");
            let mut e = engine.lock().unwrap();
            if value["type"] == "seedLogging" {
                let mut settings = e.settings().clone();
                settings.fd_active = false;
                settings.save_qso_wav = false;
                e.apply_settings(settings);
                e.set_log_path(path.clone());
            }
            println!(
                "REMOTE_TEST:{}",
                json!({"count":e.log_records().len(),"adif":std::fs::read_to_string(path).unwrap_or_default(),"txEnabled":e.snapshot().radio.tx_enabled})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "ftCallDecode" {
            let mut e = engine.lock().unwrap();
            // Generate a peer's CQ into memory and decode through the real native
            // path. No radio, PTT, sound device or external station is opened.
            let mut peer = Engine::new("W1AW", "FN31", 0);
            peer.set_tier(e.tier());
            peer.start_cq(None).unwrap();
            let slot = if peer.tx_even() { 0 } else { 1 };
            let mut frame: Vec<f32> = peer.poll_tx(slot).into_iter().flatten().collect();
            assert!(!frame.is_empty(), "the synthetic peer must generate a CQ");
            frame.resize((e.active_slot_secs() * 12000.0) as usize, 0.0);
            assert!(
                e.ingest(&frame, 8) > 0,
                "the native decoder must hear the peer"
            );
            let snapshot = e.snapshot();
            let decode = snapshot
                .recent_decodes
                .iter()
                .find(|d| d.from.as_deref() == Some("W1AW"))
                .unwrap();
            println!(
                "REMOTE_TEST:{}",
                json!({"call":"W1AW","grid":null,
                "message":decode.message,"snr":decode.snr,"freq":decode.freq_hz})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "ftCallEvidence" {
            let e = engine.lock().unwrap();
            println!(
                "REMOTE_TEST:{}",
                json!({"qso":e.snapshot().qso,"owned":e.remote_ft_tx_owned()})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "ftRuntimeEvidence" {
            let e = engine.lock().unwrap();
            println!(
                "REMOTE_TEST:{}",
                json!({"runtime":e.remote_ft_runtime(),"txEnabled":e.tx_enabled(),"owned":e.remote_ft_tx_owned()})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "ftSettingsEvidence" {
            let e = engine.lock().unwrap();
            println!(
                "REMOTE_TEST:{}",
                json!({"settings":e.remote_ft_settings(),"txEnabled":e.tx_enabled(),"owned":e.remote_ft_tx_owned()})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "seedFt" || value["type"] == "ftEvidence" {
            let mut e = engine.lock().unwrap();
            if value["type"] == "seedFt" {
                e.halt_tx();
                e.set_tier(serde_json::from_value(value["tier"].clone()).unwrap());
                e.take_immediate_retune();
                e.take_slot_tx_abort();
            }
            let snapshot = e.snapshot();
            println!(
                "REMOTE_TEST:{}",
                json!({"tier":snapshot.link.tier,
                "txEnabled":snapshot.radio.tx_enabled,"owned":e.remote_ft_tx_owned(),
                "logCount":e.log_records().len()})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "seedConfiguration" {
            let root = std::path::Path::new(config["configurationRoot"].as_str().unwrap());
            let path = crate::radioprog_path();
            assert!(
                path.starts_with(root),
                "the actual station sidecar must resolve inside the isolated probe profile"
            );
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let file: crate::RadioProgFile =
                serde_json::from_value(json!({"version":1,"projects":value["projects"]})).unwrap();
            let bytes = serde_json::to_vec(&file).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            let result = query::configuration_probe(&engine).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert!(!engine.lock().unwrap().snapshot().radio.tx_enabled);
            println!("REMOTE_TEST:{}", result);
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "seedNavigation" {
            let (call, grid, log) = {
                let e = engine.lock().unwrap();
                (
                    e.settings().mycall.clone(),
                    e.settings().mygrid.clone(),
                    e.log_read_token(),
                )
            };
            let mut prop = propagation::offline(crate::now_unix(), &call, &grid);
            prop.source = "live".into();
            *prop_cache.lock().unwrap() = Some((
                Instant::now(),
                prop.clone(),
                crate::PropContext { call, grid, log },
            ));
            let seed = query::navigation::test_fresh_catalog();
            let expected_tles = seed.elements.len();
            assert!(expected_tles > 300);
            *crate::TLES.lock().unwrap() = Some(seed);
            *crate::SATNOGS.lock().unwrap() = Some(Default::default());
            let e = engine.lock().unwrap();
            assert!(!e.snapshot().radio.tx_enabled);
            println!(
                "REMOTE_TEST:{}",
                json!({"prop":serde_json::from_slice::<serde_json::Value>(&serde_json::to_vec(&prop).unwrap()).unwrap(),"tleCount":expected_tles,"logCount":e.log_records().len(),"live":query::navigation::live(&e).unwrap()})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        if value["type"] == "seedStationModes" {
            let mut e = engine.lock().unwrap();
            sstv_files.seed(&mut e);
            aprs::seed(&mut e);
            let images:Vec<_>=e.sstv_gallery().iter().map(|g| json!({"extension":std::path::Path::new(&g.path).extension().unwrap().to_str().unwrap(),
                "base64":crate::b64_encode(&std::fs::read(&g.path).unwrap())})).collect();
            assert!(!e.snapshot().radio.tx_enabled);
            println!(
                "REMOTE_TEST:{}",
                json!({"sstv":crate::sstv_state_dto(&e),"aprs":aprs::live(&e).unwrap(),
                "heard":e.aprs_heard(),"roster":e.aprs_stations(crate::now_unix()),"images":images})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
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

// ---- Remote across a Nexus restart (operator decision 2026-09-13) --------------------------------
//
// A restart is simulated the way the probe's `restart` does it: drop the Service and start a new
// one over the SAME vault and engine. Nothing else carries across, so anything that survives came
// through the vault. The cloud is a fake HTTP responder on 127.0.0.1: it answers the native device
// list and station revoke, and refuses everything else, so the transport's WebSocket attempt fails
// and backs off (`reconnecting`) without any real service.

const BROWSER: &str = "10000000-0000-4000-8000-00000000000b";
const STATION: &str = "20000000-0000-4000-8000-00000000000a";
const ACCOUNT: &str = "30000000-0000-4000-8000-00000000000c";
const APPROVED_UNTIL: u64 = 4_102_444_800_000; // 2100-01-01, far past any test run

struct FakeCloud {
    origin: String,
    devices: Arc<Mutex<String>>,
}
fn device_list(approved: u8, expires_at: u64) -> String {
    json!({"devices":[{"id":BROWSER,"name":"Test browser","approved":approved,"expiresAt":expires_at}]})
        .to_string()
}
async fn fake_cloud() -> FakeCloud {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let devices = Arc::new(Mutex::new(device_list(1, APPROVED_UNTIL)));
    let served = devices.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let served = served.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                // Read the head, then whatever body its Content-Length names.
                let head_end = loop {
                    let Ok(n) = socket.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..n]);
                    if let Some(at) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let head = String::from_utf8_lossy(&request[..head_end]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                while request.len() < head_end + length {
                    let Ok(n) = socket.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..n]);
                }
                let (status, body) = if head.contains("/native/devices ") {
                    ("200 OK", served.lock().unwrap().clone())
                } else if head.contains("/native/revoke ") {
                    ("200 OK", r#"{"ok":true}"#.to_string())
                } else {
                    ("503 Service Unavailable", "{}".to_string())
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    FakeCloud { origin, devices }
}
fn paired_vault(origin: &str) -> MemoryVault {
    let vault = MemoryVault::default();
    let binding = Binding {
        origin: origin.into(),
        station_id: STATION.into(),
        account_id: ACCOUNT.into(),
    };
    vault
        .save(&binding, &transport::random_secret().unwrap())
        .unwrap();
    vault
}
fn launch(cloud: &FakeCloud, vault: &MemoryVault, engine: &crate::SharedEngine) -> Service {
    Service::configured(
        cloud.origin.clone(),
        Box::new(vault.clone()),
        engine.clone(),
        crate::remote_monitor::Publisher::default(),
    )
}
async fn eventually(service: &Service, what: &str, test: impl Fn(&Status) -> bool) -> Status {
    for _ in 0..250 {
        let status = service.status().unwrap();
        if test(&status) {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "{what}: never happened; last status {}",
        serde_json::to_string(&service.status().unwrap()).unwrap()
    );
}
fn on(status: &Status) -> bool {
    status.observation_generation.is_some()
        && ["connecting", "connected", "reconnecting"].contains(&status.phase.as_str())
}
/// Settle the launch, and let an asynchronous vault write land before the simulated exit.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}
/// Turn Remote on, list the browser, and grant it station control and remote logging.
async fn on_with_grants(service: &Service) {
    eventually(service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    service.action(Action::Refresh {}).await.unwrap();
    service
        .action(Action::StationPermission {
            device_id: BROWSER.into(),
            allow: true,
        })
        .await
        .unwrap();
    service
        .action(Action::LoggingPermission {
            device_id: BROWSER.into(),
            allow: true,
        })
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_that_was_on_turns_itself_back_on_after_a_restart() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    let first = eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    assert_eq!(
        first.phase, "disabled",
        "a pairing with nothing remembered starts off"
    );
    assert!(on(&service.action(Action::Enable {}).await.unwrap()));
    settle().await;
    drop(service);

    let service = launch(&cloud, &vault, &engine);
    let status = eventually(&service, "Remote turned itself back on", on).await;
    assert_eq!(status.station_id.as_deref(), Some(STATION));
    assert!(
        !engine.lock().unwrap().tx_enabled(),
        "turning Remote back on never arms the transmitter"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_that_was_turned_off_stays_off_after_a_restart() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    assert!(
        on(&service.action(Action::Enable {}).await.unwrap()),
        "positive control: it was on"
    );
    settle().await;
    service.action(Action::Disable {}).await.unwrap();
    settle().await;
    drop(service);

    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    settle().await;
    let status = service.status().unwrap();
    assert_eq!(status.phase, "disabled", "Turn off Remote is remembered");
    assert!(status.observation_generation.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_station_access_is_remembered_as_off() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    on_with_grants(&service).await;
    settle().await;
    let revoked = service.action(Action::Forget {}).await.unwrap();
    assert_eq!(revoked.station_id, None);
    settle().await;
    drop(service);

    // Pair the same station id again (as a fresh approval would store it). The old "on" and the
    // old grants belonged to the revoked pairing and must not come back with the new one.
    let vault2 = paired_vault(&cloud.origin);
    *vault2.values.lock().unwrap() = {
        let mut values = vault.values.lock().unwrap().clone();
        values.extend(vault2.values.lock().unwrap().clone());
        values
    };
    let service = launch(&cloud, &vault2, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    settle().await;
    let status = service.status().unwrap();
    assert_eq!(
        status.phase, "disabled",
        "Revoke station access is remembered as off"
    );
    assert!(status.station_permissions.is_empty() && status.logging_permissions.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_locked_credential_store_leaves_remote_off_after_a_restart() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    assert!(
        on(&service.action(Action::Enable {}).await.unwrap()),
        "positive control: it was on"
    );
    settle().await;
    drop(service);

    vault.fail.store(true, Ordering::Relaxed);
    let service = launch(&cloud, &vault, &engine);
    settle().await;
    let status = service.status().unwrap();
    assert!(
        !on(&status),
        "a locked vault never turns Remote on: {}",
        status.phase
    );
    assert_eq!(status.error, Some("credentialStoreUnavailable"));
}

#[tokio::test(flavor = "multi_thread")]
async fn station_and_logging_grants_survive_a_restart_for_a_still_approved_browser() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    on_with_grants(&service).await;
    settle().await;
    drop(service);

    let service = launch(&cloud, &vault, &engine);
    // Nobody opens Settings after an unattended restart: the station must fetch the browser list
    // itself before it can restore anything.
    let status = eventually(&service, "grants restored", |s| {
        s.station_permissions == [BROWSER] && s.logging_permissions == [BROWSER]
    })
    .await;
    assert!(on(&status));
}

#[tokio::test(flavor = "multi_thread")]
async fn grants_are_not_restored_for_a_revoked_or_reapproved_browser() {
    for (case, after_restart) in [
        ("revoked", device_list(0, APPROVED_UNTIL)),
        (
            "re-approved (a new approval generation)",
            device_list(1, APPROVED_UNTIL + 1),
        ),
        ("gone from the station", json!({"devices":[]}).to_string()),
        (
            "positive control: unchanged",
            device_list(1, APPROVED_UNTIL),
        ),
    ] {
        let cloud = fake_cloud().await;
        let vault = paired_vault(&cloud.origin);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        on_with_grants(&service).await;
        settle().await;
        drop(service);

        *cloud.devices.lock().unwrap() = after_restart;
        let service = launch(&cloud, &vault, &engine);
        eventually(&service, "Remote back on", on).await;
        // Let the station's own device fetch run, then look.
        settle().await;
        service.action(Action::Refresh {}).await.unwrap();
        let status = service.status().unwrap();
        let restored =
            !status.station_permissions.is_empty() || !status.logging_permissions.is_empty();
        assert_eq!(
            restored,
            case.starts_with("positive"),
            "{case}: {}",
            serde_json::to_string(&status).unwrap()
        );
    }
}

/// THE KEY SAFETY TEST. FT8/FT4 transmit permission is granted at the shack and dies with the
/// process. Station control survives a restart; transmit never does, not even for the browser
/// that held it a moment before.
#[tokio::test(flavor = "multi_thread")]
async fn transmit_grant_never_survives_a_restart() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    on_with_grants(&service).await;
    let before = service
        .action(Action::TransmitPermission {
            device_id: BROWSER.into(),
            allow: true,
        })
        .await
        .unwrap();
    assert_eq!(
        before.transmit_permissions,
        [BROWSER],
        "positive control: the grant existed before the restart"
    );
    settle().await;
    drop(service);

    let service = launch(&cloud, &vault, &engine);
    let status = eventually(&service, "station control restored", |s| {
        s.station_permissions == [BROWSER]
    })
    .await;
    assert!(
        status.transmit_permissions.is_empty(),
        "transmit permission must start empty after a restart"
    );
    // And nothing in the vault could carry it: no persisted record mentions transmission.
    for value in vault.values.lock().unwrap().values() {
        assert!(
            !value.to_ascii_lowercase().contains("transmit"),
            "vault holds transmit state: {value}"
        );
    }
    assert!(
        !engine.lock().unwrap().tx_enabled(),
        "the TX-enable latch is still off"
    );
    // The browser can have its station control back, but asking for FT8/FT4 is refused until the
    // operator grants it again here.
    let authority = service.control.lock().unwrap().operations.clone();
    assert_eq!(authority.local_status()["transmitDevices"], json!([]));
}

#[tokio::test]
async fn local_transmit_grant_requires_approved_enabled_station_control_and_clears_on_stop() {
    const DEVICE: &str = "10000000-0000-4000-8000-000000000001";
    let (commands, _receiver) = mpsc::channel(1);
    let service = Service {
        commands,
        status: Arc::new(Mutex::new(Status::default())),
        control: Arc::new(Mutex::new(Control::default())),
    };
    let grant = || Action::TransmitPermission {
        device_id: DEVICE.into(),
        allow: true,
    };
    assert!(matches!(service.action(grant()).await, Err("accessDenied")));
    service.control.lock().unwrap().enabled = true;
    assert!(matches!(service.action(grant()).await, Err("accessDenied")));
    service.status.lock().unwrap().devices.push(Device {
        id: DEVICE.into(),
        name: "Test browser".into(),
        approved: 1,
        expires_at: u64::MAX,
    });
    assert!(matches!(
        service.action(grant()).await,
        Err("localPermissionRequired")
    ));
    service
        .action(Action::StationPermission {
            device_id: DEVICE.into(),
            allow: true,
        })
        .await
        .unwrap();
    assert!(service.status().unwrap().transmit_permissions.is_empty());
    assert_eq!(
        service.action(grant()).await.unwrap().transmit_permissions,
        [DEVICE]
    );
    service.control.lock().unwrap().stop();
    assert!(service.status().unwrap().transmit_permissions.is_empty());
    assert!(matches!(service.action(grant()).await, Err("accessDenied")));
    // Revocation remains available while disabled; it never creates a grant.
    assert!(service
        .action(Action::TransmitPermission {
            device_id: DEVICE.into(),
            allow: false
        })
        .await
        .unwrap()
        .transmit_permissions
        .is_empty());
    assert!(_receiver.is_empty());
}

/// A pairing saved under an origin this build does not know is never used, never turned on and never
/// deleted. The retired-origin migration must NOT widen this: only the one origin a release retires
/// may ever be treated as moved, and a record for anything else is left for the build that owns it.
#[tokio::test(flavor = "multi_thread")]
async fn a_pairing_saved_under_an_unknown_origin_is_never_used_or_removed() {
    let cloud = fake_cloud().await;
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    // Positive control: the same record under this build's own origin loads, so an "unpaired" below
    // is the origin check refusing and not a harness that cannot load a pairing at all.
    let own = paired_vault(&cloud.origin);
    let service = launch(&cloud, &own, &engine);
    eventually(&service, "the matching pairing loaded", |s| {
        s.station_id.is_some()
    })
    .await;
    drop(service);

    let vault = paired_vault("https://remote.example.invalid");
    let binding = vault.binding().unwrap().unwrap();
    vault
        .save_state(&vault::State {
            binding: binding.clone(),
            enabled: true,
            grants: Vec::new(),
        })
        .unwrap();
    let service = launch(&cloud, &vault, &engine);
    settle().await;
    let status = service.status().unwrap();
    assert!(!on(&status), "a foreign pairing never turns Remote on");
    assert_eq!(status.phase, "unpaired");
    assert_eq!(status.station_id, None);
    assert!(
        vault.binding().unwrap() == Some(binding.clone()),
        "the record is left untouched"
    );
    assert!(vault
        .state()
        .unwrap()
        .is_some_and(|state| state.binding == binding && state.enabled));
}

// DESIGN ONLY. Both tests below encode behaviour that changes pairing and credential handling, which
// waits for the operator's approval; they are ignored until then and fail today. Run them with
// `cargo test --manifest-path src-tauri/Cargo.toml --lib --features radio remote_service -- --ignored`.
const RETIRED_STAGING_ORIGIN: &str = "https://remote-staging.hamradiotools.io";

/// Every 1.12.0 desktop holds a pairing under the staging origin. A build pointed at production must
/// name that as a move (not "unlock your credential store", which is false), must not turn Remote on
/// from it, must keep the record so going back to the previous release still works during the
/// overlap, and must let the operator clear it locally, since the retired service cannot be asked to.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "credential gate: pending operator approval of the Remote two-environment cutover design (retired-origin pairing)"]
async fn a_pairing_under_the_retired_origin_is_named_as_moved_and_can_be_cleared_locally() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(RETIRED_STAGING_ORIGIN);
    let old = vault.binding().unwrap().unwrap();
    vault
        .save_state(&vault::State {
            binding: old.clone(),
            enabled: true,
            grants: Vec::new(),
        })
        .unwrap();
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    settle().await;
    let status = service.status().unwrap();
    assert!(!on(&status), "a moved pairing never turns Remote on");
    assert_eq!(
        status.phase, "unpaired",
        "the station can pair with the new service straight away"
    );
    assert_eq!(
        status.error,
        Some("pairingMoved"),
        "nothing is wrong with the credential store"
    );
    assert!(
        vault.binding().unwrap() == Some(old),
        "nothing is deleted at start-up"
    );

    let forgotten = service
        .action(Action::Forget {})
        .await
        .expect("forgetting a moved pairing is local and needs no service");
    assert_eq!(forgotten.error, None);
    settle().await;
    assert!(vault.binding().unwrap().is_none());
    assert!(vault.state().unwrap().is_none());
}

/// A service that has closed must be able to say so by name, on HTTP and on the station socket. Today
/// the HTTP refusal reads as `pairingExpired` and every socket refusal as `accessDenied`, and both
/// render as "check the connection".
#[tokio::test(flavor = "multi_thread")]
#[ignore = "credential gate: pending operator approval of the Remote two-environment cutover design (serviceMoved refusal)"]
async fn a_closed_service_is_named_on_http_and_on_the_station_socket() {
    let gone = refusing_cloud("410 Gone", r#"{"error":"serviceMoved"}"#).await;
    let client = Client::new(&gone).unwrap();
    assert_eq!(
        client
            .post("enroll", None, json!({ "name": "Station" }))
            .await
            .err(),
        Some("serviceMoved")
    );

    let refused = FakeCloud {
        origin: refusing_cloud("403 Forbidden", r#"{"error":"serviceMoved"}"#).await,
        devices: Arc::new(Mutex::new(String::new())),
    };
    let vault = paired_vault(&refused.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&refused, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    let status = eventually(&service, "the socket refusal was reported", |s| {
        s.phase == "disabled" && s.error.is_some()
    })
    .await;
    assert_eq!(status.error, Some("serviceMoved"));
}

/// Answers every request, HTTP or WebSocket upgrade, with one fixed status and JSON body.
async fn refusing_cloud(status: &'static str, body: &'static str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => request.extend_from_slice(&chunk[..n]),
                    }
                    if let Some(at) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let length = String::from_utf8_lossy(&request[..head_end])
                    .to_ascii_lowercase()
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(str::to_owned))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                while request.len() < head_end + length {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&chunk[..n]),
                    }
                }
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    origin
}
