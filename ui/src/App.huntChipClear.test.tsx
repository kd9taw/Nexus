// @vitest-environment jsdom
//
// THE LOG LINE'S HUNT TAG ✕, IN THE REAL APP. Every log line that shows the tag ends the hunt from it
// (Phone, CW, RTTY, PSK, JS8 and Satellites), and App applies the snapshot the clear answered, as it
// does the POTA / SOTA banner ✕'s: the tag goes at once, and nothing waits at the top of the POTA /
// SOTA view. No snapshot is ever pushed here (`subscribeSnapshot` never answers), so only the handed
// one can take the tag away. The strip's own behaviour is LogEntry.huntClear.test.tsx.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, waitFor, within } from '@testing-library/react'

// THE STATION: one hunt, set on the POTA / SOTA view, that only `clear_hunt_target` ends. Every answer
// that carries the station's snapshot names it, as the engine's would.
const station = vi.hoisted(() => ({ hunt: null as { program: string; reference: string; call: string } | null }))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers, APP_SNAPSHOT } = await import('./appCockpits.testkit')
  const snapshot = () => ({ ...APP_SNAPSHOT, hunt: station.hunt })
  return {
    ...auto,
    ...appApiAnswers(),
    getSnapshot: vi.fn(async () => snapshot()),
    setOperatingMode: vi.fn(async () => snapshot()),
    setArea: vi.fn(async () => snapshot()),
    selectPeer: vi.fn(async () => snapshot()),
    workSpot: vi.fn(async () => snapshot()),
    clearHuntTarget: vi.fn(async () => {
      station.hunt = null
      return snapshot()
    }),
    // The Satellites view with no catalog (App.connectBoards.test.tsx's answer).
    getSatellites: vi.fn(async () => null),
    // The log line looks the park up; nothing here knows it.
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    qrzLookup: vi.fn(async () => null),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import App from './App'
import { clearHuntTarget } from './api'
import { COCKPIT_MAIN } from './appCockpits.testkit'
import { t } from './i18n'

// THE BUDGET: App.neededPark.test.tsx's, for the same mount (15 s; a test that hangs still fails).
vi.setConfig({ testTimeout: 15_000 })

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, js8: true, sats: true, pota: true }

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  localStorage.setItem('nexus.needed.autopop', 'off')
  vi.mocked(clearHuntTarget).mockClear()
  station.hunt = { program: 'POTA', reference: 'US-1000', call: 'K9ABC' }
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

async function mountOn(view: string): Promise<void> {
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
}

/** The hunt tag of the log line under `root`, once it is there. */
function tagIn(root: string): Promise<HTMLElement> {
  return waitFor(() => {
    expect(document.querySelector('.view-crash'), 'the view crashed').toBeNull()
    const el = document.querySelector<HTMLElement>(`${root} .le-hunt-chip`)
    expect(el, `no hunt tag on the log line under ${root}`).toBeTruthy()
    return el!
  })
}

/** ModeNav's button for a section, by its visible label (App.dashRail.test.tsx's helper). */
function navTo(label: string): void {
  const btn = [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')].find(
    (b) => b.querySelector('.mode-label')?.textContent === label,
  )
  expect(btn, `no navigation button labelled ${label}`).toBeTruthy()
  fireEvent.click(btn!)
}

describe('the hunt tag’s ✕ on every log line that shows it', () => {
  const lines: [string, string][] = [
    ['phone', COCKPIT_MAIN.phone],
    ['cw', COCKPIT_MAIN.cw],
    ['rtty', COCKPIT_MAIN.rtty],
    ['psk', COCKPIT_MAIN.psk],
    ['js8', COCKPIT_MAIN.js8],
    ['sats', '.sats-log'],
  ]
  for (const [view, root] of lines) {
    it(`${view}: ends the hunt, and the tag goes with the snapshot it answered`, async () => {
      await mountOn(view)
      const tag = await tagIn(root)
      fireEvent.click(within(tag).getByRole('button', { name: t('ota.hunt.clear') }))
      await waitFor(() => expect(document.querySelector(`${root} .le-hunt-chip`), 'the tag outlived the hunt').toBeNull())
      expect(vi.mocked(clearHuntTarget).mock.calls).toEqual([[]])
    })
  }
})

describe('after the ✕, the POTA / SOTA view', () => {
  // The view itself, not a box of it (`data-ota-box`).
  const VIEW = 'section.pota-view:not([data-ota-box])'
  it('has nothing waiting at the top', async () => {
    await mountOn('pota')
    // CONTROL — the same hunt shows there before the ✕.
    await waitFor(() => expect(document.querySelector(`${VIEW} .pota-hunt-banner`), 'premise: the hunt’s banner').toBeTruthy())
    navTo('Phone')
    const tag = await tagIn(COCKPIT_MAIN.phone)
    fireEvent.click(within(tag).getByRole('button', { name: t('ota.hunt.clear') }))
    await waitFor(() => expect(document.querySelector(`${COCKPIT_MAIN.phone} .le-hunt-chip`)).toBeNull())
    navTo('POTA/SOTA')
    const view = await waitFor(() => {
      const el = document.querySelector<HTMLElement>(VIEW)
      expect(el, 'the POTA / SOTA view never came back').toBeTruthy()
      return el!
    })
    expect(view.querySelector('.pota-hunt-banner'), 'the ended hunt still waits at the top of the view').toBeNull()
  })
})
