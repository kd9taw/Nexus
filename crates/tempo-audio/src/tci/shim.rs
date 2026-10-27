//! The rigctld shim for a TCI radio: [`TciShim`] serves the session's transceiver as the radio's
//! dial, answering reads from the session's model of the radio (the server's own reports) and
//! turning each write into ONE typed command, done only once the server reports it. Every byte on
//! the rigctld wire is still produced by [`crate::rigctld_server::handle_command`], the encoder every
//! backend shares.
//!
//! ## The verb map
//!
//! | rigctld | TCI | Gate |
//! |---|---|---|
//! | `F <hz>` / `f` | `vfo:<trx>,0,<hz>;` / the model's VFO A | within the server's `vfo_limits`, VFO A not locked; done once the server reports it |
//! | `M <mode> <width>` / `m` | `modulation:<trx>,<mode>;`, then `rx_filter_band:<trx>,<lo>,<hi>;` for a width | a name the server's `modulations_list` offers (`PKTUSB` is `digu`, `PKTLSB` `digl`, `CW` `cw` or `cwu`, `CWR` `cwl` or `cwr`); a width placed on the mode's side; each done once reported |
//! | `T 1` (any non-zero) | `trx:<trx>,true,tci;` for a digital over, the voice keyer and Remote's voice; `trx:<trx>,true;` for Phone at the shack | every engine gate first; the mode the radio loop asked for decides, and is put back first when the server reports another; Nexus's audio only while its route is up; then the session's admission, with the drive Nexus set put back first when the server reports more |
//! | `T 0` | `trx:<trx>,false;` (`tune:<trx>,false;` first for a tune of ours) | never gated |
//! | `t` | the model's `trx` and `tune`, whoever keyed | — |
//! | `S <0/1> <vfo>` / `s` | `split_enable:<trx>,<bool>;` / the model | done once reported |
//! | `I <hz>` | `vfo:<trx>,1,<hz>;` (VFO B) | as `F` |
//! | `U RIT`/`U XIT <0/1>` / `u RIT`/`u XIT` | `rit_enable`/`xit_enable:<trx>,<bool>;` / the model | done once reported |
//! | `J <hz>` / `Z <hz>` | `rit_offset`/`xit_offset:<trx>,<hz>;` | done once reported |
//! | `L RFPOWER <0..1>` / `l RFPOWER` | `drive:<trx>,<0–100>;` (`drive:<v>;` in TCI 1.x) / the model | the engine's ceiling decides the value, and it becomes the ceiling admission holds the server's drive to |
//! | `l STRENGTH` | `rx_channel_sensors`, `rx_sensors` or the polled `rx_smeter`: dBm as dB from S9 at the dial | — |
//! | `l SWR`, `l RFPOWER_METER_WATTS` | `tx_sensors` (its SWR, and its peak watts as WSJT-X reads them), else the older `tx_swr`, `tx_power` | — |
//! | `\dump_caps`, `\dump_state` | authored ([`authored_caps`]): the server's `vfo_limits` as the receive range | — |
//!
//! Everything else answers as the shared encoder answers a backend that lacks it (`RPRT -11`),
//! byte for byte. CW (`b`, `\stop_morse`, `L KEYSPD`) is the SDR program's keyer's, a later change;
//! Tune is typed calls on the daemon ([`super::TciDaemon::tune_on`]).
//!
//! ## A write is done when the server says so
//! TCI has no reply to a write: the server broadcasts every change it takes to every client
//! (TCI 2.0 §3.1, §3.5), and it ignores a write inside another client's 200 ms hold, a band change
//! makes it restore the band's own settings over one, and a slow server reports late. So each write
//! waits, bounded ([`READBACK_WAIT`]), for the server's report of the value it asked for, and only
//! that value: a late report of an earlier write is not this one's answer. No report in time is the
//! radio not answering (`RPRT -5`), which the radio loop words, counts and retries as for any rig,
//! never a write taken. A write the server could not take (outside its limits, a mode it does not
//! offer, a locked VFO) is refused before anything is sent (`RPRT -1`).
//!
//! ## Which source a key takes
//! A key either names Nexus's audio as its source or names none, which the SDR program takes as its
//! own selected microphone (TCI 2.0 §4.2). Which one follows the operator's rule for the Flex client
//! (ruling 2026-10-03, "Follow the slice's mode"): Nexus's audio for a digital over, a voice-keyer
//! message and Remote's voice ([`super::TciDaemon::set_voice_key`]), when the radio's TCI audio is on;
//! the microphone for Phone at the shack, and for everything when it is off (the operator then routes
//! Nexus's audio to the SDR program some other way). A key in CW, or in a mode Nexus does not
//! transmit in, is refused.
//!
//! The class is that of the mode the radio loop last asked for, not only the one the server
//! reports: moving the dial to another band makes ExpertSDR restore that band's own mode and drive
//! (§3.5), and a digital over keyed on a restored phone mode would go out on the SDR's microphone.
//! So before a key the shim puts the asked-for mode back when the server reports another, and the
//! drive Nexus last set when admission finds the server's above it, each bounded
//! ([`REASSERT_WAIT`]) so the key still answers inside the radio
//! loop's PTT deadline. Only a value Nexus itself asked for is ever put back, and the drive only
//! ever down, to Nexus's own. A key the shim keeps off the air sends nothing and says why
//! ([`super::TciDaemon::key_refused_since`]): the rigctld answer, `RPRT -1`, carries no reason.
//!
//! Nexus's own design, not a port.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use tempo_net::tci::admission::{Refusal, TxStart};
use tempo_net::tci::model::Reading;
use tempo_net::tci::session::{Connection, Phase, SendError, Snapshot, StopOutcome};
use tempo_net::tci::wire::{Command, Flag, Hz, Modes, Setting, Word};
use tempo_net::tci::{Channel, Trx};

use crate::baud_ladder::{RigCaps, SplitDetect};
use crate::rigctld_server::{RigBackend, SetFault};

use super::ClientState;

/// How long a write waits for the server to report it: well inside the radio loop's own CAT
/// deadline, so a server that does not take it gets an answer of its own, not a dropped CAT
/// connection.
pub(crate) const READBACK_WAIT: Duration = Duration::from_millis(500);

/// How long each value put back before a key (the mode, the drive) waits for the server's report:
/// both, and the key, fit inside the radio loop's 700 ms PTT deadline.
pub(crate) const REASSERT_WAIT: Duration = Duration::from_millis(250);

/// How long a wait holds the session before it looks again whether the daemon is closing: a wait
/// never keeps a closing session open longer than this.
const SLICE: Duration = Duration::from_millis(20);

/// The words for a key or a write once the session is gone.
const CLOSED: &str = "the connection to the SDR program is closed";

/// A rigctld mode word as the server's: the data modes have TCI names of their own, CW is the upper
/// side where a server names both, FM is whichever FM the server offers, and the rest are the same
/// word. `None` when the server does not offer it (its `modulations_list`; the first name when it
/// has sent none; none at all when its list could not be read): refused, never approximated.
pub(crate) fn tci_mode_for(rigctld: &str, offered: &Reading<Modes>) -> Option<Word> {
    let candidates: &[&str] = match rigctld.trim().to_ascii_uppercase().as_str() {
        "PKTUSB" => &["digu"],
        "PKTLSB" => &["digl"],
        "USB" => &["usb"],
        "LSB" => &["lsb"],
        "CW" => &["cw", "cwu"],
        "CWR" => &["cwl", "cwr"],
        "AM" => &["am"],
        "SAM" => &["sam"],
        "DSB" => &["dsb"],
        "FM" => &["fm", "nfm", "fmn"],
        "WFM" => &["wfm"],
        _ => return None,
    };
    let word = match offered {
        Reading::Known(modes) => candidates
            .iter()
            .find(|c| modes.as_slice().iter().any(|m| m.as_str() == **c))?,
        Reading::NotReported => candidates.first()?,
        Reading::Unreadable => return None,
    };
    Word::new(word)
}

/// A server's mode as the rigctld word Nexus uses for every radio; the server's own word, in
/// capitals, where rigctld has none.
pub(crate) fn rigctld_mode(tci: &str) -> String {
    match tci {
        "digu" => "PKTUSB".to_string(),
        "digl" => "PKTLSB".to_string(),
        "cwu" => "CW".to_string(),
        "cwl" => "CWR".to_string(),
        "nfm" | "fmn" => "FM".to_string(),
        other => other.to_ascii_uppercase(),
    }
}

/// What kind of over a mode carries, for the key's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    /// DIGU, DIGL: Nexus's own audio.
    Digital,
    /// SSB, AM, FM: a voice, from the SDR's microphone or Nexus's voice keyer.
    Voice,
    /// CW: the SDR program's keyer, which this client does not drive yet.
    Cw,
    /// Anything else the server names (DRM, SPEC …): Nexus does not transmit in it.
    Other,
}

pub(crate) fn class(tci: &str) -> Class {
    match tci {
        "digu" | "digl" => Class::Digital,
        "usb" | "lsb" | "am" | "sam" | "dsb" | "fm" | "nfm" | "fmn" | "wfm" => Class::Voice,
        "cw" | "cwu" | "cwl" | "cwr" => Class::Cw,
        _ => Class::Other,
    }
}

/// Which side of the dial a mode's passband lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Upper,
    Lower,
    Centred,
}

fn side(tci: &str) -> Option<Side> {
    match tci {
        "usb" | "digu" => Some(Side::Upper),
        "lsb" | "digl" => Some(Side::Lower),
        "cw" | "cwu" | "cwl" | "cwr" | "am" | "sam" | "dsb" | "fm" | "nfm" | "fmn" | "wfm" => {
            Some(Side::Centred)
        }
        _ => None,
    }
}

/// The filter edges (Hz from the dial) for a `width_hz` passband in `mode`, from the mode and edges
/// the receiver had.
///
/// TCI's filter is two edges (`rx_filter_band`) and rigctld sends a width, so one edge has to be
/// chosen. Where the receiver already sits on the same side, its own edge stays where the operator
/// put it (the low edge on the upper side, the high edge on the lower side, the centre for a
/// centred mode); otherwise the edge is the dial itself. `None` for a mode with no side to place a
/// width on, or a width that is not one. The Flex client's rule (`crate::flex::shim::filter_for`).
pub(crate) fn filter_for(
    mode: &str,
    width_hz: u32,
    current_mode: Option<&str>,
    current: Option<(i64, i64)>,
) -> Option<(i64, i64)> {
    let width = i64::from(width_hz);
    if !(1..=100_000).contains(&width) {
        return None;
    }
    let side = side(mode)?;
    let anchor = current.filter(|_| current_mode.and_then(self::side) == Some(side));
    Some(match side {
        Side::Upper => {
            let low = anchor.map_or(0, |(low, _)| low);
            (low, low + width)
        }
        Side::Lower => {
            let high = anchor.map_or(0, |(_, high)| high);
            (high - width, high)
        }
        Side::Centred => {
            let centre = anchor.map_or(0, |(low, high)| (low + high) / 2);
            let low = centre - width / 2;
            (low, low + width)
        }
    })
}

/// A TCI radio's capabilities, AUTHORED rather than read out of Hamlib:
/// - **receive coverage**: the server's own `vfo_limits` (TCI 2.0 §4.1), or none known;
/// - **split not readable for the privilege gate**: VFO B is where `I` writes the split
///   frequency, but servers map a transceiver's second channel differently (AetherSDR's notes),
///   so a read of it is not trusted to grant permission until a bench shows it;
/// - **drive floor 0**: `drive` runs 0–100, so a reading of zero is a real zero;
/// - **native VFO read**: the VFO this shim answers is the server's report, never a cache.
pub fn authored_caps(rx_coverage: Vec<(u64, u64)>) -> RigCaps {
    RigCaps {
        serial_rates: None,
        rx_coverage,
        split_detect: SplitDetect::Absent,
        rfpower_floor_milli: Some(0),
        vfo_read_native: true,
    }
}

/// A 0..1 level as TCI's 0–100.
fn percent(value: &str) -> Option<u32> {
    let v: f64 = value.trim().parse().ok()?;
    (v.is_finite() && (0.0..=1.0).contains(&v)).then(|| (v * 100.0).round() as u32)
}

fn lock<T>(m: &std::sync::Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A key on its way: counted from before the shim looks whether teardown has begun until it has
/// gone out or been refused, so teardown, which marks itself first and then waits for the count to
/// fall, sends its stops after every key that saw it had not begun ([`super::TciDaemon`]'s `Drop`).
struct KeyInFlight<'a>(&'a AtomicUsize);

impl<'a> KeyInFlight<'a> {
    fn new(count: &'a AtomicUsize) -> KeyInFlight<'a> {
        count.fetch_add(1, Ordering::SeqCst);
        KeyInFlight(count)
    }
}

impl Drop for KeyInFlight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The rigctld backend over one TCI session.
pub struct TciShim {
    /// Weak, so the daemon's drop closes the session whatever clients are still connected here.
    conn: Weak<Connection>,
    trx: Trx,
    /// True while Nexus itself intends to transmit: the broker's disconnect fail-safe stands down
    /// then. The same contract as the CI-V, OmniRig and Flex daemons'.
    tx_intent: Arc<AtomicBool>,
    state: Arc<ClientState>,
}

impl TciShim {
    pub(crate) fn new(
        conn: Weak<Connection>,
        trx: Trx,
        tx_intent: Arc<AtomicBool>,
        state: Arc<ClientState>,
    ) -> TciShim {
        TciShim {
            conn,
            trx,
            tx_intent,
            state,
        }
    }

    /// Read the session's state in place; `None` once it is gone.
    fn read<T>(&self, f: impl FnOnce(&Snapshot) -> Option<T>) -> Option<T> {
        self.conn.upgrade()?.read(f)
    }

    /// Send `command` and wait at most `wait` for the server to report what `done` looks for
    /// (see the module notes).
    fn write(
        &self,
        command: Command,
        wait: Duration,
        done: impl Fn(&Snapshot) -> bool,
    ) -> Result<(), SetFault> {
        if self.state.closing.load(Ordering::SeqCst) {
            return Err(SetFault::NoAnswer);
        }
        let conn = self.conn.upgrade().ok_or(SetFault::NoAnswer)?;
        match conn.send(command) {
            Ok(()) => {}
            Err(SendError::NotReady | SendError::Closed) => return Err(SetFault::NoAnswer),
            Err(SendError::AudioInUse) => return Err(SetFault::Refused),
        }
        drop(conn);
        self.confirm(wait, done)
    }

    /// Wait at most `wait` for `done`, a slice at a time, the session held only for each slice:
    /// a closing daemon is never kept waiting by a write in flight here.
    fn confirm(&self, wait: Duration, done: impl Fn(&Snapshot) -> bool) -> Result<(), SetFault> {
        let deadline = Instant::now() + wait;
        loop {
            if self.state.closing.load(Ordering::SeqCst) {
                return Err(SetFault::NoAnswer);
            }
            let conn = self.conn.upgrade().ok_or(SetFault::NoAnswer)?;
            let left = deadline.saturating_duration_since(Instant::now());
            conn.wait_until(left.min(SLICE), |s| s.phase == Phase::Closed || done(s));
            match conn.read(|s| (s.phase == Phase::Closed, done(s))) {
                (true, _) => return Err(SetFault::NoAnswer),
                (false, true) => return Ok(()),
                (false, false) if left.is_zero() => return Err(SetFault::NoAnswer),
                (false, false) => {}
            }
        }
    }

    /// A dial write for VFO `channel`, refused before anything is sent where the server could not
    /// take it.
    fn tune_vfo(&self, channel: Channel, hz: u64) -> Result<(), SetFault> {
        let trx = self.trx;
        let hz = i64::try_from(hz)
            .ok()
            .and_then(Hz::new)
            .ok_or(SetFault::Refused)?;
        let may = self
            .read(|s| {
                let within = match s.model.vfo_limits.known() {
                    Some((low, high)) => (*low..=*high).contains(&hz),
                    None => true,
                };
                let locked = s.model.trx(trx).vfo_lock[channel.index()].is_true();
                Some(within && !locked)
            })
            .ok_or(SetFault::NoAnswer)?;
        if !may {
            return Err(SetFault::Refused);
        }
        self.write(
            Command::Set(Setting::Vfo { trx, channel, hz }),
            self.state.readback,
            |s| s.model.trx(trx).vfo[channel.index()].known() == Some(&hz),
        )
    }

    /// One of the transceiver's switches (split, RIT, XIT).
    fn switch(&self, flag: Flag, on: bool) -> Result<(), SetFault> {
        let trx = self.trx;
        self.write(
            Command::Set(Setting::Flag { flag, trx, on }),
            self.state.readback,
            |s| {
                let t = s.model.trx(trx);
                let reading = match flag {
                    Flag::Split => &t.split,
                    Flag::Rit => &t.rit,
                    _ => &t.xit,
                };
                reading.known() == Some(&on)
            },
        )
    }

    /// An offset (RIT, XIT), in Hz.
    fn offset(&self, rit: bool, hz: i32) -> bool {
        let trx = self.trx;
        let Some(hz) = Hz::new(i64::from(hz)) else {
            return false;
        };
        let setting = if rit {
            Setting::RitOffset { trx, hz }
        } else {
            Setting::XitOffset { trx, hz }
        };
        self.write(Command::Set(setting), self.state.readback, |s| {
            let t = s.model.trx(trx);
            let reading = if rit { &t.rit_offset } else { &t.xit_offset };
            reading.known() == Some(&hz)
        })
        .is_ok()
    }

    /// The drive Nexus sets (0–100) in the server's own form; it becomes the ceiling the session's
    /// admission holds the server's drive to.
    fn drive(&self, level: u32, wait: Duration) -> Result<(), SetFault> {
        let trx = self.trx;
        let dialect = self.read(|s| Some(s.dialect)).ok_or(SetFault::NoAnswer)?;
        self.write(
            Command::Set(Setting::drive(trx, level, &dialect)),
            wait,
            |s| s.model.drive(trx).known() == Some(&level),
        )
    }

    /// Keep why a key was kept off the air, for the radio loop to put on screen
    /// ([`super::TciDaemon::key_refused_since`]), and log it.
    fn refuse(&self, why: String) {
        tempo_core::applog::warn("cat", &format!("TCI: {why}"));
        *lock(&self.state.refused) = Some((Instant::now(), why));
    }

    /// Key an over, or say why not (see the module notes). The radio loop has run every engine
    /// gate before it sent `T 1`.
    fn key(&self) -> Result<(), String> {
        let _in_flight = KeyInFlight::new(&self.state.keys_in_flight);
        if self.state.closing.load(Ordering::SeqCst) {
            return Err("not keyed: the TCI connection is closing".into());
        }
        let trx = self.trx;
        let reported = self
            .read(|s| Some(s.model.trx(trx).modulation.known().cloned()))
            .ok_or_else(|| format!("not keyed: {CLOSED}"))?;
        let asked = lock(&self.state.asked_mode).clone();
        let Some(mode) = asked.clone().or_else(|| reported.clone()) else {
            return Err("not keyed: the SDR program has not said which mode it is in".into());
        };
        let voice = self.state.voice.load(Ordering::SeqCst);
        let start = match class(mode.as_str()) {
            Class::Cw => {
                return Err(format!(
                    "not keyed in {}: CW through the SDR program's own keyer is not built yet; \
                     the soundcard keyer sends CW over TCI",
                    rigctld_mode(mode.as_str())
                ))
            }
            Class::Other => {
                return Err(format!(
                    "not keyed: the SDR program is in {}, which Nexus does not transmit in",
                    rigctld_mode(mode.as_str())
                ))
            }
            Class::Digital if self.state.audio => TxStart::Audio,
            Class::Voice if voice && self.state.audio => TxStart::Audio,
            Class::Digital | Class::Voice => TxStart::Microphone,
        };
        if start == TxStart::Audio && !self.state.tx_route.load(Ordering::SeqCst) {
            return Err(
                "not keyed: Nexus's transmit audio is not connected to the SDR program yet".into(),
            );
        }
        // The mode the radio loop asked for, put back where the server reports another (a band
        // change restored the band's own): the source above was chosen for it.
        if let Some(asked) = asked.filter(|a| reported.as_ref() != Some(a)) {
            let put_back = self.write(
                Command::Set(Setting::Modulation {
                    trx,
                    channel: None,
                    mode: asked.clone(),
                }),
                self.state.reassert,
                |s| s.model.trx(trx).modulation.known() == Some(&asked),
            );
            if put_back.is_err() {
                return Err(format!(
                    "not keyed: the SDR program reports {} where Nexus set {}, and did not take \
                     {} back in time",
                    reported.map_or_else(|| "no mode".to_string(), |m| rigctld_mode(m.as_str())),
                    rigctld_mode(asked.as_str()),
                    rigctld_mode(asked.as_str())
                ));
            }
        }
        let conn = self
            .conn
            .upgrade()
            .ok_or_else(|| format!("not keyed: {CLOSED}"))?;
        match conn.key(start) {
            Ok(()) => Ok(()),
            // The drive Nexus set, put back where the server reports more (a band restore) or
            // none it can read: the one value Nexus writes before a key, and only ever down to its
            // own.
            Err(Some(
                Refusal::DriveAboveCeiling {
                    ceiling,
                    tune: false,
                    ..
                }
                | Refusal::DriveUnknown {
                    ceiling: Some(ceiling),
                    tune: false,
                },
            )) => {
                drop(conn);
                if self.drive(ceiling, self.state.reassert).is_err() {
                    return Err(format!(
                        "not keyed: the SDR program's drive is not the {ceiling} Nexus set, and it \
                         did not take {ceiling} back in time"
                    ));
                }
                let conn = self
                    .conn
                    .upgrade()
                    .ok_or_else(|| format!("not keyed: {CLOSED}"))?;
                match conn.key(start) {
                    Ok(()) => Ok(()),
                    Err(Some(refusal)) => Err(format!("not keyed: {refusal}")),
                    Err(None) => Err(format!("not keyed: {CLOSED}")),
                }
            }
            Err(Some(refusal)) => Err(format!("not keyed: {refusal}")),
            Err(None) => Err(format!("not keyed: {CLOSED}")),
        }
    }

    /// End everything of ours, never gated: what `T 0` means here. Only a closed session fails it.
    fn end_ours(&self) -> bool {
        self.conn
            .upgrade()
            .is_some_and(|conn| matches!(conn.end_ours(), StopOutcome::Sent(_)))
    }
}

impl RigBackend for TciShim {
    fn owner_transmitting(&self) -> bool {
        self.tx_intent.load(Ordering::Relaxed)
    }

    fn freq_hz(&self) -> u64 {
        // 0 is "no honest reading", the answer every daemon gives a dead link, and one the radio
        // loop never takes for a dial.
        self.read(|s| {
            s.model.trx(self.trx).vfo[Channel::A.index()]
                .known()
                .copied()
        })
        .and_then(|hz| u64::try_from(hz.get()).ok())
        .unwrap_or(0)
    }

    fn mode(&self) -> (String, u32) {
        // An unknown mode is an empty word, never a guessed "USB": a partial answer is not a
        // reading.
        self.read(|s| {
            let t = s.model.trx(self.trx);
            let mode = rigctld_mode(t.modulation.known()?.as_str());
            let width = t
                .filter
                .known()
                .and_then(|(low, high)| u32::try_from(high.get() - low.get()).ok())
                .unwrap_or(0);
            Some((mode, width))
        })
        .unwrap_or_default()
    }

    fn ptt(&self) -> bool {
        // What the server reports, whoever keyed: Nexus must see another program's transmission
        // and the SDR operator's too. A report that could not be read is not "unkeyed": until it
        // is read again (the session asks within a second), our own key is the only thing known.
        self.read(|s| {
            let t = s.model.trx(self.trx);
            Some(if t.transmitting.is_true() || t.tuning.is_true() {
                true
            } else if t.transmitting.is_unreadable() || t.tuning.is_unreadable() {
                s.keyed.is_some()
            } else {
                false
            })
        })
        .unwrap_or(false)
    }

    fn vfo(&self) -> String {
        "VFOA".to_string()
    }

    fn split(&self) -> bool {
        self.read(|s| Some(s.model.trx(self.trx).split.is_true()))
            .unwrap_or(false)
    }

    fn set_freq(&self, hz: u64) -> bool {
        self.try_set_freq(hz).is_ok()
    }

    /// The dial: done once the server reports it; no report in time is silence (`RPRT -5`).
    fn try_set_freq(&self, hz: u64) -> Result<(), SetFault> {
        self.tune_vfo(Channel::A, hz)
    }

    fn set_mode(&self, mode: &str, passband_hz: u32) -> bool {
        self.try_set_mode(mode, passband_hz).is_ok()
    }

    /// The mode, then its width; each done once the server reports it. A width that cannot be
    /// placed refuses the verb before the mode moves.
    fn try_set_mode(&self, mode: &str, passband_hz: u32) -> Result<(), SetFault> {
        let trx = self.trx;
        let (offered, current_mode, current) = self
            .read(|s| {
                let t = s.model.trx(trx);
                Some((
                    s.model.modulations.clone(),
                    t.modulation.known().cloned(),
                    t.filter.known().map(|(low, high)| (low.get(), high.get())),
                ))
            })
            .ok_or(SetFault::NoAnswer)?;
        let word = tci_mode_for(mode, &offered).ok_or(SetFault::Refused)?;
        let filter = match passband_hz {
            0 => None,
            width => Some(
                filter_for(
                    word.as_str(),
                    width,
                    current_mode.as_ref().map(Word::as_str),
                    current,
                )
                .and_then(|(low, high)| Some((Hz::new(low)?, Hz::new(high)?)))
                .ok_or(SetFault::Refused)?,
            ),
        };
        // What the radio loop asks for is the over's mode from now, whether or not the server takes
        // it: a key is classed by it, and puts it back first ([`Self::key`]).
        *lock(&self.state.asked_mode) = Some(word.clone());
        self.write(
            Command::Set(Setting::Modulation {
                trx,
                channel: None,
                mode: word.clone(),
            }),
            self.state.readback,
            |s| s.model.trx(trx).modulation.known() == Some(&word),
        )?;
        match filter {
            None => Ok(()),
            Some((low, high)) => self.write(
                Command::Set(Setting::RxFilterBand { trx, low, high }),
                self.state.readback,
                |s| s.model.trx(trx).filter.known() == Some(&(low, high)),
            ),
        }
    }

    fn set_ptt(&self, on: bool) -> bool {
        if !on {
            // Never gated: nothing is asked before the stop goes out.
            return self.end_ours();
        }
        match self.key() {
            Ok(()) => true,
            Err(why) => {
                self.refuse(why);
                false
            }
        }
    }

    fn set_vfo(&self, vfo: &str) -> bool {
        // VFO A is the dial; B is reached as the split frequency (`I`).
        matches!(vfo, "VFOA" | "currVFO")
    }

    /// The server's own frequency range (`vfo_limits`), or an empty list, which Nexus's reader takes
    /// as unknown: every caller fails open, and the server answers for itself.
    fn rx_ranges(&self) -> Option<Vec<(u64, u64)>> {
        Some(
            self.read(|s| {
                let (low, high) = s.model.vfo_limits.known()?;
                let low = u64::try_from(low.get()).ok()?;
                let high = u64::try_from(high.get()).ok()?;
                (low < high).then(|| vec![(low, high)])
            })
            .unwrap_or_default(),
        )
    }

    fn level(&self, name: &str) -> Option<String> {
        let trx = self.trx;
        match name {
            "RFPOWER" => self.read(|s| {
                let level = *s.model.drive(trx).known()?;
                Some(format!("{:.6}", f64::from(level.min(100)) / 100.0))
            }),
            // dB from S9 (−73 dBm on HF, −93 dBm from 30 MHz), at the dial.
            "STRENGTH" => self.read(|s| {
                let t = s.model.trx(trx);
                let dbm = t.rx_dbm[Channel::A.index()].known()?.to_f64();
                let dial = t.vfo[Channel::A.index()]
                    .known()
                    .and_then(|hz| u64::try_from(hz.get()).ok())
                    .unwrap_or(0);
                Some(crate::flexspectrum::smeter_dbm_to_rel_s9(dbm as f32, dial).to_string())
            }),
            "SWR" => self.read(|s| {
                let swr = match s.model.trx(trx).tx_sensors.known() {
                    Some(sensors) => sensors.swr,
                    None => *s.model.tx_swr.known()?,
                };
                Some(format!("{:.2}", swr.to_f64()))
            }),
            // Peak watts, as WSJT-X reads `tx_sensors` (`TCITransceiver.cpp:656`).
            "RFPOWER_METER_WATTS" => self.read(|s| {
                let watts = match s.model.trx(trx).tx_sensors.known() {
                    Some(sensors) => sensors.peak_w,
                    None => *s.model.tx_power.known()?,
                };
                Some(format!("{:.1}", watts.to_f64()))
            }),
            _ => None,
        }
    }

    fn set_level(&self, name: &str, value: &str) -> Option<bool> {
        self.try_set_level(name, value).map(|r| r.is_ok())
    }

    /// The drive: done once the server reports it. A value TCI cannot carry is refused before
    /// anything is sent.
    fn try_set_level(&self, name: &str, value: &str) -> Option<Result<(), SetFault>> {
        match name {
            "RFPOWER" => Some(percent(value).map_or(Err(SetFault::Refused), |level| {
                self.drive(level, self.state.readback)
            })),
            _ => None,
        }
    }

    fn func(&self, token: &str) -> Option<bool> {
        let trx = self.trx;
        self.read(|s| {
            let t = s.model.trx(trx);
            match token {
                "RIT" => t.rit.known().copied(),
                "XIT" => t.xit.known().copied(),
                _ => None,
            }
        })
    }

    fn set_func(&self, token: &str, on: bool) -> Option<bool> {
        self.try_set_func(token, on).map(|r| r.is_ok())
    }

    /// RIT and XIT on or off: done once the server reports it.
    fn try_set_func(&self, token: &str, on: bool) -> Option<Result<(), SetFault>> {
        let flag = match token {
            "RIT" => Flag::Rit,
            "XIT" => Flag::Xit,
            _ => return None,
        };
        Some(self.switch(flag, on))
    }

    fn set_split(&self, on: bool, tx_vfo: &str) -> Option<bool> {
        self.try_set_split(on, tx_vfo).map(|r| r.is_ok())
    }

    /// Split on or off; the transmit VFO is VFO B, whatever the client names.
    fn try_set_split(&self, on: bool, _tx_vfo: &str) -> Option<Result<(), SetFault>> {
        Some(self.switch(Flag::Split, on))
    }

    fn set_split_freq(&self, hz: u64) -> Option<bool> {
        self.try_set_split_freq(hz).map(|r| r.is_ok())
    }

    /// The split frequency: VFO B, as the dial is VFO A.
    fn try_set_split_freq(&self, hz: u64) -> Option<Result<(), SetFault>> {
        Some(self.tune_vfo(Channel::B, hz))
    }

    fn set_rit(&self, hz: i32) -> Option<bool> {
        Some(self.offset(true, hz))
    }

    fn set_xit(&self, hz: i32) -> Option<bool> {
        Some(self.offset(false, hz))
    }

    fn caps(&self) -> Option<RigCaps> {
        Some(authored_caps(self.rx_ranges().unwrap_or_default()))
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    fn modes(list: &str) -> Reading<Modes> {
        Reading::Known(
            Modes::new(list.split(',').filter_map(Word::new).collect()).expect("a mode list"),
        )
    }

    fn word(m: &str, offered: &Reading<Modes>) -> Option<String> {
        tci_mode_for(m, offered).map(|w| w.as_str().to_string())
    }

    /// Each rigctld word becomes the name the server offers, and comes back as itself.
    #[test]
    fn the_rigctld_words_map_to_the_names_a_server_offers_and_back() {
        let esdr3 = modes("AM,SAM,DSB,LSB,USB,CW,NFM,WFM,DIGL,DIGU,DRM");
        let thetis = modes("LSB,USB,DSB,CWL,CWU,FM,AM,DIGU,SPEC,DIGL,SAM,DRM");
        assert_eq!(word("PKTUSB", &esdr3).as_deref(), Some("digu"));
        assert_eq!(word("pktlsb", &esdr3).as_deref(), Some("digl"));
        assert_eq!(word("USB", &esdr3).as_deref(), Some("usb"));
        assert_eq!(word("CW", &esdr3).as_deref(), Some("cw"));
        assert_eq!(word("CW", &thetis).as_deref(), Some("cwu"));
        assert_eq!(word("CWR", &thetis).as_deref(), Some("cwl"));
        assert_eq!(word("FM", &esdr3).as_deref(), Some("nfm"));
        assert_eq!(word("FM", &thetis).as_deref(), Some("fm"));
        for (tci, back) in [
            ("digu", "PKTUSB"),
            ("digl", "PKTLSB"),
            ("cwu", "CW"),
            ("cwl", "CWR"),
            ("nfm", "FM"),
            ("usb", "USB"),
            ("drm", "DRM"),
        ] {
            assert_eq!(rigctld_mode(tci), back, "{tci}");
        }
    }

    /// A mode the server does not offer is refused, never approximated, and so is every mode
    /// while its list cannot be read.
    #[test]
    fn a_mode_the_server_does_not_offer_is_refused() {
        let esdr3 = modes("AM,SAM,DSB,LSB,USB,CW,NFM,WFM,DIGL,DIGU,DRM");
        assert_eq!(word("CWR", &esdr3), None, "no cwl on this server");
        assert_eq!(word("PKTFM", &esdr3), None);
        assert_eq!(word("RTTY", &esdr3), None);
        assert_eq!(word("", &esdr3), None);
        assert_eq!(word("USB", &Reading::Unreadable), None);
        assert_eq!(
            word("PKTUSB", &Reading::NotReported).as_deref(),
            Some("digu"),
            "no list sent: the spec's names"
        );
    }

    /// The class decides the key's source: only DIGU/DIGL are digital, CW is its own, and a mode
    /// Nexus does not transmit in is neither.
    #[test]
    fn each_mode_has_its_class() {
        assert_eq!(class("digu"), Class::Digital);
        assert_eq!(class("digl"), Class::Digital);
        for voice in ["usb", "lsb", "am", "sam", "dsb", "fm", "nfm", "fmn", "wfm"] {
            assert_eq!(class(voice), Class::Voice, "{voice}");
        }
        for cw in ["cw", "cwu", "cwl", "cwr"] {
            assert_eq!(class(cw), Class::Cw, "{cw}");
        }
        for other in ["drm", "spec", ""] {
            assert_eq!(class(other), Class::Other, "{other}");
        }
    }

    /// A width keeps the receiver's own edge on the side it already sits on, and starts from the
    /// dial on a new side.
    #[test]
    fn a_width_keeps_the_edge_the_receiver_already_has_on_its_side() {
        assert_eq!(
            filter_for("digu", 3000, Some("usb"), Some((100, 2900))),
            Some((100, 3100))
        );
        assert_eq!(
            filter_for("digu", 3000, Some("lsb"), Some((-2900, -100))),
            Some((0, 3000))
        );
        assert_eq!(
            filter_for("digl", 3000, Some("lsb"), Some((-2900, -100))),
            Some((-3100, -100))
        );
        assert_eq!(
            filter_for("am", 6000, Some("usb"), Some((100, 2900))),
            Some((-3000, 3000))
        );
        assert_eq!(
            filter_for("cw", 500, Some("cw"), Some((-300, 300))),
            Some((-250, 250))
        );
        assert_eq!(filter_for("drm", 500, None, None), None);
        assert_eq!(filter_for("usb", 0, None, None), None);
        assert_eq!(filter_for("usb", 200_000, None, None), None);
    }

    #[test]
    fn levels_are_fractions_of_full_scale() {
        assert_eq!(percent("0.5"), Some(50));
        assert_eq!(percent("1"), Some(100));
        assert_eq!(percent("0"), Some(0));
        assert_eq!(percent("1.5"), None);
        assert_eq!(percent("-0.1"), None);
        assert_eq!(percent("NaN"), None);
        assert_eq!(percent("x"), None);
    }
}
