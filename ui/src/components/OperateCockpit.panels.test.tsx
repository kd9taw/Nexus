// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import { TX_METERS_WHEN } from './TxMeters'
import type { AppSnapshot } from '../types'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

// The waterfall paints to a canvas jsdom does not implement, and it polls the spectrum
// on a timer — stub it. The point of these cases is whether it MOUNTS at all.
vi.mock('./Waterfall', () => ({
  Waterfall: () => <div data-testid="waterfall-canvas" />,
}))

// Every engine call the cockpit's subtree makes on mount, stubbed harmlessly.
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
    getSatTransponder: vi.fn(nothing),
    setSatTransponder: vi.fn(nothing),
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
  }
})

const snap = {
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
    qsoRecording: false,
    catOk: true,
    splitTxMhz: null,
  },
} as unknown as AppSnapshot

/** A host-owned record, frozen for the render under test. */
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

function renderCockpit(
  state: Partial<Record<OperatePanelId, PanelState>>,
  layoutMode: 'classic' | 'roster' = 'classic',
  extra: { active?: boolean; onHaltTx?: () => void; dxClearTick?: number } = {},
) {
  const noop = () => {}
  const panels = panelsApi(state)
  const el = (tick: number) => (
    <OperateCockpit
      dxClearTick={tick}
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
      onHaltTx={extra.onHaltTx ?? noop}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={noop}
      layoutMode={layoutMode}
      onLayoutMode={noop}
      panels={panels}
      active={extra.active ?? false}
    />
  )
  const view = render(el(extra.dxClearTick ?? 0))
  /** Move ONLY `dxClearTick` on the SAME mounted instance — the whole point is what survives
   *  a clear, which a fresh render could never show. */
  const bumpDxClear = (tick: number) => view.rerender(el(tick))
  return { ...view, panels, bumpDxClear }
}

afterEach(() => cleanup())

describe('OperateCockpit — waterfall removal', () => {
  it('mounts the docked waterfall AND its resize splitter by default', () => {
    const { container } = renderCockpit({})
    expect(container.querySelector('.cockpit-waterfall')).not.toBeNull()
    expect(screen.getByRole('separator', { name: 'waterfall height' })).toBeTruthy()
    expect(container.querySelector('.wf-redock')).toBeNull()
  })

  it('removed: the waterfall, its splitter AND the re-dock bar all unmount', () => {
    const { container } = renderCockpit({ waterfall: 'removed' })
    expect(container.querySelector('.cockpit-waterfall')).toBeNull()
    // The 8px seam under it must go too — a stranded handle would resize nothing.
    expect(screen.queryByRole('separator', { name: 'waterfall height' })).toBeNull()
    // 'removed' means gone: no placeholder, no bar, nothing to click.
    expect(container.querySelector('.wf-redock')).toBeNull()
    // …and the decode region is still there to take the space.
    expect(container.querySelector('.cockpit-lower')).not.toBeNull()
  })

  it('popped: the re-dock bar stands in, but the strip and splitter are still unmounted', () => {
    const { container } = renderCockpit({ waterfall: 'popped' })
    expect(container.querySelector('.cockpit-waterfall')).toBeNull()
    expect(screen.queryByRole('separator', { name: 'waterfall height' })).toBeNull()
    expect(container.querySelector('.wf-redock')).not.toBeNull()
  })
})

describe('OperateCockpit — the reclaimed space', () => {
  it('classic: emptying the qsocol AND the roster column collapses the grid to one column', () => {
    const { container } = renderCockpit({ txmsgs: 'removed', rxfreq: 'removed', stations: 'removed' })
    expect(container.querySelector('aside.cockpit-side')).toBeNull()
    expect(container.querySelector('.cockpit-qsocol')).toBeNull()
    expect(container.querySelector('.cockpit-lower')?.getAttribute('data-cols')).toBe('one')
    // Band Activity keeps its cell and now owns the full width.
    expect(container.querySelector('.cockpit-decodes')).not.toBeNull()
  })

  it('classic: removing Band Activity leaves the pair column + roster as two columns', () => {
    const { container } = renderCockpit({ bandActivity: 'removed' })
    expect(container.querySelector('.cockpit-decodes')).toBeNull()
    expect(container.querySelector('.cockpit-qsocol')).not.toBeNull()
    expect(container.querySelector('aside.cockpit-side')).not.toBeNull()
    expect(container.querySelector('.cockpit-lower')?.getAttribute('data-cols')).toBe('two')
  })

  it('keeps all three columns while each holds a panel (Tx machine holds the qsocol alone)', () => {
    const { container } = renderCockpit({ rxfreq: 'removed' })
    expect(container.querySelector('.cockpit-rxfreq')).toBeNull()
    // txmsgs still populates the middle column, stations the third.
    expect(container.querySelector('.cockpit-qsocol')).not.toBeNull()
    expect(container.querySelector('aside.cockpit-side')).not.toBeNull()
    expect(container.querySelector('.cockpit-lower')?.getAttribute('data-cols')).toBe('three')
  })

  it('roster: the layout drops its own panels independently', () => {
    const { container } = renderCockpit({ callRoster: 'removed' }, 'roster')
    expect(container.querySelector('.cockpit-roster-main')).toBeNull()
    expect(container.querySelector('.cockpit-decodes-side')).not.toBeNull()
    expect(container.querySelector('.cockpit-lower')?.getAttribute('data-cols')).toBe('one')
  })
})

describe('⊞ Panels menu', () => {
  it('lists only the panels the current layout renders, and unticking removes one', () => {
    const { panels } = renderCockpit({}, 'classic')
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    // Classic has no Call Roster pane, so offering it would tick a panel into nowhere.
    expect(screen.queryByLabelText('Call Roster')).toBeNull()
    fireEvent.click(screen.getByLabelText('Waterfall'))
    expect(panels.setPanelState).toHaveBeenCalledWith('waterfall', 'removed')
  })

  it('a removed panel stays listed and ticked-off, so it can always be brought back', () => {
    const { panels } = renderCockpit({ waterfall: 'removed' })
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    const box = screen.getByLabelText('Waterfall') as HTMLInputElement
    expect(box.checked).toBe(false)
    fireEvent.click(box)
    expect(panels.setPanelState).toHaveBeenCalledWith('waterfall', 'docked')
  })

  it('TX Meters stay operable, with the note that says when they read', () => {
    // Operate carries the same `txmeters` id as Phone and CW, so it gets the same
    // honesty: the gate works (the strip is there to hide), and the entry says WHEN the
    // meters have readings instead of leaving the operator to guess mid-menu. A note
    // annotates an entry; nothing in this menu may refuse the operator's tick.
    renderCockpit({}, 'classic')
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    const box = screen.getByLabelText('TX Meters') as HTMLInputElement
    expect(box.disabled).toBe(false)
    expect(box.getAttribute('aria-disabled')).toBeNull()
    expect(box.getAttribute('aria-describedby')).toBe(screen.getByText(TX_METERS_WHEN).id)
  })

  it('always offers Undo and Reset, so a mis-tick can never strand the operator', () => {
    const { panels } = renderCockpit({ waterfall: 'removed', stations: 'removed' })
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    fireEvent.click(screen.getByRole('button', { name: 'Reset layout' }))
    expect(panels.reset).toHaveBeenCalled()
  })
})

describe('OperateCockpit — TX controls are not panels', () => {
  it('Stop TX survives removing every removable panel', () => {
    renderCockpit({
      waterfall: 'removed',
      bandActivity: 'removed',
      callRoster: 'removed',
      rxfreq: 'removed',
      txmsgs: 'removed',
      stations: 'removed',
    })
    expect(screen.getByRole('button', { name: /stop tx/i })).toBeTruthy()
    // The Rx/Tx offset spinners are the only way to place TX in the passband once the
    // waterfall's click-to-tune is gone — they are chrome, so they must still be here.
    expect(screen.getByLabelText('Rx offset in Hz')).toBeTruthy()
    expect(screen.getByLabelText('Tx offset in Hz')).toBeTruthy()
  })

  it('Escape halts TX even while focus is in a text field', () => {
    const onHaltTx = vi.fn()
    renderCockpit({}, 'classic', { active: true, onHaltTx })
    // Escape is an abort key, not an editing key: the typing guard that disarms
    // F4/F6/Alt+1–6 must not disarm it.
    fireEvent.keyDown(screen.getByLabelText('Rx offset in Hz'), { key: 'Escape' })
    expect(onHaltTx).toHaveBeenCalledTimes(1)
  })
})

// USER REPORT via the operator, 2026-08-23 (KR4FQG): "I'd like to send CQ DX but I can't find a
// way to send that without going back to Classic. I can change it in Classic and it will work
// for one call, then revert back to just 'CQ' unless I go back to Classic and change it again."
//
// The Tx6 field IS the way — `cqDirFromText` parses it and Tx6 fires `startCq(dir)`. What broke
// it is that the stock "Clear DX call and grid after logging" option wiped Tx6 too, so the
// directed CQ survived exactly one contact.
//
// WSJT-X keeps the two apart: editing Tx6 sets `m_CQtype` (`on_tx6_editingFinished`), which the
// DX-clear never touches. The option's own name says what it clears, and the CQ message is not
// the DX call.
describe('a directed CQ survives the after-logging DX clear', () => {
  const TX6 = 'CQ DX KD9TAW EN52'

  function tx6Field(): HTMLInputElement | null {
    // The Tx6 row's editable text input, found by its current value.
    const inputs = Array.from(document.querySelectorAll('input')) as HTMLInputElement[]
    return inputs.find((i) => i.value.toUpperCase().startsWith('CQ')) ?? null
  }

  it('keeps the operator edit when the DX call is cleared after a QSO', () => {
    const { bumpDxClear } = renderCockpit({}, 'classic', { dxClearTick: 0 })
    const f = tx6Field()
    expect(f, 'control: the Tx6 CQ field is on screen').not.toBeNull()

    fireEvent.change(f as HTMLInputElement, { target: { value: TX6 } })
    expect(tx6Field()?.value, 'control: the edit took').toBe(TX6)

    // The QSO logs and the stock option fires the DX clear, on the SAME instance.
    bumpDxClear(1)
    expect(tx6Field()?.value, 'the directed CQ must survive the DX clear').toBe(TX6)

    // …and again, because a pileup is many contacts, not one.
    bumpDxClear(2)
    expect(tx6Field()?.value).toBe(TX6)
  })
})

