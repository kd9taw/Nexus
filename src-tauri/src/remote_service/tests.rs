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
        .try_send((Action::Refresh {}, 0, 0, reply))
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
        .try_send((Action::Refresh {}, 0, 0, reply))
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
        .try_send((Action::Refresh {}, 0, 0, reply))
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
        pounces: Default::default(),
        health: Default::default(),
        propagation: prop_cache.clone(),
        memories: Default::default(),
        navigation: Default::default(),
        sstv: sstv_files.source(),
    };
    let ota_cache = sources.ota.clone();
    let park_index = sources.parks.clone();
    let pounce_recent = sources.pounces.clone();
    let mut service = Service::start(
        origin.clone(),
        Box::new(vault.clone()),
        engine.clone(),
        transport::Feeds {
            monitor: crate::remote_monitor::Publisher::default(),
            spectrum: Some(scope_feed.clone()),
            meters: Default::default(),
            sources: Some(sources.clone()),
            #[cfg(feature = "radio")]
            audio: None,
        },
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
                    let tx_slot = if slot.is_multiple_of(2) == e.tx_even() {
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
        if value["type"] == "settingsEvidence" {
            // What a browser's preference change did: the live value, the saved file, the revision
            // the Settings document shows, and the TX-enable latch and dial it must not move.
            let e = engine.lock().unwrap();
            let path = std::path::Path::new(config["configurationRoot"].as_str().unwrap())
                .join("settings.json");
            let saved: serde_json::Value = std::fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or(serde_json::Value::Null);
            println!(
                "REMOTE_TEST:{}",
                json!({"revision":super::query::settings_revision(e.settings()).unwrap(),
                "autoLog":e.settings().auto_log,"contestCheck":e.settings().contest_check,
                "savedAutoLog":saved["autoLog"],"savedContestCheck":saved["contestCheck"],
                "txEnabled":e.tx_enabled(),"dialHz":e.settings().dial_hz()})
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
        if value["type"] == "keyLocal" || value["type"] == "keyEvidence" {
            let mut e = engine.lock().unwrap();
            if value["type"] == "keyLocal" {
                // Stop anything: start a transmission the way the shack does, through the local
                // verbs, so a browser's Stop is shown to reach it.
                match value["kind"].as_str().unwrap() {
                    "ptt" => {
                        e.set_tx_enabled(true);
                        e.set_ptt(true);
                    }
                    "tune" => e.set_tune(true),
                    "cw" => e.send_cw("CQ TEST"),
                    other => panic!("unknown local transmission {other}"),
                }
                println!("REMOTE_TEST:{}", json!({"keyed":e.tx_owner().is_some()}));
            } else {
                println!(
                    "REMOTE_TEST:{}",
                    json!({"keyed":e.tx_owner().is_some(),"manualPtt":e.manual_ptt(),
                    "tuning":e.tuning(),"txEnabled":e.tx_enabled()})
                );
            }
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
        // Test-only park directory. The response is the desktop search_parks and
        // lookup_park answer for the same search, for parity checks.
        if value["type"] == "seedParks" {
            let index = tempo_core::pota::ParkIndex::parse_csv(value["csv"].as_str().unwrap());
            let search = value["search"].as_str().unwrap();
            let parks: Vec<_> = index
                .search(search, 12)
                .into_iter()
                .map(crate::ParkDto::from)
                .collect();
            let exact = index.lookup(search).map(crate::ParkDto::from);
            *park_index.lock().unwrap() = index;
            println!("REMOTE_TEST:{}", json!({ "parks": parks, "exact": exact }));
            std::io::stdout().flush().unwrap();
            continue;
        }
        // Test-only rare-DX input: spots run through the desktop's own Pounce detector. The
        // response is what the desktop raised (its `pounce` event payloads), for parity checks.
        if value["type"] == "seedPounce" {
            let mut e = engine.lock().unwrap();
            let mut settings = e.settings().clone();
            settings.pounce_threshold = tempo_app::settings::PounceThreshold::Atno;
            e.apply_settings(settings);
            drop(e);
            let (tx, rx) = crate::pouncer::channel();
            for spot in value["spots"].as_array().unwrap() {
                tx.offer(crate::pouncer::SpotHint {
                    call: spot["call"].as_str().unwrap().into(),
                    freq_mhz: spot["freqMhz"].as_f64().unwrap(),
                    mode: spot["mode"].as_str().unwrap().into(),
                    spotted_unix: crate::now_unix(),
                });
            }
            drop(tx);
            let mut fired = Vec::new();
            crate::pouncer::run(engine.clone(), rx, pounce_recent.clone(), |p| fired.push(p));
            println!("REMOTE_TEST:{}", json!({ "fired": fired }));
            std::io::stdout().flush().unwrap();
            continue;
        }
        // Test-only desktop truth for the hosted Awards diagnostics: the report the
        // desktop get_confirmation_diagnostics command returns for the seeded log.
        if value["type"] == "confirmationDiagnostics" {
            let e = engine.lock().unwrap();
            let report = tempo_app::dto::DiagnosticsReportDto::from(
                e.confirmation_diagnostics(crate::now_unix(), |call| {
                    propagation::dxcc::resolve(call).map(|i| i.entity.to_string())
                }),
            );
            let log_count = e.get_log().len();
            drop(e);
            println!(
                "REMOTE_TEST:{}",
                json!({ "report": report, "logCount": log_count })
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
                    transport::Feeds {
                        monitor: crate::remote_monitor::Publisher::default(),
                        spectrum: Some(scope_feed.clone()),
                        meters: Default::default(),
                        sources: Some(sources.clone()),
                        #[cfg(feature = "radio")]
                        audio: None,
                    },
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
                cancellation, &engine, &transport::Feeds { monitor: publisher.clone(), spectrum: None, meters: Default::default(), sources: None, #[cfg(feature = "radio")] audio: None }, &status).await;
            assert_eq!(result, Err("invalidResponse"));
            server.await.unwrap();
            assert_eq!(serde_json::to_value(engine.lock().unwrap().snapshot().radio).unwrap(), before);
        }
    });
}

/// One message this build cannot parse - a newer service's shape, or a corrupt frame - ends THAT
/// connection and nothing more. It used to end Remote for good: `supervise` treated `invalidResponse`
/// like `accessDenied`, and the session status turned that into `control.stop()` plus
/// `Persist::Off`, so the station stayed off across restarts and the remote operator was told to
/// turn Remote on at the shack - the one thing a remote operator cannot do. A parse failure is not
/// an access decision; the station backs off and reconnects, and the moment it is updated it works.
#[test]
fn an_unparseable_service_message_reconnects_and_never_turns_remote_off() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let client = Client::new(&origin).unwrap();
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let control = Arc::new(Mutex::new(Control { enabled: true, ..Default::default() }));
        let status = SessionStatus { status: Arc::new(Mutex::new(Status::default())), control: control.clone(), generation: 0 };
        let (cancel, cancellation) = watch::channel(false);
        // Every connection is greeted with a message from a service this build has never met.
        let server = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                    socket.send(Message::Text(r#"{"type":"applicationWatch","watchId":"bbe7d95a-6fbd-47aa-95a7-ab1e4c083dd6","topics":[],"requestId":null,"newerField":true}"#.into())).await.unwrap();
                    let _ = tokio::time::timeout(Duration::from_secs(3), socket.next()).await;
                });
            }
        });
        let binding = Binding { origin: origin.clone(), station_id: STATION.into(), account_id: ACCOUNT.into() };
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), spectrum: None, meters: Default::default(), sources: None, #[cfg(feature = "radio")] audio: None };
        let supervised = tokio::spawn(transport::supervise(client, binding, transport::random_secret().unwrap(), cancellation, engine, feeds, status.clone()));
        let mut seen = None;
        for _ in 0..250 {
            let current = status.status.lock().unwrap().clone();
            if current.phase == "disabled" || current.phase == "reconnecting" {
                seen = Some(current);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let seen = seen.expect("the connection ended one way or the other");
        assert_eq!((seen.phase.as_str(), seen.error), ("reconnecting", Some("invalidResponse")), "a parse failure backs off; it is not an access decision");
        assert!(control.lock().unwrap().enabled, "Remote is still on");
        assert!(!supervised.is_finished(), "the supervisor is still trying");
        let _ = cancel.send(true);
        supervised.await.unwrap();
        server.abort();
    });
}
/// Exclusive use of the process-wide satellite track badge — the crate's one implementation,
/// delegated to so the guard-on-the-guard scanner sees the same `alone()` name in every test
/// module that reaches the badge. See `crate::sat_track_alone`.
fn alone() -> std::sync::MutexGuard<'static, ()> {
    crate::sat_track_alone()
}

/// The class: something slow held something shared. Every send used to be awaited inline on the
/// socket loop, so while one drained, nothing was read — including Stop. A first application
/// batch can be 768 KB; on a slow shack uplink that outlasts the send's 2 s cutoff, the session
/// is torn down, the browser reconnects, and the same batch goes again. Here the pipe from the
/// station holds 4 KB and the relay reads none of it: the legacy reads the station answers
/// outgrow the pipe together (asserted below, the positive control), so the station's writer is
/// stuck by the time the Stop goes in. That Stop must still be admitted, and the engine must show
/// it, before the relay reads a byte.
#[test]
fn a_stop_is_admitted_while_the_stations_sends_are_stuck_on_a_slow_link() {
    // ⚠️ THIS STOP REACHES THE SATELLITE TRACK BADGE. `stopTransmit` → `stop_station` →
    // `satellite::disarm_track` → `disarm_sat_track_locked`, which bumps the process-wide
    // `SAT_TRACK_GEN`. Measured 2026-09-21: without this guard the bump landed inside
    // `a_rotor_that_stops_answering…`'s pass, the pass bailed on its generation check and
    // skipped the LOS handback, and that test failed on "the dial is the operator's again".
    let _alone = alone();
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
    const PIPE: usize = 4096;
    const READS: usize = 8;
    const DEVICE: &str = "10000000-0000-4000-8000-00000000000d";
    const SESSION: &str = "20000000-0000-4000-8000-00000000000e";
    async fn text(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
    ) -> serde_json::Value {
        loop {
            let next = tokio::time::timeout(Duration::from_secs(3), socket.next())
                .await
                .expect("the station answered")
                .unwrap()
                .unwrap();
            if let Message::Text(text) = next {
                return serde_json::from_str(&text).unwrap();
            }
        }
    }
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (station_io, relay_io) = tokio::io::duplex(PIPE);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let status = SessionStatus { status: Arc::new(Mutex::new(Status::default())),
            control: Arc::new(Mutex::new(Control { enabled: true, ..Default::default() })), generation: 0 };
        let authority = status.control.lock().unwrap().operations.clone();
        authority.permit_station(DEVICE, true).unwrap();
        let (cancel, cancellation) = watch::channel(false);
        let relay_engine = engine.clone();
        let relay = tokio::spawn(async move {
            let mut socket = tokio_tungstenite::accept_async(relay_io).await.unwrap();
            let operation = |request: serde_json::Value| Message::Text(json!({"type":"operationRequest","sessionId":SESSION,"deviceId":DEVICE,"operationVersion":4,"request":request}).to_string().into());
            let id = |n: u32| format!("00000000-0000-4000-8000-0000000000{n:02x}");
            // A controlling v4 browser with the stop token, and the FT latch armed at the shack.
            socket.send(operation(json!({"type":"state","requestId":id(1)}))).await.unwrap();
            let state = text(&mut socket).await;
            let boot = state["value"]["stationBootId"].as_str().unwrap().to_owned();
            socket.send(operation(json!({"type":"acquire","requestId":id(2),"stationBootId":boot}))).await.unwrap();
            let lease = text(&mut socket).await["value"]["leaseId"].as_str().unwrap().to_owned();
            socket.send(operation(json!({"type":"state","requestId":id(3)}))).await.unwrap();
            let epoch = text(&mut socket).await["value"]["transmitEpoch"].as_str().expect("the controller holds the stop token").to_owned();
            relay_engine.lock().unwrap().set_tx_enabled(true);
            // Fill the pipe: legacy reads the station answers inline, none of them read here.
            for n in 0..READS as u32 {
                socket.send(Message::Text(json!({"type":"applicationRead","requestId":id(0x10 + n),"command":"get_snapshot","revision":null}).to_string().into())).await.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            // The station's writer is stuck. Stop must still get through.
            socket.send(operation(json!({"type":"stopTransmit","requestId":id(0x20),"stationBootId":boot,"leaseId":lease,"transmitEpoch":epoch}))).await.unwrap();
            let mut stopped = false;
            for _ in 0..300 {
                if !relay_engine.lock().unwrap().tx_enabled() { stopped = true; break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(stopped, "the Stop was admitted while the station's sends were stuck");
            // Only now does the relay read: every read answer, then the Stop's acceptance behind
            // them. Nothing was dropped.
            let mut answers = Vec::new();
            loop {
                let answer = text(&mut socket).await;
                if answer["type"] == "operationResponse" { answers.push(answer); break; }
                assert_eq!(answer["type"], "applicationResult");
                answers.push(answer);
            }
            assert_eq!(answers.len(), READS + 1);
            let queued: usize = answers[..READS].iter().map(|a| a.to_string().len()).sum();
            assert!(queued > PIPE, "positive control: {queued} bytes of read answers outgrow the {PIPE}-byte pipe nobody was draining, so the writer was stuck when the Stop went in");
            assert_eq!(answers[READS]["requestId"], id(0x20));
            assert_eq!(answers[READS]["value"], json!({"stop":"accepted"}));
            cancel.send(true).unwrap();
        });
        let request = "ws://localhost/api/remote/stations/x/connect".into_client_request().unwrap();
        let (socket, _) = tokio_tungstenite::client_async_with_config(request, station_io, Some(transport::socket_config())).await.unwrap();
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), spectrum: None, meters: Default::default(), sources: None, #[cfg(feature = "radio")] audio: None };
        let served = transport::serve(socket, cancellation, &engine, &feeds, &status).await;
        relay.await.unwrap();
        assert_eq!(served, Ok(()), "the session ended because Remote stopped, not because a send timed out");
    });
}

/// Operation v5: the station SAYS when a rig-touching control settles. Before it, every such
/// control answered `pending` by construction and the browser learned the outcome by polling -
/// a `state` read, a `result` read, each a relay round trip, with a 1 s back-off between them.
/// The station knew the answer the whole time: the radio loop finishes the `Completion` the
/// receipt holds. Here a v5 browser gets an `operationEvent` with the outcome and a fresh state
/// (new command window, advanced revision) the moment the readback lands, having sent NOTHING
/// after the control. The positive control is the same exchange at v4: nothing arrives until
/// the browser asks with a `result` read - the old path, kept exactly for a browser that never
/// negotiated the push, and the proof that the event is version-gated rather than broadcast.
#[test]
#[cfg(feature = "radio")]
fn a_settled_control_is_pushed_to_a_v5_browser_and_polled_by_a_v4_one() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
    const DEVICE: &str = "10000000-0000-4000-8000-00000000001d";
    const SESSION: &str = "20000000-0000-4000-8000-00000000001e";
    async fn text(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
        within: Duration,
    ) -> Option<serde_json::Value> {
        loop {
            let next = tokio::time::timeout(within, socket.next()).await.ok()??;
            if let Message::Text(text) = next.unwrap() {
                return Some(serde_json::from_str(&text).unwrap());
            }
        }
    }
    fn sample(
        engine: &crate::SharedEngine,
        radio: &tempo_app::remote_monitor::provenance::Connection,
        hz: u64,
    ) {
        let mut e = engine.lock().unwrap();
        let read = e.remote_radio_read(radio, Instant::now()).unwrap();
        e.remote_observe_cat(Some(&read), Some(true));
        e.remote_observe_dial(Some(&read), Some(hz));
        e.remote_observe_mode(Some(&read), Some("PKTUSB"));
        e.remote_observe_ptt(Some(&read), Some(false));
    }
    for version in [5_u8, 4] {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (station_io, relay_io) = tokio::io::duplex(65536);
            let dir = std::env::temp_dir().join(format!("nexus-push-completion-{version}-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN52", 0)));
            let radio = {
                let mut e = engine.lock().unwrap();
                e.configure_remote_settings_store(dir.join("settings.json"));
                e.set_tx_enabled(false);
                e.set_frequency(14.074, "20m", "USB");
                e.take_immediate_retune();
                e.remote_open_radio().unwrap()
            };
            sample(&engine, &radio, 14_074_000);
            let status = SessionStatus { status: Arc::new(Mutex::new(Status::default())),
                control: Arc::new(Mutex::new(Control { enabled: true, ..Default::default() })), generation: 0 };
            let authority = status.control.lock().unwrap().operations.clone();
            authority.permit_station(DEVICE, true).unwrap();
            let (cancel, cancellation) = watch::channel(false);
            let relay_engine = engine.clone();
            let relay = tokio::spawn(async move {
                let mut socket = tokio_tungstenite::accept_async(relay_io).await.unwrap();
                let operation = |request: serde_json::Value| Message::Text(json!({"type":"operationRequest","sessionId":SESSION,"deviceId":DEVICE,"operationVersion":version,"request":request}).to_string().into());
                let id = |n: u32| format!("00000000-0000-4000-8000-0000000000{n:02x}");
                let reply = Duration::from_secs(3);
                socket.send(operation(json!({"type":"state","requestId":id(1)}))).await.unwrap();
                let boot = text(&mut socket, reply).await.unwrap()["value"]["stationBootId"].as_str().unwrap().to_owned();
                socket.send(operation(json!({"type":"acquire","requestId":id(2),"stationBootId":boot}))).await.unwrap();
                text(&mut socket, reply).await.unwrap();
                socket.send(operation(json!({"type":"state","requestId":id(3)}))).await.unwrap();
                let state = text(&mut socket, reply).await.unwrap()["value"].clone();
                assert_eq!(state["phase"], "controlling");
                // The hint the page trusts instead of its own version: named at v5, and only there.
                let hinted = state["controls"]["capabilities"].as_array().unwrap().contains(&json!("outcomePush"));
                assert_eq!(hinted, version == 5, "outcomePush is negotiated, not assumed: v{version}");
                let before = state["revision"].as_u64().unwrap();
                socket.send(operation(json!({"type":"stationControl","requestId":id(4),"stationBootId":boot,
                    "leaseId":state["leaseId"],"expectedRevision":before,"commandWindowId":state["commandWindowId"],
                    "clientSequence":state["nextSequence"],"context":state["controls"]["context"],
                    "action":{"action":"radio.frequency","dialMhz":7.074,"band":"40m","sideband":"USB"}}))).await.unwrap();
                let answer = text(&mut socket, reply).await.unwrap();
                assert_eq!(answer["requestId"], id(4));
                assert_eq!(answer["value"]["outcome"], "pending", "a rig-touching control is pending by construction");
                // The radio loop: take the target, write it, read it back.
                let work = relay_engine.lock().unwrap().take_remote_radio().unwrap();
                assert_eq!(work.target(), (7_074_000, "PKTUSB"));
                work.permission().begin_write(Instant::now()).unwrap();
                sample(&relay_engine, &radio, 7_074_000);
                let power = work.power_limit();
                assert!(work.commit_tuning_readback(&mut relay_engine.lock().unwrap(), power, None));
                if version == 5 {
                    // Unprompted: the browser sent nothing after the control.
                    let event = text(&mut socket, reply).await.expect("the station pushed the outcome");
                    assert_eq!(event["type"], "operationEvent");
                    assert_eq!(event["sessionId"], SESSION);
                    assert_eq!(event["operationId"], id(4));
                    assert_eq!(event["value"]["operation"], "stationControl");
                    assert_eq!(event["value"]["operationId"], id(4));
                    assert_eq!(event["value"]["outcome"], "applied");
                    assert_eq!(event["value"]["evidence"], "radioReadback");
                    let fresh = &event["state"];
                    assert_eq!(fresh["phase"], "controlling");
                    assert!(fresh["commandWindowId"].is_string(), "a new window, the control having spent the old one");
                    assert_ne!(fresh["commandWindowId"], state["commandWindowId"]);
                    assert!(fresh["revision"].as_u64().unwrap() > before, "the revision moved with the dial");
                    assert_eq!(fresh["nextSequence"].as_u64().unwrap(), state["nextSequence"].as_u64().unwrap() + 1);
                    assert!(fresh["controls"]["capabilities"].as_array().unwrap().contains(&json!("outcomePush")));
                    assert_eq!(event.as_object().unwrap().len(), 5, "exactly type, sessionId, operationId, value, state");
                } else {
                    // Positive control: a v4 browser is told nothing it did not ask for...
                    assert_eq!(text(&mut socket, Duration::from_millis(500)).await, None, "no push to a v4 browser");
                    // ...and learns the outcome the old way, by asking.
                    socket.send(operation(json!({"type":"result","requestId":id(5),"operationId":id(4)}))).await.unwrap();
                    let polled = text(&mut socket, reply).await.unwrap();
                    assert_eq!(polled["requestId"], id(5));
                    assert_eq!(polled["value"]["outcome"], "applied");
                }
                cancel.send(true).unwrap();
            });
            let request = "ws://localhost/api/remote/stations/x/connect".into_client_request().unwrap();
            let (socket, _) = tokio_tungstenite::client_async_with_config(request, station_io, Some(transport::socket_config())).await.unwrap();
            let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), spectrum: None, meters: Default::default(), sources: None, audio: None };
            let served = transport::serve(socket, cancellation, &engine, &feeds, &status).await;
            relay.await.unwrap();
            assert_eq!(served, Ok(()));
            let _ = std::fs::remove_dir_all(&dir);
        });
    }
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
const DAY_MS: u64 = 86_400_000;
/// A browser as a service with the approval-lifetime batch lists it to a Nexus that asks for it: the
/// approval generation, and the end that use cannot move. The fake strips both for a Nexus that does
/// not send the lifetime header, exactly as the service does.
fn device_list_at(approved: u8, expires_at: u64, generation: u64) -> String {
    json!({"devices":[{"id":BROWSER,"name":"Test browser","approved":approved,"expiresAt":expires_at,
        "generation":generation,"renewsUntil":(approved == 1).then_some(expires_at + 60 * DAY_MS)}]})
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
                let lifetime = head.contains("\r\nx-nexus-device-lifetime: 1\r\n");
                let (status, body) = if head.contains("/enroll ") {
                    let enrollment = json!({"id":STATION,"proof":"a".repeat(64),
                        "code":"0123456789abcdef","expiresAt":now_ms() + 600000});
                    ("200 OK", enrollment.to_string())
                } else if head.contains("/enroll/check ") {
                    (
                        "200 OK",
                        json!({"accountId":ACCOUNT,"approved":false}).to_string(),
                    )
                } else if head.contains("/enroll/approve ") {
                    // The service approves the browser that confirmed the pairing along with it.
                    let device = if lifetime {
                        json!({"id":BROWSER,"expiresAt":APPROVED_UNTIL,"generation":2})
                    } else {
                        json!({"id":BROWSER,"expiresAt":APPROVED_UNTIL})
                    };
                    let approved = json!({"stationId":STATION,"accountId":ACCOUNT,"device":device});
                    ("200 OK", approved.to_string())
                } else if head.contains("/native/devices ") {
                    let mut listed: serde_json::Value =
                        serde_json::from_str(&served.lock().unwrap()).unwrap();
                    if !lifetime {
                        // What the service sends a Nexus that did not ask: the 1.12.0 shape.
                        for device in listed["devices"].as_array_mut().unwrap() {
                            let device = device.as_object_mut().unwrap();
                            device.remove("generation");
                            device.remove("renewsUntil");
                        }
                    }
                    ("200 OK", listed.to_string())
                } else if head.contains("/native/approve-device ") {
                    // The service writes a new approval, and with it the approval generation.
                    *served.lock().unwrap() = if lifetime {
                        device_list_at(1, APPROVED_UNTIL, 2)
                    } else {
                        device_list(1, APPROVED_UNTIL)
                    };
                    ("200 OK", r#"{"ok":true}"#.to_string())
                } else if head.contains("/native/revoke-device ") {
                    // A revoked browser's expiry becomes "now", so the service stops listing it.
                    *served.lock().unwrap() = json!({"devices":[]}).to_string();
                    ("200 OK", r#"{"ok":true}"#.to_string())
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

// ---- One approval (operator decision 2026-09-13, late) --------------------------------------------
//
// Approving a browser grants it station control and logging, and FT8/FT4 transmit when the approval
// ticks it. All three are remembered across a restart for as long as that approval stands. That
// REPLACES "transmit grants never survive a restart"; what did not change is that nothing arms the
// transmitter: the TX-enable latch starts off and keying still needs the browser's TX On (proven at
// the authority in operations/transmit_tests.rs, `a_restored_transmit_grant_arms_nothing_...`).

/// A browser the service lists as waiting for approval.
fn pending_browser(cloud: &FakeCloud) {
    *cloud.devices.lock().unwrap() = device_list(0, APPROVED_UNTIL - 1);
}
/// Approve the browser here, the one-approval way, with or without the transmit tick.
async fn approve(service: &Service, transmit: bool) -> Status {
    service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: true,
            transmit,
        })
        .await
        .unwrap()
}
fn transmitter_idle(engine: &crate::SharedEngine) -> bool {
    let e = engine.lock().unwrap();
    !e.tx_enabled() && !e.remote_ft_tx_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn approving_a_browser_grants_control_and_logging_and_transmit_only_when_ticked() {
    for tick in [false, true] {
        let cloud = fake_cloud().await;
        let vault = paired_vault(&cloud.origin);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
        service.action(Action::Enable {}).await.unwrap();
        pending_browser(&cloud);
        let before = service.action(Action::Refresh {}).await.unwrap();
        assert!(
            before.station_permissions.is_empty() && before.logging_permissions.is_empty(),
            "a browser waiting for approval holds nothing"
        );
        let status = approve(&service, tick).await;
        assert!(on(&status), "approving a browser does not toggle Remote");
        assert_eq!(status.station_permissions, [BROWSER], "tick={tick}");
        assert_eq!(status.logging_permissions, [BROWSER], "tick={tick}");
        let expected: &[&str] = if tick { &[BROWSER] } else { &[] };
        assert_eq!(status.transmit_permissions, expected, "tick={tick}");
        assert!(transmitter_idle(&engine), "a grant arms nothing");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_approval_given_while_remote_is_off_applies_at_turn_on() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    pending_browser(&cloud);
    service.action(Action::Refresh {}).await.unwrap();
    let off = approve(&service, true).await;
    assert!(
        !on(&off) && off.observation_generation.is_none(),
        "approving a browser does not turn Remote on; only approving the pairing does"
    );
    assert!(
        off.station_permissions.is_empty() && off.transmit_permissions.is_empty(),
        "nothing is live while Remote is off"
    );
    service.action(Action::Enable {}).await.unwrap();
    eventually(&service, "the approval took effect at Turn on", |s| {
        s.station_permissions == [BROWSER]
            && s.logging_permissions == [BROWSER]
            && s.transmit_permissions == [BROWSER]
    })
    .await;
    assert!(transmitter_idle(&engine));
}

/// Operator decision 2026-09-14: Turn off Remote PAUSES. Turning it back on restores what each
/// still-approved browser had (transmit only where granted). Revoking a browser, and End remote
/// control (take over), still clear. The TX-enable latch is off at every step, and nothing keys.
#[tokio::test(flavor = "multi_thread")]
async fn turn_off_pauses_but_revoking_a_browser_or_ending_remote_control_clears() {
    for case in [
        "positive control: turn off, turn on",
        "turn off, revoke the browser, turn on",
        "end remote control, turn off, turn on",
    ] {
        let cloud = fake_cloud().await;
        let vault = paired_vault(&cloud.origin);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
        service.action(Action::Enable {}).await.unwrap();
        pending_browser(&cloud);
        service.action(Action::Refresh {}).await.unwrap();
        let granted = approve(&service, true).await;
        assert_eq!(granted.transmit_permissions, [BROWSER], "{case}: granted");
        assert!(transmitter_idle(&engine), "{case}");

        if case.starts_with("end remote control") {
            service.action(Action::TakeOverLogging {}).await.unwrap();
        }
        let off = service.action(Action::Disable {}).await.unwrap();
        assert!(!on(&off), "{case}: Remote is off");
        assert!(
            off.station_permissions.is_empty() && off.transmit_permissions.is_empty(),
            "{case}: nothing is live while Remote is off"
        );
        assert!(transmitter_idle(&engine), "{case}");
        if case.contains("revoke the browser") {
            service
                .action(Action::Device {
                    device_id: BROWSER.into(),
                    approve: false,
                    transmit: false,
                })
                .await
                .unwrap();
        }
        service.action(Action::Enable {}).await.unwrap();

        let status = if case.starts_with("positive") {
            eventually(&service, "the paused grants came back", |s| {
                s.station_permissions == [BROWSER]
                    && s.logging_permissions == [BROWSER]
                    && s.transmit_permissions == [BROWSER]
            })
            .await
        } else {
            settle().await;
            let status = service.action(Action::Refresh {}).await.unwrap();
            assert!(
                status.station_permissions.is_empty()
                    && status.logging_permissions.is_empty()
                    && status.transmit_permissions.is_empty(),
                "{case}: nothing comes back: {}",
                serde_json::to_string(&status).unwrap()
            );
            status
        };
        assert!(on(&status), "{case}: Remote is back on");
        assert!(transmitter_idle(&engine), "{case}: the latch is still off");
    }
}

/// Operator decision 2026-09-14: approving the first pairing turns Remote on, the same way Turn on
/// Remote does, and the browser that did the pairing is approved with it. The TX-enable latch stays
/// off, transmit is granted only if the approval ticked it, and a later Turn off is remembered.
#[tokio::test(flavor = "multi_thread")]
async fn approving_the_pairing_turns_remote_on_and_approves_the_browser_that_paired() {
    for tick in [false, true] {
        let cloud = fake_cloud().await;
        let vault = MemoryVault::default();
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        let begun = service
            .action(Action::Begin {
                name: "Test station".into(),
            })
            .await
            .unwrap();
        assert_eq!(begun.phase, "pairing");
        let asked = service.action(Action::Refresh {}).await.unwrap();
        assert_eq!(asked.phase, "approval");
        assert!(
            !on(&asked),
            "positive control: Remote is off before the approval"
        );
        let approved = service
            .action(Action::Approve {
                enrollment_id: STATION.into(),
                account_id: ACCOUNT.into(),
                transmit: tick,
            })
            .await
            .unwrap();
        assert!(
            on(&approved),
            "approving the pairing turns Remote on: {}",
            approved.phase
        );
        let status = eventually(&service, "the pairing browser's grants", |s| {
            s.station_permissions == [BROWSER] && s.logging_permissions == [BROWSER]
        })
        .await;
        let expected: &[&str] = if tick { &[BROWSER] } else { &[] };
        assert_eq!(status.transmit_permissions, expected, "tick={tick}");
        assert!(transmitter_idle(&engine), "the TX-enable latch stays off");

        service.action(Action::Disable {}).await.unwrap();
        settle().await;
        drop(service);
        let service = launch(&cloud, &vault, &engine);
        eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
        settle().await;
        let status = service.status().unwrap();
        assert_eq!(status.phase, "disabled", "the later Turn off is remembered");
        assert!(status.observation_generation.is_none());
        assert!(transmitter_idle(&engine));
    }
}

/// THE KEY SAFETY TEST for the reversed rule. FT8/FT4 transmit comes back after a restart only for
/// a browser the service still lists as approved at the SAME approval, and only if the approval
/// ticked it. In every case the TX-enable latch is still off after the restart and nothing is keyed.
#[tokio::test(flavor = "multi_thread")]
async fn transmit_grant_survives_a_restart_only_if_ticked_on_a_still_approved_browser() {
    for (case, tick, after_restart, restored) in [
        (
            "positive control: ticked, approval unchanged",
            true,
            device_list(1, APPROVED_UNTIL),
            true,
        ),
        ("not ticked", false, device_list(1, APPROVED_UNTIL), false),
        ("revoked", true, device_list(0, APPROVED_UNTIL), false),
        (
            "re-approved (a new approval generation)",
            true,
            device_list(1, APPROVED_UNTIL + 1),
            false,
        ),
        (
            "gone from the station",
            true,
            json!({"devices":[]}).to_string(),
            false,
        ),
    ] {
        let cloud = fake_cloud().await;
        let vault = paired_vault(&cloud.origin);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
        service.action(Action::Enable {}).await.unwrap();
        pending_browser(&cloud);
        service.action(Action::Refresh {}).await.unwrap();
        let before = approve(&service, tick).await;
        assert_eq!(
            before.transmit_permissions.len(),
            usize::from(tick),
            "{case}: the grant as given before the restart"
        );
        settle().await;
        drop(service);

        *cloud.devices.lock().unwrap() = after_restart;
        let service = launch(&cloud, &vault, &engine);
        let still_approved = case.starts_with("positive") || case == "not ticked";
        let status = if still_approved {
            eventually(&service, "station control restored", |s| {
                s.station_permissions == [BROWSER]
            })
            .await
        } else {
            eventually(&service, "Remote back on", on).await;
            settle().await;
            service.action(Action::Refresh {}).await.unwrap()
        };
        let expected: &[&str] = if restored { &[BROWSER] } else { &[] };
        assert_eq!(
            status.transmit_permissions,
            expected,
            "{case}: {}",
            serde_json::to_string(&status).unwrap()
        );
        let authority = service.control.lock().unwrap().operations.clone();
        assert_eq!(
            authority.local_status()["transmitDevices"],
            json!(expected),
            "{case}"
        );
        assert!(
            !engine.lock().unwrap().tx_enabled(),
            "{case}: the TX-enable latch is off after every restart"
        );
        assert!(transmitter_idle(&engine), "{case}: nothing is keyed");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_transmit_or_control_or_taking_over_is_remembered_across_a_restart() {
    for case in ["revoke transmit", "revoke station control", "take over"] {
        let cloud = fake_cloud().await;
        let vault = paired_vault(&cloud.origin);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
        service.action(Action::Enable {}).await.unwrap();
        pending_browser(&cloud);
        service.action(Action::Refresh {}).await.unwrap();
        let granted = approve(&service, true).await;
        assert_eq!(
            granted.transmit_permissions,
            [BROWSER],
            "{case}: positive control"
        );
        let revoked = match case {
            "revoke transmit" => Action::TransmitPermission {
                device_id: BROWSER.into(),
                allow: false,
            },
            "revoke station control" => Action::StationPermission {
                device_id: BROWSER.into(),
                allow: false,
            },
            _ => Action::TakeOverLogging {},
        };
        let now = service.action(revoked).await.unwrap();
        assert!(
            now.transmit_permissions.is_empty(),
            "{case}: takes effect at once"
        );
        settle().await;
        drop(service);

        let service = launch(&cloud, &vault, &engine);
        eventually(&service, "Remote back on", on).await;
        settle().await;
        let status = service.action(Action::Refresh {}).await.unwrap();
        let (control, logging): (&[&str], &[&str]) = match case {
            "revoke transmit" => (&[BROWSER], &[BROWSER]),
            "revoke station control" => (&[], &[BROWSER]),
            _ => (&[], &[]),
        };
        assert_eq!(status.station_permissions, control, "{case}");
        assert_eq!(status.logging_permissions, logging, "{case}");
        assert!(status.transmit_permissions.is_empty(), "{case}");
        assert!(transmitter_idle(&engine), "{case}");
    }
}

// ---- Browser approval lifetime (operator decision 2026-09-14) -------------------------------------
//
// The service renews a browser's approval on use, capped at ninety days since the shack approved it.
// A renewal moves the approval's EXPIRY and never its GENERATION, so everything a restart restores is
// bound to the generation. Renewal restores and grants nothing, and the TX-enable latch is untouched.

#[tokio::test(flavor = "multi_thread")]
async fn restored_grants_follow_the_approval_generation_not_its_expiry() {
    let renewed = APPROVED_UNTIL + 10 * DAY_MS;
    for (case, after_restart, restored) in [
        (
            "positive control: unchanged",
            device_list_at(1, APPROVED_UNTIL, 2),
            true,
        ),
        (
            "renewed on use: a later expiry, the same generation",
            device_list_at(1, renewed, 2),
            true,
        ),
        (
            "re-approved: a new generation at the same expiry",
            device_list_at(1, APPROVED_UNTIL, 3),
            false,
        ),
        (
            "re-approved: a new generation and a new expiry",
            device_list_at(1, renewed, 3),
            false,
        ),
        ("revoked", device_list_at(0, APPROVED_UNTIL, 3), false),
    ] {
        let cloud = fake_cloud().await;
        let vault = paired_vault(&cloud.origin);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
        service.action(Action::Enable {}).await.unwrap();
        pending_browser(&cloud);
        service.action(Action::Refresh {}).await.unwrap();
        let granted = approve(&service, true).await;
        assert_eq!(
            granted.transmit_permissions,
            [BROWSER],
            "{case}: positive control"
        );
        settle().await;
        drop(service);

        *cloud.devices.lock().unwrap() = after_restart;
        let service = launch(&cloud, &vault, &engine);
        let status = if restored {
            eventually(&service, case, |s| {
                s.station_permissions == [BROWSER]
                    && s.logging_permissions == [BROWSER]
                    && s.transmit_permissions == [BROWSER]
            })
            .await
        } else {
            eventually(&service, "Remote back on", on).await;
            settle().await;
            let status = service.action(Action::Refresh {}).await.unwrap();
            assert!(
                status.station_permissions.is_empty()
                    && status.logging_permissions.is_empty()
                    && status.transmit_permissions.is_empty(),
                "{case}: nothing comes back: {}",
                serde_json::to_string(&status).unwrap()
            );
            status
        };
        assert!(on(&status), "{case}");
        assert!(
            transmitter_idle(&engine),
            "{case}: the TX-enable latch is off and nothing is keyed"
        );
    }
}

/// A permission changed after the service renewed an approval is a change to THAT approval, so the
/// browser keeps the rest of what it was given.
#[tokio::test(flavor = "multi_thread")]
async fn a_permission_change_after_a_renewal_keeps_the_rest_of_that_approval() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    pending_browser(&cloud);
    service.action(Action::Refresh {}).await.unwrap();
    approve(&service, true).await;
    *cloud.devices.lock().unwrap() = device_list_at(1, APPROVED_UNTIL + 10 * DAY_MS, 2);
    service.action(Action::Refresh {}).await.unwrap();
    for allow in [false, true] {
        service
            .action(Action::StationPermission {
                device_id: BROWSER.into(),
                allow,
            })
            .await
            .unwrap();
    }
    let now = service.status().unwrap();
    assert_eq!(now.station_permissions, [BROWSER]);
    assert_eq!(now.logging_permissions, [BROWSER]);
    assert!(
        now.transmit_permissions.is_empty(),
        "revoking station control took transmit with it"
    );
    settle().await;
    drop(service);

    let service = launch(&cloud, &vault, &engine);
    let status = eventually(&service, "the renewed approval's grants", |s| {
        s.station_permissions == [BROWSER] && s.logging_permissions == [BROWSER]
    })
    .await;
    assert!(status.transmit_permissions.is_empty());
    assert!(transmitter_idle(&engine));
}

/// A record 1.12.0 wrote holds no generation. This build still reads it, restores it for a browser
/// the service lists at the same expiry, and binds it to the generation the service reports, so a
/// later renewal keeps it. The service never renews an approval an older Nexus gave, so for such a
/// record a listed expiry that moved means a new approval, and that browser gets nothing back.
#[tokio::test(flavor = "multi_thread")]
async fn a_record_written_by_1_12_0_still_restores_and_is_rebound_to_the_generation() {
    for (case, listed, restored) in [
        (
            "the same approval",
            device_list_at(1, APPROVED_UNTIL, 2),
            true,
        ),
        (
            "a different approval",
            device_list_at(1, APPROVED_UNTIL + DAY_MS, 2),
            false,
        ),
    ] {
        let cloud = fake_cloud().await;
        let vault = paired_vault(&cloud.origin);
        let previous = json!({"binding":{"origin":cloud.origin,"stationId":STATION,"accountId":ACCOUNT},
            "enabled":true,"grants":[{"deviceId":BROWSER,"expiresAt":APPROVED_UNTIL,"logging":true,"control":true}]});
        vault
            .values
            .lock()
            .unwrap()
            .insert("state".into(), previous.to_string());
        *cloud.devices.lock().unwrap() = listed;
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        if !restored {
            eventually(&service, "Remote back on", on).await;
            settle().await;
            let status = service.action(Action::Refresh {}).await.unwrap();
            assert!(
                status.station_permissions.is_empty() && status.logging_permissions.is_empty(),
                "{case}: {}",
                serde_json::to_string(&status).unwrap()
            );
            assert!(transmitter_idle(&engine), "{case}");
            continue;
        }
        eventually(&service, case, |s| {
            s.station_permissions == [BROWSER] && s.logging_permissions == [BROWSER]
        })
        .await;
        settle().await;
        let record: serde_json::Value =
            serde_json::from_str(&vault.values.lock().unwrap()["state"]).unwrap();
        assert_eq!(
            record["grants"][0]["generation"],
            json!(2),
            "{case}: rebound to the approval generation: {record}"
        );
        drop(service);

        *cloud.devices.lock().unwrap() = device_list_at(1, APPROVED_UNTIL + 10 * DAY_MS, 2);
        let service = launch(&cloud, &vault, &engine);
        let status = eventually(&service, "restored after a renewal", |s| {
            s.station_permissions == [BROWSER] && s.logging_permissions == [BROWSER]
        })
        .await;
        assert!(status.transmit_permissions.is_empty(), "{case}");
        assert!(transmitter_idle(&engine), "{case}");
    }
}

/// Downgrade safety, as behaviour. An older build reads a record carrying a field it does not know
/// (1.12.0 and the generation, say) through the same `state()` path as this one, which treats it as
/// unreadable. That leaves Remote OFF, restores nothing, and nothing can transmit. Shown with a field
/// THIS build does not know, beside a positive control that the same record, readable, restores.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_remembered_record_leaves_remote_off_with_nothing_restored() {
    for unreadable in [false, true] {
        let cloud = fake_cloud().await;
        let vault = paired_vault(&cloud.origin);
        let mut grant = json!({"deviceId":BROWSER,"expiresAt":APPROVED_UNTIL,"generation":2,
            "logging":true,"control":true,"transmit":true});
        if unreadable {
            grant["fromANewerBuild"] = json!(1);
        }
        let record = json!({"binding":{"origin":cloud.origin,"stationId":STATION,"accountId":ACCOUNT},
            "enabled":true,"grants":[grant]});
        vault
            .values
            .lock()
            .unwrap()
            .insert("state".into(), record.to_string());
        *cloud.devices.lock().unwrap() = device_list_at(1, APPROVED_UNTIL, 2);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        if !unreadable {
            eventually(
                &service,
                "positive control: a readable record restores",
                |s| s.transmit_permissions == [BROWSER],
            )
            .await;
            assert!(transmitter_idle(&engine));
            continue;
        }
        eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
        settle().await;
        let status = service.action(Action::Refresh {}).await.unwrap();
        assert_eq!(status.phase, "disabled", "Remote stays off");
        assert!(status.observation_generation.is_none());
        assert!(
            status.station_permissions.is_empty()
                && status.logging_permissions.is_empty()
                && status.transmit_permissions.is_empty(),
            "nothing restored: {}",
            serde_json::to_string(&status).unwrap()
        );
        assert!(transmitter_idle(&engine), "nothing can transmit");
    }
}

/// Downgrade safety. The pairing record is untouched; the remembered-state record written without
/// a transmit grant is still readable by the 1.12.0 build (its `Grant` had no transmit field and
/// denies unknown fields), and one holding a transmit grant reads as unreadable there, which
/// leaves Remote off rather than handing that build a grant it has no rule for.
#[test]
fn a_record_without_a_transmit_grant_stays_readable_by_the_previous_build() {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    #[allow(dead_code)]
    struct PreviousGrant {
        device_id: String,
        expires_at: u64,
        logging: bool,
        control: bool,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    #[allow(dead_code)]
    struct PreviousState {
        binding: Binding,
        enabled: bool,
        grants: Vec<PreviousGrant>,
    }
    let state = |transmit, generation| vault::State {
        binding: Binding {
            origin: REMOTE_ORIGIN.into(),
            station_id: STATION.into(),
            account_id: ACCOUNT.into(),
        },
        enabled: true,
        grants: vec![vault::Grant {
            device_id: BROWSER.into(),
            expires_at: APPROVED_UNTIL,
            generation,
            logging: true,
            control: true,
            transmit,
        }],
    };
    let without = serde_json::to_string(&state(false, None)).unwrap();
    assert!(!without.contains("transmit") && !without.contains("generation"));
    assert!(serde_json::from_str::<PreviousState>(&without).is_ok());
    let with = serde_json::to_string(&state(true, None)).unwrap();
    assert!(serde_json::from_str::<PreviousState>(&with).is_err());
    // A grant bound to an approval generation (a service with the approval lifetime) is unreadable
    // there too, which that build answers with Remote off and nothing restored; see
    // an_unreadable_remembered_record_leaves_remote_off_with_nothing_restored.
    let bound = serde_json::to_string(&state(false, Some(2))).unwrap();
    assert!(bound.contains("\"generation\":2"));
    assert!(serde_json::from_str::<PreviousState>(&bound).is_err());
    let read: vault::State = serde_json::from_str(&bound).unwrap();
    assert!(read == state(false, Some(2)));
    // And this build reads a record the previous build wrote.
    let previous = json!({"binding":{"origin":REMOTE_ORIGIN,"stationId":STATION,"accountId":ACCOUNT},
        "enabled":true,"grants":[{"deviceId":BROWSER,"expiresAt":APPROVED_UNTIL,"logging":true,"control":true}]});
    let read: vault::State = serde_json::from_value(previous).unwrap();
    assert!(read == state(false, None));
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
        generation: None,
        renews_until: None,
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
