// @vitest-environment jsdom
//
// The logbook database could not be opened at launch (C9). The station then keeps the session's
// log in log.adi, as 1.13 did, and used to say so only in the diagnostic log. Maintainer,
// 2026-09-23: "Show both on screen." So the snapshot carries the problem, and the shell raises
// ONE sticky toast from it: visible, dismissable, never in the way, and never repeated in the
// same session however many snapshots follow.
//
// This mounts the REAL App and the real toast column, because what is proved is the wiring from
// the snapshot to the screen — the words are the catalog's, and are read from it here.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, act, within, waitFor, fireEvent } from '@testing-library/react'
import { dismissToast, subscribeToasts, type Toast } from './toast'
import { t } from './i18n'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
import type { AppSnapshot, Settings } from './types'

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

/** What the station's diagnostic log said, as `OpenError`'s Display words it. */
const REASON = 'the logbook database could not be opened: logbook database: database disk image is malformed'

const state = vi.hoisted(() => ({
  snap: null as unknown,
  /** The snapshot subscription App made — how a test delivers the next snapshot. */
  push: null as null | ((s: unknown) => void),
}))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSnapshot: vi.fn(async () => state.snap),
    subscribeSnapshot: vi.fn((next: (s: unknown) => void) => {
      state.push = next
      return () => {}
    }),
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
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'

let toasts: Toast[] = []
let unsubscribe: () => void = () => {}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { spots: true } }))
  localStorage.setItem('nexus.needed.autopop', 'off')
  state.snap = base
  state.push = null
  unsubscribe = subscribeToasts((now) => {
    toasts = now
  })
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  // The toast bus is module state: clear it so no test inherits another's notice.
  for (const toast of toasts) dismissToast(toast.id)
  unsubscribe()
  vi.clearAllMocks()
})

/** Mount the real App on the Spots view (App.spotsWorked.test.tsx's mount) and let the first
 *  snapshot land. */
async function openApp() {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(state.push).not.toBeNull())
  await waitFor(() => expect(document.querySelector('.app')).not.toBeNull())
}

/** The toasts on the bus that are this notice, by the words only it says. */
const notices = () => toasts.filter((x) => x.message.includes('log.adi'))

describe('the logbook database could not be opened (C9)', () => {
  it('says so on screen, with the reason, that nothing is lost, and where to look', async () => {
    state.snap = { ...base, logStoreProblem: { networkFolder: false, reason: REASON } }
    await openApp()
    const notice = await screen.findByText(t('shell.logStore.failed', { reason: REASON }))
    const said = notice.textContent ?? ''
    expect(said, 'the reason the station gave').toContain(REASON)
    expect(said).toMatch(/Nothing is lost/)
    expect(said, 'this session keeps the log where 1.13 did').toMatch(/log\.adi/)
    expect(said, 'where to look').toMatch(/Diagnostic log/)
    expect(said, 'not the network-folder remedy').not.toMatch(/Data & log folder/)
    // Sticky and dismissable: it waits for the operator, and the × takes it away.
    const toast = notice.closest('.ui-toast') as HTMLElement
    expect(notices()).toHaveLength(1)
    act(() => {
      fireEvent.click(within(toast).getByRole('button', { name: t('toast.dismiss') }))
    })
    expect(screen.queryByText(t('shell.logStore.failed', { reason: REASON }))).toBeNull()
  })

  it('tells a network data folder to move to a drive inside this computer, in Settings', async () => {
    state.snap = {
      ...base,
      logStoreProblem: {
        networkFolder: true,
        reason: 'the data folder is on a network drive (nfs4), so the logbook stays in log.adi for this session',
      },
    }
    await openApp()
    const notice = await screen.findByText(t('shell.logStore.network'))
    const said = notice.textContent ?? ''
    expect(said).toMatch(/network drive/)
    expect(said).toMatch(/Nothing is lost/)
    expect(said).toMatch(/log\.adi/)
    expect(said, 'the remedy, where it is').toMatch(/Settings ▸ Config ▸ Data & log folder/)
  })

  it('says it once a session: more snapshots never repeat it, and dismissed it stays gone', async () => {
    const problem = { networkFolder: false, reason: REASON }
    state.snap = { ...base, logStoreProblem: problem }
    await openApp()
    await screen.findByText(t('shell.logStore.failed', { reason: REASON }))
    // Every poll delivers a NEW snapshot object carrying the same problem.
    for (let i = 0; i < 3; i++) act(() => state.push?.({ ...base, logStoreProblem: { ...problem } }))
    expect(notices(), 'one notice, however many snapshots carry the problem').toHaveLength(1)

    act(() => dismissToast(notices()[0].id))
    act(() => state.push?.({ ...base, logStoreProblem: { ...problem } }))
    expect(notices(), 'dismissed means gone for the session').toHaveLength(0)
  })

  it('stays off the Remote, whose Settings hide the remedy', async () => {
    const snapshot = { ...base, logStoreProblem: { networkFolder: true, reason: REASON } } as AppSnapshot
    state.snap = snapshot
    const page = render(
      <App remote={{ snapshot, settings: settingsFixture as unknown as Settings, bandPlan: [], status: <div>Observer</div> }} />,
    )
    await screen.findByText('Observer')
    expect(document.querySelector('.app.remote-workspace'), 'premise: the Remote page mounted').not.toBeNull()
    expect(notices(), 'a Remote page raises no notice').toHaveLength(0)
    page.unmount()
    // The control: the same snapshot on the station's own screen does raise it.
    await openApp()
    await screen.findByText(t('shell.logStore.network'))
    expect(notices()).toHaveLength(1)
  })

  it('shows nothing while the database owns the log, or from a station older than it', async () => {
    // `null`: the database owns the log. Absent: a station from before the field existed.
    state.snap = { ...base, logStoreProblem: null }
    await openApp()
    act(() => state.push?.({ ...base }))
    expect(notices()).toHaveLength(0)
    // The control: the same mounted App, handed a problem, does say it — so the silence above
    // was the absence of a problem, not an App that never looked.
    act(() => state.push?.({ ...base, logStoreProblem: { networkFolder: false, reason: REASON } }))
    await screen.findByText(t('shell.logStore.failed', { reason: REASON }))
    expect(notices()).toHaveLength(1)
  })
})
