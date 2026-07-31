// @vitest-environment jsdom
//
// The portal-zoom contract (assessment-2026-07-30 V3): Radix Dialog/Tooltip portal
// to document.body — OUTSIDE `.app`'s `zoom: var(--ui-zoom)` — so their content
// rendered at 1/zoom of the app (1.54x too large at auto-65; 0.57x at pinned 175,
// the accessibility inversion). The fix re-applies the zoom on an INNER wrapper,
// leaving the portal element's own box (.ui-dialog / .ui-tooltip) in real viewport
// units, which its top/max-height/width rules depend on. These tests pin that
// structure: content lives inside a zoom-carrying wrapper; the box does not zoom.
import { describe, it, expect, beforeAll, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { Dialog } from './Dialog'
import { Tooltip, TooltipProvider } from './Tooltip'

// Radix Popper (Tooltip) observes its elements with a ResizeObserver jsdom lacks.
beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})

afterEach(cleanup)

const ZOOM = 'var(--ui-zoom, 1)'

/** The inner wrapper that re-applies the app zoom inside a portaled box. */
function zoomWrapper(box: Element): HTMLElement | undefined {
  return Array.from(box.querySelectorAll('div')).find((d) => d.style.zoom === ZOOM)
}

describe('Dialog portal zoom', () => {
  it('renders title, description and children inside a zoom-carrying wrapper', () => {
    render(
      <Dialog open onOpenChange={() => {}} title="Portal test" description="a description">
        <button>dialog-child</button>
      </Dialog>,
    )
    const box = document.querySelector<HTMLElement>('.ui-dialog')
    expect(box, 'dialog content mounts on document.body').toBeTruthy()
    const wrapper = zoomWrapper(box!)
    expect(wrapper, 'dialog must re-apply --ui-zoom on an inner wrapper').toBeTruthy()
    // Everything the operator reads must scale: title and description included.
    expect(wrapper!.contains(screen.getByText('dialog-child'))).toBe(true)
    expect(wrapper!.querySelector('.ui-dialog-title')).toBeTruthy()
    expect(wrapper!.querySelector('.ui-dialog-desc')).toBeTruthy()
    // The box itself must NOT zoom — its 12vh/84vh/width rules measure the real
    // window precisely because the portal escapes the app zoom.
    expect(box!.style.zoom).not.toBe(ZOOM)
  })
})

describe('Tooltip portal zoom', () => {
  it('renders content inside a zoom-carrying wrapper; arrow stays outside it', () => {
    render(
      <TooltipProvider>
        <Tooltip content="tip-body">
          <button>trigger</button>
        </Tooltip>
      </TooltipProvider>,
    )
    // Focus opens a Radix tooltip immediately (no pointer delay path).
    fireEvent.focus(screen.getByText('trigger'))
    const box = document.querySelector<HTMLElement>('.ui-tooltip')
    expect(box, 'tooltip content mounts on document.body').toBeTruthy()
    const wrapper = zoomWrapper(box!)
    expect(wrapper, 'tooltip must re-apply --ui-zoom on an inner wrapper').toBeTruthy()
    expect(wrapper!.textContent).toContain('tip-body')
    // Radix positions the arrow absolutely against the popper box; a zoom on an
    // ancestor would mis-scale those offsets, so it must not sit in the wrapper.
    const arrow = box!.querySelector('.ui-tooltip-arrow')
    if (arrow) expect(wrapper!.contains(arrow)).toBe(false)
    expect(box!.style.zoom).not.toBe(ZOOM)
  })
})
