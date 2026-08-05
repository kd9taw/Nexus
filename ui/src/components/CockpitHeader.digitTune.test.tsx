// @vitest-environment jsdom
//
// ONE WHEEL EVENT ⇒ ONE CHANGE, on the shared main dial.
//
// THE HAZARD THIS FILE EXISTS FOR. `CockpitHeader` already attaches a NATIVE, non-passive wheel
// listener to `.ch-readout` (useWheelTune), and the per-digit hit regions live INSIDE that
// element. Any handler on a digit therefore sees an event that also bubbles to the hook. Measured
// on a probe before this was built: a digit listener without `stopPropagation` moved the dial
// twice (`setFrequency` calls `[14.075, 14.0741]`) and, because the backend is latest-wins, the
// DIGIT's 1 kHz was the one silently discarded. A React `onWheel` on the digit cannot fix it —
// React 18 delegates to the render root, so the ancestor's native listener has already run, and
// React's wheel listener is passive so it cannot `preventDefault` either. Both halves fail
// quietly, and each component looks correct in isolation.
//
// THE SHAPE THAT MAKES IT UNREPRESENTABLE: there is still exactly ONE listener, ONE accumulator,
// ONE optimistic target and ONE flush. The digit under the pointer only chooses the STEP for that
// event (`resolveStepHz`). Nothing was removed — the step selector still drives the nudge buttons
// and the scope wheel, and wheeling a NON-digit part of the readout still uses it.
//
// All five main dials (Operate, Phone, CW, RTTY, SSTV) render through this one component, so
// every assertion here covers all five.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import { setFrequency } from '../api'
import { pushToast } from '../toast'
import type { AppSnapshot } from '../types'

// 20 m only, 14.000–14.350 MHz — a compact band gate, same stub the hook's own suite uses.
vi.mock('../band', () => ({
  bandLabelForMhz: (mhz: number) => (mhz >= 14 && mhz <= 14.35 ? '20m' : null),
  bandRangeForLabel: (label: string) => (label === '20m' ? { lo: 14, hi: 14.35 } : null),
}))
vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const mockSetFreq = setFrequency as unknown as ReturnType<typeof vi.fn>
const mockToast = pushToast as unknown as ReturnType<typeof vi.fn>

const snapWith = (over: Record<string, unknown> = {}) =>
  ({
    radio: {
      dialMhz: 14.074,
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: false,
      tuning: false,
      txAllowed: true,
      ...over,
    },
  }) as unknown as AppSnapshot

function mount(props: Record<string, unknown> = {}, snap = snapWith()) {
  return render(
    <CockpitHeader
      snap={snap}
      modeIndicator={<span>CW</span>}
      bandControl={<span>20m</span>}
      onCommitDial={() => {}}
      {...props}
    />,
  )
}

/** Wheel one notch UP over `el` — a real bubbling, cancelable native event, because that is the
 *  only kind the hook's non-passive listener sees. */
function wheelUp(el: Element): WheelEvent {
  const e = new WheelEvent('wheel', { deltaY: -100, cancelable: true, bubbles: true })
  el.dispatchEvent(e)
  return e
}

const digit = (root: ParentNode, decade: number) =>
  root.querySelector(`[data-decade="${decade}"]`) as Element

describe('per-digit wheel tuning on the shared main dial', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSetFreq.mockClear()
    mockToast.mockClear()
  })
  afterEach(() => {
    vi.useRealTimers()
    cleanup()
  })

  it('one notch on the 1 kHz digit moves 1 kHz — ONE CAT write, not digit + selected step', () => {
    // The double-handling shape would produce 14.0751 (digit) then 14.0741 (uniform), two writes.
    const { container } = mount({ digitTune: true, wheelTune: true, wheelStepHz: 100 })
    wheelUp(digit(container, 3))
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.075, 6)
  })

  it('carries like a real VFO — 14.1990 + one notch on the kHz digit = 14.2000', () => {
    const { container } = mount(
      { digitTune: true, wheelTune: true, wheelStepHz: 100 },
      snapWith({ dialMhz: 14.199 }),
    )
    wheelUp(digit(container, 3))
    vi.advanceTimersByTime(120)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.2, 6)
  })

  it('every digit is live, MHz included — one notch on the 1 MHz digit moves 1 MHz', () => {
    // The operator chose the powerful option over the safe one: no modifier key, all digits live.
    const { container } = mount({ digitTune: true, wheelTune: true, wheelStepHz: 100 })
    wheelUp(digit(container, 6))
    vi.advanceTimersByTime(120)
    // ⚠️ CHANGED 2026-08-05. 15.074 is off the band plan, and this USED to assert that the whole
    // notch was thrown away (`mockSetFreq` never called) while a toast named 15.0740. That was
    // the F3 defect from the per-digit adversarial pass: a flick toward the edge moved the dial
    // NOWHERE, while the same notch delivered slowly moved it. A real VFO stops AT the edge.
    // The dial now goes to 14.3500 and the toast names where it stopped, not a frequency the
    // radio never visited. The operator's condition — the edge is never silent — is unchanged
    // and still asserted.
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.35, 6)
    expect(mockToast).toHaveBeenCalledTimes(1)
    expect(String(mockToast.mock.calls[0][0])).toContain('14.3500')
    expect(mockToast.mock.calls[0][1]).toBe('error')
  })

  it('names the EDGE, not just the refusal — the condition was that the edge is visible', () => {
    // "You cannot go to 15.0740" leaves the operator guessing where the wall is. Carry walked
    // him off the TOP of 20 m, so say where the top is.
    const { container } = mount({ digitTune: true, wheelTune: true, wheelStepHz: 100 })
    wheelUp(digit(container, 6))
    vi.advanceTimersByTime(120)
    const msg = String(mockToast.mock.calls[0][0])
    expect(msg).toContain('20m')
    expect(msg).toContain('14.3500') // the edge he hit — and, since the clamp, where he now IS
  })

  it('names the LOWER edge when carry walks off the bottom', () => {
    const { container } = mount(
      { digitTune: true, wheelTune: true, wheelStepHz: 100 },
      snapWith({ dialMhz: 14.05 }),
    )
    const e = new WheelEvent('wheel', { deltaY: 100, cancelable: true, bubbles: true }) // DOWN
    digit(container, 5)!.dispatchEvent(e) // −100 kHz → 13.950
    vi.advanceTimersByTime(120)
    expect(String(mockToast.mock.calls[0][0])).toContain('14.0000')
  })

  it('the NON-digit parts of the readout still tune by the SELECTED step — nothing removed', () => {
    // The '.', the MHz unit and the band chip. This is the shipped "scroll anywhere on the
    // readout" behavior that Phone and CW have today; per-digit is additive to it.
    const { container } = mount({ digitTune: true, wheelTune: true, wheelStepHz: 100 })
    wheelUp(container.querySelector('.readout-unit')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.0741, 6) // the 100 Hz selector step
  })

  it('the selected step still drives the readout wheel when it changes — the selector is untouched', () => {
    const { container } = mount({ digitTune: true, wheelTune: true, wheelStepHz: 5000 })
    wheelUp(container.querySelector('.readout-unit')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.079, 6)
  })

  it('WITHOUT uniform wheel-tune (Operate/RTTY/SSTV) only the digits claim the wheel', () => {
    const { container } = mount({ digitTune: true, wheelTune: false, wheelStepHz: 100 })
    // A digit tunes…
    wheelUp(digit(container, 4))
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.084, 6) // +10 kHz
    // …and everything else gives the page its scroll back, untouched.
    mockSetFreq.mockClear()
    const e = wheelUp(container.querySelector('.readout-unit')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
    expect(e.defaultPrevented).toBe(false)
  })

  it('WITHOUT digitTune the readout has no digit regions at all (the excluded surfaces)', () => {
    const { container } = mount({ wheelTune: true, wheelStepHz: 100 })
    expect(container.querySelectorAll('[data-decade]')).toHaveLength(0)
    wheelUp(container.querySelector('.readout-val')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.0741, 6) // unchanged uniform behavior
  })
})

describe('the wheel may never move the VFO under a live transmitter', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSetFreq.mockClear()
  })
  afterEach(() => {
    vi.useRealTimers()
    cleanup()
  })

  // A 100 Hz creep during an over was nearly invisible; a 1 MHz digit notch is an off-frequency
  // emission on someone else's allocation, and on the engine side a band change during an over
  // halts it. The gate is the same one the scope wheel uses — never weakened, and widened to the
  // keyed tune carrier, which `transmitting` alone does not cover.
  for (const [what, over] of [
    ['CAT is down', { catOk: false }],
    ['a slot TX is on the air', { transmitting: true }],
    ['a tune carrier is keyed', { tuning: true }],
    // ⭐ THE FIVE THE OLD GATE WAS BLIND TO. `!transmitting && !tuning` reimplemented
    // `Engine::tx_owner()` and knew 2 of its 7 owners: `transmitting` is written ONLY by the
    // FT slot-TX path, so a held mic key, the voice keyer, CW, RTTY and SSTV all read as
    // "transmitter free". The soundcard modes are covered by accident downstream via
    // `tx_until_ms`; MANUAL PTT IS DELIBERATELY NOT ("a section/mode change must always reach
    // the rig"), so nothing backstopped this and one notch on the 1 MHz digit moved the VFO —
    // and could carry across a band edge, which halts the over and re-commands mode — under a
    // keyed transmitter.
    ['the mic PTT is held', { txBusyReason: 'Mic PTT is held — release it first' }],
    ['the voice keyer is playing', { txBusyReason: 'A voice message is transmitting — stop it first' }],
    ['CW is sending', { txBusyReason: 'CW is sending — stop it first' }],
    ['RTTY is transmitting', { txBusyReason: 'RTTY is transmitting — stop it first' }],
    ['SSTV is transmitting', { txBusyReason: 'An SSTV image is transmitting — stop it first' }],
  ] as const) {
    it(`refuses a digit notch while ${what}`, () => {
      const { container } = mount({ digitTune: true, wheelTune: true, wheelStepHz: 100 }, snapWith(over))
      const d = digit(container, 3)
      // CAT-down also drops the editor, but the digits must still render — the readout is not
      // allowed to change shape under the operator just because the rig went quiet.
      expect(d).not.toBeNull()
      wheelUp(d)
      vi.advanceTimersByTime(120)
      expect(mockSetFreq).not.toHaveBeenCalled()
    })
  }
})

describe('the keyboard equivalent reaches the same coalescer', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSetFreq.mockClear()
  })
  afterEach(() => {
    vi.useRealTimers()
    cleanup()
  })

  it('↑ on the readout spins the selected digit through the WHEEL’s target', () => {
    const { container } = mount({ digitTune: true, wheelTune: true, wheelStepHz: 100 })
    const readout = container.querySelector('[role="button"]')!
    // The first ← only selects the default (100 Hz); two more walk up to the 10 kHz digit.
    for (let i = 0; i < 3; i++) fireEvent.keyDown(readout, { key: 'ArrowLeft' })
    fireEvent.keyDown(readout, { key: 'ArrowUp' })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.084, 6) // +10 kHz
  })
})
