// @vitest-environment jsdom
//
// WORKING A PARK FROM THE CONNECT MAP TAGS THE HUNT — and says so when it cannot.
//
// The map's work path (`handleWorkMapSpot`) is the third and fourth place the hunt target is set,
// after the docked Needed board and the torn-off one. It had the same three defects those did: the
// rejection was swallowed by `.catch(() => {})`, the tag was armed before the work path's own
// bail-out, and it was never awaited.
//
// THIS MOUNTS THE REAL APP and stubs only ConnectView down to the seam under test — the
// DetachedPanel.workspot.test.tsx pattern. `App.workspot.test.ts` covers this same handler by
// READING App.tsx as text, which is why it passed all along with the swallow in place: a regex
// over the source cannot see a rejected promise. This file runs the handler instead.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen } from '@testing-library/react'
import type { AppSnapshot } from './types'

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null,
  recentDecodes: [], harqRescues: 0, logTick: 1,
} as unknown as AppSnapshot

// The seam: three spots the map can hand up. The stranded one carries no frequency of its own and
// sits on a band the (empty) plan has no channel for — exactly the shape `workTarget` answers null
// for, and the shape `handleQsy` then cannot move the rig to either.
vi.mock('./components/ConnectView', () => ({
  ConnectView: ({ onWorkSpot }: {
    onWorkSpot?: (t: {
      call: string; band: string; mode: string | null; freqMhz: number | null
      program?: string; reference?: string
    }) => void
  }) => (
    <>
      <button data-testid="work-park" onClick={() => onWorkSpot?.({
        call: 'K1ABC', band: '20m', mode: 'FT8', freqMhz: 14.074,
        program: 'POTA', reference: 'US-1234',
      })}>work park</button>
      <button data-testid="work-stranded" onClick={() => onWorkSpot?.({
        call: 'N0PRK', band: '60m', mode: 'FT8', freqMhz: null,
        program: 'POTA', reference: 'US-0003',
      })}>work stranded park</button>
      <button data-testid="work-plain" onClick={() => onWorkSpot?.({
        call: 'DX1XYZ', band: '20m', mode: 'FT8', freqMhz: 14.074,
      })}>work plain</button>
    </>
  ),
}))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    selectPeer: vi.fn(async () => snapshot),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => null),
    getBandPlan: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    getFdRuleset: vi.fn(async () => null),
    logOperators: vi.fn(async () => []),
    logActivations: vi.fn(async () => []),
    radioLaunchInfo: vi.fn(async () => ({ showPicker: false })),
    uiStateLoad: vi.fn(async () => ({})),
    uiStateSave: vi.fn(async () => ({})),
    getAllSpots: vi.fn(async () => []),
    getNeedAlerts: vi.fn(async () => []),
    getPropagation: vi.fn(async () => null),
    getFeedHealth: vi.fn(async () => null),
    getXrayNow: vi.fn(async () => null),
    getDxpedWindows: vi.fn(async () => []),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
    workSpot: vi.fn(async () => snapshot),
    setHuntTarget: vi.fn(async () => snapshot),
    openPanelWindow: vi.fn(async () => {}),
  }
})

const toasted = vi.hoisted(() => vi.fn())
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: toasted,
  // Faithful to the real helper in toast.ts — a rejection becomes an error toast and null, never a
  // swallowed promise. That IS what the failure case below is about.
  withErrorToast: vi.fn(async (action: () => Promise<unknown>, fallback: string) => {
    try {
      return await action()
    } catch {
      toasted(fallback, 'error')
      return null
    }
  }),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import { setHuntTarget, workSpot } from './api'

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.connect.autopop', 'off')
  vi.mocked(setHuntTarget).mockClear()
  vi.mocked(workSpot).mockClear()
  toasted.mockClear()
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

async function connect(): Promise<void> {
  window.location.hash = '#connect'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(screen.getByTestId('work-park')).toBeTruthy())
}

describe('working a park from the Connect map', () => {
  it('tags the hunt with the spot’s own call and park', async () => {
    await connect()
    fireEvent.click(screen.getByTestId('work-park'))
    await waitFor(() => expect(setHuntTarget).toHaveBeenCalledWith('K1ABC', 'POTA', 'US-1234'))
  })

  // CONTROL — a handler that tagged every worked spot would pass the test above too.
  it('a spot carrying no park identity tags nothing', async () => {
    await connect()
    fireEvent.click(screen.getByTestId('work-plain'))
    await waitFor(() => expect(workSpot).toHaveBeenCalled())
    expect(setHuntTarget).not.toHaveBeenCalled()
  })

  // A HUNT THAT COULD NOT BE SET HAS TO SAY SO. set_hunt_target rejects whenever the feed's
  // spelling of a reference will not normalize; swallowed, the operator watched the QSY land,
  // worked the park, and logged the contact with no reference at all.
  it('says so when the hunt cannot be set, and still works the station', async () => {
    await connect()
    vi.mocked(setHuntTarget).mockRejectedValueOnce(new Error('invalid POTA reference "K-1234"'))
    fireEvent.click(screen.getByTestId('work-park'))
    await waitFor(() =>
      expect(toasted).toHaveBeenCalledWith(expect.stringContaining('K1ABC'), 'error'),
    )
    await waitFor(() => expect(workSpot).toHaveBeenCalled())
  })

  // CONTROL for the one above — a handler that toasted on every work would pass it.
  it('a hunt that IS set says nothing', async () => {
    await connect()
    fireEvent.click(screen.getByTestId('work-park'))
    await waitFor(() => expect(setHuntTarget).toHaveBeenCalled())
    expect(toasted).not.toHaveBeenCalledWith(expect.anything(), 'error')
  })

  // A SPOT THE RIG CANNOT BE SENT TO MUST NOT ARM A PEND. The tag used to be set before the work
  // path's bail-out, so a spot off the band plan armed a four-hour hunt (HUNT_TTL_SECS) for a QSY
  // that never happened — waiting to stamp that park on the next contact with the callsign.
  it('a park spot with nowhere to QSY to tags nothing', async () => {
    await connect()
    fireEvent.click(screen.getByTestId('work-stranded'))
    // It DID take the bail-out: the rig has no channel for that band.
    await waitFor(() => expect(toasted).toHaveBeenCalledWith(expect.anything(), 'error', 3000))
    expect(setHuntTarget).not.toHaveBeenCalled()
    expect(workSpot).not.toHaveBeenCalled()
  })
})
