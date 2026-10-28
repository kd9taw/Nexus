// The radio setup's answers, and what Finish writes from them. Pure data in, data out.
//
// The rule model is the one the engine already routes by (`Settings::route_radio`): each radio's band
// list (empty = every band), the ordered band+mode rules (first match wins) and the default radio. The
// setup WRITES those; it adds no model of its own. Its two screens map onto them:
//   - "Which radio for which band" is each radio's band list, exactly.
//   - "Where two radios share a band" is one cell per shared band and mode class, each naming a radio
//     or no rule, opened on what the engine does there now. Finish turns ONLY the cells the operator
//     changed into rules (one per mode and radio, every changed band with that answer in it), placed
//     ABOVE the station's rules so what the screen showed is what wins. Every stored rule stays below,
//     in its order; a rule naming a band and the mode of a changed cell loses just that band (and
//     goes when it has none left). So a band and mode nobody changed routes exactly as before,
//     including through a rule aimed at a radio that is turned off (the engine passes over it to the
//     next rule, and that next rule is still there).
//   - "Audio and keying" is each radio's receive and transmit sound card and keying method: the same
//     per-radio fields Settings' Edit of that radio writes, through the same per-radio verb.
// An untouched setup writes nothing: every part of the plan below is the difference from what is
// stored, and the rules are rebuilt only when a cell was changed.
import type { RadioProfile, RadioSwitchPolicy, RouteMode, RoutingRule, Settings } from '../types'
import type { RoutingDraft } from '../api'
import { ROUTE_MODES } from './describeRouting'

/** A radio's sound cards and keying, as the "Audio and keying" step edits them: the keying method
 *  and, for Serial RTS or DTR, the port the keying line is on (blank = the CAT port), as Settings. */
export interface RadioKit {
  audioIn: string
  audioOut: string
  pttMethod: string
  pttSerialPort: string
}
export const KIT_FIELDS: readonly (keyof RadioKit)[] = ['audioIn', 'audioOut', 'pttMethod', 'pttSerialPort']

export interface RadioSetupAnswers {
  /** Radio ids, in the station's order. */
  ids: number[]
  names: Record<number, string>
  enabled: Record<number, boolean>
  /** Each radio's band list; empty = every band (a catch-all). */
  bands: Record<number, string[]>
  /** For each band two radios list: per mode class, the radio, or null for no rule. */
  cells: Record<string, Partial<Record<RouteMode, number | null>>>
  /** What each cell opened on. A cell that differs from it is a change Finish writes; the others
   *  write nothing. */
  prefill: Record<string, Partial<Record<RouteMode, number | null>>>
  defaultRadio: number | null
  policy: RadioSwitchPolicy
  /** Each radio's sound cards and keying method. */
  kit: Record<number, RadioKit>
}

const sameList = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((x, i) => x === b[i])
/** A band list is a set: ticking a band off and on again is no change. */
const sameBands = (a: readonly string[], b: readonly string[]) =>
  a.length === b.length && a.every((x) => b.some((y) => y.toLowerCase() === x.toLowerCase()))

/** The radio a stored rule list gives (band, mode): the first plain rule that matches and names a
 *  radio that can take it, as the engine reads them (`Settings::route_radio` passes over a rule aimed
 *  at a radio that is turned off or gone). Null when no rule matches. */
export function ruleRadio(
  rules: readonly RoutingRule[],
  band: string,
  mode: RouteMode,
  usable: (id: number) => boolean = () => true,
): number | null {
  const hit = rules.find(
    (r) =>
      r.context !== 'satellite' &&
      (r.bands.length === 0 || r.bands.some((b) => b.toLowerCase() === band.toLowerCase())) &&
      (r.mode == null || r.mode === mode) &&
      usable(r.radio),
  )
  return hit ? hit.radio : null
}

/** Whether a band list holds `band`, read without case as the engine reads it (a stored "2M" is 2m). */
export const listsBand = (list: readonly string[] | undefined, band: string) =>
  (list ?? []).some((b) => b.toLowerCase() === band.toLowerCase())

/** Bands at least two radios that are on list by name, in `order`. A catch-all radio (no list) is not
 *  counted: a radio that lists the band already outranks it there. */
export function sharedBands(a: RadioSetupAnswers, order: readonly string[]): string[] {
  return order.filter((band) => a.ids.filter((id) => a.enabled[id] && listsBand(a.bands[id], band)).length >= 2)
}

export const kitOf = (r: RadioProfile): RadioKit => ({
  audioIn: r.audioIn ?? '',
  audioOut: r.audioOut ?? '',
  pttMethod: r.pttMethod ?? '',
  pttSerialPort: r.pttSerialPort ?? '',
})

/** The answers the stored settings already give: what the setup opens on. */
export function answersFrom(s: Settings, order: readonly string[]): RadioSetupAnswers {
  const radios: RadioProfile[] = s.radios ?? []
  const a: RadioSetupAnswers = {
    ids: radios.map((r) => r.id),
    names: Object.fromEntries(radios.map((r) => [r.id, r.name])),
    enabled: Object.fromEntries(radios.map((r) => [r.id, r.enabled !== false])),
    bands: Object.fromEntries(radios.map((r) => [r.id, [...r.bands]])),
    cells: {},
    prefill: {},
    defaultRadio: s.defaultRadio ?? null,
    policy: s.radioSwitchPolicy ?? 'act',
    kit: Object.fromEntries(radios.map((r) => [r.id, kitOf(r)])),
  }
  return withCells(a, s.routingRules ?? [], order)
}

/** Fill a cell for every shared band and mode that has none yet, from the stored rules, as the
 *  engine would route it with the radios on and off as the answers have them. Cells already answered
 *  keep their answer. */
export function withCells(a: RadioSetupAnswers, rules: readonly RoutingRule[], order: readonly string[]): RadioSetupAnswers {
  const usable = (id: number) => a.ids.includes(id) && a.enabled[id] !== false
  const cells = { ...a.cells }
  const prefill = { ...a.prefill }
  for (const band of sharedBands(a, order)) {
    const row = { ...(cells[band] ?? {}) }
    const was = { ...(prefill[band] ?? {}) }
    for (const [mode] of ROUTE_MODES) {
      if (mode in row) continue
      row[mode] = ruleRadio(rules, band, mode, usable)
      was[mode] = row[mode]
    }
    cells[band] = row
    prefill[band] = was
  }
  return { ...a, cells, prefill }
}

/** The cells the operator changed, on bands still shared: (band, mode) → the radio, or null. */
export function changedCells(a: RadioSetupAnswers, order: readonly string[]): { band: string; mode: RouteMode; radio: number | null }[] {
  const out: { band: string; mode: RouteMode; radio: number | null }[] = []
  for (const band of sharedBands(a, order)) {
    for (const [mode] of ROUTE_MODES) {
      const now = a.cells[band]?.[mode]
      if (now !== undefined && now !== (a.prefill[band]?.[mode] ?? null)) out.push({ band, mode, radio: now })
    }
  }
  return out
}

/** The rules Finish writes, in order. No changed cell: the stored rules, unchanged. */
export function compileRules(a: RadioSetupAnswers, stored: readonly RoutingRule[], order: readonly string[]): RoutingRule[] {
  const changed = changedCells(a, order)
  if (!changed.length) return [...stored]
  const out: RoutingRule[] = []
  for (const [mode] of ROUTE_MODES) {
    const byRadio = new Map<number, string[]>()
    for (const c of changed) {
      if (c.mode !== mode || c.radio == null) continue
      byRadio.set(c.radio, [...(byRadio.get(c.radio) ?? []), c.band])
    }
    for (const radio of [...byRadio.keys()].sort((x, y) => x - y)) {
      out.push({ bands: byRadio.get(radio)!, mode, context: null, radio })
    }
  }
  // Every stored rule stays, in order. A rule for one mode that names the band of a changed cell of
  // that mode loses that band — the cell now says what happens there — and goes when none is left.
  // A rule for every mode, for every band, or for satellites is kept whole: it also answers bands
  // and modes nobody changed (and "No rule" leaves it in charge, as the step says).
  const isChanged = (band: string, mode: RouteMode) =>
    changed.some((c) => c.mode === mode && c.band.toLowerCase() === band.toLowerCase())
  const kept: RoutingRule[] = []
  for (const r of stored) {
    const mode = r.mode
    if (r.context === 'satellite' || mode == null || r.bands.length === 0) {
      kept.push(r)
      continue
    }
    const bands = r.bands.filter((b) => !isChanged(b, mode))
    if (bands.length === r.bands.length) kept.push(r)
    else if (bands.length) kept.push({ ...r, bands })
  }
  return [...out, ...kept]
}

/** The routing as the setup shows it, for "Where would this go?" (the backend lays it over a copy of
 *  the settings and asks the engine's own routing function). */
export function routingDraft(a: RadioSetupAnswers, stored: readonly RoutingRule[], order: readonly string[]): RoutingDraft {
  return {
    radios: a.ids.map((id) => ({ id, bands: a.bands[id] ?? [], enabled: a.enabled[id] !== false })),
    rules: compileRules(a, stored, order),
    defaultRadio: a.defaultRadio,
  }
}

/** Everything Finish writes: only what differs from the stored settings. Empty = nothing to write. */
export interface FinishPlan {
  renames: { id: number; name: string }[]
  enabled: { id: number; enabled: boolean }[]
  bands: { id: number; bands: string[] }[]
  /** The full rule list, or null to leave the stored one alone. */
  rules: RoutingRule[] | null
  /** The new default (null = stay on the radio in use), or undefined to leave it. */
  defaultRadio?: number | null
  policy?: RadioSwitchPolicy
  /** Per radio, the sound card and keying fields changed: only those are written, over the radio as
   *  stored when Finish writes it. */
  radios: { id: number; changes: Partial<RadioKit> }[]
}

const sameRule = (x: RoutingRule, y: RoutingRule) =>
  x.radio === y.radio && (x.mode ?? null) === (y.mode ?? null) && (x.context ?? null) === (y.context ?? null) && sameList(x.bands, y.bands)

export function finishPlan(s: Settings, a: RadioSetupAnswers, order: readonly string[]): FinishPlan {
  const radios = s.radios ?? []
  const stored = s.routingRules ?? []
  const rules = compileRules(a, stored, order)
  const rulesChanged = rules.length !== stored.length || rules.some((r, i) => !sameRule(r, stored[i]))
  const plan: FinishPlan = {
    // A name left as it was is no rename, even one stored with spaces around it.
    renames: radios
      .filter((r) => (a.names[r.id] ?? r.name) !== r.name && (a.names[r.id] ?? r.name).trim() !== '' && (a.names[r.id] ?? r.name).trim() !== r.name)
      .map((r) => ({ id: r.id, name: a.names[r.id].trim() })),
    enabled: radios
      .filter((r) => (a.enabled[r.id] ?? true) !== (r.enabled !== false))
      .map((r) => ({ id: r.id, enabled: a.enabled[r.id] })),
    bands: radios.filter((r) => !sameBands(a.bands[r.id] ?? [], r.bands)).map((r) => ({ id: r.id, bands: a.bands[r.id] })),
    rules: rulesChanged ? rules : null,
    radios: radios
      .map((r) => {
        const now = a.kit[r.id]
        const was = kitOf(r)
        const changes: Partial<RadioKit> = {}
        if (now) for (const f of KIT_FIELDS) if (now[f] !== was[f]) changes[f] = now[f]
        return { id: r.id, changes }
      })
      .filter((x) => Object.keys(x.changes).length > 0),
  }
  if ((s.defaultRadio ?? null) !== a.defaultRadio) plan.defaultRadio = a.defaultRadio
  if ((s.radioSwitchPolicy ?? 'act') !== a.policy) plan.policy = a.policy
  return plan
}

/** Whether two reads of the settings agree on everything the setup's plan is a difference from:
 *  the radios (names, on/off, band lists), the radio in use, the rules, the default and the switch
 *  choice. Sound cards and keying are left out: Finish writes those field by field over what is
 *  stored then. */
export function sameSetup(a: Settings, b: Settings): boolean {
  const view = (s: Settings) =>
    JSON.stringify({
      radios: (s.radios ?? []).map((r) => [r.id, r.name, r.enabled !== false, r.bands]),
      active: s.activeRadio ?? null,
      rules: (s.routingRules ?? []).map((r) => [r.bands, r.mode ?? null, r.context ?? null, r.radio]),
      defaultRadio: s.defaultRadio ?? null,
      policy: s.radioSwitchPolicy ?? 'act',
    })
  return view(a) === view(b)
}

export function planIsEmpty(p: FinishPlan): boolean {
  return (
    !p.renames.length &&
    !p.enabled.length &&
    !p.bands.length &&
    !p.radios.length &&
    p.rules == null &&
    p.defaultRadio === undefined &&
    p.policy === undefined
  )
}
