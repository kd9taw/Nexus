// @vitest-environment jsdom
//
// THE MAIN WINDOW'S LOG BUTTON, WIRED THROUGH THE REAL APP.
//
// With Auto-log and the popup both off, finished contacts a click, Call CQ or a mode change
// replaced wait for Log QSO. The snapshot's count reaches the Operate strip, Log is live while one
// waits even with no station on screen (after Call CQ), and a press answered "already waiting in
// the popup" says that, not "Logged". The popped-out window's half is DetachedPanel's own test.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent } from '@testing-library/react'
import { pushToast } from './toast'
import { t } from './i18n'
import type { AppSnapshot } from './types'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

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

const state = vi.hoisted(() => ({
  snap: null as unknown,
  confirm: null as unknown,
  confirmCalls: [] as unknown[][],
  snapshotReads: 0,
  logReply: null as unknown,
}))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    // A computer running its own station: the backend answers None, and the banner asks once. The
    // catch-all's `{}` would read as driving a remote station, asked every half second.
    clientStatus: vi.fn(async () => null),
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
    logCurrentQso: vi.fn(async () => state.logReply),
    // The log's answers never arrive: the cockpit shows the empty log's, as while it loads.
    askLog: vi.fn(() => new Promise(() => {})),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => ({ ...(defaultSettings as Record<string, unknown>) })),
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

// THE BUDGET (2026-10-04). The App this file mounts is real work, and it scales with the CPU a test gets: the
// slowest test takes 0.63 s on a quiet box, 2.5–4.1 s with a fifth of a CPU and 4.9–5.4 s with a tenth, against
// vitest's 5 s default. 15 s is over twice the tenth; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.wizardSeen', '1')
  state.snap = { ...base, logQsoWaiting: 2 } as AppSnapshot
  state.logReply = { logged: true, alreadyWaiting: true, call: '7P8RU', snapshot: state.snap }
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

async function logButton() {
  window.location.hash = '#operate'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.cockpit-qso .cq-log')).not.toBeNull())
  return document.querySelector<HTMLButtonElement>('.cockpit-qso .cq-log')!
}

describe("the main window's Log button", () => {
  it('shows how many contacts wait for Log QSO, and stays live with no station on screen', async () => {
    const log = await logButton()
    await waitFor(() => expect(document.querySelector('.cockpit-qso .cq-log-waiting')?.textContent).toBe('2'))
    expect(log.disabled).toBe(false)
  })

  it('says a contact is already waiting in the popup, not that it was logged', async () => {
    const log = await logButton()
    await waitFor(() => expect(log.disabled).toBe(false))
    fireEvent.click(log)
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('shell.toast.logWaiting', { call: '7P8RU' }), 'info', 4000),
    )
    expect(pushToast).not.toHaveBeenCalledWith(t('shell.toast.loggedCall', { call: '7P8RU' }), 'success', 2500)
  })
})
