//! The diagnostic log's PTT lines: each key and unkey of the transmitter, who keyed it and how, the
//! radio keyed by something that is not Nexus, and the PTT method in use. LOG ONLY: nothing here
//! keys, unkeys, refuses or delays anything. The loop calls in with what it already decided, and
//! the only thing that can come out is a line in the file.
//!
//! An operator's log could not answer the question it was sent to answer: did anything outside
//! Nexus key the radio? It held every arm, halt and over, and no PTT edge, no PTT method and no word
//! of the radio's own PTT read keyed while Nexus held nothing. These are those lines:
//!
//! - `PTT KEY: <who>, via <method>` and `PTT unkey: <who>, after <s> s`, from the loop's own key
//!   ([`Rig::keyed`](crate::rig::Rig) or a keying flag of the loop). Who is the engine's arbiter at
//!   the key edge ([`Engine::ptt_keyer`]), else the loop's flag that keyed.
//! - `radio keyed by something else …` and its end, from the radio's own PTT read back over CAT
//!   while Nexus holds nothing (`RadioLoop::rig_keyed`): another program on a shared rigctld or CAT
//!   port, VOX, or the radio's own PTT or front panel.
//! - `PTT method: …` once when the loop first runs, and again on each change: how Nexus keys this
//!   radio, the CAT share and its "Other programs may key transmit" switch.
//!
//! ⚠️ EDGES ONLY, AND BOUNDED. A line is written on a change and never on a tick. Key lines are held
//! to [`KEY_LINES_PER_MIN`] a minute (the count of the ones not written rides on the next one), and
//! an unkey is written exactly when its key was, so the file never shows a key with no end. A method
//! change waits [`METHOD_MIN_GAP_MS`] after the last method line, then the state it settled on is
//! written: a flapping reconnect is at most one line per gap, and the last line is the truth.

use super::{flex_client_ip, native_civ_addr, RadioLoop, Transport};
use crate::rig::{PttMode, Rig, SerialLine};
use std::sync::{Arc, Mutex};
use tempo_app::engine::{engine_lock, Engine};

/// Key lines (each with its unkey) at most, per minute of the loop's clock.
const KEY_LINES_PER_MIN: u32 = 20;
/// The shortest gap between two method lines (loop ms).
const METHOD_MIN_GAP_MS: f64 = 5_000.0;

/// How long after Nexus's own unkey a radio still reporting transmit is that over's tail (VOX or
/// break-in hang, an ATU cycle, a slow changeover), not a key from something else (loop ms). The
/// loop's own CW hang is 1 s (`CW_HANG_MS`); this allows the poll's lag on top.
const TAIL_GRACE_MS: f64 = 1_500.0;

/// The CAT share's port while it runs, and its "Other programs may key transmit" switch.
type Share = (Option<u16>, bool);

/// A key the log has seen and not yet seen end.
#[derive(Debug)]
struct Held {
    since_ms: f64,
    who: String,
    /// A radio reading that began inside [`TAIL_GRACE_MS`] of Nexus's unkey: the tail of that over.
    tail: bool,
    /// Its key line was written, so its unkey line is.
    logged: bool,
}

/// What the PTT lines remember between ticks.
#[derive(Debug, Default)]
pub(super) struct PttDiag {
    nexus: Option<Held>,
    foreign: Option<Held>,
    /// When Nexus's own key last ended (loop ms), for [`TAIL_GRACE_MS`].
    last_unkey_ms: Option<f64>,
    /// The CAT share's port while it runs, and "Other programs may key transmit", mirrored from the
    /// engine on the tick's one unconditional engine lock.
    pub(super) share: Share,
    /// The last method line written, by what it was made from.
    method: Option<(PttMode, &'static str, Share)>,
    method_at_ms: f64,
    window_start_ms: f64,
    in_window: u32,
    not_written: u32,
    /// Every line written, for the tests (the diagnostic log is inert in a test).
    #[cfg(test)]
    pub(super) lines: Vec<String>,
}

impl PttDiag {
    /// May a key line be written now? Counts the ones that may not.
    fn admit(&mut self, now_ms: f64) -> bool {
        if now_ms - self.window_start_ms >= 60_000.0 || now_ms < self.window_start_ms {
            self.window_start_ms = now_ms;
            self.in_window = 0;
        }
        if self.in_window < KEY_LINES_PER_MIN {
            self.in_window += 1;
            true
        } else {
            self.not_written += 1;
            false
        }
    }

    /// The count of key lines not written, said once on the next one that is.
    fn not_written_note(&mut self) -> String {
        match std::mem::take(&mut self.not_written) {
            0 => String::new(),
            n => format!(" ({n} more key edges in the last minute were not logged)"),
        }
    }

    fn emit(&mut self, line: String) -> Option<String> {
        #[cfg(test)]
        self.lines.push(line.clone());
        Some(line)
    }

    /// Whether the log holds Nexus's transmitter keyed.
    pub(super) fn nexus_keyed(&self) -> bool {
        self.nexus.is_some()
    }

    /// Nexus's own key, this tick: the line for its edge, if there is one. `who` is asked at the
    /// key edge only.
    pub(super) fn nexus(
        &mut self,
        keyed: bool,
        now_ms: f64,
        who: impl FnOnce() -> String,
        via: &str,
    ) -> Option<String> {
        match (keyed, self.nexus.take()) {
            (true, None) => {
                let who = who();
                let logged = self.admit(now_ms);
                let note = if logged {
                    self.not_written_note()
                } else {
                    String::new()
                };
                let line = format!("PTT KEY: {who}, via {via}{note}");
                self.nexus = Some(Held {
                    since_ms: now_ms,
                    who,
                    tail: false,
                    logged,
                });
                logged.then(|| self.emit(line)).flatten()
            }
            (false, Some(held)) => {
                self.last_unkey_ms = Some(now_ms);
                held.logged
                    .then(|| {
                        self.emit(format!(
                            "PTT unkey: {}, after {:.1} s",
                            held.who,
                            (now_ms - held.since_ms).max(0.0) / 1000.0
                        ))
                    })
                    .flatten()
            }
            (_, held) => {
                self.nexus = held;
                None
            }
        }
    }

    /// The radio's own PTT read keyed (or not) while Nexus holds nothing: the line for its edge.
    /// `ended` says why a keyed reading ended, for its end line.
    pub(super) fn foreign(&mut self, keyed: bool, now_ms: f64, ended: &str) -> Option<String> {
        match (keyed, self.foreign.take()) {
            (true, None) => {
                let logged = self.admit(now_ms);
                let note = if logged {
                    self.not_written_note()
                } else {
                    String::new()
                };
                // ⚠️ A radio still reporting transmit just after Nexus's own release is that over's
                // tail, not a key from something else: said as such, with how long after.
                let after_unkey = self
                    .last_unkey_ms
                    .map(|at| (now_ms - at).max(0.0))
                    .filter(|ms| *ms < TAIL_GRACE_MS);
                self.foreign = Some(Held {
                    since_ms: now_ms,
                    who: String::new(),
                    tail: after_unkey.is_some(),
                    logged,
                });
                let line = match after_unkey {
                    Some(ms) => format!(
                        "radio still reports transmit {:.1} s after Nexus let go: the over's \
                         tail (VOX or break-in hang, an ATU cycle or a slow changeover){note}",
                        ms / 1000.0
                    ),
                    None => format!(
                        "radio keyed by something else, with no Nexus over: another program on \
                         a shared rigctld or CAT port, VOX, or the radio's own PTT or front \
                         panel{note}"
                    ),
                };
                logged.then(|| self.emit(line)).flatten()
            }
            (false, Some(held)) => held
                .logged
                .then(|| {
                    let secs = (now_ms - held.since_ms).max(0.0) / 1000.0;
                    self.emit(if held.tail {
                        format!("radio no longer reports transmit, after {secs:.1} s ({ended})")
                    } else {
                        format!(
                            "radio no longer keyed by something else, after {secs:.1} s ({ended})"
                        )
                    })
                })
                .flatten(),
            (_, held) => {
                self.foreign = held;
                None
            }
        }
    }

    /// The PTT method, this tick: its line when it changed (or on the first tick), once the gap
    /// since the last one has passed.
    pub(super) fn method(&mut self, t: &Transport, mode: &PttMode, now_ms: f64) -> Option<String> {
        let kind = cat_words(t);
        if self
            .method
            .as_ref()
            .is_some_and(|(m, k, s)| m == mode && *k == kind && *s == self.share)
        {
            return None;
        }
        if self.method.is_some() && now_ms - self.method_at_ms < METHOD_MIN_GAP_MS {
            return None; // written again once the gap has passed, as it is then
        }
        self.method = Some((mode.clone(), kind, self.share));
        self.method_at_ms = now_ms;
        let share = match self.share {
            (Some(port), foreign) => format!(
                "CAT share on port {port}, Other programs may key transmit {}",
                if foreign { "on" } else { "off" }
            ),
            (None, _) => "CAT share off".to_string(),
        };
        let line = format!(
            "PTT method: {}; CAT through {kind}; {share}",
            method_words(t, mode)
        );
        self.emit(line)
    }
}

/// How the transmitter is keyed, in short, for a key line. `plays`: Nexus plays this key's
/// transmit audio (an over, a tune carrier, a microphone over). The operator's PTT, a CAT-share
/// program's key and a CW word on the radio's keyer play none: they never key the DATA input
/// (`Rig::ptt_plain`), and under VOX nothing of Nexus's puts the radio on the air.
fn key_words(mode: &PttMode, plays: bool) -> String {
    match mode {
        PttMode::Cat => "CAT".to_string(),
        PttMode::CatData if plays => "CAT (DATA input)".to_string(),
        PttMode::CatData => "CAT".to_string(),
        PttMode::Serial { port, line } => format!("{} on {port}", line_name(*line)),
        PttMode::Vox if plays => {
            "VOX (Nexus plays the audio; the radio keys itself on it)".to_string()
        }
        PttMode::Vox => "VOX (Nexus sends no key and plays no audio for this; only the radio's \
                         own VOX, key or keyer puts it on the air)"
            .to_string(),
    }
}

fn line_name(line: SerialLine) -> &'static str {
    match line {
        SerialLine::Rts => "RTS",
        SerialLine::Dtr => "DTR",
    }
}

/// How the transmitter is keyed, for the method line.
fn method_words(t: &Transport, mode: &PttMode) -> String {
    match mode {
        // RTS/DTR on the CAT port: rigctld holds the port and asserts the line for Nexus.
        PttMode::Cat if matches!(t.ptt_method.as_str(), "rts" | "dtr") && !t.is_tci() => {
            format!(
                "{} on the CAT port, asserted by the CAT daemon",
                t.ptt_method.to_uppercase()
            )
        }
        PttMode::Cat => "CAT (T 1)".to_string(),
        PttMode::CatData => "CAT on the DATA input (T 3)".to_string(),
        PttMode::Serial { port, line } => format!("{} on {port}", line_name(*line)),
        PttMode::Vox => "VOX (Nexus never keys this radio; it keys itself on audio)".to_string(),
    }
}

/// What carries CAT (and a CAT key) to this radio.
fn cat_words(t: &Transport) -> &'static str {
    if t.is_tci() {
        "the TCI client (the SDR program keys)"
    } else if flex_client_ip(t).is_some() {
        "Nexus's own Flex client"
    } else if t.is_icom_lan() {
        "the Icom network client"
    } else if native_civ_addr(t).is_some() {
        "Nexus's own CI-V daemon"
    } else if t.is_omnirig() {
        "OmniRig"
    } else if tempo_app::settings::net_rigctl_addr(t.rig_model, &t.rig_conn, &t.rig_addr).is_some()
    {
        "a rigctld Nexus did not start (NET rigctl): other programs on it can key the radio"
    } else if t.cat_available() {
        "rigctld"
    } else {
        "nothing (no CAT)"
    }
}

impl RadioLoop {
    /// This tick's PTT lines: the method on a change, and Nexus's own key edge. `keyed_now` is the
    /// loop's keying flags as the tick computed them. Reads only.
    pub(super) fn log_ptt_edges(
        &mut self,
        engine: &Arc<Mutex<Engine>>,
        rig: &Rig,
        keyed_now: bool,
        now: f64,
    ) {
        if let Some(line) = self.ptt_diag.method(&self.applied, rig.ptt_mode(), now) {
            tempo_core::applog::info("ptt", &line);
        }
        let keyed = rig.keyed || keyed_now;
        if keyed == self.ptt_diag.nexus_keyed() {
            return; // no edge: the common tick, and no lock
        }
        let fallback = if self.tuning_keyed {
            "Tune"
        } else if self.manual_ptt_applied {
            "manual PTT"
        } else if now < self.cw_keying_until {
            "the CW keyer"
        } else if self.tx_until_ms.is_some() {
            "an over Nexus is playing"
        } else {
            "a key Nexus sent"
        };
        let who = || {
            engine_lock(engine)
                .ptt_keyer()
                .unwrap_or_else(|| fallback.to_string())
        };
        // Field reads only: what of this key's audio Nexus plays, for its "via".
        let plays =
            self.tx_until_ms.is_some() || self.tuning_keyed || self.mic_keyed || self.pc_mic_keyed;
        if let Some(line) = self
            .ptt_diag
            .nexus(keyed, now, who, &key_words(rig.ptt_mode(), plays))
        {
            tempo_core::applog::info("ptt", &line);
        }
    }

    /// The radio's own PTT belief changed (`rig_keyed`): its line, if one is due.
    pub(super) fn log_foreign_ptt(&mut self, keyed: bool, now: f64, ended: &str) {
        if let Some(line) = self.ptt_diag.foreign(keyed, now, ended) {
            tempo_core::applog::info("ptt", &line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_and_its_unkey_are_one_line_each_and_a_held_key_is_none() {
        let mut d = PttDiag::default();
        let key = d.nexus(true, 1_000.0, || "FT8 over".into(), "CAT");
        assert_eq!(key.as_deref(), Some("PTT KEY: FT8 over, via CAT"));
        for t in 1..50 {
            let who = || panic!("who is asked at the key edge only");
            assert_eq!(d.nexus(true, 1_000.0 + t as f64 * 20.0, who, "CAT"), None);
        }
        let unkey = d.nexus(false, 13_600.0, || unreachable!(), "CAT");
        assert_eq!(unkey.as_deref(), Some("PTT unkey: FT8 over, after 12.6 s"));
        assert_eq!(d.nexus(false, 13_620.0, || unreachable!(), "CAT"), None);
    }

    #[test]
    fn the_radio_keyed_by_something_else_is_one_line_and_its_end_one_more() {
        let mut d = PttDiag::default();
        let on = d.foreign(true, 0.0, "").unwrap();
        assert!(on.contains("radio keyed by something else"), "{on}");
        assert!(on.contains("another program"), "{on}");
        assert_eq!(d.foreign(true, 1_000.0, ""), None);
        let off = d
            .foreign(false, 15_000.0, "the radio reports its PTT off")
            .unwrap();
        assert!(off.contains("after 15.0 s"), "{off}");
        assert!(off.contains("reports its PTT off"), "{off}");
        assert_eq!(d.foreign(false, 16_000.0, ""), None);
    }

    /// ⚠️ BOUNDED: a flood of edges is held to the minute's budget, every key that was written has
    /// its unkey, and the count of the rest rides on the next line.
    #[test]
    fn key_lines_are_held_to_a_budget_and_never_lose_an_unkey() {
        let mut d = PttDiag::default();
        let mut written = 0;
        for i in 0..(KEY_LINES_PER_MIN + 5) {
            let t = i as f64 * 100.0;
            written += d.nexus(true, t, || "Tune".into(), "CAT").is_some() as u32;
            written += d.nexus(false, t + 50.0, || "Tune".into(), "CAT").is_some() as u32;
        }
        assert_eq!(
            written,
            KEY_LINES_PER_MIN * 2,
            "each written key has its unkey"
        );
        let next = d.nexus(true, 61_000.0, || "Tune".into(), "CAT").unwrap();
        assert!(next.contains("5 more key edges"), "{next}");
    }

    #[test]
    fn the_method_is_said_once_and_again_on_a_change() {
        let mut d = PttDiag::default();
        let t = Transport::from_cfg(&super::super::RadioConfig::default());
        let first = d.method(&t, &PttMode::Cat, 0.0).unwrap();
        assert!(first.starts_with("PTT method: CAT (T 1)"), "{first}");
        assert!(first.contains("CAT share off"), "{first}");
        assert_eq!(
            d.method(&t, &PttMode::Cat, 20.0),
            None,
            "no change, no line"
        );
        // The share comes up with foreign PTT on: said, once the gap has passed.
        d.share = (Some(4532), true);
        assert_eq!(d.method(&t, &PttMode::Cat, 40.0), None, "inside the gap");
        let share = d.method(&t, &PttMode::Cat, 6_000.0).unwrap();
        assert!(
            share.contains("CAT share on port 4532, Other programs may key transmit on"),
            "{share}"
        );
        let rts = PttMode::Serial {
            port: "COM4".into(),
            line: SerialLine::Rts,
        };
        let line = d.method(&t, &rts, 12_000.0).unwrap();
        assert!(line.starts_with("PTT method: RTS on COM4"), "{line}");
        let vox = d.method(&t, &PttMode::Vox, 18_000.0).unwrap();
        assert!(vox.contains("VOX"), "{vox}");
    }

    /// Inside the grace after Nexus's unkey the radio's reading is that over's tail; after it, it is
    /// something else.
    #[test]
    fn a_reading_just_after_the_unkey_is_the_tail_and_later_one_is_something_else() {
        let mut d = PttDiag::default();
        d.nexus(true, 0.0, || "FT8 over".into(), "CAT");
        d.nexus(false, 12_600.0, || unreachable!(), "CAT");
        let tail = d.foreign(true, 13_200.0, "").unwrap();
        assert!(
            tail.starts_with("radio still reports transmit 0.6 s after Nexus let go"),
            "{tail}"
        );
        let end = d
            .foreign(false, 13_400.0, "the radio reports its PTT off")
            .unwrap();
        assert!(
            end.starts_with("radio no longer reports transmit, after 0.2 s"),
            "{end}"
        );
        let later = d.foreign(true, 12_600.0 + TAIL_GRACE_MS + 1.0, "").unwrap();
        assert!(
            later.starts_with("radio keyed by something else"),
            "{later}"
        );
    }

    /// The key line's "via" follows what keyed: no DATA input and no "Nexus plays the audio" for a
    /// key whose audio Nexus does not play.
    #[test]
    fn the_via_follows_whether_nexus_plays_the_audio() {
        assert!(key_words(&PttMode::Vox, true).contains("Nexus plays the audio"));
        let ptt = key_words(&PttMode::Vox, false);
        assert!(!ptt.contains("Nexus plays the audio"), "{ptt}");
        assert!(ptt.contains("plays no audio"), "{ptt}");
        assert_eq!(key_words(&PttMode::CatData, true), "CAT (DATA input)");
        assert_eq!(key_words(&PttMode::CatData, false), "CAT");
    }
}
