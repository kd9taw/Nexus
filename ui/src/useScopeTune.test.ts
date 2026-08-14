// @vitest-environment jsdom
//
// SCOPE / BAND-MAP CLICK-AND-DRAG TUNING, and the one thing it used to swallow.
//
// `send()` derived a band label from the TARGET and returned when the table did not name one, so
// a click on the part of a band map that lies outside the amateur allocations did nothing at all
// — no tune, no message, no cursor move. Listening off the ham bands is first-class (operator,
// 2026-08-13), so the empty label now goes on the wire with the frequency.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useScopeTune } from './useScopeTune'
import { setFrequency } from './api'

// 20 m only. Off-plan returns '' — what the real `bandLabelForMhz` returns.
vi.mock('./band', () => ({
  bandLabelForMhz: (mhz: number) => (mhz >= 14 && mhz <= 14.35 ? '20m' : ''),
}))
vi.mock('./api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)) }))

const mockSetFreq = setFrequency as unknown as ReturnType<typeof vi.fn>

type Opts = Parameters<typeof useScopeTune>[0]

function mount(props: Opts) {
  return renderHook((p: Opts) => useScopeTune(p), { initialProps: props }).result
}

describe('useScopeTune', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSetFreq.mockClear()
  })
  afterEach(() => vi.useRealTimers())

  it('a click OFF the band plan tunes there instead of vanishing', () => {
    const tune = mount({ sideband: 'USB', enabled: true })
    tune.current({ dialHz: 9_600_000, kind: 'click' }) // a shortwave broadcaster
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0]).toEqual([9.6, '', 'USB'])
  })

  it('POSITIVE CONTROL — an in-band click is unchanged and still names its band', () => {
    const tune = mount({ sideband: 'USB', enabled: true })
    tune.current({ dialHz: 14_074_000, kind: 'click' })
    expect(mockSetFreq.mock.calls[0]).toEqual([14.074, '20m', 'USB'])
  })

  it('a drag that ends off the band plan still flushes — coalescing is unchanged', () => {
    const tune = mount({ sideband: 'USB', enabled: true })
    tune.current({ dialHz: 14_100_000, kind: 'drag' })
    tune.current({ dialHz: 13_900_000, kind: 'drag' }) // dragged off the bottom of 20 m
    expect(mockSetFreq).not.toHaveBeenCalled() // still throttled
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1) // latest target wins, one write
    expect(mockSetFreq.mock.calls[0]).toEqual([13.9, '', 'USB'])
  })

  it('POSITIVE CONTROL — a frequency that is not a frequency is still refused', () => {
    // The band lookup was doing double duty as the sanity check (it answers '' for NaN too).
    // Removing it must not let a degenerate scope span put NaN or a negative dial on the wire.
    const tune = mount({ sideband: 'USB', enabled: true })
    tune.current({ dialHz: Number.NaN, kind: 'click' })
    tune.current({ dialHz: -14_074_000, kind: 'click' })
    tune.current({ dialHz: 0, kind: 'click' })
    expect(mockSetFreq).not.toHaveBeenCalled()
  })

  it('POSITIVE CONTROL — the TX/CAT gate still refuses an off-band click', () => {
    const tune = mount({ sideband: 'USB', enabled: false })
    tune.current({ dialHz: 9_600_000, kind: 'click' })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
  })
})
