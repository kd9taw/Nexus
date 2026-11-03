// @vitest-environment jsdom
//
// #445: WHICH CORNER THE POP-UPS STAND IN. Settings ▸ Spots & Alerts ▸ Pop-up corner: bottom right
// (as it always was, and the default), bottom left, top right, top left. The value is a validated
// enum: anything this build does not know reads as bottom right. The swipe that dismisses a toast
// and the way it slides in follow the corner's side. Placement is COMPUTED from the real sheets'
// cascade (cssCascade.testkit) on the rendered column, never matched by regex.
import { describe, it, expect, beforeAll, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Toasts } from './Toasts'
import { dismissToast, pushToast, subscribeToasts, type Toast } from '../toast'
import { popupCornerOf, setPopupCorner, type PopupCorner } from '../features/popupCorner'
import { css, loadSheets, resolveVal } from '../cssCascade.testkit'

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
