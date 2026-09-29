// @vitest-environment jsdom
//
// ONE UPDATE PROMPT, NOT TWO (operator, 2026-09-29): "since you implemented the option to have the
// program automatically download and install, its still showing the old popup that takes you to
// the GitHub download page. shouldn't we kill that so it just executes the auto upgrade".
//
// Two checks ran at every launch, each with its own prompt: the old notice (a toast whose Download
// button opens the release page) and the signed self-updater (a banner whose Install button
// replaces the app). Where the self-updater works, the operator saw both. The notice stays where
// it cannot: the .deb packages (apt owns those files) and any page with no updater at all.
//
// This mounts the REAL App, because the second prompt was App's own launch effect: no test of
// either module alone could see the two of them on one screen.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, act, waitFor } from '@testing-library/react'
import { dismissToast, subscribeToasts, type Toast } from './toast'
import { t } from './i18n'
import type { AppSnapshot, UpdateInfo } from './types'

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

const PAGE = 'https://github.com/kd9taw/nexus/releases/latest'
/** What the notice's feed says: a newer build than this one. */
const INFO: UpdateInfo = { current: '1.15.0', latest: '9.9.9', updateAvailable: true, downloadUrl: PAGE }

const state = vi.hoisted(() => ({
  snap: null as unknown,
  push: null as null | ((s: unknown) => void),
  /** What `update_route` says about this install's package. */
  selfUpdate: true,
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
    appVersion: vi.fn(async () => '1.15.0'),
    openPanelWindow: vi.fn(async () => {}),
    // The two update checks.
    checkForUpdate: vi.fn(async () => INFO),
    updateRoute: vi.fn(async () => ({ selfUpdate: state.selfUpdate, downloadPage: PAGE })),
    updateInstallBlock: vi.fn(async () => null),
  }
})
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import { checkForUpdate, updateRoute } from './api'

/** The updater plugin's JS surface, as `withGlobalTauri` injects it into a desktop window. */
function installUpdater(check: () => Promise<unknown>): { calls: number } {
  const seen = { calls: 0 }
  ;(window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    updater: {
      check: () => {
        seen.calls++
        return check()
      },
    },
  }
  return seen
}
const offering = (version: string, download: () => Promise<void> = async () => {}) => () =>
  Promise.resolve({ available: true, version, download, install: async () => {} })

let toasts: Toast[] = []
/** Every toast pushed since the test began, including any taken down again: a notice that shows
 *  and is then retired was still a second prompt on the screen. */
let ever: Toast[] = []
let unsubscribe: () => void = () => {}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { spots: true } }))
  localStorage.setItem('nexus.needed.autopop', 'off')
  state.snap = base
  state.push = null
  state.selfUpdate = true
  ever = []
  unsubscribe = subscribeToasts((now) => {
    toasts = now
    for (const x of now) if (!ever.some((e) => e.id === x.id)) ever.push(x)
  })
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  for (const toast of toasts) dismissToast(toast.id)
  unsubscribe()
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__
  vi.clearAllMocks()
})

async function openApp() {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(state.push).not.toBeNull())
  await waitFor(() => expect(document.querySelector('.app')).not.toBeNull())
}

/** Let every check that is going to answer, answer. */
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30))
  })
}

/** The old prompt: the "update available" toast, found by its own words. */
const isNotice = (x: Toast) => x.message === t('update.available', { latest: INFO.latest!, current: INFO.current })
const notices = () => toasts.filter(isNotice)
const everNotices = () => ever.filter(isNotice)
const banner = () => document.querySelector('.update-banner')

describe('where Nexus updates itself (Windows, macOS, the AppImage)', () => {
  it('the install prompt is the only prompt', async () => {
    const updater = installUpdater(offering('9.9.9'))
    await openApp()
    await screen.findByText(t('update.ready', { version: '9.9.9' }))
    await settle()
    expect(updater.calls, 'premise: the self-updater checked').toBe(1)
    expect(everNotices(), 'no second prompt pointing at the download page, not even for a moment').toHaveLength(0)
  })

  it('an up-to-date check leaves the old notice silent too, even when its feed disagrees', async () => {
    const updater = installUpdater(() => Promise.resolve(null))
    await openApp()
    await waitFor(() => expect(updater.calls).toBe(1))
    await settle()
    expect(banner(), 'up to date: nothing to install').toBeNull()
    expect(everNotices(), 'the self-updater answered; the notice does not second-guess it').toHaveLength(0)
    // The notice's check still runs, as before: it is the launch's line in the diagnostic log.
    expect(checkForUpdate).toHaveBeenCalledTimes(1)
  })
})

describe('where it cannot, the notice stays exactly as it was', () => {
  it('a .deb keeps the notice, and gets no install prompt it could never use', async () => {
    // A .deb on a PC: the plugin IS in the build, and the manifest's bare `linux-x86_64` key
    // (the AppImage) matches it, so the check would offer an update whose install always fails.
    state.selfUpdate = false
    const updater = installUpdater(offering('9.9.9'))
    await openApp()
    await screen.findByText(t('update.available', { latest: '9.9.9', current: '1.15.0' }))
    await settle()
    expect(notices(), 'the notice, once').toHaveLength(1)
    expect(updater.calls, 'the updater never checked, so it never downloaded an AppImage').toBe(0)
    expect(banner(), 'no Install button that can only fail').toBeNull()
  })

  it('no updater at all (the Remote page, a plain browser): the notice, and nothing asked of a missing backend', async () => {
    await openApp()
    await screen.findByText(t('update.available', { latest: '9.9.9', current: '1.15.0' }))
    expect(notices()).toHaveLength(1)
    expect(updateRoute, 'the route is only asked where an updater exists').not.toHaveBeenCalled()
  })

  it('a self-update check that fails leaves the notice to speak', async () => {
    installUpdater(() => Promise.reject(new Error('Could not fetch a valid release JSON from the remote')))
    await openApp()
    await screen.findByText(t('update.available', { latest: '9.9.9', current: '1.15.0' }))
    expect(notices()).toHaveLength(1)
    expect(banner()).toBeNull()
  })

  it('a download that fails in the background leaves the notice to speak', async () => {
    installUpdater(offering('9.9.9', () => Promise.reject(new Error('connection reset'))))
    await openApp()
    await screen.findByText(t('update.available', { latest: '9.9.9', current: '1.15.0' }))
    expect(notices()).toHaveLength(1)
    expect(banner(), 'nothing was downloaded, so nothing is offered').toBeNull()
  })
})
