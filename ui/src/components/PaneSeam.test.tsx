// @vitest-environment jsdom
//
// THE PANE SEAM (layout L1) — the one divider, by itself. The cockpits' and Tempo's own wiring is
// tested where it renders (OperateCockpit.panels, PhoneCockpit.structure/boards,
// CwCockpit.structure, App.tempoRails); this file holds what every divider of a kind does.
//
// jsdom lays nothing out, so every box here is stubbed, and one case stubs a LAYOUT: a strip whose
// rendered height follows the variable the way the flex column does, capped by a neighbour.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, fireEvent, act } from '@testing-library/react'
import { createRef } from 'react'
import { PaneSeam } from './PaneSeam'
import { seamShares } from '../features/panelState'

/** Every observer this file's components create, with what it watches, so a test can resize ONE. */
let observers: Array<{ cb: () => void; els: Element[] }> = []
beforeEach(() => {
  localStorage.clear()
  document.documentElement.style.removeProperty('--vh-eff')
  document.documentElement.style.removeProperty('--ui-zoom')
  observers = []
  globalThis.ResizeObserver = class {
    entry: { cb: () => void; els: Element[] }
    constructor(cb: () => void) {
      this.entry = { cb, els: [] }
      observers.push(this.entry)
    }
    observe(el: Element) {
      this.entry.els.push(el)
    }
    unobserve() {}
    disconnect() {
      this.entry.els = []
    }
  } as unknown as typeof ResizeObserver
})
afterEach(() => cleanup())

function rectOf(el: HTMLElement, box: () => { top?: number; left?: number; width?: number; height?: number }) {
  el.getBoundingClientRect = () => {
    const { top = 0, left = 0, width = 800, height = 0 } = box()
    return { top, left, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) } as DOMRect
  }
}
/** Fire the observer(s) watching `el`, as the browser does after a resize. */
function resized(el: Element) {
  act(() => observers.filter((o) => o.els.includes(el)).forEach((o) => o.cb()))
}

describe('a STRIP divider', () => {
  /** A container `span` tall and a strip in it; the strip lays out like `flex: 0 1 var(--h)` with
   *  `min-height: minPx` and a NEIGHBOUR that stops it at `capPx`, or not laid out at all. */
  function mount(opts: { span: number; stored?: string; minPx?: number; capPx?: number; layout?: boolean; zoom?: number }) {
    if (opts.stored != null) localStorage.setItem('nexus.split.test.h', opts.stored)
    const target = document.createElement('div')
    const strip = document.createElement('section')
    target.appendChild(strip)
    document.body.appendChild(target)
    let span = opts.span
    rectOf(target, () => ({ height: span * (opts.zoom ?? 1) }))
    if (opts.zoom) Object.defineProperty(target, 'currentCSSZoom', { value: opts.zoom, configurable: true })
    if (opts.layout) {
      rectOf(strip, () => {
        const v = target.style.getPropertyValue('--h')
        const basis = v.endsWith('%') ? (parseFloat(v) / 100) * span : parseFloat(v) || 0
        const h = Math.min(opts.capPx ?? Infinity, Math.max(opts.minPx ?? 0, basis))
        return { height: h * (opts.zoom ?? 1) }
      })
    }
    const view = render(
      <PaneSeam
        axis="y"
        varName="--h"
        strip={{ current: strip }}
        storageKey="nexus.split.test.h"
        min={100}
        max={420}
        defaultPct={22}
        label="test height"
      />,
    )
    const sep = view.getByRole('separator', { name: 'test height' })
    return { target, strip, sep, setSpan: (s: number) => (span = s), view }
  }
  const pct = (target: HTMLElement) => target.style.getPropertyValue('--h')
  const aria = (sep: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => sep.getAttribute(a))

  it('clamps a stored % against the CURRENT box at load, and never rewrites the stored value', () => {
    // Legal on some taller container, not on this 1000 px one with a 420 px cap.
    const { target } = mount({ span: 1000, stored: '90' })
    expect(pct(target)).toBe('42%')
    expect(localStorage.getItem('nexus.split.test.h')).toBe('90')
  })

  it('applies the default % when nothing is stored', () => {
    const { target } = mount({ span: 1000 })
    expect(pct(target)).toBe('22%')
  })

  it('re-fits the STORED preference on every resize: clamped in a box too small for it, back again in a big one', () => {
    const { target, sep, setSpan } = mount({ span: 400, stored: '50' })
    expect(pct(target)).toBe('50%') // 200 px of 400
    setSpan(1000)
    resized(target)
    expect(pct(target), 'a stored value larger than the new box is clamped').toBe('42%')
    expect(aria(sep)).toEqual(['420', '100', '420'])
    setSpan(400)
    resized(target)
    expect(pct(target), 'the preference came back when the room did').toBe('50%')
    expect(localStorage.getItem('nexus.split.test.h')).toBe('50')
  })

  it('a neighbour-set ceiling the declared clamps cannot see is MEASURED, so no end of the drag is dead', () => {
    // The strip stops at 300 px (a sibling's floor) under a declared 420 px.
    const { target, sep } = mount({ span: 1000, stored: '40', layout: true, minPx: 120, capPx: 300 })
    expect(pct(target), 'opened past what the layout will render').toBe('30%')
    expect(aria(sep)).toEqual(['300', '120', '300'])
    fireEvent.keyDown(sep, { key: 'End' })
    expect(sep.getAttribute('aria-valuenow')).toBe('300')
    // One arrow up from the ceiling moves the strip at once — no presses spent in dead travel.
    fireEvent.keyDown(sep, { key: 'ArrowUp' })
    expect(pct(target)).toBe(`${(284 / 1000) * 100}%`)
    fireEvent.keyDown(sep, { key: 'Home' })
    expect(sep.getAttribute('aria-valuenow'), 'the sheet’s 120 px floor, measured').toBe('120')
    // The probe leaves nothing behind: the variable is what the divider set.
    expect(pct(target)).toBe('12%')
  })

  it('a drag paints live, commits ONCE on release, and follows the pointer in CSS px at any zoom', () => {
    const { target, sep } = mount({ span: 1000, zoom: 0.5 })
    const writes = vi.spyOn(Storage.prototype, 'setItem')
    const splitWrites = () => writes.mock.calls.filter(([k]) => k === 'nexus.split.test.h').length
    // 22 % = 220 CSS px. A 50 px pointer move at zoom 0.5 is 100 CSS px.
    fireEvent.pointerDown(sep, { clientY: 300, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientY: 325, pointerId: 1 })
    fireEvent.pointerMove(window, { clientY: 350, pointerId: 1 })
    expect(pct(target)).toBe(`${(320 / 1000) * 100}%`)
    expect(splitWrites(), 'a move wrote the stored split').toBe(0)
    expect(document.body.classList.contains('resizing')).toBe(true)
    fireEvent.pointerUp(window, { clientY: 350, pointerId: 1 })
    expect(splitWrites()).toBe(1)
    expect(localStorage.getItem('nexus.split.test.h')).toBe(String((320 / 1000) * 100))
    expect(sep.getAttribute('aria-valuenow')).toBe('320')
    expect(document.body.classList.contains('resizing')).toBe(false)
    writes.mockRestore()
  })

  it('a click without a move changes and stores nothing (half of a double-click)', () => {
    const { target, sep } = mount({ span: 1000, stored: '30' })
    fireEvent.pointerDown(sep, { clientY: 300, pointerId: 1, button: 0 })
    fireEvent.pointerUp(window, { clientY: 300, pointerId: 1 })
    expect(pct(target)).toBe('30%')
    expect(localStorage.getItem('nexus.split.test.h')).toBe('30')
  })

  it('a cancelled drag puts the strip back and stores nothing', () => {
    const { target, sep } = mount({ span: 1000, stored: '30' })
    fireEvent.pointerDown(sep, { clientY: 300, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientY: 380, pointerId: 1 })
    expect(pct(target)).toBe('38%')
    fireEvent(window, new Event('pointercancel'))
    expect(pct(target)).toBe('30%')
    expect(localStorage.getItem('nexus.split.test.h')).toBe('30')
  })

  it('a drag still in flight when the divider goes is cancelled with it — a later release commits nothing', () => {
    const { target, sep, view } = mount({ span: 1000, stored: '30' })
    fireEvent.pointerDown(sep, { clientY: 300, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientY: 380, pointerId: 1 })
    view.unmount()
    expect(pct(target), 'the strip was left at the half-dragged size').toBe('30%')
    expect(document.body.classList.contains('resizing')).toBe(false)
    fireEvent.pointerUp(window, { clientY: 380, pointerId: 1 })
    expect(localStorage.getItem('nexus.split.test.h')).toBe('30')
  })

  it('a hidden box (a keep-alive host at 0×0) gets the stored % raw, and the box it is shown in clamps it', () => {
    const { target, sep, setSpan } = mount({ span: 0, stored: '75' })
    expect(pct(target)).toBe('75%')
    expect(sep.getAttribute('aria-valuenow'), 'no value is invented for a box that is not there').toBeNull()
    setSpan(1000)
    resized(target)
    expect(pct(target)).toBe('42%')
  })

  it('the arrows, Home/End and the resets act through the same clamps', () => {
    const { target, sep } = mount({ span: 1000 })
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(pct(target)).toBe(`${(236 / 1000) * 100}%`)
    fireEvent.keyDown(sep, { key: 'ArrowDown', shiftKey: true })
    expect(sep.getAttribute('aria-valuenow')).toBe('300')
    fireEvent.keyDown(sep, { key: 'End' })
    expect(pct(target)).toBe('42%')
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(pct(target), 'past the end stays at the end').toBe('42%')
    fireEvent.keyDown(sep, { key: 'Home' })
    expect(pct(target)).toBe('10%')
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(pct(target)).toBe('22%')
    expect(localStorage.getItem('nexus.split.test.h')).toBe('22')
    fireEvent.keyDown(sep, { key: 'End' })
    fireEvent.doubleClick(sep)
    expect(pct(target)).toBe('22%')
  })

  it('leaves keys that are not its own alone', () => {
    const { target, sep } = mount({ span: 1000 })
    for (const init of [{ key: 'ArrowLeft' }, { key: 'Enter' }, { key: ' ' }, { key: 'ArrowDown', ctrlKey: true }, { key: 'Backspace', altKey: true }]) {
      const ev = new KeyboardEvent('keydown', { ...init, bubbles: true, cancelable: true })
      sep.dispatchEvent(ev)
      expect(ev.defaultPrevented, JSON.stringify(init)).toBe(false)
    }
    expect(pct(target)).toBe('22%')
  })
})

describe('a STRIP divider over a strip the SHEET sizes until it is moved (defaultPct null)', () => {
  // SSTV's picture stage (layout L6): a grower no fixed share reproduces at every window. Until the
  // divider is moved nothing is painted and the sheet's own size stands; moved, the strip is marked
  // `data-sized`, the attribute its sized sheet rule keys on, and a reset gives the grower back.
  /** A container `span` tall and a strip in it that renders `own` px while unmarked (the sheet's
   *  grower), and once marked lays out like `flex: 0 1 var(--h)` with `min-height: minPx` under a
   *  neighbour that stops it at `capPx`. */
  function mount(opts: { span: number; own: number; stored?: string; minPx?: number; capPx?: number }) {
    if (opts.stored != null) localStorage.setItem('nexus.split.test.h', opts.stored)
    const target = document.createElement('div')
    const strip = document.createElement('section')
    target.appendChild(strip)
    document.body.appendChild(target)
    let own = opts.own
    rectOf(target, () => ({ height: opts.span }))
    rectOf(strip, () => {
      if (!strip.hasAttribute('data-sized')) return { height: own }
      const v = target.style.getPropertyValue('--h')
      const basis = v.endsWith('%') ? (parseFloat(v) / 100) * opts.span : parseFloat(v) || 0
      return { height: Math.min(opts.capPx ?? Infinity, Math.max(opts.minPx ?? 0, basis)) }
    })
    const view = render(
      <PaneSeam
        axis="y"
        varName="--h"
        strip={{ current: strip }}
        storageKey="nexus.split.test.h"
        min={100}
        max={420}
        defaultPct={null}
        label="stage height"
      />,
    )
    const sep = view.getByRole('separator', { name: 'stage height' })
    return { target, strip, sep, setOwn: (px: number) => (own = px), view }
  }
  const pct = (target: HTMLElement) => target.style.getPropertyValue('--h')
  const sized = (strip: HTMLElement) => strip.hasAttribute('data-sized')
  const aria = (sep: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => sep.getAttribute(a))

  it('paints, marks and stores nothing, and announces the size the sheet gives the strip', () => {
    // The range is the SIZED shape's, measured: its 120 px floor and a neighbour's 350 px stop.
    const { target, strip, sep } = mount({ span: 1000, own: 280, minPx: 120, capPx: 350 })
    expect(pct(target)).toBe('')
    expect(sized(strip)).toBe(false)
    expect(localStorage.getItem('nexus.split.test.h')).toBeNull()
    expect(aria(sep)).toEqual(['280', '120', '350'])
  })

  it('follows the sheet’s size as the strip grows or shrinks by itself, still painting nothing', () => {
    const { target, strip, sep, setOwn } = mount({ span: 1000, own: 280, minPx: 120 })
    setOwn(330)
    resized(strip)
    expect(sep.getAttribute('aria-valuenow')).toBe('330')
    expect(pct(target)).toBe('')
    expect(sized(strip)).toBe(false)
  })

  it('a key sizes it FROM where it stands: painted, marked and stored', () => {
    const { target, strip, sep } = mount({ span: 1000, own: 280, minPx: 120 })
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(pct(target)).toBe(`${(296 / 1000) * 100}%`)
    expect(sized(strip)).toBe(true)
    expect(sep.getAttribute('aria-valuenow')).toBe('296')
    expect(localStorage.getItem('nexus.split.test.h')).toBe(String((296 / 1000) * 100))
    fireEvent.keyDown(sep, { key: 'Home' })
    expect(pct(target)).toBe('12%')
  })

  it('a reset gives the sheet its size back — the variable, the mark and the stored size all go — and a reload stays so', () => {
    const { target, strip, sep, view } = mount({ span: 1000, own: 280, minPx: 120 })
    fireEvent.keyDown(sep, { key: 'End' })
    expect(pct(target)).toBe('42%')
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(pct(target)).toBe('')
    expect(sized(strip)).toBe(false)
    expect(sep.getAttribute('aria-valuenow')).toBe('280')
    fireEvent.keyDown(sep, { key: 'End' })
    fireEvent.doubleClick(sep)
    expect(pct(target)).toBe('')
    expect(sized(strip)).toBe(false)
    view.unmount()
    const again = mount({ span: 1000, own: 280, minPx: 120 })
    expect(pct(again.target), 'a reload after a reset came back sized').toBe('')
    expect(sized(again.strip)).toBe(false)
  })

  it('a stored size restores the SIZED strip, clamped against the current box and never rewritten', () => {
    const { target, strip, sep } = mount({ span: 1000, own: 280, minPx: 120, stored: '30' })
    expect(pct(target)).toBe('30%')
    expect(sized(strip)).toBe(true)
    expect(aria(sep)).toEqual(['300', '120', '420'])
    cleanup()
    localStorage.clear()
    const big = mount({ span: 1000, own: 280, minPx: 120, stored: '90' })
    expect(pct(big.target)).toBe('42%')
    expect(sized(big.strip)).toBe(true)
    expect(localStorage.getItem('nexus.split.test.h')).toBe('90')
  })

  it('a hidden box gets a stored % raw and marked, and with nothing stored stays unpainted', () => {
    const stored = mount({ span: 0, own: 0, stored: '30' })
    expect(pct(stored.target)).toBe('30%')
    expect(sized(stored.strip)).toBe(true)
    cleanup()
    localStorage.clear()
    const stock = mount({ span: 0, own: 0 })
    expect(pct(stock.target)).toBe('')
    expect(sized(stock.strip)).toBe(false)
    expect(stock.sep.getAttribute('aria-valuenow'), 'no value is invented for a box that is not there').toBeNull()
  })

  it('a drag from the sheet’s size starts where the strip stands; a cancel gives it back; a release commits once', () => {
    const { target, strip, sep } = mount({ span: 1000, own: 280, minPx: 120 })
    fireEvent.pointerDown(sep, { clientY: 300, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientY: 350, pointerId: 1 })
    expect(pct(target), 'the grab jumped the strip').toBe(`${(330 / 1000) * 100}%`)
    expect(sized(strip)).toBe(true)
    fireEvent(window, new Event('pointercancel'))
    expect(pct(target)).toBe('')
    expect(sized(strip)).toBe(false)
    expect(localStorage.getItem('nexus.split.test.h')).toBeNull()
    const writes = vi.spyOn(Storage.prototype, 'setItem')
    fireEvent.pointerDown(sep, { clientY: 300, pointerId: 2, button: 0 })
    fireEvent.pointerMove(window, { clientY: 330, pointerId: 2 })
    fireEvent.pointerMove(window, { clientY: 350, pointerId: 2 })
    expect(writes.mock.calls.filter(([k]) => k === 'nexus.split.test.h')).toHaveLength(0)
    fireEvent.pointerUp(window, { clientY: 350, pointerId: 2 })
    expect(writes.mock.calls.filter(([k]) => k === 'nexus.split.test.h')).toHaveLength(1)
    expect(localStorage.getItem('nexus.split.test.h')).toBe(String((330 / 1000) * 100))
    expect(sized(strip)).toBe(true)
    writes.mockRestore()
  })
})

describe('a STRIP divider over a strip that comes AFTER it (after)', () => {
  // Operate's Tx1–Tx6 machine sits under its divider (layout L5): moving the divider down shrinks
  // it, and its ref is a FOLLOWING sibling's, not attached when the divider's layout effect runs.
  /** A 1000 px column holding the divider and then the strip, in one React tree; the strip lays out
   *  like `flex: 0 1 var(--h)` held between 50 and 400 px. */
  function mount(stored?: string) {
    if (stored != null) localStorage.setItem('nexus.split.test.after', stored)
    const rect = (height: number) =>
      ({ top: 0, left: 0, width: 800, height, right: 800, bottom: height, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (this.dataset.box === 'col') return rect(1000)
      if (this.dataset.box === 'strip') {
        const v = (this.parentElement as HTMLElement).style.getPropertyValue('--h')
        const basis = v.endsWith('%') ? parseFloat(v) * 10 : v === '' ? 200 : parseFloat(v)
        return rect(Math.min(400, Math.max(50, basis)))
      }
      return rect(0)
    })
    const ref = createRef<HTMLElement>()
    const view = render(
      <div data-box="col">
        <PaneSeam axis="y" varName="--h" strip={ref} storageKey="nexus.split.test.after" min={50} max={400} defaultPct={20} label="tx height" after />
        <section data-box="strip" ref={ref} />
      </div>,
    )
    const col = view.container.querySelector<HTMLElement>('[data-box="col"]')!
    return { col, sep: view.getByRole('separator', { name: 'tx height' }) }
  }
  const pct = (col: HTMLElement) => col.style.getPropertyValue('--h')
  afterEach(() => vi.restoreAllMocks())

  it('finds the strip after it on mount and fits the stored size before anything is touched', () => {
    const { col, sep } = mount('30')
    expect(pct(col), 'the divider never found the strip below it').toBe('30%')
    expect(['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => sep.getAttribute(a))).toEqual(['300', '50', '400'])
  })

  it('the arrows move the divider the way they point, so down shrinks the strip below it', () => {
    const { col, sep } = mount('30')
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(pct(col)).toBe(`${(284 / 1000) * 100}%`)
    fireEvent.keyDown(sep, { key: 'ArrowUp', shiftKey: true })
    expect(pct(col)).toBe(`${(348 / 1000) * 100}%`)
    fireEvent.keyDown(sep, { key: 'Home' })
    expect(sep.getAttribute('aria-valuenow')).toBe('50')
    fireEvent.keyDown(sep, { key: 'End' })
    expect(sep.getAttribute('aria-valuenow')).toBe('400')
  })

  it('a drag down shrinks it by exactly the pointer’s travel, and commits once on release', () => {
    const { col, sep } = mount('30')
    fireEvent.pointerDown(sep, { clientY: 500, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientY: 550, pointerId: 1 })
    expect(pct(col)).toBe('25%')
    fireEvent.pointerUp(window, { clientY: 550, pointerId: 1 })
    expect(localStorage.getItem('nexus.split.test.after')).toBe('25')
  })
})

describe('a SPLIT divider', () => {
  function mount(axis: 'x' | 'y', columns = false, scale?: number, columnVars?: [string, string], floors?: [number, number]) {
    const a = document.createElement('div')
    const b = document.createElement('div')
    const grid = document.createElement('div')
    rectOf(a, () => (axis === 'y' ? { top: 100, height: 300 } : { left: 100, width: 300, height: 400 }))
    rectOf(b, () => (axis === 'y' ? { top: 410, height: 200 } : { left: 410, width: 200, height: 400 }))
    const onCommit = vi.fn()
    const onReset = vi.fn()
    const view = render(
      <PaneSeam
        above={{ current: a }}
        below={{ current: b }}
        axis={axis}
        columnsOn={columns ? { current: grid } : undefined}
        varName={columns ? '--col' : '--share'}
        onCommit={onCommit}
        onReset={onReset}
        label="pair"
        scale={scale}
        columnVars={columnVars}
        floors={floors}
      />,
    )
    return { a, b, grid, onCommit, onReset, sep: view.getByRole('separator', { name: 'pair' }) }
  }

  it('announces the split as measured on screen, in percent of the pair', () => {
    const { sep } = mount('y')
    expect(['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((x) => sep.getAttribute(x))).toEqual(['60', '8', '93'])
    expect(sep.getAttribute('aria-orientation')).toBe('horizontal')
  })

  it('re-announces the split when a pane changes size with no window resize (the UI scale, a neighbour)', async () => {
    const { a, sep } = mount('y')
    expect(sep.getAttribute('aria-valuenow')).toBe('60')
    // The pane above stops at a floor: 200 of 400 now, and nothing resized the window.
    rectOf(a, () => ({ top: 100, height: 200 }))
    resized(a)
    await act(async () => {
      await new Promise((r) => requestAnimationFrame(() => r(null)))
    })
    expect(sep.getAttribute('aria-valuenow'), 'the divider still announces the split it opened with').toBe('50')
  })

  it('a drag maps the pointer to its place in the pair, paints live, and commits once', () => {
    const { a, b, sep, onCommit } = mount('y')
    fireEvent.pointerDown(sep, { clientY: 405, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientY: 355, pointerId: 1 })
    const [a1, b1] = seamShares((355 - 100) / 510)
    expect(a.style.getPropertyValue('--share')).toBe(String(a1))
    expect(b.style.getPropertyValue('--share')).toBe(String(b1))
    expect(onCommit).not.toHaveBeenCalled()
    fireEvent.pointerUp(window, { clientY: 355, pointerId: 1 })
    expect(onCommit).toHaveBeenCalledTimes(1)
    expect(onCommit).toHaveBeenCalledWith(a1, b1)
  })

  it('in column mode paints fr tokens on the grid, and a cancel puts back exactly what was there', () => {
    const { grid, sep, onCommit } = mount('x', true)
    grid.style.setProperty('--col-a', '1.2fr')
    fireEvent.pointerDown(sep, { clientX: 405, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientX: 460, pointerId: 1 })
    const [a1, b1] = seamShares((460 - 100) / 510)
    expect(grid.style.getPropertyValue('--col-a')).toBe(`${a1}fr`)
    expect(grid.style.getPropertyValue('--col-b')).toBe(`${b1}fr`)
    fireEvent(window, new Event('pointercancel'))
    expect(grid.style.getPropertyValue('--col-a')).toBe('1.2fr')
    expect(grid.style.getPropertyValue('--col-b'), 'a token the drag added is removed again').toBe('')
    expect(onCommit).not.toHaveBeenCalled()
  })

  it('with a scale, paints each pane’s GROW (share × scale) and commits the pair’s own shares', () => {
    // A pair of unequal stock weights (JS8: Activity 2, Band activity 1 — mean 1.5) keeps its
    // stock total of grow while the divider moves, so the other fill panes in its column do not.
    const { a, b, sep, onCommit } = mount('y', false, 1.5)
    fireEvent.pointerDown(sep, { clientY: 405, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientY: 355, pointerId: 1 })
    const [a1, b1] = seamShares((355 - 100) / 510)
    expect(Number(a.style.getPropertyValue('--share'))).toBeCloseTo(a1 * 1.5, 10)
    expect(Number(b.style.getPropertyValue('--share'))).toBeCloseTo(b1 * 1.5, 10)
    expect(Number(a.style.getPropertyValue('--share')) + Number(b.style.getPropertyValue('--share'))).toBeCloseTo(3, 10)
    fireEvent.pointerUp(window, { clientY: 355, pointerId: 1 })
    expect(onCommit).toHaveBeenCalledWith(a1, b1)
  })

  it('in column mode a scale multiplies the fr tokens too, so the pair keeps its fr total', () => {
    // Operate's Classic (layout L5): a pair of columns beside a third fr column. Painted at the
    // pair's own total, the third column keeps its width; painted as shares summing to 2, the
    // first step of the Rx Frequency column / Stations divider narrowed Band Activity.
    const { grid, sep, onCommit } = mount('x', true, 0.835)
    fireEvent.pointerDown(sep, { clientX: 405, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientX: 460, pointerId: 1 })
    const [a1, b1] = seamShares((460 - 100) / 510)
    expect(parseFloat(grid.style.getPropertyValue('--col-a'))).toBeCloseTo(a1 * 0.835, 10)
    expect(parseFloat(grid.style.getPropertyValue('--col-b'))).toBeCloseTo(b1 * 0.835, 10)
    expect(parseFloat(grid.style.getPropertyValue('--col-a')) + parseFloat(grid.style.getPropertyValue('--col-b'))).toBeCloseTo(1.67, 10)
    fireEvent.pointerUp(window, { clientX: 460, pointerId: 1 })
    expect(onCommit, 'the record still takes the pair’s own shares').toHaveBeenCalledWith(a1, b1)
  })

  it('in column mode `columnVars` names the two tokens (a column two dividers share)', () => {
    const { grid, sep } = mount('x', true, 1, ['--ba', '--q'])
    grid.style.setProperty('--q', '0.9fr')
    fireEvent.pointerDown(sep, { clientX: 405, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientX: 460, pointerId: 1 })
    const [a1, b1] = seamShares((460 - 100) / 510)
    expect(grid.style.getPropertyValue('--ba')).toBe(`${a1}fr`)
    expect(grid.style.getPropertyValue('--q')).toBe(`${b1}fr`)
    expect(grid.style.getPropertyValue('--col-a'), 'the default names are not painted').toBe('')
    fireEvent(window, new Event('pointercancel'))
    expect(grid.style.getPropertyValue('--ba')).toBe('')
    expect(grid.style.getPropertyValue('--q')).toBe('0.9fr')
  })

  it('with floors, it stops where a pane reaches its own: the announced range, End and a drag all stop there', () => {
    // 300 + 200 px on screen; the second pane floors at 150 px, so the split goes no further than
    // 350 / 500. Past it the grid would freeze that track and take the rest from a third.
    const { sep, onCommit } = mount('x', true, 1, undefined, [0, 150])
    expect(['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((x) => sep.getAttribute(x))).toEqual(['60', '8', '70'])
    fireEvent.keyDown(sep, { key: 'End' })
    expect(onCommit).toHaveBeenLastCalledWith(...seamShares(0.7))
    fireEvent.pointerDown(sep, { clientX: 405, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientX: 590, pointerId: 1 })
    fireEvent.pointerUp(window, { clientX: 590, pointerId: 1 })
    expect(onCommit).toHaveBeenLastCalledWith(...seamShares(0.7))
    // The first pane's floor, likewise, at the other end.
    cleanup()
    const other = mount('x', true, 1, undefined, [200, 0])
    expect(other.sep.getAttribute('aria-valuemin')).toBe('40')
    fireEvent.keyDown(other.sep, { key: 'Home' })
    expect(other.onCommit).toHaveBeenLastCalledWith(...seamShares(0.4))
  })

  it('the keyboard steps from where the panes ARE; Home/End stop at the share floor; reset is the host’s', () => {
    const { sep, onCommit, onReset } = mount('x')
    fireEvent.keyDown(sep, { key: 'ArrowRight' })
    expect(onCommit).toHaveBeenLastCalledWith(...seamShares(0.6 + 0.05))
    fireEvent.keyDown(sep, { key: 'ArrowLeft', shiftKey: true })
    expect(onCommit).toHaveBeenLastCalledWith(...seamShares(0.6 - 0.15))
    fireEvent.keyDown(sep, { key: 'Home' })
    expect(onCommit).toHaveBeenLastCalledWith(...seamShares(0.075))
    fireEvent.keyDown(sep, { key: 'ArrowDown' }) // across the axis
    expect(onCommit).toHaveBeenCalledTimes(3)
    fireEvent.keyDown(sep, { key: 'Backspace' })
    fireEvent.doubleClick(sep)
    expect(onReset).toHaveBeenCalledTimes(2)
  })
})

describe('a VALUE divider (the host owns the size)', () => {
  function mount(grows: 1 | -1, zoom?: number) {
    const onPaint = vi.fn()
    const onCommit = vi.fn()
    const onReset = vi.fn()
    const view = render(
      <PaneSeam
        axis="x"
        className="right"
        value={300}
        min={260}
        max={600}
        grows={grows}
        onPaint={onPaint}
        onCommit={onCommit}
        onReset={onReset}
        label="rail"
      />,
    )
    const sep = view.getByRole('separator', { name: 'rail' })
    if (zoom) Object.defineProperty(sep, 'currentCSSZoom', { value: zoom, configurable: true })
    return { sep, onPaint, onCommit, onReset }
  }

  it('keeps the class that places it, and announces the host’s value and clamps', () => {
    const { sep } = mount(-1)
    expect(sep.className).toBe('pane-splitter right')
    expect(sep.tabIndex).toBe(0)
    expect(['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((x) => sep.getAttribute(x))).toEqual(['300', '260', '600'])
  })

  it('a rail on the far side of its divider widens on the arrow pointing at it, clamped by the host’s limits', () => {
    const { sep, onCommit } = mount(-1)
    fireEvent.keyDown(sep, { key: 'ArrowLeft' })
    expect(onCommit).toHaveBeenLastCalledWith(316)
    fireEvent.keyDown(sep, { key: 'ArrowRight', shiftKey: true })
    expect(onCommit).toHaveBeenLastCalledWith(260) // 300 − 64 = 236, floored
    fireEvent.keyDown(sep, { key: 'End' })
    expect(onCommit).toHaveBeenLastCalledWith(600)
  })

  it('a drag is relative to where it started, in CSS px, painted live and committed once', () => {
    const { sep, onPaint, onCommit } = mount(-1, 0.85)
    fireEvent.pointerDown(sep, { clientX: 700, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientX: 649, pointerId: 1 })
    expect(onPaint).toHaveBeenLastCalledWith(300 + 51 / 0.85)
    expect(onCommit).not.toHaveBeenCalled()
    fireEvent.pointerUp(window, { clientX: 649, pointerId: 1 })
    expect(onCommit).toHaveBeenCalledTimes(1)
    expect(onCommit).toHaveBeenCalledWith(300 + 51 / 0.85)
  })

  it('a cancelled drag paints the value it started from', () => {
    const { sep, onPaint, onCommit } = mount(1)
    fireEvent.pointerDown(sep, { clientX: 700, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientX: 760, pointerId: 1 })
    fireEvent(window, new Event('pointercancel'))
    expect(onPaint).toHaveBeenLastCalledWith(300)
    expect(onCommit).not.toHaveBeenCalled()
  })

  it('a host whose default is NO value puts that back on a cancel (onCancel), not the size it started at', () => {
    const onCancel = vi.fn()
    const onPaint = vi.fn()
    const view = render(
      <PaneSeam axis="x" className="log" value={300} min={260} max={600} grows={-1} onPaint={onPaint} onCommit={vi.fn()} onReset={vi.fn()} onCancel={onCancel} label="log width" />,
    )
    const sep = view.getByRole('separator', { name: 'log width' })
    fireEvent.pointerDown(sep, { clientX: 700, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientX: 650, pointerId: 1 })
    fireEvent(window, new Event('pointercancel'))
    expect(onCancel).toHaveBeenCalledTimes(1)
    expect(onPaint).toHaveBeenLastCalledWith(350) // the drag's own paint, never a "restore" to 300
  })

  it('with nothing measurable (null) it announces no values, only a reset acts, and a drag does not start', () => {
    const onPaint = vi.fn()
    const onCommit = vi.fn()
    const onReset = vi.fn()
    const view = render(
      <PaneSeam axis="x" className="log" value={null} min={0} max={0} grows={-1} onPaint={onPaint} onCommit={onCommit} onReset={onReset} label="log width" />,
    )
    const sep = view.getByRole('separator', { name: 'log width' })
    expect(sep.hasAttribute('aria-valuenow')).toBe(false)
    fireEvent.keyDown(sep, { key: 'ArrowLeft' })
    expect(onCommit).not.toHaveBeenCalled()
    fireEvent.pointerDown(sep, { clientX: 700, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientX: 650, pointerId: 1 })
    expect(onPaint).not.toHaveBeenCalled()
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(onReset).toHaveBeenCalledTimes(1)
  })

  it('a secondary button does not start a drag', () => {
    const { sep, onPaint } = mount(1)
    fireEvent.pointerDown(sep, { clientX: 700, pointerId: 1, button: 2 })
    fireEvent.pointerMove(window, { clientX: 760, pointerId: 1 })
    expect(onPaint).not.toHaveBeenCalled()
  })
})

describe('the element', () => {
  it('names the gesture and its keys in the tooltip, around the caller’s label', () => {
    const ref = createRef<HTMLDivElement>()
    const view = render(<div ref={ref}><PaneSeam axis="y" className="x" value={1} min={0} max={2} grows={1} onPaint={() => {}} onCommit={() => {}} onReset={() => {}} label="scope height" /></div>)
    const title = view.getByRole('separator').getAttribute('title')!
    expect(title).toContain('scope height')
    expect(title).toMatch(/arrow keys/)
    expect(title).toMatch(/Backspace/)
  })
})
