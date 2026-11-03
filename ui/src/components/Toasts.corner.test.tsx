// @vitest-environment jsdom
//
// #445: WHICH CORNER THE POP-UPS STAND IN. Settings ▸ Spots & Alerts ▸ Pop-up corner: bottom right
// (as it always was, and the default), bottom left, top right, top left. The value is a validated
// enum: anything this build does not know reads as bottom right. The swipe that dismisses a toast
// and the way it slides in follow the corner's side. Placement is COMPUTED from the real sheets'
// cascade (cssCascade.testkit) on the rendered column, never matched by regex.
import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act, waitFor } from '@testing-library/react'
import { Toasts } from './Toasts'
import { dismissToast, pushToast, subscribeToasts, type Toast } from '../toast'
import { popupCornerOf, setPopupCorner, type PopupCorner } from '../features/popupCorner'
import type { Box } from '../features/toastRoom'
import { atToken, css, loadSheets, resolveVal } from '../cssCascade.testkit'

beforeAll(() => loadSheets())

let toasts: Toast[] = []
const unsubscribe = subscribeToasts((now) => {
  toasts = now
})
afterEach(() => {
  cleanup()
  act(() => {
    for (const toast of toasts) dismissToast(toast.id)
    setPopupCorner('bottom-right')
  })
})
void unsubscribe

/** The app's own root (the zoomed `.app` tree), its toast column, and one toast in it. */
function mount(corner: PopupCorner, opts: { banner?: boolean } = {}) {
  act(() => setPopupCorner(corner))
  const { container } = render(
    <div className="app">
      {opts.banner && <div className="update-banner" />}
      <Toasts />
    </div>,
  )
  act(() => {
    pushToast('Logged W1AW', 'success')
  })
  const viewport = container.querySelector('.ui-toast-viewport') as HTMLElement
  const toast = container.querySelector('.ui-toast') as HTMLElement
  return { viewport, toast }
}

const GAP = () => resolveVal('var(--space-4)')
/** The four insets the cascade gives the column; `auto` and undeclared both read as unset. */
function insets(el: Element) {
  const v = (p: string) => {
    const w = css(el, p)
    return w == null || w === 'auto' ? null : w
  }
  return { top: v('top'), right: v('right'), bottom: v('bottom'), left: v('left') }
}

describe('popupCornerOf — the validated enum', () => {
  it('keeps the four corners and reads anything else as bottom right', () => {
    for (const c of ['bottom-right', 'bottom-left', 'top-right', 'top-left'] as const) expect(popupCornerOf(c)).toBe(c)
    for (const junk of [undefined, null, '', 'middle', 'TOP-LEFT', 'top_left', 3, {}, ['top-left']]) {
      expect(popupCornerOf(junk), JSON.stringify(junk)).toBe('bottom-right')
    }
  })
})

describe('the pop-up column stands in the chosen corner (#445)', () => {
  it('bottom right, as it always was, by default', () => {
    const { viewport } = mount('bottom-right')
    expect(viewport.getAttribute('data-corner')).toBe('bottom-right')
    expect(insets(viewport)).toEqual({ top: null, right: GAP(), bottom: GAP(), left: null })
  })

  it.each([
    ['bottom-left', { top: null, right: null, bottom: 'gap', left: 'gap' }],
    ['top-right', { top: 'gap', right: 'gap', bottom: null, left: null }],
    ['top-left', { top: 'gap', right: null, bottom: null, left: 'gap' }],
  ] as const)('%s', (corner, want) => {
    const { viewport } = mount(corner)
    const g = GAP()
    const expected = Object.fromEntries(Object.entries(want).map(([k, v]) => [k, v === 'gap' ? g : null]))
    expect(insets(viewport)).toEqual(expected)
  })

  it('keeps the same zoom-corrected caps in every corner', () => {
    // The caps are bottom right's (`--vh-eff` / `--vw-eff`, resolved here at their fallbacks);
    // a corner rule that set its own would be a raw-unit cap inside `.app`.
    const caps = (el: Element) => ['max-height', 'max-width', 'overflow-y', 'width'].map((p) => css(el, p))
    const home = caps(mount('bottom-right').viewport)
    cleanup()
    expect(home[0]).toContain('- 2 * 16px')
    for (const corner of ['bottom-left', 'top-right', 'top-left'] as const) {
      expect(caps(mount(corner).viewport), corner).toEqual(home)
      cleanup()
    }
  })

  it('lifts clear of the update banner in the bottom corners only', () => {
    const br = insets(mount('bottom-right', { banner: true }).viewport)
    expect(br.bottom).not.toBe(GAP())
    expect(br.bottom).toContain('72px')
    cleanup()
    const bl = insets(mount('bottom-left', { banner: true }).viewport)
    expect(bl.bottom).toContain('72px')
    cleanup()
    // A top corner has no bottom edge to lift: a `bottom` here would stretch the column.
    const tl = insets(mount('top-left', { banner: true }).viewport)
    expect(tl).toEqual({ top: GAP(), right: null, bottom: null, left: GAP() })
  })

  it('swipes and slides toward its own side', () => {
    for (const [corner, side] of [
      ['bottom-right', 'right'],
      ['top-right', 'right'],
      ['bottom-left', 'left'],
      ['top-left', 'left'],
    ] as const) {
      const { toast } = mount(corner)
      expect(toast.getAttribute('data-swipe-direction'), corner).toBe(side)
      // The slide-in comes from the corner's own edge: from the right is +16px, from the left −16px.
      const x = css(toast, '--toast-in-x')
      expect(x, corner).toBe(side === 'right' ? '16px' : '-16px')
      act(() => {
        for (const t of toasts) dismissToast(t.id)
      })
      cleanup()
    }
  })
})

// The column stands clear of every transmit surface (features/toastRoom.ts). The stylesheet takes
// the measured room through two variables, in EVERY corner and with the update banner up too; with
// none measured (jsdom, a window with no TX surface) the corner is the stylesheet's own.
describe('the column takes its measured room, clear of the TX surfaces (#445)', () => {
  it('every corner reads the measured inset and height, the banner lift included', () => {
    for (const [corner, edge] of [
      ['bottom-right', 'bottom'],
      ['bottom-left', 'bottom'],
      ['top-right', 'top'],
      ['top-left', 'top'],
    ] as const) {
      for (const banner of [false, true]) {
        const { viewport } = mount(corner, { banner })
        const at = atToken('--toast-inset', '123px', () => atToken('--toast-max-h', '456px', () => [css(viewport, edge), css(viewport, 'max-height')]))
        // With the banner up, a bottom corner never stands lower than the banner's own lift: a banner
        // that arrives after the last measure cannot lose to it.
        const want = banner && edge === 'bottom' ? 'max(123px, calc(16px + 72px))' : '123px'
        expect(at, `${corner}${banner ? ' with the banner' : ''}`).toEqual([want, '456px'])
        cleanup()
      }
    }
  })

  it('measures the TX dock and puts the room on the column (Phone: PTT at the bottom left)', () => {
    const rects = new Map<string, Box>([
      ['cockpit-txdock', { left: 0, top: 700, right: 1024, bottom: 768 }],
      ['ui-toast-viewport', { left: 16, top: 600, right: 356, bottom: 752 }],
    ])
    const spy = vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
      const k = [...rects.keys()].find((c) => this.classList.contains(c))
      const r = k ? rects.get(k)! : { left: 0, top: 0, right: 0, bottom: 0 }
      return { ...r, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, toJSON: () => r } as DOMRect
    })
    const was = [window.innerWidth, window.innerHeight]
    Object.defineProperty(window, 'innerWidth', { value: 1024, configurable: true })
    Object.defineProperty(window, 'innerHeight', { value: 768, configurable: true })
    try {
      const dock = document.createElement('div')
      dock.className = 'cockpit-txdock'
      document.body.appendChild(dock)
      const { viewport } = mount('bottom-left')
      expect(viewport.style.getPropertyValue('--toast-inset'), 'the column did not lift above the dock').toBe(`${768 - (700 - 16)}px`)
      expect(viewport.style.getPropertyValue('--toast-max-h')).toBe(`${700 - 16 - 16}px`)
      dock.remove()
    } finally {
      spy.mockRestore()
      Object.defineProperty(window, 'innerWidth', { value: was[0], configurable: true })
      Object.defineProperty(window, 'innerHeight', { value: was[1], configurable: true })
    }
  })

  // The surfaces change under a column that is already up: a screen's first visit mounts its TX strip
  // after the toasts (PSK, JTTY), a notice above a strip moves it without resizing it, a banner
  // arrives late. Each is a change in the app's tree, and the column re-measures on it (one frame
  // later), observing every surface it now finds for size changes too.
  describe('re-measures when the surfaces change under it', () => {
    let rects: Map<string, Box>
    let observed: Set<Element>
    let spy: { mockRestore(): void }
    const was = [window.innerWidth, window.innerHeight, globalThis.ResizeObserver] as const
    beforeEach(() => {
      rects = new Map<string, Box>([['ui-toast-viewport', { left: 16, top: 600, right: 356, bottom: 752 }]])
      observed = new Set()
      spy = vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
        const k = [...rects.keys()].find((c) => this.classList.contains(c))
        const r = k ? rects.get(k)! : { left: 0, top: 0, right: 0, bottom: 0 }
        return { ...r, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, toJSON: () => r } as DOMRect
      })
      Object.defineProperty(window, 'innerWidth', { value: 1024, configurable: true })
      Object.defineProperty(window, 'innerHeight', { value: 768, configurable: true })
      globalThis.ResizeObserver = class {
        observe(el: Element) {
          observed.add(el)
        }
        unobserve() {}
        disconnect() {
          observed.clear()
        }
      } as unknown as typeof ResizeObserver
    })
    afterEach(() => {
      spy.mockRestore()
      Object.defineProperty(window, 'innerWidth', { value: was[0], configurable: true })
      Object.defineProperty(window, 'innerHeight', { value: was[1], configurable: true })
      globalThis.ResizeObserver = was[2]
    })
    const inset = (vp: HTMLElement) => vp.style.getPropertyValue('--toast-inset')

    it('a TX strip mounted after the toasts (a first visit) is measured, and observed', async () => {
      const { viewport } = mount('bottom-left')
      expect(inset(viewport), 'nothing to keep off yet').toBe('16px')
      rects.set('cockpit-txstrip', { left: 0, top: 375, right: 1024, bottom: 415 })
      rects.set('cockpit-txdock', { left: 0, top: 700, right: 1024, bottom: 768 })
      const strip = document.createElement('div')
      strip.className = 'cockpit-txstrip'
      const dock = document.createElement('div')
      dock.className = 'cockpit-txdock'
      act(() => {
        viewport.closest('.app')!.append(strip, dock)
      })
      await waitFor(() => expect(inset(viewport), 'the new screen was never measured').toBe(`${768 - (700 - 16)}px`))
      expect(viewport.style.getPropertyValue('--toast-max-h')).toBe(`${700 - 16 - (415 + 16)}px`)
      expect(observed.has(strip) && observed.has(dock), 'the new surfaces are not watched for size').toBe(true)
    })

    it('a strip that moves without resizing is followed', async () => {
      rects.set('cockpit-txstrip', { left: 0, top: 375, right: 1024, bottom: 415 })
      const strip = document.createElement('div')
      strip.className = 'cockpit-txstrip'
      const { viewport } = mount('top-left')
      act(() => {
        viewport.closest('.app')!.append(strip)
      })
      await waitFor(() => expect(viewport.style.getPropertyValue('--toast-max-h')).toBe(`${375 - 16 - 16}px`))
      // A notice above it pushes it down 132 px; its own size is unchanged.
      rects.set('cockpit-txstrip', { left: 0, top: 507, right: 1024, bottom: 547 })
      act(() => {
        viewport.closest('.app')!.prepend(document.createElement('p'))
      })
      await waitFor(() => expect(viewport.style.getPropertyValue('--toast-max-h'), 'the column kept the old room').toBe(`${507 - 16 - 16}px`))
    })

    it('its own writes do not set it measuring again', async () => {
      const { viewport } = mount('bottom-left')
      const calls = () => spy as unknown as { mock: { calls: unknown[] } }
      await new Promise((r) => setTimeout(r, 60))
      const before = calls().mock.calls.length
      await new Promise((r) => setTimeout(r, 120))
      expect(calls().mock.calls.length, 'the column measures itself in a loop').toBe(before)
      void viewport
    })
  })
})