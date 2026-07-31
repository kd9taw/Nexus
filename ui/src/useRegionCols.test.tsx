// @vitest-environment jsdom
//
// The pane region's column tier (2026-07-30 layout assessment, design3 §3). The tier is
// measured from the REGION, not the window: a splitter drag, a pop-out or a future rail
// changes the region's width with the window untouched, and `data-viewport` cannot see
// that. Two properties are pinned here because both have burned this codebase before:
//   1. the thresholds are pure and boundary-exact (a size class that disagrees with the
//      CSS template is the "dead rule" failure mode in attribute form), and
//   2. a HIDDEN region (keep-alive host, mid-layout) measures 0×0 and must KEEP its last
//      tier — re-tiering to 1 there means every view switch flashes a single-column
//      relayout (PhoneScope.tsx's resize guard, same reason).
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, screen, cleanup, act } from '@testing-library/react'
import { classifyRegionCols, useRegionCols } from './useRegionCols'

/** The observed element's callback, so a test can fire a resize the way the browser does. */
let fire: (() => void) | null = null
let observed: Element | null = null
let disconnected = false

beforeEach(() => {
  fire = null
  observed = null
  disconnected = false
  globalThis.ResizeObserver = class {
    constructor(cb: () => void) {
      fire = cb
    }
    observe(el: Element) {
      observed = el
    }
    disconnect() {
      disconnected = true
    }
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

/** jsdom has no layout: clientWidth is 0 unless stubbed. */
function stubWidth(el: HTMLElement, w: number) {
  Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => w })
}

function Region({ max }: { max?: 1 | 2 | 3 }) {
  const { ref, cols } = useRegionCols<HTMLDivElement>(max)
  return (
    <div data-testid="region" ref={ref}>
      <span data-testid="cols">{cols}</span>
    </div>
  )
}

/** Flush the hook's rAF debounce. */
async function frame() {
  await act(async () => {
    await new Promise((r) => requestAnimationFrame(() => r(null)))
  })
}

describe('classifyRegionCols', () => {
  it('maps region width to the column tier at each boundary', () => {
    expect(classifyRegionCols(0)).toBe(1)
    expect(classifyRegionCols(900)).toBe(1)
    expect(classifyRegionCols(1079)).toBe(1)
    expect(classifyRegionCols(1080)).toBe(2)
    expect(classifyRegionCols(1470)).toBe(2) // 1200×750 @ 80% zoom — the default window
    expect(classifyRegionCols(1699)).toBe(2)
    expect(classifyRegionCols(1700)).toBe(3)
    expect(classifyRegionCols(3390)).toBe(3) // 3440 fullscreen
  })
})

describe('useRegionCols', () => {
  it('stamps data-cols on the region itself and reports the same tier', async () => {
    const { rerender } = render(<Region />)
    const el = screen.getByTestId('region')
    stubWidth(el, 1800)
    fire!()
    await frame()
    rerender(<Region />)
    expect(el.getAttribute('data-cols')).toBe('3')
    expect(screen.getByTestId('cols').textContent).toBe('3')
    expect(observed).toBe(el)
  })

  it('re-tiers when the region (not the window) narrows', async () => {
    render(<Region />)
    const el = screen.getByTestId('region')
    stubWidth(el, 1800)
    fire!()
    await frame()
    expect(el.getAttribute('data-cols')).toBe('3')
    stubWidth(el, 1200) // e.g. a scope splitter drag / rail opening, window unchanged
    fire!()
    await frame()
    expect(el.getAttribute('data-cols')).toBe('2')
  })

  it('keeps the last tier while hidden (0×0) instead of flashing back to 1 column', async () => {
    render(<Region />)
    const el = screen.getByTestId('region')
    stubWidth(el, 1800)
    fire!()
    await frame()
    expect(el.getAttribute('data-cols')).toBe('3')
    stubWidth(el, 0) // keep-alive host hid the cockpit
    fire!()
    await frame()
    expect(el.getAttribute('data-cols')).toBe('3')
  })

  it('always stamps a tier, even when the region has never been measurable', async () => {
    render(<Region />) // clientWidth stays 0 — CSS must never see a region with no tier
    const el = screen.getByTestId('region')
    expect(el.getAttribute('data-cols')).toBe('1')
  })

  it('caps the tier at the number of column groups the cockpit can actually fill', async () => {
    // Phone with no rig connected has no aux column: a 3-track template would leave a
    // permanently empty middle — the "empty black box" complaint, rebuilt.
    const { rerender } = render(<Region max={2} />)
    const el = screen.getByTestId('region')
    stubWidth(el, 1800)
    fire!()
    await frame()
    expect(el.getAttribute('data-cols')).toBe('2')
    // The rig connects: the cap lifts and the REMEMBERED measurement re-stamps, with no
    // resize event to prompt it.
    rerender(<Region max={3} />)
    expect(el.getAttribute('data-cols')).toBe('3')
  })

  it('debounces bursts to one measurement per frame and disconnects on unmount', async () => {
    const { unmount } = render(<Region />)
    const el = screen.getByTestId('region')
    const widths: number[] = []
    Object.defineProperty(el, 'clientWidth', {
      configurable: true,
      get: () => {
        widths.push(1)
        return 1800
      },
    })
    const before = widths.length
    fire!()
    fire!()
    fire!()
    await frame()
    expect(widths.length - before).toBe(1)
    unmount()
    expect(disconnected).toBe(true)
  })

  it('does not throw where ResizeObserver is unavailable (torn-off window boot order)', () => {
    const saved = globalThis.ResizeObserver
    // @ts-expect-error deliberately removing the global for this case
    delete globalThis.ResizeObserver
    expect(() => render(<Region />)).not.toThrow()
    globalThis.ResizeObserver = saved
    vi.restoreAllMocks()
  })
})
