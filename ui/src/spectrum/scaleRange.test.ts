import { describe, expect, it } from 'vitest'
import { applyGainZero, WF_ZERO_TRIM_DB } from '../waterfall'
import { axisOf, valueToDb, type ScaleAxis } from './scale'
import { AUTO_SPAN_DB, AUTO_UNDER_MEAN_DB, autoRange, displayRange } from './scaleRange'

const AUDIO = axisOf({ kind: 'dbfs', loDb: -120, hiDb: 0 }, 'audio')
const CIV = axisOf({ kind: 'relative' }, 'civ')

/** A CI-V sweep as the backend publishes it: byte / 160. */
const civRow = (bytes: number[]) => bytes.map((b) => b / 160)

describe("the automatic range, in each source's own dB", () => {
  it('puts the black point 5 dB under the mean and the top 55 dB over it on a CI-V scope (80 dB a unit)', () => {
    const row = civRow([20, 22, 18, 21, 19, 140, 20, 20])
    const meanDb = row.reduce((s, v) => s + valueToDb(CIV, v), 0) / row.length
    const r = autoRange(row, CIV.dbPerUnit)
    expect(valueToDb(CIV, r.floor)).toBeCloseTo(meanDb - AUTO_UNDER_MEAN_DB, 9)
    expect(valueToDb(CIV, r.ceil) - valueToDb(CIV, r.floor)).toBeCloseTo(AUTO_SPAN_DB, 9)
    // In value units that is 55/80 of the radio's display, not the 55/120 the audio axis would give.
    expect(r.ceil - r.floor).toBeCloseTo(55 / 80, 12)
  })

  it('skips what is not a value, and gives a row with none a range all the same', () => {
    expect(autoRange([0.5, Number.NaN, 0.5, Number.POSITIVE_INFINITY], 120)).toEqual(autoRange([0.5, 0.5], 120))
    const empty = autoRange([Number.NaN], 120)
    expect(empty.ceil - empty.floor).toBeCloseTo(55 / 120, 12)
    // A dB per unit that cannot be divided by is the audio axis's.
    expect(autoRange([0.5], 0)).toEqual(autoRange([0.5], 120))
  })
})

describe("G and Z: today's sliders, in the frame's own dB", () => {
  const cases: [string, ScaleAxis][] = [
    ['audio', AUDIO],
    ['civ', CIV],
  ]

  it('Z moves the black point the same dB on every scale, and G scales the window as it does today', () => {
    for (const [name, axis] of cases) {
      const auto = { floor: 0.3, ceil: 0.3 + 55 / axis.dbPerUnit }
      const db = (v: number) => valueToDb(axis, v)
      const same = displayRange(auto, 0, 0, axis.dbPerUnit)
      expect(same, name).toEqual(auto)
      for (const zero of [-1, -0.5, 0.5, 1]) {
        const r = displayRange(auto, 0, zero, axis.dbPerUnit)
        expect(db(r.floor) - db(auto.floor), `${name} Z ${zero}`).toBeCloseTo(zero * WF_ZERO_TRIM_DB, 9)
        expect(db(r.ceil) - db(r.floor), `${name} Z ${zero}`).toBeCloseTo(55, 9)
      }
      for (const [gain, factor] of [
        [1, 0.4],
        [0.5, 0.7],
        [-0.5, 1.5],
        [-1, 2],
      ]) {
        const r = displayRange(auto, gain, 0, axis.dbPerUnit)
        expect(db(r.ceil) - db(r.floor), `${name} G ${gain}`).toBeCloseTo(55 * factor, 9)
      }
    }
  })

  it("is exactly today's applyGainZero on the audio axis, so nothing drawn today moves", () => {
    for (const [g, z] of [
      [0, 0],
      [0.35, -0.6],
      [-1, 1],
    ]) {
      expect(displayRange({ floor: 0.2, ceil: 0.65 }, g, z, 120)).toEqual(applyGainZero(0.2, 0.65, g, z))
    }
  })
})
