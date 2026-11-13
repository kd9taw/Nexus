// @vitest-environment jsdom
//
// THE RAIL SAYS WHEN SECTIONS SIT BELOW ITS EDGE. On a laptop the section list holds more than the
// rail's height, and a 4 px scrollbar was the only sign the rest existed (1366×768 drew 11 of 23).
// The cue under the list counts the sections whose box ends past the scroller's lower edge, and
// pressing it scrolls the first of them to the top. jsdom draws no boxes, so the boxes are given
// here: a 300 px scroller with 40 px sections stacked from its top, moved by its scrollTop. The real
// geometry is laptopfit-probe.mjs's, in Chrome.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, screen, act } from '@testing-library/react'
import { ModeNav } from './ModeNav'
import type { FeatureId } from '../features/registry'

const ALL_ON = new Proxy({}, { get: () => true }) as Record<FeatureId, boolean>
const ROW = 40
/** The cue, found by its accessible name (which carries the count). */
const CUE = { name: /more sections below/i }

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

function rect(top: number, height: number): DOMRect {
  return { top, bottom: top + height, height, left: 0, right: 88, width: 88, x: 0, y: top, toJSON: () => ({}) } as DOMRect
}

/** Lay the rail out: the scroller `h` px tall at y=0, every section button ROW px, in order. */
function layout(h: number) {
  const top = document.querySelector<HTMLElement>('.mode-nav-top')!
  const btns = [...top.querySelectorAll<HTMLElement>('button.mode-btn')]
  Object.defineProperty(top, 'clientHeight', { configurable: true, get: () => h })
  top.getBoundingClientRect = () => rect(0, h)
  btns.forEach((b, i) => {
    b.getBoundingClientRect = () => rect(i * ROW - top.scrollTop, ROW)
  })
  const scrollBy = vi.fn((o: ScrollToOptions) => {
    top.scrollTop += o.top ?? 0
    fireEvent.scroll(top)
  })
  top.scrollBy = scrollBy as unknown as HTMLElement['scrollBy']
  return { top, btns, scrollBy }
}

function renderRail() {
  return render(
    <ModeNav view="operate" mode="chat" enabled={ALL_ON} onSelect={vi.fn()} tier="FT8" onDigitalMode={vi.fn()} onClubBoard={vi.fn()} clubBoard />,
  )
}

describe('the rail’s "more below" cue', () => {
  it('counts the sections past the lower edge, and pressing it brings the first of them to the top', () => {
    renderRail()
    const { top, btns, scrollBy } = layout(300)
    act(() => {
      fireEvent.scroll(top)
    })
    // 300 / 40: seven sections end inside the edge, the eighth (280-320) and the rest do not.
    const hidden = btns.length - 7
    expect(btns.length).toBeGreaterThan(20)
    const cue = screen.getByRole('button', CUE)
    expect(cue.textContent).toBe(`${hidden} more`)
    act(() => {
      fireEvent.click(cue)
    })
    expect(scrollBy).toHaveBeenCalledWith({ top: 7 * ROW, behavior: 'smooth' })
    expect(screen.getByRole('button', CUE).textContent).toBe(`${hidden - 7} more`)
  })

  it('is gone with every section in view, and once the list is scrolled to its end', () => {
    renderRail()
    const { top, btns } = layout(2000)
    act(() => {
      fireEvent.scroll(top)
    })
    expect(screen.queryByRole('button', CUE)).toBeNull()
    const short = layout(300)
    act(() => {
      short.top.scrollTop = btns.length * ROW - 300
      fireEvent.scroll(short.top)
    })
    expect(screen.queryByRole('button', CUE)).toBeNull()
  })

  it('counts again when the club board button comes or goes (no section setting changes)', () => {
    const rail = (clubBoard: boolean) => (
      <ModeNav view="operate" mode="chat" enabled={ALL_ON} onSelect={vi.fn()} tier="FT8" onDigitalMode={vi.fn()} onClubBoard={vi.fn()} clubBoard={clubBoard} />
    )
    const r = render(rail(false))
    const top = document.querySelector<HTMLElement>('.mode-nav-top')!
    Object.defineProperty(top, 'clientHeight', { configurable: true, get: () => 300 })
    top.getBoundingClientRect = () => rect(0, 300)
    // Boxes from the live DOM, so a button that arrives later is laid out too.
    const real = HTMLElement.prototype.getBoundingClientRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      const all = [...top.querySelectorAll<HTMLElement>('button.mode-btn')]
      const i = all.indexOf(this)
      return i < 0 ? real.call(this) : rect(i * ROW, ROW)
    })
    act(() => {
      fireEvent.scroll(top)
    })
    const cue = () => screen.getByRole('button', CUE).textContent
    const without = top.querySelectorAll('button.mode-btn').length - 7
    expect(cue()).toBe(`${without} more`)
    r.rerender(rail(true))
    expect(cue()).toBe(`${without + 1} more`)
  })

  it('says the count in its accessible name, which starts with the words it shows (WCAG 2.5.3)', () => {
    renderRail()
    const { top, btns } = layout(300)
    act(() => {
      fireEvent.scroll(top)
    })
    const cue = screen.getByRole('button', CUE)
    const n = btns.length - 7
    expect(cue.getAttribute('aria-label')).toBe(`${n} more sections below — show them`)
    expect(cue.getAttribute('aria-label')!.startsWith(cue.textContent!)).toBe(true)
  })

  it('hands keyboard focus to the last section, not the page, when pressing it reaches the end', () => {
    // Focus opens the cue's tooltip, whose popper measures itself; jsdom has no ResizeObserver.
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    )
    renderRail()
    const { top, btns } = layout(300)
    act(() => {
      fireEvent.scroll(top)
    })
    const cue = screen.getByRole('button', CUE)
    cue.focus()
    expect(document.activeElement).toBe(cue)
    act(() => {
      top.scrollTop = btns.length * ROW - 300
      fireEvent.scroll(top)
    })
    expect(screen.queryByRole('button', CUE)).toBeNull()
    expect(document.activeElement).toBe(btns[btns.length - 1])
  })

  it('counts nothing in a hidden (0×0) host', () => {
    renderRail()
    const { top } = layout(0)
    act(() => {
      fireEvent.scroll(top)
    })
    expect(screen.queryByRole('button', CUE)).toBeNull()
  })
})
