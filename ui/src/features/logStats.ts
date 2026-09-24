// Descriptive analytics over the logbook — a pure roll-up of getLog()'s LoggedQso[] into the
// counts a "my ham life" dashboard shows. No React, no IO, fully node-testable. Deliberately
// distinct from the Journey layer (gamified goals) and Awards (official credit): this is just the
// operator's log, sliced. Continent / CQ-zone / POTA breakdowns need the cty.dat resolver + the
// ota field, which live only in the Rust layer — a backend get_log_stats supplies those later.

import type { LoggedQso } from '../types'

/** A labelled count for a bar chart. */
export interface Tally {
  label: string
  count: number
}

export interface LogStats {
  total: number
  /** Distinct callsigns worked (case-insensitive). */
  uniqueCalls: number
  /** Worked on any confirmation channel. */
  confirmed: number
  /** Award-grade confirmed (LoTW / paper). */
  awardConfirmed: number
  /** Distinct DXCC entities (resolved `country`) in the log. */
  dxccEntities: number
  /** QSOs by band, most-worked first. */
  byBand: Tally[]
  /** QSOs by mode, most-worked first. */
  byMode: Tally[]
  /** QSOs by UTC year, oldest first (the time axis). */
  byYear: Tally[]
  /** QSOs by US state (WAS), most-worked first. */
  byState: Tally[]
  /** Most-worked DXCC entities (top slice), most first. */
  topEntities: Tally[]
  /** QSOs by UTC hour-of-day, index 0..23 — counting only QSOs with a KNOWN time-of-day
   * (see `hourUnknown`). */
  hourUtc: number[]
  /** QSOs with no real time-of-day (logged at exactly 00:00:00 UTC — the hallmark of a QRZ/LoTW
   * import, which carries the date but not the time). Excluded from `hourUtc` so the histogram
   * shows the operator's actual on-air pattern instead of a spike at midnight. */
  hourUnknown: number
  /** Confirmation channels — how many QSOs carry each QSL source. */
  qsl: { card: number; lotw: number; eqsl: number }
}

/** The statistics COUNTED and not yet ordered — what the engine answers the `statistics` question
 * with (SPEC-2 v3 C17a), and what `finishLogStats` turns into `LogStats`. Each tally is in
 * first-seen order, and `entities` holds every entity, not only the top twelve.
 *
 * Ordering is the window's job, not the engine's: ties are broken by `localeCompare`, which orders
 * labels the way THIS webview's locale does ("Åland Islands" beside "Aland Islands", lower case
 * beside upper). Nothing else in the dashboard depends on the locale, so the counting can move and
 * the ordering stays exactly where it was. */
export interface LogStatCounts {
  total: number
  uniqueCalls: number
  confirmed: number
  awardConfirmed: number
  dxccEntities: number
  byBand: Tally[]
  byMode: Tally[]
  byYear: Tally[]
  byState: Tally[]
  /** Every entity (resolved, or the stored country), grouped as `topEntities` groups them. */
  entities: Tally[]
  hourUtc: number[]
  hourUnknown: number
  qsl: { card: number; lotw: number; eqsl: number }
}

/** Count occurrences of a key extracted from each QSO, dropping blanks. */
function tallyBy(log: LoggedQso[], key: (q: LoggedQso) => string | null | undefined): Map<string, number> {
  const m = new Map<string, number>()
  for (const q of log) {
    const k = key(q)?.trim()
    if (k) m.set(k, (m.get(k) ?? 0) + 1)
  }
  return m
}

/** The mode label the "By mode" roll-up counts a QSO under: the sidebands fold into SSB.
 *
 * A log holds SSB, USB and LSB rows for one mode — from any logger that writes the sideband in
 * MODE (Log4OM and N1MM both do), and now from Nexus's own phone contacts, which carry the
 * sideband they were worked on. Tallying the raw spelling split the bar list into three
 * entries, none of which was the operator's phone total. Nothing else folds: FM and AM are
 * modes in their own right, not sidebands.
 *
 * ⛔ DELIBERATELY NOT `callHistory.modeKey`, which is the same fold for the live UI. This
 * module is a LEAF on purpose: `remote/test/insights-reference.mjs` transpiles it standalone
 * and runs it as an independent comparator for the native Remote summary, with an explicit
 * dependency allow-list. Importing `modeKey` drags in `callHistory` and its own `../band`
 * import, and the guard goes red — correctly. Three lines of duplication is the cheaper side
 * of that trade; the fold is also spelled out in `logbook.rs dedup_mode` and four other
 * places, so this is the shape the codebase already has.
 *
 * Every other spelling is passed through UNCHANGED, case included — this folds the sidebands
 * and nothing else. Uppercasing the rest would quietly merge "ssb" with "SSB" too, a wider
 * change than the defect asks for, and it would diverge from the Remote-side roll-up in
 * `remote_service/query/insights.rs`, which the native Remote gate compares against this one. */
function phoneModeLabel(mode: string | null | undefined): string {
  const m = (mode ?? '').trim()
  const u = m.toUpperCase()
  return u === 'USB' || u === 'LSB' ? 'SSB' : m
}

/**
 * Case-insensitive tally: groups by the uppercased key (so an imported "UNITED STATES" and a
 * Nexus-resolved "United States" count as one entity, matching the `dxccEntities` headline), but
 * labels each bucket with the first-seen casing so the display stays readable.
 */
function tallyByCI(
  log: LoggedQso[],
  key: (q: LoggedQso) => string | null | undefined,
): Map<string, number> {
  const counts = new Map<string, number>()
  const labels = new Map<string, string>()
  for (const q of log) {
    const raw = key(q)?.trim()
    if (!raw) continue
    const u = raw.toUpperCase()
    counts.set(u, (counts.get(u) ?? 0) + 1)
    if (!labels.has(u)) labels.set(u, raw)
  }
  const out = new Map<string, number>()
  for (const [u, c] of counts) out.set(labels.get(u) ?? u, c)
  return out
}

// WAS is a US-only award. Mirror crates/propagation/src/awards.rs: gate on a
// US-family DXCC entity (United States / Alaska / Hawaii, resolved into
// `q.country`) so foreign subdivision codes that collide with US postal codes
// (e.g. Australian "WA" = Western Australia, Brazilian "SC"/"PA") don't pollute
// the breakdown, then canonicalize to one of the 50 valid WAS codes. Mapping to
// the canonical code also folds casing, so "ct" and "CT" land in one bucket.
const US_ENTITIES = new Set(['UNITED STATES', 'ALASKA', 'HAWAII'])
const WAS_STATES = new Set([
  'AK', 'AL', 'AR', 'AZ', 'CA', 'CO', 'CT', 'DE', 'FL', 'GA', 'HI', 'IA', 'ID', 'IL', 'IN', 'KS',
  'KY', 'LA', 'MA', 'MD', 'ME', 'MI', 'MN', 'MO', 'MS', 'MT', 'NC', 'ND', 'NE', 'NH', 'NJ', 'NM',
  'NV', 'NY', 'OH', 'OK', 'OR', 'PA', 'RI', 'SC', 'SD', 'TN', 'TX', 'UT', 'VA', 'VT', 'WA', 'WI',
  'WV', 'WY',
])

/** ADIF STATE → a valid WAS code, but only for US-family entities. `null` otherwise. */
function wasState(q: LoggedQso): string | null {
  if (!US_ENTITIES.has(q.country?.trim().toUpperCase() ?? '')) return null
  const code = q.state?.trim().toUpperCase()
  return code && WAS_STATES.has(code) ? code : null
}

/** Tallies by count descending (ties broken by label for stability). */
export const compareTallies = (a: Tally, b: Tally): number => b.count - a.count || a.label.localeCompare(b.label)
/** Map → Tally[], in the map's order (first seen). */
function tallies(m: Map<string, number>): Tally[] {
  return [...m.entries()].map(([label, count]) => ({ label, count }))
}
function byCountDesc(t: readonly Tally[]): Tally[] {
  return [...t].sort(compareTallies)
}

/** Count a logbook for the dashboard — one pass, nothing ordered (`finishLogStats` orders). Pure. */
export function countLogStats(log: LoggedQso[]): LogStatCounts {
  const hourUtc = new Array(24).fill(0) as number[]
  let hourUnknown = 0
  const calls = new Set<string>()
  const countries = new Set<string>()
  let confirmed = 0
  let awardConfirmed = 0
  const qsl = { card: 0, lotw: 0, eqsl: 0 }

  for (const q of log) {
    calls.add(q.call.trim().toUpperCase())
    // The RESOLVED entity when the backend supplied one, country as legacy
    // fallback — counting raw country strings tallied "Germany" and "Fed. Rep.
    // of Germany" (and three Russias) as separate entities.
    const c = (q.entity ?? q.country)?.trim()
    if (c) countries.add(c.toUpperCase())
    if (q.confirmed) confirmed++
    if (q.awardConfirmed) awardConfirmed++
    if (q.qslRcvd?.card) qsl.card++
    if (q.qslRcvd?.lotw) qsl.lotw++
    if (q.qslRcvd?.eqsl) qsl.eqsl++
    if (Number.isFinite(q.whenUnix)) {
      // A QSO stamped at exactly 00:00:00 UTC has no real time-of-day — that is what a QRZ/LoTW
      // import writes (date, no time). Counting it as "midnight" buries the operator's genuine
      // activity pattern under an import spike, so it goes to `hourUnknown` instead.
      if (q.whenUnix % 86400 === 0) {
        hourUnknown++
      } else {
        const h = new Date(q.whenUnix * 1000).getUTCHours()
        if (h >= 0 && h < 24) hourUtc[h]++
      }
    }
  }

  const byYear = tallies(
    tallyBy(log, (q) => {
      if (!Number.isFinite(q.whenUnix)) return null
      const y = new Date(q.whenUnix * 1000).getUTCFullYear() // NaN for an out-of-range timestamp
      return Number.isFinite(y) ? String(y) : null
    }),
  )

  return {
    total: log.length,
    uniqueCalls: calls.size,
    confirmed,
    awardConfirmed,
    dxccEntities: countries.size,
    byBand: tallies(tallyBy(log, (q) => q.band)),
    byMode: tallies(tallyBy(log, (q) => phoneModeLabel(q.mode))),
    byYear,
    byState: tallies(tallyBy(log, wasState)),
    entities: tallies(tallyByCI(log, (q) => q.entity ?? q.country)),
    hourUtc,
    hourUnknown,
    qsl,
  }
}

/** Order counted statistics as the dashboard shows them: the bars most-worked first (ties by
 * label, in this webview's locale), the years chronologically, the twelve most-worked entities.
 * Copies what it orders — the counts are left as they were. Pure. */
export function finishLogStats(c: LogStatCounts): LogStats {
  return {
    total: c.total,
    uniqueCalls: c.uniqueCalls,
    confirmed: c.confirmed,
    awardConfirmed: c.awardConfirmed,
    dxccEntities: c.dxccEntities,
    byBand: byCountDesc(c.byBand),
    byMode: byCountDesc(c.byMode),
    byYear: [...c.byYear].sort((a, b) => a.label.localeCompare(b.label)), // chronological
    byState: byCountDesc(c.byState),
    topEntities: byCountDesc(c.entities).slice(0, 12),
    hourUtc: [...c.hourUtc],
    hourUnknown: c.hourUnknown,
    qsl: { ...c.qsl },
  }
}

/** Roll a logbook up into the descriptive-stats dashboard shape. Pure. */
export function computeLogStats(log: LoggedQso[]): LogStats {
  return finishLogStats(countLogStats(log))
}
