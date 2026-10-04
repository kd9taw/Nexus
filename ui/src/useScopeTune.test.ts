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
import { useScopePassband, useScopeTune } from './useScopeTune'
import { PASSBAND_LIMITS } from './spectrum/markers'
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

// THE FILTER EDGE'S WRITER: the dial drag's coalescing, carrying a WIDTH. By value, because "one
// command per flush" is only proven by the number that went out.
describe('useScopePassband', () => {
  type POpts = Parameters<typeof useScopePassband>[0]
  const write = vi.fn((_hz: number) => Promise.resolve(undefined))
  function mountP(props: Partial<POpts> = {}) {
    const base: POpts = { enabled: true, limits: PASSBAND_LIMITS.phone, send: write }
    return renderHook((p: POpts) => useScopePassband(p), { initialProps: { ...base, ...props } })
  }
  beforeEach(() => {
    vi.useFakeTimers()
    write.mockClear()
  })
  afterEach(() => vi.useRealTimers())

  it('a dragged edge sends ONE width per flush, and it is the last one reported', () => {
    const { result } = mountP()
    for (const hz of [2400, 2500, 2600, 2700, 1800]) result.current(hz)
    vi.advanceTimersByTime(119)
    expect(write).not.toHaveBeenCalled() // still inside the flush window
    vi.advanceTimersByTime(1)
    expect(write.mock.calls).toEqual([[1800]])
    // The next window is its own flush: again one write, again the last width.
    result.current(2000)
    result.current(2100)
    vi.advanceTimersByTime(120)
    expect(write.mock.calls).toEqual([[1800], [2100]])
  })

  it("every width is clamped to the cockpit's range and step on the way out", () => {
    const { result } = mountP()
    const sent = (hz: number) => {
      write.mockClear()
      result.current(hz)
      vi.advanceTimersByTime(120)
      return write.mock.calls.map((c) => c[0])
    }
    expect(sent(50)).toEqual([300])
    expect(sent(9000)).toEqual([4000])
    expect(sent(2449)).toEqual([2400])
    expect(sent(Number.NaN)).toEqual([300])
    const cw = mountP({ limits: PASSBAND_LIMITS.cw })
    write.mockClear()
    cw.result.current(512)
    vi.advanceTimersByTime(120)
    expect(write.mock.calls).toEqual([[500]])
  })

  it('NO WRITE WHILE KEYED: refused at the report, and dropped at a flush that finds TX keyed', () => {
    const held = mountP({ enabled: false })
    held.result.current(1800)
    vi.advanceTimersByTime(500)
    expect(write).not.toHaveBeenCalled()

    // The edge is moving when the transmitter keys: the pending width must not go out.
    const { result, rerender } = mountP()
    result.current(1900)
    rerender({ enabled: false, limits: PASSBAND_LIMITS.phone, send: write })
    vi.advanceTimersByTime(500)
    expect(write).not.toHaveBeenCalled()

    // POSITIVE CONTROL: the same report with nothing keyed is written.
    rerender({ enabled: true, limits: PASSBAND_LIMITS.phone, send: write })
    result.current(1900)
    vi.advanceTimersByTime(120)
    expect(write.mock.calls).toEqual([[1900]])
  })

  it('a failed write is reported once, and nothing else happens', async () => {
    const onError = vi.fn()
    const failing = vi.fn(() => Promise.reject(new Error('notController')))
    const { result } = mountP({ send: failing, onError })
    result.current(2000)
    await vi.advanceTimersByTimeAsync(120)
    expect(failing).toHaveBeenCalledTimes(1)
    expect(onError).toHaveBeenCalledTimes(1)
  })
})

describe('useScopeTune — the dial drag still coalesces exactly as before', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSetFreq.mockClear()
  })
  afterEach(() => vi.useRealTimers())

  it('one set_frequency per flush, the last dial, and a click still goes at once', () => {
    const tune = mount({ sideband: 'USB', enabled: true })
    tune.current({ dialHz: 14_200_000, kind: 'drag' })
    tune.current({ dialHz: 14_210_000, kind: 'drag' })
    tune.current({ dialHz: 14_220_000, kind: 'drag' })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq.mock.calls).toEqual([[14.22, '20m', 'USB']])
    tune.current({ dialHz: 14_074_000, kind: 'click' })
    expect(mockSetFreq.mock.calls).toEqual([[14.22, '20m', 'USB'], [14.074, '20m', 'USB']])
  })
})
