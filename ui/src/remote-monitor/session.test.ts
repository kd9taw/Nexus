import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import fixtures from './fixtures.v1.json'
import { startMonitor } from './session'
import type { MonitorSource, MonitorState } from './session'

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

it('duplicate cached frames do not renew freshness; a new frame restores it', async () => {
  let frame = fixtures.spe
  let state: MonitorState | undefined
  const source: MonitorSource = { id: 'test', kind: 'fixture', read: async () => frame }
  const stop = startMonitor(source, (next) => { state = next }, () => Date.now())
  await vi.advanceTimersByTimeAsync(0)
  expect(state?.status).toBe('current')
  await vi.advanceTimersByTimeAsync(3000)
  expect(state?.status).toBe('unavailable')
  frame = { ...frame, sequence: 2 }
  await vi.advanceTimersByTimeAsync(500)
  expect(state?.status).toBe('current')
  stop()
})

it('a stalled native read has one in-flight slot and late results cannot revive it', async () => {
  let resolve!: (frame: unknown) => void
  let signal!: AbortSignal
  const publish = vi.fn()
  const read = vi.fn((abort: AbortSignal) => {
    signal = abort
    return new Promise<unknown>((done) => { resolve = done })
  })
  const stop = startMonitor({ id: 'stalled', kind: 'native', read }, publish, () => Date.now())
  await vi.advanceTimersByTimeAsync(30000)
  expect(read).toHaveBeenCalledTimes(1)
  expect(signal.aborted).toBe(true)
  expect(publish.mock.lastCall?.[0].status).toBe('unavailable')
  resolve({ ...fixtures.spe, source: 'native' })
  await vi.advanceTimersByTimeAsync(0)
  expect(publish.mock.lastCall?.[0].status).toBe('unavailable')
  stop()
  await vi.advanceTimersByTimeAsync(1000)
  expect(read).toHaveBeenCalledTimes(1)
})

it('rejects fictional data from a native source and ignores completions after unmount', async () => {
  const publish = vi.fn()
  let resolve!: (frame: unknown) => void
  const stop = startMonitor({ id: 'native', kind: 'native', read: async () => fixtures.spe }, publish)
  await vi.advanceTimersByTimeAsync(0)
  expect(publish.mock.lastCall?.[0]).toEqual({ status: 'invalid', frame: null })
  stop()
  const cancel = startMonitor({ id: 'late', kind: 'fixture', read: () => new Promise((done) => { resolve = done }) }, publish)
  await vi.advanceTimersByTimeAsync(0)
  cancel()
  const count = publish.mock.calls.length
  resolve(fixtures.spe)
  await vi.advanceTimersByTimeAsync(1000)
  expect(publish).toHaveBeenCalledTimes(count)
})
