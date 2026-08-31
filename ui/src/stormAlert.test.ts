// The geomagnetic storm heads-up. A storm is hours-to-days of degraded HF, and until
// this existed it reached an operator only if they were looking at the Space Weather
// pane — unlike a flare or a band opening, which both alert app-wide.
import { describe, it, expect, beforeEach, vi } from 'vitest'

const toast = vi.hoisted(() => ({ push: vi.fn() }))
const beep = vi.hoisted(() => ({ double: vi.fn() }))
vi.mock('./toast', () => ({ pushToast: toast.push }))
vi.mock('./alerts', () => ({ doubleBeep: beep.double }))

import { processStorm, processStormForecast, resetStormAlerts, gScale } from './stormAlert'
import type { KpForecast } from './types'

beforeEach(() => {
  toast.push.mockClear()
  beep.double.mockClear()
  resetStormAlerts()
})

describe('the G scale', () => {
  it('starts at Kp 5 and tops out at G5', () => {
    expect(gScale(4.99)).toBe(0)
    expect(gScale(5)).toBe(1)
    expect(gScale(6.7)).toBe(2)
    expect(gScale(9)).toBe(5)
    expect(gScale(12)).toBe(5)
  })
  it('is 0 for junk rather than a storm', () => {
    expect(gScale(NaN)).toBe(0)
  })
})

describe('the measured storm alert fires on the edge, not every poll', () => {
  it('announces once when a storm starts', () => {
    processStorm(5.3)
    expect(toast.push).toHaveBeenCalledTimes(1)
    // Polling again during the same storm must stay quiet.
    processStorm(5.4)
    processStorm(5.1)
    expect(toast.push).toHaveBeenCalledTimes(1)
  })

  it('escalates when the storm deepens', () => {
    processStorm(5.2)
    processStorm(6.5) // G1 → G2
    expect(toast.push).toHaveBeenCalledTimes(2)
    expect(beep.double, 'G2 is prominent and should beep').toHaveBeenCalledTimes(1)
  })

  it('is quiet at G1 and prominent from G2', () => {
    processStorm(5.1)
    expect(toast.push.mock.calls[0][1]).toBe('info')
    expect(beep.double).not.toHaveBeenCalled()
    resetStormAlerts()
    toast.push.mockClear()
    processStorm(7.2)
    expect(toast.push.mock.calls[0][1]).toBe('error')
    expect(toast.push.mock.calls[0][3]).toEqual({ prominent: true })
  })

  /// ⚠️ THE HYSTERESIS IS THE WHOLE POINT. Kp hovering either side of 5 must not
  /// re-announce the same storm over and over; it re-arms only after a real return
  /// to quiet.
  it('does not re-fire while Kp wobbles across the boundary', () => {
    processStorm(5.1)
    processStorm(4.7) // dipped below storm level, but not back to quiet
    processStorm(5.2)
    expect(toast.push, 'a wobbling Kp re-announced the same storm').toHaveBeenCalledTimes(1)
  })

  /// Re-arming clears the EDGE, but the per-tier cooldown still stands, so a genuine
  /// second storm has to be separated in time as well. That is not a limitation worth
  /// removing: Kp is a 3-hour index, so two of these readings are hours apart in
  /// reality and the cooldown can only ever suppress flapping. Tested with the clock
  /// moved, because testing it back-to-back would be testing a sequence that cannot
  /// happen.
  it('re-arms once it really settles, so the NEXT storm is announced', () => {
    vi.useFakeTimers()
    try {
      vi.setSystemTime(new Date('2026-08-31T00:00:00Z'))
      processStorm(5.1)
      processStorm(3.0) // quiet — event over
      vi.setSystemTime(new Date('2026-08-31T09:00:00Z')) // three Kp periods later
      processStorm(5.4) // a new storm
      expect(toast.push).toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  /// The other half of that rule: within the cooldown the alert is DELAYED, never
  /// dropped. The edge stays armed, so once the hour lapses it still announces.
  it('delays rather than drops an escalation caught by the cooldown', () => {
    vi.useFakeTimers()
    try {
      vi.setSystemTime(new Date('2026-08-31T00:00:00Z'))
      processStorm(5.1)
      processStorm(3.0)
      processStorm(5.4) // inside the cooldown — suppressed
      expect(toast.push).toHaveBeenCalledTimes(1)
      vi.setSystemTime(new Date('2026-08-31T02:00:00Z'))
      processStorm(5.4) // cooldown lapsed, still armed
      expect(toast.push, 'a suppressed storm was dropped instead of delayed').toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('says nothing on a quiet sky, and nothing about missing data', () => {
    processStorm(2.3)
    processStorm(null)
    processStorm(undefined)
    expect(toast.push).not.toHaveBeenCalled()
  })
})

describe('the forecast heads-up', () => {
  const fc = (rows: Array<[number, number, string]>): KpForecast => ({
    points: rows.map(([timeUnix, kp, kind]) => ({
      timeUnix,
      kp,
      kind: kind as 'observed' | 'estimated' | 'predicted',
      noaaScale: null,
    })),
  })

  it('announces a predicted storm once, however often it is polled', () => {
    const f = fc([
      [1_000, 2.0, 'observed'],
      [2_000, 6.0, 'predicted'],
    ])
    processStormForecast(f, 2.0)
    processStormForecast(f, 2.0)
    processStormForecast(f, 2.0)
    expect(toast.push).toHaveBeenCalledTimes(1)
  })

  it('announces again when a revision names a different onset — the forecast moving is news', () => {
    processStormForecast(fc([[2_000, 6.0, 'predicted']]), 2.0)
    processStormForecast(fc([[9_000, 6.0, 'predicted']]), 2.0)
    expect(toast.push).toHaveBeenCalledTimes(2)
  })

  /// Saying "expected" about weather already on top of you is noise — the measured
  /// alert owns that case.
  it('stays silent while a storm is already running', () => {
    processStormForecast(fc([[2_000, 6.0, 'predicted']]), 6.1)
    expect(toast.push).not.toHaveBeenCalled()
  })

  it('ignores an observed row — that is history, not a forecast', () => {
    processStormForecast(fc([[1_000, 7.0, 'observed']]), 2.0)
    expect(toast.push).not.toHaveBeenCalled()
  })

  it('says nothing when the outlook is quiet or missing', () => {
    processStormForecast(fc([[2_000, 3.0, 'predicted']]), 2.0)
    processStormForecast({ points: [] }, 2.0)
    processStormForecast(null, 2.0)
    expect(toast.push).not.toHaveBeenCalled()
  })
})
