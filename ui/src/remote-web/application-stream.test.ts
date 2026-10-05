import { afterEach, expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationStreamClient } from './application-stream-client'
import { ApplicationRelay } from './application-relay'
import { APPLICATION_COMMANDS, APPLICATION_TIMEOUT_MS, applicationReply } from './application-protocol'
import { STREAM_TOPICS, streamUpdates } from './application-stream-protocol'
import type { StreamTopic } from './application-stream-protocol'

afterEach(() => vi.useRealTimers())
const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn<(c: number, r: string) => void>() })
const last = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0])
const id = () => crypto.randomUUID()
function sample(requestId: string, command: StreamTopic = 'get_snapshot', revision = 1, baseRevision: number | null = null) {
  return { type: 'applicationResult', requestId, command, revision, baseRevision, ageMs: 0,
    data: { mycall: 'TEST', radio: { dialMhz: 7.074 } }, removed: [] }
}
function setup() {
  const station = peer(), one = peer(), two = peer(), relay = new ApplicationRelay()
  const observers = [{ sessionId: 'one', peer: one }, { sessionId: 'two', peer: two }]
  relay.sync({ peer: station, version: 2 }, observers, 0)
  for (const name of ['one', 'two']) relay.receiveBrowser(name, { type: 'applicationHello', version: 2 }, 0)
  const subscribe = (name: string, topics: StreamTopic[], requestId = id(), now = 1) => {
    relay.receiveBrowser(name, { type: 'applicationSubscribe', topics, requestId: topics.length ? requestId : null }, now)
    return requestId
  }
  const batch = (watch = last(station), now = 10, updates = [sample(watch.requestId)]) => {
    relay.receiveStation({ type: 'applicationBatch', watchId: watch.watchId, requestId: watch.requestId, updates }, now)
  }
  return { station, one, two, relay, observers, subscribe, batch }
}
it('keeps the v1 command vocabulary closed while negotiating all seven passive v2 topics', () => {
  const { one, station, relay } = setup()
  expect(last(one)).toEqual({ type: 'applicationCapabilities', version: 2, commands: STREAM_TOPICS })
  expect(() => applicationReply(sample(id(), 'get_cw_state'))).toThrow()
  relay.sync({ peer: station, version: 2 }, [{ sessionId: 'legacy', peer: one }], 1)
  relay.receiveBrowser('legacy', { type: 'applicationHello' }, 1)
  expect(last(one)).toEqual({ type: 'applicationCapabilities', version: 1, commands: APPLICATION_COMMANDS })
})
it('shares one native watch across observers, gives a joining browser a full base, and does not loop on ACKs', () => {
  const { station, one, two, relay, subscribe, batch } = setup()
  subscribe('one', ['get_snapshot']); const watch = last(station)
  subscribe('two', ['get_snapshot'])
  expect(station.send).toHaveBeenCalledOnce()
  batch(watch)
  expect(last(one).updates[0].data.mycall).toBe('TEST')
  expect(last(two).updates[0].baseRevision).toBeNull()
  expect(station.send).toHaveBeenCalledTimes(2) // exactly one next native credit
  for (const [name, browser] of [['one', one], ['two', two]] as const) {
    relay.receiveBrowser(name, { type: 'applicationFrameAck', requestId: last(browser).requestId, nextRequestId: id() }, 11)
    expect(browser.send).toHaveBeenCalledTimes(2) // hello + frame; ACK sends nothing
  }
  const credit = last(station)
  batch(credit, 500, [sample(credit.requestId, 'get_snapshot', 2, 1)])
  expect(last(one).updates[0].baseRevision).toBe(1)
  expect(last(two).updates[0].baseRevision).toBe(1)
})
it('retires old watch epochs and removes native demand when the last subscriber leaves', () => {
  const { station, one, subscribe, batch } = setup()
  const request = subscribe('one', ['get_snapshot']), old = last(station)
  subscribe('one', ['get_snapshot', 'get_cw_state'], request, 2)
  const current = last(station)
  expect(current.watchId).not.toBe(old.watchId)
  batch(old, 3)
  expect(one.send).toHaveBeenCalledOnce()
  batch(current, 4)
  subscribe('one', [], request, 5)
  expect(last(station).topics).toEqual([])
  expect(last(station).requestId).toBeNull()
})
it('hibernates using routing metadata only and restarts with a fresh watch/full samples', () => {
  const { station, one, relay, observers, subscribe, batch } = setup()
  subscribe('one', ['get_snapshot']); batch()
  const saved = relay.checkpoint('one')!
  expect(JSON.stringify(saved)).not.toMatch(/TEST|mycall|dialMhz|radio/)
  const oldWatch = last(station)
  const resumed = new ApplicationRelay()
  resumed.sync({ peer: station, version: 2 }, observers, 20)
  resumed.restore('one', saved)
  resumed.sync({ peer: station, version: 2 }, observers, 20)
  const watch = last(station)
  expect(watch.type).toBe('applicationWatch')
  expect(watch.watchId).not.toBe(oldWatch.watchId)
  resumed.receiveBrowser('one', { type: 'applicationFrameAck', requestId: last(one).requestId, nextRequestId: id() }, 21)
  resumed.receiveStation({ type: 'applicationBatch', watchId: watch.watchId, requestId: watch.requestId, updates: [sample(watch.requestId)] }, 22)
  expect(last(one).updates[0].baseRevision).toBeNull()
})
it('isolates slow or disconnected observers and refuses reused credits', () => {
  const { station, one, two, relay, subscribe, batch } = setup()
  subscribe('one', ['get_snapshot']); subscribe('two', ['get_snapshot'])
  one.send.mockImplementation(() => { throw new Error('closed') })
  batch()
  expect(one.close).toHaveBeenCalledWith(1001, 'applicationUnavailable')
  expect(station.close).not.toHaveBeenCalled()
  const ack = { type: 'applicationFrameAck', requestId: last(two).requestId, nextRequestId: id() }
  relay.receiveBrowser('two', ack, 11)
  relay.receiveBrowser('two', ack, 12)
  expect(two.close).toHaveBeenCalledWith(1008, 'invalidApplicationRequest')
  expect(station.close).not.toHaveBeenCalled()
})
it('rejects stale native responses, prototype payloads, duplicate topics and mutation requests', () => {
  const stale = setup()
  stale.subscribe('one', ['get_snapshot'])
  stale.batch(last(stale.station), 3001)
  expect(stale.station.close).toHaveBeenCalledWith(1008, 'invalidApplicationResult')
  const requestId = id()
  expect(() => streamUpdates([sample(requestId), sample(requestId)], requestId)).toThrow()
  expect(() => streamUpdates([{ ...sample(requestId), data: JSON.parse('{"__proto__":{"polluted":true}}') }], requestId)).toThrow()
  const commands = setup()
  commands.relay.receiveBrowser('one', { type: 'applicationRead', command: 'set_ptt', requestId, revision: null }, 1)
  expect(commands.one.close).toHaveBeenCalled()
  expect(commands.station.send).not.toHaveBeenCalled()
})
// A WALL CLOCK STEPPING BACK NEVER DISCONNECTS A STATION. The relay times credits and samples on its own
// clock (a Durable Object reads the host's wall clock), so an answer that reads as arriving before its
// credit is that clock stepping back, never anything the station did. Found on a WSL2 dev box whose clock
// steps back ~115 ms every ~30 s: a station answering 100 ms after a credit read as -15 ms, and the relay
// dropped it, and every browser with it.
it('accepts a station answer that reads as arriving before its credit, when the relay clock steps back', () => {
  const { station, one, subscribe, batch } = setup()
  subscribe('one', ['get_snapshot'], id(), 1000)
  batch(last(station), 985)
  expect(station.close, 'a clock step never closes the station').not.toHaveBeenCalled()
  expect(last(station).type, 'the next credit is issued').toBe('applicationCredit')
  expect(last(one).updates[0].data.mycall, 'and the sample reaches the browser').toBe('TEST')
})
// Both kinds of answer, because a sample carries its own age check that refuses at the same instant
// (age = elapsed + ageMs): only a station error, which has no age, shows the credit's own bound.
it('keeps the 3 s bound exactly: an answer 2999 ms after its credit is accepted, one 3000 ms after closes the station', () => {
  for (const kind of ['error', 'sample'] as const) for (const [after, closes] of [[2999, false], [3000, true]] as const) {
    const { station, relay, subscribe } = setup()
    subscribe('one', ['get_snapshot'], id(), 1000)
    const watch = last(station)
    const update = kind === 'error'
      ? { type: 'applicationError', requestId: watch.requestId, command: 'get_snapshot', error: 'applicationUnavailable' }
      : sample(watch.requestId)
    relay.receiveStation({ type: 'applicationBatch', watchId: watch.watchId, requestId: watch.requestId, updates: [update] }, 1000 + after)
    if (closes) expect(station.close, `${kind} at ${after} ms`).toHaveBeenCalledWith(1008, 'invalidApplicationResult')
    else expect(station.close, `${kind} at ${after} ms`).not.toHaveBeenCalled()
  }
})
it('delivers a held sample on a browser ACK that reads as arriving before the sample, when the relay clock steps back', () => {
  const { station, one, relay, subscribe, batch } = setup()
  subscribe('one', ['get_snapshot'], id(), 1000)
  batch(last(station), 1100)
  const frame = last(one), credit = last(station)
  // Revision 2 arrives while the browser still owes an ACK for the first frame, so the relay holds it.
  batch(credit, 1200, [sample(credit.requestId, 'get_snapshot', 2, 1)])
  expect(last(one).requestId, 'precondition: revision 2 is held for the ACK').toBe(frame.requestId)
  // The clock steps back: the ACK reads as arriving 50 ms before revision 2 was taken in.
  relay.receiveBrowser('one', { type: 'applicationFrameAck', requestId: frame.requestId, nextRequestId: id() }, 1050)
  expect(last(one).updates?.[0]?.revision, 'revision 2 is delivered on the ACK').toBe(2)
  expect(one.close).not.toHaveBeenCalled()
})
it('combines local panel reads into one subscription, shares results and stops expired interest', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  const sent: Record<string, unknown>[] = [], close = vi.fn()
  const client = new ApplicationClient(message => sent.push(JSON.parse(message)), close, 2)
  client.open(); client.receive({ type: 'applicationCapabilities', version: 2, commands: STREAM_TOPICS })
  const reads = Array.from({ length: 12 }, () => client.invoke('get_cw_state'))
  const scope = client.invoke('get_scope_snapshot')
  await vi.advanceTimersByTimeAsync(1)
  expect(sent).toHaveLength(2)
  expect(sent[1].topics).toEqual(['get_cw_state', 'get_scope_snapshot'])
  const respond = () => {
    const requestId = (sent[sent.length - 1].nextRequestId ?? sent[sent.length - 1].requestId) as string
    client.receive({ type: 'applicationFrame', requestId, updates: [sample(requestId, 'get_cw_state'), sample(requestId, 'get_scope_snapshot')] })
  }
  respond()
  expect((await Promise.all(reads)).every(v => (v as { mycall: string }).mycall === 'TEST')).toBe(true)
  expect(await scope).toHaveProperty('mycall', 'TEST')
  for (let i = 0; i < 5; i++) { await vi.advanceTimersByTimeAsync(500); if (i < 4) respond() }
  expect(sent[sent.length - 1]).toEqual({ type: 'applicationSubscribe', topics: [], requestId: null })
  const count = sent.length
  await vi.advanceTimersByTimeAsync(5000)
  expect(sent).toHaveLength(count); expect(close).not.toHaveBeenCalled()
  client.disconnected()
})
it('includes the browser credit round trip in freshness and falls back to the old installer contract', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  const sent: Record<string, unknown>[] = [], client = new ApplicationClient(m => sent.push(JSON.parse(m)), vi.fn(), 2)
  client.open(); client.receive({ type: 'applicationCapabilities', version: 2, commands: STREAM_TOPICS })
  const pending = client.invoke('get_snapshot').catch(e => e.message)
  await vi.advanceTimersByTimeAsync(2000)
  const requestId = sent[1].requestId as string
  // Older than the freshness window: committed as stale so the next delta still applies, never
  // handed to the waiting reader, and not a protocol error. The stream continues.
  client.receive({ type: 'applicationFrame', requestId, updates: [{ ...sample(requestId), ageMs: 1100 }] })
  expect(client.age('get_snapshot')).toBeGreaterThanOrEqual(APPLICATION_TIMEOUT_MS)
  expect(sent[sent.length - 1]).toMatchObject({ type: 'applicationFrameAck', requestId })
  let settled = false; void pending.then(() => { settled = true })
  await vi.advanceTimersByTimeAsync(0); expect(settled).toBe(false)
  const next = sent[sent.length - 1].nextRequestId as string
  client.receive({ type: 'applicationFrame', requestId: next, updates: [sample(next, 'get_snapshot', 2, 1)] })
  expect(await pending).toHaveProperty('mycall', 'TEST')
  expect(client.age('get_snapshot')).toBeLessThan(APPLICATION_TIMEOUT_MS)
  client.disconnected()
  client.open(); client.receive({ type: 'applicationCapabilities', version: 1, commands: APPLICATION_COMMANDS })
  expect(client.supports('get_snapshot')).toBe(true)
  expect(client.supports('get_cw_state')).toBe(false)
  await expect(client.invoke('get_cw_state')).rejects.toThrow('applicationUnsupported')
  client.disconnected()
})

// One busy station sample must not blank the panel. The station's radio loop holds its Engine
// across blocking CAT, so a busy miss is routine; the browser used to answer it by deleting the
// snapshot it was showing, which made age() Infinity, disabled every station control at once and
// cancelled a wheel tune in flight. The value stays, and the read that was waiting is asked again
// (the operator's pick "Retry like the page", 2026-10-04).
it('keeps a known-good value through a station error, aged, and applies the next delta to it', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  const sent: Record<string, unknown>[] = [], client = new ApplicationClient(m => sent.push(JSON.parse(m)), vi.fn(), 2)
  client.open(); client.receive({ type: 'applicationCapabilities', version: 2, commands: STREAM_TOPICS })
  const first = client.invoke('get_snapshot')
  await vi.advanceTimersByTimeAsync(1)
  const requestId = sent[1].requestId as string
  client.receive({ type: 'applicationFrame', requestId, updates: [sample(requestId)] })
  expect(await first).toHaveProperty('mycall', 'TEST')
  // Past the topic's interval a read waits for the stream; the station answers it busy.
  await vi.advanceTimersByTimeAsync(600)
  const busy = client.invoke('get_snapshot').catch(e => (e as Error).message)
  const second = sent[sent.length - 1].nextRequestId as string
  client.receive({ type: 'applicationFrame', requestId: second, updates: [{ type: 'applicationError', requestId: second, command: 'get_snapshot', error: 'applicationBusy' }] })
  expect(client.age('get_snapshot')).toBeGreaterThanOrEqual(600)
  expect(client.age('get_snapshot'), 'the held value is aged, never gone').toBeLessThan(APPLICATION_TIMEOUT_MS)
  // The kept value is still the delta base the station believes the browser holds.
  const third = sent[sent.length - 1].nextRequestId as string
  const delta = { ...sample(third, 'get_snapshot', 2, 1), data: { radio: { dialMhz: 14.074 } } }
  const read = client.invoke('get_snapshot')
  client.receive({ type: 'applicationFrame', requestId: third, updates: [delta] })
  expect(await read).toEqual({ mycall: 'TEST', radio: { dialMhz: 14.074 } })
  expect(client.age('get_snapshot')).toBeLessThan(100)
  // The read the station answered busy is asked again 250 ms later, and gets that sample.
  await vi.advanceTimersByTimeAsync(250)
  expect(await busy, 'the busy read, asked again').toEqual({ mycall: 'TEST', radio: { dialMhz: 14.074 } })
  client.disconnected()
})

// A sample the station answers busy is read again from a later frame, as the page's collection reads are asked
// again (the operator's pick "Retry like the page", 2026-10-04): three attempts at most, 250 ms apart. The first
// sample of a topic finds nothing cached at the station, so a busy Engine refuses it, and the panel was left blank.
it('reads a busy sample again from a later frame, three times at most and 250 ms apart, and nothing but busy', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  const sent: Record<string, unknown>[] = [], client = new ApplicationClient(m => sent.push(JSON.parse(m)), vi.fn(), 2)
  client.open(); client.receive({ type: 'applicationCapabilities', version: 2, commands: STREAM_TOPICS })
  // The credit the next frame answers: the last ACK's next one, or the subscription's own.
  const credit = () => (sent[sent.length - 1].nextRequestId ?? sent[sent.length - 1].requestId) as string
  const refused = (command: StreamTopic, error = 'applicationBusy') => {
    const requestId = credit()
    client.receive({ type: 'applicationFrame', requestId, updates: [{ type: 'applicationError', requestId, command, error }] })
  }
  const settings = client.invoke('get_settings').catch(e => (e as Error).message)
  await vi.advanceTimersByTimeAsync(1)
  refused('get_settings')
  await vi.advanceTimersByTimeAsync(250)
  const answer = credit()
  client.receive({ type: 'applicationFrame', requestId: answer, updates: [sample(answer, 'get_settings')] })
  expect(await settings, 'busy once, then the next frame\'s sample').toHaveProperty('mycall', 'TEST')
  // Busy three times: the third answer is the caller's.
  let settled = false
  const plan = client.invoke('get_band_plan').catch(e => (e as Error).message)
  void plan.then(() => { settled = true })
  await vi.advanceTimersByTimeAsync(1)
  refused('get_band_plan')
  await vi.advanceTimersByTimeAsync(250)
  refused('get_band_plan')
  await vi.advanceTimersByTimeAsync(250)
  expect(settled, 'still asking after two').toBe(false)
  refused('get_band_plan')
  expect(await plan).toBe('applicationBusy')
  // Any other refusal is the caller's at once.
  const meters = client.invoke('get_meters').catch(e => (e as Error).message)
  await vi.advanceTimersByTimeAsync(1)
  refused('get_meters', 'applicationUnavailable')
  expect(await meters).toBe('applicationUnavailable')
  client.disconnected()
})

// A station can stop answering WITHOUT the socket dropping — a stalled shack PC, a WAN blip, a
// Nexus that is briefly too busy to serve. The browser must not go permanently silent when that
// happens. Measured against the real browser before this test existed: served-request counters
// were byte-identical over eleven seconds after data resumed, because flush() early-returns while
// the topic list is unchanged and the stale credit is never re-primed. The workspace then sits on
// "Station data unavailable" until some navigation happens to change the topic set.
it('does not go silent when a credit is never answered', async () => {
  vi.useFakeTimers()
  const send = vi.fn<(message: string) => void>()
  const fail = vi.fn<() => void>()
  const client = new ApplicationStreamClient(send, fail)
  void client.invoke<unknown>('get_snapshot')
  await vi.advanceTimersByTimeAsync(1)
  const first = JSON.parse(send.mock.lastCall![0]) as { type: string; requestId: string | null }
  expect(first.type).toBe('applicationSubscribe')
  expect(first.requestId).toBeTruthy()

  // The station consumes that credit and answers nothing, ever.
  send.mockClear()
  await vi.advanceTimersByTimeAsync(10_000)

  // Something must reach the wire or the connection must be failed. Silence is the bug: the
  // station is waiting for a credit that will never arrive, and so is the operator.
  expect(send.mock.calls.length + fail.mock.calls.length,
    'a stalled credit must either be re-primed or fail the connection, never leave it silent').toBeGreaterThan(0)
})
