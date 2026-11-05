// @vitest-environment jsdom
//
// ⊞ PANELS STANDS IN THE ROOM IT HAS, AND NEVER OVER THE STOP BUTTON (PanelsMenu `fitMenu`).
//
// Measured at 150–175 %, on the way here: the popover hung off the window's left edge (700×800: one of RTTY's
// nine entries reachable); it opened downward at 0.7 × the window whatever room was below the button, so it ran
// past the window (CW at 1366×768: 391–929 on a 768 px window); clamped into the window it stood over Stop TX,
// which an unclamped menu had missed by accident (Phone at 1024×768 by Tab + Enter: 100 % of Phone's only stop
// button); and bounded by every surface it met it could be 26 px tall.
// scripts/browser-probe --desktop digishell holds the real geometry on a grid across the whole button. This holds
// the rule's arithmetic, which a probe passing "inside the window" cannot. jsdom lays nothing out: every box is
// stubbed in window px, the popover's layout width is its rendered width over ZOOM, and a width it is given
// narrows it from the left (it hangs from its right edge).
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { PanelsMenu } from './PanelsMenu'

const ZOOM = 1.75
const EDGE = 4 * ZOOM
const WIN_W = 700
const WIN_H = 720
const SHELL = { top: 50, left: 0, width: WIN_W, height: WIN_H - 50 }
const DOCK = { top: 600, left: 0, width: 700, height: 120 }
type Box = { top: number; left: number; width: number; height: number }
let boxes: Record<string, Box> = {}
const rect = ({ top, left, width, height }: Box) =>
  ({ x: left, y: top, top, left, width, height, right: left + width, bottom: top + height, toJSON() {} }) as DOMRect
const boxOf = (el: HTMLElement): Box | undefined => {
  const b = Object.entries(boxes).find(([cls]) => el.classList.contains(cls))?.[1]
  if (!b || !el.classList.contains('panels-menu-pop') || !el.style.width) return b
  const width = parseFloat(el.style.width) * ZOOM
  return { ...b, left: b.left + b.width - width, width }
}

beforeEach(() => {
  vi.stubGlobal('innerWidth', WIN_W)
  vi.stubGlobal('innerHeight', WIN_H)
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    return rect(boxOf(this) ?? { top: 0, left: 0, width: 0, height: 0 })
  })
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
    const b = boxOf(this)
    return b ? b.width / ZOOM : 0
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  boxes = {}
})

/** The button, the menu 7 px under it as the sheet lays it out, and the shell around them. */
function layout(btnTop: number, pop: { left: number; width: number; height: number }, extra: Record<string, Box> = {}) {
  boxes = {
    shell: SHELL, 'cockpit-txdock': DOCK,
    'panels-menu-btn': { top: btnTop, left: 300, width: 60, height: 38 },
    'panels-menu-pop': { top: btnTop + 45, ...pop },
    ...extra,
  }
}
function open({ dockSticky = true, strip = true }: { dockSticky?: boolean; strip?: boolean } = {}) {
  render(
    <main className="shell" style={{ overflowY: 'auto' }}>
      <div className="head">
        <PanelsMenu items={[{ id: 'a', label: 'A', state: 'docked' }]} onToggle={() => {}} onUndo={() => {}} canUndo={false} onReset={() => {}} />
      </div>
      {strip && (
        <div className="cockpit-txstrip" style={{ position: 'sticky' }}>
          <button type="button" className="op-btn stop">Stop TX</button>
        </div>
      )}
      <div className="cockpit-txdock" style={{ position: dockSticky ? 'sticky' : 'static' }} />
    </main>,
  )
  fireEvent.click(document.querySelector('.panels-menu-btn')!)
  return document.querySelector<HTMLElement>('.panels-menu-pop')!
}
/** The popover's shift, in LAYOUT px: [x, y]. */
const shift = (pop: HTMLElement) => {
  const [x = '0', y = '0'] = pop.style.translate.split(' ')
  return [parseFloat(x), parseFloat(y)]
}
const cap = (pop: HTMLElement) => parseFloat(pop.style.maxHeight)
const STOP = { top: 300, left: 339, width: 158, height: 52 }

describe('across: inside the window, and the stop button’s column is never the menu’s', () => {
  it('shifted back inside by its overhang, 4 px × the zoom from the edge, in LAYOUT px', () => {
    // 100 window px off the left edge, shifted 107 window px: 107 / 1.75 layout px, inside the zoomed tree.
    layout(100, { left: -100, width: 200, height: 200 })
    const pop = open({ strip: false })
    expect(shift(pop)[0], 'the shift is not in layout px: at 175 % it overshoots the edge').toBeCloseTo(107 / ZOOM, 6)
  })

  it('its right bound is the scrollport’s, inside the shell’s scrollbar, not the window’s', () => {
    // 7 px past the shell's client box, under its vertical scrollbar, the menu gave the shell a HORIZONTAL scrollbar
    // too: the scrollport lost 10 layout px and the parked keep row rose under the menu, over Esc/Stop (measured, PSK
    // at 900×600 at 175 %).
    layout(100, { left: 495, width: 195, height: 200 })
    const client = vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(function (this: HTMLElement) {
      return this.classList.contains('shell') ? 390 : 0 // 682.5 window px, of 700
    })
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
      const b = boxOf(this)
      return b ? b.width / ZOOM : 0
    })
    const pop = open({ strip: false })
    expect(shift(pop)[0], 'the menu stands under the shell’s scrollbar').toBeCloseTo((682.5 - EDGE - 690) / ZOOM, 6)
    client.mockRestore()
  })

  it('that clears the stop button and fits is left alone: no shift, no width, no cap, opening down', () => {
    layout(100, { left: 100, width: 200, height: 200 }, { 'op-btn': STOP })
    const pop = open()
    expect(pop.style.translate).toBe('')
    expect(pop.style.width).toBe('')
    expect(pop.style.maxHeight).toBe('')
    expect(pop.hasAttribute('data-up')).toBe(false)
  })

  it('over the stop button with room to its right: shifted past it, 4 px × the zoom clear', () => {
    layout(100, { left: 300, width: 180, height: 200 }, { 'op-btn': STOP })
    const pop = open()
    expect(shift(pop)[0], 'the menu stands over Stop TX').toBeCloseTo((497 + EDGE - 300) / ZOOM, 6)
  })

  it('over the stop button with room only to its left: shifted past it that way', () => {
    layout(100, { left: 50, width: 300, height: 200 }, { 'op-btn': STOP })
    const pop = open()
    expect(shift(pop)[0], 'the menu stands over Stop TX').toBeCloseTo((339 - EDGE - 350) / ZOOM, 6)
    expect(pop.style.width).toBe('')
  })

  it('too wide for either side: narrowed into the larger gap, whatever its min-width', () => {
    // Clamped at 700×800 at 175 %, a 385 px menu sat at 7..392 over Stop TX at 339..497 (measured).
    layout(100, { left: -173, width: 385, height: 200 }, { 'op-btn': STOP })
    const pop = open()
    const room = 339 - 2 * EDGE // the left gap, the larger
    expect(parseFloat(pop.style.width), 'the menu did not narrow into the gap').toBeCloseTo(room / ZOOM, 6)
    expect(pop.style.minWidth, 'the sheet’s min-width would hold it over the button').toBe('0px')
    expect(shift(pop)[0]).toBeCloseTo((339 - EDGE - 212) / ZOOM, 6)
  })

  it('finds the stop button anywhere in the cockpit, not only in the TX strip (Operate’s is in its QSO strip)', () => {
    layout(100, { left: 300, width: 180, height: 200 }, { 'op-btn': STOP })
    render(
      <main className="shell" style={{ overflowY: 'auto' }}>
        <div className="head">
          <PanelsMenu items={[{ id: 'a', label: 'A', state: 'docked' }]} onToggle={() => {}} onUndo={() => {}} canUndo={false} onReset={() => {}} />
        </div>
        <div className="cockpit-qso"><button type="button" className="op-btn stop">Stop TX</button></div>
      </main>,
    )
    fireEvent.click(document.querySelector('.panels-menu-btn')!)
    expect(shift(document.querySelector<HTMLElement>('.panels-menu-pop')!)[0]).toBeCloseTo((497 + EDGE - 300) / ZOOM, 6)
  })
})

describe('up and down: the clip ancestors and the parked dock bound it, and nothing else does', () => {
  it('too tall below, less room above: capped at the room down to the parked dock', () => {
    layout(100, { left: 100, width: 200, height: 504 })
    const pop = open()
    expect(pop.hasAttribute('data-up')).toBe(false)
    expect(cap(pop), 'the menu runs under the parked dock or past the window').toBeCloseTo((600 - EDGE - 145) / ZOOM, 6)
  })

  it('the TX strip does NOT bound it: with the stop button’s column clear, it may stand over the rest of the strip', () => {
    // Bounding it by the strip is how the 26 px menu came about.
    layout(100, { left: 100, width: 200, height: 504 }, { 'cockpit-txstrip': { top: 300, left: 0, width: 700, height: 67 } })
    const pop = open()
    expect(cap(pop), 'the strip bounds the menu').toBeCloseTo((600 - EDGE - 145) / ZOOM, 6)
  })

  it('too tall below, more room above: it opens upward, capped at the room above', () => {
    layout(420, { left: 100, width: 200, height: 504 })
    const pop = open()
    expect(pop.hasAttribute('data-up'), 'it kept opening down into the little room below').toBe(true)
    expect(cap(pop)).toBeCloseTo((420 - 7 - 50 - EDGE) / ZOOM, 6)
    expect(shift(pop)[1]).toBeCloseTo(0, 6)
  })

  it('with usable room on both sides it still opens toward the larger', () => {
    // Below 198 px, above 286 px, both over the floor.
    layout(350, { left: 100, width: 200, height: 504 })
    const pop = open()
    expect(pop.hasAttribute('data-up'), 'it opened toward the smaller room').toBe(true)
    expect(cap(pop)).toBeCloseTo((350 - 7 - 50 - EDGE) / ZOOM, 6)
  })

  it('a button at or below the top of the parked dock: it opens up and lifts, its bottom on the dock’s top', () => {
    // ⊞ under the dock, or in the padding band below the keep row, at scroll 0: what a keyboard opens it from.
    layout(610, { left: 100, width: 200, height: 300 })
    const pop = open()
    expect(pop.hasAttribute('data-up')).toBe(true)
    expect(shift(pop)[1], 'the menu stands on the dock, over its stop controls').toBeCloseTo((600 - EDGE - (610 - 7)) / ZOOM, 6)
  })

  it('a dock that is not parked (an observer’s, static) bounds nothing', () => {
    layout(100, { left: 100, width: 200, height: 504 })
    const pop = open({ dockSticky: false })
    expect(pop.style.maxHeight, 'a dock that scrolls with the screen bounded the menu').toBe('')
  })

  it('a button above the scrollport: the menu stays in it, under its top edge', () => {
    layout(-40, { left: 100, width: 200, height: 300 })
    const pop = open()
    expect(pop.hasAttribute('data-up')).toBe(false)
    expect(shift(pop)[1]).toBeCloseTo((50 + EDGE - (-40 + 45)) / ZOOM, 6)
  })
})

describe('never a sliver: with no usable room on either side it takes the whole span, over its own button', () => {
  // A 26 px menu showed less than one entry. jsdom computes no font size: 6em here is 6 × 16 × 1.75 = 168 px.
  const TALL_SHELL = { top: 400, left: 0, width: WIN_W, height: WIN_H - 400 }
  const LOW_DOCK = { top: 560, left: 0, width: 700, height: 160 }
  it('both sides under the floor: the whole span from the scrollport’s top to the dock’s', () => {
    layout(470, { left: 100, width: 200, height: 504 }, { shell: TALL_SHELL, 'cockpit-txdock': LOW_DOCK })
    const pop = open()
    const span = 560 - 400 - 2 * EDGE
    expect(cap(pop), 'a sliver of a menu').toBeCloseTo(span / ZOOM, 6)
    expect(pop.hasAttribute('data-up')).toBe(true)
    expect(shift(pop)[1], 'its bottom is not on the dock’s top').toBeCloseTo((560 - EDGE - (470 - 7)) / ZOOM, 6)
  })

  it('a short menu that fits below is never given the span', () => {
    layout(470, { left: 100, width: 200, height: 30 }, { shell: TALL_SHELL, 'cockpit-txdock': LOW_DOCK })
    const pop = open()
    expect(pop.hasAttribute('data-up')).toBe(false)
    expect(pop.style.maxHeight).toBe('')
  })
})

describe('the side is latched at open: scrolling never jumps it over the button under the pointer', () => {
  const moved = (top: number) => {
    boxes['panels-menu-btn'] = { ...boxes['panels-menu-btn'], top }
    boxes['panels-menu-pop'] = { ...boxes['panels-menu-pop'], top: top + 45 }
  }
  it('opened up, it stays up when the room below grows past the room above', () => {
    layout(420, { left: 100, width: 200, height: 504 })
    const pop = open()
    expect(pop.hasAttribute('data-up')).toBe(true)
    moved(290) // above 226, below 258: at open this would go down
    fireEvent.scroll(document.querySelector('main')!)
    expect(pop.hasAttribute('data-up'), 'the menu jumped over its button while the shell scrolled').toBe(true)
    expect(cap(pop), 'the refit did not resize it').toBeCloseTo((290 - 7 - 50 - EDGE) / ZOOM, 6)
  })

  it('opened down, it stays down when the room above grows past the room below', () => {
    layout(290, { left: 100, width: 200, height: 504 })
    const pop = open()
    expect(pop.hasAttribute('data-up')).toBe(false)
    moved(320)
    fireEvent.scroll(document.querySelector('main')!)
    expect(pop.hasAttribute('data-up')).toBe(false)
    expect(cap(pop)).toBeCloseTo((600 - EDGE - 365) / ZOOM, 6)
  })

  it('…until its side has no usable room left: a latched menu never shrinks to nothing', () => {
    layout(290, { left: 100, width: 200, height: 504 })
    const pop = open()
    moved(500) // below 48 px, above 436 px
    fireEvent.scroll(document.querySelector('main')!)
    expect(pop.hasAttribute('data-up'), 'latched down into 48 px').toBe(true)
    // …and it keeps the new side.
    moved(290)
    fireEvent.scroll(document.querySelector('main')!)
    expect(pop.hasAttribute('data-up')).toBe(true)
  })

  it('a window resize refits it too, and a new open decides the side afresh', () => {
    layout(420, { left: 100, width: 200, height: 504 })
    const pop = open()
    moved(400)
    fireEvent(window, new Event('resize'))
    expect(cap(pop), 'a resize did not refit it').toBeCloseTo((400 - 7 - 50 - EDGE) / ZOOM, 6)
    const btn = document.querySelector('.panels-menu-btn')!
    fireEvent.click(btn)
    moved(100)
    fireEvent.click(btn)
    expect(document.querySelector('.panels-menu-pop')!.hasAttribute('data-up'), 'the last open’s side stuck').toBe(false)
  })

  it('the menu’s own scroll survives a refit', () => {
    // A refit measures the bare menu, which is taller and has less to scroll: the browser clamps its scrollTop.
    layout(100, { left: 100, width: 200, height: 504 })
    const pop = open()
    let top = 40
    Object.defineProperty(pop, 'scrollTop', { configurable: true, get: () => top, set: (v: number) => { top = v } })
    const measured = pop.getBoundingClientRect.bind(pop)
    pop.getBoundingClientRect = () => {
      if (!pop.style.maxHeight) top = 0
      return measured()
    }
    fireEvent.scroll(document.querySelector('main')!)
    expect(pop.scrollTop, 'the operator lost their place in the menu').toBe(40)
  })
})
