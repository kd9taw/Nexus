import { expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { applicationCommands } from './application-capabilities'
import { queryRequest } from './application-query-protocol'

const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })
const last = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0])
const command = 'get_remote_field_day'
const args = { collection: 'fieldDay', cursor: null, search: '', unconfirmed: false, after: null }
const request = () => ({ ...args, type: 'applicationQuery', requestId: crypto.randomUUID() })

it('negotiates Field Day without broadening the nine older contracts', async () => {
  for (let stationVersion = 1; stationVersion <= 10; stationVersion++) for (let browserVersion = 1; browserVersion <= 10; browserVersion++) {
    const station = peer(), browser = peer(), relay = new ApplicationRelay()
    relay.sync({ peer: station, version: stationVersion }, [{ sessionId: 'one', peer: browser }], 0)
    const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), browserVersion)
    client.open(); expect(browser.close).not.toHaveBeenCalled(); client.receive(last(browser))
    expect(last(browser).version).toBe(Math.min(stationVersion, browserVersion))
    expect(client.supports(command)).toBe(stationVersion === 10 && browserVersion === 10)
    if (stationVersion < 10 || browserVersion < 10) {
      await expect(client.invoke(command, args)).rejects.toThrow()
      expect(station.send).not.toHaveBeenCalled()
    }
    client.disconnected()
  }
})

it('admits only an argument-free Field Day read on its explicit command', async () => {
  expect(applicationCommands(10)).toContain(command)
  for (const version of [3, 4, 5, 6, 7, 8, 9]) expect(() => queryRequest(request(), version)).toThrow()
  expect(queryRequest(request(), 10).collection).toBe('fieldDay')
  for (const patch of [{ search: 'W1AW' }, { unconfirmed: true }, { after: 1 }, { cursor: `${crypto.randomUUID()}:1` }, { path: 'log.adi' }, { bank: {} }, { generation: '1' }]) {
    expect(() => queryRequest({ ...request(), ...patch }, 10)).toThrow()
  }
  const station = peer(), browser = peer(), relay = new ApplicationRelay()
  relay.sync({ peer: station, version: 10 }, [{ sessionId: 'one', peer: browser }], 0)
  const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), 10)
  client.open(); client.receive(last(browser))
  for (const old of ['get_remote_page', 'get_remote_recall', 'get_remote_insights', 'get_remote_dxpeditions', 'get_remote_memories', 'get_remote_ota', 'publish_remote_memory_bank']) await expect(client.invoke(old, args)).rejects.toThrow()
  for (const collection of ['log', 'recall', 'awards', 'set_ptt', 'get_dxped_windows']) await expect(client.invoke(command, { ...args, collection })).rejects.toThrow()
  expect(station.send).not.toHaveBeenCalled()
  const pending = client.invoke(command, args)
  expect(last(station).collection).toBe('fieldDay')
  client.disconnected(); await expect(pending).rejects.toThrow('applicationUnavailable')
})

it('resumes a query after hibernation without persisting event data or granting it to v9', () => {
  const station = peer(), modern = peer(), old = peer(), relay = new ApplicationRelay()
  const peers = [{ sessionId: 'modern', peer: modern }, { sessionId: 'old', peer: old }]
  relay.sync({ peer: station, version: 10 }, peers, 0)
  relay.receiveBrowser('modern', { type: 'applicationHello', version: 10 }, 0)
  relay.receiveBrowser('old', { type: 'applicationHello', version: 9 }, 0)
  const read = request(); relay.receiveBrowser('modern', read, 1)
  const forwarded = last(station), saved = relay.checkpoint('modern')!
  expect(saved.version).toBe(10)
  const resumed = new ApplicationRelay(); resumed.sync({ peer: station, version: 10 }, peers, 2); resumed.restore('modern', saved)
  resumed.receiveStation({ type: 'applicationPage', requestId: forwarded.requestId, collection: 'fieldDay', snapshotId: crypto.randomUUID(),
    offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [], meta: { source: { testCall: 'FD-FIXTURE' } } }, 3)
  expect(last(modern).requestId).toBe(read.requestId)
  expect(last(modern).meta.source.testCall).toBe('FD-FIXTURE')
  expect(JSON.stringify(resumed.checkpoint('modern'))).not.toContain('FD-FIXTURE')
  relay.receiveBrowser('old', request(), 4)
  expect(old.close).toHaveBeenCalledWith(1008, 'invalidApplicationQuery')
  expect(station.close).not.toHaveBeenCalled()
})
