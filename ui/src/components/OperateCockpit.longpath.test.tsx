// @vitest-environment jsdom
//
// #338's LONG-PATH BEAM, AT THE HOST. `RotorStrip.longpath.test.tsx` pins the strip's half: LP
// calls `onPointAt(call, true)` and → CALL calls `onPointAt(call, false)`. This is the other half.
// OperateCockpit wired `onPointAt={(call) => pointRotatorAtCall(call)…}`, a one-argument handler
// TypeScript accepts for the two-argument prop, so the flag never reached the backend and LP
// turned the beam the SHORT way while its tooltip promised the reciprocal. Nothing said so: the
// toast reported a heading with no path.
//
// The strip is REAL here. The strip's own test used a spy for `onPointAt`, and a spy cannot see
// what the host does with the second argument. That gap is how the drop went unseen.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import { pointRotatorAtCall } from '../api'
import { t } from '../i18n'
import type { AppSnapshot } from '../types'
import { OPERATE_PANELS, panelStateIn } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

// THE BUDGET (2026-10-09). The slowest case here, "LP asks the backend for the long path, and the toast…", takes
// 0.36 s and 0.44 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const pushToast = vi.fn()

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (CwCockpit.density's pattern): a hand-kept list
  // throws on mount the day the cockpit calls something new, which would read as this bug.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    // A rotator configured and answering, so the strip draws its beam buttons at all.
    getSettings: vi.fn(async () => ({ rotatorModel: 603, rotatorHost: '127.0.0.1' })),
    readRotator: vi.fn(async () => 212),
    // What the command answers: the bearing, and what it was taken to (here the entity centre).
    pointRotatorAtCall: vi.fn(async () => ({ bearing: 47, to: 'country', grid: null, country: 'Japan' })),
  }
})
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

const point = vi.mocked(pointRotatorAtCall)

function makeSnap(): AppSnapshot {
  return {
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
    radio: {
      dialMhz: 14.074, band: '20m', sideband: 'USB', slot: 0, source: 'native', sourceLabel: 'Native',
      nextSlotMs: 5000, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, txEven: true,
      txCycleAuto: true, txEnabled: false, txAllowed: true, transmitting: false, tuning: false,
      atu: true, qsoRecording: false, catOk: true, splitTxMhz: null,
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

function renderCockpit() {
  const noop = () => {}
  return render(
    <OperateCockpit
      snap={makeSnap()} theme="dark" tier="FT8" onTierChange={noop} bandPlan={[]}
      onSetFrequency={noop} onSourceChange={noop} onTune={noop} onCall={noop} onSetTxLevel={noop}
      onSetMode={noop} onSetTxEven={noop} onSetTxCycleAuto={noop} onResend={noop} onFreetext={noop}
      onLog={noop} onOverrideTx={noop} onHaltTx={noop} roster={<div data-testid="stations-roster" />}
      needByCall={new Map()} selectedCall="JA1ABC" onSelect={noop} onClearSelection={noop}
      layoutMode="classic" onLayoutMode={noop} panels={panelsApi()} active
    />,
  )
}

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  point.mockClear()
  pushToast.mockClear()
})
afterEach(cleanup)

describe('the FT cockpit beams the long way round when LP is pressed', () => {
  it('LP asks the backend for the long path, and the toast names it', async () => {
    renderCockpit()
    fireEvent.click(await screen.findByRole('button', { name: 'LP' }))
    await waitFor(() => expect(point).toHaveBeenCalledTimes(1))
    expect(point, 'LP slewed the short path').toHaveBeenCalledWith('JA1ABC', true)
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('shell.rotator.pointedLong', { bearing: 47, call: 'JA1ABC', to: t('rotor.pointed.to.country', { country: 'Japan' }) }), 'info'),
    )
  })

  // THE CONTROL: forwarding the flag must not have turned the ordinary beam into a long-path one.
  it('→ CALL still asks for the short path, with the toast it always had', async () => {
    renderCockpit()
    fireEvent.click(await screen.findByRole('button', { name: '→ JA1ABC' }))
    await waitFor(() => expect(point).toHaveBeenCalledTimes(1))
    expect(point).toHaveBeenCalledWith('JA1ABC', false)
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('operate.rotor.pointed', { call: 'JA1ABC', deg: 47, to: t('rotor.pointed.to.country', { country: 'Japan' }) }), 'info'),
    )
  })
})
