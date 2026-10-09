//! The session format: what the simulated radio says, and when.
//!
//! One text format serves both kinds of session. A synthetic session is written by hand from the
//! protocol authority; a recorded one is written by the recorder ([`crate::record`]) from a real
//! radio. The simulator replays either.
//!
//! ```text
//! flexsession 1                      the format line, first
//! # a comment                        whole-line comments only: '#' is ordinary text elsewhere,
//!                                    because meter and GPS status use it as a separator
//! prologue V1.4.0.0                  the version line the radio sends on connect
//! handle 2B6E1F40                    the first connection's client handle (hex, nonzero)
//! default 50000015                   the reply code for a command no rule matches
//!
//! on xmit 1                          a rule: what the radio does when a client sends this command
//! reply 0                            the reply, sent as R<the client's seq>|0|  (exactly one, first)
//! send S0|interlock state=...        a line to send after it ({h} = this connection's handle as
//!                                    eight hex digits, {peer} = the client's IP address)
//! wait 50                            a pause before the items that follow, in milliseconds
//! vita 18000007...                   a raw VITA-49 datagram for the client's UDP port, in hex
//!
//! at 1500                            a timed block: items sent 1500 ms after the connection opens
//! send S0|interlock ...
//! ```
//!
//! **Matching.** `on xmit 1` matches that exact command text. A pattern ending in `*` matches by
//! prefix: `on client gui *` matches `client gui` followed by a space and anything. An exact rule
//! beats a prefix rule, and a longer prefix beats a shorter one.
//!
//! **Repeats.** When several rules share a pattern, the first answers the first matching command
//! on a connection, the second the second, and the last answers every one after that. A recording
//! of twenty pings is twenty `on ping` rules, replayed in order.
//!
//! **Ordering.** A rule's reply is sent at once. Its `send` items keep the order the API
//! documents: every status reported by an earlier command is sent before the first status
//! reported by a later one (SmartSDR-TCPIP-API, "Status Format"), so a rule that waits holds back
//! the statuses of the commands after it, though not their replies.

use std::fmt;

/// The format line every session starts with.
pub const FORMAT_LINE: &str = "flexsession 1";

/// The reply code for a command no rule matches: `SL_UNKNOWN_COMMAND` (Known-API-Responses).
pub const UNKNOWN_COMMAND: &str = "50000015";

/// The synthetic SmartSDR v4 session, written from the protocol authority (see the file's header).
pub const V4_GUI_CLIENT: &str = include_str!("../sessions/v4-gui-client.flexsession");

/// One thing a rule or timed block sends, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A line for the TCP stream, before template expansion.
    Send(String),
    /// A pause before the next item.
    Wait(u64),
    /// A raw VITA-49 datagram for the client's registered UDP address.
    Vita(Vec<u8>),
}

/// What the radio does for one matching command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// The response code, exactly as it goes on the wire (`0`, `50000015`, …).
    pub code: String,
    /// The reply's message field (may be empty, may itself contain `|`).
    pub message: String,
    /// What follows the reply.
    pub items: Vec<Item>,
}

/// How a rule's pattern matches a command.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pattern {
    Exact(String),
    /// Everything before the `*`, trailing space included.
    Prefix(String),
}

impl Pattern {
    fn parse(text: &str) -> Pattern {
        match text.strip_suffix('*') {
            Some(prefix) => Pattern::Prefix(prefix.to_string()),
            None => Pattern::Exact(text.to_string()),
        }
    }

    fn matches(&self, command: &str) -> bool {
        match self {
            Pattern::Exact(p) => p == command,
            Pattern::Prefix(p) => command.starts_with(p.as_str()),
        }
    }
}

/// A parsed session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The version line, e.g. `V1.4.0.0`.
    pub prologue: String,
    /// The first connection's client handle; later connections count up from it.
    pub handle: u32,
    /// The reply code for an unmatched command.
    pub default_code: String,
    /// Rules grouped by pattern, groups in order of first appearance.
    pub rules: Vec<(Pattern, Vec<Rule>)>,
    /// Timed blocks: milliseconds after the connection opens, and their items.
    pub timed: Vec<(u64, Vec<Item>)>,
}

/// Where and why a session failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line number.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// The block being filled while parsing.
enum Block {
    None,
    On(Pattern, Option<Rule>),
    At(u64, Vec<Item>),
}

impl Session {
    /// The synthetic SmartSDR v4 session, parsed. It ships with the crate and a test parses it,
    /// so this cannot fail at run time.
    pub fn v4_gui_client() -> Session {
        Session::parse(V4_GUI_CLIENT).expect("the bundled v4 session parses")
    }

    /// Parse a session from its text.
    pub fn parse(text: &str) -> Result<Session, ParseError> {
        let mut prologue = None;
        let mut handle = None;
        let mut default_code = UNKNOWN_COMMAND.to_string();
        let mut rules: Vec<(Pattern, Vec<Rule>)> = Vec::new();
        let mut timed = Vec::new();
        let mut block = Block::None;
        let mut seen_format = false;

        for (i, raw) in text.lines().enumerate() {
            let n = i + 1;
            let err = |message: String| ParseError { line: n, message };
            let line = raw.trim_end();
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if !seen_format {
                if trimmed != FORMAT_LINE {
                    return Err(err(format!("expected `{FORMAT_LINE}` first")));
                }
                seen_format = true;
                continue;
            }
            let (word, rest) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
            match word {
                "prologue" | "handle" | "default" | "on" | "at" => {
                    close_block(&mut block, &mut rules, &mut timed, n)?;
                    match word {
                        "prologue" => {
                            if !rest.starts_with('V') || rest.contains(char::is_whitespace) {
                                return Err(err("a prologue is one `V<version>` word".into()));
                            }
                            prologue = Some(rest.to_string());
                        }
                        "handle" => {
                            let hex = rest.strip_prefix("0x").unwrap_or(rest);
                            match u32::from_str_radix(hex, 16) {
                                Ok(h) if h != 0 && !hex.starts_with('+') => handle = Some(h),
                                _ => return Err(err(format!("bad handle `{rest}`"))),
                            }
                        }
                        "default" => {
                            if !crate::line::is_hex_code(rest) {
                                return Err(err(format!("bad reply code `{rest}`")));
                            }
                            default_code = rest.to_string();
                        }
                        "on" => {
                            if rest.is_empty() {
                                return Err(err("`on` needs a command".into()));
                            }
                            block = Block::On(Pattern::parse(rest), None);
                        }
                        _ => {
                            let ms = rest
                                .parse::<u64>()
                                .map_err(|_| err(format!("bad time `{rest}`")))?;
                            block = Block::At(ms, Vec::new());
                        }
                    }
                }
                "reply" => match &mut block {
                    Block::On(_, rule @ None) => {
                        let (code, message) = rest.split_once(' ').unwrap_or((rest, ""));
                        if !crate::line::is_hex_code(code) {
                            return Err(err(format!("bad reply code `{code}`")));
                        }
                        *rule = Some(Rule {
                            code: code.to_string(),
                            message: message.to_string(),
                            items: Vec::new(),
                        });
                    }
                    Block::On(_, Some(_)) => return Err(err("a rule has one reply".into())),
                    _ => return Err(err("`reply` belongs in an `on` block".into())),
                },
                "send" | "wait" | "vita" => {
                    let item = match word {
                        "send" => {
                            if rest.is_empty() {
                                return Err(err("`send` needs a line".into()));
                            }
                            Item::Send(rest.to_string())
                        }
                        "wait" => Item::Wait(
                            rest.parse::<u64>()
                                .map_err(|_| err(format!("bad wait `{rest}`")))?,
                        ),
                        _ => Item::Vita(
                            hex_bytes(rest).ok_or_else(|| err("bad vita hex".to_string()))?,
                        ),
                    };
                    match &mut block {
                        Block::On(_, Some(rule)) => rule.items.push(item),
                        Block::On(_, None) => {
                            return Err(err("a rule's `reply` comes before its items".into()))
                        }
                        Block::At(_, items) => items.push(item),
                        Block::None => return Err(err(format!("`{word}` outside a block"))),
                    }
                }
                _ => return Err(err(format!("unknown directive `{word}`"))),
            }
        }
        let last = text.lines().count();
        close_block(&mut block, &mut rules, &mut timed, last)?;
        if !seen_format {
            return Err(ParseError {
                line: last,
                message: format!("expected `{FORMAT_LINE}` first"),
            });
        }
        Ok(Session {
            prologue: prologue.ok_or(ParseError {
                line: last,
                message: "no `prologue`".into(),
            })?,
            handle: handle.ok_or(ParseError {
                line: last,
                message: "no `handle`".into(),
            })?,
            default_code,
            rules,
            timed,
        })
    }

    /// The group whose pattern answers `command`: an exact pattern first, else the longest
    /// matching prefix. `None` means the default reply. Public so that a test answering for the
    /// radio on its own clock answers from the same rules as the simulator.
    pub fn lookup(&self, command: &str) -> Option<usize> {
        let exact = self
            .rules
            .iter()
            .position(|(p, _)| matches!(p, Pattern::Exact(_)) && p.matches(command));
        exact.or_else(|| {
            self.rules
                .iter()
                .enumerate()
                .filter(|(_, (p, _))| p.matches(command))
                .max_by_key(|(_, (p, _))| match p {
                    Pattern::Prefix(s) => s.len(),
                    Pattern::Exact(_) => 0,
                })
                .map(|(i, _)| i)
        })
    }

    /// The rule a group gives on its `uses`-th match (0-based): in order, then the last again.
    pub fn rule(&self, group: usize, uses: usize) -> &Rule {
        let rules = &self.rules[group].1;
        &rules[uses.min(rules.len() - 1)]
    }
}

fn close_block(
    block: &mut Block,
    rules: &mut Vec<(Pattern, Vec<Rule>)>,
    timed: &mut Vec<(u64, Vec<Item>)>,
    line: usize,
) -> Result<(), ParseError> {
    match std::mem::replace(block, Block::None) {
        Block::None => Ok(()),
        Block::On(pattern, Some(rule)) => {
            match rules.iter_mut().find(|(p, _)| *p == pattern) {
                Some((_, group)) => group.push(rule),
                None => rules.push((pattern, vec![rule])),
            }
            Ok(())
        }
        Block::On(pattern, None) => Err(ParseError {
            line,
            message: format!("the rule for `{pattern:?}` has no `reply`"),
        }),
        Block::At(ms, items) => {
            timed.push((ms, items));
            Ok(())
        }
    }
}

/// Decode an even-length hex string, or `None`.
pub fn hex_bytes(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Encode bytes as lowercase hex, the form the recorder writes after `vita`.
pub fn hex_string(bytes: &[u8]) -> String {
    use fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_v4_session_parses_and_speaks_v4() {
        let s = Session::v4_gui_client();
        // v4 first (maintainer ruling, 2026-10-03): 4.x firmware sends V1.4.0.0 on 6000 and 8000
        // radios alike.
        assert_eq!(s.prologue, "V1.4.0.0");
        assert_eq!(s.default_code, UNKNOWN_COMMAND);
        for command in [
            "client program Nexus",
            "client gui",
            "keepalive enable",
            "ping",
            "sub tx all",
            "sub slice all",
            "client udpport 4993",
            "display panafall create x=1024 y=480",
            "slice create pan=0x40000000 freq=14.074000 mode=DIGU",
            "stream create type=dax_rx dax_channel=1",
            "xmit 1",
            "xmit 0",
            "transmit tune 1",
            "transmit tune 0",
            "atu start",
            "cwx send \"CQ\" 1",
            "cwx clear",
        ] {
            assert!(s.lookup(command).is_some(), "no rule answers {command:?}");
        }
        assert_eq!(s.lookup("frobnicate"), None);
    }

    #[test]
    fn exact_beats_prefix_and_longer_prefix_beats_shorter() {
        let s = Session::parse(
            "flexsession 1\nprologue V1.4.0.0\nhandle 1\n\
             on client *\nreply 1\non client gui *\nreply 2\non client gui\nreply 3\n",
        )
        .unwrap();
        let code = |c: &str| s.lookup(c).map(|g| s.rule(g, 0).code.clone());
        assert_eq!(code("client gui").as_deref(), Some("3"));
        assert_eq!(code("client gui 1234").as_deref(), Some("2"));
        assert_eq!(code("client station x").as_deref(), Some("1"));
        assert_eq!(code("clientx"), None);
    }

    #[test]
    fn repeated_rules_answer_in_order_then_the_last_repeats() {
        let s = Session::parse(
            "flexsession 1\nprologue V1.4.0.0\nhandle 0x2B6E1F40\n\
             on ping\nreply 0\non info\nreply 0 a\non ping\nreply 1\non ping\nreply 2\n",
        )
        .unwrap();
        assert_eq!(s.handle, 0x2B6E_1F40);
        let g = s.lookup("ping").unwrap();
        let codes: Vec<_> = (0..5).map(|u| s.rule(g, u).code.clone()).collect();
        assert_eq!(codes, ["0", "1", "2", "2", "2"]);
    }

    #[test]
    fn items_keep_their_order_and_kind() {
        let s = Session::parse(
            "flexsession 1\nprologue V1.4.0.0\nhandle 1\n\
             on xmit 1\nreply 0\nsend S0|a\nwait 40\nvita 0a0B\nsend M10000001|b # not a comment\n\
             at 250\nsend S0|c\n",
        )
        .unwrap();
        let rule = s.rule(s.lookup("xmit 1").unwrap(), 0);
        assert_eq!(
            rule.items,
            vec![
                Item::Send("S0|a".into()),
                Item::Wait(40),
                Item::Vita(vec![0x0a, 0x0b]),
                Item::Send("M10000001|b # not a comment".into()),
            ]
        );
        assert_eq!(s.timed, vec![(250, vec![Item::Send("S0|c".into())])]);
    }

    #[test]
    fn malformed_sessions_name_the_line() {
        let head = "flexsession 1\nprologue V1.4.0.0\nhandle 1\n";
        for (body, line) in [
            ("on ping\nsend S0|x\n", 5),
            ("on ping\n", 4),
            ("at 10\nreply 0\n", 5),
            ("on ping\nreply 0\nreply 0\n", 6),
            ("on ping\nreply zz\n", 5),
            ("on ping\nreply 0\nvita abc\n", 6),
            ("on ping\nreply 0\nwait soon\n", 6),
            ("send S0|x\n", 4),
            ("bogus\n", 4),
            ("at later\n", 4),
        ] {
            let e = Session::parse(&format!("{head}{body}")).unwrap_err();
            assert_eq!(e.line, line, "{body:?}: {e}");
        }
        for (text, why) in [
            ("prologue V1.4.0.0\nhandle 1\n", "no format line"),
            ("flexsession 2\n", "wrong format"),
            ("flexsession 1\nhandle 1\n", "no prologue"),
            ("flexsession 1\nprologue V1.4.0.0\n", "no handle"),
            (
                "flexsession 1\nprologue V1.4.0.0\nhandle 0\n",
                "zero handle",
            ),
            (
                "flexsession 1\nprologue 1.4.0.0\nhandle 1\n",
                "prologue without V",
            ),
        ] {
            assert!(Session::parse(text).is_err(), "{why}");
        }
    }

    #[test]
    fn hex_round_trips() {
        let bytes = [0u8, 1, 0x7f, 0x80, 0xff];
        assert_eq!(hex_bytes(&hex_string(&bytes)), Some(bytes.to_vec()));
        assert_eq!(hex_bytes("0"), None);
        assert_eq!(hex_bytes("zz"), None);
        assert_eq!(hex_bytes(""), None);
    }
}
