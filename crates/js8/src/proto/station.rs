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
//! relies on that being the single gate every TX safety check sits above (spec invariant 13),
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
//! - HB-ACK (default off): only with HB + autoreply + `hb_ack`, an empty outbox and no QSO pause,
//!   answer a heard heartbeat `CALL HEARTBEAT SNR +NN [MSG ID n]`, on a free heartbeat spot picked
//!   as the heartbeat's is but with no own-offset rule (`sendHeartbeatAck`, :6299). An incoming
//!   `HEARTBEAT SNR` is never answered (no ack-of-an-ack loop).
//! - Autoreply: only to my own call and joined groups, never to a query addressed to `@ALLCALL`
//!   (JS8Call's `!isAllCall`, mainwindow.cpp:8834-8869), and only the autoreply subset
//!   (`Command::is_autoreply`, = upstream `autoreply_cmds {0,2,3,4,6,9,10,11,12,13,14,16,30}`);
//!   an empty INFO or grid draws no reply (:8841, :8861). `QUERY MSGS` on `@ALLCALL` is answered
//!   only when a message waits, once per station per `allcall_reply_interval_ms` (:8812, :9275).
//! - Relay (`>`): retransmit `rest *DE* MYCALL`; at the final hop parse the chain to `A>B>C` and
//!   answer `A>B>C ACK` (unless the embedded text is itself an autoreply command).
//! - Store-and-forward: `MSG TO:` stores a `Store` inbox row keyed to the base callsign; the next
//!   heartbeat/query from that call is offered `MSG ID n`; `QUERY MSG n` delivers it and marks it
//!   `Delivered`.
//! - A `MSG` to me or a joined group (never `@ALLCALL`) is filed `Unread` in my inbox, as
//!   JS8Call's `addCommandToMyInbox` files it (mainwindow.cpp:9121-9133), and answered as JS8Call
//!   answers it (:9139): `<from> ACK` to the sender as heard, or `<path> ACK` back along the relay
//!   path that brought it. The ACK is an automatic reply, so only with autoreply on, and one waits
//!   at a time however many copies arrive. My mail (`Unread`/`Read`) is bounded apart from mail
//!   held for others and never expires.
//! - Idle watchdog: on trip, stop TX and turn autoreply/relay/HB OFF, clear the outbox, surface a
//!   toast — `tx_enabled` (the engine's latch) is NOT touched.
use crate::phy::{Speed, Word87, I3};
use crate::proto::callsign::{split_portable, CallRef};
use crate::proto::command::Command;
// Every composer below hands `frames_with_grid` the station's locator, as JS8Call hands
// `buildMessageFrames` `my_grid().left(4)` for every message (mainwindow.cpp:5434, :7885):
// compose puts the square in a compound callsign's announcement (varicode.cpp:2125) and
// nowhere else.
use crate::proto::compose::{frames_with_grid, ComposeError};
use crate::proto::frame::{encode_frame, format_snr, Frame};
use crate::proto::reassembly::{Message, MessageEvent};
use std::collections::HashMap;

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
// stored bytes, and expire at 48 h — upstream's group-message lifetime. Over a cap, drop the
// OLDEST and toast; never silently. A MSG to me is the operator's own mail: it has the same caps,
// counted apart (so it can never push out mail held for someone else), and no expiry. `heard` is one row per station: a busy band holds a few
// hundred, so cap at 500 and evict least-recently-heard. `allcall_replied` records a reply time
// per callsign; past the reply interval it carries no information, so it self-prunes.
const MAX_INBOX: usize = 100;
const MAX_INBOX_BYTES: usize = 64 * 1024;
const INBOX_TTL_MS: u64 = 48 * 60 * 60 * 1000;
const MAX_HEARD: usize = 500;

// The outbox drains once per period; pending auto-replies queue faster than that under a flood.
// Cap both — refusing an auto-reply under flood is correct (the alternative transmits stale
// traffic for hours), and the refusal is surfaced as a Toast, never silent.
const MAX_OUTBOX: usize = 32;
const MAX_PENDING: usize = 32;

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
    pub allcall_reply_interval_ms: u64,
    pub reply_delay_ms: u64,
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
            allcall_reply_interval_ms: 15 * 60 * 1000,
            reply_delay_ms: 0,
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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationSnapshot {
    pub inbox: Vec<InboxEntry>,
    pub heard: Vec<Heard>,
    pub allcall_replied: Vec<(String, u64)>,
    pub next_inbox_id: u32,
}

/// One composed message waiting in the outbox; `next_frame` dispenses its frames one per period.
#[derive(Debug, Clone)]
struct OutMsg {
    origin: Origin,
    frames: Vec<(Frame, I3)>,
    cursor: usize,
    freq_hint: FreqHint,
    display: String,
}

/// A scheduled automatic reply, cancellable until `fires_at_ms`.
#[derive(Debug, Clone)]
struct Pending {
    origin: Origin,
    to: String,
    text: String,
    display: String,
    fires_at_ms: u64,
    freq_hint: FreqHint,
}

pub struct Station {
    cfg: StationConfig,
    outbox: std::collections::VecDeque<OutMsg>,
    pending: Vec<Pending>,
    heard: Vec<Heard>,
    inbox: Vec<InboxEntry>,
    allcall_replied: HashMap<String, u64>,
    next_inbox_id: u32,
    hb_on: bool,
    hb_next_ms: Option<u64>,
    /// The CQ repeat: session-only, exactly like `hb_on` (never journaled, so the app can
    /// never launch calling CQ). `cq_idx` is the CQS variant the operator armed it with.
    cq_on: bool,
    cq_next_ms: Option<u64>,
    cq_idx: u8,
    last_activity_ms: u64,
    idle_tripped: bool,
    last_tx_display: Option<String>,
}

impl Station {
    pub fn new(cfg: StationConfig) -> Station {
        Station {
            cfg,
            outbox: std::collections::VecDeque::new(),
            pending: Vec::new(),
            heard: Vec::new(),
            inbox: Vec::new(),
            allcall_replied: HashMap::new(),
            next_inbox_id: 1,
            hb_on: false,
            hb_next_ms: None,
            cq_on: false,
            cq_next_ms: None,
            cq_idx: 0,
            last_activity_ms: 0,
            idle_tripped: false,
            last_tx_display: None,
        }
    }

    pub fn set_config(&mut self, cfg: StationConfig) {
        self.cfg = cfg;
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
        let to = to.to_ascii_uppercase();
        to == self.base()
            || to == "@ALLCALL"
            || self.cfg.groups.iter().any(|g| g.eq_ignore_ascii_case(&to))
    }

    // ---- the gating chain -------------------------------------------------------------------

    /// Route one reassembler event through JS8Call's gating chain (mainwindow.cpp:8560-9389).
    pub fn on_event(&mut self, ev: &MessageEvent, now_ms: u64) -> Vec<StationAction> {
        let MessageEvent::Message(m) = ev else {
            return Vec::new(); // per-frame events feed the activity pane, not the station brain
        };
        let mut actions = Vec::new();
        self.record_heard(m, &mut actions);

        let to_me = m.to_text.eq_ignore_ascii_case(&self.base());
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

        // A directed message with a failed checksum is surfaced, never acted on.
        if m.checksum == crate::proto::reassembly::Checksum::Bad {
            actions.push(StationAction::ChecksumFailed {
                from: m.from.clone(),
                freq_hz: m.freq_hz,
            });
            return actions;
        }

        // Heartbeat heard → maybe HB-ACK.
        if m.is_heartbeat() && m.cq.is_none() {
            self.maybe_hb_ack(m, now_ms, &mut actions);
            return actions;
        }
        if m.cq.is_some() {
            return actions; // a CQ is displayed; the operator answers it, not the autoreply engine
        }

        // Directed command autoreply / relay / store-and-forward. JS8Call acts on no command that
        // is not addressed to me, @ALLCALL or a group I joined (mainwindow.cpp:8601), and a relay
        // (`>`, :8908), a MSG TO: (:9012) or a QUERY MSG n (:9171) never on @ALLCALL either.
        let mine = to_me || to_group;
        if let Some(cmd) = m.cmd {
            if self.cfg.relay && cmd == Command::Relay {
                if mine {
                    self.handle_relay(m, now_ms, &mut actions);
                }
                return actions;
            }
            if cmd == Command::MsgTo {
                if mine {
                    self.handle_msg_to(m, now_ms, &mut actions);
                }
                return actions;
            }
            if cmd == Command::QueryMsgs || is_query_msg(m) {
                self.handle_query_msg(m, mine, now_ms, &mut actions);
                return actions;
            }
            // A MSG to me or a group I joined is filed UNREAD (JS8Call's `d.cmd == " MSG" &&
            // !isAllCall`, mainwindow.cpp:9121, past the "to me, a group or @ALLCALL" gate at
            // :8715), whatever the switches say. Its ACK (:9139) is an automatic reply, below.
            if cmd == Command::Msg && mine {
                self.file_msg_to_me(m, now_ms, &mut actions);
            }
            // Never an @ALLCALL query: JS8Call's SNR?/INFO?/STATUS?/GRID?/HEARING? replies each
            // carry `!isAllCall` (mainwindow.cpp:8834, :8839, :8849, :8859, :8869).
            if self.cfg.autoreply
                && cmd.is_autoreply()
                && (to_me || to_group)
                && !self.is_me(&m.from)
            {
                self.autoreply(m, cmd, now_ms, &mut actions);
            }
        }
        actions
    }

    fn is_me(&self, call: &str) -> bool {
        split_portable(call).0.eq_ignore_ascii_case(&self.base())
    }

    fn record_heard(&mut self, m: &Message, actions: &mut Vec<StationAction>) {
        let call = clamp_str(split_portable(&m.from).0, MAX_CALL_LEN).to_ascii_uppercase();
        if call.is_empty() || call == "<....>" {
            return;
        }
        let grid = m.grid.as_deref().map(|g| clamp_str(g, MAX_CALL_LEN));
        let stored = self
            .inbox
            .iter()
            .filter(|e| e.state == InboxState::Store && e.to.eq_ignore_ascii_case(&call))
            .count() as u8;
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

    /// Expire mail HELD for someone else (`Store`, `Delivered`) older than 48 h, then, while
    /// either half of the inbox is over the count or byte cap, drop that half's OLDEST entry (by
    /// `at_ms`) and toast — never silently. The halves are counted apart so that a flood of MSGs
    /// to me can never push out a message held for another station, which would change what a
    /// later `QUERY MSGS` is answered. My own mail (`Unread`, `Read`) does not expire: JS8Call
    /// keeps a MSG to me until the operator deletes it.
    fn prune_inbox(&mut self, now_ms: u64, actions: &mut Vec<StationAction>) {
        let before = self.inbox.len();
        self.inbox
            .retain(|e| is_my_mail(e.state) || now_ms.saturating_sub(e.at_ms) < INBOX_TTL_MS);
        let mut evicted = before != self.inbox.len();
        let mut mine_evicted = false;
        for mine in [false, true] {
            while self.inbox_count(mine) > MAX_INBOX || self.inbox_bytes(mine) > MAX_INBOX_BYTES {
                let Some((idx, _)) = self
                    .inbox
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| is_my_mail(e.state) == mine)
                    .min_by_key(|(_, e)| e.at_ms)
                else {
                    break;
                };
                self.inbox.remove(idx);
                if mine {
                    mine_evicted = true;
                } else {
                    evicted = true;
                }
            }
        }
        if evicted || mine_evicted {
            actions.push(StationAction::InboxChanged);
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
        // mainwindow.cpp:7748 — HB + autoreply + hb_ack + empty buffer (no in-flight message).
        if !(self.hb_on && self.cfg.autoreply && self.cfg.hb_ack && self.outbox.is_empty()) {
            return;
        }
        let to = split_portable(&m.from).0.to_ascii_uppercase();
        if self.is_me(&to) {
            return;
        }
        let mut text = format!("{to} HEARTBEAT SNR {}", format_snr(m.snr_db));
        if let Some(id) = self.stored_for(&to) {
            text.push_str(&format!(" MSG ID {id}"));
        }
        // On a free heartbeat spot, as JS8Call's `sendHeartbeatAck` picks it
        // (`findFreeFreqOffset(500, 1000, 50)`, mainwindow.cpp:6299); `next_frame` picks it.
        let spot = FreqHint::HbSubband(0.0);
        self.schedule_reply(Origin::HbAck, &to, &text, spot, now_ms, actions);
    }

    fn autoreply(
        &mut self,
        m: &Message,
        cmd: Command,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) {
        if cmd == Command::Msg {
            self.ack_msg(m, now_ms, actions);
            return;
        }
        let from = split_portable(&m.from).0.to_ascii_uppercase();
        let reply = match cmd {
            Command::SnrQuery => Some(format!("{from} SNR {}", format_snr(m.snr_db))),
            // Nothing set, nothing said: JS8Call skips an empty grid or INFO
            // (mainwindow.cpp:8861-8863, :8841-8843).
            Command::GridQuery if self.cfg.grid.is_empty() => None,
            Command::InfoQuery if self.cfg.info.is_empty() => None,
            Command::GridQuery => Some(format!("{from} GRID {}", self.cfg.grid)),
            Command::InfoQuery => Some(format!(
                "{from} INFO {}",
                self.expand_grid_macros(&self.cfg.info)
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

    /// JS8Call's answer to a MSG it filed (mainwindow.cpp:9127-9139): `<from> ACK` to the
    /// sender as it was heard (`d.from`, so a `/P` or compound call stays whole), or `<path>
    /// ACK` when a relay brought it (`calls.length() > 1 ? d.relayPath : d.from`, the path
    /// `parseRelayPathCallsigns` reads and the inbox keeps). It waits out the countdown every
    /// automatic reply does, and it exists only with autoreply on: JS8Call types it into the
    /// compose box either way but keys it only with AUTO checked (`processTxQueue`, :9674-9685;
    /// the reply has no trailing space, so the `" ACK "` test there never matches it). A copy
    /// of the MSG heard while its ACK still waits draws no second one: JS8Call's waiting ACK
    /// sits in the compose box until it has gone, and no reply is queued while the box holds
    /// text (:9365). A path over `MAX_PATH_HOPS` is refused, as a relay chain is in
    /// `handle_relay`: its calls come from the sender's text, and upstream keys all of them.
    fn ack_msg(&mut self, m: &Message, now_ms: u64, actions: &mut Vec<StationAction>) {
        let from = clamp_str(&m.from, MAX_CALL_LEN).to_ascii_uppercase();
        let path = relay_path(&from, &clamp_str(m.text.trim(), MAX_TEXT_LEN));
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
        // `MSG TO:` — store the body for the named target (the first token of the body).
        let body = m.text.trim();
        let (target, text) = body.split_once(' ').unwrap_or((body, ""));
        let target = split_portable(target).0.to_ascii_uppercase();
        if target.is_empty() {
            return;
        }
        let id = self.next_inbox_id;
        self.next_inbox_id += 1;
        self.inbox.push(InboxEntry {
            id,
            from: clamp_str(split_portable(&m.from).0, MAX_CALL_LEN).to_ascii_uppercase(),
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
    }

    /// A MSG to me (or a group I joined) into my inbox as `Unread`: JS8Call's
    /// `addCommandToMyInbox` → `addCommandToStorage("UNREAD", d)` (mainwindow.cpp:9462-9467),
    /// with its PATH (`parseRelayPathCallsigns`, :9127).
    fn file_msg_to_me(&mut self, m: &Message, now_ms: u64, actions: &mut Vec<StationAction>) {
        let from = clamp_str(split_portable(&m.from).0, MAX_CALL_LEN).to_ascii_uppercase();
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

    /// `mine`: addressed to my call or a group I joined (not @ALLCALL, not another station).
    fn handle_query_msg(
        &mut self,
        m: &Message,
        mine: bool,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) {
        let from = split_portable(&m.from).0.to_ascii_uppercase();
        // `QUERY MSG n` delivers the stored message n (if it is for the querier) and marks it,
        // only when the query was addressed to me (mainwindow.cpp:8601, :9171). Anything else
        // it may be draws no reply at all: JS8Call's buffered QUERY skips every miss
        // (:9194-9233) and never answers it as a QUERY MSGS.
        if m.cmd != Some(Command::QueryMsgs) {
            let Some(id) = query_msg_id(m).filter(|_| mine) else {
                return;
            };
            if let Some(e) = self.inbox.iter_mut().find(|e| {
                e.id == id && e.state == InboxState::Store && e.to.eq_ignore_ascii_case(&from)
            }) {
                e.state = InboxState::Delivered;
                let deliver = format!("{from} MSG {} FROM {}", e.text, e.from);
                actions.push(StationAction::InboxChanged);
                self.schedule_reply(
                    Origin::AutoReply,
                    &from,
                    &deliver,
                    FreqHint::Dial,
                    now_ms,
                    actions,
                );
            }
            return;
        }
        // `QUERY MSGS` (do you have any for me?) → offer the first stored id, else NO. On
        // @ALLCALL, JS8Call answers only YES, and says NO to a directed query alone
        // (mainwindow.cpp:9255-9278); an @ALLCALL reply then holds the sender off for the
        // interval (:9377-9379, checked first at :8812).
        if self.cfg.autoreply && self.addressed_to_me(&m.to_text) {
            let to_allcall = m.to_text.eq_ignore_ascii_case("@ALLCALL");
            let reply = match self.stored_for(&from) {
                Some(id) => format!("{from} YES MSG ID {id}"),
                None if to_allcall => return,
                None => format!("{from} NO"),
            };
            if to_allcall && self.allcall_held_off(&from, now_ms, actions) {
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

    /// JS8Call's @ALLCALL cache: a station answered on @ALLCALL is held off for
    /// `allcall_reply_interval_ms` (15 minutes; mainwindow.cpp:8812). True, and `RateLimited`,
    /// while it is held off; otherwise it is recorded as answered now.
    fn allcall_held_off(
        &mut self,
        from: &str,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) -> bool {
        if let Some(&last) = self.allcall_replied.get(from) {
            if now_ms.saturating_sub(last) < self.cfg.allcall_reply_interval_ms {
                actions.push(StationAction::RateLimited {
                    from: from.to_string(),
                });
                return true;
            }
        }
        self.allcall_replied.insert(from.to_string(), now_ms);
        self.prune_allcall(now_ms); // keep the map self-limiting, not one entry per call ever
        false
    }

    fn stored_for(&self, call: &str) -> Option<u32> {
        self.inbox
            .iter()
            .find(|e| e.state == InboxState::Store && e.to.eq_ignore_ascii_case(call))
            .map(|e| e.id)
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
            clamp_str(&self.expand_grid_macros(&self.cfg.status), MAX_INFO_LEN)
        }
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
        // Cap the pending auto-reply queue: under a flood, refuse and toast rather than growing.
        // Refusing an auto-reply is correct — the alternative keys stale traffic for hours.
        if self.pending.len() >= MAX_PENDING {
            actions.push(StationAction::Toast {
                text: "Reply queue full: auto-reply dropped".into(),
                directed_to_me: false,
            });
            return;
        }
        let fires_at_ms = now_ms + self.reply_delay_ms();
        let to = clamp_str(to, MAX_CALL_LEN);
        let text = clamp_str(text, MAX_TEXT_LEN);
        let display = format!("{}: {text}", self.base());
        self.pending.push(Pending {
            origin,
            to: to.clone(),
            text,
            display: display.clone(),
            fires_at_ms,
            freq_hint,
        });
        actions.push(StationAction::ReplyPending {
            origin,
            to: to.to_string(),
            display,
            fires_at_ms,
        });
    }

    fn reply_delay_ms(&self) -> u64 {
        if self.cfg.reply_delay_ms != 0 {
            self.cfg.reply_delay_ms
        } else {
            self.cfg.speed.period_s() as u64 * 1000 + 2000
        }
    }

    // ---- the clock tick ---------------------------------------------------------------------

    /// Once per second: advance the HB schedule and check the idle watchdog.
    pub fn tick(&mut self, now_ms: u64) -> Vec<StationAction> {
        let mut actions = Vec::new();
        // reclaim aged state each tick (inbox 48 h expiry + caps, allcall interval prune).
        self.prune_inbox(now_ms, &mut actions);
        self.prune_allcall(now_ms);
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
        // per period is the whole rule.
        if self.cq_on && self.outbox.is_empty() && self.pending.is_empty() {
            if let Some(next) = self.cq_next_ms {
                if now_ms >= next {
                    let _ = self.enqueue_cq();
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
        self.stop_cq_repeat();
        self.outbox.clear();
        self.pending.clear();
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
        // release fired pending replies into the outbox (oldest first).
        self.pending.sort_by_key(|p| p.fires_at_ms);
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].fires_at_ms <= period_start_ms {
                let p = self.pending.remove(i);
                if let Ok(out) = self.compose_out(p.origin, &p.text, p.freq_hint) {
                    // If the outbox is full, the reply is dropped rather than queued behind stale
                    // traffic (bounded downstream of the surfaced `MAX_PENDING` cap).
                    let _ = self.push_outbox(out);
                }
            } else {
                i += 1;
            }
        }
        // A heartbeat due in THIS period goes out in it. Its deadline is a period boundary
        // + 1 s; JS8Call's 1 Hz `checkRepeat` (mainwindow.cpp:5723) fires within a second after
        // that boundary and its late-start rule (`guiUpdate`, :4512) keys it in the same period.
        // Nexus keys at the boundary, so the boundary releases it. A SCHEDULE, never a queue:
        // only into an EMPTY outbox with nothing pending.
        if self.hb_on && self.outbox.is_empty() && self.pending.is_empty() {
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
            origin: Origin::Operator,
            display: format!("{}: {line}", self.base()),
            frames: seq,
            cursor: 0,
            freq_hint: FreqHint::Dial,
        };
        Ok(if self.push_outbox(out) { n } else { 0 })
    }

    pub fn call_cq(&mut self, idx: u8, now_ms: u64) -> Result<(), ComposeError> {
        self.mark_active(now_ms);
        let out = self.compose_cq(idx, Origin::Operator)?;
        self.push_outbox(out);
        Ok(())
    }

    /// The CQ line both the operator's click and the repeat schedule send, differing only in
    /// `origin`. Shared so a change to the wire text can never drift between them.
    fn compose_cq(&self, idx: u8, origin: Origin) -> Result<OutMsg, ComposeError> {
        let cqs = crate::proto::alphabet::CQS[(idx & 7) as usize];
        // The 4-character square, as JS8Call's CQ carries it (its default CQ text is
        // `CQ CQ CQ <MYGRID4>`, and `sendCQ` falls back to `my_grid().left(4)`,
        // mainwindow.cpp:6344): a longer locator spills into a data frame that keys on the
        // next period.
        let grid4: String = self.cfg.grid.chars().take(4).collect();
        let line = format!("{cqs} {grid4}");
        let seq = frames_with_grid(
            &self.cfg.mycall,
            &self.cfg.grid,
            None,
            line.trim(),
            self.cfg.speed,
        )?;
        Ok(OutMsg {
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
    fn enqueue_cq(&mut self) -> Result<(), ComposeError> {
        let out = self.compose_cq(self.cq_idx, Origin::CqRepeat)?;
        self.push_outbox(out);
        Ok(())
    }

    pub fn heartbeat_now(&mut self, now_ms: u64) -> Result<(), ComposeError> {
        self.mark_active(now_ms);
        self.enqueue_heartbeat(now_ms)
    }

    fn enqueue_heartbeat(&mut self, _now_ms: u64) -> Result<(), ComposeError> {
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
            origin: Origin::Heartbeat,
            display: format!("{}: @HB {line}", self.base()),
            frames: seq,
            cursor: 0,
            freq_hint: FreqHint::HbSubband(0.0),
        };
        self.push_outbox(out);
        Ok(())
    }

    pub fn set_hb(&mut self, on: bool, now_ms: u64) {
        self.hb_on = on;
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

    /// Arm/disarm the repeating CQ with the CQS variant to send (JS8Call's checkable
    /// `cqMacroButton`). `set_hb`'s shape exactly: the first CQ fires now (interval 0 = "on
    /// demand" = once) or after the interval, and disarming clears the schedule. Arming keys
    /// NOTHING by itself — it only schedules; `plan_js8_tx` still re-reads the TX latch.
    pub fn set_cq(&mut self, on: bool, idx: u8, now_ms: u64) {
        self.cq_on = on;
        if on {
            self.cq_idx = idx & 7;
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

    /// JS8Call's `resetCQTimer(stop = true)` (mainwindow.cpp:3711): traffic addressed to us
    /// STOPS the repeating CQ outright — somebody answered, so stop calling. (It only pushes
    /// the heartbeat timer out; the heartbeat is a beacon, the CQ is a call.)
    fn stop_cq_repeat(&mut self) {
        self.cq_on = false;
        self.cq_next_ms = None;
    }

    pub fn cancel_pending_reply(&mut self) {
        self.pending.clear();
    }

    pub fn drop_queue(&mut self) {
        self.outbox.clear();
        self.pending.clear();
    }

    pub fn note_tx_done(&mut self, f: &TxFrame, now_ms: u64) {
        // Display state, updated for EVERY origin.
        self.last_tx_display = Some(f.display.clone());
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
        self.pending.clear();
        self.hb_on = false;
        self.hb_next_ms = None;
        self.stop_cq_repeat();
    }

    /// Reset the idle-watchdog baseline to `now_ms` (and clear any standing trip). Called
    /// internally by every operator send, and PUBLICLY by the engine when the operator
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

    /// Whole minutes since the operator last acted: every operator send and entering the tier
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

    pub fn clear_idle_trip(&mut self) {
        self.idle_tripped = false;
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
        }
    }

    pub fn restore(&mut self, s: StationSnapshot, _now_ms: u64) {
        self.inbox = s.inbox;
        self.heard = s.heard;
        self.allcall_replied = s.allcall_replied.into_iter().collect();
        self.next_inbox_id = s.next_inbox_id.max(1);
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

fn is_query_msg(m: &Message) -> bool {
    m.text.trim_start().to_ascii_uppercase().starts_with("MSG ") || query_msg_id(m).is_some()
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

fn query_msg_id(m: &Message) -> Option<u32> {
    let t = m.text.trim();
    let rest = t.strip_prefix("MSG ").or_else(|| t.strip_prefix("MSGS "))?;
    rest.split_whitespace().next()?.parse().ok()
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

    fn cfg() -> StationConfig {
        StationConfig {
            mycall: "KD9TAW".into(),
            grid: "EN52".into(),
            speed: Speed::Normal,
            reply_delay_ms: 1000,
            ..Default::default()
        }
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

    /// Drain the station: run next_frame at `t` and collect the emitted frame, if any.
    fn drain(s: &mut Station, t: u64) -> Option<TxFrame> {
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
        // pending until the countdown; then an HbAck frame on the dial.
        assert!(
            drain(&mut s, 1500).is_none(),
            "reply waits for its countdown"
        );
        let f = drain(&mut s, 5000).expect("HB-ack after the delay");
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

    /// Store a `MSG TO:` for `call` at my station (W1AW leaves it, addressed to me).
    fn store_for(s: &mut Station, call: &str, t: u64) {
        let body = format!("{call} FRIDAY CONTACT");
        s.on_event(
            &directed("W1AW", "KD9TAW", Some(Command::MsgTo), None, &body, -5),
            t,
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
        // K1ABC requests message n → delivered, marked Delivered immediately on the event.
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
        assert_eq!(s.inbox()[0].state, InboxState::Delivered);
        assert!(
            flush(&mut s, 400_000)
                .iter()
                .any(|d| d.contains("K1ABC MSG FRIDAY CONTACT FROM W1AW")),
            "delivery not transmitted"
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
        s.set_cq(true, 0, 0);
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
        s.set_cq(true, 0, 0);
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
        s.set_cq(true, 0, 0);
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
        s.set_cq(true, 3, 0);
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
        assert_eq!(s.pending_len(), 1, "precondition: a reply is pending");
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
        s.call_cq(0, 0).unwrap();
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
        s.set_cq(true, 0, 0);
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
        s.call_cq(0, 0).unwrap();
        assert_eq!(keyed_wire(&mut s, 0), one_cq_frame("EN52"));
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
        let mut rng = || 3u32; // the first draw: 500 + 50 × 3
        let f = s
            .next_frame(5000, &|_| false, &mut rng)
            .expect("the HB-ACK after its countdown");
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

    #[test]
    fn inbox_bytes_and_48h_expiry_are_enforced() {
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
        // 48 h expiry: a stored message older than the TTL is reclaimed on the next tick.
        let mut s2 = Station::new(cfg());
        s2.on_event(
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
        assert_eq!(s2.inbox().len(), 1);
        s2.tick(1000 + 48 * 60 * 60 * 1000 + 1);
        assert!(
            s2.inbox().is_empty(),
            "48 h expiry did not reclaim the entry"
        );
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

    /// The texts of the automatic replies counting down, oldest first.
    fn waiting(s: &Station) -> Vec<String> {
        s.pending.iter().map(|p| p.text.clone()).collect()
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
        assert!(
            acts.iter().any(|a| matches!(
                a,
                StationAction::ReplyPending { origin: Origin::AutoReply, to, display, .. }
                    if to == "W1AW" && display == "KD9TAW: W1AW ACK"
            )),
            "no ACK counts down: {acts:?}"
        );
        assert!(
            drain(&mut s, 5_500).is_none(),
            "the ACK waits for its countdown"
        );
        let f = drain(&mut s, 6_000).expect("the ACK after the countdown");
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
    /// (`processTxQueue`, :9674-9685). Once the idle watchdog trips, which turns autoreply off
    /// and drops what waits, nothing is sent either (:8818, :11000, :11005).
    #[test]
    fn no_ack_with_autoreply_off_or_once_the_idle_watchdog_trips() {
        let msg = directed("W1AW", "KD9TAW", Some(Command::Msg), None, "HELLO", -5);
        let mut c = cfg();
        c.autoreply = false;
        let mut s = Station::new(c);
        s.on_event(&msg, 1_000);
        assert_eq!(s.inbox().len(), 1, "control: the MSG is filed");
        assert!(waiting(&s).is_empty(), "autoreply off: no ACK");
        assert!(drain(&mut s, 60_000).is_none(), "…and nothing goes out");

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

    /// A copy of a MSG heard while its ACK still waits, counting down or queued behind other
    /// traffic, draws no second ACK: JS8Call's waiting ACK sits in its compose box until it has
    /// gone, and no reply is queued while that box holds text (:9365). Every copy is filed, as
    /// JS8Call files it, and a MSG from another station meanwhile has its own ACK. A copy heard
    /// once the ACK has gone is answered again, as JS8Call answers it: its sender sent it again.
    #[test]
    fn a_copy_of_a_msg_heard_while_its_ack_waits_draws_no_second_ack() {
        let mut s = Station::new(cfg());
        let msg = directed("W1AW", "KD9TAW", Some(Command::Msg), None, "HELLO", -5);
        s.send(None, "FIRST", 0)
            .expect("an operator message ahead of the ACK");
        s.on_event(&msg, 1_000);
        s.on_event(&msg, 1_500); // a copy while the ACK counts down
        assert_eq!(waiting(&s), ["W1AW ACK"], "one ACK for two copies");
        let f = drain(&mut s, 2_000).expect("the operator's message keys first");
        assert_eq!(f.origin, Origin::Operator);
        assert!(waiting(&s).is_empty(), "the ACK is queued behind it");
        s.on_event(&msg, 2_500);
        assert!(
            waiting(&s).is_empty(),
            "a copy while the ACK is queued draws none"
        );
        s.on_event(
            &directed("N0XYZ", "KD9TAW", Some(Command::Msg), None, "HI", -5),
            2_500,
        );
        assert_eq!(
            waiting(&s),
            ["N0XYZ ACK"],
            "another station's MSG has its own ACK"
        );
        let mut acks = Vec::new();
        let mut t = 3_000;
        while let Some(f) = drain(&mut s, t) {
            if f.display.ends_with(" ACK") {
                acks.push(f.display);
            }
            t += 15_000;
        }
        assert_eq!(
            acks,
            ["KD9TAW: W1AW ACK", "KD9TAW: N0XYZ ACK"],
            "one ACK each"
        );
        assert_eq!(s.inbox().len(), 4, "every copy is filed");
        s.on_event(&msg, t);
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

    /// My mail is bounded apart from mail held for someone else. A flood of MSGs to me is capped
    /// on its own, oldest first, and says so; it never pushes out a message held for another
    /// station, which would change what a later QUERY MSGS is answered. And my mail does not
    /// expire at 48 h: JS8Call keeps it until the operator deletes it.
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
        // Every entry is now over 48 h old.
        s.tick(10_000 + 48 * 60 * 60 * 1000);
        assert_eq!(
            count(&s, InboxState::Store),
            0,
            "held mail still expires at 48 h"
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

    #[test]
    fn pending_and_outbox_refuse_under_a_direct_to_me_flood() {
        let mut s = Station::new(cfg());
        // positive control: a few direct queries all queue.
        for i in 0..5u32 {
            s.on_event(
                &directed(
                    &format!("K{i}ABC"),
                    "KD9TAW",
                    Some(Command::SnrQuery),
                    None,
                    "",
                    -7,
                ),
                i as u64,
            );
        }
        assert_eq!(s.pending_len(), 5);
        // flood direct-to-me (NOT rate-limited like @ALLCALL) → pending is capped and refusals
        // are surfaced as toasts, never grown.
        let mut refused = false;
        for i in 0..5000u32 {
            let acts = s.on_event(
                &directed(
                    &format!("N{i:04}"),
                    "KD9TAW",
                    Some(Command::SnrQuery),
                    None,
                    "",
                    -7,
                ),
                100 + i as u64,
            );
            if acts.iter().any(|a| matches!(a, StationAction::Toast { text, .. } if text.contains("Reply queue full"))) { refused = true; }
        }
        assert!(
            refused,
            "flood exceeded the cap but no refusal was surfaced"
        );
        assert!(
            s.pending_len() <= 32,
            "pending unbounded: {}",
            s.pending_len()
        );
        // drain does not let the outbox grow past its cap either.
        let mut rng = || 0u32;
        for k in 0..200u64 {
            let _ = s.next_frame(1_000_000 + k * 15000, &|_| false, &mut rng);
        }
        assert!(s.outbox_len() <= 32, "outbox unbounded: {}", s.outbox_len());
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
}
