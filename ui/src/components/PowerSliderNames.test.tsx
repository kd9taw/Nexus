// @vitest-environment jsdom
//
// ONE WORD FOR EACH THING A POWER SLIDER CAN MOVE. The cockpit headers carry a slider for one of two
// different settings, and the words did not say which: Operate's "Pwr" and the Settings "Tx Power"
// moved the AUDIO DRIVE (`set_tx_level`, the sound-card level into the radio), while SSTV's "Power",
// Phone's "Power" (and RTTY's and PSK's) moved the radio's RF OUTPUT over CAT (`set_rf_power`). So
// "Pwr" on one screen and "Power" on the next were different controls. The rule now: the audio drive
// is "Drive" everywhere (as JS8 already said), the radio's output is "RF power" everywhere.
//
// Behaviour, not strings: each cockpit is RENDERED, its header slider is found by its accessible
// name, moved, and the test reads which of the two settings it actually reached.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, act } from '@testing-library/react'
import * as api from '../api'
import { RttyCockpit } from './RttyCockpit'
import { PskCockpit } from './PskCockpit'
import { SstvView } from './SstvView'
import { PhoneCockpit } from './PhoneCockpit'
import { Js8Cockpit } from './Js8Cockpit'
import { OperateCockpit } from './OperateCockpit'
import { TempoHeader } from './TempoHeader'
import type { PanelLayoutApi } from '../features/panelState'
import type { AppSnapshot, Js8State, PskState, RadioStatus, RttyState, SstvState } from '../types'

const rttyState = {
  armed: true, afcHz: 0, afcLocked: false, text: '', charConf: [], baud: 45.45, shiftHz: 170, markHz: 2125,
  spaceHz: 2295, sending: false, latched: false, backend: 'afsk', keyerError: null, auto: false, seqState: 'idle',
  peer: null, peerExchange: [], heardCq: null,
} as unknown as RttyState
const pskState = { armed: true, afcHz: 0, signal: false, centerHz: 1000, text: '', charConf: [], sending: false, latched: false, keyerError: null } as unknown as PskState
const js8State = {
  speed: 'normal', rxSpeeds: 15, txEnabled: true, sending: false, hbOn: false, hbNextAtMs: null, hbIntervalMin: 0,
  cqOn: false, cqNextAtMs: null, cqIntervalMin: 0, autoreply: true, relay: true, hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false }, idleMinutes: 0, idleLimitMin: 60,
  idleTripped: false, activity: [], stations: [], inbox: [], queue: [], pendingReply: null, lastError: null,
} as unknown as Js8State
const sstvState = {
  armed: false, mode: null, linesDone: 0, linesTotal: 0, previewRgbBase64: null, previewWidth: 0, previewHeight: 0,
  hedrShiftHz: 0, gallery: [],
  health: { armed: false, audioPeak: 0, lastAudioUnix: null, drains: 0, visSeen: 0, lastVisUnix: null, unknownVis: 0, lastUnknownVisCode: null, lastUnknownVisUnix: null, images: 0, lastImageUnix: null },
  sending: false, txMode: null, txProgress: 0, txElapsedSecs: 0, txTotalSecs: 0,
} as unknown as SstvState

// The real module with every function stubbed; the entries below are the ones whose SHAPE a host
// reads on mount.
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    appVersion: vi.fn(async () => '0.0.0'),
    getVoiceMessages: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
    selectPeer: vi.fn(async () => null),
    pointRotatorAtCall: vi.fn(async () => 0),
    readRotator: vi.fn(async () => null),
    getDeclination: vi.fn(async () => 0),
    getSatTrackStatus: vi.fn(async () => null),
    getSatTransponder: vi.fn(async () => null),
    getLicensedBandPlan: vi.fn(async () => []),
    getSpectrumRow: vi.fn(async () => null),
    getRttyState: vi.fn(async () => rttyState),
    rttyArm: vi.fn(async () => rttyState),
    rttyAutoArm: vi.fn(async () => rttyState),
    getPskState: vi.fn(async () => pskState),
    pskArm: vi.fn(async () => pskState),
    pskAutoArm: vi.fn(async () => pskState),
    getJs8State: vi.fn(async () => js8State),
    js8Enter: vi.fn(async () => js8State),
    js8Arm: vi.fn(async () => js8State),
    getSstvState: vi.fn(async () => sstvState),
    sstvArm: vi.fn(async () => sstvState),
    sstvAutoArm: vi.fn(async () => sstvState),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()) }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

const noop = () => {}
function panels<P extends string>(): PanelLayoutApi<P> {
  return {
    layout: { v: 1, state: {}, share: {} }, stateOf: () => 'docked', setPanelState: noop, shareOf: () => 1, setShare: noop,
    setShares: noop, undo: noop, canUndo: false, undoRemoves: [], reset: noop,
  }
}
const radio = {
  dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB', rigConfirmed: true,
  transmitting: false, tuning: false, txEnabled: true, txAllowed: true, qsoRecording: false, rfPower: 0.5, micGain: null,
  nrLevel: 0.3, agc: 'fast', nb: true, nr: true, notch: null, comp: null, vox: null, filterWidthHz: 500, splitTxMhz: null,
  smeterDb: null, cwWpm: 22, cwKeyer: 'cat', phoneSegLo: null, phoneSegHi: null, nextSlotMs: 5000, slot: 0,
  rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, txEven: true, txCycleAuto: true, txBusyReason: null, rigKeyed: false,
  dtSec: 0, clockOffsetMs: 0, source: 'native', sourceLabel: 'Native', atu: true,
} as unknown as RadioStatus
const snap = {
  mycall: 'KD9TAW', mygrid: 'EN61', radio, stations: [], recentDecodes: [], conversations: [], highlights: [],
  harqRescues: 0, clearTick: 0, qso: null, link: { tier: 'FT8' },
} as unknown as AppSnapshot

/** The drive a screen reaches through its own prop rather than the api (Operate, Tempo). */
const driveProp = vi.fn()
const HOSTS: { name: string; mount: () => void }[] = [
  { name: 'RTTY', mount: () => render(<RttyCockpit snap={snap} panels={panels()} onSetTxEnabled={noop} />) },
  { name: 'PSK', mount: () => render(<PskCockpit snap={snap} panels={panels()} onSetTxEnabled={noop} />) },
  { name: 'SSTV', mount: () => render(<SstvView snap={snap} panels={panels()} onSetTxEnabled={noop} />) },
  { name: 'Phone', mount: () => render(<PhoneCockpit snap={snap} theme="dark" onWorkSpot={noop} spots={[]} panels={panels()} />) },
  { name: 'JS8', mount: () => render(<Js8Cockpit snap={snap} panels={panels()} onSetTxEnabled={noop} />) },
  {
    name: 'Operate',
    mount: () =>
      render(
        <OperateCockpit
          snap={snap} theme="dark" tier="FT8" fdActive={false} fdRuleset={null} onTierChange={noop} bandPlan={[]}
          onSetFrequency={noop} onSourceChange={noop} onTune={noop} onCall={noop} onSetTxLevel={driveProp} onSetMode={noop}
          onSetTxEven={noop} onSetTxCycleAuto={noop} onResend={noop} onFreetext={noop} onLog={noop} onOverrideTx={noop}
          onHaltTx={noop} roster={<div />} needByCall={new Map()} selectedCall={null} onSelect={noop}
          layoutMode="classic" onLayoutMode={noop} panels={panels()} active={false}
        />,
      ),
  },
  {
    name: 'Tempo',
    mount: () =>
      render(
        <TempoHeader snap={snap} tier="TempoFast" onTierChange={noop} bandPlan={[]} onSetFrequency={noop} onSetTxLevel={driveProp}
          onToggleCqRun={noop} onResumeCqRun={noop} onSetTxEnabled={noop} onSetTune={noop} onAtuTune={noop} onHaltTx={noop} onSetHoldTxFreq={noop} />,
      ),
  },
]

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

/** What the screen's header power slider is called, and which setting moving it reached. */
async function probe(mount: () => void): Promise<{ name: string; reached: string[] }> {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  mount()
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  const slider = document.querySelector('.cockpit-pwr input[type="range"]') as HTMLInputElement | null
  if (!slider) throw new Error('no header power slider rendered')
  vi.clearAllMocks()
  fireEvent.change(slider, { target: { value: slider.max === '100' ? '40' : '0.4' } })
  await act(async () => {
    await Promise.resolve()
  })
  const reached: string[] = []
  if (vi.mocked(api.setRfPower).mock.calls.length) reached.push('RF output')
  if (vi.mocked(api.setTxLevel).mock.calls.length || driveProp.mock.calls.length) reached.push('audio drive')
  return { name: slider.getAttribute('aria-label') ?? '', reached }
}

describe("every header power slider is named for what it moves", () => {
  it.each(HOSTS.map((h) => [h.name, h] as const))('%s', async (_n, h) => {
    const { name, reached } = await probe(h.mount)
    expect(reached, `${h.name}: the slider must move exactly one of the two settings`).toHaveLength(1)
    const want = reached[0] === 'audio drive' ? 'Drive' : 'RF power'
    expect(name, `${h.name}: a slider that moves the ${reached[0]} is called`).toBe(want)
    // And the same word is what the operator reads beside it.
    expect(screen.getByText(want, { selector: '.cockpit-pwr > span' })).toBeTruthy()
  })
})
