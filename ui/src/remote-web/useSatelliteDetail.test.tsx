// @vitest-environment jsdom
// The browser's `getSatDetail`: one bird's sealed detail document, fetched once for a gesture.
//
// "Work this pass" indexes the transmitter list BY POSITION — the pick it makes is a choice of
// uplink — so the one thing this must never do is hand back a list belonging to a different bird.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { RemoteCollectionsContext, type RemoteCollections } from './collections'
import { useSatelliteDetail } from './satellite'
import { navigationPages } from './__fixtures__/navigation-page'
import satellite from './__fixtures__/navigation-satellite.json'
import type { QueryPage } from './application-query-protocol'

afterEach(() => { cleanup(); vi.clearAllMocks() })

const BIRD = 'ISS (ZARYA)'

/** A station that seals `answers` as the document for whatever bird it is asked about. */
function source(answers: unknown) {
  const page = vi.fn(async (args: { collection: string; search: string; cursor: string | null }): Promise<QueryPage> => {
    const pages = navigationPages(args.collection, answers, args.search)
    return structuredClone(pages[args.cursor === null ? 0 : Number(args.cursor.split(':')[1])])
  })
  return { source: { page } as unknown as RemoteCollections, page }
}

const wrap = (value: RemoteCollections | null) => ({ children }: { children: ReactNode }) =>
  <RemoteCollectionsContext.Provider value={value}>{children}</RemoteCollectionsContext.Provider>

it('reads the bird it was asked about, chunk by chunk, off the sealed document', async () => {
  const { source: collections, page } = source(satellite)
  const { result } = renderHook(() => useSatelliteDetail(), { wrapper: wrap(collections) })
  const detail = await result.current(BIRD)
  expect(detail.name).toBe(BIRD)
  // The native fixture's bird has no transmitters fetched; what matters is that the list arrived
  // as a list and belongs to this bird, because the pick indexes it by position.
  expect(Array.isArray(detail.transmitters)).toBe(true)
  expect(detail.passTrack).toEqual(satellite.detail.passTrack)
  // Every page carried the bird's own name as the search, so the station sealed the right document.
  for (const call of page.mock.calls) expect(call[0].search).toBe(BIRD)
})

it('refuses a document for another bird, and a station that is not there', async () => {
  // ⛔ The list is indexed by POSITION. Another bird's list would pick a different uplink and say
  // nothing about it, which is the one failure this guard exists for.
  const { source: collections } = source({ ...satellite, name: 'AO-91', detail: { ...satellite.detail, name: 'AO-91' } })
  const { result } = renderHook(() => useSatelliteDetail(), { wrapper: wrap(collections) })
  await expect(result.current(BIRD)).rejects.toThrow()

  const { result: offline } = renderHook(() => useSatelliteDetail(), { wrapper: wrap(null) })
  await expect(offline.current(BIRD)).rejects.toThrow('stationUnavailable')

  // POSITIVE CONTROL: the matching document still resolves, so the two refusals above are about
  // the bird and the station rather than a reader that rejects everything.
  const { source: good } = source(satellite)
  const { result: ok } = renderHook(() => useSatelliteDetail(), { wrapper: wrap(good) })
  await waitFor(async () => expect((await ok.current(BIRD)).name).toBe(BIRD))
})
