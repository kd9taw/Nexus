// @vitest-environment jsdom
//
// The "now" line above the modelled outlook: the band outlook pane (no station selected) and the
// map's band-condition strip print what a flare, a proton event or a NOAA watch is doing now, with
// its source and time, from the shack's `spaceNow` — and nothing at all when nothing is happening.
import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import type { BandReport, PathPrediction, PropagationSnapshot, SpaceNow } from '../../types'
import { SpaceNowLine } from './SpaceNowLine'
import { MapInsightRail } from './MapInsightRail'
import { PANES } from '../connect/panes'
import type { PaneContext } from '../connect/paneContext'

const NOW = Math.floor(Date.now() / 1000)

const FLARE: SpaceNow = {
  flare: {
    r: 3,
    xrayLong: 1e-4,
    xrayAt: NOW - 60,
    haf: 'noaa',
    drapValidAt: NOW - 45,
    hafSubMhz: 30,
    out: '60m',
    weak: '10m',
    recovery: { at: NOW + 40 * 60, source: 'noaa' },
  },
  protons: { s: 2, j10: 150, at: NOW - 300 },
}

function outlook(spaceNow?: SpaceNow): PathPrediction {
  return { engine: 'p533', bands: [], mufNow: 21, mufHourly: [], ...(spaceNow ? { spaceNow } : {}) }
}

const band = (b: string): BandReport =>
  ({ band: b, score: 0.6, tier: 'Moderate', reason: 'modelled', spots: 0 }) as unknown as BandReport

function snap(): PropagationSnapshot {
  return { advisory: { bands: [band('20m'), band('40m')] } } as unknown as PropagationSnapshot
}

describe('the now line', () => {
  it('says each thing happening on its own line, as a status, with its source and time', () => {
    const { getByRole } = render(<SpaceNowLine now={FLARE} nowSec={NOW} />)
    const s = getByRole('status')
    const items = [...s.querySelectorAll('li')].map((l) => l.textContent)
    expect(items).toHaveLength(2)
    expect(items[0]).toContain('60m and below out on the dayside')
    expect(items[0]).toContain('NOAA D-RAP')
    expect(items[1]).toContain('S2 proton event')
  })

  it('renders nothing when nothing is happening', () => {
    const { container } = render(<SpaceNowLine now={undefined} nowSec={NOW} />)
    expect(container.innerHTML).toBe('')
  })

  it('sits on the map’s band-condition strip, and is absent without a now from the shack', () => {
    const on = render(<MapInsightRail prop={snap()} outlook={outlook(FLARE)} />)
    const card = on.container.querySelector('.band-cond')?.closest('.mi-card')
    expect(card?.querySelector('.space-now')?.textContent).toContain('60m and below out')
    on.unmount()
    const off = render(<MapInsightRail prop={snap()} outlook={outlook()} />)
    expect(off.container.querySelector('.band-cond')).not.toBeNull()
    expect(off.container.querySelector('.space-now')).toBeNull()
  })

  it('heads the band outlook pane with nothing selected', () => {
    const pane = PANES.find((p) => p.id === 'outlook')!
    const ctx = (o: PathPrediction) => ({ selectedCall: null, bandOutlook: o, outlookOpen: [] }) as unknown as PaneContext
    const on = render(<>{pane.expert(ctx(outlook(FLARE)))}</>)
    expect(on.container.querySelector('.space-now')?.textContent).toContain('S2 proton event')
    on.unmount()
    const off = render(<>{pane.expert(ctx(outlook()))}</>)
    expect(off.container.querySelector('.connect-path')).not.toBeNull()
    expect(off.container.querySelector('.space-now')).toBeNull()
  })
})
