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
// A stalled read fails THAT read, never the session. The v1 lane used to close the socket at the
// deadline, which ended the operating session - operations, lease and all - over one slow panel read.
// The lane is held until the late answer lands (the relay still holds its own slot for this read),
// then ACKed unread so the next read can go.
it('bounds a stalled read without closing the session, and holds the lane until the late answer is ACKed', async () => {
  const { client, sent, close } = setup()
  const request = client.invoke('get_snapshot').catch(error => error.message)
  await vi.advanceTimersByTimeAsync(3000)
  expect(await request).toBe('applicationUnavailable')
  expect(close).not.toHaveBeenCalled()
  expect(client.getPhase()).toBe('ready')
  const queued = client.invoke('get_settings')
  expect(sent.filter(m => m.type === 'applicationRead')).toHaveLength(1)
  client.receive(result(sent[1], 1, { mycall: 'LATE' }))
  expect(sent[2]).toEqual({ type: 'applicationAck', requestId: sent[1].requestId })
  expect(sent[3].command).toBe('get_settings')
  client.receive(result(sent[3], 1, { units: 'metric' }))
  expect(await queued).toEqual({ units: 'metric' })
  // The abandoned answer was dropped unread: a fresh read of that command is a full read.
  const again = client.invoke('get_snapshot')
  expect(sent[sent.length - 1].revision).toBe(null)
  client.receive(result(sent[sent.length - 1], 2, { mycall: 'TEST' }))
  expect(await again).toEqual({ mycall: 'TEST' })
})
// A busy station is asked again, as the page's collection reads are (the operator's pick "Retry like the page",
// 2026-10-04). The station reads with try_lock and never queues behind the radio loop, so `applicationBusy` for a
// sample it has not cached is routine; the panel used to go blank until its next poll. Three attempts at most,
// 250 ms apart, and any other answer goes back to the caller at once.
it('asks a busy read again, three times at most and 250 ms apart, and nothing but busy', async () => {
  const { client, sent } = setup()
  const reads = () => sent.filter(m => m.type === 'applicationRead')
  const latest = () => reads()[reads().length - 1]
  const refused = (request: Record<string, unknown>, error = 'applicationBusy') => ({ type: 'applicationError', requestId: request.requestId, error })
  const read = client.invoke('get_snapshot').catch(error => (error as Error).message)
  client.receive(refused(latest()))
  expect(sent[sent.length - 1], 'the refusal is ACKed').toEqual({ type: 'applicationAck', requestId: latest().requestId })
  await vi.advanceTimersByTimeAsync(249)
  expect(reads(), 'not asked again before 250 ms').toHaveLength(1)
  await vi.advanceTimersByTimeAsync(1)
  expect(reads(), 'asked again at 250 ms').toHaveLength(2)
  client.receive(result(latest(), 1, { mycall: 'TEST' }))
  expect(await read).toEqual({ mycall: 'TEST' })
  // Busy three times: the third answer is the caller's.
  const settings = client.invoke('get_settings').catch(error => (error as Error).message)
  for (let attempt = 0; attempt < 3; attempt++) {
    client.receive(refused(latest()))
    await vi.advanceTimersByTimeAsync(250)
  }
  expect(await settings).toBe('applicationBusy')
  expect(reads().filter(m => m.command === 'get_settings'), 'three attempts').toHaveLength(3)
  // Any other refusal is the caller's at once.
  const plan = client.invoke('get_band_plan').catch(error => (error as Error).message)
  client.receive(refused(latest(), 'applicationUnavailable'))
  expect(await plan).toBe('applicationUnavailable')
  await vi.advanceTimersByTimeAsync(1000)
  expect(reads().filter(m => m.command === 'get_band_plan'), 'never asked again').toHaveLength(1)
})
it('reports an older installer without attempting application reads', async () => {
  const { client, sent } = setup()
  client.disconnected()
  client.open(); client.receive({ type: 'applicationCapabilities', version: 0, commands: [] })
  const count = sent.length
  await expect(client.invoke('get_snapshot')).rejects.toThrow('stationUpdateRequired')
  expect(sent).toHaveLength(count)
})
