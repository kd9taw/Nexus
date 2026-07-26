//! Pounce — the edge-triggered "a new one just appeared" alert.
//!
//! The needed board answers "what is workable right now" on a 30 s poll. That is the wrong shape
//! for a rare one: by the time the board refreshes, a genuinely new DXCC entity may already be
//! buried under a pileup. Pounce fires the INSTANT such a spot first appears in the RBN/cluster
//! firehose, once, loudly.
//!
//! WHERE THIS RUNS. Not on the firehose callback. That callback fires per inbound spot at high
//! rate, and scoring needs the operator's worked sets — taking that lock on the hot path is the
//! same mistake that made the waterfall stall (see tempo-audio `rxtap.rs`). The feed hands each
//! spot off; a detector elsewhere decides. This module is the pure decision, with no I/O and no
//! locks, so it can be tested exhaustively and placed wherever the caller likes.
//!
//! THE DESIGN CONSTRAINT THAT MATTERS. An alert that cries wolf is worse than no alert: the
//! operator learns to ignore it and it becomes noise. At 280+ DXCC an all-time-new entity is
//! genuinely rare, which is why [`PounceThreshold::Atno`] is the default and the wider tiers are
//! opt-in (operator decision, 2026-07-25).

use std::collections::HashMap;

use crate::needalert::{NeedAlert, NeedTag};

/// How rare a spot must be before Pounce shouts about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PounceThreshold {
    /// DEFAULT — no Pounce alerts until the operator opts in.
    ///
    /// Off by default because the right threshold depends on how much the operator still has to
    /// chase, and we cannot know that at install. At 280+ DXCC an ATNO is rare and the alert is
    /// precious; with 50 worked, nearly every DX spot is an all-time-new one and the same setting
    /// is a siren that never stops. An alert that fires constantly gets ignored within a session,
    /// and then it is worse than absent — so the operator turns it ON when they want it.
    #[default]
    Off,
    /// An all-time-new DXCC entity only. The sensible first choice for an established chaser.
    Atno,
    /// ATNO plus a new CQ zone — for zone chasers working 5BWAZ.
    AtnoOrZone,
    /// ATNO, zone, or a new US state. The widest tier; expect it to fire often enough that it
    /// stops being an interrupt and starts being a feed.
    AtnoZoneOrState,
}

impl PounceThreshold {
    /// Does this alert clear the bar? Only the AWARD tags count — `Confirm` (worked, needs a
    /// QSL) is never a Pounce: nothing is getting away.
    fn admits(self, tags: &[NeedTag]) -> bool {
        let has = |t: NeedTag| tags.contains(&t);
        match self {
            PounceThreshold::Off => false,
            PounceThreshold::Atno => has(NeedTag::NewEntity),
            PounceThreshold::AtnoOrZone => has(NeedTag::NewEntity) || has(NeedTag::NewZone),
            PounceThreshold::AtnoZoneOrState => {
                has(NeedTag::NewEntity) || has(NeedTag::NewZone) || has(NeedTag::NewState)
            }
        }
    }
}

/// One fired alert — what the UI needs to shout and to act on.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pounce {
    pub call: String,
    pub band: String,
    pub mode: String,
    pub freq_mhz: Option<f64>,
    /// The reason, most valuable first — drives the banner text and the earcon choice.
    pub tags: Vec<NeedTag>,
    /// DXCC entity name, for the banner ("new one: Bouvet Island").
    pub entity: String,
    /// Unix seconds when it fired.
    pub at_unix: i64,
}

/// Edge detector. Holds only what it needs to answer "is this NEW news?".
///
/// Deliberately NOT a general cache: it remembers what it has SHOUTED about, not what it has
/// seen. A spot that fails the threshold is never recorded, so raising the threshold mid-session
/// cannot silence a station that would now qualify.
pub struct PounceGate {
    /// (call, band, mode) → when we last fired. One shout per slot per cooldown.
    fired: HashMap<(String, String, String), i64>,
    /// Seconds before the same (call, band, mode) may fire again. A DXpedition that works a
    /// pileup for hours must not re-alert every time a skimmer re-spots it.
    cooldown_secs: i64,
    /// A spot older than this when we see it is history, not news — a cluster reconnect can
    /// replay a backlog, and replaying old spots as live alerts is the fastest way to make the
    /// operator distrust the feature.
    freshness_secs: i64,
}

impl Default for PounceGate {
    fn default() -> Self {
        Self::new()
    }
}

impl PounceGate {
    pub fn new() -> Self {
        Self {
            fired: HashMap::new(),
            // An hour: long enough to cover a normal activation without re-shouting, short
            // enough that a station reappearing next session is news again.
            cooldown_secs: 3600,
            // Two minutes. The board's own retention is 20 minutes, which is right for "what is
            // workable" and far too long for "this just appeared".
            freshness_secs: 120,
        }
    }

    /// Test/config hook for the two windows.
    pub fn with_windows(cooldown_secs: i64, freshness_secs: i64) -> Self {
        Self {
            fired: HashMap::new(),
            cooldown_secs,
            freshness_secs,
        }
    }

    /// Decide whether this scored alert is worth shouting about RIGHT NOW.
    ///
    /// `spotted_unix` is when the spot was heard; `now` is the clock. Returns the alert to fire,
    /// or `None`. Firing records the slot, so the immediate re-spot that always follows a rare
    /// one is silent.
    pub fn admit(
        &mut self,
        alert: &NeedAlert,
        threshold: PounceThreshold,
        spotted_unix: i64,
        now: i64,
    ) -> Option<Pounce> {
        if !threshold.admits(&alert.tags) {
            return None;
        }
        // Stale spot: a reconnect backlog is history, not news.
        if spotted_unix > 0 && now.saturating_sub(spotted_unix) > self.freshness_secs {
            return None;
        }
        let key = (
            alert.call.trim().to_uppercase(),
            alert.band.clone(),
            alert.mode.clone(),
        );
        if let Some(&last) = self.fired.get(&key) {
            if now.saturating_sub(last) < self.cooldown_secs {
                return None;
            }
        }
        self.fired.insert(key, now);
        Some(Pounce {
            call: alert.call.trim().to_uppercase(),
            band: alert.band.clone(),
            mode: alert.mode.clone(),
            freq_mhz: alert.freq_mhz,
            tags: alert.tags.clone(),
            entity: alert.entity.clone(),
            at_unix: now,
        })
    }

    /// Forget everything (band change, callsign change, log import — anything that makes the
    /// worked sets different, so a previously-uninteresting station may now be news).
    pub fn reset(&mut self) {
        self.fired.clear();
    }

    /// Drop cooldown entries older than the window, so a long session cannot grow this forever.
    pub fn prune(&mut self, now: i64) {
        let cutoff = now.saturating_sub(self.cooldown_secs);
        self.fired.retain(|_, &mut at| at >= cutoff);
    }

    /// How many slots are currently in cooldown (diagnostics/tests).
    pub fn tracked(&self) -> usize {
        self.fired.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(call: &str, band: &str, mode: &str, tags: Vec<NeedTag>) -> NeedAlert {
        NeedAlert {
            call: call.to_string(),
            band: band.to_string(),
            mode: mode.to_string(),
            freq_mhz: Some(14.025),
            tags,
            entity: "Bouvet Island".to_string(),
            zone: 38,
            priority: 100,
            headline: String::new(),
            admitted_at: None,
            evidence: None,
            grid_rarity: None,
        }
    }

    #[test]
    fn an_atno_fires_once_then_stays_quiet() {
        let mut g = PounceGate::new();
        let a = alert("3Y0X", "20m", "CW", vec![NeedTag::NewEntity]);
        assert!(
            g.admit(&a, PounceThreshold::Atno, 1000, 1000).is_some(),
            "the first sighting of a new one shouts"
        );
        // The RBN re-spots a rare station within seconds, repeatedly. It must not shout again.
        for t in 1001..1100 {
            assert!(
                g.admit(&a, PounceThreshold::Atno, t, t).is_none(),
                "re-spot at {t} must be silent"
            );
        }
    }

    #[test]
    fn the_default_threshold_ignores_everything_but_an_atno() {
        let mut g = PounceGate::new();
        for tags in [
            vec![NeedTag::NewZone],
            vec![NeedTag::NewState],
            vec![NeedTag::NewGrid],
            vec![NeedTag::NewBand],
            vec![NeedTag::NewMode],
            vec![NeedTag::Confirm],
        ] {
            let a = alert("K1ABC", "20m", "CW", tags.clone());
            assert!(
                g.admit(&a, PounceThreshold::Atno, 100, 100).is_none(),
                "{tags:?} is not an ATNO — the default tier must stay quiet"
            );
        }
    }

    #[test]
    fn a_confirmation_need_never_pounces_at_any_tier() {
        // Worked-but-unconfirmed is not urgent: nothing is getting away.
        let mut g = PounceGate::new();
        let a = alert("K1ABC", "20m", "CW", vec![NeedTag::Confirm]);
        for th in [
            PounceThreshold::Atno,
            PounceThreshold::AtnoOrZone,
            PounceThreshold::AtnoZoneOrState,
        ] {
            assert!(
                g.admit(&a, th, 100, 100).is_none(),
                "{th:?} must ignore Confirm"
            );
        }
    }

    #[test]
    fn the_opt_in_tiers_widen_exactly_as_specified() {
        let zone = alert("UA0FZ", "15m", "CW", vec![NeedTag::NewZone]);
        let state = alert("KL7ABC", "15m", "SSB", vec![NeedTag::NewState]);

        assert!(PounceGate::new()
            .admit(&zone, PounceThreshold::AtnoOrZone, 100, 100)
            .is_some());
        assert!(
            PounceGate::new()
                .admit(&state, PounceThreshold::AtnoOrZone, 100, 100)
                .is_none(),
            "the zone tier must NOT admit a state"
        );
        assert!(PounceGate::new()
            .admit(&state, PounceThreshold::AtnoZoneOrState, 100, 100)
            .is_some());
    }

    #[test]
    fn off_means_off() {
        let a = alert("3Y0X", "20m", "CW", vec![NeedTag::NewEntity]);
        assert!(PounceGate::new()
            .admit(&a, PounceThreshold::Off, 100, 100)
            .is_none());
    }

    #[test]
    fn the_same_call_on_a_different_band_or_mode_is_separate_news() {
        // A DXpedition appearing on a NEW band is a genuinely new opportunity.
        let mut g = PounceGate::new();
        assert!(g
            .admit(
                &alert("3Y0X", "20m", "CW", vec![NeedTag::NewEntity]),
                PounceThreshold::Atno,
                100,
                100
            )
            .is_some());
        assert!(
            g.admit(
                &alert("3Y0X", "40m", "CW", vec![NeedTag::NewEntity]),
                PounceThreshold::Atno,
                101,
                101
            )
            .is_some(),
            "a new BAND for the same call is new news"
        );
        assert!(
            g.admit(
                &alert("3Y0X", "20m", "SSB", vec![NeedTag::NewEntity]),
                PounceThreshold::Atno,
                102,
                102
            )
            .is_some(),
            "a new MODE for the same call is new news"
        );
    }

    #[test]
    fn a_replayed_backlog_after_a_reconnect_does_not_shout() {
        // A cluster reconnect can dump a backlog of old spots. Alerting on those is the fastest
        // way to teach the operator to ignore the alert.
        let mut g = PounceGate::new();
        let a = alert("3Y0X", "20m", "CW", vec![NeedTag::NewEntity]);
        assert!(
            g.admit(&a, PounceThreshold::Atno, 1000, 1000 + 600)
                .is_none(),
            "a 10-minute-old spot is history, not news"
        );
        // ...and having stayed quiet, it did not burn the slot: a LIVE sighting still fires.
        assert!(
            g.admit(&a, PounceThreshold::Atno, 1600, 1600).is_some(),
            "the stale spot must not have consumed the one shout"
        );
    }

    #[test]
    fn the_cooldown_expires_so_a_later_session_is_news_again() {
        let mut g = PounceGate::with_windows(60, 120);
        let a = alert("3Y0X", "20m", "CW", vec![NeedTag::NewEntity]);
        assert!(g.admit(&a, PounceThreshold::Atno, 100, 100).is_some());
        assert!(
            g.admit(&a, PounceThreshold::Atno, 150, 150).is_none(),
            "still cooling"
        );
        assert!(
            g.admit(&a, PounceThreshold::Atno, 200, 200).is_some(),
            "past the cooldown it is news again"
        );
    }

    #[test]
    fn a_failed_threshold_is_never_recorded() {
        // Raising the threshold mid-session must not find a station already "used up".
        let mut g = PounceGate::new();
        let zone = alert("UA0FZ", "15m", "CW", vec![NeedTag::NewZone]);
        assert!(g.admit(&zone, PounceThreshold::Atno, 100, 100).is_none());
        assert_eq!(g.tracked(), 0, "a non-firing spot leaves no trace");
        assert!(
            g.admit(&zone, PounceThreshold::AtnoOrZone, 101, 101)
                .is_some(),
            "widening the tier makes it audible immediately"
        );
    }

    #[test]
    fn reset_makes_everything_news_again() {
        // A log import or callsign change rewrites what counts as needed.
        let mut g = PounceGate::new();
        let a = alert("3Y0X", "20m", "CW", vec![NeedTag::NewEntity]);
        assert!(g.admit(&a, PounceThreshold::Atno, 100, 100).is_some());
        assert!(g.admit(&a, PounceThreshold::Atno, 101, 101).is_none());
        g.reset();
        assert!(g.admit(&a, PounceThreshold::Atno, 102, 102).is_some());
    }

    #[test]
    fn prune_bounds_the_cooldown_table() {
        let mut g = PounceGate::with_windows(60, 120);
        for i in 0..500 {
            let a = alert(&format!("K{i}ABC"), "20m", "CW", vec![NeedTag::NewEntity]);
            g.admit(&a, PounceThreshold::Atno, 100, 100);
        }
        assert_eq!(g.tracked(), 500);
        g.prune(1000); // well past the 60 s cooldown
        assert_eq!(g.tracked(), 0, "expired slots are reclaimed");
    }
}
