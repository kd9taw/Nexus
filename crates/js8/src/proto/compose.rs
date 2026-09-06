//! Compose — operator text → the frame sequence JS8Call would send (varicode.cpp:1980-2191
//! `buildMessageFrames`, read as the spec).
//!
//! CONTRACT. `frames_with_grid(mycall, grid, to, text, speed)` returns `(Frame, I3)` pairs in
//! transmit order with `first` on frame 0 and `last` on frame n-1. Order of attempts per
//! remaining line — heartbeat/CQ, compound (a `` ` `` line), directed, data — and after a
//! directed or data frame ONLY data frames follow (:2047-2070). `to` is JS8Call's "selected
//! call": prepended unless the line already starts with it, with @ALLCALL / a CQ or HB word /
//! a callsign, or with `` ` `` (:2016-2040). A leading "MYCALL:" or "MYCALL " is stripped
//! (:2001-2004) and plain data is prefixed "MYCALL: " when nothing else identifies the
//! sender (:2065-2069; Nexus has no avoid_forced_identify — spec invariant 10). The remainder
//! after a directed command is left-stripped and, for the checksummed commands, gets
//! `" " + checksum3(body)` appended (:2148-2172) before it is data-framed. Compound cases
//! (:2100-2146): when MYCALL or the destination is not base-packable, the frames are
//! `Compound { mycall, grid }` then `CompoundDirected { to, cmd, num }`.
//!
//! WHAT IS REFUSED, and why here: an empty or non-packable MYCALL (`NoCallsign`) — this is the
//! mode's identity gate below the engine's `structured_tx_ready` (spec TX invariant 2);
//! `@APRSIS` / `@JS8NET` destinations (`ForbiddenDestination`, mainwindow.cpp:4043/4068 via
//! `may_transmit_to`); an empty line; and more than `max_frames(speed)` frames (`TooLong`) —
//! frames × period must stay under 600 s so §97.119 identification holds without inserting a
//! frame a JS8Call receiver would read as a new message.
//!
//! Text discipline: uppercase (JS8Call's editor forces it; Huffman is uppercase-only) and
//! Latin-1 (chars above U+00FF become '?'). The grammars below are hand ports of three Qt
//! regexes; alternation ORDER is preserved because it is the wire.
//!
//! BASE-vs-COMPOUND DECISION (carry-forward B1 review finding a): base-vs-compound is decided
//! the way `buildMessageFrames`/`packDirectedMessage` decide it — `isCompoundCallsign` for
//! MYCALL and `isValidCallsign`'s `isCompound` for the destination — NEVER by `is_base_call`
//! alone, which applies the 3DA0/3X substitution and so labels "3DA0AB"/"3XA1BC" base when
//! JS8Call treats them as compound. `is_compound_call` is the faithful `isCompoundCallsign`
//! port; `!special && is_compound_call(to)` reproduces `isValidCallsign`'s `isCompound` (its
//! only difference from `isCompoundCallsign` is that a `basecalls` special is never compound).
use crate::phy::{Speed, I3};
use crate::proto::alphabet::CQS;
use crate::proto::callsign::{
    is_base_call, is_compound_call, may_transmit_to, pack28, split_portable, CallRef,
};
use crate::proto::command::Command;
use crate::proto::crc16::checksum3;
use crate::proto::frame::{pack_data_prefix, Frame};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    NoCallsign,
    ForbiddenDestination,
    Empty,
    TooLong { frames: usize, max: usize },
}

/// §97.119 airtime cap: the largest frame count with frames × period < 600 s
/// (Slow 19 · Normal 39 · Fast 59 · Turbo 99).
pub const fn max_frames(speed: Speed) -> usize {
    (599 / speed.period_s()) as usize
}

const NO_DATA: I3 = I3 {
    first: false,
    last: false,
    data: false,
};

fn normalise(text: &str) -> String {
    text.chars()
        .map(|c| {
            if (c as u32) > 0xFF {
                '?'
            } else {
                c.to_ascii_uppercase()
            }
        })
        .collect()
}

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `\b` after a word char at `k`.
fn boundary(b: &[u8], k: usize) -> bool {
    k >= b.len() || !is_word(b[k])
}

fn is_grid4(s: &[u8]) -> bool {
    s.len() == 4
        && s[..2].iter().all(|c| (b'A'..=b'R').contains(c))
        && s[2..].iter().all(u8::is_ascii_digit)
}

/// `[@]?[A-Z0-9/]+` at the start; returns the token length (0 = no token).
fn callsign_token(b: &[u8]) -> usize {
    let mut i = 0;
    if b.first() == Some(&b'@') {
        i = 1;
    }
    let start = i;
    while i < b.len() && (b[i].is_ascii_uppercase() || b[i].is_ascii_digit() || b[i] == b'/') {
        i += 1;
    }
    if i == start {
        0
    } else {
        i
    }
}

// ---- heartbeat_re (varicode.cpp:146) ----
// ^\s*(?<callsign>[@](?:ALLCALL|HB)\s+)?(?<type>CQ CQ CQ|CQ DX|CQ QRP|CQ CONTEST|CQ FIELD|CQ FD|CQ CQ|CQ|HB|HEARTBEAT(?!\s+SNR))(?:\s(?<grid>[A-R]{2}[0-9]{2}))?\b
const HB_TYPES: [&str; 10] = [
    "CQ CQ CQ",
    "CQ DX",
    "CQ QRP",
    "CQ CONTEST",
    "CQ FIELD",
    "CQ FD",
    "CQ CQ",
    "CQ",
    "HB",
    "HEARTBEAT",
];

/// (is_cq, cq index, grid, bytes consumed)
fn parse_heartbeat(line: &str) -> Option<(bool, u8, Option<String>, usize)> {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() && is_ws(b[i]) {
        i += 1;
    }
    let mut j = i;
    for tag in ["@ALLCALL", "@HB"] {
        if b[j..].starts_with(tag.as_bytes()) {
            let mut k = j + tag.len();
            if k < b.len() && is_ws(b[k]) {
                while k < b.len() && is_ws(b[k]) {
                    k += 1;
                }
                j = k;
                break;
            }
        }
    }
    for (ti, t) in HB_TYPES.iter().enumerate() {
        if !b[j..].starts_with(t.as_bytes()) {
            continue;
        }
        let k = j + t.len();
        if *t == "HEARTBEAT" {
            // (?!\s+SNR)
            let mut k2 = k;
            while k2 < b.len() && is_ws(b[k2]) {
                k2 += 1;
            }
            if k2 > k && b[k2..].starts_with(b"SNR") {
                continue;
            }
        }
        // optional single whitespace + 4-char grid, then \b; else \b right after the type.
        if k < b.len()
            && is_ws(b[k])
            && k + 5 <= b.len()
            && is_grid4(&b[k + 1..k + 5])
            && boundary(b, k + 5)
        {
            let grid = String::from_utf8_lossy(&b[k + 1..k + 5]).into_owned();
            return Some((
                t.starts_with("CQ"),
                if t.starts_with("CQ") { ti as u8 } else { 0 },
                Some(grid),
                k + 5,
            ));
        }
        if boundary(b, k) {
            return Some((
                t.starts_with("CQ"),
                if t.starts_with("CQ") { ti as u8 } else { 0 },
                None,
                k,
            ));
        }
    }
    None
}

// ---- optional_cmd_pattern + optional_num_pattern (varicode.cpp:137-141) ----
const WORD_CMDS_Q: [(&str, Command); 10] = [
    ("AGN?", Command::AgnQuery),
    ("QSL?", Command::QslQuery),
    ("HW CPY?", Command::HwCpyQuery),
    ("MSG TO:", Command::MsgTo),
    ("SNR?", Command::SnrQuery),
    ("INFO?", Command::InfoQuery),
    ("GRID?", Command::GridQuery),
    ("STATUS?", Command::StatusQuery),
    ("QUERY MSGS?", Command::QueryMsgs),
    ("HEARING?", Command::HearingQuery),
];
const WORD_CMDS: [(&str, Command); 21] = [
    ("STATUS", Command::Status),
    ("HEARING", Command::Hearing),
    ("QUERY CALL", Command::QueryCall),
    ("QUERY MSGS", Command::QueryMsgs),
    ("QUERY", Command::Query),
    ("CMD", Command::Cmd),
    ("MSG", Command::Msg),
    ("NACK", Command::Nack),
    ("ACK", Command::Ack),
    ("73", Command::SeventyThree),
    ("YES", Command::Yes),
    ("NO", Command::No),
    ("HEARTBEAT SNR", Command::HeartbeatSnr),
    ("SNR", Command::Snr),
    ("QSL", Command::Qsl),
    ("RR", Command::Rr),
    ("SK", Command::Sk),
    ("FB", Command::Fb),
    ("INFO", Command::Info),
    ("GRID", Command::Grid),
    ("DIT DIT", Command::DitDit),
];

/// `\s?(?:<word cmd>|[?> ])` — the whitespace is tried first, then without (regex backtracking).
fn parse_cmd(rest: &[u8]) -> Option<(Command, usize)> {
    let leads: &[usize] = if rest.first().is_some_and(|&c| is_ws(c)) {
        &[1, 0]
    } else {
        &[0]
    };
    for &lead in leads {
        let r = &rest[lead..];
        for (t, c) in WORD_CMDS_Q {
            if r.starts_with(t.as_bytes()) {
                return Some((c, lead + t.len()));
            }
        }
        for (t, c) in WORD_CMDS {
            if r.starts_with(t.as_bytes()) && (r.len() == t.len() || r[t.len()] == b' ') {
                return Some((c, lead + t.len()));
            }
        }
        match r.first() {
            Some(b'?') => return Some((Command::SnrQuery, lead + 1)),
            Some(b'>') => return Some((Command::Relay, lead + 1)),
            Some(b' ') => return Some((Command::Freetext, lead + 1)),
            _ => {}
        }
    }
    None
}

/// `(?<=SNR)\s?[-+]?(?:3[01]|[0-2]?[0-9])` — only after " SNR" / " HEARTBEAT SNR".
fn parse_num(rest: &[u8]) -> Option<(i8, usize)> {
    let mut i = 0;
    if rest.first().is_some_and(|&c| is_ws(c)) {
        i = 1;
    }
    let mut neg = false;
    match rest.get(i) {
        Some(b'-') => {
            neg = true;
            i += 1;
        }
        Some(b'+') => i += 1,
        _ => {}
    }
    let d0 = *rest.get(i)?;
    if !d0.is_ascii_digit() {
        return None;
    }
    let d1 = rest.get(i + 1).copied();
    let (val, used) = match (d0, d1) {
        (b'3', Some(x)) if x == b'0' || x == b'1' => (30 + (x - b'0') as i32, 2),
        (b'0'..=b'2', Some(x)) if x.is_ascii_digit() => (((d0 - b'0') * 10 + (x - b'0')) as i32, 2),
        _ => ((d0 - b'0') as i32, 1),
    };
    Some(((if neg { -val } else { val }) as i8, i + used))
}

struct Directed {
    to: String,
    to_compound: bool,
    cmd: Command,
    num: Option<i8>,
    consumed: usize,
}

/// directed_re: `^<callsign><cmd?><num?>`; `None` unless a command is present, the
/// destination is a valid call/group and differs from MYCALL (varicode.cpp:1542-1600).
///
/// Validity and base-vs-compound follow `isValidCallsign`(to, &isCompound) (varicode.cpp:1266):
/// a `basecalls` special (`special`) is a valid, non-compound destination; a bare or PORTABLE
/// base call (`is_base_call` on `split_portable(to).0` — upstream's `base_callsign_pattern`
/// carries its own optional `/P` group) is valid and non-compound; otherwise a valid
/// `isCompoundCallsign` is a valid COMPOUND destination. `is_compound_call` — NOT
/// `!is_base_call` — is the compound authority (finding a); the base check strips `/P` so a
/// "W1AW/P" directed message is a Directed frame, not misfiled as data.
fn parse_directed(line: &str, mycall: &str) -> Option<Directed> {
    let b = line.as_bytes();
    let n = callsign_token(b);
    if n == 0 {
        return None;
    }
    let to = &line[..n];
    let (cmd, used) = parse_cmd(&b[n..])?;
    let mut consumed = n + used;
    let mut num = None;
    if cmd.carries_snr() {
        if let Some((v, k)) = parse_num(&b[consumed..]) {
            num = Some(v);
            consumed += k;
        }
    }
    if to == mycall {
        return None;
    }
    let special = CallRef::parse(to).is_some_and(|c| !matches!(c, CallRef::Base(_)));
    let (to_base, _) = split_portable(to);
    if !special && !is_base_call(to_base) && !is_compound_call(to) {
        return None;
    }
    let to_compound = !special && is_compound_call(to);
    Some(Directed {
        to: to.to_string(),
        to_compound,
        cmd,
        num,
        consumed,
    })
}

// ---- compound_re: ^\s*[`]<callsign>(?<grid>\s?[A-R]{2}[0-9]{2})?<cmd?><num?> ----
fn parse_compound(line: &str) -> Option<(Frame, usize)> {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() && is_ws(b[i]) {
        i += 1;
    }
    if b.get(i) != Some(&b'`') {
        return None;
    }
    i += 1;
    let n = callsign_token(&b[i..]);
    if n == 0 {
        return None;
    }
    let call = line[i..i + n].to_string();
    i += n;
    let mut grid = None;
    let g0 = if b.get(i).is_some_and(|&c| is_ws(c)) {
        i + 1
    } else {
        i
    };
    if g0 + 4 <= b.len() && is_grid4(&b[g0..g0 + 4]) {
        grid = Some(String::from_utf8_lossy(&b[g0..g0 + 4]).into_owned());
        i = g0 + 4;
    }
    if let Some((cmd, used)) = parse_cmd(&b[i..]) {
        i += used;
        let mut num = None;
        if cmd.carries_snr() {
            if let Some((v, k)) = parse_num(&b[i..]) {
                num = Some(v);
                i += k;
            }
        }
        return Some((Frame::CompoundDirected { call, cmd, num }, i));
    }
    Some((Frame::Compound { call, grid }, i))
}

fn first_token_is_callsign(line: &str) -> bool {
    let n = callsign_token(line.as_bytes());
    let tok = &line[..n];
    n > 3 && (is_base_call(tok) || is_compound_call(tok) || CallRef::parse(tok).is_some())
}

fn lstrip(s: &str) -> &str {
    s.trim_start_matches([' ', '\t', '\n', '\r'])
}

/// `isCommandBuffered` (varicode.cpp:1217): the matched command text contains a space OR its id
/// is in `buffered_cmds`. In our `Command` model every `text()` has a leading space except
/// Relay (">"), which is in the buffered set, so this is true for every command — but the
/// remainder-strip gate expresses the REAL rule (carry-forward B1 review finding c: do not
/// treat `is_buffered()` as the whole rule), so a spaceless non-buffered command added later
/// would leave its trailing text unstripped, exactly as upstream would.
fn is_command_buffered(cmd: Command) -> bool {
    cmd.is_buffered() || cmd.text().contains(' ')
}

fn build(
    mycall: &str,
    grid: &str,
    to: Option<&CallRef>,
    text: &str,
    speed: Speed,
) -> Result<Vec<(Frame, I3)>, ComposeError> {
    let mycall = normalise(mycall.trim());
    if mycall.is_empty() {
        return Err(ComposeError::NoCallsign);
    }
    // isCompoundCallsign(mycall) — NOT `!is_base_call && is_compound_call` (finding a).
    let mycall_compound = is_compound_call(&mycall);
    if !mycall_compound {
        let (base, _) = split_portable(&mycall);
        if !is_base_call(&mycall) || pack28(&CallRef::Base(base.to_string())).is_none() {
            return Err(ComposeError::NoCallsign);
        }
    }
    if let Some(t) = to {
        if !may_transmit_to(t) {
            return Err(ComposeError::ForbiddenDestination);
        }
    }
    let mut line = normalise(text);
    let stripped = line.trim_end().to_string();
    if !stripped.is_empty() {
        line = stripped;
    }
    if line.trim().is_empty() {
        return Err(ComposeError::Empty);
    }
    // AUTO_REMOVE_MYCALL
    if line.starts_with(&format!("{mycall}:")) || line.starts_with(&format!("{mycall} ")) {
        line = lstrip(&line[mycall.len() + 1..]).to_string();
    }
    // AUTO_PREPEND_DIRECTED
    if let Some(sel) = to {
        let sel = sel.render();
        if !line.starts_with(&sel) && !line.starts_with('`') {
            let starts_base = line.starts_with("@ALLCALL")
                || CQS.iter().any(|c| line.starts_with(c))
                || line.starts_with("HB");
            if !starts_base && !first_token_is_callsign(&line) {
                let sep = if line.starts_with(' ') { "" } else { " " };
                line = format!("{sel}{sep}{line}");
            }
        }
    }
    let grid4: String = normalise(grid.trim()).chars().take(4).collect();
    let mut out: Vec<(Frame, I3)> = Vec::new();
    let mut has_directed = false;
    let mut has_data = false;
    while !line.is_empty() {
        if !has_directed && !has_data {
            if let Some((is_cq, idx, g, n)) = parse_heartbeat(&line) {
                out.push((
                    Frame::Heartbeat {
                        call: mycall.clone(),
                        grid: g,
                        is_cq,
                        idx,
                    },
                    NO_DATA,
                ));
                line = line[n..].to_string();
                continue;
            }
            if let Some((frame, n)) = parse_compound(&line) {
                out.push((frame, NO_DATA));
                line = line[n..].to_string();
                continue;
            }
            if let Some(d) = parse_directed(&line, &mycall) {
                if mycall_compound || d.to_compound {
                    // CASE 1-3: `MYCALL GRID then `TO CMD NUM
                    let g = if is_grid4(grid4.as_bytes()) {
                        Some(grid4.clone())
                    } else {
                        None
                    };
                    out.push((
                        Frame::Compound {
                            call: mycall.clone(),
                            grid: g,
                        },
                        NO_DATA,
                    ));
                    out.push((
                        Frame::CompoundDirected {
                            call: d.to.clone(),
                            cmd: d.cmd,
                            num: d.num,
                        },
                        NO_DATA,
                    ));
                } else {
                    let (from_base, portable_from) = split_portable(&mycall);
                    let (to_base, portable_to) = split_portable(&d.to);
                    let to_ref = CallRef::parse(to_base).ok_or(ComposeError::NoCallsign)?;
                    out.push((
                        Frame::Directed {
                            from: CallRef::Base(from_base.to_string()),
                            to: to_ref,
                            cmd: d.cmd,
                            num: d.num,
                            portable_from,
                            portable_to,
                        },
                        NO_DATA,
                    ));
                }
                has_directed = true;
                line = line[d.consumed..].to_string();
                if is_command_buffered(d.cmd) && !line.is_empty() {
                    // strip leading whitespace after a buffered directed command, then checksum
                    // if the command is one of the checksummed ones (varicode.cpp:2148-2172).
                    line = lstrip(&line).to_string();
                    if d.cmd.is_checksummed() {
                        line = format!("{line} {}", checksum3(&line));
                    }
                }
                continue;
            }
            // forceIdentify (:2065-2069): first frame, no selected call, nothing parsed.
            if out.is_empty() && to.is_none() && !line.contains(&mycall) {
                line = format!("{mycall}: {line}");
            }
        }
        match pack_data_prefix(&line, speed) {
            Some((_, i3, n)) => {
                let taken: String = line.chars().take(n).collect();
                line = line.chars().skip(n).collect();
                out.push((
                    Frame::Data {
                        text: taken,
                        dense: i3.data,
                    },
                    i3,
                ));
                has_data = true;
            }
            None => break, // nothing packable at the head of the line (upstream would spin)
        }
    }
    if let Some(first) = out.first_mut() {
        first.1.first = true;
    }
    if let Some(last) = out.last_mut() {
        last.1.last = true;
    }
    Ok(out)
}

/// buildMessageFrames order with the compound grid available; see the module header.
pub fn frames_with_grid(
    mycall: &str,
    grid: &str,
    to: Option<&CallRef>,
    text: &str,
    speed: Speed,
) -> Result<Vec<(Frame, I3)>, ComposeError> {
    let out = build(mycall, grid, to, text, speed)?;
    let max = max_frames(speed);
    if out.len() > max {
        return Err(ComposeError::TooLong {
            frames: out.len(),
            max,
        });
    }
    Ok(out)
}

/// The binding entry (interfaces §1.11): no grid → a compound announcement carries none.
pub fn frames(
    mycall: &str,
    to: Option<&CallRef>,
    text: &str,
    speed: Speed,
) -> Result<Vec<(Frame, I3)>, ComposeError> {
    frames_with_grid(mycall, "", to, text, speed)
}

/// Frame count for the composer meter — UNCAPPED so the UI can say "62 frames, max 59".
pub fn frame_count_estimate(mycall: &str, to: Option<&CallRef>, text: &str, speed: Speed) -> usize {
    build(mycall, "", to, text, speed)
        .map(|v| v.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::crc16::verify_checksum3;
    use crate::proto::frame::Frame;

    fn f(to: Option<&CallRef>, text: &str, speed: Speed) -> Vec<(Frame, I3)> {
        frames_with_grid("KD9TAW", "EN52", to, text, speed).unwrap()
    }

    fn data_text(v: &[(Frame, I3)]) -> String {
        v.iter()
            .filter_map(|(fr, _)| {
                if let Frame::Data { text, .. } = fr {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn max_frames_keeps_every_speed_under_ten_minutes() {
        assert_eq!(max_frames(Speed::Slow), 19);
        assert_eq!(max_frames(Speed::Normal), 39);
        assert_eq!(max_frames(Speed::Fast), 59);
        assert_eq!(max_frames(Speed::Turbo), 99);
        for s in Speed::ALL {
            assert!(max_frames(s) as u32 * s.period_s() < 600);
            assert!((max_frames(s) as u32 + 1) * s.period_s() >= 600);
        }
    }

    #[test]
    fn cq_and_heartbeat_are_single_heartbeat_frames_with_first_and_last() {
        let v = f(None, "CQ CQ CQ EN52", Speed::Normal);
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0].0,
            Frame::Heartbeat {
                call: "KD9TAW".into(),
                grid: Some("EN52".into()),
                is_cq: true,
                idx: 0
            }
        );
        assert!(v[0].1.first && v[0].1.last && !v[0].1.data);
        assert_eq!(v[0].0.render(), "KD9TAW: @ALLCALL CQ CQ CQ EN52 ");
        // JS8Call's own HB text is "MYCALL: HEARTBEAT GRID" — the leading MYCALL: is stripped.
        let v = f(None, "KD9TAW: HEARTBEAT EN52", Speed::Fast);
        assert_eq!(v[0].0.render(), "KD9TAW: @HB HEARTBEAT EN52 ");
        let v = f(None, "CQ", Speed::Slow);
        assert_eq!(
            v[0].0,
            Frame::Heartbeat {
                call: "KD9TAW".into(),
                grid: None,
                is_cq: true,
                idx: 7
            }
        );
        // "CQ WFD EM73" is a bare CQ (no grid) followed by data — the WFD text is not a grid.
        let v = f(None, "CQ WFD EM73", Speed::Fast);
        assert!(matches!(
            v[0].0,
            Frame::Heartbeat {
                is_cq: true,
                idx: 7,
                grid: None,
                ..
            }
        ));
        assert_eq!(data_text(&v), " WFD EM73");
    }

    #[test]
    fn a_selected_call_is_prepended_and_a_command_becomes_one_directed_frame() {
        let to = CallRef::Base("W1AW".into());
        for text in ["SNR?", "W1AW SNR?", " SNR?"] {
            let v = f(Some(&to), text, Speed::Normal);
            assert_eq!(v.len(), 1, "{text:?}");
            assert_eq!(
                v[0].0,
                Frame::Directed {
                    from: CallRef::Base("KD9TAW".into()),
                    to: to.clone(),
                    cmd: Command::SnrQuery,
                    num: None,
                    portable_from: false,
                    portable_to: false
                }
            );
            assert_eq!(v[0].0.render(), "KD9TAW: W1AW SNR? ");
        }
        let v = f(Some(&to), "SNR -12", Speed::Normal);
        assert!(matches!(
            v[0].0,
            Frame::Directed {
                cmd: Command::Snr,
                num: Some(-12),
                ..
            }
        ));
        assert_eq!(v[0].0.render(), "KD9TAW: W1AW SNR -12 ");
        let v = f(None, "W1AW/P HEARTBEAT SNR +07", Speed::Normal);
        assert!(matches!(
            &v[0].0,
            Frame::Directed {
                cmd: Command::HeartbeatSnr,
                num: Some(7),
                portable_to: true,
                ..
            }
        ));
        assert_eq!(v[0].0.render(), "KD9TAW: W1AW/P HEARTBEAT SNR +07 ");
        let v = f(None, "@ALLCALL QUERY CALL KF0UOJ?", Speed::Normal);
        assert!(matches!(
            &v[0].0,
            Frame::Directed {
                to: CallRef::AllCall,
                cmd: Command::QueryCall,
                ..
            }
        ));
    }

    #[test]
    fn a_buffered_checksummed_command_gets_the_body_and_a_kermit_checksum_in_data_frames() {
        let to = CallRef::Base("W1AW".into());
        let v = f(Some(&to), "MSG HELLO WORLD", Speed::Fast);
        assert!(matches!(
            v[0].0,
            Frame::Directed {
                cmd: Command::Msg,
                ..
            }
        ));
        assert!(v[0].1.first && !v[0].1.last);
        assert!(v.last().unwrap().1.last);
        let body = data_text(&v);
        assert_eq!(body, format!("HELLO WORLD {}", checksum3("HELLO WORLD")));
        assert_eq!(verify_checksum3(&body), Some("HELLO WORLD"));
        assert!(v[1..]
            .iter()
            .all(|(fr, i3)| matches!(fr, Frame::Data { .. }) && i3.data));
        // Free text after a call is a Freetext directed frame + data WITHOUT a checksum.
        let v = f(Some(&to), "HELLO", Speed::Normal);
        assert!(matches!(
            v[0].0,
            Frame::Directed {
                cmd: Command::Freetext,
                ..
            }
        ));
        assert_eq!(v[0].0.render(), "KD9TAW: W1AW  ");
        assert_eq!(data_text(&v), "HELLO");
        assert!(
            v[1..].iter().all(|(_, i3)| !i3.data),
            "Normal emits the deprecated data form"
        );
    }

    #[test]
    fn plain_text_is_identified_and_split_by_speed() {
        let v = f(None, "TNX 73 GL", Speed::Normal);
        assert!(v.iter().all(|(fr, _)| matches!(fr, Frame::Data { .. })));
        assert_eq!(data_text(&v), "KD9TAW: TNX 73 GL");
        assert!(v.iter().all(|(_, i3)| !i3.data));
        let v = f(None, "hello, world", Speed::Turbo);
        assert_eq!(
            data_text(&v),
            "KD9TAW: HELLO, WORLD",
            "uppercased before framing"
        );
        assert!(v.iter().all(|(_, i3)| i3.data));
        assert!(v[0].1.first && v.last().unwrap().1.last);
        assert!(v.iter().skip(1).all(|(_, i3)| !i3.first));
        assert!(v.iter().take(v.len() - 1).all(|(_, i3)| !i3.last));
        assert_eq!(
            frame_count_estimate("KD9TAW", None, "hello, world", Speed::Turbo),
            v.len()
        );
    }

    #[test]
    fn compound_calls_go_out_as_compound_plus_compound_directed() {
        let to = CallRef::Base("W1AW".into());
        let v = frames_with_grid("KD9TAW/QRP", "EN52", Some(&to), "SNR?", Speed::Normal).unwrap();
        assert_eq!(
            v[0].0,
            Frame::Compound {
                call: "KD9TAW/QRP".into(),
                grid: Some("EN52".into())
            }
        );
        assert_eq!(
            v[1].0,
            Frame::CompoundDirected {
                call: "W1AW".into(),
                cmd: Command::SnrQuery,
                num: None
            }
        );
        assert!(v[0].1.first && v[1].1.last && v.len() == 2);
        let v = f(None, "KD4JRX/WFD SNR?", Speed::Normal);
        assert_eq!(
            v[0].0,
            Frame::Compound {
                call: "KD9TAW".into(),
                grid: Some("EN52".into())
            }
        );
        assert_eq!(
            v[1].0,
            Frame::CompoundDirected {
                call: "KD4JRX/WFD".into(),
                cmd: Command::SnrQuery,
                num: None
            }
        );
        let v = f(None, "@WFD SNR?", Speed::Normal);
        assert!(
            matches!(&v[1].0, Frame::CompoundDirected { call, .. } if call == "@WFD"),
            "an unlisted group is a compound destination"
        );
        let v = f(None, "`KD9TAW/QRP EN52", Speed::Normal);
        assert_eq!(
            v[0].0,
            Frame::Compound {
                call: "KD9TAW/QRP".into(),
                grid: Some("EN52".into())
            }
        );
    }

    /// Carry-forward B1 review finding (a): `is_base_call` applies the 3DA0/3X substitution and
    /// labels "3DA0AB"/"3XA1BC" a BASE call, but JS8Call decides base-vs-compound with
    /// `isCompoundCallsign` (`base_callsign_pattern`, no substitution), which treats them as
    /// COMPOUND. So a directed message TO such a call goes out as Compound + CompoundDirected,
    /// and such a call as MYCALL prepends its own Compound announcement — as JS8Call would, not
    /// as calling `is_base_call` alone would. (None of these appears in the 1333-frame golden
    /// log, so only this test pins the divergence.)
    #[test]
    fn a_workaround_prefix_destination_routes_as_compound_not_base() {
        // 3DA0AB as the destination: Compound(MYCALL) + CompoundDirected(3DA0AB).
        let v = f(None, "3DA0AB SNR?", Speed::Normal);
        assert_eq!(v.len(), 2);
        assert_eq!(
            v[0].0,
            Frame::Compound {
                call: "KD9TAW".into(),
                grid: Some("EN52".into())
            }
        );
        assert!(
            matches!(&v[1].0, Frame::CompoundDirected { call, cmd: Command::SnrQuery, .. } if call == "3DA0AB")
        );
        // 3XA1BC as MYCALL: its own compound announcement leads.
        let to = CallRef::Base("W1AW".into());
        let v = frames_with_grid("3XA1BC", "EN52", Some(&to), "SNR?", Speed::Normal).unwrap();
        assert_eq!(
            v[0].0,
            Frame::Compound {
                call: "3XA1BC".into(),
                grid: Some("EN52".into())
            }
        );
        assert_eq!(
            v[1].0,
            Frame::CompoundDirected {
                call: "W1AW".into(),
                cmd: Command::SnrQuery,
                num: None
            }
        );
    }

    /// The plan's original validity gate (`is_base_call(to)` alone) rejected a PORTABLE base call
    /// like "W1AW/P" (is_base=false because it never strips `/P`; is_compound=false because the
    /// base under it fits `base_callsign_pattern`), turning a legal directed message into data.
    /// Upstream's `isValidCallsign` accepts it via `base_callsign_pattern`'s own `/P` group, so
    /// the base check runs on `split_portable(to).0`.
    #[test]
    fn a_portable_base_destination_is_a_directed_frame_not_data() {
        let v = f(None, "W1AW/P SNR?", Speed::Normal);
        assert_eq!(v.len(), 1);
        assert!(
            matches!(&v[0].0, Frame::Directed { to: CallRef::Base(b), portable_to: true, cmd: Command::SnrQuery, .. } if b == "W1AW")
        );
        assert_eq!(v[0].0.render(), "KD9TAW: W1AW/P SNR? ");
    }

    #[test]
    fn refusals() {
        assert_eq!(
            frames("", None, "HELLO", Speed::Normal),
            Err(ComposeError::NoCallsign)
        );
        assert_eq!(
            frames("1", None, "HELLO", Speed::Normal),
            Err(ComposeError::NoCallsign)
        );
        assert_eq!(
            frames("KD9TAW", None, "   ", Speed::Normal),
            Err(ComposeError::Empty)
        );
        assert_eq!(
            frames("KD9TAW", Some(&CallRef::Js8Net), "HELLO", Speed::Normal),
            Err(ComposeError::ForbiddenDestination)
        );
        let aprsis = CallRef::parse("@APRSIS").expect("@APRSIS is group 33-4");
        assert_eq!(
            frames("KD9TAW", Some(&aprsis), "HELLO", Speed::Normal),
            Err(ComposeError::ForbiddenDestination)
        );
        let long = "LOREM IPSUM DOLOR SIT AMET CONSECTETUR ".repeat(40);
        match frames("KD9TAW", None, &long, Speed::Slow) {
            Err(ComposeError::TooLong { frames, max }) => assert!(frames > 19 && max == 19),
            other => panic!("{other:?}"),
        }
        assert!(
            frame_count_estimate("KD9TAW", None, &long, Speed::Slow) > 19,
            "the estimate reports the uncapped count"
        );
    }
}
