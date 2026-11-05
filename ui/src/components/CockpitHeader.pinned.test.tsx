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

  // ⊞ Panels' menu renders INSIDE the header. At the header's 4 the strip and the dock (4, later) painted
  // over the open menu; while it is open the header stands at 5, and closed it is back at 4.
  it('a menu open in a pinned header lifts it over the strip; closed, it is back under it; unpinned, never', async () => {
    const head = mount('rtty-cockpit', true)
    const flush = () => new Promise((r) => setTimeout(r, 0))
    const pop = document.createElement('div')
    pop.className = 'panels-menu-pop'
    head.querySelector('.ch-identity')!.appendChild(pop)
    await flush()
    expect(head.getAttribute('data-menu'), 'the open menu was not seen').toBe('open')
    expect(Number(css(head, 'z-index'))).toBe(5)
    head.setAttribute('data-pin', 'off')
    expect(css(head, 'z-index'), 'an unpinned header was lifted: a static flex item still stacks').toBe('auto')
    head.setAttribute('data-pin', 'on')
    pop.remove()
    await flush()
    expect(head.getAttribute('data-menu')).toBeNull()
    expect(Number(css(head, 'z-index')), 'closed, the header must be back under the strip and the dock').toBe(4)
  })

  it('the control: an unpinned header (FT, Phone, CW, JS8, JTTY) is not sticky', () => {
    const head = mount('cw-cockpit', false)
    expect(head.classList.contains('cockpit-header-pinned')).toBe(false)
    expect(css(head, 'position')).not.toBe('sticky')
  })
})

// THE LIFT'S LIFECYCLE (review 3): pinned flipping while a menu is open, a menu already open when the header
// pins, an unpinned header, and an unmount mid-menu. Each one is a way the lift could stick on a header that
// should stand under the strip and the dock, or never come.
describe('the menu lift follows the pin and never outlives the header', () => {
  const el = (pinned: boolean) => (
    <main className="layout single rtty-cockpit">
      <CockpitHeader snap={snap} modeIndicator={<span>RTTY</span>} bandControl={<span>20m</span>} pinned={pinned} />
    </main>
  )
  const flush = () => new Promise((r) => setTimeout(r, 0))
  const pop = () => {
    const p = document.createElement('div')
    p.className = 'panels-menu-pop'
    return p
  }
  const head = () => document.querySelector<HTMLElement>('.cockpit-header')!

  it('unpinned while open drops the lift at once, and a later menu never lifts it', async () => {
    const r = render(el(true))
    const p = pop()
    head().querySelector('.ch-identity')!.appendChild(p)
    await flush()
    expect(head().getAttribute('data-menu')).toBe('open')
    r.rerender(el(false))
    await flush()
    expect(head().getAttribute('data-menu'), 'left lifted after the header stopped being pinned').toBeNull()
    p.remove()
    head().querySelector('.ch-identity')!.appendChild(pop())
    await flush()
    expect(head().getAttribute('data-menu'), 'an unpinned header is still being watched').toBeNull()
  })

  it('a header that was never pinned is never watched', async () => {
    render(el(false))
    head().querySelector('.ch-identity')!.appendChild(pop())
    await flush()
    expect(head().getAttribute('data-menu')).toBeNull()
  })

  it('pinned while a menu is already open lifts at once', async () => {
    const r = render(el(false))
    head().querySelector('.ch-identity')!.appendChild(pop())
    await flush()
    r.rerender(el(true))
    await flush()
    expect(head().getAttribute('data-menu')).toBe('open')
  })

  it('unmounted while open leaves nothing behind (the node, if reused, is clean)', async () => {
    const r = render(el(true))
    const h = head()
    h.querySelector('.ch-identity')!.appendChild(pop())
    await flush()
    r.unmount()
    expect(h.getAttribute('data-menu')).toBeNull()
  })
})
