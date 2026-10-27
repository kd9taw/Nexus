// @vitest-environment jsdom
//
// THE TX PANEL'S "NEXT" DOT FOLLOWS THE OVER THE STATION WILL SEND.
//
// The dot marks which of the six rows goes out next. The station names that over in
// `snap.qso.txNow`; the rows are regenerated here from the DX Call box, the heard SNR and the
// operator's Tx5. When the two texts differed in anything but the step — the station holding the
// report it sent while the DX is heard again at another SNR, an edited Tx5, a different call in
// the DX box — the dot found no row and stayed where it was while the QSO went on.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, QsoStatus } from '../types'
import { OPERATE_PANELS, panelStateIn } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  getSettings: vi.fn(() => Promise.resolve({})),
  setSettings: vi.fn(async () => null),
  openPanelWindow: vi.fn(async () => null),
  notifyErase: vi.fn(async () => null),
  pointRotatorAtCall: vi.fn(async () => null),
  redecode: vi.fn(async () => null),
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

/** One snapshot: the sequencer's status, and the SNR the roster last heard W1ABC at. */
function makeSnap(qso: Partial<QsoStatus> | null, heardSnr: number): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN61',
    stations: [
      {
        call: 'W1ABC',
        grid: 'FN42',
        snr: heardSnr,
        lastHeardSlot: 0,
        heardCount: 3,
        presence: 'live',
        worked: false,
        country: 'United States',
      },
    ],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
    qso: qso && { running: true, cqRunning: false, rxReport: null, dxcall: null, txNow: null, ...qso },
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
      txEnabled: true,
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

function renderCockpit(first: AppSnapshot) {
  const noop = () => {}
  const el = (snap: AppSnapshot) => (
    <OperateCockpit
      snap={snap}
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
  const view = render(el(first))
  /** The next snapshot, to the same cockpit. */
  const next = (snap: AppSnapshot) => view.rerender(el(snap))
  return { ...view, next }
}

/** The Tx row (1–6) the dot is on, or null when it is on none. */
function dotRow(): number | null {
  const rows = Array.from(document.querySelectorAll('.tx-panel .txp-row'))
  expect(rows, 'the six Tx rows render').toHaveLength(6)
  const on = rows.findIndex((r) => r.querySelector('.txp-dot.on'))
  return on >= 0 ? on + 1 : null
}

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('the Tx panel next-dot', () => {
  it('stays on Tx 2 while the station holds its report and the DX is heard again at another SNR', () => {
    const { next } = renderCockpit(makeSnap({ state: 'CallingCq', cqRunning: true, txNow: 'CQ KD9TAW EN61' }, -7))
    expect(dotRow(), 'control: calling CQ').toBe(6)
    // W1ABC answers at -12; the station sends that report.
    next(makeSnap({ state: 'AwaitRoger', dxcall: 'W1ABC', txNow: 'W1ABC KD9TAW -12' }, -12))
    expect(dotRow(), 'control: the report matches the generated Tx 2').toBe(2)
    // W1ABC did not copy and calls again, now heard at -9. The over the station repeats still
    // carries the -12 it sent.
    next(makeSnap({ state: 'AwaitRoger', dxcall: 'W1ABC', txNow: 'W1ABC KD9TAW -12' }, -9))
    expect(dotRow(), 'the repeated report').toBe(2)
    // W1ABC rogers: RR73 goes out.
    next(makeSnap({ state: 'Confirming', dxcall: 'W1ABC', txNow: 'W1ABC KD9TAW RR73' }, -9))
    expect(dotRow(), 'the roger').toBe(4)
  })

  it('moves to Tx 3 when the roger carries a report the generated row does not', () => {
    // Answering W1ABC's CQ; its report decode was heard at -14, a later decode at -11.
    const { next } = renderCockpit(makeSnap({ state: 'AwaitReport', dxcall: 'W1ABC', txNow: 'W1ABC KD9TAW EN61' }, -14))
    expect(dotRow(), 'control: our grid').toBe(1)
    // The operator presses Tx 1 to send the grid again: the row they picked is remembered.
    fireEvent.click(screen.getByRole('button', { name: 'Tx 1' }))
    expect(dotRow(), 'control: the pick').toBe(1)
    next(makeSnap({ state: 'AwaitRr73', dxcall: 'W1ABC', txNow: 'W1ABC KD9TAW R-14' }, -11))
    expect(dotRow(), 'our roger and report').toBe(3)
  })

  it('moves to Tx 5 for the closing 73 when the operator has edited Tx5', () => {
    const { next } = renderCockpit(makeSnap({ state: 'AwaitRr73', dxcall: 'W1ABC', txNow: 'W1ABC KD9TAW R-12' }, -12))
    expect(dotRow(), 'control: our roger').toBe(3)
    fireEvent.change(screen.getByLabelText('Tx5 free text'), { target: { value: 'TNX 73 GL' } })
    next(makeSnap({ state: 'Done', dxcall: 'W1ABC', txNow: 'W1ABC KD9TAW 73' }, -12))
    expect(dotRow(), 'the station sends its 73').toBe(5)
  })
})
