// @vitest-environment jsdom
//
// THE SPECTRUM STRIP'S OWN ✕ (2026-09-15).
//
// Five cockpits render this component as a ⊞-removable pane — Operate's `waterfall` and the
// `scope` entry Phone/CW/RTTY/PSK/SSTV/JS8 share — and none of them could close it from the
// strip itself until now. Both cockpit sweeps (paneClose.test.tsx, OperateCockpit.paneClose
// .test.tsx) STUB this component, because it is a canvas; so the button that lives inside it
// is computed here, where the real one renders.
//
// WHAT IT COMPUTES: the ✕ is there when the host hands down a hide, it is NOT there when the
// host does not (a surface with no panel record — the torn-off waterfall window — must show
// no button rather than a dead one), and pressing it calls the hide. What the hide DOES is
// the cockpit's business and is asserted in the cockpit sweeps against a live record.
//
// THE STOP LINE: this strip hosts no control that stops a transmission — palette, floor,
// resolution, pause (a RENDER pause), scroll direction, pop-out and click-to-tune, which
// moves the dial and keys nothing. So its hide ends nothing and it carries no warning, and
// that absence is asserted rather than assumed.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { Waterfall } from './Waterfall'

vi.mock('../api', () => ({
  getSpectrumRow: () => Promise.resolve({ row: [], loHz: 200, hiHz: 4000 }),
}))

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

function view(props: Partial<React.ComponentProps<typeof Waterfall>> = {}) {
  return (
    <Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1500} theme="dark" {...props} />
  )
}

describe('the spectrum strip carries its own ✕', () => {
  it('names the strip the cockpit calls it, and presses through to the hide', () => {
    const onRemove = vi.fn()
    render(view({ onRemove, paneTitle: 'Waterfall' }))
    const close = screen.getByRole('button', { name: 'Hide Waterfall' })
    fireEvent.click(close)
    expect(onRemove).toHaveBeenCalledTimes(1)
  })

  it('takes the cockpit\'s own word for it — "Scope" in Phone and CW', () => {
    render(view({ onRemove: vi.fn(), paneTitle: 'Scope' }))
    expect(screen.getByRole('button', { name: 'Hide Scope' })).toBeTruthy()
  })

  it('no hide handed down ⇒ no button at all, not a dead one', () => {
    render(view({ paneTitle: 'Waterfall' }))
    // POSITIVE CONTROL: the strip DID render its header row, so the absence below is the
    // missing prop and not a component that failed to mount.
    expect(screen.getAllByRole('button').length).toBeGreaterThan(0)
    expect(screen.queryByRole('button', { name: /^Hide / })).toBeNull()
  })

  it('carries no consequence warning — hiding a spectrum display ends nothing', () => {
    render(view({ onRemove: vi.fn(), paneTitle: 'Waterfall' }))
    const close = screen.getByRole('button', { name: 'Hide Waterfall' })
    // A note the operator cannot act on teaches him to ignore the next one (THE PRACTICE,
    // features/panelState.ts). The tooltip says only how to get the strip back.
    expect(close.getAttribute('aria-describedby')).toBeNull()
    expect(close.getAttribute('title')).toMatch(/panels menu/i)
  })
})
