//! Best times to work one station — a path prediction folded into a plain answer: can I work them
//! now and on what band, then the next good windows over the coming 24 hours, then what the week's
//! Kp outlook does to this path.
//!
//! A pure fold, no I/O. The caller hands it the short-path prediction (with its geometry and its
//! "now" overlay attached), the long-path one when the engine models it, the mode asked about, the
//! bands the operator's licence lets them transmit on in that mode, and the Kp outlook held. It
//! never predicts anything itself and never alters a score: every word it gives is read off the
//! engine's own figures.
//!
//! **The word for one mode at one hour.** The P.533 day word for a band is a ladder over the modes
//! it models ([`crate::p533::engine::p533_workability`]): Excellent when SSB is usable three days
//! in four, Good when CW is, Fair when FT8 is usable half the days, Marginal at one day in ten. Here
//! the same ladder is read at a single hour, with the asked mode taking FT8's place on the lower
//! rungs and having to reach Good's line itself before the hour can read Good:
//!
//! - **Excellent:** SSB usable three days in four;
//! - **Good:** CW usable three days in four, and the asked mode too;
//! - **Fair:** the asked mode usable half the days;
//! - **Marginal:** the asked mode usable one day in ten;
//! - **Closed:** less.
//!
//! For FT8 that is exactly the app's band ladder hour by hour. For SSB "Good" folds into
//! Excellent, because SSB is the hardest mode modelled. An engine with no per-mode figures (the
//! heuristic) reads its own hourly score through [`Workability::from_score`], the same for every
//! mode, and the answer says it has no per-mode figures ([`BestTimes::per_mode`]).
//!
//! **Which way round.** Per band and hour, the route the model favours is [`infer_route`] fed
//! this mode's figures; the favoured route's figures make the word. Where it names neither (or
//! near the antipode, where both routes are the same path within noise) the larger figure is
//! used and no route is claimed.
//!
//! **What it never says.** No percentages (the figures are relative and a reader takes them as
//! the chance of a contact); no time finer than the hour (the grid is hourly); nothing about
//! whether the station is on the air. The P.533 grid is a monthly median, so the week repeats the
//! same hours each day; only the Kp outlook tells one day from the next, and only for a path that
//! reaches the auroral zone at the forecast Kp.

use serde::Serialize;

use crate::kpforecast::KpForecast;
use crate::likelihood::{BandOutlook, Workability};
use crate::model::Band;
use crate::pathday::{path_day, PathDayHour};
use crate::pathgeo::KpSource;
use crate::pca::cutoff_lat_deg;
use crate::predict::PathPrediction;
use crate::route::{infer_route, Route, RouteEvidence, RouteVerdict, NEAR_ANTIPODE_KM};

/// Windows in the full answer; a card shows the first three.
pub const MAX_WINDOWS: usize = 5;
/// The absorption (dB, flare plus proton) at which the headline band is passed over now — the
/// line the "now" overlay's strong segments already draw at.
pub const PASS_OVER_DB: f32 = 10.0;
/// The forecast Kp from which a path reaching the auroral zone gets a note: NOAA's G1.
pub const STORM_KP: f32 = 5.0;

/// The mode an answer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum BestMode {
    #[serde(rename = "FT8")]
    Ft8,
    #[serde(rename = "FT4")]
    Ft4,
    #[serde(rename = "CW")]
    Cw,
    #[serde(rename = "SSB")]
    Ssb,
}

impl BestMode {
    /// The engine's own name for the mode (`mode_hourly`).
    pub fn label(self) -> &'static str {
        match self {
            BestMode::Ft8 => "FT8",
            BestMode::Ft4 => "FT4",
            BestMode::Cw => "CW",
            BestMode::Ssb => "SSB",
        }
    }

    /// `FT8`, `FT4`, `CW` or `SSB` (any case); anything else is no mode.
    pub fn parse(s: &str) -> Option<BestMode> {
        match s.trim().to_ascii_uppercase().as_str() {
            "FT8" => Some(BestMode::Ft8),
            "FT4" => Some(BestMode::Ft4),
            "CW" => Some(BestMode::Cw),
            "SSB" => Some(BestMode::Ssb),
            _ => None,
        }
    }
}

/// What the fold is asked.
#[derive(Debug, Clone, Copy)]
pub struct BestAsk<'a> {
    /// The short-path prediction, with its geometry (and the "now" overlay, when one stands).
    pub sp: &'a PathPrediction,
    /// The long-path prediction, when the engine models it.
    pub lp: Option<&'a PathPrediction>,
    pub mode: BestMode,
    /// The bands the operator may transmit on in this mode; `None` filters nothing.
    pub tx_bands: Option<&'a [String]>,
    /// The Kp outlook held, if any.
    pub forecast: Option<&'a KpForecast>,
    /// The request's time (unix seconds UTC).
    pub now: i64,
}

/// One band at one UTC hour, as the chart draws it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartCell {
    pub word: Workability,
    /// The route the model favours; absent when it names neither.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<Route>,
}

/// One band's 24 hours (index = UTC hour).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartBand {
    pub band: String,
    /// The operator's licence lets them transmit here in this mode. A band without it never
    /// appears in the headline or the windows; the chart dims it.
    pub licensed: bool,
    pub cells: Vec<ChartCell>,
}

/// What "now" says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NowKind {
    /// A licensed band is Fair or better this hour.
    Try,
    /// The best licensed band is only Marginal.
    LongShot,
    /// Every licensed band is Closed.
    NotNow,
}

/// A band and its word.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandWord {
    pub band: String,
    pub word: Workability,
}

/// Estimated absorption on a band now (a flare, a proton event), from the "now" overlay.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Absorbed {
    pub band: String,
    /// Whole dB, as estimated.
    pub db: u16,
}

/// The answer for this hour.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowAnswer {
    pub kind: NowKind,
    /// The band named (absent for [`NowKind::NotNow`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub band: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub word: Option<Workability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<Route>,
    /// When the band's run at Fair or better ends: the top of an hour (absent with `all_day`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until_unix: Option<i64>,
    /// The band is Fair or better in every hour of the day.
    pub all_day: bool,
    /// The next licensed band that is also Fair or better now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner_up: Option<BandWord>,
    /// The band that would have headlined, passed over for absorption of [`PASS_OVER_DB`] or more.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passed_over: Option<Absorbed>,
    /// Absorption under [`PASS_OVER_DB`] on the band named: it stays, with a note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub absorbed: Option<Absorbed>,
}

/// One window over the next 24 hours: a run of hours at Fair or better on one band.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub band: String,
    /// The top of its first hour, and of the hour after its last (unix seconds UTC).
    pub from_unix: i64,
    pub to_unix: i64,
    /// Its best hour's word.
    pub word: Workability,
    /// The route at its best hour, when the model names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<Route>,
    /// A short low-band run with an end on the grey line.
    pub greyline: bool,
    /// Only Marginal: offered because nothing reaches Fair.
    pub long_shot: bool,
    /// The highest forecast Kp of [`STORM_KP`] or more in its hours, on a route that reaches the
    /// auroral zone at that Kp. Never changes its rank.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storm_kp: Option<f32>,
}

/// A day of the week ahead whose forecast Kp reaches this path.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekFlag {
    /// Midnight UTC of the day.
    pub day_unix: i64,
    /// Its highest forecast Kp.
    pub kp: f32,
}

/// The week: the same hours each day, and the forecast days that differ.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Week {
    /// The last forecast period's start; absent when no forecast is held (nothing is claimed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast_until_unix: Option<i64>,
    pub flags: Vec<WeekFlag>,
}

/// The whole answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BestTimes {
    /// The engine the figures came from (`p533`, `heuristic`).
    pub engine: String,
    pub mode: BestMode,
    /// The engine gave per-mode figures; false means every mode reads the same.
    pub per_mode: bool,
    pub now: NowAnswer,
    /// By start time, at most [`MAX_WINDOWS`].
    pub windows: Vec<Window>,
    pub week: Week,
    /// Highest band first.
    pub chart: Vec<ChartBand>,
    /// 24 entries, UTC hour 0..23: the grey line and the hour's Kp (the path-over-the-day's hours).
    pub hours: Vec<PathDayHour>,
    /// The request's UTC hour.
    pub now_hour: u8,
    /// Any part of the answer rests on the long path (the screen labels it unvalidated).
    pub long_used: bool,
}

/// Order of the words, Closed lowest.
fn rank(w: Workability) -> u8 {
    match w {
        Workability::Closed => 0,
        Workability::Marginal => 1,
        Workability::Fair => 2,
        Workability::Good => 3,
        Workability::Excellent => 4,
    }
}

fn usable(w: Workability) -> bool {
    rank(w) >= rank(Workability::Fair)
}

/// One mode's figure at hour `h`, `None` when the band carries no figure for it.
fn mode_fig(b: &BandOutlook, mode: &str, h: usize) -> Option<f32> {
    b.mode_hourly
        .iter()
        .find(|m| m.mode == mode)
        .map(|m| m.hourly[h % 24])
}

/// The word and the deciding figure for `mode` on one band at hour `h` (see the module header).
fn hour_word(b: &BandOutlook, mode: BestMode, h: usize) -> (Workability, f32) {
    let own = mode_fig(b, mode.label(), h);
    let (Some(m), Some(cw), Some(ssb)) = (own, mode_fig(b, "CW", h), mode_fig(b, "SSB", h)) else {
        let s = b.hourly.get(h).copied().unwrap_or(0.0);
        return (Workability::from_score(s), s);
    };
    let w = if ssb >= 0.75 {
        Workability::Excellent
    } else if cw >= 0.75 && m >= 0.75 {
        Workability::Good
    } else if m >= 0.5 {
        Workability::Fair
    } else if m >= 0.1 {
        Workability::Marginal
    } else {
        Workability::Closed
    };
    (w, m)
}

/// A cell with its ranking strength (word, then figure).
#[derive(Debug, Clone, Copy)]
struct Cell {
    word: Workability,
    route: Option<Route>,
    fig: f32,
}

impl Cell {
    fn stronger(&self, o: &Cell) -> bool {
        (rank(self.word), self.fig) > (rank(o.word), o.fig)
    }
}

fn evidence(p: &PathPrediction, b: &BandOutlook, bcr: f32, h: usize) -> RouteEvidence {
    RouteEvidence {
        bcr,
        snr_db: b
            .detail
            .as_ref()
            .and_then(|d| d.get(h).copied().flatten())
            .map(|ph| ph.snr_db),
        greyline_frac: p
            .geometry
            .as_ref()
            .and_then(|g| g.hours.get(h))
            .map(|x| x.greyline_frac),
    }
}

/// Every band's 24 cells, in `sp.bands`' order.
fn grid(ask: &BestAsk) -> Vec<(String, [Cell; 24])> {
    let antipodal = match (
        ask.sp.geometry.as_ref(),
        ask.lp.and_then(|l| l.geometry.as_ref()),
    ) {
        (Some(s), Some(l)) => l.dist_km - s.dist_km < NEAR_ANTIPODE_KM,
        _ => false,
    };
    ask.sp
        .bands
        .iter()
        .map(|sb| {
            let lb = ask
                .lp
                .and_then(|lp| lp.bands.iter().find(|b| b.band == sb.band));
            let cells = std::array::from_fn(|h| {
                let (sw, sf) = hour_word(sb, ask.mode, h);
                let short = Cell {
                    word: sw,
                    route: None,
                    fig: sf,
                };
                let (Some(lp), Some(lb)) = (ask.lp, lb) else {
                    return short;
                };
                let (lw, lf) = hour_word(lb, ask.mode, h);
                let long = Cell {
                    word: lw,
                    route: None,
                    fig: lf,
                };
                let larger = if long.stronger(&short) { long } else { short };
                if antipodal {
                    return larger;
                }
                let call = infer_route(
                    &sb.band,
                    evidence(ask.sp, sb, sf, h),
                    evidence(lp, lb, lf, h),
                );
                match call.verdict {
                    RouteVerdict::Short => Cell {
                        route: Some(Route::Short),
                        ..short
                    },
                    RouteVerdict::Long => Cell {
                        route: Some(Route::Long),
                        ..long
                    },
                    RouteVerdict::Either | RouteVerdict::Unknown => larger,
                }
            });
            (sb.band.clone(), cells)
        })
        .collect()
}

/// A run of hours at the request's offsets `from..to` (0 = this hour).
#[derive(Debug, Clone, Copy)]
struct Run {
    band: usize,
    from: usize,
    to: usize,
    peak: Cell,
}

/// The runs of hours meeting `keep` on one band over the next 24 hours, from this hour on.
fn runs(
    cells: &[Cell; 24],
    band: usize,
    now_hour: usize,
    keep: impl Fn(Workability) -> bool,
) -> Vec<Run> {
    let at = |i: usize| cells[(now_hour + i) % 24];
    let mut out = Vec::new();
    let mut i = 0;
    while i < 24 {
        if !keep(at(i).word) {
            i += 1;
            continue;
        }
        let from = i;
        let mut peak = at(i);
        while i < 24 && keep(at(i).word) {
            if at(i).stronger(&peak) {
                peak = at(i);
            }
            i += 1;
        }
        out.push(Run {
            band,
            from,
            to: i,
            peak,
        });
    }
    // A run open now that also reaches the end of the 24 hours is one daily run seen twice: its
    // tail is tomorrow's start of the same hours, cut off by the horizon. It is the run open now.
    if out.len() > 1 && out[0].from == 0 && out[out.len() - 1].to == 24 {
        if let Some(tail) = out.pop() {
            if tail.peak.stronger(&out[0].peak) {
                out[0].peak = tail.peak;
            }
        }
    }
    out
}

/// Two runs overlap by more than half the shorter one.
fn overlap_more_than_half(a: &Run, b: &Run) -> bool {
    let o = a.to.min(b.to).saturating_sub(a.from.max(b.from));
    let shorter = (a.to - a.from).min(b.to - b.from);
    2 * o > shorter
}

/// Strongest first among overlapping runs; then by start.
fn merge_and_order(mut runs: Vec<Run>) -> Vec<Run> {
    runs.sort_by(|a, b| {
        (rank(b.peak.word), b.peak.fig)
            .partial_cmp(&(rank(a.peak.word), a.peak.fig))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<Run> = Vec::new();
    for r in runs {
        if !kept.iter().any(|k| overlap_more_than_half(k, &r)) {
            kept.push(r);
        }
    }
    kept.sort_by_key(|r| r.from);
    kept
}

fn earns_credit(band: &str) -> bool {
    Band::from_label(band).is_some_and(crate::awards::earns_dxcc_credit)
}

fn is_low(band: &str) -> bool {
    matches!(
        Band::from_label(band),
        Some(Band::B160 | Band::B80 | Band::B40)
    )
}

/// Estimated absorption now on `band` along `route`, from the overlay on that route.
fn absorbed_db(ask: &BestAsk, band: &str, route: Option<Route>) -> f32 {
    let p = match route {
        Some(Route::Long) => ask.lp.unwrap_or(ask.sp),
        _ => ask.sp,
    };
    p.overlay
        .as_ref()
        .and_then(|o| o.bands.iter().find(|b| b.band == band))
        .map_or(0.0, |b| b.flare_db.unwrap_or(0.0) + b.pca_db.unwrap_or(0.0))
}

/// The answer for `ask`.
pub fn best_times(ask: &BestAsk) -> BestTimes {
    let now_hour = (ask.now.rem_euclid(86_400) / 3600) as usize;
    let hour0 = ask.now - ask.now.rem_euclid(3600);
    let at_offset = |i: usize| hour0 + i as i64 * 3600;
    let cells = grid(ask);
    let licensed = |band: &str| ask.tx_bands.is_none_or(|l| l.iter().any(|b| b == band));
    let day_hours = path_day(ask.sp, ask.lp)
        .map(|d| d.hours)
        .unwrap_or_default();

    // --- Now: licensed bands, strongest first, then the chase rule for 60 m. ---
    let mut order: Vec<usize> = (0..cells.len())
        .filter(|&i| licensed(&cells[i].0))
        .collect();
    order.sort_by(|&a, &b| {
        let (ca, cb) = (&cells[a].1[now_hour], &cells[b].1[now_hour]);
        (rank(cb.word), cb.fig)
            .partial_cmp(&(rank(ca.word), ca.fig))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // `predict::rank_for_chase`'s rule on words: this is a chase answer, so the headline goes to
    // the best band that earns DXCC credit when one is Fair or better; nothing else moves.
    if let Some(p) = order
        .iter()
        .position(|&i| earns_credit(&cells[i].0) && usable(cells[i].1[now_hour].word))
    {
        order[..=p].rotate_right(1);
    }
    let mut passed_over = None;
    if let Some(&top) = order.first() {
        let c = cells[top].1[now_hour];
        let db = absorbed_db(ask, &cells[top].0, c.route);
        if usable(c.word) && db >= PASS_OVER_DB && order.len() > 1 {
            passed_over = Some(Absorbed {
                band: cells[top].0.clone(),
                db: db.round() as u16,
            });
            order.remove(0);
        }
    }
    let head = order.first().copied();
    let now = match head {
        Some(i) if rank(cells[i].1[now_hour].word) > 0 => {
            let c = cells[i].1[now_hour];
            let band = cells[i].0.clone();
            let run = runs(&cells[i].1, i, now_hour, usable)
                .into_iter()
                .find(|r| r.from == 0);
            let all_day = cells[i].1.iter().all(|c| usable(c.word));
            let db = absorbed_db(ask, &band, c.route);
            let kind = if usable(c.word) {
                NowKind::Try
            } else {
                NowKind::LongShot
            };
            NowAnswer {
                kind,
                band: Some(band.clone()),
                word: Some(c.word),
                route: c.route,
                until_unix: run.filter(|_| !all_day).map(|r| at_offset(r.to)),
                all_day: kind == NowKind::Try && all_day,
                runner_up: order
                    .iter()
                    .skip(1)
                    .find(|&&j| usable(cells[j].1[now_hour].word))
                    .filter(|_| kind == NowKind::Try)
                    .map(|&j| BandWord {
                        band: cells[j].0.clone(),
                        word: cells[j].1[now_hour].word,
                    }),
                absorbed: (db > 0.5 && db < PASS_OVER_DB).then(|| Absorbed {
                    band,
                    db: db.round() as u16,
                }),
                passed_over,
            }
        }
        _ => NowAnswer {
            kind: NowKind::NotNow,
            band: None,
            word: None,
            route: None,
            until_unix: None,
            all_day: false,
            runner_up: None,
            passed_over,
            absorbed: None,
        },
    };

    // --- Next best: runs at Fair or better, minus the run named now. ---
    let lic: Vec<usize> = (0..cells.len())
        .filter(|&i| licensed(&cells[i].0))
        .collect();
    let mut fair: Vec<Run> = lic
        .iter()
        .flat_map(|&i| runs(&cells[i].1, i, now_hour, usable))
        .filter(|r| !(now.kind == NowKind::Try && Some(r.band) == head && r.from == 0))
        .collect();
    let mut long_shot = false;
    if fair.is_empty() && now.kind != NowKind::Try {
        let marginal: Vec<Run> = lic
            .iter()
            .flat_map(|&i| runs(&cells[i].1, i, now_hour, |w| rank(w) == 1))
            .filter(|r| !(Some(r.band) == head && r.from == 0))
            .collect();
        if let Some(best) = merge_and_order(marginal).into_iter().max_by(|a, b| {
            a.peak
                .fig
                .partial_cmp(&b.peak.fig)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            fair = vec![best];
            long_shot = true;
        }
    }
    let grey_at = |offset: usize| {
        day_hours
            .get((now_hour + offset) % 24)
            .is_some_and(|h| h.grey_me || h.grey_dx)
    };
    let storm_at = |offset: usize, route: Option<Route>| -> Option<f32> {
        let h = day_hours.get((now_hour + offset) % 24)?;
        let polar = match route {
            Some(Route::Long) => h.polar_long == Some(true),
            _ => h.polar_short,
        };
        (h.kp_source == KpSource::Forecast && polar)
            .then_some(h.kp)
            .flatten()
            .filter(|&kp| kp >= STORM_KP)
    };
    let windows: Vec<Window> = merge_and_order(fair)
        .into_iter()
        .take(MAX_WINDOWS)
        .map(|r| {
            let band = cells[r.band].0.clone();
            Window {
                greyline: is_low(&band)
                    && r.to - r.from <= 2
                    && (grey_at(r.from) || grey_at(r.to - 1)),
                storm_kp: (r.from..r.to)
                    .filter_map(|o| storm_at(o, r.peak.route))
                    .fold(None, |m: Option<f32>, k| Some(m.map_or(k, |m| m.max(k)))),
                band,
                from_unix: at_offset(r.from),
                to_unix: at_offset(r.to),
                word: r.peak.word,
                route: r.peak.route,
                long_shot,
            }
        })
        .collect();

    // --- The week: the Kp outlook's days that reach this path. ---
    let reach = |route: Route| {
        let p = match route {
            Route::Short => Some(ask.sp),
            Route::Long => ask.lp,
        };
        p.and_then(|p| p.geometry.as_ref()).map(|g| g.max_gmlat_deg)
    };
    let long_used =
        now.route == Some(Route::Long) || windows.iter().any(|w| w.route == Some(Route::Long));
    let crosses_at = |kp: f32| {
        let at = |g: Option<f32>| g.is_some_and(|g| g as f64 >= cutoff_lat_deg(kp as f64));
        at(reach(Route::Short)) || (long_used && at(reach(Route::Long)))
    };
    let today0 = ask.now - ask.now.rem_euclid(86_400);
    let mut flags: Vec<WeekFlag> = Vec::new();
    let mut until = None;
    if let Some(f) = ask.forecast {
        for p in f.forward().filter(|p| p.time_unix + 3 * 3600 > ask.now) {
            until = Some(until.map_or(p.time_unix, |u: i64| u.max(p.time_unix)));
            let day = p.time_unix - p.time_unix.rem_euclid(86_400);
            if day < today0 || p.kp < STORM_KP || !crosses_at(p.kp) {
                continue;
            }
            match flags.iter_mut().find(|f| f.day_unix == day) {
                Some(f) => f.kp = f.kp.max(p.kp),
                None => flags.push(WeekFlag {
                    day_unix: day,
                    kp: p.kp,
                }),
            }
        }
    }
    flags.sort_by_key(|f| f.day_unix);

    // --- The chart: every band, highest first. ---
    let mhz = |b: &str| Band::from_label(b).map_or(0.0, |b| b.center_mhz());
    let mut chart: Vec<ChartBand> = cells
        .iter()
        .map(|(band, c)| ChartBand {
            band: band.clone(),
            licensed: licensed(band),
            cells: c
                .iter()
                .map(|c| ChartCell {
                    word: c.word,
                    route: c.route,
                })
                .collect(),
        })
        .collect();
    chart.sort_by(|a, b| mhz(&b.band).total_cmp(&mhz(&a.band)));

    BestTimes {
        engine: ask.sp.engine.clone(),
        mode: ask.mode,
        per_mode: ask.sp.bands.iter().any(|b| !b.mode_hourly.is_empty()),
        now,
        windows,
        week: Week {
            forecast_until_unix: until,
            flags,
        },
        chart,
        hours: day_hours,
        now_hour: now_hour as u8,
        long_used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kpforecast::{KpKind, KpPoint};
    use crate::likelihood::ModeHourly;
    use crate::pathgeo::path_geometry_route;
    use crate::pathnow::{BandNow, OverlaySource, PathHourOverlay};

    /// 2026-05-15 12:00 UTC.
    const MAY_12Z: i64 = 1_778_846_400;
    const BOSTON: (f64, f64) = (42.36, -71.06);
    const LONDON: (f64, f64) = (51.5, -0.12);
    const TOKYO: (f64, f64) = (35.68, 139.69);

    /// A band whose every mode reads `f(h)` per hour (so FT8, CW and SSB agree).
    fn flat(name: &str, f: impl Fn(usize) -> f32) -> BandOutlook {
        modes(name, &f, &f, &f)
    }

    /// A band with FT8/FT4 at `ft8(h)`, CW at `cw(h)` and SSB at `ssb(h)`.
    fn modes(
        name: &str,
        ft8: &dyn Fn(usize) -> f32,
        cw: &dyn Fn(usize) -> f32,
        ssb: &dyn Fn(usize) -> f32,
    ) -> BandOutlook {
        let one = |m: &str, f: &dyn Fn(usize) -> f32| ModeHourly {
            mode: m.into(),
            hourly: std::array::from_fn(f),
        };
        BandOutlook {
            band: name.into(),
            workability: "Fair".into(),
            score: 0.5,
            window: String::new(),
            grayline: false,
            hourly: (0..24).map(ft8).collect(),
            reliability: 0.0,
            mode_now: Vec::new(),
            mode_hourly: vec![
                one("FT8", ft8),
                one("FT4", ft8),
                one("CW", cw),
                one("SSB", ssb),
            ],
            detail: None,
        }
    }

    fn pred_to(
        dx: (f64, f64),
        bands: Vec<BandOutlook>,
        route: Route,
        kp: Option<f32>,
    ) -> PathPrediction {
        PathPrediction {
            engine: "p533".into(),
            bands,
            muf_now: 0.0,
            muf_hourly: vec![20.0; 24],
            geometry: Some(path_geometry_route(
                BOSTON, dx, route, MAY_12Z, kp, None, None,
            )),
            long_path: None,
            routes: None,
            day: None,
            overlay: None,
            space_now: None,
            engine_fallback: false,
            storm: None,
            power: None,
        }
    }

    fn pred(bands: Vec<BandOutlook>) -> PathPrediction {
        pred_to(LONDON, bands, Route::Short, Some(2.0))
    }

    fn ask<'a>(
        sp: &'a PathPrediction,
        lp: Option<&'a PathPrediction>,
        mode: BestMode,
    ) -> BestAsk<'a> {
        BestAsk {
            sp,
            lp,
            mode,
            tx_bands: None,
            forecast: None,
            now: MAY_12Z,
        }
    }

    /// Open (Good: every mode 0.8) in UTC hours `from..to`, wrapping; closed elsewhere.
    fn open(from: usize, to: usize) -> impl Fn(usize) -> f32 {
        move |h| {
            let inside = if from <= to {
                (from..to).contains(&h)
            } else {
                h >= from || h < to
            };
            if inside {
                0.8
            } else {
                0.0
            }
        }
    }

    #[test]
    fn now_picks_the_best_licensed_band_and_never_an_unlicensed_one() {
        let sp = pred(vec![
            flat("20m", |_| 0.9),
            flat("40m", |_| 0.6),
            flat("80m", |_| 0.2),
        ]);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.now.kind, NowKind::Try);
        assert_eq!(a.now.band.as_deref(), Some("20m"));
        assert_eq!(
            a.now.runner_up.as_ref().map(|r| r.band.as_str()),
            Some("40m")
        );
        // A licence without 20 m: 40 m leads, 20 m is nowhere in the answer but the chart, dimmed.
        let tx = vec!["40m".to_string(), "80m".to_string()];
        let a = best_times(&BestAsk {
            tx_bands: Some(&tx),
            ..ask(&sp, None, BestMode::Ft8)
        });
        assert_eq!(a.now.band.as_deref(), Some("40m"));
        assert!(a.windows.iter().all(|w| w.band != "20m"));
        let c20 = a.chart.iter().find(|b| b.band == "20m").unwrap();
        assert!(!c20.licensed);
        assert!(a.chart.iter().find(|b| b.band == "40m").unwrap().licensed);
    }

    #[test]
    fn until_stops_at_the_end_of_the_run_and_wraps_past_midnight() {
        // 12Z now; 20 m open 10Z-15Z: until 15Z today.
        let sp = pred(vec![flat("20m", open(10, 15))]);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.now.until_unix, Some(MAY_12Z + 3 * 3600));
        assert!(!a.now.all_day);
        // Open 10Z-03Z (across midnight): until 03Z tomorrow.
        let sp = pred(vec![flat("20m", open(10, 3))]);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.now.until_unix, Some(MAY_12Z + 15 * 3600));
        // Every hour on the hour.
        assert_eq!(a.now.until_unix.unwrap() % 3600, 0);
    }

    #[test]
    fn open_all_day_reads_as_all_day() {
        let sp = pred(vec![flat("20m", |_| 0.9)]);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert!(a.now.all_day);
        assert_eq!(a.now.until_unix, None);
        // Negative control: one closed hour and it is not all day.
        let sp = pred(vec![flat("20m", |h| if h == 3 { 0.0 } else { 0.9 })]);
        assert!(!best_times(&ask(&sp, None, BestMode::Ft8)).now.all_day);
    }

    #[test]
    fn nothing_above_closed_is_the_no_hour_state() {
        let sp = pred(vec![flat("20m", |_| 0.01), flat("40m", |_| 0.0)]);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.now.kind, NowKind::NotNow);
        assert!(a.windows.is_empty());
    }

    #[test]
    fn marginal_only_is_a_long_shot() {
        let sp = pred(vec![
            flat("20m", |_| 0.2),
            flat("40m", |h| if h < 6 { 0.3 } else { 0.0 }),
        ]);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.now.kind, NowKind::LongShot);
        assert_eq!(a.now.band.as_deref(), Some("20m"));
        assert_eq!(a.now.runner_up, None);
        // The one window offered is the strongest Marginal run, labelled a long shot.
        assert_eq!(a.windows.len(), 1);
        assert!(a.windows[0].long_shot);
        assert_eq!(a.windows[0].band, "40m");
        // Negative control: with a Fair run somewhere, nothing is labelled a long shot.
        let sp = pred(vec![
            flat("20m", |_| 0.2),
            flat("40m", |h| if h < 6 { 0.6 } else { 0.0 }),
        ]);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert!(a.windows.iter().all(|w| !w.long_shot));
    }

    #[test]
    fn sixty_metres_cannot_headline_when_a_creditable_band_clears_fair_and_does_when_none_does() {
        let sp = pred(vec![flat("60m", |_| 0.9), flat("40m", |_| 0.6)]);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.now.band.as_deref(), Some("40m"));
        assert_eq!(
            a.now.runner_up.as_ref().map(|r| r.band.as_str()),
            Some("60m")
        );
        let sp = pred(vec![flat("60m", |_| 0.9), flat("40m", |_| 0.2)]);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.now.band.as_deref(), Some("60m"));
    }

    #[test]
    fn ssb_and_ft8_on_one_path_answer_differently_and_identical_figures_identically() {
        // FT8 usable all day on 20 m, SSB only Marginal: FT8 is told 20 m, SSB a long shot.
        let b = modes("20m", &|_| 0.9, &|_| 0.6, &|_| 0.2);
        let sp = pred(vec![b]);
        let ft8 = best_times(&ask(&sp, None, BestMode::Ft8));
        let ssb = best_times(&ask(&sp, None, BestMode::Ssb));
        assert_eq!(ft8.now.kind, NowKind::Try);
        assert_eq!(ssb.now.kind, NowKind::LongShot);
        assert_ne!(ft8.now, ssb.now);
        // Negative control: every mode reading the same gives the same answer, mode aside.
        let sp = pred(vec![flat("20m", open(10, 15))]);
        let ft8 = best_times(&ask(&sp, None, BestMode::Ft8));
        let ssb = best_times(&ask(&sp, None, BestMode::Ssb));
        assert_eq!(
            (ft8.now, ft8.windows, ft8.chart),
            (ssb.now, ssb.windows, ssb.chart)
        );
    }

    #[test]
    fn the_ft8_hour_word_is_the_apps_ladder_and_good_needs_the_mode_itself() {
        let b = modes("20m", &|_| 0.6, &|_| 0.8, &|_| 0.2);
        // FT8 at 0.6: Fair on its own, but CW at 0.8 makes the band Good only if FT8 also clears
        // 0.75 — it does not, so Fair.
        assert_eq!(hour_word(&b, BestMode::Ft8, 0).0, Workability::Fair);
        assert_eq!(hour_word(&b, BestMode::Cw, 0).0, Workability::Good);
        let b = modes("20m", &|_| 0.9, &|_| 0.8, &|_| 0.8);
        assert_eq!(hour_word(&b, BestMode::Ft8, 0).0, Workability::Excellent);
        assert_eq!(hour_word(&b, BestMode::Ssb, 0).0, Workability::Excellent);
    }

    #[test]
    fn a_long_only_band_carries_the_long_mark_and_near_the_antipode_no_route_is_claimed() {
        let sp = pred_to(TOKYO, vec![flat("20m", |_| 0.0)], Route::Short, Some(2.0));
        let lp = pred_to(TOKYO, vec![flat("20m", |_| 0.8)], Route::Long, Some(2.0));
        let a = best_times(&ask(&sp, Some(&lp), BestMode::Ft8));
        assert_eq!(a.now.route, Some(Route::Long));
        assert!(a.long_used);
        assert!(a.chart[0]
            .cells
            .iter()
            .all(|c| c.route == Some(Route::Long)));
        // Near Boston's antipode (south-west of Australia) no route is claimed.
        let antipode = (-41.5, 108.0);
        let sp = pred_to(
            antipode,
            vec![flat("20m", |_| 0.0)],
            Route::Short,
            Some(2.0),
        );
        let lp = pred_to(antipode, vec![flat("20m", |_| 0.8)], Route::Long, Some(2.0));
        let a = best_times(&ask(&sp, Some(&lp), BestMode::Ft8));
        assert_eq!(a.now.kind, NowKind::Try);
        assert_eq!(a.now.route, None);
        assert!(!a.long_used);
    }

    #[test]
    fn windows_skip_the_now_run_merge_overlaps_to_the_strongest_and_cap_at_five() {
        // 20 m open now (10Z-15Z) and again 20Z-23Z; 40 m 21Z-23Z (overlaps 20 m's evening, weaker).
        let weak = |h: usize| if (21..23).contains(&h) { 0.55 } else { 0.0 };
        let sp = pred(vec![
            flat("20m", |h| {
                if (10..15).contains(&h) || (20..23).contains(&h) {
                    0.9
                } else {
                    0.0
                }
            }),
            flat("40m", weak),
        ]);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.now.band.as_deref(), Some("20m"));
        // The now run is not repeated; the 40 m run under more than half overlap merges away.
        assert_eq!(a.windows.len(), 1);
        assert_eq!(a.windows[0].band, "20m");
        assert_eq!(a.windows[0].from_unix, MAY_12Z + 8 * 3600);
        assert_eq!(a.windows[0].to_unix, MAY_12Z + 11 * 3600);
        // Many separate runs: by start time, at most five.
        let bands: Vec<BandOutlook> = ["10m", "12m", "15m", "17m", "30m", "40m", "80m"]
            .iter()
            .enumerate()
            .map(|(i, b)| flat(b, open(14 + i * 2, 15 + i * 2)))
            .collect();
        let sp = pred(bands);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.windows.len(), MAX_WINDOWS);
        assert!(a
            .windows
            .windows(2)
            .all(|w| w[0].from_unix <= w[1].from_unix));
    }

    fn with_flare(mut sp: PathPrediction, band: &str, db: f32) -> PathPrediction {
        sp.overlay = Some(PathHourOverlay {
            unix: MAY_12Z,
            kp: Some(2.0),
            kp_source: KpSource::Current,
            cap_lat_deg: None,
            source: OverlaySource::default(),
            bands: vec![BandNow {
                band: band.into(),
                hops: 2,
                hops_modelled: true,
                flare_db: Some(db),
                pca_db: None,
                flare_from: None,
                segments: Vec::new(),
            }],
        });
        sp
    }

    #[test]
    fn a_flare_of_ten_db_passes_the_headline_on_and_changes_no_score() {
        let base = pred(vec![flat("20m", |_| 0.9), flat("15m", |_| 0.6)]);
        let before = best_times(&ask(&base, None, BestMode::Ft8));
        let sp = with_flare(base.clone(), "20m", 12.0);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.now.band.as_deref(), Some("15m"));
        assert_eq!(
            a.now.passed_over,
            Some(Absorbed {
                band: "20m".into(),
                db: 12
            })
        );
        // Scores untouched: the chart is the same as without the flare.
        assert_eq!(a.chart, before.chart);
        // Under 10 dB the band stays, with a note.
        let sp = with_flare(base, "20m", 4.0);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.now.band.as_deref(), Some("20m"));
        assert_eq!(
            a.now.absorbed,
            Some(Absorbed {
                band: "20m".into(),
                db: 4
            })
        );
        assert_eq!(a.now.passed_over, None);
    }

    fn forecast(kp: f32) -> KpForecast {
        KpForecast {
            points: (0..24)
                .map(|i| KpPoint {
                    time_unix: MAY_12Z + i * 3 * 3600,
                    kp,
                    kind: KpKind::Predicted,
                    noaa_scale: None,
                })
                .collect(),
            fetched_at: None,
        }
    }

    #[test]
    fn a_polar_path_at_forecast_kp_five_is_flagged_and_an_equatorial_one_at_nine_is_not() {
        let f = forecast(5.0);
        let sp = pred_to(TOKYO, vec![flat("20m", |_| 0.9)], Route::Short, Some(2.0));
        let a = best_times(&BestAsk {
            forecast: Some(&f),
            ..ask(&sp, None, BestMode::Ft8)
        });
        assert!(!a.week.flags.is_empty());
        assert!(a.week.flags.iter().all(|f| f.kp == 5.0));
        assert_eq!(a.week.forecast_until_unix, Some(MAY_12Z + 23 * 3 * 3600));
        // Singapore-Nairobi stays clear of the zone even at Kp 9.
        let f9 = forecast(9.0);
        let mut eq = pred_to(TOKYO, vec![flat("20m", |_| 0.9)], Route::Short, Some(2.0));
        eq.geometry = Some(path_geometry_route(
            (1.35, 103.8),
            (-1.29, 36.82),
            Route::Short,
            MAY_12Z,
            Some(9.0),
            None,
            None,
        ));
        let a = best_times(&BestAsk {
            forecast: Some(&f9),
            ..ask(&eq, None, BestMode::Ft8)
        });
        assert!(a.week.flags.is_empty());
        // And a quiet forecast flags nothing on the polar path either.
        let f2 = forecast(3.0);
        let a = best_times(&BestAsk {
            forecast: Some(&f2),
            ..ask(&sp, None, BestMode::Ft8)
        });
        assert!(a.week.flags.is_empty());
    }

    #[test]
    fn unknown_kp_claims_nothing() {
        let sp = pred_to(TOKYO, vec![flat("20m", open(14, 17))], Route::Short, None);
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert_eq!(a.week.forecast_until_unix, None);
        assert!(a.week.flags.is_empty());
        assert!(a.windows.iter().all(|w| w.storm_kp.is_none()));
    }

    #[test]
    fn heuristic_input_without_mode_figures_answers_without_panic() {
        let mut b = flat("20m", open(10, 15));
        b.mode_hourly.clear();
        let mut sp = pred(vec![b]);
        sp.engine = "heuristic".into();
        let a = best_times(&ask(&sp, None, BestMode::Ssb));
        assert!(!a.per_mode);
        assert_eq!(a.now.kind, NowKind::Try);
        assert_eq!(a.engine, "heuristic");
        // No geometry at all (no operator grid): still an answer, with no hours.
        sp.geometry = None;
        let a = best_times(&ask(&sp, None, BestMode::Ft8));
        assert!(a.hours.is_empty());
    }

    #[test]
    fn no_word_in_the_answer_is_a_number_of_percent() {
        let sp = pred(vec![flat("20m", open(10, 15)), flat("40m", |_| 0.6)]);
        let json = serde_json::to_string(&best_times(&ask(&sp, None, BestMode::Ft8))).unwrap();
        assert!(!json.contains('%'));
        assert!(!json.contains("pct"), "{json}");
    }
}
