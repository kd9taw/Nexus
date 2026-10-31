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
//!   [`PROTONS_FRESH_SECS`] old; with no Kp there is no cap edge, so nothing is claimed.
//!
//! Both laws give the absorption of a VERTICAL pass through the D region (1 dB at the HAF, and
//! `(HAF/f)^1.5` dB at f). A path crosses the D region twice per hop, obliquely: [`path_overlay`]
//! sums the crossings at the hop ends, each times the secant of the ray's incidence at 110 km
//! (P.533's own reference height for absorption), which the geometry bounds at ~5.4 for a grazing
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
/// A GOES proton sample older than this does not speak for now (NOAA's product is 5-minutely).
pub const PROTONS_FRESH_SECS: i64 = 20 * 60;
/// NOAA's S1: J(≥10 MeV) at or above 10 pfu.
const S1_PFU: f64 = 10.0;
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

/// The GOES integral protons (pfu) the overlay runs on, with the sample's own time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProtonSample {
    pub j1: f64,
    pub j5: f64,
    pub j10: f64,
    pub at: i64,
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

/// NOAA's S-scale level for a J(≥10 MeV) flux (0 below S1).
pub fn s_scale(j10: f64) -> u8 {
    if j10.is_nan() || j10 < S1_PFU {
        return 0;
    }
    // S1 at 10 pfu, each level a decade more, S5 from 100 000.
    ((j10 / S1_PFU).log10().floor() as u8 + 1).min(5)
}

/// A proton sample that may speak for `now`: at most [`PROTONS_FRESH_SECS`] old.
pub fn protons_fresh(p: Option<ProtonSample>, now: i64) -> Option<ProtonSample> {
    p.filter(|p| now - p.at < PROTONS_FRESH_SECS && p.at - now <= DRAP_AHEAD_SECS)
}

/// The secant of a ray's incidence where it crosses the D region (110 km) for a take-off angle:
/// `sin i = R·cos Δ / (R + h)`. Bounded by the geometry (≈ 5.4 at a grazing take-off).
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
    // A proton event speaks only at S1+, beside a fresh sample, with a Kp to place the cap edge.
    let protons = protons_fresh(protons, now).filter(|p| s_scale(p.j10) >= 1);
    let pca = protons.zip(geo_now.kp);
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
        Some(fp) => pts.iter().map(|&(la, lo)| fp.haf_at(la, lo)).collect(),
        None => Vec::new(),
    };
    let out = bands
        .iter()
        .filter_map(|b| {
            let f = band_mhz(&b.band)?;
            let (hops, takeoff, hops_modelled) = hops_for(b, now_hour, dist_km);
            let sec = d_secant(takeoff);
            let flare_db = flare.map(|fp| {
                tenth(over_crossings(me, bearing, dist_km, hops, sec, |la, lo| {
                    let haf = fp.haf_at(la, lo);
                    if haf > 0.0 {
                        (haf / f).powf(1.5)
                    } else {
                        0.0
                    }
                }))
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
        source.recovery = fp.recovery();
    }
    if let Some((p, _)) = pca {
        source.protons_at = Some(p.at);
        source.j10 = Some(p.j10 as f32);
        source.s_scale = Some(s_scale(p.j10));
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
    let protons = protons_fresh(protons, now)
        .filter(|p| s_scale(p.j10) >= 1)
        .map(|p| NowProtons {
            s: s_scale(p.j10),
            j10: p.j10 as f32,
            at: p.at,
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
            at,
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
        assert_eq!(s_scale(9.9), 0);
        assert_eq!(s_scale(10.0), 1);
        assert_eq!(s_scale(150.0), 2);
        assert_eq!(s_scale(1e3), 3);
        assert_eq!(s_scale(1e5), 5);
        assert!(protons_fresh(storm(150.0, MAY_12Z - PROTONS_FRESH_SECS), MAY_12Z).is_none());
        assert!(protons_fresh(storm(150.0, MAY_12Z - 300), MAY_12Z).is_some());
        assert!(protons_fresh(None, MAY_12Z).is_none());
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
