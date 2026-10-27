//! Station — the JS8 protocol brain (mainwindow.cpp:7740-9400 + Inbox.cpp, read as the spec).
//!
//! A PURE state machine: it never sees audio, never keys, never reads a clock or an RNG of its
//! own. The engine drives it — `on_event` routes one reassembler event through JS8Call's gating
//! chain, `tick(now_ms)` advances the CQ repeat and the idle watchdog (a due heartbeat is
//! released by `next_frame`), and the operator verbs (`send`, `send_command`, `call_cq`,
//! `heartbeat_now`, …) enqueue intent. Everything the station wants to transmit leaves through
//! EXACTLY ONE seam:
//!
//! ```text
//!     next_frame(period_start_ms, busy, rng) -> Option<TxFrame>
//! ```
//!
//! No other method, field or trait hands out a transmittable frame — B7 keys the radio and
//! relies on that being the single gate every TX safety check sits above,
//! and a test pins it. Every automatic frame carries its `Origin` (Operator / Heartbeat / HbAck /
//! AutoReply / Relay) so B7 can gate each class independently. Time and randomness are INJECTED:
//! `tick`/`next_frame`/`on_event` take `now_ms`, and `next_frame` takes an RNG closure, so every
//! schedule is deterministic under test.
//!
//! Behaviours (each verified against upstream and the approved defaults — autoreply ON, relay ON,
//! HB-ack OFF, HB on-demand):
//! - Heartbeat: `MYCALL: HEARTBEAT GRID4` (the 4-character square) as ONE type-000 frame, at an
//!   offset picked once per message by JS8Call's `findFreeFreqOffset(500, 1000, 50)`
//!   (`FreqHint::HbSubband`; the engine keeps an operator at or below 1000 Hz on their own
//!   offset, as `sendHeartbeat` does, mainwindow.cpp:6256). A periodic heartbeat is due one
//!   interval after the next transmit cycle (:6311) and `next_frame` releases it at the boundary
//!   of that period; the end of every message and directed traffic to me re-base it the same
//!   way (:3706). Replies and ACKs never QSY (`FreqHint::Dial`); the HB-ACK is the exception.
//! - HB-ACK (default off): only with heartbeat networking + autoreply + `hb_ack`, no message still
//!   arriving, no QSO pause, the caller on no deny list and not stopped by the 55-minute rule
//!   (`hb_rate_limit`), answer a heard heartbeat `CALL HEARTBEAT SNR +NN [MSG ID n]`, on a free
//!   heartbeat spot picked as the heartbeat's is but with no own-offset rule (`sendHeartbeatAck`,
//!   :6299). An incoming `HEARTBEAT SNR` is never answered (no ack-of-an-ack loop). A heartbeat
//!   is @ALLCALL traffic to JS8Call, so the HB-ACK both checks and enters its @ALLCALL cache: a
//!   station is acknowledged at most once per `allcall_reply_interval_ms`, and one acknowledged
//!   is held off @ALLCALL replies as long (processCommandActivity.cpp:303-311, :719-723).
//! - Heartbeat networking (JS8Call 3.0.3's Mode menu, default off) and the speed: heartbeats and
//!   HB-ACKs go out only with networking on, at Slow, Normal and Fast ([`heartbeat_speed`], an
//!   allow-list); elsewhere the HB repeat stops and what of either waits is dropped. With
//!   `hb_anywhere` both key on the operator's own offset instead of a free 500–1000 Hz spot.
//! - QSO pause (default on): the call in the cockpit's To box is JS8Call's selected call
//!   ([`Station::select_call`]); selecting one pauses the HB repeat until it is cleared, stops the
//!   repeating CQ, and holds every HB-ACK back. With `avoid_allcall` nothing addressed to
//!   @ALLCALL but a heartbeat or a CQ is acted on.
//! - Autoreply: only to my own call and joined groups, never to a query addressed to `@ALLCALL`
//!   (JS8Call's `!isAllCall`, mainwindow.cpp:8834-8869), and only the autoreply subset
//!   (`Command::is_autoreply`, = upstream `autoreply_cmds {0,2,3,4,6,9,10,11,12,13,14,16,30}`);
//!   an empty INFO or grid draws no reply (:8841, :8861). `QUERY MSGS` on `@ALLCALL` is answered
//!   only when a message waits, once per station per `allcall_reply_interval_ms` (:8812, :9275),
//!   the station keyed on its call as heard, as JS8Call's @ALLCALL cache keys it (`d.from`).
//! - Every automatic reply asks the operator first while `autoreply_confirmation` is on
//!   (JS8Call's default) and is No by itself after 89 s; off, it is queued as it is made. A
//!   queued reply waits in JS8Call's message queue until the compose box is free and nothing is
//!   going out (`process_tx_queue`), then keys at the next period with AUTO on, or is put in the
//!   compose box with AUTO off, never keyed by itself. No reply is made while the box holds text
//!   or a message to me is still arriving, and nothing at all is done for a station the
//!   allow/deny lists keep out or while the idle watchdog stands (mainwindow.cpp:8792-8820,
//!   :9364-9389, :9627-9690).
//! - Relay (`>`): retransmit `rest *DE* MYCALL`; at the final hop parse the chain to `A>B>C` and
//!   answer `A>B>C ACK` (unless the embedded text is itself an autoreply command).
//! - Store-and-forward: with relay on, `MSG TO:` stores a `Store` inbox row keyed to the target's
//!   base call (JS8Call's `Radio::base_callsign`: VE3/W1AW and W1AW/P are held for W1AW) and is
//!   answered `<from> ACK`, as JS8Call answers it (mainwindow.cpp:9014-9051); the next
//!   heartbeat/query from that station, as heard or by its base call, is offered `MSG ID n`;
//!   `QUERY MSG n` delivers it and marks it `Delivered`. One with no text is held and answered
//!   but never offered or delivered, as JS8Call skips it (:9516-9529, :9223-9226). The offers and
//!   the delivery read as JS8Call 3.0.3's (processCommandActivity.cpp:661-706, :938-1045, the
//!   lookups in `held`): `YES MSG ID n +k` and the HB-ACK's `MSG ID n +k)` say k more wait, and a
//!   delivery names the next one, `… FROM <from> NEXT MSG ID m +k`. A message held for one
//!   station is kept until the operator deletes it or a cap drops it, as JS8Call keeps it. One
//!   whose hand-over would not fit one transmission at the current speed (the §97.119 cap,
//!   which JS8Call does not have) is not offered, counted, named next or handed over until the
//!   speed allows it, and the Inbox is told why (`held_too_long`).
//! - Group store-and-forward: a `MSG TO:@GROUP` is held 48 h and offered once to each station —
//!   when a QUERY MSGS (or a heartbeat) addressed to a group I joined finds nothing held for the
//!   asker itself (`getNextGroupMessageIdForCallsign`, Inbox.cpp:468) — and handed over to any
//!   station that asks for it by number, as often as it asks, as JS8Call 3.0.3 hands it
//!   (processCommandActivity.cpp:905-935).
//! - QUERY CALL `<call>?`, on my call, a joined group or @ALLCALL, with AUTO on: `<from> YES <snr>
//!   (<age>)` when the call is heard (processCommandActivity.cpp:1052-1101). AGN? to my call: my
//!   last message again, as it went out (:625-629).
//! - "Store message…" (`store_message`): the operator leaves a message here for a call, held and
//!   handed over as any held message is.
//! - A cap that drops a message keeps a note of which and why (`inbox_dropped`) for the Inbox.
//! - A `MSG` to me or a joined group (never `@ALLCALL`) is filed `Unread` in my inbox, as
//!   JS8Call's `addCommandToMyInbox` files it (mainwindow.cpp:9121-9133), and answered as JS8Call
//!   answers it (:9139): `<from> ACK` to the sender as heard, or `<path> ACK` back along the relay
//!   path that brought it. The ACK is an automatic reply, so only with autoreply on, and one waits
//!   at a time however many copies arrive. My mail (`Unread`/`Read`) is bounded apart from mail
//!   held for others and never expires.
//! - Idle watchdog: on trip, stop TX and turn autoreply/relay/HB OFF, clear the outbox, surface a
//!   toast — `tx_enabled` (the engine's latch) is NOT touched.
//! - The APRS-IS relay (JS8Call 3.0.3): the engine hands `enqueue_aprs_relay` an APRS message for a
//!   station heard here, as `@APRSIS MSG TO:<call> <text> DE <sender>`; it waits ahead of every
//!   automatic reply and keys whatever AUTO says (`Origin::AprsRelay`). Heard the other way, an
//!   `@APRSIS MSG TO:` (or `MSG TO:` text) naming my call is filed in my inbox from "APRS".
use crate::phy::{Speed, Word87, I3};
use crate::proto::callsign::{is_base_call, is_compound_call, split_portable, CallRef};
use crate::proto::command::Command;
use crate::proto::held;
// Every composer below hands `frames_with_grid` the station's locator, as JS8Call hands
// `buildMessageFrames` `my_grid().left(4)` for every message (mainwindow.cpp:5434, :7885):
// compose puts the square in a compound callsign's announcement (varicode.cpp:2125) and
// nowhere else.
use crate::proto::compose::{frames_with_grid, ComposeError};
use crate::proto::frame::{encode_frame, format_snr, Frame};
use crate::proto::reassembly::{Message, MessageEvent};
use std::collections::HashMap;

/// JS8Call's typeahead: the compose box's message while it goes out.
mod typeahead;
pub use typeahead::{TypeaheadError, TypeaheadView};

// ---- resource bounds (hostile input: the station processes frames from anyone with a TX) -----
//
// Every collection here grows from air-sourced data, and JS8 stations run unattended for days, so
// each limit is picked from the protocol's own numbers, not a round guess:
//
// A JS8 frame carries at most 12 six-bit characters, and the §97.119 airtime cap tops a message
// out at 99 frames (Turbo); a callsign is at most 11 characters even compound. So no single
// air-sourced string is legitimately large: callsign-shaped fields clamp to 32 bytes, a message
// body / display line to 512 bytes (well above any real multi-frame message), operator
// info/status to 256. A relay chain has NO hop counter upstream (mainwindow.cpp) — a deliberate
// omission we DIVERGE from for safety: the path is capped at 8 hops so a relay loop terminates.
const MAX_CALL_LEN: usize = 32;
const MAX_TEXT_LEN: usize = 512;
const MAX_INFO_LEN: usize = 256;
const MAX_PATH_HOPS: usize = 8;

// Store-and-forward puts a stranger's text in our memory. Cap the entry count AND the total
// stored bytes. A message held for ONE station is kept until it is deleted or a cap drops it, as
// JS8Call keeps it (it never expires one, mainwindow.cpp:9508-9530); a GROUP's is kept 48 h, as
// long as JS8Call offers one (`held::GROUP_HOLD_MS`). Over a cap, drop the one held for others
// that was already handed over first, then the OLDEST, toast, and keep a note of which and why for
// the Inbox (`inbox_dropped`); never silently. A MSG to me is the operator's own mail: it has the
// same caps, counted apart (so it can never push out mail held for someone else), and no expiry.
// `heard` is one row per station: a busy band holds a few
// hundred, so cap at 500 and evict least-recently-heard. `allcall_replied` records a reply time
// per callsign; past the reply interval it carries no information, so it self-prunes.
const MAX_INBOX: usize = 100;
const MAX_INBOX_BYTES: usize = 64 * 1024;
const MAX_HEARD: usize = 500;
// Who has been handed a group's message: one row per station per message, each an over on the
// air, so a busy group fills it slowly; past the cap the oldest row goes (that station could be
// offered that message again). The Inbox keeps the last ten drop notes until the operator clears
// them. My last message, for AGN?, is at most a message's frames: bounded anyway.
const MAX_GROUP_RECIPIENTS: usize = 1000;
const MAX_DROP_NOTES: usize = 10;
const MAX_LAST_TX_LEN: usize = 4 * 1024;

// The outbox drains once per period; pending auto-replies queue faster than that under a flood.
// Cap both — refusing an auto-reply under flood is correct (the alternative transmits stale
// traffic for hours), and the refusal is surfaced as a Toast, never silent.
const MAX_OUTBOX: usize = 32;
const MAX_PENDING: usize = 32;

/// JS8Call 3.0's heartbeat-acknowledgement window (`BLOCK_WINDOW_SECS = 55 * 60`,
/// processHeartbeatRateLimit.cpp:14): with `hb_rate_limit` on, a station heard heartbeating again
/// less than this long after the heartbeat it was last acknowledged for is acknowledged no more.
pub const HB_RATE_WINDOW_MS: u64 = 55 * 60 * 1000;
/// The rule's two lists grow from air-sourced calls, so each is capped as `heard` is, dropping its
/// oldest entry. A timestamp past the window carries nothing and is pruned on its own. The rule
/// also adds no call to a "never acknowledge" list already this long: that list is the
/// operator's Settings.
const MAX_HB_RATE: usize = MAX_HEARD;

/// The speeds JS8Call sends heartbeats and heartbeat acknowledgements at: Slow, Normal and Fast
/// (`canCurrentModeSendHeartbeat`, mainwindow.cpp:4113-4117; 2.3: mainwindow.cpp:5975-5980). Named
/// one by one, so a speed added later sends none until it is listed here. Elsewhere JS8Call hides
/// its HB button and stops the repeat (`prepareHeartbeatMode`, :4124-4130) and answers no
/// heartbeat (processCommandActivity.cpp:636).
pub fn heartbeat_speed(speed: Speed) -> bool {
    matches!(speed, Speed::Slow | Speed::Normal | Speed::Fast)
}

/// Is `origin` a heartbeat or a heartbeat acknowledgement, the two that go out only where
/// [`heartbeat_speed`] and heartbeat networking allow?
pub fn heartbeat_origin(origin: Origin) -> bool {
    matches!(origin, Origin::Heartbeat | Origin::HbAck)
}

/// How long JS8Call's confirmation box waits before it answers No by itself. Every automatic
/// reply opens one with a 90-second timeout (`confirmThenEnqueueMessage(90, …)`,
/// mainwindow.cpp:9386, and the HB-ACK's, :6302); the box takes a second off as it opens
/// (`showEvent` → `tick`, SelfDestructMessageBox.cpp:26-31, :39), so it reads "No (89)" and
/// clicks its default, No (:68), 89 seconds after it opened.
pub const CONFIRM_NO_AFTER_MS: u64 = 89_000;

/// JS8Call's `Radio::base_callsign` (Radio.cpp:117-133): the longer side of a `/` (the right one
/// on a tie), upper-cased: `W1AW/P` → `W1AW`, `VE3/W1AW` → `W1AW`.
pub(crate) fn radio_base_callsign(call: &str) -> String {
    let call = call.trim();
    match call.find('/') {
        Some(slash) if call.len() - slash > slash => call[slash + 1..].to_ascii_uppercase(),
        Some(slash) => call[..slash].to_ascii_uppercase(),
        None => call.to_ascii_uppercase(),
    }
}

/// Whether `from`, as heard or by its base call, is on `list` (JS8Call's `list.contains(d.from) ||
/// list.contains(Radio::base_callsign(d.from))`, mainwindow.cpp:8797, :8804, :9073).
fn on_list(list: &[String], from: &str) -> bool {
    let base = radio_base_callsign(from);
    list.iter()
        .any(|c| c.trim().eq_ignore_ascii_case(from.trim()) || c.trim().eq_ignore_ascii_case(&base))
}

/// Whether `e` is a message held for `who` (the station as heard): held under that call or its
/// base call, the test JS8Call hands a message over on (QUERY MSG n skips one only when `to !=
/// who && to != Radio::base_callsign(who)`, mainwindow.cpp:9219).
fn held_for(e: &InboxEntry, who: &str) -> bool {
    e.state == InboxState::Store
        && (e.to.eq_ignore_ascii_case(who) || e.to.eq_ignore_ascii_case(&radio_base_callsign(who)))
}

/// Whether a held message has something to deliver. A `MSG TO:` with nothing after the call is
/// held and acknowledged, but JS8Call never offers it: its lookup skips a held message whose
/// TEXT is empty, trimmed (`getNextMessageIdForCallsign`, mainwindow.cpp:9516-9520, :9525-9529),
/// and so does QUERY MSG n (:9223-9226).
pub(crate) fn has_text(e: &InboxEntry) -> bool {
    !e.text.trim().is_empty()
}

/// JS8Call files a repeat of the same relayed APRS message only after a minute
/// (`m_aprsRelayDedupCache`, processCommandActivity.cpp:553-566).
const APRS_FILE_DEDUPE_MS: u64 = 60_000;

/// The inbound relay's `<CALL> <message>` when `m` is an APRS message relayed to @APRSIS, in either
/// form JS8Call 3.0.3 files: a `MSG TO:` whose body is `<CALL> <message>`, or a `MSG` whose body
/// is `TO:<CALL> <message>` (`^TO:\s*(\S+)\s+(.*)$`). `None` for anything else.
///
/// ⚠️ JS8Call 3.0.3 KEYS these with no checksum (`skipAprsChecksum`, Varicode.cpp:2275-2281) but its
/// own receiver still demands one for MSG and MSG TO: (processBufferedActivity.cpp:64-78), so it
/// discards every relay it sends. Here a body whose checksum verifies is read without it, and one
/// that does not (`Checksum::Bad`, its raw body) is read whole, so a JS8Call relay is filed.
fn aprs_relay_text(m: &Message) -> Option<String> {
    if !m.to_text.eq_ignore_ascii_case("@APRSIS") {
        return None;
    }
    let body = m.text.trim();
    match m.cmd? {
        Command::MsgTo => Some(body.to_string()),
        Command::Msg => {
            let rest = body.strip_prefix("TO:")?.trim_start();
            let (to, text) = rest.split_once(char::is_whitespace)?;
            let text = text.trim();
            (!to.is_empty() && !text.is_empty()).then(|| format!("{to} {text}"))
        }
        _ => None,
    }
}

/// Truncate `s` to at most `max` bytes on a char boundary (air-sourced strings are never large;
/// see the bounds block above).
fn clamp_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut n = max;
    while n > 0 && !s.is_char_boundary(n) {
        n -= 1;
    }
    s[..n].to_string()
}

/// Which automatic (or operator) class produced a frame — the gate key B7 uses. A CQ the
/// operator CLICKS counts as `Operator`; a CQ the repeat schedule produces is `CqRepeat`,
/// an AUTOMATIC origin. The split is load-bearing, not cosmetic: `note_tx_done` resets the
/// idle-watchdog baseline for `Operator` alone, so folding a scheduled CQ into `Operator`
/// would let the repeat loop reset the very watchdog that is meant to stop it — an
/// unattended station calling CQ forever with no bound. `CqRepeat` is bounded exactly as
/// `Heartbeat` is: the idle watchdog, which no automatic TX can reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    Operator,
    Heartbeat,
    HbAck,
    AutoReply,
    Relay,
    CqRepeat,
    /// An APRS-IS message relayed to a station heard here (`enqueue_aprs_relay`). Automatic: it
    /// keys with nobody at the radio, gated by its own persisted switch at plan time.
    AprsRelay,
}

/// Where a frame wants to transmit. `HbSubband` is a free heartbeat spot in 500..1000 Hz
/// (`find_free_freq_offset`), picked once per message, for a heartbeat or an HB-ACK; everything
/// else stays on the dial (replies and ACKs never QSY).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FreqHint {
    Dial,
    HbSubband(f32),
}

/// The one thing the station transmits — handed out ONLY by `Station::next_frame`.
#[derive(Debug, Clone, PartialEq)]
pub struct TxFrame {
    pub word: Word87,
    pub speed: Speed,
    pub freq_hint: FreqHint,
    pub origin: Origin,
    pub first: bool,
    pub last: bool,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StationConfig {
    pub mycall: String,
    pub grid: String,
    pub speed: Speed,
    pub autoreply: bool,
    pub relay: bool,
    pub hb_ack: bool,
    pub hb_interval_min: u16,
    /// CQ repeat interval in minutes; 0 = on demand (JS8Call `CQInterval`). JS8Call offers
    /// 1/5/10/15 for CQ — a shorter ladder than the heartbeat's, because a CQ is a call, not
    /// a beacon (`buildRepeatMenu`'s `isLowInterval`, mainwindow.cpp:6186).
    pub cq_interval_min: u16,
    pub idle_watchdog_min: u16,
    /// JS8Call's callsign aging (`CallsignAging`, whole minutes; 0, its default, is off:
    /// Configuration.cpp:1853). A call last heard this long ago or more is left out of the
    /// HEARING? reply (mainwindow.cpp:8894) and out of the saved heard list (:2021).
    pub callsign_aging_min: u16,
    pub groups: Vec<String>,
    pub info: String,
    pub status: String,
    /// JS8Call's CQ message (`CQMessage`, Configuration.cpp:2154-2155): what the CQ button and
    /// the CQ repeat send, its macros replaced ([`crate::proto::macros`]). Empty sends JS8Call's
    /// fallback, `CQ CQ CQ` and the square (`sendCQ`, mainwindow.cpp:4534-4537).
    pub cq_text: String,
    /// JS8Call's reply message (`Reply`, Configuration.cpp:2156). The station reads it only for
    /// `<MYREPLY>`; the cockpit's REPLY button sends it.
    pub reply_text: String,
    pub allcall_reply_interval_ms: u64,
    /// JS8Call's `AutoreplyConfirmation`, "Ask for confirmation before sending autoreply
    /// transmissions" (Configuration.ui:855), on as JS8Call ships it (Configuration.cpp:1949).
    /// On, every automatic reply waits for the operator's Yes and is No by itself after
    /// [`CONFIRM_NO_AFTER_MS`] (`confirmThenEnqueueMessage`, mainwindow.cpp:9385-9389; the
    /// HB-ACK's own, :6301-6307). Off, a reply goes into the queue as it is made
    /// (`enqueueMessage`, :9388) and keys in the next period.
    pub autoreply_confirmation: bool,
    /// JS8Call's "Only autoreply to these callsigns" (`AutoWhitelist`, Configuration.ui:797):
    /// empty (its default, Configuration.cpp:1847) means everyone. Otherwise a command from a
    /// station not on it, by its call as heard or its base call, is acted on in no way
    /// (mainwindow.cpp:8792-8800).
    pub autoreply_allow: Vec<String>,
    /// "Never autoreply to these callsigns" (`AutoBlacklist`, Configuration.ui:807, default empty,
    /// Configuration.cpp:1848): a command from one, as heard or by its base call, is acted on in
    /// no way (mainwindow.cpp:8802-8807).
    pub autoreply_deny: Vec<String>,
    /// "Never acknowledge heartbeats from these callsigns" (`HBBlacklist`, Configuration.ui:762,
    /// default empty, Configuration.cpp:1849): a heartbeat from one draws no HB-ACK
    /// (mainwindow.cpp:9073-9077). The 55-minute rule adds the stations it stops to it, as
    /// JS8Call adds them to its blacklist (`addToHbBlacklist`, Configuration.cpp:1226-1231), and
    /// the engine writes each into the operator's Settings.
    pub hb_ack_deny: Vec<String>,
    // ---- heartbeat networking and presence, as JS8Call 3.0.3 has them ----
    /// "Enable Heartbeat Networking" (the Mode menu's `actionModeJS8HB`, `SubModeHB`, default off,
    /// 3.0.3 mainwindow.cpp:524-525): no heartbeat and no HB ACK without it, and its HB button is
    /// hidden (`prepareHeartbeatMode`, :4124-4130; processCommandActivity.cpp:637).
    pub hb_networking: bool,
    /// "Allow heartbeat transmissions outside of heartbeat sub-channel (500Hz - 1000Hz)"
    /// (`BeaconAnywhere`, default off, Configuration.cpp:2291): a heartbeat or HB ACK keys on the
    /// operator's own offset instead of a free spot in 500–1000 Hz (`sendHB`, `sendHeartbeatAck`,
    /// mainwindow.cpp:4442-4447, :4471-4472).
    pub hb_anywhere: bool,
    /// "Pause heartbeat transmissions while in a QSO (i.e., callsign is selected)"
    /// (`HeartbeatQSOPause`, default ON, Configuration.cpp:2292): see [`Station::select_call`].
    pub hb_qso_pause: bool,
    /// "Suppress ACK to stations that send heartbeats at less than 55 minute intervals"
    /// (`HBRateLimit`, default off, Configuration.cpp:2134): see [`HB_RATE_WINDOW_MS`].
    pub hb_rate_limit: bool,
    /// "Do not participate in the @ALLCALL group" (`AvoidAllcall`, default off,
    /// Configuration.cpp:2407): nothing addressed to @ALLCALL is acted on but a heartbeat or a
    /// CQ (processCommandActivity.cpp:181-185).
    pub avoid_allcall: bool,
}

impl Default for StationConfig {
    fn default() -> Self {
        StationConfig {
            mycall: String::new(),
            grid: String::new(),
            speed: Speed::Normal,
            autoreply: true,
            relay: true,
            hb_ack: false,
            hb_interval_min: 0,
            cq_interval_min: 0,
            idle_watchdog_min: 60,
            callsign_aging_min: 0,
            groups: Vec::new(),
            info: String::new(),
            status: String::new(),
            cq_text: crate::proto::macros::DEFAULT_CQ.to_string(),
            reply_text: crate::proto::macros::DEFAULT_REPLY.to_string(),
            allcall_reply_interval_ms: 15 * 60 * 1000,
            autoreply_confirmation: true,
            autoreply_allow: Vec::new(),
            autoreply_deny: Vec::new(),
            hb_ack_deny: Vec::new(),
            hb_networking: false,
            hb_anywhere: false,
            hb_qso_pause: true,
            hb_rate_limit: false,
            avoid_allcall: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InboxState {
    Unread,
    Read,
    Store,
    Delivered,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxEntry {
    pub id: u32,
    pub from: String,
    pub to: String,
    pub text: String,
    pub path: Vec<String>,
    pub state: InboxState,
    pub at_ms: u64,
    pub freq_hz: f32,
    pub snr_db: i32,
}

/// Which cap took a message out of the inbox: the count (100 a half) or the bytes (64 KB a half).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DropReason {
    Count,
    Bytes,
}

/// A message a cap took out of the inbox, kept for the Inbox to say which and why until the
/// operator clears the notes. `entry.state` says which half: the operator's mail, or mail held
/// for another station.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxDrop {
    pub at_ms: u64,
    pub why: DropReason,
    pub entry: InboxEntry,
}

/// A held message whose hand-over would not fit one transmission at the current speed (the
/// §97.119 cap, `compose::max_frames`): not offered, counted or handed over until the speed allows
/// it. The Inbox says which, the frames its hand-over needs, the cap, and the slowest speed it
/// fits at (`None`: none).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldTooLong {
    pub id: u32,
    pub frames: usize,
    pub max: usize,
    pub fits_at: Option<Speed>,
}

/// Why "Store message…" refused (`Station::store_message`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreRefusal {
    /// No callsign in Settings: the message would be handed over from nobody.
    NoCallsign,
    /// Not a station's call (empty, @ALLCALL or @HB, which JS8Call's menu greys out).
    NotAStation,
    /// Nothing to store.
    Empty,
    /// Its delivery would not fit one transmission at this speed (the §97.119 cap).
    TooLong { frames: usize, max: usize },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heard {
    pub call: String,
    pub grid: Option<String>,
    pub snr_db: i32,
    pub freq_hz: f32,
    pub speed: Speed,
    pub last_ms: u64,
    pub last_hb: bool,
    pub last_cq: bool,
    pub stored_msgs: u8,
    /// JS8Call's `ackTimestamp`, its call list's ★ "Hearing Your Station": when this station last
    /// sent something addressed to my call or its base call (`toMe`, processCommandActivity.cpp:59-65),
    /// kept until a later one, as `logCallActivity` keeps it (mainwindow.cpp:2302-2304). Display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ack_ms: Option<u64>,
    /// JS8Call's `cqTimestamp`, its call list's ☎ "Calling CQ" for five minutes
    /// (displayCallActivity.cpp:203-205): when this station last called CQ (processCommandActivity.cpp:746),
    /// kept until a later one. Display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cq_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StationAction {
    Queued {
        origin: Origin,
        display: String,
        frames: usize,
    },
    ReplyPending {
        origin: Origin,
        to: String,
        display: String,
        fires_at_ms: u64,
    },
    Toast {
        text: String,
        directed_to_me: bool,
    },
    InboxChanged,
    HeardChanged,
    Relayed {
        path: Vec<String>,
        text: String,
    },
    RateLimited {
        from: String,
    },
    ChecksumFailed {
        from: String,
        freq_hz: f32,
    },
    IdleTripped,
    /// The 55-minute rule stopped acknowledging `from` (`hb_rate_limit`): `from` is on the
    /// station's "never acknowledge" list ([`StationConfig::hb_ack_deny`]) now, unless that list
    /// was full, and the rule's own record changed, to be journaled.
    HbAckRateLimited {
        from: String,
    },
    /// A heartbeat from `from` is not acknowledged: that station was acknowledged, or answered on
    /// @ALLCALL, within the reply interval (JS8Call's @ALLCALL cache).
    HbAckHeldOff {
        from: String,
    },
}

/// A UI view of a queued message (structural mirror of an outbox item).
#[derive(Debug, Clone, PartialEq)]
pub struct QueuedFrame {
    pub origin: Origin,
    pub display: String,
    pub first: bool,
    pub last: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingReply {
    pub origin: Origin,
    pub to: String,
    pub display: String,
    pub fires_at_ms: u64,
}

/// Why an automatic origin whose switch is on would not act now: the first rule in
/// [`Station::readiness`]'s order that stops it. The cockpit names it on that origin's chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Unarmed {
    /// The idle watchdog stands: nothing is acted on until the operator's next act
    /// (`if(m_tx_watchdog) continue;`, mainwindow.cpp:8817-8820).
    Idle,
    /// The engine's TX latch is down: nothing keys.
    TxOff,
    /// An HB ACK answers a heartbeat only while heartbeat networking is on (`actionModeJS8HB`,
    /// processCommandActivity.cpp:637; 2.3: mainwindow.cpp:9061), whatever the HB repeat does.
    NeedsHb,
    /// An HB ACK answers only with AUTO on (:9061), and a relay keys only with AUTO on: with AUTO
    /// off it is put in the compose box (`processTxQueue`, :9674-9685).
    NeedsAuto,
    /// An HB ACK with neither HB nor AUTO on.
    NeedsHbAndAuto,
    /// The heartbeat or the repeating CQ has nothing scheduled: an on-demand one (interval 0) has
    /// gone, and the next waits for its switch to be turned on again.
    Unscheduled,
}

/// Why a heartbeat heard now would draw no acknowledgement although HB ACK is armed: the first of
/// JS8Call's own checks that holds it back (processCommandActivity.cpp:634-651), what its HB
/// button and mode text show by dropping "ACK" (`presentlyWantHBReplies`, mainwindow.cpp:5612-5619).
/// The 55-minute rule holds back single stations, not every acknowledgement: those are
/// [`Station::hb_rate_blocked`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HbAckSuppressed {
    /// The transmit speed sends no heartbeats ([`heartbeat_speed`]).
    Speed,
    /// A message is still arriving (`!m_messageBuffer.isEmpty()`, :640-644).
    Arriving,
    /// A call is selected and the QSO pause is on (:647-651).
    QsoPause,
}

/// Each automatic origin as [`Station::readiness`] answers for it: `None` when the station would
/// act on it now, given its switch is on, else why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub autoreply: Option<Unarmed>,
    pub relay: Option<Unarmed>,
    pub hb_ack: Option<Unarmed>,
    pub hb: Option<Unarmed>,
    pub cq: Option<Unarmed>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationSnapshot {
    pub inbox: Vec<InboxEntry>,
    pub heard: Vec<Heard>,
    pub allcall_replied: Vec<(String, u64)>,
    pub next_inbox_id: u32,
    /// Which calls on the "never acknowledge" list the 55-minute rule put there, for the HB ACK
    /// chip; the list itself is the operator's Settings, as JS8Call's HB blacklist is
    /// (Configuration.cpp:1226-1231). Left out while empty, so a journal that never used the rule
    /// is written as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hb_rate_blocked: Vec<String>,
    /// The stations the rule stopped while that list was full, still turned away
    /// ([`Station::hb_rate_overflow`]). Left out while empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hb_rate_overflow: Vec<String>,
    /// Who has been handed which group message, `(id, call as heard)`. Absent from a journal
    /// written before groups were handed over, and left out while empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group_recipients: Vec<(u32, String)>,
    /// What the caps took out of the inbox, until the operator clears the notes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inbox_dropped: Vec<InboxDrop>,
}

/// One composed message waiting in the outbox; `next_frame` dispenses its frames one per period.
#[derive(Debug, Clone)]
struct OutMsg {
    origin: Origin,
    frames: Vec<(Frame, I3)>,
    cursor: usize,
    freq_hint: FreqHint,
    display: String,
    /// The compose box's message, which its typeahead changes while it goes out (at most one).
    typeahead: bool,
}

/// An automatic reply waiting for the operator's Yes (JS8Call's AutoreplyConfirmation box):
/// `fires_at_ms` is when the box answers No by itself.
#[derive(Debug, Clone)]
struct Pending {
    origin: Origin,
    to: String,
    text: String,
    display: String,
    fires_at_ms: u64,
    freq_hint: FreqHint,
    /// The held message a `QUERY MSG n` hands over, and to whom (the querier as heard): marked
    /// Delivered when the reply is released, as JS8Call's callback marks it (`markMsgDelivered`,
    /// mainwindow.cpp:9227-9235, run by `processTxQueue` once the reply is dequeued,
    /// :9687-9689), so a No leaves it held. A group's message is marked handed to that station
    /// alone (`markGroupMsgDeliveredForCallsign`, processCommandActivity.cpp:931-934).
    delivers: Option<(u32, String)>,
}

/// A reply made (or confirmed) and waiting in JS8Call's `m_txMessageQueue` for the compose box to
/// be free (`processTxQueue`, mainwindow.cpp:9627-9690).
#[derive(Debug, Clone)]
struct Queued {
    origin: Origin,
    text: String,
    freq_hint: FreqHint,
    delivers: Option<(u32, String)>,
}

pub struct Station {
    cfg: StationConfig,
    outbox: std::collections::VecDeque<OutMsg>,
    pending: Vec<Pending>,
    /// JS8Call's `m_txMessageQueue`: the replies made or confirmed, each waiting until the compose
    /// box is free and nothing is going out (`processTxQueue`, mainwindow.cpp:9652-9659), then
    /// keyed (AUTO on) or put in the box (AUTO off).
    queued: std::collections::VecDeque<Queued>,
    /// The reply the station has put in the composer (`addMessageText`, :9671), (id, text),
    /// until the cockpit takes it into its compose box.
    composer: Option<(u32, String)>,
    composer_seq: u32,
    /// The cockpit's compose box holds text, as it last said: JS8Call's `extFreeTextMsgEdit`
    /// not empty (:9657).
    composing: bool,
    /// The receive buffers as the engine last said: one is open (a message still arriving), and
    /// one addressed to me is (`hasExistingMessageBufferToMe`, mainwindow.cpp:4176-4190).
    rx_open: bool,
    rx_to_me: bool,
    heard: Vec<Heard>,
    inbox: Vec<InboxEntry>,
    allcall_replied: HashMap<String, u64>,
    next_inbox_id: u32,
    hb_on: bool,
    hb_next_ms: Option<u64>,
    /// The CQ repeat: session-only, exactly like `hb_on` (never journaled, so the app can
    /// never launch calling CQ). It sends the CQ message, as the CQ button does.
    cq_on: bool,
    cq_next_ms: Option<u64>,
    last_activity_ms: u64,
    idle_tripped: bool,
    last_tx_display: Option<String>,
    /// JS8Call's selected call (`callsignSelected`, mainwindow.cpp:5895), which in Nexus is the
    /// call in the cockpit's To box: the QSO pause reads it (`select_call`).
    selected: Option<String>,
    /// The QSO pause stopped the HB repeat when a call was selected, and turns it back on when the
    /// selection clears (`m_hbPaused`, mainwindow.cpp:5962-5992).
    hb_paused: bool,
    /// JS8Call 3.0's HB ACK rate limit (`HBBlockingDB`, processHeartbeatRateLimit.cpp): when each
    /// station, by its call as heard, was last acknowledged…
    hb_acked_at: HashMap<String, u64>,
    /// …and the stations the rule put on the "never acknowledge" list and that are still on it,
    /// oldest first: which of that list's calls are the rule's, for the HB ACK chip. The list
    /// itself (`hb_ack_deny`) is what turns them away, as JS8Call's blacklist does…
    hb_rate_blocked: Vec<String>,
    /// …and the stations it stopped while that list was full (`MAX_HB_RATE`), which the list could
    /// not take: turned away all the same, as JS8Call's uncapped blacklist would turn them away,
    /// oldest dropped past the cap. Journaled; the rule turned off forgets them.
    hb_rate_overflow: Vec<String>,
    /// The compose box's message while it goes out, for its typeahead.
    typeahead: Option<typeahead::Typeahead>,
    /// Who has been handed which group message, `(id, call as heard)` (JS8Call's
    /// `inbox_group_recip_v1`), oldest first.
    group_recipients: Vec<(u32, String)>,
    /// What the caps took out of the inbox, oldest first, until the operator clears them.
    inbox_dropped: Vec<InboxDrop>,
    /// My last message as a receiver decodes it, frame by frame as it was keyed: JS8Call's
    /// `m_lastTxMessage` (mainwindow.cpp:3652-3656), which AGN? sends again.
    last_tx_message: String,
    /// The @APRSIS messages filed from "APRS" in the last minute, by `TO|TEXT|SENDER`, and when:
    /// JS8Call files a repeat of one only after 60 s (`m_aprsRelayDedupCache`). Session-only.
    aprs_filed: HashMap<String, u64>,
}

impl Station {
    pub fn new(cfg: StationConfig) -> Station {
        Station {
            cfg,
            outbox: std::collections::VecDeque::new(),
            pending: Vec::new(),
            queued: std::collections::VecDeque::new(),
            composer: None,
            composer_seq: 0,
            composing: false,
            rx_open: false,
            rx_to_me: false,
            heard: Vec::new(),
            inbox: Vec::new(),
            allcall_replied: HashMap::new(),
            next_inbox_id: 1,
            hb_on: false,
            hb_next_ms: None,
            cq_on: false,
            cq_next_ms: None,
            last_activity_ms: 0,
            idle_tripped: false,
            last_tx_display: None,
            selected: None,
            hb_paused: false,
            hb_acked_at: HashMap::new(),
            hb_rate_blocked: Vec::new(),
            hb_rate_overflow: Vec::new(),
            typeahead: None,
            group_recipients: Vec::new(),
            inbox_dropped: Vec::new(),
            last_tx_message: String::new(),
            aprs_filed: HashMap::new(),
        }
    }

    /// The operator's Settings, as the engine hands them over. Heartbeat networking turned off,
    /// or a speed JS8Call sends no heartbeats at, stops heartbeats as JS8Call's
    /// `prepareHeartbeatMode(false)` stops them (mainwindow.cpp:4124-4130): see
    /// `stop_heartbeats`. The 55-minute rule turned off forgets the times it kept; the stations
    /// it stopped stay on the "never acknowledge" list, as JS8Call's stay on its blacklist, and
    /// taking one off that list is how it is acknowledged again.
    pub fn set_config(&mut self, cfg: StationConfig) {
        if self.cfg.hb_rate_limit && !cfg.hb_rate_limit {
            self.hb_acked_at.clear();
            self.hb_rate_overflow.clear();
        }
        self.cfg = cfg;
        self.keep_listed_rate_blocks();
        if !self.hb_allowed() {
            self.stop_heartbeats();
        }
    }

    /// Heartbeats and their acknowledgements may go out at all: heartbeat networking on, at a
    /// speed JS8Call heartbeats at (`prepareHeartbeatMode(canCurrentModeSendHeartbeat() &&
    /// actionModeJS8HB->isChecked())`, mainwindow.cpp:4185-4186).
    pub fn hb_allowed(&self) -> bool {
        self.cfg.hb_networking && heartbeat_speed(self.cfg.speed)
    }

    /// Where heartbeats may not go out: the HB repeat stops and is not resumed by the QSO pause,
    /// and a heartbeat or acknowledgement still waiting, asking or queued is dropped. JS8Call
    /// cancels its loop and unchecks the button there (mainwindow.cpp:4127-4130); what it had
    /// already queued would still key, and here it never does.
    fn stop_heartbeats(&mut self) {
        self.hb_on = false;
        self.hb_next_ms = None;
        self.hb_paused = false;
        self.outbox.retain(|o| !heartbeat_origin(o.origin));
        self.pending.retain(|p| !heartbeat_origin(p.origin));
        self.queued.retain(|q| !heartbeat_origin(q.origin));
    }

    pub fn config(&self) -> &StationConfig {
        &self.cfg
    }

    /// My base callsign, uppercased (the identity autoreply and store-and-forward key on).
    fn base(&self) -> String {
        split_portable(self.cfg.mycall.trim())
            .0
            .to_ascii_uppercase()
    }

    /// Is `to` addressed to me — my base call, `@ALLCALL`, or a joined group?
    fn addressed_to_me(&self, to: &str) -> bool {
        self.is_to_me(to)
            || to.eq_ignore_ascii_case("@ALLCALL")
            || self.cfg.groups.iter().any(|g| g.eq_ignore_ascii_case(to))
    }

    /// JS8Call's `toMe` (mainwindow.cpp:8599): my call as set, a `/P` or compound one included,
    /// or its base call (`Radio::base_callsign`).
    fn is_to_me(&self, to: &str) -> bool {
        let mine = self.cfg.mycall.trim();
        !mine.is_empty()
            && (to.eq_ignore_ascii_case(mine)
                || to.eq_ignore_ascii_case(&radio_base_callsign(mine)))
    }

    /// The allow/deny lists (JS8Call's `auto_whitelist` / `auto_blacklist`, mainwindow.cpp:
    /// 8792-8807): an empty allow list lets everyone in, a listed station is let in by its call as
    /// heard or its base call; a denied one, either way, is kept out.
    fn lists_allow(&self, from: &str) -> bool {
        let on = |list: &[String]| on_list(list, from);
        (self.cfg.autoreply_allow.is_empty() || on(&self.cfg.autoreply_allow))
            && !on(&self.cfg.autoreply_deny)
    }

    // ---- the gating chain -------------------------------------------------------------------

    /// Route one reassembler event through JS8Call's gating chain (mainwindow.cpp:8560-9389).
    pub fn on_event(&mut self, ev: &MessageEvent, now_ms: u64) -> Vec<StationAction> {
        let MessageEvent::Message(m) = ev else {
            return Vec::new(); // per-frame events feed the activity pane, not the station brain
        };
        let mut actions = Vec::new();
        self.record_heard(m, &mut actions);

        let to_me = self.is_to_me(&m.to_text);
        let to_group = self
            .cfg
            .groups
            .iter()
            .any(|g| g.eq_ignore_ascii_case(&m.to_text));
        if to_me || to_group {
            actions.push(StationAction::Toast {
                text: crate::proto::reassembly::render_directed(m),
                directed_to_me: true,
            });
            // Defer a periodic heartbeat while a QSO is in progress — JS8Call's
            // HeartbeatQSOPause. This is POLITENESS on frequency, not evidence a human is
            // present, so it does NOT reset the idle-watchdog baseline: only operator TX
            // (`note_tx_done` for `Origin::Operator`) and entering the tier (`mark_active`)
            // do. A stranger calling us must not hold our unattended station on the air —
            // JS8Call resets its idle timer on UI key/mouse activity alone, never on RX
            // (mainwindow.cpp:2987). (No-op at interval 0, the on-demand case.)
            self.bump_hb_schedule(now_ms);
            // The repeating CQ, by contrast, is STOPPED outright — JS8Call's
            // `resetAutomaticIntervalTransmissions(stopCQ = true, …)` (mainwindow.cpp:8333,
            // 8788). Somebody answered the call, so stop calling; a beacon keeps beaconing.
            self.stop_cq_repeat();
        }

        // An APRS message relayed to @APRSIS is the inbound relay's, whole: filed from "APRS" when
        // it names my call, and acted on in no other way (JS8Call `continue`s past both branches).
        // Before the checksum gate, because JS8Call 3.0.3 keys these with NO checksum
        // (`skipAprsChecksum`) — see `aprs_relay_text`.
        if let Some(text) = aprs_relay_text(m) {
            if !self.idle_tripped && self.lists_allow(&m.from) {
                self.file_aprs_relay(m, &text, now_ms, &mut actions);
            }
            return actions;
        }

        // A directed message with a failed checksum is surfaced, never acted on.
        if m.checksum == crate::proto::reassembly::Checksum::Bad {
            actions.push(StationAction::ChecksumFailed {
                from: m.from.clone(),
                freq_hz: m.freq_hz,
            });
            return actions;
        }

        // While the idle watchdog stands nothing is acted on: nothing answered, relayed or
        // delivered, and nothing filed or stored. JS8Call's `if(m_tx_watchdog) continue;`
        // (mainwindow.cpp:8817-8820) comes before every command branch, the MSG and MSG TO: ones
        // included. Gated here on the trip itself, not on the switches it turned off: a settings
        // change re-applies those to the station while the trip still stands. Nor is anything
        // acted on from a station the allow/deny lists keep out (:8792-8807), checked first.
        let replies = !self.idle_tripped && self.lists_allow(&m.from);

        // Heartbeat heard → maybe HB-ACK.
        if m.is_heartbeat() && m.cq.is_none() {
            if replies {
                self.maybe_hb_ack(m, now_ms, &mut actions);
            }
            return actions;
        }
        if m.cq.is_some() {
            return actions; // a CQ is displayed; the operator answers it, not the autoreply engine
        }
        // "Do not participate in the @ALLCALL group": nothing else addressed to @ALLCALL is acted
        // on, a QUERY MSGS on it included (processCommandActivity.cpp:181-185, which spares only
        // a heartbeat and a CQ, both handled above).
        if self.cfg.avoid_allcall && m.to_text.eq_ignore_ascii_case("@ALLCALL") {
            return actions;
        }

        // Directed command autoreply / relay / store-and-forward. JS8Call acts on no command that
        // is not addressed to me, @ALLCALL or a group I joined (mainwindow.cpp:8601), and a relay
        // (`>`, :8908), a MSG TO: (:9012) or a QUERY MSG n (:9171) never on @ALLCALL either.
        let mine = to_me || to_group;
        if let Some(cmd) = m.cmd {
            if self.cfg.relay && cmd == Command::Relay {
                if mine && replies {
                    self.handle_relay(m, now_ms, &mut actions);
                }
                return actions;
            }
            // A `MSG TO:` is stored, and acknowledged, only with relaying on, as JS8Call does it
            // (`d.cmd == " MSG TO:" && !isAllCall && !m_config.relay_off()`, :9014).
            if cmd == Command::MsgTo {
                if mine && self.cfg.relay && replies {
                    self.handle_msg_to(m, now_ms, &mut actions);
                }
                return actions;
            }
            // QUERY MSGS, and QUERY with "MSG n", dispatched on the command as JS8Call dispatches
            // them (` QUERY`, processCommandActivity.cpp:864; ` QUERY MSGS`, :983): a MSG whose
            // text reads "MSG n" is a MSG, filed and acknowledged below.
            if matches!(cmd, Command::QueryMsgs | Command::Query) {
                if replies {
                    self.handle_query_msg(m, mine, now_ms, &mut actions);
                }
                return actions;
            }
            // QUERY CALL is answered on @ALLCALL too (no `!isAllCall`, processCommandActivity.cpp:
            // 1052); AGN? only to my own call (`!isAllCall && !isGroupCall`, :625-626).
            if cmd == Command::QueryCall {
                let allcall = m.to_text.eq_ignore_ascii_case("@ALLCALL");
                if replies && (mine || allcall) && !self.is_me(&m.from) {
                    self.handle_query_call(m, allcall, now_ms, &mut actions);
                }
                return actions;
            }
            if cmd == Command::AgnQuery {
                if replies && to_me && !self.is_me(&m.from) {
                    self.handle_agn(m, now_ms, &mut actions);
                }
                return actions;
            }
            // A MSG to me or a group I joined is filed UNREAD (JS8Call's `d.cmd == " MSG" &&
            // !isAllCall`, mainwindow.cpp:9121, past the "to me, a group or @ALLCALL" gate at
            // :8715), whatever the switches say, but not while the idle watchdog stands or from a
            // station the lists keep out (both `continue` before it). Its ACK (:9139) is an
            // automatic reply, below.
            if cmd == Command::Msg && mine && replies {
                self.file_msg_to_me(m, now_ms, &mut actions);
            }
            // Never an @ALLCALL query: JS8Call's SNR?/INFO?/STATUS?/GRID?/HEARING? replies each
            // carry `!isAllCall` (mainwindow.cpp:8834, :8839, :8849, :8859, :8869). Made whatever
            // AUTO says, as JS8Call makes them (none of those branches asks it); with AUTO off
            // they go to the compose box (`release`).
            if replies && cmd.is_autoreply() && (to_me || to_group) && !self.is_me(&m.from) {
                self.autoreply(m, cmd, now_ms, &mut actions);
            }
        }
        actions
    }

    fn is_me(&self, call: &str) -> bool {
        split_portable(call).0.eq_ignore_ascii_case(&self.base())
    }

    /// The heard list keeps a station under its call as heard, `/P` included, as JS8Call's call
    /// activity keys it (`m_callActivity[d.call]`, mainwindow.cpp:2297-2311): W1AW and W1AW/P are
    /// two stations to it, each with its own SNR and time. A relay route (`W1AW>K1ABC`) is not a
    /// station: JS8Call does not log relay calls (`logCallActivity`, mainwindow.cpp:2290-2293).
    fn record_heard(&mut self, m: &Message, actions: &mut Vec<StationAction>) {
        let call = clamp_str(&m.from, MAX_CALL_LEN).to_ascii_uppercase();
        if call.is_empty() || call == "<....>" || call.contains('>') {
            return;
        }
        let grid = m.grid.as_deref().map(|g| clamp_str(g, MAX_CALL_LEN));
        // The messages held for the station as it was heard, found as its queries find them.
        let stored = self.inbox.iter().filter(|e| held_for(e, &call)).count() as u8;
        let ack_ms = self.is_to_me(&m.to_text).then_some(m.last_ms);
        let cq_ms = m.cq.is_some().then_some(m.last_ms);
        if let Some(h) = self.heard.iter_mut().find(|h| h.call == call) {
            h.snr_db = m.snr_db;
            h.freq_hz = m.freq_hz;
            h.speed = m.speed;
            h.last_ms = m.last_ms;
            h.last_hb = m.is_heartbeat() && m.cq.is_none();
            h.last_cq = m.cq.is_some();
            if grid.is_some() {
                h.grid = grid;
            }
            h.stored_msgs = stored;
            h.ack_ms = ack_ms.or(h.ack_ms);
            h.cq_ms = cq_ms.or(h.cq_ms);
        } else {
            self.heard.push(Heard {
                call,
                grid,
                snr_db: m.snr_db,
                freq_hz: m.freq_hz,
                speed: m.speed,
                last_ms: m.last_ms,
                last_hb: m.is_heartbeat() && m.cq.is_none(),
                last_cq: m.cq.is_some(),
                stored_msgs: stored,
                ack_ms,
                cq_ms,
            });
            self.prune_heard();
        }
        actions.push(StationAction::HeardChanged);
    }

    /// Evict the least-recently-heard station when `heard` exceeds `MAX_HEARD`.
    fn prune_heard(&mut self) {
        while self.heard.len() > MAX_HEARD {
            if let Some((idx, _)) = self.heard.iter().enumerate().min_by_key(|(_, h)| h.last_ms) {
                self.heard.remove(idx);
            } else {
                break;
            }
        }
    }

    /// Expire a GROUP's held message 48 h after it was stored, as long as JS8Call offers one
    /// (`held::GROUP_HOLD_MS`); a message held for one station does not expire, as JS8Call keeps
    /// it. Then, while either half of the inbox is over the count or byte cap, drop that half's
    /// entry that matters least — in mail held for others one already handed over, then the
    /// OLDEST (by `at_ms`) — toast, and note which and why for the Inbox: never silently. The
    /// halves are counted apart so that a flood of MSGs to me can never push out a message held
    /// for another station, which would change what a later `QUERY MSGS` is answered. My own
    /// mail (`Unread`, `Read`) does not expire: JS8Call keeps a MSG to me until the operator
    /// deletes it.
    fn prune_inbox(&mut self, now_ms: u64, actions: &mut Vec<StationAction>) {
        let before = self.inbox.len();
        self.inbox.retain(|e| {
            is_my_mail(e.state)
                || !held::is_group(&e.to)
                || now_ms.saturating_sub(e.at_ms) < held::GROUP_HOLD_MS
        });
        let expired = before != self.inbox.len();
        let mut evicted = false;
        let mut mine_evicted = false;
        for mine in [false, true] {
            loop {
                let why = if self.inbox_count(mine) > MAX_INBOX {
                    DropReason::Count
                } else if self.inbox_bytes(mine) > MAX_INBOX_BYTES {
                    DropReason::Bytes
                } else {
                    break;
                };
                let Some((idx, _)) = self
                    .inbox
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| is_my_mail(e.state) == mine)
                    .min_by_key(|(_, e)| (e.state == InboxState::Store, e.at_ms))
                else {
                    break;
                };
                let entry = self.inbox.remove(idx);
                self.inbox_dropped.push(InboxDrop {
                    at_ms: now_ms,
                    why,
                    entry,
                });
                if self.inbox_dropped.len() > MAX_DROP_NOTES {
                    self.inbox_dropped.remove(0);
                }
                if mine {
                    mine_evicted = true;
                } else {
                    evicted = true;
                }
            }
        }
        // A group message gone takes its record of who had it.
        let inbox = &self.inbox;
        self.group_recipients
            .retain(|(id, _)| inbox.iter().any(|e| e.id == *id));
        if expired || evicted || mine_evicted {
            actions.push(StationAction::InboxChanged);
        }
        if expired {
            actions.push(StationAction::Toast {
                text: "Group message held 48 hours removed".into(),
                directed_to_me: false,
            });
        }
        if evicted {
            actions.push(StationAction::Toast {
                text: "Inbox full: oldest stored messages evicted".into(),
                directed_to_me: false,
            });
        }
        if mine_evicted {
            actions.push(StationAction::Toast {
                text: "Inbox full: oldest messages to you removed".into(),
                directed_to_me: false,
            });
        }
    }

    /// How many entries one half of the inbox holds (`mine`: my mail, else mail held for others).
    fn inbox_count(&self, mine: bool) -> usize {
        self.inbox
            .iter()
            .filter(|e| is_my_mail(e.state) == mine)
            .count()
    }

    fn inbox_bytes(&self, mine: bool) -> usize {
        self.inbox
            .iter()
            .filter(|e| is_my_mail(e.state) == mine)
            .map(|e| {
                e.from.len()
                    + e.to.len()
                    + e.text.len()
                    + e.path.iter().map(String::len).sum::<usize>()
            })
            .sum()
    }

    /// Drop `allcall_replied` entries older than the reply interval: past it they carry no
    /// information (the rate limiter would allow a reply again), so the map self-limits.
    fn prune_allcall(&mut self, now_ms: u64) {
        let ttl = self.cfg.allcall_reply_interval_ms;
        self.allcall_replied
            .retain(|_, &mut t| now_ms.saturating_sub(t) < ttl);
    }

    /// Enqueue an outbox message unless the outbox is at `MAX_OUTBOX`; returns false if refused.
    fn push_outbox(&mut self, out: OutMsg) -> bool {
        if self.outbox.len() >= MAX_OUTBOX {
            return false;
        }
        self.outbox.push_back(out);
        true
    }

    fn maybe_hb_ack(&mut self, m: &Message, now_ms: u64, actions: &mut Vec<StationAction>) {
        // Heartbeat networking at a speed that heartbeats + autoreply + hb_ack
        // (processCommandActivity.cpp:634-638; 2.3: mainwindow.cpp:9059), and no message still
        // arriving: "do not process HB activity if buffer is not empty, this prevents broken
        // incoming MSG's" (`!m_messageBuffer.isEmpty()`, the RECEIVE buffers, :9062-9066), nor
        // while a call is selected with the QSO pause on (processCommandActivity.cpp:647-651), nor
        // a station the heartbeat deny list names (:9073-9077).
        if !(self.hb_allowed() && self.cfg.autoreply && self.cfg.hb_ack) || self.rx_open {
            return;
        }
        if self.cfg.hb_qso_pause && self.selected.is_some() {
            return;
        }
        if on_list(&self.cfg.hb_ack_deny, &m.from) || on_list(&self.hb_rate_overflow, &m.from) {
            return;
        }
        // The station as heard, `/P` included (`sendHeartbeatAck(d.from, …)`, :9098); a message
        // held for it is found as JS8Call finds it (`getNextMessageIdForCallsign(d.from)`, :9082).
        // A station the 55-minute rule stopped is on that deny list, here as in JS8Call
        // (`hb_blacklist`, processCommandActivity.cpp:655-661), and was turned away above.
        let to = clamp_str(&m.from, MAX_CALL_LEN).to_ascii_uppercase();
        if self.is_me(&to) {
            return;
        }
        // A heartbeat is @ALLCALL traffic to JS8Call (`isAllCallIncluded` reads @HB as @ALLCALL,
        // mainwindow.cpp:6088-6090), so its @ALLCALL cache holds off a station acknowledged or
        // answered on @ALLCALL in the last 15 minutes (processCommandActivity.cpp:303-311), and
        // every heartbeat that gets this far enters it, acknowledged or stopped by the 55-minute
        // rule below (:719-723). Checked before that rule, as JS8Call checks it: a heartbeat held
        // off here is not counted by the rule.
        if self.allcall_held_off(&to, now_ms) {
            actions.push(StationAction::HbAckHeldOff { from: to });
            return;
        }
        // JS8Call 3.0's 55-minute rule, the last check before it answers
        // (processCommandActivity.cpp:708-717).
        if self.cfg.hb_rate_limit && self.hb_rate_held_off(&to, now_ms, actions) {
            return;
        }
        let mut text = format!("{to} HEARTBEAT SNR {}", format_snr(m.snr_db));
        // 3.0's note (processCommandActivity.cpp:661-706): `MSG ID n`, and `+k)` when k more wait
        // past it — the parenthesis unopened, as JS8Call 3.0.3 keys it. A heartbeat to a group I
        // joined is offered that group's message when none is held for the station itself.
        let group = self.joined_group(&m.to_text);
        if let Some((id, more)) = self.offer_for(&to, group.as_deref(), now_ms) {
            text.push_str(&if more > 0 {
                format!(" MSG ID {id} +{more})")
            } else {
                format!(" MSG ID {id}")
            });
        }
        // On a free heartbeat spot, as JS8Call's `sendHeartbeatAck` picks it
        // (`findFreeFreqOffset(500, 1000, 50)`, mainwindow.cpp:6299); `next_frame` picks it.
        let spot = self.hb_spot();
        self.schedule_reply(Origin::HbAck, &to, &text, spot, now_ms, actions);
    }

    /// Where a heartbeat or its acknowledgement keys: a free spot in 500–1000 Hz, which
    /// `next_frame` picks, or with "Allow heartbeat transmissions outside of heartbeat
    /// sub-channel" on, the operator's own offset (JS8Call's `-1`, which `processTxQueue` reads as
    /// `freq()`: `sendHB`, `sendHeartbeatAck`, mainwindow.cpp:4442-4447, :4471-4472, :6614-6616).
    fn hb_spot(&self) -> FreqHint {
        if self.cfg.hb_anywhere {
            FreqHint::Dial
        } else {
            FreqHint::HbSubband(0.0)
        }
    }

    /// JS8Call 3.0's 55-minute rule (`processHeartbeatRateLimit`, processHeartbeatRateLimit.cpp:
    /// 8-38), for a station it has not stopped: one it stopped is on the "never acknowledge" list,
    /// by its call as heard, as JS8Call writes it into its HB blacklist (`addToHbBlacklist`,
    /// :31; Configuration.cpp:1226-1231), and `maybe_hb_ack` turns it away first, by that call or
    /// its base call (processCommandActivity.cpp:655-661, :713-715). A station acknowledged less
    /// than [`HB_RATE_WINDOW_MS`] ago is stopped now, for good, and its time forgotten (:26-33);
    /// any other has its time kept, and is acknowledged (:36-37).
    fn hb_rate_held_off(
        &mut self,
        who: &str,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) -> bool {
        match self.hb_acked_at.get(who) {
            Some(&last) if now_ms.saturating_sub(last) < HB_RATE_WINDOW_MS => {
                self.hb_acked_at.remove(who);
                // Once, as heard (`if (!hb_blacklist_.contains(callsign)) append`), and never
                // into a list already `MAX_HB_RATE` long: it grows from calls heard on the air.
                let deny = &mut self.cfg.hb_ack_deny;
                if !deny.iter().any(|c| c == who) {
                    if deny.len() < MAX_HB_RATE {
                        deny.push(who.to_string());
                    } else if !self.hb_rate_overflow.iter().any(|c| c == who) {
                        // A full list takes no more, but the rule still holds: the station is
                        // kept in the rule's own record, which `maybe_hb_ack` reads as well.
                        self.hb_rate_overflow.push(who.to_string());
                        let over = self.hb_rate_overflow.len().saturating_sub(MAX_HB_RATE);
                        self.hb_rate_overflow.drain(..over);
                    }
                }
                if !self.hb_rate_blocked.iter().any(|c| c == who) {
                    self.hb_rate_blocked.push(who.to_string());
                }
                self.keep_listed_rate_blocks();
                actions.push(StationAction::HbAckRateLimited {
                    from: who.to_string(),
                });
                true
            }
            _ => {
                self.hb_acked_at.insert(who.to_string(), now_ms);
                // A time past the window holds nothing back, so the map limits itself; the cap
                // is for a flood inside one window.
                self.hb_acked_at
                    .retain(|_, &mut t| now_ms.saturating_sub(t) < HB_RATE_WINDOW_MS);
                while self.hb_acked_at.len() > MAX_HB_RATE {
                    let Some(oldest) = self
                        .hb_acked_at
                        .iter()
                        .min_by_key(|(_, &t)| t)
                        .map(|(k, _)| k.clone())
                    else {
                        break;
                    };
                    self.hb_acked_at.remove(&oldest);
                }
                false
            }
        }
    }

    fn autoreply(
        &mut self,
        m: &Message,
        cmd: Command,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) {
        if cmd == Command::Msg {
            self.ack(m, m.text.trim(), now_ms, actions);
            return;
        }
        // The reply names the sender as heard, `/P` included (JS8Call's `d.from`,
        // mainwindow.cpp:8834-8903).
        let from = clamp_str(&m.from, MAX_CALL_LEN).to_ascii_uppercase();
        let reply = match cmd {
            Command::SnrQuery => Some(format!("{from} SNR {}", format_snr(m.snr_db))),
            // Nothing set, nothing said: JS8Call skips an empty grid or INFO
            // (mainwindow.cpp:8861-8863, :8841-8843).
            Command::GridQuery if self.cfg.grid.is_empty() => None,
            Command::InfoQuery if self.cfg.info.is_empty() => None,
            Command::GridQuery => Some(format!("{from} GRID {}", self.cfg.grid)),
            Command::InfoQuery => Some(format!(
                "{from} INFO {}",
                self.expand_macros(&self.cfg.info, now_ms)
            )),
            Command::StatusQuery => Some(format!("{from} STATUS {}", self.status_text(now_ms))),
            Command::HearingQuery => Some(self.hearing_reply(&from, now_ms)),
            Command::Nack | Command::Ack => None, // acks are logged, not answered
            _ => None,
        };
        if let Some(text) = reply {
            self.schedule_reply(
                Origin::AutoReply,
                &from,
                &text,
                FreqHint::Dial,
                now_ms,
                actions,
            );
        }
    }

    /// JS8Call's answer to a MSG it filed (mainwindow.cpp:9127-9139) or a `MSG TO:` it stored
    /// (:9027-9051): `<from> ACK` to the sender as it was heard (`d.from`, so a `/P` or compound
    /// call stays whole), or `<path> ACK` when a relay brought it (`calls.length() > 1 ?
    /// d.relayPath : d.from`, the path `parseRelayPathCallsigns` reads from `text`: a MSG's
    /// whole text, a `MSG TO:`'s text after the target, as the inbox keeps them). It asks or goes
    /// as every automatic reply does, and with AUTO off it is put in the compose box, never
    /// keyed: JS8Call types it there either way but keys it only with AUTO checked
    /// (`processTxQueue`, :9674-9685; the reply has no trailing space, so the `" ACK "` test
    /// there never matches it). A copy heard while its ACK still waits draws no second one:
    /// JS8Call's waiting ACK sits in the compose box until it has gone, and no reply is queued
    /// while the box holds text (:9365). A path over `MAX_PATH_HOPS` is refused, as a relay
    /// chain is in `handle_relay`: its calls come from the sender's text, and upstream keys all
    /// of them.
    fn ack(&mut self, m: &Message, text: &str, now_ms: u64, actions: &mut Vec<StationAction>) {
        let from = clamp_str(&m.from, MAX_CALL_LEN).to_ascii_uppercase();
        let path = relay_path(&from, &clamp_str(text, MAX_TEXT_LEN));
        if path.len() > MAX_PATH_HOPS {
            actions.push(StationAction::Toast {
                text: format!("Relay chain over {MAX_PATH_HOPS} hops refused"),
                directed_to_me: false,
            });
            return;
        }
        let who = if path.len() > 1 {
            path.join(">")
        } else {
            from.clone()
        };
        let text = format!("{who} ACK");
        if self.reply_waits(&text) {
            return;
        }
        self.schedule_reply(
            Origin::AutoReply,
            &from,
            &text,
            FreqHint::Dial,
            now_ms,
            actions,
        );
    }

    /// A reply with exactly this text still waits: counting down, or queued and not yet wholly
    /// sent (a queued message's display is `MYCALL: text`, `compose_out`'s).
    fn reply_waits(&self, text: &str) -> bool {
        let display = format!("{}: {text}", self.base());
        self.pending.iter().any(|p| p.text == text)
            || self.outbox.iter().any(|o| o.display == display)
            || self.queued.iter().any(|r| r.text == text)
            || self.composer.as_ref().is_some_and(|c| c.1 == text)
    }

    fn handle_relay(&mut self, m: &Message, now_ms: u64, actions: &mut Vec<StationAction>) {
        let rest = clamp_str(m.text.trim(), MAX_TEXT_LEN);
        if m.from.contains('>') {
            // I am the tail of a relay chain `A>B>…>me`: answer `A>B>…>me ACK`. Upstream has NO
            // hop counter, so a chain can grow forever — we cap it at MAX_PATH_HOPS (a deliberate,
            // safety-motivated divergence: over the cap the chain is refused, not relayed).
            let mut path: Vec<String> = m
                .from
                .split('>')
                .take(MAX_PATH_HOPS - 1)
                .map(|s| clamp_str(s.trim(), MAX_CALL_LEN))
                .collect();
            path.push(self.base());
            if m.from.split('>').count() >= MAX_PATH_HOPS {
                actions.push(StationAction::Toast {
                    text: format!("Relay chain over {MAX_PATH_HOPS} hops refused"),
                    directed_to_me: false,
                });
                return;
            }
            let chain = path.join(">");
            let text = format!("{chain} ACK");
            actions.push(StationAction::Relayed {
                path,
                text: text.clone(),
            });
            self.schedule_reply(
                Origin::Relay,
                &m.from,
                &text,
                FreqHint::Dial,
                now_ms,
                actions,
            );
        } else {
            // A relay REQUEST to me: retransmit the payload with `*DE* MYCALL`.
            let text = format!("{rest} *DE* {}", self.base());
            let path = vec![clamp_str(&m.from, MAX_CALL_LEN), self.base()];
            actions.push(StationAction::Relayed {
                path,
                text: text.clone(),
            });
            self.schedule_reply(
                Origin::Relay,
                &m.from,
                &text,
                FreqHint::Dial,
                now_ms,
                actions,
            );
        }
    }

    fn handle_msg_to(&mut self, m: &Message, now_ms: u64, actions: &mut Vec<StationAction>) {
        // `MSG TO:` — store the body for the named target (the first token of the body), held
        // under its base call as JS8Call holds it, `cd.to = Radio::base_callsign(to)`
        // (mainwindow.cpp:9042): VE3/W1AW's and W1AW/P's are held for W1AW.
        let body = m.text.trim();
        let (target, text) = body.split_once(' ').unwrap_or((body, ""));
        let target = radio_base_callsign(target);
        if target.is_empty() {
            return;
        }
        let id = self.next_inbox_id;
        self.next_inbox_id += 1;
        self.inbox.push(InboxEntry {
            id,
            // Its sender as heard, as JS8Call stores it (`cd.from = d.from`, :9036), so the
            // delivery names it so too.
            from: clamp_str(&m.from, MAX_CALL_LEN).to_ascii_uppercase(),
            to: clamp_str(&target, MAX_CALL_LEN),
            text: clamp_str(text, MAX_TEXT_LEN),
            path: Vec::new(),
            state: InboxState::Store,
            at_ms: now_ms,
            freq_hz: m.freq_hz,
            snr_db: m.snr_db,
        });
        actions.push(StationAction::InboxChanged);
        self.prune_inbox(now_ms, actions); // cap count + bytes, drop oldest, never silently
        if !self.idle_tripped && !self.is_me(&m.from) {
            // …and acknowledged as JS8Call acknowledges the store (:9027-9051), along the relay
            // path read from the text after the target's call: an automatic reply, so with AUTO
            // off it goes to the compose box.
            self.ack(m, text, now_ms, actions);
        }
    }

    /// A MSG to me (or a group I joined) into my inbox as `Unread`: JS8Call's
    /// `addCommandToMyInbox` → `addCommandToStorage("UNREAD", d)` (mainwindow.cpp:9462-9467),
    /// with its PATH (`parseRelayPathCallsigns`, :9127). Its sender is kept as heard, `/P`
    /// included (`{"FROM", d.from}`, mainwindow.cpp:6356), the key the heard list keeps it under,
    /// so the station's row in the Stations list is the one the unread message flags.
    fn file_msg_to_me(&mut self, m: &Message, now_ms: u64, actions: &mut Vec<StationAction>) {
        let from = clamp_str(&m.from, MAX_CALL_LEN).to_ascii_uppercase();
        let text = clamp_str(m.text.trim(), MAX_TEXT_LEN);
        let path = relay_path(&from, &text);
        let id = self.next_inbox_id;
        self.next_inbox_id += 1;
        self.inbox.push(InboxEntry {
            id,
            from,
            to: clamp_str(&m.to_text, MAX_CALL_LEN),
            text,
            path,
            state: InboxState::Unread,
            at_ms: now_ms,
            freq_hz: m.freq_hz,
            snr_db: m.snr_db,
        });
        actions.push(StationAction::InboxChanged);
        self.prune_inbox(now_ms, actions);
    }

    /// The receive side of the APRS-IS relay (JS8Call 3.0.3, processCommandActivity.cpp:536-572
    /// and :765-805): `text` is `<CALL> <message>`; when CALL is my call as set, the message is
    /// filed UNREAD from "APRS", by the path "APRS", unless the same `TO|TEXT|SENDER` was filed in
    /// the last 60 s. SENDER is the word after a closing `DE`, else "APRS"; the text keeps its
    /// `DE <sender>`, as JS8Call files it.
    fn file_aprs_relay(
        &mut self,
        m: &Message,
        text: &str,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) {
        let (to, body) = text.split_once(' ').unwrap_or((text, ""));
        let body = clamp_str(body.trim(), MAX_TEXT_LEN);
        if !to.eq_ignore_ascii_case(self.cfg.mycall.trim()) || body.is_empty() {
            return;
        }
        let words: Vec<&str> = body.split_whitespace().collect();
        let sender = match words.as_slice() {
            // `\s+DE\s+(\S+)\s*$`: a DE with a word before it and one after it, at the end.
            [_, .., de, who] if *de == "DE" => who,
            _ => "APRS",
        };
        let key = format!("{to}|{body}|{sender}").to_ascii_uppercase();
        // Kept while it is at or after `now − 60 s`: JS8Call erases one only when it is OLDER than
        // its cutoff (`it.value() < cutoff`).
        self.aprs_filed
            .retain(|_, at| now_ms.saturating_sub(*at) <= APRS_FILE_DEDUPE_MS);
        if self.aprs_filed.contains_key(&key) {
            return;
        }
        self.aprs_filed.insert(key, now_ms);
        let id = self.next_inbox_id;
        self.next_inbox_id += 1;
        self.inbox.push(InboxEntry {
            id,
            from: "APRS".into(),
            to: clamp_str(to, MAX_CALL_LEN).to_ascii_uppercase(),
            text: body,
            path: vec!["APRS".into()],
            state: InboxState::Unread,
            at_ms: now_ms,
            freq_hz: m.freq_hz,
            snr_db: m.snr_db,
        });
        actions.push(StationAction::InboxChanged);
        self.prune_inbox(now_ms, actions);
    }

    /// `mine`: addressed to my call or a group I joined (not @ALLCALL, not another station).
    fn handle_query_msg(
        &mut self,
        m: &Message,
        mine: bool,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) {
        // The querier as heard names the reply (`replyPath = d.from`, mainwindow.cpp:9248,
        // :9270-9285) and keys the @ALLCALL hold-off (`d.from`), and a held message is found for
        // it as JS8Call finds one: under that call or its base call (QUERY MSG n, :9219; QUERY
        // MSGS, :9268 → :9508-9533), among those whose hand-over fits this speed (`offerable`).
        let from = clamp_str(&m.from, MAX_CALL_LEN).to_ascii_uppercase();
        // The query addressed to a group I joined: JS8Call's `isGroupCall`
        // (processCommandActivity.cpp:31), which brings that group's messages in.
        let group = self.joined_group(&m.to_text);
        // `QUERY MSG n` delivers the stored message n (if it is for the querier and has text,
        // :9223-9226) and marks it, only when the query was addressed to me (mainwindow.cpp:8601,
        // :9171). Anything else it may be draws no reply at all: JS8Call's buffered QUERY skips
        // every miss (:9194-9233) and never answers it as a QUERY MSGS.
        if m.cmd != Some(Command::QueryMsgs) {
            let Some(id) = query_msg_id(m).filter(|_| mine) else {
                return;
            };
            let inbox = self.offerable(&from);
            // A group's message goes to any station that asks for it, and again to one that asks
            // again: JS8Call checks nothing but the message's `@` ("allow any message to a @GROUP
            // to be retrieved by anybody", processCommandActivity.cpp:905-912) and records the
            // hand-over (`markGroupMsgDeliveredForCallsign`, :931-934) only so that QUERY MSGS
            // and a heartbeat no longer offer it to that station.
            if let Some(e) = inbox.iter().find(|e| {
                e.id == id
                    && has_text(e)
                    && (held_for(e, &from)
                        || (held::is_group(&e.to) && e.state == InboxState::Store))
            }) {
                // 3.0's reply (:938-975): the message, then the next one waiting, `NEXT MSG ID m`,
                // and `+k` when k more wait past that — counted before this one is marked.
                let bare = hand_over_text(e, &from);
                let mut deliver = bare.clone();
                let next = held::lookahead_for(&inbox, &from, id).or_else(|| {
                    held::is_group(&e.to)
                        .then(|| {
                            held::group_lookahead_for(
                                &inbox,
                                &self.group_recipients,
                                &m.to_text,
                                &from,
                                id,
                                now_ms,
                            )
                        })
                        .flatten()
                });
                if let Some(next) = next {
                    let more = held::pending(
                        &inbox,
                        &self.group_recipients,
                        &from,
                        group.as_deref(),
                        now_ms,
                        2,
                    );
                    deliver.push_str(&if more > 0 {
                        format!(" NEXT MSG ID {next} +{more}")
                    } else {
                        format!(" NEXT MSG ID {next}")
                    });
                    // A hand-over that fits one transmission at this speed only without its NEXT
                    // MSG ID goes without it, rather than never; the next one is offered on the
                    // next QUERY MSGS. (JS8Call, with no such cap, always names it.) So does one
                    // the reply's length cap would cut into: the cap must never take a word of it.
                    if self.too_long(&deliver, self.cfg.speed).is_some()
                        || deliver.len() > MAX_TEXT_LEN
                    {
                        deliver = bare;
                    }
                }
                // Marked Delivered as the reply is released, not here: JS8Call marks it in the
                // reply's callback (mainwindow.cpp:9227-9235), so a No leaves it held. A delivery
                // already waiting draws no second one.
                if !self.reply_waits(&deliver) {
                    self.schedule(
                        Origin::AutoReply,
                        &from,
                        &deliver,
                        FreqHint::Dial,
                        Some((id, from.clone())),
                        now_ms,
                        actions,
                    );
                }
            }
            return;
        }
        // `QUERY MSGS` (do you have any for me?) → offer the first stored id, else NO. On
        // @ALLCALL, JS8Call answers only YES, and says NO to a directed query alone
        // (mainwindow.cpp:9255-9278); an @ALLCALL reply then holds the sender off for the
        // interval (:9377-9379, checked first at :8812).
        if self.cfg.autoreply && self.addressed_to_me(&m.to_text) {
            let to_allcall = m.to_text.eq_ignore_ascii_case("@ALLCALL");
            // 3.0's YES (processCommandActivity.cpp:995-1045): `YES MSG ID n`, and `+k` when k
            // more wait past it; a query to a group I joined is offered that group's message
            // when none is held for the querier itself.
            let reply = match self.offer_for(&from, group.as_deref(), now_ms) {
                Some((id, more)) if more > 0 => format!("{from} YES MSG ID {id} +{more}"),
                Some((id, _)) => format!("{from} YES MSG ID {id}"),
                None if to_allcall => return,
                None => format!("{from} NO"),
            };
            if to_allcall && self.allcall_held_off(&from, now_ms) {
                actions.push(StationAction::RateLimited { from });
                return;
            }
            self.schedule_reply(
                Origin::AutoReply,
                &from,
                &reply,
                FreqHint::Dial,
                now_ms,
                actions,
            );
        }
    }

    /// JS8Call's @ALLCALL cache (`m_txAllcallCommandCache`): a station answered on @ALLCALL, or
    /// whose heartbeat was acknowledged, is held off for `allcall_reply_interval_ms` (15 minutes;
    /// processCommandActivity.cpp:303-311), keyed on its call as heard (`d.from`), so W1AW/P is
    /// not W1AW. True while it is held off; otherwise it is recorded as answered now (:719-723,
    /// :1097-1100, :1148-1150).
    fn allcall_held_off(&mut self, from: &str, now_ms: u64) -> bool {
        if let Some(&last) = self.allcall_replied.get(from) {
            if now_ms.saturating_sub(last) < self.cfg.allcall_reply_interval_ms {
                return true;
            }
        }
        self.allcall_replied.insert(from.to_string(), now_ms);
        self.prune_allcall(now_ms); // keep the map self-limiting, not one entry per call ever
        false
    }

    /// Hand over the held message `id` to `who` (a `QUERY MSG n` delivery being released):
    /// Delivered, so it is offered no more (`markMsgDelivered`, mainwindow.cpp:9227-9235). A
    /// group's message stays held for the group and is marked handed to `who` alone
    /// (`markGroupMsgDeliveredForCallsign`, Inbox.cpp:418-466).
    fn mark_delivered(
        &mut self,
        delivers: Option<(u32, String)>,
        actions: &mut Vec<StationAction>,
    ) {
        let Some((id, who)) = delivers else { return };
        let Some(e) = self
            .inbox
            .iter_mut()
            .find(|e| e.id == id && e.state == InboxState::Store)
        else {
            return;
        };
        if !held::is_group(&e.to) {
            e.state = InboxState::Delivered;
        } else if !held::handed(&self.group_recipients, id, &who) {
            self.group_recipients.push((id, who));
            if self.group_recipients.len() > MAX_GROUP_RECIPIENTS {
                self.group_recipients.remove(0);
            }
        } else {
            return;
        }
        actions.push(StationAction::InboxChanged);
    }

    /// What a heartbeat's acknowledgement or a `QUERY MSGS` offers `who` (the station as heard):
    /// the message held for it first, else, for a heartbeat or query addressed to a group I joined,
    /// that group's next one for it; with JS8Call 3.0's count of how many more wait past it
    /// (processCommandActivity.cpp:661-706, :995-1045). Only those whose hand-over to `who` fits
    /// one transmission at this speed (`offerable`).
    fn offer_for(&self, who: &str, group: Option<&str>, now_ms: u64) -> Option<(u32, i64)> {
        let inbox = self.offerable(who);
        let id = Self::stored_for(&inbox, who).or_else(|| {
            group.and_then(|g| held::group_next_for(&inbox, &self.group_recipients, g, who, now_ms))
        })?;
        let more = held::pending(&inbox, &self.group_recipients, who, group, now_ms, 1);
        Some((id, more))
    }

    /// `to` names a group I joined (JS8Call's `isGroupCallIncluded`, mainwindow.cpp:6092-6094).
    fn joined_group(&self, to: &str) -> Option<String> {
        self.cfg
            .groups
            .iter()
            .find(|g| g.eq_ignore_ascii_case(to))
            .map(|g| g.to_ascii_uppercase())
    }

    /// QUERY CALL, "can you hear this station?" (processCommandActivity.cpp:1052-1101), answered
    /// only with AUTO on (:1053) and only when the call asked about is heard and its callsign aging
    /// has not passed (:1080-1084): `<from> YES <snr> (<age>)`, the SNR and age of the station
    /// as last heard. JS8Call matches the call asked about against each heard call and its base
    /// call, in the heard list's key order (:1074-1095), and its list keeps each call as heard: a
    /// question naming W1AW/P is answered for W1AW/P, and one naming W1AW for W1AW or, by its base
    /// call, W1AW/P. On @ALLCALL it holds the asker off for the reply interval, as every @ALLCALL
    /// reply does (:1097-1100, :1128-1131).
    fn handle_query_call(
        &mut self,
        m: &Message,
        allcall: bool,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) {
        if !self.cfg.autoreply {
            return;
        }
        let Some(call) = first_callsign(&m.text) else {
            return;
        };
        let mut heard: Vec<&Heard> = self
            .heard
            .iter()
            .filter(|h| !self.aged(h, now_ms))
            .collect();
        heard.sort_by(|a, b| a.call.cmp(&b.call));
        let Some(h) = heard
            .into_iter()
            .find(|h| call == h.call || call == radio_base_callsign(&h.call))
        else {
            return;
        };
        let report = format!(
            "{} ({})",
            format_snr(h.snr_db),
            since(now_ms.saturating_sub(h.last_ms) / 1000)
        );
        let from = clamp_str(&m.from, MAX_CALL_LEN).to_ascii_uppercase();
        let reply = format!("{from} YES {}", report.trim());
        if allcall && self.allcall_held_off(&from, now_ms) {
            actions.push(StationAction::RateLimited { from });
            return;
        }
        self.schedule_reply(
            Origin::AutoReply,
            &from,
            &reply,
            FreqHint::Dial,
            now_ms,
            actions,
        );
    }

    /// AGN?, "say again" (processCommandActivity.cpp:625-629): my last message again, as a
    /// receiver decoded it, whatever sent it (`m_lastTxMessage`, mainwindow.cpp:3652-3656), its
    /// trailing space gone (`Varicode::rstrip`); nothing when I have sent nothing. An automatic
    /// reply like the rest, so with AUTO off it goes to the compose box. The leading `MYCALL:` that
    /// JS8Call types into its compose box is the one compose drops (AUTO_REMOVE_MYCALL, varicode.cpp:
    /// 2001-2004), so it is left off here and the frames are the same; a checksum my last message
    /// carried is part of its text, and goes again, as in JS8Call.
    fn handle_agn(&mut self, m: &Message, now_ms: u64, actions: &mut Vec<StationAction>) {
        let last = self.last_tx_message.trim_end();
        let mycall = self.cfg.mycall.trim().to_ascii_uppercase();
        let text = [format!("{mycall}:"), format!("{mycall} ")]
            .iter()
            .find_map(|p| last.strip_prefix(p.as_str()))
            .map(str::trim_start)
            .unwrap_or(last)
            .to_string();
        if text.is_empty() {
            return;
        }
        let from = clamp_str(&m.from, MAX_CALL_LEN).to_ascii_uppercase();
        self.schedule_reply(
            Origin::AutoReply,
            &from,
            &text,
            FreqHint::Dial,
            now_ms,
            actions,
        );
    }

    /// The message held for `who` (the station as heard) that is offered first, as JS8Call's
    /// `getNextMessageIdForCallsign` finds it (mainwindow.cpp:9508-9533): one held under the
    /// call as heard, else one held under its base call (`Radio::base_callsign`), the oldest
    /// first in each, skipping any with no text (:9516-9520, :9525-9529).
    fn stored_for(inbox: &[InboxEntry], who: &str) -> Option<u32> {
        let held_under = |call: &str| {
            inbox
                .iter()
                .find(|e| {
                    e.state == InboxState::Store && e.to.eq_ignore_ascii_case(call) && has_text(e)
                })
                .map(|e| e.id)
        };
        held_under(who).or_else(|| held_under(&radio_base_callsign(who)))
    }

    /// The frames `text` needs and the cap at `speed`, when it would not fit one transmission
    /// there (the §97.119 cap, `compose::max_frames`).
    fn too_long(&self, text: &str, speed: Speed) -> Option<(usize, usize)> {
        match frames_with_grid(&self.cfg.mycall, &self.cfg.grid, None, text, speed) {
            Err(ComposeError::TooLong { frames, max }) => Some((frames, max)),
            _ => None,
        }
    }

    /// The hand-over of held message `e` to `who`, `<who> MSG <text> FROM <from>` (the delivery
    /// a QUERY MSG n draws), when it would not fit one transmission at `speed`: its frames and the
    /// cap. Store… refuses such a message up front; one held from the air can be one, or become
    /// one when the speed goes down.
    fn hand_over_too_long(
        &self,
        e: &InboxEntry,
        who: &str,
        speed: Speed,
    ) -> Option<(usize, usize)> {
        self.too_long(&hand_over_text(e, who), speed)
    }

    /// The inbox as the held-message lookups see it for `who`: without a held message whose
    /// hand-over to `who` would not fit one transmission at the current speed. Such a message is
    /// not offered (QUERY MSGS, a heartbeat's acknowledgement), counted (`+k`), named next (NEXT
    /// MSG ID) or handed over (QUERY MSG n) until the speed allows it, where it was offered and
    /// then dropped without a word each time its delivery failed to compose. JS8Call has no such
    /// cap. The Inbox says why (`held_too_long`).
    fn offerable(&self, who: &str) -> Vec<InboxEntry> {
        self.inbox
            .iter()
            .filter(|e| {
                e.state != InboxState::Store
                    || self.hand_over_too_long(e, who, self.cfg.speed).is_none()
            })
            .cloned()
            .collect()
    }

    /// A set STATUS, or JS8Call's default, "IDLE <MYIDLE> VERSION <MYVERSION>"
    /// (Configuration.cpp:1858), with Nexus for the version.
    fn status_text(&self, now_ms: u64) -> String {
        if self.cfg.status.is_empty() {
            format!(
                "IDLE {} VERSION Nexus",
                idle_since(self.idle_minutes(now_ms))
            )
        } else {
            clamp_str(&self.expand_macros(&self.cfg.status, now_ms), MAX_INFO_LEN)
        }
    }

    /// JS8Call's macros in a text this station makes itself (the CQ, the INFO? and STATUS?
    /// replies): every one it can value, then every `<…>` left removed, as JS8Call's `sendCQ` and
    /// replies call `replaceMacros(…, true)` (mainwindow.cpp:4541; processCommandActivity.cpp:
    /// 347-358). A station has no selected call, so `<CALL>`, `<SNR>` and `<TDELTA>` go.
    fn expand_macros(&self, text: &str, now_ms: u64) -> String {
        let values = self.macro_values(self.idle_minutes(now_ms));
        crate::proto::macros::replace(text, &values, true)
    }

    /// JS8Call's macro values for this station (`buildMacroValues`, mainwindow.cpp:4848-4888),
    /// idle `idle_minutes`. A station has no selected call: no value for `<CALL>`, `<SNR>` or
    /// `<TDELTA>`.
    fn macro_values(&self, idle_minutes: u16) -> crate::proto::macros::Values {
        crate::proto::macros::values(&crate::proto::macros::MacroInput {
            mycall: &self.cfg.mycall,
            grid: &self.cfg.grid,
            info: &self.cfg.info,
            status: &self.cfg.status,
            cq: &self.cfg.cq_text,
            reply: &self.cfg.reply_text,
            idle_minutes,
            selected: None,
        })
    }

    /// My INFO as JS8Call's spot client announces it (`spotSetLocal`, mainwindow.cpp:5543-5547):
    /// every macro replaced and the rest removed, as the `INFO?` reply has it.
    pub fn info_text(&self, now_ms: u64) -> String {
        self.expand_macros(&self.cfg.info, now_ms)
    }

    /// JS8Call's `<MYGRID4>` and `<MYGRID12>` (`buildMacroValues`, mainwindow.cpp:7024-7025): the
    /// locator's first 4 and first 12 characters, upper-cased, stand wherever the token does
    /// (`replaceMacros`, :181-194, which walks its map in key order, `<MYGRID12>` first). JS8Call's
    /// editor has upper-cased the text by then; the token is matched here whatever its case, for
    /// the same result. Its other macros are not Nexus's, and their tokens are left as typed.
    fn expand_grid_macros(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (token, n) in [("<MYGRID12>", 12), ("<MYGRID4>", 4)] {
            let value: String = self.cfg.grid.chars().take(n).collect();
            out = replace_ignoring_ascii_case(&out, token, &value.to_ascii_uppercase());
        }
        out
    }

    /// JS8Call's HEARING? reply (mainwindow.cpp:8868-8905): "<FROM> HEARING" (:8903), then up to
    /// four calls, newest first (:8873-8883), never the station that asked (:8890) and none its
    /// callsign aging has passed (:8894), a setting that ships off.
    fn hearing_reply(&self, from: &str, now_ms: u64) -> String {
        // The heard list keeps each call as heard (`record_heard`), and JS8Call leaves out the
        // asker by that key (`call == d.from`, processCommandActivity.cpp:391).
        let mut calls: Vec<&Heard> = self
            .heard
            .iter()
            .filter(|h| h.call != from && !self.aged(h, now_ms))
            .collect();
        calls.sort_by(|a, b| b.last_ms.cmp(&a.last_ms));
        let mut words = vec![format!("{from} HEARING")];
        words.extend(calls.iter().take(4).map(|h| h.call.clone()));
        words.join(" ")
    }

    /// JS8Call's callsign-aging test, `callsignAging && cd.utcTimestamp.secsTo(now) / 60 >=
    /// callsignAging` (mainwindow.cpp:8894, :2021): whole minutes since the call was last heard.
    fn aged(&self, h: &Heard, now_ms: u64) -> bool {
        let aging = u64::from(self.cfg.callsign_aging_min);
        aging > 0 && now_ms.saturating_sub(h.last_ms) / 60_000 >= aging
    }

    fn schedule_reply(
        &mut self,
        origin: Origin,
        to: &str,
        text: &str,
        freq_hint: FreqHint,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) {
        self.schedule(origin, to, text, freq_hint, None, now_ms, actions);
    }

    /// Every automatic reply's one way in. With `autoreply_confirmation` on it waits for the
    /// operator's Yes, and answers No by itself after [`CONFIRM_NO_AFTER_MS`], as JS8Call's
    /// `confirmThenEnqueueMessage(90, …)` box does (mainwindow.cpp:5209-5230, :9385-9389).
    /// Otherwise it is released at once, JS8Call's `enqueueMessage` (:9388): `processTxQueue`
    /// dequeues it within the second (:9627-9690) and its TX starts at the next period
    /// (`guiUpdate`, :4510-4521), which here is the next `next_frame`. `delivers` is the held
    /// message a `QUERY MSG n` hands over and the station it goes to, marked as the reply is
    /// released.
    #[allow(clippy::too_many_arguments)]
    fn schedule(
        &mut self,
        origin: Origin,
        to: &str,
        text: &str,
        freq_hint: FreqHint,
        delivers: Option<(u32, String)>,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) {
        // JS8Call queues no reply while its compose box holds text (:9364-9367) or while a message
        // to me is still arriving (`hasExistingMessageBufferToMe`, :9369-9374), and both come
        // before the question is asked. Its HB-ACK takes another path (`sendHeartbeatAck`) that
        // checks neither: `maybe_hb_ack` has its own rule.
        if origin != Origin::HbAck && (self.box_busy() || self.rx_to_me) {
            let why = if self.rx_to_me {
                "a message to you is still arriving"
            } else {
                "the message box holds text"
            };
            actions.push(StationAction::Toast {
                text: format!("Auto-reply not made, {why}: {text}"),
                directed_to_me: false,
            });
            return;
        }
        // Cap the pending auto-reply queue: under a flood, refuse and toast rather than growing.
        // Refusing an auto-reply is correct — the alternative keys stale traffic for hours.
        if self.pending.len() >= MAX_PENDING {
            actions.push(StationAction::Toast {
                text: "Reply queue full: auto-reply dropped".into(),
                directed_to_me: false,
            });
            return;
        }
        let to = clamp_str(to, MAX_CALL_LEN);
        let text = clamp_str(text, MAX_TEXT_LEN);
        if !self.cfg.autoreply_confirmation {
            self.release(origin, &text, freq_hint, delivers, actions);
            return;
        }
        let fires_at_ms = now_ms + CONFIRM_NO_AFTER_MS;
        let display = format!("{}: {text}", self.base());
        self.pending.push(Pending {
            origin,
            to: to.clone(),
            text,
            display: display.clone(),
            fires_at_ms,
            freq_hint,
            delivers,
        });
        actions.push(StationAction::ReplyPending {
            origin,
            to: to.to_string(),
            display,
            fires_at_ms,
        });
    }

    /// A reply goes into JS8Call's message queue (`enqueueMessage`, mainwindow.cpp:5232-5238),
    /// where `process_tx_queue` takes it once the compose box is free. A full queue refuses it,
    /// said, rather than queue it behind stale traffic.
    fn release(
        &mut self,
        origin: Origin,
        text: &str,
        freq_hint: FreqHint,
        delivers: Option<(u32, String)>,
        actions: &mut Vec<StationAction>,
    ) {
        if self.queued.len() >= MAX_PENDING {
            actions.push(StationAction::Toast {
                text: "Reply queue full: auto-reply dropped".into(),
                directed_to_me: false,
            });
            return;
        }
        self.queued.push_back(Queued {
            origin,
            text: text.to_string(),
            freq_hint,
            delivers,
        });
    }

    /// The compose box holds text: the operator is typing, a reply the station put there waits
    /// to be taken, or a message is going out (JS8Call's box holds a message until its last frame
    /// is sent, `stopTx`, mainwindow.cpp:4826-4833; here until that frame is handed out, which
    /// keys nothing differently: nothing is heard while it is on the air).
    fn box_busy(&self) -> bool {
        self.composing || self.composer.is_some() || !self.outbox.is_empty()
    }

    /// JS8Call's `processTxQueue` (mainwindow.cpp:9627-9690), run once a second, after each
    /// batch of decodes, after an answer and when the cockpit's box changes: once the box is free
    /// and nothing is going out (:9652-9659), the oldest waiting reply is taken. With AUTO on (or
    /// for a heartbeat acknowledgement, whose " HEARTBEAT " text JS8Call keys whatever AUTO says,
    /// :9674-9685, or for the APRS-IS relay's High-priority message, which it keys the same way)
    /// it goes to the outbox, where the next period keys it; otherwise it is put in
    /// the compose box (`addMessageText`, :9671) and never keyed by itself. A delivery it carries
    /// is handed over then, as its callback runs once the reply is dequeued (:9687-9689). One at
    /// a time: the box holds one message.
    pub fn process_tx_queue(&mut self) -> Vec<StationAction> {
        let mut actions = Vec::new();
        if self.box_busy() {
            return actions;
        }
        let Some(r) = self.queued.pop_front() else {
            return actions;
        };
        if self.cfg.autoreply || matches!(r.origin, Origin::HbAck | Origin::AprsRelay) {
            // A reply that no longer composes (the speed went down while it asked, so it no longer
            // fits one transmission) is not sent, and said, never dropped without a word; a
            // delivery it carried stays held. A relay is said as a relay.
            let out = match self.compose_out(r.origin, &r.text, r.freq_hint) {
                Ok(out) => out,
                Err(e) => {
                    let why = match e {
                        ComposeError::TooLong { frames, max } => format!(
                            "it needs {frames} frames and one transmission at this speed holds \
                             {max}"
                        ),
                        ComposeError::NoCallsign => "no callsign is set".to_string(),
                        ComposeError::Empty => "it is empty".to_string(),
                    };
                    let what = if r.origin == Origin::AprsRelay {
                        "APRS-IS relay"
                    } else {
                        "Auto-reply"
                    };
                    actions.push(StationAction::Toast {
                        text: format!("{what} not sent, {why}: {}", r.text),
                        directed_to_me: false,
                    });
                    return actions;
                }
            };
            if self.push_outbox(out) {
                self.mark_delivered(r.delivers, &mut actions);
            }
        } else {
            self.mark_delivered(r.delivers, &mut actions);
            self.composer_seq = self.composer_seq.wrapping_add(1);
            self.composer = Some((self.composer_seq, r.text));
        }
        actions
    }

    /// The engine's word on its receive buffers: whether one is open (a message still
    /// arriving) and the destinations of those that carry a directed command (`buffer.cmd.to`).
    pub fn note_rx_buffers<'a>(&mut self, open: bool, to: impl IntoIterator<Item = &'a str>) {
        self.rx_open = open;
        self.rx_to_me = to.into_iter().any(|t| self.is_to_me(t));
    }

    /// The cockpit's word on its compose box: whether it holds text, and the id of the reply it
    /// took into the box since it last said (that one is now the cockpit's). Answers the reply
    /// waiting for the box, if one does; it stays offered until the cockpit takes it.
    pub fn sync_composer(
        &mut self,
        composing: bool,
        taken: Option<u32>,
    ) -> (Vec<StationAction>, Option<(u32, String)>) {
        if taken.is_some() && taken == self.composer.as_ref().map(|c| c.0) {
            self.composer = None;
        }
        self.composing = composing;
        let actions = self.process_tx_queue();
        (actions, self.composer.clone())
    }

    // ---- the clock tick ---------------------------------------------------------------------

    /// Once per second: advance the HB schedule and check the idle watchdog.
    pub fn tick(&mut self, now_ms: u64) -> Vec<StationAction> {
        let mut actions = Vec::new();
        self.expire_typeahead(now_ms);
        // reclaim aged state each tick (inbox 48 h expiry + caps, allcall interval prune).
        self.prune_inbox(now_ms, &mut actions);
        self.prune_allcall(now_ms);
        // JS8Call's processTxQueue runs once a second (mainwindow.cpp:4734-4737).
        actions.extend(self.process_tx_queue());
        // A reply nobody answered is No when its box runs out (SelfDestructMessageBox.cpp:68):
        // nothing is sent and a held message it would have handed over stays held.
        self.pending.retain(|p| {
            let no = now_ms >= p.fires_at_ms;
            if no {
                actions.push(StationAction::Toast {
                    text: format!("Auto-reply not sent, no answer: {}", p.display),
                    directed_to_me: false,
                });
            }
            !no
        });
        // idle watchdog
        if self.cfg.idle_watchdog_min > 0 && !self.idle_tripped {
            let limit = self.cfg.idle_watchdog_min as u64 * 60 * 1000;
            if now_ms.saturating_sub(self.last_activity_ms) >= limit {
                self.trip_idle(&mut actions);
            }
        }
        // CQ repeat schedule (JS8Call runs it off the same 1 Hz `checkRepeat` as the heartbeat,
        // mainwindow.cpp:5723). A SCHEDULE, never a queue: it enqueues only into an EMPTY
        // outbox with nothing pending, so a slow drain can never let CQs pile up, and
        // `bump_cq_schedule` re-bases on NOW, so a missed window is skipped rather than
        // replayed. If anything is queued, the CQ simply waits for the next tick — one frame
        // per period is the whole rule. A reply still asking does not hold it (see `next_frame`).
        if self.cq_on && self.outbox.is_empty() {
            if let Some(next) = self.cq_next_ms {
                if now_ms >= next {
                    let _ = self.enqueue_cq(now_ms);
                    if self.cfg.cq_interval_min > 0 {
                        self.bump_cq_schedule(now_ms);
                    } else {
                        self.cq_next_ms = None; // interval 0 = "on demand" = ONCE
                    }
                }
            }
        }
        actions
    }

    fn trip_idle(&mut self, actions: &mut Vec<StationAction>) {
        self.idle_tripped = true;
        self.cfg.autoreply = false;
        self.cfg.relay = false;
        self.hb_on = false;
        self.hb_next_ms = None;
        self.hb_paused = false;
        self.stop_cq_repeat();
        self.outbox.clear();
        self.typeahead = None;
        self.pending.clear();
        self.queued.clear();
        self.composer = None;
        actions.push(StationAction::IdleTripped);
        actions.push(StationAction::Toast {
            text: "Idle watchdog: TX stopped, autoreply/relay/HB off".into(),
            directed_to_me: false,
        });
    }

    fn bump_hb_schedule(&mut self, now_ms: u64) {
        // JS8Call's `resetHeartbeatTimer(false)` (mainwindow.cpp:3720). Only reschedules a
        // PERIODIC heartbeat; at interval 0 ("on demand") it is a no-op, so a call from
        // `on_event` (RX resets the HB timer) cannot cancel a pending on-demand HB. The
        // interval-0 "fire once" clear lives in `next_frame`, right after the HB is released.
        if self.hb_on && self.cfg.hb_interval_min > 0 {
            self.hb_next_ms = Some(self.hb_due_after(now_ms));
        }
    }

    /// Drop a heartbeat that has fallen due while nothing may transmit, and re-base the
    /// interval, as JS8Call does: `checkRepeat` (mainwindow.cpp:5723) sends it anyway, `startTx`
    /// finds TX off (`ensureCanTransmit`, :5295) and calls `on_stopTxButton_clicked` (:5304),
    /// which clears the queue and re-bases the heartbeat (`resetAutomaticIntervalTransmissions
    /// (false, false)`, :7397 → `resetHeartbeatTimer`, :3720). An on-demand heartbeat (interval
    /// 0) is simply dropped. "Due" is `checkRepeat`'s test, `secsTo(next) <= 0`: under a second
    /// before `next`. The engine calls this while its TX latch is down or Settings has no
    /// locator (`ensureCallsignSet`, :5309, fails the same way). True when one was dropped.
    pub fn drop_due_heartbeat(&mut self, now_ms: u64) -> bool {
        let Some(next) = self.hb_next_ms else {
            return false;
        };
        if !self.hb_on || now_ms + 1000 <= next {
            return false;
        }
        self.hb_next_ms = if self.cfg.hb_interval_min == 0 {
            None
        } else {
            Some(self.hb_due_after(now_ms))
        };
        true
    }

    /// A periodic heartbeat's next deadline: `nextTransmitCycle()` + the interval
    /// (`on_hbMacroButton_toggled`, mainwindow.cpp:6319).
    fn hb_due_after(&self, now_ms: u64) -> u64 {
        next_transmit_cycle_ms(now_ms, u64::from(self.cfg.speed.period_s()))
            + u64::from(self.cfg.hb_interval_min) * 60 * 1000
    }

    /// Re-base the CQ repeat on NOW. Deliberately `now + interval`, never
    /// `previous_deadline + interval`: a window the station slept through is SKIPPED, so a
    /// suspended laptop or a long busy period can never wake up owing four CQs.
    fn bump_cq_schedule(&mut self, now_ms: u64) {
        if self.cq_on && self.cfg.cq_interval_min > 0 {
            self.cq_next_ms = Some(now_ms + self.cfg.cq_interval_min as u64 * 60 * 1000);
        }
    }

    // ---- the single TX seam -----------------------------------------------------------------

    /// Once per period: release any pending reply whose countdown elapsed and a heartbeat due
    /// in this period, then dispense the outbox head's next frame. THE ONLY method that hands
    /// out a `TxFrame`.
    pub fn next_frame(
        &mut self,
        period_start_ms: u64,
        busy: &dyn Fn(f32) -> bool,
        rng: &mut dyn FnMut() -> u32,
    ) -> Option<TxFrame> {
        // A reply waiting for the operator's Yes is never released here: the Yes releases it
        // (`answer_reply`), and one made with the confirmation off is in the outbox already.
        // A heartbeat due in THIS period goes out in it. Its deadline is a period boundary
        // + 1 s; JS8Call's 1 Hz `checkRepeat` (mainwindow.cpp:5723) fires within a second after
        // that boundary and its late-start rule (`guiUpdate`, :4512) keys it in the same period.
        // Nexus keys at the boundary, so the boundary releases it. A SCHEDULE, never a queue:
        // only into an EMPTY outbox. A reply still asking does not hold it: JS8Call's question
        // box holds nothing back (mainwindow.cpp:5227-5229).
        //
        // First, the compose box's newest text for the operator's message going out is framed,
        // as JS8Call reads its box just before each frame (typeahead, `prepareNextMessageFrame`).
        self.take_typeahead(period_start_ms);
        if self.hb_on && self.outbox.is_empty() {
            if let Some(next) = self.hb_next_ms {
                if period_start_ms + 1000 >= next {
                    let _ = self.enqueue_heartbeat(period_start_ms);
                    if self.cfg.hb_interval_min == 0 {
                        // interval 0 = "on demand" = ONCE: clear the schedule so this does not
                        // re-enqueue every period, and the cockpit shows no stuck past
                        // `hb_next_ms`. Re-arming (`set_hb`) schedules the next on-demand HB.
                        self.hb_next_ms = None;
                    }
                }
            }
        }
        let head = self.outbox.front_mut()?;
        let (frame, i3) = head.frames.get(head.cursor)?.clone();
        let word = encode_frame(&frame, i3, self.cfg.speed).ok()?;
        // The heartbeat's offset is picked ONCE and the whole message keys there (JS8Call
        // picks it in `sendHeartbeat`, :6277): the first frame picks, the rest keep it.
        let freq_hint = match head.freq_hint {
            FreqHint::HbSubband(f) if head.cursor > 0 => FreqHint::HbSubband(f),
            FreqHint::HbSubband(_) => {
                head.freq_hint = FreqHint::HbSubband(find_free_freq_offset(busy, rng));
                head.freq_hint
            }
            other => other,
        };
        let tx = TxFrame {
            word,
            speed: self.cfg.speed,
            freq_hint,
            origin: head.origin,
            first: i3.first,
            last: i3.last,
            display: head.display.clone(),
        };
        self.last_tx_display = Some(head.display.clone());
        head.cursor += 1;
        typeahead::handed_out(&mut self.typeahead, head, &frame, period_start_ms);
        if head.cursor >= head.frames.len() {
            self.outbox.pop_front();
            // A whole message has gone out, whatever it was: JS8Call's `stopTx` calls
            // `on_stopTxButton_clicked` after the last frame of every message (:4832), and that
            // re-bases the heartbeat (`resetAutomaticIntervalTransmissions(false, false)`,
            // :7397). The next cycle after this period's start is the next one after its end.
            self.bump_hb_schedule(period_start_ms);
        }
        Some(tx)
    }

    fn compose_out(
        &self,
        origin: Origin,
        text: &str,
        freq_hint: FreqHint,
    ) -> Result<OutMsg, ComposeError> {
        let seq = frames_with_grid(&self.cfg.mycall, &self.cfg.grid, None, text, self.cfg.speed)?;
        Ok(OutMsg {
            typeahead: false,
            origin,
            display: format!("{}: {text}", self.base()),
            frames: seq,
            cursor: 0,
            freq_hint,
        })
    }

    // ---- operator verbs (each resets the idle counter) --------------------------------------

    pub fn send(
        &mut self,
        to: Option<&CallRef>,
        text: &str,
        now_ms: u64,
    ) -> Result<usize, ComposeError> {
        self.mark_active(now_ms);
        let text = self.expand_grid_macros(text);
        let seq = frames_with_grid(&self.cfg.mycall, &self.cfg.grid, to, &text, self.cfg.speed)?;
        let n = seq.len();
        let out = OutMsg {
            typeahead: false,
            origin: Origin::Operator,
            display: format!("{}: {text}", self.base()),
            frames: seq,
            cursor: 0,
            freq_hint: FreqHint::Dial,
        };
        Ok(if self.push_outbox(out) { n } else { 0 })
    }

    pub fn send_command(
        &mut self,
        to: &CallRef,
        cmd: Command,
        arg: &str,
        now_ms: u64,
    ) -> Result<usize, ComposeError> {
        self.mark_active(now_ms);
        let word = cmd.text().trim();
        let arg = self.expand_grid_macros(arg);
        let line = if arg.is_empty() {
            format!("{} {word}", to.render())
        } else {
            format!("{} {word} {arg}", to.render())
        };
        let seq = frames_with_grid(
            &self.cfg.mycall,
            &self.cfg.grid,
            None,
            &line,
            self.cfg.speed,
        )?;
        let n = seq.len();
        let out = OutMsg {
            typeahead: false,
            origin: Origin::Operator,
            display: format!("{}: {line}", self.base()),
            frames: seq,
            cursor: 0,
            freq_hint: FreqHint::Dial,
        };
        Ok(if self.push_outbox(out) { n } else { 0 })
    }

    pub fn call_cq(&mut self, now_ms: u64) -> Result<(), ComposeError> {
        self.mark_active(now_ms);
        let out = self.compose_cq(Origin::Operator, now_ms)?;
        self.push_outbox(out);
        Ok(())
    }

    /// The CQ line both the operator's click and the repeat schedule send, differing only in
    /// `origin`. Shared so a change to the wire text can never drift between them.
    ///
    /// JS8Call's `sendCQ` (mainwindow.cpp:4524-4545): the CQ message with its macros replaced and
    /// the rest pruned, or, when the message is empty, `CQ CQ CQ` and the 4-character square
    /// (:4534-4537). Its default, `CQ CQ CQ <MYGRID4>`, is the one CQ frame JS8Call sends: the
    /// square, never a longer locator, which would spill into a data frame keying on the next
    /// period. A message that is not one of the eight CQ forms and a square goes as JS8Call
    /// frames it, through `frames_with_grid`, in as many frames as it takes.
    fn compose_cq(&self, origin: Origin, now_ms: u64) -> Result<OutMsg, ComposeError> {
        let message = self.cfg.cq_text.trim().replace("CQCQCQ", "CQ CQ CQ");
        let line = if message.is_empty() {
            let grid4: String = self.cfg.grid.chars().take(4).collect();
            format!("CQ CQ CQ {grid4}")
        } else {
            self.expand_macros(&message, now_ms)
        };
        let seq = frames_with_grid(
            &self.cfg.mycall,
            &self.cfg.grid,
            None,
            line.trim(),
            self.cfg.speed,
        )?;
        Ok(OutMsg {
            typeahead: false,
            origin,
            display: format!("{}: @ALLCALL {line}", self.base()),
            frames: seq,
            cursor: 0,
            freq_hint: FreqHint::Dial,
        })
    }

    /// Enqueue ONE scheduled CQ. Deliberately does NOT `mark_active`: an automatic origin
    /// must not reset the idle-watchdog baseline, or the repeat outlives the watchdog that
    /// bounds it. `enqueue_heartbeat`'s rule, for the same reason.
    fn enqueue_cq(&mut self, now_ms: u64) -> Result<(), ComposeError> {
        let out = self.compose_cq(Origin::CqRepeat, now_ms)?;
        self.push_outbox(out);
        Ok(())
    }

    pub fn heartbeat_now(&mut self, now_ms: u64) -> Result<(), ComposeError> {
        self.mark_active(now_ms);
        self.enqueue_heartbeat(now_ms)
    }

    fn enqueue_heartbeat(&mut self, _now_ms: u64) -> Result<(), ComposeError> {
        // Never where heartbeats may not go out, whoever asks (`hb_allowed`).
        if !self.hb_allowed() {
            return Ok(());
        }
        // The 4-character square, as JS8Call sends it (`my_grid().left(4)`, mainwindow.cpp:6258):
        // the heartbeat frame carries no more, and a longer locator spills into a data frame
        // that keys on the next period.
        let grid4: String = self.cfg.grid.chars().take(4).collect();
        let line = format!("HEARTBEAT {grid4}");
        let seq = frames_with_grid(
            &self.cfg.mycall,
            &self.cfg.grid,
            None,
            line.trim(),
            self.cfg.speed,
        )?;
        let out = OutMsg {
            typeahead: false,
            origin: Origin::Heartbeat,
            display: format!("{}: @HB {line}", self.base()),
            frames: seq,
            cursor: 0,
            freq_hint: self.hb_spot(),
        };
        self.push_outbox(out);
        Ok(())
    }

    /// The HB repeat on or off. It never goes on where heartbeats may not go out (`hb_allowed`:
    /// JS8Call has no HB button there), and either way a pause the QSO pause holds is over. On,
    /// with the QSO pause on, it clears the selected call, as JS8Call's HB button does
    /// (`clearCallsignSelected`, mainwindow.cpp:4487-4490).
    pub fn set_hb(&mut self, on: bool, now_ms: u64) {
        let on = on && self.hb_allowed();
        self.hb_on = on;
        self.hb_paused = false;
        if on && self.cfg.hb_qso_pause {
            self.selected = None;
        }
        if on {
            // On demand (interval 0): the next period. Periodic: one interval after the NEXT
            // transmit cycle, so the button counts down before the first one
            // (`on_hbMacroButton_toggled`, mainwindow.cpp:6319).
            self.hb_next_ms = Some(if self.cfg.hb_interval_min == 0 {
                now_ms
            } else {
                self.hb_due_after(now_ms)
            });
        } else {
            self.hb_next_ms = None;
        }
    }

    pub fn hb_on(&self) -> bool {
        self.hb_on
    }

    pub fn hb_next_ms(&self) -> Option<u64> {
        self.hb_next_ms
    }

    /// Arm/disarm the repeating CQ (JS8Call's checkable `cqMacroButton`), which sends the CQ
    /// message (`compose_cq`). `set_hb`'s shape exactly: the first CQ fires now (interval 0 = "on
    /// demand" = once) or after the interval, and disarming clears the schedule. Arming keys
    /// NOTHING by itself — it only schedules; `plan_js8_tx` still re-reads the TX latch.
    pub fn set_cq(&mut self, on: bool, now_ms: u64) {
        self.cq_on = on;
        if on {
            self.cq_next_ms = Some(now_ms + self.cfg.cq_interval_min as u64 * 60 * 1000);
        } else {
            self.cq_next_ms = None;
        }
    }

    pub fn cq_on(&self) -> bool {
        self.cq_on
    }

    pub fn cq_next_ms(&self) -> Option<u64> {
        self.cq_next_ms
    }

    /// The call in the cockpit's To box, JS8Call's selected call (`callsignSelectedChanged`,
    /// mainwindow.cpp:5958-6000); empty text is none. With the QSO pause on, selecting one stops
    /// the HB repeat, to turn it back on when the selection clears (`m_hbPaused`), and stops the
    /// repeating CQ for good; while one is selected no heartbeat is acknowledged (`maybe_hb_ack`).
    /// Clearing it turns a paused repeat back on (`set_hb`), its first heartbeat one interval
    /// after the next cycle, as JS8Call's loop starts (TxLoop.cpp:193-209). A repeat at interval 0
    /// is a single heartbeat JS8Call never pauses.
    pub fn select_call(&mut self, call: Option<&str>, now_ms: u64) {
        let call = call
            .map(|c| clamp_str(c.trim(), MAX_CALL_LEN).to_ascii_uppercase())
            .filter(|c| !c.is_empty());
        if call == self.selected {
            return;
        }
        self.selected = call;
        if self.selected.is_some() {
            if self.cfg.hb_qso_pause {
                if self.hb_on && self.cfg.hb_interval_min > 0 {
                    self.hb_on = false;
                    self.hb_next_ms = None;
                    self.hb_paused = true;
                }
                self.stop_cq_repeat();
            }
        } else if self.hb_paused {
            self.set_hb(true, now_ms);
        }
    }

    /// The QSO pause is holding the HB repeat off until the To box is cleared.
    pub fn hb_paused(&self) -> bool {
        self.hb_paused
    }

    /// The stations the 55-minute rule put on the "never acknowledge" list and that are still on
    /// it, oldest first.
    pub fn hb_rate_blocked(&self) -> &[String] {
        &self.hb_rate_blocked
    }

    /// Keep in the rule's record only the stations still on the "never acknowledge" list, the
    /// newest `MAX_HB_RATE`: one the operator took off the list is the rule's no more.
    fn keep_listed_rate_blocks(&mut self) {
        let deny = &self.cfg.hb_ack_deny;
        self.hb_rate_blocked.retain(|c| deny.iter().any(|d| d == c));
        let over = self.hb_rate_blocked.len().saturating_sub(MAX_HB_RATE);
        self.hb_rate_blocked.drain(..over);
    }

    /// Why a heartbeat heard now would draw no acknowledgement, the HB ACK switches being on: the
    /// first of `maybe_hb_ack`'s own checks that holds it back, in its order.
    pub fn hb_ack_suppressed(&self) -> Option<HbAckSuppressed> {
        if !heartbeat_speed(self.cfg.speed) {
            Some(HbAckSuppressed::Speed)
        } else if self.rx_open {
            Some(HbAckSuppressed::Arriving)
        } else if self.cfg.hb_qso_pause && self.selected.is_some() {
            Some(HbAckSuppressed::QsoPause)
        } else {
            None
        }
    }

    /// JS8Call's `resetCQTimer(stop = true)` (mainwindow.cpp:3711): traffic addressed to us
    /// STOPS the repeating CQ outright — somebody answered, so stop calling. (It only pushes
    /// the heartbeat timer out; the heartbeat is a beacon, the CQ is a call.)
    fn stop_cq_repeat(&mut self) {
        self.cq_on = false;
        self.cq_next_ms = None;
    }

    /// The operator's answer to the reply shown as (`display`, `fires_at_ms`), JS8Call's Yes or
    /// No (mainwindow.cpp:5219-5226). Yes releases it to the outbox, where the next period keys
    /// it, and hands over the held message a delivery carries; No drops it, so a No is never
    /// answered by a frame. Either is the operator's act: it restarts the idle
    /// count and clears an idle trip, as any click in JS8Call does (`eventFilter`,
    /// mainwindow.cpp:2979-2988). `None` when no such reply waits any more.
    pub fn answer_reply(
        &mut self,
        display: &str,
        fires_at_ms: u64,
        yes: bool,
        now_ms: u64,
    ) -> Option<Vec<StationAction>> {
        let i = self
            .pending
            .iter()
            .position(|p| p.display == display && p.fires_at_ms == fires_at_ms)?;
        self.mark_active(now_ms);
        let mut actions = Vec::new();
        let p = self.pending.remove(i);
        if yes {
            self.release(p.origin, &p.text, p.freq_hint, p.delivers, &mut actions);
            actions.extend(self.process_tx_queue());
        }
        Some(actions)
    }

    pub fn drop_queue(&mut self) {
        self.outbox.clear();
        self.typeahead = None;
        self.pending.clear();
        self.queued.clear();
        self.composer = None;
    }

    /// Queue the APRS-IS relay's message (JS8Call 3.0.3's `AprsInboundRelay` →
    /// `enqueueMessage(PriorityHigh, …)`, UI_Constructor.cpp:153-155): with no confirmation box,
    /// on my own offset (`offset == -1` is `freq()`), ahead of every automatic reply waiting —
    /// JS8Call's queue is ordered by priority and this one is High — and behind any relay already
    /// there. `process_tx_queue` keys it whatever AUTO says, as JS8Call's `processTxQueue` keys a
    /// High message (mainwindow.cpp:6650-6660). A full queue refuses it: returns false.
    pub fn enqueue_aprs_relay(&mut self, text: &str) -> bool {
        if self.queued.len() >= MAX_PENDING {
            return false;
        }
        let at = self
            .queued
            .iter()
            .position(|q| q.origin != Origin::AprsRelay)
            .unwrap_or(self.queued.len());
        self.queued.insert(
            at,
            Queued {
                origin: Origin::AprsRelay,
                text: text.to_string(),
                freq_hint: FreqHint::Dial,
                delivers: None,
            },
        );
        true
    }

    /// The outbox alone: what waits to be keyed, the rest of a message already going out
    /// included. A reply still asking and every schedule stay as they are. Returns how many
    /// messages it held and whether one of them was the operator's own.
    pub fn drop_outbox(&mut self) -> (usize, bool) {
        let operator = self.outbox.iter().any(|o| o.origin == Origin::Operator);
        let held = self.outbox.len();
        self.outbox.clear();
        self.typeahead = None;
        (held, operator)
    }

    /// Whether more frames of the message on the air follow the one handed out last: the outbox
    /// head is part way through a message (JS8Call's `m_txFrameCount > 0` once a frame has gone,
    /// mainwindow.cpp:2919-2932). Never across two messages: a message's last frame pops it, so a
    /// head with a frame handed out still has frames to go. The compose box's typeahead is the one
    /// way on past a last frame: text typed after it goes straight after it.
    pub fn frames_follow(&self) -> bool {
        self.outbox.front().is_some_and(|o| o.cursor > 0) || self.typeahead_follows()
    }

    pub fn note_tx_done(&mut self, f: &TxFrame, now_ms: u64) {
        // Display state, updated for EVERY origin.
        self.last_tx_display = Some(f.display.clone());
        // My last message, for AGN?: each frame as a receiver decodes it, appended as it keys,
        // a message's first frame starting it afresh (JS8Call's `m_totalTxMessage`, reset after
        // every message and appended in `prepareNextMessageFrame`, mainwindow.cpp:3510-3520,
        // :3652-3656). A message stopped part way stays as far as it went.
        if f.first {
            self.last_tx_message.clear();
        }
        if let Ok((frame, _)) = crate::proto::frame::decode_word(&f.word, f.speed) {
            self.last_tx_message.push_str(&frame.render());
            self.last_tx_message = clamp_str(&self.last_tx_message, MAX_LAST_TX_LEN);
        }
        // Idle-watchdog baseline: only OPERATOR TX resets it. JS8Call's `resetIdleTimer()`
        // fires on UI key/mouse activity alone (mainwindow.cpp:2987), never on any TX — so
        // its own heartbeats do NOT hold its idle watchdog off, and an idle station stops
        // beaconing after `watchdog` minutes. If automatic origins (heartbeat/autoreply/
        // relay) reset the baseline here, an unattended station beacons forever with no
        // bound. Autoreply/relay stay bounded by the 6-minute wall clock (which automatic TX
        // does not reset either — `js8_operator_verbs_restart_the_wall_clock_and_automatic_
        // replies_do_not`). Operator TX resetting the baseline is the engine's proxy for the
        // UI activity JS8Call watches — the same as upstream in the unattended case, never
        // more lenient.
        if f.origin == Origin::Operator {
            self.mark_active(now_ms);
        }
    }

    /// Stop TX / set_tier / set_mode: drop the outbox, pending replies and the HB schedule.
    pub fn halt(&mut self) {
        self.outbox.clear();
        self.typeahead = None;
        self.pending.clear();
        self.queued.clear();
        self.composer = None;
        self.hb_on = false;
        self.hb_next_ms = None;
        self.hb_paused = false;
        self.stop_cq_repeat();
    }

    /// Reset the idle-watchdog baseline to `now_ms` (and clear any standing trip): the only way
    /// a trip ends, so the count restarts whenever it does (a trip cleared with the count left
    /// running came back on the next tick). Called internally by every operator send, and
    /// PUBLICLY by the engine for every operator act and when the operator
    /// ENTERS the tier: a freshly built `Station` has `last_activity_ms == 0`, so without a
    /// baseline the first `tick` at a real wall clock would read the station as decades idle
    /// and trip the watchdog on the operator's first decode. Entering the view is the
    /// session start; the idle clock counts from there.
    pub fn mark_active(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
        if self.idle_tripped {
            self.idle_tripped = false; // an operator verb clears the trip
        }
    }

    // ---- read side --------------------------------------------------------------------------

    pub fn heard(&self) -> &[Heard] {
        &self.heard
    }

    /// Move every heard station's offset down by `delta_hz`: the dial went up by that much (a QSY
    /// that centres the offset), so each station sits that much lower in the audio, as JS8Call
    /// shifts its call activity (mainwindow.cpp:5360-5364). Display only: nothing the station
    /// sends reads a heard offset.
    pub fn shift_offsets(&mut self, delta_hz: f32) {
        for h in &mut self.heard {
            h.freq_hz -= delta_hz;
        }
    }

    pub fn inbox(&self) -> &[InboxEntry] {
        &self.inbox
    }

    pub fn inbox_mark(&mut self, id: u32, state: InboxState) -> bool {
        if let Some(e) = self.inbox.iter_mut().find(|e| e.id == id) {
            e.state = state;
            true
        } else {
            false
        }
    }

    pub fn inbox_delete(&mut self, id: u32) -> bool {
        let n = self.inbox.len();
        self.inbox.retain(|e| e.id != id);
        self.inbox.len() != n
    }

    /// JS8Call's "Store Message…" (UI_Constructor.cpp:1170-1191): the operator leaves `text` at
    /// this station for `to`, held as a message from my call (`addCommandToStorage("STORE", …)`
    /// with `d.from = my_callsign()`, `d.to` the call as selected), and handed over as any held
    /// message is, when `to` asks with QUERY MSGS and then QUERY MSG n. Keys nothing. The text is
    /// what JS8Call's box lets through, printable ASCII upper-cased (MessageReplyDialog.cpp:
    /// 47-56), on one line. Refused: no callsign of mine, no station (JS8Call greys the entry for
    /// none and for @ALLCALL or @HB, :1291-1292), no text, or a delivery that would not fit one
    /// transmission at this speed — it could never go. The actions say the inbox changed.
    pub fn store_message(
        &mut self,
        to: &str,
        text: &str,
        now_ms: u64,
    ) -> Result<Vec<StationAction>, StoreRefusal> {
        let mycall = self.cfg.mycall.trim().to_ascii_uppercase();
        if mycall.is_empty() {
            return Err(StoreRefusal::NoCallsign);
        }
        let to = to.trim().to_ascii_uppercase();
        let station = is_base_call(split_portable(&to).0) || is_compound_call(&to);
        if to.is_empty() || !station || to.contains("@ALLCALL") || to.contains("@HB") {
            return Err(StoreRefusal::NotAStation);
        }
        let text: String = text
            .chars()
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .filter(|c| (' '..='~').contains(c))
            .collect::<String>()
            .to_ascii_uppercase();
        let text = clamp_str(text.trim(), MAX_TEXT_LEN);
        if text.is_empty() {
            return Err(StoreRefusal::Empty);
        }
        let delivery = format!("{to} MSG {text} FROM {mycall}");
        match frames_with_grid(&mycall, &self.cfg.grid, None, &delivery, self.cfg.speed) {
            Ok(_) => {}
            Err(ComposeError::TooLong { frames, max }) => {
                return Err(StoreRefusal::TooLong { frames, max });
            }
            Err(_) => return Err(StoreRefusal::NoCallsign),
        }
        let id = self.next_inbox_id;
        self.next_inbox_id += 1;
        self.inbox.push(InboxEntry {
            id,
            from: mycall,
            to: clamp_str(&to, MAX_CALL_LEN),
            text,
            path: Vec::new(),
            state: InboxState::Store,
            at_ms: now_ms,
            freq_hz: 0.0,
            snr_db: 0,
        });
        let mut actions = vec![StationAction::InboxChanged];
        self.prune_inbox(now_ms, &mut actions);
        Ok(actions)
    }

    /// What the caps took out of the inbox, oldest first: which, and why.
    pub fn inbox_dropped(&self) -> &[InboxDrop] {
        &self.inbox_dropped
    }

    /// The held messages too long to hand over at the current speed, for the Inbox: each counted
    /// as its hand-over to the call (or group) it is held for, with the slowest speed it fits at.
    pub fn held_too_long(&self) -> Vec<HeldTooLong> {
        self.inbox
            .iter()
            .filter(|e| e.state == InboxState::Store && has_text(e))
            .filter_map(|e| {
                let (frames, max) = self.hand_over_too_long(e, &e.to, self.cfg.speed)?;
                let fits_at = Speed::ALL
                    .into_iter()
                    .find(|&s| self.hand_over_too_long(e, &e.to, s).is_none());
                Some(HeldTooLong {
                    id: e.id,
                    frames,
                    max,
                    fits_at,
                })
            })
            .collect()
    }

    /// The operator has read the notes: clear them. True when there were any.
    pub fn clear_inbox_dropped(&mut self) -> bool {
        let any = !self.inbox_dropped.is_empty();
        self.inbox_dropped.clear();
        any
    }

    pub fn queue(&self) -> Vec<QueuedFrame> {
        self.outbox
            .iter()
            .flat_map(|o| {
                o.frames
                    .iter()
                    .skip(o.cursor)
                    .map(move |(_, i3)| QueuedFrame {
                        origin: o.origin,
                        display: o.display.clone(),
                        first: i3.first,
                        last: i3.last,
                    })
            })
            .collect()
    }

    /// Cost of the display projection, before cloning any queued frame text.
    /// One outbox message can have many remaining frames; the message-count cap
    /// alone is not a bound on the UI queue. Reads never advance a cursor.
    pub fn display_queue_budget(&self, max_rows: usize, max_text: usize) -> Option<usize> {
        let mut rows = 0_usize;
        let mut bytes = 0_usize;
        for o in &self.outbox {
            let count = o.frames.len().saturating_sub(o.cursor);
            rows = rows.checked_add(count)?;
            if rows > max_rows || o.display.len() > max_text {
                return None;
            }
            bytes = bytes.checked_add(count.checked_mul(o.display.len())?)?;
        }
        for p in &self.pending {
            if p.to.len() > max_text || p.display.len() > max_text {
                return None;
            }
            bytes = bytes.checked_add(p.to.len() + p.display.len())?;
        }
        Some(bytes)
    }

    pub fn pending_reply(&self) -> Option<PendingReply> {
        self.pending
            .iter()
            .min_by_key(|p| p.fires_at_ms)
            .map(|p| PendingReply {
                origin: p.origin,
                to: p.to.clone(),
                display: p.display.clone(),
                fires_at_ms: p.fires_at_ms,
            })
    }

    /// Whole minutes since the operator last acted: every operator act and entering the tier
    /// (`mark_active`), the baseline the idle watchdog reads. It is the count JS8Call's
    /// once-a-minute `incrementIdleTimer` keeps and its UI activity resets (mainwindow.cpp:
    /// 10969-10979). A station never marked active has no baseline and reads 0, as JS8Call's
    /// count starts at 0.
    pub fn idle_minutes(&self, now_ms: u64) -> u16 {
        if self.last_activity_ms == 0 {
            return 0;
        }
        let minutes = now_ms.saturating_sub(self.last_activity_ms) / 60_000;
        u16::try_from(minutes).unwrap_or(u16::MAX)
    }

    pub fn idle_tripped(&self) -> bool {
        self.idle_tripped
    }

    /// Would the station act on each automatic origin now, its switch being on (the switches are
    /// the engine's to say, as the cockpit shows them)? `tx_on` is the engine's TX latch, which
    /// the station does not hold. Read by the cockpit's chips, so a chip reads armed only when
    /// the rules that decide each origin here would let it go out. In order:
    /// - the idle trip stops every origin: `on_event` acts on nothing (`replies`) and the engine
    ///   withholds whatever would key;
    /// - then the latch: with it down nothing keys, and what falls due is dropped;
    /// - an automatic reply needs nothing more;
    /// - a relay keys only with AUTO on: `process_tx_queue` otherwise puts it in the compose box
    ///   (mainwindow.cpp:9674-9685);
    /// - an HB ACK needs HB and AUTO on (`maybe_hb_ack`, mainwindow.cpp:9061);
    /// - a heartbeat or a repeating CQ needs something scheduled (`next_frame`, `tick`).
    pub fn readiness(&self, tx_on: bool) -> Readiness {
        let every = |why| Readiness {
            autoreply: Some(why),
            relay: Some(why),
            hb_ack: Some(why),
            hb: Some(why),
            cq: Some(why),
        };
        if self.idle_tripped {
            return every(Unarmed::Idle);
        }
        if !tx_on {
            return every(Unarmed::TxOff);
        }
        let auto = self.cfg.autoreply;
        Readiness {
            autoreply: None,
            relay: (!auto).then_some(Unarmed::NeedsAuto),
            hb_ack: match (self.cfg.hb_networking, auto) {
                (true, true) => None,
                (false, true) => Some(Unarmed::NeedsHb),
                (true, false) => Some(Unarmed::NeedsAuto),
                (false, false) => Some(Unarmed::NeedsHbAndAuto),
            },
            hb: self.hb_next_ms.is_none().then_some(Unarmed::Unscheduled),
            cq: self.cq_next_ms.is_none().then_some(Unarmed::Unscheduled),
        }
    }

    pub fn last_tx_display(&self) -> Option<&str> {
        self.last_tx_display.as_deref()
    }

    /// The journal's copy of the station, taken at `now_ms`. JS8Call saves its call activity
    /// without the calls its callsign aging has passed (`writeSettings`, mainwindow.cpp:2013-2023,
    /// run as it closes, :3059), and so does this. Nexus writes the journal when the inbox
    /// changes rather than as it closes, so a call that ages after the last write is restored,
    /// still aged.
    pub fn snapshot(&self, now_ms: u64) -> StationSnapshot {
        StationSnapshot {
            inbox: self.inbox.clone(),
            heard: self
                .heard
                .iter()
                .filter(|h| !self.aged(h, now_ms))
                .cloned()
                .collect(),
            allcall_replied: self
                .allcall_replied
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
            next_inbox_id: self.next_inbox_id,
            hb_rate_blocked: self.hb_rate_blocked.clone(),
            hb_rate_overflow: self.hb_rate_overflow.clone(),
            group_recipients: self.group_recipients.clone(),
            inbox_dropped: self.inbox_dropped.clone(),
        }
    }

    pub fn restore(&mut self, s: StationSnapshot, _now_ms: u64) {
        self.inbox = s.inbox;
        self.heard = s.heard;
        self.allcall_replied = s.allcall_replied.into_iter().collect();
        self.next_inbox_id = s.next_inbox_id.max(1);
        // A journal is read as data: the newest calls the cap holds, and of those only the ones
        // still on the "never acknowledge" list the Settings hand over.
        let mut blocked = s.hb_rate_blocked;
        blocked
            .iter_mut()
            .for_each(|c| *c = clamp_str(c, MAX_CALL_LEN));
        self.hb_rate_blocked = blocked;
        self.keep_listed_rate_blocks();
        let mut overflow = s.hb_rate_overflow;
        overflow
            .iter_mut()
            .for_each(|c| *c = clamp_str(c, MAX_CALL_LEN));
        let over = overflow.len().saturating_sub(MAX_HB_RATE);
        overflow.drain(..over);
        self.hb_rate_overflow = overflow;
        self.group_recipients = s.group_recipients;
        self.inbox_dropped = s.inbox_dropped;
    }
}

/// The first callsign in `text`, as `Varicode::parseCallsigns` finds callsigns (varicode.cpp:
/// 518-538): a run of callsign characters that is a call (a base call, `/P` or not, or a compound
/// call) and holds no locator (`grid_pattern`, matched anywhere in it, :531-534).
fn first_callsign(text: &str) -> Option<String> {
    let holds_locator = |t: &str| {
        t.as_bytes().windows(4).any(|w| {
            w[0].is_ascii_uppercase()
                && w[0] <= b'X'
                && w[1].is_ascii_uppercase()
                && w[1] <= b'X'
                && w[2].is_ascii_digit()
                && w[3].is_ascii_digit()
        })
    };
    text.to_ascii_uppercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '/' || c == '@'))
        .find(|t| {
            !t.is_empty()
                && (is_base_call(split_portable(t).0) || is_compound_call(t))
                && !holds_locator(t)
        })
        .map(str::to_string)
}

/// JS8Call's `since()` (mainwindow.h:768-781) for an age in whole seconds, upper-cased as its
/// compose box upper-cases every reply (TransmitTextEdit.cpp:182): days, hours, minutes, then
/// seconds in steps of 15, and "NOW" under 15 s.
fn since(secs: u64) -> String {
    if secs >= 86_400 {
        format!("{}D", secs / 86_400)
    } else if secs >= 3_600 {
        format!("{}H", secs / 3_600)
    } else if secs >= 60 {
        format!("{}M", secs / 60)
    } else if secs >= 15 {
        format!("{}S", secs - secs % 15)
    } else {
        "NOW".to_string()
    }
}

/// What JS8Call's API reads of the sending side (`tempo-app`'s `engine/js8_api.rs`). Reads only.
impl Station {
    /// The message going out, or next to go, as the queue shows it.
    pub fn going_out(&self) -> Option<&str> {
        self.outbox.front().map(|o| o.display.as_str())
    }

    /// Messages composed and waiting to key, the one going out first.
    pub fn outbox_count(&self) -> usize {
        self.outbox.len()
    }

    /// Replies made and waiting for the compose box: JS8Call's `m_txMessageQueue`.
    pub fn queued_len(&self) -> usize {
        self.queued.len()
    }

    /// JS8Call's selected call (`callsignSelected`): the cockpit's To box, as
    /// [`Station::select_call`] holds it.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
}

/// JS8Call's `<MYIDLE>` for an idle count: `since()` of the last activity (mainwindow.cpp:
/// 121-130), upper-cased, with "NOW" read as "0M" (:7018-7019). The count is whole minutes, so
/// `since()`'s seconds branch never applies.
fn idle_since(minutes: u16) -> String {
    let secs = u64::from(minutes) * 60;
    if secs >= 86_400 {
        format!("{}D", secs / 86_400)
    } else if secs >= 3_600 {
        format!("{}H", secs / 3_600)
    } else if secs >= 60 {
        format!("{}M", secs / 60)
    } else {
        "0M".to_string()
    }
}

/// Every non-overlapping `token` in `text`, found left to right whatever its ASCII case, replaced
/// by `value` (`QString::replace`, as JS8Call's `replaceMacros` calls it). ASCII case-folding moves
/// no byte, so an offset found in the folded copy is an offset in `text`.
fn replace_ignoring_ascii_case(text: &str, token: &str, value: &str) -> String {
    let folded = text.to_ascii_uppercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in folded.match_indices(token) {
        out.push_str(&text[last..at]);
        out.push_str(value);
        last = at + token.len();
    }
    out.push_str(&text[last..]);
    out
}

/// JS8Call's `findFreeFreqOffset(500, 1000, 50)` (mainwindow.cpp:5592), the heartbeat's call,
/// draw for draw: ten tries at a random one of `(1000 − 500) / 50 = 10` slots (500..=950, so
/// 1000 Hz is never drawn), then ten tries at a random whole hertz in 500..=999, then 500
/// (upstream never blocks). `busy` is `!isFreqOffsetFree(f, 50)`.
fn find_free_freq_offset(busy: &dyn Fn(f32) -> bool, rng: &mut dyn FnMut() -> u32) -> f32 {
    const FMIN: u32 = 500;
    const FMAX: u32 = 1000;
    const BW: u32 = 50;
    let nslots = (FMAX - FMIN) / BW;
    for _ in 0..nslots {
        let f = (FMIN + BW * (rng() % nslots)) as f32;
        if !busy(f) {
            return f;
        }
    }
    for _ in 0..nslots {
        let f = (FMIN + rng() % (FMAX - FMIN)) as f32;
        if !busy(f) {
            return f;
        }
    }
    FMIN as f32
}

/// JS8Call's `nextTransmitCycle()` (mainwindow.cpp:3690): drop the milliseconds, round UP to the
/// next period boundary (`roundUp`, :169, moves on even from an exact boundary), add one second.
fn next_transmit_cycle_ms(now_ms: u64, period_s: u64) -> u64 {
    let secs = now_ms / 1000;
    (secs / period_s * period_s + period_s + 1) * 1000
}

/// Parse the `n` from a `QUERY MSG n` body.
/// The operator's own mail (a MSG to me, filed `Unread`, and what they have `Read`), as against
/// mail held for another station (`Store`, `Delivered`).
fn is_my_mail(state: InboxState) -> bool {
    matches!(state, InboxState::Unread | InboxState::Read)
}

/// JS8Call's `parseRelayPathCallsigns` (mainwindow.cpp:9568-9579): the sender, then every call
/// named after a space-separated `*DE*` or `VIA` in the text, the last one named first. The call
/// is the token's run of callsign characters, kept when it has the pattern's shape: an optional
/// prefix of one to four characters and `/`, the base (one or two characters, a digit, up to
/// three letters) and an optional `/` suffix of one to four characters.
fn relay_path(from: &str, text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut calls: Vec<String> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        // `\s(*DE*|VIA)\s`: a marker with a word before it and one after it.
        if i == 0 || !(*w == "*DE*" || *w == "VIA") {
            continue;
        }
        let Some(next) = words.get(i + 1) else {
            continue;
        };
        let call: String = next
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '/')
            .collect();
        if relay_call_shape(&call) {
            calls.insert(0, call);
        }
    }
    calls.insert(0, from.to_string());
    calls
}

/// The callsign shape `parseRelayPathCallsigns` matches (mainwindow.cpp:9570):
/// `(prefix/)?base(/suffix)?`, the base being `([0-9A-Z])?([0-9A-Z])([0-9])([A-Z]){0,3}`.
fn relay_call_shape(call: &str) -> bool {
    let alnum = |s: &str, max: usize| {
        !s.is_empty()
            && s.len() <= max
            && s.bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    };
    let base = |s: &str| {
        let b = s.as_bytes();
        (1..=2).any(|lead| {
            b.len() > lead
                && b[..lead]
                    .iter()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && b[lead].is_ascii_digit()
                && b.len() - lead - 1 <= 3
                && b[lead + 1..].iter().all(|c| c.is_ascii_uppercase())
        })
    };
    let parts: Vec<&str> = call.split('/').collect();
    match parts.as_slice() {
        [b] => base(b),
        [a, b] => (alnum(a, 4) && base(b)) || (base(a) && alnum(b, 4)),
        [p, b, x] => alnum(p, 4) && base(b) && alnum(x, 4),
        _ => false,
    }
}

/// The `n` of a QUERY reading "MSG n". Only "MSG": JS8Call takes the first word of the text and
/// answers only when it is `MSG` (processCommandActivity.cpp:875-891), so "MSGS 5" asks for
/// nothing.
fn query_msg_id(m: &Message) -> Option<u32> {
    let rest = m.text.trim().strip_prefix("MSG ")?;
    rest.split_whitespace().next()?.parse().ok()
}

/// The hand-over of held message `e` to `who`, `<who> MSG <text> FROM <from>` (the reply a QUERY
/// MSG n draws, processCommandActivity.cpp:973-978). Every reply is cut to `MAX_TEXT_LEN`
/// (`schedule`), and a held text may be that long itself, so it is the held text that is cut, to
/// the room the reply leaves it: the reply always ends `FROM <from>`, at every speed.
fn hand_over_text(e: &InboxEntry, who: &str) -> String {
    let head = format!("{who} MSG ");
    let tail = format!(" FROM {}", e.from);
    let room = MAX_TEXT_LEN.saturating_sub(head.len() + tail.len());
    format!("{head}{}{tail}", clamp_str(&e.text, room))
}

#[cfg(test)]
impl Station {
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }
    pub(crate) fn outbox_len(&self) -> usize {
        self.outbox.len()
    }
    pub(crate) fn heard_len(&self) -> usize {
        self.heard.len()
    }
    pub(crate) fn allcall_len(&self) -> usize {
        self.allcall_replied.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::reassembly::Checksum;

    /// A station whose replies go by themselves (the confirmation off), so a test sees each reply
    /// on the air; `asking_cfg` is JS8Call's default, where each one asks first. Heartbeat
    /// networking is on, as every heartbeat test needs it (`hb_networking`, off by default).
    fn cfg() -> StationConfig {
        StationConfig {
            mycall: "KD9TAW".into(),
            grid: "EN52".into(),
            speed: Speed::Normal,
            autoreply_confirmation: false,
            hb_networking: true,
            ..Default::default()
        }
    }

    /// JS8Call's default: every automatic reply asks Yes/No first.
    fn asking_cfg() -> StationConfig {
        StationConfig {
            autoreply_confirmation: true,
            ..cfg()
        }
    }

    fn snr_query(from: &str) -> MessageEvent {
        directed(from, "KD9TAW", Some(Command::SnrQuery), None, "", -7)
    }

    /// Whatever keys in the `n` periods from `from_ms` (one entry per frame).
    fn keyed_in(s: &mut Station, from_ms: u64, n: u64) -> Vec<String> {
        (0..n)
            .filter_map(|k| drain(s, from_ms + k * 15_000))
            .map(|f| f.display)
            .collect()
    }

    fn directed(
        from: &str,
        to: &str,
        cmd: Option<Command>,
        num: Option<i8>,
        text: &str,
        snr: i32,
    ) -> MessageEvent {
        MessageEvent::Message(Message {
            from: from.into(),
            to: CallRef::parse(to),
            to_text: to.into(),
            cmd,
            num,
            text: text.into(),
            checksum: Checksum::NotRequired,
            path: vec![],
            freq_hz: 1500.0,
            snr_db: snr,
            speed: Speed::Normal,
            first_ms: 0,
            last_ms: 0,
            frames: 1,
            complete: true,
            compound_from: None,
            grid: None,
            cq: None,
        })
    }

    fn heartbeat(from: &str, snr: i32) -> MessageEvent {
        MessageEvent::Message(Message {
            from: from.into(),
            to: CallRef::parse("@HB"),
            to_text: "@HB".into(),
            cmd: None,
            num: None,
            text: String::new(),
            checksum: Checksum::NotRequired,
            path: vec![],
            freq_hz: 1500.0,
            snr_db: snr,
            speed: Speed::Normal,
            first_ms: 0,
            last_ms: 0,
            frames: 1,
            complete: true,
            compound_from: None,
            grid: Some("FN30".into()),
            cq: None,
        })
    }

    /// Drain the station: JS8Call's once-a-second `processTxQueue` (which the engine runs after
    /// every batch of decodes and every second), then next_frame at `t`; the emitted frame, if any.
    fn drain(s: &mut Station, t: u64) -> Option<TxFrame> {
        s.process_tx_queue();
        let mut rng = || 0u32;
        s.next_frame(t, &|_| false, &mut rng)
    }

    #[test]
    fn hb_ack_fires_only_when_enabled_and_never_answers_an_ack() {
        let mut c = cfg();
        c.hb_ack = true;
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        s.on_event(&heartbeat("W1AW", -5), 1000);
        // queued at once (the confirmation off), and keyed in the next period.
        let f = drain(&mut s, 1500).expect("HB-ack in the next period");
        assert_eq!(f.origin, Origin::HbAck);
        assert_eq!(
            f.freq_hint,
            FreqHint::HbSubband(500.0),
            "an HB-ack goes on a free heartbeat spot (sendHeartbeatAck, mainwindow.cpp:6299)"
        );
        assert_eq!(f.display, "KD9TAW: W1AW HEARTBEAT SNR -05");
        // An incoming HEARTBEAT SNR is never answered (no ack-of-an-ack). The heartbeat is a
        // periodic one not yet due, so nothing of our own is scheduled in the window and
        // anything drained would be a reply.
        let mut s2 = Station::new({
            let mut c = cfg();
            c.hb_ack = true;
            c.hb_interval_min = 5;
            c
        });
        s2.set_hb(true, 0);
        s2.on_event(
            &directed(
                "W1AW",
                "KD9TAW",
                Some(Command::HeartbeatSnr),
                Some(-5),
                "",
                -5,
            ),
            1000,
        );
        assert!(
            drain(&mut s2, 100000).is_none(),
            "HEARTBEAT SNR draws no reply"
        );
        // hb_ack off → nothing (the same not-yet-due periodic heartbeat).
        let mut s3 = Station::new(StationConfig {
            hb_interval_min: 5,
            ..cfg()
        });
        s3.set_hb(true, 0);
        s3.on_event(&heartbeat("W1AW", -5), 1000);
        assert!(drain(&mut s3, 100000).is_none());
    }

    /// What `readiness` says of HB ACK is what a heard heartbeat draws, HB ACK on and TX on, for
    /// heartbeat networking, the HB repeat and AUTO on or off: the ACK on the air with networking
    /// and AUTO, nothing otherwise, and each time the reason names what is missing. Our own
    /// heartbeat is periodic and not yet due, so anything drained is the ACK.
    #[test]
    fn readiness_says_what_a_heard_heartbeat_draws() {
        // The "HB" an acknowledgement needs is heartbeat networking (`actionModeJS8HB`,
        // processCommandActivity.cpp:637), not the HB repeat: networking on with the repeat off
        // still answers, as JS8Call does. Networking off, the repeat cannot be on.
        let ack = Some((Origin::HbAck, "KD9TAW: W1AW HEARTBEAT SNR -05".to_string()));
        for (networking, hb_on, autoreply, want) in [
            (true, true, true, (None, ack.clone())),
            (true, false, true, (None, ack)),
            (false, false, true, (Some(Unarmed::NeedsHb), None)),
            (true, true, false, (Some(Unarmed::NeedsAuto), None)),
            (false, false, false, (Some(Unarmed::NeedsHbAndAuto), None)),
        ] {
            let mut s = Station::new(StationConfig {
                hb_ack: true,
                autoreply,
                hb_interval_min: 5,
                hb_networking: networking,
                ..cfg()
            });
            s.set_hb(hb_on, 0);
            let ready = s.readiness(true).hb_ack;
            s.on_event(&heartbeat("W1AW", -5), 1000);
            let keyed = drain(&mut s, 15_000).map(|f| (f.origin, f.display));
            assert_eq!(
                (ready, keyed),
                want,
                "networking {networking}, HB {hb_on}, AUTO {autoreply}: what readiness says is \
                 what keys"
            );
        }
    }

    /// A relay request keys only with AUTO on; with AUTO off it is put in the compose box, and
    /// `readiness` says the relay needs AUTO.
    #[test]
    fn readiness_says_whether_a_relay_keys() {
        let relay = directed(
            "W1AW",
            "KD9TAW",
            Some(Command::Relay),
            None,
            "K1ABC HELLO WORLD",
            -5,
        );
        for autoreply in [true, false] {
            let mut s = Station::new(StationConfig { autoreply, ..cfg() });
            let ready = s.readiness(true).relay;
            s.on_event(&relay, 1000);
            let keyed = drain(&mut s, 15_000).map(|f| (f.origin, f.display));
            let boxed = s.sync_composer(false, None).1.map(|c| c.1);
            let text = "K1ABC HELLO WORLD *DE* KD9TAW";
            let want = if autoreply {
                (None, Some((Origin::Relay, format!("KD9TAW: {text}"))), None)
            } else {
                (Some(Unarmed::NeedsAuto), None, Some(text.to_string()))
            };
            assert_eq!((ready, keyed, boxed), want, "AUTO {autoreply}");
        }
    }

    /// A heartbeat or a repeating CQ at interval 0 goes once: then nothing is scheduled, and
    /// `readiness` says so, while one with an interval stays scheduled.
    #[test]
    fn readiness_says_unscheduled_once_an_on_demand_heartbeat_or_cq_has_gone() {
        for interval in [0, 5] {
            for origin in [Origin::Heartbeat, Origin::CqRepeat] {
                let mut s = Station::new(StationConfig {
                    hb_interval_min: interval,
                    cq_interval_min: interval,
                    ..cfg()
                });
                let of = |r: Readiness| match origin {
                    Origin::Heartbeat => r.hb,
                    _ => r.cq,
                };
                match origin {
                    Origin::Heartbeat => s.set_hb(true, 0),
                    _ => s.set_cq(true, 0),
                }
                let before = of(s.readiness(true));
                let mut keyed = 0;
                for k in 0..40u64 {
                    let t = k * 15_000;
                    s.tick(t);
                    keyed += usize::from(drain(&mut s, t).is_some_and(|f| f.origin == origin));
                }
                let want = (interval == 0).then_some(Unarmed::Unscheduled);
                assert_eq!(
                    (before, of(s.readiness(true))),
                    (None, want),
                    "{origin:?} at interval {interval}"
                );
                assert!(keyed > 0, "control: {origin:?} at interval {interval} went");
            }
        }
    }

    /// The idle trip comes first, then the TX latch, whatever else holds: each stops every
    /// origin, autoreply included, which otherwise needs nothing more.
    #[test]
    fn readiness_puts_the_idle_trip_then_the_latch_first() {
        let mut c = cfg();
        c.idle_watchdog_min = 5;
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        let every = |why| Readiness {
            autoreply: Some(why),
            relay: Some(why),
            hb_ack: Some(why),
            hb: Some(why),
            cq: Some(why),
        };
        assert_eq!(s.readiness(false), every(Unarmed::TxOff), "TX off");
        assert_eq!(
            s.readiness(true).autoreply,
            None,
            "control: TX on, an automatic reply needs nothing more"
        );
        s.tick(5 * 60 * 1000);
        assert!(s.idle_tripped(), "control: the watchdog tripped");
        assert_eq!(s.readiness(true), every(Unarmed::Idle), "tripped, TX on");
        assert_eq!(s.readiness(false), every(Unarmed::Idle), "tripped, TX off");
    }

    #[test]
    fn snr_query_to_me_is_answered_with_my_snr_of_the_sender() {
        let mut s = Station::new(cfg());
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::SnrQuery), None, "", -7),
            1000,
        );
        let f = drain(&mut s, 5000).expect("SNR? reply");
        assert_eq!(f.origin, Origin::AutoReply);
        assert_eq!(f.display, "KD9TAW: W1AW SNR -07");
    }

    /// The one query JS8Call answers on @ALLCALL, `QUERY MSGS` with a message waiting, holds the
    /// sender off for 15 minutes (the @ALLCALL cache: set when the reply is queued,
    /// mainwindow.cpp:9377-9379, and checked first, :8812); another station is still answered.
    #[test]
    fn allcall_replies_are_rate_limited_to_one_per_station_per_interval() {
        let mut s = Station::new(cfg());
        for call in ["K1ABC", "N0XYZ"] {
            store_for(&mut s, call, 0);
        }
        assert_eq!(
            allcall_query_msgs(&mut s, "K1ABC", 1000).len(),
            1,
            "answered once"
        );
        assert!(
            allcall_query_msgs(&mut s, "K1ABC", 100_000).is_empty(),
            "the same station inside 15 minutes is held off"
        );
        assert_eq!(
            allcall_query_msgs(&mut s, "N0XYZ", 200_000).len(),
            1,
            "a DIFFERENT station is still answered"
        );
        assert_eq!(
            allcall_query_msgs(&mut s, "K1ABC", 1000 + 15 * 60 * 1000).len(),
            1,
            "…and the first again, 15 minutes on"
        );
    }

    /// Store a `MSG TO:` for `call` at my station (W1AW leaves it, addressed to me). The ACK it
    /// draws (mainwindow.cpp:9051) is taken out of the countdown here, so a test sees only the
    /// replies it is about; `a_stored_msg_to_is_acked_as_js8call_acks_it` is the ACK's own.
    fn store_for(s: &mut Station, call: &str, t: u64) {
        let body = format!("{call} FRIDAY CONTACT");
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::MsgTo), None, &body, -5),
            t,
        );
        let before = s.pending.len() + s.queued.len() + s.outbox.len();
        s.pending.retain(|p| p.text != "W1AW ACK");
        s.queued.retain(|q| q.text != "W1AW ACK");
        s.outbox.retain(|o| o.display != "KD9TAW: W1AW ACK");
        assert!(
            s.pending.len() + s.queued.len() + s.outbox.len() + 1 == before
                || s.pending.len() == MAX_PENDING
                || s.queued.len() == MAX_PENDING,
            "the stored MSG TO: draws its ACK, unless the reply queue is full"
        );
    }

    /// `call` asks @ALLCALL `QUERY MSGS` at `t`; every message the station keys in reply (one
    /// entry per message: a frame's `display` is its whole message's, so count first frames).
    fn allcall_query_msgs(s: &mut Station, call: &str, t: u64) -> Vec<String> {
        s.on_event(
            &directed(call, "@ALLCALL", Some(Command::QueryMsgs), None, "", -5),
            t,
        );
        (0..3u64)
            .filter_map(|k| drain(s, t + 2000 + k * 15_000))
            .filter(|f| f.first)
            .map(|f| f.display)
            .collect()
    }

    /// `QUERY MSGS` on @ALLCALL is answered only when a message waits for the asker: "YES MSG ID
    /// n", and never "NO" (mainwindow.cpp:9255-9278: the NO is `!isAllCall`).
    #[test]
    fn an_allcall_query_msgs_is_answered_only_when_a_message_waits() {
        let mut s = Station::new(cfg());
        assert_eq!(
            allcall_query_msgs(&mut s, "K1ABC", 1000),
            Vec::<String>::new(),
            "nothing stored for K1ABC: no reply at all, never NO"
        );
        store_for(&mut s, "K1ABC", 100_000);
        let id = s.inbox()[0].id;
        assert_eq!(
            allcall_query_msgs(&mut s, "K1ABC", 200_000),
            vec![format!("KD9TAW: K1ABC YES MSG ID {id}")],
            "a waiting message is offered"
        );
    }

    /// Everything the station keys over six periods from `from_ms` (one entry per frame).
    fn keyed_displays(s: &mut Station, from_ms: u64) -> Vec<String> {
        (0..6u64)
            .filter_map(|k| drain(s, from_ms + k * 15_000))
            .map(|f| f.display)
            .collect()
    }

    /// JS8Call acts on no command that is not addressed to me, @ALLCALL or a group I joined
    /// (`if (!isAllCall && !toMe && !isGroupCall) continue;`, mainwindow.cpp:8601), and it never
    /// relays one addressed to @ALLCALL (`d.cmd == ">" && !isAllCall`, :8908).
    #[test]
    fn a_relay_request_not_addressed_to_me_is_not_relayed() {
        for to in ["K1ABC", "@ALLCALL"] {
            let mut s = Station::new(cfg());
            let acts = s.on_event(
                &directed("W1AW", to, Some(Command::Relay), None, "N0XYZ HELLO", -5),
                1000,
            );
            let relayed = acts
                .iter()
                .any(|a| matches!(a, StationAction::Relayed { .. }));
            assert!(
                !relayed && keyed_displays(&mut s, 2000).is_empty(),
                "a relay request to {to} is not relayed, got {acts:?}"
            );
        }
    }

    /// …nor stores a `MSG TO:` that was addressed to someone else, or to @ALLCALL
    /// (`d.cmd == " MSG TO:" && !isAllCall`, :9012).
    #[test]
    fn a_msg_to_not_addressed_to_me_is_not_stored() {
        for to in ["K1ABC", "@ALLCALL"] {
            let mut s = Station::new(cfg());
            s.on_event(
                &directed("W1AW", to, Some(Command::MsgTo), None, "N0XYZ HELLO", -5),
                1000,
            );
            assert!(
                s.inbox().is_empty(),
                "a MSG TO: sent to {to} is not stored at my station"
            );
        }
    }

    /// …nor delivers a stored message to a `QUERY MSG n` addressed to someone else, or to
    /// @ALLCALL (`d.cmd == " QUERY" && !isAllCall`, :9171).
    #[test]
    fn a_query_msg_not_addressed_to_me_is_not_delivered() {
        for to in ["K1ABC", "@ALLCALL"] {
            let mut s = Station::new(cfg());
            store_for(&mut s, "N0XYZ", 0);
            let id = s.inbox()[0].id;
            s.on_event(
                &directed(
                    "N0XYZ",
                    to,
                    Some(Command::Query),
                    None,
                    &format!("MSG {id}"),
                    -5,
                ),
                1000,
            );
            assert_eq!(
                (s.inbox()[0].state, keyed_displays(&mut s, 2000)),
                (InboxState::Store, Vec::new()),
                "a QUERY MSG {id} sent to {to} delivers nothing"
            );
        }
    }

    /// …and a `QUERY MSG n` that matches nothing waiting for the asker draws no reply at all:
    /// JS8Call's buffered QUERY skips every miss (mainwindow.cpp:9194-9233) and is never
    /// answered as a `QUERY MSGS` (no YES, no NO).
    #[test]
    fn a_query_msg_that_matches_nothing_draws_no_reply() {
        let mut s = Station::new(cfg());
        store_for(&mut s, "N0XYZ", 0);
        s.on_event(
            &directed("N0XYZ", "KD9TAW", Some(Command::Query), None, "MSG 99", -5),
            1000,
        );
        assert_eq!(
            keyed_displays(&mut s, 2000),
            Vec::<String>::new(),
            "QUERY MSG 99 (no such message) draws no reply"
        );
    }

    /// JS8Call answers no query addressed to @ALLCALL: `SNR?`, `INFO?`, `STATUS?`, `GRID?` and
    /// `HEARING?` each carry `!isAllCall` (mainwindow.cpp:8834, :8839, :8849, :8859, :8869).
    #[test]
    fn queries_to_allcall_draw_no_automatic_reply() {
        for cmd in [
            Command::SnrQuery,
            Command::InfoQuery,
            Command::StatusQuery,
            Command::GridQuery,
            Command::HearingQuery,
        ] {
            let mut s = Station::new(StationConfig {
                info: "RIG IC7300".into(),
                ..cfg()
            });
            let acts = s.on_event(&directed("W1AW", "@ALLCALL", Some(cmd), None, "", -7), 1000);
            let sent: Vec<String> = (0..4u64)
                .filter_map(|k| drain(&mut s, 5000 + k * 15_000))
                .map(|f| f.display)
                .collect();
            let pending = acts
                .iter()
                .any(|a| matches!(a, StationAction::ReplyPending { .. }));
            assert!(
                !pending && sent.is_empty(),
                "{cmd:?} to @ALLCALL draws no automatic reply, got {acts:?} / {sent:?}"
            );
        }
    }

    /// …while a query to my own call or to a group I joined is answered as before.
    #[test]
    fn a_query_to_my_call_or_a_joined_group_is_still_answered() {
        for to in ["KD9TAW", "@RAGCHEW"] {
            let mut s = Station::new(StationConfig {
                groups: vec!["@RAGCHEW".into()],
                ..cfg()
            });
            s.on_event(
                &directed("W1AW", to, Some(Command::SnrQuery), None, "", -7),
                1000,
            );
            let f = drain(&mut s, 5000).expect("the SNR reply");
            assert_eq!(f.display, "KD9TAW: W1AW SNR -07", "a query to {to}");
        }
    }

    /// Where JS8Call has nothing to say it says nothing: an empty INFO (mainwindow.cpp:8841-8843)
    /// or an empty grid (:8861-8863) draws no reply, even to a query addressed to me.
    #[test]
    fn an_empty_info_or_grid_draws_no_reply() {
        for cmd in [Command::InfoQuery, Command::GridQuery] {
            let mut s = Station::new(StationConfig {
                info: String::new(),
                grid: String::new(),
                ..cfg()
            });
            s.on_event(&directed("W1AW", "KD9TAW", Some(cmd), None, "", -7), 1000);
            assert_eq!(
                drain(&mut s, 5000).map(|f| f.display),
                None,
                "{cmd:?} with nothing set draws no reply"
            );
        }
        let mut s = Station::new(StationConfig {
            info: "RIG IC7300".into(),
            ..cfg()
        });
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::InfoQuery), None, "", -7),
            1000,
        );
        assert_eq!(
            drain(&mut s, 5000).map(|f| f.display).as_deref(),
            Some("KD9TAW: W1AW INFO RIG IC7300"),
            "control: a set INFO is answered"
        );
    }

    #[test]
    fn store_and_forward_msg_to_then_query_delivers_and_marks_delivered() {
        let mut s = Station::new(cfg());
        // W1AW stores a message for K1ABC at my station.
        s.on_event(
            &directed(
                "W1AW",
                "KD9TAW",
                Some(Command::MsgTo),
                None,
                "K1ABC FRIDAY CONTACT",
                -5,
            ),
            1000,
        );
        assert_eq!(s.inbox().len(), 1);
        assert_eq!(s.inbox()[0].state, InboxState::Store);
        assert_eq!(s.inbox()[0].to, "K1ABC");
        let id = s.inbox()[0].id;
        // pull every frame the station wants to send over the next ~20 periods (a reply spans
        // more than one frame — the drain is once-per-period, so a multi-frame reply keys across
        // periods, exactly as on the air).
        let flush = |s: &mut Station, from: u64| -> Vec<String> {
            let mut ds = Vec::new();
            let mut rng = || 0u32;
            for k in 0..20u64 {
                s.process_tx_queue();
                if let Some(f) = s.next_frame(from + k * 15000, &|_| false, &mut rng) {
                    ds.push(f.display);
                }
            }
            ds
        };
        // K1ABC asks whether I have any → YES MSG ID n.
        s.on_event(
            &directed("K1ABC", "KD9TAW", Some(Command::QueryMsgs), None, "", -5),
            2000,
        );
        assert!(flush(&mut s, 6000)
            .iter()
            .any(|d| *d == format!("KD9TAW: K1ABC YES MSG ID {id}")));
        // K1ABC requests message n → delivered, marked Delivered as the reply leaves the queue
        // within the second (JS8Call's callback, mainwindow.cpp:9687-9689).
        s.on_event(
            &directed(
                "K1ABC",
                "KD9TAW",
                Some(Command::Query),
                None,
                &format!("MSG {id}"),
                -5,
            ),
            300_000,
        );
        s.process_tx_queue();
        assert_eq!(s.inbox()[0].state, InboxState::Delivered);
        assert!(
            flush(&mut s, 400_000)
                .iter()
                .any(|d| d.contains("K1ABC MSG FRIDAY CONTACT FROM W1AW")),
            "delivery not transmitted"
        );
    }

    /// A `MSG TO:` stored for another station is acknowledged as JS8Call acknowledges it
    /// (mainwindow.cpp:9014-9051): `<from> ACK`, or `<path> ACK` along the relay path read from
    /// the text AFTER the target's call (`parseRelayPathCallsigns(d.from, text)`, :9027), which
    /// is why a marker right after the target names no hop.
    #[test]
    fn a_stored_msg_to_is_acked_as_js8call_acks_it() {
        let mut s = Station::new(cfg());
        s.on_event(
            &directed(
                "W1AW",
                "KD9TAW",
                Some(Command::MsgTo),
                None,
                "K1ABC FRIDAY CONTACT",
                -5,
            ),
            1_000,
        );
        assert_eq!(s.inbox().len(), 1, "control: the MSG TO: is stored");
        assert_eq!(waiting(&s), ["W1AW ACK"], "the store is acknowledged");
        let f = drain(&mut s, 2_000).expect("the ACK after the countdown");
        assert_eq!(
            (f.origin, f.display.as_str(), f.word),
            (
                Origin::AutoReply,
                "KD9TAW: W1AW ACK",
                ack_frame("W1AW", false)
            )
        );
        for (t, text, ack) in [
            (3_000, "K1ABC HELLO *DE* N0JDS", "OH8STN>N0JDS ACK"),
            (4_000, "K1ABC *DE* N0JDS", "OH8STN ACK"),
        ] {
            s.on_event(
                &directed("OH8STN", "KD9TAW", Some(Command::MsgTo), None, text, -5),
                t,
            );
            assert_eq!(
                waiting(&s).last().map(String::as_str),
                Some(ack),
                "{text:?}: the relay path is read after the target"
            );
        }
    }

    /// …only with relaying on: JS8Call neither stores nor acknowledges a `MSG TO:` with its relay
    /// off (`d.cmd == " MSG TO:" && !isAllCall && !m_config.relay_off()`, :9014), as Settings'
    /// Relay hint says. And its ACK, an automatic reply, keys only with autoreply on: off, it is
    /// put in the compose box (JS8Call keys it only with AUTO checked, `processTxQueue`,
    /// :9671-9685).
    #[test]
    fn a_msg_to_is_stored_only_with_relay_on_and_acked_only_with_autoreply_on() {
        let msg_to = directed(
            "W1AW",
            "KD9TAW",
            Some(Command::MsgTo),
            None,
            "K1ABC FRIDAY CONTACT",
            -5,
        );
        let mut c = cfg();
        c.relay = false;
        let mut s = Station::new(c);
        s.on_event(&msg_to, 1_000);
        assert!(s.inbox().is_empty(), "relay off: not stored");
        assert!(waiting(&s).is_empty(), "…and not acknowledged");
        let mut c = cfg();
        c.autoreply = false;
        let mut s = Station::new(c);
        s.on_event(&msg_to, 1_000);
        assert_eq!(s.inbox().len(), 1, "control: relay on, it is stored");
        s.process_tx_queue();
        assert!(
            s.queue().is_empty() && s.pending_reply().is_none(),
            "autoreply off: not acknowledged on the air"
        );
        assert_eq!(
            s.sync_composer(false, None).1.map(|c| c.1).as_deref(),
            Some("W1AW ACK"),
            "…but put in the composer"
        );
    }

    #[test]
    fn relay_retransmits_a_request_and_acks_at_the_chain_tail() {
        let mut s = Station::new(cfg());
        // A relay REQUEST to me: retransmit `rest *DE* MYCALL`.
        let a = s.on_event(
            &directed(
                "W1AW",
                "KD9TAW",
                Some(Command::Relay),
                None,
                "K1ABC HELLO WORLD",
                -5,
            ),
            1000,
        );
        assert!(a.iter().any(|x| matches!(x, StationAction::Relayed { .. })));
        let f = drain(&mut s, 6000).expect("relayed retransmit");
        assert_eq!(f.origin, Origin::Relay);
        assert_eq!(f.display, "KD9TAW: K1ABC HELLO WORLD *DE* KD9TAW");
        // A relay chain arriving at me (I am the tail) → ACK the chain.
        let mut s2 = Station::new(cfg());
        s2.on_event(
            &directed(
                "W1AW>N0XYZ",
                "KD9TAW",
                Some(Command::Relay),
                None,
                "PLS CPY",
                -5,
            ),
            1000,
        );
        let f = drain(&mut s2, 6000).expect("chain ACK");
        assert_eq!(f.display, "KD9TAW: W1AW>N0XYZ>KD9TAW ACK");
    }

    /// The repeating CQ is a SCHEDULE, never a queue that can burst: one frame per period,
    /// bounded by the interval, and a MISSED period is SKIPPED rather than batched. Jumping
    /// the clock four intervals forward must produce ONE CQ, not four — `bump_cq_schedule`
    /// sets the next fire to `now + interval`, it does not accumulate.
    #[test]
    fn a_repeating_cq_is_a_schedule_that_never_bursts() {
        let mut c = cfg();
        c.cq_interval_min = 5;
        let mut s = Station::new(c);
        s.mark_active(0);
        s.set_cq(true, 0);
        assert_eq!(
            s.cq_next_ms(),
            Some(5 * 60 * 1000),
            "first CQ one interval out"
        );
        assert!(
            drain(&mut s, 60_000).is_none(),
            "nothing before the interval"
        );
        // Four intervals pass with no tick in between (a suspended laptop, a busy loop).
        s.tick(20 * 60 * 1000);
        let mut sent = 0;
        while let Some(f) = drain(&mut s, 20 * 60 * 1000) {
            assert_eq!(f.origin, Origin::CqRepeat);
            sent += 1;
        }
        assert_eq!(
            sent, 1,
            "one CQ for the missed window, never a burst of four"
        );
        assert_eq!(
            s.cq_next_ms(),
            Some(25 * 60 * 1000),
            "rescheduled from NOW, not from the missed deadline"
        );
    }

    /// FINDING 1's sibling for the CQ repeat, and the whole reason `Origin::CqRepeat` exists:
    /// a scheduled CQ must NOT reset the idle-watchdog baseline. A station left calling CQ
    /// unattended MUST still stand down at the watchdog. Positive control: CQs really did go
    /// out, so "it stopped" is not a broken fixture.
    #[test]
    fn a_repeating_cq_does_not_reset_the_idle_watchdog_and_the_watchdog_stops_it() {
        let mut c = cfg();
        c.idle_watchdog_min = 60;
        c.cq_interval_min = 5;
        let mut s = Station::new(c);
        s.mark_active(0);
        s.set_cq(true, 0);
        let mut cqs_sent = 0;
        let mut tripped_at = None;
        for min in 1..=61u64 {
            let now = min * 60 * 1000;
            if s.tick(now)
                .iter()
                .any(|a| matches!(a, StationAction::IdleTripped))
            {
                tripped_at = Some(min);
                break;
            }
            while let Some(f) = drain(&mut s, now) {
                s.note_tx_done(&f, now);
                if f.origin == Origin::CqRepeat {
                    cqs_sent += 1;
                }
            }
        }
        assert!(
            cqs_sent >= 10,
            "control: the repeat really ran ({cqs_sent} CQs)"
        );
        assert_eq!(
            tripped_at,
            Some(60),
            "the idle watchdog still trips at 60 min"
        );
        assert!(
            !s.cq_on() && s.cq_next_ms().is_none(),
            "the trip stops the repeat"
        );
        assert!(
            drain(&mut s, 100 * 60 * 1000).is_none(),
            "and nothing is left to key afterwards"
        );
    }

    /// JS8Call `resetCQTimer(stop = true)` (mainwindow.cpp:3711, called from the directed-RX
    /// paths at 8333/8788): traffic addressed to me STOPS the repeating CQ — someone answered.
    /// The heartbeat is only PUSHED OUT by the same event, never stopped: a beacon is not a call.
    #[test]
    fn directed_traffic_to_me_stops_the_cq_repeat_but_only_defers_the_heartbeat() {
        let mut c = cfg();
        c.cq_interval_min = 5;
        c.hb_interval_min = 5;
        let mut s = Station::new(c);
        s.mark_active(0);
        s.set_cq(true, 0);
        s.set_hb(true, 0);
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::SnrQuery), None, "", -7),
            60_000,
        );
        assert!(
            !s.cq_on() && s.cq_next_ms().is_none(),
            "a reply stops the CQ repeat"
        );
        assert!(s.hb_on(), "the heartbeat is not stopped");
        assert_eq!(
            s.hb_next_ms(),
            Some(76_000 + 5 * 60 * 1000),
            "…only pushed out: the next transmit cycle + one interval"
        );
    }

    /// `halt` is TOTAL for the CQ repeat too, and arming it never touches the idle baseline
    /// on its own (only the ENGINE verb does, deliberately).
    #[test]
    fn halt_cancels_the_cq_repeat() {
        let mut c = cfg();
        c.cq_interval_min = 1;
        let mut s = Station::new(c);
        s.mark_active(0);
        s.set_cq(true, 0);
        s.tick(60_000);
        assert!(
            s.cq_on() && drain(&mut s, 60_000).is_some(),
            "precondition: it runs"
        );
        s.halt();
        assert!(!s.cq_on() && s.cq_next_ms().is_none());
        s.tick(10 * 60_000);
        assert!(drain(&mut s, 10 * 60_000).is_none(), "nothing after a halt");
    }

    #[test]
    fn idle_watchdog_trips_and_clears_the_automatic_origins() {
        let mut c = cfg();
        c.idle_watchdog_min = 5;
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        // queue an automatic reply, then let the clock run past the watchdog.
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::SnrQuery), None, "", -7),
            0,
        );
        assert_eq!(waiting(&s).len(), 1, "precondition: a reply is waiting");
        let acts = s.tick(5 * 60 * 1000);
        assert!(acts.iter().any(|a| matches!(a, StationAction::IdleTripped)));
        assert!(s.idle_tripped() && !s.config().autoreply && !s.config().relay && !s.hb_on());
        assert!(
            drain(&mut s, 10 * 60 * 1000).is_none(),
            "the outbox and pending were cleared"
        );
    }

    /// FINDING 1: automatic TX must NOT reset the idle watchdog. JS8Call resets its idle
    /// timer only on UI key/mouse activity (mainwindow.cpp:2987 `resetIdleTimer()`), never on
    /// any TX — so an unattended station's own heartbeats do not hold the watchdog off, and it
    /// stops beaconing after `watchdog` minutes. Here a periodic HB station, ticked and drained
    /// (each over `note_tx_done`'d as `plan_js8_tx` does), MUST still trip at 60 min. Positive
    /// control: heartbeats DID fire, so "it stopped" is not a broken fixture.
    #[test]
    fn heartbeats_do_not_reset_the_idle_watchdog() {
        let mut c = cfg();
        c.idle_watchdog_min = 60;
        c.hb_interval_min = 5; // a PERIODIC heartbeat
        let mut s = Station::new(c);
        s.mark_active(0); // session start at t = 0 (the engine seeds this on entry)
        s.set_hb(true, 0);
        let mut hb_sent = 0;
        let mut tripped_at = None;
        for min in 1..=61u64 {
            let now = min * 60 * 1000;
            if s.tick(now)
                .iter()
                .any(|a| matches!(a, StationAction::IdleTripped))
            {
                tripped_at = Some(min);
                break;
            }
            // The engine sends every enqueued HB and notes it done — exactly `plan_js8_tx`.
            while let Some(f) = drain(&mut s, now) {
                s.note_tx_done(&f, now);
                if f.origin == Origin::Heartbeat {
                    hb_sent += 1;
                }
            }
        }
        assert!(hb_sent > 0, "positive control: heartbeats DID fire");
        assert_eq!(
            tripped_at,
            Some(60),
            "the idle watchdog must trip at 60 min despite the heartbeats"
        );
        assert!(!s.hb_on(), "the trip stands the heartbeat down");
    }

    /// Positive control for FINDING 1's fix: an OPERATOR over STILL resets the idle baseline —
    /// the fix silences automatic origins only, it does not disable the reset entirely.
    #[test]
    fn an_operator_over_still_resets_the_idle_watchdog() {
        let mut c = cfg();
        c.idle_watchdog_min = 60;
        let mut s = Station::new(c);
        s.mark_active(0);
        let t59 = 59 * 60 * 1000;
        s.send(None, "HELLO", t59).expect("queues");
        let f = drain(&mut s, t59).expect("an operator frame");
        assert_eq!(f.origin, Origin::Operator);
        s.note_tx_done(&f, t59);
        s.tick(t59 + 59 * 60 * 1000);
        assert!(!s.idle_tripped(), "the operator over reset the idle clock");
        s.tick(t59 + 61 * 60 * 1000);
        assert!(
            s.idle_tripped(),
            "…and 60 min after the operator over it trips"
        );
    }

    /// FINDING 2: `hb_interval_min == 0` is "on demand" and must arm EXACTLY ONE heartbeat,
    /// not one per drain cycle. Ticked and drained across ten periods, exactly one HB fires and
    /// the schedule is cleared (no stuck past `hb_next_ms` for the cockpit to show).
    #[test]
    fn heartbeat_interval_zero_arms_exactly_one() {
        let mut c = cfg();
        c.hb_interval_min = 0; // on demand
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        let mut count = 0;
        for min in 0..10u64 {
            let now = min * 60 * 1000;
            s.tick(now);
            while let Some(f) = drain(&mut s, now) {
                if f.origin == Origin::Heartbeat {
                    count += 1;
                }
                s.note_tx_done(&f, now);
            }
        }
        assert_eq!(count, 1, "interval 0 = on demand = exactly one heartbeat");
        assert_eq!(
            s.hb_next_ms(),
            None,
            "the on-demand schedule is cleared after firing (no stuck timestamp)"
        );
    }

    /// The interval-0 "fire once" clear lives in `tick` after the HB fires, NOT in
    /// `bump_hb_schedule` — so received directed traffic (which resets the HB timer via
    /// `on_event`) must NOT cancel a pending on-demand heartbeat before it has a chance to fire.
    #[test]
    fn inbound_traffic_does_not_cancel_a_pending_on_demand_heartbeat() {
        let mut c = cfg();
        c.hb_interval_min = 0; // on demand
        c.autoreply = false; // keep the outbox empty for the HB
        let mut s = Station::new(c);
        s.set_hb(true, 0); // armed; hb_next_ms = Some(0), not yet fired
                           // A directed message TO ME arrives first — `on_event` calls `bump_hb_schedule`.
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::SnrQuery), None, "", -5),
            1000,
        );
        assert!(
            s.hb_next_ms().is_some(),
            "inbound traffic must not cancel the pending on-demand HB"
        );
        // …and the heartbeat still fires on the next tick.
        s.tick(2000);
        let mut fired = 0;
        while let Some(f) = drain(&mut s, 2000) {
            if f.origin == Origin::Heartbeat {
                fired += 1;
            }
            s.note_tx_done(&f, 2000);
        }
        assert_eq!(
            fired, 1,
            "the on-demand heartbeat fires despite the inbound traffic"
        );
    }

    /// FINDING 1's RX SIBLING: inbound directed traffic must NOT reset the idle-watchdog
    /// baseline. The watchdog answers "is a human still here?"; a stranger calling us is not
    /// evidence of that (JS8Call resets on UI key/mouse only, never on RX), and letting it reset
    /// the timer means a third party can hold our unattended, autoreply-armed station on the air
    /// indefinitely. An hour of inbound queries every 5 minutes MUST still trip the watchdog.
    #[test]
    fn inbound_traffic_does_not_hold_off_the_idle_watchdog() {
        let mut c = cfg();
        c.idle_watchdog_min = 60;
        c.autoreply = true; // armed to answer
        let mut s = Station::new(c);
        s.mark_active(0); // session start; only the operator (or entering) resets this
        let mut tripped_at = None;
        for min in 1..=61u64 {
            let now = min * 60 * 1000;
            if min % 5 == 0 {
                // a stranger calls us — not evidence a human is at the station
                s.on_event(
                    &directed("W1AW", "KD9TAW", Some(Command::SnrQuery), None, "", -5),
                    now,
                );
            }
            if s.tick(now)
                .iter()
                .any(|a| matches!(a, StationAction::IdleTripped))
            {
                tripped_at = Some(min);
                break;
            }
        }
        assert_eq!(
            tripped_at,
            Some(60),
            "inbound traffic must not hold off the idle watchdog"
        );
        assert!(!s.config().autoreply, "…and the trip stands autoreply down");
    }

    /// Positive control for the RX-sibling fix: an OPERATOR verb during the same inbound window
    /// DOES reset the baseline, so a station a human is actually working stays alive — the fix
    /// gates the reset on presence, it does not turn the watchdog into an unconditional timer.
    #[test]
    fn an_operator_verb_during_inbound_traffic_still_resets_the_idle_baseline() {
        let mut c = cfg();
        c.idle_watchdog_min = 60;
        c.autoreply = true;
        let mut s = Station::new(c);
        s.mark_active(0);
        let mut tripped_at = None;
        for min in 1..=61u64 {
            let now = min * 60 * 1000;
            if min % 5 == 0 {
                s.on_event(
                    &directed("W1AW", "KD9TAW", Some(Command::SnrQuery), None, "", -5),
                    now,
                );
            }
            if min == 50 {
                // a human sends — this DOES reset the idle baseline
                s.send(None, "HELLO", now).expect("operator send");
                while let Some(f) = drain(&mut s, now) {
                    s.note_tx_done(&f, now);
                }
            }
            if s.tick(now)
                .iter()
                .any(|a| matches!(a, StationAction::IdleTripped))
            {
                tripped_at = Some(min);
                break;
            }
        }
        assert_eq!(
            tripped_at, None,
            "the operator send at min 50 keeps the station alive past min 61 (would trip at 110)"
        );
    }

    /// The HB-schedule bump is KEPT on inbound traffic (JS8Call's HeartbeatQSOPause: defer a
    /// periodic heartbeat while a QSO is in progress — politeness on frequency, not presence).
    /// Gating the idle reset must NOT take the deferral with it.
    #[test]
    fn inbound_traffic_still_defers_a_periodic_heartbeat() {
        let mut c = cfg();
        c.hb_interval_min = 5; // periodic
        let mut s = Station::new(c);
        s.set_hb(true, 0); // hb_next_ms = 5 min
        let before = s.hb_next_ms().unwrap();
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::SnrQuery), None, "", -5),
            3 * 60 * 1000,
        );
        let after = s.hb_next_ms().unwrap();
        assert!(
            after > before,
            "inbound traffic defers the next periodic heartbeat"
        );
        assert_eq!(
            after,
            196_000 + 5 * 60 * 1000,
            "deferred to the next transmit cycle + interval (HeartbeatQSOPause)"
        );
    }

    /// JS8Call sends the 4-character square (`my_grid().left(4)`, mainwindow.cpp:6258), so a
    /// heartbeat is ONE frame. Given the whole 6-character locator, the heartbeat grammar
    /// stops at the square (no word boundary follows it), the locator spills into a data
    /// frame, and every heartbeat keyed on two periods in a row.
    #[test]
    fn a_heartbeat_with_a_six_character_locator_is_one_frame_carrying_the_square() {
        let mut c = cfg();
        c.grid = "EN52HW".into();
        let mut s = Station::new(c);
        s.heartbeat_now(0).unwrap();
        let keyed: Vec<(bool, bool, String)> = (0..6u64)
            .filter_map(|k| drain(&mut s, k * 15_000))
            .map(|f| (f.first, f.last, f.display))
            .collect();
        assert_eq!(
            keyed,
            vec![(true, true, "KD9TAW: @HB HEARTBEAT EN52".to_string())],
            "one heartbeat, one frame, the 4-character square"
        );
    }

    /// JS8Call picks the heartbeat's offset ONCE, in `sendHeartbeat` (:6277), and the whole
    /// message goes out there (`processTxQueue` → `setFreqOffsetForRestore`, :9681). A
    /// heartbeat with a proper square is one frame, but a locator that is not one still spills
    /// into a data frame (in JS8Call too), and that frame must key where the first did or no
    /// receiver can join the two.
    #[test]
    fn a_heartbeat_message_keys_every_frame_at_one_offset() {
        let mut c = cfg();
        c.grid = "XX".into(); // not a square: the heartbeat grammar leaves it to a data frame
        let mut s = Station::new(c);
        s.heartbeat_now(0).unwrap();
        let mut n = 0u32;
        let mut rng = move || {
            n += 1;
            n - 1
        };
        let first = s.next_frame(0, &|_| false, &mut rng).expect("frame 1");
        let second = s.next_frame(15_000, &|_| false, &mut rng).expect("frame 2");
        assert!(
            first.first && !first.last && second.last,
            "precondition: a two-frame heartbeat"
        );
        assert_eq!(second.freq_hint, first.freq_hint, "one message, one offset");
    }

    /// The heartbeat's offset is JS8Call's `findFreeFreqOffset(500, 1000, 50)` (:5592), draw
    /// for draw: ten tries at one of `(1000 − 500) / 50 = 10` slots (500..=950, so 1000 Hz is
    /// never drawn), then ten tries at any whole hertz in 500..=999, then 500.
    #[test]
    fn the_heartbeat_offset_is_js8calls_find_free_freq_offset() {
        fn pick(busy: &dyn Fn(f32) -> bool, draw: u32) -> f32 {
            let mut s = Station::new(cfg());
            s.heartbeat_now(0).unwrap();
            let mut rng = move || draw;
            match s
                .next_frame(0, busy, &mut rng)
                .expect("the heartbeat")
                .freq_hint
            {
                FreqHint::HbSubband(f) => f,
                other => panic!("a heartbeat rides the sub-band hint, got {other:?}"),
            }
        }
        let free = |_: f32| false;
        let slots: Vec<f32> = (0..20).map(|k| pick(&free, k)).collect();
        let want: Vec<f32> = (0..20).map(|k| 500.0 + 50.0 * (k % 10) as f32).collect();
        assert_eq!(slots, want, "ten slots, 500..=950; 1000 Hz is never drawn");
        let on_a_slot = |f: f32| f % 50.0 == 0.0;
        assert_eq!(
            pick(&on_a_slot, 123),
            623.0,
            "every slot busy: the second pass takes 500 + draw % 500"
        );
        assert_eq!(pick(&|_| true, 7), 500.0, "nothing free anywhere: 500");
    }

    /// The schedule is JS8Call's arithmetic, not "now + interval". `nextTransmitCycle()`
    /// (:3690) drops the milliseconds, rounds UP to the next period boundary (`roundUp`, :169,
    /// moves on even from an exact boundary) and adds one second; the interval goes on top.
    /// Arming (`on_hbMacroButton_toggled`, :6319) and directed traffic to me
    /// (`resetAutomaticIntervalTransmissions(true, false)`, :8333 → `resetHeartbeatTimer`,
    /// :3720) both land there. Normal: 15 s periods.
    #[test]
    fn the_heartbeat_schedule_is_the_next_transmit_cycle_plus_the_interval() {
        let mut c = cfg();
        c.hb_interval_min = 5;
        let mut s = Station::new(c);
        s.set_hb(true, 67_250); // 7.25 s into the period that began at 60 s
        assert_eq!(
            s.hb_next_ms(),
            Some(76_000 + 300_000),
            "armed mid-period: the 75 s boundary + 1 s + 5 min"
        );
        s.set_hb(true, 75_000);
        assert_eq!(
            s.hb_next_ms(),
            Some(91_000 + 300_000),
            "armed ON a boundary: roundUp still moves to the next one"
        );
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::SnrQuery), None, "", -5),
            100_400,
        );
        assert_eq!(
            s.hb_next_ms(),
            Some(106_000 + 300_000),
            "directed traffic re-bases on the same rule"
        );
    }

    /// ONE heartbeat per interval, in the period JS8Call keys it. JS8Call's deadline is always
    /// a period boundary + 1 s. Its 1 Hz `checkRepeat` (:5723) fires in the first second after
    /// that boundary (`secsTo` truncates) and the late-start rule (`guiUpdate`, :4512) keys it
    /// in the SAME period. The end of each over then re-bases the next heartbeat to the
    /// following cycle + the interval (`stopTx` → `on_stopTxButton_clicked`, :4832 → :7397), so
    /// they go out an interval and a period apart, and never on consecutive periods.
    #[test]
    fn a_periodic_heartbeat_keys_once_per_interval_in_the_period_js8call_keys_it() {
        let mut c = cfg();
        c.hb_interval_min = 5;
        let mut s = Station::new(c);
        s.mark_active(0);
        s.set_hb(true, 67_250); // due at 376 s
        let mut keyed = Vec::new();
        for slot in 0..120u64 {
            // half an hour: one plan per 15 s period, the engine's 1 Hz tick in between
            let start = slot * 15_000;
            if let Some(f) = drain(&mut s, start) {
                assert_eq!(f.origin, Origin::Heartbeat, "only heartbeats are queued");
                s.note_tx_done(&f, start);
                keyed.push(start);
            }
            for sec in 0..15 {
                s.tick(start + sec * 1000);
            }
        }
        assert_eq!(
            keyed,
            vec![375_000, 690_000, 1_005_000, 1_320_000, 1_635_000],
            "one interval after the next cycle, then every interval + one period"
        );
    }

    /// …and the end of ANY message re-bases the heartbeat, not only its own: JS8Call's
    /// `stopTx` calls `on_stopTxButton_clicked` after the last frame of every message
    /// (:4832), which calls `resetHeartbeatTimer(false)` (:7397 → :3720).
    #[test]
    fn the_end_of_any_message_re_bases_the_periodic_heartbeat() {
        let mut c = cfg();
        c.hb_interval_min = 5;
        let mut s = Station::new(c);
        s.set_hb(true, 67_250); // due at 376 s
        let w1aw = CallRef::Base("W1AW".into());
        s.send_command(&w1aw, Command::SnrQuery, "", 120_000)
            .unwrap();
        let f = drain(&mut s, 120_000).expect("the operator's message");
        assert!(
            f.first && f.last,
            "precondition: one frame, so that was its end"
        );
        assert_eq!(
            s.hb_next_ms(),
            Some(136_000 + 300_000),
            "the next cycle after it, plus the interval"
        );
    }

    /// Every frame the station keys over six periods from `from_ms`, as it reads on the air.
    fn keyed_wire(s: &mut Station, from_ms: u64) -> Vec<(Frame, bool, bool)> {
        (0..6u64)
            .filter_map(|k| drain(s, from_ms + k * 15_000))
            .map(|f| {
                let (frame, i3) = crate::proto::frame::decode_word(&f.word, f.speed)
                    .expect("what the station keys decodes");
                (frame, i3.first, i3.last)
            })
            .collect()
    }

    /// One `CQ CQ CQ <grid>` frame from KD9TAW, first and last: a whole CQ on the air.
    fn one_cq_frame(grid: &str) -> Vec<(Frame, bool, bool)> {
        let cq = Frame::Heartbeat {
            call: "KD9TAW".into(),
            grid: Some(grid.into()),
            is_cq: true,
            idx: 0,
        };
        vec![(cq, true, true)]
    }

    /// The CQ carries the 4-character square, as JS8Call's does: its default CQ text is
    /// `CQ CQ CQ <MYGRID4>` (Configuration.cpp:1860; the macro is `my_grid().left(4)`,
    /// mainwindow.cpp:7024), and `sendCQ` falls back to `my_grid().left(4)` when that text is
    /// empty (:6344). Given the whole 6-character locator, the CQ grammar stops at the square,
    /// the locator spills into a data frame, and every CQ keyed on two periods in a row.
    #[test]
    fn a_clicked_cq_with_a_six_character_locator_is_one_frame_carrying_the_square() {
        let mut c = cfg();
        c.grid = "EN52HW".into();
        let mut s = Station::new(c);
        s.call_cq(0).unwrap();
        assert_eq!(
            keyed_wire(&mut s, 0),
            one_cq_frame("EN52"),
            "one CQ, one frame, the 4-character square"
        );
    }

    /// …and so is a CQ the repeat schedule sends: both buttons compose through `compose_cq`.
    #[test]
    fn a_repeated_cq_with_a_six_character_locator_is_one_frame_carrying_the_square() {
        let mut c = cfg();
        c.grid = "EN52HW".into();
        c.cq_interval_min = 1;
        let mut s = Station::new(c);
        s.mark_active(0);
        s.set_cq(true, 0);
        s.tick(60_000);
        assert_eq!(
            keyed_wire(&mut s, 60_000),
            one_cq_frame("EN52"),
            "one repeated CQ, one frame, the 4-character square"
        );
    }

    /// A 4-character locator was already right and stays exactly as it was.
    #[test]
    fn a_cq_with_a_four_character_locator_is_unchanged() {
        let mut s = Station::new(cfg()); // EN52
        s.call_cq(0).unwrap();
        assert_eq!(keyed_wire(&mut s, 0), one_cq_frame("EN52"));
    }

    /// One CQ frame of form `idx` (JS8Call's `cqs` table: 1 is CQ DX, 2 CQ QRP) with `grid`.
    fn one_cq_form(idx: u8, grid: &str) -> Vec<(Frame, bool, bool)> {
        let cq = Frame::Heartbeat {
            call: "KD9TAW".into(),
            grid: Some(grid.into()),
            is_cq: true,
            idx,
        };
        vec![(cq, true, true)]
    }

    /// The CQ message is what the CQ button keys (JS8Call's `CQMessage`, sent by `sendCQ`,
    /// mainwindow.cpp:4524-4545), its macros replaced: `CQ DX <MYGRID4>` is the one CQ DX frame
    /// with the square.
    #[test]
    fn the_cq_message_is_what_the_cq_button_keys() {
        let mut c = cfg();
        c.cq_text = "CQ DX <MYGRID4>".into();
        let mut s = Station::new(c);
        s.call_cq(0).unwrap();
        assert_eq!(keyed_wire(&mut s, 0), one_cq_form(1, "EN52"));
    }

    /// …and what the CQ repeat keys: both compose through `compose_cq`.
    #[test]
    fn the_cq_message_is_what_the_cq_repeat_keys() {
        let mut c = cfg();
        c.cq_text = "CQ QRP <MYGRID4>".into();
        c.cq_interval_min = 1;
        let mut s = Station::new(c);
        s.mark_active(0);
        s.set_cq(true, 0);
        s.tick(60_000);
        assert_eq!(keyed_wire(&mut s, 60_000), one_cq_form(2, "EN52"));
    }

    /// A CQ message longer than a CQ form goes as JS8Call frames the same words: the CQ frame,
    /// then the rest in data frames. Every macro is replaced, and the selected-call words, which
    /// a CQ never has (`sendCQ` clears the selection first, mainwindow.cpp:4539), are pruned.
    #[test]
    fn a_longer_cq_message_keys_its_words_with_the_macros_replaced() {
        let mut c = cfg();
        c.cq_text = "CQ CQ CQ <MYGRID4> <CALL>QRP DE <MYCALL>".into();
        let mut s = Station::new(c);
        s.call_cq(0).unwrap();
        let mut plain = Station::new(cfg());
        plain.send(None, "CQ CQ CQ EN52 QRP DE KD9TAW", 0).unwrap();
        let cq = keyed_wire(&mut s, 0);
        assert_eq!(
            cq,
            keyed_wire(&mut plain, 0),
            "the same frames as those words"
        );
        assert!(cq.len() > 1, "more than the CQ frame: {cq:?}");
        assert_eq!(cq[0].0, one_cq_frame("EN52")[0].0, "the CQ frame leads");
        let mut s = Station::new(StationConfig {
            cq_text: "CQ CQ CQ <MYGRID4> <CALL>QRP DE <MYCALL>".into(),
            ..cfg()
        });
        s.call_cq(0).unwrap();
        assert_eq!(
            drain(&mut s, 0).map(|f| f.display).as_deref(),
            Some("KD9TAW: @ALLCALL CQ CQ CQ EN52 QRP DE KD9TAW")
        );
    }

    /// An empty CQ message sends JS8Call's fallback, `CQ CQ CQ` and the square
    /// (mainwindow.cpp:4534-4537); the legacy `CQCQCQ` is read as `CQ CQ CQ` (Configuration.cpp:
    /// 1288-1290).
    #[test]
    fn an_empty_cq_message_sends_cq_cq_cq_and_the_square() {
        for text in ["", "   ", "CQCQCQ <MYGRID4>"] {
            let mut c = cfg();
            c.cq_text = text.into();
            c.grid = "EN52HW".into();
            let mut s = Station::new(c);
            s.call_cq(0).unwrap();
            assert_eq!(keyed_wire(&mut s, 0), one_cq_frame("EN52"), "{text:?}");
        }
    }

    /// A compound callsign's directed message leads with JS8Call's compound announcement,
    /// `` `MYCALL GRID4 `` (varicode.cpp:2125, live under `ALLOW_SEND_COMPOUND_DIRECTED`,
    /// :1934), and every message JS8Call builds is handed the square for it
    /// (`buildMessageFrames(…, my_grid().left(4), …)`, mainwindow.cpp:5434). The station
    /// composed without one, so the announcement went out with no grid. (A `/P` call is not
    /// compound: it rides the directed frame's portable bit, in JS8Call too.)
    #[test]
    fn a_compound_callsigns_directed_message_announces_the_square() {
        let mut c = cfg();
        c.mycall = "KD9TAW/QRP".into();
        c.grid = "EN52HW".into();
        let mut s = Station::new(c);
        s.send_command(&CallRef::Base("W1AW".into()), Command::SnrQuery, "", 0)
            .unwrap();
        let announcement = Frame::Compound {
            call: "KD9TAW/QRP".into(),
            grid: Some("EN52".into()),
        };
        let directed = Frame::CompoundDirected {
            call: "W1AW".into(),
            cmd: Command::SnrQuery,
            num: None,
        };
        assert_eq!(
            keyed_wire(&mut s, 0),
            vec![(announcement, true, false), (directed, false, true)],
            "the compound announcement carries the 4-character square"
        );
    }

    /// The first frame the station keys from `from_ms`, as it reads on the air.
    fn first_on_air(s: &mut Station, from_ms: u64) -> Option<Frame> {
        keyed_wire(s, from_ms).into_iter().next().map(|(f, _, _)| f)
    }

    /// KD9TAW/QRP, a compound callsign, with a 6-character locator.
    fn compound_station() -> Station {
        Station::new(StationConfig {
            mycall: "KD9TAW/QRP".into(),
            grid: "EN52HW".into(),
            ..cfg()
        })
    }

    /// …a typed message to a station takes the same route: JS8Call frames the message box at
    /// the start of the over (`appendMessage`, mainwindow.cpp:5492 → :5344), through the same
    /// `buildMessageFrames` that is handed the square (:5434)…
    #[test]
    fn a_compound_callsigns_typed_message_announces_the_square() {
        let mut s = compound_station();
        s.send(Some(&CallRef::Base("W1AW".into())), "SNR?", 0)
            .unwrap();
        assert_eq!(
            first_on_air(&mut s, 0),
            Some(Frame::Compound {
                call: "KD9TAW/QRP".into(),
                grid: Some("EN52".into()),
            }),
            "the typed message's announcement carries the square"
        );
    }

    /// A heartbeat that falls due while nothing may transmit is dropped and its interval
    /// re-based, as JS8Call's is: `checkRepeat` (mainwindow.cpp:5723) sends it anyway, `startTx`
    /// finds TX off (`ensureCanTransmit`, :5295) and calls `on_stopTxButton_clicked` (:5304),
    /// which clears the queue and re-bases the heartbeat (`resetAutomaticIntervalTransmissions`,
    /// :7397 → :3720). "Due" is `checkRepeat`'s: `secsTo(next) <= 0`, under a second before it.
    #[test]
    fn a_heartbeat_due_while_tx_is_off_is_dropped_and_re_based() {
        let mut c = cfg();
        c.hb_interval_min = 5;
        let mut s = Station::new(c);
        s.set_hb(true, 67_250); // due at 376 s
        assert!(
            !s.drop_due_heartbeat(375_000),
            "not dropped a whole second early"
        );
        assert_eq!(
            s.hb_next_ms(),
            Some(376_000),
            "a whole second early is not due yet"
        );
        assert!(s.drop_due_heartbeat(375_001), "dropped once due");
        assert_eq!(
            s.hb_next_ms(),
            Some(391_000 + 300_000),
            "due: dropped, and re-based to the next transmit cycle + the interval"
        );
        assert!(drain(&mut s, 390_000).is_none(), "and nothing is queued");
    }

    /// …and an on-demand one (interval 0) is simply dropped: JS8Call's single press sends it at
    /// once, and TX being off discards it the same way.
    #[test]
    fn an_on_demand_heartbeat_pressed_while_tx_is_off_is_dropped() {
        let mut s = Station::new(cfg()); // interval 0
        s.set_hb(true, 1_000);
        assert!(s.drop_due_heartbeat(1_000), "dropped");
        assert_eq!(s.hb_next_ms(), None, "the on-demand heartbeat is dropped");
        assert!(drain(&mut s, 15_000).is_none(), "and nothing is queued");
    }

    /// JS8Call sends to @APRSIS and @JS8NET as to any group: both are packable destinations
    /// (varicode.cpp:217, :259), and `isGroupAllowed` (:1314-1320), which names them, is asked
    /// only when a group is JOINED (Configuration.cpp:1016, :2450). A message with one in the
    /// To field goes out as the same directed traffic as the line typed with it.
    #[test]
    fn a_message_to_aprsis_or_js8net_goes_out_as_directed_traffic() {
        for to in ["@APRSIS", "@JS8NET"] {
            let dest = CallRef::parse(to).expect("a packable group");
            let mut s = Station::new(cfg());
            if let Err(e) = s.send(Some(&dest), "GRID EN52", 0) {
                panic!("{to}: JS8Call sends it, got {e:?}");
            }
            let wire = keyed_wire(&mut s, 0);
            assert!(
                matches!(
                    wire.first(),
                    Some((Frame::Directed { from, to: d, cmd: Command::Grid, .. }, true, _))
                        if from.render() == "KD9TAW" && *d == dest
                ),
                "{to}: a GRID directed to {to} from KD9TAW: {wire:?}"
            );
            let mut typed = Station::new(cfg());
            typed.send(None, &format!("{to} GRID EN52"), 0).unwrap();
            assert_eq!(
                wire,
                keyed_wire(&mut typed, 0),
                "{to}: the same frames as the line typed with it"
            );
        }
    }

    /// A station whose locator is `grid`, with INFO and STATUS set.
    fn macro_station(grid: &str, info: &str, status: &str) -> Station {
        Station::new(StationConfig {
            grid: grid.into(),
            info: info.into(),
            status: status.into(),
            ..cfg()
        })
    }

    /// The reply the station schedules to `cmd` from W1AW, as it goes out.
    fn reply_to(s: &mut Station, cmd: Command) -> Option<String> {
        s.on_event(&directed("W1AW", "KD9TAW", Some(cmd), None, "", -7), 1000);
        drain(s, 5000).map(|f| f.display)
    }

    /// JS8Call answers INFO? and STATUS? with the text's macros replaced (`replaceMacros`,
    /// mainwindow.cpp:8845, :8855), `<MYGRID4>` and `<MYGRID12>` among them: the locator's first 4
    /// and first 12 characters, upper-cased (`buildMacroValues`, :7024-7025; `replaceMacros`,
    /// :181-194). An 8-character locator tells the two apart.
    #[test]
    fn info_and_status_replies_expand_the_grid_macros() {
        let mut s = macro_station("EN52hw12", "QTH <MYGRID4> LOC <MYGRID12>", "");
        assert_eq!(
            reply_to(&mut s, Command::InfoQuery).as_deref(),
            Some("KD9TAW: W1AW INFO QTH EN52 LOC EN52HW12"),
            "INFO? is answered with the macros replaced"
        );
        let mut s = macro_station("EN52hw12", "", "PORTABLE IN <MYGRID4>");
        assert_eq!(
            reply_to(&mut s, Command::StatusQuery).as_deref(),
            Some("KD9TAW: W1AW STATUS PORTABLE IN EN52"),
            "STATUS? is answered with the macros replaced"
        );
    }

    /// INFO? and STATUS? are answered with every macro JS8Call knows replaced and any `<…>` left
    /// pruned (`replaceMacros(…, true)`, processCommandActivity.cpp:347-358). A station has no
    /// selected call, so `<CALL>` goes; an idle station that has not yet been used is idle 0M.
    #[test]
    fn info_and_status_replies_replace_every_macro_and_prune_the_rest() {
        let mut s = macro_station(
            "EN52hw12",
            "<mycall> QTH <MYGRID4><CALL> IDLE <MYIDLE> <MYHB>",
            "",
        );
        assert_eq!(
            reply_to(&mut s, Command::InfoQuery).as_deref(),
            Some("KD9TAW: W1AW INFO KD9TAW QTH EN52 IDLE 0M HB EN52")
        );
        let mut s = macro_station("EN52", "", "<FOO>ON <MYVERSION> <MYCQ>");
        assert_eq!(
            reply_to(&mut s, Command::StatusQuery).as_deref(),
            Some("KD9TAW: W1AW STATUS ON NEXUS CQ CQ CQ EN52")
        );
    }

    /// Everything the operator sends is framed with its macros replaced: JS8Call runs the message
    /// box through `replaceMacros` whenever it frames it (`appendMessage`, :5345, from
    /// `prepareNextMessageFrame`, :5492). Its editor has upper-cased the text by then, so a token
    /// typed in lower case is replaced here too.
    #[test]
    fn a_sent_message_expands_the_grid_macros() {
        let mut s = macro_station("EN52HW12", "", "");
        s.send(None, "MY GRID IS <mygrid12>", 0).unwrap();
        assert_eq!(
            drain(&mut s, 0).map(|f| f.display).as_deref(),
            Some("KD9TAW: MY GRID IS EN52HW12"),
            "a typed message"
        );
        let mut s = macro_station("EN52HW12", "", "");
        let w1aw = CallRef::Base("W1AW".into());
        s.send_command(&w1aw, Command::Msg, "QRV FROM <MYGRID4>", 0)
            .unwrap();
        assert_eq!(
            drain(&mut s, 0).map(|f| f.display).as_deref(),
            Some("KD9TAW: W1AW MSG QRV FROM EN52"),
            "a command's text"
        );
    }

    /// An HB-ACK goes on a free heartbeat spot, as JS8Call's `sendHeartbeatAck` picks it
    /// (`findFreeFreqOffset(500, 1000, 50)`, mainwindow.cpp:6299), not on the dial.
    #[test]
    fn an_hb_ack_goes_on_a_free_heartbeat_spot() {
        let mut c = cfg();
        c.hb_ack = true;
        c.hb_interval_min = 5; // our own heartbeat is not due in this window
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        s.on_event(&heartbeat("W1AW", -5), 1000);
        s.process_tx_queue();
        let mut rng = || 3u32; // the first draw: 500 + 50 × 3
        let f = s
            .next_frame(5000, &|_| false, &mut rng)
            .expect("the HB-ACK in the next period");
        assert_eq!(f.origin, Origin::HbAck, "precondition: the HB-ACK");
        assert_eq!(
            f.freq_hint,
            FreqHint::HbSubband(650.0),
            "the HB-ACK goes on a free heartbeat spot"
        );
    }

    /// …and so does an automatic reply: JS8Call puts a queued reply in the same message box
    /// (`processTxQueue`, :9671) and frames it the same way.
    #[test]
    fn a_compound_callsigns_automatic_reply_announces_the_square() {
        let mut s = compound_station();
        s.on_event(
            &directed("W1AW", "KD9TAW/QRP", Some(Command::SnrQuery), None, "", -5),
            0,
        );
        assert_eq!(
            first_on_air(&mut s, 1000), // cfg()'s reply countdown is 1 s
            Some(Frame::Compound {
                call: "KD9TAW/QRP".into(),
                grid: Some("EN52".into()),
            }),
            "the automatic reply's announcement carries the square"
        );
    }

    #[test]
    fn halt_mid_queue_produces_nothing_afterward_and_is_idempotent() {
        let mut s = Station::new(cfg());
        // a multi-frame message: emit the first frame, then halt.
        s.send(
            Some(&CallRef::Base("W1AW".into())),
            "MSG HELLO WORLD THIS IS A LONG ONE",
            0,
        )
        .unwrap();
        assert!(drain(&mut s, 1000).is_some(), "first frame goes out");
        s.halt();
        s.halt(); // idempotent
        assert!(drain(&mut s, 2000).is_none(), "nothing keys after halt");
        assert!(s.queue().is_empty());
    }

    #[test]
    fn the_only_tx_seam_is_next_frame_and_frames_carry_their_origin() {
        let mut s = Station::new(cfg());
        // operator verbs return counts/units, never a TxFrame; the frame only exists after next_frame.
        let _n: usize = s.send(None, "TNX 73 GL", 0).unwrap();
        let f = drain(&mut s, 1000).expect("operator frame");
        assert_eq!(f.origin, Origin::Operator);
        assert!(!f.display.is_empty());
        // heartbeat picks an HbSubband slot from the injected RNG; a reply stays on the dial.
        let mut s2 = Station::new(cfg());
        s2.heartbeat_now(0).unwrap();
        let mut rng = || 3u32; // slot 500 + 3*50 = 650
        let f = s2.next_frame(1000, &|_| false, &mut rng).unwrap();
        assert_eq!(f.origin, Origin::Heartbeat);
        assert_eq!(f.freq_hint, FreqHint::HbSubband(650.0));
    }

    #[test]
    fn snapshot_restore_round_trips_the_inbox_and_heard() {
        let mut s = Station::new(cfg());
        s.on_event(
            &directed(
                "W1AW",
                "KD9TAW",
                Some(Command::MsgTo),
                None,
                "K1ABC HELLO",
                -5,
            ),
            1000,
        );
        s.on_event(&heartbeat("N0XYZ", -12), 2000);
        let snap = s.snapshot(2500);
        let json = serde_json::to_string(&snap).unwrap();
        let back: StationSnapshot = serde_json::from_str(&json).unwrap();
        let mut s2 = Station::new(cfg());
        s2.restore(back, 3000);
        assert_eq!(s2.inbox(), s.inbox());
        assert_eq!(s2.heard(), s.heard());
        assert!(s2
            .inbox()
            .iter()
            .any(|e| e.to == "K1ABC" && e.state == InboxState::Store));
    }

    // ---- resource-exhaustion / hostile-input bounds (security fix) ----

    #[test]
    fn inbox_is_bounded_under_a_flood_of_msg_to_from_cycled_callsigns() {
        let mut s = Station::new(cfg());
        for i in 0..5000u32 {
            let from = format!("K{}AAA", i % 10);
            let to = format!("T{i:04}X");
            let mut acts = s.on_event(
                &directed(
                    &from,
                    "KD9TAW",
                    Some(Command::MsgTo),
                    None,
                    &format!("{to} HELLO {i}"),
                    -5,
                ),
                i as u64 * 1000,
            );
            let _ = acts.drain(..);
        }
        assert!(
            s.inbox().len() <= 100,
            "inbox grew unbounded: {} entries",
            s.inbox().len()
        );
    }

    #[test]
    fn inbox_eviction_is_surfaced_and_normal_traffic_is_not_evicted() {
        // positive control: a handful of stores stays put, no eviction toast.
        let mut s = Station::new(cfg());
        for i in 0..5u32 {
            let acts = s.on_event(
                &directed(
                    "W1AW",
                    "KD9TAW",
                    Some(Command::MsgTo),
                    None,
                    &format!("T{i} HI {i}"),
                    -5,
                ),
                i as u64,
            );
            assert!(!acts.iter().any(
                |a| matches!(a, StationAction::Toast { text, .. } if text.contains("Inbox full"))
            ));
        }
        assert_eq!(s.inbox().len(), 5);
        // now flood past the cap: eviction is surfaced (never silent) and the oldest goes.
        let mut saw_evict = false;
        for i in 0..2000u32 {
            let acts = s.on_event(
                &directed(
                    "W1AW",
                    "KD9TAW",
                    Some(Command::MsgTo),
                    None,
                    &format!("T{i} HI {i}"),
                    -5,
                ),
                100 + i as u64,
            );
            if acts.iter().any(
                |a| matches!(a, StationAction::Toast { text, .. } if text.contains("Inbox full")),
            ) {
                saw_evict = true;
            }
        }
        assert!(saw_evict, "eviction happened but was never surfaced");
        assert!(s.inbox().len() <= 100);
    }

    /// The byte cap holds; and only a GROUP's held message expires, at 48 h, as long as JS8Call
    /// offers one. A message held for one station stays, as JS8Call keeps it (it expires none,
    /// mainwindow.cpp:9508-9530): the ruling that took the 48 h expiry off it.
    #[test]
    fn inbox_bytes_are_capped_and_only_a_group_message_expires_at_48h() {
        // byte cap: even under 100 entries, oversized bodies cannot blow past 64 KB.
        let mut s = Station::new(cfg());
        for i in 0..100u32 {
            s.on_event(
                &directed(
                    "W1AW",
                    "KD9TAW",
                    Some(Command::MsgTo),
                    None,
                    &format!("T{i} {}", "X".repeat(2000)),
                    -5,
                ),
                i as u64,
            );
        }
        let bytes: usize = s.inbox().iter().map(|e| e.text.len()).sum();
        assert!(bytes <= 64 * 1024 + 512, "inbox bytes unbounded: {bytes}");
        // 48 h: the group's message is reclaimed on the next tick; K1ABC's stays, a month on too.
        let mut c = cfg();
        c.groups = vec!["@FUN".into()];
        let mut s2 = Station::new(c);
        for body in ["K1ABC HELLO", "@FUN HELLO ALL"] {
            s2.on_event(
                &directed("W1AW", "KD9TAW", Some(Command::MsgTo), None, body, -5),
                1000,
            );
        }
        let held =
            |s: &Station| -> Vec<String> { s.inbox().iter().map(|e| e.to.clone()).collect() };
        assert_eq!(held(&s2), ["K1ABC", "@FUN"]);
        s2.tick(1000 + 48 * 60 * 60 * 1000 - 1);
        assert_eq!(held(&s2), ["K1ABC", "@FUN"], "both held until 48 h");
        s2.tick(1000 + 48 * 60 * 60 * 1000);
        assert_eq!(held(&s2), ["K1ABC"], "the group's message goes at 48 h");
        s2.tick(1000 + 30 * 24 * 60 * 60 * 1000);
        assert_eq!(held(&s2), ["K1ABC"], "the one for K1ABC does not expire");
    }

    /// The frame JS8Call keys for `to ACK` from KD9TAW: ONE directed frame, first and last.
    fn ack_frame(to: &str, portable_to: bool) -> Word87 {
        let ack = Frame::Directed {
            from: CallRef::Base("KD9TAW".into()),
            to: CallRef::Base(to.into()),
            cmd: Command::Ack,
            num: None,
            portable_from: false,
            portable_to,
        };
        let whole = I3 {
            first: true,
            last: true,
            data: false,
        };
        encode_frame(&ack, whole, Speed::Normal).expect("packs")
    }

    /// The texts of the automatic replies waiting to go, oldest first: asking for a Yes, or in
    /// the queue, where one made with the confirmation off goes at once.
    fn waiting(s: &Station) -> Vec<String> {
        let mine = format!("{}: ", s.base());
        s.pending
            .iter()
            .map(|p| p.text.clone())
            .chain(s.queued.iter().map(|q| q.text.clone()))
            .chain(
                s.outbox
                    .iter()
                    .filter(|o| {
                        matches!(o.origin, Origin::AutoReply | Origin::Relay | Origin::HbAck)
                    })
                    .map(|o| {
                        o.display
                            .strip_prefix(&mine)
                            .unwrap_or(&o.display)
                            .to_string()
                    }),
            )
            .collect()
    }

    /// ⭐ A MSG TO ME IS FILED UNREAD, as JS8Call's `addCommandToMyInbox` files it
    /// (mainwindow.cpp:9121-9133, :9462-9467): who from, to whom, the text and when, at the
    /// offset and SNR it was heard. And it is answered as JS8Call answers it (:9139): `<from>
    /// ACK`, an automatic reply after the countdown, one directed frame on my own offset.
    #[test]
    fn a_msg_to_me_is_filed_unread_and_answered_with_an_ack() {
        let mut s = Station::new(cfg());
        let acts = s.on_event(
            &directed(
                "W1AW",
                "KD9TAW",
                Some(Command::Msg),
                None,
                "HELLO FROM OHIO",
                -7,
            ),
            5_000,
        );
        assert_eq!(s.inbox().len(), 1, "the MSG was not filed");
        let e = &s.inbox()[0];
        assert_eq!(
            (e.from.as_str(), e.to.as_str(), e.text.as_str(), e.state),
            ("W1AW", "KD9TAW", "HELLO FROM OHIO", InboxState::Unread)
        );
        assert_eq!((e.at_ms, e.freq_hz, e.snr_db), (5_000, 1500.0, -7));
        assert_eq!(
            e.path,
            vec!["W1AW".to_string()],
            "JS8Call's PATH is the sender when nothing relayed it"
        );
        assert!(
            acts.iter()
                .any(|a| matches!(a, StationAction::InboxChanged)),
            "the journal is not told"
        );
        assert_eq!(waiting(&s), ["W1AW ACK"], "the ACK is queued: {acts:?}");
        let f = drain(&mut s, 6_000).expect("the ACK in the next period");
        assert_eq!(
            (f.origin, f.display.as_str(), f.freq_hint, f.first, f.last),
            (
                Origin::AutoReply,
                "KD9TAW: W1AW ACK",
                FreqHint::Dial,
                true,
                true
            )
        );
        assert_eq!(
            f.word,
            ack_frame("W1AW", false),
            "JS8Call's `W1AW ACK`: KD9TAW to W1AW, ACK"
        );
        assert!(
            drain(&mut s, 60_000).is_none(),
            "one ACK, and nothing after it"
        );
    }

    /// …and a MSG to a group I joined (JS8Call's `isGroupCall`, :8584, :8715). Never one on
    /// @ALLCALL (`!isAllCall`, :9121), to another station, or to a group I have not joined.
    #[test]
    fn a_msg_is_filed_for_me_and_my_groups_only() {
        let mut c = cfg();
        c.groups = vec!["@FUN".into()];
        let mut s = Station::new(c);
        for (to, text) in [
            ("@FUN", "GROUP HELLO"),
            ("@ALLCALL", "TO EVERYONE"),
            ("K1ABC", "NOT FOR ME"),
            ("@OTHER", "NOT MY GROUP"),
        ] {
            s.on_event(
                &directed("W1AW", to, Some(Command::Msg), None, text, -5),
                1_000,
            );
        }
        let filed: Vec<_> = s
            .inbox()
            .iter()
            .map(|e| (e.to.as_str(), e.text.as_str(), e.state))
            .collect();
        assert_eq!(
            filed,
            [("@FUN", "GROUP HELLO", InboxState::Unread)],
            "filed for me and my groups only"
        );
    }

    /// The PATH JS8Call keeps with it (`parseRelayPathCallsigns`, :9568-9579): the sender, then
    /// each call named after `*DE*` or `VIA`, the last one named first.
    #[test]
    fn a_relayed_msg_keeps_its_relay_path() {
        let mut s = Station::new(cfg());
        s.on_event(
            &directed(
                "OH8STN",
                "KD9TAW",
                Some(Command::Msg),
                None,
                "HELLO BRAVE SOUL *DE* N0JDS",
                -5,
            ),
            1_000,
        );
        s.on_event(
            &directed(
                "OH8STN",
                "KD9TAW",
                Some(Command::Msg),
                None,
                "QRV *DE* N0JDS VIA K1ABC",
                -5,
            ),
            2_000,
        );
        s.on_event(
            &directed(
                "OH8STN",
                "KD9TAW",
                Some(Command::Msg),
                None,
                "*DE* N0JDS SAYS VIA NOBODY HERE",
                -5,
            ),
            3_000,
        );
        let paths: Vec<Vec<String>> = s.inbox().iter().map(|e| e.path.clone()).collect();
        assert_eq!(
            paths,
            [
                vec!["OH8STN".to_string(), "N0JDS".to_string()],
                vec![
                    "OH8STN".to_string(),
                    "K1ABC".to_string(),
                    "N0JDS".to_string()
                ],
                // A marker opening the text has no space before it, and NOBODY is no callsign.
                vec!["OH8STN".to_string()],
            ],
            "the relay paths"
        );
    }

    /// …and a MSG to a group I joined is answered to its sender, as the thirteen JS8Call
    /// stations in the golden log answered KJ5MIW's @SITREP MSG. A MSG on @ALLCALL
    /// (`!isAllCall`, :9121), to another station or to a group I have not joined is never
    /// answered.
    #[test]
    fn a_msg_to_a_joined_group_is_acked_to_its_sender_and_no_other_msg_is() {
        let mut c = cfg();
        c.groups = vec!["@FUN".into()];
        let mut s = Station::new(c);
        // A sender each, so no ACK can hide behind another with the same text.
        for (from, to) in [
            ("N0FUN", "@FUN"),
            ("N0ALL", "@ALLCALL"),
            ("N0DX", "K1ABC"),
            ("N0GRP", "@OTHER"),
        ] {
            s.on_event(
                &directed(from, to, Some(Command::Msg), None, "HELLO", -5),
                1_000,
            );
        }
        assert_eq!(waiting(&s), ["N0FUN ACK"], "the joined group's sender only");
    }

    /// A relayed MSG is answered back along its relay path, as JS8Call answers it
    /// (`calls.length() > 1 ? d.relayPath : d.from`, :9139, the path the inbox keeps): the ACK
    /// is a relay request to the station that brought the MSG.
    #[test]
    fn a_relayed_msg_is_acked_along_its_relay_path() {
        let mut s = Station::new(cfg());
        for (t, text) in [
            (1_000, "HELLO BRAVE SOUL *DE* N0JDS"),
            (2_000, "QRV *DE* N0JDS VIA K1ABC"),
            (3_000, "*DE* N0JDS SAYS VIA NOBODY HERE"),
        ] {
            s.on_event(
                &directed("OH8STN", "KD9TAW", Some(Command::Msg), None, text, -5),
                t,
            );
        }
        assert_eq!(
            waiting(&s),
            ["OH8STN>N0JDS ACK", "OH8STN>K1ABC>N0JDS ACK", "OH8STN ACK"],
            "back along the relay path"
        );
        let sent = frames_with_grid("KD9TAW", "EN52", None, "OH8STN>N0JDS ACK", Speed::Normal)
            .expect("composes");
        assert_eq!(
            sent[0].0,
            Frame::Directed {
                from: CallRef::Base("KD9TAW".into()),
                to: CallRef::Base("OH8STN".into()),
                cmd: Command::Relay,
                num: None,
                portable_from: false,
                portable_to: false,
            },
            "the relayed ACK is a relay request to OH8STN"
        );
        let f = drain(&mut s, 10_000).expect("the first ACK");
        assert_eq!(
            (f.word, f.first, f.last),
            (
                encode_frame(&sent[0].0, sent[0].1, Speed::Normal).unwrap(),
                true,
                false
            ),
            "…and keys as one"
        );
    }

    /// The ACK names the sender as it was heard (`d.from`, :9139): a portable call keeps its
    /// `/P`, which is the portable flag in the directed frame, and a compound call stays whole.
    #[test]
    fn the_ack_names_the_sender_as_heard() {
        let mut s = Station::new(cfg());
        for from in ["W1AW/P", "VE3/W1AW"] {
            s.on_event(
                &directed(from, "KD9TAW", Some(Command::Msg), None, "HI", -5),
                1_000,
            );
        }
        assert_eq!(
            waiting(&s),
            ["W1AW/P ACK", "VE3/W1AW ACK"],
            "the sender as heard"
        );
        let f = drain(&mut s, 10_000).expect("the first ACK");
        assert_eq!(
            f.word,
            ack_frame("W1AW", true),
            "W1AW/P ACK: W1AW with the portable flag"
        );
    }

    /// The ACK is an automatic reply. With autoreply off the MSG is filed and nothing is sent:
    /// JS8Call types the ACK into its compose box and keys it only with AUTO checked
    /// (`processTxQueue`, :9671-9685), and so the ACK is in the composer. Once the idle watchdog
    /// trips, which turns autoreply off and drops what waits, nothing is made at all (:8818,
    /// :11000, :11005).
    #[test]
    fn with_autoreply_off_the_ack_goes_to_the_composer_and_none_once_the_watchdog_trips() {
        let msg = directed("W1AW", "KD9TAW", Some(Command::Msg), None, "HELLO", -5);
        let mut c = cfg();
        c.autoreply = false;
        let mut s = Station::new(c);
        s.on_event(&msg, 1_000);
        assert_eq!(s.inbox().len(), 1, "control: the MSG is filed");
        s.process_tx_queue();
        assert!(s.queue().is_empty(), "autoreply off: no ACK on the air");
        assert!(drain(&mut s, 60_000).is_none(), "…and nothing goes out");
        assert_eq!(
            s.sync_composer(false, None).1.map(|c| c.1).as_deref(),
            Some("W1AW ACK"),
            "the ACK is in the composer, for the operator to send"
        );

        let mut c = cfg();
        c.idle_watchdog_min = 5;
        let mut s = Station::new(c);
        s.mark_active(0);
        s.on_event(&msg, 1_000);
        assert_eq!(
            waiting(&s),
            ["W1AW ACK"],
            "control: before the trip it is answered"
        );
        s.tick(5 * 60 * 1000);
        assert!(s.idle_tripped(), "control: the watchdog tripped");
        assert!(
            waiting(&s).is_empty(),
            "the trip drops the ACK that was waiting"
        );
        s.on_event(&msg, 5 * 60 * 1000 + 1);
        assert!(waiting(&s).is_empty(), "tripped: no ACK");
        assert!(
            drain(&mut s, 6 * 60 * 1000).is_none(),
            "…and nothing goes out"
        );
    }

    /// A copy of a MSG heard while its ACK still waits draws no second ACK: queued, or on its way,
    /// it is in JS8Call's message box, and no reply is queued while the box holds text (:9364).
    /// Every copy is filed, as JS8Call files it. A MSG from another station heard in the same
    /// pass has its own ACK (JS8Call queues every reply of one pass before `processTxQueue`
    /// fills the box); a copy heard once the ACK has gone is answered again, as JS8Call answers
    /// it: its sender sent it again.
    #[test]
    fn a_copy_of_a_msg_heard_while_its_ack_waits_draws_no_second_ack() {
        let mut s = Station::new(cfg());
        let msg = directed("W1AW", "KD9TAW", Some(Command::Msg), None, "HELLO", -5);
        let other = directed("N0XYZ", "KD9TAW", Some(Command::Msg), None, "HI", -5);
        s.on_event(&msg, 1_000);
        s.on_event(&msg, 1_000); // a copy in the same pass
        s.on_event(&other, 1_000);
        assert_eq!(
            waiting(&s),
            ["W1AW ACK", "N0XYZ ACK"],
            "one ACK for two copies, and the other station's"
        );
        let f = drain(&mut s, 15_000).expect("the first ACK keys");
        assert_eq!(f.display, "KD9TAW: W1AW ACK");
        let f = drain(&mut s, 30_000).expect("the second ACK keys");
        assert_eq!(f.display, "KD9TAW: N0XYZ ACK");
        assert!(drain(&mut s, 45_000).is_none(), "one ACK each, no more");
        assert_eq!(s.inbox().len(), 3, "every copy is filed");
        s.on_event(&msg, 46_000);
        assert_eq!(
            waiting(&s),
            ["W1AW ACK"],
            "a copy heard once the ACK has gone is answered again"
        );
    }
    /// A relay path over the hop cap is not answered: its calls come from the sender's own
    /// text, and upstream, which has no cap, would key every one of them. `handle_relay`'s cap.
    #[test]
    fn a_msg_whose_relay_path_is_over_the_hop_cap_is_not_acked() {
        let hops = |n: usize| {
            (0..n)
                .map(|i| format!("*DE* K{i}AA"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut s = Station::new(cfg());
        let text = format!("HI {}", hops(MAX_PATH_HOPS));
        let acts = s.on_event(
            &directed("OH8STN", "KD9TAW", Some(Command::Msg), None, &text, -5),
            1_000,
        );
        assert!(
            waiting(&s).is_empty(),
            "a path of {} calls is not answered",
            MAX_PATH_HOPS + 1
        );
        assert!(
            acts.iter().any(|a| matches!(
                a,
                StationAction::Toast { text, .. } if text == "Relay chain over 8 hops refused"
            )),
            "…and it says so: {acts:?}"
        );
        let text = format!("HI {}", hops(MAX_PATH_HOPS - 1));
        s.on_event(
            &directed("OH8STN", "KD9TAW", Some(Command::Msg), None, &text, -5),
            2_000,
        );
        assert_eq!(
            waiting(&s).len(),
            1,
            "control: a path of {MAX_PATH_HOPS} calls is answered"
        );
    }

    /// While the idle watchdog stands, nothing is answered, relayed or delivered, as JS8Call
    /// processes no automatic reply while idle (mainwindow.cpp:8818), even after a settings change
    /// has put the switches back on in the station (the engine's `js8_apply_station_config`). An
    /// operator act clears the trip, and the same query is answered again.
    #[test]
    fn no_reply_of_any_kind_while_the_idle_watchdog_stands() {
        let mut c = cfg();
        c.idle_watchdog_min = 5;
        c.hb_ack = true;
        let mut s = Station::new(c.clone());
        s.mark_active(0);
        // A message held for K1ABC from before the trip (its ACK is not what this is about).
        store_for(&mut s, "K1ABC", 1000);
        let id = s.inbox()[0].id;
        s.tick(5 * 60 * 1000);
        assert!(s.idle_tripped(), "control: the watchdog tripped");
        s.set_config(c);
        assert!(
            s.config().autoreply && s.config().relay,
            "precondition: a settings change put the switches back on in the station"
        );
        let t = 5 * 60 * 1000 + 1;
        // Nothing is stored or filed while it stands either: JS8Call's `continue` at :8817-8820
        // comes before the MSG TO: and MSG branches (JS8Call's filing difference).
        s.on_event(
            &directed(
                "N0XYZ",
                "KD9TAW",
                Some(Command::MsgTo),
                None,
                "K2DEF FRIDAY CONTACT",
                -5,
            ),
            t,
        );
        assert_eq!(s.inbox().len(), 1, "MSG TO: not stored while tripped");
        assert!(waiting(&s).is_empty(), "MSG TO: no ACK while tripped");
        for (what, ev) in [
            (
                "SNR?",
                directed("W1AW", "KD9TAW", Some(Command::SnrQuery), None, "", -5),
            ),
            (
                "MSG",
                directed("W1AW", "KD9TAW", Some(Command::Msg), None, "HELLO", -5),
            ),
            (
                "QUERY MSGS",
                directed("K1ABC", "KD9TAW", Some(Command::QueryMsgs), None, "", -5),
            ),
            (
                "QUERY MSG",
                directed(
                    "K1ABC",
                    "KD9TAW",
                    Some(Command::Query),
                    None,
                    &format!("MSG {id}"),
                    -5,
                ),
            ),
            (
                "relay",
                directed("W1AW", "KD9TAW", Some(Command::Relay), None, "N0XYZ HI", -5),
            ),
        ] {
            s.on_event(&ev, t);
            assert!(waiting(&s).is_empty(), "{what}: no reply while tripped");
        }
        assert_eq!(s.inbox().len(), 1, "MSG: not filed while tripped");
        s.set_hb(true, t);
        s.on_event(&heartbeat("N0XYZ", -5), t);
        assert!(waiting(&s).is_empty(), "HB-ACK: no reply while tripped");
        assert_eq!(
            s.inbox().iter().find(|e| e.id == id).map(|e| e.state),
            Some(InboxState::Store),
            "the held message is not marked delivered"
        );
        s.mark_active(t);
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::SnrQuery), None, "", -5),
            t,
        );
        assert_eq!(
            waiting(&s),
            ["W1AW SNR -05"],
            "control: with the trip cleared it is answered"
        );
    }

    /// Every automatic reply names the sender as heard, a `/P` included, as JS8Call answers
    /// `d.from` (mainwindow.cpp:8834-8903 the queries, :9098 the HB-ACK, :9248 a delivery,
    /// :9270-9285 QUERY MSGS), and a delivered message names its sender as JS8Call stored it
    /// (`cd.from = d.from`, :9036). What a station is filed and found under stays its base call
    /// (`getNextMessageIdForCallsign` matches the base too, :9523), so a message held for W1AW is
    /// still offered and delivered to W1AW/P.
    #[test]
    fn every_automatic_reply_names_the_sender_as_heard() {
        let mut c = cfg();
        c.hb_ack = true;
        c.info = "RIG IC7300".into();
        c.status = "QRV".into();
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        s.on_event(&heartbeat("W1AW/P", -5), 1_000);
        for cmd in [
            Command::SnrQuery,
            Command::GridQuery,
            Command::InfoQuery,
            Command::StatusQuery,
            Command::HearingQuery,
            Command::QueryMsgs,
        ] {
            s.on_event(
                &directed("W1AW/P", "KD9TAW", Some(cmd), None, "", -5),
                1_000,
            );
        }
        assert_eq!(
            waiting(&s),
            [
                "W1AW/P HEARTBEAT SNR -05",
                "W1AW/P SNR -05",
                "W1AW/P GRID EN52",
                "W1AW/P INFO RIG IC7300",
                "W1AW/P STATUS QRV",
                "W1AW/P HEARING",
                "W1AW/P NO",
            ],
            "every reply names W1AW/P as heard"
        );
        let snr = Frame::Directed {
            from: CallRef::Base("KD9TAW".into()),
            to: CallRef::Base("W1AW".into()),
            cmd: Command::Snr,
            num: Some(-5),
            portable_from: false,
            portable_to: true,
        };
        let whole = I3 {
            first: true,
            last: true,
            data: false,
        };
        let hb_ack = drain(&mut s, 2_000).map(|f| f.origin);
        assert_eq!(
            hb_ack,
            Some(Origin::HbAck),
            "the HB-ACK, first in the queue"
        );
        assert_eq!(
            drain(&mut s, 17_000).map(|f| f.word),
            Some(encode_frame(&snr, whole, Speed::Normal).unwrap()),
            "the SNR reply carries the portable flag"
        );

        let mut s = Station::new(cfg());
        s.on_event(
            &directed(
                "N0XYZ/P",
                "KD9TAW",
                Some(Command::MsgTo),
                None,
                "W1AW FRIDAY CONTACT",
                -5,
            ),
            0,
        );
        // its ACK is not what this is about
        s.pending.clear();
        s.queued.clear();
        s.outbox.clear();
        let id = s.inbox()[0].id;
        s.on_event(
            &directed("W1AW/P", "KD9TAW", Some(Command::QueryMsgs), None, "", -5),
            1_000,
        );
        s.on_event(
            &directed(
                "W1AW/P",
                "KD9TAW",
                Some(Command::Query),
                None,
                &format!("MSG {id}"),
                -5,
            ),
            2_000,
        );
        assert_eq!(
            waiting(&s),
            [
                format!("W1AW/P YES MSG ID {id}"),
                "W1AW/P MSG FRIDAY CONTACT FROM N0XYZ/P".to_string(),
            ],
            "the message held for W1AW is offered and delivered to W1AW/P, from N0XYZ/P"
        );
    }

    /// JS8Call holds a `MSG TO:` under its target's base call, `cd.to =
    /// Radio::base_callsign(to)` (mainwindow.cpp:9042): the longer side of the first `/`
    /// (Radio.cpp:117-133), so a prefix goes as a suffix does. Nexus took off `/P` alone.
    #[test]
    fn a_msg_to_for_a_compound_call_is_held_under_its_base_call_as_js8call_holds_it() {
        for (target, held) in [
            ("VE3/K1ABC", "K1ABC"),
            ("K1ABC/MM", "K1ABC"),
            ("K1ABC/P", "K1ABC"),
            ("K1ABC", "K1ABC"),
            ("VE3/K1ABC/P", "K1ABC/P"),
        ] {
            let mut s = Station::new(cfg());
            store_for(&mut s, target, 0);
            assert_eq!(s.inbox()[0].to, held, "MSG TO:{target} is held for {held}");
        }
    }

    /// …and found as JS8Call finds it: under the asking station's call as heard, then under its
    /// base call (`getNextMessageIdForCallsign`, mainwindow.cpp:9508-9533, for QUERY MSGS at
    /// :9268 and a heartbeat's MSG ID at :9082), and handed over to either form (QUERY MSG n
    /// skips only `to != who && to != Radio::base_callsign(who)`, :9219). A message held for
    /// VE3/K1ABC (under K1ABC) is offered to K1ABC and to VE3/K1ABC, its heartbeat is told of it,
    /// VE3/K1ABC gets it, and the roster counts it. One held for VE3/K1ABC/P (under K1ABC/P) is
    /// offered to K1ABC/P as heard, heartbeat and query, and not to K1ABC: JS8Call's lookup does
    /// not find it there.
    #[test]
    fn a_message_held_for_a_compound_call_is_found_under_either_form_as_js8call_finds_it() {
        let mut c = cfg();
        c.hb_ack = true;
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        store_for(&mut s, "VE3/K1ABC", 0);
        let id = s.inbox()[0].id;
        s.on_event(&heartbeat("VE3/K1ABC", -5), 1_000);
        for who in ["K1ABC", "VE3/K1ABC"] {
            s.on_event(
                &directed(who, "KD9TAW", Some(Command::QueryMsgs), None, "", -5),
                1_000,
            );
        }
        s.on_event(
            &directed(
                "VE3/K1ABC",
                "KD9TAW",
                Some(Command::Query),
                None,
                &format!("MSG {id}"),
                -5,
            ),
            2_000,
        );
        assert_eq!(
            waiting(&s),
            [
                format!("VE3/K1ABC HEARTBEAT SNR -05 MSG ID {id}"),
                format!("K1ABC YES MSG ID {id}"),
                format!("VE3/K1ABC YES MSG ID {id}"),
                "VE3/K1ABC MSG FRIDAY CONTACT FROM W1AW".to_string(),
            ],
            "held for VE3/K1ABC: offered to both forms, its heartbeat told, delivered"
        );
        let heard = s.heard().iter().find(|h| h.call == "VE3/K1ABC");
        assert_eq!(
            heard.map(|h| h.stored_msgs),
            Some(1),
            "the roster counts the message held for VE3/K1ABC"
        );

        let mut c = cfg();
        c.hb_ack = true;
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        store_for(&mut s, "VE3/K1ABC/P", 0);
        let id = s.inbox()[0].id;
        s.on_event(&heartbeat("K1ABC/P", -5), 1_000);
        for who in ["K1ABC/P", "K1ABC"] {
            s.on_event(
                &directed(who, "KD9TAW", Some(Command::QueryMsgs), None, "", -5),
                1_000,
            );
        }
        assert_eq!(
            waiting(&s),
            [
                format!("K1ABC/P HEARTBEAT SNR -05 MSG ID {id}"),
                format!("K1ABC/P YES MSG ID {id}"),
                "K1ABC NO".to_string(),
            ],
            "held for VE3/K1ABC/P: found under the call as heard, K1ABC/P"
        );
    }

    /// A `MSG TO:` with nothing after the call is held and acknowledged, as JS8Call holds it (its
    /// store keeps no TEXT, `addCommandToStorage`, mainwindow.cpp:9499-9501; the ACK, :9051), and
    /// never offered: JS8Call's lookup skips a held message whose text is empty
    /// (`getNextMessageIdForCallsign`, :9516-9520), so the heartbeat's acknowledgement carries no
    /// MSG ID and QUERY MSGS is answered NO, and a QUERY MSG n for it draws no reply at all
    /// (:9223-9226). Nexus offered it, `K1ABC YES MSG ID 1`, and delivered it with nothing in it.
    #[test]
    fn a_msg_to_with_no_text_is_held_and_acked_but_never_offered_as_js8call_does() {
        let mut c = cfg();
        c.hb_ack = true;
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::MsgTo), None, "K1ABC", -5),
            0,
        );
        let held: Vec<(&str, &str, InboxState)> = s
            .inbox()
            .iter()
            .map(|e| (e.to.as_str(), e.text.as_str(), e.state))
            .collect();
        assert_eq!(
            held,
            [("K1ABC", "", InboxState::Store)],
            "held for K1ABC, with no text"
        );
        assert_eq!(
            waiting(&s),
            ["W1AW ACK"],
            "acknowledged, as JS8Call acknowledges it"
        );
        s.pending.clear();
        s.queued.clear();
        s.outbox.clear();
        let id = s.inbox()[0].id;
        s.on_event(&heartbeat("K1ABC", -5), 1_000);
        s.on_event(
            &directed("K1ABC", "KD9TAW", Some(Command::QueryMsgs), None, "", -5),
            1_000,
        );
        s.on_event(
            &directed(
                "K1ABC",
                "KD9TAW",
                Some(Command::Query),
                None,
                &format!("MSG {id}"),
                -5,
            ),
            2_000,
        );
        assert_eq!(
            waiting(&s),
            ["K1ABC HEARTBEAT SNR -05", "K1ABC NO"],
            "never offered: no MSG ID, NO to QUERY MSGS, and nothing for QUERY MSG {id}"
        );
    }

    /// …and skipped, not a stop: JS8Call's lookup goes on to the next message held for the
    /// station (mainwindow.cpp:9516-9520), and on to its base call when each one held under the
    /// call as heard is empty (:9525-9529). With empty ones held for K1ABC and for K1ABC/P, then
    /// FRIDAY CONTACT for K1ABC, K1ABC is offered FRIDAY CONTACT, and so is K1ABC/P, under its base
    /// call; neither empty one is delivered (:9223-9226).
    #[test]
    fn a_held_message_with_no_text_is_skipped_for_the_next_one_as_js8call_skips_it() {
        let mut c = cfg();
        c.hb_ack = true;
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        for target in ["K1ABC", "VE3/K1ABC/P"] {
            s.on_event(
                &directed("W1AW", "KD9TAW", Some(Command::MsgTo), None, target, -5),
                0,
            );
        }
        s.pending.clear();
        s.queued.clear();
        s.outbox.clear(); // their ACKs, the test above's own
        store_for(&mut s, "K1ABC", 0);
        let held: Vec<(&str, &str)> = s
            .inbox()
            .iter()
            .map(|e| (e.to.as_str(), e.text.as_str()))
            .collect();
        assert_eq!(
            held,
            [("K1ABC", ""), ("K1ABC/P", ""), ("K1ABC", "FRIDAY CONTACT")],
            "control: the empty ones are held first"
        );
        let ids: Vec<u32> = s.inbox().iter().map(|e| e.id).collect();
        let (empty, empty_p, friday) = (ids[0], ids[1], ids[2]);
        s.on_event(&heartbeat("K1ABC", -5), 1_000);
        for who in ["K1ABC", "K1ABC/P"] {
            s.on_event(
                &directed(who, "KD9TAW", Some(Command::QueryMsgs), None, "", -5),
                1_000,
            );
        }
        for (who, id) in [("K1ABC", empty), ("K1ABC/P", empty_p), ("K1ABC", friday)] {
            s.on_event(
                &directed(
                    who,
                    "KD9TAW",
                    Some(Command::Query),
                    None,
                    &format!("MSG {id}"),
                    -5,
                ),
                2_000,
            );
        }
        assert_eq!(
            waiting(&s),
            [
                format!("K1ABC HEARTBEAT SNR -05 MSG ID {friday}"),
                format!("K1ABC YES MSG ID {friday}"),
                format!("K1ABC/P YES MSG ID {friday}"),
                "K1ABC MSG FRIDAY CONTACT FROM W1AW".to_string(),
            ],
            "FRIDAY CONTACT offered past the empty ones, as heard and by base; neither empty one delivered"
        );
    }

    /// My mail is bounded apart from mail held for someone else. A flood of MSGs to me is capped
    /// on its own, oldest first, and says so; it never pushes out a message held for another
    /// station, which would change what a later QUERY MSGS is answered. And my mail does not
    /// expire at 48 h, nor does a message held for one station: JS8Call keeps each until the
    /// operator deletes it.
    #[test]
    fn my_mail_is_bounded_apart_from_mail_held_for_others_and_does_not_expire() {
        let mut s = Station::new(cfg());
        s.on_event(
            &directed(
                "W1AW",
                "KD9TAW",
                Some(Command::MsgTo),
                None,
                "K1ABC HELD FOR YOU",
                -5,
            ),
            1_000,
        );
        let mut said = false;
        for i in 0..150u32 {
            let acts = s.on_event(
                &directed(
                    "N0XYZ",
                    "KD9TAW",
                    Some(Command::Msg),
                    None,
                    &format!("HI {i}"),
                    -5,
                ),
                2_000 + u64::from(i),
            );
            said |= acts
                .iter()
                .any(|a| matches!(a, StationAction::Toast { text, .. } if text.contains("to you")));
        }
        let count =
            |s: &Station, st: InboxState| s.inbox().iter().filter(|e| e.state == st).count();
        assert_eq!(
            count(&s, InboxState::Store),
            1,
            "a flood of MSGs to me pushed out the message held for K1ABC"
        );
        assert_eq!(
            count(&s, InboxState::Unread),
            100,
            "my mail is capped at 100"
        );
        assert!(said, "my mail's eviction was never said");
        assert!(
            s.inbox().iter().any(|e| e.text == "HI 149")
                && !s.inbox().iter().any(|e| e.text == "HI 49"),
            "the oldest went first"
        );
        // Every entry is now over 48 h old. Neither half expires: my mail, and the message held for
        // the one station K1ABC, which JS8Call keeps until it is deleted.
        s.tick(10_000 + 48 * 60 * 60 * 1000);
        assert_eq!(
            count(&s, InboxState::Store),
            1,
            "a message held for one station does not expire at 48 h"
        );
        assert_eq!(
            count(&s, InboxState::Unread),
            100,
            "my mail does not expire"
        );
    }

    #[test]
    fn heard_is_bounded_and_normal_population_is_kept() {
        // positive control: a small band is fully retained.
        let mut s = Station::new(cfg());
        for i in 0..50u32 {
            s.on_event(&heartbeat(&format!("N{i:03}AA"), -10), i as u64 * 1000);
        }
        assert_eq!(s.heard_len(), 50);
        // flood distinct callsigns → capped, least-recently-heard evicted.
        for i in 0..5000u32 {
            s.on_event(
                &heartbeat(&format!("K{i:04}Z"), -10),
                100000 + i as u64 * 1000,
            );
        }
        assert!(
            s.heard_len() <= 500,
            "heard grew unbounded: {}",
            s.heard_len()
        );
    }

    #[test]
    fn allcall_replied_self_prunes_past_the_interval() {
        // A station answered on @ALLCALL (its QUERY MSGS, with a message waiting) is remembered.
        let answer = |s: &mut Station, call: &str, t: u64| {
            store_for(s, call, t);
            s.on_event(
                &directed(call, "@ALLCALL", Some(Command::QueryMsgs), None, "", -7),
                t,
            );
        };
        let mut s = Station::new(cfg());
        // positive control: a few distinct stations inside the interval are all remembered.
        for i in 0..5u32 {
            answer(&mut s, &format!("K{i}XYZ"), i as u64 * 1000);
        }
        assert_eq!(s.allcall_len(), 5);
        // flood distinct callsigns spread over long gaps → entries older than the 15-min interval
        // are dropped, so the map never grows one-per-station-ever.
        for i in 0..5000u32 {
            answer(&mut s, &format!("W{i:04}"), 1_000_000 + i as u64 * 1000);
        }
        assert!(
            s.allcall_len() <= 16 * 60 + 5,
            "allcall map unbounded: {}",
            s.allcall_len()
        );
    }

    /// Both ways a reply waits are bounded under a flood of queries to me, which @ALLCALL's
    /// hold-off does not limit: asking (the questions, `MAX_PENDING`) and, with the confirmation
    /// off, in JS8Call's message queue (`MAX_PENDING`; the outbox below it, `MAX_OUTBOX`). A
    /// refusal is said, never silent.
    #[test]
    fn pending_and_outbox_refuse_under_a_direct_to_me_flood() {
        for asks in [true, false] {
            let mut s = Station::new(StationConfig {
                autoreply_confirmation: asks,
                ..cfg()
            });
            let query = |i: &str| directed(i, "KD9TAW", Some(Command::SnrQuery), None, "", -7);
            // positive control: a few direct queries all wait.
            for i in 0..5u32 {
                s.on_event(&query(&format!("K{i}ABC")), i as u64);
            }
            assert_eq!(waiting(&s).len(), 5, "asks: {asks}");
            let mut refused = false;
            for i in 0..5000u32 {
                let acts = s.on_event(&query(&format!("N{i:04}")), 100 + i as u64);
                if acts.iter().any(|a| matches!(a, StationAction::Toast { text, .. } if text.contains("Reply queue full"))) {
                    refused = true;
                }
            }
            assert!(
                refused,
                "asks: {asks}: the flood exceeded the cap but no refusal was surfaced"
            );
            assert!(
                s.pending_len() <= MAX_PENDING && s.queued.len() <= MAX_PENDING,
                "pending unbounded: {}",
                s.pending_len()
            );
            assert!(
                s.outbox_len() <= MAX_OUTBOX,
                "outbox unbounded: {}",
                s.outbox_len()
            );
            // drain does not let the outbox grow past its cap either.
            let mut rng = || 0u32;
            for k in 0..200u64 {
                let _ = s.next_frame(1_000_000 + k * 15000, &|_| false, &mut rng);
            }
            assert!(
                s.outbox_len() <= MAX_OUTBOX,
                "outbox unbounded: {}",
                s.outbox_len()
            );
        }
    }

    #[test]
    fn air_sourced_strings_and_relay_depth_are_clamped() {
        // a giant MSG TO: body is truncated to the text cap before storage.
        let mut s = Station::new(cfg());
        s.on_event(
            &directed(
                "W1AW",
                "KD9TAW",
                Some(Command::MsgTo),
                None,
                &format!("K1ABC {}", "A".repeat(10_000)),
                -5,
            ),
            0,
        );
        assert!(
            s.inbox()[0].text.len() <= 512,
            "stored text not clamped: {}",
            s.inbox()[0].text.len()
        );
        // a relay chain over the hop cap is refused, not relayed forever.
        let mut s2 = Station::new(cfg());
        let long_chain = (0..50)
            .map(|i| format!("K{i}AA"))
            .collect::<Vec<_>>()
            .join(">");
        let acts = s2.on_event(
            &directed(&long_chain, "KD9TAW", Some(Command::Relay), None, "HI", -5),
            0,
        );
        assert!(acts.iter().any(
            |a| matches!(a, StationAction::Toast { text, .. } if text.contains("Relay chain"))
        ));
        assert!(
            !acts
                .iter()
                .any(|a| matches!(a, StationAction::Relayed { .. })),
            "an over-long chain was still relayed"
        );
        // a short chain still relays (positive control).
        let mut s3 = Station::new(cfg());
        let acts = s3.on_event(
            &directed("W1AW>N0XYZ", "KD9TAW", Some(Command::Relay), None, "HI", -5),
            0,
        );
        assert!(acts
            .iter()
            .any(|a| matches!(a, StationAction::Relayed { .. })));
    }

    /// The STATUS? reply's text when STATUS is not set: JS8Call's default status, "IDLE <MYIDLE>
    /// VERSION <MYVERSION>" (Configuration.cpp:1858). <MYIDLE> is `since()` of the operator's
    /// last activity, upper-cased, "NOW" read as "0M" (mainwindow.cpp:7018-7019, :121-130), and
    /// the idle count is whole minutes since the operator last acted (:10969-10979).
    #[test]
    fn a_status_reply_says_how_long_the_operator_has_been_idle() {
        let t0 = 3_600_000_000; // the operator's last act
        for (idle_ms, want) in [
            (30_000, "IDLE 0M"),
            (5 * 60_000 + 30_000, "IDLE 5M"),
            (95 * 60_000, "IDLE 1H"),
            (49 * 3_600_000, "IDLE 2D"),
        ] {
            let mut s = Station::new(cfg()); // STATUS not set
            s.mark_active(t0);
            let at = t0 + idle_ms;
            s.on_event(
                &directed("W1AW", "KD9TAW", Some(Command::StatusQuery), None, "", -7),
                at,
            );
            assert_eq!(
                drain(&mut s, at + 5_000).map(|f| f.display),
                Some(format!("KD9TAW: W1AW STATUS {want} VERSION Nexus")),
                "{idle_ms} ms idle"
            );
        }
    }

    /// The idle count is whole minutes since the last operator act, and 0 before the station has
    /// a baseline (a fresh station, before the operator enters JS8), as JS8Call's starts at 0.
    #[test]
    fn the_idle_count_is_whole_minutes_since_the_last_act() {
        let mut s = Station::new(cfg());
        assert_eq!(s.idle_minutes(3_600_000_000), 0, "no baseline yet");
        s.mark_active(3_600_000_000);
        assert_eq!(s.idle_minutes(3_600_000_000 + 59_999), 0, "under a minute");
        assert_eq!(
            s.idle_minutes(3_600_000_000 + 7 * 60_000),
            7,
            "seven minutes"
        );
    }

    /// `call` heard in a heartbeat at `t`, so the heard list knows when.
    fn heard_at(s: &mut Station, call: &str, t: u64) {
        let MessageEvent::Message(mut m) = heartbeat(call, -10) else {
            unreachable!("heartbeat() builds a message");
        };
        m.first_ms = t;
        m.last_ms = t;
        s.on_event(&MessageEvent::Message(m), t);
    }

    /// The HEARING? reply `querier` draws at `t`.
    fn hearing_reply(s: &mut Station, querier: &str, t: u64) -> Option<String> {
        let MessageEvent::Message(mut m) =
            directed(querier, "KD9TAW", Some(Command::HearingQuery), None, "", -7)
        else {
            unreachable!("directed() builds a message");
        };
        m.first_ms = t;
        m.last_ms = t;
        s.on_event(&MessageEvent::Message(m), t);
        drain(s, t + 5_000).map(|f| f.display)
    }

    /// JS8Call's HEARING? reply (mainwindow.cpp:8868-8905): "<FROM> HEARING" (:8903) and up to four
    /// calls, newest first (:8873-8883), never the station that asked (:8890).
    #[test]
    fn a_hearing_reply_names_the_newest_four_but_never_the_asker() {
        let mut s = Station::new(cfg());
        for (i, call) in ["K1ABC", "N0XYZ", "W1AW", "K2DEF", "K3GHI", "K4JKL"]
            .iter()
            .enumerate()
        {
            heard_at(&mut s, call, 10_000 + i as u64 * 1_000);
        }
        assert_eq!(
            hearing_reply(&mut s, "W1AW", 60_000).as_deref(),
            Some("KD9TAW: W1AW HEARING K4JKL K3GHI K2DEF N0XYZ"),
            "the asker is skipped and takes no place in the four"
        );
        let mut s = Station::new(cfg());
        assert_eq!(
            hearing_reply(&mut s, "W1AW", 60_000).as_deref(),
            Some("KD9TAW: W1AW HEARING"),
            "with nobody else heard, the reply names nobody"
        );
    }

    /// JS8Call's callsign aging (mainwindow.cpp:8894): with it set, a call last heard that many
    /// whole minutes ago or more takes no place in the HEARING? reply. Off (0, its default,
    /// Configuration.cpp:1853), nothing ages.
    #[test]
    fn a_hearing_reply_leaves_out_calls_older_than_the_callsign_aging() {
        let aging = |min: u16| {
            Station::new(StationConfig {
                callsign_aging_min: min,
                ..cfg()
            })
        };
        let t = 20 * 60_000;
        let mut s = aging(10);
        heard_at(&mut s, "K1ABC", t - 10 * 60_000);
        heard_at(&mut s, "N0XYZ", t - 10 * 60_000 + 1_000);
        heard_at(&mut s, "K2DEF", t - 60_000);
        assert_eq!(
            hearing_reply(&mut s, "W1AW", t).as_deref(),
            Some("KD9TAW: W1AW HEARING K2DEF N0XYZ"),
            "heard ten whole minutes ago is aged out, 9:59 ago is not"
        );
        let mut s = aging(0);
        heard_at(&mut s, "K1ABC", 0);
        heard_at(&mut s, "K2DEF", t - 60_000);
        assert_eq!(
            hearing_reply(&mut s, "W1AW", t).as_deref(),
            Some("KD9TAW: W1AW HEARING K2DEF K1ABC"),
            "off, nothing ages"
        );
    }

    /// JS8Call saves its call activity without the calls its callsign aging has passed
    /// (mainwindow.cpp:2013-2023), keeping them in memory, so they do not come back after a
    /// restart. The journal's snapshot leaves them out the same way; with the aging off it keeps
    /// every call.
    #[test]
    fn the_journal_leaves_out_calls_older_than_the_callsign_aging() {
        let t = 20 * 60_000;
        let mut s = Station::new(StationConfig {
            callsign_aging_min: 10,
            ..cfg()
        });
        heard_at(&mut s, "K1ABC", t - 10 * 60_000);
        heard_at(&mut s, "K2DEF", t - 60_000);
        let calls = |snap: StationSnapshot| -> Vec<String> {
            snap.heard.into_iter().map(|h| h.call).collect()
        };
        assert_eq!(
            calls(s.snapshot(t)),
            ["K2DEF"],
            "the aged call is not saved"
        );
        assert_eq!(s.heard().len(), 2, "and it is still heard, as in JS8Call");
        s.set_config(cfg());
        assert_eq!(
            calls(s.snapshot(t)),
            ["K1ABC", "K2DEF"],
            "off, every call is saved"
        );
    }

    // ===== (E) JS8Call's AutoreplyConfirmation: each automatic reply asks Yes/No first =====

    /// JS8Call ships the confirmation on (`AutoreplyConfirmation`, default true,
    /// Configuration.cpp:1949).
    #[test]
    fn replies_ask_for_confirmation_by_default_as_js8call_ships_it() {
        assert!(StationConfig::default().autoreply_confirmation);
    }

    /// ⭐ Each automatic reply asks first (`confirmThenEnqueueMessage`, mainwindow.cpp:9385-9389):
    /// nothing keys while it waits, however many periods pass, and the box says No by itself 89 s
    /// after it opened. The operator's Yes releases it, and it keys in the period after the Yes.
    #[test]
    fn an_automatic_reply_waits_for_yes_and_keys_after_it() {
        let mut s = Station::new(asking_cfg());
        s.on_event(&snr_query("W1AW"), 1000);
        let p = s.pending_reply().expect("the reply asks");
        assert_eq!(
            (p.display.as_str(), p.fires_at_ms),
            ("KD9TAW: W1AW SNR -07", 1000 + 89_000),
            "JS8Call's reply, No 89 s after it was made"
        );
        assert_eq!(
            keyed_in(&mut s, 15_000, 5),
            Vec::<String>::new(),
            "nothing keys while it asks"
        );
        assert!(
            s.answer_reply(&p.display, p.fires_at_ms, true, 80_000)
                .is_some(),
            "Yes"
        );
        assert!(s.pending_reply().is_none(), "answered");
        assert_eq!(
            keyed_in(&mut s, 90_000, 4),
            ["KD9TAW: W1AW SNR -07"],
            "Yes: it keys once, in the next period"
        );
    }

    /// No, or no answer for 89 s, sends nothing: JS8Call's box closes on No either way
    /// (mainwindow.cpp:5219-5226; SelfDestructMessageBox.cpp:68). A Yes after that is refused.
    #[test]
    fn no_or_no_answer_for_89_seconds_sends_nothing() {
        let mut s = Station::new(asking_cfg());
        s.on_event(&snr_query("W1AW"), 1000);
        let p = s.pending_reply().expect("asks");
        assert!(s
            .answer_reply(&p.display, p.fires_at_ms, false, 2000)
            .is_some());
        assert!(s.pending_reply().is_none(), "No: answered");
        assert!(keyed_in(&mut s, 15_000, 8).is_empty(), "No: nothing keys");

        let mut s = Station::new(asking_cfg());
        s.on_event(&snr_query("W1AW"), 1000);
        let p = s.pending_reply().expect("asks");
        s.tick(1000 + 89_000 - 1);
        assert!(s.pending_reply().is_some(), "still asking a moment before");
        s.tick(1000 + 89_000);
        assert!(s.pending_reply().is_none(), "No by itself at 89 s");
        assert!(
            s.answer_reply(&p.display, p.fires_at_ms, true, 91_000)
                .is_none(),
            "a Yes after that answers nothing"
        );
        assert!(keyed_in(&mut s, 15_000, 8).is_empty(), "nothing keys");

        // Only a Yes releases it: a period past its time, even with no tick to close it first.
        let mut s = Station::new(asking_cfg());
        s.on_event(&snr_query("W1AW"), 1000);
        assert!(
            keyed_in(&mut s, 120_000, 4).is_empty(),
            "an unanswered reply never keys, even past its time"
        );
    }

    /// Off, a reply goes by itself, JS8Call's `enqueueMessage` path (mainwindow.cpp:9388): it is
    /// never a question.
    #[test]
    fn with_confirmation_off_a_reply_goes_by_itself() {
        let mut s = Station::new(cfg());
        s.on_event(&snr_query("W1AW"), 1000);
        assert_eq!(
            keyed_in(&mut s, 5000, 4),
            ["KD9TAW: W1AW SNR -07"],
            "keyed with no Yes"
        );
    }

    /// Every automatic reply asks, not the queries alone: the MSG ACK, the stored MSG TO:'s ACK,
    /// the relay, QUERY MSGS and the HB-ACK (`sendHeartbeatAck`'s own box, mainwindow.cpp:6301-6307).
    #[test]
    fn every_automatic_reply_asks() {
        let events = [
            directed("W1AW", "KD9TAW", Some(Command::Msg), None, "HELLO", -7),
            directed(
                "W1AW",
                "KD9TAW",
                Some(Command::MsgTo),
                None,
                "K1ABC FRIDAY",
                -7,
            ),
            directed(
                "W1AW",
                "KD9TAW",
                Some(Command::Relay),
                None,
                "K1ABC> HI",
                -7,
            ),
            directed("W1AW", "KD9TAW", Some(Command::QueryMsgs), None, "", -7),
            heartbeat("W1AW", -7),
        ];
        for ev in &events {
            let mut s = Station::new(StationConfig {
                hb_ack: true,
                ..asking_cfg()
            });
            s.set_hb(true, 0);
            s.hb_next_ms = None; // no heartbeat of our own in the window
            s.on_event(ev, 1000);
            let p = s.pending_reply().expect("it asks");
            assert_eq!(p.fires_at_ms, 1000 + 89_000, "{}", p.display);
            assert!(
                keyed_in(&mut s, 15_000, 5).is_empty(),
                "{}: nothing keys without a Yes",
                p.display
            );
        }
    }

    /// A `QUERY MSG n` hands the held message over on Yes alone: JS8Call marks it delivered in
    /// the reply's callback (mainwindow.cpp:9227-9235), so a No leaves it held, and the station
    /// still offers it.
    #[test]
    fn a_held_message_is_handed_over_on_yes_only() {
        let mut s = Station::new(asking_cfg());
        store_for(&mut s, "N0XYZ", 0);
        let id = s.inbox()[0].id;
        let ask = |s: &mut Station, t: u64| {
            s.on_event(
                &directed(
                    "N0XYZ",
                    "KD9TAW",
                    Some(Command::Query),
                    None,
                    &format!("MSG {id}"),
                    -5,
                ),
                t,
            );
        };
        ask(&mut s, 1000);
        let p = s.pending_reply().expect("the delivery asks");
        assert_eq!(p.display, "KD9TAW: N0XYZ MSG FRIDAY CONTACT FROM W1AW");
        assert_eq!(s.inbox()[0].state, InboxState::Store, "held while it asks");
        s.answer_reply(&p.display, p.fires_at_ms, false, 2000);
        assert_eq!(s.inbox()[0].state, InboxState::Store, "No: still held");
        ask(&mut s, 3000);
        let p = s.pending_reply().expect("asked again, it asks again");
        let actions = s
            .answer_reply(&p.display, p.fires_at_ms, true, 4000)
            .expect("Yes");
        assert_eq!(
            s.inbox()[0].state,
            InboxState::Delivered,
            "Yes: handed over"
        );
        assert!(actions.contains(&StationAction::InboxChanged), "…journaled");
        let first_frames: Vec<String> = (0..6u64)
            .filter_map(|k| drain(&mut s, 15_000 + k * 15_000))
            .filter(|f| f.first)
            .map(|f| f.display)
            .collect();
        assert_eq!(
            first_frames,
            ["KD9TAW: N0XYZ MSG FRIDAY CONTACT FROM W1AW"],
            "the delivery keys once"
        );
    }

    /// The operator's answer is their act: JS8Call resets its idle count on any click
    /// (`eventFilter` → `resetIdleTimer`, mainwindow.cpp:2979-2988), the Yes and the No included.
    #[test]
    fn answering_a_reply_restarts_the_idle_count() {
        for yes in [true, false] {
            let mut s = Station::new(asking_cfg());
            s.mark_active(1);
            s.on_event(&snr_query("W1AW"), 10 * 60_000 + 1);
            let p = s.pending_reply().expect("asks");
            assert_eq!(
                s.idle_minutes(10 * 60_000 + 1),
                10,
                "control: ten idle minutes"
            );
            s.answer_reply(&p.display, p.fires_at_ms, yes, 10 * 60_000 + 1);
            assert_eq!(s.idle_minutes(10 * 60_000 + 30_000), 0, "yes={yes}");
        }
    }

    /// Stop TX (`halt`) and the idle watchdog drop a reply still asking, as they drop every reply.
    #[test]
    fn stop_tx_and_the_idle_watchdog_drop_a_reply_still_asking() {
        let mut s = Station::new(asking_cfg());
        s.on_event(&snr_query("W1AW"), 1000);
        assert!(s.pending_reply().is_some(), "control: it asks");
        s.halt();
        assert!(s.pending_reply().is_none(), "Stop TX drops it");

        let mut s = Station::new(StationConfig {
            idle_watchdog_min: 5,
            ..asking_cfg()
        });
        s.mark_active(0);
        s.on_event(&snr_query("W1AW"), 60_000);
        assert!(s.pending_reply().is_some(), "control: it asks");
        s.tick(5 * 60_000);
        assert!(s.idle_tripped(), "control: tripped");
        assert!(s.pending_reply().is_none(), "the trip drops it");
    }

    // ===== (C) JS8Call's cadence: a reply keys in the next period =====

    /// ⭐ JS8Call's schedule for an automatic reply, measured. The decoder takes a cycle once
    /// `framesForSymbols` of it are in (`decodeEnqueueReadyExperiment`, mainwindow.cpp:3472-3476),
    /// `frames_needed` here; the 1 Hz `guiUpdate` then runs `processCommandActivity`, which
    /// queues the reply (`enqueueMessage`, :9388), and `processTxQueue` in the same pass
    /// (:4734-4737), which puts it in the compose box and starts TX (:9671-9685). That moment is
    /// past the cycle's transmit window (`m_bTxTime`, :4441-4447), so the reply keys at the next
    /// period's start (:4510-4521): the period right after the one the query was in. With the
    /// confirmation off Nexus keys it there too, at every speed. (The golden log agrees: twelve
    /// Normal ACKs one period after the MSG, a Fast one in the next Fast period.)
    #[test]
    fn with_confirmation_off_a_reply_keys_in_the_period_js8call_keys_it() {
        for speed in Speed::ALL {
            let period = u64::from(speed.period_s()) * 1000;
            let decoded = 100 * period + (speed.frames_needed() as u64 * 1000) / 12_000;
            let mut s = Station::new(StationConfig {
                mycall: "KD9TAW".into(),
                grid: "EN52".into(),
                speed,
                autoreply_confirmation: false,
                ..Default::default()
            });
            s.on_event(&snr_query("W1AW"), decoded);
            let keyed: Vec<u64> = (101..106u64)
                .filter(|k| drain(&mut s, k * period).is_some())
                .collect();
            assert_eq!(keyed, [101], "{speed:?}: the period after the query, once");
        }
    }

    /// With the confirmation off a reply is queued as it is made: in view in the queue at once,
    /// never a countdown, and Drop queue or Stop TX take it back before its period.
    #[test]
    fn with_confirmation_off_a_reply_is_queued_at_once_and_droppable() {
        let mut s = Station::new(cfg());
        s.on_event(&snr_query("W1AW"), 1000);
        assert!(s.pending_reply().is_none(), "nothing counts down");
        s.process_tx_queue(); // within the second, as JS8Call's processTxQueue
        let q: Vec<String> = s.queue().into_iter().map(|f| f.display).collect();
        assert_eq!(q, ["KD9TAW: W1AW SNR -07"], "queued at once");
        s.drop_queue();
        assert!(
            keyed_in(&mut s, 15_000, 4).is_empty(),
            "Drop queue: nothing keys"
        );
        s.on_event(&snr_query("W1AW"), 20_000);
        s.halt();
        assert!(
            keyed_in(&mut s, 30_000, 4).is_empty(),
            "Stop TX: nothing keys"
        );
    }

    /// A reply still asking holds back nothing else: a heartbeat or a repeating CQ due meanwhile
    /// keys on time, as JS8Call's question box holds nothing back (mainwindow.cpp:5227-5229).
    #[test]
    fn a_reply_still_asking_holds_back_no_heartbeat_or_cq() {
        let mut s = Station::new(StationConfig {
            hb_interval_min: 5,
            ..asking_cfg()
        });
        s.on_event(&snr_query("K1ABC"), 500);
        s.set_hb(true, 1000);
        let hb_due = s.hb_next_ms().expect("scheduled");
        assert!(
            s.pending_reply().is_some(),
            "control: a reply asks at the heartbeat"
        );
        let boundary = (hb_due - 1000) / 15_000 * 15_000;
        let f = drain(&mut s, boundary).expect("the heartbeat keys on time");
        assert_eq!(f.origin, Origin::Heartbeat);

        let mut s = Station::new(StationConfig {
            cq_interval_min: 1,
            ..asking_cfg()
        });
        s.on_event(&snr_query("W1AW"), 500); // before the CQ: traffic to me stops a repeat
        s.set_cq(true, 1000);
        assert!(
            s.pending_reply().is_some(),
            "control: a reply asks at the CQ"
        );
        s.tick(61_000);
        let f = drain(&mut s, 75_000).expect("the CQ keys on time");
        assert_eq!(f.origin, Origin::CqRepeat);
    }

    // ===== (D) AUTO off: the reply goes to the compose box, never keyed by itself =====

    fn auto_off() -> StationConfig {
        StationConfig {
            autoreply: false,
            ..cfg()
        }
    }

    /// The reply the composer is offered now, if any (taking nothing).
    fn offered(s: &mut Station) -> Option<String> {
        s.sync_composer(false, None).1.map(|c| c.1)
    }

    /// ⭐ With AUTO off a query to me is still answered as JS8Call answers it (no branch of
    /// processCommandActivity's SNR?/GRID?/INFO?/STATUS?/HEARING? asks AUTO, mainwindow.cpp:
    /// 8834-8903), and `processTxQueue` puts the reply in the compose box (`addMessageText`,
    /// :9671) without starting TX (:9674-9685): it is in the composer, and nothing keys.
    #[test]
    fn with_auto_off_a_reply_is_put_in_the_composer_and_never_keyed() {
        let mut s = Station::new(auto_off());
        s.on_event(&snr_query("W1AW"), 1000);
        assert_eq!(
            offered(&mut s).as_deref(),
            Some("W1AW SNR -07"),
            "in the composer"
        );
        assert!(
            keyed_in(&mut s, 15_000, 8).is_empty(),
            "never keyed by itself"
        );
        assert!(s.queue().is_empty() && s.pending_reply().is_none());
    }

    /// Nothing is put in the box while something is going out (`addMessageText` adds nothing
    /// while a transmission is queued, :5170-5172; the frame queue must be empty, :9652-9654):
    /// a reply queued before the operator's message waits for it to go.
    #[test]
    fn the_composer_waits_while_something_is_going_out() {
        let mut s = Station::new(auto_off());
        s.on_event(&snr_query("W1AW"), 1000);
        s.send(None, "CQ TEST", 1000)
            .expect("an operator message on its way");
        assert!(
            offered(&mut s).is_none(),
            "the operator's message goes first"
        );
        let mut t = 15_000;
        while !s.queue().is_empty() {
            assert!(
                offered(&mut s).is_none(),
                "nothing while it is still going out"
            );
            drain(&mut s, t);
            t += 15_000;
        }
        s.tick(t);
        assert_eq!(
            offered(&mut s).as_deref(),
            Some("W1AW SNR -07"),
            "then the reply"
        );
    }
    /// AUTO off with the confirmation on: JS8Call still asks (the box is opened before AUTO is
    /// read, :9385-9389), and the Yes puts the reply in the compose box, not on the air.
    #[test]
    fn with_auto_off_a_confirmed_reply_goes_to_the_composer() {
        let mut s = Station::new(StationConfig {
            autoreply: false,
            ..asking_cfg()
        });
        s.on_event(&snr_query("W1AW"), 1000);
        let p = s.pending_reply().expect("it asks");
        assert!(
            offered(&mut s).is_none(),
            "nothing in the composer before the Yes"
        );
        s.answer_reply(&p.display, p.fires_at_ms, true, 2000);
        assert_eq!(
            offered(&mut s).as_deref(),
            Some("W1AW SNR -07"),
            "Yes: the composer"
        );
        assert!(
            keyed_in(&mut s, 15_000, 8).is_empty(),
            "never keyed by itself"
        );
    }

    /// What JS8Call answers only with AUTO checked draws nothing with it off: QUERY MSGS
    /// (`d.cmd == " QUERY MSGS" && actionModeAutoreply`, :9252) and the heartbeat
    /// acknowledgement (:9059). A relay is made and goes to the composer: JS8Call keys it only
    /// with AUTO checked (:9674-9685).
    #[test]
    fn with_auto_off_query_msgs_and_hb_acks_draw_nothing_and_a_relay_goes_to_the_composer() {
        let mut s = Station::new(StationConfig {
            hb_ack: true,
            ..auto_off()
        });
        s.set_hb(true, 0);
        s.hb_next_ms = None;
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::QueryMsgs), None, "", -5),
            1000,
        );
        s.on_event(&heartbeat("K1ABC", -5), 1000);
        assert!(
            offered(&mut s).is_none(),
            "QUERY MSGS and the heartbeat: nothing"
        );
        assert!(keyed_in(&mut s, 15_000, 4).is_empty());
        s.on_event(
            &directed(
                "W1AW",
                "KD9TAW",
                Some(Command::Relay),
                None,
                "K1ABC> HI",
                -5,
            ),
            70_000,
        );
        assert_eq!(
            offered(&mut s).as_deref(),
            Some("K1ABC> HI *DE* KD9TAW"),
            "the relay waits in the composer"
        );
        assert!(
            keyed_in(&mut s, 75_000, 4).is_empty(),
            "never keyed by itself"
        );
    }

    /// A `QUERY MSG n` with AUTO off hands the held message over as its reply reaches the box,
    /// as the callback runs once the reply is dequeued (:9227-9235, :9687-9689): not while it
    /// waits for the box.
    #[test]
    fn with_auto_off_a_delivery_hands_the_message_over_as_it_reaches_the_composer() {
        let mut s = Station::new(auto_off());
        store_for(&mut s, "N0XYZ", 0);
        let id = s.inbox()[0].id;
        s.on_event(
            &directed(
                "N0XYZ",
                "KD9TAW",
                Some(Command::Query),
                None,
                &format!("MSG {id}"),
                -5,
            ),
            1000,
        );
        assert!(
            s.sync_composer(true, None).1.is_none(),
            "the operator typing: it waits"
        );
        assert_eq!(s.inbox()[0].state, InboxState::Store, "held while it waits");
        let (_, text) = s
            .sync_composer(false, None)
            .1
            .expect("box empty: the delivery");
        assert_eq!(text, "N0XYZ MSG FRIDAY CONTACT FROM W1AW");
        assert_eq!(
            s.inbox()[0].state,
            InboxState::Delivered,
            "handed over as it got there"
        );
    }
    /// Stop TX, Drop queue and the idle watchdog drop the replies waiting for the box, and the one
    /// offered to it.
    #[test]
    fn stop_tx_drop_queue_and_the_idle_watchdog_clear_the_composer_queue() {
        for how in ["halt", "drop", "trip"] {
            let mut s = Station::new(StationConfig {
                idle_watchdog_min: 5,
                ..auto_off()
            });
            s.mark_active(1);
            s.on_event(&snr_query("W1AW"), 1000);
            s.on_event(&snr_query("K1ABC"), 1000);
            assert!(offered(&mut s).is_some(), "{how}: control");
            match how {
                "halt" => s.halt(),
                "drop" => s.drop_queue(),
                _ => {
                    s.tick(6 * 60_000);
                    assert!(s.idle_tripped(), "control: tripped");
                }
            }
            assert!(
                offered(&mut s).is_none(),
                "{how}: nothing offered or waiting"
            );
        }
    }

    /// A heartbeat acknowledgement is keyed even with AUTO off, as JS8Call's `processTxQueue`
    /// keys a message holding " HEARTBEAT " whatever AUTO says (:9674-9685): one asked about with
    /// AUTO on, and confirmed after AUTO went off, goes on the air, not into the box.
    #[test]
    fn a_heartbeat_ack_confirmed_after_auto_went_off_still_keys_as_in_js8call() {
        let mut s = Station::new(StationConfig {
            hb_ack: true,
            ..asking_cfg()
        });
        s.set_hb(true, 0);
        s.hb_next_ms = None;
        s.on_event(&heartbeat("W1AW", -5), 1000);
        let p = s.pending_reply().expect("the HB-ACK asks");
        s.set_config(StationConfig {
            hb_ack: true,
            autoreply: false,
            ..asking_cfg()
        });
        s.answer_reply(&p.display, p.fires_at_ms, true, 2000);
        assert!(offered(&mut s).is_none(), "not in the box");
        let f = drain(&mut s, 15_000).expect("keyed");
        assert_eq!(f.origin, Origin::HbAck);
    }

    // ===== (F) JS8Call's reply-skipping rules, its lists and its filing differences =====

    /// Everything a station does for a command, in one look: a reply waiting (asked, queued or on
    /// its way), the inbox size, and what the composer is offered.
    fn acted(s: &mut Station) -> (Vec<String>, usize) {
        s.process_tx_queue();
        (waiting(s), s.inbox().len())
    }

    /// JS8Call's `Radio::base_callsign` (Radio.cpp:117-133), which its lists and its "to me"
    /// test match by: the longer side of the first `/`, the right one on a tie.
    #[test]
    fn the_base_call_is_js8calls() {
        for (call, base) in [
            ("W1AW", "W1AW"),
            ("W1AW/P", "W1AW"),
            ("VE3/W1AW", "W1AW"),
            ("KH6/W1AW/P", "W1AW/P"),
            ("w1aw/p", "W1AW"),
            ("AB/CD", "CD"), // a tie takes the right side
        ] {
            assert_eq!(radio_base_callsign(call), base, "{call}");
        }
    }

    /// ⭐ Text in the compose box holds replies: JS8Call queues no reply while its box holds text
    /// (`do not queue for reply if there's text in the window`, mainwindow.cpp:9364-9367): the
    /// operator typing, a reply put there and not yet sent, or a message going out (the box holds
    /// it until its last frame is sent). Such a reply is not made at all, not even asked about;
    /// once the box is free the next query is answered.
    #[test]
    fn no_reply_is_made_while_the_compose_box_holds_text() {
        for busy in ["typing", "a reply in the box", "a message going out"] {
            let mut s = Station::new(StationConfig {
                autoreply: busy != "a reply in the box",
                ..asking_cfg()
            });
            match busy {
                "typing" => {
                    s.sync_composer(true, None);
                }
                "a reply in the box" => {
                    s.set_config(StationConfig {
                        autoreply: false,
                        ..cfg()
                    });
                    s.on_event(&snr_query("N0XYZ"), 500);
                    assert!(offered(&mut s).is_some(), "control: a reply in the box");
                    s.set_config(asking_cfg());
                }
                _ => {
                    s.send(None, "CQ TEST", 500).expect("on its way");
                }
            }
            let before = acted(&mut s);
            s.on_event(&snr_query("W1AW"), 1000);
            assert_eq!(
                acted(&mut s),
                before,
                "{busy}: no reply made, nothing asked"
            );
            assert!(s.pending_reply().is_none(), "{busy}: not asked");
        }
        let mut s = Station::new(asking_cfg());
        s.sync_composer(true, None);
        s.sync_composer(false, None);
        s.on_event(&snr_query("W1AW"), 1000);
        assert!(s.pending_reply().is_some(), "the box free again: answered");
    }

    /// …and a reply already queued (confirmed, or made before) waits while the box holds text, as
    /// `processTxQueue` waits (:9656-9659), going once it is free.
    #[test]
    fn a_queued_reply_waits_while_the_box_holds_text() {
        let mut s = Station::new(asking_cfg());
        s.on_event(&snr_query("W1AW"), 1000);
        let p = s.pending_reply().expect("asks");
        s.sync_composer(true, None); // the operator starts typing
        s.answer_reply(&p.display, p.fires_at_ms, true, 2000);
        assert!(
            keyed_in(&mut s, 15_000, 3).is_empty(),
            "held while the box holds text"
        );
        s.sync_composer(false, None);
        assert_eq!(
            keyed_in(&mut s, 60_000, 2),
            ["KD9TAW: W1AW SNR -07"],
            "then it goes"
        );
    }

    /// ⭐ A message to me still arriving holds replies: JS8Call queues none while a buffer whose
    /// directed command is to me is open (`hasExistingMessageBufferToMe`, mainwindow.cpp:
    /// 4176-4190, :9369-9374). One to another station holds nothing.
    #[test]
    fn no_reply_is_made_while_a_message_to_me_is_still_arriving() {
        let mut s = Station::new(asking_cfg());
        s.note_rx_buffers(true, ["KD9TAW"]);
        s.on_event(&snr_query("W1AW"), 1000);
        assert!(
            s.pending_reply().is_none(),
            "a message to me arriving: no reply"
        );
        s.note_rx_buffers(true, ["K1ABC"]);
        s.on_event(&snr_query("W1AW"), 2000);
        assert!(
            s.pending_reply().is_some(),
            "one to another station: answered"
        );
    }

    /// The heartbeat acknowledgement keeps JS8Call's own two rules (mainwindow.cpp:9062-9077): no
    /// message still arriving, any at all, and not a station the heartbeat deny list names. It is
    /// not held by my own queue (its path, `sendHeartbeatAck`, checks no box): made while my
    /// message goes out, it waits and keys after it.
    #[test]
    fn the_hb_ack_waits_for_no_message_arriving_and_keeps_its_deny_list() {
        let hb_cfg = || StationConfig {
            hb_ack: true,
            hb_ack_deny: vec!["W1AW".into()],
            ..cfg()
        };
        let mut s = Station::new(hb_cfg());
        s.set_hb(true, 0);
        s.hb_next_ms = None;
        s.note_rx_buffers(true, ["K9ZZZ"]);
        s.on_event(&heartbeat("K1ABC", -5), 1000);
        assert!(waiting(&s).is_empty(), "a message arriving: no HB-ACK");
        s.note_rx_buffers(false, []);
        s.on_event(&heartbeat("W1AW/P", -5), 2000);
        assert!(
            waiting(&s).is_empty(),
            "the deny list: no HB-ACK (W1AW/P by its base call)"
        );
        s.send(None, "CQ TEST", 2500)
            .expect("my message on its way");
        s.on_event(&heartbeat("K1ABC", -5), 3000);
        assert_eq!(
            waiting(&s),
            ["K1ABC HEARTBEAT SNR -05"],
            "made while my message goes out"
        );
        let keyed: Vec<Origin> = (1..6u64)
            .filter_map(|k| drain(&mut s, k * 15_000))
            .map(|f| f.origin)
            .collect();
        assert_eq!(
            keyed.last(),
            Some(&Origin::HbAck),
            "it keys after my message"
        );
    }

    /// The allow list (JS8Call's "Only autoreply to these callsigns", :8792-8800): with it set,
    /// a station on it (as heard, or by its base call) is answered and anyone else is acted on in
    /// no way: no reply, no relay, nothing filed or stored. Empty lets everyone in.
    #[test]
    fn the_allow_list_answers_only_its_stations() {
        let mut s = Station::new(StationConfig {
            autoreply_allow: vec!["W1AW".into()],
            ..cfg()
        });
        s.on_event(&snr_query("W1AW/P"), 1000);
        assert_eq!(
            acted(&mut s).0,
            ["W1AW/P SNR -07"],
            "on it by its base call"
        );
        let mut s = Station::new(StationConfig {
            autoreply_allow: vec!["W1AW".into()],
            ..cfg()
        });
        for ev in [
            snr_query("K1ABC"),
            directed("K1ABC", "KD9TAW", Some(Command::Msg), None, "HELLO", -5),
            directed(
                "K1ABC",
                "KD9TAW",
                Some(Command::MsgTo),
                None,
                "N0XYZ HI",
                -5,
            ),
            directed(
                "K1ABC",
                "KD9TAW",
                Some(Command::Relay),
                None,
                "N0XYZ> HI",
                -5,
            ),
        ] {
            s.on_event(&ev, 1000);
        }
        assert_eq!(
            acted(&mut s),
            (Vec::<String>::new(), 0),
            "kept out: nothing at all"
        );
    }

    /// The deny list (JS8Call's "Never autoreply to these callsigns", :8802-8807): a station on it,
    /// as heard or by its base call, is acted on in no way, its MSG not even filed.
    #[test]
    fn the_deny_list_keeps_a_station_out_entirely() {
        let mut s = Station::new(StationConfig {
            autoreply_deny: vec!["W1AW".into()],
            ..cfg()
        });
        s.on_event(&snr_query("W1AW/P"), 1000);
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::Msg), None, "HELLO", -5),
            1000,
        );
        assert_eq!(
            acted(&mut s),
            (Vec::<String>::new(), 0),
            "denied: nothing at all"
        );
        s.on_event(&snr_query("K1ABC"), 2000);
        assert_eq!(acted(&mut s).0, ["K1ABC SNR -07"], "anyone else: answered");
    }

    /// "To me" as JS8Call tests it (`toMe`, mainwindow.cpp:8599): my call as set, a `/P` or a
    /// compound one included, or its base call. A MSG to `KD9TAW/P` is mine when I am KD9TAW/P,
    /// and so is one to `KD9TAW`; set to VE3/KD9TAW, one to KD9TAW is mine too.
    #[test]
    fn a_msg_to_my_call_as_set_or_its_base_is_mine() {
        for (mine, to) in [
            ("KD9TAW/P", "KD9TAW/P"),
            ("KD9TAW/P", "KD9TAW"),
            ("VE3/KD9TAW", "KD9TAW"),
            ("VE3/KD9TAW", "VE3/KD9TAW"),
            ("KD9TAW", "KD9TAW"),
        ] {
            let mut s = Station::new(StationConfig {
                mycall: mine.into(),
                ..cfg()
            });
            s.on_event(
                &directed("W1AW", to, Some(Command::Msg), None, "HELLO", -5),
                1000,
            );
            assert_eq!(s.inbox().len(), 1, "{mine}: a MSG to {to} is filed");
            // (Its ACK needs my call composed: compose takes a compound call but not a `/P` one
            // as MYCALL, which is outside this rule.)
            if !mine.ends_with("/P") {
                assert_eq!(acted(&mut s).0, ["W1AW ACK"], "{mine}: …and acknowledged");
            }
        }
        let mut s = Station::new(StationConfig {
            mycall: "KD9TAW/P".into(),
            ..cfg()
        });
        s.on_event(
            &directed("W1AW", "KD9TAX", Some(Command::Msg), None, "HELLO", -5),
            1000,
        );
        assert_eq!(
            acted(&mut s),
            (Vec::<String>::new(), 0),
            "control: another call is not"
        );
    }

    /// The other filing difference: nothing is filed or stored while the idle watchdog stands,
    /// as JS8Call's `if(m_tx_watchdog) continue;` (:8817-8820) comes before the MSG and MSG TO:
    /// branches. Once an operator act clears the trip, a MSG to me is filed again.
    #[test]
    fn nothing_is_filed_or_stored_while_the_idle_watchdog_stands() {
        let mut s = Station::new(StationConfig {
            idle_watchdog_min: 5,
            ..cfg()
        });
        s.mark_active(1);
        s.tick(6 * 60_000);
        assert!(s.idle_tripped(), "control: tripped");
        s.set_config(StationConfig {
            idle_watchdog_min: 5,
            ..cfg()
        });
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::Msg), None, "HELLO", -5),
            6 * 60_000,
        );
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::MsgTo), None, "K1ABC HI", -5),
            6 * 60_000,
        );
        assert_eq!(s.inbox().len(), 0, "tripped: nothing filed or stored");
        s.mark_active(7 * 60_000);
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::Msg), None, "HELLO", -5),
            7 * 60_000,
        );
        assert_eq!(s.inbox().len(), 1, "the trip cleared: filed");
    }

    // ===== heartbeat networking and presence, as JS8Call 3.0.3 has them =====

    /// What one station keys: its origin, its speed and its display.
    type Keyed = Vec<(Origin, Speed, String)>;

    fn keyed(f: Option<TxFrame>) -> Keyed {
        f.into_iter()
            .map(|f| (f.origin, f.speed, f.display))
            .collect()
    }

    /// JS8Call sends heartbeats and their acknowledgements at Slow, Normal and Fast and nowhere
    /// else (`canCurrentModeSendHeartbeat`, mainwindow.cpp:4113-4117; processCommandActivity.cpp:
    /// 636). At Turbo the HB repeat cannot be turned on, "send heartbeat now" queues nothing, and a
    /// heard heartbeat is neither answered nor asked about. By value, at each speed.
    #[test]
    fn heartbeats_and_their_acknowledgements_key_only_at_slow_normal_and_fast() {
        assert_eq!(
            Speed::ALL
                .into_iter()
                .filter(|&s| heartbeat_speed(s))
                .collect::<Vec<_>>(),
            vec![Speed::Slow, Speed::Normal, Speed::Fast],
            "the allow-list, by name"
        );
        for speed in Speed::ALL {
            let at = || StationConfig {
                speed,
                hb_ack: true,
                ..cfg()
            };
            // The HB repeat at interval 0: one heartbeat, in the next period.
            let mut s = Station::new(at());
            s.set_hb(true, 0);
            assert_eq!(
                (s.hb_on(), s.hb_next_ms().is_some()),
                (heartbeat_speed(speed), heartbeat_speed(speed)),
                "{speed:?}: the HB repeat goes on where heartbeats go out, and only there"
            );
            let repeat = keyed(drain(&mut s, 0));
            // "Send heartbeat now".
            let mut s = Station::new(at());
            s.heartbeat_now(0).expect("composes");
            let now = keyed(drain(&mut s, 0));
            // A heard heartbeat, answered by itself (the HB repeat off)…
            let mut s = Station::new(at());
            s.on_event(&heartbeat("W1AW", -5), 1000);
            let ack = keyed(drain(&mut s, 15_000));
            // …or, as JS8Call ships, asking first.
            let mut s = Station::new(StationConfig {
                autoreply_confirmation: true,
                ..at()
            });
            s.on_event(&heartbeat("W1AW", -5), 1000);
            let asked = s.pending_reply().map(|p| p.display);
            let (hb_want, ack_want, asked_want) = if heartbeat_speed(speed) {
                (
                    vec![(
                        Origin::Heartbeat,
                        speed,
                        "KD9TAW: @HB HEARTBEAT EN52".to_string(),
                    )],
                    vec![(
                        Origin::HbAck,
                        speed,
                        "KD9TAW: W1AW HEARTBEAT SNR -05".to_string(),
                    )],
                    Some("KD9TAW: W1AW HEARTBEAT SNR -05".to_string()),
                )
            } else {
                (Keyed::new(), Keyed::new(), None)
            };
            assert_eq!(
                (repeat, now, ack, asked),
                (hb_want.clone(), hb_want, ack_want, asked_want),
                "{speed:?}: the repeat's heartbeat, the one sent now, the acknowledgement, the \
                 question"
            );
        }
    }

    /// Switching to Turbo, or heartbeat networking off, stops the HB repeat and drops a heartbeat
    /// or acknowledgement still waiting, asking or queued: nothing of either keys afterwards.
    /// JS8Call cancels its loop there (`prepareHeartbeatMode(false)`, mainwindow.cpp:4124-4130).
    #[test]
    fn turbo_or_networking_off_stops_the_heartbeat_and_drops_what_waits() {
        let base = || StationConfig {
            hb_ack: true,
            hb_interval_min: 5,
            ..cfg()
        };
        for (what, change) in [
            (
                "Turbo",
                StationConfig {
                    speed: Speed::Turbo,
                    ..base()
                },
            ),
            (
                "networking off",
                StationConfig {
                    hb_networking: false,
                    ..base()
                },
            ),
        ] {
            let mut s = Station::new(base());
            s.set_hb(true, 0);
            s.on_event(&heartbeat("W1AW", -5), 1000);
            s.process_tx_queue();
            let mut asking = Station::new(StationConfig {
                autoreply_confirmation: true,
                ..base()
            });
            asking.on_event(&heartbeat("W1AW", -5), 1000);
            assert!(
                s.queue().len() == 1 && s.hb_next_ms().is_some() && asking.pending_len() == 1,
                "{what}: control: an acknowledgement waits, one asks, and the repeat is scheduled"
            );
            s.set_config(change.clone());
            asking.set_config(change);
            let keyed: Vec<String> = (0..150u64)
                .filter_map(|k| drain(&mut s, 2_000 + k * 6_000))
                .map(|f| f.display)
                .collect();
            assert_eq!(
                (
                    s.hb_on(),
                    s.hb_next_ms(),
                    keyed,
                    asking.pending_reply().is_some()
                ),
                (false, None, Vec::<String>::new(), false),
                "{what}: the repeat stopped, nothing keys in the next 15 minutes, nothing asks"
            );
        }
    }

    /// Heartbeat networking off (JS8Call's default): no heartbeat by the repeat or sent now, and a
    /// heard heartbeat is neither answered nor asked about, whatever HB ACK and AUTO say.
    #[test]
    fn with_heartbeat_networking_off_no_heartbeat_and_no_acknowledgement_go_out() {
        let off = StationConfig {
            hb_networking: false,
            hb_ack: true,
            ..cfg()
        };
        let mut s = Station::new(off.clone());
        s.set_hb(true, 0);
        assert!(
            !s.hb_on() && s.hb_next_ms().is_none(),
            "the repeat cannot be turned on"
        );
        s.heartbeat_now(0).expect("composes");
        s.on_event(&heartbeat("W1AW", -5), 1000);
        let keyed: Vec<String> = (0..4u64)
            .filter_map(|k| drain(&mut s, k * 15_000))
            .map(|f| f.display)
            .collect();
        assert_eq!(keyed, Vec::<String>::new(), "nothing keys");
        let mut asking = Station::new(StationConfig {
            autoreply_confirmation: true,
            ..off
        });
        asking.on_event(&heartbeat("W1AW", -5), 1000);
        assert_eq!(asking.pending_reply(), None, "nor is one asked about");
    }

    /// "Allow heartbeat transmissions outside of heartbeat sub-channel (500Hz - 1000Hz)": on, a
    /// heartbeat and an acknowledgement key on the operator's own offset (JS8Call's `-1`, which
    /// `processTxQueue` reads as `freq()`, mainwindow.cpp:4442-4447, :4472, :6614-6616); off, on a
    /// free spot in 500-1000 Hz (500 here: the draw is slot 0 and nothing is busy).
    #[test]
    fn with_hb_anywhere_a_heartbeat_and_its_acknowledgement_key_on_the_operators_offset() {
        for (anywhere, want) in [(false, FreqHint::HbSubband(500.0)), (true, FreqHint::Dial)] {
            let c = StationConfig {
                hb_anywhere: anywhere,
                hb_ack: true,
                ..cfg()
            };
            let mut s = Station::new(c.clone());
            s.heartbeat_now(0).expect("composes");
            let hb = drain(&mut s, 0).map(|f| (f.origin, f.freq_hint));
            let mut s = Station::new(c);
            s.on_event(&heartbeat("W1AW", -5), 1000);
            let ack = drain(&mut s, 15_000).map(|f| (f.origin, f.freq_hint));
            assert_eq!(
                (hb, ack),
                (Some((Origin::Heartbeat, want)), Some((Origin::HbAck, want))),
                "HB anywhere {anywhere}"
            );
        }
    }

    /// The periods (from `from` to `to`, 15 s apart) in which the station keyed something.
    fn keyed_at(s: &mut Station, from: u64, to: u64) -> Vec<u64> {
        (from / 15_000..to / 15_000)
            .map(|k| k * 15_000)
            .filter(|&t| drain(s, t).is_some())
            .collect()
    }

    /// The QSO pause (JS8Call's default): selecting a call stops the HB repeat, and clearing it
    /// turns the repeat back on, the first heartbeat one interval after the next cycle
    /// (`callsignSelectedChanged`, mainwindow.cpp:5958-5992; `on_hbMacroButton_toggled`, :4483-4497).
    /// With the pause off, the repeat goes on regardless.
    #[test]
    fn the_qso_pause_holds_the_hb_repeat_while_a_call_is_selected_and_restarts_it_after() {
        let mut s = Station::new(StationConfig {
            hb_interval_min: 5,
            ..cfg()
        });
        s.set_hb(true, 0);
        assert_eq!(
            s.hb_next_ms(),
            Some(316_000),
            "control: due one interval after the next cycle"
        );
        s.select_call(Some(" w1aw "), 100_000);
        assert_eq!(
            (s.hb_on(), s.hb_paused(), s.hb_next_ms()),
            (false, true, None),
            "W1AW selected: the repeat is paused"
        );
        assert_eq!(
            keyed_at(&mut s, 105_000, 1_005_000),
            Vec::<u64>::new(),
            "no heartbeat while W1AW is selected (it was due in the period at 315 s)"
        );
        s.select_call(None, 1_000_000);
        assert_eq!(
            (s.hb_on(), s.hb_paused(), s.hb_next_ms()),
            (true, false, Some(1_306_000)),
            "cleared: the repeat is back, due one interval after the next cycle"
        );
        assert_eq!(
            keyed_at(&mut s, 1_005_000, 1_600_000),
            vec![1_305_000],
            "and the first heartbeat goes in the period that holds it, not before"
        );
        let mut s = Station::new(StationConfig {
            hb_interval_min: 5,
            hb_qso_pause: false,
            ..cfg()
        });
        s.set_hb(true, 0);
        s.select_call(Some("W1AW"), 100_000);
        assert_eq!(
            keyed_at(&mut s, 105_000, 400_000),
            vec![315_000],
            "the QSO pause off: the heartbeat goes while W1AW is selected"
        );
        let mut s = Station::new(cfg());
        s.select_call(Some("W1AW"), 0);
        s.select_call(None, 1_000);
        assert!(
            !s.hb_on() && s.hb_next_ms().is_none(),
            "a repeat that was off stays off when the selection clears"
        );
    }

    /// A heartbeat the QSO pause holds is not turned back on after Stop TX, the idle watchdog, or
    /// heartbeat networking turned off: clearing the To box then leaves the repeat off.
    #[test]
    fn a_heartbeat_the_qso_pause_holds_is_not_restarted_after_a_stop() {
        type Stop = fn(&mut Station);
        let stops: [(&str, Stop); 3] = [
            ("Stop TX", |s| s.halt()),
            ("the idle watchdog", |s| {
                s.mark_active(1);
                s.tick(6 * 60_000);
            }),
            ("networking off", |s| {
                s.set_config(StationConfig {
                    hb_networking: false,
                    ..s.config().clone()
                })
            }),
        ];
        for (what, stop) in stops {
            let mut s = Station::new(StationConfig {
                hb_interval_min: 5,
                idle_watchdog_min: 5,
                ..cfg()
            });
            s.set_hb(true, 0);
            s.select_call(Some("W1AW"), 1_000);
            assert!(s.hb_paused(), "{what}: control: paused");
            stop(&mut s);
            // Networking back on (a no-op after the other two), so only the stop can hold it.
            s.set_config(StationConfig {
                hb_networking: true,
                ..s.config().clone()
            });
            s.select_call(None, 400_000);
            assert_eq!(
                (
                    s.hb_on(),
                    s.hb_paused(),
                    keyed_at(&mut s, 405_000, 1_500_000)
                ),
                (false, false, Vec::<u64>::new()),
                "{what}: the repeat stays off once the To box is cleared"
            );
        }
    }

    /// The QSO pause (JS8Call's default): no heartbeat is acknowledged, nor asked about, while a
    /// call is selected (processCommandActivity.cpp:647-651); once it is cleared, one is. With the
    /// pause off the acknowledgement goes regardless.
    #[test]
    fn no_heartbeat_is_acknowledged_while_a_call_is_selected_with_the_qso_pause_on() {
        let ack = Some("KD9TAW: K1ABC HEARTBEAT SNR -05".to_string());
        for (pause, while_selected) in [(true, None), (false, ack.clone())] {
            let mut s = Station::new(StationConfig {
                hb_ack: true,
                hb_qso_pause: pause,
                ..cfg()
            });
            s.select_call(Some("W1AW"), 0);
            s.on_event(&heartbeat("K1ABC", -5), 1000);
            let selected = drain(&mut s, 15_000).map(|f| f.display);
            s.select_call(Some(""), 20_000);
            // 16 minutes on: with the pause off, K1ABC was acknowledged at once, and JS8Call's
            // @ALLCALL cache holds a second acknowledgement off for 15 minutes.
            s.on_event(&heartbeat("K1ABC", -5), 16 * 60_000);
            let cleared = drain(&mut s, 16 * 60_000 + 14_000).map(|f| f.display);
            assert_eq!(
                (selected, cleared),
                (while_selected, ack.clone()),
                "QSO pause {pause}: while W1AW is selected, then once the To box is empty"
            );
        }
        let mut s = Station::new(StationConfig {
            hb_ack: true,
            autoreply_confirmation: true,
            ..cfg()
        });
        s.select_call(Some("W1AW"), 0);
        s.on_event(&heartbeat("K1ABC", -5), 1000);
        assert_eq!(s.pending_reply(), None, "nothing is asked about either");
    }

    /// The QSO pause: selecting a call stops the repeating CQ for good, as JS8Call unchecks its
    /// CQ button and never turns it back on (mainwindow.cpp:5993-6000). With the pause off the
    /// CQ repeat goes on.
    #[test]
    fn selecting_a_call_stops_the_cq_repeat_with_the_qso_pause_on() {
        for (pause, want) in [(true, vec![]), (false, vec![300_000])] {
            let mut s = Station::new(StationConfig {
                cq_interval_min: 5,
                hb_qso_pause: pause,
                ..cfg()
            });
            s.set_cq(true, 0);
            s.select_call(Some("W1AW"), 1_000);
            s.select_call(None, 2_000);
            let cqs: Vec<u64> = (1..=25u64)
                .map(|k| k * 15_000)
                .filter(|&t| {
                    s.tick(t);
                    drain(&mut s, t).is_some()
                })
                .collect();
            assert_eq!(
                (s.cq_on(), cqs),
                (!pause, want),
                "QSO pause {pause}: the CQ repeat after W1AW was selected and cleared"
            );
        }
    }

    /// Turning the HB repeat on with the QSO pause on clears the selected call, as JS8Call's HB
    /// button does (`clearCallsignSelected`, mainwindow.cpp:4487-4490), so the acknowledgements it
    /// held back go out again.
    #[test]
    fn turning_the_hb_repeat_on_clears_the_selected_call() {
        let mut s = Station::new(StationConfig {
            hb_ack: true,
            hb_interval_min: 30,
            ..cfg()
        });
        s.select_call(Some("W1AW"), 0);
        assert_eq!(
            s.hb_ack_suppressed(),
            Some(HbAckSuppressed::QsoPause),
            "control: held back while W1AW is selected"
        );
        s.set_hb(true, 1_000);
        s.on_event(&heartbeat("K1ABC", -5), 2_000);
        assert_eq!(
            (
                s.hb_ack_suppressed(),
                drain(&mut s, 15_000).map(|f| f.display)
            ),
            (None, Some("KD9TAW: K1ABC HEARTBEAT SNR -05".to_string())),
            "the selection is cleared and the heartbeat is acknowledged"
        );
    }

    /// What holds an acknowledgement back now, in JS8Call's order: the speed, a message still
    /// arriving, then the QSO pause. Each one is also what the station does.
    #[test]
    fn hb_ack_suppressed_names_what_holds_an_acknowledgement_back_now() {
        let mut s = Station::new(StationConfig {
            hb_ack: true,
            ..cfg()
        });
        assert_eq!(s.hb_ack_suppressed(), None, "nothing holds it back");
        s.note_rx_buffers(true, ["@ALLCALL"]);
        assert_eq!(
            s.hb_ack_suppressed(),
            Some(HbAckSuppressed::Arriving),
            "a message is still arriving"
        );
        s.select_call(Some("W1AW"), 0);
        assert_eq!(
            s.hb_ack_suppressed(),
            Some(HbAckSuppressed::Arriving),
            "the arriving message is named before the QSO pause"
        );
        s.note_rx_buffers(false, std::iter::empty::<&str>());
        assert_eq!(
            s.hb_ack_suppressed(),
            Some(HbAckSuppressed::QsoPause),
            "a call is selected"
        );
        s.set_config(StationConfig {
            hb_ack: true,
            speed: Speed::Turbo,
            ..cfg()
        });
        assert_eq!(
            s.hb_ack_suppressed(),
            Some(HbAckSuppressed::Speed),
            "the speed is named first"
        );
        s.set_config(StationConfig {
            hb_ack: true,
            hb_qso_pause: false,
            ..cfg()
        });
        assert_eq!(
            s.hb_ack_suppressed(),
            None,
            "the QSO pause off: a selected call holds nothing back"
        );
    }

    /// JS8Call 3.0's rule (processHeartbeatRateLimit.cpp:8-38), on: a station's first heartbeat
    /// is acknowledged and its time kept; heard again less than 55 minutes later it is
    /// acknowledged no more, ever, as JS8Call puts it in its HB blacklist (by its call as heard,
    /// which also covers it with a suffix): here the "never acknowledge" list, after the
    /// operator's own calls. 55 minutes or more later it is acknowledged again. Off, every
    /// heartbeat not on the list is acknowledged, and the stations the rule put on it stay there,
    /// as on JS8Call's list; taken off the list, a station is acknowledged again.
    #[test]
    fn the_55_minute_rule_stops_acknowledging_a_station_heard_again_inside_the_window() {
        const MIN: u64 = 60_000;
        let ack = |c: &str| Some(format!("KD9TAW: {c} HEARTBEAT SNR -05"));
        let heard = |s: &mut Station, call: &str, t: u64| {
            let actions = s.on_event(&heartbeat(call, -5), t);
            (drain(s, t + 14_000).map(|f| f.display), actions)
        };
        let on = || StationConfig {
            hb_ack: true,
            hb_rate_limit: true,
            hb_ack_deny: vec!["N0XYZ".into()],
            ..cfg()
        };
        let mut s = Station::new(on());
        assert_eq!(heard(&mut s, "W1AW", 0).0, ack("W1AW"), "W1AW's first");
        assert_eq!(heard(&mut s, "K1ABC", MIN).0, ack("K1ABC"), "K1ABC's first");
        let (keyed, actions) = heard(&mut s, "W1AW", 30 * MIN);
        assert_eq!(
            (
                keyed,
                s.config().hb_ack_deny.clone(),
                s.hb_rate_blocked().to_vec()
            ),
            (
                None,
                vec!["N0XYZ".to_string(), "W1AW".to_string()],
                vec!["W1AW".to_string()]
            ),
            "W1AW again 30 minutes later: not acknowledged, and on the never-ACK list after the \
             operator's own, as JS8Call's addToHbBlacklist appends it"
        );
        assert!(
            actions.contains(&StationAction::HbAckRateLimited {
                from: "W1AW".into()
            }),
            "the change is reported, for the engine to write into Settings: {actions:?}"
        );
        assert_eq!(
            heard(&mut s, "K1ABC", MIN + 55 * MIN).0,
            ack("K1ABC"),
            "K1ABC again exactly 55 minutes later: acknowledged"
        );
        assert_eq!(
            (
                heard(&mut s, "W1AW", 120 * MIN).0,
                heard(&mut s, "W1AW/P", 121 * MIN).0
            ),
            (None, None),
            "W1AW two hours on, and as W1AW/P: still not"
        );
        // Kept across a launch by the Settings the engine wrote it into; the journal names it.
        let mut back = Station::new(StationConfig {
            hb_ack_deny: s.config().hb_ack_deny.clone(),
            ..on()
        });
        back.restore(s.snapshot(122 * MIN), 122 * MIN);
        assert_eq!(
            (
                back.hb_rate_blocked().to_vec(),
                heard(&mut back, "W1AW", 200 * MIN).0
            ),
            (vec!["W1AW".to_string()], None),
            "restored: still on the list, still not acknowledged"
        );
        // The rule off: its times forgotten, but W1AW stays on the list, as on JS8Call's.
        s.set_config(StationConfig {
            hb_rate_limit: false,
            ..s.config().clone()
        });
        // 16 minutes apart: inside the rule's 55, past JS8Call's @ALLCALL cache, which holds a
        // second acknowledgement off for 15 minutes whatever the rule says.
        assert_eq!(
            (
                s.hb_rate_blocked().to_vec(),
                heard(&mut s, "W1AW", 130 * MIN).0,
                heard(&mut s, "K1ABC", 131 * MIN).0,
                heard(&mut s, "K1ABC", 147 * MIN).0
            ),
            (vec!["W1AW".to_string()], None, ack("K1ABC"), ack("K1ABC")),
            "the rule off: W1AW still on the list; K1ABC 16 minutes apart, acknowledged both times"
        );
        // Taken off the list (the operator's Settings): acknowledged again, and the rule's no more.
        s.set_config(on());
        assert_eq!(
            (
                s.hb_rate_blocked().len(),
                heard(&mut s, "W1AW", 162 * MIN).0
            ),
            (0, ack("W1AW")),
            "off the list: W1AW starts afresh"
        );
    }

    /// "Do not participate in the @ALLCALL group" (processCommandActivity.cpp:181-185): a QUERY
    /// MSGS on @ALLCALL goes unanswered even with a message waiting for the asker. The same query
    /// addressed to me is answered, and a heartbeat is still acknowledged.
    #[test]
    fn not_taking_part_in_allcall_leaves_an_allcall_query_unanswered() {
        for avoid in [false, true] {
            let mut s = Station::new(StationConfig {
                avoid_allcall: avoid,
                hb_ack: true,
                ..cfg()
            });
            store_for(&mut s, "K1ABC", 1_000);
            let yes = format!("KD9TAW: K1ABC YES MSG ID {}", s.inbox()[0].id);
            let allcall = allcall_query_msgs(&mut s, "K1ABC", 100_000);
            s.on_event(
                &directed("K1ABC", "KD9TAW", Some(Command::QueryMsgs), None, "", -5),
                200_000,
            );
            let direct: Vec<String> = (0..6u64)
                .filter_map(|k| drain(&mut s, 202_000 + k * 15_000))
                .filter(|f| f.first)
                .map(|f| f.display)
                .collect();
            s.on_event(&heartbeat("W1AW", -5), 400_000);
            let hb = drain(&mut s, 415_000).map(|f| f.display);
            assert_eq!(
                (allcall, direct, hb),
                (
                    if avoid { vec![] } else { vec![yes.clone()] },
                    vec![yes],
                    Some("KD9TAW: W1AW HEARTBEAT SNR -05".to_string())
                ),
                "not taking part in @ALLCALL {avoid}"
            );
        }
    }

    // ---- the APRS-IS relay (JS8Call 3.0.3): the receive side, and its message on the air ----

    /// `W1RLY: @APRSIS <cmd> <text>` as the reassembler closes it: a checksum that verified, or
    /// (`Checksum::Bad`) the raw body of one that did not.
    fn aprsis(cmd: Command, text: &str, checksum: Checksum) -> MessageEvent {
        let MessageEvent::Message(mut m) = directed("W1RLY", "@APRSIS", Some(cmd), None, text, -9)
        else {
            unreachable!("directed() builds a message")
        };
        m.checksum = checksum;
        m.complete = checksum != Checksum::Bad;
        m.frames = 3;
        MessageEvent::Message(m)
    }

    #[test]
    fn an_aprsis_msg_to_naming_me_is_filed_from_aprs_and_its_repeat_only_after_a_minute() {
        let mut s = Station::new(cfg());
        let ev = aprsis(
            Command::MsgTo,
            "KD9TAW HELLO THERE DE N0CALL-9",
            Checksum::Ok,
        );
        let acts = s.on_event(&ev, 1_000);
        assert!(acts.contains(&StationAction::InboxChanged));
        assert_eq!(s.inbox().len(), 1);
        let e = &s.inbox()[0];
        assert_eq!(
            (e.from.as_str(), e.to.as_str(), e.text.as_str()),
            ("APRS", "KD9TAW", "HELLO THERE DE N0CALL-9")
        );
        assert_eq!(e.path, vec!["APRS".to_string()]);
        assert_eq!(
            (e.state, e.at_ms, e.snr_db),
            (InboxState::Unread, 1_000, -9)
        );
        // JS8Call keeps the key while it is no older than the minute, and drops it after.
        s.on_event(&ev, 61_000);
        assert_eq!(
            s.inbox().len(),
            1,
            "a repeat inside the minute is not filed"
        );
        s.on_event(&ev, 61_001);
        assert_eq!(s.inbox().len(), 2, "past the minute it is");
        // Nothing answers it: no ACK to the relay station, nothing held, nothing keyed.
        assert!(keyed_in(&mut s, 75_000, 4).is_empty());
    }

    #[test]
    fn a_relay_keyed_without_a_checksum_is_still_filed_and_so_is_the_msg_form() {
        let mut s = Station::new(cfg());
        // JS8Call 3.0.3 keys these with no checksum, so the body closes Bad, raw.
        s.on_event(
            &aprsis(Command::MsgTo, "KD9TAW QSL DE N0CALL", Checksum::Bad),
            1_000,
        );
        assert_eq!(s.inbox().len(), 1);
        assert_eq!(s.inbox()[0].text, "QSL DE N0CALL");
        // Its other branch: a MSG whose text is `TO:<call> <message>`.
        s.on_event(
            &aprsis(Command::Msg, "TO:KD9TAW 73 DE N0CALL", Checksum::Bad),
            2_000,
        );
        assert_eq!(s.inbox().len(), 2);
        assert_eq!(
            (s.inbox()[1].from.as_str(), s.inbox()[1].text.as_str()),
            ("APRS", "73 DE N0CALL")
        );
        // CONTROL: a failed checksum on anything else is still refused, and said.
        let MessageEvent::Message(mut bad) = directed(
            "W1AW",
            "KD9TAW",
            Some(Command::MsgTo),
            None,
            "K1ABC HI XYZ",
            -5,
        ) else {
            unreachable!()
        };
        bad.checksum = Checksum::Bad;
        let acts = s.on_event(&MessageEvent::Message(bad), 3_000);
        assert!(acts
            .iter()
            .any(|a| matches!(a, StationAction::ChecksumFailed { .. })));
        assert_eq!(s.inbox().len(), 2, "nothing stored from it");
    }

    #[test]
    fn an_aprsis_message_for_another_call_is_neither_filed_nor_stored_and_the_lists_hold() {
        let mut s = Station::new(cfg());
        s.on_event(
            &aprsis(Command::MsgTo, "W1AW HELLO DE N0CALL", Checksum::Ok),
            1_000,
        );
        assert!(
            s.inbox().is_empty(),
            "not mine to file, and @APRSIS is no store-and-forward"
        );
        assert!(
            keyed_in(&mut s, 15_000, 4).is_empty(),
            "and nothing answers"
        );
        // No `DE <sender>`: filed all the same, its text as it came.
        s.on_event(&aprsis(Command::MsgTo, "KD9TAW HELLO", Checksum::Ok), 2_000);
        assert_eq!(s.inbox()[0].text, "HELLO");
        // A relay station the deny list names is acted on in no way.
        let mut d = Station::new(StationConfig {
            autoreply_deny: vec!["W1RLY".into()],
            ..cfg()
        });
        d.on_event(
            &aprsis(Command::MsgTo, "KD9TAW HELLO DE N0CALL", Checksum::Ok),
            1_000,
        );
        assert!(d.inbox().is_empty());
        // …nor while the idle watchdog stands.
        let mut t = Station::new(StationConfig {
            idle_watchdog_min: 5,
            ..cfg()
        });
        t.mark_active(1);
        t.tick(6 * 60_000);
        assert!(t.idle_tripped(), "control: tripped");
        t.on_event(
            &aprsis(Command::MsgTo, "KD9TAW HELLO DE N0CALL", Checksum::Ok),
            6 * 60_000,
        );
        assert!(t.inbox().is_empty());
    }

    #[test]
    fn the_aprs_relay_keys_its_msg_to_whatever_auto_says_ahead_of_a_waiting_reply() {
        // AUTO off: an ordinary reply would go to the compose box, never keyed by itself.
        let mut s = Station::new(StationConfig {
            autoreply: false,
            ..cfg()
        });
        assert!(s.enqueue_aprs_relay("@APRSIS MSG TO:W1AW HELLO DE N0CALL"));
        let wire = keyed_wire(&mut s, 15_000);
        let want_body = format!(
            "W1AW HELLO DE N0CALL {}",
            crate::proto::crc16::checksum3("W1AW HELLO DE N0CALL")
        );
        assert_eq!(
            wire.first().map(|w| &w.0),
            Some(&Frame::Directed {
                from: CallRef::Base("KD9TAW".into()),
                to: CallRef::parse("@APRSIS").expect("a group"),
                cmd: Command::MsgTo,
                num: None,
                portable_from: false,
                portable_to: false,
            }),
            "a directed MSG TO: to @APRSIS, from me"
        );
        let body: String = wire
            .iter()
            .skip(1)
            .map(|(f, _, _)| match f {
                Frame::Data { text, .. } => text.clone(),
                other => panic!("only data follows: {other:?}"),
            })
            .collect();
        assert_eq!(body, want_body, "the message and DE, then its checksum");
        assert!(wire.first().is_some_and(|w| w.1) && wire.last().is_some_and(|w| w.2));

        // Ahead of an automatic reply already waiting (High beats Normal in JS8Call's queue), and
        // behind a relay already there. The reply is made first; nothing has left the queue yet.
        let mut q = Station::new(cfg());
        q.on_event(&snr_query("W1AW"), 1_000);
        assert!(q.enqueue_aprs_relay("@APRSIS MSG TO:W1AW ONE DE N0CALL"));
        assert!(q.enqueue_aprs_relay("@APRSIS MSG TO:W1AW TWO DE N0CALL"));
        let firsts: Vec<(Origin, String)> = (0..16u64)
            .filter_map(|k| drain(&mut q, 15_000 * (k + 1)))
            .filter(|f| f.first)
            .map(|f| (f.origin, f.display))
            .collect();
        assert_eq!(
            firsts,
            vec![
                (
                    Origin::AprsRelay,
                    "KD9TAW: @APRSIS MSG TO:W1AW ONE DE N0CALL".to_string()
                ),
                (
                    Origin::AprsRelay,
                    "KD9TAW: @APRSIS MSG TO:W1AW TWO DE N0CALL".to_string()
                ),
                (Origin::AutoReply, "KD9TAW: W1AW SNR -07".to_string()),
            ]
        );
    }

    #[test]
    fn a_full_queue_refuses_the_aprs_relay() {
        let mut s = Station::new(cfg());
        for k in 0..MAX_PENDING {
            assert!(s.enqueue_aprs_relay(&format!("@APRSIS MSG TO:W1AW N{k} DE N0CALL")));
        }
        assert!(!s.enqueue_aprs_relay("@APRSIS MSG TO:W1AW OVER DE N0CALL"));
    }

    /// A relay that does not compose when its turn comes (here no callsign is set) is not sent,
    /// and the log says so as a relay; an automatic reply that does not compose is still said as
    /// one (`a_reply_that_no_longer_fits_when_it_goes_is_said_and_its_message_stays_held`).
    #[test]
    fn a_relay_that_does_not_compose_is_said_as_a_relay() {
        let mut s = Station::new(StationConfig {
            mycall: String::new(),
            ..cfg()
        });
        assert!(s.enqueue_aprs_relay("@APRSIS MSG TO:W1AW HELLO DE N0CALL"));
        let said: Vec<String> = s
            .process_tx_queue()
            .into_iter()
            .filter_map(|a| match a {
                StationAction::Toast { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(
            said,
            ["APRS-IS relay not sent, no callsign is set: @APRSIS MSG TO:W1AW HELLO DE N0CALL"]
        );
        assert!(s.queue().is_empty(), "nothing waits to key");
    }

    /// JS8Call's call list marks (displayCallActivity.cpp:200-245): ★ "Hearing Your Station" from the
    /// `ackTimestamp` a message addressed to my call or its base call sets (processCommandActivity.cpp:
    /// 59-65), ☎ "Calling CQ" from the `cqTimestamp` a CQ sets (:746). Each is kept until a later one
    /// (mainwindow.cpp:2302-2307); traffic to someone else and a heartbeat set neither. Both ride the
    /// station journal, and a journal from before them loads with neither.
    #[test]
    fn a_message_to_me_marks_its_sender_hearing_me_and_a_cq_marks_when_it_called() {
        fn at(ev: MessageEvent, ms: u64) -> MessageEvent {
            match ev {
                MessageEvent::Message(m) => MessageEvent::Message(Message { last_ms: ms, ..m }),
                other => other,
            }
        }
        fn cq(from: &str, ms: u64) -> MessageEvent {
            match heartbeat(from, -8) {
                MessageEvent::Message(m) => MessageEvent::Message(Message {
                    to: CallRef::parse("@ALLCALL"),
                    to_text: "@ALLCALL".into(),
                    cq: Some(0),
                    last_ms: ms,
                    ..m
                }),
                other => other,
            }
        }
        fn marks(s: &Station, call: &str) -> Option<(Option<u64>, Option<u64>)> {
            s.heard()
                .iter()
                .find(|h| h.call == call)
                .map(|h| (h.ack_ms, h.cq_ms))
        }
        let mut s = Station::new(cfg());
        let snr_to = |to: &str| directed("W1AW", to, Some(Command::SnrQuery), None, "", -5);
        s.on_event(&at(snr_to("K1ABC"), 1_000), 1_000);
        s.on_event(&at(heartbeat("N0GRD", -9), 2_000), 2_000);
        assert_eq!(
            marks(&s, "W1AW"),
            Some((None, None)),
            "to someone else: no ★"
        );
        assert_eq!(
            marks(&s, "N0GRD"),
            Some((None, None)),
            "a heartbeat: neither"
        );
        s.on_event(&at(snr_to("KD9TAW"), 3_000), 3_000);
        assert_eq!(marks(&s, "W1AW"), Some((Some(3_000), None)), "to me: ★");
        s.on_event(&at(cq("W1AW", 4_000), 4_000), 4_000);
        assert_eq!(
            marks(&s, "W1AW"),
            Some((Some(3_000), Some(4_000))),
            "a CQ: ☎, and the ★ stays"
        );
        s.on_event(&at(snr_to("K1ABC"), 5_000), 5_000);
        assert_eq!(
            marks(&s, "W1AW"),
            Some((Some(3_000), Some(4_000))),
            "later traffic to someone else keeps both"
        );
        s.on_event(&at(snr_to("KD9TAW"), 6_000), 6_000);
        assert_eq!(
            marks(&s, "W1AW"),
            Some((Some(6_000), Some(4_000))),
            "a later one moves ★"
        );

        // My base call, from a portable call of mine, is to me too (`toMe`'s second half).
        let mut p = Station::new(StationConfig {
            mycall: "KD9TAW/P".into(),
            ..cfg()
        });
        p.on_event(&at(snr_to("KD9TAW"), 7_000), 7_000);
        assert_eq!(marks(&p, "W1AW"), Some((Some(7_000), None)));

        // The journal: both written, and an entry from before them reads as neither.
        let json = serde_json::to_value(&s.heard()[0]).expect("serialises");
        assert_eq!(
            (json["ackMs"].as_u64(), json["cqMs"].as_u64()),
            (Some(6_000), Some(4_000))
        );
        let old: Heard = serde_json::from_value(serde_json::json!({
            "call": "W1AW", "grid": null, "snrDb": -5, "freqHz": 1500.0, "speed": "normal",
            "lastMs": 1, "lastHb": false, "lastCq": false, "storedMsgs": 0
        }))
        .expect("a journal from before the marks loads");
        assert_eq!((old.ack_ms, old.cq_ms), (None, None));
        let none = serde_json::to_value(&old).expect("serialises");
        assert!(
            none.get("ackMs").is_none() && none.get("cqMs").is_none(),
            "no mark, no key"
        );
    }
}

#[cfg(test)]
#[path = "station_replies_tests.rs"]
mod replies_tests;

#[cfg(test)]
#[path = "station_followups_tests.rs"]
mod followups_tests;

#[cfg(test)]
#[path = "station_design_tests.rs"]
mod design_tests;
