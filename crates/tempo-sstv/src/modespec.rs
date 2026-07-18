//! SSTV mode specifications.
//!
//! Translated from slowrx's `modespec.c` (Oona Räisänen, ISC License).
//! See `NOTICE.md` for full attribution.
//!
//! Implemented as of V2.4 (0.5.0): PD120, PD180, PD240, Robot 24,
//! Robot 36, Robot 72, Scottie 1, Scottie 2, Scottie DX, Martin 1,
//! Martin 2. All RGB-sequential modes (Scottie + Martin) share a
//! single decode path; the per-line offsets branch on
//! [`SyncPosition`].

/// SSTV operating mode. Implemented: [`SstvMode::Pd120`], [`SstvMode::Pd180`],
/// [`SstvMode::Pd240`], [`SstvMode::Robot24`], [`SstvMode::Robot36`],
/// [`SstvMode::Robot72`], [`SstvMode::Scottie1`], [`SstvMode::Scottie2`],
/// [`SstvMode::ScottieDx`], [`SstvMode::Martin1`], [`SstvMode::Martin2`].
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SstvMode {
    /// PD-50. VIS `0x5D`. Nexus addition (slowrx `modespec.c` PD50 entry).
    Pd50,
    /// PD-90. VIS `0x63`. Nexus addition.
    Pd90,
    /// PD-120. VIS `0x5F`. See [`for_mode`] for full timing.
    Pd120,
    /// PD-160. VIS `0x62`. Nexus addition.
    Pd160,
    /// PD-180. VIS `0x60`.
    Pd180,
    /// PD-240. VIS `0x61`.
    Pd240,
    /// PD-290. VIS `0x5E`. Nexus addition.
    Pd290,
    /// Robot 24 (conventional name — decode buffer is ~36 s). VIS `0x04`.
    Robot24,
    /// Robot 36. VIS `0x08`.
    Robot36,
    /// Robot 72. VIS `0x0C`.
    Robot72,
    /// Scottie 1. VIS `0x3C`.
    Scottie1,
    /// Scottie 2. VIS `0x38`.
    Scottie2,
    /// Scottie DX. VIS `0x4C`.
    ScottieDx,
    /// Martin 1. VIS `0x2C`.
    Martin1,
    /// Martin 2. VIS `0x28`.
    Martin2,
}

/// Mode timing + layout table entry.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct ModeSpec {
    /// The mode this entry describes.
    pub mode: SstvMode,
    /// CLI/filename slug. Stable across releases (filenames like
    /// `img-NNN-{short_name}.png` depend on this). lowercase, no
    /// separators: "pd120", "robot24", "scottiedx", "scottie1",
    /// "martin1", etc. (audit #91 B13)
    pub short_name: &'static str,
    /// Human-readable mode name. For log lines and any future
    /// user-facing display. "PD-120", "Robot 24", "Scottie DX", etc.
    /// (audit #91 B13)
    pub name: &'static str,
    /// 7-bit VIS code identifying this mode on the wire.
    pub vis_code: u8,
    /// Visible image width in pixels.
    pub line_pixels: u32,
    /// Total visible scan lines per image.
    pub image_lines: u32,
    /// Total per-line duration including sync + porches, seconds.
    pub line_seconds: f64,
    /// Sync pulse duration, seconds.
    pub sync_seconds: f64,
    /// Porch (post-sync settling) duration, seconds.
    pub porch_seconds: f64,
    /// Per-pixel duration within a colour channel, seconds.
    pub pixel_seconds: f64,
    /// Channel separator pulse duration, seconds. Translated from slowrx's
    /// `SeptrTime` field (`modespec.c`). Zero for all PD-family modes; non-zero
    /// for Robot, Martin, and Scottie modes (V2). Stored here so the
    /// `chan_starts_sec` formula in `mode_pd::decode_pd_line_pair` matches
    /// slowrx's `video.c:88-92` term-for-term and won't silently break when
    /// non-PD modes are added.
    pub septr_seconds: f64,
    /// Channel layout used by per-mode decoders.
    pub channel_layout: ChannelLayout,
    /// Where the sync pulse sits within a radio line. See [`SyncPosition`]
    /// for the rationale (V2 carve-out forcing mid-line sync to be
    /// explicit when V2.3 Scottie lands).
    pub sync_position: SyncPosition,
}

/// Per-mode channel arrangement. PD-family modes use
/// [`ChannelLayout::PdYcbcr`]; Robot family uses [`ChannelLayout::RobotYuv`].
/// Future V2 mode families (Scottie, Martin) add their own values.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelLayout {
    /// PD-family: Y(odd) → Cr → Cb → Y(even). One radio line carries
    /// two image rows; chroma is shared between paired rows.
    PdYcbcr,
    /// Robot family: Y (single luma channel) plus chroma. R36/R24 carry
    /// alternating Cr/Cb per radio line with each chroma sample
    /// duplicated to the next image row; R72 carries Y/U/V sequentially
    /// per line. The shape difference is mode-internal — see
    /// `mode_robot::decode_line` for the per-mode dispatch.
    RobotYuv,
    /// Sequential single-line RGB layout — three channels per radio
    /// line. Used by Scottie (G→B→R, sync mid-line) and Martin (G→B→R,
    /// sync at line start).
    RgbSequential,
}

/// Where the sync pulse sits within a radio line.
///
/// PD/Robot/Martin all place sync at line start (the standard SSTV
/// convention). Scottie modes are the exception — sync sits between B
/// and R channels, not at line start. Stored here so future mode
/// decoders are forced to make their sync placement explicit at dispatch
/// time, surfacing the V1 line-clock-advance assumption that sync ==
/// line start.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyncPosition {
    /// Sync pulse at the start of each radio line. PD, Robot, Martin.
    /// Scottie family uses [`SyncPosition::Scottie`] instead.
    LineStart,
    /// Sync pulse between B and R within each radio line. Scottie family.
    Scottie,
}

impl ModeSpec {
    /// Offset (seconds) applied to the raw `xmax`-derived skip to
    /// land on line 0's content start. `LineStart` modes return 0;
    /// `Scottie` modes return `-chan_len/2 + 2 × porch_seconds` (the
    /// Scottie sync is mid-line, so the slip-wrapped `xmax` needs
    /// to be hoisted back left to align with line 0's content
    /// start). Audit #88 B4.
    #[must_use]
    pub(crate) fn skip_correction_seconds(&self) -> f64 {
        match self.sync_position {
            SyncPosition::LineStart => 0.0,
            SyncPosition::Scottie => {
                let chan_len = f64::from(self.line_pixels) * self.pixel_seconds;
                -chan_len / 2.0 + 2.0 * self.porch_seconds
            }
        }
    }
}

/// Look up the [`ModeSpec`] for a given 7-bit VIS code. Returns `None`
/// if the code is reserved, undefined, or maps to a mode not yet
/// implemented in this release. Derived from `ALL_SPECS`.
///
/// VIS codes are taken from Dave Jones (KB4YZ), 1998: "List of SSTV
/// Modes with VIS Codes".
///
/// **Parity-audit note (#27):** `0x00` is intentionally unmapped and
/// returns `None`. In slowrx (`vis.c:172-174`), an unknown VIS code causes
/// `GetVIS()` to return 0 and `Listen()` loops back to re-detect
/// (`do { ... } while (Mode == 0)`). Rust's equivalent is `None` from
/// this function: the caller in `SstvDecoder::process` emits
/// `SstvEvent::UnknownVis`, reseeds the VIS detector on the post-stop-bit
/// residue, and stays in `AwaitingVis` — the same "try again" effect as
/// slowrx's re-detect loop (slowrx's `printf("Unknown VIS")` becomes the
/// `UnknownVis` event). An unknown code is never an `Error`.
#[must_use]
pub fn lookup(vis_code: u8) -> Option<ModeSpec> {
    ALL_SPECS.iter().find(|s| s.vis_code == vis_code).copied()
}

/// Look up the [`ModeSpec`] for an [`SstvMode`].
///
/// Total over [`SstvMode`] — every implemented variant has a `const`
/// entry. Adding a new variant without adding its `const ModeSpec`
/// (and an arm here) is a compile error, by design. Pair with
/// [`lookup`] when starting from a VIS code on the wire.
#[must_use]
pub fn for_mode(mode: SstvMode) -> ModeSpec {
    match mode {
        SstvMode::Pd50 => PD50,
        SstvMode::Pd90 => PD90,
        SstvMode::Pd120 => PD120,
        SstvMode::Pd160 => PD160,
        SstvMode::Pd180 => PD180,
        SstvMode::Pd240 => PD240,
        SstvMode::Pd290 => PD290,
        SstvMode::Robot24 => ROBOT24,
        SstvMode::Robot36 => ROBOT36,
        SstvMode::Robot72 => ROBOT72,
        SstvMode::Scottie1 => SCOTTIE1,
        SstvMode::Scottie2 => SCOTTIE2,
        SstvMode::ScottieDx => SCOTTIE_DX,
        SstvMode::Martin1 => MARTIN1,
        SstvMode::Martin2 => MARTIN2,
    }
}

// Mode timing constants — translated row-for-row from slowrx's
// modespec.c (PD120 lines 260-271, PD180 lines 286-297, PD240 lines 299-310,
// R72 lines 130-141, R36 lines 143-154, R24 lines 156-167).

// PD50/PD90/PD160/PD290 are Nexus additions over the vendored slowrx.rs
// v0.5.3 baseline — timings row-for-row from slowrx C modespec.c (N7CXI,
// 2000): PD50 lines 234-245, PD90 lines 247-258, PD160 lines 273-284,
// PD290 lines 312-323; VIS codes from the same file's VISmap (0x5D=PD50,
// 0x5E=PD290, 0x62=PD160, 0x63=PD90).

const PD50: ModeSpec = ModeSpec {
    mode: SstvMode::Pd50,
    short_name: "pd50",
    name: "PD-50",
    vis_code: 0x5D,
    line_pixels: 320,
    image_lines: 256,
    // slowrx modespec.c:234-245 — PD50 LineTime = 388.16e-3,
    // PixelTime = 0.286e-3, SyncTime = 20e-3, PorchTime = 2.08e-3.
    line_seconds: 0.388_16,
    sync_seconds: 0.020,
    porch_seconds: 0.002_08,
    pixel_seconds: 0.000_286,
    septr_seconds: 0.0, // modespec.c: SeptrTime = 0e-3 for PD-family
    channel_layout: ChannelLayout::PdYcbcr,
    sync_position: SyncPosition::LineStart,
};

const PD90: ModeSpec = ModeSpec {
    mode: SstvMode::Pd90,
    short_name: "pd90",
    name: "PD-90",
    vis_code: 0x63,
    line_pixels: 320,
    image_lines: 256,
    // slowrx modespec.c:247-258 — PD90 LineTime = 703.04e-3,
    // PixelTime = 0.532e-3, SyncTime = 20e-3, PorchTime = 2.08e-3.
    line_seconds: 0.703_04,
    sync_seconds: 0.020,
    porch_seconds: 0.002_08,
    pixel_seconds: 0.000_532,
    septr_seconds: 0.0, // modespec.c: SeptrTime = 0e-3 for PD-family
    channel_layout: ChannelLayout::PdYcbcr,
    sync_position: SyncPosition::LineStart,
};

const PD120: ModeSpec = ModeSpec {
    mode: SstvMode::Pd120,
    short_name: "pd120",
    name: "PD-120",
    vis_code: 0x5F,
    line_pixels: 640,
    image_lines: 496,
    line_seconds: 0.508_48,
    sync_seconds: 0.020,
    porch_seconds: 0.002_08,
    pixel_seconds: 0.000_19,
    septr_seconds: 0.0, // modespec.c: SeptrTime = 0e-3 for PD-family
    channel_layout: ChannelLayout::PdYcbcr,
    sync_position: SyncPosition::LineStart,
};

const PD160: ModeSpec = ModeSpec {
    mode: SstvMode::Pd160,
    short_name: "pd160",
    name: "PD-160",
    vis_code: 0x62,
    line_pixels: 512,
    image_lines: 400,
    // slowrx modespec.c:273-284 — PD160 LineTime = 804.416e-3,
    // PixelTime = 0.382e-3, SyncTime = 20e-3, PorchTime = 2.08e-3.
    line_seconds: 0.804_416,
    sync_seconds: 0.020,
    porch_seconds: 0.002_08,
    pixel_seconds: 0.000_382,
    septr_seconds: 0.0, // modespec.c: SeptrTime = 0e-3 for PD-family
    channel_layout: ChannelLayout::PdYcbcr,
    sync_position: SyncPosition::LineStart,
};

const PD180: ModeSpec = ModeSpec {
    mode: SstvMode::Pd180,
    short_name: "pd180",
    name: "PD-180",
    vis_code: 0x60,
    line_pixels: 640,
    image_lines: 496,
    line_seconds: 0.754_24,
    sync_seconds: 0.020,
    porch_seconds: 0.002_08,
    pixel_seconds: 0.000_286,
    septr_seconds: 0.0, // modespec.c: SeptrTime = 0e-3 for PD-family
    channel_layout: ChannelLayout::PdYcbcr,
    sync_position: SyncPosition::LineStart,
};

const PD240: ModeSpec = ModeSpec {
    mode: SstvMode::Pd240,
    short_name: "pd240",
    name: "PD-240",
    vis_code: 0x61,
    line_pixels: 640,
    image_lines: 496,
    // slowrx modespec.c:299-310 — PD240 LineTime = 1000e-3,
    // PixelTime = 0.382e-3, SyncTime = 20e-3, PorchTime = 2.08e-3.
    line_seconds: 1.000,
    sync_seconds: 0.020,
    porch_seconds: 0.002_08,
    pixel_seconds: 0.000_382,
    septr_seconds: 0.0, // modespec.c: SeptrTime = 0e-3 for PD-family
    channel_layout: ChannelLayout::PdYcbcr,
    sync_position: SyncPosition::LineStart,
};

const PD290: ModeSpec = ModeSpec {
    mode: SstvMode::Pd290,
    short_name: "pd290",
    name: "PD-290",
    vis_code: 0x5E,
    line_pixels: 800,
    image_lines: 616,
    // slowrx modespec.c:312-323 — PD290 LineTime = 937.28e-3,
    // PixelTime = 0.286e-3, SyncTime = 20e-3, PorchTime = 2.08e-3.
    line_seconds: 0.937_28,
    sync_seconds: 0.020,
    porch_seconds: 0.002_08,
    pixel_seconds: 0.000_286,
    septr_seconds: 0.0, // modespec.c: SeptrTime = 0e-3 for PD-family
    channel_layout: ChannelLayout::PdYcbcr,
    sync_position: SyncPosition::LineStart,
};

const ROBOT24: ModeSpec = ModeSpec {
    mode: SstvMode::Robot24,
    short_name: "robot24",
    name: "Robot 24",
    vis_code: 0x04,
    line_pixels: 320,
    image_lines: 240,
    // slowrx modespec.c:156-167 — R24 LineTime = 150e-3,
    // PixelTime = 0.1375e-3, SyncTime = 9e-3, PorchTime = 3e-3,
    // SeptrTime = 6e-3.
    line_seconds: 0.150,
    sync_seconds: 0.009,
    porch_seconds: 0.003,
    pixel_seconds: 0.000_137_5,
    septr_seconds: 0.006,
    channel_layout: ChannelLayout::RobotYuv,
    sync_position: SyncPosition::LineStart,
};

const ROBOT36: ModeSpec = ModeSpec {
    mode: SstvMode::Robot36,
    short_name: "robot36",
    name: "Robot 36",
    vis_code: 0x08,
    line_pixels: 320,
    image_lines: 240,
    // slowrx modespec.c:143-154 — R36 LineTime = 150e-3,
    // PixelTime = 0.1375e-3, SyncTime = 9e-3, PorchTime = 3e-3,
    // SeptrTime = 6e-3.  Identical timing to R24.
    line_seconds: 0.150,
    sync_seconds: 0.009,
    porch_seconds: 0.003,
    pixel_seconds: 0.000_137_5,
    septr_seconds: 0.006,
    channel_layout: ChannelLayout::RobotYuv,
    sync_position: SyncPosition::LineStart,
};

const ROBOT72: ModeSpec = ModeSpec {
    mode: SstvMode::Robot72,
    short_name: "robot72",
    name: "Robot 72",
    vis_code: 0x0C,
    line_pixels: 320,
    image_lines: 240,
    // slowrx modespec.c:130-141 — R72 LineTime = 300e-3,
    // PixelTime = 0.2875e-3, SyncTime = 9e-3, PorchTime = 3e-3,
    // SeptrTime = 4.7e-3.
    line_seconds: 0.300,
    sync_seconds: 0.009,
    porch_seconds: 0.003,
    pixel_seconds: 0.000_287_5,
    septr_seconds: 0.0047,
    channel_layout: ChannelLayout::RobotYuv,
    sync_position: SyncPosition::LineStart,
};

const SCOTTIE1: ModeSpec = ModeSpec {
    mode: SstvMode::Scottie1,
    short_name: "scottie1",
    name: "Scottie 1",
    vis_code: 0x3C,
    line_pixels: 320,
    image_lines: 256,
    // slowrx modespec.c:91-104 — S1 LineTime = 428.38e-3,
    // PixelTime = 0.4320e-3, SyncTime = 9e-3, PorchTime = 1.5e-3,
    // SeptrTime = 1.5e-3.
    line_seconds: 0.428_38,
    sync_seconds: 0.009,
    porch_seconds: 0.001_5,
    pixel_seconds: 0.000_432_0,
    septr_seconds: 0.001_5,
    channel_layout: ChannelLayout::RgbSequential,
    sync_position: SyncPosition::Scottie,
};

const SCOTTIE2: ModeSpec = ModeSpec {
    mode: SstvMode::Scottie2,
    short_name: "scottie2",
    name: "Scottie 2",
    vis_code: 0x38,
    line_pixels: 320,
    image_lines: 256,
    // slowrx modespec.c:105-117 — S2 LineTime = 277.692e-3,
    // PixelTime = 0.2752e-3, SyncTime = 9e-3, PorchTime = 1.5e-3,
    // SeptrTime = 1.5e-3.
    line_seconds: 0.277_692,
    sync_seconds: 0.009,
    porch_seconds: 0.001_5,
    pixel_seconds: 0.000_275_2,
    septr_seconds: 0.001_5,
    channel_layout: ChannelLayout::RgbSequential,
    sync_position: SyncPosition::Scottie,
};

const SCOTTIE_DX: ModeSpec = ModeSpec {
    mode: SstvMode::ScottieDx,
    short_name: "scottiedx",
    name: "Scottie DX",
    vis_code: 0x4C,
    line_pixels: 320,
    image_lines: 256,
    // slowrx modespec.c:118-128 — SDX LineTime = 1050.3e-3,
    // PixelTime = 1.08053e-3, SyncTime = 9e-3, PorchTime = 1.5e-3,
    // SeptrTime = 1.5e-3.
    line_seconds: 1.050_3,
    sync_seconds: 0.009,
    porch_seconds: 0.001_5,
    pixel_seconds: 0.001_080_53,
    septr_seconds: 0.001_5,
    channel_layout: ChannelLayout::RgbSequential,
    sync_position: SyncPosition::Scottie,
};

/// Martin 1. slowrx `modespec.c:39-50`.
const MARTIN1: ModeSpec = ModeSpec {
    mode: SstvMode::Martin1,
    short_name: "martin1",
    name: "Martin 1",
    vis_code: 0x2C,
    line_pixels: 320,
    image_lines: 256,
    // slowrx modespec.c:39-50 — M1 LineTime = 446.446e-3,
    // PixelTime = 0.4576e-3, SyncTime = 4.862e-3,
    // PorchTime = 0.572e-3, SeptrTime = 0.572e-3.
    line_seconds: 0.446_446,
    sync_seconds: 0.004_862,
    porch_seconds: 0.000_572,
    pixel_seconds: 0.000_457_6,
    septr_seconds: 0.000_572,
    channel_layout: ChannelLayout::RgbSequential,
    sync_position: SyncPosition::LineStart,
};

/// Martin 2. slowrx `modespec.c:52-63`.
const MARTIN2: ModeSpec = ModeSpec {
    mode: SstvMode::Martin2,
    short_name: "martin2",
    name: "Martin 2",
    vis_code: 0x28,
    line_pixels: 320,
    image_lines: 256,
    // slowrx modespec.c:52-63 — M2 LineTime = 226.7986e-3,
    // PixelTime = 0.2288e-3, SyncTime = 4.862e-3,
    // PorchTime = 0.572e-3, SeptrTime = 0.572e-3.
    line_seconds: 0.226_798_6,
    sync_seconds: 0.004_862,
    porch_seconds: 0.000_572,
    pixel_seconds: 0.000_228_8,
    septr_seconds: 0.000_572,
    channel_layout: ChannelLayout::RgbSequential,
    sync_position: SyncPosition::LineStart,
};

/// All implemented mode specs. Single source of truth — [`lookup`] is
/// derived from this; [`for_mode`] keeps its exhaustive match so
/// adding a `SstvMode` variant without a `const ModeSpec` (and a
/// matching arm in `for_mode`) is a compile error, by design.
///
/// The F8 round-trip test (`all_specs_roundtrip`) verifies every
/// entry's `(mode, vis_code, short_name, name)` quadruple is unique
/// and that `lookup` and `for_mode` agree with the table.
pub(crate) const ALL_SPECS: [ModeSpec; 15] = [
    PD50, PD90, PD120, PD160, PD180, PD240, PD290, ROBOT24, ROBOT36, ROBOT72, SCOTTIE1, SCOTTIE2,
    SCOTTIE_DX, MARTIN1, MARTIN2,
];

#[cfg(test)]
#[allow(clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn pd120_vis_code_resolves() {
        let spec = lookup(0x5F).expect("PD120 VIS resolves");
        assert_eq!(spec.mode, SstvMode::Pd120);
        assert_eq!(spec.vis_code, 0x5F);
        assert_eq!(spec.line_pixels, 640);
        assert_eq!(spec.image_lines, 496);
        assert_eq!(spec.channel_layout, ChannelLayout::PdYcbcr);
        assert_eq!(spec.line_seconds, 0.508_48);
        assert_eq!(spec.sync_seconds, 0.020);
        assert_eq!(spec.porch_seconds, 0.002_08);
        assert_eq!(spec.pixel_seconds, 0.000_19);
    }

    #[test]
    fn pd180_vis_code_resolves() {
        let spec = lookup(0x60).expect("PD180 VIS resolves");
        assert_eq!(spec.mode, SstvMode::Pd180);
        assert_eq!(spec.pixel_seconds, 0.000_286);
    }

    #[test]
    fn unknown_vis_codes_return_none() {
        assert!(lookup(0x00).is_none());
        assert!(lookup(0x42).is_none()); // reserved
        assert!(lookup(0xFF).is_none());
    }

    #[test]
    fn for_mode_returns_matching_spec() {
        assert_eq!(for_mode(SstvMode::Pd120).vis_code, 0x5F);
        assert_eq!(for_mode(SstvMode::Pd180).vis_code, 0x60);
    }

    #[test]
    fn pd_modes_have_zero_septr_seconds() {
        // PD-family: SeptrTime = 0e-3 (modespec.c). The field exists for
        // V2 parity (Robot/Scottie/Martin have non-zero SeptrTime); for PD
        // modes it must be zero so chan_starts_sec is numerically unchanged.
        let pd120 = lookup(0x5F).expect("PD120");
        let pd180 = lookup(0x60).expect("PD180");
        let pd240 = lookup(0x61).expect("PD240");
        assert_eq!(pd120.septr_seconds, 0.0);
        assert_eq!(pd180.septr_seconds, 0.0);
        assert_eq!(pd240.septr_seconds, 0.0);
    }

    #[test]
    fn all_v2_modes_have_line_start_sync_position() {
        // V2 carve-out: ModeSpec.sync_position lets V2.3 Scottie declare
        // mid-line sync without retrofitting V1. PD/Robot/Martin all use
        // line-start sync; Scottie is the V2.3 exception.
        for mode in [
            SstvMode::Pd50,
            SstvMode::Pd90,
            SstvMode::Pd120,
            SstvMode::Pd160,
            SstvMode::Pd180,
            SstvMode::Pd240,
            SstvMode::Pd290,
            SstvMode::Robot24,
            SstvMode::Robot36,
            SstvMode::Robot72,
        ] {
            let spec = for_mode(mode);
            assert_eq!(spec.sync_position, SyncPosition::LineStart);
        }
    }

    // Nexus-added PD variants (not in vendored slowrx.rs 0.5.3). Timings
    // asserted against slowrx C modespec.c (N7CXI, 2000).

    #[test]
    fn pd50_vis_code_resolves() {
        let spec = lookup(0x5D).expect("PD50 VIS resolves");
        assert_eq!(spec.mode, SstvMode::Pd50);
        assert_eq!(spec.line_pixels, 320);
        assert_eq!(spec.image_lines, 256);
        assert_eq!(spec.channel_layout, ChannelLayout::PdYcbcr);
        assert_eq!(spec.line_seconds, 0.388_16);
        assert_eq!(spec.sync_seconds, 0.020);
        assert_eq!(spec.porch_seconds, 0.002_08);
        assert_eq!(spec.pixel_seconds, 0.000_286);
        assert_eq!(spec.septr_seconds, 0.0);
    }

    #[test]
    fn pd90_vis_code_resolves() {
        let spec = lookup(0x63).expect("PD90 VIS resolves");
        assert_eq!(spec.mode, SstvMode::Pd90);
        assert_eq!(spec.line_pixels, 320);
        assert_eq!(spec.image_lines, 256);
        assert_eq!(spec.line_seconds, 0.703_04);
        assert_eq!(spec.pixel_seconds, 0.000_532);
    }

    #[test]
    fn pd160_vis_code_resolves() {
        let spec = lookup(0x62).expect("PD160 VIS resolves");
        assert_eq!(spec.mode, SstvMode::Pd160);
        assert_eq!(spec.line_pixels, 512);
        assert_eq!(spec.image_lines, 400);
        assert_eq!(spec.line_seconds, 0.804_416);
        assert_eq!(spec.pixel_seconds, 0.000_382);
    }

    #[test]
    fn pd290_vis_code_resolves() {
        let spec = lookup(0x5E).expect("PD290 VIS resolves");
        assert_eq!(spec.mode, SstvMode::Pd290);
        assert_eq!(spec.line_pixels, 800);
        assert_eq!(spec.image_lines, 616);
        assert_eq!(spec.line_seconds, 0.937_28);
        assert_eq!(spec.pixel_seconds, 0.000_286);
    }

    #[test]
    fn pd240_vis_code_resolves() {
        let spec = lookup(0x61).expect("PD240 VIS resolves");
        assert_eq!(spec.mode, SstvMode::Pd240);
        assert_eq!(spec.vis_code, 0x61);
        assert_eq!(spec.line_pixels, 640);
        assert_eq!(spec.image_lines, 496);
        assert_eq!(spec.channel_layout, ChannelLayout::PdYcbcr);
        assert_eq!(spec.sync_position, SyncPosition::LineStart);
        assert_eq!(spec.line_seconds, 1.000);
        assert_eq!(spec.sync_seconds, 0.020);
        assert_eq!(spec.porch_seconds, 0.002_08);
        assert_eq!(spec.pixel_seconds, 0.000_382);
        assert_eq!(spec.septr_seconds, 0.0);
    }

    #[test]
    fn for_mode_returns_pd240_spec() {
        assert_eq!(for_mode(SstvMode::Pd240).vis_code, 0x61);
    }

    #[test]
    fn robot24_vis_code_resolves() {
        let spec = lookup(0x04).expect("R24 VIS resolves");
        assert_eq!(spec.mode, SstvMode::Robot24);
        assert_eq!(spec.vis_code, 0x04);
        assert_eq!(spec.line_pixels, 320);
        assert_eq!(spec.image_lines, 240);
        assert_eq!(spec.channel_layout, ChannelLayout::RobotYuv);
        assert_eq!(spec.sync_position, SyncPosition::LineStart);
        assert_eq!(spec.line_seconds, 0.150);
        assert_eq!(spec.sync_seconds, 0.009);
        assert_eq!(spec.porch_seconds, 0.003);
        assert_eq!(spec.septr_seconds, 0.006);
        assert_eq!(spec.pixel_seconds, 0.000_137_5);
    }

    #[test]
    fn robot36_vis_code_resolves() {
        let spec = lookup(0x08).expect("R36 VIS resolves");
        assert_eq!(spec.mode, SstvMode::Robot36);
        assert_eq!(spec.vis_code, 0x08);
        assert_eq!(spec.line_pixels, 320);
        assert_eq!(spec.image_lines, 240);
        assert_eq!(spec.channel_layout, ChannelLayout::RobotYuv);
        assert_eq!(spec.sync_position, SyncPosition::LineStart);
        assert_eq!(spec.line_seconds, 0.150);
        assert_eq!(spec.sync_seconds, 0.009);
        assert_eq!(spec.porch_seconds, 0.003);
        assert_eq!(spec.septr_seconds, 0.006);
        assert_eq!(spec.pixel_seconds, 0.000_137_5);
    }

    #[test]
    fn robot72_vis_code_resolves() {
        let spec = lookup(0x0C).expect("R72 VIS resolves");
        assert_eq!(spec.mode, SstvMode::Robot72);
        assert_eq!(spec.vis_code, 0x0C);
        assert_eq!(spec.line_pixels, 320);
        assert_eq!(spec.image_lines, 240);
        assert_eq!(spec.channel_layout, ChannelLayout::RobotYuv);
        assert_eq!(spec.sync_position, SyncPosition::LineStart);
        assert_eq!(spec.line_seconds, 0.300);
        assert_eq!(spec.sync_seconds, 0.009);
        assert_eq!(spec.porch_seconds, 0.003);
        assert_eq!(spec.septr_seconds, 0.0047);
        assert_eq!(spec.pixel_seconds, 0.000_287_5);
    }

    #[test]
    fn for_mode_returns_robot_specs() {
        assert_eq!(for_mode(SstvMode::Robot24).vis_code, 0x04);
        assert_eq!(for_mode(SstvMode::Robot36).vis_code, 0x08);
        assert_eq!(for_mode(SstvMode::Robot72).vis_code, 0x0C);
    }

    #[test]
    fn scottie1_modespec() {
        let spec = for_mode(SstvMode::Scottie1);
        assert_eq!(spec.mode, SstvMode::Scottie1);
        assert_eq!(spec.vis_code, 0x3C);
        assert_eq!(spec.line_pixels, 320);
        assert_eq!(spec.image_lines, 256);
        assert_eq!(spec.channel_layout, ChannelLayout::RgbSequential);
        assert_eq!(spec.sync_position, SyncPosition::Scottie);
        assert!((spec.pixel_seconds - 0.4320e-3).abs() < 1e-9);
        assert!((spec.line_seconds - 428.38e-3).abs() < 1e-9);
    }

    #[test]
    fn scottie2_modespec() {
        let spec = for_mode(SstvMode::Scottie2);
        assert_eq!(spec.mode, SstvMode::Scottie2);
        assert_eq!(spec.vis_code, 0x38);
        assert!((spec.pixel_seconds - 0.2752e-3).abs() < 1e-9);
        assert!((spec.line_seconds - 277.692e-3).abs() < 1e-9);
        assert_eq!(spec.channel_layout, ChannelLayout::RgbSequential);
        assert_eq!(spec.sync_position, SyncPosition::Scottie);
    }

    #[test]
    fn scottie_dx_modespec() {
        let spec = for_mode(SstvMode::ScottieDx);
        assert_eq!(spec.mode, SstvMode::ScottieDx);
        assert_eq!(spec.vis_code, 0x4C);
        assert!((spec.pixel_seconds - 1.08053e-3).abs() < 1e-9);
        assert!((spec.line_seconds - 1050.3e-3).abs() < 1e-9);
        assert_eq!(spec.channel_layout, ChannelLayout::RgbSequential);
        assert_eq!(spec.sync_position, SyncPosition::Scottie);
    }

    #[test]
    fn scottie_vis_codes_resolve() {
        // Codebase uses `lookup` (returning `Option<ModeSpec>`) rather
        // than `for_vis_code`; mirrors the existing
        // `pd120_vis_code_resolves` style.
        assert_eq!(
            lookup(0x3C).expect("S1 VIS resolves").mode,
            SstvMode::Scottie1
        );
        assert_eq!(
            lookup(0x38).expect("S2 VIS resolves").mode,
            SstvMode::Scottie2
        );
        assert_eq!(
            lookup(0x4C).expect("SDX VIS resolves").mode,
            SstvMode::ScottieDx
        );
    }

    #[test]
    fn martin1_modespec() {
        let spec = for_mode(SstvMode::Martin1);
        assert_eq!(spec.mode, SstvMode::Martin1);
        assert_eq!(spec.vis_code, 0x2C);
        assert_eq!(spec.line_pixels, 320);
        assert_eq!(spec.image_lines, 256);
        assert_eq!(spec.channel_layout, ChannelLayout::RgbSequential);
        assert_eq!(spec.sync_position, SyncPosition::LineStart);
        assert!((spec.pixel_seconds - 0.000_457_6).abs() < 1e-9);
        assert!((spec.line_seconds - 0.446_446).abs() < 1e-9);
    }

    #[test]
    fn martin2_modespec() {
        let spec = for_mode(SstvMode::Martin2);
        assert_eq!(spec.mode, SstvMode::Martin2);
        assert_eq!(spec.vis_code, 0x28);
        assert!((spec.pixel_seconds - 0.000_228_8).abs() < 1e-9);
        assert!((spec.line_seconds - 0.226_798_6).abs() < 1e-9);
        assert_eq!(spec.channel_layout, ChannelLayout::RgbSequential);
        assert_eq!(spec.sync_position, SyncPosition::LineStart);
    }

    #[test]
    fn martin_vis_codes_resolve() {
        assert_eq!(lookup(0x2C).expect("M1").mode, SstvMode::Martin1);
        assert_eq!(lookup(0x28).expect("M2").mode, SstvMode::Martin2);
    }

    #[test]
    fn skip_correction_seconds_zero_for_line_start_modes() {
        for mode in [
            SstvMode::Pd120,
            SstvMode::Pd240,
            SstvMode::Pd180,
            SstvMode::Robot24,
            SstvMode::Robot36,
            SstvMode::Robot72,
            SstvMode::Martin1,
            SstvMode::Martin2,
        ] {
            let spec = for_mode(mode);
            assert_eq!(
                spec.skip_correction_seconds(),
                0.0,
                "{mode:?} expected 0.0 skip correction"
            );
        }
    }

    #[test]
    fn skip_correction_seconds_scottie_formula() {
        for mode in [SstvMode::Scottie1, SstvMode::Scottie2, SstvMode::ScottieDx] {
            let spec = for_mode(mode);
            let expected =
                -f64::from(spec.line_pixels) * spec.pixel_seconds / 2.0 + 2.0 * spec.porch_seconds;
            assert!(
                (spec.skip_correction_seconds() - expected).abs() < 1e-12,
                "{mode:?} got {} expected {expected}",
                spec.skip_correction_seconds()
            );
            assert!(
                spec.skip_correction_seconds() < 0.0,
                "{mode:?} Scottie correction should be negative"
            );
        }
    }

    /// F8 (#91). Every entry in `ALL_SPECS` round-trips cleanly
    /// through `lookup` (VIS code → spec) and `for_mode` (mode →
    /// spec); the table has unique modes, VIS codes and `short_names`;
    /// every `name` and `short_name` is non-empty.
    ///
    /// Subsumes the per-mode `vis_code_resolves` tests as a
    /// structural invariant. The individual per-mode tests stay as
    /// fast-failing regression guards with descriptive names.
    #[test]
    fn all_specs_roundtrip() {
        use std::collections::HashSet;

        let modes: HashSet<_> = ALL_SPECS.iter().map(|s| s.mode).collect();
        assert_eq!(
            modes.len(),
            ALL_SPECS.len(),
            "ALL_SPECS has duplicate modes"
        );

        let vis: HashSet<_> = ALL_SPECS.iter().map(|s| s.vis_code).collect();
        assert_eq!(
            vis.len(),
            ALL_SPECS.len(),
            "ALL_SPECS has duplicate VIS codes"
        );

        let short_names: HashSet<_> = ALL_SPECS.iter().map(|s| s.short_name).collect();
        assert_eq!(
            short_names.len(),
            ALL_SPECS.len(),
            "ALL_SPECS has duplicate short_names"
        );

        let names: HashSet<_> = ALL_SPECS.iter().map(|s| s.name).collect();
        assert_eq!(
            names.len(),
            ALL_SPECS.len(),
            "ALL_SPECS has duplicate `name`s"
        );

        for spec in ALL_SPECS.iter().copied() {
            assert_eq!(
                lookup(spec.vis_code),
                Some(spec),
                "lookup({:#04x}) did not return ALL_SPECS entry for {:?}",
                spec.vis_code,
                spec.mode
            );
            assert_eq!(
                for_mode(spec.mode),
                spec,
                "for_mode({:?}) did not match ALL_SPECS entry",
                spec.mode
            );
            assert!(
                !spec.short_name.is_empty(),
                "{:?}: short_name empty",
                spec.mode
            );
            assert!(!spec.name.is_empty(), "{:?}: name empty", spec.mode);
        }
    }
}
