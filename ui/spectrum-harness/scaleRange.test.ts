// The automatic range (ui/src/spectrum/scaleRange.ts) measured on the harness's own fixtures: the same
// rows the pixel probes draw. Arithmetic only, so it runs with the unit suite, not in the browser.
import { describe, expect, it } from 'vitest'
import { INDEXED_SETS, RENDERER_SETS, type IndexedSet } from './frames'
import { lutIndex } from '../src/spectrum/aggregate'
import { LogRecursiveAverager } from '../src/spectrum/scaleAverage'
import { AUTO_SPAN_DB, AUTO_UNDER_MEAN_DB, autoRange } from '../src/spectrum/scaleRange'
import { agcRange, parkFloor, SCOPE_WINDOW_DB, WF_FLOOR_PCT, dbToSpan } from '../src/waterfall'

const toDb = (v: number) => (v - 1) * 120
const median = (row: number[]) => [...row].sort((a, b) => a - b)[Math.floor(row.length / 2)]
const SETS: IndexedSet[] = [
  INDEXED_SETS.carrier,
  INDEXED_SETS['two-tone'],
  INDEXED_SETS['noise-step'],
  INDEXED_SETS['ft8-slot'],
  RENDERER_SETS.wide,
]

describe('auto range on the fixtures', () => {
  it("is deskHPSDR's rule on every fixture: the black point 5 dB under the mean of the row's dB, the top 55 dB over it", () => {
    for (const set of SETS) {
      for (const i of [0, Math.floor(set.count / 2), set.count - 1]) {
        const row = set.frame(i).row
        const meanDb = row.reduce((s, v) => s + toDb(v), 0) / row.length
        const r = autoRange(row, 120)
        expect(toDb(r.floor), `${set.id} row ${i}`).toBeCloseTo(meanDb - AUTO_UNDER_MEAN_DB, 9)
        expect(toDb(r.ceil) - toDb(r.floor), `${set.id} row ${i}`).toBeCloseTo(AUTO_SPAN_DB, 9)
      }
    }
  })

  it("measures only the bins it is given: a host's view", () => {
    // The carrier fixture's 1000 Hz tone, outside a 2000–4000 Hz view, must not move that view's range.
    const row = INDEXED_SETS.carrier.frame(0).row
    const whole = autoRange(row, 120)
    const upper = autoRange(row, 120, 256, 512)
    const upperMeanDb = row.slice(256).reduce((s, v) => s + toDb(v), 0) / 256
    expect(toDb(upper.floor)).toBeCloseTo(upperMeanDb - 5, 9)
    expect(toDb(whole.floor)).toBeGreaterThan(toDb(upper.floor))
  })

  it('draws the carriers where the rig scope draws them today, and lifts its noise about 9% off the floor', () => {
    // carrier: one at −45 dBFS on a −95 dBFS floor. two-tone: two at −50. Measured (palette index of the
    // tone's peak bin / of the row's noise median): carrier auto 246 / 22, today 247 / 0; two-tone auto
    // 229 / 20, today 230 / 0.
    for (const [id, bin] of [
      ['carrier', Math.round((1000 / 4000) * 512 - 0.5)],
      ['two-tone', Math.round((700 / 4000) * 512 - 0.5)],
    ] as const) {
      const row = INDEXED_SETS[id].frame(40).row
      const auto = autoRange(row, 120)
      // Today's rig scope: the median, then a fixed SCOPE_WINDOW_DB over it.
      const floor = agcRange(row, WF_FLOOR_PCT).floor
      const scope = { floor, ceil: floor + dbToSpan(SCOPE_WINDOW_DB) }
      const peak = Math.max(...row.slice(bin - 2, bin + 3))
      const at = (v: number, r: { floor: number; ceil: number }) => lutIndex(v, r.floor, r.ceil)
      expect(Math.abs(at(peak, auto) - at(peak, scope)), `${id}: the tone`).toBeLessThanOrEqual(2)
      expect(at(median(row), scope), `${id}: the noise, today`).toBe(0)
      expect(at(median(row), auto), `${id}: the noise, auto`).toBeGreaterThanOrEqual(15)
      expect(at(median(row), auto), `${id}: the noise, auto`).toBeLessThanOrEqual(30)
    }
  })

  it('follows a noise floor that steps 15 dB within the row, and with 250 ms of averaging within τ', () => {
    const set = INDEXED_SETS['noise-step']
    const floorDb = (i: number) => toDb(autoRange(set.frame(i).row, 120).floor)
    expect(floorDb(60) - floorDb(59)).toBeGreaterThan(14)
    expect(floorDb(60) - floorDb(59)).toBeLessThan(16)
    // The rig scope polls a row every 50 ms; through the averager the range takes the step with τ.
    const avg = new LogRecursiveAverager(250)
    const averaged: number[] = []
    for (let i = 0; i < 120; i++) averaged.push(toDb(autoRange(avg.push({ seq: i, tMs: 0, loHz: 0, hiHz: 4000, bins: set.frame(i).row, dbPerUnit: 120 }, i * 50).bins, 120).floor))
    const before = averaged[59]
    const after = averaged[119]
    // One row after the step: 1 − e^(−50/250) of it. One τ (five rows) after: 1 − 1/e.
    expect((averaged[60] - before) / (after - before)).toBeCloseTo(1 - Math.exp(-50 / 250), 1)
    expect((averaged[64] - before) / (after - before)).toBeCloseTo(1 - Math.exp(-1), 1)
  })

  it("lights the FT waterfall's noise field that its parked floor keeps black (the case against using it there)", () => {
    // The quiet half-second before the FT8 slot: noise only. The FT waterfall's black point today is the
    // median plus WF_PARK_DB (`parkFloor`); the auto range's is the mean minus 5 dB.
    let parkedBlack = 0
    let autoBlack = 0
    let n = 0
    for (let i = 0; i < 4; i++) {
      const row = INDEXED_SETS['ft8-slot'].frame(i).row
      const m = median(row)
      const parked = parkFloor(m, agcRange(row).ceil)
      const auto = autoRange(row, 120)
      for (const v of row) {
        if (lutIndex(v, parked.floor, parked.ceil) === 0) parkedBlack++
        if (lutIndex(v, auto.floor, auto.ceil) === 0) autoBlack++
        n++
      }
    }
    // Measured: 78% black under the parked floor, 2.4% under the auto range.
    expect(parkedBlack / n).toBeGreaterThan(0.7)
    expect(autoBlack / n).toBeLessThan(0.1)
  })
})
