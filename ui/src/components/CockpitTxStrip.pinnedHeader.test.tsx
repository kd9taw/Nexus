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
