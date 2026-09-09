import { expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { queryRequest } from './application-query-protocol'

const id = '10000000-0000-4000-8000-000000000001'
const recall = { type: 'applicationQuery', requestId: id, collection: 'recall', cursor: null, search: 'W1AW', unconfirmed: false, after: null }
const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })

it('recall requires its separately negotiated capability on both ends', async () => {
  for (const stationVersion of [1, 2, 3, 4]) {
    for (const browserVersion of [1, 2, 3, 4]) {
      const station = peer(), browser = peer()
      const relay = new ApplicationRelay(() => id)
      relay.sync({ peer: station, version: stationVersion }, [{ sessionId: 'one', peer: browser }], 0)
      const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), () => {}, browserVersion)
      client.open()
      client.receive(JSON.parse(browser.send.mock.lastCall![0]))
      expect(client.supports('get_remote_recall')).toBe(stationVersion === 4 && browserVersion === 4)
      if (stationVersion === 4 && browserVersion === 4) {
        relay.receiveBrowser('one', recall, 1)
        expect(JSON.parse(station.send.mock.lastCall![0])).toMatchObject(recall)
        const saved = relay.checkpoint('one')!
        expect(JSON.stringify(saved)).not.toContain('W1AW')
        const restored = new ApplicationRelay(() => id)
        restored.sync({ peer: station, version: 4 }, [{ sessionId: 'one', peer: browser }], 2)
        restored.restore('one', saved)
        expect(restored.checkpoint('one')).toEqual(saved)
      } else {
        await expect(client.invoke('get_remote_page', { collection: 'recall', cursor: null, search: 'W1AW', unconfirmed: false, after: null })).rejects.toThrow()
        relay.receiveBrowser('one', recall, 1)
        expect(station.send).not.toHaveBeenCalled()
      }
      client.disconnected()
    }
  }
})

it('recall accepts a callsign only and cannot broaden the old collection contract', () => {
  expect(() => queryRequest(recall)).toThrow()
  expect(queryRequest(recall, 4)).toEqual(recall)
  for (const patch of [{ search: '' }, { search: 'W1AW*' }, { search: '../log.adi' }, { search: 'W1AW K1ABC' },
    { search: 'A'.repeat(33) }, { cursor: `${id}:1` }, { after: 1 }, { unconfirmed: true }, { path: 'log.adi' }]) {
    expect(() => queryRequest({ ...recall, ...patch }, 4)).toThrow()
  }
})
