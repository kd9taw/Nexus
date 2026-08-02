// @vitest-environment jsdom
//
// LIVE METER POLLING CONTRACT (display-liveliness round 2).
//
// Three pins on the meter fast lane:
// - ONE shared subscription: however many meter widgets are mounted, exactly one 100 ms
//   `get_meters` poll runs (TopBar + cockpit + Settings used to stack 3-4 concurrent
//   10 Hz IPC pollers — one interval, many subscribers, the subscribeSnapshot idiom).
// - Hidden hosts cost nothing: the kept-alive [hidden] host rule — a widget told
//   `active={false}` must not poll at all.
// - Honesty under mid-session failure: a `get_meters` that stops succeeding renders an
//   at-rest meter within a bounded staleness window, never a forever-frozen last reading.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { useEffect } from 'react'

vi.mock('../api', () => ({ getMeters: vi.fn() }))
import { getMeters } from '../api'
import { LiveLevelMeter, useSmeterDb } from './LiveMeters'

const mocked = vi.mocked(getMeters)

/** Advance the fake clock inside act() so interval ticks + promise reactions flush. */
async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms)
  })
}

const meterEl = () => document.querySelector('[role="meter"]')!

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'Date'],
  })
  mocked.mockReset()
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  // Restore jsdom's prototype visibilityState if a test shadowed it on the instance.
  delete (document as { visibilityState?: unknown }).visibilityState
})

describe('the shared meter poll', () => {
  it('runs ONE cadence no matter how many widgets subscribe', async () => {
    mocked.mockResolvedValue({ rxLevel: 0.1, smeterDb: null })
    render(
      <>
        <LiveLevelMeter />
        <LiveLevelMeter variant="full" />
        <LiveLevelMeter />
      </>,
    )
    await advance(1000)
    // One shared 100 ms interval → 10 fetches. Per-widget intervals would be ~30.
    expect(mocked.mock.calls.length).toBe(10)
  })

  it('costs nothing for an inactive (hidden-host) widget', async () => {
    mocked.mockResolvedValue({ rxLevel: 0.1, smeterDb: null })
    render(<LiveLevelMeter active={false} />)
    await advance(1000)
    expect(mocked).not.toHaveBeenCalled()
  })

  it('drops to an at-rest meter within the staleness bound when get_meters stops succeeding', async () => {
    mocked.mockResolvedValueOnce({ rxLevel: 0.5, smeterDb: -10 })
    mocked.mockRejectedValue(new Error('backend gone'))
    render(<LiveLevelMeter />)
    await advance(100)
    // The one real reading landed (0.5 RMS ≈ 84 dB on the WSJT-X scale).
    expect(meterEl().getAttribute('aria-valuenow')).not.toBe('0')
    // Every later poll fails. The needle must FALL TO REST within the bounded staleness
    // window — a frozen last reading is a meter lying about a live measurement.
    await advance(1000)
    expect(meterEl().getAttribute('aria-valuenow')).toBe('0')
  })

  it('never re-renders an smeterDb-only host on RX-level churn (containment)', async () => {
    // The cockpits subscribe to smeterDb ONLY, so the ~1100-line host tree must not
    // re-render at 10 Hz just because the RX level moves. Committed renders are counted
    // via an effect: a bailed-out identical-state render never commits.
    const commits: (number | null)[] = []
    function Probe() {
      const db = useSmeterDb()
      useEffect(() => {
        commits.push(db)
      })
      return null
    }
    mocked
      .mockResolvedValueOnce({ rxLevel: 0.1, smeterDb: -5 })
      .mockResolvedValueOnce({ rxLevel: 0.5, smeterDb: -5 })
      .mockResolvedValueOnce({ rxLevel: 0.9, smeterDb: -5 })
      .mockResolvedValue({ rxLevel: 0.2, smeterDb: 7 })
    render(<Probe />)
    // One poll per act(): only an act() EXIT guarantees React flushes the queued render, so a
    // single advance(400) can legally collapse −5→7 into one commit and vanish the −5 (it did,
    // under full-suite scheduler load). Stepping poll-by-poll makes each commit boundary real.
    await advance(100) // first reading lands: S-meter −5 → one commit
    await advance(100) // RX level churns, S-meter unchanged → must NOT commit
    await advance(100) // RX level churns again → must NOT commit
    await advance(100) // S-meter changes: 7 → one commit
    expect(commits).toEqual([null, -5, 7])
  })

  it('stops polling while the document is hidden and resumes on return', async () => {
    mocked.mockResolvedValue({ rxLevel: 0.1, smeterDb: null })
    render(<LiveLevelMeter />)
    await advance(300)
    const whileVisible = mocked.mock.calls.length
    expect(whileVisible).toBeGreaterThan(0)
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      get: () => 'hidden',
    })
    document.dispatchEvent(new Event('visibilitychange'))
    await advance(500)
    expect(mocked.mock.calls.length).toBe(whileVisible) // a hidden window costs nothing
    delete (document as { visibilityState?: unknown }).visibilityState
    document.dispatchEvent(new Event('visibilitychange'))
    await advance(200)
    expect(mocked.mock.calls.length).toBeGreaterThan(whileVisible)
  })
})
