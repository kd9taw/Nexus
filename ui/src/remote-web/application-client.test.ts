import { afterEach, expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { APPLICATION_COMMANDS } from './application-protocol'
afterEach(() => vi.useRealTimers())
function setup() {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
  const sent: Record<string, unknown>[] = [], close = vi.fn()
  const client = new ApplicationClient(message => sent.push(JSON.parse(message)), close)
  client.open()
  client.receive({ type: 'applicationCapabilities', version: 1, commands: APPLICATION_COMMANDS })
  return { client, sent, close }
}
function result(request: Record<string, unknown>, revision: number, data: unknown, baseRevision: number | null = null) {
  return { type: 'applicationResult', requestId: request.requestId, command: request.command,
    revision, baseRevision, ageMs: 0, data, removed: [] }
}
it('coalesces panel reads, ACKs before the next request and applies exact-base deltas', async () => {
  const { client, sent } = setup()
  const reads = Array.from({ length: 20 }, () => client.invoke('get_snapshot'))
  const settings = client.invoke('get_settings')
  expect(sent.filter(m => m.type === 'applicationRead')).toHaveLength(1)
  client.receive(result(sent[1], 1, { mycall: 'TEST', radio: { dialMhz: 14.074 } }))
  expect(sent[2].type).toBe('applicationAck')
  expect(sent[3].command).toBe('get_settings')
  client.receive(result(sent[3], 2, { units: 'imperial' }))
  expect(await settings).toEqual({ units: 'imperial' })
  for (const reading of await Promise.all(reads)) expect(reading).toEqual({ mycall: 'TEST', radio: { dialMhz: 14.074 } })
  const count = sent.length
  await client.invoke('get_snapshot')
  expect(sent).toHaveLength(count)
  await vi.advanceTimersByTimeAsync(501)
  const next = client.invoke('get_snapshot')
  const request = sent[sent.length - 1]
  expect(request.revision).toBe(1)
  client.receive(result(request, 3, { radio: { dialMhz: 7.074 } }, 1))
  expect(await next).toEqual({ mycall: 'TEST', radio: { dialMhz: 7.074 } })
})
it('cannot send controls, arguments or unreviewed credential reads', async () => {
  const { client, sent } = setup()
  for (const command of ['halt_tx', 'log_current_qso', 'get_credentials_status']) {
    await expect(client.invoke(command)).rejects.toThrow('applicationUnsupported')
  }
  await expect(client.invoke('get_snapshot', { stationId: 'other' })).rejects.toThrow('applicationUnsupported')
  expect(sent).toHaveLength(1)
})
it('drops cached data and pending reads on loss, and requires a new full response', async () => {
  const { client, sent } = setup()
  const read = client.invoke('get_snapshot')
  client.receive(result(sent[1], 1, { mycall: 'TEST' }))
  await read
  const pending = client.invoke('get_settings').catch(error => error.message)
  client.disconnected()
  expect(await pending).toBe('applicationUnavailable')
  await expect(client.invoke('get_snapshot')).rejects.toThrow('applicationUnavailable')
  client.open(); client.receive({ type: 'applicationCapabilities', version: 1, commands: APPLICATION_COMMANDS })
  const again = client.invoke('get_snapshot')
  expect(sent[sent.length - 1].revision).toBe(null)
  client.receive(result(sent[sent.length - 1], 1, { mycall: 'TEST2' }))
  expect(await again).toEqual({ mycall: 'TEST2' })
})
it('bounds a stalled read and reports an older installer without attempting application reads', async () => {
  const { client, sent, close } = setup()
  const request = client.invoke('get_snapshot').catch(error => error.message)
  await vi.advanceTimersByTimeAsync(3000)
  expect(await request).toBe('applicationUnavailable')
  expect(close).toHaveBeenCalledOnce()
  client.open(); client.receive({ type: 'applicationCapabilities', version: 0, commands: [] })
  const count = sent.length
  await expect(client.invoke('get_snapshot')).rejects.toThrow('stationUpdateRequired')
  expect(sent).toHaveLength(count)
})
