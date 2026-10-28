// The radio setup's answers → what Finish writes. An untouched setup writes nothing; the shared-band
// cells become rules above the station's other rules, replacing only the rules they restate.
import { describe, it, expect } from 'vitest'
import { readFileSync, writeFileSync } from 'node:fs'
import { answersFrom, compileRules, finishPlan, planIsEmpty, routingDraft, ruleRadio, sharedBands } from './radioSetup'
import type { RadioProfile, RouteMode, RoutingRule, Settings } from '../types'

const BANDS = ['160m', '80m', '40m', '20m', '10m', '6m', '2m', '70cm']
const radio = (id: number, name: string, bands: string[], enabled = true) =>
  ({ id, name, bands, enabled }) as unknown as RadioProfile

function station(rules: RoutingRule[] = []): Settings {
  return {
    radios: [radio(0, 'FTdx10', []), radio(1, 'IC-9700', ['2m', '70cm']), radio(2, 'FT-991A', ['2m', '70cm'])],
    activeRadio: 0,
    routingRules: rules,
    defaultRadio: 0,
    radioSwitchPolicy: 'act',
  } as Settings
}

describe('the radio setup answers', () => {
  it('opens on what is stored, and an untouched setup writes nothing', () => {
    const fm: RoutingRule = { bands: ['2m'], mode: 'fm', context: null, radio: 2 }
    const sat: RoutingRule = { bands: [], mode: null, context: 'satellite', radio: 1 }
    const s = station([fm, sat])
    const a = answersFrom(s, BANDS)
    expect(sharedBands(a, BANDS)).toEqual(['2m', '70cm'])
    expect(a.cells['2m'].fm).toBe(2)
    expect(a.cells['2m'].digital).toBeNull()
    const plan = finishPlan(s, a, BANDS)
    expect(planIsEmpty(plan)).toBe(true)
    expect(compileRules(a, s.routingRules!, BANDS)).toEqual([fm, sat])
  })

  it('a name left as it was is no rename, even one stored with spaces around it', () => {
    const s = station()
    s.radios = s.radios!.map((r) => (r.id === 1 ? { ...r, name: ' IC-9700 ' } : r))
    expect(planIsEmpty(finishPlan(s, answersFrom(s, BANDS), BANDS))).toBe(true)
  })

  it('ticking a band off and on again is no change', () => {
    const s = station()
    const a = answersFrom(s, BANDS)
    a.bands[1] = ['70cm', '2m']
    expect(finishPlan(s, a, BANDS).bands).toEqual([])
  })

  it('changed cells become rules above the rest; every stored rule stays below, in order', () => {
    const fm: RoutingRule = { bands: ['2m'], mode: 'fm', context: null, radio: 2 }
    const allDigital: RoutingRule = { bands: [], mode: 'digital', context: null, radio: 1 }
    const sat: RoutingRule = { bands: [], mode: null, context: 'satellite', radio: 1 }
    const s = station([fm, allDigital, sat])
    const a = answersFrom(s, BANDS)
    // Prefilled from the first matching rule, as the engine reads them.
    expect(a.cells['70cm'].digital).toBe(1)
    a.cells['70cm'] = { ...a.cells['70cm'], digital: 2 }
    const rules = compileRules(a, s.routingRules!, BANDS)
    expect(rules).toEqual([{ bands: ['70cm'], mode: 'digital', context: null, radio: 2 }, fm, allDigital, sat])
    // The engine's reading of the compiled list gives back every cell.
    for (const band of ['2m', '70cm'])
      for (const mode of ['digital', 'fm', 'ssb', 'cw', 'rtty'] as const)
        expect(ruleRadio(rules, band, mode), `${band} ${mode}`).toBe(a.cells[band][mode] ?? ruleRadio(s.routingRules!, band, mode))
    expect(finishPlan(s, a, BANDS).rules).toEqual(rules)
    expect(routingDraft(a, s.routingRules!, BANDS).rules).toEqual(rules)
  })

  it('writes only what changed', () => {
    const s = station()
    const a = answersFrom(s, BANDS)
    a.names[1] = '  IC-9700 shack  '
    a.enabled[2] = false
    a.bands[0] = ['20m', '40m']
    a.defaultRadio = null
    a.policy = 'tell'
    const plan = finishPlan(s, a, BANDS)
    expect(plan).toEqual({
      renames: [{ id: 1, name: 'IC-9700 shack' }],
      enabled: [{ id: 2, enabled: false }],
      bands: [{ id: 0, bands: ['20m', '40m'] }],
      rules: null,
      defaultRadio: null,
      policy: 'tell',
      radios: [],
    })
  })

  it("a radio's sound cards and keying: only what changed, per radio", () => {
    const s = station()
    s.radios = s.radios!.map((r) => ({ ...r, audioIn: 'in-0', audioOut: 'out-0', pttMethod: 'cat' }))
    const a = answersFrom(s, BANDS)
    expect(planIsEmpty(finishPlan(s, a, BANDS))).toBe(true)
    a.kit[1] = { ...a.kit[1], audioOut: 'out-9' }
    a.kit[2] = { ...a.kit[2], pttMethod: 'cat' }
    const plan = finishPlan(s, a, BANDS)
    expect(plan.radios).toEqual([{ id: 1, changes: { audioOut: 'out-9' } }])
    expect(planIsEmpty(plan)).toBe(false)
  })

  // A rule aimed at a radio that is turned off: the engine passes over it to the next rule. The
  // setup opens on what the engine does, and changing ANOTHER cell must leave that next rule there.
  it('a radio turned off: its cell opens on the rule the engine uses, and changing another cell keeps every rule', () => {
    const toC: RoutingRule = { bands: ['2m'], mode: 'digital', context: null, radio: 2 }
    const toB: RoutingRule = { bands: ['2m'], mode: 'digital', context: null, radio: 1 }
    const s = {
      radios: [radio(0, 'IC-7300', ['2m']), radio(1, 'FT-991A', ['2m']), radio(2, 'IC-9700', ['2m'], false)],
      activeRadio: 0,
      routingRules: [toC, toB],
      defaultRadio: null,
      radioSwitchPolicy: 'act',
    } as unknown as Settings
    const a = answersFrom(s, BANDS)
    expect(sharedBands(a, BANDS)).toEqual(['2m'])
    expect(a.cells['2m'].digital).toBe(1)
    a.cells['2m'] = { ...a.cells['2m'], fm: 0 }
    const rules = compileRules(a, s.routingRules!, BANDS)
    expect(rules).toEqual([{ bands: ['2m'], mode: 'fm', context: null, radio: 0 }, toC, toB])
    expect(ruleRadio(rules, '2m', 'digital', (id) => id !== 2)).toBe(1)
  })
})

// ⭐ ANY ONE CHANGED CELL LEAVES EVERY OTHER BAND AND MODE ROUTED AS BEFORE — asked of the ENGINE.
// Every single change of every shared cell (each radio, and "No rule") on two stations, compiled
// here, is written to a fixture that the engine's own test reads (`radio_setup_tests.rs`,
// `one_changed_cell_moves_only_that_cell`), and it routes every band and mode before and after with
// `Settings::route_radio`. This test fails when the compiler and the fixture disagree; rewrite the
// fixture with RADIO_SETUP_FIXTURE=write.
describe('the single-change fixture the engine checks', () => {
  const FIXTURE = new URL('./__fixtures__/radio-setup-single-changes.json', import.meta.url)
  const MODES: RouteMode[] = ['digital', 'fm', 'ssb', 'cw', 'rtty']
  type Station = { name: string; radios: { id: number; bands: string[]; enabled: boolean }[]; activeRadio: number; defaultRadio: number | null; rules: RoutingRule[] }
  const STATIONS: Station[] = [
    {
      name: 'a radio turned off, with a fallback behind its rule',
      radios: [
        { id: 0, bands: ['2m'], enabled: true },
        { id: 1, bands: ['2m'], enabled: true },
        { id: 2, bands: ['2m'], enabled: false },
      ],
      activeRadio: 0,
      defaultRadio: null,
      rules: [
        { bands: ['2m'], mode: 'digital', context: null, radio: 2 },
        { bands: ['2m'], mode: 'digital', context: null, radio: 1 },
      ],
    },
    {
      name: 'every kind of rule',
      radios: [
        { id: 0, bands: [], enabled: true },
        { id: 1, bands: ['6m', '2m', '70cm'], enabled: true },
        { id: 2, bands: ['2m', '70cm'], enabled: true },
        { id: 3, bands: ['2m', '70cm'], enabled: false },
      ],
      activeRadio: 0,
      defaultRadio: 0,
      rules: [
        { bands: ['2m'], mode: 'fm', context: null, radio: 3 },
        { bands: ['2m', '70cm'], mode: 'fm', context: null, radio: 2 },
        { bands: ['70cm', '6m'], mode: 'digital', context: null, radio: 1 },
        { bands: ['2m'], mode: null, context: null, radio: 2 },
        { bands: [], mode: 'cw', context: null, radio: 1 },
        { bands: [], mode: null, context: 'satellite', radio: 1 },
      ],
    },
  ]
  const build = () =>
    STATIONS.map((st) => {
      const s = {
        radios: st.radios.map((r) => radio(r.id, `R${r.id}`, r.bands, r.enabled)),
        activeRadio: st.activeRadio,
        routingRules: st.rules,
        defaultRadio: st.defaultRadio,
        radioSwitchPolicy: 'act',
      } as unknown as Settings
      const open = answersFrom(s, BANDS)
      const cases: { band: string; mode: RouteMode; radio: number | null; rules: RoutingRule[] }[] = []
      for (const band of sharedBands(open, BANDS))
        for (const mode of MODES)
          for (const value of [null, ...st.radios.map((r) => r.id)]) {
            if (value === open.cells[band][mode]) continue
            const a = answersFrom(s, BANDS)
            a.cells[band] = { ...a.cells[band], [mode]: value }
            cases.push({ band, mode, radio: value, rules: compileRules(a, st.rules, BANDS) })
          }
      return { ...st, cases }
    })

  it('matches what Finish compiles for every single change', () => {
    const built = JSON.stringify(build(), null, 1) + '\n'
    if (process.env.RADIO_SETUP_FIXTURE === 'write') writeFileSync(FIXTURE, built)
    expect(readFileSync(FIXTURE, 'utf8')).toBe(built)
    // Every case is a change Finish writes.
    for (const st of build()) expect(st.cases.length).toBeGreaterThan(10)
  })
})

