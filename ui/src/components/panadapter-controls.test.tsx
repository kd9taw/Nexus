// @vitest-environment jsdom
//
// THE PANADAPTER CONTROLS MUST OUTLIVE THE FEED (2026-08-20).
//
// Two operator-reported faults in a row came from cockpit JSX gates, and both walked past a suite of
// 3400 tests because nothing here ever rendered a cockpit with an FT-710 RF scope and asked which
// controls exist. This file does exactly that, and only that.
//
// The fault it pins: the panadapter block was gated on RF rows ARRIVING. In FIX with no start
// stated no rows arrive BY DESIGN, so the block unmounted and took the "FIX starts here" button with
// it — the one control that would have brought the rows back. `PhoneScope` is stubbed here, so no
// feed is ever reported, which IS that state: the radio has a scope, Nexus cannot place the sweep.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import type { AppSnapshot } from '../types'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'

vi.mock('../api', () => ({
  // Added by the 1.9.x merge-forward: CwCockpit now asks which rig models have an
  // UNPROVEN CAT-CW path. A mock missing it throws inside the component, which surfaces
  // as three unrelated-looking panadapter failures rather than as a missing export.
  getCatCwUnprovenRigModels: vi.fn(async () => []),
  setPtt: vi.fn(async () => {}),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  getVoiceMessages: vi.fn(async () => []),
  playVoiceMessage: vi.fn(async () => ({})),
  stopVoice: vi.fn(async () => ({})),
  startVoiceRecording: vi.fn(async () => ({})),
  stopVoiceRecording: vi.fn(async () => []),
  cancelVoiceRecording: vi.fn(async () => ({})),
  clearVoiceMessage: vi.fn(async () => []),
  importVoiceMessage: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
  setSettings: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => {}),
  setCwKeyer: vi.fn(async () => ({})),
  setCwWpm: vi.fn(async () => {}),
  stopCw: vi.fn(async () => {}),
  cwDecode: vi.fn(async () => ({})),
  cwClear: vi.fn(async () => {}),
  setAiCw: vi.fn(async () => {}),
  selectPeer: vi.fn(async () => ({})),
  previewCw: vi.fn(async () => ({})),
  pointRotatorAtCall: vi.fn(async () => 0),
  // The real CockpitHeader hosts RotorStrip, which polls these on mount.
  readRotator: vi.fn(async () => ({})),
  stopRotator: vi.fn(async () => ({})),
  getDeclination: vi.fn(async () => 0),
  getSatTrackStatus: vi.fn(async () => ({})),
  getSatTransponder: vi.fn(async () => ({})),
  setSatTransponder: vi.fn(async () => {}),
  stopSatTrack: vi.fn(async () => ({})),
  getRttyState: vi.fn(async () => ({})),
  getLicensedBandPlan: vi.fn(async () => []),
  rttyArm: vi.fn(async () => ({})),
  rttySend: vi.fn(async () => ({})),
  rttyStop: vi.fn(async () => ({})),
  rttyClear: vi.fn(async () => ({})),
  rttyAfcReset: vi.fn(async () => ({})),
  rttyNet: vi.fn(async () => ({})),
  rttySetAuto: vi.fn(async () => ({})),
  rttyAutoCq: vi.fn(async () => ({})),
  rttyAutoAnswer: vi.fn(async () => ({})),
  rttyAutoAbort: vi.fn(async () => ({})),
  getPskState: vi.fn(async () => ({})),
  pskArm: vi.fn(async () => ({})),
  pskAutoArm: vi.fn(async () => ({})),
  pskClear: vi.fn(async () => ({})),
  pskAfcReset: vi.fn(async () => ({})),
  pskNet: vi.fn(async () => ({})),
  pskSend: vi.fn(async () => ({})),
  pskSetLatched: vi.fn(async () => ({})),
  pskType: vi.fn(async () => ({})),
  pskStop: vi.fn(async () => ({})),
  getSstvState: vi.fn(async () => ({})),
  sstvArm: vi.fn(async () => ({})),
  sstvAutoArm: vi.fn(async () => ({})),
  sstvSend: vi.fn(async () => ({})),
  sstvStop: vi.fn(async () => ({})),
  setOperatingMode: vi.fn(async () => ({})),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// Canvas/scope children only. CockpitHeader is DELIBERATELY REAL — see the file header.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
const radio = {
  dialMhz: 14.2,
  band: '20m',
  catOk: true,
  sideband: 'USB',
  sidebandOverride: null,
  rigMode: 'USB',
  transmitting: false,
  tuning: false,
  txEnabled: true,
  txAllowed: true,
  qsoRecording: false,
  rfPower: null,
  micGain: null,
  nrLevel: 0.3,
  agc: 'fast',
  nb: true,
  nr: true,
  notch: null,
  comp: null,
  vox: null,
  filterWidthHz: 500,
  splitTxMhz: null,
  smeterDb: null,
  cwWpm: 22,
  cwKeyer: 'cat',
  phoneSegLo: null,
  phoneSegHi: null,
}


function panelsWith(removed: readonly string[]) {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id: string) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  } as never
}

const panels = panelsWith([])

/** A snapshot whose radio reports the rig's scope MODE — the `SS` P3 byte, widened for JSON. */
function snapWithMode(code: number): AppSnapshot {
  return { mycall: 'KD9TAW', radio: { ...radio, scopeModeCode: code } } as unknown as AppSnapshot
}

const FIX_NORMAL = 0x41 // 'A' — W/F FIX (NORMAL)
const CENTER_NORMAL = 0x34 // '4' — W/F CENTER (NORMAL)
const CURSOR_NORMAL = 0x37 // '7' — W/F CURSOR (NORMAL)

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver

afterEach(() => cleanup())

const cockpits = [
  { name: 'Phone', render: (s: AppSnapshot) => render(<PhoneCockpit snap={s} theme="dark" onWorkSpot={() => {}} spots={[]} panels={panels} />) },
  { name: 'CW', render: (s: AppSnapshot) => render(<CwCockpit snap={s} theme="dark" onWorkSpot={() => {}} spots={[]} panels={panels} />) },
]

describe.each(cockpits)('$name cockpit panadapter controls', ({ render: mount }) => {
  it('are present while the sweep cannot be placed — no feed, FIX, no start stated', () => {
    mount(snapWithMode(FIX_NORMAL))
    // The controls, found the way the operator finds them.
    expect(screen.getByLabelText('Panadapter span (sets the radio)')).toBeTruthy()
    expect(screen.getByLabelText('Panadapter position (sets the radio)')).toBeTruthy()
    // And NOTHING clickable beside the mode dropdown. The FIX window is derived — band centre, in
    // the narrowest covering span — so there is nothing for the operator to state and nothing to
    // offer them (operator, 2026-08-20: "I do NOT want to see a FIX or for that matter any text
    // appearing as clickable next to the mode dropdown").
    expect(screen.queryByRole('button', { name: /FIX/i })).toBeNull()
  })

  it('offer no FIX-start action in any position', () => {
    // Not in CENTER or CURSOR either, and not because it would be meaningless there — because the
    // window is derived in every position now, so no such control exists at all.
    for (const code of [CENTER_NORMAL, CURSOR_NORMAL, FIX_NORMAL]) {
      mount(snapWithMode(code))
      expect(screen.queryByRole('button', { name: /FIX/i })).toBeNull()
      cleanup()
    }
  })

  it('show the position the RADIO reports, not a local default', () => {
    mount(snapWithMode(CURSOR_NORMAL))
    const sel = screen.getByLabelText('Panadapter position (sets the radio)') as HTMLSelectElement
    expect(sel.value).toBe('cursor')
  })

  it('stay away entirely when the radio has no scope Nexus has read', () => {
    // No mode code = not an FT-710 with the bridge running. The audio cockpit must be unchanged.
    render(<PhoneCockpit snap={{ mycall: 'KD9TAW', radio } as unknown as AppSnapshot} theme="dark" onWorkSpot={() => {}} spots={[]} panels={panels} />)
    expect(screen.queryByLabelText('Panadapter span (sets the radio)')).toBeNull()
  })
})
