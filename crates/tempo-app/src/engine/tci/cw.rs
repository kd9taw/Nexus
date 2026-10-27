//! A program's CW on Nexus's TCI server: a logger's CW text,
//! `cw_macros:0,<text>;` or `cw_msg:0,<prefix>,<call>,<suffix>;`, keyed by Nexus's own CW keyer,
//! behind every gate in front of Nexus's own transmissions, in the CW section only.
//!
//! ## What a program's CW goes through
//!
//! - **The verdict** ([`Engine::tci_verdict`], the CW ask): "Programs on TCI may transmit" and the
//!   CW tier (`Settings::tci_server_cw`), then every station gate (Stop's hold, a watchdog trip, TX
//!   armed, the context hold, the connection, the licence as CW), one owner of the transmitter, no
//!   clock repair, and the CW section. ⛔ A program never arms: its CW is queued as Enter Sends
//!   Message queues (`send_cw_armed`), never as an F-key, which is the operator's act and turns TX
//!   on. Nothing here writes the latch, a hold, a trip or the watchdog clock.
//! - **The text filter** ([`cw_plan`]): only characters every one of Nexus's keyers sends, by the
//!   one rule Nexus's own CW keeps on the radio's keyer too ([`tempo_core::cw::sendable`]), or the
//!   whole command is refused ([`Refusal::CwText`]) and nothing is queued; never a `;`, which would
//!   carry a second command to a Kenwood, Yaesu or Elecraft radio inside the rig keyer's own one.
//! - **The programs' speed** ([`Engine::tci_cw_wpm`]): what a program set (`cw_macros_speed`,
//!   `_up`, `_down`) inside the keyer's range, else the operator's own. ⛔ A program never moves
//!   the operator's speed: Nexus's CW control, F-keys and paddle keep theirs, and the radio loop
//!   puts the operator's back on the radio's keyer after a program's words, as after CW speed
//!   match's.
//! - **The one CW queue**, word by word, as Nexus's own CW goes: the radio loop keys it on whichever
//!   keyer the station uses and re-checks TX armed, the connection, the licence and the clock
//!   repair before every word (`poll_cw_word`). Each queued message names its program, so a
//!   program's stops end its own CW and nobody else's.
//!
//! ## How it ends: on `cw_macros_stop;` from anyone, and on every stop
//!
//! - Stop TX, Esc, TX Off and every `halt_tx`: the CW stop they already make for all CW (the queue
//!   dropped, the word on the air cut mid-word by the radio loop), and the stop holds programs off
//!   until the operator's own arm.
//! - "Programs on TCI may transmit" or the CW tier switched off, the program's own `stop;` and its
//!   lost connection: [`Engine::end_tci_cw`], hard, now: its words still queued are dropped, and its
//!   word the keyer is sending, or may be, is cut mid-word.
//! - `cw_macros_stop` from any program: CW from any origin ([`Engine::tci_cw_stop`]).
//!
//! What stays as it was: an F-key, typed text and Enter Sends Message queue behind a program's CW
//! as behind any CW, and Esc stops both.
use super::*;
use crate::engine::{TxOwner, CW_KEYING_MARGIN_MS};
use std::collections::VecDeque;
use std::time::Duration;
use tempo_net::tci::server::{Cw, CwSpeed};
use tempo_net::tci::wire::Text;

/// The most characters one program command keys, its marks and a `cw_msg`'s repeated call
/// counted.
const CW_TEXT_MAX: usize = 256;
/// The most of programs' CW that may wait in the queue at once, in characters: four whole
/// commands. TCI queues a macro behind the one being sent; a program that keeps sending is refused
/// rather than piling up an hour of CW.
const CW_WAITING_MAX: usize = 4 * CW_TEXT_MAX;
/// The speeds Nexus's CW keyer keys at, in WPM: the operator's own control's range
/// ([`Engine::set_cw_wpm`]), so a program reaches no speed the operator could not set.
const CW_WPM: (u32, u32) = (5, 50);
/// How far one `<` or `>` in a macro moves the speed (TCI 2.0 §3.2.1: "The speed step is 5 wpm").
const CW_MARK_WPM: u32 = 5;
/// The longest the radio loop takes between taking a word from the queue and reporting it keying
/// ([`Engine::observe_cw_keying`]), which it does at the end of the same tick: a slow serial
/// radio's CAT deadline (2.5 s) and some.
const CW_HANDOFF_MS: u64 = 3_000;

/// Programs' CW, as the engine holds it.
#[derive(Debug, Default)]
pub(super) struct TciCw {
    /// The programs' speed as a program set it, or `None`: the operator's own.
    wpm: Option<u32>,
    /// The CW word the radio loop took last: whose it was, and the latest it can still be keying.
    word: Option<Word>,
}

/// A CW word the radio loop handed a keyer.
#[derive(Debug, Clone, Copy)]
struct Word {
    /// The program that sent it; `None`: Nexus's own.
    from: Option<u64>,
    /// Its Morse, its word space, the keying margin and the loop's hand-off, from its taking.
    until: Instant,
}

fn wpm_in_range(wpm: u32) -> u32 {
    wpm.clamp(CW_WPM.0, CW_WPM.1)
}

impl Engine {
    // ── Keying ───────────────────────────────────────────────────────────────────────────────

    /// `cw_macros` or `cw_msg` from `program`: the verdict, then the text filter, then its words
    /// queued for the radio loop at the programs' speed, behind its own CW if that is on the air
    /// (TCI's macros queue in line). `Ok` when queued, or with nothing to send; otherwise the first
    /// gate that failed, or [`Refusal::CwText`] for text Nexus will not key, refused whole.
    pub fn tci_cw(&mut self, program: u64, cw: &Cw) -> Result<(), Refusal> {
        self.tci_verdict(program, Ask::Cw, false)?;
        let plan = cw_plan(cw, self.tci_cw_wpm())?;
        if plan.is_empty() {
            return Ok(());
        }
        let text = plan
            .iter()
            .map(|(words, _)| words.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        // A fresh admission finds the queue empty (nothing owned the transmitter); one in line
        // finds only this program's own words there.
        let waiting: usize = self.cw_queue.iter().map(|w| w.len() + 1).sum();
        if waiting + text.len() > CW_WAITING_MAX {
            return Err(Refusal::CwText);
        }
        if !self.tci_reasserts(program, Ask::Cw) {
            // Who may transmit changed hands: an over planned before it must not key.
            self.tx_gate_gen = self.tx_gate_gen.wrapping_add(1);
        }
        self.note_cw_send(&text);
        for (words, wpm) in &plan {
            self.queue_cw_message(words, Some(*wpm), Some(program));
        }
        let speeds = plan
            .iter()
            .map(|(_, wpm)| wpm.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        tempo_core::applog::info(
            "tx",
            &format!("TCI program {program} queued CW: {text:?} at {speeds} WPM"),
        );
        Ok(())
    }

    /// `cw_macros_stop` from any program: CW stops, from any origin, Nexus's own included,
    /// as the CW keyer's own stop stops it: what is queued is dropped and the word on the
    /// air is cut mid-word. Never gated, and nothing but CW: with anything else on the air it does
    /// nothing, because the CW stop also cuts a slot over in flight. No hold: it is not the
    /// operator's Stop.
    pub fn tci_cw_stop(&mut self, _program: u64) {
        if self.tx_owner() == Some(TxOwner::Cw) {
            self.stop_cw();
            self.tci.cw.word = None;
        } else if self.tx_owner().is_none() && self.cw_word_may_key(|_| true) {
            // The word the loop has just handed a keyer and not reported yet: the arbiter cannot
            // see it until the loop's report, so it is cut on its own.
            self.cut_cw_word();
            self.tci.cw.word = None;
        }
    }

    // ── Speed ────────────────────────────────────────────────────────────────────────────────

    /// A CW speed from a program (`cw_macros_speed`, `cw_macros_speed_up`, `cw_macros_speed_down`):
    /// the programs' speed, held to the keyer's range ([`CW_WPM`]), for the CW they send next.
    /// ⛔ NEVER the operator's own speed (`Settings::cw_wpm`). `Ok` with the programs' speed now in
    /// force; refused (`Tier`) while the CW tier is off.
    pub fn tci_cw_speed(&mut self, speed: CwSpeed) -> Result<u32, Refusal> {
        if !self.settings.tci_server_cw {
            return Err(Refusal::Tier);
        }
        let now = self.tci_cw_wpm();
        let asked = match speed {
            CwSpeed::Set(wpm) => wpm,
            CwSpeed::Up(n) => now.saturating_add(n),
            CwSpeed::Down(n) => now.saturating_sub(n),
        };
        self.tci.cw.wpm = Some(wpm_in_range(asked));
        Ok(self.tci_cw_wpm())
    }

    /// The speed programs' CW keys at, WPM: what a program set, else the operator's own.
    pub fn tci_cw_wpm(&self) -> u32 {
        wpm_in_range(self.tci.cw.wpm.unwrap_or(self.cw_wpm()))
    }

    // ── Who is on the air ────────────────────────────────────────────────────────────────────

    /// The program whose CW is on the air, as the arbiter sees CW ([`TxOwner::Cw`]): the program
    /// whose word the radio's keyer is sending, or, between two words, whose word is next. `None`
    /// while CW does not own the transmitter, and while it is Nexus's own.
    pub(super) fn tci_cw_on_air(&self) -> Option<u64> {
        if self.tx_owner() != Some(TxOwner::Cw) {
            return None;
        }
        if self.cw_keying() {
            return self.tci.cw.word.and_then(|w| w.from);
        }
        if self.cw_queue.is_empty() {
            return None;
        }
        self.cw_messages.front().and_then(|m| m.3)
    }

    /// Whether `program`'s CW may be extended in line: it is on the air, and nothing of anyone
    /// else's waits behind it. So a program's words in the queue are always one run at its front,
    /// and its stops can end them without touching anyone else's.
    pub(super) fn tci_cw_extends(&self, program: u64) -> bool {
        if self.tci_cw_on_air() != Some(program) {
            return false;
        }
        let queued: usize = self.cw_messages.iter().map(|m| m.0).sum();
        self.cw_queue.is_empty()
            || (queued == self.cw_queue.len()
                && self.cw_messages.iter().all(|m| m.3 == Some(program)))
    }

    /// The radio loop took `word` from the CW queue to key at `wpm`, sent by `from` (a program, or
    /// `None`: Nexus's own): whose word the keyer has, and the latest it can still be keying.
    pub(in crate::engine) fn note_tci_cw_word(&mut self, from: Option<u64>, word: &str, wpm: u32) {
        let space = 7.0 * 1200.0 / f64::from(wpm.clamp(5, 60));
        let ms = tempo_core::cw::morse_duration_ms(word, wpm) + space;
        let ms = ms.ceil() as u64 + CW_KEYING_MARGIN_MS + CW_HANDOFF_MS;
        self.tci.cw.word = Some(Word {
            from,
            until: Instant::now() + Duration::from_millis(ms),
        });
    }

    /// Whether the word the keyer was handed last is one `whose` names and may still be keying:
    /// the radio loop says so, or it was taken so lately that the loop has not said yet.
    fn cw_word_may_key(&self, whose: impl Fn(Option<u64>) -> bool) -> bool {
        self.tci
            .cw
            .word
            .is_some_and(|w| whose(w.from) && (self.cw_keying() || Instant::now() < w.until))
    }

    // ── Ending ───────────────────────────────────────────────────────────────────────────────

    /// ⛔ THE CW THAT `whose` NAMES ENDS, HARD, NOW: its words still queued are
    /// dropped, and its word the keyer is sending, or may be, is cut mid-word (the radio loop stops
    /// every keyer on its next tick: the radio's own, a WinKeyer, the keyline, the soundcard tone).
    /// Anyone else's CW queued behind it keys as it would have. With none of its CW queued or on
    /// the air, nothing happens.
    pub(super) fn end_tci_cw(&mut self, whose: impl Fn(Option<u64>) -> bool, why: &str) {
        let owned = self.tx_owner() == Some(TxOwner::Cw);
        let keying = self.cw_word_may_key(&whose);
        let dropped = self.drop_cw_messages(&whose);
        if keying {
            if owned && self.cw_queue.is_empty() {
                // Nothing else waits: the CW stop itself, as Esc and `cw_macros_stop` make it.
                self.stop_cw();
            } else {
                // Cut that word alone: what waits behind it keys next, and the transmitter is not
                // known to be CW's, so the slot-TX abort is not armed.
                self.cut_cw_word();
            }
            self.tci.cw.word = None;
        }
        if keying || dropped > 0 {
            tempo_core::applog::info(
                "tx",
                &format!(
                    "TCI CW ended: {why} ({dropped} queued words dropped{})",
                    if keying {
                        ", the word on the air cut"
                    } else {
                        ""
                    }
                ),
            );
        }
    }

    /// Every program's CW ends hard ([`Self::end_tci_cw`]): the stops' hook, and the CW tier
    /// switched off.
    pub(in crate::engine) fn end_programs_cw(&mut self, why: &str) {
        self.end_tci_cw(|from| from.is_some(), why);
    }

    /// Cut the word the keyer was handed last, and nothing else: the radio loop stops every keyer
    /// on its next tick (`take_cw_abort`), and the word no longer owns the transmitter.
    fn cut_cw_word(&mut self) {
        self.cw_abort = true;
        self.cw_keying_until = None;
    }

    /// Drop the queued CW messages `whose` names, with their words; everyone else's stay, in order.
    /// Answers the words dropped.
    fn drop_cw_messages(&mut self, whose: impl Fn(Option<u64>) -> bool) -> usize {
        if self.cw_queue.is_empty() {
            return 0;
        }
        let mut words = std::mem::take(&mut self.cw_queue).into_iter();
        let (mut kept, mut messages, mut dropped) = (VecDeque::new(), VecDeque::new(), 0);
        for message in std::mem::take(&mut self.cw_messages) {
            let part: Vec<String> = words.by_ref().take(message.0).collect();
            if whose(message.3) {
                dropped += part.len();
            } else {
                kept.extend(part);
                messages.push_back(message);
            }
        }
        // A word queued outside any message is no program's.
        kept.extend(words);
        self.cw_queue = kept;
        self.cw_messages = messages;
        dropped
    }
}

/// ⛔ THE TEXT FILTER: a program's CW command as Nexus keys it, runs of words at their speed in the
/// order they go out, or [`Refusal::CwText`], refused whole and never sent changed:
/// - only characters every one of Nexus's keyers sends ([`tempo_core::cw::sendable`], the rule
///   Nexus's own CW keeps on the radio's keyer too: the letters in either case, the digits and
///   `. , ? / = + - ( ) : @`), with spaces between words;
/// - ⛔ never `;`, which has Morse but ends a CAT command on Kenwood, Yaesu and Elecraft radios: in
///   a word the radio's own keyer sends (`KY TEST;FA00007000000;`) it carries a second command to
///   the radio, here a retune;
/// - TCI's marks (TCI 2.0 §3.2.1), applied: each `>` or `<` raises or lowers the speed by
///   [`CW_MARK_WPM`] inside the keyer's range, from the word it begins (inside a word, from the
///   next one); a prosign's `|`s are dropped and its letters go out as a word, since no keyer here
///   runs letters together; an unpaired `|` is refused;
/// - `cw_msg`'s parts as one text: prefix, call and suffix, `_` for a part left empty, and `$n`
///   (1-9) after the call to send it n times;
/// - at most [`CW_TEXT_MAX`] characters.
fn cw_plan(cw: &Cw, wpm: u32) -> Result<Vec<(String, u32)>, Refusal> {
    let text = match cw {
        Cw::Macro(text) => text.as_str().to_string(),
        Cw::Message {
            prefix,
            call,
            suffix,
        } => {
            let part = |t: &Text| -> String {
                if t.as_str() == "_" {
                    String::new()
                } else {
                    t.as_str().to_string()
                }
            };
            let call = repeated(&part(call))?;
            [part(prefix), call, part(suffix)]
                .into_iter()
                .filter(|p| !p.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        }
    };
    if text.len() > CW_TEXT_MAX || text.matches('|').count() % 2 != 0 {
        return Err(Refusal::CwText);
    }
    let mut wpm = wpm_in_range(wpm);
    let mut plan: Vec<(String, u32)> = Vec::new();
    for raw in text.split(' ') {
        let (mut word, mut at) = (String::new(), wpm);
        for c in raw.chars() {
            match c {
                '>' => wpm = wpm_in_range(wpm.saturating_add(CW_MARK_WPM)),
                '<' => wpm = wpm_in_range(wpm.saturating_sub(CW_MARK_WPM)),
                '|' => {}
                c if tempo_core::cw::sendable(c) => word.push(c.to_ascii_uppercase()),
                _ => return Err(Refusal::CwText),
            }
            if word.is_empty() {
                at = wpm;
            }
        }
        if word.is_empty() {
            continue;
        }
        match plan.last_mut() {
            Some((words, w)) if *w == at => {
                words.push(' ');
                words.push_str(&word);
            }
            _ => plan.push((word, at)),
        }
    }
    Ok(plan)
}

/// A `cw_msg` call with its repeat (TCI 2.0 §3.2.2): `RA6LH$2` is `RA6LH RA6LH`.
fn repeated(call: &str) -> Result<String, Refusal> {
    let Some((base, n)) = call.rsplit_once('$') else {
        return Ok(call.to_string());
    };
    match n.as_bytes() {
        [d @ b'1'..=b'9'] => Ok(vec![base; usize::from(d - b'0')].join(" ")),
        _ => Err(Refusal::CwText),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P1: u64 = 1;
    const P2: u64 = 2;

    /// A station in the CW section on 40 m with every privilege, TX armed by the operator after the
    /// move from 20 m (a band change holds programs' keys until that arm), the operator's own CW
    /// at 20 WPM, and programs allowed to transmit (the CW tier is on by default).
    fn cw() -> Engine {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_license_class("extra");
        e.set_operating_mode("cw", false);
        e.set_frequency(7.030, "40m", "CW");
        e.set_tx_enabled_by_operator(true);
        assert!(
            !e.broker_context_hold,
            "premise: the operator's arm let go of the band's hold"
        );
        // The band change's halt left its one-shot cuts for the radio loop, which takes them on its
        // next tick: taken here, so a cut a test sees is its own.
        let _ = (e.take_cw_abort(), e.take_slot_tx_abort());
        e.set_cw_wpm(20);
        e.settings.tci_server_tx = true;
        assert!(e.tx_enabled(), "premise: entering CW arms TX");
        assert!(
            e.settings.tci_server_cw,
            "premise: the CW tier is on by default"
        );
        e
    }

    /// The operator's settings save, changed by `change`.
    fn save(e: &mut Engine, change: fn(&mut crate::settings::Settings)) {
        let mut s = e.settings().clone();
        change(&mut s);
        e.apply_settings(s);
    }

    fn mac(text: &str) -> Cw {
        Cw::Macro(Text::new(text).expect("printable text"))
    }

    fn msg(prefix: &str, call: &str, suffix: &str) -> Cw {
        let t = |s: &str| Text::new(s).expect("printable text");
        Cw::Message {
            prefix: t(prefix),
            call: t(call),
            suffix: t(suffix),
        }
    }

    fn words(list: &[(&str, u32)]) -> Vec<(String, u32)> {
        list.iter().map(|(w, wpm)| (w.to_string(), *wpm)).collect()
    }

    /// Every word the radio loop would key from now on, with its speed: the queue drained.
    fn keys(e: &mut Engine) -> Vec<(String, u32)> {
        std::iter::from_fn(|| e.poll_cw_word().map(|(word, wpm, _)| (word, wpm))).collect()
    }

    /// The radio loop takes the next word and reports the keyer sending it: the word is on the
    /// air, mid-word.
    fn keying(e: &mut Engine) -> String {
        let (word, wpm, _) = e.poll_cw_word().expect("premise: a word to key");
        let ms = tempo_core::cw::morse_duration_ms(&word, wpm) + 7.0 * 1200.0 / f64::from(wpm);
        e.observe_cw_keying(Some(ms));
        word
    }

    // ── The verdict ──────────────────────────────────────────────────────────────────────────

    /// ⛔ A PROGRAM'S CW IS REFUSED UNLESS "PROGRAMS ON TCI MAY TRANSMIT", THE CW SECTION AND EVERY
    /// TX GATE ALLOW IT, one row a gate, each breaking only its own from a
    /// station that admits it, for a macro and a message alike, by value; a refused command queues
    /// nothing. CONTROLS: both are admitted in the CW section, their words queued as P1's.
    #[test]
    fn a_programs_cw_is_refused_unless_the_switch_the_cw_section_and_every_gate_allow_it() {
        type Setup = fn(&mut Engine);
        let rows: Vec<(&str, Setup, Result<(), Refusal>)> = vec![
            ("control: the CW section", |_| {}, Ok(())),
            (
                "the transmit switch is off",
                |e| e.settings.tci_server_tx = false,
                Err(Refusal::Switch),
            ),
            (
                "the CW tier is off",
                |e| e.settings.tci_server_cw = false,
                Err(Refusal::Tier),
            ),
            ("Stop TX holds", |e| e.halt_tx(), Err(Refusal::StopHold)),
            (
                "the operator's TX Off holds",
                |e| e.set_tx_enabled_by_operator(false),
                Err(Refusal::StopHold),
            ),
            (
                "a watchdog trip stands",
                |e| {
                    e.tx_watchdog = true;
                    e.tx_enabled = false;
                },
                Err(Refusal::WatchdogTrip),
            ),
            (
                "TX is disarmed",
                |e| e.set_tx_enabled(false),
                Err(Refusal::Disarmed),
            ),
            (
                "a band change's hold",
                |e| e.set_frequency(14.030, "20m", "CW"),
                Err(Refusal::ContextHold),
            ),
            (
                "the Icom network connection",
                |e| e.settings.rig_conn = "icomlan".into(),
                Err(Refusal::Connection),
            ),
            (
                "a General in the Extra-only CW segment",
                |e| {
                    e.set_license_class("general");
                    e.set_frequency(7.010, "40m", "CW");
                },
                Err(Refusal::Licence),
            ),
            (
                "the operator's CW is sending",
                |e| e.send_cw("CQ"),
                Err(Refusal::Owner),
            ),
            (
                "another program's CW",
                |e| assert_eq!(e.tci_cw(P2, &mac("QRL")), Ok(()), "premise: P2's CW"),
                Err(Refusal::Owner),
            ),
            (
                "the operator's tune",
                |e| e.set_tune(true),
                Err(Refusal::Owner),
            ),
            (
                "the operator's PTT",
                |e| e.set_ptt(true),
                Err(Refusal::Owner),
            ),
            (
                "a clock repair",
                |e| e.hold_tx_for_clock_repair(Instant::now() + Duration::from_secs(600)),
                Err(Refusal::ClockRepair),
            ),
            (
                "Phone",
                |e| {
                    e.set_operating_mode("phone", false);
                    e.set_frequency(7.200, "40m", "LSB");
                },
                Err(Refusal::Section),
            ),
            (
                "Digital",
                |e| {
                    e.set_operating_mode("digital", false);
                    e.set_frequency(7.074, "40m", "USB");
                    e.set_tx_enabled_by_operator(true);
                },
                Err(Refusal::Section),
            ),
            (
                "RTTY",
                |e| {
                    e.set_operating_mode("rtty", false);
                    e.set_frequency(7.080, "40m", "LSB");
                },
                Err(Refusal::Section),
            ),
        ];
        let mut wrong = Vec::new();
        for (name, setup, want) in rows {
            for (form, ask, queued) in [
                ("cw_macros", mac("CQ TEST"), vec!["CQ", "TEST"]),
                (
                    "cw_msg",
                    msg("TU", "K1ABC", "5NN"),
                    vec!["TU", "K1ABC", "5NN"],
                ),
            ] {
                let mut e = cw();
                setup(&mut e);
                let before: Vec<String> = e.cw_queue.iter().cloned().collect();
                let got = e.tci_cw(P1, &ask);
                if got != want {
                    wrong.push(format!("{name} ({form}): got {got:?}, want {want:?}"));
                }
                let mut after: Vec<String> = e.cw_queue.iter().cloned().collect();
                let added = after.split_off(before.len().min(after.len()));
                let want_added: Vec<String> = if want.is_ok() {
                    queued.iter().map(|w| w.to_string()).collect()
                } else {
                    Vec::new()
                };
                if after != before || added != want_added {
                    wrong.push(format!(
                        "{name} ({form}): the queue went {before:?} -> {:?}",
                        e.cw_queue
                    ));
                }
                let p1 = e.cw_messages.iter().any(|m| m.3 == Some(P1));
                if p1 != want.is_ok() {
                    wrong.push(format!("{name} ({form}): P1's message queued: {p1}"));
                }
            }
        }
        assert!(wrong.is_empty(), "the verdict:\n{}", wrong.join("\n"));
    }

    // ── The text ─────────────────────────────────────────────────────────────────────────────

    /// ⛔ THE TEXT FILTER REFUSES WHAT THE KEYER CANNOT SEND, WHOLE, by value: a
    /// character with no Morse in Nexus's keyers, a `;` (inside the radio keyer's own `KY …;` it is
    /// a second CAT command: Hamlib 4.5.5 sent `KY TEST;` then `FA00007000000 ;` to a TS-2000 for
    /// `b TEST;FA00007000000`), an unpaired prosign bar, a bad repeat, over 256 characters; and a
    /// refused command queues nothing. CONTROLS: what every keyer sends goes out as the program
    /// wrote it, upper-cased, with TCI's marks, prosigns and `cw_msg` forms applied.
    #[test]
    fn the_text_filter_refuses_what_the_keyer_cannot_send_and_keys_the_rest_as_written() {
        let plan = |cw: &Cw| cw_plan(cw, 20);
        let one = |s: &str| Ok(words(&[(s, 20)]));
        for bad in [
            "CQ #",
            "TEST;FA00007000000",
            "73!",
            "A_B",
            "50$",
            "\"QSL\"",
            "QRL'",
            "{MYCALL}",
            "CQ |SK",
            "[CQ]",
        ] {
            assert_eq!(plan(&mac(bad)), Err(Refusal::CwText), "{bad:?}");
        }
        assert_eq!(
            plan(&mac(&"E".repeat(257))),
            Err(Refusal::CwText),
            "257 characters"
        );
        for call in ["K1ABC$0", "K1ABC$10", "K1ABC$X", "K1ABC$", "K1$2ABC"] {
            assert_eq!(
                plan(&msg("TU", call, "5NN")),
                Err(Refusal::CwText),
                "{call:?}"
            );
        }
        assert_eq!(
            plan(&msg("TU", "K1ABC$9", &"E".repeat(200))),
            Err(Refusal::CwText),
            "a repeat past 256 characters"
        );
        // CONTROLS, by value.
        assert_eq!(plan(&mac(&"E".repeat(256))), one(&"E".repeat(256)), "256");
        assert_eq!(plan(&mac("cq test de kd9taw")), one("CQ TEST DE KD9TAW"));
        assert_eq!(
            plan(&mac("? / = + - ( ) . , : @")),
            one("? / = + - ( ) . , : @")
        );
        assert_eq!(plan(&mac("TU 5NN |SK|")), one("TU 5NN SK"), "a prosign");
        assert_eq!(plan(&mac("  CQ   TEST ")), one("CQ TEST"), "spaces");
        assert_eq!(plan(&mac("")), Ok(vec![]), "nothing to send");
        assert_eq!(plan(&mac("> <")), Ok(vec![]), "marks alone");
        // The spec's own example (TCI 2.0 §3.2.1): each mark from the word it begins.
        assert_eq!(
            plan(&mac(">TU >599 004 |SK|")),
            Ok(words(&[("TU", 25), ("599 004 SK", 30)]))
        );
        assert_eq!(
            plan(&mac("T>U AB <CD")),
            Ok(words(&[("TU", 20), ("AB", 25), ("CD", 20)])),
            "a mark inside a word: from the next word"
        );
        assert_eq!(
            plan(&msg("TU", "RA6LH", "599 004")),
            one("TU RA6LH 599 004")
        );
        assert_eq!(
            plan(&msg("TU", "RA6LH$2", "599 004")),
            one("TU RA6LH RA6LH 599 004"),
            "the spec's repeat"
        );
        assert_eq!(
            plan(&msg("_", "RA6$2", "599 004")),
            one("RA6 RA6 599 004"),
            "the spec's empty prefix"
        );

        let mut e = cw();
        assert_eq!(
            e.tci_cw(P1, &mac("TEST;FA00007000000")),
            Err(Refusal::CwText)
        );
        assert!(
            e.cw_queue.is_empty() && e.cw_sent().is_empty() && e.tci_owner().is_none(),
            "a refused text left something to key"
        );
        assert_eq!(e.tci_cw(P1, &mac("cq >test")), Ok(()), "control");
        assert_eq!(keys(&mut e), words(&[("CQ", 20), ("TEST", 25)]));
        assert_eq!(e.cw_sent(), ["CQ TEST"], "the CW screen's echo");
    }

    // ── The speed ────────────────────────────────────────────────────────────────────────────

    /// ⛔ A PROGRAM'S SPEED IS ITS OWN, INSIDE THE KEYER'S RANGE, AND NEVER THE OPERATOR'S: by
    /// value, `cw_macros_speed` and its steps hold to 5..=50, the operator's own control's range
    /// (`set_cw_wpm` clamps the same); a program's words key at it, marks included; the operator's
    /// speed, the snapshot and an F-key keep 20; an operator's change of their own speed does not
    /// move the programs'; the CW tier off refuses a change. CONTROL: with none set, programs' CW
    /// keys at the operator's speed.
    #[test]
    fn a_programs_cw_speed_stays_inside_the_keyers_range_and_never_moves_the_operators() {
        let mut e = cw();
        assert_eq!(e.tci_cw_wpm(), 20, "control: none set, the operator's");
        assert_eq!(e.tci_cw(P1, &mac("CQ")), Ok(()));
        assert_eq!(keys(&mut e), words(&[("CQ", 20)]), "control");
        for (speed, want) in [
            (CwSpeed::Set(30), 30),
            (CwSpeed::Set(99), 50),
            (CwSpeed::Set(0), 5),
            (CwSpeed::Up(3), 8),
            (CwSpeed::Up(u32::MAX), 50),
            (CwSpeed::Down(7), 43),
            (CwSpeed::Down(u32::MAX), 5),
            (CwSpeed::Set(30), 30),
        ] {
            assert_eq!(e.tci_cw_speed(speed), Ok(want), "{speed:?}");
            assert_eq!(e.cw_wpm(), 20, "{speed:?} moved the operator's speed");
        }
        assert_eq!(e.tci_cw(P1, &mac("TEST")), Ok(()));
        assert_eq!(
            keys(&mut e),
            words(&[("TEST", 30)]),
            "at the program's speed"
        );
        assert_eq!(e.snapshot().radio.cw_wpm, 20, "the CW screen's speed");
        e.send_cw("CQ");
        assert_eq!(keys(&mut e), words(&[("CQ", 20)]), "an F-key");
        e.set_cw_wpm(25);
        assert_eq!(
            e.tci_cw_wpm(),
            30,
            "the operator's change moved the programs'"
        );
        assert_eq!(e.tci_cw_speed(CwSpeed::Set(48)), Ok(48));
        assert_eq!(e.tci_cw(P1, &mac(">A >B")), Ok(()));
        assert_eq!(
            keys(&mut e),
            words(&[("A", 50), ("B", 50)]),
            "marks at the top"
        );
        assert_eq!(e.tci_cw_speed(CwSpeed::Set(7)), Ok(7));
        assert_eq!(e.tci_cw(P1, &mac("<A")), Ok(()));
        assert_eq!(keys(&mut e), words(&[("A", 5)]), "a mark at the bottom");
        e.settings.tci_server_cw = false;
        assert_eq!(e.tci_cw_speed(CwSpeed::Set(40)), Err(Refusal::Tier));
        assert_eq!(e.tci_cw_wpm(), 7, "a refused change moved it");
        assert_eq!(e.cw_wpm(), 25);
        // The range is the operator's control's.
        e.set_cw_wpm(999);
        assert_eq!(e.cw_wpm(), 50, "premise: the control's top");
        e.set_cw_wpm(1);
        assert_eq!(e.cw_wpm(), 5, "premise: the control's bottom");
    }

    // ── Every stop ───────────────────────────────────────────────────────────────────────────

    /// ⛔ STOP IN NEXUS ENDS A PROGRAM'S CW MID-WORD AND HOLDS: with P1's macro
    /// on the air (its first word handed to the keyer and keying), Stop TX, Esc (the CW cockpit's:
    /// the CW stop and Stop TX), TX Off, "Programs on TCI may transmit" switched off and the CW
    /// tier switched off each arm the radio loop's CW cut (the keyer stopped mid-word on its next
    /// tick), leave no word to key and P1 owning nothing, and refuse P1's next CW until the
    /// operator's own act. CONTROL: with no stop, nothing is cut and the rest of the macro keys.
    #[test]
    fn stop_in_nexus_ends_a_programs_cw_mid_word_and_holds() {
        type Stop = fn(&mut Engine);
        let stops: [(&str, Stop, Refusal); 5] = [
            ("Stop TX", |e| e.halt_tx(), Refusal::StopHold),
            (
                "Esc",
                |e| {
                    e.stop_cw();
                    e.halt_tx();
                },
                Refusal::StopHold,
            ),
            (
                "TX Off",
                |e| e.set_tx_enabled_by_operator(false),
                Refusal::StopHold,
            ),
            (
                "the switch off",
                |e| save(e, |s| s.tci_server_tx = false),
                Refusal::Switch,
            ),
            (
                "the CW tier off",
                |e| save(e, |s| s.tci_server_cw = false),
                Refusal::Tier,
            ),
        ];
        let mut control = cw();
        assert_eq!(control.tci_cw(P1, &mac("CQ CQ DE KD9TAW K")), Ok(()));
        assert_eq!(keying(&mut control), "CQ");
        assert!(!control.take_cw_abort(), "control: a cut with no stop");
        assert_eq!(
            keys(&mut control),
            words(&[("CQ", 20), ("DE", 20), ("KD9TAW", 20), ("K", 20)]),
            "control: the rest keys"
        );
        for (name, stop, refusal) in stops {
            let mut e = cw();
            assert_eq!(e.tci_cw(P1, &mac("CQ CQ DE KD9TAW K")), Ok(()), "{name}");
            assert_eq!(keying(&mut e), "CQ", "{name}: premise");
            assert_eq!(
                e.tci_owner(),
                Some(P1),
                "{name}: premise: P1's CW is on the air"
            );
            stop(&mut e);
            assert!(
                e.take_cw_abort(),
                "{name}: no cut, the word on the air keys on"
            );
            assert_eq!(keys(&mut e), Vec::new(), "{name}: words still to go");
            assert_eq!(
                (e.tci_owner(), e.tx_owner()),
                (None, None),
                "{name}: still owned"
            );
            assert_eq!(e.tci_cw(P1, &mac("CQ")), Err(refusal), "{name}: P1 again");
        }
    }

    /// ⛔ `cw_macros_stop` FROM ANY PROGRAM ENDS CW MID-WORD, FROM ANY ORIGIN: P1's
    /// macro on the air is stopped by P2's, and the operator's own message by P1's: the cut armed,
    /// nothing left to key, nothing owned. It is not the operator's Stop, so P1 may send again.
    /// CONTROL: with no CW on the air it cuts nothing, the slot over's cut included.
    #[test]
    fn cw_macros_stop_ends_cw_mid_word_from_any_program() {
        let mut e = cw();
        assert_eq!(e.tci_cw(P1, &mac("CQ CQ DE KD9TAW K")), Ok(()));
        assert_eq!(keying(&mut e), "CQ");
        e.tci_cw_stop(P2);
        assert!(e.take_cw_abort(), "P1's word keys on");
        assert_eq!(keys(&mut e), Vec::new(), "P1's words still to go");
        assert_eq!((e.tci_owner(), e.tx_owner()), (None, None));
        assert_eq!(
            e.tci_cw(P1, &mac("CQ")),
            Ok(()),
            "not a hold: P1 sends again"
        );

        let mut e = cw();
        e.send_cw("CQ CQ DE KD9TAW K");
        assert_eq!(keying(&mut e), "CQ");
        e.tci_cw_stop(P1);
        assert!(e.take_cw_abort(), "the operator's word keys on");
        assert_eq!(keys(&mut e), Vec::new(), "the operator's words still to go");

        let mut e = cw();
        e.tci_cw_stop(P1);
        assert!(
            !e.take_cw_abort() && !e.take_slot_tx_abort(),
            "control: a stop with no CW on the air cut something"
        );
    }

    /// ⛔ A PROGRAM'S LOST CONNECTION AND ITS `stop;` END ITS CW, AND NO ONE ELSE'S:
    /// mid-word, the cut armed and nothing left; queued and not yet started, dropped
    /// with nothing to cut; its last word handed to the keyer and not yet reported keying by the
    /// radio loop (the arbiter cannot see it yet), cut too, with no slot cut. CONTROLS: another
    /// program's loss or `stop;` ends nothing of P1's.
    #[test]
    fn a_programs_lost_connection_and_its_stop_end_its_cw_and_no_one_elses() {
        type End = fn(&mut Engine, u64);
        let ends: [(&str, End); 2] = [
            ("its lost connection", |e, p| e.tci_disconnected(p)),
            ("its stop;", |e, p| e.tci_end_own(p)),
        ];
        for (name, end) in ends {
            let mut e = cw();
            assert_eq!(e.tci_cw(P1, &mac("CQ CQ DE KD9TAW K")), Ok(()));
            assert_eq!(keying(&mut e), "CQ");
            end(&mut e, P2);
            assert!(!e.take_cw_abort(), "{name}: control: P2's cut P1's word");
            assert_eq!(
                e.tci_owner(),
                Some(P1),
                "{name}: control: P2's ended P1's CW"
            );
            end(&mut e, P1);
            assert!(e.take_cw_abort(), "{name}: P1's word keys on");
            assert_eq!(keys(&mut e), Vec::new(), "{name}: P1's words still to go");
            assert_eq!((e.tci_owner(), e.tx_owner()), (None, None), "{name}");

            let mut e = cw();
            assert_eq!(e.tci_cw(P1, &mac("CQ TEST")), Ok(()));
            end(&mut e, P1);
            assert_eq!(keys(&mut e), Vec::new(), "{name}: queued words still to go");
            assert!(!e.take_cw_abort(), "{name}: a cut with nothing on the air");

            let mut e = cw();
            assert_eq!(e.tci_cw(P1, &mac("TEST")), Ok(()));
            assert_eq!(e.poll_cw_one().as_deref(), Some("TEST"));
            assert_eq!(
                e.tx_owner(),
                None,
                "{name}: premise: not reported keying yet"
            );
            end(&mut e, P1);
            assert!(
                e.take_cw_abort(),
                "{name}: the word just handed over keys on"
            );
            assert!(
                !e.take_slot_tx_abort(),
                "{name}: the slot cut armed with the transmitter not CW's"
            );
        }
    }

    // ── One queue ────────────────────────────────────────────────────────────────────────────

    /// ONE CW QUEUE, EACH MESSAGE ITS SENDER'S: P1 adds a macro in line behind its own (TCI's
    /// queue), P2 is refused; the operator's F-key fired during P1's macro queues behind it as
    /// behind any CW (F-keys unchanged), and P1 may then add nothing behind that; P1's loss then
    /// cuts P1's word and drops P1's words, and the operator's message keys next at the
    /// operator's speed. A fresh admission moves the gate generation; one in line does not.
    #[test]
    fn a_programs_cw_queues_only_behind_its_own_and_its_end_spares_the_operators() {
        let mut e = cw();
        let gen = e.tx_gate_gen;
        assert_eq!(e.tci_cw(P1, &mac("CQ CQ")), Ok(()));
        assert_ne!(
            e.tx_gate_gen, gen,
            "a fresh owner left planned overs keyable"
        );
        assert_eq!(keying(&mut e), "CQ");
        let gen = e.tx_gate_gen;
        assert_eq!(e.tci_cw(P1, &mac("DE KD9TAW")), Ok(()), "in line");
        assert_eq!(e.tx_gate_gen, gen, "in line is no new owner");
        assert_eq!(e.tci_cw(P2, &mac("QRL")), Err(Refusal::Owner));
        e.send_cw("TU");
        assert_eq!(
            e.tci_cw(P1, &mac("K")),
            Err(Refusal::Owner),
            "P1 behind the operator's message"
        );
        assert_eq!(e.tci_owner(), Some(P1));
        e.tci_disconnected(P1);
        assert!(e.take_cw_abort(), "P1's word keys on");
        assert_eq!(
            keys(&mut e),
            words(&[("TU", 20)]),
            "what keys after P1's loss"
        );
    }

    /// A clock repair refuses a program's CW, even in line behind its own: every CW word is dropped
    /// while one lasts (`poll_cw_word`), so none is queued under it. CONTROL: in line before it.
    #[test]
    fn a_clock_repair_refuses_a_programs_cw_even_in_line() {
        let mut e = cw();
        assert_eq!(e.tci_cw(P1, &mac("CQ CQ")), Ok(()));
        assert_eq!(keying(&mut e), "CQ");
        assert_eq!(e.tci_cw(P1, &mac("DE")), Ok(()), "control: in line");
        e.hold_tx_for_clock_repair(Instant::now() + Duration::from_secs(600));
        let queued = e.cw_queue.len();
        assert_eq!(e.tci_cw(P1, &mac("KD9TAW")), Err(Refusal::ClockRepair));
        assert_eq!(e.cw_queue.len(), queued, "queued under the repair");
    }

    /// A program cannot pile up CW: four whole commands may wait, the fifth is refused and queues
    /// nothing. CONTROL: once the loop has keyed some, there is room again.
    #[test]
    fn a_program_may_queue_four_commands_of_cw_at_once() {
        let full = vec!["E"; 128].join(" ");
        assert_eq!(full.len(), 255, "premise: one command's worth");
        let mut e = cw();
        for n in 1..=4 {
            assert_eq!(e.tci_cw(P1, &mac(&full)), Ok(()), "command {n}");
        }
        assert_eq!(e.tci_cw(P1, &mac(&full)), Err(Refusal::CwText), "the fifth");
        assert_eq!(e.cw_queue.len(), 4 * 128, "the fifth queued something");
        for _ in 0..128 {
            keying(&mut e);
        }
        assert_eq!(e.tci_cw(P1, &mac(&full)), Ok(()), "control: room again");
    }
}
