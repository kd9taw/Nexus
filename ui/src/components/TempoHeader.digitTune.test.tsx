// @vitest-environment jsdom
//
// TEMPO WAS THE ONE COCKPIT WHOSE DIAL DID NOT SCROLL.
//
// Per-digit wheel tuning ships through `CockpitHeader`, and five of the six cockpits that render
// it pass `digitTune` (Operate, Phone, CW, RTTY, SSTV). `TempoHeader` rendered the same header
// without it, so the TempoFast/TempoDeep readout was the only main dial where hovering a digit
// and scrolling did nothing at all — and nothing said so, because the readout looks identical
// either way (`digitTune` off renders one text node instead of per-digit hit regions).
//
// The mechanics are the header's and are pinned in `CockpitHeader.digitTune.test.tsx` — one
// listener, one coalescer, VFO carry, the TX gate, the band-edge toast. This file pins the two
// things that are TEMPO's: that the header is asked for digit tuning at all, and that the wheel
// sensitivity from Settings reaches it (it did not reach Operate, RTTY or SSTV either — those
// three had the digits but always tuned at ×1 however the slider was set).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { TempoHeader } from './TempoHeader'
import { setFrequency } from '../api'
import type { AppSnapshot } from '../types'

// 20 m only — the same compact band gate the header's own suite uses.
vi.mock('../band', () => ({
  bandLabelForMhz: (mhz: number) => (mhz >= 14 && mhz <= 14.35 ? '20m' : null),
  bandRangeForLabel: (label: string) => (label === '20m' ? { lo: 14, hi: 14.35 } : null),
}))
vi.mock('../api', () => ({
  setFrequency: vi.fn(() => Promise.resolve(null)),
  setRit: vi.fn(() => Promise.resolve(null)),
  setXit: vi.fn(() => Promise.resolve(null)),
  setVfo: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const mockSetFreq = setFrequency as unknown as ReturnType<typeof vi.fn>

const snap = () =>
  ({
    radio: {
      dialMhz: 14.074,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: false,
      tuning: false,
      txAllowed: true,
      txLevel: 0.5,
    },
    chatCq: 'off',
  }) as unknown as AppSnapshot

function mount(props: Record<string, unknown> = {}) {
  return render(
    <TempoHeader
      snap={snap()}
      tier="TempoFast"
      onTierChange={() => {}}
      bandPlan={[]}
      onSetFrequency={() => {}}
      onSetTxLevel={() => {}}
      onToggleCqRun={() => {}}
      onResumeCqRun={() => {}}
      {...props}
    />,
  )
}

/** One notch UP over `el` — a real bubbling, cancelable native event, the only kind the
 *  header's non-passive listener sees. `deltaY: -100` is exactly one step at sensitivity ×1. */
function wheelUp(el: Element): WheelEvent {
  const e = new WheelEvent('wheel', { deltaY: -100, cancelable: true, bubbles: true })
  el.dispatchEvent(e)
  return e
}

describe('the Tempo dial tunes per digit like the other five', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSetFreq.mockClear()
  })
  afterEach(() => {
    vi.useRealTimers()
    cleanup()
  })

  it('renders per-digit hit regions — the readout was a single inert text node before', () => {
    const { container } = mount()
    // 14.0740 → six digits, each carrying its decade in Hz; the '.' is deliberately not one.
    expect(container.querySelectorAll('[data-decade]').length).toBe(6)
    expect(container.querySelector('[data-decade="6"]')).not.toBeNull() // 1 MHz
    expect(container.querySelector('[data-decade="2"]')).not.toBeNull() // 100 Hz, the display floor
  })

  it('one notch on the 1 kHz digit moves 1 kHz — ONE CAT write', () => {
    const { container } = mount()
    wheelUp(container.querySelector('[data-decade="3"]')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.075, 6)
  })

  it('digit-only: the rest of the readout leaves the page its scroll', () => {
    // Tempo works agreed calling frequencies, so it takes `digitTune` WITHOUT uniform
    // `wheelTune` — the same shape as Operate/RTTY/SSTV. Phone and CW have both because they
    // hunt. A non-digit wheel must not tune AND must not be swallowed.
    //
    // The one assertion in this file that is a FORWARD guard rather than a repro: it also holds
    // with digit tuning absent entirely (nothing tunes, so nothing is swallowed). It is here to
    // catch a later `wheelTune` being added to Tempo without that being the intent.
    const { container } = mount()
    const e = wheelUp(container.querySelector('.readout-unit')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
    expect(e.defaultPrevented).toBe(false)
  })

  it('never moves the VFO under a live transmitter', () => {
    // The gate is the header's `txBusyReason` arbiter, not a flag guess. Asserted here because a
    // cockpit that forgets to pass `snap` through would silently open it.
    const busy = snap()
    ;(busy.radio as unknown as Record<string, unknown>).txBusyReason =
      'CW is sending — stop it first'
    const { container } = mount({ snap: busy })
    wheelUp(container.querySelector('[data-decade="3"]')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
  })
})

describe('the Settings wheel sensitivity reaches the readout', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSetFreq.mockClear()
  })
  afterEach(() => {
    vi.useRealTimers()
    cleanup()
  })

  // THE BUG THIS PINS. `wheelTuneSensitivity` was passed to CwCockpit and PhoneCockpit and to
  // nowhere else, so on Operate, RTTY and SSTV — all three of which already had the digits — the
  // slider moved and the dial did not change how far it stepped. The setting's own hint says it
  // "applies to the frequency readout", unqualified. Tempo would have joined them.
  it('×0.5 costs twice the scroll — one notch is no longer one step', () => {
    const { container } = mount({ wheelSensitivity: 0.5 })
    wheelUp(container.querySelector('[data-decade="3"]')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled() // 100 px against a 200 px threshold
    // A second notch completes it, and lands exactly one step away — the residue is not lost.
    wheelUp(container.querySelector('[data-decade="3"]')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.075, 6)
  })

  it('×2 halves it — one notch is two steps', () => {
    const { container } = mount({ wheelSensitivity: 2 })
    wheelUp(container.querySelector('[data-decade="3"]')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.076, 6)
  })

  it('omitted ⇒ ×1, so a cockpit that does not thread it is unchanged, not broken', () => {
    const { container } = mount()
    wheelUp(container.querySelector('[data-decade="3"]')!)
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0][0]).toBeCloseTo(14.075, 6)
  })
})
