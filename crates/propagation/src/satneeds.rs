//! Needs-aware satellite pass ranking — what a pass would EARN the operator.
//!
//! The S.A.T. appliance sorts passes by max elevation because elevation is all it
//! knows. Nexus knows the logbook, so it can answer the better question: *which
//! grids and entities could this pass add?* (spec `sat-tracker.md` §3). This
//! module is the pure half of that answer: given a TLE, a pass window over the
//! operator, and the operator's needs, compute the earn summary.
//!
//! ## What "reachable" means
//! During a pass the operator can work anyone who also sees the bird — anyone
//! inside the satellite's horizon footprint while it is above the operator's
//! horizon. The pass window `[aos, los]` *is* "above the operator's horizon"
//! (it was computed for this observer), so the reachable region is simply the
//! union of the footprint circles along the window. We sample the sub-point
//! along the pass and collect every 4-character Maidenhead square whose centre
//! falls inside a sampled footprint.
//!
//! ## What "earn" means
//! - **New grids:** reachable squares the operator has not worked VIA SATELLITE —
//!   Satellite-VUCC slots on offer. Per-band terrestrial VUCC is a different
//!   award and deliberately not consulted (ARRL counts satellite contacts toward
//!   Satellite VUCC only).
//! - **New entities:** current DXCC entities whose cty.dat centroid falls inside
//!   the reachable region and that the operator has never worked at all (ATNO
//!   candidates). Centroid-in-footprint is an honest approximation: a huge
//!   entity whose *edge* clips the footprint can be missed. It under-claims,
//!   never over-claims.
//!
//! The needs sets are BORROWED from [`crate::dxped::LogNeeds`] — one needs
//! engine per concept (spec §5 invariant 6); this module never re-derives them.
//!
//! ## Bounds
//! Compute is bounded and on-demand (a Tauri command, never the radio loop):
//! at most [`MAX_SAMPLES`] sub-point samples per pass, each enumerating only the
//! grid squares inside the footprint's bounding box. Pure math, no I/O.

use std::collections::{BTreeSet, HashSet};

use serde::Serialize;

use crate::dxcc;
use crate::geo::haversine_km;
use crate::sat::{subpoint, Tle};

/// Mean Earth radius (km) for the footprint horizon arc — the same constant the
/// map's footprint ring uses, so the two draw the same circle.
const RE_KM: f64 = 6371.0;
/// Sub-point samples per pass, max. A LEO pass (~15 min) samples every
/// [`MIN_STEP_SECS`]; a multi-hour MEO pass is capped here and spread evenly.
const MAX_SAMPLES: i64 = 32;
/// Floor on the sampling step — a footprint is thousands of km across and moves
/// ~7 km/s, so 60 s steps overlap heavily; finer adds cost, not coverage.
const MIN_STEP_SECS: i64 = 60;
/// Cap on the example lists in the DTO (the counts are always complete).
const SAMPLE_CAP: usize = 8;

/// The operator's satellite-relevant needs — references into
/// [`crate::dxped::LogNeeds`]'s sets. This struct only borrows answers; it is
/// not a second needs engine.
pub struct SatNeeds<'a> {
    /// 4-char grids already worked via satellite (Satellite-VUCC slots held).
    pub worked_sat_grids: &'a HashSet<String>,
    /// DXCC entities already worked at all (any band, any propagation) — the
    /// ATNO filter.
    pub worked_entities: &'a HashSet<String>,
}

/// What one pass could earn — the small DTO stamped onto a pass row.
/// Counts are complete; the `*_sample` lists are capped at [`SAMPLE_CAP`]
/// (sorted, deterministic) so the wire stays small.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SatPassEarn {
    /// Reachable 4-char grids NOT yet worked via satellite (Satellite-VUCC
    /// slots on offer through this pass).
    pub new_grids: usize,
    /// Up to [`SAMPLE_CAP`] of those grids, sorted.
    pub grid_sample: Vec<String>,
    /// Reachable current-DXCC entities never worked at all (ATNO candidates).
    pub new_entities: usize,
    /// Up to [`SAMPLE_CAP`] of those entities, sorted.
    pub entity_sample: Vec<String>,
    /// Sort key, not a quantity: an ATNO entity outranks any number of grids
    /// (`new_entities * 10_000 + min(new_grids, 9_999)`). Ties break upstream
    /// (AOS order is preserved by the caller).
    pub score: u32,
}

/// Everything this pass can reach: the union of footprint circles along the
/// window, as 4-char grid squares + the current-DXCC entities whose centroid
/// falls inside. Pure geometry — no needs filtering yet (that is
/// [`pass_earn`]'s job), so tests can pin containment invariants directly.
fn reachable(tle: &Tle, aos_unix: i64, los_unix: i64) -> (HashSet<String>, BTreeSet<&'static str>) {
    let mut grids: HashSet<String> = HashSet::new();
    let mut entities: BTreeSet<&'static str> = BTreeSet::new();
    let dur = (los_unix - aos_unix).max(0);
    let step = (dur / (MAX_SAMPLES - 1).max(1)).max(MIN_STEP_SECS);
    let mut t = aos_unix;
    loop {
        if let Some((lat, lon, alt_km)) = subpoint(tle, t) {
            // Horizon arc: the ground distance to where the bird sits on the
            // horizon — everyone closer than this sees it.
            let footprint_km = RE_KM * (RE_KM / (RE_KM + alt_km.max(0.0))).acos();
            collect_grids(&mut grids, lat, lon, footprint_km);
            for (name, elat, elon) in dxcc::dxcc_entity_locations() {
                if !entities.contains(name)
                    && haversine_km((lat, lon), (elat, elon)) <= footprint_km
                {
                    entities.insert(name);
                }
            }
        }
        if t >= los_unix {
            break;
        }
        t = (t + step).min(los_unix);
    }
    (grids, entities)
}

/// Insert every 4-char Maidenhead square whose CENTRE lies within `radius_km`
/// of `(lat, lon)`. Iterates square indices directly (lat rows are 1° tall,
/// lon columns 2° wide; 18 A–R fields × 10 squares = 180 each way) over the
/// circle's bounding box only, with longitude wrapping across the antimeridian.
fn collect_grids(out: &mut HashSet<String>, lat: f64, lon: f64, radius_km: f64) {
    const KM_PER_DEG_LAT: f64 = 111.0;
    let dlat = radius_km / KM_PER_DEG_LAT;
    let gy_lo = ((lat - dlat + 90.0).floor() as i64).max(0);
    let gy_hi = ((lat + dlat + 90.0).floor() as i64).min(179);
    // Great-circle disc, so the per-row longitude half-width comes from the
    // spherical-cap relation, EXACT at each row's centre latitude:
    //     cos Δλ = (cos(r/R) − sin φc·sin φrow) / (cos φc·cos φrow)
    // The old flat-earth span (r / (111·cos φrow)) under-measured the cap's
    // wrap toward the pole and silently DROPPED reachable grids once the
    // footprint centre passed ~70° — a polar pass (any sun-synchronous bird)
    // lost up to 20% of its earn count, and "Counts are complete" was false
    // exactly for the operators the ranking serves. x < −1 ⇒ even the
    // antipodal meridian is inside: take the whole row. x > 1 ⇒ nothing at
    // this latitude is inside (a bounding-box edge row): skip it. Division by
    // a ~0 cos at a pole rides the IEEE ±inf into the same two branches.
    let cap = (radius_km / RE_KM).cos();
    let (sin_c, cos_c) = lat.to_radians().sin_cos();
    for gy in gy_lo..=gy_hi {
        let lat_c = (gy - 90) as f64 + 0.5;
        let (sin_r, cos_r) = lat_c.to_radians().sin_cos();
        let x = (cap - sin_c * sin_r) / (cos_c * cos_r);
        let span_cols = if x <= -1.0 {
            90 // the whole row (180 columns / 2 each side)
        } else if x >= 1.0 {
            continue;
        } else {
            // Δλ bounds the CIRCLE at this latitude; a column centre sits up
            // to 1° off our longitude, hence the +1 column margin. Membership
            // itself stays with haversine below — the span only bounds the
            // enumeration.
            (((x.acos().to_degrees() / 2.0).ceil() as i64) + 1).min(90)
        };
        let gx_center = (((lon + 180.0) / 2.0).floor() as i64).clamp(0, 179);
        for ix in (gx_center - span_cols)..=(gx_center + span_cols) {
            let gx = ix.rem_euclid(180);
            let lon_c = (gx * 2 - 180) as f64 + 1.0;
            if haversine_km((lat, lon), (lat_c, lon_c)) <= radius_km {
                out.insert(grid_name(gx, gy));
            }
        }
    }
}

/// Square indices → the 4-char locator ("EN37"): field letters A–R then the
/// square digits.
fn grid_name(gx: i64, gy: i64) -> String {
    let mut s = String::with_capacity(4);
    s.push((b'A' + (gx / 10) as u8) as char);
    s.push((b'A' + (gy / 10) as u8) as char);
    s.push((b'0' + (gx % 10) as u8) as char);
    s.push((b'0' + (gy % 10) as u8) as char);
    s
}

/// The earn summary for one pass window: reachable region minus what the
/// operator already holds. `aos`/`los` come from [`crate::sat::passes`] for
/// this observer, so observer visibility is already encoded in the window.
pub fn pass_earn(tle: &Tle, aos_unix: i64, los_unix: i64, needs: &SatNeeds) -> SatPassEarn {
    let (grids, entities) = reachable(tle, aos_unix, los_unix);
    let mut new_grids: Vec<&str> = grids
        .iter()
        .map(String::as_str)
        .filter(|g| !needs.worked_sat_grids.contains(*g))
        .collect();
    new_grids.sort_unstable();
    let new_entities: Vec<&'static str> = entities
        .iter()
        .copied()
        .filter(|e| !needs.worked_entities.contains(*e))
        .collect(); // BTreeSet iteration is already sorted
    let score = (new_entities.len() as u32) * 10_000 + (new_grids.len() as u32).min(9_999);
    SatPassEarn {
        new_grids: new_grids.len(),
        grid_sample: new_grids
            .iter()
            .take(SAMPLE_CAP)
            .map(|s| s.to_string())
            .collect(),
        new_entities: new_entities.len(),
        entity_sample: new_entities
            .iter()
            .take(SAMPLE_CAP)
            .map(|s| s.to_string())
            .collect(),
        score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::latlon_to_maidenhead;
    use crate::sat::passes;

    // The same canonical ISS element set sat.rs pins its geometry with — the
    // AIAA-2006-6753 verification vector for catalog 25544. Real, reproducible.
    const ISS_L1: &str = "1 25544U 98067A   08264.51782528 -.00002182  00000-0 -11606-4 0  2927";
    const ISS_L2: &str = "2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.72125391563537";
    const ISS_EPOCH_UNIX: i64 = 1_221_913_539;

    fn iss() -> Tle {
        Tle {
            name: "ISS (ZARYA)".to_string(),
            line1: ISS_L1.to_string(),
            line2: ISS_L2.to_string(),
        }
    }

    /// Mid-US observer + their first real pass in the 24 h after the TLE epoch.
    fn first_pass() -> (Tle, (f64, f64), i64, i64) {
        let tle = iss();
        let obs = (40.0, -88.0);
        let p = passes(&tle, obs, ISS_EPOCH_UNIX, 24)
            .into_iter()
            .next()
            .expect("the ISS passes a mid-latitude observer within 24 h");
        (tle, obs, p.aos_unix, p.los_unix)
    }

    fn empty_needs() -> (HashSet<String>, HashSet<String>) {
        (HashSet::new(), HashSet::new())
    }

    #[test]
    fn reachable_region_contains_the_observer() {
        // The pass window means "the observer sees the bird", so the observer's
        // own grid square MUST be inside the reachable union — if it is not,
        // the footprint radius and the pass geometry disagree.
        let (tle, obs, aos, los) = first_pass();
        let (grids, entities) = reachable(&tle, aos, los);
        let my_grid = latlon_to_maidenhead(obs.0, obs.1)[..4].to_uppercase();
        assert!(
            grids.contains(&my_grid),
            "observer grid {my_grid} missing from {} reachable grids",
            grids.len()
        );
        // A mid-US footprint reaches the US centroid too.
        assert!(entities.contains("United States"), "entities: {entities:?}");
        // Sanity: a LEO footprint sweep is hundreds-to-thousands of squares,
        // not a handful and not the whole planet.
        assert!(
            grids.len() > 100 && grids.len() < 20_000,
            "implausible reachable-grid count {}",
            grids.len()
        );
    }

    #[test]
    fn collect_grids_matches_brute_force_everywhere_including_the_poles() {
        // The defect this pins: the per-row longitude span used the flat-earth
        // r/(111·cosφ), which under-measures a great-circle disc's wrap toward
        // the pole. Above footprint-centre lat ≈ 70° that silently DROPPED
        // reachable grids — 20% of them at 82° — understating every polar
        // pass's earn count while the DTO doc claimed "Counts are complete".
        // The ISS-geometry tests can never see it (51.6° inclination — the
        // corpus-blindness rule), so this pins the collector against brute
        // force over every grid centre on the planet, at mid-lat, high-lat
        // and antimeridian-straddling centres. Exact set equality: no square
        // missed, none over-claimed.
        let cases: &[(f64, f64, f64)] = &[
            (40.0, -88.0, 2657.0),  // mid-lat control (the ISS regime)
            (65.0, -20.0, 2657.0),  // first flat-earth misses appear here
            (75.0, 0.0, 2657.0),    // sun-synchronous territory
            (81.0, 30.0, 2663.0),   // deep polar
            (82.0, -150.0, 2980.0), // polar + antimeridian wrap, 800 km alt
        ];
        for &(clat, clon, radius) in cases {
            let mut got = HashSet::new();
            collect_grids(&mut got, clat, clon, radius);
            for gy in 0..180i64 {
                for gx in 0..180i64 {
                    let lat_c = (gy - 90) as f64 + 0.5;
                    let lon_c = (gx * 2 - 180) as f64 + 1.0;
                    let inside = haversine_km((clat, clon), (lat_c, lon_c)) <= radius;
                    assert_eq!(
                        got.contains(&grid_name(gx, gy)),
                        inside,
                        "{} (centre {lat_c},{lon_c}) vs footprint ({clat},{clon}) r={radius} km: \
                         collected={} inside={}",
                        grid_name(gx, gy),
                        got.contains(&grid_name(gx, gy)),
                        inside
                    );
                }
            }
        }
    }

    #[test]
    fn footprint_radius_agrees_with_pass_visibility() {
        // At every sampled instant of the pass the observer is inside the
        // footprint circle (el ≥ 0 ⇔ within the horizon arc). Ties the radius
        // formula to sat.rs's own visibility maths; ~1% slack covers the
        // spherical-vs-WGS84 mismatch between haversine and the geodetic
        // sub-point.
        let (tle, obs, aos, los) = first_pass();
        let mut checked = 0;
        for k in 0..=10 {
            let t = aos + (los - aos) * k / 10;
            let Some((lat, lon, alt)) = subpoint(&tle, t) else {
                continue;
            };
            let footprint = RE_KM * (RE_KM / (RE_KM + alt)).acos();
            let d = haversine_km(obs, (lat, lon));
            assert!(
                d <= footprint * 1.01 + 25.0,
                "t={t}: observer {d:.0} km out, footprint {footprint:.0} km"
            );
            checked += 1;
        }
        assert!(checked >= 8, "pass barely sampled ({checked})");
    }

    #[test]
    fn worked_sat_grids_are_not_earned_again() {
        let (tle, _obs, aos, los) = first_pass();
        let (worked_grids, worked_entities) = empty_needs();
        let all = pass_earn(
            &tle,
            aos,
            los,
            &SatNeeds {
                worked_sat_grids: &worked_grids,
                worked_entities: &worked_entities,
            },
        );
        assert!(all.new_grids > 0);
        assert_eq!(all.grid_sample.len(), SAMPLE_CAP.min(all.new_grids));
        // Work one reachable grid via satellite → exactly that one stops
        // counting, and it leaves the sample.
        let taken = all.grid_sample[0].clone();
        let worked: HashSet<String> = [taken.clone()].into();
        let fewer = pass_earn(
            &tle,
            aos,
            los,
            &SatNeeds {
                worked_sat_grids: &worked,
                worked_entities: &worked_entities,
            },
        );
        assert_eq!(fewer.new_grids, all.new_grids - 1);
        assert!(!fewer.grid_sample.contains(&taken));
    }

    #[test]
    fn worked_entities_are_not_atno() {
        let (tle, _obs, aos, los) = first_pass();
        let (worked_grids, mut worked_entities) = empty_needs();
        let before = pass_earn(
            &tle,
            aos,
            los,
            &SatNeeds {
                worked_sat_grids: &worked_grids,
                worked_entities: &worked_entities,
            },
        );
        assert!(before.new_entities > 0);
        // The mid-US pass reaches the US (pinned by reachable_region_contains_
        // the_observer); the capped sample is alphabetical so consult the full
        // region for the entity we take.
        let (_, entities) = reachable(&tle, aos, los);
        assert!(entities.contains("United States"));
        worked_entities.insert("United States".to_string());
        let after = pass_earn(
            &tle,
            aos,
            los,
            &SatNeeds {
                worked_sat_grids: &worked_grids,
                worked_entities: &worked_entities,
            },
        );
        assert_eq!(after.new_entities, before.new_entities - 1);
        assert!(!after.entity_sample.iter().any(|e| e == "United States"));
    }

    #[test]
    fn an_atno_outranks_any_number_of_grids() {
        // The score is a sort key: one reachable ATNO must beat a pass that
        // offers only grids, however many.
        let (tle, _obs, aos, los) = first_pass();
        let (no_grids_worked, no_entities_worked) = empty_needs();
        let with_atno = pass_earn(
            &tle,
            aos,
            los,
            &SatNeeds {
                worked_sat_grids: &no_grids_worked,
                worked_entities: &no_entities_worked,
            },
        );
        // Same pass with EVERY reachable entity already worked: grids only.
        let (_, entities) = reachable(&tle, aos, los);
        let all_entities: HashSet<String> = entities.iter().map(|s| s.to_string()).collect();
        let grids_only = pass_earn(
            &tle,
            aos,
            los,
            &SatNeeds {
                worked_sat_grids: &no_grids_worked,
                worked_entities: &all_entities,
            },
        );
        assert_eq!(grids_only.new_entities, 0);
        assert!(
            with_atno.score > grids_only.score,
            "ATNO pass {} must outrank grid-only pass {}",
            with_atno.score,
            grids_only.score
        );
    }

    #[test]
    fn a_long_meo_window_stays_bounded() {
        // A 6-hour (IO-117-style) window must complete quickly under the
        // sample cap — this is the "must not stall the snapshot path" bound.
        let (tle, _obs, aos, _los) = first_pass();
        let t0 = std::time::Instant::now();
        let (grids, worked_entities) = empty_needs();
        let earn = pass_earn(
            &tle,
            aos,
            aos + 6 * 3600,
            &SatNeeds {
                worked_sat_grids: &grids,
                worked_entities: &worked_entities,
            },
        );
        assert!(earn.new_grids > 0);
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(10),
            "6 h window took {:?}",
            t0.elapsed()
        );
    }
}
