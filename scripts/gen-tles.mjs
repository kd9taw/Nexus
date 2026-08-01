#!/usr/bin/env node
// Build the amateur-satellite CATALOG mirror payload (tles.json + tles.txt).
//
// WHY A MIRROR: Celestrak's 2026 policies (a 2 h update cycle, one-download-
// per-cycle 403s on popular groups, per-IP caps, 50 HTTP errors in 2 h or
// 100 MB/day → firewall, and developers held responsible for their apps'
// fleet behavior) make every-install-fetches-directly a scaling liability.
// This script runs in CI every 6 h (.github/workflows/tles.yml) and publishes
// a rolling GitHub Release asset that hamradiotools.io/nexus/tles.json 302s
// to. Nexus installs refresh against the mirror with conditional GETs (the
// release asset serves an ETag; an unchanged set costs a ~300-byte 304) and
// touch Celestrak directly only as a narrow client-side fallback.
//
// WHY A UNION, NOT A GROUP: Celestrak's `GROUP=amateur` is 97 objects and
// roughly 40% derelict — measured against AMSAT's live status catalog, 60 of
// its birds had no report in 30 days, while 8 birds heard on the air THAT DAY
// (IO-86, Foresail-1p, SAKHACUBE, QMR-KWT 2, Ten-Koh 2, Marina, …) were not in
// it at all. The population the operator actually wants is SatNOGS's: alive
// satellites carrying a live amateur transmitter — 376 birds. So the mirror
// DERIVES the population from SatNOGS and then assembles elements for exactly
// those NORADs from three sources, in freshness order:
//   1. Celestrak GROUP=amateur   — the traditional set, Space-Track derived
//   2. Celestrak GROUP=satnogs   — purpose-built for this population (649)
//   3. SatNOGS /api/tle/         — 1668 records, and the ONLY source for the
//      ~334 birds with placeholder NORAD ids and for birds Celestrak 404s
//      (IO-86 / 40931 is SATCAT-operational, heard today, and gp.php?CATNR
//      answers "No GP data found").
//
// TWO TIERS, AND WHY. A bird is LISTED (a catalog row) on a far wider rule
// than it is DRAWN (published elements):
//   * ACTIVE  = status `alive` AND at least one LIVE amateur transmitter.
//     Exactly this set gets elements — it is what "which bird can I work" means.
//   * LISTED  = every bird SatNOGS has ever given an amateur transmitter,
//     minus re-entries older than RECENT_DECAY_DAYS. So a bird that dies, goes
//     silent, or re-enters KEEPS ITS ROW, carrying the status that says why.
// The second tier is not decoration: the client's whole honesty story — the
// dead / re-entered / pre-launch / alive-but-silent chips, and a ★ bird that
// stays clickable after it stops working — is unreachable without it. Publish
// only the active set and a starred bird that dies simply ceases to exist: no
// row in the birds list, none in the excluded list, no ★ left to switch off.
// Measured: 443 listed, 381 alive (376 of them active), 6 dead, 17 pre-launch,
// 39 recently re-entered. Elements are NOT published for the inactive tier —
// SGP4 on a decayed object is a fiction, and a bird with nothing left to work
// belongs in a row, not on the map.
//
// NO PER-CATNR LOOP — deliberately. The three legs above leave 5 of 376 birds
// uncovered, and a per-CATNR fetch does not rescue them: both measured
// remainders (40931, 61757) answer HTTP 404. A loop over the uncovered set
// would spend its whole budget on 404s, and 50 of those inside 2 h firewalls
// the mirror's IP on its first pass. Uncovered birds are REPORTED (in the job
// log, and in the catalog as an entry with no `src`) rather than hammered for.
//
// IDENTITY — the single most load-bearing rule. SatNOGS mints placeholder
// NORAD ids in the 98xxx/99xxx range for birds USSPACECOM has not catalogued;
// 194 of them carry `norad_follow_id` = the real number once assigned. The
// rule is ASYMMETRIC, and inverting it silently loses 176 birds:
//   * Celestrak lookups, de-duplication, published identity → follow id
//   * SatNOGS /api/tle/ lookups                             → raw cat id
// Both ids are therefore tried against every element source, and the WINNING
// element line's NORAD becomes the published identity — the client keys birds
// by that number, so a catalog row that disagrees is a status stamped on the
// wrong bird (gate 9).
//
// SANITY GATES — ANY failure publishes NOTHING (exit 1; the Release keeps its
// last good asset, which clients re-validate on their side anyway; the Rust
// twin of gates 4–8 is propagation::live::tle::validate_tles):
//   1. Every fetch HTTP 200, and no Celestrak non-data 200 body — "No GP data
//      found" (cold cache) or "GP data has not updated since your last
//      successful download" (in-cycle refusal). Neither may read as zero
//      satellites. A 403/404 STOPS the run; it is never retried.
//   2. The amateur derivation is sane: listed population size in range, the
//      ACTIVE set's share of the alive catalog in range, and BOTH clauses
//      (`service == "Amateur"` and the ITU-band match) still finding birds on
//      their own — a fraction gate alone cannot see one clause going dark,
//      because the band clause is a strict superset of the service clause.
//   3. Bird count >= max(floor, ceil(0.85 × previous UNION count)) — the
//      ratchet against the previously published manifest catches a silent
//      mass shrink a fixed floor would wave through. TLES_RATCHET_OVERRIDE
//      waives it for ONE run (see the env seams below).
//   4. Bird count <= ceiling (a wrong or merged group would balloon).
//   5. Every NORAD numeric and unique.
//   6. Canaries: NORAD 25544 (ISS) present, plus at least 6 of the 7
//      most-worked birds — SO-50, RS-44, AO-91, AO-7, PO-101, AO-73, ISS. The
//      "6 of 7" shape is deliberate: requiring all seven stalls the mirror the
//      day one re-enters, requiring none cannot see the derivation breaking.
//      SO-50 in particular is the canary for the band clause — SatNOGS calls
//      every one of its transmitters `service: "Unknown"`.
//   7. Every element line exactly 69 chars with a valid mod-10 checksum. A
//      failing line drops its BIRD; more than max(1, 2% of candidates)
//      dropping means the payload is bad, not the birds, and refuses the run.
//      The floor of one is deliberate: a single corrupt line in a 376-bird
//      feed must cost that bird, never the whole mirror cycle.
//   8. Epoch freshness: median age <= 7 d, p90 <= 30 d, AND at least 60% of
//      birds under 3 d. Median-shaped, NEVER max — AO-10 is legitimately old
//      and must not fail a fresh set. The p90 ceiling is 30 d rather than the
//      group era's 14 because the union deliberately carries a long tail
//      Celestrak does not (measured: 330 of 371 within 7 d, 348 within 30).
//      The under-3-d floor exists to mirror the CLIENT's stricter rule:
//      validate_tles refuses any set with fewer than half its birds under 3 d,
//      so publishing one would age every install's elements for nothing.
//   9. Identity: every catalog entry claiming elements has them, under exactly
//      its own NORAD, and no element is published for a bird outside the
//      ACTIVE population.
//  10. Round-trip: re-serializing the parsed set and re-parsing it yields
//      identical elements (parser/format drift dies here, not on clients).
//
// Output:
//   tles.json — manifest-is-payload: {schema, generated, source, attribution,
//               count, catalogCount, medianEpochAgeDays, sources, elements,
//               catalog}. `elements` keys match crates/propagation sat::Tle
//               serde; `catalog` is the status-bearing bird list (norad, name,
//               SatNOGS status, amateur, decayed, element source) and is
//               ADDITIVE — every shipped client deserializes only `generated`
//               and `elements` and ignores the rest. `attribution` travels
//               INSIDE the payload (the same rule the repo's other BY-SA
//               source follows) so the licence survives the file being copied
//               out of the release asset.
//   tles.txt  — the union as raw 3LE text (the operator import path).
//
// LICENCE: the population, names and statuses are derived from the SatNOGS DB
// (CC BY-SA 4.0, © SatNOGS / Libre Space Foundation contributors) and the
// elements come from CelesTrak (Dr. T.S. Kelso) and the SatNOGS TLE API. The
// published artifact is therefore an adaptation of BY-SA material and is
// itself offered under CC BY-SA 4.0 — see NOTICE.
//
// ETIQUETTE: one run per 6 h, fetches strictly sequential with a delay
// between them, an honest UA naming the project and a contact URL, and no
// retry of anything — a non-200 fails the run and waits for a human.
//
// Run:  node scripts/gen-tles.mjs [outDir]
//   TLES_FIXTURE_DIR      — THE TEST SEAM: read all five sources from files in
//                           this directory instead of the network, and scale
//                           the absolute count floors to fixture size (every
//                           structural, ratio, canary and freshness gate still
//                           runs). scripts/fixtures/tles is the committed set.
//   TLES_PREV             — path to the previously published tles.json (the
//                           count ratchet's baseline); missing = fixed floor.
//   TLES_RATCHET_OVERRIDE — set to 1 to waive gate 3 for THIS RUN, logging the
//                           baseline it waived. The workflow sets it on
//                           workflow_dispatch only, never on the schedule: it
//                           exists for the ONE step change the ratchet is
//                           right to refuse (the 97-bird group → 376-bird
//                           union cutover), not as a way to live without it.
//   TLES_NOW              — override "now" (unix seconds or ISO date) for the
//                           freshness gate, so a committed fixture tests the
//                           same way forever.

import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { realpathSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const SATNOGS_SATELLITES = 'https://db.satnogs.org/api/satellites/?format=json'
const SATNOGS_TRANSMITTERS = 'https://db.satnogs.org/api/transmitters/?format=json'
const SATNOGS_TLE = 'https://db.satnogs.org/api/tle/?format=json'
const CELESTRAK_AMATEUR = 'https://celestrak.org/NORAD/elements/gp.php?GROUP=amateur&FORMAT=tle'
const CELESTRAK_SATNOGS = 'https://celestrak.org/NORAD/elements/gp.php?GROUP=satnogs&FORMAT=tle'
const UA =
  'nexus-tles-mirror/2.0 (+https://hamradiotools.io; ham radio satellite catalog; CI, 4x/day; kd9taw@protonmail.com)'
/// Seconds between sequential fetches — five requests across two hosts, paced.
const FETCH_GAP_SECS = 3

/// ITU amateur-satellite service allocations (MHz), downlink side. A bird is
/// amateur if SatNOGS SAYS so (`service: "Amateur"`) OR if it transmits in one
/// of these — because `service` is sparsely curated and misses SO-50, AO-7's
/// CW beacon, RS-44's beacon and every ISS voice/APRS entry. The band clause
/// is a strict superset of the service clause today; the service clause is
/// kept anyway because it is the upstream's own assertion and will catch a
/// future allocation this table does not list.
const AMATEUR_BANDS_MHZ = [
  [29.3, 29.51],
  [144, 146],
  [435, 438],
  [1260, 1270],
  [2400, 2450],
  [3400, 3410],
  [5830, 5850],
  [10450, 10500],
  [24000, 24050],
]

/// How long a re-entered bird keeps its catalog row. Long enough that an
/// operator who comes back to the shack after a season finds the row that says
/// where their ★ went; short enough that the catalog stays a catalog and not a
/// graveyard — SatNOGS carries 575 re-entered birds with amateur transmitters,
/// of which 32 fall inside this window. When SatNOGS has no decay date yet
/// (SATCAT fills it late; 88 records have none at all) the record's own
/// `updated` stamp stands in, so a bird marked re-entered TODAY is listed for
/// the whole window rather than vanishing the same day.
const RECENT_DECAY_DAYS = 180

/// The birds an amateur-satellite list exists to serve. Gate 6 requires ISS
/// unconditionally and at least `WORKED_CANARY_MIN` of these — see the header.
const WORKED_CANARIES = [
  [27607, 'SO-50'],
  [44909, 'RS-44'],
  [43017, 'AO-91'],
  [7530, 'AO-7'],
  [43678, 'PO-101'],
  [39444, 'AO-73'],
  [25544, 'ISS'],
]
const WORKED_CANARY_MIN = 6

/// Absolute counts the gates floor and ceiling on. Only these numbers differ
/// between a production run and a fixture run — every structural, ratio,
/// canary, identity and freshness gate is identical in both.
export const PROD_LIMITS = {
  /// Published elements. 371 measured; a wrong or merged group balloons past
  /// the ceiling (GROUP=active alone is 16114 objects).
  minElements: 250,
  maxElements: 900,
  /// The derived amateur population (376 measured).
  minCatalog: 250,
  maxCatalog: 900,
  /// Its share of SatNOGS' alive catalog (376 / 1690 = 22.2% measured).
  minAmateurShare: 0.05,
  maxAmateurShare: 0.5,
  /// Each clause on its own (176 service-only, 376 band-only measured) — the
  /// census that sees one of them going dark.
  minServiceOnly: 100,
  minBandOnly: 250,
}

/// Fixture scale: the committed fixture is a hand-picked 13-bird population,
/// deliberately dense in amateur birds, so only the absolute counts and the
/// share band relax.
export const FIXTURE_LIMITS = {
  minElements: 1,
  maxElements: 900,
  minCatalog: 1,
  maxCatalog: 900,
  minAmateurShare: 0.01,
  maxAmateurShare: 1,
  minServiceOnly: 1,
  minBandOnly: 1,
}

/// A refused payload. Thrown rather than exited so the gates are unit-testable;
/// `main` turns it into the unmasked exit 1 that publishes nothing.
export class GateError extends Error {
  constructor(msg) {
    super(msg)
    this.name = 'GateError'
  }
}

const fail = (msg) => {
  throw new GateError(msg)
}

// TLE mod-10 line checksum (column 69): sum of digits, '-' counts 1, over the
// first 68 columns. Matches propagation::live::tle::tle_line_checksum_ok.
export function checksumOk(line) {
  if (line.length !== 69) return false
  let sum = 0
  for (const c of line.slice(0, 68)) {
    if (c >= '0' && c <= '9') sum += +c
    else if (c === '-') sum += 1
  }
  return sum % 10 === +line[68]
}

// TLE epoch (line 1 cols 19–32: 2-digit year + fractional day-of-year) → unix
// seconds. Matches propagation::sat::tle_age_days' year window (57 pivot).
export function epochUnix(line1) {
  const yy = parseInt(line1.slice(18, 20), 10)
  const doy = parseFloat(line1.slice(20, 32))
  if (!Number.isFinite(yy) || !Number.isFinite(doy) || doy < 1) return null
  const year = yy < 57 ? 2000 + yy : 1900 + yy
  return Date.UTC(year, 0, 1) / 1000 + (doy - 1) * 86400
}

/// One candidate bird from an element source, with the two facts every later
/// stage needs (its catalog number and its epoch) already extracted.
function candidate(name, line1, line2, source) {
  const norad = Number(line1.slice(2, 7).trim())
  const epoch = epochUnix(line1)
  const ok =
    checksumOk(line1) &&
    checksumOk(line2) &&
    Number.isInteger(norad) &&
    norad > 0 &&
    epoch !== null
  return ok ? { name, line1, line2, norad, epoch, source } : null
}

/// STRICT 3LE parse: name / line1 / line2 triples and nothing else. A
/// STRUCTURAL surprise (a line count that is not a multiple of 3, an element
/// line where a name belongs) is a feed fault and fails the run by line
/// number; a PER-BIRD integrity failure drops that bird into `rejected`, for
/// the 2% budget in gate 7 to judge.
export function parse3le(text, source) {
  for (const marker of ['No GP data found', 'GP data has not updated']) {
    if (text.includes(marker)) {
      fail(`${source}: Celestrak answered "${marker}…" — that is not zero satellites`)
    }
  }
  const lines = text.split(/\r?\n/).filter((l) => l.trim() !== '')
  if (lines.length % 3 !== 0) {
    fail(`${source}: line count ${lines.length} is not a multiple of 3 — truncated or reformatted feed`)
  }
  const elements = []
  const rejected = []
  for (let i = 0; i < lines.length; i += 3) {
    const name = lines[i].replace(/^0 /, '').trim()
    const l1 = lines[i + 1].trimEnd()
    const l2 = lines[i + 2].trimEnd()
    if (name.startsWith('1 ') || name.startsWith('2 ')) {
      fail(`${source}: line ${i + 1}: expected a name line, got an element line`)
    }
    if (!l1.startsWith('1 ')) fail(`${source}: line ${i + 2}: expected TLE line 1`)
    if (!l2.startsWith('2 ')) fail(`${source}: line ${i + 3}: expected TLE line 2`)
    const c = candidate(name, l1, l2, source)
    if (c) elements.push(c)
    else rejected.push(name)
  }
  return { elements, rejected }
}

/// Parse SatNOGS `/api/tle/` — already split into tle0/tle1/tle2, so there is
/// no 3LE structure to be surprised by, only per-bird integrity. Records are
/// keyed by their RAW `norad_cat_id` (the asymmetry in the header): SatNOGS
/// files 176 placeholder birds under the placeholder and NONE under the
/// follow id.
export function parseSatnogsTle(json, source = 'satnogs-tle') {
  const arr = asArray(json, source)
  const elements = []
  const rejected = []
  for (const r of arr) {
    const l1 = String(r?.tle1 ?? '').trimEnd()
    const l2 = String(r?.tle2 ?? '').trimEnd()
    const name = String(r?.tle0 ?? '').replace(/^0 /, '').trim()
    if (!l1.startsWith('1 ') || !l2.startsWith('2 ')) {
      rejected.push(name || String(r?.norad_cat_id ?? '?'))
      continue
    }
    const c = candidate(name, l1, l2, source)
    if (!c) {
      rejected.push(name || String(r?.norad_cat_id ?? '?'))
      continue
    }
    // The record's own key wins over the line's — that is what a SatNOGS
    // lookup will match — but they agree in every observed record.
    const key = Number(r?.norad_cat_id)
    elements.push({ ...c, key: Number.isInteger(key) && key > 0 ? key : c.norad })
  }
  return { elements, rejected }
}

function asArray(json, what) {
  let v
  try {
    v = JSON.parse(json)
  } catch (e) {
    fail(`${what}: response was not valid JSON: ${e.message}`)
  }
  if (!Array.isArray(v)) fail(`${what}: response was not a JSON array (API shape change?)`)
  return v
}

const inAmateurBand = (hz) => {
  if (!Number.isFinite(hz) || hz <= 0) return false
  const mhz = hz / 1e6
  return AMATEUR_BANDS_MHZ.some(([lo, hi]) => mhz >= lo && mhz <= hi)
}

/// Is a bird still worth a catalog ROW? Everything except the historical
/// tail: alive, dead, pre-launch and unrecognised statuses all stay listed
/// (an unseen upstream value degrades to a label, exactly as the client's
/// status chip does), and only a re-entry older than [`RECENT_DECAY_DAYS`]
/// drops out — by then it has been gone for half a year and its elements
/// could not be used anyway.
function listable(status, decayed, updated, now) {
  if (status !== 're-entered') return true
  const stamp = Date.parse(decayed || updated || '')
  if (!Number.isFinite(stamp)) return false // no date at all = long gone
  return (now - stamp / 1000) / 86400 <= RECENT_DECAY_DAYS
}

/// Derive the amateur population from the two SatNOGS catalogs. A satellite
/// LISTS when it carries at least one amateur transmitter record — declared
/// `service: "Amateur"` or transmitting in an ITU amateur-satellite
/// allocation — and [`listable`] still counts it as news. It is ACTIVE (the
/// element-bearing tier; see the TWO TIERS note in the header) only when
/// `status == "alive"` AND one of those transmitters is itself `alive`.
///
/// `satellite.status` is authoritative over `transmitter.alive`: a dead bird
/// is inactive however lively its transmitter records look (IO-117 is the live
/// example — `status: "dead"` while its transmitter still says `alive: true`),
/// and a bird whose every transmitter is silent is inactive however healthy
/// the orbit is (AO-85). Neither loses its row — that is the whole point:
/// "this bird stopped working" is a fact the operator is owed, and a bird that
/// disappears from the catalog cannot report anything.
///
/// Transmitters join by `sat_id` when both sides carry one — the stable key —
/// falling back to `norad_cat_id` for records that do not.
/// `now` is REQUIRED (unix seconds): the re-entry window is a clock decision,
/// and a default here would make a committed fixture's expected population
/// change with the calendar.
export function deriveAmateurCatalog(satellites, transmitters, now) {
  const bySatId = new Map()
  const byNorad = new Map()
  for (const t of transmitters) {
    const claim = {
      service: t?.service === 'Amateur',
      band: inAmateurBand(Number(t?.downlink_low)),
      live: t?.alive === true,
    }
    if (!claim.service && !claim.band) continue
    if (typeof t.sat_id === 'string' && t.sat_id) push(bySatId, t.sat_id, claim)
    if (Number.isInteger(t.norad_cat_id)) push(byNorad, t.norad_cat_id, claim)
  }
  const birds = []
  let alive = 0
  for (const s of satellites) {
    const cat = Number.isInteger(s?.norad_cat_id) ? s.norad_cat_id : null
    if (cat === null) continue // one live record has a null NORAD — not a bird
    const status = String(s.status ?? '')
    if (status === 'alive') alive++
    const claims =
      (typeof s.sat_id === 'string' && bySatId.get(s.sat_id)) || byNorad.get(cat) || null
    if (!claims) continue // never an amateur bird — not ours to list
    if (!listable(status, s.decayed, s.updated, now)) continue
    const follow = Number.isInteger(s?.norad_follow_id) ? s.norad_follow_id : null
    // The clause census measures the LIVE claims: it exists to prove the
    // predicate that selects the active set is still finding birds.
    const live = claims.filter((c) => c.live)
    birds.push({
      // The identity: the catalogued number once USSPACECOM assigns one.
      norad: follow ?? cat,
      // The id SatNOGS itself files this bird under — /api/tle/ keys on it.
      satnogsNorad: cat,
      name: String(s.name ?? '').trim(),
      status,
      decayed: Boolean(s.decayed),
      /// Something is still transmitting on an amateur allocation.
      amateur: live.length > 0,
      /// …and the orbit is alive: the element-bearing tier.
      active: status === 'alive' && live.length > 0,
      serviceAmateur: live.some((c) => c.service),
      bandAmateur: live.some((c) => c.band),
    })
  }
  // A bird can be filed twice — once under a placeholder carrying the follow
  // id, once under the catalogued number itself. Collapse on the published
  // identity, keeping the PLACEHOLDER record: its `satnogsNorad` is the key
  // /api/tle/ actually files elements under.
  const seen = new Map()
  for (const b of birds) {
    const prev = seen.get(b.norad)
    if (!prev || (prev.satnogsNorad === prev.norad && b.satnogsNorad !== b.norad)) {
      seen.set(b.norad, b)
    }
  }
  const deduped = [...seen.values()]
  return {
    birds: deduped,
    stats: {
      alive,
      listed: deduped.length,
      active: deduped.filter((b) => b.active).length,
      serviceOnly: deduped.filter((b) => b.serviceAmateur).length,
      bandOnly: deduped.filter((b) => b.bandAmateur).length,
    },
  }
}

/// A name that can always be written down. Upstream records do come through
/// nameless; every consumer of this payload — the 3LE serializer above all —
/// needs SOMETHING, and the catalog number is the honest fallback.
const label = (name, norad) => (name && name.trim()) || `NORAD ${norad}`

function push(map, key, v) {
  const cur = map.get(key)
  if (cur) cur.push(v)
  else map.set(key, [v])
}

function parseNow(env) {
  if (!env) return Date.now() / 1000
  const n = Number(env)
  if (Number.isFinite(n) && n > 0) return n
  const t = Date.parse(env)
  if (Number.isFinite(t)) return t / 1000
  fail(`TLES_NOW=${env} is neither unix seconds nor an ISO date`)
}

/// The whole pipeline minus I/O: five source payloads in, a gated manifest
/// out. Throws [`GateError`] on any refusal — the caller publishes nothing.
export function assemble({
  satellites,
  transmitters,
  satnogsTle,
  ctAmateur,
  ctSatnogs,
  now,
  prevCount = 0,
  prevCatalog = [],
  ratchetOverride = false,
  limits = PROD_LIMITS,
}) {
  const log = []
  const say = (m) => {
    log.push(m)
    return m
  }

  // --- gate 2: the amateur derivation ---------------------------------------
  const { birds, stats } = deriveAmateurCatalog(
    asArray(satellites, 'satnogs satellites'),
    asArray(transmitters, 'satnogs transmitters'),
    now,
  )
  say(
    `amateur population ${stats.listed} listed, ${stats.active} active of ${stats.alive} alive ` +
      `(service clause ${stats.serviceOnly}, band clause ${stats.bandOnly})`,
  )
  if (stats.listed < limits.minCatalog || stats.listed > limits.maxCatalog) {
    fail(
      `amateur population ${stats.listed} outside [${limits.minCatalog}, ${limits.maxCatalog}] — the join broke`,
    )
  }
  const share = stats.alive ? stats.active / stats.alive : 0
  if (share < limits.minAmateurShare || share > limits.maxAmateurShare) {
    fail(
      `amateur share ${(share * 100).toFixed(1)}% of the alive catalog outside ` +
        `[${limits.minAmateurShare * 100}%, ${limits.maxAmateurShare * 100}%] — the join stopped discriminating`,
    )
  }
  if (stats.serviceOnly < limits.minServiceOnly) {
    fail(`only ${stats.serviceOnly} birds match service=="Amateur" — has the field gone empty upstream?`)
  }
  if (stats.bandOnly < limits.minBandOnly) {
    fail(`only ${stats.bandOnly} birds match an amateur band — has downlink_low changed shape?`)
  }

  // --- gates 1 + 7: the element sources -------------------------------------
  const legs = [
    ['celestrak-amateur', parse3le(ctAmateur, 'celestrak-amateur')],
    ['celestrak-satnogs', parse3le(ctSatnogs, 'celestrak-satnogs')],
    ['satnogs-tle', parseSatnogsTle(satnogsTle)],
  ]
  const pool = new Map() // NORAD (as each source files it) → best candidate
  let candidates = 0
  let rejected = 0
  for (const [name, leg] of legs) {
    candidates += leg.elements.length + leg.rejected.length
    rejected += leg.rejected.length
    if (leg.rejected.length) {
      say(`${name}: rejected ${leg.rejected.length} bird(s) on line integrity: ${leg.rejected.join(', ')}`)
    }
    for (const c of leg.elements) {
      const key = c.key ?? c.norad
      const prev = pool.get(key)
      // Newest epoch wins across sources; a tie keeps the earlier leg, which
      // is the more authoritative one.
      if (!prev || c.epoch > prev.epoch) pool.set(key, c)
    }
  }
  // A bad bird drops alone; a bad PAYLOAD (truncation, format drift) takes
  // the run. One drop is always tolerated — see gate 7 in the header.
  const rejectBudget = Math.max(1, Math.floor(candidates * 0.02))
  if (rejected > rejectBudget) {
    fail(
      `${rejected} of ${candidates} candidate elements failed integrity checks ` +
        `(budget ${rejectBudget}) — the payload is bad, not the birds`,
    )
  }

  // --- the union: elements for exactly the ACTIVE population ----------------
  // Both ids are tried, per the asymmetry rule in the header: Celestrak files
  // the catalogued number, SatNOGS the placeholder.
  const catalog = []
  const chosen = new Map() // published NORAD → element
  const uncovered = []
  for (const b of birds) {
    // Only the ACTIVE tier looks for elements (see TWO TIERS in the header):
    // a dead, re-entered, pre-launch or gone-silent bird gets its row and
    // nothing else. That row is what keeps a ★ readable and clickable after
    // the bird stops working; elements for it would be a fiction on a decayed
    // orbit and a distraction on a mute one.
    let best = null
    if (b.active) {
      const ids = b.satnogsNorad === b.norad ? [b.norad] : [b.norad, b.satnogsNorad]
      for (const id of ids) {
        const c = pool.get(id)
        if (c && (!best || c.epoch > best.epoch)) best = c
      }
      // Active and unfindable is the one worth naming in the job log — the
      // rest are listed without elements ON PURPOSE.
      if (!best) uncovered.push(b)
    }
    if (!best) {
      catalog.push({
        norad: b.norad,
        name: label(b.name, b.norad),
        status: b.status,
        amateur: b.amateur,
        decayed: b.decayed,
      })
      continue
    }
    // The element line's NORAD is the published identity — the client keys
    // birds by it (gate 9).
    const prev = chosen.get(best.norad)
    if (prev && prev.epoch >= best.epoch) continue // already drawn, freshest kept
    // A SatNOGS record may carry an empty `tle0`, and the 3LE form has nowhere
    // to put a nameless bird: serializing one writes a blank line that the
    // re-parse drops, failing gate 10 for the whole run over one upstream
    // typo. The catalog's own name stands in, and the NORAD behind that.
    const name = label(best.name || b.name, best.norad)
    chosen.set(best.norad, { ...best, name })
    catalog.push({
      norad: best.norad,
      name,
      status: b.status,
      amateur: b.amateur,
      decayed: b.decayed,
      src: best.source,
    })
  }
  // A second SatNOGS record resolving onto an element already drawn would
  // leave a duplicate catalog row behind; collapse on the published NORAD.
  const rows = new Map()
  for (const row of catalog) {
    const prev = rows.get(row.norad)
    if (!prev || (!prev.src && row.src)) rows.set(row.norad, row)
  }
  const finalCatalog = [...rows.values()].sort((a, b) => a.norad - b.norad)
  const els = [...chosen.values()].sort((a, b) => a.norad - b.norad)
  if (uncovered.length) {
    say(
      `uncovered: ${uncovered.length} ACTIVE amateur bird(s) have no current elements from any source ` +
        `— ${uncovered.map((b) => `${b.name} (${b.norad})`).join(', ')}`,
    )
  }
  const listedOnly = finalCatalog.filter((r) => !r.src).length - uncovered.length
  say(`listed without elements: ${listedOnly} bird(s) outside the active tier (dead, re-entered, pre-launch, silent)`)
  // The count ratchet is blind to a SWAP: 50 birds curated out and 50 new ones
  // in passes it silently. The previous manifest names its birds, so name the
  // ones that are gone — the job log is where a curation regression is caught.
  const listedNow = new Set(finalCatalog.map((r) => r.norad))
  // The one place the derivation can disagree with the list Nexus used to
  // ship: an object Celestrak files under GROUP=amateur that SatNOGS gives no
  // amateur transmitter at all. Two of 97, measured — but a silent drop of
  // birds operators already had is exactly what must not happen quietly.
  const unlisted = legs[0][1].elements.filter((c) => !listedNow.has(c.norad))
  if (unlisted.length) {
    say(
      `celestrak GROUP=amateur carries ${unlisted.length} object(s) the SatNOGS derivation does not ` +
        `list (no amateur transmitter on record): ${unlisted.map((c) => `${c.name} (${c.norad})`).join(', ')}`,
    )
  }
  const departed = prevCatalog.filter((r) => !listedNow.has(r.norad))
  if (departed.length) {
    const names = departed.slice(0, 25).map((r) => `${r.name} (${r.norad})`).join(', ')
    say(
      `departed: ${departed.length} bird(s) in the previous catalog are no longer listed — ` +
        `${names}${departed.length > 25 ? ', …' : ''}`,
    )
  }
  const sources = {}
  for (const e of els) sources[e.source] = (sources[e.source] ?? 0) + 1

  // --- gates 3 + 4: plausible count, ratcheted ------------------------------
  const floor = Math.max(limits.minElements, Math.ceil(prevCount * 0.85))
  if (els.length < floor) {
    if (!ratchetOverride) {
      fail(
        `only ${els.length} birds (< the ratchet floor max(${limits.minElements}, 0.85×${prevCount}) = ${floor}) — ` +
          `set TLES_RATCHET_OVERRIDE=1 on a dispatch run if this shrink is intended`,
      )
    }
    say(`RATCHET OVERRIDE: ${els.length} birds published against a baseline of ${prevCount} (floor ${floor})`)
  } else if (ratchetOverride) {
    say(`RATCHET OVERRIDE set but not needed: ${els.length} birds clears the floor of ${floor}`)
  }
  if (els.length > limits.maxElements) {
    fail(`${els.length} birds — that is not the amateur population`)
  }

  // --- gates 5 + 6: unique NORADs, canaries ---------------------------------
  const norads = new Set()
  for (const e of els) {
    if (norads.has(e.norad)) fail(`${e.name}: duplicate NORAD ${e.norad}`)
    norads.add(e.norad)
  }
  if (!norads.has(25544)) {
    fail('canary NORAD 25544 (ISS) missing — wrong sources or a mangled payload')
  }
  const present = WORKED_CANARIES.filter(([n]) => norads.has(n))
  if (present.length < WORKED_CANARY_MIN) {
    const missing = WORKED_CANARIES.filter(([n]) => !norads.has(n)).map(([n, s]) => `${s} (${n})`)
    fail(
      `worked-bird canary: only ${present.length} of ${WORKED_CANARIES.length} present — missing ${missing.join(', ')}`,
    )
  }

  // --- gate 8: epoch freshness ---------------------------------------------
  const ages = els.map((e) => (now - e.epoch) / 86400).sort((a, b) => a - b)
  const median = ages[Math.floor(ages.length / 2)]
  const p90 = ages[Math.min(ages.length - 1, Math.floor(ages.length * 0.9))]
  const under3 = ages.filter((a) => a < 3).length
  if (median > 7) fail(`median epoch age ${median.toFixed(1)} d (> 7 d) — the feed has gone stale`)
  if (p90 > 30) fail(`p90 epoch age ${p90.toFixed(1)} d (> 30 d) — too much of the set is stale`)
  if (under3 * 10 < ages.length * 6) {
    fail(
      `only ${under3} of ${ages.length} birds under 3 d old (< 60%) — the CLIENT's validate_tles ` +
        `would refuse this set, so publishing it would age every install for nothing`,
    )
  }

  // --- gate 9: identity -----------------------------------------------------
  for (const row of finalCatalog) {
    if (!row.src) continue
    const e = chosen.get(row.norad)
    if (!e) fail(`catalog ${row.norad} claims elements it has not got`)
    if (e.norad !== row.norad) fail(`catalog ${row.norad} carries element ${e.norad}`)
  }
  const active = new Set(finalCatalog.filter((r) => r.src).map((r) => r.norad))
  for (const e of els) {
    if (!active.has(e.norad)) fail(`${e.name} (${e.norad}) is published but not in the amateur catalog`)
  }

  // --- gate 10: round-trip --------------------------------------------------
  const elements = els.map((e) => ({ name: e.name, line1: e.line1, line2: e.line2 }))
  const threeLe = elements.map((e) => `${e.name}\n${e.line1}\n${e.line2}`).join('\n') + '\n'
  const reparsed = parse3le(threeLe, 'round-trip').elements.map((e) => ({
    name: e.name,
    line1: e.line1,
    line2: e.line2,
  }))
  if (JSON.stringify(reparsed) !== JSON.stringify(elements)) {
    fail('round-trip re-parse mismatch — parser drift')
  }

  say(
    `ok: ${elements.length} birds with elements of ${finalCatalog.length} listed ` +
      `(${Object.entries(sources).map(([k, v]) => `${k} ${v}`).join(', ')}), ` +
      `median epoch age ${median.toFixed(2)} d, p90 ${p90.toFixed(2)} d, ${under3} under 3 d`,
  )

  return {
    manifest: {
      schema: 2,
      // The real publication stamp; TLES_NOW only steers the freshness gate.
      generated: new Date().toISOString(),
      source: 'satnogs amateur population × celestrak amateur+satnogs groups + satnogs tle',
      // Travels INSIDE the payload so it survives the file being copied out of
      // the release asset — the same rule NOTICE states for the repo's other
      // share-alike source.
      attribution: {
        license: 'CC-BY-SA-4.0',
        text:
          'Satellite population, names and status derived from the SatNOGS DB ' +
          '(https://db.satnogs.org) © SatNOGS / Libre Space Foundation contributors, ' +
          'CC BY-SA 4.0. Orbital elements courtesy of CelesTrak (https://celestrak.org), ' +
          'Dr. T.S. Kelso, and the SatNOGS TLE API. This derived catalog is offered ' +
          'under CC BY-SA 4.0.',
      },
      count: elements.length,
      catalogCount: finalCatalog.length,
      medianEpochAgeDays: +median.toFixed(2),
      sources,
      elements,
      catalog: finalCatalog,
    },
    threeLe,
    log,
  }
}

/// ONE fetch, no retry. A 403/404 is Celestrak's (and SatNOGS') "stop asking":
/// retrying is how an IP reaches their firewall, so it fails the run and waits
/// for a human, exactly as their M2M policy asks.
async function fetchOnce(url) {
  console.error(`fetching ${url} …`)
  const resp = await fetch(url, { headers: { 'user-agent': UA } })
  if (resp.status === 403 || resp.status === 404) {
    fail(`HTTP ${resp.status} from ${url} — a refusal, NOT retried; investigate before the next run`)
  }
  if (!resp.ok) fail(`HTTP ${resp.status} from ${url}`)
  return resp.text()
}

const sleep = (s) => new Promise((r) => setTimeout(r, s * 1000))

/// Read the five sources: from `TLES_FIXTURE_DIR` when set, else the network,
/// strictly sequentially with a gap between requests.
async function loadSources(fixtureDir) {
  const files = {
    satellites: 'satnogs-satellites.json',
    transmitters: 'satnogs-transmitters.json',
    satnogsTle: 'satnogs-tle.json',
    ctAmateur: 'celestrak-amateur.tle',
    ctSatnogs: 'celestrak-satnogs.tle',
  }
  if (fixtureDir) {
    const out = {}
    for (const [k, f] of Object.entries(files)) out[k] = readFileSync(join(fixtureDir, f), 'utf8')
    return out
  }
  const urls = {
    satellites: SATNOGS_SATELLITES,
    transmitters: SATNOGS_TRANSMITTERS,
    satnogsTle: SATNOGS_TLE,
    ctAmateur: CELESTRAK_AMATEUR,
    ctSatnogs: CELESTRAK_SATNOGS,
  }
  const out = {}
  let first = true
  for (const [k, url] of Object.entries(urls)) {
    if (!first) await sleep(FETCH_GAP_SECS)
    first = false
    out[k] = await fetchOnce(url)
  }
  return out
}

async function main() {
  const outDir = process.argv[2] || process.cwd()
  const fixtureDir = process.env.TLES_FIXTURE_DIR || null
  if (fixtureDir) console.error(`FIXTURE RUN: sources from ${fixtureDir} (count floors scaled down)`)

  const src = await loadSources(fixtureDir)
  let prevCount = 0
  let prevCatalog = []
  if (process.env.TLES_PREV) {
    try {
      const prev = JSON.parse(readFileSync(process.env.TLES_PREV, 'utf8'))
      prevCount = prev.count | 0
      if (Array.isArray(prev.catalog)) prevCatalog = prev.catalog
    } catch {
      console.error('no previous manifest — the count gate uses the fixed floor only')
    }
  }

  const { manifest, threeLe, log } = assemble({
    ...src,
    now: parseNow(process.env.TLES_NOW),
    prevCount,
    prevCatalog,
    ratchetOverride: process.env.TLES_RATCHET_OVERRIDE === '1',
    limits: fixtureDir ? FIXTURE_LIMITS : PROD_LIMITS,
  })
  for (const line of log) console.error(line)

  writeFileSync(join(outDir, 'tles.json'), JSON.stringify(manifest) + '\n')
  writeFileSync(join(outDir, 'tles.txt'), threeLe)
}

const entry = process.argv[1] && realpathSync(process.argv[1])
if (entry === realpathSync(fileURLToPath(import.meta.url))) {
  main().catch((e) => {
    console.error(e instanceof GateError ? `GATE FAILED: ${e.message}` : e)
    process.exit(1)
  })
}
