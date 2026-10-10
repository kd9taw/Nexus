//! Field Day club-event state — the policy half of the Nexus↔Nexus sync whose
//! wire half is `tempo_net::fdsync`.
//!
//! The HOST holds a [`ClubLog`]: every position's rows merged idempotently by
//! `(position id, seq)`, an append-only NDJSON event journal (one merged row
//! per line, flushed per merge — no whole-file clobber window, unlike the ADIF
//! journal's rewrite), the club dupe index, sections, per-position stats and
//! the scored total. **The club log is always reconstructible as the union of
//! the position journals** — the invariant that makes "host dies, nothing
//! lost" true: a restarted host replays its own journal, and every position
//! re-pushes its tail for free because the merge is idempotent.
//!
//! A non-host POSITION holds only the compact [`ClubMirror`]: club dupe keys
//! (the while-typing verdict), sections, counters and board rows pushed down
//! by the host. No per-QSO attribution — which is why the scoreboard server
//! runs at the host (`Engine::fd_board_snapshot` is `Some` only there).
//!
//! Dupe semantics are N3FJP's: a CROSS-POSITION dupe is a **warning, never a
//! lock** — the host keeps both rows, exports dedupe earliest-wins, and the
//! score counts unique keys (order-independent: the key set is the same
//! whichever row "wins"). A position's OWN-log dupe stays the hard refusal it
//! has always been (`FieldDayLog::log_submode_at` → false). A contest whose
//! sponsor wants duplicates REPORTED ([`DupeRule::log_dupes`]) keeps every row in
//! the export instead, the later one marked, exactly as a position's own log does.
//!
//! ⭐ **A club runs ONE ruleset — the host's contest — and everything is read
//! from it**: the exchange the rows are resolved against, the dupe key, the
//! scoring and multipliers, the Cabrillo token and headers. A position logging a
//! different contest is refused at JOIN, by name ([`ClubLog::join_refusal`]), so is
//! one on another station call than the host's outside ARRL Field Day
//! ([`ClubLog::call_refusal`]), and a contest the club log cannot run faithfully is
//! refused before a club is built at all ([`club_refusal`]). At ARRL Field Day that other
//! call is the GOTA station's, and the club's file writes its contacts under it
//! ([`MergedRow::station_call`]).
//!
//! ⚠️ **Every string a peer sends is data off the network, and the club's files are what the
//! club submits.** A JOIN under a position id no Nexus makes, or whose call is not a call sign,
//! is refused at every club, and so is one that cannot prove the position it names: the host
//! takes each position only from the laptop whose club key it pinned at that position's first
//! JOIN ([`ClubLog::key_refusal`]; it keeps the key's hash, never the key), or the laptop its
//! operator gave the position to from its own turned-away list ([`ClubLog::give`]), where each
//! laptop shows the club code its own screen shows ([`club_code`]): a name and a call are what a
//! laptop says about itself. A second laptop with a position's key (a copied settings folder) is
//! turned away while the first is connected ([`ClubLog::connected`]), and two positions with one
//! name read apart on every board ([`ClubLog::shown_names`]). A row whose call is
//! not a call sign, or whose strings could end a line of the Cabrillo or ADIF, open another, or
//! outgrow what the board and Remote carry, is kept out where it enters ([`ClubLog::merge`])
//! and listed on the host's screen until the event ends; the writers in `tempo_core` guard
//! each line too. Each kind of string has one reader (the section "One reading of every
//! string a peer sends"), and what the host shows, or says back, is that reading: one line of
//! plain text, a `?` where a character cannot be shown.
//!
//! Pure logic, no sockets — unit-testable. Engine wiring: `Engine::fd_club_*`.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use tempo_core::contest::{
    carrier, CabrilloEntrant, ContestSession, DupeRule, ExchangeSpec, FieldKind, FieldValue,
};
use tempo_core::fd_rules::FdRuleset;
use tempo_core::fieldday::{FdEvent, FieldDayLog};
use tempo_net::fdsync::{ClubState, PosReport, WireBoardRow, WireField, WireQso};

/// Why a club cannot run a contest's ruleset — a reason the screen names, never a
/// silent wrong log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClubRefusal {
    /// The exchange carries a serial number. The entry's numbers must run in ONE
    /// sequence (Sweepstakes, CQ WPX and the California QSO Party all check them), and
    /// each position issues its own, so a merged log would send the same number twice.
    Serial,
    /// The sponsor's Cabrillo template carries a transmitter-id column (CQ WW, CQ WPX),
    /// whose value must say which transmitter made each contact — a number club sync
    /// does not assign.
    TransmitterColumn,
    /// The rules this build loaded have no ruleset for the contest (a downloaded rules
    /// file that dropped one). There is nothing to run, and the Field Day fallback a
    /// position's own session takes is not one a club may take.
    NoRuleset,
}

impl ClubRefusal {
    /// The code the UI keys its sentence by (`clubSyncRefusal` in `ui/src/fdEvent.ts`).
    pub fn code(self) -> &'static str {
        match self {
            ClubRefusal::Serial => "serial",
            ClubRefusal::TransmitterColumn => "transmitter",
            ClubRefusal::NoRuleset => "unknown",
        }
    }

    /// The same reason as words, for the host's own log line and the error a refused
    /// `fd_host_start` returns.
    pub fn sentence(self) -> &'static str {
        match self {
            ClubRefusal::Serial => {
                "its serial numbers must run in one sequence for the whole entry, and every \
                 position gives out its own"
            }
            ClubRefusal::TransmitterColumn => {
                "its log must say which transmitter made each contact, and club sync does not \
                 number the transmitters"
            }
            ClubRefusal::NoRuleset => "the contest rules this Nexus loaded do not include it",
        }
    }
}

/// ⭐ **Whether the club log can run this ruleset, decided from the ruleset's own data**
/// — `None` to run it, or the reason it cannot.
///
/// Data, not a list of contest names, so a rules-file row for a new QSO party gets club
/// sync the moment it is in the table, while a contest the merged log would get wrong is
/// refused before any club is built. Everything else a ruleset can say — its exchange,
/// roles, dupe key, mode groups, multipliers and caps, bonus stations, Cabrillo token,
/// headers and location slot, and whether duplicates are reported — the club log reads
/// from the ruleset like a position's own log does. The UI mirrors the answer per
/// contest (`CLUB_SYNC_REFUSED` in `ui/src/fdEvent.ts`), and a test holds the two
/// together.
pub fn club_refusal(rs: &FdRuleset) -> Option<ClubRefusal> {
    // An arm of a `OneOf` counts: a serial is a serial in whichever slot it rides.
    fn serial(k: &FieldKind) -> bool {
        match k {
            FieldKind::Serial { .. } => true,
            FieldKind::OneOf(arms) => arms.iter().any(serial),
            _ => false,
        }
    }
    if rs.exchange.fields.iter().any(|f| serial(&f.kind)) {
        return Some(ClubRefusal::Serial);
    }
    if rs.transmitter_column {
        return Some(ClubRefusal::TransmitterColumn);
    }
    None
}

/// Is this rules-file id one of the two Field Day events — the contests whose club log
/// is pinned byte for byte to what 1.x wrote?
pub fn is_field_day_id(event_id: &str) -> bool {
    matches!(event_id.trim(), "" | "arrlfd" | "wfd")
}

/// A rules-file id as a club compares it: trimmed, and the blank picker default read as
/// ARRL Field Day — what `set_mode` builds for it.
pub fn canonical_contest(event_id: &str) -> String {
    match event_id.trim() {
        "" => "arrlfd".to_string(),
        id => id.to_string(),
    }
}

/// The sponsor's contest id for a rules-file id — what a refusal names — or, for one this
/// build's rules table does not carry, the id as [`shown_id`] shows it: it can have come off
/// the network (a JOIN's, a welcome's), and a sentence repeats it to a screen.
pub fn contest_name(event_id: &str) -> String {
    tempo_core::fd_rules::ruleset_by_id(event_id, tempo_core::fd_rules::CURRENT_RULES_YEAR)
        .map_or_else(|| shown_id(event_id), |rs| rs.contest_id.to_string())
}

/// ⭐ **The sentence for a position logging one contest at a club running another** —
/// one wording, whichever end finds it (the host at JOIN, or the position on a welcome).
/// `club` is the club's contest as named; `mine` is the position's rules-file id.
pub fn contest_mismatch(club: &str, mine: &str) -> String {
    format!(
        "this club is running {club}, and this Nexus is logging {mine}. Pick {club} as the \
         contest on the Contesting tab in Settings, then rejoin. Contacts you log meanwhile \
         stay in your own log.",
        mine = contest_name(mine),
    )
}

/// A QSO party's exchange role as an operator knows it: the side of the state line it
/// sends for, and what that side sends. The role ids are the rules file's
/// (`exchange.roles[].id`); one this build has no words for is named by its id.
fn role_words(role: &str) -> (String, &'static str) {
    match role {
        "in_state" => ("in-state".into(), "its county"),
        "out_of_state" | "w_ve" => ("out-of-state".into(), "its state or province"),
        "dx" => ("DX".into(), "DX"),
        other => (format!("\"{}\"", shown_id(other)), "another exchange"),
    }
}

/// ⭐ **The sentence for a position set up in another exchange role than the club's** — in a
/// QSO party, inside the state against outside it (or DX). Where Settings says a station is
/// decides which exchange it sends, and a club entry is one station in one place, so every
/// position sends the club's. One wording for the position's screen and the host's.
/// `contest` is the club's contest as named; `club` and `mine` are role ids.
pub fn role_mismatch(contest: &str, club: &str, mine: &str) -> String {
    let ((club_side, club_sends), (my_side, my_sends)) = (role_words(club), role_words(mine));
    format!(
        "this club sends the {club_side} {contest} exchange ({club_sends}), and this Nexus is \
         set up to send the {my_side} one ({my_sends}). Set State or province and County under \
         Your station data on the Contesting tab in Settings to the club's, then turn Field Day \
         mode off and on again: this Nexus rejoins by itself. Contacts you log meanwhile stay \
         in your own log."
    )
}

/// ⭐ **The sentence for a position on another station call than the club's** — the call
/// goes on the air, and the club's file claims every contact under the host's. One wording
/// for the position's screen and the host's. `contest` is the club's contest as named;
/// `club` and `mine` are the two calls as the screens show them.
pub fn call_mismatch(contest: &str, club: &str, mine: &str) -> String {
    format!(
        "this club is on the air as {club} and this Nexus as {mine}, and in {contest} every \
         position of a club entry sends the club's call. Set Callsign on the air under Who's \
         who at this event on the Contesting tab in Settings to {club}, then turn Field Day \
         mode off and on again: this Nexus rejoins by itself. Contacts you log meanwhile stay \
         in your own log."
    )
}

/// ⭐ **The sentence for a position whose station call is not a call sign** ([`club_call`]),
/// at every club: the club's file writes each contact under the call its position was on, so
/// one that is not a call sign is turned away rather than written. It does not repeat the
/// call, which came off the network and may not be one line of text.
pub const NOT_A_CALL_SIGN: &str = "this Nexus joined with a Callsign on the air that is not a \
     call sign (3 to 15 letters, digits and /, with at least one letter and one digit), and the \
     club writes every contact under the call it was made on. Set Callsign on the air under \
     Who's who at this event on the Contesting tab in Settings, then turn Field Day mode off and \
     on again: this Nexus rejoins by itself. Contacts you log meanwhile stay in your own log.";

/// ⭐ **The sentence for a JOIN under a position another laptop holds** ([`ClubLog::key_refusal`]):
/// the host took that position from the laptop that first joined this event as it, or the one
/// its operator gave it to ([`ClubLog::give`]), and this JOIN's club key is not that laptop's.
/// One laptop's settings copied onto another is how two come to share a position id, so it says
/// how to give this one a position of its own, and that the host can give it this one. A laptop
/// whose position was given away is sent it too, as its connection closes.
pub const POSITION_HELD: &str = "this Nexus joined as a club position that another laptop holds \
     at this event: the host takes each position only from the laptop that first joined as it, \
     or the one the host gave it to, and this laptop's club key is not that one's. If the \
     position is this laptop's, the host can give it to this laptop: Give this laptop its \
     position, on the host's Contest screen, beside the line in its club block that says it \
     turned this laptop away. If this laptop's settings came from another laptop, quit Nexus \
     here, set \"fdPositionId\" in its settings.json to \"\", and start Nexus again: it makes a \
     position of its own and rejoins by itself, with a contest log of its own that starts empty, \
     so log nothing here until then. Contacts you log meanwhile stay in your own log, and go up \
     if the host gives this laptop its position.";

/// ⭐ **The sentence for a JOIN under a position a laptop with the same club key is connected as
/// now** ([`ClubLog::connected`]): two laptops with one position id and one key are one laptop's
/// whole settings folder copied to another, and the host cannot tell them apart by either. Served,
/// they would be one position, each numbering its own contacts from 1, so one laptop's contacts
/// would never be sent and the other's would be dropped as repeats. So the second is turned away
/// while the first is connected, and told how to get a position of its own. The same laptop
/// back from a dropped link meets it until the host notices its old link is gone.
pub const POSITION_IN_USE: &str = "this Nexus joined as a club position that a laptop with the \
     same club key is connected as now. Did you copy this laptop's settings folder to another \
     laptop, or that laptop's to this one? Then the two show the same club code beside Club on \
     their Contest screens, and the host takes a position from one laptop at a time, or the \
     contacts of one would be lost. Give this laptop a position of its own: quit Nexus here, \
     delete fd_position.key from its settings folder, set \"fdPositionId\" in its settings.json \
     to \"\", and start Nexus again. It makes a position and a club key of its own and rejoins \
     by itself, with a contest log of its own that starts empty, so log nothing here until then. \
     If this laptop has only lost its link to the host for a moment, it is let in by itself \
     within about 30 seconds.";

/// ⭐ **The sentence for a JOIN with no club key a Nexus makes** ([`ClubLog::key_refusal`]):
/// every Nexus speaking this protocol makes one and sends it, so only a startup that could not
/// make one, or a peer that is not Nexus, sends none.
pub const NO_POSITION_KEY: &str =
    "this Nexus did not send a club key, which every Nexus makes for \
     itself and the host tells club positions apart by. Restart Nexus on this laptop: it makes \
     one and rejoins by itself. Contacts you log meanwhile stay in your own log.";

/// ⭐ **The sentence for a JOIN under a position id no Nexus makes** ([`club_position_id`]). It
/// does not repeat the id, which came off the network. Only a settings file edited by hand
/// holds such an id, so it says how to get a new one.
pub const NOT_A_POSITION_ID: &str = "this Nexus joined under a club position id that is not one \
     Nexus makes (eight characters, 0 to 9 and a to f). Quit Nexus on this laptop, set \
     \"fdPositionId\" in its settings.json to \"\", and start Nexus again: it makes a new id and \
     rejoins by itself. Contacts you log meanwhile stay in your own log.";

// ---------------------------------------------------------------------------
// One reading of every string a peer sends
// ---------------------------------------------------------------------------
//
// ⭐ Each kind of field a peer sends has ONE reading below, and the check and everything that
// acts on the value or prints it afterwards use that reading: the gate, the stamp, the journal,
// the club's files, the board and the host's screen. A reader with an alphabet of its own (a
// Unicode trim, a Unicode case fold) is how a string comes to compare equal in one place and
// print as something else in another. The club's files are ASCII formats, so a field that
// reaches them, or is compared, is read as ASCII.

/// The most characters a call sign has here: a compound call with a prefix and a suffix
/// (`VP2E/W9XYZ/P` is twelve) fits with room to spare.
const MAX_CALL_CHARS: usize = 15;

/// ⭐ **A call as club sync reads one**: ASCII spacing trimmed, upper-cased the ASCII way, then 3
/// to 15 letters, digits and `/`, with at least one letter and one digit (every amateur call
/// sign has both), or `None` for anything else, a character outside ASCII included.
///
/// Both calls the club's file prints come off the network: a JOIN's (a GOTA station's, as the
/// call sent: [`MergedRow::station_call`]) and every row's (the station worked). A space, a line
/// break or a control character in either would put another column, or another line, into the
/// file the club submits. So the gate ([`ClubLog::call_refusal`]), the stamp
/// ([`ClubLog::join`]) and the merge ([`ClubLog::merge`]) read a call through this one function
/// and act on the very value it returns, and the host's screen shows one through
/// [`shown_call`].
///
/// ASCII only, because that is what keeps the readers agreeing: a full-width or Cyrillic
/// letter, a zero-width character or a byte-order mark, an `ß` that `to_uppercase` makes `SS`,
/// a no-break or ideographic space that a Unicode trim removes. Each makes a string one reader
/// takes for a call and another prints as something else, and none is a call here.
pub fn club_call(raw: &str) -> Option<String> {
    let call = raw.trim_ascii().to_ascii_uppercase();
    let ok = (3..=MAX_CALL_CHARS).contains(&call.len())
        && call
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/')
        && call.bytes().any(|b| b.is_ascii_digit())
        && call.bytes().any(|b| b.is_ascii_uppercase());
    ok.then_some(call)
}

/// The characters a club code is written in: Crockford's base 32, the digits and the capitals
/// without I, L, O and U, so no two of them read alike on a screen (0 and O, 1 and I or L).
const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// ⭐ **A laptop's club code**: the first 40 bits of its club key's hash (`key_hash`, the hex the
/// host pins), as eight characters of [`CODE_ALPHABET`] in two groups of four (`7KQ2-M9XD`), or
/// `""` for no key.
///
/// Each laptop shows its own on its club line, and the host shows the code of every laptop on its
/// turned-away list beside the name and the call, which are what a laptop says about itself and
/// anybody can say. So before the host gives a laptop a position, its operator compares the code
/// on the host's screen with the one on that laptop's own. The hash is one way, so the code never
/// reveals the key; and since a code rides no board line and is on no TV, Remote page or log, a
/// peer that has not read it off one of those two screens has one chance in 2^40 that a key it
/// makes shows the same.
pub fn club_code(key_hash: &str) -> String {
    let Some(bits) = key_hash
        .get(..10)
        .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|h| u64::from_str_radix(h, 16).ok())
    else {
        return String::new();
    };
    let ch = |i: u64| CODE_ALPHABET[((bits >> (35 - 5 * i)) & 31) as usize] as char;
    let (head, tail): (String, String) = ((0..4).map(ch).collect(), (4..8).map(ch).collect());
    format!("{head}-{tail}")
}

/// ⭐ **A position id as club sync takes one**: exactly what every Nexus makes
/// (`Engine::fd_ensure_position_id`), eight characters of `0` to `9` and `a` to `f`. The id is a
/// position's identity in the club log, its rows' half of `(position id, seq)`, and it rides
/// every board line to every position, the TV and Remote, so a JOIN under any other is turned
/// away ([`NOT_A_POSITION_ID`]) before anything of it is kept.
pub fn club_position_id(raw: &str) -> bool {
    raw.len() == 8 && raw.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// ⭐ **A call that came off the network, as the host's screen shows it**: [`club_call`]'s value
/// when it is a call sign, else the text upper-cased the ASCII way with a `?` for every
/// character a call sign does not hold, so a call the gate turned away never prints as one it
/// would have served (`w9aß` shows as `W9A?`, never as the `W9ASS` a Unicode fold makes of it).
/// At most 64 characters.
pub fn shown_call(raw: &str) -> String {
    club_call(raw).unwrap_or_else(|| {
        raw.trim_ascii()
            .chars()
            .map(|c| match c.to_ascii_uppercase() {
                c @ ('A'..='Z' | '0'..='9' | '/') => c,
                _ => '?',
            })
            .take(64)
            .collect()
    })
}

/// A rules-file or exchange-role id as a sentence repeats one that came off the network: its
/// ASCII letters, digits, `_` and `-`, a `?` for anything else, at most 32 characters. Every id
/// this build knows is made of those, and one it does not know is shown as the gate read it.
fn shown_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '?'
            }
        })
        .take(32)
        .collect()
}

/// ⭐ **A position's name as club sync reads it** (the JOIN's, every presence report's, the one
/// on the host's list of turned-away positions): trimmed, and every control character, line or
/// paragraph separator and invisible or direction-changing character a `?`, at most 64
/// characters. A name is only ever shown, never compared or written into a club file, so it
/// keeps its letters in any script; what it cannot do is break the line it is shown on, or
/// read as something it is not.
pub fn club_label(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|c| if c.is_control() || unseen(c) { '?' } else { c })
        .take(64)
        .collect()
}

/// A name as two names are compared to tell positions apart ([`ClubLog::shown_names`]): lower
/// case, and every run of spacing one space, so `CW tent` and `cw  Tent` are the same name.
fn name_key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A character that changes what text reads as without being seen: a line or paragraph
/// separator, a zero-width character or joiner, a direction mark, embedding, override or
/// isolate, a soft hyphen, a byte-order mark, an interlinear annotation or a tag character.
fn unseen(c: char) -> bool {
    matches!(
        c,
        '\u{AD}'
            | '\u{61C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// The most characters a value a peer sends may hold here (each is plain ASCII, so these are
/// bytes too): far past any value a club's file holds (a county, a grid, a satellite's name, an
/// operator's call). Each reaches the board, the TV or Remote, as itself or inside a dupe key,
/// and Remote's Field Day view takes no string past 1024 bytes: one longer would blank it.
const MAX_VALUE_CHARS: usize = 64;

/// Can `s` sit inside one field of a line of the club's files: printable ASCII only, no `<`
/// (which opens an ADIF tag for a reader that does not count a value's length), and at most
/// [`MAX_VALUE_CHARS`]. Anything else (a line break, a control character, a letter from
/// another script, an invisible character) either breaks a line of an ASCII file or reads one
/// way here and another there.
fn inert(s: &str) -> bool {
    s.len() <= MAX_VALUE_CHARS && s.bytes().all(|b| (b' '..=b'~').contains(&b) && b != b'<')
}

/// An operator as club sync reads one, a row's (the club's `OPERATORS`, its ADIF `OPERATOR`)
/// and a presence report's (the board): [`inert`], then trimmed and upper-cased the ASCII way,
/// which is also how the station's own call is compared with it. `None` for anything else.
fn club_operator(raw: &str) -> Option<String> {
    inert(raw).then(|| raw.trim().to_ascii_uppercase())
}

/// ⭐ **A wire row as it enters the club log, or the sentence for keeping it out**
/// ([`ClubLog::merge`], the boundary where a peer's row enters the club log).
///
/// Its call must be a call sign ([`club_call`]), its operator one [`club_operator`] reads, and
/// every other string it carries [`inert`]: an exchange value either way, a band, a mode, a
/// satellite. All of them come off the network and most reach a line of the club's Cabrillo or
/// ADIF, so one that could end that line, open another, or read differently to another reader
/// is turned away here, before anything is keyed, journaled or written, rather than written and
/// left to the writers' own guard. The row it gives back carries the call and the operator as
/// they were read, so the journal and every printer hold that one value. No contact a position
/// logged is rewritten: a row kept out stays in that position's own log, and the host's screen
/// says so.
fn admit(q: &WireQso) -> Result<MergedRow, String> {
    let kept_out = |why: String| {
        Err(format!(
            "{why} It stays in that position's own log, and the club's file goes without it."
        ))
    };
    let Some(call) = club_call(&q.call) else {
        return kept_out(format!(
            "a contact it sent is not in the club's log: the call it was logged with is not a \
             call sign (3 to {MAX_CALL_CHARS} letters, digits and /, with at least one letter and \
             one digit)."
        ));
    };
    let slots = |fs: &[WireField]| fs.iter().all(|f| inert(&f.k) && inert(&f.d) && inert(&f.r));
    let operator = club_operator(&q.op);
    let part = [
        (
            "the exchange it copied",
            slots(&q.ex) && inert(&q.class) && inert(&q.sect),
        ),
        ("the exchange it sent", slots(&q.mex)),
        ("its band", inert(&q.band)),
        ("its mode", inert(&q.mode) && inert(&q.sub)),
        ("its operator", operator.is_some()),
        ("its satellite", inert(&q.sat)),
    ]
    .into_iter()
    .find_map(|(part, ok)| (!ok).then_some(part));
    if let Some(part) = part {
        return kept_out(format!(
            "its contact with {call} is not in the club's log: {part} holds what the club's \
             log does not take (a line break, a control character, a <, a character outside \
             plain ASCII, or more than {MAX_VALUE_CHARS} characters)."
        ));
    }
    Ok(MergedRow {
        call,
        operator: operator.unwrap_or_default(),
        ..MergedRow::from_wire(q)
    })
}

/// A field vector as it travels — the wire's and the journal's one shape.
pub fn to_wire_fields(vs: &[FieldValue]) -> Vec<WireField> {
    vs.iter()
        .map(|v| WireField {
            k: v.key.to_string(),
            d: v.domain.unwrap_or("").to_string(),
            r: v.raw.clone(),
        })
        .collect()
}

/// …and back, resolved against the exchange this club is running. Slots the
/// running exchange does not declare are dropped, which is
/// [`carrier::resolve`]'s rule and deliberately the same one: a wire triple and
/// a journal triple are the same triple.
pub fn from_wire_fields(ws: &[WireField], spec: &ExchangeSpec) -> Vec<FieldValue> {
    ws.iter()
        .filter_map(|w| carrier::resolve(&w.k, &w.d, &w.r, spec))
        .collect()
}

/// One merged club-log row — the design's reconciled shape (also what the
/// scoreboard seam's `FdBoardRow` mirrors). Serialized one-per-line into the
/// host's event journal.
///
/// ⚠️ **EVERY field added here after 1.x is `#[serde(default)]`, and the legacy
/// `class`/`section` pair became defaulted with them.** The journal replay is a
/// tolerant loop — `if let Ok(row) = serde_json::from_str::<MergedRow>(line)` —
/// which SKIPS what it cannot decode and reports nothing. One required field
/// added here and the whole pre-upgrade journal decodes as nothing: the host
/// comes up clean and empty, every position's ack watermark resets to 0, and a
/// position that has gone off the air is simply gone. Silently — the sync chip
/// reads Synced either way. `a_1x_host_journal_survives_the_v2_upgrade` is the
/// test that goes red if this rule is ever broken.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct MergedRow {
    /// Position id (8-hex, per MACHINE, not per seat).
    pub posid: String,
    /// Per-position monotonic seq — `(posid, seq)` is the row's identity.
    pub seq: u64,
    pub call: String,
    /// LEGACY (1.x): the Field Day class they sent. `ex` is synthesised from it.
    #[serde(default)]
    pub class: String,
    /// LEGACY (1.x): the ARRL/RAC section they sent. `ex` is synthesised from it.
    #[serde(default)]
    pub section: String,
    /// The exchange THEY sent, as data — synthesised from `class`/`section` when
    /// a legacy row carries none.
    #[serde(default)]
    pub ex: Vec<WireField>,
    /// The exchange the LOGGING POSITION sent on this contact. Empty on a legacy
    /// row, which falls back to the club session's — what a Field Day host means.
    #[serde(default)]
    pub mex: Vec<WireField>,
    pub band: String,
    /// Scoring class: "DIG" | "CW" | "PH".
    pub mode_class: String,
    /// Actual on-air mode behind "DIG" ("FT8", "RTTY", …); "" = n/a.
    #[serde(default)]
    pub submode: String,
    /// The logging position's clock at log time. Earliest-wins dedupe orders
    /// by this (merge order breaks ties), and NOTHING ever adjusts it.
    pub when_unix: u64,
    /// Operator at the key, stamped by the position when the row was built.
    #[serde(default)]
    pub operator: String,
    /// ⭐ **The BIRD this contact was worked through** — the satellite's own catalogue
    /// name, empty for the terrestrial contact that is almost every row.
    ///
    /// It is here because it is part of the DUPE KEY under ARRL Field Day's rule
    /// (*"Show them listed separately on the summary sheet as a separate 'band'"*), and
    /// [`dkey`](Self::dkey) is one of the four sites that must build the identical key.
    /// Without it the host would judge every position's satellite contacts as
    /// terrestrial ones on the downlink's band — the same collision the rule removes,
    /// one layer out.
    ///
    /// ⚠️ `#[serde(default)]`, on this type's own standing rule: one required field
    /// added here and the whole pre-upgrade journal decodes as nothing, silently.
    #[serde(default)]
    pub sat: String,
    /// A single-channel FM satellite rather than a linear transponder — the other half
    /// of the key's satellite input. Defaulted for the same reason, and `false` is the
    /// safe side: it under-reports ARRL's one-QSO limit instead of refusing a legal
    /// contact through a linear bird.
    #[serde(default)]
    pub sat_fm: bool,
    /// ⭐ **The station call the logging position JOINED this club on**, stamped by the host
    /// as the row merges ([`ClubLog::merge`]) and only by a club with a GOTA station
    /// ([`ClubLog::has_gota_station`]: ARRL Field Day), whose GOTA station *"must use a
    /// different callsign from the primary Field Day station"* (rule 4.1.1.1). The club's
    /// Cabrillo writes it as the call sent wherever it is not the club's
    /// ([`ClubLog::unique_log_with`]).
    ///
    /// It is on the row because a host restart replays the journal before any position has
    /// joined again, and the file exported then must still say which station made each
    /// contact. Empty on every other club's rows, on a row from a position whose JOIN sent no
    /// call (or one that is not a call sign: [`club_call`]), and on every row merged before it
    /// existed. A position that changes its call keeps the old one here until it joins again.
    ///
    /// ⚠️ `#[serde(default)]` on this type's standing rule, and skipped when empty, so every
    /// other club's journal line is the bytes it was.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub station_call: String,
}

impl MergedRow {
    /// A wire row → the stored shape (field names per the design, not the
    /// wire's short forms).
    pub fn from_wire(q: &WireQso) -> Self {
        MergedRow {
            posid: q.pos.clone(),
            seq: q.seq,
            call: q.call.clone(),
            class: q.class.clone(),
            section: q.sect.clone(),
            ex: q.ex.clone(),
            mex: q.mex.clone(),
            band: q.band.clone(),
            mode_class: q.mode.clone(),
            submode: q.sub.clone(),
            when_unix: q.when,
            operator: q.op.clone(),
            sat: q.sat.clone(),
            sat_fm: q.sat_fm,
            // The host's to fill: a wire row names no station (`ClubLog::merge`).
            station_call: String::new(),
        }
    }

    /// ⭐ **THE one synthesis, and it is called from one place.** A row that
    /// carries the legacy `class`/`section` pair and no `ex` gets `ex` built from
    /// it, resolved through the running exchange so a synthesised slot is
    /// indistinguishable from one a v2 position sent (a `SECTION` value carries
    /// the domain that matched it, and an export tag and a multiplier bucket are
    /// chosen by that domain — dropping it would make a legacy row's exchange
    /// subtly unlike a native one).
    ///
    /// Both paths that produce a `MergedRow` — the wire decoder and the journal
    /// replay — funnel through [`ClubLog::merge_row`], which is the single call
    /// site. Two synthesis sites would drift; there is one, and it is not
    /// reachable any other way.
    fn synthesize_legacy_ex(&mut self, spec: &ExchangeSpec) {
        if !self.ex.is_empty() {
            return;
        }
        if self.class.trim().is_empty() && self.section.trim().is_empty() {
            return;
        }
        self.ex = to_wire_fields(
            &["CLASS", "SECTION"]
                .iter()
                .zip([self.class.as_str(), self.section.as_str()])
                .filter_map(|(k, v)| spec.value(k, v))
                .collect::<Vec<_>>(),
        );
    }

    /// The LEGACY club dupe key — `(CALL, band, MODE CLASS)`, the shape a v1
    /// position's while-typing check reads. Band travels verbatim (the DTO and
    /// the UI compare it against `snap.radio.band`, which is lower case), which
    /// is why this is not `dkey`'s first three components.
    pub fn dupe_key(&self) -> (String, String, String) {
        (
            self.call.to_ascii_uppercase(),
            self.band.clone(),
            self.mode_class.to_ascii_uppercase(),
        )
    }

    /// The club dupe key under the ruleset's own rule, built by the SAME
    /// `DupeRule::key_of` the position's own log and the while-typing verdict use
    /// — four sites in two languages must build the identical key, so there is
    /// one builder.
    ///
    /// For Field Day the rule is `(call, band, mode class)` with both field lists
    /// empty, so this is exactly the triple `dupe_key` returned before.
    pub fn dkey(&self, rule: &DupeRule, spec: &ExchangeSpec) -> Vec<String> {
        rule.key_of(
            &self.call,
            &self.band,
            &self.mode_class,
            &from_wire_fields(&self.ex, spec),
            &from_wire_fields(&self.mex, spec),
            tempo_core::contest::SatKey {
                bird: &self.sat,
                single_channel_fm: self.sat_fm,
            },
        )
    }
}

/// What the host remembers about one position (identity + presence).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClubPosition {
    /// Friendly label ("CW tent"), from the JOIN and refreshed by every
    /// presence report (so a rename mid-event lands). Empty until a position
    /// sends one — what an unnamed position reads as is the UI's business.
    pub label: String,
    /// Station callsign from the latest JOIN, as [`club_call`] read it: empty when that JOIN
    /// named none, or one that is not a call sign.
    pub call: String,
    /// Presence, from `pos` reports (band board fodder).
    pub band: String,
    pub mode: String,
    pub operator: String,
    pub freq: u64,
    /// Merged rows from this position (raw — dupes included).
    pub qsos_raw: u64,
    /// Newest merged row's own timestamp.
    pub last_qso_unix: u64,
    /// Host clock when this position was last heard from on its socket.
    pub last_seen_unix: u64,
    /// High-water acked seq (what `welcome` reports back on a rejoin).
    pub acked: u64,
    /// The position's clock minus this host's, in ms, as its last measured report said;
    /// `None` until one does (a Nexus older than the measurement never will). The host's
    /// own board shows it ([`ClubLog::clock_ms`]); it rides no board line, so the lines
    /// every position is sent are no longer for it.
    pub clock_ms: Option<i64>,
    /// When this run of the host first heard of this position, as a count (1 for the first): of
    /// two positions with one name, the one heard of first keeps it ([`ClubLog::shown_names`]).
    pub heard: u64,
}

/// A laptop this host turned away, for the host's own screen ([`ClubLog::refused`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// The name and station call its JOIN gave.
    pub label: String,
    pub call: String,
    /// The sentence it was sent, verbatim.
    pub reason: String,
    /// Host clock at its last refused JOIN.
    pub at_unix: u64,
    /// ⭐ **The host-made number that names this entry** to the host's own Give button
    /// ([`ClubLog::give`]): kept while the laptop keeps trying, never made twice in one run of
    /// Nexus, and nothing to do with its club key or the key's hash, which never leave the host.
    pub handle: u64,
    /// ⭐ **The laptop's club code** ([`club_code`]), from the key its JOIN carried (`""` for
    /// none): the one thing on the entry the laptop cannot choose, and the same code that laptop
    /// shows on its own club line.
    pub code: String,
}

impl Refused {
    /// Was this laptop turned away because another laptop holds its position
    /// ([`POSITION_HELD`])? Only such an entry can be given its position ([`ClubLog::give`]).
    pub fn held_out(&self) -> bool {
        self.reason == POSITION_HELD
    }
}

/// The handles [`Refused`] entries are named by: one counter for the whole run of Nexus, so a
/// handle a screen still shows from before a club restarted names nothing in the new one.
static NEXT_REFUSED_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The links a served JOIN is given ([`ClubLog::open_link`]): one counter for the whole run of
/// Nexus, so a connection still closing after the club restarted closes nothing of the new one.
static NEXT_LINK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Why the host's Give button changed nothing ([`ClubLog::give`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiveRefusal {
    /// This Nexus is not hosting a club event.
    NotHosting,
    /// No laptop on the turned-away list now is held out of its position under that handle:
    /// it joined, stopped trying, was given the position already, or the club restarted.
    Stale,
}

impl GiveRefusal {
    /// The refusal as the screen's catalog keys name it.
    pub fn code(self) -> &'static str {
        match self {
            GiveRefusal::NotHosting => "notHosting",
            GiveRefusal::Stale => "stale",
        }
    }
}

/// How long the host keeps showing a refused position after its last try. A refused
/// position tries again at least every 15 s while its sync is on, so a minute without one
/// means it has stopped.
const REFUSED_SHOWN_SECS: u64 = 60;
/// The most refused positions the host keeps: the JOIN that fills this list comes off the
/// network, and a peer that cycles position ids must not grow it without bound.
const MAX_REFUSED: usize = 16;
/// The largest whole number a JSON number carries exactly to a browser (2^53 − 1).
const JSON_EXACT: u64 = (1 << 53) - 1;

/// ⭐ **A contact this host kept out of the club's log** ([`ClubLog::kept_out`]): which
/// position sent it, and what the host's screen says of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptOut {
    /// The position that sent it, and the contact's seq there: one entry per contact, however
    /// often that position sends it again.
    pub pos: String,
    pub seq: u64,
    /// That position's name and call as the host knew them, read the way the club reads them.
    pub label: String,
    pub call: String,
    /// The sentence the host's screen shows ([`admit`]'s).
    pub reason: String,
    /// Host clock when it was first kept out.
    pub at: u64,
}

/// The most kept-out contacts the host keeps, journals and lists: a peer can send any number,
/// and a list past this is a peer's flood, not a club's typing.
const MAX_KEPT_OUT: usize = 256;
/// The most of them the host's status names, the newest, beside how many it keeps: each is a
/// sentence, and the whole Field Day view Remote reads is bounded.
pub const KEPT_OUT_SHOWN: usize = 16;

/// ⭐ **A line of the host's journal that is not a merged row**, tagged by its kind
/// (`{"pin":{…}}`). An older Nexus's replay reads none as a row (a row's `posid`, `seq` and
/// `call` are required) and skips it, as it skips a torn line; this build reads each before
/// trying a row. `at` is this host's clock, and a note from a previous event ages out with
/// that event's rows ([`ClubLog::attach_journal_since`]).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalNote {
    /// A position's key hash, pinned at its first served JOIN ([`ClubLog::pin`]): the hash,
    /// never the key.
    Pin {
        pos: String,
        key_hash: String,
        at: u64,
    },
    /// A contact kept out of the club's log ([`ClubLog::kept_out`]), so a restarted host still
    /// lists it for the rest of the event.
    KeptOut(KeptOut),
    /// ⭐ A position the host's operator gave to another laptop ([`ClubLog::give`]): pinned to
    /// that laptop's key hash, and started afresh at this line. Replayed in journal order, so
    /// the last give of a position is the one a restarted host holds.
    Give {
        pos: String,
        key_hash: String,
        at: u64,
    },
}

/// The club's claimed score, part by part ([`ClubLog::score_with`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClubScore {
    pub qso_points: u32,
    /// QSO points after the power tier (equal to them for an event with none).
    pub powered: u32,
    /// The multiplier count, caps applied — `None` for a contest with no multiplier
    /// (both Field Days), never a `0` standing in for "not applicable".
    pub mults: Option<u32>,
    /// The ticked bonus menu plus the bonus stations the club worked.
    pub bonus: u32,
    pub total: u32,
}

/// The host-side merged club log. See the module header for the invariants.
#[derive(Debug, Default)]
pub struct ClubLog {
    /// The Field Day event, for the surfaces that only a Field Day club has (the
    /// spectator scoreboard). For any other contest it is the enum's default and
    /// nothing reads it: [`event_id`](Self::event_id) names the ruleset.
    pub event: FdEvent,
    /// ⭐ **The rules-file id of the ruleset this club runs** (`"arrlfd"`, `"wfd"`,
    /// `"ilqp"`) — the key every rule below is read through, as a position's own log
    /// reads its session's.
    ///
    /// `FdEvent` cannot be that key: it has two arms and `from_code` reads every other
    /// id as ARRL Field Day, which is how hosting the Illinois QSO Party built an ARRL
    /// Field Day club log in silence.
    pub event_id: String,
    /// The contest id of the contest this club is running (the ruleset's ADIF
    /// `CONTEST_ID` — `"ARRL-FIELD-DAY"`, `"IL QSO Party"`), which the JOIN gate's
    /// refusals name: whether a position may be served is a question about the
    /// CONTEST, and the refusal has to say which.
    pub contest_id: String,
    /// The operator-facing event name (beacon + welcome).
    pub event_name: String,
    rows: Vec<MergedRow>,
    /// `(posid, seq)` → merged (the idempotence index).
    ids: HashSet<(String, u64)>,
    /// Club dupe keys in FIRST-SEEN ORDER — append-only, so a down-flow delta
    /// is "everything past your cursor" and cursors never invalidate.
    ///
    /// `dupes_list` is the LEGACY `(call, band, mode class)` triple and is
    /// **index-parallel** to this one: both grow by exactly one entry per new key,
    /// so one cursor addresses both and the two can never disagree about what has
    /// been worked. Whether the legacy list reaches the wire is decided once, in
    /// [`club_state`](Self::club_state).
    dkeys_list: Vec<Vec<String>>,
    dkeys_set: HashSet<Vec<String>>,
    dupes_list: Vec<(String, String, String)>,
    /// Sections in first-seen order (same append-only contract).
    sections_list: Vec<String>,
    sections_set: HashSet<String>,
    positions: HashMap<String, ClubPosition>,
    /// Merge ARRIVAL times (host clock), pruned to the trailing hour — the
    /// per-position rate meter. In-memory only: after a host restart the rate
    /// honestly reads 0 until fresh merges arrive.
    arrivals: VecDeque<(u64, String)>,
    /// The append-only event journal. `None` = not journaling (tests).
    journal: Option<std::fs::File>,
    /// Laptops this host turned away, by the position id their JOIN named and the hash of its
    /// club key (empty for none): two laptops can name one position
    /// ([`note_refused`](Self::note_refused)).
    refused: HashMap<(String, String), Refused>,
    /// ⭐ Each position's club key, as the HASH of the key its first served JOIN of this event
    /// carried — never the key ([`pin`](Self::pin), [`key_refusal`](Self::key_refusal)) — or of
    /// the laptop the host's operator gave it to since ([`give`](Self::give)). Journaled, so a
    /// restarted host still holds every pin it made.
    pins: HashMap<String, String>,
    /// How often each position was given to another laptop, which is each served JOIN's hold
    /// on it ([`hold`](Self::hold)): a give makes every earlier hold stale.
    gives: HashMap<String, u64>,
    /// ⭐ The connections open now, by link ([`open_link`](Self::open_link)): the position each
    /// one's JOIN was served as, and that JOIN's hold. In memory only: a restarted host has none.
    links: HashMap<u64, (String, u64)>,
    /// How many positions this run has heard of ([`ClubPosition::heard`]).
    heard: u64,
    /// ⭐ The contacts this host kept out of the club's log, oldest first, for its own screen
    /// until the event ends ([`kept_out`](Self::kept_out)). Journaled, so a restart keeps them;
    /// one entry per contact, however often its position sends it again; at most
    /// [`MAX_KEPT_OUT`].
    kept_out: Vec<KeptOut>,
    /// `(positions, shown)` while the board last sent was as big as one club line carries,
    /// from [`tempo_net::fdsync::board_fit`] ([`note_board_sent`](Self::note_board_sent)).
    /// The host's warning reads it ([`board_full`](Self::board_full)), so the screen never
    /// builds a board of its own to measure.
    board_full: Option<(usize, usize)>,
}

/// How far back a host journal replay reaches. Matches the position ADIF journal's own
/// four-day expiry — see `ClubLog::attach_journal_since` for why they must agree.
const STALE_EVENT_SECS: u64 = 4 * 86_400;

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl ClubLog {
    /// A club running one of the two Field Day events.
    pub fn new(event: FdEvent, event_name: &str) -> Self {
        ClubLog {
            event,
            event_id: event.code().to_string(),
            contest_id: event.contest_id().to_string(),
            event_name: event_name.to_string(),
            ..Default::default()
        }
    }

    /// ⭐ **A club running ANY ruleset** — the constructor the host uses for every
    /// contest, Field Day included (a Field Day ruleset lands on exactly what
    /// [`new`](Self::new) builds). Whether the ruleset CAN run is [`club_refusal`]'s
    /// question, asked before this is called.
    pub fn for_ruleset(rs: &'static FdRuleset, event_name: &str) -> Self {
        if is_field_day_id(rs.event) {
            return Self::new(FdEvent::from_code(rs.event), event_name);
        }
        ClubLog {
            event_id: rs.event.to_string(),
            contest_id: rs.contest_id.to_string(),
            event_name: event_name.to_string(),
            ..Default::default()
        }
    }

    /// ⭐ **This club's log rebuilt from rows it already merged** — the journal replay's
    /// path (no arrival stamped, nothing journaled, no presence), for a read-only surface
    /// that scores the club OFF the engine lock. The spectator scoreboard clones
    /// [`rows`](Self::rows) under the lock and rebuilds here, so its dedupe, its exchange
    /// resolution and its unique log are this type's own, never a second reading of them.
    pub fn replayed(rs: &'static FdRuleset, rows: &[MergedRow]) -> Self {
        let mut club = Self::for_ruleset(rs, "");
        for row in rows {
            club.merge_row(row.clone(), 0);
        }
        club
    }

    /// The ruleset this club runs — through the RULES-FILE id, the one key that can
    /// name a contest `FdEvent` has no arm for. The fallback only keeps a default-built
    /// log answering; [`for_ruleset`](Self::for_ruleset) took the id from the table.
    pub fn ruleset(&self) -> &'static FdRuleset {
        tempo_core::fd_rules::ruleset_by_id(
            &self.event_id,
            tempo_core::fd_rules::CURRENT_RULES_YEAR,
        )
        .unwrap_or_else(|| {
            tempo_core::fd_rules::ruleset(self.event, tempo_core::fd_rules::CURRENT_RULES_YEAR)
        })
    }

    /// The Field Day event this club RUNS, or `None` for any other contest.
    ///
    /// A Field Day club's exchange stays [`contest::field_day`](tempo_core::contest::field_day)'s
    /// and its exports stay the bytes 1.x wrote (`a_1x_host_journal_survives_the_v2_upgrade`
    /// pins them), so the two places that differ ask this.
    pub fn field_day_event(&self) -> Option<FdEvent> {
        is_field_day_id(&self.event_id).then_some(self.event)
    }

    /// The exchange this club's rows are read against — the ruleset's own, and for a
    /// Field Day club the shipped Field Day exchange it has always been.
    fn exchange(&self) -> &'static ExchangeSpec {
        match self.field_day_event() {
            Some(event) => tempo_core::contest::field_day(event),
            None => self.ruleset().exchange,
        }
    }

    /// The rule this club's dupe keys are built by — the ruleset's own, which is
    /// what a position running the same contest builds its keys with.
    fn dupe_rule(&self) -> DupeRule {
        self.ruleset().dupe_rule
    }

    /// Is this club running one of the two Field Day events — the contests a v1
    /// position can enter, display and transmit?
    pub fn is_field_day(&self) -> bool {
        self.contest_id == FdEvent::ArrlFd.contest_id()
            || self.contest_id == FdEvent::WinterFd.contest_id()
    }

    /// ⭐ **§18.2 — why a joining position of protocol version `v` cannot be
    /// served, or `None` to serve it.**
    ///
    /// ⭐ **Since v3, every older position is refused, at every club.** A v3 JOIN proves the
    /// position it names ([`key_refusal`](Self::key_refusal)); an older one has no key to
    /// send, and a host that served it would let any peer take another laptop's position by
    /// claiming to be older. (Until v3 a v1 position was served at a Field Day club, and
    /// refused elsewhere because it cannot enter, show or send another contest's exchange.)
    ///
    /// The message names the versions and says what to do, because it is the only thing
    /// its reader has: a bare "incompatible" leaves an operator on a field at 0200
    /// with no idea what to do. It also says what happens to the contacts they log
    /// meanwhile, which is true — the position journals them, and its outbox is
    /// "every own row past the host's ack", so they all go up on the first join
    /// that succeeds.
    pub fn version_refusal(&self, v: u32) -> Option<String> {
        let need = tempo_net::fdsync::PROTO_VERSION;
        (v < need).then(|| {
            format!(
                "this club's host needs club sync v{need} on every laptop, so that each laptop \
                 proves which club position it is, and this Nexus speaks v{v}. Update Nexus on \
                 this laptop, then rejoin: contacts you log meanwhile stay in your own log and \
                 go up when you do."
            )
        })
    }

    /// ⭐ **Why a JOIN cannot be served as position `posid`, or `None`** — `key_hash` is the hash
    /// of the club key the JOIN carried (empty for none, or for one no Nexus makes), hashed
    /// before it reached the engine (`fdbridge::EngineClubBackend`), so no key is ever held here.
    ///
    /// A position id is no secret: every board line carries every position's. So the first
    /// JOIN this host serves under an id pins that JOIN's key ([`pin`](Self::pin)), and a later
    /// JOIN under the same id with another key is another laptop, turned away by name
    /// ([`POSITION_HELD`]); every row comes in on a connection whose JOIN this let in, as that
    /// JOIN's position. A JOIN with no key is turned away too ([`NO_POSITION_KEY`]).
    pub fn key_refusal(&self, posid: &str, key_hash: &str) -> Option<String> {
        if key_hash.is_empty() {
            return Some(NO_POSITION_KEY.to_string());
        }
        match self.pins.get(posid) {
            Some(pinned) if pinned != key_hash => Some(POSITION_HELD.to_string()),
            _ => None,
        }
    }

    /// Pin `posid` to the key hash its first served JOIN of this event carried, and journal
    /// the pin, so a restarted host holds it. A pinned position keeps its pin: a JOIN with
    /// another key never reaches here ([`key_refusal`](Self::key_refusal)).
    pub fn pin(&mut self, posid: &str, key_hash: &str, now: u64) {
        if key_hash.is_empty() || self.pins.contains_key(posid) {
            return;
        }
        self.pins.insert(posid.to_string(), key_hash.to_string());
        self.journal_note(&JournalNote::Pin {
            pos: posid.to_string(),
            key_hash: key_hash.to_string(),
            at: now,
        });
    }

    /// ⭐ **Give a position to the laptop this host's turned-away list names under `handle`** —
    /// the host's own Give button, on the entry of a laptop turned away because another laptop
    /// holds its position ([`Refused::held_out`]). [`GiveRefusal::Stale`], and nothing changes,
    /// unless that entry is on the list now ([`refused`](Self::refused)) and its laptop is still
    /// held out.
    ///
    /// The position is pinned to that laptop's key hash, so whichever laptop tries next, only it
    /// is served: no race with the laptop that held the position. Every hold on the position goes
    /// stale ([`held`](Self::held)), so the connection of the laptop that held it is told
    /// [`POSITION_HELD`] and closed within a tick, and it is turned away by name on its next try,
    /// where its own entry gives the position back the same way.
    ///
    /// ⚠️ **And the position starts afresh.** The club keys each contact by its position and that
    /// position's own count (`seq`), and tells a joining position how far it has (`acked`); each
    /// laptop counts its own contacts from 1. Left in, the contacts the other laptop sent as this
    /// position would stand in for the given laptop's: it would be told they are in and send none
    /// of those, and any it sent would merge as repeats. So they leave the club's log (they stay
    /// in the journal, and in the log of the laptop that sent them, which sends them again when it
    /// rejoins), and the given laptop sends its whole log. One journal line
    /// ([`JournalNote::Give`]), replayed in order at attach.
    pub fn give(&mut self, handle: u64, now: u64) -> Result<(), GiveRefusal> {
        let (pos, key_hash) = self
            .refused
            .iter()
            .find(|((pos, key_hash), r)| {
                r.handle == handle
                    && r.held_out()
                    && now.saturating_sub(r.at_unix) <= REFUSED_SHOWN_SECS
                    && !key_hash.is_empty()
                    && self.pins.get(pos) != Some(key_hash)
            })
            .map(|(id, _)| id.clone())
            .ok_or(GiveRefusal::Stale)?;
        self.refused.remove(&(pos.clone(), key_hash.clone()));
        self.hand_over(&pos, &key_hash);
        self.journal_note(&JournalNote::Give {
            pos,
            key_hash,
            at: now,
        });
        Ok(())
    }

    /// [`give`](Self::give)'s change, live and at replay: the pin, every hold on the position
    /// stale, and the position started afresh. The club's dupe keys and sections stay, as they
    /// must: every position's mirror holds them by cursor, and a club dupe is a warning, never a
    /// lock.
    fn hand_over(&mut self, pos: &str, key_hash: &str) {
        self.pins.insert(pos.to_string(), key_hash.to_string());
        *self.gives.entry(pos.to_string()).or_default() += 1;
        self.rows.retain(|r| r.posid != pos);
        self.ids.retain(|(p, _)| p != pos);
        self.kept_out.retain(|k| k.pos != pos);
        self.arrivals.retain(|(_, p)| p != pos);
        self.positions.remove(pos);
    }

    /// A served JOIN's hold on position `posid` (`tempo_net::fdsync::JoinAccept::hold`): how
    /// often the host gave it to another laptop before that JOIN.
    pub fn hold(&self, posid: &str) -> u64 {
        self.gives.get(posid).copied().unwrap_or(0)
    }

    /// Does a JOIN served with `hold` still hold `posid`? `Err` with [`POSITION_HELD`] once the
    /// host has given the position to another laptop ([`give`](Self::give)).
    pub fn held(&self, posid: &str, hold: u64) -> Result<(), String> {
        if self.hold(posid) == hold {
            Ok(())
        } else {
            Err(POSITION_HELD.to_string())
        }
    }

    /// A JOIN served as `posid`: the link its connection hands back when it closes
    /// ([`close_link`](Self::close_link)). Until then a laptop is [`connected`](Self::connected) as
    /// the position, under the hold it was served with.
    pub fn open_link(&mut self, posid: &str) -> u64 {
        let link = NEXT_LINK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.links
            .insert(link, (posid.to_string(), self.hold(posid)));
        link
    }

    /// The connection `link` was opened for has closed. A link this club never opened (one from
    /// before the club restarted) changes nothing.
    pub fn close_link(&mut self, link: u64) {
        self.links.remove(&link);
    }

    /// ⭐ **Is a laptop connected as `posid` now?** A connection open under the position's current
    /// hold: one whose position the host gave away since holds it no more, and is closing.
    pub fn connected(&self, posid: &str) -> bool {
        let hold = self.hold(posid);
        self.links.values().any(|(p, h)| p == posid && *h == hold)
    }

    /// List one kept-out contact, unless it is listed already, is in the club's log after all,
    /// or the list is full. `true` = it was listed (and the caller journals it).
    fn list_kept_out(&mut self, k: KeptOut) -> bool {
        let id = (k.pos.clone(), k.seq);
        if self.ids.contains(&id)
            || self.kept_out.len() >= MAX_KEPT_OUT
            || self
                .kept_out
                .iter()
                .any(|o| (o.pos.as_str(), o.seq) == (id.0.as_str(), id.1))
        {
            return false;
        }
        self.kept_out.push(k);
        true
    }

    /// ⭐ **Every contact this host kept out of the club's log**, oldest first, for the rest of
    /// the event — its screen's lasting list, through a restart. Each stays in its position's
    /// own log; the club's file goes without it.
    pub fn kept_out(&self) -> &[KeptOut] {
        &self.kept_out
    }

    /// The key hash `posid` is pinned to, if any (the host's own tests and the shell's).
    pub fn pinned(&self, posid: &str) -> Option<&str> {
        self.pins.get(posid).map(String::as_str)
    }

    /// ⭐ **Why a joining position cannot be served, or `None` to serve it** — the
    /// version rule ([`version_refusal`](Self::version_refusal)) first, then the CONTEST
    /// the position says it is logging (`contest`, a rules-file id; `""` from a
    /// position too old to send one).
    ///
    /// A club log reads every row against ONE ruleset, so a position logging another
    /// contest is refused AT JOIN and BY NAME: its rows would be resolved against the
    /// wrong exchange and keyed by the wrong rule, and a club finding that out at
    /// submission has nothing left to fix it with. Both Field Days are contests here,
    /// so a Winter Field Day position is refused by an ARRL Field Day club too.
    ///
    /// A position that names no contest is served by a Field Day club exactly as every
    /// position before the field was, and refused by any other: it cannot say what its
    /// rows mean, and the club cannot check.
    pub fn join_refusal(&self, v: u32, contest: &str) -> Option<String> {
        if let Some(msg) = self.version_refusal(v) {
            return Some(msg);
        }
        let theirs = match contest.trim() {
            "" if self.is_field_day() => return None,
            "" => {
                return Some(format!(
                    "this club is running {club}, and this Nexus is too old to say which \
                     contest it is logging. Update this Nexus and rejoin: contacts you log \
                     meanwhile stay in your own log and go up when you do.",
                    club = self.contest_id,
                ))
            }
            id => id,
        };
        if canonical_contest(theirs) == self.event_id {
            return None;
        }
        Some(contest_mismatch(&self.contest_id, theirs))
    }

    /// ⭐ **Why a joining position on another station call cannot be served, or `None` to
    /// serve it** — `club` is the host's call, the one the club's file is written under, and
    /// `theirs` the JOIN's: Settings' "Callsign on the air" on that position, which every
    /// Nexus has sent since club sync began.
    ///
    /// A club entry is one station on the air under one call: ARRL Field Day rule 6.12 ("All
    /// stations for a single entry must be operated under one callsign"), Winter Field Day
    /// ("one or many stations, all using the same callsign") and the Illinois QSO Party
    /// ("Each Mobile or Rover vehicle is considered one station and must use only one
    /// call"). A position on another call sends that call on the air while the club's file
    /// claims its contacts under the host's, so it is refused at JOIN, by name, before its
    /// first contact reaches the club.
    ///
    /// ⚠️ **Except ARRL Field Day**, whose GOTA station "must use a different callsign from
    /// the primary Field Day station" (rule 4.1.1.1) and whose contacts "may be claimed for
    /// credit by its primary Field Day operation" (4.1.1.5): a position there is served on
    /// any call sign, and its rows keep that call into the club's file
    /// ([`MergedRow::station_call`]). Winter Field Day has no GOTA station.
    ///
    /// ⚠️ **A call that is not a call sign is refused at every club, ARRL Field Day included**
    /// ([`NOT_A_CALL_SIGN`]), because that exception writes the JOIN's call into the club's
    /// file: what the gate reads is [`club_call`]'s value, and so is what [`ClubLog::join`]
    /// stamps.
    ///
    /// Compared trimmed and case-blind, and nothing more: `W9XYZ/P` is another call on the
    /// air, and none of the rules the club runs that were read for this (ARRL Field Day and
    /// VHF, Winter Field Day, and the Illinois, New York, Ohio, Tennessee and Texas QSO
    /// parties) counts a portable suffix as the same station; an ARRL VHF rover signs `/R`
    /// as an entry of its own. A call either end cannot say (empty) is served as before.
    pub fn call_refusal(&self, club: &str, theirs: &str) -> Option<String> {
        if theirs.trim_ascii().is_empty() {
            return None;
        }
        let Some(theirs) = club_call(theirs) else {
            return Some(NOT_A_CALL_SIGN.to_string());
        };
        // The host's own call, read the same way: one that is not a call sign cannot be
        // compared, and is served as an empty one always was (the host's own position is
        // refused by the line above, which says where to set it).
        let club = club_call(club)?;
        if club == theirs || self.has_gota_station() {
            return None;
        }
        Some(call_mismatch(&self.contest_id, &club, &theirs))
    }

    /// ⭐ **Does this club's contest have a GOTA station** — the one position that is on the
    /// air under another call than the host's? ARRL Field Day only: its GOTA station *"must
    /// use a different callsign from the primary Field Day station"* (rule 4.1.1.1). Winter
    /// Field Day has none, and neither has any other contest the club log runs.
    ///
    /// One predicate for the JOIN that serves such a position ([`call_refusal`](Self::call_refusal))
    /// and for the rows that keep its call ([`MergedRow::station_call`]), so the two cannot
    /// come to disagree about which club that is.
    pub fn has_gota_station(&self) -> bool {
        self.event_id == FdEvent::ArrlFd.code()
    }

    /// Open (creating if absent) the append-only journal at `path`, replaying
    /// any rows already in it — the host-restart recovery. Replayed rows are
    /// NOT re-journaled. Call once, before serving.
    pub fn attach_journal(&mut self, path: &PathBuf) -> std::io::Result<()> {
        self.attach_journal_since(path, now_unix().saturating_sub(STALE_EVENT_SECS))
    }

    /// [`Self::attach_journal`] with the cutoff exposed, so a test can age a journal
    /// without waiting days.
    ///
    /// ⚠️ THE CUTOFF EXISTS BECAUSE ITS ABSENCE SILENTLY ATE A WHOLE POSITION'S LOG, on
    /// the DEFAULT settings. The host journal is named from the event name, and the shipped
    /// default event name is EMPTY — which slugs to the literal "event", so every host that
    /// never typed a name shares ONE file, for ever. Replaying it a year later restored last
    /// year's rows AND their per-position ack watermarks; the position's own ADIF journal had
    /// self-expired at four days, so it restarted its sequence at 1; and the host then
    /// refused every new contact as a `(posid, seq)` it already held — while the sync chip
    /// read "Synced", because the merge returns the stored ack whether or not the row landed.
    /// A position worked a full event into a host that kept none of it, with nothing on
    /// screen wrong, discovered at submission.
    ///
    /// Four days is the position journal's own expiry (see `restore_field_day_if_enabled`),
    /// deliberately: the two halves of the same event must age out together or the watermark
    /// outlives the log it describes, which is exactly this bug.
    pub fn attach_journal_since(
        &mut self,
        path: &PathBuf,
        oldest_unix: u64,
    ) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                if let Ok(note) = serde_json::from_str::<JournalNote>(line) {
                    match note {
                        // A pin from a previous event pins nothing in this one, and the first
                        // pin of a position wins, as it does live.
                        JournalNote::Pin { pos, key_hash, at } if at >= oldest_unix => {
                            self.pins.entry(pos).or_insert(key_hash);
                        }
                        JournalNote::Pin { .. } => {}
                        // A previous event's are as stale as its rows; a contact a later row
                        // merged under the same id is in the club's log after all.
                        JournalNote::KeptOut(k) if k.at >= oldest_unix => {
                            self.list_kept_out(k);
                        }
                        JournalNote::KeptOut(_) => {}
                        // In journal order: the rows above it under that position were the
                        // laptop it was given away from, and leave the club's log as they did.
                        JournalNote::Give { pos, key_hash, at } if at >= oldest_unix => {
                            self.hand_over(&pos, &key_hash);
                        }
                        JournalNote::Give { .. } => {}
                    }
                    continue;
                }
                // Tolerant per line: one torn tail line (power loss mid-append)
                // must not poison the rest of the journal.
                if let Ok(row) = serde_json::from_str::<MergedRow>(line) {
                    // A row from a previous event carries a stale watermark; taking it
                    // makes this event's contacts look like duplicates.
                    //
                    // A row with NO timestamp (0) is kept. It cannot be aged, and silently
                    // dropping a contact we merely cannot date is the same class of mistake
                    // this cutoff exists to fix — an undateable row is a row somebody worked.
                    if row.when_unix != 0 && row.when_unix < oldest_unix {
                        continue;
                    }
                    self.merge_row(row, 0);
                }
            }
        }
        self.journal = Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?,
        );
        Ok(())
    }

    /// Merge one wire row at host time `now`; returns the (possibly
    /// unchanged) high-water ack for the row's position. Idempotent: a known
    /// `(posid, seq)` changes nothing — which is what makes every re-push
    /// after an outage free.
    ///
    /// A club with a GOTA station keeps on each row the call its position joined on
    /// ([`MergedRow::station_call`]); a journal replay is not a merge, and keeps the call
    /// each row was journaled with.
    ///
    /// ⭐ **THE BOUNDARY where a peer's row enters the club log** ([`admit`]): a row whose call
    /// is not a call sign, or whose strings hold anything that could end a line of the club's
    /// files or open another, is kept out (not merged, not acked, no row journaled), and the
    /// host's lasting list names it, with the position that sent it, until the event ends
    /// ([`kept_out`](Self::kept_out): journaled as a note of its own). Every other
    /// row merges with its call and operator as [`admit`] read them. `q.pos` is the position its
    /// connection JOINED as: the socket loop refuses a row naming any other
    /// (`tempo_net::fdsync::ANOTHER_POSITIONS_ROW`), so the stamp is that JOIN's call.
    pub fn merge(&mut self, q: &WireQso, now: u64) -> u64 {
        match admit(q) {
            Ok(mut row) => {
                if self.has_gota_station() {
                    row.station_call = self
                        .positions
                        .get(&q.pos)
                        .map(|p| p.call.clone())
                        .unwrap_or_default();
                }
                self.merge_row(row, now);
            }
            Err(reason) => {
                let (label, call) = self
                    .positions
                    .get(&q.pos)
                    .map(|p| (p.label.clone(), p.call.clone()))
                    .unwrap_or_default();
                let kept = KeptOut {
                    pos: q.pos.clone(),
                    seq: q.seq,
                    label,
                    call,
                    reason,
                    at: now,
                };
                if self.list_kept_out(kept.clone()) {
                    self.journal_note(&JournalNote::KeptOut(kept));
                }
            }
        }
        self.positions.get(&q.pos).map(|p| p.acked).unwrap_or(0)
    }

    /// [`merge`](Self::merge) minus the wire type; `now == 0` = a journal
    /// replay (no arrival stamped, nothing re-journaled).
    fn merge_row(&mut self, mut row: MergedRow, now: u64) -> bool {
        let id = (row.posid.clone(), row.seq);
        if row.seq == 0 || !self.ids.insert(id) {
            return false; // seq 0 is "never assigned" — refuse, don't guess
        }
        // THE synthesis point, and the only one: both producers of a MergedRow —
        // the wire decoder and the journal replay — reach the club log through
        // here, so a legacy row's exchange is reconstructed once or never.
        row.synthesize_legacy_ex(self.exchange());
        // ⭐ A satellite contact the contest gives no credit (Winter Field Day) is kept in
        // the club's rows and journal, and acked and counted on its position's board like
        // any row, but it counts for nothing in the club: no dupe key and no section, so it
        // never stands in the way of a contact that does count.
        let counts = row.sat.is_empty() || self.ruleset().satellite_credit;
        let dkey = row.dkey(&self.dupe_rule(), self.exchange());
        if counts && self.dkeys_set.insert(dkey.clone()) {
            self.dkeys_list.push(dkey);
            // Index-parallel, always built, sent only for a Field Day club.
            self.dupes_list.push(row.dupe_key());
        }
        let sect = row.section.trim().to_ascii_uppercase();
        if counts && !sect.is_empty() && self.sections_set.insert(sect.clone()) {
            self.sections_list.push(sect);
        }
        // A contact once kept out and now merged (a later build's rules let it in) is in the
        // club's log, so the host's list no longer says it is not.
        self.kept_out
            .retain(|k| (k.pos.as_str(), k.seq) != (row.posid.as_str(), row.seq));
        let pos = self.position(&row.posid);
        pos.qsos_raw += 1;
        pos.last_qso_unix = pos.last_qso_unix.max(row.when_unix);
        pos.acked = pos.acked.max(row.seq);
        if now > 0 {
            pos.last_seen_unix = now;
            self.arrivals.push_back((now, row.posid.clone()));
            self.prune_arrivals(now);
        }
        if let Some(j) = &mut self.journal {
            // One line per merged row, flushed per merge: append-only, so a
            // crash can cost at most the final line — and the position that
            // sent it re-pushes it on reconnect anyway.
            if let Ok(line) = serde_json::to_string(&row) {
                let _ = j.write_all(line.as_bytes());
                let _ = j.write_all(b"\n");
                let _ = j.flush();
            }
        }
        self.rows.push(row);
        true
    }

    fn prune_arrivals(&mut self, now: u64) {
        while self
            .arrivals
            .front()
            .is_some_and(|(t, _)| now.saturating_sub(*t) > 3600)
        {
            self.arrivals.pop_front();
        }
    }

    /// One [`JournalNote`] line, appended and flushed like a merged row's.
    fn journal_note(&mut self, note: &JournalNote) {
        if let (Some(j), Ok(line)) = (&mut self.journal, serde_json::to_string(note)) {
            let _ = j.write_all(line.as_bytes());
            let _ = j.write_all(b"\n");
            let _ = j.flush();
        }
    }

    /// A position joined (or rejoined): remember its identity, return the
    /// high-water ack it should stream past.
    ///
    /// Its call is THIS join's, as the gate read it ([`club_call`]), and nothing when the join
    /// named none or one that is not a call sign: the call a GOTA station's rows are stamped
    /// with is never an earlier join's, and never one the gate would turn away.
    pub fn join(&mut self, posid: &str, label: &str, call: &str, now: u64) -> u64 {
        // Its position's entries go, except the laptops this JOIN holds the position against:
        // each of those keeps trying, and keeps the button that can give it the position.
        self.refused
            .retain(|(id, _), r| id != posid || r.held_out());
        let pos = self.position(posid);
        let label = club_label(label);
        if !label.is_empty() {
            pos.label = label;
        }
        pos.call = club_call(call).unwrap_or_default();
        pos.last_seen_unix = now;
        pos.acked
    }

    /// A JOIN this host refused, and the sentence it sent — kept for the host's own screen
    /// while the laptop keeps trying ([`refused`](Self::refused)), under the position id its
    /// join clears and `key_hash`, its club key's hash (empty for none), so two laptops naming
    /// one position are two entries, each keeping its handle while it keeps trying. The name
    /// and call come off the network, so each is read the way the club
    /// reads it ([`club_label`], [`shown_call`]): one line, at most 64 characters, and a call
    /// the gate turned away never shown as one it would have served. The list holds at most
    /// [`MAX_REFUSED`], the oldest going first.
    pub fn note_refused(
        &mut self,
        posid: &str,
        key_hash: &str,
        label: &str,
        call: &str,
        reason: &str,
        now: u64,
    ) {
        let id = (posid.to_string(), key_hash.to_string());
        let handle = self.refused.get(&id).map_or_else(
            || NEXT_REFUSED_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            |r| r.handle,
        );
        self.refused.insert(
            id,
            Refused {
                label: club_label(label),
                call: shown_call(call),
                reason: reason.to_string(),
                at_unix: now,
                handle,
                code: club_code(key_hash),
            },
        );
        while self.refused.len() > MAX_REFUSED {
            let oldest = self
                .refused
                .iter()
                .min_by_key(|(id, r)| (r.at_unix, (*id).clone()))
                .map(|(id, _)| id.clone());
            if let Some(id) = oldest {
                self.refused.remove(&id);
            }
        }
    }

    /// The laptops refused within the last minute, by name (unnamed ones by id after) —
    /// one that has stopped trying drops off, and one that joined went at its join.
    pub fn refused(&self, now: u64) -> Vec<&Refused> {
        let mut out: Vec<(&(String, String), &Refused)> = self
            .refused
            .iter()
            .filter(|(_, r)| now.saturating_sub(r.at_unix) <= REFUSED_SHOWN_SECS)
            .collect();
        out.sort_by(|(a_id, a), (b_id, b)| {
            (a.label.is_empty(), &a.label, *a_id).cmp(&(b.label.is_empty(), &b.label, *b_id))
        });
        out.into_iter().map(|(_, r)| r).collect()
    }

    /// A position's presence report. `r.name` is its CURRENT friendly name
    /// and is applied exactly like [`Self::join`]'s label: a non-empty one
    /// wins (that is how a rename mid-event reaches the board), an empty one
    /// changes nothing — an older peer sends no name at all, and treating
    /// that as "clear it" would blank a label the join already established.
    ///
    /// The board this feeds goes to every position, the host's TV and the hosted Remote page,
    /// so a report is read the way the club reads its rows: the name by [`club_label`], the
    /// operator by [`club_operator`], a band and a mode as [`inert`] ASCII (a mode upper-cased
    /// the ASCII way), and one that is not reads as nothing.
    pub fn position_status(&mut self, posid: &str, r: &PosReport, now: u64) {
        let pos = self.position(posid);
        let name = club_label(&r.name);
        if !name.is_empty() {
            pos.label = name;
        }
        pos.band = if inert(&r.band) {
            r.band.clone()
        } else {
            String::new()
        };
        pos.mode = if inert(&r.mode) {
            r.mode.to_ascii_uppercase()
        } else {
            String::new()
        };
        pos.operator = club_operator(&r.op).unwrap_or_default();
        pos.freq = r.freq;
        // An unmeasured report is no news either: a reconnect's first report leaves
        // before its first round trip has closed. Nor is a difference no JSON number holds
        // exactly: no clock is 285,000 years off, and Remote's Field Day view refuses one.
        if let Some(ms) = r.clock_ms.filter(|ms| ms.unsigned_abs() <= JSON_EXACT) {
            pos.clock_ms = Some(ms);
        }
        pos.last_seen_unix = now;
    }

    /// A position's clock minus this host's, in ms, as its last measured report said —
    /// the host's board column. `None` for a position that has not measured one.
    pub fn clock_ms(&self, posid: &str) -> Option<i64> {
        self.positions.get(posid).and_then(|p| p.clock_ms)
    }

    /// Note what the board just sent to a position could carry
    /// ([`tempo_net::fdsync::board_fit`]): the board rides every club line, and one too
    /// long for a line is cut to the positions heard from most recently.
    pub fn note_board_sent(&mut self, board: &[WireBoardRow]) {
        let fit = tempo_net::fdsync::board_fit(board);
        self.board_full = (!fit.room).then_some((board.len(), fit.rows.len()));
    }

    /// `Some((positions, shown))` once the board the positions are sent is as big as one club
    /// line carries — no room left for one more position as long as its longest, or already
    /// cut to `shown` of `positions`. `None` before.
    pub fn board_full(&self) -> Option<(usize, usize)> {
        self.board_full
    }

    /// Stamp a position's liveness (any socket activity counts — the board's
    /// stale marks are about the LINK, not about logging cadence).
    pub fn mark_seen(&mut self, posid: &str, now: u64) {
        if let Some(pos) = self.positions.get_mut(posid) {
            pos.last_seen_unix = now;
        }
    }

    /// The append-only cursors: (dupe keys, sections) totals. The dupe cursor
    /// addresses both key lists — they are index-parallel by construction.
    pub fn counts(&self) -> (usize, usize) {
        (self.dkeys_list.len(), self.sections_list.len())
    }

    pub fn rows(&self) -> &[MergedRow] {
        &self.rows
    }

    pub fn positions(&self) -> &HashMap<String, ClubPosition> {
        &self.positions
    }

    /// `posid`'s entry, made, when this is the first the host hears of it, in the order it heard.
    fn position(&mut self, posid: &str) -> &mut ClubPosition {
        let heard = &mut self.heard;
        self.positions.entry(posid.to_string()).or_insert_with(|| {
            *heard += 1;
            ClubPosition {
                heard: *heard,
                ..Default::default()
            }
        })
    }

    /// ⭐ **Each position's name as the host shows it**, by position id: its own name, and for a
    /// position named as one the host heard of earlier (compared case-blind, a run of spaces as
    /// one), that name with the first number after it that no name shown so far has, `CW tent
    /// (2)` for the second. So no two positions read alike on any board, the TV or Remote, and a
    /// peer cannot pass as another position by naming itself as it: the one heard of first keeps
    /// its name, and the later one reads as another. Empty for a position with no name, which
    /// the screens name their own way.
    ///
    /// Letters of another script that look like these (a Cyrillic `е` for a Latin `e`) make
    /// another name, shown as it is: the hand-over never goes by a name, but by the club code
    /// ([`club_code`]).
    pub fn shown_names(&self) -> HashMap<&str, String> {
        let mut heard: Vec<(&String, &ClubPosition)> = self.positions.iter().collect();
        heard.sort_by_key(|(id, p)| (p.heard, *id));
        let mut taken: HashSet<String> = HashSet::new();
        heard
            .into_iter()
            .map(|(id, p)| {
                let mut shown = p.label.clone();
                let mut n = 1;
                while !shown.is_empty() && !taken.insert(name_key(&shown)) {
                    n += 1;
                    shown = format!("{} ({n})", p.label);
                }
                (id.as_str(), shown)
            })
            .collect()
    }

    pub fn qsos_raw(&self) -> u64 {
        self.rows.len() as u64
    }

    pub fn dupe_keys(&self) -> &[(String, String, String)] {
        &self.dupes_list
    }

    /// The same keys under the ruleset's own rule — the generalised shape.
    pub fn dkeys(&self) -> &[Vec<String>] {
        &self.dkeys_list
    }

    pub fn sections(&self) -> &[String] {
        &self.sections_list
    }

    /// Earliest-wins unique attribution: for every club dupe key, the row
    /// that logged it FIRST by the position's own clock (merge order breaks
    /// ties — an offline position's late re-push of an EARLIER contact takes
    /// the key back, which is the honest reading of "earliest").
    fn earliest_unique_indices(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.rows.len()).collect();
        order.sort_by_key(|&i| (self.rows[i].when_unix, i));
        let rule = self.dupe_rule();
        let spec = self.exchange();
        let credit = self.ruleset().satellite_credit;
        let mut seen: HashSet<Vec<String>> = HashSet::new();
        let mut keep: Vec<usize> = Vec::new();
        for i in order {
            // A satellite contact the contest gives no credit is in no file and no score.
            if !credit && !self.rows[i].sat.is_empty() {
                continue;
            }
            if seen.insert(self.rows[i].dkey(&rule, spec)) {
                keep.push(i);
            }
        }
        keep.sort_unstable(); // back to log order
        keep
    }

    /// The rows the club EXPORTS, in the order it writes them.
    ///
    /// A Field Day club writes its earliest-wins unique rows in merge order, which is
    /// what 1.x wrote and what its goldens pin. Every other club writes in the order the
    /// contacts were MADE (each position's own clock, merge order breaking a tie) — a
    /// club log is several positions' logs interleaved, and a sponsor reads it as one
    /// station's — and a contest that wants duplicates reported
    /// ([`DupeRule::log_dupes`]) keeps them all: the later of two is marked by the log
    /// itself as it is written, so it scores zero and still reaches the file, where
    /// removing it would hand the other station a not-in-log.
    fn export_indices(&self) -> Vec<usize> {
        if self.field_day_event().is_some() {
            return self.earliest_unique_indices();
        }
        let mut keep = if self.dupe_rule().log_dupes {
            (0..self.rows.len()).collect()
        } else {
            self.earliest_unique_indices()
        };
        keep.sort_by_key(|&i| (self.rows[i].when_unix, i));
        keep
    }

    /// The row each contact of [`unique_log_with`](Self::unique_log_with) is built from,
    /// in that log's own order — an index into [`rows`](Self::rows) — so a surface can say
    /// which position logged each of the club's contacts without re-deriving the dedupe.
    pub fn unique_log_rows(&self) -> Vec<usize> {
        self.export_indices()
    }

    /// The deduped (earliest-wins) club log as a [`FieldDayLog`] under the
    /// HOST's station identity (a GOTA station's rows keep their own call:
    /// [`MergedRow::station_call`]) — the one artifact both exports and the score
    /// derive from, so they can never disagree with each other.
    pub fn unique_log(&self, mycall: &str, class: &str, section: &str) -> FieldDayLog {
        self.unique_log_with(
            mycall,
            ContestSession::field_day(self.event, class, section),
        )
    }

    /// [`unique_log`](Self::unique_log) for any ruleset: the club's rows rebuilt under
    /// `session`, which the caller builds from the HOST's station data for the contest
    /// this club runs (`Engine::fd_club_session`) — the same session a position running
    /// that contest logs under, so a club file and a position's file are written by one
    /// exporter.
    pub fn unique_log_with(&self, mycall: &str, mut session: ContestSession) -> FieldDayLog {
        // ⭐ BOTH sides come from the ROW. Every merged row used to be rebuilt under
        // the HOST's class and section — the same defect `cabrillo()` carried until
        // batch 3, one layer up — which is harmless for one Field Day club and wrong
        // the moment two positions send different exchanges, which is every QSO party
        // with a mobile. `mex` is what closes it.
        //
        // The host's own session is still the FALLBACK, and only that: a legacy row
        // carries no `mex`, and what a 1.x host meant by its absence is "the club's
        // sent exchange", which is exactly right for Field Day.
        let spec = self.exchange();
        // ⭐ A CLUB log IS a multi-operator entry, and this is the ONE place that is
        // true by construction: these rows were worked at several positions under one
        // callsign, which is exactly what `CATEGORY-OPERATOR: MULTI-OP` declares. The
        // session's default is `SINGLE-OP` because a lone operator is the default
        // user; a host merging positions is not that operator, and it says so here
        // rather than leaving the header to a picker nobody at a field site touched.
        session.entry_category = tempo_core::contest::OperatorCategory::MultiOp;
        // ⭐ WHO LOGGED EACH CONTACT, for every club but a Field Day one. The sponsor's
        // `OPERATORS` header is everyone the rows name (ILQP: *"make sure entry class,
        // personal information, call sign, station location, and club affiliation are
        // correctly shown"*), and the club file is the one the sponsor receives. A Field
        // Day club's rows name nobody, because its exports are pinned to the bytes 1.x
        // wrote, which carried no `OPERATOR`.
        let names_operators = self.field_day_event().is_none();
        let station = club_call(mycall).unwrap_or_default();
        let gota = self.has_gota_station();
        let mut log = FieldDayLog::new(mycall, session, "");
        for i in self.export_indices() {
            let r = &self.rows[i];
            log.band = r.band.clone();
            // ⭐ A GOTA station's row is sent under its own call, which its QSO line carries;
            // every other row is the club's, written under `mycall` exactly as before.
            log.station_call = if gota && r.station_call != station {
                r.station_call.clone()
            } else {
                String::new()
            };
            if names_operators {
                // A position with nobody named at the key sends the station's own call
                // (`Engine::fd_sync_outbox`, for the band board), and a row's operator is
                // never the station's own call (`LoggedQso::operator`) — so that call reads
                // back as nobody, not as an operator somebody named.
                let op = r.operator.trim().to_ascii_uppercase();
                log.operator = if op == station { String::new() } else { op };
            }
            let rx = from_wire_fields(&r.ex, spec);
            let tx = if r.mex.is_empty() {
                log.session.my_exchange.clone()
            } else {
                from_wire_fields(&r.mex, spec)
            };
            // Never refused: the indices are key-unique, or the ruleset logs its
            // duplicates and marks them.
            log.log_exchange_at(&r.call, rx, tx, &r.mode_class, &r.submode, 0, r.when_unix);
        }
        log
    }

    /// ⭐ **FIRING SITE 3 of 3 (§6.3): the club host, over the MERGED rows' own
    /// `mex`.**
    ///
    /// ⚠️ **The rule is per-LOG, so no per-position guard can see this one.** Two
    /// positions configured with different Sweepstakes checks under one club callsign
    /// each send a value that is perfectly constant *within that position's log*, and
    /// the entry the host submits is invalid. The host is the only place both halves
    /// are in the same collection, which is why this site exists and is named here
    /// rather than discovered when a club's entry is rejected.
    ///
    /// It runs the SAME scan the position's own export runs
    /// ([`FieldDayLog::constant_sent_scan`]) over the merged log, so the two cannot
    /// come to disagree about what "constant" means.
    pub fn constant_sent_mismatch(
        &self,
        mycall: &str,
        class: &str,
        section: &str,
    ) -> Option<tempo_core::contest::ConstantSentMismatch> {
        self.unique_log(mycall, class, section).constant_sent_scan()
    }

    /// Unique (scoring) rows count.
    pub fn qsos_unique(&self) -> u64 {
        self.dkeys_list.len() as u64
    }

    /// Club score under the HOST's power multiplier + claimed bonuses (the
    /// design's flagged judgment call: per-position multipliers are ignored —
    /// an ARRL entry is one station, one power tier; the operator saw and
    /// accepted this). Returns `(qso_pts, powered, bonus, total)`.
    pub fn scored(
        &self,
        mycall: &str,
        class: &str,
        section: &str,
        power_mult: u32,
        bonuses: &[String],
    ) -> (u32, u32, u32, u32) {
        let s = self.score_with(
            mycall,
            ContestSession::field_day(self.event, class, section),
            power_mult,
            bonuses,
            &[],
        );
        (s.qso_points, s.powered, s.bonus, s.total)
    }

    /// ⭐ **The club's claimed score under its own ruleset** — the arithmetic a position's
    /// own log shows (`Engine::fd_score`, `field_day_display`), over the club's rows: QSO
    /// points, the power tier where the event has one, the multipliers with their caps,
    /// then the ticked bonus menu plus the bonus stations the club worked (ILQP's two club
    /// calls, *"added to the final score"*, once for the whole entry however many
    /// positions worked them).
    ///
    /// For a Field Day club this is exactly [`scored`](Self::scored): neither event has a
    /// multiplier or a bonus station, so the total is the powered points plus the menu —
    /// except that Winter Field Day scores by objectives, so its total is the host's ticked
    /// `objectives` applied the sponsor's way (`FdRuleset::claimed_total`).
    pub fn score_with(
        &self,
        mycall: &str,
        session: ContestSession,
        power_mult: u32,
        bonuses: &[String],
        objectives: &[String],
    ) -> ClubScore {
        let rs = self.ruleset();
        let log = self.unique_log_with(mycall, session);
        let (qso_points, powered, mults, scored) = rs.scoring.score(log.score_rows(), power_mult);
        let bonus = rs.bonus_points(bonuses) + log.bonus_station_points();
        ClubScore {
            qso_points,
            powered,
            mults,
            bonus,
            total: rs.claimed_total(qso_points, scored, bonus, objectives),
        }
    }

    /// The band-board rows (one per known position), stalest-last untouched —
    /// display order is the UI's business. `age` is seconds since last heard.
    pub fn board_rows(&self, now: u64) -> Vec<WireBoardRow> {
        // Per-position uniq + rate in one pass each.
        let mut uniq: HashMap<&str, u64> = HashMap::new();
        for i in self.earliest_unique_indices() {
            *uniq.entry(self.rows[i].posid.as_str()).or_insert(0) += 1;
        }
        let mut rate: HashMap<&str, u64> = HashMap::new();
        for (t, p) in &self.arrivals {
            if now.saturating_sub(*t) <= 3600 {
                *rate.entry(p.as_str()).or_insert(0) += 1;
            }
        }
        let shown = self.shown_names();
        let mut ids: Vec<&String> = self.positions.keys().collect();
        ids.sort();
        ids.iter()
            .map(|id| {
                let p = &self.positions[*id];
                WireBoardRow {
                    pos: (*id).clone(),
                    // The position's OWN name, and empty when it has none — the raw
                    // position id used to be the fallback, which put "9a85f060" on the
                    // club board where an operator expects a tent name. That id is
                    // internal plumbing (it exists so two positions' contacts can never
                    // collide) and is not something to show anybody. The UI decides what
                    // an unnamed position reads as, because that fallback is prose and
                    // prose belongs in the catalogs, not in a Rust string literal. Named as
                    // the host shows it: two positions with one name read apart.
                    name: shown.get(id.as_str()).cloned().unwrap_or_default(),
                    band: p.band.clone(),
                    mode: p.mode.clone(),
                    op: p.operator.clone(),
                    qsos: p.qsos_raw,
                    uniq: uniq.get(id.as_str()).copied().unwrap_or(0),
                    rate: rate.get(id.as_str()).copied().unwrap_or(0),
                    age: now.saturating_sub(p.last_seen_unix.min(now)),
                }
            })
            .collect()
    }

    /// Down-flow state past the given cursors, with the current board.
    /// `(0, 0)` = the full join snapshot. Score fields come from the host's
    /// settings, passed in by the engine.
    pub fn club_state(
        &self,
        dupes_from: usize,
        sections_from: usize,
        score: u32,
        now: u64,
    ) -> ClubState {
        ClubState {
            reset: false, // the wire layer stamps the snap's first chunk
            // The legacy triple ships ONLY for a Field Day club: for any other
            // contest it is not the dupe rule, and a v1 position given one would
            // show a WRONG while-typing warning rather than none. Empty is the
            // honest failure. `dkeys` always ships — a v2 position reads that.
            dupes: if self.is_field_day() {
                self.dupes_list.get(dupes_from..).unwrap_or(&[]).to_vec()
            } else {
                Vec::new()
            },
            dkeys: self.dkeys_list.get(dupes_from..).unwrap_or(&[]).to_vec(),
            sections: self
                .sections_list
                .get(sections_from..)
                .unwrap_or(&[])
                .to_vec(),
            score,
            qsos: self.qsos_raw(),
            board: self.board_rows(now),
        }
    }

    /// Club ADIF export, deduped earliest-wins (the submittable artifact).
    pub fn export_adif(&self, mycall: &str, class: &str, section: &str) -> String {
        self.unique_log(mycall, class, section).adif()
    }

    /// [`export_adif`](Self::export_adif) for any ruleset, under the club session
    /// (see [`unique_log_with`](Self::unique_log_with)).
    pub fn export_adif_with(&self, mycall: &str, session: ContestSession) -> String {
        self.unique_log_with(mycall, session).adif()
    }

    /// [`export_cabrillo`](Self::export_cabrillo) for any ruleset: the club session's
    /// contest token and headers, and the entrant's own lines (`NAME`, `EMAIL`, `CLUB`,
    /// `ENTRY-CLASS`, `OPERATORS`), each written only where the ruleset lists it — the
    /// position's own exporter, over the club's rows.
    ///
    /// ⭐ **A Field Day club's `OPERATORS` are read off its rows here.** Those rows name nobody
    /// in the rebuilt log ([`unique_log_with`](Self::unique_log_with): the exports are pinned
    /// to the bytes 1.x wrote), yet the club's file is the one the sponsor receives, and
    /// Winter Field Day's lists OPERATORS: everyone the positions said was at the key, then
    /// the operators typed. A row naming the station's own call names nobody, as it does
    /// there. ARRL Field Day's rules list no OPERATORS, and its file is unchanged.
    pub fn export_cabrillo_with(
        &self,
        mycall: &str,
        session: ContestSession,
        entrant: &CabrilloEntrant,
    ) -> Result<String, String> {
        let mut entrant = entrant.clone();
        if self.field_day_event().is_some() {
            let station = club_call(mycall).unwrap_or_default();
            let mut named: Vec<String> = Vec::new();
            for i in self.export_indices() {
                let op = self.rows[i].operator.trim().to_ascii_uppercase();
                if !op.is_empty() && op != station && !named.contains(&op) {
                    named.push(op);
                }
            }
            named.push(entrant.operators.clone());
            entrant.operators = named.join(" ");
        }
        self.unique_log_with(mycall, session)
            .cabrillo_with(0, &entrant)
    }

    /// Club Cabrillo export, deduped earliest-wins.
    ///
    /// ⚠️ THE DIAL ARGUMENT IS A LAST RESORT AND MUST NOT LOOK LIKE A BAND. It used to be a
    /// hardcoded `14_000`, which every row with an unmapped band silently borrowed — so a
    /// 23 cm club contact exported as 20 m. A club log is multi-band by definition and the
    /// host has no single dial to speak for it, so there is no honest frequency to pass:
    /// `0` reaches the exporter only for a row that recorded no band at all, and a zero in
    /// that field reads as missing rather than as a confident wrong answer. Every row that
    /// HAS a band now carries its own, mapped or verbatim.
    ///
    /// `Err` carries the reason the log is not one submittable entry — today only a
    /// mode-split contest whose rows span both modes (§6.2), which neither Field Day
    /// event is. The message is the operator's, so it is passed up rather than
    /// collapsed into a missing file.
    pub fn export_cabrillo(
        &self,
        mycall: &str,
        class: &str,
        section: &str,
    ) -> Result<String, String> {
        self.unique_log(mycall, class, section).cabrillo(0)
    }
}

// ---------------------------------------------------------------------------
// Position side
// ---------------------------------------------------------------------------

/// The compact club state a non-host position holds — everything the host
/// pushed down, nothing more (no per-QSO attribution; the scoreboard runs at
/// the host for exactly that reason).
#[derive(Debug, Default, Clone)]
pub struct ClubMirror {
    pub event: String,
    pub host_call: String,
    /// LEGACY club dupe keys — the while-typing verdict unions these with own
    /// log. EMPTY when the host is running anything but Field Day: the triple is
    /// not that contest's dupe rule, and no club warning is honest where a wrong
    /// one is not.
    pub dupes: HashSet<(String, String, String)>,
    /// The same keys under the ruleset's own rule — what a v2 position reads.
    pub dkeys: HashSet<Vec<String>>,
    pub sections: HashSet<String>,
    pub score: u32,
    pub qsos: u64,
    pub board: Vec<WireBoardRow>,
    /// Host's high-water ack for OUR rows (queued = own max seq − this).
    pub acked: u64,
    /// Link liveness + when it went down (the Offline chip's `since`).
    pub connected: bool,
    pub down_since_unix: u64,
    /// This PC's clock minus the host's, whole seconds: the welcome's until the first timed
    /// round trip closes, measured from then on ([`Self::clock`]). The club line shows it
    /// from 2 s and warns past 30 s; NOTHING ever adjusts a clock by it.
    pub skew_secs: i64,
    /// The round-trip measurement behind [`Self::skew_secs`], this session's only.
    pub clock: crate::clubclock::ClubClock,
    /// The last host `error` line, verbatim (version refusal etc.).
    pub last_error: Option<String>,
}

impl ClubMirror {
    /// Apply one `snap`/`club` line. `reset` (the snap's first chunk) clears
    /// the lists first; every chunk unions lists and overwrites scalars.
    pub fn apply(&mut self, st: &ClubState) {
        if st.reset {
            self.dupes.clear();
            self.dkeys.clear();
            self.sections.clear();
        }
        for k in &st.dupes {
            self.dupes.insert(k.clone());
        }
        for k in &st.dkeys {
            self.dkeys.insert(k.clone());
        }
        for s in &st.sections {
            self.sections.insert(s.clone());
        }
        self.score = st.score;
        self.qsos = st.qsos;
        if !st.board.is_empty() || st.reset {
            self.board = st.board.clone();
        }
    }

    pub fn on_welcome(&mut self, acked: u64, event: &str, host_call: &str, skew_secs: i64) {
        self.acked = acked;
        self.event = event.to_string();
        self.host_call = host_call.to_string();
        self.skew_secs = skew_secs;
        // A new session may be with another host, so the measurement starts again; until
        // its first round trip the welcome's whole seconds stand.
        self.clock = Default::default();
        self.last_error = None;
    }

    /// One answered round trip of the timed ping (`fdsync::ClockSample`). Updates what the
    /// club line says, and nothing else.
    pub fn on_clock(&mut self, sample: tempo_net::fdsync::ClockSample) {
        self.clock.add(sample);
        if let Some(secs) = self.clock.skew_secs() {
            self.skew_secs = secs;
        }
    }

    pub fn on_link(&mut self, connected: bool, now: u64) {
        if self.connected && !connected {
            self.down_since_unix = now;
        }
        self.connected = connected;
    }
}

/// The sync chip's four honest states — DERIVED from (link liveness, queued =
/// own max seq − host ack), so it can never disagree with the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// No hosting, no join address — the feature is off.
    Disabled,
    /// Link down; `queued` rows wait in the journal, since `since` (unix).
    Offline {
        queued: u64,
        since: u64,
    },
    /// Link up, rows still streaming.
    Behind {
        queued: u64,
    },
    Synced,
}

impl SyncState {
    /// The DTO string the UI switches on.
    pub fn code(&self) -> &'static str {
        match self {
            SyncState::Disabled => "disabled",
            SyncState::Offline { .. } => "offline",
            SyncState::Behind { .. } => "behind",
            SyncState::Synced => "synced",
        }
    }

    /// Derive from the inputs (the single computation, used by engine + tests).
    pub fn derive(enabled: bool, connected: bool, queued: u64, down_since: u64) -> SyncState {
        if !enabled {
            SyncState::Disabled
        } else if !connected {
            SyncState::Offline {
                queued,
                since: down_since,
            }
        } else if queued > 0 {
            SyncState::Behind { queued }
        } else {
            SyncState::Synced
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ **The host's list of the positions it turned away**: one entry per position with the
    /// sentence it was sent last, gone when that position joins, dropped a minute after its
    /// last try, and never more than sixteen, the oldest going first.
    #[test]
    fn the_host_lists_each_refused_position_while_it_keeps_trying() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST");
        let shown = |c: &ClubLog, now: u64| -> Vec<(String, String, String)> {
            c.refused(now)
                .into_iter()
                .map(|r| (r.label.clone(), r.call.clone(), r.reason.clone()))
                .collect()
        };
        club.note_refused("aaaa0001", "", "SSB tent", "w9xyz", "first", 100);
        club.note_refused("aaaa0001", "", "SSB tent", "w9xyz", "second", 110);
        assert_eq!(
            shown(&club, 120),
            vec![("SSB tent".into(), "W9XYZ".into(), "second".into())],
            "one entry, the latest sentence, the call as a call"
        );
        assert_eq!(
            shown(&club, 170).len(),
            1,
            "kept a minute after the last try"
        );
        assert!(shown(&club, 171).is_empty(), "and not a second longer");
        club.note_refused("bbbb0002", "", "CW tent", "k9abc", "refused", 200);
        club.join("bbbb0002", "CW tent", "K9ABC", 201);
        assert!(
            shown(&club, 202).is_empty(),
            "a position that joins leaves the list"
        );
        for i in 0..20u64 {
            club.note_refused(&format!("c{i:07}"), "", "", "K9ZZZ", "refused", 300 + i);
        }
        let ids: Vec<&String> = club.refused.keys().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), 16, "never more than sixteen");
        assert!(
            (4..20).all(|i| club
                .refused
                .contains_key(&(format!("c{i:07}"), String::new()))),
            "the four oldest went first: {ids:?}"
        );
    }

    /// One presence report, dial fixed — these tests are about the name.
    fn report(name: &str, band: &str, mode: &str, op: &str) -> PosReport {
        PosReport {
            band: band.into(),
            mode: mode.into(),
            op: op.into(),
            freq: 14_032_100,
            name: name.into(),
            clock_ms: None,
        }
    }

    fn wq(
        pos: &str,
        seq: u64,
        call: &str,
        band: &str,
        mode: &str,
        sect: &str,
        when: u64,
    ) -> WireQso {
        WireQso {
            pos: pos.into(),
            seq,
            call: call.into(),
            // A v1 position's shape exactly: the legacy pair, and no `ex`/`mex`.
            // Every test below that uses this helper is therefore also a test that
            // a v1 position's rows still reach a v2 host's club log intact.
            class: "2A".into(),
            sect: sect.into(),
            ex: vec![],
            mex: vec![],
            band: band.into(),
            mode: mode.into(),
            sub: if mode == "DIG" {
                "FT8".into()
            } else {
                String::new()
            },
            when,
            op: "OP".into(),
            sat: String::new(),
            sat_fm: false,
        }
    }

    #[test]
    fn merge_is_idempotent_and_orders_dont_matter() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        assert_eq!(
            club.merge(&wq("aaaa", 2, "W1AW", "20m", "DIG", "CT", 200), 10),
            2
        );
        // Out of order: seq 1 arrives after 2 (a reconnect race) — merged fine,
        // ack stays the high-water.
        assert_eq!(
            club.merge(&wq("aaaa", 1, "K1ABC", "20m", "CW", "EMA", 100), 11),
            2
        );
        let before = club.qsos_raw();
        // Idempotence: the same (pos, seq) again changes NOTHING…
        assert_eq!(
            club.merge(&wq("aaaa", 2, "W1AW", "20m", "DIG", "CT", 200), 12),
            2
        );
        assert_eq!(club.qsos_raw(), before, "re-push merged nothing");
        // …POSITIVE CONTROL: a new seq from the same position DOES merge.
        assert_eq!(
            club.merge(&wq("aaaa", 3, "N0XYZ", "40m", "PH", "MN", 300), 13),
            3
        );
        assert_eq!(club.qsos_raw(), before + 1);
        // seq 0 ("never assigned") is refused, not guessed at.
        club.merge(&wq("aaaa", 0, "BAD0", "20m", "CW", "CT", 400), 14);
        assert_eq!(club.qsos_raw(), before + 1);
    }

    #[test]
    fn cross_position_dupe_merges_but_scores_once() {
        // N3FJP semantics: both rows kept (a warning at the position, never a
        // lock), the key counts ONCE for score/sections, exports dedupe.
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "DIG", "CT", 100), 10);
        club.merge(&wq("bbbb", 1, "W1AW", "20m", "DIG", "CT", 200), 11); // the dupe
        club.merge(&wq("bbbb", 2, "W1AW", "40m", "DIG", "CT", 300), 12); // NOT a dupe (new band)
        assert_eq!(club.qsos_raw(), 3, "all rows kept");
        assert_eq!(club.qsos_unique(), 2, "the same-band re-work counts once");
        let (qso_pts, powered, bonus, total) = club.scored("W9ABC", "3A", "WI", 2, &[]);
        assert_eq!(qso_pts, 4, "two unique DIG contacts × 2 pts");
        assert_eq!(powered, 8);
        assert_eq!((bonus, total), (0, 8));
        assert_eq!(club.sections(), ["CT"], "one section however many rows");
    }

    #[test]
    fn journal_replay_reproduces_identical_state() {
        let dir = std::env::temp_dir().join(format!("fdevent-j-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("fd_event_test.jsonl");

        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.attach_journal_since(&path, 0).expect("journal opens");
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "DIG", "CT", 100), 10);
        club.merge(&wq("bbbb", 1, "K1ABC", "40m", "CW", "EMA", 200), 11);
        club.merge(&wq("aaaa", 2, "N0XYZ", "20m", "PH", "MN", 300), 12);

        // The host restarts: a fresh ClubLog replays the same journal.
        let mut reborn = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        reborn
            .attach_journal_since(&path, 0)
            .expect("journal replays");
        assert_eq!(reborn.rows(), club.rows(), "identical rows after replay");
        assert_eq!(reborn.counts(), club.counts());
        assert_eq!(
            reborn.join("aaaa", "", "", 20),
            2,
            "acked high-water survives the restart — the position streams only its tail"
        );
        // Replay did NOT re-journal: the file has exactly the 3 lines.
        let lines = std::fs::read_to_string(&path).unwrap();
        assert_eq!(lines.lines().count(), 3);

        // A torn tail line (power loss mid-append) poisons nothing.
        std::fs::write(
            &path,
            format!(
                "{lines}{}",
                &serde_json::to_string(&club.rows()[0]).unwrap()[..20]
            ),
        )
        .unwrap();
        let mut torn = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        torn.attach_journal_since(&path, 0)
            .expect("torn journal still opens");
        assert_eq!(
            torn.qsos_raw(),
            3,
            "the 3 whole lines restored, the torn one skipped"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exports_dedupe_earliest_wins_by_the_position_clock() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        // The LATER-MERGED row is the EARLIER contact (an offline position's
        // re-push): earliest-wins must keep IT, not the first-merged one.
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "DIG", "CT", 500), 10);
        let mut earlier = wq("bbbb", 1, "W1AW", "20m", "DIG", "CT", 100);
        earlier.class = "5A".into(); // distinguishable in the export
        club.merge(&earlier, 11);
        let cab = club
            .export_cabrillo("W9ABC", "3A", "WI")
            .expect("a single-mode event exports one entry");
        assert_eq!(cab.matches("QSO:").count(), 1, "deduped to one line");
        assert!(
            cab.contains("W1AW 5A CT"),
            "the earlier contact won, not the first-merged: {cab}"
        );
        let adif = club.export_adif("W9ABC", "3A", "WI");
        assert!(adif.contains("<CLASS:2>5A"), "ADIF agrees: {adif}");
        // Control: a non-dupe key exports alongside.
        club.merge(&wq("aaaa", 2, "K1ABC", "40m", "CW", "EMA", 700), 12);
        assert_eq!(
            club.export_cabrillo("W9ABC", "3A", "WI")
                .expect("a single-mode event exports one entry")
                .matches("QSO:")
                .count(),
            2
        );
    }

    #[test]
    fn board_rows_carry_presence_uniq_and_rate() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.join("aaaa", "CW tent", "KD9TAW", 1000);
        // An empty name = the older-peer shape: a presence report that carries
        // none, which must leave the join's label alone (its own test is below).
        club.position_status("aaaa", &report("", "20m", "cw", "op1"), 1000);
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "CW", "CT", 900), 1000);
        club.merge(&wq("bbbb", 1, "W1AW", "20m", "CW", "CT", 950), 1010); // cross-pos dupe
        let rows = club.board_rows(1015);
        assert_eq!(rows.len(), 2);
        let a = rows.iter().find(|r| r.pos == "aaaa").unwrap();
        assert_eq!(
            (a.name.as_str(), a.band.as_str(), a.mode.as_str()),
            ("CW tent", "20m", "CW")
        );
        assert_eq!(
            (a.qsos, a.uniq, a.rate),
            (1, 1, 1),
            "the earliest holds the unique"
        );
        let b = rows.iter().find(|r| r.pos == "bbbb").unwrap();
        // An unnamed position sends NO name — it does not send its id as one. The id
        // is on the row separately, for keying and nothing else; what an operator reads
        // in place of a missing name is prose, and prose lives in the catalogs. The id
        // WAS the fallback, and it put "9a85f060" on the club board where a tent name
        // belongs (operator report, 2026-08-30).
        assert!(
            b.name.is_empty(),
            "an unnamed position sends no name, not its id: {:?}",
            b.name
        );
        assert_eq!(b.pos, "bbbb", "…while the id itself still rides the row");
        assert_eq!((b.qsos, b.uniq), (1, 0), "the dupe merges but scores 0");
        assert_eq!(a.age, 15, "age = seconds since last heard");
        // Rate window: an arrival >1 h old stops counting.
        let rows = club.board_rows(1000 + 3700);
        assert_eq!(rows.iter().find(|r| r.pos == "aaaa").unwrap().rate, 0);
    }

    /// The host keeps each position's clock as its last MEASURED report said. An unmeasured
    /// report (a reconnect's first, or every one from an older Nexus) leaves it alone, a
    /// position that never measured has none, and a new measurement replaces the old.
    #[test]
    fn the_host_keeps_each_positions_last_measured_clock() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.join("aaaa", "CW tent", "KD9TAW", 1000);
        club.join("bbbb", "SSB tent", "KD9TAW", 1000);
        let clocked = |clock_ms| PosReport {
            clock_ms,
            ..report("", "20m", "cw", "op1")
        };
        club.position_status("aaaa", &clocked(Some(-3_000)), 1001);
        club.position_status("aaaa", &clocked(None), 1002);
        club.position_status("bbbb", &report("", "40m", "ph", "op2"), 1002);
        assert_eq!(club.clock_ms("aaaa"), Some(-3_000));
        assert_eq!(club.clock_ms("bbbb"), None);
        assert_eq!(
            club.clock_ms("cccc"),
            None,
            "a position the host never heard of"
        );
        club.position_status("aaaa", &clocked(Some(400)), 1004);
        assert_eq!(club.clock_ms("aaaa"), Some(400));
        // A difference no JSON number holds exactly is no news either: Remote's Field Day
        // view refuses one, and it would blank the host's whole view. The largest it holds is
        // taken as measured.
        club.position_status("aaaa", &clocked(Some(i64::MAX)), 1005);
        club.position_status("aaaa", &clocked(Some(-(1 << 53))), 1006);
        assert_eq!(club.clock_ms("aaaa"), Some(400));
        club.position_status("aaaa", &clocked(Some((1 << 53) - 1)), 1007);
        assert_eq!(club.clock_ms("aaaa"), Some((1 << 53) - 1));
    }

    #[test]
    fn last_years_journal_cannot_swallow_this_years_event() {
        // ⚠️ THE DEFAULT-CONFIGURATION DATA LOSS. `fd_event_name` ships EMPTY, and the host
        // journal is named from its slug — an empty slug becomes the literal "event", so
        // every host that never typed a name writes to ONE file for ever. Replaying it a
        // year later restored last year's rows AND their per-position ack watermarks. The
        // position's own ADIF journal self-expires at four days, so it began this year at
        // seq 1 — and the host refused every contact as a `(posid, seq)` it already held,
        // while `merge` returned the stored ack so the position's chip read "Synced". A
        // full event logged into a host that kept none of it, with nothing on screen wrong.
        let dir = std::env::temp_dir().join(format!("nexus-fdj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fd_event_event.jsonl");
        let now = now_unix();
        let last_year = now - 365 * 86_400;

        // Last year's host wrote five rows for this position.
        {
            let mut old = ClubLog::new(FdEvent::ArrlFd, "event");
            old.attach_journal_since(&path, 0).unwrap();
            for seq in 1..=5 {
                old.merge_row(
                    MergedRow {
                        posid: "aaaa1111".into(),
                        seq,
                        call: format!("W1OLD{seq}"),
                        class: "2A".into(),
                        section: "CT".into(),
                        ex: vec![],
                        mex: vec![],
                        band: "20m".into(),
                        mode_class: "PH".into(),
                        submode: String::new(),
                        when_unix: last_year + seq,
                        operator: "KD9TAW".into(),
                        sat: String::new(),
                        sat_fm: false,
                        station_call: String::new(),
                    },
                    last_year,
                );
            }
        }
        assert!(path.exists(), "harness: last year's journal was written");

        // This year's host replays the SAME file.
        let mut host = ClubLog::new(FdEvent::ArrlFd, "event");
        host.attach_journal_since(&path, now.saturating_sub(STALE_EVENT_SECS))
            .unwrap();
        assert_eq!(
            host.rows().len(),
            0,
            "a year-old journal must not be replayed into this year's club log"
        );

        // …so this year's contacts, which restart at seq 1, are accepted.
        for seq in 1..=4 {
            host.merge_row(
                MergedRow {
                    posid: "aaaa1111".into(),
                    seq,
                    call: format!("W2NEW{seq}"),
                    class: "2A".into(),
                    section: "WI".into(),
                    ex: vec![],
                    mex: vec![],
                    band: "40m".into(),
                    mode_class: "CW".into(),
                    submode: String::new(),
                    when_unix: now + seq,
                    operator: "KD9TAW".into(),
                    sat: String::new(),
                    sat_fm: false,
                    station_call: String::new(),
                },
                now,
            );
        }
        assert_eq!(
            host.rows().len(),
            4,
            "this year's four contacts are in the club log"
        );

        // POSITIVE CONTROL: with the cutoff opened up, the bug reappears exactly as reported —
        // otherwise this test would pass against a change that simply broke journal replay.
        let mut naive = ClubLog::new(FdEvent::ArrlFd, "event");
        naive.attach_journal_since(&path, 0).unwrap();
        assert_eq!(
            naive.rows().len(),
            5,
            "control: without a cutoff last year's rows return"
        );
        let accepted = naive.merge_row(
            MergedRow {
                posid: "aaaa1111".into(),
                seq: 1,
                call: "W2NEW1".into(),
                class: "2A".into(),
                section: "WI".into(),
                ex: vec![],
                mex: vec![],
                band: "40m".into(),
                mode_class: "CW".into(),
                submode: String::new(),
                when_unix: now,
                operator: "KD9TAW".into(),
                sat: String::new(),
                sat_fm: false,
                station_call: String::new(),
            },
            now,
        );
        assert_eq!(
            naive.rows().len(),
            5,
            "control: this year's contact was REFUSED as a dupe"
        );
        assert!(
            !accepted,
            "control: the row was refused as a duplicate — and nothing on screen said so"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_presence_report_renames_a_position_and_a_nameless_one_leaves_it_alone() {
        // The club-Field-Day bug (2026-08-30): the name travelled in the JOIN
        // line only, so an operator who renamed the position — or named one
        // that joined unnamed — watched the board keep the old text until the
        // connection was rebuilt. Presence reports now carry it.
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.join("aaaa", "CW tent", "KD9TAW", 1000);
        club.position_status("aaaa", &report("GOTA tent", "20m", "cw", "op1"), 1001);
        let label = |c: &ClubLog| c.positions()["aaaa"].label.clone();
        assert_eq!(label(&club), "GOTA tent", "the rename landed");
        // ...and the other direction, which is what keeps an older peer from
        // blanking a label it simply doesn't know how to send.
        club.position_status("aaaa", &report("", "20m", "cw", "op1"), 1002);
        assert_eq!(label(&club), "GOTA tent", "a nameless report is no news");
        club.position_status("aaaa", &report("   ", "20m", "cw", "op1"), 1003);
        assert_eq!(label(&club), "GOTA tent", "whitespace is not a name either");
        // A position the host has never heard a join from is still named by
        // its report (the id is plumbing; nobody should ever read it).
        club.position_status("bbbb", &report("SSB tent", "40m", "ph", "op2"), 1004);
        assert_eq!(label(&club), "GOTA tent");
        assert_eq!(club.positions()["bbbb"].label, "SSB tent");
    }

    #[test]
    fn mirror_unions_deltas_resets_on_snap_and_keeps_scalars_current() {
        let mut m = ClubMirror::default();
        m.apply(&ClubState {
            reset: true,
            dupes: vec![("W1AW".into(), "20m".into(), "DIG".into())],
            dkeys: vec![vec!["W1AW".into(), "20M".into(), "DIG".into()]],
            sections: vec!["CT".into()],
            score: 10,
            qsos: 1,
            board: vec![],
        });
        m.apply(&ClubState {
            reset: false,
            dupes: vec![("K1ABC".into(), "40m".into(), "CW".into())],
            dkeys: vec![vec!["K1ABC".into(), "40M".into(), "CW".into()]],
            sections: vec!["EMA".into()],
            score: 14,
            qsos: 2,
            board: vec![],
        });
        assert_eq!(m.dupes.len(), 2, "deltas union");
        assert_eq!(m.dkeys.len(), 2, "…and the generalised keys with them");
        assert_eq!((m.score, m.qsos), (14, 2), "scalars overwrite");
        // A rejoin snap (host restarted into a new event) CLEARS before applying.
        m.apply(&ClubState {
            reset: true,
            dupes: vec![("N0XYZ".into(), "20m".into(), "PH".into())],
            dkeys: vec![vec!["N0XYZ".into(), "20M".into(), "PH".into()]],
            sections: vec!["MN".into()],
            score: 1,
            qsos: 1,
            board: vec![],
        });
        assert_eq!(
            m.dupes.len(),
            1,
            "a snap replaces, never unions a dead event"
        );
        assert!(m
            .dupes
            .contains(&("N0XYZ".into(), "20m".into(), "PH".into())));
        assert_eq!(m.dkeys.len(), 1, "the generalised set is reset too");
        assert!(m
            .dkeys
            .contains(&vec!["N0XYZ".to_string(), "20M".into(), "PH".into()]));
    }

    #[test]
    fn sync_state_is_derived_and_cannot_disagree_with_the_queue() {
        use SyncState::*;
        assert_eq!(SyncState::derive(false, true, 0, 0), Disabled);
        assert_eq!(
            SyncState::derive(true, false, 3, 42),
            Offline {
                queued: 3,
                since: 42
            }
        );
        assert_eq!(SyncState::derive(true, true, 2, 0), Behind { queued: 2 });
        assert_eq!(SyncState::derive(true, true, 0, 0), Synced);
        assert_eq!(SyncState::Synced.code(), "synced");
        assert_eq!(SyncState::derive(true, false, 0, 7).code(), "offline");
    }

    // ---- v2: the wire, the journal, and both compatibility directions -----

    /// A REAL 1.x host journal: twelve NDJSON rows written by the shipped
    /// `MergedRow`, three positions, all three mode classes, two digital submodes,
    /// one unrecorded operator and one cross-position dupe — with the Cabrillo and
    /// ADIF that same build exported from it.
    ///
    /// ⭐ **PROVENANCE, and it is checked rather than asserted in prose.** The bytes
    /// were produced by a build at `f27a2e0a` (this branch's base, pre-batch-4),
    /// whose `MergedRow` is byte-identical to the one released as **`v1.10.3`** —
    /// `diff <(git show v1.10.3:crates/tempo-app/src/fdevent.rs) <(git show
    /// f27a2e0a:…)` over the struct is empty. So these ARE a 1.x host's bytes and a
    /// 1.x host's exports, not a hand-typed guess at them.
    ///
    /// That git check cannot run inside a test, so [`V1MergedRow`] carries the ten
    /// fields v1.10.3 declared, in its order, and
    /// `the_fixture_is_what_v1_10_3s_merged_row_wrote` round-trips every line
    /// through it. The test therefore holds its own definition of what v1 meant
    /// instead of trusting today's type to have stayed honest about it.
    ///
    ///
    /// ⚠️ The `.cbr`'s line 2 moved once since capture, deliberately:
    /// `CONTEST: ARRL-FIELD-DAY` → `CONTEST: ARRL-FD` (694 bytes to 687), because
    /// that header is a Cabrillo token and `ARRL-FIELD-DAY` is the ADIF one. See
    /// the module header of `tempo-core/tests/fd_goldens.rs`. The `.adi` did NOT
    /// move — its `CONTEST_ID` is the ADIF value and stays that way.
    ///
    /// ⚠️ **Two honest caveats.** The twelve contacts are SYNTHETIC — what is real
    /// is the serialization, not somebody's actual Field Day log. And the `.cbr`
    /// and `.adi` capture PRE-UPGRADE exporter behaviour, which is precisely the
    /// property that lets the migration test fail if the migration is wrong.
    const J1X: &str = include_str!("../tests/fixtures/fd-1x-journal/fd_event_1x.jsonl");
    const J1X_CBR: &str = include_str!("../tests/fixtures/fd-1x-journal/fd_event_1x.cbr");
    const J1X_ADI: &str = include_str!("../tests/fixtures/fd-1x-journal/fd_event_1x.adi");

    /// `MergedRow` EXACTLY as `v1.10.3` declared it — the ten fields, in that order,
    /// with that build's serde attributes. Nothing may be added here: its whole job
    /// is to be the frozen record of what a 1.x journal line is, so a future edit to
    /// the live `MergedRow` cannot quietly redefine the thing the fixture is being
    /// checked against.
    #[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
    struct V1MergedRow {
        posid: String,
        seq: u64,
        call: String,
        class: String,
        section: String,
        band: String,
        mode_class: String,
        #[serde(default)]
        submode: String,
        when_unix: u64,
        #[serde(default)]
        operator: String,
    }

    /// ⭐ The fixture proves its own provenance: every line parses as v1.10.3's
    /// `MergedRow` and re-serializes to the identical bytes.
    ///
    /// Round-tripping is what makes this stronger than "the JSON has no `ex` key".
    /// A line with a field v1 never declared, a field missing that v1 required, or
    /// a different field ORDER would all survive a key check and fail here — so
    /// "these bytes are a 1.x journal" stops being a claim in a report and becomes
    /// something the suite re-derives on every run.
    #[test]
    fn the_fixture_is_what_v1_10_3s_merged_row_wrote() {
        let mut n = 0;
        for line in J1X.lines() {
            let v1: V1MergedRow =
                serde_json::from_str(line).expect("every line is a v1.10.3 MergedRow");
            assert_eq!(
                serde_json::to_string(&v1).unwrap(),
                line,
                "the line re-serializes byte-identically under v1.10.3's own shape"
            );
            // …and today's type reads the same row out of those same bytes.
            let now: MergedRow = serde_json::from_str(line).unwrap();
            assert_eq!(
                (now.posid, now.seq, now.call, now.class, now.section),
                (v1.posid, v1.seq, v1.call, v1.class, v1.section),
                "the v2 type reads a v1 line as the v1 type wrote it"
            );
            n += 1;
        }
        assert_eq!(n, 12, "the whole fixture was checked, not an empty file");

        // POSITIVE CONTROL — the check discriminates. A v2-shaped line (the fields
        // this batch ADDED) must NOT round-trip as v1.10.3, or the assertion above
        // would pass for a regenerated fixture that had quietly become a v2 one.
        let v2_line = serde_json::to_string(&MergedRow {
            posid: "aaaa0001".into(),
            seq: 1,
            call: "W1AW".into(),
            class: "2A".into(),
            section: "CT".into(),
            ex: fd_fields(FdEvent::ArrlFd, "2A", "CT"),
            mex: fd_fields(FdEvent::ArrlFd, "3A", "WI"),
            band: "20m".into(),
            mode_class: "PH".into(),
            submode: String::new(),
            when_unix: 1_782_579_600,
            operator: "KD9TAW".into(),
            sat: String::new(),
            sat_fm: false,
            station_call: String::new(),
        })
        .unwrap();
        let as_v1: V1MergedRow =
            serde_json::from_str(&v2_line).expect("v1 ignores fields it does not know");
        assert_ne!(
            serde_json::to_string(&as_v1).unwrap(),
            v2_line,
            "control: a v2 line does NOT round-trip as v1.10.3 — the check can tell them apart"
        );
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fdevent-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A field vector for the Field Day exchange, resolved the way a position does.
    fn fd_fields(event: FdEvent, class: &str, section: &str) -> Vec<WireField> {
        let spec = tempo_core::contest::field_day(event);
        to_wire_fields(
            &["CLASS", "SECTION"]
                .iter()
                .zip([class, section])
                .filter_map(|(k, v)| spec.value(k, v))
                .collect::<Vec<_>>(),
        )
    }

    /// [`wq`]'s row in its v2 shape: the same contact, with the exchange as data
    /// on both sides instead of only the legacy pair. `my` is the class and
    /// section the LOGGING POSITION sent.
    fn with_sent(mut q: WireQso, my: (&str, &str)) -> WireQso {
        q.ex = fd_fields(FdEvent::ArrlFd, &q.class.clone(), &q.sect.clone());
        q.mex = fd_fields(FdEvent::ArrlFd, my.0, my.1);
        q
    }

    /// ⭐ §8g — THE test that fails if the host journal is dropped by the upgrade.
    ///
    /// Add one REQUIRED field to `MergedRow` and every pre-upgrade line decodes as
    /// nothing: the replay loop is `if let Ok(row) = …`, which skips silently. The
    /// host comes up clean, every position's ack watermark resets to 0, and a tent
    /// that has gone off the air is simply gone — with the sync chip still reading
    /// Synced. So this asserts on a real 1.x journal that ALL twelve rows come
    /// back, the dupe keys rebuild, each position's high-water ack is restored, and
    /// the score and both exports are the bytes that build produced.
    #[test]
    fn a_1x_host_journal_survives_the_v2_upgrade() {
        // The fixture is only evidence while it is still a 1.x journal: a
        // regenerated one carrying `ex`/`mex` would pass every assertion below
        // without proving anything about legacy decoding.
        for line in J1X.lines() {
            let v: serde_json::Value = serde_json::from_str(line).expect("fixture line is JSON");
            let obj = v.as_object().unwrap();
            assert!(
                !obj.contains_key("ex") && !obj.contains_key("mex"),
                "the fixture must stay a 1.x journal — this line carries v2 fields: {line}"
            );
            assert!(obj.contains_key("class") && obj.contains_key("section"));
        }

        let dir = scratch("j1x");
        let path = dir.join("fd_event_granite.jsonl");
        std::fs::write(&path, J1X).unwrap();

        let mut host = ClubLog::new(FdEvent::ArrlFd, "GRANITE ARC FD");
        host.attach_journal_since(&path, 0).unwrap();

        assert_eq!(
            host.qsos_raw(),
            12,
            "every row of the 1.x journal came back"
        );
        assert_eq!(
            host.qsos_unique(),
            10,
            "the dupe-key set rebuilt identically"
        );
        assert_eq!(
            host.sections(),
            ["CT", "EMA", "MN", "STX", "ONS", "AZ", "PR", "WI", "NLI"],
            "sections rebuilt, in first-seen order"
        );
        // The high-water ack per position — the value whose loss made a position
        // restart at seq 1 into a host that then refused every contact as a dupe.
        for pos in ["aaaa0001", "bbbb0002", "cccc0003"] {
            assert_eq!(host.join(pos, "", "", 0), 4, "{pos} ack watermark restored");
        }
        assert_eq!(
            host.scored("W9ABC", "3A", "WI", 2, &[]),
            (16, 32, 0, 32),
            "the same score the 1.x build computed from these bytes"
        );
        assert_eq!(
            host.export_cabrillo("W9ABC", "3A", "WI")
                .expect("a single-mode event exports one entry"),
            J1X_CBR,
            "the club Cabrillo is byte-identical to what 1.x exported"
        );
        assert_eq!(
            host.export_adif("W9ABC", "3A", "WI"),
            J1X_ADI,
            "…and so is the club ADIF"
        );

        // POSITIVE CONTROL — a green that cannot go red is not a result. Corrupt
        // ONE line: exactly that row is lost and the other eleven are untouched,
        // which proves the assertions above discriminate rather than pass vacuously.
        let mut lines: Vec<&str> = J1X.lines().collect();
        let torn = &lines[4][..30];
        lines[4] = torn;
        let corrupt = dir.join("fd_event_torn.jsonl");
        std::fs::write(&corrupt, lines.join("\n")).unwrap();
        let mut torn_host = ClubLog::new(FdEvent::ArrlFd, "GRANITE ARC FD");
        torn_host.attach_journal_since(&corrupt, 0).unwrap();
        assert_eq!(
            torn_host.qsos_raw(),
            11,
            "control: one corrupted line costs exactly one row"
        );
        assert!(
            !torn_host.rows().iter().any(|r| r.call == "VE3GHI"),
            "control: it is the corrupted row that is missing"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A v1 position's row reaches a v2 host with its exchange intact — the
    /// old→new direction, which is the one that fails SILENTLY if it fails.
    #[test]
    fn a_v1_rows_exchange_is_synthesised_and_carries_its_domain() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "PH", "CT", 100), 100);
        let row = &club.rows()[0];
        assert_eq!(
            row.ex,
            vec![
                WireField {
                    k: "CLASS".into(),
                    d: String::new(),
                    r: "2A".into()
                },
                WireField {
                    // The domain travels: an export tag and a multiplier bucket are
                    // chosen by it, so a synthesised slot that dropped it would be
                    // subtly unlike one a v2 position sent.
                    k: "SECTION".into(),
                    d: "fd_sections".into(),
                    r: "CT".into()
                },
            ],
            "the legacy pair was synthesised into the exchange, with its domain"
        );
        assert!(row.mex.is_empty(), "a v1 row sends no sent side");
        assert_eq!(
            row.dkey(
                &tempo_core::fd_rules::ruleset(
                    FdEvent::ArrlFd,
                    tempo_core::fd_rules::CURRENT_RULES_YEAR
                )
                .dupe_rule,
                tempo_core::contest::field_day(FdEvent::ArrlFd)
            ),
            vec!["W1AW", "20M", "PH"],
            "and the generalised key is Field Day's own rule"
        );
        // A row with neither an exchange nor the legacy pair synthesises nothing
        // rather than inventing two empty slots.
        let mut bare = wq("bbbb", 1, "K1ABC", "20m", "CW", "", 200);
        bare.class = String::new();
        club.merge(&bare, 200);
        assert!(
            club.rows()[1].ex.is_empty(),
            "nothing to synthesise from, so nothing synthesised"
        );
    }

    /// ⭐ §11 batch 4's own shippability test: an all-v2 club and a MIXED club run
    /// ARRL Field Day identically. Field Day is shipped, with users; if these three
    /// clubs disagree by one byte, a real club's submission moved.
    #[test]
    fn an_all_v2_club_and_a_mixed_club_run_field_day_identically() {
        let rows: &[(&str, u64, &str, &str, &str, &str, u64)] = &[
            ("aaaa", 1, "W1AW", "20m", "PH", "CT", 100),
            ("bbbb", 1, "K1ABC", "40m", "CW", "EMA", 200),
            ("aaaa", 2, "N0XYZ", "20m", "DIG", "MN", 300),
            ("bbbb", 2, "W1AW", "20m", "PH", "CT", 400), // cross-position dupe
            ("cccc", 1, "W5DEF", "15m", "CW", "STX", 500),
        ];
        let build = |v2_from: usize| {
            let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
            for (i, (p, s, c, b, m, sect, w)) in rows.iter().enumerate() {
                // Every position of a Field Day club sends the club's own class and
                // section, so a v2 row's `mex` is the host's — which is exactly what
                // a v1 row falls back to.
                if i >= v2_from {
                    club.merge(&with_sent(wq(p, *s, c, b, m, sect, *w), ("3A", "WI")), *w);
                } else {
                    club.merge(&wq(p, *s, c, b, m, sect, *w), *w);
                }
            }
            club
        };
        let all_v1 = build(rows.len()); // every row legacy-shaped
        let mixed = build(2); // two v1 tents, three v2 tents
        let all_v2 = build(0);
        for (label, club) in [("mixed", &mixed), ("all-v2", &all_v2)] {
            assert_eq!(
                club.export_cabrillo("W9ABC", "3A", "WI")
                    .expect("a single-mode event exports one entry"),
                all_v1
                    .export_cabrillo("W9ABC", "3A", "WI")
                    .expect("a single-mode event exports one entry"),
                "{label} club's Cabrillo moved"
            );
            assert_eq!(
                club.export_adif("W9ABC", "3A", "WI"),
                all_v1.export_adif("W9ABC", "3A", "WI"),
                "{label} club's ADIF moved"
            );
            assert_eq!(
                club.scored("W9ABC", "3A", "WI", 2, &[]),
                all_v1.scored("W9ABC", "3A", "WI", 2, &[]),
                "{label} club's score moved"
            );
            assert_eq!(club.dupe_keys(), all_v1.dupe_keys(), "{label} dupe keys");
            assert_eq!(club.dkeys(), all_v1.dkeys(), "{label} generalised keys");
        }
        // The harness is not vacuous: these clubs really did carry both shapes.
        assert!(all_v2.rows().iter().all(|r| !r.mex.is_empty()));
        assert!(all_v1.rows().iter().all(|r| r.mex.is_empty()));
        assert!(mixed.rows().iter().any(|r| r.mex.is_empty()));
        assert!(mixed.rows().iter().any(|r| !r.mex.is_empty()));
    }

    /// ⭐ The defect `mex` exists to close: the host rebuilt EVERY position's rows
    /// under its OWN class and section. Harmless for one Field Day club, wrong the
    /// moment two positions send different exchanges — a club spanning a section
    /// line, and every QSO party with a mobile.
    #[test]
    fn the_host_exports_each_rows_own_sent_exchange_not_its_own() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TWO-SITE FD");
        club.merge(
            &with_sent(wq("aaaa", 1, "W1AW", "20m", "PH", "CT", 100), ("3A", "WI")),
            100,
        );
        club.merge(
            &with_sent(
                wq("bbbb", 1, "K1ABC", "40m", "CW", "EMA", 200),
                ("5A", "EMA"),
            ),
            200,
        );
        let cab = club
            .export_cabrillo("W9ABC", "3A", "WI")
            .expect("a single-mode event exports one entry");
        assert!(
            cab.contains("W9ABC 3A WI W1AW 2A CT"),
            "the first tent's own sent exchange: {cab}"
        );
        assert!(
            cab.contains("W9ABC 5A EMA K1ABC 2A EMA"),
            "the second tent sent 5A EMA and the line must say so: {cab}"
        );

        // POSITIVE CONTROL: the same two contacts as v1 rows, which carry no sent
        // side, both fall back to the host's — a DIFFERENT byte string. Without
        // this the test above would pass against a build that ignored `mex` and
        // happened to agree with the host on tent one.
        let mut legacy = ClubLog::new(FdEvent::ArrlFd, "TWO-SITE FD");
        legacy.merge(&wq("aaaa", 1, "W1AW", "20m", "PH", "CT", 100), 100);
        legacy.merge(&wq("bbbb", 1, "K1ABC", "40m", "CW", "EMA", 200), 200);
        let legacy_cab = legacy
            .export_cabrillo("W9ABC", "3A", "WI")
            .expect("a single-mode event exports one entry");
        assert!(
            legacy_cab.contains("W9ABC 3A WI K1ABC 2A EMA"),
            "control: a legacy row falls back to the host's sent exchange: {legacy_cab}"
        );
        assert_ne!(
            cab, legacy_cab,
            "control: the fixture discriminates — reading `mex` changes the bytes"
        );
    }

    /// ⭐ §18.2 — since v3, **every older position is refused at JOIN, at every club**, and
    /// the message names both versions and says what to do: an older Nexus has no club key
    /// to prove its position with, and a host that served one would let any peer take another
    /// laptop's position by claiming to be older.
    #[test]
    fn every_older_position_is_refused_by_name_at_every_club() {
        let need = tempo_net::fdsync::PROTO_VERSION;
        let mut clubs = vec![
            ClubLog::new(FdEvent::ArrlFd, "TEST FD"),
            ClubLog::new(FdEvent::WinterFd, "TEST WFD"),
            ClubLog::for_ruleset(party("ilqp"), "ILQP TEST"),
        ];
        let mut qp = ClubLog::new(FdEvent::ArrlFd, "TNQP 2026");
        qp.contest_id = "TN-QSO-PARTY".into();
        clubs.push(qp);
        for club in &clubs {
            for v in 1..need {
                let msg = club.version_refusal(v).expect("an older tent is refused");
                assert_eq!(
                    msg,
                    format!(
                        "this club's host needs club sync v{need} on every laptop, so that each \
                         laptop proves which club position it is, and this Nexus speaks v{v}. \
                         Update Nexus on this laptop, then rejoin: contacts you log meanwhile \
                         stay in your own log and go up when you do."
                    ),
                    "{}",
                    club.contest_id
                );
                assert_eq!(club.join_refusal(v, &club.event_id), Some(msg));
            }
            // …and a CURRENT position is served by the same club. Refusing on version when
            // the version is fine would lock every tent out.
            assert_eq!(club.version_refusal(need), None, "{}", club.contest_id);
        }
        assert_eq!(need, 3, "the version this rule was written for");
    }

    /// The legacy triple ships only for Field Day; the generalised key always
    /// does. A v1 position given a triple that is not the running dupe rule would
    /// show a WRONG while-typing warning, which is worse than none.
    #[test]
    fn the_legacy_dupe_triple_ships_only_for_a_field_day_club() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "PH", "CT", 100), 100);
        club.merge(&wq("aaaa", 2, "K1ABC", "40m", "CW", "EMA", 200), 200);
        let st = club.club_state(0, 0, 0, 300);
        assert_eq!(
            st.dupes,
            vec![
                ("W1AW".to_string(), "20m".into(), "PH".into()),
                ("K1ABC".to_string(), "40m".into(), "CW".into()),
            ],
            "a Field Day club ships the triple a v1 position understands, band verbatim"
        );
        assert_eq!(
            st.dkeys,
            vec![
                vec!["W1AW".to_string(), "20M".into(), "PH".into()],
                vec!["K1ABC".to_string(), "40M".into(), "CW".into()],
            ],
            "…and the generalised key beside it, index-parallel"
        );
        // The cursor addresses both lists.
        let delta = club.club_state(1, 0, 0, 300);
        assert_eq!(delta.dupes.len(), 1);
        assert_eq!(delta.dkeys.len(), 1);

        club.contest_id = "TN-QSO-PARTY".into();
        let st = club.club_state(0, 0, 0, 300);
        assert!(
            st.dupes.is_empty(),
            "no club warning beats a wrong one when the triple is not the rule"
        );
        assert_eq!(st.dkeys.len(), 2, "the generalised keys still ship");
    }

    // ---- a club runs the HOST's ruleset, whatever the contest -------------

    fn party(event: &str) -> &'static FdRuleset {
        tempo_core::fd_rules::ruleset_by_id(event, tempo_core::fd_rules::CURRENT_RULES_YEAR)
            .unwrap_or_else(|| panic!("{event} is a shipped ruleset"))
    }

    /// The HOST's session for a contest, from the station data Settings holds — what
    /// `Engine::fd_club_session` builds for a host that is not running it live.
    fn party_session(event: &str, state: &str, county: &str) -> ContestSession {
        ContestSession::for_ruleset(
            party(event),
            &tempo_core::contest::StationData {
                mycall: "W9XYZ".into(),
                mygrid: "EN61".into(),
                contest_qth_state: state.into(),
                contest_qth_county: county.into(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("{event} session refused: {e}"))
    }

    /// An exchange as a position sends it: each value resolved through the ruleset's
    /// own spec (`ExchangeSpec::copied`), so the domain that matched travels with it.
    fn party_fields(event: &str, pairs: &[(&str, &str)]) -> Vec<WireField> {
        let spec = party(event).exchange;
        to_wire_fields(
            &pairs
                .iter()
                .filter_map(|(k, v)| spec.copied(k, v))
                .collect::<Vec<_>>(),
        )
    }

    /// One Illinois QSO Party contact as a current position streams it: the county THEY
    /// sent, the county THIS POSITION sent, the RST each way at the digits its mode
    /// uses, and the station call as operator (what a position with nobody named at the
    /// key sends).
    #[allow(clippy::too_many_arguments)] // one wire row's worth of fields
    fn ilqp_row(
        pos: &str,
        seq: u64,
        call: &str,
        band: &str,
        mode: &str,
        sub: &str,
        theirs: &str,
        mine: &str,
        when: u64,
    ) -> WireQso {
        let rst = if mode == "PH" { "59" } else { "599" };
        WireQso {
            pos: pos.into(),
            seq,
            call: call.into(),
            class: String::new(),
            sect: String::new(),
            ex: party_fields("ilqp", &[("RST", rst), ("QTH", theirs)]),
            mex: party_fields("ilqp", &[("RST", rst), ("QTH", mine)]),
            band: band.into(),
            mode: mode.into(),
            sub: sub.into(),
            when,
            op: "W9XYZ".into(),
            sat: String::new(),
            sat_fm: false,
        }
    }

    /// 1700Z, Sunday 18 October 2026 — the party's first minute.
    const ILQP_START: u64 = 1_792_342_800;

    /// ⭐ **THE REVIEW'S PROBE, as a test: an Illinois QSO Party club runs the party's own
    /// rules.**
    ///
    /// Hosting the party used to build an ARRL Field Day club log in silence. These three
    /// rows from one position — K9AAA on 40 m CW from Cook, the same station on RTTY, and
    /// the same station on CW again from Will (a mobile that moved) — produced
    /// `contest_id=ARRL-FIELD-DAY`, dupe keys `[K9AAA 40M CW] [K9AAA 40M DIG]` (RTTY a
    /// second contact, the new county a dupe), merged rows with the county gone, and a
    /// club Cabrillo headed `CONTEST: ARRL-FD` whose QSO lines carried no exchange.
    ///
    /// The sponsor, *"Announcing the 2026 Illinois QSO Party"* (w9awe.org, read
    /// 2026-10-08): *"Stations may be worked once per band and mode (phone and
    /// CW/digital) and once per band/mode/county for IL Mobile and Rover stations"*; and
    /// its `Sample_Excel_Log` heads the file `CONTEST: ILLINOIS QSO PARTY` and writes each
    /// county code on the QSO line — `QSO: 7000 CW 2013-10-20 1714 W9XYZ 599 JODA K9NR 599
    /// KANK`. *"IL stations multiply points by the sum of IL counties, US states, VE
    /// provinces and DXCC countries (maximum 5) worked."*
    #[test]
    fn an_illinois_qso_party_club_runs_the_partys_own_rules() {
        let mut club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        assert_eq!(
            (club.event_id.as_str(), club.contest_id.as_str()),
            ("ilqp", "IL QSO Party"),
            "the club runs the party, not ARRL Field Day"
        );
        assert!(club.field_day_event().is_none());
        club.merge(
            &ilqp_row(
                "aaaa",
                1,
                "K9AAA",
                "40m",
                "CW",
                "",
                "COOK",
                "MCLN",
                ILQP_START + 180,
            ),
            1,
        );
        club.merge(
            &ilqp_row(
                "aaaa",
                2,
                "K9AAA",
                "40m",
                "DIG",
                "RTTY",
                "COOK",
                "MCLN",
                ILQP_START + 240,
            ),
            2,
        );
        club.merge(
            &ilqp_row(
                "aaaa",
                3,
                "K9AAA",
                "40m",
                "CW",
                "",
                "WILL",
                "MCLN",
                ILQP_START + 300,
            ),
            3,
        );
        assert_eq!(club.qsos_raw(), 3, "every row merged");
        assert_eq!(
            club.qsos_unique(),
            2,
            "RTTY after CW is the same CW/digital mode; a new county is a new contact"
        );
        let session = party_session("ilqp", "IL", "MCLN");
        let log = club.unique_log_with("W9XYZ", session.clone());
        let qths: Vec<&str> = log.qsos().iter().map(|q| q.rcvd("QTH")).collect();
        assert_eq!(
            qths,
            ["COOK", "WILL"],
            "each contact keeps the county it sent"
        );
        let cab = club
            .export_cabrillo_with("W9XYZ", session.clone(), &CabrilloEntrant::default())
            .expect("one entry");
        assert!(cab.contains("CONTEST: ILLINOIS QSO PARTY\n"), "{cab}");
        assert!(cab.contains("CATEGORY-OPERATOR: MULTI-OP\n"), "{cab}");
        assert!(cab.contains("IL-COUNTY: McLean\n"), "{cab}");
        assert!(
            cab.contains("QSO: 7000 CW 2026-10-18 1703 W9XYZ 599 MCLN K9AAA 599 COOK\n"),
            "the exchange columns, both sides: {cab}"
        );
        assert!(
            cab.contains("QSO: 7000 CW 2026-10-18 1705 W9XYZ 599 MCLN K9AAA 599 WILL\n"),
            "the mobile's new county is its own line: {cab}"
        );
        assert_eq!(
            cab.matches("QSO:").count(),
            2,
            "the RTTY repeat is not a second contact: {cab}"
        );
        // 2 CW contacts × 2 points, × 2 counties (Cook, Will).
        let s = club.score_with("W9XYZ", session, 1, &[], &[]);
        assert_eq!((s.qso_points, s.mults, s.total), (4, Some(2), 8));
        assert!(cab.contains("CLAIMED-SCORE: 8\n"), "{cab}");
    }

    /// ⭐ **Two positions logging the same station: the second is the CLUB dupe** — kept
    /// as a row (a warning at the position, never a lock), scored once, exported once —
    /// and the key the club ships for it is the very key the position's own log builds,
    /// which is what the position's while-typing check compares against.
    #[test]
    fn the_second_position_to_work_a_station_is_the_club_dupe() {
        let mut club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        club.merge(
            &ilqp_row(
                "aaaa",
                1,
                "K9BBB",
                "20m",
                "PH",
                "",
                "KANE",
                "MCLN",
                ILQP_START + 60,
            ),
            1,
        );
        club.merge(
            &ilqp_row(
                "bbbb",
                1,
                "K9BBB",
                "20m",
                "PH",
                "",
                "KANE",
                "MCLN",
                ILQP_START + 120,
            ),
            2,
        );
        assert_eq!(
            (club.qsos_raw(), club.qsos_unique()),
            (2, 1),
            "both rows kept, one contact"
        );
        let st = club.club_state(0, 0, 0, 3);
        assert_eq!(st.dkeys.len(), 1, "one club key");
        assert!(st.dupes.is_empty(), "no Field Day triple for a party");
        // The same contact in a position's OWN log, same contest, same station.
        let mut own = FieldDayLog::new("W9XYZ", party_session("ilqp", "IL", "MCLN"), "20m");
        assert!(own.log_fields_at(
            "K9BBB",
            &[
                ("RST".to_string(), "59".to_string()),
                ("QTH".to_string(), "KANE".to_string())
            ],
            "PH",
            "",
            0,
            ILQP_START + 120,
        ));
        assert_eq!(
            st.dkeys[0],
            own.dupe_rule().key(&own.qsos()[0]),
            "the club ships the key the position builds — so the second position is warned"
        );
        let cab = club
            .export_cabrillo_with(
                "W9XYZ",
                party_session("ilqp", "IL", "MCLN"),
                &CabrilloEntrant::default(),
            )
            .unwrap();
        assert_eq!(cab.matches("QSO:").count(), 1, "earliest wins: {cab}");
        assert!(cab.contains(" 1701 W9XYZ 59 MCLN K9BBB 59 KANE\n"), "{cab}");
        // The board credits the earlier position with the contact.
        let board = club.board_rows(10);
        let uniq = |p: &str| board.iter().find(|r| r.pos == p).map(|r| r.uniq);
        assert_eq!((uniq("aaaa"), uniq("bbbb")), (Some(1), Some(0)));
        // CONTROL: the same station from ANOTHER county is a new contact (the mobile rule).
        club.merge(
            &ilqp_row(
                "bbbb",
                2,
                "K9BBB",
                "20m",
                "PH",
                "",
                "DUPG",
                "MCLN",
                ILQP_START + 180,
            ),
            3,
        );
        assert_eq!(club.qsos_unique(), 2);
    }

    /// The party's two club stations are worth 100 each, *"added to the final score"* —
    /// once for the ENTRY, however many positions work them.
    #[test]
    fn the_partys_bonus_stations_count_once_for_the_whole_club() {
        let mut club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        club.merge(
            &ilqp_row(
                "aaaa",
                1,
                "W9AWE",
                "40m",
                "CW",
                "",
                "MCDN",
                "MCLN",
                ILQP_START + 60,
            ),
            1,
        );
        club.merge(
            &ilqp_row(
                "bbbb",
                1,
                "W9AWE",
                "20m",
                "PH",
                "",
                "MCDN",
                "MCLN",
                ILQP_START + 120,
            ),
            2,
        );
        let s = club.score_with("W9XYZ", party_session("ilqp", "IL", "MCLN"), 1, &[], &[]);
        // CW 2 + phone 1, × one county (McDonough), + 100 once.
        assert_eq!(
            (s.qso_points, s.mults, s.bonus, s.total),
            (3, Some(1), 100, 103)
        );
    }

    /// ⭐ **The club file names who logged each contact** — the sponsor's `OPERATORS`
    /// header is everyone the rows name, plus whoever the host typed in. A position with
    /// nobody named at the key sends the station call, and that reads as nobody: the
    /// station call is never an operator (`LoggedQso::operator`).
    #[test]
    fn the_club_file_names_the_operators_its_positions_logged_under() {
        let mut club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        let mut named = ilqp_row("aaaa", 1, "K9CCC", "40m", "CW", "", "COOK", "MCLN", 100);
        named.op = "aa9xyz".into();
        club.merge(&named, 1);
        // Nobody named at the key: the wire carries the station call.
        club.merge(
            &ilqp_row("bbbb", 1, "K9DDD", "40m", "CW", "", "COOK", "MCLN", 200),
            2,
        );
        let entrant = CabrilloEntrant {
            entry_class: "UNLIMITED".into(),
            club: "WESTERN ILL AMATEUR RADIO CLUB".into(),
            operators: "W9OP1".into(),
            ..Default::default()
        };
        let cab = club
            .export_cabrillo_with("W9XYZ", party_session("ilqp", "IL", "MCLN"), &entrant)
            .unwrap();
        assert!(cab.contains("OPERATORS: AA9XYZ W9OP1\n"), "{cab}");
        assert!(cab.contains("ENTRY-CLASS: UNLIMITED\n"), "{cab}");
        assert!(
            cab.contains("CLUB: WESTERN ILL AMATEUR RADIO CLUB\n"),
            "{cab}"
        );
        assert!(
            !cab.contains("OPERATORS: W9XYZ"),
            "the station call is not an operator"
        );
    }

    /// ⭐ **A contest whose sponsor wants duplicates REPORTED keeps them in the club file.**
    /// The New York QSO Party cross-checks logs, so a repeat is logged and worth nothing
    /// rather than removed — removing it would hand the other station a not-in-log. The
    /// club does with a second position's repeat exactly what a position's own log does
    /// with its own: the later row is marked, written, and scores zero.
    #[test]
    fn a_club_whose_contest_reports_dupes_keeps_the_repeat_and_scores_it_zero() {
        let rs = party("nyqp");
        assert!(rs.dupe_rule.log_dupes, "fixture: NYQP reports its dupes");
        let mut club = ClubLog::for_ruleset(rs, "NYQP TEST");
        let row = |pos: &str, seq: u64, when: u64| WireQso {
            pos: pos.into(),
            seq,
            call: "K2AAA".into(),
            class: String::new(),
            sect: String::new(),
            ex: party_fields("nyqp", &[("RST", "599"), ("QTH", "BRX")]),
            mex: party_fields("nyqp", &[("RST", "599"), ("QTH", "ALB")]),
            band: "20m".into(),
            mode: "CW".into(),
            sub: String::new(),
            when,
            op: String::new(),
            sat: String::new(),
            sat_fm: false,
        };
        // The LATER-merged row is the earlier contact: time order decides which is marked.
        club.merge(&row("bbbb", 1, 2_000), 1);
        club.merge(&row("aaaa", 1, 1_000), 2);
        let session = party_session("nyqp", "NY", "ALB");
        let log = club.unique_log_with("W2XYZ", session.clone());
        let marks: Vec<(u64, bool)> = log.qsos().iter().map(|q| (q.when_unix, q.dupe)).collect();
        assert_eq!(
            marks,
            [(1_000, false), (2_000, true)],
            "both kept, in the order made, the later one marked"
        );
        assert_eq!(log.qso_count(), 1, "the repeat scores nothing");
        let cab = club
            .export_cabrillo_with("W2XYZ", session, &CabrilloEntrant::default())
            .unwrap();
        assert_eq!(cab.matches("QSO:").count(), 2, "both in the file: {cab}");
        // CONTROL: the Illinois QSO Party does not report dupes — its club file keeps one.
        let mut il = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        il.merge(
            &ilqp_row("bbbb", 1, "K9AAA", "20m", "CW", "", "COOK", "MCLN", 2_000),
            1,
        );
        il.merge(
            &ilqp_row("aaaa", 1, "K9AAA", "20m", "CW", "", "COOK", "MCLN", 1_000),
            2,
        );
        let cab = il
            .export_cabrillo_with(
                "W9XYZ",
                party_session("ilqp", "IL", "MCLN"),
                &CabrilloEntrant::default(),
            )
            .unwrap();
        assert_eq!(cab.matches("QSO:").count(), 1, "{cab}");
    }

    /// ⭐ **Every contest club sync runs writes, for one position's rows, the QSO lines that
    /// position's own log writes** — one exporter, under one session, whichever file it is.
    /// Run over every ruleset the club log accepts, with the outbox's own row shape.
    #[test]
    fn a_club_of_one_position_writes_that_positions_own_qso_lines_in_every_contest_it_runs() {
        // (contest, state, county, the received exchange of two different stations)
        let cases: &[(&str, &str, &str, [&str; 2])] = &[
            ("ilqp", "IL", "MCLN", ["COOK", "WILL"]),
            ("tnqp", "TN", "ANDE", ["BEDF", "BENT"]),
            ("ohqp", "OH", "ADAM", ["ALLE", "ASHL"]),
            ("txqp", "TX", "ANDE", ["ANDR", "ANGE"]),
            ("nyqp", "NY", "ALB", ["BRX", "ALL"]),
            ("arrlvhf_jan", "", "", ["FN31", "EM12"]),
            ("arrlvhf_jun", "", "", ["FN31", "EM12"]),
            ("arrlvhf_sep", "", "", ["FN31", "EM12"]),
        ];
        let mut seen = HashSet::new();
        for (event, state, county, theirs) in cases {
            let rs = party(event);
            assert_eq!(club_refusal(rs), None, "{event} is run by the club log");
            let slot = if event.starts_with("arrlvhf") {
                "GRID"
            } else {
                "QTH"
            };
            let mut own = FieldDayLog::new("W9XYZ", party_session(event, state, county), "20m");
            for (i, value) in theirs.iter().enumerate() {
                let mut fields = vec![(slot.to_string(), value.to_string())];
                if slot == "QTH" {
                    fields.insert(0, ("RST".to_string(), "599".to_string()));
                }
                let call = ["K9AAA", "N9BBB"][i];
                assert!(
                    own.log_fields_at(call, &fields, "CW", "", 0, ILQP_START + 60 * i as u64),
                    "{event}: fixture contact refused"
                );
            }
            // The outbox's own row shape (`Engine::fd_sync_outbox`).
            let mut club = ClubLog::for_ruleset(rs, "TEST");
            for q in own.qsos() {
                club.merge(
                    &WireQso {
                        pos: "aaaa".into(),
                        seq: q.seq,
                        call: q.call.clone(),
                        class: q.class().to_string(),
                        sect: q.section().to_string(),
                        ex: to_wire_fields(&q.rx),
                        mex: to_wire_fields(&q.tx),
                        band: q.band.clone(),
                        mode: q.mode.clone(),
                        sub: q.submode.clone(),
                        when: q.when_unix,
                        op: String::new(),
                        sat: String::new(),
                        sat_fm: false,
                    },
                    1,
                );
            }
            let lines = |cab: &str| -> Vec<String> {
                cab.lines()
                    .filter(|l| l.starts_with("QSO:"))
                    .map(str::to_string)
                    .collect()
            };
            let mine = lines(&own.cabrillo(0).unwrap());
            let club_cab = club
                .export_cabrillo_with(
                    "W9XYZ",
                    party_session(event, state, county),
                    &CabrilloEntrant::default(),
                )
                .unwrap();
            assert_eq!(lines(&club_cab), mine, "{event}: the club's QSO lines");
            assert_eq!(mine.len(), 2, "{event}: both contacts");
            for v in theirs {
                assert!(club_cab.contains(v), "{event}: {v} on a line: {club_cab}");
            }
            let token = tempo_core::contest::cabrillo_contest_token(rs.contest_id).to_string();
            assert!(
                club_cab.contains(&format!("CONTEST: {token}\n")),
                "{event}: the contest's own token: {club_cab}"
            );
            seen.insert(*event);
        }
        // Every ruleset the club log runs is in the table above, Field Day aside (its
        // goldens pin it byte for byte) — a contest added to the seed must be run here.
        for event in seeded_events() {
            if club_refusal(party(&event)).is_none() && !is_field_day_id(&event) {
                assert!(
                    seen.contains(event.as_str()),
                    "{event} is run by the club log and not tested here"
                );
            }
        }
    }

    /// ⭐ **The screen mirrors the contests club sync refuses.** `CLUB_SYNC_REFUSED` in
    /// ui/src/fdEvent.ts must hold, for every seeded ruleset the club log cannot run, the
    /// reason [`club_refusal`] gives — and nothing else — or the screen and the sockets
    /// disagree: a "Not syncing" note over a club that runs, or silence over one that does
    /// not.
    #[test]
    fn the_screen_mirrors_the_contests_club_sync_refuses() {
        let ts = include_str!("../../../ui/src/fdEvent.ts");
        let block = ts
            .split("export const CLUB_SYNC_REFUSED")
            .nth(1)
            .expect("the table is in fdEvent.ts")
            .split("\n}")
            .next()
            .unwrap();
        let mirrored: HashMap<String, String> = block
            .lines()
            .filter_map(|l| {
                let (id, why) = l.trim().split_once(':')?;
                let id = id.trim();
                let ok = !id.is_empty()
                    && id
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
                ok.then(|| {
                    let why = why.trim().trim_end_matches(',').trim_matches('\'');
                    (id.to_string(), why.to_string())
                })
            })
            .collect();
        assert!(
            mirrored.len() >= 8,
            "parsed only {} entries — the parser is broken, not the table",
            mirrored.len()
        );
        let engine: HashMap<String, String> = seeded_events()
            .into_iter()
            .filter_map(|e| club_refusal(party(&e)).map(|r| (e, r.code().to_string())))
            .collect();
        assert_eq!(mirrored, engine, "ui/src/fdEvent.ts CLUB_SYNC_REFUSED");
        // POSITIVE CONTROL: the comparison sees one entry missing.
        let mut short = mirrored.clone();
        short.remove("cqp");
        assert_ne!(short, engine);
    }

    /// ⭐ **The log strip builds the key the engine builds** —
    /// `ui/src/features/__fixtures__/contest-dupe-keys.json` is one table of contacts and
    /// their dupe keys, read here against the engine's own rule (from the rules seed) and its
    /// own key builder, and in `contestDupe.keys.shared.test.ts` against the strip's. The
    /// table's rules must be the seed's too, or the strip would be checked against a rule
    /// no contest runs. The club's while-typing warning compares exactly these keys.
    #[test]
    fn the_strips_dupe_key_table_is_the_engines() {
        let table: serde_json::Value = serde_json::from_str(include_str!(
            "../../../ui/src/features/__fixtures__/contest-dupe-keys.json"
        ))
        .expect("the table is JSON");
        let strs = |v: &serde_json::Value| -> Vec<String> {
            v.as_array()
                .expect("a list")
                .iter()
                .map(|s| s.as_str().expect("a string").to_string())
                .collect()
        };
        let rules = table["rules"].as_object().expect("rules");
        for (event, r) in rules {
            let d = party(event).dupe_rule;
            assert_eq!(r["byCall"].as_bool(), Some(d.by_call), "{event} byCall");
            assert_eq!(r["byBand"].as_bool(), Some(d.by_band), "{event} byBand");
            assert_eq!(
                r["byModeClass"].as_bool(),
                Some(d.by_mode_class),
                "{event} byModeClass"
            );
            assert_eq!(
                r["logDupes"].as_bool(),
                Some(d.log_dupes),
                "{event} logDupes"
            );
            assert_eq!(strs(&r["byFields"]), d.by_fields, "{event} byFields");
            assert_eq!(
                strs(&r["bySentFields"]),
                d.by_sent_fields,
                "{event} bySentFields"
            );
            let groups: Vec<Vec<String>> = r["modeClassGroups"]
                .as_array()
                .expect("groups")
                .iter()
                .map(&strs)
                .collect();
            let seed: Vec<Vec<String>> = d
                .mode_class_groups
                .iter()
                .map(|g| g.iter().map(|m| m.to_string()).collect())
                .collect();
            assert_eq!(groups, seed, "{event} modeClassGroups");
        }
        let rows = table["rows"].as_array().expect("rows");
        assert!(rows.len() >= 9, "parsed only {} rows", rows.len());
        for row in rows {
            let event = row["ruleset"].as_str().unwrap();
            let rs = party(event);
            let side = |v: &serde_json::Value| -> Vec<FieldValue> {
                v.as_object()
                    .expect("an exchange")
                    .iter()
                    .filter_map(|(k, raw)| rs.exchange.copied(k, raw.as_str().unwrap()))
                    .collect()
            };
            let key = rs.dupe_rule.key_of(
                row["call"].as_str().unwrap(),
                row["band"].as_str().unwrap(),
                row["mode"].as_str().unwrap(),
                &side(&row["rx"]),
                &side(&row["tx"]),
                tempo_core::contest::SatKey::default(),
            );
            assert_eq!(key, strs(&row["key"]), "{event} row {row}");
        }
    }

    /// Every contest the bundled rules table carries, by rules-file id, read from the
    /// seed itself so a contest added there is seen here.
    fn seeded_events() -> Vec<String> {
        let seed: serde_json::Value =
            serde_json::from_str(include_str!("../../tempo-core/src/fd_rules.seed.json"))
                .expect("the seed is JSON");
        let events: Vec<String> = seed["rulesets"]
            .as_array()
            .expect("the seed lists rulesets")
            .iter()
            .filter_map(|r| r["event"].as_str().map(str::to_string))
            .collect();
        assert!(events.len() >= 18, "parsed only {} rulesets", events.len());
        events
    }

    /// ⭐ **The contests the club log cannot run, and why** — decided from each ruleset's
    /// own data. A serial-number exchange must run in one sequence for the whole entry;
    /// a template with a transmitter column must say which transmitter made each
    /// contact. Everything else runs.
    #[test]
    fn the_contests_club_sync_refuses_are_the_serial_and_transmitter_ones() {
        use ClubRefusal::*;
        let expect: &[(&str, Option<ClubRefusal>)] = &[
            ("arrlfd", None),
            ("wfd", None),
            ("ilqp", None),
            ("tnqp", None),
            ("ohqp", None),
            ("txqp", None),
            ("nyqp", None),
            ("arrlvhf_jan", None),
            ("arrlvhf_jun", None),
            ("arrlvhf_sep", None),
            ("cqp", Some(Serial)),
            ("arrlss_cw", Some(Serial)),
            ("arrlss_ssb", Some(Serial)),
            ("cqwpx_cw", Some(Serial)),
            ("cqwpx_ssb", Some(Serial)),
            ("cqww_cw", Some(TransmitterColumn)),
            ("cqww_ssb", Some(TransmitterColumn)),
            ("cqww_rtty", Some(TransmitterColumn)),
        ];
        for (event, want) in expect {
            assert_eq!(club_refusal(party(event)), *want, "{event}");
        }
    }

    /// ⭐ **A position logging a different contest is refused at JOIN, by name** — the
    /// club and the position's contest both named, and what to do about it. A JOIN naming
    /// none is served by a Field Day club as it always was, and refused by any other; a
    /// position too old to name one is refused by its version first.
    #[test]
    fn a_position_logging_another_contest_is_refused_by_name() {
        let v = tempo_net::fdsync::PROTO_VERSION;
        let party_club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        assert_eq!(
            party_club.join_refusal(v, "ilqp"),
            None,
            "same contest: served"
        );
        let msg = party_club
            .join_refusal(v, "arrlfd")
            .expect("a Field Day position is refused by a party club");
        assert_eq!(
            msg,
            "this club is running IL QSO Party, and this Nexus is logging ARRL-FIELD-DAY. \
             Pick IL QSO Party as the contest on the Contesting tab in Settings, then \
             rejoin. Contacts you log meanwhile stay in your own log."
        );
        let old = party_club
            .join_refusal(v, "")
            .expect("a position too old to name its contest cannot join a party club");
        assert!(
            old.contains("IL QSO Party") && old.contains("too old"),
            "{old}"
        );
        // Field Day: a JOIN naming no contest is served as before; the OTHER Field Day is not.
        let fd = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        assert_eq!(
            fd.join_refusal(v, ""),
            None,
            "a JOIN naming no contest joins Field Day as before"
        );
        assert_eq!(
            fd.join_refusal(1, ""),
            fd.version_refusal(1),
            "…and a v1 position is turned away by its version"
        );
        assert_eq!(fd.join_refusal(v, "arrlfd"), None);
        let wfd = fd
            .join_refusal(v, "wfd")
            .expect("Winter Field Day is another contest");
        assert!(
            wfd.contains("ARRL-FIELD-DAY") && wfd.contains("WFD"),
            "{wfd}"
        );
        // The version rule still comes first, with its own words.
        assert_eq!(
            party_club.join_refusal(1, "ilqp"),
            party_club.version_refusal(1)
        );
    }

    /// ⭐ **A position on another station call than the host's is refused at JOIN, by name**
    /// — both calls named, and where to set the position's. The calls are compared trimmed
    /// and case-blind, and nothing more: a portable suffix is another call on the air. A
    /// call either end cannot say is served as before.
    #[test]
    fn a_position_on_another_call_is_refused_by_name_with_where_to_set_it() {
        let party_club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        assert_eq!(
            party_club.call_refusal("W9XYZ", " k9abc "),
            Some(
                "this club is on the air as W9XYZ and this Nexus as K9ABC, and in IL QSO Party \
                 every position of a club entry sends the club's call. Set Callsign on the air \
                 under Who's who at this event on the Contesting tab in Settings to W9XYZ, then \
                 turn Field Day mode off and on again: this Nexus rejoins by itself. Contacts \
                 you log meanwhile stay in your own log."
                    .to_string()
            )
        );
        let portable = party_club
            .call_refusal("W9XYZ", "W9XYZ/P")
            .expect("W9XYZ/P is not the club's call on the air");
        assert!(portable.contains("this Nexus as W9XYZ/P,"), "{portable}");
        // CONTROLS: the club's own call joins however it is typed, and a call either end
        // cannot say joins as before.
        assert_eq!(party_club.call_refusal("W9XYZ", "W9XYZ"), None);
        assert_eq!(party_club.call_refusal("W9XYZ", " w9xyz "), None);
        assert_eq!(party_club.call_refusal("W9XYZ", ""), None);
        assert_eq!(party_club.call_refusal("W9XYZ", "  "), None);
        assert_eq!(party_club.call_refusal("", "K9ABC"), None);
    }

    /// ⭐ **Only ARRL Field Day serves a position on another call** — its GOTA station must
    /// use a call of its own (rule 4.1.1.1). Every other contest the club log runs refuses
    /// one, each by its own name: Winter Field Day, the QSO parties and the VHF contests.
    #[test]
    fn only_arrl_field_day_serves_a_position_on_another_call() {
        let (mut served, mut refused) = (Vec::new(), Vec::new());
        for event in seeded_events() {
            let rs = party(&event);
            if club_refusal(rs).is_some() {
                continue;
            }
            match ClubLog::for_ruleset(rs, "TEST").call_refusal("W9XYZ", "K9GOT") {
                None => served.push(event),
                Some(msg) => {
                    assert!(
                        msg.contains(&format!("in {} every", rs.contest_id)),
                        "{event}: {msg}"
                    );
                    refused.push(event);
                }
            }
        }
        assert_eq!(served, vec!["arrlfd".to_string()]);
        for event in ["wfd", "ilqp", "nyqp", "tnqp", "ohqp", "txqp", "arrlvhf_jun"] {
            assert!(refused.iter().any(|e| e == event), "{event}: {refused:?}");
        }
    }

    /// ⭐ **Field Day through the any-ruleset constructor is the Field Day club it always
    /// was** — the 1.x journal replayed into a club built by `for_ruleset` exports the
    /// bytes 1.x exported, and the generic score is the shipped one.
    #[test]
    fn a_field_day_club_built_for_its_ruleset_is_byte_identical_to_1x() {
        let dir = scratch("j1x-ruleset");
        let path = dir.join("fd_event_granite.jsonl");
        std::fs::write(&path, J1X).unwrap();
        let mut host = ClubLog::for_ruleset(party("arrlfd"), "GRANITE ARC FD");
        assert_eq!(host.field_day_event(), Some(FdEvent::ArrlFd));
        host.attach_journal_since(&path, 0).unwrap();
        assert_eq!(host.export_cabrillo("W9ABC", "3A", "WI").unwrap(), J1X_CBR);
        assert_eq!(host.export_adif("W9ABC", "3A", "WI"), J1X_ADI);
        let session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        assert_eq!(
            host.export_cabrillo_with(
                "W9ABC",
                session.clone(),
                &CabrilloEntrant {
                    name: "A NAME".into(),
                    email: "a@example.com".into(),
                    club: "A CLUB".into(),
                    entry_class: "UNLIMITED".into(),
                    operators: "W9OP1".into(),
                    objectives: vec!["wfd-qrp".into()],
                },
            )
            .unwrap(),
            J1X_CBR,
            "the entrant's lines are written only where the ruleset lists them — never for Field Day"
        );
        assert_eq!(host.export_adif_with("W9ABC", session.clone()), J1X_ADI);
        let s = host.score_with("W9ABC", session, 2, &[], &[]);
        assert_eq!(
            (s.qso_points, s.powered, s.bonus, s.total),
            host.scored("W9ABC", "3A", "WI", 2, &[]),
        );
        assert_eq!(s.mults, None, "Field Day has no multiplier");
        let wfd = ClubLog::for_ruleset(party("wfd"), "WFD");
        assert_eq!(
            (wfd.event, wfd.contest_id.as_str()),
            (FdEvent::WinterFd, "WFD")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 1700Z, Saturday 27 June 2026 — ARRL Field Day's first minute, as the 1.x fixture's.
    const FD_START: u64 = 1_782_579_600;

    /// The `QSO:` lines of a Cabrillo file, in order.
    fn qso_lines(cab: &str) -> Vec<&str> {
        cab.lines().filter(|l| l.starts_with("QSO:")).collect()
    }

    /// ⭐ **An ARRL Field Day club's file says which station made each contact: a GOTA
    /// position's QSO lines carry the GOTA station's own call, and every other line the
    /// club's.**
    ///
    /// ARRL Field Day rules (2026): the GOTA station *"must use a different callsign from
    /// the primary Field Day station"* and *"uses the same exchange as its parent"*
    /// (4.1.1.1), and *"QSOs made by this station may be claimed for credit by its primary
    /// Field Day operation"* (4.1.1.5). A Cabrillo QSO line's first call is the one SENT,
    /// and the club's file, written under the host's call, wrote every GOTA contact as one
    /// the primary station made.
    #[test]
    fn a_gota_positions_lines_carry_its_own_call_and_every_other_line_the_clubs() {
        let session = || ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        let entrant = CabrilloEntrant::default();
        let dir = scratch("gota");
        // The host and a second tent on the club's call (the tent's as typed), and the GOTA
        // tent on `gota_call`; every row sends the club's 3A WI, as the GOTA station must.
        let club_with = |gota_call: &str, journal: &PathBuf| {
            let mut club = ClubLog::for_ruleset(party("arrlfd"), "GOTA FD");
            club.attach_journal_since(journal, 0).unwrap();
            club.join("aaaa0001", "HQ", "W9XYZ", 1);
            club.join("bbbb0002", "CW tent", " w9xyz ", 1);
            club.join("cccc0003", "GOTA", gota_call, 1);
            for (pos, seq, call, band, mode, sect, minute) in [
                ("aaaa0001", 1, "W1AW", "20m", "PH", "CT", 0),
                ("cccc0003", 1, "K1ABC", "40m", "CW", "EMA", 1),
                ("bbbb0002", 1, "N0XYZ", "15m", "CW", "MN", 2),
                ("cccc0003", 2, "W5DEF", "20m", "DIG", "STX", 3),
            ] {
                let when = FD_START + 60 * minute;
                let row = with_sent(wq(pos, seq, call, band, mode, sect, when), ("3A", "WI"));
                club.merge(&row, when);
            }
            club
        };
        let journal = dir.join("fd_event_gota.jsonl");
        let club = club_with("K9GOT", &journal);
        let cab = club
            .export_cabrillo_with("W9XYZ", session(), &entrant)
            .unwrap();
        assert!(
            cab.contains("\nCALLSIGN: W9XYZ\n"),
            "the club's entry: {cab}"
        );
        assert_eq!(
            qso_lines(&cab),
            [
                "QSO: 14000 PH 2026-06-27 1700 W9XYZ 3A WI W1AW 2A CT",
                "QSO: 7000 CW 2026-06-27 1701 K9GOT 3A WI K1ABC 2A EMA",
                "QSO: 21000 CW 2026-06-27 1702 W9XYZ 3A WI N0XYZ 2A MN",
                "QSO: 14000 DG 2026-06-27 1703 K9GOT 3A WI W5DEF 2A STX",
            ],
            "the GOTA tent's two contacts under its own call, the rest under the club's"
        );
        // A host restart replays the journal before any position has joined again, so the
        // call has to come back with the rows.
        let mut restarted = ClubLog::for_ruleset(party("arrlfd"), "GOTA FD");
        restarted.attach_journal_since(&journal, 0).unwrap();
        assert_eq!(
            restarted
                .export_cabrillo_with("W9XYZ", session(), &entrant)
                .unwrap(),
            cab,
            "the replayed club writes the same file, with no position joined"
        );
        // The club's own lines are written under the club's call exactly as given, however
        // the positions typed it.
        assert_eq!(
            qso_lines(
                &club
                    .export_cabrillo_with("w9xyz", session(), &entrant)
                    .unwrap()
            ),
            [
                "QSO: 14000 PH 2026-06-27 1700 w9xyz 3A WI W1AW 2A CT",
                "QSO: 7000 CW 2026-06-27 1701 K9GOT 3A WI K1ABC 2A EMA",
                "QSO: 21000 CW 2026-06-27 1702 w9xyz 3A WI N0XYZ 2A MN",
                "QSO: 14000 DG 2026-06-27 1703 K9GOT 3A WI W5DEF 2A STX",
            ]
        );

        // CONTROL: that tent on the club's call is no GOTA station, and every line is the
        // club's. The call column of those two lines is the whole difference between the
        // files, and the ADIF (which names no station on any record) and the score are the
        // same bytes and numbers: only how the Cabrillo prints moved.
        let control = club_with("W9XYZ", &dir.join("fd_event_control.jsonl"));
        let control_cab = control
            .export_cabrillo_with("W9XYZ", session(), &entrant)
            .unwrap();
        assert_eq!(
            qso_lines(&control_cab),
            [
                "QSO: 14000 PH 2026-06-27 1700 W9XYZ 3A WI W1AW 2A CT",
                "QSO: 7000 CW 2026-06-27 1701 W9XYZ 3A WI K1ABC 2A EMA",
                "QSO: 21000 CW 2026-06-27 1702 W9XYZ 3A WI N0XYZ 2A MN",
                "QSO: 14000 DG 2026-06-27 1703 W9XYZ 3A WI W5DEF 2A STX",
            ]
        );
        assert_eq!(cab.replace(" K9GOT ", " W9XYZ "), control_cab);
        assert_eq!(
            club.export_adif_with("W9XYZ", session()),
            control.export_adif_with("W9XYZ", session())
        );
        assert_eq!(
            club.score_with("W9XYZ", session(), 2, &[], &[]),
            control.score_with("W9XYZ", session(), 2, &[], &[])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⭐ **CONTROLS: only ARRL Field Day writes the call a position joined on.** Winter Field
    /// Day, the QSO parties and the VHF contests have no GOTA station, and the engine refuses
    /// a position on another call at JOIN ([`ClubLog::call_refusal`]). A club that holds one
    /// anyway writes the file and the journal of the same club with that position on the
    /// club's call, byte for byte, and its QSO line is the position's own line, under the
    /// club's call. Run over every contest the club log runs but ARRL Field Day.
    #[test]
    fn no_other_contests_club_file_or_journal_carries_the_call_a_position_joined_on() {
        // (contest, the host's session, the exchange the station worked sent)
        let cases = vec![
            (
                "wfd",
                ContestSession::field_day(FdEvent::WinterFd, "3O", "WI"),
                vec![("CLASS", "2O"), ("SECTION", "CT")],
            ),
            (
                "ilqp",
                party_session("ilqp", "IL", "MCLN"),
                vec![("RST", "599"), ("QTH", "COOK")],
            ),
            (
                "tnqp",
                party_session("tnqp", "TN", "ANDE"),
                vec![("RST", "599"), ("QTH", "BEDF")],
            ),
            (
                "ohqp",
                party_session("ohqp", "OH", "ADAM"),
                vec![("RST", "599"), ("QTH", "ALLE")],
            ),
            (
                "txqp",
                party_session("txqp", "TX", "ANDE"),
                vec![("RST", "599"), ("QTH", "ANDR")],
            ),
            (
                "nyqp",
                party_session("nyqp", "NY", "ALB"),
                vec![("RST", "599"), ("QTH", "BRX")],
            ),
            (
                "arrlvhf_jan",
                party_session("arrlvhf_jan", "", ""),
                vec![("GRID", "FN31")],
            ),
            (
                "arrlvhf_jun",
                party_session("arrlvhf_jun", "", ""),
                vec![("GRID", "FN31")],
            ),
            (
                "arrlvhf_sep",
                party_session("arrlvhf_sep", "", ""),
                vec![("GRID", "FN31")],
            ),
        ];
        for (event, session, fields) in &cases {
            let rs = party(event);
            let dir = scratch(&format!("call-{event}"));
            let fields: Vec<(String, String)> = fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let mut own = FieldDayLog::new("W9XYZ", session.clone(), "20m");
            assert!(
                own.log_fields_at("K9AAA", &fields, "CW", "", 0, ILQP_START),
                "{event}: fixture contact refused"
            );
            let q = &own.qsos()[0];
            // The outbox's own row shape (`Engine::fd_sync_outbox`).
            let row = WireQso {
                pos: "aaaa0001".into(),
                seq: q.seq,
                call: q.call.clone(),
                class: q.class().to_string(),
                sect: q.section().to_string(),
                ex: to_wire_fields(&q.rx),
                mex: to_wire_fields(&q.tx),
                band: q.band.clone(),
                mode: q.mode.clone(),
                sub: q.submode.clone(),
                when: q.when_unix,
                op: String::new(),
                sat: String::new(),
                sat_fm: false,
            };
            let joined_on = |call: &str| {
                let journal = dir.join(format!("{call}.jsonl"));
                let mut club = ClubLog::for_ruleset(rs, "TEST");
                club.attach_journal_since(&journal, 0).unwrap();
                club.join("aaaa0001", "", call, 1);
                club.merge(&row, 1);
                let cab = club
                    .export_cabrillo_with("W9XYZ", session.clone(), &CabrilloEntrant::default())
                    .unwrap();
                (cab, std::fs::read_to_string(&journal).unwrap())
            };
            let (cab, journal) = joined_on("K9GOT");
            let (club_cab, club_journal) = joined_on("W9XYZ");
            assert_eq!(cab, club_cab, "{event}: the club's file");
            assert_eq!(journal, club_journal, "{event}: the club's journal");
            assert_eq!(
                qso_lines(&cab),
                qso_lines(&own.cabrillo(0).unwrap()),
                "{event}: the position's own line"
            );
            assert!(
                qso_lines(&cab)[0].contains(" W9XYZ ") && !cab.contains("K9GOT"),
                "{event}: {cab}"
            );
            assert!(!journal.contains("K9GOT"), "{event}: {journal}");
            let _ = std::fs::remove_dir_all(&dir);
        }
        // Every contest the club log runs is above, but ARRL Field Day: a contest added to
        // the seed must be run here.
        for event in seeded_events() {
            if club_refusal(party(&event)).is_none() && event != "arrlfd" {
                assert!(
                    cases.iter().any(|(e, _, _)| *e == event),
                    "{event} is run by the club log and not tested here"
                );
            }
        }
    }

    // ---- nothing a peer sends can open a line of the club's files -------------------

    /// The ways a string off the network could open a line of a club file, or break what shows
    /// it, each behind a value that would otherwise pass: a line feed and a carriage return
    /// before a QSO line of its own, a control character, ADIF's record end, Cabrillo's last
    /// line as a word, an invisible character (the value reads as itself and is not), and a
    /// value longer than any a club's file holds (Remote takes no string past 1024 bytes).
    fn breakers(value: &str) -> Vec<(&'static str, String)> {
        let line = "QSO: 7000 CW 2026-06-27 1702 W9XYZ 3A WI K1FAK 1A CT";
        vec![
            ("LF", format!("{value}\n{line}")),
            ("CR", format!("{value}\r{line}")),
            ("control", format!("{value}\u{1b}")),
            ("ADIF token", format!("{value}<EOR><CALL:5>K1FAK<EOR>")),
            ("Cabrillo token", format!("{value} END-OF-LOG:")),
            ("outside ASCII", format!("{value}\u{200B}")),
            ("too long", format!("{value}{}", "9".repeat(2048))),
        ]
    }

    /// The shape of a club's two files: each Cabrillo line's tag, how many record ends each
    /// line of the ADIF's records holds, and how many characters either file holds that end or
    /// garble a line (a control character but the line end, a line or paragraph separator).
    fn shape(cab: &str, adif: &str) -> (Vec<String>, Vec<usize>, usize) {
        let tags = cab
            .lines()
            .map(|l| l.split(':').next().unwrap_or("").to_string())
            .collect();
        let records = adif
            .lines()
            .skip_while(|l| !l.starts_with("<EOH>"))
            .skip(1)
            .map(|l| l.matches("<EOR>").count())
            .collect();
        let stray = cab
            .chars()
            .chain(adif.chars())
            .filter(|c| (c.is_control() && *c != '\n') || matches!(c, '\u{2028}' | '\u{2029}'))
            .count();
        (tags, records, stray)
    }

    /// ⭐ **A position whose Callsign on the air is not a call sign is turned away by name, at
    /// every club** — ARRL Field Day too, where a position on any CALL SIGN joins (its GOTA
    /// station must use one of its own), so a JOIN's call reached the club's file there
    /// unchecked. CONTROLS: ARRL Field Day still serves a call sign of its own, as typed or
    /// not; every club serves the club's own call and a position that names none; and
    /// elsewhere a call sign that is not the club's is refused as before, naming both.
    #[test]
    fn a_position_whose_call_is_not_a_call_sign_is_refused_by_name_at_every_club() {
        let mut wrong = Vec::new();
        for event in ["arrlfd", "wfd", "ilqp"] {
            let club = ClubLog::for_ruleset(party(event), "TEST");
            let odd = [
                ("a space", "K9 GOT".to_string()),
                ("no digit", "GOTA".to_string()),
                ("16 characters", "VP2E/K9GOTABCD/P".to_string()),
            ];
            for (how, call) in breakers("K9GOT").into_iter().chain(odd) {
                let got = club.call_refusal("W9XYZ", &call);
                if got.as_deref() != Some(NOT_A_CALL_SIGN) {
                    wrong.push(format!("{event}, {how}: {got:?}"));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "served, or refused for something else:\n{}",
            wrong.join("\n")
        );
        let fd = ClubLog::for_ruleset(party("arrlfd"), "TEST");
        for call in ["K9GOT", " k9got ", "VP2E/K9GOT/P", "W9XYZ", ""] {
            assert_eq!(fd.call_refusal("W9XYZ", call), None, "arrlfd {call:?}");
        }
        for event in ["wfd", "ilqp"] {
            let club = ClubLog::for_ruleset(party(event), "TEST");
            for call in ["W9XYZ", " w9xyz ", ""] {
                assert_eq!(club.call_refusal("W9XYZ", call), None, "{event} {call:?}");
            }
            assert_eq!(
                club.call_refusal("W9XYZ", " k9got "),
                Some(call_mismatch(&club.contest_id, "W9XYZ", "K9GOT")),
                "{event}"
            );
        }
    }

    /// ⭐ **The call a row is stamped with is the one its position's JOIN was let in on**,
    /// normalised exactly as the gate reads it, and nothing else: never a call that is not a
    /// call sign (the engine turns that JOIN away, and one that reaches the log anyway keeps
    /// no call), and never an EARLIER join's call when the latest named none. CONTROL: a call
    /// sign is stamped as the gate read it and written as the call sent.
    #[test]
    fn the_call_a_row_is_stamped_with_is_the_one_its_join_was_let_in_on() {
        let session = || ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        let file = |club: &ClubLog| {
            club.export_cabrillo_with("W9XYZ", session(), &CabrilloEntrant::default())
                .unwrap()
        };
        let row = with_sent(
            wq("cccc0003", 1, "K1ABC", "40m", "CW", "EMA", FD_START + 60),
            ("3A", "WI"),
        );
        let clubs_line = "QSO: 7000 CW 2026-06-27 1701 W9XYZ 3A WI K1ABC 2A EMA";
        let mut stamped = Vec::new();
        for (how, call) in breakers("K9GOT") {
            let mut club = ClubLog::for_ruleset(party("arrlfd"), "GOTA FD");
            club.join("cccc0003", "GOTA", &call, 1);
            club.merge(&row, 1);
            let cab = file(&club);
            let kept = &club.positions()["cccc0003"].call;
            if !kept.is_empty()
                || !club.rows()[0].station_call.is_empty()
                || qso_lines(&cab) != [clubs_line]
            {
                stamped.push(format!(
                    "{how}: kept {kept:?}, stamped {:?}",
                    club.rows()[0].station_call
                ));
            }
        }
        let mut club = ClubLog::for_ruleset(party("arrlfd"), "GOTA FD");
        club.join("cccc0003", "GOTA", "K9GOT", 1);
        club.join("cccc0003", "GOTA", "", 2);
        club.merge(&row, 2);
        if !club.rows()[0].station_call.is_empty() {
            stamped.push(format!(
                "a rejoin naming no call: stamped {:?}",
                club.rows()[0].station_call
            ));
        }
        assert!(stamped.is_empty(), "{}", stamped.join("\n"));
        let mut club = ClubLog::for_ruleset(party("arrlfd"), "GOTA FD");
        club.join("cccc0003", "GOTA", " k9got ", 1);
        club.merge(&row, 1);
        assert_eq!(club.rows()[0].station_call, "K9GOT");
        assert_eq!(
            qso_lines(&file(&club)),
            ["QSO: 7000 CW 2026-06-27 1701 K9GOT 3A WI K1ABC 2A EMA"]
        );
    }

    /// ⭐ **A contact carrying what no line of the club's file can hold is kept out of the
    /// club's log, and the host's screen names it** — at the boundary where a peer's row
    /// enters the club log, before anything is keyed, journaled or written. Every string a
    /// row carries, in an ARRL Field Day club (QSO lines), a Winter Field Day club (its
    /// `X-EXCHANGE` and `OPERATORS` headers come off the rows) and an Illinois QSO Party club
    /// (`OPERATORS` straight off the rows).
    ///
    /// Kept out, the club's files are the bytes of the club that never received it, the ack
    /// does not move, and the host's list of kept-out contacts names the position. The one
    /// string that is let in is Cabrillo's last line as a WORD inside a free-text value: it
    /// opens no line, so it is merged and every line is still the one the writer opened (a
    /// call is a call sign, so there it is kept out like the rest).
    #[test]
    fn a_contact_that_could_open_a_line_of_the_club_file_is_kept_out_by_name() {
        let wfd = |q: WireQso| WireQso {
            ex: fd_fields(FdEvent::WinterFd, "2O", &q.sect.clone()),
            mex: fd_fields(FdEvent::WinterFd, "3O", "WI"),
            ..q
        };
        let fd = |q: WireQso| with_sent(q, ("3A", "WI"));
        // (contest, the host's session, the club's own contact, the peer's contact)
        let clubs: Vec<(&str, ContestSession, WireQso, WireQso)> = vec![
            (
                "arrlfd",
                ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
                fd(wq("aaaa0001", 1, "W1AW", "20m", "PH", "CT", FD_START)),
                fd(wq(
                    "bbbb0002",
                    1,
                    "K1ABC",
                    "40m",
                    "CW",
                    "EMA",
                    FD_START + 60,
                )),
            ),
            (
                "wfd",
                ContestSession::field_day(FdEvent::WinterFd, "3O", "WI"),
                wfd(wq("aaaa0001", 1, "W1AW", "20m", "PH", "CT", FD_START)),
                wfd(wq(
                    "bbbb0002",
                    1,
                    "K1ABC",
                    "40m",
                    "CW",
                    "EMA",
                    FD_START + 60,
                )),
            ),
            (
                "ilqp",
                party_session("ilqp", "IL", "MCLN"),
                ilqp_row(
                    "aaaa0001", 1, "W1AW", "40m", "CW", "", "COOK", "MCLN", ILQP_START,
                ),
                ilqp_row(
                    "bbbb0002",
                    1,
                    "K1ABC",
                    "20m",
                    "CW",
                    "",
                    "WILL",
                    "MCLN",
                    ILQP_START + 60,
                ),
            ),
        ];
        type Field = fn(&mut WireQso) -> &mut String;
        let fields: [(&str, Field); 12] = [
            ("call", |q| &mut q.call),
            ("copied value", |q| &mut q.ex[0].r),
            ("copied slot", |q| &mut q.ex[0].k),
            ("copied domain", |q| &mut q.ex[0].d),
            ("sent value", |q| &mut q.mex[0].r),
            ("legacy class", |q| {
                q.ex.clear();
                &mut q.class
            }),
            ("legacy section", |q| {
                q.ex.clear();
                &mut q.sect
            }),
            ("band", |q| &mut q.band),
            ("mode", |q| &mut q.mode),
            // A digital contact, so the value a breaker rides behind is a real one.
            ("submode", |q| {
                q.mode = "DIG".into();
                q.sub = "RTTY".into();
                &mut q.sub
            }),
            ("operator", |q| &mut q.op),
            // A contact through a bird on both sides of the comparison, so a contest that
            // gives a satellite no credit leaves it out of both files alike.
            ("satellite", |q| {
                q.sat = "AO-91".into();
                &mut q.sat
            }),
        ];
        let entrant = CabrilloEntrant {
            operators: "W9OP1".into(),
            ..Default::default()
        };
        let mut wrong = Vec::new();
        for (event, session, own, theirs) in &clubs {
            let fresh = || {
                let mut club = ClubLog::for_ruleset(party(event), "TEST");
                club.join("aaaa0001", "HQ", "W9XYZ", 1);
                club.join("bbbb0002", "CW tent", "W9XYZ", 1);
                club.merge(own, 1);
                club
            };
            let files = |club: &ClubLog| {
                club.export_cabrillo_with("W9XYZ", session.clone(), &entrant)
                    .map(|cab| (cab, club.export_adif_with("W9XYZ", session.clone())))
            };
            let without = files(&fresh()).expect("the club's own contact exports");
            for (field, at) in fields {
                let mut clean = theirs.clone();
                let base = at(&mut clean).clone();
                let mut kept = fresh();
                kept.merge(&clean, 2);
                let clean_files = files(&kept);
                for (how, bad) in breakers(&base) {
                    let mut q = theirs.clone();
                    *at(&mut q) = bad;
                    let mut club = fresh();
                    let ack = club.merge(&q, 2);
                    let merged = club.rows().len() == 2;
                    let case = format!("{event}, {field}, {how}");
                    let opens = merged
                        && match (files(&club), &clean_files) {
                            (Ok((cab, adif)), Ok((c, a))) => shape(&cab, &adif) != shape(c, a),
                            _ => false,
                        };
                    if how == "Cabrillo token" && field != "call" {
                        if !merged || opens {
                            wrong.push(format!("{case}: merged {merged}, opens a line {opens}"));
                        }
                        continue;
                    }
                    let named = club.kept_out().iter().any(|k| {
                        k.label == "CW tent"
                            && k.call == "W9XYZ"
                            && k.reason.contains("is not in the club's log")
                    });
                    if merged || ack != 0 || files(&club).ok() != Some(without.clone()) || !named {
                        wrong.push(format!(
                            "{case}: merged {merged}{}, acked {ack}, named {named}",
                            if opens { ", OPENS A LINE" } else { "" }
                        ));
                    }
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} cases:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// The host's journal holds each row on its own line whatever the row's strings hold: JSON
    /// writes a line feed, a carriage return and every other control character escaped, and
    /// the replay reads back the very row. Not a way in, so the boundary above is the only
    /// guard the journal needs; pinned so it stays that way.
    #[test]
    fn the_journal_keeps_every_row_on_its_own_line_whatever_its_strings_hold() {
        let dir = scratch("journal-breakers");
        let path = dir.join("fd_event_breakers.jsonl");
        let mut club = ClubLog::for_ruleset(party("arrlfd"), "TEST");
        club.attach_journal_since(&path, 0).unwrap();
        let mut row = MergedRow::from_wire(&with_sent(
            wq("aaaa0001", 1, "K1ABC", "40m", "CW", "EMA", FD_START),
            ("3A", "WI"),
        ));
        let tail = "\n\r\u{1b}\u{2028}<EOR> END-OF-LOG:";
        for s in [
            &mut row.call,
            &mut row.band,
            &mut row.operator,
            &mut row.station_call,
            &mut row.ex[0].r,
            &mut row.mex[1].r,
        ] {
            s.push_str(tail);
        }
        assert!(club.merge_row(row.clone(), 1));
        let journal = std::fs::read_to_string(&path).unwrap();
        assert_eq!(journal.lines().count(), 1, "{journal}");
        let mut replayed = ClubLog::for_ruleset(party("arrlfd"), "TEST");
        replayed.attach_journal_since(&path, 0).unwrap();
        assert_eq!(replayed.rows(), club.rows());
        assert_eq!(replayed.rows()[0], row);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- one reading of every peer string, wherever it is read -----------------------

    /// Calls that a reader that is not ASCII-only reads as the call they imitate, and none is
    /// it: `(how, the club's call, the call as sent, how the host's list must show it)`. The
    /// fold is the one `to_uppercase` makes and an ASCII compare does not (`ß` → `SS`).
    fn lookalikes() -> Vec<(&'static str, &'static str, String, &'static str)> {
        vec![
            ("full-width", "W9XYZ", "Ｗ９ＸＹＺ".into(), "?????"),
            ("Cyrillic", "W9XYZ", "W9\u{425}YZ".into(), "W9?YZ"),
            ("zero-width", "W9XYZ", "W9X\u{200B}YZ".into(), "W9X?YZ"),
            ("byte-order mark", "W9XYZ", "\u{FEFF}W9XYZ".into(), "?W9XYZ"),
            ("case fold", "W9ASS", "w9aß".into(), "W9A?"),
            ("NUL", "W9XYZ", "W9XYZ\0".into(), "W9XYZ?"),
            (
                "ideographic space",
                "W9XYZ",
                "\u{3000}W9XYZ".into(),
                "?W9XYZ",
            ),
            ("no-break space", "W9XYZ", "W9XYZ\u{A0}".into(), "W9XYZ?"),
        ]
    }

    /// ⭐ **A call is read ONE way: at the gate, in the stamp, at the merge and on the host's
    /// list.** ASCII only, a call sign's characters only, so no call compares equal at the gate
    /// and prints as something else, or prints as the club's own call while the gate refused it.
    /// Each lookalike is refused by name at every club, stamps nothing, is kept out as a row's
    /// call, and shows on the host's list with a `?` where the gate saw a character that is not
    /// a call sign's. CONTROLS: ASCII spacing around a call (a tab, a line end) is read away the
    /// same way everywhere.
    #[test]
    fn a_call_reads_one_way_at_the_gate_the_stamp_the_merge_and_the_hosts_list() {
        let mut wrong = Vec::new();
        for (how, club_call, call, shown) in lookalikes() {
            for event in ["arrlfd", "wfd", "ilqp"] {
                let got = ClubLog::for_ruleset(party(event), "TEST").call_refusal(club_call, &call);
                if got.as_deref() != Some(NOT_A_CALL_SIGN) {
                    wrong.push(format!("{how}, {event} gate: {got:?}"));
                }
            }
            let mut club = ClubLog::for_ruleset(party("arrlfd"), "TEST");
            club.join("cccc0003", "GOTA", &call, 1);
            let stamped = club.positions()["cccc0003"].call.clone();
            if !stamped.is_empty() {
                wrong.push(format!("{how}, stamp: {stamped:?}"));
            }
            let mut row = with_sent(
                wq("bbbb0002", 1, "K1ABC", "40m", "CW", "EMA", FD_START),
                ("3A", "WI"),
            );
            row.call = call.clone();
            club.merge(&row, 2);
            if !club.rows().is_empty() {
                wrong.push(format!("{how}, merge: {:?}", club.rows()[0].call));
            }
            club.note_refused("dddd0004", "", "TENT", &call, NOT_A_CALL_SIGN, 3);
            let listed = club
                .refused(3)
                .iter()
                .map(|r| r.call.clone())
                .collect::<Vec<_>>();
            if !listed.contains(&shown.to_string()) {
                wrong.push(format!("{how}, the host's list: {listed:?}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
        let mut club = ClubLog::for_ruleset(party("arrlfd"), "TEST");
        club.join("cccc0003", "GOTA", "\r\n k9got\t", 1);
        assert_eq!(club.positions()["cccc0003"].call, "K9GOT");
        assert_eq!(club.call_refusal(" w9xyz\t", "W9XYZ\n"), None);
        club.note_refused("dddd0004", "", "TENT", " k9abc\t", "refused", 3);
        assert!(club.refused(3).iter().any(|r| r.call == "K9ABC"));
    }

    /// ⭐ **A journal round trip gives back every value as the boundary read it** — the stamp,
    /// the row's call and its operator, all normalised before anything is journaled, so a
    /// restarted host reads exactly what the running one did and writes the same file.
    #[test]
    fn a_journal_round_trip_gives_back_every_value_as_the_boundary_read_it() {
        let dir = scratch("journal-normalised");
        let path = dir.join("fd_event_normalised.jsonl");
        let session = || ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        let file = |club: &ClubLog| {
            club.export_cabrillo_with("W9XYZ", session(), &CabrilloEntrant::default())
                .unwrap()
        };
        let mut club = ClubLog::for_ruleset(party("arrlfd"), "GOTA FD");
        club.attach_journal_since(&path, 0).unwrap();
        club.join("cccc0003", "GOTA", " k9got\t", 1);
        let mut row = with_sent(
            wq("cccc0003", 1, "W9ABC", "40m", "CW", "EMA", FD_START + 60),
            ("3A", "WI"),
        );
        row.call = "\t w9abc \r\n".into();
        row.op = " w9op ".into();
        club.merge(&row, 1);
        let read = |c: &ClubLog| {
            let r = &c.rows()[0];
            (r.call.clone(), r.station_call.clone(), r.operator.clone())
        };
        assert_eq!(read(&club), ("W9ABC".into(), "K9GOT".into(), "W9OP".into()));
        let mut replayed = ClubLog::for_ruleset(party("arrlfd"), "GOTA FD");
        replayed.attach_journal_since(&path, 0).unwrap();
        assert_eq!(replayed.rows(), club.rows(), "the very rows come back");
        assert_eq!(file(&replayed), file(&club));
        assert_eq!(
            qso_lines(&file(&club)),
            ["QSO: 7000 CW 2026-06-27 1701 K9GOT 3A WI W9ABC 2A EMA"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Does `s` hold nothing that could break a line or change what it reads as: no control
    /// character, no line or paragraph separator, no invisible or direction-changing character.
    fn plain(s: &str) -> bool {
        !s.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    '\u{2028}' | '\u{2029}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
                        | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
                )
        })
    }

    /// ⭐ **What the host shows and sends back is one line of plain text** — the name and the
    /// call on its list of turned-away positions (its screen and the hosted Remote page), and
    /// the contest or exchange role a refusal sentence repeats (sent back to that position,
    /// whose screen shows it). Each came off the network; a character that cannot be shown is
    /// a `?`, so the text still says where it was. CONTROL: an ordinary name and a known
    /// contest read as they always did.
    #[test]
    fn what_the_host_shows_and_sends_back_is_one_line_of_plain_text() {
        let tail = "\nTurned away HQ (W9XYZ).\r\u{1b}\u{202E}\u{200B}";
        let mut club = ClubLog::for_ruleset(party("arrlfd"), "TEST");
        club.note_refused(
            "aaaa0001",
            "",
            &format!("SSB tent{tail}"),
            &format!("K9GOT{tail}"),
            "refused",
            1,
        );
        let listed = club.refused(1)[0].clone();
        let contest = club
            .join_refusal(tempo_net::fdsync::PROTO_VERSION, &format!("ilqp{tail}"))
            .unwrap_or_default();
        let role = role_mismatch("IL QSO Party", "in_state", &format!("dx{tail}"));
        let shown = [
            ("label", listed.label.as_str()),
            ("call", listed.call.as_str()),
            ("contest sentence", contest.as_str()),
            ("role sentence", role.as_str()),
        ];
        let wrong: Vec<String> = shown
            .iter()
            .filter(|(_, s)| !plain(s))
            .map(|(what, s)| format!("{what}: {s:?}"))
            .collect();
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
        assert_eq!(listed.label, "SSB tent?Turned away HQ (W9XYZ).????");
        assert_eq!(listed.call, "K9GOT?TURNED?AWAY?HQ??W9XYZ??????");
        assert!(
            contest.contains("logging ilqp?Turned?away?HQ??W9XYZ"),
            "{contest}"
        );
        assert!(role.contains("the \"dx?Turned?away?HQ??W9XYZ"), "{role}");
        club.note_refused("bbbb0002", "", "Zelt Süd", "K9ABC", "refused", 1);
        assert!(club.refused(1).iter().any(|r| r.label == "Zelt Süd"));
        assert!(contest_mismatch("ARRL-FIELD-DAY", "ilqp").contains("logging IL QSO Party"));
    }

    /// ⭐ **A position's report reaches the board as the club reads it.** The board goes to
    /// every position, the host's TV and the hosted Remote page. A name keeps its letters, in
    /// any script, and shows anything invisible or line-breaking as a `?`; a band, a mode and
    /// an operator are ASCII, upper-cased the ASCII way (the club's file's: `ß` is not `SS`
    /// there), and one holding anything else, or longer than any club file's value, reads as
    /// nothing. CONTROLS: the club line that
    /// carries the board was never at risk (the wire is JSON), and an ordinary report reads as
    /// it always did.
    #[test]
    fn a_positions_report_reaches_the_board_as_the_club_reads_it() {
        let mut club = ClubLog::for_ruleset(party("arrlfd"), "TEST");
        club.join("aaaa0001", "HQ", "W9XYZ", 1);
        club.join("bbbb0002", "CW tent", "W9XYZ", 1);
        club.position_status(
            "aaaa0001",
            &report("Zelt Süd\n\u{202E}x", "20m\u{1b}", "cw\u{200B}", "straße"),
            2,
        );
        club.position_status("bbbb0002", &report("CW tent", "40m", "cw", "w9op"), 2);
        // Longer than Remote takes (1024 bytes): a whole view blanked, by one presence report.
        let long = "9".repeat(2048);
        club.join("cccc0003", "VHF tent", "W9XYZ", 1);
        club.position_status("cccc0003", &report("VHF tent", &long, &long, &long), 2);
        let line = tempo_net::fdsync::encode_line(&tempo_net::fdsync::Msg::Club(
            club.club_state(0, 0, 0, 2),
        ));
        assert_eq!(line.matches('\n').count(), 1, "{line}");
        let board: Vec<(String, String, String, String)> = club
            .board_rows(2)
            .into_iter()
            .map(|r| (r.name, r.band, r.mode, r.op))
            .collect();
        let row = |n: &str, b: &str, m: &str, o: &str| {
            (n.to_string(), b.to_string(), m.to_string(), o.to_string())
        };
        assert_eq!(
            board,
            [
                row("Zelt Süd??x", "", "", ""),
                row("CW tent", "40m", "CW", "W9OP"),
                row("VHF tent", "", "", "")
            ]
        );
        assert!(inert(&"9".repeat(MAX_VALUE_CHARS)) && !inert(&"9".repeat(MAX_VALUE_CHARS + 1)));
    }

    // ---- a position is taken only from the laptop that first joined as it --------------

    /// A key hash for one test: generated (a per-run seed through the operating system's
    /// hasher), so no test holds a real one, and the same `tag` the same hash all run.
    fn throwaway_hash(tag: &str) -> String {
        use std::hash::BuildHasher;
        static SEED: std::sync::OnceLock<std::hash::RandomState> = std::sync::OnceLock::new();
        let seed = SEED.get_or_init(std::hash::RandomState::new);
        (0..4u8)
            .map(|i| format!("{:016x}", seed.hash_one((tag, i))))
            .collect()
    }

    /// ⭐ **A position is held by the key it first joined with, across a host restart, and
    /// the journal holds that key's hash and nothing more.** The first JOIN served under a
    /// position id pins its key hash; a later JOIN under the id with another is turned away by
    /// name, and one with none is turned away too. A pinned position keeps its pin. The pin is
    /// one journal line, so a restarted host holds it; a previous event's pin pins nothing
    /// (the same cutoff as its rows), and an older Nexus's replay reads no pin line as a row.
    /// CONTROLS: the laptop that pinned a position rejoins, and another position pins its own.
    #[test]
    fn a_position_is_held_by_the_key_it_first_joined_with_across_a_restart() {
        let dir = scratch("pins");
        let path = dir.join("fd_event_pins.jsonl");
        let (mine, other) = (
            throwaway_hash("this laptop"),
            throwaway_hash("another laptop"),
        );
        let mut club = ClubLog::for_ruleset(party("arrlfd"), "TEST FD");
        club.attach_journal_since(&path, 0).unwrap();
        assert_eq!(
            club.key_refusal("aaaa0001", &mine),
            None,
            "nobody holds it yet"
        );
        club.pin("aaaa0001", &mine, 1_000);
        let held = Some(POSITION_HELD.to_string());
        assert_eq!(
            club.key_refusal("aaaa0001", &mine),
            None,
            "CONTROL: the laptop itself"
        );
        assert_eq!(club.key_refusal("aaaa0001", &other), held, "another laptop");
        assert_eq!(
            club.key_refusal("aaaa0001", ""),
            Some(NO_POSITION_KEY.to_string())
        );
        assert_eq!(
            club.key_refusal("bbbb0002", &other),
            None,
            "CONTROL: its own position"
        );
        club.pin("aaaa0001", &other, 1_001);
        assert_eq!(
            club.pinned("aaaa0001"),
            Some(mine.as_str()),
            "a pin is kept"
        );

        let journal = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            journal,
            format!("{{\"pin\":{{\"pos\":\"aaaa0001\",\"key_hash\":\"{mine}\",\"at\":1000}}}}\n")
        );
        assert!(
            serde_json::from_str::<MergedRow>(journal.trim_end()).is_err(),
            "an older Nexus's replay skips the line rather than read a row from it"
        );
        let mut restarted = ClubLog::for_ruleset(party("arrlfd"), "TEST FD");
        restarted.attach_journal_since(&path, 0).unwrap();
        assert_eq!(restarted.pinned("aaaa0001"), Some(mine.as_str()));
        assert_eq!(restarted.key_refusal("aaaa0001", &other), held);
        assert!(restarted.rows().is_empty(), "a pin is no contact");
        let mut next_event = ClubLog::for_ruleset(party("arrlfd"), "TEST FD");
        next_event.attach_journal_since(&path, 1_001).unwrap();
        assert_eq!(
            next_event.pinned("aaaa0001"),
            None,
            "a previous event's pin"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⭐ **A give is one journal line, replayed in order at attach, skipped by every older
    /// reader and aged out with its event.** The host gives a position held by one laptop to the
    /// laptop it turned away for it: the position is pinned to that laptop, every hold on it goes
    /// stale, its contacts so far leave the club's log, and the entry and its handle are gone.
    /// A restarted host replays the pin, the rows, the give and the rows after it in that order,
    /// and so holds what the live one held. A build before gives reads the line as a note it
    /// does not know and then as a row, which needs fields it lacks; a build before notes, as a
    /// row only: each skips it. A give older than the cutoff gives nothing, and a give newer than
    /// a pin it replaced still holds once that pin has aged out. CONTROL: a handle on an entry
    /// turned away for another reason gives nothing.
    #[test]
    fn a_give_is_one_journal_line_replayed_in_order_and_aged_out_with_its_event() {
        let dir = scratch("give");
        let path = dir.join("fd_event_give.jsonl");
        let (first, theirs) = (
            throwaway_hash("the laptop that joined first"),
            throwaway_hash("the laptop the position is"),
        );
        let mut club = ClubLog::for_ruleset(party("arrlfd"), "TEST FD");
        club.attach_journal_since(&path, 0).unwrap();
        club.join("aaaa0001", "CW tent", "W9XYZ", 1_000);
        club.pin("aaaa0001", &first, 1_000);
        let first_hold = club.hold("aaaa0001");
        club.merge(
            &wq("aaaa0001", 1, "K1ABC", "40m", "CW", "EMA", 1_000),
            1_000,
        );
        club.note_refused(
            "aaaa0001",
            &theirs,
            "CW tent",
            "W9XYZ",
            POSITION_HELD,
            1_001,
        );
        // With a key of its own, as every JOIN this build sends has: only being held out of its
        // position can be what lets an entry give one.
        club.note_refused(
            "bbbb0002",
            &throwaway_hash("a laptop logging another contest"),
            "SSB tent",
            "W9XYZ",
            "another contest",
            1_001,
        );
        let handle_of = |club: &ClubLog, held: bool| {
            club.refused(1_002)
                .iter()
                .find(|r| r.held_out() == held)
                .map(|r| r.handle)
                .unwrap()
        };
        let (handle, other) = (handle_of(&club, true), handle_of(&club, false));
        assert_ne!(handle, other, "each entry has its own handle");
        assert_eq!(
            club.give(other, 1_002),
            Err(GiveRefusal::Stale),
            "CONTROL: an entry turned away for another reason gives nothing"
        );
        let lines_before = std::fs::read_to_string(&path).unwrap().lines().count();
        assert_eq!(club.give(handle, 1_002), Ok(()));
        assert_eq!(club.pinned("aaaa0001"), Some(theirs.as_str()));
        assert_eq!(
            club.held("aaaa0001", first_hold),
            Err(POSITION_HELD.to_string()),
            "the first laptop's hold is stale"
        );
        assert!(club.rows().is_empty(), "the position starts afresh");
        assert_eq!(
            club.give(handle, 1_003),
            Err(GiveRefusal::Stale),
            "a used handle"
        );
        assert_eq!(
            club.refused(1_003).len(),
            1,
            "the given laptop's entry is gone; the other stays"
        );
        let journal = std::fs::read_to_string(&path).unwrap();
        let line = journal.lines().last().unwrap();
        assert_eq!(journal.lines().count(), lines_before + 1, "one line");
        assert_eq!(
            line,
            format!("{{\"give\":{{\"pos\":\"aaaa0001\",\"key_hash\":\"{theirs}\",\"at\":1002}}}}")
        );
        assert!(
            serde_json::from_str::<MergedRow>(line).is_err(),
            "a build before notes reads no row from it"
        );
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        #[allow(dead_code)]
        enum BeforeGives {
            Pin {
                pos: String,
                key_hash: String,
                at: u64,
            },
            KeptOut(KeptOut),
        }
        assert!(
            serde_json::from_str::<BeforeGives>(line).is_err(),
            "a build before gives reads no note from it"
        );
        let accept = club.join("aaaa0001", "CW tent", "W9XYZ", 1_004);
        assert_eq!(accept, 0, "the given laptop sends its whole log");
        club.merge(
            &wq("aaaa0001", 1, "W5DEF", "40m", "CW", "STX", 1_004),
            1_004,
        );

        let replayed = |cutoff: u64| {
            let mut c = ClubLog::for_ruleset(party("arrlfd"), "TEST FD");
            c.attach_journal_since(&path, cutoff).unwrap();
            let calls: Vec<String> = c.rows().iter().map(|r| r.call.clone()).collect();
            (
                c.pinned("aaaa0001").map(str::to_string),
                calls,
                c.hold("aaaa0001"),
            )
        };
        assert_eq!(
            replayed(0),
            (Some(theirs.clone()), vec!["W5DEF".to_string()], 1),
            "a restarted host holds what the live one held"
        );
        assert_eq!(
            replayed(1_002),
            (Some(theirs.clone()), vec!["W5DEF".to_string()], 1),
            "the give outlives the pin it replaced"
        );
        assert_eq!(
            replayed(1_003),
            (None, vec!["W5DEF".to_string()], 0),
            "an aged give"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- what tells laptops and positions apart on the host's screen -------------------

    /// ⭐ **A club code is eight characters no two of which read alike, made from the key's hash
    /// alone**: the first 40 bits of it, in two groups of four, the same every time. SHA-256 of
    /// "abc" (FIPS 180-2's example) gives `Q9W1-DFWF`; the lowest and highest 40 bits give
    /// `0000-0000` and `ZZZZ-ZZZZ`. No I, L, O or U, and a thousand hashes give a thousand codes.
    /// No key, or anything but hex, gives no code. CONTROL: what follows the first ten digits
    /// changes nothing.
    #[test]
    fn a_club_code_is_eight_unambiguous_characters_from_the_key_hash_alone() {
        assert_eq!(
            club_code("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            "Q9W1-DFWF"
        );
        assert_eq!(
            club_code(&format!("0000000000{}", "f".repeat(54))),
            "0000-0000"
        );
        assert_eq!(
            club_code(&format!("ffffffffff{}", "0".repeat(54))),
            "ZZZZ-ZZZZ"
        );
        let hash = throwaway_hash("a laptop");
        assert_eq!(
            club_code(&hash),
            club_code(&format!("{}{}", &hash[..10], "0".repeat(54))),
            "CONTROL: only the first ten digits"
        );
        let codes: HashSet<String> = (0..1000)
            .map(|i| club_code(&throwaway_hash(&format!("laptop {i}"))))
            .collect();
        assert_eq!(codes.len(), 1000, "a thousand laptops, a thousand codes");
        for code in &codes {
            assert!(
                code.len() == 9
                    && code.as_bytes()[4] == b'-'
                    && code
                        .bytes()
                        .enumerate()
                        .all(|(i, b)| i == 4 || CODE_ALPHABET.contains(&b)),
                "{code}"
            );
            assert!(!code.contains(['I', 'L', 'O', 'U']), "{code}");
        }
        for none in [
            "",
            "abc",
            "zzzzzzzzzzzz",
            "+123456789abcdef",
            "12345\u{e9}6789",
        ] {
            assert_eq!(club_code(none), "", "{none:?}");
        }
    }

    /// ⭐ **Two positions with one name read apart, and the one heard of first keeps it**: the
    /// same name compared case-blind and with its spacing read as one space, numbered from (2) in
    /// the order the host heard of each, whatever their position ids; a name a peer chose to read
    /// like a numbered one is numbered past it; a rename moves a position's name and not its place.
    /// The board's lines carry these names. A restarted host hears of its positions in its
    /// journal's order. CONTROLS: a position with a name of its own keeps it, and one with no name
    /// has none.
    #[test]
    fn two_positions_with_one_name_read_apart_and_the_first_keeps_it() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST");
        club.join("bbbb0002", "CW tent", "W9XYZ", 100);
        club.join("cccc0003", "cw  Tent", "W9XYZ", 101);
        club.join("aaaa0009", "CW tent", "W9XYZ", 102);
        club.join("dddd0004", "CW tent (2)", "W9XYZ", 103);
        club.join("eeee0005", "SSB tent", "W9XYZ", 104);
        club.join("ffff0006", "", "W9XYZ", 105);
        let shown = |c: &ClubLog| -> Vec<(String, String)> {
            let mut v: Vec<(String, String)> = c
                .shown_names()
                .into_iter()
                .map(|(id, n)| (id.to_string(), n))
                .collect();
            v.sort();
            v
        };
        let want = |rows: &[(&str, &str)]| -> Vec<(String, String)> {
            rows.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect()
        };
        assert_eq!(
            shown(&club),
            want(&[
                ("aaaa0009", "CW tent (3)"),
                ("bbbb0002", "CW tent"),
                ("cccc0003", "cw  Tent (2)"),
                ("dddd0004", "CW tent (2) (2)"),
                ("eeee0005", "SSB tent"),
                ("ffff0006", ""),
            ])
        );
        let mut board: Vec<(String, String)> = club
            .board_rows(110)
            .into_iter()
            .map(|r| (r.pos, r.name))
            .collect();
        board.sort();
        assert_eq!(board, shown(&club), "the board's lines carry them");
        club.position_status("bbbb0002", &report("Phone tent", "20m", "PH", "K9OP"), 111);
        assert_eq!(
            shown(&club)[..3],
            want(&[
                ("aaaa0009", "CW tent (2)"),
                ("bbbb0002", "Phone tent"),
                ("cccc0003", "cw  Tent"),
            ])[..],
            "renamed, the tent frees its name for the one heard of next"
        );

        let dir = scratch("names");
        let path = dir.join("fd_event_names.jsonl");
        let mut live = ClubLog::new(FdEvent::ArrlFd, "TEST");
        live.attach_journal_since(&path, 0).unwrap();
        for (seq, pos) in [(1, "cccc0003"), (1, "bbbb0002")] {
            live.merge(&wq(pos, seq, "K1ABC", "20m", "CW", "EMA", 1_000), 1_000);
        }
        let mut restarted = ClubLog::new(FdEvent::ArrlFd, "TEST");
        restarted.attach_journal_since(&path, 0).unwrap();
        restarted.join("bbbb0002", "CW tent", "W9XYZ", 2_000);
        restarted.join("cccc0003", "CW tent", "W9XYZ", 2_001);
        assert_eq!(
            shown(&restarted),
            want(&[("bbbb0002", "CW tent (2)"), ("cccc0003", "CW tent")]),
            "heard of in the journal's order"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⭐ **A laptop is connected as a position from its served JOIN until its connection
    /// closes, and only under the position's current hold**: a link the host's give made stale
    /// holds the position no more though its connection has not closed yet, and a link this club
    /// never opened closes nothing. CONTROL: another position is not connected because one is.
    #[test]
    fn a_laptop_is_connected_as_its_position_until_its_link_closes() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST");
        assert!(!club.connected("bbbb0002"), "nobody yet");
        let first = club.open_link("bbbb0002");
        let second = club.open_link("bbbb0002");
        assert!(club.connected("bbbb0002"));
        assert!(!club.connected("cccc0003"), "CONTROL: another position");
        club.close_link(first);
        assert!(club.connected("bbbb0002"), "one link is still open");
        club.close_link(second);
        assert!(!club.connected("bbbb0002"), "both closed");
        club.close_link(u64::MAX);
        let held = club.open_link("bbbb0002");
        club.note_refused(
            "bbbb0002",
            &throwaway_hash("the laptop the position is"),
            "CW tent",
            "W9XYZ",
            POSITION_HELD,
            1_000,
        );
        let handle = club.refused(1_000)[0].handle;
        club.give(handle, 1_000).unwrap();
        assert!(
            !club.connected("bbbb0002"),
            "the link the give made stale, before its connection closes"
        );
        club.close_link(held);
        let given = club.open_link("bbbb0002");
        assert!(club.connected("bbbb0002"), "the laptop it was given to");
        club.close_link(given);
    }

    // ---- the host's lasting list of contacts kept out of the club's log ----------------

    /// ⭐ **A contact kept out of the club's log stays on the host's list until the event
    /// ends**: past the minute a turned-away JOIN is shown, past its position's next JOIN, and
    /// through a host restart (the journal), one entry per contact however often it is sent
    /// again, oldest first. A previous event's are not listed, an older Nexus's replay reads
    /// none as a row, and one a later row merged under the same id is in the log after all.
    /// CONTROLS: a contact the club takes is not listed, and the turned-away list, which is
    /// for JOINs, names no contact.
    #[test]
    fn a_kept_out_contact_stays_listed_until_the_event_ends() {
        let dir = scratch("kept-out");
        let path = dir.join("fd_event_kept.jsonl");
        let mut club = ClubLog::for_ruleset(party("arrlfd"), "TEST FD");
        club.attach_journal_since(&path, 0).unwrap();
        club.join("bbbb0002", "CW tent", "W9XYZ", 1_000);
        let row = |seq: u64, call: &str, band: &str| {
            with_sent(
                wq(
                    "bbbb0002",
                    seq,
                    call,
                    band,
                    "CW",
                    "EMA",
                    FD_START + seq * 60,
                ),
                ("3A", "WI"),
            )
        };
        club.merge(&row(1, "K1ABC", "40m"), 1_000);
        club.merge(&row(2, "W1AW", "40m\u{200B}"), 1_001);
        club.merge(&row(2, "W1AW", "40m\u{200B}"), 1_002);
        club.merge(&row(3, "K1F\nAKE", "20m"), 1_003);
        let listed = |c: &ClubLog| {
            c.kept_out()
                .iter()
                .map(|k| (k.seq, k.label.clone(), k.call.clone()))
                .collect::<Vec<_>>()
        };
        let want = vec![
            (2, "CW tent".to_string(), "W9XYZ".to_string()),
            (3, "CW tent".to_string(), "W9XYZ".to_string()),
        ];
        assert_eq!(
            listed(&club),
            want,
            "once each, oldest first, sent again or not"
        );
        let reason = &club.kept_out()[0].reason;
        assert!(
            reason.contains("its contact with W1AW is not in the club's log: its band"),
            "{reason}"
        );
        club.join("bbbb0002", "CW tent", "W9XYZ", 1_100);
        assert_eq!(
            listed(&club),
            want,
            "a minute on, and after its position rejoined"
        );
        assert!(
            club.refused(1_100).is_empty(),
            "the turned-away list names no contact"
        );
        let mut restarted = ClubLog::for_ruleset(party("arrlfd"), "TEST FD");
        restarted.attach_journal_since(&path, 0).unwrap();
        assert_eq!(listed(&restarted), want, "after a host restart");
        assert_eq!(
            restarted.rows().len(),
            1,
            "CONTROL: the contact the club took"
        );
        let journal = std::fs::read_to_string(&path).unwrap();
        let notes: Vec<&str> = journal
            .lines()
            .filter(|l| l.starts_with("{\"kept_out\""))
            .collect();
        assert_eq!(notes.len(), 2, "{journal}");
        assert!(
            notes
                .iter()
                .all(|l| serde_json::from_str::<MergedRow>(l).is_err()),
            "an older Nexus's replay skips them"
        );
        restarted.merge(&row(2, "W1AW", "40m"), 1_200);
        assert_eq!(
            listed(&restarted),
            want[1..],
            "merged after all: in the log"
        );
        let mut next_event = ClubLog::for_ruleset(party("arrlfd"), "TEST FD");
        next_event.attach_journal_since(&path, 1_004).unwrap();
        assert!(next_event.kept_out().is_empty(), "a previous event's");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
