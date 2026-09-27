// @vitest-environment jsdom
//
// THE EYES-FREE TX STATE FOLLOWS THE TRANSMITTER, WHOEVER KEYED IT.
//
// App tells a screen-reader user when the rig keys and unkeys ("Transmitting" / "Receiving",
// assertive) and, with Settings ▸ Appearance ▸ Accessibility & eyes-free ▸ TX / RX earcon on,
// plays a rising tone at key-up and a falling one at unkey. Both keyed on `radio.transmitting`,
// the FT slot flag, which only the slot/beacon path writes: a Phone voice over, a CW over or a
// tune carrier keyed the rig in silence while the ON AIR sign on screen was red. They now follow
// `isOnAir()`, the answer that sign shows (operator, 2026-09-27: "Fix it"). Sound and speech
// only: nothing keys differently.
//
// This mounts the REAL App and drives it through its own snapshot subscription, because the rule
// lives in App's effect: a test of a hook would prove the rule and not the wiring.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, waitFor } from '@testing-library/react'
import { t } from './i18n'
import defaults from './components/__fixtures__/defaultSettings.json'
import type { AppSnapshot, RadioStatus, Settings } from './types'

const base = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal',
  radio: {
    dialMhz: 14.25, band: '20m', catOk: true, sideband: 'USB', transmitting: false, tuning: false,
    txBusyReason: null, rigKeyed: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500,
    txOffsetHz: 1500, txLevel: 0.5, slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null,
  recentDecodes: [], harqRescues: 0, logTick: 1,
} as unknown as AppSnapshot
const snapWith = (over: Partial<RadioStatus>) => ({ ...base, radio: { ...base.radio, ...over } }) as AppSnapshot

const state = vi.hoisted(() => ({
  settings: null as unknown,
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
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => base),
    subscribeSnapshot: vi.fn((next: (s: unknown) => void) => {
      state.push = next
      return () => {}
    }),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => state.settings),
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
    setOperatingMode: vi.fn(async () => base),
    setArea: vi.fn(async () => base),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
  }
})
vi.mock('./announce', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  announce: vi.fn(),
}))
vi.mock('./alerts', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  txEarcon: vi.fn(),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import { getSettings } from './api'
import { announce } from './announce'
import { txEarcon } from './alerts'

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { spots: true } }))
  localStorage.setItem('nexus.needed.autopop', 'off')
  state.settings = { ...defaults, soundTxState: true } as unknown as Settings
  state.push = null
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

/** Mount the real App (App.spotsWorked.test.tsx's mount), idle, with its settings in. */
async function openApp() {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(state.push).not.toBeNull())
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(vi.mocked(getSettings)).toHaveBeenCalled())
  await act(async () => {})
  vi.mocked(announce).mockClear()
  vi.mocked(txEarcon).mockClear()
}

const ON = () => t('shell.tx.announce.on')
const OFF = () => t('shell.tx.announce.off')

/** The TX-state cues since the last clear: what was said assertively, and which tones played. */
function cues() {
  return {
    said: vi.mocked(announce).mock.calls.filter(([text, o]) => o?.assertive && (text === ON() || text === OFF())).map(([text]) => text),
    tones: vi.mocked(txEarcon).mock.calls.map(([on]) => on),
  }
}

async function deliver(over: Partial<RadioStatus>) {
  await act(async () => {
    state.push!(snapWith(over))
  })
}

/** Every over the task names, and the ones beside them, as the snapshot reports them. The
 *  sentences are the engine's own (`TxOwner::busy_reason`). */
const KEYED: [string, Partial<RadioStatus>][] = [
  ['a Phone voice over', { txBusyReason: 'A voice message is transmitting — stop it first' }],
  ['a CW over', { txBusyReason: 'CW is sending — stop it first' }],
  ['a tune carrier', { tuning: true, txBusyReason: 'Tune carrier is up — stop tuning first' }],
  ['an FT over (as before)', { transmitting: true }],
]

describe('the eyes-free TX state follows every over', () => {
  it.each(KEYED)('%s is announced and heard at key-up and at unkey', async (_what, over) => {
    await openApp()
    await deliver(over)
    await waitFor(() => expect(cues()).toEqual({ said: [ON()], tones: [true] }))
    await deliver({})
    await waitFor(() => expect(cues()).toEqual({ said: [ON(), OFF()], tones: [true, false] }))
  })

  it('says nothing and plays nothing while transmit is only armed', async () => {
    await openApp()
    await deliver({ txEnabled: true })
    // Control on the same mount: an FT key-up after it IS heard, so the silence above is the
    // arming state and not a subscription that never delivers.
    await deliver({ txEnabled: true, transmitting: true })
    await waitFor(() => expect(cues()).toEqual({ said: [ON()], tones: [true] }))
  })

  it('announces a voice over with the earcon off, and plays no tone', async () => {
    state.settings = { ...defaults, soundTxState: false } as unknown as Settings
    await openApp()
    await deliver({ txBusyReason: 'A voice message is transmitting — stop it first' })
    await waitFor(() => expect(cues().said).toEqual([ON()]))
    expect(cues().tones).toEqual([])
  })
})
