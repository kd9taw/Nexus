// @vitest-environment jsdom
//
// A BAND CHANGE CLEARS THE FT DX CALL (operator report, 2026-10-26: "When changing bands,
// the callsign field should clear. Especially in FT. Currently, the targeted system continues to
// be loaded in the Std Msgs and tracked on the TX line."). The engine ends the contact on a band
// change by any route and bumps `bandChangeTick`; the cockpit clears DX Call, DX Grid and Tx5 on
// the bump, so Tx1–Tx5 are those of no DX station. WSJT-X keeps the DX call there; Nexus clears
// it, by the operator's ruling. A contact the engine has ALREADY started on the new band (a spot
// clicked on another band QSYs and calls in one go) stays.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, QrzLookup, Tier } from '../types'
import { OPERATE_PANELS, panelStateIn } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

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
const setDxBox = vi.fn(async (_call: string, _grid: string) => undefined)

vi.mock('../api', () => ({
  setDxBox: (call: string, grid: string) => setDxBox(call, grid),
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

function makeSnap(): AppSnapshot {
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

function renderCockpit(tier: Tier = 'FT8', onClearSelection: () => void = () => {}) {
  const noop = () => {}
  const el = (snap: AppSnapshot) => (
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
      onOverrideTx={noop}
      onHaltTx={noop}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={noop}
      onClearSelection={onClearSelection}
      layoutMode="classic"
      onLayoutMode={noop}
      panels={panelsApi()}
      active
    />
  )
  const view = render(el({ ...makeSnap(), bandChangeTick: 3 }))
  const update = (patch: Partial<AppSnapshot>) => view.rerender(el({ ...makeSnap(), bandChangeTick: 3, ...patch }))
  return { ...view, update }
}

const dxCall = () => screen.getByLabelText('DX callsign') as HTMLInputElement
const dxGrid = () => screen.getByLabelText('DX grid locator') as HTMLInputElement

/** The operator has K1ABC loaded, as a decode click or a typed call leaves it. */
function loadK1abc() {
  fireEvent.change(dxCall(), { target: { value: 'K1ABC' } })
  fireEvent.change(dxGrid(), { target: { value: 'FN42' } })
  expect(dxCall().value, 'control: the call is in the box').toBe('K1ABC')
}

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('a band change clears the DX call in every FT mode', () => {
  // Operator ruling, 2026-10-26: FT2, Q65, MSK144, FST4 and JT65 behave exactly like FT8 and FT4.
  it.each<Tier>(['FT8', 'FT4', 'FT2', 'Q65', 'MSK144', 'FST4', 'JT65'])('%s: DX Call and DX Grid empty on a band change', (tier) => {
    const { update } = renderCockpit(tier)
    loadK1abc()
    update({ bandChangeTick: 4 })
    expect(dxCall().value).toBe('')
    expect(dxGrid().value).toBe('')
  })

  it('a snapshot with no band change keeps the call', () => {
    const { update } = renderCockpit()
    loadK1abc()
    update({ radio: { ...makeSnap().radio, dialMhz: 14.08 } })
    expect(dxCall().value).toBe('K1ABC')
    expect(dxGrid().value).toBe('FN42')
  })

  // Contest FT mode: the engine ends the contest partner on a band change too (its snapshot
  // carries no `qso`), so the empty DX box is what the station holds.
  it('in a contest the DX call clears, as the contest station ends its partner', () => {
    const { update } = renderCockpit('FT8')
    loadK1abc()
    update({
      bandChangeTick: 4,
      fieldDay: { running: false, log: [] } as unknown as AppSnapshot['fieldDay'],
    })
    expect(dxCall().value).toBe('')
    expect(dxGrid().value).toBe('')
  })

  it('a contact the engine already started on the new band stays', () => {
    const onClearSelection = vi.fn()
    const { update } = renderCockpit('FT8', onClearSelection)
    loadK1abc()
    update({
      bandChangeTick: 4,
      qso: { dxcall: 'K9NEW', dxgrid: 'EN52', state: 'AwaitReport', cqRunning: false } as unknown as AppSnapshot['qso'],
    })
    expect(dxCall().value).toBe('K9NEW')
    expect(dxGrid().value).toBe('EN52')
    // …and the new station's selection and card are left alone: nothing was cleared.
    expect(onClearSelection).not.toHaveBeenCalled()
  })
})

// WSJT-X gives its decoder the DX call whenever the box holds one, in a QSO or not; the station's
// JTDX-style FT8 decoder gets it the same way, every time the boxes change, and never on mount.
describe('the DX boxes reach the station', () => {
  it('sends the call and grid as the operator loads them, and the empty boxes a band change leaves', () => {
    setDxBox.mockClear()
    const { update } = renderCockpit('FT8')
    expect(setDxBox, 'nothing on mount').not.toHaveBeenCalled()
    loadK1abc()
    expect(setDxBox).toHaveBeenLastCalledWith('K1ABC', 'FN42')
    update({ bandChangeTick: 4 })
    expect(setDxBox).toHaveBeenLastCalledWith('', '')
  })
})
