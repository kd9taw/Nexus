//! Enrolling a computer on this network by its own sign-in: the account's road onto the LAN, beside
//! the pairing window, with no code and no press at the shack.
//!
//! A computer that signed in to the station's account (as its owner, or as someone the station
//! invited to operate) holds the issuer's ID token for exactly its keys: its proof. On its first
//! contact with this shack it opens TLS with the key it made for this shack (the LAN key its proof
//! names), and its first message is `{"type":"enrol", …, "kind", "publicKey", "proof"}`. That
//! connection reaches nothing but this check: the proof must hold for the key this very TLS session
//! proved and the other key it names (`publicKey`, its key for the internet road), under the
//! station's owner or an invitation, signed within the offline age (`tempo_client::account`). All
//! of it is read from memory, so it holds with the internet down: the owner, the issuer's keys and
//! the people invited, as the station kept them ([`Enrolment::arm`]). Then the key is kept for the
//! LAN's own device cache and paired in the book, the computer is told its device id, and the
//! connection ends; the computer comes back with a hello, as after a pairing.
//!
//! A computer invited only to listen is refused (`listenNative`): every computer in the book holds
//! station control while the LAN is on. A refused proof counts against its address at the gate, as
//! a wrong pairing code does.
//!
//! **Its proof's age.** A computer enrolled this way is admitted on the LAN for
//! `tempo_client::account::PROOF_MAX_AGE_S` from its proof's signing, owner's and guest's alike,
//! internet up or down, and then signs in again (it enrols afresh, and its new proof replaces the
//! old). The station hears the service only when it reads its list, so between lists nothing else
//! vouches for it. [`Enrolment::bounded`] holds every admission to that age: TLS, the hello, and the
//! check each session makes at every status tick, so one that passes it mid-session ends; and one
//! past it is taken out of the book at launch and after each list (`super::Lan::expire`). A computer
//! paired by code is not enrolled and is not held to it, and neither is one enrolled before and
//! then paired again with a code at the shack: the pairing takes it out of the device cache
//! ([`Enrolment::paired_by_code`]), so a code pairing at the shack outranks a sign-in's age. It then
//! stays until it is removed at the shack, and a removal on the web no longer reaches it here: the
//! service's list reaches the LAN only through that cache.
//!
//! **One writer at a time.** The device cache has writers here and beside: an enrolment
//! ([`admit`]), an expiry ([`Enrolment::forget`]), a code pairing ([`Enrolment::paired_by_code`]),
//! and each list the station hears (`remote_service::account::sweep`, and the controller keeping a
//! device it vouched for); and the copy in memory follows it ([`Enrolment::reread`]). Each holds
//! the cache (`DevicesHeld`) from its read to its save, so none saves over another's change. An
//! enrolment holds it until its computer is in the book, so no list takes its entry out between the
//! two; that is the one place the book's lock is taken inside it, and nothing holding the book's
//! takes it (an expiry and a code pairing write the book first and let its lock go). An expiry
//! takes out only an entry still past its age once it holds the cache: a computer enrolled again
//! since it was found expired is in the book again, held to its new proof.
//!
//! **The sign-in transmit hold is WAIVED here (the 2026-10-23 sign-in decision).** A computer that signs in over the
//! INTERNET and is auto-granted control carries a 15-minute transmit hold
//! (`super::super::account::GRANTS_ON_SIGN_IN`). A computer enrolling on the station's OWN network
//! is different: it is on the shack's LAN, which stands in for the shack tap the hold is waived by,
//! so [`admit`] records no hold — it never touches `operations::Authority::hold_transmit_until`, and
//! the engine's arm gate holds a LAN-enrolled computer not at all.
//!
//! **What a Remove at the shack leaves behind** (`super::super::vault::Ties`). Each key removed at
//! the shack, a LAN key or an internet key, is kept with when, for a proof's age: an enrolment on a
//! proof signed by then is refused ([`admit`]), as is the internet road's grant on one
//! (`remote_service::Controller::vouch`), and a computer still in the book with a removed key is
//! admitted nowhere ([`Enrolment::admits`]), so a book the store would not write after a Remove is
//! no road back after a restart. A fresh sign-in, signed after the removal, enrols as any does; a
//! pairing by code at the shack takes the removal out. Beside it, the link between a computer's two
//! roads: the internet key its proof names beside the LAN key, kept when the station first checks
//! the proof ([`admit`], [`tie_vouched`]), so a Remove ends both roads with no proof read again and
//! no list heard. A guest's link holds only when the computer proved its LAN key here and the
//! station checked its listing's proof, both for the same guest: so no guest's claim names another
//! computer's key into a Remove. Each link names who signed in (a hash of a guest's address), so an
//! invitation taken back takes that guest's computers off the LAN at once: those the guest proved
//! here, never one a listing only claims.
//!
//! What is kept fits the smallest credential blob, so it is bounded. A removal the bound pushes out
//! while a proof signed before it could still be in use leaves its time as the floor: every proof
//! signed by then is refused on every road, whatever key it names, until no such proof is still in
//! age. A link the bound pushes out is a claim first, and a link a computer proved here last. Each
//! removal time is read with the issuer's clock leeway, on the side that refuses. Until all of it
//! has been read, nobody is admitted and no proof is granted; an entry that does not read at all
//! stays so, and the shack says so, until Reset network identity writes it afresh. The reset sets
//! the floor to now whatever was kept, so a floor kept while the station's clock ran ahead comes
//! back ([`Enrolment::recover`]).
//!
//! Nothing here writes a log line, and no proof is kept: the cache holds the key's pin, the role
//! and the proof's signing time.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use tempo_client::account::{self, CachedDevice, DeviceCache, Invite, KeySet, Owner, Role, Tenant};
use tokio::sync::watch;

use super::super::vault::{AccountVault, DevicesHeld, Removed, Tie, Ties};
use super::book::{valid_name, Book};
use super::channel::Socket;
use super::pairing::{refuse, say, Desk, Outcome};
use super::tls;

/// The longest proof taken: an ID token is a little over a kilobyte.
const PROOF_CHARS: usize = 4096;

/// The most links kept: one for each device the LAN's cache keeps. A link left out only leaves a
/// Remove telling the shack to check the other list.
const TIES_KEPT: usize = account::CACHED_DEVICES;

/// The most removals kept, the newest: with the links, they fit the smallest credential blob.
const REMOVED_KEPT: usize = 12;

/// How often a connection may read again what did not read ([`Enrolment::retry`]).
pub(crate) const RETRY_EVERY: Duration = Duration::from_secs(5);

/// A key or pin as [`Ties`] keeps it: the first sixteen hex digits of its SHA-256.
pub(crate) fn short(pin: &str) -> String {
    pin.get(..16).unwrap_or(pin).to_ascii_lowercase()
}

/// A guest's address as [`account::mailbox`] gives it, as [`Ties`] keeps it: never in clear.
pub(crate) fn mailbox_tag(mailbox: &str) -> String {
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    hash.update(b"nexus-lan-mailbox/1");
    hash.update(mailbox.as_bytes());
    short(&tempo_client::hex(hash.finish().as_ref()))
}

/// Whether a proof signed at `issued_at` was signed by `at`, a time on the station's clock: with
/// the issuer's clock leeway, so a station clock running slow refuses more, never less.
fn signed_by(issued_at: u64, at: u64) -> bool {
    issued_at <= at.saturating_add(tempo_client::id_token::LEEWAY_S)
}

/// Whether a proof signed at `issued_at` naming any of `keys` (as [`short`] gives them) was signed
/// by the time one of them was removed at the shack.
pub(crate) fn removed_by(removed: &[Removed], keys: &[&str], issued_at: u64) -> bool {
    removed
        .iter()
        .any(|r| keys.contains(&r.key.as_str()) && signed_by(issued_at, r.at))
}

/// Whether a proof signed at `issued_at` was signed by the floor `kept` holds (see the header):
/// refused, whatever key it names.
pub(crate) fn floored(kept: &Ties, issued_at: u64) -> bool {
    kept.floor > 0 && signed_by(issued_at, kept.floor)
}

/// Whether `tie` links its two roads (see the header): the owner's, checked on either road; a
/// guest's, only when it proved its LAN key here and its listing's proof was checked, both its own.
pub(crate) fn linked(tie: &Tie) -> bool {
    match tie.mailbox {
        None => tie.enrolled || tie.vouched,
        Some(_) => tie.enrolled && tie.vouched,
    }
}

/// `ties`, each removal (and the floor) past a proof's age after it dropped, then held to what is
/// kept, and saved: each removal pushed out still in force raises the floor to its time, and the
/// links pushed out are claims first, then the oldest (see the header).
fn keep_ties(
    vault: &dyn AccountVault,
    held: &DevicesHeld<'_>,
    ties: &mut Ties,
    now_s: u64,
) -> Result<(), &'static str> {
    let age = account::PROOF_MAX_AGE_S + tempo_client::id_token::LEEWAY_S;
    ties.removed.retain(|r| now_s < r.at.saturating_add(age));
    if now_s >= ties.floor.saturating_add(age) {
        ties.floor = 0;
    }
    let over = ties.removed.len().saturating_sub(REMOVED_KEPT);
    for pushed in ties.removed.drain(..over) {
        ties.floor = ties.floor.max(pushed.at);
    }
    while ties.ties.len() > TIES_KEPT {
        // A claim that links nothing, then one checked on the internet road alone, then the oldest.
        let out = (ties.ties.iter().position(|t| !t.enrolled && !linked(t)))
            .or_else(|| ties.ties.iter().position(|t| !t.enrolled))
            .unwrap_or(0);
        ties.ties.remove(out);
    }
    vault.save_ties(held, ties)
}

/// The listing of the computer whose LAN key has `lan` as its pin and internet key `key` as its
/// fingerprint (lowercase hex each), its proof checked for `mailbox` (a guest's, or the owner's for
/// `None`): its link kept (see the header). A claim never replaces the link of a computer that
/// proved its LAN key here for another key or person. Blocks on the credential store.
pub(crate) fn tie_vouched(
    vault: &dyn AccountVault,
    lan: &str,
    key: &str,
    mailbox: Option<&str>,
    now_s: u64,
) -> Result<(), &'static str> {
    let held = DevicesHeld::take(vault);
    let mut ties = vault.ties()?.unwrap_or_default();
    let (lan, key, mailbox) = (short(lan), short(key), mailbox.map(mailbox_tag));
    let at = ties.ties.iter().position(|t| t.lan == lan);
    let was = at.map(|at| ties.ties.remove(at));
    let tie = match was.clone() {
        Some(t) if t.key == key && t.mailbox == mailbox => Tie { vouched: true, ..t },
        Some(t) if t.enrolled => t,
        _ => Tie {
            lan,
            key,
            mailbox,
            enrolled: false,
            vouched: true,
        },
    };
    // Kept as it was: nothing to write.
    if was.as_ref() == Some(&tie) {
        return Ok(());
    }
    ties.ties.push(tie);
    keep_ties(vault, &held, &mut ties, now_s)
}

/// What a station bound to its account checks an enrolment against, as it kept it: no token.
pub(crate) struct Ready {
    pub(crate) tenant: &'static Tenant,
    pub(crate) owner: Owner,
    pub(crate) keys: KeySet,
    pub(crate) invites: Vec<Invite>,
    /// Where the LAN's device cache is kept.
    pub(crate) vault: Arc<dyn AccountVault>,
    /// The clock proofs are read by: the station's own.
    pub(crate) now_s: fn() -> u64,
}

/// Whether, and against what, the LAN takes enrolments now. Unarmed (the station is not bound, or
/// its owner or the issuer's keys are not kept) it takes none, and TLS takes no unknown key for one.
/// And the computers enrolled already, which every admission holds to their proofs' age.
#[derive(Default)]
pub(crate) struct Enrolment {
    ready: Mutex<Option<Arc<Ready>>>,
    /// Where the LAN's device cache is kept. `None` only in a test with no account: then nothing is
    /// enrolled, and nothing is held to an age.
    kept: Option<Kept>,
    /// The devices that cache holds, as last read: `None` until it has been read, and then nobody
    /// is admitted, as the book admits nobody until it has read its own.
    enrolled: Mutex<Option<Vec<CachedDevice>>>,
    /// The links and the removals kept beside it, as last read: `None` until read, and then nobody
    /// is admitted, since a removal may be among them.
    ties: Mutex<Option<Ties>>,
    /// Whether that did not read at all when last read.
    unreadable: AtomicBool,
    /// When a connection last read them again ([`Enrolment::retry`]).
    retried: Mutex<Option<Instant>>,
}

/// Where the LAN's device cache is kept, and the clock proofs are read by.
struct Kept {
    vault: Arc<dyn AccountVault>,
    now_s: fn() -> u64,
}

impl Enrolment {
    /// The enrolment for the station whose LAN device cache `vault` keeps, its proofs read by `now_s`.
    pub(crate) fn kept_in(vault: Arc<dyn AccountVault>, now_s: fn() -> u64) -> Self {
        Self {
            kept: Some(Kept { vault, now_s }),
            ..Self::default()
        }
    }

    /// Reads the device cache and what a Remove leaves behind, if either has not been read yet:
    /// before anything listens, before a connection is taken, and before a Remove at the shack or a
    /// list follows what it keeps.
    pub(crate) fn load(&self) {
        if !self.is_read() {
            self.reread();
        }
    }

    /// [`Enrolment::load`], for a connection the LAN took at `now`: at most once each
    /// [`RETRY_EVERY`], so no stream of connections keeps the credential store, or the log, busy.
    pub(crate) fn retry(&self, now: Instant) {
        if self.is_read() {
            return;
        }
        let Ok(mut last) = self.retried.lock() else {
            return;
        };
        if last.is_some_and(|at| now.saturating_duration_since(at) < RETRY_EVERY) {
            return;
        }
        *last = Some(now);
        drop(last);
        self.load();
    }

    /// Whether what a Remove leaves behind did not read at all when last read: the shack says so,
    /// with its way out, Reset network identity ([`Enrolment::recover`]).
    pub(crate) fn unreadable(&self) -> bool {
        self.unreadable.load(Ordering::SeqCst)
    }

    /// Whether both have been read: until then nobody is admitted.
    pub(crate) fn is_read(&self) -> bool {
        self.enrolled.lock().is_ok_and(|held| held.is_some())
            && self.ties.lock().is_ok_and(|held| held.is_some())
    }

    /// Reads the device cache again, after the station changed it. One that does not read leaves
    /// what was read before.
    pub(crate) fn reread(&self) {
        let Some(Kept { vault, .. }) = &self.kept else {
            return;
        };
        self.read(&**vault, &DevicesHeld::take(&**vault));
    }

    /// [`Enrolment::reread`], the cache held: so the copy in memory follows the store in the order
    /// its writers saved it, and never goes back to a read older than a save.
    fn read(&self, vault: &dyn AccountVault, _held: &DevicesHeld<'_>) {
        // Whatever the cache answers: a removal must not wait on it. A missing entry is none kept
        // (a station from before links were kept); one that does not read is never taken for it.
        match vault.ties() {
            Ok(ties) => {
                self.unreadable.store(false, Ordering::SeqCst);
                self.hold_ties(ties.unwrap_or_default());
            }
            // Said once, when it starts.
            Err("unreadable") if !self.unreadable.swap(true, Ordering::SeqCst) => {
                tempo_core::applog::warn(
                    "remote",
                    "on this network: the removed computers' list does not read; until it does, \
                     no paired computer is taken. Reset network identity writes it afresh",
                );
            }
            Err(_) => {}
        }
        let read = match vault.device_cache() {
            Ok(cache) => cache.map(|cache| cache.devices).unwrap_or_default(),
            Err(_) => {
                // Never read yet, nobody is admitted (`admits`): the cause of a refusal to say.
                let first = self.enrolled.lock().is_ok_and(|held| held.is_none());
                tempo_core::applog::warn(
                    "remote",
                    if first {
                        "on this network: the signed-in computers' list did not read; until it \
                         does, no paired computer is taken"
                    } else {
                        "on this network: the signed-in computers' list did not read again; the \
                         last one read stays"
                    },
                );
                return;
            }
        };
        self.hold(read);
    }

    fn hold(&self, devices: Vec<CachedDevice>) {
        if let Ok(mut held) = self.enrolled.lock() {
            *held = Some(devices);
        }
    }

    fn hold_ties(&self, ties: Ties) {
        if let Ok(mut held) = self.ties.lock() {
            *held = Some(ties);
        }
    }

    /// The links kept, as last read (see the header): what a Remove at the shack follows.
    pub(crate) fn ties(&self) -> Vec<Tie> {
        self.kept().map(|kept| kept.ties).unwrap_or_default()
    }

    /// The links, the removals and the floor kept, as last read: `None` until read, and then no
    /// proof may be granted on, since a removal may be among them.
    pub(crate) fn kept(&self) -> Option<Ties> {
        self.ties.lock().ok()?.clone()
    }

    /// Whether the key with `pin` was removed at the shack, as last read, and has not come back
    /// since: a computer still in the book with it is a removal the book's store did not keep, and
    /// is taken out of it (`super::Lan::expire`). Until read, none is known: [`Enrolment::admits`]
    /// admits nobody meanwhile, and nothing is taken out of the book on a guess.
    pub(crate) fn is_removed(&self, pin: &[u8; 32]) -> bool {
        let pin = short(&tempo_client::hex(pin));
        self.kept()
            .is_some_and(|kept| kept.removed.iter().any(|r| r.key == pin))
    }

    /// What a Remove leaves behind, its floor set to now: every proof signed before is refused (see
    /// the header), so nothing comes back on what it held. Reset network identity's, which takes
    /// every computer out of the book too. A floor, or a removal, kept while the station's clock ran
    /// ahead comes back to now, so it refuses nothing signed after the reset. An entry that does not
    /// read at all is written afresh; one the store will not answer for is left, and said. Blocks on
    /// the credential store.
    pub(crate) fn recover(&self) -> Result<(), &'static str> {
        let Some(Kept { vault, now_s }) = &self.kept else {
            return Ok(());
        };
        let now_s = now_s();
        let held = DevicesHeld::take(&**vault);
        let written = match vault.ties() {
            Ok(ties) => {
                let mut ties = ties.unwrap_or_default();
                ties.floor = now_s;
                let ahead = now_s.saturating_add(tempo_client::id_token::LEEWAY_S);
                ties.removed.retain(|r| r.at <= ahead);
                keep_ties(&**vault, &held, &mut ties, now_s)
            }
            Err("unreadable") => vault.save_ties(
                &held,
                &Ties {
                    floor: now_s,
                    ..Ties::default()
                },
            ),
            Err(reason) => Err(reason),
        };
        self.read(&**vault, &held);
        written
    }

    /// Keeps each of `keys` (pins or fingerprints, lowercase hex) as removed at the shack now, and
    /// drops every link that names one: before any road it is removed from ends, so no road comes
    /// back on a proof signed before (see the header), and a book that is not written after it
    /// admits it nowhere. Blocks on the credential store.
    pub(crate) fn remove(&self, keys: &[String]) -> Result<(), &'static str> {
        let Some(Kept { vault, now_s }) = &self.kept else {
            return Ok(());
        };
        if keys.is_empty() {
            return Ok(());
        }
        let now_s = now_s();
        let held = DevicesHeld::take(&**vault);
        let kept = vault.ties().and_then(|ties| {
            let mut ties = ties.unwrap_or_default();
            let keys: Vec<String> = keys.iter().map(|key| short(key)).collect();
            ties.ties
                .retain(|t| !keys.contains(&t.lan) && !keys.contains(&t.key));
            ties.removed.retain(|r| !keys.contains(&r.key));
            ties.removed
                .extend(keys.into_iter().map(|key| Removed { key, at: now_s }));
            keep_ties(&**vault, &held, &mut ties, now_s)
        });
        self.read(&**vault, &held);
        kept
    }

    /// Each of `keys` (as [`Enrolment::remove`] takes them) back from its removal at the shack: the
    /// operator approved its listing again at the shack, or it came back on a proof signed after
    /// its removal. Blocks on the credential store.
    pub(crate) fn unremove(&self, keys: &[String]) -> Result<(), &'static str> {
        let Some(Kept { vault, now_s }) = &self.kept else {
            return Ok(());
        };
        let keys: Vec<String> = keys.iter().map(|key| short(key)).collect();
        if self
            .kept()
            .is_some_and(|kept| !kept.removed.iter().any(|r| keys.contains(&r.key)))
        {
            return Ok(());
        }
        let held = DevicesHeld::take(&**vault);
        let kept = vault.ties().and_then(|ties| {
            let mut ties = ties.unwrap_or_default();
            ties.removed.retain(|r| !keys.contains(&r.key));
            keep_ties(&**vault, &held, &mut ties, now_s())
        });
        self.read(&**vault, &held);
        kept
    }

    /// The LAN keys (as [`short`] gives them) of the computers the guest with `mailbox` proved here
    /// as theirs, enrolling on their sign-in; and apart, of every guest's computer kept with no
    /// such link to say whose. An invitation taken back takes them all off the LAN (see the
    /// header). A listing's claim on a LAN key counts for neither: it says nothing of whose it is.
    pub(crate) fn guests(&self, mailbox: &str) -> (Vec<String>, Vec<String>) {
        let tag = mailbox_tag(mailbox);
        let proved: Vec<Tie> = self
            .ties()
            .into_iter()
            .filter(|t| t.enrolled && t.mailbox.is_some())
            .collect();
        let unknown: Vec<String> = self
            .enrolled
            .lock()
            .ok()
            .and_then(|held| held.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|d| d.guest)
            .map(|d| short(&d.pin))
            .filter(|pin| !proved.iter().any(|t| t.lan == *pin))
            .collect();
        let theirs = proved
            .into_iter()
            .filter(|t| t.mailbox.as_deref() == Some(tag.as_str()))
            .map(|t| t.lan)
            .collect();
        (theirs, unknown)
    }

    /// Whether the LAN admits the computer holding the key with `pin` now, as far as its enrolment
    /// goes: one the cache does not hold was paired by code, and the book alone decides; one it holds
    /// is admitted for [`account::PROOF_MAX_AGE_S`] from its proof's signing (see the header).
    pub(crate) fn admits(&self, pin: &[u8; 32]) -> bool {
        let Some(Kept { now_s, .. }) = &self.kept else {
            return true;
        };
        let Ok(held) = self.enrolled.lock() else {
            return false;
        };
        let enrolled = held.as_ref().is_some_and(|devices| {
            let pin = tempo_client::hex(pin);
            devices
                .iter()
                .find(|device| device.pin == pin)
                .is_none_or(|device| device.admits(now_s(), false))
        });
        drop(held);
        // Read, and not removed: until read, a removal may be among them.
        let read = self.ties.lock().is_ok_and(|ties| ties.is_some());
        enrolled && read && !self.is_removed(pin)
    }

    /// The pins of the enrolled computers whose proofs are past their age now, as last read.
    pub(super) fn expired(&self) -> Vec<[u8; 32]> {
        let Some(Kept { now_s, .. }) = &self.kept else {
            return Vec::new();
        };
        let now_s = now_s();
        let Ok(held) = self.enrolled.lock() else {
            return Vec::new();
        };
        held.iter()
            .flatten()
            .filter(|device| !device.admits(now_s, false))
            .filter_map(|device| super::pairing::hex32(&device.pin))
            .collect()
    }

    /// Takes the computers with `pins` out of the device cache, once they are out of the book: each
    /// only while its proof is still past its age, since one enrolled again meanwhile is in the book
    /// again, held to its new proof (see the header). A store that will not keep it leaves them
    /// there, still refused.
    pub(super) fn forget(&self, pins: &[[u8; 32]]) {
        let Some(Kept { vault, now_s }) = &self.kept else {
            return;
        };
        let held = DevicesHeld::take(&**vault);
        if let Ok(Some(mut cache)) = vault.device_cache() {
            let now_s = now_s();
            for pin in pins {
                let pin = tempo_client::hex(pin);
                let within_age = cache.find(&pin).is_some_and(|d| d.admits(now_s, false));
                if !within_age {
                    cache.forget(&pin);
                }
            }
            let _ = vault.save_device_cache(&held, &cache);
        }
        self.read(&**vault, &held);
    }

    /// The computer with `pin` was just paired with a code at the shack, and the book keeps that
    /// pairing: it is taken out of the device cache, so no sign-in's age holds it from now on, as
    /// for every computer paired by code (see the header). A cache that cannot be read, or will not
    /// keep the change, leaves it held to its proof's age: `unavailable`, which the pairing says.
    pub(super) fn paired_by_code(&self, pin: &[u8; 32]) -> Result<(), &'static str> {
        let Some(Kept { vault, .. }) = &self.kept else {
            return Ok(());
        };
        let pin = tempo_client::hex(pin);
        let held = DevicesHeld::take(&**vault);
        let taken = vault.device_cache().and_then(|cache| match cache {
            Some(mut cache) if cache.find(&pin).is_some() => {
                cache.forget(&pin);
                vault.save_device_cache(&held, &cache)
            }
            _ => Ok(()),
        });
        // Nor any link, nor a removal: the pairing at the shack is the operator's word on it now.
        let short = short(&pin);
        let untied = vault.ties().and_then(|ties| match ties {
            Some(mut ties)
                if ties.ties.iter().any(|t| t.lan == short)
                    || ties.removed.iter().any(|r| r.key == short) =>
            {
                ties.ties.retain(|t| t.lan != short);
                ties.removed.retain(|r| r.key != short);
                let now_s = self.kept.as_ref().map_or(0, |kept| (kept.now_s)());
                keep_ties(&**vault, &held, &mut ties, now_s)
            }
            _ => Ok(()),
        });
        self.read(&**vault, &held);
        taken.and(untied).map_err(|_| "unavailable")
    }

    /// `paired` (who the book admits), held to [`Enrolment::admits`]: what the LAN's TLS and its
    /// sessions ask.
    pub(super) fn bounded(self: &Arc<Self>, paired: tls::Paired) -> tls::Paired {
        let enrolment = self.clone();
        Arc::new(move |pin: &[u8; 32]| paired(pin).filter(|_| enrolment.admits(pin)))
    }

    /// Armed with `ready`, or disarmed with `None`.
    pub(crate) fn arm(&self, ready: Option<Ready>) {
        if let Ok(mut held) = self.ready.lock() {
            *held = ready.map(Arc::new);
        }
    }

    /// Whether an enrolment can be checked now: what TLS asks of an unknown key.
    pub(crate) fn open(&self) -> bool {
        self.ready.lock().is_ok_and(|ready| ready.is_some())
    }

    /// What TLS asks of a key no paired computer holds: whether `pairing` (the window) is open, or
    /// this enrolment is.
    pub(super) fn or_pairing(self: &Arc<Self>, pairing: tls::Pairing) -> tls::Pairing {
        let enrolment = self.clone();
        Arc::new(move || pairing() || enrolment.open())
    }

    /// The people it takes, while armed: for a test to read.
    #[cfg(test)]
    pub(crate) fn invites(&self) -> Option<Vec<Invite>> {
        self.ready().map(|ready| ready.invites.clone())
    }

    fn ready(&self) -> Option<Arc<Ready>> {
        self.ready.lock().ok()?.clone()
    }

    /// The time now as proofs are read, while armed.
    fn now_s(&self) -> Option<u64> {
        self.ready().map(|ready| (ready.now_s)())
    }
}

/// What an enrolment's first message asked, with the key its TLS session proved.
pub(super) struct Asked {
    name: String,
    kind: String,
    /// Its other key (for the internet road), SPKI, as the message named it.
    public_key: Vec<u8>,
    proof: String,
    /// The key this session proved, SPKI: the LAN key its proof must name.
    key: Vec<u8>,
}

impl Asked {
    /// `None` for anything out of shape: a name, a kind that is not a computer or a phone, a key
    /// that is not a P-256 key, or a proof empty or too long.
    pub(super) fn new(
        name: &str,
        kind: &str,
        public_key: &str,
        proof: &str,
        key: Vec<u8>,
    ) -> Option<Self> {
        if !valid_name(name)
            || !matches!(kind, "computer" | "phone")
            || !tempo_stream::protocol::device_key(public_key)
            || proof.is_empty()
            || proof.len() > PROOF_CHARS
        {
            return None;
        }
        Some(Self {
            name: name.trim().to_string(),
            kind: kind.to_string(),
            public_key: tempo_stream::protocol::hex_bytes(public_key)?,
            proof: proof.to_string(),
            key,
        })
    }
}

/// Admits the computer `asked` names, under the LAN station id `under`, at `now_s`: its proof
/// checked (see the header), its key kept in the device cache and then paired in the book, the
/// cache held from its read until the book keeps the pairing. Its device id. Blocks on the
/// credential store.
pub(super) fn admit(
    book: &Book,
    enrolment: &Enrolment,
    under: &str,
    asked: &Asked,
    now_s: u64,
) -> Result<String, &'static str> {
    let ready = enrolment.ready().ok_or("enrolClosed")?;
    let vouched = account::vouched(
        &asked.proof,
        &asked.kind,
        &[&asked.public_key, &asked.key],
        &ready.keys,
        ready.tenant,
        &ready.owner,
        &ready.invites,
        now_s,
    )?;
    if vouched.role != Role::Operate {
        return Err("listenNative");
    }
    let pin = tls::pin(&asked.key);
    let (lan, key) = (
        short(&tempo_client::hex(&pin)),
        short(&tempo_client::hex(
            ring::digest::digest(&ring::digest::SHA256, &asked.public_key).as_ref(),
        )),
    );
    // The cache first: a key the cache does not hold is never taken out of the book by a list. Held
    // until the book keeps the pairing, so no list's sweep lands between the two.
    let station_id = &ready.owner.station_id;
    let held = DevicesHeld::take(&*ready.vault);
    // Removed at the shack since this proof was signed: it signs in again (see the header).
    let mut ties = ready.vault.ties()?.unwrap_or_default();
    if removed_by(&ties.removed, &[&lan, &key], vouched.issued_at)
        || floored(&ties, vouched.issued_at)
    {
        return Err("tokenRemoved");
    }
    let mut cache = ready
        .vault
        .device_cache()?
        .filter(|cache| cache.station_id == *station_id)
        .unwrap_or_else(|| DeviceCache::new(station_id));
    cache.keep(CachedDevice {
        pin: tempo_client::hex(&pin),
        role: vouched.role,
        guest: vouched.guest,
        issued_at: vouched.issued_at,
        // Not yet heard from the service.
        confirmed_at: 0,
    })?;
    ready.vault.save_device_cache(&held, &cache)?;
    enrolment.hold(cache.devices);
    // Its link, proven by this very TLS session for the LAN key: whatever was kept for that key
    // before, the checked listing's side kept only for the same internet key and the same person.
    let mailbox = vouched.mailbox.as_deref().map(mailbox_tag);
    let was = ties.ties.iter().position(|t| t.lan == lan);
    let vouched_side = was
        .map(|at| ties.ties.remove(at))
        .is_some_and(|t| t.key == key && t.mailbox == mailbox && t.vouched);
    ties.removed.retain(|r| r.key != lan);
    ties.ties.push(Tie {
        lan,
        key,
        mailbox,
        enrolled: true,
        vouched: vouched_side,
    });
    keep_ties(&*ready.vault, &held, &mut ties, now_s)?;
    enrolment.hold_ties(ties);
    book.add(pin, &asked.name, under)
}

/// Whether a refusal is the computer's own doing (its proof), which counts against its address.
fn counted(reason: &str) -> bool {
    reason.starts_with("token")
        || matches!(
            reason,
            "proofNotYours" | "ownerIssuer" | "deviceKind" | "listenNative"
        )
}

/// One enrol-only connection, from its first message to its close, inside `deadline`.
pub(super) async fn session<S>(
    mut socket: Socket<S>,
    asked: Asked,
    desk: Desk,
    deadline: Duration,
    mut stop: watch::Receiver<bool>,
) -> Outcome
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let outcome = tokio::select! {
        biased;
        _ = stop.changed() => Outcome::Turned,
        done = tokio::time::timeout(deadline, enrol(&mut socket, asked, &desk)) => {
            done.unwrap_or(Outcome::Failed)
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
    outcome
}

async fn enrol<S>(socket: &mut Socket<S>, asked: Asked, desk: &Desk) -> Outcome
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (book, enrolment, under) = (
        desk.book.clone(),
        desk.enrolment.clone(),
        desk.station_id.clone(),
    );
    // The store can block: off this connection's thread.
    let admitted = tokio::task::spawn_blocking(move || {
        let now_s = enrolment.now_s().ok_or("enrolClosed")?;
        admit(&book, &enrolment, &under, &asked, now_s)
    })
    .await;
    match admitted {
        Ok(Ok(device)) => {
            say(
                socket,
                json!({"type":"enrolled","deviceId":device,"stationId":desk.station_id}),
            )
            .await;
            Outcome::Paired
        }
        Ok(Err(reason)) if counted(reason) => refuse(socket, reason, Outcome::Failed).await,
        Ok(Err(reason)) => refuse(socket, reason, Outcome::Turned).await,
        Err(_) => refuse(socket, "unavailable", Outcome::Turned).await,
    }
}
