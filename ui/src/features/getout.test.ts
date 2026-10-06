import { describe, it, expect } from 'vitest'
import { octantCoverage, getoutSummary, OCTANTS } from './getout'
import type { HeardMe } from '../types'

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
