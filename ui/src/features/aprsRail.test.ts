// APRS's body layout (layout L7): what the station rail's width divider writes, the range it moves
// through, and how a stored width reads back. The layout itself — the sheet clamping the token —
// is computed in aprsLayout.test.ts and measured in Chrome by the layout harness.
import { describe, it, expect } from 'vitest'
import {
  APRS_RAIL_MIN,
  APRS_RAIL_STOCK,
  aprsRailRange,
  aprsRailValue,
  parseAprsRail,
} from './aprsRail'

describe('the APRS rail width', () => {
  it('is written capped in the sheet: half the body, never less than the stock width', () => {
    expect(aprsRailValue(512)).toBe('min(512px, max(50%, 420px))')
    // Whole CSS px: a drag's fractional pointer never reaches the record.
    expect(aprsRailValue(300.6)).toBe('min(301px, max(50%, 420px))')
  })

  it('moves from its 260 px floor to half the body', () => {
    expect(APRS_RAIL_MIN).toBe(260)
    expect(aprsRailRange(1400)).toEqual({ min: 260, max: 700 })
  })

  it('on a body under twice the stock width still reaches the stock width, so where the rail stands is always in range', () => {
    // 700 px of body: half is 350, but the stock rail is 420 and renders there.
    expect(APRS_RAIL_STOCK).toBe(420)
    expect(aprsRailRange(700)).toEqual({ min: 260, max: 420 })
  })

  it('reads a stored width back, and anything that is not one as "never set"', () => {
    expect(parseAprsRail('512')).toBe(512)
    expect(parseAprsRail('333.5')).toBe(333.5)
    for (const raw of [null, '', 'abc', '0', '-40', 'NaN', 'Infinity']) expect(parseAprsRail(raw), String(raw)).toBeNull()
  })
})
