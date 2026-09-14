// @vitest-environment jsdom
//
// #244 — Journey's Personal bests follows the Units setting. The longest-distance best was a
// miles string written by the backend, so a metric operator read miles on this one card while
// every other distance in the app followed Units. The backend now sends the raw kilometres
// (`distanceKm`) and the card formats them at the display edge like everything else.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
import type { JourneySummary } from '../types'
import { UNITS_KEY } from '../units'

const summary = {
  level: 1,
  xp: 10,
  xpIntoLevel: 10,
  xpForLevel: 100,
  totalQsos: 1,
  nextMilestone: null,
  firsts: [],
  ladders: [],
  collections: [],
  feats: [],
  bests: [
    {
      id: 'longest',
      title: 'Longest distance',
      value: '8388 mi',
      detail: 'ZL3ABC · New Zealand',
      distanceKm: 13500,
    },
    { id: 'busiest-day', title: 'Most QSOs in a day', value: '1', detail: '2026-09-14', distanceKm: null },
  ],
  streak: { enabled: false, weeks: 0, bestWeeks: 0, activeThisWeek: false },
} as unknown as JourneySummary

vi.mock('../api', () => ({
  getJourney: vi.fn(async () => summary),
  getSettings: vi.fn(async () => ({ mycall: 'KD9TAW' })),
}))

import { JourneyView } from './JourneyView'

beforeEach(() => localStorage.clear())
afterEach(cleanup)

async function bestValue(): Promise<string> {
  await waitFor(() => expect(screen.queryByText('Longest distance')).not.toBeNull())
  const card = screen.getByText('Longest distance').closest('.jy-best')!
  return card.querySelector('.jy-best-v')!.textContent ?? ''
}

describe('Personal bests follow Units (#244)', () => {
  it('metric: the longest distance reads in km', async () => {
    localStorage.setItem(UNITS_KEY, 'metric')
    render(<JourneyView />)
    expect(await bestValue()).toBe('13500 km')
  })

  it('imperial: the longest distance reads in miles', async () => {
    localStorage.setItem(UNITS_KEY, 'imperial')
    render(<JourneyView />)
    // 13500 km / 1.609344 = 8388.5 mi, which rounds to 8389.
    expect(await bestValue()).toBe('8389 mi')
  })

  it('control: a best that is not a distance keeps the backend value', async () => {
    localStorage.setItem(UNITS_KEY, 'metric')
    render(<JourneyView />)
    await bestValue()
    const busiest = screen.getByText('Most QSOs in a day').closest('.jy-best')!
    expect(busiest.querySelector('.jy-best-v')!.textContent).toBe('1')
  })
})
