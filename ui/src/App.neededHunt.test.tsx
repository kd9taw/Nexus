// @vitest-environment jsdom
//
// WORK FROM THE NEEDED BOARD TAGS THE PARK — in the docked board, in the real App.
//
// The torn-off board has its own handler and its own test (DetachedPanel.neededHunt.test.tsx);
// this is the one an operator uses. Mounts the REAL App (the App.js8workspace.test.tsx pattern),
// because what is under test is a click on a row reaching App's work handler — a source grep
// cannot see that the board is even wired to it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen } from '@testing-library/react'
import type { AppSnapshot, NeedAlert } from './types'

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

const alerts = vi.hoisted(() => [
  {
    call: 'K1ABC', entity: 'United States', band: '20m', zone: 5, tags: ['NewPark', 'Pota'],
    priority: 20, headline: 'POTA US-0001 (Test park)', mode: 'Digital', freqMhz: 14.074,
    park: { program: 'POTA', reference: 'US-0001' },
  },
  {
    call: 'W9XYZ', entity: 'United States', band: '20m', zone: 4, tags: ['NewBand'],
    priority: 50, headline: 'New band slot', mode: 'Digital', freqMhz: 14.076, park: null,
  },
  // A park row the rig cannot be sent to: no frequency of its own, Digital (so no mode
  // default) and — with the empty band plan these tests mount — no channel either. `workTarget`
  // is null for exactly this shape, and `handleQsy` reads the same missing channel.
  {
    call: 'N0PRK', entity: 'United States', band: '60m', zone: 4, tags: ['NewPark', 'Pota'],
    priority: 20, headline: 'POTA US-0003', mode: 'Digital', freqMhz: null,
    park: { program: 'POTA', reference: 'US-0003' },
  },
])

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
    getNeedAlerts: vi.fn(async () => alerts as unknown as NeedAlert[]),
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
    // Clicking any row selects the station first, and the real command answers with a SNAPSHOT.
    // The blanket `async () => ({})` above answers with an object that has no `link`, which App
    // stores and then reads `link.tier` off — a crash the shorter tests here only escaped by
    // unmounting before the promise landed.
    selectPeer: vi.fn(async () => snapshot),
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
  // Faithful to the real helper in toast.ts — a rejection becomes an error toast and a null
  // result, never a swallowed promise. That IS what the failure cases below are about, so a
  // pass-through stand-in would make them untestable.
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

const row = (call: string) =>
  screen.getAllByRole('row').find((r) => r.getAttribute('aria-label')?.includes(call)) as HTMLElement

beforeEach(() => {
  localStorage.clear()
  // The Needed board is a core section; the auto-pop would move it into its own window.
  localStorage.setItem('nexus.needed.autopop', 'off')
  vi.mocked(setHuntTarget).mockClear()
  vi.mocked(workSpot).mockClear()
  toasted.mockClear()
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

async function board(): Promise<void> {
  window.location.hash = '#needed'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(row('K1ABC')).toBeTruthy())
}

describe('the Needed board tags a park when a park row is worked', () => {
  it('sets the hunt to that row’s call and park before it works the spot', async () => {
    await board()
    fireEvent.click(row('K1ABC'))
    await waitFor(() => expect(setHuntTarget).toHaveBeenCalledWith('K1ABC', 'POTA', 'US-0001'))
    await waitFor(() => expect(workSpot).toHaveBeenCalled())
  })

  // CONTROL — a Work that tagged every row would pass the test above too.
  it('a need that is not an activation tags nothing', async () => {
    await board()
    fireEvent.click(row('W9XYZ'))
    await waitFor(() => expect(workSpot).toHaveBeenCalled())
    expect(setHuntTarget).not.toHaveBeenCalled()
  })

  // A HUNT THAT COULD NOT BE SET HAS TO SAY SO. `set_hunt_target` rejects whenever the feed's
  // spelling of a reference will not normalize (station.rs). Swallowed, the operator watched the
  // QSY land, worked the park, and logged the contact with no reference at all — the one outcome
  // that costs the hunt, and the only clue was its absence hours later.
  it('says so when the hunt cannot be set, and still works the station', async () => {
    await board()
    vi.mocked(setHuntTarget).mockRejectedValueOnce(new Error('invalid POTA reference "K-1234"'))
    fireEvent.click(row('K1ABC'))
    await waitFor(() =>
      expect(toasted).toHaveBeenCalledWith(expect.stringContaining('K1ABC'), 'error'),
    )
    await waitFor(() => expect(workSpot).toHaveBeenCalled())
  })

  // CONTROL for the one above — a handler that toasted on every Work would pass it.
  it('a hunt that IS set says nothing', async () => {
    await board()
    fireEvent.click(row('K1ABC'))
    await waitFor(() => expect(setHuntTarget).toHaveBeenCalled())
    expect(toasted).not.toHaveBeenCalledWith(expect.anything(), 'error')
  })

  // A ROW THE RIG CANNOT BE SENT TO MUST NOT ARM A PEND. The tag used to be set before the
  // work-target bail-out, so a row off the band plan armed a four-hour hunt (HUNT_TTL_SECS) for
  // a QSY that never happened — waiting to stamp that park on the next contact with the call,
  // whichever band it was made on.
  it('a park row with nowhere to QSY to tags nothing', async () => {
    await board()
    fireEvent.click(row('N0PRK'))
    // The row DID take the bail-out — the rig has no channel for its band.
    await waitFor(() => expect(toasted).toHaveBeenCalledWith(expect.anything(), 'error', 3000))
    expect(setHuntTarget).not.toHaveBeenCalled()
    expect(workSpot).not.toHaveBeenCalled()
  })
})
