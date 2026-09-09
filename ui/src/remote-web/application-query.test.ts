import { describe, expect, it } from 'vitest'
import { queryRequest, queryPage } from './application-query-protocol'
import { ApplicationRelay } from './application-relay'
import { vi } from 'vitest'

const id = '10000000-0000-4000-8000-000000000001'
const request = { type: 'applicationQuery', requestId: id, collection: 'log', cursor: null, search: '', unconfirmed: false, after: null }
const page = { type: 'applicationPage', requestId: id, collection: 'log', snapshotId: id,
  offset: 0, total: 3, retained: 3, nextCursor: `${id}:2`, ageMs: 0, rows: [{ call: 'W1AW' }, { call: 'K1ABC' }], meta: {} }
describe('bounded collection contract', () => {
  it('accepts explicit reads and contiguous bounded pages', () => {
    expect(queryRequest(request)).toEqual(request)
    expect(queryPage(page)).toEqual(page)
  })
  it('rejects writes, extra fields, executable keys, and invalid cursors', () => {
    for (const patch of [{ collection: 'delete_log' }, { index: 2 }, { search: 'a'.repeat(97) },
      { cursor: '../log.adi' }, { cursor: `${id}:2:` }, { search: '\u0085' }, { search: '\ud800' }, { collection: 'spots', search: 'W1AW' }]) {
      expect(() => queryRequest({ ...request, ...patch })).toThrow()
    }
    expect(() => queryPage({ ...page, rows: [JSON.parse('{"__proto__":{}}')] })).toThrow()
  })
  it('rejects discontinuous, excessive and expired result envelopes', () => {
    for (const patch of [{ nextCursor: `${id}:1` }, { retained: 2, total: 1 },
      { nextCursor: null }, { rows: Array(129).fill({}) }, { ageMs: 3000 }, { offset: -1 }]) {
      expect(() => queryPage({ ...page, ...patch })).toThrow()
    }
  })
})

const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })
it('v3 query credit survives hibernation and stays isolated from other observers', () => {
  const station = peer(), browser = peer(), other = peer()
  const forward = '20000000-0000-4000-8000-000000000002'
  let relay = new ApplicationRelay(() => forward)
  const sync = () => relay.sync({ peer: station, version: 3 }, [{ sessionId: 'one', peer: browser }, { sessionId: 'two', peer: other }], 0)
  sync()
  relay.receiveBrowser('one', { type: 'applicationHello', version: 3 }, 0)
  relay.receiveBrowser('two', { type: 'applicationHello', version: 2 }, 0)
  expect(JSON.parse(browser.send.mock.lastCall![0])).toMatchObject({ version: 3, commands: expect.arrayContaining(['get_remote_page']) })
  expect(JSON.parse(other.send.mock.lastCall![0]).version).toBe(2)
  relay.receiveBrowser('one', { ...request, search: 'W1SECRET' }, 1)
  const checkpoint = relay.checkpoint('one')!
  expect(JSON.stringify(checkpoint)).not.toContain('W1SECRET')
  expect(checkpoint.version).toBe(3)
  relay = new ApplicationRelay(() => forward); sync(); relay.restore('one', checkpoint)
  relay.receiveStation({ ...page, requestId: forward }, 2)
  expect(JSON.parse(browser.send.mock.lastCall![0]).requestId).toBe(id)
  expect(other.send).toHaveBeenCalledTimes(1)
  relay.receiveBrowser('one', { type: 'applicationQueryAck', requestId: id }, 3)
  expect(relay.checkpoint('one')).toMatchObject({ version: 3, query: { pending: null } })
  expect(station.close).not.toHaveBeenCalled()
})
it('unapproved queries never reach the station and a revoked query response has no new owner', () => {
  const station = peer(), browser = peer()
  const relay = new ApplicationRelay(() => id)
  relay.sync({ peer: station, version: 3 }, [{ sessionId: 'approved', peer: browser }], 0)
  relay.receiveBrowser('unknown', request, 0)
  expect(station.send).not.toHaveBeenCalled()
  relay.receiveBrowser('approved', { type: 'applicationHello', version: 3 }, 0)
  relay.receiveBrowser('approved', request, 1)
  relay.sync({ peer: station, version: 3 }, [], 2)
  relay.receiveStation(page, 3)
  expect(browser.send).toHaveBeenCalledTimes(1)
  expect(station.close).not.toHaveBeenCalled()
})
