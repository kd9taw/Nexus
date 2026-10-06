// @vitest-environment jsdom
// Field repro (0.24.6): assigning "Openings Log" to a Connect slot black-screened the
// whole app, and the black screen survived restart. The pane's empty→populated
// transition is the trigger: first render (no episodes yet) must bail to the Basic
// hint, and the render AFTER get_openings_log resolves must show the log — not throw.
// A hook declared below the early return changes the hook count between those two
// renders, and with no error boundary React unmounts the entire root.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import type { OpeningEpisode } from '../../types'

vi.mock('../../api', () => ({ getOpeningsLog: vi.fn() }))
import { getOpeningsLog } from '../../api'
import { OpeningsLogPane } from './OpeningsLogPane'
import { setUnitsMirror } from '../../units'

afterEach(() => {
  cleanup()
  localStorage.clear()
})

const EPISODE: OpeningEpisode = {
  band: '6m',
  mode: 'Sporadic-E',
  startedUtc: 1753000000,
  endedUtc: 1753003000,
  durationSecs: 3000,
  onsetKnown: true,
  peakZ: 3,
  maxKm: 1450,
  peakStations: 12,
  bearingDeg: 245,
  octant: 'SW',
}

describe('OpeningsLogPane', () => {
  it('renders the log once episodes arrive (empty→populated must not throw)', async () => {
    vi.mocked(getOpeningsLog).mockResolvedValue([EPISODE])
    render(<OpeningsLogPane />)
    // Paint 1 is the no-data bail-out; the fetch then resolves and this re-render
    // is exactly the moment the field crash happened.
    expect(await screen.findByText(/1 opening/)).toBeTruthy()
    expect(screen.getByText('Sporadic-E')).toBeTruthy()
  })

  it('survives a journal row with missing fields', async () => {
    // openings_log.json is FILE-backed and outlives any browser-data wipe, so a row
    // written by an older build (or a truncated write) comes back every launch. It
    // must format to something honest, never throw — this pane has no boundary of
    // its own below `.shell`.
    vi.mocked(getOpeningsLog).mockResolvedValue([
      EPISODE,
      { band: '2m', mode: 'Tropo' } as unknown as OpeningEpisode,
    ])
    render(<OpeningsLogPane />)
    expect(await screen.findByText(/2 openings/)).toBeTruthy()
    expect(screen.getByText('Tropo')).toBeTruthy()
  })

  // The DX column names each opening's longest path in the units the operator chose. It printed
  // kilometres whatever Settings ▸ Units said. 1450 km is 901 mi.
  it('states the longest path in miles under Imperial and in km under Metric', async () => {
    vi.mocked(getOpeningsLog).mockResolvedValue([EPISODE])
    const dx = async (units: 'metric' | 'imperial') => {
      setUnitsMirror(units)
      const { container, unmount } = render(<OpeningsLogPane />)
      await screen.findByText(/1 opening/)
      const text = container.querySelector('.openings-log-dx')?.textContent
      unmount()
      return text
    }
    expect(await dx('imperial')).toBe('~901 mi SW')
    expect(await dx('metric')).toBe('~1450 km SW')
  })
})
