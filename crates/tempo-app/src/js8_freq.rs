//! JS8 frequencies, the decode filter, QSY to centre, the band schedule and DIRECTED.TXT —
//! JS8Call 3.0.3's Settings ▸ Frequencies, its waterfall controls and its directed-message log,
//! read as the spec (`JS8_UI/mainwindow.cpp`, `JS8_UI/WideGraph.cpp`, `JS8_UI/Configuration.cpp`,
//! `JS8_Mainwindow/processCommandActivity.cpp`, `JS8_Mainwindow/processBufferedActivity.cpp`).
//! A CHILD module of `engine` (declared with `#[path]` there, as `js8.rs` is), so it reaches the
//! engine's private fields without widening any visibility.
//!
//! NOTHING HERE KEYS. Two things here MOVE THE RIG, and both go through the operator's own tune
//! path ([`Engine::set_frequency`]) with every guard it carries (radio routing, the TX halt on a
//! band change, the TX-gate generation bump, split cleared), plus one of their own: neither moves
//! the dial while the station is on the air ([`Engine::on_air`]) or a JS8 message is still going
//! out ([`Engine::js8_message_going_out`]).
//! - **QSY to centre** ([`Engine::js8_qsy_to_centre`]): the operator's press.
//! - **The band schedule** ([`Engine::js8_freq_tick`]): OFF by default. On, it changes the
//!   frequency BY ITSELF, with nobody at the radio, after the 30-second notice JS8Call shows.
//!
//! The rest is receive-side or bookkeeping: the decode filter narrows what the JS8 decoder
//! searches ([`Engine::js8_filter_window`]), the dial last used on JS8 is remembered for "return
//! to the last used frequency" ([`Engine::js8_return_dial`]), and every directed message heard is
//! written to DIRECTED.TXT beside ALL.TXT ([`js8_directed_line`]).
use std::time::{Duration, Instant};

use ::js8::proto::reassembly::{render_directed, Checksum, Message};

use super::Engine;
use crate::dto::{Js8HopNotice, SourceKind, Tier};
use crate::settings::{Js8Filter, Js8Hop};

/// JS8Call's notice before a scheduled change: a 30-second box that changes the frequency when
/// its count runs out (mainwindow.cpp:280-296; `SelfDestructMessageBox::tick` clicks the
/// default button, Ok, at zero).
pub(crate) const JS8_HOP_NOTICE_MS: u64 = 30_000;
/// The JS8 tick runs once a second while JS8 runs and not otherwise, so a gap this long means
/// JS8 stopped in between: what the schedule remembered belonged to the run that ended.
const JS8_SESSION_GAP: Duration = Duration::from_secs(5);
/// DIRECTED.TXT lines waiting for the shell to append them (ALL.TXT's buffer cap).
const JS8_DIRECTED_CAP: usize = 5_000;
/// JS8Call's end-of-transmission mark (`EOTCharacter`, default U+2662, Configuration.cpp:2146).
const JS8_EOT: &str = "\u{2662}";

const JS8_QSY_NOT_JS8: &str = "QSY to centre is a JS8 control: open JS8 first.";
const JS8_QSY_ON_AIR: &str = "Not moved: QSY to centre waits until nothing is transmitting.";
const JS8_QSY_QUEUED: &str =
    "Not moved: a JS8 message is still going out. QSY to centre once it has gone.";
const JS8_QSY_NO_DIAL: &str = "Not moved: the rig's frequency is not known yet.";
const JS8_HOP_GONE: &str = "That scheduled frequency change is no longer waiting.";

/// The schedule's memory, JS8Call's `m_bandHopped` / `m_bandHoppedFreq` (mainwindow.h:936-937)
/// and the change waiting out its notice. Session state, never persisted: JS8Call keeps none of
/// it across a restart either.
#[derive(Debug, Clone, Default)]
pub(crate) struct Js8HopState {
    /// `m_bandHopped`: the dial is where the schedule last put it, or an edit asked for a fresh
    /// look. False once the dial is anywhere else (mainwindow.cpp:5451-5453).
    hopped: bool,
    /// `m_bandHoppedFreq`, Hz: the frequency the schedule last chose, hopped to or cancelled.
    hopped_hz: u64,
    /// The dial the last look saw, Hz: `hopped` ends when the dial CHANGES to anything but
    /// `hopped_hz`, as JS8Call's ends on a rig frequency change, not while the dial stands still.
    dial_hz: u64,
    /// The change waiting out its notice.
    pending: Option<PendingHop>,
    /// The switch and the rows the last look saw: a change "kicks the scheduler", as JS8Call's
    /// `band_schedule_changed` and `auto_switch_bands_changed` do (UI_Constructor.cpp:509-514).
    seen: Option<(bool, Vec<Js8Hop>)>,
    /// The period the schedule last looked in: JS8Call looks once per period (:2777-2779).
    looked_period: Option<u64>,
    /// When the JS8 tick last ran.
    last_tick: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq)]
struct PendingHop {
    hz: u64,
    fires_at_ms: u64,
    description: String,
    /// Due, and held back: the station is on the air or a message is still going out.
    held: bool,
}

/// The schedule row in force at `now_ms` (unix ms, UTC), JS8Call's pick (mainwindow.cpp:213-259):
/// the time of day inside [switch at, until] with both ends counted, or, for a window whose end
/// is before its start, inside either half of it around midnight. Rows are taken in (start, end)
/// order and the LAST one in force wins, which is JS8Call's "in the case of overlapping windows,
/// choose the latest one". A row with no usable dial is never in force.
pub(crate) fn js8_hop_due(rows: &[Js8Hop], now_ms: u64) -> Option<&Js8Hop> {
    const DAY_MS: u64 = 86_400_000;
    let d = now_ms % DAY_MS;
    let mut rows: Vec<&Js8Hop> = rows
        .iter()
        .filter(|r| r.mhz.is_finite() && r.mhz > 0.0)
        .collect();
    rows.sort_by_key(|r| (r.from_min, r.until_min));
    let mut pick = None;
    for r in rows {
        let at = u64::from(r.from_min.min(1439)) * 60_000;
        let until = u64::from(r.until_min.min(1439)) * 60_000;
        let in_range = (at <= d && d <= until) || (until < at && (at <= d || d <= until));
        if in_range {
            pick = Some(r);
        }
    }
    pick
}

/// JS8Call's DIRECTED.TXT line for a closed message (`writeMsgTxt`, mainwindow.cpp:7052-7070,
/// written for every directed command it processes, processCommandActivity.cpp:126-156): the UTC
/// time, the dial in MHz to the hertz, the audio offset in whole hertz, the SNR as JS8Call writes
/// it (`Varicode::formatSNR`), and the message as [`render_directed`] gives it, followed by
/// JS8Call's end-of-transmission mark and a space when the message ended (`ended`: its Last frame
/// came, or JS8Call's 60-second rule closed it). Tab-separated, as JS8Call writes it; the dial
/// with a full stop whatever the locale (JS8Call uses the system locale's mark).
///
/// `None` for what JS8Call never writes: a buffered message whose checksum failed (discarded,
/// processBufferedActivity.cpp:82-91) and a head whose body never came (dropped at 90 s with
/// nothing written, :37-46).
pub(crate) fn js8_directed_line(
    m: &Message,
    ended: bool,
    dial_mhz: f64,
    unix: u64,
) -> Option<String> {
    if m.checksum == Checksum::Bad || (!m.complete && m.text.is_empty()) {
        return None;
    }
    let (y, mo, d, h, mi, s) = tempo_core::logbook::datetime_utc(unix);
    let mut text = render_directed(m);
    if ended {
        text.push(' ');
        text.push_str(JS8_EOT);
        text.push(' ');
    }
    Some(format!(
        "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}\t{dial_mhz:.6}\t{}\t{}\t{text}",
        m.freq_hz as i32,
        ::js8::proto::frame::format_snr(m.snr_db),
    ))
}

impl Engine {
    // ---- the decode filter ----

    /// The audio window the JS8 decoder searches while JS8Call's decode filter is on: centre ±
    /// width/2 (WideGraph.cpp:565-571; `nfa`/`nfb`, mainwindow.cpp:2099-2102), held inside the
    /// modem's 100–4000 Hz passband and never narrower than 50 Hz, so it is always a real window.
    /// `None` with the filter off, and the operator's F Low / F High apply as before. While it is
    /// on it REPLACES them for JS8: in JS8Call the filter is the decoder's only bound.
    pub(crate) fn js8_filter_window(&self) -> Option<(u32, u32)> {
        let f = self.settings.js8_filter;
        if !f.enabled {
            return None;
        }
        let center = i64::from(f.center_hz);
        let half = i64::from(f.width_hz) / 2;
        let lo = (center - half).clamp(100, 3950);
        let hi = (center + half).clamp(lo + 50, 4000);
        Some((lo as u32, hi as u32))
    }

    /// Set the decode filter: the cockpit's waterfall controls and the row menus' "Centre filter
    /// at …Hz". Held to JS8Call's own ranges, centre 120–4880 Hz and width 120–5000 Hz
    /// (WideGraph.ui). The next decode reads it. Receive only: nothing keys or moves.
    pub fn js8_set_filter(&mut self, filter: Js8Filter) {
        self.settings.js8_filter = Js8Filter {
            enabled: filter.enabled,
            center_hz: filter.center_hz.clamp(120, 4880),
            width_hz: filter.width_hz.clamp(120, 5000),
        };
    }

    /// Set the passband centre "QSY to centre" puts the transmit offset on: JS8Call's waterfall
    /// `Center` (500–5000 Hz there), held here to 500–4000 Hz, where an offset can go.
    pub fn js8_set_qsy_center(&mut self, hz: u16) {
        self.settings.js8_qsy_center_hz = hz.clamp(500, 4000);
    }

    // ---- QSY to centre ----

    /// Is a JS8 message still going out: waiting in the station's queue, or part-sent? JS8Call's
    /// `isMessageQueuedForTransmit` (mainwindow.cpp:3264-3266) reads its transmit frame queue; the
    /// station's queue holds a message waiting, the rest of one going out and the CQ repeat's call.
    pub(crate) fn js8_message_going_out(&self) -> bool {
        !self.js8_station.queue().is_empty()
    }

    /// JS8Call's QSY button (WideGraph.cpp:528, mainwindow.cpp:5337-5367): move the dial by the
    /// transmit offset's distance from the passband centre, so the signal sits mid-passband
    /// (narrow filters, rigs with an uneven passband: the user guide's reasons, :237). The
    /// transmit offset goes to the centre and the receive offset moves by the same amount, so both
    /// still point at the same frequency on the air. JS8Call has one offset for both and moves it
    /// to the centre, which is exactly this when they agree. The offsets heard so far move with
    /// the dial, as JS8Call shifts its band and call activity (:5345-5364).
    ///
    /// MOVES THE RIG, through the operator's own tune path ([`Engine::set_frequency`]). Refused
    /// while anything transmits ([`Engine::on_air`]) or a JS8 message is going out (JS8Call's
    /// `changeFreq` refuses a queued message, :5383-5386). Nothing here keys.
    pub fn js8_qsy_to_centre(&mut self) -> Result<(), String> {
        if self.app.tier() != Tier::Js8 {
            return Err(JS8_QSY_NOT_JS8.into());
        }
        if self.on_air() {
            return Err(JS8_QSY_ON_AIR.into());
        }
        if self.js8_message_going_out() {
            return Err(JS8_QSY_QUEUED.into());
        }
        let dial_hz = self.settings.dial_hz() as i64;
        if dial_hz <= 0 {
            return Err(JS8_QSY_NO_DIAL.into());
        }
        let center = f32::from(self.settings.js8_qsy_center_hz);
        let delta = (self.tx_offset_hz - center).round() as i64;
        if delta == 0 {
            return Ok(());
        }
        let mhz = (dial_hz + delta) as f64 / 1_000_000.0;
        let band = crate::bandplan::band_for_dial(mhz).unwrap_or("");
        let rx = self.rx_offset_hz - delta as f32;
        // JS8 is USB, as every row of its plan is.
        self.set_frequency(mhz, band, "USB");
        self.set_tx_offset(center);
        self.set_rx_offset(rx);
        self.js8_shift_offsets(delta as f32);
        tempo_core::applog::info(
            "js8",
            &format!(
                "QSY to centre: dial {:.6} → {mhz:.6} MHz, TX offset {center:.0} Hz",
                dial_hz as f64 / 1_000_000.0
            ),
        );
        Ok(())
    }

    /// The offsets heard so far, after the dial moved up by `delta_hz`: every station is that much
    /// lower in the audio now (mainwindow.cpp:5345-5364). Display only — the activity rows, the
    /// band activity the free-slot pick reads and the heard list; an open receive buffer is keyed
    /// as it was, as JS8Call leaves its message buffers.
    fn js8_shift_offsets(&mut self, delta_hz: f32) {
        for row in &mut self.js8_activity {
            row.freq_hz -= delta_hz;
        }
        let shift = delta_hz.round() as i32;
        self.js8_band_activity = std::mem::take(&mut self.js8_band_activity)
            .into_iter()
            .map(|(offset, at)| (offset - shift, at))
            .collect();
        self.js8_station.shift_offsets(delta_hz);
    }

    // ---- the dial last used on JS8 ----

    /// Remember the dial in use on JS8 for "return to the last used frequency" (JS8Call keeps
    /// `m_lastMonitoredFrequency` as the rig reports it while monitoring, mainwindow.cpp:5443).
    /// Called by the JS8 tick and by every QSY made while JS8 runs. Not while a satellite pass
    /// owns the dial or another program is the source: neither dial is the operator's JS8 one.
    pub(crate) fn js8_remember_dial(&mut self) {
        if self.settings.dial_mhz > 0.0
            && self.source_kind == SourceKind::Native
            && self.sat_dial_owner.is_none()
        {
            self.settings.js8_last_dial_mhz = self.settings.dial_mhz;
        }
    }

    /// The dial entering JS8 returns to: the one last used there, when "Return to the last used
    /// frequency" is on (JS8Call's "Monitor returns to last used frequency", off by default) and a
    /// dial is remembered. Read BEFORE the tier switch retunes to the band's JS8 frequency.
    pub(crate) fn js8_return_dial(&self) -> Option<f64> {
        let mhz = self.settings.js8_last_dial_mhz;
        (self.settings.js8_return_to_last_dial && mhz.is_finite() && mhz > 0.0).then_some(mhz)
    }

    /// Tune back to `mhz` on entering JS8 (`js8_return_dial`'s answer), through the operator's own
    /// tune path. JS8Call does this when Monitor comes on (mainwindow.cpp:1145-1153). Not while
    /// anything transmits, nor while a satellite pass owns the dial or another program is the
    /// source; a dial already there is left alone.
    pub(crate) fn js8_return_to(&mut self, mhz: f64) {
        if self.on_air() || self.source_kind != SourceKind::Native || self.sat_dial_owner.is_some()
        {
            return;
        }
        if (self.settings.dial_mhz - mhz).abs() < 0.000_000_5 {
            return;
        }
        let band = crate::bandplan::band_for_dial(mhz).unwrap_or("");
        self.set_frequency(mhz, band, "USB");
    }

    // ---- the band schedule ----

    /// The JS8 tick's share of this module (`Engine::js8_tick` calls it once a second while JS8
    /// runs): the dial remembered for "return to the last used frequency", then the schedule.
    pub(crate) fn js8_freq_tick(&mut self, now_ms: u64) {
        // The schedule moves the dial for JS8 alone, and the dial remembered is JS8's: on any
        // other tier this is not JS8's tick (the radio loop calls it on the JS8 tier only).
        if self.app.tier() != Tier::Js8 {
            return;
        }
        let now = Instant::now();
        if self
            .js8_hop
            .last_tick
            .is_some_and(|at| now.duration_since(at) > JS8_SESSION_GAP)
        {
            // JS8 was not running: a change announced in the run that ended must not land the
            // moment JS8 comes back.
            self.js8_hop = Js8HopState::default();
        }
        self.js8_hop.last_tick = Some(now);
        self.js8_remember_dial();
        self.js8_schedule_tick(now_ms);
    }

    /// JS8Call's band hop (`tryBandHop`, mainwindow.cpp:197-312), once per period as JS8Call runs
    /// it (:2777-2779). Inert unless the operator turned the schedule on.
    ///
    /// Which row: [`js8_hop_due`]. The override rule is JS8Call's: once the operator moves the dial
    /// off the frequency the schedule chose, or cancels its notice, the same window never moves it
    /// back; the next row with a different frequency does. An edit to the schedule, or the switch
    /// turned on, takes a fresh look.
    ///
    /// A change is announced, then made 30 s later unless cancelled (JS8Call's self-destructing
    /// "Scheduled Frequency Change" box, :280-296): the cockpit shows it with Cancel and Change now
    /// ([`Engine::js8_hop_answer`]). Nobody answering is the unattended case, and it is made.
    ///
    /// ⚠️ MOVES THE RIG with nobody at the radio. Never while the station is on the air or a JS8
    /// message is going out: no look starts then (JS8Call returns at once, :202-205), and a change
    /// falling due then is HELD until both are clear, where JS8Call's box would change regardless.
    fn js8_schedule_tick(&mut self, now_ms: u64) {
        let on = self.settings.js8_band_hop;
        let (was_on, was_rows) = self.js8_hop.seen.clone().unwrap_or((false, Vec::new()));
        if was_rows != self.settings.js8_schedule || (on && !was_on) {
            self.js8_hop.hopped = true;
        }
        self.js8_hop.seen = Some((on, self.settings.js8_schedule.clone()));
        if !on {
            if self.js8_hop.pending.take().is_some() {
                tempo_core::applog::info(
                    "js8",
                    "band schedule switched off: the frequency change waiting was not made",
                );
            }
            return;
        }
        let dial_hz = self.settings.dial_hz();
        if dial_hz != self.js8_hop.dial_hz {
            self.js8_hop.dial_hz = dial_hz;
            if dial_hz != self.js8_hop.hopped_hz {
                self.js8_hop.hopped = false;
            }
        }
        if let Some(due) = self
            .js8_hop
            .pending
            .as_ref()
            .map(|p| now_ms >= p.fires_at_ms)
        {
            if due {
                self.js8_hop_fire();
            }
            return;
        }
        let period_ms = u64::from(self.js8_tx_speed().period_s()) * 1000;
        let period = now_ms / period_ms.max(1);
        if self.js8_hop.looked_period == Some(period) {
            return;
        }
        self.js8_hop.looked_period = Some(period);
        if self.on_air()
            || self.js8_message_going_out()
            || self.source_kind != SourceKind::Native
            || self.sat_dial_owner.is_some()
        {
            return;
        }
        let Some(row) = js8_hop_due(&self.settings.js8_schedule, now_ms) else {
            return;
        };
        let hz = (row.mhz * 1_000_000.0).round() as u64;
        // At most 80 characters, as the editor's field holds.
        let description: String = row.description.trim().chars().take(80).collect();
        let no_override = self.js8_hop.hopped || hz != self.js8_hop.hopped_hz;
        if !no_override || hz == dial_hz {
            return;
        }
        self.js8_hop.hopped = false;
        self.js8_hop.hopped_hz = hz;
        self.js8_hop.pending = Some(PendingHop {
            hz,
            fires_at_ms: now_ms + JS8_HOP_NOTICE_MS,
            description,
            held: false,
        });
        tempo_core::applog::info(
            "js8",
            &format!(
                "band schedule: changing to {:.6} MHz in {} s",
                hz as f64 / 1_000_000.0,
                JS8_HOP_NOTICE_MS / 1000
            ),
        );
    }

    /// Make the change waiting out its notice — unless the station is on the air or a JS8 message
    /// is going out, when it is held for the first tick that finds both clear. Dropped if JS8 is
    /// no longer the mode, a satellite pass took the dial or another program became the source
    /// meanwhile. Whether the dial moved.
    fn js8_hop_fire(&mut self) -> bool {
        if self.app.tier() != Tier::Js8
            || self.source_kind != SourceKind::Native
            || self.sat_dial_owner.is_some()
        {
            self.js8_hop.pending = None;
            return false;
        }
        let blocked = self.on_air() || self.js8_message_going_out();
        let Some(p) = self.js8_hop.pending.as_mut() else {
            return false;
        };
        if blocked {
            if !p.held {
                tempo_core::applog::info(
                    "js8",
                    "band schedule: frequency change held until the transmission ends",
                );
            }
            p.held = true;
            return false;
        }
        let hz = p.hz;
        self.js8_hop.pending = None;
        self.js8_hop.hopped = true;
        self.js8_hop.hopped_hz = hz;
        let mhz = hz as f64 / 1_000_000.0;
        let band = crate::bandplan::band_for_dial(mhz).unwrap_or("");
        // JS8 is USB, as every row of its plan is.
        self.set_frequency(mhz, band, "USB");
        tempo_core::applog::info("js8", &format!("band schedule: changed to {mhz:.6} MHz"));
        true
    }

    /// The operator's answer to a scheduled change's notice: JS8Call's Ok (`go`, change now) or
    /// Cancel (stay; this window does not ask again). A change now that a transmission holds back
    /// waits for it, as an unanswered one would.
    pub fn js8_hop_answer(&mut self, go: bool) -> Result<(), String> {
        let Some(p) = self.js8_hop.pending.as_mut() else {
            return Err(JS8_HOP_GONE.into());
        };
        if !go {
            self.js8_hop.pending = None;
            tempo_core::applog::info("js8", "band schedule: frequency change cancelled");
            return Ok(());
        }
        p.fires_at_ms = 0;
        self.js8_hop_fire();
        Ok(())
    }

    /// The change waiting out its notice, for the cockpit.
    pub(crate) fn js8_hop_notice(&self) -> Option<Js8HopNotice> {
        self.js8_hop.pending.as_ref().map(|p| Js8HopNotice {
            mhz: p.hz as f64 / 1_000_000.0,
            fires_at_ms: p.fires_at_ms,
            description: p.description.clone(),
            held: p.held,
        })
    }

    // ---- DIRECTED.TXT ----

    /// Buffer the DIRECTED.TXT line for a closed message ([`js8_directed_line`]) when the logs are
    /// written: JS8Call's one "Write log files (ALL.TXT, DIRECTED.TXT, etc)" switch
    /// (Configuration.cpp:2302, mainwindow.cpp:7053) is Nexus's ALL.TXT switch.
    pub(crate) fn js8_log_directed(&mut self, m: &Message, ended: bool, now_ms: u64) {
        if !self.settings.write_all_txt {
            return;
        }
        let Some(line) = js8_directed_line(m, ended, self.settings.dial_mhz, now_ms / 1000) else {
            return;
        };
        self.js8_directed_pending.push(line);
        let len = self.js8_directed_pending.len();
        if len > JS8_DIRECTED_CAP {
            self.js8_directed_pending.drain(0..len - JS8_DIRECTED_CAP);
        }
    }

    /// Take the DIRECTED.TXT lines waiting to be appended. The shell writes them beside ALL.TXT,
    /// after releasing the Engine lock, as it writes ALL.TXT's.
    pub fn take_js8_directed_pending(&mut self) -> Vec<String> {
        std::mem::take(&mut self.js8_directed_pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::DecodePass;
    use crate::settings::Settings;
    use ::js8::proto::callsign::CallRef;
    use ::js8::proto::command::Command;
    use ::js8::proto::frame::encode_frame;
    use ::js8::{Frame, I3};
    use modes::Js8Speed;

    /// 2023-11-14 22:13:20 UTC.
    const NOW: u64 = 1_700_000_000_000;

    /// The change waiting out its notice, as the cockpit reads it.
    fn notice(e: &Engine) -> Option<Js8HopNotice> {
        e.js8_state()
            .controls
            .expect("the desktop's state carries controls")
            .hop
    }

    fn js8_engine() -> Engine {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        assert_eq!(e.tier(), Tier::Js8);
        e
    }

    fn mins(hhmm: &str) -> u16 {
        let (h, m) = hhmm.split_once(':').unwrap();
        h.parse::<u16>().unwrap() * 60 + m.parse::<u16>().unwrap()
    }

    fn hop(mhz: f64, from: &str, until: &str, description: &str) -> Js8Hop {
        Js8Hop {
            mhz,
            from_min: mins(from),
            until_min: mins(until),
            description: description.to_string(),
        }
    }

    /// Unix ms of `hh:mm:ss` UTC on NOW's day.
    fn at(hhmmss: &str) -> u64 {
        let p: Vec<u64> = hhmmss.split(':').map(|x| x.parse().unwrap()).collect();
        NOW - NOW % 86_400_000 + (p[0] * 3600 + p[1] * 60 + p[2]) * 1000
    }

    /// A decode row exactly as `Js8Mode::decode_frame` would emit it for `frame`.
    fn row(frame: &Frame, freq: f32, snr: i32) -> modes::Decode {
        let i3 = I3 {
            first: true,
            last: true,
            data: false,
        };
        let word = encode_frame(frame, i3, Js8Speed::Normal).expect("packable");
        modes::Decode {
            message: frame.render(),
            sync: 5.0,
            snr,
            dt: 0.1,
            freq,
            nap: 0,
            qual: 0.9,
            rv: None,
            mode: Some(modes::ModeKind::Js8 {
                speed: Js8Speed::Normal,
            }),
            raw: Some(*word.as_bytes()),
            unchecked: false,
        }
    }

    fn hb(call: &str) -> Frame {
        Frame::Heartbeat {
            call: call.to_string(),
            grid: Some("FN31".to_string()),
            is_cq: false,
            idx: 0,
        }
    }

    // ---- the decode filter ----

    /// JS8Call's filter is the decoder's window: on by default at 1500 ± 1000, so both JS8 jobs
    /// search 500–2500 Hz; off, the operator's F Low / F High (200–2900 by default) are back.
    #[test]
    fn the_decode_filter_is_the_js8_decode_window_and_on_by_default() {
        let mut e = js8_engine();
        assert_eq!(e.settings().js8_filter, Js8Filter::default());
        let multi = e.build_js8_multi_job(Vec::new(), 1);
        assert_eq!((multi.params.nfa, multi.params.nfb), (500.0, 2500.0));
        let boundary = e.build_decode_job(vec![0.0; 16], 1, DecodePass::Boundary);
        assert_eq!((boundary.nfa, boundary.nfb), (500, 2500));

        e.js8_set_filter(Js8Filter {
            enabled: true,
            center_hz: 1200,
            width_hz: 300,
        });
        let multi = e.build_js8_multi_job(Vec::new(), 1);
        assert_eq!((multi.params.nfa, multi.params.nfb), (1050.0, 1350.0));
        let boundary = e.build_decode_job(vec![0.0; 16], 1, DecodePass::Boundary);
        assert_eq!((boundary.nfa, boundary.nfb), (1050, 1350));

        e.js8_set_filter(Js8Filter {
            enabled: false,
            ..e.settings().js8_filter
        });
        let multi = e.build_js8_multi_job(Vec::new(), 1);
        assert_eq!((multi.params.nfa, multi.params.nfb), (200.0, 2900.0));
        let boundary = e.build_decode_job(vec![0.0; 16], 1, DecodePass::Boundary);
        assert_eq!((boundary.nfa, boundary.nfb), (200, 2900));
    }

    /// The filter is JS8's alone: on FT8 the boundary job keeps F Low / F High.
    #[test]
    fn the_js8_decode_filter_leaves_ft8_alone() {
        let mut e = js8_engine();
        e.set_tier(Tier::Ft8);
        assert!(e.settings().js8_filter.enabled);
        let job = e.build_decode_job(vec![0.0; 16], 1, DecodePass::Boundary);
        assert_eq!((job.nfa, job.nfb), (200, 2900));
    }

    /// The window stays a real one inside the modem's passband, whatever the filter holds, and
    /// the setter keeps JS8Call's own ranges (centre 120–4880, width 120–5000).
    #[test]
    fn the_filter_window_and_the_setter_keep_their_ranges() {
        let mut e = js8_engine();
        e.js8_set_filter(Js8Filter {
            enabled: true,
            center_hz: 10,
            width_hz: 10,
        });
        assert_eq!(
            e.settings().js8_filter,
            Js8Filter {
                enabled: true,
                center_hz: 120,
                width_hz: 120,
            }
        );
        assert_eq!(e.js8_filter_window(), Some((100, 180)));
        e.js8_set_filter(Js8Filter {
            enabled: true,
            center_hz: 9_000,
            width_hz: 9_000,
        });
        assert_eq!(e.settings().js8_filter.center_hz, 4880);
        assert_eq!(e.settings().js8_filter.width_hz, 5000);
        assert_eq!(e.js8_filter_window(), Some((2380, 4000)));
        e.js8_set_filter(Js8Filter {
            enabled: true,
            center_hz: 4880,
            width_hz: 120,
        });
        assert_eq!(e.js8_filter_window(), Some((3950, 4000)));
    }

    /// The filter, the QSY centre and the dial last used on JS8 are the engine's: a Settings
    /// form loaded before they changed does not put the old ones back.
    #[test]
    fn a_stale_form_save_keeps_the_live_filter_centre_and_last_dial() {
        let mut e = js8_engine();
        let stale = e.settings().clone();
        e.js8_set_filter(Js8Filter {
            enabled: false,
            center_hz: 1800,
            width_hz: 500,
        });
        e.js8_set_qsy_center(1700);
        e.set_frequency(7.095, "40m", "USB");
        assert_eq!(e.settings().js8_last_dial_mhz, 7.095);
        e.apply_settings(stale);
        assert_eq!(
            e.settings().js8_filter,
            Js8Filter {
                enabled: false,
                center_hz: 1800,
                width_hz: 500,
            }
        );
        assert_eq!(e.settings().js8_qsy_center_hz, 1700);
        assert_eq!(e.settings().js8_last_dial_mhz, 7.095);
    }

    // ---- QSY to centre ----

    /// The dial moves by the TX offset's distance from the centre, the TX offset lands on the
    /// centre and RX keeps its station; what was heard moves with the dial.
    #[test]
    fn qsy_to_centre_moves_the_dial_and_both_offsets_by_value() {
        let mut e = js8_engine();
        assert_eq!(e.settings().dial_hz(), 14_078_000);
        e.js8_ingest(&[row(&hb("W1AW"), 2100.0, -7)], 1);
        e.set_tx_offset(2100.0);
        e.set_rx_offset(2050.0);
        e.js8_qsy_to_centre().unwrap();
        assert_eq!(e.settings().dial_hz(), 14_078_600);
        assert_eq!(e.settings().band, "20m");
        assert_eq!(e.tx_offset_hz(), 1500.0);
        assert_eq!(e.rx_offset_hz(), 1450.0);
        let state = e.js8_state();
        assert_eq!(state.activity.last().unwrap().freq_hz, 1500.0);
        assert_eq!(state.stations[0].freq_hz, 1500.0);
        assert!(e.js8_band_activity.contains_key(&1500));
        assert!(!e.js8_band_activity.contains_key(&2100));
        // At the centre already: nothing moves.
        e.js8_qsy_to_centre().unwrap();
        assert_eq!(e.settings().dial_hz(), 14_078_600);
        // A centre of the operator's own.
        e.js8_set_qsy_center(1000);
        e.js8_qsy_to_centre().unwrap();
        assert_eq!(e.settings().dial_hz(), 14_079_100);
        assert_eq!(e.tx_offset_hz(), 1000.0);
    }

    /// Never while transmitting: refused, and nothing moved.
    #[test]
    fn qsy_to_centre_is_refused_while_transmitting() {
        let mut e = js8_engine();
        e.set_tx_offset(2100.0);
        e.app.set_transmitting(true);
        assert_eq!(e.js8_qsy_to_centre(), Err(JS8_QSY_ON_AIR.to_string()));
        assert_eq!(e.settings().dial_hz(), 14_078_000);
        assert_eq!(e.tx_offset_hz(), 2100.0);
    }

    /// Never with a JS8 message going out: refused, and nothing moved.
    #[test]
    fn qsy_to_centre_is_refused_while_a_message_is_going_out() {
        let mut e = js8_engine();
        e.set_tx_offset(2100.0);
        e.js8_send(None, "HELLO WORLD".into()).unwrap();
        assert!(e.js8_message_going_out());
        assert_eq!(e.js8_qsy_to_centre(), Err(JS8_QSY_QUEUED.to_string()));
        assert_eq!(e.settings().dial_hz(), 14_078_000);
        assert_eq!(e.tx_offset_hz(), 2100.0);
    }

    /// A JS8 control: on FT8 it moves nothing.
    #[test]
    fn qsy_to_centre_is_refused_off_js8() {
        let mut e = js8_engine();
        e.set_tier(Tier::Ft8);
        let dial = e.settings().dial_hz();
        e.set_tx_offset(2100.0);
        assert_eq!(e.js8_qsy_to_centre(), Err(JS8_QSY_NOT_JS8.to_string()));
        assert_eq!(e.settings().dial_hz(), dial);
    }

    // ---- the dial last used on JS8 ----

    /// Off (the default), entering JS8 tunes the band's JS8 frequency as before; on, it goes
    /// back to the dial last used on JS8.
    #[test]
    fn entering_js8_returns_to_the_last_dial_only_when_asked() {
        let mut e = js8_engine();
        e.set_frequency(7.095, "40m", "USB");
        assert_eq!(e.settings().js8_last_dial_mhz, 7.095);
        e.set_tier(Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB");
        assert_eq!(
            e.settings().js8_last_dial_mhz,
            7.095,
            "a QSY made on FT8 is not JS8's dial"
        );
        e.js8_enter();
        assert_eq!(
            e.settings().dial_hz(),
            14_078_000,
            "off: the band's JS8 frequency"
        );

        e.set_tier(Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB");
        e.settings.js8_last_dial_mhz = 7.095;
        e.settings.js8_return_to_last_dial = true;
        e.js8_enter();
        assert_eq!(
            e.settings().dial_hz(),
            7_095_000,
            "on: the dial last used on JS8"
        );
        assert_eq!(e.settings().band, "40m");
    }

    /// The JS8 tick keeps the dial in use (a knob turn reaches settings without a QSY verb).
    #[test]
    fn the_js8_tick_remembers_the_dial() {
        let mut e = js8_engine();
        e.settings.dial_mhz = 7.1;
        e.js8_tick(NOW);
        assert_eq!(e.settings().js8_last_dial_mhz, 7.1);
    }

    // ---- the band schedule ----

    /// JS8Call's window rules, by value: both ends count, a window ending before it starts runs
    /// over midnight, and the last row in force in (start, end) order wins.
    #[test]
    fn the_schedule_picks_by_js8calls_window_rules() {
        let rows = vec![
            hop(14.078, "12:00", "16:00", "afternoon"),
            hop(7.078, "06:00", "08:00", "morning"),
            hop(3.578, "22:00", "02:00", "night"),
            hop(10.130, "12:00", "13:00", "lunch"),
        ];
        let pick = |t: &str| js8_hop_due(&rows, at(t)).map(|r| r.description.as_str());
        assert_eq!(pick("06:00:00"), Some("morning"));
        assert_eq!(pick("08:00:00"), Some("morning"));
        assert_eq!(pick("08:00:01"), None);
        assert_eq!(
            pick("12:30:00"),
            Some("afternoon"),
            "(12:00,16:00) sorts after (12:00,13:00)"
        );
        assert_eq!(pick("16:00:00"), Some("afternoon"));
        assert_eq!(pick("16:00:01"), None);
        assert_eq!(pick("23:59:59"), Some("night"));
        assert_eq!(pick("00:00:00"), Some("night"));
        assert_eq!(pick("02:00:00"), Some("night"));
        assert_eq!(pick("02:00:01"), None);
    }

    /// OFF by default, and off it moves nothing, whatever rows it holds.
    #[test]
    fn the_schedule_moves_nothing_while_it_is_off() {
        let mut e = js8_engine();
        assert!(!Settings::default().js8_band_hop);
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "")];
        for i in 0..10 {
            e.js8_tick(NOW + i * 15_000);
        }
        assert!(notice(&e).is_none());
        assert_eq!(e.settings().dial_hz(), 14_078_000);
    }

    /// On: the change is announced, made 30 s later through the tune path, and the same window
    /// does not ask again.
    #[test]
    fn the_schedule_announces_then_changes_after_its_notice() {
        let mut e = js8_engine();
        e.settings.js8_band_hop = true;
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "40 m net")];
        e.js8_tick(NOW);
        let announced = notice(&e).expect("announced");
        assert_eq!(announced.mhz, 7.078);
        assert_eq!(announced.fires_at_ms, NOW + 30_000);
        assert_eq!(announced.description, "40 m net");
        assert!(!announced.held);
        assert_eq!(
            e.settings().dial_hz(),
            14_078_000,
            "nothing moves during the notice"
        );
        e.js8_tick(NOW + 29_999);
        assert_eq!(e.settings().dial_hz(), 14_078_000);
        e.js8_tick(NOW + 30_000);
        assert_eq!(e.settings().dial_hz(), 7_078_000);
        assert_eq!(e.settings().band, "40m");
        assert!(notice(&e).is_none());
        for i in 1..10 {
            e.js8_tick(NOW + 30_000 + i * 15_000);
        }
        assert!(
            notice(&e).is_none(),
            "already there: nothing more to change"
        );
        assert_eq!(e.settings().dial_hz(), 7_078_000);
    }

    /// Cancel keeps the frequency, and the same window does not ask again; an edit to the
    /// schedule takes a fresh look. Change now makes it at once.
    #[test]
    fn cancel_and_change_now_answer_the_notice() {
        let mut e = js8_engine();
        e.settings.js8_band_hop = true;
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "")];
        e.js8_tick(NOW);
        assert!(notice(&e).is_some());
        e.js8_hop_answer(false).unwrap();
        assert!(notice(&e).is_none());
        for i in 1..10 {
            e.js8_tick(NOW + i * 15_000);
        }
        assert!(
            notice(&e).is_none(),
            "a cancelled window does not ask again"
        );
        assert_eq!(e.settings().dial_hz(), 14_078_000);
        assert_eq!(
            e.js8_hop_answer(true),
            Err(JS8_HOP_GONE.to_string()),
            "nothing waits"
        );

        e.settings
            .js8_schedule
            .push(hop(28.078, "06:00", "07:00", ""));
        e.js8_tick(NOW + 200_000);
        assert!(notice(&e).is_some(), "an edit takes a fresh look");
        e.js8_hop_answer(true).unwrap();
        assert_eq!(e.settings().dial_hz(), 7_078_000, "change now: at once");
        assert!(notice(&e).is_none());
    }

    /// The operator's own move is not undone by the window that moved them.
    #[test]
    fn the_same_window_never_overrides_the_operators_move() {
        let mut e = js8_engine();
        e.settings.js8_band_hop = true;
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "")];
        e.js8_tick(NOW);
        e.js8_tick(NOW + 30_000);
        assert_eq!(e.settings().dial_hz(), 7_078_000);
        e.set_frequency(7.095, "40m", "USB");
        for i in 1..20 {
            e.js8_tick(NOW + 30_000 + i * 15_000);
        }
        assert!(notice(&e).is_none());
        assert_eq!(e.settings().dial_hz(), 7_095_000);
    }

    /// No change starts while the station is on the air or a message is going out.
    #[test]
    fn no_scheduled_change_starts_on_the_air_or_with_a_message_going_out() {
        let mut e = js8_engine();
        e.settings.js8_band_hop = true;
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "")];
        e.app.set_transmitting(true);
        e.js8_tick(NOW);
        assert!(notice(&e).is_none(), "on the air: no look");
        e.app.set_transmitting(false);
        e.set_tx_enabled(true);
        e.js8_send(None, "HELLO WORLD".into()).unwrap();
        e.js8_tick(NOW + 15_000);
        assert!(e.js8_message_going_out());
        assert!(notice(&e).is_none(), "a message going out: no look");
        e.js8_drop_queue();
        e.js8_tick(NOW + 30_000);
        assert!(notice(&e).is_some(), "clear: the look happens");
        assert_eq!(e.settings().dial_hz(), 14_078_000);
    }

    /// A change falling due while the station transmits waits, held, and is made once it stops.
    #[test]
    fn a_due_change_is_held_while_transmitting_and_made_when_clear() {
        let mut e = js8_engine();
        e.settings.js8_band_hop = true;
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "")];
        e.js8_tick(NOW);
        e.app.set_transmitting(true);
        e.js8_tick(NOW + 30_000);
        assert_eq!(
            e.settings().dial_hz(),
            14_078_000,
            "never while transmitting"
        );
        assert!(notice(&e).expect("still waiting").held);
        e.js8_hop_answer(true).unwrap();
        assert_eq!(e.settings().dial_hz(), 14_078_000, "change now waits too");
        e.app.set_transmitting(false);
        e.js8_tick(NOW + 31_000);
        assert_eq!(e.settings().dial_hz(), 7_078_000);
    }

    /// …and while a message is going out.
    #[test]
    fn a_due_change_is_held_while_a_message_is_going_out() {
        let mut e = js8_engine();
        e.settings.js8_band_hop = true;
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "")];
        e.js8_tick(NOW);
        e.set_tx_enabled(true);
        e.js8_send(None, "HELLO WORLD".into()).unwrap();
        e.js8_tick(NOW + 30_000);
        assert_eq!(e.settings().dial_hz(), 14_078_000);
        assert!(notice(&e).expect("still waiting").held);
        e.js8_drop_queue();
        e.js8_tick(NOW + 31_000);
        assert_eq!(e.settings().dial_hz(), 7_078_000);
    }

    /// The schedule is JS8's: on another tier its tick moves nothing, and a change announced on
    /// JS8 is not made there.
    #[test]
    fn the_schedule_moves_nothing_off_js8() {
        let mut e = js8_engine();
        e.settings.js8_band_hop = true;
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "")];
        e.js8_tick(NOW);
        assert!(notice(&e).is_some());
        e.set_tier(Tier::Ft8);
        let dial = e.settings().dial_hz();
        e.js8_tick(NOW + 30_000);
        assert_eq!(e.settings().dial_hz(), dial);
        e.js8_hop_answer(true).unwrap();
        assert_eq!(e.settings().dial_hz(), dial);
        assert!(notice(&e).is_none());
    }

    /// Off JS8 the schedule does not even look: no notice, and the dial on another mode is not
    /// remembered as JS8's.
    #[test]
    fn the_schedule_never_looks_off_js8() {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        assert_ne!(e.tier(), Tier::Js8);
        e.settings.js8_band_hop = true;
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "")];
        let dial = e.settings().dial_hz();
        e.js8_tick(NOW);
        assert!(notice(&e).is_none(), "no look off JS8");
        e.js8_tick(NOW + 30_000);
        assert_eq!(e.settings().dial_hz(), dial);
        assert_eq!(e.settings().js8_last_dial_mhz, 0.0);
    }

    /// The new settings' wire keys, and a file from before them loads JS8Call's defaults: no own
    /// list, the filter on at 1500 ± 1000, the QSY centre 1500, both switches off.
    #[test]
    fn the_js8_frequency_settings_keys_and_an_older_file() {
        let s = Settings {
            js8_frequencies: Some(vec![crate::settings::Js8Freq {
                mhz: 7.095,
                description: "net".into(),
            }]),
            js8_return_to_last_dial: true,
            js8_last_dial_mhz: 7.095,
            js8_filter: Js8Filter {
                enabled: false,
                center_hz: 1200,
                width_hz: 300,
            },
            js8_qsy_center_hz: 1700,
            js8_band_hop: true,
            js8_schedule: vec![hop(7.078, "22:00", "02:30", "night")],
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        for k in [
            r#""js8Frequencies":[{"mhz":7.095,"description":"net"}]"#,
            r#""js8ReturnToLastDial":true"#,
            r#""js8LastDialMhz":7.095"#,
            r#""js8Filter":{"enabled":false,"centerHz":1200,"widthHz":300}"#,
            r#""js8QsyCenterHz":1700"#,
            r#""js8BandHop":true"#,
            r#""js8Schedule":[{"mhz":7.078,"fromMin":1320,"untilMin":150,"description":"night"}]"#,
        ] {
            assert!(json.contains(k), "missing {k}");
        }
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.js8_frequencies, s.js8_frequencies);
        assert_eq!(back.js8_schedule, s.js8_schedule);
        assert_eq!(back.js8_filter, s.js8_filter);

        let old: Settings = serde_json::from_str(r#"{"mycall":"KD9TAW"}"#).unwrap();
        assert_eq!(old.js8_frequencies, None);
        assert!(!old.js8_return_to_last_dial && !old.js8_band_hop);
        assert_eq!(old.js8_last_dial_mhz, 0.0);
        assert_eq!(old.js8_filter, Js8Filter::default());
        assert_eq!(old.js8_qsy_center_hz, 1500);
        assert!(old.js8_schedule.is_empty());
        let partial: Js8Filter = serde_json::from_str(r#"{"widthHz":500}"#).unwrap();
        assert_eq!(
            partial,
            Js8Filter {
                enabled: true,
                center_hz: 1500,
                width_hz: 500,
            }
        );
    }

    /// A gap in the ticks is a new JS8 run: what was announced before does not land on return,
    /// it is announced afresh.
    #[test]
    fn a_new_js8_run_announces_afresh() {
        let mut e = js8_engine();
        e.settings.js8_band_hop = true;
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "")];
        e.js8_tick(NOW);
        e.js8_hop.last_tick = Some(Instant::now() - Duration::from_secs(60));
        e.js8_tick(NOW + 60_000);
        assert_eq!(e.settings().dial_hz(), 14_078_000);
        assert_eq!(
            notice(&e).expect("announced afresh").fires_at_ms,
            NOW + 60_000 + 30_000
        );
    }

    /// Turning the schedule off drops a change waiting out its notice.
    #[test]
    fn switching_the_schedule_off_drops_a_waiting_change() {
        let mut e = js8_engine();
        e.settings.js8_band_hop = true;
        e.settings.js8_schedule = vec![hop(7.078, "22:00", "23:00", "")];
        e.js8_tick(NOW);
        e.settings.js8_band_hop = false;
        e.js8_tick(NOW + 30_000);
        assert!(notice(&e).is_none());
        assert_eq!(e.settings().dial_hz(), 14_078_000);
    }

    // ---- DIRECTED.TXT ----

    /// JS8Call's line, by value: time, dial, offset, SNR and the message, ♢ at the end of one
    /// that ended. Only with the logs on, as JS8Call's one switch.
    #[test]
    fn directed_txt_lines_are_js8calls() {
        let mut e = js8_engine();
        e.settings.write_all_txt = false;
        e.js8_ingest(&[row(&hb("W1AW"), 1210.6, -7)], 1);
        assert!(
            e.take_js8_directed_pending().is_empty(),
            "logs off: nothing"
        );

        e.settings.write_all_txt = true;
        let query = Frame::Directed {
            from: CallRef::Base("KD2UWR".to_string()),
            to: CallRef::Base("K1ABC".to_string()),
            cmd: Command::SnrQuery,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        e.js8_ingest(&[row(&hb("W1AW"), 1210.6, -7), row(&query, 1700.0, 5)], 2);
        let lines = e.take_js8_directed_pending();
        let fields: Vec<Vec<&str>> = lines.iter().map(|l| l.split('\t').collect()).collect();
        assert_eq!(fields.len(), 2, "{lines:?}");
        for f in &fields {
            assert_eq!(f.len(), 5, "{f:?}");
            let ts = f[0].as_bytes();
            assert_eq!(ts.len(), 19, "yyyy-MM-dd hh:mm:ss: {}", f[0]);
            assert_eq!(
                (ts[4], ts[7], ts[10], ts[13], ts[16]),
                (b'-', b'-', b' ', b':', b':')
            );
        }
        assert_eq!(
            fields[0][1..],
            ["14.078000", "1210", "-07", "W1AW: @HB HEARTBEAT \u{2662} "]
        );
        assert_eq!(
            fields[1][1..],
            ["14.078000", "1700", "+05", "KD2UWR: K1ABC SNR? \u{2662} "]
        );
    }

    /// The line from the message alone, the date included; and what JS8Call never writes.
    #[test]
    fn directed_txt_line_shape_and_what_is_never_written() {
        let mut r = ::js8::Reassembler::new();
        let word = encode_frame(
            &hb("W1AW"),
            I3 {
                first: true,
                last: true,
                data: false,
            },
            Js8Speed::Normal,
        )
        .unwrap();
        let rx = ::js8::RawDecode {
            speed: Js8Speed::Normal,
            freq_hz: 999.9,
            dt_s: 0.0,
            snr_db: 12,
            sync: 0.0,
            word,
            nharderrors: 0,
            quality: 1.0,
        };
        let m = r
            .feed(&rx, 0)
            .into_iter()
            .find_map(|ev| match ev {
                ::js8::MessageEvent::Message(m) => Some(m),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            js8_directed_line(&m, true, 7.078, 1_700_000_000).as_deref(),
            Some("2023-11-14 22:13:20\t7.078000\t999\t+12\tW1AW: @HB HEARTBEAT \u{2662} ")
        );
        assert_eq!(
            js8_directed_line(&m, false, 7.078, 1_700_000_000).as_deref(),
            Some("2023-11-14 22:13:20\t7.078000\t999\t+12\tW1AW: @HB HEARTBEAT")
        );
        let bad = Message {
            checksum: Checksum::Bad,
            complete: false,
            text: "HELLO".into(),
            ..m.clone()
        };
        assert_eq!(js8_directed_line(&bad, true, 7.078, 0), None);
        let headless = Message {
            complete: false,
            text: String::new(),
            ..m
        };
        assert_eq!(js8_directed_line(&headless, true, 7.078, 0), None);
    }
}
