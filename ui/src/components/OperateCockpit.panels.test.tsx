// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import { TX_METERS_WHEN } from './TxMeters'
import type { AppSnapshot } from '../types'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'
import { OPERATE_PANELS, panelStateIn, seamShares } from '../features/panelState'
import { CLASSIC_FR, classicCommit, classicWidths } from '../features/operateColumns'

// THE BUDGET (2026-10-09). The slowest case here, "is a checkbox at the top of ⊞ Panels, remembered per…", takes
// 0.27 s and 0.20 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
    closePanelWindow: vi.fn(nothing),
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
  const layout = { v: 1 as const, state, share: {} }
  return {
    layout,
    // The vocabulary's own reading of an absent entry, as App's record reads it: docked, but for
    // the pane it ships hidden (the RF scope pane).
    stateOf: (id) => panelStateIn(OPERATE_PANELS, layout, id),
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    setCols: vi.fn(),
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

  it('re-dock also CLOSES the torn-off waterfall window — exactly one waterfall afterwards (#263)', async () => {
    // Re-dock put the strip back in the cockpit but never told the pop-out window to close,
    // so the operator had two waterfalls until they closed the outside one by hand.
    const api = await import('../api')
    const close = api.closePanelWindow as unknown as ReturnType<typeof vi.fn>
    close.mockClear()
    const { container, panels } = renderCockpit({ waterfall: 'popped' })
    fireEvent.click(container.querySelector('.wf-redock')!)
    expect(panels.setPanelState).toHaveBeenCalledWith('waterfall', 'docked')
    expect(close, 're-dock left the torn-off window open').toHaveBeenCalledWith('waterfall')
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
  it('lists the panels the current layout can show, the other layout’s unticked, and unticking removes one', () => {
    const { panels } = renderCockpit({}, 'classic')
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    // Every FT pane can stand in both layouts: Classic offers the Call Roster, unticked until added
    // there, though the record says it is docked (it is, in Roster).
    expect((screen.getByLabelText('Call Roster') as HTMLInputElement).checked).toBe(false)
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
    // F6/Alt+1–6 must not disarm it. (F4 joined Escape above the guard for #204 —
    // WSJT-X parity — and has its own coverage in OperateCockpit.clearcard.test.tsx.)
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


// ── THE DIVIDERS (layout L1, PaneSeam) ──────────────────────────────────────────────────────
// Operate has three: the waterfall's height, and one pane seam per layout — Band Activity /
// Rx Frequency in Roster, the Rx-Frequency column / Stations in Classic. Each must be reachable
// from the keyboard and say where it stands, not only drag. jsdom lays nothing out, so each case
// gives the boxes it needs a size (CSS px at zoom 1) before the cockpit mounts.
describe('Operate dividers answer the keyboard (PaneSeam)', () => {
  function layOut(boxes: Record<string, { top?: number; left?: number; width?: number; height?: number }>) {
    const real = HTMLElement.prototype.getBoundingClientRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      for (const [sel, b] of Object.entries(boxes)) {
        if (!this.matches(sel)) continue
        const { top = 0, left = 0, width = 800, height = 0 } = b
        return { top, left, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) } as DOMRect
      }
      return real.call(this)
    })
  }
  const body = () => document.querySelector<HTMLElement>('.cockpit-body')!
  const key = (el: HTMLElement, k: string, shiftKey = false) => fireEvent.keyDown(el, { key: k, shiftKey })
  beforeEach(() => localStorage.clear())
  afterEach(() => vi.restoreAllMocks())

  it('the waterfall height: focusable, announces its value in CSS px, and steps, jumps and resets', () => {
    layOut({ '.cockpit-body': { height: 600 } })
    renderCockpit({})
    const sep = screen.getByRole('separator', { name: 'waterfall height' })
    expect(sep.tabIndex, 'a divider only a mouse can reach').toBe(0)
    // 22 % of a 600 px body, inside the declared 88–420 px.
    expect([sep.getAttribute('aria-valuenow'), sep.getAttribute('aria-valuemin'), sep.getAttribute('aria-valuemax')]).toEqual(['132', '88', '420'])
    key(sep, 'ArrowDown')
    expect(body().style.getPropertyValue('--cockpit-wf-h')).toBe(`${(148 / 600) * 100}%`)
    expect(localStorage.getItem('nexus.split.operate.waterfall')).toBe(String((148 / 600) * 100))
    expect(sep.getAttribute('aria-valuenow')).toBe('148')
    key(sep, 'ArrowUp', true)
    expect(sep.getAttribute('aria-valuenow')).toBe('88') // 148 − 64 = 84, floored at 88
    key(sep, 'End')
    expect(body().style.getPropertyValue('--cockpit-wf-h')).toBe('70%') // 420 of 600
    key(sep, 'Home')
    expect(sep.getAttribute('aria-valuenow')).toBe('88')
    key(sep, 'Backspace')
    expect(body().style.getPropertyValue('--cockpit-wf-h')).toBe('22%')
    expect(localStorage.getItem('nexus.split.operate.waterfall')).toBe('22')
    key(sep, 'End')
    fireEvent.doubleClick(sep)
    expect(body().style.getPropertyValue('--cockpit-wf-h'), 'a double-click puts the default back').toBe('22%')
  })

  it('a waterfall height stored by an earlier build opens where it was left — clamped, and the stored value kept', () => {
    localStorage.setItem('nexus.split.operate.waterfall', '30')
    layOut({ '.cockpit-body': { height: 600 } })
    const first = renderCockpit({})
    expect(body().style.getPropertyValue('--cockpit-wf-h')).toBe('30%')
    expect(screen.getByRole('separator', { name: 'waterfall height' }).getAttribute('aria-valuenow')).toBe('180')
    first.unmount()
    // A value legal on some taller window: applied at the cap here, and never rewritten.
    localStorage.setItem('nexus.split.operate.waterfall', '90')
    renderCockpit({})
    expect(body().style.getPropertyValue('--cockpit-wf-h')).toBe('70%')
    expect(localStorage.getItem('nexus.split.operate.waterfall')).toBe('90')
  })

  it('Roster: the Band Activity / Rx Frequency seam steps the shares from where the panes ARE, and resets to stock', () => {
    // 300 : 200 on screen — the stock 1.6 : 1 shares are CSS defaults the record never saw, so the
    // step has to start from the measured split, not from the record's 1 : 1.
    layOut({ '.cockpit-decodes-side': { top: 100, height: 300 }, '.cockpit-rxfreq': { top: 408, height: 200 } })
    const { panels } = renderCockpit({}, 'roster')
    const sep = screen.getByRole('separator', { name: 'Band Activity / Rx Frequency' })
    expect(sep.tabIndex).toBe(0)
    expect([sep.getAttribute('aria-valuenow'), sep.getAttribute('aria-valuemin'), sep.getAttribute('aria-valuemax')]).toEqual(['60', '8', '93'])
    key(sep, 'ArrowDown')
    const [a, b] = seamShares(0.6 + 0.05)
    expect(panels.setShares).toHaveBeenLastCalledWith({ bandActivity: a, rxfreq: b })
    key(sep, 'ArrowLeft') // across the axis: not this divider's key
    expect(panels.setShares).toHaveBeenCalledTimes(1)
    key(sep, 'Backspace')
    expect(panels.setShares, 'reset clears the pair back to the sheet defaults').toHaveBeenLastCalledWith({ bandActivity: null, rxfreq: null })
  })

  // Classic's columns on screen for these cases: Band Activity 400, the Rx Frequency column 500,
  // Stations 300, 8 px gaps (the sheet's own widths are fr, which jsdom never resolves).
  const classicBoxes = {
    '.cockpit-decodes': { left: 0, width: 400, height: 400 },
    '.cockpit-qsocol': { left: 408, width: 500, height: 400 },
    '.cockpit-side': { left: 916, width: 300, height: 400 },
  }
  const stock = classicWidths({ v: 1, state: {}, share: {} })
  /** Band Activity's, the Rx Frequency column's and Stations' fractions of the grid, as stored. */
  const storedFractions = (c: { a: number; b: number }) => [c.a / 2, 1 - c.a / 2 - c.b / 2, c.b / 2]

  it('Classic: the Rx Frequency column / Stations divider steps on the horizontal arrows and never moves Band Activity', () => {
    // L1's leftover: this divider's FIRST step used to narrow Band Activity, because it stored the
    // pair as shares summing to 2 beside Band Activity's 1.15fr. It paints and stores the pair at
    // the pair's own total now, so Band Activity keeps the fraction of the grid it had.
    layOut(classicBoxes)
    const { panels } = renderCockpit({}, 'classic')
    const sep = screen.getByRole('separator', { name: 'Rx Frequency column / Stations roster' })
    expect(sep.tabIndex).toBe(0)
    expect(sep.getAttribute('aria-orientation')).toBe('vertical')
    expect(sep.getAttribute('aria-valuenow')).toBe('63') // 500 of 800
    key(sep, 'ArrowRight')
    const [a, b] = seamShares(0.625 + 0.05)
    expect(panels.setShares, 'the pair is no longer stored as the old shares').not.toHaveBeenCalled()
    expect(panels.setCols).toHaveBeenLastCalledWith(classicCommit(stock, 1, 2, a, b))
    const stored = vi.mocked(panels.setCols!).mock.lastCall![0] as { a: number; b: number }
    expect(storedFractions(stored)[0], 'Band Activity moved').toBeCloseTo(CLASSIC_FR[0] / (CLASSIC_FR[0] + CLASSIC_FR[1] + CLASSIC_FR[2]), 12)
    // End stops where Stations reaches its 260 px floor (of the pair's 800), not at the share
    // floor: past it the grid would freeze Stations and take the rest from Band Activity.
    key(sep, 'End')
    expect(panels.setCols).toHaveBeenLastCalledWith(classicCommit(stock, 1, 2, ...seamShares(1 - 260 / 800)))
    fireEvent.doubleClick(sep)
    // From the sheet's own widths, the pair's reset IS the sheet: nothing to store.
    expect(panels.setCols).toHaveBeenLastCalledWith({ a: null, b: null })
  })

  it('Classic: the Band Activity / Rx Frequency column divider steps and never moves Stations', () => {
    layOut(classicBoxes)
    const { panels } = renderCockpit({}, 'classic')
    const sep = screen.getByRole('separator', { name: 'Band Activity / Rx Frequency column' })
    expect(sep.tabIndex).toBe(0)
    expect(sep.getAttribute('aria-valuenow')).toBe('44') // 400 of 900
    key(sep, 'ArrowLeft')
    const [a, b] = seamShares(400 / 900 - 0.05)
    expect(panels.setCols).toHaveBeenLastCalledWith(classicCommit(stock, 0, 1, a, b))
    const stored = vi.mocked(panels.setCols!).mock.lastCall![0] as { a: number; b: number }
    expect(storedFractions(stored)[2], 'Stations moved').toBeCloseTo(CLASSIC_FR[2] / (CLASSIC_FR[0] + CLASSIC_FR[1] + CLASSIC_FR[2]), 12)
  })

  it('Classic: a layout stored by an earlier build opens exactly as it painted, and the first step keeps its Band Activity', () => {
    // Band Activity 1.15fr (the sheet) beside an L1-era pair of 1.4 / 0.6: 1.15 / 3.15 of the grid.
    layOut(classicBoxes)
    const panels = panelsApi({})
    panels.layout.share = { txmsgs: 1.4, stations: 0.6 }
    render(
      <OperateCockpit
        snap={snap}
        theme="dark"
        tier="FT8"
        onTierChange={() => {}}
        bandPlan={[]}
        onSetFrequency={() => {}}
        onSourceChange={() => {}}
        onTune={() => {}}
        onCall={() => {}}
        onSetTxLevel={() => {}}
        onSetMode={() => {}}
        onSetTxEven={() => {}}
        onSetTxCycleAuto={() => {}}
        onResend={() => {}}
        onFreetext={() => {}}
        onLog={() => {}}
        onOverrideTx={() => {}}
        onHaltTx={() => {}}
        roster={<div data-testid="stations-roster" />}
        needByCall={new Map()}
        selectedCall={null}
        onSelect={() => {}}
        layoutMode="classic"
        onLayoutMode={() => {}}
        panels={panels}
      />,
    )
    const grid = document.querySelector<HTMLElement>('.cockpit-lower.classic')!
    // The same proportions that build painted (1.15 : 1.4 : 0.6), in the sheet's whole fr.
    expect(parseFloat(grid.style.getPropertyValue('--op-col-a'))).toBeCloseTo(140, 9)
    expect(parseFloat(grid.style.getPropertyValue('--op-col-b'))).toBeCloseTo(60, 9)
    expect(grid.style.getPropertyValue('--op-col-ba'), 'Band Activity is left to the sheet, as that build left it').toBe('')
    fireEvent.keyDown(screen.getByRole('separator', { name: 'Rx Frequency column / Stations roster' }), { key: 'ArrowRight' })
    const stored = vi.mocked(panels.setCols!).mock.lastCall![0] as { a: number; b: number }
    expect(storedFractions(stored)[0]).toBeCloseTo(1.15 / (1.15 + 1.4 + 0.6), 12)
  })

  it('Roster: the Call Roster / side rail divider stores the Call Roster’s share, and resets to stock', () => {
    layOut({ '.cockpit-roster-main': { left: 0, width: 600, height: 400 }, '.cockpit-side': { left: 612, width: 400, height: 400 } })
    const { panels } = renderCockpit({}, 'roster')
    const sep = screen.getByRole('separator', { name: 'Call Roster / side rail' })
    expect(sep.tabIndex).toBe(0)
    expect(sep.getAttribute('aria-valuenow')).toBe('60')
    // The side rail floors at 360 of the pair's 1000: the step stops there, at 64 %.
    expect(sep.getAttribute('aria-valuemax')).toBe('64')
    key(sep, 'ArrowRight')
    expect(panels.setShares).toHaveBeenLastCalledWith({ callRoster: seamShares(1 - 360 / 1000)[0] })
    key(sep, 'Backspace')
    expect(panels.setShares).toHaveBeenLastCalledWith({ callRoster: null })
  })
})

describe('Operate Classic: the Rx Frequency / Tx1–Tx6 divider (layout L5)', () => {
  // The Tx machine keeps its content height until the divider moves; the divider sits ABOVE it, so
  // moving down shrinks it (it scrolls) and gives Rx Frequency the room. jsdom lays nothing out: the
  // column is 400 px, the machine's rows 198 px, and sized it lays out like its rule (the painted
  // basis, the 4em floor at jsdom's 16 px, never taller than its rows).
  beforeEach(() => localStorage.clear())
  afterEach(() => vi.restoreAllMocks())
  const KEY = 'nexus.split.operate.tx'
  const qsocol = () => document.querySelector<HTMLElement>('.cockpit-qsocol')!
  const tx = () => document.querySelector<HTMLElement>('.cockpit-qsocol > .tx-panel')!
  const divider = () => screen.queryByRole('separator', { name: 'Tx messages height' })
  const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => el.getAttribute(a))
  function layOut() {
    const real = HTMLElement.prototype.getBoundingClientRect
    const rect = (height: number) => ({ top: 0, left: 0, width: 500, height, right: 500, bottom: height, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (this.matches('.cockpit-qsocol')) return rect(400)
      if (this.matches('.cockpit-qsocol > .tx-panel')) {
        if (!this.hasAttribute('data-sized')) return rect(198)
        const v = (this.parentElement as HTMLElement).style.getPropertyValue('--op-tx-h')
        const basis = v === '' ? 198 : v.endsWith('%') ? parseFloat(v) * 4 : parseFloat(v)
        return rect(Math.min(198, Math.max(64, basis)))
      }
      return real.call(this)
    })
  }

  it('sits between Rx Frequency and the Tx machine, and paints nothing until it is moved', () => {
    layOut()
    renderCockpit({}, 'classic')
    const sep = divider()!
    expect(sep, 'no divider between Rx Frequency and the Tx machine').not.toBeNull()
    expect(sep.previousElementSibling!.classList.contains('cockpit-rxfreq')).toBe(true)
    expect(sep.nextElementSibling).toBe(tx())
    expect(sep.tabIndex).toBe(0)
    expect(qsocol().style.getPropertyValue('--op-tx-h')).toBe('')
    expect(tx().hasAttribute('data-sized')).toBe(false)
    expect(localStorage.getItem(KEY)).toBeNull()
    // The machine as it stands; its 4em floor; its own rows as the ceiling.
    expect(aria(sep)).toEqual(['198', '64', '198'])
  })

  it('down shrinks the machine (it scrolls), up gives it back, and a reset returns its content height', () => {
    layOut()
    renderCockpit({}, 'classic')
    const sep = divider()!
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(qsocol().style.getPropertyValue('--op-tx-h')).toBe(`${(182 / 400) * 100}%`)
    expect(tx().hasAttribute('data-sized')).toBe(true)
    expect(localStorage.getItem(KEY)).toBe(String((182 / 400) * 100))
    fireEvent.keyDown(sep, { key: 'ArrowUp' })
    expect(sep.getAttribute('aria-valuenow')).toBe('198')
    fireEvent.keyDown(sep, { key: 'Home' })
    expect(sep.getAttribute('aria-valuenow')).toBe('64')
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(qsocol().style.getPropertyValue('--op-tx-h')).toBe('')
    expect(tx().hasAttribute('data-sized')).toBe(false)
    expect(sep.getAttribute('aria-valuenow')).toBe('198')
  })

  it('a stored height is restored sized and clamped, and never rewritten', () => {
    localStorage.setItem(KEY, '10')
    layOut()
    renderCockpit({}, 'classic')
    expect(tx().hasAttribute('data-sized')).toBe(true)
    expect(divider()!.getAttribute('aria-valuenow'), 'the 4em floor').toBe('64')
    expect(localStorage.getItem(KEY)).toBe('10')
  })

  it('goes with either of the two panes it sits between', () => {
    renderCockpit({ rxfreq: 'removed' }, 'classic')
    expect(divider()).toBeNull()
    cleanup()
    renderCockpit({ txmsgs: 'removed' }, 'classic')
    expect(divider()).toBeNull()
    cleanup()
    renderCockpit({}, 'roster')
    expect(divider(), 'Roster has no Tx machine').toBeNull()
  })
})

describe('Operate: the rail on the left (layout L5)', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => vi.restoreAllMocks())
  /** ⊞ Panels ▸ Side rail on the left: the menu opened, its checkbox. (In the header it wrapped
   *  the row at 1024×768 and took 44 px from the decode lists: measured in Chrome.) */
  const toggle = () => {
    const btn = screen.getByRole('button', { name: /Panels/ })
    if (btn.getAttribute('aria-expanded') !== 'true') fireEvent.click(btn)
    return screen.getByRole('checkbox', { name: 'Side rail on the left' }) as HTMLInputElement
  }
  const grid = () => document.querySelector<HTMLElement>('.cockpit-lower')!

  it('is a checkbox at the top of ⊞ Panels, remembered per surface, moving the rail by CSS alone', () => {
    renderCockpit({}, 'classic')
    expect(toggle().checked).toBe(false)
    expect(toggle().getAttribute('aria-describedby'), 'the line saying what the rail holds').toBeTruthy()
    expect(screen.queryByRole('button', { name: /Rail/ }), 'a header control wraps the header at 1024').toBeNull()
    expect(grid().hasAttribute('data-rail')).toBe(false)
    const aside = document.querySelector('aside.cockpit-side')!
    const decodes = document.querySelector('.cockpit-decodes')!
    fireEvent.click(toggle())
    expect(toggle().checked).toBe(true)
    expect(grid().getAttribute('data-rail')).toBe('left')
    expect(localStorage.getItem('nexus.operate.railSide')).toBe('left')
    // No reparent, no remount: the same nodes, in the same order in the tree.
    expect(document.querySelector('aside.cockpit-side')).toBe(aside)
    expect(document.querySelector('.cockpit-decodes')).toBe(decodes)
    expect(decodes.compareDocumentPosition(aside) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    cleanup()
    renderCockpit({}, 'classic')
    expect(grid().getAttribute('data-rail'), 'the choice did not survive a remount').toBe('left')
  })

  it('names Classic’s dividers by the columns they now sit between, and still moves only their pairs', () => {
    localStorage.setItem('nexus.operate.railSide', 'left')
    renderCockpit({}, 'classic')
    expect(screen.getAllByRole('separator').filter((e) => e.classList.contains('op-colseam')).map((e) => e.getAttribute('aria-label'))).toEqual([
      'Stations roster / Band Activity',
      'Band Activity / Rx Frequency column',
    ])
  })

  it('in Roster, puts the rail first and names its divider that way round; the Call Roster keeps its share', () => {
    localStorage.setItem('nexus.operate.railSide', 'left')
    const real = HTMLElement.prototype.getBoundingClientRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      const box = this.matches('.cockpit-side') ? [0, 400] : this.matches('.cockpit-roster-main') ? [412, 600] : null
      if (!box) return real.call(this)
      const [left, width] = box
      return { top: 0, left, width, height: 400, right: left + width, bottom: 400, x: left, y: 0, toJSON: () => ({}) } as DOMRect
    })
    const { panels } = renderCockpit({}, 'roster')
    const sep = screen.getByRole('separator', { name: 'Side rail / Call Roster' })
    expect(sep.getAttribute('aria-valuenow')).toBe('40')
    fireEvent.keyDown(sep, { key: 'ArrowRight' })
    // The rail grew: the Call Roster's share is the pair's second.
    expect(panels.setShares).toHaveBeenLastCalledWith({ callRoster: seamShares(0.4 + 0.05)[1] })
  })

  it('stays where it is while the rail is not on screen, and says so by not pressing the grid', () => {
    localStorage.setItem('nexus.operate.railSide', 'left')
    renderCockpit({ stations: 'removed' }, 'classic')
    expect(toggle().checked).toBe(true)
    expect(grid().hasAttribute('data-rail'), 'a rail-left template with no rail').toBe(false)
  })
})
