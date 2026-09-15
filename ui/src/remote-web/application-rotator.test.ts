import { expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { APPLICATION_VERSIONS, applicationCommands, applicationQueryVersion, applicationStreamVersion } from './application-capabilities'
import { collection, queryRequest } from './application-query-protocol'
import type { QueryPage } from './application-query-protocol'
import { RemoteCollections } from './collections'
import { parseRotator } from './rotator'

const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })
const last = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0])
const command = 'get_remote_rotator'
const args = () => ({ collection: 'rotator', cursor: null, search: '', unconfirmed: false, after: null })
const request = () => ({ ...args(), type: 'applicationQuery', requestId: crypto.randomUUID() })
const page = (source: unknown, over: Partial<QueryPage> = {}): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(),
  collection: 'rotator' as QueryPage['collection'], snapshotId: crypto.randomUUID(), offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0,
  rows: [], meta: { capturedAgeMs: 0, source } as QueryPage['meta'], ...over })

it('adds exactly the rotator heading read to v17 while every older collection stays readable', () => {
  expect(APPLICATION_VERSIONS[APPLICATION_VERSIONS.length - 1]).toBe(17)
  expect(applicationCommands(17)).toEqual([...applicationCommands(16), command])
  expect(applicationStreamVersion(17)).toBe(13)
  expect(applicationQueryVersion(17)).toBe(17)
  for (let v = 1; v < 17; v++) expect(applicationCommands(v)).not.toContain(command)
  for (const name of ['decodes', 'needs', 'recall', 'awards', 'dxpeditions', 'memories', 'ota', 'fieldDay', 'js8Context', 'sstvImage', 'aprs', 'connect', 'settings', 'parks', 'confirmations', 'pounce']) {
    expect(collection(name, 17), name).toBe(true)
  }
  expect(queryRequest(request(), 17)).toMatchObject(args())
  for (const v of [3, 9, 14, 15, 16]) expect(() => queryRequest(request(), v)).toThrow()
  // Argument-free: the read names no rotator, no address and no bearing of its own.
  for (const patch of [{ search: '180' }, { unconfirmed: true }, { after: 1 }, { cursor: `${crypto.randomUUID()}:1` }, { azimuthDeg: 0 }]) {
    expect(() => queryRequest({ ...request(), ...patch }, 17)).toThrow()
  }
})

it('negotiates every station/browser pair and never sends the heading read to an older station', async () => {
  for (let stationVersion = 1; stationVersion <= 17; stationVersion++) for (let browserVersion = 1; browserVersion <= 17; browserVersion++) {
    const station = peer(), browser = peer(), relay = new ApplicationRelay()
    relay.sync({ peer: station, version: stationVersion }, [{ sessionId: 'one', peer: browser }], 0)
    const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), browserVersion)
    client.open(); client.receive(last(browser))
    expect(last(browser).version).toBe(Math.min(stationVersion, browserVersion))
    const supported = stationVersion === 17 && browserVersion === 17
    expect(client.supports(command)).toBe(supported)
    if (!supported) {
      await expect(client.invoke(command, args())).rejects.toThrow()
      expect(station.send).not.toHaveBeenCalled()
    }
    client.disconnected()
  }
})

it('keeps a v17 heading credit through hibernation and refuses it from a v16 browser', () => {
  const station = peer(), modern = peer(), old = peer(), relay = new ApplicationRelay()
  const peers = [{ sessionId: 'modern', peer: modern }, { sessionId: 'old', peer: old }]
  relay.sync({ peer: station, version: 17 }, peers, 0)
  relay.receiveBrowser('modern', { type: 'applicationHello', version: 17 }, 0)
  relay.receiveBrowser('old', { type: 'applicationHello', version: 16 }, 0)
  const read = request(); relay.receiveBrowser('modern', read, 1)
  const forwarded = last(station), saved = relay.checkpoint('modern')!
  expect(saved.version).toBe(17)
  const resumed = new ApplicationRelay(); resumed.sync({ peer: station, version: 17 }, peers, 2); resumed.restore('modern', saved)
  resumed.receiveStation({ ...page({ configured: true, azimuthDeg: 212.5 }), requestId: forwarded.requestId }, 3)
  expect(last(modern).requestId).toBe(read.requestId)
  expect(last(modern).meta.source.azimuthDeg).toBe(212.5)
  relay.receiveBrowser('old', request(), 4)
  expect(old.close).toHaveBeenCalledWith(1008, 'invalidApplicationQuery')
  expect(station.close).not.toHaveBeenCalled()
})

it('reads a bearing, and reads UNKNOWN as unknown rather than as zero', () => {
  expect(parseRotator(page({ configured: true, azimuthDeg: 212.5 }))).toEqual({ configured: true, azimuthDeg: 212.5 })
  expect(parseRotator(page({ configured: true, azimuthDeg: 0 }))).toEqual({ configured: true, azimuthDeg: 0 })
  // The two honest failures the station distinguishes: no rotator, and one that did not answer.
  expect(parseRotator(page({ configured: false, azimuthDeg: null }))).toEqual({ configured: false, azimuthDeg: null })
  expect(parseRotator(page({ configured: true, azimuthDeg: null }))).toEqual({ configured: true, azimuthDeg: null })
  for (const bad of [
    page({ configured: true, azimuthDeg: 360 }),
    page({ configured: true, azimuthDeg: -1 }),
    page({ configured: true, azimuthDeg: Number.NaN }),
    page({ configured: true, azimuthDeg: '212.5' }),
    // A bearing from a station that says it has no rotator is a contradiction, not a reading.
    page({ configured: false, azimuthDeg: 90 }),
    page({ configured: true }),
    page({ configured: true, azimuthDeg: null, address: '127.0.0.1:4533' }),
    page({ azimuthDeg: null }),
    { ...page({ configured: true, azimuthDeg: 1 }), collection: 'pounce' as QueryPage['collection'] },
    page({ configured: true, azimuthDeg: 1 }, { rows: [{ azimuthDeg: 1 }] as QueryPage['rows'], total: 1, retained: 1 }),
    { ...page({ configured: true, azimuthDeg: 1 }), meta: { source: { configured: true, azimuthDeg: 1 } } as QueryPage['meta'] },
  ]) expect(() => parseRotator(bad as QueryPage)).toThrow()
})

it('pages the heading through its own command and nothing else', async () => {
  const invoke = vi.fn(async (): Promise<unknown> => page({ configured: true, azimuthDeg: 45 }))
  const client = { invoke, supports: () => true, getPhase: () => 'ready' } as unknown as ApplicationClient
  const collections = new RemoteCollections(client)
  const value = parseRotator(await collections.page({ ...args(), collection: 'rotator' } as Parameters<RemoteCollections['page']>[0]))
  expect(value).toEqual({ configured: true, azimuthDeg: 45 })
  expect(invoke.mock.calls).toEqual([[command, args()]])
  collections.dispose()
})
