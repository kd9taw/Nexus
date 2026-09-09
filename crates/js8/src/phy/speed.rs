//! The JS8 submode table — one enum, every per-speed constant as a `const fn`.
//!
//! Transcribed FACTS from JS8Call commons.h:36-54 (NSPS / period / start delay),
//! JS8Submode.cpp:55-125 (derived framesNeeded / txDuration, Costas selection, rxThreshold,
//! rxSNRThreshold) and JS8.cpp:211-367 (JZ, NDOWNSPS). Ultra ("JS8 60", submode I) is
//! compiled out upstream (`JS8_ENABLE_JS8I 0`) and deliberately absent here.
//!
//! WHY a `const fn` table rather than a struct of constants: `ModeKind::Js8 { speed }` carries
//! this enum directly (interfaces.md §0 — no mirror enum in `modes`), settings store
//! `index()` (a stale settings.json degrades to Normal instead of failing to load), and the
//! DTO carries the lowercase serde form.

use crate::phy::costas::{COSTAS_MODIFIED, COSTAS_ORIGINAL};

/// WSJT-X-family audio sample rate the tables are stated at.
const SAMPLE_RATE: usize = 12_000;

/// The four on-air JS8 speeds. Display/index order is Slow, Normal, Fast, Turbo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speed {
    Slow,
    Normal,
    Fast,
    Turbo,
}

impl Speed {
    /// Display / index order. `index()` and `from_index()` map into this array.
    pub const ALL: [Speed; 4] = [Speed::Slow, Speed::Normal, Speed::Fast, Speed::Turbo];

    /// 0..=3 — the settings `js8_speed` value.
    pub const fn index(self) -> u8 {
        match self {
            Speed::Slow => 0,
            Speed::Normal => 1,
            Speed::Fast => 2,
            Speed::Turbo => 3,
        }
    }

    /// Inverse of [`Speed::index`]; `None` for anything else (the caller degrades to Normal).
    pub const fn from_index(i: u8) -> Option<Speed> {
        match i {
            0 => Some(Speed::Slow),
            1 => Some(Speed::Normal),
            2 => Some(Speed::Fast),
            3 => Some(Speed::Turbo),
            _ => None,
        }
    }

    /// Bit in the `js8_rx_speeds` mask: Slow 1, Normal 2, Fast 4, Turbo 8; all four = 15.
    pub const fn bit(self) -> u8 {
        1 << self.index()
    }

    /// JS8Call's submode letter as written in ALL.TXT and taken by `js8 -b`: E / A / B / C.
    pub const fn letter(self) -> char {
        match self {
            Speed::Slow => 'E',
            Speed::Normal => 'A',
            Speed::Fast => 'B',
            Speed::Turbo => 'C',
        }
    }

    /// Inverse of [`Speed::letter`]. 'I' (Ultra) is not a speed Nexus knows.
    pub fn from_letter(c: char) -> Option<Speed> {
        match c {
            'E' => Some(Speed::Slow),
            'A' => Some(Speed::Normal),
            'B' => Some(Speed::Fast),
            'C' => Some(Speed::Turbo),
            _ => None,
        }
    }

    /// Samples per symbol at 12 kHz (commons.h `JS8x_SYMBOL_SAMPLES`): 3840 / 1920 / 1200 / 600.
    pub const fn nsps(self) -> usize {
        match self {
            Speed::Slow => 3840,
            Speed::Normal => 1920,
            Speed::Fast => 1200,
            Speed::Turbo => 600,
        }
    }

    /// T/R period in seconds (commons.h `JS8x_TX_SECONDS`): 30 / 15 / 10 / 6. Slow is 30 on the
    /// air (JS8.cpp:305-317 — the Fortran's 28 was a bug the C++ port fixed).
    pub const fn period_s(self) -> u32 {
        match self {
            Speed::Slow => 30,
            Speed::Normal => 15,
            Speed::Fast => 10,
            Speed::Turbo => 6,
        }
    }

    /// TX start delay into the period (commons.h `JS8x_START_DELAY_MS`): 500 / 500 / 200 / 100 ms.
    pub const fn delay_ms(self) -> u32 {
        match self {
            Speed::Slow => 500,
            Speed::Normal => 500,
            Speed::Fast => 200,
            Speed::Turbo => 100,
        }
    }

    /// 12000 / nsps: 3.125 / 6.25 / 10 / 20 Hz (JS8Submode.cpp:70 `toneSpacing`).
    pub const fn tone_spacing_hz(self) -> f32 {
        SAMPLE_RATE as f32 / self.nsps() as f32
    }

    /// Costas kernel: `COSTAS_ORIGINAL` for Normal, `COSTAS_MODIFIED` otherwise
    /// (JS8Submode.cpp:121-125).
    pub const fn costas(self) -> &'static [[u8; 7]; 3] {
        match self {
            Speed::Normal => &COSTAS_ORIGINAL,
            Speed::Slow | Speed::Fast | Speed::Turbo => &COSTAS_MODIFIED,
        }
    }

    /// JS8Call's decode moment (JS8Submode.cpp:71 `framesNeeded`):
    /// floor(79·nsps + (0.5 + delay_s)·12000) samples from cycle start
    /// = 315 360 / 163 680 / 103 200 / 54 600.
    pub const fn frames_needed(self) -> usize {
        // (0.5 + delay_ms/1000) * 12000 == 6000 + delay_ms * 12 — exact in integers.
        79 * self.nsps() + 6_000 + self.delay_ms() as usize * 12
    }

    /// Length of one transmission incl. the start delay (JS8Submode.cpp:73 `txDuration`):
    /// 79·nsps/12000 + delay_s = 25.78 / 13.14 / 8.10 / 4.05 s — always < `period_s`.
    pub const fn slot_fit_s(self) -> f32 {
        (79 * self.nsps()) as f32 / SAMPLE_RATE as f32 + self.delay_ms() as f32 / 1000.0
    }

    /// Candidate-search lag half-width in quarter-symbol steps (JS8.cpp `JZ`): 32 / 62 / 144 / 172.
    pub const fn jz(self) -> usize {
        match self {
            Speed::Slow => 32,
            Speed::Normal => 62,
            Speed::Fast => 144,
            Speed::Turbo => 172,
        }
    }

    /// Downsampled samples per symbol (JS8.cpp `NDOWNSPS`): 32 / 32 / 20 / 12.
    pub const fn ndownsps(self) -> usize {
        match self {
            Speed::Slow => 32,
            Speed::Normal => 32,
            Speed::Fast => 20,
            Speed::Turbo => 12,
        }
    }

    /// Reassembly drift tolerance (JS8Submode.cpp `rxThreshold`): 10 / 10 / 16 / 32 Hz.
    pub const fn drift_hz(self) -> f32 {
        match self {
            Speed::Slow => 10.0,
            Speed::Normal => 10.0,
            Speed::Fast => 16.0,
            Speed::Turbo => 32.0,
        }
    }

    /// Published sensitivity floor (JS8Submode.cpp `rxSNRThreshold`): −28 / −24 / −22 / −20 dB.
    pub const fn snr_floor_db(self) -> i32 {
        match self {
            Speed::Slow => -28,
            Speed::Normal => -24,
            Speed::Fast => -22,
            Speed::Turbo => -20,
        }
    }

    /// Data frames use the 72-bit JSC "fast data" form with the i3 Data bit set — false only for
    /// Normal, which still emits the deprecated `[1][compressed][70]` form (varicode.cpp:1797-1921).
    pub const fn uses_fast_data(self) -> bool {
        !matches!(self, Speed::Normal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::costas::{COSTAS_MODIFIED, COSTAS_ORIGINAL};

    #[test]
    #[allow(clippy::type_complexity)] // one-off 9-tuple, table-driven test only
    fn table_matches_js8call_submode_constants() {
        // commons.h:36-54, JS8Submode.cpp:121-125, JS8.cpp:211-367 (ModeA/B/C/E).
        let rows: [(Speed, char, usize, u32, u32, usize, usize, f32, i32); 4] = [
            (Speed::Slow, 'E', 3840, 30, 500, 32, 32, 10.0, -28),
            (Speed::Normal, 'A', 1920, 15, 500, 62, 32, 10.0, -24),
            (Speed::Fast, 'B', 1200, 10, 200, 144, 20, 16.0, -22),
            (Speed::Turbo, 'C', 600, 6, 100, 172, 12, 32.0, -20),
        ];
        for (s, letter, nsps, period, delay, jz, ndownsps, drift, floor) in rows {
            assert_eq!(s.letter(), letter, "{s:?}");
            assert_eq!(s.nsps(), nsps, "{s:?}");
            assert_eq!(s.period_s(), period, "{s:?}");
            assert_eq!(s.delay_ms(), delay, "{s:?}");
            assert_eq!(s.jz(), jz, "{s:?}");
            assert_eq!(s.ndownsps(), ndownsps, "{s:?}");
            assert_eq!(s.drift_hz(), drift, "{s:?}");
            assert_eq!(s.snr_floor_db(), floor, "{s:?}");
            assert!(
                (s.tone_spacing_hz() - 12000.0 / nsps as f32).abs() < 1e-6,
                "{s:?}"
            );
        }
    }

    #[test]
    fn derived_constants_match_js8submode_cpp() {
        // JS8Submode.cpp:71-73: framesNeeded = floor(79*NSPS + (0.5 + delay)*12000);
        // txDuration = 79*NSPS/12000 + delay.
        assert_eq!(Speed::Slow.frames_needed(), 315_360);
        assert_eq!(Speed::Normal.frames_needed(), 163_680);
        assert_eq!(Speed::Fast.frames_needed(), 103_200);
        assert_eq!(Speed::Turbo.frames_needed(), 54_600);
        for (s, fit) in [
            (Speed::Slow, 25.78),
            (Speed::Normal, 13.14),
            (Speed::Fast, 8.10),
            (Speed::Turbo, 4.05),
        ] {
            assert!(
                (s.slot_fit_s() - fit).abs() < 1e-3,
                "{s:?}: {}",
                s.slot_fit_s()
            );
        }
    }

    #[test]
    fn every_transmission_fits_inside_its_period_with_margin() {
        // The slot-fit invariant (spec TX safety 6): tx_deadline_ms clamps PTT at the period
        // boundary, so an over-length wave would be truncated silently. Margin ≥ 1.8 s everywhere.
        for s in Speed::ALL {
            let margin = s.period_s() as f32 - s.slot_fit_s();
            assert!(margin >= 1.8, "{s:?}: margin {margin}");
            assert!(s.frames_needed() < s.period_s() as usize * 12_000, "{s:?}");
        }
    }

    #[test]
    fn costas_selection_is_original_for_normal_only() {
        // JS8Submode.cpp:121-125 — Normal = ORIGINAL, Fast/Turbo/Slow = MODIFIED.
        assert!(std::ptr::eq(Speed::Normal.costas(), &COSTAS_ORIGINAL));
        for s in [Speed::Slow, Speed::Fast, Speed::Turbo] {
            assert!(std::ptr::eq(s.costas(), &COSTAS_MODIFIED), "{s:?}");
        }
    }

    #[test]
    fn index_bit_and_letter_round_trip() {
        for (i, s) in Speed::ALL.iter().enumerate() {
            assert_eq!(s.index() as usize, i);
            assert_eq!(Speed::from_index(s.index()), Some(*s));
            assert_eq!(s.bit(), 1 << i);
            assert_eq!(Speed::from_letter(s.letter()), Some(*s));
        }
        assert_eq!(Speed::from_index(4), None);
        assert_eq!(
            Speed::from_letter('I'),
            None,
            "Ultra is compiled out upstream (JS8_ENABLE_JS8I 0)"
        );
        assert_eq!(Speed::from_letter('D'), None);
        assert_eq!(
            Speed::ALL.iter().map(|s| s.bit()).sum::<u8>(),
            15,
            "js8_rx_speeds default = all four"
        );
    }

    #[test]
    fn only_normal_uses_the_deprecated_70_bit_data_form() {
        // varicode.cpp:1797-1921 / :2049-2056 — Normal still emits [1][compressed][70]; the rest
        // use the full-72-bit JSC form with the i3 Data bit set.
        assert!(!Speed::Normal.uses_fast_data());
        for s in [Speed::Slow, Speed::Fast, Speed::Turbo] {
            assert!(s.uses_fast_data(), "{s:?}");
        }
    }

    #[test]
    fn serde_wire_form_is_lowercase() {
        assert_eq!(serde_json::to_string(&Speed::Turbo).unwrap(), "\"turbo\"");
        assert_eq!(
            serde_json::from_str::<Speed>("\"slow\"").unwrap(),
            Speed::Slow
        );
        assert!(serde_json::from_str::<Speed>("\"ultra\"").is_err());
    }
}
