// @vitest-environment jsdom
//
// A LOGGER'S UDP FREETEXT SETS TX5, AS WSJT-X'S DOES.
//
// JTAlert and GridTracker send WSJT-X a FreeText: its text goes into Tx5 whether or not it is sent
// (`ui->tx5->setCurrentText`, WSJT-X 3.0.2 `widgets/mainwindow.cpp:692-709`), and an empty one
// without "send" clears Tx5. The station takes it and says, in the snapshot, what Tx5 now holds and
// why the over did not go out when it did not. The cockpit applies each FreeText once, by its seq,
// and never one sent before it mounted.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, QrzLookup } from '../types'
import { OPERATE_PANELS, panelStateIn } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

const toasts = vi.hoisted(() => vi.fn())
vi.mock('../toast', async (importOriginal) => {
  const real = await importOriginal<typeof import('../toast')>()
  return { ...real, pushToast: (...a: Parameters<typeof real.pushToast>) => (toasts(...a), 0) }
})

const resolved: QrzLookup = {
  call: 'W1ABC',
  name: 'Alice Example',
  nickname: null,
  qth: 'Hartford, CT',
  grid: 'FN31',
  state: 'CT',
  country: 'United States',
  dxcc: 291,
  cqZone: 5,
  ituZone: 8,
  image: null,
}

const redecode = vi.fn(async () => null)

vi.mock('../api', () => ({
  qrzLookup: vi.fn(async () => resolved),
  resolveEntity: vi.fn(async () => 'United States'),
  getSettings: vi.fn(() => Promise.resolve({})),
  setSettings: vi.fn(async () => null),
  openPanelWindow: vi.fn(async () => null),
  notifyErase: vi.fn(async () => null),
  pointRotatorAtCall: vi.fn(async () => null),
  redecode: (...a: unknown[]) => redecode(...(a as [])),
  startCq: vi.fn(async () => null),
  startQsoRecording: vi.fn(async () => null),
  stopQsoRecording: vi.fn(async () => null),
  setSkipTx1: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
  getSatTransponder: vi.fn(async () => null),
  readRotator: vi.fn(async () => null),
  stopRotator: vi.fn(async () => null),
  stopSatTrack: vi.fn(async () => null),
  openQrzPage: vi.fn(async () => null),
  postSpot: vi.fn(async () => null),
  setFrequency: vi.fn(async () => null),
  setRit: vi.fn(async () => null),
  setXit: vi.fn(async () => null),
  setVfo: vi.fn(async () => null),
  getSpectrumRow: vi.fn(async () => null),
  setDecodeDepth: vi.fn(async () => null),
  atuTune: vi.fn(async () => null),
  setMsk144Period: vi.fn(async () => null),
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./OperateDecodes', async (importOriginal) => {
  const real = await importOriginal<typeof import('./OperateDecodes')>()
  return { ...real, OperateDecodes: () => <div data-testid="od-pane" /> }
})

function makeSnap(udpFreeText?: AppSnapshot['udpFreeText']): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN61',
    stations: [
      {
        call: 'W1ABC',
        grid: 'FN42',
        snr: -7,
        lastHeardSlot: 0,
        heardCount: 3,
        presence: 'live',
        worked: true,
        country: 'United States',
      },
    ],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
    udpFreeText,
    qso: null,
    link: { tier: 'FT8' },
    radio: {
      dialMhz: 14.074,
      band: '20m',
      sideband: 'USB',
      slot: 0,
      source: 'native',
      sourceLabel: 'Native',
      nextSlotMs: 5000,
      rxOffsetHz: 1500,
      txOffsetHz: 1500,
      txLevel: 0.5,
      txEven: true,
      txCycleAuto: true,
      txEnabled: false,
      txAllowed: true,
      transmitting: false,
      tuning: false,
      atu: true,
      qsoRecording: false,
      catOk: true,
      splitTxMhz: null,
    },
  } as unknown as AppSnapshot
}

function panelsApi(): PanelLayoutApi<OperatePanelId> {
  const state: Partial<Record<OperatePanelId, PanelState>> = {}
  return {
    layout: { v: 1, state, share: {} },
    stateOf: (id) => panelStateIn(OPERATE_PANELS, { v: 1, state, share: {} }, id),
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    undo: vi.fn(),
    canUndo: false,
    undoRemoves: [],
    reset: vi.fn(),
  }
}

function renderCockpit(udpFreeText?: AppSnapshot['udpFreeText']) {
  const noop = () => {}
  const el = (u?: AppSnapshot['udpFreeText']) => (
    <OperateCockpit
      snap={makeSnap(u)}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
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
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={noop}
      onClearSelection={noop}
      layoutMode="classic"
      onLayoutMode={noop}
      panels={panelsApi()}
      active
    />
  )
  const view = render(el(udpFreeText))
  /** The next snapshot, to the same cockpit. */
  const next = (u?: AppSnapshot['udpFreeText']) => view.rerender(el(u))
  return { ...view, next }
}

const tx5 = () => (screen.getByLabelText('Tx5 free text') as HTMLInputElement).value

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  toasts.mockClear()
})
afterEach(cleanup)

describe("a logger's UDP FreeText in Tx5", () => {
  it('fills Tx5 once per FreeText, and an empty one clears it', () => {
    const { next } = renderCockpit()
    expect(tx5(), 'control: Tx5 before any FreeText').toBe('')
    next({ seq: 1, fill: 'TNX 73', refused: null })
    expect(tx5()).toBe('TNX 73')
    // The same FreeText again (the next snapshot) is not applied twice: an edit since stands.
    next({ seq: 1, fill: 'GL', refused: null })
    expect(tx5(), 'same seq').toBe('TNX 73')
    next({ seq: 2, fill: 'GL', refused: null })
    expect(tx5(), 'the next FreeText').toBe('GL')
    next({ seq: 3, fill: '', refused: null })
    expect(tx5(), 'an empty one clears').toBe('')
    expect(toasts).not.toHaveBeenCalled()
  })

  it('leaves Tx5 alone for a "send" with no text, and says why it did not go out', () => {
    const { next } = renderCockpit()
    next({ seq: 1, fill: 'TNX 73', refused: null })
    next({ seq: 2, fill: null, refused: 'TX is off' })
    expect(tx5()).toBe('TNX 73')
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts).toHaveBeenCalledWith("A logger's free text did not go out: TX is off", 'error')
  })

  it('never applies a FreeText sent before the cockpit mounted', () => {
    const { next } = renderCockpit({ seq: 4, fill: 'TNX 73', refused: 'TX is off' })
    expect(tx5()).toBe('')
    expect(toasts).not.toHaveBeenCalled()
    next({ seq: 5, fill: 'GL', refused: null })
    expect(tx5(), 'control: the next one is').toBe('GL')
  })
})
