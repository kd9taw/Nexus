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
//! RECEIVE-ONLY IN THIS BUILD (B5): every transmit verb below refuses and says so in
//! `last_error`; `Js8Mode` declares `tx: false`; and `js8_apply_station_config` forces the
//! station's autoreply/relay/HB-ack OFF so it can never build an outbox it has no way to
//! drain. The operator-gated TX batch replaces the stubs and removes the override; the RX
//! path (`js8_ingest` → `Reassembler` → `Station` → activity/heard/inbox) does not change.
//!
//! ⚠️ Inside this file `js8` would name THIS module; the crate is spelled `::js8::…`.
//! The actions match every path funnels through is `js8_handle_actions`.

use std::path::PathBuf;

use ::js8::proto::callsign::{split_portable, CallRef};
use ::js8::proto::reassembly::RxFrame;
use ::js8::proto::station::{FreqHint, InboxState, StationSnapshot};
use ::js8::{Frame, MessageEvent, Origin, RawDecode, StationAction, StationConfig, Word87};
use modes::Js8Speed;

use super::{now_unix_secs, Engine, TxPlan, TxWaveform};
use crate::dto::{
    Js8ActivityRow, Js8Armed, Js8PendingReply, Js8QueueRow, Js8State, SourceKind, Tier,
};

/// Activity rows kept for the cockpit (newest last).
const JS8_ACTIVITY_CAP: usize = 200;
/// JS8Call flags a decode with quality < 0.17 as low-confidence (decodedtext.cpp).
const JS8_LOW_CONF: f32 = 0.17;
/// Row-dedupe depth (multi-speed pass vs boundary pass; see `js8_dedupe`).
const JS8_SEEN_CAP: usize = 64;
/// JS8Call's @ALLCALL reply cap: one reply per station per 15 minutes.
const JS8_ALLCALL_INTERVAL_MS: u64 = 15 * 60 * 1000;
/// The one refusal every transmit verb returns in the receive-only build.
const JS8_RX_ONLY: &str = "JS8 transmit is not in this build yet — receive only";

/// The SECOND act of the two-act rule, by origin: `Autoreply`/`Relay`/`HbAck` are the
/// persisted switches, `Hb` is the session-only heartbeat schedule. Lowercase on the wire
/// (`autoreply | relay | hback | hb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Js8Switch {
    Autoreply,
    Relay,
    HbAck,
    Hb,
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

impl Engine {
    /// ONE place Settings → `StationConfig`. Identity is uppercased and trimmed the way the
    /// wire packs it; the idle floor of 5 minutes is applied here (0 stays 0 = off); the
    /// autoreply countdown is one period + 2 s at the TRANSMIT speed.
    pub(crate) fn js8_station_config(&self) -> StationConfig {
        let s = &self.settings;
        let speed = self.js8_tx_speed_setting();
        StationConfig {
            mycall: s.mycall.trim().to_ascii_uppercase(),
            grid: s.mygrid.trim().to_ascii_uppercase(),
            speed,
            autoreply: s.js8_autoreply,
            relay: s.js8_relay,
            hb_ack: s.js8_hb_ack,
            hb_interval_min: s.js8_hb_interval_min,
            idle_watchdog_min: match s.js8_idle_watchdog_min {
                0 => 0,
                m => m.max(5),
            },
            groups: s
                .js8_groups
                .iter()
                .map(|g| g.trim().to_ascii_uppercase())
                .filter(|g| !g.is_empty())
                .collect(),
            info: s.js8_info.clone(),
            status: s.js8_status.clone(),
            allcall_reply_interval_ms: JS8_ALLCALL_INTERVAL_MS,
            reply_delay_ms: u64::from(speed.period_s()) * 1000 + 2_000,
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
    /// speed/mask change and by `js8_enter`.
    pub(crate) fn js8_apply_station_config(&mut self) {
        let mut cfg = self.js8_station_config();
        // ⭐ B5 RECEIVE-ONLY OVERRIDE. The persisted switches keep JS8Call's defaults (the
        // cockpit shows them), but the STATION is told they are off: a station with no
        // transmit path must never compute a reply into an outbox nothing drains. The TX
        // batch deletes these three lines together with the refusing stubs below.
        cfg.autoreply = false;
        cfg.relay = false;
        cfg.hb_ack = false;
        if self.js8_station.config() != &cfg {
            self.js8_station.set_config(cfg);
        }
    }

    /// View entry: the station reads Settings, then `set_tier(Js8)` — which is a complete
    /// no-op on the same tier, and on a real switch flushes the decode context, swaps the
    /// decoder to the JS8 window and retunes to `js8_band_plan()` for the current band
    /// (stay-on-miss). KEYS NOTHING: the TX latch is untouched and `tier_is_rx_only`
    /// refuses to arm it anyway.
    pub fn js8_enter(&mut self) {
        self.js8_apply_station_config();
        self.set_tier(Tier::Js8);
    }

    /// The cockpit poll. Every field is engine truth at poll time; `armed` is
    /// `switch && tx_enabled && !idle_tripped` per origin — never the switch alone.
    pub fn js8_state(&self) -> Js8State {
        let s = &self.settings;
        let idle_tripped = self.js8_station.idle_tripped();
        let live = self.tx_enabled && !idle_tripped;
        Js8State {
            speed: self.js8_tx_speed_setting(),
            rx_speeds: s.js8_rx_speeds,
            tx_enabled: self.tx_enabled,
            sending: self.app.transmitting(),
            hb_on: self.js8_hb_on,
            hb_next_at_ms: self.js8_station.hb_next_ms(),
            hb_interval_min: s.js8_hb_interval_min,
            autoreply: s.js8_autoreply,
            relay: s.js8_relay,
            hb_ack: s.js8_hb_ack,
            armed: Js8Armed {
                autoreply: s.js8_autoreply && live,
                relay: s.js8_relay && live,
                hb_ack: s.js8_hb_ack && live,
                hb: self.js8_hb_on && live,
            },
            idle_minutes: self.js8_station.idle_minutes(),
            idle_limit_min: self.js8_station.config().idle_watchdog_min,
            idle_tripped,
            activity: self.js8_activity.iter().cloned().collect(),
            stations: self.js8_station.heard().to_vec(),
            inbox: self.js8_station.inbox().to_vec(),
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
            let events = self.js8_reasm.feed(&rx, now_ms);
            self.js8_handle_events(events, low_conf, now_ms);
        }
    }

    /// The engine's once-a-second JS8 clock (the radio loop calls it at `Tier::Js8`; ingest
    /// calls it first): buffer ageing, then the station's own tick (HB schedule, idle
    /// minutes — inert in the receive-only build, but the plumbing is the TX batch's).
    pub fn js8_tick(&mut self, now_ms: u64) {
        let aged = self.js8_reasm.age(now_ms);
        self.js8_handle_events(aged, false, now_ms);
        let actions = self.js8_station.tick(now_ms);
        self.js8_handle_actions(actions);
    }

    /// Reassembler events → activity rows + station actions.
    fn js8_handle_events(&mut self, events: Vec<MessageEvent>, low_conf: bool, now_ms: u64) {
        for ev in events {
            let row = match &ev {
                MessageEvent::Frame(rx) => Some(self.js8_row_for_frame(rx, low_conf)),
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
                    directed_to_me: m.to.as_ref().is_some_and(|to| self.js8_addressed_to_me(to)),
                    mine: false,
                    complete: m.complete,
                    low_conf: false,
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
        }
    }

    /// One decoded frame → its activity row (JS8Call's display line, byte-exact).
    fn js8_row_for_frame(&self, rx: &RxFrame, low_conf: bool) -> Js8ActivityRow {
        let (from, directed_to_me) = match &rx.frame {
            Frame::Heartbeat { call, .. }
            | Frame::Compound { call, .. }
            | Frame::CompoundDirected { call, .. } => (call.clone(), false),
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
        }
    }

    /// My base call, @ALLCALL, or a group I have joined (JS8Call's addressed-to-me rule).
    fn js8_addressed_to_me(&self, to: &CallRef) -> bool {
        let (my_base, _) = split_portable(self.settings.mycall.trim());
        match to {
            CallRef::Base(c) => c.eq_ignore_ascii_case(my_base),
            CallRef::AllCall => true,
            CallRef::Group(_) => {
                let g = to.render();
                self.js8_station
                    .config()
                    .groups
                    .iter()
                    .any(|x| x.eq_ignore_ascii_case(&g))
            }
            CallRef::Placeholder | CallRef::Js8Net => false,
        }
    }

    /// THE actions match. Every path (ingest, tick, the verbs) funnels here so a new action
    /// is handled once. In the receive-only build the outbox actions cannot occur (the
    /// station's switches are forced off); the TX batch fills those arms.
    fn js8_handle_actions(&mut self, actions: Vec<StationAction>) {
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
                StationAction::Relayed { path, text } => tempo_core::applog::info(
                    "js8",
                    &format!("relayed via {}: {text}", path.join(">")),
                ),
                // Receive-only: nothing to trip, nothing queued, nothing pending. The heard
                // list is read straight from the station at poll time.
                StationAction::IdleTripped
                | StationAction::Queued { .. }
                | StationAction::ReplyPending { .. }
                | StationAction::HeardChanged => {}
            }
        }
    }

    // ---- operator verbs (RX side) ----

    /// Change the TRANSMIT speed (0..=3). Re-points the decoder at the new window and the
    /// slot clock at the new period (the audio loop follows `active_slot_secs`), and the
    /// station at the new countdown. The latch is untouched. The command layer persists.
    pub fn js8_set_speed(&mut self, speed_idx: u8) -> Result<(), String> {
        if Js8Speed::from_index(speed_idx).is_none() {
            return Err(format!(
                "JS8 speed index {speed_idx} is not 0..=3 (Slow/Normal/Fast/Turbo)"
            ));
        }
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
                self.install_source(Box::new(modes::NativeSource::from_kind(kind)));
                self.clear_decode_context();
            }
        }
        self.js8_apply_station_config();
        Ok(())
    }

    /// Change which speeds the receiver decodes (bitmask, `Js8Speed::bit()`). A mask that
    /// decodes nothing is refused — nobody means "go deaf". The command layer persists.
    pub fn js8_set_rx_speeds(&mut self, mask: u8) -> Result<(), String> {
        if mask & 0x0F == 0 {
            return Err("at least one JS8 speed must stay enabled".to_string());
        }
        self.settings.js8_rx_speeds = mask & 0x0F;
        Ok(())
    }

    pub fn js8_inbox_mark(&mut self, id: u32, state: InboxState) -> Result<(), String> {
        if self.js8_station.inbox_mark(id, state) {
            self.js8_persist();
            Ok(())
        } else {
            Err(format!("no JS8 inbox message #{id}"))
        }
    }

    pub fn js8_inbox_delete(&mut self, id: u32) -> Result<(), String> {
        if self.js8_station.inbox_delete(id) {
            self.js8_persist();
            Ok(())
        } else {
            Err(format!("no JS8 inbox message #{id}"))
        }
    }

    // ---- journal (the pending_msgs.json contract) ----

    /// Where the station snapshot (inbox, heard, @ALLCALL replies, next id) is journaled.
    pub fn set_js8_journal_path(&mut self, path: PathBuf) {
        self.js8_journal_path = Some(path);
    }

    /// Restore the journal at startup (best-effort: a missing/corrupt file yields an empty
    /// station, exactly like `load_pending_msgs`).
    pub fn js8_load_journal(&mut self, text: &str) {
        let Ok(snap) = serde_json::from_str::<StationSnapshot>(text) else {
            return;
        };
        self.js8_station.restore(snap, now_unix_secs() * 1000);
    }

    /// Journal the station the MOMENT its inbox changes — write-tmp + fsync + rename, like
    /// `persist_pending_msgs`, so a crash cannot drop a stored message the operator saw land.
    pub(crate) fn js8_persist(&self) {
        let Some(path) = &self.js8_journal_path else {
            return;
        };
        let Ok(text) = serde_json::to_string(&self.js8_station.snapshot()) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        let res = std::fs::File::create(&tmp)
            .and_then(|mut f| {
                std::io::Write::write_all(&mut f, text.as_bytes())?;
                f.sync_all()
            })
            .and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = res {
            eprintln!("tempo: failed to journal the JS8 station: {e}");
        }
    }

    // ---- transmit verbs: REFUSING STUBS in the receive-only build (the TX batch replaces
    // every body below; the signatures are the contract) ----

    fn js8_refuse(&mut self) -> Result<(), String> {
        self.js8_last_error = Some(JS8_RX_ONLY.to_string());
        Err(JS8_RX_ONLY.to_string())
    }

    pub fn js8_send(&mut self, _to: Option<String>, _text: String) -> Result<(), String> {
        self.js8_refuse()
    }

    pub fn js8_send_command(&mut self, _to: String, _cmd: u8, _arg: String) -> Result<(), String> {
        self.js8_refuse()
    }

    pub fn js8_call_cq(&mut self, _idx: u8) -> Result<(), String> {
        self.js8_refuse()
    }

    /// The second act. Refused outright here: with no transmit path there is nothing to arm,
    /// and persisting a switch from a verb that cannot act would misreport the station.
    pub fn js8_arm(&mut self, _which: Js8Switch, _on: bool) -> Result<(), String> {
        self.js8_refuse()
    }

    /// Cancel the pending autoreply (none can exist here; safe no-op through the station).
    pub fn js8_cancel(&mut self) {
        self.js8_station.cancel_pending_reply();
    }

    /// Drop the outbox (sender-class, not a stop; empty here).
    pub fn js8_drop_queue(&mut self) {
        self.js8_station.drop_queue();
    }

    /// Halt is total: outbox, pending reply and HB schedule. Wired from `halt_tx` /
    /// `set_tier` / `set_mode` by the TX batch; callable now.
    pub fn js8_halt_clear(&mut self) {
        self.js8_station.halt();
        self.js8_hb_on = false;
    }

    /// Book a JS8 over at PLAN time — the beacon and QSO arms' rule, and for the same
    /// reason: the plan is the transmit decision, and `commit_tx` refuses only the
    /// microsecond races (JS8's build is pure Rust). Three records: the own-TX row for the
    /// Rx-Frequency feed (`record_own_tx`), a `mine` row in the JS8 activity ring the
    /// cockpit's activity pane reads, and — when ALL.TXT is on — the `Tx` line in the
    /// FT/beacon writers' shape (SNR/DT 0, audio = our TX offset, into the shared ALL.TXT
    /// buffer with the same 5000-line cap). `now_ms` is the PERIOD START of the over
    /// (slot × period), not the wall clock: alltxt.rs's rule is that only the slot knows
    /// which period an over belongs to.
    pub(crate) fn js8_note_tx_done(&mut self, plan_display: &str, now_ms: u64) {
        self.record_own_tx(plan_display.to_string());
        self.js8_activity.push_back(Js8ActivityRow {
            at_ms: now_ms,
            speed: self.js8_tx_speed(),
            freq_hz: self.tx_offset_hz,
            snr_db: 0,
            dt_s: 0.0,
            from: self.settings.mycall.trim().to_ascii_uppercase(),
            text: plan_display.to_string(),
            directed_to_me: false,
            mine: true,
            complete: true,
            low_conf: false,
        });
        while self.js8_activity.len() > JS8_ACTIVITY_CAP {
            self.js8_activity.pop_front();
        }
        if self.settings.write_all_txt {
            self.station
                .all_txt_pending
                .push(crate::alltxt::all_txt_line(
                    now_ms / 1000,
                    self.settings.dial_mhz,
                    true,
                    "JS8",
                    0,
                    0.0,
                    self.tx_offset_hz,
                    plan_display,
                ));
            let len = self.station.all_txt_pending.len();
            if len > 5000 {
                self.station.all_txt_pending.drain(0..len - 5000);
            }
        }
    }

    /// Tier-routed TX planner — the ONLY place a `proto::TxFrame` becomes a `TxPlan`
    /// (spec invariant 13). Reached only after `plan_tx`'s mode-agnostic guards
    /// (`!tx_enabled || tuning || !tx_allowed()`, `tier_is_rx_only`, operating mode
    /// Digital), so `tx_enabled` — the FIRST operator act — is already true here.
    ///
    /// Order: identity gate → decode-only refusal → one-frame-per-period latch → station
    /// outbox → origin gate (the SECOND act, re-read at plan time every slot) → wall-clock
    /// watchdog (all origins but Heartbeat) → f0 → book → plan. Nothing here moves the dial
    /// (invariant 9). Booking is at plan time, the beacon / QSO arms' rule.
    pub fn plan_js8_tx(&mut self, slot: u64) -> Option<TxPlan> {
        // Identity, fail-closed: Js8Mode declares `structured_identity`, so a blank or
        // unparsable MYCALL refuses here (`needs_grid = false` — JS8 frames carry the
        // grid optionally; NMAXGRID means "no grid"). `proto::compose` additionally
        // refuses a MYCALL that cannot be base-packed, before anything is queued.
        if self.structured_tx_ready(false).is_err() {
            self.set_transmitting(false);
            return None;
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
        // JS8Call's "free HB slot" rule: no activity within one signal bandwidth in the
        // last 30 s. Snapshot the heard table first — the station is borrowed mutably
        // by `next_frame` below.
        let bw = 8.0 * speed.tone_spacing_hz();
        let heard: Vec<(f32, u64)> = self
            .js8_station
            .heard()
            .iter()
            .map(|h| (h.freq_hz, h.last_ms))
            .collect();
        let busy = move |f: f32| {
            heard
                .iter()
                .any(|&(hf, at)| (hf - f).abs() < bw && period_start_ms.saturating_sub(at) < 30_000)
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
            Origin::Operator | Origin::Heartbeat => true,
            Origin::HbAck => self.settings().js8_hb_ack,
            Origin::AutoReply => self.settings().js8_autoreply,
            Origin::Relay => self.settings().js8_relay,
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
        // Wall-clock watchdog, RE-APPLIED for every origin but Heartbeat (G2: the HB is a
        // beacon by design — `plan.beacon` below carries the exemption to commit_tx).
        if tf.origin != Origin::Heartbeat && self.js8_wall_clock_trips() {
            return None;
        }
        // f0: the operator's TX offset, or the station's HB sub-band pick — an AUDIO
        // offset only. A pick outside 500–1000 Hz cannot come from a correct station;
        // fall back to the operator's offset rather than trust it.
        let f0 = match tf.freq_hint {
            FreqHint::Dial => self.tx_offset_hz(),
            FreqHint::HbSubband(f) if (500.0..=1000.0).contains(&f) => f,
            FreqHint::HbSubband(_) => self.tx_offset_hz(),
        };
        let beacon = tf.origin == Origin::Heartbeat;
        // Book the over (station bookkeeping: Last sent, HB timer, idle counter; own-TX
        // row; activity row; ALL.TXT Tx line) — plan time, on the period-start axis.
        self.js8_station.note_tx_done(&tf, period_start_ms);
        self.js8_note_tx_done(&tf.display, period_start_ms);
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

#[cfg(test)]
mod tests {
    use super::*;
    use ::js8::proto::callsign::CallRef;
    use ::js8::proto::command::Command;
    use ::js8::proto::frame::encode_frame;
    use ::js8::{Frame, I3};

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
        assert_eq!(st.rx_speeds, 15);
        assert!(
            !st.armed.autoreply && !st.armed.relay && !st.armed.hb_ack && !st.armed.hb,
            "latch down at launch → nothing is armed"
        );
        assert!(st.queue.is_empty() && st.pending_reply.is_none() && st.activity.is_empty());
    }

    /// The RX chain end to end: `Decode.raw` → `RawDecode` → `Reassembler` → `Station`, with the
    /// activity pane and the heard list populated — and the receive-only override holding: a
    /// query addressed to me computes NO reply (empty outbox, no countdown), because a station
    /// with no way to drain an outbox must not build one.
    #[test]
    fn js8_ingest_feeds_the_station_and_never_queues_a_reply_in_the_rx_only_build() {
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
            "receive-only: the station must not queue an autoreply"
        );
        assert!(st.pending_reply.is_none());
        assert!(
            !e.js8_station.config().autoreply,
            "the B5 override forces autoreply OFF in the station"
        );
        assert!(
            e.settings().js8_autoreply,
            "…while the persisted switch keeps JS8Call's default"
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

    /// A store-and-forward `MSG TO:` lands in the inbox as `Store`, the journal is written
    /// the moment it changes, and a fresh engine restores it — the `pending_msgs.json`
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
        assert!(path.exists(), "the journal is written on InboxChanged");
        let id = st.inbox[0].id;
        e.js8_inbox_mark(id, InboxState::Read).unwrap();
        assert!(e.js8_inbox_mark(id + 1000, InboxState::Read).is_err());

        let mut fresh = Engine::new("KD9TAW", "EN52", 0);
        fresh.set_js8_journal_path(path.clone());
        fresh.js8_load_journal(&std::fs::read_to_string(&path).unwrap());
        let st = fresh.js8_state();
        assert_eq!(st.inbox.len(), 1);
        assert_eq!(st.inbox[0].state, InboxState::Read);
        fresh.js8_inbox_delete(id).unwrap();
        assert!(fresh.js8_state().inbox.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A speed change re-points the decoder at the new window and the station at the new
    /// period, refuses a bad index, and never touches the latch.
    #[test]
    fn js8_set_speed_rebuilds_the_kind_and_refuses_a_bad_index() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        assert!(e.js8_set_speed(4).is_err());
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
        assert!(!e.tx_enabled());
    }

    /// Every transmit verb refuses in this build and says so in `last_error`; nothing keys.
    #[test]
    fn js8_tx_verbs_refuse_in_the_receive_only_build() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        assert!(e.js8_send(None, "HELLO".to_string()).is_err());
        assert!(e
            .js8_send_command("KD2UWR".to_string(), 0, String::new())
            .is_err());
        assert!(e.js8_call_cq(0).is_err());
        assert!(e.js8_arm(Js8Switch::Hb, true).is_err());
        assert!(e.js8_state().last_error.is_some());
        assert!(!e.js8_hb_on);
        assert!(e.js8_state().queue.is_empty());
        e.js8_cancel();
        e.js8_drop_queue();
        e.js8_halt_clear();
        for slot in 0..4 {
            assert!(e.poll_tx(slot).is_empty());
        }
    }

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
}
