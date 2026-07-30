// @vitest-environment jsdom
// A one-way beacon/bulletin must be VISIBLE (an audible beacon is genuine propagation
// evidence) but must never wear a need colour. The backend refuses to score it
// (propagation::needalert::rank); these pin the display half, including the case the
// backend cannot fix on its own — `needByCall` is keyed by CALLSIGN, so a call that is
// both a beacon site and a real station would otherwise leak its need colour onto the
// beacon row.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { BandMap } from './BandMap'
import { BandStrip } from './BandStrip'
import type { NeedTag, SpotRow } from '../types'

function spot(over: Partial<SpotRow> = {}): SpotRow {
  return {
    call: '4U1UN',
    entity: 'United Nations HQ',
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

// 4U1UN is spotted BOTH as the IBP beacon on 14.100 and as the real UN station on 14.025,
// and it is an all-time-new entity. Exactly the leak described above.
const needByCall = new Map<string, NeedTag>([['4U1UN', 'NewEntity']])

afterEach(cleanup)

describe('BandMap — beacon rows', () => {
  it('shows a beacon spot rather than hiding it', () => {
    render(
      <BandMap
        band="20m"
        dialMhz={14.1}
        txAllowed
        spots={[spot({ beacon: 'ncdxf' })]}
        spotMode="CW"
        onWorkSpot={() => {}}
      />,
    )
    expect(screen.getByText('4U1UN')).toBeTruthy()
  })

  it('badges it as a beacon and says it is not workable', () => {
    const { container } = render(
      <BandMap
        band="20m"
        dialMhz={14.1}
        txAllowed
        spots={[spot({ beacon: 'ncdxf' })]}
        spotMode="CW"
        onWorkSpot={() => {}}
      />,
    )
    expect(container.querySelector('.bandmap-spot .spot-type-badge.type-beacon')).toBeTruthy()
    const btn = container.querySelector('.bandmap-spot')
    expect(btn?.getAttribute('title')).toMatch(/not workable/i)
  })

  it('never paints a need colour on a beacon row, even when the CALL is an ATNO', () => {
    const { container } = render(
      <BandMap
        band="20m"
        dialMhz={14.1}
        txAllowed
        spots={[spot({ beacon: 'ncdxf' })]}
        spotMode="CW"
        needByCall={needByCall}
        onWorkSpot={() => {}}
      />,
    )
    expect(container.querySelector('.bandmap-spot.need-entity')).toBeNull()
    expect(container.querySelector('.bandmap-tick.need-entity')).toBeNull()
    // …and the tooltip must not advertise it as a new one either.
    expect(container.querySelector('.bandmap-spot')?.getAttribute('title')).not.toMatch(
      /new one/i,
    )
  })

  it('still colours the SAME call on its real frequency', () => {
    // The other half of the rule: suppressing the callsign would hide a genuine ATNO.
    const { container } = render(
      <BandMap
        band="20m"
        dialMhz={14.025}
        txAllowed
        spots={[spot({ freqMhz: 14.025 })]}
        spotMode="CW"
        needByCall={needByCall}
        onWorkSpot={() => {}}
      />,
    )
    expect(container.querySelector('.bandmap-spot.need-entity')).toBeTruthy()
  })

  it('W1AW bulletins badge as W1AW', () => {
    const { container } = render(
      <BandMap
        band="20m"
        dialMhz={14.0475}
        txAllowed
        spots={[spot({ call: 'W1AW', freqMhz: 14.0475, beacon: 'w1aw' })]}
        spotMode="CW"
        onWorkSpot={() => {}}
      />,
    )
    const badge = container.querySelector('.bandmap-spot .spot-type-badge.type-beacon')
    expect(badge?.textContent).toBe('W')
    expect(container.querySelector('.bandmap-spot')?.getAttribute('title')).toMatch(
      /W1AW bulletin/,
    )
  })
})

describe('BandStrip — beacon rows', () => {
  it('badges the beacon and drops the need colour', () => {
    const { container } = render(
      <BandStrip
        band="20m"
        dialMhz={14.1}
        txAllowed
        spots={[spot({ beacon: 'ncdxf' })]}
        spotMode="CW"
        needByCall={needByCall}
        onWorkSpot={() => {}}
      />,
    )
    expect(screen.getByText('4U1UN')).toBeTruthy()
    expect(container.querySelector('.bandstrip-spot .spot-type-badge.type-beacon')).toBeTruthy()
    expect(container.querySelector('.bandstrip-tick.need-entity')).toBeNull()
  })
})
