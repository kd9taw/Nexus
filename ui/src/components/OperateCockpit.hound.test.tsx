// @vitest-environment jsdom
//
// THE HOUND TOGGLE, RENDERED. Operator ask: "add ft8 superfox as a clickable button option on
// the main nexus ft8 screen so users can click it on and off without having to go into the
// settings" — the hound side of it. Hound is a per-DXpedition mode an operator enters and
// leaves inside one session, so it has to be one click from the cockpit; it was a dropdown in
// the header (open it, read two options, pick one) behind a `specialOp` that otherwise only
// Settings wrote.
//
// WHAT THIS FILE ASSERTS, and why each is a RENDER rather than a selector match:
//   - the control is a real toggle in the document, found by its accessible name, with
//     `aria-pressed` reporting the saved state;
//   - clicking it WRITES — `setSettings` is called with `specialOp` flipped, which is the only
//     thing that makes it a control rather than a light. A presence test cannot see a dead
//     onClick, and dead selectors passing presence tests is how two fixes shipped here;
//   - it is a TOGGLE, not a menu: `SuperHound` is a retired alias that behaves as plain Hound
//     (settings.rs), so it is never offered as a choice, and a settings file that still carries
//     it renders the button ON rather than some third state;
//   - the SuperFox notice appears BEFORE the operator calls — whenever the calendar says an
//     operation on the air runs SuperFox, which Nexus does not decode in this version.
//
// NOT A STOP CONTROL. Hound neither starts nor stops a transmission, so it takes no part in the
// stop line and belongs in no sweep's `stopControls` (CLAUDE.md). It renders in the header
// beside the other header state controls — outside every ⊞-removable pane, because that is
// where this cockpit's non-pane controls live, not because the stop line demands it.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, Settings } from '../types'
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
    getLog: vi.fn(() => Promise.resolve([])),
    qrzLookup: vi.fn(nothing),
    resolveEntity: vi.fn(nothing),
    setMsk144Period: vi.fn(nothing),
  }
})

import { getSettings, setSettings } from '../api'

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
    stateOf: () => 'docked' as PanelState,
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

function renderCockpit(over: { superFoxCalls?: string[] } = {}) {
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
      superFoxCalls={over.superFoxCalls}
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

const houndButton = () => screen.getByRole('button', { name: /^hound$/i })

afterEach(() => {
  cleanup()
  saved.value = {}
  vi.mocked(setSettings).mockClear()
  vi.mocked(getSettings).mockClear()
})

describe('Hound is one click from the cockpit, and it is wired', () => {
  it('renders as an OFF toggle when the saved settings carry no special op', async () => {
    renderCockpit()
    await waitFor(() => expect(getSettings).toHaveBeenCalled())
    expect(houndButton().getAttribute('aria-pressed')).toBe('false')
  })

  it('clicking it SAVES specialOp: hound — not just a class change', async () => {
    renderCockpit()
    await waitFor(() => expect(getSettings).toHaveBeenCalled())
    fireEvent.click(houndButton())
    await waitFor(() => expect(setSettings).toHaveBeenCalled())
    expect(vi.mocked(setSettings).mock.calls[0][0]).toMatchObject({ specialOp: 'hound' })
    expect(houndButton().getAttribute('aria-pressed')).toBe('true')
  })

  it('clicking it again LEAVES Hound — the operator gets out mid-session without Settings', async () => {
    saved.value = { specialOp: 'hound' }
    renderCockpit()
    await waitFor(() => expect(houndButton().getAttribute('aria-pressed')).toBe('true'))
    fireEvent.click(houndButton())
    await waitFor(() => expect(setSettings).toHaveBeenCalled())
    expect(vi.mocked(setSettings).mock.calls[0][0]).toMatchObject({ specialOp: 'none' })
    expect(houndButton().getAttribute('aria-pressed')).toBe('false')
  })

  it('offers TWO states only — the retired SuperHound alias is not a third choice', async () => {
    // `SuperHound` behaves exactly as `Hound` in the engine (settings.rs). Offering it as its
    // own option would be a label promising something it does not do.
    saved.value = { specialOp: 'superhound' }
    renderCockpit()
    await waitFor(() => expect(houndButton().getAttribute('aria-pressed')).toBe('true'))
    expect(screen.queryByRole('button', { name: /super ?hound/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /super ?fox/i })).toBeNull()
    // …and it is a button, not the dropdown it replaced: no select survives in that slot.
    expect(document.querySelector('.cockpit-specialop select')).toBeNull()
  })
})

describe('SuperFox is named before the operator calls', () => {
  const superFoxNote = () => document.querySelector('.cockpit-superfox-note')

  it('says nothing when no operation on the air is running SuperFox', async () => {
    renderCockpit({ superFoxCalls: [] })
    await waitFor(() => expect(getSettings).toHaveBeenCalled())
    expect(superFoxNote(), 'a permanent notice is noise, not a warning').toBeNull()
  })

  it('names the operation and says Nexus does not decode it in this version', async () => {
    renderCockpit({ superFoxCalls: ['3Y0X'] })
    await waitFor(() => expect(superFoxNote()).not.toBeNull())
    const text = superFoxNote()!.textContent ?? ''
    expect(text).toContain('3Y0X')
    expect(text).toMatch(/SuperFox/i)
    expect(text).toMatch(/WSJT-X/)
    // The retirement ruling is being re-tested right now; the wording must not close the
    // door on it.
    expect(text, 'do not hard-code "never"').not.toMatch(/\bnever\b/i)
  })

  it('is a HEADER notice, beside the Hound button the operator is about to press', async () => {
    renderCockpit({ superFoxCalls: ['3Y0X'] })
    await waitFor(() => expect(superFoxNote()).not.toBeNull())
    expect(superFoxNote()!.closest('.cockpit-specialop')).not.toBeNull()
  })
})
