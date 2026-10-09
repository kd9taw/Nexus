// @vitest-environment jsdom
//
// AUTO-ROTATE IS OFFERED ON THE DASHBOARD WINDOW AND THE TV PAGE, AND NOWHERE ELSE (the operator's
// pick, 2026-09-29: "Auto-rotating boxes on the dashboard/TV"). ConnectView rotates only when its
// host passes `autoRotate`; what is proved HERE is which hosts do, by mounting the real three — the
// Connect pop-out (DetachedPanel), the TV page (ConnectTv) and the main window (App) — with
// ConnectView stubbed to record what it is handed. ConnectView's own rotation (timer, pauses, the
// menu) is in components/ConnectView.boxes.test.tsx.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, waitFor } from '@testing-library/react'

const fx = vi.hoisted(() => ({
  props: [] as Array<Record<string, unknown>>,
  SNAP: {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    mode: 'Normal',
    radio: { dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0, amp: null },
    aiCw: { enabled: false, status: '', text: '' },
    link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
    stations: [],
    conversations: [],
    activePeer: null,
    qso: null,
    fieldDay: null,
    recentDecodes: [],
    harqRescues: 0,
  },
  PROP: {
    advisory: { headline: '', bands: [], banners: [] },
    openings: [],
    dxpeditions: { workableNow: [], active: [], upcoming: [] },
    spaceWx: { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null },
    source: 'live',
    asOf: 0,
  },
}))

vi.mock('./components/ConnectView', () => ({
  ConnectView: (p: Record<string, unknown>) => {
    fx.props.push(p)
    return <main className="layout single" data-testid="connect-view" />
  },
}))
// Every api export auto-stubbed from the real module (App.js8workspace.test.tsx's pattern), with the
// shapes the three hosts read on mount.
vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => fx.SNAP),
    subscribeSnapshot: vi.fn((cb: (s: unknown) => void) => {
      cb(fx.SNAP)
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
    getPropagation: vi.fn(async () => fx.PROP),
    getFeedHealth: vi.fn(async () => null),
    getXrayNow: vi.fn(async () => null),
    // NOAA's daily file as the command answers when there is none. The auto-stub's `{}` has no `days`,
    // so a host that draws the file's newest day (the dashboard bar's SSN) would throw on it.
    getSolarIndices: vi.fn(async () => null),
    getDxpedWindows: vi.fn(async () => []),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => fx.SNAP),
    setArea: vi.fn(async () => fx.SNAP),
    appVersion: vi.fn(async () => '0.0.0-test'),
    getTvStation: vi.fn(async () => ({ call: 'KD9TAW', grid: 'EN52' })),
    getWindowBehind: vi.fn(async () => ({ supported: false, on: false })),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import { DetachedPanel } from './DetachedPanel'
import { ConnectTv } from './tv/ConnectTv'

// THE BUDGET (2026-10-09). The slowest case here, "the main window’s Connect does NOT", takes 0.49 s and 0.50 s on
// one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one core, past
// vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

beforeEach(() => {
  fx.props.length = 0
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  window.location.hash = ''
})

const lastProps = async () => {
  await waitFor(() => expect(fx.props.length, 'ConnectView was rendered').toBeGreaterThan(0))
  return fx.props[fx.props.length - 1]
}

describe('which Connect rotates a slot’s tabs', () => {
  it('the dashboard window (Connect popped out) offers it', async () => {
    await act(async () => {
      render(<DetachedPanel panel="connect" />)
    })
    expect((await lastProps()).autoRotate).toBe(true)
  })

  it('the TV page offers it', async () => {
    await act(async () => {
      render(<ConnectTv />)
    })
    expect((await lastProps()).autoRotate).toBe(true)
  })

  it('the main window’s Connect does NOT', async () => {
    localStorage.setItem('nexus.workspace', 'dx')
    window.location.hash = '#connect'
    render(<App />)
    const p = await lastProps()
    // Control: this is the main window's ConnectView — the one that offers ⧉ Pop out.
    expect(typeof p.onPopOut, 'control: the main window renders it').toBe('function')
    expect(p.autoRotate).not.toBe(true)
  })
})
