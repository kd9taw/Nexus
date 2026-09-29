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
  document.documentElement.style.removeProperty('--ui-zoom')
})

describe('usePaneWidths clamp-on-load', () => {
  it('clamps a stored width against the CURRENT window at mount, without re-persisting', () => {
    setInnerWidth(1366)
    localStorage.setItem('tempo-right-rail-w', '2064') // legal on 3440, not here
    const { result } = renderHook(() => usePaneWidths(100))
    // Its own 60 % ceiling is 820, but beside the stations rail's 246 that would leave the
    // conversation 140 px: the pair gets 1366 − 160 − 360 = 846, so the waterfall rail gets 626
    // and the stations rail gives way to its 220 floor.
    expect(result.current.rightW).toBe(626)
    expect(document.documentElement.style.getPropertyValue('--right-rail-w')).toBe('626px')
    // The 3440-monitor preference must survive: no write-back of the clamped value.
    expect(localStorage.getItem('tempo-right-rail-w')).toBe('2064')
  })

  it('re-clamps on window resize — and grows back toward the stored preference', async () => {
    setInnerWidth(1366)
    localStorage.setItem('tempo-right-rail-w', '2064')
    const { result } = renderHook(() => usePaneWidths(100))
    expect(result.current.rightW).toBe(626)
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
    expect(result.current.rightW).toBe(626)
  })

  it('commit clamps against its own ceiling AND the room the other rail leaves, and persists', () => {
    setInnerWidth(1366)
    const { result } = renderHook(() => usePaneWidths(100))
    act(() => result.current.commitRight(5000))
    // The stations rail shows its default 246; the pair gets 846, so 600 — not the 820 ceiling.
    expect(result.current.rightW).toBe(600)
    expect(result.current.leftW).toBe(246)
    expect(localStorage.getItem('tempo-right-rail-w')).toBe('600')
  })

  it('clamps the left rail too (40% ceiling, 220 floor)', () => {
    setInnerWidth(1366)
    localStorage.setItem('tempo-left-rail-w', '1200')
    const { result } = renderHook(() => usePaneWidths(100))
    expect(result.current.leftW).toBe(Math.round(1366 * 0.4)) // 546
    expect(localStorage.getItem('tempo-left-rail-w')).toBe('1200')
  })
})

// ── THE PAIR (layout L1, ruling L1-1) ───────────────────────────────────────────────────────
// Each rail keeps its own limits (≥ 220 / ≥ 260, ≤ 40 % / ≤ 60 % of the effective width), and the
// two together never take the conversation between them below CENTER_MIN (360 CSS px). Everything
// else across the window at md and up — the navigation rail, the layout's padding and its four
// gaps — is RAIL_CHROME (160 px), so the pair shares floor(effective width − 520). When the pair
// does not fit, the rail the operator set LAST keeps its width and the other gives way; with
// neither known, both shrink in proportion (connectRails.fitRails' shape). Stored widths are the
// operator's preference and a re-fit never writes them.
//
// jsdom DOES return a custom property set inline on <html>, so these cases set --ui-zoom itself.
describe('the two rails never squeeze the conversation (L1-1)', () => {
  const zoom = (z: number) => document.documentElement.style.setProperty('--ui-zoom', String(z))
  /** The conversation's width, as the grid lays it out at md and up. */
  const centre = (ew: number, l: number, r: number) => ew - 160 - l - r

  it('stored 900 / 1400 px from a wide monitor, opened at 1366×768 (85 %): the conversation keeps its floor', () => {
    setInnerWidth(1366)
    zoom(0.85) // effective width 1607
    localStorage.setItem('tempo-left-rail-w', '900')
    localStorage.setItem('tempo-right-rail-w', '1400')
    const { result } = renderHook(() => usePaneWidths(85))
    // Each alone: 643 (40 %) and 964 (60 %) — together the whole window, a 0 px conversation.
    // As a pair, in proportion into the 1087 they may share:
    expect([result.current.leftW, result.current.rightW]).toEqual([434, 652])
    expect(centre(1366 / 0.85, 434, 652)).toBeGreaterThanOrEqual(360)
    expect(localStorage.getItem('tempo-left-rail-w'), 'a re-fit never writes the preference').toBe('900')
    expect(localStorage.getItem('tempo-right-rail-w')).toBe('1400')
  })

  it('the same squeeze made by dragging on a big window, then shrinking it: the rail set last keeps its width', async () => {
    setInnerWidth(1920)
    zoom(1)
    const { result } = renderHook(() => usePaneWidths(100))
    act(() => result.current.commitLeft(700))
    // Dragging the waterfall rail wide now stops where the conversation would drop below its floor.
    act(() => result.current.commitRight(1100))
    expect([result.current.leftW, result.current.rightW]).toEqual([700, 700])
    expect(centre(1920, 700, 700)).toBe(360)
    setInnerWidth(1366)
    zoom(0.85)
    await act(async () => {
      window.dispatchEvent(new Event('resize'))
    })
    await flushRaf()
    // 1087 to share: the waterfall rail (set last) keeps its 700, the stations rail takes 387.
    expect([result.current.leftW, result.current.rightW]).toEqual([387, 700])
    expect(centre(1366 / 0.85, 387, 700)).toBeGreaterThanOrEqual(360)
    // And a bigger window gives both back.
    setInnerWidth(1920)
    zoom(1)
    await act(async () => {
      window.dispatchEvent(new Event('resize'))
    })
    await flushRaf()
    expect([result.current.leftW, result.current.rightW]).toEqual([700, 700])
  })

  it('moving one rail never changes the other’s width — it stops where the room ends', () => {
    setInnerWidth(1366)
    zoom(0.85)
    localStorage.setItem('tempo-left-rail-w', '900')
    localStorage.setItem('tempo-right-rail-w', '1400')
    const { result } = renderHook(() => usePaneWidths(85))
    act(() => result.current.commitLeft(600))
    expect([result.current.leftW, result.current.rightW]).toEqual([435, 652])
    expect(result.current.leftMax, 'the ceiling the divider announces is the room it has').toBe(435)
    expect(result.current.rightMax).toBe(652)
  })

  it('below the floor the rails hold their minimums and the conversation gives — nothing is trapped', () => {
    setInnerWidth(900)
    zoom(1) // 900 − 520 = 380 to share: less than the 480 the two minimums need
    localStorage.setItem('tempo-left-rail-w', '400')
    localStorage.setItem('tempo-right-rail-w', '500')
    const { result } = renderHook(() => usePaneWidths(100))
    expect([result.current.leftW, result.current.rightW]).toEqual([220, 260])
  })

  it('a reset puts one rail back as far as the room beside the other allows, and the other stays', () => {
    setInnerWidth(1366)
    zoom(0.85)
    localStorage.setItem('tempo-left-rail-w', '900')
    localStorage.setItem('tempo-right-rail-w', '1400')
    const { result } = renderHook(() => usePaneWidths(85))
    act(() => result.current.resetRight())
    // Its default at 1607 is 354 (22 %), and 1087 − 434 leaves room for it.
    expect([result.current.leftW, result.current.rightW]).toEqual([434, 354])
  })
})
