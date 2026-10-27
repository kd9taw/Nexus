// @vitest-environment jsdom
//
// THE CW POTA SET. While a park activation runs and no contest does, the cockpit's built-in keys
// are the POTA set: F1 CQ POTA, F2 the activator's exchange with every park, F3 TU 73. An
// operator's own profile keys still win, and a contest's set wins over it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { CW_POTA_SET, CwCockpit } from './CwCockpit'
import { cwStoredBuiltIn, withCwEntry } from '../features/cwMacros'
import type { AppSnapshot, FieldDayStatus } from '../types'

// 15 s is the house budget for a cockpit mount under a loaded suite; a test that hangs still fails.
vi.setConfig({ testTimeout: 15_000 })

const decodeState = {
  text: 'CQ CQ DE KD9TAW',
  wpm: 22,
  sent: [] as string[],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
  rst: null as string | null,
  name: null as string | null,
}

/** What the mount-time `getSettings` resolves with. Mutable per test, like `decodeState` —
 *  the cockpit reads `rigModel` from here to decide whether the CAT-keying caution applies. */
const settingsState = {
  macros: { cwProfiles: [] as unknown[], activeCwProfile: 0 },
  rigModel: 0,
}
/** The backend's "CAT CW keying is unproven on this model" list. Empty = rule unread. */
let unprovenModels: number[] = []
/** The activation the snapshot carries (`snap.activation`). */
let activation: { program: string | null; reference: string | null; qsoCount: number; restored?: boolean } = {
  program: null,
  reference: null,
  qsoCount: 0,
}

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => settingsState),
  getCatCwUnprovenRigModels: vi.fn(async () => unprovenModels),
  setSettings: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => {}),
  setCwKeyer: vi.fn(async () => null),
  setCwWpm: vi.fn(async () => {}),
  stopCw: vi.fn(async () => {}),
  cwDecode: vi.fn(async () => decodeState),
  cwClear: vi.fn(async () => {}),
  setAiCw: vi.fn(async () => {}),
  selectPeer: vi.fn(async () => null),
  previewCw: vi.fn(async (t: string) => t),
  pointRotatorAtCall: vi.fn(async () => 0),
  setRigFunc: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  setTune: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
}))

vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
// The stub reports `titled` so this suite can see the ONE prop that is a placement decision
// rather than log behaviour: whether the strip draws its own heading under a frame head that
// already says LOG. The strip's own half is in LogEntry.density.test.tsx.
vi.mock('./LogEntry', () => ({
  LogEntry: (p: { titled?: boolean }) => (
    <div data-testid="log-stub" data-titled={String(p.titled ?? true)} />
  ),
}))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeEach(() => {
  decodeState.sent = []
  decodeState.keyerError = null
  settingsState.macros.cwProfiles = []
  settingsState.rigModel = 0
  unprovenModels = []
  activation = { program: null, reference: null, qsoCount: 0 }
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      rigMode: 'CW',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      cwWpm: 22,
      cwKeyer: 'cat',
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      filterWidthHz: 500,
      splitTxMhz: null,
      smeterDb: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

const withActivation = () => ({ ...makeSnap(), activation }) as AppSnapshot

async function renderCockpit(props: Partial<Parameters<typeof CwCockpit>[0]> = {}) {
  const r = render(<CwCockpit snap={withActivation()} theme="dark" onWorkSpot={() => {}} spots={[]} {...props} />)
  // Let the mount-time getSettings / cwDecode / previewCw promises land.
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}


const ILQP = { event: 'ilqp', running: false } as unknown as FieldDayStatus

/** The label and key of each macro button the dock renders. */
const macroRows = () =>
  [...document.querySelectorAll('.cw-macro')].map((b) => ({
    key: b.querySelector('.cw-macro-key')?.textContent ?? '',
    label: b.querySelector('.cw-macro-label')?.textContent ?? '',
  }))

const pressF = async (n: number) => {
  await act(async () => {
    fireEvent.click([...document.querySelectorAll('.cw-macro')][n - 1])
  })
}

describe('the POTA set while a park activation runs', () => {
  it('keys CQ POTA and the park exchange, and a contest or no activation keeps its own set', async () => {
    const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
    activation = { program: 'POTA', reference: 'US-1234', qsoCount: 0 }
    await renderCockpit()
    expect(macroRows().map((m) => m.key)).toEqual(['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8'])
    await pressF(1)
    expect(api.sendCw).toHaveBeenLastCalledWith('CQ POTA DE {MYCALL} {MYCALL} K')
    await pressF(2)
    expect(api.sendCw).toHaveBeenLastCalledWith('! UR 5NN 5NN {MYSTATE} {MYPARK} BK')
    await pressF(3)
    expect(api.sendCw).toHaveBeenLastCalledWith('TU 73 {MYCALL} POTA')

    // A contest running wins: its exchange is the one that belongs on the air.
    api.sendCw.mockClear()
    cleanup()
    await renderCockpit({ fieldDay: ILQP })
    await pressF(1)
    expect(api.sendCw).toHaveBeenLastCalledWith('CQ TEST DE {MYCALL} {MYCALL} K')
    // A summit, and no activation at all, keep the everyday set.
    for (const a of [
      { program: 'SOTA', reference: 'W9/WI-001', qsoCount: 0 },
      { program: null, reference: null, qsoCount: 0 },
    ]) {
      activation = a
      api.sendCw.mockClear()
      cleanup()
      await renderCockpit()
      await pressF(1)
      expect(api.sendCw, String(a.program)).toHaveBeenLastCalledWith('CQ CQ DE {MYCALL} {MYCALL} K')
    }
  })

  it('follows the snapshot: an activation started with the cockpit open brings the set, Stop takes it away', async () => {
    const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
    const r = await renderCockpit()
    await pressF(1)
    expect(api.sendCw).toHaveBeenLastCalledWith('CQ CQ DE {MYCALL} {MYCALL} K')
    activation = { program: 'POTA', reference: 'US-1234,US-5678', qsoCount: 0 }
    r.rerender(<CwCockpit snap={withActivation()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await pressF(1)
    expect(api.sendCw).toHaveBeenLastCalledWith('CQ POTA DE {MYCALL} {MYCALL} K')
    activation = { program: null, reference: null, qsoCount: 0 }
    r.rerender(<CwCockpit snap={withActivation()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await pressF(1)
    expect(api.sendCw).toHaveBeenLastCalledWith('CQ CQ DE {MYCALL} {MYCALL} K')
  })

  it('a paused activation (taken up at launch, not yet confirmed) keys the everyday set until it is resumed', async () => {
    const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
    activation = { program: 'POTA', reference: 'US-1234', qsoCount: 0, restored: true }
    const r = await renderCockpit()
    await pressF(1)
    expect(api.sendCw).toHaveBeenLastCalledWith('CQ CQ DE {MYCALL} {MYCALL} K')
    await pressF(2)
    expect(api.sendCw).not.toHaveBeenLastCalledWith('! UR 5NN 5NN {MYSTATE} {MYPARK} BK')
    activation = { program: 'POTA', reference: 'US-1234', qsoCount: 0, restored: false }
    r.rerender(<CwCockpit snap={withActivation()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await pressF(1)
    expect(api.sendCw).toHaveBeenLastCalledWith('CQ POTA DE {MYCALL} {MYCALL} K')
  })

  it('an operator’s own profile keys win over the POTA set', async () => {
    const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
    activation = { program: 'POTA', reference: 'US-1234', qsoCount: 0 }
    settingsState.macros.cwProfiles = [{ name: 'Mine', macros: [{ key: 'F1', label: 'CQ', text: 'CQ CQ POTA {MYCALL} K' }] }]
    await renderCockpit()
    await pressF(1)
    expect(api.sendCw).toHaveBeenLastCalledWith('CQ CQ POTA {MYCALL} K')
  })

  it('is editable: a key saved from it makes the profile the operator’s, the POTA keys with the one changed', () => {
    const builtIn = CW_POTA_SET.map(cwStoredBuiltIn)
    const saved = withCwEntry([{ name: 'Default', macros: [] }], 0, builtIn, 'F3', {
      label: 'TU',
      text: 'TU 72 {MYCALL} POTA',
    })
    expect(saved.profiles[0].macros.map((m) => m.text)).toEqual([
      'CQ POTA DE {MYCALL} {MYCALL} K',
      '! UR 5NN 5NN {MYSTATE} {MYPARK} BK',
      'TU 72 {MYCALL} POTA',
      '{MYCALL}',
      '! ',
      '{MYPARK} {MYPARK}',
      'AGN AGN',
      '? ',
    ])
  })
})
