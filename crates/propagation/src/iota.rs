//! IOTA (Islands On The Air) references: the one spelling both sides of the IOTA need compare
//! by, the reference a DX-cluster spot's comment carries, and the island-group list.
//!
//! **One resolver, both sides ([`canonical`]).** The New IOTA need is a comparison between the
//! group a heard station is on (a spot's comment, a callbook answer) and the groups the log has
//! worked ([`crate::dxped::LogNeeds::add_iota`]). Both go through [`canonical`], so a reference
//! the log holds as `eu-131 ` and one a spot carries as `EU-131` are the same group, and the need
//! clears the moment the contact is logged. Two spellings on two sides is the bug class that kept
//! New State lit forever (see `needalert.rs`).
//!
//! **A spot comment is free text ([`in_comment`]).** Spotters write the island group the station
//! is on beside everything else: "IOTA EU-131", "OC-146 QSL via M0XXX", "TNX IOTA:AS-007". The
//! parser is strict on purpose, because a wrong reference offered to the operator, or a need lit
//! for a group the station is not on, costs more than a missed one: the reference must be in
//! capitals (`EU-131`), stand on its own (not `EU-1311`, not the `HA-001` of a summit code `EU/HA-001`,
//! not a four-digit park number), and a comment naming two different groups names none. The bare
//! `EU131` spelling counts only right after the word IOTA.
//!
//! **The island-group list ([`IotaGroups`]).** It is © Radio Society of Great Britain and
//! © Islands On The Air (IOTA) Ltd, published at iota-world.org. A copy ships with Nexus by IOTA
//! Ltd's permission and the shell refreshes it at most monthly (see NOTICE); this module only
//! parses it. Without a list (the operator turned it off), a reference is checked by its shape alone.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The seven IOTA continent codes.
const CONTINENTS: [&str; 7] = ["AF", "AN", "AS", "EU", "NA", "OC", "SA"];

/// The one spelling of an IOTA reference: trimmed, upper case, `XX-NNN` with `XX` one of the
/// seven continent codes. `None` for anything else, so junk never counts on either side.
pub fn canonical(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_uppercase();
    let (cont, num) = s.split_once('-')?;
    let ok =
        CONTINENTS.contains(&cont) && num.len() == 3 && num.bytes().all(|b| b.is_ascii_digit());
    ok.then_some(s)
}

/// The IOTA reference a spot comment names, when it names exactly one (see the module header).
pub fn in_comment(comment: &str) -> Option<String> {
    let b = comment.as_bytes();
    let mut found: Option<String> = None;
    for i in 0..b.len() {
        let Some(r) = reference_at(b, i) else {
            continue;
        };
        match &found {
            Some(f) if *f != r => return None, // two different groups: neither
            _ => found = Some(r),
        }
    }
    found
}

/// The reference starting at byte `i`, if one does: `XX-NNN` standing on its own, or `XXNNN` /
/// `XX NNN` right after the word IOTA.
fn reference_at(b: &[u8], i: usize) -> Option<String> {
    let cont = b.get(i..i + 2)?;
    if !CONTINENTS.iter().any(|c| c.as_bytes() == cont) {
        return None;
    }
    // It must start a word: not the tail of `ONFF`, not the `HA` of `EU/HA-001`, not after a
    // hyphen (`XX-EU-131`).
    if i > 0 && is_joined(b[i - 1]) {
        return None;
    }
    let after_iota = follows_iota(b, i);
    let digits_at = match b.get(i + 2) {
        Some(b'-') => i + 3,
        Some(b' ') if after_iota => i + 3,
        Some(c) if c.is_ascii_digit() && after_iota => i + 2,
        _ => return None,
    };
    let digits = b.get(digits_at..digits_at + 3)?;
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    // …and end one: not a four-digit park number (`AS-1234`), not `NA-046A`.
    if b.get(digits_at + 3)
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return None;
    }
    let cont = std::str::from_utf8(cont).ok()?;
    let digits = std::str::from_utf8(digits).ok()?;
    Some(format!("{cont}-{digits}"))
}

/// A byte that joins what follows it to what came before: a letter, a digit, `/` or `-`.
fn is_joined(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'/' || c == b'-'
}

/// Whether the word IOTA (any case) ends just before byte `i`, with at most one `:`, `=`, `-` or
/// space between them.
fn follows_iota(b: &[u8], i: usize) -> bool {
    let mut end = i;
    if end > 0 && matches!(b[end - 1], b':' | b'=' | b'-' | b' ') {
        end -= 1;
    }
    end >= 4
        && b[end - 4..end].eq_ignore_ascii_case(b"IOTA")
        && (end == 4 || !b[end - 5].is_ascii_alphanumeric())
}

/// One island group of the IOTA list: its reference and its name as IOTA Ltd writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IotaGroup {
    pub reference: String,
    pub name: String,
}

/// The island-group list, by reference (see the module header).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IotaGroups {
    by_ref: BTreeMap<String, String>,
}

/// Fewer groups than this in a download means its shape changed (or the server sent something
/// else): the copy is refused and the one already held is kept. The list held 1,178 groups in
/// October 2026 and changes by a handful a year.
pub const MIN_PLAUSIBLE_GROUPS: usize = 900;

/// The longest group name kept, in characters; a longer one is cut to it. A real name is a few
/// words.
pub const NAME_MAX_CHARS: usize = 200;

/// One entry of IOTA's `groups.json`, as far as Nexus reads it.
#[derive(Deserialize)]
struct RawGroup {
    refno: Option<String>,
    name: Option<String>,
}

impl IotaGroups {
    /// Parse IOTA's `groups.json` (an array of objects carrying `refno` and `name`). An entry
    /// without a valid reference is skipped; a document that is not such an array is an `Err`,
    /// never a panic, so a changed file format leaves the operator on the shape-only check.
    pub fn parse(json: &str) -> Result<IotaGroups, String> {
        let raw: Vec<RawGroup> = serde_json::from_str(json)
            .map_err(|e| format!("the IOTA list is not in the expected format: {e}"))?;
        let mut by_ref = BTreeMap::new();
        for g in raw {
            let (Some(r), Some(name)) = (g.refno.as_deref().and_then(canonical), g.name) else {
                continue;
            };
            let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
            let name = match name.char_indices().nth(NAME_MAX_CHARS) {
                Some((cut, _)) => name[..cut].trim_end().to_string(),
                None => name,
            };
            if !name.is_empty() {
                by_ref.insert(r, name);
            }
        }
        if by_ref.is_empty() {
            return Err("the IOTA list holds no island groups".to_string());
        }
        Ok(IotaGroups { by_ref })
    }

    /// Whether a download is big enough to be the real list ([`MIN_PLAUSIBLE_GROUPS`]).
    pub fn plausible(&self) -> bool {
        self.by_ref.len() >= MIN_PLAUSIBLE_GROUPS
    }

    /// The group's name, for a reference in any spelling [`canonical`] accepts.
    pub fn name(&self, reference: &str) -> Option<&str> {
        self.by_ref.get(&canonical(reference)?).map(String::as_str)
    }

    /// Whether the list has this group.
    pub fn contains(&self, reference: &str) -> bool {
        self.name(reference).is_some()
    }

    pub fn len(&self) -> usize {
        self.by_ref.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_ref.is_empty()
    }

    /// Every group, in reference order.
    pub fn groups(&self) -> Vec<IotaGroup> {
        self.by_ref
            .iter()
            .map(|(r, n)| IotaGroup {
                reference: r.clone(),
                name: n.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_takes_the_one_spelling_and_refuses_junk() {
        assert_eq!(canonical(" eu-131 ").as_deref(), Some("EU-131"));
        assert_eq!(canonical("NA-046").as_deref(), Some("NA-046"));
        for junk in [
            "EU-5", "EU-1311", "XX-131", "EU131", "", "US-0001", "EU-13A",
        ] {
            assert_eq!(canonical(junk), None, "{junk:?}");
        }
    }

    /// Comment shapes as spotters write them on the DX cluster.
    #[test]
    fn a_spot_comment_names_its_island_group() {
        let cases = [
            ("IOTA EU-131", "EU-131"),
            ("EU-131 QSL via IK3XXX", "EU-131"),
            ("CQ CQ NA-046 up 2", "NA-046"),
            ("TNX QSO IOTA OC-001 5NN", "OC-001"),
            ("op from AF-004 loud", "AF-004"),
            ("IOTA:AS-007", "AS-007"),
            ("iota AS007", "AS-007"),
            ("IOTA SA 060 pse QSL", "SA-060"),
            ("(AN-016) new one", "AN-016"),
            ("EU-131, EU-131 again", "EU-131"),
            ("FT8 -12 dB IOTA=OC-146", "OC-146"),
        ];
        for (comment, want) in cases {
            assert_eq!(in_comment(comment).as_deref(), Some(want), "{comment:?}");
        }
    }

    #[test]
    fn a_comment_that_names_no_single_group_offers_none() {
        let cases = [
            "EU-5",            // too few digits
            "eu-131",          // lower case outside an IOTA keyword
            "SOTA EU/HA-001",  // a summit code
            "W7A/NA-001",      // a summit in a region spelled like a continent
            "POTA US-0001",    // a park
            "AS-1234",         // a four-digit park number
            "NA-046A",         // a reference glued to more
            "ONFF-0123 WWFF",  // a WWFF park
            "EU131 tnx",       // the bare spelling needs the word IOTA
            "EU-131 / AF-004", // two different groups: neither
            "UP 2 QSX 14.195", // nothing at all
            "XEU-131",         // the tail of a word
            "NOTIOTA EU131",   // IOTA must be a word of its own
        ];
        for comment in cases {
            assert_eq!(in_comment(comment), None, "{comment:?}");
        }
    }

    const SAMPLE: &str = include_str!("../tests/fixtures/iota_groups_sample.json");

    #[test]
    fn the_island_list_parses_names_by_reference() {
        let g = IotaGroups::parse(SAMPLE).expect("the sample parses");
        assert_eq!(g.len(), 3, "the entry with a bad reference is skipped");
        assert_eq!(g.name("EU-901"), Some("Example Isles group"));
        assert_eq!(
            g.name("eu-901 "),
            Some("Example Isles group"),
            "any spelling"
        );
        assert_eq!(g.name("OC-902"), Some("Sample Reef -aka Test Atoll-"));
        assert!(!g.contains("EU-999"));
        assert!(!g.plausible(), "three groups is not the real list");
    }

    /// A name far longer than any group's is cut to [`NAME_MAX_CHARS`], at a character, never
    /// mid-way through one; a name at the limit is kept whole.
    #[test]
    fn a_very_long_group_name_is_cut() {
        let long = "é".repeat(NAME_MAX_CHARS + 50);
        let at = "a".repeat(NAME_MAX_CHARS);
        let json =
            format!(r#"[{{"refno":"EU-901","name":"{long}"}},{{"refno":"EU-902","name":"{at}"}}]"#);
        let g = IotaGroups::parse(&json).expect("parses");
        assert_eq!(g.name("EU-901"), Some("é".repeat(NAME_MAX_CHARS).as_str()));
        assert_eq!(
            g.name("EU-902"),
            Some(at.as_str()),
            "CONTROL: at the limit, kept whole"
        );
    }

    #[test]
    fn a_changed_file_format_is_an_error_not_a_panic() {
        for bad in [
            "",
            "<html>maintenance</html>",
            r#"{"groups":[{"refno":"EU-001","name":"x"}]}"#,
            r#"[{"ref":"EU-001","title":"x"}]"#,
            "[]",
        ] {
            assert!(IotaGroups::parse(bad).is_err(), "{bad:?}");
        }
    }
}
