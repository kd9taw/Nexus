//! Amateur-satellite TLE currency (the `live` feature) — the orbital elements
//! the pure geometry in [`crate::sat`] runs on, and the fetch policy that keeps
//! them current without abusing anyone's server.
//!
//! Two fetch legs, one acceptance gate:
//! - **Mirror** (primary): `hamradiotools.io/nexus/tles.json`, a rolling
//!   GitHub-Release asset regenerated every 6 h by `.github/workflows/tles.yml`
//!   running `scripts/gen-tles.mjs`. Served WITH cache validators, so a refresh
//!   is a conditional GET — an unchanged set costs a ~300-byte 304. Since
//!   schema 2 it carries a [`SatCatalogEntry`] list beside the elements: the
//!   derived amateur POPULATION with each bird's status, including the birds
//!   no element source could cover.
//! - **Celestrak direct** ([`fetch_tles`], the narrow fallback):
//!   `gp.php?GROUP=amateur&FORMAT=tle`. Celestrak serves NO cache validators,
//!   updates on a 2 h cycle, and 403s consumers that re-download inside one
//!   cycle — so this leg runs only when the cache is >24 h old or absent,
//!   never inside Celestrak's own 2 h floor, and a 403/404 hard-stops it for
//!   24 h ([`TleFetchError::Blocked`]).
//!
//! [`validate_tles`] is the gate every FETCHED set passes before it may
//! replace a good cache (mirrored on the publishing side by
//! `scripts/gen-tles.mjs`); operator FILE IMPORTS pass only the per-bird
//! [`tle_bird_ok`] integrity check — no count ratchet (one new launch is one
//! bird) and no set-freshness gate (the 30 d per-bird ceiling at USE stays
//! the honesty line). [`tle_fetch_target`] is the pure when/where decision
//! the shell's background refresh obeys. The parse is pure and unit-tested;
//! both fetches return `Err` on trouble so the caller keeps its cache rather
//! than fabricating orbits.
//!
//! Data courtesy of CelesTrak (Dr. T.S. Kelso).

use std::time::Duration;

use crate::sat::{self, Tle};

const TLE_URL: &str = "https://celestrak.org/NORAD/elements/gp.php?GROUP=amateur&FORMAT=tle";
const MIRROR_URL: &str = "https://hamradiotools.io/nexus/tles.json";
const UA: &str = "nexus-propagation/0.1 (+https://hamradiotools.io; ham radio satellite tracking)";

/// Refresh TTL against the MIRROR: 6 h ≈ 4 conditional GETs (mostly 304s) per
/// install per day. The Celestrak leg has its own, stricter timing below.
pub const TLE_TTL_SECS: i64 = 6 * 3600;

/// True if `line` looks like TLE line `n` (1 or 2): the `n ` prefix and the full
/// 69-column body. Tolerant of trailing junk (length only floored).
fn is_tle_line(line: &str, n: u8) -> bool {
    let prefix = if n == 1 { "1 " } else { "2 " };
    line.starts_with(prefix) && line.len() >= 69
}

/// Parse Celestrak 3LE (or bare 2LE) text into TLEs. Pure — unit-testable
/// without the network. Tolerant of `\r\n` and trailing junk; malformed triples
/// are skipped rather than fabricated. Names are trimmed; a name-less 2LE pair
/// keeps an empty name.
pub fn parse_tles(text: &str) -> Vec<Tle> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        // Named 3LE: <name> / "1 …" / "2 …".
        if i + 2 < lines.len()
            && !is_tle_line(lines[i], 1)
            && !is_tle_line(lines[i], 2)
            && is_tle_line(lines[i + 1], 1)
            && is_tle_line(lines[i + 2], 2)
        {
            out.push(Tle {
                name: lines[i].trim_start_matches("0 ").trim().to_string(),
                line1: lines[i + 1].to_string(),
                line2: lines[i + 2].to_string(),
            });
            i += 3;
        // Bare 2LE: "1 …" / "2 …" with no name line.
        } else if i + 1 < lines.len() && is_tle_line(lines[i], 1) && is_tle_line(lines[i + 1], 2) {
            out.push(Tle {
                name: String::new(),
                line1: lines[i].to_string(),
                line2: lines[i + 1].to_string(),
            });
            i += 2;
        } else {
            // Junk / half a malformed pair — skip and resync.
            i += 1;
        }
    }
    out
}

/// Why a TLE fetch failed — typed, because the shell's policy BRANCHES on it:
/// `Blocked` (Celestrak 403/404) hard-stops the direct leg for 24 h with an
/// operator-visible reason, while the rest just count toward the backoff.
/// (The old `Result<_, String>` made the 403 rule unimplementable.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TleFetchError {
    /// HTTP 403/404 — Celestrak's "stop asking" statuses (per-cycle re-download
    /// caps, per-IP limits). Retrying these is how IPs get firewalled.
    Blocked(u16),
    /// Any other non-success HTTP status.
    Http(u16),
    /// Connect/DNS/timeout trouble — the network, not the server's answer.
    Network(String),
    /// A 200 with nothing usable in it ("No GP data found" on a cold Celestrak
    /// cache, a truncated body) — must never read as "zero satellites".
    Empty,
}

impl std::fmt::Display for TleFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TleFetchError::Blocked(c) => write!(f, "server refused (HTTP {c})"),
            TleFetchError::Http(c) => write!(f, "HTTP {c}"),
            TleFetchError::Network(e) => write!(f, "network: {e}"),
            TleFetchError::Empty => write!(f, "empty payload (no TLEs parsed)"),
        }
    }
}

/// Fetch + parse the current amateur TLE set DIRECTLY from Celestrak — the
/// narrow fallback leg. `Err` typed so the caller can hard-stop on 403/404;
/// on any error the caller serves stale-or-nothing, never fabricated orbits.
pub fn fetch_tles() -> Result<Vec<Tle>, TleFetchError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .build()
        .map_err(|e| TleFetchError::Network(e.to_string()))?;
    let resp = client
        .get(TLE_URL)
        .send()
        .map_err(|e| TleFetchError::Network(e.to_string()))?;
    let status = resp.status();
    if status.as_u16() == 403 || status.as_u16() == 404 {
        return Err(TleFetchError::Blocked(status.as_u16()));
    }
    if !status.is_success() {
        return Err(TleFetchError::Http(status.as_u16()));
    }
    let text = resp
        .text()
        .map_err(|e| TleFetchError::Network(e.to_string()))?;
    let tles = parse_tles(&text);
    if tles.is_empty() {
        return Err(TleFetchError::Empty);
    }
    Ok(tles)
}

/// One bird in the mirror's amateur CATALOG (`tles.json` schema 2) — who it
/// is and whether it is still worth chasing, INDEPENDENT of whether anyone
/// could find current elements for it.
///
/// The catalog exists because existence used to be elements-only: a bird
/// SatNOGS knows about but no element source carries simply did not appear,
/// with no row, no warning and no way for the UI to say "no current
/// elements". It is also the status SEED — the live per-bird status path is
/// still the SatNOGS join in the shell, but that only knows the birds it has
/// been asked about, and it cannot answer at all for a bird SatNOGS files
/// under a placeholder catalog number.
///
/// Absent on a legacy (schema 1) manifest and on the Celestrak fallback leg,
/// where it stays empty and nothing is stamped — exactly the pre-catalog
/// behavior.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SatCatalogEntry {
    /// The catalog number the ELEMENT LINE carries (the publisher resolves
    /// SatNOGS placeholder ids to their `norad_follow_id` first) — the key
    /// every client surface already identifies a bird by.
    pub norad: u32,
    pub name: String,
    /// The SatNOGS `status` verbatim (`alive` | `dead` | `re-entered` |
    /// `future`), kept as the source string for the same reason
    /// [`crate::live::satnogs::SatStatus`] does: an unseen value degrades to a
    /// label instead of being dropped. NOT always `alive`: the catalog lists
    /// the birds an operator may ask about, which includes the ones that just
    /// stopped being workable.
    pub status: String,
    /// The bird carries at least one LIVE amateur transmitter. `false` is a
    /// real answer, not a placeholder — an alive-in-orbit bird whose every
    /// amateur transmitter has gone quiet (AO-85) is listed with `false`,
    /// which is the one fact no pass elevation can ever show.
    #[serde(default)]
    pub amateur: bool,
    /// A decay date is on record upstream.
    #[serde(default)]
    pub decayed: bool,
    /// Which mirror leg supplied this bird's elements — `None` when the
    /// publisher had none to give: either no source carried any (the whole
    /// point of the catalog — a bird known, alive, amateur and unfindable) or
    /// the bird is outside the ACTIVE tier, where elements would be a fiction
    /// on a decayed orbit and a distraction on a mute one. Either way the UI
    /// must be able to say so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
}

/// A conditional mirror-fetch outcome: the published set changed (new elements
/// + the ETag to send next time), or the server said 304 (bump the freshness
///   stamp, keep what you have).
pub enum TleMirrorFetch {
    NotModified,
    Fresh {
        elements: Vec<Tle>,
        catalog: Vec<SatCatalogEntry>,
        generated: Option<String>,
        etag: Option<String>,
    },
}

/// The mirror manifest (`tles.json`, written by `scripts/gen-tles.mjs`) — the
/// manifest IS the payload. Only the fields the client consumes are
/// deserialized here; `schema`/`count`/`sources`/`medianEpochAgeDays` are the
/// publisher's own bookkeeping and the client re-derives everything it gates
/// on. Unknown fields are ignored and every field defaults, so a schema-1
/// manifest (no `catalog`) and a schema-2 one both load.
#[derive(serde::Deserialize)]
struct MirrorManifest {
    #[serde(default)]
    generated: Option<String>,
    #[serde(default)]
    elements: Vec<Tle>,
    #[serde(default)]
    catalog: Vec<SatCatalogEntry>,
}

/// Fetch the mirror's element set with the previous fetch's ETag (the
/// `lotw_users` conditional-GET shape). Celestrak's gp.php serves NO cache
/// validators, so conditional GET is a mirror-leg-only economy. A mirror
/// 403/404 is a plain `Http` failure — the 24 h `Blocked` hard stop is
/// Celestrak etiquette, and our own mirror being down must not trip it.
pub fn fetch_tles_mirror(etag: Option<&str>) -> Result<TleMirrorFetch, TleFetchError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .build()
        .map_err(|e| TleFetchError::Network(e.to_string()))?;
    let mut req = client.get(MIRROR_URL);
    if let Some(t) = etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, t);
    }
    let resp = req
        .send()
        .map_err(|e| TleFetchError::Network(e.to_string()))?;
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(TleMirrorFetch::NotModified);
    }
    if !resp.status().is_success() {
        return Err(TleFetchError::Http(resp.status().as_u16()));
    }
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let manifest: MirrorManifest = resp
        .json()
        .map_err(|e| TleFetchError::Network(e.to_string()))?;
    if manifest.elements.is_empty() {
        return Err(TleFetchError::Empty);
    }
    Ok(TleMirrorFetch::Fresh {
        elements: manifest.elements,
        catalog: manifest.catalog,
        generated: manifest.generated,
        etag,
    })
}

/// The TLE mod-10 line checksum (column 69): the sum of the digits, counting
/// each `-` as 1, over the first 68 columns. Every line of a live amateur
/// group passes — a FREE truncation/corruption gate.
pub fn tle_line_checksum_ok(line: &str) -> bool {
    let b = line.trim_end().as_bytes();
    if b.len() != 69 {
        return false;
    }
    let Some(want) = (b[68] as char).to_digit(10) else {
        return false;
    };
    let sum: u32 = b[..68]
        .iter()
        .map(|&c| match c {
            b'0'..=b'9' => u32::from(c - b'0'),
            b'-' => 1,
            _ => 0,
        })
        .sum();
    sum % 10 == want
}

/// Per-bird integrity: full 69-char lines with mod-10 checksums, a NORAD id,
/// a parseable epoch, and sgp4 accepting the element math. The one per-bird
/// gate [`validate_tles`] filters on — and the ONLY gate an operator file
/// import passes: an import may legitimately be a single new launch (no count
/// ratchet applies) or days old (the per-bird 30 d gate at USE stays the
/// honesty line), but a corrupt line is a corrupt line everywhere.
pub fn tle_bird_ok(t: &Tle, now_unix: i64) -> bool {
    tle_line_checksum_ok(&t.line1)
        && tle_line_checksum_ok(&t.line2)
        && sat::norad_id(&t.line1).is_some()
        && sat::tle_age_days(&t.line1, now_unix).is_some()
        && sat::sgp4_constructible(t)
}

/// The ONE acceptance gate a candidate element set passes before it may
/// replace a good cache — shared by both fetch legs, mirrored on the
/// publishing side by `scripts/gen-tles.mjs`. Returns the cleaned set
/// (per-bird rejects dropped, NORAD-deduped newest-epoch-wins, input order
/// kept) or `Err` with why the WHOLE set is refused — in which case the caller
/// keeps what it has: an invalid or empty result never overwrites a good cache.
///
/// `prev_count` is the bird count of the cache being replaced (0 = none): the
/// ratchet floor is `max(40, 0.6 × prev)` — looser than the publisher's
/// 0.85×, because a legitimate regroup the mirror already vetted must still
/// install here. Freshness gates are median-shaped, NEVER max: AO-10 is
/// legitimately old and must not condemn a fresh set.
pub fn validate_tles(
    candidate: &[Tle],
    prev_count: usize,
    now_unix: i64,
) -> Result<Vec<Tle>, String> {
    if candidate.is_empty() {
        return Err("no elements parsed".into());
    }
    // Per-bird integrity ([`tle_bird_ok`]): a bad bird drops alone…
    let clean: Vec<&Tle> = candidate
        .iter()
        .filter(|t| tle_bird_ok(t, now_unix))
        .collect();
    // …but more than 10% dropping means the PAYLOAD is bad (truncation,
    // format drift), not the birds: refuse the lot.
    if clean.len() * 10 < candidate.len() * 9 {
        return Err(format!(
            "{} of {} birds failed integrity checks",
            candidate.len() - clean.len(),
            candidate.len()
        ));
    }
    // NORAD-dedupe, newest epoch wins (a merged or doubled feed must not draw
    // one bird twice, and must keep its freshest elements).
    let age = |t: &Tle| sat::tle_age_days(&t.line1, now_unix).unwrap_or(f64::INFINITY);
    let mut by_norad: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    let mut deduped: Vec<Tle> = Vec::with_capacity(clean.len());
    for t in clean {
        let norad = sat::norad_id(&t.line1).expect("filtered above");
        match by_norad.get(&norad) {
            Some(&i) => {
                if age(t) < age(&deduped[i]) {
                    deduped[i] = t.clone();
                }
            }
            None => {
                by_norad.insert(norad, deduped.len());
                deduped.push(t.clone());
            }
        }
    }
    // Count ratchet: a plausible amateur group (~95 birds live), and never a
    // silent mass shrink against what we already have.
    let floor = std::cmp::max(40, prev_count * 6 / 10);
    if deduped.len() < floor {
        return Err(format!(
            "only {} birds (floor is max(40, 0.6×{prev_count}) = {floor})",
            deduped.len()
        ));
    }
    // The canary: NORAD 25544 (ISS) is always in the amateur group; its
    // absence means the wrong group or a mangled payload.
    if !by_norad.contains_key(&25_544) {
        return Err("canary NORAD 25544 (ISS) missing".into());
    }
    // Freshness: median epoch age ≤ 7 d AND at least half the birds < 3 d.
    // Median-shaped, never max — see the doc comment (AO-10).
    let mut ages: Vec<f64> = deduped.iter().map(age).collect();
    ages.sort_by(|a, b| a.total_cmp(b));
    let median = ages[ages.len() / 2];
    if median > 7.0 {
        return Err(format!(
            "median epoch age {median:.1} d (> 7 d) — a stale set"
        ));
    }
    let under3 = ages.iter().filter(|a| **a < 3.0).count();
    if under3 * 2 < ages.len() {
        return Err(format!(
            "only {under3} of {} birds under 3 d old — a stale set",
            ages.len()
        ));
    }
    Ok(deduped)
}

/// Where the next refresh attempt should go — or `None` for "not now".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TleFetchTarget {
    Mirror,
    Celestrak,
}

/// [`validate_tles`]' ratchet baseline for ONE leg: the held bird count, but
/// only when the incoming set is drawn the same way as the held one.
///
/// The two legs publish DIFFERENT POPULATIONS. The mirror's union is the whole
/// amateur catalog (~370 birds with elements); Celestrak's `GROUP=amateur` is
/// 97 of them. Ratcheting the fallback against a mirror-sized cache sets a
/// floor of `max(40, 0.6 × 367) = 220` that a complete, healthy `GROUP=amateur`
/// response can never clear — so the day the mirror goes down, which is the
/// only day this leg ever runs, the fallback refuses too and the elements sit
/// frozen until they age past the 30 d ceiling and the sky goes blank. A
/// ratchet is only meaningful against a set drawn from the same source; across
/// sources it is comparing two different questions. `0` = the fixed floor of
/// 40 alone, which is the honest gate for "we have nothing comparable".
pub fn tle_ratchet_count(
    prev_source: Option<&str>,
    prev_count: usize,
    target: TleFetchTarget,
) -> usize {
    let same_leg = matches!(
        (prev_source, target),
        (Some("mirror"), TleFetchTarget::Mirror) | (Some("celestrak"), TleFetchTarget::Celestrak)
    );
    if same_leg {
        prev_count
    } else {
        0
    }
}

/// The pure when/where decision for a background TLE refresh — every timing
/// rule in one table-tested function; the shell supplies state and obeys.
///
/// - `cache_age_secs`: wall-clock age of the snapshot's `fetchedAt` (`None` =
///   no cache, or a legacy cache with no stamp).
/// - `last_try` / `fails`: last attempt on either leg (unix) + consecutive
///   failures — the exponential backoff (30 min × 2ⁿ⁻¹, capped at the 6 h TTL).
/// - `ct_last_try`: last CELESTRAK attempt — its own 2 h floor (Celestrak's
///   update cycle; re-asking inside one cycle is what draws 403s).
/// - `blocked_until`: the 24 h hard stop a Celestrak 403/404 set.
/// - `manual`: the operator's refresh button — bypasses TTL and backoff, but
///   NEVER the Celestrak floor or block (etiquette isn't operator-waivable;
///   the mirror leg is always available to a manual refresh).
///
/// The mirror is the primary leg. Celestrak is chosen only when the mirror is
/// demonstrably failing (`fails > 0` — the previous attempt lost) AND the
/// cache is >24 h old or absent AND its floor + block allow: the narrow
/// fallback, expected to run on ~0% of installs.
pub fn tle_fetch_target(
    cache_age_secs: Option<i64>,
    last_try: i64,
    ct_last_try: i64,
    fails: u32,
    blocked_until: i64,
    now: i64,
    manual: bool,
) -> Option<TleFetchTarget> {
    if !manual {
        if cache_age_secs.is_some_and(|a| a < TLE_TTL_SECS) {
            return None; // fresh — nothing to do
        }
        if fails > 0 {
            let backoff = 1800i64
                .saturating_mul(1 << (fails - 1).min(4))
                .min(TLE_TTL_SECS);
            if now - last_try < backoff {
                return None; // inside the failure backoff window
            }
        }
    }
    let celestrak_ok = fails > 0
        && cache_age_secs.is_none_or(|a| a >= 24 * 3600)
        && now >= blocked_until
        && now - ct_last_try >= 2 * 3600;
    Some(if celestrak_ok {
        TleFetchTarget::Celestrak
    } else {
        TleFetchTarget::Mirror
    })
}

/// Same-flight Celestrak escalation for a MANUAL refresh whose mirror leg
/// just failed (404 / network — the fetch itself, not a refused payload).
/// [`tle_fetch_target`] starts a manual attempt at the mirror; when that leg
/// dies mid-flight, an explicit click is explicit intent — the >24 h-cache
/// eligibility (and the `fails > 0` prerequisite: the failure is THIS
/// flight's own) are waived. Celestrak etiquette is not: the 2 h floor and
/// the 403/404 hard stop hold, manual or not.
pub fn tle_manual_escalation(ct_last_try: i64, blocked_until: i64, now: i64) -> bool {
    now >= blocked_until && now - ct_last_try >= 2 * 3600
}

#[cfg(test)]
mod tests {
    use super::*;

    // Two real, published element sets (AIAA-2006-6753 verification vectors) in
    // Celestrak 3LE form, with `\r\n` and a trailing malformed pair to exercise
    // the tolerance paths.
    const FIXTURE: &str = "ISS (ZARYA)             \r\n\
1 25544U 98067A   08264.51782528 -.00002182  00000-0 -11606-4 0  2927\r\n\
2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.72125391563537\r\n\
VANGUARD 1\r\n\
1 00005U 58002B   00179.78495062  .00000023  00000-0  28098-4 0  4753\r\n\
2 00005  34.2682 348.7242 1859667 331.7664  19.3264 10.82419157413667\r\n\
BROKEN BIRD\r\n\
1 99999U short line\r\n";

    /// The committed LIVE corpus (fetched 2026-07-31): 97 birds / 16.3 KB,
    /// every line checksum-clean, ISS present, median epoch age ~0.3 d.
    const LIVE: &str = include_str!("../../tests/fixtures/celestrak-amateur-2026-08-01.tle");
    /// A frozen "now" one day past the corpus fetch, so the freshness gates
    /// see the corpus exactly as a client would have — forever.
    const LIVE_NOW: i64 = 1_785_542_400; // 2026-08-01T00:00:00Z

    #[test]
    fn parses_two_birds_and_skips_malformed() {
        let tles = parse_tles(FIXTURE);
        assert_eq!(
            tles.len(),
            2,
            "two well-formed birds, the broken one skipped"
        );
        assert_eq!(tles[0].name, "ISS (ZARYA)"); // trailing spaces trimmed
        assert!(tles[0].line1.starts_with("1 25544U"));
        assert!(tles[0].line2.starts_with("2 25544"));
        assert_eq!(tles[0].line1.len(), 69); // no trailing \r left on the lines
        assert_eq!(tles[1].name, "VANGUARD 1");
        assert!(tles[1].line1.starts_with("1 00005U"));
    }

    #[test]
    fn empty_or_garbage_yields_no_tles() {
        assert!(parse_tles("").is_empty());
        assert!(parse_tles("No GP data found").is_empty());
        assert!(parse_tles("random\ntext\nlines\n").is_empty());
    }

    #[test]
    fn accepts_bare_two_line_pairs() {
        let two_le = "1 25544U 98067A   08264.51782528 -.00002182  00000-0 -11606-4 0  2927\n\
2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.72125391563537\n";
        let tles = parse_tles(two_le);
        assert_eq!(tles.len(), 1);
        assert_eq!(tles[0].name, "");
        assert!(tles[0].line1.starts_with("1 25544U"));
    }

    // --- checksum ------------------------------------------------------------

    #[test]
    fn checksum_accepts_every_live_line_and_rejects_corruption() {
        let tles = parse_tles(LIVE);
        assert!(tles.len() >= 90, "the live corpus is ~97 birds");
        for t in &tles {
            assert!(tle_line_checksum_ok(&t.line1), "line1 of {}", t.name);
            assert!(tle_line_checksum_ok(&t.line2), "line2 of {}", t.name);
        }
        // Flip one digit — the checksum must notice.
        let good = &tles[0].line1;
        let mut corrupt = good.clone().into_bytes();
        corrupt[20] = if corrupt[20] == b'9' { b'8' } else { b'9' };
        assert!(!tle_line_checksum_ok(&String::from_utf8(corrupt).unwrap()));
        // Truncation and over-length both fail on shape alone.
        assert!(!tle_line_checksum_ok(&good[..40]));
        assert!(!tle_line_checksum_ok(&format!("{good}X")));
    }

    // --- validate_tles -------------------------------------------------------

    /// Recompute a line's mod-10 checksum after a test edits it — so edits
    /// exercise the SEMANTIC gates rather than tripping the checksum first.
    fn fix_checksum(line: &str) -> String {
        let body = &line[..68];
        let sum: u32 = body
            .bytes()
            .map(|c| match c {
                b'0'..=b'9' => u32::from(c - b'0'),
                b'-' => 1,
                _ => 0,
            })
            .sum();
        format!("{body}{}", sum % 10)
    }

    #[test]
    fn validate_accepts_the_live_corpus() {
        let tles = parse_tles(LIVE);
        let n = tles.len();
        let clean = validate_tles(&tles, n, LIVE_NOW).expect("the live corpus is valid");
        assert_eq!(clean.len(), n, "no live bird should drop");
        assert!(clean
            .iter()
            .any(|t| crate::sat::norad_id(&t.line1) == Some(25544)));
    }

    #[test]
    fn validate_rejects_empty_so_nothing_can_overwrite_a_cache_with_nothing() {
        assert!(validate_tles(&[], 0, LIVE_NOW).is_err());
        assert!(validate_tles(&[], 97, LIVE_NOW).is_err());
    }

    #[test]
    fn validate_rejects_a_truncated_copy_via_the_count_ratchet() {
        // Cut the corpus mid-file: the tolerant parser still yields ~half the
        // birds, each individually clean — only the ratchet against the
        // previous count can catch it.
        let tles = parse_tles(&LIVE[..LIVE.len() / 2]);
        assert!(tles.len() >= 40, "half a corpus still parses");
        let err = validate_tles(&tles, 97, LIVE_NOW).unwrap_err();
        assert!(err.contains("floor"), "got {err:?}");
    }

    #[test]
    fn the_celestrak_leg_is_never_ratcheted_against_the_mirror_population() {
        // THE cross-leg trap: a complete, healthy GROUP=amateur response is
        // ~97 birds, and the mirror union it would be replacing is ~370. Held
        // as one number, the fallback leg refuses its own good data forever —
        // and it only ever runs when the mirror is already down.
        let group = parse_tles(LIVE);
        assert!(
            validate_tles(&group, 367, LIVE_NOW).is_err(),
            "the raw count ratchet does refuse it — that is the bug"
        );
        let ratchet = tle_ratchet_count(Some("mirror"), 367, TleFetchTarget::Celestrak);
        assert_eq!(
            ratchet, 0,
            "a mirror cache says nothing about a group fetch"
        );
        assert_eq!(
            validate_tles(&group, ratchet, LIVE_NOW).unwrap().len(),
            group.len()
        );
        // …and the ratchet still bites WITHIN a leg, which is its whole job.
        let same = tle_ratchet_count(Some("celestrak"), 97, TleFetchTarget::Celestrak);
        assert_eq!(same, 97);
        assert!(validate_tles(&group[..50], same, LIVE_NOW).is_err());
        assert_eq!(
            tle_ratchet_count(Some("mirror"), 367, TleFetchTarget::Mirror),
            367
        );
        // A legacy/imported/bundled cache is not a leg's own population.
        assert_eq!(
            tle_ratchet_count(Some("legacy"), 97, TleFetchTarget::Celestrak),
            0
        );
        assert_eq!(tle_ratchet_count(None, 0, TleFetchTarget::Mirror), 0);
    }

    #[test]
    fn validate_rejects_a_dropped_canary() {
        let tles: Vec<Tle> = parse_tles(LIVE)
            .into_iter()
            .filter(|t| crate::sat::norad_id(&t.line1) != Some(25544))
            .collect();
        let err = validate_tles(&tles, 0, LIVE_NOW).unwrap_err();
        assert!(err.contains("25544"), "got {err:?}");
    }

    #[test]
    fn validate_drops_a_corrupt_bird_alone_but_refuses_mass_corruption() {
        let mut tles = parse_tles(LIVE);
        // One corrupt bird (bad checksum) drops alone; the set still installs.
        let victim = crate::sat::norad_id(&tles[3].line1).unwrap();
        tles[3].line1 = {
            let mut b = tles[3].line1.clone().into_bytes();
            b[25] = if b[25] == b'5' { b'6' } else { b'5' };
            String::from_utf8(b).unwrap()
        };
        let clean = validate_tles(&tles, tles.len(), LIVE_NOW).expect("one bad bird drops alone");
        assert_eq!(clean.len(), tles.len() - 1);
        assert!(!clean
            .iter()
            .any(|t| crate::sat::norad_id(&t.line1) == Some(victim)));
        // Corrupt >10% of the set and the whole payload is refused.
        for t in tles.iter_mut().take(15) {
            let mut b = t.line1.clone().into_bytes();
            b[25] = if b[25] == b'5' { b'6' } else { b'5' };
            t.line1 = String::from_utf8(b).unwrap();
        }
        let err = validate_tles(&tles, 0, LIVE_NOW).unwrap_err();
        assert!(err.contains("integrity"), "got {err:?}");
    }

    #[test]
    fn validate_dedupes_by_norad_keeping_the_newest_epoch() {
        let mut tles = parse_tles(LIVE);
        // Append an OLDER duplicate of the first bird (epoch pushed back a
        // year, checksum recomputed so only the dedupe logic is under test).
        let mut old = tles[0].clone();
        let yy: u32 = old.line1[18..20].parse().unwrap();
        old.line1 = fix_checksum(&format!(
            "{}{:02}{}",
            &old.line1[..18],
            yy - 1,
            &old.line1[20..]
        ));
        let newest = tles[0].line1.clone();
        tles.push(old);
        let n_unique = tles.len() - 1;
        let clean = validate_tles(&tles, n_unique, LIVE_NOW).expect("dupes dedupe, not reject");
        assert_eq!(clean.len(), n_unique);
        let norad = crate::sat::norad_id(&newest).unwrap();
        let kept = clean
            .iter()
            .find(|t| crate::sat::norad_id(&t.line1) == Some(norad))
            .unwrap();
        assert_eq!(kept.line1, newest, "the newest epoch must win");
    }

    #[test]
    fn validate_freshness_is_median_shaped_never_max() {
        let tles = parse_tles(LIVE);
        // The live corpus CONTAINS a legitimately old bird (AO-10, ~4 d) —
        // accepted above. Age the whole view by 6 d: median ~6.3 d is still
        // ≤ 7, but now almost nothing is under 3 d → the 50%-under-3 gate
        // refuses (a set this uniformly old means the pipeline stopped).
        let err = validate_tles(&tles, 0, LIVE_NOW + 6 * 86_400).unwrap_err();
        assert!(err.contains("under 3 d"), "got {err:?}");
        // Age it by 30 d: the median gate itself refuses.
        let err = validate_tles(&tles, 0, LIVE_NOW + 30 * 86_400).unwrap_err();
        assert!(err.contains("median"), "got {err:?}");
    }

    // --- the mirror manifest -------------------------------------------------

    #[test]
    fn manifest_carries_the_status_catalog() {
        // A schema-2 manifest, field for field as scripts/gen-tles.mjs writes
        // it (including the publisher-only keys the client must ignore).
        let json = r#"{
          "schema":2,"generated":"2026-08-01T18:00:00.000Z",
          "source":"satnogs amateur population × celestrak amateur+satnogs groups + satnogs tle",
          "attribution":{"license":"CC-BY-SA-4.0","text":"SatNOGS DB, CC BY-SA 4.0; CelesTrak"},
          "count":1,"catalogCount":4,"medianEpochAgeDays":0.56,
          "sources":{"celestrak-amateur":1},
          "elements":[{"name":"ISS (ZARYA)",
            "line1":"1 25544U 98067A   26212.89378683  .00008757  00000+0  16519-3 0  9996",
            "line2":"2 25544  51.6315  78.8506 0007211 358.5886   1.5081 15.49290909578688"}],
          "catalog":[
            {"norad":25544,"name":"ISS (ZARYA)","status":"alive","amateur":true,"decayed":false,
             "src":"celestrak-amateur"},
            {"norad":40320,"name":"FO-82","status":"alive","amateur":true,"decayed":false},
            {"norad":40967,"name":"FOX-1A","status":"alive","amateur":false,"decayed":false},
            {"norad":50988,"name":"TEVEL-1","status":"re-entered","amateur":true,"decayed":true}]
        }"#;
        let m: MirrorManifest = serde_json::from_str(json).expect("schema 2 parses");
        assert_eq!(m.elements.len(), 1);
        assert_eq!(m.generated.as_deref(), Some("2026-08-01T18:00:00.000Z"));
        assert_eq!(m.catalog.len(), 4);
        assert_eq!(
            m.catalog[0],
            SatCatalogEntry {
                norad: 25544,
                name: "ISS (ZARYA)".into(),
                status: "alive".into(),
                amateur: true,
                decayed: false,
                src: Some("celestrak-amateur".into()),
            }
        );
        // THE bird the catalog exists for: known, alive, amateur — and no
        // element source could cover it. It must arrive as a real entry, not
        // as an absence.
        assert_eq!(m.catalog[1].norad, 40320);
        assert_eq!(m.catalog[1].src, None);
        assert!(m.catalog[1].amateur);
        // …and the two shapes that carry the OTHER half of the catalog's job:
        // a bird still in orbit with nothing amateur transmitting on it, and
        // one that has left. Both arrive without elements, and neither
        // `amateur:false` nor a non-alive status may be read as "no data".
        assert_eq!(m.catalog[2].name, "FOX-1A");
        assert!(!m.catalog[2].amateur, "alive, and silent — a real answer");
        assert_eq!(m.catalog[3].status, "re-entered");
        assert!(m.catalog[3].decayed);
        assert_eq!(m.catalog[3].src, None);
    }

    #[test]
    fn a_schema_1_manifest_still_loads_with_an_empty_catalog() {
        // Every shipped mirror payload before schema 2, and the shape a
        // rollback would serve: elements only. It must behave exactly as it
        // always did — no catalog, nothing stamped, no refusal.
        let json = r#"{"schema":1,"generated":"2026-07-31T00:00:00.000Z","source":"celestrak amateur group",
          "count":1,"medianEpochAgeDays":0.3,
          "elements":[{"name":"ISS (ZARYA)",
            "line1":"1 25544U 98067A   26212.89378683  .00008757  00000+0  16519-3 0  9996",
            "line2":"2 25544  51.6315  78.8506 0007211 358.5886   1.5081 15.49290909578688"}]}"#;
        let m: MirrorManifest = serde_json::from_str(json).expect("schema 1 still parses");
        assert_eq!(m.elements.len(), 1);
        assert!(m.catalog.is_empty(), "no catalog is empty, never an error");
    }

    #[test]
    fn a_catalog_entry_round_trips_and_tolerates_a_thinner_publisher() {
        let entry = SatCatalogEntry {
            norad: 27607,
            name: "SAUDISAT 1C (SO-50)".into(),
            status: "alive".into(),
            amateur: true,
            decayed: false,
            src: Some("celestrak-amateur".into()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert_eq!(
            serde_json::from_str::<SatCatalogEntry>(&json).unwrap(),
            entry
        );
        assert!(!json.contains("null"), "an absent src is absent, not null");
        // Only norad/name/status are required; the flags default.
        let thin: SatCatalogEntry =
            serde_json::from_str(r#"{"norad":7530,"name":"AO-7","status":"alive"}"#).unwrap();
        assert!(!thin.amateur);
        assert!(!thin.decayed);
        assert_eq!(thin.src, None);
    }

    // --- tle_fetch_target ----------------------------------------------------

    #[test]
    fn fetch_target_table() {
        use TleFetchTarget::*;
        const H: i64 = 3600;
        let now = 1_785_542_400;
        // (age, last_try, ct_last_try, fails, blocked_until, manual) → expected
        type Case = (
            Option<i64>,
            i64,
            i64,
            u32,
            i64,
            bool,
            Option<TleFetchTarget>,
        );
        let table: [Case; 12] = [
            // Fresh cache (< 6 h TTL): nothing to do.
            (Some(3 * H), 0, 0, 0, 0, false, None),
            // No cache at all: the mirror, immediately.
            (None, 0, 0, 0, 0, false, Some(Mirror)),
            // Aged past the TTL: the mirror.
            (Some(7 * H), 0, 0, 0, 0, false, Some(Mirror)),
            // Manual bypasses the TTL…
            (Some(3 * H), 0, 0, 0, 0, true, Some(Mirror)),
            // One failure 10 min ago: inside the 30 min backoff.
            (Some(7 * H), now - 600, 0, 1, 0, false, None),
            // …which manual also bypasses.
            (Some(7 * H), now - 600, 0, 1, 0, true, Some(Mirror)),
            // Failure backoff elapsed, cache still < 24 h: retry the MIRROR
            // (Celestrak is not eligible on a merely-aging cache).
            (Some(7 * H), now - 2000, 0, 1, 0, false, Some(Mirror)),
            // Mirror failing AND cache > 24 h AND floor/block clear: Celestrak.
            (Some(25 * H), now - 2000, 0, 1, 0, false, Some(Celestrak)),
            // Same, but Celestrak was tried 1 h ago: its 2 h floor holds.
            (Some(25 * H), now - 2000, now - H, 1, 0, false, Some(Mirror)),
            // Same, but the 403 hard stop is live: never Celestrak — manual
            // included; the mirror keeps retrying.
            (
                Some(25 * H),
                now - 2000,
                0,
                1,
                now + 20 * H,
                false,
                Some(Mirror),
            ),
            (
                Some(25 * H),
                now - 2000,
                0,
                1,
                now + 20 * H,
                true,
                Some(Mirror),
            ),
            // Deep backoff caps at the 6 h TTL: 10 fails, last try 6 h+ ago.
            (
                Some(48 * H),
                now - 6 * H - 1,
                now - 3 * H,
                10,
                0,
                false,
                Some(Celestrak),
            ),
        ];
        for (i, (age, lt, ct, fails, blocked, manual, want)) in table.into_iter().enumerate() {
            assert_eq!(
                tle_fetch_target(age, lt, ct, fails, blocked, now, manual),
                want,
                "row {i}"
            );
        }
    }

    // --- tle_manual_escalation -----------------------------------------------

    #[test]
    fn manual_escalation_table() {
        const H: i64 = 3600;
        let now = 1_785_542_400;
        // (ct_last_try, blocked_until) → may a manual flight whose mirror leg
        // just failed go to Celestrak in the SAME flight?
        let table: [(i64, i64, bool); 6] = [
            // Floor + block clear: escalate. Note there is NO cache-age or
            // fails input at all — the >24 h eligibility and the fails>0
            // prerequisite are exactly what a manual click waives (the
            // pre-launch mirror 404s with a fresh cache, and "come back
            // tomorrow" is no answer to an explicit click).
            (0, 0, true),
            // Celestrak asked 1 h 59 m ago: inside its 2 h floor — never,
            // manual included (their update cycle; re-asking inside one
            // cycle is what draws 403s).
            (now - 2 * H + 60, 0, false),
            // Exactly 2 h since the last ask: the floor is met.
            (now - 2 * H, 0, true),
            // The 403/404 hard stop is live: never.
            (0, now + 20 * H, false),
            // The block expires exactly now: clear.
            (0, now, true),
            // Floor met but still blocked: the block alone refuses.
            (now - 3 * H, now + H, false),
        ];
        for (i, (ct, blocked, want)) in table.into_iter().enumerate() {
            assert_eq!(tle_manual_escalation(ct, blocked, now), want, "row {i}");
        }
    }

    #[test]
    fn fetch_target_backoff_grows_and_caps() {
        // fails=3 → 2 h backoff: 1 h59 m after the try is too soon, 2 h isn't.
        assert_eq!(
            tle_fetch_target(None, 1000, 0, 3, 0, 1000 + 7199, false),
            None
        );
        assert!(tle_fetch_target(None, 1000, 0, 3, 0, 1000 + 7200, false).is_some());
        // fails=30 must not overflow, and caps at 6 h.
        assert!(tle_fetch_target(None, 1000, 0, 30, 0, 1000 + 21_600, false).is_some());
        assert_eq!(
            tle_fetch_target(None, 1000, 0, 30, 0, 1000 + 21_599, false),
            None
        );
    }
}
