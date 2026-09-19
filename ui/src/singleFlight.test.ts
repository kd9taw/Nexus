// @vitest-environment jsdom
//
// `pollSingleFlight` — the one helper every engine-locking poller runs through since #335. The
// latch itself is pinned in rowFetchLatch.test.ts (the watchdog, the generation counter) and the
// snapshot poll end to end in snapshotPoll.test.ts; these pin what a POLLER relies on besides.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { POLL_STUCK_MS, pollSingleFlight } from './singleFlight'

beforeEach(() => vi.useFakeTimers())
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

/** A call that settles when told. */
function deferred() {
  let resolve!: () => void
  const promise = new Promise<void>((r) => (resolve = r))
  return { promise, resolve }
}

describe('pollSingleFlight', () => {
  it('polls at once by default, and at the interval while each call answers', async () => {
    const poll = vi.fn(() => Promise.resolve())
    const stop = pollSingleFlight('t', 500, poll)
    expect(poll, 'the leading poll').toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(2000)
    expect(poll).toHaveBeenCalledTimes(5)
    stop()
  })

  it('holds the latch until EVERY call a poll makes has settled', async () => {
    // A poller with several reads (the APRS cockpit has four) returns them all; one still out is
    // enough to keep the next tick from stacking another set.
    const a = deferred()
    const b = deferred()
    const poll = vi.fn(() => Promise.allSettled([a.promise, b.promise]))
    const stop = pollSingleFlight('t', 500, poll)
    a.resolve()
    await vi.advanceTimersByTimeAsync(1500)
    expect(poll, 'polled again with a read still out').toHaveBeenCalledTimes(1)
    b.resolve()
    await vi.advanceTimersByTimeAsync(500)
    expect(poll).toHaveBeenCalledTimes(2)
    stop()
  })

  it('a poll that throws before it returns a promise does not wedge the latch', async () => {
    let n = 0
    const poll = vi.fn(() => {
      if (n++ === 0) throw new Error('boom')
      return Promise.resolve()
    })
    const stop = pollSingleFlight('t', 500, poll)
    await vi.advanceTimersByTimeAsync(1000)
    expect(poll).toHaveBeenCalledTimes(3)
    stop()
  })

  it('owns() is false once stopped, so an unmounted poller writes nothing', async () => {
    const call = deferred()
    let owns: () => boolean = () => true
    const stop = pollSingleFlight('t', 500, (o) => {
      owns = o
      return call.promise
    })
    expect(owns(), 'the control: a live poll owns its answer').toBe(true)
    stop()
    expect(owns()).toBe(false)
    call.resolve()
    await vi.advanceTimersByTimeAsync(2000)
  })

  it('gives up a call that never settles after POLL_STUCK_MS, and only then', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {})
    const poll = vi.fn(() => new Promise<void>(() => {}))
    const stop = pollSingleFlight('t', 1000, poll)
    await vi.advanceTimersByTimeAsync(POLL_STUCK_MS)
    expect(poll, 'abandoned at or before the limit').toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1000)
    expect(poll).toHaveBeenCalledTimes(2)
    stop()
  })
})
