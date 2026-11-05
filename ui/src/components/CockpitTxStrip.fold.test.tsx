// @vitest-environment jsdom
//
// THE DOCK FOLDS ONLY WHERE IT DOES NOT FIT WHOLE (CockpitTxStrip decideFold, cockpit-panes.css
// `.cockpit-txdock[data-fold='on']`).
//
// On a small window at a large zoom (900×600 and 700×800 at 175 %) the TX dock was taller than the shell's
// scrollport. Parked whole at the bottom it painted over the strip's TX On/Off, Tune and Stop TX at every
// scroll position, pushed Send below the window and hid ⊞ Panels from the pointer (measured in Chrome;
// scripts/browser-probe --desktop digishell holds the geometry). So the strip measures the strip, the
// WHOLE dock and FOLD_ROOM_EM against the scrollport, and where they do not fit it folds the dock: the
// dock and its keep row (Send / PTT / Esc/Stop) carry `data-fold="on"`, the keep row alone parks, and the
// strip clears the keep row instead of the dock. jsdom loads no sheet and lays nothing out: the `sticky`s
// are injected and the heights stubbed. A folded dock has no box (`display: contents`), so the stub reports
// 0 for it while it carries the attribute — which is exactly what the decision must never read. And as in
// Chrome, the keep row's folded padding and border (BAND) count in the dock's height while the keep row
// is folded: the decision unfolds only the dock to measure it, which is the anti-flap band.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { CockpitTxStrip, FOLD_ROOM_EM, PIN_ROOM_EM, keepInSight, revealAfterPress } from './CockpitTxStrip'
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
const HEAD_H = 90
/** The keep row's folded padding and border, in the dock's height while the keep row is folded (Chrome: 7–8 px). */
const BAND = 8
/** An extra dock row (a sequencer row, say) and its height. */
const EXTRA_H = 100
/** Strip + whole dock + the room the fold leaves: the smallest room that parks the dock whole. */
const FITS = STRIP_H + DOCK_H + FOLD_ROOM_EM * EM

let shellH = 0
let sheet: HTMLStyleElement | null = null
function injectSheet(css: string) {
  sheet = document.createElement('style')
  sheet.textContent = css
  document.head.appendChild(sheet)
}
const SHEET = '.cockpit-txstrip { position: sticky } .cockpit-txdock { position: sticky } ' +
  ".cockpit-header-pinned { position: sticky } .cockpit-header-pinned[data-pin='off'] { position: static }"

beforeEach(() => {
  shellH = 0
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
    return this.tagName === 'MAIN' ? shellH : 0
  })
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (this: HTMLElement) {
    if (this.classList.contains('cockpit-txdock')) {
      if (this.getAttribute('data-fold') === 'on') return 0
      const keepFolded = this.querySelector('.cockpit-txdock-keep')?.getAttribute('data-fold') === 'on'
      return DOCK_H + (keepFolded ? BAND : 0) + (this.querySelector('.cw-extra') ? EXTRA_H : 0)
    }
    if (this.classList.contains('cockpit-txdock-keep')) return KEEP_H
    if (this.classList.contains('cockpit-txstrip')) return STRIP_H
    if (this.classList.contains('cockpit-header')) return HEAD_H
    return 0
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  sheet?.remove()
  sheet = null
})

function Shell({ keep = true, dial = 7.04, header = false, dockClass = 'cockpit-txdock', extra = false, reason = false }: { keep?: boolean; dial?: number; header?: boolean; dockClass?: string; extra?: boolean; reason?: boolean }) {
  return (
    <main className="layout single cw-cockpit">
      {header && <div className="cockpit-header cockpit-header-pinned" data-pin="on" />}
      <CockpitTxStrip radio={{ ...radio, dialMhz: dial }} onSetTxEnabled={() => {}} onTune={() => {}} onStopTx={() => {}} />
      <div className={dockClass}>
        {extra && <div className="cw-extra" />}
        <div className="cw-macros" />
        <div className={keep ? 'cw-send cockpit-txdock-keep' : 'cw-send'}>{reason && <div className="the-reason" role="status" />}</div>
      </div>
    </main>
  )
}
const dock = () => document.querySelector<HTMLElement>('main > .cockpit-txdock')!
const keepRow = () => document.querySelector<HTMLElement>('.cw-send')!
const bottom = () => document.querySelector<HTMLElement>('.cockpit-txstrip')!.style.getPropertyValue('--cockpit-txstrip-bottom')

describe('the dock folds only where it does not fit whole', () => {
  it('a scrollport with room for the strip, the whole dock and the fold room parks the dock whole', () => {
    injectSheet(SHEET)
    shellH = FITS
    render(<Shell />)
    expect(dock().getAttribute('data-fold'), 'a dock that fits was folded').toBeNull()
    expect(keepRow().getAttribute('data-fold')).toBeNull()
    expect(bottom(), 'the strip no longer clears the whole dock').toBe(`${DOCK_H}px`)
  })

  it('one pixel short folds it: the dock and its keep row carry the fold, and the strip clears the keep row alone', () => {
    injectSheet(SHEET)
    shellH = FITS - 1
    render(<Shell />)
    expect(dock().getAttribute('data-fold'), 'a dock taller than the room stayed whole, over Stop TX').toBe('on')
    expect(keepRow().getAttribute('data-fold'), 'the keep row would not park').toBe('on')
    expect(bottom(), 'the strip would park a whole dock above the keep row').toBe(`${KEEP_H}px`)
  })

  it('the shell’s own padding is not room: a sticky box parks inside it', () => {
    injectSheet(`${SHEET} main { padding: 10px 0 7px }`)
    shellH = FITS + 16
    render(<Shell />)
    expect(dock().getAttribute('data-fold')).toBe('on')
  })

  it('stays folded on the next render: it never reads the height folding takes away (no flapping)', () => {
    injectSheet(SHEET)
    shellH = FITS - 1
    const r = render(<Shell />)
    expect(dock().getAttribute('data-fold')).toBe('on')
    // Folded, the dock reports 0 (no box). Read that, and it would "fit" and unfold, then fold again.
    r.rerender(<Shell dial={7.041} />)
    expect(dock().getAttribute('data-fold'), 'the fold flapped off on a re-render').toBe('on')
    r.rerender(<Shell dial={7.042} />)
    expect(dock().getAttribute('data-fold')).toBe('on')
    expect(keepRow().getAttribute('data-fold'), 'the keep row let go of a folded dock').toBe('on')
    expect(bottom()).toBe(`${KEEP_H}px`)
  })

  it('a window that grows back unfolds it on the next measurement, and the keep row lets go', () => {
    injectSheet(SHEET)
    shellH = FITS - 1
    const r = render(<Shell />)
    shellH = 900
    r.rerender(<Shell dial={7.041} />)
    expect(dock().getAttribute('data-fold')).toBeNull()
    expect(keepRow().getAttribute('data-fold'), 'the keep row stays parked inside a whole dock').toBeNull()
    expect(bottom()).toBe(`${DOCK_H}px`)
  })

  it('a shell with no height (a keep-alive host under [hidden]) decides nothing either way', () => {
    injectSheet(SHEET)
    shellH = FITS - 1
    const r = render(<Shell />)
    shellH = 0
    r.rerender(<Shell dial={7.041} />)
    expect(dock().getAttribute('data-fold'), 'hiding the host unfolded the dock').toBe('on')
    cleanup()
    render(<Shell />)
    expect(dock().getAttribute('data-fold'), 'an unlaid-out host folded the dock').toBeNull()
  })

  it('the control: a dock with no keep row never folds, however small the window', () => {
    injectSheet(SHEET)
    shellH = 60
    render(<Shell keep={false} />)
    expect(dock().getAttribute('data-fold')).toBeNull()
    expect(bottom()).toBe(`${DOCK_H}px`)
  })

  it('the control: a dock that is not sticky (a remote observer’s) never folds', () => {
    injectSheet(`${SHEET} .remote-observer-dock { position: static }`)
    shellH = 60
    render(<Shell dockClass="cockpit-txdock remote-observer-dock" />)
    expect(dock().getAttribute('data-fold')).toBeNull()
    expect(keepRow().getAttribute('data-fold')).toBeNull()
  })

  it('the header pin is decided on the WHOLE dock, folded or not', () => {
    // Folded, the dock measures 0. Pinned against that, the header (90) + strip (40) + 0 + 6em = 226 would
    // "fit" a 260 px scrollport and park over Stop TX; against the whole dock it needs 426 and stays unpinned.
    injectSheet(SHEET)
    shellH = 260
    expect(HEAD_H + STRIP_H + PIN_ROOM_EM * EM).toBeLessThan(shellH)
    const r = render(<Shell header />)
    expect(dock().getAttribute('data-fold')).toBe('on')
    expect(document.querySelector('.cockpit-header')!.getAttribute('data-pin'), 'the header pinned against a folded dock').toBe('off')
    r.rerender(<Shell header dial={7.041} />)
    expect(document.querySelector('.cockpit-header')!.getAttribute('data-pin')).toBe('off')
  })
})

describe('the fold keeps its decision, and is re-measured when what it reads changes', () => {
  it('the band: folded, it unfolds only once the window has grown the keep row’s folded padding more', () => {
    injectSheet(SHEET)
    shellH = FITS - 1
    const r = render(<Shell />)
    expect(dock().getAttribute('data-fold')).toBe('on')
    // The height that parks an unfolded dock whole does not unfold a folded one: its keep row still counts.
    shellH = FITS
    r.rerender(<Shell dial={7.041} />)
    expect(dock().getAttribute('data-fold'), 'no band: at the fold height it would fold and unfold by turns').toBe('on')
    shellH = FITS + BAND - 1
    r.rerender(<Shell dial={7.042} />)
    expect(dock().getAttribute('data-fold')).toBe('on')
    shellH = FITS + BAND
    r.rerender(<Shell dial={7.043} />)
    expect(dock().getAttribute('data-fold'), 'past the band it stays folded').toBeNull()
  })

  it('a render that changes nothing it reads does not unfold the dock to measure it again', () => {
    injectSheet(SHEET)
    shellH = FITS - 1
    const r = render(<Shell />)
    expect(dock().getAttribute('data-fold')).toBe('on')
    const watch = new MutationObserver(() => {})
    watch.observe(dock(), { attributes: true, attributeFilter: ['data-fold'] })
    for (let i = 1; i <= 5; i++) r.rerender(<Shell dial={7.04 + i / 1000} />)
    expect(watch.takeRecords().length, 'the folded dock was unfolded and refolded on renders that changed nothing').toBe(0)
    // …but a shell of another height is measured again (and here, folded again: still too small).
    shellH = FITS - 2
    r.rerender(<Shell dial={7.05} />)
    expect(watch.takeRecords().length, 'a new shell height was not re-measured').toBeGreaterThan(0)
    expect(dock().getAttribute('data-fold')).toBe('on')
    watch.disconnect()
  })

  it('the dock’s own rows changing is re-measured: a row that goes away can unfold it', () => {
    injectSheet(SHEET)
    shellH = FITS + 20 // the plain dock fits; with the extra row it does not
    const r = render(<Shell extra />)
    expect(dock().getAttribute('data-fold')).toBe('on')
    r.rerender(<Shell />)
    expect(dock().getAttribute('data-fold'), 'the dock lost a row and stayed folded on a stale measurement').toBeNull()
    r.rerender(<Shell extra />)
    expect(dock().getAttribute('data-fold')).toBe('on')
  })

  it('a shell that resizes with no render re-decides through the observer, header or not (CW, Phone)', () => {
    // Every render re-applies, which hides a missing observer; here nothing renders: only the observer can.
    const observers: Array<{ cb: ResizeObserverCallback; targets: Element[] }> = []
    vi.stubGlobal('ResizeObserver', class {
      targets: Element[] = []
      constructor(cb: ResizeObserverCallback) { observers.push({ cb, targets: this.targets }) }
      observe(el: Element) { this.targets.push(el) }
      unobserve() {}
      disconnect() {}
    })
    injectSheet(SHEET)
    shellH = 900
    render(<Shell />)
    expect(dock().getAttribute('data-fold')).toBeNull()
    const main = document.querySelector('main')!
    const o = observers.find((x) => x.targets.includes(main))
    expect(o, 'the shell is not observed without a pinned header: a shrinking window never folds the dock').toBeDefined()
    shellH = FITS - 1
    o!.cb([], {} as ResizeObserver)
    expect(dock().getAttribute('data-fold')).toBe('on')
    vi.unstubAllGlobals()
  })
})

// A FOCUSED CONTROL IS NEVER LEFT BEHIND THE PARKED KEEP ROW. Folded, the macro keys scroll with the panes, so Tab could
// stop on one behind the keep row, and Enter then sent a key the operator could not see (RTTY at 900×600: 15 of 18
// controls, measured). Two halves, and both are load-bearing:
//  · `--cockpit-txdock-keep-h` on the shell, read by its `scroll-padding-bottom`, in RENDERED px. Chrome applies scroll
//    padding unzoomed: offsetHeight (89 px for RTTY's 156 px row at 175 %) cleared CW and PSK by luck and not RTTY.
//  · Where the room between the strip and the keep row is shorter than the control (JS8 at 900×600: 22 px for 38 px
//    buttons) no padding can fit it and Chrome top-aligns it, its centre behind the row; the strip centres it instead.
describe('a focused control is never left behind the parked keep row', () => {
  const box = (top: number, height: number) => ({ x: 0, y: top, left: 0, right: 100, top, bottom: top + height, width: 100, height, toJSON() {} }) as DOMRect
  const boxes: Record<string, DOMRect> = {}
  const raf: Array<() => void> = []
  const flush = () => { while (raf.length) raf.shift()!() }
  const mountFolded = () => {
    injectSheet(SHEET)
    shellH = FITS - 1
    return render(<Shell />)
  }
  beforeEach(() => {
    for (const k of Object.keys(boxes)) delete boxes[k]
    Object.assign(boxes, { 'cockpit-txstrip': box(100, 67), 'cockpit-txdock-keep': box(300, 156), 'layout': box(50, 400) })
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      for (const [c, b] of Object.entries(boxes)) if (this.classList.contains(c)) return b
      return box(0, 0)
    })
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (this: HTMLElement) {
      if (this.tagName === 'MAIN') return 400 / 1.75
      if (this.classList.contains('cockpit-txdock')) return this.getAttribute('data-fold') === 'on' ? 0 : DOCK_H
      if (this.classList.contains('cockpit-txdock-keep')) return 156 / 1.75
      if (this.classList.contains('cockpit-txstrip')) return STRIP_H
      return 0
    })
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((cb) => { raf.push(() => cb(0)); return raf.length })
  })
  const addControl = (cls: string, top: number, height = 61, tag = 'button') => {
    boxes[cls] = box(top, height)
    const el = document.createElement(tag)
    el.className = cls
    document.querySelector('main > .cockpit-txdock')!.insertBefore(el, document.querySelector('.cw-macros'))
    return el
  }
  /** What a real Tab does: the key first, then the focus it moves. */
  const tabTo = (el: HTMLElement) => {
    fireEvent.keyDown(window, { key: 'Tab' })
    el.focus()
  }
  const focusOn = (cls: string, top: number, height = 61) => {
    const el = addControl(cls, top, height)
    tabTo(el)
    flush()
    return el
  }
  const main = () => document.querySelector<HTMLElement>('main')!

  it('folded, the shell pads its scroll by the keep row’s RENDERED height, not its layout height', () => {
    mountFolded()
    expect(main().style.getPropertyValue('--cockpit-txdock-keep-h'), 'in layout px, RTTY’s row is not cleared').toBe('156px')
  })

  it('a notice standing on the parked row keeps to the column right of Stop TX, in layout px', () => {
    boxes['op-btn'] = { ...box(100, 52), left: 339, right: 497, width: 158 }
    boxes['cockpit-txdock-keep'] = { ...box(300, 156), left: 24, right: 876, width: 852 }
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
      return this.classList.contains('cockpit-txdock-keep') ? 852 / 1.75 : 0
    })
    mountFolded()
    const left = parseFloat(document.querySelector<HTMLElement>('.cockpit-txdock-keep')!.style.getPropertyValue('--cockpit-txdock-reason-left'))
    expect(left, 'the notice can stand over Stop TX').toBeCloseTo((497 - 24) / 1.75 + 4, 3)
  })

  it('unfolded it pads nothing, and the shell’s inline style is what it was', () => {
    injectSheet(SHEET)
    shellH = 900
    render(<Shell />)
    expect(main().style.getPropertyValue('--cockpit-txdock-keep-h')).toBe('')
  })

  it('a control whose centre is still behind the keep row is centred between the strip and the row', () => {
    // The band is 167..300, its middle 233.5; the control's centre 330.5. At 175 % that is 97 / 1.75 of the shell's scroll.
    mountFolded()
    main().scrollTop = 500
    focusOn('key-a', 300)
    expect(main().scrollTop, 'focus stays on a key behind the keep row').toBeCloseTo(500 + (330.5 - 233.5) / 1.75, 3)
  })

  it('a control whose centre is under the strip is centred too', () => {
    mountFolded()
    main().scrollTop = 500
    focusOn('key-b', 90)
    expect(main().scrollTop).toBeCloseTo(500 + (120.5 - 233.5) / 1.75, 3)
  })

  it('a control already in sight does not move the shell', () => {
    mountFolded()
    main().scrollTop = 500
    focusOn('key-c', 200, 40)
    expect(main().scrollTop).toBe(500)
  })

  it('a focus with no Tab behind it never moves the shell, on a text field either', () => {
    // `:focus-visible` matches a text field on a mouse press, so it let a press re-centre the field under the held
    // pointer, and the second click of a double click reached a sender (21 send calls, measured).
    mountFolded()
    main().scrollTop = 500
    const field = addControl('a-field', 300, 40, 'input')
    fireEvent.pointerDown(field)
    field.focus()
    flush()
    expect(main().scrollTop).toBe(500)
  })

  it('a window regain never moves the shell: focusin fires again on the key Tab left focus on', async () => {
    // The operator has since scrolled that key behind the keep row; a click just after the jump sent a different key.
    mountFolded()
    const key = focusOn('tabbed', 300)
    main().scrollTop = 500
    await new Promise((r) => setTimeout(r, 130))
    key.dispatchEvent(new FocusEvent('focusin', { bubbles: true }))
    flush()
    expect(main().scrollTop, 'the regain scrolled the shell').toBe(500)
  })

  it('a frame later, only the control still focused is centred', () => {
    // Tab held down moves focus on before the frame: the one it left must not scroll the shell.
    mountFolded()
    main().scrollTop = 500
    const passed = addControl('passed', 300)
    const now = addControl('now', 200, 40)
    tabTo(passed)
    tabTo(now)
    flush()
    expect(main().scrollTop, 'the shell scrolled for a control focus had already left').toBe(500)
  })

  it('a notice standing on the keep row counts: the shell’s padding and the centring both clear it', () => {
    // The whole row plus its notice is what stands there. Tab stopped under JTTY's words otherwise (6 of 14, measured).
    injectSheet(SHEET)
    shellH = FITS - 1
    boxes['the-reason'] = box(250, 50)
    render(<Shell reason />)
    expect(main().style.getPropertyValue('--cockpit-txdock-keep-h'), 'the notice is not in what the shell clears').toBe('206px')
    main().scrollTop = 500
    focusOn('under-words', 250, 40) // centre 270: behind the notice, not the row. The room is 167..250, its middle 208.5.
    expect(main().scrollTop, 'Tab stops behind the notice').toBeCloseTo(500 + (270 - 208.5) / 1.75, 3)
  })

  it('a notice gets 2 lines, or 3 in a column under 160 layout px', () => {
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
      return this.classList.contains('cockpit-txdock-keep') ? 852 / 1.75 : 0
    })
    boxes['cockpit-txdock-keep'] = { ...box(300, 156), left: 24, right: 876, width: 852 }
    boxes['op-btn'] = { ...box(100, 52), left: 339, right: 497, width: 158 }
    mountFolded()
    const keep = document.querySelector<HTMLElement>('.cockpit-txdock-keep')!
    expect(keep.style.getPropertyValue('--cockpit-txdock-reason-lines')).toBe('2')
    cleanup()
    boxes['op-btn'] = { ...box(100, 52), left: 500, right: 658, width: 158 } // 218 window px, 124.6 layout px of column
    mountFolded()
    expect(document.querySelector<HTMLElement>('.cockpit-txdock-keep')!.style.getPropertyValue('--cockpit-txdock-reason-lines')).toBe('3')
  })

  it('while a press is held on a control the shell stays put, and lets go on release', () => {
    // Chrome reveals a field or a <select> on a mouse press, and it honours the shell's scroll padding, so the press moved
    // the keys under the pointer: 87 px on a <select>, 102 px on a field even before the padding (measured).
    mountFolded()
    main().scrollTop = 500
    const field = addControl('held', 300, 40, 'select')
    fireEvent.pointerDown(field)
    main().scrollTop = 587
    fireEvent.scroll(main())
    expect(main().scrollTop, 'the press moved the shell').toBe(500)
    main().scrollTop = 560
    fireEvent.pointerUp(window)
    expect(main().scrollTop, 'a reveal not yet delivered as a scroll event survived the release').toBe(500)
    main().scrollTop = 620
    fireEvent.scroll(main())
    expect(main().scrollTop, 'the operator’s own scroll is held after the release').toBe(620)
  })

  it('a press on a label holds a moment past the release: its click focuses the field then', () => {
    vi.useFakeTimers()
    try {
      mountFolded()
      main().scrollTop = 500
      const label = addControl('a-label', 300, 40, 'label')
      fireEvent.pointerDown(label)
      fireEvent.pointerUp(window)
      main().scrollTop = 580
      fireEvent.scroll(main())
      expect(main().scrollTop).toBe(500)
      vi.advanceTimersByTime(200)
      main().scrollTop = 580
      fireEvent.scroll(main())
      expect(main().scrollTop).toBe(580)
    } finally {
      vi.useRealTimers()
    }
  })

  it('a pin never sticks: a press with no release (a <select>’s native list) lets go by itself', () => {
    vi.useFakeTimers()
    try {
      mountFolded()
      main().scrollTop = 500
      fireEvent.pointerDown(addControl('stuck', 300, 40, 'select'))
      vi.advanceTimersByTime(1100)
      main().scrollTop = 580
      fireEvent.scroll(main())
      expect(main().scrollTop).toBe(580)
    } finally {
      vi.useRealTimers()
    }
  })

  it('the controls: a press on the shell’s scrollbar, or on an unfolded dock, pins nothing', () => {
    mountFolded()
    main().scrollTop = 500
    fireEvent.pointerDown(main())
    main().scrollTop = 580
    fireEvent.scroll(main())
    expect(main().scrollTop, 'the scrollbar cannot be dragged').toBe(580)
    cleanup()
    injectSheet(SHEET)
    shellH = 900
    render(<Shell />)
    main().scrollTop = 500
    fireEvent.pointerDown(document.querySelector('.cw-send')!, {})
    fireEvent.pointerDown(addControl('unfolded', 300, 40, 'select'))
    main().scrollTop = 580
    fireEvent.scroll(main())
    expect(main().scrollTop).toBe(580)
  })

  it('what a click opens is revealed only after a double click’s second press could land', () => {
    vi.useFakeTimers()
    try {
      mountFolded()
      const key = addControl('opener', 200, 40)
      fireEvent.pointerDown(key)
      fireEvent.pointerUp(window)
      let ran = 0
      revealAfterPress(() => { ran++ })
      expect(ran, 'it moved the keys under the second click').toBe(0)
      vi.advanceTimersByTime(600)
      expect(ran).toBe(1)
      vi.advanceTimersByTime(1000)
      revealAfterPress(() => { ran++ })
      expect(ran, 'by keyboard there is no second click to wait for').toBe(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('the parked surfaces themselves never scroll the shell for their own focus', () => {
    mountFolded()
    main().scrollTop = 500
    const send = document.querySelector<HTMLElement>('.cockpit-txdock-keep')!
    send.tabIndex = 0
    send.focus()
    flush()
    expect(main().scrollTop).toBe(500)
  })

  it('keepInSight does the same for something opened rather than focused, and needs no focus', () => {
    mountFolded()
    main().scrollTop = 500
    boxes['opened'] = box(300, 61)
    const el = document.createElement('span')
    el.className = 'opened'
    document.querySelector('main > .cockpit-txdock')!.appendChild(el)
    keepInSight(el)
    expect(main().scrollTop).toBeCloseTo(500 + (330.5 - 233.5) / 1.75, 3)
  })

  it('the control: an unfolded dock leaves focus to the browser', () => {
    injectSheet(SHEET)
    shellH = 900
    render(<Shell />)
    main().scrollTop = 500
    focusOn('key-d', 300)
    expect(main().scrollTop).toBe(500)
  })
})
