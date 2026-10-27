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
//! ⚠️ **The embedded file is the SEED, not the whole story.** A weekly-refreshed
//! copy downloaded to the shared data dir can be installed at startup via
//! [`init_from`] — but only *before* anything resolves: the `OnceLock` is set
//! once, every [`DxccInfo`] borrow is `&'static` off it, and there is no live
//! swap. The seed-floor rule (embedded wins over an invalid, truncated or
//! *older* download) lives in [`init_from`]; a resolve that beats `init_from`
//! to the lock is the ordering hazard [`CtyInitError::AlreadyInitialized`]
//! makes loud. [`active_cty_ver`]/[`embedded_ver`] tell the status surface
//! which file won.
//!
//! NB cty.dat stores **West-positive longitude**; we negate it to the usual
//! East-positive convention the rest of the crate uses.
//!
//! # DXCC entity numbers
//!
//! cty.dat names an entity; it carries no ARRL DXCC entity number, and it lists no deleted
//! entity. A logged contact's record can carry the number (ADIF `DXCC`, which LoTW and other
//! loggers write), and the awards count by it ([`entity_by_dxcc`]). Two embedded tables give the
//! numbers:
//!
//! - **`data/cty.csv`**, AD1C's file from the same release zip as the embedded `cty.dat`, kept
//!   as published. Only its first and third fields are read: each entity's primary prefix and
//!   its DXCC number. It is joined to cty.dat by that **primary prefix, never by name** (the two
//!   files spell a few names differently), so a weekly-downloaded cty.dat gets the same numbers
//!   while AD1C's prefixes hold. A WAE/CQ-only area (Sicily) carries its parent's number
//!   (Italy's), which is how ARRL credits it. An entity with no row — a newer cty.dat's new
//!   entity — has no number and counts by its name, as before.
//! - **`data/dxcc_deleted.csv`**, the 62 deleted entities, transcribed from TrustedQSL's
//!   configuration data (see that file's header and NOTICE).

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
    /// The entity's ARRL DXCC number, from the embedded cty.csv — for a WAE/CQ-only area, its
    /// parent's (Sicily carries Italy's 248), which is how ARRL credits it. `None` for an entity
    /// that table has no row for (see the module header).
    pub dxcc: Option<u32>,
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
    /// The DXCC number cty.csv gives this entity's primary prefix — see [`DxccInfo::dxcc`].
    dxcc: Option<u32>,
}

struct Resolver {
    entities: Vec<Entity>,
    /// Full uppercased call → (entity index, optional per-call CQ-zone override).
    exact: HashMap<String, (u32, Option<u8>)>,
    /// Prefix → (entity index, optional per-prefix CQ-zone override).
    prefixes: HashMap<String, (u32, Option<u8>)>,
    /// AD1C's release date, from the `=VERyyyymmdd` pseudo-call in the exact
    /// list (`=VER20250115` in the vendored file). `None` if the file carries
    /// no marker.
    ver: Option<String>,
    /// DXCC number → the index of the DXCC (non-WAE) entity carrying it, the first if two did.
    by_dxcc: HashMap<u32, u32>,
}

static CTY: &str = include_str!("../data/cty.dat");
static RESOLVER: OnceLock<Resolver> = OnceLock::new();

/// AD1C's cty.csv from the embedded cty.dat's own release, as published — read only for each
/// entity's primary prefix (field 1) and DXCC number (field 3). See the module header.
static CTY_CSV: &str = include_str!("../data/cty.csv");

/// The deleted DXCC entities, `number,name` per line after a `#` header.
static DELETED_CSV: &str = include_str!("../data/dxcc_deleted.csv");

/// ADIF's DXCC code for a station known to be in no DXCC entity (a ship at sea, an aircraft):
/// a contact whose record carries it earns no DXCC entity.
pub const NO_DXCC_ENTITY: u32 = 0;

/// cty.csv's primary prefix → DXCC number, parsed once.
fn csv_numbers() -> &'static HashMap<&'static str, u32> {
    static N: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    N.get_or_init(|| parse_csv_numbers(CTY_CSV))
}

/// The primary prefix and DXCC number of each cty.csv row. A row whose third field is not a
/// number is skipped rather than guessed at.
fn parse_csv_numbers(text: &str) -> HashMap<&str, u32> {
    text.lines()
        .filter_map(|line| {
            let mut f = line.split(',');
            let prefix = f.next()?.trim();
            let number = f.nth(1)?.trim().parse().ok()?;
            (!prefix.is_empty()).then_some((prefix, number))
        })
        .collect()
}

/// The deleted entities, number → name, parsed once.
fn deleted_entities() -> &'static HashMap<u32, &'static str> {
    static D: OnceLock<HashMap<u32, &'static str>> = OnceLock::new();
    D.get_or_init(|| {
        DELETED_CSV
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                let (number, name) = l.split_once(',')?;
                Some((number.trim().parse().ok()?, name.trim()))
            })
            .collect()
    })
}

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
    let mut ver: Option<String> = None;

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
            // The number cty.csv gives the same primary prefix, `*` and all — the join is on
            // the prefix, never the name (see the module header).
            let dxcc = csv_numbers().get(parts[7].trim()).copied();
            entities.push(Entity {
                name,
                lat,
                lon,
                cq_zone,
                cont,
                is_dxcc,
                dxcc,
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
                        // AD1C's release-date marker rides the exact list as a
                        // pseudo-call (`=VERyyyymmdd`, in Canada's block in
                        // current files). Capture it; the map entry itself is
                        // harmless — nobody's callsign is VER20250115.
                        if let Some(d) = key.strip_prefix("VER") {
                            if d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()) {
                                ver = Some(d.to_string());
                            }
                        }
                        exact.insert(key, (idx, zone));
                    } else {
                        prefixes.insert(key, (idx, zone));
                    }
                }
                buf.clear();
            }
        }
    }

    let mut by_dxcc: HashMap<u32, u32> = HashMap::new();
    for (i, e) in entities.iter().enumerate() {
        if let (true, Some(n)) = (e.is_dxcc, e.dxcc) {
            by_dxcc.entry(n).or_insert(i as u32);
        }
    }

    Resolver {
        entities,
        exact,
        prefixes,
        ver,
        by_dxcc,
    }
}

// ---------------------------------------------------------------------------
// cty.dat refresh: validation + the startup-only install seam.
// ---------------------------------------------------------------------------

/// Runtime floor for an installed (downloaded) file. Deliberately looser than the publisher
/// gate (the generator script refuses < 340): this guards against a truncated or garbage
/// download, not against entity-count drift between AD1C releases.
const MIN_ENTITIES: usize = 330;

/// What validation learned about a cty.dat text — the numbers the manifest, the meta stamp
/// and the status surface carry.
#[derive(Debug, Clone, PartialEq)]
pub struct CtyStats {
    pub entities: usize,
    pub prefixes: usize,
    /// The `=VERyyyymmdd` marker's date, e.g. `"20250115"`. Guaranteed `Some` when the text
    /// passed [`validate`]/[`init_from`] (a file with no marker is refused).
    pub ver: Option<String>,
}

/// Why [`init_from`] refused a candidate file. Each cause is distinct so the caller can log
/// what actually happened; `AlreadyInitialized` is the one that names a CODE bug (something
/// resolved a call before the startup install) rather than a bad file.
#[derive(Debug, Clone, PartialEq)]
pub enum CtyInitError {
    /// Doesn't parse to a plausible country file: failed a spot resolve, or has no
    /// `=VERyyyymmdd` marker.
    Invalid(String),
    /// Parsed, but far too few entities — a truncated or wrong file.
    TooFewEntities(usize),
    /// Valid, but older than the embedded seed (an app upgrade can bundle a newer file than
    /// an old download) — the embedded file wins.
    OlderThanEmbedded { candidate: String, embedded: String },
    /// The resolver was already set — something resolved a callsign before the startup
    /// install ran. The embedded file is locked in for this session; the ordering must be
    /// fixed in code, not retried.
    AlreadyInitialized,
}

impl std::fmt::Display for CtyInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CtyInitError::Invalid(why) => write!(f, "not a valid cty.dat: {why}"),
            CtyInitError::TooFewEntities(n) => {
                write!(f, "only {n} entities (floor {MIN_ENTITIES}) — truncated file?")
            }
            CtyInitError::OlderThanEmbedded { candidate, embedded } => write!(
                f,
                "candidate =VER{candidate} is older than the embedded =VER{embedded} — the seed wins"
            ),
            CtyInitError::AlreadyInitialized => write!(
                f,
                "resolver already initialized — something resolved a call before the startup install"
            ),
        }
    }
}

/// Entity name for `call` against a CANDIDATE resolver — the same exact-then-longest-prefix
/// walk [`resolve`] does, minus the affix/KG4 refinements the spot calls below don't need.
/// Borrow-generic on purpose: validation must interrogate the candidate, never the global.
fn spot_entity<'a>(r: &'a Resolver, call: &str) -> Option<&'a str> {
    if let Some(&(i, _)) = r.exact.get(call) {
        return Some(r.entities[i as usize].name.as_str());
    }
    for prefix in prefixes_longest_first(call) {
        if let Some(&(i, _)) = r.prefixes.get(prefix) {
            return Some(r.entities[i as usize].name.as_str());
        }
    }
    None
}

/// Parse + structural validation, against the CANDIDATE only (never the global): entity
/// floor, two spot resolves on names that will never respell, and the `=VER` marker.
///
/// NB the wins-test fixture (`tests/cty_init_wins.rs`) renames a *different* entity — the two
/// names pinned here must stay ones no fixture edits.
fn analyze(text: &str) -> Result<(Resolver, CtyStats), CtyInitError> {
    let r = parse_cty(text);
    let stats = CtyStats {
        entities: r.entities.len(),
        prefixes: r.prefixes.len(),
        ver: r.ver.clone(),
    };
    if stats.entities < MIN_ENTITIES {
        return Err(CtyInitError::TooFewEntities(stats.entities));
    }
    for (call, want) in [("W1AW", "United States"), ("JA1XYZ", "Japan")] {
        match spot_entity(&r, call) {
            Some(got) if got == want => {}
            got => {
                return Err(CtyInitError::Invalid(format!(
                    "spot resolve {call} → {got:?}, want {want:?}"
                )))
            }
        }
    }
    if stats.ver.is_none() {
        return Err(CtyInitError::Invalid("no =VERyyyymmdd marker".into()));
    }
    Ok((r, stats))
}

/// Validate a cty.dat text WITHOUT touching the global resolver — the scratch parse the
/// download client runs before installing a file to disk (a corrupt download must never
/// replace a good local copy).
pub fn validate(text: &str) -> Result<CtyStats, String> {
    analyze(text).map(|(_, s)| s).map_err(|e| e.to_string())
}

/// The embedded half of the seed-floor rule, split out so it is unit-testable without
/// setting the global. Equal is allowed on purpose: an upgrade can bundle exactly the file
/// the operator already downloaded, and rejecting it would strand a harmless install.
fn embedded_floor(candidate: &str) -> Result<(), CtyInitError> {
    if let Some(embedded) = embedded_ver() {
        // `yyyymmdd` compares correctly as a plain string.
        if *candidate < *embedded {
            return Err(CtyInitError::OlderThanEmbedded {
                candidate: candidate.to_string(),
                embedded,
            });
        }
    }
    Ok(())
}

/// Install a downloaded cty.dat as THE resolver for this process. Startup-only: call before
/// anything resolves (feed threads resolve on their first spot), because the `OnceLock` is
/// set once and every accessor's `&'static` borrow hangs off it — there is no live swap, a
/// downloaded file activates at the next launch.
///
/// Seed floor: the embedded file wins over a candidate that is unparseable
/// ([`CtyInitError::Invalid`]), truncated ([`CtyInitError::TooFewEntities`]) or older than
/// the seed ([`CtyInitError::OlderThanEmbedded`]) — on any `Err` the global is untouched and
/// the embedded file activates lazily as before. `AlreadyInitialized` means something
/// resolved first; the CALLER must log it loudly (it is a code-ordering regression).
pub fn init_from(text: &str) -> Result<CtyStats, CtyInitError> {
    let (r, stats) = analyze(text)?;
    let candidate = stats.ver.clone().expect("analyze guarantees a ver");
    embedded_floor(&candidate)?;
    if RESOLVER.set(r).is_err() {
        return Err(CtyInitError::AlreadyInitialized);
    }
    Ok(stats)
}

/// The EMBEDDED file's `=VER` date — the floor [`init_from`] compares against, and the
/// status surface's "what would we fall back to". A lightweight scan (no full parse),
/// cached; never touches the global resolver.
pub fn embedded_ver() -> Option<String> {
    static V: OnceLock<Option<String>> = OnceLock::new();
    V.get_or_init(|| ver_marker(CTY)).clone()
}

/// First `=VERyyyymmdd` token in `text`, without a full parse. Scans every `=VER` hit
/// because AD1C also ships a literal `=VERSION` pseudo-call, which is not the marker.
fn ver_marker(text: &str) -> Option<String> {
    for (i, _) in text.match_indices("=VER") {
        let digits: String = text[i + 4..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if digits.len() == 8 {
            return Some(digits);
        }
    }
    None
}

/// The ACTIVE file's `=VER` date — whichever file won at startup. NB this initializes the
/// resolver (with the embedded seed) if nothing has yet, exactly like every other accessor.
pub fn active_cty_ver() -> Option<String> {
    resolver().ver.clone()
}

/// Entity/prefix counts + ver of the ACTIVE file, for the status surface. Same
/// initialize-if-needed caveat as [`active_cty_ver`].
pub fn active_stats() -> CtyStats {
    let r = resolver();
    CtyStats {
        entities: r.entities.len(),
        prefixes: r.prefixes.len(),
        ver: r.ver.clone(),
    }
}

/// Strip a portable affix and pick the side that indicates the DXCC. A plain
/// operating suffix (`/P`, `/M`, `/QRP`, a digit, …) → the base call; otherwise
/// the location side is usually the shorter one (e.g. `KH8/W1AW` → `KH8`).
pub(crate) fn base_call(up: &str) -> &str {
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

/// Every prefix of `s`, longest first — the candidates of a longest-prefix walk. Each ends on a
/// character boundary, so the walk cannot cut a multi-byte character in half, which counting the
/// length down one BYTE at a time (`&s[..n]`) does on the first one it meets.
fn prefixes_longest_first(s: &str) -> impl Iterator<Item = &str> {
    s.char_indices()
        .rev()
        .map(move |(i, c)| &s[..i + c.len_utf8()])
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
    let full = spelled(call);
    if full.is_empty() {
        return None;
    }
    // Exact-call exceptions win (full call, before affix stripping).
    if let Some(&(i, zone)) = r.exact.get(&full) {
        return Some(info(r, i, zone));
    }
    // Longest-prefix on the base call.
    let prefix = matched_prefix(r, base_call(&full))?;
    let &(i, zone) = r.prefixes.get(prefix)?;
    Some(info(r, i, zone))
}

/// Whether the country file can call a contact logged under `call` with the DXCC number `logged`
/// misfiled ([`resolve`] places the call elsewhere). Never when `call` is one of the file's
/// exact-call exceptions — a DXpedition or special operation, whose entry is the file's whole
/// knowledge of it — and never when the logged entity has an exception or prefix of its own that
/// begins with the prefix the call resolved through. `3Y0PI` (Peter I, 1994) has no exact entry
/// any more and resolves through Antarctica's bare `3Y`; Peter I's and Bouvet's calls are `3Y`
/// calls too, so a 3Y call logged as either is no evidence of a mistake. AD1C drops an operation's
/// exact entry once it is over, so the file cannot say which past operation a call was.
pub fn prefix_decides(call: &str, logged: u32) -> bool {
    let r = resolver();
    let full = spelled(call);
    if full.is_empty() || r.exact.contains_key(&full) {
        return false;
    }
    let Some(prefix) = matched_prefix(r, base_call(&full)) else {
        return false;
    };
    let of_logged = |&(i, _): &(u32, Option<u8>)| r.entities[i as usize].dxcc == Some(logged);
    // An exception by the call it is under its affix: `=W0SD/VE2` is a Canadian `VE2` call, no
    // sign that a `W` call may be Canadian.
    let shares = r
        .exact
        .iter()
        .any(|(call, at)| base_call(call).starts_with(prefix) && of_logged(at))
        || r.prefixes
            .iter()
            .any(|(key, at)| key.starts_with(prefix) && of_logged(at));
    !shares
}

/// A call as the country file is asked about it: up to its first non-ASCII character, out of its
/// hashed-call brackets, trimmed and upper-cased. Empty when nothing of a call is left.
fn spelled(call: &str) -> String {
    // ⚠️ A NON-ASCII CHARACTER ENDS THE CALL. A callsign is ASCII by definition, but what
    // reaches here is outside input — POTA spots, cluster lines, decodes, imported logs — and
    // the POTA feed carried `KK4JAB ☕️` (2026-09-23): the call, then decoration. The prefix
    // walk below already answers a call with trailing ASCII junk by its longest real prefix
    // (`KK4JAB XYZ` is United States), and the #84 bracket strip below reads the call inside
    // its decoration rather than refusing it; this is the same reading. Every all-ASCII input
    // resolves exactly as before, and nothing ahead of the first non-ASCII character means no
    // call at all (`☕️KK4JAB`, full-width letters → `None`).
    let call = call
        .find(|c: char| !c.is_ascii())
        .map_or(call, |i| &call[..i]);
    // ⚠️ STRIP THE HASHED-CALL BRACKETS FIRST (issue #84). FT8's 77-bit protocol sends a
    // compound call as `<DX1ABC>` when the message will not otherwise fit, and the decoded
    // token keeps its brackets all the way here. `cty.dat` contains no `<` anywhere, so a
    // bracketed call misses the exact map AND every prefix walk below and resolves to nothing
    // — a worked DXpedition scoring as neither a new country nor a new band, which is exactly
    // what the reporter saw. Stripping here rather than at each call site because this is the
    // boundary: everything past this line is prefix arithmetic that assumes a bare call.
    call.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_ascii_uppercase()
}

/// The longest prefix of `base` the country file holds: the one [`resolve`] places a call by.
fn matched_prefix<'a>(r: &Resolver, base: &'a str) -> Option<&'a str> {
    for prefix in prefixes_longest_first(base) {
        // The one prefix in the file that is wrong on its own: `KG4` covers a block shared with
        // the United States, and only a 2-character suffix is Guantanamo Bay (#52). Skipping the
        // entry rather than special-casing the RESULT lets the walk carry on to `K`, so the call
        // picks up the United States entity, its CQ zone and its continent by the ordinary path.
        if prefix == "KG4" && !kg4_is_guantanamo(base) {
            continue;
        }
        if r.prefixes.contains_key(prefix) {
            return Some(prefix);
        }
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
        dxcc: e.dxcc,
    }
}

/// A DXCC entity named by its ARRL number, as a contact's record carries it (ADIF `DXCC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DxccEntity {
    pub code: u32,
    /// A current entity's cty.dat name — the name [`resolve`] gives its calls, so a contact
    /// counted by number and one counted by call land on the same entity — or a deleted entity's
    /// name as the ARRL lists it.
    pub name: &'static str,
    /// One of the 62 deleted entities, which count only toward a "total" (never toward a current
    /// count, Honor Roll or the Challenge).
    pub deleted: bool,
}

/// The DXCC entity `code` names: a current entity through cty.csv's numbers, or a deleted one.
/// `None` for [`NO_DXCC_ENTITY`] and for a number neither table holds — a garbled one, or one
/// the ARRL added after this table — so the caller can fall back to the call.
pub fn entity_by_dxcc(code: u32) -> Option<DxccEntity> {
    let r = resolver();
    if let Some(&i) = r.by_dxcc.get(&code) {
        return Some(DxccEntity {
            code,
            name: r.entities[i as usize].name.as_str(),
            deleted: false,
        });
    }
    deleted_entities().get(&code).map(|&name| DxccEntity {
        code,
        name,
        deleted: true,
    })
}

/// Is `entity` one of the DXCC entities whose stations carry a **US state** as their ADIF
/// primary administrative subdivision — the WAS family, United States + Alaska + Hawaii?
///
/// The group is ENUMERATED rather than derived from "does it fly the US flag", because the
/// other US-flagged entities (Guam, Puerto Rico, the Virgin Islands, the Marianas…) are DXCC
/// entities in their own right and have no state. Same three names
/// `tempo_core::diagnostics` groups a logged QSO by, deliberately.
pub fn is_us_state_entity(entity: &str) -> bool {
    matches!(entity, "United States" | "Alaska" | "Hawaii")
}

/// [`is_us_state_entity`] by DXCC number — the United States (291), Alaska (6) and Hawaii (110) —
/// for a contact whose record carries the number.
pub fn is_us_state_dxcc(code: u32) -> bool {
    matches!(code, 291 | 6 | 110)
}

/// Every DXCC entity a contact's record can name, by number: the current ones under their cty.dat
/// names and the deleted ones under the ARRL's, in number order — what the Logbook offers when
/// the operator sets the entity a contact counts toward by hand. One per number, as
/// [`entity_by_dxcc`] names it, so a WAE area (which carries its parent's number) is not offered
/// as an entity of its own.
pub fn entities() -> Vec<DxccEntity> {
    let current = resolver().by_dxcc.keys();
    let mut all: Vec<DxccEntity> = current
        .chain(deleted_entities().keys())
        .filter_map(|&code| entity_by_dxcc(code))
        .collect();
    all.sort_unstable_by_key(|e| e.code);
    all
}

/// The entity a logged contact is SHOWN under — the Logbook's Country column and its sort (the
/// callsign card's is [`card_entity`]): the entity its record's own DXCC number names,
/// wherever it names one ([`country_by_dxcc`], the name its `COUNTRY` takes, so the column and
/// `COUNTRY` never differ); else its call's (`resolved`).
///
/// The number is followed strictly. A 1×1 call like K1N logged as Navassa, a deleted entity and a
/// number the operator set by hand are shown as the number names them, and so is a WAE area's
/// contact carrying its parent's number: Sicily's with Italy's 248 is shown as Italy. Without a
/// number a WAE area's contact is shown as its call places it (Sicily). A number that names no
/// entity (0, or one newer than the table) is passed over. LoTW's number, kept beside the log's
/// where the two differ, is not the log's own: it is shown beside it, never in its place.
///
/// Not the entity a contact COUNTS toward ([`crate::awards::counted_entity`]): the awards take
/// LoTW's number first, and a WAE area as its parent.
pub fn shown_entity(resolved: Option<&DxccInfo>, dxcc: Option<u32>) -> Option<&'static str> {
    dxcc.and_then(country_by_dxcc)
        .or_else(|| resolved.map(|i| i.entity))
}

/// The country (ADIF `COUNTRY`) a contact's record takes from its own DXCC number `code`, so the
/// two never disagree: the name of the DXCC entity the number names ([`entity_by_dxcc`]), as the
/// Logbook's Country column shows it ([`shown_entity`]). Strictly the number's: a WAE area is no
/// DXCC entity, so a Sicily contact carrying Italy's 248 is Italy here, as in ADIF.
///
/// `None` for a number that names no entity (0, or one newer than the table): there is nothing to
/// follow, and `COUNTRY` is whatever else names it — the call's entity on an edit or a fill, the
/// record's own text in a file.
pub fn country_by_dxcc(code: u32) -> Option<&'static str> {
    entity_by_dxcc(code).map(|e| e.name)
}

/// The DXCC number of the entity a contact's `COUNTRY` text names, read against the names of the
/// current entities, as cty.dat gives them (a WAE area's carries its parent's number, as in ADIF),
/// case and spacing aside. So "fed. rep. of  GERMANY" is 230 and "Sicily" 248.
///
/// `None` where the text names nothing the table holds — another program's spelling, "Germany"
/// for the Fed. Rep. of Germany — and where it cannot say which entity: a name entities of
/// different numbers share, or one whose entity has no number. A caller comparing the text with
/// a contact's number reads `None` as "cannot tell", never as "differs". The deleted entities'
/// names are not read: the ARRL's are the plain ones today's programs write for a current entity
/// ("GERMANY", 81; "OKINAWA", 193), so they cannot say which of the two a text means.
pub fn dxcc_by_name(text: &str) -> Option<u32> {
    let folded = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let want = folded(text);
    if want.is_empty() {
        return None;
    }
    let mut found = None;
    for e in &resolver().entities {
        if folded(&e.name) != want {
            continue;
        }
        let code = e.dxcc?;
        if found.is_some_and(|f| f != code) {
            return None;
        }
        found = Some(code);
    }
    found
}

/// The entity a contact is under on the callsign card: [`shown_entity`] for the contact as it
/// would be with its DXCC number — its record's own where that names an entity, else the number its
/// call carries. A typed call has no record (`dxcc` is `None`) and asks its card for this, and an
/// entity's worked slots keep each logged contact under it, so the two always meet.
///
/// It differs from the Country column only for a WAE area's contact without a number, which the
/// column shows as its call places it (Sicily) and the card counts as its DXCC entity (Italy, from
/// the 248 the call carries). So IT9XYZ's card and I1ABC's both ask for Italy, and both find a
/// Sicily contact carrying 248 (an import, or a number a LoTW confirmation filled in) and one
/// logged in Nexus, which carries none. A call outside the WAE areas is its own entity, as it
/// always was: each DXCC entity's number names it.
pub fn card_entity(resolved: Option<&DxccInfo>, dxcc: Option<u32>) -> Option<&'static str> {
    dxcc.and_then(country_by_dxcc)
        .or_else(|| shown_entity(resolved, resolved.and_then(|i| i.dxcc)))
}

/// The DXCC entity a WAE/CQ-only area belongs to, by the number it carries (Sicily → Italy,
/// European Turkey → Turkey); `None` for every other name, a DXCC entity's included. Read by
/// the contest scorer, which keeps a row's entity as [`resolve`] named it and counts a WAE area
/// as a country of its own only in the contests whose list has them (CQ WW, WAE DX).
pub fn wae_parent(entity: &str) -> Option<&'static str> {
    // Six areas in the vendored file: the scan is over those, never over every entity.
    static AREAS: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    AREAS
        .get_or_init(|| {
            let r = resolver();
            r.entities
                .iter()
                .filter(|e| !e.is_dxcc)
                .filter_map(|e| {
                    let parent = r.by_dxcc.get(&e.dxcc?)?;
                    Some((e.name.as_str(), r.entities[*parent as usize].name.as_str()))
                })
                .collect()
        })
        .iter()
        .find(|(area, _)| *area == entity)
        .map(|(_, parent)| *parent)
}

/// The subdivision an ENTITY settles on its own, because the entity IS one state (#171).
///
/// Alaska and Hawaii are single-state DXCC entities: a call the prefix places there is in AK
/// or HI, whatever any other resolver says — and one other resolver says otherwise. The FCC
/// ULS index answers with the licensee's **mailing address**, so WL7E came out country
/// "Alaska", state "CA". The prefix knows where the station is; a mailing address does not,
/// and nothing was comparing the two.
///
/// `None` for every other entity, including the United States — a state there is a real
/// per-licensee fact this cannot supply.
pub fn state_for_entity(entity: &str) -> Option<&'static str> {
    match entity {
        "Alaska" => Some("AK"),
        "Hawaii" => Some("HI"),
        _ => None,
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
/// Every cty.dat entity name with its continent code — the table band activity's "hide by
/// continent" filter (#229) maps a decode row's `country` through. Covers every name [`resolve`]
/// can return (the WAE/CQ-only entities too, like [`entity_locations`]), de-duplicated by name
/// with the first occurrence winning, and sorted so the wire order is stable. An entity whose
/// row carries no continent is left out rather than given a guessed one.
pub fn entity_continents() -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for e in &resolver().entities {
        if seen.insert(e.name.as_str()) && !e.cont.is_empty() {
            out.push((e.name.as_str(), e.cont.as_str()));
        }
    }
    out.sort_unstable_by_key(|(name, _)| *name);
    out
}

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

    /// A `COUNTRY` text names the current entity whose name it is, case and spacing aside — a WAE
    /// area under its parent's number — and nothing for a text the table does not hold. A deleted
    /// entity's name is not read: "Germany" is deleted 81's, and a QRZ "Germany" means 230.
    #[test]
    fn a_country_text_names_the_entity_the_table_calls_by_it() {
        for (text, code) in [
            ("Fed. Rep. of Germany", Some(230)),
            ("  fed. rep. of   GERMANY ", Some(230)),
            ("United States", Some(291)),
            ("NAVASSA ISLAND", Some(182)),
            ("Sicily", Some(248)),
            ("Germany", None),
            ("UNITED STATES OF AMERICA", None),
            ("", None),
            ("   ", None),
        ] {
            assert_eq!(dxcc_by_name(text), code, "{text:?}");
        }
        assert_eq!(
            country_by_dxcc(81),
            Some("GERMANY"),
            "premise: deleted 81's name"
        );
        for deleted in ["GERMANY", "Okinawa", "CANAL ZONE"] {
            assert_eq!(dxcc_by_name(deleted), None, "{deleted}");
        }
        assert_eq!(dxcc_by_name("Palestine"), Some(510), "the current entity");
    }

    /// The file judges an ordinary call by its prefix, and never a DXpedition's: an exact-call
    /// exception, or a call whose prefix the logged entity's own calls share (`3Y0PI` falls to
    /// Antarctica's bare `3Y`, and Peter I's and Bouvet's calls are `3Y` calls too).
    #[test]
    fn a_dxpedition_call_is_not_judged_by_its_prefix() {
        assert!(prefix_decides("W1AW", 1), "a US call logged as Canada");
        assert!(
            prefix_decides("3Y0PI", 291),
            "a 3Y call logged as the United States"
        );
        assert!(!prefix_decides("3Y0PI", 199), "Peter I");
        assert!(!prefix_decides("3Y5X", 24), "Bouvet");
        assert!(!prefix_decides("3Y0J", 199), "an exact-call exception");
        // An exception is never judged, even against an entity sharing nothing with it: the
        // file's entry is all it knows of the operation (=OP0LE is an Antarctic station).
        assert!(
            prefix_decides("OP4LE", 339),
            "premise: an ordinary OP call is judged"
        );
        assert!(!prefix_decides("OP0LE", 339), "an exact-call exception");
        assert!(!prefix_decides("", 1));
    }

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

    /// #229 (pa0kgb): band activity's hide-by-continent maps a decode row's `country` — the
    /// entity name `resolve` puts on it — to a continent through this table. So it must answer
    /// for EVERY name `resolve` can return (the WAE/CQ-only entities included, as
    /// `entity_locations` does) and with the very continent `resolve` reports.
    #[test]
    fn entity_continents_answer_for_every_resolvable_entity_with_resolves_own_continent() {
        let table: HashMap<&str, &str> = entity_continents().into_iter().collect();
        assert_eq!(
            table.len(),
            entity_locations().len(),
            "one row per cty.dat entity name"
        );
        for call in [
            "W1AW", "DL1ABC", "JA1ABC", "VK2ABC", "PY2ABC", "ZS6ABC", "IT9ABC",
        ] {
            let info = resolve(call).expect(call);
            assert_eq!(
                table.get(info.entity).copied(),
                Some(info.cont),
                "{call} → {}",
                info.entity
            );
        }
        assert!(
            table
                .values()
                .all(|c| ["AF", "AS", "EU", "NA", "OC", "SA"].contains(c)),
            "every continent is one of cty.dat's six codes"
        );
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

    /// #171: the entity is the only resolver that knows where a station IS. WL7E was reported
    /// showing country "Alaska" and state "CA" — the state came from the FCC index, which holds
    /// the licensee's mailing address. Alaska and Hawaii settle their own subdivision, and
    /// everything outside the WAS family has none to settle.
    #[test]
    fn single_state_entities_settle_their_own_subdivision() {
        let ent = |c: &str| resolve(c).unwrap().entity;
        assert_eq!(state_for_entity(ent("WL7E")), Some("AK"));
        assert_eq!(state_for_entity(ent("KL7XX")), Some("AK"));
        assert_eq!(state_for_entity(ent("KH6ABC")), Some("HI"));
        // The lower 48 has 50 answers this cannot give — that is the FCC index's job.
        assert_eq!(state_for_entity(ent("KD9TAW")), None);

        assert!(is_us_state_entity(ent("KD9TAW")));
        assert!(is_us_state_entity(ent("WL7E")));
        assert!(is_us_state_entity(ent("KH6ABC")));
        // US-flagged, but DXCC entities of their own with no state.
        assert!(!is_us_state_entity(ent("KP4ABC")));
        // Guam by prefix. (`KH2AB` would NOT do here: cty.dat carries it as an exact-call
        // exception filed under the United States, which is the whole reason the entity — not
        // the prefix — is what this asks.)
        assert!(!is_us_state_entity(ent("KH2XYZ")));
        assert!(!is_us_state_entity(ent("VE3ABC")));
        assert!(!is_us_state_entity(ent("XE1ABC")));
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
mod cty_refresh_tests {
    //! The seed-floor rules, unit-tested WITHOUT [`init_from`]: `analyze`/`embedded_floor`
    //! build throwaway resolvers and read only `embedded_ver`'s private cache, so nothing
    //! here can poison the global `RESOLVER` other tests in this process depend on. The
    //! install path itself (set + win + the AlreadyInitialized hazard) lives in
    //! `tests/cty_init_wins.rs` / `tests/cty_init_too_late.rs`, each its own process.
    use super::*;

    /// Positive control for the whole module: the embedded file itself must pass every
    /// check, and the lightweight `ver_marker` scan must agree with the parser's capture.
    #[test]
    fn the_embedded_file_passes_validation() {
        let stats = validate(CTY).expect("the vendored cty.dat must validate");
        assert!(stats.entities >= 340, "entities: {}", stats.entities);
        assert!(stats.prefixes > 1000, "prefixes: {}", stats.prefixes);
        assert_eq!(stats.ver.as_deref(), Some("20250115"));
        assert_eq!(
            embedded_ver(),
            stats.ver,
            "ver_marker's scan and parse_cty's capture must agree on the same file"
        );
    }

    #[test]
    fn a_truncated_file_is_rejected_with_the_count_it_found() {
        let short: String = CTY.lines().take(200).collect::<Vec<_>>().join("\n");
        match analyze(&short) {
            Err(CtyInitError::TooFewEntities(n)) => {
                assert!(n > 0 && n < MIN_ENTITIES, "found {n}")
            }
            other => panic!("expected TooFewEntities, got {:?}", other.map(|(_, s)| s)),
        }
        // Garbage parses to zero entities and lands on the same floor.
        assert_eq!(
            analyze("hello world\n").map(|(_, s)| s),
            Err(CtyInitError::TooFewEntities(0))
        );
    }

    /// The spot resolves interrogate the CANDIDATE: a full-size file whose United States
    /// entry respelled must be refused, proving the check reads the new text rather than
    /// the global.
    #[test]
    fn a_wrong_spot_resolve_is_rejected() {
        let bad = CTY.replace("United States:", "Untied States:");
        assert_ne!(bad, CTY, "fixture edit missed");
        match analyze(&bad) {
            Err(CtyInitError::Invalid(why)) => assert!(why.contains("W1AW"), "{why}"),
            other => panic!("expected Invalid, got {:?}", other.map(|(_, s)| s)),
        }
    }

    #[test]
    fn a_file_without_a_ver_marker_is_rejected() {
        let bare = CTY.replace("=VER20250115,", "");
        assert_ne!(bare, CTY, "fixture edit missed");
        match analyze(&bare) {
            Err(CtyInitError::Invalid(why)) => assert!(why.contains("=VER"), "{why}"),
            other => panic!("expected Invalid, got {:?}", other.map(|(_, s)| s)),
        }
    }

    /// The embedded-wins half: older is refused, equal and newer pass (an app upgrade can
    /// bundle exactly the file the operator already downloaded).
    #[test]
    fn older_than_the_embedded_ver_is_rejected_equal_and_newer_pass() {
        let embedded = embedded_ver().expect("embedded file carries =VER");
        assert_eq!(
            embedded_floor("20200101"),
            Err(CtyInitError::OlderThanEmbedded {
                candidate: "20200101".into(),
                embedded: embedded.clone(),
            })
        );
        assert_eq!(embedded_floor(&embedded), Ok(()), "equal must pass");
        assert_eq!(embedded_floor("20991231"), Ok(()), "newer must pass");

        // End to end on a real full-size text: only the ver edited, everything else valid.
        let older = CTY.replace("=VER20250115", "=VER20200101");
        let (_, stats) = analyze(&older).expect("still a valid file structurally");
        assert_eq!(stats.ver.as_deref(), Some("20200101"));
        assert!(matches!(
            embedded_floor(stats.ver.as_deref().unwrap()),
            Err(CtyInitError::OlderThanEmbedded { .. })
        ));
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

#[cfg(test)]
mod hashed_call_tests {
    use super::*;

    /// A HASHED (bracketed) CALL MUST RESOLVE — issue #84.
    ///
    /// FT8's 77-bit protocol sends a compound call as `<DX1ABC>` when the message will not
    /// otherwise fit, and the decoded token keeps its brackets. `cty.dat` contains no `<`
    /// anywhere, so a bracketed call misses the exact map AND every prefix walk, and comes back
    /// as no entity at all. From the operator's chair that is a worked DXpedition scoring as
    /// neither a new country nor a new band — the reporter's "any assessment (NEW, GRID) failed".
    ///
    /// `tempo_core::message::base_call` strips brackets; this module's own `base_call` splits
    /// only on '/', and nothing upstream of it strips them either.
    #[test]
    fn a_bracketed_hashed_call_resolves_to_the_same_entity_as_its_plain_form() {
        for call in ["W1AW", "G0ABC", "JA1XYZ"] {
            let plain = resolve(call).map(|d| d.entity);
            assert!(plain.is_some(), "control: {call} must resolve at all");
            let hashed = resolve(&format!("<{call}>")).map(|d| d.entity);
            assert_eq!(
                hashed, plain,
                "<{call}> must resolve exactly as {call} does — cty.dat holds no '<', so a \
                 bracketed call otherwise scores as no country and no award"
            );
        }
    }

    /// The compound forms are the whole reason brackets exist on the wire.
    #[test]
    fn a_bracketed_compound_call_still_picks_the_location_side() {
        let plain = resolve("KH8/W1AW").map(|d| d.entity);
        assert!(plain.is_some(), "control: the compound form resolves");
        assert_eq!(resolve("<KH8/W1AW>").map(|d| d.entity), plain);
    }
}

#[cfg(test)]
mod non_ascii_calls {
    use super::*;

    /// THE LIVE CASE, 2026-09-23: the POTA feed carried `"activator": "KK4JAB ☕️"` (US-7615,
    /// 14236.5). The longest-prefix walk sliced the call one BYTE at a time, landed inside the
    /// emoji's variation selector and panicked ("byte index 12 is not a char boundary") on every
    /// Needed-board read while the spot was up. The station is KK4JAB, at a US park.
    #[test]
    fn the_live_pota_activator_resolves_to_its_call() {
        assert_eq!(
            resolve("KK4JAB ☕️").map(|d| d.entity),
            Some("United States")
        );
    }

    /// The rule: a callsign is ASCII, so its first non-ASCII character ends it, and whatever
    /// follows cannot change the answer. Each plain call is paired with its own answer, so a
    /// tail that changed it would show — including the two answers a walk over the whole string
    /// gets wrong even where it does not panic: an exact-call override (KG4BBX is Alaska) and
    /// the KG4 2×2 rule (KG4AB is Guantanamo), both of which read the call as a whole.
    #[test]
    fn a_non_ascii_tail_never_changes_the_answer() {
        let tails = [
            " ☕️",
            "☕",
            "é",
            "\u{301}",
            "Ø",
            "\u{a0}",
            "ＡＢ",
            "\u{3000}73",
        ];
        for call in [
            "KK4JAB", "W1AW", "JA1XYZ", "KG4BBX", "KG4AB", "KH8/W1AW", "DL1ABC/P",
        ] {
            let plain = resolve(call).map(|d| d.entity);
            assert!(plain.is_some(), "control: {call} must resolve on its own");
            for tail in tails {
                assert_eq!(
                    resolve(&format!("{call}{tail}")).map(|d| d.entity),
                    plain,
                    "{call:?} + {tail:?}"
                );
            }
        }
        // The hashed-call brackets (#84) still come off when decoration follows them.
        assert_eq!(
            resolve("<KK4JAB>☕").map(|d| d.entity),
            Some("United States")
        );
    }

    /// Nothing callsign-shaped in front of the first non-ASCII character means no call at all:
    /// the same `None` a call that starts with junk has always had.
    #[test]
    fn no_leading_call_resolves_to_nothing() {
        for s in [
            "☕️KK4JAB",       // emoji first
            "ＫＫ４ＪＡＢ",   // full-width letters: callsign-shaped, not callsign characters
            "é",              // a lone two-byte character
            "☕",             // a lone three-byte character
            "𝐊𝐊4𝐉𝐀𝐁",         // four-byte letters
            "\u{feff}KD9TAW", // a byte-order mark from a spreadsheet export
            "\u{301}",        // a lone combining mark
        ] {
            assert_eq!(resolve(s).map(|d| d.entity), None, "{s:?}");
        }
    }

    /// The walk itself, independent of the cut above: every candidate ends on a character
    /// boundary, longest first. This is what keeps a non-ASCII key in a downloaded cty.dat, or
    /// any future caller of the walk, from ever splitting a character.
    #[test]
    fn the_prefix_walk_never_splits_a_character() {
        assert_eq!(
            prefixes_longest_first("K☕é").collect::<Vec<_>>(),
            ["K☕é", "K☕", "K"]
        );
        assert_eq!(
            prefixes_longest_first("W1AW").collect::<Vec<_>>(),
            ["W1AW", "W1A", "W1", "W"]
        );
        assert_eq!(prefixes_longest_first("").count(), 0);
    }
}

#[cfg(test)]
mod dxcc_numbers {
    use super::*;

    /// cty.csv and cty.dat are one release's pair, so the join is exact: the same `=VER` marker,
    /// every embedded cty.dat entity numbered through its primary prefix (346 of 346), and one
    /// number per DXCC entity.
    #[test]
    fn the_embedded_pair_is_one_release_and_numbers_every_entity() {
        assert_eq!(
            ver_marker(CTY_CSV),
            embedded_ver(),
            "cty.csv must come from the embedded cty.dat's own release"
        );
        let r = resolver();
        assert_eq!(r.entities.len(), 346, "premise: the 2025-01-15 file");
        let missing: Vec<&str> = r
            .entities
            .iter()
            .filter(|e| e.dxcc.is_none())
            .map(|e| e.name.as_str())
            .collect();
        assert!(
            missing.is_empty(),
            "entities with no DXCC number: {missing:?}"
        );
        assert_eq!(
            r.by_dxcc.len(),
            current_dxcc_entities(),
            "one distinct number per current DXCC entity"
        );
        assert_eq!(r.by_dxcc.len(), 340);
    }

    /// A WAE/CQ-only area carries its parent's number — which is how ARRL credits it — while its
    /// own name and CQ zone stay what cty.dat says.
    #[test]
    fn a_wae_area_carries_its_parents_number() {
        for (wae, parent, code) in [
            ("IT9ABC", "I1ABC", 248),
            ("IG9ABC", "I1ABC", 248),
            ("TA1ABC", "TA2ABC", 390),
        ] {
            let area = resolve(wae).unwrap();
            assert!(!area.is_dxcc, "premise: {wae} is a WAE area");
            assert_eq!(area.dxcc, Some(code), "{wae}");
            assert_eq!(resolve(parent).unwrap().dxcc, Some(code), "{parent}");
            let named = entity_by_dxcc(code).unwrap();
            assert_eq!(named.name, resolve(parent).unwrap().entity, "{code}");
            assert!(!named.deleted);
        }
    }

    /// Every WAE area the file holds folds into the DXCC entity its own number names, and a DXCC
    /// entity folds into nothing — the contest scorer keeps it as it is.
    #[test]
    fn a_wae_area_folds_into_its_dxcc_entity_and_nothing_else_folds() {
        let areas: Vec<_> = resolver()
            .entities
            .iter()
            .filter(|e| !e.is_dxcc)
            .map(|e| (e.name.as_str(), wae_parent(&e.name)))
            .collect();
        assert_eq!(
            areas,
            [
                ("Vienna Intl Ctr", Some("Austria")),
                ("Shetland Islands", Some("Scotland")),
                ("African Italy", Some("Italy")),
                ("Sicily", Some("Italy")),
                ("Bear Island", Some("Svalbard")),
                ("European Turkey", Some("Asiatic Turkey")),
            ]
        );
        for dxcc in ["Italy", "Austria", "United States", "Fed. Rep. of Germany"] {
            assert_eq!(wae_parent(dxcc), None, "{dxcc}");
        }
        assert_eq!(wae_parent(""), None);
    }

    /// Each number the reports and the awards' tests rest on, checked against the table: it names
    /// the entity whose own calls resolve to it. (Taken from cty.csv and TrustedQSL, never from
    /// memory: Navassa is 182, and 79 is Guadeloupe.)
    #[test]
    fn numbers_name_the_entities_their_own_calls_resolve_to() {
        for (code, call, name) in [
            (291, "W1AW", "United States"),
            (6, "KL7XX", "Alaska"),
            (110, "KH6ABC", "Hawaii"),
            (202, "KP4ABC", "Puerto Rico"),
            (20, "KH1ABC", "Baker & Howland Islands"),
            (182, "KP1AA", "Navassa Island"),
            (123, "KH3ABC", "Johnston Island"),
            (43, "KP5AA", "Desecheo Island"),
            (174, "KH4XYZ", "Midway Island"),
            (197, "KH5AA", "Palmyra & Jarvis Islands"),
            (515, "KH8SI", "Swains Island"),
            (79, "FG5ABC", "Guadeloupe"),
            (248, "I1ABC", "Italy"),
            (230, "DL1ABC", "Fed. Rep. of Germany"),
        ] {
            assert_eq!(
                entity_by_dxcc(code).map(|e| (e.name, e.deleted)),
                Some((name, false)),
                "{code}"
            );
            assert_eq!(resolve(call).and_then(|i| i.dxcc), Some(code), "{call}");
        }
        for code in [291, 6, 110] {
            assert!(is_us_state_dxcc(code));
            assert!(is_us_state_entity(entity_by_dxcc(code).unwrap().name));
        }
        assert!(!is_us_state_dxcc(202), "Puerto Rico is no WAS state");
    }

    /// The 62 deleted entities: none is a current number, and with the 340 current ones they are
    /// the ARRL's 402. A name keeps its commas and its XML escapes come out read.
    #[test]
    fn the_deleted_entities_are_62_and_never_a_current_number() {
        let deleted = deleted_entities();
        assert_eq!(deleted.len(), 62);
        let current = &resolver().by_dxcc;
        let both: Vec<&u32> = deleted.keys().filter(|n| current.contains_key(n)).collect();
        assert!(both.is_empty(), "deleted and current at once: {both:?}");
        assert_eq!(current.len() + deleted.len(), 402);
        for (code, name) in [
            (229, "GERMAN DEMOCRATIC REPUBLIC"),
            (134, "KINGMAN REEF"),
            (85, "BONAIRE,CURACAO (NETH ANTILLES)"),
            (255, "SINT MAARTEN, SABA, SAINT EUSTATIUS"),
            (243, "PEOPLE'S DEM REP OF YEMEN"),
            (30, "CELEBES & MOLUCCA ISLANDS"),
        ] {
            assert_eq!(
                entity_by_dxcc(code),
                Some(DxccEntity {
                    code,
                    name,
                    deleted: true
                })
            );
        }
    }

    /// ADIF's 0 ("no DXCC entity") and a number neither table holds name nothing, so a caller can
    /// tell them from a real entity.
    #[test]
    fn no_entity_and_an_unknown_number_name_nothing() {
        assert_eq!(entity_by_dxcc(NO_DXCC_ENTITY), None);
        assert_eq!(entity_by_dxcc(73), None, "a number the ARRL never assigned");
        assert_eq!(entity_by_dxcc(523), None, "past the last one assigned");
        // Control: the neighbours of that gap do name entities.
        assert!(entity_by_dxcc(72).is_some() && entity_by_dxcc(74).is_some());
    }

    /// The join is on the primary prefix: a row whose number is not a number is skipped, never
    /// read as some other field.
    #[test]
    fn a_csv_row_without_a_number_is_skipped() {
        let parsed = parse_csv_numbers("K,United States,291,NA\r\nXX,Nowhere,abc,EU\r\n");
        assert_eq!(parsed.get("K"), Some(&291));
        assert_eq!(parsed.get("XX"), None);
    }

    /// What the Logbook's picker offers: the ARRL's 402, current and deleted, one per number in
    /// number order, each named as a record carrying its number is — and no WAE area of its own.
    #[test]
    fn the_entities_offered_are_the_402_by_number() {
        let all = entities();
        assert_eq!(all.len(), 402);
        assert_eq!(all.iter().filter(|e| e.deleted).count(), 62);
        assert!(
            all.windows(2).all(|w| w[0].code < w[1].code),
            "number order"
        );
        for e in &all {
            assert_eq!(entity_by_dxcc(e.code), Some(*e), "{}", e.code);
        }
        let named = |code| {
            all.iter()
                .find(|e| e.code == code)
                .map(|e| (e.name, e.deleted))
        };
        assert_eq!(named(182), Some(("Navassa Island", false)));
        assert_eq!(named(229), Some(("GERMAN DEMOCRATIC REPUBLIC", true)));
        assert_eq!(
            named(248),
            Some(("Italy", false)),
            "Sicily's number is Italy's"
        );
        assert!(
            !all.iter().any(|e| e.name == "Sicily"),
            "a WAE area is no entity"
        );
        assert_eq!(named(NO_DXCC_ENTITY), None);
    }

    /// A contact is shown under the entity its record's number names, wherever it names one — a WAE
    /// area's contact carrying its parent's number as the parent — and under its call's otherwise.
    #[test]
    fn a_contact_is_shown_under_the_entity_its_records_number_names() {
        let shown = |call: &str, dxcc| shown_entity(resolve(call).as_ref(), dxcc);
        assert_eq!(resolve("K1N").map(|i| i.entity), Some("United States"));
        assert_eq!(shown("K1N", Some(182)), Some("Navassa Island"));
        assert_eq!(
            shown("Y24UK", Some(229)),
            Some("GERMAN DEMOCRATIC REPUBLIC")
        );
        assert_eq!(shown("IT9ABC", Some(248)), Some("Italy"));
        // Controls: no number, the call's own, none, and an unknown one.
        assert_eq!(shown("K1N", None), Some("United States"));
        assert_eq!(shown("W1AW", Some(291)), Some("United States"));
        assert_eq!(shown("K1N", Some(NO_DXCC_ENTITY)), Some("United States"));
        assert_eq!(shown("K1N", Some(523)), Some("United States"));
        // A call cty.dat cannot place is shown under its record's entity, or none.
        assert_eq!(shown("Q", Some(182)), Some("Navassa Island"));
        assert_eq!(shown("Q", None), None);
    }

    /// All six WAE areas follow the number strictly: a contact whose record carries its parent's
    /// DXCC number is shown as the parent, the DXCC entity that number names, and one without a
    /// number as its call places it. cty.dat lists every Vienna Intl Ctr call under Austria as
    /// well, after it, so that call already resolves to Austria; with 206 it is Austria too.
    #[test]
    fn a_wae_areas_contact_carrying_its_parents_number_is_shown_as_the_parent() {
        for (call, code, parent, area, area_is_dxcc) in [
            ("IT9ABC", 248, "Italy", "Sicily", false),
            ("IG9ABC", 248, "Italy", "African Italy", false),
            ("2M0BDR", 279, "Scotland", "Shetland Islands", false),
            ("JW0BEA", 259, "Svalbard", "Bear Island", false),
            ("TA1ABC", 390, "Asiatic Turkey", "European Turkey", false),
            ("4U1A", 206, "Austria", "Austria", true),
        ] {
            let info = resolve(call).unwrap_or_else(|| panic!("{call} resolves"));
            assert_eq!(
                (info.entity, info.is_dxcc, info.dxcc),
                (area, area_is_dxcc, Some(code)),
                "premise: {call} is placed in {area}, carrying {parent}'s number"
            );
            assert_eq!(
                shown_entity(Some(&info), Some(code)),
                Some(parent),
                "{call} with {code}"
            );
            assert_eq!(
                shown_entity(Some(&info), None),
                Some(area),
                "{call} without a number"
            );
        }
    }

    /// A record's COUNTRY is the name of the DXCC entity its number names — never a WAE area's —
    /// and nothing where the number names none, so the record's own text stands. The Country
    /// column says the same for every number, whatever the call.
    #[test]
    fn a_records_country_is_the_name_its_dxcc_number_gives_it() {
        assert_eq!(country_by_dxcc(182), Some("Navassa Island"));
        assert_eq!(country_by_dxcc(229), Some("GERMAN DEMOCRATIC REPUBLIC"));
        assert_eq!(country_by_dxcc(291), Some("United States"));
        assert_eq!(country_by_dxcc(248), Some("Italy"), "never Sicily");
        assert_eq!(country_by_dxcc(390), Some("Asiatic Turkey"));
        // 0 and an unknown number name nothing: no call's entity is offered.
        assert_eq!(country_by_dxcc(NO_DXCC_ENTITY), None);
        assert_eq!(country_by_dxcc(523), None);
        for call in ["K1N", "IT9ABC", "TA1ABC", "Q"] {
            for e in entities() {
                assert_eq!(
                    shown_entity(resolve(call).as_ref(), Some(e.code)),
                    country_by_dxcc(e.code),
                    "{call} with {}",
                    e.code
                );
            }
        }
    }

    /// On the callsign card a WAE area's call is its DXCC entity: typed (no record), and logged with
    /// its parent's number, with none (as Nexus logs it), and with a number that names nothing,
    /// which the column passes over as well. A number its record carries wins, as in the column.
    #[test]
    fn a_wae_areas_call_is_its_dxcc_entity_on_the_card_with_or_without_a_number() {
        for (call, code, parent) in [
            ("IT9ABC", 248, "Italy"),
            ("IG9ABC", 248, "Italy"),
            ("2M0BDR", 279, "Scotland"),
            ("JW0BEA", 259, "Svalbard"),
            ("TA1ABC", 390, "Asiatic Turkey"),
            ("4U1A", 206, "Austria"),
        ] {
            let card = |dxcc| card_entity(resolve(call).as_ref(), dxcc);
            assert_eq!(
                [
                    card(None),
                    card(Some(code)),
                    card(Some(NO_DXCC_ENTITY)),
                    card(Some(523))
                ],
                [Some(parent); 4],
                "{call}"
            );
        }
        let card = |call: &str, dxcc| card_entity(resolve(call).as_ref(), dxcc);
        assert_eq!(card("IT9ABC", Some(182)), Some("Navassa Island"));
        assert_eq!(card("K1N", Some(182)), Some("Navassa Island"));
        assert_eq!(card("K1N", None), Some("United States"));
        assert_eq!(card("Q", Some(182)), Some("Navassa Island"));
        assert_eq!(card("Q", None), None);
    }

    /// A call outside the WAE areas is its own entity on the card, for every entity in cty.dat:
    /// each DXCC entity's number names that entity, and only the six WAE areas name another.
    #[test]
    fn every_other_call_is_its_own_entity_on_the_card() {
        let r = resolver();
        let mut areas = Vec::new();
        for i in 0..r.entities.len() {
            let e = info(r, i as u32, None);
            if e.is_dxcc {
                assert_eq!(card_entity(Some(&e), None), Some(e.entity));
            } else {
                areas.push((e.entity, card_entity(Some(&e), None)));
            }
        }
        areas.sort_unstable();
        assert_eq!(
            areas,
            [
                ("African Italy", Some("Italy")),
                ("Bear Island", Some("Svalbard")),
                ("European Turkey", Some("Asiatic Turkey")),
                ("Shetland Islands", Some("Scotland")),
                ("Sicily", Some("Italy")),
                ("Vienna Intl Ctr", Some("Austria")),
            ]
        );
    }
}
