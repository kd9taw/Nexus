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
    /// ONE place Settings → `StationConfig` (an associated fn on `&Settings`, so the arm
    /// verbs and `js8_apply_station_config` build it from the SAME source the persisted
    /// switches live in). Identity is uppercased and trimmed the way the wire packs it; the
    /// idle floor of 5 minutes is applied here (0 stays 0 = off); the autoreply countdown is
    /// one period + 2 s at the TRANSMIT speed (`js8_speed`, degraded to Normal on a stale
    /// index — the same rule `js8_tx_speed` applies).
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
            // A group JS8Call will not let be joined (@APRSIS, @JS8NET) is never joined here,
            // whatever put it in Settings: an older Nexus accepted one, and the Remote can write
            // the list. Settings keeps it as written, and the panel refuses a save that adds one.
            groups: s
                .js8_groups
                .iter()
                .map(|g| g.trim().to_ascii_uppercase())
                .filter(|g| !g.is_empty() && ::js8::proto::callsign::may_join_group(g))
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
    /// speed/mask change and by `js8_enter`. B7 removed the B5 receive-only override (the
    /// three lines that forced the station's autoreply/relay/HB-ack OFF): the station now
    /// carries the operator's real switches, so `Js8State.armed` and actual station
    /// behaviour agree. Nothing keys without the TX latch regardless — `plan_js8_tx` gates
    /// on it — so the two-act rule still holds.
    pub(crate) fn js8_apply_station_config(&mut self) {
        let cfg = Self::js8_station_config(&self.settings);
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
        self.js8_start_session();
        self.set_tier(Tier::Js8);
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
        if !add(self.js8_last_error.as_deref().unwrap_or("")) {
            return None;
        }
        Some(self.js8_state())
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
            // Read STRAIGHT off the station — no engine-side mirror to drift. The CQ repeat
            // is stopped from three places the engine never sees (the idle trip, `halt`, and
            // JS8Call's "somebody answered, stop calling" on directed RX), and a mirror would
            // have to be kept in step with each of them.
            cq_on: self.js8_station.cq_on(),
            cq_next_at_ms: self.js8_station.cq_next_ms(),
            cq_interval_min: s.js8_cq_interval_min,
            autoreply: s.js8_autoreply,
            relay: s.js8_relay,
            hb_ack: s.js8_hb_ack,
            armed: Js8Armed {
                autoreply: s.js8_autoreply && live,
                relay: s.js8_relay && live,
                hb_ack: s.js8_hb_ack && live,
                hb: self.js8_hb_on && live,
                cq: self.js8_station.cq_on() && live,
            },
            idle_minutes: self
                .js8_station
                .idle_minutes(tempo_core::timing::now_unix_ms() as u64),
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
        // A countdown that expires while the TX latch is DOWN is cancelled, never carried:
        // arming TX ten minutes later must not fire a reply to a query nobody is waiting
        // for. The countdown was shown the whole time (the cockpit's "would have replied"
        // row) — that is the Auto-arm behaviour spec invariant 11 asks for.
        // No locator in Settings is the same case: JS8Call's `startTx` refuses there too
        // (`ensureCallsignSet`, mainwindow.cpp:5309), through the same `on_stopTxButton_clicked`
        // (:5310), and with TX on it says why (the alert at :5265).
        let no_locator = self.js8_no_usable_locator();
        if !self.tx_enabled() || no_locator {
            let mut dropped = false;
            if let Some(p) = self.js8_station.pending_reply() {
                if p.fires_at_ms <= now_ms {
                    self.js8_station.cancel_pending_reply();
                    dropped = true;
                }
            }
            // …and a heartbeat that falls due is dropped and its interval re-based, so turning
            // TX back on sends nothing: JS8Call's `startTx` finds TX off (`ensureCanTransmit`,
            // mainwindow.cpp:5295) and `on_stopTxButton_clicked` re-bases it (:5304 → :7397).
            dropped |= self.js8_station.drop_due_heartbeat(now_ms);
            if dropped && no_locator && self.tx_enabled() {
                self.js8_last_error = Some(JS8_NO_LOCATOR.to_string());
            }
        }
        let actions = self.js8_station.tick(now_ms);
        self.js8_handle_actions(actions);
    }

    /// Reassembler events → activity rows + station actions.
    fn js8_handle_events(&mut self, events: Vec<MessageEvent>, low_conf: bool, now_ms: u64) {
        for ev in events {
            let row = match &ev {
                MessageEvent::Frame(rx) => {
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
                StationAction::IdleTripped => {
                    // JS8Call parity: HB, autoreply and relay stand down and the queues
                    // drop; `tx_enabled` is UNTOUCHED (it is the operator's latch, not the
                    // station's). The persisted switches are NOT rewritten — `Js8State.armed`
                    // reads `!idle_tripped`, and any operator verb clears the trip. The
                    // cockpit toasts on the rising edge of `idle_tripped` (B7.9).
                    self.js8_hb_on = false;
                    self.js8_station.halt();
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

    // ---- operator verbs (RX side) ----

    /// Change the TRANSMIT speed (0..=3). Re-points the decoder at the new window and the
    /// slot clock at the new period (the audio loop follows `active_slot_secs`), and the
    /// station at the new countdown. The latch is untouched. The command layer persists.
    pub fn js8_set_speed(&mut self, speed_idx: u8) -> Result<(), String> {
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
                "JS8 speed index {speed_idx} is not 0..=3 (Slow/Normal/Fast/Turbo)"
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
    ///
    /// The snapshot is taken here, under the lock, and written on the journals' own thread
    /// ([`tempo_core::journal`]), in the order the station changed: this runs from the radio
    /// loop's decode path and its once-a-second tick, and the fsync is the disk's to finish.
    /// It lands a moment later; the exit path waits for it.
    pub(crate) fn js8_persist(&self) {
        let Some(path) = &self.js8_journal_path else {
            return;
        };
        let Ok(text) = serde_json::to_string(&self.js8_station.snapshot()) else {
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
    fn js8_no_usable_locator(&self) -> bool {
        !::js8::proto::grid::is_station_locator(self.settings.mygrid.trim())
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

    /// Operator send: `to` is a callsign or @group (None = plain text, which compose
    /// prefixes with "MYCALL: " — identity on the wire, spec invariant 10). An operator
    /// verb: on success it restarts the wall-clock watchdog. Never arms TX.
    pub fn js8_send(&mut self, to: Option<String>, text: String) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        let to_ref = match to.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => Some(
                ::js8::proto::callsign::CallRef::parse(s)
                    .ok_or_else(|| format!("{s} is not a callsign or @group JS8 can address"))?,
            ),
            None => None,
        };
        let r = self.js8_identity_set().and_then(|()| {
            self.js8_station
                .send(to_ref.as_ref(), text.trim(), now_ms)
                .map(|_| ())
                .map_err(Self::js8_compose_error)
        });
        self.js8_after_verb(&r);
        r
    }

    /// A directed command from the 32-entry palette (`cmd` = `Command::id`).
    pub fn js8_send_command(&mut self, to: String, cmd: u8, arg: String) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        let to_ref = ::js8::proto::callsign::CallRef::parse(to.trim())
            .ok_or_else(|| format!("{to} is not a callsign or @group JS8 can address"))?;
        let command =
            ::js8::Command::from_id(cmd).ok_or_else(|| format!("unknown JS8 command {cmd}"))?;
        let r = self.js8_identity_set().and_then(|()| {
            self.js8_station
                .send_command(&to_ref, command, arg.trim(), now_ms)
                .map(|_| ())
                .map_err(Self::js8_compose_error)
        });
        self.js8_after_verb(&r);
        r
    }

    /// CQ (`idx` into the CQS table: 0 "CQ CQ CQ" … 7 "CQ"). Counts as Operator origin.
    pub fn js8_call_cq(&mut self, idx: u8) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        let r = self.js8_identity_set().and_then(|()| {
            self.js8_station
                .call_cq(idx, now_ms)
                .map_err(Self::js8_compose_error)
        });
        self.js8_after_verb(&r);
        r
    }

    /// What every operator verb does with its outcome: a success clears `last_error` and
    /// restarts the wall-clock watchdog (an operator act); a refusal is kept for the
    /// cockpit and touches no clock.
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
    /// HB-ack persist to Settings (the Tauri command saves them); Hb is session-only and
    /// never persisted (G3). Turning a switch ON keys nothing by itself — `plan_js8_tx`
    /// still needs `tx_enabled` — but it is an operator verb: it retires an idle trip and
    /// restarts the wall-clock watchdog, exactly as any other operator action does.
    pub fn js8_arm(&mut self, which: Js8Switch, on: bool) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        match which {
            Js8Switch::Autoreply => self.settings.js8_autoreply = on,
            Js8Switch::Relay => self.settings.js8_relay = on,
            Js8Switch::HbAck => self.settings.js8_hb_ack = on,
            Js8Switch::Hb => {}
        }
        let cfg = Self::js8_station_config(&self.settings);
        self.js8_station.set_config(cfg);
        if which == Js8Switch::Hb {
            // `Station::set_hb(true)` schedules the first heartbeat for the next period
            // (JS8Call: nextTransmitCycle + interval; interval 0 = "on demand" = once).
            self.js8_hb_on = on;
            self.js8_station.set_hb(on, now_ms);
        }
        if on {
            self.js8_station.clear_idle_trip();
            self.reset_tx_watchdog();
        }
        Ok(())
    }

    /// Arm/disarm JS8Call's repeating CQ (`idx` = the CQS variant, ignored when disarming).
    /// The SECOND act, session-only and never persisted — the same rule the heartbeat toggle
    /// follows (G3), so a crash or a relaunch can never come back calling CQ. Arming keys
    /// nothing: `plan_js8_tx` re-reads the TX latch every slot, and the idle watchdog, Stop
    /// TX, a tier change and a mode change each cancel the schedule.
    pub fn js8_set_cq_repeat(&mut self, on: bool, idx: u8) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        self.js8_station.set_cq(on, idx, now_ms);
        if on {
            // An operator verb: it retires an idle trip and restarts the wall clock.
            self.js8_station.clear_idle_trip();
            self.reset_tx_watchdog();
        }
        Ok(())
    }

    /// The operator's veto on a pending automatic reply (the visible countdown's Cancel).
    pub fn js8_cancel(&mut self) {
        self.js8_station.cancel_pending_reply();
    }

    /// Sender-class, NOT a stop: empties the outbox and nothing else. The HB schedule, the
    /// TX latch and a frame already on the air are untouched — Stop TX is `halt_tx`.
    pub fn js8_drop_queue(&mut self) {
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

    /// Book a JS8 over at PLAN time — the beacon and QSO arms' rule, and for the same
    /// reason: the plan is the transmit decision, and `commit_tx` refuses only the
    /// microsecond races (JS8's build is pure Rust). Three records: the own-TX row for the
    /// Rx-Frequency feed (`record_own_tx_at`), a `mine` row in the JS8 activity ring the
    /// cockpit's activity pane reads, and — when ALL.TXT is on — the `Tx` line in the
    /// FT/beacon writers' shape (SNR/DT 0, audio = `f0`, into the shared ALL.TXT
    /// buffer with the same 5000-line cap). `now_ms` is the PERIOD START of the over
    /// (slot × period), not the wall clock: alltxt.rs's rule is that only the slot knows
    /// which period an over belongs to. `f0` is the offset the over KEYED at, a heartbeat's
    /// sub-band slot included: JS8Call moves `freq()` there before it shows the frame
    /// (`setFreqOffsetForRestore`, mainwindow.cpp:9681), so all three records sit there.
    pub(crate) fn js8_note_tx_done(&mut self, plan_display: &str, now_ms: u64, f0: f32) {
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
                    f0,
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
    /// Order: identity gate (a callsign for every frame, a locator for a message's first) →
    /// decode-only refusal → one-frame-per-period latch → station outbox → origin gate (the
    /// SECOND act, re-read at plan time every slot) → wall-clock watchdog (all origins but
    /// Heartbeat) → f0 → book → plan. Nothing here moves the dial (invariant 9). Booking is at
    /// plan time, the beacon / QSO arms' rule.
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
            Origin::Operator | Origin::Heartbeat | Origin::CqRepeat => true,
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
        // so an HB-ACK always takes the pick. Its `heartbeat_anywhere` has no Nexus
        // setting, and upstream it defaults off, which is this. A pick outside 500–1000 Hz
        // cannot come from a correct station; fall back to the operator's offset rather
        // than trust it.
        let own_offset = self.tx_offset_hz() <= 1000.0;
        let f0 = match tf.freq_hint {
            FreqHint::Dial => self.tx_offset_hz(),
            FreqHint::HbSubband(_) if tf.origin == Origin::Heartbeat && own_offset => {
                self.tx_offset_hz()
            }
            FreqHint::HbSubband(f) if (500.0..=1000.0).contains(&f) => f,
            FreqHint::HbSubband(_) => self.tx_offset_hz(),
        };
        // Book the over (station bookkeeping: Last sent, idle counter; own-TX row; activity
        // row; ALL.TXT Tx line) — plan time, on the period-start axis. The heartbeat timer
        // re-based itself in `next_frame` when the message's last frame went.
        self.js8_station.note_tx_done(&tf, period_start_ms);
        self.js8_note_tx_done(&tf.display, period_start_ms, f0);
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

    #[test]
    fn bounded_observation_preserves_native_js8_state_at_every_speed() {
        for speed in 0..4 {
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
        assert_eq!(st.rx_speeds, 15);
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
        let last = serde_json::to_string(&e.js8_station.snapshot()).unwrap();
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
    fn hb_engine(grid: &str, hb_min: u16, offset_hz: f32) -> Engine {
        let mut e = Engine::new("KD9TAW", grid, 0);
        e.settings.js8_hb_interval_min = hb_min;
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
            "KD9TAW: @HB HEARTBEAT EN52",
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
    /// at every speed, not the signal's bandwidth, so at Turbo (160 Hz wide) a station heard at
    /// 700 Hz keeps the heartbeat off 700 and not off its neighbours.
    #[test]
    fn a_heard_station_keeps_the_js8_heartbeat_off_fifty_hertz_around_it_at_every_speed() {
        let mut offsets = std::collections::BTreeSet::new();
        for seed in 1..=40u32 {
            let mut e = hb_engine("EN52", 0, 1500.0);
            e.js8_set_speed(3).expect("Turbo");
            let slot = now_unix_secs() / 6 + 1;
            e.js8_ingest(
                &[row(&hb("W1AW", "FN31"), whole(), Js8Speed::Turbo, 700.0)],
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
        e.js8_call_cq(0).expect("CQ queued");
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
            assert_eq!(e.js8_call_cq(0), refused, "{grid:?}: a CQ");
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
        e.js8_set_cq_repeat(true, 0).expect("CQ repeat on");
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
            e.js8_state().pending_reply.is_some(),
            "control: autoreply is on, and a reply counts down"
        );
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let overs = run_js8_loop_from(&mut e, t0, 6 * 60); // through the reply and the heartbeat
        assert!(overs.is_empty(), "nothing keys with no locator: {overs:?}");
        let st = e.js8_state();
        assert!(
            st.pending_reply.is_none(),
            "the reply was dropped when it fell due"
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
}
