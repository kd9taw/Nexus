//! How a contest turns a log into a score — the three INDEPENDENT axes, as data.
//!
//! This replaces `fd_rules::ScoringModel`, a two-arm enum that fused two of the axes
//! and had no representation of the third at all:
//!
//! 1. **QSO points** — [`PointsRule`]. Today's `points_by_mode_class` table.
//! 2. **Multipliers** — [`MultiplierRule`]. A count of distinct values at a scope.
//!    Neither Field Day event has one, which is exactly why `ScoringModel` had no
//!    concept of one and why a third arm for CQ WW would have had to invent it while
//!    re-implementing the points path inside itself.
//! 3. **Post-multipliers** — [`PostMultiplier`]. FD's power tier, WFD's objectives,
//!    the claimed bonus menu.
//!
//! ⚠️ **This module is a REFACTOR of shipped scoring, not a change to it.** ARRL Field
//! Day and Winter Field Day are shipped software with users;
//! `crates/tempo-core/tests/fd_goldens.rs` pins their Cabrillo bytes, their ADIF bytes
//! and their six score numbers against output captured before any of this existed. If a
//! golden moves, this file is what is wrong.
//!
//! ⚠️ **[`MultiplierRule`] has no evaluator here and that is deliberate.** No shipped
//! ruleset declares one (the seed writes `"multipliers": []` for both events, and
//! `fd_rules`'s tests pin that), and counting distinct received values needs the
//! per-row field vectors that arrive with the generalised contest log. A multiplier
//! evaluator written now would be untested code on the scoring path of a live contest.
//! What lands here is the TYPE: it deserialises from a rules file, the loader validates
//! it against the exchange (§2.5), and a ruleset that declares none still loads.

/// Per-mode-class QSO points (the `points_by_mode_class` table in the rules data).
/// The seed matches the historical hardcoded map (phone 1, CW/digital 2); a data edit
/// here provably changes computed scores (the install integration test's whole point).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModePoints {
    pub ph: u32,
    pub cw: u32,
    pub dig: u32,
}

impl ModePoints {
    /// Points for one logged mode class — the same normalization as the legacy
    /// [`qso_points_for_mode`](crate::fieldday::qso_points_for_mode) (which keeps
    /// serving the per-QSO interop push with the historical constants).
    pub fn for_mode(&self, mode: &str) -> u32 {
        match mode.to_ascii_uppercase().as_str() {
            "PH" | "PHONE" | "SSB" | "FM" => self.ph,
            "CW" => self.cw,
            _ => self.dig, // digital
        }
    }
}

/// One logged contact as the SCORER sees it — the seam that took scoring off
/// `FieldDayLog`.
///
/// `Scoring::qso_and_powered` used to be typed on `&FieldDayLog`, which is what welded
/// the scoring math to one contest's log type: a second contest could not be scored
/// without either owning a `FieldDayLog` or copying the math. It now takes an iterator
/// of these, and each log type presents its own rows as such a view
/// ([`FieldDayLog::score_rows`](crate::fieldday::FieldDayLog::score_rows)).
///
/// Borrowed, never owned: the score is recomputed on every snapshot tick, so scoring a
/// log must stay O(rows) with no allocation and no lookups.
///
/// ⚠️ It carries what the scorer READS, and grows as the rules that read it land — the
/// generalised contest log adds the band (which [`MultScope::PerBand`] and a band-group
/// points rule both need), the resolved DXCC entity and prefix, and the received field
/// vector. Adding a field here is additive for every caller; changing the signature
/// again would not be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreRow<'a> {
    /// `"PH"` | `"CW"` | `"DIG"` — the class scoring buckets by, never the on-air mode.
    pub mode_class: &'a str,
}

/// How a contact becomes points.
///
/// One arm today, and the enum is the point: ARRL FD and Winter FD both score by mode
/// class, and every other researched contest scores by something this arm cannot say
/// (a flat 2 for Sweepstakes, a band group for ARRL VHF, the relation between my
/// station and theirs for CQ WW and WPX). Those arms land with the contests that need
/// them and the rows those contests put on [`ScoreRow`]; landing them now would mean
/// shipping arms the scorer cannot evaluate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointsRule {
    /// Points from the per-mode-class table (ARRL FD, Winter FD).
    ByModeClass(ModePoints),
}

impl PointsRule {
    /// Points for one row.
    pub fn points_for(&self, row: &ScoreRow<'_>) -> u32 {
        match self {
            PointsRule::ByModeClass(p) => p.for_mode(row.mode_class),
        }
    }
}

/// Where a multiplier's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultSource {
    /// An exchange slot the worked station sent me. `domain` names which arm of a
    /// `OneOf` slot counts (`None` = the slot's whole value, whatever it matched) —
    /// this is how a QSO party counts counties and states as two separate universes
    /// out of one QTH slot.
    Field {
        key: &'static str,
        domain: Option<&'static str>,
    },
    /// The DXCC entity of the worked callsign (CQ WW's country multiplier).
    DxccEntity,
    /// The callsign prefix (CQ WPX).
    Prefix,
}

/// The scope a multiplier is counted at — a value counts once per what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultScope {
    /// Once for the whole log (Sweepstakes sections, CQP).
    PerLog,
    /// Once per band (CQ WW zones and countries, ARRL VHF grids, TNQP).
    PerBand,
    /// Once per mode (OhQP).
    PerMode,
    /// Once per band and mode.
    PerBandMode,
}

/// One multiplier universe: what counts, from where, at what scope, for whom.
///
/// ⚠️ **This has exactly one home — `Scoring::multipliers`.** An earlier design also
/// hung a list on the exchange's `RoleSpec`, which left two collections, no rule for
/// which won, and a validator that walked one of them. The per-role variation a QSO
/// party needs (an Ohio station counts states, provinces, counties and DX; an
/// out-of-state station counts only the 88 counties) is expressed by [`roles`] — a
/// filter on the one collection — rather than by a second collection.
///
/// [`roles`]: MultiplierRule::roles
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiplierRule {
    /// Stable id, unique within a ruleset (`"zone"`, `"country"`, `"section"`,
    /// `"county"`, `"prefix"`, `"grid"`). Names one board on the multiplier display and
    /// one column in a summary.
    pub id: &'static str,
    pub source: MultSource,
    pub scope: MultScope,
    /// Values that do NOT count (IARU: the HQ and official rows are not zone
    /// multipliers).
    pub excluding: &'static [&'static str],
    /// Which roles count this multiplier. **Empty = every role.**
    pub roles: &'static [&'static str],
}

/// What happens to the QSO-point total after the multipliers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostMultiplier {
    /// ARRL Field Day: multiply by a legal power tier. A stored multiplier is snapped
    /// to the highest tier at or below it, so a hand-edited settings file cannot score
    /// with a ×3 that is not a real tier.
    PowerTier { tiers: &'static [u32] },
    /// Winter Field Day: QSO points × (objectives + 1). `at_submission` records that
    /// the objective multipliers are applied by the sponsor at submission rather than
    /// on the air, which is why this arm applies NO on-air multiplier and the surfaces
    /// show a labelled provisional total instead.
    Objectives { at_submission: bool },
    /// The claimed bonus menu adds to the total.
    ///
    /// Bonuses are CLAIMED by the operator, not derived from the log, so this arm is a
    /// declaration rather than something [`Scoring::qso_and_powered`] can evaluate: the
    /// points come from [`FdRuleset::bonus_points`](crate::fd_rules::FdRuleset::bonus_points)
    /// over the claimed ids, exactly as they did before this refactor.
    Bonuses,
}

/// One contest's whole scoring model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scoring {
    pub qso_points: PointsRule,
    /// Empty for both Field Day events — neither has a multiplier. See the module note
    /// on why nothing evaluates this yet.
    pub multipliers: &'static [MultiplierRule],
    pub post: &'static [PostMultiplier],
}

impl Scoring {
    /// `(qso_pts, powered_pts)` for these rows at the given stored power tier.
    ///
    /// Byte-for-byte the old `ScoringModel::qso_and_powered`: sum the per-row points,
    /// then apply the power tier if this event has one. [`PostMultiplier::Objectives`]
    /// and [`PostMultiplier::Bonuses`] apply no on-air multiplier, so an event carrying
    /// only those returns `powered == qso_pts` — which is what the WFD arm returned.
    pub fn qso_and_powered<'a, I>(&self, rows: I, power_mult: u32) -> (u32, u32)
    where
        I: IntoIterator<Item = ScoreRow<'a>>,
    {
        let qso_pts: u32 = rows
            .into_iter()
            .map(|r| self.qso_points.points_for(&r))
            .sum();
        let powered = match self.power_tiers() {
            Some(tiers) => qso_pts * legal_power(tiers, power_mult),
            None => qso_pts,
        };
        (qso_pts, powered)
    }

    /// This event's legal power tiers, or `None` when it applies no power multiplier —
    /// the successor to matching on `ScoringModel::PoweredMultiplier`, and what the
    /// scoreboard reads to decide whether a payload carries power fields at all.
    pub fn power_tiers(&self) -> Option<&'static [u32]> {
        self.post.iter().find_map(|p| match p {
            PostMultiplier::PowerTier { tiers } => Some(*tiers),
            _ => None,
        })
    }
}

/// Snap a stored power multiplier to the highest legal tier ≤ `v` (or the smallest
/// tier). Replaces the engine's old `legal_fd_power` for the ARRL `{1, 2, 5}` tiers — a
/// hand-edited settings file must never score with a ×3/×4 that isn't a real tier.
fn legal_power(tiers: &[u32], v: u32) -> u32 {
    tiers
        .iter()
        .rev()
        .copied()
        .find(|&t| v >= t)
        .unwrap_or_else(|| tiers.first().copied().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FD_POINTS: ModePoints = ModePoints {
        ph: 1,
        cw: 2,
        dig: 2,
    };

    fn rows<'a>(classes: &'a [&'a str]) -> impl Iterator<Item = ScoreRow<'a>> {
        classes.iter().map(|m| ScoreRow { mode_class: m })
    }

    /// The ARRL tiers, and every value between and around them. Moved here with
    /// `legal_power` itself, unchanged: it matches the engine's old
    /// `legal_fd_power` (≥5→5, ≥2→2, else 1), and a hand-edited `fd_power_mult`
    /// must never score with a ×3.
    #[test]
    fn legal_power_snaps_to_arrl_tiers() {
        let tiers = &[1, 2, 5];
        for (v, want) in [
            (0, 1),
            (1, 1),
            (2, 2),
            (3, 2),
            (4, 2),
            (5, 5),
            (6, 5),
            (150, 5),
        ] {
            assert_eq!(legal_power(tiers, v), want, "power {v}");
        }
    }

    /// The ARRL Field Day model: mode-class points × a legal power tier.
    #[test]
    fn the_power_tier_arm_multiplies_the_mode_class_points() {
        static POST: &[PostMultiplier] = &[
            PostMultiplier::PowerTier { tiers: &[1, 2, 5] },
            PostMultiplier::Bonuses,
        ];
        let s = Scoring {
            qso_points: PointsRule::ByModeClass(FD_POINTS),
            multipliers: &[],
            post: POST,
        };
        // 2 CW + 3 phone + 2 digital = 4 + 3 + 4 = 11, the goldens' number.
        let log = ["CW", "CW", "PH", "PH", "PH", "DIG", "DIG"];
        assert_eq!(s.qso_and_powered(rows(&log), 5), (11, 55));
        assert_eq!(s.qso_and_powered(rows(&log), 2), (11, 22));
        // …and an illegal stored tier is snapped, not honoured.
        assert_eq!(s.qso_and_powered(rows(&log), 4), (11, 22));
    }

    /// The Winter Field Day model: the same points, NO on-air power multiplier. The
    /// pair is the point — a `qso_and_powered` that ignored `post` entirely would pass
    /// this test alone and fail the one above.
    #[test]
    fn the_objectives_arm_applies_no_on_air_power_multiplier() {
        static POST: &[PostMultiplier] = &[
            PostMultiplier::Objectives {
                at_submission: true,
            },
            PostMultiplier::Bonuses,
        ];
        let s = Scoring {
            qso_points: PointsRule::ByModeClass(FD_POINTS),
            multipliers: &[],
            post: POST,
        };
        let log = ["CW", "CW", "PH", "PH", "PH", "DIG", "DIG"];
        assert_eq!(s.qso_and_powered(rows(&log), 5), (11, 11));
        assert_eq!(s.power_tiers(), None);
    }

    /// The mode-class normalisation the interop paths depend on: `SSB`/`FM`/`PHONE` all
    /// score as phone, and an unrecognised name scores as digital rather than zero.
    #[test]
    fn mode_class_names_normalise_the_way_they_always_have() {
        for (m, want) in [
            ("PH", 1),
            ("ph", 1),
            ("PHONE", 1),
            ("SSB", 1),
            ("FM", 1),
            ("CW", 2),
            ("cw", 2),
            ("DIG", 2),
            ("RTTY", 2),
            ("", 2),
        ] {
            assert_eq!(FD_POINTS.for_mode(m), want, "mode {m:?}");
        }
    }

    /// An empty log scores zero rather than the power tier — `0 × 5` is still 0, but a
    /// scorer that seeded the total at 1 would not be.
    #[test]
    fn an_empty_log_scores_zero_on_both_models() {
        static POST: &[PostMultiplier] = &[PostMultiplier::PowerTier { tiers: &[1, 2, 5] }];
        let s = Scoring {
            qso_points: PointsRule::ByModeClass(FD_POINTS),
            multipliers: &[],
            post: POST,
        };
        assert_eq!(s.qso_and_powered(rows(&[]), 5), (0, 0));
    }
}
