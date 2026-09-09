import { afterEach, expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { STREAM_TOPICS, streamTopics, streamUpdates } from './application-stream-protocol'

afterEach(() => vi.useRealTimers())
const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })
const last = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0])
const keyboard = ['get_rtty_state', 'get_psk_state']
const sample = (requestId: string, command: string, revision = 1, baseRevision: number | null = null) => ({
  type: 'applicationResult', requestId, command, revision, baseRevision, ageMs: 0,
  data: { text: 'CQ W1AW', armed: true }, removed: [],
})

it('negotiates keyboard observation only when both station and browser support v5', async () => {
  for (const stationVersion of [1, 2, 3, 4, 5]) for (const browserVersion of [1, 2, 3, 4, 5]) {
    const station = peer(), browser = peer(), relay = new ApplicationRelay()
    relay.sync({ peer: station, version: stationVersion }, [{ sessionId: 'one', peer: browser }], 0)
    const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), browserVersion)
    client.open()
    expect(browser.close).not.toHaveBeenCalled()
    client.receive(last(browser))
    expect(last(browser).version).toBe(Math.min(stationVersion, browserVersion))
    expect(client.supports('get_remote_recall')).toBe(stationVersion >= 4 && browserVersion >= 4)
    for (const command of keyboard) {
      const supported = stationVersion === 5 && browserVersion === 5
      expect(client.supports(command)).toBe(supported)
      if (!supported) {
        await expect(client.invoke(command)).rejects.toThrow()
        expect(station.send).not.toHaveBeenCalled()
      }
    }
    client.disconnected()
  }
})

it('keeps old stream vocabularies closed and validates the v5 frame as a whole', () => {
  const id = crypto.randomUUID()
  expect(streamTopics([...STREAM_TOPICS])).toEqual(STREAM_TOPICS)
  for (const command of keyboard) {
    expect(() => streamTopics([command])).toThrow()
    expect(() => streamUpdates([sample(id, command)], id)).toThrow()
  }
  for (const command of ['rtty_auto_arm', 'psk_auto_arm', 'rtty_send', 'psk_type', 'log_qso', 'set_ptt']) {
    expect(() => streamTopics([command], 5)).toThrow()
  }
  expect(streamTopics([...STREAM_TOPICS, ...keyboard], 5)).toHaveLength(9)
  expect(streamUpdates(keyboard.map(command => sample(id, command)), id, 5)).toHaveLength(2)
  expect(() => streamUpdates([sample(id, keyboard[0]), sample(id, keyboard[0])], id, 5)).toThrow()
})

it('routes mixed versions separately and restores keyboard demand without retaining transcript content', () => {
  const station = peer(), modern = peer(), old = peer(), relay = new ApplicationRelay()
  const observers = [{ sessionId: 'modern', peer: modern }, { sessionId: 'old', peer: old }]
  relay.sync({ peer: station, version: 5 }, observers, 0)
  relay.receiveBrowser('modern', { type: 'applicationHello', version: 5 }, 0)
  relay.receiveBrowser('old', { type: 'applicationHello', version: 4 }, 0)
  const credit = crypto.randomUUID()
  relay.receiveBrowser('modern', { type: 'applicationSubscribe', topics: keyboard, requestId: credit }, 1)
  const watch = last(station)
  expect(watch.topics).toEqual(keyboard)
  relay.receiveStation({ type: 'applicationBatch', watchId: watch.watchId, requestId: watch.requestId,
    updates: keyboard.map(command => sample(watch.requestId, command)) }, 2)
  expect(last(modern).updates.map((s: { command: string }) => s.command)).toEqual(keyboard)
  expect(old.send).toHaveBeenCalledOnce()
  const saved = relay.checkpoint('modern')!
  expect(saved.version).toBe(5)
  expect(JSON.stringify(saved)).not.toContain('CQ W1AW')
  const resumed = new ApplicationRelay()
  resumed.sync({ peer: station, version: 5 }, observers, 3)
  resumed.restore('modern', saved)
  resumed.sync({ peer: station, version: 5 }, observers, 3)
  const fresh = last(station)
  expect(fresh.watchId).not.toBe(watch.watchId)
  expect(fresh.topics).toEqual(keyboard)
  resumed.receiveBrowser('modern', { type: 'applicationFrameAck', requestId: credit, nextRequestId: crypto.randomUUID() }, 4)
  resumed.receiveStation({ type: 'applicationBatch', watchId: fresh.watchId, requestId: fresh.requestId,
    updates: keyboard.map(command => sample(fresh.requestId, command, 2)) }, 5)
  expect(last(modern).updates.every((s: { baseRevision: number | null }) => s.baseRevision === null)).toBe(true)
  relay.receiveBrowser('old', { type: 'applicationSubscribe', topics: keyboard, requestId: crypto.randomUUID() }, 6)
  expect(old.close).toHaveBeenCalledWith(1008, 'invalidApplicationRequest')
  expect(station.close).not.toHaveBeenCalled()
})
