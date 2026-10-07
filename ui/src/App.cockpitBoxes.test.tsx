// @vitest-environment jsdom
//
// THE BOXES IN THE REAL APP (any pane in any area, 2026-10-07): what App lends a cockpit's boxes, by what
// the screen does with it.
//   · ⛔ A click in a box never selects a station app-wide. The selected station is the one a CW macro's
//     `!` sends, so a stray click on a Getting Out row mid-QSO would re-target the next macro from a
//     display — the dashboard rail's rule (App.dashRail.test.tsx), here inside the CW cockpit itself.
//   · A Spots box's row click works the spot through the board's own Work and keys nothing.
//   · The hosted Remote page keeps its own panes: the same stored record draws no box there.
// Each case asserts the cockpit itself rendered, so nothing here can pass over a crash panel.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, cleanup, render, screen, waitFor, within, fireEvent } from '@testing-library/react'

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  return { ...auto, ...appApiAnswers() }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import * as api from './api'
import App from './App'
import { APP_SNAPSHOT } from './appCockpits.testkit'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
import type { AppSnapshot, Settings } from './types'

// The real App mounts per case; under the full suite's load that outruns vitest's default 5 s.
vi.setConfig({ testTimeout: 30_000 })

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, sstv: true, aprs: true, js8: true, connect: true }
/** CW's record with two boxes in column 2: the Selection box, so the boxes' own selection is visible,
 *  and a list a station can be clicked in. */
const cwBoxes = (second: string) =>
  JSON.stringify({ v: 2, state: { box1: 'docked', box2: 'docked' }, share: {}, boxes: { box1: 'selection', box2: second } })

async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await act(async () => {})
}
const cockpit = () => document.querySelector<HTMLElement>('main.cw-cockpit')
const box = (b: string) => cockpit()?.querySelector<HTMLElement>(`.pane-frame[data-pane="${b}"]`) ?? null

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  vi.mocked(api.selectPeer).mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})

describe('⛔ a click in a box never changes the station the cockpit is working', () => {
  it('a Getting Out row in a CW box selects in the boxes only — no app-wide select, which a macro would send', async () => {
    localStorage.setItem('nexus.panels.cw.main', cwBoxes('getout'))
    await mountOn('cw')
    expect(cockpit(), 'the CW cockpit did not render').not.toBeNull()
    await waitFor(() => expect(within(box('box2')!).getByText('K1ABC')).toBeTruthy())
    vi.mocked(api.selectPeer).mockClear()
    fireEvent.click(within(box('box2')!).getByText('K1ABC'))
    await act(async () => {})
    expect(api.selectPeer, 'a click in a box selected a station app-wide').not.toHaveBeenCalled()
    expect(box('box1')!.querySelector('.cs-call')?.textContent, 'the boxes’ own selection').toBe('K1ABC')
  })

  it('a Spots box works a spot through the board’s own Work, selecting in the boxes only and keying nothing', async () => {
    const K1CW = {
      call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW',
      submode: 'CW', spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: '', licensed: true, spotterLocal: true,
    }
    vi.mocked(api.getAllSpots).mockResolvedValue([K1CW] as unknown as Awaited<ReturnType<typeof api.getAllSpots>>)
    try {
      localStorage.setItem('nexus.panels.cw.main', cwBoxes('spotsBoard'))
      await mountOn('cw')
      expect(cockpit(), 'the CW cockpit did not render').not.toBeNull()
      const row = await waitFor(() => {
        const r = [...box('box2')!.querySelectorAll<HTMLElement>('.sp-row')].find((x) => x.querySelector('.np-call')?.textContent === 'K1CW')
        if (!r) throw new Error('the Spots box does not list the spot')
        return r
      })
      const VERBS = ['sendCw', 'atuTune', 'callStation', 'startCq'] as const
      const SWITCHES = ['setPtt', 'setTune', 'setTxEnabled'] as const
      for (const fn of [api.selectPeer, api.workSpot, ...VERBS.map((v) => api[v]), ...SWITCHES.map((v) => api[v])]) vi.mocked(fn).mockClear()
      await act(async () => {
        fireEvent.click(row)
      })
      await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
      await act(async () => {})
      expect(document.querySelector('.view-crash'), 'the cockpit the Work opened crashed').toBeNull()
      expect(vi.mocked(api.workSpot).mock.calls, 'the Work is not the board’s own').toEqual([['cw', 14.025, '20m', 'K1CW', undefined]])
      const worked = vi.mocked(api.workSpot).mock.invocationCallOrder[0]
      expect(vi.mocked(api.selectPeer).mock.invocationCallOrder.filter((n) => n < worked), 'the click selected the station app-wide').toEqual([])
      for (const v of VERBS) expect(api[v], `the Work keyed through ${v}`).not.toHaveBeenCalled()
      for (const v of SWITCHES) expect(vi.mocked(api[v]).mock.calls.filter(([on]) => on === true), `the Work turned ${v} on`).toEqual([])
    } finally {
      vi.mocked(api.getAllSpots).mockImplementation(async () => [])
    }
  })
})

describe('the hosted Remote page keeps its own panes', () => {
  it('the same stored record draws no box there, while the desktop draws it', async () => {
    localStorage.setItem('nexus.panels.cw.main', cwBoxes('getout'))
    await mountOn('cw')
    expect(box('box1'), 'control: the desktop draws the stored box').not.toBeNull()
    cleanup()
    window.location.hash = '#cw'
    render(
      <App
        remote={{
          snapshot: APP_SNAPSHOT as unknown as AppSnapshot,
          settings: settingsFixture as unknown as Settings,
          bandPlan: [],
          status: <div>Observer</div>,
          // A station that serves the CW and Phone screens, as the hosted page learns from its offer.
          cwPhone: true,
        }}
      />,
    )
    await screen.findByText('Observer')
    await act(async () => {})
    expect(document.querySelector('.app.remote-workspace'), 'premise: the Remote page mounted').not.toBeNull()
    // The hosted page opens on its own landing screen: go to CW the way an operator does.
    const cw = [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')].find((b) => b.querySelector('.mode-label')?.textContent === 'CW')
    expect(cw, 'premise: the Remote page offers CW').toBeTruthy()
    fireEvent.click(cw!)
    await act(async () => {})
    expect(cockpit(), 'premise: the Remote page shows the CW cockpit').not.toBeNull()
    expect(document.querySelector('.pane-frame[data-pane^="box"]'), 'a box on the Remote page').toBeNull()
  })
})
