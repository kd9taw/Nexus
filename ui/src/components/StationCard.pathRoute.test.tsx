// @vitest-environment jsdom
//
// Short path or long path on the two surfaces that answer it: the selected station card (the same
// one outlook request the why line uses) and the Path card. The long path is labelled modelled and
// unvalidated wherever it is part of the answer; the heuristic engine, which models no long path,
// shows nothing.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import type { PathGeometry, PathPrediction, Station } from '../types'

const T12 = 1_778_846_400 // 2026-05-15 12:00 UTC

const fx = vi.hoisted(() => ({ pred: null as unknown }))
vi.mock('../api', async (importOriginal) => {
  const real = await importOriginal<typeof import('../api')>()
  return { ...real, getPathOutlook: vi.fn(async () => fx.pred) }
})

import * as api from '../api'
import { StationCard } from './StationCard'
import { PathRoute } from './prop/PathRoute'

function geo(bearingDeg: number, distKm: number, polarFrac: number): PathGeometry {
  return {
    distKm,
    bearingDeg,
    maxGmlatDeg: 70,
    nowHour: 12,
    hours: Array.from({ length: 24 }, (_, h) => ({
      unix: T12 + ((h + 12) % 24) * 3600,
      kp: h === 18 ? 6 : 2,
      kpSource: h === 18 ? ('forecast' as const) : ('current' as const),
      capLatDeg: 63,
      polarFrac,
      greylineFrac: 0,
      sunlitFrac: 1,
      greyMe: false,
      greyDx: false,
    })),
    segmentsNow: [],
  }
}

function band(score: number) {
  const hourly = Array(24).fill(0)
  hourly[12] = score
  return { band: '20m', workability: 'Good', score, window: '—', grayline: false, hourly, reliability: 40 }
}

/** W to VK at Boston's sunset: short path closed, long path open, the long arc over a cap. */
function longPred(): PathPrediction {
  return {
    engine: 'p533',
    bands: [band(0)],
    mufNow: 12,
    mufHourly: Array(24).fill(12),
    geometry: geo(260, 16_000, 0),
    longPath: { engine: 'p533', bands: [band(0.92)], mufNow: 21, mufHourly: Array(24).fill(21), geometry: geo(80, 24_000, 0.2) },
    routes: [
      {
        band: '20m',
        verdict: 'long',
        confidence: 'possible',
        marginDb: 31,
        shortBcr: 0,
        longBcr: 0.92,
        reasons: [{ kind: 'oneClosed', open: 'long' }],
      },
    ],
  }
}

const HINT = 'Beam 80° for 20m: long path (possible).'

const station = {
  call: 'VK2ABC',
  grid: 'QF56',
  snr: -12,
  lastHeardSlot: 100,
  heardCount: 2,
  presence: 'active',
  worked: false,
} as unknown as Station

function card(selected: boolean) {
  return render(
    <StationCard
      station={station}
      myGrid="FN42"
      currentSlot={100}
      selected={selected}
      unread={0}
      need={null}
      needAll={[]}
      onSelect={vi.fn()}
      onCall={vi.fn()}
    />,
  )
}

beforeEach(() => {
  fx.pred = longPred()
  vi.mocked(api.getPathOutlook).mockClear()
})

describe('the heading hint on the station card', () => {
  it('the selected card says which way to beam, labels the long path, and asks once', async () => {
    const { container } = card(true)
    await waitFor(() => expect(container.querySelector('.station-route .route-hint')?.textContent).toBe(HINT))
    expect(container.querySelector('.station-route .route-lp-badge')?.textContent).toBe(
      'modelled, unvalidated for long path',
    )
    // The long route crosses the cap and a storm is forecast: the chip names it as a forecast.
    expect(container.querySelector('.station-route .route-polar-chip')?.textContent).toBe(
      'Polar path · Kp 6 forecast from 1800Z',
    )
    // The reason and the numbers, one hover away.
    const title = container.querySelector('.station-route')!.getAttribute('title')!
    expect(title).toContain('the short path is modelled closed now')
    expect(title).toContain('long path 80°, reliability 92%')
    expect(vi.mocked(api.getPathOutlook).mock.calls).toEqual([['QF56']])
  })

  it('an unselected card shows no hint', async () => {
    const { container } = card(false)
    await new Promise((r) => setTimeout(r, 20))
    expect(container.querySelector('.station-route')).toBeNull()
  })
})

describe('the heading hint on the Path card', () => {
  it('a short-path answer carries no long-path label and no chip when the short route is clear', () => {
    const p = longPred()
    p.routes = [{ ...p.routes![0], verdict: 'short', confidence: 'likely', marginDb: -31, shortBcr: 0.9, longBcr: 0, reasons: [{ kind: 'oneClosed', open: 'short' }] }]
    const { container } = render(<PathRoute pred={p} className="cp-route" />)
    expect(container.querySelector('.route-hint')?.textContent).toBe('Beam 260° for 20m: short path (likely).')
    expect(container.querySelector('.route-lp-badge')).toBeNull()
    expect(container.querySelector('.route-polar-chip')).toBeNull()
  })

  it('the heuristic engine, which models no long path, shows nothing', () => {
    const p: PathPrediction = { ...longPred(), engine: 'heuristic', longPath: undefined, routes: undefined }
    const { container } = render(<PathRoute pred={p} className="cp-route" />)
    expect(container.innerHTML).toBe('')
  })

  it('near the antipode it makes no call and says why: no heading, no label, no chip', () => {
    const p = longPred()
    p.geometry = geo(90, 19_950, 0.2)
    p.longPath!.geometry = geo(270, 20_080, 0.2)
    const { container } = render(<PathRoute pred={p} className="cp-route" />)
    expect(container.querySelector('.route-hint')?.textContent).toBe(
      'Near the antipode: every heading is about the same distance.',
    )
    expect(container.querySelector('.route-lp-badge')).toBeNull()
    expect(container.querySelector('.route-polar-chip')).toBeNull()
  })
})
