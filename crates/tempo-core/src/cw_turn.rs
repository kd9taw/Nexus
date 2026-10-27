//! WHOSE TURN IT IS in a CW contact, and what fits when it is yours: the model behind the CW
//! cockpit's turn cue.
//!
//! An operator who copies CW through the decoder cannot hear an over end, so this reads it from
//! what the AI decoder copied, the moment it heard each letter, and when Nexus itself was keying.
//! The rules, each pinned by a test below:
//!
//! - **"Your turn" needs silence AND an end marker.** The silence is the decoder's own: from the
//!   other station's last letter to the end of the audio the decoder has listened to, so a pass
//!   that has not caught up yet never counts as quiet. The marker is how the over ENDS: K, KN or
//!   BK, a query (QRZ, AGN, part of a call with "?"), a CQ closing on the caller's call, UP, and
//!   in a contest the runner's TU. Silence with no marker is only "not sure", however long.
//! - **An over starts after ours.** Our own transmissions cut the copy into overs, so a 73 heard
//!   before we last sent can never stand for the next station's CQ (the copilot's 60-word window
//!   had no such boundary, which is why its next-key guess could not be shown as it was). A long
//!   silence cuts it too, and so does an end marker followed by silence.
//! - **Our own sidetone is not theirs.** Letters heard while we were keying, and for a short hang
//!   after, are dropped: a radio that feeds its sidetone back into the receive audio would
//!   otherwise hand us our own KN.
//! - **Keying heard again withdraws the turn.** The classic decoder hears keying at the operator's
//!   pitch in real time, ahead of the AI's copy; a mark there more recent than the silence needs
//!   is the other station still sending.
//! - **Who the over was for.** One addressed to another station is theirs to answer (Hold); a
//!   query is matched against our call, a near miss as "probably you" ([`match_call`]).
//!
//! ⛔ IT NEVER TRANSMITS. It names a state and at most the KIND of message that fits ([`Role`]);
//! the cockpit lights the key that sends it, and only the operator presses one. Nothing is held
//! back either: a key pressed while the lamp says "sending" still sends.
//!
//! The thresholds scale with the other station's speed ([`Thresholds`]) and are a first guess, to
//! be tuned against recorded contacts. Pure and deterministic: the engine keeps the letters and
//! the transmit times, the caller passes the clocks, and a test drives any timeline by value.

use crate::cw_parse::{decut_rst, is_strict_callsign};
use crate::message::{base_call, same_call};
use serde::Serialize;

/// One letter of the copy and the stream time the decoder placed it at (seconds of received
/// audio). A word space is `' '`.
pub type Letter = (f64, char);

/// One transmission of ours on the same clock: when it started, and when it ended (`None` while
/// we are still keying it).
pub type Keyed = (f64, Option<f64>);

/// What the model reads.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// The copy, oldest first: every letter the decoder has given, its unconfirmed tail included.
    pub letters: &'a [Letter],
    /// Our transmissions, oldest first.
    pub keyed: &'a [Keyed],
    /// The end of the newest audio the decoder has listened to.
    pub heard_until: f64,
    /// The stream clock now (it runs ahead of `heard_until` by the decoder's lag).
    pub now: f64,
    /// Seconds since the classic decoder last heard a keyed element at the operator's pitch.
    pub mark_age: Option<f64>,
    /// The other station's speed, when it was measured recently.
    pub wpm: Option<u32>,
    /// Our call.
    pub mycall: &'a str,
    /// The call the F-keys' `!` sends: the station worked, else the decoder's best guess.
    pub his_call: Option<&'a str>,
    /// What we last sent, expanded, if anything.
    pub last_sent: Option<&'a str>,
    /// A contest is running: the F-keys are a contest set.
    pub contest: bool,
    /// Split is on.
    pub split: bool,
}

/// Where the contact stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Nothing heard, or nothing recent.
    Idle,
    /// The other station is sending.
    TheySend,
    /// Their over has just ended in a marker that hands us the turn, and the silence is not long
    /// enough yet: the pre-cue.
    Finishing,
    /// Ours to send.
    YourTurn,
    /// Not ours: the over was for another station, or the DX listens up and split is off.
    Hold,
    /// The silence is long, and nothing said whose turn it is.
    Unsure,
    /// We are keying.
    YouSend,
    /// Our over has ended and we listen for a reply.
    Listen,
    /// They are signing: 73, SK.
    Closing,
}

/// The kind of message that fits. The cockpit finds the key that sends it in the set in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// Call CQ.
    Cq,
    /// Answer a CQ with their call and ours.
    Answer,
    /// Reply to a station that came back to us: the report (the exchange, in a contest).
    Reply,
    /// Our call, once.
    MyCall,
    /// Our exchange, in a contest's search and pounce.
    Exchange,
    /// The runner's TU, once the caller's exchange is in.
    Tu,
    /// Our 73.
    Close,
    /// Ask for it again.
    Again,
    /// Send our last message again: they asked for it.
    Repeat,
}

/// What the lamp says beside the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Hint {
    /// Ours, with no key that fits: a ragchew over, typed.
    Type,
    /// A DX came back to us in a pileup: the everyday reply key sends a whole ragchew, so no key
    /// is suggested and the lamp says to send 5NN TU.
    FiveNnTu,
    /// The call was only part of ours, or near it: probably us.
    Partial,
    /// The DX listens up and split is off.
    SplitOff,
    /// Nobody answered our over within its window.
    NoReply,
    /// The window to answer a race (a runner's TU, a pileup's partial) has run out.
    Late,
}

/// A window the lamp counts down, in seconds, to the hundredth.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub total: f64,
    pub left: f64,
}

/// The model's read.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub state: State,
    pub role: Option<Role>,
    pub hint: Option<Hint>,
    /// The station the lamp names: the one sending, or the one the over was for.
    pub call: Option<String>,
    /// Which over this read is about: its first letter's stream time, in milliseconds. The
    /// cockpit announces a turn at most once per over.
    pub over: Option<u64>,
    /// What the lamp counts down: our listening after a CQ or a call, or a race.
    pub window: Option<Window>,
}

impl Turn {
    fn idle() -> Self {
        Turn {
            state: State::Idle,
            role: None,
            hint: None,
            call: None,
            over: None,
            window: None,
        }
    }
}

/// A speed nobody has measured is taken as this.
const DEFAULT_WPM: u32 = 20;
/// After this much silence a hold, or our own listening after an over, is over.
pub const IDLE_SECS: f64 = 8.0;
/// After this much silence nothing is anyone's turn any more.
pub const STALE_SECS: f64 = 30.0;
/// Letters the decoder places this long after our key-up are still our own sidetone.
pub const ECHO_HANG_SECS: f64 = 0.3;
/// How long a race stays worth taking once it is ours: a contest runner's listening after his
/// TU, and a pileup's (a DX op answers the first caller he copies).
const RACE_CONTEST_SECS: f64 = 2.0;
const RACE_PILEUP_SECS: f64 = 1.5;
/// How long we listen for a reply after our own over, by what it was.
const LISTEN_CQ_SECS: f64 = 8.0;
const LISTEN_CALL_SECS: f64 = 5.0;
const LISTEN_CONTEST_SECS: f64 = 2.5;
/// Words that ask a question with "?" and are not a call: a targeted repeat ("NAME?").
const QUERY_WORDS: [&str; 18] = [
    "HW", "NAME", "QTH", "RST", "CALL", "UR", "WX", "RIG", "ANT", "PWR", "AGE", "OP", "QSL", "R",
    "OK", "CPY", "COPY", "NR",
];

/// The silences that end an over, from the other station's speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// After an end marker, this much silence ends the over: one and a half word gaps.
    pub s1: f64,
    /// With no marker, this much silence is "not sure": five word gaps. It also starts a new
    /// over, and it is longer than any letter at the speed, so it never cuts a word.
    pub s2: f64,
}

impl Thresholds {
    /// At `wpm`, held to 10–40; an unmeasured speed is taken as 20.
    pub fn at(wpm: Option<u32>) -> Self {
        let wpm = wpm.filter(|w| *w > 0).unwrap_or(DEFAULT_WPM).clamp(10, 40);
        // A word gap is seven dits of 1.2/WPM seconds.
        let word_gap = 8.4 / f64::from(wpm);
        Thresholds {
            s1: (1.5 * word_gap).max(0.5),
            s2: (5.0 * word_gap).max(2.0),
        }
    }
}

/// How a call the other station sent matches ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallMatch {
    /// Our call (a portable affix aside).
    Exact,
    /// Part of our call, or one letter off it: probably us.
    Near,
    /// Someone else, or too little to tell.
    No,
}

/// Match a call or a query (`KD9?`) the other station sent against `mycall`, the way a DX
/// operator means one. A trailing "?" is the question, an inner one stands for any one letter.
/// Letters of our call the query left out cost nothing (a query is usually a piece of a call);
/// a letter our call does not have, or a wrong one, costs one edit, and one edit is still
/// "probably us". Under three letters fits too many calls to mean ours.
pub fn match_call(query: &str, mycall: &str) -> CallMatch {
    let q = query.trim().to_ascii_uppercase();
    let q = q.trim_end_matches('?');
    let mine = base_call(mycall);
    if mine.is_empty() || q.chars().filter(|c| *c != '?').count() < 3 {
        return CallMatch::No;
    }
    if !q.contains('?') && base_call(q) == mine {
        return CallMatch::Exact;
    }
    let call: Vec<char> = mine.chars().collect();
    // cost[j]: the query so far against the first j letters of our call. An empty query costs
    // nothing wherever it starts.
    let mut cost = vec![0u32; call.len() + 1];
    for qc in q.chars() {
        let mut next = vec![0u32; call.len() + 1];
        next[0] = cost[0] + 2;
        for j in 1..=call.len() {
            let fits = qc == '?' || qc == call[j - 1];
            next[j] = (cost[j - 1] + if fits { 0 } else { 2 })
                .min(next[j - 1])
                .min(cost[j] + 2);
        }
        cost = next;
    }
    if cost[call.len()] <= 2 {
        CallMatch::Near
    } else {
        CallMatch::No
    }
}

/// Read the contact.
pub fn read(inp: &Input) -> Turn {
    let th = Thresholds::at(inp.wpm);
    if matches!(inp.keyed.last(), Some((_, None))) {
        return Turn {
            state: State::YouSend,
            call: inp.his_call.map(str::to_string),
            ..Turn::idle()
        };
    }
    let overs = overs(inp, th);
    // The over read is the newest, unless we have keyed since it began.
    let last_keyed = inp.keyed.last().map(|k| k.0);
    let Some(over) = overs
        .last()
        .filter(|o| last_keyed.is_none_or(|k| o.first() > k))
    else {
        return listening(inp, th);
    };
    let silent = (inp.heard_until - over.last()).max(0.0);
    let keying = inp.mark_age.is_some_and(|a| a < th.s1);
    let verdict = classify(over, inp);
    let base = Turn {
        call: sender(over, inp).map(str::to_string),
        over: Some((over.first().max(0.0) * 1000.0).round() as u64),
        ..Turn::idle()
    };
    if silent < th.s1 || keying {
        return match verdict {
            Some(v) if v.state == State::YourTurn && !keying => Turn {
                state: State::Finishing,
                role: v.role,
                call: v.call,
                ..base
            },
            _ => Turn {
                state: State::TheySend,
                ..base
            },
        };
    }
    let Some(v) = verdict else {
        return if silent < th.s2 {
            // A pause inside the over.
            Turn {
                state: State::TheySend,
                ..base
            }
        } else if silent >= STALE_SECS {
            Turn::idle()
        } else {
            Turn {
                state: State::Unsure,
                role: Some(Role::Again),
                ..base
            }
        };
    };
    if silent >= STALE_SECS || (v.state == State::Hold && silent >= IDLE_SECS) {
        return Turn::idle();
    }
    let window = if v.race {
        let total = if inp.contest {
            RACE_CONTEST_SECS
        } else {
            RACE_PILEUP_SECS
        };
        let left = total - (silent - th.s1);
        if left <= 0.0 {
            return Turn {
                state: State::Listen,
                hint: Some(Hint::Late),
                call: v.call,
                ..base
            };
        }
        Some(window(total, left))
    } else {
        None
    };
    Turn {
        state: v.state,
        role: v.role,
        hint: v.hint,
        call: v.call,
        window,
        ..base
    }
}

fn window(total: f64, left: f64) -> Window {
    let hundredths = |x: f64| (x * 100.0).round() / 100.0;
    Window {
        total: hundredths(total),
        left: hundredths(left.max(0.0)),
    }
}

/// One word of the copy, with the times of its first and last letters.
#[derive(Debug)]
struct Word {
    text: String,
    first: f64,
    last: f64,
}

/// One over: its words, and whether we had transmitted before it began.
#[derive(Debug)]
struct Over {
    words: Vec<Word>,
    after_us: bool,
}

impl Over {
    fn first(&self) -> f64 {
        self.words.first().map_or(0.0, |w| w.first)
    }
    fn last(&self) -> f64 {
        self.words.last().map_or(0.0, |w| w.last)
    }
}

/// The copy cut into overs, our own transmissions' letters dropped first. A space ends a word.
/// A new over starts after one of our transmissions, after `s2` of silence, and after an end
/// marker followed by `s1`; the first two also end a word the decoder printed no space after.
fn overs(inp: &Input, th: Thresholds) -> Vec<Over> {
    let ours = |t: f64| {
        inp.keyed
            .iter()
            .any(|&(start, end)| t >= start && end.is_none_or(|e| t <= e + ECHO_HANG_SECS))
    };
    let keyed_between = |a: f64, b: f64| inp.keyed.iter().any(|&(s, _)| s > a && s < b);
    let mut words: Vec<Word> = Vec::new();
    let mut word: Option<Word> = None;
    let mut prev: Option<f64> = None;
    for &(t, c) in inp.letters {
        if ours(t) {
            continue;
        }
        if prev.is_some_and(|p| keyed_between(p, t) || t - p >= th.s2) {
            words.extend(word.take());
        }
        prev = Some(t);
        if c.is_whitespace() {
            words.extend(word.take());
            continue;
        }
        let c = c.to_ascii_uppercase();
        match &mut word {
            Some(w) => {
                w.text.push(c);
                w.last = t;
            }
            None => {
                word = Some(Word {
                    text: c.to_string(),
                    first: t,
                    last: t,
                })
            }
        }
    }
    words.extend(word);
    let mut overs: Vec<Over> = Vec::new();
    for w in words {
        if let Some(o) = overs.last_mut() {
            let gap = w.first - o.last();
            let ended = o
                .words
                .last()
                .is_some_and(|x| ends_over(&x.text, inp.contest));
            if !keyed_between(o.last(), w.first) && gap < th.s2 && !(ended && gap >= th.s1) {
                o.words.push(w);
                continue;
            }
        }
        let after_us = inp.keyed.iter().any(|&(s, _)| s < w.first);
        overs.push(Over {
            words: vec![w],
            after_us,
        });
    }
    overs
}

/// A word an over may end on: a marker followed by silence ends it.
fn ends_over(word: &str, contest: bool) -> bool {
    matches!(
        word,
        "K" | "KN" | "BK" | "(" | "QRZ" | "AGN" | "RPT" | "UP" | "B4" | "73" | "SK" | "VA" | "EE"
    ) || word.ends_with('?')
        || (contest && word == "TU")
}

/// The station sending this over: the call after its last DE, else, once we have transmitted
/// to them, the call the F-keys send.
fn sender<'a>(over: &'a Over, inp: &Input<'a>) -> Option<&'a str> {
    let w: Vec<&str> = over.words.iter().map(|x| x.text.as_str()).collect();
    w.iter()
        .rposition(|x| *x == "DE")
        .and_then(|i| {
            over.words[i + 1..]
                .iter()
                .map(|x| x.text.as_str())
                .find(|x| is_strict_callsign(x))
        })
        .or(if over.after_us { inp.his_call } else { None })
}

/// What we last sent, by its words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sent {
    /// A CQ (a contest runner's "TU <our call>" too: it asks for the next caller).
    Cq,
    /// Our call, or theirs and ours: an answer to a CQ.
    Call,
    /// A contest exchange: running when it starts with his call, search and pounce otherwise.
    Exchange {
        run: bool,
    },
    /// Our report: we are in the contact.
    Report,
    /// Our 73.
    Closing,
    Other,
}

fn sent_kind(text: Option<&str>, mycall: &str, contest: bool) -> Option<Sent> {
    let words: Vec<String> = text?
        .split_whitespace()
        .map(|w| w.to_ascii_uppercase())
        .collect();
    if words.is_empty() {
        return None;
    }
    let has = |x: &str| words.iter().any(|w| w == x);
    let ours = words.iter().any(|w| same_call(w, mycall));
    let report = words.iter().any(|w| decut_rst(w).is_some());
    Some(
        if has("CQ") || (contest && has("TU") && ours && words.len() <= 3) {
            Sent::Cq
        } else if has("73") || has("SK") || has("VA") {
            Sent::Closing
        } else if contest && report {
            Sent::Exchange {
                run: is_strict_callsign(&words[0]) && !same_call(&words[0], mycall),
            }
        } else if report || has("UR") || has("RST") {
            Sent::Report
        } else if ours && words.len() <= 6 {
            Sent::Call
        } else {
            Sent::Other
        },
    )
}

/// What a finished over says: whose turn, and what fits. `None` when nothing in it says.
struct Verdict {
    state: State,
    role: Option<Role>,
    hint: Option<Hint>,
    call: Option<String>,
    /// A race: the turn is worth taking only for a moment (a runner's TU, a pileup's query).
    race: bool,
}

fn verdict(
    state: State,
    role: Option<Role>,
    hint: Option<Hint>,
    call: Option<&str>,
    race: bool,
) -> Option<Verdict> {
    Some(Verdict {
        state,
        role,
        hint,
        call: call.map(str::to_string),
        race,
    })
}

fn classify(over: &Over, inp: &Input) -> Option<Verdict> {
    use State::{Closing, Hold, YourTurn};
    let w: Vec<&str> = over.words.iter().map(|x| x.text.as_str()).collect();
    let n = w.len();
    let last = *w.last()?;
    let has = |x: &str| w.contains(&x);
    let me = |x: &str| match_call(x, inp.mycall);
    let exact_me = w.iter().any(|x| me(x) == CallMatch::Exact);
    let de = w.iter().rposition(|x| *x == "DE");
    let from = sender(over, inp);
    let reported = w.iter().position(|x| decut_rst(x).is_some());
    // Who the over is for: the call nearest before its last DE, or, with no DE, a call that
    // leads a report ("W2XYZ 5NN").
    let to = match de {
        Some(i) => w[..i].iter().rev().copied().find(|x| is_strict_callsign(x)),
        None => w
            .first()
            .copied()
            .filter(|x| is_strict_callsign(x) && reported.is_some_and(|r| r > 0)),
    };
    let to_me = to.map(me);
    let kind = sent_kind(inp.last_sent, inp.mycall, inp.contest);

    // A CQ: open to anyone once it has ended, on its K or on the caller's call (a contest's CQ
    // carries no K; an everyday one without it repeats the call). One that fades out before
    // either has not ended. The key that answers sends His Call, so it fits only when His Call
    // is the caller.
    if (has("CQ") || (inp.contest && w[0] == "TEST")) && !exact_me {
        let caller = from.or_else(|| w.iter().rev().copied().find(|x| is_strict_callsign(x)));
        let twice = caller.is_some_and(|c| w.iter().filter(|x| **x == c).count() >= 2);
        let ended = matches!(last, "K" | "KN") || (caller == Some(last) && (inp.contest || twice));
        if !ended {
            return None;
        }
        let fits = caller
            .zip(inp.his_call)
            .is_some_and(|(c, h)| same_call(c, h));
        return verdict(
            YourTurn,
            fits.then_some(Role::Answer),
            None,
            caller,
            inp.contest,
        );
    }
    // An over for another station is theirs to answer, whatever it ends on.
    if let (Some(t), Some(CallMatch::No)) = (to, to_me) {
        return verdict(Hold, None, None, Some(t), false);
    }
    // Signing: ours to close only if the contact is ours.
    if w.iter()
        .rev()
        .take(4)
        .any(|x| matches!(*x, "73" | "SK" | "VA" | "EE" | "GB"))
    {
        return if exact_me || over.after_us || to_me == Some(CallMatch::Near) {
            verdict(
                Closing,
                (!inp.contest).then_some(Role::Close),
                None,
                from,
                false,
            )
        } else {
            verdict(Hold, None, None, to.or(from), false)
        };
    }
    // They ask for it again.
    let lone_query = n == 1 && last == "?";
    if matches!(last, "AGN" | "AGN?" | "RPT" | "RPT?") || lone_query {
        if !(exact_me || over.after_us) {
            return verdict(Hold, None, None, from, false);
        }
        // A lone "?" after our CQ or our call asks for the call.
        let role = if lone_query && matches!(kind, Some(Sent::Cq) | Some(Sent::Call)) {
            Role::MyCall
        } else {
            Role::Repeat
        };
        return verdict(YourTurn, Some(role), None, from, role == Role::MyCall);
    }
    // QRZ: anyone may call.
    if matches!(last, "QRZ" | "QRZ?") {
        return verdict(YourTurn, Some(Role::MyCall), None, from, true);
    }
    // A query: part of a call ("KD9?", "KD9 ?"), or a word ("NAME?").
    if last.ends_with('?') {
        let frag = if last == "?" {
            w[n - 2]
        } else {
            last.trim_end_matches('?')
        };
        if QUERY_WORDS.contains(&frag) {
            return if exact_me || over.after_us {
                verdict(YourTurn, None, Some(Hint::Type), from, false)
            } else {
                verdict(Hold, None, None, from, false)
            };
        }
        return match me(frag) {
            CallMatch::Exact => verdict(YourTurn, Some(Role::MyCall), None, from, true),
            CallMatch::Near => verdict(
                YourTurn,
                Some(Role::MyCall),
                Some(Hint::Partial),
                from,
                true,
            ),
            CallMatch::No => verdict(Hold, None, None, Some(frag), false),
        };
    }
    // UP: the DX listens above his own frequency. Calling there needs split.
    let digits = |x: &str| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit());
    if last == "UP"
        || last.strip_prefix("UP").is_some_and(digits)
        || (n >= 2 && w[n - 2] == "UP" && digits(last))
    {
        return if inp.split {
            verdict(YourTurn, Some(Role::MyCall), None, from, true)
        } else {
            verdict(Hold, None, Some(Hint::SplitOff), from, false)
        };
    }
    if inp.contest {
        // Searching and pouncing, our exchange sent: the runner's TU confirms it.
        if over.after_us && kind == Some(Sent::Exchange { run: false }) && has("TU") {
            return verdict(Closing, None, None, from, false);
        }
        // Running, our exchange sent: theirs is back, and the TU is ours.
        if over.after_us && kind == Some(Sent::Exchange { run: true }) {
            return verdict(YourTurn, Some(Role::Tu), None, from, false);
        }
        // The runner's TU, alone or with his call: he listens for the next caller.
        if last == "TU" {
            return verdict(YourTurn, Some(Role::MyCall), None, from, true);
        }
        if n >= 2 && w[n - 2] == "TU" && is_strict_callsign(last) && me(last) == CallMatch::No {
            return verdict(YourTurn, Some(Role::MyCall), None, Some(last), true);
        }
        // Worked before: nothing to send.
        if last == "B4" {
            return verdict(Hold, None, None, from, false);
        }
    }
    // Our call leading a report, with no DE: a DX or a runner came back to us. In a contest the
    // exchange key fits; outside one the everyday reply key sends a whole ragchew, so none does.
    if de.is_none()
        && reported.is_some_and(|r| r > 0)
        && w.first().is_some_and(|x| me(x) != CallMatch::No)
    {
        return if inp.contest {
            verdict(YourTurn, Some(Role::Exchange), None, from, false)
        } else {
            verdict(YourTurn, None, Some(Hint::FiveNnTu), from, false)
        };
    }
    // The hand-over: K, KN or BK.
    if matches!(last, "K" | "KN" | "BK" | "(") {
        if exact_me || to_me == Some(CallMatch::Near) {
            // They came back to our CQ or our call: the report fits. Further on, it is a ragchew.
            let (role, hint) = match kind {
                None | Some(Sent::Cq) | Some(Sent::Call) => (Some(Role::Reply), None),
                _ => (None, Some(Hint::Type)),
            };
            let hint = hint.or((!exact_me).then_some(Hint::Partial));
            return verdict(YourTurn, role, hint, from, false);
        }
        return if over.after_us {
            verdict(YourTurn, None, Some(Hint::Type), from, false)
        } else {
            // A contact we are not in.
            verdict(Hold, None, None, from, false)
        };
    }
    // After our CQ, a call alone (or twice): a caller answering, contest style.
    if over.after_us && kind == Some(Sent::Cq) && n <= 3 && w.iter().all(|x| is_strict_callsign(x))
    {
        let fits = inp.his_call.is_some_and(|h| same_call(h, w[0]));
        return verdict(
            YourTurn,
            fits.then_some(Role::Reply),
            None,
            Some(w[0]),
            false,
        );
    }
    None
}

/// No over of theirs since we last keyed: we listen, for as long as what we sent deserves.
fn listening(inp: &Input, th: Thresholds) -> Turn {
    let Some(&(_, Some(end))) = inp.keyed.last() else {
        return Turn::idle();
    };
    let since = (inp.now - end).max(0.0);
    let his = inp.his_call.map(str::to_string);
    // Keying at the pitch since our own sidetone died away: they have started, ahead of the copy.
    if inp
        .mark_age
        .is_some_and(|a| a < th.s1 && a < since - ECHO_HANG_SECS)
    {
        return Turn {
            state: State::TheySend,
            call: his,
            ..Turn::idle()
        };
    }
    let kind = sent_kind(inp.last_sent, inp.mycall, inp.contest);
    let total = match kind {
        Some(Sent::Cq | Sent::Call) if inp.contest => Some(LISTEN_CONTEST_SECS),
        Some(Sent::Cq) => Some(LISTEN_CQ_SECS),
        Some(Sent::Call) => Some(LISTEN_CALL_SECS),
        _ => None,
    };
    // After a CQ nobody is named.
    let call = if kind == Some(Sent::Cq) { None } else { his };
    match total {
        Some(total) if since >= total => Turn {
            state: State::Idle,
            role: Some(match kind {
                Some(Sent::Cq) => Role::Cq,
                _ if inp.contest => Role::MyCall,
                _ => Role::Answer,
            }),
            hint: Some(Hint::NoReply),
            call,
            ..Turn::idle()
        },
        Some(total) => Turn {
            state: State::Listen,
            window: Some(window(total, total - since)),
            call,
            ..Turn::idle()
        },
        None if since >= IDLE_SECS => Turn::idle(),
        None => Turn {
            state: State::Listen,
            call,
            ..Turn::idle()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Letters a decoder places about this far apart: near 20 WPM's average.
    const STEP: f64 = 0.15;

    /// A timeline: what the decoder heard, what we keyed, and the clock.
    #[derive(Default, Clone)]
    struct Scene {
        letters: Vec<Letter>,
        keyed: Vec<Keyed>,
        last_sent: Option<String>,
        t: f64,
    }

    impl Scene {
        fn new() -> Self {
            Scene {
                t: 1.0,
                ..Default::default()
            }
        }
        /// The other station sends `text`, starting now; the clock stops at its last letter.
        fn hear(mut self, text: &str) -> Self {
            for (i, c) in text.chars().enumerate() {
                if i > 0 {
                    self.t += if c == ' ' { STEP * 2.0 } else { STEP };
                }
                self.letters.push((self.t, c));
            }
            self
        }
        fn quiet(mut self, secs: f64) -> Self {
            self.t += secs;
            self
        }
        /// We key `text` for `secs`, from a beat after now.
        fn send(mut self, text: &str, secs: f64) -> Self {
            self.t += 0.2;
            self.keyed.push((self.t, Some(self.t + secs)));
            self.last_sent = Some(text.to_string());
            self.t += secs;
            self
        }
        /// We key `text` and the receiver hears it back, every letter inside our transmission.
        fn send_heard_back(mut self, text: &str, secs: f64) -> Self {
            let start = self.t + 0.2;
            let step = secs / text.chars().count() as f64;
            for (i, c) in text.chars().enumerate() {
                self.letters.push((start + 0.05 + i as f64 * step, c));
            }
            self.send(text, secs)
        }
        /// We start keying `text` and are still at it.
        fn sending(mut self, text: &str) -> Self {
            self.t += 0.2;
            self.keyed.push((self.t, None));
            self.last_sent = Some(text.to_string());
            self.t += 0.5;
            self
        }
    }

    #[derive(Default, Clone, Copy)]
    struct Opts<'a> {
        his: Option<&'a str>,
        contest: bool,
        split: bool,
        mark_age: Option<f64>,
        /// How far the decoder's listening trails the clock.
        lag: f64,
    }

    fn read_with(s: &Scene, o: Opts) -> Turn {
        read(&Input {
            letters: &s.letters,
            keyed: &s.keyed,
            heard_until: s.t - o.lag,
            now: s.t,
            mark_age: o.mark_age,
            wpm: Some(20),
            mycall: "KD9TAW",
            his_call: o.his,
            last_sent: s.last_sent.as_deref(),
            contest: o.contest,
            split: o.split,
        })
    }

    fn his(call: &'static str) -> Opts<'static> {
        Opts {
            his: Some(call),
            ..Default::default()
        }
    }

    /// The parts a test names: state, role, hint and call.
    fn said(t: &Turn) -> (State, Option<Role>, Option<Hint>, Option<&str>) {
        (t.state, t.role, t.hint, t.call.as_deref())
    }

    // ── an over ends at silence AND a marker ────────────────────────────────────────────

    #[test]
    fn an_answered_cq_is_your_turn_only_once_the_k_has_had_its_silence() {
        let cq = Scene::new().hear("CQ CQ DE W1ABC W1ABC K");
        // The K is in, the silence is not yet: the pre-cue, never the turn.
        assert_eq!(
            said(&read_with(&cq.clone().quiet(0.3), his("W1ABC"))),
            (State::Finishing, Some(Role::Answer), None, Some("W1ABC"))
        );
        assert_eq!(
            said(&read_with(&cq.clone().quiet(0.7), his("W1ABC"))),
            (State::YourTurn, Some(Role::Answer), None, Some("W1ABC"))
        );
        // The key that answers sends His Call: with another station in it, no key is suggested.
        assert_eq!(
            said(&read_with(&cq.quiet(0.7), his("K2XYZ"))),
            (State::YourTurn, None, None, Some("W1ABC"))
        );
    }

    #[test]
    fn silence_with_no_marker_is_only_not_sure_however_long() {
        let over = Scene::new()
            .send("W1ABC DE KD9TAW KD9TAW K", 3.0)
            .quiet(1.0)
            .hear("KD9TAW DE W1ABC UR 579 579 NAME BOB");
        // A pause longer than a word gap, inside the over.
        assert_eq!(
            said(&read_with(&over.clone().quiet(1.0), his("W1ABC"))),
            (State::TheySend, None, None, Some("W1ABC"))
        );
        assert_eq!(
            said(&read_with(&over.clone().quiet(2.5), his("W1ABC"))),
            (State::Unsure, Some(Role::Again), None, Some("W1ABC"))
        );
        assert_eq!(
            read_with(&over.quiet(31.0), his("W1ABC")).state,
            State::Idle,
            "half a minute on, it is nobody's turn"
        );
    }

    #[test]
    fn a_cq_that_fades_out_before_its_end_never_hands_over() {
        let faded = Scene::new().hear("CQ CQ DE W1A").quiet(2.5);
        assert_eq!(read_with(&faded, Opts::default()).state, State::Unsure);
    }

    #[test]
    fn the_turn_waits_for_what_the_decoder_has_heard_not_for_the_clock() {
        // Two seconds on by the clock, but the decoder has listened to only 0.3 s past the K.
        let late = Scene::new().hear("CQ CQ DE W1ABC W1ABC K").quiet(2.0);
        let o = Opts {
            his: Some("W1ABC"),
            lag: 1.7,
            ..Default::default()
        };
        assert_eq!(read_with(&late, o).state, State::Finishing);
    }

    #[test]
    fn keying_heard_at_the_pitch_withdraws_the_turn() {
        let cq = Scene::new().hear("CQ CQ DE W1ABC W1ABC K").quiet(0.9);
        let keying = Opts {
            his: Some("W1ABC"),
            mark_age: Some(0.1),
            ..Default::default()
        };
        assert_eq!(read_with(&cq, keying).state, State::TheySend);
        let quiet = Opts {
            mark_age: Some(5.0),
            ..keying
        };
        assert_eq!(read_with(&cq, quiet).state, State::YourTurn, "control");
    }

    // ── overs start after ours ──────────────────────────────────────────────────────────

    #[test]
    fn the_next_stations_cq_after_a_closing_is_an_answer_never_73() {
        let closing = Scene::new()
            .send("W1ABC DE KD9TAW UR 599 599 NAME ALEX HW? KN", 4.0)
            .quiet(1.0)
            .hear("KD9TAW DE W1ABC TU 73 SK")
            .quiet(0.8);
        assert_eq!(
            said(&read_with(&closing, his("W1ABC"))),
            (State::Closing, Some(Role::Close), None, Some("W1ABC"))
        );
        let next = closing
            .send("W1ABC DE KD9TAW TU 73 SK", 2.5)
            .quiet(3.0)
            .hear("CQ CQ DE K2XYZ K2XYZ K")
            .quiet(0.8);
        assert_eq!(
            said(&read_with(&next, his("K2XYZ"))),
            (State::YourTurn, Some(Role::Answer), None, Some("K2XYZ"))
        );
    }

    #[test]
    fn a_long_silence_also_starts_a_new_over() {
        // No transmission of ours between them: W1ABC's 73, a pause, then another station's CQ.
        let s = Scene::new()
            .hear("W2XYZ DE W1ABC 73 SK")
            .quiet(3.0)
            .hear("CQ CQ DE K2XYZ K2XYZ K")
            .quiet(0.8);
        let t = read_with(&s, his("K2XYZ"));
        assert_eq!(
            said(&t),
            (State::YourTurn, Some(Role::Answer), None, Some("K2XYZ"))
        );
        let first_of_cq = s
            .letters
            .iter()
            .rev()
            .nth("CQ CQ DE K2XYZ K2XYZ K".len() - 1)
            .unwrap()
            .0;
        assert_eq!(
            t.over,
            Some((first_of_cq * 1000.0).round() as u64),
            "the over read"
        );
    }

    #[test]
    fn our_own_break_in_starts_a_new_over_however_short() {
        // We break in with a quick "R" mid-over: shorter than any silence that ends an over, and
        // with no marker before it. What they send after it is still a new over, read on its own.
        let s = Scene::new()
            .hear("UR 579 NAME BOB")
            .quiet(0.2)
            .send("R", 0.4)
            .quiet(0.3)
            .hear("KD9TAW DE W1ABC KN")
            .quiet(0.7);
        assert_eq!(
            said(&read_with(&s, his("W1ABC"))),
            (State::YourTurn, None, Some(Hint::Type), Some("W1ABC"))
        );
    }

    #[test]
    fn our_own_sidetone_heard_back_is_not_their_over() {
        let s = Scene::new()
            .send_heard_back("W1ABC DE KD9TAW KD9TAW KN", 3.0)
            .quiet(1.0);
        assert_eq!(read_with(&s, his("W1ABC")).state, State::Listen);
        // A letter just after our key-up is still our echo.
        let mut tail = s.clone();
        let end = tail.keyed[0].1.unwrap();
        tail.letters.push((end + 0.2, 'K'));
        assert_eq!(read_with(&tail, his("W1ABC")).state, State::Listen);
    }

    // ── who it is for ───────────────────────────────────────────────────────────────────

    #[test]
    fn they_came_back_to_my_cq_and_the_reply_fits() {
        let s = Scene::new()
            .send("CQ CQ DE KD9TAW KD9TAW K", 3.0)
            .quiet(1.5)
            .hear("KD9TAW DE W1ABC W1ABC K")
            .quiet(0.7);
        assert_eq!(
            said(&read_with(&s, his("W1ABC"))),
            (State::YourTurn, Some(Role::Reply), None, Some("W1ABC"))
        );
    }

    #[test]
    fn they_came_back_to_my_answer_and_the_reply_fits() {
        let s = Scene::new()
            .send("W1ABC DE KD9TAW KD9TAW K", 3.0)
            .quiet(1.0)
            .hear("KD9TAW DE W1ABC GM TNX CALL UR 579 579 NAME BOB HW? KD9TAW DE W1ABC KN")
            .quiet(0.7);
        assert_eq!(
            said(&read_with(&s, his("W1ABC"))),
            (State::YourTurn, Some(Role::Reply), None, Some("W1ABC"))
        );
    }

    #[test]
    fn in_a_ragchew_kn_or_bk_to_me_is_mine_to_type() {
        let ragchew = Scene::new()
            .send("W1ABC DE KD9TAW UR 599 599 NAME ALEX ALEX HW? KN", 5.0)
            .quiet(2.0);
        let kn = ragchew
            .clone()
            .hear("KD9TAW DE W1ABC R FB ALEX WX HR SUNNY HW? KD9TAW DE W1ABC KN")
            .quiet(0.7);
        assert_eq!(
            said(&read_with(&kn, his("W1ABC"))),
            (State::YourTurn, None, Some(Hint::Type), Some("W1ABC"))
        );
        let bk = ragchew.hear("R R FB BK").quiet(0.7);
        assert_eq!(
            said(&read_with(&bk, his("W1ABC"))),
            (State::YourTurn, None, Some(Hint::Type), Some("W1ABC"))
        );
    }

    #[test]
    fn a_bare_bk_in_a_contact_we_are_not_in_is_theirs() {
        let s = Scene::new().hear("UR 579 579 BK").quiet(0.8);
        assert_eq!(read_with(&s, Opts::default()).state, State::Hold);
    }

    #[test]
    fn an_over_for_another_station_is_a_hold() {
        let s = Scene::new()
            .send("CQ CQ DE KD9TAW K", 2.0)
            .quiet(1.0)
            .hear("W2XYZ DE W1ABC KN")
            .quiet(0.8);
        assert_eq!(
            said(&read_with(&s, his("W1ABC"))),
            (State::Hold, None, None, Some("W2XYZ"))
        );
    }

    #[test]
    fn a_second_station_calling_my_partner_is_not_my_turn() {
        let s = Scene::new()
            .send("W1ABC DE KD9TAW UR 599 KN", 3.0)
            .quiet(1.0)
            .hear("W1ABC DE W2XYZ W2XYZ K")
            .quiet(0.8);
        assert_eq!(
            said(&read_with(&s, his("W1ABC"))),
            (State::Hold, None, None, Some("W1ABC"))
        );
    }

    #[test]
    fn a_closing_in_a_contact_we_are_not_in_is_a_hold() {
        let s = Scene::new().hear("W2XYZ DE W1ABC TU 73 SK").quiet(0.8);
        assert_eq!(
            said(&read_with(&s, Opts::default())),
            (State::Hold, None, None, Some("W2XYZ"))
        );
    }

    #[test]
    fn agn_after_us_asks_for_a_repeat() {
        let s = Scene::new()
            .send("W1ABC DE KD9TAW UR 599 NAME ALEX KN", 3.0)
            .quiet(1.0)
            .hear("AGN?")
            .quiet(0.8);
        assert_eq!(
            said(&read_with(&s, his("W1ABC"))),
            (State::YourTurn, Some(Role::Repeat), None, Some("W1ABC"))
        );
    }

    // ── a pileup ────────────────────────────────────────────────────────────────────────

    fn called() -> Scene {
        Scene::new().send("KD9TAW", 1.0).quiet(0.8)
    }

    #[test]
    fn a_dx_working_another_call_is_a_hold() {
        let s = called().hear("W2XYZ 5NN").quiet(0.8);
        assert_eq!(
            said(&read_with(&s, his("VP8A"))),
            (State::Hold, None, None, Some("W2XYZ"))
        );
    }

    #[test]
    fn a_partial_that_is_mine_is_probably_me_and_one_that_is_not_is_a_hold() {
        let mine = read_with(&called().hear("KD9?").quiet(0.8), his("VP8A"));
        assert_eq!(
            said(&mine),
            (
                State::YourTurn,
                Some(Role::MyCall),
                Some(Hint::Partial),
                Some("VP8A")
            )
        );
        assert_eq!(mine.window.map(|w| w.total), Some(1.5), "a race: it decays");
        assert_eq!(
            said(&read_with(&called().hear("W1A?").quiet(0.8), his("VP8A"))),
            (State::Hold, None, None, Some("W1A"))
        );
        assert_eq!(
            read_with(&called().hear("JA?").quiet(0.8), his("VP8A")).state,
            State::Hold,
            "an area that is not ours"
        );
    }

    #[test]
    fn the_race_window_runs_out_into_they_may_go_on() {
        let late = read_with(&called().hear("KD9?").quiet(0.7 + 1.6), his("VP8A"));
        assert_eq!((late.state, late.hint), (State::Listen, Some(Hint::Late)));
    }

    #[test]
    fn up_with_split_off_holds_and_with_split_on_is_a_call() {
        let s = called().hear("TU UP").quiet(0.8);
        assert_eq!(
            said(&read_with(&s, his("VP8A"))),
            (State::Hold, None, Some(Hint::SplitOff), Some("VP8A"))
        );
        let split = Opts {
            his: Some("VP8A"),
            split: true,
            ..Default::default()
        };
        assert_eq!(
            said(&read_with(&s, split)),
            (State::YourTurn, Some(Role::MyCall), None, Some("VP8A"))
        );
    }

    #[test]
    fn a_dx_coming_back_to_me_suggests_no_key_but_5nn_tu() {
        let s = called().hear("KD9TAW 5NN").quiet(0.8);
        assert_eq!(
            said(&read_with(&s, his("VP8A"))),
            (State::YourTurn, None, Some(Hint::FiveNnTu), Some("VP8A"))
        );
    }

    #[test]
    fn qrz_is_a_call_once() {
        let s = Scene::new().hear("QRZ?").quiet(0.8);
        assert_eq!(read_with(&s, Opts::default()).role, Some(Role::MyCall));
    }

    // ── a contest ───────────────────────────────────────────────────────────────────────

    fn contest(his: &'static str) -> Opts<'static> {
        Opts {
            his: Some(his),
            contest: true,
            ..Default::default()
        }
    }

    #[test]
    fn search_and_pounce_calls_on_tu_and_sends_the_exchange_when_answered() {
        let tu = read_with(&Scene::new().hear("TU N9XX").quiet(0.8), contest("N9XX"));
        assert_eq!(
            said(&tu),
            (State::YourTurn, Some(Role::MyCall), None, Some("N9XX"))
        );
        assert_eq!(tu.window.map(|w| w.total), Some(2.0));
        let answered = called().hear("KD9TAW 5NN 05").quiet(0.8);
        assert_eq!(
            said(&read_with(&answered, contest("N9XX"))),
            (State::YourTurn, Some(Role::Exchange), None, Some("N9XX"))
        );
        let done = Scene::new()
            .send("TU 599 05", 1.5)
            .quiet(0.6)
            .hear("TU N9XX")
            .quiet(0.8);
        assert_eq!(read_with(&done, contest("N9XX")).state, State::Closing);
        let dupe = called().hear("B4").quiet(0.8);
        assert_eq!(read_with(&dupe, contest("N9XX")).state, State::Hold);
    }

    #[test]
    fn running_answers_a_lone_call_and_sends_tu_for_the_exchange() {
        let caller = Scene::new()
            .send("CQ TEST DE KD9TAW KD9TAW K", 2.5)
            .quiet(0.8)
            .hear("W1ABC")
            .quiet(0.8);
        assert_eq!(
            said(&read_with(&caller, contest("W1ABC"))),
            (State::YourTurn, Some(Role::Reply), None, Some("W1ABC"))
        );
        let exchange = Scene::new()
            .send("W1ABC 599 05", 1.5)
            .quiet(0.6)
            .hear("TU 5NN 12")
            .quiet(0.8);
        assert_eq!(
            said(&read_with(&exchange, contest("W1ABC"))),
            (State::YourTurn, Some(Role::Tu), None, Some("W1ABC"))
        );
    }

    // ── our side ────────────────────────────────────────────────────────────────────────

    #[test]
    fn while_we_key_it_is_ours() {
        let s = Scene::new().sending("CQ CQ DE KD9TAW KD9TAW K");
        assert_eq!(read_with(&s, Opts::default()).state, State::YouSend);
    }

    #[test]
    fn after_a_cq_it_listens_then_says_no_reply() {
        let cq = Scene::new().send("CQ CQ DE KD9TAW KD9TAW K", 3.0);
        let listening = read_with(&cq.clone().quiet(3.0), Opts::default());
        assert_eq!(listening.state, State::Listen);
        assert_eq!(
            listening.window,
            Some(Window {
                total: 8.0,
                left: 5.0
            })
        );
        let none = read_with(&cq.clone().quiet(9.0), Opts::default());
        assert_eq!(
            (none.state, none.role, none.hint),
            (State::Idle, Some(Role::Cq), Some(Hint::NoReply))
        );
        let contest_cq = read_with(&cq.quiet(1.0), contest("W1ABC"));
        assert_eq!(contest_cq.window.map(|w| w.total), Some(2.5));
    }

    #[test]
    fn after_calling_a_station_no_reply_suggests_calling_again() {
        let s = Scene::new()
            .send("W1ABC DE KD9TAW KD9TAW K", 3.0)
            .quiet(6.0);
        let t = read_with(&s, his("W1ABC"));
        assert_eq!(
            (t.state, t.role, t.hint),
            (State::Idle, Some(Role::Answer), Some(Hint::NoReply))
        );
    }

    #[test]
    fn keying_at_the_pitch_after_our_over_is_them_starting() {
        let cq = Scene::new().send("CQ CQ DE KD9TAW KD9TAW K", 3.0);
        let o = Opts {
            mark_age: Some(0.1),
            ..Default::default()
        };
        assert_eq!(read_with(&cq.clone().quiet(1.5), o).state, State::TheySend);
        // 0.4 s after our key-up, a mark 0.35 s old is our own sidetone's last one.
        let echo = Opts {
            mark_age: Some(0.35),
            ..Default::default()
        };
        assert_eq!(read_with(&cq.quiet(0.4), echo).state, State::Listen);
    }

    #[test]
    fn nothing_heard_and_nothing_sent_is_idle() {
        assert_eq!(read_with(&Scene::new(), Opts::default()), Turn::idle());
    }

    #[test]
    fn a_hold_ends_after_eight_seconds_and_a_turn_after_thirty() {
        let hold = Scene::new().hear("W2XYZ DE W1ABC KN");
        assert_eq!(
            read_with(&hold.clone().quiet(7.0), Opts::default()).state,
            State::Hold
        );
        assert_eq!(
            read_with(&hold.quiet(8.5), Opts::default()).state,
            State::Idle
        );
        let turn = Scene::new().hear("CQ CQ DE W1ABC W1ABC K");
        assert_eq!(
            read_with(&turn.clone().quiet(29.0), his("W1ABC")).state,
            State::YourTurn
        );
        assert_eq!(
            read_with(&turn.quiet(31.0), his("W1ABC")).state,
            State::Idle
        );
    }

    // ── the pieces ──────────────────────────────────────────────────────────────────────

    #[test]
    fn thresholds_follow_the_other_stations_speed() {
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let t20 = Thresholds::at(Some(20));
        assert!(near(t20.s1, 0.63) && near(t20.s2, 2.1), "{t20:?}");
        let t10 = Thresholds::at(Some(10));
        assert!(near(t10.s1, 1.26) && near(t10.s2, 4.2), "{t10:?}");
        // Fast senders keep a floor.
        assert_eq!(Thresholds::at(Some(40)), Thresholds { s1: 0.5, s2: 2.0 });
        // Unknown speed is taken as 20.
        assert_eq!(Thresholds::at(None), t20);
    }

    #[test]
    fn a_query_matches_our_call_the_way_a_dx_operator_means_it() {
        for (q, want) in [
            ("KD9TAW", CallMatch::Exact),
            ("KD9TAW/P", CallMatch::Exact),
            ("KD9?", CallMatch::Near),
            ("TAW?", CallMatch::Near),
            ("KD9TAX", CallMatch::Near),
            ("W1ABC", CallMatch::No),
            ("AW", CallMatch::No),
            ("K?", CallMatch::No),
            ("?", CallMatch::No),
        ] {
            assert_eq!(match_call(q, "KD9TAW"), want, "{q}");
        }
    }
}
