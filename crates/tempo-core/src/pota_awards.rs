//! POTA's activator achievements, counted from the log — the numbers the Awards view's POTA
//! activator panel and the map's activated-parks markers show.
//!
//! # Counted from the activations, by the counter's own rule
//!
//! Everything here is folded from [`LoggedActivation`]s: your park × UTC day × the callsign you
//! signed, each with its contacts counted DISTINCT by [`crate::logbook::hot::pota_key`] — the
//! activation strip's "N of 10" rule (call as logged, band, mode; a dupe adds nothing). So a
//! number here and the strip's never disagree about a day. Only POTA activations count: a summit
//! of yours (`MY_SIG` SOTA) is not a park, and a record naming a reference but no program counts
//! only when the reference has a park's shape.
//!
//! **Per callsign.** POTA credits an activation to the account the log is uploaded under, the
//! owner of `STATION_CALLSIGN` ("uniqueness … User account that the log was uploaded under",
//! docs.pota.app/docs/rules.html, read 2026-10-25). Six contacts under one call and six under
//! another at one park on one day are two short activations, never one valid one, so the stats
//! are kept per callsign ([`by_callsign`]). **A portable `/P` is the same call** (operator ruling,
//! the log clean-up's rule, [`without_portable_p`]): `KD9TAW/P` and `KD9TAW` are one account, and
//! their contacts at a park on a day are one activation, each distinct contact once. Only `/P` —
//! `KD9TAW/M` or any other suffix stays a call of its own. The activations are folded for this by
//! [`add`]; the per-activation export still keeps a file per call as signed.
//!
//! # The achievements, as POTA defines them
//!
//! From docs.pota.app/docs/achievements.html and docs.pota.app/docs/rules.html, read 2026-10-25:
//!
//! - **A valid activation**: "a minimum of 10 QSOs from a park in the designated list within a
//!   single UTC day"; "multiple activities at the same park … and the same UTC day count as a
//!   single activation" ([`VALID_QSOS`]).
//! - **Parks activated** (Bronze 10 … Sapphire 75, then 100, 200 … 1000, 1500 … 20000):
//!   "activate … different Parks" ([`PARK_TIERS`]) — a park counts once it has a valid activation.
//! - **Repeat Offender**: "activated 20 times as an activator" at a single park; tiers every 20,
//!   "ad infinitum" ([`REPEAT_STEP`]).
//! - **Kilo**: "Make 1000 QSOs from the same park" ([`KILO_QSOS`]) — every day's distinct contacts
//!   at that park, short days included (they are contacts POTA accepts).
//! - **Rover**: "activating certain numbers of unique parks in a single UTC day" — Warthog 5,
//!   Rhino 10, Cheetah 15, Ostrich 20, Leopard 25, Lion 30, earnable again
//!   ([`ROVER_TIERS`]); each park counted needs its own valid activation that day (the glossary's
//!   "Rove: the act of completing an Activation at a minimum number of Parks in a single UTC day").
//!
//! POTA names no "N-fer" achievement: a two-fer is an activation style (rules, "Multi-loc"), and
//! each of its parks is an activation of its own here. Early/Late Shift, the 6-pack, N1CC and the
//! DX-entity achievements are not counted.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::logbook::cleanup::without_portable_p;
use crate::logbook::{worked_under, Activations, LoggedActivation, QsoRecord};

/// ★ Take the next record of the log (in log order) into `found` as its awards count it: under
/// the call it was worked under with a portable `/P` taken off ([`without_portable_p`]), so a
/// `/P` contact joins the plain call's activation that day. [`by_callsign`] of `found.finish()`
/// is the standing.
pub fn add(found: &mut Activations, r: &QsoRecord) {
    found.add_under(r, worked_under(r).map(without_portable_p));
}

/// Distinct contacts in one UTC day a park needs for a valid activation.
pub const VALID_QSOS: usize = 10;

/// The "different parks activated" achievement thresholds, in order.
pub const PARK_TIERS: [u32; 54] = [
    10, 20, 30, 40, 50, 75, 100, 200, 300, 400, 500, 600, 700, 800, 900, 1000, 1500, 2000, 2500,
    3000, 3500, 4000, 4500, 5000, 5500, 6000, 6500, 7000, 7500, 8000, 8500, 9000, 9500, 10000,
    10500, 11000, 11500, 12000, 12500, 13000, 13500, 14000, 14500, 15000, 15500, 16000, 16500,
    17000, 17500, 18000, 18500, 19000, 19500, 20000,
];

/// Repeat Offender: one tier per this many valid activations of one park.
pub const REPEAT_STEP: u32 = 20;

/// Kilo: distinct contacts from one park.
pub const KILO_QSOS: u32 = 1000;

/// Rover: parks validly activated in one UTC day, per tier.
pub const ROVER_TIERS: [u32; 6] = [5, 10, 15, 20, 25, 30];

/// One park's standing under one callsign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParkStanding {
    /// The park ("US-1234"), upper case.
    pub reference: String,
    /// UTC days with a valid activation here.
    pub activations: u32,
    /// UTC days with contacts here that fell short of [`VALID_QSOS`].
    pub attempts: u32,
    /// Distinct contacts here, each day's counted by the counter's rule — Kilo's count.
    pub qsos: u32,
    /// 00:00 UTC of the last day with contacts here.
    pub last_day_unix: u64,
}

/// One callsign's activator standing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatorStats {
    /// The callsign the activations were worked under; `None` for records naming none.
    pub callsign: Option<String>,
    /// Valid activations: park × UTC day with at least [`VALID_QSOS`].
    pub activations: u32,
    /// Park × UTC day with contacts that fell short.
    pub attempts: u32,
    /// Distinct contacts as an activator, summed over every park and day.
    pub qsos: u32,
    /// Parks with at least one valid activation.
    pub parks_activated: u32,
    /// The most parks validly activated in one UTC day, and the first such day (00:00 UTC).
    pub rover_best: u32,
    pub rover_best_day_unix: Option<u64>,
    /// How many UTC days reached each of [`ROVER_TIERS`].
    pub rover_days: [u32; 6],
    /// Every park with contacts, most valid activations first, then most contacts, then by name.
    pub parks: Vec<ParkStanding>,
}

impl ActivatorStats {
    /// The next "different parks" threshold not yet reached; `None` past the last.
    pub fn next_park_tier(&self) -> Option<u32> {
        PARK_TIERS
            .iter()
            .copied()
            .find(|t| self.parks_activated < *t)
    }
}

/// Whether `a` is a POTA activation: its program says so, or it names none and its reference has
/// a park's shape.
fn is_pota(a: &LoggedActivation) -> bool {
    match a.program.as_deref() {
        Some(p) => p.eq_ignore_ascii_case("POTA"),
        None => crate::pota::normalize_pota_ref(&a.reference).is_some(),
    }
}

/// ★ The activator standing of each callsign in `activations` (the log's, as
/// [`crate::logbook::Logbook::activations`] lists them), most valid activations first.
pub fn by_callsign(activations: &[LoggedActivation]) -> Vec<ActivatorStats> {
    let mut calls: HashMap<Option<String>, Vec<&LoggedActivation>> = HashMap::new();
    for a in activations.iter().filter(|a| is_pota(a)) {
        calls.entry(a.callsign.clone()).or_default().push(a);
    }
    let mut out: Vec<ActivatorStats> = calls
        .into_iter()
        .map(|(callsign, acts)| one_call(callsign, &acts))
        .collect();
    out.sort_by(|a, b| {
        b.activations
            .cmp(&a.activations)
            .then(b.qsos.cmp(&a.qsos))
            .then_with(|| a.callsign.cmp(&b.callsign))
    });
    out
}

fn one_call(callsign: Option<String>, acts: &[&LoggedActivation]) -> ActivatorStats {
    let mut parks: BTreeMap<&str, ParkStanding> = BTreeMap::new();
    let mut valid_by_day: BTreeMap<u64, HashSet<&str>> = BTreeMap::new();
    let mut stats = ActivatorStats {
        callsign,
        activations: 0,
        attempts: 0,
        qsos: 0,
        parks_activated: 0,
        rover_best: 0,
        rover_best_day_unix: None,
        rover_days: [0; 6],
        parks: Vec::new(),
    };
    for a in acts {
        let valid = a.qsos >= VALID_QSOS;
        let p = parks
            .entry(a.reference.as_str())
            .or_insert_with(|| ParkStanding {
                reference: a.reference.clone(),
                activations: 0,
                attempts: 0,
                qsos: 0,
                last_day_unix: 0,
            });
        p.qsos += a.qsos as u32;
        p.last_day_unix = p.last_day_unix.max(a.day_start_unix);
        stats.qsos += a.qsos as u32;
        if valid {
            p.activations += 1;
            stats.activations += 1;
            valid_by_day
                .entry(a.day_start_unix)
                .or_default()
                .insert(a.reference.as_str());
        } else {
            p.attempts += 1;
            stats.attempts += 1;
        }
    }
    stats.parks_activated = parks.values().filter(|p| p.activations > 0).count() as u32;
    // Days in order, so the best day named is the first that reached it.
    for (day, refs) in &valid_by_day {
        let n = refs.len() as u32;
        if n > stats.rover_best {
            stats.rover_best = n;
            stats.rover_best_day_unix = Some(*day);
        }
        for (k, tier) in ROVER_TIERS.iter().enumerate() {
            if n >= *tier {
                stats.rover_days[k] += 1;
            }
        }
    }
    stats.parks = parks.into_values().collect();
    stats.parks.sort_by(|a, b| {
        b.activations
            .cmp(&a.activations)
            .then(b.qsos.cmp(&a.qsos))
            .then_with(|| a.reference.cmp(&b.reference))
    });
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::{adif_header, parse_adif};

    const DAY: u64 = 86_400;
    /// 2026-01-01 00:00 UTC.
    const D0: u64 = 1_767_225_600;

    fn act(reference: &str, day: u64, call: Option<&str>, qsos: usize) -> LoggedActivation {
        LoggedActivation {
            program: Some("POTA".into()),
            reference: reference.into(),
            day_start_unix: D0 + day * DAY,
            date: String::new(),
            callsign: call.map(str::to_string),
            qsos,
        }
    }

    /// `n` contacts from `park` on 2026-01-0`day`, under `call`, each a different station.
    fn contacts(park: &str, day: u32, call: &str, n: usize, from: usize) -> String {
        (from..from + n)
            .map(|i| {
                let worked = format!("K{i}AA");
                format!(
                    "<CALL:{}>{worked}<BAND:3>20m<MODE:2>CW<QSO_DATE:8>202601{day:02}<TIME_ON:6>1200{:02}\
                     <STATION_CALLSIGN:{}>{call}<MY_SIG:4>POTA<MY_SIG_INFO:{}>{park}<EOR>\n",
                    worked.len(),
                    i % 60,
                    call.len(),
                    park.len(),
                )
            })
            .collect()
    }

    /// The standing of a log's records, through [`add`] in log order.
    fn from_adif(adif: &str) -> Vec<ActivatorStats> {
        let mut found = Activations::default();
        for r in parse_adif(adif) {
            add(&mut found, &r);
        }
        by_callsign(&found.finish())
    }

    /// ★ `/P` is the plain call (operator ruling): KD9TAW's six and KD9TAW/P's four other
    /// contacts at one park on one day are ONE valid activation of ten, and a contact made under
    /// both spellings counts once. KD9TAW/M is another call: its ten are its own activation.
    #[test]
    fn a_portable_p_is_the_plain_call_and_mobile_is_not() {
        let mut adif = adif_header();
        adif += &contacts("US-0001", 1, "KD9TAW", 6, 0);
        adif += &contacts("US-0001", 1, "KD9TAW/P", 4, 6);
        // K0AA again under /P: the same contact, already counted.
        adif += &contacts("US-0001", 1, "KD9TAW/P", 1, 0);
        adif += &contacts("US-0001", 1, "KD9TAW/M", 10, 100);
        let stats = from_adif(&adif);
        let of = |call: &str| {
            let s = stats
                .iter()
                .find(|s| s.callsign.as_deref() == Some(call))
                .unwrap_or_else(|| panic!("{call} has no standing"));
            (s.activations, s.attempts, s.qsos)
        };
        assert_eq!(stats.len(), 2, "KD9TAW (with /P) and KD9TAW/M");
        assert_eq!(of("KD9TAW"), (1, 0, 10));
        assert_eq!(of("KD9TAW/M"), (1, 0, 10));
        assert!(stats
            .iter()
            .all(|s| s.callsign.as_deref() != Some("KD9TAW/P")));
    }

    /// ★ A fixture log, through the log's own activation list: ten distinct contacts make a valid
    /// activation and a dupe adds nothing toward it, short days are attempts that still count
    /// toward Kilo, and a two-fer is an activation at each park.
    #[test]
    fn a_fixture_log_counts_activations_parks_and_attempts_by_the_counters_rule() {
        let mut adif = adif_header();
        // Day 1: US-0001 with ten distinct, plus a dupe of the first (same call, band, mode).
        adif += &contacts("US-0001", 1, "KD9TAW", 10, 0);
        adif += &contacts("US-0001", 1, "KD9TAW", 1, 0);
        // Day 2: US-0001 again, nine distinct (short), and a two-fer US-0002,US-0003 with ten.
        adif += &contacts("US-0001", 2, "KD9TAW", 9, 100);
        adif += &contacts("US-0002,US-0003", 2, "KD9TAW", 10, 200);
        // A summit of mine is not a park.
        adif += "<CALL:4>K1AA<BAND:3>20m<MODE:2>CW<QSO_DATE:8>20260103<TIME_ON:6>120000\
                 <STATION_CALLSIGN:6>KD9TAW<MY_SIG:4>SOTA<MY_SOTA_REF:10>W7A/MN-001<EOR>\n";
        let stats = from_adif(&adif);
        assert_eq!(stats.len(), 1, "one callsign");
        let s = &stats[0];
        assert_eq!(s.callsign.as_deref(), Some("KD9TAW"));
        assert_eq!(s.activations, 3, "US-0001 day 1, US-0002 and US-0003 day 2");
        assert_eq!(s.attempts, 1, "US-0001 day 2 had nine");
        assert_eq!(s.parks_activated, 3);
        assert_eq!(
            s.qsos,
            10 + 9 + 10 + 10,
            "the dupe adds nothing; the two-fer counts at each"
        );
        assert_eq!(s.rover_best, 2, "two parks validly activated on day 2");
        assert_eq!(s.rover_best_day_unix, Some(D0 + DAY));
        let us1 = s.parks.iter().find(|p| p.reference == "US-0001").unwrap();
        assert_eq!((us1.activations, us1.attempts, us1.qsos), (1, 1, 19));
        assert_eq!(s.parks[0].reference, "US-0001", "most activations first");
        assert_eq!(s.next_park_tier(), Some(10));
    }

    #[test]
    fn ten_is_valid_and_nine_is_not() {
        let s = &by_callsign(&[
            act("US-0001", 0, Some("W1A"), 10),
            act("US-0002", 0, Some("W1A"), 9),
        ])[0];
        assert_eq!((s.activations, s.attempts, s.parks_activated), (1, 1, 1));
    }

    /// Two calls at one park on one day are two short activations, never one valid one.
    #[test]
    fn two_callsigns_are_counted_apart() {
        let stats = by_callsign(&[
            act("US-0001", 0, Some("W1A"), 6),
            act("US-0001", 0, Some("W1A/M"), 6),
        ]);
        assert_eq!(stats.len(), 2);
        assert!(stats.iter().all(|s| s.activations == 0 && s.attempts == 1));
    }

    /// Repeat Offender's and Kilo's raw counts: valid activations and every day's contacts at a park.
    #[test]
    fn repeat_offender_and_kilo_counts_per_park() {
        let mut acts: Vec<LoggedActivation> = (0..21)
            .map(|d| act("US-0001", d, Some("W1A"), 50))
            .collect();
        acts.push(act("US-0001", 30, Some("W1A"), 5));
        let s = &by_callsign(&acts)[0];
        let p = &s.parks[0];
        assert_eq!(p.activations, 21, "one Oasis (20), one toward the next");
        assert_eq!(p.activations / REPEAT_STEP, 1);
        assert_eq!(
            p.qsos,
            21 * 50 + 5,
            "Kilo counts the short day's contacts too"
        );
        assert!(p.qsos >= KILO_QSOS);
        assert_eq!(p.last_day_unix, D0 + 30 * DAY);
    }

    /// Rover: parks validly activated on one UTC day, each tier reached counted per day.
    #[test]
    fn rover_counts_valid_parks_per_utc_day() {
        let mut acts: Vec<LoggedActivation> = (0..5)
            .map(|k| act(&format!("US-000{k}"), 0, Some("W1A"), 10))
            .collect();
        // A sixth park that day fell short: not a rover park.
        acts.push(act("US-0009", 0, Some("W1A"), 9));
        // Another day with five again.
        acts.extend((0..5).map(|k| act(&format!("US-001{k}"), 1, Some("W1A"), 12)));
        let s = &by_callsign(&acts)[0];
        assert_eq!(s.rover_best, 5);
        assert_eq!(
            s.rover_best_day_unix,
            Some(D0),
            "the first day that reached it"
        );
        assert_eq!(s.rover_days, [2, 0, 0, 0, 0, 0], "Warthog twice");
    }

    /// A summit is not a park; a reference with no program counts only when it is park-shaped.
    #[test]
    fn only_pota_activations_count() {
        let mut sota = act("W7A/MN-001", 0, Some("W1A"), 10);
        sota.program = Some("SOTA".into());
        let mut bare = act("US-0001", 0, Some("W1A"), 10);
        bare.program = None;
        let mut odd = act("SOMEWHERE", 0, Some("W1A"), 10);
        odd.program = None;
        let s = &by_callsign(&[sota, bare, odd])[0];
        assert_eq!(s.activations, 1);
        assert_eq!(s.parks[0].reference, "US-0001");
    }

    #[test]
    fn park_tiers_are_ascending_and_name_the_next() {
        assert!(PARK_TIERS.windows(2).all(|w| w[0] < w[1]));
        let s = &by_callsign(
            &(0..10)
                .map(|k| act(&format!("US-{:04}", k), 0, Some("W1A"), 10))
                .collect::<Vec<_>>(),
        )[0];
        assert_eq!(s.parks_activated, 10);
        assert_eq!(s.next_park_tier(), Some(20));
    }
}
