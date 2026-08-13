//! DXCC entity resolver backed by the vendored AD1C **`cty.dat`** country file
//! (`data/cty.dat`, MIT-licensed — see `data/cty.dat_copyright.txt`).
//!
//! Resolves a callsign → DXCC entity name + a representative lat/lon. Used to
//! locate live DXpeditions and to bucket the operator's logged contacts into
//! worked entities for the "needs" model ([`crate::dxped::LogNeeds`]).
//!
//! The file is embedded with `include_str!` and parsed once behind a
//! [`OnceLock`], so resolution is offline and self-contained. Matching mirrors
//! standard DXCC practice: **exact-call overrides first** (cty.dat's `=CALL`
//! entries — e.g. `3Y0J`→Bouvet, which has no plain `3Y` prefix), then
//! **longest-prefix** after stripping a portable affix.
//!
//! NB cty.dat stores **West-positive longitude**; we negate it to the usual
//! East-positive convention the rest of the crate uses.

use std::collections::HashMap;
use std::sync::OnceLock;

/// A resolved DXCC entity with a representative location (entity centroid) and
/// CQ zone (for WAZ). The zone is the prefix's `(cq)` override when cty.dat gives
/// one (multi-zone entities like W/VE/UA), else the entity's default zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DxccInfo {
    pub entity: &'static str,
    pub lat: f64,
    pub lon: f64,
    pub cq_zone: u8,
    /// Continent code from cty.dat's `Cont` field — one of `AF`/`AS`/`EU`/`NA`/
    /// `OC`/`SA` (empty only if the file ever omits it).
    ///
    /// This is the coarse locality unit for HF, where an entity CENTROID is far
    /// too coarse to threshold on: every US callsign resolves to one point in
    /// Missouri, so "distance to the spotter" is 597 km for a Utah skimmer and a
    /// Florida skimmer alike, and any radius between ~4500 and ~6000 km is
    /// numerically identical to "same continent" while pretending to be a
    /// measurement. See [`crate::needalert::hf_admit_spotters`].
    ///
    /// Do NOT substitute a CQ-zone→continent mapping: measured against 258,237
    /// real RBN spots (2026-08-03) this field agreed with RBN's own `de_cont` on
    /// every single one, while zone-derived continents were 6.5% wrong — CQ zone
    /// 20 spans both, so Bulgaria, Romania and Greece (some of the busiest EU
    /// skimmers) come out as Asia.
    pub cont: &'static str,
    /// `true` for ARRL DXCC entities; `false` for WAE/CQ-only entities (Sicily,
    /// European Turkey, African Italy, Shetland, Bear Island, Vienna Intl Ctr —
    /// valid contest multipliers but NOT DXCC). The CQ zone is still valid for
    /// WAZ on a non-DXCC entity, so callers gate DXCC credit on this flag while
    /// still using `cq_zone`.
    pub is_dxcc: bool,
}

struct Entity {
    name: String,
    lat: f64,
    lon: f64,
    /// Default CQ zone (cty.dat header field 2); 0 if unparsed.
    cq_zone: u8,
    /// Continent code (cty.dat header field 4) — see [`DxccInfo::cont`].
    cont: String,
    /// `false` if the primary-prefix field is `*`-marked (WAE/CQ-only, non-DXCC).
    is_dxcc: bool,
}

struct Resolver {
    entities: Vec<Entity>,
    /// Full uppercased call → (entity index, optional per-call CQ-zone override).
    exact: HashMap<String, (u32, Option<u8>)>,
    /// Prefix → (entity index, optional per-prefix CQ-zone override).
    prefixes: HashMap<String, (u32, Option<u8>)>,
}

static CTY: &str = include_str!("../data/cty.dat");
static RESOLVER: OnceLock<Resolver> = OnceLock::new();

fn resolver() -> &'static Resolver {
    RESOLVER.get_or_init(|| parse_cty(CTY))
}

/// Parse cty.dat: header line `Name:CQ:ITU:Cont:Lat:Lon:GMT:Pfx:` sets the
/// current entity (name + lat + negated lon); indented continuation lines hold
/// the comma-separated alias list, terminated by `;`. Aliases are plain
/// prefixes or `=exact` calls, optionally carrying `(cq)`/`[itu]` zone (and
/// other bracketed) annotations which we strip.
fn parse_cty(text: &str) -> Resolver {
    let mut entities: Vec<Entity> = Vec::new();
    let mut exact: HashMap<String, (u32, Option<u8>)> = HashMap::new();
    let mut prefixes: HashMap<String, (u32, Option<u8>)> = HashMap::new();
    let mut cur: Option<u32> = None;
    let mut buf = String::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let header = !matches!(line.as_bytes()[0], b' ' | b'\t');
        if header {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() < 8 {
                continue;
            }
            let name = parts[0].trim().to_string();
            let cq_zone = parts[1].trim().parse::<u8>().unwrap_or(0);
            let cont = parts[3].trim().to_ascii_uppercase();
            let lat = parts[4].trim().parse::<f64>().unwrap_or(0.0);
            // cty.dat longitude is West-positive → negate to East-positive.
            let lon = -parts[5].trim().parse::<f64>().unwrap_or(0.0);
            // A `*` on the primary-prefix field (field 8) marks a WAE/CQ-only
            // entity: a valid CQWW/WAE contest multiplier the AD1C format says a
            // logging program "will ignore" otherwise — i.e. NOT an ARRL DXCC
            // entity (Sicily, European Turkey, African Italy, Shetland, Bear
            // Island, Vienna Intl Ctr). 346 cty.dat entities − 6 of these = the
            // 340 current DXCC entities.
            let is_dxcc = !parts[7].trim().starts_with('*');
            entities.push(Entity {
                name,
                lat,
                lon,
                cq_zone,
                cont,
                is_dxcc,
            });
            cur = Some((entities.len() - 1) as u32);
            buf.clear();
        } else if let Some(idx) = cur {
            buf.push_str(line.trim());
            if let Some(semi) = buf.find(';') {
                let aliases = buf[..semi].to_string();
                for tok in aliases.split(',') {
                    let t = tok.trim();
                    if t.is_empty() {
                        continue;
                    }
                    let (is_exact, body) = match t.strip_prefix('=') {
                        Some(s) => (true, s),
                        None => (false, t),
                    };
                    // Cut at the first annotation char: (cq) [itu] {cont} <lat/lon> ~tz~.
                    let cut = body.find(['(', '[', '{', '<', '~']).unwrap_or(body.len());
                    let key = body[..cut].trim().to_ascii_uppercase();
                    if key.is_empty() {
                        continue;
                    }
                    // Per-prefix CQ-zone override `(N)`, when present.
                    let zone = body.find('(').and_then(|p| {
                        body[p + 1..]
                            .find(')')
                            .and_then(|q| body[p + 1..p + 1 + q].trim().parse::<u8>().ok())
                    });
                    if is_exact {
                        exact.insert(key, (idx, zone));
                    } else {
                        prefixes.insert(key, (idx, zone));
                    }
                }
                buf.clear();
            }
        }
    }

    Resolver {
        entities,
        exact,
        prefixes,
    }
}

/// Strip a portable affix and pick the side that indicates the DXCC. A plain
/// operating suffix (`/P`, `/M`, `/QRP`, a digit, …) → the base call; otherwise
/// the location side is usually the shorter one (e.g. `KH8/W1AW` → `KH8`).
fn base_call(up: &str) -> &str {
    match up.split_once('/') {
        Some((a, b)) => {
            let suffix = matches!(b, "P" | "M" | "MM" | "AM" | "A" | "QRP" | "QRPP")
                || (b.len() == 1 && b.chars().all(|c| c.is_ascii_digit()));
            if suffix || a.len() <= b.len() {
                a
            } else {
                b
            }
        }
        None => up,
    }
}

/// Is `base` a Guantanamo Bay call under the 2×2 rule? (#52)
///
/// cty.dat gives Guantanamo the bare `KG4` prefix plus a list of exact calls, and a prefix that
/// broad is wrong: **only a KG4 call with exactly TWO characters after it is Guantanamo Bay.**
/// `KG4ABC` and `KG4A` are ordinary United States calls — the block is shared, and the length of
/// the suffix is what separates them. Reported by graafpeter-web against 1.0.5, and he is right:
/// every KG4 with a 3-letter suffix was being credited to Guantanamo.
///
/// cty.dat DOES enumerate the exceptions — 63 `=KG4…` exact entries, some under United States,
/// some under Alaska and Hawaii — and those still win, because they are checked before this and
/// they encode real operators whose entity the suffix length alone cannot tell you. But an
/// enumeration only covers the calls AD1C already knew: any newly issued 2×3 KG4 lands on the
/// prefix and is misfiled until the next file refresh. The rule is structural, so apply it
/// structurally.
///
/// A bare `KG4` with no suffix stays Guantanamo — that is the prefix itself, not a callsign.
fn kg4_is_guantanamo(base: &str) -> bool {
    match base.strip_prefix("KG4") {
        None => false,
        Some("") => true,
        Some(suffix) => suffix.len() == 2 && suffix.chars().all(|c| c.is_ascii_alphabetic()),
    }
}

/// Resolve a callsign to a DXCC entity + representative location.
pub fn resolve(call: &str) -> Option<DxccInfo> {
    let r = resolver();
    let full = call.trim().to_ascii_uppercase();
    if full.is_empty() {
        return None;
    }
    // Exact-call exceptions win (full call, before affix stripping).
    if let Some(&(i, zone)) = r.exact.get(&full) {
        return Some(info(r, i, zone));
    }
    // Longest-prefix on the base call.
    let base = base_call(&full);
    let mut n = base.len();
    while n > 0 {
        // The one prefix in the file that is wrong on its own: `KG4` covers a block shared with
        // the United States, and only a 2-character suffix is Guantanamo Bay (#52). Skipping the
        // entry rather than special-casing the RESULT lets the walk carry on to `K`, so the call
        // picks up the United States entity, its CQ zone and its continent by the ordinary path.
        if &base[..n] == "KG4" && !kg4_is_guantanamo(base) {
            n -= 1;
            continue;
        }
        if let Some(&(i, zone)) = r.prefixes.get(&base[..n]) {
            return Some(info(r, i, zone));
        }
        n -= 1;
    }
    None
}

/// Build a [`DxccInfo`], using the per-prefix CQ-zone override when present, else
/// the entity's default zone.
fn info(r: &'static Resolver, i: u32, zone_override: Option<u8>) -> DxccInfo {
    let e = &r.entities[i as usize];
    DxccInfo {
        entity: e.name.as_str(),
        lat: e.lat,
        lon: e.lon,
        cq_zone: zone_override.unwrap_or(e.cq_zone),
        cont: e.cont.as_str(),
        is_dxcc: e.is_dxcc,
    }
}

/// The number of **current ARRL DXCC entities** — the count of non-WAE entities
/// in cty.dat. This is the DXCC Honor Roll denominator. cty.dat carries only
/// current entities (the 62 deleted ones are absent), so this is the live
/// current total (340 in the vendored file) and updates automatically when
/// cty.dat is refreshed.
pub fn current_dxcc_entities() -> usize {
    resolver().entities.iter().filter(|e| e.is_dxcc).count()
}

/// Every **current ARRL DXCC entity** with its representative (cty.dat) location —
/// for geometric "which entities can this satellite footprint reach" queries
/// ([`crate::satneeds`]). WAE/CQ-only entities are excluded, matching
/// [`current_dxcc_entities`]. The location is the entity's single cty.dat point:
/// good for "the footprint covers this entity", approximate for continent-scale
/// entities whose edges extend far from it.
pub fn dxcc_entity_locations() -> impl Iterator<Item = (&'static str, f64, f64)> {
    resolver()
        .entities
        .iter()
        .filter(|e| e.is_dxcc)
        .map(|e| (e.name.as_str(), e.lat, e.lon))
}

/// EVERY cty.dat entity name with its representative location — the DXCC entities
/// **and** the six WAE/CQ-only ones, which is the difference from
/// [`dxcc_entity_locations`].
///
/// The `is_dxcc` filter is right for satellite-footprint geometry and wrong here.
/// [`resolve`] hands a decode row its `country` string off this same table without
/// ever consulting the flag, so an IT9 call puts "Sicily" on the screen like any
/// other country. A DXCC-only table would leave that row — and the five others —
/// showing a country with no bearing beside it while every neighbouring row had
/// one, which reads as a broken pane rather than as a deliberate exclusion.
///
/// De-duplicated by name, first occurrence winning: the consumer keys a map on the
/// name, so a repeated name would otherwise make the location depend on iteration
/// order. Sorted, like [`dxcc_entity_names`], so the wire order is stable.
pub fn entity_locations() -> Vec<(&'static str, f64, f64)> {
    let mut out: Vec<(&'static str, f64, f64)> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for e in &resolver().entities {
        if seen.insert(e.name.as_str()) {
            out.push((e.name.as_str(), e.lat, e.lon));
        }
    }
    out.sort_unstable_by_key(|(name, _, _)| *name);
    out
}

/// Every current ARRL DXCC entity NAME, sorted and de-duplicated — the source for the
/// decode panes' "hide any entity" picker (F4MQS), which opens the curated-18 country
/// exclude to the full table. The names are exactly the `country`/`entity` strings a
/// decode row carries (cty.dat's own names), so a picked name matches directly.
pub fn dxcc_entity_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = resolver()
        .entities
        .iter()
        .filter(|e| e.is_dxcc)
        .map(|e| e.name.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_names_are_the_full_sorted_deduped_set() {
        // The source for the country-hide picker's "Other country…" search (F4MQS).
        let names = dxcc_entity_names();
        assert_eq!(
            names.len(),
            current_dxcc_entities(),
            "one per current entity"
        );
        assert!(names.windows(2).all(|w| w[0] <= w[1]), "sorted");
        assert!(names.contains(&"United States"));
        assert!(names.contains(&"Fiji"));
    }

    #[test]
    fn entity_locations_carry_the_wae_entities_the_dxcc_table_drops() {
        // The UI's azimuth fallback keys a location map on the SAME string `resolve`
        // puts on screen, so this table has to answer for every entity `resolve` can
        // return — the WAE/CQ-only six included, or those rows show a country with no
        // bearing beside it while every neighbouring row has one.
        let all: HashMap<&str, (f64, f64)> = entity_locations()
            .into_iter()
            .map(|(n, lat, lon)| (n, (lat, lon)))
            .collect();
        let dxcc_only: std::collections::HashSet<&str> =
            dxcc_entity_locations().map(|(n, _, _)| n).collect();

        // Positive control for the filter itself: if the two accessors returned the
        // same set, every assertion below would pass while proving nothing.
        assert!(
            all.keys().any(|n| !dxcc_only.contains(*n)),
            "no entity is WAE-only — the is_dxcc filter is inert, so this test is vacuous"
        );
        for n in &dxcc_only {
            assert!(all.contains_key(n), "{n} missing from the unfiltered table");
        }

        // The case that motivated it, end to end: an IT9 call resolves to a WAE-only
        // entity, and that entity must have somewhere to point at.
        let sicily = resolve("IT9ABC").expect("IT9 resolves");
        assert!(!sicily.is_dxcc, "IT9 is the WAE-only case");
        assert!(
            all.contains_key(sicily.entity),
            "{} has no location",
            sicily.entity
        );
        assert!(
            !dxcc_only.contains(sicily.entity),
            "control: the DXCC-only table must omit {}",
            sicily.entity
        );
    }

    #[test]
    fn parses_the_full_list() {
        // cty.dat 2025-01-15 carries 346 entities; assert we got the full file.
        assert!(
            resolver().entities.len() >= 340,
            "entities: {}",
            resolver().entities.len()
        );
        assert!(resolver().prefixes.len() > 1000);
    }

    #[test]
    fn resolves_common_entities() {
        assert_eq!(resolve("KD9TAW").unwrap().entity, "United States");
        assert_eq!(resolve("JA1XYZ").unwrap().entity, "Japan");
        assert_eq!(resolve("C91RU").unwrap().entity, "Mozambique");
        assert_eq!(resolve("SP1ABC").unwrap().entity, "Poland");
        assert_eq!(resolve("XE1ABC").unwrap().entity, "Mexico");
        assert_eq!(resolve("SM3ABC").unwrap().entity, "Sweden");
        assert_eq!(resolve("EA4XYZ").unwrap().entity, "Spain");
        // longest-prefix: Hawaii/American Samoa beat the bare "K"/"N"/"W".
        assert_eq!(resolve("KH6ABC").unwrap().entity, "Hawaii");
        assert_eq!(resolve("KL7XX").unwrap().entity, "Alaska");
    }

    #[test]
    fn exact_call_overrides_prefix() {
        // Bouvet & Peter I have NO plain "3Y" prefix — only `=CALL` overrides.
        assert_eq!(resolve("3Y0J").unwrap().entity, "Bouvet");
        assert_eq!(resolve("3Y0X").unwrap().entity, "Peter 1 Island");
    }

    #[test]
    fn longitude_is_east_positive() {
        // Mexico is ~100°W → negative; Japan ~138°E → positive.
        assert!(resolve("XE1ABC").unwrap().lon < -90.0);
        assert!(resolve("JA1XYZ").unwrap().lon > 130.0);
    }

    #[test]
    fn handles_portable_and_unknown() {
        assert_eq!(resolve("DL1ABC/P").unwrap().entity, "Fed. Rep. of Germany");
        assert_eq!(resolve("KH8/N0CALL").unwrap().entity, "American Samoa");
        assert!(resolve("").is_none());
    }

    /// The entity names the UI's country-exclusion catalog matches on
    /// (`ui/src/features/countryExclude.ts`). That catalog stores the cty.dat
    /// entity NAME — there is no numeric DXCC id here, `DxccInfo.entity` is a
    /// `&'static str` — so a name that stops resolving does not error anywhere:
    /// the operator's ticked box silently hides nothing. A cty.dat refresh that
    /// respells one fails here, and again in countryExclude.catalog.test.ts.
    #[test]
    fn resolves_every_excludable_country() {
        for (call, entity) in [
            ("KD9TAW", "United States"),
            ("VE3ABC", "Canada"),
            ("XE1ABC", "Mexico"),
            // The operator says "Germany (DL)"; cty.dat says this.
            ("DL1ABC", "Fed. Rep. of Germany"),
            ("I1ABC", "Italy"),
            ("EA4XYZ", "Spain"),
            ("G3XYZ", "England"),
            ("F5ABC", "France"),
            ("JA1XYZ", "Japan"),
            ("PY2ABC", "Brazil"),
            ("LU1ABC", "Argentina"),
            ("SP1ABC", "Poland"),
            ("UA3ABC", "European Russia"),
            ("UR5ABC", "Ukraine"),
            ("PA0ABC", "Netherlands"),
            ("OK1ABC", "Czech Republic"),
            ("S51ABC", "Slovenia"),
            ("BY1ABC", "China"),
        ] {
            assert_eq!(resolve(call).unwrap().entity, entity, "{call}");
        }
    }

    /// Excluding a country must key on the RESOLVED entity, never on the letters
    /// a callsign starts with — the catalog's prefixes are operator vocabulary
    /// only. These are the cases a text prefix gets wrong in both directions.
    #[test]
    fn exclusion_identity_survives_portable_calls() {
        // The location side wins over the home call, and vice versa.
        assert_eq!(resolve("VE3XYZ/W1").unwrap().entity, "United States");
        assert_eq!(resolve("W1ABC/VE3").unwrap().entity, "Canada");
        // A plain operating suffix keeps the home entity.
        assert_eq!(
            resolve("DL1ABC/QRP").unwrap().entity,
            "Fed. Rep. of Germany"
        );
        assert_eq!(resolve("VE3ABC/M").unwrap().entity, "Canada");
        assert_eq!(resolve("XE1ABC/2").unwrap().entity, "Mexico");
        // Hiding "United States" must catch every US block, not just K.
        for c in ["AA1AA", "KD9TAW", "N0CALL", "W1AW", "AB7XX"] {
            assert_eq!(resolve(c).unwrap().entity, "United States", "{c}");
        }
        // …and hiding Germany must catch the whole DA–DR range, not just "DL".
        for c in ["DA1ABC", "DB2ABC", "DJ3ABC", "DK4ABC", "DL5ABC"] {
            assert_eq!(resolve(c).unwrap().entity, "Fed. Rep. of Germany", "{c}");
        }
        // A US call that merely STARTS with the letters of an excluded prefix is
        // United States, not Germany — the false positive a text match would make.
        assert_eq!(resolve("WD8L").unwrap().entity, "United States");
    }
}

#[cfg(test)]
mod zone_tests {
    use super::*;
    #[test]
    fn resolves_cq_zone() {
        // W = multi-zone (3/4/5); W1 (New England) is CQ zone 5 via prefix override.
        assert_eq!(resolve("W1AW").unwrap().cq_zone, 5, "W1 → CQ 5");
        assert_eq!(resolve("JA1XYZ").unwrap().cq_zone, 25, "Japan → CQ 25");
        assert_eq!(resolve("G3XYZ").unwrap().cq_zone, 14, "England → CQ 14");
        // every resolvable major call yields a valid zone 1..=40
        for c in ["DL1AA", "VK2AA", "PY2AA", "UA3AA", "ZL1AA"] {
            let z = resolve(c).unwrap().cq_zone;
            assert!((1..=40).contains(&z), "{c} → zone {z} out of range");
        }
    }

    #[test]
    fn marks_wae_entities_non_dxcc() {
        // Prefix-registered WAE entities shadow their DXCC parent but are NOT
        // ARRL DXCC themselves.
        assert!(
            !resolve("IT9ABC").unwrap().is_dxcc,
            "Sicily is WAE, not DXCC"
        );
        assert!(
            !resolve("TA1ABC").unwrap().is_dxcc,
            "European Turkey is WAE, not DXCC"
        );
        // Mainland / parent entities are DXCC.
        assert!(resolve("W1AW").unwrap().is_dxcc, "USA is DXCC");
        assert!(resolve("I1ABC").unwrap().is_dxcc, "Italy is DXCC");
        // African Italy keeps its own CQ zone (33) for WAZ even though it is not
        // a DXCC entity — proves we do NOT fall through to Italy's zone (15).
        let ig9 = resolve("IG9ABC").unwrap();
        assert!(!ig9.is_dxcc, "African Italy is WAE, not DXCC");
        assert_eq!(
            ig9.cq_zone, 33,
            "African Italy CQ zone is 33, not Italy's 15"
        );
        // The Honor Roll denominator = current DXCC entities (346 − 6 WAE).
        assert_eq!(
            current_dxcc_entities(),
            340,
            "current ARRL DXCC entities (matches ARRL 2026)"
        );
    }
}

#[cfg(test)]
mod kg4_suffix_rule {
    use super::{kg4_is_guantanamo, resolve};

    /// #52 (graafpeter-web, on 1.0.5): "A KG4-callsign with a 3-letter suffix is decoded as
    /// Guantanamo Bay. This is not correct. Only 2-letter suffixes after KG4 are really from
    /// Guantanamo Bay." He is right — the KG4 block is shared with the United States and the
    /// suffix length is what separates them.
    #[test]
    fn only_a_two_character_suffix_is_guantanamo() {
        for call in ["KG4AB", "KG4ZZ"] {
            assert_eq!(
                resolve(call).map(|d| d.entity),
                Some("Guantanamo Bay"),
                "{call} is a 2x2 KG4 and IS Guantanamo"
            );
        }
    }

    /// The bug itself. Every one of these resolved to Guantanamo Bay before the fix, which
    /// mis-credits the entity on the Needed board, in the log and in any award count built on it.
    #[test]
    fn a_three_character_suffix_is_the_united_states() {
        for call in ["KG4ABC", "KG4XYZ", "KG4QQQ"] {
            assert_eq!(
                resolve(call).map(|d| d.entity),
                Some("United States"),
                "{call} is an ordinary US call, not Guantanamo"
            );
        }
    }

    /// The other end of the rule, and easy to miss: a ONE-character suffix is US too.
    #[test]
    fn a_one_character_suffix_is_also_the_united_states() {
        assert_eq!(resolve("KG4A").map(|d| d.entity), Some("United States"));
    }

    /// ⚠️ cty.dat's exact-call overrides must still win. Some KG4 holders are genuinely in
    /// Alaska or Hawaii, and no suffix-length rule can know that — only the enumeration does.
    /// If this ever flips to United States, the structural rule has been applied too early.
    #[test]
    fn an_exact_override_still_beats_the_suffix_rule() {
        assert_eq!(
            resolve("KG4BBX").map(|d| d.entity),
            Some("Alaska"),
            "cty.dat names this one explicitly; the rule must not override the file"
        );
    }

    /// A bare prefix with no callsign after it is the prefix itself, not a 2x0 call.
    #[test]
    fn the_bare_prefix_stays_guantanamo() {
        assert!(kg4_is_guantanamo("KG4"));
    }

    /// Digits are not a callsign suffix of the kind this rule is about — `KG44WW` is in
    /// cty.dat's exact list, and the predicate must not claim it on shape alone.
    #[test]
    fn a_numeric_suffix_is_not_matched_on_shape() {
        assert!(!kg4_is_guantanamo("KG44W"));
        // ...and the real call still resolves via the exact override.
        assert_eq!(resolve("KG44WW").map(|d| d.entity), Some("Guantanamo Bay"));
    }

    /// Nothing outside the KG4 block is touched.
    #[test]
    fn other_entities_are_unaffected() {
        assert_eq!(resolve("KD9TAW").map(|d| d.entity), Some("United States"));
        assert_eq!(resolve("KH6ABC").map(|d| d.entity), Some("Hawaii"));
        assert_eq!(resolve("KL7ABC").map(|d| d.entity), Some("Alaska"));
        assert_eq!(resolve("G0ABC").map(|d| d.entity), Some("England"));
    }
}
