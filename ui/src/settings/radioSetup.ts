// The radio setup's answers, and what Finish writes from them. Pure data in, data out.
//
// The rule model is the one the engine already routes by (`Settings::route_radio`): each radio's band
// list (empty = every band), the ordered band+mode rules (first match wins) and the default radio. The
// setup WRITES those; it adds no model of its own. Its two screens map onto them:
//   - "Which radio for which band" is each radio's band list, exactly.
//   - "Where two radios share a band" is one cell per shared band and mode class, each naming a radio
//     or no rule. Finish turns the cells into rules (one per mode and radio, every band with that
//     answer in it), placed ABOVE the station's other rules so what the screen showed is what wins.
//     A rule the cells fully restate (plain mode rules naming only shared bands) is replaced by them;
//     every other rule (satellite rules, rules with no band list, rules naming another band) is kept,
//     in its order, below.
//   - "Audio and keying" is each radio's receive and transmit sound card and keying method: the same
//     per-radio fields Settings' Edit of that radio writes, through the same per-radio verb.
// An untouched setup writes nothing: every part of the plan below is the difference from what is
// stored, and the rules are rebuilt only when a cell was changed.
import type { RadioProfile, RadioSwitchPolicy, RouteMode, RoutingRule, Settings } from '../types'
import type { RoutingDraft } from '../api'
import { ROUTE_MODES } from './describeRouting'

/** A radio's sound cards and keying method, as the "Audio and keying" step edits them. */
export interface RadioKit {
  audioIn: string
  audioOut: string
  pttMethod: string
}
export const KIT_FIELDS: readonly (keyof RadioKit)[] = ['audioIn', 'audioOut', 'pttMethod']

export interface RadioSetupAnswers {
  /** Radio ids, in the station's order. */
  ids: number[]
  names: Record<number, string>
  enabled: Record<number, boolean>
  /** Each radio's band list; empty = every band (a catch-all). */
  bands: Record<number, string[]>
  /** For each band two radios list: per mode class, the radio, or null for no rule. */
  cells: Record<string, Partial<Record<RouteMode, number | null>>>
  /** Whether any cell was changed. Untouched, the stored rules are kept exactly. */
  cellsEdited: boolean
  defaultRadio: number | null
  policy: RadioSwitchPolicy
  /** Each radio's sound cards and keying method. */
  kit: Record<number, RadioKit>
}

const sameList = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((x, i) => x === b[i])
/** A band list is a set: ticking a band off and on again is no change. */
const sameBands = (a: readonly string[], b: readonly string[]) =>
  a.length === b.length && a.every((x) => b.some((y) => y.toLowerCase() === x.toLowerCase()))

/** The radio a stored rule list gives (band, mode): the first plain rule that matches, as the engine
 *  reads them. Null when no rule matches. */
export function ruleRadio(rules: readonly RoutingRule[], band: string, mode: RouteMode): number | null {
  const hit = rules.find(
    (r) =>
      r.context !== 'satellite' &&
      (r.bands.length === 0 || r.bands.some((b) => b.toLowerCase() === band.toLowerCase())) &&
      (r.mode == null || r.mode === mode),
  )
  return hit ? hit.radio : null
}

/** Bands at least two radios that are on list by name, in `order`. A catch-all radio (no list) is not
 *  counted: a radio that lists the band already outranks it there. */
export function sharedBands(a: RadioSetupAnswers, order: readonly string[]): string[] {
  return order.filter((band) => a.ids.filter((id) => a.enabled[id] && a.bands[id]?.includes(band)).length >= 2)
}

const kitOf = (r: RadioProfile): RadioKit => ({ audioIn: r.audioIn ?? '', audioOut: r.audioOut ?? '', pttMethod: r.pttMethod ?? '' })

/** The answers the stored settings already give: what the setup opens on. */
export function answersFrom(s: Settings, order: readonly string[]): RadioSetupAnswers {
  const radios: RadioProfile[] = s.radios ?? []
  const a: RadioSetupAnswers = {
    ids: radios.map((r) => r.id),
    names: Object.fromEntries(radios.map((r) => [r.id, r.name])),
    enabled: Object.fromEntries(radios.map((r) => [r.id, r.enabled !== false])),
    bands: Object.fromEntries(radios.map((r) => [r.id, [...r.bands]])),
    cells: {},
    cellsEdited: false,
    defaultRadio: s.defaultRadio ?? null,
    policy: s.radioSwitchPolicy ?? 'act',
    kit: Object.fromEntries(radios.map((r) => [r.id, kitOf(r)])),
  }
  return withCells(a, s.routingRules ?? [], order)
}

/** Fill a cell for every shared band and mode that has none yet, from the stored rules. Cells already
 *  answered keep their answer. */
export function withCells(a: RadioSetupAnswers, rules: readonly RoutingRule[], order: readonly string[]): RadioSetupAnswers {
  const cells = { ...a.cells }
  for (const band of sharedBands(a, order)) {
    const row = { ...(cells[band] ?? {}) }
    for (const [mode] of ROUTE_MODES) if (!(mode in row)) row[mode] = ruleRadio(rules, band, mode)
    cells[band] = row
  }
  return { ...a, cells }
}

/** The rules Finish writes, in order. Untouched cells: the stored rules, unchanged. */
export function compileRules(a: RadioSetupAnswers, stored: readonly RoutingRule[], order: readonly string[]): RoutingRule[] {
  if (!a.cellsEdited) return [...stored]
  const shared = sharedBands(a, order)
  const out: RoutingRule[] = []
  for (const [mode] of ROUTE_MODES) {
    const byRadio = new Map<number, string[]>()
    for (const band of shared) {
      const radio = a.cells[band]?.[mode]
      if (radio == null) continue
      byRadio.set(radio, [...(byRadio.get(radio) ?? []), band])
    }
    for (const radio of [...byRadio.keys()].sort((x, y) => x - y)) {
      out.push({ bands: byRadio.get(radio)!, mode, context: null, radio })
    }
  }
  const restated = (r: RoutingRule) =>
    r.context !== 'satellite' && r.bands.length > 0 && r.bands.every((b) => shared.includes(b))
  return [...out, ...stored.filter((r) => !restated(r))]
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
    renames: radios
      .filter((r) => (a.names[r.id] ?? r.name).trim() !== '' && (a.names[r.id] ?? r.name).trim() !== r.name)
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
