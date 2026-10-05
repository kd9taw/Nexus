// Frequency numbers on the Phone waterfall (operator, 2026-08-22: "it's a bit difficult to see
// where a mouse click will take you"): where the ticks fall.
//
// What each tick READS is the axis model's (`axisToRf`, `spectrum/overlays.ts`), pinned by value on
// every axis kind through the real scope in `components/PhoneScope.scale.test.tsx` — the trap this
// file first pinned among them: a NATIVE RF panadapter is already absolute, and adding the dial there
// labels a 14 MHz scope at 28 MHz, a scale an operator would tune by and be wrong.
import { describe, it, expect } from 'vitest'
import { axisTicks } from './waterfall'

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
