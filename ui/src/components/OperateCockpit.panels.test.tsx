// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
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
    undo: vi.fn(),
    canUndo: false,
    reset: vi.fn(),
  }
}

function renderCockpit(
  state: Partial<Record<OperatePanelId, PanelState>>,
  layoutMode: 'classic' | 'roster' = 'classic',
  extra: { active?: boolean; onHaltTx?: () => void } = {},
) {
  const noop = () => {}
  const panels = panelsApi(state)
  const view = render(
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
      onHaltTx={extra.onHaltTx ?? noop}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={noop}
      layoutMode={layoutMode}
      onLayoutMode={noop}
      panels={panels}
      active={extra.active ?? false}
    />,
  )
  return { ...view, panels }
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
  it('classic: emptying the side rail unmounts it and collapses the grid to one column', () => {
    const { container } = renderCockpit({ txmsgs: 'removed', rxfreq: 'removed', stations: 'removed' })
    expect(container.querySelector('aside.cockpit-side')).toBeNull()
    expect(container.querySelector('.cockpit-lower')?.getAttribute('data-cols')).toBe('one')
    // Band Activity keeps its cell and now owns the full width.
    expect(container.querySelector('.cockpit-decodes')).not.toBeNull()
  })

  it('classic: removing the MAIN pane collapses the grid too, so the rail reclaims', () => {
    const { container } = renderCockpit({ bandActivity: 'removed' })
    expect(container.querySelector('.cockpit-decodes')).toBeNull()
    expect(container.querySelector('aside.cockpit-side')).not.toBeNull()
    expect(container.querySelector('.cockpit-lower')?.getAttribute('data-cols')).toBe('one')
  })

  it('keeps both columns while both sides hold a panel', () => {
    const { container } = renderCockpit({ rxfreq: 'removed' })
    expect(container.querySelector('.cockpit-rxfreq')).toBeNull()
    expect(container.querySelector('aside.cockpit-side')).not.toBeNull()
    expect(container.querySelector('.cockpit-lower')?.getAttribute('data-cols')).toBe('two')
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
