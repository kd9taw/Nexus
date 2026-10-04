//! Local approval and the Remote background task. Engine access is restricted to
//! bounded observation, reviewed application reads and separately approved manual logging.
mod application;
mod aprs;
/// Receive audio for a listening browser. Gated with the radio feature because the
/// encoder lives in tempo-audio, which a build without it does not have at all.
#[cfg(feature = "radio")]
mod audio;
/// Remote over this network: the shack's own listener, for a paired computer on the same network.
pub(crate) mod lan;
/// The relay's older lanes, held to the browser's own key (security review S1-M1).
mod lanes;
pub(crate) mod operations;
pub(crate) mod query;
pub(crate) mod sstv;
/// The station's own signing key, which signs its stream answers (security review S3-M1).
mod station_key;
#[cfg(test)]
pub(crate) mod stored_log_tests;
/// Remote as a stream: the `streamSignal` lane and the session thread (see its header).
pub(crate) mod stream;
#[cfg(test)]
mod tests;
mod transport;
pub(crate) mod vault;

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot, watch};
/// The one Remote origin: the service this station talks to, and the page the Remote stations
/// window shows (`remote_window`).
pub(crate) use transport::REMOTE_ORIGIN;
use transport::{credential, empty, identifier, station_path, Client};
use vault::{Binding, SystemVault, Vault};

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    phase: String,
    origin: String,
    station_id: Option<String>,
    account_id: Option<String>,
    pairing_id: Option<String>,
    pairing_code: Option<String>,
    expires_at: Option<u64>,
    devices: Vec<Device>,
    error: Option<&'static str>,
    observation_generation: Option<String>,
    logging_permissions: Vec<String>,
    station_permissions: Vec<String>,
    transmit_permissions: Vec<String>,
    logging_controller: Option<String>,
    /// A5: the browsers whose listed device key is the one pinned at the radio, the ones that can
    /// stream. An approved browser listed with a key and missing here is approved again to stream.
    pinned_devices: Vec<String>,
    /// S3-M1: the service holds another signing key for this station, so browsers refuse its stream
    /// until it is paired again. Its own field, because it lasts past any one request's error.
    key_refused: bool,
    /// Remote over this network (`lan`): its switch, its paired computers and its pairing window.
    /// Absent only from a build of the service without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    lan: Option<lan::LanStatus>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Device {
    id: String,
    name: String,
    approved: u8,
    expires_at: u64,
    /// The approval generation. Listed only by a service with the browser approval lifetime, and only
    /// to a Nexus that asks (`transport::Client::post` does).
    #[serde(default)]
    generation: Option<u64>,
    /// The end that use cannot move, ninety days after the shack approved it. None for an approval that
    /// never renews.
    #[serde(default)]
    renews_until: Option<u64>,
    /// The browser's device key (A5): its SPKI as lowercase hex. Listed only to a Nexus that asks
    /// (`transport::Client::post` does), and never shown: the desktop is given `key`.
    #[serde(default, skip_serializing)]
    public_key: Option<String>,
    /// SHA-256 of that key as lowercase hex: shown, shortened, beside the browser's name, and what
    /// approving it pins. Worked out here from the listed key, never taken from the service.
    #[serde(skip_deserializing)]
    key: Option<String>,
}
/// SHA-256 of a device key's SPKI, lowercase hex in and out: its pin (A5). `None` for anything that
/// is not the shape of a P-256 key.
fn fingerprint(public_key: &str) -> Option<String> {
    if !tempo_stream::protocol::device_key(public_key) {
        return None;
    }
    let spki = tempo_stream::protocol::hex_bytes(public_key)?;
    Some(
        ring::digest::digest(&ring::digest::SHA256, &spki)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
/// A pin as stored: 32 bytes of SHA-256 as lowercase hex.
fn pin_shape(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
impl Device {
    fn approval(&self) -> Approval {
        Approval {
            expires_at: self.expires_at,
            generation: self.generation,
        }
    }
}
/// One browser approval as the service lists it.
#[derive(Clone, Copy, PartialEq)]
struct Approval {
    expires_at: u64,
    generation: Option<u64>,
}
impl Approval {
    /// Was `grant` given against this approval? By generation when both sides know it: a renewal on
    /// use moves the expiry and keeps the generation, while approving again or revoking moves it.
    /// Otherwise by expiry, which every approval and revocation rewrites: a record 1.12.0 wrote, or a
    /// service without the approval lifetime, which never renews.
    fn holds(&self, grant: &vault::Grant) -> bool {
        match (grant.generation, self.generation) {
            (Some(held), Some(listed)) => held == listed,
            _ => grant.expires_at == self.expires_at,
        }
    }
    /// Rebind a grant this approval holds to the approval as listed now, so a record written before
    /// the service reported a generation carries one from here on. Grants nothing.
    fn bind(&self, grant: &mut vault::Grant) {
        if self.generation.is_some() {
            grant.generation = self.generation;
            grant.expires_at = self.expires_at;
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
// Empty struct variants preserve deny_unknown_fields; Serde unit variants ignore extra fields.
pub enum Action {
    LoggingPermission {
        #[serde(rename = "deviceId")]
        device_id: String,
        allow: bool,
    },
    StationPermission {
        #[serde(rename = "deviceId")]
        device_id: String,
        allow: bool,
    },
    TransmitPermission {
        #[serde(rename = "deviceId")]
        device_id: String,
        allow: bool,
    },
    TakeOverLogging {},
    Begin {
        name: String,
    },
    Refresh {},
    Cancel {},
    Approve {
        #[serde(rename = "enrollmentId")]
        enrollment_id: String,
        #[serde(rename = "accountId")]
        account_id: String,
        /// "Also allow FT8/FT4 transmit" for the browser that did the pairing.
        #[serde(default)]
        transmit: bool,
    },
    Enable {},
    Disable {},
    Forget {},
    Device {
        #[serde(rename = "deviceId")]
        device_id: String,
        approve: bool,
        /// "Also allow FT8/FT4 transmit" on this approval. Ignored when revoking.
        #[serde(default)]
        transmit: bool,
        /// A5: the device key the operator was shown beside this browser (its SHA-256, the device
        /// list's `key`). Pinned only if the service still lists that key once it has approved.
        #[serde(default)]
        key: Option<String>,
    },
    /// Remote over this network on, at the shack (`lan`): on `address`, this computer's private
    /// IPv4 address the operator picked, or with none as `lan` chooses; on `port`, or as it was.
    LanOn {
        #[serde(default)]
        address: Option<String>,
        #[serde(default)]
        port: Option<u16>,
    },
    /// Remote over this network off, at the shack: the port closes and every LAN session ends.
    LanOff {},
    /// Where Remote over this network listens, picked at the shack: one of this computer's private
    /// IPv4 addresses, or with none as `lan` chooses. Kept, on or off.
    LanAddress {
        #[serde(default)]
        address: Option<String>,
    },
    /// Pair a computer over this network, at the shack: the pairing window opens with a new code,
    /// and this press is the approval (as ruled on 2026-10-04, "One press").
    LanPair {},
    /// Close the pairing window early.
    LanCancel {},
    /// Remove a paired computer, at the shack: out at once, its station control and its session
    /// with it.
    LanRevoke {
        #[serde(rename = "deviceId")]
        device_id: String,
    },
    /// Reset this station's network identity, at the shack: a new LAN key, and every paired
    /// computer removed.
    LanReset {},
}
type Reply = oneshot::Sender<Result<(), &'static str>>;
/// The most browsers a restart remembers grants for. The service lists at most eight per station,
/// and the record has to fit the smallest OS credential blob (Windows).
const MAX_REMEMBERED: usize = 8;
/// A decision a restart must remember. Operator decisions 2026-09-13: Remote remembers being on;
/// approving a browser grants it station control and logging, plus FT8/FT4 transmit when the
/// approval ticks it, and each is kept for as long as that approval stands, until revoked.
///
/// Every decision is applied to `Control::remembered` under the `Control` lock at the moment it is
/// made, so the record always holds the operator's decisions in the order they were made, and Turn
/// on reads exactly what has been decided. Writing it happens on the vault writer's own thread,
/// never under a lock and never on the path that revokes transmission, because an OS credential
/// store can block (a locked keychain can prompt).
enum Persist {
    /// The controller loaded or created this pairing; later decisions apply to it.
    Bound(Binding, Option<vault::State>, Option<Box<vault::Pins>>),
    /// Remote was turned on and is connecting.
    Enabled,
    /// Remote went off for a reason that ends access: Revoke station access, a cancelled pairing,
    /// or the connection refusing itself. The permissions it cleared are remembered as cleared.
    Off,
    /// Turn off Remote. It only pauses (operator decision 2026-09-14): every browser is
    /// disconnected and every live permission cleared, but the remembered grants stay, so Turn on
    /// or a restart restores them for browsers still approved at the same approval.
    Paused,
    /// A local decision cleared every browser permission (take over, revoke a browser).
    ClearGrants,
    /// One browser's grants changed. `approval` is the approval the change was made against: a grant
    /// is only ever created against a known approval, and a grant against a different approval (not
    /// merely a renewed one) starts that browser afresh. A `None` field is left as it was.
    Browser {
        device_id: String,
        approval: Option<Approval>,
        logging: Option<bool>,
        control: Option<bool>,
        transmit: Option<bool>,
    },
    /// The approvals the service lists right now. A grant remembered against any other approval
    /// (a browser revoked, re-approved, expired or gone) is forgotten; the rest are rebound to the
    /// approval as listed. So is a pin for a browser no longer approved.
    Approvals(Vec<(String, Approval)>),
    /// The pairing was removed.
    Removed,
    /// A5: the operator approved this browser at the radio with its device key shown (`Some`: the
    /// key's SHA-256, now pinned), or the approval pinned nothing, or the browser was revoked
    /// (`None`: no pin).
    Pin {
        device_id: String,
        key: Option<String>,
    },
    /// The browsers the service lists as approved right now: a pin for any other is forgotten.
    Listed(Vec<String>),
}
/// What the vault must hold next. Each write is a whole record, so only the latest of each matters.
enum Write {
    Save(vault::State),
    Remove,
    /// The pins' own entry (A5).
    SavePins(vault::Pins),
    RemovePins,
}
/// The record for the current pairing, kept in step with every decision.
#[derive(Default)]
struct Remembered {
    binding: Option<Binding>,
    state: Option<vault::State>,
    /// A5: browser id to the SHA-256 of the device key the operator pinned for it, kept in the
    /// vault's pins entry. A pin is an approval's, not a permission's: the switches, take over and
    /// Turn off leave it; approving at the radio sets it and revoking the browser removes it.
    pins: BTreeMap<String, String>,
}
impl Remembered {
    /// Apply one decision. Returns the writes it calls for: at most one for each entry, and none
    /// when nothing changed.
    fn apply(&mut self, decision: Persist) -> Vec<Write> {
        let pins = self.apply_pins(&decision);
        self.apply_state(decision).into_iter().chain(pins).collect()
    }
    fn apply_pins(&mut self, decision: &Persist) -> Option<Write> {
        let before = self.pins.clone();
        match decision {
            Persist::Bound(bound, _, pins) => {
                self.pins = pins
                    .as_ref()
                    .filter(|p| p.binding == *bound)
                    .map(|p| p.keys.clone())
                    .unwrap_or_default();
                return None;
            }
            Persist::Removed => {
                self.pins.clear();
                return Some(Write::RemovePins);
            }
            Persist::Off => self.pins.clear(),
            Persist::Pin { device_id, key } => match key {
                Some(key) => {
                    self.pins.insert(device_id.clone(), key.clone());
                }
                None => {
                    self.pins.remove(device_id);
                }
            },
            Persist::Listed(approved) => self.pins.retain(|id, _| approved.contains(id)),
            Persist::Approvals(approvals) => self
                .pins
                .retain(|id, _| approvals.iter().any(|(listed, _)| listed == id)),
            _ => return None,
        }
        if self.pins == before {
            return None;
        }
        let binding = self.binding.clone()?;
        Some(if self.pins.is_empty() {
            Write::RemovePins
        } else {
            Write::SavePins(vault::Pins {
                binding,
                keys: self.pins.clone(),
            })
        })
    }
    fn apply_state(&mut self, decision: Persist) -> Option<Write> {
        match decision {
            Persist::Bound(bound, state, _) => {
                self.state = state.filter(|s| s.binding == bound);
                self.binding = Some(bound);
                return None;
            }
            Persist::Removed => {
                self.binding = None;
                self.state = None;
                return Some(Write::Remove);
            }
            Persist::Pin { .. } | Persist::Listed(_) => return None,
            _ => {}
        }
        let bound = self.binding.clone()?;
        let mut state = self.state.clone().unwrap_or(vault::State {
            binding: bound,
            enabled: false,
            grants: Vec::new(),
        });
        match decision {
            Persist::Enabled => state.enabled = true,
            Persist::Off => {
                state.enabled = false;
                state.grants.clear();
            }
            Persist::Paused => state.enabled = false,
            Persist::ClearGrants => state.grants.clear(),
            Persist::Browser {
                device_id,
                approval,
                logging,
                control,
                transmit,
            } => {
                let at = state.grants.iter().position(|g| g.device_id == device_id);
                let mut grant = match (at.map(|i| state.grants[i].clone()), approval) {
                    (Some(mut grant), Some(approval)) if approval.holds(&grant) => {
                        approval.bind(&mut grant);
                        grant
                    }
                    (_, Some(approval)) => vault::Grant {
                        device_id,
                        expires_at: approval.expires_at,
                        generation: approval.generation,
                        logging: false,
                        control: false,
                        transmit: false,
                    },
                    (Some(grant), None) => grant,
                    // Nothing remembered for this browser, and no approval to bind a grant to.
                    (None, None) => return None,
                };
                grant.logging = logging.unwrap_or(grant.logging);
                grant.control = control.unwrap_or(grant.control);
                // FT8/FT4 transmit is only ever held alongside station control.
                grant.transmit = transmit.unwrap_or(grant.transmit) && grant.control;
                let keep = grant.logging || grant.control;
                match at {
                    Some(i) if keep => state.grants[i] = grant,
                    Some(i) => {
                        state.grants.remove(i);
                    }
                    None if keep && state.grants.len() < MAX_REMEMBERED => state.grants.push(grant),
                    None => {}
                }
            }
            Persist::Approvals(approvals) => state.grants.retain_mut(|g| {
                let listed = approvals
                    .iter()
                    .find(|(id, approval)| *id == g.device_id && approval.holds(g));
                if let Some((_, approval)) = listed {
                    approval.bind(g);
                }
                listed.is_some()
            }),
            Persist::Bound(..) | Persist::Removed | Persist::Pin { .. } | Persist::Listed(_) => {
                return None
            }
        }
        if self.state.as_ref() == Some(&state) {
            return None;
        }
        self.state = Some(state.clone());
        Some(Write::Save(state))
    }
    fn grants(&self) -> &[vault::Grant] {
        self.state.as_ref().map_or(&[], |s| s.grants.as_slice())
    }
    /// The device key pinned for this browser (A5), as the SHA-256 a stream offer's key must have.
    fn pinned(&self, device_id: &str) -> Option<[u8; 32]> {
        tempo_stream::protocol::hex_bytes(self.pins.get(device_id)?)?
            .try_into()
            .ok()
    }
}
#[derive(Default)]
struct Control {
    operations: Arc<operations::Authority>,
    memories: query::memories::Bank,
    enabled: bool,
    cancel: Option<watch::Sender<bool>>,
    generation: u64,
    vault_writer: Option<mpsc::UnboundedSender<Write>>,
    remembered: Remembered,
    /// This pairing's signing key (S3-M1), read from the vault or made there once. `None` while there
    /// is no pairing, or when the credential store would not give one up: the station's answers then
    /// go unsigned, and pages refuse them.
    station_key: Option<Arc<station_key::Signer>>,
    /// The service holds another key for this station (`stationKeyPinned`): pages will refuse its
    /// answers until it is paired again. Said at the shack.
    key_refused: bool,
    /// Remote over this network (`lan`), beside the relay's road and sharing its authority.
    /// Taken out of the lock to be used. `None` in a test build of the service.
    lan: Option<Arc<lan::Lan>>,
}
impl Control {
    fn stop(&mut self) {
        self.operations.invalidate();
        self.enabled = false;
        if let Ok(mut bank) = self.memories.lock() {
            *bank = None;
        }
        self.generation = self.generation.wrapping_add(1);
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(true);
        }
    }
    /// Hand a decision to the vault writer. Never called from `stop`: `stop` also runs when Nexus
    /// exits, and an exit must not be remembered as the operator turning Remote off.
    fn remember(&mut self, decision: Persist) {
        for write in self.remembered.apply(decision) {
            if let Some(writer) = &self.vault_writer {
                let _ = writer.send(write);
            }
        }
    }
}
/// A command, the Remote generation it was issued under, the authority epoch at that moment (so a
/// local decision made after it wins), and where to reply.
type Command = (Action, u64, u64, Reply);
pub struct Service {
    commands: mpsc::Sender<Command>,
    status: Arc<Mutex<Status>>,
    control: Arc<Mutex<Control>>,
}
// A superseded connection may finish an I/O operation after a newer local
// decision. Its status update must not undo that decision in the desktop UI.
#[derive(Clone)]
struct SessionStatus {
    status: Arc<Mutex<Status>>,
    control: Arc<Mutex<Control>>,
    generation: u64,
}
impl SessionStatus {
    fn set(&self, value: &str, error: Option<&'static str>) {
        if let Ok(mut control) = self.control.lock() {
            if !control.enabled || control.generation != self.generation {
                return;
            }
            if value == "disabled" {
                control.stop();
                // The service refused this station (access denied) or its credential is unusable.
                // Remote is off now, so the next launch must not turn it back on. A malformed
                // service reply is deliberately NOT this: it reconnects (see `supervise`), because
                // an off that is remembered can only be undone at the shack.
                control.remember(Persist::Off);
            }
            phase(&self.status, value, error);
        }
    }
}
pub(super) fn phase(status: &Arc<Mutex<Status>>, value: &str, error: Option<&'static str>) {
    if let Ok(mut status) = status.lock() {
        status.phase = value.into();
        status.error = error;
    }
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 48 && !name.chars().any(char::is_control)
}
impl Service {
    pub fn new(
        engine: crate::SharedEngine,
        publisher: crate::remote_monitor::Publisher,
        spectrum: tempo_app::engine::SpectrumFeed,
        meters: tempo_app::engine::MeterFeed,
        sources: Option<query::Sources>,
        // `audio`: the station's bounded receive-audio copy, for a listening browser.
        // `None` leaves the audio lane unadvertised, so nothing is ever offered it.
        #[cfg(feature = "radio")] audio: Option<Arc<tempo_audio::receive_audio::ReceiveAudioFeed>>,
        // What a streamed session needs from the application: the window its input goes to.
        stream: stream::Host,
    ) -> Self {
        tempo_app::engine::engine_lock(&engine)
            .configure_remote_settings_store(crate::settings_path());
        let feeds = transport::Feeds {
            monitor: publisher,
            spectrum: Some(spectrum),
            meters,
            sources,
            // One encoder for the station, whoever listens: the Listen lane and every stream.
            #[cfg(feature = "radio")]
            audio: audio.map(audio::ReceiveFanout::new),
            stream,
        };
        let service = Self::start(
            REMOTE_ORIGIN.to_string(),
            Box::new(SystemVault),
            engine.clone(),
            feeds.clone(),
        );
        // Its LAN key and paired computers are in the OS credential store, read on its own thread.
        service.with_lan(
            crate::settings_path().with_file_name("remote-lan.json"),
            engine,
            feeds,
            Arc::new(lan::Book::new(Arc::new(SystemVault))),
            Arc::new(tempo_stream::lan::look),
        )
    }
    /// Remote over this network, beside the relay's road and sharing its one authority: its switch
    /// kept at `path`, its key and paired computers in `book`.
    fn with_lan(
        self,
        path: std::path::PathBuf,
        engine: crate::SharedEngine,
        feeds: transport::Feeds,
        book: Arc<lan::Book>,
        resolve: lan::Resolve,
    ) -> Self {
        let authority = self.control.lock().ok().map(|c| c.operations.clone());
        if let Some(authority) = authority {
            let lan = Arc::new(lan::Lan::start(
                path,
                lan::Deps {
                    authority,
                    engine,
                    feeds,
                    book,
                    resolve,
                    advertise: lan::advertise(),
                    firewall: lan::firewall_says(),
                },
            ));
            if let Ok(mut control) = self.control.lock() {
                control.lan = Some(lan);
            }
        }
        self
    }
    #[cfg(test)]
    fn configured(
        origin: String,
        vault: Box<dyn Vault>,
        engine: crate::SharedEngine,
        publisher: crate::remote_monitor::Publisher,
    ) -> Self {
        Self::start(
            origin,
            vault,
            engine,
            transport::Feeds {
                monitor: publisher,
                spectrum: None,
                meters: Default::default(),
                sources: None,
                #[cfg(feature = "radio")]
                audio: None,
                stream: stream::Host::default(),
            },
        )
    }
    /// `feeds` arrives whole rather than as four parameters. They were four until the audio
    /// lane made it five, and a five-feed argument list is the shape that gets called with two
    /// of them transposed — they already travel together into the controller.
    fn start(
        origin: String,
        vault: Box<dyn Vault>,
        engine: crate::SharedEngine,
        feeds: transport::Feeds,
    ) -> Self {
        let vault: Arc<dyn Vault> = Arc::from(vault);
        let status = Arc::new(Mutex::new(Status {
            phase: "unpaired".into(),
            origin: origin.clone(),
            ..Default::default()
        }));
        // The vault writer runs on a thread of its own, so remembering a decision never waits on
        // the OS credential store while a lock is held or while the controller is mid-request. If
        // it cannot start, nothing is remembered, and the next launch leaves Remote off.
        let (writer, decisions) = mpsc::unbounded_channel();
        let writer_vault = vault.clone();
        let vault_writer = std::thread::Builder::new()
            .name("nexus-remote-vault".into())
            .spawn(move || write_remembered(writer_vault, decisions))
            .ok()
            .map(|_| writer);
        let operations = Arc::new(operations::Authority::with_spots(
            feeds.sources.as_ref().map(|s| s.spots.clone()),
        ));
        // A stand-down at the shack retires remote transmit authority. Handing the engine this
        // revocation is what makes `Engine::halt_tx` — the verb every local stop funnels through,
        // including WSJT-X's UDP HaltTx — move the generation a browser carries as its
        // `transmitEpoch`, so a gesture held across the operator's own Stop TX is refused instead
        // of arming the rig a second later. See `Engine::halt_tx`.
        tempo_app::engine::engine_lock(&engine)
            .set_remote_transmit_revocation(operations.transmit_revocation());
        // A streamed operator's held PTT: the stream records the holds, the engine arms and
        // releases the microphone over on its own radio-loop tick (`engine/remote_stream.rs`).
        tempo_app::engine::engine_lock(&engine).set_remote_ptt_hold(feeds.stream.ptt.clone());
        // …and the page's microphone, which the stream decodes and the engine's microphone over
        // plays, only while that press has armed it (`engine/remote_mic.rs`).
        tempo_app::engine::engine_lock(&engine).set_remote_mic_feed(feeds.stream.mic.clone());
        let control = Arc::new(Mutex::new(Control {
            operations,
            memories: feeds
                .sources
                .as_ref()
                .map(|s| s.memories.clone())
                .unwrap_or_default(),
            vault_writer,
            ..Default::default()
        }));
        let (commands, receiver) = mpsc::channel(1);
        let task_status = status.clone();
        let task_control = control.clone();
        let thread = std::thread::Builder::new()
            .name("nexus-remote".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let client = Client::new(&origin);
                match (runtime, client) {
                    (Ok(runtime), Ok(client)) => runtime.block_on(
                        Controller {
                            client,
                            vault,
                            binding: None,
                            pending: None,
                            status: task_status,
                            control: task_control,
                            engine,
                            feeds,
                            restore: None,
                        }
                        .run(receiver),
                    ),
                    _ => phase(&task_status, "unpaired", Some("serviceUnavailable")),
                }
            });
        if thread.is_err() {
            phase(&status, "unpaired", Some("serviceUnavailable"));
        }
        Self {
            commands,
            status,
            control,
        }
    }
    pub fn status(&self) -> Result<Status, &'static str> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| "serviceUnavailable")?
            .clone();
        let control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        let enabled = control.enabled;
        let operations = control.operations.local_status();
        status.logging_permissions =
            serde_json::from_value(operations["devices"].clone()).unwrap_or_default();
        status.logging_controller = operations["controller"].as_str().map(str::to_string);
        status.station_permissions =
            serde_json::from_value(operations["controlDevices"].clone()).unwrap_or_default();
        status.transmit_permissions =
            serde_json::from_value(operations["transmitDevices"].clone()).unwrap_or_default();
        status.pinned_devices = status
            .devices
            .iter()
            .filter(|d| d.key.is_some() && control.remembered.pins.get(&d.id) == d.key.as_ref())
            .map(|d| d.id.clone())
            .collect();
        status.observation_generation = enabled.then(|| control.generation.to_string());
        status.key_refused = control.key_refused;
        status.lan = control.lan.as_deref().map(lan::Lan::status);
        if !enabled && ["connected", "connecting", "reconnecting"].contains(&status.phase.as_str())
        {
            status.phase = "disabled".into();
        }
        Ok(status)
    }
    /// Remote over this network, out of the lock.
    fn lan(&self) -> Result<Option<Arc<lan::Lan>>, &'static str> {
        Ok(self
            .control
            .lock()
            .map_err(|_| "serviceUnavailable")?
            .lan
            .clone())
    }
    fn publish_memories(&self, generation: &str, bank: Option<&str>) -> bool {
        // Parse outside the control lock. Recheck the local enable generation at
        // admission so an old WebView reply cannot revive a disabled snapshot.
        let value = bank.and_then(query::memories::parse);
        let Ok(control) = self.control.try_lock() else {
            return false;
        };
        if !control.enabled || control.generation.to_string() != generation {
            return false;
        }
        let Ok(mut cache) = control.memories.try_lock() else {
            return false;
        };
        *cache = value.map(|v| (std::time::Instant::now(), Arc::new(v)));
        cache.is_some()
    }
    pub async fn action(&self, action: Action) -> Result<Status, &'static str> {
        match &action {
            Action::LoggingPermission { device_id, allow }
            | Action::StationPermission { device_id, allow }
            | Action::TransmitPermission { device_id, allow } => {
                // Keep admission and grant under the same local control lock
                // as Disable/Take over. A prior status snapshot must not install
                // a new grant after that local authority has been stopped.
                let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
                // The approval being granted against. A restart checks it (by generation where the
                // service reports one) before giving the grant back, so a revoked or re-approved
                // browser gets nothing, and a renewed one keeps what it had.
                let mut approval = None;
                if *allow {
                    let status = self.status.lock().map_err(|_| "serviceUnavailable")?;
                    let Some(device) = status
                        .devices
                        .iter()
                        .find(|d| d.id == *device_id && d.approved == 1)
                        .filter(|_| control.enabled)
                    else {
                        return Err("accessDenied");
                    };
                    approval = Some(device.approval());
                }
                // These are the restrict switches: each allows or revokes one permission for one
                // browser, live first, and the record follows only what took effect. Revoking
                // station control revokes transmit with it.
                let operations = control.operations.clone();
                let (logging, station, transmit) = match &action {
                    Action::StationPermission { .. } => {
                        operations.permit_station(device_id, *allow)?;
                        (None, Some(*allow), (!*allow).then_some(false))
                    }
                    Action::TransmitPermission { .. } => {
                        operations.permit_transmit(device_id, *allow)?;
                        (None, None, Some(*allow))
                    }
                    _ => {
                        operations.permit(device_id, *allow)?;
                        (Some(*allow), None, None)
                    }
                };
                control.remember(Persist::Browser {
                    device_id: device_id.clone(),
                    approval,
                    logging,
                    control: station,
                    transmit,
                });
                drop(control);
                return self.status();
            }
            Action::TakeOverLogging {} => {
                let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
                control.operations.invalidate();
                control.remember(Persist::ClearGrants);
                let lan = control.lan.clone();
                drop(control);
                // "End remote control" ends Remote over this network too: its port closes and every
                // LAN session ends, and the shack says why until it is turned on again.
                if let Some(lan) = lan {
                    lan.end_at_shack();
                }
                return self.status();
            }
            Action::LanOn { address, port } => {
                let lan = self.lan()?.ok_or("serviceUnavailable")?;
                lan.turn_on(address.as_deref(), *port)?;
                return self.status();
            }
            Action::LanOff {} => {
                if let Some(lan) = self.lan()? {
                    lan.turn_off();
                }
                return self.status();
            }
            Action::LanAddress { address } => {
                self.lan()?
                    .ok_or("serviceUnavailable")?
                    .pick(address.as_deref())?;
                return self.status();
            }
            Action::LanPair {} => {
                self.lan()?.ok_or("serviceUnavailable")?.pair()?;
                return self.status();
            }
            Action::LanCancel {} => {
                if let Some(lan) = self.lan()? {
                    lan.cancel_pairing();
                }
                return self.status();
            }
            Action::LanRevoke { device_id } => {
                if !identifier(device_id) {
                    return Err("invalidRequest");
                }
                let (lan, device) = (self.lan()?.ok_or("serviceUnavailable")?, device_id.clone());
                // Off this thread, since the credential store can block. Inside, the computer is
                // out and its control revoked before the store is written.
                tokio::task::spawn_blocking(move || lan.revoke(&device))
                    .await
                    .map_err(|_| "serviceUnavailable")??;
                return self.status();
            }
            Action::LanReset {} => {
                let lan = self.lan()?.ok_or("serviceUnavailable")?;
                tokio::task::spawn_blocking(move || lan.reset())
                    .await
                    .map_err(|_| "serviceUnavailable")??;
                return self.status();
            }
            _ => {}
        }
        let (reply, response) = oneshot::channel();
        {
            let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
            if let Action::Device {
                approve: false,
                device_id,
                ..
            } = &action
            {
                control.operations.invalidate();
                control.remember(Persist::ClearGrants);
                // A5: revoking ends the approval, and its pin with it.
                control.remember(Persist::Pin {
                    device_id: device_id.clone(),
                    key: None,
                });
            }
            if matches!(
                action,
                Action::Disable {} | Action::Forget {} | Action::Cancel {}
            ) {
                control.stop();
                // Remembered here rather than in the controller: this stop is immediate even when
                // the command queue is full, and what a restart remembers must match it. The stop
                // clears every live permission either way; only Turn off keeps the remembered ones.
                control.remember(if matches!(action, Action::Disable {}) {
                    Persist::Paused
                } else {
                    Persist::Off
                });
            }
            // A refused Enable must not leave enabled authority behind. Stop
            // actions remain immediate even when the command queue is full.
            let permit = self.commands.try_reserve().map_err(|_| "remoteBusy")?;
            if matches!(action, Action::Enable {}) {
                control.stop();
                control.enabled = true;
                // Turning on starts a fresh authority: every permission in memory is cleared. The
                // grants remembered for still-approved browsers come back once the service lists
                // them (see `Controller::apply_restore`). "On" itself is remembered by the
                // controller, once the connection has actually started.
            }
            permit.send((
                action,
                control.generation,
                control.operations.epoch(),
                reply,
            ));
        }
        response.await.map_err(|_| "serviceUnavailable")??;
        self.status()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        if let Ok(mut control) = self.control.lock() {
            control.stop();
        }
    }
}

struct Pending {
    id: String,
    proof: String,
    code: String,
    expires_at: u64,
    account_id: Option<String>,
    station_credential: Option<String>,
}
struct Controller {
    client: Client,
    vault: Arc<dyn Vault>,
    binding: Option<Binding>,
    pending: Option<Pending>,
    status: Arc<Mutex<Status>>,
    control: Arc<Mutex<Control>>,
    engine: crate::SharedEngine,
    feeds: transport::Feeds,
    restore: Option<Restore>,
}
/// Remote came on with grants remembered; they wait for the service's browser list.
struct Restore {
    /// The authority epoch when Remote came on. Any local decision since then (Turn off, take over,
    /// revoking a browser) moves it, and that decision wins.
    epoch: u64,
    at: tokio::time::Instant,
    attempts: u32,
}
impl Controller {
    async fn run(mut self, mut receiver: mpsc::Receiver<Command>) {
        match self.vault.binding() {
            Ok(Some(binding))
                if binding.origin == self.client.origin()
                    && identifier(&binding.station_id)
                    && identifier(&binding.account_id) =>
            {
                self.binding = Some(binding.clone());
                self.resume(binding).await;
            }
            Ok(None) => {}
            _ => phase(&self.status, "unpaired", Some("credentialStoreUnavailable")),
        }
        loop {
            let retry = self.restore.as_ref().map(|r| r.at);
            tokio::select! {
                next = receiver.recv() => {
                    let Some((action, generation, epoch, reply)) = next else { break };
                    let result = self.handle(action, generation, epoch).await;
                    if let Err(error) = result {
                        if let Ok(mut status) = self.status.lock() {
                            status.error = Some(error);
                        }
                    }
                    let _ = reply.send(result);
                }
                _ = async {
                    match retry {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => self.retry_restore().await,
            }
        }
        if let Ok(mut control) = self.control.lock() {
            control.stop();
        }
    }
    /// Remote remembers being on. If it was on when Nexus last exited, turn it on again exactly as
    /// Turn on Remote does: a fresh local authority (every in-memory permission cleared, transmit
    /// included), a fresh generation, and the remembered grants restored for still-approved
    /// browsers. A locked or unreadable store, a record written for another pairing, or a record
    /// that says off all leave it off.
    async fn resume(&mut self, binding: Binding) {
        let remembered = match self.vault.state() {
            Ok(state) => state.filter(|s| {
                s.binding == binding
                    && s.grants.len() <= MAX_REMEMBERED
                    && s.grants.iter().all(|g| identifier(&g.device_id))
            }),
            Err(error) => {
                if let Ok(mut control) = self.control.lock() {
                    control.remember(Persist::Bound(binding, None, None));
                }
                self.reflect("disabled");
                phase(&self.status, "disabled", Some(error));
                return;
            }
        };
        // A5: the pins, from their own entry. One that is locked, unreadable or out of shape pins
        // nothing: every browser is then approved again at the radio before it streams.
        let pins = self.vault.pins().ok().flatten().filter(|p| {
            p.binding == binding
                && p.keys.len() <= MAX_REMEMBERED
                && p.keys
                    .iter()
                    .all(|(id, key)| identifier(id) && pin_shape(key))
        });
        // S3-M1: this pairing's signing key, made here once for a pairing from before it existed.
        let key = self.station_key(&binding);
        // Decide under the lock BEFORE the pairing becomes visible, so a Turn off that arrives
        // afterwards is ordered after this decision and wins.
        let resumed = {
            let Ok(mut control) = self.control.lock() else {
                return;
            };
            let enabled = remembered.as_ref().is_some_and(|s| s.enabled);
            control.station_key = key;
            control.key_refused = false;
            control.remember(Persist::Bound(binding, remembered, pins.map(Box::new)));
            if enabled {
                control.stop();
                control.enabled = true;
                Some(control.generation)
            } else {
                None
            }
        };
        self.reflect("disabled");
        let Some(generation) = resumed else {
            return;
        };
        if let Err(error) = self.enable(generation) {
            if let Ok(mut status) = self.status.lock() {
                status.error = Some(error);
            }
        }
    }
    /// Nobody opens Settings after an unattended restart, so the station fetches its own browser
    /// list to restore grants, backing off while the service is unreachable.
    async fn retry_restore(&mut self) {
        let current = self.restore.as_ref().is_some_and(|restore| {
            self.control
                .lock()
                .is_ok_and(|c| c.enabled && c.operations.epoch() == restore.epoch)
        });
        if !current {
            self.restore = None;
            return;
        }
        match self.devices().await {
            Ok(devices) => {
                self.apply_restore(&devices);
                if let Ok(mut status) = self.status.lock() {
                    status.devices = devices;
                }
            }
            Err(_) => {
                if let Some(restore) = &mut self.restore {
                    restore.attempts = restore.attempts.saturating_add(1).min(5);
                    restore.at = tokio::time::Instant::now()
                        + std::time::Duration::from_secs(1 << restore.attempts);
                }
            }
        }
    }
    /// Put back remembered grants for each browser the service still lists as approved at the SAME
    /// approval (`Approval::holds`: the same generation, so a renewal on use still matches), under the
    /// epoch captured when Remote came on. The grants are read from the record now, under the lock, so
    /// a revocation made while the list was on its way is honoured. FT8/FT4 transmit comes back only
    /// where it was granted, only with station control, and arms nothing.
    fn apply_restore(&mut self, devices: &[Device]) {
        let Some(restore) = self.restore.take() else {
            return;
        };
        let now = now_ms();
        let approvals: Vec<(String, Approval)> = devices
            .iter()
            .filter(|d| d.approved == 1 && d.expires_at > now)
            .map(|d| (d.id.clone(), d.approval()))
            .collect();
        let Ok(mut control) = self.control.lock() else {
            return;
        };
        if !control.enabled || control.operations.epoch() != restore.epoch {
            return;
        }
        let kept: Vec<vault::Grant> = control
            .remembered
            .grants()
            .iter()
            .filter(|g| {
                approvals
                    .iter()
                    .any(|(id, approval)| *id == g.device_id && approval.holds(g))
            })
            .cloned()
            .collect();
        let ids = |held: fn(&vault::Grant) -> bool| {
            kept.iter()
                .filter(|g| held(g))
                .map(|g| g.device_id.clone())
                .collect()
        };
        let grants = operations::DurableGrants {
            logging: ids(|g| g.logging),
            control: ids(|g| g.control),
            transmit: ids(|g| g.control && g.transmit),
        };
        let operations = control.operations.clone();
        match operations.restore(restore.epoch, &grants) {
            // Browsers no longer approved at the same approval are forgotten.
            Ok(true) => control.remember(Persist::Approvals(approvals)),
            Ok(false) => {}
            Err(_) => {
                drop(control);
                self.restore = Some(Restore {
                    at: tokio::time::Instant::now() + std::time::Duration::from_secs(1),
                    ..restore
                });
            }
        }
    }
    async fn devices(&self) -> Result<Vec<Device>, &'static str> {
        let (binding, token) = self.bound()?;
        let value = self
            .client
            .post(&station_path(&binding, "devices"), Some(&token), empty())
            .await?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Devices {
            devices: Vec<Device>,
        }
        let mut value: Devices = serde_json::from_value(value).map_err(|_| "invalidResponse")?;
        if value.devices.len() > 8
            || value
                .devices
                .iter()
                .any(|d| !identifier(&d.id) || !valid_name(&d.name) || d.approved > 1)
        {
            return Err("invalidResponse");
        }
        // A5: a key that is not the shape of a P-256 key is no key. It shows nothing and pins
        // nothing, and the rest of the list (approvals, grants) stands.
        for device in &mut value.devices {
            device.key = device.public_key.as_deref().and_then(fingerprint);
        }
        Ok(value.devices)
    }
    /// This pairing's signing key (S3-M1), made once: the one the vault holds for it, or a new one
    /// saved there first. A store that will not answer (locked) makes nothing, because the key may
    /// be in it: the station's answers go unsigned until it does.
    fn station_key(&self, binding: &Binding) -> Option<Arc<station_key::Signer>> {
        match self.vault.station_key() {
            Ok(Some(kept)) if kept.binding == *binding => {
                if let Some(signer) = station_key::Signer::restore(&kept.pkcs8) {
                    return Some(Arc::new(signer));
                }
            }
            Ok(_) => {}
            Err(_) => return None,
        }
        self.new_station_key(binding)
    }
    /// A new signing key for `binding`, saved before it is used: a key the vault did not keep would
    /// be gone at the next start, and the service would refuse the one made then.
    fn new_station_key(&self, binding: &Binding) -> Option<Arc<station_key::Signer>> {
        let (signer, pkcs8) = station_key::Signer::generate()?;
        self.vault
            .save_station_key(&vault::StationKey {
                binding: binding.clone(),
                pkcs8,
            })
            .ok()?;
        Some(Arc::new(signer))
    }
    fn reflect(&self, value: &str) {
        if let Ok(mut status) = self.status.lock() {
            status.phase = value.into();
            status.error = None;
            status.station_id = self.binding.as_ref().map(|b| b.station_id.clone());
            status.account_id = self
                .binding
                .as_ref()
                .map(|b| b.account_id.clone())
                .or_else(|| self.pending.as_ref().and_then(|p| p.account_id.clone()));
            status.pairing_id = self.pending.as_ref().map(|p| p.id.clone());
            status.pairing_code = self.pending.as_ref().map(|p| p.code.clone());
            status.expires_at = self.pending.as_ref().map(|p| p.expires_at);
            if self.binding.is_none() {
                status.devices.clear();
            }
        }
    }
    fn bound(&self) -> Result<(Binding, String), &'static str> {
        let binding = self.binding.as_ref().ok_or("stationNotPaired")?.clone();
        let token = self.vault.credential(&binding)?;
        if !credential(&token) {
            return Err("credentialStoreUnavailable");
        }
        Ok((binding, token))
    }
    /// ONE APPROVAL (operator decision 2026-09-13): approving a browser grants it station control
    /// and logging, and FT8/FT4 transmit when the approval ticked it. With Remote on they take
    /// effect at once; with Remote off they wait in the record for Turn on. A local decision made
    /// after the approval was asked for (Turn off or on, take over, revoking a browser) wins, and
    /// nothing is granted. ⛔ Granting transmit arms nothing: the browser still has to press TX On.
    ///
    /// A5: the device key the operator was shown (`shown`) is pinned if the service still lists that
    /// key for the browser now that it has approved it. A key that changed in between pins nothing,
    /// and that browser is approved again before it streams.
    fn grant_approved(
        &self,
        devices: &[Device],
        device_id: String,
        transmit: bool,
        shown: Option<String>,
        (generation, epoch): (u64, u64),
    ) -> Result<(), &'static str> {
        let now = now_ms();
        let device = devices
            .iter()
            .find(|d| d.id == device_id && d.approved == 1 && d.expires_at > now)
            .ok_or("invalidResponse")?;
        let approval = device.approval();
        let pin = shown.filter(|shown| device.key.as_ref() == Some(shown));
        let approved: Vec<String> = devices
            .iter()
            .filter(|d| d.approved == 1 && d.expires_at > now)
            .map(|d| d.id.clone())
            .collect();
        let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        if control.generation != generation || control.operations.epoch() != epoch {
            return Ok(());
        }
        if control.enabled {
            let operations = control.operations.clone();
            operations.permit(&device_id, true)?;
            operations.permit_station(&device_id, true)?;
            // A fresh approval without the tick holds no transmit, even if an earlier one did.
            operations.permit_transmit(&device_id, transmit)?;
        }
        control.remember(Persist::Browser {
            device_id: device_id.clone(),
            approval: Some(approval),
            logging: Some(true),
            control: Some(true),
            transmit: Some(transmit),
        });
        control.remember(Persist::Listed(approved));
        control.remember(Persist::Pin {
            device_id,
            key: pin,
        });
        Ok(())
    }
    /// Connect under `generation`, which the caller has already made enabled with a fresh
    /// authority (Turn on Remote, a launch that remembers being on, or approving the pairing).
    fn enable(&mut self, generation: u64) -> Result<(), &'static str> {
        let bound = self.bound();
        let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        if !control.enabled || control.generation != generation {
            return Ok(());
        }
        let (binding, token) = match bound {
            Ok(value) => value,
            Err(error) => {
                control.stop();
                return Err(error);
            }
        };
        if let Some(cancel) = control.cancel.take() {
            let _ = cancel.send(true);
        }
        let (cancel, receiver) = watch::channel(false);
        control.cancel = Some(cancel);
        phase(&self.status, "connecting", None);
        tokio::spawn(transport::supervise(
            self.client.clone(),
            binding,
            token,
            receiver,
            self.engine.clone(),
            self.feeds.clone(),
            SessionStatus {
                status: self.status.clone(),
                control: self.control.clone(),
                generation,
            },
        ));
        // Remote is on and connecting: the next launch turns it back on.
        control.remember(Persist::Enabled);
        // Remembered grants come back once the service confirms each browser is still approved at
        // the same approval. The epoch is read here, under the lock: a later take over, revocation
        // or Turn off moves it, and that decision wins.
        if !control.remembered.grants().is_empty() {
            self.restore = Some(Restore {
                epoch: control.operations.epoch(),
                at: tokio::time::Instant::now(),
                attempts: 0,
            });
        }
        Ok(())
    }
    async fn handle(
        &mut self,
        action: Action,
        generation: u64,
        epoch: u64,
    ) -> Result<(), &'static str> {
        match action {
            Action::LoggingPermission { .. }
            | Action::StationPermission { .. }
            | Action::TransmitPermission { .. }
            | Action::TakeOverLogging {}
            | Action::LanOn { .. }
            | Action::LanOff {}
            | Action::LanAddress { .. }
            | Action::LanPair {}
            | Action::LanCancel {}
            | Action::LanRevoke { .. }
            | Action::LanReset {} => return Err("invalidRequest"),
            Action::Begin { name } => {
                if self.binding.is_some() || self.pending.is_some() || !valid_name(&name) {
                    return Err("invalidRequest");
                }
                let value = self
                    .client
                    .post("enroll", None, json!({ "name": name.trim() }))
                    .await?;
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Enrollment {
                    id: String,
                    proof: String,
                    code: String,
                    expires_at: u64,
                }
                let e: Enrollment = serde_json::from_value(value).map_err(|_| "invalidResponse")?;
                if !identifier(&e.id)
                    || !credential(&e.proof)
                    || e.code.len() != 16
                    || !e.code.bytes().all(|b| b.is_ascii_hexdigit())
                    || e.expires_at <= now_ms()
                    || e.expires_at > now_ms() + 660000
                {
                    return Err("invalidResponse");
                }
                self.pending = Some(Pending {
                    id: e.id,
                    proof: e.proof,
                    code: e.code,
                    expires_at: e.expires_at,
                    account_id: None,
                    station_credential: None,
                });
                self.reflect("pairing");
            }
            Action::Cancel {} => {
                self.pending = None;
                self.reflect(if self.binding.is_some() {
                    "disabled"
                } else {
                    "unpaired"
                });
            }
            Action::Refresh {} => {
                if let Some(pending) = &mut self.pending {
                    if pending.expires_at <= now_ms() {
                        self.pending = None;
                        self.reflect("unpaired");
                        return Err("pairingExpired");
                    }
                    let value = self
                        .client
                        .post(
                            "enroll/check",
                            None,
                            json!({ "id": pending.id, "proof": pending.proof }),
                        )
                        .await?;
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase", deny_unknown_fields)]
                    struct Check {
                        account_id: Option<String>,
                        approved: bool,
                    }
                    let check: Check =
                        serde_json::from_value(value).map_err(|_| "invalidResponse")?;
                    if check
                        .account_id
                        .as_deref()
                        .is_some_and(|value| !identifier(value))
                    {
                        return Err("invalidResponse");
                    }
                    // A successful approve whose response was lost may be retried
                    // with the same in-memory credential; never issue a new one.
                    if check.approved && pending.station_credential.is_none() {
                        return Err("invalidResponse");
                    }
                    pending.account_id = check.account_id;
                    self.reflect(
                        if self
                            .pending
                            .as_ref()
                            .is_some_and(|p| p.account_id.is_some())
                        {
                            "approval"
                        } else {
                            "pairing"
                        },
                    );
                } else if self.binding.is_some() {
                    let devices = self.devices().await?;
                    // A fresh list is also what a restore after a restart is waiting for.
                    self.apply_restore(&devices);
                    if let Ok(mut status) = self.status.lock() {
                        status.devices = devices;
                        status.error = None;
                    }
                }
            }
            Action::Approve {
                enrollment_id,
                account_id,
                transmit,
            } => {
                let pending = self.pending.as_mut().ok_or("pairingExpired")?;
                if pending.id != enrollment_id
                    || pending.account_id.as_deref() != Some(&account_id)
                    || pending.expires_at <= now_ms()
                {
                    return Err("pairingExpired");
                }
                let token = pending
                    .station_credential
                    .get_or_insert(transport::random_secret()?)
                    .clone();
                let binding = Binding {
                    origin: self.client.origin().into(),
                    station_id: pending.id.clone(),
                    account_id: account_id.clone(),
                };
                // Confirm the OS vault works before creating cloud station authority.
                self.vault.stage(&binding, &token)?;
                let result = self
                    .client
                    .post(
                        "enroll/approve",
                        None,
                        json!({ "id": pending.id, "proof": pending.proof, "credential": token }),
                    )
                    .await;
                let value = match result {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = self.vault.remove(&binding);
                        return Err(error);
                    }
                };
                if value.get("stationId").and_then(|v| v.as_str()) != Some(&binding.station_id)
                    || value.get("accountId").and_then(|v| v.as_str()) != Some(&binding.account_id)
                {
                    let _ = self.vault.remove(&binding);
                    return Err("invalidResponse");
                }
                self.vault.save(&binding, &token)?;
                // S3-M1: a new pairing is a new station, with a signing key of its own.
                let key = self.new_station_key(&binding);
                // One approval: the service approved the browser that confirmed this pairing along
                // with the station. It gets what any approved browser gets, waiting in the record
                // for Turn on. A service that reports no browser pairs exactly as it used to.
                let paired = value
                    .get("device")
                    .and_then(|d| {
                        Some((
                            d.get("id")?.as_str()?.to_string(),
                            Approval {
                                expires_at: d.get("expiresAt")?.as_u64()?,
                                generation: d.get("generation").and_then(|g| g.as_u64()),
                            },
                            // The device key the confirming browser brought (A5), if any.
                            d.get("publicKey")
                                .and_then(|k| k.as_str())
                                .and_then(fingerprint),
                        ))
                    })
                    .filter(|(id, approval, _)| identifier(id) && approval.expires_at > now_ms());
                if let Ok(mut control) = self.control.lock() {
                    control.station_key = key;
                    control.key_refused = false;
                    // A new pairing starts with nothing remembered: off, no grants, no pins.
                    control.remember(Persist::Bound(binding.clone(), None, None));
                    if let Some((device_id, approval, key)) = paired {
                        control.remember(Persist::Browser {
                            device_id: device_id.clone(),
                            approval: Some(approval),
                            logging: Some(true),
                            control: Some(true),
                            transmit: Some(transmit),
                        });
                        // A5, operator ruling D4 (2026-09-28): that browser's key is pinned on first
                        // use, the trust the pairing already places in the service. Both ends show
                        // its fingerprint straight after, and revoking the browser is one click.
                        control.remember(Persist::Pin { device_id, key });
                    }
                }
                self.binding = Some(binding);
                self.pending = None;
                self.reflect("disabled");
                // Approving the pairing also turns Remote on (operator decision 2026-09-14), exactly
                // as Turn on Remote does: a fresh authority, and the TX-enable latch untouched. A
                // Turn off or Cancel made while the approval was on its way moved the generation,
                // and wins.
                let turned_on = self.control.lock().ok().and_then(|mut control| {
                    (control.generation == generation).then(|| {
                        control.stop();
                        control.enabled = true;
                        control.generation
                    })
                });
                if let Some(generation) = turned_on {
                    self.enable(generation)?;
                }
            }
            Action::Enable {} => self.enable(generation)?,
            Action::Disable {} => {
                self.reflect(if self.binding.is_some() {
                    "disabled"
                } else {
                    "unpaired"
                });
            }
            Action::Forget {} => {
                let (binding, token) = self.bound()?;
                self.client
                    .post(&station_path(&binding, "revoke"), Some(&token), empty())
                    .await?;
                self.vault.remove(&binding)?;
                // The pairing's key goes with it: a new pairing makes its own.
                let _ = self.vault.remove_station_key();
                if let Ok(mut control) = self.control.lock() {
                    control.station_key = None;
                    control.key_refused = false;
                    control.remember(Persist::Removed);
                }
                self.binding = None;
                self.pending = None;
                self.reflect("unpaired");
            }
            Action::Device {
                device_id,
                approve,
                transmit,
                key,
            } => {
                if !identifier(&device_id) || key.as_deref().is_some_and(|key| !pin_shape(key)) {
                    return Err("invalidRequest");
                }
                let (binding, token) = self.bound()?;
                self.client
                    .post(
                        &station_path(
                            &binding,
                            if approve {
                                "approve-device"
                            } else {
                                "revoke-device"
                            },
                        ),
                        Some(&token),
                        json!({ "deviceId": device_id }),
                    )
                    .await?;
                if let Ok(mut status) = self.status.lock() {
                    status.devices.clear();
                }
                if approve {
                    // The list names the approval the service just wrote; the grant binds to it.
                    let devices = self.devices().await?;
                    let granted = self.grant_approved(
                        &devices,
                        device_id,
                        transmit,
                        key,
                        (generation, epoch),
                    );
                    if let Ok(mut status) = self.status.lock() {
                        status.devices = devices;
                    }
                    granted?;
                }
            }
        }
        Ok(())
    }
}

/// The vault writer. Each write is a whole record for the current pairing, produced in the order
/// the decisions were made, so a backlog collapses to the latest write of each entry. A failed save
/// is followed by deleting the record: a record that could not be updated must not survive to be
/// restored. No state record means Remote stays off at the next launch; no pins record means every
/// browser is approved again at the radio before it streams.
fn write_remembered(vault: Arc<dyn Vault>, mut writes: mpsc::UnboundedReceiver<Write>) {
    while let Some(first) = writes.blocking_recv() {
        let (mut state, mut pins) = (None, None);
        let mut next = Some(first);
        while let Some(write) = next {
            match write {
                Write::Save(_) | Write::Remove => state = Some(write),
                Write::SavePins(_) | Write::RemovePins => pins = Some(write),
            }
            next = writes.try_recv().ok();
        }
        for write in [state, pins].into_iter().flatten() {
            match write {
                Write::Save(state) => {
                    if vault.save_state(&state).is_err() {
                        let _ = vault.remove_state();
                    }
                }
                Write::Remove => {
                    let _ = vault.remove_state();
                }
                Write::SavePins(pins) => {
                    if vault.save_pins(&pins).is_err() {
                        let _ = vault.remove_pins();
                    }
                }
                Write::RemovePins => {
                    let _ = vault.remove_pins();
                }
            }
        }
    }
}

#[tauri::command]
pub fn get_remote_station_status(
    service: tauri::State<'_, Service>,
) -> Result<Status, &'static str> {
    service.status()
}
#[tauri::command]
pub async fn remote_station_action(
    service: tauri::State<'_, Service>,
    action: Action,
) -> Result<Status, &'static str> {
    service.action(action).await
}

// The canonical desktop WebView owns the in-memory bank. Detached windows and
// the hosted browser cannot publish it or send a generic durable-state map.
#[tauri::command]
pub fn publish_remote_memory_bank(
    service: tauri::State<'_, Service>,
    window: tauri::WebviewWindow,
    generation: String,
    bank: Option<String>,
) -> bool {
    window.label() == "main" && service.publish_memories(&generation, bank.as_deref())
}
