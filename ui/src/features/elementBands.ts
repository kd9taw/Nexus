// The set-wide element BANDS, said one way for every surface that shows them.
// `elementAgeDays` (the median) answers "is the typical bird current"; these
// counters answer the question a median cannot — how much of the catalog that
// typical bird is not speaking for. Pure, and shared by the Satellites header,
// the Now-Bar lane, the Connect Passes pane, the Settings fieldset and the
// refresh toast, so two surfaces can never describe one catalog differently
// (which is exactly how "TLE 26 days — STALE" survived beside a 0.2 d
// catalog).
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The
// words come from the catalog; the COUNTS and the 14 d / 30 d thresholds are
// the facts themselves and stay here. The two hand-rolled plurals below moved
// to `Intl.PluralRules` forms — English has two, Polish four, and the ternary
// is unrepresentable in most languages.
import { t } from '../i18n'

/** The three set-wide counters, carried identically by `TleStatus` (Settings +
 * the Now-Bar lane) and `SatView` (the Satellites chip + the Passes pane) —
 * one backend partition fills both. */
export interface ElementBands {
  /** Birds inside the 30 d ceiling — what the satellite surfaces draw on. */
  usableCount: number
  /** …of THOSE, the ones past the 14 d line: still drawn, and drifting. */
  agingCount: number
  /** Birds past the ceiling: they sit out entirely. Disjoint from
   * `usableCount`, so the two sum to every bird with a readable epoch. */
  heldBackCount: number
}

/** Every bird with a readable epoch — the denominator that turns a count into
 * a share. A bare count is the whole problem: "30 held back" reads identically
 * at 367 birds and at 60. */
const total = (b: ElementBands) => b.usableCount + b.heldBackCount

/** Birds the median does not speak for: past the 14 d line, whether they are
 * still in use and drifting or held out altogether. */
const pastLine = (b: ElementBands) => b.agingCount + b.heldBackCount

/** "89 of 140 past 14 d" — the degraded-set headline, identical on both loud
 * surfaces so an operator who sees it twice sees one fact, not two. */
export const elementsPastLineLabel = (b: ElementBands) =>
  t('sat.elements.pastLine', { past: pastLine(b), total: total(b) })

/** More than half the catalog past the 14 d line: the ONE threshold the loud
 * surfaces (the app-wide Now-Bar lane, the Connect planning pane) share.
 *
 * Deliberately a majority. The shipped catalog permanently carries a
 * slow-cadence tail — AO-7 and the SatNOGS birds re-observed every few weeks —
 * and a surface that warns about a healthy catalog teaches the operator to
 * stop reading it. The median cannot catch this case on its own: it stays calm
 * until half the USABLE birds cross the line, and the held-back ones never do,
 * so "half my birds sit at 29 d" reads exactly like a fresh set. */
export const elementsMostlyPastLine = (b: ElementBands) =>
  total(b) > 0 && pastLine(b) * 2 > total(b)

/** The exception bands, compact, for a chip or a one-line status. Empty when
 * every bird is current — a zero is not news. */
export function elementBandParts(b: ElementBands): string[] {
  const parts: string[] = []
  if (b.agingCount > 0) parts.push(t('sat.elements.parts.aging', { count: b.agingCount }))
  if (b.heldBackCount > 0) {
    parts.push(t('sat.elements.parts.heldBack', { count: b.heldBackCount }))
  }
  return parts
}

/** "367 birds · 30 sit out past 30 d" — the total first, so what follows reads
 * as a share of it rather than as a bare alarm. */
export const elementBandSummary = (b: ElementBands) =>
  [t('sat.elements.summary.birds', { count: total(b) }), ...elementBandParts(b)].join(' · ')

/** The same facts in prose, for the refresh toast and the Settings "Last
 * refresh" line: a leading space, appended after a headline; '' when there is
 * nothing to add. Same numbers as [`elementBandSummary`], different register —
 * that one labels a chip, this one finishes a sentence. */
export function elementBandSentence(b: ElementBands): string {
  // ZERO is not a plural form — it is silence, so the count gates the sentence
  // and `Intl.PluralRules` only picks between the forms that do get spoken.
  let s = ''
  if (b.agingCount > 0) s += ' ' + t('sat.elements.sentence.aging', { count: b.agingCount })
  if (b.heldBackCount > 0) {
    s += ' ' + t('sat.elements.sentence.heldBack', { count: b.heldBackCount })
  }
  return s
}
