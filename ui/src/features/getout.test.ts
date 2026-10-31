import { describe, it, expect } from 'vitest'
import { octantCoverage, getoutSummary, OCTANTS, heardMeSince, fmtSinceUtc } from './getout'
import type { HeardMe, MapSpot, PropagationSnapshot } from '../types'

function heard(octant: string, km: number): HeardMe {
  return { call: 'X', grid: null, band: '20m', snr: null, bearingDeg: 0, km, octant, ageSecs: 0 }
}

describe('octantCoverage', () => {
  it('returns all 8 octants, aggregating count + maxKm', () => {
    const cov = octantCoverage([heard('NE', 1000), heard('NE', 6500), heard('S', 2000)])
    expect(cov).toHaveLength(OCTANTS.length)
    const ne = cov.find((c) => c.octant === 'NE')!
    expect(ne.count).toBe(2)
    expect(ne.maxKm).toBe(6500)
    const w = cov.find((c) => c.octant === 'W')!
    expect(w.count).toBe(0) // a gap — not getting out that way
  })
})

describe('getoutSummary', () => {
  it('names the strongest direction and the dead ones when lopsided', () => {
    const s = getoutSummary([heard('NE', 6500), heard('E', 3000)], 'metric')
    expect(s).toContain('strongest toward NE')
    expect(s).toContain('(~6500 km)')
    expect(s).toContain('little/nothing to the')
  })
  it('states the strongest direction’s reach in the units the operator chose', () => {
    // 6500 km is 4039 mi. The sentence used to print kilometres whatever the setting said.
    const reports = [heard('NE', 6500), heard('E', 3000)]
    expect(getoutSummary(reports, 'imperial')).toContain('strongest toward NE (~4039 mi)')
    expect(getoutSummary(reports, 'metric')).toContain('strongest toward NE (~6500 km)')
  })
  it('omits the dead-direction clause when coverage is all around', () => {
    const s = getoutSummary(OCTANTS.map((o) => heard(o, 3000)), 'metric')
    expect(s).toContain('strongest toward')
    expect(s).not.toContain('little/nothing')
  })
  it('is empty with no reports', () => {
    expect(getoutSummary([], 'imperial')).toBe('')
  })
})

describe('heardMeSince (the map follows the Getting Out box\'s Start fresh time)', () => {
  const AS_OF = 1_700_000_000
  const spot = (call: string, ageSecs: number, heardMe: boolean): MapSpot => ({
    call, lat: 48, lon: 11, band: '20m', heardMe, ageSecs, time: AS_OF - ageSecs, approx: false,
  })
  const prop = (spots: MapSpot[]) => ({ asOf: AS_OF, spots }) as unknown as PropagationSnapshot

  // The station serves a cached snapshot for minutes (and the last good one, for as long as the
  // refetch fails) without moving its `asOf`, while it rebuilds the spots and their ages on every
  // poll. The filter must go by each report's own time, as the box does, or the dots and the box
  // disagree.
  const T0 = AS_OF
  const PRESS = T0 + 200
  const at = (call: string, time: number, heardMe: boolean, now: number): MapSpot => ({
    call, lat: 48, lon: 11, band: '20m', heardMe, ageSecs: now - time, time, approx: false,
  })
  it('keeps a receiver heard after the press while the snapshot is a cached one', () => {
    // Fetched at t0, pressed at t0+200, DL1AA heard at t0+250, polled at t0+280: a cache hit.
    const now = T0 + 280
    const cached = {
      source: 'live',
      asOf: T0,
      spots: [at('DL1AA', T0 + 250, true, now), at('VK3AA', T0 + 100, true, now)],
    } as unknown as PropagationSnapshot
    expect(heardMeSince(cached, PRESS)!.spots!.map((s) => s.call)).toEqual(['DL1AA'])
  })
  it('keeps it while the refetch fails and the last good snapshot is served', () => {
    const now = T0 + 280
    const stale = {
      source: 'cached',
      asOf: T0 - 3600,
      spots: [at('DL1AA', T0 + 250, true, now), at('VK3AA', T0 + 100, true, now), at('JA1AA', T0 - 60, false, now)],
    } as unknown as PropagationSnapshot
    expect(heardMeSince(stale, PRESS)!.spots!.map((s) => s.call)).toEqual(['DL1AA', 'JA1AA'])
  })

  it('leaves out "hearing you" spots reported before the start, and keeps everything else', () => {
    const p = prop([spot('VK3AA', 600, true), spot('DL1AA', 100, true), spot('JA1AA', 600, false)])
    const out = heardMeSince(p, AS_OF - 300)!
    expect(out.spots!.map((s) => s.call)).toEqual(['DL1AA', 'JA1AA'])
    expect(out.asOf).toBe(AS_OF)
  })
  it('counts a report AT the start time', () => {
    expect(heardMeSince(prop([spot('W1AW', 300, true)]), AS_OF - 300)!.spots).toHaveLength(1)
  })
  it('is the snapshot itself with no start time (Show all) or nothing to leave out', () => {
    const p = prop([spot('VK3AA', 600, true)])
    expect(heardMeSince(p, null)).toBe(p)
    expect(heardMeSince(p, AS_OF - 900)).toBe(p)
    expect(heardMeSince(null, AS_OF)).toBeNull()
  })
})

describe('fmtSinceUtc', () => {
  // To the second: a report at 14:05:10 is left out after a 14:05:30 press, and "since 14:05" would
  // say it counts.
  it('shows the start as the station clock does, to the second: HH:MM:SS UTC', () => {
    expect(fmtSinceUtc(Date.UTC(2026, 9, 30, 14, 5, 59) / 1000)).toBe('14:05:59 UTC')
    expect(fmtSinceUtc(Date.UTC(2026, 9, 30, 0, 0) / 1000)).toBe('00:00:00 UTC')
  })
})
