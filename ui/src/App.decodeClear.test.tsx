// @vitest-environment jsdom
//
// A UDP CLEAR ON THE FT DECODE PANES, through the REAL App and the REAL FT cockpit: what the panes
// show on the poll after it.
//
// A Clear (a companion program's clear, `clearTick` in the snapshot; the pane's own Erase does the
// same wipe) empties Band Activity, and the next poll's decode list, unchanged, fills it again with
// the last period's rows. That refill rides the panes re-ingesting every new decode list, so it holds
// only while App hands the cockpit the snapshot as polled, a new list each time. The keep-alive CPU
// work must leave this exactly as it was: this pins it, on screen and for a Clear that arrives while
// the FT screen is hidden.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, act, fireEvent } from '@testing-library/react'
import type { AppSnapshot } from './types'
import defaults from './components/__fixtures__/defaultSettings.json'

const snapshot = {
  mycall: 'W9XYZ',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    operatingMode: 'digital',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 100,
    nextSlotMs: 9000,
    rxLevel: 0.2,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: 0, dtSec: 0, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [
    { from: 'JA1XYZ', snr: -12, dtSec: 0.1, freqHz: 1200, message: 'CQ JA1XYZ PM95', isCq: true, directedToMe: false, worked: false, tier: 'FT8', rv: 0 },
  ],
  harqRescues: 0,
  hunt: null,
  b4MatchMode: false,
  clearTick: 0,
} as unknown as AppSnapshot

/** A fresh copy, as the IPC bridge decodes one every poll, with `edit` applied. */
function poll(edit: (s: AppSnapshot) => void = () => {}): AppSnapshot {
  const s = structuredClone(snapshot)
  edit(s)
  return s
}

let deliver: ((s: AppSnapshot) => void) | null = null
let latest: AppSnapshot = poll()

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    clientStatus: vi.fn(async () => null),
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => structuredClone(latest)),
    subscribeSnapshot: vi.fn((fn: (s: AppSnapshot) => void) => {
      deliver = fn
      return () => {
        deliver = null
      }
    }),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => structuredClone(defaults)),
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
    getKpForecast: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => structuredClone(latest)),
    setArea: vi.fn(async () => structuredClone(latest)),
    setTier: vi.fn(async () => structuredClone(latest)),
    haltTx: vi.fn(async () => structuredClone(latest)),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'

vi.setConfig({ testTimeout: 20_000 })

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { logbook: true } }))
  localStorage.setItem('nexus.workspace', 'dx')
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
  deliver = null
  latest = poll()
})

const send = async (s: AppSnapshot) => {
  latest = s
  await act(async () => {
    deliver!(s)
  })
}
/** Whether the FT cockpit's decode panes show JA1XYZ's row. */
const shown = () => (document.querySelector('.operate-host')?.textContent ?? '').includes('CQ JA1XYZ PM95')
const go = async (label: string) => {
  const btn = [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')].find(
    (b) => b.querySelector('.mode-label')?.textContent === label,
  )
  expect(btn, label).toBeTruthy()
  await act(async () => {
    fireEvent.click(btn!)
  })
}

async function mount(view: string) {
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(deliver).toBeTruthy())
  await send(poll())
}

describe('a UDP Clear on the FT decode panes', () => {
  it('on screen: the Clear empties the panes, and the next unchanged poll fills them again', async () => {
    await mount('operate')
    await waitFor(() => expect(shown()).toBe(true))
    await send(poll((s) => (s.clearTick = 1)))
    expect(shown()).toBe(false)
    await send(poll((s) => (s.clearTick = 1)))
    expect(shown()).toBe(true)
  })

  it('hidden: after a Clear and an unchanged poll behind Logbook, FT shows the rows when opened', async () => {
    await mount('logbook')
    await send(poll((s) => (s.clearTick = 1)))
    await send(poll((s) => (s.clearTick = 1)))
    await go('FT')
    await waitFor(() => expect(shown()).toBe(true))
  })
})
