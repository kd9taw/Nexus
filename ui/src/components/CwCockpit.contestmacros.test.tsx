// @vitest-environment jsdom
//
// THE CW MACRO SET IN A CONTEST THAT IS NOT FIELD DAY.
//
// The cockpit had two built-in sets: casual, and Field Day. Whenever ANY contest was running
// it used the Field Day one, so `F1` in the Illinois QSO Party or CQ WW CW keyed
// `CQ FD DE …` — Field Day's own call, on the air, in somebody else's contest. There is now a
// third set: the same contest cadence with the universal contest call, `CQ TEST`.
//
// Field Day's set is unchanged, and that is asserted here rather than assumed: it is what its
// operators have been keying for releases.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot, FieldDayStatus } from '../types'

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
  settingsState.rigModel = 0
  unprovenModels = []
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

async function renderCockpit(props: Partial<Parameters<typeof CwCockpit>[0]> = {}) {
  const r = render(<CwCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} {...props} />)
  // Let the mount-time getSettings / cwDecode / previewCw promises land.
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}


const FD = { event: 'arrlfd', running: false } as unknown as FieldDayStatus
const ILQP = { event: 'ilqp', running: false } as unknown as FieldDayStatus

/** The label and key of each macro button the dock renders. */
const macroRows = () =>
  [...document.querySelectorAll('.cw-macro')].map((b) => ({
    key: b.querySelector('.cw-macro-key')?.textContent ?? '',
    label: b.querySelector('.cw-macro-label')?.textContent ?? '',
  }))

describe('the CW macro set follows the contest that is running', () => {
  it('keys CQ TEST in another contest, CQ FD in Field Day, and CQ outside a contest', async () => {
    await renderCockpit({ fieldDay: ILQP })
    expect(macroRows()[0]).toEqual({ key: 'F1', label: 'CQ TEST' })
    // The rest of the contest cadence is Field Day's, which is what a contest needs.
    expect(macroRows().map((m) => m.key)).toEqual(['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8'])
    expect(macroRows()[2].label).toBe('Exch')

    // POSITIVE CONTROLS: Field Day still says CQ FD, and no contest still says CQ.
    cleanup()
    await renderCockpit({ fieldDay: FD })
    expect(macroRows()[0]).toEqual({ key: 'F1', label: 'CQ FD' })
    cleanup()
    await renderCockpit()
    expect(macroRows()[0]).toEqual({ key: 'F1', label: 'CQ' })
  })

  it('sends the contest call, not Field Day\'s', async () => {
    const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
    await renderCockpit({ fieldDay: ILQP })
    await act(async () => {
      fireEvent.click([...document.querySelectorAll('.cw-macro')][0])
    })
    expect(api.sendCw).toHaveBeenCalledWith('CQ TEST DE {MYCALL} {MYCALL} K')
    // POSITIVE CONTROL: Field Day sends its own.
    api.sendCw.mockClear()
    cleanup()
    await renderCockpit({ fieldDay: FD })
    await act(async () => {
      fireEvent.click([...document.querySelectorAll('.cw-macro')][0])
    })
    expect(api.sendCw).toHaveBeenCalledWith('CQ FD DE {MYCALL} {MYCALL} K')
  })
})
