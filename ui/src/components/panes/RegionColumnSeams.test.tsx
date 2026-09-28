// @vitest-environment jsdom
//
// THE GRID COCKPITS' COLUMN DIVIDERS (layout L2), with the REAL panel record (usePanelLayout) and a
// minimal region: what a key, a drag, a reset and a stored record do, and what is announced.
// jsdom lays nothing out, so each box's size is stubbed from data attributes the host renders
// (`data-x` / `data-w` for a rect, `data-cw` for clientWidth) and a resize is fired by hand — the
// real layout, where the sheet clamps the log column to 24em … half the region, is measured in
// Chrome by the layout harness.
import { useRef } from 'react'
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { RegionColumnSeams } from './RegionColumnSeams'
import { PHONE_PANELS, panelStorageKey, seamShares, usePanelLayout } from '../../features/panelState'
import { regionColsStyle } from '../../features/paneColumns'

const KEY = panelStorageKey('phone')
const record = () => JSON.parse(localStorage.getItem(KEY) ?? '{}')

let observers: Array<() => void> = []
const resized = () => act(() => observers.forEach((f) => f()))
const realRect = HTMLElement.prototype.getBoundingClientRect
const realClient = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')

beforeEach(() => {
  localStorage.clear()
  observers = []
  globalThis.ResizeObserver = class {
    cb: () => void
    constructor(cb: () => void) {
      this.cb = cb
      observers.push(cb)
    }
    observe() {}
    unobserve() {}
    disconnect() {
      observers = observers.filter((f) => f !== this.cb)
    }
  } as unknown as typeof ResizeObserver
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    const x = Number(this.dataset.x ?? 0)
    const w = Number(this.dataset.w ?? 0)
    return { left: x, right: x + w, width: w, top: 0, bottom: 500, height: 500, x, y: 0, toJSON: () => ({}) } as DOMRect
  }
  Object.defineProperty(Element.prototype, 'clientWidth', {
    configurable: true,
    get(this: HTMLElement) {
      return Number(this.dataset?.cw ?? 0)
    },
  })
})
afterEach(() => {
  cleanup()
  HTMLElement.prototype.getBoundingClientRect = realRect
  if (realClient) Object.defineProperty(Element.prototype, 'clientWidth', realClient)
})

/** A region `regionW` wide at 14 px, its columns at the given boxes, the dividers over them. */
function Host({ cols, regionW, boxes }: { cols: 2 | 3; regionW: number; boxes: Array<[number, number]> }) {
  const panels = usePanelLayout(PHONE_PANELS)
  const region = useRef<HTMLDivElement>(null)
  const refs = [useRef<HTMLDivElement>(null), useRef<HTMLDivElement>(null), useRef<HTMLDivElement>(null)]
  const tracks = refs.slice(0, cols)
  return (
    <>
      <div
        className="cockpit-panes"
        ref={region}
        data-cw={regionW}
        data-w={regionW}
        style={{ fontSize: '14px', ...regionColsStyle(panels.layout.cols) }}
      >
        {tracks.map((r, i) => (
          <div key={i} className="cockpit-col" ref={r} data-x={boxes[i][0]} data-w={boxes[i][1]} />
        ))}
        <RegionColumnSeams
          region={region}
          cols={cols}
          tracks={tracks}
          stored={panels.layout.cols}
          setCols={panels.setCols!}
          splitLabel="Band activity column / Receiver column"
          widthLabel="log column width"
        />
      </div>
      <button type="button" onClick={panels.reset}>Reset</button>
      <button type="button" onClick={panels.undo}>Undo</button>
    </>
  )
}

const width = () => screen.getByRole('separator', { name: 'log column width' })
const split = () => screen.getByRole('separator', { name: 'Band activity column / Receiver column' })
const region = () => document.querySelector<HTMLElement>('.cockpit-panes')!
const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => Number(el.getAttribute(a)))

// Two columns in a 1200 px region: the feed column 700 wide, the log 488 (a 12 px gap).
const TWO: Array<[number, number]> = [[0, 700], [712, 488]]
// Three in 1800: 700 · 452 · 624.
const THREE: Array<[number, number]> = [[0, 700], [712, 452], [1176, 624]]

describe('the log column’s width divider', () => {
  it('is a focusable separator announcing the log column’s width in the range the layout honours', () => {
    render(<Host cols={2} regionW={1200} boxes={TWO} />)
    const sep = width()
    expect(sep.tabIndex).toBe(0)
    expect(sep.getAttribute('aria-orientation')).toBe('vertical')
    // Its width, 24em at 14 px, and half the region.
    expect(aria(sep)).toEqual([488, 336, 600])
    expect(sep.className).toContain('cockpit-colseam cockpit-colseam-2')
  })

  it('at three columns it sits on the third track, beside the split divider on the second', () => {
    render(<Host cols={3} regionW={1800} boxes={THREE} />)
    expect(width().className).toContain('cockpit-colseam-3')
    expect(aria(width())).toEqual([624, 336, 900])
    expect(split().className).toContain('cockpit-colseam cockpit-colseam-2')
  })

  it('answers the arrows the way they point (left widens the log), Shift for a big step, Home/End to its ends, Backspace to the default', () => {
    render(<Host cols={2} regionW={1200} boxes={TWO} />)
    const sep = width()
    fireEvent.keyDown(sep, { key: 'ArrowLeft' })
    expect(record().cols).toEqual({ log: 504 })
    expect(region().style.getPropertyValue('--cockpit-col-log')).toBe('min(504px, 50%)')
    expect(Number(sep.getAttribute('aria-valuenow')), 'the next key must step from the committed width').toBe(504)
    fireEvent.keyDown(sep, { key: 'ArrowRight', shiftKey: true })
    expect(record().cols).toEqual({ log: 440 })
    fireEvent.keyDown(sep, { key: 'Home' })
    expect(record().cols).toEqual({ log: 336 })
    fireEvent.keyDown(sep, { key: 'End' })
    expect(record().cols).toEqual({ log: 600 })
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(record().cols, 'a reset leaves no width of its own behind').toBeUndefined()
    expect(region().style.getPropertyValue('--cockpit-col-log'), 'the default is no token at all').toBe('')
    // A chord belongs to the OS and the app, never the divider.
    fireEvent.keyDown(sep, { key: 'ArrowLeft', ctrlKey: true })
    expect(record().cols).toBeUndefined()
  })

  it('a drag paints the width live and commits ONCE, on release; a click commits nothing', () => {
    render(<Host cols={2} regionW={1200} boxes={TWO} />)
    const sep = width()
    fireEvent.pointerDown(sep, { button: 0, clientX: 706, pointerId: 1 })
    fireEvent.pointerMove(window, { clientX: 656, pointerId: 1 })
    expect(region().style.getPropertyValue('--cockpit-col-log'), 'the drag did not repaint the region').toBe('min(538px, 50%)')
    expect(record().cols, 'committed before release').toBeUndefined()
    fireEvent.pointerMove(window, { clientX: 606, pointerId: 1 })
    fireEvent.pointerUp(window, { clientX: 606, pointerId: 1 })
    expect(record().cols).toEqual({ log: 588 })
    // Past its ceiling the drag stops at half the region.
    fireEvent.pointerDown(sep, { button: 0, clientX: 606, pointerId: 2 })
    fireEvent.pointerMove(window, { clientX: 100, pointerId: 2 })
    fireEvent.pointerUp(window, { clientX: 100, pointerId: 2 })
    expect(record().cols).toEqual({ log: 600 })
    const before = localStorage.getItem(KEY)
    fireEvent.pointerDown(sep, { button: 0, clientX: 500, pointerId: 3 })
    fireEvent.pointerUp(window, { clientX: 500, pointerId: 3 })
    expect(localStorage.getItem(KEY), 'a click is half of a double-click, not a move').toBe(before)
  })

  it('a cancelled drag puts back exactly what was painted before it — here, no token at all', () => {
    render(<Host cols={2} regionW={1200} boxes={TWO} />)
    fireEvent.pointerDown(width(), { button: 0, clientX: 706, pointerId: 1 })
    fireEvent.pointerMove(window, { clientX: 606, pointerId: 1 })
    expect(region().style.getPropertyValue('--cockpit-col-log')).toBe('min(588px, 50%)')
    fireEvent.pointerCancel(window, { pointerId: 1 })
    expect(region().style.getPropertyValue('--cockpit-col-log')).toBe('')
    expect(record().cols).toBeUndefined()
  })

  it('a record saved before the dividers existed opens on the stock columns: no token on the region', () => {
    localStorage.setItem(KEY, JSON.stringify({ v: 1, state: { spots: 'docked' }, share: { spots: 1.2, needed: 0.8 } }))
    render(<Host cols={3} regionW={1800} boxes={THREE} />)
    for (const t of ['--cockpit-col-a', '--cockpit-col-b', '--cockpit-col-log']) {
      expect(region().style.getPropertyValue(t), t).toBe('')
    }
    expect(aria(width())[0]).toBe(624)
  })

  it('a stored width wider than this window is painted capped by the layout, announced as shown, and kept for a bigger window', () => {
    // Set on a 3440 monitor; this region is 1200 wide, so the layout shows it at 600 (half).
    localStorage.setItem(KEY, JSON.stringify({ v: 2, state: {}, share: {}, cols: { log: 1500 } }))
    render(<Host cols={2} regionW={1200} boxes={[[0, 588], [600, 600]]} />)
    expect(region().style.getPropertyValue('--cockpit-col-log')).toBe('min(1500px, 50%)')
    expect(aria(width())).toEqual([600, 336, 600])
    expect(record().cols, 'a load never rewrites the preference').toEqual({ log: 1500 })
    // A key steps from what is SHOWN, never from the stored 1500.
    fireEvent.keyDown(width(), { key: 'ArrowRight' })
    expect(record().cols).toEqual({ log: 584 })
  })

  it('a window resize re-measures: the ceiling follows the region, and the next key steps from what is shown', () => {
    const { rerender } = render(<Host cols={2} regionW={1600} boxes={[[0, 900], [912, 688]]} />)
    expect(aria(width())).toEqual([688, 336, 800])
    // The window shrinks: the region is 1100 now and the log shows at its 550 cap.
    rerender(<Host cols={2} regionW={1100} boxes={[[0, 538], [550, 550]]} />)
    resized()
    expect(aria(width())).toEqual([550, 336, 550])
    fireEvent.keyDown(width(), { key: 'ArrowLeft' })
    expect(record().cols, 'a step past the new ceiling is clamped to it').toEqual({ log: 550 })
  })

  it('⊞ Reset layout puts the stock columns back, and Undo brings the operator’s back', () => {
    render(<Host cols={3} regionW={1800} boxes={THREE} />)
    fireEvent.keyDown(width(), { key: 'ArrowLeft', shiftKey: true })
    fireEvent.keyDown(split(), { key: 'ArrowRight' })
    const mine = record().cols
    expect(Object.keys(mine).sort()).toEqual(['a', 'b', 'log'])
    fireEvent.click(screen.getByRole('button', { name: 'Reset' }))
    expect(record().cols).toBeUndefined()
    for (const t of ['--cockpit-col-a', '--cockpit-col-b', '--cockpit-col-log']) {
      expect(region().style.getPropertyValue(t), `${t} survived Reset`).toBe('')
    }
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }))
    expect(record().cols).toEqual(mine)
    expect(region().style.getPropertyValue('--cockpit-col-log')).toBe(`min(${mine.log}px, 50%)`)
  })
})

describe('the divider between the two feed columns (three columns)', () => {
  it('announces the split as measured, steps from it, and commits the pair as fr shares on the region', () => {
    render(<Host cols={3} regionW={1800} boxes={THREE} />)
    const sep = split()
    expect(sep.tabIndex).toBe(0)
    // 700 : 452 on screen.
    expect(Number(sep.getAttribute('aria-valuenow'))).toBe(Math.round((700 / 1152) * 100))
    fireEvent.keyDown(sep, { key: 'ArrowRight' })
    const [a, b] = seamShares(700 / 1152 + 0.05)
    expect(record().cols).toEqual({ a, b })
    expect(region().style.getPropertyValue('--cockpit-col-a')).toBe(`${a}fr`)
    expect(region().style.getPropertyValue('--cockpit-col-b')).toBe(`${b}fr`)
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(record().cols).toBeUndefined()
    expect(region().style.getPropertyValue('--cockpit-col-a')).toBe('')
  })

  it('a drag paints both fr tokens live and commits once on release', () => {
    render(<Host cols={3} regionW={1800} boxes={THREE} />)
    const sep = split()
    // The pair spans 0 … 1164 (the first column's left to the second's right).
    fireEvent.pointerDown(sep, { button: 0, clientY: 10, clientX: 706, pointerId: 1 })
    fireEvent.pointerMove(window, { clientX: 400, pointerId: 1 })
    const [a1, b1] = seamShares(400 / 1164)
    expect(region().style.getPropertyValue('--cockpit-col-a')).toBe(`${a1}fr`)
    expect(region().style.getPropertyValue('--cockpit-col-b')).toBe(`${b1}fr`)
    expect(record().cols).toBeUndefined()
    fireEvent.pointerUp(window, { clientX: 450, pointerId: 1 })
    const [a, b] = seamShares(450 / 1164)
    expect(record().cols).toEqual({ a, b })
  })
})

describe('the stacking tier', () => {
  it('has no column divider at all: a stack cannot be divided sideways', () => {
    function One() {
      const panels = usePanelLayout(PHONE_PANELS)
      const region = useRef<HTMLDivElement>(null)
      const a = useRef<HTMLDivElement>(null)
      return (
        <div className="cockpit-panes" ref={region}>
          <div ref={a} />
          <RegionColumnSeams region={region} cols={1} tracks={[a]} stored={panels.layout.cols} setCols={panels.setCols!} splitLabel="x" widthLabel="y" />
        </div>
      )
    }
    render(<One />)
    expect(screen.queryAllByRole('separator')).toEqual([])
  })
})
