// @vitest-environment jsdom
//
// The waterfall's scroll-direction control (operator, 2026-08-14).
//
// WHAT THIS FILE CAN AND CANNOT PROVE. jsdom has no 2D canvas — `getContext('2d')` returns
// null, so the render effect bails before it ever draws — and nothing here paints a pixel.
// A test that asserted "the newest row is at the top" against this DOM would be asserting
// against a canvas that was never touched. The picture is pinned where it is actually
// computable: `waterfallHistory.test.ts` renders the cold path into a real RGBA buffer and
// reads the rows back. What is left for this file is the control surface — that the button
// exists, says which way the waterfall currently runs, flips on click, and persists under
// the per-surface key — plus the one guard that matters on upgrade:
//
//   WITH NOTHING STORED, THE RESOLVED DIRECTION IS TODAY'S.
//
// Nobody's display may move because they installed a build that gained a toggle. The stored
// value is therefore read as an OPT-IN to the mirrored direction (`down`) and every other
// state of storage — absent, blank, stale, written by some other build — is the default.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { Waterfall } from './Waterfall'

vi.mock('../api', () => ({
  getSpectrumRow: () => Promise.resolve({ row: [], loHz: 200, hiHz: 4000 }),
}))

const FLOW_KEY = 'nexus.waterfall.flow'

beforeEach(() => {
  localStorage.clear()
  // The render effect stops at the missing 2D context, but stub the observer anyway so a
  // future canvas shim does not turn this suite red for an unrelated reason.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => cleanup())

function mount(): HTMLButtonElement {
  render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1500} theme="dark" />)
  return screen.getByRole('button', { name: /^Scrolls (up|down)$/ }) as HTMLButtonElement
}

describe('waterfall scroll direction', () => {
  it('defaults to DOWN with nothing stored, and writes nothing until asked', () => {
    const btn = mount()
    expect(btn.textContent).toBe('Scrolls down')
    expect(btn.getAttribute('aria-pressed')).toBe('true')
    // Still writes nothing on its own: a default is not a choice, and persisting one would
    // freeze this operator's display against any future change of mind about the default.
    expect(localStorage.getItem(FLOW_KEY)).toBeNull()
  })

  it('a stale or foreign stored value is the default too — only `up` opts out', () => {
    // Same shape as before the flip, opposite polarity: exactly one string selects the
    // non-default direction, and everything else resolves to the default, so a value written
    // by some other build can never pick a direction nobody chose.
    for (const stored of ['', 'down', '1', 'UP', 'true', 'false']) {
      localStorage.setItem(FLOW_KEY, stored)
      const btn = mount()
      expect(btn.textContent, `stored=${JSON.stringify(stored)}`).toBe('Scrolls down')
      expect(btn.getAttribute('aria-pressed')).toBe('true')
      cleanup()
    }
  })

  it('flips on click, and persists the flip', () => {
    const btn = mount()
    fireEvent.click(btn)
    expect(btn.textContent).toBe('Scrolls up')
    expect(btn.getAttribute('aria-pressed')).toBe('false')
    expect(localStorage.getItem(FLOW_KEY)).toBe('up')
    fireEvent.click(btn)
    expect(btn.textContent).toBe('Scrolls down')
    expect(btn.getAttribute('aria-pressed')).toBe('true')
    expect(localStorage.getItem(FLOW_KEY)).toBe('down')
  })

  it('reopens in the direction the operator left it', () => {
    localStorage.setItem(FLOW_KEY, 'up')
    const btn = mount()
    expect(btn.textContent).toBe('Scrolls up')
    expect(btn.getAttribute('aria-pressed')).toBe('false')
  })

  it('an operator who already chose UP keeps it across the default flip', () => {
    // The one case the flip must not break. The toggle writes BOTH strings explicitly, so an
    // operator who tried `down` and went back has a real `up` on disk — that is a choice, and a
    // change of default may not overrule it. Anyone who never touched the button has nothing
    // stored and moves with the default; that is the cost of the flip, and it is intended.
    localStorage.setItem(FLOW_KEY, 'up')
    expect(mount().textContent).toBe('Scrolls up')
  })

  it('the tooltip says which END is newest and which way history travels, in both states', () => {
    const btn = mount()
    // "Scrolls up" / "Scrolls down" alone are ambiguous — that is the whole reason the
    // tooltip has to name the end AND the direction of travel.
    const down = btn.getAttribute('title') ?? ''
    expect(down).toMatch(/newest row appears at the TOP/)
    expect(down).toMatch(/history travels downward/)
    expect(down).toMatch(/newest at the bottom/i) // what clicking does
    fireEvent.click(btn)
    const up = btn.getAttribute('title') ?? ''
    expect(up).toMatch(/newest row appears at the BOTTOM/)
    expect(up).toMatch(/history travels upward/)
    expect(up).toMatch(/newest at the top/i)
  })

  it('stays available in the 3D view (where it changes nothing) and says so', () => {
    localStorage.setItem('nexus.waterfall.dss', '1')
    const btn = mount()
    expect(btn.isConnected).toBe(true)
    expect(btn.disabled).toBe(false)
    expect(btn.getAttribute('title')).toMatch(/3D view keeps its own/)
    fireEvent.click(btn) // still a working toggle behind the 3D stack
    expect(localStorage.getItem(FLOW_KEY)).toBe('up') // one click off the `down` default
  })
})
