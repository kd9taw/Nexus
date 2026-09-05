// @vitest-environment jsdom
//
// Task 4 of the POTA map plan: the "Map" button in the FT header opens the `operatemap`
// pop-out (Task 3) via `openPanelWindow('operatemap')`. It sits beside the Classic/Roster
// toggle as a plain header control — it does not touch `layoutMode` and is not inside any
// ⊞-removable pane.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import { openPanelWindow } from '../api'
import { t } from '../i18n'
import type { AppSnapshot } from '../types'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

vi.mock('./Waterfall', () => ({
  Waterfall: () => <div data-testid="waterfall-canvas" />,
}))

vi.mock('../api', () => {
  const nothing = () => Promise.resolve(null)
  return {
    getSettings: vi.fn(() => Promise.resolve({})),
    setSettings: vi.fn(nothing),
    openPanelWindow: vi.fn(nothing),
    notifyErase: vi.fn(nothing),
    pointRotatorAtCall: vi.fn(nothing),
    redecode: vi.fn(nothing),
    startCq: vi.fn(nothing),
    startQsoRecording: vi.fn(nothing),
    stopQsoRecording: vi.fn(nothing),
    setSkipTx1: vi.fn(nothing),
    getDeclination: vi.fn(nothing),
    getSatTrackStatus: vi.fn(nothing),
    readRotator: vi.fn(nothing),
    stopRotator: vi.fn(nothing),
    stopSatTrack: vi.fn(nothing),
    openQrzPage: vi.fn(nothing),
    postSpot: vi.fn(nothing),
    setFrequency: vi.fn(nothing),
    setRit: vi.fn(nothing),
    setXit: vi.fn(nothing),
    setVfo: vi.fn(nothing),
    getSpectrumRow: vi.fn(nothing),
    setDecodeDepth: vi.fn(nothing),
  }
})

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
      atu: null,
      qsoRecording: false,
      catOk: true,
      splitTxMhz: null,
    },
  } as unknown as AppSnapshot
}

function panelsApi(state: Partial<Record<OperatePanelId, PanelState>>): PanelLayoutApi<OperatePanelId> {
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
  const onLayoutMode = vi.fn()
  render(
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
      layoutMode="classic"
      onLayoutMode={onLayoutMode}
      panels={panelsApi({})}
      active={false}
    />,
  )
  return { onLayoutMode }
}

afterEach(() => cleanup())

describe('the Map button in the FT header', () => {
  it('opens the operatemap pop-out and leaves layoutMode untouched', () => {
    const { onLayoutMode } = renderCockpit()
    const btn = screen.getByTitle(t('operate.header.map.title'))
    expect(btn.textContent).toBe(t('operate.header.map.label'))
    fireEvent.click(btn)
    expect(openPanelWindow).toHaveBeenCalledWith('operatemap')
    expect(onLayoutMode).not.toHaveBeenCalled()
  })
})
