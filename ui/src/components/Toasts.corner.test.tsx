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

  // A dock that does not fit FOLDS (cockpit-panes.css `.cockpit-txdock[data-fold='on']`): it has no box at all,
  // and only its keep row — Send, PTT, Esc/Stop and the TX latch — stays parked at the foot. The column must keep
  // off that row: measuring the folded dock alone found nothing, and a large notice stood over Esc/Stop (RTTY at
  // 900×600 at 175 %, scripts/browser-probe --desktop toastcorner).
  it('a folded dock has no box: the column keeps off its parked keep row (RTTY: Esc/Stop at the bottom right)', () => {
    const rects = new Map<string, Box>([
      ['cockpit-txdock-keep', { left: 300, top: 520, right: 900, bottom: 600 }],
      ['ui-toast-viewport', { left: 544, top: 430, right: 884, bottom: 584 }],
    ])
    const spy = vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
      const k = [...rects.keys()].find((c) => this.classList.contains(c))
      const r = k ? rects.get(k)! : { left: 0, top: 0, right: 0, bottom: 0 }
      return { ...r, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, toJSON: () => r } as DOMRect
    })
    const was = [window.innerWidth, window.innerHeight]
    Object.defineProperty(window, 'innerWidth', { value: 900, configurable: true })
    Object.defineProperty(window, 'innerHeight', { value: 600, configurable: true })
    try {
      const dock = document.createElement('div')
      dock.className = 'cockpit-txdock'
      dock.dataset.fold = 'on'
      const keep = document.createElement('div')
      keep.className = 'cockpit-txdock-keep'
      keep.dataset.fold = 'on'
      dock.appendChild(keep)
      document.body.appendChild(dock)
      const { viewport } = mount('bottom-right')
      expect(viewport.style.getPropertyValue('--toast-inset'), 'the column stands over the keep row').toBe(`${600 - (520 - 16)}px`)
      expect(viewport.style.getPropertyValue('--toast-max-h')).toBe(`${520 - 16 - 16}px`)
      dock.remove()
    } finally {
      spy.mockRestore()
      Object.defineProperty(window, 'innerWidth', { value: was[0], configurable: true })
      Object.defineProperty(window, 'innerHeight', { value: was[1], configurable: true })
    }
  })

  // Every STOP CONTROL is kept clear wherever it sits (`data-stop-control`), not only the rows the list names: RTTY's
  // sequencer Abort stands in a dock row outside the keep row, and folded that row scrolls with the panes, under no
  // other surface. A large notice stood over Abort (RTTY at 900×600 at 175 %, an auto QSO running). A hidden
  // keep-alive cockpit's copy of the same stop has no box and keeps nothing clear.
  it('a stop control outside every listed surface is kept clear: the sequencer Abort in a folded dock', () => {
    const rects = new Map<string, Box>([
      ['cockpit-txdock-keep', { left: 300, top: 520, right: 900, bottom: 600 }],
      ['rtty-auto-stop', { left: 600, top: 270, right: 800, bottom: 330 }],
      ['ui-toast-viewport', { left: 544, top: 100, right: 884, bottom: 488 }],
    ])
    const spy = vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
      const k = [...rects.keys()].find((c) => this.classList.contains(c))
      // The hidden cockpit's copy: no box.
      const r = k && !this.closest('[hidden]') ? rects.get(k)! : { left: 0, top: 0, right: 0, bottom: 0 }
      return { ...r, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, toJSON: () => r } as DOMRect
    })
    const was = [window.innerWidth, window.innerHeight]
    Object.defineProperty(window, 'innerWidth', { value: 900, configurable: true })
    Object.defineProperty(window, 'innerHeight', { value: 600, configurable: true })
    const folded = () => {
      const dock = document.createElement('div')
      dock.className = 'cockpit-txdock'
      dock.dataset.fold = 'on'
      const row = document.createElement('div')
      row.className = 'cw-macros rtty-auto-row'
      const abort = document.createElement('button')
      abort.className = 'cw-macro rtty-auto-stop'
      abort.setAttribute('data-stop-control', 'sequencer-abort')
      row.appendChild(abort)
      const keep = document.createElement('div')
      keep.className = 'cockpit-txdock-keep'
      keep.dataset.fold = 'on'
      dock.append(row, keep)
      return dock
    }
    try {
      const shown = folded()
      const hidden = document.createElement('div')
      hidden.hidden = true
      hidden.appendChild(folded())
      document.body.append(hidden, shown)
      const { viewport } = mount('bottom-right')
      // The lowest free stretch at least one toast tall: between Abort and the keep row.
      expect(viewport.style.getPropertyValue('--toast-inset'), 'the column stands over Abort').toBe(`${600 - (520 - 16)}px`)
      expect(viewport.style.getPropertyValue('--toast-max-h'), 'the column runs up over Abort').toBe(`${520 - 16 - (330 + 16)}px`)
      shown.remove()
      hidden.remove()
    } finally {
      spy.mockRestore()
      Object.defineProperty(window, 'innerWidth', { value: was[0], configurable: true })
      Object.defineProperty(window, 'innerHeight', { value: was[1], configurable: true })
    }
  })

  // WHAT THE OPERATOR IS USING comes second only to the transmit surfaces (features/toastRoom.ts `toastPlace`). A toast's
  // buttons covered an item of the open ⊞ Panels menu (CW at 1366×768, 150 % and 175 %), and Tab left focus behind a
  // toast on a folded dock's macro key (PSK, CW and JTTY at 175 %), all measured in the shell probe (check 13).
  describe('keeps off what the operator is using, and steps aside where it cannot', () => {
    let rects: Map<string, Box>
    let spies: { mockRestore(): void }[]
    const was = [window.innerWidth, window.innerHeight] as const
    beforeEach(() => {
      rects = new Map<string, Box>([
        ['cockpit-txdock-keep', { left: 300, top: 520, right: 900, bottom: 600 }],
        ['ui-toast-viewport', { left: 544, top: 380, right: 884, bottom: 488 }],
      ])
      spies = [
        vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
          const k = [...rects.keys()].find((c) => this.classList.contains(c))
          const r = k ? rects.get(k)! : { left: 0, top: 0, right: 0, bottom: 0 }
          return { ...r, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, toJSON: () => r } as DOMRect
        }),
        // One toast's content: 108 px.
        vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockImplementation(function (this: HTMLElement) {
          return this.classList.contains('ui-toast-viewport') ? 108 : 0
        }),
      ]
      Object.defineProperty(window, 'innerWidth', { value: 900, configurable: true })
      Object.defineProperty(window, 'innerHeight', { value: 600, configurable: true })
    })
    afterEach(() => {
      for (const s of spies) s.mockRestore()
      Object.defineProperty(window, 'innerWidth', { value: was[0], configurable: true })
      Object.defineProperty(window, 'innerHeight', { value: was[1], configurable: true })
    })
    const folded = () => {
      const keep = document.createElement('div')
      keep.className = 'cockpit-txdock-keep'
      keep.dataset.fold = 'on'
      return keep
    }

    it('an open menu over the whole stretch: the column keeps its room clear of the keep row and steps aside', async () => {
      const { viewport } = mount('bottom-right')
      const app = viewport.closest('.app')!
      const keep = folded()
      const menu = document.createElement('div')
      menu.className = 'panels-menu-pop'
      rects.set('panels-menu-pop', { left: 400, top: 10, right: 890, bottom: 510 })
      act(() => {
        app.append(keep, menu)
      })
      await waitFor(() => expect(viewport.hasAttribute('data-step-aside'), 'the column stands over the open menu').toBe(true))
      // Still clear of the keep row: the transmit rule is never traded away.
      expect(viewport.style.getPropertyValue('--toast-inset')).toBe(`${600 - (520 - 16)}px`)
      // Hidden while it steps aside, by the real sheet.
      expect(css(viewport, 'visibility')).toBe('hidden')
      act(() => {
        menu.remove()
      })
      await waitFor(() => expect(viewport.hasAttribute('data-step-aside'), 'the column never came back').toBe(false))
      keep.remove()
    })

    it('a menu that leaves room: the column stands clear of it, drawn', async () => {
      const { viewport } = mount('bottom-right')
      const app = viewport.closest('.app')!
      const keep = folded()
      const menu = document.createElement('div')
      menu.className = 'panels-menu-pop'
      rects.set('panels-menu-pop', { left: 400, top: 10, right: 890, bottom: 300 })
      act(() => {
        app.append(keep, menu)
      })
      await waitFor(() => expect(viewport.style.getPropertyValue('--toast-max-h')).toBe(`${520 - 16 - (300 + 16)}px`))
      expect(viewport.hasAttribute('data-step-aside')).toBe(false)
      menu.remove()
      keep.remove()
    })

    it('JTTY\'s refusal, parked above the keep row\'s own box, is kept clear like the row', async () => {
      const { viewport } = mount('bottom-right')
      const app = viewport.closest('.app')!
      const keep = folded()
      const reason = document.createElement('div')
      reason.setAttribute('role', 'status')
      reason.className = 'jtty-dock-reason'
      keep.appendChild(reason)
      rects.set('jtty-dock-reason', { left: 420, top: 470, right: 890, bottom: 520 })
      act(() => {
        app.append(keep)
      })
      await waitFor(() => expect(viewport.style.getPropertyValue('--toast-inset'), 'the column stands over the refusal').toBe(`${600 - (470 - 16)}px`))
      keep.remove()
    })

    it('the focused control: Tab onto a macro key under the column moves the column off it', async () => {
      const { viewport } = mount('bottom-right')
      const app = viewport.closest('.app')!
      const keep = folded()
      const key = document.createElement('button')
      key.className = 'cw-macro'
      rects.set('cw-macro', { left: 700, top: 440, right: 820, bottom: 480 })
      act(() => {
        app.append(keep, key)
      })
      await waitFor(() => expect(viewport.style.getPropertyValue('--toast-inset')).toBe(`${600 - (520 - 16)}px`))
      act(() => {
        key.focus()
      })
      // Above the key now: the stretch between the window's top edge and the key.
      await waitFor(() => expect(viewport.style.getPropertyValue('--toast-inset'), 'the column stands over the focused key').toBe(`${600 - (440 - 16)}px`))
      act(() => {
        key.blur()
      })
      await waitFor(() => expect(viewport.style.getPropertyValue('--toast-inset'), 'the column did not come back down').toBe(`${600 - (520 - 16)}px`))
      key.remove()
      keep.remove()
    })
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