// @vitest-environment jsdom
//
// WHAT IS PARKED AT THE BOTTOM NEVER COVERS STOP TX (CockpitTxStrip decideSqueeze, cockpit-panes.css
// `.cockpit-txdock-keep[data-squeeze='on']` / `.sstv-tx-bar[data-squeeze='on']`).
//
// The strip parks at the top of the shell's scrollport and the dock's keep row (folded) or SSTV's bar at the bottom.
// On a small window at 175 % the top bar and the header leave the shell 120–250 px, and an alarm, a status message or
// SSTV's dropped-picture sentence took more: the bottom surface painted over the strip and over Stop TX (RTTY, JS8 and
// SSTV at 900×600, measured in Chrome; `scripts/browser-probe --desktop stopvisible` holds the geometry). So where the
// two do not both fit, the bottom surface is capped at the room the strip leaves and scrolls inside. jsdom loads no
// sheet and lays nothing out: the `sticky`s are injected and the heights stubbed, the cap read back from the variable
// the strip writes, as Chrome would apply it.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { CockpitTxStrip, FOLD_ROOM_EM } from './CockpitTxStrip'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)), haltTx: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

const radio = {
  dialMhz: 7.04, catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, tuning: false, txAllowed: true, atu: false, holdTxFreq: false,
} as unknown as AppSnapshot['radio']

const EM = 16 // jsdom computes no font size: the strip falls back to 16 px
const STRIP_H = 40
const DOCK_H = 200
const KEEP_H = 50
const BAR_H = 133
/** The pixel the squeeze keeps spare against the rounding of zoomed sizes. */
const SPARE = 1

let shellH = 0
let sheet: HTMLStyleElement | null = null
function injectSheet(css: string) {
  sheet = document.createElement('style')
  sheet.textContent = css
  document.head.appendChild(sheet)
}
const SHEET = '.cockpit-txstrip { position: sticky } .cockpit-txdock { position: sticky } .sstv-tx-bar { position: sticky }'
/** What a squeezed surface's cap leaves of it, as Chrome draws `max-height: var(--cockpit-txdock-room)`. */
const capped = (el: HTMLElement, natural: number) =>
  el.getAttribute('data-squeeze') === 'on' ? Math.min(natural, parseFloat(el.style.getPropertyValue('--cockpit-txdock-room'))) : natural

beforeEach(() => {
  shellH = 0
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
    return this.tagName === 'MAIN' ? shellH : 0
  })
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (this: HTMLElement) {
    if (this.classList.contains('cockpit-txdock')) {
      if (this.getAttribute('data-fold') === 'on') return 0
      // Unfolded for the fold decision's measurement, a dock holding a squeezed keep row would measure short.
      const keep = this.querySelector<HTMLElement>('.cockpit-txdock-keep')
      return DOCK_H - KEEP_H + (keep ? capped(keep, KEEP_H) : KEEP_H)
    }
    if (this.classList.contains('cockpit-txdock-keep')) return capped(this, KEEP_H)
    if (this.classList.contains('sstv-tx-bar')) return capped(this, BAR_H)
    if (this.classList.contains('cockpit-txstrip')) return STRIP_H
    return 0
  })
  // A cap does not change what the surface holds.
  vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockImplementation(function (this: HTMLElement) {
    if (this.classList.contains('cockpit-txdock-keep')) return KEEP_H
    if (this.classList.contains('sstv-tx-bar')) return BAR_H
    return 0
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  sheet?.remove()
  sheet = null
})

function Keyboard() {
  return (
    <main className="layout single rtty-cockpit">
      <CockpitTxStrip radio={radio} onSetTxEnabled={() => {}} onTune={() => {}} onStopTx={() => {}} />
      <div className="cockpit-txdock">
        <div className="cw-macros" />
        <div className="cw-send cockpit-txdock-keep" />
      </div>
    </main>
  )
}
function Sstv() {
  return (
    <main className="layout single sstv-view">
      <CockpitTxStrip radio={radio} onSetTxEnabled={() => {}} onTune={() => {}} onStopTx={() => {}} />
      <div className="sstv-tx-bar" />
    </main>
  )
}
const keepRow = () => document.querySelector<HTMLElement>('.cockpit-txdock-keep')!
const dock = () => document.querySelector<HTMLElement>('main > .cockpit-txdock')!
const bar = () => document.querySelector<HTMLElement>('.sstv-tx-bar')!
const stripBottom = () => document.querySelector<HTMLElement>('.cockpit-txstrip')!.style.getPropertyValue('--cockpit-txstrip-bottom')
const room = (el: HTMLElement) => el.style.getPropertyValue('--cockpit-txdock-room')

/** The least shell that parks the keep row whole beside the strip (the dock folded, as at every such size). */
const KEEP_FITS = STRIP_H + KEEP_H + SPARE

describe('a folded dock’s keep row never covers Stop TX', () => {
  it('beside the strip where both fit: parked whole, not squeezed, and the strip clears all of it', () => {
    injectSheet(SHEET)
    shellH = KEEP_FITS
    render(<Keyboard />)
    expect(dock().getAttribute('data-fold'), 'control: a dock this short is folded').toBe('on')
    expect(keepRow().getAttribute('data-squeeze'), 'a keep row that fits was squeezed').toBeNull()
    expect(room(keepRow())).toBe('')
    expect(stripBottom()).toBe(`${KEEP_H}px`)
  })

  it('one pixel short squeezes it into the room the strip leaves, and the strip parks right above it', () => {
    injectSheet(SHEET)
    shellH = KEEP_FITS - 1
    render(<Keyboard />)
    expect(keepRow().getAttribute('data-squeeze'), 'the keep row parked over Stop TX').toBe('on')
    expect(room(keepRow())).toBe(`${KEEP_FITS - 1 - STRIP_H - SPARE}px`)
    // Strip + capped row = the scrollport, less the spare pixel: they cannot overlap.
    expect(stripBottom()).toBe(`${KEEP_FITS - 1 - STRIP_H - SPARE}px`)
  })

  it('the shell’s own padding is not room: a sticky box parks inside it', () => {
    injectSheet(`${SHEET} main { padding: 10px 0 7px }`)
    shellH = KEEP_FITS + 16
    render(<Keyboard />)
    expect(keepRow().getAttribute('data-squeeze')).toBe('on')
    expect(room(keepRow())).toBe(`${KEEP_FITS + 16 - 17 - STRIP_H - SPARE}px`)
  })

  it('released when the window has room again, and stable on every render in between (no flapping)', () => {
    injectSheet(SHEET)
    shellH = KEEP_FITS - 10
    const { rerender } = render(<Keyboard />)
    for (let i = 0; i < 3; i++) {
      rerender(<Keyboard />)
      expect(keepRow().getAttribute('data-squeeze'), `render ${i}`).toBe('on')
      expect(room(keepRow()), `render ${i}`).toBe(`${KEEP_FITS - 10 - STRIP_H - SPARE}px`)
      expect(dock().getAttribute('data-fold'), `render ${i}: a squeezed row unfolded the dock`).toBe('on')
    }
    shellH = KEEP_FITS
    rerender(<Keyboard />)
    expect(keepRow().getAttribute('data-squeeze'), 'still squeezed with the room back').toBeNull()
    expect(room(keepRow()), 'the cap outlived the squeeze').toBe('')
  })

  it('unfolded, a keep row is squeezed by nothing: the whole dock parks, and the cap goes with the fold', () => {
    injectSheet(SHEET)
    shellH = KEEP_FITS - 10
    const { rerender } = render(<Keyboard />)
    expect(keepRow().getAttribute('data-squeeze')).toBe('on')
    shellH = STRIP_H + DOCK_H + FOLD_ROOM_EM * EM
    rerender(<Keyboard />)
    expect(dock().getAttribute('data-fold'), 'control: a dock that fits whole parks whole').toBeNull()
    expect(keepRow().getAttribute('data-squeeze'), 'an unfolded keep row kept its cap').toBeNull()
    expect(room(keepRow())).toBe('')
    expect(stripBottom()).toBe(`${DOCK_H}px`)
  })
})

describe('SSTV’s bar never covers Stop TX', () => {
  it('beside the strip where both fit: not squeezed', () => {
    injectSheet(SHEET)
    shellH = STRIP_H + BAR_H + SPARE
    render(<Sstv />)
    expect(bar().getAttribute('data-squeeze')).toBeNull()
    expect(stripBottom()).toBe(`${BAR_H}px`)
  })

  it('grown past the room (the dropped-picture sentence, an alarm that shrank the shell): squeezed under the strip', () => {
    injectSheet(SHEET)
    shellH = STRIP_H + BAR_H
    render(<Sstv />)
    expect(bar().getAttribute('data-squeeze'), 'the bar parked over Stop TX').toBe('on')
    expect(room(bar())).toBe(`${BAR_H - SPARE}px`)
    expect(stripBottom()).toBe(`${BAR_H - SPARE}px`)
  })
})

describe('only a parked strip squeezes anything', () => {
  it('a strip that scrolls (the hosted observer’s) leaves the bottom surface as it is', () => {
    injectSheet('.cockpit-txdock { position: sticky } .sstv-tx-bar { position: sticky }')
    shellH = 60
    render(<Sstv />)
    expect(bar().getAttribute('data-squeeze')).toBeNull()
  })

  it('a shell with no height (a keep-alive host under [hidden]) squeezes nothing', () => {
    injectSheet(SHEET)
    shellH = 0
    render(<Sstv />)
    expect(bar().getAttribute('data-squeeze')).toBeNull()
  })
})

describe('a squeezed surface shows its own stop where the cap can hold it', () => {
  // SSTV's bar wraps its mode picker onto the first line, so at the top of its own scroll the cap showed the picker and
  // not Send and Stop (900×600 at 175 % in Russian, measured). The surface is scrolled to its stop when the squeeze begins
  // and when the stop goes live, and never otherwise, so an operator's own scroll of it stands.
  const STOP_AT = 90 // the stop's top inside the bar's content, layout px
  const STOP_H = 30
  let scrolled = 0
  function SstvStop({ live }: { live: boolean }) {
    return (
      <main className="layout single sstv-view">
        <CockpitTxStrip radio={radio} onSetTxEnabled={() => {}} onTune={() => {}} onStopTx={() => {}} />
        <div className="sstv-tx-bar">
          <button type="button" className="sstv-tx-stop" disabled={!live}>Stop</button>
        </div>
      </main>
    )
  }
  function stubBar() {
    scrolled = 0
    const b = bar()
    Object.defineProperty(b, 'scrollTop', { configurable: true, get: () => scrolled, set: (v: number) => { scrolled = Math.max(0, v) } })
    const capOf = () => parseFloat(b.style.getPropertyValue('--cockpit-txdock-room')) || BAR_H
    // On the bar itself: a spy on an inherited getter would replace every element's, the shell's too.
    Object.defineProperty(b, 'clientHeight', { configurable: true, get: () => (b.getAttribute('data-squeeze') === 'on' ? capOf() : BAR_H) })
    b.getBoundingClientRect = () => ({ top: 500, height: b.clientHeight, bottom: 500 + b.clientHeight, left: 0, right: 800, width: 800, x: 0, y: 500, toJSON: () => ({}) }) as DOMRect
    const stop = b.querySelector<HTMLElement>('.sstv-tx-stop')!
    stop.getBoundingClientRect = () => ({ top: 500 + STOP_AT - scrolled, height: STOP_H, bottom: 500 + STOP_AT - scrolled + STOP_H, left: 0, right: 80, width: 80, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
  }

  it('the squeeze begins: the bar scrolls the least that shows its Stop whole', () => {
    injectSheet(SHEET)
    shellH = STRIP_H + BAR_H + SPARE
    const { rerender } = render(<SstvStop live={false} />)
    stubBar()
    shellH = STRIP_H + 60 + SPARE
    rerender(<SstvStop live={false} />)
    expect(bar().getAttribute('data-squeeze')).toBe('on')
    expect(scrolled, 'the cap showed the top of the bar, and not its Stop').toBe(STOP_AT + STOP_H - 60)
  })

  it('the operator’s own scroll stands on later renders, and the Stop going live brings it back', () => {
    injectSheet(SHEET)
    shellH = STRIP_H + BAR_H + SPARE
    const { rerender } = render(<SstvStop live={false} />)
    stubBar()
    shellH = STRIP_H + 60 + SPARE
    rerender(<SstvStop live={false} />)
    scrolled = 0 // the operator scrolls the bar back to its top, to the mode picker
    rerender(<SstvStop live={false} />)
    expect(scrolled, 'a render took the operator’s scroll back').toBe(0)
    rerender(<SstvStop live={true} />)
    expect(scrolled, 'SSTV is sending and its live Stop is out of the cap').toBe(STOP_AT + STOP_H - 60)
  })

  it('the cap changing (a line arriving a frame later) brings the Stop back into it', () => {
    injectSheet(SHEET)
    shellH = STRIP_H + BAR_H + SPARE
    const { rerender } = render(<SstvStop live={false} />)
    stubBar()
    shellH = STRIP_H + 100 + SPARE
    rerender(<SstvStop live={false} />)
    expect(scrolled, 'control: a 100 px cap holds the Stop with a 20 px scroll').toBe(STOP_AT + STOP_H - 100)
    shellH = STRIP_H + 60 + SPARE
    rerender(<SstvStop live={false} />)
    expect(scrolled, 'the cap shrank and left the Stop below it').toBe(STOP_AT + STOP_H - 60)
  })

  it('a cap shorter than the Stop cannot hold it, and nothing is scrolled', () => {
    injectSheet(SHEET)
    shellH = STRIP_H + BAR_H + SPARE
    const { rerender } = render(<SstvStop live={true} />)
    stubBar()
    shellH = STRIP_H + STOP_H - 5 + SPARE
    rerender(<SstvStop live={true} />)
    expect(bar().getAttribute('data-squeeze')).toBe('on')
    expect(scrolled).toBe(0)
  })
})

describe('a room shorter than one control', () => {
  // JS8 in Japanese at 900×600 at 175 %: 27 px of room, shorter than one of its buttons (measured). A cap of the room
  // shows a sliver of no control. The surface takes one control's height instead, over the strip's lower part (the TX
  // state, the meters) and never over the line Stop TX is on; where even that is too short, it keeps the room.
  const CTL_H = 38
  const LINE_BOTTOM = 20 // where the strip's first line, with Stop TX, ends: layout px from the strip's top
  function Js8() {
    return (
      <main className="layout single js8-cockpit">
        <CockpitTxStrip radio={radio} onSetTxEnabled={() => {}} onTune={() => {}} onStopTx={() => {}} />
        <div className="sstv-tx-bar"><input className="js8-msg" /></div>
      </main>
    )
  }
  function stubLine() {
    const strip = document.querySelector<HTMLElement>('.cockpit-txstrip')!
    strip.getBoundingClientRect = () => ({ top: 100, height: STRIP_H, bottom: 100 + STRIP_H, left: 0, right: 800, width: 800, x: 0, y: 100, toJSON: () => ({}) }) as DOMRect
    // Stop TX's line: the cluster it stands in (as the strip draws it), or the button.
    const stop = strip.querySelector<HTMLElement>('.op-btn.stop')!.closest<HTMLElement>('.cq-txctl') ?? strip.querySelector<HTMLElement>('.op-btn.stop')!
    stop.getBoundingClientRect = () => ({ top: 104, height: LINE_BOTTOM - 4, bottom: 100 + LINE_BOTTOM, left: 0, right: 80, width: 80, x: 0, y: 104, toJSON: () => ({}) }) as DOMRect
    Object.defineProperty(document.querySelector('.js8-msg')!, 'offsetHeight', { configurable: true, get: () => CTL_H })
  }

  it('the cap is one control, and the surface stands over the strip below Stop TX’s line, never over it', () => {
    injectSheet(SHEET)
    shellH = STRIP_H + BAR_H + SPARE
    const { rerender } = render(<Js8 />)
    stubLine()
    shellH = STRIP_H + 27 + SPARE // 27 px of room
    rerender(<Js8 />)
    expect(bar().getAttribute('data-squeeze')).toBe('on')
    expect(room(bar()), 'a sliver shorter than any control').toBe(`${CTL_H}px`)
    // One control stands over the strip from the scrollport's foot: its top stays below Stop TX's line.
    expect(shellH - CTL_H, 'the surface stands over Stop TX').toBeGreaterThanOrEqual(LINE_BOTTOM)
  })

  it('where one control would reach Stop TX’s line, it keeps the room', () => {
    injectSheet(SHEET)
    shellH = STRIP_H + BAR_H + SPARE
    const { rerender } = render(<Js8 />)
    stubLine()
    shellH = LINE_BOTTOM + 20 + SPARE // the room below Stop TX's line is 20 px: shorter than one control
    rerender(<Js8 />)
    expect(bar().getAttribute('data-squeeze')).toBe('on')
    const cap = parseFloat(room(bar()))
    expect(cap, 'the surface would stand over Stop TX').toBeLessThanOrEqual(shellH - LINE_BOTTOM - SPARE)
  })
})

