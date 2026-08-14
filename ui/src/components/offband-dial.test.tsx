// @vitest-environment jsdom
//
// TYPING AN OFF-BAND FREQUENCY INTO A MAIN DIAL MUST TUNE THERE (operator, 2026-08-13).
//
// All six cockpit hero read-outs commit through a `commitDial` of the same five lines, and every
// one of them derived a band label from the TYPED frequency and refused when the table named
// none — five with an error toast, TempoHeader in complete silence, discarding the value with no
// feedback at all. Between them that is the whole reason an operator could not reach WWV, a
// shortwave broadcaster, or any frequency in the gap between two band edges without turning the
// rig's own knob.
//
// Six cockpits, six closures, one shape: this file drives each one through the real
// CockpitHeader + FrequencyReadout, because the refusal lived in the cockpit, not in the header.
// Heavy children (canvas scopes, waterfalls, the log strip) are stubbed — nothing here is about
// them.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, fireEvent, cleanup } from '@testing-library/react'
import type { AppSnapshot } from '../types'
import { CwCockpit } from './CwCockpit'
import { PhoneCockpit } from './PhoneCockpit'
import { OperateCockpit } from './OperateCockpit'
import { RttyCockpit } from './RttyCockpit'
import { SstvView } from './SstvView'
import { TempoHeader } from './TempoHeader'
import { setFrequency } from '../api'
import { pushToast } from '../toast'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

vi.mock('../api', () => {
  const nada = () => Promise.resolve(null)
  return {
    // The dial write every assertion below reads.
    setFrequency: vi.fn(nada),
    // …and everything the six cockpits + their un-stubbed children touch on mount. A verb a
    // component imports but this factory omits is not a silent no-op: Vitest throws
    // 'No "x" export is defined on the "../api" mock' the first time it is touched.
    setRit: vi.fn(nada),
    setXit: vi.fn(nada),
    setVfo: vi.fn(nada),
    setPtt: vi.fn(nada),
    setRfPower: vi.fn(nada),
    setMicGain: vi.fn(nada),
    setNrLevel: vi.fn(nada),
    setAgc: vi.fn(nada),
    setScopeSpan: vi.fn(nada),
    setScopeRef: vi.fn(nada),
    setFlexPanSpan: vi.fn(nada),
    setFlexPanRef: vi.fn(nada),
    setSplit: vi.fn(nada),
    setRigFunc: vi.fn(nada),
    setSidebandOverride: vi.fn(nada),
    setFilterWidth: vi.fn(nada),
    setTune: vi.fn(nada),
    haltTx: vi.fn(nada),
    startQsoRecording: vi.fn(nada),
    stopQsoRecording: vi.fn(nada),
    openPanelWindow: vi.fn(nada),
    getSettings: vi.fn(() => Promise.resolve({})),
    setSettings: vi.fn(nada),
    notifyErase: vi.fn(nada),
    redecode: vi.fn(nada),
    startCq: vi.fn(nada),
    setDecodeDepth: vi.fn(nada),
    setSkipTx1: vi.fn(nada),
    pointRotatorAtCall: vi.fn(nada),
    pointRotator: vi.fn(nada),
    readRotator: vi.fn(nada),
    stopRotator: vi.fn(nada),
    getDeclination: vi.fn(nada),
    getSatTrackStatus: vi.fn(nada),
    stopSatTrack: vi.fn(nada),
    openQrzPage: vi.fn(nada),
    postSpot: vi.fn(nada),
    getSpectrumRow: vi.fn(nada),
    getMeters: vi.fn(nada),
    sendCw: vi.fn(nada),
    setCwKeyer: vi.fn(nada),
    setCwWpm: vi.fn(nada),
    stopCw: vi.fn(nada),
    cwDecode: vi.fn(nada),
    cwClear: vi.fn(nada),
    setAiCw: vi.fn(nada),
    selectPeer: vi.fn(nada),
    previewCw: vi.fn(async (t: string) => t),
    getLicensedBandPlan: vi.fn(() => Promise.resolve([])),
    getRttyState: vi.fn(nada),
    rttyArm: vi.fn(nada),
    rttySend: vi.fn(nada),
    rttySetLatched: vi.fn(nada),
    rttyType: vi.fn(nada),
    rttyStop: vi.fn(nada),
    rttyClear: vi.fn(nada),
    rttyAfcReset: vi.fn(nada),
    rttySetAuto: vi.fn(nada),
    rttyAutoCq: vi.fn(nada),
    rttyAutoAnswer: vi.fn(nada),
    rttyAutoAbort: vi.fn(nada),
    getSstvState: vi.fn(nada),
    sstvArm: vi.fn(nada),
    sstvAutoArm: vi.fn(nada),
    sstvAutoDisarm: vi.fn(nada),
    sstvSend: vi.fn(nada),
    sstvStop: vi.fn(nada),
    sstvDeleteImage: vi.fn(nada),
    setOperatingMode: vi.fn(nada),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => {
    try {
      return await action()
    } catch {
      return null
    }
  }),
}))

// Canvas-backed / structurally irrelevant children.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))

const mockSetFreq = setFrequency as unknown as ReturnType<typeof vi.fn>
const mockToast = pushToast as unknown as ReturnType<typeof vi.fn>

/** A 20 m dial with CAT up and the transmitter free — the state a typed QSY starts from. */
const snap = () =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN61',
    stations: [],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
    qso: null,
    link: { tier: 'FT8' },
    chatCq: 'off',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      sideband: 'USB',
      catOk: true,
      transmitting: false,
      tuning: false,
      txEnabled: false,
      txAllowed: true,
      txLevel: 0.5,
      rxOffsetHz: 1500,
      txOffsetHz: 1500,
      slot: 0,
      source: 'native',
      sourceLabel: 'Native',
      splitTxMhz: null,
      qsoRecording: false,
    },
  }) as unknown as AppSnapshot

const operatePanels = (): PanelLayoutApi<OperatePanelId> => ({
  layout: { v: 1, state: {}, share: {} },
  stateOf: () => 'docked' as PanelState,
  setPanelState: vi.fn(),
  shareOf: () => 1,
  setShare: vi.fn(),
  setShares: vi.fn(),
  undo: vi.fn(),
  canUndo: false,
  undoRemoves: [],
  reset: vi.fn(),
})

/** Click the cockpit's hero read-out, type `mhz`, press Enter — the operator's route in. */
function typeDial(container: HTMLElement, mhz: string) {
  const readout = container.querySelector('.ch-readout [role="button"]') as HTMLElement
  expect(readout, 'the cockpit must render an editable hero read-out').not.toBeNull()
  fireEvent.click(readout)
  const input = container.querySelector('.readout-input') as HTMLInputElement
  fireEvent.change(input, { target: { value: mhz } })
  fireEvent.keyDown(input, { key: 'Enter' })
}

/** Each cockpit under one name, with the props App gives it, reporting how it commands a QSY:
 *  CW and Phone call `setFrequency` themselves; the other four hand it to their host. */
const COCKPITS: { name: string; mount: () => { container: HTMLElement; qsy: () => unknown[][] } }[] = [
  {
    name: 'CwCockpit',
    mount: () => {
      const { container } = render(<CwCockpit snap={snap()} theme="dark" />)
      return { container, qsy: () => mockSetFreq.mock.calls }
    },
  },
  {
    name: 'PhoneCockpit',
    mount: () => {
      const { container } = render(<PhoneCockpit snap={snap()} theme="dark" phoneMode="SSB" />)
      return { container, qsy: () => mockSetFreq.mock.calls }
    },
  },
  {
    name: 'OperateCockpit',
    mount: () => {
      const onSetFrequency = vi.fn()
      const noop = () => {}
      const { container } = render(
        <OperateCockpit
          snap={snap()}
          theme="dark"
          tier="FT8"
          onTierChange={noop}
          bandPlan={[]}
          onSetFrequency={onSetFrequency}
          onSourceChange={noop}
          onTune={noop}
          onCall={noop}
          onSetTxLevel={noop}
          onSetMode={noop}
          onSetTxEven={noop}
          onSetTxCycleAuto={noop}
          onResend={noop}
          onFreetext={noop}
          onLog={noop}
          onOverrideTx={noop}
          onHaltTx={noop}
          roster={<div />}
          needByCall={new Map()}
          selectedCall={null}
          onSelect={noop}
          layoutMode="classic"
          onLayoutMode={noop}
          panels={operatePanels()}
          active={false}
        />,
      )
      return { container, qsy: () => onSetFrequency.mock.calls }
    },
  },
  {
    name: 'RttyCockpit',
    mount: () => {
      const onSetFrequency = vi.fn()
      const { container } = render(
        <RttyCockpit snap={snap()} active={false} onSetFrequency={onSetFrequency} />,
      )
      return { container, qsy: () => onSetFrequency.mock.calls }
    },
  },
  {
    name: 'SstvView',
    mount: () => {
      const onSetFrequency = vi.fn()
      const { container } = render(
        <SstvView snap={snap()} active={false} onSetFrequency={onSetFrequency} />,
      )
      return { container, qsy: () => onSetFrequency.mock.calls }
    },
  },
  {
    name: 'TempoHeader',
    mount: () => {
      const onSetFrequency = vi.fn()
      const noop = () => {}
      const { container } = render(
        <TempoHeader
          snap={snap()}
          tier="TempoFast"
          onTierChange={noop}
          bandPlan={[]}
          onSetFrequency={onSetFrequency}
          onSetTxLevel={noop}
          onToggleCqRun={noop}
          onResumeCqRun={noop}
        />,
      )
      return { container, qsy: () => onSetFrequency.mock.calls }
    },
  },
]

beforeEach(() => {
  mockSetFreq.mockClear()
  mockToast.mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe.each(COCKPITS)('$name — the hero read-out', ({ mount }) => {
  it('tunes to a typed OFF-BAND frequency, with an empty band label', () => {
    const { container, qsy } = mount()
    typeDial(container, '5.000')
    expect(qsy()).toHaveLength(1)
    // The band label is '' and that is the whole point — the engine routes a bandless dial.
    // The sideband is whatever each cockpit's own rule says; only the first two args are the
    // refusal this file is about.
    const [mhz, band] = qsy()[0]
    expect(mhz).toBe(5)
    expect(band).toBe('')
    expect(mockToast, 'off-band listening is not an error').not.toHaveBeenCalled()
  })

  it('POSITIVE CONTROL — an in-band entry is unchanged and still names its band', () => {
    const { container, qsy } = mount()
    typeDial(container, '14.250')
    expect(qsy()).toHaveLength(1)
    const [mhz, band] = qsy()[0]
    expect(mhz).toBe(14.25)
    expect(band).toBe('20m')
  })
})
