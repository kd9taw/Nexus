// @vitest-environment jsdom
//
// WORK FROM THE NEEDED BOARD TAGS THE PARK (operator decision, 2026-09-17).
//
// A park or summit row on the Needed board now names its activation (`park`), and working it sets
// the hunt the way HUNT and a map double-click already did — so the contact it leads to is logged
// with the park: it counts as hunted, hides from the POTA board, and exports with SIG/SIG_INFO.
// Before, a POTA contact worked from the board was logged as an ordinary QSO.
//
// This is the torn-off board, with the REAL NeededPanel: the click under test is the one on the
// operator's row, not a stub's callback.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import type { NeedAlert } from './types'

const rows = vi.hoisted(() => ({
  park: {
    call: 'K1ABC',
    entity: 'United States',
    band: '20m',
    zone: 5,
    tags: ['NewPark', 'Pota'],
    priority: 20,
    headline: 'POTA US-0001 (Test park)',
    mode: 'Digital',
    freqMhz: 14.074,
    park: { program: 'POTA', reference: 'US-0001' },
  },
  plain: {
    call: 'W9XYZ',
    entity: 'United States',
    band: '20m',
    zone: 4,
    tags: ['NewBand'],
    priority: 50,
    headline: 'New band slot',
    mode: 'Digital',
    freqMhz: 14.076,
    park: null,
  },
}))

vi.mock('./api', () => ({
  subscribeSnapshot: vi.fn(() => () => {}),
  selectPeer: vi.fn(() => Promise.resolve(null)),
  getBandPlan: vi.fn(() => Promise.resolve([])),
  getPropagation: vi.fn(() => Promise.resolve(null)),
  getNeedAlerts: vi.fn(() => Promise.resolve([rows.park, rows.plain] as NeedAlert[])),
  getSettings: vi.fn(() => Promise.resolve(null)),
  workSpot: vi.fn(() => Promise.resolve(null)),
  setFrequency: vi.fn(() => Promise.resolve(null)),
  setHuntTarget: vi.fn(() => Promise.resolve(null)),
  pointRotator: vi.fn(() => Promise.resolve(null)),
  readRotator: vi.fn(() => Promise.resolve(null)),
  openQrzPage: vi.fn(() => Promise.resolve(null)),
}))

import { DetachedPanel } from './DetachedPanel'
import { setHuntTarget, workSpot } from './api'

const row = (call: string) =>
  screen.getAllByRole('row').find((r) => r.getAttribute('aria-label')?.includes(call)) as HTMLElement

beforeEach(() => {
  vi.mocked(setHuntTarget).mockClear()
  vi.mocked(workSpot).mockClear()
})
afterEach(cleanup)

describe('the torn-off Needed board tags a park when a park row is worked', () => {
  it('sets the hunt to the row’s own call and park, and still works the spot', async () => {
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(row('K1ABC')).toBeTruthy())
    fireEvent.click(row('K1ABC'))
    expect(setHuntTarget).toHaveBeenCalledWith('K1ABC', 'POTA', 'US-0001')
    expect(workSpot).toHaveBeenCalledWith('digital', 14.074, '20m', 'K1ABC', undefined)
  })

  // CONTROL — without it, a version that tags every Work would pass the test above.
  it('a need that is not an activation tags nothing', async () => {
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(row('W9XYZ')).toBeTruthy())
    fireEvent.click(row('W9XYZ'))
    expect(setHuntTarget).not.toHaveBeenCalled()
    expect(workSpot).toHaveBeenCalledWith('digital', 14.076, '20m', 'W9XYZ', undefined)
  })
})
