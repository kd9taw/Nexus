// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { startMemoryPublisher } from './memoryPublisher'
import { memoriesStore } from '../features/memories'
import { memoryBank } from './memoryBank'
import type { RemoteStationStatus } from './types'
import fixture from '../remote-web/__fixtures__/memories.json'

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); localStorage.clear() })
it('observes a cold bank without invoking its load/migration writer', async () => {
  vi.useFakeTimers()
  expect(memoriesStore.peek()).toBeNull()
  localStorage.setItem('nexus.memory.bank.v1', JSON.stringify([{ label: 'Legacy channel', freqMhz: 14.06, mode: 'CW' }]))
  const get = vi.spyOn(memoriesStore, 'get'), storage = vi.spyOn(Storage.prototype, 'setItem')
  const status = vi.fn(async () => ({ phase: 'connected', observationGeneration: '1' }) as RemoteStationStatus)
  const publish = vi.fn<(generation: string, bank: string | null) => Promise<boolean>>(async () => false)
  const stop = startMemoryPublisher(status, publish)
  await vi.advanceTimersByTimeAsync(5000)
  expect(publish).toHaveBeenCalledWith('1', null)
  expect(get).not.toHaveBeenCalled(); expect(storage).not.toHaveBeenCalled()
  expect(localStorage.getItem('nexus.memory.bank.v2')).toBeNull()
  stop()
  // The native owner still performs its normal lossless migration on first use.
  expect(memoriesStore.get().memories[0].name).toBe('Legacy channel')
  expect(localStorage.getItem('nexus.memory.bank.v2')).not.toBeNull()
})
it('publishes the actual canonical bank only while observation is enabled, without writing storage', async () => {
  vi.useFakeTimers()
  expect(memoryBank(fixture)).toBe(true)
  if (!memoryBank(fixture)) throw new Error('invalid fixture')
  memoriesStore.set(structuredClone(fixture))
  const stored = vi.spyOn(Storage.prototype, 'setItem'), get = vi.spyOn(memoriesStore, 'get')
  const status = vi.fn(async () => ({ phase: 'disabled', observationGeneration: null }) as RemoteStationStatus)
  const publish = vi.fn<(generation: string, bank: string | null) => Promise<boolean>>(async () => true)
  const stop = startMemoryPublisher(status, publish)
  await vi.advanceTimersByTimeAsync(5000)
  expect(get).not.toHaveBeenCalled(); expect(publish).not.toHaveBeenCalled()
  status.mockResolvedValue({ phase: 'connected', observationGeneration: '2' } as RemoteStationStatus)
  await vi.advanceTimersByTimeAsync(5000)
  expect(publish).toHaveBeenCalledWith('2', JSON.stringify(memoriesStore.get()))
  expect(stored).not.toHaveBeenCalled()
  await vi.advanceTimersByTimeAsync(15000)
  expect(publish).toHaveBeenCalledTimes(1)
  await vi.advanceTimersByTimeAsync(5000)
  expect(publish).toHaveBeenCalledTimes(2)
  const next = structuredClone(fixture); next.memories[0].name = 'Changed at the station'
  memoriesStore.set(next); stored.mockClear()
  await vi.advanceTimersByTimeAsync(5000)
  expect(JSON.parse(publish.mock.lastCall![1]!).memories[0].name).toBe('Changed at the station')
  expect(stored).not.toHaveBeenCalled()
  next.memories[0].notes = 'x'.repeat(1025)
  memoriesStore.set(structuredClone(next))
  await vi.advanceTimersByTimeAsync(5000)
  expect(publish.mock.lastCall).toEqual(['2', null])
  stop(); const count = status.mock.calls.length
  await vi.advanceTimersByTimeAsync(30000); expect(status).toHaveBeenCalledTimes(count)
})
it('does not publish a status reply after disposal or overlap a stalled native read', async () => {
  vi.useFakeTimers()
  let resolve!: (v: RemoteStationStatus) => void
  const status = vi.fn(() => new Promise<RemoteStationStatus>(r => { resolve = r }))
  const publish = vi.fn<(generation: string, bank: string | null) => Promise<boolean>>(async () => true), stop = startMemoryPublisher(status, publish)
  await vi.advanceTimersByTimeAsync(30000)
  expect(status).toHaveBeenCalledTimes(1)
  stop(); resolve({ phase: 'connected', observationGeneration: '3' } as RemoteStationStatus)
  await Promise.resolve(); expect(publish).not.toHaveBeenCalled()
})

it('retries a refused native publication on the next local check', async () => {
  vi.useFakeTimers()
  if (!memoryBank(fixture)) throw new Error('invalid fixture')
  memoriesStore.set(structuredClone(fixture))
  const status = vi.fn(async () => ({ phase: 'connected', observationGeneration: '1' }) as RemoteStationStatus)
  const publish = vi.fn<(generation: string, bank: string | null) => Promise<boolean>>().mockResolvedValueOnce(false).mockResolvedValue(true)
  const stop = startMemoryPublisher(status, publish)
  await vi.advanceTimersByTimeAsync(5000)
  expect(publish).toHaveBeenCalledTimes(2)
  stop()
})
