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
//! ⚠️ **[`MultiplierRule`] gained its evaluator with the batch that made a ruleset
//! declaring one reachable**, and not before: while the only shipped rulesets were the
//! two Field Day events — both of which write `"multipliers": []`, which `fd_rules`'s
//! tests pin — an evaluator would have been untested code on the scoring path of a live
//! contest. The four state QSO parties are what a score without it gets wrong, by the
//! size of a multiplier total. [`Scoring::mult_counts`] counts, [`Scoring::score`]
//! applies, and an event that declares NO rule keeps exactly the total it had (the
//! count is not treated as a ×0).

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
    /// The band label (`"20m"`). Read by [`MultScope::PerBand`] and
    /// [`MultScope::PerBandMode`].
    pub band: &'a str,
    /// The [`RoleSpec::id`](super::RoleSpec::id) this contact was worked under — what
    /// [`MultiplierRule::roles`] filters on. An Ohio station counts four multiplier
    /// universes and a Michigan station counts one, out of the same log shape.
    pub role: &'a str,
    /// ⭐ **The exchange THEY sent me**, which is where a multiplier value comes from.
    /// Each value carries the domain arm that matched it, so `MultSource::Field`'s
    /// `domain` selects the county bucket or the state bucket out of one `QTH` slot
    /// without re-parsing (§2.4).
    pub rx: &'a [super::FieldValue],
    /// The resolved DXCC entity of the worked callsign, for
    /// [`MultSource::DxccEntity`]. `None` — which counts nothing — everywhere this
    /// build does not resolve one (`LoggedQso::entity`'s own note).
    pub entity: Option<&'a str>,
    /// The resolved callsign prefix, for [`MultSource::Prefix`] — CQ WPX's whole
    /// multiplier. `None` for a row whose call could not be read as one
    /// ([`wpx_prefix`](super::callsign::wpx_prefix)).
    pub prefix: Option<&'a str>,
    /// ⭐ **How this contact relates to MY station** — the axis [`PointsRule::ByRelation`]
    /// prices a contact on.
    ///
    /// It is computed where BOTH sides are known (the log knows my call; the row knows
    /// theirs) and carried here rather than recomputed by the scorer, which knows neither.
    /// `None` for every contest that does not price by it, and for a contact whose
    /// callsign the country file cannot place.
    pub relation: Option<super::callsign::Relation>,
}

/// ⭐ **One row of a [`PointsRule::ByRelation`] table**: what a contact of this relation
/// is worth, on these bands.
///
/// CQ WW's four arms are flat; CQ WPX prices the same four arms differently on the low
/// bands, which is why the band list is here rather than in a second rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationPoints {
    pub relation: super::callsign::Relation,
    /// Band labels (`"20m"`) this row applies to. **Empty = every band** — CQ WPX's
    /// *"1 point regardless of band"* arm, and every one of CQ WW's four.
    pub bands: &'static [&'static str],
    pub points: u32,
}

/// How a contact becomes points.
///
/// ARRL FD and Winter FD score by mode class; CQ WW and CQ WPX score by the relation
/// between my station and theirs. The remaining researched shapes (a flat 2 for
/// Sweepstakes, a band group for ARRL VHF) are expressible as one of these two — a flat
/// table is a mode-class table with three equal entries — and land as new arms only if a
/// contest arrives that neither can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointsRule {
    /// Points from the per-mode-class table (ARRL FD, Winter FD, Sweepstakes).
    ByModeClass(ModePoints),
    /// ⭐ Points from the relation between my station and the station worked, per band
    /// group (CQ WW, CQ WPX).
    ///
    /// The table is a LOOKUP, not an ordered rule list: exactly one
    /// [`Relation`](super::callsign::Relation) is true of a contact, so no ordering can
    /// change the answer and no row can shadow another.
    ByRelation(&'static [RelationPoints]),
}

impl PointsRule {
    /// Points for one row.
    ///
    /// ⚠️ **A `ByRelation` row with no relation scores zero**, and it can only be one of
    /// two things: a contest running with no country file (which
    /// [`ContestSession::for_ruleset`](super::ContestSession::for_ruleset) refuses before
    /// the first contact) or a callsign the country file cannot place. The alternative —
    /// picking an arm — would price a contact by a relation nobody established.
    pub fn points_for(&self, row: &ScoreRow<'_>) -> u32 {
        match self {
            PointsRule::ByModeClass(p) => p.for_mode(row.mode_class),
            PointsRule::ByRelation(table) => {
                let Some(rel) = row.relation else {
                    return 0;
                };
                relation_points(table, rel, row.band)
                    // The North American exception is an exception TO the plain
                    // same-continent arm, so a table that declares no NA row falls back
                    // to the arm it would have overridden rather than scoring zero.
                    .or_else(|| {
                        (rel == super::callsign::Relation::WithinNorthAmerica).then(|| {
                            relation_points(
                                table,
                                super::callsign::Relation::SameContinent,
                                row.band,
                            )
                        })?
                    })
                    .unwrap_or(0)
            }
        }
    }
}

/// One received value as a multiplier BUCKET KEY.
///
/// Trimmed and upper-cased, which is what a section or a county code needs — and then,
/// ⭐ **for an all-digit value, read as its number**, so a CQ zone copied as `05` and one
/// copied as `5` are one multiplier and not two.
///
/// ⚠️ This is the one normalisation applied anywhere to a copied exchange value, and it
/// is applied HERE rather than at copy time on purpose: `FieldSpec`'s own header forbids
/// rewriting what a station actually sent, and `05` is what the operator logged and what
/// the Cabrillo line must carry. What a bucket is keyed by is a different question from
/// what was copied. It touches nothing but digits, so no county or section code
/// (`SB`/`SBEN`, `BEE`, `4U`) is affected — the closest thing to a hazard would be a
/// domain whose codes are numeric and leading-zero-significant, and no researched
/// contest has one.
fn canonical_mult_value(raw: &str) -> String {
    let v = raw.trim().to_ascii_uppercase();
    if v.len() > 1 && v.bytes().all(|b| b.is_ascii_digit()) {
        let stripped = v.trim_start_matches('0');
        return if stripped.is_empty() {
            "0".to_string()
        } else {
            stripped.to_string()
        };
    }
    v
}

/// The points a table gives one relation on one band: a row naming the band wins over a
/// row naming every band, so a `bands: []` arm can be a floor under band-specific ones.
fn relation_points(
    table: &[RelationPoints],
    rel: super::callsign::Relation,
    band: &str,
) -> Option<u32> {
    let matches = |r: &&RelationPoints| r.relation == rel;
    table
        .iter()
        .find(|r| matches(r) && r.bands.iter().any(|b| b.eq_ignore_ascii_case(band)))
        .or_else(|| table.iter().find(|r| matches(r) && r.bands.is_empty()))
        .map(|r| r.points)
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

    /// ⭐ **The multiplier total: how many distinct values these rows count, summed
    /// over every universe this ruleset declares.**
    ///
    /// The QSO parties' whole score depends on it — CQP is *"the total number of QSO
    /// Points multiplied by the total number of scored multipliers"* — and until it
    /// existed the type shipped with no evaluator at all, which was correct while no
    /// shipped ruleset declared a multiplier and is a wrong score the moment one does.
    ///
    /// **Distinct at the rule's own SCOPE.** A value counts once per log (CQP, TXQP),
    /// once per band (TNQP), or once per mode (OhQP: *"working the same multiplier on
    /// both CW and SSB counts as two multipliers"*) — so the key counted is the value
    /// plus whatever the scope adds to it, and the scopes are summed across rules
    /// rather than intersected.
    ///
    /// **A row counts only for the rules its own ROLE counts** ([`MultiplierRule::roles`]),
    /// and a value in [`MultiplierRule::excluding`] counts for none.
    ///
    /// ⚠️ **A `Field` rule with a `domain` counts only values that MATCHED that
    /// domain** — not every value in the slot. That is the whole reason the matched arm
    /// travels on the value: an Ohio operator's `QTH` slot holds counties and states
    /// and provinces and `DX`, and the county board must count the counties. A value
    /// whose arm is `None` (out of every declared universe, or a free-text arm with no
    /// domain to name) counts for a rule that names no domain and for no other.
    pub fn mult_counts<'a, I>(&self, rows: I) -> Vec<(&'static str, usize)>
    where
        I: IntoIterator<Item = ScoreRow<'a>>,
    {
        use std::collections::HashSet;
        let mut seen: Vec<HashSet<(String, String)>> = vec![HashSet::new(); self.multipliers.len()];
        for row in rows {
            for (i, m) in self.multipliers.iter().enumerate() {
                if !(m.roles.is_empty() || m.roles.contains(&row.role)) {
                    continue;
                }
                let value = match m.source {
                    MultSource::Field { key, domain } => row
                        .rx
                        .iter()
                        .find(|v| v.key == key && (domain.is_none() || v.domain == domain))
                        .map(|v| canonical_mult_value(&v.raw)),
                    MultSource::DxccEntity => row.entity.map(|e| e.to_string()),
                    MultSource::Prefix => row.prefix.map(|p| p.to_string()),
                };
                let Some(value) = value.filter(|v| !v.is_empty()) else {
                    continue;
                };
                if m.excluding.iter().any(|e| *e == value) {
                    continue;
                }
                let bucket = match m.scope {
                    MultScope::PerLog => String::new(),
                    MultScope::PerBand => row.band.to_ascii_uppercase(),
                    MultScope::PerMode => row.mode_class.to_ascii_uppercase(),
                    MultScope::PerBandMode => {
                        format!("{}/{}", row.band.to_ascii_uppercase(), row.mode_class)
                    }
                };
                seen[i].insert((bucket, value));
            }
        }
        self.multipliers
            .iter()
            .zip(seen)
            .map(|(m, set)| (m.id, set.len()))
            .collect()
    }

    /// The claimed total: `(qso_points, powered, multipliers, total)`.
    ///
    /// ⚠️ **A ruleset with NO multiplier is not a ruleset with zero multipliers, and the
    /// count says which it is.** `None` = this contest has no multiplier concept (both
    /// Field Day events); `Some(0)` = it has one and none has been worked yet. A `0`
    /// standing for both would leave every consumer to guess, and multiplying a shipped
    /// Field Day total by it would zero the score. `bonuses` stay outside this: they are
    /// CLAIMED by the operator, not derived from the log, and
    /// `FdRuleset::bonus_points` is where they are added.
    pub fn score<'a, I>(&self, rows: I, power_mult: u32) -> (u32, u32, Option<u32>, u32)
    where
        I: IntoIterator<Item = ScoreRow<'a>> + Clone,
    {
        let (qso_pts, powered) = self.qso_and_powered(rows.clone(), power_mult);
        if self.multipliers.is_empty() {
            return (qso_pts, powered, None, powered);
        }
        let mults: u32 = self.mult_counts(rows).iter().map(|(_, n)| *n as u32).sum();
        (qso_pts, powered, Some(mults), powered * mults)
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

/// ⭐ **One block on the multiplier display** — §9's generalisation of the Field Day
/// worked-sections board.
///
/// The board is a DISPLAY object, not a scoring one: it names which slot's received
/// values to colour in and which domain supplies the universe of cells. The scorer is
/// untouched by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardSpec {
    /// Stable id — a [`MultiplierRule::id`] when a rule drives the board, else the
    /// slot id. One id, one block, and the id is what the UI keys its cells by.
    pub id: &'static str,
    /// The received slot whose values fill the board.
    pub slot: &'static str,
    /// The domain supplying the cell universe, when the slot has one. `None` for a
    /// board over a slot with no closed value set (a serial, a grid), which renders
    /// what has been worked and nothing else.
    pub domain: Option<&'static str>,
    /// Once per what — carried through so a per-band board can say so rather than
    /// being silently drawn as a per-log one.
    pub scope: MultScope,
}

/// ⭐ **The boards a session displays, in order** (§9).
///
/// > CQ WW shows two boards (zone, country) and a QSO party shows one.
///
/// **Two sources, and the fallback is not a fudge.** A ruleset that declares
/// [`MultiplierRule`]s gets one board per rule, filtered to the ones this role counts.
/// A ruleset that declares NONE — which is both Field Day events, neither of which has
/// a multiplier — gets one board per received [`FieldKind::Enum`] slot, because the
/// shipped worked-sections board is a WORKED-STATUS board over a closed value set and
/// that is exactly what it has always been. Deriving the FD board from the rules would
/// have made a UI batch delete a shipped display, which is the behaviour change §11
/// item 7 says this batch does not make.
///
/// A [`MultSource`] that is not a slot ([`MultSource::DxccEntity`],
/// [`MultSource::Prefix`]) yields no board here: its universe is the country file, not
/// a domain.
///
/// ⚠️ **CQ WW and CQ WPX have now shipped and this is unchanged, deliberately.** CQ WW
/// gets one board — its ZONE, off the `ZN` slot, with `domain: None` because a `Number`
/// slot has no closed value set to colour in — and its COUNTRY board and CQ WPX's PREFIX
/// board are both absent, because a board over 340 DXCC entities or over an unbounded
/// prefix universe is a display design, not a rules-file derivation, and inventing one
/// here would ship a surface nobody drew. Both multipliers are COUNTED
/// ([`Scoring::mult_counts`] names them `country` and `prefix`); what is missing is a
/// worked-status grid for them, which is a UI batch's to design.
pub fn boards(
    scoring: &Scoring,
    exchange: &super::ExchangeSpec,
    role: &super::RoleSpec,
) -> Vec<BoardSpec> {
    let declared: Vec<BoardSpec> = scoring
        .multipliers
        .iter()
        .filter(|m| m.roles.is_empty() || m.roles.contains(&role.id))
        .filter_map(|m| match m.source {
            MultSource::Field { key, domain } => Some(BoardSpec {
                id: m.id,
                slot: key,
                domain: domain.or_else(|| match exchange.field(key).map(|f| f.kind) {
                    Some(super::FieldKind::Enum { domain }) => Some(domain.id),
                    _ => None,
                }),
                scope: m.scope,
            }),
            MultSource::DxccEntity | MultSource::Prefix => None,
        })
        .collect();
    if !declared.is_empty() {
        return declared;
    }
    role.receives
        .iter()
        .filter_map(|key| match exchange.field(key).map(|f| f.kind) {
            Some(super::FieldKind::Enum { domain }) => Some(BoardSpec {
                id: key,
                slot: key,
                domain: Some(domain.id),
                scope: MultScope::PerLog,
            }),
            _ => None,
        })
        .collect()
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
        classes.iter().map(|m| ScoreRow {
            mode_class: m,
            band: "20m",
            role: "",
            rx: &[],
            entity: None,
            prefix: None,
            relation: None,
        })
    }

    /// One relation-scored row on a named band.
    fn rel_row(rel: super::super::callsign::Relation, band: &'static str) -> ScoreRow<'static> {
        ScoreRow {
            mode_class: "CW",
            band,
            role: "",
            rx: &[],
            entity: None,
            prefix: None,
            relation: Some(rel),
        }
    }

    /// ⭐ **CQ WW's four point arms**, from cqww.com/rules §IV.B (read 2026-09-09):
    /// *"Contacts between stations on different continents count three (3) points.
    /// Contacts between stations on the same continent but in different countries count
    /// one (1) point. Exception: Contacts between stations in different countries within
    /// the North American boundaries count two (2) points. Contacts between stations in
    /// the same country have zero (0) QSO point value…"* — flat across all six bands.
    #[test]
    fn cq_wws_four_arms_are_flat_across_every_band() {
        use super::super::callsign::Relation;
        static T: &[RelationPoints] = &[
            RelationPoints {
                relation: Relation::DifferentContinent,
                bands: &[],
                points: 3,
            },
            RelationPoints {
                relation: Relation::WithinNorthAmerica,
                bands: &[],
                points: 2,
            },
            RelationPoints {
                relation: Relation::SameContinent,
                bands: &[],
                points: 1,
            },
            RelationPoints {
                relation: Relation::SameCountry,
                bands: &[],
                points: 0,
            },
        ];
        let r = PointsRule::ByRelation(T);
        for band in ["160m", "80m", "40m", "20m", "15m", "10m"] {
            assert_eq!(
                r.points_for(&rel_row(Relation::DifferentContinent, band)),
                3
            );
            assert_eq!(
                r.points_for(&rel_row(Relation::WithinNorthAmerica, band)),
                2
            );
            assert_eq!(r.points_for(&rel_row(Relation::SameContinent, band)), 1);
            assert_eq!(r.points_for(&rel_row(Relation::SameCountry, band)), 0);
        }
    }

    /// ⭐ **CQ WPX's band groups**, from cqwpx.com/rules §V.B (read 2026-09-09) — the
    /// same four relations, priced by band, including the arm §14 of the design spec was
    /// missing: *"Contacts between stations in the same country are worth 1 point
    /// regardless of band."*
    #[test]
    fn cq_wpx_doubles_the_low_bands_and_prices_same_country_at_one_everywhere() {
        use super::super::callsign::Relation;
        static HIGH: &[&str] = &["10m", "15m", "20m"];
        static LOW: &[&str] = &["40m", "80m", "160m"];
        static T: &[RelationPoints] = &[
            RelationPoints {
                relation: Relation::DifferentContinent,
                bands: HIGH,
                points: 3,
            },
            RelationPoints {
                relation: Relation::DifferentContinent,
                bands: LOW,
                points: 6,
            },
            RelationPoints {
                relation: Relation::WithinNorthAmerica,
                bands: HIGH,
                points: 2,
            },
            RelationPoints {
                relation: Relation::WithinNorthAmerica,
                bands: LOW,
                points: 4,
            },
            RelationPoints {
                relation: Relation::SameContinent,
                bands: HIGH,
                points: 1,
            },
            RelationPoints {
                relation: Relation::SameContinent,
                bands: LOW,
                points: 2,
            },
            RelationPoints {
                relation: Relation::SameCountry,
                bands: &[],
                points: 1,
            },
        ];
        let r = PointsRule::ByRelation(T);
        for b in HIGH {
            assert_eq!(r.points_for(&rel_row(Relation::DifferentContinent, b)), 3);
            assert_eq!(r.points_for(&rel_row(Relation::WithinNorthAmerica, b)), 2);
            assert_eq!(r.points_for(&rel_row(Relation::SameContinent, b)), 1);
        }
        for b in LOW {
            assert_eq!(r.points_for(&rel_row(Relation::DifferentContinent, b)), 6);
            assert_eq!(r.points_for(&rel_row(Relation::WithinNorthAmerica, b)), 4);
            assert_eq!(r.points_for(&rel_row(Relation::SameContinent, b)), 2);
        }
        // ⭐ The arm the source verification found missing: 1 point on EVERY band, where
        // CQ WW gives the same relation zero.
        for b in HIGH.iter().chain(LOW) {
            assert_eq!(
                r.points_for(&rel_row(Relation::SameCountry, b)),
                1,
                "same country is 1 point on {b}"
            );
        }
    }

    /// A table with no North American row falls back to the plain same-continent arm the
    /// exception exists to override — not to zero.
    #[test]
    fn a_table_with_no_north_american_row_falls_back_to_same_continent() {
        use super::super::callsign::Relation;
        static T: &[RelationPoints] = &[RelationPoints {
            relation: Relation::SameContinent,
            bands: &[],
            points: 1,
        }];
        let r = PointsRule::ByRelation(T);
        assert_eq!(
            r.points_for(&rel_row(Relation::WithinNorthAmerica, "20m")),
            1
        );
        // NEGATIVE CONTROL: a relation the table names nothing for at all is zero, and
        // that is the honest answer — nothing declared a price.
        assert_eq!(r.points_for(&rel_row(Relation::SameCountry, "20m")), 0);
    }

    /// A row the country file could not place scores nothing rather than being priced by
    /// an arm nobody established.
    #[test]
    fn an_unplaced_contact_scores_zero_under_a_relation_table() {
        use super::super::callsign::Relation;
        static T: &[RelationPoints] = &[RelationPoints {
            relation: Relation::DifferentContinent,
            bands: &[],
            points: 3,
        }];
        let r = PointsRule::ByRelation(T);
        let mut row = rel_row(Relation::DifferentContinent, "20m");
        assert_eq!(r.points_for(&row), 3, "positive control");
        row.relation = None;
        assert_eq!(r.points_for(&row), 0);
    }

    /// ⭐ §9: Field Day declares NO multiplier rule, and it still shows its
    /// worked-sections board. The fallback is what keeps a shipped display alive
    /// through a UI batch that is not allowed to change behaviour.
    #[test]
    fn field_day_gets_one_board_over_its_one_enum_slot() {
        let spec = crate::contest::field_day(crate::fieldday::FdEvent::ArrlFd);
        let scoring = crate::fd_rules::ruleset(
            crate::fieldday::FdEvent::ArrlFd,
            crate::fd_rules::CURRENT_RULES_YEAR,
        )
        .scoring;
        assert!(
            scoring.multipliers.is_empty(),
            "the premise: Field Day declares no multiplier rule"
        );
        let b = boards(&scoring, spec, &spec.roles[0]);
        assert_eq!(b.len(), 1, "one board, not none and not one per slot");
        assert_eq!(b[0].id, "SECTION");
        assert_eq!(b[0].slot, "SECTION");
        assert_eq!(b[0].domain, Some("fd_sections"));
        // CLASS is a Pattern, so it is not a board — a board needs a closed universe.
        assert!(b.iter().all(|x| x.slot != "CLASS"));
    }

    /// The declared path: one board per rule this role counts, in rule order — the
    /// "CQ WW shows two boards" case, with the role filter doing its job.
    #[test]
    fn declared_rules_win_and_are_filtered_by_role() {
        static RULES: &[MultiplierRule] = &[
            MultiplierRule {
                id: "zone",
                source: MultSource::Field {
                    key: "ZONE",
                    domain: None,
                },
                scope: MultScope::PerBand,
                excluding: &[],
                roles: &[],
            },
            MultiplierRule {
                id: "county",
                source: MultSource::Field {
                    key: "QTH",
                    domain: Some("tn_counties"),
                },
                scope: MultScope::PerLog,
                excluding: &[],
                roles: &["out_of_state"],
            },
            // Not a slot: its universe is the country file, not a domain.
            MultiplierRule {
                id: "country",
                source: MultSource::DxccEntity,
                scope: MultScope::PerBand,
                excluding: &[],
                roles: &[],
            },
        ];
        static POST: &[PostMultiplier] = &[];
        let scoring = Scoring {
            qso_points: PointsRule::ByModeClass(FD_POINTS),
            multipliers: RULES,
            post: POST,
        };
        let spec = crate::contest::field_day(crate::fieldday::FdEvent::ArrlFd);
        // A role the county rule does not name gets the zone board only…
        let mine = super::super::RoleSpec {
            id: "in_state",
            selector: super::super::RoleSelector::Always,
            sends: &[],
            receives: &["SECTION"],
            constant_sent: &[],
        };
        let b = boards(&scoring, spec, &mine);
        assert_eq!(
            b.iter().map(|x| x.id).collect::<Vec<_>>(),
            vec!["zone"],
            "the country rule is not a slot and the county rule is another role's"
        );
        assert_eq!(b[0].scope, MultScope::PerBand, "a per-band board says so");
        // …and the role that IS named gets both. POSITIVE CONTROL for the filter:
        // without it this would be identical to the line above.
        let theirs = super::super::RoleSpec {
            id: "out_of_state",
            ..mine
        };
        assert_eq!(
            boards(&scoring, spec, &theirs)
                .iter()
                .map(|x| x.id)
                .collect::<Vec<_>>(),
            vec!["zone", "county"]
        );
        // The declared list wins outright — SECTION is received and gets no board.
        assert!(boards(&scoring, spec, &theirs)
            .iter()
            .all(|x| x.slot != "SECTION"));
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
