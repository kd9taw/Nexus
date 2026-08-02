import { describe, it, expect } from 'vitest'

import { satElementsLane } from './satLane'
import type { TleStatus } from '../api'

const NOW = 1_900_000_000

/** A healthy mirror-fed status; tests override the fields under trial. */
function status(over: Partial<TleStatus>): TleStatus {
  return {
    count: 93,
    usableCount: 93,
    agingCount: 0,
    heldBackCount: 0,
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

  // THE FIELD REPORT (2026-08-01): birds sitting out past the 30 d ceiling
  // are a normal, permanent feature of the catalog (AO-7 and the SatNOGS
  // slow-cadence tail). They must not put a warning chip on every screen —
  // the age the lane judges is the MEDIAN of the usable sets, and the
  // held-back birds are their own count, spoken where there is room for it.
  it('a current catalog with birds sitting out past 30 d is quiet', () => {
    const s = status({ count: 367, usableCount: 337, heldBackCount: 30, elementAgeDays: 0.2 })
    expect(satElementsLane(s, NOW)).toBeNull()
  })

  // THE RESIDUAL FALSE CALM a median alone still allows: 51 birds fetched
  // this morning, 49 sitting at 29 d and 40 past the ceiling. The median is
  // 0.2 d and TRUE — every age test on this lane passes — while two thirds of
  // the operator's catalog drifts. The band counters are what the lane reads
  // to catch it; the age alone cannot.
  it('most of the catalog past the 14 d line warns, even on a calm median', () => {
    const s = status({
      count: 140,
      usableCount: 100,
      agingCount: 49,
      heldBackCount: 40,
      elementAgeDays: 0.2,
    })
    const lane = satElementsLane(s, NOW)
    expect(lane?.tier).toBe('warning')
    expect(lane?.message).toBe('Sat: 89 of 140 past 14 d')
    expect(lane?.detail).toContain('89 of 140')
  })

  // …and the same lane must NOT nag. This chip is app-wide furniture: the
  // shipped catalog permanently carries a slow-cadence tail (AO-7 and the
  // SatNOGS birds re-observed every few weeks), and a minority past the line
  // is what a healthy catalog looks like — not a warning.
  it('a large minority past the line is still quiet — the chip is not a nag', () => {
    const s = status({
      count: 367,
      usableCount: 337,
      agingCount: 60,
      heldBackCount: 30,
      elementAgeDays: 0.2,
    })
    expect(satElementsLane(s, NOW)).toBeNull()
  })

  it('never had elements: nothing to warn about (setup, not degradation)', () => {
    const s = status({ count: 0, usableCount: 0, elementAgeDays: null, source: 'none', fetchedAt: 0 })
    expect(satElementsLane(s, NOW)).toBeNull()
  })
})
