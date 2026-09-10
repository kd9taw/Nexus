// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'
import type { ReactNode } from 'react'
import { RemoteCollectionsContext, type RemoteCollections } from './collections'
import { useSatelliteSchedule } from './useNavigation'
import satellite from './__fixtures__/navigation-satellite.json'
import { navigationPages } from './__fixtures__/navigation-page'
import type { QueryArgs } from './application-query-protocol'

afterEach(() => { cleanup(); vi.useRealTimers() })

function fixture() {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  let failure: string | null = null
  const page = vi.fn(async (args: QueryArgs) => {
    if (failure) throw new Error(failure)
    const value = { ...satellite, name: args.search, detail: { ...satellite.detail, name: args.search },
      schedule: satellite.schedule.map(p => ({ ...p, name: args.search })) }
    const pages = navigationPages('satellite', value, args.search)
    await new Promise(resolve => setTimeout(resolve, 2000))
    return { ...pages[0], rows: pages.flatMap(p => p.rows), nextCursor: null }
  })
  const source = { page } as unknown as RemoteCollections
  const hook = renderHook(() => useSatelliteSchedule('ISS (ZARYA),AO-91,SO-50'), {
    wrapper: ({ children }: { children: ReactNode }) =>
      <RemoteCollectionsContext.Provider value={source}>{children}</RemoteCollectionsContext.Provider>,
  })
  return { ...hook, page, fail: (value: string | null) => { failure = value } }
}
async function advance(ms: number) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

it('refreshes a slow multi-bird schedule before expiry without collapsing the displayed rows', async () => {
  const { result, page } = fixture()
  await advance(6000)
  const count = satellite.schedule.length * 3
  expect(count).toBeGreaterThan(0)
  expect(result.current.value?.rows).toHaveLength(count)
  for (let elapsed = 0; elapsed < 70_000; elapsed += 500) {
    await advance(500)
    expect(result.current.value?.rows, `schedule disappeared at ${performance.now()} ms`).toHaveLength(count)
  }
  expect(page.mock.calls.length).toBeGreaterThan(9)
})

it('keeps a still-valid schedule during temporary congestion, then hides it at its original deadline', async () => {
  const { result, fail } = fixture()
  await advance(6000)
  const initial = result.current.value
  expect(initial).not.toBeNull()
  fail('applicationBusy')
  for (let elapsed = 0; elapsed < 23_000; elapsed += 500) {
    await advance(500)
    expect(result.current.value).toBe(initial)
  }
  await advance(1500)
  expect(result.current.value).toBeNull()
  fail(null)
  await advance(8500)
  expect(result.current.value?.rows.length).toBe(satellite.schedule.length * 3)
})

it('clears the schedule when a refresh reports unavailable data', async () => {
  const { result, fail } = fixture()
  await advance(6000)
  expect(result.current.value).not.toBeNull()
  fail('applicationUnavailable')
  await advance(23_000)
  expect(result.current.value).toBeNull()
})
