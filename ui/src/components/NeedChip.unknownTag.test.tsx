// @vitest-environment jsdom
// A need tag the BROWSER has no visual for must not take the render down.
//
// Remote's station and browser are separately versioned: the browser bundle is whatever
// was last deployed, the station is whatever the operator installed. Need rows cross the
// wire and are cast straight through — `useNeedAlerts` does `rows as unknown as NeedAlert[]`
// with no validation of `tags` — so a station newer than the browser sends a tag the
// browser's NEED_CHIP has no key for. `NEED_CHIP[tag]` is then `undefined`, and every
// unguarded `.cls` / `.label` / `.short` throws a TypeError inside render, which in React
// unmounts the whole subtree.
//
// That is not hypothetical: staging (built at 23918d68) crashed with
//   "Cannot read properties of undefined (reading 'cls')" at BandStrip
// once the station started sending `NewPark`, the tag added in 24042f7f. Adding the missing
// key fixes that ONE tag; these tests pin the general rule, so the NEXT tag added to
// NeedTag cannot repeat it. An unknown tag must degrade to "no need colour", never crash.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { BandStrip } from './BandStrip'
import { BandMap } from './BandMap'
import { SpotLegend } from './SpotLegend'
import type { NeedTag, SpotRow } from '../types'

function spot(over: Partial<SpotRow> = {}): SpotRow {
  return {
    call: 'K1ABC',
    entity: 'United States',
    zone: 5,
    band: '20m',
    freqMhz: 14.1,
    mode: 'CW',
    spotter: 'W3LPL',
    corroborators: [],
    ageSecs: 10,
    comment: '',
    licensed: true,
    ...over,
  }
}

// A tag from a station build the browser predates. `NewPark` was exactly this on staging;
// the literal here stands for "any tag this bundle does not know", which is what the next
// one will be too.
const FUTURE_TAG = 'NewSomethingTheBrowserPredates' as NeedTag
const needByCall = new Map<string, NeedTag>([['K1ABC', FUTURE_TAG]])

afterEach(cleanup)

describe('an unrecognised need tag', () => {
  it('does not crash the band strip', () => {
    const { container } = render(
      <BandStrip
        band="20m"
        dialMhz={14.1}
        txAllowed
        spots={[spot()]}
        spotMode="CW"
        needByCall={needByCall}
        onWorkSpot={() => {}}
      />,
    )
    // The spot still renders — it just carries no need colour.
    expect(screen.getByText('K1ABC')).toBeTruthy()
    expect(container.querySelector('.bandstrip-tick')).toBeTruthy()
    expect(container.querySelector('.bandstrip-tick.is-need')).toBeNull()
  })

  it('does not crash the band map', () => {
    const { container } = render(
      <BandMap
        band="20m"
        dialMhz={14.1}
        txAllowed
        spots={[spot()]}
        spotMode="CW"
        needByCall={needByCall}
        onWorkSpot={() => {}}
      />,
    )
    expect(screen.getByText('K1ABC')).toBeTruthy()
    expect(container.querySelector('.need-chip')).toBeNull()
  })

  it('does not crash the spot legend', () => {
    // The legend renders a fixed list, but it reads the same record; pin it so a tag
    // reaching the legend order without a chip cannot take the cockpit down either.
    expect(() => render(<SpotLegend />)).not.toThrow()
  })

  it('keeps a known tag working', () => {
    const { container } = render(
      <BandStrip
        band="20m"
        dialMhz={14.1}
        txAllowed
        spots={[spot()]}
        spotMode="CW"
        needByCall={new Map<string, NeedTag>([['K1ABC', 'NewEntity']])}
        onWorkSpot={() => {}}
      />,
    )
    expect(container.querySelector('.bandstrip-tick.is-need.need-entity')).toBeTruthy()
  })
})
