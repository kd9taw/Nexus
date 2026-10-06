// @vitest-environment jsdom
//
// A JOURNEY UNLOCK TOAST NAMES ITS DISTANCE IN THE OPERATOR'S UNITS.
//
// The toast for a newly unlocked distance first carried the backend's detail line, "ZL3ABC · 8388 mi",
// composed in Rust whatever Units said. The distance now crosses as a km token the screen writes.
// The setting and the OS locale DISAGREE in each case, so a toast that read only one of them cannot
// pass. 13500 km is 8389 mi.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, waitFor } from '@testing-library/react'
import type { JourneySummary } from './types'
import { setUnitsMirror } from './units'

const SUMMARY = {
  firsts: [
    {
      id: 'first-5000mi',
      title: 'First 5,000-Mile Contact',
      meaning: 'You spanned more than 5,000 miles in a single contact.',
      heritage: '',
      unlocked: true,
      whenUnix: 1,
      detail: 'ZL3ABC · {km:13500}',
    },
  ],
  feats: [],
  ladders: [],
} as unknown as JourneySummary

vi.mock('./api', () => ({ getJourney: vi.fn(async () => SUMMARY) }))
vi.mock('./toast', () => ({ pushToast: vi.fn() }))

import { pushToast } from './toast'
import { useJourneyUnlocks } from './useJourneyUnlocks'

function Host() {
  useJourneyUnlocks(true)
  return null
}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  // Not the first run: a first run baselines silently and toasts nothing.
  localStorage.setItem('nexus-journey-seen', '[]')
  vi.mocked(pushToast).mockClear()
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

async function toast(setting: 'imperial' | 'metric', locale: string): Promise<string> {
  vi.spyOn(navigator, 'language', 'get').mockReturnValue(locale)
  setUnitsMirror(setting)
  render(<Host />)
  await waitFor(() => expect(vi.mocked(pushToast)).toHaveBeenCalled())
  return String(vi.mocked(pushToast).mock.calls[0][0])
}

describe('the unlock toast for a distance first', () => {
  it('Imperial beats a British locale: miles', async () => {
    expect(await toast('imperial', 'en-GB')).toBe('✦ First 5,000-Mile Contact — ZL3ABC · 8389 mi')
  })

  it('Metric beats a US locale: kilometres', async () => {
    expect(await toast('metric', 'en-US')).toBe('✦ First 5,000-Mile Contact — ZL3ABC · 13500 km')
  })
})
