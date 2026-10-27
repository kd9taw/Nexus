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

/// Signing in at the shack binds the station; a device the service approved on its own proof is
/// granted only once the station has checked that proof itself.
mod account_grants;
/// Remote over this network beside the hosted mode: a paired computer's grant through hosted
/// decisions.
mod lan_grants;
/// One rule for every device: a tunnel computer's station control, and End remote control holding
/// for everyone until Remote is turned on again.
mod tunnel_grants;

#[derive(Clone, Default)]
pub(super) struct MemoryVault {
    values: Arc<Mutex<HashMap<String, String>>>,
    fail: Arc<AtomicBool>,
    devices: Arc<Mutex<()>>,
    /// The entries a store refuses alone, read and written, as `fail` refuses every one.
    refusing: Arc<Mutex<std::collections::BTreeSet<&'static str>>>,
    /// The remembered entries (`state`, `pins`, `waivers`) asked to be saved or removed, in order,
    /// refused or not.
    written: Arc<Mutex<Vec<&'static str>>>,
}
impl MemoryVault {
    /// Whether the store refuses `entry`: every entry while `fail` is set, or this one alone.
    fn refuses(&self, entry: &str) -> bool {
        self.fail.load(Ordering::Relaxed) || self.refusing.lock().unwrap().contains(entry)
    }
    fn wrote(&self, entry: &'static str) {
        self.written.lock().unwrap().push(entry);
    }
    fn account_entry<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
    ) -> Result<Option<T>, &'static str> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("credentialStoreUnavailable");
        }
        self.values
            .lock()
            .unwrap()
            .get(name)
            .map(|v| serde_json::from_str(v).map_err(|_| "unreadable"))
            .transpose()
    }
    fn save_account_entry<T: serde::Serialize>(
        &self,
        name: &str,
        value: &T,
    ) -> Result<(), &'static str> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("credentialStoreUnavailable");
        }
        self.values
            .lock()
            .unwrap()
            .insert(name.into(), serde_json::to_string(value).unwrap());
        Ok(())
    }
}
impl vault::AccountVault for MemoryVault {
    fn owner(&self) -> Result<Option<tempo_client::account::Owner>, &'static str> {
        self.account_entry("account-owner")
    }
    fn save_owner(&self, owner: &tempo_client::account::Owner) -> Result<(), &'static str> {
        self.save_account_entry("account-owner", owner)
    }
    fn account_keys(&self) -> Result<Option<vault::AccountKeys>, &'static str> {
        self.account_entry("account-keys")
    }
    fn save_account_keys(&self, keys: &vault::AccountKeys) -> Result<(), &'static str> {
        self.save_account_entry("account-keys", keys)
    }
    fn device_cache(&self) -> Result<Option<tempo_client::account::DeviceCache>, &'static str> {
        self.account_entry("account-devices")
    }
    fn save_device_cache(
        &self,
        _held: &vault::DevicesHeld<'_>,
        cache: &tempo_client::account::DeviceCache,
    ) -> Result<(), &'static str> {
        self.save_account_entry("account-devices", cache)
    }
    fn devices_lock(&self) -> &Mutex<()> {
        &self.devices
    }
    fn people(&self) -> Result<Option<vault::People>, &'static str> {
        self.account_entry("account-people")
    }
    fn save_people(&self, people: &vault::People) -> Result<(), &'static str> {
        self.save_account_entry("account-people", people)
    }
    fn revokes(&self) -> Result<Option<vault::Revokes>, &'static str> {
        self.account_entry("account-revokes")
    }
    fn save_revokes(&self, revokes: &vault::Revokes) -> Result<(), &'static str> {
        self.save_account_entry("account-revokes", revokes)
    }
    fn uninvites(&self) -> Result<Option<vault::Uninvites>, &'static str> {
        self.account_entry("account-uninvites")
    }
    fn save_uninvites(&self, uninvites: &vault::Uninvites) -> Result<(), &'static str> {
        self.save_account_entry("account-uninvites", uninvites)
    }
    fn notified(&self) -> Result<Option<vault::Notified>, &'static str> {
        self.account_entry("account-notified")
    }
    fn save_notified(&self, notified: &vault::Notified) -> Result<(), &'static str> {
        self.save_account_entry("account-notified", notified)
    }
    fn ties(&self) -> Result<Option<vault::Ties>, &'static str> {
        if self.refuses("account-ties") {
            return Err("credentialStoreUnavailable");
        }
        self.account_entry("account-ties")
    }
    fn save_ties(
        &self,
        _held: &vault::DevicesHeld<'_>,
        ties: &vault::Ties,
    ) -> Result<(), &'static str> {
        if self.refuses("account-ties") {
            return Err("credentialStoreUnavailable");
        }
        self.save_account_entry("account-ties", ties)
    }
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
        if self.refuses("state") {
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
        self.wrote("state");
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
        self.wrote("state");
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
        self.wrote("pins");
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
        self.wrote("pins");
        self.values.lock().unwrap().remove("pins");
        Ok(())
    }
    fn waivers(&self) -> Result<Option<vault::Waivers>, &'static str> {
        if self.refuses("waivers") {
            return Err("credentialStoreUnavailable");
        }
        Ok(self
            .values
            .lock()
            .unwrap()
            .get("waivers")
            .and_then(|v| serde_json::from_str(v).ok()))
    }
    fn save_waivers(&self, waivers: &vault::Waivers) -> Result<(), &'static str> {
        self.wrote("waivers");
        if self.refuses("waivers") {
            return Err("credentialStoreUnavailable");
        }
        self.values
            .lock()
            .unwrap()
            .insert("waivers".into(), serde_json::to_string(waivers).unwrap());
        Ok(())
    }
    fn remove_waivers(&self) -> Result<(), &'static str> {
        self.wrote("waivers");
        if self.refuses("waivers") {
            return Err("credentialStoreUnavailable");
        }
        self.values.lock().unwrap().remove("waivers");
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
    }
    let read_engine = engine.clone();
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
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let vault = MemoryVault::default();
    let mut service = Service::start(
        origin.clone(),
        Box::new(vault.clone()),
        engine.clone(),
        transport::Feeds {
            monitor: crate::remote_monitor::Publisher::default(),
            #[cfg(feature = "radio")]
            audio: None,
            stream: Default::default(),
        },
        Accounts::not_ready(),
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    println!("REMOTE_TEST:{{\"ready\":true}}");
    std::io::stdout().flush().unwrap();
    for line in lines {
        let value: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
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
        // Test-only park directory. The response is the desktop search_parks and
        // lookup_park answer for the same search, for parity checks.
        // Test-only rare-DX input: spots run through the desktop's own Pounce detector. The
        // response is what the desktop raised (its `pounce` event payloads), for parity checks.
        // Test-only desktop truth for the hosted Awards diagnostics: the report the
        // desktop get_confirmation_diagnostics command returns for the seeded log.
        // Test-only input to the in-memory engine. The production controller has
        // no import action; the response supplies desktop truth for parity checks.
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
                        #[cfg(feature = "radio")]
                        audio: None,
                        stream: Default::default(),
                    },
                    Accounts::not_ready(),
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
                cancellation, &engine, &transport::Feeds { monitor: publisher.clone(), #[cfg(feature = "radio")] audio: None, stream: Default::default() }, &status).await;
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
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), #[cfg(feature = "radio")] audio: None, stream: Default::default() };
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
/// An operation the hosted page used to send and this station no longer takes — a retired request
/// or a station control that is neither an FT gesture nor the held contact's log action — is
/// answered `stationUnsupported`, as an older station answers what it cannot do, and the socket
/// stays up with Stop's road on it. CONTROL: the parse failure above still reconnects.
#[test]
fn a_retired_operation_is_refused_by_name_and_the_station_stays_connected() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    const SESSION: &str = "20000000-0000-4000-8000-000000000001";
    let retired = [
        json!({"type":"logManual","requestId":"30000000-0000-4000-8000-000000000001"}),
        json!({"type":"activationExport","requestId":"30000000-0000-4000-8000-000000000002"}),
        json!({"type":"stationControl","requestId":"30000000-0000-4000-8000-000000000003",
            "action":{"action":"decoder.clear","receiver":"cw"}}),
    ];
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let client = Client::new(&origin).unwrap();
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let control = Arc::new(Mutex::new(Control { enabled: true, ..Default::default() }));
        let status = SessionStatus { status: Arc::new(Mutex::new(Status::default())), control: control.clone(), generation: 0 };
        let (cancel, cancellation) = watch::channel(false);
        let (answers, mut answered) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
        let requests = retired.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            for request in requests {
                let message = json!({"type":"operationRequest","sessionId":SESSION,"deviceId":SESSION,"operationVersion":4,"request":request});
                socket.send(Message::Text(message.to_string().into())).await.unwrap();
                loop {
                    match tokio::time::timeout(Duration::from_secs(5), socket.next()).await {
                        Ok(Some(Ok(Message::Text(text)))) => { let _ = answers.send(serde_json::from_str(&text).unwrap()); break }
                        Ok(Some(Ok(_))) => continue,
                        _ => return,
                    }
                }
            }
            // Hold the socket until the station closes it. The station's first heartbeat Ping can
            // come after the last answer on a busy machine, and ending at it dropped the socket
            // without a close, which the station reads as `invalidResponse` and reconnects.
            while let Ok(Some(Ok(_))) = tokio::time::timeout(Duration::from_secs(30), socket.next()).await {}
        });
        let binding = Binding { origin: origin.clone(), station_id: STATION.into(), account_id: ACCOUNT.into() };
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), #[cfg(feature = "radio")] audio: None, stream: Default::default() };
        let supervised = tokio::spawn(transport::supervise(client, binding, transport::random_secret().unwrap(), cancellation, engine, feeds, status.clone()));
        for request in &retired {
            let answer = tokio::time::timeout(Duration::from_secs(10), answered.recv()).await.expect("the station answers").expect("an answer");
            assert_eq!(answer, json!({"type":"operationResponse","sessionId":SESSION,"requestId":request["requestId"],"error":"stationUnsupported"}));
        }
        let current = status.status.lock().unwrap().clone();
        assert_eq!((current.phase.as_str(), current.error), ("connected", None), "the socket stayed up");
        assert!(control.lock().unwrap().enabled, "Remote is still on");
        let _ = cancel.send(true);
        supervised.await.unwrap();
        server.abort();
    });
}
/// ★ The room's tunnel at the station's own socket: a station that can serve one advertises the
/// pipe (`x-nexus-pipe-version: 1`), and for each `pipeOpen` the room sends it opens its end of
/// that pipe, a socket of its own on the pairing's credential, naming the pipe; never more than the
/// room's four at once. CONTROL: a station that cannot serve one (no LAN road in this build of the
/// service) advertises nothing and opens nothing, whatever it is sent.
#[test]
fn a_station_that_can_serve_a_tunnel_advertises_the_pipe_and_opens_its_end_of_each() {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    use tokio_tungstenite::tungstenite::Message;
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        for serves in [true, false] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let client = Client::new(&origin).unwrap();
            let engine: crate::SharedEngine =
                Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
            let feeds = transport::Feeds {
                monitor: crate::remote_monitor::Publisher::default(),
                #[cfg(feature = "radio")]
                audio: None,
                stream: Default::default(),
            };
            let mut control = Control {
                enabled: true,
                ..Default::default()
            };
            if serves {
                let (signer, pkcs8) = station_key::Signer::generate().unwrap();
                drop(signer);
                control.station_identity = Some(Arc::new(
                    lan::tls::Identity::new(&pkcs8, STATION.into()).unwrap(),
                ));
                control.tunnel = Some(lan::TunnelDeps {
                    authority: control.operations.clone(),
                    engine: engine.clone(),
                    feeds: feeds.clone(),
                    dispatcher: crate::command_dispatch::Dispatcher::new(
                        crate::command_dispatch::ShackStores {
                            engine: engine.clone(),
                        },
                    ),
                    commands: lan::commands::Commands::default(),
                });
            }
            let control = Arc::new(Mutex::new(control));
            let status = SessionStatus {
                status: Arc::new(Mutex::new(Status::default())),
                control,
                generation: 0,
            };
            let (cancel, cancellation) = watch::channel(false);
            let token = transport::random_secret().unwrap();
            // The service: the station's main socket first, then each pipe end it opens.
            let (seen, mut heard) = tokio::sync::mpsc::unbounded_channel::<(
                String,
                Option<String>,
                Option<String>,
                bool,
            )>();
            let pipes: Vec<String> = (1..=5)
                .map(|n| format!("50000000-0000-4000-8000-00000000000{n}"))
                .collect();
            let sent = pipes.clone();
            let server = tokio::spawn(async move {
                let mut held = Vec::new();
                while let Ok((stream, _)) = listener.accept().await {
                    let seen = seen.clone();
                    let mut asked = None;
                    let socket = tokio_tungstenite::accept_hdr_async(
                        stream,
                        |request: &Request, response: Response| {
                            let header = |name: &str| {
                                request
                                    .headers()
                                    .get(name)
                                    .and_then(|v| v.to_str().ok())
                                    .map(str::to_string)
                            };
                            asked = Some((
                                request.uri().path().to_string(),
                                header("x-nexus-pipe-version"),
                                header("x-nexus-pipe-id"),
                                header("authorization").is_some(),
                            ));
                            Ok(response)
                        },
                    )
                    .await;
                    let (Ok(mut socket), Some(asked)) = (socket, asked) else {
                        continue;
                    };
                    if asked.0.ends_with("/connect") {
                        for pipe in &sent {
                            socket
                                .send(Message::Text(
                                    format!(r#"{{"type":"pipeOpen","pipeId":"{pipe}"}}"#).into(),
                                ))
                                .await
                                .unwrap();
                        }
                    }
                    let _ = seen.send(asked);
                    held.push(socket);
                }
            });
            let binding = Binding {
                origin: origin.clone(),
                station_id: STATION.into(),
                account_id: ACCOUNT.into(),
            };
            let supervised = tokio::spawn(transport::supervise(
                client,
                binding,
                token,
                cancellation,
                engine,
                feeds,
                status.clone(),
            ));
            // The main socket and four ends, then a moment for a fifth end that must not come.
            let mut asked = Vec::new();
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            while let Ok(Some(one)) = tokio::time::timeout_at(deadline, heard.recv()).await {
                asked.push(one);
                if asked.len() == 1 + super::tunnel::MAX_TUNNELS || (!serves && asked.len() == 1) {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            while let Ok(one) = heard.try_recv() {
                asked.push(one)
            }
            let _ = cancel.send(true);
            supervised.await.unwrap();
            server.abort();
            let connect = format!("/api/remote/stations/{STATION}/connect");
            let main: Vec<_> = asked.iter().filter(|a| a.0 == connect).collect();
            assert!(!main.is_empty(), "premise: the station connected");
            assert_eq!(
                main[0].1.as_deref(),
                serves.then_some("1"),
                "the pipe advertised only by a station that can serve it"
            );
            let ends: Vec<_> = asked.iter().filter(|a| a.0 != connect).collect();
            if !serves {
                assert!(
                    ends.is_empty(),
                    "a station that cannot serve a tunnel opened a pipe: {ends:?}"
                );
                continue;
            }
            assert_eq!(
                ends.len(),
                super::tunnel::MAX_TUNNELS,
                "one end for each pipe, up to the room's bound: {ends:?}"
            );
            for end in &ends {
                assert_eq!(end.0, format!("/api/remote/stations/{STATION}/pipe"));
                assert!(end.3, "on the pairing's credential");
                assert!(
                    pipes[..4].contains(end.2.as_ref().unwrap()),
                    "names a pipe the room asked for: {end:?}"
                );
            }
        }
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
    const READS: usize = 40;
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
            // Fill the pipe: retired operations the station refuses inline, none of the refusals
            // read here.
            for n in 0..READS as u32 {
                socket.send(operation(json!({"type":"logManual","requestId":id(0x30 + n)}))).await.unwrap();
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
                let refused = answer["error"] == "stationUnsupported";
                answers.push(answer);
                if !refused { break; }
            }
            assert_eq!(answers.len(), READS + 1);
            let queued: usize = answers[..READS].iter().map(|a| a.to_string().len()).sum();
            assert!(queued > PIPE, "positive control: {queued} bytes of refusals outgrow the {PIPE}-byte pipe nobody was draining, so the writer was stuck when the Stop went in");
            assert_eq!(answers[READS]["requestId"], id(0x20));
            assert_eq!(answers[READS]["value"], json!({"stop":"accepted"}));
            stop_remote(&mut socket, &cancel).await;
        });
        let request = "ws://localhost/api/remote/stations/x/connect".into_client_request().unwrap();
        let (socket, _) = tokio_tungstenite::client_async_with_config(request, station_io, Some(transport::socket_config())).await.unwrap();
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), #[cfg(feature = "radio")] audio: None, stream: Default::default() };
        let served = transport::serve(socket, cancellation, &engine, &feeds, &status, None).await;
        relay.await.unwrap();
        assert_eq!(served, Ok(()), "the session ended because Remote stopped, not because a send timed out");
    });
}

// ---- The relay's older lanes, bound to the browser's own key (security review S1-M1) -----------
//
// The relay stamps `sessionId` and `deviceId` on everything it forwards, so on its word alone a
// relay that is not the one the operator trusts could act as any approved browser: take control,
// keep a lease alive, listen. The stream's offer has carried the browser's signature since A5;
// these hold the older lanes to the same key. Every message here is written byte for byte, and
// each proof is made the way the page makes one, so the station is held to the bytes the relay
// hands it.

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

/// [`pinned_station`] for `LANE_DEVICE`, granted station control at the radio.
fn lane_station(key: &LaneKey) -> SessionStatus {
    let status = pinned_station(key, LANE_DEVICE);
    {
        let control = status.control.lock().unwrap();
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

/// Remote stops, as the relay sees it: the stop is asked for, and the relay holds its end of the
/// socket until the station closes the session (for at most 5 s). A relay end dropped at once can
/// reach the station's loop between its look at the stop and its look at the socket, and the
/// session then ends on the broken socket (`invalidResponse`), not on the stop.
async fn stop_remote(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
    cancel: &watch::Sender<bool>,
) {
    use futures_util::StreamExt;
    cancel.send(true).unwrap();
    let closed = async {
        while let Some(Ok(message)) = socket.next().await {
            if message.is_close() {
                return;
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(5), closed).await;
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

            // Heartbeats keep a lease: unsigned, refused; signed, renewed.
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
            stop_remote(&mut socket, &cancel).await;
        });
        let request = "ws://localhost/api/remote/stations/x/connect".into_client_request().unwrap();
        let (socket, _) = tokio_tungstenite::client_async_with_config(request, station_io, Some(transport::socket_config())).await.unwrap();
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), #[cfg(feature = "radio")] audio: None, stream: Default::default() };
        let served = transport::serve(socket, cancellation, &engine, &feeds, &status, None).await;
        relay.await.unwrap();
        assert_eq!(served, Ok(()), "the session ended because Remote stopped");
    });
}

/// ★ The old Remote page's FT8/FT4 and held-contact actions are gone. A browser that holds what an
/// older build's approval restores (logging and station control, its key pinned) and a live lease,
/// and that still sends one of them, signed as the page signed it, is answered `stationUnsupported`
/// and nothing runs: the latch stays off, and its lease and the socket are untouched. So is a
/// `result`, which read only those actions' receipts. CONTROL: the same lease still heartbeats, and
/// its Stop is still taken.
#[test]
fn the_old_pages_ft_and_held_contact_actions_are_refused_and_the_lease_stays() {
    // A stopTransmit reaches the satellite track badge (see the slow-link Stop test above).
    let _alone = alone();
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let key = LaneKey::new();
    let ids = [LANE_STATION, LANE_DEVICE, LANE_SESSION];
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (station_io, relay_io) = tokio::io::duplex(65536);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let status = lane_station(&key);
        let (cancel, cancellation) = watch::channel(false);
        let watched = engine.clone();
        let relay = tokio::spawn(async move {
            let mut socket = tokio_tungstenite::accept_async(relay_io).await.unwrap();
            let id = |n: u32| format!("00000000-0000-4000-8000-0000000002{n:02x}");
            let mut seq = 0;
            let mut signed = |request: serde_json::Value| {
                seq += 1;
                lane_operation(&request, LANE_DEVICE, Some(key.proof(LANE_OPERATION, &request.to_string(), ids, seq)))
            };
            let state = |n| lane_operation(&json!({"type":"state","requestId":id(n)}), LANE_DEVICE, None);
            let boot = lane_exchange(&mut socket, state(1)).await["value"]["stationBootId"].as_str().unwrap().to_owned();
            let owner = lane_exchange(&mut socket, signed(json!({"type":"acquire","requestId":id(2),"stationBootId":boot}))).await;
            assert_eq!(owner["value"]["phase"], "controlling", "premise: the browser holds the lease: {owner}");
            let lease = owner["value"]["leaseId"].as_str().unwrap().to_owned();
            for (n, action) in [
                (3, json!({"action":"qso.discard","expectedKey":"0123456789abcdef"})),
                (5, json!({"action":"ft.txEnabled","expectedTier":"FT8","on":true})),
                (7, json!({"action":"ft.cq","expectedTier":"FT8","direction":null})),
            ] {
                let current = lane_exchange(&mut socket, state(n)).await["value"].clone();
                assert_eq!(current["phase"], "controlling", "{action}: the lease is held");
                let mut action = action;
                if action["action"].as_str().unwrap().starts_with("ft.") {
                    action["transmitEpoch"] = current["transmitEpoch"].clone();
                }
                let request = json!({"type":"stationControl","requestId":id(n + 1),"stationBootId":current["stationBootId"],
                    "leaseId":current["leaseId"],"expectedRevision":current["revision"],"commandWindowId":current["commandWindowId"],
                    "clientSequence":current["nextSequence"],"context":current["controls"]["context"],"action":action});
                let answer = lane_exchange(&mut socket, signed(request)).await;
                assert_eq!(answer["error"], "stationUnsupported", "{action}: {answer}");
                assert!(!watched.lock().unwrap().tx_enabled(), "{action}: nothing armed");
            }
            let result = lane_exchange(&mut socket, signed(json!({"type":"result","requestId":id(9),"operationId":id(3)}))).await;
            assert_eq!(result["error"], "stationUnsupported", "{result}");
            // CONTROL: the socket and the lease are as they were, and Stop is still taken.
            let beat = lane_exchange(&mut socket, signed(json!({"type":"heartbeat","requestId":id(10),"leaseId":lease}))).await;
            assert_eq!(beat["value"]["phase"], "controlling", "the refusals left the lease alone: {beat}");
            let stop = lane_exchange(&mut socket, lane_operation(&json!({"type":"stopTransmit","requestId":id(11),
                "stationBootId":boot,"leaseId":lease,"transmitEpoch":beat["value"]["transmitEpoch"]}), LANE_DEVICE, None)).await;
            assert_eq!(stop["value"], json!({"stop":"accepted"}), "{stop}");
            stop_remote(&mut socket, &cancel).await;
        });
        let request = "ws://localhost/api/remote/stations/x/connect".into_client_request().unwrap();
        let (socket, _) = tokio_tungstenite::client_async_with_config(request, station_io, Some(transport::socket_config())).await.unwrap();
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), #[cfg(feature = "radio")] audio: None, stream: Default::default() };
        let served = transport::serve(socket, cancellation, &engine, &feeds, &status, None).await;
        relay.await.unwrap();
        assert_eq!(served, Ok(()), "the session ended because Remote stopped, and no refusal ended it");
    });
}

/// A computer (LAN-paired, or approved for the room's tunnel) reads only over its own pinned TLS
/// link. The relay's lane is unsigned for `state`, so the station answers it no read in a
/// computer's name, while a browser's `state` and a Stop in a computer's name are answered as before.
#[test]
fn the_relay_lane_answers_no_read_in_a_computers_name() {
    let _alone = alone();
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    const LAN_COMPUTER: &str = "40000000-0000-4000-8000-0000000000c1";
    const TUNNEL_COMPUTER: &str = "40000000-0000-4000-8000-0000000000c2";
    let key = LaneKey::new();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (station_io, relay_io) = tokio::io::duplex(65536);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let status = lane_station(&key);
        {
            let control = status.control.lock().unwrap();
            control.operations.hold_lan_devices(Box::new(|| vec![LAN_COMPUTER.into()]));
            control.operations.hold_tunnel_devices(Box::new(|| vec![TUNNEL_COMPUTER.into()]));
        }
        let (cancel, cancellation) = watch::channel(false);
        let relay = tokio::spawn(async move {
            let mut socket = tokio_tungstenite::accept_async(relay_io).await.unwrap();
            let id = |n: u32| format!("00000000-0000-4000-8000-0000000002{n:02x}");
            let state = |n| json!({"type":"state","requestId":id(n)});
            // CONTROL: the browser's unsigned state is answered, as the page needs.
            let browser = lane_exchange(&mut socket, lane_operation(&state(1), LANE_DEVICE, None)).await;
            assert_eq!(browser["value"]["allowed"], true, "{browser}");
            let boot = browser["value"]["stationBootId"].as_str().unwrap().to_owned();
            for (n, computer) in [(2, LAN_COMPUTER), (3, TUNNEL_COMPUTER)] {
                let read = lane_exchange(&mut socket, lane_operation(&state(n), computer, None)).await;
                assert_eq!(read, json!({"type":"operationResponse","sessionId":LANE_SESSION,"requestId":id(n),"error":"stationUnsupported"}), "{computer}");
            }
            // Stop still needs nothing: in a computer's name it is answered, not refused.
            let stop = lane_exchange(&mut socket, lane_operation(&json!({"type":"stopTransmit","requestId":id(4),"stationBootId":boot,"leaseId":id(5),"transmitEpoch":"0000000000000000"}), LAN_COMPUTER, None)).await;
            assert_eq!(stop["requestId"], id(4), "{stop}");
            assert_ne!(stop["error"], "stationUnsupported", "{stop}");
            stop_remote(&mut socket, &cancel).await;
        });
        let request = "ws://localhost/api/remote/stations/x/connect".into_client_request().unwrap();
        let (socket, _) = tokio_tungstenite::client_async_with_config(request, station_io, Some(transport::socket_config())).await.unwrap();
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), #[cfg(feature = "radio")] audio: None, stream: Default::default() };
        let served = transport::serve(socket, cancellation, &engine, &feeds, &status, None).await;
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
            stop_remote(&mut socket, &cancel).await;
        });
        let request = "ws://localhost/api/remote/stations/x/connect".into_client_request().unwrap();
        let (socket, _) = tokio_tungstenite::client_async_with_config(request, station_io, Some(transport::socket_config())).await.unwrap();
        let feeds = transport::Feeds { monitor: crate::remote_monitor::Publisher::default(), audio: Some(audio::ReceiveFanout::new(feed.feed.clone())), stream: Default::default() };
        let served = transport::serve(socket, cancellation, &engine, &feeds, &status, None).await;
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
    /// While set, the device list and a removal are not answered (a 503), as a service that is down
    /// answers.
    refusing: Arc<AtomicBool>,
    /// Every `enroll/approve` body, as sent.
    approvals: Arc<Mutex<Vec<serde_json::Value>>>,
    /// While set, a removal alone is not answered (a 503), while the list still is.
    refusing_removals: Arc<AtomicBool>,
    /// Every invitation and its removal, as sent: the route's last word and the body.
    invites: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    /// While set, the device list is answered only once it is cleared, as a slow service answers.
    holding_lists: Arc<AtomicBool>,
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
    let refusing = Arc::new(AtomicBool::new(false));
    let down = refusing.clone();
    let approvals = Arc::new(Mutex::new(Vec::new()));
    let approved_bodies = approvals.clone();
    let refusing_removals = Arc::new(AtomicBool::new(false));
    let removals_down = refusing_removals.clone();
    let invites = Arc::new(Mutex::new(Vec::new()));
    let invited = invites.clone();
    let holding_lists = Arc::new(AtomicBool::new(false));
    let slow = holding_lists.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let served = served.clone();
            let held = held.clone();
            let down = down.clone();
            let approved_bodies = approved_bodies.clone();
            let removals_down = removals_down.clone();
            let invited = invited.clone();
            let slow = slow.clone();
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
                let kinds = head.contains("\r\nx-nexus-device-kind: 1\r\n");
                while head.contains("/native/devices ") && slow.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
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
                    if let Ok(sent) = serde_json::from_slice(&request[head_end..]) {
                        approved_bodies.lock().unwrap().push(sent);
                    }
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
                } else if head.contains("/native/devices ") && down.load(Ordering::SeqCst) {
                    ("503 Service Unavailable", "{}".to_string())
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
                    if !kinds {
                        // The native client's devices, and every device's kind and platform, only
                        // to a Nexus that asks: any other is listed its browsers alone.
                        let devices = listed["devices"].as_array_mut().unwrap();
                        devices.retain(|d| d.get("kind").is_none_or(|k| k == "browser"));
                        for device in devices {
                            let device = device.as_object_mut().unwrap();
                            device.remove("kind");
                            device.remove("platform");
                            device.remove("untilRemoved");
                        }
                    }
                    if !head.contains("\r\nx-nexus-device-proof: 1\r\n") {
                        // Each device's role, how it was added, its proof and a computer's LAN
                        // key, only to a Nexus that checks proofs itself.
                        for device in listed["devices"].as_array_mut().unwrap() {
                            let device = device.as_object_mut().unwrap();
                            for field in ["role", "addedBy", "proof", "lanKey"] {
                                device.remove(field);
                            }
                        }
                    }
                    ("200 OK", listed.to_string())
                } else if head.contains("/native/approve-device ")
                    && serde_json::from_str::<serde_json::Value>(&served.lock().unwrap())
                        .is_ok_and(|l| l["devices"].as_array().is_some_and(|d| d.len() > 1))
                {
                    // Of several devices, the one the request names (every one under its id) gets
                    // the new approval, and the others are left as they are.
                    let asked = serde_json::from_slice::<serde_json::Value>(&request[head_end..])
                        .ok()
                        .and_then(|v| v["deviceId"].as_str().map(str::to_string));
                    let mut listed: serde_json::Value =
                        serde_json::from_str(&served.lock().unwrap()).unwrap();
                    for device in listed["devices"].as_array_mut().unwrap() {
                        if device["id"].as_str() == asked.as_deref() {
                            device["approved"] = 1.into();
                            device["expiresAt"] = APPROVED_UNTIL.into();
                            if lifetime {
                                device["generation"] = 2.into();
                                device["renewsUntil"] = (APPROVED_UNTIL + 60 * DAY_MS).into();
                            }
                        }
                    }
                    *served.lock().unwrap() = listed.to_string();
                    ("200 OK", r#"{"ok":true}"#.to_string())
                } else if head.contains("/native/approve-device ") {
                    // The service writes a new approval, and with it the approval generation. The
                    // browser's device key (A5) is the browser's, and approving leaves it as it is,
                    // as it leaves a device's kind and platform.
                    let listed = serde_json::from_str::<serde_json::Value>(&served.lock().unwrap())
                        .unwrap_or_default();
                    let mut approved: serde_json::Value = serde_json::from_str(&if lifetime {
                        device_list_at(1, APPROVED_UNTIL, 2)
                    } else {
                        device_list(1, APPROVED_UNTIL)
                    })
                    .unwrap();
                    for field in ["id", "publicKey", "kind", "platform", "name"] {
                        if let Some(value) = listed["devices"][0].get(field) {
                            approved["devices"][0][field] = value.clone();
                        }
                    }
                    *served.lock().unwrap() = approved.to_string();
                    ("200 OK", r#"{"ok":true}"#.to_string())
                } else if head.contains("/native/revoke-device ")
                    && (down.load(Ordering::SeqCst) || removals_down.load(Ordering::SeqCst))
                {
                    ("503 Service Unavailable", "{}".to_string())
                } else if head.contains("/native/revoke-device ")
                    && serde_json::from_str::<serde_json::Value>(&served.lock().unwrap())
                        .is_ok_and(|l| l["devices"].as_array().is_some_and(|d| d.len() > 1))
                {
                    // Of several devices, only the one the request names stops being listed.
                    let asked = serde_json::from_slice::<serde_json::Value>(&request[head_end..])
                        .ok()
                        .and_then(|v| v["deviceId"].as_str().map(str::to_string));
                    let mut listed: serde_json::Value =
                        serde_json::from_str(&served.lock().unwrap()).unwrap();
                    listed["devices"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|d| d["id"].as_str() != asked.as_deref());
                    *served.lock().unwrap() = listed.to_string();
                    ("200 OK", r#"{"ok":true}"#.to_string())
                } else if head.contains("/native/revoke-device ") {
                    // A revoked browser's expiry becomes "now", so the service stops listing it.
                    *served.lock().unwrap() = json!({"devices":[]}).to_string();
                    ("200 OK", r#"{"ok":true}"#.to_string())
                } else if (head.contains("/native/invite ") || head.contains("/native/uninvite "))
                    && down.load(Ordering::SeqCst)
                {
                    ("503 Service Unavailable", "{}".to_string())
                } else if head.contains("/native/invite ") || head.contains("/native/uninvite ") {
                    let verb = if head.contains("/native/invite ") {
                        "invite"
                    } else {
                        "uninvite"
                    };
                    if let Ok(sent) = serde_json::from_slice(&request[head_end..]) {
                        invited.lock().unwrap().push((verb.to_string(), sent));
                    }
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
        refusing,
        approvals,
        refusing_removals,
        invites,
        holding_lists,
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
/// Turn Remote on, list the browser, and grant it station control.
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
    assert!(status.station_permissions.is_empty());
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
async fn station_grants_survive_a_restart_for_a_still_approved_browser() {
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
        s.station_permissions == [BROWSER]
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
        let restored = !status.station_permissions.is_empty();
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
// Approving a browser grants it station control. It is remembered across a restart
// for as long as that approval stands, and nothing arms the transmitter: the TX-enable latch starts
// off.

/// A browser the service lists as waiting for approval.
fn pending_browser(cloud: &FakeCloud) {
    *cloud.devices.lock().unwrap() = device_list(0, APPROVED_UNTIL - 1);
}
/// Approve the browser here, the one-approval way.
async fn approve(service: &Service) -> Status {
    service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: true,
            key: None,
            kind: Some(Kind::Browser),
        })
        .await
        .unwrap()
}
fn transmitter_idle(engine: &crate::SharedEngine) -> bool {
    let e = engine.lock().unwrap();
    !e.tx_enabled() && !e.remote_ft_tx_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn approving_a_browser_grants_station_control() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    pending_browser(&cloud);
    let before = service.action(Action::Refresh {}).await.unwrap();
    assert!(
        before.station_permissions.is_empty(),
        "a browser waiting for approval holds nothing"
    );
    let status = approve(&service).await;
    assert!(on(&status), "approving a browser does not toggle Remote");
    assert_eq!(status.station_permissions, [BROWSER]);
    assert!(transmitter_idle(&engine), "a grant arms nothing");
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
    let off = approve(&service).await;
    assert!(
        !on(&off) && off.observation_generation.is_none(),
        "approving a browser does not turn Remote on; only approving the pairing does"
    );
    assert!(
        off.station_permissions.is_empty(),
        "nothing is live while Remote is off"
    );
    service.action(Action::Enable {}).await.unwrap();
    eventually(&service, "the approval took effect at Turn on", |s| {
        s.station_permissions == [BROWSER]
    })
    .await;
    assert!(transmitter_idle(&engine));
}

/// Operator decision 2026-09-14: Turn off Remote PAUSES. Turning it back on restores what each
/// still-approved browser had. Since 2026-10-10 End remote control
/// (take over) pauses the same way: it drops everyone and holds until Remote is turned on again, and
/// no browser is approved again after it. Revoking a browser still clears. The TX-enable latch is
/// off at every step, and nothing keys.
#[tokio::test(flavor = "multi_thread")]
async fn turn_off_and_ending_remote_control_pause_but_revoking_a_browser_clears() {
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
        let granted = approve(&service).await;
        assert_eq!(granted.station_permissions, [BROWSER], "{case}: granted");
        assert!(transmitter_idle(&engine), "{case}");

        if case.starts_with("end remote control") {
            service.action(Action::TakeOverLogging {}).await.unwrap();
        }
        let off = service.action(Action::Disable {}).await.unwrap();
        assert!(!on(&off), "{case}: Remote is off");
        assert!(
            off.station_permissions.is_empty(),
            "{case}: nothing is live while Remote is off"
        );
        assert!(transmitter_idle(&engine), "{case}");
        if case.contains("revoke the browser") {
            service
                .action(Action::Device {
                    device_id: BROWSER.into(),
                    approve: false,
                    key: None,
                    kind: None,
                })
                .await
                .unwrap();
        }
        service.action(Action::Enable {}).await.unwrap();

        let status = if !case.contains("revoke the browser") {
            eventually(&service, "the paused grants came back", |s| {
                s.station_permissions == [BROWSER]
            })
            .await
        } else {
            settle().await;
            let status = service.action(Action::Refresh {}).await.unwrap();
            assert!(
                status.station_permissions.is_empty(),
                "{case}: nothing comes back: {}",
                serde_json::to_string(&status).unwrap()
            );
            status
        };
        assert!(on(&status), "{case}: Remote is back on");
        assert!(transmitter_idle(&engine), "{case}: the latch is still off");
    }
}

/// Removing one browser's approval at the shack takes that browser's grants, and only its (operator
/// decision 2026-10-14): every other browser keeps station control and a lease it holds, now and at the next Turn on. REFUSED: the removed browser at its next command, and
/// a browser never approved here. Nothing is keyed throughout.
#[tokio::test(flavor = "multi_thread")]
async fn removing_one_browser_leaves_every_other_device_its_grants() {
    const OTHER: &str = "10000000-0000-4000-8000-00000000000e";
    const NEVER: &str = "10000000-0000-4000-8000-00000000000f";
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    *cloud.devices.lock().unwrap() = json!({"devices":[
        {"id":BROWSER,"name":"Test browser","approved":0,"expiresAt":APPROVED_UNTIL - 1},
        {"id":OTHER,"name":"Other browser","approved":0,"expiresAt":APPROVED_UNTIL - 1}]})
    .to_string();
    service.action(Action::Refresh {}).await.unwrap();
    for id in [BROWSER, OTHER] {
        service
            .action(Action::Device {
                device_id: id.into(),
                approve: true,
                key: None,
                kind: Some(Kind::Browser),
            })
            .await
            .unwrap();
    }
    let holds = |s: &Status, id: &str| s.station_permissions.contains(&id.to_string());
    let both = service.status().unwrap();
    assert!(holds(&both, BROWSER), "premise");
    assert!(holds(&both, OTHER), "premise");

    // The other browser holds the lease, through a connection that stays open.
    let authority = service.control.lock().unwrap().operations.clone();
    let connection = operations::LanConnection::new(authority.clone());
    let acquire = |device: &str| -> String {
        let session = lan_grants::fresh_id();
        let ask = |request: operations::Request| {
            for _ in 0..50 {
                match authority.handle_version(
                    (connection.id, 4),
                    &session,
                    device,
                    &request,
                    &engine,
                    Instant::now(),
                ) {
                    Err("stationBusy" | "remoteBusy") => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    other => return other,
                }
            }
            Err("busy throughout")
        };
        let boot = match ask(operations::Request::State {
            request_id: lan_grants::fresh_id(),
        }) {
            Ok(state) => state["stationBootId"].as_str().unwrap().to_string(),
            Err(refused) => return refused.into(),
        };
        match ask(operations::Request::Acquire {
            request_id: lan_grants::fresh_id(),
            station_boot_id: boot,
        }) {
            Ok(acquired) => acquired["phase"].as_str().unwrap_or("none").into(),
            Err(refused) => refused.into(),
        }
    };
    assert_eq!(acquire(OTHER), "controlling", "premise: the other's lease");

    service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    let after = service.status().unwrap();
    assert!(!holds(&after, BROWSER), "removed");
    assert!(
        holds(&after, OTHER),
        "removing one browser took another's grants: {}",
        serde_json::to_string(&after).unwrap()
    );
    assert_eq!(
        authority.local_status()["controller"],
        json!(OTHER),
        "removing one browser ended another's lease"
    );
    assert_eq!(acquire(BROWSER), "localPermissionRequired", "removed");
    assert_eq!(acquire(NEVER), "localPermissionRequired", "never approved");
    assert!(transmitter_idle(&engine));

    // Turn off and on: the other browser's grants come back, the removed one's never do.
    service.action(Action::Disable {}).await.unwrap();
    service.action(Action::Enable {}).await.unwrap();
    eventually(&service, "the other browser's grants came back", |s| {
        holds(s, OTHER)
    })
    .await;
    settle().await;
    let back = service.action(Action::Refresh {}).await.unwrap();
    assert!(!holds(&back, BROWSER), "came back");
    assert!(holds(&back, OTHER));
    assert_eq!(acquire(BROWSER), "localPermissionRequired", "removed");
    assert_eq!(acquire(NEVER), "localPermissionRequired", "never approved");
    assert!(transmitter_idle(&engine), "the latch is still off");
}

/// A removal the service never heard still stands at the shack: the browser's grants end at once and
/// are forgotten, so Turn on gives them back to nobody, though the service still lists the approval
/// as it was. CONTROL: the same Turn on with no removal gives them back.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_the_service_never_heard_still_stands_at_the_shack() {
    for removed in [false, true] {
        let cloud = fake_cloud().await;
        let engine = engine();
        let service = on_at(&cloud, &engine).await;
        pending_browser(&cloud);
        service.action(Action::Refresh {}).await.unwrap();
        assert_eq!(approve(&service).await.station_permissions, [BROWSER]);
        if removed {
            cloud.refusing.store(true, Ordering::SeqCst);
            let refused = service
                .action(Action::Device {
                    device_id: BROWSER.into(),
                    approve: false,
                    key: None,
                    kind: None,
                })
                .await;
            assert_eq!(
                refused.map(|s| s.account.removals),
                Ok(vec![BROWSER.to_string()]),
                "premise: the service never heard it, and it waits to be told"
            );
            let status = service.status().unwrap();
            assert!(
                status.station_permissions.is_empty(),
                "the removal did not stand at once"
            );
            // The service answers again, but still never hears the removal: it lists the approval as
            // it was.
            cloud.refusing_removals.store(true, Ordering::SeqCst);
            cloud.refusing.store(false, Ordering::SeqCst);
        }
        service.action(Action::Disable {}).await.unwrap();
        service.action(Action::Enable {}).await.unwrap();
        let status = if removed {
            eventually(&service, "Remote back on", on).await;
            settle().await;
            service.action(Action::Refresh {}).await.unwrap()
        } else {
            eventually(&service, "control: the grants came back", |s| {
                s.station_permissions == [BROWSER]
            })
            .await
        };
        assert_eq!(
            status.station_permissions.is_empty(),
            removed,
            "removed={removed}: {}",
            serde_json::to_string(&status).unwrap()
        );
        assert!(transmitter_idle(&engine));
    }
}

/// Operator decision 2026-09-14: approving the first pairing turns Remote on, the same way Turn on
/// Remote does, and the browser that did the pairing is approved with it. The TX-enable latch stays
/// off, and a later Turn off is remembered.
#[tokio::test(flavor = "multi_thread")]
async fn approving_the_pairing_turns_remote_on_and_approves_the_browser_that_paired() {
    {
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
            })
            .await
            .unwrap();
        assert!(
            on(&approved),
            "approving the pairing turns Remote on: {}",
            approved.phase
        );
        eventually(&service, "the pairing browser's grants", |s| {
            s.station_permissions == [BROWSER]
        })
        .await;
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

/// A revoked switch is remembered across a restart. End remote control (take over) is remembered as
/// Remote off: after a restart it stays off, and turning it on gives the browser back everything its
/// approval had (the operator's decision of 2026-10-10: no device is approved again after it).
#[tokio::test(flavor = "multi_thread")]
async fn revoking_control_or_taking_over_is_remembered_across_a_restart() {
    for case in ["revoke station control", "take over"] {
        let cloud = fake_cloud().await;
        let vault = paired_vault(&cloud.origin);
        let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
        let service = launch(&cloud, &vault, &engine);
        eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
        service.action(Action::Enable {}).await.unwrap();
        pending_browser(&cloud);
        service.action(Action::Refresh {}).await.unwrap();
        let granted = approve(&service).await;
        assert_eq!(
            granted.station_permissions,
            [BROWSER],
            "{case}: positive control"
        );
        let revoked = match case {
            "revoke station control" => Action::StationPermission {
                device_id: BROWSER.into(),
                allow: false,
            },
            _ => Action::TakeOverLogging {},
        };
        let now = service.action(revoked).await.unwrap();
        assert!(
            now.station_permissions.is_empty(),
            "{case}: takes effect at once"
        );
        settle().await;
        drop(service);

        let service = launch(&cloud, &vault, &engine);
        if case == "take over" {
            eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
            settle().await;
            let held = service.status().unwrap();
            assert!(!on(&held), "{case}: Remote came back on by itself");
            assert!(
                held.station_permissions.is_empty(),
                "{case}: a grant came back by itself"
            );
            assert!(transmitter_idle(&engine), "{case}");
            service.action(Action::Enable {}).await.unwrap();
            eventually(
                &service,
                "the approval's grants came back at Turn on",
                |s| s.station_permissions == [BROWSER],
            )
            .await;
            assert!(transmitter_idle(&engine), "{case}: the latch is still off");
            continue;
        }
        eventually(&service, "Remote back on", on).await;
        settle().await;
        let status = service.action(Action::Refresh {}).await.unwrap();
        assert!(status.station_permissions.is_empty(), "{case}");
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
        let granted = approve(&service).await;
        assert_eq!(
            granted.station_permissions,
            [BROWSER],
            "{case}: positive control"
        );
        settle().await;
        drop(service);

        *cloud.devices.lock().unwrap() = after_restart;
        let service = launch(&cloud, &vault, &engine);
        let status = if restored {
            eventually(&service, case, |s| s.station_permissions == [BROWSER]).await
        } else {
            eventually(&service, "Remote back on", on).await;
            settle().await;
            let status = service.action(Action::Refresh {}).await.unwrap();
            assert!(
                status.station_permissions.is_empty(),
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
    approve(&service).await;
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
    settle().await;
    drop(service);

    let service = launch(&cloud, &vault, &engine);
    let status = eventually(&service, "the renewed approval's grants", |s| {
        s.station_permissions == [BROWSER]
    })
    .await;
    assert_eq!(status.station_permissions, [BROWSER]);
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
                status.station_permissions.is_empty(),
                "{case}: {}",
                serde_json::to_string(&status).unwrap()
            );
            assert!(transmitter_idle(&engine), "{case}");
            continue;
        }
        eventually(&service, case, |s| s.station_permissions == [BROWSER]).await;
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
        eventually(&service, "restored after a renewal", |s| {
            s.station_permissions == [BROWSER]
        })
        .await;
        assert!(transmitter_idle(&engine), "{case}");
    }
}

/// An older build remembered a browser's remote logging on a grant (`"logging":true`). That
/// permission is gone: a browser that held only it is restored nothing and cannot take control, and
/// the record is written again with it `false` (an earlier build still reads it). Positive control:
/// in the same record, a browser that held station control gets it back and takes control.
#[tokio::test(flavor = "multi_thread")]
async fn a_browser_holding_the_old_logging_grant_cannot_take_control() {
    const OTHER: &str = "10000000-0000-4000-8000-00000000000e";
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let grant = |id: &str, control: bool| json!({"deviceId":id,"expiresAt":APPROVED_UNTIL,"generation":2,"logging":true,"control":control});
    let previous = json!({"binding":{"origin":cloud.origin,"stationId":STATION,"accountId":ACCOUNT},
        "enabled":true,"grants":[grant(BROWSER, false), grant(OTHER, true)]});
    vault
        .values
        .lock()
        .unwrap()
        .insert("state".into(), previous.to_string());
    let listed = |id: &str| {
        json!({"id":id,"name":"Test browser","approved":1,"expiresAt":APPROVED_UNTIL,
        "generation":2,"renewsUntil":APPROVED_UNTIL + 60 * DAY_MS})
    };
    *cloud.devices.lock().unwrap() =
        json!({"devices":[listed(BROWSER), listed(OTHER)]}).to_string();
    let engine = engine();
    let service = launch(&cloud, &vault, &engine);
    // The restore is one step for every browser in the record, so once OTHER's grant is back the
    // whole record has been restored.
    eventually(&service, "the old record restored", |s| {
        s.station_permissions == [OTHER]
    })
    .await;
    settle().await;

    let authority = service.control.lock().unwrap().operations.clone();
    let connection = operations::LanConnection::new(authority.clone());
    let acquire = |device: &str| -> String {
        let session = lan_grants::fresh_id();
        let ask = |request: operations::Request| {
            for _ in 0..50 {
                match authority.handle_version(
                    (connection.id, 4),
                    &session,
                    device,
                    &request,
                    &engine,
                    Instant::now(),
                ) {
                    Err("stationBusy" | "remoteBusy") => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    other => return other,
                }
            }
            Err("busy throughout")
        };
        let state = ask(operations::Request::State {
            request_id: lan_grants::fresh_id(),
        })
        .unwrap();
        assert_eq!(
            state["allowed"],
            json!(device == OTHER),
            "{device}: {state}"
        );
        match ask(operations::Request::Acquire {
            request_id: lan_grants::fresh_id(),
            station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        }) {
            Ok(acquired) => {
                let phase = acquired["phase"].as_str().unwrap_or("none").to_string();
                if let Some(lease) = acquired["leaseId"].as_str() {
                    let _ = ask(operations::Request::Release {
                        request_id: lan_grants::fresh_id(),
                        lease_id: lease.into(),
                    });
                }
                phase
            }
            Err(refused) => refused.into(),
        }
    };
    assert_eq!(
        acquire(BROWSER),
        "localPermissionRequired",
        "the old logging grant took control"
    );
    assert_eq!(
        acquire(OTHER),
        "controlling",
        "positive control: station control"
    );

    let record: serde_json::Value =
        serde_json::from_str(&vault.values.lock().unwrap()["state"]).unwrap();
    let grants = record["grants"].as_array().unwrap();
    assert!(
        grants.iter().all(|g| g["logging"] == json!(false)),
        "the old logging grant was written back: {record}"
    );
    assert_eq!(
        grants.iter().map(|g| &g["deviceId"]).collect::<Vec<_>>(),
        [&json!(OTHER)],
        "a browser that held only logging is remembered for nothing: {record}"
    );
    assert!(transmitter_idle(&engine));
}

/// The shack has no remote logging switch: its action is not one Nexus takes. CONTROL: the station
/// control switch, in the same shape, is.
#[test]
fn the_shack_takes_no_remote_logging_switch() {
    let press = |kind: &str| json!({"type":kind,"deviceId":BROWSER,"allow":true});
    assert!(serde_json::from_value::<Action>(press("loggingPermission")).is_err());
    assert!(serde_json::from_value::<Action>(press("stationPermission")).is_ok());
}

/// An older build remembered the old Remote page's FT8/FT4 transmit on a grant (`"transmit":true`).
/// This build still reads that record, restores the browser's station control from it,
/// and writes it again without the field: nothing of the old grant is kept, offered or acted on.
#[tokio::test(flavor = "multi_thread")]
async fn a_record_holding_the_old_pages_transmit_grant_restores_the_rest_and_drops_it() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    // Written before the service reported a generation, so binding to the listed one rewrites it.
    let previous = json!({"binding":{"origin":cloud.origin,"stationId":STATION,"accountId":ACCOUNT},
        "enabled":true,"grants":[{"deviceId":BROWSER,"expiresAt":APPROVED_UNTIL,"logging":true,"control":true,"transmit":true}]});
    vault
        .values
        .lock()
        .unwrap()
        .insert("state".into(), previous.to_string());
    *cloud.devices.lock().unwrap() = device_list_at(1, APPROVED_UNTIL, 2);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    let status = eventually(&service, "the old record restored", |s| {
        s.station_permissions == [BROWSER]
    })
    .await;
    settle().await;
    let record: serde_json::Value =
        serde_json::from_str(&vault.values.lock().unwrap()["state"]).unwrap();
    assert_eq!(
        record["grants"][0]["generation"],
        json!(2),
        "premise: the record was written again: {record}"
    );
    assert!(
        record["grants"][0].get("transmit").is_none(),
        "the old transmit grant was written back: {record}"
    );
    assert!(
        !serde_json::to_string(&status).unwrap().contains("transmit"),
        "no transmit permission is shown: {}",
        serde_json::to_string(&status).unwrap()
    );
    let authority = service.control.lock().unwrap().operations.clone();
    assert_eq!(
        authority.local_status(),
        json!({"controlDevices":[BROWSER],"controller":null})
    );
    assert!(transmitter_idle(&engine));
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
                |s| s.station_permissions == [BROWSER],
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
            status.station_permissions.is_empty(),
            "nothing restored: {}",
            serde_json::to_string(&status).unwrap()
        );
        assert!(transmitter_idle(&engine), "nothing can transmit");
    }
}

/// Downgrade safety. The pairing record is untouched; the remembered-state record this build writes
/// without a generation is still readable by the 1.12.0 build (its `Grant` denies unknown fields),
/// for this build never writes the old Remote page's transmit grant, and writes the old logging
/// permission `false`, never granted, where that build requires it.
#[test]
fn a_record_without_a_generation_stays_readable_by_the_previous_build() {
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
    let state = |generation| vault::State {
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
            control: true,
            retired_logging: Default::default(),
            retired_transmit: Default::default(),
        }],
    };
    let without = serde_json::to_string(&state(None)).unwrap();
    assert!(!without.contains("transmit") && !without.contains("generation"));
    assert!(without.contains("\"logging\":false"), "{without}");
    assert!(serde_json::from_str::<PreviousState>(&without).is_ok());
    // A grant bound to an approval generation (a service with the approval lifetime) is unreadable
    // there too, which that build answers with Remote off and nothing restored; see
    // an_unreadable_remembered_record_leaves_remote_off_with_nothing_restored.
    let bound = serde_json::to_string(&state(Some(2))).unwrap();
    assert!(bound.contains("\"generation\":2"));
    assert!(serde_json::from_str::<PreviousState>(&bound).is_err());
    let read: vault::State = serde_json::from_str(&bound).unwrap();
    assert!(read == state(Some(2)));
    // And this build reads a record the previous build wrote.
    let previous = json!({"binding":{"origin":REMOTE_ORIGIN,"stationId":STATION,"accountId":ACCOUNT},
        "enabled":true,"grants":[{"deviceId":BROWSER,"expiresAt":APPROVED_UNTIL,"logging":true,"control":true}]});
    let read: vault::State = serde_json::from_value(previous).unwrap();
    assert!(read == state(None));
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
        refusing: Default::default(),
        approvals: Default::default(),
        refusing_removals: Default::default(),
        invites: Default::default(),
        holding_lists: Default::default(),
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

/// One trial per callsign and per shack PC: approving sends the station callsign configured here, as
/// configured (trimmed; the service normalises it), and the PC's two identifiers as hashes only. The
/// raw identifier never leaves the computer.
#[tokio::test(flavor = "multi_thread")]
async fn approving_sends_the_station_callsign_and_the_pc_hashes_never_the_raw_ids() {
    let cloud = fake_cloud().await;
    let vault = MemoryVault::default();
    let settings = Settings {
        mycall: " w1abc/p ".into(),
        ..Settings::default()
    };
    let engine = Arc::new(Mutex::new(Engine::with_settings(settings)));
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
        })
        .await
        .unwrap();
    let sent = cloud.approvals.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "{sent:?}");
    let body = sent[0].as_object().unwrap();
    let mut keys: Vec<&str> = body.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["callsign", "credential", "id", "pc", "proof"]);
    assert_eq!(body["callsign"], "w1abc/p");
    let pc = body["pc"].as_object().unwrap();
    let mut kinds: Vec<&str> = pc.keys().map(String::as_str).collect();
    kinds.sort_unstable();
    assert_eq!(kinds, ["hw", "os"]);
    // By value where this computer has the identifier: the hash of the kind and the lower-cased id.
    #[cfg(target_os = "linux")]
    if let Some(id) = std::fs::read_to_string("/etc/machine-id")
        .ok()
        .map(|id| id.trim().to_ascii_lowercase())
        .filter(|id| !id.is_empty())
    {
        let expected: String = ring::digest::digest(
            &ring::digest::SHA256,
            format!("nexus-pc:os:{id}").as_bytes(),
        )
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
        assert_eq!(pc["os"], expected.as_str());
        assert!(
            !sent[0].to_string().contains(&id),
            "the raw machine id was sent"
        );
    }
    for kind in ["hw", "os"] {
        assert!(
            pc[kind].is_null() || pc[kind].as_str().is_some_and(|h| h.len() == 64),
            "{kind}: {}",
            pc[kind]
        );
    }
}

/// The service's refusals of a trial for this callsign or this PC, or of an older Nexus, are named at
/// the shack. Unnamed, each 400 or 403 read as a failed request or `accessDenied`.
#[tokio::test(flavor = "multi_thread")]
async fn the_trial_identity_refusals_are_named_at_the_shack() {
    for (status, code) in [
        ("400 Bad Request", "callsignInvalid"),
        ("403 Forbidden", "callsignNotLicensed"),
        ("403 Forbidden", "trialUsedByCallsign"),
        ("403 Forbidden", "trialUsedOnThisPc"),
        ("403 Forbidden", "updateRequired"),
    ] {
        let body: &'static str = Box::leak(format!(r#"{{"error":"{code}"}}"#).into_boxed_str());
        let origin = refusing_cloud(status, body).await;
        let client = Client::new(&origin).unwrap();
        assert_eq!(
            client
                .post("enroll/approve", None, json!({ "id": "pairing" }))
                .await
                .err(),
            Some(code)
        );
    }
    // The control: a code outside the list keeps the status mapping.
    let origin = refusing_cloud("403 Forbidden", r#"{"error":"somethingElse"}"#).await;
    let client = Client::new(&origin).unwrap();
    assert_eq!(
        client
            .post("enroll/approve", None, json!({ "id": "pairing" }))
            .await
            .err(),
        Some("accessDenied")
    );
}

/// Approval refused because the same mailbox already has a trial running under another sign-in. Unnamed,
/// the 403 read as `accessDenied` and the shack told the operator access was denied, which says nothing
/// about the sign-in that needs changing.
#[tokio::test(flavor = "multi_thread")]
async fn a_trial_running_under_another_sign_in_is_named_at_the_shack() {
    let origin = refusing_cloud("403 Forbidden", r#"{"error":"trialActiveElsewhere"}"#).await;
    let client = Client::new(&origin).unwrap();
    assert_eq!(
        client
            .post("enroll/approve", None, json!({ "id": "pairing" }))
            .await
            .err(),
        Some("trialActiveElsewhere")
    );
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
async fn approve_shown(service: &Service, shown: Option<String>) -> Status {
    service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: true,
            key: shown,
            kind: Some(Kind::Browser),
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
        let status = approve_shown(&service, shown).await;
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
    approve_shown(&service, Some(hex(&key.pin()))).await;
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
            key: None,
            kind: None,
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

/// A computer running Nexus (the native client), as the service lists it to a Nexus that asks for
/// kinds: its device key, its kind and its platform.
const COMPUTER: &str = "10000000-0000-4000-8000-00000000000d";
fn computer_listed(approved: u8, key: &DeviceKey) -> serde_json::Value {
    let mut listed = json!({"id":COMPUTER,"name":"Shack laptop","approved":approved,
        "expiresAt":if approved == 1 { APPROVED_UNTIL } else { APPROVED_UNTIL - 1 },
        "kind":"computer","platform":"windows","publicKey":key.public_key});
    if approved == 1 {
        // The approval the fake service writes when the shack approves it.
        listed["generation"] = 2.into();
    }
    listed
}

/// ★ The native client's devices reach the shack: this Nexus asks for kinds, so the service lists a
/// computer beside the browsers, each with its kind and platform, and the desktop is given both. A
/// service without kinds lists browsers alone, with neither field, and each is a browser.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_is_listed_with_its_kind_and_platform_and_a_service_without_kinds_lists_browsers(
) {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let key = DeviceKey::new();
    // The fake lists the computer only to a Nexus that sends the kinds header, as the service does,
    // so seeing it here is the header having been sent.
    *cloud.devices.lock().unwrap() = json!({"devices":[
        {"id":BROWSER,"name":"Test browser","approved":1,"expiresAt":APPROVED_UNTIL,
            "kind":"browser","platform":null},
        computer_listed(0, &key),
    ]})
    .to_string();
    let listed = service.action(Action::Refresh {}).await.unwrap();
    let shown: Vec<_> = listed
        .devices
        .iter()
        .map(|d| (d.id.as_str(), d.kind, d.platform, d.key.clone()))
        .collect();
    assert_eq!(
        shown,
        [
            (BROWSER, Kind::Browser, None, None),
            (
                COMPUTER,
                Kind::Computer,
                Some(Platform::Windows),
                Some(hex(&key.pin()))
            ),
        ]
    );
    let page = serde_json::to_value(&listed).unwrap();
    assert_eq!(
        (
            &page["devices"][0]["kind"],
            &page["devices"][0]["platform"],
            &page["devices"][1]["kind"],
            &page["devices"][1]["platform"],
        ),
        (
            &json!("browser"),
            &json!(null),
            &json!("computer"),
            &json!("windows")
        ),
        "what the desktop is given"
    );
    assert!(
        page["devices"][1].get("publicKey").is_none(),
        "the key itself never reaches the desktop page"
    );
    // A service without kinds: 1.12.0's shape, which names no kind.
    *cloud.devices.lock().unwrap() = device_list(1, APPROVED_UNTIL);
    let listed = service.action(Action::Refresh {}).await.unwrap();
    let shown: Vec<_> = listed
        .devices
        .iter()
        .map(|d| (d.id.as_str(), d.kind, d.platform))
        .collect();
    assert_eq!(shown, [(BROWSER, Kind::Browser, None)]);
}

/// ★ Each device's kind and platform are held to the service's own values: a pair it never lists
/// refuses the list, as an id or a name out of shape does.
#[tokio::test(flavor = "multi_thread")]
async fn a_devices_kind_and_platform_are_held_to_the_services_values() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let key = DeviceKey::new();
    let listing = |kind: serde_json::Value, platform: serde_json::Value| {
        let mut device = computer_listed(0, &key);
        device["kind"] = kind;
        device["platform"] = platform;
        json!({ "devices": [device] }).to_string()
    };
    for (kind, platform) in [
        ("computer", "windows"),
        ("computer", "macos"),
        ("computer", "linux"),
        ("phone", "ios"),
        ("phone", "android"),
    ] {
        *cloud.devices.lock().unwrap() = listing(json!(kind), json!(platform));
        let listed = service.action(Action::Refresh {}).await;
        assert!(listed.is_ok(), "control: {kind} on {platform} is taken");
    }
    *cloud.devices.lock().unwrap() = listing(json!("browser"), json!(null));
    assert!(
        service.action(Action::Refresh {}).await.is_ok(),
        "control: a browser on none"
    );
    for (kind, platform) in [
        (json!("browser"), json!("windows")),
        (json!("computer"), json!(null)),
        (json!("computer"), json!("ios")),
        (json!("computer"), json!("android")),
        (json!("phone"), json!("linux")),
        (json!("phone"), json!(null)),
        (json!("tablet"), json!("windows")),
        (json!("computer"), json!("beos")),
        (json!("Computer"), json!("windows")),
        (json!(null), json!("windows")),
    ] {
        *cloud.devices.lock().unwrap() = listing(kind.clone(), platform.clone());
        assert!(
            matches!(
                service.action(Action::Refresh {}).await,
                Err("invalidResponse")
            ),
            "{kind} on {platform} is no device the service lists"
        );
    }
}

/// ★ Approving a computer pins the key the shack showed beside it, as a browser's is (A5), for the
/// tunnel's TLS to check, and grants nothing on the relay's lanes: no station control
/// under its id, and the switch cannot give it one. What it holds is station control under the shack's own name for its key while Remote is on
/// (the operator's decision of 2026-10-10), as a LAN computer does. The pin outlives a restart, still
/// granting nothing more; revoking the computer removes it.
#[tokio::test(flavor = "multi_thread")]
async fn approving_a_computer_pins_its_key_and_grants_nothing_on_the_relays_lanes() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    let key = DeviceKey::new();
    let fingerprint = hex(&key.pin());
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    let listed = service.action(Action::Refresh {}).await.unwrap();
    assert!(listed.pinned_devices.is_empty(), "premise: nothing pinned");
    let status = service
        .action(Action::Device {
            device_id: COMPUTER.into(),
            approve: true,
            key: Some(fingerprint.clone()),
            kind: Some(Kind::Computer),
        })
        .await
        .unwrap();
    assert_eq!(status.devices[0].approved, 1, "premise: approved");
    assert_eq!(status.pinned_devices, [COMPUTER], "its key is pinned");
    // As a computer's, under this approval: never the fingerprint a browser's pin is.
    let pin = native_pin(&fingerprint, 2).unwrap();
    assert_ne!(pin, fingerprint);
    assert_eq!(
        pinned_device(&service, COMPUTER).map(|p| hex(&p)),
        Some(pin.clone())
    );
    let granted = |status: &Status| status.station_permissions.clone();
    let named = lan::tunnel_device_id(&key.pin());
    assert_eq!(
        granted(&status),
        vec![named.clone()],
        "nothing on the relay's lanes"
    );
    assert!(
        service
            .control
            .lock()
            .unwrap()
            .remembered
            .grants()
            .is_empty(),
        "nothing remembered to give back"
    );
    let allow = Action::StationPermission {
        device_id: COMPUTER.into(),
        allow: true,
    };
    assert!(matches!(service.action(allow).await, Err("accessDenied")));
    assert_eq!(granted(&service.status().unwrap()), vec![named.clone()]);
    settle().await;
    assert_eq!(
        vault
            .pins()
            .unwrap()
            .and_then(|p| p.keys.get(COMPUTER).cloned()),
        Some(pin.clone()),
        "the pins entry"
    );
    // A restart: the pin comes back, and still nothing on the lanes.
    drop(service);
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    assert_eq!(
        pinned_device(&service, COMPUTER).map(|p| hex(&p)),
        Some(pin),
        "the pin survived"
    );
    let listed = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        listed.pinned_devices,
        [COMPUTER],
        "shown pinned after the restart"
    );
    assert_eq!(granted(&listed), vec![named.clone()]);
    service
        .action(Action::Device {
            device_id: COMPUTER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert_eq!(
        pinned_device(&service, COMPUTER),
        None,
        "revoking left the pin"
    );
}
fn pinned_device(service: &Service, device: &str) -> Option<[u8; 32]> {
    service.control.lock().unwrap().remembered.pinned(device)
}

/// ★ The room's tunnel takes a computer approved here by the key pinned as it was approved, and
/// nothing else: a browser's pin is never one; a computer the service lists with another key than
/// its pin is not one; and a computer removed here is out, with the change told at once to every
/// session through the room.
#[tokio::test(flavor = "multi_thread")]
async fn the_tunnel_takes_an_approved_computer_by_its_pinned_key_and_never_a_browser() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    let approved = service.control.lock().unwrap().approved.clone();
    let mut told = approved.roster().subscribe.as_ref()();
    // A browser, approved with its key pinned (A5): the tunnel never takes it.
    let browser = DeviceKey::new();
    pending_keyed_browser(&cloud, &browser.public_key);
    service.action(Action::Refresh {}).await.unwrap();
    approve_shown(&service, Some(hex(&browser.pin()))).await;
    assert_eq!(
        pinned(&service),
        Some(browser.pin()),
        "premise: the browser is pinned"
    );
    assert_eq!(approved.paired(&browser.pin()), None, "a browser's pin");
    // A computer, approved with its key pinned: taken, by that key alone.
    let key = DeviceKey::new();
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(approved.paired(&key.pin()), None, "waiting: not yet");
    let before = approved.revisions();
    service
        .action(Action::Device {
            device_id: COMPUTER.into(),
            approve: true,
            key: Some(hex(&key.pin())),
            kind: Some(Kind::Computer),
        })
        .await
        .unwrap();
    // Taken as the name its key gives through the room, never as the service's id for it.
    let named = lan::tunnel_device_id(&key.pin());
    assert_ne!(named, COMPUTER);
    assert_eq!(approved.paired(&key.pin()), Some(named.clone()));
    assert!(approved.revisions() > before, "the change is counted");
    assert!(told.has_changed().unwrap(), "and told");
    told.mark_unchanged();
    // The service lists another key for it: not the pin, so not taken.
    let other = DeviceKey::new();
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(1, &other)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        approved.paired(&key.pin()),
        None,
        "a listing with another key"
    );
    assert_eq!(
        approved.paired(&other.pin()),
        None,
        "nor the key never pinned"
    );
    assert!(told.has_changed().unwrap(), "told");
    // Listed with its pinned key again, then removed here: out, and told.
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(1, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(approved.paired(&key.pin()), Some(named), "control: back");
    told.mark_unchanged();
    service
        .action(Action::Device {
            device_id: COMPUTER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert_eq!(approved.paired(&key.pin()), None, "removed here");
    assert!(told.has_changed().unwrap(), "told at once");
}

use lan_grants::lan_acquire;

/// The shack's own screen's press, read as `remote_station_action` reads it.
fn press(value: serde_json::Value) -> Action {
    serde_json::from_value(value).unwrap()
}
/// Approve `device` here as the operator was shown it: a computer, with `key` beside it.
async fn approve_computer(
    service: &Service,
    device: &str,
    key: &DeviceKey,
) -> Result<Status, &'static str> {
    service
        .action(Action::Device {
            device_id: device.into(),
            approve: true,
            key: Some(hex(&key.pin())),
            kind: Some(Kind::Computer),
        })
        .await
}
/// A service with Remote on, through `cloud`.
async fn on_at(cloud: &FakeCloud, engine: &crate::SharedEngine) -> Service {
    let service = launch(cloud, &paired_vault(&cloud.origin), engine);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    service
}
fn engine() -> crate::SharedEngine {
    Arc::new(Mutex::new(Engine::with_settings(Settings::default())))
}

/// ★ An approval is of the device as the operator was shown it. A computer shown as one (Nexus on
/// Windows, its key, nothing on the relay's lanes) that the service names a browser once the press
/// is in is granted nothing on those lanes and pinned for nothing: the press is the shack's own
/// screen's, as it sent it before it carried the kind it showed.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_approved_as_shown_is_never_granted_as_the_browser_the_service_names_after() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    let shown = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        shown.devices[0].kind,
        Kind::Computer,
        "premise: shown as one"
    );
    // Once the press is in, the service names it a browser.
    let mut renamed = computer_listed(0, &key);
    renamed["kind"] = "browser".into();
    renamed.as_object_mut().unwrap().remove("platform");
    *cloud.devices.lock().unwrap() = json!({ "devices": [renamed] }).to_string();
    let status = service
        .action(press(
            json!({"type":"device","deviceId":COMPUTER,"approve":true,
            "key":hex(&key.pin())}),
        ))
        .await
        .unwrap();
    assert_eq!(
        status.station_permissions,
        Vec::<String>::new(),
        "the computer approved was granted as a browser"
    );
    assert!(
        status.pinned_devices.is_empty(),
        "pinned as something it was not shown as"
    );
}

/// ★ The press carries the kind it was shown, and the shack grants and pins only for that kind. A
/// computer shown as one that the service names a browser after the press: nothing on the relay's
/// lanes, no pin. CONTROL: a browser shown and listed as one is granted what one approval gives; a
/// computer shown and listed as one is pinned as a computer, and granted nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_press_carries_the_kind_shown_and_the_shack_acts_on_no_other() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let granted = |status: &Status| status.station_permissions.clone();
    let key = DeviceKey::new();
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    let mut renamed = computer_listed(0, &key);
    renamed["kind"] = "browser".into();
    renamed.as_object_mut().unwrap().remove("platform");
    *cloud.devices.lock().unwrap() = json!({ "devices": [renamed] }).to_string();
    let status = approve_computer(&service, COMPUTER, &key).await.unwrap();
    assert_eq!(
        granted(&status),
        Vec::<String>::new(),
        "granted as a browser"
    );
    assert!(status.pinned_devices.is_empty(), "pinned as a browser");
    // CONTROL: a browser shown and listed as one.
    let browser = DeviceKey::new();
    pending_keyed_browser(&cloud, &browser.public_key);
    service.action(Action::Refresh {}).await.unwrap();
    let status = approve_shown(&service, Some(hex(&browser.pin()))).await;
    let one = vec![BROWSER.to_string()];
    assert_eq!(granted(&status), one);
    assert_eq!(status.pinned_devices, [BROWSER]);
    // CONTROL: a computer shown and listed as one.
    let computer = DeviceKey::new();
    *cloud.devices.lock().unwrap() =
        json!({ "devices": [computer_listed(0, &computer)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    let status = approve_computer(&service, COMPUTER, &computer)
        .await
        .unwrap();
    assert_eq!(status.pinned_devices, [COMPUTER]);
    assert!(
        !status.station_permissions.contains(&COMPUTER.to_string()),
        "a computer granted on the relay's lanes"
    );
}

/// ★ The service's ids never meet the names the shack gives its computers: a browser the service
/// lists under the name an approved computer is taken as through the room is refused with the list,
/// so approving it can never hand that computer a browser's grant, remembered under its name.
/// Premise: the computer holds station control of its own, as every approved computer does while
/// Remote is on (the operator's decision of 2026-10-10), and nothing else.
#[tokio::test(flavor = "multi_thread")]
async fn a_browser_listed_under_a_computers_name_through_the_room_is_refused() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    approve_computer(&service, COMPUTER, &key).await.unwrap();
    let approved = service.control.lock().unwrap().approved.clone();
    let taken = approved.paired(&key.pin()).expect("premise: taken");
    assert_eq!(
        lan_acquire(&service, &engine, &taken),
        "controlling",
        "premise: its own station control"
    );
    let held = |service: &Service| {
        let control = service.control.lock().unwrap();
        control
            .remembered
            .grants()
            .iter()
            .any(|g| g.device_id == taken)
    };
    assert!(!held(&service), "control: it holds no browser's grant");
    let browser = DeviceKey::new();
    let mut listed: serde_json::Value =
        serde_json::from_str(&cloud.devices.lock().unwrap()).unwrap();
    let mut twin: serde_json::Value = serde_json::from_str(&device_list(0, APPROVED_UNTIL - 1))
        .map(|l: serde_json::Value| l["devices"][0].clone())
        .unwrap();
    twin["id"] = taken.clone().into();
    twin["publicKey"] = browser.public_key.clone().into();
    listed["devices"].as_array_mut().unwrap().push(twin);
    *cloud.devices.lock().unwrap() = listed.to_string();
    let refreshed = service.action(Action::Refresh {}).await;
    let _ = service
        .action(Action::Device {
            device_id: taken.clone(),
            approve: true,
            key: Some(hex(&browser.pin())),
            kind: Some(Kind::Browser),
        })
        .await;
    assert!(
        !held(&service),
        "the computer through the room took the grants given the browser named after it"
    );
    assert!(
        matches!(refreshed, Err("invalidResponse")),
        "a browser named as the shack names a computer was listed"
    );
}

/// ★ One id names one device. A list naming two devices under one id is refused whole, as a
/// malformed id is, so a press meant for one never reaches the other: here a computer listed under
/// the id of a browser approved with its key pinned, whose approval would have unpinned the browser.
#[tokio::test(flavor = "multi_thread")]
async fn a_list_naming_two_devices_under_one_id_is_refused_whole() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let browser = DeviceKey::new();
    pending_keyed_browser(&cloud, &browser.public_key);
    service.action(Action::Refresh {}).await.unwrap();
    approve_shown(&service, Some(hex(&browser.pin()))).await;
    assert_eq!(pinned(&service), Some(browser.pin()), "premise: pinned");
    let computer = DeviceKey::new();
    let mut twin = computer_listed(0, &computer);
    twin["id"] = BROWSER.into();
    let mut listed: serde_json::Value =
        serde_json::from_str(&cloud.devices.lock().unwrap()).unwrap();
    listed["devices"].as_array_mut().unwrap().push(twin);
    *cloud.devices.lock().unwrap() = listed.to_string();
    let refreshed = service.action(Action::Refresh {}).await;
    let _ = service
        .action(press(
            json!({"type":"device","deviceId":BROWSER,"approve":true,
            "key":hex(&computer.pin())}),
        ))
        .await;
    assert_eq!(
        pinned(&service),
        Some(browser.pin()),
        "a press under a shared id reached the browser"
    );
    let approved = service.control.lock().unwrap().approved.clone();
    assert_eq!(approved.paired(&computer.pin()), None);
    assert!(
        matches!(refreshed, Err("invalidResponse")),
        "two devices under one id were listed"
    );
}

/// ★ Through the room a computer holds nothing granted to another device under the service's id
/// for it. Approved here, shown as a computer, under the id of a browser approved with station
/// control, the device the tunnel takes it as cannot take the lease.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_through_the_room_never_holds_a_browsers_grants_under_its_id() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let browser = DeviceKey::new();
    pending_keyed_browser(&cloud, &browser.public_key);
    service.action(Action::Refresh {}).await.unwrap();
    let granted = approve_shown(&service, Some(hex(&browser.pin()))).await;
    assert!(
        granted.station_permissions.contains(&BROWSER.to_string()),
        "premise: the browser's grant"
    );
    let computer = DeviceKey::new();
    let mut listed = computer_listed(0, &computer);
    listed["id"] = BROWSER.into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [listed] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    approve_computer(&service, BROWSER, &computer)
        .await
        .unwrap();
    let approved = service.control.lock().unwrap().approved.clone();
    let taken = approved
        .paired(&computer.pin())
        .expect("premise: the tunnel takes the computer approved here");
    assert_ne!(taken, BROWSER, "named by the service's id");
    // Since 2026-10-10 a computer approved here holds station control of its own while Remote is on
    // (as a LAN computer does), under its own name. It holds none of the browser's grants: its
    // control is its own, untouched when the browser's is revoked.
    service
        .action(Action::StationPermission {
            device_id: BROWSER.into(),
            allow: false,
        })
        .await
        .unwrap();
    assert!(
        !service
            .status()
            .unwrap()
            .station_permissions
            .contains(&BROWSER.to_string()),
        "control: the browser's own station control is revoked"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &taken),
        "controlling",
        "the computer's control was the browser's, and went with it"
    );
}

/// ★ A browser's approval admits nothing through the room: a browser approved here with its key
/// pinned, which the service then names an approved computer under the same id and key, is not
/// taken by the tunnel. CONTROL: approved here as a computer, the same key is.
#[tokio::test(flavor = "multi_thread")]
async fn a_browsers_pin_never_admits_a_computer_through_the_room() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let browser = DeviceKey::new();
    pending_keyed_browser(&cloud, &browser.public_key);
    service.action(Action::Refresh {}).await.unwrap();
    approve_shown(&service, Some(hex(&browser.pin()))).await;
    assert_eq!(pinned(&service), Some(browser.pin()), "premise: pinned");
    let approved = service.control.lock().unwrap().approved.clone();
    let mut renamed = computer_listed(1, &browser);
    renamed["id"] = BROWSER.into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [renamed] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        approved.paired(&browser.pin()),
        None,
        "a browser's pin admitted a computer"
    );
    approve_computer(&service, BROWSER, &browser).await.unwrap();
    assert!(
        approved.paired(&browser.pin()).is_some(),
        "control: approved as a computer"
    );
}

/// ★ A computer's pin is its approval's. One the service lists as approved again under an approval
/// the shack never gave (another generation) is not taken on the pin made for the one before.
/// CONTROL: renewed on use (the same approval, a later end), it is.
#[tokio::test(flavor = "multi_thread")]
async fn an_approval_the_shack_never_gave_is_not_taken_on_an_older_pin() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    approve_computer(&service, COMPUTER, &key).await.unwrap();
    let approved = service.control.lock().unwrap().approved.clone();
    assert!(approved.paired(&key.pin()).is_some(), "premise: taken");
    let mut renewed = computer_listed(1, &key);
    renewed["expiresAt"] = (APPROVED_UNTIL + DAY_MS).into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [renewed] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert!(
        approved.paired(&key.pin()).is_some(),
        "control: renewed on use"
    );
    let mut again = computer_listed(1, &key);
    again["generation"] = 4.into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [again] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        approved.paired(&key.pin()),
        None,
        "an approval the shack never gave was taken"
    );
}

/// ★ An approval ends where the service last said it ends, at the shack as at the room: a computer
/// whose approval has run out is not taken by the tunnel, before the service is asked again.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_whose_approval_ran_out_is_not_taken() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    approve_computer(&service, COMPUTER, &key).await.unwrap();
    let approved = service.control.lock().unwrap().approved.clone();
    let mut ending = computer_listed(1, &key);
    ending["expiresAt"] = (now_ms() + 1500).into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [ending] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert!(
        approved.paired(&key.pin()).is_some(),
        "premise: taken before its end"
    );
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert_eq!(
        approved.paired(&key.pin()),
        None,
        "taken after its approval ran out"
    );
}

/// A lookup that fails is never read as approved: an approval whose list will not come pins
/// nothing, and the tunnel takes nobody. An empty list takes nobody either. CONTROL: the list back,
/// the same approval pins and is taken.
#[tokio::test(flavor = "multi_thread")]
async fn a_list_that_fails_or_is_empty_admits_nobody() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    let approved = service.control.lock().unwrap().approved.clone();
    cloud.refusing.store(true, Ordering::SeqCst);
    assert!(approve_computer(&service, COMPUTER, &key).await.is_err());
    assert_eq!(pinned_device(&service, COMPUTER), None, "pinned on no list");
    assert_eq!(approved.paired(&key.pin()), None, "taken on no list");
    cloud.refusing.store(false, Ordering::SeqCst);
    approve_computer(&service, COMPUTER, &key).await.unwrap();
    assert!(approved.paired(&key.pin()).is_some(), "control: taken");
    *cloud.devices.lock().unwrap() = json!({ "devices": [] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(approved.paired(&key.pin()), None, "taken on an empty list");
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
    approve_shown(&service, Some(hex(&key.pin()))).await;
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
    assert!(grant.control, "a grant lost");
    assert!(
        !grant.logging,
        "the retired logging permission was written as granted"
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
    let approved = approve_shown(&service, Some(hex(&old.pin()))).await;
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
    let again = approve_shown(&service, Some(hex(&new.pin()))).await;
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

// ---- This station's key, shown at the shack (security review S3-L1) -----------------------------
//
// The Remote page keeps the first key the service lists for a station and shows its fingerprint. The
// shack shows the fingerprint of the key it signs with, worked out the same way, so the operator can
// compare the two: equal means the service lists the station's own key.

#[tokio::test(flavor = "multi_thread")]
async fn s3l1_the_shack_shows_the_fingerprint_of_its_own_key_while_it_is_paired() {
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
    assert_eq!(
        service.status().unwrap().station_key,
        None,
        "no key before the pairing"
    );
    service
        .action(Action::Approve {
            enrollment_id: STATION.into(),
            account_id: ACCOUNT.into(),
        })
        .await
        .unwrap();
    let made = kept_station_key(&vault).expect("approving the pairing makes the station's key");
    // What a page shows for the key the service lists: SHA-256 of its SPKI, lowercase hex.
    let spki = tempo_stream::protocol::hex_bytes(&made).unwrap();
    let expected: String = ring::digest::digest(&ring::digest::SHA256, &spki)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let status = service.status().unwrap();
    assert_eq!(status.station_key.as_deref(), Some(expected.as_str()));
    assert_eq!(
        serde_json::to_value(&status).unwrap()["stationKey"],
        expected.as_str(),
        "the desktop is given it as stationKey"
    );
    service.action(Action::Forget {}).await.unwrap();
    assert_eq!(
        service.status().unwrap().station_key,
        None,
        "it goes with the pairing"
    );
}

#[test]
fn s3l1_the_shack_and_the_page_fingerprint_a_key_alike() {
    // A made-up key shape (the P-256 SPKI prefix, 04, then 0xab sixty-four times), not a key. The
    // page's tests show this same fingerprint as A8DD D2FF AD49 30AC 6B77 647B 8DE0 D379.
    let shape = format!(
        "{}04{}",
        tempo_stream::protocol::P256_SPKI_PREFIX_HEX,
        "ab".repeat(64)
    );
    assert_eq!(
        fingerprint(&shape).as_deref(),
        Some("a8ddd2ffad4930ac6b77647b8de0d37935aaeb9fb957326da6370df67205dc77")
    );
    assert_eq!(fingerprint("not a key"), None, "control: refused");
}

/// ★ EVERY DECISION THAT ENDS AN APPROVAL ENDS ITS KEPT WAIVER, in the record and in the vault: an
/// off that ends access, a removal that clears every grant (`ClearGrants`), a restrict, a list or
/// approvals that no longer name the device, a grant on its own sign-in again (`Hold`), a pairing
/// removed, and a pairing loaded or made with no waivers. CONTROL: Turn off and a list that still
/// names the device keep it, and write nothing.
#[test]
fn every_decision_that_ends_an_approval_ends_its_kept_waiver() {
    let binding = || Binding {
        origin: "https://remote.test.invalid".into(),
        station_id: STATION.into(),
        account_id: ACCOUNT.into(),
    };
    let approval = Approval {
        expires_at: APPROVED_UNTIL,
        generation: Some(2),
    };
    let cases: Vec<(&str, Persist, bool)> = vec![
        ("Turn off", Persist::Paused, true),
        ("listed", Persist::Listed(vec![BROWSER.into()]), true),
        ("off", Persist::Off, false),
        ("ClearGrants", Persist::ClearGrants, false),
        (
            "restrict",
            Persist::Browser {
                device_id: BROWSER.into(),
                approval: None,
                control: Some(false),
            },
            false,
        ),
        ("not listed", Persist::Listed(Vec::new()), false),
        ("not signed in", Persist::SignedIn(Vec::new()), false),
        ("no approvals", Persist::Approvals(Vec::new()), false),
        (
            "approved again",
            Persist::Approvals(vec![(
                BROWSER.into(),
                Approval {
                    generation: Some(3),
                    ..approval
                },
            )]),
            false,
        ),
        (
            "held again",
            Persist::Hold {
                device_id: BROWSER.into(),
            },
            false,
        ),
        ("removed", Persist::Removed, false),
        (
            "bound afresh",
            Persist::Bound(binding(), None, None, None),
            false,
        ),
    ];
    for (what, decision, kept) in cases {
        let mut remembered = Remembered::default();
        let _ = remembered.apply(Persist::Bound(binding(), None, None, None));
        let _ = remembered.apply(Persist::Browser {
            device_id: BROWSER.into(),
            approval: Some(approval),
            control: Some(true),
        });
        let _ = remembered.apply(Persist::Waive {
            device_id: BROWSER.into(),
        });
        assert!(remembered.waivers.contains(BROWSER), "{what}: premise");
        let writes = remembered.apply(decision);
        assert_eq!(remembered.waivers.contains(BROWSER), kept, "{what}");
        let removes = writes
            .iter()
            .any(|w| matches!(w, super::Write::RemoveWaivers));
        let saves = writes
            .iter()
            .any(|w| matches!(w, super::Write::SaveWaivers(_)));
        assert_eq!((removes, saves), (!kept, false), "{what}: written");
    }
}

/// A pairing's remembered state granting the owner's browser station control, and the waiver kept
/// beside it.
fn waived_browser() -> Remembered {
    let mut remembered = Remembered::default();
    let _ = remembered.apply(Persist::Bound(
        Binding {
            origin: "https://remote.test.invalid".into(),
            station_id: STATION.into(),
            account_id: ACCOUNT.into(),
        },
        None,
        None,
        None,
    ));
    let _ = remembered.apply(Persist::Browser {
        device_id: BROWSER.into(),
        approval: Some(Approval {
            expires_at: APPROVED_UNTIL,
            generation: Some(2),
        }),
        control: Some(true),
    });
    let _ = remembered.apply(Persist::Waive {
        device_id: BROWSER.into(),
    });
    remembered
}

/// ★ THE WAIVERS ARE WRITTEN FIRST: station control taken back queues the waiver's removal ahead of
/// the state, and the writer writes the waivers ahead of the state in a batch whatever order they
/// came in, so a crash between the two leaves the device held, never a waiver beside a grant the
/// station took back.
#[test]
fn the_waivers_are_written_before_the_state() {
    let mut remembered = waived_browser();
    let state = remembered.state.clone().unwrap();
    let writes = remembered.apply(Persist::Browser {
        device_id: BROWSER.into(),
        approval: None,
        control: Some(false),
    });
    assert!(
        matches!(
            writes.as_slice(),
            [super::Write::RemoveWaivers, super::Write::Save(_)]
        ),
        "the restrict queues the state first"
    );

    let vault = MemoryVault::default();
    let (queue, writes) = mpsc::unbounded_channel();
    queue.send(super::Write::Save(state)).unwrap();
    queue.send(super::Write::RemoveWaivers).unwrap();
    drop(queue);
    super::write_remembered(Arc::new(vault.clone()), writes);
    assert_eq!(
        *vault.written.lock().unwrap(),
        ["waivers", "state"],
        "the writer writes the state first"
    );
}

/// ★ A WAIVERS WRITE THAT DID NOT LAND IS TRIED AGAIN, AND HOLDS THE STATE BACK: while the store
/// refuses the waivers entry, a waiver the station took back stays in it, so a state saved beside it
/// would hand that waiver a grant again at the next launch. The state is removed instead, each time,
/// and the waivers write is tried again ahead of every later batch; once it lands, the latest state
/// is saved again, in a batch with no state of its own too.
#[test]
fn a_waivers_write_that_did_not_land_is_tried_again_and_holds_the_state_back() {
    let state = waived_browser().state.unwrap();
    let vault = MemoryVault::default();
    vault
        .values
        .lock()
        .unwrap()
        .insert("waivers".into(), "kept".into());
    vault.refusing.lock().unwrap().insert("waivers");
    let (queue, writes) = mpsc::unbounded_channel();
    let writer = {
        let vault = vault.clone();
        std::thread::spawn(move || super::write_remembered(Arc::new(vault), writes))
    };
    // Each write, once the writer has asked the store for `entries` in all, or after a while.
    let write = |write: super::Write, entries: usize| {
        queue.send(write).unwrap();
        for _ in 0..500 {
            if vault.written.lock().unwrap().len() >= entries {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    let has = |entry: &str| vault.values.lock().unwrap().contains_key(entry);

    write(super::Write::RemoveWaivers, 1);
    assert!(has("waivers"), "premise: refused");
    write(super::Write::Save(state.clone()), 3);
    assert_eq!(
        vault.written.lock().unwrap()[1..],
        ["waivers", "state"],
        "the waivers write was not tried again"
    );
    assert!(
        !has("state"),
        "a state saved beside a waiver the station took back"
    );

    vault.refusing.lock().unwrap().clear();
    write(super::Write::RemovePins, 6);
    assert!(!has("waivers"), "the waivers write never landed");
    assert!(
        has("state"),
        "the state is still held back once the waivers landed"
    );
    drop(queue);
    writer.join().unwrap();
}
