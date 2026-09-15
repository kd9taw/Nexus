import { expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { APPLICATION_VERSIONS, applicationCommands, applicationQueryVersion, applicationStreamVersion } from './application-capabilities'
import { collection, queryRequest } from './application-query-protocol'
import type { QueryPage } from './application-query-protocol'
import { RemoteCollections } from './collections'
import { POUNCE_ROWS, parsePounce } from './pounce'

const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })
const last = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0])
const command = 'get_remote_pounce'
const args = () => ({ collection: 'pounce', cursor: null, search: '', unconfirmed: false, after: null })
const request = () => ({ ...args(), type: 'applicationQuery', requestId: crypto.randomUUID() })
const pounce = (call: string, atUnix: number, over: Record<string, unknown> = {}) => ({ call, band: '20m', mode: 'CW', freqMhz: 14.025,
  tags: ['NewEntity'], entity: 'Bouvet Island', atUnix, ...over })
const page = (rows: unknown[], threshold: unknown = 'atno'): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(),
  collection: 'pounce' as QueryPage['collection'], snapshotId: crypto.randomUUID(), offset: 0, total: rows.length, retained: rows.length, nextCursor: null, ageMs: 0,
  rows: rows as QueryPage['rows'], meta: { capturedAgeMs: 0, source: { threshold } as QueryPage['meta'] } })

it('adds exactly the rare-DX alert read to v16 while every older collection stays readable', () => {
  expect(APPLICATION_VERSIONS).toContain(16)
  expect(applicationCommands(16)).toEqual([...applicationCommands(15), command])
  expect(applicationStreamVersion(16)).toBe(13)
  expect(applicationQueryVersion(16)).toBe(16)
  for (let v = 1; v < 16; v++) expect(applicationCommands(v)).not.toContain(command)
  for (const name of ['decodes', 'needs', 'recall', 'awards', 'dxpeditions', 'memories', 'ota', 'fieldDay', 'js8Context', 'sstvImage', 'aprs', 'connect', 'settings', 'parks', 'confirmations']) {
    expect(collection(name, 16), name).toBe(true)
  }
  expect(queryRequest(request(), 16)).toMatchObject(args())
  for (const v of [3, 9, 14, 15]) expect(() => queryRequest(request(), v)).toThrow()
  for (const patch of [{ search: '3Y0J' }, { unconfirmed: true }, { after: 1 }, { cursor: `${crypto.randomUUID()}:1` }, { threshold: 'off' }]) {
    expect(() => queryRequest({ ...request(), ...patch }, 16)).toThrow()
  }
})

it('negotiates every station/browser pair and never sends the alert read to an older station', async () => {
  for (let stationVersion = 1; stationVersion <= 17; stationVersion++) for (let browserVersion = 1; browserVersion <= 17; browserVersion++) {
    const station = peer(), browser = peer(), relay = new ApplicationRelay()
    relay.sync({ peer: station, version: stationVersion }, [{ sessionId: 'one', peer: browser }], 0)
    const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), browserVersion)
    client.open(); client.receive(last(browser))
    expect(last(browser).version).toBe(Math.min(stationVersion, browserVersion))
    const supported = stationVersion >= 16 && browserVersion >= 16
    expect(client.supports(command)).toBe(supported)
    if (!supported) {
      await expect(client.invoke(command, args())).rejects.toThrow()
      expect(station.send).not.toHaveBeenCalled()
    }
    client.disconnected()
  }
})

it('keeps a v16 alert read credit through hibernation and refuses it from a v15 browser', () => {
  const station = peer(), modern = peer(), old = peer(), relay = new ApplicationRelay()
  const peers = [{ sessionId: 'modern', peer: modern }, { sessionId: 'old', peer: old }]
  relay.sync({ peer: station, version: 16 }, peers, 0)
  relay.receiveBrowser('modern', { type: 'applicationHello', version: 16 }, 0)
  relay.receiveBrowser('old', { type: 'applicationHello', version: 15 }, 0)
  const read = request(); relay.receiveBrowser('modern', read, 1)
  const forwarded = last(station), saved = relay.checkpoint('modern')!
  expect(saved.version).toBe(16)
  const resumed = new ApplicationRelay(); resumed.sync({ peer: station, version: 16 }, peers, 2); resumed.restore('modern', saved)
  resumed.receiveStation({ ...page([pounce('3Y0J', 100, { entity: 'POUNCE-FIXTURE' })]), requestId: forwarded.requestId }, 3)
  expect(last(modern).requestId).toBe(read.requestId)
  expect(last(modern).rows[0].entity).toBe('POUNCE-FIXTURE')
  expect(JSON.stringify(resumed.checkpoint('modern'))).not.toContain('POUNCE-FIXTURE')
  relay.receiveBrowser('old', request(), 4)
  expect(old.close).toHaveBeenCalledWith(1008, 'invalidApplicationQuery')
  expect(station.close).not.toHaveBeenCalled()
})

it('validates the bounded list of raised alerts and the station threshold', () => {
  const rows = [pounce('3Y0J', 100), pounce('BS7H', 200, { band: '17m', freqMhz: null, tags: ['NewEntity', 'NewZone'], entity: 'Scarborough Reef' })]
  expect(parsePounce(page(rows))).toEqual({ threshold: 'atno', alerts: rows })
  expect(parsePounce(page([], 'off'))).toEqual({ threshold: 'off', alerts: [] })
  for (const bad of [
    page(Array.from({ length: POUNCE_ROWS + 1 }, (_, i) => pounce(`K${i}X`, i))),
    page([{ ...pounce('3Y0J', 100), notes: 'x' }]),
    page([{ call: '3Y0J', band: '20m', mode: 'CW', freqMhz: 14.025, tags: ['NewEntity'], atUnix: 1 }]),
    page([pounce('', 100)]),
    page([pounce('3Y0J', 1.5)]),
    page([pounce('3Y0J', 100, { freqMhz: -1 })]),
    page([pounce('3Y0J', 100, { tags: 'NewEntity' })]),
    page([pounce('3Y0J', 100, { entity: 'E'.repeat(300) })]),
    page([], 'sometimes'),
    { ...page([]), collection: 'ota' },
    { ...page([pounce('3Y0J', 100)]), total: 2 },
    { ...page([]), meta: { capturedAgeMs: 60_000, source: { threshold: 'atno' } } },
  ]) expect(() => parsePounce(bad as QueryPage)).toThrow()
})

it('pages the alert collection through its own command and nothing else', async () => {
  const invoke = vi.fn(async (): Promise<unknown> => page([pounce('3Y0J', 100)]))
  const client = { invoke, supports: () => true, getPhase: () => 'ready' } as unknown as ApplicationClient
  const collections = new RemoteCollections(client)
  const value = parsePounce(await collections.page({ ...args(), collection: 'pounce' } as Parameters<RemoteCollections['page']>[0]))
  expect(value.alerts.map(a => a.call)).toEqual(['3Y0J'])
  expect(invoke.mock.calls).toEqual([[command, args()]])
  collections.dispose()
})
