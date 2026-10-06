// @vitest-environment jsdom
//
// #244 — Journey's Personal bests follows the Units setting. The longest-distance best was a
// miles string written by the backend, so a metric operator read miles on this one card while
// every other distance in the app followed Units. The backend now sends the raw kilometres
// (`distanceKm`) and the card formats them at the display edge like everything else.
//
// The firsts and feats followed later (2026-10-06): a distance first named its contact as
// "ZL3ABC · 8388 mi" and the sporadic-E feat its threshold as "over 1,000 km", both composed in
// Rust whatever Units said. Each distance now crosses as a km token the screen writes.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/react'
import type { JourneySummary } from '../types'
import { UNITS_KEY, setUnitsMirror } from '../units'
import { shareCard } from '../features/shareCard'

const summary = {
  level: 1,
  xp: 10,
  xpIntoLevel: 10,
  xpForLevel: 100,
  totalQsos: 1,
  nextMilestone: null,
  firsts: [
    {
      id: 'first-5000mi',
      title: 'First 5,000-Mile Contact',
      meaning: 'You spanned more than 5,000 miles in a single contact.',
      heritage: 'Five thousand miles usually means a real intercontinental opening worked.',
      unlocked: true,
      whenUnix: 1,
      detail: 'ZL3ABC · {km:13500}',
    },
  ],
  ladders: [],
  collections: [],
  feats: [
    {
      id: 'es-season',
      title: 'Sporadic-E Summer',
      meaning: 'Work 6 m over {km:1000} during the summer sporadic-E season.',
      heritage: 'Each summer the E layer thickens into fleeting clouds.',
      tier: 'silver',
      unlocked: true,
      current: 1,
      target: 1,
      unit: 'opening',
      detail: 'EA1ABC',
      gated: false,
      gateHint: null,
    },
  ],
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
vi.mock('../features/shareCard', () => ({ shareCard: vi.fn() }))

import { JourneyView } from './JourneyView'

beforeEach(() => localStorage.clear())
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

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

describe('the distance firsts and feats follow Units', () => {
  /** The Units setting as App stores it, and the OS locale the 'auto' setting would read. */
  function units(setting: 'imperial' | 'metric', locale: string): void {
    vi.spyOn(navigator, 'language', 'get').mockReturnValue(locale)
    setUnitsMirror(setting)
  }
  async function view() {
    render(<JourneyView />)
    await waitFor(() => expect(screen.queryByText('Sporadic-E Summer')).not.toBeNull())
    return {
      detail: document.querySelector('.jy-first-detail')?.textContent ?? '',
      meaning: document.querySelector('.jy-feat-meaning')?.textContent ?? '',
    }
  }

  it('Imperial beats a British locale: miles', async () => {
    units('imperial', 'en-GB')
    const { detail, meaning } = await view()
    // 13500 km is 8388.5 mi, which rounds to 8389; 1000 km is 621 mi.
    expect(detail).toBe('ZL3ABC · 8389 mi')
    expect(meaning).toBe('Work 6 m over 621 mi during the summer sporadic-E season.')
  })

  it('Metric beats a US locale: kilometres', async () => {
    units('metric', 'en-US')
    const { detail, meaning } = await view()
    expect(detail).toBe('ZL3ABC · 13500 km')
    expect(meaning).toBe('Work 6 m over 1000 km during the summer sporadic-E season.')
  })

  it("a shared feat card carries the operator's units, not a token", async () => {
    units('imperial', 'en-GB')
    await view()
    fireEvent.click(document.querySelector('.jy-feat .jy-share') as HTMLElement)
    expect(vi.mocked(shareCard)).toHaveBeenCalledWith(
      expect.objectContaining({ sub: 'Work 6 m over 621 mi during the summer sporadic-E season.' }),
    )
  })
})
