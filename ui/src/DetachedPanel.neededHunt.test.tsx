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
  // A park row the rig cannot be sent to: no frequency of its own, Digital (so no mode default)
  // and — with the empty band plan this file mounts — no channel either. `workTarget` is null for
  // exactly this shape, and `qsyBand` reads the same missing channel and moves nothing at all.
  stranded: {
    call: 'N0PRK',
    entity: 'United States',
    band: '60m',
    zone: 4,
    tags: ['NewPark', 'Pota'],
    priority: 20,
    headline: 'POTA US-0003',
    mode: 'Digital',
    freqMhz: null,
    park: { program: 'POTA', reference: 'US-0003' },
  },
}))

vi.mock('./api', () => ({
  subscribeSnapshot: vi.fn(() => () => {}),
  selectPeer: vi.fn(() => Promise.resolve(null)),
  getBandPlan: vi.fn(() => Promise.resolve([])),
  getPropagation: vi.fn(() => Promise.resolve(null)),
  getNeedAlerts: vi.fn(() => Promise.resolve([rows.park, rows.plain, rows.stranded] as NeedAlert[])),
  getSettings: vi.fn(() => Promise.resolve(null)),
  workSpot: vi.fn(() => Promise.resolve(null)),
  setFrequency: vi.fn(() => Promise.resolve(null)),
  setHuntTarget: vi.fn(() => Promise.resolve(null)),
  pointRotator: vi.fn(() => Promise.resolve(null)),
  readRotator: vi.fn(() => Promise.resolve(null)),
  openQrzPage: vi.fn(() => Promise.resolve(null)),
}))

import { DetachedPanel } from './DetachedPanel'
import { setHuntTarget, workSpot, setFrequency } from './api'
import { t } from './i18n'

const row = (call: string) =>
  screen.getAllByRole('row').find((r) => r.getAttribute('aria-label')?.includes(call)) as HTMLElement

beforeEach(() => {
  vi.mocked(setHuntTarget).mockClear()
  vi.mocked(workSpot).mockClear()
  vi.mocked(setFrequency).mockClear()
})
afterEach(cleanup)

describe('the torn-off Needed board tags a park when a park row is worked', () => {
  // The hunt is AWAITED before the work now, so both land a tick later than the click.
  it('sets the hunt to the row’s own call and park, and still works the spot', async () => {
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(row('K1ABC')).toBeTruthy())
    fireEvent.click(row('K1ABC'))
    await waitFor(() => expect(setHuntTarget).toHaveBeenCalledWith('K1ABC', 'POTA', 'US-0001'))
    await waitFor(() =>
      expect(workSpot).toHaveBeenCalledWith('digital', 14.074, '20m', 'K1ABC', undefined),
    )
  })

  // CONTROL — without it, a version that tags every Work would pass the test above.
  it('a need that is not an activation tags nothing', async () => {
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(row('W9XYZ')).toBeTruthy())
    fireEvent.click(row('W9XYZ'))
    await waitFor(() =>
      expect(workSpot).toHaveBeenCalledWith('digital', 14.076, '20m', 'W9XYZ', undefined),
    )
    expect(setHuntTarget).not.toHaveBeenCalled()
  })

  // A HUNT THAT COULD NOT BE SET HAS TO SAY SO — in the pop-out too, which has its own toast host
  // (DetachedShell). `set_hunt_target` rejects whenever the feed's spelling of a reference will
  // not normalize; swallowed, the operator watched the QSY land, worked the park, and logged the
  // contact with no reference at all.
  it('shows the failure when the hunt cannot be set, and still works the station', async () => {
    vi.mocked(setHuntTarget).mockRejectedValueOnce(new Error('invalid POTA reference "K-1234"'))
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(row('K1ABC')).toBeTruthy())
    fireEvent.click(row('K1ABC'))
    await screen.findByText(
      new RegExp(t('ota.hunt.setFailed', { call: 'K1ABC' }).replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
    )
    await waitFor(() => expect(workSpot).toHaveBeenCalled())
  })

  // A ROW THE RIG CANNOT BE SENT TO MUST NOT ARM A PEND. The tag used to be set before the
  // work-target bail-out, so a row off the band plan armed a four-hour hunt (HUNT_TTL_SECS) for a
  // QSY that never happened — waiting to stamp that park on the next contact with the call.
  it('a park row with nowhere to QSY to tags nothing', async () => {
    render(<DetachedPanel panel="needed" />)
    await waitFor(() => expect(row('N0PRK')).toBeTruthy())
    fireEvent.click(row('N0PRK'))
    // CONTROL, same run: the click path itself works — a row that CAN be worked still tags.
    fireEvent.click(row('K1ABC'))
    await waitFor(() => expect(setHuntTarget).toHaveBeenCalledWith('K1ABC', 'POTA', 'US-0001'))
    expect(setHuntTarget).not.toHaveBeenCalledWith('N0PRK', 'POTA', 'US-0003')
    expect(workSpot).not.toHaveBeenCalledWith(
      expect.anything(), expect.anything(), '60m', 'N0PRK', expect.anything(),
    )
    expect(setFrequency).not.toHaveBeenCalled() // …and the stranded row moved nothing
  })
})
