// @vitest-environment jsdom
//
// The prompt-to-log popup shows one held contact at a time, and it must show the one it is
// answering for.
//
// Operator ruling, 2026-09-19: QUEUE them. A second completed contact used to replace the first
// while the popup kept the FIRST one's call in its own state (`useState(record.call)`, with no
// remount outside Remote) — so clicking Log filed the first station's call onto the second
// contact. The engine now holds a queue and refuses an answer that does not name the head
// (`expectedKey`); the popup remounts when that identity changes, and says how many more are
// behind this one.
//
// This mounts the REAL App, because the defect was in the wiring — LogConfirm's own tests pass
// it a record directly and would pass either way.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, screen, fireEvent, within } from '@testing-library/react'
import { pushToast } from './toast'
import { EN } from './i18n/en'
import type { AppSnapshot, LoggedQso } from './types'

const qso = (call: string): LoggedQso =>
  ({
    call, grid: 'FN03', band: '20m', freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12',
    whenUnix: Math.floor(Date.UTC(2026, 8, 20, 0, 58) / 1000), confirmed: false, awardConfirmed: false,
  }) as unknown as LoggedQso

const base = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal',
  radio: {
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null,
  recentDecodes: [], harqRescues: 0, logTick: 1,
} as unknown as AppSnapshot

/** The head of the queue, its identity, how many are behind it, and how many were logged
 *  unreviewed since the operator last answered this popup. */
const held = (call: string, key: string, waiting: number, autoLogged = 0): AppSnapshot =>
  ({
    ...base, pendingLog: qso(call), pendingQsoLogKey: key,
    pendingLogsWaiting: waiting, pendingLogsAutoLogged: autoLogged,
  }) as AppSnapshot

const state = vi.hoisted(() => ({
  snap: null as unknown,
  confirm: null as unknown,
  confirmCalls: [] as unknown[][],
  snapshotReads: 0,
}))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSnapshot: vi.fn(async () => {
      state.snapshotReads += 1
      return state.snap
    }),
    subscribeSnapshot: vi.fn(() => () => {}),
    confirmPendingLog: vi.fn(async (...args: unknown[]) => {
      state.confirmCalls.push(args)
      const next = state.confirm
      // A Tauri command returning `Err(String)` rejects with the bare string, not an Error.
      if (next instanceof Error || typeof next === 'string') throw next
      return next
    }),
    discardPendingLog: vi.fn(async () => state.confirm),
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
    setOperatingMode: vi.fn(async () => state.snap),
    setArea: vi.fn(async () => state.snap),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { spots: true } }))
  localStorage.setItem('nexus.needed.autopop', 'off')
  state.snap = held('VE3ABC', 'hold-1', 1)
  state.confirm = held('K1ABC', 'hold-2', 0)
  state.confirmCalls = []
  state.snapshotReads = 0
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const callBox = () => screen.getByLabelText('Call') as HTMLInputElement

async function openApp() {
  // The Spots view, as App.spotsWorked.test.tsx mounts it: the popup is App's own, above every
  // view, so this keeps the rest of the screen out of the way.
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.logconfirm')).not.toBeNull())
}

describe('the prompt-to-log popup and the queue behind it', () => {
  it('shows the contact that was promoted, not the one just answered', async () => {
    await openApp()
    expect(callBox().value).toBe('VE3ABC')
    // The operator corrects the call of the contact in front of them…
    fireEvent.change(callBox(), { target: { value: 'VE3ABD' } })
    fireEvent.click(screen.getByRole('button', { name: 'Log QSO' }))

    // …and the next held contact arrives with ITS own call, not the edited one.
    await waitFor(() => expect(callBox().value).toBe('K1ABC'))
    // The answer named the hold it was for, so the engine could refuse a stale one.
    expect(state.confirmCalls[0][1]).toBe('hold-1')
  })

  it('says how many more contacts are waiting behind this one', async () => {
    await openApp()
    expect(screen.getByText(/1 more/)).toBeTruthy()
  })

  it('shows nothing about a queue when this is the only held contact', async () => {
    state.snap = held('VE3ABC', 'hold-1', 0)
    await openApp()
    expect(screen.queryByText(/more contact/)).toBeNull()
  })

  it('says when contacts were logged without confirmation, and how many', async () => {
    // The queue has a cap (64). At the cap the OLDEST waiting contact is logged as it stands —
    // it goes into the log, uploads to QRZ/ClubLog/eQSL and joins the LoTW batch under the
    // operator's certificate. Someone coming back to a full queue has to be told that happened
    // here, on the popup they are looking at, not only in the Connections log.
    state.snap = held('VE3ABC', 'hold-1', 63, 2)
    await openApp()
    // Announced, not merely printed: the operator may be coming back to the screen. (The app
    // keeps a live region of its own, so scope this to the popup.)
    const popup = document.querySelector('.logconfirm') as HTMLElement
    const warned = within(popup).getByRole('alert').textContent ?? ''
    expect(warned).toMatch(/without your confirmation/i)
    expect(warned, 'the count of unreviewed contacts is missing').toMatch(/\b2\b/)
  })

  it('says nothing about unreviewed contacts when none were logged that way', async () => {
    await openApp() // the default hold has pendingLogsAutoLogged 0
    expect(document.querySelector('.logconfirm')?.textContent ?? '').not.toMatch(/without your confirmation/i)
  })

  it('re-reads the station and logs nothing when the answer is refused', async () => {
    // The engine's refusal is a CODE (a Tauri `Err(String)` arrives as a bare string).
    state.confirm = 'pendingLogMovedOn'
    await openApp()
    const readsBefore = state.snapshotReads
    fireEvent.click(screen.getByRole('button', { name: 'Log QSO' }))
    // A refusal means the popup was answering a contact that is no longer the head: re-read
    // the station rather than leave a stale popup, and log nothing.
    await waitFor(() => expect(state.snapshotReads).toBeGreaterThan(readsBefore))
    expect(document.querySelector('.logconfirm'), 'the popup vanished on a refusal').not.toBeNull()
    // …and the operator is told in words, from the catalog. The raw code must never reach the
    // screen: it would read the same in every language and say nothing to anyone.
    const said = vi.mocked(pushToast).mock.calls.map((c) => String(c[0]))
    expect(said).toContain(EN['shell.log.movedOn'])
    expect(said.join(' ')).not.toMatch(/pendingLogMovedOn/)
  })

  it('still shows an unknown failure as itself, rather than dressing it up', async () => {
    state.confirm = new Error('diskFull')
    await openApp()
    fireEvent.click(screen.getByRole('button', { name: 'Log QSO' }))
    await waitFor(() => expect(vi.mocked(pushToast).mock.calls.length).toBeGreaterThan(0))
    const said = vi.mocked(pushToast).mock.calls.map((c) => String(c[0])).join(' ')
    expect(said).toContain(EN['shell.log.failed'])
    expect(said, 'an unknown failure was reported as the queue refusal').toContain('diskFull')
  })
})
