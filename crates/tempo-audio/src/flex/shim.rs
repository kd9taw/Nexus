//! The rigctld shim for a FlexRadio: [`FlexShim`] serves one slice as the radio's dial, answering
//! reads from the session's status model and turning each write into ONE typed command or intent
//! of the protocol core. Every byte on the rigctld wire is still produced by
//! [`crate::rigctld_server::handle_command`], the encoder every backend shares.
//!
//! ## The served slice
//! Our transmit slice; else our lowest-numbered slice, receive only (admission refuses to key a
//! transmit slice that is not ours); else none, and the reads say so (`f` answers 0, which the
//! radio loop never takes for a dial). Another client's slice is never served.
//!
//! ## The verb map
//!
//! | rigctld | Typed command or intent | Gate |
//! |---|---|---|
//! | `T 1` (any non-zero) | `TxStart::Key` → `xmit 1` | the core's admission, after every engine gate the loop ran; with native audio on, a digital over only while the radio takes its audio from Nexus's DAX; with it off, never while Nexus's own write still has the radio on DAX; a voice over never while that write has the radio on DAX in place of the mic, unless the stream's browser voice rides it |
//! | `T 0` | everything of ours (`Connection::end_ours`): the open operation's own stop (`xmit 0`, `transmit tune 0`, `cwx clear`, or the ATU's `xmit 0` and `transmit tune 0`); with none open, every stop the radio's status shows an earlier session of ours may need | never gated; sent only for what is ours |
//! | `U TUNER <n≠0>` | `TxStart::AtuStart` → `atu start` | admission (refuses today: no readback) |
//! | `b <text>` | `TxStart::CwxSend` → `cwx send`, one word, with its keying time at the radio's speed | first, every character one Nexus can time; admission (refuses today: no readback); then the slice in CW, the radio's break-in on, Sync CWX off, XIT off; a refusal says why ([`super::FlexDaemon::key_refused_since`]) |
//! | `\stop_morse` | `TxStop::CwxClear` → `cwx clear` | never gated |
//! | `L KEYSPD <wpm>` | `Command::CwSpeed` → `cw wpm <5–100>` | keys nothing; the engine's WPM control decides the value |
//! | `F <hz>` | `slice tune <n> <MHz>` | the slice must be ours |
//! | `M <mode> <width>` | `slice set <n> mode=`, then `filt <n> <lo> <hi>` for a width | a mode the radio offers; a width placed on the mode's side |
//! | `L RFPOWER <0..1>` | `transmit set rfpower=<0–100>` | the engine's per-mode ceiling decides the value |
//! | `L AF <0..1>` | `slice set <n> audio_level=` | the slice must be ours |
//! | `U NB/NR/ANF <0/1>` | `slice set <n> nb=/nr=/anf=` | the slice must be ours |
//! | `f`, `m`, `t`, `v`, `s`, `l RFPOWER`, `l AF`, `l KEYSPD`, `u NB/NR/ANF` | read from the status model | — |
//! | `\dump_caps` | the authored capabilities ([`authored_caps`]) | — |
//!
//! Everything else answers as the shared encoder answers a backend that lacks it (`RPRT -11`),
//! byte for byte. `T 1` is a key whatever its number (rigctld's `T 3` is a data-port key), as in
//! every other backend.
//!
//! ## A digital over never goes out on the radio's mic
//! While the operator's native audio is on and no other program feeds DAX, Nexus's over reaches
//! the radio only as DAX. The DAX source flag follows the TX slice's mode at quiet moments
//! (`super::routing`), so right after a mode change, or after the operator flips the flag by
//! hand, the radio can still be taking its audio from its mic input. Keying then would put
//! whatever that input hears on a digital frequency. So `T 1` is refused while the transmit
//! slice's mode is digital (the mode this shim just commanded counts, before the radio reports
//! it) and the radio does not report `transmit dax=1` on Nexus's own transmit stream. It only
//! refuses, sends nothing, and the loop reports a refused PTT, with the shim's reason
//! ([`super::FlexDaemon::key_refused_since`]: the rigctld answer, `RPRT -1`, carries none).
//!
//! ## Nor on a DAX nothing feeds
//! The other way round: native audio turned off (by the operator, or by the receive floor giving
//! up on DAX) hands Nexus's audio back to the sound card at once, but the operator's own setting,
//! the mic, comes back only at the next quiet point, after the boundary when the toggle fell
//! inside the guard before it. Keyed in between, the radio would take a digital over from the DAX
//! Nexus wrote, which nothing feeds: a silent over. So with native audio off, `T 1` is refused
//! for a digital transmit slice while Nexus's own write still has the radio on DAX
//! ([`super::routing::Routing::leaves_dax`]), the same way, with its own reason (operator ruling,
//! 2026-10-07, "Refuse that over").
//!
//! ## Nor a voice over on DAX
//! Phone the same way (operator ruling, 2026-10-08, "Same rule for Phone"): right after the
//! transmit slice leaves a digital mode for a voice mode, or native audio goes off, the radio
//! still takes its transmit audio from the DAX Nexus set until the routing's next quiet point puts
//! the operator's mic back, and a key held from then on holds that quiet point off, so the voice
//! would not reach the air for the whole over. `T 1` is refused for a voice transmit slice while
//! Nexus's own write still has the radio on DAX in place of the mic
//! ([`super::routing::Routing::leaves_voice_on_dax`]), with its own reason; not while the stream's
//! browser voice is live with native audio on, which rides DAX.
//!
//! ## A CW word is timed at the radio's speed
//! The radio's own keyer sends each word (`cwx send`) and keys and unkeys itself with its
//! break-in; the readback proves the word ended once the radio holds idle past the word's
//! expected end, so the shim hands the session the word's keying time
//! ([`tempo_core::cw::morse_duration_ms`]) at the speed the radio reports, or at the speed the shim
//! has just set when that is slower and the radio has not reported it yet ([`word_wpm`]): a word
//! is never timed shorter than the radio keys it. A word with a character Nexus has no Morse for
//! is not sent: it cannot be timed (`morse_duration_ms` skips such a character, while the radio's
//! keyer may send it), so it could outlast its window and end in a false alarm. A word the client
//! does not send says why on the CW line, in place of the rigctld answer, `RPRT -1`.
//!
//! Nexus's own design, not a port.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, Weak};
use std::time::{Duration, Instant};

use tempo_app::dto::{FlexAudioCause, FlexAudioRefusal};
use tempo_net::flex::admission::{another_dax_feeder, Refusal};
use tempo_net::flex::encode::{Command, CwxText, Mode, SliceFunction, TxStart, TxStop};
use tempo_net::flex::model::{owner_of, Owner, StatusModel};
use tempo_net::flex::session::{Connection, EndOutcome, Snapshot, StopOutcome};
use tempo_net::flex::status::SliceDelta;

use crate::baud_ladder::{RigCaps, SplitDetect};
use crate::rigctld_server::RigBackend;

use super::routing::{mode_class, ModeClass};
use super::{effective_mode, routing_view, ClientState, COMMANDED_FOR};

/// How long a write waits for the radio's reply: well inside the radio loop's own CAT deadline,
/// so a radio that does not answer is a refused write, not a dropped CAT connection.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);

/// The modes a slice offers when the radio has not sent its `mode_list` (FlexRadio's API
/// documentation, `TCPIP-slice`).
const FLEX_MODES: [&str; 11] = [
    "USB", "LSB", "CW", "AM", "SAM", "FM", "NFM", "DFM", "DIGU", "DIGL", "RTTY",
];

/// The slice this shim serves as the radio's dial: our transmit slice, else our lowest-numbered
/// slice, else none.
pub(crate) fn served_slice(model: &StatusModel, ours: Option<u32>) -> Option<u8> {
    let owned =
        |i: &u8| owner_of(model.slices.get(i).and_then(|s| s.client_handle), ours) == Owner::Ours;
    if let [tx] = model.tx_slices().as_slice() {
        if owned(tx) {
            return Some(*tx);
        }
    }
    model.slices.keys().copied().find(|i| owned(i))
}

/// A rigctld mode word as a slice's: the data modes have Flex names of their own, the rest are
/// the same word. `None` when the radio does not offer it (its `mode_list`, or the documented
/// modes when it has not sent one) — refused, never approximated: rigctld's `CWR` or `PKTFM`
/// have no Flex mode.
pub(crate) fn flex_mode_for(rigctld: &str, offered: Option<&[String]>) -> Option<String> {
    let word = match rigctld.trim().to_ascii_uppercase().as_str() {
        "PKTUSB" => "DIGU".to_string(),
        "PKTLSB" => "DIGL".to_string(),
        other => other.to_string(),
    };
    let ok = match offered {
        Some(list) => list.contains(&word),
        None => FLEX_MODES.contains(&word.as_str()),
    };
    ok.then_some(word)
}

/// A slice's mode as the rigctld word Nexus uses for every radio; the radio's own word where
/// rigctld has none.
pub(crate) fn rigctld_mode(flex: &str) -> String {
    match flex {
        "DIGU" => "PKTUSB",
        "DIGL" => "PKTLSB",
        other => other,
    }
    .to_string()
}

/// Which side of the slice's frequency a mode's passband lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Upper,
    Lower,
    Centred,
}

fn side(flex_mode: &str) -> Option<Side> {
    match flex_mode {
        "USB" | "DIGU" => Some(Side::Upper),
        "LSB" | "DIGL" => Some(Side::Lower),
        "CW" | "AM" | "SAM" | "FM" | "NFM" | "DFM" => Some(Side::Centred),
        _ => None,
    }
}

/// The filter edges (Hz from the slice's frequency) for a `width_hz` passband in `mode`.
///
/// A slice's filter is two edges, and rigctld sends a width, so one edge has to be chosen. Where
/// the slice already sits on the same side, its own edge stays where the operator put it (the low
/// edge on the upper side, the high edge on the lower side, the centre for a centred mode);
/// otherwise the edge is the slice's frequency itself. `None` for a mode with no side to place a
/// width on (RTTY's passband sits around its mark and shift), or a width that is not one.
pub(crate) fn filter_for(
    mode: &str,
    width_hz: u32,
    current_mode: Option<&str>,
    current: Option<(i32, i32)>,
) -> Option<(i32, i32)> {
    let width = i32::try_from(width_hz)
        .ok()
        .filter(|w| (1..=100_000).contains(w))?;
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

/// A FlexRadio's capabilities, AUTHORED rather than read out of Hamlib (the spec's "authored
/// `RigCaps`"):
/// - **split by slice pair**: the transmit slice is the dial this shim serves, so there is no
///   rigctld split to read, and the split answer is "cannot" — silence is never permission;
/// - **RF power floor 0**: `transmit set rfpower=` runs 0–100, so a reading of zero is a real
///   zero;
/// - **native VFO read**: the VFO this shim answers is the served slice itself, never a cache of
///   what Nexus last set;
/// - **receive coverage unknown**: the `radio` status names the model and carries a `bands`
///   value whose format is not established, so no coverage is claimed, and unknown fails open.
///   The transmit side is the licence table's, as on every radio.
pub fn authored_caps() -> RigCaps {
    RigCaps {
        serial_rates: None,
        rx_coverage: Vec::new(),
        split_detect: SplitDetect::Absent,
        rfpower_floor_milli: Some(0),
        vfo_read_native: true,
    }
}

/// The rigctld backend over one Flex session.
pub struct FlexShim {
    /// Weak, so the daemon's drop closes the session whatever clients are still connected here.
    conn: Weak<Connection>,
    /// True while Nexus itself intends to transmit: the broker's disconnect fail-safe stands
    /// down then. The same contract as the CI-V and OmniRig daemons'.
    tx_intent: Arc<AtomicBool>,
    state: Arc<ClientState>,
}

impl FlexShim {
    pub(crate) fn new(
        conn: Weak<Connection>,
        tx_intent: Arc<AtomicBool>,
        state: Arc<ClientState>,
    ) -> FlexShim {
        FlexShim {
            conn,
            tx_intent,
            state,
        }
    }

    /// Why a key must not go out on the audio route as it stands, if it must not: a digital
    /// over while the radio would take its audio from its mic input, or, with native audio off,
    /// from a DAX nothing feeds, and a voice over while it would take its audio from DAX in place
    /// of the mic (see the module header). In the shim's words, for the log and the refusal on
    /// screen, and as a cause, for the UI's own.
    pub(crate) fn audio_refuses_key(&self) -> Option<(String, FlexAudioRefusal)> {
        let native = self.state.native_audio.load(Ordering::Relaxed);
        let conn = self.conn.upgrade()?;
        let snap = conn.snapshot();
        if another_dax_feeder(&snap.model, snap.handle, snap.dax_tx_stream).is_some() {
            // Another program's DAX carries Nexus's audio from the sound card: not ours to judge.
            return None;
        }
        // No single transmit slice: admission refuses the key itself.
        let [slice] = snap.model.tx_slices()[..] else {
            return None;
        };
        let mode = effective_mode(&snap.model, slice, &self.state)?;
        let class = mode_class(&mode);
        if class == ModeClass::Phone {
            // The operator's mic carries a voice over, unless the browser voice rides DAX.
            let view = routing_view(&snap, &self.state);
            let routing = self
                .state
                .routing
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !routing.leaves_voice_on_dax(&view, native) {
                return None;
            }
            let why = format!(
                "not keying a {mode} over: the radio still takes its transmit audio from the DAX \
                 Nexus set, not its mic input, until Nexus puts the mic back"
            );
            return Some((
                why,
                FlexAudioRefusal {
                    mode,
                    cause: FlexAudioCause::MicNotBack,
                },
            ));
        }
        if class != ModeClass::Digital {
            return None;
        }
        if !native {
            // The over goes to the sound card: only a DAX Nexus itself left the radio on carries
            // nothing.
            let view = routing_view(&snap, &self.state);
            let routing = self
                .state
                .routing
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !routing.leaves_dax(&view) {
                return None;
            }
            let why = format!(
                "not keying a {mode} over: native audio is off, and the radio still takes its \
                 transmit audio from the DAX Nexus set, which nothing feeds until its mic input \
                 is back"
            );
            return Some((
                why,
                FlexAudioRefusal {
                    mode,
                    cause: FlexAudioCause::DaxUnfed,
                },
            ));
        }
        let from_dax = snap.model.transmit.dax == Some(true);
        if from_dax && snap.dax_tx_stream.is_some() {
            return None;
        }
        let why = format!(
            "not keying a {mode} over: the radio takes its transmit audio from {}{}",
            if from_dax { "DAX" } else { "its mic input" },
            if snap.dax_tx_stream.is_some() {
                ""
            } else {
                " and Nexus's DAX transmit stream does not exist yet"
            }
        );
        Some((
            why,
            FlexAudioRefusal {
                mode,
                cause: FlexAudioCause::NotYetDax,
            },
        ))
    }

    /// The session, its state, and the slice served now.
    fn served(&self) -> Option<(Arc<Connection>, Snapshot, u8)> {
        let conn = self.conn.upgrade()?;
        let snap = conn.snapshot();
        let slice = served_slice(&snap.model, snap.handle)?;
        Some((conn, snap, slice))
    }

    /// The served slice's status, for a read.
    fn read<T>(&self, f: impl FnOnce(&SliceDelta) -> Option<T>) -> Option<T> {
        let (_, snap, slice) = self.served()?;
        snap.model.slices.get(&slice).and_then(f)
    }

    /// Send `command` (built for the served slice) and say whether the radio took it.
    fn write(&self, command: impl FnOnce(u8) -> Option<Command>) -> bool {
        let Some((conn, _, slice)) = self.served() else {
            return false;
        };
        let Some(command) = command(slice) else {
            return false;
        };
        matches!(conn.request(command, REQUEST_TIMEOUT), Ok(r) if r.code == 0)
    }

    /// Keep why a key was kept off the air, for the radio loop to put on screen
    /// ([`super::FlexDaemon::key_refused_since`]): the rigctld answer it reads is only `RPRT -1`.
    fn refuse(&self, why: String, cause: Option<FlexAudioRefusal>) {
        *self
            .state
            .refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some((Instant::now(), why, cause));
    }

    /// Send one CW word to the radio's keyer, timed at the radio's speed, or say why not.
    fn send_word(&self, text: &str) -> Result<(), String> {
        let Some(text) = CwxText::new(text) else {
            return Err(
                "CW not sent: the text has a character the radio's CW keyer cannot \
                        take."
                    .to_string(),
            );
        };
        if let Some(c) = text
            .as_str()
            .chars()
            .find(|c| *c != ' ' && tempo_core::cw::morse_code(*c).is_none())
        {
            return Err(format!(
                "CW not sent: Nexus has no Morse for \"{c}\", so it cannot tell how long the \
                 radio takes to send it. Take it out of the message, or use another keyer."
            ));
        }
        let conn = self
            .conn
            .upgrade()
            .ok_or("CW not sent: the connection to the radio is closed.")?;
        let wpm = word_wpm(&conn.snapshot().model, &self.state);
        let word_ms = tempo_core::cw::morse_duration_ms(text.as_str(), wpm).ceil() as u64;
        match conn.start_lasting(TxStart::CwxSend(text), Some(word_ms)) {
            Ok(_) => Ok(()),
            Err(Some(refusal)) => Err(cw_refusal_words(&refusal)),
            Err(None) => Err("CW not sent: the connection to the radio is closed.".to_string()),
        }
    }

    /// Run a start through the core's admission; `false` for any refusal.
    fn start(&self, start: TxStart) -> bool {
        self.conn
            .upgrade()
            .is_some_and(|conn| conn.start(start).is_ok())
    }

    /// Run a stop, which is never gated. Nothing of ours to stop is success: there is nothing on
    /// the air of ours, and another client's transmission is never stopped from here.
    fn stop(&self, stop: TxStop) -> bool {
        self.conn.upgrade().is_some_and(|conn| {
            matches!(
                conn.stop(stop),
                StopOutcome::Sent { .. } | StopOutcome::NothingOfOurs
            )
        })
    }

    /// End everything of ours, never gated: what `T 0` means here. Success as for [`Self::stop`].
    fn end_ours(&self) -> bool {
        self.conn
            .upgrade()
            .is_some_and(|conn| matches!(conn.end_ours(), EndOutcome::Sent(_)))
    }
}

/// A 0..1 level as the radio's 0–100.
fn percent(value: &str) -> Option<i32> {
    let v: f64 = value.trim().parse().ok()?;
    (v.is_finite() && (0.0..=1.0).contains(&v)).then(|| (v * 100.0).round() as i32)
}

/// The radio keyer's slowest speed (`cw wpm` takes 5–100).
const SLOWEST_WPM: u32 = 5;

/// The speed a CW word is timed at: the radio's reported speed, or the speed the shim set within
/// [`COMMANDED_FOR`] when that is slower (the radio may not have reported it yet); the one the shim
/// set when the radio reports none; with neither, the keyer's slowest. Never faster than the radio
/// keys it.
fn word_wpm(model: &StatusModel, state: &ClientState) -> u32 {
    let reported = model.transmit.cw_speed.and_then(|w| u32::try_from(w).ok());
    let commanded = *state.cw_wpm.lock().unwrap_or_else(PoisonError::into_inner);
    match (reported, commanded) {
        (Some(r), Some((c, at))) if at.elapsed() < COMMANDED_FOR => r.min(c),
        (Some(r), _) => r,
        (None, Some((c, _))) => c,
        (None, None) => SLOWEST_WPM,
    }
}

/// The CW line's words for a word the client did not send: what kept it back, and what to do.
pub(crate) fn cw_refusal_words(refusal: &Refusal) -> String {
    match refusal {
        Refusal::NoReadback(_) => "CW not sent: the Flex native client does not send CW yet. For \
                                   CW, turn the Flex native client off (SmartSDR CAT sends it), \
                                   or use the WinKeyer or Soundcard keyer."
            .to_string(),
        Refusal::NotCw { mode } if mode.is_empty() => "CW not sent: the radio has not said which \
                                                        mode its transmit slice is in."
            .to_string(),
        Refusal::NotCw { mode } => format!(
            "CW not sent: the radio's transmit slice is in {mode}, not CW. Nexus sends CW to the \
             radio's keyer only in CW."
        ),
        Refusal::BreakInOff => "CW not sent: break-in is off on the radio. Turn on break-in in \
                                SmartSDR, or use another keyer."
            .to_string(),
        Refusal::SyncCwx => "CW not sent: Sync CWX is on in SmartSDR. Turn it off, or use another \
                             keyer."
            .to_string(),
        Refusal::XitOn => "CW not sent: XIT is on for the radio's transmit slice, so the radio \
                           would send off the frequency Nexus checked. Turn XIT off."
            .to_string(),
        other => format!("CW not sent: {other}."),
    }
}

fn slice_function(token: &str) -> Option<SliceFunction> {
    match token {
        "NB" => Some(SliceFunction::Nb),
        "NR" => Some(SliceFunction::Nr),
        "ANF" => Some(SliceFunction::Anf),
        _ => None,
    }
}

impl RigBackend for FlexShim {
    fn owner_transmitting(&self) -> bool {
        self.tx_intent.load(Ordering::Relaxed)
    }

    fn freq_hz(&self) -> u64 {
        // 0 is "no honest reading", the answer the CI-V and OmniRig backends give a dead link,
        // and one the radio loop never takes for a dial.
        self.read(|s| s.frequency_mhz)
            .filter(|mhz| mhz.is_finite() && *mhz > 0.0)
            .map_or(0, |mhz| (mhz * 1e6).round() as u64)
    }

    fn mode(&self) -> (String, u32) {
        // An unknown mode is an empty word, never a guessed "USB": a partial answer is not a
        // reading.
        self.read(|s| {
            let mode = rigctld_mode(s.mode.as_deref()?);
            let width = match (s.filter_low, s.filter_high) {
                (Some(low), Some(high)) if high > low => u32::try_from(high - low).unwrap_or(0),
                _ => 0,
            };
            Some((mode, width))
        })
        .unwrap_or_default()
    }

    fn ptt(&self) -> bool {
        // What the radio's interlock says, whoever keyed it: Nexus must see another client's
        // transmission too. NOT the session's own "keyed", which stays set until the readback has
        // proved an unkey (a few hundred milliseconds after `READY`); the radio loop reads `t`
        // only while Nexus is idle, to spot SOMEONE ELSE keying, and would take that tail of our
        // own over for exactly that. With no whole interlock sample, our own unconfirmed start
        // is the only thing known.
        self.conn.upgrade().is_some_and(|conn| {
            let snap = conn.snapshot();
            match &snap.model.interlock.sample {
                Some(s) => matches!(
                    s.state.as_str(),
                    "PTT_REQUESTED" | "TRANSMITTING" | "UNKEY_REQUESTED"
                ),
                None => snap.keyed,
            }
        })
    }

    fn vfo(&self) -> String {
        "VFOA".to_string()
    }

    fn split(&self) -> bool {
        false
    }

    fn set_freq(&self, hz: u64) -> bool {
        self.write(|slice| {
            Some(Command::SliceTune {
                slice,
                freq_hz: hz as f64,
                keep_pan: false,
            })
        })
    }

    fn set_mode(&self, mode: &str, passband_hz: u32) -> bool {
        let Some((conn, snap, slice)) = self.served() else {
            return false;
        };
        let Some(s) = snap.model.slices.get(&slice) else {
            return false;
        };
        let Some(word) = flex_mode_for(mode, s.mode_list.as_deref()) else {
            return false;
        };
        // Decide the whole change before sending any of it: a width that cannot be placed refuses
        // the verb without moving the mode.
        let filter = match passband_hz {
            0 => None,
            width => match filter_for(
                &word,
                width,
                s.mode.as_deref(),
                s.filter_low.zip(s.filter_high),
            ) {
                Some(edges) => Some(edges),
                None => return false,
            },
        };
        let Some(mode) = Mode::new(&word) else {
            return false;
        };
        let took = |command| matches!(conn.request(command, REQUEST_TIMEOUT), Ok(r) if r.code == 0);
        let moved = took(Command::SliceMode { slice, mode });
        if moved {
            // The over goes out in this mode from now, whether or not the radio has said so yet.
            *self
                .state
                .commanded
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some((slice, word, Instant::now()));
        }
        moved
            && filter.is_none_or(|(low_hz, high_hz)| {
                took(Command::SliceFilter {
                    slice,
                    low_hz,
                    high_hz,
                })
            })
    }

    fn set_ptt(&self, on: bool) -> bool {
        if on {
            if let Some((why, cause)) = self.audio_refuses_key() {
                tempo_core::applog::warn("cat", &format!("Flex client: {why}"));
                self.refuse(why, Some(cause));
                return false;
            }
            self.start(TxStart::Key)
        } else {
            self.end_ours()
        }
    }

    fn set_vfo(&self, vfo: &str) -> bool {
        // The served slice is the only VFO: A, by name or as the current one.
        matches!(vfo, "VFOA" | "currVFO")
    }

    /// UNKNOWN: the client declares no receive range, so `\dump_state` carries an empty RX list,
    /// which Nexus's reader takes as unknown: every caller fails open, and the radio is tuned and
    /// answers for itself. Left at the trait's `None`, the reply carried the CAT broker's wide row
    /// for WSJT-X, and the app took 135.7 kHz to 1.3 GHz as this radio's range.
    fn rx_ranges(&self) -> Option<Vec<(u64, u64)>> {
        Some(Vec::new())
    }

    fn level(&self, name: &str) -> Option<String> {
        match name {
            "RFPOWER" => {
                let conn = self.conn.upgrade()?;
                let power = conn.snapshot().model.transmit.rf_power?;
                Some(format!("{:.6}", f64::from(power.clamp(0, 100)) / 100.0))
            }
            "AF" => self
                .read(|s| s.audio_level)
                .map(|level| format!("{:.6}", (level / 100.0).clamp(0.0, 1.0))),
            // The keyer's speed as the radio reports it, whoever set it.
            "KEYSPD" => {
                let conn = self.conn.upgrade()?;
                let wpm = conn.snapshot().model.transmit.cw_speed?;
                Some(wpm.to_string())
            }
            _ => None,
        }
    }

    fn set_level(&self, name: &str, value: &str) -> Option<bool> {
        match name {
            "RFPOWER" => Some(percent(value).is_some_and(|level| {
                self.conn.upgrade().is_some_and(|conn| {
                    matches!(
                        conn.request(Command::TransmitRfPower { level }, REQUEST_TIMEOUT),
                        Ok(r) if r.code == 0
                    )
                })
            })),
            "AF" => Some(percent(value).is_some_and(|level| {
                self.write(|slice| Some(Command::SliceAudioLevel { slice, level }))
            })),
            // The keyer's speed, which CWX keys at too: the one CW setting Nexus owns.
            "KEYSPD" => Some(value.trim().parse::<u32>().is_ok_and(|wpm| {
                let took = self.conn.upgrade().is_some_and(|conn| {
                    matches!(
                        conn.request(Command::CwSpeed { wpm }, REQUEST_TIMEOUT),
                        Ok(r) if r.code == 0
                    )
                });
                if took {
                    *self
                        .state
                        .cw_wpm
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = Some((wpm, Instant::now()));
                }
                took
            })),
            _ => None,
        }
    }

    fn func(&self, token: &str) -> Option<bool> {
        let function = slice_function(token)?;
        self.read(|s| match function {
            SliceFunction::Nb => s.nb,
            SliceFunction::Nr => s.nr,
            SliceFunction::Anf => s.anf,
        })
    }

    fn set_func(&self, token: &str, on: bool) -> Option<bool> {
        if token == "TUNER" {
            // The tuner function starts a tune cycle (`U TUNER 2`). Its off half has no sender
            // in Nexus and no meaning here: not implemented, as before.
            return on.then(|| self.start(TxStart::AtuStart));
        }
        let function = slice_function(token)?;
        Some(self.write(|slice| {
            Some(Command::SliceDsp {
                slice,
                function,
                on,
            })
        }))
    }

    fn send_morse(&self, text: &str) -> Option<bool> {
        Some(match self.send_word(text) {
            Ok(()) => true,
            Err(why) => {
                tempo_core::applog::info("cat", &format!("Flex client: {why}"));
                self.refuse(why, None);
                false
            }
        })
    }

    fn stop_morse(&self) -> Option<bool> {
        Some(self.stop(TxStop::CwxClear))
    }

    fn caps(&self) -> Option<RigCaps> {
        Some(authored_caps())
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn the_data_modes_have_flex_names_and_the_rest_keep_theirs() {
        assert_eq!(flex_mode_for("PKTUSB", None).as_deref(), Some("DIGU"));
        assert_eq!(flex_mode_for("pktlsb", None).as_deref(), Some("DIGL"));
        assert_eq!(flex_mode_for("USB", None).as_deref(), Some("USB"));
        assert_eq!(flex_mode_for("CW", None).as_deref(), Some("CW"));
        assert_eq!(rigctld_mode("DIGU"), "PKTUSB");
        assert_eq!(rigctld_mode("DIGL"), "PKTLSB");
        assert_eq!(rigctld_mode("NFM"), "NFM");
    }

    /// A mode the radio does not offer is refused, never approximated.
    #[test]
    fn a_mode_the_radio_does_not_offer_is_refused() {
        assert_eq!(flex_mode_for("CWR", None), None);
        assert_eq!(flex_mode_for("PKTFM", None), None);
        assert_eq!(flex_mode_for("", None), None);
        let offered = vec!["USB".to_string(), "LSB".to_string()];
        assert_eq!(flex_mode_for("USB", Some(&offered)).as_deref(), Some("USB"));
        assert_eq!(
            flex_mode_for("PKTUSB", Some(&offered)),
            None,
            "this radio's list has no DIGU"
        );
    }

    /// A width keeps the operator's own edge on the side the slice already sits on, and starts
    /// from the slice's frequency on a new side.
    #[test]
    fn a_width_keeps_the_edge_the_slice_already_has_on_its_side() {
        // USB 100..2900 to DIGU 3000: the low edge stays.
        assert_eq!(
            filter_for("DIGU", 3000, Some("USB"), Some((100, 2900))),
            Some((100, 3100))
        );
        // LSB to DIGU: a new side starts at the slice's frequency.
        assert_eq!(
            filter_for("DIGU", 3000, Some("LSB"), Some((-2900, -100))),
            Some((0, 3000))
        );
        // The lower side keeps its high edge.
        assert_eq!(
            filter_for("DIGL", 3000, Some("LSB"), Some((-2900, -100))),
            Some((-3100, -100))
        );
        // AM is centred: a 6 kHz width around the slice's own centre.
        assert_eq!(
            filter_for("AM", 6000, Some("USB"), Some((100, 2900))),
            Some((-3000, 3000))
        );
        assert_eq!(
            filter_for("CW", 500, Some("CW"), Some((400, 1000))),
            Some((450, 950))
        );
        // Nothing to place a width on, or not a width.
        assert_eq!(filter_for("RTTY", 500, None, None), None);
        assert_eq!(filter_for("USB", 0, None, None), None);
        assert_eq!(filter_for("USB", 200_000, None, None), None);
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
