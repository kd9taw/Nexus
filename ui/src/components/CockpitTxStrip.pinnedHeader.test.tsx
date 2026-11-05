// @vitest-environment jsdom
//
// THE TX STRIP PARKS UNDER A PINNED HEADER, NOT OVER IT (SSTV, RTTY, PSK).
//
// Those three shells pin their cockpit header (`.cockpit-header-pinned`, sticky at the top of the
// shell's scrollport) so the mode, the dial, the band, ⊞ Panels, power and CAT never scroll out of
// the window. The TX strip is sticky at the top too; parked at `top: 0` it would cover the header.
// So the strip measures a pinned header among its shell's children and writes its height as its own
// top offset (`--cockpit-txstrip-top`, read by cockpit-panes.css), the way it already writes the
// dock's height as its bottom offset. jsdom loads no sheet: the `sticky`s are injected and the
// heights stubbed; what is computed is that the strip finds the header, and only a pinned, sticky one.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { CockpitTxStrip } from './CockpitTxStrip'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)), haltTx: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

const radio = {
  dialMhz: 14.23, catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, tuning: false, txAllowed: true, atu: false, holdTxFreq: false,
} as unknown as AppSnapshot['radio']

const HEAD_H = 90
const DOCK_H = 117
const STRIP_RENDERED = 41.5

let sheet: HTMLStyleElement | null = null
function injectSheet(css: string) {
  sheet = document.createElement('style')
  sheet.textContent = css
  document.head.appendChild(sheet)
}
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (this: HTMLElement) {
    if (this.classList.contains('cockpit-header')) return HEAD_H
    if (this.classList.contains('cockpit-txdock')) return DOCK_H
    return 0
  })
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const h = this.classList.contains('cockpit-header') ? HEAD_H + 0.4 : this.classList.contains('cockpit-txstrip') ? STRIP_RENDERED : 0
    return { x: 0, y: 0, left: 0, top: 0, right: 0, bottom: h, width: 0, height: h, toJSON() {} } as DOMRect
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  sheet?.remove()
  sheet = null
})

function shell(headerClass: string | null) {
  return render(
    <main className="layout single sstv-view">
      {headerClass && <div className={headerClass} />}
      <CockpitTxStrip radio={radio} onSetTxEnabled={() => {}} onTune={() => {}} onStopTx={() => {}} />
      <div className="cockpit-txdock" />
    </main>,
  )
}
const strip = () => document.querySelector<HTMLElement>('.cockpit-txstrip')!
const main = () => document.querySelector<HTMLElement>('main')!

describe('the TX strip under a pinned header', () => {
  it("parks at the pinned header's height, and the shell's scroll padding clears both", () => {
    injectSheet('.cockpit-header-pinned { position: sticky } .cockpit-txstrip { position: sticky } .cockpit-txdock { position: sticky }')
    shell('cockpit-header cockpit-header-pinned')
    expect(strip().style.getPropertyValue('--cockpit-txstrip-top'), 'the strip would park over the dial and the band').toBe(`${HEAD_H}px`)
    expect(strip().style.getPropertyValue('--cockpit-txstrip-bottom'), 'the dock clearance changed').toBe(`${DOCK_H}px`)
    // Rendered heights, rounded up: a focus scrolled to the start lands under the header AND the strip.
    expect(main().style.getPropertyValue('--cockpit-txstrip-h')).toBe(`${Math.ceil(HEAD_H + 0.4 + STRIP_RENDERED)}px`)
  })

  it('the control: a header that is not pinned clears nothing (every other cockpit)', () => {
    injectSheet('.cockpit-txstrip { position: sticky } .cockpit-txdock { position: sticky }')
    shell('cockpit-header')
    // Nothing written at all: the sheet's fallback (0) stands, and the strip's inline style is what it was.
    expect(strip().style.getPropertyValue('--cockpit-txstrip-top')).toBe('')
    expect(main().style.getPropertyValue('--cockpit-txstrip-h')).toBe(`${Math.ceil(STRIP_RENDERED)}px`)
  })

  it('a pinned header that is not sticky (its rule lost the cascade) clears nothing, so nothing is left hanging', () => {
    injectSheet('.cockpit-txstrip { position: sticky } .cockpit-txdock { position: sticky }')
    shell('cockpit-header cockpit-header-pinned')
    expect(strip().style.getPropertyValue('--cockpit-txstrip-top')).toBe('')
  })
})

// PINNED ONLY WHERE IT FITS. On a small window at a large zoom the pinned header, the strip and the dock
// are together taller than the scrollport, and the header covered Stop TX, Send and Esc/Stop at every
// scroll position (900×600 / 700×800 at 175 %, measured in Chrome). The strip measures the three against
// the shell and turns the pin off there; the header then scrolls and the strip parks at 0 again.
// Heights stubbed: header 90, strip 0, dock 117, the room PIN_ROOM_EM × 16 = 96 ⇒ the three need 303 px.
describe('the strip unpins the header where the three parked surfaces do not fit', () => {
  const SHEET =
    ".cockpit-header-pinned { position: sticky } .cockpit-header-pinned[data-pin='off'] { position: static } " +
    '.cockpit-txstrip { position: sticky } .cockpit-txdock { position: sticky }'
  const stubShell = (h: number) =>
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
      return this.tagName === 'MAIN' ? h : 0
    })
  const mount = () =>
    render(
      <main className="layout single sstv-view">
        <div className="cockpit-header cockpit-header-pinned" data-pin="on" />
        <CockpitTxStrip radio={radio} onSetTxEnabled={() => {}} onTune={() => {}} onStopTx={() => {}} />
        <div className="cockpit-txdock" />
      </main>,
    )
  const head = () => document.querySelector<HTMLElement>('.cockpit-header')!

  it('a scrollport with room keeps it pinned, and the strip parks under it', () => {
    injectSheet(SHEET)
    stubShell(303)
    mount()
    expect(head().getAttribute('data-pin')).toBe('on')
    expect(strip().style.getPropertyValue('--cockpit-txstrip-top')).toBe(`${HEAD_H}px`)
  })

  it('one pixel short of room unpins it: the header scrolls, and the strip parks at the top again', () => {
    injectSheet(SHEET)
    stubShell(302)
    mount()
    expect(head().getAttribute('data-pin'), 'the header stays pinned over Stop TX and the dock').toBe('off')
    expect(strip().style.getPropertyValue('--cockpit-txstrip-top')).toBe('')
    expect(main().style.getPropertyValue('--cockpit-txstrip-h'), 'the scroll padding still counts the header').toBe(`${Math.ceil(STRIP_RENDERED)}px`)
  })

  it('a window that grows back pins it again on the next measurement', () => {
    injectSheet(SHEET)
    const spy = stubShell(200)
    const r = mount()
    expect(head().getAttribute('data-pin')).toBe('off')
    spy.mockImplementation(function (this: HTMLElement) {
      return this.tagName === 'MAIN' ? 900 : 0
    })
    r.rerender(
      <main className="layout single sstv-view">
        <div className="cockpit-header cockpit-header-pinned" data-pin="on" />
        <CockpitTxStrip radio={{ ...radio, dialMhz: 14.231 }} onSetTxEnabled={() => {}} onTune={() => {}} onStopTx={() => {}} />
        <div className="cockpit-txdock" />
      </main>,
    )
    expect(head().getAttribute('data-pin')).toBe('on')
    expect(strip().style.getPropertyValue('--cockpit-txstrip-top')).toBe(`${HEAD_H}px`)
  })
})
