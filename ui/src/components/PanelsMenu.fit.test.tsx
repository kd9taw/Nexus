// @vitest-environment jsdom
//
// ⊞ PANELS STAYS INSIDE THE ROOM IT HAS (PanelsMenu `fitMenu`).
//
// Two measured defects at 150–175 %: the popover hung off the window's LEFT edge (700×800 at 175 %: one of
// RTTY's nine entries reachable), and it opened DOWNWARD at the sheet's 0.7 × the window whatever room was
// left below the button, so its box ran past the window and its last entries could not be reached by its
// own scroll (CW at 1280×720 at 175 %: 617–1121 px on a 720 px window). scripts/browser-probe --desktop
// digishell holds the real geometry; this holds the arithmetic, which a probe passing "inside the window"
// cannot: the shift is applied INSIDE the zoomed tree, so it must be divided by the zoom, and the room it
// sizes the menu to is the room between the button and the parked dock. jsdom lays nothing out: every box
// is stubbed in window px, and the popover's layout width is its rendered width over ZOOM.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { PanelsMenu } from './PanelsMenu'

const ZOOM = 1.75
const WIN_W = 700
const WIN_H = 720
type Box = { top: number; left: number; width: number; height: number }
let boxes: Record<string, Box> = {}
const rect = ({ top, left, width, height }: Box) =>
  ({ x: left, y: top, top, left, width, height, right: left + width, bottom: top + height, toJSON() {} }) as DOMRect

beforeEach(() => {
  vi.stubGlobal('innerWidth', WIN_W)
  vi.stubGlobal('innerHeight', WIN_H)
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    for (const [cls, b] of Object.entries(boxes)) if (this.classList.contains(cls)) return rect(b)
    return rect({ top: 0, left: 0, width: 0, height: 0 })
  })
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
    const b = Object.entries(boxes).find(([cls]) => this.classList.contains(cls))?.[1]
    return b ? b.width / ZOOM : 0
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  boxes = {}
})

function open(shellBox: Box) {
  boxes['shell'] = shellBox
  render(
    <main className="shell" style={{ overflowY: 'auto' }}>
      <div className="head">
        <PanelsMenu items={[{ id: 'a', label: 'A', state: 'docked' }]} onToggle={() => {}} onUndo={() => {}} canUndo={false} onReset={() => {}} />
      </div>
      <div className="cockpit-txdock" />
    </main>,
  )
  fireEvent.click(document.querySelector('.panels-menu-btn')!)
  return document.querySelector<HTMLElement>('.panels-menu-pop')!
}

describe('the ⊞ Panels popover is fitted into the room it has', () => {
  it('shifted back inside the window by its overhang, 4 px × the zoom from the edge, in LAYOUT px', () => {
    // Hangs 100 window px off the left edge. The shift lands it 4 × 1.75 = 7 px from the edge: 107 window
    // px, applied inside the zoomed tree as 107 / 1.75 layout px. Without the division it overshoots to 194.
    boxes = { 'panels-menu-btn': { top: 100, left: 150, width: 60, height: 38 }, 'panels-menu-pop': { top: 145, left: -100, width: 385, height: 200 },
      'cockpit-txdock': { top: 600, left: 0, width: 700, height: 120 } }
    const pop = open({ top: 50, left: 0, width: WIN_W, height: WIN_H - 50 })
    expect(pop.style.translate.endsWith('px 0'), pop.style.translate).toBe(true)
    expect(parseFloat(pop.style.translate), 'the shift is not in layout px: at 175 % it overshoots the edge').toBeCloseTo(107 / ZOOM, 6)
  })

  it('that fits below the button is left alone: no shift, no cap, opening down as always', () => {
    boxes = { 'panels-menu-btn': { top: 100, left: 300, width: 60, height: 38 }, 'panels-menu-pop': { top: 145, left: 100, width: 300, height: 200 },
      'cockpit-txdock': { top: 600, left: 0, width: 700, height: 120 } }
    const pop = open({ top: 50, left: 0, width: WIN_W, height: WIN_H - 50 })
    expect(pop.style.translate).toBe('')
    expect(pop.style.maxHeight).toBe('')
    expect(pop.hasAttribute('data-up')).toBe(false)
  })

  it('too tall for the room below, and less room above: capped at the room down to the parked dock', () => {
    // Button 100–138, the menu 7 px under it at 145, the parked dock from 600: below = 600 − 138 − 7 − 7 = 448.
    boxes = { 'panels-menu-btn': { top: 100, left: 300, width: 60, height: 38 }, 'panels-menu-pop': { top: 145, left: 100, width: 300, height: 504 },
      'cockpit-txdock': { top: 600, left: 0, width: 700, height: 120 } }
    const pop = open({ top: 50, left: 0, width: WIN_W, height: WIN_H - 50 })
    expect(pop.hasAttribute('data-up')).toBe(false)
    expect(parseFloat(pop.style.maxHeight), 'the menu runs under the parked dock or past the window').toBeCloseTo(448 / ZOOM, 6)
  })

  it('too tall for the room below, and more room above: it opens upward, capped at the room above', () => {
    // Button 500–538 (low in the window), menu 7 px under it; the dock from 600 leaves 48 px below.
    // Above, the shell's top at 50: 500 − 50 − 7 − 7 = 436.
    boxes = { 'panels-menu-btn': { top: 500, left: 300, width: 60, height: 38 }, 'panels-menu-pop': { top: 545, left: 100, width: 300, height: 504 },
      'cockpit-txdock': { top: 600, left: 0, width: 700, height: 120 } }
    const pop = open({ top: 50, left: 0, width: WIN_W, height: WIN_H - 50 })
    expect(pop.hasAttribute('data-up'), 'it kept opening down into 48 px').toBe(true)
    expect(parseFloat(pop.style.maxHeight)).toBeCloseTo(436 / ZOOM, 6)
  })
})
