//! Station — the JS8 protocol brain (mainwindow.cpp:7740-9400 + Inbox.cpp, read as the spec).
//!
//! A PURE state machine: it never sees audio, never keys, never reads a clock or an RNG of its
//! own. The engine drives it — `on_event` routes one reassembler event through JS8Call's gating
//! chain, `tick(now_ms)` advances the heartbeat schedule and the idle watchdog, and the operator
//! verbs (`send`, `send_command`, `call_cq`, `heartbeat_now`, …) enqueue intent. Everything the
//! station wants to transmit leaves through EXACTLY ONE seam:
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
//! - Heartbeat: `MYCALL: HEARTBEAT GRID` as a type-000 frame on a random free 50 Hz slot in
//!   500..=1000 Hz (`FreqHint::HbSubband`); the interval timer resets when directed traffic to me
//!   is displayed (mainwindow.cpp:7740). Replies/ACKs never QSY (`FreqHint::Dial`).
//! - HB-ACK (default off): only with HB + autoreply + `hb_ack`, an empty outbox and no QSO pause,
//!   answer a heard heartbeat `CALL HEARTBEAT SNR +NN [MSG ID n]`. An incoming `HEARTBEAT SNR` is
//!   never answered (no ack-of-an-ack loop).
//! - Autoreply: only to my own call, `@ALLCALL`, and joined groups; only the autoreply subset
//!   (`Command::is_autoreply`, = upstream `autoreply_cmds {0,2,3,4,6,9,10,11,12,13,14,16,30}`);
//!   `@ALLCALL` replies are rate-limited to one per station per `allcall_reply_interval_ms`.
//! - Relay (`>`): retransmit `rest *DE* MYCALL`; at the final hop parse the chain to `A>B>C` and
//!   answer `A>B>C ACK` (unless the embedded text is itself an autoreply command).
//! - Store-and-forward: `MSG TO:` stores a `Store` inbox row keyed to the base callsign; the next
//!   heartbeat/query from that call is offered `MSG ID n`; `QUERY MSG n` delivers it and marks it
//!   `Delivered`.
//! - Idle watchdog: on trip, stop TX and turn autoreply/relay/HB OFF, clear the outbox, surface a
//!   toast — `tx_enabled` (the engine's latch) is NOT touched.
use crate::phy::{Speed, Word87, I3};
use crate::proto::callsign::{split_portable, CallRef};
use crate::proto::command::Command;
use crate::proto::compose::{frames, ComposeError};
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
// OLDEST and toast; never silently. `heard` is one row per station: a busy band holds a few
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

/// Which automatic (or operator) class produced a frame — the gate key B7 uses. CQ counts as
/// Operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    Operator,
    Heartbeat,
    HbAck,
    AutoReply,
    Relay,
}

/// Where a frame wants to transmit. `HbSubband` is a random free 50 Hz slot, 500..=1000 Hz;
/// everything else stays on the dial (replies/ACK/HB-ack never QSY).
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
    pub idle_watchdog_min: u16,
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
            idle_watchdog_min: 60,
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
        let to_allcall = m.to_text.eq_ignore_ascii_case("@ALLCALL");
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
            self.last_activity_ms = now_ms; // directed traffic to me resets the idle + HB timers
            self.bump_hb_schedule(now_ms);
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

        // Directed command autoreply / relay / store-and-forward.
        if let Some(cmd) = m.cmd {
            if self.cfg.relay && cmd == Command::Relay {
                self.handle_relay(m, now_ms, &mut actions);
                return actions;
            }
            if cmd == Command::MsgTo {
                self.handle_msg_to(m, now_ms, &mut actions);
                return actions;
            }
            if cmd == Command::QueryMsgs || is_query_msg(m) {
                self.handle_query_msg(m, now_ms, &mut actions);
                return actions;
            }
            if self.cfg.autoreply
                && cmd.is_autoreply()
                && (to_me || to_allcall || to_group)
                && !self.is_me(&m.from)
            {
                self.autoreply(m, cmd, to_allcall, now_ms, &mut actions);
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

    /// Expire inbox entries older than 48 h, then, while over the count or byte cap, drop the
    /// OLDEST (by `at_ms`) and toast — never silently. Returns whether anything changed.
    fn prune_inbox(&mut self, now_ms: u64, actions: &mut Vec<StationAction>) {
        let before = self.inbox.len();
        self.inbox
            .retain(|e| now_ms.saturating_sub(e.at_ms) < INBOX_TTL_MS);
        let mut evicted = before != self.inbox.len();
        while self.inbox.len() > MAX_INBOX || self.inbox_bytes() > MAX_INBOX_BYTES {
            let Some((idx, _)) = self.inbox.iter().enumerate().min_by_key(|(_, e)| e.at_ms) else {
                break;
            };
            self.inbox.remove(idx);
            evicted = true;
        }
        if evicted {
            actions.push(StationAction::InboxChanged);
            actions.push(StationAction::Toast {
                text: "Inbox full: oldest stored messages evicted".into(),
                directed_to_me: false,
            });
        }
    }

    fn inbox_bytes(&self) -> usize {
        self.inbox
            .iter()
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
        self.schedule_reply(Origin::HbAck, &to, &text, FreqHint::Dial, now_ms, actions);
    }

    fn autoreply(
        &mut self,
        m: &Message,
        cmd: Command,
        to_allcall: bool,
        now_ms: u64,
        actions: &mut Vec<StationAction>,
    ) {
        let from = split_portable(&m.from).0.to_ascii_uppercase();
        if to_allcall {
            // rate-limit @ALLCALL replies to one per station per interval.
            if let Some(&last) = self.allcall_replied.get(&from) {
                if now_ms.saturating_sub(last) < self.cfg.allcall_reply_interval_ms {
                    actions.push(StationAction::RateLimited { from });
                    return;
                }
            }
            self.allcall_replied.insert(from.clone(), now_ms);
            self.prune_allcall(now_ms); // keep the map self-limiting, not one entry per call ever
        }
        let reply = match cmd {
            Command::SnrQuery => Some(format!("{from} SNR {}", format_snr(m.snr_db))),
            Command::GridQuery => Some(format!("{from} GRID {}", self.cfg.grid)),
            Command::InfoQuery => Some(format!("{from} INFO {}", self.cfg.info)),
            Command::StatusQuery => Some(format!("{from} STATUS {}", self.status_text())),
            Command::HearingQuery => Some(format!("{from} HEARING {}", self.hearing_text())),
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

    fn handle_query_msg(&mut self, m: &Message, now_ms: u64, actions: &mut Vec<StationAction>) {
        let from = split_portable(&m.from).0.to_ascii_uppercase();
        // `QUERY MSG n` delivers the stored message n (if it is for the querier) and marks it.
        if let Some(id) = query_msg_id(m) {
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
                return;
            }
        }
        // `QUERY MSGS` (do you have any for me?) → offer the first stored id, else NO.
        if self.cfg.autoreply && self.addressed_to_me(&m.to_text) {
            let reply = match self.stored_for(&from) {
                Some(id) => format!("{from} YES MSG ID {id}"),
                None => format!("{from} NO"),
            };
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

    fn stored_for(&self, call: &str) -> Option<u32> {
        self.inbox
            .iter()
            .find(|e| e.state == InboxState::Store && e.to.eq_ignore_ascii_case(call))
            .map(|e| e.id)
    }

    fn status_text(&self) -> String {
        if self.cfg.status.is_empty() {
            format!("IDLE {} VERSION Nexus", self.idle_minutes())
        } else {
            clamp_str(&self.cfg.status, MAX_INFO_LEN)
        }
    }

    fn hearing_text(&self) -> String {
        let mut calls: Vec<&Heard> = self.heard.iter().collect();
        calls.sort_by(|a, b| b.last_ms.cmp(&a.last_ms));
        calls
            .iter()
            .take(4)
            .map(|h| h.call.as_str())
            .collect::<Vec<_>>()
            .join(" ")
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
        // heartbeat schedule
        if self.hb_on && self.outbox.is_empty() && self.pending.is_empty() {
            if let Some(next) = self.hb_next_ms {
                if now_ms >= next {
                    let _ = self.enqueue_heartbeat(now_ms);
                    self.bump_hb_schedule(now_ms);
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
        self.outbox.clear();
        self.pending.clear();
        actions.push(StationAction::IdleTripped);
        actions.push(StationAction::Toast {
            text: "Idle watchdog: TX stopped, autoreply/relay/HB off".into(),
            directed_to_me: false,
        });
    }

    fn bump_hb_schedule(&mut self, now_ms: u64) {
        if self.hb_on && self.cfg.hb_interval_min > 0 {
            self.hb_next_ms = Some(now_ms + self.cfg.hb_interval_min as u64 * 60 * 1000);
        } else {
            // interval 0 = "on demand" = ONCE: clear the schedule so `tick` does not
            // re-enqueue a heartbeat every drain cycle, and the cockpit shows no stuck past
            // `hb_next_ms`. Re-arming (`set_hb`) schedules the next on-demand heartbeat.
            self.hb_next_ms = None;
        }
    }

    // ---- the single TX seam -----------------------------------------------------------------

    /// Once per period: release any pending reply whose countdown elapsed, then dispense the
    /// outbox head's next frame. THE ONLY method that hands out a `TxFrame`.
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
        let head = self.outbox.front_mut()?;
        let (frame, i3) = head.frames.get(head.cursor)?.clone();
        let word = encode_frame(&frame, i3, self.cfg.speed).ok()?;
        let freq_hint = match head.freq_hint {
            FreqHint::HbSubband(_) => FreqHint::HbSubband(pick_hb_slot(busy, rng)),
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
        }
        Some(tx)
    }

    fn compose_out(
        &self,
        origin: Origin,
        text: &str,
        freq_hint: FreqHint,
    ) -> Result<OutMsg, ComposeError> {
        let seq = frames(&self.cfg.mycall, None, text, self.cfg.speed)?;
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
        let seq = frames(&self.cfg.mycall, to, text, self.cfg.speed)?;
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
        let line = if arg.is_empty() {
            format!("{} {word}", to.render())
        } else {
            format!("{} {word} {arg}", to.render())
        };
        let seq = frames(&self.cfg.mycall, None, &line, self.cfg.speed)?;
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
        let cqs = crate::proto::alphabet::CQS[(idx & 7) as usize];
        let line = format!("{cqs} {}", self.cfg.grid);
        let seq = frames(&self.cfg.mycall, None, line.trim(), self.cfg.speed)?;
        let out = OutMsg {
            origin: Origin::Operator,
            display: format!("{}: @ALLCALL {line}", self.base()),
            frames: seq,
            cursor: 0,
            freq_hint: FreqHint::Dial,
        };
        self.push_outbox(out);
        Ok(())
    }

    pub fn heartbeat_now(&mut self, now_ms: u64) -> Result<(), ComposeError> {
        self.mark_active(now_ms);
        self.enqueue_heartbeat(now_ms)
    }

    fn enqueue_heartbeat(&mut self, _now_ms: u64) -> Result<(), ComposeError> {
        let line = format!("HEARTBEAT {}", self.cfg.grid);
        let seq = frames(&self.cfg.mycall, None, line.trim(), self.cfg.speed)?;
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
            // next HB fires now (on-demand) or after the interval.
            self.hb_next_ms = Some(now_ms + self.cfg.hb_interval_min as u64 * 60 * 1000);
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

    pub fn idle_minutes(&self) -> u16 {
        0
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

    pub fn snapshot(&self) -> StationSnapshot {
        StationSnapshot {
            inbox: self.inbox.clone(),
            heard: self.heard.clone(),
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

/// Pick a random free 50 Hz slot in 500..=1000 Hz (11 slots): step through pseudo-random slots
/// until `busy` reports one clear, else fall back to the drawn slot (upstream never blocks).
fn pick_hb_slot(busy: &dyn Fn(f32) -> bool, rng: &mut dyn FnMut() -> u32) -> f32 {
    let mut chosen = 500.0 + (rng() % 11) as f32 * 50.0;
    for _ in 0..11 {
        let slot = 500.0 + (rng() % 11) as f32 * 50.0;
        if !busy(slot) {
            return slot;
        }
        chosen = slot;
    }
    chosen
}

fn is_query_msg(m: &Message) -> bool {
    m.text.trim_start().to_ascii_uppercase().starts_with("MSG ") || query_msg_id(m).is_some()
}

/// Parse the `n` from a `QUERY MSG n` body.
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
        assert_eq!(f.freq_hint, FreqHint::Dial, "an HB-ack never QSYs");
        assert_eq!(f.display, "KD9TAW: W1AW HEARTBEAT SNR -05");
        // An incoming HEARTBEAT SNR is never answered (no ack-of-an-ack).
        let mut s2 = Station::new({
            let mut c = cfg();
            c.hb_ack = true;
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
        // hb_ack off → nothing.
        let mut s3 = Station::new(cfg());
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

    #[test]
    fn allcall_replies_are_rate_limited_to_one_per_station_per_interval() {
        let mut s = Station::new(cfg());
        let a1 = s.on_event(
            &directed("W1AW", "@ALLCALL", Some(Command::SnrQuery), None, "", -7),
            1000,
        );
        assert!(a1
            .iter()
            .any(|a| matches!(a, StationAction::ReplyPending { .. })));
        // second request from the same station inside 15 min → rate-limited, no reply queued.
        let a2 = s.on_event(
            &directed("W1AW", "@ALLCALL", Some(Command::SnrQuery), None, "", -7),
            2000,
        );
        assert!(a2
            .iter()
            .any(|a| matches!(a, StationAction::RateLimited { from } if from == "W1AW")));
        // a DIFFERENT station is still answered.
        let a3 = s.on_event(
            &directed("K1ABC", "@ALLCALL", Some(Command::SnrQuery), None, "", -7),
            3000,
        );
        assert!(a3
            .iter()
            .any(|a| matches!(a, StationAction::ReplyPending { .. })));
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

    #[test]
    fn idle_watchdog_trips_and_clears_the_automatic_origins() {
        let mut c = cfg();
        c.idle_watchdog_min = 5;
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        // queue an automatic reply, then let the clock run past the watchdog.
        s.on_event(
            &directed("W1AW", "@ALLCALL", Some(Command::SnrQuery), None, "", -7),
            0,
        );
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
        let snap = s.snapshot();
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
        let mut s = Station::new(cfg());
        // positive control: a few distinct stations inside the interval are all remembered.
        for i in 0..5u32 {
            s.on_event(
                &directed(
                    &format!("K{i}XYZ"),
                    "@ALLCALL",
                    Some(Command::SnrQuery),
                    None,
                    "",
                    -7,
                ),
                i as u64 * 1000,
            );
        }
        assert_eq!(s.allcall_len(), 5);
        // flood distinct callsigns spread over long gaps → entries older than the 15-min interval
        // are dropped, so the map never grows one-per-station-ever.
        for i in 0..5000u32 {
            s.on_event(
                &directed(
                    &format!("W{i:04}"),
                    "@ALLCALL",
                    Some(Command::SnrQuery),
                    None,
                    "",
                    -7,
                ),
                1_000_000 + i as u64 * 1000,
            );
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
}
