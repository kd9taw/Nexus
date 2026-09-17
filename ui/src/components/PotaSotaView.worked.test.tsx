// @vitest-environment jsdom
//
// HIDE WORKED TODAY — the POTA/SOTA board (operator decisions, 2026-09-17).
//
// An activator already logged at the park they are spotted at, since 0000Z, is an activation the
// operator has hunted: the board hides it by default and says how many it hid. It comes back at
// 0000Z or at a new park — both decided by the station (`huntedToday`), which these tests take as
// given; the board's job is to hide, count, badge, point at the chip, and re-read when the log
// changes WITHOUT fetching pota.app again.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
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
  clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
  selfSpot: vi.fn(),
}))

import { PotaSotaView } from './PotaSotaView'
// The REAL toast bus and its host — so "no toast" is what the operator would not see, rather
// than a spy that `withErrorToast` never reaches (it calls its own module-local `pushToast`).
import { Toasts } from './Toasts'
import { dismissToast, subscribeToasts } from '../toast'

const spot = (activator: string, reference: string, over: Partial<OtaSpot> = {}): OtaSpot => ({
  program: 'POTA',
  reference,
  name: 'Test park',
  activator,
  freqKhz: 14_285,
  mode: 'SSB',
  spotter: null,
  comment: null,
  grid: null,
  newPark: false,
  bandOpen: false,
  huntedToday: false,
  ...over,
})
const snap = (logTick: number) => ({ hunt: null, radio: { dialMhz: 14.285 }, logTick }) as unknown as AppSnapshot

beforeEach(() => {
  // BRACES, and they are load-bearing: a concise arrow returns `mockReset()`'s value — the mock
  // itself — and vitest calls a hook's return as its teardown, so the mock was being invoked with
  // no arguments after every test. Harmless while it resolved; an unhandled rejection the moment
  // one test made it reject.
  api.getOtaSpots.mockReset()
})
afterEach(() => {
  cleanup()
  localStorage.clear()
  // The toast bus is module state, so one test's toast would otherwise be the next test's
  // "there is a toast on screen". Drain it.
  let live: { id: number }[] = []
  subscribeToasts((all) => { live = all })()
  for (const toast of live) dismissToast(toast.id)
})

describe('Hide worked today', () => {
  it('is on by default: hides the activation hunted today and counts it on the chip', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001', { huntedToday: true }), spot('W9XYZ', 'US-0002')])
    render(<PotaSotaView snap={snap(1)} />)
    await screen.findByText('W9XYZ')
    expect(screen.queryByText('K1ABC')).toBeNull()
    expect(screen.getByRole('button', { name: t('ota.filter.hideWorked.hidden', { count: 1 }) })).toBeTruthy()
  })

  it('off shows the row again with its WORKED TODAY badge', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001', { huntedToday: true }), spot('W9XYZ', 'US-0002')])
    render(<PotaSotaView snap={snap(1)} />)
    fireEvent.click(await screen.findByRole('button', { name: t('ota.filter.hideWorked.hidden', { count: 1 }) }))
    expect(screen.getByText('K1ABC')).toBeTruthy()
    expect(screen.getByText(t('ota.badge.workedToday'))).toBeTruthy()
    // CONTROL: the unworked row carries no badge, so the badge is about the row, not the board.
    expect(screen.getAllByText(t('ota.badge.workedToday'))).toHaveLength(1)
  })

  it('offers no chip and hides nothing when the rows carry no flag (an older station)', async () => {
    const legacy = spot('K1ABC', 'US-0001')
    delete (legacy as Partial<OtaSpot>).huntedToday
    api.getOtaSpots.mockResolvedValue([legacy])
    render(<PotaSotaView snap={snap(1)} />)
    await screen.findByText('K1ABC')
    expect(screen.queryByRole('button', { name: t('ota.filter.hideWorked.label') })).toBeNull()
  })

  it('points at the chip when it is what empties the board', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001', { huntedToday: true })])
    render(<PotaSotaView snap={snap(1)} />)
    expect(await screen.findByText(t('ota.empty.worked', { count: 1 }))).toBeTruthy()
  })

  it('re-reads from the cache when the log changes, and never without a change', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001')])
    const view = render(<PotaSotaView snap={snap(1)} />)
    await screen.findByText('K1ABC')
    expect(api.getOtaSpots).toHaveBeenCalledWith('POTA')
    api.getOtaSpots.mockClear()

    // A new snapshot with the SAME log: nothing to re-derive.
    view.rerender(<PotaSotaView snap={snap(1)} />)
    await act(async () => { await Promise.resolve() })
    expect(api.getOtaSpots).not.toHaveBeenCalled()

    // The contact is logged: the station now calls the activation hunted.
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001', { huntedToday: true })])
    view.rerender(<PotaSotaView snap={snap(2)} />)
    await waitFor(() => expect(screen.queryByText('K1ABC')).toBeNull())
    expect(api.getOtaSpots).toHaveBeenCalledWith('POTA', true)
    expect(api.getOtaSpots, 'a log change must never fetch pota.app').not.toHaveBeenCalledWith('POTA')
  })

  it('keeps what it shows when nothing is cached, and says nothing about it', async () => {
    // A cold cache is not an operator's problem: the next poll fetches anyway, so the board keeps
    // its rows and raises nothing. An error toast on a log change would be pure noise, and the
    // log changes several times per contact.
    api.getOtaSpots.mockResolvedValueOnce([spot('K1ABC', 'US-0001')])
    const view = render(<><PotaSotaView snap={snap(1)} /><Toasts /></>)
    await screen.findByText('K1ABC')
    api.getOtaSpots.mockRejectedValueOnce('No POTA spots cached yet.')
    view.rerender(<><PotaSotaView snap={snap(2)} /><Toasts /></>)
    await waitFor(() => expect(api.getOtaSpots).toHaveBeenCalledWith('POTA', true))
    expect(screen.getByText('K1ABC'), 'the board keeps the rows it had').toBeTruthy()
    expect(document.querySelectorAll('.ui-toast')).toHaveLength(0)
  })

  // POSITIVE CONTROL for the silence above, and it is the whole of what makes that assertion
  // mean anything: a FETCH that fails is a different event — the operator asked for fresh spots
  // and did not get them — and it still says so, through the same real toast bus and host.
  it('a failed FETCH still says so', async () => {
    api.getOtaSpots.mockImplementation(async () => { throw new Error('offline') })
    render(<><PotaSotaView snap={snap(1)} /><Toasts /></>)
    await waitFor(() => expect(document.querySelectorAll('.ui-toast').length).toBeGreaterThan(0))
    expect(document.querySelector('.ui-toast-msg')?.textContent).toContain(
      t('ota.spots.failed', { program: 'POTA' }),
    )
  })
})
