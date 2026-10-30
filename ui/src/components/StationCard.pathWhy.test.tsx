// @vitest-environment jsdom
//
// The path "why" line on the two surfaces that show it: the selected station card (one outlook
// request for the selected station, never one per row) and the Path card's disclosure (sentence
// first, the numbers one click away).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { PathPrediction, Station } from '../types'

const T12 = 1_778_846_400 // 2026-05-15 12:00 UTC

const fx = vi.hoisted(() => ({ pred: null as unknown }))
vi.mock('../api', async (importOriginal) => {
  const real = await importOriginal<typeof import('../api')>()
  return { ...real, getPathOutlook: vi.fn(async () => fx.pred) }
})

import * as api from '../api'
import { StationCard } from './StationCard'
import { PathWhy } from './prop/PathWhy'

function polarPred(): PathPrediction {
  const hourly = Array(24).fill(0.1)
  hourly[12] = 0.8
  return {
    engine: 'p533',
    bands: [
      {
        band: '20m',
        workability: 'Good',
        score: 0.8,
        window: '—',
        grayline: false,
        hourly,
        reliability: 40,
        detail: Array.from({ length: 24 }, () => ({ layer: 'F2' as const, hops: 3, takeoffDeg: 4.9, estimated: false })),
      },
    ],
    mufNow: 19,
    mufHourly: Array(24).fill(19),
    geometry: {
      distKm: 10800,
      bearingDeg: 335,
      maxGmlatDeg: 72,
      nowHour: 12,
      hours: Array.from({ length: 24 }, (_, h) => ({
        unix: T12 + ((h + 12) % 24) * 3600,
        kp: h === 18 ? 6 : 2,
        kpSource: h === 18 ? ('forecast' as const) : ('current' as const),
        capLatDeg: 63,
        polarFrac: 0.25,
        greylineFrac: 0,
        sunlitFrac: 1,
        greyMe: false,
        greyDx: false,
      })),
    },
  }
}

const LINE = '20m short path, 3 F2 hops, crosses the polar cap — an aurora (Kp 6 forecast at 1800Z) may weaken it.'

const station = {
  call: 'JA1XYZ',
  grid: 'PM95',
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
  fx.pred = polarPred()
  vi.mocked(api.getPathOutlook).mockClear()
})

describe('the station card', () => {
  it('the selected card asks for its path once and says why', async () => {
    const { container } = card(true)
    await waitFor(() => expect(container.querySelector('.station-why')?.textContent).toBe(LINE))
    expect(vi.mocked(api.getPathOutlook).mock.calls).toEqual([['PM95']])
    // The numbers behind it, one hover away.
    const title = container.querySelector('.station-why')!.getAttribute('title')!
    expect(title).toContain('Mode 3F2, take-off 4.9°')
    expect(title).toContain('Auroral zone and polar cap: 25% of the path')
  })

  it('an unselected card asks for nothing and shows no line', async () => {
    const { container } = card(false)
    // Let any effect run before asserting the absence.
    await new Promise((r) => setTimeout(r, 20))
    expect(api.getPathOutlook).not.toHaveBeenCalled()
    expect(container.querySelector('.station-why')).toBeNull()
  })
})

describe('the Path card disclosure', () => {
  it('leads with the sentence and opens to the numbers', () => {
    const { container } = render(<PathWhy pred={polarPred()} />)
    const details = container.querySelector('details.cp-why') as HTMLDetailsElement
    expect(container.querySelector('.cp-why-line')?.textContent).toBe(LINE)
    expect(details.open).toBe(false)
    fireEvent.click(container.querySelector('summary')!)
    const rows = [...container.querySelectorAll('.cp-why-detail li')].map((li) => li.textContent)
    expect(rows[0]).toBe('Mode 3F2, take-off 4.9° (P.533 monthly median)')
    expect(rows[rows.length - 1]).toContain('does not include storms')
  })

  it('renders nothing without geometry', () => {
    const { container } = render(<PathWhy pred={{ ...polarPred(), geometry: undefined }} />)
    expect(container.innerHTML).toBe('')
  })
})
