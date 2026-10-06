import { expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { APPLICATION_VERSIONS, applicationCommands, applicationQueryVersion, applicationStreamVersion } from './application-capabilities'
import { collection, queryRequest } from './application-query-protocol'
import type { QueryPage } from './application-query-protocol'
import fixture from './__fixtures__/ota.json'

const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })
const last = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0])
const command = 'get_remote_ota'
const args = () => ({ collection: 'ota', cursor: null, search: '', unconfirmed: false, after: null })
const request = () => ({ ...args(), type: 'applicationQuery', requestId: crypto.randomUUID() })
const ASKED = ['after', 'collection', 'cursor', 'requestId', 'search', 'type', 'unconfirmed']

it('adds no command at v18 and keeps every older collection readable', () => {
  expect(APPLICATION_VERSIONS).toContain(18)
  expect(applicationCommands(18)).toEqual(applicationCommands(17))
  expect(applicationStreamVersion(18)).toBe(13)
  expect(applicationQueryVersion(18)).toBe(18)
  for (const name of ['decodes', 'needs', 'log', 'recall', 'awards', 'dxpeditions', 'memories', 'ota', 'fieldDay', 'js8Context', 'sstvImage', 'aprs', 'connect', 'settings', 'parks', 'confirmations', 'pounce', 'rotator']) {
    expect(collection(name, 18), name).toBe(true)
  }
  expect(queryRequest(request(), 18)).toMatchObject(args())
  // The page never names a version on a query: only the relay tells the station what a session agreed.
  expect(() => queryRequest({ ...request(), queryVersion: 18 }, 18)).toThrow()
})

it('tells the station a session agreed v18 only when the station and the page both did', () => {
  for (let stationVersion = 9; stationVersion <= 18; stationVersion++) for (let browserVersion = 9; browserVersion <= 18; browserVersion++) {
    const station = peer(), browser = peer(), relay = new ApplicationRelay()
    relay.sync({ peer: station, version: stationVersion }, [{ sessionId: 'one', peer: browser }], 0)
    const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), browserVersion)
    client.open(); client.receive(last(browser))
    expect(browser.close).not.toHaveBeenCalled()
    void client.invoke(command, args()).catch(() => undefined)
    const forwarded = last(station), told = stationVersion >= 18 && browserVersion >= 18
    // A station before v18 refuses a query carrying any key it does not know, and a page before v18
    // refuses a spot carrying a key it does not list, so only a pair that both agreed v18 is told.
    expect(Object.keys(forwarded).sort(), `station ${stationVersion}, page ${browserVersion}`).toEqual(told ? [...ASKED.slice(0, 3), 'queryVersion', ...ASKED.slice(3)] : ASKED)
    if (told) expect(forwarded.queryVersion).toBe(18)
    client.disconnected()
  }
})

it('keeps a v18 session through hibernation, delivers the states, and never tells a station that comes back older', () => {
  const station = peer(), modern = peer(), relay = new ApplicationRelay()
  const peers = [{ sessionId: 'modern', peer: modern }]
  relay.sync({ peer: station, version: 18 }, peers, 0)
  relay.receiveBrowser('modern', { type: 'applicationHello', version: 18 }, 0)
  const saved = relay.checkpoint('modern')!
  expect(saved.version).toBe(18)
  const resumed = new ApplicationRelay(); resumed.sync({ peer: station, version: 18 }, peers, 1); resumed.restore('modern', saved)
  const read = request(); resumed.receiveBrowser('modern', read, 2)
  const forwarded = last(station)
  expect(forwarded.queryVersion).toBe(18)
  const source = structuredClone(fixture)
  Object.assign(source.feeds[0].spots[0], { states: ['US-MT', 'US-ND'], neededStates: ['US-ND'] })
  const page: QueryPage = { type: 'applicationPage', requestId: forwarded.requestId, collection: 'ota', snapshotId: crypto.randomUUID(),
    offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [], meta: { capturedAgeMs: 0, source } as QueryPage['meta'] }
  resumed.receiveStation(page, 3)
  expect(last(modern).requestId).toBe(read.requestId)
  expect(last(modern).meta.source.feeds[0].spots[0].neededStates).toEqual(['US-ND'])
  resumed.receiveBrowser('modern', { type: 'applicationQueryAck', requestId: read.requestId }, 4)
  // An older Nexus started at the shack ends the page's session, and the page opens a new one. A
  // query that raced the close is not told either: the relay asks the station it has NOW.
  const older = peer(); resumed.sync({ peer: older, version: 17 }, peers, 5)
  expect(modern.close).toHaveBeenCalledWith(1001, 'applicationUnavailable')
  resumed.receiveBrowser('modern', request(), 6)
  expect(Object.keys(last(older)).sort()).toEqual(ASKED)
  const again = peer(), next = [{ sessionId: 'again', peer: again }]
  resumed.sync({ peer: older, version: 17 }, next, 7)
  resumed.receiveBrowser('again', { type: 'applicationHello', version: 18 }, 7)
  expect(last(again).version).toBe(17)
  resumed.receiveBrowser('again', request(), 8)
  expect(Object.keys(last(older)).sort()).toEqual(ASKED)
  expect(again.close).not.toHaveBeenCalled()
  expect(station.close).not.toHaveBeenCalled()
  expect(older.close).not.toHaveBeenCalled()
})
