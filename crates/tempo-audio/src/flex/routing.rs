//! Where a Flex's transmitter takes its audio from while Nexus's own client serves it: the DAX
//! source flag (`transmit set dax`), decided as the operator ruled, and the memory that puts the
//! operator's own setting back.
//!
//! ## The ruling (operator, 2026-10-03, "Follow the slice's mode")
//! - When Nexus is the only program feeding the radio's transmit audio, follow the TX slice's
//!   mode: DAX for the digital modes and for the stream's browser voice, and the radio's own mic
//!   for Phone at the shack.
//! - Write the flag when the mode or the TX slice changes, never between the slot boundary and
//!   `xmit 1`.
//! - Restore the operator's own setting on disconnect and at the next connect.
//! - When SmartSDR's DAX is also connected, never write the flag.
//!
//! It replaces the 2026-07-26 ruling that one toggle means DAX both ways. That ruling governed the
//! older native audio worker on SmartSDR CAT, retired 2026-10-04 (operator ruling, "Retire it"):
//! native audio now rides this client only.
//!
//! ## How each part holds
//! - **Follow the mode.** [`wanted_source`] maps the TX slice's mode to the source: the digital
//!   modes ([`mode_class`]) to DAX, the phone modes to the mic, or to DAX while the stream's
//!   browser voice is live (a streamed operator's transmit presence); CW, FreeDV and anything
//!   else leave the operator's setting alone. Only while Nexus's native audio is on: with it off,
//!   Nexus feeds nothing over DAX and the flag goes back to the operator's.
//! - **On a change only.** [`Routing`] decides when its inputs change (the TX slice, its mode,
//!   the browser voice, native audio on or off) and writes once per decision, so a flag the
//!   operator flips by hand is not fought. A write the radio never reports back within
//!   [`CONFIRM_MS`] (refused, or lost) is the same decision unfinished, and is tried again.
//! - **Never in the keying path.** Nothing here writes: [`Routing::step`] returns what to write,
//!   and the radio loop asks for it only at the end of a tick in which nothing is keyed or about
//!   to be (`crate::service`, the quiet point). The protocol core's admission refuses the write
//!   while anything is keyed or the interlock is not idle, at the wire.
//! - **Never beside SmartSDR's DAX.** A DAX transmit stream of another program's
//!   ([`tempo_net::flex::admission::another_dax_feeder`]) stops every write here, and admission
//!   refuses it too.
//! - **Restore.** The operator's setting is the radio's `transmit dax` as first seen, before
//!   Nexus wrote anything. While Nexus's write leaves the radio on another value, that setting is
//!   kept in a [`Memory`]: in process for a session that dies and is replaced, and in a small file
//!   beside the app's diagnostic log for a crash. A clean disconnect writes it back
//!   ([`Routing::restore_on_disconnect`]) unless the operator has changed the flag since; the next
//!   connect to the same radio writes it back first if it was not ([`Routing::step`]).
//! - **Never a silent over.** Native audio turned off inside the guard before a boundary leaves
//!   the radio on the DAX Nexus wrote until the next quiet point, with nothing feeding it
//!   ([`Routing::leaves_dax`]); the shim refuses a digital key until the operator's setting is
//!   back (`super::shim`).
//!
//! Nexus's own design, not a port.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

/// How long the radio has to report a write back before it is tried again.
pub const CONFIRM_MS: u64 = 2_000;

/// What a mode asks of the transmitter's audio source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeClass {
    /// Audio from the computer: DAX.
    Digital,
    /// Voice: the radio's mic at the shack, DAX for the stream's browser voice.
    Phone,
    /// The radio makes the signal itself (CW, the FreeDV waveform) or the mode is not known: the
    /// operator's setting is left alone.
    Other,
}

/// The class of a Flex mode word. Digital follows the modes AetherSDR switches to DAX (its
/// `updateDaxTxMode`, read as a fact), less `NFM` (a voice mode here) and `NT` (not a documented
/// mode).
pub fn mode_class(mode: &str) -> ModeClass {
    match mode.trim().to_ascii_uppercase().as_str() {
        "DIGU" | "DIGL" | "RTTY" | "DFM" => ModeClass::Digital,
        "USB" | "LSB" | "AM" | "SAM" | "FM" | "NFM" | "DSB" => ModeClass::Phone,
        _ => ModeClass::Other,
    }
}

/// The source the ruling asks for: `Some(true)` DAX, `Some(false)` the mic, `None` leave it.
pub fn wanted_source(mode: Option<&str>, browser_voice: bool) -> Option<bool> {
    match mode.map(mode_class)? {
        ModeClass::Digital => Some(true),
        ModeClass::Phone => Some(browser_voice),
        ModeClass::Other => None,
    }
}

/// What the radio reports, read for one decision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    /// `transmit dax` as last reported.
    pub radio: Option<bool>,
    /// The transmit slice, when it is ours.
    pub tx_slice: Option<u8>,
    /// Its mode: the one Nexus last commanded while the radio has not yet echoed it, else the
    /// radio's.
    pub tx_mode: Option<String>,
    /// Another program feeds the radio's DAX transmit audio.
    pub other_feeder: bool,
    /// Our DAX transmit stream exists.
    pub dax_tx_stream: bool,
}

/// What the routing wants written next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// `transmit set dax=<value>`, and why.
    Write { dax: bool, why: Why },
    /// `stream create type=dax_tx`: DAX is wanted and our transmit stream does not exist yet,
    /// whether or not the radio already takes DAX.
    CreateDaxTx,
}

/// Why a write is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// A previous session of ours changed the flag and did not put it back.
    RestoreAtConnect,
    /// The TX slice's mode (or the browser voice) asks for it.
    FollowMode,
    /// Native audio went off: the operator's own setting comes back.
    NativeAudioOff,
}

/// What the end of a connection owes the operator's setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restore {
    /// Nothing: Nexus left the flag as it found it, or the operator has changed it since.
    Nothing,
    /// Put this value back now.
    Write(bool),
    /// It cannot be put back now (the flag is unknown, or another program feeds DAX): keep the
    /// memory, so the next connect does it.
    Later,
}

/// The inputs a decision is made from. A change of any of them is "the mode or the TX slice
/// changes".
#[derive(Debug, Clone, PartialEq, Eq)]
struct Inputs {
    tx_slice: Option<u8>,
    mode: Option<String>,
    browser_voice: bool,
    native_audio: bool,
}

/// The routing state of one connection.
#[derive(Debug, Default)]
pub struct Routing {
    /// The operator's own setting: the radio's flag before Nexus first wrote it, on this
    /// connection or a previous one.
    operator: Option<bool>,
    /// A previous session left the flag changed: put this back first.
    restore: Option<bool>,
    /// The last decision's inputs, and whether it is carried out.
    decided: Option<Inputs>,
    applied: bool,
    /// What Nexus last wrote on this connection.
    wrote: Option<bool>,
    /// A write not yet reported back by the radio, and when it went.
    unconfirmed: Option<(bool, u64)>,
}

impl Routing {
    /// A connection's routing. `remembered` is the operator's setting a previous session of ours
    /// left changed on this radio, if any.
    pub fn new(remembered: Option<bool>) -> Routing {
        Routing {
            operator: remembered,
            restore: remembered,
            ..Routing::default()
        }
    }

    /// The operator's own setting, as far as it is known.
    pub fn operator(&self) -> Option<bool> {
        self.operator
    }

    /// What to write now, if anything, at `now_ms` on any monotonic clock. Call it only at a
    /// quiet point; carry out at most one step per call and report a written flag with
    /// [`Routing::written`].
    pub fn step(
        &mut self,
        view: &View,
        native_audio: bool,
        browser_voice: bool,
        now_ms: u64,
    ) -> Option<Step> {
        if view.other_feeder {
            return None;
        }
        let radio = view.radio?;
        if let Some(back) = self.restore {
            if radio == back {
                self.restore = None;
            } else {
                return Some(Step::Write {
                    dax: back,
                    why: Why::RestoreAtConnect,
                });
            }
        }
        if self.operator.is_none() {
            self.operator = Some(radio);
        }
        // A write in flight: the radio's flag is about to change, so nothing is decided against
        // the value it still reports. Never reported back in time: refused or lost, and the
        // decision it carried is not done.
        if let Some((dax, at)) = self.unconfirmed {
            if radio == dax {
                self.unconfirmed = None;
            } else if now_ms.saturating_sub(at) < CONFIRM_MS {
                return None;
            } else {
                self.unconfirmed = None;
                self.applied = false;
            }
        }
        let inputs = Inputs {
            tx_slice: view.tx_slice,
            mode: view.tx_mode.clone(),
            browser_voice,
            native_audio,
        };
        if self.decided.as_ref() != Some(&inputs) {
            self.decided = Some(inputs);
            self.applied = false;
        }
        if self.applied {
            return None;
        }
        // The flag is radio-wide: written only while the transmit slice is ours.
        if view.tx_slice.is_none() {
            self.applied = true;
            return None;
        }
        let (target, why) = if native_audio {
            match wanted_source(view.tx_mode.as_deref(), browser_voice) {
                Some(dax) => (dax, Why::FollowMode),
                None => {
                    self.applied = true;
                    return None;
                }
            }
        } else {
            match (self.wrote, self.operator) {
                // Only what Nexus changed goes back.
                (Some(w), Some(op)) if w != op => (op, Why::NativeAudioOff),
                _ => {
                    self.applied = true;
                    return None;
                }
            }
        };
        // DAX for Nexus's own audio: its transmit stream first, whatever the flag says now. A
        // radio already on DAX (SmartSDR's own DAX switch sets the same radio-wide flag) still
        // needs it, or nothing carries the over.
        if native_audio && target && !view.dax_tx_stream {
            return Some(Step::CreateDaxTx);
        }
        if target == radio {
            self.applied = true;
            return None;
        }
        if target && !view.dax_tx_stream {
            return Some(Step::CreateDaxTx);
        }
        Some(Step::Write { dax: target, why })
    }

    /// A write of `dax` went to the radio at `now_ms`. Returns the operator's setting to remember
    /// for this radio (Nexus left it changed), or `None` to forget it (the radio is back on the
    /// operator's).
    pub fn written(&mut self, dax: bool, why: Why, now_ms: u64) -> Option<bool> {
        self.wrote = Some(dax);
        self.unconfirmed = Some((dax, now_ms));
        match why {
            Why::RestoreAtConnect => {
                self.restore = None;
                None
            }
            Why::FollowMode | Why::NativeAudioOff => {
                self.applied = true;
                self.operator.filter(|op| *op != dax)
            }
        }
    }

    /// What this connection's end owes the operator's setting: the flag back, if Nexus left it
    /// changed and the operator has not changed it since.
    pub fn restore_on_disconnect(&self, view: &View) -> Restore {
        if view.other_feeder {
            return Restore::Later;
        }
        if let Some(back) = self.restore {
            // Never got the chance to restore at connect: still owed.
            return match view.radio {
                Some(r) if r == back => Restore::Nothing,
                Some(_) => Restore::Write(back),
                None => Restore::Later,
            };
        }
        let (Some(wrote), Some(op)) = (self.wrote, self.operator) else {
            return Restore::Nothing;
        };
        if wrote == op {
            return Restore::Nothing;
        }
        // A write still in flight stands as written: the flag reported is about to change.
        let radio = match self.unconfirmed {
            Some((dax, _)) => Some(dax),
            None => view.radio,
        };
        match radio {
            Some(r) if r == wrote => Restore::Write(op),
            // The operator moved it after us: their latest choice stands.
            Some(_) => Restore::Nothing,
            None => Restore::Later,
        }
    }

    /// Whether Nexus's own write (this connection's, or one a previous session left) has the
    /// radio taking its transmit audio from DAX in place of the operator's own setting, its mic:
    /// what a disconnect now would put back. With native audio off nothing feeds that DAX, so a
    /// digital over keyed then would go out silent (`super::shim`).
    pub fn leaves_dax(&self, view: &View) -> bool {
        self.restore_on_disconnect(view) == Restore::Write(false)
    }
}

/// Where the operator's setting is kept while Nexus has it changed, by radio.
pub trait Memory: Send + Sync {
    fn recall(&self, radio: &str) -> Option<bool>;
    /// `Some` to remember, `None` to forget.
    fn keep(&self, radio: &str, operator: Option<bool>);
}

/// The production memory: in process, and in a file when there is a place for one.
pub struct FileMemory {
    path: Option<PathBuf>,
    entries: Mutex<Option<BTreeMap<String, bool>>>,
}

/// The file's name, beside the app's diagnostic log.
pub const MEMORY_FILE: &str = "flex-tx-audio.txt";

impl FileMemory {
    /// Kept at `path` (and in process); `None` keeps it in process only.
    pub fn new(path: Option<PathBuf>) -> FileMemory {
        FileMemory {
            path,
            entries: Mutex::new(None),
        }
    }

    /// The app's: beside its diagnostic log, when the log is open.
    pub fn app() -> &'static FileMemory {
        static APP: std::sync::OnceLock<FileMemory> = std::sync::OnceLock::new();
        APP.get_or_init(|| {
            FileMemory::new(
                tempo_core::applog::path()
                    .and_then(|p| p.parent())
                    .map(|dir| dir.join(MEMORY_FILE)),
            )
        })
    }

    fn with<T>(&self, f: impl FnOnce(&mut BTreeMap<String, bool>) -> T) -> T {
        let mut guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entries = guard.get_or_insert_with(|| {
            self.path
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|text| parse(&text))
                .unwrap_or_default()
        });
        f(entries)
    }
}

/// `<radio> <0|1>` per line; anything else is skipped.
fn parse(text: &str) -> BTreeMap<String, bool> {
    text.lines()
        .filter_map(|line| {
            let (radio, value) = line.trim().rsplit_once(' ')?;
            let value = match value {
                "0" => false,
                "1" => true,
                _ => return None,
            };
            (!radio.trim().is_empty()).then(|| (radio.trim().to_string(), value))
        })
        .collect()
}

impl Memory for FileMemory {
    fn recall(&self, radio: &str) -> Option<bool> {
        self.with(|e| e.get(radio).copied())
    }

    fn keep(&self, radio: &str, operator: Option<bool>) {
        self.with(|e| {
            let changed = match operator {
                Some(v) => e.insert(radio.to_string(), v) != Some(v),
                None => e.remove(radio).is_some(),
            };
            if !changed {
                return;
            }
            if let Some(path) = &self.path {
                let text: String = e
                    .iter()
                    .map(|(r, v)| format!("{r} {}\n", u8::from(*v)))
                    .collect();
                if let Err(err) = std::fs::write(path, text) {
                    tempo_core::applog::warn(
                        "cat",
                        &format!("Flex client: could not keep the DAX source setting ({err})"),
                    );
                }
            }
        });
    }
}

#[cfg(test)]
mod tests;
