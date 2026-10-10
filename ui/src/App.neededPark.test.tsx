// @vitest-environment jsdom
//
// A NEW PARK WORKED FROM THE NEEDED BOARD FILLS THE LOG LINE, AND NOTHING IS HUNTED — in the real App.
//
// The operator: "There's no way to clear a 'POTA - Hunted' tag if you click through from 'Needed'. It
// should instead simply populate the call data (and the park number) without setting it as 'Hunted'.
// Hunted should only be used if you chase a park from the POTA tab." A Phone or CW park row opens its
// cockpit with the call AND the park in the log line, and sets no hunt. The rows that open no log line
// (FT8/FT4, RTTY) still tag the hunt: App.neededHunt.test.tsx holds those. Mounts the REAL App on the
// Needed view, so the click travels the board, App's work handler and the cockpit's own log line.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { AppSnapshot, NeedAlert } from './types'

const alerts = vi.hoisted(() => [
  {
    call: 'K9ABC', entity: 'United States', band: '20m', zone: 4, tags: ['NewPark', 'Pota'],
    priority: 20, headline: 'POTA US-1000 (Test park)', mode: 'Phone', freqMhz: 14.285,
    park: { program: 'POTA', reference: 'US-1000' },
  },
  {
    call: 'N0CWP', entity: 'United States', band: '20m', zone: 4, tags: ['NewPark', 'Pota'],
    priority: 20, headline: 'POTA US-2000 (Test park)', mode: 'CW', freqMhz: 14.062,
    park: { program: 'POTA', reference: 'US-2000' },
  },
  // Not an activation: the control for every test below.
  {
    call: 'W9XYZ', entity: 'United States', band: '20m', zone: 4, tags: ['NewBand'],
    priority: 50, headline: 'New band slot', mode: 'Phone', freqMhz: 14.25, park: null,
  },
])

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  return {
    ...auto,
    ...appApiAnswers(),
    getNeedAlerts: vi.fn(async () => alerts as unknown as NeedAlert[]),
    // The log line looks the park up; nothing here knows it.
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    qrzLookup: vi.fn(async () => null),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import App from './App'
import { setHuntTarget, subscribeSnapshot, workSpot } from './api'
import { APP_SNAPSHOT, COCKPIT_MAIN } from './appCockpits.testkit'

// THE BUDGET: App.neededHunt.test.tsx's, for the same mount (15 s; a test that hangs still fails).
vi.setConfig({ testTimeout: 15_000 })

const row = (call: string) =>
  screen.getAllByRole('row').find((r) => r.getAttribute('aria-label')?.includes(call)) as HTMLElement

beforeEach(() => {
  localStorage.clear()
  // The Needed board is a core section; the auto-pop would move it into its own window.
  localStorage.setItem('nexus.needed.autopop', 'off')
  vi.mocked(setHuntTarget).mockClear()
  vi.mocked(workSpot).mockClear()
  vi.mocked(subscribeSnapshot).mockImplementation((() => () => {}) as never)
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

/** Work a row on the Needed view; answer the log line of the cockpit it opened. */
async function work(call: string, cockpit: 'phone' | 'cw'): Promise<{ call: HTMLInputElement; park: HTMLInputElement }> {
  window.location.hash = '#needed'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(row(call)).toBeTruthy())
  fireEvent.click(row(call))
  await waitFor(() => expect(workSpot).toHaveBeenCalled())
  const line = await waitFor(() => {
    const el = document.querySelector<HTMLElement>(`${COCKPIT_MAIN[cockpit]} .log-entry`)
    expect(el, `the ${cockpit} cockpit's log line never mounted`).toBeTruthy()
    return el!
  })
  expect(document.querySelector('.view-crash')).toBeNull()
  const callBox = line.querySelector<HTMLInputElement>('input.le-call')!
  await waitFor(() => expect(callBox.value).toBe(call))
  return { call: callBox, park: line.querySelector<HTMLInputElement>('input.le-park-ref')! }
}

describe('a park row worked from the Needed board fills the log line and hunts nothing', () => {
  it('Phone: the call and the park in the log line, no hunt', async () => {
    const line = await work('K9ABC', 'phone')
    await waitFor(() => expect(line.park.value, 'the park never reached the log line').toBe('US-1000'))
    expect(setHuntTarget, 'a Needed click set a hunt').not.toHaveBeenCalled()
    // The park rides the work as well, so the snapshot's echo of this click says the same.
    expect(vi.mocked(workSpot).mock.calls).toEqual([
      ['phone', 14.285, '20m', 'K9ABC', undefined, { program: 'POTA', reference: 'US-1000' }],
    ])
  })

  it('CW: the call and the park in the log line, no hunt', async () => {
    const line = await work('N0CWP', 'cw')
    await waitFor(() => expect(line.park.value, 'the park never reached the log line').toBe('US-2000'))
    expect(setHuntTarget, 'a Needed click set a hunt').not.toHaveBeenCalled()
  })

  // A switched-off cockpit is never opened, so no log line takes the park: the hunt still carries it.
  it('with the Phone cockpit switched off, a Phone park row still tags the hunt', async () => {
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { phone: false } }))
    window.location.hash = '#needed'
    render(<App />)
    await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
    await waitFor(() => expect(row('K9ABC')).toBeTruthy())
    fireEvent.click(row('K9ABC'))
    await waitFor(() => expect(setHuntTarget).toHaveBeenCalledWith('K9ABC', 'POTA', 'US-1000'))
    expect(workSpot).not.toHaveBeenCalled()
  })

  // CONTROL — a row that is not an activation works exactly as it did: call in, no park, no hunt.
  it('a row that is not an activation fills the call and no park', async () => {
    const line = await work('W9XYZ', 'phone')
    expect(line.park.value).toBe('')
    expect(setHuntTarget).not.toHaveBeenCalled()
    expect(vi.mocked(workSpot).mock.calls).toEqual([['phone', 14.25, '20m', 'W9XYZ', undefined]])
  })
})

// THE TORN-OFF BOARD (it opens at launch by default) cannot reach this window's log line; its work
// does, through the snapshot's work hint: `workCall`, and now `workPark` with it.
describe('a park row worked on the pop-out board reaches this window’s log line', () => {
  async function hinted(workPark: { program: string; reference: string } | null) {
    let push: ((s: AppSnapshot) => void) | null = null
    vi.mocked(subscribeSnapshot).mockImplementation(((fn: (s: AppSnapshot) => void) => {
      push = fn
      return () => {}
    }) as never)
    window.location.hash = '#needed'
    render(<App />)
    await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
    await waitFor(() => expect(push).toBeTruthy())
    // The first snapshot is the baseline (no work replayed on a reload); the next carries the work.
    act(() => push!({ ...APP_SNAPSHOT, workTick: 0 } as unknown as AppSnapshot))
    act(() =>
      push!({ ...APP_SNAPSHOT, workTick: 1, workView: 'phone', workCall: 'K9ABC', workPark } as unknown as AppSnapshot),
    )
    const line = await waitFor(() => {
      const el = document.querySelector<HTMLElement>(`${COCKPIT_MAIN.phone} .log-entry`)
      expect(el, 'the hint never opened the Phone cockpit').toBeTruthy()
      return el!
    })
    const callBox = line.querySelector<HTMLInputElement>('input.le-call')!
    await waitFor(() => expect(callBox.value).toBe('K9ABC'))
    return line.querySelector<HTMLInputElement>('input.le-park-ref')!
  }

  it('fills the call and the park', async () => {
    const park = await hinted({ program: 'POTA', reference: 'US-1000' })
    await waitFor(() => expect(park.value, 'the pop-out’s park never reached the log line').toBe('US-1000'))
    expect(setHuntTarget).not.toHaveBeenCalled()
  })

  // CONTROL — a work with no park (the band map, a spot) fills the call alone, as before.
  it('a work with no park fills the call alone', async () => {
    const park = await hinted(null)
    expect(park.value).toBe('')
  })
})
