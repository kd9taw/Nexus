// @vitest-environment jsdom
//
// THE TORN-OFF NEEDED BOARD: A PHONE OR CW PARK ROW HUNTS NOTHING. The board opens in its own window
// at launch by default, so it is the Needed board most operators click. It cannot fill the main
// window's log line itself; the work hint does (`workSpot`'s park → the snapshot's `workPark`, which
// App.neededPark.test.tsx follows into that log line). A row that opens no log line still tags the
// hunt: DetachedPanel.neededHunt.test.tsx holds those. The REAL NeededPanel, as that file mounts it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import type { NeedAlert } from './types'

const rows = vi.hoisted(() => ({
  phone: {
    call: 'K9ABC', entity: 'United States', band: '20m', zone: 4, tags: ['NewPark', 'Pota'],
    priority: 20, headline: 'POTA US-1000 (Test park)', mode: 'Phone', freqMhz: 14.285,
    park: { program: 'POTA', reference: 'US-1000' },
  },
  cw: {
    call: 'N0CWP', entity: 'United States', band: '20m', zone: 4, tags: ['NewPark', 'Pota'],
    priority: 20, headline: 'POTA US-2000 (Test park)', mode: 'CW', freqMhz: 14.062,
    park: { program: 'POTA', reference: 'US-2000' },
  },
  plain: {
    call: 'W9XYZ', entity: 'United States', band: '20m', zone: 4, tags: ['NewBand'],
    priority: 50, headline: 'New band slot', mode: 'Phone', freqMhz: 14.25, park: null,
  },
}))

vi.mock('./api', () => ({
  subscribeSnapshot: vi.fn(() => () => {}),
  selectPeer: vi.fn(() => Promise.resolve(null)),
  getBandPlan: vi.fn(() => Promise.resolve([])),
  getPropagation: vi.fn(() => Promise.resolve(null)),
  getNeedAlerts: vi.fn(() => Promise.resolve([rows.phone, rows.cw, rows.plain] as NeedAlert[])),
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
  localStorage.clear()
  vi.mocked(setHuntTarget).mockClear()
  vi.mocked(workSpot).mockClear()
})
afterEach(cleanup)

describe('the torn-off Needed board hands a Phone or CW park to the log line, not to a hunt', () => {
  it('Phone: the park rides the work, and nothing is hunted', async () => {
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(row('K9ABC')).toBeTruthy())
    fireEvent.click(row('K9ABC'))
    await waitFor(() =>
      expect(workSpot).toHaveBeenCalledWith('phone', 14.285, '20m', 'K9ABC', undefined, {
        program: 'POTA',
        reference: 'US-1000',
      }),
    )
    expect(setHuntTarget, 'a Needed click set a hunt').not.toHaveBeenCalled()
  })

  it('CW: the park rides the work, and nothing is hunted', async () => {
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(row('N0CWP')).toBeTruthy())
    fireEvent.click(row('N0CWP'))
    await waitFor(() =>
      expect(workSpot).toHaveBeenCalledWith('cw', 14.062, '20m', 'N0CWP', undefined, {
        program: 'POTA',
        reference: 'US-2000',
      }),
    )
    expect(setHuntTarget).not.toHaveBeenCalled()
  })

  // A switched-off cockpit is never opened, so no log line takes the park: the hunt still carries it.
  it('with the Phone cockpit switched off, a Phone park row still tags the hunt', async () => {
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { phone: false } }))
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(row('K9ABC')).toBeTruthy())
    fireEvent.click(row('K9ABC'))
    await waitFor(() => expect(setHuntTarget).toHaveBeenCalledWith('K9ABC', 'POTA', 'US-1000'))
    expect(workSpot).not.toHaveBeenCalled()
  })

  // CONTROL — a row that is not an activation works exactly as it did: no park, no hunt.
  it('a row that is not an activation carries no park', async () => {
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(row('W9XYZ')).toBeTruthy())
    fireEvent.click(row('W9XYZ'))
    await waitFor(() => expect(workSpot).toHaveBeenCalled())
    expect(vi.mocked(workSpot).mock.calls).toEqual([['phone', 14.25, '20m', 'W9XYZ', undefined]])
    expect(setHuntTarget).not.toHaveBeenCalled()
  })
})
