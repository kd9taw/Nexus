// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import type { RefObject } from 'react'
import { renderHook } from '@testing-library/react'
import { useWheelTune } from './useWheelTune'
import { setFrequency } from './api'

// 20 m only, 14.000–14.350 MHz — a compact band gate for edge tests.
vi.mock('./band', () => ({
  bandLabelForMhz: (mhz: number) => (mhz >= 14 && mhz <= 14.35 ? '20m' : null),
}))
vi.mock('./api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)) }))

const mockSetFreq = setFrequency as unknown as ReturnType<typeof vi.fn>

type Opts = Parameters<typeof useWheelTune>[1]

function mountHook(props: Opts) {
  return mountHookFull(props).el
}

/** `mountHook` plus the handles the per-digit tests need: `rerender` to move the TX gate
 *  mid-flight, and `result` for the hook's own Hz applier (the keyboard's route in). */
function mountHookFull(props: Opts) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const ref = { current: el } as RefObject<HTMLElement | null>
  const { rerender, result } = renderHook((p: Opts) => useWheelTune(ref, p), { initialProps: props })
  return { el, rerender, result }
}

function wheel(el: HTMLElement, init: WheelEventInit): WheelEvent {
  const e = new WheelEvent('wheel', { cancelable: true, bubbles: true, ...init })
  el.dispatchEvent(e)
  return e
}

describe('useWheelTune', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSetFreq.mockClear()
  })
  afterEach(() => vi.useRealTimers())

  it('wheel up tunes up by one step, flushed once after the throttle window', () => {
    const el = mountHook({ dialMhz: 14.1, sideband: 'USB', enabled: true, stepHz: 100 })
    wheel(el, { deltaY: -100 })
    expect(mockSetFreq).not.toHaveBeenCalled() // throttled — nothing yet
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    const [mhz, band, sb] = mockSetFreq.mock.calls[0]
    expect(mhz).toBeCloseTo(14.1001, 6) // +100 Hz
    expect(band).toBe('20m')
    expect(sb).toBe('USB')
  })

  it('Shift+wheel arrives as HORIZONTAL scroll (deltaX, deltaY=0) and still tunes up ×10', () => {
    // WebView2/WebKit convert Shift+wheel to horizontal — direction must come from the dominant axis.
    const el = mountHook({ dialMhz: 14.1, sideband: 'USB', enabled: true, stepHz: 100 })
    wheel(el, { deltaX: -100, deltaY: 0, shiftKey: true })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.101, 6) // +1000 Hz (×10)
  })

  it('wheel down tunes down', () => {
    const el = mountHook({ dialMhz: 14.1, sideband: 'USB', enabled: true, stepHz: 100 })
    wheel(el, { deltaY: 100 })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.0999, 6) // -100 Hz
  })

  it('coalesces several fast notches into a single CAT write', () => {
    const el = mountHook({ dialMhz: 14.1, sideband: 'USB', enabled: true, stepHz: 100 })
    wheel(el, { deltaY: -100 })
    wheel(el, { deltaY: -100 })
    wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1) // one flush, not three
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.1003, 6) // +300 Hz total
  })

  it('stops silently at a band edge (no CAT write, no throw)', () => {
    const el = mountHook({ dialMhz: 14.3495, sideband: 'USB', enabled: true, stepHz: 1000 })
    wheel(el, { deltaY: -100 }) // +1000 Hz → 14.3505 MHz, past the 20 m top
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
  })

  it('does nothing and leaves the page scroll intact when disabled', () => {
    const el = mountHook({ dialMhz: 14.1, sideband: 'USB', enabled: false, stepHz: 100 })
    const e = wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
    expect(e.defaultPrevented).toBe(false) // page scroll not hijacked when CAT is down
  })
})

// ── PER-DIGIT TUNING RIDES THIS HOOK, IT DOES NOT SIT BESIDE IT ────────────────────────────────
// The operator asked to hover any digit and scroll it. The readout that carries those digits is
// already inside an element this hook listens on, so a SECOND listener (or a second hook) means
// two optimistic targets, two coalescers and two CAT writes racing on one dial — measured as
// `[14.075, 14.0741]`, latest-wins, the digit's 1 kHz silently discarded. Resolving the step
// PER EVENT keeps one accumulator, one target and one flush: double-handling becomes
// unrepresentable rather than guarded.
describe('useWheelTune — per-event step resolution', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSetFreq.mockClear()
  })
  afterEach(() => vi.useRealTimers())

  it('a resolver claims the event and sets ITS step for that notch only', () => {
    const el = mountHook({
      dialMhz: 14.1,
      sideband: 'USB',
      enabled: true,
      stepHz: 100, // the selected step — untouched, and NOT what this event uses
      resolveStepHz: () => 1000, // "the 1 kHz digit is under the pointer"
    })
    wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1) // ONE write — not one per mechanism
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.101, 6) // +1 kHz, not +1.1 kHz
  })

  it('a resolver that declines leaves the event ALONE — no tune, no preventDefault', () => {
    // The '.', the MHz unit, the band chip, the wrapper's own padding. On a cockpit with no
    // uniform wheel-tune this must give the page its scroll back, untouched.
    const el = mountHook({
      dialMhz: 14.1,
      sideband: 'USB',
      enabled: true,
      stepHz: 100,
      resolveStepHz: () => null,
    })
    const e = wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
    expect(e.defaultPrevented).toBe(false)
  })

  it('mid-burst the digit under the pointer only changes the INCREMENT on one target', () => {
    let step = 100
    const el = mountHook({
      dialMhz: 14.1,
      sideband: 'USB',
      enabled: true,
      stepHz: 100,
      resolveStepHz: () => step,
    })
    wheel(el, { deltaY: -100 }) // +100 Hz on the 100 Hz digit
    step = 1000
    wheel(el, { deltaY: -100 }) // +1 kHz on the 1 kHz digit — same burst, same target
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.1011, 6)
  })

  it('WITHOUT a resolver every event still uses stepHz — the scope wheel is untouched', () => {
    // The scope/waterfall hooks (CwCockpit, PhoneCockpit) and the strip pass no resolver.
    const el = mountHook({ dialMhz: 14.1, sideband: 'USB', enabled: true, stepHz: 1000 })
    wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.101, 6)
  })
})

describe('useWheelTune — the caps and gates a 10 MHz step makes reachable', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSetFreq.mockClear()
  })
  afterEach(() => vi.useRealTimers())

  it('one flick still applies the full 8 steps at a SELECTOR step — uniform tuning unchanged', () => {
    const el = mountHook({ dialMhz: 14.1, sideband: 'USB', enabled: true, stepHz: 5000 })
    wheel(el, { deltaY: -900 }) // 9 whole steps of scroll; the step cap allows 8
    vi.advanceTimersByTime(120)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.14, 6) // +40 kHz
  })

  it('the same flick on the 10 MHz digit moves ONE decade, not eight', () => {
    // MAX_STEPS_PER_EVENT was written when a step was ≤5 kHz (8 × 5 kHz = 40 kHz). A per-digit
    // 10 MHz step turns the identical flick into 80 MHz of dial. One whole step must still
    // apply — a 10 MHz notch IS 10 MHz, the operator chose that — only the MULTIPLE is bounded.
    const edges: number[] = []
    const el = mountHook({
      dialMhz: 14.1,
      sideband: 'USB',
      enabled: true,
      stepHz: 100,
      resolveStepHz: () => 1e7,
      onEdge: (mhz) => edges.push(mhz),
    })
    wheel(el, { deltaY: -900 })
    vi.advanceTimersByTime(120)
    expect(edges).toEqual([24.1]) // 14.1 + ONE × 10 MHz — not 94.1
  })

  it('a target off the band plan is REPORTED, once per burst — the edge is not silent', () => {
    // The operator accepted VFO carry past a band edge on the condition that the edge is
    // VISIBLE. Silence was right for a 100 Hz creep; a 1 MHz digit lands here on click one.
    const edges: number[] = []
    const el = mountHook({
      dialMhz: 14.34,
      sideband: 'USB',
      enabled: true,
      stepHz: 1e6,
      onEdge: (mhz) => edges.push(mhz),
    })
    wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    wheel(el, { deltaY: -100 }) // still leaning on the edge, same burst
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
    expect(edges).toEqual([15.34]) // said once, not once per 120 ms flush
    // A fresh burst (finger lifted past the idle window) is allowed to say it again.
    vi.advanceTimersByTime(500)
    wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    expect(edges).toEqual([15.34, 15.34])
  })

  it('the flush RE-CHECKS the gate — a burst that ends as the operator keys up is dropped', () => {
    // The gate was read once, at event time, and the flush lands up to 120 ms later. Scroll,
    // then key the mic inside that window, and the CAT set_frequency went out DURING the over.
    const props: Opts = { dialMhz: 14.1, sideband: 'USB', enabled: true, stepHz: 100 }
    const { el, rerender } = mountHookFull(props)
    wheel(el, { deltaY: -100 })
    rerender({ ...props, enabled: false }) // PTT down / tune carrier keyed, 0–120 ms later
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
  })

  it('the returned applier shares the SAME target and coalescer as the wheel', () => {
    // This is how the keyboard (←/→ pick a digit, ↑/↓ spin it) tunes: through the hook, never
    // through a writer of its own — two accumulators on one dial is the bug this avoids.
    const { el, result } = mountHookFull({ dialMhz: 14.1, sideband: 'USB', enabled: true, stepHz: 100 })
    result.current(1000)
    result.current(1000)
    wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.1021, 6) // +1 kHz +1 kHz +100 Hz
  })

  it('the applier obeys the TX/CAT gate too', () => {
    const { result } = mountHookFull({ dialMhz: 14.1, sideband: 'USB', enabled: false, stepHz: 100 })
    result.current(1000)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
  })
})
