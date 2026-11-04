// @vitest-environment jsdom
//
// THE PINNED HEADER (SSTV, RTTY, PSK). On a short or zoomed window those shells scroll, and their
// header — the mode, the dial, the band, ⊞ Panels, power, CAT — used to scroll out of the window with
// the scope while only the TX strip and the dock stayed. `pinned` makes the RENDERED header sticky at
// the top of its shell. Computed over the real sheets for the element CockpitHeader renders, inside
// each shell it is pinned in, so a selector that stopped matching (or a heavier rule that took the
// position back) turns this red; the unpinned header every other cockpit renders is the control.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import { css, loadSheets } from '../cssCascade.testkit'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
vi.mock('../useWheelTune', () => ({ useWheelTune: () => undefined }))

const snap = {
  radio: { dialMhz: 14.23, catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, tuning: false, txAllowed: true },
} as unknown as AppSnapshot

function mount(shell: string, pinned: boolean) {
  render(
    <div className="app">
      <div className="shell">
        <main className={`layout single ${shell}`}>
          <CockpitHeader snap={snap} modeIndicator={<span>SSTV</span>} bandControl={<span>20m</span>} pinned={pinned} />
        </main>
      </div>
    </div>,
  )
  return document.querySelector<HTMLElement>('.cockpit-header')!
}

beforeAll(() => loadSheets())
afterEach(cleanup)

describe('a pinned cockpit header stays at the top of its shell', () => {
  for (const shell of ['sstv-view', 'rtty-cockpit', 'psk-cockpit']) {
    it(`${shell}: the rendered header is sticky at top 0, and never stacked over the strip`, () => {
      const head = mount(shell, true)
      expect(head.parentElement!.tagName, 'a sticky box must be a direct shell child').toBe('MAIN')
      expect(head.getAttribute('data-pin'), 'it starts pinned').toBe('on')
      expect(css(head, 'position')).toBe('sticky')
      expect(css(head, 'top')).toMatch(/^0(px)?$/)
      expect(Number(css(head, 'z-index'))).toBe(4)
    })

    it(`${shell}: turned off (the three parked surfaces do not fit), the rendered header is static`, () => {
      const head = mount(shell, true)
      head.setAttribute('data-pin', 'off')
      expect(css(head, 'position')).toBe('static')
    })
  }

  it('the control: an unpinned header (FT, Phone, CW, JS8, JTTY) is not sticky', () => {
    const head = mount('cw-cockpit', false)
    expect(head.classList.contains('cockpit-header-pinned')).toBe(false)
    expect(css(head, 'position')).not.toBe('sticky')
  })
})
