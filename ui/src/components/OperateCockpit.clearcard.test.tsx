// @vitest-environment jsdom
//
// #204 (KR4FQG) — CLEARING THE CALLSIGN CARD, AND F4 WHILE TYPING.
//
// The reporter was told "Clear on the callsign block is F4 today", pressed F4, and nothing
// happened. Both halves of that were wrong, and they are independent defects:
//
//  1. F4 COULD NOT CLEAR THE CARD. The card renders `selectedCall || snap.qso.dxcall`.
//     `selectedCall` is App's `activePeer` — BACKEND state, cleared only by `clear_peer` or by
//     archiving — and `snap.qso.dxcall` is the sequencer's. `clearDx` set `dxCall`, `dxGrid`,
//     `tx5` and `localNext` and touched NEITHER, so no control anywhere put the card back to
//     empty. Not "F4 was bound to the wrong thing": there was nothing to bind it to.
//  2. F4 WAS DEAD IN ANY TEXT FIELD. The cockpit's keydown listener returned early on
//     INPUT/TEXTAREA/SELECT before reaching the F4 arm, and Escape was the only key hoisted
//     above that guard. WSJT-X handles F4 in `MainWindow::keyPressEvent`, so a focused
//     QLineEdit never swallows it — the one moment an operator reaches for F4 is mid-typing,
//     which is exactly the moment ours ignored it.
//
// The assertions below are written so that either half regressing alone goes red, and each
// carries the negative it needs: a key that must NOT clear (Alt+F4, the platform's
// close-window gesture), and a guard that must STILL hold for the keys that were never
// hoisted (F6).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, QrzLookup } from '../types'
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

vi.mock('../api', () => ({
  getLog: vi.fn(async () => []),
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

function renderCockpit(onClearSelection: () => void) {
  const noop = () => {}
  const el = (selectedCall: string | null) => (
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
      selectedCall={selectedCall}
      onSelect={noop}
      onClearSelection={onClearSelection}
      layoutMode="classic"
      onLayoutMode={noop}
      panels={panelsApi()}
      active
    />
  )
  const view = render(el('W1ABC'))
  /** Re-render the SAME instance with a different selection — the dismissal is scoped to a
   *  call, so this is how "the card comes back on its own" is observable at all. */
  const reselect = (call: string | null) => view.rerender(el(call))
  return { ...view, reselect }
}

const cardEl = () => document.querySelector('.recall-card')
/** Any INPUT the cockpit renders unconditionally — the Escape suite uses this same one. */
const anInput = () => screen.getByLabelText('Rx offset in Hz')

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  redecode.mockClear()
})
afterEach(cleanup)

describe('#204 — something clears the callsign card', () => {
  it('F4 empties the card and clears the app-wide selection', async () => {
    const onClearSelection = vi.fn()
    renderCockpit(onClearSelection)
    await waitFor(() => expect(cardEl(), 'control: the card is up to begin with').not.toBeNull())

    fireEvent.keyDown(document.body, { key: 'F4' })

    // The half this component owns: the card is gone from the screen.
    await waitFor(() => expect(cardEl()).toBeNull())
    // The half it does not: `selectedCall` is backend state, so the clear has to round-trip.
    // Without this the card would come straight back on the next snapshot.
    expect(onClearSelection).toHaveBeenCalledTimes(1)
  })

  it('the card comes back for a DIFFERENT station without anything resetting the clear', async () => {
    const { reselect } = renderCockpit(vi.fn())
    await waitFor(() => expect(cardEl()).not.toBeNull())
    fireEvent.keyDown(document.body, { key: 'F4' })
    await waitFor(() => expect(cardEl()).toBeNull())

    // A dismissal scoped to a CALL needs no reset path — and this is the assertion that
    // would fail on a plain boolean "hidden" flag, which is the shape it is not.
    reselect('K9XYZ')
    await waitFor(() => expect(cardEl(), 'a new station must show its card').not.toBeNull())
  })
})

describe('#204 — F4 fires while the operator is typing, as WSJT-X does', () => {
  it('F4 works with focus in a text field', async () => {
    const onClearSelection = vi.fn()
    renderCockpit(onClearSelection)
    await waitFor(() => expect(cardEl()).not.toBeNull())

    fireEvent.keyDown(anInput(), { key: 'F4' })

    expect(onClearSelection, 'F4 must not be swallowed by a focused input').toHaveBeenCalledTimes(1)
    await waitFor(() => expect(cardEl()).toBeNull())
  })

  it('Alt+F4 is NOT answered — it is the platform close-window gesture', async () => {
    const onClearSelection = vi.fn()
    renderCockpit(onClearSelection)
    await waitFor(() => expect(cardEl()).not.toBeNull())

    fireEvent.keyDown(anInput(), { key: 'F4', altKey: true })

    expect(onClearSelection).not.toHaveBeenCalled()
    expect(cardEl(), 'the card must survive a window-close keystroke').not.toBeNull()
  })

  it('the typing guard still holds for the keys that were NOT hoisted', () => {
    // The control on the change: F4 moved above the guard, F6 did not. If this goes green
    // for the wrong reason — the guard deleted rather than F4 lifted past it — the whole
    // point of hoisting one key is gone.
    renderCockpit(vi.fn())
    fireEvent.keyDown(anInput(), { key: 'F6' })
    expect(redecode, 'F6 must stay disarmed while typing').not.toHaveBeenCalled()
    // …and it is genuinely wired, so the negative above is not just a dead key.
    fireEvent.keyDown(document.body, { key: 'F6' })
    expect(redecode).toHaveBeenCalledTimes(1)
  })
})
