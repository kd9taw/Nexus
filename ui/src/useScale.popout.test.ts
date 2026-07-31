// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { useScale } from './useScale'

// Pin policy per surface (census item 2): in the MAIN window an explicit pin is an
// operator choice — honoured verbatim, never silently overridden. A pop-out has no
// scale UI and inherits the pin via the bare-key fallback, so a pin above what that
// window can even fit (a 175 pin in a 380×180-min waterfall window → 217×103 CSS px)
// had no in-window recovery path — the applied scale is capped at the window's own
// fit ceiling, in the LOAD path so the first paint is already correct.

function setWin(w: number, h: number) {
  Object.defineProperty(window, 'innerWidth', { value: w, configurable: true, writable: true })
  Object.defineProperty(window, 'innerHeight', { value: h, configurable: true, writable: true })
}

beforeEach(() => localStorage.clear())
afterEach(() => window.history.replaceState(null, '', '/'))

describe('useScale pinned-scale surface policy', () => {
  it('main window: an explicit pin applies verbatim even when it does not fit', () => {
    window.history.replaceState(null, '', '/')
    localStorage.setItem('nexus-ui-scale-mode', '175')
    setWin(900, 600)
    const { result } = renderHook(() => useScale())
    expect(result.current.scale).toBe(175)
    expect(result.current.mode).toBe(175)
  })

  it('pop-out: an inherited pin is capped at the window fit ceiling on load', () => {
    window.history.replaceState(null, '', '/?panel=waterfall')
    localStorage.setItem('nexus-ui-scale-mode', '175') // main's pin, inherited (bare key)
    setWin(900, 300) // torn-off waterfall strip — fitScale ceiling is the 65 floor
    const { result } = renderHook(() => useScale())
    expect(result.current.scale).toBe(65)
    // The PIN itself is not rewritten — only the applied scale is capped, so the main
    // window's stored choice is never clobbered from a pop-out.
    expect(result.current.mode).toBe(175)
  })

  it('pop-out: a pin at or under the ceiling still applies as pinned', () => {
    window.history.replaceState(null, '', '/?panel=operate')
    localStorage.setItem('nexus-ui-scale-mode', '70')
    setWin(1140, 760) // default operate pop-out — ceiling 80, pin 70 fits
    const { result } = renderHook(() => useScale())
    expect(result.current.scale).toBe(70)
  })

  it('pop-out: the pin cap re-evaluates on window RESIZE, not only at open', async () => {
    // The cap was sampled once at state init: a waterfall strip opened at its 380×180
    // minimum capped an inherited 175 pin to the 65 floor — and then STAYED at 65 after
    // the operator maximized the window, the exact "no in-window recovery path"
    // stranding capPinnedScale exists to remove (review 2026-07-31). The pinned branch
    // must re-run the cap on resize like the auto branch re-runs its fit.
    window.history.replaceState(null, '', '/?panel=waterfall')
    localStorage.setItem('nexus-ui-scale-mode', '175')
    setWin(900, 300)
    const { result } = renderHook(() => useScale())
    expect(result.current.scale).toBe(65)
    setWin(1920, 1080) // maximized — fit ceiling is now 110
    await act(async () => {
      window.dispatchEvent(new Event('resize'))
      await new Promise((r) => requestAnimationFrame(() => r(null)))
    })
    expect(result.current.scale).toBe(110)
    expect(result.current.mode).toBe(175) // the stored pin is still never rewritten
  })
})
