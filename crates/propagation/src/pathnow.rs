//! What is happening to a path, and to the bands, RIGHT NOW that the monthly-median model cannot
//! know: a flare's D-region absorption on the sunlit side and a proton event's polar-cap
//! absorption, laid over the model as display-only overlays. Nothing here changes a number the
//! P.533 or heuristic engine produced, and nothing here is kept with a logged contact.
//!
//! # Where the numbers come from
//!
//! - **The flare footprint** ([`footprint`]): NOAA's D-RAP grid while it is current (at most
//!   [`DRAP_FRESH_SECS`] past its `Product Valid At`), else the analytic D-RAP field from the GOES
//!   long-band flux (`10·log10(flux) + 65` MHz under the sun, `cos χ^0.75` away from it). The same
//!   rule the map's flare layer draws by (ui `flareNow.ts`), so the ribbon turns where the rings
//!   are. Only at R1 (M1) and above, only beside a GOES sample at most
//!   [`XRAY_FRESH_SECS`] old, and only on the sunlit side: the grid's night cells are the proton
//!   part, which the polar-cap overlay speaks for.
//! - **Proton absorption**: NOAA's D-RAP proton formula ([`crate::pca`]) on the GOES integral
//!   protons, poleward of the cap edge at the hour's Kp. Only during a NOAA S1+ proton event
//!   (J(≥10 MeV) ≥ 10 pfu, NOAA's own threshold) and beside a sample at most
//!   [`crate::pca::PROTON_FRESH_SECS`] old; with no Kp there is no cap edge, so nothing is claimed.
//!
//! **The grid's proton part is never flare absorption.** NOAA's grid folds a proton event's
//! absorption into the same HAF as the X-rays (D-RAP sums the two in dB). So while a proton event
//! is held ([`held_protons`]: S1+, a dated reading under 30 minutes old — the one rule both solar
//! phases use), a point poleward of [`CAP_FROM_GEOMAG_DEG`] where the grid holds more than the X-ray
//! part the GOES flux gives there (beyond Phase 1's agreement bound, [`SPLIT_TOL_DAY_MHZ`] /
//! [`SPLIT_TOL_TAPER_MHZ`]) takes that GOES X-ray part for the flare figure and its stretches. The
//! proton part is then counted once, as polar-cap absorption, when a Kp places the cap edge;
//! without one it is not counted, and the detail row says so. Each band says where its figure came
//! from ([`FlareFrom`]: NOAA's grid, the GOES estimate, or both), and the route says when the split
//! took GOES (`OverlaySource::flare_cap_geomag`). With no proton event held (a quiet, stale or
//! undated reading, or none), NOAA's grid stands everywhere as NOAA's word.
//!
//! Both laws give the absorption of a VERTICAL pass through the D region (1 dB at the HAF, and
//! `(HAF/f)^1.5` dB at f). A path crosses the D region twice per hop, obliquely: [`path_overlay`]
//! sums the crossings at the hop ends, each times the secant of the ray's incidence at 110 km
//! (P.533's own reference height for absorption), which the geometry bounds at ~5.25 for a grazing
//! take-off. Hops and take-off come from the model's mode this hour when it gives one, else from
//! [`ASSUMED_HOP_KM`] F2 hops, and the overlay says which. Every figure is an ESTIMATE from NOAA's
//! inputs, worded "estimated from NOAA's D-RAP grid" or "estimated from GOES" — never NOAA's own.
//!
//! # The "now" line
//!
//! [`space_now`] is the one generator of the band outlook's and the band-condition strip's "now"
//! line, so the shack, the Remote and the TV page say the same thing: which bands a flare takes
//! out under the sun and until when (NOAA's own recovery time when its grid gives one, else
//! NOAA's rule of thumb from the X-ray level), an S1+ proton event, and a NOAA geomagnetic watch.
//! The words are the UI's (every language); this decides the facts and carries their times.

use serde::Serialize;

use crate::arrival::Watch;
use crate::flare::{analytic_haf, DrapGrid, XRAY_FRESH_SECS};
use crate::geo;
use crate::likelihood::BandOutlook;
use crate::pathgeo::{route_points, segments_of, KpSource, PathGeoHour, PathSegment, SegmentKind};
use crate::route::Route;

/// The ten HF bands the overlay speaks for, at their FT8 watering holes (MHz) — the same table
/// the map's flare rings use (ui `flareNow.ts` `HF_BANDS`). 6 m is not among them: no flare's HAF
/// reaches 50 MHz.
pub const HF_BANDS: [(&str, f64); 10] = [
    ("160m", 1.84),
    ("80m", 3.573),
    ("60m", 5.357),
    ("40m", 7.074),
    ("30m", 10.136),
    ("20m", 14.074),
    ("17m", 18.1),
    ("15m", 21.074),
    ("12m", 24.915),
    ("10m", 28.074),
];

/// NOAA updates its D-RAP grid every minute; older than this it is not the picture of now (ui
/// `flareNow.ts` `DRAP_FRESH_SECS`).
pub const DRAP_FRESH_SECS: i64 = 180;
/// A grid stamped this far ahead of our clock is still taken (a clock a little behind NOAA's).
const DRAP_AHEAD_SECS: i64 = 120;
/// Poleward of this geomagnetic latitude a cell of NOAA's grid can hold proton absorption (the
/// lowest cap edge, 55°, less its 5° taper) — the polar-cap layer reads the grid's proton part from
/// the same line.
pub const CAP_FROM_GEOMAG_DEG: f64 = 50.0;
/// How far NOAA's grid may sit above the GOES X-ray field at a point and still be taken as X-rays
/// alone (the grid's own interpolation, NOAA's taper): Phase 1's agreement bounds, used here per
/// point — 1 MHz where the sun is high (cos χ ≥ 0.5) and 3 MHz in the taper toward the terminator,
/// where most polar crossings lie. They are the grid-vs-GOES success measure's MEDIAN limits, not a
/// per-cell fit; a point within them keeps NOAA's grid (the reading that holds most of the doubt).
pub const SPLIT_TOL_DAY_MHZ: f64 = 1.0;
pub const SPLIT_TOL_TAPER_MHZ: f64 = 3.0;

/// The bound at a point now: [`SPLIT_TOL_DAY_MHZ`] under a high sun, else [`SPLIT_TOL_TAPER_MHZ`].
fn split_tol_mhz(lat: f64, lon: f64, now: i64) -> f64 {
    if geo::solar_elevation_deg(lat, lon, now).to_radians().sin() >= 0.5 {
        SPLIT_TOL_DAY_MHZ
    } else {
        SPLIT_TOL_TAPER_MHZ
    }
}
/// 10 dB at f needs a local HAF of f·10^(2/3).
fn ten_db() -> f64 {
    10f64.powf(2.0 / 3.0)
}
/// Earth radius (km), the sphere [`geo`] uses.
const EARTH_KM: f64 = 6371.0;
/// The height (km) a ray's incidence is taken at for the D-region crossing: P.533's i₁₁₀.
const D_REGION_KM: f64 = 110.0;
/// Hop length (km) assumed when the model gives no mode for the hour: an F2 hop.
pub const ASSUMED_HOP_KM: f64 = 4000.0;
/// F2 reflection height (km) for an assumed hop's take-off angle.
const F2_HEIGHT_KM: f64 = 300.0;
/// The lowest take-off angle (degrees) a hop is given, P.533's minimum elevation.
const MIN_TAKEOFF_DEG: f64 = 3.0;

/// A band's FT8 frequency (MHz), for the ten bands of [`HF_BANDS`].
pub fn band_mhz(band: &str) -> Option<f64> {
    HF_BANDS.iter().find(|(b, _)| *b == band).map(|(_, f)| *f)
}

/// The GOES long-band X-ray sample the overlay runs on, with its own time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XraySample {
    pub flux: f32,
    pub at: i64,
}

/// The GOES integral protons (pfu) the overlay runs on, with the sample's own time — `None` when
/// the reading carried none (an undated reading is never an event: [`held_protons`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProtonSample {
    pub j1: f64,
    pub j5: f64,
    pub j10: f64,
    pub at: Option<i64>,
}

/// Where a flare footprint came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HafSource {
    /// NOAA's D-RAP grid, current.
    Noaa,
    /// The analytic D-RAP field from the GOES flux (no current NOAA grid).
    Goes,
}

/// Whose recovery time it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoverySource {
    /// NOAA's own `Estimated Recovery Time` from its current grid.
    Noaa,
    /// NOAA's empirical rule of thumb from the current X-ray level.
    RuleOfThumb,
}

/// When HF is estimated to recover from the flare, and whose estimate it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    pub at: i64,
    pub source: RecoverySource,
}

/// The flare footprint for now: NOAA's grid or the analytic field, sunlit side only.
#[derive(Debug, Clone, Copy)]
pub struct Footprint<'a> {
    pub source: HafSource,
    pub xray: XraySample,
    /// NOAA's grid, when it is the footprint.
    pub grid: Option<&'a DrapGrid>,
    pub now: i64,
}

impl Footprint<'_> {
    /// The HAF (MHz) at a point now; 0 where the sun is down.
    pub fn haf_at(&self, lat: f64, lon: f64) -> f64 {
        // X-rays are line-of-sight: only the sunlit side. (The grid's night cells are the proton
        // part, which the polar-cap overlay speaks for.)
        if geo::solar_elevation_deg(lat, lon, self.now) <= 0.0 {
            return 0.0;
        }
        match self.grid {
            Some(g) => f64::from(g.at(lat, lon)),
            None => f64::from(analytic_haf(lat, lon, self.now, self.xray.flux)),
        }
    }

    /// The HAF (MHz) at the sub-solar point now.
    pub fn haf_sub(&self) -> f64 {
        let (lat, lon) = geo::subsolar_point(self.now);
        match self.grid {
            Some(g) => f64::from(g.at(lat, lon)),
            None => f64::from(crate::model::flare_haf_mhz(self.xray.flux)),
        }
    }

    /// When HF recovers: NOAA's own time from its current grid while that is still ahead, else
    /// the rule of thumb on the current flux.
    pub fn recovery(&self) -> Option<Recovery> {
        if let Some(at) = self
            .grid
            .and_then(|g| g.recovery_at)
            .filter(|&at| at > self.now)
        {
            return Some(Recovery {
                at,
                source: RecoverySource::Noaa,
            });
        }
        recovery_min_rule_of_thumb(self.xray.flux).map(|mins| Recovery {
            at: self.now + mins.round() as i64 * 60,
            source: RecoverySource::RuleOfThumb,
        })
    }
}

/// NOAA's empirical fade-recovery rule of thumb (minutes) from the current long-band flux,
/// L = log10(flux): `32.19·L² + 323.45·L + 837.2` over M1–X5, continued above as `100·L + 450`
/// (X10 → 150 min). Below M1 NOAA's fit is relative to a background flux this does not have:
/// `None`. The map's `flareRecoveryMin` is the same function.
pub fn recovery_min_rule_of_thumb(flux: f32) -> Option<f64> {
    // NaN fails this too.
    if flux.is_nan() || flux < 1e-5 {
        return None;
    }
    let l = f64::from(flux).log10();
    Some(if l > -3.3 {
        100.0 * l + 450.0
    } else {
        32.19 * l * l + 323.45 * l + 837.2
    })
}

/// The footprint for `xray` at `now`, or `None` below M1, without a sample, or beside a sample
/// older than [`XRAY_FRESH_SECS`]. `grid` is NOAA's latest grid; it is the footprint only while
/// current.
pub fn footprint(
    xray: Option<XraySample>,
    grid: Option<&DrapGrid>,
    now: i64,
) -> Option<Footprint<'_>> {
    let xray = xray.filter(|x| {
        crate::model::r_scale(x.flux) >= 1
            && now - x.at < XRAY_FRESH_SECS
            && x.at - now <= DRAP_AHEAD_SECS
    })?;
    let grid = grid.filter(|g| {
        let age = now - g.valid_at;
        age < DRAP_FRESH_SECS && age > -DRAP_AHEAD_SECS
    });
    Some(Footprint {
        source: if grid.is_some() {
            HafSource::Noaa
        } else {
            HafSource::Goes
        },
        xray,
        grid,
        now,
    })
}

/// The sample, while it holds a proton event at `now` by THE one rule, [`crate::pca::proton_event`]
/// (S1+ from a finite J(≥10 MeV), dated, no more than [`crate::pca::PROTON_AHEAD_SECS`] ahead of
/// our clock and under [`crate::pca::PROTON_FRESH_SECS`] old); else `None`. The polar-cap layer
/// and the path overlay judge a reading by that one rule. It gates the PCA figure, the grid split
/// and the now line.
pub fn held_protons(p: Option<ProtonSample>, now: i64) -> Option<ProtonSample> {
    p.filter(|p| crate::pca::proton_event(p.j10, p.at, now))
}

/// The secant of a ray's incidence where it crosses the D region (110 km) for a take-off angle:
/// `sin i = R·cos Δ / (R + h)`. Bounded by the geometry (≈ 5.25 at the 3° minimum take-off).
fn d_secant(takeoff_deg: f64) -> f64 {
    let sin_i =
        EARTH_KM * takeoff_deg.max(MIN_TAKEOFF_DEG).to_radians().cos() / (EARTH_KM + D_REGION_KM);
    1.0 / (1.0 - sin_i * sin_i).sqrt()
}

/// The take-off angle (degrees) of an F2 hop of `hop_km` reflected at [`F2_HEIGHT_KM`].
fn hop_takeoff_deg(hop_km: f64) -> f64 {
    let half = hop_km / EARTH_KM / 2.0;
    ((half.cos() - EARTH_KM / (EARTH_KM + F2_HEIGHT_KM)) / half.sin())
        .atan()
        .to_degrees()
}

/// Hops and take-off for a band now: the model's mode for the hour when it gives one, else
/// assumed F2 hops of [`ASSUMED_HOP_KM`].
fn hops_for(b: &BandOutlook, now_hour: usize, dist_km: f64) -> (u8, f64, bool) {
    match b
        .detail
        .as_ref()
        .and_then(|d| d.get(now_hour).copied().flatten())
        .filter(|h| h.hops >= 1)
    {
        Some(h) => (h.hops, f64::from(h.takeoff_deg), true),
        None => {
            let n = (dist_km / ASSUMED_HOP_KM).ceil().max(1.0);
            (
                n.min(f64::from(u8::MAX)) as u8,
                hop_takeoff_deg(dist_km / n),
                false,
            )
        }
    }
}

/// Sum a vertical absorption over a route's D-region crossings: two per hop, at the hop ends
/// (the route's two ends once, every ground reflection twice), each times the secant.
fn over_crossings(
    me: (f64, f64),
    bearing: f64,
    dist_km: f64,
    hops: u8,
    secant: f64,
    vertical_db: impl Fn(f64, f64) -> f64,
) -> f64 {
    let n = usize::from(hops.max(1));
    (0..=n)
        .map(|k| {
            let (lat, lon) = geo::destination_point(me, bearing, dist_km * k as f64 / n as f64);
            let w = if k == 0 || k == n { 1.0 } else { 2.0 };
            w * vertical_db(lat, lon)
        })
        .sum::<f64>()
        * secant
}

/// One decimal, as the figures cross the IPC.
fn tenth(x: f64) -> f32 {
    ((x * 10.0).round() / 10.0) as f32
}

/// Where the overlay's inputs came from, each with its own time.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySource {
    /// The GOES sample the flare figures rest on, when a flare footprint stands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xray_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xray_long: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub haf: Option<HafSource>,
    /// NOAA's grid valid time, when the grid is the footprint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drap_valid_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Recovery>,
    /// The GOES proton sample, during an S1+ event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protons_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub j10: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s_scale: Option<u8>,
    /// Set (to [`CAP_FROM_GEOMAG_DEG`]) only when, on this route, NOAA's grid held more than the GOES
    /// X-ray part somewhere poleward of that geomagnetic latitude and the flare figure or its
    /// stretches took the GOES estimate there instead (during a held proton event).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flare_cap_geomag: Option<f32>,
}

/// Where a band's flare figure came from on a route: NOAA's grid at every crossing it counted,
/// the GOES estimate at every one (no current grid, or the split took it), or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FlareFrom {
    Noaa,
    Goes,
    Both,
}

/// One band's overlay on a route, now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandNow {
    pub band: String,
    /// Hops the crossings were counted for, and whether they are the model's mode this hour
    /// (false: assumed [`ASSUMED_HOP_KM`] F2 hops).
    pub hops: u8,
    pub hops_modelled: bool,
    /// Estimated flare absorption over the path (dB); present while a flare footprint stands
    /// (0 on a path the sun is not on).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flare_db: Option<f32>,
    /// Estimated proton (polar-cap) absorption over the path (dB); present during an S1+ event
    /// with a Kp to place the cap edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pca_db: Option<f32>,
    /// Where `flare_db` came from (present with it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flare_from: Option<FlareFrom>,
    /// The stretches of the route a map draws for this band now: ≥ 1 dB / ≥ 10 dB under the
    /// flare (vertical), ≥ 1 dB in the polar cap.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<PathSegment>,
}

/// What a flare and a proton event do to one route NOW, beside the model's hour — the overlay on
/// the current hour's `PathHour`s. Display only: never kept with a contact.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathHourOverlay {
    /// The instant it describes.
    pub unix: i64,
    /// The hour's Kp and where it came from (the geometry's), which placed the cap edge.
    pub kp: Option<f32>,
    pub kp_source: KpSource,
    pub cap_lat_deg: Option<f32>,
    pub source: OverlaySource,
    pub bands: Vec<BandNow>,
}

/// The overlay for one route `me` → `dx` at `now`, for the prediction's `bands` (their mode at
/// `now_hour` gives the hops). `geo_now` is the route's geometry for the current hour (its Kp).
/// `None` when there is no flare footprint and no S1+ proton event: nothing to lay over.
#[allow(clippy::too_many_arguments)]
pub fn path_overlay(
    me: (f64, f64),
    dx: (f64, f64),
    route: Route,
    bands: &[BandOutlook],
    geo_now: &PathGeoHour,
    now_hour: usize,
    now: i64,
    flare: Option<&Footprint>,
    protons: Option<ProtonSample>,
) -> Option<PathHourOverlay> {
    // A proton event is held only at S1+, beside a fresh dated sample ([`held_protons`]).
    let protons = held_protons(protons, now);
    // NOAA's grid during a held proton event: its polar excess over the X-ray part is the proton
    // part (see the header). Otherwise the grid stands as NOAA's word.
    let split = protons.is_some() && flare.is_some_and(|fp| fp.grid.is_some());
    // The PCA figure also needs a Kp to place the cap edge.
    let pca = protons.zip(geo_now.kp);
    // Whether the split took the GOES estimate anywhere on this route.
    let took_goes = std::cell::Cell::new(false);
    // The flare's HAF at a point, and whether it is the GOES estimate (no grid, or the split took it).
    let flare_haf = |fp: &Footprint, la: f64, lo: f64| -> (f64, bool) {
        let h = fp.haf_at(la, lo);
        if fp.grid.is_none() {
            return (h, true);
        }
        if split && h > 0.0 && geo::geomagnetic_lat_deg(la, lo).abs() >= CAP_FROM_GEOMAG_DEG {
            let x = f64::from(analytic_haf(la, lo, now, fp.xray.flux));
            if h > x + split_tol_mhz(la, lo, now) {
                took_goes.set(true);
                return (x, true);
            }
        }
        (h, false)
    };
    if flare.is_none() && pca.is_none() {
        return None;
    }
    let (pts, dist_km, bearing) = route_points(me, dx, route);
    let pca_vertical = |lat: f64, lon: f64, f: f64| {
        pca.map_or(0.0, |(p, kp)| {
            crate::pca::absorption_db(lat, lon, now, f, p.j5, p.j1, f64::from(kp))
        })
    };
    let hafs: Vec<f64> = match flare {
        Some(fp) => pts
            .iter()
            .map(|&(la, lo)| flare_haf(fp, la, lo).0)
            .collect(),
        None => Vec::new(),
    };
    let out = bands
        .iter()
        .filter_map(|b| {
            let f = band_mhz(&b.band)?;
            let (hops, takeoff, hops_modelled) = hops_for(b, now_hour, dist_km);
            let sec = d_secant(takeoff);
            // Which source the crossings that absorb anything took.
            let (noaa_used, goes_used) = (std::cell::Cell::new(false), std::cell::Cell::new(false));
            let flare_db = flare.map(|fp| {
                tenth(over_crossings(me, bearing, dist_km, hops, sec, |la, lo| {
                    let (haf, goes) = flare_haf(fp, la, lo);
                    if haf > 0.0 {
                        if goes { &goes_used } else { &noaa_used }.set(true);
                        (haf / f).powf(1.5)
                    } else {
                        0.0
                    }
                }))
            });
            let flare_from = flare.map(|fp| match (noaa_used.get(), goes_used.get()) {
                (true, true) => FlareFrom::Both,
                (false, true) => FlareFrom::Goes,
                (true, false) => FlareFrom::Noaa,
                // Nothing absorbed (a path the sun is not on): the footprint's own source.
                (false, false) if fp.grid.is_some() => FlareFrom::Noaa,
                (false, false) => FlareFrom::Goes,
            });
            let pca_db = pca.map(|_| {
                tenth(over_crossings(me, bearing, dist_km, hops, sec, |la, lo| {
                    pca_vertical(la, lo, f)
                }))
            });
            // The stretches a map draws: the vertical-pass lines the flare rings are (1 dB where
            // HAF ≥ f, 10 dB where HAF ≥ f·10^(2/3)), and ≥ 1 dB of proton absorption.
            let mut segments = Vec::new();
            if !hafs.is_empty() {
                let weak: Vec<bool> = hafs.iter().map(|&h| h > 0.0 && h >= f).collect();
                let gone: Vec<bool> = hafs.iter().map(|&h| h > 0.0 && h >= f * ten_db()).collect();
                segments_of(&weak, SegmentKind::FlareWeak, &mut segments);
                segments_of(&gone, SegmentKind::FlareOut, &mut segments);
            }
            if pca.is_some() {
                let cap: Vec<bool> = pts
                    .iter()
                    .map(|&(la, lo)| pca_vertical(la, lo, f) >= 1.0)
                    .collect();
                segments_of(&cap, SegmentKind::Pca, &mut segments);
            }
            Some(BandNow {
                band: b.band.clone(),
                hops,
                hops_modelled,
                flare_db,
                pca_db,
                flare_from,
                segments,
            })
        })
        .collect();
    let mut source = OverlaySource::default();
    if let Some(fp) = flare {
        source.xray_at = Some(fp.xray.at);
        source.xray_long = Some(fp.xray.flux);
        source.haf = Some(fp.source);
        source.drap_valid_at = fp.grid.map(|g| g.valid_at);
        source.flare_cap_geomag = took_goes.get().then_some(CAP_FROM_GEOMAG_DEG as f32);
        source.recovery = fp.recovery();
    }
    if let Some((p, _)) = pca {
        source.protons_at = p.at;
        source.j10 = Some(p.j10 as f32);
        source.s_scale = Some(crate::pca::s_scale(p.j10));
    }
    Some(PathHourOverlay {
        unix: now,
        kp: geo_now.kp,
        kp_source: geo_now.kp_source,
        cap_lat_deg: geo_now.cap_lat_deg,
        source,
        bands: out,
    })
}

/// The highest band out (≥ 10 dB) and weakened (≥ 1 dB) for a local HAF — the map's
/// `flareUnderSun` rule.
fn bands_at(haf: f64) -> (Option<String>, Option<String>) {
    let mut out = None;
    let mut weak = None;
    for (b, f) in HF_BANDS {
        if haf >= f * ten_db() {
            out = Some(b.to_string());
        }
        if haf >= f {
            weak = Some(b.to_string());
        }
    }
    (out, weak)
}

/// A flare now, for the "now" line.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowFlare {
    /// NOAA's R level of the flux.
    pub r: u8,
    pub xray_long: f32,
    pub xray_at: i64,
    pub haf: HafSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drap_valid_at: Option<i64>,
    /// HAF under the sun (MHz).
    pub haf_sub_mhz: f32,
    /// The highest band out (≥ 10 dB) and weakened (≥ 1 dB) under the sun.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weak: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Recovery>,
}

/// An S1+ proton event now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowProtons {
    pub s: u8,
    pub j10: f32,
    pub at: i64,
}

/// A NOAA geomagnetic watch in force or ahead.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowWatch {
    pub g: u8,
    /// Start of its first G1+ day and end of its last (Unix seconds UTC).
    pub from: i64,
    pub to: i64,
    /// NOAA's issue time.
    pub issued: i64,
}

/// The "now" line's facts: what is happening that the model's climatology does not include.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceNow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flare: Option<NowFlare>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protons: Option<NowProtons>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch: Option<NowWatch>,
}

/// The "now" line at `now`, or `None` when nothing is happening (no flare at M1+, no S1+ proton
/// event, no watch in force or ahead).
pub fn space_now(
    flare: Option<&Footprint>,
    protons: Option<ProtonSample>,
    watch: Option<&Watch>,
    now: i64,
) -> Option<SpaceNow> {
    let flare = flare.map(|fp| {
        let haf = fp.haf_sub();
        let (out, weak) = bands_at(haf);
        NowFlare {
            r: crate::model::r_scale(fp.xray.flux),
            xray_long: fp.xray.flux,
            xray_at: fp.xray.at,
            haf: fp.source,
            drap_valid_at: fp.grid.map(|g| g.valid_at),
            haf_sub_mhz: tenth(haf),
            out,
            weak,
            recovery: fp.recovery(),
        }
    });
    let protons = held_protons(protons, now).map(|p| NowProtons {
        s: crate::pca::s_scale(p.j10),
        j10: p.j10 as f32,
        // An event is always dated (`held_protons`).
        at: p.at.unwrap_or(now),
    });
    let watch = watch.and_then(|w| {
        let (from, to) = w.window?;
        (to > now && w.max_g >= 1).then_some(NowWatch {
            g: w.max_g,
            from,
            to,
            issued: w.issued,
        })
    });
    (flare.is_some() || protons.is_some() || watch.is_some()).then_some(SpaceNow {
        flare,
        protons,
        watch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrival::WatchDay;
    use crate::likelihood::PathHour;
    use crate::pathgeo::path_geometry_route;

    /// 2026-05-15 12:00 UTC: the sun is over the Atlantic/Africa side (~0° E).
    const MAY_12Z: i64 = 1_778_846_400;
    const BOSTON: (f64, f64) = (42.36, -71.06);
    const LONDON: (f64, f64) = (51.5, -0.12);
    const TOKYO: (f64, f64) = (35.68, 139.69);
    const SYDNEY: (f64, f64) = (-33.87, 151.21);
    const SINGAPORE: (f64, f64) = (1.35, 103.82);
    const NAIROBI: (f64, f64) = (-1.29, 36.82);
    const X1: f32 = 1e-4;

    fn x(flux: f32, at: i64) -> Option<XraySample> {
        Some(XraySample { flux, at })
    }

    /// A NOAA grid with `haf` MHz in every cell, valid at `valid_at`.
    fn grid(haf: f32, valid_at: i64, recovery_at: Option<i64>) -> DrapGrid {
        let lats: Vec<f32> = (0..90).map(|i| 89.0 - 2.0 * i as f32).collect();
        let lons: Vec<f32> = (0..90).map(|i| -178.0 + 4.0 * i as f32).collect();
        DrapGrid {
            valid_at,
            recovery_at,
            haf: vec![haf; lats.len() * lons.len()],
            lats,
            lons,
        }
    }

    fn band(name: &str, detail: Option<(u8, f32)>) -> BandOutlook {
        BandOutlook {
            band: name.to_string(),
            workability: "Fair".into(),
            score: 0.5,
            window: String::new(),
            grayline: false,
            hourly: vec![0.5; 24],
            reliability: 50.0,
            mode_now: Vec::new(),
            mode_hourly: Vec::new(),
            detail: detail.map(|(hops, takeoff_deg)| {
                vec![
                    Some(PathHour {
                        layer: crate::likelihood::PathLayer::F2,
                        hops,
                        takeoff_deg,
                        snr_db: 0.0,
                        estimated: false,
                    });
                    24
                ]
            }),
        }
    }

    fn bands() -> Vec<BandOutlook> {
        ["80m", "40m", "20m", "10m"]
            .iter()
            .map(|b| band(b, None))
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn overlay(
        me: (f64, f64),
        dx: (f64, f64),
        route: Route,
        bands: &[BandOutlook],
        kp: Option<f32>,
        now: i64,
        flare: Option<&Footprint>,
        protons: Option<ProtonSample>,
    ) -> Option<PathHourOverlay> {
        let g = path_geometry_route(me, dx, route, now, kp, None, None);
        path_overlay(
            me,
            dx,
            route,
            bands,
            &g.hours[g.now_hour as usize],
            g.now_hour as usize,
            now,
            flare,
            protons,
        )
    }

    fn of<'a>(o: &'a PathHourOverlay, b: &str) -> &'a BandNow {
        o.bands.iter().find(|x| x.band == b).unwrap()
    }

    fn kinds(b: &BandNow) -> Vec<SegmentKind> {
        b.segments.iter().map(|s| s.kind).collect()
    }

    fn storm(j10: f64, at: i64) -> Option<ProtonSample> {
        // An S2-class event: the ≥5 and ≥1 MeV channels sit above the ≥10.
        Some(ProtonSample {
            j1: j10 * 4.0,
            j5: j10 * 2.0,
            j10,
            at: Some(at),
        })
    }

    #[test]
    fn the_sub_solar_point_is_where_the_sun_is_overhead() {
        for t in [MAY_12Z, MAY_12Z + 7 * 3600, MAY_12Z + 200 * 86_400 + 1234] {
            let (lat, lon) = geo::subsolar_point(t);
            let el = geo::solar_elevation_deg(lat, lon, t);
            assert!(el > 89.9, "{t}: {el}");
            assert!((-180.0..180.0).contains(&lon));
        }
    }

    #[test]
    fn no_footprint_below_m1_without_a_sample_or_beside_an_old_one() {
        assert!(footprint(x(9.9e-6, MAY_12Z), None, MAY_12Z).is_none());
        assert!(footprint(None, None, MAY_12Z).is_none());
        // 10 minutes old: the map no longer uses it, nor does the overlay.
        assert!(footprint(x(X1, MAY_12Z - XRAY_FRESH_SECS), None, MAY_12Z).is_none());
        // Positive controls: M1 and a sample 9 minutes old.
        assert!(footprint(x(1e-5, MAY_12Z), None, MAY_12Z).is_some());
        assert!(footprint(x(X1, MAY_12Z - 540), None, MAY_12Z).is_some());
    }

    #[test]
    fn noaas_grid_is_the_footprint_while_current_and_never_on_the_night_side() {
        let g = grid(30.0, MAY_12Z - 60, None);
        let f = footprint(x(X1, MAY_12Z), Some(&g), MAY_12Z).unwrap();
        assert_eq!(f.source, HafSource::Noaa);
        // NOAA's 30 MHz under the sun, not GOES's 25.
        assert!((f.haf_sub() - 30.0).abs() < 0.01, "{}", f.haf_sub());
        // The night side of the grid is the proton part: no flare there.
        assert_eq!(f.haf_at(0.0, 180.0), 0.0);
        // Three minutes old: the analytic field from GOES stands in, labelled so.
        let old = grid(30.0, MAY_12Z - DRAP_FRESH_SECS, None);
        let f = footprint(x(X1, MAY_12Z), Some(&old), MAY_12Z).unwrap();
        assert_eq!(f.source, HafSource::Goes);
        assert!((f.haf_sub() - 25.0).abs() < 0.05, "{}", f.haf_sub());
        assert!(f.grid.is_none());
        // The analytic field is also dark at night.
        assert_eq!(f.haf_at(0.0, 180.0), 0.0);
    }

    #[test]
    fn recovery_is_noaas_own_while_its_grid_gives_one_else_the_rule_of_thumb() {
        let own = grid(30.0, MAY_12Z - 30, Some(MAY_12Z + 42 * 60));
        let f = footprint(x(X1, MAY_12Z), Some(&own), MAY_12Z).unwrap();
        assert_eq!(
            f.recovery(),
            Some(Recovery {
                at: MAY_12Z + 42 * 60,
                source: RecoverySource::Noaa
            })
        );
        // NOAA's time already past: the rule of thumb on the flux now (X1 ≈ 58 min).
        let past = grid(30.0, MAY_12Z - 30, Some(MAY_12Z - 60));
        let r = footprint(x(X1, MAY_12Z), Some(&past), MAY_12Z)
            .unwrap()
            .recovery()
            .unwrap();
        assert_eq!(r.source, RecoverySource::RuleOfThumb);
        assert_eq!(r.at, MAY_12Z + 58 * 60);
        // No grid: the rule of thumb too.
        let r = footprint(x(X1, MAY_12Z), None, MAY_12Z)
            .unwrap()
            .recovery()
            .unwrap();
        assert_eq!(r.source, RecoverySource::RuleOfThumb);
        // The fit's anchors (the map's flareRecoveryMin): M1 ≈ 25, X10 → 150, none below M1.
        assert_eq!(recovery_min_rule_of_thumb(1e-5).map(f64::round), Some(25.0));
        assert_eq!(
            recovery_min_rule_of_thumb(1e-3).map(f64::round),
            Some(150.0)
        );
        assert_eq!(recovery_min_rule_of_thumb(9e-6), None);
    }

    #[test]
    fn s_scale_follows_noaas_thresholds_and_old_protons_do_not_speak() {
        assert_eq!(crate::pca::s_scale(9.9), 0);
        assert_eq!(crate::pca::s_scale(10.0), 1);
        assert_eq!(crate::pca::s_scale(150.0), 2);
        assert_eq!(crate::pca::s_scale(1e3), 3);
        assert_eq!(crate::pca::s_scale(1e5), 5);
        assert!(held_protons(
            storm(150.0, MAY_12Z - crate::pca::PROTON_FRESH_SECS),
            MAY_12Z
        )
        .is_none());
        assert!(held_protons(storm(150.0, MAY_12Z - 300), MAY_12Z).is_some());
        assert!(held_protons(None, MAY_12Z).is_none());
    }

    #[test]
    fn a_dayside_path_under_a_flare_loses_more_on_the_lower_band_and_a_night_path_loses_none() {
        // Boston–London at 12Z: daylight all the way.
        let f = footprint(x(X1, MAY_12Z), None, MAY_12Z).unwrap();
        let o = overlay(
            BOSTON,
            LONDON,
            Route::Short,
            &bands(),
            Some(2.0),
            MAY_12Z,
            Some(&f),
            None,
        )
        .unwrap();
        let (b40, b20) = (of(&o, "40m"), of(&o, "20m"));
        let (a40, a20) = (b40.flare_db.unwrap(), b20.flare_db.unwrap());
        assert!(a20 > 1.0, "{a20}");
        // The D-RAP law: (f20/f40)^1.5 ≈ 2.8 times more at 40 m.
        assert!(
            (a40 / a20 - (14.074f32 / 7.074).powf(1.5)).abs() < 0.05,
            "{a40} {a20}"
        );
        assert_eq!(o.source.haf, Some(HafSource::Goes));
        assert_eq!(o.source.xray_at, Some(MAY_12Z));
        assert!(o.source.recovery.is_some());
        // 80 m is out (≥ 10 dB on a vertical pass) under the sun near London, 40 m weakened
        // there but not out (X1 tops out at 25 MHz; 40 m is out only from ~33), and 10 m, above
        // the HAF everywhere, is not touched.
        assert!(kinds(of(&o, "80m")).contains(&SegmentKind::FlareOut));
        assert!(
            kinds(b40).contains(&SegmentKind::FlareWeak),
            "{:?}",
            b40.segments
        );
        assert!(!kinds(b40).contains(&SegmentKind::FlareOut));
        assert!(of(&o, "10m").segments.is_empty());
        // Singapore–Sydney at 12Z is dark end to end: the flare takes nothing there.
        let n = overlay(
            SINGAPORE,
            SYDNEY,
            Route::Short,
            &bands(),
            Some(2.0),
            MAY_12Z,
            Some(&f),
            None,
        )
        .unwrap();
        assert_eq!(of(&n, "40m").flare_db, Some(0.0));
        assert!(of(&n, "40m").segments.is_empty());
    }

    #[test]
    fn noaas_grid_shapes_the_figure_when_it_is_the_footprint() {
        let g = grid(30.0, MAY_12Z - 60, None);
        let noaa = footprint(x(X1, MAY_12Z), Some(&g), MAY_12Z).unwrap();
        let goes = footprint(x(X1, MAY_12Z), None, MAY_12Z).unwrap();
        let a = |f: &Footprint| {
            let o = overlay(
                BOSTON,
                LONDON,
                Route::Short,
                &bands(),
                Some(2.0),
                MAY_12Z,
                Some(f),
                None,
            )
            .unwrap();
            (of(&o, "20m").flare_db.unwrap(), o.source)
        };
        let (n, ns) = a(&noaa);
        let (e, _) = a(&goes);
        assert!(
            n > e,
            "NOAA's flat 30 MHz grid outweighs the tapered GOES field: {n} {e}"
        );
        assert_eq!(ns.haf, Some(HafSource::Noaa));
        assert_eq!(ns.drap_valid_at, Some(MAY_12Z - 60));
    }

    #[test]
    fn more_hops_cross_the_d_region_more_often() {
        let f = footprint(x(X1, MAY_12Z), None, MAY_12Z).unwrap();
        let at = |hops: u8| {
            let b = [band("20m", Some((hops, 10.0)))];
            let o = overlay(
                BOSTON,
                LONDON,
                Route::Short,
                &b,
                Some(2.0),
                MAY_12Z,
                Some(&f),
                None,
            )
            .unwrap();
            let b = &o.bands[0];
            assert!(b.hops_modelled);
            assert_eq!(b.hops, hops);
            b.flare_db.unwrap()
        };
        assert!(at(3) > at(2), "{} {}", at(3), at(2));
        // No mode from the model: assumed 4000 km F2 hops, and it says so.
        let o = overlay(
            BOSTON,
            LONDON,
            Route::Short,
            &bands(),
            Some(2.0),
            MAY_12Z,
            Some(&f),
            None,
        )
        .unwrap();
        assert!(!of(&o, "20m").hops_modelled);
        assert_eq!(of(&o, "20m").hops, 2);
    }

    #[test]
    fn every_crossing_counts_once_at_the_ends_twice_between_and_times_its_secant() {
        // NOAA's grid at 30 MHz everywhere: on Boston–London at 12Z (daylight end to end) every
        // crossing has the same vertical loss, (30/14.074)^1.5 dB on 20 m.
        let g = grid(30.0, MAY_12Z - 60, None);
        let f = footprint(x(X1, MAY_12Z), Some(&g), MAY_12Z).unwrap();
        let vertical = (30.0f64 / 14.074).powf(1.5);
        let sec = |deg: f64| {
            let sin_i = 6371.0 * deg.to_radians().cos() / (6371.0 + 110.0);
            1.0 / (1.0 - sin_i * sin_i).sqrt()
        };
        for (hops, takeoff) in [(1u8, 5.0f32), (2, 12.0), (3, 30.0)] {
            let b = [band("20m", Some((hops, takeoff)))];
            let o = overlay(
                BOSTON,
                LONDON,
                Route::Short,
                &b,
                Some(2.0),
                MAY_12Z,
                Some(&f),
                None,
            )
            .unwrap();
            let want = 2.0 * f64::from(hops) * vertical * sec(f64::from(takeoff));
            let got = f64::from(o.bands[0].flare_db.unwrap());
            assert!(
                (got - want).abs() < 0.06,
                "{hops} hops at {takeoff}°: {got} vs {want}"
            );
        }
        // A grazing ray crosses more of the layer than a steep one, but never without bound.
        assert!(sec(3.0) > 5.0 && sec(3.0) < 5.5 && sec(30.0) < 2.0);
    }

    /// NOAA's grid folds a proton event's absorption into the same HAF as the flare's. A grid that
    /// is the analytic X1 field at `t` everywhere, and (capped) the same plus 15 MHz at
    /// |geomagnetic lat| ≥ 60° in either hemisphere (the proton part), valid 30 s before `t`.
    fn proton_grid(capped: bool, t: i64) -> DrapGrid {
        let mut g = grid(0.0, t - 30, None);
        let cols = g.lons.len();
        for (r, la) in g.lats.iter().enumerate() {
            for (c, lo) in g.lons.iter().enumerate() {
                let (la, lo) = (f64::from(*la), f64::from(*lo));
                let cap = capped && geo::geomagnetic_lat_deg(la, lo).abs() >= 60.0;
                g.haf[r * cols + c] =
                    analytic_haf(la, lo, t - 30, X1) + if cap { 15.0 } else { 0.0 };
            }
        }
        g
    }

    const TROMSO: (f64, f64) = (69.65, 18.96);
    const FAIRBANKS: (f64, f64) = (64.84, -147.72);
    const OSLO: (f64, f64) = (59.91, 10.75);
    const ANCHORAGE: (f64, f64) = (61.22, -149.9);
    /// Two Antarctic stations, both deep in the southern cap and sunlit in December.
    const MAWSON: (f64, f64) = (-67.6, 62.87);
    const CASEY: (f64, f64) = (-66.28, 110.53);
    /// 2026-12-15 12:00 UTC: southern summer.
    const DEC_12Z: i64 = 1_797_336_000;

    type Stretches = Vec<(SegmentKind, f32, f32)>;
    type FlarePart = (f32, Stretches, Option<f32>, Option<FlareFrom>, Option<f32>);
    type Ends = ((f64, f64), (f64, f64), i64);

    /// 20m on a path at `t` with NOAA's grid `g` (None: the GOES estimate alone) and `protons`:
    /// the flare figure, its flare stretches, the PCA figure, where the flare figure came from, and
    /// the route's split flag.
    fn flare_part(
        me: (f64, f64),
        dx: (f64, f64),
        t: i64,
        g: Option<&DrapGrid>,
        protons: Option<ProtonSample>,
    ) -> FlarePart {
        let f = footprint(x(X1, t - 30), g, t).unwrap();
        assert_eq!(f.source == HafSource::Noaa, g.is_some());
        let b = [band("20m", None)];
        let o = overlay(me, dx, Route::Short, &b, Some(2.0), t, Some(&f), protons).unwrap();
        // Every input keeps its own time, never the ask's.
        assert_eq!(o.source.xray_at, Some(t - 30));
        assert_eq!(o.source.drap_valid_at, g.map(|_| t - 30));
        let segs = o.bands[0]
            .segments
            .iter()
            .filter(|s| matches!(s.kind, SegmentKind::FlareWeak | SegmentKind::FlareOut))
            .map(|s| (s.kind, s.from, s.to))
            .collect();
        let b = &o.bands[0];
        (
            b.flare_db.unwrap(),
            segs,
            b.pca_db,
            b.flare_from,
            o.source.flare_cap_geomag,
        )
    }

    fn same_stretches(a: &[(SegmentKind, f32, f32)], b: &[(SegmentKind, f32, f32)], what: &str) {
        assert_eq!(a.len(), b.len(), "{what}: {a:?} vs {b:?}");
        for (x, y) in a.iter().zip(b) {
            assert_eq!(x.0, y.0, "{what}");
            assert!(
                (x.1 - y.1).abs() <= 0.02 && (x.2 - y.2).abs() <= 0.02,
                "{what}: {x:?} {y:?}"
            );
        }
    }

    const POLAR_PATHS: [Ends; 4] = [
        (TROMSO, FAIRBANKS, MAY_12Z),
        (OSLO, ANCHORAGE, MAY_12Z),
        (LONDON, TOKYO, MAY_12Z),
        (MAWSON, CASEY, DEC_12Z),
    ];

    #[test]
    fn with_a_proton_reading_the_flare_figure_is_the_x_ray_part_alone_in_both_caps() {
        // The oracle is independent of the split: the GOES estimate with no grid at all, which
        // never goes through it.
        for (me, dx, t) in POLAR_PATHS {
            let s2 = storm(200.0, t - 300);
            let (oracle, oracle_segs, oracle_pca, _, _) = flare_part(me, dx, t, None, s2);
            let capped = proton_grid(true, t);
            let (got, segs, pca, _, flag) = flare_part(me, dx, t, Some(&capped), s2);
            assert!(
                (got - oracle).abs() <= 0.3,
                "{me:?}: {got} vs X-ray part {oracle}"
            );
            same_stretches(&segs, &oracle_segs, "capped vs GOES");
            // The proton part is counted once, as PCA, and the route says the split took GOES.
            assert_eq!(pca, oracle_pca);
            assert!(pca.unwrap() > 1.0);
            assert_eq!(flag, Some(50.0), "{me:?}");
            // A proton-free grid with no proton reading at all is NOAA's word, and agrees.
            let plain = proton_grid(false, t);
            let (noaa, noaa_segs, _, from, flag) = flare_part(me, dx, t, Some(&plain), None);
            assert!((noaa - oracle).abs() <= 0.3, "{me:?}: {noaa} vs {oracle}");
            same_stretches(&noaa_segs, &oracle_segs, "proton-free grid vs GOES");
            assert_eq!((from, flag), (Some(FlareFrom::Noaa), None));
        }
    }

    #[test]
    fn only_a_held_proton_event_splits_the_grid_otherwise_it_stands_as_noaas_word() {
        // The one rule both solar phases use for "a proton event is held": S1+, at a dated
        // reading under `pca::PROTON_FRESH_SECS` old. A quiet reading (fresh or six hours old) or a stale
        // S1+ one is no event, so NOAA's grid stays the flare's word — never our lower estimate.
        for (me, dx, t) in POLAR_PATHS {
            let capped = proton_grid(true, t);
            let (grid_word, grid_segs, _, _, _) = flare_part(me, dx, t, Some(&capped), None);
            for (what, reading) in [
                ("quiet, fresh", storm(0.3, t - 300)),
                ("quiet, 6 h old", storm(0.3, t - 6 * 3600)),
                ("5 pfu tail", storm(5.0, t - 300)),
                ("S2, 31 min old", storm(200.0, t - 31 * 60)),
                ("S2, 6 h old", storm(200.0, t - 6 * 3600)),
            ] {
                let (got, segs, pca, from, flag) = flare_part(me, dx, t, Some(&capped), reading);
                assert_eq!(got, grid_word, "{me:?} {what}");
                same_stretches(&segs, &grid_segs, what);
                assert_eq!(
                    (pca, from, flag),
                    (None, Some(FlareFrom::Noaa), None),
                    "{me:?} {what}"
                );
            }
            // Control: a fresh S2 reading is an event, and the split takes the X-ray part.
            let (oracle, _, _, _, _) = flare_part(me, dx, t, None, None);
            let (got, _, _, _, flag) = flare_part(me, dx, t, Some(&capped), storm(200.0, t - 300));
            assert!((got - oracle).abs() <= 0.3, "{me:?}: {got} vs {oracle}");
            assert!(got < grid_word, "{me:?}");
            assert_eq!(flag, Some(50.0));
        }
        // The rule itself, at its edges.
        let now = MAY_12Z;
        assert!(held_protons(storm(10.0, now - crate::pca::PROTON_FRESH_SECS + 1), now).is_some());
        assert!(held_protons(storm(10.0, now - crate::pca::PROTON_FRESH_SECS), now).is_none());
        assert!(held_protons(storm(9.9, now), now).is_none());
        assert!(held_protons(None, now).is_none());
    }

    /// THE ONE RULE, as a case table both solar phases run unchanged: (J(≥10 MeV) pfu, the
    /// reading's age in seconds — None for undated, negative for ahead of our clock —, is it an
    /// event, its S level). [`check_the_one_rule`] runs this table against `pca::proton_event` /
    /// `pca::s_scale` and against [`held_protons`], which calls them, so the two answer alike.
    pub(crate) const PROTON_EVENT_CASES: &[(f64, Option<i64>, bool, u8)] = &[
        // NOAA's S thresholds, at and just under each decade.
        (9.999, Some(60), false, 0),
        (10.0, Some(60), true, 1),
        (99.99, Some(60), true, 1),
        (100.0, Some(60), true, 2),
        (999.9, Some(60), true, 2),
        (1e3, Some(60), true, 3),
        (1e4, Some(60), true, 4),
        (99_999.0, Some(60), true, 4),
        (1e5, Some(60), true, 5),
        (1e6, Some(60), true, 5),
        // Huge but finite: the top level, never an overflow.
        (1e300, Some(60), true, 5),
        (f64::MAX, Some(60), true, 5),
        // Non-finite: no reading at all, so S0 and no event (fail closed).
        (f64::INFINITY, Some(60), false, 0),
        (f64::NAN, Some(60), false, 0),
        (f64::NEG_INFINITY, Some(60), false, 0),
        (0.0, Some(60), false, 0),
        (-5.0, Some(60), false, 0),
        // Age: under 30 minutes, the boundary itself refused.
        (200.0, Some(0), true, 2),
        (200.0, Some(1799), true, 2),
        (200.0, Some(1800), false, 2),
        (200.0, Some(6 * 3600), false, 2),
        // Ahead of our clock: up to 120 s is clock skew and taken; 3 minutes or an hour is refused.
        (200.0, Some(-120), true, 2),
        (200.0, Some(-121), false, 2),
        (200.0, Some(-180), false, 2),
        (200.0, Some(-3600), false, 2),
        // Undated: never an event.
        (200.0, None, false, 2),
        // Ages at the ends of the clock: never an overflow, never an event.
        (200.0, Some(i64::MAX), false, 2),
        (200.0, Some(i64::MIN), false, 2),
    ];

    /// Run [`PROTON_EVENT_CASES`] against a rule `event(j10, at, now)` and a scale `s(j10)`.
    pub(crate) fn check_the_one_rule(
        event: impl Fn(f64, Option<i64>, i64) -> bool,
        s: impl Fn(f64) -> u8,
    ) {
        let now = MAY_12Z;
        for &(j10, age, want_event, want_s) in PROTON_EVENT_CASES {
            let at = age.map(|a| now.saturating_sub(a));
            assert_eq!(
                event(j10, at, now),
                want_event,
                "event: j10 {j10} age {age:?}"
            );
            assert_eq!(s(j10), want_s, "s_scale: j10 {j10}");
        }
    }

    #[test]
    fn the_one_rule_for_a_held_proton_event_matches_its_case_table() {
        check_the_one_rule(
            |j10, at, now| {
                let p = ProtonSample {
                    j1: j10,
                    j5: j10,
                    j10,
                    at,
                };
                held_protons(Some(p), now).is_some()
            },
            crate::pca::s_scale,
        );
        // And the polar-cap layer's rule itself, which `held_protons` calls: the same table.
        check_the_one_rule(crate::pca::proton_event, crate::pca::s_scale);
        // Sample times at the ends of the clock: no overflow (a panic in a debug build), no event.
        for at in [i64::MIN, i64::MAX] {
            let p = storm(200.0, 0).map(|p| ProtonSample { at: Some(at), ..p });
            assert!(held_protons(p, MAY_12Z).is_none(), "at {at}");
        }
    }

    #[test]
    fn a_dated_s2_reading_splits_and_the_same_reading_undated_does_not() {
        for (me, dx, t) in POLAR_PATHS {
            let capped = proton_grid(true, t);
            let dated = storm(200.0, t - 300);
            let undated = dated.map(|p| ProtonSample { at: None, ..p });
            let (oracle, _, _, _, _) = flare_part(me, dx, t, None, None);
            let (grid_word, _, _, _, _) = flare_part(me, dx, t, Some(&capped), None);
            let (got, _, pca, from, flag) = flare_part(me, dx, t, Some(&capped), dated);
            assert!(
                (got - oracle).abs() <= 0.3,
                "{me:?}: dated {got} vs {oracle}"
            );
            assert_eq!(flag, Some(50.0), "{me:?}");
            assert!(pca.is_some() && from != Some(FlareFrom::Noaa), "{me:?}");
            // Undated: no event — no split, no PCA figure, NOAA's grid stands.
            let (got, _, pca, from, flag) = flare_part(me, dx, t, Some(&capped), undated);
            assert_eq!(got, grid_word, "{me:?}: undated");
            assert_eq!(
                (pca, from, flag),
                (None, Some(FlareFrom::Noaa), None),
                "{me:?}"
            );
        }
        assert!(held_protons(
            storm(200.0, MAY_12Z).map(|p| ProtonSample { at: None, ..p }),
            MAY_12Z
        )
        .is_none());
    }

    /// A grid that is the analytic X1 field at `t` plus `excess` MHz at |geomagnetic lat| ≥ 50°.
    fn excess_grid(t: i64, excess: f32) -> DrapGrid {
        let mut g = proton_grid(false, t);
        let cols = g.lons.len();
        for (r, la) in g.lats.iter().enumerate() {
            for (c, lo) in g.lons.iter().enumerate() {
                if geo::geomagnetic_lat_deg(f64::from(*la), f64::from(*lo)).abs() >= 50.0 {
                    g.haf[r * cols + c] += excess;
                }
            }
        }
        g
    }

    #[test]
    fn the_grid_stands_within_phase_ones_bound_1_mhz_under_a_high_sun_3_in_the_taper() {
        // Tromsø–Kiruna, a short path inside the northern cap, at an hour of 15 May when the sun is
        // high at both ends (cos χ ≥ 0.5) and one when it is low (0 < cos χ < 0.5, the taper).
        const TROMSO_: (f64, f64) = (69.65, 18.96);
        const KIRUNA: (f64, f64) = (67.86, 20.23);
        let day0 = MAY_12Z - 12 * 3600;
        let sin_el =
            |p: (f64, f64), t: i64| geo::solar_elevation_deg(p.0, p.1, t).to_radians().sin();
        let hour = |want: &dyn Fn(f64) -> bool| {
            (0..24)
                .map(|h| day0 + h * 3600)
                .find(|&t| want(sin_el(TROMSO_, t)) && want(sin_el(KIRUNA, t)))
                .expect("such an hour on 15 May")
        };
        let high = hour(&|c| c >= 0.55);
        let low = hour(&|c| c > 0.05 && c < 0.45);
        let at = |t: i64| {
            let g = excess_grid(t, 2.0);
            let (_, _, _, from, flag) =
                flare_part(TROMSO_, KIRUNA, t, Some(&g), storm(200.0, t - 300));
            (from, flag)
        };
        // 2 MHz over the X-ray part: past the 1 MHz bound under a high sun, so it is the proton
        // part there; inside the 3 MHz bound in the taper, so NOAA's grid stands.
        assert_eq!(at(high), (Some(FlareFrom::Goes), Some(50.0)));
        assert_eq!(at(low), (Some(FlareFrom::Noaa), None));
    }

    #[test]
    fn each_band_says_whether_its_flare_figure_came_from_noaa_goes_or_both() {
        let s2 = storm(200.0, MAY_12Z - 300);
        let capped = proton_grid(true, MAY_12Z);
        // Wholly inside the cap: every crossing took the GOES estimate.
        let (_, _, _, from, _) = flare_part(TROMSO, FAIRBANKS, MAY_12Z, Some(&capped), s2);
        assert_eq!(from, Some(FlareFrom::Goes));
        // London–Tokyo: NOAA's grid at London, GOES over the cap.
        let (_, _, _, from, _) = flare_part(LONDON, TOKYO, MAY_12Z, Some(&capped), s2);
        assert_eq!(from, Some(FlareFrom::Both));
        // Madrid–Cairo stays far below the cap: the grid stands, and the route carries no flag.
        let (_, _, _, from, flag) =
            flare_part((40.42, -3.7), (30.04, 31.24), MAY_12Z, Some(&capped), s2);
        assert_eq!((from, flag), (Some(FlareFrom::Noaa), None));
        // The GOES estimate alone is GOES's.
        let (_, _, _, from, flag) = flare_part(TROMSO, FAIRBANKS, MAY_12Z, None, s2);
        assert_eq!((from, flag), (Some(FlareFrom::Goes), None));
    }

    #[test]
    fn a_proton_event_absorbs_a_polar_path_and_not_an_equatorial_one() {
        let s2 = storm(150.0, MAY_12Z - 300);
        let o = overlay(
            BOSTON,
            TOKYO,
            Route::Short,
            &bands(),
            Some(3.0),
            MAY_12Z,
            None,
            s2,
        )
        .unwrap();
        let b = of(&o, "20m");
        assert!(b.pca_db.unwrap() > 1.0, "{:?}", b.pca_db);
        assert!(kinds(b).contains(&SegmentKind::Pca));
        assert!(b.flare_db.is_none(), "no flare: no flare figure");
        assert_eq!(o.source.s_scale, Some(2));
        assert_eq!(o.source.protons_at, Some(MAY_12Z - 300));
        assert_eq!(o.kp, Some(3.0));
        // Negative control: Singapore–Nairobi never reaches the cap.
        let e = overlay(
            SINGAPORE,
            NAIROBI,
            Route::Short,
            &bands(),
            Some(3.0),
            MAY_12Z,
            None,
            s2,
        )
        .unwrap();
        assert_eq!(of(&e, "20m").pca_db, Some(0.0));
        assert!(of(&e, "20m").segments.is_empty());
    }

    #[test]
    fn no_overlay_without_an_event_a_fresh_sample_or_a_kp() {
        // Quiet protons (S0) and no flare: nothing to lay over.
        let quiet = storm(1.0, MAY_12Z - 300);
        assert!(overlay(
            BOSTON,
            TOKYO,
            Route::Short,
            &bands(),
            Some(3.0),
            MAY_12Z,
            None,
            quiet
        )
        .is_none());
        // An S2 sample half an hour old does not speak for now.
        let old = storm(150.0, MAY_12Z - 1800);
        assert!(overlay(
            BOSTON,
            TOKYO,
            Route::Short,
            &bands(),
            Some(3.0),
            MAY_12Z,
            None,
            old
        )
        .is_none());
        // With Kp unknown there is no cap edge, so no proton figure is claimed.
        let s2 = storm(150.0, MAY_12Z - 300);
        assert!(overlay(
            BOSTON,
            TOKYO,
            Route::Short,
            &bands(),
            None,
            MAY_12Z,
            None,
            s2
        )
        .is_none());
        // Positive control: the same event with a Kp.
        assert!(overlay(
            BOSTON,
            TOKYO,
            Route::Short,
            &bands(),
            Some(3.0),
            MAY_12Z,
            None,
            s2
        )
        .is_some());
    }

    #[test]
    fn the_long_route_gets_its_own_figures() {
        let f = footprint(x(X1, MAY_12Z), None, MAY_12Z).unwrap();
        // London–Tokyo at 12Z: the short path runs over Siberia's evening, the long path down
        // through the sunlit Atlantic and Africa.
        let sp = overlay(
            LONDON,
            TOKYO,
            Route::Short,
            &bands(),
            Some(2.0),
            MAY_12Z,
            Some(&f),
            None,
        )
        .unwrap();
        let lp = overlay(
            LONDON,
            TOKYO,
            Route::Long,
            &bands(),
            Some(2.0),
            MAY_12Z,
            Some(&f),
            None,
        )
        .unwrap();
        assert!(
            of(&lp, "20m").flare_db.unwrap() > of(&sp, "20m").flare_db.unwrap(),
            "{:?} {:?}",
            of(&lp, "20m").flare_db,
            of(&sp, "20m").flare_db
        );
    }

    fn watch(from: i64, to: i64, g: u8) -> Watch {
        Watch {
            serial: 1134,
            code: "WATA20".into(),
            issued: from - 86_400,
            days: vec![WatchDay { day: from, g }],
            comment: None,
            window: Some((from, to)),
            max_g: g,
        }
    }

    #[test]
    fn the_now_line_names_the_bands_out_under_the_sun_and_until_when() {
        // X1 from GOES: 25 MHz under the sun, so 60 m (5.357 ≤ 25/4.64) is out and 12 m
        // (24.915 ≤ 25) weakened — the map's flareUnderSun gives the same.
        let f = footprint(x(X1, MAY_12Z), None, MAY_12Z).unwrap();
        let n = space_now(Some(&f), None, None, MAY_12Z).unwrap();
        let fl = n.flare.unwrap();
        assert_eq!(fl.out.as_deref(), Some("60m"));
        assert_eq!(fl.weak.as_deref(), Some("12m"));
        assert_eq!(fl.haf, HafSource::Goes);
        assert_eq!(fl.r, 3);
        assert_eq!(fl.xray_at, MAY_12Z);
        assert_eq!(fl.recovery.unwrap().source, RecoverySource::RuleOfThumb);
        // NOAA's grid, current, says 30 MHz: its word stands over GOES's estimate.
        let g = grid(30.0, MAY_12Z - 60, Some(MAY_12Z + 3600));
        let f = footprint(x(X1, MAY_12Z), Some(&g), MAY_12Z).unwrap();
        let fl = space_now(Some(&f), None, None, MAY_12Z)
            .unwrap()
            .flare
            .unwrap();
        assert_eq!(fl.haf, HafSource::Noaa);
        assert_eq!(fl.drap_valid_at, Some(MAY_12Z - 60));
        assert_eq!(fl.out.as_deref(), Some("60m"));
        assert_eq!(fl.weak.as_deref(), Some("10m"));
        assert_eq!(
            fl.recovery,
            Some(Recovery {
                at: MAY_12Z + 3600,
                source: RecoverySource::Noaa
            })
        );
    }

    #[test]
    fn the_now_line_carries_a_proton_event_and_a_watch_and_is_silent_when_quiet() {
        let n = space_now(None, storm(150.0, MAY_12Z - 300), None, MAY_12Z).unwrap();
        assert_eq!(
            n.protons,
            Some(NowProtons {
                s: 2,
                j10: 150.0,
                at: MAY_12Z - 300
            })
        );
        assert!(n.flare.is_none() && n.watch.is_none());
        let w = watch(MAY_12Z + 12 * 3600, MAY_12Z + 36 * 3600, 1);
        let n = space_now(None, None, Some(&w), MAY_12Z).unwrap();
        assert_eq!(
            n.watch.as_ref().map(|w| (w.g, w.from)),
            Some((1, MAY_12Z + 12 * 3600))
        );
        // Quiet: no flare, S0, an old proton sample, a watch already over → nothing.
        assert!(space_now(None, storm(1.0, MAY_12Z), None, MAY_12Z).is_none());
        assert!(space_now(None, storm(150.0, MAY_12Z - 1800), None, MAY_12Z).is_none());
        let over = watch(MAY_12Z - 86_400, MAY_12Z - 1, 1);
        assert!(space_now(None, None, Some(&over), MAY_12Z).is_none());
    }
}
