import { expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { applicationCommands } from './application-capabilities'
import { queryRequest } from './application-query-protocol'

const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })
const last = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0])
const insights = 'get_remote_insights'
const request = (collection: string) => ({ type: 'applicationQuery', requestId: crypto.randomUUID(), collection,
  cursor: null, search: '', unconfirmed: false, after: null })

it('negotiates full-log insights without broadening any of the five existing versions', async () => {
  for (let stationVersion = 1; stationVersion <= 6; stationVersion++) for (let browserVersion = 1; browserVersion <= 6; browserVersion++) {
    const station = peer(), browser = peer(), relay = new ApplicationRelay()
    relay.sync({ peer: station, version: stationVersion }, [{ sessionId: 'one', peer: browser }], 0)
    const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), browserVersion)
    client.open()
    expect(browser.close).not.toHaveBeenCalled()
    client.receive(last(browser))
    expect(last(browser).version).toBe(Math.min(stationVersion, browserVersion))
    const supported = Math.min(stationVersion, browserVersion) >= 6
    expect(client.supports(insights)).toBe(supported)
    if (!supported) {
      await expect(client.invoke(insights, request('awards'))).rejects.toThrow()
      expect(station.send).not.toHaveBeenCalled()
    }
    client.disconnected()
  }
})

it('admits only the two argument-free insight collections in their explicit query extension', () => {
  expect(applicationCommands(6)).toContain(insights)
  for (const collection of ['awards', 'statistics']) {
    for (const version of [3, 4, 5]) expect(() => queryRequest(request(collection), version)).toThrow()
    expect(queryRequest(request(collection), 6).collection).toBe(collection)
    for (const patch of [{ search: 'W1AW' }, { unconfirmed: true }, { after: 1 }, { cursor: `${crypto.randomUUID()}:1` }]) {
      expect(() => queryRequest({ ...request(collection), ...patch }, 6)).toThrow()
    }
  }
  for (const collection of ['get_log', 'upload', 'journey', 'set_ptt', 'settings']) {
    expect(() => queryRequest(request(collection), 6)).toThrow()
  }
})

it('keeps insights out of the legacy collection command even in a v6 browser', async () => {
  const station = peer(), browser = peer(), relay = new ApplicationRelay()
  relay.sync({ peer: station, version: 6 }, [{ sessionId: 'one', peer: browser }], 0)
  const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), 6)
  client.open(); client.receive(last(browser))
  await expect(client.invoke('get_remote_page', request('awards'))).rejects.toThrow()
  await expect(client.invoke(insights, request('log'))).rejects.toThrow()
  expect(station.send).not.toHaveBeenCalled()
  const { type: _type, requestId: _id, ...args } = request('statistics')
  const pending = client.invoke(insights, args)
  expect(last(station).collection).toBe('statistics')
  client.disconnected()
  await expect(pending).rejects.toThrow('applicationUnavailable')
})

it('restores a v6 query credit without storing the summary or admitting a legacy observer', () => {
  const station = peer(), modern = peer(), old = peer(), relay = new ApplicationRelay()
  const peers = [{ sessionId: 'modern', peer: modern }, { sessionId: 'old', peer: old }]
  relay.sync({ peer: station, version: 6 }, peers, 0)
  relay.receiveBrowser('modern', { type: 'applicationHello', version: 6 }, 0)
  relay.receiveBrowser('old', { type: 'applicationHello', version: 5 }, 0)
  const read = request('awards')
  relay.receiveBrowser('modern', read, 1)
  const forwarded = last(station)
  const saved = relay.checkpoint('modern')!
  expect(saved.version).toBe(6)
  expect(JSON.stringify(saved)).not.toContain('logCount')
  const resumed = new ApplicationRelay()
  resumed.sync({ peer: station, version: 6 }, peers, 2)
  resumed.restore('modern', saved)
  resumed.receiveStation({ type: 'applicationPage', requestId: forwarded.requestId, collection: 'awards', snapshotId: crypto.randomUUID(),
    offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [], meta: { source: { logCount: 2301 } } }, 3)
  expect(last(modern).requestId).toBe(read.requestId)
  expect(last(modern).meta.source.logCount).toBe(2301)
  expect(JSON.stringify(resumed.checkpoint('modern'))).not.toContain('2301')
  relay.receiveBrowser('old', request('awards'), 4)
  expect(old.close).toHaveBeenCalledWith(1008, 'invalidApplicationQuery')
  expect(station.close).not.toHaveBeenCalled()
})
