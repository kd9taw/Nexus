// @vitest-environment jsdom
//
// #335: the waterfall froze a few minutes into a session. `get_snapshot` takes the engine mutex,
// which is held across blocking CAT I/O and log saves, and every engine-locking command waits for
// it on a tokio worker — one per logical CPU. The snapshot poll fired every 300 ms whatever the
// last call was doing, in the main window AND in the Needed pop-out, so one stall stacked a waiter
// per tick until no worker was left for the lock-free `get_spectrum_row` that feeds the waterfall.
// The poll is single-flight now, with a watchdog so a call that never settles cannot stop it.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { subscribeSnapshot } from './api'
import { POLL_STUCK_MS } from './singleFlight'

/** The Tauri bridge, answering only when a test says so. */
let pending: { cmd: string; resolve: (v: unknown) => void }[] = []
const bridge = vi.fn((cmd: string) => new Promise((resolve) => pending.push({ cmd, resolve })))
const snapshotCalls = () => bridge.mock.calls.filter(([cmd]) => cmd === 'get_snapshot').length
/** Let settled promises run their callbacks without moving the clock. */
const flush = () => vi.advanceTimersByTimeAsync(0)

beforeEach(() => {
  vi.useFakeTimers()
  pending = []
  bridge.mockReset() // back to the answer-when-told implementation, calls cleared
  ;(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = { invoke: bridge }
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('the snapshot poll waits for its last answer (#335)', () => {
  it('asks ONCE in 3 s while a snapshot is not answering, not every 300 ms', async () => {
    const stop = subscribeSnapshot(() => {})
    await vi.advanceTimersByTimeAsync(3000)
    expect(snapshotCalls(), 'a waiter stacked on every tick').toBe(1)
    stop()
  })

  it('resumes as soon as the late answer arrives, and delivers it', async () => {
    const got = vi.fn()
    const stop = subscribeSnapshot(got)
    await vi.advanceTimersByTimeAsync(3000)
    pending[0].resolve({ n: 1 })
    await flush()
    expect(got).toHaveBeenCalledWith({ n: 1 })
    await vi.advanceTimersByTimeAsync(300)
    expect(snapshotCalls()).toBe(2)
    stop()
  })

  it('keeps the usual 300 ms cadence against a backend that answers', async () => {
    // The control: single-flight costs a healthy backend nothing. Answer each call at once.
    bridge.mockImplementation((cmd: string) => Promise.resolve({ cmd }))
    const got = vi.fn()
    const stop = subscribeSnapshot(got)
    await vi.advanceTimersByTimeAsync(3000)
    expect(snapshotCalls()).toBe(10)
    expect(got).toHaveBeenCalledTimes(10)
    stop()
  })

  it('gives up a call that never answers after the watchdog, asks again, and drops the late answer', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const got = vi.fn()
    const stop = subscribeSnapshot(got)
    // Patient up to the limit: a slow snapshot behind a CAT stall is not a lost one.
    await vi.advanceTimersByTimeAsync(300 + POLL_STUCK_MS)
    expect(snapshotCalls(), 'abandoned before the limit').toBe(1)
    // Past it, the next tick gives the call up and polls again.
    await vi.advanceTimersByTimeAsync(300)
    expect(snapshotCalls(), 'the poll never resumed').toBe(2)
    expect(warn).toHaveBeenCalledTimes(1)

    pending[1].resolve({ n: 2 })
    await flush()
    expect(got).toHaveBeenLastCalledWith({ n: 2 })
    // The given-up call finally answers: older than what is on screen, so it is dropped.
    pending[0].resolve({ n: 1 })
    await flush()
    expect(got).not.toHaveBeenCalledWith({ n: 1 })
    stop()
  })

  it('stops for good when unsubscribed, and delivers nothing after', async () => {
    const got = vi.fn()
    const stop = subscribeSnapshot(got)
    await vi.advanceTimersByTimeAsync(300)
    stop()
    pending[0].resolve({ n: 1 })
    await vi.advanceTimersByTimeAsync(3000)
    expect(got).not.toHaveBeenCalled()
    expect(snapshotCalls()).toBe(1)
  })
})
