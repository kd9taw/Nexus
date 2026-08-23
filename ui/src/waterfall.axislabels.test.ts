// Frequency numbers on the Phone waterfall (operator, 2026-08-22: "it's a bit difficult to see
// where a mouse click will take you").
//
// The trap this pins: the scope axis means TWO DIFFERENT THINGS depending on the feed. On the
// carrier-centred AUDIO axis, scopeView returns RF OFFSETS from the dial — that is what puts the
// dial at the 1/4 mark on USB and the 3/4 mark on LSB. On a NATIVE RF panadapter it already
// returns absolute RF. Adding the dial in the second case labels a 14 MHz scope at 28 MHz, which
// is a scale an operator would tune by and be wrong.
import { describe, it, expect } from 'vitest'
import { axisAbsoluteHz, axisTicks } from './waterfall'

const DIAL = 14_200_000

describe('axisAbsoluteHz — what the operator is pointing at', () => {
  it('adds the dial on an AUDIO axis, where the axis is an offset', () => {
    expect(axisAbsoluteHz(0, 'audio', DIAL)).toBe(DIAL)
    expect(axisAbsoluteHz(1500, 'audio', DIAL)).toBe(14_201_500)
    // The guard band on the empty side is a NEGATIVE offset, and must read below the dial.
    expect(axisAbsoluteHz(-800, 'audio', DIAL)).toBe(14_199_200)
  })

  it('does NOT add the dial on a native RF panadapter, which is already absolute', () => {
    // The control that matters: getting this backwards is a scale 14 MHz out.
    expect(axisAbsoluteHz(14_201_500, 'civ', null)).toBe(14_201_500)
    expect(axisAbsoluteHz(14_201_500, 'flex', DIAL)).toBe(14_201_500)
    // NB: 'yaesu' is not an RF source on main — that is PR #147. When it lands, it must be
    // added to isRfScopeSource or this axis will label an FT-710 scope at twice its frequency.
  })

  it('says nothing rather than guessing when the dial is unknown', () => {
    // A wrong number on a scale someone tunes by is worse than a blank one.
    expect(axisAbsoluteHz(1500, 'audio', null)).toBeNull()
    expect(axisAbsoluteHz(1500, 'audio', 0)).toBeNull()
    expect(axisAbsoluteHz(1500, 'audio', Number.NaN)).toBeNull()
  })
})

describe('axisTicks — numbers an operator recognises', () => {
  it('lands on round steps, not on even divisions of the span', () => {
    // A 3 kHz SSB window: 500 Hz steps starting at a multiple of 500.
    // Span 3200 Hz with a 6-tick budget picks the 1000 Hz step (500 would give 6.4 ticks).
    const t = axisTicks(-800, 2400)
    expect(t.every((v) => v % 1000 === 0)).toBe(true)
    expect(t[0]).toBe(0)
    expect(Object.is(t[0], -0)).toBe(false) // "-0" on the dial's own tick would be absurd
  })

  it('thins out rather than crowding a narrow scope', () => {
    const wide = axisTicks(0, 200_000)
    const narrow = axisTicks(0, 2000)
    expect(wide.length).toBeLessThanOrEqual(6)
    expect(narrow.length).toBeLessThanOrEqual(6)
    expect(narrow.length).toBeGreaterThan(0)
  })

  it('returns nothing for a degenerate span instead of looping', () => {
    // The control: a zero or inverted span must not produce an infinite tick loop.
    expect(axisTicks(1000, 1000)).toEqual([])
    expect(axisTicks(2000, 1000)).toEqual([])
    expect(axisTicks(0, Number.NaN)).toEqual([])
  })
})
