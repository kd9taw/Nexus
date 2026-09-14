import { expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { APPLICATION_VERSIONS, applicationCommands, applicationQueryVersion, applicationStreamVersion } from './application-capabilities'
import { queryRequest } from './application-query-protocol'
import type { QueryPage } from './application-query-protocol'
import { RemoteCollections } from './collections'
import { parseParks } from './parks'

const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })
const last = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0])
const command = 'get_remote_parks'
const args = (search = 'US-00') => ({ collection: 'parks', cursor: null, search, unconfirmed: false, after: null })
const request = (search?: string) => ({ ...args(search), type: 'applicationQuery', requestId: crypto.randomUUID() })
const park = (reference: string, name = 'Synthetic Park') => ({ reference, name, grid: 'FN31', location: 'US-CT', latitude: null, longitude: null })
const page = (rows: unknown[], source: unknown = { parkCount: rows.length, exact: null }): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(),
  collection: 'parks', snapshotId: crypto.randomUUID(), offset: 0, total: rows.length, retained: rows.length, nextCursor: null, ageMs: 0,
  rows: rows as QueryPage['rows'], meta: { capturedAgeMs: 0, source: source as QueryPage['meta'] } })

it('adds exactly the station lookups to v15 while preserving every older grammar', () => {
  expect(APPLICATION_VERSIONS[APPLICATION_VERSIONS.length - 1]).toBe(15)
  expect(applicationCommands(15)).toEqual([...applicationCommands(14), 'get_remote_parks', 'get_remote_confirmations'])
  expect(applicationStreamVersion(15)).toBe(13)
  expect(applicationQueryVersion(15)).toBe(15)
  for (let v = 1; v < 15; v++) expect(applicationCommands(v)).not.toContain(command)
  expect(queryRequest(request(), 15)).toMatchObject(args())
  for (const v of [3, 9, 13, 14]) expect(() => queryRequest(request(), v)).toThrow()
  for (const patch of [{ search: '' }, { search: 'U' }, { search: ' US-0001' }, { search: 'X'.repeat(33) }, { unconfirmed: true }, { after: 1 },
    { cursor: `${crypto.randomUUID()}:1` }, { path: 'parks.csv' }, { limit: 500 }]) expect(() => queryRequest({ ...request(), ...patch }, 15)).toThrow()
})

it('negotiates all 225 station/browser pairs and never sends a park search to an older station', async () => {
  for (let stationVersion = 1; stationVersion <= 15; stationVersion++) for (let browserVersion = 1; browserVersion <= 15; browserVersion++) {
    const station = peer(), browser = peer(), relay = new ApplicationRelay()
    relay.sync({ peer: station, version: stationVersion }, [{ sessionId: 'one', peer: browser }], 0)
    const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), browserVersion)
    client.open(); client.receive(last(browser))
    expect(last(browser).version).toBe(Math.min(stationVersion, browserVersion))
    const supported = stationVersion === 15 && browserVersion === 15
    expect(client.supports(command)).toBe(supported)
    if (!supported) {
      await expect(client.invoke(command, args())).rejects.toThrow()
      expect(station.send).not.toHaveBeenCalled()
    }
    client.disconnected()
  }
})

it('keeps a v15 park query credit through hibernation without persisting the search or its results', () => {
  const station = peer(), modern = peer(), old = peer(), relay = new ApplicationRelay()
  const peers = [{ sessionId: 'modern', peer: modern }, { sessionId: 'old', peer: old }]
  relay.sync({ peer: station, version: 15 }, peers, 0)
  relay.receiveBrowser('modern', { type: 'applicationHello', version: 15 }, 0)
  relay.receiveBrowser('old', { type: 'applicationHello', version: 14 }, 0)
  const read = request('US-0001'); relay.receiveBrowser('modern', read, 1)
  const forwarded = last(station), saved = relay.checkpoint('modern')!
  expect(saved.version).toBe(15)
  expect(JSON.stringify(saved)).not.toContain('US-0001')
  const resumed = new ApplicationRelay(); resumed.sync({ peer: station, version: 15 }, peers, 2); resumed.restore('modern', saved)
  resumed.receiveStation({ ...page([park('US-0001', 'PARK-FIXTURE')]), requestId: forwarded.requestId }, 3)
  expect(last(modern).requestId).toBe(read.requestId)
  expect(last(modern).rows[0].name).toBe('PARK-FIXTURE')
  expect(JSON.stringify(resumed.checkpoint('modern'))).not.toContain('PARK-FIXTURE')
  relay.receiveBrowser('old', request(), 4)
  expect(old.close).toHaveBeenCalledWith(1008, 'invalidApplicationQuery')
  expect(station.close).not.toHaveBeenCalled()
})

it('validates a bounded park page and its exact match', () => {
  const exact = park('US-0001')
  expect(parseParks(page([park('US-0001'), park('US-0010')], { parkCount: 90000, exact }))).toEqual({
    parks: [exact, park('US-0010')], exact, parkCount: 90000 })
  for (const bad of [
    page(Array.from({ length: 13 }, (_, i) => park(`US-${String(i).padStart(4, '0')}`))),
    page([{ ...park('US-0001'), notes: 'x' }]),
    page([{ ...park(''), reference: '' }]),
    page([{ ...park('US-0001'), latitude: 41.7 }]),
    page([park('US-0001')], { parkCount: 1 }),
    page([park('US-0001')], { parkCount: -1, exact: null }),
    { ...page([]), collection: 'ota' },
    { ...page([]), meta: { capturedAgeMs: 60_000, source: { parkCount: 0, exact: null } } },
  ]) expect(() => parseParks(bad as QueryPage)).toThrow()
})

it('answers the desktop park search and lookup through the station collection, and leaves an older station untouched', async () => {
  const invoke = vi.fn(async (name: string, a?: Record<string, unknown>): Promise<unknown> => {
    if (name !== command) throw new Error('applicationUnsupported')
    return page([park('US-0001'), park('US-0010'), park('US-0011')], { parkCount: 3, exact: a?.search === 'US-0001' ? park('US-0001') : null })
  })
  const client = { invoke, supports: (c: string) => c === command || c === 'get_remote_page', getPhase: () => 'ready' } as unknown as ApplicationClient
  const collections = new RemoteCollections(client)
  expect(await collections.invoke('search_parks', { query: 'us-00', limit: 2 })).toEqual([park('US-0001'), park('US-0010')])
  expect(invoke).toHaveBeenLastCalledWith(command, args('US-00'))
  expect(await collections.invoke('lookup_park', { reference: ' us-0001 ' })).toEqual(park('US-0001'))
  expect(await collections.invoke('lookup_park', { reference: 'US-0099' })).toBeNull()
  // A one-letter query is not a search; nothing leaves the browser.
  const calls = invoke.mock.calls.length
  expect(await collections.invoke('search_parks', { query: 'u', limit: 8 })).toEqual([])
  expect(invoke.mock.calls.length).toBe(calls)
  for (const bad of [{ query: 'US-00', limit: 0 }, { query: 'US-00', limit: 99 }, { query: 'US-00', path: '/tmp' }, { query: 7 }, { reference: 'US-0001', live: true }]) {
    await expect(collections.invoke(bad.reference ? 'lookup_park' : 'search_parks', bad)).rejects.toThrow('applicationUnsupported')
  }
  // Positive control for the old-station path: the same gesture reaches only the ordinary
  // allowlist, which refuses it, so the log form falls back to no suggestions.
  const old = new RemoteCollections({ ...client, supports: (c: string) => c === 'get_remote_page' } as unknown as ApplicationClient)
  await expect(old.invoke('search_parks', { query: 'US-00', limit: 8 })).rejects.toThrow('applicationUnsupported')
  expect(invoke).toHaveBeenLastCalledWith('search_parks', { query: 'US-00', limit: 8 })
  collections.dispose(); old.dispose()
})
