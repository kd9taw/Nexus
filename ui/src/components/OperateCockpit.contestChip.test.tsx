// @vitest-environment jsdom
//
// "CONTEST: NYQP" IN THE FT HEADER WHILE THE CONTEST SWITCH IS ON. An operator was in a contest without
// knowing it: with the switch on and the station in plain FT8, Tx6 and Call CQ look as they always do
// (a QSO party uses standard messages), a click on a CQ enters the contest's S&P, and the only sign was
// the top bar's "Set operator". The header now names the contest whenever the switch is on, and the
// chip is a button that opens the Contest screen.
//
// RENDERED: the chip is found by its accessible name, and its click is proven to call the handler (a
// presence test cannot see a dead onClick). CONTROLS: the switch off draws no chip; a blank pick names
// ARRL Field Day, as the engine runs it.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, Settings } from '../types'
import { OPERATE_PANELS, panelStateIn } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-canvas" /> }))

/** The settings the backend would hand back — the fixture the toggle reads its state from. */
const saved = vi.hoisted(() => ({ value: {} as Partial<Settings> }))

vi.mock('../api', () => {
  const nothing = () => Promise.resolve(null)
  return {
    getSettings: vi.fn(() => Promise.resolve(saved.value)),
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
    atuTune: vi.fn(nothing),
    qrzLookup: vi.fn(nothing),
    resolveEntity: vi.fn(nothing),
    setMsk144Period: vi.fn(nothing),
  }
})

// The house budget: a loaded full suite runs cases far slower than one core.
vi.setConfig({ testTimeout: 15_000 })

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
      atu: true,
      qsoRecording: false,
      catOk: true,
      splitTxMhz: null,
    },
  } as unknown as AppSnapshot
}

function panelsApi(): PanelLayoutApi<OperatePanelId> {
  return {
    layout: { v: 1, state: {} as Partial<Record<OperatePanelId, PanelState>>, share: {} },
    stateOf: (id) => panelStateIn(OPERATE_PANELS, { v: 1, state: {}, share: {} }, id),
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

function renderCockpit(over: { fdActive?: boolean; fdEvent?: string; onOpenContest?: () => void }) {
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
      onCall={vi.fn()}
      onSetTxLevel={noop}
      onSetMode={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onOverrideTx={noop}
      onHaltTx={noop}
      fdActive={over.fdActive}
      fdEvent={over.fdEvent}
      onOpenContest={over.onOpenContest}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={noop}
      layoutMode="classic"
      onLayoutMode={noop}
      panels={panelsApi()}
      active={false}
    />,
  )
}

const chips = () => document.querySelectorAll('.cockpit-header .cockpit-contest-chip')

afterEach(() => {
  cleanup()
  saved.value = {}
})

describe('the FT header names the contest while the contest switch is on', () => {
  it('reads "Contest: NYQP" with the New York QSO Party picked, in the header, and a click opens the Contest screen', () => {
    const open = vi.fn()
    renderCockpit({ fdActive: true, fdEvent: 'nyqp', onOpenContest: open })
    const chip = screen.getByRole('button', { name: 'Contest: NYQP' })
    expect(chip.closest('.cockpit-header')).not.toBeNull()
    // The full name, and what the switch means, on hover.
    expect(chip.getAttribute('title')).toContain('New York QSO Party')
    fireEvent.click(chip)
    expect(open).toHaveBeenCalledTimes(1)
  })

  it('a blank pick is ARRL Field Day, by the short form the Contest screen shows', () => {
    renderCockpit({ fdActive: true, fdEvent: '', onOpenContest: vi.fn() })
    expect(screen.getByRole('button', { name: 'Contest: Field Day' })).toBeTruthy()
  })

  it('CONTROL: the switch off draws no chip, the party still picked', () => {
    renderCockpit({ fdActive: false, fdEvent: 'nyqp', onOpenContest: vi.fn() })
    expect(chips()).toHaveLength(0)
  })
})
