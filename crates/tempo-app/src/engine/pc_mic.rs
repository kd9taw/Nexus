//! The station computer's own microphone, on the air while the operator holds Phone's PTT.
//!
//! With a radio's "Use this computer's microphone" switch on ([`crate::settings::RadioProfile::pc_mic`]),
//! Phone's PTT press ARMS a microphone over instead of keying the radio's own microphone: the radio
//! loop opens the chosen input device, its audio is fed to the radio's transmit audio route (the
//! sound card's output, or the TCI connection on a TCI radio), and the radio is keyed as it is for
//! any audio Nexus plays. Off (the default) the press keys the radio's own microphone exactly as it
//! always has.
//!
//! It is the Remote microphone's machinery ([`crate::mic`]) with a sound card's numbers
//! ([`PC_MIC`]): a latched transmission with no precomputed end, so every gate is re-checked on every
//! radio-loop tick ([`Engine::poll_pc_mic`]) and every failure DROPS the over and unkeys.
//!
//! ## The safety rules, and where each is held
//!
//! - **Only while the operator's PTT is held.** The press arms ([`Engine::arm_pc_mic`], from
//!   [`Engine::set_ptt`]); the release ends the over at once ([`Engine::release_pc_mic`]): the feed
//!   closes, the queued audio is flushed and the radio unkeys on the next tick. No tail is played
//!   out: the operator let go. The microphone is not even open while no over is armed.
//! - **Never keys by itself.** An armed over keys on the first audio the microphone delivers after
//!   the press, and on nothing else; a press that never gets audio keys nothing.
//! - **Every TX gate, every tick.** The TX-enable latch, the licence privileges, no tune, the Phone
//!   section, this radio's switch still on, the same radio, the same transmit route, the 10-minute
//!   ceiling and the wall-clock TX watchdog. Stop TX ([`Engine::halt_tx`]) and TX Off
//!   ([`Engine::set_tx_enabled`]) drop it through the one kill path, [`Engine::drop_pc_mic`].
//! - **A microphone that goes away unkeys.** The loop reports a capture stream the OS killed
//!   ([`Engine::pc_mic_device_failed`]) and the over ends on that tick; one that just stops
//!   delivering ends at [`PC_MIC`]'s gap.
//! - **The digital modes never take it.** Only Phone arms it, and leaving Phone drops it.
//! - **No feedback path.** The microphone's audio goes to the transmit route and nowhere else: never
//!   to the headphone monitor, which plays only received audio. A microphone that is the radio's own
//!   receive input is refused (its audio would be the band).
//! - **One owner.** From the arm it owns the transmitter as the operator's held PTT
//!   ([`super::TxOwner::ManualPtt`]), so nothing else keys under it, and it arms only when nothing
//!   else holds the transmitter.
use super::{now_unix_millis, now_unix_secs, Engine};
use crate::dto::{PcMicCause, PcMicEnded};
use crate::mic::{MicDrop, MicFeed, MicFrame, MicGates, MicLatch, MicTick, PC_MIC};
use crate::settings::OperatingMode;
use std::time::Instant;

/// One frame of the microphone, 20 ms at the transmit route's rate: the size [`PC_MIC`]'s feed cap
/// counts in.
const FRAME: usize = PC_MIC.samples(20) as usize;

/// The gain range the setting is clamped to, in dB.
const GAIN_DB_MIN: f32 = -20.0;
const GAIN_DB_MAX: f32 = 20.0;

/// The computer microphone's over and what it needs.
pub(crate) struct PcMicState {
    pub(crate) over: MicLatch,
    feed: MicFeed,
    /// One-shot abort: the loop flushes the output ring and unkeys, then clears it.
    abort: bool,
    /// The radio the over armed on: a switch to another radio ends it.
    radio: Option<u32>,
    /// The device the over armed with: choosing another ends it.
    device: String,
    /// The microphone's own sample clock since the arm, and its frame sequence.
    media: u64,
    seq: u64,
    /// Audio short of a whole frame, carried to the next delivery.
    partial: Vec<f32>,
    /// Moves at every arm: the radio loop closes and reopens the microphone for a new over, so
    /// nothing captured before a quick re-press reaches it, and the old over's stop is taken first.
    gen: u64,
    /// Why the last over ended or never keyed, when the operator did not choose it (the status lane).
    pub(crate) ended: Option<PcMicEnded>,
}

impl Default for PcMicState {
    fn default() -> Self {
        Self {
            over: MicLatch::new(PC_MIC),
            feed: MicFeed::new(&PC_MIC),
            abort: false,
            radio: None,
            device: String::new(),
            media: 0,
            seq: 0,
            partial: Vec::new(),
            gen: 0,
            ended: None,
        }
    }
}

/// The capture the radio loop must hold open for an armed or keyed over: which input device, and
/// nothing else (the gain is the engine's, applied as the audio is framed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcMicSource {
    pub device: String,
    /// The over this capture is for ([`PcMicState::gen`]): a new one is a new capture.
    pub gen: u64,
}

impl Engine {
    /// Whether the operator's PTT press feeds this computer's microphone: the active radio's switch
    /// is on, and the station is in Phone. Nothing else (FT, CW, the keyboard modes) ever does.
    pub fn pc_mic_wanted(&self) -> bool {
        self.settings.pc_mic && self.settings.operating_mode == OperatingMode::Phone
    }

    /// Whether a computer-microphone over is armed or keyed.
    pub fn pc_mic_armed(&self) -> bool {
        self.pc_mic.over.active()
    }

    /// Whether a computer-microphone over has keyed the radio.
    pub fn pc_mic_keyed(&self) -> bool {
        self.pc_mic.over.keyed()
    }

    /// The operator pressed PTT with the computer's microphone on: arm its over. Keys NOTHING: the
    /// first audio from the microphone does, on the radio loop. Returns whether an over is armed
    /// after. Refused, and nothing armed, unless every gate is up and nothing else holds the
    /// transmitter.
    pub(super) fn arm_pc_mic(&mut self) -> bool {
        if self.pc_mic.over.active() {
            return true;
        }
        if !self.pc_mic_wanted()
            || !self.tx_enabled
            || !self.tx_allowed()
            || self.tuning
            || self.clock_repair_holds_tx()
            || self.remote_transmit_held()
            || self.connection_tx_refusal().is_some()
        {
            return false;
        }
        // Something else holds the transmitter (a voice-keyer message queued or playing among
        // them): two sources would share one output ring, so the press arms nothing, and says so.
        if let Some(owner) = self.tx_owner() {
            tempo_core::applog::info(
                "tx",
                &format!(
                    "{} over refused: {owner:?} holds the transmitter",
                    PC_MIC.name
                ),
            );
            self.note_pc_mic(PcMicCause::Busy);
            return false;
        }
        // Nexus's own Flex client gives the radio its own mic in Phone at the shack, and audio
        // Nexus plays leaves over DAX, which the radio is then not listening to: the operator
        // would be on the radio's mic believing they were on this one. The voice keyer's switch
        // to DAX around a message is not reused: it carries at most the client's DAX queue
        // (`FLEX_VOICE_MAX_SAMPLES`), and a held over has no length. Refused, as APRS and SSTV are.
        if self.flex_radio_has_mic {
            self.note_pc_mic(PcMicCause::FlexMic);
            return false;
        }
        let device = self.settings.pc_mic_device.trim().to_string();
        if device.is_empty() {
            self.note_pc_mic(PcMicCause::NoDevice);
            return false;
        }
        if let Some(cause) = self.pc_mic_feedback(&device) {
            self.note_pc_mic(cause);
            return false;
        }
        self.pc_mic.over.arm();
        self.pc_mic.gen += 1;
        self.pc_mic.feed.open();
        self.pc_mic.radio = Some(self.settings.active_radio);
        self.pc_mic.device = device;
        self.pc_mic.media = 0;
        self.pc_mic.seq = 0;
        self.pc_mic.partial.clear();
        // A press is an operator action: the unattended-transmit clock restarts here.
        self.reset_tx_watchdog();
        true
    }

    /// The operator let go of PTT: the over ends NOW, with no tail played out.
    pub fn release_pc_mic(&mut self) {
        self.drop_pc_mic();
    }

    /// End the computer-microphone over NOW: the ONE kill path. The feed closes first, so nothing
    /// captured for this over can reach the next; the abort is armed only when the radio was keyed.
    pub fn drop_pc_mic(&mut self) {
        self.pc_mic.feed.close();
        self.pc_mic.partial.clear();
        self.pc_mic.radio = None;
        if self.pc_mic.over.drop_latch() {
            self.pc_mic.abort = true;
        }
    }

    /// [`Self::drop_pc_mic`], recording why for the operator.
    fn drop_pc_mic_for(&mut self, cause: PcMicCause) {
        let was_active = self.pc_mic.over.active();
        self.drop_pc_mic();
        if was_active {
            self.note_pc_mic(cause);
        }
    }

    fn note_pc_mic(&mut self, cause: PcMicCause) {
        tempo_core::applog::info(
            "tx",
            &format!("{} over not keyed or ended: {cause:?}", PC_MIC.name),
        );
        self.pc_mic.ended = Some(PcMicEnded {
            at: now_unix_secs(),
            cause,
        });
    }

    /// Why `device` may not be this computer's microphone, if it may not: it carries receive audio
    /// or Nexus's own, which would go back out on the air (a feedback path). Refuses when in doubt:
    /// any radio's receive input, under any name for the same card ([`same_audio_device`]); a Flex
    /// DAX receive channel; and anything named like a loopback or a virtual cable. The input the
    /// sound card ACTUALLY opened for receive (the system default when none is set) is checked by
    /// the radio loop before it opens the microphone ([`Self::pc_mic_is_receive`]).
    fn pc_mic_feedback(&self, device: &str) -> Option<PcMicCause> {
        if same_audio_device(device, &self.settings.audio_in) {
            return Some(PcMicCause::SameAsReceive);
        }
        for radio in &self.settings.radios {
            if same_audio_device(device, &radio.audio_in) {
                return Some(if radio.id == self.settings.active_radio {
                    PcMicCause::SameAsReceive
                } else {
                    PcMicCause::RadioReceive
                });
            }
        }
        let name = device.to_lowercase();
        if DAX_RECEIVE.iter().any(|n| name.contains(n)) {
            return Some(PcMicCause::RadioReceive);
        }
        if LOOPBACK.iter().any(|n| name.contains(n)) || name.ends_with(".monitor") {
            return Some(PcMicCause::Loopback);
        }
        None
    }

    /// The radio loop found the microphone is the input the sound card opened for receive (the
    /// system default, when no receive input is set): the over ends before the microphone opens.
    pub fn pc_mic_is_receive(&mut self) {
        self.drop_pc_mic_for(PcMicCause::SameAsReceive);
    }

    /// The radio loop could not carry the over on: something else unkeyed the radio under it, the
    /// radio is not the operator's to key this tick (a handoff, a CAT hold, a radio switch), or the
    /// loop was rebuilt under it. The over ends, and the lane says so.
    pub fn pc_mic_interrupted(&mut self) {
        self.drop_pc_mic_for(PcMicCause::Interrupted);
    }

    /// The radio did not accept the key for the over's first audio: nothing was sent.
    pub fn pc_mic_key_refused(&mut self) {
        self.drop_pc_mic_for(PcMicCause::KeyRefused);
    }

    /// Take + reset the one-shot abort (the loop flushes the output and unkeys).
    pub fn take_pc_mic_abort(&mut self) -> bool {
        std::mem::take(&mut self.pc_mic.abort)
    }

    /// The capture the radio loop must hold open: `Some` exactly while an over is armed or keyed.
    pub fn pc_mic_source(&self) -> Option<PcMicSource> {
        self.pc_mic.over.active().then(|| PcMicSource {
            device: self.pc_mic.device.clone(),
            gen: self.pc_mic.gen,
        })
    }

    /// The microphone could not be opened (`lost` false) or the OS killed it mid-over (`lost`
    /// true): the over ends on this tick.
    pub fn pc_mic_device_failed(&mut self, lost: bool) {
        self.drop_pc_mic_for(if lost {
            PcMicCause::DeviceLost
        } else {
            PcMicCause::OpenFailed
        });
    }

    /// Audio the microphone delivered (12 kHz mono, as captured), arriving `at`: the gain applied,
    /// clipped to full scale, and offered to the over in 20 ms frames. The feed takes nothing while
    /// no over is armed, so audio captured before the press or after the release goes nowhere.
    pub fn push_pc_mic(&mut self, samples: &[f32], at: Instant) {
        if !self.pc_mic.feed.is_open() {
            return;
        }
        let gain_db = self.settings.pc_mic_gain_db;
        let gain_db = if gain_db.is_finite() {
            gain_db.clamp(GAIN_DB_MIN, GAIN_DB_MAX)
        } else {
            0.0
        };
        let gain = 10f32.powf(gain_db / 20.0);
        self.pc_mic.partial.extend(samples.iter().map(|&x| {
            if x.is_finite() {
                (x * gain).clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }));
        while self.pc_mic.partial.len() >= FRAME {
            let rest = self.pc_mic.partial.split_off(FRAME);
            let frame = std::mem::replace(&mut self.pc_mic.partial, rest);
            self.pc_mic.seq += 1;
            let media = self.pc_mic.media;
            self.pc_mic.media += FRAME as u64;
            // Digital silence (every sample exactly zero) is no audio: a live microphone always
            // carries some noise, and a stalled or muted device that keeps calling back with zeros
            // must not hold the radio keyed. It keys nothing, and the gap ends an over that gets
            // only that.
            if frame.iter().all(|&x| x == 0.0) {
                continue;
            }
            // A frame the feed refuses (full: the loop stopped reading) is a moment of silence, as
            // a lost one is; the gap ends an over that gets none.
            let _ = self.pc_mic.feed.push(MicFrame {
                seq: self.pc_mic.seq,
                media,
                arrived: at,
                samples: frame,
            });
        }
    }

    /// One radio-loop tick of the computer-microphone over: the ONLY path from the microphone to
    /// the transmitter, and its per-tick predicate ([`MicLatch::tick`]). `budget_ms` is what the
    /// output ring can take this tick (clamped to [`PC_MIC`]'s per-push cap here too); `route` is
    /// the loop's transmit-route generation.
    pub fn poll_pc_mic(&mut self, now: Instant, budget_ms: f64, route: u64) -> MicTick {
        if !self.pc_mic.over.active() {
            return MicTick::Idle;
        }
        // The section gate carries this feature's own conditions too: Phone, this radio's switch
        // still on, the radio the over armed on, and the microphone it armed with.
        // The radio taking its own mic (Nexus's Flex client) ends it too ([`Self::arm_pc_mic`]).
        let same_source = self.pc_mic.radio == Some(self.settings.active_radio)
            && self.settings.pc_mic_device.trim() == self.pc_mic.device;
        let gates = MicGates {
            tx_enabled: self.tx_enabled,
            tx_allowed: self.tx_allowed(),
            tuning: self.tuning,
            in_section: self.pc_mic_wanted() && same_source && !self.flex_radio_has_mic,
            // The operator's own hold: the release ends the over directly, so while it is armed
            // the hold is held.
            presence: true,
            route,
            now,
            now_ms: now_unix_millis(),
            now_secs: now_unix_secs(),
            watchdog_limit_secs: self.settings.tx_watchdog_min as u64 * 60,
            watchdog_start_secs: self.tx_watchdog_start,
        };
        let arrived = self.pc_mic.feed.take();
        let budget_ms = budget_ms.clamp(0.0, PC_MIC.max_push_ms as f64);
        let budget = (budget_ms * f64::from(PC_MIC.rate_hz) / 1000.0) as usize;
        let res = self.pc_mic.over.tick(gates, arrived, budget);
        if let Some(start) = res.watchdog_start {
            self.tx_watchdog_start = Some(start);
        }
        if let Some(why) = res.drop {
            match why {
                MicDrop::Watchdog => {
                    // A trip disarms TX so it stays stopped, as every other over's does.
                    self.tx_watchdog = true;
                    self.tx_enabled = false;
                    self.drop_pc_mic_for(PcMicCause::Watchdog);
                }
                MicDrop::Ceiling => self.drop_pc_mic_for(PcMicCause::Ceiling),
                MicDrop::AudioGap => self.drop_pc_mic_for(PcMicCause::NoAudio),
                // A gate went down under the over (TX Off, a QSY out of the licence, a tune,
                // leaving Phone, the switch, another radio or microphone, a new transmit route,
                // the radio taking its own mic): the lane says the over ended on one, since the
                // operator's PTT is still held.
                MicDrop::GateDown | MicDrop::DeviceChanged => {
                    self.drop_pc_mic_for(PcMicCause::GateDown)
                }
                // Never raised here: the hold is the operator's own (`presence` is always up), and
                // nothing releases this over gracefully. Ended all the same.
                MicDrop::Presence | MicDrop::Drained => self.drop_pc_mic(),
            }
            tempo_core::applog::info("tx", &format!("{} over ended: {why:?}", PC_MIC.name));
        }
        if res.tick == MicTick::Key {
            self.pc_mic.ended = None; // this over keyed: the last one's reason is old news
        }
        res.tick
    }
}

/// Input names that are a Flex radio's DAX receive channels (lower case): band audio.
const DAX_RECEIVE: [&str; 3] = ["dax audio rx", "dax rx", "dax iq"];

/// Input names that are a loopback or a virtual cable (lower case): what plays on this computer,
/// Nexus's own transmit audio among it, comes back in on them. A PulseAudio/PipeWire monitor source
/// is also caught by its `.monitor` name.
const LOOPBACK: [&str; 13] = [
    "stereo mix",
    "what u hear",
    "wave out mix",
    "cable output",
    "cable-a output",
    "cable-b output",
    "virtual audio cable",
    "vb-audio",
    "voicemeeter",
    "loopback",
    "monitor of ",
    "blackhole",
    "soundflower",
];

/// Whether two saved device names may be the same input. Refuses when in doubt: names compare
/// trimmed and case-blind, without the ` #N` ordinal Nexus adds to identically named devices (two
/// codecs of one name are not told apart), and ALSA names of one card compare equal under any
/// access path (`hw:` / `plughw:` / `dsnoop:` / `default:` / `sysdefault:` …, by `CARD=` or by card
/// number). An empty name is no device.
pub fn same_audio_device(a: &str, b: &str) -> bool {
    let (a, b) = (device_base(a), device_base(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    matches!((alsa_card(&a), alsa_card(&b)), (Some(x), Some(y)) if x == y)
}

/// A saved name trimmed, lower case, without a trailing ` #N` ordinal.
fn device_base(name: &str) -> String {
    let name = name.trim();
    let base = match name.rsplit_once(" #") {
        Some((base, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => base,
        _ => name,
    };
    base.trim().to_lowercase()
}

/// The card an ALSA device name addresses (lower case), whatever its access path: the `CARD=`
/// token's value, or the card number of `hw:1,0`. `None` for a name that is no ALSA PCM name.
fn alsa_card(name: &str) -> Option<String> {
    if let Some(at) = name.find("card=") {
        let rest = &name[at + 5..];
        return Some(
            rest[..rest.find(',').unwrap_or(rest.len())]
                .trim()
                .to_string(),
        );
    }
    let (path, rest) = name.split_once(':')?;
    const PATHS: [&str; 8] = [
        "hw",
        "plughw",
        "dsnoop",
        "dmix",
        "default",
        "sysdefault",
        "front",
        "iec958",
    ];
    if !PATHS.contains(&path.trim()) {
        return None;
    }
    let card = rest[..rest.find(',').unwrap_or(rest.len())].trim();
    (!card.is_empty()).then(|| card.to_string())
}

#[cfg(test)]
#[path = "pc_mic_tests.rs"]
mod tests;
