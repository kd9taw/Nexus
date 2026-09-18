// @vitest-environment jsdom
//
// Task 1 of the POTA map plan: the pop-out's generic work-spot path (DetachedPanel's own
// `onWorkSpot`) is the PRIMARY surface for this feature — the POTA map ships as a pop-out
// window (a later task), and the 'connect' branch already wires `onWorkSpot={onWorkSpot}`
// straight into ConnectView/MapView. Tagging only App.tsx's handler would ship a
// tune-and-tag that never tags where a torn-off POTA map actually operates.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup, waitFor } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { workSpot, setFrequency, setHuntTarget } from './api'
import { t } from './i18n'

// Stub ConnectView down to the one seam under test — the real component renders the
// whole map/canvas stack, which is irrelevant here (that's MapView's own domain).
vi.mock('./components/ConnectView', () => ({
  ConnectView: ({
    onWorkSpot,
  }: {
    onWorkSpot?: (t: {
      call: string
      band: string
      mode: string | null
      freqMhz: number | null
      program?: string
      reference?: string
    }) => void
  }) => (
    <>
      <button
        data-testid="work-park"
        onClick={() =>
          onWorkSpot?.({
            call: 'K1ABC',
            band: '20m',
            mode: 'FT8',
            freqMhz: 14.074,
            program: 'POTA',
            reference: 'US-1234',
          })
        }
      >
        work park
      </button>
      <button
        data-testid="work-plain"
        onClick={() => onWorkSpot?.({ call: 'DX1XYZ', band: '20m', mode: 'FT8', freqMhz: 14.074 })}
      >
        work plain
      </button>
      {/* No frequency of its own, and no channel for its band in the (empty) plan this file
          mounts — so `qsyBand` below can move nothing at all. */}
      <button
        data-testid="work-stranded"
        onClick={() =>
          onWorkSpot?.({
            call: 'N0PRK',
            band: '60m',
            mode: 'FT8',
            freqMhz: null,
            program: 'POTA',
            reference: 'US-0003',
          })
        }
      >
        work stranded park
      </button>
    </>
  ),
}))

vi.mock('./api', () => ({
  subscribeSnapshot: vi.fn(() => () => {}),
  selectPeer: vi.fn(() => Promise.resolve(null)),
  getBandPlan: vi.fn(() => Promise.resolve([])),
  getPropagation: vi.fn(() => Promise.resolve(null)),
  getNeedAlerts: vi.fn(() => Promise.resolve([])),
  getSettings: vi.fn(() => Promise.resolve(null)),
  pointRotatorAtCall: vi.fn(() => Promise.resolve(null)),
  workSpot: vi.fn(() => Promise.resolve(null)),
  setFrequency: vi.fn(() => Promise.resolve(null)),
  setHuntTarget: vi.fn(() => Promise.resolve(null)),
}))

const mockedWorkSpot = vi.mocked(workSpot)
const mockedSetFrequency = vi.mocked(setFrequency)
const mockedSetHuntTarget = vi.mocked(setHuntTarget)

beforeEach(() => {
  mockedWorkSpot.mockClear()
  mockedSetFrequency.mockClear()
  mockedSetHuntTarget.mockClear()
})
afterEach(() => cleanup())

describe('DetachedPanel work-spot path tags the POTA hunt target', () => {
  // The hunt is AWAITED before the work now, so both land a tick after the click.
  it('tags the hunt target when a worked spot carries a POTA reference', async () => {
    render(<DetachedPanel panel="connect" />)
    await act(async () => {
      await Promise.resolve()
    })
    fireEvent.click(screen.getByTestId('work-park'))
    await waitFor(() => expect(mockedSetHuntTarget).toHaveBeenCalledWith('K1ABC', 'POTA', 'US-1234'))
    await waitFor(() =>
      expect(mockedWorkSpot).toHaveBeenCalledWith('digital', 14.074, '20m', 'K1ABC'),
    )
  })

  // POSITIVE CONTROL — proves the gate actually gates: without this, a version of the fix
  // that always calls setHuntTarget (ignoring program/reference) would also pass the test
  // above.
  it('a plain spot with no park identity does not tag the hunt', async () => {
    render(<DetachedPanel panel="connect" />)
    await act(async () => {
      await Promise.resolve()
    })
    fireEvent.click(screen.getByTestId('work-plain'))
    await waitFor(() =>
      expect(mockedWorkSpot).toHaveBeenCalledWith('digital', 14.074, '20m', 'DX1XYZ'),
    )
    expect(mockedSetHuntTarget).not.toHaveBeenCalled()
  })

  // A HUNT THAT COULD NOT BE SET HAS TO SAY SO — in the pop-out too, which has its own toast host
  // (DetachedShell). set_hunt_target rejects whenever the feed's spelling of a reference will not
  // normalize; swallowed, the operator watched the QSY land, worked the park, and logged the
  // contact with no reference at all.
  it('shows the failure when the hunt cannot be set, and still works the spot', async () => {
    mockedSetHuntTarget.mockRejectedValueOnce(new Error('invalid POTA reference "K-1234"'))
    render(<DetachedPanel panel="connect" />)
    await act(async () => {
      await Promise.resolve()
    })
    fireEvent.click(screen.getByTestId('work-park'))
    await screen.findByText(
      new RegExp(t('ota.hunt.setFailed', { call: 'K1ABC' }).replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
    )
    await waitFor(() => expect(mockedWorkSpot).toHaveBeenCalled())
  })

  // A SPOT THE RIG CANNOT BE SENT TO MUST NOT ARM A PEND. The tag used to run before anything
  // moved, so a spot with no frequency and no channel for its band armed a four-hour hunt
  // (HUNT_TTL_SECS) for a QSY that never happened — waiting to stamp that park on the next
  // contact with the callsign, whatever band it was made on.
  it('a park spot with nowhere to QSY to tags nothing', async () => {
    render(<DetachedPanel panel="connect" />)
    await act(async () => {
      await Promise.resolve()
    })
    fireEvent.click(screen.getByTestId('work-stranded'))
    // CONTROL, same run: the click path itself works — a spot that CAN be worked still tags.
    fireEvent.click(screen.getByTestId('work-park'))
    await waitFor(() => expect(mockedSetHuntTarget).toHaveBeenCalledWith('K1ABC', 'POTA', 'US-1234'))
    expect(mockedSetHuntTarget).not.toHaveBeenCalledWith('N0PRK', 'POTA', 'US-0003')
    expect(mockedSetFrequency).not.toHaveBeenCalled() // …and the stranded spot moved nothing
  })
})
