import { expect, it, vi } from 'vitest'
import { ApplicationRelay } from './application-relay'
const id = '8aa041cb-c642-459c-83f3-11a5b720647d'
const forwardId = 'bbe7d95a-6fbd-47aa-95a7-ab1e4c083dd6'
const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn<(c: number, r: string) => void>() })
const request = () => ({ type: 'applicationRead', requestId: id, command: 'get_snapshot', revision: null })
const reply = () => ({ type: 'applicationResult', requestId: forwardId, command: 'get_snapshot', revision: 1,
  baseRevision: null, ageMs: 0, data: { mycall: 'TEST' }, removed: [] })
function setup(version = 1) {
  const station = peer(), browser = peer(), relay = new ApplicationRelay(() => forwardId)
  relay.sync({ peer: station, version }, [{ sessionId: 'browser', peer: browser }], 0)
  relay.receiveBrowser('browser', { type: 'applicationHello' }, 0)
  return { station, browser, relay }
}
it('negotiates old installers without forwarding new messages to them', () => {
  const { station, browser, relay } = setup(0)
  relay.receiveBrowser('browser', request(), 1)
  expect(station.send).not.toHaveBeenCalled()
  expect(JSON.parse(browser.send.mock.calls[0][0]).version).toBe(0)
  expect(JSON.parse(browser.send.mock.lastCall![0]).error).toBe('stationUpdateRequired')
})
it('routes a valid read once and requires the exact result ACK before another read', () => {
  const { station, browser, relay } = setup()
  relay.receiveBrowser('outsider', request(), 0)
  expect(station.send).not.toHaveBeenCalled()
  relay.receiveBrowser('browser', request(), 1)
  expect(JSON.parse(station.send.mock.lastCall![0]).requestId).toBe(forwardId)
  relay.receiveStation(reply(), 20)
  expect(JSON.parse(browser.send.mock.lastCall![0]).requestId).toBe(id)
  relay.receiveBrowser('browser', { type: 'applicationAck', requestId: 'wrong' }, 21)
  expect(relay.checkpoint('browser')).toMatchObject({ version: 1, pending: expect.anything() })
  relay.receiveBrowser('browser', { type: 'applicationAck', requestId: id }, 22)
  relay.receiveBrowser('browser', request(), 23)
  expect(station.send).toHaveBeenCalledTimes(2)
})
it('refuses controls and preserves bounded pending routing through hibernation', () => {
  const { station, browser, relay } = setup()
  relay.receiveBrowser('browser', request(), 1)
  const checkpoint = relay.checkpoint('browser')!
  expect(JSON.stringify(checkpoint)).not.toContain('mycall')
  const restored = new ApplicationRelay(() => forwardId)
  restored.sync({ peer: station, version: 1 }, [{ sessionId: 'browser', peer: browser }], 2)
  restored.restore('browser', checkpoint)
  restored.receiveStation(reply(), 3)
  expect(JSON.parse(browser.send.mock.lastCall![0]).data.mycall).toBe('TEST')
  restored.receiveBrowser('browser', { ...request(), command: 'set_frequency' }, 4)
  expect(browser.close).toHaveBeenCalledWith(1008, 'invalidApplicationRequest')
  expect(station.send).toHaveBeenCalledTimes(1)
})
it('drops a result after revocation and disconnects a consumer that never acknowledges', () => {
  const { station, browser, relay } = setup()
  relay.receiveBrowser('browser', request(), 1)
  const sent = browser.send.mock.calls.length
  relay.sync({ peer: station, version: 1 }, [], 2)
  relay.receiveStation(reply(), 3)
  expect(browser.send).toHaveBeenCalledTimes(sent)
  const next = setup()
  next.relay.receiveBrowser('browser', request(), 1)
  next.relay.receiveStation(reply(), 2)
  next.relay.expire(3001)
  expect(next.browser.close).toHaveBeenCalledWith(1008, 'applicationTimeout')
})
it('isolates a failed consumer send from the station and another approved browser', () => {
  const station = peer(), failed = peer(), other = peer()
  const ids: ReturnType<typeof crypto.randomUUID>[] = [forwardId, '056c07c9-65c3-48fc-a188-957de3338a4a']
  const relay = new ApplicationRelay(() => ids.shift()!)
  relay.sync({ peer: station, version: 1 }, [{ sessionId: 'failed', peer: failed }, { sessionId: 'other', peer: other }], 0)
  for (const session of ['failed', 'other']) {
    relay.receiveBrowser(session, { type: 'applicationHello' }, 0)
    relay.receiveBrowser(session, request(), 1)
  }
  failed.send.mockImplementation(() => { throw new Error('socket closed') })
  failed.close.mockImplementation(() => { throw new Error('already closed') })
  expect(() => relay.receiveStation(reply(), 2)).not.toThrow()
  expect(failed.close).toHaveBeenCalledWith(1001, 'applicationUnavailable')
  expect(station.close).not.toHaveBeenCalled()
  expect(relay.checkpoint('failed')).toMatchObject({ version: 1, pending: null })
  const otherRequest = JSON.parse(station.send.mock.lastCall![0])
  relay.receiveStation({ ...reply(), requestId: otherRequest.requestId }, 3)
  expect(JSON.parse(other.send.mock.lastCall![0]).data.mycall).toBe('TEST')
})
