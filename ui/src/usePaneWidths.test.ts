// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { usePaneWidths } from './usePaneWidths'

// Clamp-on-load for the persisted rail widths (census: a 2064px rail persisted on a
// 3440-wide monitor replayed raw on a 1366 laptop zeroes the center pane). The clamp
// is APPLY-side only: storage keeps the operator's preferred width so the big-monitor
// layout comes back when the room does.

function setInnerWidth(w: number) {
  Object.defineProperty(window, 'innerWidth', { value: w, configurable: true, writable: true })
}

/** Flush the hook's rAF-debounced re-clamp (mirrors useViewport's deferral). */
const flushRaf = () =>
  act(async () => {
    await new Promise((r) => requestAnimationFrame(() => r(null)))
  })

beforeEach(() => {
  localStorage.clear()
  document.documentElement.style.removeProperty('--right-rail-w')
  document.documentElement.style.removeProperty('--left-rail-w')
})

describe('usePaneWidths clamp-on-load', () => {
  it('clamps a stored width against the CURRENT window at mount, without re-persisting', () => {
    setInnerWidth(1366)
    localStorage.setItem('tempo-right-rail-w', '2064') // legal on 3440, not here
    const { result } = renderHook(() => usePaneWidths(100))
    expect(result.current.rightW).toBe(Math.round(1366 * 0.6)) // 820
    expect(document.documentElement.style.getPropertyValue('--right-rail-w')).toBe('820px')
    // The 3440-monitor preference must survive: no write-back of the clamped value.
    expect(localStorage.getItem('tempo-right-rail-w')).toBe('2064')
  })

  it('re-clamps on window resize — and grows back toward the stored preference', async () => {
    setInnerWidth(1366)
    localStorage.setItem('tempo-right-rail-w', '2064')
    const { result } = renderHook(() => usePaneWidths(100))
    expect(result.current.rightW).toBe(820)
    setInnerWidth(3440)
    await act(async () => {
      window.dispatchEvent(new Event('resize'))
    })
    await flushRaf()
    expect(result.current.rightW).toBe(2064) // preference restored, not the clamp
  })

  it('re-clamps when the UI scale changes (the ceilings are zoom-relative)', async () => {
    setInnerWidth(3440)
    localStorage.setItem('tempo-right-rail-w', '2000')
    const { result, rerender } = renderHook(({ s }: { s: number }) => usePaneWidths(s), {
      initialProps: { s: 100 },
    })
    expect(result.current.rightW).toBe(2000)
    // A zoom change moves effWidth without firing a resize event — the scale dep
    // must re-run the clamp. (jsdom can't resolve --ui-zoom from computed style, so
    // the width shift stands in for the effWidth shift the zoom causes.)
    setInnerWidth(1366)
    rerender({ s: 65 })
    await flushRaf()
    expect(result.current.rightW).toBe(820)
  })

  it('commit clamps AND persists (unchanged behavior)', () => {
    setInnerWidth(1366)
    const { result } = renderHook(() => usePaneWidths(100))
    act(() => result.current.commitRight(5000))
    expect(result.current.rightW).toBe(820)
    expect(localStorage.getItem('tempo-right-rail-w')).toBe('820')
  })

  it('clamps the left rail too (40% ceiling, 220 floor)', () => {
    setInnerWidth(1366)
    localStorage.setItem('tempo-left-rail-w', '1200')
    const { result } = renderHook(() => usePaneWidths(100))
    expect(result.current.leftW).toBe(Math.round(1366 * 0.4)) // 546
    expect(localStorage.getItem('tempo-left-rail-w')).toBe('1200')
  })
})
