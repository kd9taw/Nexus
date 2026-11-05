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
// 0 for it while it carries the attribute — which is exactly what the decision must never read.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { CockpitTxStrip, FOLD_ROOM_EM, PIN_ROOM_EM } from './CockpitTxStrip'
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
    if (this.classList.contains('cockpit-txdock')) return this.getAttribute('data-fold') === 'on' ? 0 : DOCK_H
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

function Shell({ keep = true, dial = 7.04, header = false, dockClass = 'cockpit-txdock' }: { keep?: boolean; dial?: number; header?: boolean; dockClass?: string }) {
  return (
    <main className="layout single cw-cockpit">
      {header && <div className="cockpit-header cockpit-header-pinned" data-pin="on" />}
      <CockpitTxStrip radio={{ ...radio, dialMhz: dial }} onSetTxEnabled={() => {}} onTune={() => {}} onStopTx={() => {}} />
      <div className={dockClass}>
        <div className="cw-macros" />
        <div className={keep ? 'cw-send cockpit-txdock-keep' : 'cw-send'} />
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

  it('the control: a dock with no keep row never folds, however small the window (JS8, JTTY)', () => {
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
