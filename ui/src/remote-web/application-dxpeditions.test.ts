import { expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { applicationCommands } from './application-capabilities'
import { queryRequest } from './application-query-protocol'

const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })
const last = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0])
const command = 'get_remote_dxpeditions'
const args = { collection: 'dxpeditions', cursor: null, search: '', unconfirmed: false, after: null }
const request = () => ({ ...args, type: 'applicationQuery', requestId: crypto.randomUUID() })

it('negotiates DXpeditions without broadening the six older contracts', async () => {
  for (let stationVersion = 1; stationVersion <= 7; stationVersion++) for (let browserVersion = 1; browserVersion <= 7; browserVersion++) {
    const station = peer(), browser = peer(), relay = new ApplicationRelay()
    relay.sync({ peer: station, version: stationVersion }, [{ sessionId: 'one', peer: browser }], 0)
    const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), browserVersion)
    client.open(); expect(browser.close).not.toHaveBeenCalled(); client.receive(last(browser))
    expect(last(browser).version).toBe(Math.min(stationVersion, browserVersion))
    expect(client.supports(command)).toBe(stationVersion === 7 && browserVersion === 7)
    if (stationVersion < 7 || browserVersion < 7) {
      await expect(client.invoke(command, args)).rejects.toThrow()
      expect(station.send).not.toHaveBeenCalled()
    }
    client.disconnected()
  }
})

it('admits only an argument-free DXpeditions read on its explicit command', async () => {
  expect(applicationCommands(7)).toContain(command)
  for (const version of [3, 4, 5, 6]) expect(() => queryRequest(request(), version)).toThrow()
  expect(queryRequest(request(), 7).collection).toBe('dxpeditions')
  for (const patch of [{ search: 'W1AW' }, { unconfirmed: true }, { after: 1 }, { cursor: `${crypto.randomUUID()}:1` }, { path: 'log.adi' }]) {
    expect(() => queryRequest({ ...request(), ...patch }, 7)).toThrow()
  }
  const station = peer(), browser = peer(), relay = new ApplicationRelay()
  relay.sync({ peer: station, version: 7 }, [{ sessionId: 'one', peer: browser }], 0)
  const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), 7)
  client.open(); client.receive(last(browser))
  for (const old of ['get_remote_page', 'get_remote_recall', 'get_remote_insights']) await expect(client.invoke(old, args)).rejects.toThrow()
  for (const collection of ['log', 'recall', 'awards', 'set_ptt', 'get_dxped_windows']) await expect(client.invoke(command, { ...args, collection })).rejects.toThrow()
  expect(station.send).not.toHaveBeenCalled()
  const pending = client.invoke(command, args)
  expect(last(station).collection).toBe('dxpeditions')
  client.disconnected(); await expect(pending).rejects.toThrow('applicationUnavailable')
})

it('resumes a query after hibernation without persisting DX data or granting it to v6', () => {
  const station = peer(), modern = peer(), old = peer(), relay = new ApplicationRelay()
  const peers = [{ sessionId: 'modern', peer: modern }, { sessionId: 'old', peer: old }]
  relay.sync({ peer: station, version: 7 }, peers, 0)
  relay.receiveBrowser('modern', { type: 'applicationHello', version: 7 }, 0)
  relay.receiveBrowser('old', { type: 'applicationHello', version: 6 }, 0)
  const read = request(); relay.receiveBrowser('modern', read, 1)
  const forwarded = last(station), saved = relay.checkpoint('modern')!
  expect(saved.version).toBe(7)
  const resumed = new ApplicationRelay(); resumed.sync({ peer: station, version: 7 }, peers, 2); resumed.restore('modern', saved)
  resumed.receiveStation({ type: 'applicationPage', requestId: forwarded.requestId, collection: 'dxpeditions', snapshotId: crypto.randomUUID(),
    offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [], meta: { source: { testCall: 'DX-FIXTURE' } } }, 3)
  expect(last(modern).requestId).toBe(read.requestId)
  expect(last(modern).meta.source.testCall).toBe('DX-FIXTURE')
  expect(JSON.stringify(resumed.checkpoint('modern'))).not.toContain('DX-FIXTURE')
  relay.receiveBrowser('old', request(), 4)
  expect(old.close).toHaveBeenCalledWith(1008, 'invalidApplicationQuery')
  expect(station.close).not.toHaveBeenCalled()
})
