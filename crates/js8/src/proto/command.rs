//! The 32 directed commands of a JS8 frame — ids, wire texts, the behaviour subsets, and the
//! two small packers that put a command + number into a frame field.
//!
//! varicode.cpp:46-135 read as spec. The id is the 5-bit field of a Directed frame and the
//! low 7 bits of a compound-directed `packCmd`. The TEXT is what appears in the rendered
//! line, leading space included (`" SNR?"`, `" MSG TO:"`, `">"`, and `" "` for freetext);
//! `from_text` is a longest-match parse with the two parse-only aliases upstream accepts
//! (`"?"` → SNR?, `" QUERY MSGS?"` → QUERY MSGS) and the `(?=[ ]|$)` word-boundary rule of
//! `optional_cmd_pattern` (:138) for word-like commands, so `" 730"` is freetext, not `73`.
//! The subsets (autoreply / buffered / snr / checksummed, :89-107) drive B4's compose and
//! reassembly and the station's autoreply policy; they are facts, not policy.
//!
//! Numbers: `packNum` (:1154-1163) clamps to −30..=31 and adds 31 so 0 means "none";
//! `packCmd` (:1166-1206) packs an SNR-carrying command as `[1][X][6-bit num]` (X = HEARTBEAT
//! SNR) and any other as `cmd & 0x7F` — that byte rides above `NUSERGRID` in a
//! compound-directed frame's grid field.

/// The directed-command table, `id()` = the wire value.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Command {
    SnrQuery = 0,
    DitDit = 1,
    Nack = 2,
    HearingQuery = 3,
    GridQuery = 4,
    Relay = 5,
    StatusQuery = 6,
    Status = 7,
    Hearing = 8,
    Msg = 9,
    MsgTo = 10,
    Query = 11,
    QueryMsgs = 12,
    QueryCall = 13,
    Ack = 14,
    Grid = 15,
    InfoQuery = 16,
    Info = 17,
    Fb = 18,
    HwCpyQuery = 19,
    Sk = 20,
    Rr = 21,
    QslQuery = 22,
    Qsl = 23,
    Cmd = 24,
    Snr = 25,
    No = 26,
    Yes = 27,
    SeventyThree = 28,
    HeartbeatSnr = 29,
    AgnQuery = 30,
    Freetext = 31,
}

/// Parse-only spellings upstream accepts (varicode.cpp:51 `"?"`, :67 `" QUERY MSGS?"`).
const ALIASES: [(&str, Command); 2] = [
    ("?", Command::SnrQuery),
    (" QUERY MSGS?", Command::QueryMsgs),
];

impl Command {
    /// In id order.
    pub const ALL: [Command; 32] = [
        Command::SnrQuery,
        Command::DitDit,
        Command::Nack,
        Command::HearingQuery,
        Command::GridQuery,
        Command::Relay,
        Command::StatusQuery,
        Command::Status,
        Command::Hearing,
        Command::Msg,
        Command::MsgTo,
        Command::Query,
        Command::QueryMsgs,
        Command::QueryCall,
        Command::Ack,
        Command::Grid,
        Command::InfoQuery,
        Command::Info,
        Command::Fb,
        Command::HwCpyQuery,
        Command::Sk,
        Command::Rr,
        Command::QslQuery,
        Command::Qsl,
        Command::Cmd,
        Command::Snr,
        Command::No,
        Command::Yes,
        Command::SeventyThree,
        Command::HeartbeatSnr,
        Command::AgnQuery,
        Command::Freetext,
    ];

    /// The 5-bit wire value.
    pub const fn id(self) -> u8 {
        self as u8
    }

    pub fn from_id(id: u8) -> Option<Command> {
        Command::ALL.get(usize::from(id)).copied()
    }

    /// Wire text exactly as varicode.cpp:46-84, leading space included.
    pub const fn text(self) -> &'static str {
        match self {
            Command::SnrQuery => " SNR?",
            Command::DitDit => " DIT DIT",
            Command::Nack => " NACK",
            Command::HearingQuery => " HEARING?",
            Command::GridQuery => " GRID?",
            Command::Relay => ">",
            Command::StatusQuery => " STATUS?",
            Command::Status => " STATUS",
            Command::Hearing => " HEARING",
            Command::Msg => " MSG",
            Command::MsgTo => " MSG TO:",
            Command::Query => " QUERY",
            Command::QueryMsgs => " QUERY MSGS",
            Command::QueryCall => " QUERY CALL",
            Command::Ack => " ACK",
            Command::Grid => " GRID",
            Command::InfoQuery => " INFO?",
            Command::Info => " INFO",
            Command::Fb => " FB",
            Command::HwCpyQuery => " HW CPY?",
            Command::Sk => " SK",
            Command::Rr => " RR",
            Command::QslQuery => " QSL?",
            Command::Qsl => " QSL",
            Command::Cmd => " CMD",
            Command::Snr => " SNR",
            Command::No => " NO",
            Command::Yes => " YES",
            Command::SeventyThree => " 73",
            Command::HeartbeatSnr => " HEARTBEAT SNR",
            Command::AgnQuery => " AGN?",
            Command::Freetext => " ",
        }
    }

    /// Longest-match parse of a command at the START of `s`; returns the command and the rest.
    /// Word-like commands (ending in a letter or digit) must be followed by a space or the end
    /// (`optional_cmd_pattern`'s `(?=[ ]|$)`); `"?"`, `":"`, `">"` and `" "` forms need no boundary.
    pub fn from_text(s: &str) -> Option<(Command, &str)> {
        fn boundary_ok(text: &str, rest: &str) -> bool {
            let last = text.as_bytes()[text.len() - 1];
            !last.is_ascii_alphanumeric() || rest.is_empty() || rest.starts_with(' ')
        }
        let mut best: Option<(&'static str, Command)> = None;
        let candidates = Command::ALL.iter().map(|c| (c.text(), *c)).chain(ALIASES);
        for (text, cmd) in candidates {
            if let Some(rest) = s.strip_prefix(text) {
                if boundary_ok(text, rest) && best.is_none_or(|(bt, _)| text.len() > bt.len()) {
                    best = Some((text, cmd));
                }
            }
        }
        best.map(|(text, cmd)| (cmd, &s[text.len()..]))
    }

    /// Commands the station answers automatically (varicode.cpp:92 `autoreply_cmds`).
    pub const fn is_autoreply(self) -> bool {
        matches!(
            self,
            Command::SnrQuery
                | Command::Nack
                | Command::HearingQuery
                | Command::GridQuery
                | Command::StatusQuery
                | Command::Msg
                | Command::MsgTo
                | Command::Query
                | Command::QueryMsgs
                | Command::QueryCall
                | Command::Ack
                | Command::InfoQuery
                | Command::AgnQuery
        )
    }

    /// Commands whose text spans frames and waits for Last (varicode.cpp:95 `buffered_cmds`).
    pub const fn is_buffered(self) -> bool {
        matches!(
            self,
            Command::Relay
                | Command::Msg
                | Command::MsgTo
                | Command::Query
                | Command::QueryMsgs
                | Command::QueryCall
                | Command::Grid
                | Command::Cmd
        )
    }

    /// Commands that carry an SNR in the num field (varicode.cpp:98 `snr_cmds`).
    pub const fn carries_snr(self) -> bool {
        matches!(self, Command::Snr | Command::HeartbeatSnr)
    }

    /// Buffered commands that end in a 16-bit checksum (varicode.cpp:101-107; GRID's size is 0).
    pub const fn is_checksummed(self) -> bool {
        matches!(
            self,
            Command::Relay
                | Command::Msg
                | Command::MsgTo
                | Command::Query
                | Command::QueryMsgs
                | Command::QueryCall
                | Command::Cmd
        )
    }
}

/// `clamp(v, −30..=31) + 31`; `None` → 0 (varicode.cpp:1154-1163).
pub fn pack_num(v: Option<i8>) -> u8 {
    match v {
        None => 0,
        Some(n) => (n.clamp(-30, 31) + 31) as u8,
    }
}

/// Inverse of [`pack_num`]: 0 → `None`, else `b − 31`.
pub fn unpack_num(b: u8) -> Option<i8> {
    (b != 0).then(|| i8::try_from(i16::from(b) - 31).unwrap_or(i8::MAX))
}

/// Compound-directed command byte: SNR-carrying commands pack `[1][X][6-bit num]` with X =
/// HEARTBEAT SNR; any other command is `id & 0x7F` and its number is dropped.
pub fn pack_cmd(cmd: Command, num: Option<i8>) -> u8 {
    if cmd.carries_snr() {
        let x = u8::from(matches!(cmd, Command::HeartbeatSnr));
        ((0b10 | x) << 6) | (pack_num(num) & 0x3F)
    } else {
        cmd.id() & 0x7F
    }
}

/// Inverse of [`pack_cmd`]; `None` when the low 7 bits are not a command id.
pub fn unpack_cmd(b: u8) -> Option<(Command, Option<i8>)> {
    if b & 0x80 != 0 {
        let cmd = if b & 0x40 != 0 {
            Command::HeartbeatSnr
        } else {
            Command::Snr
        };
        Some((cmd, unpack_num(b & 0x3F)))
    } else {
        Command::from_id(b & 0x7F).map(|c| (c, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_texts_and_the_all_table_match_varicode_cpp() {
        // varicode.cpp:46-84 directed_cmds.
        let want: [(u8, &str); 32] = [
            (0, " SNR?"),
            (1, " DIT DIT"),
            (2, " NACK"),
            (3, " HEARING?"),
            (4, " GRID?"),
            (5, ">"),
            (6, " STATUS?"),
            (7, " STATUS"),
            (8, " HEARING"),
            (9, " MSG"),
            (10, " MSG TO:"),
            (11, " QUERY"),
            (12, " QUERY MSGS"),
            (13, " QUERY CALL"),
            (14, " ACK"),
            (15, " GRID"),
            (16, " INFO?"),
            (17, " INFO"),
            (18, " FB"),
            (19, " HW CPY?"),
            (20, " SK"),
            (21, " RR"),
            (22, " QSL?"),
            (23, " QSL"),
            (24, " CMD"),
            (25, " SNR"),
            (26, " NO"),
            (27, " YES"),
            (28, " 73"),
            (29, " HEARTBEAT SNR"),
            (30, " AGN?"),
            (31, " "),
        ];
        assert_eq!(Command::ALL.len(), 32);
        for (i, cmd) in Command::ALL.iter().enumerate() {
            assert_eq!(cmd.id() as usize, i, "{cmd:?} sits at its id in ALL");
            assert_eq!(Command::from_id(cmd.id()), Some(*cmd));
            assert_eq!((cmd.id(), cmd.text()), want[i], "{cmd:?}");
        }
        assert_eq!(Command::from_id(32), None);
        assert_eq!(Command::Freetext as u8, 31);
        assert_eq!(Command::SnrQuery.id(), 0);
    }

    #[test]
    fn from_text_takes_the_longest_match_and_honours_word_boundaries() {
        // optional_cmd_pattern (varicode.cpp:138) — word-like commands must be followed by a
        // space or the end; "?"/":"/">"/" " forms need no boundary.
        let cases: [(&str, Option<(Command, &str)>); 16] = [
            (" SNR?", Some((Command::SnrQuery, ""))),
            ("?", Some((Command::SnrQuery, ""))),
            (" SNR +10", Some((Command::Snr, " +10"))),
            (" SNR", Some((Command::Snr, ""))),
            (" SNRX", Some((Command::Freetext, "SNRX"))),
            (" QUERY MSGS?", Some((Command::QueryMsgs, ""))),
            (" QUERY MSGS", Some((Command::QueryMsgs, ""))),
            (" QUERY MSG 3", Some((Command::Query, " MSG 3"))),
            (" QUERY CALL W1AW", Some((Command::QueryCall, " W1AW"))),
            (" MSG TO:W1AW HELLO", Some((Command::MsgTo, "W1AW HELLO"))),
            (" MSG HELLO", Some((Command::Msg, " HELLO"))),
            (">W1AW HELLO", Some((Command::Relay, "W1AW HELLO"))),
            (" HEARTBEAT SNR +05", Some((Command::HeartbeatSnr, " +05"))),
            (" 73", Some((Command::SeventyThree, ""))),
            (" 730", Some((Command::Freetext, "730"))),
            ("HELLO", None),
        ];
        for (text, want) in cases {
            assert_eq!(Command::from_text(text), want, "{text:?}");
        }
        assert_eq!(Command::from_text(""), None);
        for cmd in Command::ALL {
            let (back, rest) = Command::from_text(cmd.text()).unwrap_or_else(|| panic!("{cmd:?}"));
            assert_eq!((back, rest), (cmd, ""), "{cmd:?} text parses to itself");
        }
    }

    #[test]
    fn command_sets_match_varicode_cpp() {
        // varicode.cpp:89-107.
        let ids = |f: fn(Command) -> bool| {
            Command::ALL
                .iter()
                .filter(|c| f(**c))
                .map(|c| c.id())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(Command::is_autoreply),
            vec![0, 2, 3, 4, 6, 9, 10, 11, 12, 13, 14, 16, 30]
        );
        assert_eq!(
            ids(Command::is_buffered),
            vec![5, 9, 10, 11, 12, 13, 15, 24]
        );
        assert_eq!(ids(Command::carries_snr), vec![25, 29]);
        assert_eq!(
            ids(Command::is_checksummed),
            vec![5, 9, 10, 11, 12, 13, 24],
            "GRID (15) is buffered but carries no checksum"
        );
    }

    #[test]
    fn num_packing_is_clamped_plus_31_with_zero_meaning_none() {
        // varicode.cpp:1154-1163 packNum.
        assert_eq!(pack_num(None), 0);
        assert_eq!(pack_num(Some(0)), 31);
        assert_eq!(pack_num(Some(-30)), 1);
        assert_eq!(pack_num(Some(31)), 62);
        assert_eq!(pack_num(Some(-40)), 1, "clamped");
        assert_eq!(pack_num(Some(40)), 62, "clamped");
        assert_eq!(unpack_num(0), None);
        assert_eq!(unpack_num(31), Some(0));
        assert_eq!(unpack_num(1), Some(-30));
        assert_eq!(unpack_num(62), Some(31));
        assert_eq!(
            unpack_num(63),
            Some(32),
            "6-bit max still decodes (upstream subtracts 31)"
        );
        for v in -30..=31i8 {
            assert_eq!(unpack_num(pack_num(Some(v))), Some(v));
        }
    }

    #[test]
    fn cmd_packing_for_compound_directed_frames() {
        // varicode.cpp:1166-1206 packCmd/unpackCmd: SNR commands → [1][X][6-bit num], X = HEARTBEAT SNR.
        assert_eq!(pack_cmd(Command::Snr, Some(7)), 0x80 | 38);
        assert_eq!(pack_cmd(Command::HeartbeatSnr, Some(-5)), 0xC0 | 26);
        assert_eq!(pack_cmd(Command::Snr, None), 0x80);
        assert_eq!(pack_cmd(Command::Ack, None), 14);
        assert_eq!(
            pack_cmd(Command::Ack, Some(9)),
            14,
            "non-SNR commands drop the number"
        );
        assert_eq!(unpack_cmd(0x80 | 38), Some((Command::Snr, Some(7))));
        assert_eq!(
            unpack_cmd(0xC0 | 26),
            Some((Command::HeartbeatSnr, Some(-5)))
        );
        assert_eq!(unpack_cmd(0x80), Some((Command::Snr, None)));
        assert_eq!(unpack_cmd(14), Some((Command::Ack, None)));
        assert_eq!(
            unpack_cmd(40),
            None,
            "ids above 31 without the SNR flag are not commands"
        );
        for cmd in Command::ALL {
            let n = if cmd.carries_snr() { Some(-12) } else { None };
            assert_eq!(unpack_cmd(pack_cmd(cmd, n)), Some((cmd, n)), "{cmd:?}");
        }
    }

    #[test]
    fn serde_uses_the_variant_names() {
        assert_eq!(
            serde_json::to_string(&Command::HeartbeatSnr).unwrap(),
            "\"HeartbeatSnr\""
        );
        assert_eq!(
            serde_json::from_str::<Command>("\"Relay\"").unwrap(),
            Command::Relay
        );
    }
}
