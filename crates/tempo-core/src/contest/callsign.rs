//! ⭐ **What a contest needs to know about a CALLSIGN** — the entity and continent it
//! sits in, and the CQ WPX prefix it counts as.
//!
//! Every contest shipped before this one took its multipliers and its points out of the
//! EXCHANGE: a section, a county, a state, a serial. CQ WW and CQ WPX do not. Their
//! country multiplier, their prefix multiplier and all four of their QSO-point arms are
//! functions of the call sign itself, so a batch that ships them has to answer two
//! questions this crate had no answer for:
//!
//! * **Where is that station?** — [`CallLocation`], which comes from a country file.
//! * **What prefix does that call count as?** — [`wpx_prefix`], which is a rule, not data.
//!
//! ## Why the country file arrives through an INSTALLED resolver
//!
//! The cty.dat resolver lives in `crates/propagation`, and `propagation` depends on
//! `tempo-core` — so this crate cannot call it, now or ever, without a dependency cycle.
//! The composition root (`src-tauri`) holds both, so it installs the resolver at startup
//! exactly as it installs the rules file (`fd_rules::install_from`). That keeps the
//! country file in the one crate that vendors it and leaves every signature in this
//! crate unchanged.
//!
//! ⚠️ **A build with no resolver installed resolves NOTHING, and that is a refusal, not
//! a zero.** [`ContestSession::for_ruleset`](super::ContestSession::for_ruleset) refuses
//! a contest whose points are [`PointsRule::ByRelation`](super::PointsRule::ByRelation)
//! when the resolver is absent or when the operator's own call does not resolve — because
//! the alternative is a 48-hour contest scoring every contact at zero and saying nothing.
//! Contests that merely declare a
//! [`MultSource::DxccEntity`](super::MultSource::DxccEntity) multiplier are NOT refused:
//! that is TNQP's and TXQP's shape, they shipped counting nothing, and refusing them here
//! would take away contests over a multiplier the sponsor treats as one term of several.

use std::sync::OnceLock;

/// Where a callsign is, as a contest reads it.
///
/// Both halves are `&'static str` because the country file is parsed once into a static
/// table; this type is therefore `Copy` and costs nothing to carry on a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallLocation {
    /// The DXCC/WAE entity name, exactly as the country file spells it
    /// (`"United States"`, `"Canada"`, `"Alaska"`, `"Hawaii"`). It is compared by
    /// STRING against [`MultiplierRule::excluding`](super::MultiplierRule::excluding),
    /// so a rules file that excludes an entity must use the country file's spelling.
    pub entity: &'static str,
    /// The continent code — `AF` / `AS` / `EU` / `NA` / `OC` / `SA`.
    ///
    /// ⚠️ **Not derivable from the CQ zone.** `propagation::dxcc::DxccInfo::cont` carries
    /// the measurement that settled it: against 258 237 real RBN spots a zone-derived
    /// continent was 6.5 % wrong, because CQ zone 20 spans Europe and Asia. CQ WW's own
    /// rule names *"continental boundaries"* as a standard in its own right.
    pub continent: &'static str,
}

impl CallLocation {
    /// `(entity, continent)` — the pair [`Relation::between`] compares.
    pub const fn as_pair(&self) -> (&'static str, &'static str) {
        (self.entity, self.continent)
    }
}

/// A function that places a callsign. Installed once, by the composition root.
pub type CallResolver = fn(&str) -> Option<CallLocation>;

static RESOLVER: OnceLock<CallResolver> = OnceLock::new();

/// Install the callsign resolver. `Err` when one is already installed — the same
/// once-only contract `fd_rules::install_from` has, and for the same reason: every
/// resolved [`CallLocation`] borrows `&'static` data out of the installed table, and a
/// live swap would leave rows already logged pointing at a table that is gone.
pub fn install_call_resolver(f: CallResolver) -> Result<(), &'static str> {
    RESOLVER
        .set(f)
        .map_err(|_| "a call resolver is already installed")
}

/// Is a resolver installed at all? Read by the session constructor, which refuses a
/// relation-scored contest without one rather than scoring its whole log at zero.
pub fn call_resolver_installed() -> bool {
    RESOLVER.get().is_some()
}

/// Place a callsign, or `None` when no resolver is installed or the country file cannot
/// place it.
pub fn resolve_call(call: &str) -> Option<CallLocation> {
    let f = RESOLVER.get()?;
    let call = call.trim();
    if call.is_empty() {
        return None;
    }
    f(call)
}

/// ⭐ **The relation between MY station and the station I worked** — the axis CQ WW and
/// CQ WPX price a contact on.
///
/// Exactly one arm is true of any contact, which is what makes
/// [`PointsRule::ByRelation`](super::PointsRule::ByRelation) a lookup rather than an
/// ordered rule list: an ordered list has to be read in the right order to be right, and
/// nothing in a JSON array enforces an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// Same DXCC entity. CQ WW: *"Contacts between stations in the same country have zero
    /// (0) QSO point value"*. CQ WPX: *"worth 1 point regardless of band"* — the same
    /// relation, a different price, which is the whole reason this is data.
    SameCountry,
    /// Same continent, different country, **and both stations inside North America** —
    /// CQ WW's and CQ WPX's *"Exception: … within the North American boundaries"*.
    ///
    /// A contest that declares no row for this arm falls back to [`SameContinent`], which
    /// is what the exception is an exception TO.
    ///
    /// [`SameContinent`]: Relation::SameContinent
    WithinNorthAmerica,
    /// Same continent, different country, outside the North American exception.
    SameContinent,
    /// Different continents.
    DifferentContinent,
}

/// The continent code North America carries in the country file.
pub const NORTH_AMERICA: &str = "NA";

impl Relation {
    /// The relation between two placed stations, or `None` when either is unplaced.
    ///
    /// Each side is `(entity, continent)` rather than a [`CallLocation`] because the two
    /// sides come from different lifetimes: mine is the session's `&'static` answer, and
    /// theirs is the `String` pair its own row recorded at log time. Widening the
    /// argument is what keeps a row from having to leak a `&'static str` to be scored.
    ///
    /// ⚠️ **Country first, continent second.** Two stations in the same entity are always
    /// [`SameCountry`](Relation::SameCountry) even though they are trivially on the same
    /// continent — CQ WW prices those at 0 and CQ WPX at 1, and reading the continent
    /// first would price every domestic contact as a same-continent one.
    pub fn between(mine: Option<(&str, &str)>, theirs: Option<(&str, &str)>) -> Option<Self> {
        let ((a_entity, a_cont), (b_entity, b_cont)) = (mine?, theirs?);
        if a_entity == b_entity {
            return Some(Relation::SameCountry);
        }
        if a_cont != b_cont {
            return Some(Relation::DifferentContinent);
        }
        Some(if a_cont == NORTH_AMERICA {
            Relation::WithinNorthAmerica
        } else {
            Relation::SameContinent
        })
    }

    /// The wire name in the rules file.
    pub const fn wire(self) -> &'static str {
        match self {
            Relation::SameCountry => "same_country",
            Relation::WithinNorthAmerica => "within_north_america",
            Relation::SameContinent => "same_continent",
            Relation::DifferentContinent => "different_continent",
        }
    }
}

/// ⭐ **The CQ WPX prefix a callsign counts as** (CQ WPX rules §V.C.1, read 2026-09-09 at
/// <https://cqwpx.com/rules/>), or `None` for something that is not a callsign.
///
/// The sponsor's own words, in full, because every branch below is one of its sentences:
///
/// > *"A PREFIX is the letter/numeral combination which forms the first part of the
/// > amateur call. Examples: N8, W8, WD8, HG1, HG19, KC2, OE2, OE25, LY1000, etc. Any
/// > difference in the numbering, lettering, or order of same shall count as a separate
/// > prefix. In cases of portable operation, the portable designator will then become the
/// > prefix. The portable prefix must be an authorized prefix of the country/call area of
/// > operation. Portable designators without numbers will be assigned a zero (Ø) after the
/// > second letter of the portable designator to form the prefix. Example: PA/N8BJQ would
/// > become PAØ. All calls without numbers will be assigned a zero (Ø) after the first two
/// > letters to form the prefix. Example: XEFTJW would count as XEØ. Maritime mobile,
/// > mobile, /A, /E, /J, /P, or other license class identifiers do not count as
/// > prefixes."*
///
/// and the sponsor's own FAQ (<https://cqwpx.com/rules_faq.htm>, read 2026-09-09), which
/// is where the "where does the prefix END" question is actually answered:
///
/// > *"The prefix includes everything up to the end of the first numbers in the call.
/// > Some examples showing the call and how the prefix is counted. OL25LP = OL25 /
/// > DL60CHILD = DL60 / 9A800VZ = 9A800 / DR2006Q = DR2006 / LY1000CW = LY1000 /
/// > KL7RA/WK9 = WK9 / OE/K5ZD = OEØ"*
///
/// ⚠️ **"the first numbers" is the first digit run that FOLLOWS A LETTER, not the first
/// digit.** `9A800VZ` is the sponsor's own counterexample: its first digit is the leading
/// `9`, and stopping there would count the prefix as `9`. The run this stops on is `800`,
/// which is why the answer is `9A800`. `4U1ITU` is the same shape — leading `4`, then the
/// run `1` after the `U` — and counts as `4U1`.
///
/// ⚠️ **One arm has no sponsor worked example: a portable designator that is BARE DIGITS
/// (`W1AW/4`).** The rule sentence it is built on is *"the portable designator will then
/// become the prefix"* plus *"must be an authorized prefix of the country/call area of
/// operation"* — and a bare `4` is not an authorized prefix on its own, so the digits
/// replace the base prefix's own digits (`W1AW/4` → `W4`). Every logger in the field does
/// this and the sponsor publishes no example either way; it is flagged here and in the
/// seed's `_provenance` rather than presented as quoted.
pub fn wpx_prefix(call: &str) -> Option<String> {
    let call = call.trim().to_ascii_uppercase();
    if call.is_empty() {
        return None;
    }
    // "Maritime mobile, mobile, /A, /E, /J, /P, or other license class identifiers do not
    // count as prefixes" — they are dropped before anything else looks at the parts, so
    // `PA/N8BJQ/P` is the same question as `PA/N8BJQ`.
    let parts: Vec<&str> = call
        .split('/')
        .filter(|p| !p.is_empty() && !is_license_class_identifier(p))
        .collect();
    let (base, designator) = match parts.as_slice() {
        [] => return None,
        [one] => (*one, None),
        // Two carriers of information: one is the call, the other the portable
        // designator. The designator is the SHORTER — `OE/K5ZD` and `KL7RA/WK9` are both
        // the sponsor's own examples and both resolve that way. A tie takes the first,
        // which is the `PREFIX/CALL` form.
        [a, b, ..] => {
            if b.len() < a.len() {
                (*a, Some(*b))
            } else {
                (*b, Some(*a))
            }
        }
    };
    let base_prefix = leading_prefix(base)?;
    let Some(d) = designator else {
        return Some(base_prefix);
    };
    if d.chars().all(|c| c.is_ascii_digit()) {
        // The bare-digit arm (see the note above): the base prefix's own digits are
        // replaced by the designator's.
        let letters: String = base_prefix
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .to_string();
        if letters.is_empty() {
            return None;
        }
        return Some(format!("{letters}{d}"));
    }
    if d.chars().any(|c| c.is_ascii_digit()) {
        // "the portable designator will then become the prefix" — verbatim, because the
        // designator IS an authorized prefix (`KL7RA/WK9` = `WK9`).
        return Some(d.to_string());
    }
    // "Portable designators without numbers will be assigned a zero (Ø) after the second
    // letter of the portable designator" — `OE/K5ZD` = `OE0`, `PA/N8BJQ` = `PA0`.
    let mut p: String = d.chars().take(2).collect();
    if p.len() < 2 {
        return None;
    }
    p.push('0');
    Some(p)
}

/// The prefix of a call that carries no portable designator.
///
/// "everything up to the end of the first numbers in the call", where "the first numbers"
/// is the first digit run preceded by at least one letter; a call with no such run gets
/// "a zero after the first two letters" (`XEFTJW` = `XE0`).
fn leading_prefix(call: &str) -> Option<String> {
    let b = call.as_bytes();
    if b.len() < 2 || !b.iter().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let mut seen_letter = false;
    for (i, c) in b.iter().enumerate() {
        if c.is_ascii_alphabetic() {
            seen_letter = true;
        } else if seen_letter {
            // Take the whole run, not one digit: `LY1000CW` is `LY1000`.
            let mut end = i;
            while end < b.len() && b[end].is_ascii_digit() {
                end += 1;
            }
            return Some(call[..end].to_string());
        }
    }
    // "All calls without numbers will be assigned a zero after the first two letters."
    Some(format!("{}0", &call[..2]))
}

/// The `/`-parts the sponsor names as NOT prefixes. `MM` and `AM` are maritime and
/// aeronautical mobile; the single letters are the license-class and mobile/portable
/// identifiers the rule lists; `QRP` is the one other identifier common enough to name.
///
/// ⚠️ The rule's *"or other license class identifiers"* is open-ended and this list is
/// not. A designator this build does not recognise is treated as a designator — which is
/// the direction that counts a real prefix, rather than silently dropping one.
fn is_license_class_identifier(p: &str) -> bool {
    matches!(p, "MM" | "AM" | "M" | "P" | "A" | "E" | "J" | "QRP")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn na(entity: &'static str) -> Option<(&'static str, &'static str)> {
        Some((entity, "NA"))
    }

    /// ⭐ Every prefix example the sponsor publishes, from both of its own pages, as
    /// literals — so a "tidy" of the scan below is a red test with the sponsor's own
    /// worked answer attached.
    #[test]
    fn the_sponsors_own_prefix_examples_all_come_out_as_published() {
        for (call, want) in [
            // rules §V.C.1's own list of what a prefix looks like
            ("N8BJQ", "N8"),
            ("W8ABC", "W8"),
            ("WD8XYZ", "WD8"),
            ("HG1ABC", "HG1"),
            ("HG19ABC", "HG19"),
            ("KC2ABC", "KC2"),
            ("OE2ABC", "OE2"),
            ("OE25ABC", "OE25"),
            ("LY1000ABC", "LY1000"),
            // rules §V.C.1's two worked conversions
            ("PA/N8BJQ", "PA0"),
            ("XEFTJW", "XE0"),
            // the FAQ's seven
            ("OL25LP", "OL25"),
            ("DL60CHILD", "DL60"),
            ("9A800VZ", "9A800"),
            ("DR2006Q", "DR2006"),
            ("LY1000CW", "LY1000"),
            ("KL7RA/WK9", "WK9"),
            ("OE/K5ZD", "OE0"),
        ] {
            assert_eq!(
                wpx_prefix(call).as_deref(),
                Some(want),
                "{call} counts as {want}"
            );
        }
    }

    /// ⚠️ `4U1ITU` — the call the brief names, and the shape `9A800VZ` proves the rule
    /// for: a LEADING digit is not "the first numbers", because the run that ends the
    /// prefix has to follow a letter.
    #[test]
    fn a_leading_digit_is_not_the_prefixs_own_number() {
        assert_eq!(wpx_prefix("4U1ITU").as_deref(), Some("4U1"));
        assert_eq!(wpx_prefix("4U1UN").as_deref(), Some("4U1"));
        assert_eq!(wpx_prefix("9A800VZ").as_deref(), Some("9A800"));
        assert_eq!(wpx_prefix("2E0ABC").as_deref(), Some("2E0"));
        // NEGATIVE CONTROL: the naive "stop at the first digit" reading would answer `4`
        // and `9` for the two above, so this is the assertion that would fail if somebody
        // simplified the scan.
        assert_ne!(wpx_prefix("4U1ITU").as_deref(), Some("4"));
        assert_ne!(wpx_prefix("9A800VZ").as_deref(), Some("9"));
    }

    /// *"Maritime mobile, mobile, /A, /E, /J, /P, or other license class identifiers do
    /// not count as prefixes"* — each one dropped, and the POSITIVE CONTROL that a
    /// designator which IS a prefix is not dropped by the same filter.
    #[test]
    fn a_license_class_identifier_is_dropped_and_a_real_designator_is_not() {
        for id in ["MM", "AM", "M", "P", "A", "E", "J", "QRP"] {
            assert_eq!(
                wpx_prefix(&format!("N8BJQ/{id}")).as_deref(),
                Some("N8"),
                "/{id} does not count as a prefix"
            );
        }
        // POSITIVE CONTROL: the same two-part shape with a real designator DOES move the
        // prefix, so the eight answers above are the filter and not a parser that ignores
        // everything after the slash.
        assert_eq!(wpx_prefix("N8BJQ/HC8").as_deref(), Some("HC8"));
        // …and a stacked identifier behind a real designator leaves the designator.
        assert_eq!(wpx_prefix("PA/N8BJQ/P").as_deref(), Some("PA0"));
    }

    /// The bare-digit designator — the one arm with no sponsor worked example. The base
    /// prefix's digits are replaced, not appended to.
    #[test]
    fn a_bare_digit_designator_replaces_the_base_prefixs_digits() {
        assert_eq!(wpx_prefix("W1AW/4").as_deref(), Some("W4"));
        assert_eq!(wpx_prefix("N8BJQ/9").as_deref(), Some("N9"));
        assert_eq!(wpx_prefix("WD8ABC/0").as_deref(), Some("WD0"));
        // A leading-digit call keeps its leading digit and swaps only the prefix's own.
        assert_eq!(wpx_prefix("4U1ITU/2").as_deref(), Some("4U2"));
        // ⚠️ NOT `W14` — appending instead of replacing invents a prefix nobody holds.
        assert_ne!(wpx_prefix("W1AW/4").as_deref(), Some("W14"));
    }

    /// *"Any difference in the numbering, lettering, or order of same shall count as a
    /// separate prefix"* — the sentence that makes the prefix multiplier big, asserted as
    /// four calls that share a country and count four ways.
    #[test]
    fn calls_differing_only_in_number_or_letters_are_separate_prefixes() {
        let p: Vec<String> = ["N8BJQ", "N9BJQ", "W8BJQ", "WD8BJQ"]
            .iter()
            .filter_map(|c| wpx_prefix(c))
            .collect();
        assert_eq!(p, vec!["N8", "N9", "W8", "WD8"]);
    }

    #[test]
    fn something_that_is_not_a_callsign_has_no_prefix() {
        for s in ["", "   ", "/", "W", "1", "//", "N8-BJQ"] {
            assert_eq!(wpx_prefix(s), None, "{s:?} is not a callsign");
        }
        // An EMPTY `/`-part is dropped like a stray identifier rather than making the
        // call unreadable: `W1AW/` is a typo for `W1AW`, and refusing it would drop a
        // real prefix from the multiplier count over a trailing keystroke.
        assert_eq!(wpx_prefix("W1AW/").as_deref(), Some("W1"));
        // POSITIVE CONTROL: the same function answers for a real call, so the `None`s
        // above are the input and not a function that always refuses.
        assert_eq!(wpx_prefix(" w1aw ").as_deref(), Some("W1"));
    }

    /// ⭐ The four relation arms, each from one side of the two facts that decide it.
    #[test]
    fn the_relation_reads_country_first_and_continent_second() {
        let us = na("United States");
        let ve = na("Canada");
        let eu = Some(("Germany", "EU"));
        let sa = Some(("Brazil", "SA"));
        assert_eq!(Relation::between(us, us), Some(Relation::SameCountry));
        assert_eq!(
            Relation::between(us, ve),
            Some(Relation::WithinNorthAmerica)
        );
        assert_eq!(
            Relation::between(us, eu),
            Some(Relation::DifferentContinent)
        );
        assert_eq!(
            Relation::between(eu, sa),
            Some(Relation::DifferentContinent)
        );
        // Two EU countries are plain same-continent: the NA exception is North America's
        // alone.
        let eu2 = Some(("France", "EU"));
        assert_eq!(Relation::between(eu, eu2), Some(Relation::SameContinent));
        // ⚠️ Same entity wins over same continent — a US-to-US contact is SameCountry,
        // which CQ WW prices at zero and WithinNorthAmerica at two.
        assert_ne!(
            Relation::between(us, us),
            Some(Relation::WithinNorthAmerica)
        );
        // An unplaced station on either side has no relation at all.
        assert_eq!(Relation::between(us, None), None);
        assert_eq!(Relation::between(None, us), None);
    }

    /// Hawaii is `OC` in the country file, so a US-to-Hawaii contact is a
    /// DIFFERENT-continent contact — three points in CQ WW, not two. This is the
    /// continental-boundary fact a CQ-zone-derived continent would get wrong.
    #[test]
    fn hawaii_is_oceania_and_alaska_is_north_america() {
        let us = na("United States");
        let hi = Some(("Hawaii", "OC"));
        let ak = na("Alaska");
        assert_eq!(
            Relation::between(us, hi),
            Some(Relation::DifferentContinent)
        );
        assert_eq!(
            Relation::between(us, ak),
            Some(Relation::WithinNorthAmerica)
        );
    }

    /// With nothing installed, nothing resolves — and the session constructor's refusal
    /// is built on exactly this answer.
    ///
    /// ⚠️ It does NOT install one: `install_call_resolver` is once-per-process and this
    /// is the lib test binary, so installing here would decide the answer for every other
    /// test in it. The installed path is exercised by
    /// `crates/tempo-core/tests/cqww.rs`, which is its own binary.
    #[test]
    fn a_build_with_no_resolver_places_nothing() {
        assert!(!call_resolver_installed());
        assert_eq!(resolve_call("W1AW"), None);
    }
}
