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
// the FT screen is hidden, including a hidden Clear followed by the next period before FT is opened
// (the cleared period's rows are back, because the poll after the Clear refilled the hidden panes).
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
const shownIn = (sel: string, msg: string) => (document.querySelector(sel)?.textContent ?? '').includes(msg)
const shown = () => shownIn('.operate-host', 'CQ JA1XYZ PM95')
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

  // The next period, polled while FT is still hidden: the engine's decode list now holds only that
  // period, so the cleared period's rows are back only if the poll right after the Clear refilled the
  // hidden panes, as every poll did before the keep-alive work.
  const nextPeriod = (s: AppSnapshot) => {
    ;(s.radio as { slot: number }).slot = 101
    s.recentDecodes = [{ ...s.recentDecodes[0], from: 'K1ABC', freqHz: 1520, message: 'CQ K1ABC FN42' }]
  }

  it('hidden: a Clear, an unchanged poll, then a new period before FT is opened: the cleared rows are back', async () => {
    await mount('logbook')
    await send(poll((s) => (s.clearTick = 1)))
    await send(poll((s) => (s.clearTick = 1)))
    const later = (s: AppSnapshot) => {
      s.clearTick = 1
      nextPeriod(s)
    }
    await send(poll(later))
    await send(poll(later))
    await go('FT')
    await waitFor(() => expect(shownIn('.operate-host', 'CQ K1ABC FN42')).toBe(true))
    expect(shown()).toBe(true)
  })

  it('hidden: the same for a Clear of Rx Frequency, in the Rx Frequency pane', async () => {
    const atRx = (s: AppSnapshot) => (s.recentDecodes[0].freqHz = 1510)
    latest = poll(atRx)
    window.location.hash = '#logbook'
    render(<App />)
    await waitFor(() => expect(deliver).toBeTruthy())
    await send(poll(atRx))
    await send(poll((s) => (atRx(s), (s.rxFreqClearTick = 1))))
    await send(poll((s) => (atRx(s), (s.rxFreqClearTick = 1))))
    const later = (s: AppSnapshot) => {
      s.rxFreqClearTick = 1
      nextPeriod(s)
    }
    await send(poll(later))
    await send(poll(later))
    await go('FT')
    await waitFor(() => expect(shownIn('.cockpit-rxfreq', 'CQ K1ABC FN42')).toBe(true))
    expect(shownIn('.cockpit-rxfreq', 'CQ JA1XYZ PM95')).toBe(true)
  })

  // Leaving FT within one poll of a wipe made ON SCREEN: the next poll, unchanged, arrives with FT
  // already hidden, and it is that poll that refills the panes. Then the next period arrives before
  // FT is opened again: the wiped period's rows must be back, as they were before the keep-alive work.
  it("on screen, the pane's own Erase, then FT left before the next poll: the erased rows are back", async () => {
    await mount('operate')
    await waitFor(() => expect(shown()).toBe(true))
    const erase = document.querySelector<HTMLButtonElement>('.operate-host .od-clear')
    expect(erase).toBeTruthy()
    await act(async () => {
      fireEvent.click(erase!)
    })
    expect(shown()).toBe(false)
    await go('Logbook')
    await send(poll())
    await send(poll(nextPeriod))
    await send(poll(nextPeriod))
    await go('FT')
    await waitFor(() => expect(shownIn('.operate-host', 'CQ K1ABC FN42')).toBe(true))
    expect(shown()).toBe(true)
  })

  it('on screen, a UDP Clear, then FT left before the next poll: the cleared rows are back', async () => {
    await mount('operate')
    await waitFor(() => expect(shown()).toBe(true))
    await send(poll((s) => (s.clearTick = 1)))
    expect(shown()).toBe(false)
    await go('Logbook')
    await send(poll((s) => (s.clearTick = 1)))
    const later = (s: AppSnapshot) => {
      s.clearTick = 1
      nextPeriod(s)
    }
    await send(poll(later))
    await send(poll(later))
    await go('FT')
    await waitFor(() => expect(shownIn('.operate-host', 'CQ K1ABC FN42')).toBe(true))
    expect(shown()).toBe(true)
  })
})
