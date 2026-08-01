import { describe, it, expect } from 'vitest'

import { satElementsLane } from './satLane'
import type { TleStatus } from '../api'

const NOW = 1_900_000_000

/** A healthy mirror-fed status; tests override the fields under trial. */
function status(over: Partial<TleStatus>): TleStatus {
  return {
    count: 93,
    usableCount: 93,
    fetchedAt: NOW - 3600,
    source: 'mirror',
    importedCount: 0,
    elementAgeDays: 0.4,
    blockedUntil: 0,
    ...over,
  }
}

describe('satElementsLane', () => {
  it('is quiet when elements are current', () => {
    expect(satElementsLane(status({}), NOW)).toBeNull()
  })

  it('warns on the Celestrak hard stop while elements can still suffer for it', () => {
    const s = status({ blockedUntil: NOW + 3600, elementAgeDays: 16 })
    expect(satElementsLane(s, NOW)?.message).toBe('Sat: Celestrak blocked')
  })

  // THE REVIEW CATCH: block still standing backend-side (correct etiquette),
  // but the mirror has since landed a fresh usable set — a warning claiming
  // "elements may age until it lands" over 0.4 d elements is a lie. The lane
  // goes quiet; the backend keeps the block itself.
  it('goes quiet once the mirror recovers, even mid-block', () => {
    expect(satElementsLane(status({ blockedUntil: NOW + 3600 }), NOW)).toBeNull()
  })

  it('an expired block is no block', () => {
    const s = status({ blockedUntil: NOW - 60, elementAgeDays: 16 })
    expect(satElementsLane(s, NOW)?.message).toBe('Sat: elements 16 d old')
  })

  it('an all-rotten cache warns as unusable', () => {
    const s = status({ usableCount: 0, elementAgeDays: null })
    expect(satElementsLane(s, NOW)?.message).toBe('Sat: elements unusable')
  })

  it('blocked + all-rotten shows the block (the actionable root cause)', () => {
    const s = status({ blockedUntil: NOW + 3600, usableCount: 0, elementAgeDays: null })
    expect(satElementsLane(s, NOW)?.message).toBe('Sat: Celestrak blocked')
  })

  it('past 14 d warns with the rounded age', () => {
    expect(satElementsLane(status({ elementAgeDays: 16.4 }), NOW)?.message).toBe(
      'Sat: elements 16 d old',
    )
  })

  it('13 d is not stale — one threshold, no third number', () => {
    expect(satElementsLane(status({ elementAgeDays: 13 }), NOW)).toBeNull()
  })

  // THE PRE-LAUNCH REALITY: the TLE mirror 404s until the next site release
  // ships it. A dead mirror with CURRENT elements is nothing the operator
  // can or should act on — the lane stays quiet (the Settings ▸ Orbital
  // elements line carries the friendly explanation); nagging here would
  // teach operators to ignore the chip before the mirror even goes live.
  it('a dead mirror (404, pre-launch) with current elements does not nag', () => {
    const s = status({
      lastError: 'TLE mirror fetch failed: HTTP 404',
      lastErrorKind: 'mirrorUnreachable',
    })
    expect(satElementsLane(s, NOW)).toBeNull()
  })

  it('never had elements: nothing to warn about (setup, not degradation)', () => {
    const s = status({ count: 0, usableCount: 0, elementAgeDays: null, source: 'none', fetchedAt: 0 })
    expect(satElementsLane(s, NOW)).toBeNull()
  })
})
