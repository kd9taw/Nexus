// @vitest-environment jsdom
//
// Task 1 of the POTA map plan: the pop-out's generic work-spot path (DetachedPanel's own
// `onWorkSpot`) is the PRIMARY surface for this feature — the POTA map ships as a pop-out
// window (a later task), and the 'connect' branch already wires `onWorkSpot={onWorkSpot}`
// straight into ConnectView/MapView. Tagging only App.tsx's handler would ship a
// tune-and-tag that never tags where a torn-off POTA map actually operates.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { workSpot, setFrequency, setHuntTarget } from './api'

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
  it('tags the hunt target when a worked spot carries a POTA reference', async () => {
    render(<DetachedPanel panel="connect" />)
    await act(async () => {
      await Promise.resolve()
    })
    fireEvent.click(screen.getByTestId('work-park'))
    expect(mockedSetHuntTarget).toHaveBeenCalledWith('K1ABC', 'POTA', 'US-1234')
    expect(mockedWorkSpot).toHaveBeenCalledWith('digital', 14.074, '20m', 'K1ABC')
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
    expect(mockedSetHuntTarget).not.toHaveBeenCalled()
    expect(mockedWorkSpot).toHaveBeenCalledWith('digital', 14.074, '20m', 'DX1XYZ')
  })
})
