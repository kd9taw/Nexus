//! Programs on Nexus's TCI server, on the engine: what a program may key, the over that carries its
//! audio, and every stop that ends it (the operator's rulings of
//! 2026-10-19). The server (`tempo_net::tci::server`) refuses outright what it can on its own; every
//! command that could put the transmitter on the air reaches the engine as a verdict
//! ([`Engine::tci_key`], [`Engine::tci_tune_on`]), called by the app's backend under the engine
//! lock. A program is named by its connection's number on the server (`ClientId::get`).
//!
//! ## What a program can never do
//!
//! TCI adds refusals; it removes no gate. Nothing here sets `tx_enabled`, clears the stop's hold, a
//! watchdog trip or the context hold, or starts or restarts the watchdog clock: a program's key
//! passes every station gate first and TCI's own terms after, and a program never arms, as a WSJT-X
//! UDP request never does. No program raises the power, moves the section, retunes while anything
//! is on the air, or keys outside the licence, which judges the whole transmit passband of a
//! program's audio, because Nexus cannot know where in it the program puts its signal.
//!
//! ## The verdict, in order
//!
//! | # | Gate | Refusal |
//! |---|---|---|
//! | 2 | "Programs on TCI may transmit" is on; for CW, the CW tier as well | `Switch`, `Tier` |
//! | 3 | no Stop held, no watchdog trip, TX armed, no context hold | `StopHold`, `WatchdogTrip`, `Disarmed`, `ContextHold` |
//! | 4 | the radio's connection can transmit; for a `tci` over, a radio that takes the audio Nexus plays, not its own microphone | `Connection`, `RadioMic` |
//! | 5 | the licence: a `tci` over across its whole passband, a microphone key as Nexus's own, Tune at its carrier, CW as Nexus's own CW; no `tci` over or microphone key where the class may send only CW and single-sideband voice, whatever the radio's mode | `Licence`, `VoiceOnly` |
//! | 6 | nothing else owns the transmitter (a program re-asserting its own key passes, and its CW queues behind its own) | `Owner` |
//! | 7 | no clock repair and no sign-in transmit hold (neither drops a key already held; CW is never admitted under either) | `ClockRepair`, `OnHold` |
//! | 8 | the section: a `tci` over or a microphone key in Phone, CW in CW, Tune in any; a `tci` over in Digital not yet, nor in any section from a program whose last `modulation:` asked a data mode; and a `tci` over only from a program whose last `modulation:` asked a voice mode | `Section`, `SlotTimed`, `VoiceMode` |
//! | 9 | a `tci` over has the program's audio stream started | `AudioStream` |
//!
//! Item 1, receiver 0, is the server's. The verdict for a `tci` key is also what each program is
//! told as `tx_enable` ([`Engine::tci_tx_enable`]), so the notice and the admission never disagree.
//!
//! ⛔ SLOT-TIMED MODES WAIT FOR THE BENCH (the operator's ruling of 2026-10-19): FT8, FT4 and the
//! other slot-timed modes from WSJT-X, JTDX or MSHV go out through the server only once the loop
//! bench has measured Nexus's added delay at or under 0.2 s and the recorded overs decode. Every
//! tier of the Digital section is slot-timed, so until then a `tci` over in Digital is refused as
//! `SlotTimed`. Allowing it is the one arm of the section rule in [`Engine::tci_verdict`].
//!
//! ⛔ AND A DATA-MODE OVER FROM A PROGRAM IS HELD LIKE FT8 (the operator's ruling of 2026-10-20), in
//! whatever section Nexus is: a program whose last `modulation:` asked a data mode (`digu`, `digl`),
//! as WSJT-X asks before its FT8 keys, has its `tci` overs refused as `SlotTimed` until it asks
//! another mode, and an over it holds ends on the tick after it asks one
//! ([`Engine::tci_note_modulation`]). Without it, FT8 from a program on the Phone screen went out
//! wherever the licence let a phone emission through (every frequency, for the Open class).
//!
//! ⛔ AND A PROGRAM'S OWN AUDIO GOES OUT ONLY AFTER IT ASKS A VOICE MODE (the operator's ruling of
//! 2026-10-20): a `tci` over is admitted only from a program whose last `modulation:` asked `usb`,
//! `lsb`, `am` or `nfm` (TCI's FM), the voice modes Nexus's server offers programs. One that has
//! asked none, or `cw` (a program's CW is text, keyed by Nexus's own keyer), is refused as
//! `VoiceMode`, after the section's own refusals, and an over it holds ends on the tick after it
//! asks anything but a voice mode. WSJT-X, JTDX and MSHV ask no mode in their default setup (their
//! Radio mode "None"), so without it their FT8 on the Phone screen went out wherever the licence
//! let a phone emission through, as a data-mode over did. A program's microphone key and its Tune
//! carry no audio of its own and are not judged on what it asked.
//!
//! ## The three kinds of key
//!
//! - **A `tci` over** (`trx:0,true,tci`): the Remote microphone's latch with a program's numbers
//!   ([`TCI_OVER`]). Armed by the key, keyed by its first audio, re-gated on every radio-loop tick
//!   ([`Engine::poll_tci`]), and ended by any gate, 300 ms with no audio, the 10-minute ceiling, a
//!   lost socket or any stop. Its owner's own `trx:0,false` is the one graceful end: what it sent
//!   plays out ([`MicLatch::release`]), because flushing would cut the last of an FT8 over.
//! - **A microphone key** (`trx:0,true` with no source or a microphone's): the radio's own
//!   microphone, Phone only, held as the CAT broker's key is ([`Engine::manual_ptt`]). Ended by its
//!   owner's release, a lost socket, any stop and the same ceiling.
//! - **Tune** (`tune:0,true`): Nexus's own tune carrier, with its licence rule and its timeout, and
//!   not the operator's act, so it never restarts the watchdog clock.
//! - **CW** (`cw_macros`, `cw_msg`): a logger's CW text, keyed by Nexus's own CW keyer in the CW
//!   section, filtered to what that keyer can send and at the programs' own speed
//!   (`engine/tci/cw.rs`, which has the detail).
//!
//! ## Where each stop reaches a program
//!
//! - `halt_tx` and TX Off end every program's key, over, tune and CW, hard
//!   ([`Engine::drop_tci_all`]): the over's feed closes, its abort is armed when it had keyed (the
//!   loop flushes and unkeys), and the stop holds, so no key is admitted until the operator's own
//!   arm. A band or radio change halts through `halt_tx`, and its context hold then refuses
//!   programs' keys until that arm.
//! - A program's lost socket ([`Engine::tci_disconnected`]) and its `stop;` ([`Engine::tci_end_own`])
//!   end what THAT program holds, hard, now, its CW included.
//! - `cw_macros_stop` from any program stops CW from any origin ([`Engine::tci_cw_stop`]).
//! - Another program's `trx:0,false` stops whatever is on the air, as WSJT-X's UDP HaltTx does
//!   ([`Engine::tci_release`]).
//! - The radio loop's tick: every station gate, Nexus's own key (a tune or the operator's PTT takes
//!   the transmitter), the switch, the section, the ceiling and the gap ([`Engine::poll_tci`]).
//! - A settings save turning "Programs on TCI may transmit" off, or the CW tier for programs' CW
//!   ([`Engine::apply_settings`]).
//!
//! ## One owner
//!
//! From its admission a program owns the transmitter ([`super::TxOwner::Tci`]): every source that
//! consults the arbiter refuses while it does, the CAT broker's clients and the other programs
//! included, and the slot planner plans no over ([`Engine::plan_tx`]). A program is refused while
//! anything else owns it. A program's CW owns it as Nexus's own CW does ([`super::TxOwner::Cw`]),
//! through the one CW queue.
use super::{now_unix_millis, reset_ft8_a7_without_waiting, DataFrame, Engine};
use crate::mic::{MicDrop, MicFeed, MicFrame, MicGates, MicLatch, MicTick, TCI_OVER};
use crate::settings::OperatingMode;
use std::collections::HashMap;
use std::time::Instant;
use tempo_net::tci::server::{KeyKind, Refusal};
use tempo_net::tci::wire::Word;

mod cw;

/// What a program's audio over, and every other key from a program outside Nexus, is judged as:
/// the whole transmit passband, 0 to 3 kHz of audio on the sideband's side of the carrier
/// ([`Engine::outside_key_passband_allowed`]). In Phone the gate judges its own phone passband.
pub(super) const PASSBAND: DataFrame = DataFrame {
    f0_hz: 0.0,
    bw_hz: 3_000.0,
};

/// The frames the over's feed is handed, whatever size a program sends: 20 ms at the route's rate,
/// the size [`TCI_OVER`]'s feed cap counts in.
const FRAME: usize = TCI_OVER.samples(20) as usize;

/// What a program asks to transmit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// `trx:0,true,tci`: an over of its own audio.
    Over,
    /// `trx:0,true[,<microphone>]`: the radio's microphone.
    Mic,
    /// `tune:0,true`.
    Tune,
    /// `cw_macros`, `cw_msg`: CW text, keyed by Nexus's CW keyer.
    Cw,
}

/// A program's key, which owns the transmitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    program: u64,
    /// The radio's microphone, or else an over of the program's audio.
    mic: bool,
    /// Unix ms of the admission: a microphone key's ceiling counts from it.
    since_ms: u64,
    /// The section it was admitted in: a change ends it.
    section: OperatingMode,
}

/// What a program's last `modulation:` asked, as its audio overs are judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    /// A voice mode: its own audio may go out, in Phone.
    Voice,
    /// A data mode, as WSJT-X asks before its FT8: held like FT8.
    Data,
    /// Any other (`cw`): no audio over.
    Other,
}

impl Asked {
    /// A `modulation:` word's kind. The voice modes are those Nexus's server offers programs
    /// (`tempo_net::tci::server::MODULATIONS`; `nfm` is TCI's FM). A word the server does not offer
    /// never reaches the engine, and one added to that list is no voice mode until it is named here.
    fn of(mode: &Word) -> Asked {
        match mode.as_str() {
            "usb" | "lsb" | "am" | "nfm" => Asked::Voice,
            "digu" | "digl" => Asked::Data,
            _ => Asked::Other,
        }
    }
}

/// The TCI server's programs as the engine holds them.
pub(super) struct TciState {
    /// The program's key that owns the transmitter.
    key: Option<Key>,
    /// The program whose tune carrier is up, while one is.
    tune_by: Option<u64>,
    /// A `tci` key's over, and its audio between the program and the radio loop.
    over: MicLatch,
    feed: MicFeed,
    /// The owner's audio short of a whole [`FRAME`].
    pending: Vec<f32>,
    /// The next frame's number, and its place on the over's media clock.
    seq: u64,
    media: u64,
    /// One-shot: the radio loop flushes the output ring and unkeys (an over's hard end).
    abort: bool,
    /// Programs' CW: their speed, and whose word the keyer was handed last.
    cw: cw::TciCw,
    /// What each program's last `modulation:` asked, kept until it asks another or its connection
    /// ends; a program that has asked none has no entry. Only a voice mode lets its overs out.
    modulation: HashMap<u64, Asked>,
}

impl Default for TciState {
    fn default() -> Self {
        Self {
            key: None,
            tune_by: None,
            over: MicLatch::new(TCI_OVER),
            feed: MicFeed::new(&TCI_OVER),
            pending: Vec::new(),
            seq: 0,
            media: 0,
            abort: false,
            cw: cw::TciCw::default(),
            modulation: HashMap::new(),
        }
    }
}

impl Engine {
    // ── The switch ───────────────────────────────────────────────────────────────────────────

    /// Whether programs on TCI may transmit: the station's own setting, "Programs on TCI may
    /// transmit" (`Settings::tci_server_tx`), off by default and changed only at the shack. One
    /// copy, read at every verdict and every tick; a save that turns it off ends every program's
    /// key, over and tune at once ([`Self::apply_settings`]).
    pub fn tci_transmit(&self) -> bool {
        self.settings.tci_server_tx
    }

    // ── The verdict, the keys and their ends ─────────────────────────────────────────────────

    /// ⛔ THE VERDICT: may `program` transmit `ask` now? `Ok`, or the first gate that fails,
    /// in the order of the module header's table.
    fn tci_verdict(&self, program: u64, ask: Ask, audio_started: bool) -> Result<(), Refusal> {
        if !self.tci_transmit() {
            return Err(Refusal::Switch);
        }
        // CW is the server's CW tier as well (`Settings::tci_server_cw`): off, it is not offered.
        if ask == Ask::Cw && !self.settings.tci_server_cw {
            return Err(Refusal::Tier);
        }
        if self.tx_stop_hold {
            return Err(Refusal::StopHold);
        }
        if self.tx_watchdog {
            return Err(Refusal::WatchdogTrip);
        }
        if !self.tx_enabled {
            return Err(Refusal::Disarmed);
        }
        if self.broker_context_hold {
            return Err(Refusal::ContextHold);
        }
        if self.connection_tx_refusal().is_some() {
            return Err(Refusal::Connection);
        }
        if ask == Ask::Over && !self.tci_over_routed() {
            return Err(Refusal::RadioMic);
        }
        let licensed = match ask {
            Ask::Over => self.tci_over_licensed(),
            // As Nexus's own PTT, and across the program's whole passband.
            Ask::Mic => self.tx_allowed() && self.outside_key_passband_allowed(),
            Ask::Tune => self.tune_allowed(),
            // As Nexus's own CW asks before it queues (`send_cw_armed`): at the dial, and as CW
            // where the keyer puts it, which in the CW section are the one judgement.
            Ask::Cw => self.tx_allowed() && self.tx_allowed_as(OperatingMode::Cw),
        };
        if !licensed {
            return Err(Refusal::Licence);
        }
        // A program's audio or microphone from 28.3 to 28.5 MHz for a Novice or a Technician:
        // never, whatever the radio's mode (`Engine::outside_key_allowed`).
        if matches!(ask, Ask::Over | Ask::Mic) && !self.outside_key_allowed() {
            return Err(Refusal::VoiceOnly);
        }
        let again = self.tci_reasserts(program, ask);
        if !again && self.tx_owner().is_some() {
            return Err(Refusal::Owner);
        }
        // A key already held is not dropped by a clock repair, but every CW word is while one
        // lasts (`poll_cw_word`), so a program's CW is never queued under one, its own included.
        if (!again || ask == Ask::Cw) && self.clock_repair_holds_tx() {
            return Err(Refusal::ClockRepair);
        }
        // …nor while a newly signed-in device holds transmit (the 2026-10-23 sign-in decision): a
        // program's key is refused under the already-armed latch until the device is approved,
        // except the re-assert of a key already held, as with the clock repair.
        if (!again || ask == Ask::Cw) && self.remote_transmit_held() {
            return Err(Refusal::OnHold);
        }
        // A data-mode over from a program is held like FT8, in any section (module header).
        if ask == Ask::Over && self.tci_asked(program) == Some(Asked::Data) {
            return Err(Refusal::SlotTimed);
        }
        match (ask, self.settings.operating_mode) {
            (Ask::Over | Ask::Mic, OperatingMode::Phone)
            | (Ask::Cw, OperatingMode::Cw)
            | (Ask::Tune, _) => {}
            // The slot-timed modes wait for the loop bench (module header).
            (Ask::Over, OperatingMode::Digital) => return Err(Refusal::SlotTimed),
            _ => return Err(Refusal::Section),
        }
        // A program's own audio goes out only after it asked a voice mode (module header).
        if ask == Ask::Over && !self.tci_asked_voice(program) {
            return Err(Refusal::VoiceMode);
        }
        if ask == Ask::Over && !audio_started {
            return Err(Refusal::AudioStream);
        }
        Ok(())
    }

    /// Is `program` asking again for what it already holds? Idempotent, as the CAT broker's
    /// re-asserted key is: answered as before, changing nothing. An over playing out after its
    /// release is not held any more.
    fn tci_reasserts(&self, program: u64, ask: Ask) -> bool {
        match ask {
            Ask::Over => {
                self.tci.key.is_some_and(|k| k.program == program && !k.mic)
                    && self.tci.over.active()
                    && !self.tci.over.releasing()
            }
            Ask::Mic => self.tci.key.is_some_and(|k| k.program == program && k.mic),
            Ask::Tune => self.tuning && self.tci.tune_by == Some(program),
            // TCI's macros queue behind the one being sent: only behind the program's own.
            Ask::Cw => self.tci_cw_extends(program),
        }
    }

    /// The licence for a program's audio over: the gate as for Nexus's own overs
    /// ([`Engine::tx_allowed`]) and its whole passband ([`PASSBAND`],
    /// [`Engine::outside_key_passband_allowed`]). Both, so it can never allow what the gate
    /// refuses, whatever section an over is admitted in.
    pub(super) fn tci_over_licensed(&self) -> bool {
        self.tx_allowed() && self.outside_key_passband_allowed()
    }

    /// ⛔ A program's audio over never goes out on the radio's own microphone: the radio must take
    /// the audio Nexus plays. Not while Nexus's own Flex client reports the radio on its own mic
    /// ([`Self::observe_flex_radio_has_mic`]: native audio on, Phone at the shack), where the over's
    /// audio would leave over DAX, which the radio then ignores, and the microphone would carry the
    /// over in its place: refused, as APRS and SSTV are refused there. Judged at the admission and
    /// on every tick.
    fn tci_over_routed(&self) -> bool {
        !self.flex_radio_has_mic
    }

    /// What `program`'s last `modulation:` asked, or `None` before its first.
    fn tci_asked(&self, program: u64) -> Option<Asked> {
        self.tci.modulation.get(&program).copied()
    }

    /// ⛔ Whether `program`'s own audio may go out: its last `modulation:` asked a voice mode. A
    /// program that has asked none has not (WSJT-X's default setup asks none). Judged at the
    /// admission and on every tick.
    fn tci_asked_voice(&self, program: u64) -> bool {
        self.tci_asked(program) == Some(Asked::Voice)
    }

    /// The station's own gates on a program's key, re-checked on every tick: the switch, TX armed,
    /// no Stop held, no watchdog trip, no context hold.
    fn tci_may_transmit(&self) -> bool {
        self.tci_transmit()
            && self.tx_enabled
            && !self.tx_stop_hold
            && !self.tx_watchdog
            && !self.broker_context_hold
    }

    /// `trx:0,true[,<source>]` from `program`: the verdict, and on `Ok` the key. A `tci` key ARMS an
    /// over, which keys on the program's first audio; a microphone key keys the radio's own
    /// microphone on the next radio-loop tick. A program re-asserting the key it holds is answered
    /// `Ok` and changes nothing. `audio_started`: this program's `audio_start` is in force.
    pub fn tci_key(
        &mut self,
        program: u64,
        kind: KeyKind,
        audio_started: bool,
    ) -> Result<(), Refusal> {
        let ask = match kind {
            KeyKind::Tci => Ask::Over,
            KeyKind::Mic(_) => Ask::Mic,
        };
        self.tci_verdict(program, ask, audio_started)?;
        if self.tci_reasserts(program, ask) {
            return Ok(());
        }
        // Who may transmit changed hands: an over planned before it must not key (`commit_tx`
        // checks the generation).
        self.tx_gate_gen = self.tx_gate_gen.wrapping_add(1);
        self.tci.key = Some(Key {
            program,
            mic: ask == Ask::Mic,
            since_ms: now_unix_millis(),
            section: self.settings.operating_mode,
        });
        if ask == Ask::Over {
            self.tci.over.arm();
            self.tci.feed.open();
            self.tci.pending.clear();
            self.tci.seq = 0;
            self.tci.media = 0;
        }
        tempo_core::applog::info(
            "tx",
            &format!(
                "TCI program {program} keyed {}",
                if ask == Ask::Mic {
                    "the radio's microphone"
                } else {
                    "an over of its own audio"
                }
            ),
        );
        Ok(())
    }

    /// What `program` is told as `tx_enable`: the verdict for a `tci` key, now.
    pub fn tci_tx_enable(&self, program: u64, audio_started: bool) -> bool {
        self.tci_verdict(program, Ask::Over, audio_started).is_ok()
    }

    /// The program that owns the transmitter, by any kind of key, its tune or its CW, or `None`.
    pub fn tci_owner(&self) -> Option<u64> {
        self.tci
            .key
            .map(|k| k.program)
            .or(self.tci.tune_by.filter(|_| self.tuning))
            .or_else(|| self.tci_cw_on_air())
    }

    /// `trx:0,false…` from `program`. From the program that holds the key, the GRACEFUL end:
    /// what its over has accepted plays out, and the loop's deadline unkeys after it; a microphone
    /// key ends now. From any other program: whatever is on the air stops, as WSJT-X's UDP HaltTx
    /// stops it ([`Self::halt_tx`], which holds until the operator's arm); with nothing on the air,
    /// nothing. Never gated. `true` when it halted what was on the air, for the connection log.
    pub fn tci_release(&mut self, program: u64) -> bool {
        match self.tci.key {
            Some(k) if k.program == program => {
                if k.mic {
                    self.drop_tci_key("its release");
                } else {
                    self.release_tci_over();
                }
                false
            }
            _ if self.tuning && self.tci.tune_by == Some(program) => {
                self.set_tune(false);
                false
            }
            _ => {
                if !self.on_air() {
                    return false;
                }
                tempo_core::applog::info(
                    "tx",
                    &format!("TCI program {program} released a key it did not hold: halting"),
                );
                self.halt_tx();
                true
            }
        }
    }

    /// The graceful end of the over: the owner's audio short of a frame goes in, nothing it sends
    /// from now is taken ([`Self::tci_tx_audio`]), and the over plays out what it holds, still
    /// gated on every tick. One that never keyed ends now, with nothing on the air.
    fn release_tci_over(&mut self) {
        if !self.tci.pending.is_empty() {
            let rest = std::mem::take(&mut self.tci.pending);
            self.push_tci_frame(rest, Instant::now());
        }
        if !self.tci.over.release() {
            self.tci.feed.close();
            self.tci.key = None;
        }
    }

    /// `stop;` from `program`: its own key, over and tune end, hard; nothing else does.
    pub fn tci_end_own(&mut self, program: u64) {
        self.end_tci_program(program, "its stop");
    }

    /// `program`'s connection ended: it closed, failed or went silent, or the server or the
    /// operator ended it. ⛔ Everything it holds ends hard, NOW: the over's abort is armed here, so
    /// the radio loop flushes and unkeys on its next tick. What it asked as its mode
    /// is forgotten with it.
    pub fn tci_disconnected(&mut self, program: u64) {
        self.end_tci_program(program, "its connection ended");
        self.tci.modulation.remove(&program);
    }

    /// `modulation:0,<mode>` from `program`, whatever comes of the change itself (the CAT broker's
    /// mode rule judges it after, and may refuse it): what it asked, kept until it asks another or
    /// its connection ends. After a voice mode its audio overs are judged as Nexus's own phone;
    /// after a data mode (`digu`, `digl`) they are held like FT8 (`SlotTimed`, in any section);
    /// after any other they are refused (`VoiceMode`), as they are before its first ask. An over it
    /// holds ends on the radio loop's next tick when it asks anything but a voice mode.
    pub fn tci_note_modulation(&mut self, program: u64, mode: &Word) {
        self.tci.modulation.insert(program, Asked::of(mode));
    }

    fn end_tci_program(&mut self, program: u64, why: &str) {
        if self.tci.key.is_some_and(|k| k.program == program) {
            self.drop_tci_key(why);
        }
        if self.tci.tune_by == Some(program) {
            self.end_tci_tune(why);
        }
        self.end_tci_cw(|from| from == Some(program), why);
    }

    /// Every program's key, over, tune and CW, hard: the stops' hook (`halt_tx`, TX Off, the
    /// switch).
    pub(super) fn drop_tci_all(&mut self) {
        self.drop_tci_key("a stop");
        self.end_tci_tune("a stop");
        self.end_programs_cw("a stop");
    }

    /// ⛔ THE ONE KILL PATH for a program's key, hard: the over's feed closes first (its epoch
    /// moves, so nothing sent to it reaches the next one), the latch drops, and the abort is armed
    /// when the rig was keyed, never for an over that keyed nothing; a microphone key ends with the
    /// key, and the loop unkeys on its next tick ([`Self::manual_ptt`]).
    fn drop_tci_key(&mut self, why: &str) {
        let key = self.tci.key.take();
        self.tci.feed.close();
        self.tci.pending.clear();
        if self.tci.over.drop_latch() {
            self.tci.abort = true;
        }
        if let Some(key) = key {
            tempo_core::applog::info(
                "tx",
                &format!("TCI program {}'s key ended: {why}", key.program),
            );
        }
    }

    /// A program's tune carrier, ended hard, as `halt_tx` ends a tune.
    fn end_tci_tune(&mut self, why: &str) {
        let Some(program) = self.tci.tune_by.take() else {
            return;
        };
        if self.tuning {
            self.tuning = false;
            // A tune toggle invalidates any planned over (commit_tx checks the generation).
            self.tx_gate_gen = self.tx_gate_gen.wrapping_add(1);
            tempo_core::applog::info("tx", &format!("TCI program {program}'s tune ended: {why}"));
        }
    }

    /// `tune:0,true` from `program`: the verdict (the licence at the carrier, as Nexus's own Tune),
    /// and on `Ok` Nexus's tune carrier, bounded by its own timeout. Not the operator's act: it
    /// never restarts the watchdog clock.
    pub fn tci_tune_on(&mut self, program: u64) -> Result<(), Refusal> {
        self.tci_verdict(program, Ask::Tune, false)?;
        if self.tci_reasserts(program, Ask::Tune) {
            return Ok(());
        }
        self.set_tune_for(true, reset_ft8_a7_without_waiting, Some(program));
        // The verdict judged every gate the tune checks; one it missed is still a refusal.
        if self.tuning {
            Ok(())
        } else {
            Err(Refusal::Licence)
        }
    }

    /// `tune:0,false` from any program: ends Tune, whoever started it, the operator's included (a
    /// stop). Never gated, and nothing at all with no carrier up, so a stray one cannot cancel a
    /// planned over.
    pub fn tci_tune_off(&mut self, _program: u64) {
        if self.tuning {
            self.set_tune(false);
        }
    }

    /// Who holds the tune carrier from now: `program`, or the station (`None`). Told by every
    /// change of the tune ([`Self::set_tune_for`]).
    pub(super) fn note_tci_tune(&mut self, program: Option<u64>) {
        self.tci.tune_by = program.filter(|_| self.tuning);
    }

    /// A program's key owns the transmitter: an over armed, keyed or playing out, or the radio's
    /// microphone held for it ([`super::TxOwner::Tci`]).
    pub(super) fn tci_holds_tx(&self) -> bool {
        self.tci.key.is_some() || self.tci.over.active()
    }

    /// A program's microphone key keys the radio now, read by [`Self::manual_ptt`]: masked on read
    /// by every gate it was admitted under, so the loop unkeys on the tick a gate goes down.
    pub(super) fn tci_mic_keyed(&self) -> bool {
        self.tci
            .key
            .is_some_and(|k| k.mic && self.tci_mic_down(k, now_unix_millis()).is_none())
    }

    /// Why a program's microphone key may not key the radio at `now_ms`, or `None` while it may.
    fn tci_mic_down(&self, k: Key, now_ms: u64) -> Option<&'static str> {
        if !self.tci_may_transmit() {
            Some("a transmit gate went down")
        } else if self.settings.operating_mode != k.section {
            Some("the section changed")
        } else if !self.tx_allowed() || !self.outside_key_passband_allowed() {
            Some("outside the licence")
        } else if !self.outside_key_allowed() {
            Some(Refusal::VoiceOnly.text())
        } else if self.tuning || self.manual_ptt {
            Some("Nexus keyed its own transmission")
        } else if now_ms.saturating_sub(k.since_ms) >= TCI_OVER.max_latch_ms {
            Some("it reached its 10-minute ceiling")
        } else {
            None
        }
    }

    // ── Audio ────────────────────────────────────────────────────────────────────────────────

    /// One frame of `program`'s transmit audio, decoded to the route's 12 kHz mono, as it arrived.
    ///
    /// ⛔ AUDIO ONLY FOR OUR KEY: taken only from the program whose `tci` key holds an
    /// armed or keyed over it has not let go. Anything else is dropped (`false`), never kept for a
    /// later key. Sanitised here as well: a sample that is not finite is silence, and
    /// none passes full scale. Re-cut into 20 ms frames, so a program's small frames cannot fill the
    /// feed between two ticks.
    pub fn tci_tx_audio(&mut self, program: u64, samples: &[f32], arrived: Instant) -> bool {
        let ours = self.tci.key.is_some_and(|k| k.program == program && !k.mic)
            && self.tci.over.active()
            && !self.tci.over.releasing();
        if !ours {
            return false;
        }
        self.tci.pending.extend(samples.iter().map(|&s| {
            if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }));
        while self.tci.pending.len() >= FRAME {
            let rest = self.tci.pending.split_off(FRAME);
            let frame = std::mem::replace(&mut self.tci.pending, rest);
            self.push_tci_frame(frame, arrived);
        }
        true
    }

    /// Hand one frame to the over's feed, next on its media clock: TCI audio is one contiguous
    /// stream. A frame the feed refuses (the loop has stopped reading it) is gone, and its moment
    /// plays as silence.
    fn push_tci_frame(&mut self, samples: Vec<f32>, arrived: Instant) {
        let len = samples.len() as u64;
        let _ = self.tci.feed.push(MicFrame {
            seq: self.tci.seq,
            media: self.tci.media,
            arrived,
            samples,
        });
        self.tci.seq += 1;
        self.tci.media += len;
    }

    // ── The radio loop ───────────────────────────────────────────────────────────────────────

    /// One radio-loop tick of the programs' keys: the ONLY path from a program's audio to the
    /// transmitter, and the per-tick predicate of its over ([`MicLatch::tick`]). The loop
    /// keys, plays and unkeys what this answers, as it does the Remote microphone's.
    ///
    /// `budget_ms` is how much audio the output ring can take this tick (clamped to the push cap
    /// here as well); `route` is the loop's transmit-route generation (G7). A microphone key and a
    /// program's tune are re-gated here too, and so is a CAT broker client's held key
    /// ([`Self::drop_masked_broker_key`]).
    pub fn poll_tci(&mut self, now: Instant, budget_ms: f64, route: u64) -> MicTick {
        self.poll_tci_at(now, now_unix_millis(), budget_ms, route)
    }

    /// [`Self::poll_tci`] at the wall clock `now_ms`.
    fn poll_tci_at(&mut self, now: Instant, now_ms: u64, budget_ms: f64, route: u64) -> MicTick {
        // A CAT broker client's key outside the licence ends too, as a microphone key does.
        self.drop_masked_broker_key();
        if let Some(k) = self.tci.key.filter(|k| k.mic) {
            if let Some(why) = self.tci_mic_down(k, now_ms) {
                self.drop_tci_key(why);
            }
        }
        if self.tci.tune_by.is_some() && !self.tci_may_transmit() {
            self.end_tci_tune("a transmit gate went down");
        }
        if !self.tci.over.active() {
            return MicTick::Idle;
        }
        let key = self.tci.key;
        let gates = MicGates {
            tx_enabled: self.tci_may_transmit(),
            // G2: the passband in the licence, not where only CW and voice are allowed, and a
            // radio that takes the audio Nexus plays.
            tx_allowed: self.tci_over_licensed()
                && self.outside_key_allowed()
                && self.tci_over_routed(),
            // Nexus's own key wins: a tune, or the operator's own PTT, takes the transmitter.
            tuning: self.tuning || self.manual_ptt,
            // G4: the section it was admitted in, and its program's last ask still a voice mode
            // (a data mode is held like FT8; any other carries no audio over).
            in_section: key.is_some_and(|k| {
                k.section == self.settings.operating_mode && self.tci_asked_voice(k.program)
            }),
            // G5: still its owner's. The socket is the server's: a lost one ends the key at once
            // (`tci_disconnected`).
            presence: key.is_some_and(|k| !k.mic),
            route,
            now,
            now_ms,
            now_secs: now_ms / 1000,
            // ⛔ C2 OFF: a program's over never starts or restarts the watchdog clock;
            // a standing trip ends it through G1, which reads the trip.
            watchdog_limit_secs: 0,
            watchdog_start_secs: None,
        };
        let arrived = self.tci.feed.take();
        let budget_ms = budget_ms.clamp(0.0, TCI_OVER.max_push_ms as f64);
        let budget = (budget_ms * f64::from(TCI_OVER.rate_hz) / 1000.0) as usize;
        let res = self.tci.over.tick(gates, arrived, budget);
        if let Some(why) = res.drop {
            let text = match why {
                MicDrop::GateDown => "a transmit gate went down, or Nexus keyed its own",
                MicDrop::Presence => "its program no longer holds it",
                MicDrop::DeviceChanged => "the transmit audio route changed",
                MicDrop::Ceiling => "it reached its 10-minute ceiling",
                MicDrop::Watchdog => "the TX watchdog tripped",
                MicDrop::AudioGap => "no audio arrived for 300 ms",
                MicDrop::Drained => "it played out after its release",
            };
            if why == MicDrop::Drained {
                // Nothing left to flush: no abort, so the output ring plays out and the loop's
                // deadline unkeys after it.
                self.tci.over.drop_latch();
                self.tci.feed.close();
                if let Some(key) = self.tci.key.take() {
                    tempo_core::applog::info(
                        "tx",
                        &format!("TCI program {}'s over ended: {text}", key.program),
                    );
                }
            } else {
                self.drop_tci_key(text);
            }
        }
        res.tick
    }

    /// Take + reset the one-shot over abort (the loop flushes the output ring and unkeys).
    pub fn take_tci_abort(&mut self) -> bool {
        std::mem::take(&mut self.tci.abort)
    }

    /// The radio loop ends a program's over through the one kill path: the rig it keys is not the
    /// operator's this tick, the rig would not key, or something else unkeyed it.
    pub fn drop_tci_over(&mut self) {
        if self.tci.key.is_some_and(|k| !k.mic) {
            self.drop_tci_key("the radio loop ended it");
        }
    }

    /// Whether a program's over is armed, keyed or playing out.
    pub fn tci_over_armed(&self) -> bool {
        self.tci.over.active()
    }

    /// Whether a program's over has keyed the rig.
    pub fn tci_over_keyed(&self) -> bool {
        self.tci.over.keyed()
    }

    /// For tests outside this crate (the radio loop's): a program's keyed over aged `by_ms`, its
    /// key moved that much earlier ([`MicLatch::age_for_test`]), so the radio loop can be shown its
    /// 10-minute ceiling without waiting ten minutes. It moves the over's key time and nothing
    /// else: no rule, clock or guard.
    #[cfg(any(test, feature = "test-util"))]
    pub fn age_tci_over_for_test(&mut self, by_ms: u64) {
        self.tci.over.age_for_test(by_ms);
    }

    // ── Dial, mode and power ─────────────────────────────────────────────────────────────────

    /// Why a program's `vfo` or `modulation` change is refused now, before the CAT broker's own
    /// dial and mode rules judge it: not while anything is on the air (the program is
    /// told the dial in force). The app must ask this and make the change under ONE hold of the
    /// engine lock, or a key can come between the two.
    pub fn tci_retune_refusal(&self) -> Option<Refusal> {
        self.on_air().then_some(Refusal::OnAir)
    }

    /// The power in force, in percent: what Nexus commands, else what the rig reports; `None`
    /// before either is known.
    pub fn tci_drive(&self) -> Option<u32> {
        self.rf_power
            .or(self.rig_rf_power)
            .map(|p| (p * 100.0).round() as u32)
    }

    /// `drive:0,<percent>` from a program: it may LOWER the power, never raise it,
    /// through [`Self::set_rf_power`], so the operator's ceiling still holds. `Ok` with the power
    /// now in force. The level it asks is judged against [`Self::tci_drive`]: above it, or with no
    /// power known to lower from, it is refused; at it, nothing is written.
    pub fn tci_set_drive(&mut self, percent: u32) -> Result<u32, Refusal> {
        let Some(now) = self.tci_drive() else {
            return Err(Refusal::Raise);
        };
        if percent > now {
            return Err(Refusal::Raise);
        }
        if percent < now {
            self.set_rf_power(percent as f32 / 100.0);
        }
        Ok(self.tci_drive().unwrap_or(percent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::Tier;
    use crate::engine::{now_unix_secs, TxOwner};
    use std::time::Duration;
    use tempo_net::tci::server::{Cw, CwSpeed, Mic};
    use tempo_net::tci::wire::Text;

    const P1: u64 = 1;
    const P2: u64 = 2;
    /// A program that has asked no mode, as WSJT-X, JTDX and MSHV ask none in their default setup
    /// (their Radio mode "None").
    const P3: u64 = 3;
    /// The radio loop's tick, in ms, and the audio it may take each tick.
    const TICK: u64 = 20;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A station in Phone on 20 m with every HF privilege, TX armed as entering Phone arms it, and
    /// programs allowed to transmit. P1 and P2 asked `usb`, as a logger's voice keyer asks it, so
    /// their audio overs are judged as voice; P3 asked nothing.
    fn phone() -> Engine {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_license_class("extra");
        e.set_operating_mode("phone", false);
        e.set_frequency(14.250, "20m", "USB");
        e.settings.tci_server_tx = true;
        assert!(e.tx_enabled(), "premise: entering Phone arms TX");
        e.tci_note_modulation(P1, &word("usb"));
        e.tci_note_modulation(P2, &word("usb"));
        e
    }

    /// The same station in Digital on FT8's 20 m dial, TX armed by the operator.
    fn digital() -> Engine {
        let mut e = phone();
        e.set_operating_mode("digital", false);
        e.set_frequency(14.074, "20m", "USB");
        e.set_tx_enabled_by_operator(true);
        assert!(e.tx_enabled(), "premise: the operator armed Digital");
        e
    }

    /// The same station in the CW section, on 20 m's CW segment, TX armed as entering CW arms it.
    fn cw_section() -> Engine {
        let mut e = phone();
        e.set_operating_mode("cw", false);
        e.set_frequency(14.030, "20m", "CW");
        assert!(e.tx_enabled(), "premise: entering CW arms TX");
        e
    }

    /// A watchdog trip as the transmit paths make one: tripped, and TX off.
    fn trip(e: &mut Engine) {
        e.tx_watchdog = true;
        e.tx_enabled = false;
    }

    /// The operator's settings save with "Programs on TCI may transmit" set to `on`.
    fn save_tci_transmit(e: &mut Engine, on: bool) {
        let mut s = e.settings().clone();
        s.tci_server_tx = on;
        e.apply_settings(s);
    }

    /// An engine, the radio loop's monotonic clock and the wall clock the loop reads.
    struct Scene {
        e: Engine,
        t0: Instant,
        ms0: u64,
    }

    impl Scene {
        fn new(e: Engine) -> Self {
            Self {
                e,
                t0: Instant::now(),
                ms0: now_unix_millis(),
            }
        }
        fn at(&self, n: u64) -> Instant {
            self.t0 + ms(n)
        }
        /// The radio loop's tick `n` ms in, on both clocks.
        fn tick(&mut self, n: u64) -> MicTick {
            let (now, now_ms) = (self.at(n), self.ms0 + n);
            self.e.poll_tci_at(now, now_ms, TICK as f64, 1)
        }
        /// 20 ms of `program`'s audio at `level`, arriving `n` ms in.
        fn audio(&mut self, program: u64, n: u64, level: f32) -> bool {
            let at = self.at(n);
            self.e.tci_tx_audio(program, &[level; FRAME], at)
        }
        /// P1's over, admitted and keyed by its first audio at 0 ms.
        fn keyed(&mut self) {
            assert_eq!(
                self.e.tci_key(P1, KeyKind::Tci, true),
                Ok(()),
                "premise: P1's key is admitted"
            );
            assert_eq!(
                self.tick(0),
                MicTick::Armed,
                "premise: armed, nothing keyed"
            );
            assert!(self.audio(P1, 0, 0.5), "premise: P1's audio is taken");
            assert_eq!(self.tick(0), MicTick::Key, "premise: the first audio keys");
        }
        /// P1's audio and the loop's tick every 20 ms over `from..to` ms, the last frame at
        /// `last_level`: what was fed to the transmitter, and where the over dropped, if it did.
        fn play(&mut self, from: u64, to: u64, last_level: f32) -> (Vec<f32>, Option<u64>) {
            let mut out = Vec::new();
            let mut n = from;
            while n < to {
                let level = if n + TICK >= to { last_level } else { 0.5 };
                self.audio(P1, n, level);
                match self.tick(n) {
                    MicTick::Samples(s) => out.extend(s),
                    MicTick::Idle => return (out, Some(n)),
                    _ => {}
                }
                n += TICK;
            }
            (out, None)
        }
        /// Ticks with no new audio from `from` until the over ends: what was fed, and when.
        fn drain(&mut self, from: u64) -> (Vec<f32>, Option<u64>) {
            let mut out = Vec::new();
            for k in 0..100 {
                let n = from + TICK * k;
                match self.tick(n) {
                    MicTick::Samples(s) => out.extend(s),
                    MicTick::Idle => return (out, Some(n)),
                    _ => {}
                }
            }
            (out, None)
        }
    }

    /// What a program tries.
    #[derive(Debug, Clone, Copy)]
    enum Try {
        /// `trx:0,true,tci`, with its audio stream started or not.
        Over(bool),
        Mic,
        Tune,
    }

    /// A `modulation:` word, as the server hands it over.
    fn word(w: &str) -> Word {
        Word::new(w).expect("a mode word")
    }

    fn try_key(e: &mut Engine, program: u64, t: Try) -> Result<(), Refusal> {
        match t {
            Try::Over(started) => e.tci_key(program, KeyKind::Tci, started),
            Try::Mic => e.tci_key(program, KeyKind::Mic(Mic::Selected), false),
            Try::Tune => e.tci_tune_on(program),
        }
    }

    // ── The verdict ──────────────────────────────────────────────────────────────────────────

    /// ⛔ THE VERDICT NAMES THE FIRST GATE THAT FAILS, one row a gate, each row breaking
    /// that gate alone from a station that admits everything, by value. CONTROLS: the station
    /// admits each kind of key, and Tune in CW.
    #[test]
    fn the_verdict_names_the_first_gate_that_fails() {
        type Setup = fn(&mut Engine);
        let rows: Vec<(&str, Setup, Try, Result<(), Refusal>)> = vec![
            ("control: a tci over", |_| {}, Try::Over(true), Ok(())),
            ("control: a microphone key", |_| {}, Try::Mic, Ok(())),
            ("control: Tune", |_| {}, Try::Tune, Ok(())),
            (
                "control: Tune in CW",
                |e| e.set_operating_mode("cw", false),
                Try::Tune,
                Ok(()),
            ),
            (
                "the transmit switch is off",
                |e| e.settings.tci_server_tx = false,
                Try::Over(true),
                Err(Refusal::Switch),
            ),
            (
                "Stop TX holds",
                |e| e.halt_tx(),
                Try::Over(true),
                Err(Refusal::StopHold),
            ),
            (
                "the operator's TX Off holds",
                |e| e.set_tx_enabled_by_operator(false),
                Try::Mic,
                Err(Refusal::StopHold),
            ),
            (
                "a watchdog trip stands",
                trip,
                Try::Over(true),
                Err(Refusal::WatchdogTrip),
            ),
            (
                "TX is disarmed",
                |e| e.set_tx_enabled(false),
                Try::Over(true),
                Err(Refusal::Disarmed),
            ),
            (
                "a band change's hold",
                |e| e.set_frequency(7.200, "40m", "LSB"),
                Try::Over(true),
                Err(Refusal::ContextHold),
            ),
            (
                "the Icom network connection",
                |e| e.settings.rig_conn = "icomlan".into(),
                Try::Over(true),
                Err(Refusal::Connection),
            ),
            (
                "an over with the radio on its own microphone",
                |e| e.observe_flex_radio_has_mic(true),
                Try::Over(true),
                Err(Refusal::RadioMic),
            ),
            (
                "control: a microphone key with the radio on its own microphone",
                |e| e.observe_flex_radio_has_mic(true),
                Try::Mic,
                Ok(()),
            ),
            (
                "control: Tune with the radio on its own microphone",
                |e| e.observe_flex_radio_has_mic(true),
                Try::Tune,
                Ok(()),
            ),
            (
                "the over's passband outside the licence",
                |e| e.set_frequency(14.349, "20m", "USB"),
                Try::Over(true),
                Err(Refusal::Licence),
            ),
            (
                "the microphone outside the licence",
                |e| e.set_frequency(14.349, "20m", "USB"),
                Try::Mic,
                Err(Refusal::Licence),
            ),
            (
                "Tune outside the licence",
                |e| e.set_frequency(14.349, "20m", "USB"),
                Try::Tune,
                Err(Refusal::Licence),
            ),
            (
                "the operator's PTT is held",
                |e| e.set_ptt(true),
                Try::Over(true),
                Err(Refusal::Owner),
            ),
            (
                "a CAT-broker client's key",
                |e| {
                    e.settings.cat_broker_ptt = true;
                    assert!(e.broker_ptt(true), "premise: the broker's key");
                },
                Try::Mic,
                Err(Refusal::Owner),
            ),
            (
                "another program's key",
                |e| assert_eq!(try_key(e, P2, Try::Mic), Ok(()), "premise: P2 keyed"),
                Try::Over(true),
                Err(Refusal::Owner),
            ),
            (
                "the operator's tune",
                |e| e.set_tune(true),
                Try::Tune,
                Err(Refusal::Owner),
            ),
            (
                "a clock repair",
                |e| e.hold_tx_for_clock_repair(Instant::now() + Duration::from_secs(600)),
                Try::Over(true),
                Err(Refusal::ClockRepair),
            ),
            (
                "an over in CW",
                |e| e.set_operating_mode("cw", false),
                Try::Over(true),
                Err(Refusal::Section),
            ),
            (
                "a microphone key in RTTY",
                |e| {
                    e.set_operating_mode("rtty", false);
                    e.set_frequency(14.080, "20m", "LSB");
                },
                Try::Mic,
                Err(Refusal::Section),
            ),
            (
                "a microphone key in Digital",
                |e| *e = digital(),
                Try::Mic,
                Err(Refusal::Section),
            ),
            (
                "an over in Digital: slot-timed",
                |e| *e = digital(),
                Try::Over(true),
                Err(Refusal::SlotTimed),
            ),
            (
                "an over in Phone from a program that asked a data mode",
                |e| e.tci_note_modulation(P1, &word("digu")),
                Try::Over(true),
                Err(Refusal::SlotTimed),
            ),
            (
                "control: a microphone key from a program that asked a data mode",
                |e| e.tci_note_modulation(P1, &word("digu")),
                Try::Mic,
                Ok(()),
            ),
            (
                "an over in Phone from a program that asked no voice mode (cw)",
                |e| e.tci_note_modulation(P1, &word("cw")),
                Try::Over(true),
                Err(Refusal::VoiceMode),
            ),
            (
                "an over in Digital from a program that asked cw: slot-timed first",
                |e| {
                    *e = digital();
                    e.tci_note_modulation(P1, &word("cw"));
                },
                Try::Over(true),
                Err(Refusal::SlotTimed),
            ),
            (
                "an over in CW from a program that asked cw: the section first",
                |e| {
                    e.set_operating_mode("cw", false);
                    e.tci_note_modulation(P1, &word("cw"));
                },
                Try::Over(true),
                Err(Refusal::Section),
            ),
            (
                "an over with no audio stream from a program that asked cw: the voice mode first",
                |e| e.tci_note_modulation(P1, &word("cw")),
                Try::Over(false),
                Err(Refusal::VoiceMode),
            ),
            (
                "control: a microphone key from a program that asked cw",
                |e| e.tci_note_modulation(P1, &word("cw")),
                Try::Mic,
                Ok(()),
            ),
            (
                "control: Tune from a program that asked cw",
                |e| e.tci_note_modulation(P1, &word("cw")),
                Try::Tune,
                Ok(()),
            ),
            (
                "an over with no audio stream",
                |_| {},
                Try::Over(false),
                Err(Refusal::AudioStream),
            ),
        ];
        let mut wrong = Vec::new();
        for (name, setup, t, want) in rows {
            let mut e = phone();
            setup(&mut e);
            let got = try_key(&mut e, P1, t);
            if got != want {
                wrong.push(format!("{name} ({t:?}): got {got:?}, want {want:?}"));
            }
        }
        assert!(wrong.is_empty(), "the verdict:\n{}", wrong.join("\n"));
    }

    /// ⛔ A PROGRAM'S OVER IS JUDGED ACROSS ITS WHOLE PASSBAND, not as Nexus's own next
    /// frame: a General in Digital at 14.148 MHz, where the frame at the TX offset is inside the
    /// 20 m data segment and the passband reaches past its top at 14.150, is refused for the
    /// licence. CONTROL: at 14.140 the passband fits, and only the slot-timed rule refuses.
    #[test]
    fn a_program_over_is_judged_across_its_whole_passband() {
        let at = |mhz: f64| {
            let mut e = digital();
            e.set_license_class("general");
            e.set_frequency(mhz, "20m", "USB");
            e.set_tx_enabled_by_operator(true);
            assert!(
                e.tx_allowed(),
                "premise at {mhz}: Nexus's own frame at its TX offset is licensed"
            );
            e.tci_key(P1, KeyKind::Tci, true)
        };
        assert_eq!(at(14.148), Err(Refusal::Licence), "at 14.148");
        assert_eq!(at(14.140), Err(Refusal::SlotTimed), "control at 14.140");
    }

    /// ⛔ RULING OF 2026-10-19: FT8, FT4 and the other slot-timed modes may not go out through
    /// the server until the loop bench. In Digital a program's over is refused as `SlotTimed`
    /// (every Digital tier is slot-timed), its `tx_enable` says false, and nothing is armed.
    /// CONTROL: the same program's over in Phone is admitted.
    #[test]
    fn slot_timed_modes_are_refused_until_the_bench() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let mut e = digital();
            e.set_tier(tier);
            e.set_tx_enabled_by_operator(true);
            assert_eq!(
                e.settings.operating_mode,
                OperatingMode::Digital,
                "premise: {tier:?} is a Digital tier"
            );
            assert_eq!(
                e.tci_key(P1, KeyKind::Tci, true),
                Err(Refusal::SlotTimed),
                "{tier:?}"
            );
            assert!(!e.tci_tx_enable(P1, true), "{tier:?}: told tx_enable true");
            assert!(!e.tci_over_armed(), "{tier:?}: an over armed");
            assert_eq!(e.tx_owner(), None, "{tier:?}: something owns TX");
        }
        let mut e = phone();
        assert_eq!(e.tci_key(P1, KeyKind::Tci, true), Ok(()), "control: Phone");
    }

    /// `tx_enable` is the verdict for a `tci` key: true where one would be admitted, false where
    /// it would be refused, and true for the program that holds the key (its re-assert passes).
    #[test]
    fn tx_enable_is_the_verdict_for_a_tci_key() {
        let mut e = phone();
        assert!(e.tci_tx_enable(P1, true), "a station that admits it");
        assert!(!e.tci_tx_enable(P1, false), "no audio stream");
        assert_eq!(e.tci_key(P1, KeyKind::Tci, true), Ok(()));
        assert!(e.tci_tx_enable(P1, true), "the program that holds the key");
        assert!(
            !e.tci_tx_enable(P2, true),
            "another program while P1 holds it"
        );
        e.set_tx_enabled(false);
        assert!(!e.tci_tx_enable(P1, true), "TX disarmed");
        assert!(!digital().tci_tx_enable(P1, true), "Digital: slot-timed");
    }

    // ── Nothing a program sends can arm ──────────────────────────────────────────────────────

    /// The values no program may move upward: TX armed, and the holds and the clock.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Holds {
        tx_enabled: bool,
        stop_hold: bool,
        watchdog: bool,
        context_hold: bool,
        watchdog_start: Option<u64>,
    }

    fn holds(e: &Engine) -> Holds {
        Holds {
            tx_enabled: e.tx_enabled,
            stop_hold: e.tx_stop_hold,
            watchdog: e.tx_watchdog,
            context_hold: e.broker_context_hold,
            watchdog_start: e.tx_watchdog_start,
        }
    }

    /// What a step may do to them: lower TX (a stop), set a hold, never the reverse, and never
    /// touch the watchdog clock.
    fn arms_or_clears(before: Holds, after: Holds) -> bool {
        (!before.tx_enabled && after.tx_enabled)
            || (before.stop_hold && !after.stop_hold)
            || (before.watchdog && !after.watchdog)
            || (before.context_hold && !after.context_hold)
            || before.watchdog_start != after.watchdog_start
    }

    type Step = (&'static str, fn(&mut Scene));

    /// Every command a program can send that reaches the engine, from two programs.
    fn steps() -> Vec<Step> {
        vec![
            ("P1 tci key", |s| {
                let _ = s.e.tci_key(P1, KeyKind::Tci, true);
            }),
            ("P1 tci key, no stream", |s| {
                let _ = s.e.tci_key(P1, KeyKind::Tci, false);
            }),
            ("P2 microphone key", |s| {
                let _ = s.e.tci_key(P2, KeyKind::Mic(Mic::Mic1), false);
            }),
            ("P1 audio", |s| {
                s.audio(P1, 0, 0.5);
            }),
            ("P2 audio", |s| {
                s.audio(P2, 0, 0.5);
            }),
            ("the loop's tick", |s| {
                s.tick(0);
            }),
            ("P1 tx_enable", |s| {
                let _ = s.e.tci_tx_enable(P1, true);
            }),
            ("P1 release", |s| {
                s.e.tci_release(P1);
            }),
            ("P2 release", |s| {
                s.e.tci_release(P2);
            }),
            ("P1 stop;", |s| s.e.tci_end_own(P1)),
            ("P2 lost", |s| s.e.tci_disconnected(P2)),
            ("P1 tune on", |s| {
                let _ = s.e.tci_tune_on(P1);
            }),
            ("P2 tune off", |s| s.e.tci_tune_off(P2)),
            ("P1 drive 100", |s| {
                let _ = s.e.tci_set_drive(100);
            }),
            ("P1 drive 10", |s| {
                let _ = s.e.tci_set_drive(10);
            }),
            ("P1 retune", |s| {
                let _ = s.e.tci_retune_refusal();
            }),
            ("P1 modulation lsb", |s| {
                s.e.tci_note_modulation(P1, &word("lsb"))
            }),
            ("P2 modulation digu", |s| {
                s.e.tci_note_modulation(P2, &word("digu"))
            }),
            ("P1 cw_macros", |s| {
                let text = Text::new("CQ TEST").expect("printable");
                let _ = s.e.tci_cw(P1, &Cw::Macro(text));
            }),
            ("P2 cw_msg", |s| {
                let t = |p: &str| Text::new(p).expect("printable");
                let _ = s.e.tci_cw(
                    P2,
                    &Cw::Message {
                        prefix: t("TU"),
                        call: t("K1ABC$2"),
                        suffix: t("5NN"),
                    },
                );
            }),
            ("P1 cw_macros_speed 99", |s| {
                let _ = s.e.tci_cw_speed(CwSpeed::Set(99));
            }),
            ("the loop keys a CW word", |s| {
                if let Some((_, wpm, _)) = s.e.poll_cw_word() {
                    s.e.observe_cw_keying(Some(f64::from(12_000 / wpm)));
                }
            }),
            ("P2 cw_macros_stop", |s| s.e.tci_cw_stop(P2)),
        ]
    }

    type State = (&'static str, fn() -> Engine);

    /// The station states a program can find.
    fn states() -> Vec<State> {
        vec![
            ("launch, TX off", || {
                let mut e = Engine::new("KD9TAW", "EN52", 0);
                e.set_license_class("extra");
                e.settings.tci_server_tx = true;
                e
            }),
            ("Phone, armed", phone),
            ("Phone, armed, the watchdog clock running", || {
                let mut e = phone();
                e.tx_watchdog_start = Some(now_unix_secs() - 30);
                e
            }),
            ("after Stop TX", || {
                let mut e = phone();
                e.halt_tx();
                e
            }),
            ("after a watchdog trip", || {
                let mut e = phone();
                trip(&mut e);
                e
            }),
            ("after a band change", || {
                let mut e = phone();
                e.set_frequency(7.200, "40m", "LSB");
                e
            }),
            ("Digital, armed", digital),
            ("CW, armed", cw_section),
            ("CW, after Stop TX", || {
                let mut e = cw_section();
                e.halt_tx();
                e
            }),
        ]
    }

    /// ⛔ NO COMMAND, IN ANY STATE, ARMS TX, CLEARS A STOP'S HOLD, A WATCHDOG TRIP OR THE CONTEXT
    /// HOLD, OR STARTS OR RESTARTS THE WATCHDOG CLOCK: every command alone from
    /// every state, then all of them in a row from every state. CONTROL: the sweep is not
    /// vacuous: keys are admitted, and a stop does lower TX, in states where they should be.
    #[test]
    fn no_program_command_arms_or_clears_a_hold_in_any_state() {
        let mut moved = Vec::new();
        let (mut admitted, mut stopped) = (0, 0);
        for (state, build) in states() {
            for (name, step) in steps() {
                let mut s = Scene::new(build());
                let before = holds(&s.e);
                step(&mut s);
                s.tick(TICK);
                if arms_or_clears(before, holds(&s.e)) {
                    moved.push(format!("{state}: {name}: {before:?} -> {:?}", holds(&s.e)));
                }
            }
            let mut s = Scene::new(build());
            for (name, step) in steps() {
                let before = holds(&s.e);
                step(&mut s);
                s.tick(TICK);
                let after = holds(&s.e);
                admitted += usize::from(s.e.tci_owner().is_some());
                stopped += usize::from(before.tx_enabled && !after.tx_enabled);
                if arms_or_clears(before, after) {
                    moved.push(format!(
                        "{state}, in a row: {name}: {before:?} -> {after:?}"
                    ));
                }
            }
        }
        assert!(
            moved.is_empty(),
            "a program moved them:\n{}",
            moved.join("\n")
        );
        assert!(admitted > 0, "control: no key was ever admitted");
        assert!(stopped > 0, "control: no stop ever lowered TX");
    }

    // ── Stop in Nexus always wins ────────────────────────────────────────────────────────────

    /// ⛔ STOP WINS: Stop TX, the operator's TX Off and the transmit switch going off
    /// each end a program's keyed over at once, with the abort that flushes and unkeys; its audio
    /// is refused from then; and a key after it is refused until the operator's own arm (or the
    /// switch, for the switch), which then admits it.
    #[test]
    fn every_stop_ends_a_program_over_hard_and_the_stop_holds() {
        type Stop = fn(&mut Engine);
        let stops: [(&str, Stop, Refusal); 3] = [
            ("Stop TX", |e| e.halt_tx(), Refusal::StopHold),
            (
                "TX Off",
                |e| e.set_tx_enabled_by_operator(false),
                Refusal::StopHold,
            ),
            (
                "the switch off",
                |e| save_tci_transmit(e, false),
                Refusal::Switch,
            ),
        ];
        for (name, stop, refusal) in stops {
            let mut s = Scene::new(phone());
            s.keyed();
            stop(&mut s.e);
            assert!(!s.e.tci_over_armed(), "{name}: the over survived");
            assert!(s.e.take_tci_abort(), "{name}: no abort to flush and unkey");
            assert_eq!(s.e.tci_owner(), None, "{name}: still the owner");
            assert_eq!(s.e.tx_owner(), None, "{name}: something owns TX");
            assert!(
                !s.audio(P1, 20, 0.5),
                "{name}: the over's audio still taken"
            );
            assert_eq!(
                s.e.tci_key(P1, KeyKind::Tci, true),
                Err(refusal),
                "{name}: a key after it"
            );
            if refusal == Refusal::Switch {
                save_tci_transmit(&mut s.e, true);
            } else {
                s.e.set_tx_enabled_by_operator(true);
            }
            assert_eq!(
                s.e.tci_key(P1, KeyKind::Tci, true),
                Ok(()),
                "{name}: control, after the operator's arm"
            );
        }
    }

    /// ⛔ THE SWITCH IS THE STATION'S SETTING ("Programs on TCI may transmit", off on a fresh
    /// station), and the operator's save that turns it off ends every program's over, microphone
    /// key and tune at once, hard. CONTROL: a save that leaves it on ends nothing.
    #[test]
    fn the_transmit_switch_is_the_stations_setting_and_a_save_turning_it_off_ends_every_key() {
        assert!(
            !Engine::new("KD9TAW", "EN52", 0).tci_transmit(),
            "programs may transmit on a fresh station"
        );
        let mut e = phone();
        assert!(e.tci_transmit(), "the switch is not the station's setting");
        e.settings.tci_server_tx = false;
        assert!(!e.tci_transmit(), "the switch is not the station's setting");

        let mut s = Scene::new(phone());
        s.keyed();
        let mut other = s.e.settings().clone();
        other.tx_watchdog_min += 1;
        s.e.apply_settings(other);
        assert!(
            s.e.tci_over_keyed() && !s.e.take_tci_abort(),
            "control: a save that left the switch on ended the over"
        );
        save_tci_transmit(&mut s.e, false);
        assert!(!s.e.tci_over_armed(), "the save left the over on");
        assert!(s.e.take_tci_abort(), "no abort to flush and unkey");
        assert_eq!(s.e.tci_owner(), None);

        let mut e = phone();
        assert_eq!(try_key(&mut e, P1, Try::Mic), Ok(()));
        assert!(e.manual_ptt(), "premise: the microphone is keyed");
        save_tci_transmit(&mut e, false);
        assert!(!e.manual_ptt(), "the save left the microphone keyed");
        assert_eq!(e.tci_owner(), None, "the save left the microphone's key");

        let mut e = phone();
        assert_eq!(try_key(&mut e, P1, Try::Tune), Ok(()));
        save_tci_transmit(&mut e, false);
        assert!(!e.tuning(), "the save left the program's tune up");
        assert_eq!(e.tci_owner(), None);
    }

    /// A stop ends a program's microphone key and its tune as well, at once.
    #[test]
    fn every_stop_ends_a_programs_microphone_key_and_tune() {
        let mut e = phone();
        assert_eq!(try_key(&mut e, P1, Try::Mic), Ok(()));
        assert!(e.manual_ptt(), "premise: the microphone is keyed");
        e.halt_tx();
        assert!(
            !e.manual_ptt(),
            "Stop TX left the program's microphone keyed"
        );
        assert_eq!(e.tci_owner(), None);

        let mut e = phone();
        assert_eq!(try_key(&mut e, P1, Try::Tune), Ok(()));
        assert!(e.tuning(), "premise: the program's tune is up");
        e.set_tx_enabled_by_operator(false);
        assert!(!e.tuning(), "TX Off left the program's tune up");
        assert_eq!(e.tci_owner(), None);
    }

    // ── A lost socket ────────────────────────────────────────────────────────────────────────

    /// ⛔ A PROGRAM'S LOST SOCKET ENDS WHAT IT HOLDS AT ONCE: its keyed over is
    /// dropped and the abort armed in the same call, so the radio loop's next tick flushes and
    /// unkeys; a microphone key and a tune are down at once. CONTROL: another program's loss
    /// changes nothing.
    #[test]
    fn a_lost_socket_ends_what_its_program_holds_at_once() {
        let mut s = Scene::new(phone());
        s.keyed();
        s.e.tci_disconnected(P2);
        assert!(
            s.e.tci_over_keyed() && !s.e.take_tci_abort(),
            "control: another program's loss ended P1's over"
        );
        s.e.tci_disconnected(P1);
        assert!(!s.e.tci_over_armed(), "the lost program's over survived");
        assert!(s.e.take_tci_abort(), "no abort armed for the next tick");
        assert!(!s.e.on_air(), "still on the air after the loss");
        assert_eq!(s.tick(20), MicTick::Idle);

        let mut e = phone();
        assert_eq!(try_key(&mut e, P1, Try::Mic), Ok(()));
        e.tci_disconnected(P1);
        assert!(
            !e.manual_ptt(),
            "the lost program's microphone key is still keyed"
        );

        let mut e = phone();
        assert_eq!(try_key(&mut e, P1, Try::Tune), Ok(()));
        e.tci_disconnected(P1);
        assert!(!e.tuning(), "the lost program's tune is still up");
    }

    // ── The over's own ends ──────────────────────────────────────────────────────────────────

    /// The over's gap is the design's 300 ms (by value, not read from [`TCI_OVER`]), from the
    /// arrival of its last audio: audio stops at 1000 ms, every tick to 1280 ms keeps the over
    /// (the control), and the tick at 1300 ms ends it, with the abort.
    #[test]
    fn a_program_over_ends_300_ms_after_its_audio_stops() {
        let mut s = Scene::new(phone());
        s.keyed();
        let (_, dropped) = s.play(20, 1_020, 0.5);
        assert_eq!(dropped, None, "premise: the over runs while audio comes");
        for n in (1_020..1_300).step_by(TICK as usize) {
            assert_ne!(s.tick(n), MicTick::Idle, "control: ended at {n} ms");
        }
        assert_eq!(s.tick(1_300), MicTick::Idle, "the gap's end");
        assert!(
            s.e.take_tci_abort(),
            "the gap ended the over without its abort"
        );
    }

    /// The 10-minute ceiling, which no setting raises: ten minutes by value, not read from
    /// [`TCI_OVER`].
    const TEN_MINUTES: u64 = 10 * 60 * 1000;

    /// A keyed over ends at the ceiling, counted from its key, with its abort. CONTROL: a minute
    /// short of it, it goes on.
    #[test]
    fn a_program_over_ends_at_ten_minutes() {
        let ceiling = TEN_MINUTES;
        let mut s = Scene::new(phone());
        s.keyed();
        s.audio(P1, 20, 0.5);
        let (now, ms0) = (s.at(20), s.ms0);
        assert_ne!(
            s.e.poll_tci_at(now, ms0 + ceiling - 60_000, TICK as f64, 1),
            MicTick::Idle,
            "control: a minute short of the ceiling"
        );
        s.audio(P1, 40, 0.5);
        let now = s.at(40);
        assert_eq!(
            s.e.poll_tci_at(now, ms0 + ceiling, TICK as f64, 1),
            MicTick::Idle,
            "the over ran past its ceiling"
        );
        assert!(
            s.e.take_tci_abort(),
            "the ceiling ended the over without its abort"
        );
    }

    /// A microphone key ends at the same ceiling, counted from its admission. CONTROL: a minute
    /// short of it, it is held.
    #[test]
    fn a_programs_microphone_key_ends_at_ten_minutes() {
        let ceiling = TEN_MINUTES;
        let mut s = Scene::new(phone());
        assert_eq!(try_key(&mut s.e, P1, Try::Mic), Ok(()));
        let (now, ms0) = (s.at(20), s.ms0);
        s.e.poll_tci_at(now, ms0 + ceiling - 60_000, TICK as f64, 1);
        assert_eq!(
            s.e.tci_owner(),
            Some(P1),
            "control: a minute short of the ceiling"
        );
        s.e.poll_tci_at(now, ms0 + ceiling + 1_000, TICK as f64, 1);
        assert_eq!(
            s.e.tci_owner(),
            None,
            "the microphone key ran past its ceiling"
        );
        assert!(!s.e.manual_ptt());
    }

    // ── Audio only for our key ───────────────────────────────────────────────────────────────

    /// ⛔ AUDIO ONLY FOR OUR KEY: another program's audio is refused and keys
    /// nothing; with no over, or after the owner let go, nothing is taken. CONTROL: the owner's
    /// own audio is taken and keys its over.
    #[test]
    fn only_the_overs_owner_has_its_audio_taken() {
        let mut s = Scene::new(phone());
        assert!(!s.audio(P1, 0, 0.5), "audio taken with no over");
        assert_eq!(s.e.tci_key(P1, KeyKind::Tci, true), Ok(()));
        assert!(!s.audio(P2, 0, 0.5), "another program's audio was taken");
        assert_eq!(
            s.tick(0),
            MicTick::Armed,
            "another program's audio keyed P1's over"
        );
        assert!(s.audio(P1, 20, 0.5), "control: the owner's audio");
        assert_eq!(s.tick(20), MicTick::Key, "control: the owner's audio keys");
        s.e.tci_release(P1);
        assert!(!s.audio(P1, 40, 0.5), "audio taken after the owner let go");
    }

    /// Every sample a program sends is made safe for the transmitter: not finite is silence,
    /// nothing past full scale. And small frames are re-cut, so thirty 8-sample frames between
    /// two ticks do not overrun the feed: the over plays them all, with no hole.
    #[test]
    fn program_audio_is_sanitised_and_recut_into_whole_frames() {
        let mut s = Scene::new(phone());
        assert_eq!(s.e.tci_key(P1, KeyKind::Tci, true), Ok(()));
        let wild = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 2.0, -3.0, 0.25];
        let t = s.at(0);
        assert!(s.e.tci_tx_audio(P1, &[wild.as_slice(); 40].concat(), t));
        assert_eq!(s.tick(0), MicTick::Key);
        let (out, _) = s.drain(20);
        assert!(!out.is_empty(), "premise: the audio was played");
        assert!(
            out.iter()
                .all(|x| x.is_finite() && (-1.0..=1.0).contains(x)),
            "unsafe samples reached the transmitter"
        );
        assert!(out.contains(&1.0) && out.contains(&-1.0) && out.contains(&0.25));

        let mut s = Scene::new(phone());
        assert_eq!(s.e.tci_key(P1, KeyKind::Tci, true), Ok(()));
        let mut out = Vec::new();
        for k in 0..20u64 {
            let at = s.at(TICK * k);
            for _ in 0..30 {
                assert!(s.e.tci_tx_audio(P1, &[0.5; 8], at));
            }
            if let MicTick::Samples(x) = s.tick(TICK * k) {
                out.extend(x);
            }
        }
        assert_eq!(
            s.e.tci.over.holes(),
            (0, 0),
            "small frames left holes on the air"
        );
        let voice = &out[TCI_OVER.samples(TCI_OVER.ahead_ms) as usize..];
        assert!(
            !voice.is_empty() && voice.iter().all(|x| *x == 0.5),
            "silence went out between small frames"
        );
    }

    // ── The owner's release, another's ───────────────────────────────────────────────────────

    /// ⛔ THE OWNER'S RELEASE IS GRACEFUL: what its over accepted plays out, the last
    /// frame included, and the over ends with no abort, nothing halted and nothing held. CONTROL:
    /// Stop TX at the same moment arms the abort that flushes it.
    #[test]
    fn the_owners_release_plays_out_its_audio_and_a_stop_flushes_it() {
        let marker = 0.25;
        let mut s = Scene::new(phone());
        s.keyed();
        let (mut out, dropped) = s.play(20, 220, marker);
        assert_eq!(dropped, None);
        assert!(!s.e.tci_release(P1), "the owner's release said it halted");
        assert!(s.e.tci_over_keyed(), "the release cut the over at once");
        let (rest, ended) = s.drain(220);
        out.extend(rest);
        assert!(
            out.contains(&marker),
            "the over's last frame never went out"
        );
        assert!(ended.is_some_and(|n| n < 220 + 300), "ended at {ended:?}");
        assert!(!s.e.take_tci_abort(), "the release flushed what was queued");
        assert_eq!(s.e.tci_owner(), None, "still the owner after it played out");
        assert!(
            s.e.tx_enabled() && !s.e.tx_stop_hold,
            "the release halted TX"
        );

        let mut s = Scene::new(phone());
        s.keyed();
        s.play(20, 220, marker);
        s.e.halt_tx();
        assert!(s.e.take_tci_abort(), "control: Stop TX did not flush");
    }

    /// ⛔ ANOTHER PROGRAM'S `trx:0,false` STOPS WHATEVER IS ON THE AIR, as WSJT-X's UDP HaltTx
    /// does: a program's over or microphone key, or Nexus's own over, with the stop's hold.
    /// CONTROL: with nothing on the air it does nothing at all.
    #[test]
    fn another_programs_release_stops_what_is_on_the_air() {
        let mut s = Scene::new(phone());
        s.keyed();
        assert!(s.e.tci_release(P2), "said it halted nothing");
        assert!(!s.e.tci_over_armed(), "P1's over went on");
        assert!(s.e.take_tci_abort(), "no abort");
        assert!(
            !s.e.tx_enabled() && s.e.tx_stop_hold,
            "not a stop that holds"
        );

        let mut e = phone();
        assert_eq!(try_key(&mut e, P1, Try::Mic), Ok(()));
        assert!(e.tci_release(P2), "said it halted nothing");
        assert!(!e.manual_ptt(), "P1's microphone key went on");

        let mut e = phone();
        e.app.radio.transmitting = true; // Nexus's own over
        assert!(e.tci_release(P2), "said it halted nothing");
        assert!(!e.tx_enabled(), "Nexus's own over went on");

        let mut e = phone();
        assert!(!e.tci_release(P2), "control: said it halted");
        assert!(
            e.tx_enabled() && !e.tx_stop_hold,
            "control: with nothing on the air a release stopped the station"
        );
    }

    // ── One owner ────────────────────────────────────────────────────────────────────────────

    /// ⛔ ONE OWNER: a program's admitted key owns the transmitter and is on the
    /// air from its admission; nothing else that asks the arbiter keys under it, the CAT broker's
    /// clients and the other programs included, and its own re-assert changes nothing. The other
    /// way round, it is refused while the broker holds the key.
    #[test]
    fn a_programs_key_is_one_owner_among_the_others() {
        let mut s = Scene::new(phone());
        s.keyed();
        assert_eq!(s.e.tx_owner(), Some(TxOwner::Tci));
        assert!(s.e.on_air());
        assert_eq!(s.e.tci_owner(), Some(P1));
        assert_eq!(try_key(&mut s.e, P2, Try::Over(true)), Err(Refusal::Owner));
        assert_eq!(try_key(&mut s.e, P2, Try::Mic), Err(Refusal::Owner));
        assert_eq!(try_key(&mut s.e, P2, Try::Tune), Err(Refusal::Owner));
        assert_eq!(
            try_key(&mut s.e, P1, Try::Over(true)),
            Ok(()),
            "P1's re-assert"
        );
        assert!(s.e.tci_over_keyed(), "the re-assert restarted the over");
        let (_, dropped) = s.play(20, 600, 0.5);
        assert_eq!(dropped, None, "the re-assert broke the over's audio");
        s.e.settings.cat_broker_ptt = true;
        assert!(
            !s.e.broker_ptt(true),
            "the broker keyed under a program's over"
        );
        assert_eq!(
            s.e.send_voice(vec![0.25; 12_000]),
            Err(TxOwner::Tci.busy_reason()),
            "a voice message under a program's over"
        );

        let mut e = phone();
        e.settings.cat_broker_ptt = true;
        assert!(e.broker_ptt(true), "premise: the broker keys");
        assert_eq!(try_key(&mut e, P1, Try::Over(true)), Err(Refusal::Owner));
    }

    /// ⛔ ONE OWNER WHILE A VOICE MESSAGE PLAYS: the radio loop takes the voice keyer's message
    /// from the engine at its key, and the message still owns the transmitter until the loop says
    /// its over ended, so no program's key is admitted under it. CONTROL: the same key, once the
    /// over has ended, is admitted.
    #[test]
    fn a_programs_key_is_refused_while_a_voice_message_plays() {
        for t in [Try::Over(true), Try::Mic, Try::Tune] {
            let mut e = phone();
            e.send_voice(vec![0.25; 12_000]).unwrap();
            assert!(
                e.poll_voice().is_some(),
                "premise: the loop takes the message"
            );
            assert_eq!(
                try_key(&mut e, P1, t),
                Err(Refusal::Owner),
                "{t:?}: admitted between the take and the loop's report"
            );
            e.set_voice_on_air(true);
            assert_eq!(
                try_key(&mut e, P1, t),
                Err(Refusal::Owner),
                "{t:?}: admitted under a playing message"
            );
            assert_eq!(e.tx_owner(), Some(TxOwner::Voice));
            e.set_voice_on_air(false);
            assert_eq!(
                try_key(&mut e, P1, t),
                Ok(()),
                "control: {t:?} once the message's over ended"
            );
        }
    }

    /// A program's admission moves the transmit gate's generation, as every change of who may
    /// transmit does, so an over Nexus planned before it never keys (`commit_tx`); its re-assert
    /// moves nothing.
    #[test]
    fn a_programs_admission_cancels_an_over_planned_before_it() {
        let mut e = phone();
        let before = e.tx_gate_gen;
        assert_eq!(try_key(&mut e, P1, Try::Mic), Ok(()));
        assert_ne!(
            e.tx_gate_gen, before,
            "the admission left a planned over keyable"
        );
        let before = e.tx_gate_gen;
        assert_eq!(try_key(&mut e, P1, Try::Mic), Ok(()), "its re-assert");
        assert_eq!(
            e.tx_gate_gen, before,
            "a re-assert cancelled a planned over"
        );
    }

    /// ⛔ NEXUS'S OWN KEY WINS: the operator's PTT or Tune during a program's over takes
    /// the transmitter, and the next tick ends the over with its abort. A program's microphone
    /// key goes down at once.
    #[test]
    fn nexus_own_key_ends_a_programs_over_on_the_next_tick() {
        type Press = fn(&mut Engine);
        let presses: [(&str, Press); 2] = [
            ("the operator's PTT", |e| e.set_ptt(true)),
            ("the operator's Tune", |e| e.set_tune(true)),
        ];
        for (name, press) in presses {
            let mut s = Scene::new(phone());
            s.keyed();
            press(&mut s.e);
            s.audio(P1, 20, 0.5);
            assert_eq!(s.tick(20), MicTick::Idle, "{name}: the over went on");
            assert!(s.e.take_tci_abort(), "{name}: no abort");
        }
        let mut s = Scene::new(phone());
        assert_eq!(try_key(&mut s.e, P1, Try::Mic), Ok(()));
        s.e.set_tune(true);
        assert!(
            !s.e.tci_mic_keyed(),
            "a program's microphone keyed under the operator's tune"
        );
        s.tick(20);
        assert_eq!(s.e.tci_owner(), None, "the program kept its key");
    }

    /// ⛔ A DATA-MODE OVER FROM A PROGRAM IS HELD LIKE FT8, IN ANY SECTION (the operator's ruling of
    /// 2026-10-20). WSJT-X's shape on the Phone screen at 14.074 with the licence class Open, where
    /// no other gate refuses a phone emission: `modulation:0,digu` and then `trx:0,true,tci` is
    /// refused as slot-timed, told `tx_enable` false, and arms nothing; `digl` the same. CONTROLS:
    /// another program that asked `usb` is admitted (one program's ask holds only that program), and
    /// the first one's later `usb` lets its next over in. A lost connection forgets its ask, so its
    /// number is a program that asked nothing (`VoiceMode`, not `SlotTimed`). An over keyed before
    /// its program asks a data mode ends with its abort on the next tick.
    #[test]
    fn a_programs_data_mode_over_is_held_like_ft8_in_any_section() {
        let mut e = phone();
        e.set_license_class("open");
        e.set_frequency(14.074, "20m", "USB");
        assert!(
            e.tx_allowed() && e.tci_over_licensed(),
            "premise: the Open class lets a phone emission out at 14.074"
        );
        e.tci_note_modulation(P1, &word("digu"));
        assert_eq!(
            e.tci_key(P1, KeyKind::Tci, true),
            Err(Refusal::SlotTimed),
            "WSJT-X's shape"
        );
        assert!(!e.tci_tx_enable(P1, true), "told it may transmit");
        assert!(!e.tci_over_armed(), "an over was armed");
        e.tci_note_modulation(P2, &word("usb"));
        assert_eq!(
            e.tci_key(P2, KeyKind::Tci, true),
            Ok(()),
            "control: a program that asked usb"
        );
        e.tci_release(P2);
        e.tci_note_modulation(P1, &word("digl"));
        assert_eq!(
            e.tci_key(P1, KeyKind::Tci, true),
            Err(Refusal::SlotTimed),
            "digl"
        );
        e.tci_note_modulation(P1, &word("usb"));
        assert_eq!(
            e.tci_key(P1, KeyKind::Tci, true),
            Ok(()),
            "control: after its usb"
        );
        e.tci_release(P1);
        e.tci_note_modulation(P2, &word("digu"));
        e.tci_disconnected(P2);
        assert_eq!(
            e.tci_key(P2, KeyKind::Tci, true),
            Err(Refusal::VoiceMode),
            "a lost program's ask outlived it"
        );
        // Asked part way through an over: it ends with its abort on the next tick.
        let mut s = Scene::new(phone());
        s.keyed();
        s.e.tci_note_modulation(P1, &word("digu"));
        s.audio(P1, 20, 0.5);
        assert_eq!(
            s.tick(20),
            MicTick::Idle,
            "the over went on after its program asked digu"
        );
        assert!(s.e.take_tci_abort(), "no abort");
    }

    /// ⛔ A PROGRAM'S OWN AUDIO GOES OUT ONLY AFTER IT ASKS A VOICE MODE (the operator's ruling of
    /// 2026-10-20). WSJT-X's default shape (its Radio mode "None": no `modulation:` at all, then
    /// `trx:0,true,tci`) on the Phone screen at 14.074 with the licence class Open, where no other
    /// gate refuses a phone emission, is refused as `VoiceMode`, told `tx_enable` false, and arms
    /// nothing. Every mode the server offers programs is judged by name: `usb`, `lsb`, `am` and
    /// `nfm` (TCI's FM) let the over in, `digu` and `digl` hold it like FT8, `cw` refuses it.
    /// CONTROLS: the program's microphone key and its Tune, which carry no audio of its own, are
    /// admitted with no mode asked; after `usb` its over is admitted and `tx_enable` says so;
    /// `digu` holds it again and a later `usb` lets its next over in. A lost connection forgets a
    /// voice ask too. In Digital and in CW the section's refusal comes first. An over keyed before
    /// its program asks `cw` ends with its abort on the next tick; one whose program asks another
    /// voice mode goes on.
    #[test]
    fn a_programs_own_audio_goes_out_only_after_it_asks_a_voice_mode() {
        let at_14074 = || {
            let mut e = phone();
            e.set_license_class("open");
            e.set_frequency(14.074, "20m", "USB");
            assert!(
                e.tx_allowed() && e.tci_over_licensed(),
                "premise: the Open class lets a phone emission out at 14.074"
            );
            e
        };
        let mut e = at_14074();
        assert_eq!(
            e.tci_key(P3, KeyKind::Tci, true),
            Err(Refusal::VoiceMode),
            "WSJT-X's default shape"
        );
        assert!(!e.tci_tx_enable(P3, true), "told it may transmit");
        assert!(!e.tci_over_armed(), "an over was armed");
        assert_eq!(e.tx_owner(), None, "something owns TX");
        assert_eq!(
            try_key(&mut e, P3, Try::Mic),
            Ok(()),
            "control: its microphone key"
        );
        e.tci_release(P3);
        assert_eq!(try_key(&mut e, P3, Try::Tune), Ok(()), "control: its Tune");
        e.tci_tune_off(P3);
        e.tci_note_modulation(P3, &word("usb"));
        assert!(
            e.tci_tx_enable(P3, true),
            "control: after its usb, told it may not transmit"
        );
        assert_eq!(
            e.tci_key(P3, KeyKind::Tci, true),
            Ok(()),
            "control: after its usb"
        );
        e.tci_release(P3);
        e.tci_note_modulation(P3, &word("digu"));
        assert_eq!(
            e.tci_key(P3, KeyKind::Tci, true),
            Err(Refusal::SlotTimed),
            "digu after usb"
        );
        e.tci_note_modulation(P3, &word("usb"));
        assert_eq!(
            e.tci_key(P3, KeyKind::Tci, true),
            Ok(()),
            "control: usb after digu"
        );
        e.tci_release(P3);
        e.tci_disconnected(P3);
        assert_eq!(
            e.tci_key(P3, KeyKind::Tci, true),
            Err(Refusal::VoiceMode),
            "a lost program's usb outlived it"
        );

        // Every mode the server offers programs, by name.
        let judged = [
            ("usb", Ok(())),
            ("lsb", Ok(())),
            ("am", Ok(())),
            ("nfm", Ok(())),
            ("digu", Err(Refusal::SlotTimed)),
            ("digl", Err(Refusal::SlotTimed)),
            ("cw", Err(Refusal::VoiceMode)),
        ];
        let mut names: Vec<&str> = judged.iter().map(|(w, _)| *w).collect();
        let mut offered = tempo_net::tci::server::MODULATIONS.to_vec();
        names.sort_unstable();
        offered.sort_unstable();
        assert_eq!(names, offered, "the modes the server offers programs");
        let mut wrong = Vec::new();
        for (w, want) in judged {
            let mut e = at_14074();
            e.tci_note_modulation(P3, &word(w));
            let got = e.tci_key(P3, KeyKind::Tci, true);
            if got != want {
                wrong.push(format!("after {w}: got {got:?}, want {want:?}"));
            }
        }
        assert!(wrong.is_empty(), "the modes:\n{}", wrong.join("\n"));

        assert_eq!(
            digital().tci_key(P3, KeyKind::Tci, true),
            Err(Refusal::SlotTimed),
            "Digital: the section first"
        );
        assert_eq!(
            cw_section().tci_key(P3, KeyKind::Tci, true),
            Err(Refusal::Section),
            "CW: the section first"
        );

        // Asked part way through an over: another voice mode keeps it, cw ends it.
        let mut s = Scene::new(phone());
        s.keyed();
        s.e.tci_note_modulation(P1, &word("lsb"));
        s.audio(P1, 20, 0.5);
        assert_ne!(
            s.tick(20),
            MicTick::Idle,
            "control: another voice mode ended the over"
        );
        s.e.tci_note_modulation(P1, &word("cw"));
        s.audio(P1, 40, 0.5);
        assert_eq!(
            s.tick(40),
            MicTick::Idle,
            "the over went on after its program asked cw"
        );
        assert!(s.e.take_tci_abort(), "no abort");
    }

    /// ⛔ A PROGRAM'S OVER NEVER GOES OUT ON THE RADIO'S OWN MICROPHONE. With Nexus's own Flex client
    /// reporting the radio on its mic (native audio on, Phone at the shack), the over's audio would
    /// leave over DAX, which the radio then ignores, and the microphone would carry the over: a key
    /// is refused, its `tx_enable` is false and nothing is armed; a keyed over ends with its abort
    /// on the tick the radio takes its mic back. CONTROLS: the program's microphone key and its Tune
    /// are what they ask for and stay admitted; with the radio on DAX the over is admitted.
    #[test]
    fn a_programs_over_never_goes_out_on_the_radios_own_microphone() {
        let mut e = phone();
        e.observe_flex_radio_has_mic(true);
        assert_eq!(
            e.tci_key(P1, KeyKind::Tci, true),
            Err(Refusal::RadioMic),
            "the over's key"
        );
        assert!(!e.tci_tx_enable(P1, true), "told it may transmit");
        assert!(!e.tci_over_armed(), "an over was armed");
        assert_eq!(e.tci_owner(), None);
        assert_eq!(
            try_key(&mut e, P1, Try::Mic),
            Ok(()),
            "control: its microphone key"
        );
        e.tci_release(P1);
        assert_eq!(try_key(&mut e, P1, Try::Tune), Ok(()), "control: its Tune");
        e.tci_tune_off(P1);
        e.observe_flex_radio_has_mic(false);
        assert_eq!(e.tci_key(P1, KeyKind::Tci, true), Ok(()), "control: on DAX");
        // Keyed, and then the radio takes its mic back.
        let mut s = Scene::new(phone());
        s.keyed();
        s.e.observe_flex_radio_has_mic(true);
        s.audio(P1, 20, 0.5);
        assert_eq!(s.tick(20), MicTick::Idle, "the over went on on the mic");
        assert!(s.e.take_tci_abort(), "no abort");
        assert!(!s.e.tci_over_armed());
    }

    /// ⛔ AN ARMED FT RUN PLANS NO OVER WHILE A PROGRAM'S KEY OWNS THE TRANSMITTER:
    /// the slot is skipped as for a tune, the run stays armed and its next free slot plans as
    /// usual. A program's over in Digital waits for the bench, so it is set up here as one will
    /// stand once allowed.
    #[test]
    fn an_armed_ft_run_plans_no_over_under_a_programs_key() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        e.start_cq(None).unwrap();
        let slot = if e.tx_even() { 0 } else { 1 };
        e.tci.key = Some(Key {
            program: P1,
            mic: false,
            since_ms: now_unix_millis(),
            section: OperatingMode::Digital,
        });
        assert!(e.tci.over.arm());
        assert_eq!(e.tx_owner(), Some(TxOwner::Tci), "premise");
        assert!(
            e.plan_tx(slot).is_none(),
            "an FT over planned under a program's over"
        );
        assert!(e.tx_enabled(), "the run was disarmed");
        e.drop_tci_over();
        assert!(
            e.plan_tx(slot).is_some(),
            "control: the run plans once the program let go"
        );
    }

    // ── The section, the band, the watchdog ──────────────────────────────────────────────────

    /// A program's microphone key is Phone's: leaving Phone takes it down at once and the next
    /// tick ends it. A program's over ends on the next tick of a section change.
    #[test]
    fn leaving_phone_ends_a_programs_microphone_key_and_over() {
        let mut s = Scene::new(phone());
        assert_eq!(try_key(&mut s.e, P1, Try::Mic), Ok(()));
        s.e.set_operating_mode("cw", false);
        assert!(!s.e.manual_ptt(), "the microphone stayed keyed in CW");
        s.tick(20);
        assert_eq!(s.e.tci_owner(), None);

        let mut s = Scene::new(phone());
        s.keyed();
        s.e.set_operating_mode("cw", false);
        s.audio(P1, 20, 0.5);
        assert_eq!(s.tick(20), MicTick::Idle, "the over went on in CW");
        assert!(s.e.take_tci_abort());
    }

    /// ⛔ AFTER A BAND CHANGE A PROGRAM WAITS FOR THE OPERATOR'S ARM (the CAT
    /// broker's rule): the change ends a program's key, a new one is refused under the context
    /// hold in Phone and as disarmed in Digital, and the operator's TX On admits it.
    #[test]
    fn after_a_band_change_a_program_waits_for_the_operators_arm() {
        let mut e = phone();
        assert_eq!(try_key(&mut e, P1, Try::Mic), Ok(()));
        e.set_frequency(7.200, "40m", "LSB");
        assert!(!e.manual_ptt(), "the key rode the band change");
        assert!(e.tx_enabled(), "premise: Phone keeps the operator's arm");
        assert_eq!(try_key(&mut e, P1, Try::Mic), Err(Refusal::ContextHold));
        e.set_tx_enabled_by_operator(true);
        assert_eq!(
            try_key(&mut e, P1, Try::Mic),
            Ok(()),
            "control: after the arm"
        );

        let mut e = digital();
        e.set_frequency(7.074, "40m", "USB");
        assert_eq!(try_key(&mut e, P1, Try::Tune), Err(Refusal::Disarmed));
        e.set_tx_enabled_by_operator(true);
        assert_eq!(
            try_key(&mut e, P1, Try::Tune),
            Ok(()),
            "control: after the arm"
        );
    }

    /// ⛔ A PROGRAM NEVER STARTS OR RESTARTS THE WATCHDOG CLOCK: its over, played for
    /// two seconds, and its tune leave the clock as it was, stopped or running. A trip mid-over
    /// ends the over on the next tick. CONTROL: the operator's Tune restarts the clock.
    #[test]
    fn a_program_never_starts_or_restarts_the_watchdog_clock() {
        for start in [None, Some(now_unix_secs() - 30)] {
            let mut s = Scene::new(phone());
            s.e.tx_watchdog_start = start;
            s.keyed();
            let (_, dropped) = s.play(20, 2_000, 0.5);
            assert_eq!(dropped, None);
            assert_eq!(s.e.tx_watchdog_start, start, "the over moved the clock");
            s.e.halt_tx();
            s.e.set_tx_enabled_by_operator(true);
            s.e.tx_watchdog_start = start;
            assert_eq!(try_key(&mut s.e, P1, Try::Tune), Ok(()));
            assert_eq!(s.e.tx_watchdog_start, start, "the tune moved the clock");
        }
        let mut e = phone();
        e.tx_watchdog_start = Some(1);
        e.set_tune(true);
        assert_eq!(
            e.tx_watchdog_start, None,
            "control: the operator's Tune restarts it"
        );

        let mut s = Scene::new(phone());
        s.keyed();
        trip(&mut s.e);
        s.audio(P1, 20, 0.5);
        assert_eq!(s.tick(20), MicTick::Idle, "a trip left the over on the air");
        assert!(s.e.take_tci_abort());
    }

    // ── Tune ─────────────────────────────────────────────────────────────────────────────────

    /// A program's tune is Nexus's carrier, held for that program: its own `stop;` and loss end
    /// it, another's do not; anyone's `tune:0,false` ends any tune; and with none up it does
    /// nothing, so it cannot cancel a planned over.
    #[test]
    fn a_programs_tune_ends_with_it_and_tune_off_is_never_gated() {
        let mut e = phone();
        assert_eq!(try_key(&mut e, P1, Try::Tune), Ok(()));
        assert_eq!(
            (e.tx_owner(), e.tci_owner()),
            (Some(TxOwner::Tune), Some(P1))
        );
        assert_eq!(try_key(&mut e, P1, Try::Tune), Ok(()), "its re-assert");
        e.tci_end_own(P2);
        assert!(e.tuning(), "another program's stop; ended P1's tune");
        e.tci_end_own(P1);
        assert!(!e.tuning(), "its own stop; left the tune up");

        let mut e = phone();
        e.set_tune(true);
        e.tci_tune_off(P2);
        assert!(
            !e.tuning(),
            "a program's tune:false left the operator's tune up"
        );

        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Ft8);
        e.start_cq(None).unwrap();
        let slot = if e.tx_even() { 0 } else { 1 };
        let plan = e.plan_tx(slot).expect("premise: an over planned");
        e.tci_tune_off(P2);
        assert!(
            !e.commit_tx(&plan, vec![0.0; 120], slot).is_empty(),
            "a stray tune:false cancelled the planned over"
        );
    }

    // ── Dial and power ───────────────────────────────────────────────────────────────────────

    /// ⛔ NO RETUNE ON THE AIR: a program's dial or mode change is refused while
    /// anything is on the air, a program's admitted key included. CONTROL: idle, it is not.
    #[test]
    fn a_program_may_not_retune_while_anything_is_on_the_air() {
        let mut e = phone();
        assert_eq!(e.tci_retune_refusal(), None, "control: idle");
        assert_eq!(try_key(&mut e, P1, Try::Over(true)), Ok(()));
        assert_eq!(
            e.tci_retune_refusal(),
            Some(Refusal::OnAir),
            "a program's armed over"
        );
        let mut e = phone();
        e.app.radio.transmitting = true;
        assert_eq!(
            e.tci_retune_refusal(),
            Some(Refusal::OnAir),
            "Nexus's own over"
        );
        let mut e = phone();
        e.set_tune(true);
        assert_eq!(
            e.tci_retune_refusal(),
            Some(Refusal::OnAir),
            "the tune carrier"
        );
    }

    /// ⛔ A PROGRAM MAY LOWER THE POWER, NEVER RAISE IT: above the level in force,
    /// or with none known, it is refused and nothing moves; at it, nothing is written; below it,
    /// the level lands through the operator's ceiling.
    #[test]
    fn a_program_may_lower_the_power_and_never_raise_it() {
        let mut e = phone();
        assert_eq!(e.tci_set_drive(10), Err(Refusal::Raise), "no level known");
        assert_eq!(e.rf_power, None, "a refused drive wrote a level");
        e.set_rf_power(0.5);
        assert_eq!(e.tci_set_drive(60), Err(Refusal::Raise));
        assert_eq!(e.rf_power, Some(0.5), "a refused raise moved the power");
        assert_eq!(e.tci_set_drive(50), Ok(50));
        assert_eq!(e.tci_set_drive(30), Ok(30));
        assert_eq!(e.rf_power, Some(0.3));
        // The rig's own level above the operator's ceiling: lowered, and through the ceiling.
        let mut e = phone();
        e.settings.max_power_phone = Some(0.4);
        e.rig_rf_power = Some(0.8);
        assert_eq!(e.tci_set_drive(70), Ok(40), "the ceiling did not hold");
    }

    // ── The microphone key ───────────────────────────────────────────────────────────────────

    /// A program's microphone key keys the radio's own microphone as the CAT broker's does
    /// (`manual_ptt`), owns the transmitter as a program, and its owner's release ends it at once.
    #[test]
    fn a_programs_microphone_key_is_held_until_its_release() {
        let mut e = phone();
        assert_eq!(try_key(&mut e, P1, Try::Mic), Ok(()));
        assert!(e.manual_ptt(), "the program's microphone key keyed nothing");
        assert_eq!(e.tx_owner(), Some(TxOwner::Tci));
        assert!(!e.tci_over_armed(), "a microphone key armed an over");
        e.tci_release(P1);
        assert!(!e.manual_ptt(), "its release left it keyed");
        assert!(
            e.tx_enabled() && !e.tx_stop_hold,
            "its own release halted TX"
        );
    }
}
