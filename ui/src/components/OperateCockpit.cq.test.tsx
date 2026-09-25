// @vitest-environment jsdom
//
// #254 — "CQ KD9WES POTA" SILENTLY SENT A PLAIN CQ.
//
// The reporter typed his directed CQ with the words the other way round. `cqDirFromText`
// accepts only WSJT-X's grammar (`CQ [TOKEN] MYCALL [GRID4]`), so it returned `undefined`
// — and `doTx(6)` read that as "fall back to a plain CQ" and called `startCq(null)`. The
// Tx6 box still showed what he typed. The radio sent something else.
//
// THE INVARIANT, and it is the transmit path's: **never transmit a message different from
// the one shown.** The operator decision (2026-09-14) is WARN-ONLY — WSJT-X's CQ grammar is
// not widened and nothing on the air is rewritten. A malformed CQ must be refused visibly,
// before anything keys, with the valid form named where we can derive it honestly.
//
// Both directions are asserted, because a refusal that also refuses the valid form is the
// same defect wearing the other hat: the good text must still reach `startCq` byte for byte.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot } from '../types'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

const startCq = vi.fn(async () => null)
const pushToast = vi.fn()

vi.mock('../api', () => ({
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  getSettings: vi.fn(async () => ({})),
  setSettings: vi.fn(async () => null),
  openPanelWindow: vi.fn(async () => null),
  notifyErase: vi.fn(async () => null),
  pointRotatorAtCall: vi.fn(async () => null),
  redecode: vi.fn(async () => null),
  startCq: (...a: unknown[]) => startCq(...(a as [])),
  startQsoRecording: vi.fn(async () => null),
  stopQsoRecording: vi.fn(async () => null),
  setSkipTx1: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
  getSatTransponder: vi.fn(async () => null),
  setSatTransponder: vi.fn(async () => null),
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
vi.mock('../toast', () => ({
  pushToast: (...a: unknown[]) => pushToast(...(a as [])),
  subscribeToasts: () => () => {},
  dismissToast: vi.fn(),
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./OperateDecodes', async (importOriginal) => {
  const real = await importOriginal<typeof import('./OperateDecodes')>()
  return { ...real, OperateDecodes: () => <div data-testid="od-pane" /> }
})

/** The reporter's own callsign, so the malformed text below is his verbatim. */
const MYCALL = 'KD9WES'

function makeSnap(): AppSnapshot {
  return {
    mycall: MYCALL,
    mygrid: 'EN61',
    stations: [],
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
    stateOf: (id) => state[id] ?? 'docked',
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

function renderCockpit() {
  const noop = () => {}
  return render(
    <OperateCockpit
      snap={makeSnap()}
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
    />,
  )
}

/** The Tx6 editable CQ box and its Tx 6 button. */
const cqField = () => screen.getByLabelText('Tx6 Call CQ (edit for a directed CQ)') as HTMLInputElement
const cqButton = () => screen.getByTitle('Call CQ (Alt+6)')

/** Type `text` into Tx6 and press Tx 6. */
function callCq(text: string) {
  fireEvent.change(cqField(), { target: { value: text } })
  expect(cqField().value, 'control: the edit took').toBe(text)
  fireEvent.click(cqButton())
}

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  startCq.mockClear()
  pushToast.mockClear()
})
afterEach(cleanup)

describe('#254 — a malformed directed CQ is refused, never quietly replaced', () => {
  it('"CQ KD9WES POTA" does not key, and says so', async () => {
    renderCockpit()
    callCq(`CQ ${MYCALL} POTA`)

    await waitFor(() => expect(pushToast).toHaveBeenCalled())
    expect(
      startCq,
      'the words are in the wrong order — nothing may go on the air',
    ).not.toHaveBeenCalled()

    const [body, kind] = pushToast.mock.calls[0] as [string, string]
    expect(kind, 'a refusal is an error, not a note').toBe('error')
    expect(
      body,
      'the refusal names the form that would work, so the operator can retype it',
    ).toContain(`CQ POTA ${MYCALL}`)

    // …and the box still shows what the operator typed. Nothing was rewritten under them.
    expect(cqField().value).toBe(`CQ ${MYCALL} POTA`)
  })

  it('a CQ naming somebody else is refused too, with no suggestion invented', async () => {
    renderCockpit()
    callCq('CQ POTA W1ABC')

    await waitFor(() => expect(pushToast).toHaveBeenCalled())
    expect(startCq).not.toHaveBeenCalled()
    const [body] = pushToast.mock.calls[0] as [string]
    expect(
      body,
      'we must never propose a message the operator did not type',
    ).not.toContain('W1ABC')
  })

  // THE POSITIVE CONTROL. A guard that refuses everything is not a guard.
  it('the valid directed form still calls CQ, byte for byte', async () => {
    renderCockpit()
    callCq(`CQ POTA ${MYCALL}`)

    await waitFor(() => expect(startCq).toHaveBeenCalledTimes(1))
    expect(startCq).toHaveBeenCalledWith('POTA')
    expect(pushToast, 'a valid CQ raises nothing').not.toHaveBeenCalled()
  })

  it('a plain CQ still calls CQ', async () => {
    renderCockpit()
    callCq(`CQ ${MYCALL} EN61`)

    await waitFor(() => expect(startCq).toHaveBeenCalledTimes(1))
    expect(startCq).toHaveBeenCalledWith(null)
    expect(pushToast).not.toHaveBeenCalled()
  })
})
