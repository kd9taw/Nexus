// @vitest-environment jsdom
//
// A file the station could not read at launch — a torn journal, or one a newer Nexus wrote — is
// KEPT rather than saved over (tempo_core::keep_aside), and the snapshot names it. The shell
// raises ONE sticky toast per file from it: what the file held, where it is now, and that
// nothing was deleted; never repeated in the session however many snapshots follow, and never
// on the Remote.
//
// This mounts the REAL App and the real toast column, because what is proved is the wiring from
// the snapshot to the screen — the words are the catalog's, and are read from it here.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, act, within, waitFor, fireEvent } from '@testing-library/react'
import { dismissToast, subscribeToasts, type Toast } from './toast'
import { t } from './i18n'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
import type { AppSnapshot, KeptFile, Settings } from './types'

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

const ASIDE = 'C:\\Users\\op\\AppData\\Roaming\\tempo\\pending_qso.unreadable-20260930-142233.json'
const MOVED: KeptFile = { store: 'pendingQso', path: ASIDE, keptInPlace: false }

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

/** The toasts on the bus that are about a kept file, by the words only they say. */
const notices = () => toasts.filter((x) => x.message.includes('Nothing was deleted'))

describe('a file the station could not read, and kept', () => {
  it('says what it held, where it is now, and that nothing was deleted', async () => {
    state.snap = { ...base, keptFiles: [MOVED] }
    await openApp()
    const notice = await screen.findByText(t('shell.keptFile.pendingQso', { path: ASIDE }))
    const said = notice.textContent ?? ''
    expect(said, 'what the file held').toMatch(/Log this QSO\?/)
    expect(said, 'where it is now').toContain(ASIDE)
    expect(said).toMatch(/Nothing was deleted/)
    // Sticky and dismissable, as the logbook-database notice is.
    const toast = notice.closest('.ui-toast') as HTMLElement
    expect(notices()).toHaveLength(1)
    act(() => {
      fireEvent.click(within(toast).getByRole('button', { name: t('toast.dismiss') }))
    })
    expect(screen.queryByText(t('shell.keptFile.pendingQso', { path: ASIDE }))).toBeNull()
  })

  it('says a file it could not move is where it was, and that nothing writes over it', async () => {
    const stuck: KeptFile = { store: 'pendingQso', path: 'C:\\tempo\\pending_qso.json', keptInPlace: true }
    state.snap = { ...base, keptFiles: [stuck] }
    await openApp()
    const notice = await screen.findByText(t('shell.keptFile.keptInPlace', { path: stuck.path }))
    expect(notice.textContent ?? '').toMatch(/will not write over it/)
  })

  it('says each file once a session, and a second file gets its own', async () => {
    state.snap = { ...base, keptFiles: [MOVED] }
    await openApp()
    await screen.findByText(t('shell.keptFile.pendingQso', { path: ASIDE }))
    // Every poll delivers a NEW snapshot object carrying the same list.
    for (let i = 0; i < 3; i++) act(() => state.push?.({ ...base, keptFiles: [{ ...MOVED }] }))
    expect(notices(), 'one notice, however many snapshots carry the file').toHaveLength(1)
    const later: KeptFile = { store: 'somethingNewer', path: 'C:\\tempo\\x.unreadable-20260930-150000.json', keptInPlace: false }
    act(() => state.push?.({ ...base, keptFiles: [MOVED, later] }))
    await screen.findByText(t('shell.keptFile.other', { path: later.path }))
    expect(notices(), 'the second file, and only it, is new').toHaveLength(2)
  })

  it('stays off the Remote', async () => {
    const snapshot = { ...base, keptFiles: [MOVED] } as AppSnapshot
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
    await screen.findByText(t('shell.keptFile.pendingQso', { path: ASIDE }))
    expect(notices()).toHaveLength(1)
  })

  it('shows nothing on a healthy launch, or from a station older than the rule', async () => {
    state.snap = { ...base, keptFiles: [] }
    await openApp()
    act(() => state.push?.({ ...base }))
    expect(notices()).toHaveLength(0)
    // The control: the same mounted App, handed a kept file, does say it — so the silence above
    // was the absence of one, not an App that never looked.
    act(() => state.push?.({ ...base, keptFiles: [MOVED] }))
    await screen.findByText(t('shell.keptFile.pendingQso', { path: ASIDE }))
    expect(notices()).toHaveLength(1)
  })
})
