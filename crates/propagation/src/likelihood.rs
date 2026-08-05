//! Contact-likelihood model — the linkage between propagation and DXpedition
//! tracking. For a given operator↔DX great-circle path it estimates, **per band
//! and per UTC time**, how workable the path is, and finds the best time-of-day
//! windows so the operator knows *whether* and *when* to chase a DXpedition.
//!
//! This is a deliberately physics-*lite*, median-conditions model (NOT VOACAP):
//! it gets the shape right — which bands open, day/night, greyline spikes, polar
//! penalties, rough best-time windows — but not absolute reliability. Treat the
//! output as relative workability, never a guarantee. Every constant lives in
//! [`PropParams`] and is tunable against on-air reality.
//!
//! The chain (all grounded in standard ionospheric behavior):
//! - **MUF** from foF2(SFI, solar zenith at the path's control points) × an
//!   obliquity factor that grows with path length; a band above MUF is closed.
//! - **D-layer absorption** (∝ cos(χ)^0.75 / f²) — the daytime killer of the low
//!   bands, zero at night.
//! - **Greyline** bonus on the low bands when either end rides the terminator.
//! - **Auroral/Kp** penalty, harsh on paths crossing high geomagnetic latitude.
//!
//! 6 m / VHF is intentionally out of scope here (Es-driven, not foF2-driven) —
//! it routes to the [`crate::detector`] opening detector instead.

use serde::Serialize;

use crate::geo::{
    geomagnetic_lat_deg, haversine_km, interpolate, solar_declination_deg, solar_elevation_deg,
};
use crate::model::{Band, SpaceWx};

/// Tunable constants for the likelihood model. [`Default`] reproduces standard
/// band behavior (20 m daytime workhorse, 80 m a night/greyline band, etc.);
/// calibrate `fof2_*` and `k_abs` first against known openings.
#[derive(Debug, Clone, Copy)]
pub struct PropParams {
    /// foF2 (MHz) ≈ (a + b·SFI)·cos(zenith)^p when sunlit, else `fof2_floor`.
    pub fof2_a: f64,
    pub fof2_b: f64,
    pub fof2_p: f64,
    pub fof2_floor: f64,
    /// Max obliquity factor M (MUF = foF2·M) reached on long DX paths.
    pub muf_obliquity_max: f64,
    /// Logistic sharpness of the band-vs-MUF cutoff.
    pub k_muf: f64,
    /// D-layer absorption scale (dB at zenith overhead, ÷ f²).
    pub k_abs: f64,
    pub abs_exp: f64,
    /// |elevation| (deg) under which an end counts as "on the greyline".
    pub grey_deg: f64,
    /// Greyline gain (multiplied by a per-band weight).
    pub grey_gain: f64,
    /// Global per-Kp degradation (above Kp 2).
    pub kp_global: f64,
    /// Polar-path per-Kp degradation (above Kp 3) when the path is high-latitude.
    pub kp_polar: f64,
    /// Geomagnetic latitude (deg) above which a path is treated as polar.
    pub geomag_high: f64,
}

impl Default for PropParams {
    fn default() -> Self {
        Self {
            fof2_a: 4.0,
            fof2_b: 0.04,
            fof2_p: 0.25,
            fof2_floor: 3.0,
            muf_obliquity_max: 3.2,
            k_muf: 8.0,
            k_abs: 500.0,
            abs_exp: 0.75,
            grey_deg: 6.0,
            grey_gain: 0.7,
            kp_global: 0.06,
            kp_polar: 0.18,
            geomag_high: 60.0,
        }
    }
}

/// Qualitative workability bucket (the user-facing word, not a raw number).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Workability {
    Closed,
    Marginal,
    Fair,
    Good,
    Excellent,
}

impl Workability {
    pub fn label(self) -> &'static str {
        match self {
            Workability::Closed => "Closed",
            Workability::Marginal => "Marginal",
            Workability::Fair => "Fair",
            Workability::Good => "Good",
            Workability::Excellent => "Excellent",
        }
    }

    pub fn from_score(s: f32) -> Workability {
        if s >= 0.80 {
            Workability::Excellent
        } else if s >= 0.55 {
            Workability::Good
        } else if s >= 0.30 {
            Workability::Fair
        } else if s >= 0.10 {
            Workability::Marginal
        } else {
            Workability::Closed
        }
    }

    /// Is the band at least marginally workable (worth surfacing)?
    pub fn is_open(self) -> bool {
        !matches!(self, Workability::Closed)
    }

    /// Coarse Open / Marginal / Closed — the 3-state the band-condition strip and the
    /// advisor's `modeled` field use (collapses the 5-bucket).
    pub fn openness3(self) -> &'static str {
        match self {
            Workability::Fair | Workability::Good | Workability::Excellent => "Open",
            Workability::Marginal => "Marginal",
            Workability::Closed => "Closed",
        }
    }
}

/// One band's outlook on a path: the best workability over the scanned day and
/// the time window that achieves it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandOutlook {
    pub band: String,
    pub workability: String,
    /// Peak likelihood over the scan (0..1).
    pub score: f32,
    /// Human window, e.g. "1400–1700Z", "~1015Z greyline", or "—".
    pub window: String,
    /// True when this band's best window is a short low-band terminator (greyline)
    /// spike — the UI shows a greyline glyph. Structured so the UI need not parse
    /// the `window` string.
    pub grayline: bool,
    /// Per-UTC-hour likelihood (24 values, hour 0..23) for the day containing the
    /// scan — drives the band×hour calendar heatmap in the UI.
    pub hourly: Vec<f32>,
    /// Circuit reliability: the fraction of the 24 h scan (0–100) the band clears the
    /// usable (≥ Fair) threshold — a VOACAP-style coverage metric derived from the model's
    /// per-time scores. Day coverage, NOT a per-hour SNR-based reliability (that needs the
    /// CCIR-coefficient + statistical-SNR port — a separate effort).
    pub reliability: f32,
    /// Per-mode workability RIGHT NOW (current hour) from the engine's SNR
    /// distribution vs each mode's required SNR (FT8/FT4/CW/SSB). Only the
    /// P.533 engine fills this (it has real SNR statistics); the heuristic
    /// leaves it empty and the UI hides the row — honesty over guessing.
    pub mode_now: Vec<ModeNow>,
    /// The full per-mode × per-hour grid behind `mode_now` — in-process only
    /// (`serde(skip)`), retained so a day-scale cache can re-derive the "now"
    /// chips for the SERVING hour instead of freezing them at compute time
    /// (`mode_now` is a now-scalar exactly like `muf_now`).
    #[serde(skip)]
    pub mode_hourly: Vec<ModeHourly>,
}

/// One mode's "workable right now" entry — see [`BandOutlook::mode_now`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeNow {
    pub mode: String,
    /// BCR-derived likelihood 0..1 for this mode at the current hour.
    pub score: f32,
}

/// One mode's per-UTC-hour scores — see [`BandOutlook::mode_hourly`].
#[derive(Debug, Clone)]
pub struct ModeHourly {
    pub mode: String,
    pub hourly: [f32; 24],
}

/// Slice a `mode_hourly` grid at one UTC hour → the `mode_now` chips for that
/// hour. Empty in → empty out (the heuristic's honest no-data state).
pub fn mode_now_at(mode_hourly: &[ModeHourly], hour: usize) -> Vec<ModeNow> {
    mode_hourly
        .iter()
        .map(|m| ModeNow {
            mode: m.mode.clone(),
            score: m.hourly[hour % 24],
        })
        .collect()
}

/// The contact-likelihood model, anchored at one operator location.
pub struct PathModel {
    me: Option<(f64, f64)>,
    pub params: PropParams,
}

impl PathModel {
    pub fn new(me_latlon: Option<(f64, f64)>) -> Self {
        Self {
            me: me_latlon,
            params: PropParams::default(),
        }
    }

    /// Instantaneous propagation likelihood (0..1) for `band` on the path to
    /// `dx` at `unix`. Returns 0 for VHF (handled by the Es detector) or if the
    /// operator location is unknown.
    pub fn score(&self, dx: (f64, f64), band: Band, unix: i64, wx: &SpaceWx) -> f32 {
        let Some(me) = self.me else { return 0.0 };
        if band.is_vhf() {
            return 0.0;
        }
        let p = &self.params;
        let f = band.center_mhz();
        let dist = haversine_km(me, dx);

        // Control points: ~1/7 in from each end + midpoint. The path is limited
        // by its weakest (lowest-MUF) reflection region.
        let cps = [
            interpolate(me, dx, 1.0 / 7.0),
            interpolate(me, dx, 0.5),
            interpolate(me, dx, 6.0 / 7.0),
        ];
        let m = self.obliquity(dist);
        let mut path_muf = f64::MAX;
        let mut abs_db = 0.0;
        for cp in cps {
            let elev = solar_elevation_deg(cp.0, cp.1, unix);
            path_muf = path_muf.min(self.fof2(elev, wx) * m);
            if elev > 0.0 {
                // cos(zenith) = sin(elev). Daytime D-layer absorption only.
                let cz = elev.to_radians().sin();
                abs_db += p.k_abs * cz.powf(p.abs_exp) / (f * f);
            }
        }
        // Mean (representative) absorption along the path — night control points
        // contribute zero, so a path with a dark end (greyline) is far quieter.
        abs_db /= cps.len() as f64;

        // MUF headroom: logistic falloff centered just below the MUF.
        let r = f / path_muf;
        let muf_factor = 1.0 / (1.0 + (p.k_muf * (r - 0.9)).exp());
        let absorption_factor = 10f64.powf(-abs_db / 10.0);
        let aurora_factor = self.aurora(me, dx, band, wx);
        let grey_factor = self.greyline(me, dx, band, unix);

        let raw = muf_factor * absorption_factor * aurora_factor * grey_factor;
        // Cap by the band's real-world DX achievability: even a wide-open top
        // band is hard DX (antenna size, atmospheric QRN, residual absorption),
        // so 160/80 m should top out well below "Excellent". This is a lumped
        // calibration, not physics — tune to taste.
        (raw.min(band_ceiling(band))).clamp(0.0, 1.0) as f32
    }

    /// Best workability + window for `band` over the 24 h starting at
    /// `from_unix` (30-min steps). For active ops pass `now`; for calendar
    /// planning pass the operation's start.
    pub fn outlook_24h(
        &self,
        dx: (f64, f64),
        band: Band,
        from_unix: i64,
        wx: &SpaceWx,
    ) -> BandOutlook {
        const STEP: i64 = 1800;
        const N: i64 = 48;
        let mut scores = [0f32; 48];
        let mut peak = 0f32;
        let mut peak_i = 0usize;
        for i in 0..N {
            let s = self.score(dx, band, from_unix + i * STEP, wx);
            scores[i as usize] = s;
            if s > peak {
                peak = s;
                peak_i = i as usize;
            }
        }

        let workability = Workability::from_score(peak);
        // Window = the contiguous ≥Fair run containing the peak.
        let mut grayline = false;
        let window = if peak < 0.30 {
            "—".to_string()
        } else {
            let thresh = 0.30f32;
            let mut lo = peak_i;
            while lo > 0 && scores[lo - 1] >= thresh {
                lo -= 1;
            }
            let mut hi = peak_i;
            while (hi + 1) < N as usize && scores[hi + 1] >= thresh {
                hi += 1;
            }
            let start = from_unix + lo as i64 * STEP;
            let end = from_unix + (hi as i64 + 1) * STEP;
            let width = end - start;
            let low_band = matches!(band, Band::B160 | Band::B80 | Band::B40);
            if low_band && width <= 2 * 3600 {
                // A short low-band spike around the terminator → greyline label.
                grayline = true;
                format!("~{} greyline", hhmm_z(from_unix + peak_i as i64 * STEP))
            } else {
                format!("{}–{}", hhmm(start), hhmm_z(end))
            }
        };

        // Per-UTC-hour likelihood for the day containing `from_unix` (hour 0..23),
        // for the calendar heatmap. Anchored to UTC midnight so the x-axis and the
        // "now" hairline line up across bands.
        let day0 = from_unix - from_unix.rem_euclid(86_400);
        let hourly: Vec<f32> = (0..24)
            .map(|h| self.score(dx, band, day0 + h * 3600, wx))
            .collect();

        // Circuit reliability = the fraction of the scan the band is usable (≥ Fair) — a
        // VOACAP-style coverage metric over the same 30-min steps the window uses.
        let usable = scores.iter().filter(|&&s| s >= 0.30).count();
        let reliability = (usable as f32 / N as f32) * 100.0;

        BandOutlook {
            band: band.label().to_string(),
            workability: workability.label().to_string(),
            score: peak,
            window,
            grayline,
            hourly,
            reliability,
            mode_now: Vec::new(), // heuristic has no SNR statistics — stays empty
            mode_hourly: Vec::new(),
        }
    }

    /// The path's controlling **MUF** (MHz) — the band CEILING on the path to `dx`
    /// at `unix`: the minimum across the hop control points of foF2·obliquity.
    /// Bands whose center frequency is below this are open; above it, closed. This
    /// is per-path-per-time (NOT per band), so the UI surfaces it as the one-glance
    /// "ceiling" line above the band ladder. Returns 0 when the operator location is
    /// unknown. Mirrors the `path_muf` computed inside [`PathModel::score`].
    pub fn muf(&self, dx: (f64, f64), unix: i64, wx: &SpaceWx) -> f64 {
        let Some(me) = self.me else {
            return 0.0;
        };
        let dist = haversine_km(me, dx);
        let cps = [
            interpolate(me, dx, 1.0 / 7.0),
            interpolate(me, dx, 0.5),
            interpolate(me, dx, 6.0 / 7.0),
        ];
        let m = self.obliquity(dist);
        let mut path_muf = f64::MAX;
        for cp in cps {
            let elev = solar_elevation_deg(cp.0, cp.1, unix);
            path_muf = path_muf.min(self.fof2(elev, wx) * m);
        }
        path_muf
    }

    /// foF2 (MHz) at a control point given the sun's elevation there.
    fn fof2(&self, elev_deg: f64, wx: &SpaceWx) -> f64 {
        let p = &self.params;
        let day = if elev_deg > 0.0 {
            let cz = elev_deg.to_radians().sin();
            (p.fof2_a + p.fof2_b * wx.sfi as f64) * cz.powf(p.fof2_p)
        } else {
            0.0
        };
        day.max(p.fof2_floor)
    }

    /// Obliquity factor M: MUF = foF2·M, growing 1.0→`max` with path length.
    fn obliquity(&self, dist_km: f64) -> f64 {
        let max = self.params.muf_obliquity_max;
        if dist_km <= 3000.0 {
            1.0 + (dist_km / 3000.0) * (max - 1.0)
        } else {
            max
        }
    }

    /// Geomagnetic/auroral factor (0..1) — mild global Kp penalty plus a harsh
    /// polar-path penalty when the great circle crosses high geomagnetic lat.
    fn aurora(&self, me: (f64, f64), dx: (f64, f64), band: Band, wx: &SpaceWx) -> f64 {
        let p = &self.params;
        let kp = wx.kp as f64;
        let global = (1.0 - p.kp_global * (kp - 2.0).max(0.0)).max(0.0);

        let mut max_geo = 0.0f64;
        for i in 0..=8 {
            let pt = interpolate(me, dx, i as f64 / 8.0);
            max_geo = max_geo.max(geomagnetic_lat_deg(pt.0, pt.1).abs());
        }
        let polar = if max_geo > p.geomag_high {
            // Low bands feel auroral absorption more.
            let g = if matches!(band, Band::B160 | Band::B80) {
                p.kp_polar * 1.3
            } else {
                p.kp_polar
            };
            (1.0 - g * (kp - 3.0).max(0.0)).max(0.0).powi(2)
        } else {
            1.0
        };
        (global * polar).clamp(0.0, 1.0)
    }

    /// Greyline bonus (≥1) on the low bands when either end is on the terminator.
    ///
    /// ⚠️ KNOWN GAP, DELIBERATELY LEFT — this table omits `B60` exactly as
    /// [`band_ceiling`] did, and 60 m (5.36 MHz, between 80 m's 0.8 and 40 m's 0.6)
    /// should carry roughly 0.7. It is NOT fixed here because, unlike the ceiling
    /// omission, this one is currently inert: it *understates* 60 m, and the 0.80
    /// ceiling binds first on every path where it would apply, so today's ranking is
    /// identical either way (measured 2026-08-05). Closing it is a calibration change
    /// that also has to teach `outlook_24h`'s `low_band` window label about 60 m, and
    /// it RAISES 60 m — so it wants its own measurement and its own decision rather
    /// than a free ride on a bug fix. `low_band_ceilings_rise_with_frequency` guards
    /// the ceiling ramp; it does not cover this table.
    fn greyline(&self, me: (f64, f64), dx: (f64, f64), band: Band, unix: i64) -> f64 {
        let bw = match band {
            Band::B160 => 1.0,
            Band::B80 => 0.8,
            Band::B40 => 0.6,
            Band::B30 => 0.3,
            _ => 0.0, // ⚠️ 60 m lands here — see the note above, not an oversight
        };
        if bw == 0.0 {
            return 1.0;
        }
        let on_terminator =
            |pt: (f64, f64)| solar_elevation_deg(pt.0, pt.1, unix).abs() < self.params.grey_deg;
        if on_terminator(me) || on_terminator(dx) {
            1.0 + self.params.grey_gain * bw
        } else {
            1.0
        }
    }
}

/// Max achievable likelihood per band — a lumped "how hard is DX on this band
/// even when it's open" ceiling (antenna size, atmospheric noise, residual
/// absorption). Low bands cap below "Excellent"; mid/high bands are uncapped.
/// ⚠️ EVERY band below 30 m MUST have an arm here. At night `path_muf` is the hard
/// constant `fof2_floor · muf_obliquity_max` (9.6 MHz) on any path over 3000 km, and
/// absorption is daylight-gated to exactly zero — so the score of every band under it
/// collapses to this ceiling, and an omitted band lands on `_ => 1.0` and out-ranks every
/// capped one on every path on Earth. 60 m did exactly that (field report 2026-08-05: all
/// 11 WORK NOW cards read "Best shot: 60m", score 0.93894 to three decimals on 7 of 8
/// paths spanning 2,214–11,509 km). The `low_band_ceilings_rise_with_frequency` test is
/// the guard: it fails on the next omission rather than shipping it.
fn band_ceiling(band: Band) -> f64 {
    match band {
        Band::B160 => 0.55, // tops out at "Good" — top-band DX is hard
        Band::B80 => 0.72,
        // Harder DX than either neighbour despite sitting between them: secondary,
        // channelised, no split (so no DXpedition pileup), and the WRC-15 segment is
        // 9.15 W ERP. Placed on the ramp, not above it.
        //
        // WHY 0.80 AND NOT NEARBY — counterfactual over the 8 field-report paths, and it
        // is EXACT rather than sampled: this is a per-sample constant and `outlook_24h`
        // takes a max, so `max_i min(raw_i, c) ≡ min(max_i raw_i, c)` — capping the
        // reported peak is algebraically identical to adding the arm.
        //   1.00 (no arm) → 60m tops 7/8   0.85 → 40m 7/8 (VP9 still reads 60m)
        //   0.80          → 40m 8/8        0.78 → 40m 8/8
        // 0.80 has margin on both sides; 0.85 does not.
        Band::B60 => 0.80,
        Band::B40 => 0.88,
        _ => 1.0,
    }
}

/// Seasonal hint so the demo/text can mention 6 m without the HF model:
/// northern-hemisphere Es season peaks late-May–July.
pub fn is_es_season(unix: i64) -> bool {
    let decl = solar_declination_deg(unix);
    decl > 15.0 // sun well north → boreal summer Es peak
}

pub(crate) fn hhmm(unix: i64) -> String {
    let s = unix.rem_euclid(86_400);
    format!("{:02}{:02}", s / 3600, (s % 3600) / 60)
}

pub(crate) fn hhmm_z(unix: i64) -> String {
    format!("{}Z", hhmm(unix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::maidenhead_to_latlon;

    fn p(grid: &str) -> (f64, f64) {
        maidenhead_to_latlon(grid).unwrap()
    }

    // A summer-solstice-ish base time so daytime/nighttime are pronounced.
    const NOON_UTC: i64 = 1_718_886_000; // 2024-06-20 13:00 UTC (DOY 172)
    const MIDNIGHT_UTC: i64 = NOON_UTC - 13 * 3600; // ~00:00 UTC

    #[test]
    fn high_band_is_a_daytime_band() {
        // EN52 (WI) ↔ JN58 (Munich). 20 m should be far better when the path is
        // sunlit (around mid-day overlap) than in deep local night for the op.
        let m = PathModel::new(Some(p("EN52")));
        let wx = SpaceWx {
            sfi: 150.0,
            kp: 1.0,
            ..Default::default()
        };
        let dx = p("JN58");
        // Sweep finds a daytime opening on 20 m.
        let o20 = m.outlook_24h(dx, Band::B20, MIDNIGHT_UTC, &wx);
        assert!(o20.score >= 0.5, "20m peak {}", o20.score);
        assert_ne!(o20.window, "—");
        // 24 UTC-hour samples for the calendar heatmap; peak ≈ the sweep peak.
        assert_eq!(o20.hourly.len(), 24);
        assert!(o20.hourly.iter().cloned().fold(0.0f32, f32::max) >= 0.4);
    }

    #[test]
    fn low_band_dies_in_daylight_lives_at_night() {
        let m = PathModel::new(Some(p("EN52")));
        let wx = SpaceWx {
            sfi: 120.0,
            kp: 1.0,
            ..Default::default()
        };
        let dx = p("JN58");
        // 80 m at ~15:00 UTC — both ends of this EN52↔JN58 path are sunlit, so
        // the D-layer murders it.
        let day = m.score(dx, Band::B80, NOON_UTC + 2 * 3600, &wx);
        // 80 m somewhere over the night sweep → much better.
        let night_peak = m.outlook_24h(dx, Band::B80, MIDNIGHT_UTC, &wx).score;
        assert!(
            night_peak > day,
            "80m night {night_peak} should beat day {day}"
        );
        assert!(day < 0.2, "80m daytime should be ~closed, got {day}");
    }

    #[test]
    fn band_above_muf_is_closed() {
        // 10 m to a short path at low SFI: MUF won't reach 28 MHz → closed.
        let m = PathModel::new(Some(p("EN52")));
        let wx = SpaceWx {
            sfi: 70.0,
            kp: 1.0,
            ..Default::default()
        };
        let dx = p("EN90"); // ~short eastward hop
        let peak = m.outlook_24h(dx, Band::B10, MIDNIGHT_UTC, &wx).score;
        assert!(
            peak < 0.3,
            "10m at SFI 70 short path should be poor, got {peak}"
        );
    }

    #[test]
    fn polar_storm_penalizes_transpolar_path() {
        // EN52 ↔ Franz Josef Land (polar). A geomagnetic storm should depress
        // the path relative to quiet conditions.
        let m = PathModel::new(Some(p("EN52")));
        let dx = (80.6, 55.0);
        let quiet = SpaceWx {
            sfi: 140.0,
            kp: 1.0,
            ..Default::default()
        };
        let storm = SpaceWx {
            sfi: 140.0,
            kp: 7.0,
            ..Default::default()
        };
        let q = m.outlook_24h(dx, Band::B20, MIDNIGHT_UTC, &quiet).score;
        let s = m.outlook_24h(dx, Band::B20, MIDNIGHT_UTC, &storm).score;
        assert!(
            s < q,
            "storm {s} should be worse than quiet {q} on a polar path"
        );
    }

    #[test]
    fn vhf_and_unknown_are_zero() {
        let m = PathModel::new(Some(p("EN52")));
        let wx = SpaceWx::default();
        assert_eq!(m.score(p("JN58"), Band::B6, NOON_UTC, &wx), 0.0);
        let unknown = PathModel::new(None);
        assert_eq!(unknown.score(p("JN58"), Band::B20, NOON_UTC, &wx), 0.0);
    }

    /// The operator's real WORK NOW board, 2026-08-05 (entity centroids from the
    /// screenshot): 11 operations on the air, 2,214–11,509 km, all eight modelled here.
    const FIELD_REPORT_PATHS: &[(&str, (f64, f64))] = &[
        ("OH0ERF Aland", (60.20, 20.00)),
        ("KH0 Marianas", (15.20, 145.75)),
        ("J3 Grenada", (12.12, -61.68)),
        ("VP9 Bermuda", (32.32, -64.75)),
        ("C6 Bahamas", (25.03, -77.40)),
        ("T2 Tuvalu", (-8.52, 179.20)),
        ("E51KEE S.Cook", (-21.21, -159.78)),
        ("9Y Trinidad", (10.65, -61.40)),
    ];

    fn hf_bands() -> Vec<Band> {
        Band::ALL.iter().copied().filter(|b| !b.is_vhf()).collect()
    }

    /// Peak-ranked bands on one path, best first.
    fn ranked(m: &PathModel, dx: (f64, f64), t: i64, wx: &SpaceWx) -> Vec<(String, f32)> {
        let mut v: Vec<(String, f32)> = hf_bands()
            .into_iter()
            .map(|b| {
                let o = m.outlook_24h(dx, b, t, wx);
                (o.band, o.score)
            })
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    /// FIELD REPORT 2026-08-05 — every WORK NOW card read "Best shot: **60m**", on
    /// every operation, whatever the card's own band. The windows differed per path,
    /// so the per-path modelling was running; only the BAND was stuck.
    ///
    /// Cause: [`band_ceiling`] had no `B60` arm, so 60 m fell through to `_ => 1.0`
    /// while its neighbours were capped — and at night the score of every band under
    /// the 9.6 MHz MUF floor IS its ceiling, so 60 m was the argmax of a constant table.
    ///
    /// NON-VACUITY — delete the `Band::B60 => 0.80` arm and this goes red with the
    /// numbers the operator saw (measured 2026-08-05, EN52 / SFI 150 / Kp 2):
    ///   OH0ERF KH0 J3 T2 E51KEE 9Y → 60m **0.93894** (identical to 5 dp on all six),
    ///   VP9 → 60m 0.884, C6 → 40m 0.880. 60 m topped 7 of 8.
    /// With the arm: 40m 0.880 on 7 of 8 (VP9 0.836), 60m 0.800 second or third.
    #[test]
    fn sixty_m_does_not_top_every_dxpedition_path() {
        let m = PathModel::new(Some(p("EN52")));
        let t = 1_785_888_000; // 2026-08-05 00:00 UTC
        let wx = SpaceWx {
            sfi: 150.0,
            kp: 2.0,
            ..Default::default()
        };
        for (name, dx) in FIELD_REPORT_PATHS {
            let r = ranked(&m, *dx, t, &wx);
            assert_ne!(
                r[0].0,
                "60m",
                "{name}: 60 m headlines the card again — ranking was {:?}",
                &r[..4]
            );
        }
    }

    /// THE STRUCTURAL GUARD, and the reason the 60 m bug was possible at all:
    /// [`band_ceiling`] is the whole ranking below the night MUF floor, so a band
    /// missing from it does not merely score oddly — it out-ranks every band that IS
    /// listed, on every path. The table is a ramp, so require the ramp: a ceiling may
    /// never DROP as frequency rises. An omitted low band lands on `_ => 1.0` and
    /// breaks that immediately.
    ///
    /// NON-VACUITY: without the `B60` arm, 60 m (5.36 MHz) reads 1.0 between 80 m's
    /// 0.72 and 40 m's 0.88 — red on the 60 m → 40 m step.
    #[test]
    fn low_band_ceilings_rise_with_frequency() {
        let mut prev: Option<(Band, f64)> = None;
        for b in Band::ALL {
            let c = band_ceiling(b);
            if let Some((pb, pc)) = prev {
                assert!(
                    c >= pc,
                    "band_ceiling falls with frequency: {} ({} MHz) = {pc} but the band \
                     ABOVE it, {} ({} MHz), = {c}. The suspect is {} — a band with no arm \
                     takes `_ => 1.0` and out-ranks every capped band on every path.",
                    pb.label(),
                    pb.center_mhz(),
                    b.label(),
                    b.center_mhz(),
                    pb.label()
                );
            }
            prev = Some((b, c));
        }
    }

    /// Standing invariant, NOT a repro for the 60 m bug (it passed before the fix too):
    /// no single band may own the instantaneous ranking. `advisor.rs` carries the same
    /// invariant for the observed-spot path after the "always 10 m" bug; this is the
    /// modelled path's copy, and it is what would have caught the 60 m regression had
    /// the 24 h-peak headline not been a separate constant (see the module note there).
    ///
    /// Measured 2026-08-05 over 4 QTHs × 8 paths × 24 h: 4–9 distinct winners, the
    /// biggest share 96/192 (50 %) at PM95/SFI 70. Thresholds sit clear of that.
    #[test]
    fn no_single_band_owns_the_instantaneous_ranking() {
        use std::collections::BTreeMap;
        let t0 = 1_785_888_000;
        for &(sfi, kp) in &[(70.0f32, 0.0f32), (150.0, 2.0), (220.0, 5.0)] {
            let wx = SpaceWx {
                sfi,
                kp,
                ..Default::default()
            };
            for grid in ["EN52", "JN58", "PM95", "GG66"] {
                let m = PathModel::new(Some(p(grid)));
                let mut wins: BTreeMap<&'static str, usize> = BTreeMap::new();
                let mut n = 0usize;
                for h in 0..24i64 {
                    for (_, dx) in FIELD_REPORT_PATHS {
                        let best = hf_bands()
                            .into_iter()
                            .map(|b| (b, m.score(*dx, b, t0 + h * 3600, &wx)))
                            .fold(None::<(Band, f32)>, |acc, x| match acc {
                                Some(a) if a.1 >= x.1 => Some(a),
                                _ => Some(x),
                            })
                            .unwrap();
                        *wins.entry(best.0.label()).or_default() += 1;
                        n += 1;
                    }
                }
                let top = wins.values().copied().max().unwrap();
                assert!(
                    wins.len() >= 4,
                    "{grid} SFI {sfi}: only {} distinct best-bands over 8 paths × 24 h — {wins:?}",
                    wins.len()
                );
                assert!(
                    top * 5 <= n * 3,
                    "{grid} SFI {sfi}: one band won {top}/{n} — {wins:?}"
                );
            }
        }
    }
}
