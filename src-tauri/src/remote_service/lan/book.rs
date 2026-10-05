//! The shack's LAN identity and the computers paired with it: what the listener proves itself
//! with, whom it admits, and the pairing window that adds one. Kept in the OS credential store
//! (`vault::LanVault`) and in memory while Nexus runs, so a handshake never waits on the store.
//!
//! ## The LAN key
//!
//! P-256, made with ring the first time the listener starts with none kept, and kept in its own
//! entry with the LAN station id beside it, before anything uses it. It is not the hosted station
//! key: the two are paired and revoked separately. Its private half is never logged, shown or
//! sent. The book hands out [`tls::Identity`] (no `Debug`, `Clone` or `Serialize`) and the public
//! half's fingerprint, nothing else. A store that will not answer makes no key, because one may be
//! in it: the listener then says `noKey`. A key record that does not read, as JSON or as a key, is
//! treated the same, and so is a paired list that does not read or is out of shape (it admits
//! nobody and is not written over): either may be a newer Nexus's, and making a key over one would
//! delete the paired list too. Resetting the network identity at the shack is what replaces them.
//!
//! ## The paired computers
//!
//! A record is the SHA-256 of the computer's key (its pin, which is all that admits it) and the name
//! it gave. Its device id is worked out from the pin ([`device_id`]), so the record does not carry
//! one, and pairing the same key again gives the same id. At most [`MAX_PAIRED`], with names of up
//! to [`NAME_CHARS`] characters: whatever the names, that fits the smallest credential blob
//! (Windows, 2560 bytes of UTF-16) in one entry, and a list that would not is refused before it is
//! written. A computer stays paired until it is removed at the shack: no approval lapses (the
//! operator's ruling of 2026-10-04, "Until revoked").
//!
//! Removing one takes it out of memory first, so it is refused at once and its session told to
//! end, and only then out of the store. If the store will not take the shorter list, its entry is
//! deleted instead, so a restart admits nobody rather than a computer removed here.
//!
//! ## The pairing window (the operator's ruling of 2026-10-04, "One press")
//!
//! Opening it at the shack is the approval. It holds a code of sixteen hexadecimal characters (64
//! random bits), lives ten minutes, and closes for good on the first pairing it completes or the
//! [`WRONG_PROOFS`]th wrong proof, whichever comes first. A computer that proves the code
//! (`super::pairing`) is paired at once, with no second press. The code is kept nowhere but here
//! and on the shack's screen.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ring::digest::{digest, Context, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use tokio::sync::watch;

use super::super::station_key::Signer;
use super::super::transport::identifier;
use super::super::vault::{LanDevice, LanDevices, LanKey, LanVault};
use super::pairing;
use super::tls;

/// The most computers paired at once.
pub const MAX_PAIRED: usize = 8;
/// The longest name a computer may give, in characters.
pub const NAME_CHARS: usize = 32;
/// How long a pairing window stays open.
pub const PAIRING_FOR: Duration = Duration::from_secs(600);
/// The wrong proof that closes the window.
pub const WRONG_PROOFS: u8 = 3;
/// The smallest OS credential blob, Windows': 2560 bytes of UTF-16.
const CREDENTIAL_BYTES: usize = 2560;
/// What a device id's hash begins with.
const DEVICE_LABEL: &[u8] = b"nexus-lan-device/1";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The device id a computer's key gives: SHA-256 over [`DEVICE_LABEL`] and its pin, the first
/// sixteen bytes, shaped as every id the authority takes is. Its version digit is 8 (RFC 9562's
/// own-use form), so it is never a hosted browser's id, which the service makes as version 4.
pub fn device_id(pin: &[u8; 32]) -> String {
    let mut hash = Context::new(&SHA256);
    hash.update(DEVICE_LABEL);
    hash.update(pin);
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash.finish().as_ref()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h = hex(&bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// A name a computer may give: something to read, short, and no control characters.
pub fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= NAME_CHARS
        && !name.chars().any(char::is_control)
}

/// Does this record fit the smallest credential blob?
fn fits(devices: &LanDevices) -> bool {
    serde_json::to_string(devices)
        .is_ok_and(|value| value.encode_utf16().count() * 2 <= CREDENTIAL_BYTES)
}

/// The LAN key, as the store answered.
#[derive(Default)]
enum Key {
    /// Not read yet.
    #[default]
    Unread,
    /// The store would not answer, or holds one that does not read. No key is made, because one
    /// may be in it.
    Locked,
    /// The store holds none: one is made when the listener first starts.
    Absent,
    /// The key, and the fingerprint of its public half (SHA-256 of its SPKI, lowercase hex).
    Kept { key: LanKey, fingerprint: String },
}

/// One paired computer, in memory.
#[derive(Clone)]
struct Device {
    pin: [u8; 32],
    id: String,
    name: String,
}

impl Device {
    fn from_record(record: &LanDevice) -> Option<Self> {
        let pin: [u8; 32] = record
            .pin
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            .then(|| tempo_stream::protocol::hex_bytes(&record.pin))
            .flatten()?
            .try_into()
            .ok()?;
        valid_name(&record.name).then(|| Self {
            id: device_id(&pin),
            pin,
            name: record.name.clone(),
        })
    }

    fn record(&self) -> LanDevice {
        LanDevice {
            pin: hex(&self.pin),
            name: self.name.clone(),
        }
    }
}

#[derive(Default)]
struct Kept {
    key: Key,
    /// Moves whenever the key changes (made, or reset), so a listener on the old one restarts.
    generation: u64,
    /// The paired computers. `None` until read, or when the store would not give them up: nobody
    /// is admitted then, and nobody added, since writing would lose what the store holds.
    devices: Option<Vec<Device>>,
}

/// The pairing window while it is open.
struct Window {
    code: [u8; 8],
    /// Which window this is: a pairing started under one cannot finish under another.
    serial: u64,
    until: Instant,
    /// When it closes, in milliseconds since 1970, for the shack's screen.
    closes_at: u64,
    wrong: u8,
}

/// What a computer's proof came to ([`Book::check`]).
#[derive(Debug, PartialEq, Eq)]
pub enum Checked {
    /// The code: the window has closed for good, and the computer may be paired.
    Right,
    /// Not the code. The third closes the window.
    Wrong,
    /// No window, or not the one the pairing started under, or past its ten minutes.
    Closed,
}

/// The pairing window as the shack shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingView {
    /// The code, sixteen lowercase hexadecimal characters.
    pub code: String,
    /// When it closes, in milliseconds since 1970.
    pub closes_at: u64,
}

/// A paired computer as the shack shows it: its device id, the name it gave, and its key's
/// fingerprint (the pin, lowercase hex).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ComputerView {
    pub id: String,
    pub name: String,
    pub key: String,
}

/// What the shack shows of the book.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    /// The LAN key's fingerprint, once there is one.
    pub key: Option<String>,
    pub pairing: Option<PairingView>,
    pub devices: Vec<ComputerView>,
}

/// The shack's LAN key, its paired computers and its pairing window. See the module header.
pub struct Book {
    vault: Arc<dyn LanVault>,
    kept: Mutex<Kept>,
    window: Mutex<Option<Window>>,
    serial: AtomicU64,
    /// Moves on every change to who is paired, so a connected computer's session asks again.
    revision: watch::Sender<u64>,
    /// One write to the store at a time. Never held with `kept` or `window`, and never on the
    /// path that refuses a computer: that is memory's.
    writing: Mutex<()>,
}

impl Book {
    /// A book on `vault`, not read yet: it admits nobody until [`Book::load`].
    pub fn new(vault: Arc<dyn LanVault>) -> Self {
        Self {
            vault,
            kept: Mutex::new(Kept::default()),
            window: Mutex::new(None),
            serial: AtomicU64::new(0),
            revision: watch::Sender::new(0),
            writing: Mutex::new(()),
        }
    }

    /// Read the key and the paired computers from the store, on the listener's own thread, since
    /// the store can block. Once both have been read it reads nothing again, so nothing read here
    /// can undo a later change; until then (a store that would not answer) each call tries again,
    /// and nothing could have changed meanwhile, since nothing is added or removed until the
    /// computers have been read. A list for another key admits nobody; one that does not read, or is
    /// out of shape, admits nobody and is never written over, as a store that would not answer.
    pub fn load(&self) {
        let _writing = self.writing.lock();
        if self
            .kept
            .lock()
            .is_ok_and(|k| matches!(k.key, Key::Kept { .. } | Key::Absent) && k.devices.is_some())
        {
            return;
        }
        let key = match self.vault.lan_key() {
            Ok(Some(key)) => kept(key).unwrap_or(Key::Locked),
            Ok(None) => Key::Absent,
            Err(_) => Key::Locked,
        };
        let devices = match &key {
            Key::Kept { key, .. } => match self.vault.lan_devices() {
                Ok(Some(list)) if list.station_id == key.station_id => {
                    let devices: Option<Vec<Device>> =
                        list.devices.iter().map(Device::from_record).collect();
                    devices.filter(|d| d.len() <= MAX_PAIRED && unique(d))
                }
                Ok(_) => Some(Vec::new()),
                Err(_) => None,
            },
            Key::Absent => Some(Vec::new()),
            Key::Unread | Key::Locked => None,
        };
        if let Ok(mut kept) = self.kept.lock() {
            kept.key = key;
            kept.devices = devices;
            kept.generation = kept.generation.wrapping_add(1);
        }
        self.revised();
    }

    /// The listener's identity and the key's generation: the kept key, or a new one made and kept
    /// now when the store holds none. `None` when the store would not answer, or would not keep a
    /// new one. A store that would not answer is asked again first, so turning LAN on after
    /// unlocking it finds the key.
    pub fn identity(&self) -> Option<(tls::Identity, u64)> {
        self.load();
        if let Some(found) = self.kept_identity() {
            return found;
        }
        self.make(false).ok()?;
        self.kept_identity().flatten()
    }

    /// `Some` with the kept key's identity, `Some(None)` when there is none to be had, and `None`
    /// when one may be made.
    fn kept_identity(&self) -> Option<Option<(tls::Identity, u64)>> {
        let kept = self.kept.lock().ok()?;
        match &kept.key {
            Key::Kept { key, .. } => Some(
                tls::Identity::new(&key.pkcs8, key.station_id.clone())
                    .map(|identity| (identity, kept.generation)),
            ),
            Key::Absent => None,
            Key::Unread | Key::Locked => Some(None),
        }
    }

    /// A new LAN key, kept in the store before it is used, with nobody paired to it. `anew`: even
    /// if there is one already (a reset); otherwise only when the store holds none.
    fn make(&self, anew: bool) -> Result<(), &'static str> {
        let _writing = self
            .writing
            .lock()
            .map_err(|_| "credentialStoreUnavailable")?;
        if !anew
            && !matches!(
                self.kept.lock().map(|k| matches!(k.key, Key::Absent)),
                Ok(true)
            )
        {
            return Ok(());
        }
        // The old key's computers first: if the new key cannot be kept, they are still gone.
        self.vault.remove_lan_devices()?;
        let (_, pkcs8) = Signer::generate().ok_or("credentialStoreUnavailable")?;
        let station_id = super::channel::session_id().ok_or("credentialStoreUnavailable")?;
        let key = LanKey { station_id, pkcs8 };
        let made = kept(LanKey {
            station_id: key.station_id.clone(),
            pkcs8: key.pkcs8.clone(),
        })
        .ok_or("credentialStoreUnavailable")?;
        self.vault.save_lan_key(&key)?;
        if let Ok(mut kept) = self.kept.lock() {
            kept.key = made;
            kept.devices = Some(Vec::new());
            kept.generation = kept.generation.wrapping_add(1);
        }
        self.revised();
        Ok(())
    }

    /// The key's generation, which moves when it is made or reset.
    pub fn generation(&self) -> u64 {
        self.kept.lock().map_or(0, |k| k.generation)
    }

    /// The paired computer that holds the key with this pin: its device id.
    pub fn paired(&self, pin: &[u8; 32]) -> Option<String> {
        let kept = self.kept.lock().ok()?;
        kept.devices
            .as_ref()?
            .iter()
            .find(|d| d.pin == *pin)
            .map(|d| d.id.clone())
    }

    /// What TLS asks at each handshake: who holds a key, and whether the window is open now.
    pub fn verifier(self: &Arc<Self>) -> (tls::Paired, tls::Pairing) {
        let (paired, open) = (self.clone(), self.clone());
        (
            Arc::new(move |pin: &[u8; 32]| paired.paired(pin)),
            Arc::new(move || open.pairing(Instant::now())),
        )
    }

    /// Every paired computer's device id.
    pub fn ids(&self) -> Vec<String> {
        self.kept.lock().map_or_else(
            |_| Vec::new(),
            |k| k.devices.iter().flatten().map(|d| d.id.clone()).collect(),
        )
    }

    /// Told each time who is paired changes.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.revision.subscribe()
    }

    fn revised(&self) {
        self.revision.send_modify(|r| *r = r.wrapping_add(1));
    }

    /// Open the pairing window: a press at the shack, and the approval itself. A new code, for ten
    /// minutes, in place of any window already open. Refused with no key (`noKey`), a store that
    /// would not give up the paired computers, or [`MAX_PAIRED`] of them already (`lanFull`).
    pub fn open(&self, now: Instant, wall_ms: u64) -> Result<(), &'static str> {
        {
            let kept = self.kept.lock().map_err(|_| "credentialStoreUnavailable")?;
            if !matches!(kept.key, Key::Kept { .. }) {
                return Err("noKey");
            }
            let devices = kept.devices.as_ref().ok_or("credentialStoreUnavailable")?;
            if devices.len() >= MAX_PAIRED {
                return Err("lanFull");
            }
        }
        let mut code = [0; 8];
        SystemRandom::new()
            .fill(&mut code)
            .map_err(|_| "serviceUnavailable")?;
        let serial = self.serial.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        *self.window.lock().map_err(|_| "serviceUnavailable")? = Some(Window {
            code,
            serial,
            until: now + PAIRING_FOR,
            closes_at: wall_ms.saturating_add(PAIRING_FOR.as_millis() as u64),
            wrong: 0,
        });
        Ok(())
    }

    /// Close the pairing window, if it is open.
    pub fn close(&self) {
        if let Ok(mut window) = self.window.lock() {
            *window = None;
        }
    }

    /// Is the pairing window open at `now`?
    pub fn pairing(&self, now: Instant) -> bool {
        self.window
            .lock()
            .is_ok_and(|w| w.as_ref().is_some_and(|w| now < w.until))
    }

    /// For a pairing that starts at `now`: which window it is, and the key its code gives.
    pub(super) fn proof_key(&self, now: Instant) -> Option<(u64, ring::hmac::Key)> {
        let window = self.window.lock().ok()?;
        let window = window.as_ref().filter(|w| now < w.until)?;
        Some((window.serial, pairing::code_key(&window.code)?))
    }

    /// The computer's proof over `transcript`, for a pairing started under window `serial`.
    /// Right closes the window for good (one pairing per code); wrong counts, and the third
    /// closes it. Checked in constant time.
    pub(super) fn check(
        &self,
        serial: u64,
        transcript: &[u8; 32],
        proof: &[u8],
        now: Instant,
    ) -> Checked {
        let Ok(mut slot) = self.window.lock() else {
            return Checked::Closed;
        };
        let Some(window) = slot
            .as_mut()
            .filter(|w| w.serial == serial && now < w.until)
        else {
            return Checked::Closed;
        };
        let right = pairing::code_key(&window.code)
            .is_some_and(|key| pairing::holds(&key, pairing::Side::Computer, transcript, proof));
        if right {
            *slot = None;
            return Checked::Right;
        }
        window.wrong = window.wrong.saturating_add(1);
        if window.wrong >= WRONG_PROOFS {
            *slot = None;
        }
        Checked::Wrong
    }

    /// Pair the key with this pin as `name`, for a pairing proved under the LAN station id `under`:
    /// kept in the store first, then admitted. Pairing a key already paired replaces its record.
    /// `pairingClosed` when the key has been reset since that proof, which was made for an
    /// identity that is gone; `pairingFull` with [`MAX_PAIRED`] paired already, or a list that
    /// would not fit; `unavailable` when the store will not keep it; `invalidRequest` for a name
    /// out of shape, which the pairing has already refused.
    pub(super) fn add(
        &self,
        pin: [u8; 32],
        name: &str,
        under: &str,
    ) -> Result<String, &'static str> {
        let _writing = self.writing.lock().map_err(|_| "unavailable")?;
        let (station_id, mut devices) = {
            let kept = self.kept.lock().map_err(|_| "unavailable")?;
            let Key::Kept { key, .. } = &kept.key else {
                return Err("unavailable");
            };
            // Asked under the store's lock, which a reset takes too, so no reset lands between.
            if key.station_id != under {
                return Err("pairingClosed");
            }
            (
                key.station_id.clone(),
                kept.devices.clone().ok_or("unavailable")?,
            )
        };
        if !valid_name(name) {
            return Err("invalidRequest");
        }
        devices.retain(|d| d.pin != pin);
        if devices.len() >= MAX_PAIRED {
            return Err("pairingFull");
        }
        let device = Device {
            pin,
            id: device_id(&pin),
            name: name.trim().to_string(),
        };
        devices.push(device.clone());
        let record = LanDevices {
            station_id,
            devices: devices.iter().map(Device::record).collect(),
        };
        if !fits(&record) {
            return Err("pairingFull");
        }
        self.vault
            .save_lan_devices(&record)
            .map_err(|_| "unavailable")?;
        // Admitted now that the store keeps it, and only under the key it was paired with.
        {
            let mut kept = self.kept.lock().map_err(|_| "unavailable")?;
            let same =
                matches!(&kept.key, Key::Kept { key, .. } if key.station_id == record.station_id);
            let Some(paired) = kept.devices.as_mut().filter(|_| same) else {
                return Err("unavailable");
            };
            paired.retain(|d| d.pin != pin);
            paired.push(device.clone());
        }
        self.revised();
        Ok(device.id)
    }

    /// Take `device` out of memory: refused from now on, and its session told to end. Returns
    /// whether it was paired. The store follows with [`Book::keep`].
    pub fn forget(&self, device: &str) -> bool {
        let removed = self.kept.lock().is_ok_and(|mut kept| {
            kept.devices.as_mut().is_some_and(|devices| {
                let before = devices.len();
                devices.retain(|d| d.id != device);
                devices.len() != before
            })
        });
        if removed {
            self.revised();
        }
        removed
    }

    /// Take every computer out of memory, and close the window: the first half of a reset. Returns
    /// their device ids. The store follows with [`Book::renew`].
    pub fn forget_all(&self) -> Vec<String> {
        self.close();
        let gone = self.kept.lock().map_or_else(
            |_| Vec::new(),
            |mut kept| match kept.devices.as_mut() {
                Some(devices) => devices.drain(..).map(|d| d.id).collect(),
                None => Vec::new(),
            },
        );
        self.revised();
        gone
    }

    /// Write who is paired now to the store. If the store will not take it, its entry is deleted
    /// instead, so a restart admits nobody rather than a computer removed here. `Err` only when
    /// neither could be done.
    pub fn keep(&self) -> Result<(), &'static str> {
        let _writing = self
            .writing
            .lock()
            .map_err(|_| "credentialStoreUnavailable")?;
        let record = {
            let kept = self.kept.lock().map_err(|_| "credentialStoreUnavailable")?;
            match (&kept.key, &kept.devices) {
                (Key::Kept { key, .. }, Some(devices)) => LanDevices {
                    station_id: key.station_id.clone(),
                    devices: devices.iter().map(Device::record).collect(),
                },
                // Nothing was read, so nothing is written over what the store holds.
                _ => return Ok(()),
            }
        };
        if !record.devices.is_empty() && self.vault.save_lan_devices(&record).is_ok() {
            return Ok(());
        }
        self.vault.remove_lan_devices()
    }

    /// A new LAN key with nobody paired to it, kept in the store: the second half of a reset. The
    /// old computers leave the store first, so if the new key cannot be kept they are gone anyway,
    /// and the old key admits nobody.
    pub fn renew(&self) -> Result<(), &'static str> {
        self.make(true)
    }

    /// What the shack shows: the key's fingerprint, the window while it is open, the computers.
    pub fn view(&self, now: Instant) -> View {
        let mut view = View::default();
        if let Ok(kept) = self.kept.lock() {
            if let Key::Kept { fingerprint, .. } = &kept.key {
                view.key = Some(fingerprint.clone());
            }
            view.devices = kept
                .devices
                .iter()
                .flatten()
                .map(|d| ComputerView {
                    id: d.id.clone(),
                    name: d.name.clone(),
                    key: hex(&d.pin),
                })
                .collect();
        }
        if let Ok(window) = self.window.lock() {
            view.pairing = window
                .as_ref()
                .filter(|w| now < w.until)
                .map(|w| PairingView {
                    code: hex(&w.code),
                    closes_at: w.closes_at,
                });
        }
        view
    }
}

/// A key the store gave, ready to keep: its station id an id, its document a P-256 key.
fn kept(key: LanKey) -> Option<Key> {
    if !identifier(&key.station_id) {
        return None;
    }
    let identity = tls::Identity::new(&key.pkcs8, key.station_id.clone())?;
    let spki = tempo_stream::protocol::hex_bytes(identity.public_key())?;
    Some(Key::Kept {
        fingerprint: hex(digest(&SHA256, &spki).as_ref()),
        key,
    })
}

/// No two records hold the same key.
fn unique(devices: &[Device]) -> bool {
    devices
        .iter()
        .enumerate()
        .all(|(i, d)| devices[..i].iter().all(|e| e.pin != d.pin))
}
