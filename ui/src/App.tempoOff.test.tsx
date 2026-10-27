// @vitest-environment jsdom
//
// #402: Tempo switched off in Settings ▸ Features. A Tempo mode remembered from before must not
// bring the station up in Tempo, where its rail item is gone and nothing leads back: App opens it
// on FT8/FT4 instead, and says so to the station. The control is the same start with Tempo on.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import type { AppSnapshot } from './types'

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.078,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'TempoFast', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [],
  harqRescues: 0,
} as unknown as AppSnapshot

// Every export auto-stubbed from the real module (App.js8workspace.test.tsx's pattern); the few App
// needs a SHAPE from on mount are given one.
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
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
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
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import * as api from './api'
import { defaultState, toggleFeature } from './features/state'

vi.setConfig({ testTimeout: 15_000 })

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  vi.mocked(api.setArea).mockClear()
})

async function start(tempoOn: boolean) {
  localStorage.setItem('nexus.workspace', 'msg')
  const features = tempoOn ? defaultState() : toggleFeature(defaultState(), 'chat')
  localStorage.setItem('nexus.features.v1', JSON.stringify(features))
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(vi.mocked(api.setArea)).toHaveBeenCalled())
  return vi.mocked(api.setArea).mock.calls.map((c) => c[0])
}

describe('a remembered Tempo mode with Tempo switched off (#402)', () => {
  it('opens on FT8/FT4 and never asks the station for Tempo', async () => {
    const areas = await start(false)
    expect(areas).not.toContain('msg')
    expect(areas).toContain('dx')
    // The remembered mode is left as it was, so turning Tempo back on brings it back next start.
    expect(localStorage.getItem('nexus.workspace')).toBe('msg')
  })

  it('control: with Tempo on, the remembered Tempo mode still opens', async () => {
    const areas = await start(true)
    expect(areas).toEqual(['msg'])
  })
})
