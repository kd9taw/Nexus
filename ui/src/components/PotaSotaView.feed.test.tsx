// @vitest-environment jsdom
//
// THE POTA/SOTA LISTS ARE WINDOW FEEDS (features/connectFeeds `POTA_SPOTS`, `SOTA_SPOTS`). Every
// board in one window that shows a programme shares its poll — the view, a Conditions box and the
// dashboard rail's box ask pota.app once a minute between them, not once each — and they all show
// the same answer. Two boards are mounted side by side here, as the view and a box (`pane`), whose
// filter records differ, so nothing but the feed can make them agree.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { AppSnapshot, OtaSpot } from '../types'
import { t } from '../i18n'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async (_program: string, _cached?: boolean): Promise<OtaSpot[]> => []),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))
vi.mock('../api', () => ({
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  setActivationState: vi.fn(), clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(),
  importHuntedParksCsv: vi.fn(), selfSpot: vi.fn(),
}))

import { PotaSotaView } from './PotaSotaView'
import { Toasts } from './Toasts'
import { dismissToast, subscribeToasts } from '../toast'

const spot = (activator: string, reference: string, program: 'POTA' | 'SOTA' = 'POTA'): OtaSpot => ({
  program, reference, name: 'Test park', activator, freqKhz: 14_285, mode: 'SSB',
  spotter: null, comment: null, grid: null, newPark: false, bandOpen: false, huntedToday: false,
})
const snap = { hunt: null, radio: { dialMhz: 14.285 }, logTick: 1 } as unknown as AppSnapshot

/** The view and a box, side by side in one window. */
const twoBoards = () =>
  render(
    <>
      <div data-testid="view"><PotaSotaView snap={snap} /></div>
      <div data-testid="box"><PotaSotaView snap={snap} pane /></div>
      <Toasts />
    </>,
  )
const fetched = (program: string) => api.getOtaSpots.mock.calls.filter((c) => c.length === 1 && c[0] === program).length
const shows = (board: 'view' | 'box', call: string) => within(screen.getByTestId(board)).queryByText(call) != null

beforeEach(() => {
  // Only the poll's interval is faked: the board's other timers, and waitFor's, keep real time.
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  api.getOtaSpots.mockReset()
  api.getOtaSpots.mockImplementation(async (program: string) => (program === 'POTA' ? [spot('K1ABC', 'US-0001')] : []))
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  localStorage.clear()
  let live: { id: number }[] = []
  subscribeToasts((all) => { live = all })()
  for (const toast of live) dismissToast(toast.id)
})

describe('two POTA/SOTA boards in one window share one poll', () => {
  it('ask pota.app once on arrival and once a minute between them, and show the same rows', async () => {
    twoBoards()
    await waitFor(() => expect(shows('view', 'K1ABC') && shows('box', 'K1ABC')).toBe(true))
    expect(fetched('POTA'), 'each board fetched on arrival').toBe(1)
    api.getOtaSpots.mockImplementation(async (program: string) => (program === 'POTA' ? [spot('W9XYZ', 'US-0002')] : []))
    await act(async () => {
      vi.advanceTimersByTime(60_000)
    })
    await waitFor(() => expect(shows('view', 'W9XYZ') && shows('box', 'W9XYZ')).toBe(true))
    expect(fetched('POTA'), 'each board polled on its own timer').toBe(2)
  })

  it('a board on Both and a board on POTA share the POTA poll', async () => {
    localStorage.setItem('nexus.connect.ota.program', 'Both')
    twoBoards()
    await waitFor(() => expect(shows('box', 'K1ABC')).toBe(true))
    expect(fetched('POTA'), 'POTA was fetched once per board').toBe(1)
    expect(fetched('SOTA')).toBe(1)
  })

  it('a Refresh on one board asks once, and every board shows the answer', async () => {
    twoBoards()
    await waitFor(() => expect(shows('box', 'K1ABC')).toBe(true))
    api.getOtaSpots.mockImplementation(async (program: string) => (program === 'POTA' ? [spot('W9XYZ', 'US-0002')] : []))
    fireEvent.click(within(screen.getByTestId('view')).getByRole('button', { name: t('ota.refresh.title') }))
    await waitFor(() => expect(shows('view', 'W9XYZ') && shows('box', 'W9XYZ')).toBe(true))
    expect(fetched('POTA')).toBe(2)
  })

  it('a failed fetch says so once, however many boards show it, and the list empties as it always has', async () => {
    twoBoards()
    await waitFor(() => expect(shows('box', 'K1ABC')).toBe(true))
    api.getOtaSpots.mockImplementation(async () => {
      throw new Error('offline')
    })
    await act(async () => {
      vi.advanceTimersByTime(60_000)
    })
    await waitFor(() => expect(document.querySelectorAll('.ui-toast').length).toBeGreaterThan(0))
    expect(document.querySelectorAll('.ui-toast'), 'one failed fetch, one toast').toHaveLength(1)
    await waitFor(() => expect(shows('view', 'K1ABC') || shows('box', 'K1ABC')).toBe(false))
  })
})
