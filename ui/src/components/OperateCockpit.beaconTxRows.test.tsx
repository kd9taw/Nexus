// @vitest-environment jsdom
//
// TX1-TX5 WORK NOBODY ON A BEACON OR A RECEIVE-ONLY TIER.
//
// A Tx button forces its row as the next over to the DX Call and arms TX. On WSPR or FST4W the
// armed TX is the beacon schedule, so one press with a call in the box put the beacon on the air
// with no contact behind it; on a receive-only tier there is no over to send. There the five
// rows are disabled, Alt+1..5 does nothing, and the pop-out (the same cockpit) follows. The
// station refuses the press as well. CONTROL: on FT8 the rows are live and Alt+1 presses Tx 1.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, QsoStatus, Tier } from '../types'
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

function renderCockpit(tier: Tier, onOverrideTx: (call: string, grid: string | null, text: string) => void) {
  const noop = () => {}
  const snap = { ...makeSnap(null, -12), link: { tier } } as AppSnapshot
  return render(
    <OperateCockpit
      snap={snap}
      theme="dark"
      tier={tier}
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
      onOverrideTx={onOverrideTx}
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
    />,
  )
}

/** Tx 1..Tx 5, with W1ABC typed into the DX Call box. */
function txRows(): HTMLButtonElement[] {
  fireEvent.change(screen.getByLabelText('DX callsign'), { target: { value: 'W1ABC' } })
  return [1, 2, 3, 4, 5].map((n) => screen.getByRole('button', { name: `Tx ${n}` }) as HTMLButtonElement)
}

/** Alt+n, pressed with the focus on the page, not in a box. */
function altDigit(n: number) {
  fireEvent.keyDown(window, { key: String(n), code: `Digit${n}`, altKey: true })
}

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('Tx1-Tx5 on a tier that answers no station', () => {
  // JS8 is not here: it transmits (the modes crate's `Js8Mode`), so it is not a receive-only
  // tier, and it works stations from its own screen. App never shows this cockpit on the JS8
  // tier: entering the FT screen puts the tier back on one the screen owns.
  for (const tier of ['WSPR', 'FST4W'] as Tier[]) {
    it(`${tier}: the five rows are disabled and Alt+1..5 presses none`, () => {
      const pressed = vi.fn()
      renderCockpit(tier, pressed)
      const rows = txRows()
      for (const b of rows) expect(b.disabled, `${b.textContent} on ${tier}`).toBe(true)
      for (const n of [1, 2, 3, 4, 5]) altDigit(n)
      expect(pressed).not.toHaveBeenCalled()
    })
  }

  it('CONTROL FT8: the rows are live and Alt+1 presses Tx 1', () => {
    const pressed = vi.fn()
    renderCockpit('FT8', pressed)
    // The same rows, call and texts as above: only the tier differs.
    const rows = txRows()
    for (const b of rows) expect(b.disabled, `${b.textContent} on FT8`).toBe(false)
    altDigit(1)
    expect(pressed).toHaveBeenCalledWith('W1ABC', null, 'W1ABC KD9TAW EN61')
  })
})
