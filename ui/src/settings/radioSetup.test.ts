// The radio setup's answers → what Finish writes. An untouched setup writes nothing; the shared-band
// cells become rules above the station's other rules, replacing only the rules they restate.
import { describe, it, expect } from 'vitest'
import { answersFrom, compileRules, finishPlan, planIsEmpty, routingDraft, ruleRadio, sharedBands } from './radioSetup'
import type { RadioProfile, RoutingRule, Settings } from '../types'

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

  it('ticking a band off and on again is no change', () => {
    const s = station()
    const a = answersFrom(s, BANDS)
    a.bands[1] = ['70cm', '2m']
    expect(finishPlan(s, a, BANDS).bands).toEqual([])
  })

  it('edited cells become rules above the rest; restated rules go, others stay in order', () => {
    const fm: RoutingRule = { bands: ['2m'], mode: 'fm', context: null, radio: 2 }
    const allDigital: RoutingRule = { bands: [], mode: 'digital', context: null, radio: 1 }
    const sat: RoutingRule = { bands: [], mode: null, context: 'satellite', radio: 1 }
    const s = station([fm, allDigital, sat])
    const a = answersFrom(s, BANDS)
    // Prefilled from the first matching rule, as the engine reads them.
    expect(a.cells['70cm'].digital).toBe(1)
    a.cells['70cm'] = { ...a.cells['70cm'], digital: 2 }
    a.cellsEdited = true
    const rules = compileRules(a, s.routingRules!, BANDS)
    expect(rules).toEqual([
      { bands: ['2m'], mode: 'digital', context: null, radio: 1 },
      { bands: ['70cm'], mode: 'digital', context: null, radio: 2 },
      { bands: ['2m'], mode: 'fm', context: null, radio: 2 },
      allDigital,
      sat,
    ])
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
    })
  })
})
