// The own-grid views' column split (layout L7): what a stored share reads back as and what the
// tracks are painted with. The divider's keys and drag are PaneSeam's (PaneSeam.test.tsx); the
// views' wiring is in SatellitesView.columns.test.tsx and AwardsView.columns.test.tsx, and the
// templates in view-grids.test.ts.
import { describe, it, expect } from 'vitest'
import { MIN_SHARE, seamShares } from './panelState'
import { columnShareStyle, parseColumnShare } from './viewColumns'

describe('a view’s stored column share', () => {
  it('reads back the share a divider commits', () => {
    const [a] = seamShares(0.6)
    expect(parseColumnShare(String(a))).toBe(a)
  })

  it('is clamped into the range every split divider keeps, so no column can be collapsed by hand', () => {
    expect(parseColumnShare('0')).toBe(MIN_SHARE)
    expect(parseColumnShare('1.99')).toBe(2 - MIN_SHARE)
    expect(parseColumnShare('-3')).toBe(MIN_SHARE)
  })

  it('reads anything that is not a share as "never set"', () => {
    for (const raw of [null, '', '  ', 'wide', 'NaN', 'Infinity']) expect(parseColumnShare(raw), String(raw)).toBeNull()
  })

  it('paints the pair as two fr tokens summing to 2 — or nothing, which is the sheet’s stock split', () => {
    expect(columnShareStyle(1.2, ['--x-a', '--x-b'])).toEqual({ '--x-a': '1.2fr', '--x-b': '0.8fr' })
    expect(columnShareStyle(null, ['--x-a', '--x-b'])).toBeUndefined()
  })
})
