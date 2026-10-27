// @vitest-environment jsdom
//
// PROVE TX IN THE SETUP WIZARD KEYS WITH WHAT THE OWNER JUST PICKED.
//
// A tune carrier keys through the radio loop, and the loop keys with SAVED settings. The wizard
// holds its picks in a draft until Test CAT or Finish saves them, so a Prove TX that keyed at once
// proved the old setting. On the radios Hamlib cannot key over CAT that is the whole point of the
// button: the owner picks how the interface keys the radio and wants to see it transmit.
//
// Mounts the REAL App (the App.neededWizard.test.tsx pattern) because App owns both halves, the
// draft save and the key, and reads what is saved at the moment the key goes down.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen, within } from '@testing-library/react'
import type { AppSnapshot } from './types'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074,
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
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [],
  harqRescues: 0,
} as unknown as AppSnapshot

// The station: what is saved, and what happened to it, in order.
let saved: Record<string, unknown> = {}
let keyedWith: unknown[] = []
let order: string[] = []

const BRIDGE = {
  portName: 'COM4',
  suggestedModel: null,
  suggestedModelName: null,
  suggestedAudio: null,
  suggestedAudioOut: null,
  chip: 'FT232R',
  civSide: null,
  interfaceName: null,
  interfacePttMethod: null,
}

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
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
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
    haltTx: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
    // The rig step: one bare bridge chip, nothing answers Auto-test, and the IC-706MKIIG to pick.
    detectRigs: vi.fn(async () => [BRIDGE]),
    discoverFlex: vi.fn(async () => []),
    probeCatPorts: vi.fn(async () => ({ found: false, detail: 'no rig answered' })),
    getRigModels: vi.fn(async () => [[3011, 'Icom IC-706MKIIG']]),
    getAllRigModels: vi.fn(async () => []),
    getAudioDevices: vi.fn(async () => ({ input: [], output: [] })),
    // The station itself.
    getSettings: vi.fn(async () => ({ ...saved })),
    setSettings: vi.fn(async (s: Record<string, unknown>) => {
      saved = { ...s }
      order.push('save')
      return snapshot
    }),
    testCat: vi.fn(async () => {
      order.push('test')
      return { ok: true, detail: 'the radio answered' }
    }),
    setTune: vi.fn(async (on: boolean) => {
      if (on) {
        keyedWith.push(saved.pttMethod)
        order.push('key')
      }
      return snapshot
    }),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'

// THE BUDGET: the same real App as App.neededWizard.test.tsx, whose slowest test takes 0.55 s on a
// quiet box and 4.5 s with a tenth of a CPU. 15 s is the house budget; a hang still fails.
vi.setConfig({ testTimeout: 15_000 })

beforeEach(() => {
  localStorage.clear()
  saved = { ...(defaultSettings as Record<string, unknown>) }
  keyedWith = []
  order = []
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
afterEach(cleanup)

describe('Prove TX in the setup wizard, through App', () => {
  it('saves the Serial RTS just picked, then keys with it', async () => {
    expect(saved.pttMethod).toBe('vox') // the disagreeing value: what a key without the save uses
    window.location.hash = '#operate'
    render(<App />)
    const wizard = await screen.findByRole('dialog', { name: 'Set up Nexus' })
    // A first run asks "How much have you done?" first; passing it unanswered is allowed.
    fireEvent.click(within(wizard).getByRole('button', { name: /Next/ }))
    fireEvent.click(within(wizard).getByRole('button', { name: /Next/ }))
    fireEvent.click(await within(wizard).findByRole('button', { name: /COM4/ }))
    const model = (await within(wizard).findByLabelText(/Which radio is this/)) as HTMLSelectElement
    await waitFor(() => expect(within(model).getByText('Icom IC-706MKIIG')).toBeTruthy())
    fireEvent.change(model, { target: { value: '3011' } })
    const keying = within(wizard).getByRole('group', { name: 'How does your interface key the radio?' })
    fireEvent.click(within(keying).getByRole('button', { name: /Serial RTS/ }))

    order = []
    fireEvent.click(within(keying).getByRole('button', { name: 'Prove TX' }))
    const ask = await screen.findByRole('dialog', { name: 'Prove the transmit path?' })
    expect(order).toEqual([]) // nothing saved or keyed on the ask
    fireEvent.click(within(ask).getByRole('button', { name: 'Prove TX' }))

    await waitFor(() => expect(keyedWith).toHaveLength(1))
    expect(keyedWith).toEqual(['rts'])
    expect(order).toEqual(['save', 'test', 'key'])
  })
})
