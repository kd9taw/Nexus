import { describe, it, expect } from 'vitest'
import { SweepRate, SLOW_SWEEPS_PER_SEC, scopeIsSlow } from './sweepRate'

/** Feed `rate` sweeps a second for `seconds`, starting at `t0`; returns the last time fed. */
function feed(r: SweepRate, rate: number, seconds: number, t0 = 0): number {
  const step = 1000 / rate
  let t = t0
  for (let i = 0; i <= rate * seconds; i++) {
    t = t0 + i * step
    r.sweep(t)
  }
  return t
}

describe('the scope sweep rate', () => {
  it('reads 3 sweeps a second as slow and 10 as not', () => {
    const slow = new SweepRate()
    const end = feed(slow, 3, 6)
    const r = slow.perSecond(end)
    expect(r).not.toBeNull()
    expect(r!).toBeCloseTo(3, 1)
    expect(r! < SLOW_SWEEPS_PER_SEC).toBe(true)

    const fast = new SweepRate()
    const end2 = feed(fast, 10, 6)
    expect(fast.perSecond(end2)! >= SLOW_SWEEPS_PER_SEC).toBe(true)
  })

  it('says nothing until most of a window has been seen', () => {
    const r = new SweepRate()
    const end = feed(r, 3, 2)
    expect(r.perSecond(end)).toBeNull()
  })

  it('says nothing once the sweeps stop, rather than keep the last rate', () => {
    const r = new SweepRate()
    const end = feed(r, 3, 6)
    expect(r.perSecond(end)).not.toBeNull()
    expect(r.perSecond(end + 6000)).toBeNull()
  })

  it('follows a radio switched to a faster speed within one window', () => {
    const r = new SweepRate()
    const end = feed(r, 3, 6)
    const end2 = feed(r, 12, 6, end + 1)
    expect(r.perSecond(end2)! >= SLOW_SWEEPS_PER_SEC).toBe(true)
  })
})

describe('the one rule for the slow-scope hint', () => {
  it("the station's report decides when it is there: SPEED where readable, else its rate", () => {
    expect(scopeIsSlow({ sweepsPerSec: 9, dropped: 0, speed: 'slow' }, 'civ', null)).toBe(true)
    // A readable FAST is never slow, whatever this scope measured.
    expect(scopeIsSlow({ sweepsPerSec: 2, dropped: 4, speed: 'fast' }, 'civ', 1)).toBe(false)
    expect(scopeIsSlow({ sweepsPerSec: 2, dropped: 0, speed: null }, 'civ', 12)).toBe(true)
    expect(scopeIsSlow({ sweepsPerSec: 10, dropped: 0, speed: null }, 'civ', 1)).toBe(false)
    // No rig scope streaming: no hint.
    expect(scopeIsSlow(null, 'civ', 1)).toBe(false)
  })
  it('falls back to the measured rate, for an Icom only, while the station does not report one', () => {
    expect(scopeIsSlow(undefined, 'civ', 3)).toBe(true)
    expect(scopeIsSlow(undefined, 'civ', 9)).toBe(false)
    expect(scopeIsSlow(undefined, 'civ', null)).toBe(false)
    expect(scopeIsSlow(undefined, 'flex', 1)).toBe(false)
  })
})
