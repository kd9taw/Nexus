// A DRAG IN A MOUNTED COCKPIT, for vitest. jsdom lays nothing out — every box is 0×0 — and a drag is
// computed from boxes, so this stubs the one layout the drag reads: the pane region at the origin, its columns
// side by side (300 px wide, 1200 tall, 12 px apart, in tree order), each pane in a column 60 px tall and 8 px
// apart (in tree order, counting only what holds a grip, so dividers take no room), Phone's left side left of
// the region, and the TX strip, the dock and FT's QSO strip below it. It says nothing about the real screen:
// the real-browser runs measure that. A drag is then real pointer events, through the cockpit's own handler.
import { act, fireEvent } from '@testing-library/react'
import { LIFT_PX } from './PaneDrag'

const COL_W = 300
const COL_GAP = 12
const COL_H = 1200
const PANE_H = 60
const PANE_GAP = 8

/** The region's columns: its children but the dividers between them and the drop layer. */
export function columnsOf(region: Element): HTMLElement[] {
  return [...region.children].filter((c): c is HTMLElement => !c.matches('[role="separator"], .pane-splitter, .pane-drop-layer'))
}

const rect = (left: number, top: number, width: number, height: number) =>
  ({ left, top, width, height, x: left, y: top, right: left + width, bottom: top + height, toJSON: () => ({}) }) as DOMRect

/** Install the stubbed layout for the region `regionSel`; returns the undo. */
export function stubLayout(regionSel: string): () => void {
  const real = Element.prototype.getBoundingClientRect
  const realW = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')!
  const realH = Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight')!
  const boxOf = (el: Element): DOMRect | null => {
    const region = document.querySelector(regionSel)
    if (!region) return null
    const cols = columnsOf(region)
    if (el === region) return rect(0, 0, cols.length * (COL_W + COL_GAP) - COL_GAP, COL_H)
    if (el.matches('.cockpit-txstrip, .cockpit-txdock, .cockpit-qso')) return rect(0, COL_H + 20, 600, 40)
    const side = document.querySelector('.cockpit-left-col')
    if (el === side) return rect(-(COL_W + COL_GAP), 0, COL_W, COL_H)
    const at = cols.indexOf(el as HTMLElement)
    if (at >= 0) return rect(at * (COL_W + COL_GAP), 0, COL_W, COL_H)
    const parent = el.parentElement
    if (!parent) return null
    const col = parent === side ? -1 : cols.indexOf(parent as HTMLElement)
    if (col < -1 || (col === -1 && parent !== side)) return null
    const panes = [...parent.children].filter((c) => c.matches('[data-pane-grip]') || c.querySelector('[data-pane-grip]'))
    const j = panes.indexOf(el)
    return j < 0 ? null : rect(col * (COL_W + COL_GAP), j * (PANE_H + PANE_GAP), COL_W, PANE_H)
  }
  Element.prototype.getBoundingClientRect = function (this: Element) {
    return boxOf(this) ?? real.call(this)
  }
  // The region's inside, where the drop layer draws (and, for the grid cockpits, three tracks wide).
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get(this: HTMLElement) {
      return this.matches(regionSel) ? 1800 : realW.get!.call(this)
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get(this: HTMLElement) {
      return this.matches(regionSel) ? COL_H : realH.get!.call(this)
    },
  })
  return () => {
    Element.prototype.getBoundingClientRect = real
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientWidth
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientHeight
  }
}

/** A pane's grip in the region (or on the left side) — not its row in ⊞ Arrange's list. */
export function gripOf(regionSel: string, id: string): HTMLElement | null {
  return document.querySelector<HTMLElement>(`${regionSel} [data-pane-grip="${id}"], .cockpit-left-col [data-pane-grip="${id}"]`)
}

/** The box a pane stands in: its grip's ancestor that is a child of a column or of the left side. */
export function paneBoxOf(regionSel: string, id: string): Element | null {
  const region = document.querySelector(regionSel)
  const grip = gripOf(regionSel, id)
  if (!region || !grip) return null
  const cols: Element[] = [...columnsOf(region), ...[document.querySelector('.cockpit-left-col')].filter((x): x is Element => x != null)]
  let node: Element = grip
  while (node.parentElement && !cols.includes(node.parentElement)) node = node.parentElement
  return node.parentElement ? node : null
}

export const centre = (r: DOMRect) => ({ x: r.left + r.width / 2, y: r.top + r.height / 2 })

/** Press `from`, lift it (a move past the threshold), then move to each point in turn; nothing released. */
export function pickUp(from: Element, ...to: Array<{ x: number; y: number }>) {
  const r = from.getBoundingClientRect()
  const x0 = r.left + Math.max(1, r.width / 2)
  const y0 = r.top + 2
  fireEvent.pointerDown(from, { clientX: x0, clientY: y0, pointerId: 7, button: 0, pointerType: 'mouse' })
  fireEvent.pointerMove(window, { clientX: x0, clientY: y0 + LIFT_PX + 4, pointerId: 7, pointerType: 'mouse' })
  for (const p of to) fireEvent.pointerMove(window, { clientX: p.x, clientY: p.y, pointerId: 7, pointerType: 'mouse' })
}

export function release(at: { x: number; y: number }) {
  fireEvent.pointerUp(window, { clientX: at.x, clientY: at.y, pointerId: 7, pointerType: 'mouse' })
}

export function pressEscape() {
  act(() => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
  })
}
