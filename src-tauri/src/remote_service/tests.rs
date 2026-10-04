//! The cloud runtime test invokes the ignored probe through piped stdin/stdout.
//! Only this test substitutes a vault and synthetic readings. No test mode, local
//! origin allowance, synthetic RF data or credential export exists in the app.
use super::*;
use crate::remote_service::stored_log_tests::StoredLog;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tempo_app::{dto::AmpStatusDto, engine::Engine, settings::Settings};

/// Remote over this network beside the hosted mode: a paired computer's grant through hosted
/// decisions.
mod lan_grants;

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
    fn pins(&self) -> Result<Option<vault::Pins>, &'static str> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("credentialStoreUnavailable");
        }
        Ok(self
            .values
            .lock()
            .unwrap()
            .get("pins")
            .and_then(|v| serde_json::from_str(v).ok()))
    }
    fn save_pins(&self, pins: &vault::Pins) -> Result<(), &'static str> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("credentialStoreUnavailable");
        }
        self.values
            .lock()
            .unwrap()
            .insert("pins".into(), serde_json::to_string(pins).unwrap());
        Ok(())
    }
    fn remove_pins(&self) -> Result<(), &'static str> {
        self.values.lock().unwrap().remove("pins");
        Ok(())
    }
    fn station_key(&self) -> Result<Option<vault::StationKey>, &'static str> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("credentialStoreUnavailable");
        }
        Ok(self
            .values
            .lock()
            .unwrap()
            .get("stationKey")
            .and_then(|v| serde_json::from_str(v).ok()))
    }
    fn save_station_key(&self, key: &vault::StationKey) -> Result<(), &'static str> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("credentialStoreUnavailable");
        }
        self.values
            .lock()
            .unwrap()
            .insert("stationKey".into(), serde_json::to_string(key).unwrap());
        Ok(())
    }
    fn remove_station_key(&self) -> Result<(), &'static str> {
        self.values.lock().unwrap().remove("stationKey");
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
        needs: Default::default(),
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
            stream: Default::default(),
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
                "records":e.stored_records().into_iter().map(tempo_app::dto::LoggedQso::from).collect::<Vec<_>>(),"adif":std::fs::read_to_string(root.join("ft-contacts.adi")).unwrap_or_default(),
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
                json!({"count":e.stored_log().len(),"adif":std::fs::read_to_string(path).unwrap_or_default(),"txEnabled":e.snapshot().radio.tx_enabled})
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
                "logCount":e.stored_log().len()})
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
                    e.log_revision(),
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
                json!({"prop":serde_json::from_slice::<serde_json::Value>(&serde_json::to_vec(&prop).unwrap()).unwrap(),"tleCount":expected_tles,"logCount":e.stored_log().len(),"live":query::navigation::live(&e).unwrap()})
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
                json!({"state":e.js8_state(),"log":e.stored_records().into_iter().map(|r| {
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
                log: e.log_revision(),
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
                "txEnabled": e.snapshot().radio.tx_enabled, "logCount": e.stored_records().len() })
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
            crate::pouncer::run(
                engine.clone(),
                Default::default(),
                rx,
                pounce_recent.clone(),
                |p| fired.push(p),
            );
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
                })
                .expect("the test's log reads"),
            );
            let log_count = e.stored_records().len();
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
            let records = e.stored_records();
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
                        stream: Default::default(),
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
                cancellation, &engine, &transport::Feeds { monitor: publisher.clone(), spectrum: None, meters: Default::default(), sources: None, #[cfg(feature = "radio")] audio: None, stream: Default::default() }, &status).await;
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
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), spectrum: None, meters: Default::default(), sources: None, #[cfg(feature = "radio")] audio: None, stream: Default::default() };
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
        // The browser's key, pinned at the radio: its acquire carries a proof (S1-M1). Stop never does.
        let key = LaneKey::new();
        let status = pinned_station(&key, DEVICE);
        let authority = status.control.lock().unwrap().operations.clone();
        authority.permit_station(DEVICE, true).unwrap();
        let (cancel, cancellation) = watch::channel(false);
        let relay_engine = engine.clone();
        let relay = tokio::spawn(async move {
            let mut socket = tokio_tungstenite::accept_async(relay_io).await.unwrap();
            let operation = |request: serde_json::Value| Message::Text(json!({"type":"operationRequest","sessionId":SESSION,"deviceId":DEVICE,"operationVersion":4,"request":request}).to_string().into());
            let signed = |request: serde_json::Value, seq: u64| {
                let proof = key.proof(LANE_OPERATION, &request.to_string(), [LANE_STATION, DEVICE, SESSION], seq);
                Message::Text(json!({"type":"operationRequest","sessionId":SESSION,"deviceId":DEVICE,"operationVersion":4,"request":request,"proof":proof}).to_string().into())
            };
            let id = |n: u32| format!("00000000-0000-4000-8000-0000000000{n:02x}");
            // A controlling v4 browser with the stop token, and the FT latch armed at the shack.
            socket.send(operation(json!({"type":"state","requestId":id(1)}))).await.unwrap();
            let state = text(&mut socket).await;
            let boot = state["value"]["stationBootId"].as_str().unwrap().to_owned();
            socket.send(signed(json!({"type":"acquire","requestId":id(2),"stationBootId":boot}), 1)).await.unwrap();
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
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), spectrum: None, meters: Default::default(), sources: None, #[cfg(feature = "radio")] audio: None, stream: Default::default() };
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
            // The browser's key, pinned at the radio: what it sends carries a proof (S1-M1).
            let key = LaneKey::new();
            let status = pinned_station(&key, DEVICE);
            let authority = status.control.lock().unwrap().operations.clone();
            authority.permit_station(DEVICE, true).unwrap();
            let (cancel, cancellation) = watch::channel(false);
            let relay_engine = engine.clone();
            let relay = tokio::spawn(async move {
                let mut socket = tokio_tungstenite::accept_async(relay_io).await.unwrap();
                let mut seq = 0;
                let mut operation = |request: serde_json::Value| {
                    seq += 1;
                    let proof = key.proof(LANE_OPERATION, &request.to_string(), [LANE_STATION, DEVICE, SESSION], seq);
                    Message::Text(json!({"type":"operationRequest","sessionId":SESSION,"deviceId":DEVICE,"operationVersion":version,"request":request,"proof":proof}).to_string().into())
                };
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
                assert!(work.commit_tuning_readback(&mut tempo_app::engine::engine_lock(&relay_engine), power, None));
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
            let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), spectrum: None, meters: Default::default(), sources: None, audio: None, stream: Default::default() };
            let served = transport::serve(socket, cancellation, &engine, &feeds, &status).await;
            relay.await.unwrap();
            assert_eq!(served, Ok(()));
            let _ = std::fs::remove_dir_all(&dir);
        });
    }
}

// ---- The relay's older lanes, bound to the browser's own key (security review S1-M1) -----------
//
// The relay stamps `sessionId` and `deviceId` on everything it forwards, so on its word alone a
// relay that is not the one the operator trusts could act as any approved browser: take control,
// keep a lease alive, transmit FT where it is granted, write the log, listen. The stream's offer
// has carried the browser's signature since A5; these hold the older lanes to the same key. Every
// message here is written byte for byte, and each proof is made the way the page makes one, so
// the station is held to the bytes the relay hands it.

const LANE_STATION: &str = "30000000-0000-4000-8000-0000000000a1";
const LANE_DEVICE: &str = "10000000-0000-4000-8000-0000000000a1";
const LANE_SESSION: &str = "20000000-0000-4000-8000-0000000000a1";

/// A browser's device key for these tests, played by ring: the page's non-extractable WebCrypto
/// key. Made for the run and never written down.
struct LaneKey {
    pair: ring::signature::EcdsaKeyPair,
    spki: String,
}
impl LaneKey {
    fn new() -> Self {
        use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        let spki = format!(
            "{}{}",
            tempo_stream::protocol::P256_SPKI_PREFIX_HEX,
            hex(pair.public_key().as_ref())
        );
        Self { pair, spki }
    }
    /// What the operator pins at the radio: SHA-256 of the SPKI, lowercase hex.
    fn pin(&self) -> String {
        let spki = tempo_stream::protocol::hex_bytes(&self.spki).unwrap();
        hex(ring::digest::digest(&ring::digest::SHA256, &spki).as_ref())
    }
    /// The proof the page attaches to one lane message: its signature over the lane's label,
    /// SHA-256 of the message's own bytes, the station, browser and session ids, and `seq`.
    fn proof(&self, label: &str, body: &str, ids: [&str; 3], seq: u64) -> serde_json::Value {
        let mut signed = label.as_bytes().to_vec();
        signed.extend_from_slice(
            ring::digest::digest(&ring::digest::SHA256, body.as_bytes()).as_ref(),
        );
        for id in ids {
            signed.extend_from_slice(id.as_bytes());
        }
        signed.extend_from_slice(&seq.to_be_bytes());
        let signature = self
            .pair
            .sign(&ring::rand::SystemRandom::new(), &signed)
            .unwrap();
        json!({"publicKey": self.spki, "seq": seq, "signature": hex(signature.as_ref())})
    }
}

/// What an operation request's proof begins with.
const LANE_OPERATION: &str = "nexus-operation/1";

/// A station paired as `LANE_STATION`, Remote on, and `device`'s key pinned at the radio.
fn pinned_station(key: &LaneKey, device: &str) -> SessionStatus {
    let status = SessionStatus {
        status: Arc::new(Mutex::new(Status::default())),
        control: Arc::new(Mutex::new(Control {
            enabled: true,
            ..Default::default()
        })),
        generation: 0,
    };
    {
        let mut control = status.control.lock().unwrap();
        control.remembered.binding = Some(Binding {
            origin: "http://127.0.0.1:9/".into(),
            station_id: LANE_STATION.into(),
            account_id: ACCOUNT.into(),
        });
        control.remembered.pins.insert(device.into(), key.pin());
    }
    status
}

/// [`pinned_station`] for `LANE_DEVICE`, granted logging and station control at the radio.
fn lane_station(key: &LaneKey) -> SessionStatus {
    let status = pinned_station(key, LANE_DEVICE);
    {
        let control = status.control.lock().unwrap();
        control.operations.permit(LANE_DEVICE, true).unwrap();
        control
            .operations
            .permit_station(LANE_DEVICE, true)
            .unwrap();
    }
    status
}

async fn lane_text(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
    within: Duration,
) -> Option<serde_json::Value> {
    use futures_util::StreamExt;
    loop {
        let next = tokio::time::timeout(within, socket.next()).await.ok()??;
        if let tokio_tungstenite::tungstenite::Message::Text(text) = next.unwrap() {
            return Some(serde_json::from_str(&text).unwrap());
        }
    }
}

/// Send one message as the relay and read the station's answer to it.
async fn lane_exchange(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
    text: String,
) -> serde_json::Value {
    use futures_util::SinkExt;
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
        .await
        .unwrap();
    lane_text(socket, Duration::from_secs(3))
        .await
        .expect("the station answered")
}

/// An operation request as the relay hands it on: `request` exactly as the page wrote it.
fn lane_operation(
    request: &serde_json::Value,
    device: &str,
    proof: Option<serde_json::Value>,
) -> String {
    let proof = proof
        .map(|p| format!(r#","proof":{p}"#))
        .unwrap_or_default();
    format!(
        r#"{{"type":"operationRequest","sessionId":"{LANE_SESSION}","deviceId":"{device}","operationVersion":4,"request":{request}{proof}}}"#
    )
}

#[test]
fn s1m1_the_operation_lane_takes_only_what_the_pinned_device_key_signed() {
    // A stopTransmit reaches the satellite track badge (see the slow-link Stop test above).
    let _alone = alone();
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
    const LABEL: &str = "nexus-operation/1";
    const OTHER: &str = "40000000-0000-4000-8000-0000000000a1";
    const UNPINNED: &str = "10000000-0000-4000-8000-0000000000a2";
    let key = LaneKey::new();
    let forger = LaneKey::new();
    let ids = [LANE_STATION, LANE_DEVICE, LANE_SESSION];
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (station_io, relay_io) = tokio::io::duplex(65536);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let status = lane_station(&key);
        status.control.lock().unwrap().operations.permit_station(UNPINNED, true).unwrap();
        let (cancel, cancellation) = watch::channel(false);
        let relay = tokio::spawn(async move {
            let mut socket = tokio_tungstenite::accept_async(relay_io).await.unwrap();
            let id = |n: u32| format!("00000000-0000-4000-8000-0000000001{n:02x}");
            // `state` needs no proof: it grants nothing, and Stop is composed from it.
            let state = lane_exchange(&mut socket, lane_operation(&json!({"type":"state","requestId":id(1)}), LANE_DEVICE, None)).await;
            let boot = state["value"]["stationBootId"].as_str().expect("an unsigned state is answered").to_owned();
            let acquire = |n| json!({"type":"acquire","requestId":id(n),"stationBootId":boot});

            // THE FINDING: an acquire with no proof. The station took it on the relay's word.
            let unsigned = lane_exchange(&mut socket, lane_operation(&acquire(2), LANE_DEVICE, None)).await;
            assert_eq!(unsigned["error"], "stationUnsupported", "an unsigned acquire is refused: {unsigned}");
            // Signed, but by a key the operator never pinned for this browser.
            let forged = lane_exchange(&mut socket, lane_operation(&acquire(3), LANE_DEVICE, Some(forger.proof(LABEL, &acquire(3).to_string(), ids, 1)))).await;
            assert_eq!(forged["error"], "deviceKeyMismatch", "{forged}");
            // The browser's own key, but made for another station, another session, or another request.
            let other_station = key.proof(LABEL, &acquire(4).to_string(), [OTHER, LANE_DEVICE, LANE_SESSION], 2);
            assert_eq!(lane_exchange(&mut socket, lane_operation(&acquire(4), LANE_DEVICE, Some(other_station))).await["error"], "deviceKeyMismatch");
            let other_session = key.proof(LABEL, &acquire(5).to_string(), [LANE_STATION, LANE_DEVICE, OTHER], 3);
            assert_eq!(lane_exchange(&mut socket, lane_operation(&acquire(5), LANE_DEVICE, Some(other_session))).await["error"], "deviceKeyMismatch");
            let other_request = key.proof(LABEL, &json!({"type":"state","requestId":id(6)}).to_string(), ids, 4);
            assert_eq!(lane_exchange(&mut socket, lane_operation(&acquire(6), LANE_DEVICE, Some(other_request))).await["error"], "deviceKeyMismatch");
            // A browser the station holds no pin for (approved before keys existed) is told so by name.
            let unpinned = LaneKey::new().proof(LABEL, &acquire(7).to_string(), [LANE_STATION, UNPINNED, LANE_SESSION], 1);
            assert_eq!(lane_exchange(&mut socket, lane_operation(&acquire(7), UNPINNED, Some(unpinned))).await["error"], "deviceNotPinned");

            // POSITIVE CONTROL: the same acquire, signed as the page signs it, takes control.
            let signed = lane_operation(&acquire(8), LANE_DEVICE, Some(key.proof(LABEL, &acquire(8).to_string(), ids, 5)));
            let owner = lane_exchange(&mut socket, signed.clone()).await;
            assert_eq!(owner["value"]["phase"], "controlling", "{owner}");
            let lease = owner["value"]["leaseId"].as_str().unwrap().to_owned();
            // A replay, byte for byte, and a fresh request under a sequence already spent: both dropped.
            socket.send(Message::Text(signed.into())).await.unwrap();
            let heartbeat = |n| json!({"type":"heartbeat","requestId":id(n),"leaseId":lease});
            socket.send(Message::Text(lane_operation(&heartbeat(9), LANE_DEVICE, Some(key.proof(LABEL, &heartbeat(9).to_string(), ids, 5))).into())).await.unwrap();
            assert_eq!(lane_text(&mut socket, Duration::from_millis(500)).await, None, "a replayed proof is never answered");

            // Heartbeats keep a lease, and FT transmit with it: unsigned, refused; signed, renewed.
            assert_eq!(lane_exchange(&mut socket, lane_operation(&heartbeat(10), LANE_DEVICE, None)).await["error"], "stationUnsupported");
            let renewed = lane_exchange(&mut socket, lane_operation(&heartbeat(11), LANE_DEVICE, Some(key.proof(LABEL, &heartbeat(11).to_string(), ids, 6)))).await;
            assert_eq!(renewed["value"]["phase"], "controlling", "{renewed}");
            let epoch = renewed["value"]["transmitEpoch"].as_str().expect("the controller holds the stop token").to_owned();
            // A release the browser did not sign releases nothing.
            let release = |n| json!({"type":"release","requestId":id(n),"leaseId":lease});
            assert_eq!(lane_exchange(&mut socket, lane_operation(&release(12), LANE_DEVICE, None)).await["error"], "stationUnsupported");
            let forged = forger.proof(LABEL, &release(13).to_string(), ids, 7);
            assert_eq!(lane_exchange(&mut socket, lane_operation(&release(13), LANE_DEVICE, Some(forged))).await["error"], "deviceKeyMismatch");
            let still = lane_exchange(&mut socket, lane_operation(&heartbeat(14), LANE_DEVICE, Some(key.proof(LABEL, &heartbeat(14).to_string(), ids, 8)))).await;
            assert_eq!(still["value"]["phase"], "controlling", "the refused releases released nothing: {still}");
            // Stop never needs a proof: a forged one only stops.
            let stop = lane_exchange(&mut socket, lane_operation(&json!({"type":"stopTransmit","requestId":id(15),"stationBootId":boot,"leaseId":lease,"transmitEpoch":epoch}), LANE_DEVICE, None)).await;
            assert_eq!(stop["value"], json!({"stop":"accepted"}), "{stop}");
            // The signed release does release.
            let released = lane_exchange(&mut socket, lane_operation(&release(16), LANE_DEVICE, Some(key.proof(LABEL, &release(16).to_string(), ids, 9)))).await;
            assert_eq!(released["value"]["phase"], "available", "{released}");
            cancel.send(true).unwrap();
        });
        let request = "ws://localhost/api/remote/stations/x/connect".into_client_request().unwrap();
        let (socket, _) = tokio_tungstenite::client_async_with_config(request, station_io, Some(transport::socket_config())).await.unwrap();
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), spectrum: None, meters: Default::default(), sources: None, #[cfg(feature = "radio")] audio: None, stream: Default::default() };
        let served = transport::serve(socket, cancellation, &engine, &feeds, &status).await;
        relay.await.unwrap();
        assert_eq!(served, Ok(()), "the session ended because Remote stopped");
    });
}

#[test]
#[cfg(feature = "radio")]
fn s1m1_listen_takes_only_what_the_pinned_device_key_signed() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    const OPERATION: &str = "nexus-operation/1";
    const LISTEN: &str = "nexus-listen/1";
    let key = LaneKey::new();
    let forger = LaneKey::new();
    let ids = [LANE_STATION, LANE_DEVICE, LANE_SESSION];
    let feed = tempo_audio::receive_encode::DetachedFeed::new(48_000);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (station_io, relay_io) = tokio::io::duplex(65536);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let status = lane_station(&key);
        let (cancel, cancellation) = watch::channel(false);
        let relay = tokio::spawn(async move {
            let mut socket = tokio_tungstenite::accept_async(relay_io).await.unwrap();
            let id = |n: u32| format!("00000000-0000-4000-8000-0000000002{n:02x}");
            let state = lane_exchange(&mut socket, lane_operation(&json!({"type":"state","requestId":id(1)}), LANE_DEVICE, None)).await;
            let acquire = json!({"type":"acquire","requestId":id(2),"stationBootId":state["value"]["stationBootId"]});
            let owner = lane_exchange(&mut socket, lane_operation(&acquire, LANE_DEVICE, Some(key.proof(OPERATION, &acquire.to_string(), ids, 1)))).await;
            let lease = owner["value"]["leaseId"].as_str().expect("a signed acquire takes control").to_owned();
            // The canonical body a Listen proof covers: what it asks, and under which lease.
            let body = |listening: bool, lease: &str| format!(r#"{{"listening":{listening},"leaseId":"{lease}"}}"#);
            let listen = |lease: &str, proof: Option<serde_json::Value>| {
                let proof = proof.map(|p| format!(r#","proof":{p}"#)).unwrap_or_default();
                format!(r#"{{"type":"audioListen","sessionId":"{LANE_SESSION}","deviceId":"{LANE_DEVICE}","listening":true,"leaseId":"{lease}"{proof}}}"#)
            };
            // THE FINDING: Listen on the relay's word alone.
            let unsigned = lane_exchange(&mut socket, listen(&lease, None)).await;
            assert_eq!(unsigned["type"], "audioState");
            assert_eq!(unsigned["listening"], false, "an unsigned Listen is refused: {unsigned}");
            let forged = lane_exchange(&mut socket, listen(&lease, Some(forger.proof(LISTEN, &body(true, &lease), ids, 2)))).await;
            assert_eq!((forged["listening"].clone(), forged["reason"].clone()), (json!(false), json!("deviceKeyMismatch")), "{forged}");
            // A proof for an operation is not a proof for Listen, and the lease it names is covered.
            let operation_label = key.proof(OPERATION, &body(true, &lease), ids, 3);
            assert_eq!(lane_exchange(&mut socket, listen(&lease, Some(operation_label))).await["reason"], "deviceKeyMismatch");
            let other_lease = key.proof(LISTEN, &body(true, LANE_STATION), ids, 4);
            assert_eq!(lane_exchange(&mut socket, listen(&lease, Some(other_lease))).await["reason"], "deviceKeyMismatch");
            // POSITIVE CONTROL: signed as the page signs it, the station listens.
            let heard = lane_exchange(&mut socket, listen(&lease, Some(key.proof(LISTEN, &body(true, &lease), ids, 5)))).await;
            assert_eq!((heard["listening"].clone(), heard.get("reason").cloned()), (json!(true), None), "{heard}");
            cancel.send(true).unwrap();
        });
        let request = "ws://localhost/api/remote/stations/x/connect".into_client_request().unwrap();
        let (socket, _) = tokio_tungstenite::client_async_with_config(request, station_io, Some(transport::socket_config())).await.unwrap();
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), spectrum: None, meters: Default::default(), sources: None, audio: Some(audio::ReceiveFanout::new(feed.feed.clone())), stream: Default::default() };
        let served = transport::serve(socket, cancellation, &engine, &feeds, &status).await;
        relay.await.unwrap();
        assert_eq!(served, Ok(()), "the session ended because Remote stopped");
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
    /// The signing key the service holds for the station (S3-M1): the first one sent to it.
    station_key: Arc<Mutex<Option<String>>>,
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
    let station_key = Arc::new(Mutex::new(None::<String>));
    let held = station_key.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let served = served.clone();
            let held = held.clone();
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
                let keys = head.contains("\r\nx-nexus-device-key: 1\r\n");
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
                    // The service approves the browser that confirmed the pairing along with it, with
                    // the device key its confirm brought (A5), if it brought one: here, the key the
                    // browser list names for it.
                    let mut device = if lifetime {
                        json!({"id":BROWSER,"expiresAt":APPROVED_UNTIL,"generation":2})
                    } else {
                        json!({"id":BROWSER,"expiresAt":APPROVED_UNTIL})
                    };
                    let confirmed =
                        serde_json::from_str::<serde_json::Value>(&served.lock().unwrap())
                            .ok()
                            .and_then(|l| {
                                l["devices"][0]["publicKey"].as_str().map(str::to_string)
                            });
                    if let Some(key) = confirmed {
                        device["publicKey"] = key.into();
                    }
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
                    if !keys {
                        // The device key (A5), likewise only to a Nexus that asks.
                        for device in listed["devices"].as_array_mut().unwrap() {
                            device.as_object_mut().unwrap().remove("publicKey");
                        }
                    }
                    ("200 OK", listed.to_string())
                } else if head.contains("/native/approve-device ") {
                    // The service writes a new approval, and with it the approval generation. The
                    // browser's device key (A5) is the browser's, and approving leaves it as it is.
                    let key = serde_json::from_str::<serde_json::Value>(&served.lock().unwrap())
                        .ok()
                        .and_then(|l| l["devices"][0]["publicKey"].as_str().map(str::to_string));
                    let mut approved: serde_json::Value = serde_json::from_str(&if lifetime {
                        device_list_at(1, APPROVED_UNTIL, 2)
                    } else {
                        device_list(1, APPROVED_UNTIL)
                    })
                    .unwrap();
                    if let Some(key) = key {
                        approved["devices"][0]["publicKey"] = key.into();
                    }
                    *served.lock().unwrap() = approved.to_string();
                    ("200 OK", r#"{"ok":true}"#.to_string())
                } else if head.contains("/native/revoke-device ") {
                    // A revoked browser's expiry becomes "now", so the service stops listing it.
                    *served.lock().unwrap() = json!({"devices":[]}).to_string();
                    ("200 OK", r#"{"ok":true}"#.to_string())
                } else if head.contains("/native/revoke ") {
                    ("200 OK", r#"{"ok":true}"#.to_string())
                } else if head.contains("/native/key ") {
                    // S3-M1: the first key a station sends is kept; the same one again is fine; any
                    // other is refused by name.
                    let sent = serde_json::from_slice::<serde_json::Value>(&request[head_end..])
                        .ok()
                        .and_then(|v| v["publicKey"].as_str().map(str::to_string));
                    let mut held = held.lock().unwrap();
                    match (sent, held.clone()) {
                        (Some(sent), None) => {
                            *held = Some(sent);
                            ("200 OK", r#"{"ok":true}"#.to_string())
                        }
                        (Some(sent), Some(kept)) if sent == kept => {
                            ("200 OK", r#"{"ok":true}"#.to_string())
                        }
                        (Some(_), Some(_)) => (
                            "409 Conflict",
                            r#"{"error":"stationKeyPinned"}"#.to_string(),
                        ),
                        (None, _) => (
                            "400 Bad Request",
                            r#"{"error":"invalidRequest"}"#.to_string(),
                        ),
                    }
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
    FakeCloud {
        origin,
        devices,
        station_key,
    }
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
            key: None,
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
                    key: None,
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
        public_key: None,
        key: None,
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
        station_key: Default::default(),
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

// ---- The device key (security test A5) --------------------------------------------------------------
//
// The operator pins a browser's device key when they approve it at the radio: the SHA-256 of the key
// the station showed, and only if the service still lists that key once it has approved. A pin is
// the approval's, not a permission's: it outlives the switches, take over, Turn off and a restart,
// and revoking the browser removes it. The pins have their own vault entry (operator ruling D6,
// 2026-09-28), so the state record an older Nexus reads keeps the shape it has always had.

use super::stream::tests::DeviceKey;

/// A browser the service lists as waiting for approval, with its device key.
fn pending_keyed_browser(cloud: &FakeCloud, public_key: &str) {
    let mut listed: serde_json::Value =
        serde_json::from_str(&device_list(0, APPROVED_UNTIL - 1)).unwrap();
    listed["devices"][0]["publicKey"] = public_key.into();
    *cloud.devices.lock().unwrap() = listed.to_string();
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
/// Approve the browser here, with the key the operator was shown beside it.
async fn approve_shown(service: &Service, shown: Option<String>, transmit: bool) -> Status {
    service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: true,
            transmit,
            key: shown,
        })
        .await
        .unwrap()
}
fn pinned(service: &Service) -> Option<[u8; 32]> {
    service.control.lock().unwrap().remembered.pinned(BROWSER)
}

/// ★ A5: approving a browser pins the key the station showed beside it, and only that key: a key
/// that changed between the showing and the approval (the service lists another) pins nothing, and
/// neither does an approval with no key shown. The browser is approved either way.
#[tokio::test(flavor = "multi_thread")]
async fn a5_approving_a_browser_pins_the_key_it_was_shown_and_only_that_key() {
    for case in [
        "positive control: the key shown",
        "another key shown",
        "no key shown",
    ] {
        let cloud = fake_cloud().await;
        let vault = paired_vault(&cloud.origin);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
        service.action(Action::Enable {}).await.unwrap();
        let key = DeviceKey::new();
        let fingerprint = hex(&key.pin());
        pending_keyed_browser(&cloud, &key.public_key);
        let listed = service.action(Action::Refresh {}).await.unwrap();
        assert_eq!(
            listed.devices[0].key.as_deref(),
            Some(fingerprint.as_str()),
            "{case}: the station shows the listed key"
        );
        assert!(
            listed.pinned_devices.is_empty(),
            "{case}: pinned before approval"
        );
        let shown = if case.starts_with("positive") {
            Some(fingerprint.clone())
        } else if case.starts_with("another") {
            Some(hex(&DeviceKey::new().pin()))
        } else {
            None
        };
        let status = approve_shown(&service, shown, false).await;
        assert_eq!(
            status.station_permissions,
            [BROWSER],
            "{case}: approved either way"
        );
        let pin = case.starts_with("positive");
        let expected: &[&str] = if pin { &[BROWSER] } else { &[] };
        assert_eq!(status.pinned_devices, expected, "{case}");
        assert_eq!(
            pinned(&service),
            pin.then(|| key.pin()),
            "{case}: what a stream offer is held to"
        );
        settle().await;
        assert_eq!(
            vault
                .pins()
                .unwrap()
                .and_then(|p| p.keys.get(BROWSER).cloned()),
            pin.then(|| fingerprint.clone()),
            "{case}: the pins entry"
        );
    }
}

/// ★ A5: a pin outlives the switches, take over, Turn off and a restart (it is the approval's), and
/// revoking the browser removes it, from the station and from the vault.
#[tokio::test(flavor = "multi_thread")]
async fn a5_a_pin_outlives_switches_take_over_turn_off_and_a_restart_and_revoking_removes_it() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let key = DeviceKey::new();
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    pending_keyed_browser(&cloud, &key.public_key);
    service.action(Action::Refresh {}).await.unwrap();
    approve_shown(&service, Some(hex(&key.pin())), false).await;
    assert_eq!(pinned(&service), Some(key.pin()), "premise: pinned");
    for allow in [false, true] {
        service
            .action(Action::StationPermission {
                device_id: BROWSER.into(),
                allow,
            })
            .await
            .unwrap();
    }
    assert_eq!(pinned(&service), Some(key.pin()), "a switch moved the pin");
    service.action(Action::TakeOverLogging {}).await.unwrap();
    assert_eq!(pinned(&service), Some(key.pin()), "take over moved the pin");
    service.action(Action::Disable {}).await.unwrap();
    settle().await;
    drop(service);
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    assert_eq!(
        pinned(&service),
        Some(key.pin()),
        "the pin did not survive Turn off and a restart"
    );
    let listed = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        listed.pinned_devices,
        [BROWSER],
        "shown pinned after the restart"
    );
    service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            transmit: false,
            key: None,
        })
        .await
        .unwrap();
    assert_eq!(pinned(&service), None, "revoking left the pin");
    settle().await;
    assert!(
        vault.pins().unwrap().is_none(),
        "revoking left the pins entry"
    );
}

/// ★ Operator ruling D6 (2026-09-28): the operator moves between the stream test build and normal
/// builds, and a pin must not cost an older Nexus its record. The state record this build writes,
/// with a pin made, is read by the previous build's schema (its `State`, copied here as it stood
/// before the pins) with every grant intact; the pin sits in its own entry, which that build never
/// reads. CONTROL: the same record with the pin written into it is unreadable to that schema.
#[tokio::test(flavor = "multi_thread")]
async fn a5_the_state_record_with_a_pin_made_is_still_read_by_the_previous_build() {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    #[allow(dead_code)]
    struct PreviousBinding {
        origin: String,
        station_id: String,
        account_id: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    #[allow(dead_code)]
    struct PreviousGrant {
        device_id: String,
        expires_at: u64,
        #[serde(default)]
        generation: Option<u64>,
        logging: bool,
        control: bool,
        #[serde(default)]
        transmit: bool,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct PreviousState {
        #[allow(dead_code)]
        binding: PreviousBinding,
        enabled: bool,
        grants: Vec<PreviousGrant>,
    }
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let key = DeviceKey::new();
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    pending_keyed_browser(&cloud, &key.public_key);
    service.action(Action::Refresh {}).await.unwrap();
    approve_shown(&service, Some(hex(&key.pin())), true).await;
    settle().await;
    let values = vault.values.lock().unwrap().clone();
    assert!(values.contains_key("pins"), "premise: a pin was made");
    let written = values.get("state").expect("a state record");
    let read: PreviousState =
        serde_json::from_str(written).expect("the previous build cannot read this build's record");
    assert!(read.enabled);
    let grant = read
        .grants
        .iter()
        .find(|g| g.device_id == BROWSER)
        .expect("the grant");
    assert!(
        grant.logging && grant.control && grant.transmit,
        "a grant lost"
    );
    let mut inside: serde_json::Value = serde_json::from_str(written).unwrap();
    inside["pins"] = json!({ BROWSER: hex(&key.pin()) });
    assert!(
        serde_json::from_value::<PreviousState>(inside).is_err(),
        "control: a pin inside the record would be unreadable"
    );
}

/// The pins entry fits the smallest OS credential blob (Windows: 2560 bytes of UTF-16) with a pin
/// for every browser the service lists.
#[test]
fn a5_a_full_pins_record_fits_the_windows_credential_blob() {
    let keys = (0..MAX_REMEMBERED)
        .map(|i| (format!("10000000-0000-4000-8000-{i:012}"), "f".repeat(64)))
        .collect();
    let pins = vault::Pins {
        binding: Binding {
            origin: REMOTE_ORIGIN.into(),
            station_id: STATION.into(),
            account_id: ACCOUNT.into(),
        },
        keys,
    };
    let bytes = serde_json::to_string(&pins).unwrap().encode_utf16().count() * 2;
    assert!(bytes <= 2560, "{bytes} bytes");
}

/// A5: a listed key that is not the shape of a P-256 key is no key: it shows nothing and pins
/// nothing, and the rest of the list stands. CONTROL: a real key is shown.
#[tokio::test(flavor = "multi_thread")]
async fn a5_a_listed_key_that_is_not_a_p256_key_shows_and_pins_nothing() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let key = DeviceKey::new();
    let prefix = tempo_stream::protocol::P256_SPKI_PREFIX_HEX.len();
    // The point's tag says "compressed", which no P-256 SPKI the page sends carries.
    let not_a_key = format!(
        "{}05{}",
        &key.public_key[..prefix],
        &key.public_key[prefix + 2..]
    );
    pending_keyed_browser(&cloud, &not_a_key);
    let listed = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(listed.devices.len(), 1, "the list stands");
    assert_eq!(listed.devices[0].key, None);
    pending_keyed_browser(&cloud, &key.public_key);
    let listed = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(listed.devices[0].key, Some(hex(&key.pin())), "control");
}

/// ★ A5, operator ruling D5 (2026-09-28): a browser that comes back with a new key (it lost the one
/// it had) keeps its approval and its pin, and the station shows it unpinned, with the new key, so
/// the operator can approve it again; until then its stream is refused, since the pin still holds
/// the old key. Approving again with the new key shown pins that. CONTROL: before the change it is
/// shown pinned.
#[tokio::test(flavor = "multi_thread")]
async fn a5_a_browser_that_changed_its_key_is_shown_unpinned_until_approved_again() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let (old, new) = (DeviceKey::new(), DeviceKey::new());
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    pending_keyed_browser(&cloud, &old.public_key);
    service.action(Action::Refresh {}).await.unwrap();
    let approved = approve_shown(&service, Some(hex(&old.pin())), false).await;
    assert_eq!(approved.pinned_devices, [BROWSER], "control: pinned");
    let mut listed: serde_json::Value =
        serde_json::from_str(&cloud.devices.lock().unwrap()).unwrap();
    listed["devices"][0]["publicKey"] = new.public_key.clone().into();
    *cloud.devices.lock().unwrap() = listed.to_string();
    let changed = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(changed.devices[0].approved, 1, "still approved");
    assert_eq!(
        changed.devices[0].key,
        Some(hex(&new.pin())),
        "the new key is shown"
    );
    assert!(
        changed.pinned_devices.is_empty(),
        "shown pinned with a key it does not hold"
    );
    assert_eq!(
        pinned(&service),
        Some(old.pin()),
        "the pin still holds the old key"
    );
    let again = approve_shown(&service, Some(hex(&new.pin())), false).await;
    assert_eq!(again.pinned_devices, [BROWSER]);
    assert_eq!(pinned(&service), Some(new.pin()));
}

/// ★ A5, operator ruling D4 (2026-09-28): approving the pairing pins the device key of the browser
/// that confirmed it, on first use (the same trust the pairing already places in the service). Both
/// ends show its fingerprint straight after, and revoking the browser is one click. CONTROL: a
/// pairing whose browser brought no key pins nothing, and that browser is approved again before it
/// streams.
#[tokio::test(flavor = "multi_thread")]
async fn a5_approving_the_pairing_pins_the_key_the_confirming_browser_brought() {
    for keyed in [true, false] {
        let cloud = fake_cloud().await;
        let vault = MemoryVault::default();
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let key = DeviceKey::new();
        let mut listed: serde_json::Value =
            serde_json::from_str(&device_list(1, APPROVED_UNTIL)).unwrap();
        if keyed {
            listed["devices"][0]["publicKey"] = key.public_key.clone().into();
        }
        *cloud.devices.lock().unwrap() = listed.to_string();
        let service = launch(&cloud, &vault, &engine);
        service
            .action(Action::Begin {
                name: "Test station".into(),
            })
            .await
            .unwrap();
        service.action(Action::Refresh {}).await.unwrap();
        service
            .action(Action::Approve {
                enrollment_id: STATION.into(),
                account_id: ACCOUNT.into(),
                transmit: false,
            })
            .await
            .unwrap();
        assert_eq!(pinned(&service), keyed.then(|| key.pin()), "keyed={keyed}");
        let shown = eventually(&service, "the browser list after the pairing", |s| {
            !s.devices.is_empty()
        })
        .await;
        let expected: &[&str] = if keyed { &[BROWSER] } else { &[] };
        assert_eq!(shown.pinned_devices, expected, "keyed={keyed}");
        settle().await;
        assert_eq!(
            vault
                .pins()
                .unwrap()
                .and_then(|p| p.keys.get(BROWSER).cloned()),
            keyed.then(|| hex(&key.pin())),
            "keyed={keyed}: the pins entry"
        );
    }
}

// ---- The station's own signing key (security review S3-M1) ---------------------------------------
//
// The key that signs the station's stream answers is made once for each pairing, kept in its own
// vault entry bound to that pairing, sent to the service before every connection, and goes with the
// pairing. The fake cloud keeps the first key it is sent and refuses any other, as the service does.

/// The public half of the station key the vault holds, or `None`.
fn kept_station_key(vault: &MemoryVault) -> Option<String> {
    let kept = vault.values.lock().unwrap().get("stationKey").cloned()?;
    let kept: vault::StationKey = serde_json::from_str(&kept).ok()?;
    Some(
        station_key::Signer::restore(&kept.pkcs8)?
            .public_key()
            .to_string(),
    )
}
async fn sent_station_key(cloud: &FakeCloud) -> Option<String> {
    for _ in 0..250 {
        let sent = cloud.station_key.lock().unwrap().clone();
        if sent.is_some() {
            return sent;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
async fn s3m1_approving_a_pairing_makes_its_key_which_a_restart_keeps_and_revoking_removes() {
    let cloud = fake_cloud().await;
    let vault = MemoryVault::default();
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    service
        .action(Action::Begin {
            name: "Test station".into(),
        })
        .await
        .unwrap();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(kept_station_key(&vault), None, "no key before the pairing");
    service
        .action(Action::Approve {
            enrollment_id: STATION.into(),
            account_id: ACCOUNT.into(),
            transmit: false,
        })
        .await
        .unwrap();
    let made = kept_station_key(&vault).expect("approving the pairing makes the station's key");
    assert!(tempo_stream::protocol::device_key(&made), "a P-256 SPKI");
    let kept: vault::StationKey =
        serde_json::from_str(&vault.values.lock().unwrap()["stationKey"]).unwrap();
    assert_eq!(kept.binding.station_id, STATION, "bound to this pairing");
    // Approving turned Remote on, and the service is sent the public half before the station connects.
    assert_eq!(
        sent_station_key(&cloud).await.as_deref(),
        Some(made.as_str())
    );
    let status = service.status().unwrap();
    assert!(!status.key_refused, "the service took it");
    assert!(
        !serde_json::to_string(&status)
            .unwrap()
            .contains(&kept.pkcs8),
        "the private key is in no status"
    );

    service.action(Action::Disable {}).await.unwrap();
    settle().await;
    drop(service);
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    assert_eq!(
        kept_station_key(&vault).as_deref(),
        Some(made.as_str()),
        "a restart signs with the key it made, never a new one"
    );
    service.action(Action::Forget {}).await.unwrap();
    assert_eq!(
        kept_station_key(&vault),
        None,
        "the key goes with the pairing"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn s3m1_a_pairing_from_before_the_key_gets_one_once_and_sends_it() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    assert_eq!(
        kept_station_key(&vault),
        None,
        "premise: paired before the key"
    );
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let made = kept_station_key(&vault).expect("made at the first start with the pairing");
    settle().await;
    drop(service);
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    assert_eq!(kept_station_key(&vault), Some(made.clone()), "made once");
    assert!(on(&service.action(Action::Enable {}).await.unwrap()));
    assert_eq!(sent_station_key(&cloud).await, Some(made));
}

#[tokio::test(flavor = "multi_thread")]
async fn s3m1_a_locked_credential_store_makes_no_key() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    vault.fail.store(true, Ordering::Relaxed);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    settle().await;
    assert_eq!(
        kept_station_key(&vault),
        None,
        "a store that will not answer may hold the key: nothing is made in its place"
    );
    drop(service);
    // Control: the same pairing, with the store answering, gets its key.
    vault.fail.store(false, Ordering::Relaxed);
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    assert!(kept_station_key(&vault).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3m1_a_service_holding_another_key_for_the_station_is_said_at_the_shack() {
    let cloud = fake_cloud().await;
    let held = station_key::Signer::generate()
        .unwrap()
        .0
        .public_key()
        .to_string();
    *cloud.station_key.lock().unwrap() = Some(held.clone());
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    assert!(
        !service.status().unwrap().key_refused,
        "positive control: nothing said before the service is asked"
    );
    assert!(on(&service.action(Action::Enable {}).await.unwrap()));
    eventually(&service, "the refusal said at the shack", |s| s.key_refused).await;
    assert_eq!(
        cloud.station_key.lock().unwrap().as_deref(),
        Some(held.as_str()),
        "the service's key is never replaced"
    );
}

#[test]
fn s3m1_a_station_key_record_fits_the_windows_credential_blob() {
    let (_, pkcs8) = station_key::Signer::generate().unwrap();
    let record = vault::StationKey {
        binding: Binding {
            origin: REMOTE_ORIGIN.into(),
            station_id: STATION.into(),
            account_id: ACCOUNT.into(),
        },
        pkcs8,
    };
    let bytes = serde_json::to_string(&record)
        .unwrap()
        .encode_utf16()
        .count()
        * 2;
    assert!(bytes <= 2560, "{bytes} bytes");
}
