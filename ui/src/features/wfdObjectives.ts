// ---------------------------------------------------------------------------
// Winter Field Day's objectives — the sponsor's thirteen, each with its objective
// multiplier (OM).
//
// This is a TS mirror of `wfd.objective_menu` in crates/tempo-core/src/fd_rules.seed.json
// (the authoritative list, which scores). A Rust test in fd_rules.rs reads this file and
// fails when an id, a multiplier or an `implies` here differs from the seed, so the
// checklist can never offer a box that scores something else.
//
// ⚠️ The LABELS are invariant and deliberately not catalog entries: each is the name the
// sponsor's own worksheet gives the objective (2027 rules, p.11), the box an entrant ticks
// on the sponsor's submission form. Like the ARRL bonus names, a translated one names
// nothing the sponsor scores.
//
// The score is the sponsor's p.7 formula, computed in Rust: QSO points × (OM + 1). The
// arithmetic below is for the checklist's own counts — what is ticked, what is planned —
// and reads the same `implies` the seed does: "Achieving this objective qualifies you for
// the 'Operate station equipment on alternative power' objective" (p.6) and "The six bands
// from the previous objective count toward this one" (p.7).
// ---------------------------------------------------------------------------
import type { FieldDayQso } from '../types'

export interface WfdObjective {
  id: string
  label: string
  multiplier: number
  /** Objectives this one earns with it, each counted once. */
  implies: readonly string[]
}

export const WFD_OBJECTIVES: readonly WfdObjective[] = [
  { id: 'wfd-alt-power-equipment', label: 'Operate station equipment on alternative power', multiplier: 1, implies: [] },
  { id: 'wfd-alt-power-100',       label: 'Operate 100% on alternative Power',               multiplier: 2, implies: ['wfd-alt-power-equipment'] },
  { id: 'wfd-away-from-home',      label: 'Operate away from home',                          multiplier: 3, implies: [] },
  { id: 'wfd-multiple-antennas',   label: 'Deploy multiple antennas',                        multiplier: 1, implies: [] },
  { id: 'wfd-sstv-image',          label: 'Send and receive the WFD SSTV image',             multiplier: 2, implies: [] },
  { id: 'wfd-crossband-repeater',  label: 'Make at least three contacts on a cross-band repeater', multiplier: 3, implies: [] },
  { id: 'wfd-winlink-email',       label: 'Send and receive at least one Winlink email',     multiplier: 1, implies: [] },
  { id: 'wfd-bulletin',            label: 'Copy the Winter Field Day Special Bulletin',      multiplier: 1, implies: [] },
  { id: 'wfd-six-bands',           label: 'Make at least 3 contacts on at least 6 different bands', multiplier: 6, implies: [] },
  { id: 'wfd-twelve-bands',        label: 'Make at least 3 contacts on at least 12 different bands', multiplier: 6, implies: ['wfd-six-bands'] },
  { id: 'wfd-multiple-modes',      label: 'Use multiple modes',                              multiplier: 2, implies: [] },
  { id: 'wfd-qrp',                 label: 'Operate the event QRP',                           multiplier: 4, implies: [] },
  { id: 'wfd-six-hours',           label: 'Operate six continuous hours during the event',   multiplier: 2, implies: [] },
]

/** The objective a QRP declaration agrees with — the one the Power-category note compares. */
export const WFD_QRP_OBJECTIVE = 'wfd-qrp'

const BY_ID = new Map(WFD_OBJECTIVES.map((o) => [o.id, o]))

/** The ids a ticked set earns: each ticked objective and what it implies, each once.
 *  Unknown ids (an ARRL bonus, a typo) earn nothing. */
export function wfdEarnedIds(ticked: readonly string[]): Set<string> {
  const earned = new Set<string>()
  for (const id of ticked) {
    const o = BY_ID.get(id)
    if (!o) continue
    earned.add(o.id)
    for (const i of o.implies) if (BY_ID.has(i)) earned.add(i)
  }
  return earned
}

/** Σ OM over a ticked set, each earned objective once. */
export function wfdObjectiveMultiplier(ticked: readonly string[]): number {
  let om = 0
  for (const id of wfdEarnedIds(ticked)) om += BY_ID.get(id)?.multiplier ?? 0
  return om
}

/** One objective's state: ticked, earned through another one it comes with, planned, or none.
 *  Earned wins over planned, exactly as the bonus checklist reads its two lists. */
export type WfdObjectiveState = 'earned' | 'implied' | 'planned' | 'none'

export function wfdObjectiveState(id: string, earned: readonly string[], planned: readonly string[]): WfdObjectiveState {
  if (earned.includes(id)) return 'earned'
  if (wfdEarnedIds(earned).has(id)) return 'implied'
  if (planned.includes(id)) return 'planned'
  return 'none'
}

/** The chase, in OM. `earnedOm` is the multiplier the score is made of; `plannedOm` is what
 *  the planned-but-not-yet-earned objectives would add (never double-counting one the ticked
 *  set already earns); the ceiling is both. */
export function wfdObjectiveTally(earned: readonly string[], planned: readonly string[]) {
  const earnedIds = wfdEarnedIds(earned)
  const earnedOm = wfdObjectiveMultiplier(earned)
  const ceilingOm = wfdObjectiveMultiplier([...earned, ...planned])
  return {
    earnedCount: earnedIds.size,
    earnedOm,
    plannedCount: [...wfdEarnedIds(planned)].filter((id) => !earnedIds.has(id)).length,
    plannedOm: ceilingOm - earnedOm,
    ceilingOm,
  }
}

/** What the log shows toward the objectives a log can evidence — hints only; the operator
 *  ticks, as the sponsor's form asks ("When you submit your log, we will ask you to select your
 *  completed objectives", p.7).
 *
 *  `bands` counts the bands holding at least three counting contacts ("Log at least three QSOs
 *  on at least six different bands", p.7); `modes` counts the mode classes worked ("at least
 *  one QSO on multiple modes … Phone and CW, CW and Digital, or Phone and Digital", p.7). A
 *  duplicate is logged but counts for nothing, so it counts for neither. */
export function wfdLogHints(log: readonly FieldDayQso[]): { bands: number; modes: number } {
  const perBand = new Map<string, number>()
  const modes = new Set<string>()
  for (const q of log) {
    if (q.dupe) continue
    const band = q.band.trim().toLowerCase()
    if (band) perBand.set(band, (perBand.get(band) ?? 0) + 1)
    const mode = (q.mode ?? '').trim().toUpperCase()
    if (mode) modes.add(mode)
  }
  let bands = 0
  for (const n of perBand.values()) if (n >= 3) bands++
  return { bands, modes: modes.size }
}
