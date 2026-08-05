// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import type { RefObject } from 'react'
import { renderHook } from '@testing-library/react'
import { useWheelTune } from './useWheelTune'
import { setFrequency } from './api'

// 40 m and 20 m — two bands with a 6.7 MHz gap, so a burst that walks THROUGH the gap is
// representable. A single-band gate could only ever test the edge, never the cross-band landing.
const BANDS: Record<string, { lo: number; hi: number }> = {
  '40m': { lo: 7, hi: 7.3 },
  '20m': { lo: 14, hi: 14.35 },
}
vi.mock('./band', () => ({
  bandLabelForMhz: (mhz: number) =>
    mhz >= 7 && mhz <= 7.3 ? '40m' : mhz >= 14 && mhz <= 14.35 ? '20m' : null,
  bandRangeForLabel: (label: string) =>
    label === '40m' ? { lo: 7, hi: 7.3 } : label === '20m' ? { lo: 14, hi: 14.35 } : null,
}))
void BANDS
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
    // ⚠️ `performance` MUST be faked, not just the timers. The hook decides where one wheel
    // BURST ends from `performance.now()` (IDLE_RESEED_MS = 400 ms), so a test that advances
    // setTimeout while `performance.now()` runs on the real wall clock measures idleness against
    // whatever the machine happened to take between two lines — and a slow run silently splits
    // one intended burst into two, re-seeding the target and re-arming the once-per-burst edge
    // report. That is exactly what made the edge assertion below flap.
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
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

  it('stops AT a band edge — tunes to it, never past it, never nowhere', () => {
    // ⚠️ CHANGED 2026-08-05, deliberately. This used to assert NO CAT write at all: a notch that
    // overshot the edge threw the whole burst away, so a flick toward the top of the band moved
    // the dial nowhere while the same notches delivered slowly moved it. That was a defect (F3
    // of the per-digit adversarial pass), not a feature — a real VFO stops at the edge, it does
    // not refuse to turn. The guarantee that matters is unchanged and is asserted below: the
    // dial NEVER lands outside the band plan.
    const el = mountHook({ dialMhz: 14.3495, sideband: 'USB', enabled: true, stepHz: 1000 })
    wheel(el, { deltaY: -100 }) // +1000 Hz → 14.3505 MHz, past the 20 m top
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.35, 6) // the edge, not 14.3505
    expect(mockSetFreq.mock.calls[0][1]).toBe('20m')
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
    // ⚠️ `performance` MUST be faked, not just the timers. The hook decides where one wheel
    // BURST ends from `performance.now()` (IDLE_RESEED_MS = 400 ms), so a test that advances
    // setTimeout while `performance.now()` runs on the real wall clock measures idleness against
    // whatever the machine happened to take between two lines — and a slow run silently splits
    // one intended burst into two, re-seeding the target and re-arming the once-per-burst edge
    // report. That is exactly what made the edge assertion below flap.
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
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
    // ⚠️ `performance` MUST be faked, not just the timers. The hook decides where one wheel
    // BURST ends from `performance.now()` (IDLE_RESEED_MS = 400 ms), so a test that advances
    // setTimeout while `performance.now()` runs on the real wall clock measures idleness against
    // whatever the machine happened to take between two lines — and a slow run silently splits
    // one intended burst into two, re-seeding the target and re-arming the once-per-burst edge
    // report. That is exactly what made the edge assertion below flap.
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
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
    // ⚠️ MEASURED FROM AN OFF-PLAN DIAL (10 MHz — WWV, not a ham band), because the band clamp
    // added 2026-08-05 would otherwise stop the dial at 14.35 before the cap could be observed,
    // and the cap is what this test exists for. Off-plan the clamp is a deliberate no-op (an
    // operator listening outside a ham band must still be able to tune), so one 10 MHz step
    // lands at 20 MHz and eight would land at 90.
    const edges: number[] = []
    const el = mountHook({
      dialMhz: 10,
      sideband: 'USB',
      enabled: true,
      stepHz: 100,
      resolveStepHz: () => 1e7,
      onEdge: (mhz) => edges.push(mhz),
    })
    wheel(el, { deltaY: -900 })
    vi.advanceTimersByTime(120)
    expect(edges).toEqual([20]) // 10 + ONE × 10 MHz — not 90
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
    // ⚠️ The REPORTED value is now the edge the dial stopped at (14.35), not the off-plan target
    // it would have reached (15.34) — the dial goes to the edge instead of nowhere. Saying
    // "15.34 MHz is outside the band plan" while the dial sits at 14.35 described a place the
    // radio never went. Once-per-burst, which is what this test is for, is unchanged.
    expect(edges).toEqual([14.35])
    // A fresh burst (finger lifted past the idle window) is allowed to say it again — the dial
    // has been re-seeded from the live snapshot, so this is a new attempt, not a repeat.
    vi.advanceTimersByTime(500)
    wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    expect(edges).toEqual([14.35, 14.35])
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


describe('useWheelTune — the two HIGH findings from the per-digit adversarial pass', () => {
  beforeEach(() => {
    // ⚠️ `performance` MUST be faked, not just the timers. The hook decides where one wheel
    // BURST ends from `performance.now()` (IDLE_RESEED_MS = 400 ms), so a test that advances
    // setTimeout while `performance.now()` runs on the real wall clock measures idleness against
    // whatever the machine happened to take between two lines — and a slow run silently splits
    // one intended burst into two, re-seeding the target and re-arming the once-per-burst edge
    // report. That is exactly what made the edge assertion below flap.
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
    mockSetFreq.mockClear()
  })
  afterEach(() => vi.useRealTimers())

  it('does NOT spend sub-step scroll accrued on one digit at another decade', () => {
    // F1. `accumRef` is ONE pixel accumulator shared by every decade, and nothing reset it when
    // `resolveStepHz` returned a different step than the events that filled it. Measured on the
    // shipped build at 14.0740: 60 px over the 100 Hz digit (below one step, nothing moves),
    // then 60 px over the 100 kHz digit → the accumulator crosses one whole step and commits
    // 14.174 — a 100 kHz QSY from 0.6 of a notch, 60% of which was intent expressed at 100 Hz.
    // On the 10 MHz digit the same gesture yields 24.074.
    let step = 100
    const el = mountHook({
      dialMhz: 14.074,
      sideband: 'USB',
      enabled: true,
      stepHz: 100,
      resolveStepHz: () => step,
    })
    wheel(el, { deltaY: -60 }) // 0.6 of a notch on the 100 Hz digit — nothing should move
    vi.advanceTimersByTime(10)
    expect(mockSetFreq).not.toHaveBeenCalled()
    step = 100_000 // pointer moves to the 100 kHz digit
    wheel(el, { deltaY: -60 }) // 0.6 of a notch THERE — still below one step
    vi.advanceTimersByTime(120)
    expect(
      mockSetFreq,
      'sub-step scroll from the 100 Hz digit must not complete a step at 100 kHz',
    ).not.toHaveBeenCalled()
  })

  it('stops at the band edge instead of walking THROUGH the band plan in one burst', () => {
    // F2. `flush()` band-checked only the FINAL accumulated target, so a fast spin skipped every
    // intermediate refusal. Measured on the shipped build at 7.0740 LSB: 7 notches on the 1 MHz
    // digit inside one 120 ms window → ONE write, setFrequency(14.074, '20m', 'LSB') — a 7 MHz
    // cross-band QSY carrying 40 m's sideband. Delivered slowly the same gesture is refused at
    // every step. Speed alone decided whether it happened.
    //
    // Worse than the frequency: Engine::tune_dial sees a band change and runs
    // halt_tx_for_context_change, clear_decode_context, clear_stations, reset_ft8_a7 — and auto
    // radio routing can hand the QSY to a DIFFERENT RIG. All from one wheel nudge.
    const edges: number[] = []
    const el = mountHook({
      dialMhz: 7.074,
      sideband: 'LSB',
      enabled: true,
      stepHz: 1_000_000,
      onEdge: (mhz) => edges.push(mhz),
    })
    for (let i = 0; i < 7; i++) wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    const landed = mockSetFreq.mock.calls.map((c) => c[0] as number)
    expect(
      landed.every((mhz) => mhz <= 7.3),
      `a burst must not cross out of 40m — landed at ${JSON.stringify(landed)}`,
    ).toBe(true)
    expect(mockSetFreq.mock.calls.every((c) => c[1] === '40m')).toBe(true)
  })

  it('a burst that overshoots the edge still moves the dial TO the edge', () => {
    // F3, same root cause: the whole burst used to be discarded, so a quick flick toward the top
    // of the band moved the dial NOWHERE while the same notches delivered slowly moved it.
    const el = mountHook({
      dialMhz: 14.174,
      sideband: 'USB',
      enabled: true,
      stepHz: 100_000,
      onEdge: () => {},
    })
    for (let i = 0; i < 3; i++) wheel(el, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq, 'a flick toward the band edge must still tune').toHaveBeenCalled()
    const mhz = mockSetFreq.mock.calls[0][0] as number
    expect(mhz).toBeGreaterThan(14.174)
    expect(mhz).toBeLessThanOrEqual(14.35)
  })
})
