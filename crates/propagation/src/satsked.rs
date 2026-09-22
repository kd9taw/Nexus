//! Satellite SKEDS — the windows in which TWO stations can work each other
//! through one bird.
//!
//! Everything else in the satellite stack answers "when is this bird up for
//! ME". A sked is the two-observer question: *given the other operator's grid,
//! when is the satellite above the horizon for BOTH of us at once, high enough
//! at each end to actually work?* That is the whole feature — two operators
//! agreeing when to meet on a satellite — and it is the one thing the geometry
//! in [`crate::sat`] has never been joined up for.
//!
//! ## Why this is not an intersection of two pass lists
//! It reads like one, and that reading is wrong in both directions:
//!
//! * **Time overlap is not simultaneity at a USABLE elevation.** Two 0°-horizon
//!   pass intervals can overlap for minutes while the bird never once stands
//!   above 5° at both ends *at the same instant* — the pair is simply too far
//!   apart for the footprint to cover them both at that height. Filtering each
//!   station's passes to a minimum elevation and *then* intersecting is the
//!   same mistake wearing a threshold: `max_el` is a fact about one moment of
//!   the pass, and the two stations' best moments are not the same moment.
//! * **The constraint is pointwise.** A window is a maximal run of instants at
//!   which `min(el_here, el_there) ≥ min_el`. Nothing about the endpoints of
//!   either station's pass is sufficient to compute it, so this module does not
//!   try: it evaluates the condition on a time grid.
//!
//! The operator's own pass list *is* used, but only as a BRACKET. Mutual
//! visibility requires `el_here ≥ min_el > 0`, which happens strictly inside
//! the operator's own above-horizon interval, so [`crate::sat::passes`] over
//! the operator bounds the search for free and the fine scan only runs where a
//! window could exist (a LEO is up perhaps 4% of the day — the rest is skipped
//! rather than sampled).
//!
//! ## The two numbers this module has to defend
//!
//! **Minimum elevation: [`MUTUAL_MIN_EL_DEG`] = 5°, at EACH end.** 0° is the
//! geometric horizon and no real station has it — trees, houses and terrain put
//! a typical amateur horizon somewhere between 5° and 15°, so a "window" that
//! exists only below 5° at one end is a window that station cannot use. It is
//! deliberately **not** the app's own 10° `WORKABLE_EL_DEG`
//! (`ui/src/features/satSeed.ts`), and that is a distinction rather than a
//! second threshold: 10° answers *"is this bird worth starring for me"*, a
//! one-station quality rank where a grazer is a poor use of an evening when a
//! 40° pass comes later. A mutual window is not a choice among many — it is a
//! scarce coincidence, and satellite DX is worked at exactly these low mutual
//! elevations. The arithmetic makes the cost concrete: for a 400 km LEO the
//! ground radius of the 10° circle is ~1,350 km, so demanding 10° at both ends
//! caps the pair separation at ~2,700 km and deletes most of the long-haul
//! skeds this feature exists to arrange; at 5° the circle is ~1,660 km and the
//! ceiling is ~3,300 km. The floor is a parameter, not a baked constant, so the
//! choice is visible to callers and pinned by test.
//!
//! **Reporting floor: [`MIN_WINDOW_SECS`] = 60 s**, scanned at
//! [`SCAN_STEP_SECS`] = 10 s. Near the separation ceiling a mutual window
//! collapses to seconds, and a grid exchange needs about a minute even when
//! both ends are pointed and tuned. Keeping the floor at six times the scan
//! step is also what makes the scan honest: any window long enough to be
//! REPORTED is long enough that the grid cannot step over it, so what this
//! module misses is only ever what it would have refused to report anyway.
//!
//! ## Honesty about the far end of the horizon
//! A prediction 14 days out is a different claim from a pass tonight, because
//! the elements are ageing the whole way: predicting `n` days forward from a
//! set that is already `d` days old propagates `d + n` days past epoch. The
//! rule here reuses the two ages the project already applies to elements rather
//! than inventing a third — see [`ACT_CEILING_DAYS`] and [`FIRM_AGE_DAYS`].
//!
//! Pure math over caller-supplied TLEs; no I/O, no clock, no network.

use serde::Serialize;

use crate::sat::{self, Tle};

/// Minimum elevation (degrees) at EACH end for an instant to count as mutually
/// workable. See the module docs for why 5° and not 0° or 10°.
pub const MUTUAL_MIN_EL_DEG: f64 = 5.0;

/// Time grid (seconds) the mutual condition is evaluated on, inside a candidate
/// pass. Matches the resolution [`crate::sat::passes`] refines its own horizon
/// crossings to, so a sked window and a pass row can never disagree by more
/// than one another's quantisation.
pub const SCAN_STEP_SECS: i64 = 10;

/// The shortest window worth putting in front of an operator (seconds). Six
/// scan steps — see the module docs on why the ratio, not just the value, is
/// load-bearing.
pub const MIN_WINDOW_SECS: i64 = 60;

/// Element age (days) past which this project refuses to ACT on a TLE — the
/// same ceiling the satellite view and the ISS auto-arm apply (`SAT_STALE_DAYS`
/// in `src-tauri`), applied here to the age the elements will have reached AT
/// THE PREDICTED TIME rather than at the moment of asking. A bird whose
/// elements are already 25 days old therefore yields skeds for five more days
/// and then stops, instead of quietly extrapolating a month past epoch.
pub const ACT_CEILING_DAYS: f64 = 30.0;

/// Element age (days) past which a window is still shown but no longer called
/// firm. The same 14-day tier [`crate::sat::tle_age_days`] documents as the
/// badge threshold: SGP4's along-track error on an amateur LEO runs a few km
/// per day, which is seconds of AOS error over a fortnight — tolerable against
/// a window measured in minutes — but a drag or manoeuvre surprise (the ISS
/// reboosts) is not modelled at all, so the far end of the horizon is a plan to
/// re-check, never a promise.
pub const FIRM_AGE_DAYS: f64 = 14.0;

/// The longest horizon this module will predict over, in days, whatever the
/// caller asks for. Matches what the specialist tools offer; [`ACT_CEILING_DAYS`]
/// trims it per bird, so a stale bird never reaches it.
pub const MAX_HORIZON_DAYS: u32 = 14;

/// One window in which both stations can see the bird at or above the floor.
///
/// The interval is the CONSERVATIVE inner bound: `start_unix` is the first
/// sampled instant at which the condition holds and `end_unix` the last, so the
/// true crossings lie within one [`SCAN_STEP_SECS`] OUTSIDE each end. The error
/// is deliberately in the safe direction — this never offers a second of sked
/// that is not there.
///
/// ## Three elevations, because one would be a lie either way
/// A mutual window has no single "elevation". [`mutual_el_deg`](Self::mutual_el_deg)
/// is the pair's shared ceiling and the honest quality number; the two
/// per-station maxima are what each operator's own antenna actually has to do,
/// and on a real pass they are wildly different (measured on a 1,473 km pair:
/// 66° at one end against 20° at the other, in the same window). Reporting only
/// the shared ceiling would hide that one end has an easy overhead pass while
/// the other is scraping the treeline; reporting only a per-station peak would
/// claim a quality the pair never has, because the two stations' best moments
/// are not the same moment.
///
/// ⭐ Do not "simplify" this to the elevations AT `peak_unix`: maximising
/// `min(el_here, el_there)` lands at the instant the two elevations CROSS, so
/// that pair is near-identical by construction (measured: within 1.9° on every
/// window of the fixture) and carries no information about either station.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutualWindow {
    /// First instant both stations are at or above the floor (unix seconds).
    pub start_unix: i64,
    /// Last such instant (unix seconds).
    pub end_unix: i64,
    /// The instant of BEST mutual geometry — where `min(el_here, el_there)` is
    /// largest. On a long window this is when to make the call; on a short one
    /// it is the middle of it.
    pub peak_unix: i64,
    /// THE quality number: the highest elevation the WORSE-OFF end reaches at
    /// any single instant of the window, i.e. `max over t of min(el_here,
    /// el_there)`, attained at `peak_unix`. Never above either station's own
    /// maximum, and always at or above the floor the window was found with.
    pub mutual_el_deg: f64,
    /// Highest elevation (°) the bird reaches for THIS station inside the
    /// window — not necessarily at `peak_unix`.
    pub max_el_here_deg: f64,
    /// Highest elevation (°) the bird reaches for the OTHER station inside the
    /// window — not necessarily at `peak_unix`.
    pub max_el_there_deg: f64,
    /// Compass azimuth (°, 0 = N clockwise) to the bird from THIS station at
    /// `peak_unix` — where to point when the window is at its best.
    pub peak_az_here_deg: f64,
}

impl MutualWindow {
    /// Window length in seconds.
    pub fn secs(&self) -> i64 {
        self.end_unix - self.start_unix
    }
}

/// A [`MutualWindow`] with the bird it belongs to and how far past its elements'
/// epoch the prediction reached — the row a sked list is built from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkedWindow {
    /// The element set's object name, as the caller labelled it.
    pub sat_name: String,
    /// NORAD catalog number when line 1 carries one.
    pub norad: Option<u32>,
    /// The window itself.
    #[serde(flatten)]
    pub window: MutualWindow,
    /// How old the elements will be (days) at `start_unix`. Always reported —
    /// the operator planning a sked twelve days out is entitled to know the
    /// prediction is running on two-week-old elements by then.
    pub element_age_days: f64,
    /// `false` once `element_age_days` passes [`FIRM_AGE_DAYS`]: still shown,
    /// but to be re-checked nearer the time rather than relied on.
    pub firm: bool,
}

/// Every window in which `here` and `there` can both see `tle` at or above
/// `min_el_deg`, within `[from_unix, from_unix + hours·3600)`.
///
/// `here`/`there` are geodetic `(lat°, lon°)`. Empty when the elements are
/// unparseable or decayed, when the pair is too far apart for the bird's
/// footprint to cover both (the common and correct answer — see the module
/// docs), or when every coincidence is shorter than [`MIN_WINDOW_SECS`].
/// Windows come back in time order.
pub fn mutual_windows(
    tle: &Tle,
    here: (f64, f64),
    there: (f64, f64),
    from_unix: i64,
    hours: u32,
    min_el_deg: f64,
) -> Vec<MutualWindow> {
    let mut out = Vec::new();
    // The bracket: mutual visibility needs el_here ≥ min_el > 0, which can only
    // happen inside one of THIS station's own above-horizon intervals. Scanning
    // the whole horizon instead would sample ~25× more instants for the same
    // answer.
    for p in sat::passes(tle, here, from_unix, hours) {
        let here_track = sat::pass_track(tle, here, p.aos_unix, p.los_unix, SCAN_STEP_SECS as u32);
        let there_track =
            sat::pass_track(tle, there, p.aos_unix, p.los_unix, SCAN_STEP_SECS as u32);
        out.extend(windows_from_tracks(&here_track, &there_track, min_el_deg));
    }
    out
}

/// The run extraction, split out so a test can drive it on synthetic tracks
/// with no propagator in the way.
///
/// Both tracks are the same time grid sampled for two observers, ascending;
/// either may be missing samples where the propagation diverged, so they are
/// MERGE-JOINED on the timestamp rather than zipped by index. A missing sample
/// does not split a run — it is simply not evaluated, matching
/// [`crate::sat::passes`]'s "a single divergent step: keep scanning rather than
/// truncate".
fn windows_from_tracks(
    here: &[(i64, f64, f64)],
    there: &[(i64, f64, f64)],
    min_el_deg: f64,
) -> Vec<MutualWindow> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    // The run being accumulated: (start, last-ok instant, best-mutual sample).
    let mut run: Option<(i64, i64, MutualWindow)> = None;

    // Close the run under construction, keeping it only if it clears the floor.
    fn close(run: Option<(i64, i64, MutualWindow)>, out: &mut Vec<MutualWindow>) {
        if let Some((start, last, mut w)) = run {
            if last - start >= MIN_WINDOW_SECS {
                w.start_unix = start;
                w.end_unix = last;
                out.push(w);
            }
        }
    }

    while i < here.len() && j < there.len() {
        let (th, az_here, el_here) = here[i];
        let (tt, _az_there, el_there) = there[j];
        match th.cmp(&tt) {
            std::cmp::Ordering::Less => {
                i += 1;
                continue;
            }
            std::cmp::Ordering::Greater => {
                j += 1;
                continue;
            }
            std::cmp::Ordering::Equal => {}
        }
        i += 1;
        j += 1;

        if el_here >= min_el_deg && el_there >= min_el_deg {
            let mutual = el_here.min(el_there);
            let sample = MutualWindow {
                start_unix: th,
                end_unix: th,
                peak_unix: th,
                mutual_el_deg: mutual,
                max_el_here_deg: el_here,
                max_el_there_deg: el_there,
                peak_az_here_deg: az_here,
            };
            run = Some(match run {
                Some((start, _, mut acc)) => {
                    // The peak fields move together, to the best mutual instant.
                    if mutual > acc.mutual_el_deg {
                        acc.mutual_el_deg = mutual;
                        acc.peak_unix = th;
                        acc.peak_az_here_deg = az_here;
                    }
                    // The per-station maxima run independently of it and of
                    // each other — that is the whole point of carrying them.
                    acc.max_el_here_deg = acc.max_el_here_deg.max(el_here);
                    acc.max_el_there_deg = acc.max_el_there_deg.max(el_there);
                    (start, th, acc)
                }
                None => (th, th, sample),
            });
        } else {
            close(run.take(), &mut out);
        }
    }
    close(run.take(), &mut out);
    out
}

/// The sked list over a set of birds: [`mutual_windows`] per element set,
/// stamped with the bird and with how far past epoch the prediction ran, sorted
/// by start time.
///
/// `days` is clamped to `1..=`[`MAX_HORIZON_DAYS`], and then trimmed PER BIRD so
/// no window is predicted past [`ACT_CEILING_DAYS`] of element age — a bird with
/// old elements simply stops contributing part-way along the horizon instead of
/// being extrapolated beyond what this project is willing to act on. A bird
/// already past the ceiling at `from_unix` contributes nothing, and one whose
/// line 1 has no readable epoch is dropped rather than guessed at.
pub fn mutual_schedule(
    tles: &[&Tle],
    here: (f64, f64),
    there: (f64, f64),
    from_unix: i64,
    days: u32,
    min_el_deg: f64,
) -> Vec<SkedWindow> {
    let days = days.clamp(1, MAX_HORIZON_DAYS);
    let mut out = Vec::new();
    for tle in tles {
        let Some(age_now) = sat::tle_age_days(&tle.line1, from_unix) else {
            continue; // no readable epoch — no age claim, so no prediction
        };
        // How much of the asked-for horizon is inside the acting ceiling.
        let allowed_days = (ACT_CEILING_DAYS - age_now).min(days as f64);
        if allowed_days <= 0.0 {
            continue;
        }
        let hours = (allowed_days * 24.0).floor().max(1.0) as u32;
        let norad = sat::norad_id(&tle.line1);
        for window in mutual_windows(tle, here, there, from_unix, hours, min_el_deg) {
            let element_age_days = age_now + (window.start_unix - from_unix) as f64 / 86_400.0;
            out.push(SkedWindow {
                sat_name: tle.name.clone(),
                norad,
                window,
                element_age_days,
                firm: element_age_days <= FIRM_AGE_DAYS,
            });
        }
    }
    out.sort_by_key(|w| w.window.start_unix);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::{haversine_km, maidenhead_to_latlon};

    // The canonical ISS element set — the AIAA-2006-6753 SGP4 verification
    // vector for catalog 25544 (epoch 2008-09-20 12:25:40 UTC), the same one
    // `sat.rs`'s own tests are pinned against. Real, published, reproducible.
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

    fn grid(g: &str) -> (f64, f64) {
        maidenhead_to_latlon(g).expect("fixture grid must parse")
    }

    /// Elevation (°) of the bird from a ground point, derived from the
    /// SUB-SATELLITE POINT by spherical trigonometry — deliberately NOT the
    /// SEZ transform the module under test uses.
    ///
    /// This is the independent reference the window elevations are checked
    /// against: it consumes only `subpoint`'s geodetic lat/lon/alt and a
    /// great-circle distance, so a sign flip, a swapped axis or a crossed
    /// observer in `look_angles` shows up as tens of degrees here. It models a
    /// SPHERICAL earth where the module models WGS84, which is the whole of the
    /// disagreement between them (a few tenths of a degree at mid-latitudes) —
    /// see `mutual_elevations_match_an_independent_spherical_derivation` for
    /// the measured bound.
    fn elevation_from_subpoint(sub: (f64, f64, f64), station: (f64, f64)) -> f64 {
        const RE_KM: f64 = 6371.0;
        let (sub_lat, sub_lon, alt_km) = sub;
        // Central angle between the station and the sub-satellite point.
        let lambda = haversine_km((sub_lat, sub_lon), station) / RE_KM;
        // Standard look-angle relation: tan(el) = (cos λ − Re/(Re+h)) / sin λ.
        let (sin_l, cos_l) = lambda.sin_cos();
        (cos_l - RE_KM / (RE_KM + alt_km)).atan2(sin_l).to_degrees()
    }

    /// Passes over one station whose peak clears `min_el` — the "each end has
    /// plenty of chances on its own" control every negative result below needs.
    fn solo_workable_passes(station: (f64, f64), hours: u32, min_el: f64) -> usize {
        sat::passes(&iss(), station, ISS_EPOCH_UNIX, hours)
            .into_iter()
            .filter(|p| p.max_el_deg >= min_el)
            .count()
    }

    // ---------------------------------------------------------------------
    // The fixtures. Every pair below is chosen so that a NAIVE implementation
    // gets a DIFFERENT answer — co-located stations are never used to prove
    // anything about mutual visibility, because for them the two-observer
    // problem degenerates into the one-observer problem the rest of the stack
    // already solves.
    // ---------------------------------------------------------------------

    /// Chicago-ish (the operator's own grid in this tree's tests) and a station
    /// ~1,400 km east-north-east: far enough apart that their elevations differ
    /// by tens of degrees on the same pass, close enough that real windows
    /// exist. THE working pair.
    const EN52: &str = "EN52"; // ~41.5 N, 87 W
    const FN42: &str = "FN42"; // ~42.5 N, 71 W — Boston

    /// A transatlantic pair at ~5,700 km: each station sees the ISS many times
    /// a day, and it is NEVER up for both at once at any usable elevation.
    const IO91: &str = "IO91"; // ~51.5 N, 1 W — London

    /// Antipodal-ish: ~18,000 km. The far negative.
    const OF78: &str = "OF78"; // ~32.5 S, 116 E — Perth

    #[test]
    fn a_far_pair_with_passes_at_both_ends_has_no_mutual_window() {
        // THE headline negative, and the reason the naive reading is wrong: both
        // stations work this bird all day, and they can never work EACH OTHER
        // through it. The pair is 5,700 km apart; the ISS's 0° footprint radius
        // is ~2,200 km, so the two 0° circles cannot even touch.
        let (here, there) = (grid(EN52), grid(IO91));
        let sep = haversine_km(here, there);
        assert!(
            (5_000.0..6_500.0).contains(&sep),
            "fixture drift: EN52↔IO91 is {sep:.0} km"
        );

        // POSITIVE CONTROL — the negative below must be the geometry, never a
        // propagator that returned nothing. Each end has a full day of passes,
        // most of them good ones.
        let solo_here = solo_workable_passes(here, 24, MUTUAL_MIN_EL_DEG);
        let solo_there = solo_workable_passes(there, 24, MUTUAL_MIN_EL_DEG);
        assert!(
            solo_here >= 4 && solo_there >= 4,
            "control failed — the bird must be workable at each end alone \
             (here {solo_here}, there {solo_there})"
        );

        let w = mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, 24, MUTUAL_MIN_EL_DEG);
        assert!(
            w.is_empty(),
            "{} windows for a pair 5,700 km apart; first {:?}",
            w.len(),
            w.first()
        );
        // And not merely because of the 5° floor: there is no mutual instant at
        // the bare horizon either.
        let at_horizon = mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, 24, 0.0);
        assert!(
            at_horizon.is_empty(),
            "{} windows at a 0° floor — the footprints cannot overlap at this \
             separation, so this is a geometry bug",
            at_horizon.len()
        );
    }

    #[test]
    fn antipodal_stations_never_share_a_window() {
        let (here, there) = (grid(EN52), grid(OF78));
        assert!(haversine_km(here, there) > 15_000.0, "fixture drift");
        assert!(
            solo_workable_passes(there, 24, MUTUAL_MIN_EL_DEG) >= 4,
            "control failed — the far station must see the bird on its own"
        );
        assert!(mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, 24, 0.0).is_empty());
    }

    #[test]
    fn a_workable_pair_gets_windows_and_they_are_short_and_real() {
        let (here, there) = (grid(EN52), grid(FN42));
        let sep = haversine_km(here, there);
        assert!(
            (1_200.0..1_700.0).contains(&sep),
            "fixture drift: EN52↔FN42 is {sep:.0} km"
        );
        let w = mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, 24, MUTUAL_MIN_EL_DEG);
        assert!(!w.is_empty(), "a 1,400 km pair must get skeds in 24 h");
        for win in &w {
            assert!(
                win.secs() >= MIN_WINDOW_SECS,
                "a window shorter than the reporting floor escaped: {win:?}"
            );
            // A mutual window cannot outlast either station's own pass.
            assert!(
                win.secs() <= 15 * 60,
                "implausibly long LEO window: {win:?}"
            );
            assert!(win.start_unix <= win.peak_unix && win.peak_unix <= win.end_unix);
        }
    }

    #[test]
    fn the_window_is_exactly_where_both_stations_are_above_the_floor() {
        // The defining property, asserted POINTWISE through a different entry
        // point (`look_at`, the one-shot path) than the batch `pass_track` the
        // module scans with — inside the window the condition holds at every
        // sample, and just outside it does not. An off-by-one run boundary, an
        // `||` where the code needs `&&`, or a crossed observer all fail here.
        let (here, there) = (grid(EN52), grid(FN42));
        let w = mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, 24, MUTUAL_MIN_EL_DEG);
        assert!(!w.is_empty(), "control: the fixture must produce windows");
        let el = |station: (f64, f64), t: i64| sat::look_at(&iss(), station, t).map(|(_, e)| e);

        let mut outside_checked = 0;
        for win in &w {
            let mut t = win.start_unix;
            while t <= win.end_unix {
                let (a, b) = (el(here, t), el(there, t));
                assert!(
                    a.is_some_and(|e| e >= MUTUAL_MIN_EL_DEG - 1e-6)
                        && b.is_some_and(|e| e >= MUTUAL_MIN_EL_DEG - 1e-6),
                    "inside the window at t={t} the floor is not held: \
                     here {a:?}, there {b:?} ({win:?})"
                );
                t += 30;
            }
            // Maximality: two scan steps beyond each end the condition is gone.
            for probe in [
                win.start_unix - 2 * SCAN_STEP_SECS,
                win.end_unix + 2 * SCAN_STEP_SECS,
            ] {
                let (a, b) = (el(here, probe), el(there, probe));
                let both = a.is_some_and(|e| e >= MUTUAL_MIN_EL_DEG)
                    && b.is_some_and(|e| e >= MUTUAL_MIN_EL_DEG);
                assert!(
                    !both,
                    "the window is not maximal — at t={probe}, {} s outside, both \
                     stations still clear the floor (here {a:?}, there {b:?})",
                    (probe - win.start_unix).min(probe - win.end_unix).abs()
                );
                outside_checked += 1;
            }
        }
        assert!(outside_checked >= 2, "the maximality probe never ran");
    }

    #[test]
    fn mutual_elevations_match_an_independent_spherical_derivation() {
        // The reported peak elevations are checked against a derivation that
        // shares NO code with the SEZ transform: sub-satellite point → central
        // angle → look-angle relation. The two models differ only by the
        // ellipsoid, so they must agree to a fraction of a degree.
        let (here, there) = (grid(EN52), grid(FN42));
        let w = mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, 24, MUTUAL_MIN_EL_DEG);
        assert!(!w.is_empty(), "control: the fixture must produce windows");
        let mut worst: f64 = 0.0;
        let mut check = |label: &str, reported: f64, independent: f64, t: i64| {
            let err: f64 = (independent - reported).abs();
            worst = worst.max(err);
            assert!(
                err < 0.5,
                "{label}: reported {reported:.3}° vs spherical derivation \
                 {independent:.3}° at t={t} — {err:.3}° apart"
            );
        };
        for win in &w {
            // The shared ceiling, at the instant the module says it happens.
            let sub = sat::subpoint(&iss(), win.peak_unix).expect("subpoint at peak");
            let ceiling =
                elevation_from_subpoint(sub, here).min(elevation_from_subpoint(sub, there));
            check("ceiling", win.mutual_el_deg, ceiling, win.peak_unix);

            // The per-station maxima, re-derived over the same window on the
            // same 10 s grid the module scans on.
            let (mut mh, mut mt) = (f64::MIN, f64::MIN);
            let mut t = win.start_unix;
            while t <= win.end_unix {
                let s = sat::subpoint(&iss(), t).expect("subpoint inside window");
                mh = mh.max(elevation_from_subpoint(s, here));
                mt = mt.max(elevation_from_subpoint(s, there));
                t += SCAN_STEP_SECS;
            }
            check("max here", win.max_el_here_deg, mh, win.start_unix);
            check("max there", win.max_el_there_deg, mt, win.start_unix);
        }
        // A tolerance nothing ever approaches is a tolerance that proves
        // nothing; say what the real disagreement is.
        assert!(worst > 0.0, "the independent check never actually ran");
    }

    #[test]
    fn the_two_ends_are_not_interchangeable() {
        // ⚠️ THE ANTI-VACUITY TEST. Everything above would still pass with the
        // two observers accidentally identical, so this pins that the fixture
        // makes them differ a lot AND that the module keeps track of which is
        // which: swapping the arguments must swap the per-station maxima and
        // move the azimuth to the other station's sky, while leaving the window
        // and its shared ceiling alone.
        let (here, there) = (grid(EN52), grid(FN42));
        let fwd = mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, 24, MUTUAL_MIN_EL_DEG);
        let rev = mutual_windows(&iss(), there, here, ISS_EPOCH_UNIX, 24, MUTUAL_MIN_EL_DEG);
        assert_eq!(
            fwd.len(),
            rev.len(),
            "mutual visibility is symmetric — the window COUNT cannot depend on \
             argument order"
        );
        assert!(!fwd.is_empty(), "control: the fixture must produce windows");

        // The discriminating quantity is the per-station MAXIMUM, not the
        // elevation at the peak: measured on this fixture the two ends differ
        // by up to 46° on their maxima, and by under 2° at the peak (the peak
        // is where the two curves cross — see MutualWindow's docs).
        let spread = fwd
            .iter()
            .map(|w| (w.max_el_here_deg - w.max_el_there_deg).abs())
            .fold(0.0_f64, f64::max);
        assert!(
            spread > 20.0,
            "the fixture is too symmetric to test anything: the two ends' peak \
             elevations never differ by more than {spread:.1}°"
        );

        let mut az_moved = 0;
        for (f, r) in fwd.iter().zip(rev.iter()) {
            assert_eq!(
                f.start_unix, r.start_unix,
                "the window itself must be symmetric"
            );
            assert_eq!(f.end_unix, r.end_unix);
            assert!(
                (f.mutual_el_deg - r.mutual_el_deg).abs() < 1e-9,
                "the shared ceiling cannot depend on argument order: {f:?} vs {r:?}"
            );
            assert!(
                (f.max_el_here_deg - r.max_el_there_deg).abs() < 1e-9
                    && (f.max_el_there_deg - r.max_el_here_deg).abs() < 1e-9,
                "swapping the stations must swap the per-station maxima: {f:?} vs {r:?}"
            );
            // 1,473 km apart: the bird is never in the same bit of both skies.
            if (f.peak_az_here_deg - r.peak_az_here_deg).abs() > 5.0 {
                az_moved += 1;
            }
        }
        assert_eq!(
            az_moved,
            fwd.len(),
            "the reported azimuth did not follow the station it belongs to"
        );
    }

    #[test]
    fn a_pass_both_stations_work_well_can_still_have_no_mutual_window() {
        // THE non-obvious case, and the one the naive implementation gets
        // wrong: on the SAME pass both stations individually climb well past
        // 20°, their 0° intervals overlap for minutes — and there is no instant
        // at which both are at 20° together. Filtering each pass list by
        // max-elevation and intersecting the survivors would report this pass
        // as a 20° sked. It is not one.
        let (here, there) = (grid(EN52), grid(FN42));
        const HIGH: f64 = 20.0;

        let mine = sat::passes(&iss(), here, ISS_EPOCH_UNIX, 24);
        let theirs = sat::passes(&iss(), there, ISS_EPOCH_UNIX, 24);
        let mutual_high = mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, 24, HIGH);

        // Find a pass good at BOTH ends whose 0° intervals overlap — the naive
        // "this is a 20° sked" verdict.
        let mut naive_hits = 0;
        let mut refuted = 0;
        for a in &mine {
            if a.max_el_deg < HIGH {
                continue;
            }
            for b in &theirs {
                if b.max_el_deg < HIGH {
                    continue;
                }
                let lo = a.aos_unix.max(b.aos_unix);
                let hi = a.los_unix.min(b.los_unix);
                if hi - lo < MIN_WINDOW_SECS {
                    continue;
                }
                naive_hits += 1;
                // The honest answer for that same span.
                if !mutual_high
                    .iter()
                    .any(|w| w.start_unix < hi && w.end_unix > lo)
                {
                    refuted += 1;
                }
            }
        }
        assert!(
            naive_hits > 0,
            "control failed — the fixture no longer contains a pass both \
             stations work above {HIGH}° with overlapping horizon intervals, so \
             this test proves nothing"
        );
        assert!(
            refuted > 0,
            "{naive_hits} passes look like {HIGH}° skeds to a pass-list \
             intersection and every one of them turned out to be a real mutual \
             window — the fixture can no longer distinguish the two answers"
        );
    }

    #[test]
    fn raising_the_floor_shrinks_the_windows_it_does_not_move_them() {
        // The floor is load-bearing, asserted BY VALUE: a higher floor must
        // yield strictly less mutual time, and every high-floor window must sit
        // inside a low-floor one.
        // A week, not a day: at this separation a 20° mutual floor is
        // geometrically impossible (the 20° circle has a ~870 km ground radius,
        // so the ceiling on the pair is ~1,750 km against their 1,473 km) and a
        // 15° floor yields ONE 60 s window in 24 h — a control that thin cannot
        // tell a shrinking window from an empty one.
        let (here, there) = (grid(EN52), grid(FN42));
        const WEEK_H: u32 = 24 * 7;
        let total = |min_el: f64| -> i64 {
            mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, WEEK_H, min_el)
                .iter()
                .map(MutualWindow::secs)
                .sum()
        };
        let (at0, at5, at15) = (total(0.0), total(MUTUAL_MIN_EL_DEG), total(15.0));
        assert!(
            at0 > at5 && at5 > at15,
            "mutual seconds must fall as the floor rises: 0°={at0}, \
             {MUTUAL_MIN_EL_DEG}°={at5}, 15°={at15}"
        );
        assert!(
            at15 > 0,
            "control: the 15° case must not be vacuously empty"
        );

        let low = mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, WEEK_H, 0.0);
        let high = mutual_windows(&iss(), here, there, ISS_EPOCH_UNIX, WEEK_H, 15.0);
        assert!(
            !high.is_empty(),
            "control: the containment check must have input"
        );
        for hi in &high {
            assert!(
                low.iter()
                    .any(|l| l.start_unix <= hi.start_unix && l.end_unix >= hi.end_unix),
                "a 15° window is not contained in any 0° window: {hi:?}"
            );
            assert!(
                hi.mutual_el_deg >= 15.0,
                "a window found at a 15° floor reports a shared ceiling below \
                 it: {hi:?}"
            );
        }
    }

    #[test]
    fn a_bird_past_the_acting_ceiling_contributes_nothing() {
        let (here, there) = (grid(EN52), grid(FN42));
        let tle = iss();
        // 31 days past epoch: over the ceiling before the horizon even starts.
        let from = ISS_EPOCH_UNIX + 31 * 86_400;
        assert!(
            mutual_schedule(&[&tle], here, there, from, 7, MUTUAL_MIN_EL_DEG).is_empty(),
            "elements {ACT_CEILING_DAYS}+ days old must not be extrapolated"
        );
        // POSITIVE CONTROL: the same call at epoch DOES produce rows, so the
        // emptiness above is the ceiling and not a broken fixture.
        assert!(
            !mutual_schedule(&[&tle], here, there, ISS_EPOCH_UNIX, 7, MUTUAL_MIN_EL_DEG).is_empty()
        );
    }

    #[test]
    fn the_horizon_is_trimmed_to_the_acting_ceiling_part_way_along() {
        // Elements 27 days old with a 7-day ask: the first three days are
        // inside the ceiling and the rest is not, so the rows must stop — not
        // be absent, and not run the full week.
        let (here, there) = (grid(EN52), grid(FN42));
        let tle = iss();
        let from = ISS_EPOCH_UNIX + 27 * 86_400;
        let rows = mutual_schedule(&[&tle], here, there, from, 7, MUTUAL_MIN_EL_DEG);
        assert!(
            !rows.is_empty(),
            "three days of horizon remain — expected rows"
        );
        let last = rows.last().expect("checked non-empty").window.start_unix;
        let reach_days = (last - from) as f64 / 86_400.0;
        assert!(
            reach_days <= 3.0,
            "rows reached {reach_days:.2} days out on 27-day-old elements — past \
             the {ACT_CEILING_DAYS}-day ceiling"
        );
        // And the ceiling, not a coincidence, is what stopped them.
        assert!(
            reach_days > 1.0,
            "only {reach_days:.2} days of rows — the trim took too much"
        );
        for r in &rows {
            assert!(
                r.element_age_days > FIRM_AGE_DAYS && !r.firm,
                "a row on 27-day-old elements cannot be firm: {r:?}"
            );
        }
    }

    #[test]
    fn firmness_flips_at_the_fourteen_day_tier() {
        // The near rows are firm, the far rows are not, and the flip is at
        // FIRM_AGE_DAYS. A run where every row has the same firmness would be
        // an assertion that cannot differ — and that is exactly what asking
        // from EPOCH gives (ages 0…14 never cross the tier), so the ask starts
        // from five-day-old elements, which is also the realistic case: the
        // TLE cache is half-daily but a bird's elements are whatever Celestrak
        // last published.
        let (here, there) = (grid(EN52), grid(FN42));
        let tle = iss();
        let from = ISS_EPOCH_UNIX + 5 * 86_400;
        let rows = mutual_schedule(
            &[&tle],
            here,
            there,
            from,
            MAX_HORIZON_DAYS,
            MUTUAL_MIN_EL_DEG,
        );
        let firm = rows.iter().filter(|r| r.firm).count();
        let soft = rows.len() - firm;
        assert!(
            firm > 0 && soft > 0,
            "a 14-day horizon from epoch must straddle the {FIRM_AGE_DAYS}-day \
             tier (firm {firm}, soft {soft} of {})",
            rows.len()
        );
        for r in &rows {
            assert_eq!(
                r.firm,
                r.element_age_days <= FIRM_AGE_DAYS,
                "firmness disagrees with the age it is defined by: {r:?}"
            );
        }
    }

    #[test]
    fn the_schedule_is_time_ordered_across_birds() {
        // Two element sets in one call: the rows must interleave by time, not
        // come back grouped by bird.
        let (here, there) = (grid(EN52), grid(FN42));
        let a = iss();
        let mut b = iss();
        b.name = "ISS TWIN".to_string();
        let rows = mutual_schedule(&[&a, &b], here, there, ISS_EPOCH_UNIX, 2, MUTUAL_MIN_EL_DEG);
        assert!(rows.len() >= 4, "expected several rows, got {}", rows.len());
        assert!(
            rows.windows(2)
                .all(|p| p[0].window.start_unix <= p[1].window.start_unix),
            "rows are not in time order"
        );
        assert!(
            rows.windows(2).any(|p| p[0].sat_name != p[1].sat_name),
            "the two birds never interleaved — the sort is grouping by bird"
        );
        assert_eq!(
            rows[0].norad,
            Some(25544),
            "the catalog number must ride along"
        );
    }

    #[test]
    fn a_window_shorter_than_the_floor_is_not_reported() {
        // Driven on synthetic tracks so the floor is tested at exactly its
        // boundary rather than wherever the sky happens to put one.
        let track = |els: &[f64]| -> Vec<(i64, f64, f64)> {
            els.iter()
                .enumerate()
                .map(|(k, e)| (1_000 + k as i64 * SCAN_STEP_SECS, 90.0, *e))
                .collect()
        };
        // Five steps above the floor = 40 s span: under the 60 s floor.
        let short = track(&[0.0, 9.0, 9.0, 9.0, 9.0, 9.0, 0.0]);
        assert!(windows_from_tracks(&short, &short, 5.0).is_empty());
        // Seven steps = 60 s span: exactly at it.
        let ok = track(&[0.0, 9.0, 9.0, 9.0, 9.0, 9.0, 9.0, 9.0, 0.0]);
        let w = windows_from_tracks(&ok, &ok, 5.0);
        assert_eq!(w.len(), 1, "a window exactly at the floor must be reported");
        assert_eq!(w[0].secs(), MIN_WINDOW_SECS);
    }

    #[test]
    fn a_divergent_sample_does_not_split_a_window() {
        // One station's track is missing a sample in the middle (the shape
        // `pass_track` produces when the propagation diverges at one step).
        // That must not be read as the window ending and a new one starting.
        let full: Vec<(i64, f64, f64)> = (0..12)
            .map(|k| (1_000 + k * SCAN_STEP_SECS, 90.0, 30.0))
            .collect();
        let mut holed = full.clone();
        holed.remove(6);
        let w = windows_from_tracks(&full, &holed, 5.0);
        assert_eq!(w.len(), 1, "the hole split the window: {w:?}");
        assert_eq!(w[0].start_unix, full[0].0);
        assert_eq!(w[0].end_unix, full[11].0);
    }

    #[test]
    fn the_peak_is_the_best_mutual_instant_not_the_best_for_either_end() {
        // A hand-built pair where one station's best moment is NOT the pair's:
        // `here` peaks at t=3 (60°) while `there` is at 6°; the mutual optimum
        // is t=5, where the WORSE end is highest. Picking either station's own
        // maximum fails this.
        let here: Vec<(i64, f64, f64)> = [10.0, 30.0, 50.0, 60.0, 45.0, 25.0, 12.0, 7.0]
            .iter()
            .enumerate()
            .map(|(k, e)| (1_000 + k as i64 * SCAN_STEP_SECS, 45.0, *e))
            .collect();
        let there: Vec<(i64, f64, f64)> = [6.0, 6.0, 6.0, 6.0, 12.0, 22.0, 14.0, 6.0]
            .iter()
            .enumerate()
            .map(|(k, e)| (1_000 + k as i64 * SCAN_STEP_SECS, 270.0, *e))
            .collect();
        let w = windows_from_tracks(&here, &there, 5.0);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].peak_unix, 1_000 + 5 * SCAN_STEP_SECS);
        assert_eq!(
            w[0].mutual_el_deg, 22.0,
            "the ceiling is the WORSE end at the best instant"
        );
        assert_eq!(
            w[0].peak_az_here_deg, 45.0,
            "the azimuth must be THIS station's"
        );
        // The per-station maxima are elsewhere entirely — 60° at t=3 for
        // `here`, 22° at t=5 for `there` — and neither is the ceiling.
        assert_eq!(w[0].max_el_here_deg, 60.0);
        assert_eq!(w[0].max_el_there_deg, 22.0);
    }

    #[test]
    fn an_unusable_element_set_yields_nothing_rather_than_a_guess() {
        let junk = Tle {
            name: "GARBAGE".to_string(),
            line1: "1 99999U not a tle".to_string(),
            line2: "2 99999 nonsense".to_string(),
        };
        let (here, there) = (grid(EN52), grid(FN42));
        assert!(
            mutual_windows(&junk, here, there, ISS_EPOCH_UNIX, 24, MUTUAL_MIN_EL_DEG).is_empty()
        );
        assert!(
            mutual_schedule(&[&junk], here, there, ISS_EPOCH_UNIX, 7, MUTUAL_MIN_EL_DEG).is_empty()
        );
    }
}
