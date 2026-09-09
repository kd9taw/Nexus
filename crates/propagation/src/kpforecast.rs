//! NOAA SWPC planetary-K forecast — the three-day outlook.
//!
//! Nexus can say what the ionosphere is doing now; this is the one question it could
//! not answer: **when does it get better?** SWPC publishes Kp on a single 3-hourly
//! series that runs about a week back and three days forward, and every sample is
//! labelled with how it was arrived at, so the measured-vs-modelled distinction the
//! rest of this crate insists on comes straight off the wire rather than being
//! inferred here.
//!
//! Pure parsing only (no `live` feature needed); the HTTP fetch lives in
//! [`crate::live::swpc`].

use serde::{Deserialize, Serialize};

/// How a Kp sample was arrived at. SWPC's own word for it, not our guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KpKind {
    /// Measured from the ground magnetometer network.
    Observed,
    /// SWPC's estimate for a period whose observations are not final yet.
    Estimated,
    /// Modelled — the actual forecast.
    Predicted,
}

impl KpKind {
    fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "observed" => Self::Observed,
            "estimated" => Self::Estimated,
            // Anything SWPC has not measured is a model output. Defaulting the
            // UNKNOWN case to Predicted is deliberate: labelling a modelled value
            // "observed" would overstate what we know, and this feed's whole job is
            // to keep that line visible.
            _ => Self::Predicted,
        }
    }

    /// True for a value SWPC actually measured.
    pub fn is_measured(self) -> bool {
        matches!(self, Self::Observed)
    }
}

/// One 3-hourly planetary-K sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KpPoint {
    /// Start of the 3-hour period, unix seconds UTC.
    pub time_unix: i64,
    pub kp: f32,
    pub kind: KpKind,
    /// NOAA G-scale for the period ("G1".."G5") when the feed names one. Null on a
    /// quiet sky, which is the usual case.
    pub noaa_scale: Option<String>,
}

/// The parsed outlook: the whole series, split at "now" by the feed's own labels.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KpForecast {
    /// Every sample the feed carried, oldest first.
    pub points: Vec<KpPoint>,
}

impl KpForecast {
    /// Samples SWPC has not measured — the forward-looking part of the series.
    pub fn forward(&self) -> impl Iterator<Item = &KpPoint> {
        self.points.iter().filter(|p| !p.kind.is_measured())
    }

    /// The worst period still ahead, which is the thing an operator plans around.
    /// `None` when the feed carried no forward samples at all.
    pub fn peak_ahead(&self) -> Option<&KpPoint> {
        self.forward()
            .max_by(|a, b| a.kp.partial_cmp(&b.kp).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// The first period ahead at or above `kp`, i.e. "when does it get bad".
    pub fn next_at_or_above(&self, kp: f32) -> Option<&KpPoint> {
        self.forward().find(|p| p.kp >= kp)
    }

    /// The first period ahead at or below `kp`, i.e. "when does it get better".
    /// Only meaningful while conditions are currently disturbed.
    pub fn next_at_or_below(&self, kp: f32) -> Option<&KpPoint> {
        self.forward().find(|p| p.kp <= kp)
    }

    /// The most recent measured sample — what it is doing right now.
    pub fn latest_measured(&self) -> Option<&KpPoint> {
        self.points.iter().rev().find(|p| p.kind.is_measured())
    }
}

/// Parse `noaa-planetary-k-index-forecast.json`: an array of objects with
/// `time_tag`, `kp`, `observed` and `noaa_scale`. Rows without a usable time or Kp
/// are skipped; malformed JSON yields an empty forecast rather than an error, so a
/// feed change degrades to "no outlook" instead of taking a panel down.
///
/// ⚠️ The `kp` field is a NUMBER here, unlike several sibling SWPC products that
/// deliver a header row and string cells. Both spellings are accepted anyway, because
/// this feed has changed shape before and a string "3.00" must not read as no data.
pub fn parse_kp_forecast(json: &str) -> KpForecast {
    let rows: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    let mut points: Vec<KpPoint> = rows
        .iter()
        .filter_map(|v| {
            let t = v.get("time_tag").and_then(|x| x.as_str())?;
            let time_unix = crate::kc2g::parse_naive_utc_unix(t)?;
            let raw = v.get("kp")?;
            let kp = raw
                .as_f64()
                .or_else(|| raw.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
                .filter(|x| x.is_finite() && (0.0..=9.0).contains(x))? as f32;
            Some(KpPoint {
                time_unix,
                kp,
                kind: KpKind::parse(v.get("observed").and_then(|x| x.as_str()).unwrap_or("")),
                noaa_scale: v
                    .get("noaa_scale")
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            })
        })
        .collect();
    points.sort_by_key(|p| p.time_unix);
    KpForecast { points }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed real-shape payload (services.swpc.noaa.gov/products/
    /// noaa-planetary-k-index-forecast.json), including all three `observed` words.
    const REAL: &str = r#"[
      {"time_tag":"2026-08-31T09:00:00","kp":2.33,"observed":"observed","noaa_scale":null},
      {"time_tag":"2026-08-31T12:00:00","kp":1.67,"observed":"observed","noaa_scale":null},
      {"time_tag":"2026-08-31T18:00:00","kp":1.33,"observed":"estimated","noaa_scale":null},
      {"time_tag":"2026-09-01T00:00:00","kp":3.00,"observed":"predicted","noaa_scale":null},
      {"time_tag":"2026-09-01T03:00:00","kp":5.67,"observed":"predicted","noaa_scale":"G2"},
      {"time_tag":"2026-09-01T06:00:00","kp":2.00,"observed":"predicted","noaa_scale":null}
    ]"#;

    #[test]
    fn parses_the_real_shape_and_keeps_swpcs_own_labels() {
        let f = parse_kp_forecast(REAL);
        assert_eq!(f.points.len(), 6);
        assert_eq!(f.points[0].kp, 2.33);
        assert_eq!(f.points[0].kind, KpKind::Observed);
        assert_eq!(f.points[2].kind, KpKind::Estimated);
        assert_eq!(f.points[3].kind, KpKind::Predicted);
        assert_eq!(f.points[4].noaa_scale.as_deref(), Some("G2"));
        assert_eq!(f.points[0].noaa_scale, None, "a null scale became a string");
    }

    /// ⚠️ ESTIMATED IS NOT MEASURED. SWPC estimates a period before its observations
    /// are final, so it belongs with the forward-looking half — calling it measured
    /// would report a modelled number as fact, which is the one thing this feed is
    /// here to keep straight.
    #[test]
    fn estimated_counts_as_forward_not_measured() {
        let f = parse_kp_forecast(REAL);
        assert_eq!(f.forward().count(), 4);
        assert_eq!(
            f.latest_measured().map(|p| p.kp),
            Some(1.67),
            "an estimated sample was reported as the latest measurement"
        );
    }

    #[test]
    fn finds_the_peak_and_the_crossings_ahead() {
        let f = parse_kp_forecast(REAL);
        assert_eq!(f.peak_ahead().map(|p| p.kp), Some(5.67));
        // "when does it get bad" is the FIRST crossing, not the worst one.
        assert_eq!(
            f.next_at_or_above(5.0).map(|p| p.time_unix),
            f.points[4].time_unix.into()
        );
        assert!(f.next_at_or_above(9.0).is_none());
        // "when does it get better" reads forward from now.
        assert_eq!(f.next_at_or_below(1.5).map(|p| p.kp), Some(1.33));
    }

    /// A feed change must degrade to "no outlook", never to a panic or a wrong number.
    #[test]
    fn junk_degrades_to_empty() {
        assert!(parse_kp_forecast("not json").points.is_empty());
        assert!(parse_kp_forecast("{}").points.is_empty());
        // Out-of-range and unparseable rows are dropped, good rows survive.
        let mixed = r#"[
          {"time_tag":"2026-09-01T00:00:00","kp":99.0,"observed":"predicted"},
          {"time_tag":"nonsense","kp":3.0,"observed":"predicted"},
          {"time_tag":"2026-09-01T03:00:00","kp":"3.33","observed":"predicted"}
        ]"#;
        let f = parse_kp_forecast(mixed);
        assert_eq!(
            f.points.len(),
            1,
            "a bad row survived or a good one was dropped"
        );
        assert_eq!(
            f.points[0].kp, 3.33,
            "the string spelling of kp was refused"
        );
    }

    /// The series is time-ordered even if the feed is not, because every accessor
    /// above ("the FIRST period ahead", "the most recent measurement") is an
    /// order-dependent claim.
    #[test]
    fn points_come_out_in_time_order() {
        let shuffled = r#"[
          {"time_tag":"2026-09-01T06:00:00","kp":2.0,"observed":"predicted"},
          {"time_tag":"2026-09-01T00:00:00","kp":3.0,"observed":"predicted"}
        ]"#;
        let f = parse_kp_forecast(shuffled);
        assert!(f.points[0].time_unix < f.points[1].time_unix);
    }
}
