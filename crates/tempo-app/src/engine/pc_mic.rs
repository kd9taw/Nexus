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
//!   [`Engine::set_ptt`]); the release closes the feed at once, so nothing captured after it is
//!   sent, and lets what the over already holds play out for at most [`RELEASE_TAIL_MS`] by the
//!   wall clock before the flush + unkey ([`Engine::release_pc_mic_at`]), so the end of the last
//!   word is not clipped. The microphone is not even open while no over is armed.
//! - **Never keys by itself.** An armed over keys on the first audio the microphone delivers after
//!   the press, and on nothing else; a press that never gets audio keys nothing.
//! - **Every TX gate, every tick.** The TX-enable latch, the licence privileges, no tune, the Phone
//!   section, this radio's switch still on, the same radio, the same transmit route, the 10-minute
//!   ceiling and the wall-clock TX watchdog, the release's tail included. Stop TX
//!   ([`Engine::halt_tx`]) and TX Off ([`Engine::set_tx_enabled`]) drop it through the one kill
//!   path, [`Engine::drop_pc_mic`], at once, tail or no tail.
//! - **A microphone that goes away unkeys.** The loop reports a capture stream the OS killed
//!   ([`Engine::pc_mic_device_failed`]) and the over ends on that tick; one that just stops
//!   delivering ends at [`PC_MIC`]'s gap; one that sends only digital silence keys nothing, and
//!   ends a keyed over after [`DIGITAL_SILENCE_LIMIT_MS`] of it.
//! - **The digital modes never take it.** Only Phone arms it, and leaving Phone drops it.
//! - **No feedback path.** The microphone's audio goes to the transmit route and nowhere else: never
//!   to the headphone monitor, which plays only received audio. A microphone that is the radio's own
//!   receive input is refused (its audio would be the band).
//! - **One owner.** From the arm it owns the transmitter as the operator's held PTT
//!   ([`super::TxOwner::ManualPtt`]), so nothing else keys under it, and it arms only when nothing
//!   else holds the transmitter.
//!
//! ## On a Flex served by Nexus's own client (native audio on, Phone at the shack)
//! The client gives the radio its own mic in Phone, and audio Nexus plays reaches the radio only as
//! DAX. So an over pressed then rides a DAX HOLD for as long as the operator holds PTT: the press
//! arms it on the hold ([`Engine::pc_mic_dax_hold`]), the radio loop asks the client to switch the
//! radio to DAX, and nothing of the microphone reaches the over until the radio reports DAX on
//! Nexus's own stream ([`Engine::observe_pc_mic_dax`]): until then it would key the room on the
//! radio's mic. No DAX within the client's confirm window ends the press with nothing keyed
//! ([`Engine::pc_mic_no_dax`]), and the radio leaving DAX under a keyed over ends it at once. The
//! hold has no length of its own: it ends with the over, by every stop above, and the client puts
//! the mic back only after the radio has proved the unkey. The voice keyer's switch is the same
//! switch, bounded by its message instead.
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

/// How long a KEYED over may receive nothing but digital silence (every sample exactly zero) before
/// it ends as no audio. A microphone with a noise gate (a headset's own, or a noise-suppressing
/// virtual microphone) sends exact zeros between words, so those count as audio for the over's gap;
/// a stalled or muted device that keeps calling back with zeros still ends the over here.
const DIGITAL_SILENCE_LIMIT_MS: u64 = 4_000;

/// The most of the microphone's audio a PTT release lets play out before the radio unkeys.
pub const RELEASE_TAIL_MS: u64 = 200;

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
    /// When the keyed over's unbroken run of digital silence began ([`DIGITAL_SILENCE_LIMIT_MS`]).
    silent_since: Option<Instant>,
    /// Why the last over ended or never keyed, when the operator did not choose it (the status lane).
    pub(crate) ended: Option<PcMicEnded>,
    /// The PTT release's bound on the wall clock: the keyed over unkeys by then at the latest.
    pub(crate) tail: Option<Instant>,
    /// What the feed held at the release (captured before it), for the next tick.
    held: Vec<MicFrame>,
    /// The over armed on the Flex client's DAX hold, at the press ([`Engine::pc_mic_dax_hold`]).
    dax_hold: Option<Instant>,
    /// The radio loop's word, each tick, that the radio takes this over's audio from Nexus's DAX
    /// ([`Engine::observe_pc_mic_dax`]). Read only while `dax_hold` stands.
    on_dax: bool,
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
            silent_since: None,
            ended: None,
            tail: None,
            held: Vec::new(),
            dax_hold: None,
            on_dax: false,
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

/// What the device guard says of two input names (`tempo_audio::notify_sound::same_input`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameInput {
    /// One device, under these names.
    Same,
    /// Not provably different: refused as if the same.
    Unsure,
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
            if self.pc_mic.tail.is_none() {
                return true;
            }
            // A press during the release's tail: the tail is cut (flush + unkey) and a fresh over
            // arms, on its own capture, as a quick re-press always has.
            self.drop_pc_mic();
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
        self.pc_mic.silent_since = None;
        // ⛔ Nexus's own Flex client has given the radio its own mic (Phone at the shack), and audio
        // Nexus plays reaches the radio only as DAX: the over rides the client's switch to DAX for
        // as long as PTT is held, and keys nothing until the radio reports it (module header).
        self.pc_mic.dax_hold = self.flex_radio_has_mic.then(Instant::now);
        self.pc_mic.on_dax = false;
        // A press is an operator action: the unattended-transmit clock restarts here.
        self.reset_tx_watchdog();
        true
    }

    /// The operator let go of PTT: the end of the last word plays out, then the radio unkeys
    /// ([`Self::release_pc_mic_at`], timed from now on the wall clock).
    pub fn release_pc_mic(&mut self) {
        self.release_pc_mic_at(Instant::now());
    }

    /// ⛔ THE RELEASE TAIL (operator ruling, 2026-10-28: "Let ≤200 ms finish"). The operator let go
    /// of PTT at `at`: the microphone audio the over already holds plays out, then the radio
    /// unkeys, so the end of the last word is not clipped. Bounded three ways, none of them "until
    /// the queue empties":
    /// - nothing captured after the release is sent: the feed closes HERE (what arrived before the
    ///   release, the part of a frame included, is kept for the over);
    /// - no sample that would play [`RELEASE_TAIL_MS`] or more after `at` is ever fed
    ///   ([`MicLatch::release_by`]);
    /// - [`Self::poll_pc_mic`] ends the over (flush + unkey) at `at + RELEASE_TAIL_MS` by the wall
    ///   clock whatever is left, and on the first tick after a late or wedged loop comes back.
    ///
    /// Only the release. Stop TX, TX Off, the watchdog, a gate going down, a handoff and every other
    /// stop still end the over at once ([`Self::drop_pc_mic`]), tail or no tail; a new press cuts
    /// the tail and starts a fresh over. An over that never keyed has put nothing on the air and
    /// ends now. A second release never moves the bound.
    pub fn release_pc_mic_at(&mut self, at: Instant) {
        if self.pc_mic.tail.is_some() {
            return;
        }
        if !self.pc_mic.over.keyed() {
            self.drop_pc_mic();
            return;
        }
        // The part of a frame captured before the release, made whole with silence.
        if !self.pc_mic.partial.is_empty() {
            let mut frame = std::mem::take(&mut self.pc_mic.partial);
            frame.resize(FRAME, 0.0);
            self.offer_pc_mic_frame(frame, at);
        }
        self.pc_mic.held = self.pc_mic.feed.take();
        self.pc_mic.feed.close();
        let until = at + std::time::Duration::from_millis(RELEASE_TAIL_MS);
        self.pc_mic.over.release_by(until);
        self.pc_mic.tail = Some(until);
    }

    /// End the computer-microphone over NOW: the ONE kill path. The feed closes first, so nothing
    /// captured for this over can reach the next; the abort is armed only when the radio was keyed.
    pub fn drop_pc_mic(&mut self) {
        self.pc_mic.feed.close();
        self.pc_mic.partial.clear();
        self.pc_mic.held.clear();
        self.pc_mic.tail = None;
        self.pc_mic.radio = None;
        if self.pc_mic.over.drop_latch() {
            self.pc_mic.abort = true;
        }
    }

    /// [`Self::drop_pc_mic`], recording why for the operator.
    fn drop_pc_mic_for(&mut self, cause: PcMicCause) {
        // In the release's tail the over was ending by the operator's own act: only a watchdog
        // trip, which also turned TX off, is worth the operator's attention.
        let note = self.pc_mic.over.active()
            && (self.pc_mic.tail.is_none() || cause == PcMicCause::Watchdog);
        self.drop_pc_mic();
        if note {
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

    /// Why `device` may not be this computer's microphone by its NAME alone, if it may not: it
    /// carries receive audio or Nexus's own, which would go back out on the air (a feedback path).
    /// A Flex DAX receive channel, and anything named like a loopback or a virtual cable. Whether
    /// it is a receive input under another name is the device guard's to say, asked by the radio
    /// loop before it opens the microphone ([`Self::pc_mic_receive_refusal`]).
    fn pc_mic_feedback(&self, device: &str) -> Option<PcMicCause> {
        let name = device.to_lowercase();
        if DAX_RECEIVE.iter().any(|n| name.contains(n)) {
            return Some(PcMicCause::RadioReceive);
        }
        if LOOPBACK.iter().any(|n| name.contains(n)) || name.ends_with(".monitor") {
            return Some(PcMicCause::Loopback);
        }
        None
    }

    /// ⛔ Why the armed over's microphone `device` may not be opened, if it may not: it may be an
    /// input that carries the band. `same_input` is THE device guard (the one the notification
    /// sounds are refused by, `tempo_audio::notify_sound::same_input`), asked against the system
    /// default input (`""`), this station's receive input, every radio's, and the input the sound
    /// card actually opened for receive (`opened_receive`). A device that IS one of them names
    /// whose receive it is; one the guard cannot tell apart from one of them is refused as unsure.
    pub fn pc_mic_receive_refusal(
        &self,
        device: &str,
        opened_receive: Option<&str>,
        same_input: &dyn Fn(&str, &str) -> Option<SameInput>,
    ) -> Option<PcMicCause> {
        let mut inputs = vec![
            ("", PcMicCause::SameAsReceive),
            (self.settings.audio_in.as_str(), PcMicCause::SameAsReceive),
        ];
        inputs.extend(self.settings.radios.iter().map(|radio| {
            let cause = if radio.id == self.settings.active_radio {
                PcMicCause::SameAsReceive
            } else {
                PcMicCause::RadioReceive
            };
            (radio.audio_in.as_str(), cause)
        }));
        inputs.extend(opened_receive.map(|rx| (rx, PcMicCause::SameAsReceive)));
        let mut unsure = false;
        for (input, cause) in inputs {
            match same_input(device, input) {
                Some(SameInput::Same) => return Some(cause),
                Some(SameInput::Unsure) => unsure = true,
                None => {}
            }
        }
        unsure.then_some(PcMicCause::Unsure)
    }

    /// The radio loop refused the armed over's microphone before opening it
    /// ([`Self::pc_mic_receive_refusal`]): the over ends, nothing keyed, and the lane says why.
    pub fn pc_mic_refused(&mut self, cause: PcMicCause) {
        self.drop_pc_mic_for(cause);
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

    /// When the operator pressed PTT for the armed or keyed over that rides Nexus's Flex client's
    /// switch to DAX ([`Self::arm_pc_mic`], the radio having had its own mic); `None` for any other
    /// over, or none. The radio loop's cue to ask the client for DAX at its quiet point, to watch
    /// the radio take it before the over may key and keep it while keyed
    /// ([`Self::observe_pc_mic_dax`]), and to give up on it after the client's confirm window
    /// ([`Self::pc_mic_no_dax`]).
    pub fn pc_mic_dax_hold(&self) -> Option<Instant> {
        self.pc_mic.dax_hold.filter(|_| self.pc_mic.over.active())
    }

    /// The radio loop's word, before each [`Self::poll_pc_mic`], on whether the radio takes the
    /// over on the DAX hold's audio from Nexus's DAX: before the key, it reports DAX on Nexus's own
    /// transmit stream, the route is in, and nothing is keyed; while keyed, it still reports DAX on
    /// that stream. Until it does, no audio reaches the over; once keyed, its going ends the over.
    pub fn observe_pc_mic_dax(&mut self, on_dax: bool) {
        self.pc_mic.on_dax = on_dax;
    }

    /// The radio did not switch to DAX for the over on the hold within the client's confirm window:
    /// the press ends, nothing was keyed, and the lane says why.
    pub fn pc_mic_no_dax(&mut self) {
        if !self.pc_mic.over.keyed() {
            self.drop_pc_mic_for(PcMicCause::FlexMic);
        }
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
            // Digital silence (every sample exactly zero) never STARTS an over: a stalled or muted
            // device that keeps calling back with zeros keys nothing. Once the over is keyed it is
            // the pause between words of a gated microphone, so it holds the gap like any audio;
            // an unbroken run of it ends the over at `DIGITAL_SILENCE_LIMIT_MS` (`poll_pc_mic`).
            if frame.iter().all(|&x| x == 0.0) {
                if !self.pc_mic.over.keyed() {
                    // Its moment passes on the microphone's clock all the same.
                    self.pc_mic.seq += 1;
                    self.pc_mic.media += FRAME as u64;
                    continue;
                }
                self.pc_mic.silent_since.get_or_insert(at);
            } else {
                self.pc_mic.silent_since = None;
            }
            self.offer_pc_mic_frame(frame, at);
        }
    }

    /// Offer one whole frame to the over's feed, next on its sample clock. A frame the feed
    /// refuses (closed, or full: the loop stopped reading) is a moment of silence, as a lost one
    /// is; the gap ends an over that gets none.
    fn offer_pc_mic_frame(&mut self, samples: Vec<f32>, at: Instant) {
        self.pc_mic.seq += 1;
        let media = self.pc_mic.media;
        self.pc_mic.media += FRAME as u64;
        let _ = self.pc_mic.feed.push(MicFrame {
            seq: self.pc_mic.seq,
            media,
            arrived: at,
            samples,
        });
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
        // still on, the radio the over armed on, and the microphone it armed with. The radio taking
        // its own mic (Nexus's Flex client) ends an over that did not arm on the DAX hold; one that
        // did is watched for DAX itself, below.
        let hold = self.pc_mic.dax_hold.is_some();
        let was_keyed = self.pc_mic.over.keyed();
        let same_source = self.pc_mic.radio == Some(self.settings.active_radio)
            && self.settings.pc_mic_device.trim() == self.pc_mic.device;
        let gates = MicGates {
            tx_enabled: self.tx_enabled,
            tx_allowed: self.tx_allowed(),
            tuning: self.tuning,
            in_section: self.pc_mic_wanted() && same_source && (hold || !self.flex_radio_has_mic),
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
        let mut arrived = std::mem::take(&mut self.pc_mic.held);
        arrived.extend(self.pc_mic.feed.take());
        // ⛔ THE DAX HOLD, BEFORE THE KEY: until the radio takes Nexus's DAX, the microphone's audio
        // goes nowhere (keying on it would put the room on the air through the radio's own mic).
        // Every gate is still judged on this tick.
        let waiting = hold && !was_keyed && !self.pc_mic.on_dax;
        if waiting {
            arrived.clear();
            self.pc_mic.partial.clear();
        }
        let budget_ms = budget_ms.clamp(0.0, PC_MIC.max_push_ms as f64);
        let budget = (budget_ms * f64::from(PC_MIC.rate_hz) / 1000.0) as usize;
        let mut res = self.pc_mic.over.tick(gates, arrived, budget);
        if waiting {
            // The client's transmit route coming up for the switch is no change of route: the over
            // takes its route on the tick the radio is on DAX, the route then in place.
            self.pc_mic.over.unpin_route();
        }
        if let Some(start) = res.watchdog_start {
            self.tx_watchdog_start = Some(start);
        }
        // ⛔ THE DAX HOLD, WHILE KEYED: the radio no longer takes Nexus's DAX (SmartSDR's own
        // switch, the stream gone), so the over's audio would not reach the air and the radio's
        // mic would: off the air now, release tail or not, with this tick's audio not played.
        if res.drop.is_none() && hold && was_keyed && !self.pc_mic.on_dax {
            res.drop = Some(MicDrop::DeviceChanged);
            res.tick = MicTick::Idle;
        }
        // The release's tail ([`Self::release_pc_mic_at`]). The latch has already judged every
        // gate, the ceiling and the watchdog on this tick; a drop it raised stands.
        if let Some(until) = self.pc_mic.tail {
            if res.drop.is_none() && now >= until {
                // ⛔ The wall clock: off the air now, whatever is left, and nothing more is fed
                // (this tick's audio is not played; the loop gets `Idle` and the abort).
                res.drop = Some(MicDrop::Drained);
                res.tick = MicTick::Idle;
            } else if res.drop == Some(MicDrop::Drained)
                && now < until
                && self
                    .pc_mic
                    .over
                    .fed_until()
                    .is_some_and(|played| now < played)
            {
                // Everything is fed and still playing: stay keyed until it has.
                res.drop = None;
                res.tick = MicTick::Ahead;
            }
        }
        if res.drop.is_none()
            && self.pc_mic.tail.is_none()
            && self.pc_mic.over.keyed()
            && self.pc_mic.silent_since.is_some_and(|since| {
                now.saturating_duration_since(since)
                    >= std::time::Duration::from_millis(DIGITAL_SILENCE_LIMIT_MS)
            })
        {
            // Nothing but digital silence for the limit: the device has stalled, as far as anyone
            // can tell. This tick's audio is not played (the loop gets `Idle` and the abort).
            res.drop = Some(MicDrop::AudioGap);
            res.tick = MicTick::Idle;
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
                // The release's tail has played out, or reached its bound: the flush + unkey, with
                // nothing left to flush but what the clock cut. (`Presence` is never raised here:
                // the hold is the operator's own.)
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

#[cfg(test)]
#[path = "pc_mic_tests.rs"]
mod tests;
