// @vitest-environment jsdom
//
// The main window's auto-fit on a FRACTIONALLY scaled display (Windows 125/150%), and the
// one-time prompt that offers it. Measured in real Chrome (2026-11-01): a 1080p panel at 125%
// hands the webview a 1536×824 window, which the classic fit put at 90% — an effective width
// just under where the top bar and the RTTY/PSK docks stop wrapping. 80% there (70% at 150%)
// is the size whose glyphs equal the same panel at 100% scaling.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { act, cleanup, renderHook } from '@testing-library/react'
import { fitScale, scaledFitScale, isFractionalScaling, useScale, type Scale } from './useScale'
import { useScaleFitNotice } from './useScaleFitNotice'
import { dismissToast, subscribeToasts, type Toast } from './toast'

const RULE_KEY = 'nexus-ui-scale-fit-rule'
const MODE_KEY = 'nexus-ui-scale-mode'
const CAP_KEY = 'nexus-ui-scale-cap'

function setWin(w: number, h: number, dpr: number) {
  Object.defineProperty(window, 'innerWidth', { value: w, configurable: true, writable: true })
  Object.defineProperty(window, 'innerHeight', { value: h, configurable: true, writable: true })
  Object.defineProperty(window, 'devicePixelRatio', { value: dpr, configurable: true, writable: true })
}

/** The desktop app (the scaled rule is the desktop's; a browser's ratio is also its own zoom). */
function desktop(on: boolean) {
  const w = window as unknown as { __TAURI_INTERNALS__?: unknown }
  if (on) w.__TAURI_INTERNALS__ = { invoke: () => Promise.resolve(null) }
  else delete w.__TAURI_INTERNALS__
}

/** A matchMedia whose resolution queries can be fired, standing in for a monitor change. */
let mediaListeners: Array<() => void> = []
function stubMatchMedia() {
  mediaListeners = []
  window.matchMedia = ((query: string) => ({
    media: query,
    matches: true,
    addEventListener: (_: string, fn: () => void) => mediaListeners.push(fn),
    removeEventListener: (_: string, fn: () => void) => {
      mediaListeners = mediaListeners.filter((f) => f !== fn)
    },
  })) as unknown as typeof window.matchMedia
}

const frame = () => new Promise((r) => requestAnimationFrame(() => r(null)))

let toasts: Toast[] = []
let unsub = () => {}
beforeEach(() => {
  localStorage.clear()
  window.history.replaceState(null, '', '/')
  desktop(true)
  stubMatchMedia()
  unsub = subscribeToasts((ts) => (toasts = ts))
})
afterEach(() => {
  for (const ts of toasts) dismissToast(ts.id)
  unsub()
  cleanup()
  desktop(false)
  setWin(1024, 768, 1)
})

describe('scaledFitScale', () => {
  // [window, dpr, classic fit, scaled fit]
  const cases: Array<[number, number, number, Scale, Scale]> = [
    [1920, 1040, 1, 100, 100], // 1080p at 100%: unscaled, untouched
    [1536, 824, 1.25, 90, 80], // 1080p at 125%
    [1280, 680, 1.5, 75, 70], // 1080p at 150%
    [2560, 1440, 1.5, 100, 100], // 4K at 150%: room to spare, stays at 100
    [3840, 2100, 1, 100, 100], // 4K at 100%: unscaled
  ]
  it.each(cases)('%i×%i at dpr %s: classic %i, scaled %i', (w, h, dpr, classic, scaled) => {
    expect(fitScale(w, h)).toBe(classic)
    expect(scaledFitScale(w, h, dpr)).toBe(scaled)
  })

  it('also lands on 80 / 70 with the taskbar auto-hidden (the taller window)', () => {
    expect(scaledFitScale(1536, 864, 1.25)).toBe(80)
    expect(scaledFitScale(1280, 720, 1.5)).toBe(70)
  })

  it('never goes below the same panel at 100% scaling (100 / dpr, snapped up)', () => {
    // 1280×680 against the scaled footprint alone is 67% → the 65 step; the floor holds 70.
    for (let w = 1100; w <= 1400; w += 20) {
      for (let h = 600; h <= 800; h += 20) {
        const z = scaledFitScale(w, h, 1.5)
        expect(z >= 70 || z === fitScale(w, h)).toBe(true)
      }
    }
  })

  it('only ever shrinks: never above the classic fit, at any size or cap', () => {
    for (const dpr of [1.25, 1.5, 1.75]) {
      for (const cap of [100, 125, 175] as Scale[]) {
        for (let w = 700; w <= 3000; w += 70) {
          for (let h = 500; h <= 1800; h += 70) {
            expect(scaledFitScale(w, h, dpr, cap)).toBeLessThanOrEqual(fitScale(w, h, cap))
          }
        }
      }
    }
  })

  it('a raised cap is the operator asking for bigger: the classic fit stays', () => {
    expect(scaledFitScale(1536, 824, 1.25, 110)).toBe(fitScale(1536, 824, 110))
    expect(scaledFitScale(2560, 1440, 1.5, 150)).toBe(150)
  })

  it('is the classic fit at an integer ratio (Retina Mac, Windows 200%) and at 1', () => {
    expect(isFractionalScaling(1)).toBe(false)
    expect(isFractionalScaling(2)).toBe(false)
    expect(isFractionalScaling(1.25)).toBe(true)
    expect(isFractionalScaling(1.2499999)).toBe(true)
    expect(isFractionalScaling(1.75)).toBe(true)
    expect(isFractionalScaling(Number.NaN)).toBe(false)
    expect(scaledFitScale(1512, 945, 2)).toBe(fitScale(1512, 945)) // a 14" MacBook stays at 100
  })
})

describe('useScale on a scaled display', () => {
  it('a fresh install at 125% opens at 80, and offers the choice against the classic 90', async () => {
    setWin(1536, 824, 1.25)
    const { result } = renderHook(() => useScale())
    expect(result.current.scale).toBe(80)
    await act(frame)
    expect(result.current.fitOffer).toEqual({ scaled: 80, classic: 90 })
  })

  it('an unscaled screen is unchanged and offers nothing', async () => {
    setWin(1536, 824, 1)
    const { result } = renderHook(() => useScale())
    await act(frame)
    expect(result.current.scale).toBe(90)
    expect(result.current.fitOffer).toBeNull()
  })

  it("respects the operator's explicit zoom: a pin applies verbatim and nothing is offered", async () => {
    localStorage.setItem(MODE_KEY, '100')
    setWin(1536, 824, 1.25)
    const { result } = renderHook(() => useScale())
    await act(frame)
    expect(result.current.scale).toBe(100)
    expect(result.current.fitOffer).toBeNull()
  })

  it('a pop-out keeps its own fit (the Operate pop-out at 125% is not shrunk)', async () => {
    window.history.replaceState(null, '', '/?panel=operate')
    setWin(1536, 824, 1.25)
    const { result } = renderHook(() => useScale())
    await act(frame)
    expect(result.current.scale).toBe(90)
    expect(result.current.fitOffer).toBeNull()
  })

  it('in a browser (Remote) the ratio may be its zoom: the classic fit stays', async () => {
    desktop(false)
    setWin(1536, 824, 1.25)
    const { result } = renderHook(() => useScale())
    await act(frame)
    expect(result.current.scale).toBe(90)
    expect(result.current.fitOffer).toBeNull()
  })

  it('follows the window to a monitor with a different scaling, with no resize event', async () => {
    setWin(1536, 824, 1)
    const { result } = renderHook(() => useScale())
    await act(frame)
    expect(result.current.scale).toBe(90)
    // Windows keeps the logical size across a DPI change: same CSS window, new ratio.
    setWin(1536, 824, 1.25)
    await act(async () => {
      for (const fn of [...mediaListeners]) fn()
      await frame()
    })
    expect(result.current.scale).toBe(80)
    // …and back.
    setWin(1536, 824, 1)
    await act(async () => {
      for (const fn of [...mediaListeners]) fn()
      await frame()
    })
    expect(result.current.scale).toBe(90)
  })
})

describe('the one-time prompt', () => {
  const both = () => {
    const s = useScale()
    useScaleFitNotice(s.fitOffer, s.setFitRule)
    return s
  }

  it('shows once, records the question as asked, and Keep keeps 80', async () => {
    setWin(1536, 824, 1.25)
    const { result, unmount } = renderHook(both)
    await act(frame)
    expect(toasts).toHaveLength(1)
    expect(toasts[0].message).toMatch(/80%/)
    expect(toasts[0].actionLabel).toMatch(/80%/)
    expect(toasts[0].secondaryLabel).toMatch(/90%/)
    // Asked the moment it shows: closing it without a click is the default answer.
    expect(localStorage.getItem(RULE_KEY)).toBe('scaled')
    act(() => toasts[0].action?.())
    expect(result.current.scale).toBe(80)
    unmount()
    for (const ts of toasts) dismissToast(ts.id)

    // The next launch: same screen, no prompt, same size.
    const again = renderHook(both)
    await act(frame)
    expect(again.result.current.scale).toBe(80)
    expect(toasts).toHaveLength(0)
  })

  it('the second button goes back to the classic size, and that answer persists', async () => {
    setWin(1536, 824, 1.25)
    const { result, unmount } = renderHook(both)
    await act(frame)
    expect(toasts).toHaveLength(1)
    act(() => toasts[0].secondaryAction?.())
    await act(frame) // the re-subscribed fit runs on the next frame
    expect(localStorage.getItem(RULE_KEY)).toBe('classic')
    expect(result.current.scale).toBe(90)
    unmount()
    for (const ts of toasts) dismissToast(ts.id)

    const again = renderHook(both)
    expect(again.result.current.scale).toBe(90)
    await act(frame)
    expect(again.result.current.scale).toBe(90)
    expect(toasts).toHaveLength(0)
  })

  it('never shows where the two rules agree (unscaled, or a scaled 4K)', async () => {
    setWin(1920, 1040, 1)
    renderHook(both)
    await act(frame)
    setWin(2560, 1440, 1.5)
    renderHook(both)
    await act(frame)
    expect(toasts).toHaveLength(0)
    expect(localStorage.getItem(RULE_KEY)).toBeNull() // nothing asked, nothing written
  })
})

describe('corrupt saved values are clamped on load', () => {
  it('a damaged rule reads as the default rule and never re-asks', async () => {
    localStorage.setItem(RULE_KEY, '{"oops":')
    setWin(1536, 824, 1.25)
    const { result } = renderHook(() => {
      const s = useScale()
      useScaleFitNotice(s.fitOffer, s.setFitRule)
      return s
    })
    await act(frame)
    expect(result.current.scale).toBe(80)
    expect(toasts).toHaveLength(0)
  })

  it.each(['87', '9999', '-5', 'NaN', '', '1e3'])(
    'a stored zoom of %j falls back to auto-fit on the ladder',
    async (raw) => {
      localStorage.setItem(MODE_KEY, raw)
      localStorage.setItem(CAP_KEY, raw)
      setWin(1536, 824, 1.25)
      const { result } = renderHook(() => useScale())
      await act(frame)
      expect(result.current.mode).toBe('auto')
      expect(result.current.cap).toBe(100)
      expect(result.current.scale).toBe(80)
    },
  )
})
