//! Shared domain types for the propagation pillars: bands, world regions, a
//! two-ended path spot, space-weather, and the small enums the advisor /
//! detector / dxped tracker share. Pure data + cheap geo glue.

use serde::{Deserialize, Serialize};

use crate::geo::maidenhead_to_latlon;

/// HF/VHF bands Nexus reasons about (FT8/FT4 relevant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Band {
    B160,
    B80,
    B40,
    B30,
    B20,
    B17,
    B15,
    B12,
    B10,
    B6,
    B4,
    B2,
}

impl Band {
    pub const ALL: [Band; 12] = [
        Band::B160,
        Band::B80,
        Band::B40,
        Band::B30,
        Band::B20,
        Band::B17,
        Band::B15,
        Band::B12,
        Band::B10,
        Band::B6,
        Band::B4,
        Band::B2,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Band::B160 => "160m",
            Band::B80 => "80m",
            Band::B40 => "40m",
            Band::B30 => "30m",
            Band::B20 => "20m",
            Band::B17 => "17m",
            Band::B15 => "15m",
            Band::B12 => "12m",
            Band::B10 => "10m",
            Band::B6 => "6m",
            Band::B4 => "4m",
            Band::B2 => "2m",
        }
    }

    /// Representative center frequency (MHz).
    pub fn center_mhz(self) -> f64 {
        match self {
            Band::B160 => 1.9,
            Band::B80 => 3.6,
            Band::B40 => 7.1,
            Band::B30 => 10.13,
            Band::B20 => 14.1,
            Band::B17 => 18.1,
            Band::B15 => 21.2,
            Band::B12 => 24.9,
            Band::B10 => 28.5,
            Band::B6 => 50.2,
            Band::B4 => 70.2,
            Band::B2 => 144.2,
        }
    }

    /// Is this a VHF band where "openings" (Es/F2/aurora/MS) are the story?
    pub fn is_vhf(self) -> bool {
        matches!(self, Band::B6 | Band::B4 | Band::B2)
    }

    /// Parse a band label ("20m", "160M") back to a [`Band`] (inverse of
    /// [`Band::label`], case-insensitive). Used when ingesting ADIF log rows.
    pub fn from_label(s: &str) -> Option<Band> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "160m" => Band::B160,
            "80m" => Band::B80,
            "40m" => Band::B40,
            "30m" => Band::B30,
            "20m" => Band::B20,
            "17m" => Band::B17,
            "15m" => Band::B15,
            "12m" => Band::B12,
            "10m" => Band::B10,
            "6m" => Band::B6,
            "4m" => Band::B4,
            "2m" => Band::B2,
            _ => return None,
        })
    }

    /// Map a frequency (MHz) to its band.
    pub fn from_mhz(f: f64) -> Option<Band> {
        let b = match f {
            x if (1.8..2.0).contains(&x) => Band::B160,
            x if (3.5..4.0).contains(&x) => Band::B80,
            x if (7.0..7.3).contains(&x) => Band::B40,
            x if (10.1..10.15).contains(&x) => Band::B30,
            x if (14.0..14.35).contains(&x) => Band::B20,
            x if (18.0..18.2).contains(&x) => Band::B17,
            x if (21.0..21.45).contains(&x) => Band::B15,
            x if (24.8..25.0).contains(&x) => Band::B12,
            x if (28.0..29.7).contains(&x) => Band::B10,
            x if (50.0..54.0).contains(&x) => Band::B6,
            x if (70.0..71.0).contains(&x) => Band::B4,
            x if (144.0..148.0).contains(&x) => Band::B2,
            _ => return None,
        };
        Some(b)
    }
}

/// DXCC mode-award class. Awards (and "new mode" needs) are tracked by class —
/// CW / Phone / Digital — not by individual submode. Nexus operates Digital, so
/// its work-now cards evaluate [`ModeClass::Digital`]; an imported ADIF log's
/// CW/SSB contacts still classify correctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModeClass {
    Cw,
    Phone,
    Digital,
}

impl ModeClass {
    /// Classify an ADIF MODE string. Anything not clearly CW or phone (incl.
    /// FT8/FT4/FT1/RTTY/PSK/JT* and blank) is treated as Digital.
    pub fn from_adif(mode: &str) -> ModeClass {
        match mode.trim().to_ascii_uppercase().as_str() {
            "CW" => ModeClass::Cw,
            "SSB" | "USB" | "LSB" | "AM" | "FM" | "PHONE" | "DV" | "C4FM" => ModeClass::Phone,
            _ => ModeClass::Digital,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ModeClass::Cw => "CW",
            ModeClass::Phone => "Phone",
            ModeClass::Digital => "Digital",
        }
    }
}

/// Coarse world region (for "point NE at Europe" style guidance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Region {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Africa,
    Asia,
    Oceania,
    Unknown,
}

impl Region {
    pub fn label(self) -> &'static str {
        match self {
            Region::NorthAmerica => "North America",
            Region::SouthAmerica => "South America",
            Region::Europe => "Europe",
            Region::Africa => "Africa",
            Region::Asia => "Asia",
            Region::Oceania => "Oceania",
            Region::Unknown => "—",
        }
    }

    /// Crude continent binning from lat/lon (good enough for direction hints).
    pub fn from_latlon(lat: f64, lon: f64) -> Region {
        // Order matters; first matching box wins.
        if (35.0..72.0).contains(&lat) && (-12.0..40.0).contains(&lon) {
            Region::Europe
        } else if (5.0..75.0).contains(&lat) && (40.0..180.0).contains(&lon) {
            Region::Asia
        } else if (-50.0..5.0).contains(&lat) && (110.0..180.0).contains(&lon) {
            Region::Oceania
        } else if (-35.0..37.0).contains(&lat) && (-18.0..52.0).contains(&lon) {
            Region::Africa
        } else if (-56.0..14.0).contains(&lat) && (-82.0..-34.0).contains(&lon) {
            Region::SouthAmerica
        } else if (5.0..75.0).contains(&lat) && (-170.0..-50.0).contains(&lon) {
            Region::NorthAmerica
        } else {
            Region::Unknown
        }
    }

    pub fn from_grid(grid: &str) -> Region {
        maidenhead_to_latlon(grid)
            .map(|(lat, lon)| Region::from_latlon(lat, lon))
            .unwrap_or(Region::Unknown)
    }
}

/// A two-ended reception report (PSK Reporter style): `tx` was heard by `rx`.
/// The detector consumes the simpler [`crate::Spot`] (the far end); the advisor
/// and dxped tracker use this so they can tell "who hears me" from "who I hear".
#[derive(Debug, Clone)]
pub struct PathSpot {
    pub time: i64,
    pub tx_call: String,
    pub tx_grid: Option<String>,
    pub rx_call: String,
    pub rx_grid: Option<String>,
    pub band: Band,
    pub mode: Option<String>,
    pub snr: Option<f32>,
}

/// Which side of a path the operator is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Operator transmitted; the far end heard us ("who hears me").
    HeardMe,
    /// Operator received; we heard the far end ("who I hear").
    IHeard,
    /// Neither end is the operator.
    Neither,
}

impl PathSpot {
    /// Which side of this path the operator (`me`) is on.
    pub fn side(&self, me: &str) -> Side {
        let me = me.to_uppercase();
        if self.tx_call.to_uppercase() == me {
            Side::HeardMe
        } else if self.rx_call.to_uppercase() == me {
            Side::IHeard
        } else {
            Side::Neither
        }
    }

    /// The far-end callsign relative to the operator.
    pub fn far_call(&self, me: &str) -> Option<&str> {
        match self.side(me) {
            Side::HeardMe => Some(&self.rx_call),
            Side::IHeard => Some(&self.tx_call),
            Side::Neither => None,
        }
    }

    /// The far-end grid relative to the operator.
    pub fn far_grid(&self, me: &str) -> Option<&str> {
        match self.side(me) {
            Side::HeardMe => self.rx_grid.as_deref(),
            Side::IHeard => self.tx_grid.as_deref(),
            Side::Neither => None,
        }
    }
}

/// Current space-weather snapshot (from NOAA SWPC).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SpaceWx {
    /// Solar flux index (10.7 cm).
    pub sfi: f32,
    /// Planetary K-index (0–9).
    pub kp: f32,
    /// Planetary A-index.
    pub a_index: f32,
    /// GOES long-band X-ray flux (W/m²); ≥ 1e-5 is an M-class flare.
    pub xray_long: f32,
}

impl Default for SpaceWx {
    fn default() -> Self {
        // Benign mid-cycle defaults.
        Self {
            sfi: 120.0,
            kp: 2.0,
            a_index: 8.0,
            xray_long: 1e-7,
        }
    }
}

impl SpaceWx {
    /// True if an M-class (or larger) flare is in progress (low-band fadeout risk).
    pub fn flare_in_progress(&self) -> bool {
        self.xray_long >= 1e-5
    }

    /// Flare class letter (A/B/C/M/X) for display.
    pub fn xray_class(&self) -> char {
        match self.xray_long {
            x if x >= 1e-4 => 'X',
            x if x >= 1e-5 => 'M',
            x if x >= 1e-6 => 'C',
            x if x >= 1e-7 => 'B',
            _ => 'A',
        }
    }
}

/// The propagation mode behind an opening (grounded in the research thresholds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropMode {
    SporadicE,
    F2,
    Aurora,
    MeteorScatter,
    Tropo,
    Unknown,
}

impl PropMode {
    pub fn label(self) -> &'static str {
        match self {
            PropMode::SporadicE => "Sporadic-E",
            PropMode::F2 => "F2",
            PropMode::Aurora => "Aurora",
            PropMode::MeteorScatter => "Meteor scatter",
            PropMode::Tropo => "Tropo",
            PropMode::Unknown => "Unknown",
        }
    }
}

/// Honest confidence word tied to observed evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confidence {
    Strong,
    Likely,
    Marginal,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Confidence::Strong => "Strong",
            Confidence::Likely => "Likely",
            Confidence::Marginal => "Marginal",
        }
    }

    /// From an observed unique-station count + whether the path is two-way.
    pub fn from_evidence(unique: usize, bidirectional: bool) -> Confidence {
        if unique >= 10 && bidirectional {
            Confidence::Strong
        } else if unique >= 3 {
            Confidence::Likely
        } else {
            Confidence::Marginal
        }
    }
}

/// Per-band activity tier for the band ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityTier {
    Active,
    Moderate,
    Quiet,
    Closed,
}

impl ActivityTier {
    pub fn label(self) -> &'static str {
        match self {
            ActivityTier::Active => "Active",
            ActivityTier::Moderate => "Moderate",
            ActivityTier::Quiet => "Quiet",
            ActivityTier::Closed => "Closed",
        }
    }

    pub fn from_score(score: f32) -> ActivityTier {
        if score >= 0.6 {
            ActivityTier::Active
        } else if score >= 0.25 {
            ActivityTier::Moderate
        } else if score > 0.03 {
            ActivityTier::Quiet
        } else {
            ActivityTier::Closed
        }
    }
}

/// Classify the propagation mode behind a VHF opening from geometry + space
/// weather (research thresholds): Es ≈ 500–2350 km single-hop & SFI-independent;
/// F2 > 4000 km & SFI ≥ 150; aurora Kp-gated & ≤ 1800 km.
pub fn classify_vhf_mode(median_km: f64, max_km: f64, wx: &SpaceWx) -> PropMode {
    if wx.kp >= 5.0 && max_km <= 1800.0 {
        PropMode::Aurora
    } else if max_km > 4000.0 && wx.sfi >= 150.0 {
        PropMode::F2
    } else if (480.0..=5000.0).contains(&median_km) {
        // 500–2350 single-hop, up to ~5000 multi-hop.
        PropMode::SporadicE
    } else if median_km < 480.0 && max_km < 2200.0 {
        PropMode::MeteorScatter
    } else {
        PropMode::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_from_mhz() {
        assert_eq!(Band::from_mhz(50.313), Some(Band::B6));
        assert_eq!(Band::from_mhz(14.074), Some(Band::B20));
        assert_eq!(Band::from_mhz(144.174), Some(Band::B2));
        assert_eq!(Band::from_mhz(5.0), None);
    }

    #[test]
    fn band_from_label_roundtrips() {
        for b in Band::ALL {
            assert_eq!(Band::from_label(b.label()), Some(b));
        }
        assert_eq!(Band::from_label("20M"), Some(Band::B20)); // case-insensitive
        assert_eq!(Band::from_label("70cm"), None);
    }

    #[test]
    fn mode_class_from_adif() {
        assert_eq!(ModeClass::from_adif("CW"), ModeClass::Cw);
        assert_eq!(ModeClass::from_adif("SSB"), ModeClass::Phone);
        assert_eq!(ModeClass::from_adif("usb"), ModeClass::Phone);
        assert_eq!(ModeClass::from_adif("FT8"), ModeClass::Digital);
        assert_eq!(ModeClass::from_adif("RTTY"), ModeClass::Digital);
        assert_eq!(ModeClass::from_adif(""), ModeClass::Digital);
    }

    #[test]
    fn region_binning() {
        assert_eq!(Region::from_grid("JN58"), Region::Europe); // Munich
        assert_eq!(Region::from_grid("EN52"), Region::NorthAmerica); // WI
        assert_eq!(Region::from_grid("PM95"), Region::Asia); // Japan-ish
    }

    #[test]
    fn vhf_classifier() {
        let calm = SpaceWx {
            sfi: 90.0,
            kp: 1.0,
            ..Default::default()
        };
        assert_eq!(
            classify_vhf_mode(1500.0, 2000.0, &calm),
            PropMode::SporadicE
        );
        let high = SpaceWx {
            sfi: 180.0,
            kp: 1.0,
            ..Default::default()
        };
        assert_eq!(classify_vhf_mode(5000.0, 6000.0, &high), PropMode::F2);
        let storm = SpaceWx {
            sfi: 100.0,
            kp: 6.0,
            ..Default::default()
        };
        assert_eq!(classify_vhf_mode(1200.0, 1500.0, &storm), PropMode::Aurora);
    }
}
