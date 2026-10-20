//! JS8 engine adapter — the ONLY place the JS8 message layer (`::js8::proto`) meets the
//! engine and, from the TX batch on, the ONLY place a `proto::TxFrame` becomes a `TxPlan`
//! (spec invariant 13). A CHILD module of `engine` (declared with `#[path]` there) so it
//! reaches the engine's private fields without widening any visibility.
//!
//! WHY a separate file: the JS8 station runs JS8Call's cadence — heartbeats, autoreply,
//! relay, store-and-forward, the 15-min @ALLCALL cap — which shares NOTHING with the Tempo
//! chat arm or the FT sequencers. Keeping it here keeps every JS8Call-behaviour concern out
//! of engine.rs and keeps `Tier::is_chat()` untouched (the Tempo wire is on-air incompatible).
//!
//! The station transmits: `js8_send` and `js8_send_command` queue frames through the station,
//! and `js8_apply_station_config` hands the station the operator's Settings as they are
//! (autoreply, relay and HB-ack included), forcing nothing off. The RX path (`js8_ingest` →
//! `Reassembler` → `Station` → activity/heard/inbox) is unchanged by the transmit side.
//!
//! ⚠️ Inside this file `js8` would name THIS module; the crate is spelled `::js8::…`.
//! The actions match every path funnels through is `js8_handle_actions`.

use std::path::PathBuf;

use ::js8::proto::callsign::{split_portable, CallRef};
use ::js8::proto::reassembly::RxFrame;
use ::js8::proto::station::{heartbeat_speed, FreqHint, InboxState, StationSnapshot};
use ::js8::{Frame, MessageEvent, Origin, RawDecode, StationAction, StationConfig, Word87};
use modes::Js8Speed;

use super::{now_unix_secs, DataFrame, Engine, TxPlan, TxWaveform};
use crate::dto::{
    Js8ActivityRow, Js8Armed, Js8ComposerPrefill, Js8NotArmed, Js8PendingReply, Js8Presence,
    Js8QueueRow, Js8RowFlags, Js8State, Js8Unarmed, SourceKind, Tier,
};
use crate::settings::Settings;

/// Activity rows kept for the cockpit (newest last).
const JS8_ACTIVITY_CAP: usize = 200;
/// JS8Call flags a decode with quality < 0.17 as low-confidence (decodedtext.cpp).
const JS8_LOW_CONF: f32 = 0.17;
/// Row-dedupe depth (multi-speed pass vs boundary pass; see `js8_dedupe`).
const JS8_SEEN_CAP: usize = 64;
/// JS8Call's @ALLCALL reply cap: one reply per station per 15 minutes.
const JS8_ALLCALL_INTERVAL_MS: u64 = 15 * 60 * 1000;
/// JS8Call starts no transmission without a locator in Settings (`ensureCallsignSet`,
/// mainwindow.cpp:5264-5268, "Please enter your grid locator in the settings."). The words are
/// the FT gate's (`structured_tx_ready`).
const JS8_NO_LOCATOR: &str =
    "Set your Maidenhead grid (e.g. EN52) in Settings before transmitting JS8.";
/// …and what it says when a refusal outside the licence's privileges drops what was waiting to
/// go out: a reply or heartbeat falling due, or the queue. The sentence shape of CW's, RTTY's and
/// PSK's own refusals.
const JS8_REFUSED_PRIVILEGES: &str = "JS8 not sent: this frequency is outside your license \
     privileges, so what was waiting to go out was dropped, not held for later.";
/// …and a Yes to an automatic reply that no longer waits for one.
const JS8_REPLY_GONE: &str = "That automatic reply is no longer waiting: it was answered, or its \
     time ran out. Nothing was sent.";
/// …and when transmit going off drops a message the operator queued: RTTY's and PSK's words.
const JS8_QUEUE_REFUSED_TX_OFF: &str = "JS8 stopped: transmit was turned off, so what was still \
     queued was dropped, not held for later. Send it again when you are ready.";
/// …and a heartbeat asked for where JS8Call has no HB button: heartbeat networking off…
const JS8_HB_NEEDS_NETWORKING: &str =
    "No heartbeat: heartbeat networking (HB NET) is off. Turn it on first, as in JS8Call.";
/// …or a speed JS8Call sends no heartbeats at.
const JS8_HB_SPEED: &str = "No heartbeat at this speed: JS8Call sends heartbeats only at Slow, \
     Normal and Fast.";

/// The SECOND act of the two-act rule, by origin: `Autoreply`/`Relay`/`HbAck` are the
/// persisted switches, `Hb` is the session-only heartbeat schedule. `HbNet` is JS8Call's
/// "Enable Heartbeat Networking", persisted, which keys nothing by itself: without it there is no
/// HB button and no HB ACK. Lowercase on the wire (`autoreply | relay | hback | hb | hbnet`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Js8Switch {
    Autoreply,
    Relay,
    HbAck,
    Hb,
    HbNet,
}

/// A callsign list from Settings as the station reads it: trimmed, upper-cased, the empties gone
/// (JS8Call's `splitWords`, Configuration.cpp:2416-2428).
fn js8_call_list(list: &[String]) -> Vec<String> {
    list.iter()
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| !c.is_empty())
        .collect()
}

/// xorshift32 — a deterministic, dependency-free source for the heartbeat sub-band pick.
/// Quality is irrelevant (it spreads HBs across 50 Hz slots); having no new crate is what
/// matters. Never used for anything that could key the radio differently.
fn js8_rng_next(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// JS8Call's word check for the compose box (`js8::proto::suggest::needs_suggestion`): one answer
/// per word, in order, true where JS8Call underlines the word. The JSC dictionary only: no engine
/// state, and it keys nothing.
pub fn js8_word_check(words: &[String]) -> Vec<bool> {
    words
        .iter()
        .map(|w| ::js8::proto::suggest::needs_suggestion(w))
        .collect()
}

/// What JS8Call's right-click offers for an underlined word (`js8::proto::suggest::suggestions`):
/// up to five cheaper words, cheapest first. The JSC dictionary only; it keys nothing.
pub fn js8_word_suggestions(word: &str) -> Js8WordSuggestions {
    ::js8::proto::suggest::suggestions(word, ::js8::proto::suggest::OFFERED)
}

pub use ::js8::proto::suggest::Suggestions as Js8WordSuggestions;

impl Engine {
    /// ONE place Settings → `StationConfig` (an associated fn on `&Settings`, so the arm
    /// verbs and `js8_apply_station_config` build it from the SAME source the persisted
    /// switches live in). Identity is uppercased and trimmed the way the wire packs it; the
    /// idle floor of 5 minutes is applied here (0 stays 0 = off); the speed is the TRANSMIT
    /// speed (`js8_speed`, degraded to Normal on a stale index — the same rule `js8_tx_speed`
    /// applies).
    pub(crate) fn js8_station_config(s: &Settings) -> StationConfig {
        let speed = Js8Speed::from_index(s.js8_speed).unwrap_or(Js8Speed::Normal);
        StationConfig {
            mycall: s.mycall.trim().to_ascii_uppercase(),
            grid: s.mygrid.trim().to_ascii_uppercase(),
            speed,
            autoreply: s.js8_autoreply,
            relay: s.js8_relay,
            hb_ack: s.js8_hb_ack,
            hb_interval_min: s.js8_hb_interval_min,
            cq_interval_min: s.js8_cq_interval_min,
            idle_watchdog_min: match s.js8_idle_watchdog_min {
                0 => 0,
                m => m.max(5),
            },
            callsign_aging_min: s.js8_callsign_aging_min,
            // A group JS8Call will not let be joined (@APRSIS, @JS8NET) is never joined here,
            // whatever put it in Settings: an older Nexus accepted one. (The Remote cannot write
            // the list: `js8Groups` is denied to every Remote write.) Settings keeps it as
            // written, and the panel refuses every Save while one is in the field.
            groups: s
                .js8_groups
                .iter()
                .map(|g| g.trim().to_ascii_uppercase())
                .filter(|g| !g.is_empty() && ::js8::proto::callsign::may_join_group(g))
                .collect(),
            info: s.js8_info.clone(),
            status: s.js8_status.clone(),
            cq_text: s.js8_cq_text.clone(),
            reply_text: s.js8_reply_text.clone(),
            allcall_reply_interval_ms: JS8_ALLCALL_INTERVAL_MS,
            autoreply_confirmation: s.js8_autoreply_confirmation,
            autoreply_allow: js8_call_list(&s.js8_autoreply_allow),
            autoreply_deny: js8_call_list(&s.js8_autoreply_deny),
            hb_ack_deny: js8_call_list(&s.js8_hb_ack_deny),
            hb_networking: s.js8_hb_networking,
            hb_anywhere: s.js8_hb_anywhere,
            hb_qso_pause: s.js8_hb_qso_pause,
            hb_rate_limit: s.js8_hb_ack_rate_limit,
            avoid_allcall: s.js8_avoid_allcall,
        }
    }

    /// The transmit speed the `js8_speed` setting resolves to (degrade-don't-refuse).
    fn js8_tx_speed_setting(&self) -> Js8Speed {
        match self.tier_mode_kind(Tier::Js8) {
            Some(modes::ModeKind::Js8 { speed }) => speed,
            _ => Js8Speed::Normal,
        }
    }

    /// The speed the station TRANSMITS at: the persisted `js8_speed` index, degraded to
    /// Normal on a stale or hand-edited value — the same degrade-don't-refuse rule
    /// `Tier::js8_kind` applies to the decoder, so TX and RX can never disagree.
    pub(crate) fn js8_tx_speed(&self) -> modes::Js8Speed {
        modes::Js8Speed::from_index(self.settings().js8_speed).unwrap_or(modes::Js8Speed::Normal)
    }

    /// Push the current Settings into the station. Called by `apply_settings`, by every
    /// speed/mask change and by `js8_enter`. B7 removed the B5 receive-only override (the
    /// three lines that forced the station's autoreply/relay/HB-ack OFF): the station now
    /// carries the operator's real switches, so `Js8State.armed` and actual station
    /// behaviour agree. Nothing keys without the TX latch regardless — `plan_js8_tx` gates
    /// on it — so the two-act rule still holds.
    pub(crate) fn js8_apply_station_config(&mut self) {
        let cfg = Self::js8_station_config(&self.settings);
        if self.js8_station.config() != &cfg {
            self.js8_station.set_config(cfg);
            // Heartbeat networking off or a speed with no heartbeats stops the HB repeat in the
            // station; the dock reads it from here.
            self.js8_hb_on = self.js8_station.hb_on();
        }
    }

    /// View entry: the station reads Settings, then `set_tier(Js8)` — which is a complete
    /// no-op on the same tier, and on a real switch flushes the decode context, swaps the
    /// decoder to the JS8 window and retunes to `js8_band_plan()` for the current band
    /// (stay-on-miss). KEYS NOTHING: the TX latch is untouched and `tier_is_rx_only`
    /// refuses to arm it anyway.
    pub fn js8_enter(&mut self) {
        // "Return to the last used frequency" (off by default): read before the tier switch
        // retunes to the band's JS8 frequency, put back after it (`js8_freq`).
        let back_to = self.js8_return_dial();
        self.js8_start_session();
        self.set_tier(Tier::Js8);
        if let Some(mhz) = back_to {
            self.js8_return_to(mhz);
        }
    }

    /// Shared session-entry effects before the native decoder transition.
    /// Remote performs that transition under the existing decoder mutex too.
    pub(crate) fn js8_start_session(&mut self) {
        self.js8_apply_station_config();
        // Entering the view is the session start: seed the idle-watchdog baseline to now so
        // the operator's first decode doesn't read as decades idle and trip the watchdog
        // (a freshly built Station has `last_activity_ms == 0`).
        self.js8_station.mark_active(now_unix_secs() * 1000);
    }

    /// Borrow the native heard list for display joins without copying inbox or queue.
    pub fn js8_heard(&self) -> &[::js8::proto::station::Heard] {
        self.js8_station.heard()
    }

    /// A bounded copy of the ordinary display DTO. The station's retained data,
    /// decoder, idle clock, queue and transmit policy are never changed by a read.
    pub fn bounded_js8_state(&self) -> Option<Js8State> {
        const TEXT: usize = 1024;
        if self.js8_activity.len() > 200
            || self.js8_station.heard().len() > 500
            || self.js8_station.inbox().len() > 100
        {
            return None;
        }
        let mut bytes = self.js8_station.display_queue_budget(2048, TEXT)?;
        let mut add = |s: &str| {
            bytes += s.len();
            s.len() <= TEXT && bytes <= 192 * 1024
        };
        for row in &self.js8_activity {
            if !add(&row.from) || !add(&row.text) {
                return None;
            }
        }
        for row in self.js8_station.heard() {
            if row.call.len() > 32 || !add(&row.call) || !add(row.grid.as_deref().unwrap_or("")) {
                return None;
            }
        }
        for row in self.js8_station.inbox() {
            if row.path.len() > 8
                || !add(&row.from)
                || !add(&row.to)
                || !add(&row.text)
                || !row.path.iter().all(|hop| add(hop))
            {
                return None;
            }
        }
        // The Inbox's notes of what a cap dropped: ten at most, each an inbox row.
        let dropped = self.js8_station.inbox_dropped();
        if dropped.len() > 10 {
            return None;
        }
        for d in dropped {
            let row = &d.entry;
            if row.path.len() > 8
                || !add(&row.from)
                || !add(&row.to)
                || !add(&row.text)
                || !row.path.iter().all(|hop| add(hop))
            {
                return None;
            }
        }
        if !add(self.js8_last_error.as_deref().unwrap_or("")) {
            return None;
        }
        // The desktop's own keys ride as they are, each listed by the page's parser: `presence`,
        // `controls`, `inboxTooLong`, each row's place in its message (`flags`) and each station's
        // ★ and ☎ (`ackMs`, `cqMs`). Their text is inside the page's bounds as the station keeps
        // it: at most 500 calls the 55-minute rule stopped, each held to 32 bytes, and a band
        // schedule description of at most 80 characters.
        Some(self.js8_state())
    }

    /// The cockpit poll. Every field is engine truth at poll time; `armed` is, per origin, the
    /// switch on and the station's own answer that it would act now (`Station::readiness`, with
    /// the TX latch) — never the switch alone — and `not_armed` names why for a switch that is on.
    pub fn js8_state(&self) -> Js8State {
        let s = &self.settings;
        let idle_tripped = self.js8_station.idle_tripped();
        let cq_on = self.js8_station.cq_on();
        let ready = self.js8_station.readiness(self.tx_enabled);
        // A switch that is off has no reason: its chip reads off.
        let why = |on: bool, why: Option<Js8Unarmed>| if on { why } else { None };
        let not_armed = Js8NotArmed {
            autoreply: why(s.js8_autoreply, ready.autoreply),
            relay: why(s.js8_relay, ready.relay),
            hb_ack: why(s.js8_hb_ack, ready.hb_ack),
            hb: why(self.js8_hb_on, ready.hb),
            cq: why(cq_on, ready.cq),
        };
        Js8State {
            speed: self.js8_tx_speed_setting(),
            rx_speeds: s.js8_rx_speeds,
            controls: Some(crate::dto::Js8Controls {
                filter: s.js8_filter,
                qsy_center_hz: s.js8_qsy_center_hz,
                hop: self.js8_hop_notice(),
            }),
            tx_enabled: self.tx_enabled,
            sending: self.app.transmitting(),
            hb_on: self.js8_hb_on,
            hb_next_at_ms: self.js8_station.hb_next_ms(),
            hb_interval_min: s.js8_hb_interval_min,
            // Read STRAIGHT off the station — no engine-side mirror to drift. The CQ repeat
            // is stopped from three places the engine never sees (the idle trip, `halt`, and
            // JS8Call's "somebody answered, stop calling" on directed RX), and a mirror would
            // have to be kept in step with each of them.
            cq_on,
            cq_next_at_ms: self.js8_station.cq_next_ms(),
            cq_interval_min: s.js8_cq_interval_min,
            autoreply: s.js8_autoreply,
            relay: s.js8_relay,
            hb_ack: s.js8_hb_ack,
            armed: Js8Armed {
                autoreply: s.js8_autoreply && not_armed.autoreply.is_none(),
                relay: s.js8_relay && not_armed.relay.is_none(),
                hb_ack: s.js8_hb_ack && not_armed.hb_ack.is_none(),
                hb: self.js8_hb_on && not_armed.hb.is_none(),
                cq: cq_on && not_armed.cq.is_none(),
            },
            not_armed,
            idle_minutes: self
                .js8_station
                .idle_minutes(tempo_core::timing::now_unix_ms() as u64),
            idle_limit_min: self.js8_station.config().idle_watchdog_min,
            idle_tripped,
            activity: self.js8_activity.iter().cloned().collect(),
            stations: self.js8_station.heard().to_vec(),
            inbox: self.js8_station.inbox().to_vec(),
            inbox_dropped: self.js8_station.inbox_dropped().to_vec(),
            inbox_too_long: self.js8_station.held_too_long(),
            queue: self
                .js8_station
                .queue()
                .into_iter()
                .map(|q| Js8QueueRow {
                    origin: q.origin,
                    display: q.display,
                    first: q.first,
                    last: q.last,
                })
                .collect(),
            pending_reply: self.js8_station.pending_reply().map(|p| Js8PendingReply {
                origin: p.origin,
                to: p.to,
                display: p.display,
                fires_at_ms: p.fires_at_ms,
            }),
            last_error: self.js8_last_error.clone(),
            presence: Some(Js8Presence {
                hb_networking: s.js8_hb_networking,
                hb_speed: heartbeat_speed(self.js8_tx_speed()),
                hb_paused: self.js8_station.hb_paused(),
                // Said only of an HB ACK that is armed: one that is not says why on its chip.
                hb_ack_suppressed: if s.js8_hb_ack && not_armed.hb_ack.is_none() {
                    self.js8_station.hb_ack_suppressed()
                } else {
                    None
                },
                hb_ack_rate_blocked: self.js8_station.hb_rate_blocked().to_vec(),
                qso_pause: s.js8_hb_qso_pause,
                avoid_allcall: s.js8_avoid_allcall,
            }),
        }
    }

    /// Row-level dedupe between the multi-speed pass (JS8Call's decode moment, ~1-2 s before
    /// the boundary) and the boundary pass (the same audio, re-decoded at the boundary): the
    /// SAME (speed, word) within ¾ of that speed's period is the same transmission. A
    /// genuine repeat is a period later and passes. Runs at the row chokepoint, so ALL.TXT,
    /// the roster, the decode rows and the station all see each word ONCE. Non-JS8 rows pass
    /// through untouched.
    pub(crate) fn js8_dedupe(&mut self, decodes: Vec<modes::Decode>) -> Vec<modes::Decode> {
        let now_ms = now_unix_secs() * 1000;
        let mut keep = Vec::with_capacity(decodes.len());
        for d in decodes {
            let (Some(raw), Some(modes::ModeKind::Js8 { speed })) = (d.raw, d.mode) else {
                keep.push(d);
                continue;
            };
            let window_ms = u64::from(speed.period_s()) * 750;
            let dup = self.js8_seen.iter().any(|(s, at, w)| {
                *s == speed && *w == raw && now_ms.saturating_sub(*at) <= window_ms
            });
            if dup {
                continue;
            }
            self.js8_seen.push_back((speed, now_ms, raw));
            while self.js8_seen.len() > JS8_SEEN_CAP {
                self.js8_seen.pop_front();
            }
            keep.push(d);
        }
        keep
    }

    /// RX ingest from `process_decodes` at `Tier::Js8`: each row's typed word → `RawDecode` →
    /// the reassembler (ages first, so a stale buffer closes before this cycle's frames can be
    /// mistaken for its continuation) → the station → activity rows / heard / inbox. Rows
    /// were deduped upstream (`js8_dedupe`); rows without `raw` are not JS8 and are skipped.
    pub fn js8_ingest(&mut self, decodes: &[modes::Decode], _slot: u64) {
        let now_ms = now_unix_secs() * 1000;
        self.js8_tick(now_ms);
        // The whole batch into the buffers first, as JS8Call files a decode pass before
        // `processCommandActivity` reads it; the station then sees which buffers are still open
        // (a message still arriving), and its queue runs once the pass is processed, as
        // `processTxQueue` does (mainwindow.cpp:4734-4737).
        let mut batch = Vec::new();
        for d in decodes {
            let (Some(raw), Some(modes::ModeKind::Js8 { speed })) = (d.raw, d.mode) else {
                continue;
            };
            let rx = RawDecode {
                speed,
                freq_hz: d.freq,
                dt_s: d.dt,
                snr_db: d.snr,
                sync: d.sync,
                word: Word87::from_bytes(raw),
                nharderrors: 0,
                quality: d.qual,
            };
            let low_conf = d.qual < JS8_LOW_CONF;
            batch.push((self.js8_reasm.feed(&rx, now_ms), low_conf));
        }
        self.js8_note_rx_buffers();
        for (events, low_conf) in batch {
            self.js8_handle_events(events, low_conf, now_ms);
        }
        let actions = self.js8_station.process_tx_queue();
        self.js8_handle_actions(actions);
    }

    /// Tell the station which receive buffers are open, for JS8Call's "a message still arriving"
    /// rules (mainwindow.cpp:9062-9066, :9369-9374).
    fn js8_note_rx_buffers(&mut self) {
        let open = self.js8_reasm.has_open();
        let to: Vec<String> = self.js8_reasm.open_heads_to().map(str::to_string).collect();
        self.js8_station
            .note_rx_buffers(open, to.iter().map(String::as_str));
    }

    /// The engine's once-a-second JS8 clock (the radio loop calls it at `Tier::Js8`; ingest
    /// calls it first): buffer ageing, then the station's own tick (HB schedule, idle
    /// minutes — inert in the receive-only build, but the plumbing is the TX batch's).
    pub fn js8_tick(&mut self, now_ms: u64) {
        let aged = self.js8_reasm.age(now_ms);
        self.js8_note_rx_buffers();
        self.js8_handle_events(aged, false, now_ms);
        // What waits while the TX latch is DOWN is dropped, never carried: arming TX ten
        // minutes later must not fire a reply to a query nobody is waiting for. (A reply asking
        // for the operator's Yes stays shown, "would reply … TX is off"; a Yes queues it, and the
        // queue is dropped here.) No locator in Settings is the same case: JS8Call's `startTx` refuses there too
        // (`ensureCallsignSet`, mainwindow.cpp:5309), through the same `on_stopTxButton_clicked`
        // (:5310), and with TX on it says why (the alert at :5265). So is a frequency outside
        // the licence's privileges, which `plan_tx` refuses before this mode's planner runs:
        // held, the reply or heartbeat would key after a tune back inside them.
        let no_locator = self.js8_no_usable_locator();
        let outside = !self.tx_allowed();
        if !self.tx_enabled() || no_locator || outside {
            // An automatic reply is in the queue as soon as it is made (or a Yes puts it there),
            // so the queue's rule below is its rule. One still asking stays shown.
            let mut dropped = false;
            // …and a heartbeat that falls due is dropped and its interval re-based, so turning
            // TX back on sends nothing: JS8Call's `startTx` finds TX off (`ensureCanTransmit`,
            // mainwindow.cpp:5295) and `on_stopTxButton_clicked` re-bases it (:5304 → :7397).
            dropped |= self.js8_station.drop_due_heartbeat(now_ms);
            // …and the queue: a message waiting, the rest of one already going out, the CQ
            // repeat's call. Held, it keyed by itself once TX came back or the dial came back
            // inside the privileges; it is dropped instead, the rule CW, RTTY and PSK keep, and
            // JS8Call's when its TX button goes off (`on_monitorTxButton_toggled` →
            // `on_stopTxButton_clicked` → `resetMessage`, mainwindow.cpp:2855-2861, :7390-7398,
            // :5383-5391). Not at the locator's refusal: a message on the air finishes there.
            let operator = if !self.tx_enabled() || outside {
                let (held, operator) = self.js8_station.drop_outbox();
                if held > 0 {
                    let why = if !self.tx_enabled() {
                        "transmit is off"
                    } else {
                        self.connection_tx_refusal()
                            .unwrap_or("outside the licence's privileges")
                    };
                    tempo_core::applog::info(
                        "tx",
                        &format!("JS8 not keyed: {why} (queue dropped)"),
                    );
                }
                dropped |= held > 0;
                operator
            } else {
                false
            };
            // TX off refuses first, and silently for automatic traffic; an operator's own
            // message it drops is said, as CW, RTTY and PSK say theirs. A connection that refuses
            // the section is named, as CW's notice names it, never the licence.
            let why = if !self.tx_enabled() {
                operator.then_some(JS8_QUEUE_REFUSED_TX_OFF)
            } else if no_locator {
                dropped.then_some(JS8_NO_LOCATOR)
            } else if let Some(connection) = self.connection_tx_refusal() {
                dropped.then_some(connection)
            } else {
                dropped.then_some(JS8_REFUSED_PRIVILEGES)
            };
            if let Some(why) = why {
                self.js8_last_error = Some(why.to_string());
            }
        }
        let actions = self.js8_station.tick(now_ms);
        self.js8_handle_actions(actions);
        // The dial remembered for "return to the last used frequency", and the band schedule.
        self.js8_freq_tick(now_ms);
    }

    /// Reassembler events → activity rows + station actions.
    fn js8_handle_events(&mut self, events: Vec<MessageEvent>, low_conf: bool, now_ms: u64) {
        // Whether the frame before a single-frame message was its Last: DIRECTED.TXT marks a
        // message that ended (`js8_freq::js8_directed_line`).
        let mut frame_last = true;
        for ev in events {
            if let MessageEvent::Message(m) = &ev {
                self.js8_log_directed(m, m.frames > 1 || !m.complete || frame_last, now_ms);
            }
            self.js8_api_observe(&ev, now_ms);
            let row = match &ev {
                MessageEvent::Frame(rx) => {
                    frame_last = rx.i3.last;
                    self.js8_file_band_activity(rx.freq_hz, rx.speed, rx.at_ms);
                    Some(self.js8_row_for_frame(rx, low_conf))
                }
                // A single-frame message IS its frame row; only multi-frame text (or an
                // incomplete/force-closed buffer) earns a second, reassembled row.
                MessageEvent::Message(m) if m.frames > 1 || !m.complete => Some(Js8ActivityRow {
                    at_ms: m.last_ms,
                    speed: m.speed,
                    freq_hz: m.freq_hz,
                    snr_db: m.snr_db,
                    dt_s: 0.0,
                    from: m.from.clone(),
                    text: m.text.clone(),
                    // `to` holds no custom group or compound call; its text does.
                    directed_to_me: m
                        .to
                        .clone()
                        .or_else(|| CallRef::parse_destination(&m.to_text))
                        .is_some_and(|to| self.js8_addressed_to_me(&to)),
                    mine: false,
                    complete: m.complete,
                    low_conf: false,
                    flags: Some(Js8RowFlags {
                        first: false,
                        last: false,
                        assembled: true,
                    }),
                }),
                MessageEvent::Message(_) => None,
            };
            if let Some(row) = row {
                self.js8_activity.push_back(row);
                while self.js8_activity.len() > JS8_ACTIVITY_CAP {
                    self.js8_activity.pop_front();
                }
            }
            let actions = self.js8_station.on_event(&ev, now_ms);
            self.js8_handle_actions(actions);
            self.js8_links_on_event(&ev, now_ms);
        }
    }

    /// File a decoded frame in the band activity, as JS8Call's decode path does for every frame
    /// (mainwindow.cpp:3967-4016), at its whole-hertz offset (`frequencyOffset()`, the decoder's
    /// float truncated to an int). An offset not yet filed first takes over the filed offset
    /// within the speed's `rxThreshold` of it that `generateOffsets` meets first counting up
    /// from offset − range (:3970-3981, :3730-3739), so a drifting signal keeps one entry.
    fn js8_file_band_activity(&mut self, freq_hz: f32, speed: Js8Speed, at_ms: u64) {
        let offset = freq_hz as i32;
        if !self.js8_band_activity.contains_key(&offset) {
            let range = speed.drift_hz() as i32;
            let prev = self
                .js8_band_activity
                .range(offset - range..=offset + range)
                .next()
                .map(|(&o, _)| o);
            if let Some(prev) = prev {
                self.js8_band_activity.remove(&prev);
            }
        }
        self.js8_band_activity.insert(offset, at_ms);
    }

    /// One decoded frame → its activity row (JS8Call's display line, byte-exact).
    fn js8_row_for_frame(&self, rx: &RxFrame, low_conf: bool) -> Js8ActivityRow {
        let (from, directed_to_me) = match &rx.frame {
            Frame::Heartbeat { call, .. } | Frame::Compound { call, .. } => (call.clone(), false),
            Frame::CompoundDirected { call, .. } => (
                call.clone(),
                CallRef::parse_destination(call).is_some_and(|to| self.js8_addressed_to_me(&to)),
            ),
            Frame::Directed { from, to, .. } => (from.render(), self.js8_addressed_to_me(to)),
            Frame::Data { .. } => (String::new(), false),
        };
        Js8ActivityRow {
            at_ms: rx.at_ms,
            speed: rx.speed,
            freq_hz: rx.freq_hz,
            snr_db: rx.snr_db,
            dt_s: rx.dt_s,
            from,
            text: rx.display.clone(),
            directed_to_me,
            mine: false,
            complete: true,
            low_conf,
            flags: Some(Js8RowFlags {
                first: rx.i3.first,
                last: rx.i3.last,
                assembled: false,
            }),
        }
    }

    /// My call, @ALLCALL, or a group I have joined, built in or my own (JS8Call's addressed-to-me
    /// rule; `isGroupCallIncluded`, mainwindow.cpp:8153-8155, takes any group in `my_groups`). Not
    /// @ALLCALL while I take no part in it, as JS8Call then shows none of it as mine
    /// (processCommandActivity.cpp:181-185).
    fn js8_addressed_to_me(&self, to: &CallRef) -> bool {
        let mycall = self.settings.mycall.trim();
        let (my_base, _) = split_portable(mycall);
        let joined = |g: &str| {
            self.js8_station
                .config()
                .groups
                .iter()
                .any(|x| x.eq_ignore_ascii_case(g))
        };
        match to {
            CallRef::Base(c) => c.eq_ignore_ascii_case(my_base),
            CallRef::AllCall => !self.settings.js8_avoid_allcall,
            CallRef::Group(_) => joined(&to.render()),
            CallRef::Compound(c) => c.eq_ignore_ascii_case(mycall) || joined(c),
            CallRef::Placeholder | CallRef::Js8Net => false,
        }
    }

    /// THE actions match. Every path (ingest, tick, the verbs) funnels here so a new action
    /// is handled once. In the receive-only build the outbox actions cannot occur (the
    /// station's switches are forced off); the TX batch fills those arms.
    pub(super) fn js8_handle_actions(&mut self, actions: Vec<StationAction>) {
        for a in actions {
            match a {
                StationAction::InboxChanged => self.js8_persist(),
                StationAction::Toast {
                    text,
                    directed_to_me,
                } => tempo_core::applog::info(
                    "js8",
                    &format!("{}{text}", if directed_to_me { "to me: " } else { "" }),
                ),
                StationAction::ChecksumFailed { from, freq_hz } => tempo_core::applog::info(
                    "js8",
                    &format!("checksum failed from {from} at {freq_hz:.0} Hz — message dropped"),
                ),
                StationAction::RateLimited { from } => tempo_core::applog::info(
                    "js8",
                    &format!("@ALLCALL from {from} not answered (15-minute cap)"),
                ),
                StationAction::HbAckHeldOff { from } => tempo_core::applog::info(
                    "js8",
                    &format!(
                        "heartbeat from {from} not acknowledged: acknowledged or answered on \
                         @ALLCALL in the last 15 minutes"
                    ),
                ),
                StationAction::Relayed { path, text } => tempo_core::applog::info(
                    "js8",
                    &format!("relayed via {}: {text}", path.join(">")),
                ),
                StationAction::IdleTripped => {
                    // JS8Call parity: HB, autoreply and relay stand down and the queues
                    // drop; `tx_enabled` is UNTOUCHED (it is the operator's latch, not the
                    // station's). The persisted switches are NOT rewritten — `Js8State.armed`
                    // reads the trip (`Station::readiness`), and any operator verb ends it and
                    // gives the station back its switches (`js8_restart_idle_clock`). The
                    // cockpit toasts on the rising edge of `idle_tripped` (B7.9).
                    self.js8_hb_on = false;
                    self.js8_station.halt();
                }
                // The 55-minute rule stopped a station: it goes into Settings' "never
                // acknowledge" list, as JS8Call writes it into its HB blacklist and saves its
                // settings at once (`addToHbBlacklist`, Configuration.cpp:1226-1231). The rule's
                // record of it is journaled.
                StationAction::HbAckRateLimited { from } => {
                    self.js8_list_rate_blocked(&from);
                    self.js8_persist();
                }
                // Queued/ReplyPending/HeardChanged carry no engine-side effect: the queue,
                // pending countdown and heard list are read straight from the station at
                // poll time.
                StationAction::Queued { .. }
                | StationAction::ReplyPending { .. }
                | StationAction::HeardChanged => {}
            }
        }
    }

    /// Write a station the 55-minute rule stopped into the operator's "never acknowledge" list,
    /// which the station put it on (`hb_rate_held_off`): the one call appended to the settings
    /// this Engine holds, under its lock, so it is a change to the live settings and never a
    /// write of an older copy; then handed to `settings.json`'s writer, as a command's change is
    /// (`settings::writer`). The settings file is the native host's (installed at launch with
    /// `configure_remote_settings_store`, the file every command saves to); with none, as in a
    /// test, the change stays in memory. The Settings form saves its own edits of the list over
    /// this one rather than over its copy (`ui/src/settings/patch.ts`, `mergeListEdit`).
    fn js8_list_rate_blocked(&mut self, call: &str) {
        let listed = self
            .js8_station
            .config()
            .hb_ack_deny
            .iter()
            .any(|c| c == call);
        if !listed {
            tempo_core::applog::info(
                "js8",
                &format!(
                    "{call} heartbeats more often than every 55 minutes, but the \"never \
                     acknowledge\" list is full: it was not added, and it is still not acknowledged \
                     until the 55-minute rule is turned off"
                ),
            );
            return;
        }
        tempo_core::applog::info(
            "js8",
            &format!(
                "{call} heartbeats more often than every 55 minutes: added to \"Never acknowledge \
                 heartbeats from these callsigns\" (Settings ▸ Digital ▸ JS8)"
            ),
        );
        let deny = &mut self.settings.js8_hb_ack_deny;
        if deny.iter().any(|c| c.trim().eq_ignore_ascii_case(call)) {
            return;
        }
        deny.push(call.to_string());
        self.js8_apply_station_config();
        if let Some(path) = &self.remote_settings_path {
            crate::settings::writer::SettingsWriter::of(path).queue(self.settings.clone(), |e| {
                eprintln!("tempo: failed to save the never-acknowledge list: {e}")
            });
        }
    }

    // ---- operator verbs (RX side) ----

    /// Change the TRANSMIT speed (0..=3). Re-points the decoder at the new window and the
    /// slot clock at the new period (the audio loop follows `active_slot_secs`), and the
    /// station at the new countdown. The latch is untouched. The command layer persists. The
    /// cockpit's press is an operator act; the Remote's speed write (`_with_installer` alone,
    /// a saved choice) is not.
    pub fn js8_set_speed(&mut self, speed_idx: u8) -> Result<(), String> {
        self.js8_restart_idle_clock(tempo_core::timing::now_unix_ms() as u64);
        self.js8_set_speed_with_installer(speed_idx, |engine, source| engine.install_source(source))
    }

    /// Remote may already hold the same decoder lock. Keep the native speed
    /// policy here; the caller only supplies the serialized installation point.
    pub(super) fn js8_set_speed_with_installer(
        &mut self,
        speed_idx: u8,
        mut install: impl FnMut(&mut Engine, Box<dyn super::SignalSource>),
    ) -> Result<(), String> {
        if Js8Speed::from_index(speed_idx).is_none() {
            return Err(format!(
                "JS8 speed index {speed_idx} is not 0..=4 (Slow/Normal/Fast/JS8 40/JS8 60)"
            ));
        }
        self.remote_actuation.revoke();
        if self.settings.js8_speed == speed_idx {
            return Ok(());
        }
        self.settings.js8_speed = speed_idx;
        // An over planned under the old period must not key (commit_tx checks the generation).
        self.tx_gate_gen = self.tx_gate_gen.wrapping_add(1);
        if self.app.tier() == Tier::Js8 && self.source_kind == SourceKind::Native {
            if let Some(kind) = self.tier_mode_kind(Tier::Js8) {
                // Swap UNDER the lock and flush the context, exactly as `apply_settings`
                // does for a Q65 period change — the epoch bump is the load-bearing part.
                install(self, Box::new(modes::NativeSource::from_kind(kind)));
                self.clear_decode_context();
            }
        }
        self.js8_apply_station_config();
        Ok(())
    }

    /// Change which speeds the receiver decodes (bitmask, `Js8Speed::bit()`). A mask that
    /// decodes nothing is refused — nobody means "go deaf". The command layer persists.
    pub fn js8_set_rx_speeds(&mut self, mask: u8) -> Result<(), String> {
        self.js8_restart_idle_clock(tempo_core::timing::now_unix_ms() as u64);
        if mask & Js8Speed::ALL_BITS == 0 {
            return Err("at least one JS8 speed must stay enabled".to_string());
        }
        self.settings.js8_rx_speeds = mask & Js8Speed::ALL_BITS;
        Ok(())
    }

    pub fn js8_inbox_mark(&mut self, id: u32, state: InboxState) -> Result<(), String> {
        self.js8_restart_idle_clock(tempo_core::timing::now_unix_ms() as u64);
        if self.js8_station.inbox_mark(id, state) {
            self.js8_persist();
            Ok(())
        } else {
            Err(format!("no JS8 inbox message #{id}"))
        }
    }

    pub fn js8_inbox_delete(&mut self, id: u32) -> Result<(), String> {
        self.js8_restart_idle_clock(tempo_core::timing::now_unix_ms() as u64);
        if self.js8_station.inbox_delete(id) {
            self.js8_persist();
            Ok(())
        } else {
            Err(format!("no JS8 inbox message #{id}"))
        }
    }

    /// JS8Call's "Store Message…": leave `text` at this station for `to`, held in the Inbox and
    /// handed over when `to` asks (QUERY MSGS, then QUERY MSG n), through every reply gate. An
    /// operator act that keys nothing. Journaled.
    pub fn js8_store_message(&mut self, to: String, text: String) -> Result<(), String> {
        use ::js8::proto::station::StoreRefusal as R;
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        self.js8_restart_idle_clock(now_ms);
        let actions = self
            .js8_station
            .store_message(&to, &text, now_ms)
            .map_err(|r| match r {
                R::NoCallsign => {
                    "Set your callsign in Settings before storing a JS8 message.".to_string()
                }
                R::NotAStation => format!("{} is not a station's call.", to.trim()),
                R::Empty => "Nothing to store.".to_string(),
                R::TooLong { frames, max } => format!(
                    "Handing that message over needs {frames} frames; the cap at this speed is \
                     {max} (one message must stay under 10 minutes of airtime, §97.119). \
                     Shorten it."
                ),
            })?;
        self.js8_handle_actions(actions);
        Ok(())
    }

    /// The operator has read the Inbox's notes of what a cap dropped: clear them. Journaled.
    pub fn js8_inbox_dropped_clear(&mut self) {
        self.js8_restart_idle_clock(tempo_core::timing::now_unix_ms() as u64);
        if self.js8_station.clear_inbox_dropped() {
            self.js8_persist();
        }
    }

    // ---- the call list's own writes (JS8Call's call activity menu, UI_Constructor.cpp:1059-1129):
    // one field each, never the settings form, which would advance the TX gate generation ----

    /// JS8Call 3.0's RX block list (`RXCallsignBlocklist`): "Block This Station" adds a call to it
    /// (UI_Constructor.cpp:1119-1129) and Settings edits it. Stored trimmed and upper-cased, the
    /// empties and repeats gone. Display only: nothing the station answers, files or sends reads it.
    pub fn js8_set_rx_blocklist(&mut self, calls: Vec<String>) {
        let mut list = js8_call_list(&calls);
        let mut seen = std::collections::HashSet::new();
        list.retain(|c| seen.insert(c.clone()));
        self.settings.js8_rx_blocklist = list;
    }

    /// "Add New Station or Group…" with a @GROUP and "Remove This Group" (JS8Call's `addGroup` and
    /// `removeGroup`, UI_Constructor.cpp:1077-1081, :1108-1109): the joined groups, the Settings field
    /// both write, reach the station at once, as a Settings save would. Each is stored upper-cased
    /// with one '@', the repeats gone; a group JS8Call will not let be joined (@APRSIS, @JS8NET) is
    /// refused, and nothing changes.
    pub fn js8_set_groups(&mut self, groups: Vec<String>) -> Result<(), String> {
        let mut joined: Vec<String> = Vec::new();
        for g in &groups {
            let name = g.trim().trim_start_matches('@').to_ascii_uppercase();
            if name.is_empty() {
                continue;
            }
            let group = format!("@{name}");
            if !::js8::proto::callsign::may_join_group(&group) {
                return Err(format!("{group} is a group that cannot be joined"));
            }
            if !joined.contains(&group) {
                joined.push(group);
            }
        }
        self.settings.js8_groups = joined;
        self.js8_apply_station_config();
        Ok(())
    }

    /// "Remove This Station" on the `@ALLCALL` row and "Add New Station or Group…" with `@ALLCALL`
    /// (JS8Call's `set_avoid_allcall(true)` and `(false)`, UI_Constructor.cpp:1108, :1075): the
    /// "Do not participate in the @ALLCALL group" setting both write, reaching the station at once,
    /// as a Settings save would. The call list leaves `@ALLCALL` out while it is on.
    pub fn js8_set_avoid_allcall(&mut self, avoid: bool) {
        self.settings.js8_avoid_allcall = avoid;
        self.js8_apply_station_config();
    }

    // ---- journal (the pending_msgs.json contract) ----

    /// Where the station snapshot (inbox, heard, @ALLCALL replies, next id) is journaled.
    pub fn set_js8_journal_path(&mut self, path: PathBuf) {
        self.js8_journal_path = Some(path);
    }

    /// Restore a journal's text (best-effort: text that does not parse yields an empty station).
    pub fn js8_load_journal(&mut self, text: &str) {
        let Ok(snap) = serde_json::from_str::<StationSnapshot>(text) else {
            return;
        };
        self.js8_station.restore(snap, now_unix_secs() * 1000);
    }

    /// Restore the journal at launch, from the path the shell set ([`Self::set_js8_journal_path`]).
    /// A journal this build cannot read — torn, or from a newer build with a value this one does
    /// not know — holds stored messages, so it is kept aside ([`tempo_core::keep_aside`]) and the
    /// screen says where, rather than starting empty and letting the next inbox change write
    /// over it.
    pub fn restore_js8_journal(&mut self, now_unix: i64) {
        let Some(path) = self.js8_journal_path.clone() else {
            return;
        };
        let snap = tempo_core::keep_aside::read_or_keep("js8Inbox", &path, now_unix, |text| {
            serde_json::from_str::<StationSnapshot>(text).map_err(|e| e.to_string())
        });
        if let Some(snap) = snap {
            self.js8_station.restore(snap, now_unix_secs() * 1000);
        }
    }

    /// Journal the station the MOMENT its inbox changes — write-tmp + fsync + rename, like
    /// `persist_pending_msgs`, so a crash cannot drop a stored message the operator saw land.
    ///
    /// The snapshot is taken here, under the lock, and written on the journals' own thread
    /// ([`tempo_core::journal`]), in the order the station changed: this runs from the radio
    /// loop's decode path and its once-a-second tick, and the fsync is the disk's to finish.
    /// It lands a moment later; the exit path waits for it.
    pub(crate) fn js8_persist(&self) {
        let Some(path) = &self.js8_journal_path else {
            return;
        };
        // A journal this run could not read and could not move aside holds stored messages: it
        // is never written over (`restore_js8_journal`).
        if tempo_core::keep_aside::refuses(path) {
            return;
        }
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        let Ok(text) = serde_json::to_string(&self.js8_station.snapshot(now_ms)) else {
            return;
        };
        self.station.journals.replace(
            path,
            path.with_extension("json.tmp"),
            text.into_bytes(),
            "tempo: failed to journal the JS8 station",
        );
    }

    // ---- operator transmit verbs (each resets the idle counter and, on success, restarts
    // the wall-clock watchdog; none arms TX — the TX latch is the operator's first act) ----

    /// One operator-facing sentence per compose refusal. English at the engine (the
    /// `structured_tx_ready` precedent); the cockpit shows `Js8State.last_error` verbatim.
    fn js8_compose_error(e: ::js8::proto::compose::ComposeError) -> String {
        use ::js8::proto::compose::ComposeError as E;
        match e {
            E::NoCallsign => "Set your callsign in Settings before transmitting JS8.".to_string(),
            E::Empty => "Nothing to send.".to_string(),
            E::TooLong { frames, max } => format!(
                "That message needs {frames} frames; the cap at this speed is {max} \
                 (one message must stay under 10 minutes of airtime, §97.119). \
                 Shorten it or send it in parts."
            ),
        }
    }

    /// Settings holds no locator JS8Call would transmit with. JS8Call refuses to start with an
    /// empty one (`my_grid().trimmed().isEmpty()`, mainwindow.cpp:5264) and its Settings dialog
    /// refuses to save a malformed one (Configuration.cpp:2443), so between them it sends
    /// neither. Nexus's Settings field can hold either, so the gate refuses both, by JS8Call's
    /// own rule (`is_station_locator`) on the trimmed text it stores (Configuration.cpp:2749).
    pub(super) fn js8_no_usable_locator(&self) -> bool {
        !::js8::proto::grid::is_station_locator(self.settings.mygrid.trim())
    }

    /// The gate's reason for refusing the locator in Settings, or None when it would take it: the
    /// cockpit's "send my grid" is disabled, with this as its tooltip, whenever the gate says no.
    /// It asks the gate's own rule above, so the two cannot disagree.
    pub fn js8_locator_refusal(&self) -> Option<&'static str> {
        self.js8_no_usable_locator().then_some(JS8_NO_LOCATOR)
    }

    /// JS8Call's `ensureCallsignSet` (mainwindow.cpp:5257-5271), which its Enter asks before
    /// anything is queued (:749): a callsign first, then a locator. Every operator send asks it.
    fn js8_identity_set(&self) -> Result<(), String> {
        if self.settings.mycall.trim().is_empty() {
            return Err(Self::js8_compose_error(
                ::js8::proto::compose::ComposeError::NoCallsign,
            ));
        }
        if self.js8_no_usable_locator() {
            return Err(JS8_NO_LOCATOR.to_string());
        }
        Ok(())
    }

    /// The operator's To field as a destination JS8Call can address (`isValidCallsign`): a call,
    /// a compound call, a built-in group by its short code or a group of the operator's own by
    /// name. Anything else is refused, saying what a group may be.
    fn js8_destination(to: &str) -> Result<CallRef, String> {
        CallRef::parse_destination(to).ok_or_else(|| {
            format!(
                "{to} is not a callsign or @group JS8 can address. A group is @ and up to 8 \
                 letters or digits (a / may separate them)."
            )
        })
    }

    /// Operator send: `to` is a callsign or @group (None = plain text, which compose
    /// prefixes with "MYCALL: " — identity on the wire, spec invariant 10). An operator
    /// verb: on success it restarts the wall-clock watchdog. Never arms TX.
    pub fn js8_send(&mut self, to: Option<String>, text: String) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        self.js8_restart_idle_clock(now_ms);
        let to_ref = match to.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => Some(Self::js8_destination(s)?),
            None => None,
        };
        let r = self.js8_identity_set().and_then(|()| {
            self.js8_station
                .send(to_ref.as_ref(), text.trim(), now_ms)
                .map_err(Self::js8_compose_error)
        });
        // The compose box keeps a message it queued while it goes out (JS8Call's typeahead).
        if r.as_ref().is_ok_and(|&frames| frames > 0) {
            self.js8_station.begin_typeahead(to_ref.as_ref());
        }
        let r = r.map(|_| ());
        self.js8_after_verb(&r);
        r
    }

    /// A directed command from the 32-entry palette (`cmd` = `Command::id`).
    pub fn js8_send_command(&mut self, to: String, cmd: u8, arg: String) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        self.js8_restart_idle_clock(now_ms);
        let to_ref = Self::js8_destination(to.trim())?;
        let command =
            ::js8::Command::from_id(cmd).ok_or_else(|| format!("unknown JS8 command {cmd}"))?;
        let r = self.js8_identity_set().and_then(|()| {
            self.js8_station
                .send_command(&to_ref, command, arg.trim(), now_ms)
                .map_err(Self::js8_compose_error)
        });
        // …and so is a directed command it sent (typeahead).
        if r.as_ref().is_ok_and(|&frames| frames > 0) {
            self.js8_station.begin_typeahead(Some(&to_ref));
        }
        let r = r.map(|_| ());
        self.js8_after_verb(&r);
        r
    }

    /// CQ: the CQ message from Settings (`js8_cq_text`, JS8Call's `CQMessage`), its macros
    /// replaced (`Station::call_cq`). Counts as Operator origin.
    pub fn js8_call_cq(&mut self) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        self.js8_restart_idle_clock(now_ms);
        let r = self.js8_identity_set().and_then(|()| {
            self.js8_station
                .call_cq(now_ms)
                .map_err(Self::js8_compose_error)
        });
        self.js8_after_verb(&r);
        r
    }

    /// An operator act, whatever it then does: every JS8 verb below starts here. It restarts the
    /// idle count and ends an idle trip, as every key or mouse press in JS8Call's window does
    /// (`eventFilter` → `resetIdleTimer()`, `tx_watchdog(false)`, mainwindow.cpp:2979-2988). The
    /// trip turned the station's autoreply and relay off (`trip_idle`), so the station gets them
    /// back as Settings has them, which is what the dock shows (`js8_state`'s `armed`), as
    /// JS8Call's Idle Timeout box puts AUTO back when the operator closes it (:11013-11019). The
    /// heartbeat and the repeating CQ stay off until their own switch. Not an act, and never
    /// here: a poll (`js8_state`, `js8_composer`), a read, the clock, what is heard, a Settings
    /// change, a Remote's write. The wall-clock watchdog keeps its own rule (`js8_after_verb`).
    pub(crate) fn js8_restart_idle_clock(&mut self, now_ms: u64) {
        self.js8_station.mark_active(now_ms);
        self.js8_apply_station_config();
    }

    /// What every operator verb does with its outcome: a success clears `last_error` and
    /// restarts the wall-clock watchdog (an operator act); a refusal is kept for the
    /// cockpit and leaves the wall clock alone. (The idle count restarted as the act began.)
    fn js8_after_verb(&mut self, r: &Result<(), String>) {
        match r {
            Ok(()) => {
                self.js8_last_error = None;
                self.reset_tx_watchdog();
            }
            Err(e) => self.js8_last_error = Some(e.clone()),
        }
    }

    /// The SECOND operator act (the first is the session TX latch). Autoreply / Relay /
    /// HB-ack and heartbeat networking persist to Settings (the Tauri command saves them); Hb is
    /// session-only and never persisted (G3), and is refused where JS8Call has no HB button. Turning a switch ON keys nothing by itself — `plan_js8_tx`
    /// still needs `tx_enabled` — but it is an operator verb: on or off, it restarts the idle
    /// count and ends an idle trip, and on, it restarts the wall-clock watchdog.
    pub fn js8_arm(&mut self, which: Js8Switch, on: bool) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        self.js8_restart_idle_clock(now_ms);
        // The HB repeat never goes on where JS8Call has no HB button.
        if which == Js8Switch::Hb && on {
            if let Some(why) = self.js8_hb_refusal() {
                return Err(why.to_string());
            }
        }
        match which {
            Js8Switch::Autoreply => self.settings.js8_autoreply = on,
            Js8Switch::Relay => self.settings.js8_relay = on,
            Js8Switch::HbAck => self.settings.js8_hb_ack = on,
            Js8Switch::HbNet => self.settings.js8_hb_networking = on,
            Js8Switch::Hb => {}
        }
        let cfg = Self::js8_station_config(&self.settings);
        self.js8_station.set_config(cfg);
        if which == Js8Switch::Hb {
            // `Station::set_hb(true)` schedules the first heartbeat for the next period
            // (JS8Call: nextTransmitCycle + interval; interval 0 = "on demand" = once).
            self.js8_station.set_hb(on, now_ms);
        }
        // Read back: heartbeat networking off stops the HB repeat in the station.
        self.js8_hb_on = self.js8_station.hb_on();
        if on {
            self.reset_tx_watchdog();
        }
        Ok(())
    }

    /// Why no heartbeat may go out now, as JS8Call has no HB button then: heartbeat networking
    /// off, or a speed it sends none at (`prepareHeartbeatMode`, mainwindow.cpp:4124-4130).
    fn js8_hb_refusal(&self) -> Option<&'static str> {
        if !self.settings.js8_hb_networking {
            Some(JS8_HB_NEEDS_NETWORKING)
        } else if !heartbeat_speed(self.js8_tx_speed()) {
            Some(JS8_HB_SPEED)
        } else {
            None
        }
    }

    /// "Send Heartbeat Now", from the HB button's menu (`sendHB`, mainwindow.cpp:4321-4323): one
    /// heartbeat, keyed in the next period whether or not the HB repeat is on, which it leaves as
    /// it is. An operator verb, refused where JS8Call has no HB button and without a callsign and
    /// locator, as every send is.
    pub fn js8_heartbeat_now(&mut self) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        self.js8_restart_idle_clock(now_ms);
        let r = match self.js8_hb_refusal() {
            Some(why) => Err(why.to_string()),
            None => self.js8_identity_set(),
        }
        .and_then(|()| {
            self.js8_station
                .heartbeat_now(now_ms)
                .map_err(Self::js8_compose_error)
        });
        self.js8_after_verb(&r);
        r
    }

    /// The call in the cockpit's To box, JS8Call's selected call, for the QSO pause
    /// (`Station::select_call`). Not an operator act: the box also changes by itself (a reply the
    /// station puts in the compose box clears it). Keys nothing; clearing it may turn a paused HB
    /// repeat back on, whose first heartbeat is one interval away.
    pub fn js8_select_call(&mut self, call: Option<String>) {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        self.js8_station.select_call(call.as_deref(), now_ms);
        self.js8_hb_on = self.js8_station.hb_on();
    }

    /// Arm/disarm JS8Call's repeating CQ, which sends the CQ message from Settings.
    /// The SECOND act, session-only and never persisted — the same rule the heartbeat toggle
    /// follows (G3), so a crash or a relaunch can never come back calling CQ. Arming keys
    /// nothing: `plan_js8_tx` re-reads the TX latch every slot, and the idle watchdog, Stop
    /// TX, a tier change and a mode change each cancel the schedule.
    pub fn js8_set_cq_repeat(&mut self, on: bool) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        // An operator verb: on or off, it restarts the idle count and ends an idle trip; on, it
        // restarts the wall clock.
        self.js8_restart_idle_clock(now_ms);
        self.js8_station.set_cq(on, now_ms);
        if on {
            self.reset_tx_watchdog();
        }
        Ok(())
    }

    /// The operator's Yes or No to the automatic reply the cockpit shows as (`display`,
    /// `fires_at_ms`), JS8Call's confirmation box (mainwindow.cpp:5209-5230). A Yes puts it in
    /// the queue, where every gate applies when its period comes, and is an operator act like a
    /// send (it restarts the wall-clock watchdog); a No drops it. A Yes to a reply no longer
    /// waiting (answered, or its 89 s ran out) is refused, so it never sends something else.
    pub fn js8_answer_reply(
        &mut self,
        yes: bool,
        display: String,
        fires_at_ms: u64,
    ) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        // Yes or No, and to a reply no longer waiting too: the press is the act.
        self.js8_restart_idle_clock(now_ms);
        match self
            .js8_station
            .answer_reply(&display, fires_at_ms, yes, now_ms)
        {
            Some(actions) => {
                self.js8_handle_actions(actions);
                if yes {
                    self.js8_after_verb(&Ok(()));
                }
                Ok(())
            }
            None if yes => Err(JS8_REPLY_GONE.to_string()),
            None => Ok(()),
        }
    }

    /// The cockpit's compose box, both ways (JS8Call's `extFreeTextMsgEdit`): whether it holds
    /// text, and the id of the reply it took into the box since it last said; answered with the
    /// reply the station has put in the composer (AUTO off, `addMessageText`, mainwindow.cpp:
    /// 9671), until the cockpit takes it. Never keys anything: the operator's Send does.
    pub fn js8_composer(
        &mut self,
        composing: bool,
        taken: Option<u32>,
    ) -> Option<Js8ComposerPrefill> {
        let (actions, offered) = self.js8_station.sync_composer(composing, taken);
        self.js8_handle_actions(actions);
        offered.map(|(id, text)| Js8ComposerPrefill { id, text })
    }

    /// Sender-class, NOT a stop: empties the outbox, and as an operator act restarts the idle
    /// count. The HB schedule, the TX latch and a frame already on the air are untouched —
    /// Stop TX is `halt_tx`.
    pub fn js8_drop_queue(&mut self) {
        self.js8_restart_idle_clock(tempo_core::timing::now_unix_ms() as u64);
        self.js8_station.drop_queue();
    }

    /// Halt is TOTAL (spec invariant 7): drop the outbox, the pending autoreply and the
    /// heartbeat schedule, release the per-slot latch. Called from `halt_tx` (Stop TX, the
    /// UDP HaltTx, the watchdog kill path), from `set_tier` when LEAVING the tier, and
    /// from `set_mode`. `tx_enabled` is the caller's business: `halt_tx` drops it, a tier
    /// change decides for itself, `set_mode` may be arming. The one-shot `slot_tx_abort`
    /// that cuts a frame in flight is `halt_tx`'s and is armed there.
    pub fn js8_halt_clear(&mut self) {
        self.js8_station.halt();
        self.js8_hb_on = false;
        self.js8_planned_slot = None;
    }

    /// TX Off, from `set_tx_enabled`: what JS8 still has queued is dropped, the rest of a
    /// message already going out included, as the other modes' queues are there. The frame on
    /// the air finishes (the latch's contract); a countdown stays in view and is cancelled when
    /// it falls due (`js8_tick`); the schedules stay. An operator's own message dropped is said.
    pub(crate) fn js8_tx_off(&mut self) {
        let (held, operator) = self.js8_station.drop_outbox();
        if held > 0 {
            tempo_core::applog::info("tx", "JS8 not keyed: transmit is off (queue dropped)");
        }
        if operator {
            self.js8_last_error = Some(JS8_QUEUE_REFUSED_TX_OFF.to_string());
        }
    }

    /// Book a JS8 over at PLAN time — the beacon and QSO arms' rule, and for the same
    /// reason: the plan is the transmit decision, and `commit_tx` refuses only the
    /// microsecond races (JS8's build is pure Rust). Three records: the own-TX row for the
    /// Rx-Frequency feed (`record_own_tx_at`), a `mine` row in the JS8 activity ring the
    /// cockpit's activity pane reads, and — when ALL.TXT is on — the `Tx` line in the
    /// FT/beacon writers' shape (SNR/DT 0, audio = `f0`, into the shared ALL.TXT
    /// buffer with the same 5000-line cap), with the speed's letter as a JS8 frame's line has
    /// it (`alltxt::js8_all_txt_line`). `now_ms` is the PERIOD START of the over
    /// (slot × period), not the wall clock: alltxt.rs's rule is that only the slot knows
    /// which period an over belongs to. `f0` is the offset the over KEYED at, a heartbeat's
    /// sub-band slot included: JS8Call moves `freq()` there before it shows the frame
    /// (`setFreqOffsetForRestore`, mainwindow.cpp:9681), so all three records sit there.
    /// `first`/`last` are the frame's own i3 bits, carried to the activity row so the RX pane closes
    /// the line with its end mark as JS8Call's does (mainwindow.cpp:3660-3668).
    pub(crate) fn js8_note_tx_done(
        &mut self,
        plan_display: &str,
        now_ms: u64,
        f0: f32,
        first: bool,
        last: bool,
    ) {
        self.record_own_tx_at(plan_display.to_string(), f0);
        self.js8_activity.push_back(Js8ActivityRow {
            at_ms: now_ms,
            speed: self.js8_tx_speed(),
            freq_hz: f0,
            snr_db: 0,
            dt_s: 0.0,
            from: self.settings.mycall.trim().to_ascii_uppercase(),
            text: plan_display.to_string(),
            directed_to_me: false,
            mine: true,
            complete: true,
            low_conf: false,
            flags: Some(Js8RowFlags {
                first,
                last,
                assembled: false,
            }),
        });
        while self.js8_activity.len() > JS8_ACTIVITY_CAP {
            self.js8_activity.pop_front();
        }
        if self.settings.write_all_txt {
            self.station
                .all_txt_pending
                .push(crate::alltxt::js8_all_txt_line(
                    now_ms / 1000,
                    self.settings.dial_mhz,
                    true,
                    self.js8_tx_speed(),
                    0,
                    0.0,
                    f0,
                    plan_display,
                ));
            let len = self.station.all_txt_pending.len();
            if len > 5000 {
                self.station.all_txt_pending.drain(0..len - 5000);
            }
        }
    }

    /// Whether the licence gate allows a JS8 frame keyed at `f0` Hz at `speed`: the whole gate
    /// ([`Engine::tx_allowed_for`]), across the width the frame occupies at that speed. For a frame
    /// on the operator's TX offset at the TX speed it is [`Engine::tx_allowed`]'s judgement.
    pub(crate) fn js8_frame_allowed(&self, f0: f32, speed: Js8Speed) -> bool {
        self.tx_allowed_for(DataFrame {
            f0_hz: f64::from(f0),
            bw_hz: DataFrame::width_hz(Some(modes::ModeKind::Js8 { speed })),
        })
    }

    /// Whether the licence gate allows the JS8 frame `plan` keys, where it keys: on its own offset,
    /// a heartbeat's spot included ([`Self::js8_frame_allowed`]). `true` for an over of any other
    /// mode, which keys on the operator's TX offset that [`Engine::tx_allowed`] judges (FT8/FT4's
    /// Split Operation moves the dial and the audio together, the RF unchanged).
    pub(crate) fn js8_plan_frame_allowed(&self, plan: &TxPlan) -> bool {
        match &plan.waveform {
            TxWaveform::Js8 { speed, f0, .. } => self.js8_frame_allowed(*f0, *speed),
            _ => true,
        }
    }

    /// Tier-routed TX planner — the ONLY place a `proto::TxFrame` becomes a `TxPlan`
    /// (spec invariant 13). Reached only after `plan_tx`'s mode-agnostic guards
    /// (`!tx_enabled || tuning || !tx_allowed()`, `tier_is_rx_only`, operating mode
    /// Digital), so `tx_enabled` — the FIRST operator act — is already true here.
    ///
    /// Order: identity gate (a callsign for every frame, a locator for a message's first) →
    /// decode-only refusal → one-frame-per-period latch → station outbox → origin gate (the
    /// SECOND act, re-read at plan time every slot) → wall-clock watchdog (all origins but
    /// Heartbeat) → f0 → the licence gate at f0 → book → plan. Nothing here moves the dial
    /// (invariant 9). Booking is at plan time, the beacon / QSO arms' rule.
    pub fn plan_js8_tx(&mut self, slot: u64) -> Option<TxPlan> {
        // Identity, fail-closed: Js8Mode declares `structured_identity`, so a blank or
        // unparsable MYCALL refuses here (`needs_grid = false`: the FT gate's 4-or-6-character
        // grid rule is not JS8Call's, whose locator rule follows). `proto::compose`
        // additionally refuses a MYCALL that cannot be base-packed, before anything is queued.
        if self.structured_tx_ready(false).is_err() {
            self.set_transmitting(false);
            return None;
        }
        // …and no transmission STARTS without a locator JS8Call would accept, as in JS8Call:
        // `startTx` (mainwindow.cpp:4768) → `ensureCreateMessageReady` → `ensureCallsignSet`
        // (:5309, :5264-5268). The frames after a message's first go out through `stopTx` →
        // `prepareNextMessageFrame` (:4825), which never asks again, so a message already on
        // the air finishes. Refused before `next_frame`: nothing is released, popped or keyed,
        // and `js8_tick` drops a reply or heartbeat that falls due meanwhile.
        if self.js8_no_usable_locator() {
            let queue = self.js8_station.queue();
            if queue.first().is_none_or(|next| next.first) {
                if !queue.is_empty() {
                    // …and everything waiting is dropped, as JS8Call's refusal does
                    // (`on_stopTxButton_clicked` → `resetMessage` → `resetMessageTransmitQueue`,
                    // :5310 → :7396 → :5383-5391), so nothing old goes out once a locator is
                    // set. The CQ repeat and the heartbeat keep their schedules.
                    self.js8_station.drop_queue();
                    self.js8_last_error = Some(JS8_NO_LOCATOR.to_string());
                }
                self.set_transmitting(false);
                return None;
            }
        }
        let speed = self.js8_tx_speed();
        // Decode-only refusal, in the planner and not the builder (the FT arms' rule):
        // a `None` here means a receive-only mode reached the TX path, which is a bug,
        // and refusing to key is the right answer to a bug.
        if modes::tx_mode(modes::ModeKind::Js8 { speed }).is_none() {
            self.set_transmitting(false);
            return None;
        }
        // ONE frame per period. `plan_tx` is polled once at the boundary, but the snappy
        // immediate-TX path can poll again inside the period; popping a second frame
        // there would key it mid-period on top of the first.
        if self.js8_planned_slot == Some(slot) {
            return None;
        }
        let period_ms = u64::from(speed.period_s()) * 1000;
        let period_start_ms = slot.saturating_mul(period_ms);
        // JS8Call's "free HB slot" rule, `isFreqOffsetFree(f, 50)` (mainwindow.cpp:5566-5590): the
        // operator's own offset is free (`freq() == f`, :5572); any other is taken while the band
        // activity holds an offset within 50 Hz heard in the last 30 s (:5579-5587). The 50 is
        // `findFreeFreqOffset(500, 1000, 50)`'s own argument at every speed, not the signal's
        // bandwidth. Its other exception, an offset in the directed cache (:5572), never holds on
        // the air: only `initializeDummyData` (:1658) marks one. Snapshot the band activity first
        // — the station is borrowed mutably by `next_frame` below.
        let bw = 50.0;
        let own = self.tx_offset_hz().trunc();
        let activity: Vec<(f32, u64)> = self
            .js8_band_activity
            .iter()
            .map(|(&o, &at)| (o as f32, at))
            .collect();
        let busy = move |f: f32| {
            f != own
                && activity.iter().any(|&(o, at)| {
                    (o - f).abs() < bw && period_start_ms.saturating_sub(at) < 30_000
                })
        };
        let mut seed = self.js8_rng;
        let next = {
            let mut rng = || js8_rng_next(&mut seed);
            self.js8_station
                .next_frame(period_start_ms, &busy, &mut rng)
        };
        self.js8_rng = seed;
        let Some(tf) = next else {
            self.set_transmitting(false);
            return None;
        };
        // The frame is POPPED now; whatever happens below, this slot is spent.
        self.js8_planned_slot = Some(slot);
        // THE SECOND ACT, re-read at plan time: an automatic origin keys only with its
        // persisted switch on and no idle trip standing. Operator frames were queued by an
        // operator verb; a Heartbeat exists only because of the session toggle — that IS
        // the act (HB is never persisted, G3).
        let switch_on = match tf.origin {
            // A `CqRepeat` frame, like a `Heartbeat`, EXISTS only because the session
            // toggle is on — that toggle IS the second act. Neither is persisted (G3), and
            // the schedule behind both is cancelled by Stop TX, the idle watchdog, a tier
            // change and a mode change, so there is no state here to re-read.
            Origin::Operator | Origin::CqRepeat => true,
            // …and a heartbeat or its acknowledgement keys only where JS8Call sends them:
            // heartbeat networking on, at Slow, Normal or Fast. The station drops both when
            // either changes; this holds should one reach the planner anyway.
            Origin::Heartbeat => self.settings().js8_hb_networking && heartbeat_speed(speed),
            Origin::HbAck => {
                self.settings().js8_hb_ack
                    && self.settings().js8_hb_networking
                    && heartbeat_speed(speed)
            }
            Origin::AutoReply => self.settings().js8_autoreply,
            Origin::Relay => self.settings().js8_relay,
            // The inbound APRS-IS relay's switch and the two it hangs from (spotting, the
            // @APRSIS gateway), re-read at the key: turn any off and a relay already queued
            // never keys.
            Origin::AprsRelay => self.js8_aprs_relay_switch(),
        };
        if !switch_on || self.js8_station.idle_tripped() {
            // Dropped, not deferred: a reply withheld now must not fire ten minutes later
            // when the operator flips a switch (the `js8_tick` rule cancels an expired
            // unarmed countdown for the same reason).
            tempo_core::applog::info(
                "tx",
                &format!(
                    "JS8 {:?} frame withheld (not armed): {}",
                    tf.origin, tf.display
                ),
            );
            self.set_transmitting(false);
            return None;
        }
        // BEACON-CLASS ORIGINS: the scheduled heartbeat (G2) and the scheduled CQ repeat.
        // Both are unattended repeated transmission BY DESIGN, which is the exact premise of
        // the 2026-08-17 beacon exemption ("the watchdog's premise is idleness"); applying a
        // 6-minute wall clock to a 15-minute CQ repeat would kill it after the first call and
        // the feature would not exist. They are not unbounded: `Station::note_tx_done` resets
        // the idle baseline for `Origin::Operator` ALONE, so neither a heartbeat nor a
        // repeated CQ can hold off the idle watchdog that stops them (default 60 min, floor
        // 5), and every over stays hard-bounded by the slot clamp.
        let beacon = matches!(tf.origin, Origin::Heartbeat | Origin::CqRepeat);
        // Wall-clock watchdog, RE-APPLIED for every non-beacon origin — operator, autoreply,
        // relay and HB-ack traffic is bounded by it exactly as before.
        if !beacon && self.js8_wall_clock_trips() {
            return None;
        }
        // f0: the operator's TX offset, or the station's HB sub-band pick — an AUDIO
        // offset only. JS8Call's `sendHeartbeat` (mainwindow.cpp:6279) keeps an operator at
        // or below 1000 Hz on their own offset; `sendHeartbeatAck` (:6299) has no such rule,
        // so an HB-ACK always takes the pick. With `heartbeat_anywhere` on (Settings, off by
        // default as upstream) the station hands both out on `FreqHint::Dial`, the operator's
        // offset. A pick outside 500–1000 Hz cannot come from a correct station; fall back to
        // the operator's offset rather than trust it.
        let own_offset = self.tx_offset_hz() <= 1000.0;
        let f0 = match tf.freq_hint {
            FreqHint::Dial => self.tx_offset_hz(),
            FreqHint::HbSubband(_) if tf.origin == Origin::Heartbeat && own_offset => {
                self.tx_offset_hz()
            }
            FreqHint::HbSubband(f) if (500.0..=1000.0).contains(&f) => f,
            FreqHint::HbSubband(_) => self.tx_offset_hz(),
        };
        // THE LICENCE GATE, AT THE OFFSET THE FRAME KEYS ON (operator ruling, 2026-10-15: "Judge
        // the heartbeat's own spot."). `plan_tx` judged the operator's TX offset; a heartbeat or
        // HB ACK keys on its own spot, and on a dial just under a segment's bottom that spot is
        // below it while the TX offset is inside. The whole gate, across the frame's width; for a
        // frame on the TX offset it is the judgement `plan_tx` made. Refused, the frame is dropped,
        // as a refusal outside the privileges drops what falls due (`js8_tick`), and nothing is
        // booked.
        if !self.js8_frame_allowed(f0, speed) {
            tempo_core::applog::info(
                "tx",
                &format!(
                    "JS8 {:?} frame not keyed: {f0} Hz is outside the licence's privileges: {}",
                    tf.origin, tf.display
                ),
            );
            self.js8_last_error = Some(JS8_REFUSED_PRIVILEGES.to_string());
            self.set_transmitting(false);
            return None;
        }
        // Book the over (station bookkeeping: Last sent, idle counter; own-TX row; activity
        // row; ALL.TXT Tx line) — plan time, on the period-start axis. The heartbeat timer
        // re-based itself in `next_frame` when the message's last frame went.
        self.js8_station.note_tx_done(&tf, period_start_ms);
        self.js8_note_tx_done(&tf.display, period_start_ms, f0, tf.first, tf.last);
        self.js8_on_air = Some(Js8OnAir {
            slot,
            speed,
            word: tf.word,
            f0,
            follows: f0 == self.tx_offset_hz(),
            moves: std::sync::Arc::new(std::sync::Mutex::new(vec![(0, f0)])),
        });
        self.js8_api_note_tx(&tf, speed, f0);
        self.set_transmitting(true);
        Some(TxPlan {
            slot,
            tier: Tier::Js8,
            waveform: TxWaveform::Js8 {
                speed,
                word: tf.word,
                f0,
            },
            beacon,
            stamp: self.tx_gate_stamp(),
        })
    }
}

/// The JS8 frame last planned (`Engine::js8_on_air`): the slot it keys in, its speed and word, the
/// offset it is on now, whether that is the operator's TX offset — a heartbeat or HB ACK on its own
/// spot in 500–1000 Hz is not, and stays there — and where its offset has moved while it played.
#[derive(Debug, Clone)]
pub(crate) struct Js8OnAir {
    pub(crate) slot: u64,
    pub(crate) speed: Js8Speed,
    pub(crate) word: Word87,
    pub(crate) f0: f32,
    pub(crate) follows: bool,
    /// `(sample of the frame's wave, offset)` from its planned offset at sample 0 on: each move's
    /// rest is built on the ones before it, written as the sound card takes it
    /// (`js8_retune_on_air`), so the phase carries on unbroken across every move.
    pub(crate) moves: std::sync::Arc<std::sync::Mutex<Vec<(usize, f32)>>>,
}

/// JS8's send mechanics: Hold PTT between frames, the TX offset while sending, and typeahead.
#[path = "js8_send.rs"]
mod send;
pub use send::JS8_RETUNE_LEAD;

/// JS8's TX delay: how early a frame keys and how late it may still start.
#[path = "js8_lead.rs"]
mod lead;
pub use lead::{js8_late_start_ms, JS8_TX_DELAY_MAX_MS, JS8_TX_DELAY_MIN_MS};

#[cfg(test)]
mod tests {
    use super::*;
    use ::js8::proto::callsign::CallRef;
    use ::js8::proto::command::Command;
    use ::js8::proto::frame::encode_frame;
    use ::js8::{Frame, I3};

    #[test]
    fn bounded_observation_preserves_native_js8_state_at_every_speed() {
        for speed in Js8Speed::ALL.map(Js8Speed::index) {
            let mut e = Engine::with_settings(Settings {
                mycall: "N0CALL".into(),
                mygrid: "AA00".into(),
                ..Default::default()
            });
            e.js8_set_speed(speed).unwrap();
            e.js8_ingest(
                &[row(&hb("W1AW", "FN31"), whole(), Js8Speed::Normal, 1500.0)],
                1,
            );
            e.js8_send(None, "TEST MESSAGE WITH MULTIPLE FRAMES".into())
                .unwrap();
            let before = serde_json::to_value(e.js8_state()).unwrap();
            assert!(!before["activity"].as_array().unwrap().is_empty());
            assert!(!before["queue"].as_array().unwrap().is_empty());
            // The Remote's sample is the native state whole: the desktop's `presence` and
            // `controls` (the waterfall controls and the schedule's notice) and each row's place
            // in its message ride in it, as the page's parser lists them.
            assert!(
                before.get("presence").is_some() && before.get("controls").is_some(),
                "control: the desktop's state carries presence and controls"
            );
            assert!(
                before["activity"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|row| row.get("flags").is_some()),
                "control: the desktop's rows carry their places"
            );
            assert_eq!(
                serde_json::to_value(e.bounded_js8_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(serde_json::to_value(e.js8_state()).unwrap(), before);
            assert!(!e.snapshot().radio.tx_enabled);
            let mut oversized = e.js8_activity[0].clone();
            oversized.text = "x".repeat(1025);
            e.js8_activity.push_back(oversized);
            assert!(e.bounded_js8_state().is_none());
            assert_eq!(e.js8_activity.back().unwrap().text.len(), 1025);
        }
    }

    #[test]
    fn queue_copy_budget_counts_remaining_frames_instead_of_messages() {
        let mut e = Engine::with_settings(Settings {
            mycall: "N0CALL".into(),
            mygrid: "AA00".into(),
            ..Default::default()
        });
        e.js8_send(None, "TEST MESSAGE WITH MULTIPLE FRAMES".into())
            .unwrap();
        let rows = e.js8_state().queue;
        assert!(rows.len() > 1);
        assert_eq!(
            e.js8_station.display_queue_budget(rows.len() - 1, 1024),
            None
        );
        assert_eq!(
            e.js8_station.display_queue_budget(rows.len(), 1024),
            Some(rows.iter().map(|r| r.display.len()).sum())
        );
        assert_eq!(e.js8_station.display_queue_budget(rows.len(), 1), None);
        assert_eq!(e.js8_state().queue.len(), rows.len());
    }

    /// A decode row exactly as `Js8Mode::decode_frame` would emit it for `frame`.
    fn row(frame: &Frame, i3: I3, speed: Js8Speed, freq: f32) -> modes::Decode {
        let word = encode_frame(frame, i3, speed).expect("packable");
        modes::Decode {
            message: frame.render(),
            sync: 5.0,
            snr: -7,
            dt: 0.1,
            freq,
            nap: 0,
            qual: 0.9,
            rv: None,
            mode: Some(modes::ModeKind::Js8 { speed }),
            raw: Some(*word.as_bytes()),
        }
    }

    fn hb(call: &str, grid: &str) -> Frame {
        Frame::Heartbeat {
            call: call.to_string(),
            grid: Some(grid.to_string()),
            is_cq: false,
            idx: 0,
        }
    }

    fn whole() -> I3 {
        I3 {
            first: true,
            last: true,
            data: false,
        }
    }

    /// View entry = the tier + the watering hole, and NOTHING that keys: at launch the TX
    /// latch is down (the first act), so every armed flag is false and no slot keys — even
    /// with autoreply persisted ON. This is the B5 half of `js8_autoreply_never_keys_at_launch`
    /// (the TX batch's `js8_arm` + the armed-latch case add the switch-on half). B7.3 makes
    /// the tier transmit-capable, so the latch CAN now be armed — that is pinned separately by
    /// `the_tx_latch_arms_on_the_js8_tier` and `js8_enter_keys_nothing_even_with_tx_enabled`.
    #[test]
    fn js8_enter_keys_nothing_and_lands_on_the_watering_hole() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        assert!(
            e.settings().js8_autoreply,
            "JS8Call default ON — and still nothing keys"
        );
        e.js8_enter();
        assert_eq!(e.tier(), Tier::Js8);
        assert!(
            (e.settings().dial_mhz - 14.078).abs() < 1e-6,
            "20 m JS8: {}",
            e.settings().dial_mhz
        );
        assert!(!e.tx_enabled(), "launch is listen-only");
        for slot in 0..8 {
            assert!(
                e.poll_tx(slot).is_empty(),
                "slot {slot}: launch is listen-only, nothing keys"
            );
        }
        let st = e.js8_state();
        assert_eq!(st.speed, Js8Speed::Normal);
        assert_eq!(st.rx_speeds, 31, "every speed decodes, JS8 60 too");
        assert!(
            !st.armed.autoreply && !st.armed.relay && !st.armed.hb_ack && !st.armed.hb,
            "latch down at launch → nothing is armed"
        );
        assert!(st.queue.is_empty() && st.pending_reply.is_none() && st.activity.is_empty());
    }

    /// A JS8 heartbeat's grid is somewhere the station told us it is: the rotator's point-at-call
    /// reads it from the heard list (`Engine::station_grids`), after the roster's.
    #[test]
    fn a_heard_js8_grid_reaches_the_point_at_call() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        assert!(e.station_grids("EC1DD").heard.is_empty());
        e.js8_ingest(
            &[row(&hb("EC1DD", "IN52"), whole(), Js8Speed::Normal, 1500.0)],
            4,
        );
        assert_eq!(e.station_grids("ec1dd").heard, vec!["IN52".to_string()]);
    }

    /// The RX chain end to end: `Decode.raw` → `RawDecode` → `Reassembler` → `Station`, with the
    /// activity pane and the heard list populated. B7 removed the receive-only override, so a
    /// query addressed to me now schedules a SHOWN autoreply countdown (autoreply is JS8Call's
    /// default) — it is a pending countdown, not yet an outbox frame, and nothing keys here
    /// because the TX latch is down (proved end to end by `js8_autoreply_never_keys_at_launch`).
    #[test]
    fn js8_ingest_feeds_the_station_and_a_directed_query_schedules_a_shown_countdown() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        let heartbeat = hb("KD2UWR", "FN30");
        e.js8_ingest(&[row(&heartbeat, whole(), Js8Speed::Normal, 1500.0)], 4);
        let st = e.js8_state();
        assert_eq!(st.activity.len(), 1);
        assert_eq!(st.activity[0].from, "KD2UWR");
        assert_eq!(st.activity[0].text, heartbeat.render());
        assert_eq!(st.activity[0].speed, Js8Speed::Normal);
        assert!(!st.activity[0].directed_to_me && !st.activity[0].mine && !st.activity[0].low_conf);
        assert!(
            st.stations.iter().any(|h| h.call == "KD2UWR"),
            "the heard list learned the station"
        );

        let query = Frame::Directed {
            from: CallRef::Base("KD2UWR".to_string()),
            to: CallRef::Base("KD9TAW".to_string()),
            cmd: Command::SnrQuery,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        let mut d = row(&query, whole(), Js8Speed::Fast, 1200.0);
        d.qual = 0.1; // JS8Call's low-confidence line
        e.js8_ingest(&[d], 5);
        let st = e.js8_state();
        assert_eq!(st.activity.len(), 2);
        assert!(st.activity[1].directed_to_me, "SNR? to my call");
        assert!(st.activity[1].low_conf);
        assert_eq!(st.activity[1].speed, Js8Speed::Fast);
        assert!(
            st.queue.is_empty(),
            "the reply is a pending countdown, not yet an outbox frame"
        );
        assert!(
            st.pending_reply.is_some(),
            "autoreply ON (the B5 override is gone): the query gets a shown countdown"
        );
        assert!(
            e.js8_station.config().autoreply,
            "the station now carries the operator's real autoreply switch"
        );
        assert!(
            e.settings().js8_autoreply,
            "…matching the persisted JS8Call default"
        );
        // The OTHER half of the pair (a test that only checked the first half would pass on a
        // build that keys the shown reply): the countdown is SHOWN but does NOT key, because
        // the TX latch is down (never armed here). Proved end to end by
        // `js8_autoreply_never_keys_at_launch`; asserted here so this test is self-contained.
        assert!(!e.tx_enabled(), "the latch was never armed");
        let s0 = now_unix_secs() / 15;
        for s in s0..s0 + 4 {
            assert!(
                e.poll_tx(s).is_empty(),
                "the shown reply must not key with the latch down (slot {s})"
            );
        }
        assert!(
            !e.snapshot().recent_decodes.iter().any(|d| d.mine),
            "nothing was booked as an own-TX row"
        );
    }

    /// The boundary pass re-decodes the tier speed a second or two after the multi-speed
    /// pass folded the same word: the duplicate is dropped at the row chokepoint. A GENUINE
    /// repeat (the same word a period later) is kept — the window is ¾ period, not forever.
    #[test]
    fn js8_dedupe_drops_the_boundary_duplicate_but_keeps_a_later_repeat() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        let d = row(&hb("W0IND", "EN52"), whole(), Js8Speed::Turbo, 900.0);
        assert_eq!(
            e.js8_dedupe(vec![d.clone()]).len(),
            1,
            "first sighting passes"
        );
        assert_eq!(
            e.js8_dedupe(vec![d.clone()]).len(),
            0,
            "the same word again = the boundary duplicate"
        );
        // Age the sighting past ¾ of Turbo's 6 s period: a repeat is a new transmission.
        e.js8_seen[0].1 -= 5_000;
        assert_eq!(
            e.js8_dedupe(vec![d.clone()]).len(),
            1,
            "a later repeat is kept"
        );
        // Rows that are not JS8 pass straight through, untouched.
        let ft8 = modes::Decode {
            raw: None,
            mode: Some(modes::ModeKind::Ft8),
            ..d.clone()
        };
        assert_eq!(e.js8_dedupe(vec![ft8.clone(), ft8]).len(), 2);
    }

    /// A store-and-forward `MSG TO:` lands in the inbox as `Store`, the journal is handed to
    /// its thread the moment it changes, and a fresh engine restores it — the `pending_msgs.json`
    /// contract for JS8's store. (The B4 Station inboxes store-and-forward traffic only; a
    /// direct `MSG` to me is an ACTIVITY row, as JS8Call itself does. Storing is a receive
    /// action, so it works in the receive-only build; DELIVERY, which is TX, does not.)
    #[test]
    fn js8_inbox_journal_round_trips() {
        let dir = std::env::temp_dir().join(format!("nexus-js8-journal-{}", std::process::id()));
        let path = dir.join("js8_station.json");
        let _ = std::fs::remove_file(&path);
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_js8_journal_path(path.clone());
        e.js8_enter();
        let frames = ::js8::proto::compose::frames(
            "W1AW",
            Some(&CallRef::Base("KD9TAW".to_string())),
            "MSG TO:K1ABC FRIDAY CONTACT",
            Js8Speed::Normal,
        )
        .expect("composes");
        assert!(
            frames.len() >= 2,
            "a MSG TO: is a directed frame plus data frame(s)"
        );
        for (i, (f, i3)) in frames.iter().enumerate() {
            e.js8_ingest(&[row(f, *i3, Js8Speed::Normal, 1750.0)], 10 + i as u64);
        }
        let st = e.js8_state();
        assert_eq!(st.inbox.len(), 1, "MSG TO: is stored");
        assert_eq!(st.inbox[0].from, "W1AW");
        assert_eq!(
            st.inbox[0].to, "K1ABC",
            "the store target is the first token of the body"
        );
        assert_eq!(st.inbox[0].state, InboxState::Store);
        assert!(
            st.queue.is_empty(),
            "receive-only: nothing is queued for delivery"
        );
        e.station.journals.settle(); // written on the journals' own thread
        assert!(path.exists(), "the journal is written on InboxChanged");
        let id = st.inbox[0].id;
        e.js8_inbox_mark(id, InboxState::Read).unwrap();
        assert!(e.js8_inbox_mark(id + 1000, InboxState::Read).is_err());
        e.station.journals.settle();

        let mut fresh = Engine::new("KD9TAW", "EN52", 0);
        fresh.set_js8_journal_path(path.clone());
        fresh.js8_load_journal(&std::fs::read_to_string(&path).unwrap());
        let st = fresh.js8_state();
        assert_eq!(st.inbox.len(), 1);
        assert_eq!(st.inbox[0].state, InboxState::Read);
        fresh.js8_inbox_delete(id).unwrap();
        assert!(fresh.js8_state().inbox.is_empty());
        drop((e, fresh)); // their journal writes land before the folder goes
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A folder of the test's own with a FIFO at `name` inside it — the temporary file a
    /// journal write opens first. Opening a FIFO for writing blocks until something opens it
    /// for reading: a disk that will not take the write. Exactly ONE write may be sent at it.
    #[cfg(unix)]
    fn stalled(tag: &str, name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexus-js8-stall-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(dir.join(name))
            .status()
            .expect("mkfifo runs");
        assert!(made.success(), "premise: a FIFO to stall the write on");
        dir
    }

    /// Read the FIFO at `fifo` to its end on a thread of its own, which lets the one write
    /// stalled on it through (it then fails at its fsync, as a write to a FIFO does, and is
    /// reported like any failed journal write). The bytes come back, or the wait times out.
    #[cfg(unix)]
    fn release(fifo: PathBuf) -> Vec<u8> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = std::fs::File::open(&fifo).expect("open the FIFO for reading");
            let mut got = Vec::new();
            let _ = std::io::Read::read_to_end(&mut reader, &mut got);
            let _ = tx.send(got);
        });
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("a journal write was waiting on the FIFO")
    }

    /// The frames of a store-and-forward `MSG TO:` for K1ABC, as W1AW sends it to KD9TAW.
    fn msg_to_frames() -> Vec<(Frame, I3)> {
        ::js8::proto::compose::frames(
            "W1AW",
            Some(&CallRef::Base("KD9TAW".to_string())),
            "MSG TO:K1ABC FRIDAY CONTACT",
            Js8Speed::Normal,
        )
        .expect("composes")
    }

    /// ★ THE RADIO LOOP DOES NOT WAIT ON THE JS8 JOURNAL'S DISK. A stored message arrives
    /// through `js8_ingest` — the radio loop's decode path, under the engine lock — and the
    /// station journals its inbox. With the journal's disk stalled the ingest comes back at
    /// once, and the lock is free while the write waits on the disk.
    ///
    /// The control is the stall itself: nothing is on disk until the FIFO is read, and the
    /// write that was waiting there carries the message.
    #[cfg(unix)]
    #[test]
    fn a_stored_message_under_the_lock_does_not_wait_for_a_stalled_js8_journal() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        let dir = stalled("ingest", "js8_station.json.tmp");
        let journal = dir.join("js8_station.json");
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_js8_journal_path(journal.clone());
        e.js8_enter();
        let engine = Arc::new(Mutex::new(e));
        let (tx, rx) = std::sync::mpsc::channel();
        let ingest = {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                let mut e = crate::engine::engine_lock(&engine);
                let t = Instant::now();
                for (i, (f, i3)) in msg_to_frames().iter().enumerate() {
                    e.js8_ingest(&[row(f, *i3, Js8Speed::Normal, 1750.0)], 10 + i as u64);
                }
                let _ = tx.send((e.js8_state().inbox.len(), t.elapsed()));
            })
        };
        let answered = rx.recv_timeout(Duration::from_secs(2));
        let (lock_free, on_disk) = if answered.is_ok() {
            ingest.join().expect("joined");
            (
                crate::engine::engine_try_lock(&engine).is_ok(),
                journal.exists(),
            )
        } else {
            (false, false)
        };
        // Let the stalled write through whatever happened, so a red cannot hang the suite.
        let drained = release(dir.join("js8_station.json.tmp"));
        let (stored, took) = answered.expect(
            "the ingest must come back under the lock while the journal's disk is stalled — \
             it waited for the journal",
        );
        assert_eq!(stored, 1, "the message is stored");
        assert!(
            took < Duration::from_millis(500),
            "returned at once: {took:?}"
        );
        assert!(
            lock_free,
            "the lock is free while the journal write waits on the disk"
        );
        assert!(!on_disk, "control: the journal really was stalled");
        assert!(
            String::from_utf8_lossy(&drained).contains("FRIDAY CONTACT"),
            "and the stalled write was the inbox's journal"
        );
        crate::engine::engine_lock(&engine)
            .station
            .journals
            .settle();
        drop(engine);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ THE ORDER AND THE CONTENT ARE THE STATION'S. With the journals' thread held up behind
    /// a stalled write of the message queue's journal, three inbox changes queue behind it —
    /// read, unread, delete — and each hands over the station's snapshot at that moment. The
    /// journal ends as the LAST change left it, byte for byte: a write reordered past a later
    /// one would leave the message in the file.
    #[cfg(unix)]
    #[test]
    fn js8_journal_writes_land_in_the_order_the_inbox_changed() {
        let dir = stalled("order", "pending_msgs.json.tmp");
        let journal = dir.join("js8_station.json");
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_js8_journal_path(journal.clone());
        e.set_pending_msgs_path(dir.join("pending_msgs.json"));
        e.js8_enter();
        e.send_message("W1ABC", "hi"); // the queue's journal write: held on the FIFO
        for (i, (f, i3)) in msg_to_frames().iter().enumerate() {
            e.js8_ingest(&[row(f, *i3, Js8Speed::Normal, 1750.0)], 10 + i as u64);
        }
        let id = e.js8_state().inbox[0].id;
        e.js8_inbox_mark(id, InboxState::Read).unwrap();
        e.js8_inbox_mark(id, InboxState::Unread).unwrap();
        e.js8_inbox_delete(id).unwrap();
        let last = serde_json::to_string(&e.js8_station.snapshot(0)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let queued = !journal.exists();
        release(dir.join("pending_msgs.json.tmp"));
        e.station.journals.settle();
        assert!(
            queued,
            "control: the thread really was held — nothing behind the stall was written yet"
        );
        assert_eq!(
            std::fs::read_to_string(&journal).unwrap(),
            last,
            "the journal is the last change's snapshot, byte for byte"
        );
        assert!(
            !last.contains("FRIDAY CONTACT"),
            "premise: the last change removed the message"
        );
        drop(e);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A speed change re-points the decoder at the new window and the station at the new
    /// period, refuses a bad index, and never touches the latch.
    #[test]
    fn js8_set_speed_rebuilds_the_kind_and_refuses_a_bad_index() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        assert!(e.js8_set_speed(5).is_err(), "past JS8 60, the last speed");
        e.js8_set_speed(3).unwrap();
        assert_eq!(e.settings().js8_speed, 3);
        assert_eq!(
            e.active_slot_secs(),
            6.0,
            "Turbo's period drives the slot clock"
        );
        assert_eq!(e.active_capture_samples(), 432_000, "the ring stays 36 s");
        assert_eq!(e.js8_station.config().speed, Js8Speed::Turbo);
        assert_eq!(e.js8_state().speed, Js8Speed::Turbo);
        assert!(
            e.js8_set_rx_speeds(0).is_err(),
            "a mask that decodes nothing is refused"
        );
        e.js8_set_rx_speeds(0b0110).unwrap();
        assert_eq!(e.js8_state().rx_speeds, 6);
        e.js8_set_rx_speeds(0b1_0000).unwrap();
        assert_eq!(
            e.js8_state().rx_speeds,
            16,
            "JS8 60 alone is a mask that decodes"
        );
        assert!(
            e.js8_set_rx_speeds(0b10_0000).is_err(),
            "a bit past the five speeds decodes nothing"
        );
        assert!(!e.tx_enabled());
    }

    /// JS8 60, JS8Call 3.0's fifth speed (submode I, index 4): picking it points the slot
    /// clock at its 4 s period and the decoder at its window, and the station transmits at
    /// it — the speed's numbers BY VALUE, so the test reads the same before and after the
    /// table grew a row.
    #[test]
    fn js8_set_speed_4_is_js8_60_on_the_slot_clock_the_decoder_and_the_station() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        e.js8_set_speed(4).expect("JS8 60 is index 4");
        assert_eq!(e.settings().js8_speed, 4);
        let tx = e.js8_tx_speed();
        assert_eq!(
            (tx.letter(), tx.period_s(), tx.nsps(), tx.delay_ms()),
            ('I', 4, 384, 100),
            "3.0.3 commons.h:45-47"
        );
        assert_eq!(
            e.active_slot_secs(),
            4.0,
            "JS8 60's period drives the slot clock"
        );
        assert_eq!(e.js8_station.config().speed, tx);
        assert_eq!(e.js8_state().speed, tx);
        assert!(
            (e.tx_over_secs() - 2.628).abs() < 1e-3,
            "100 ms start delay + 79 symbols of 384 samples"
        );
        assert!(!e.tx_enabled(), "a speed pick never arms TX");
    }

    // REMOVED at B7.7: `js8_tx_verbs_refuse_in_the_receive_only_build` tested a state that no
    // longer exists — the receive-only build where EVERY transmit verb refuses. B7.7 makes
    // `js8_arm` a real verb (the two-act arm), and B7.8 does the same for `js8_send` /
    // `js8_send_command` / `js8_call_cq`. The two-act arm tests in engine.rs (js8_autoreply_
    // never_keys_at_launch, a_js8_switch_without_the_tx_latch_is_silent, both_acts_present_…)
    // and B7.8's operator-verb tests replace it — a real replacement, not a deletion.

    /// A real multi-speed job: a Normal heartbeat and a Turbo heartbeat, each in its own
    /// speed's window, decoded in parallel under `std::thread::scope`, folding into the
    /// activity pane with the right speed on each row — and NOT reaching the boundary path.
    #[test]
    fn run_js8_multi_job_decodes_each_slice_and_folds_as_early() {
        use super::super::{run_js8_multi_job, DecodeApplied, DecodePass};
        fn slice(frame: &Frame, speed: Js8Speed, f0: f32) -> Vec<f32> {
            let word = encode_frame(frame, whole(), speed).expect("packable");
            let tones: Vec<i32> = ::js8::phy::encode_word(&word, speed)
                .iter()
                .map(|&t| i32::from(t))
                .collect();
            let m = modes::make_mode(modes::ModeKind::Js8 { speed });
            let wave = m.gen_wave(&tones, 12_000.0, f0);
            // Capture scale: the engine's `capture_to_i16` multiplies by 32767, so a ±0.03
            // wave lands at ±1000 — the same level the modes tests decode cleanly.
            let mut out = vec![0.0f32; speed.frames_needed()];
            for (dst, &s) in out.iter_mut().zip(&wave) {
                *dst = s * 0.03;
            }
            out
        }
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        // The tier switch bumped the decode epoch; the radio loop re-syncs the capture epoch
        // at every consumed boundary (`begin_slot_capture`). Without it the result is Stale.
        e.begin_slot_capture();
        let job = e.build_js8_multi_job(
            vec![
                (
                    Js8Speed::Normal,
                    slice(&hb("KD2UWR", "FN30"), Js8Speed::Normal, 1500.0),
                    0,
                ),
                (
                    Js8Speed::Turbo,
                    slice(&hb("W0IND", "EN52"), Js8Speed::Turbo, 900.0),
                    0,
                ),
            ],
            7,
        );
        let results = run_js8_multi_job(job);
        assert_eq!(results.len(), 2);
        let mut folded = 0;
        for r in results {
            assert!(matches!(r.pass(), DecodePass::Js8Multi { .. }));
            match e.apply_decode_result(r) {
                DecodeApplied::Early { n } => folded += n,
                _ => panic!("a Js8Multi result must fold as Early"),
            }
        }
        assert_eq!(folded, 2, "one decode per slice");
        let st = e.js8_state();
        assert_eq!(st.activity.len(), 2);
        assert!(st
            .activity
            .iter()
            .any(|r| r.speed == Js8Speed::Normal && r.from == "KD2UWR"));
        assert!(st
            .activity
            .iter()
            .any(|r| r.speed == Js8Speed::Turbo && r.from == "W0IND"));
        assert!(st
            .stations
            .iter()
            .any(|h| h.call == "W0IND" && h.speed == Js8Speed::Turbo));
    }

    // ===== the heartbeat against JS8Call (the 2026-09-29 report) =====

    /// A JS8 engine on 20 m with the TX latch up, the heartbeat interval `hb_min`, the
    /// operator's locator `grid` and TX offset `offset_hz`. The heartbeat is not armed.
    /// An engine on 20 m with TX on, whose automatic replies go by themselves (the confirmation
    /// off), so a test sees each one on the air; `asking_engine` is JS8Call's default.
    fn hb_engine(grid: &str, hb_min: u16, offset_hz: f32) -> Engine {
        let mut e = Engine::new("KD9TAW", grid, 0);
        e.settings.js8_hb_interval_min = hb_min;
        e.settings.js8_autoreply_confirmation = false;
        // Heartbeat networking on: JS8Call sends no heartbeat and no HB ACK without it.
        e.settings.js8_hb_networking = true;
        e.js8_apply_station_config();
        e.js8_enter();
        e.set_frequency(14.078, "20m", "USB");
        e.set_tx_offset(offset_hz);
        e.set_tx_enabled(true);
        e
    }

    /// The radio loop's JS8 shape, from now on: the once-a-second `js8_tick` and, at every
    /// period boundary, the transmit decision (`poll_tx`'s plan → build → commit, the plan
    /// kept for its offset). Returns (period start, f0) for each over that keyed.
    fn run_js8_loop(e: &mut Engine, secs: u64) -> Vec<(u64, f32)> {
        run_js8_loop_from(e, tempo_core::timing::now_unix_ms() as u64, secs)
    }

    /// `run_js8_loop` from `t0` (ms, the wall-clock axis) instead of from now.
    fn run_js8_loop_from(e: &mut Engine, t0: u64, secs: u64) -> Vec<(u64, f32)> {
        let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
        let mut last = t0 / period_ms;
        let mut overs = Vec::new();
        for k in 0..=secs {
            let t = t0 + k * 1000;
            e.js8_tick(t);
            let slot = t / period_ms;
            if slot == last {
                continue;
            }
            last = slot;
            let Some(plan) = e.plan_tx(slot) else {
                continue;
            };
            let TxWaveform::Js8 { f0, .. } = &plan.waveform else {
                panic!("a JS8 plan carries the typed waveform");
            };
            let f0 = *f0;
            let wave = plan.waveform.build();
            if !e.commit_tx(&plan, wave, slot).is_empty() {
                overs.push((slot * period_ms, f0));
            }
        }
        overs
    }

    /// "Then it sends it on every frame." On the real path, from arming until well past the
    /// first heartbeat, exactly ONE heartbeat over keys. The operator's locator in Settings may
    /// be six characters; JS8Call's heartbeat carries four (`my_grid().left(4)`), and the rest
    /// used to spill into a second frame that keyed on the next period.
    #[test]
    fn a_due_js8_heartbeat_keys_exactly_once_in_the_periods_after_it() {
        for grid in ["EN52", "EN52HW"] {
            let mut e = hb_engine(grid, 5, 1500.0);
            e.js8_arm(Js8Switch::Hb, true).expect("HB on");
            let overs = run_js8_loop(&mut e, 8 * 60);
            assert_eq!(
                overs.len(),
                1,
                "{grid}: one heartbeat over in the eight minutes after arming, got {overs:?}"
            );
        }
    }

    /// "The first send appears to be wherever you are." The heartbeat KEYED in the 500–1000 Hz
    /// sub-band, but every record of it carried the operator's own offset: the Activity row
    /// (and so the Band Activity by offset pane), the Rx-Frequency own-TX row and the ALL.TXT
    /// `Tx` line. JS8Call moves `freq()` to the heartbeat's offset before the frame is shown
    /// (`setFreqOffsetForRestore`, :9681, then `displayTextForFreq(…, freq(), …)`, :5538).
    #[test]
    fn a_js8_heartbeat_is_booked_at_the_offset_it_keyed() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.settings.write_all_txt = true;
        e.js8_arm(Js8Switch::Hb, true).expect("HB on"); // interval 0: the next period
        let overs = run_js8_loop(&mut e, 20);
        let [(start, f0)] = overs[..] else {
            panic!("one heartbeat over, got {overs:?}");
        };
        assert!(
            (500.0..=1000.0).contains(&f0),
            "keyed in the sub-band, clear of the operator's 1500 Hz: {f0}"
        );
        let row = e
            .js8_state()
            .activity
            .last()
            .cloned()
            .expect("the over's row");
        assert_eq!(
            (row.mine, row.freq_hz),
            (true, f0),
            "the Activity row sits where the over keyed"
        );
        let own = e
            .snapshot()
            .recent_decodes
            .into_iter()
            .rfind(|d| d.mine)
            .expect("the own-TX row");
        assert_eq!(own.freq_hz, f0, "…and so does the Rx-Frequency own-TX row");
        let want = crate::alltxt::all_txt_line(
            start / 1000,
            14.078,
            true,
            "JS8",
            0,
            0.0,
            f0,
            // JS8Call's letter for Normal before the text, as a JS8 frame's line has it.
            "A KD9TAW: @HB HEARTBEAT EN52",
        );
        assert_eq!(
            e.station.all_txt_pending.last(),
            Some(&want),
            "…and the ALL.TXT Tx line"
        );
    }

    /// JS8Call's `sendHeartbeat` (:6279-6281): an operator at or below 1000 Hz beacons on their
    /// own offset; only above it does the heartbeat move to a free sub-band offset.
    #[test]
    fn a_js8_heartbeat_keys_on_the_operators_offset_at_or_below_1000_hz() {
        for (offset, own) in [(777.0, true), (1000.0, true), (1001.0, false)] {
            let mut e = hb_engine("EN52", 0, offset);
            e.js8_arm(Js8Switch::Hb, true).expect("HB on");
            let overs = run_js8_loop(&mut e, 20);
            let [(_, f0)] = overs[..] else {
                panic!("{offset} Hz: one heartbeat over, got {overs:?}");
            };
            if own {
                assert_eq!(
                    f0, offset,
                    "at {offset} Hz the heartbeat keys on the operator's offset"
                );
            } else {
                assert!(
                    (500.0..1000.0).contains(&f0),
                    "above 1000 Hz it moves to a free sub-band offset: {f0}"
                );
            }
        }
    }

    /// The free-offset test is JS8Call's `isFreqOffsetFree(f, 50)` (:5566): nothing heard within
    /// 50 Hz in the last 30 s. The 50 is `findFreeFreqOffset(500, 1000, 50)`'s literal argument
    /// at every speed, not the signal's bandwidth, so at Fast (80 Hz wide; JS8Call sends no
    /// heartbeat at Turbo) a station heard at 700 Hz keeps the heartbeat off 700 and not off its
    /// neighbours 650 and 750, which a bandwidth rule would take too.
    #[test]
    fn a_heard_station_keeps_the_js8_heartbeat_off_fifty_hertz_around_it_at_every_speed() {
        let mut offsets = std::collections::BTreeSet::new();
        for seed in 1..=40u32 {
            let mut e = hb_engine("EN52", 0, 1500.0);
            e.js8_set_speed(2).expect("Fast");
            let slot = now_unix_secs() / 10 + 1;
            e.js8_ingest(
                &[row(&hb("W1AW", "FN31"), whole(), Js8Speed::Fast, 700.0)],
                slot - 1,
            );
            e.js8_arm(Js8Switch::Hb, true).expect("HB on");
            e.js8_tick(tempo_core::timing::now_unix_ms() as u64);
            e.js8_rng = seed.wrapping_mul(2_654_435_761) | 1;
            let plan = e.plan_tx(slot).expect("the heartbeat");
            let TxWaveform::Js8 { f0, .. } = &plan.waveform else {
                panic!("a JS8 plan carries the typed waveform");
            };
            offsets.insert(*f0 as u32);
        }
        assert!(
            !offsets.contains(&700),
            "control: never on the station heard there, {offsets:?}"
        );
        assert!(
            offsets.iter().any(|f| (550..=850).contains(f) && *f != 700),
            "only 50 Hz either side of 700 is busy, so its neighbours are used: {offsets:?}"
        );
        assert!(
            offsets.contains(&650) || offsets.contains(&750),
            "650 and 750 are 50 Hz off, free, though inside Fast's 80 Hz: {offsets:?}"
        );
    }

    /// Symptom 1 is JS8Call's behaviour and stays: turning the heartbeat on with a 5-minute
    /// interval keys NOTHING until one interval after the next transmit cycle
    /// (`on_hbMacroButton_toggled`, :6319), and then keys in exactly that period.
    #[test]
    fn a_js8_heartbeat_turned_on_keys_nothing_until_one_interval_after_the_next_cycle() {
        let mut e = hb_engine("EN52", 5, 1500.0);
        // JS8Call's `nextTransmitCycle()`: whole seconds, the NEXT 15 s boundary, + 1 s.
        let cycle = |t: u64| (t / 1000 / 15 * 15 + 15 + 1) * 1000;
        let before = tempo_core::timing::now_unix_ms() as u64;
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        let after = tempo_core::timing::now_unix_ms() as u64;
        let next = e.js8_state().hb_next_at_ms.expect("scheduled");
        assert!(
            [cycle(before), cycle(after)].contains(&(next - 300_000)),
            "the countdown ends one interval after the next transmit cycle: {next}, armed {before}..{after}"
        );
        let overs = run_js8_loop(&mut e, 6 * 60);
        assert_eq!(
            overs.iter().map(|o| o.0).collect::<Vec<_>>(),
            vec![next - 1_000],
            "nothing before, then the heartbeat in the period one interval after the next cycle"
        );
    }

    /// The clicked CQ on the real path: with a 6-character locator in Settings it is ONE over
    /// (JS8Call's CQ carries `my_grid().left(4)`, mainwindow.cpp:6344), booked with the square.
    #[test]
    fn a_js8_cq_with_a_six_character_locator_keys_one_over() {
        let mut e = hb_engine("EN52HW", 0, 1500.0);
        e.js8_call_cq().expect("CQ queued");
        let overs = run_js8_loop(&mut e, 60);
        assert_eq!(
            overs.len(),
            1,
            "one CQ over in the minute after the click, got {overs:?}"
        );
        let booked: Vec<String> = e
            .js8_state()
            .activity
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.text.clone())
            .collect();
        assert_eq!(
            booked,
            vec!["KD9TAW: @ALLCALL CQ CQ CQ EN52".to_string()],
            "…booked once, with the square it carried"
        );
    }

    /// The compose box's word check and suggestions reach the dictionary.
    #[test]
    fn the_word_check_and_suggestions_answer_from_the_dictionary() {
        let words: Vec<String> = ["HELLOX", "HELLO", "KD9TAW", "TNX"]
            .iter()
            .map(|w| w.to_string())
            .collect();
        assert_eq!(js8_word_check(&words), vec![true, false, false, false]);
        let s = js8_word_suggestions("HELLOX");
        assert!(
            s.found && s.words.first().map(String::as_str) == Some("HELLO"),
            "{s:?}"
        );
    }

    /// What the operator's own CQs read as, booked in the activity (the text each over carried).
    fn booked_mine(e: &Engine) -> Vec<String> {
        e.js8_state()
            .activity
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.text.clone())
            .collect()
    }

    /// The CQ message is a Setting (JS8Call's `CQMessage`): set to `CQ DX <MYGRID4>`, the clicked
    /// CQ keys one over on the real path, booked as the CQ DX it carried, and the CQ repeat keys
    /// the same words from the same Setting.
    #[test]
    fn the_cq_message_in_settings_is_what_the_js8_cq_and_its_repeat_key() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.settings.js8_cq_text = "CQ DX <MYGRID4>".into();
        e.js8_apply_station_config();
        e.js8_call_cq().expect("CQ queued");
        let overs = run_js8_loop(&mut e, 60);
        assert_eq!(overs.len(), 1, "one CQ over, got {overs:?}");
        assert_eq!(
            booked_mine(&e),
            vec!["KD9TAW: @ALLCALL CQ DX EN52".to_string()]
        );

        let mut e = hb_engine("EN52", 0, 1500.0);
        e.settings.js8_cq_text = "CQ QRP <MYGRID4>".into();
        e.settings.js8_cq_interval_min = 1;
        e.js8_apply_station_config();
        e.js8_set_cq_repeat(true).expect("CQ repeat on");
        let overs = run_js8_loop(&mut e, 150);
        assert!(!overs.is_empty(), "control: the repeat keyed");
        let booked = booked_mine(&e);
        assert!(
            !booked.is_empty() && booked.iter().all(|t| t == "KD9TAW: @ALLCALL CQ QRP EN52"),
            "every repeated CQ is the CQ message: {booked:?}"
        );
    }

    /// An HB-ACK keys on a free heartbeat spot, as JS8Call's `sendHeartbeatAck` picks it
    /// (`findFreeFreqOffset(500, 1000, 50)`, mainwindow.cpp:6299), and is booked there. Unlike
    /// the heartbeat's (`sendHeartbeat`, :6279), that pick has no "at or below 1000 Hz, stay on
    /// your own offset" rule, so an operator at 777 Hz still gets a spot (777 is never one).
    #[test]
    fn a_js8_hb_ack_keys_on_a_free_heartbeat_spot_not_the_operators_offset() {
        let mut e = hb_engine("EN52", 30, 777.0); // our own heartbeat is 30 min away
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        e.js8_arm(Js8Switch::HbAck, true).expect("HB-ack on");
        e.js8_ingest(
            &[row(&hb("W1AW", "FN31"), whole(), Js8Speed::Normal, 1200.0)],
            now_unix_secs() / 15,
        );
        let overs = run_js8_loop(&mut e, 60);
        let [(_, f0)] = overs[..] else {
            panic!("one HB-ACK over, got {overs:?}");
        };
        assert!(
            f0 != 777.0 && (500.0..=950.0).contains(&f0) && f0 % 50.0 == 0.0,
            "the HB-ACK keys on a free heartbeat spot, not the operator's 777 Hz: {f0}"
        );
        let row = e
            .js8_state()
            .activity
            .last()
            .cloned()
            .expect("the over's row");
        assert_eq!(
            (row.text.as_str(), row.freq_hz),
            ("KD9TAW: W1AW HEARTBEAT SNR -07", f0),
            "…and it is booked where it keyed"
        );
    }

    /// TX on, AUTOREPLY and HB ACK on, heartbeat networking off: JS8Call answers a heartbeat only
    /// in its heartbeat networking mode with AUTO and the acknowledgements on
    /// (processCommandActivity.cpp:634-638; 2.3: mainwindow.cpp:9061), and so does the station
    /// (`maybe_hb_ack`), so a heard heartbeat draws nothing. The HB ACK chip must not read armed
    /// then, and it names networking as what it needs. With networking on it reads armed, and
    /// the heartbeat is answered on the air, the HB repeat still off: the repeat is not what an
    /// acknowledgement needs.
    #[test]
    fn hb_ack_without_heartbeat_networking_is_not_armed_and_says_what_it_needs() {
        let mut e = hb_engine("EN52", 30, 1500.0); // our own heartbeat would be 30 min away
        e.js8_arm(Js8Switch::HbNet, false)
            .expect("heartbeat networking off");
        e.js8_arm(Js8Switch::HbAck, true).expect("HB ACK on");
        let heard = |e: &mut Engine| {
            e.js8_ingest(
                &[row(&hb("W1AW", "FN31"), whole(), Js8Speed::Normal, 1200.0)],
                now_unix_secs() / 15,
            )
        };
        let st = e.js8_state();
        assert!(
            st.tx_enabled && st.autoreply && st.hb_ack && !st.hb_on,
            "precondition: TX, AUTOREPLY and HB ACK on, HB off"
        );
        assert_eq!(
            (st.armed.hb_ack, st.not_armed.hb_ack),
            (false, Some(Js8Unarmed::NeedsHb)),
            "HB ACK is not armed while heartbeat networking is off, and says that is what it \
             needs"
        );
        heard(&mut e);
        let overs = run_js8_loop(&mut e, 60);
        assert!(
            overs.is_empty(),
            "…and a heard heartbeat draws nothing: {overs:?}"
        );

        e.js8_arm(Js8Switch::HbNet, true)
            .expect("heartbeat networking on");
        let st = e.js8_state();
        assert_eq!(
            (st.armed.hb_ack, st.not_armed.hb_ack, st.hb_on),
            (true, None, false),
            "control: with heartbeat networking on, HB ACK is armed, the HB repeat still off"
        );
        heard(&mut e);
        let overs = run_js8_loop(&mut e, 60);
        let booked: Vec<String> = e
            .js8_state()
            .activity
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.text.clone())
            .collect();
        assert_eq!(
            booked,
            ["KD9TAW: W1AW HEARTBEAT SNR -07"],
            "…and the heartbeat is answered on the air: {overs:?}"
        );
    }

    /// A heartbeat that falls due while the TX latch is down is dropped and its interval
    /// re-based, as JS8Call's is (`startTx` → `ensureCanTransmit` fails →
    /// `on_stopTxButton_clicked`, mainwindow.cpp:5304 → :7397), so turning TX back on sends
    /// nothing.
    #[test]
    fn a_js8_heartbeat_due_while_tx_is_off_is_not_sent_when_tx_comes_back() {
        let mut e = hb_engine("EN52", 5, 1500.0);
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        let due = e.js8_state().hb_next_at_ms.expect("scheduled");
        e.set_tx_enabled(false);
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let off = run_js8_loop_from(&mut e, t0, 6 * 60); // through the due moment, latch down
        assert!(off.is_empty(), "control: nothing keys with the latch down");
        let rebased = e.js8_state().hb_next_at_ms;
        e.set_tx_enabled(true);
        let on = run_js8_loop_from(&mut e, t0 + 6 * 60 * 1000, 2 * 60);
        assert!(on.is_empty(), "turning TX back on sends nothing: {on:?}");
        // Dropped in its due second (the boundary before `due` + under 1 s), so the next cycle
        // is that boundary + 16 s, and the interval goes on top: `due` + 315 s.
        assert_eq!(
            rebased,
            Some(due + 315_000),
            "…because it was dropped and re-based to the next cycle + the interval"
        );
    }

    /// Interval 0 is JS8Call's single press: one heartbeat, once (`on_hbMacroButton_toggled`,
    /// :6326).
    #[test]
    fn an_on_demand_js8_heartbeat_keys_once() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        let overs = run_js8_loop(&mut e, 3 * 60);
        assert_eq!(
            overs.len(),
            1,
            "one heartbeat in three minutes, got {overs:?}"
        );
    }

    // ===== no locator in Settings: JS8Call transmits nothing =====

    /// The refusal the operator reads, word for word.
    const NO_LOCATOR: &str =
        "Set your Maidenhead grid (e.g. EN52) in Settings before transmitting JS8.";

    /// `SNR?` from `from` to my call, as it decodes.
    fn snr_query_to_me(from: &str) -> modes::Decode {
        let query = Frame::Directed {
            from: CallRef::Base(from.to_string()),
            to: CallRef::Base("KD9TAW".to_string()),
            cmd: Command::SnrQuery,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        row(&query, whole(), Js8Speed::Normal, 1200.0)
    }

    /// JS8Call's Enter asks `ensureCallsignSet` before anything is queued (mainwindow.cpp:749),
    /// and with no locator that refuses (:5264-5268, "Please enter your grid locator in the
    /// settings."). Every operator send refuses, queues nothing and says where the locator goes.
    #[test]
    fn js8_sends_refuse_with_no_locator_and_say_where_to_set_it() {
        for grid in ["", "  "] {
            let mut e = hb_engine(grid, 5, 1500.0);
            let refused = Err(NO_LOCATOR.to_string());
            assert_eq!(
                e.js8_send(None, "TEST".into()),
                refused,
                "{grid:?}: a message"
            );
            let snr = Command::SnrQuery.id();
            let query = e.js8_send_command("W1AW".into(), snr, String::new());
            assert_eq!(query, refused, "{grid:?}: a directed query");
            assert_eq!(e.js8_call_cq(), refused, "{grid:?}: a CQ");
            let st = e.js8_state();
            assert!(
                st.queue.is_empty(),
                "{grid:?}: nothing queued: {:?}",
                st.queue
            );
            assert_eq!(
                st.last_error.as_deref(),
                Some(NO_LOCATOR),
                "{grid:?}: the cockpit says why"
            );
        }
        let mut e = hb_engine("EN52", 5, 1500.0);
        e.js8_send(None, "TEST".into())
            .expect("control: with a locator the same send queues");
        let mut blank = Engine::new("", "", 0);
        blank.js8_enter();
        assert_eq!(
            blank.js8_send(None, "TEST".into()),
            Err("Set your callsign in Settings before transmitting JS8.".to_string()),
            "with neither, the callsign is asked for first, as in ensureCallsignSet"
        );
    }

    /// The refusal drops everything waiting to go, as JS8Call's does (`resetMessageTransmitQueue`,
    /// mainwindow.cpp:5383-5391: its frame and message queues), so a locator set afterwards sends
    /// nothing old. The CQ repeat's schedule is its own: the call it queued is dropped with the
    /// rest, and the repeat stays armed for its next time.
    #[test]
    fn a_start_refused_for_the_locator_drops_the_queue_and_nothing_old_goes_out_later() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.settings.js8_cq_interval_min = 1;
        e.js8_apply_station_config();
        e.js8_send(None, "FIRST".into()).expect("queues");
        e.js8_send(None, "SECOND".into()).expect("queues");
        e.js8_set_cq_repeat(true).expect("CQ repeat on");
        e.settings.mygrid = "EN5".into(); // a slip in Settings
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let refused = run_js8_loop_from(&mut e, t0, 75); // the CQ repeat queues its call at 60 s
        assert!(refused.is_empty(), "nothing keys: {refused:?}");
        let st = e.js8_state();
        assert!(
            st.queue.is_empty(),
            "the queue was dropped at the refusal: {:?}",
            st.queue
        );
        assert!(
            st.cq_on && st.cq_next_at_ms.is_some_and(|next| next > t0 + 75_000),
            "the CQ repeat keeps its schedule: {:?}",
            st.cq_next_at_ms
        );
        assert_eq!(
            st.last_error.as_deref(),
            Some(NO_LOCATOR),
            "…and the cockpit says why"
        );
        e.settings.mygrid = "EN52".into();
        let later = run_js8_loop_from(&mut e, t0 + 75_000, 20);
        assert!(
            later.is_empty(),
            "setting the locator sends nothing old: {later:?}"
        );
    }

    /// JS8Call never holds a malformed locator: its Settings dialog refuses to save one
    /// (Configuration.cpp:2443, the `Maidenhead::ExtendedValidator` set at :1332), so it never
    /// transmits one. Nexus's Settings field can hold one, and the JS8 gate refuses it with the
    /// same words as no locator. JS8Call's rule accepts 4 to 12 characters in whole pairs, in
    /// either case.
    #[test]
    fn js8_refuses_a_malformed_locator_as_it_refuses_none() {
        for grid in [
            "EN5",
            "EN52H",
            "ZZ99",
            "EN52HW1",
            "EN52 HW",
            "BP51AD95RF00A",
        ] {
            let mut e = hb_engine(grid, 5, 1500.0);
            assert_eq!(
                e.js8_send(None, "TEST".into()),
                Err(NO_LOCATOR.to_string()),
                "{grid:?} is refused"
            );
        }
        for grid in ["en52", " EN52HW ", "EN52HW12", "BP51AD95RF", "BP51AD95RF00"] {
            let mut e = hb_engine(grid, 5, 1500.0);
            if let Err(err) = e.js8_send(None, "TEST".into()) {
                panic!("control: {grid:?} is a locator JS8Call accepts, got {err}");
            }
        }
    }

    /// "Send my grid" asks this gate, never a copy of its rule: `js8_locator_refusal` is the
    /// gate's own reason exactly when a send is refused for the locator, and None exactly when
    /// the same send goes through.
    #[test]
    fn the_locator_refusal_is_the_js8_gates_own_answer() {
        for grid in [
            "", "  ", "EN5", "EN52H", "ZZ99", "EN52 HW", "en52", " EN52HW ", "EN52HW12",
        ] {
            let mut e = hb_engine(grid, 5, 1500.0);
            let refusal = e.js8_locator_refusal().map(str::to_string);
            assert_eq!(
                refusal,
                e.js8_send(None, "TEST".into()).err(),
                "{grid:?}: the button's answer is the gate's"
            );
        }
    }

    /// …and it starts nothing on the air either.
    #[test]
    fn a_malformed_locator_keys_nothing() {
        let mut e = hb_engine("EN5", 0, 1500.0);
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        let overs = run_js8_loop(&mut e, 40);
        assert!(
            overs.is_empty(),
            "nothing keys with the locator EN5: {overs:?}"
        );
    }

    /// Automatic traffic meets the same refusal when its transmission would start (`startTx` →
    /// `ensureCreateMessageReady` → `ensureCallsignSet`, :4768 → :5309), and the
    /// `on_stopTxButton_clicked` that follows (:5310) drops it: the reply is gone and the
    /// heartbeat re-based to the next cycle + the interval (:7397 → :3720). So setting the
    /// locator afterwards sends nothing stale.
    #[test]
    fn with_no_locator_js8_keys_nothing_and_drops_what_falls_due() {
        let mut e = hb_engine("", 5, 1500.0);
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        let due = e.js8_state().hb_next_at_ms.expect("scheduled");
        e.js8_ingest(&[snr_query_to_me("W1AW")], now_unix_secs() / 15);
        assert!(
            queued(&e).iter().any(|q| q.contains("W1AW SNR")),
            "control: autoreply is on, and the reply is queued: {:?}",
            queued(&e)
        );
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let overs = run_js8_loop_from(&mut e, t0, 6 * 60); // through the reply and the heartbeat
        assert!(overs.is_empty(), "nothing keys with no locator: {overs:?}");
        let st = e.js8_state();
        assert!(
            st.queue.is_empty() && st.pending_reply.is_none(),
            "the reply was dropped at the refusal"
        );
        assert_eq!(
            st.hb_next_at_ms,
            Some(due + 315_000),
            "the heartbeat was dropped and re-based to the next cycle + the interval"
        );
        assert_eq!(
            st.last_error.as_deref(),
            Some(NO_LOCATOR),
            "…and the cockpit says why"
        );
        e.settings.mygrid = "EN52".into();
        e.js8_apply_station_config();
        let after = run_js8_loop_from(&mut e, t0 + 6 * 60 * 1000, 2 * 60);
        assert!(
            after.is_empty(),
            "setting the locator sends nothing stale: {after:?}"
        );
    }

    /// With TX off, JS8Call's `ensureCanTransmit` refuses first (:5304) and the locator's alert
    /// never comes: the heartbeat is dropped as Q6's is, and nothing is said.
    #[test]
    fn with_tx_off_a_heartbeat_dropped_with_no_locator_says_nothing() {
        let mut e = hb_engine("", 5, 1500.0);
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        let due = e.js8_state().hb_next_at_ms.expect("scheduled");
        e.set_tx_enabled(false);
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        run_js8_loop_from(&mut e, t0, 6 * 60);
        let st = e.js8_state();
        assert_eq!(
            st.hb_next_at_ms,
            Some(due + 315_000),
            "control: the heartbeat was dropped and re-based"
        );
        assert_eq!(st.last_error, None, "TX off refuses first, and silently");
    }

    /// Only a transmission's START asks. JS8Call sends the frames after a message's first
    /// through `stopTx` → `prepareNextMessageFrame` (:4825), which never asks again: a message
    /// already on the air finishes, and the next one does not start. Its refusal drops it, as
    /// JS8Call's `on_stopTxButton_clicked` → `resetMessage` clears the queue (:7396, :5240-5243).
    #[test]
    fn a_js8_message_on_the_air_finishes_when_the_locator_goes_and_the_next_does_not_start() {
        let mut e = hb_engine("EN52", 5, 1500.0);
        e.js8_send(None, "TEST MESSAGE WITH MULTIPLE FRAMES".into())
            .expect("queues");
        e.js8_send(None, "SECOND".into()).expect("queues");
        let queue = e.js8_state().queue;
        let long = queue
            .iter()
            .filter(|q| q.display.ends_with("FRAMES"))
            .count();
        assert!(
            long > 1,
            "control: the first message is multi-frame: {queue:?}"
        );
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let first = run_js8_loop_from(&mut e, t0, 15); // exactly one period boundary
        assert_eq!(first.len(), 1, "control: the first frame keyed");
        e.settings.mygrid = String::new(); // the locator goes while the message is on the air
        let rest = run_js8_loop_from(&mut e, t0 + 15_000, 5 * 60);
        assert!(
            rest.len() >= long - 1,
            "the message on the air finishes: {} of {} more frames",
            rest.len(),
            long - 1
        );
        assert_eq!(
            rest.len(),
            long - 1,
            "…and the next one never starts: {rest:?}"
        );
        let st = e.js8_state();
        assert!(
            st.queue.is_empty(),
            "the second message is dropped when its start is refused: {:?}",
            st.queue
        );
        assert_eq!(
            st.last_error.as_deref(),
            Some(NO_LOCATOR),
            "…and the cockpit says why"
        );
    }

    // ===== outside the licence's privileges: dropped, never held =====

    /// What the JS8 cockpit says when a refusal outside privileges drops what was waiting to go
    /// out: a reply or heartbeat falling due, or the queue.
    const OUTSIDE_PRIVILEGES: &str = "JS8 not sent: this frequency is outside your license \
         privileges, so what was waiting to go out was dropped, not held for later.";

    /// …and when transmit going off drops a message the operator queued.
    const QUEUE_TX_OFF: &str = "JS8 stopped: transmit was turned off, so what was still queued \
         was dropped, not held for later. Send it again when you are ready.";

    /// A reply or heartbeat that falls due while the frequency is outside the licence's
    /// privileges is dropped, as one that falls due with TX off or with no locator is, so a tune
    /// back inside them sends nothing stale: the rule CW, RTTY, PSK and the voice keyer keep.
    #[test]
    fn outside_privileges_js8_keys_nothing_and_drops_what_falls_due() {
        let mut e = hb_engine("EN52", 5, 1500.0);
        e.set_license_class("general");
        assert!(
            e.tx_allowed(),
            "precondition: 14.078 is inside a General's data"
        );
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        let due = e.js8_state().hb_next_at_ms.expect("scheduled");
        e.js8_ingest(&[snr_query_to_me("W1AW")], now_unix_secs() / 15);
        assert!(
            queued(&e).iter().any(|q| q.contains("W1AW SNR")),
            "control: autoreply is on, and the reply is queued: {:?}",
            queued(&e)
        );
        e.set_frequency(14.020, "20m", "USB");
        assert!(
            !e.tx_allowed(),
            "precondition: 14.020 is Extra-only, outside a General's privileges"
        );
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let overs = run_js8_loop_from(&mut e, t0, 6 * 60); // through the reply and the heartbeat
        assert!(
            overs.is_empty(),
            "nothing keys outside privileges: {overs:?}"
        );
        let st = e.js8_state();
        assert!(
            st.queue.is_empty() && st.pending_reply.is_none(),
            "the reply was dropped at the refusal"
        );
        assert_eq!(
            st.hb_next_at_ms,
            Some(due + 315_000),
            "the heartbeat was dropped and re-based to the next cycle + the interval"
        );
        assert_eq!(
            st.last_error.as_deref(),
            Some(OUTSIDE_PRIVILEGES),
            "…and the cockpit says why"
        );
        e.set_frequency(14.078, "20m", "USB");
        let after = run_js8_loop_from(&mut e, t0 + 6 * 60 * 1000, 2 * 60);
        assert!(
            after.is_empty(),
            "a tune back inside them sends nothing stale: {after:?}"
        );
    }

    /// JS8Call 3.0.3's 60 m dial (5.363 MHz, `bandplan::js8_band_plan`) for a US General: a
    /// message at a TX offset that keeps the signal inside the 5351.5–5366.5 kHz segment keys
    /// there, every frame at that offset; at an offset that puts it above 5366.5 kHz the
    /// transmitter is locked and nothing keys.
    #[test]
    fn js8_on_its_60_m_dial_keys_inside_the_segment_and_is_locked_outside_it() {
        let dial = crate::bandplan::js8_band_plan()
            .into_iter()
            .find(|c| c.band == "60m")
            .expect("JS8's plan has a 60 m row")
            .dial_mhz;
        assert_eq!(dial, 5.363, "JS8Call 3.0.3's 60 m dial");
        let mut e = hb_engine("EN52", 5, 1500.0);
        e.set_license_class("general");
        e.set_frequency(dial, "60m", "USB");
        // The band change turned TX off (`halt_tx_for_context_change`); the operator turns it on.
        e.set_tx_enabled(true);
        // 1500 Hz: the signal starts at 5364.5 kHz, inside the segment.
        assert!(e.tx_allowed(), "5.363 MHz + 1500 Hz is a General's data");
        e.js8_send(None, "HELLO SIXTY".into()).expect("queues");
        let frames = e.js8_state().queue.len();
        assert!(frames > 0, "premise: the message waits to go");
        let overs = run_js8_loop(&mut e, (frames as u64 + 2) * 15);
        assert_eq!(overs.len(), frames, "every frame keys: {overs:?}");
        for (i, &(at, f0)) in overs.iter().enumerate() {
            assert_eq!(f0, 1500.0, "on the operator's offset: {overs:?}");
            assert_eq!(at, overs[0].0 + i as u64 * 15_000, "one frame a period");
        }
        assert_eq!(e.settings.dial_mhz, dial, "keyed on the 60 m dial");
        // 3600 Hz: the signal starts at 5366.6 kHz, above the segment.
        e.set_tx_offset(3600.0);
        assert_eq!(
            e.tx_offset_hz(),
            3600.0,
            "premise: nothing is queued, so the offset moves"
        );
        assert!(
            !e.tx_allowed(),
            "5.363 MHz + 3600 Hz is outside a General's privileges"
        );
        let _ = e.js8_send(None, "HELLO SIXTY".into());
        let overs = run_js8_loop(&mut e, 4 * 15);
        assert!(
            overs.is_empty(),
            "nothing keys above the segment: {overs:?}"
        );
    }

    // ===== the ACK for a MSG to me, as JS8Call sends it (mainwindow.cpp:9139) =====

    /// A MSG from `from` to my call, as it decodes: its directed frame and data frames, a row
    /// each, at 1750 Hz.
    fn msg_to_me(from: &str, text: &str) -> Vec<modes::Decode> {
        ::js8::proto::compose::frames(
            from,
            Some(&CallRef::Base("KD9TAW".to_string())),
            &format!("MSG {text}"),
            Js8Speed::Normal,
        )
        .expect("composes")
        .iter()
        .map(|(f, i3)| row(f, *i3, Js8Speed::Normal, 1750.0))
        .collect()
    }

    /// The frame JS8Call keys for `W1AW ACK` from KD9TAW. That the station's ACK is JS8Call's
    /// own frame bit for bit is `js8`'s golden `every_logged_msg_ack_is_the_frame_the_station_keys`.
    fn w1aw_ack() -> Word87 {
        let ack = Frame::Directed {
            from: CallRef::Base("KD9TAW".to_string()),
            to: CallRef::Base("W1AW".to_string()),
            cmd: Command::Ack,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        encode_frame(&ack, whole(), Js8Speed::Normal).expect("packs")
    }

    /// ⭐ THE ACK ON THE AIR. A MSG to me with TX on and autoreply at its default (on) is filed
    /// and answered `W1AW ACK` in the next period, as JS8Call answers it: one frame, on my own
    /// offset, booked where it keyed, and nothing after it. (With the confirmation off, so it goes
    /// by itself; asking first is `a_js8_reply_asks_first_and_keys_once_after_the_yes`.)
    #[test]
    fn a_js8_msg_to_me_is_acked_on_the_air_in_the_next_period() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        for d in msg_to_me("W1AW", "HELLO FROM OHIO") {
            e.js8_ingest(&[d], 0);
        }
        let st = e.js8_state();
        assert_eq!(st.inbox.len(), 1, "control: the MSG is filed");
        assert!(st.pending_reply.is_none(), "nothing counts down");
        let q = st.queue.first().cloned().expect("the ACK is queued");
        assert_eq!(
            (q.origin, q.display.as_str()),
            (Origin::AutoReply, "KD9TAW: W1AW ACK")
        );
        assert!(st.armed.autoreply, "…armed: TX on, autoreply on");
        // The next period's boundary.
        let slot = tempo_core::timing::now_unix_ms() as u64 / 15_000 + 1;
        e.js8_tick(slot * 15_000 - 1_000);
        let plan = e.plan_tx(slot).expect("the ACK keys in the next period");
        let TxWaveform::Js8 { word, f0, .. } = &plan.waveform else {
            panic!("a JS8 plan carries the typed waveform");
        };
        assert_eq!(*word, w1aw_ack(), "JS8Call's `W1AW ACK`, one frame");
        assert_eq!(*f0, 1500.0, "on my own offset: a reply never moves");
        let wave = plan.waveform.build();
        assert!(!e.commit_tx(&plan, wave, slot).is_empty(), "…and it keys");
        let row = e.js8_state().activity.last().cloned().expect("its row");
        assert_eq!(
            (row.mine, row.text.as_str(), row.freq_hz),
            (true, "KD9TAW: W1AW ACK", 1500.0),
            "booked where it keyed"
        );
        let after = run_js8_loop_from(&mut e, (slot + 1) * 15_000, 60);
        assert!(after.is_empty(), "one ACK, and nothing after it: {after:?}");
    }

    /// ⭐ THE ACK AT JS8 60. The same MSG with the transmit speed at JS8 60 (index 4) is
    /// answered in the next 4 s period: `W1AW ACK`, one frame, on my own offset, at JS8 60 —
    /// the keyed wave is exactly that word modulated at 31.25 Hz tones after a 100 ms start
    /// delay, 2.628 s in all — and nothing after it.
    #[test]
    fn a_js8_msg_to_me_is_acked_at_js8_60_in_the_next_4_s_period() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.js8_set_speed(4).expect("JS8 60 is index 4");
        let speed = e.js8_tx_speed();
        assert_eq!(
            (speed.letter(), speed.period_s()),
            ('I', 4),
            "control: JS8 60"
        );
        for d in msg_to_me("W1AW", "HELLO FROM OHIO") {
            e.js8_ingest(&[d], 0);
        }
        let slot = tempo_core::timing::now_unix_ms() as u64 / 4_000 + 1;
        e.js8_tick(slot * 4_000 - 1_000);
        let plan = e
            .plan_tx(slot)
            .expect("the ACK keys in the next JS8 60 period");
        let TxWaveform::Js8 {
            word,
            f0,
            speed: keyed,
        } = &plan.waveform
        else {
            panic!("a JS8 plan carries the typed waveform");
        };
        assert_eq!(*word, w1aw_ack(), "JS8Call's `W1AW ACK`, one frame");
        assert_eq!(*f0, 1500.0, "on my own offset: a reply never moves");
        assert_eq!(*keyed, speed, "at the transmit speed, JS8 60");
        let wave = plan.waveform.build();
        assert_eq!(
            wave.len(),
            1_200 + 79 * 384,
            "100 ms of start delay, then 79 symbols of 384 samples"
        );
        assert!(
            wave[..1_200].iter().all(|&x| x == 0.0),
            "the delay is silence"
        );
        assert_eq!(
            wave,
            ::js8::phy::modulate(
                &::js8::phy::encode_word(&w1aw_ack(), speed),
                speed,
                1500.0,
                tempo_core::tempo_fast::SAMPLE_RATE
            ),
            "the word's own JS8 60 tones"
        );
        assert!(!e.commit_tx(&plan, wave, slot).is_empty(), "…and it keys");
        let after = run_js8_loop_from(&mut e, (slot + 1) * 4_000, 60);
        assert!(after.is_empty(), "one ACK, and nothing after it: {after:?}");
    }

    /// With TX off the ACK counts down in view and is dropped when it falls due, as every
    /// automatic reply is, so turning TX on later sends nothing old.
    #[test]
    fn a_js8_msg_ack_due_while_tx_is_off_is_dropped_and_never_sent_later() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.set_tx_enabled(false);
        for d in msg_to_me("W1AW", "HELLO") {
            e.js8_ingest(&[d], 0);
        }
        assert_eq!(
            queued(&e),
            ["KD9TAW: W1AW ACK"],
            "control: the ACK is queued, in view"
        );
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let off = run_js8_loop_from(&mut e, t0, 60);
        assert!(off.is_empty(), "nothing keys with TX off: {off:?}");
        assert!(
            queued(&e).is_empty(),
            "the ACK was dropped at the refusal, not carried"
        );
        e.set_tx_enabled(true);
        let on = run_js8_loop_from(&mut e, t0 + 60_000, 60);
        assert!(on.is_empty(), "turning TX on sends nothing old: {on:?}");
    }

    /// Outside the licence's privileges the ACK is dropped when it falls due, and says so, and a
    /// tune back inside them sends nothing old.
    #[test]
    fn a_js8_msg_ack_due_outside_privileges_is_dropped_and_never_sent_later() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.set_license_class("general");
        for d in msg_to_me("W1AW", "HELLO") {
            e.js8_ingest(&[d], 0);
        }
        assert_eq!(
            queued(&e),
            ["KD9TAW: W1AW ACK"],
            "control: the ACK is queued"
        );
        e.set_frequency(14.020, "20m", "USB");
        assert!(
            !e.tx_allowed(),
            "precondition: 14.020 is outside a General's privileges"
        );
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let off = run_js8_loop_from(&mut e, t0, 60);
        assert!(off.is_empty(), "nothing keys outside privileges: {off:?}");
        let st = e.js8_state();
        assert!(st.queue.is_empty(), "the ACK was dropped at the refusal");
        assert_eq!(
            st.last_error.as_deref(),
            Some(OUTSIDE_PRIVILEGES),
            "…and the cockpit says why"
        );
        e.set_frequency(14.078, "20m", "USB");
        let on = run_js8_loop_from(&mut e, t0 + 60_000, 60);
        assert!(
            on.is_empty(),
            "a tune back inside them sends nothing old: {on:?}"
        );
    }

    /// The multi-speed pass and then the boundary pass decode each frame; the row dedupe drops
    /// the copy, so the MSG is filed once and draws one ACK. The MSG spans several data frames:
    /// a two-frame one would pass without the dedupe, its doubled last frame finding its buffer
    /// already closed.
    #[test]
    fn a_js8_msg_decoded_by_both_passes_is_filed_once_and_acked_once() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        let rows = msg_to_me("W1AW", "HELLO FROM OHIO, THE BAND IS OPEN TO EUROPE");
        assert!(rows.len() > 3, "control: several data frames");
        for d in rows {
            for _pass in 0..2 {
                let kept = e.js8_dedupe(vec![d.clone()]);
                e.js8_ingest(&kept, 0);
            }
        }
        assert_eq!(e.js8_state().inbox.len(), 1, "filed once");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let overs = run_js8_loop_from(&mut e, t0, 60);
        assert_eq!(overs.len(), 1, "one ACK: {overs:?}");
    }

    /// Stop TX cancels a waiting ACK with everything else waiting (the JS8 halt is total), so
    /// turning TX back on sends nothing.
    #[test]
    fn stop_tx_cancels_a_waiting_js8_msg_ack() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        for d in msg_to_me("W1AW", "HELLO") {
            e.js8_ingest(&[d], 0);
        }
        assert_eq!(
            queued(&e),
            ["KD9TAW: W1AW ACK"],
            "control: the ACK is queued"
        );
        e.halt_tx();
        assert!(queued(&e).is_empty(), "Stop TX drops it");
        e.set_tx_enabled(true);
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let overs = run_js8_loop_from(&mut e, t0, 60);
        assert!(overs.is_empty(), "nothing old keys: {overs:?}");
    }

    /// With autoreply off, JS8Call's AUTO unchecked, the MSG is filed and nothing keys: its ACK
    /// is put in the composer, as JS8Call types it into its compose box.
    #[test]
    fn with_autoreply_off_a_js8_msg_is_filed_and_nothing_keys() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.js8_arm(Js8Switch::Autoreply, false)
            .expect("autoreply off");
        for d in msg_to_me("W1AW", "HELLO") {
            e.js8_ingest(&[d], 0);
        }
        let st = e.js8_state();
        assert_eq!(st.inbox.len(), 1, "control: the MSG is filed");
        assert!(
            st.pending_reply.is_none() && st.queue.is_empty(),
            "no ACK waits to key"
        );
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let overs = run_js8_loop_from(&mut e, t0, 60);
        assert!(overs.is_empty(), "nothing keys: {overs:?}");
        let offered = e.js8_composer(false, None).map(|p| p.text);
        assert_eq!(
            offered.as_deref(),
            Some("W1AW ACK"),
            "the ACK is in the composer"
        );
    }

    /// A settings change during an idle trip re-applies the switches to the station
    /// (`js8_apply_station_config`), and a query heard then must not be answered: JS8Call
    /// processes no automatic reply while its idle watchdog stands (mainwindow.cpp:8818). Nor may
    /// it go out later, once a send of the operator's clears the trip: only that send keys.
    #[test]
    fn a_settings_change_during_an_idle_trip_does_not_re_arm_a_reply() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.settings.js8_idle_watchdog_min = 5;
        e.js8_apply_station_config();
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        e.js8_station.mark_active(t0 - 6 * 60_000); // the operator's last act, six minutes ago
        e.js8_tick(t0);
        assert!(e.js8_state().idle_tripped, "control: the watchdog tripped");
        e.settings.js8_info = "RIG IC7300".into(); // any settings change
        e.js8_apply_station_config();
        assert!(
            e.js8_station.config().autoreply,
            "precondition: the change put autoreply back on in the station"
        );
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        assert!(
            e.js8_state().pending_reply.is_none(),
            "tripped: no reply counts down"
        );
        e.js8_send(None, "TEST".into())
            .expect("the operator's send clears the trip");
        assert!(!e.js8_state().idle_tripped, "control: the trip is cleared");
        let overs = run_js8_loop_from(&mut e, t0, 60);
        let booked: Vec<String> = e
            .js8_state()
            .activity
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.text.clone())
            .collect();
        assert_eq!(
            booked,
            ["KD9TAW: TEST"],
            "only the operator's message keys, nothing heard while tripped: {overs:?}"
        );
    }

    /// A JS8 engine whose 5-minute idle watchdog has just tripped at `t0` (the wall clock's now):
    /// the operator's last act six minutes ago, W1AW's MSG in the inbox.
    fn tripped_engine() -> (Engine, u64) {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.settings.js8_idle_watchdog_min = 5;
        e.js8_apply_station_config();
        let msg = ::js8::proto::compose::frames(
            "W1AW",
            Some(&CallRef::Base("KD9TAW".to_string())),
            "MSG HELLO",
            Js8Speed::Normal,
        )
        .expect("composes");
        for (f, i3) in msg {
            e.js8_ingest(&[row(&f, i3, Js8Speed::Normal, 1750.0)], 0);
        }
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        e.js8_station.mark_active(t0 - 6 * 60_000); // the operator's last act, six minutes ago
        e.js8_tick(t0);
        let st = e.js8_state();
        assert!(st.idle_tripped, "control: the watchdog tripped");
        assert_eq!(st.inbox.len(), 1, "control: W1AW's MSG is in the inbox");
        (e, t0)
    }

    /// Every operator act in the JS8 cockpit restarts the idle count and ends an idle trip,
    /// whatever the act then does, as every key or mouse press in JS8Call's window does
    /// (`eventFilter` → `resetIdleTimer()`, `tx_watchdog(false)`, mainwindow.cpp:2979-2988): each
    /// switch on or off, the repeating CQ on or off, Drop queue, a Yes or a No (to a question the
    /// trip took away), the inbox's Read and Delete, a speed, a send. A second later the trip has
    /// not come back and the count reads 0, and the station carries AUTOREPLY and RELAY as
    /// Settings has them, which is what the dock shows (`js8_state`'s `armed`). A switch and the
    /// repeating CQ cleared the trip but not the count, so the next tick tripped it again.
    #[test]
    fn every_js8_operator_act_restarts_the_idle_count_and_the_trip_stays_ended() {
        type Act = fn(&mut Engine);
        let acts: [(&str, Act); 20] = [
            ("AUTOREPLY on", |e| {
                e.js8_arm(Js8Switch::Autoreply, true).unwrap()
            }),
            ("AUTOREPLY off", |e| {
                e.js8_arm(Js8Switch::Autoreply, false).unwrap()
            }),
            ("RELAY on", |e| e.js8_arm(Js8Switch::Relay, true).unwrap()),
            ("RELAY off", |e| e.js8_arm(Js8Switch::Relay, false).unwrap()),
            ("HB ACK on", |e| e.js8_arm(Js8Switch::HbAck, true).unwrap()),
            ("HB ACK off", |e| {
                e.js8_arm(Js8Switch::HbAck, false).unwrap()
            }),
            ("HB on", |e| e.js8_arm(Js8Switch::Hb, true).unwrap()),
            ("HB off", |e| e.js8_arm(Js8Switch::Hb, false).unwrap()),
            ("CQ repeat on", |e| e.js8_set_cq_repeat(true).unwrap()),
            ("CQ repeat off", |e| e.js8_set_cq_repeat(false).unwrap()),
            ("Drop queue", |e| e.js8_drop_queue()),
            ("Yes", |e| {
                let _ = e.js8_answer_reply(true, "W1AW SNR -07".into(), 0);
            }),
            ("No", |e| {
                let _ = e.js8_answer_reply(false, "W1AW SNR -07".into(), 0);
            }),
            ("Read", |e| {
                let id = e.js8_state().inbox[0].id;
                e.js8_inbox_mark(id, InboxState::Read).unwrap()
            }),
            ("Delete", |e| {
                let id = e.js8_state().inbox[0].id;
                e.js8_inbox_delete(id).unwrap()
            }),
            ("speed", |e| e.js8_set_speed(2).unwrap()),
            ("RX speeds", |e| e.js8_set_rx_speeds(0x03).unwrap()),
            ("send", |e| e.js8_send(None, "TEST".into()).unwrap()),
            ("command", |e| {
                e.js8_send_command("W1AW".into(), Command::SnrQuery.id(), String::new())
                    .unwrap()
            }),
            ("CQ", |e| e.js8_call_cq().unwrap()),
        ];
        for (name, act) in acts {
            let (mut e, t0) = tripped_engine();
            act(&mut e);
            e.js8_tick(t0 + 1_000);
            let st = e.js8_state();
            assert!(
                !st.idle_tripped,
                "{name}: the trip stays ended a second later"
            );
            assert_eq!(st.idle_minutes, 0, "{name}: the idle count restarted");
            let cfg = e.js8_station.config();
            assert_eq!(
                (cfg.autoreply, cfg.relay),
                (e.settings.js8_autoreply, e.settings.js8_relay),
                "{name}: the station carries AUTOREPLY and RELAY as the dock shows them"
            );
        }
    }

    /// …and what that means on the air. After the trip, the operator's send ends it and the dock
    /// shows AUTOREPLY and RELAY armed; an SNR? heard once the send has gone out is then answered
    /// on the air, not put in the message box, and a MSG TO: is held, as those two switches say.
    /// (The station kept both off after the send: the SNR? waited in the message box as with
    /// AUTOREPLY off, and the MSG TO: was not held.)
    #[test]
    fn after_an_idle_trip_a_send_re_arms_autoreply_and_relay_as_the_dock_shows() {
        let (mut e, t0) = tripped_engine();
        e.js8_send(None, "TEST".into())
            .expect("the operator's send ends the trip");
        let st = e.js8_state();
        assert!(
            !st.idle_tripped && st.armed.autoreply && st.armed.relay,
            "control: the dock shows AUTOREPLY and RELAY armed"
        );
        run_js8_loop_from(&mut e, t0, 60); // the send goes out
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let overs = run_js8_loop_from(&mut e, t0 + 61_000, 60);
        let booked: Vec<String> = e
            .js8_state()
            .activity
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.text.clone())
            .collect();
        assert_eq!(
            booked,
            ["KD9TAW: TEST", "KD9TAW: W1AW SNR -07"],
            "the SNR? is answered on the air: {overs:?}"
        );
        assert_eq!(
            e.js8_composer(false, None),
            None,
            "nothing waits in the message box"
        );
        for (f, i3) in msg_to_frames() {
            e.js8_ingest(&[row(&f, i3, Js8Speed::Normal, 1750.0)], 0);
        }
        assert!(
            e.js8_state()
                .inbox
                .iter()
                .any(|m| m.state == InboxState::Store && m.to == "K1ABC"),
            "the MSG TO: for K1ABC is held, as RELAY on says"
        );
    }

    /// Every origin armed, TX on, then the idle watchdog trips: nothing is armed. AUTOREPLY,
    /// RELAY and HB ACK stay on and each says the watchdog is why; the trip turned HB and the
    /// repeating CQ off, so those read off, with no reason. The TX latch is untouched.
    #[test]
    fn after_an_idle_trip_nothing_is_armed_and_each_switch_still_on_says_idle() {
        let mut e = hb_engine("EN52", 30, 1500.0);
        e.settings.js8_idle_watchdog_min = 5;
        e.settings.js8_cq_interval_min = 10;
        e.js8_apply_station_config();
        e.js8_arm(Js8Switch::HbAck, true).expect("HB ACK on");
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        e.js8_set_cq_repeat(true).expect("CQ repeat on");
        let all = Js8Armed {
            autoreply: true,
            relay: true,
            hb_ack: true,
            hb: true,
            cq: true,
        };
        assert_eq!(e.js8_state().armed, all, "control: every origin is armed");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        e.js8_station.mark_active(t0 - 6 * 60_000); // the operator's last act, six minutes ago
        e.js8_tick(t0);
        let st = e.js8_state();
        assert!(
            st.idle_tripped && st.tx_enabled,
            "control: tripped, TX untouched"
        );
        let idle = Some(Js8Unarmed::Idle);
        assert_eq!(
            (st.armed, st.not_armed),
            (
                Js8Armed {
                    autoreply: false,
                    relay: false,
                    hb_ack: false,
                    hb: false,
                    cq: false,
                },
                Js8NotArmed {
                    autoreply: idle,
                    relay: idle,
                    hb_ack: idle,
                    hb: None,
                    cq: None,
                },
            ),
            "nothing armed; what is still on says idle"
        );
        assert!(
            st.autoreply && st.relay && st.hb_ack && !st.hb_on && !st.cq_on,
            "control: AUTOREPLY, RELAY and HB ACK are still on, HB and the CQ repeat off"
        );
        assert_eq!(
            e.js8_station.readiness(true),
            Js8NotArmed {
                autoreply: idle,
                relay: idle,
                hb_ack: idle,
                hb: idle,
                cq: idle,
            },
            "the station: the trip stops every origin"
        );
    }

    /// A switch that is off reads off, with no reason, whatever the station would need for it:
    /// HB ACK off with heartbeat networking off is not "HB ACK needs HB".
    #[test]
    fn a_switch_that_is_off_carries_no_reason() {
        let mut e = hb_engine("EN52", 30, 1500.0);
        e.js8_arm(Js8Switch::HbNet, false)
            .expect("heartbeat networking off");
        let st = e.js8_state();
        assert!(!st.hb_ack && !st.hb_on, "precondition: HB ACK and HB off");
        assert_eq!(
            e.js8_station.readiness(true).hb_ack,
            Some(Js8Unarmed::NeedsHb),
            "control: the station would need HB for an HB ACK"
        );
        assert_eq!(
            (
                st.armed.hb_ack,
                st.not_armed.hb_ack,
                st.armed.hb,
                st.not_armed.hb
            ),
            (false, None, false, None),
            "off reads off"
        );
    }

    /// Nothing that is not the operator's hand restarts the idle count, or the watchdog would
    /// never trip with the cockpit open: the cockpit's polls (`js8_state`, the message box's
    /// `js8_composer`), the locator check, what is heard, the clock, and a Remote's speed write
    /// (a saved choice, as a Settings change is). JS8Call's count also runs on until a key or
    /// mouse press: `resetIdleTimer` has one caller, the `eventFilter` (mainwindow.cpp:2987).
    #[test]
    fn a_poll_what_is_heard_and_a_remote_write_do_not_restart_the_idle_count() {
        let (mut e, t0) = tripped_engine();
        let _ = e.js8_state();
        let _ = e.js8_composer(true, None);
        let _ = e.js8_composer(false, None);
        let _ = e.js8_locator_refusal();
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        e.js8_tick(t0 + 1_000);
        e.js8_set_speed_with_installer(2, |engine, source| engine.install_source(source))
            .expect("the Remote's speed write");
        let st = e.js8_state();
        assert!(st.idle_tripped, "still tripped");
        assert_eq!(st.idle_minutes, 6, "the idle count runs on");
    }

    /// Coming back to JS8 after an idle trip starts the session's count again and ends the trip,
    /// and the station carries AUTOREPLY and RELAY as Settings has them, by whichever path the
    /// tier comes back: the view's `js8_enter`, or `set_tier` alone (the UDP and companion
    /// paths, which never call `js8_enter`).
    #[test]
    fn coming_back_to_js8_after_an_idle_trip_gives_the_station_back_its_switches() {
        type Back = fn(&mut Engine);
        let paths: [(&str, Back); 2] = [
            ("js8_enter", |e| e.js8_enter()),
            ("set_tier", |e| e.set_tier(Tier::Js8)),
        ];
        for (name, back) in paths {
            let (mut e, _) = tripped_engine();
            e.set_tier(Tier::Ft8);
            back(&mut e);
            let st = e.js8_state();
            assert!(!st.idle_tripped, "{name}: the trip ended with the session");
            let cfg = e.js8_station.config();
            assert_eq!(
                (cfg.autoreply, cfg.relay),
                (e.settings.js8_autoreply, e.settings.js8_relay),
                "{name}: the station carries AUTOREPLY and RELAY as the dock shows them"
            );
        }
    }

    /// The five catalogs the cockpit speaks, as the UI ships them.
    const CATALOGS: [(&str, &str); 5] = [
        ("en", include_str!("../../../ui/src/i18n/en.ts")),
        ("de", include_str!("../../../ui/src/i18n/de.ts")),
        ("es", include_str!("../../../ui/src/i18n/es.ts")),
        ("fr", include_str!("../../../ui/src/i18n/fr.ts")),
        ("ja", include_str!("../../../ui/src/i18n/ja.ts")),
    ];

    /// The text a catalog gives `key`: the string literal after `'key':` (en.ts quotes its keys
    /// singly, the translations doubly), each backslash escape taken as the character it escapes.
    fn catalog_value(src: &str, key: &str) -> String {
        let at = [format!("'{key}':"), format!("\"{key}\":")]
            .iter()
            .find_map(|k| src.find(k.as_str()).map(|i| i + k.len()))
            .unwrap_or_else(|| panic!("the catalog has no {key}"));
        let rest = src[at..].trim_start();
        let mut chars = rest.chars();
        let quote = chars.next().expect("a value after the key");
        assert!(
            quote == '\'' || quote == '"',
            "{key}: a string literal, not {quote}"
        );
        let mut out = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.push(chars.next().expect("a character after the backslash")),
                c if c == quote => return out,
                c => out.push(c),
            }
        }
        panic!("{key}: the string literal never closes")
    }

    /// `text`'s clauses, cut at each sentence or clause stop of the five languages.
    fn clauses(text: &str) -> Vec<&str> {
        text.split(['.', ';', '。', '；'])
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect()
    }

    /// The dock switches `clause` names, by their own labels (`CQ` for the repeating CQ), which
    /// no catalog translates.
    fn switches_named(clause: &str) -> std::collections::BTreeSet<&'static str> {
        let words: Vec<&str> = clause
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let mut named = std::collections::BTreeSet::new();
        for (i, w) in words.iter().enumerate() {
            match *w {
                "HB" if words.get(i + 1) == Some(&"ACK") => named.insert("HB ACK"),
                "HB" => named.insert("HB"),
                "AUTOREPLY" => named.insert("AUTOREPLY"),
                "RELAY" => named.insert("RELAY"),
                "CQ" => named.insert("CQ"),
                _ => false,
            };
        }
        named
    }

    /// The idle notice and the dock's idle line say what the next act gives back, and it is what
    /// the station does. With every automatic switch on, the watchdog trips; then one act the
    /// notice names (a send, a switch, a speed, Read or Delete in the Inbox) ends it. Measured on
    /// the station, which is what keys, and on the dock, which must agree, switch by switch and on
    /// whether each would act now: AUTOREPLY, RELAY and HB ACK come back as Settings has them, HB
    /// and the repeating CQ stay off until their own switch, and HB ACK, needing heartbeat
    /// networking rather than the HB repeat, is armed again. In every catalog the notice opens
    /// with the switches the trip turned off, and in both strings the last clause names exactly
    /// the ones still off and the clause before it exactly the ones back. The words said "any
    /// send or switch re-arms them", the heartbeat included, and "off until you send something".
    #[test]
    fn the_idle_notice_and_the_dock_line_say_what_the_next_act_gives_back() {
        type Act = fn(&mut Engine);
        let acts: [(&str, Act); 9] = [
            ("AUTOREPLY on", |e| {
                e.js8_arm(Js8Switch::Autoreply, true).unwrap()
            }),
            ("RELAY on", |e| e.js8_arm(Js8Switch::Relay, true).unwrap()),
            ("HB ACK on", |e| e.js8_arm(Js8Switch::HbAck, true).unwrap()),
            ("Read", |e| {
                let id = e.js8_state().inbox[0].id;
                e.js8_inbox_mark(id, InboxState::Read).unwrap()
            }),
            ("Delete", |e| {
                let id = e.js8_state().inbox[0].id;
                e.js8_inbox_delete(id).unwrap()
            }),
            ("speed", |e| e.js8_set_speed(2).unwrap()),
            ("RX speeds", |e| e.js8_set_rx_speeds(0x03).unwrap()),
            ("send", |e| e.js8_send(None, "TEST".into()).unwrap()),
            ("command", |e| {
                e.js8_send_command("W1AW".into(), Command::SnrQuery.id(), String::new())
                    .unwrap()
            }),
        ];
        // Each switch by the dock's label: on the station and on the dock, then whether the
        // station would act on it now (`Station::readiness`) and whether the dock shows it armed.
        type Switch = (&'static str, bool, bool, bool, bool);
        let switches = |e: &Engine| -> [Switch; 5] {
            let s = &e.js8_station;
            let live = !s.idle_tripped();
            let st = e.js8_state();
            let (c, r, a) = (s.config(), s.readiness(e.tx_enabled()), st.armed);
            let acts = |on: bool, why: Option<Js8Unarmed>| on && why.is_none();
            [
                (
                    "AUTOREPLY",
                    c.autoreply && live,
                    st.autoreply,
                    acts(c.autoreply, r.autoreply),
                    a.autoreply,
                ),
                (
                    "RELAY",
                    c.relay && live,
                    st.relay,
                    acts(c.relay, r.relay),
                    a.relay,
                ),
                (
                    "HB ACK",
                    c.hb_ack && live,
                    st.hb_ack,
                    acts(c.hb_ack, r.hb_ack),
                    a.hb_ack,
                ),
                ("HB", s.hb_on(), st.hb_on, acts(s.hb_on(), r.hb), a.hb),
                ("CQ", s.cq_on(), st.cq_on, acts(s.cq_on(), r.cq), a.cq),
            ]
        };
        let set = |sw: &[Switch; 5], on: bool| {
            sw.iter()
                .filter(|(_, station, ..)| *station == on)
                .map(|(label, ..)| *label)
                .collect::<std::collections::BTreeSet<_>>()
        };
        let mut measured = None;
        for (name, act) in acts {
            let mut e = hb_engine("EN52", 10, 1500.0);
            e.settings.js8_idle_watchdog_min = 5;
            e.settings.js8_hb_ack = true;
            e.settings.js8_cq_interval_min = 10;
            e.js8_apply_station_config();
            let msg = ::js8::proto::compose::frames(
                "W1AW",
                Some(&CallRef::Base("KD9TAW".to_string())),
                "MSG HELLO",
                Js8Speed::Normal,
            )
            .expect("composes");
            for (f, i3) in msg {
                e.js8_ingest(&[row(&f, i3, Js8Speed::Normal, 1750.0)], 0);
            }
            // After the MSG: traffic to me stops a repeating CQ (somebody answered).
            e.js8_arm(Js8Switch::Hb, true).unwrap();
            e.js8_set_cq_repeat(true).unwrap();
            let before = switches(&e);
            assert!(
                before
                    .iter()
                    .all(|&(_, station, dock, acts, armed)| station && dock && acts && armed),
                "control: every switch is on and armed before the trip: {before:?}"
            );
            let t0 = tempo_core::timing::now_unix_ms() as u64;
            e.js8_station.mark_active(t0 - 6 * 60_000);
            e.js8_tick(t0);
            assert!(e.js8_state().idle_tripped, "control: the watchdog tripped");
            let turned_off = set(&switches(&e), false);
            act(&mut e);
            e.js8_tick(t0 + 1_000);
            assert!(
                !e.js8_state().idle_tripped,
                "{name}: control: the act ended the trip"
            );
            let after = switches(&e);
            for (label, station, dock, acts, armed) in after {
                assert_eq!(
                    (station, acts),
                    (dock, armed),
                    "{name}: the dock shows {label} as the station has it, and armed as it acts"
                );
            }
            let st = e.js8_state();
            assert_eq!(
                (st.hb_ack, st.armed.hb_ack, st.not_armed.hb_ack, st.hb_on),
                (true, true, None, false),
                "{name}: HB ACK is back on and armed, heartbeat networking still on, though the \
                 HB repeat is still off: the repeat is not what it needs"
            );
            let now = (turned_off, set(&after, true), set(&after, false));
            match &measured {
                None => measured = Some(now),
                Some(first) => assert_eq!(&now, first, "{name}: the same as every other act"),
            }
        }
        let (turned_off, back, still_off) = measured.expect("measured");
        for (lang, src) in CATALOGS {
            let notice = catalog_value(src, "js8.toast.idleTripped");
            assert_eq!(
                switches_named(clauses(&notice)[0]),
                turned_off,
                "{lang} notice: it opens with the switches the trip turned off: {notice}"
            );
            let line = catalog_value(src, "js8.dock.idle.tripped");
            for (what, text) in [("notice", &notice), ("dock line", &line)] {
                let c = clauses(text);
                assert!(
                    c.len() >= 2,
                    "{lang} {what}: what comes back, then what does not: {text}"
                );
                assert_eq!(
                    switches_named(c[c.len() - 2]),
                    back,
                    "{lang} {what}: the clause before the last names what the act gives back: {text}"
                );
                assert_eq!(
                    switches_named(c[c.len() - 1]),
                    still_off,
                    "{lang} {what}: the last clause names what stays off until its own switch: {text}"
                );
            }
        }
    }

    /// A `MSG TO:` I hold for another station, on the air: stored, then answered `W1AW ACK`
    /// after the countdown, as JS8Call answers it (mainwindow.cpp:9051), one frame on my offset.
    #[test]
    fn a_js8_msg_to_held_for_another_station_is_acked_on_the_air() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        for (f, i3) in msg_to_frames() {
            e.js8_ingest(&[row(&f, i3, Js8Speed::Normal, 1750.0)], 0);
        }
        let st = e.js8_state();
        assert_eq!(
            (st.inbox.len(), st.inbox[0].state),
            (1, InboxState::Store),
            "control: the MSG TO: is held for K1ABC"
        );
        let shown: Vec<String> = st.queue.into_iter().map(|q| q.display).collect();
        assert_eq!(shown, ["KD9TAW: W1AW ACK"], "the ACK is queued");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let overs = run_js8_loop_from(&mut e, t0, 60);
        assert_eq!(overs.len(), 1, "one ACK over: {overs:?}");
        let row = e.js8_state().activity.last().cloned().expect("its row");
        assert_eq!(
            (row.mine, row.text.as_str(), row.freq_hz),
            (true, "KD9TAW: W1AW ACK", 1500.0),
            "booked where it keyed"
        );
    }

    /// A `MSG TO:` for a compound call, on the air: held for its base call, as JS8Call holds it
    /// (`Radio::base_callsign`, mainwindow.cpp:9042), and offered to the compound station when it
    /// asks, its query decoded from the compound frames it sends (`getNextMessageIdForCallsign`,
    /// :9508-9533): VE3/K1ABC's QUERY MSGS is answered `VE3/K1ABC YES MSG ID 1` on the air.
    #[test]
    fn a_js8_msg_to_for_a_compound_call_is_held_for_its_base_call_and_offered_on_the_air() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        let store = ::js8::proto::compose::frames(
            "W1AW",
            Some(&CallRef::Base("KD9TAW".to_string())),
            "MSG TO:VE3/K1ABC FRIDAY CONTACT",
            Js8Speed::Normal,
        )
        .expect("composes");
        for (f, i3) in store {
            e.js8_ingest(&[row(&f, i3, Js8Speed::Normal, 1750.0)], 0);
        }
        let held: Vec<(String, InboxState)> = e
            .js8_state()
            .inbox
            .into_iter()
            .map(|m| (m.to, m.state))
            .collect();
        assert_eq!(
            held,
            [("K1ABC".to_string(), InboxState::Store)],
            "held for K1ABC"
        );
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        run_js8_loop_from(&mut e, t0, 60); // the store's ACK goes out
        let ask = ::js8::proto::compose::frames(
            "VE3/K1ABC",
            Some(&CallRef::Base("KD9TAW".to_string())),
            "QUERY MSGS",
            Js8Speed::Normal,
        )
        .expect("a compound station's query composes");
        assert!(
            ask.len() > 1,
            "control: a compound sender's query is more than one frame"
        );
        for (f, i3) in ask {
            e.js8_ingest(&[row(&f, i3, Js8Speed::Normal, 1210.0)], 0);
        }
        let overs = run_js8_loop_from(&mut e, t0 + 61_000, 60);
        let booked: Vec<String> = e
            .js8_state()
            .activity
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.text.clone())
            .collect();
        // Each frame books its message's row, and a reply to a compound call is several frames:
        // the reply's own composition says how many one message is.
        let reply = "VE3/K1ABC YES MSG ID 1";
        let frames = ::js8::proto::compose::frames_with_grid(
            "KD9TAW",
            "EN52",
            None,
            reply,
            Js8Speed::Normal,
        )
        .expect("the reply composes")
        .len();
        let mut expected = vec!["KD9TAW: W1AW ACK".to_string()];
        expected.extend(std::iter::repeat_n(format!("KD9TAW: {reply}"), frames));
        assert_eq!(
            booked, expected,
            "the message held for K1ABC is offered to VE3/K1ABC on the air, once: {overs:?}"
        );
    }

    /// A `MSG TO:` with nothing after the call, on the air: held and acknowledged, and K1ABC's
    /// QUERY MSGS is answered `K1ABC NO`, as JS8Call answers it: its lookup skips a held message
    /// whose text is empty (`getNextMessageIdForCallsign`, mainwindow.cpp:9516-9529). Nexus
    /// answered `K1ABC YES MSG ID 1`, for a message with nothing to deliver.
    #[test]
    fn a_js8_msg_to_with_no_text_is_held_and_acked_but_not_offered_on_the_air() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        let store = ::js8::proto::compose::frames(
            "W1AW",
            Some(&CallRef::Base("KD9TAW".to_string())),
            "MSG TO:K1ABC",
            Js8Speed::Normal,
        )
        .expect("composes");
        for (f, i3) in store {
            e.js8_ingest(&[row(&f, i3, Js8Speed::Normal, 1750.0)], 0);
        }
        let held: Vec<(String, String, InboxState)> = e
            .js8_state()
            .inbox
            .into_iter()
            .map(|m| (m.to, m.text, m.state))
            .collect();
        assert_eq!(
            held,
            [("K1ABC".to_string(), String::new(), InboxState::Store)],
            "held for K1ABC, with no text"
        );
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        run_js8_loop_from(&mut e, t0, 60); // the store's ACK goes out
        let ask = ::js8::proto::compose::frames(
            "K1ABC",
            Some(&CallRef::Base("KD9TAW".to_string())),
            "QUERY MSGS",
            Js8Speed::Normal,
        )
        .expect("the query composes");
        for (f, i3) in ask {
            e.js8_ingest(&[row(&f, i3, Js8Speed::Normal, 1210.0)], 0);
        }
        let overs = run_js8_loop_from(&mut e, t0 + 61_000, 60);
        let booked: Vec<String> = e
            .js8_state()
            .activity
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.text.clone())
            .collect();
        let reply = "K1ABC NO";
        let frames = ::js8::proto::compose::frames_with_grid(
            "KD9TAW",
            "EN52",
            None,
            reply,
            Js8Speed::Normal,
        )
        .expect("the reply composes")
        .len();
        let mut expected = vec!["KD9TAW: W1AW ACK".to_string()];
        expected.extend(std::iter::repeat_n(format!("KD9TAW: {reply}"), frames));
        assert_eq!(
            booked, expected,
            "acknowledged, then K1ABC is told NO on the air: {overs:?}"
        );
    }

    // ===== the queue at a TX-off or privileges refusal: dropped, never sent later =====

    /// A message queued inside the licence's privileges, then the dial moved out of them before
    /// it started: the queue is dropped at the refusal, and a tune back inside them sends nothing.
    #[test]
    fn a_queued_js8_message_is_dropped_when_the_dial_leaves_privileges_and_never_sent_later() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.set_license_class("general");
        e.js8_send(None, "TEST".into())
            .expect("queues inside privileges");
        e.set_frequency(14.020, "20m", "USB");
        assert!(
            !e.tx_allowed(),
            "precondition: 14.020 is outside a General's privileges"
        );
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let off = run_js8_loop_from(&mut e, t0, 60);
        assert!(off.is_empty(), "nothing keys outside privileges: {off:?}");
        let st = e.js8_state();
        assert!(
            st.queue.is_empty(),
            "the queue was dropped at the refusal: {:?}",
            st.queue
        );
        assert_eq!(
            st.last_error.as_deref(),
            Some(OUTSIDE_PRIVILEGES),
            "…and the cockpit says why"
        );
        e.set_frequency(14.078, "20m", "USB");
        let on = run_js8_loop_from(&mut e, t0 + 60_000, 60);
        assert!(
            on.is_empty(),
            "a tune back inside them sends nothing old: {on:?}"
        );
    }

    /// A message already going out when the dial leaves the privileges: the frame on the air was
    /// keyed inside them, and the frames after it are dropped at the refusal, so none keys after
    /// a tune back inside them.
    #[test]
    fn the_rest_of_a_js8_message_on_the_air_is_dropped_when_the_dial_leaves_privileges() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.set_license_class("general");
        e.js8_send(None, "TEST MESSAGE WITH MULTIPLE FRAMES".into())
            .expect("queues");
        let frames = e.js8_state().queue.len();
        assert!(frames > 1, "control: a multi-frame message: {frames}");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let first = run_js8_loop_from(&mut e, t0, 15); // exactly one period boundary
        assert_eq!(first.len(), 1, "control: its first frame keyed");
        e.set_frequency(14.020, "20m", "USB");
        let off = run_js8_loop_from(&mut e, t0 + 15_000, 5 * 60);
        assert!(
            off.is_empty(),
            "nothing more keys outside privileges: {off:?}"
        );
        let st = e.js8_state();
        assert!(
            st.queue.is_empty(),
            "the rest of the message was dropped at the refusal: {:?}",
            st.queue
        );
        assert_eq!(
            st.last_error.as_deref(),
            Some(OUTSIDE_PRIVILEGES),
            "…and the cockpit says why"
        );
        e.set_frequency(14.078, "20m", "USB");
        let on = run_js8_loop_from(&mut e, t0 + 15_000 + 5 * 60_000, 2 * 60);
        assert!(
            on.is_empty(),
            "none of its frames keys after a tune back inside them: {on:?}"
        );
    }

    /// The CQ repeat's call that comes due outside privileges is dropped with the rest of the
    /// queue, and the repeat keeps its schedule, as it does at the locator's refusal.
    #[test]
    fn a_js8_cq_repeat_call_due_outside_privileges_is_dropped_and_the_repeat_keeps_its_schedule() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.set_license_class("general");
        e.settings.js8_cq_interval_min = 1;
        e.js8_apply_station_config();
        e.js8_set_cq_repeat(true).expect("CQ repeat on");
        e.set_frequency(14.020, "20m", "USB");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let off = run_js8_loop_from(&mut e, t0, 75); // the repeat queues its call at 60 s
        assert!(off.is_empty(), "nothing keys outside privileges: {off:?}");
        let st = e.js8_state();
        assert!(
            st.queue.is_empty(),
            "the call it queued was dropped: {:?}",
            st.queue
        );
        assert!(
            st.cq_on && st.cq_next_at_ms.is_some_and(|next| next > t0 + 75_000),
            "the CQ repeat keeps its schedule: {:?}",
            st.cq_next_at_ms
        );
        assert_eq!(
            st.last_error.as_deref(),
            Some(OUTSIDE_PRIVILEGES),
            "…and the cockpit says why"
        );
        e.set_frequency(14.078, "20m", "USB");
        let on = run_js8_loop_from(&mut e, t0 + 75_000, 20);
        assert!(
            on.is_empty(),
            "a tune back inside them sends nothing old: {on:?}"
        );
    }

    /// TX Off lets the frame on the air finish (its contract, as in the FT8 screen) and drops
    /// what follows it, as JS8Call's TX button does (`on_monitorTxButton_toggled` →
    /// `on_stopTxButton_clicked` → `resetMessage`, mainwindow.cpp:2855-2861, :7390-7398), so
    /// turning TX back on sends nothing old.
    #[test]
    fn turning_js8_tx_off_drops_the_rest_of_the_queue_and_turning_it_on_sends_nothing_old() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.js8_send(None, "TEST MESSAGE WITH MULTIPLE FRAMES".into())
            .expect("queues");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let first = run_js8_loop_from(&mut e, t0, 15); // exactly one period boundary
        assert_eq!(first.len(), 1, "control: its first frame keyed");
        e.set_tx_enabled(false);
        let st = e.js8_state();
        assert!(
            st.queue.is_empty(),
            "TX off drops what was still queued: {:?}",
            st.queue
        );
        assert_eq!(
            st.last_error.as_deref(),
            Some(QUEUE_TX_OFF),
            "…and the cockpit says why"
        );
        e.set_tx_enabled(true);
        let on = run_js8_loop_from(&mut e, t0 + 15_000, 3 * 60);
        assert!(
            on.is_empty(),
            "turning TX back on sends nothing old: {on:?}"
        );
    }

    /// A message sent while TX is off meets the same refusal on the next tick, so turning TX on
    /// afterwards sends nothing old. (JS8Call's Enter does nothing at all with TX off,
    /// mainwindow.cpp:748.)
    #[test]
    fn a_js8_message_sent_while_tx_is_off_is_dropped_and_not_sent_when_tx_comes_on() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.set_tx_enabled(false);
        e.js8_send(None, "TEST".into())
            .expect("a send with TX off is taken");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let off = run_js8_loop_from(&mut e, t0, 5);
        assert!(off.is_empty(), "nothing keys with TX off: {off:?}");
        let st = e.js8_state();
        assert!(
            st.queue.is_empty(),
            "dropped at the refusal: {:?}",
            st.queue
        );
        assert_eq!(
            st.last_error.as_deref(),
            Some(QUEUE_TX_OFF),
            "…and the cockpit says why"
        );
        e.set_tx_enabled(true);
        let on = run_js8_loop_from(&mut e, t0 + 5_000, 60);
        assert!(on.is_empty(), "turning TX on sends nothing old: {on:?}");
    }

    /// With TX off the CQ repeat's call that comes due is dropped as the heartbeat is, and
    /// nothing is said: TX off refuses first, and silently, for automatic traffic. The repeat
    /// stays armed for its next time.
    #[test]
    fn with_tx_off_a_dropped_js8_cq_repeat_call_says_nothing() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.settings.js8_cq_interval_min = 1;
        e.js8_apply_station_config();
        e.js8_set_cq_repeat(true).expect("CQ repeat on");
        e.set_tx_enabled(false);
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let off = run_js8_loop_from(&mut e, t0, 75); // the repeat queues its call at 60 s
        assert!(off.is_empty(), "nothing keys with TX off: {off:?}");
        let st = e.js8_state();
        assert!(
            st.queue.is_empty(),
            "the call it queued was dropped: {:?}",
            st.queue
        );
        assert!(st.cq_on, "the CQ repeat stays armed");
        assert_eq!(
            st.last_error, None,
            "TX off drops an automatic call silently"
        );
    }

    // ===== the free heartbeat spot: JS8Call's band activity =====

    /// A data frame at `freq`: a message's continuation, which carries no callsign.
    fn continuation(freq: f32) -> modes::Decode {
        let data = Frame::Data {
            text: "HELLO WORLD".into(),
            dense: false,
        };
        let i3 = I3 {
            first: false,
            last: false,
            data: true,
        };
        row(&data, i3, Js8Speed::Normal, freq)
    }

    /// The one heartbeat over an on-demand heartbeat keys in the next 20 s, and its offset.
    fn heartbeat_offset(e: &mut Engine) -> f32 {
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        let overs = run_js8_loop(e, 20);
        let [(_, f0)] = overs[..] else {
            panic!("one heartbeat over, got {overs:?}");
        };
        f0
    }

    /// JS8Call's free-spot test reads its band activity (`isFreqOffsetFree`, mainwindow.cpp:
    /// 5579-5587), where every decoded frame is filed (:4013). A station in the middle of a long
    /// message sends frames that carry no callsign, and its offset stays taken.
    #[test]
    fn a_spot_heard_only_in_continuation_frames_is_taken() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.js8_ingest(&[continuation(900.0)], now_unix_secs() / 15);
        e.js8_rng = 2; // the first draw is 900 Hz, the second 500 Hz
        assert_eq!(
            heartbeat_offset(&mut e),
            500.0,
            "900 Hz is taken, so the second draw is used"
        );
    }

    /// …except the operator's own offset, which the test calls free whatever was heard there
    /// (`freq() == f`, :5572): an HB-ACK may go out where the operator already is.
    #[test]
    fn the_operators_own_offset_counts_as_a_free_spot() {
        let mut e = hb_engine("EN52", 30, 700.0); // our own heartbeat is 30 min away
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        e.js8_arm(Js8Switch::HbAck, true).expect("HB-ack on");
        e.js8_ingest(
            &[row(&hb("W1AW", "FN31"), whole(), Js8Speed::Normal, 700.0)],
            now_unix_secs() / 15,
        );
        e.js8_rng = 23; // the first draw is 700 Hz, the second 800 Hz
        let overs = run_js8_loop(&mut e, 60);
        let [(_, f0)] = overs[..] else {
            panic!("one HB-ACK over, got {overs:?}");
        };
        assert_eq!(
            f0, 700.0,
            "W1AW was heard at 700 Hz, but that is the operator's own offset, so it is free"
        );
    }

    /// A drifting signal keeps one entry: a frame at an offset not yet filed takes over the one
    /// filed within the speed's `rxThreshold` (:3970-3981), so the spot the signal left is free.
    #[test]
    fn a_drifting_signal_keeps_one_band_activity_entry() {
        for (drifted, free) in [(false, false), (true, true)] {
            let mut e = hb_engine("EN52", 0, 1500.0);
            let slot = now_unix_secs() / 15;
            e.js8_ingest(&[continuation(745.0)], slot); // 45 Hz from 700
            if drifted {
                e.js8_ingest(&[continuation(752.0)], slot); // 7 Hz on: 52 Hz from 700
            }
            e.js8_rng = 6; // the first two draws are 700 Hz, the third 500 Hz
            let f0 = heartbeat_offset(&mut e);
            assert_eq!(
                f0 == 700.0,
                free,
                "drifted {drifted}: 700 Hz is {} ({f0})",
                if free { "free" } else { "taken" }
            );
        }
    }

    /// …and the entry that moves is the first one `generateOffsets` meets counting up from
    /// offset − range (:3730-3739, then the first hit, :3975-3979), not the nearest: a frame at
    /// 750 Hz takes over the stale 740 and leaves the fresh 755, which still holds 800 Hz.
    #[test]
    fn a_new_frame_takes_over_the_lowest_band_activity_entry_in_range() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        let now = tempo_core::timing::now_unix_ms() as u64;
        e.js8_file_band_activity(740.0, Js8Speed::Normal, now - 60_000);
        e.js8_file_band_activity(755.0, Js8Speed::Normal, now - 5_000);
        e.js8_file_band_activity(750.0, Js8Speed::Normal, now);
        e.js8_rng = 4; // the first draw is 800 Hz, the second 500 Hz
        assert_eq!(
            heartbeat_offset(&mut e),
            500.0,
            "755 Hz, heard 5 s ago, still holds 800 Hz"
        );
    }

    /// A frame is filed at its offset truncated to whole hertz, as `DecodedText` takes the
    /// decoder's float into an int (decodedtext.cpp:248 → decodedtext.h:80): 750.9 Hz is filed
    /// at 750, a full 50 Hz from 800, which stays free.
    #[test]
    fn a_frame_is_filed_at_its_offset_truncated_to_whole_hertz() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.js8_ingest(&[continuation(750.9)], now_unix_secs() / 15);
        e.js8_rng = 4; // the first draw is 800 Hz
        assert_eq!(
            heartbeat_offset(&mut e),
            800.0,
            "filed at 750 Hz, 800 Hz is free"
        );
    }

    // ===== @APRSIS and @JS8NET: destinations JS8Call sends to =====

    /// A message with @APRSIS or @JS8NET in the To field is sent and keys, as in JS8Call, whose
    /// `isGroupAllowed` (varicode.cpp:1314-1320) guards joining a group, never sending to one.
    #[test]
    fn a_js8_message_to_aprsis_or_js8net_is_sent() {
        for to in ["@APRSIS", "@JS8NET"] {
            let mut e = hb_engine("EN52", 0, 1500.0);
            assert_eq!(
                e.js8_send(Some(to.into()), "GRID EN52".into()),
                Ok(()),
                "{to}: the send is accepted"
            );
            let overs = run_js8_loop(&mut e, 2 * 60);
            assert!(!overs.is_empty(), "{to}: and it keys");
        }
    }

    // ===== a custom @group: addressed the way JS8Call addresses it =====

    /// What the engine keys over the next `n` periods, frame by frame as it reads on the air.
    fn keyed_frames(e: &mut Engine, n: u64) -> Vec<(Frame, I3)> {
        let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
        let first = now_unix_secs() * 1000 / period_ms + 1;
        let mut out = Vec::new();
        for slot in first..first + n {
            e.js8_tick(slot * period_ms);
            let Some(plan) = e.plan_tx(slot) else {
                continue;
            };
            let TxWaveform::Js8 { speed, word, .. } = plan.waveform else {
                panic!("a JS8 plan carries the typed waveform");
            };
            out.push(::js8::proto::decode_word(&word, speed).expect("decodes"));
            let wave = plan.waveform.build();
            e.commit_tx(&plan, wave, slot);
        }
        out
    }

    /// "@MAGNET hello" from KD9TAW in EN52 goes out as JS8Call frames it, worked by hand from
    /// varicode.cpp. `isValidCallsign("@MAGNET")` (:1266-1290) finds no `basecalls` entry and no
    /// base-call shape, and `isValidCompoundCallsign` takes an `@` name (:1248-1250), so the
    /// destination is valid and COMPOUND. `packDirectedMessage` (:1542-1600) reads the freetext
    /// command " " after it, and `buildMessageFrames` CASE 2 (:2100-2135) sends "`KD9TAW EN52" as
    /// a compound frame, then "`@MAGNET " as a compound directed frame, then the rest, "HELLO",
    /// as data: three frames, the first flagged first and the last flagged last.
    #[test]
    fn a_js8_message_to_a_custom_group_keys_the_frames_js8call_keys() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        assert_eq!(e.js8_send(Some("@MAGNET".into()), "hello".into()), Ok(()));
        let keyed = keyed_frames(&mut e, 8);
        let frames: Vec<&Frame> = keyed.iter().map(|(f, _)| f).collect();
        assert_eq!(
            frames[..2],
            [
                &Frame::Compound {
                    call: "KD9TAW".into(),
                    grid: Some("EN52".into()),
                },
                &Frame::CompoundDirected {
                    call: "@MAGNET".into(),
                    cmd: Command::Freetext,
                    num: None,
                },
            ]
        );
        assert!(
            matches!(&frames[2], Frame::Data { text, .. } if text == "HELLO"),
            "{frames:?}"
        );
        assert_eq!(frames.len(), 3, "{frames:?}");
        let flags: Vec<(bool, bool)> = keyed.iter().map(|(_, i)| (i.first, i.last)).collect();
        assert_eq!(flags, [(true, false), (false, false), (false, true)]);
        // The same message with the group typed in the text and no call selected: one framing.
        let mut typed = hb_engine("EN52", 0, 1500.0);
        typed
            .js8_send(None, "@MAGNET hello".into())
            .expect("queues");
        assert_eq!(keyed_frames(&mut typed, 8), keyed);
    }

    /// A directed command to a custom group goes out as JS8Call's CASE 2 too: "`@MAGNET SNR?".
    #[test]
    fn a_js8_command_to_a_custom_group_is_a_compound_directed_frame() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.js8_send_command("@magnet".into(), Command::SnrQuery.id(), String::new())
            .expect("queues");
        let frames: Vec<Frame> = keyed_frames(&mut e, 8)
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        assert_eq!(
            frames,
            [
                Frame::Compound {
                    call: "KD9TAW".into(),
                    grid: Some("EN52".into()),
                },
                Frame::CompoundDirected {
                    call: "@MAGNET".into(),
                    cmd: Command::SnrQuery,
                    num: None,
                },
            ]
        );
    }

    /// A built-in group keeps its short code in a standard directed frame.
    #[test]
    fn a_js8_command_to_a_built_in_group_keeps_its_short_code() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.js8_send_command("@QRP".into(), Command::SnrQuery.id(), String::new())
            .expect("queues");
        let frames: Vec<Frame> = keyed_frames(&mut e, 8)
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        assert_eq!(
            frames,
            [Frame::Directed {
                from: CallRef::Base("KD9TAW".into()),
                to: CallRef::parse("@QRP").expect("a built-in group"),
                cmd: Command::SnrQuery,
                num: None,
                portable_from: false,
                portable_to: false,
            }]
        );
    }

    /// A name JS8Call would not take as a group is refused, and the refusal says what a group is:
    /// over 9 characters without the slashes (`isValidCompoundCallsign`, varicode.cpp:1233-1236),
    /// a bare "@", or one ending in "/" (`compound_callsign_pattern` ends in `\b`, :42). Nothing
    /// is queued.
    #[test]
    fn a_js8_send_to_a_group_js8call_would_refuse_is_refused_with_what_is_allowed() {
        for to in ["@MAGNETICS", "@", "@MAG/", "@MAG NET"] {
            let mut e = hb_engine("EN52", 0, 1500.0);
            let want = Err(format!(
                "{to} is not a callsign or @group JS8 can address. A group is @ and up to 8 \
                 letters or digits (a / may separate them)."
            ));
            assert_eq!(e.js8_send(Some(to.into()), "hello".into()), want, "{to}");
            assert_eq!(
                e.js8_send_command(to.into(), Command::SnrQuery.id(), String::new()),
                want,
                "{to}"
            );
            assert!(e.js8_state().queue.is_empty(), "{to}: nothing queued");
        }
        let mut e = hb_engine("EN52", 0, 1500.0);
        assert_eq!(
            e.js8_send(Some("@MAGNETIC".into()), "hello".into()),
            Ok(()),
            "control: nine characters is JS8Call's longest group"
        );
    }

    /// A message to a group in the operator's Groups is shown as addressed to them, as JS8Call's
    /// `isGroupCallIncluded` (mainwindow.cpp:8153-8155) takes any group in `my_groups`.
    #[test]
    fn a_js8_message_to_a_custom_group_i_joined_is_shown_as_mine() {
        let ingest = |e: &mut Engine| {
            let seq = ::js8::proto::compose::frames_with_grid(
                "W1AW",
                "FN31",
                None,
                "@MAGNET HELLO",
                Js8Speed::Normal,
            )
            .expect("composes");
            assert!(matches!(&seq[1].0, Frame::CompoundDirected { call, .. } if call == "@MAGNET"));
            for (i, (f, i3)) in seq.iter().enumerate() {
                e.js8_ingest(&[row(f, *i3, Js8Speed::Normal, 1750.0)], 10 + i as u64);
            }
            let rows = e.js8_state().activity;
            let msg = rows
                .iter()
                .find(|r| r.flags.as_ref().is_some_and(|f| f.assembled))
                .cloned()
                .expect("the reassembled row");
            let directed = rows
                .iter()
                .find(|r| r.from == "@MAGNET")
                .cloned()
                .expect("the compound directed frame's row");
            (msg, directed)
        };
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        e.js8_set_groups(vec!["@MAGNET".into()]).expect("joins");
        let (msg, directed) = ingest(&mut e);
        assert_eq!(
            (msg.from.as_str(), msg.text.as_str(), msg.directed_to_me),
            ("W1AW", "HELLO", true)
        );
        assert!(directed.directed_to_me, "its directed frame too");
        let mut other = Engine::new("KD9TAW", "EN52", 0);
        other.js8_enter();
        let (msg, directed) = ingest(&mut other);
        assert!(
            !msg.directed_to_me && !directed.directed_to_me,
            "control: not mine without the group joined"
        );
    }

    // ===== <MYGRID4> / <MYGRID12> =====

    /// The operator's send reaches the station with JS8Call's grid macros still in it, and goes out
    /// with them replaced by Settings' locator (`buildMacroValues`, mainwindow.cpp:7024-7025).
    #[test]
    fn a_js8_send_goes_out_with_the_grid_macros_replaced() {
        let mut e = hb_engine("EN52hw", 0, 1500.0);
        e.js8_send(None, "QTH <MYGRID4>, <MYGRID12> TO BE EXACT".into())
            .expect("queues");
        let queue = e.js8_state().queue;
        assert_eq!(
            queue.first().map(|q| q.display.as_str()),
            Some("KD9TAW: QTH EN52, EN52HW TO BE EXACT"),
            "the queued message carries the locator"
        );
    }

    // ===== the idle count (JS8Call's <MYIDLE>) =====

    /// The cockpit's idle chip ("Idle 12/60 min") counts the whole minutes since the operator's
    /// last act, the count JS8Call's idle timer keeps (mainwindow.cpp:10969-10979).
    #[test]
    fn the_js8_idle_count_is_the_minutes_since_the_operators_last_act() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        let now = tempo_core::timing::now_unix_ms() as u64;
        e.js8_station.mark_active(now - 12 * 60_000 - 5_000);
        assert_eq!(
            e.js8_state().idle_minutes,
            12,
            "twelve minutes since the last act"
        );
        e.js8_send(None, "TEST".into()).expect("queues");
        assert_eq!(e.js8_state().idle_minutes, 0, "an operator send resets it");
    }

    // ===== groups that cannot be joined =====

    /// JS8Call will not let @APRSIS or @JS8NET be joined (`isGroupAllowed`, varicode.cpp:1314-1320,
    /// asked when a group is added and when Settings is saved, Configuration.cpp:1016, :2450). A
    /// settings file written by a Nexus that accepted one still loads, and the station does not
    /// join it: a query to @APRSIS draws no automatic reply, while one to a real group does.
    #[test]
    fn a_stored_aprsis_or_js8net_group_is_not_joined() {
        let older =
            r#"{"mycall":"KD9TAW","mygrid":"EN52","js8Groups":["@APRSIS","@ares","@JS8NET"]}"#;
        let settings: Settings = serde_json::from_str(older).expect("an older settings file loads");
        assert_eq!(
            settings.js8_groups,
            vec!["@APRSIS", "@ares", "@JS8NET"],
            "the file's list is kept as written"
        );
        let mut e = Engine::with_settings(settings);
        e.js8_enter();
        assert_eq!(
            e.js8_station.config().groups,
            vec!["@ARES".to_string()],
            "only the group that can be joined is joined"
        );
        let query = |to: &str| Frame::Directed {
            from: CallRef::Base("W1AW".to_string()),
            to: CallRef::parse(to).expect("a group"),
            cmd: Command::SnrQuery,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        let slot = now_unix_secs() / 15;
        e.js8_ingest(
            &[row(&query("@APRSIS"), whole(), Js8Speed::Normal, 1200.0)],
            slot,
        );
        assert!(
            e.js8_state().pending_reply.is_none(),
            "a query to @APRSIS draws no automatic reply"
        );
        e.js8_ingest(
            &[row(&query("@ARES"), whole(), Js8Speed::Normal, 1200.0)],
            slot,
        );
        assert!(
            e.js8_state().pending_reply.is_some(),
            "control: a query to a joined group is answered"
        );
    }

    // ===== callsign aging =====

    /// JS8Call's callsign aging reaches the station from Settings (`js8_station_config`, the one
    /// seam), and a settings file from before the setting ages nothing, as JS8Call's default.
    #[test]
    fn the_callsign_aging_setting_reaches_the_station() {
        let mut e = Engine::with_settings(Settings {
            mycall: "KD9TAW".into(),
            mygrid: "EN52".into(),
            js8_callsign_aging_min: 10,
            ..Settings::default()
        });
        e.js8_enter();
        assert_eq!(
            e.js8_station.config().callsign_aging_min,
            10,
            "the setting reaches the station"
        );
        let older: Settings = serde_json::from_str(r#"{"mycall":"KD9TAW","mygrid":"EN52"}"#)
            .expect("an older settings file loads");
        let mut e = Engine::with_settings(older);
        e.js8_enter();
        assert_eq!(
            e.js8_station.config().callsign_aging_min,
            0,
            "an older file ages nothing"
        );
    }

    // ===== (E) JS8Call's AutoreplyConfirmation, on the air =====

    /// `hb_engine` at JS8Call's default: every automatic reply asks Yes/No first.
    fn asking_engine() -> Engine {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.settings.js8_autoreply_confirmation = true;
        e.js8_apply_station_config();
        e
    }

    /// What the cockpit's queue shows, a message per frame.
    fn queued(e: &Engine) -> Vec<String> {
        e.js8_state().queue.into_iter().map(|q| q.display).collect()
    }

    /// The question the cockpit shows: (display, the moment it says No by itself).
    fn question(e: &Engine) -> (String, u64) {
        let p = e.js8_state().pending_reply.expect("a reply asks");
        let now = tempo_core::timing::now_unix_ms() as u64;
        assert!(
            p.fires_at_ms >= now + 88_000,
            "a question, No 89 s after it asked (not a countdown): {p:?}"
        );
        (p.display, p.fires_at_ms)
    }

    /// The setting is the station's: JS8Call's `AutoreplyConfirmation` reaches it both ways.
    #[test]
    fn the_js8_confirmation_setting_reaches_the_station() {
        let mut s = Settings::default();
        assert!(
            Engine::js8_station_config(&s).autoreply_confirmation,
            "on by default"
        );
        s.js8_autoreply_confirmation = false;
        assert!(
            !Engine::js8_station_config(&s).autoreply_confirmation,
            "off"
        );
    }

    /// ⭐ ON THE AIR. A query with TX on asks first: nothing keys while it waits, over periods
    /// on end; the operator's Yes puts it in the queue and it keys once, in the next period.
    #[test]
    fn a_js8_reply_asks_first_and_keys_once_after_the_yes() {
        let mut e = asking_engine();
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let (display, no_at) = question(&e);
        assert_eq!(display, "KD9TAW: W1AW SNR -07");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        assert!(
            no_at >= t0 + 88_000,
            "it says No by itself 89 s after it asked"
        );
        let waiting = run_js8_loop_from(&mut e, t0, 60);
        assert!(
            waiting.is_empty(),
            "nothing keys while it asks: {waiting:?}"
        );
        e.js8_answer_reply(true, display, no_at).expect("Yes");
        assert!(e.js8_state().pending_reply.is_none(), "answered");
        let after = run_js8_loop_from(&mut e, t0 + 61_000, 60);
        assert_eq!(after.len(), 1, "Yes: one reply keys: {after:?}");
        let mine: Vec<String> = e
            .js8_state()
            .activity
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.text.clone())
            .collect();
        assert_eq!(mine, ["KD9TAW: W1AW SNR -07"]);
    }

    /// No, or no answer in 89 s, keys nothing, now or later; a Yes after that is refused and says
    /// so, so it can never send something else.
    #[test]
    fn a_js8_reply_answered_no_or_not_at_all_never_keys() {
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let mut e = asking_engine();
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let (display, no_at) = question(&e);
        e.js8_answer_reply(false, display.clone(), no_at)
            .expect("No");
        assert!(e.js8_state().pending_reply.is_none(), "No: answered");
        assert!(
            run_js8_loop_from(&mut e, t0, 120).is_empty(),
            "No: nothing keys"
        );

        let mut e = asking_engine();
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let (display, no_at) = question(&e);
        let overs = run_js8_loop_from(&mut e, t0, 120);
        assert!(overs.is_empty(), "unanswered: nothing keys: {overs:?}");
        assert!(
            e.js8_state().pending_reply.is_none(),
            "No by itself at 89 s"
        );
        assert_eq!(
            e.js8_answer_reply(true, display, no_at),
            Err(JS8_REPLY_GONE.to_string()),
            "a late Yes is refused"
        );
        assert!(
            run_js8_loop_from(&mut e, t0 + 121_000, 60).is_empty(),
            "…and sends nothing"
        );
    }

    /// TX off: the question shows (JS8Call asks regardless); a Yes puts the reply in the queue,
    /// which TX off drops, and turning TX on later sends nothing old.
    #[test]
    fn a_yes_with_js8_tx_off_sends_nothing_now_or_later() {
        let mut e = asking_engine();
        e.set_tx_enabled(false);
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let (display, no_at) = question(&e);
        e.js8_answer_reply(true, display, no_at).expect("Yes");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        assert!(
            run_js8_loop_from(&mut e, t0, 30).is_empty(),
            "nothing keys with TX off"
        );
        assert!(e.js8_state().queue.is_empty(), "the queue was dropped");
        e.set_tx_enabled(true);
        assert!(
            run_js8_loop_from(&mut e, t0 + 31_000, 60).is_empty(),
            "TX on: nothing old"
        );
    }

    /// Outside the licence's privileges a Yes'd reply is dropped at the refusal and the cockpit
    /// says why (N61's rule); a tune back inside them sends nothing old.
    #[test]
    fn a_yes_outside_privileges_is_dropped_says_so_and_is_never_sent_later() {
        let mut e = asking_engine();
        e.set_license_class("general");
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let (display, no_at) = question(&e);
        e.set_frequency(14.020, "20m", "USB");
        assert!(
            !e.tx_allowed(),
            "precondition: outside a General's privileges"
        );
        e.js8_answer_reply(true, display, no_at).expect("Yes");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        assert!(
            run_js8_loop_from(&mut e, t0, 30).is_empty(),
            "nothing keys outside them"
        );
        assert_eq!(
            e.js8_state().last_error.as_deref(),
            Some(OUTSIDE_PRIVILEGES),
            "…and says why"
        );
        e.set_frequency(14.078, "20m", "USB");
        assert!(
            run_js8_loop_from(&mut e, t0 + 31_000, 60).is_empty(),
            "a tune back: nothing old"
        );
    }

    /// With no locator in Settings a Yes'd reply keys nothing: the start is refused and the queue
    /// dropped, as for every JS8 transmission (JS8Call's `ensureCallsignSet`, :5264-5268).
    #[test]
    fn a_yes_with_no_js8_locator_keys_nothing() {
        let mut e = asking_engine();
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let (display, no_at) = question(&e);
        e.settings.mygrid = String::new();
        e.js8_answer_reply(true, display, no_at).expect("Yes");
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        assert!(run_js8_loop_from(&mut e, t0, 60).is_empty(), "nothing keys");
        assert_eq!(e.js8_state().last_error.as_deref(), Some(JS8_NO_LOCATOR));
    }

    /// Stop TX drops a reply still asking, with everything else waiting: nothing keys after.
    #[test]
    fn stop_tx_drops_a_js8_reply_still_asking() {
        let mut e = asking_engine();
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let (display, no_at) = question(&e);
        e.halt_tx();
        assert!(
            e.js8_state().pending_reply.is_none(),
            "Stop TX drops the question"
        );
        assert_eq!(
            e.js8_answer_reply(true, display, no_at),
            Err(JS8_REPLY_GONE.to_string()),
            "a Yes after Stop TX answers nothing"
        );
        e.set_tx_enabled(true);
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        assert!(run_js8_loop_from(&mut e, t0, 60).is_empty(), "nothing keys");
    }

    // ===== (C) JS8Call's cadence, on the air =====

    /// ⭐ THE CADENCE, MEASURED ON THE AIR PATH. With the confirmation off, a query heard now is
    /// answered at the next period's boundary: the period after the one it was heard in, as
    /// JS8Call schedules it (`processCommandActivity` → `enqueueMessage`, mainwindow.cpp:9388 →
    /// `processTxQueue`, :9627-9690, in the same 1 Hz pass → TX at the next period start,
    /// `guiUpdate`, :4510-4521). Nexus counted down one period + 2 s first and keyed it two
    /// periods later. A Yes is the same: the boundary after it.
    #[test]
    fn a_js8_reply_keys_at_the_next_boundary_as_js8call_keys_it() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let overs = run_js8_loop_from(&mut e, t0, 60);
        let next = (t0 / 15_000 + 1) * 15_000;
        assert_eq!(
            overs.iter().map(|o| o.0).collect::<Vec<_>>(),
            [next],
            "the reply keys once, at the next boundary"
        );

        let mut e = asking_engine();
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let (display, no_at) = question(&e);
        let t1 = tempo_core::timing::now_unix_ms() as u64;
        e.js8_answer_reply(true, display, no_at).expect("Yes");
        let overs = run_js8_loop_from(&mut e, t1, 60);
        assert_eq!(
            overs.iter().map(|o| o.0).collect::<Vec<_>>(),
            [(t1 / 15_000 + 1) * 15_000],
            "a Yes: the boundary after it"
        );
    }

    // ===== (D) AUTO off: the reply goes to the composer =====

    /// ⭐ With AUTO off a reply is put in the composer, JS8Call's compose box (`addMessageText`,
    /// mainwindow.cpp:9671), and never keyed by itself (:9674-9685), over minutes; the operator's
    /// Send keys it then, as their own message, the very frame the automatic reply keys.
    #[test]
    fn with_auto_off_a_js8_reply_goes_to_the_composer_and_keys_only_when_sent() {
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let mut auto = hb_engine("EN52", 0, 1500.0);
        auto.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let plan = auto
            .plan_tx(t0 / 15_000 + 1)
            .expect("AUTO on: the reply keys");
        let TxWaveform::Js8 {
            word: auto_word, ..
        } = plan.waveform
        else {
            panic!("a JS8 plan carries the typed waveform");
        };

        let mut e = hb_engine("EN52", 0, 1500.0);
        e.js8_arm(Js8Switch::Autoreply, false).expect("AUTO off");
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let p = e
            .js8_composer(false, None)
            .expect("the reply is in the composer");
        assert_eq!(p.text, "W1AW SNR -07");
        assert!(
            run_js8_loop_from(&mut e, t0, 120).is_empty(),
            "never keyed by itself"
        );
        assert_eq!(
            e.js8_composer(true, Some(p.id)),
            None,
            "taken into the box: nothing more is offered"
        );
        e.js8_send(None, p.text)
            .expect("the operator sends what the box holds");
        let slot = tempo_core::timing::now_unix_ms() as u64 / 15_000 + 1;
        let plan = e.plan_tx(slot).expect("Send keys it");
        let TxWaveform::Js8 { word, .. } = plan.waveform else {
            panic!("a JS8 plan carries the typed waveform");
        };
        assert_eq!(
            word, auto_word,
            "the frame the automatic reply keys, bit for bit"
        );
    }

    /// The composer is the operator's: TX off, outside the privileges, nothing in it keys, and
    /// the reply is still put there (JS8Call fills its box whatever TX is doing).
    #[test]
    fn with_auto_off_and_js8_tx_off_the_reply_still_reaches_the_composer() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.js8_arm(Js8Switch::Autoreply, false).expect("AUTO off");
        e.set_tx_enabled(false);
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        assert!(run_js8_loop_from(&mut e, t0, 30).is_empty(), "nothing keys");
        assert_eq!(
            e.js8_composer(false, None).map(|p| p.text).as_deref(),
            Some("W1AW SNR -07")
        );
    }

    // ===== (F) JS8Call's reply-skipping rules, on the engine =====

    /// The compose box holds replies (mainwindow.cpp:9364-9367): while the cockpit says its box
    /// holds text, a query to me is not answered, not even asked about; once it is empty, it is.
    #[test]
    fn a_js8_reply_is_not_made_while_the_compose_box_holds_text() {
        let mut e = asking_engine();
        assert_eq!(e.js8_composer(true, None), None, "the operator types");
        e.js8_ingest(&[snr_query_to_me("W1AW")], 0);
        let st = e.js8_state();
        assert!(
            st.pending_reply.is_none() && st.queue.is_empty(),
            "no reply made"
        );
        e.js8_composer(false, None);
        e.js8_ingest(&[snr_query_to_me("K1ABC")], 0);
        assert!(
            e.js8_state().pending_reply.is_some(),
            "the box empty: answered"
        );
    }

    /// A message to me still arriving holds replies (mainwindow.cpp:9369-9374): the first frame of
    /// a MSG to me and a query heard in the same pass draw no reply to the query; the MSG, once it
    /// has closed, draws its ACK.
    #[test]
    fn no_js8_reply_while_a_message_to_me_is_still_arriving() {
        let mut e = hb_engine("EN52", 0, 1500.0);
        let msg = msg_to_me("K1ABC", "HELLO FROM OHIO, THE BAND IS OPEN TO EUROPE");
        assert!(msg.len() > 2, "control: a multi-frame MSG");
        e.js8_ingest(&[msg[0].clone(), snr_query_to_me("W1AW")], 0);
        assert!(
            queued(&e).is_empty(),
            "no reply while the MSG to me arrives"
        );
        for d in &msg[1..] {
            e.js8_ingest(std::slice::from_ref(d), 0);
        }
        assert_eq!(
            queued(&e),
            ["KD9TAW: K1ABC ACK"],
            "the MSG, closed, draws its ACK"
        );
    }

    /// The lists reach the station as JS8Call reads them (`splitWords`, Configuration.cpp:
    /// 2416-2428): trimmed, upper-cased, the empties gone.
    #[test]
    fn the_js8_allow_and_deny_lists_reach_the_station() {
        let s = Settings {
            js8_autoreply_allow: vec![" w1aw ".into(), "".into(), "K1ABC".into()],
            js8_autoreply_deny: vec!["n0xyz".into()],
            js8_hb_ack_deny: vec!["kd2uwr ".into()],
            ..Settings::default()
        };
        let c = Engine::js8_station_config(&s);
        assert_eq!(c.autoreply_allow, ["W1AW", "K1ABC"]);
        assert_eq!(c.autoreply_deny, ["N0XYZ"]);
        assert_eq!(c.hb_ack_deny, ["KD2UWR"]);
    }

    // ===== heartbeat networking and presence, as JS8Call 3.0.3 has them =====

    /// `call`'s heartbeat, heard now at 1200 Hz (Normal).
    fn heard_hb(e: &mut Engine, call: &str) {
        e.js8_ingest(
            &[row(&hb(call, "FN31"), whole(), Js8Speed::Normal, 1200.0)],
            now_unix_secs() / 15,
        );
    }

    /// What the station has keyed, as the activity pane books it.
    fn booked(e: &Engine) -> Vec<String> {
        e.js8_state()
            .activity
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.text.clone())
            .collect()
    }

    /// The five switches reach the station as Settings has them.
    #[test]
    fn the_js8_heartbeat_and_presence_switches_reach_the_station() {
        let s = Settings {
            js8_hb_networking: true,
            js8_hb_anywhere: true,
            js8_hb_qso_pause: false,
            js8_hb_ack_rate_limit: true,
            js8_avoid_allcall: true,
            ..Settings::default()
        };
        let c = Engine::js8_station_config(&s);
        assert_eq!(
            (
                c.hb_networking,
                c.hb_anywhere,
                c.hb_qso_pause,
                c.hb_rate_limit,
                c.avoid_allcall
            ),
            (true, true, false, true, true)
        );
    }

    /// "Send Heartbeat Now" (JS8Call's HB button menu, `sendHB`, mainwindow.cpp:4321-4323): one
    /// heartbeat over in the next period, for an operator at 1500 Hz on a free 500-1000 Hz spot,
    /// booked where it keyed. The HB repeat stays as it was: off.
    #[test]
    fn a_js8_heartbeat_sent_now_keys_once_in_the_next_period() {
        let mut e = hb_engine("EN52", 5, 1500.0);
        e.js8_heartbeat_now().expect("sent now");
        let overs = run_js8_loop(&mut e, 60);
        let [(_, f0)] = overs[..] else {
            panic!("one heartbeat over, got {overs:?}");
        };
        assert!(
            (500.0..=950.0).contains(&f0) && f0 % 50.0 == 0.0,
            "on a free heartbeat spot: {f0}"
        );
        let st = e.js8_state();
        let mine: Vec<(String, f32)> = st
            .activity
            .iter()
            .filter(|r| r.mine)
            .map(|r| (r.text.clone(), r.freq_hz))
            .collect();
        assert_eq!(
            (mine, st.hb_on),
            (vec![("KD9TAW: @HB HEARTBEAT EN52".to_string(), f0)], false),
            "booked where it keyed, and the repeat is still off"
        );
    }

    /// A heartbeat, sent now or by the repeat, is refused where JS8Call has no HB button:
    /// heartbeat networking off, or a speed it sends none at. Nothing keys, and the operator reads
    /// why.
    #[test]
    fn a_js8_heartbeat_is_refused_without_networking_or_at_turbo() {
        let mut e = hb_engine("EN52", 5, 1500.0);
        e.js8_arm(Js8Switch::HbNet, false).expect("networking off");
        assert_eq!(
            (e.js8_heartbeat_now(), e.js8_arm(Js8Switch::Hb, true)),
            (
                Err(JS8_HB_NEEDS_NETWORKING.to_string()),
                Err(JS8_HB_NEEDS_NETWORKING.to_string())
            ),
            "networking off"
        );
        assert_eq!(
            (run_js8_loop(&mut e, 60), e.js8_state().hb_on),
            (Vec::new(), false),
            "networking off: nothing keys"
        );
        let mut e = hb_engine("EN52", 5, 1500.0);
        e.js8_set_speed(3).expect("Turbo");
        assert_eq!(
            (e.js8_heartbeat_now(), e.js8_arm(Js8Switch::Hb, true)),
            (Err(JS8_HB_SPEED.to_string()), Err(JS8_HB_SPEED.to_string())),
            "Turbo"
        );
        assert_eq!(
            (run_js8_loop(&mut e, 60), e.js8_state().hb_on),
            (Vec::new(), false),
            "Turbo: nothing keys"
        );
    }

    /// At Turbo neither a heartbeat nor an HB ACK keys: switching to it stops a running HB repeat
    /// and drops an HB ACK waiting to key, and a heartbeat heard there is not answered. No over at
    /// all in two minutes. Back at Normal the same heartbeat is answered (the control).
    #[test]
    fn at_turbo_no_js8_heartbeat_or_hb_ack_keys() {
        let mut e = hb_engine("EN52", 1, 1500.0);
        e.js8_arm(Js8Switch::HbAck, true).expect("HB ACK on");
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        heard_hb(&mut e, "W1AW");
        assert!(
            !e.js8_state().queue.is_empty() && e.js8_state().hb_on,
            "control: an HB ACK waits and the repeat is on"
        );
        e.js8_set_speed(3).expect("Turbo");
        let st = e.js8_state();
        assert_eq!(
            (st.hb_on, st.queue.len()),
            (false, 0),
            "Turbo: the repeat stopped and the HB ACK was dropped"
        );
        heard_hb(&mut e, "K1ABC");
        assert_eq!(
            (run_js8_loop(&mut e, 120), booked(&e)),
            (Vec::new(), Vec::<String>::new()),
            "nothing keys at Turbo"
        );
        e.js8_set_speed(1).expect("Normal");
        heard_hb(&mut e, "K1ABC");
        let overs = run_js8_loop(&mut e, 60);
        assert_eq!(
            booked(&e),
            ["KD9TAW: K1ABC HEARTBEAT SNR -07"],
            "control: at Normal the heartbeat is answered: {overs:?}"
        );
    }

    /// The planner's own hold: a heartbeat or HB ACK keys only with heartbeat networking on and at
    /// a speed that heartbeats, as Settings has them at plan time. A station not yet told (its
    /// config stale) is what it holds; with Settings unchanged both key (the control).
    #[test]
    fn the_planner_withholds_a_heartbeat_or_hb_ack_settings_no_longer_allow() {
        type Stale = fn(&mut Engine);
        let rows: [(&str, Stale, usize); 3] = [
            ("unchanged", |_| {}, 2),
            ("Turbo", |e| e.settings.js8_speed = 3, 0),
            (
                "networking off",
                |e| e.settings.js8_hb_networking = false,
                0,
            ),
        ];
        for (what, stale, keys) in rows {
            let mut e = hb_engine("EN52", 30, 1500.0);
            e.js8_arm(Js8Switch::HbAck, true).expect("HB ACK on");
            e.js8_heartbeat_now().expect("sent now");
            heard_hb(&mut e, "W1AW");
            stale(&mut e);
            assert!(
                e.js8_station.hb_allowed(),
                "{what}: control: the station still holds its old config"
            );
            let overs = run_js8_loop(&mut e, 60);
            assert_eq!(
                (overs.len(), booked(&e).len()),
                (keys, keys),
                "{what}: what keys and is booked: {overs:?}"
            );
        }
    }

    /// "Allow heartbeat transmissions outside of heartbeat sub-channel": on, a heartbeat and an HB
    /// ACK key on the operator's 1500 Hz; off, each on a free spot in 500-1000 Hz.
    #[test]
    fn with_hb_anywhere_a_js8_heartbeat_and_hb_ack_key_on_the_tx_offset() {
        for anywhere in [false, true] {
            let mut e = hb_engine("EN52", 30, 1500.0);
            e.settings.js8_hb_anywhere = anywhere;
            e.js8_apply_station_config();
            e.js8_arm(Js8Switch::HbAck, true).expect("HB ACK on");
            e.js8_heartbeat_now().expect("sent now");
            heard_hb(&mut e, "W1AW");
            let f0s: Vec<f32> = run_js8_loop(&mut e, 60).iter().map(|o| o.1).collect();
            if anywhere {
                assert_eq!(f0s, [1500.0, 1500.0], "anywhere: both on the TX offset");
            } else {
                assert!(
                    f0s.len() == 2 && f0s.iter().all(|f| (500.0..=950.0).contains(f)),
                    "not anywhere: both on free heartbeat spots: {f0s:?}"
                );
            }
        }
    }

    /// The To box is JS8Call's selected call for the QSO pause (on by default): while it holds a
    /// call the HB repeat is paused and no heartbeat is answered; cleared, the repeat is back and
    /// one is. Turning the repeat on with a call there clears the station's selection, as
    /// JS8Call's HB button does.
    #[test]
    fn the_js8_to_box_is_the_selected_call_for_the_qso_pause() {
        use crate::dto::Js8HbAckSuppressed;
        let mut e = hb_engine("EN52", 5, 1500.0);
        e.js8_arm(Js8Switch::HbAck, true).expect("HB ACK on");
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        e.js8_select_call(Some(" w1aw ".into()));
        let st = e.js8_state();
        let p = st.presence.clone().expect("the desktop's presence");
        assert_eq!(
            (st.hb_on, p.hb_paused, p.hb_ack_suppressed),
            (false, true, Some(Js8HbAckSuppressed::QsoPause)),
            "W1AW in the To box: the repeat paused, an HB ACK held back"
        );
        heard_hb(&mut e, "K1ABC");
        assert_eq!(
            (run_js8_loop(&mut e, 60), booked(&e)),
            (Vec::new(), Vec::<String>::new()),
            "nothing keys while W1AW is selected"
        );
        e.js8_select_call(None);
        let st = e.js8_state();
        assert_eq!(
            (st.hb_on, st.presence.map(|p| p.hb_paused)),
            (true, Some(false)),
            "the To box cleared: the repeat is back"
        );
        heard_hb(&mut e, "K1ABC");
        run_js8_loop(&mut e, 60);
        assert_eq!(
            booked(&e),
            ["KD9TAW: K1ABC HEARTBEAT SNR -07"],
            "and the heartbeat is answered"
        );
        e.js8_select_call(Some("W1AW".into()));
        e.js8_arm(Js8Switch::Hb, true).expect("HB on");
        assert_eq!(
            e.js8_state().presence.map(|p| p.hb_ack_suppressed),
            Some(None),
            "turning the repeat on cleared the selected call"
        );
    }

    /// The desktop's cockpit reads presence, and the Remote page's sample carries the same, which
    /// that page's parser lists. At Turbo an armed HB ACK says the speed holds it back, on both.
    #[test]
    fn js8_presence_reaches_the_desktop_and_the_remote_sample() {
        use crate::dto::Js8HbAckSuppressed;
        let mut e = hb_engine("EN52", 0, 1500.0);
        e.settings.js8_avoid_allcall = true;
        e.js8_apply_station_config();
        assert_eq!(
            e.js8_state().presence,
            Some(Js8Presence {
                hb_networking: true,
                hb_speed: true,
                hb_paused: false,
                hb_ack_suppressed: None,
                hb_ack_rate_blocked: Vec::new(),
                qso_pause: true,
                avoid_allcall: true,
            })
        );
        let sample = e.bounded_js8_state().expect("bounded");
        let json = serde_json::to_value(&sample).expect("serialises");
        assert_eq!(
            (&sample.presence, &json["presence"]["avoidAllcall"]),
            (&e.js8_state().presence, &serde_json::json!(true)),
            "the Remote's sample carries the desktop's presence"
        );
        e.js8_arm(Js8Switch::HbAck, true).expect("HB ACK on");
        e.js8_set_speed(3).expect("Turbo");
        assert_eq!(
            e.js8_state()
                .presence
                .map(|p| (p.hb_speed, p.hb_ack_suppressed)),
            Some((false, Some(Js8HbAckSuppressed::Speed))),
            "Turbo: no heartbeats, and the armed HB ACK says why"
        );
        assert_eq!(
            e.bounded_js8_state()
                .and_then(|s| s.presence)
                .map(|p| (p.hb_speed, p.hb_ack_suppressed)),
            Some((false, Some(Js8HbAckSuppressed::Speed))),
            "and so does the Remote's"
        );
    }

    /// Not taking part in @ALLCALL, an @ALLCALL query is not marked as addressed to me in the
    /// activity pane; taking part (the default), it is.
    #[test]
    fn allcall_traffic_is_not_shown_as_mine_while_i_take_no_part_in_it() {
        for avoid in [false, true] {
            let mut e = hb_engine("EN52", 0, 1500.0);
            e.settings.js8_avoid_allcall = avoid;
            e.js8_apply_station_config();
            let q = Frame::Directed {
                from: CallRef::Base("W1AW".to_string()),
                to: CallRef::AllCall,
                cmd: Command::QueryMsgs,
                num: None,
                portable_from: false,
                portable_to: false,
            };
            e.js8_ingest(&[row(&q, whole(), Js8Speed::Normal, 1200.0)], 0);
            let r = e.js8_state().activity.last().cloned().expect("the row");
            assert_eq!(
                r.directed_to_me, !avoid,
                "not taking part in @ALLCALL {avoid}"
            );
        }
    }

    /// The 55-minute rule's record is journaled the moment it changes: W1AW, heard twice within the
    /// window, draws one HB ACK and goes on the "never acknowledge" list in Settings, and the
    /// journal names it as the rule's. A fresh engine with those Settings, loading that journal,
    /// answers K1ABC but not W1AW, and still names W1AW as the rule's.
    #[test]
    fn the_55_minute_rule_list_is_kept_in_the_js8_journal() {
        let dir = std::env::temp_dir().join(format!("nexus-js8-hbrate-{}", std::process::id()));
        let path = dir.join("js8_station.json");
        let _ = std::fs::remove_file(&path);
        let with_rule = |e: &mut Engine| {
            e.settings.js8_hb_ack_rate_limit = true;
            e.js8_apply_station_config();
            e.js8_arm(Js8Switch::HbAck, true).expect("HB ACK on");
        };
        let mut e = hb_engine("EN52", 30, 1500.0);
        e.set_js8_journal_path(path.clone());
        with_rule(&mut e);
        // The two heartbeats come a moment apart on the wall clock, inside the 15 minutes
        // JS8Call's @ALLCALL cache holds a second acknowledgement off before the rule sees it:
        // the cache is shortened here so the rule acts on the second.
        let mut c = e.js8_station.config().clone();
        c.allcall_reply_interval_ms = 0;
        e.js8_station.set_config(c);
        heard_hb(&mut e, "W1AW");
        heard_hb(&mut e, "W1AW");
        run_js8_loop(&mut e, 60);
        assert_eq!(
            (
                booked(&e),
                e.js8_state().presence.map(|p| p.hb_ack_rate_blocked)
            ),
            (
                vec!["KD9TAW: W1AW HEARTBEAT SNR -07".to_string()],
                Some(vec!["W1AW".to_string()])
            ),
            "one HB ACK, then W1AW is on the rule's list"
        );
        e.station.journals.settle();
        let text = std::fs::read_to_string(&path).expect("journaled");
        assert!(
            text.contains(r#""hbRateBlocked":["W1AW"]"#),
            "the journal holds the list: {text}"
        );
        assert_eq!(
            e.settings().js8_hb_ack_deny,
            ["W1AW"],
            "on the never-ACK list in Settings"
        );
        let mut fresh = hb_engine("EN52", 30, 1500.0);
        fresh.settings.js8_hb_ack_deny = e.settings().js8_hb_ack_deny.clone();
        with_rule(&mut fresh);
        fresh.js8_load_journal(&text);
        assert_eq!(
            fresh.js8_state().presence.map(|p| p.hb_ack_rate_blocked),
            Some(vec!["W1AW".to_string()]),
            "the journal names it as the rule's"
        );
        heard_hb(&mut fresh, "W1AW");
        heard_hb(&mut fresh, "K1ABC");
        run_js8_loop(&mut fresh, 60);
        assert_eq!(
            booked(&fresh),
            ["KD9TAW: K1ABC HEARTBEAT SNR -07"],
            "after the launch: K1ABC answered, W1AW not"
        );
        drop((e, fresh)); // their journal writes land before the folder goes
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ===== the lists and views: where each row stands in its message, and the call list's writes =====

    fn flags(first: bool, last: bool, assembled: bool) -> Option<Js8RowFlags> {
        Some(Js8RowFlags {
            first,
            last,
            assembled,
        })
    }

    /// The RX pane and Band activity join a message's frames and close it with ♢ as JS8Call's
    /// `displayTextForFreq` does (mainwindow.cpp:3106-3170, processRxActivity.cpp:116-128), by each
    /// frame's i3 First/Last bits. So every frame row carries its own, and the reassembled line of a
    /// multi-frame message says it is one, never a frame.
    #[test]
    fn each_activity_row_says_where_it_stands_in_its_message() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        e.js8_ingest(
            &[row(&hb("W0IND", "EN52"), whole(), Js8Speed::Normal, 900.0)],
            4,
        );
        let header = Frame::Directed {
            from: CallRef::Base("W1AW".to_string()),
            to: CallRef::Base("KD9TAW".to_string()),
            cmd: Command::Msg,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        let open = I3 {
            first: true,
            last: false,
            data: false,
        };
        e.js8_ingest(&[row(&header, open, Js8Speed::Normal, 1500.0)], 5);
        let text = Frame::Data {
            text: "HELLO".into(),
            dense: false,
        };
        let close = I3 {
            first: false,
            last: true,
            data: true,
        };
        e.js8_ingest(&[row(&text, close, Js8Speed::Normal, 1500.0)], 6);
        let got: Vec<_> = e
            .js8_state()
            .activity
            .iter()
            .map(|r| (r.from.clone(), r.flags))
            .collect();
        assert_eq!(
            got,
            vec![
                ("W0IND".to_string(), flags(true, true, false)),
                ("W1AW".to_string(), flags(true, false, false)),
                (String::new(), flags(false, true, false)),
                ("W1AW".to_string(), flags(false, false, true)),
            ],
            "a heartbeat is one whole frame; the MSG's header opens, its text closes, then the line"
        );
    }

    /// My own frames too: JS8Call prints what it sends in the RX pane and closes the line with ♢
    /// after the last frame (mainwindow.cpp:3660-3668).
    #[test]
    fn my_own_frames_say_where_they_stand_in_the_message() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_tier(Tier::Js8);
        e.js8_note_tx_done("KD9TAW: K1ABC MSG ", 15_000, 1500.0, true, false);
        e.js8_note_tx_done("HELLO", 30_000, 1500.0, false, true);
        let got: Vec<_> = e
            .js8_state()
            .activity
            .iter()
            .map(|r| (r.mine, r.flags))
            .collect();
        assert_eq!(
            got,
            vec![
                (true, flags(true, false, false)),
                (true, flags(false, true, false))
            ]
        );
    }

    /// The Remote page's sample carries each row's place and each station's ★ and ☎ as the
    /// desktop's state does, which that page's parser lists.
    #[test]
    fn the_remote_sample_carries_the_rows_places_and_the_stations_marks() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        e.js8_load_journal(
            &serde_json::json!({"heard": [{"call": "W1AW", "grid": "FN31", "snrDb": -8,
                "freqHz": 1500.0, "speed": "normal", "lastMs": 1, "lastHb": false, "lastCq": true,
                "storedMsgs": 0, "ackMs": 5, "cqMs": 6}],
            "inbox": [], "allcallReplied": [], "nextInboxId": 1})
            .to_string(),
        );
        e.js8_ingest(
            &[row(&hb("W0IND", "EN52"), whole(), Js8Speed::Normal, 900.0)],
            4,
        );
        let native = serde_json::to_value(e.js8_state()).unwrap();
        let w1aw = native["stations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["call"] == "W1AW")
            .expect("the journal's station");
        assert_eq!(
            (w1aw["ackMs"].as_u64(), w1aw["cqMs"].as_u64()),
            (Some(5), Some(6)),
            "control"
        );
        assert!(native["activity"][0].get("flags").is_some(), "control");
        let sample = serde_json::to_value(e.bounded_js8_state().unwrap()).unwrap();
        assert_eq!(sample["stations"], native["stations"]);
        assert_eq!(sample["activity"], native["activity"]);
    }

    /// The call list's two writes: "Block This Station" (UI_Constructor.cpp:1119-1129) and the
    /// groups it joins and leaves (:1077-1081, :1108-1109). Each lands in Settings normalised, the
    /// groups reach the station at once, and @APRSIS / @JS8NET are refused with nothing changed.
    #[test]
    fn the_call_lists_writes_land_in_settings_and_the_station() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        e.js8_set_rx_blocklist(vec![
            " w1aw ".into(),
            "".into(),
            "W1AW".into(),
            "k1abc".into(),
        ]);
        assert_eq!(e.settings().js8_rx_blocklist, ["W1AW", "K1ABC"]);
        e.js8_set_groups(vec![
            "fun".into(),
            "@NET ".into(),
            "@FUN".into(),
            " ".into(),
        ])
        .expect("ordinary groups join");
        assert_eq!(e.settings().js8_groups, ["@FUN", "@NET"]);
        assert_eq!(
            e.js8_station.config().groups,
            ["@FUN", "@NET"],
            "the station at once"
        );
        let refused = e.js8_set_groups(vec!["@FUN".into(), "@aprsis".into()]);
        assert_eq!(
            refused,
            Err("@APRSIS is a group that cannot be joined".to_string())
        );
        assert_eq!(e.settings().js8_groups, ["@FUN", "@NET"], "nothing changed");
        e.js8_set_groups(vec!["@NET".into()]).expect("leave @FUN");
        assert_eq!(e.js8_station.config().groups, ["@NET"]);
    }

    /// The call list's "Remove" on `@ALLCALL` and its "Add" of `@ALLCALL` (JS8Call's
    /// `set_avoid_allcall`, UI_Constructor.cpp:1108, :1075): "Do not participate in the @ALLCALL
    /// group" lands in Settings, in the station at once, and in the state the cockpit reads.
    #[test]
    fn removing_and_adding_allcall_on_the_call_list_sets_do_not_participate() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        let seen = |e: &Engine| {
            (
                e.settings().js8_avoid_allcall,
                e.js8_station.config().avoid_allcall,
                e.js8_state().presence.map(|p| p.avoid_allcall),
            )
        };
        assert_eq!(
            seen(&e),
            (false, false, Some(false)),
            "control: taking part, as JS8Call ships"
        );
        e.js8_set_avoid_allcall(true);
        assert_eq!(
            seen(&e),
            (true, true, Some(true)),
            "removed: not taking part"
        );
        e.js8_set_avoid_allcall(false);
        assert_eq!(
            seen(&e),
            (false, false, Some(false)),
            "added back: taking part again"
        );
    }
}

#[cfg(test)]
#[path = "js8_replies_tests.rs"]
mod replies_tests;

#[cfg(test)]
#[path = "js8_followups_tests.rs"]
mod followups_tests;

#[cfg(test)]
#[path = "js8_design_tests.rs"]
mod design_tests;

#[cfg(test)]
#[path = "js8_programs_tests.rs"]
mod programs_tests;
