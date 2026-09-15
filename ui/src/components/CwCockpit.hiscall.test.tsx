// @vitest-environment jsdom
//
// #286 — AN EDITABLE HIS CALL FIELD ON THE CW SCREEN, FEEDING THE `!` MACRO.
//
// `!` in a CW macro is the worked call, expanded by the ENGINE from its active peer
// (`Engine::expand_cw` → `app.active_peer()`), and `selectPeer` is the one setter: a confirmed
// decode chip and a spot handoff already go through it. Until now the only way to change that call
// was to click a decoded chip — a station the decoder read wrong, or never read at all, could not
// be answered with F2. This field shows the active call (a decode fills it) and lets the operator
// type over it, committing through the SAME `selectPeer`. Nothing new keys the rig.
//
// Pinned here:
//   · it lives in the pinned TX dock beside the macros, never in a ⊞-removable pane;
//   · a decode fills it;
//   · Enter commits the typed call, normalised to callsign characters (the call is keyed
//     verbatim, so a space or a prosign character must not ride along);
//   · F2 pressed straight after typing sends the TYPED call — the commit lands before the send;
//   · clearing it clears the active peer.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot } from '../types'
import { selectPeer, sendCw } from '../api'
import { EN } from '../i18n'

const decodeState = {
  text: '',
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

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 }, rigModel: 0 })),
  getCatCwUnprovenRigModels: vi.fn(async () => []),
  setSettings: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => ({})),
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
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

const mockSelectPeer = selectPeer as unknown as ReturnType<typeof vi.fn>
const mockSendCw = sendCw as unknown as ReturnType<typeof vi.fn>

function snap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false,
      txEnabled: true, txAllowed: true, cwWpm: 22, cwKeyer: 'cat', filterWidthHz: 500,
      splitTxMhz: null, smeterDb: null,
    },
  } as unknown as AppSnapshot
}

async function flush() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

async function renderCockpit() {
  const r = render(<CwCockpit snap={snap()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
  await flush()
  return r
}

const field = () => screen.getByRole('textbox', { name: EN['cw.hisCall.label'] }) as HTMLInputElement

beforeEach(() => {
  decodeState.workedCall = null
  mockSelectPeer.mockClear()
  mockSendCw.mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('#286 His Call on the CW screen', () => {
  it('is in the pinned TX dock, never in a removable pane', async () => {
    await renderCockpit()
    expect(field().closest('.cockpit-txdock')).not.toBeNull()
    expect(field().closest('.pane-frame')).toBeNull()
  })

  it('a decode fills it with the worked call', async () => {
    decodeState.workedCall = 'W1AW'
    await renderCockpit()
    expect(field().value).toBe('W1AW')
  })

  it('Enter commits the typed call through selectPeer, normalised to callsign characters', async () => {
    await renderCockpit()
    fireEvent.change(field(), { target: { value: 'k1 abc?' } })
    fireEvent.keyDown(field(), { key: 'Enter' })
    await flush()
    expect(mockSelectPeer).toHaveBeenCalledWith('K1ABC')
  })

  it('F2 straight after typing sends the TYPED call: the commit lands before the send', async () => {
    decodeState.workedCall = 'W1AW'
    await renderCockpit()
    fireEvent.change(field(), { target: { value: 'N0CALL' } })
    fireEvent.keyDown(window, { key: 'F2' })
    await flush()
    await flush()
    expect(mockSelectPeer).toHaveBeenCalledWith('N0CALL')
    expect(mockSendCw).toHaveBeenCalledTimes(1)
    expect(mockSelectPeer.mock.invocationCallOrder[0]).toBeLessThan(mockSendCw.mock.invocationCallOrder[0])
  })

  it('clearing it clears the active peer', async () => {
    decodeState.workedCall = 'W1AW'
    await renderCockpit()
    fireEvent.change(field(), { target: { value: '' } })
    fireEvent.keyDown(field(), { key: 'Enter' })
    await flush()
    expect(mockSelectPeer).toHaveBeenCalledWith(null)
  })
})
