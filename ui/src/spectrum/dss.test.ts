import { describe, expect, it } from 'vitest'
import { DSS_ROWS, dssStrengths, median3 } from './dss'
import { SpectrumRing } from './ring'

const COLS = 8
// Levels a float32 store holds exactly, so the floor reads as exactly 0.
const RANGE = { floor: 0.25, ceil: 0.75 }
const FLOOR_ROW = new Array(64).fill(0.25)

function push(ring: SpectrumRing, bins: number[], loHz = 0, hiHz = 6400): void {
  ring.push({ seq: ring.serial, tMs: ring.serial, loHz, hiHz, bins, dbPerUnit: 120 }, RANGE)
}

function field(ring: SpectrumRing, offset = 0, view = { loHz: 0, hiHz: 6400 }) {
  const out = new Float32Array(DSS_ROWS * COLS)
  const present = new Uint8Array(DSS_ROWS)
  const rows = dssStrengths(ring, COLS, view, offset, 'peak', out, present)
  return { rows, at: (r: number) => Array.from(out.subarray(r * COLS, (r + 1) * COLS)), present }
}

describe('the 3-D stack', () => {
  it('takes the median of three', () => {
    expect(median3(1, 5, 3)).toBe(3)
    expect(median3(9, 0, 0)).toBe(0)
    expect(median3(0.4, 0.4, 0.9)).toBe(0.4)
  })

  it('anchors height on the floor the row was committed with: the floor is 0, the ceiling 1', () => {
    const ring = new SpectrumRing(16, 64)
    push(ring, FLOOR_ROW)
    push(ring, new Array(64).fill(0.75))
    const f = field(ring)
    expect(f.at(0)).toEqual(new Array(COLS).fill(1))
    expect(f.at(1)).toEqual(new Array(COLS).fill(0))
  })

  it('rejects a one-row broadband burst and keeps a carrier that persists', () => {
    const ring = new SpectrumRing(16, 64)
    const carrier = FLOOR_ROW.slice()
    carrier[20] = 0.75 // column 2 of 8 (bins 16..23)
    const burst = new Array(64).fill(0.75)
    for (const row of [carrier, carrier, burst, carrier, carrier]) push(ring, row)
    const f = field(ring)
    // The burst is age 2. Through the median it reads as the carrier rows around it.
    expect(f.at(2)).toEqual(f.at(0))
    expect(f.at(2)[2]).toBe(1)
    expect(f.at(2)[5]).toBe(0)
  })

  it('takes the median at one FREQUENCY across a retune, not at one column', () => {
    const ring = new SpectrumRing(16, 64)
    // A carrier at 2050 Hz while the span slides 800 Hz between rows: it moves one column each
    // row in column terms, and must stay put in frequency terms.
    for (const shift of [0, 800, 1600]) {
      const row = FLOOR_ROW.slice()
      row[Math.floor(((2050 - shift) / 6400) * 64)] = 0.75
      push(ring, row, shift, 6400 + shift)
    }
    const f = field(ring, 0, { loHz: 1600, hiHz: 8000 })
    expect(f.at(0)[0]).toBe(1)
    expect(f.at(0).slice(1)).toEqual(new Array(COLS - 1).fill(0))
  })

  it('shows at most DSS_ROWS rows, ending where the scrollback does', () => {
    const ring = new SpectrumRing(256, 64)
    for (let i = 0; i < 150; i++) push(ring, FLOOR_ROW)
    expect(field(ring).rows).toBe(DSS_ROWS)
    expect(field(ring, 100).rows).toBe(50)
    expect(field(ring, 150).rows).toBe(0)
  })
})
