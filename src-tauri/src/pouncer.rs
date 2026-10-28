//! The Pounce detector — turns the inbound spot firehose into a rare, loud alert.
//!
//! The decision itself is pure and lives in `propagation::pounce`. This is the placement: a
//! channel the cluster feeds hand spots to, and a thread that does the expensive part.
//!
//! WHY A CHANNEL AND A THREAD, rather than scoring in the feed callback. That callback runs per
//! inbound spot on the RBN/cluster firehose, and scoring needs the operator's worked sets — which
//! means the engine lock, and rebuilding `LogNeeds` over the whole logbook. Doing that on the
//! ingest path would put expensive, lock-taking work on a hot producer. That is exactly the shape
//! that made the waterfall stall (see tempo-audio `rxtap.rs`):
//! the callback's only job is a non-blocking handoff.
//!
//! THE COST MODEL. `LogNeeds` is derived from every logged QSO — at 11k contacts that is far too
//! expensive per spot. It is re-read on a slow cadence from the model every other reader shares
//! (`crate::NeedsKept`: folded again only when the log has moved, with the engine lock released),
//! and each arriving spot is scored against that snapshot, which is cheap. A needs snapshot that
//! is a few tens of seconds stale can at worst produce one late alert for a station just worked —
//! and the gate's own cooldown already covers that.

use std::collections::VecDeque;
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
/// How many raised alerts are kept for Remote browsers, newest last. A browser polls this list
/// and notifies for entries it has not seen, so it only has to outlast one poll; it is bounded so
/// a long session cannot grow it.
pub const RECENT: usize = 64;

/// The alerts the detector raised, oldest first — exactly what it passed to `on_fire`. Read by the
/// Remote `pounce` query; nothing reads it back into the decision.
pub type SharedRecent = Arc<Mutex<VecDeque<Pounce>>>;

fn remember(recent: &SharedRecent, alert: &Pounce) {
    let mut alerts = recent.lock().unwrap_or_else(|e| e.into_inner());
    while alerts.len() >= RECENT {
        alerts.pop_front();
    }
    alerts.push_back(alert.clone());
}

/// One inbound spot, as much as the detector needs to score it.
pub struct SpotHint {
    pub call: String,
    pub freq_mhz: f64,
    pub mode: String,
    pub spotted_unix: i64,
    /// Who reported it: the spotter, then any corroborators the spot already carries. Read only
    /// by the "Spotted from" filter ([`SpotterFilter`]).
    pub voices: Vec<String>,
    /// The IOTA island group the spot's comment names, when it names exactly one
    /// ([`propagation::iota::in_comment`]) — the New IOTA need's heard side.
    pub iota: Option<String>,
}

/// THE SPOTS BOARD'S "SPOTTED FROM" CHIPS, WHICH POUNCE FOLLOWS (#174). The operator who has
/// narrowed the board to spots heard from France should not be called to the radio by a spot only
/// a US skimmer heard. The board sends its chips here whenever they change (`set_pounce_spotter_filter`);
/// nothing is saved, as the chips themselves are kept for the session only.
///
/// The board's own rule, so the two cannot disagree: both lists empty admits everything; otherwise
/// a spot is kept when ANY of its voices is on a chosen continent (if any is chosen) and in a
/// chosen country (if any is chosen), each voice placed by `spotter_origin`, the resolver that
/// made the chips. A spot none of whose voices can be placed matches nothing. Applied BEFORE the
/// gate, so a filtered-out spot spends none of the station's cooldown: when the same station is
/// then heard from France, Pounce fires.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct SpotterFilter {
    /// Continent codes as the chips carry them (`EU`, `NA`, ...).
    #[serde(default)]
    pub conts: Vec<String>,
    /// DXCC entity names as the chips carry them.
    #[serde(default)]
    pub entities: Vec<String>,
}

impl SpotterFilter {
    pub fn admits(&self, voices: &[String]) -> bool {
        if self.conts.is_empty() && self.entities.is_empty() {
            return true;
        }
        let origins: Vec<(&str, &str)> = voices
            .iter()
            .filter_map(|v| propagation::needalert::spotter_origin(v))
            .collect();
        (self.conts.is_empty()
            || origins
                .iter()
                .any(|(c, _)| self.conts.iter().any(|x| x == c)))
            && (self.entities.is_empty()
                || origins
                    .iter()
                    .any(|(_, e)| self.entities.iter().any(|x| x == e)))
    }
}

/// The filter the detector reads per spot and the board's command writes. Its own type, so
/// Tauri's managed state can tell it apart.
#[derive(Clone, Default)]
pub struct SharedSpotterFilter(pub Arc<Mutex<SpotterFilter>>);

impl SharedSpotterFilter {
    pub fn set(&self, filter: SpotterFilter) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = filter;
    }
    fn admits(&self, voices: &[String]) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .admits(voices)
    }
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

/// The operator's worked sets — the needs model every reader shares ([`crate::NeedsKept`]),
/// folded again only when the log has moved. Call on the slow cadence.
///
/// Under the engine lock only the freshness check and the kept model or the log's rows; a fold,
/// when one is needed, reads the logbook store after the lock is released.
/// It used to clone every record and fold them all under the lock, every minute a spot came in.
/// `None` when the engine or the store cannot be read: the detector keeps the model it has.
fn snapshot_needs(
    engine: &Arc<Mutex<Engine>>,
    kept: &crate::NeedsKept,
) -> Option<Arc<propagation::LogNeeds>> {
    // Another window's contacts taken in first, with the Engine lock released for every read —
    // not on a poisoned engine, which the capture below skips as it always has.
    if engine.is_poisoned() {
        return None;
    }
    tempo_app::engine::sync_shared_log(engine);
    let capture = {
        let eng = tempo_app::engine::engine_lock_result(engine).ok()?;
        crate::needs_capture(&eng, kept)
    };
    match crate::needs_finish(capture, kept) {
        Ok(needs) => Some(needs),
        Err(e) => {
            tempo_core::applog::warn("pounce", &format!("the needs model was not refreshed: {e}"));
            None
        }
    }
}

/// Read the operator's configured threshold (cheap; the setting can change mid-session).
fn threshold_of(engine: &Arc<Mutex<Engine>>) -> PounceThreshold {
    tempo_app::engine::engine_lock_result(engine)
        .ok()
        .map(|e| to_scoring(e.settings().pounce_threshold))
        .unwrap_or_default()
}

/// Run the detector. `on_fire` is called for each alert that clears the gate — the caller wires
/// that to the UI (a Tauri event). The same alert is first appended to `recent`, so a Remote
/// browser reads exactly what the desktop was told. `ota` is the hunter feed's cache, which puts
/// a live activator in its park's state. `spotted_from` is the Spots board's "Spotted from"
/// chips. Blocks; spawn it.
pub fn run(
    engine: Arc<Mutex<Engine>>,
    needs_kept: crate::NeedsKept,
    ota: crate::SharedOtaSpots,
    rx: Receiver<SpotHint>,
    recent: SharedRecent,
    spotted_from: SharedSpotterFilter,
    mut on_fire: impl FnMut(Pounce),
) {
    let mut gate = PounceGate::new();
    let mut needs: Option<Arc<propagation::LogNeeds>> = None;
    let mut needs_at: i64 = 0;
    let mut last_prune: i64 = 0;

    while let Ok(hint) = rx.recv() {
        let now = crate::now_unix();
        let threshold = threshold_of(&engine);
        if threshold == PounceThreshold::Off {
            continue; // still drain, so a re-enable starts from live traffic
        }
        if !spotted_from.admits(&hint.voices) {
            continue; // heard only from where the operator chose not to look
        }
        // Refresh the worked sets on the slow cadence, never per spot.
        if needs.is_none() || now.saturating_sub(needs_at) > NEEDS_REFRESH_SECS {
            if let Some(n) = snapshot_needs(&engine, &needs_kept) {
                needs = Some(n);
                needs_at = now;
                // The log changed under us; a station that was uninteresting may now be news
                // (and vice versa). Cheap and correct: let the gate re-evaluate.
                gate.prune(now);
            }
        }
        let Some(ref n) = needs else {
            continue;
        };
        let Some(mut heard) =
            propagation::needalert::heard_from_freq(&hint.call, hint.freq_mhz, &hint.mode)
        else {
            continue; // off-band frequency — not workable, not news
        };
        heard.iota = hint.iota.clone();
        // A spot names no state, so the row is placed as the Needed board places the same
        // station: a live activator in its park's state, any other station by the radio's own
        // decodes' resolver. Unplaced, "…or a new US state" never fired on a state. The hunter
        // feed is read per spot, as the board reads it per poll, so the two agree on an activator.
        let live: Vec<propagation::OtaSpot> = ota
            .lock()
            .map(|cache| crate::live_ota_spots(&cache, now).cloned().collect())
            .unwrap_or_default();
        crate::place_heards(
            std::slice::from_mut(&mut heard),
            &live,
            n,
            crate::fcc_state_for_call,
        );
        crate::place_iotas(
            std::slice::from_mut(&mut heard),
            crate::callbook_iota_for_call,
            crate::iota_list::known,
        );
        let alerts = propagation::rank_needs(std::slice::from_ref(&heard), &**n, &n.slots());
        let Some(alert) = alerts.into_iter().next() else {
            continue;
        };
        if let Some(p) = gate.admit(&alert, threshold, hint.spotted_unix, now) {
            remember(&recent, &p);
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
        assert_eq!(
            to_scoring(SettingThreshold::default()),
            PounceThreshold::Off
        );
    }

    /// The Remote list keeps the newest `RECENT` alerts in the order they were raised.
    #[test]
    fn the_remote_list_keeps_the_newest_alerts_in_order() {
        let recent: SharedRecent = Default::default();
        for i in 0..(RECENT + 5) {
            remember(
                &recent,
                &Pounce {
                    call: format!("K{i}ABC"),
                    band: "20m".into(),
                    mode: "CW".into(),
                    freq_mhz: None,
                    tags: Vec::new(),
                    entity: String::new(),
                    at_unix: i as i64,
                },
            );
        }
        let alerts = recent.lock().unwrap();
        assert_eq!(alerts.len(), RECENT);
        assert_eq!(alerts.front().unwrap().call, "K5ABC");
        assert_eq!(alerts.back().unwrap().at_unix, (RECENT + 4) as i64);
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
                voices: Vec::new(),
                iota: None,
            });
        }
        // Reaching here at all is the assertion: `offer` never blocked despite the queue being
        // long past full.
    }

    /// The pounce thread reads the needs model the other readers share: the one the Needed
    /// board and a propagation refetch read, folded once for all of them while the log stands
    /// still. It used to clone and fold the whole log for itself, under the engine lock.
    #[test]
    #[cfg(debug_assertions)]
    fn the_pounce_thread_reads_the_needs_model_every_reader_shares() {
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        engine.lock().unwrap().import_adif(
            "<CALL:5>JA1AA<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260101<TIME_ON:6>010000<EOR>\n",
        );
        let tallies = crate::LogTallies::default();
        crate::LOG_TALLIES.with(|c| c.set(0));
        let pounce = snapshot_needs(&engine, &tallies.needs).expect("the engine is there");
        let board = crate::needs_kept(&engine, &tallies).expect("the log reads");
        let again = snapshot_needs(&engine, &tallies.needs).expect("the engine is there");
        assert!(
            Arc::ptr_eq(&pounce, &board) && Arc::ptr_eq(&board, &again),
            "one model for every reader"
        );
        assert_eq!(crate::LOG_TALLIES.with(|c| c.get()), 1, "folded once");
        assert_eq!(pounce.worked_entities(), 1, "premise: the contact is in it");
    }

    /// What the detector raises for `calls`, each spotted just now on 20 m CW, run through the
    /// detector itself: the log is `adif`, Pounce is set to `threshold`, and the hunter feed has
    /// `feed` live. Each alert as its call, its reasons and its entity.
    fn raised(
        adif: &str,
        threshold: SettingThreshold,
        feed: &[propagation::OtaSpot],
        calls: &[&str],
    ) -> Vec<(String, Vec<propagation::NeedTag>, String)> {
        let spots: Vec<(&str, &str)> = calls.iter().map(|c| (*c, "W3LPL-#")).collect();
        raised_with(adif, threshold, feed, &spots, SpotterFilter::default())
    }

    /// [`raised`], with each spot's spotter given (`(call, spotter)`) and the Spots board's
    /// "Spotted from" chips set to `spotted_from`.
    fn raised_with(
        adif: &str,
        threshold: SettingThreshold,
        feed: &[propagation::OtaSpot],
        spots: &[(&str, &str)],
        spotted_from: SpotterFilter,
    ) -> Vec<(String, Vec<propagation::NeedTag>, String)> {
        let mut engine = Engine::new("KD9TAW", "EN52", 0);
        engine.import_adif(adif);
        let mut settings = engine.settings().clone();
        settings.pounce_threshold = threshold;
        engine.apply_settings(settings);
        let ota: crate::SharedOtaSpots = Default::default();
        ota.lock()
            .unwrap()
            .insert("POTA".into(), (crate::now_unix(), feed.to_vec()));
        let (tx, rx) = channel();
        for (call, spotter) in spots {
            tx.offer(SpotHint {
                call: (*call).into(),
                freq_mhz: 14.025,
                mode: "CW".into(),
                spotted_unix: crate::now_unix(),
                voices: vec![(*spotter).into()],
                iota: None,
            });
        }
        drop(tx);
        let filter = SharedSpotterFilter::default();
        filter.set(spotted_from);
        let mut fired = Vec::new();
        run(
            Arc::new(Mutex::new(engine)),
            Default::default(),
            ota,
            rx,
            Default::default(),
            filter,
            |p| fired.push((p.call, p.tags, p.entity)),
        );
        fired
    }

    /// #174: POUNCE FOLLOWS THE SPOTS BOARD'S "SPOTTED FROM" CHIPS. With France chosen, an
    /// all-time new one heard only by a US skimmer raises nothing, and the same station heard
    /// from France then does: the filtered spot spent none of the gate's cooldown. A continent
    /// chip works the same way, a spotter nobody can place matches nothing, and with no chip
    /// chosen every spot is scored as before.
    #[test]
    fn pounce_fires_only_on_spots_heard_from_where_the_spots_board_is_narrowed() {
        let calls = |fired: Vec<(String, Vec<propagation::NeedTag>, String)>| -> Vec<String> {
            fired.into_iter().map(|(c, ..)| c).collect()
        };
        let france = SpotterFilter {
            conts: Vec::new(),
            entities: vec!["France".into()],
        };
        let spots = [("3Y0J", "W3LPL-#"), ("3Y0J", "F5ABC-#")];
        let fired = raised_with("", SettingThreshold::Atno, &[], &spots, france.clone());
        assert_eq!(calls(fired), vec!["3Y0J"], "fires once, on the French spot");
        let us_only = raised_with("", SettingThreshold::Atno, &[], &spots[..1], france.clone());
        assert!(
            us_only.is_empty(),
            "a US-only spot must not fire: {us_only:?}"
        );

        let europe = SpotterFilter {
            conts: vec!["EU".into()],
            entities: Vec::new(),
        };
        let eu = raised_with(
            "",
            SettingThreshold::Atno,
            &[],
            &[("3Y0J", "DL1ABC")],
            europe.clone(),
        );
        assert_eq!(calls(eu), vec!["3Y0J"]);
        let na = raised_with(
            "",
            SettingThreshold::Atno,
            &[],
            &[("3Y0J", "W3LPL")],
            europe,
        );
        assert!(na.is_empty(), "{na:?}");

        let unplaced = raised_with("", SettingThreshold::Atno, &[], &[("3Y0J", "???")], france);
        assert!(unplaced.is_empty(), "{unplaced:?}");

        // CONTROL: no chip chosen — the US-only spot fires, as it always has.
        let open = raised_with(
            "",
            SettingThreshold::Atno,
            &[],
            &spots[..1],
            SpotterFilter::default(),
        );
        assert_eq!(calls(open), vec!["3Y0J"]);
    }

    /// The filter's rule is the board's: ANY voice placed on a chosen continent AND in a chosen
    /// country, each list ignored while empty.
    #[test]
    fn the_spotter_filter_admits_on_any_voice_as_the_board_does() {
        let f = SpotterFilter {
            conts: vec!["EU".into()],
            entities: vec!["Fed. Rep. of Germany".into()],
        };
        assert!(f.admits(&["W3LPL".into(), "DL1ABC-#".into()]));
        assert!(!f.admits(&["F5ABC".into()]), "EU but not Germany");
        assert!(!f.admits(&[]), "nobody to place");
        assert!(SpotterFilter::default().admits(&[]));
    }

    /// ⭐ "NEW ENTITY, ZONE, OR US STATE" FIRES ON A NEEDED STATE, AND ON NOTHING ELSE. A spot
    /// carries a call and a frequency but no state, and the detector scored its row as it came, so
    /// this threshold could never fire on a state; the gate's own test passed on a New State alert
    /// made by hand. The row is placed now as the Needed board places the same station.
    ///
    /// Alaska, the United States and their zones are worked and confirmed on 20 m CW, so a station
    /// heard there is news only for its state. An Alaskan is in AK, so it fires until AK is worked
    /// on 20 m. An activator is in its park's state, so W1ABC at a Maine park fires and W1XYZ at a
    /// Connecticut park, a state already worked, does not. The zone threshold fires on none of
    /// them. No FCC index is loaded in a test, so no station here has a licence state. 3Y0J, an
    /// all-time new one, fires in every run: the detector scored each spot it stayed quiet on.
    #[test]
    fn the_state_threshold_fires_on_a_needed_state_and_on_nothing_else() {
        use propagation::NeedTag::{NewEntity, NewState, NewZone};
        use SettingThreshold::{AtnoOrZone, AtnoZoneOrState};
        let log = |alaska: &str| {
            format!(
                "<CALL:6>KL7XYZ<BAND:3>20m<MODE:2>CW<QSO_DATE:8>20260101<TIME_ON:6>010000{alaska}\
                 <LOTW_QSL_RCVD:1>Y<EOR>\n\
                 <CALL:4>W1AA<BAND:3>20m<MODE:2>CW<QSO_DATE:8>20260101<TIME_ON:6>020000<STATE:2>CT\
                 <LOTW_QSL_RCVD:1>Y<EOR>\n"
            )
        };
        let activation = |call: &str, reference: &str, state: &str| propagation::OtaSpot {
            program: "POTA".into(),
            reference: reference.into(),
            name: String::new(),
            activator: call.into(),
            freq_khz: 14_025.0,
            mode: "CW".into(),
            spotter: None,
            comment: None,
            grid: None,
            lat: None,
            lon: None,
            spot_time_unix: None,
            states: vec![state.into()],
        };
        let feed = [
            activation("W1ABC", "US-0001", "US-ME"),
            activation("W1XYZ", "US-0002", "US-CT"),
        ];
        let heard = ["3Y0J", "KL7AA", "W1ABC", "W1XYZ"];
        let new_one = (
            "3Y0J".to_string(),
            vec![NewEntity, NewZone],
            "Bouvet".to_string(),
        );
        let new_state =
            |call: &str, entity: &str| (call.to_string(), vec![NewState], entity.to_string());
        assert_eq!(
            [
                raised(&log(""), AtnoZoneOrState, &feed, &heard),
                raised(&log("<STATE:2>AK"), AtnoZoneOrState, &feed, &heard),
                raised(&log(""), AtnoOrZone, &feed, &heard),
            ],
            [
                vec![
                    new_one.clone(),
                    new_state("KL7AA", "Alaska"),
                    new_state("W1ABC", "United States")
                ],
                vec![new_one.clone(), new_state("W1ABC", "United States")],
                vec![new_one],
            ]
        );
    }
}
