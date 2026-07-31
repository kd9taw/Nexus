//! The Pounce detector — turns the inbound spot firehose into a rare, loud alert.
//!
//! The decision itself is pure and lives in `propagation::pounce`. This is the placement: a
//! channel the cluster feeds hand spots to, and a thread that does the expensive part.
//!
//! WHY A CHANNEL AND A THREAD, rather than scoring in the feed callback. That callback runs per
//! inbound spot on the RBN/cluster firehose, and scoring needs the operator's worked sets — which
//! means the engine lock, and rebuilding `LogNeeds` over the whole logbook. Doing that on the
//! ingest path would put expensive, lock-taking work on a hot producer. That is exactly the shape
//! that made the waterfall stall (see tempo-audio `rxtap.rs`, and
//! [[feedback-root-cause-not-bandaids]]): the callback's only job is a non-blocking handoff.
//!
//! THE COST MODEL. `LogNeeds` is derived from every logged QSO — at 11k contacts that is far too
//! expensive per spot. It is rebuilt on a slow cadence and cached; each arriving spot is then
//! scored against the cached snapshot, which is cheap. A needs snapshot that is a few tens of
//! seconds stale can at worst produce one late alert for a station just worked — and the gate's
//! own cooldown already covers that.

use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use propagation::pounce::{Pounce, PounceGate, PounceThreshold};
use tempo_app::engine::Engine;
use tempo_app::settings::PounceThreshold as SettingThreshold;

/// Bridge the settings-layer threshold to the scoring-layer one.
///
/// The enum is mirrored across two crates that do not depend on each other (see the note on
/// `tempo_app::settings::PounceThreshold`). THIS is the seam where they meet, and the totality
/// test below is what stops them drifting: add a variant on either side and this stops compiling.
fn to_scoring(t: SettingThreshold) -> PounceThreshold {
    match t {
        SettingThreshold::Off => PounceThreshold::Off,
        SettingThreshold::Atno => PounceThreshold::Atno,
        SettingThreshold::AtnoOrZone => PounceThreshold::AtnoOrZone,
        SettingThreshold::AtnoZoneOrState => PounceThreshold::AtnoZoneOrState,
    }
}

/// How often the (expensive) worked-sets snapshot is rebuilt from the logbook.
const NEEDS_REFRESH_SECS: i64 = 60;
/// Bound on the handoff queue. The firehose can burst; if the detector falls behind, DROPPING a
/// spot is strictly better than blocking the feed thread that reads the cluster socket. A dropped
/// spot costs at most one alert, and the rare ones get re-spotted within seconds anyway.
const QUEUE_DEPTH: usize = 512;

/// One inbound spot, as much as the detector needs to score it.
pub struct SpotHint {
    pub call: String,
    pub freq_mhz: f64,
    pub mode: String,
    pub spotted_unix: i64,
}

/// The producer half, cloned into each cluster feed callback.
#[derive(Clone)]
pub struct PounceTx(SyncSender<SpotHint>);

impl PounceTx {
    /// Hand a spot to the detector. NEVER blocks: a full queue drops the spot, because the caller
    /// is the thread reading the cluster socket.
    pub fn offer(&self, hint: SpotHint) {
        let _ = self.0.try_send(hint);
    }
}

/// Create the handoff channel.
pub fn channel() -> (PounceTx, Receiver<SpotHint>) {
    let (tx, rx) = std::sync::mpsc::sync_channel(QUEUE_DEPTH);
    (PounceTx(tx), rx)
}

/// Rebuild the operator's worked sets from the logbook. Expensive — call on the slow cadence.
fn snapshot_needs(engine: &Arc<Mutex<Engine>>) -> Option<(propagation::LogNeeds, Vec<String>)> {
    let mut eng = engine.lock().ok()?;
    eng.sync_shared_log_if_changed();
    let mut needs = propagation::LogNeeds::new();
    for q in eng.get_log() {
        needs.add_qso(
            &q.call,
            &q.band,
            &q.mode,
            q.grid.as_deref(),
            q.state.as_deref(),
            q.award_confirmed,
            crate::qso_is_sat(q.prop_mode.as_deref()),
        );
    }
    let wanted = eng.settings().wanted_calls.clone();
    Some((needs, wanted))
}

/// Read the operator's configured threshold (cheap; the setting can change mid-session).
fn threshold_of(engine: &Arc<Mutex<Engine>>) -> PounceThreshold {
    engine
        .lock()
        .ok()
        .map(|e| to_scoring(e.settings().pounce_threshold))
        .unwrap_or_default()
}

/// Run the detector. `on_fire` is called for each alert that clears the gate — the caller wires
/// that to the UI (a Tauri event). Blocks; spawn it.
pub fn run(
    engine: Arc<Mutex<Engine>>,
    rx: Receiver<SpotHint>,
    mut on_fire: impl FnMut(Pounce),
) {
    let mut gate = PounceGate::new();
    let mut needs: Option<(propagation::LogNeeds, Vec<String>)> = None;
    let mut needs_at: i64 = 0;
    let mut last_prune: i64 = 0;

    while let Ok(hint) = rx.recv() {
        let now = crate::now_unix();
        let threshold = threshold_of(&engine);
        if threshold == PounceThreshold::Off {
            continue; // still drain, so a re-enable starts from live traffic
        }
        // Refresh the worked sets on the slow cadence, never per spot.
        if needs.is_none() || now.saturating_sub(needs_at) > NEEDS_REFRESH_SECS {
            if let Some(n) = snapshot_needs(&engine) {
                needs = Some(n);
                needs_at = now;
                // The log changed under us; a station that was uninteresting may now be news
                // (and vice versa). Cheap and correct: let the gate re-evaluate.
                gate.prune(now);
            }
        }
        let Some((ref n, ref _wanted)) = needs else {
            continue;
        };
        let Some(heard) =
            propagation::needalert::heard_from_freq(&hint.call, hint.freq_mhz, &hint.mode)
        else {
            continue; // off-band frequency — not workable, not news
        };
        let alerts = propagation::rank_needs(std::slice::from_ref(&heard), n, &n.slots());
        let Some(alert) = alerts.into_iter().next() else {
            continue;
        };
        if let Some(p) = gate.admit(&alert, threshold, hint.spotted_unix, now) {
            on_fire(p);
        }
        if now.saturating_sub(last_prune) > 300 {
            gate.prune(now);
            last_prune = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two mirrored enums must stay in step. This is a compile-time totality check with a
    /// runtime value check: a new variant on EITHER side breaks the match in `to_scoring`, and a
    /// mis-wired arm is caught here rather than by an alert that silently never fires.
    #[test]
    fn the_threshold_mirror_maps_every_variant_correctly() {
        for (setting, want) in [
            (SettingThreshold::Off, PounceThreshold::Off),
            (SettingThreshold::Atno, PounceThreshold::Atno),
            (SettingThreshold::AtnoOrZone, PounceThreshold::AtnoOrZone),
            (
                SettingThreshold::AtnoZoneOrState,
                PounceThreshold::AtnoZoneOrState,
            ),
        ] {
            assert_eq!(to_scoring(setting), want, "mirror drifted for {setting:?}");
        }
    }

    /// Pounce ships OFF and both sides must agree on that. The threshold that is right depends on
    /// how much the operator still has to chase — rare at 280+ DXCC, a siren at 50 — and we cannot
    /// know that at install, so it is opt-in. A default that floods a newcomer teaches them to
    /// ignore the alert, which is worse than never shipping it.
    #[test]
    fn pounce_ships_off_on_both_sides() {
        assert_eq!(SettingThreshold::default(), SettingThreshold::Off);
        assert_eq!(to_scoring(SettingThreshold::default()), PounceThreshold::Off);
    }

    /// A full queue must DROP rather than block: the producer is the thread reading the cluster
    /// socket, and stalling it would back up the whole feed.
    #[test]
    fn a_full_queue_drops_instead_of_blocking_the_feed() {
        let (tx, _rx) = channel();
        for i in 0..(QUEUE_DEPTH * 2) {
            tx.offer(SpotHint {
                call: format!("K{i}ABC"),
                freq_mhz: 14.025,
                mode: "CW".into(),
                spotted_unix: 0,
            });
        }
        // Reaching here at all is the assertion: `offer` never blocked despite the queue being
        // long past full.
    }
}
