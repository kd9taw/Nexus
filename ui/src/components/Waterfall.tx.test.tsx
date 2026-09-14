// @vitest-environment jsdom
//
// #230 (W9GTY) — "on RTTY transmit the waterfall goes out of focus and does not show the
// transmitted signal". The receive picture is HELD while we key: the backend drops the rows
// captured from the rig's muted receiver (SpectrumFeed's tx hold) and readers repeat the last
// real picture. That is deliberate, and on a surface that does not paint the FT dark band it is
// indistinguishable from a dead waterfall — which is what the report describes.
//
// So the frozen picture gets a label saying it is frozen. No transmit-path or audio change: the
// waterfall is never fed our own TX audio.
//
// The label belongs to the surfaces that FREEZE (RTTY, PSK, SSTV, JS8 — no `txBlanks`); an FT
// surface paints its dark band and says it that way, which is the control below.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
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
    <Waterfall
      transmitting={false}
      rxOffsetHz={1500}
      txOffsetHz={1500}
      theme="dark"
      {...props}
    />
  )
}

/** The held-display label, found the way an operator reads it — by its words — with its live
 *  region role asserted separately (role=status takes its name from aria-label, not content). */
const label = () => screen.queryByText(/transmit/i)

describe('the held waterfall says so while keying (#230)', () => {
  it('keyed on a freezing surface: the label is there and names the held display', () => {
    render(view({ keyed: true }))
    const held = label()
    expect(held, 'no transmitting label on a frozen waterfall').not.toBeNull()
    expect(held!.getAttribute('role'), 'the label is not announced').toBe('status')
  })

  it('unkeyed: the label is gone', () => {
    const { rerender, container } = render(view({ keyed: true }))
    const canvas = container.querySelector('canvas.waterfall-canvas')
    expect(label()).not.toBeNull()
    rerender(view({ keyed: false }))
    expect(label(), 'the label outlived the transmission').toBeNull()
    // The picture is RETAINED across the transition: the same canvas, never remounted (a
    // remount would clear the backing store and blank the history the hold exists to keep).
    expect(container.querySelector('canvas.waterfall-canvas')).toBe(canvas)
  })

  it('control: an FT surface (txBlanks) keys without the label — its dark band says it', () => {
    render(view({ keyed: true, txBlanks: true, transmitting: true }))
    expect(label()).toBeNull()
  })

  it('control: not keyed, no label', () => {
    render(view())
    expect(label()).toBeNull()
  })
})
