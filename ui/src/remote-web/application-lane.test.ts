// The lane separation BrowserApplication's `stale` depends on, pinned end to end.
//
// `stale` is `client.age('get_snapshot') >= APPLICATION_TIMEOUT_MS`, and it sets StationDataContext
// false, which disables every station control. 1.13.0 added three background alert polls (needs
// 30 s, pounce 15 s, POTA 120 s) alongside the 1 s decode-history poll, and the report was that
// they were saturating the one lane every control depends on.
//
// They are not on that lane. From v2 `get_snapshot` rides the credit-based instrument stream
// (ApplicationStreamClient <-> ApplicationStreamRelay), while every alert poll rides the query
// lane (ApplicationQueryClient <-> ApplicationQueryRelay): separate single-flights, separate
// relay budgets, separate station branches. This test holds that apart, and the control below
// proves the measurement can actually see a starved snapshot when there is one.
import { afterEach, expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { APPLICATION_TIMEOUT_MS } from './application-protocol'
import { STREAM_INTERVAL } from './application-stream-protocol'
import type { StreamTopic } from './application-stream-protocol'
import { OTA_COMMAND, POUNCE_COMMAND, QUERY_COMMAND } from './application-query-protocol'

afterEach(() => vi.useRealTimers())

const TOPICS: StreamTopic[] = ['get_snapshot', 'get_settings', 'get_band_plan', 'get_spectrum_row', 'get_meters']

/** The station as `src-tauri/src/remote_service/transport.rs` runs it: a 100 ms application tick
 * that spends its credit on the topics whose interval has elapsed, and ONE query at a time
 * (`query_task.is_some()` answers `applicationBusy` without reading the board). `starveAfterMs`
 * is the positive control: past it the station stops publishing `get_snapshot` and nothing else. */
function harness({ queryMs: initialQueryMs, starveAfterMs = Infinity }: { queryMs: number; starveAfterMs?: number }) {
  const relay = new ApplicationRelay()
  let queryMs = initialQueryMs
  let watch: { id: string; topics: StreamTopic[] } | null = null
  let credit: string | null = null
  let revision = 0
  let queryBusy = false
  let closed = false
  const sentAt = new Map<StreamTopic, number>()
  // A socket delivers on a later turn. Delivering synchronously lets a reply re-enter the sender
  // mid-statement, which no real peer can do and which the relay's own bookkeeping assumes away.
  const wire = (deliver: () => void) => { setTimeout(deliver, 0) }
  const station = { send: (text: string) => wire(() => fromRelay(JSON.parse(text) as Record<string, unknown>)), close: () => { closed = true } }
  // Once either end has closed, the socket is gone: nothing more is delivered in either direction.
  const browser = { send: (text: string) => wire(() => { if (!closed) client.receive(JSON.parse(text) as Record<string, unknown>) }), close: () => { closed = true } }
  const client = new ApplicationClient(
    text => wire(() => { if (!closed) relay.receiveBrowser('one', JSON.parse(text) as Record<string, unknown>, performance.now()) }),
    () => { closed = true }, 17)

  function fromRelay(message: Record<string, unknown>): void {
    if (message.type === 'applicationWatch') {
      watch = message.topics ? { id: String(message.watchId), topics: message.topics as StreamTopic[] } : null
      credit = (message.requestId as string | null) ?? null
      sentAt.clear()
      return
    }
    if (message.type === 'applicationCredit') {
      if (watch && message.watchId === watch.id) credit = String(message.requestId)
      return
    }
    if (message.type !== 'applicationQuery') return
    const requestId = String(message.requestId), collection = message.collection
    if (queryBusy) { relay.receiveStation({ type: 'applicationQueryError', requestId, error: 'applicationBusy' }, performance.now()); return }
    queryBusy = true
    setTimeout(() => {
      queryBusy = false
      relay.receiveStation({ type: 'applicationPage', requestId, collection, snapshotId: crypto.randomUUID(),
        offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [], meta: {} }, performance.now())
    }, queryMs)
  }

  const tick = setInterval(() => {
    const now = performance.now()
    if (!watch || !credit) return
    const updates = watch.topics.flatMap(topic => {
      if (topic === 'get_snapshot' && now >= starveAfterMs) return []
      const at = sentAt.get(topic)
      if (at !== undefined && now - at < STREAM_INTERVAL[topic]) return []
      sentAt.set(topic, now)
      return [{ type: 'applicationResult', requestId: credit, command: topic, revision: ++revision,
        baseRevision: null, ageMs: 0, data: { at: now }, removed: [] }]
    })
    if (!updates.length) return
    const requestId = credit
    credit = null
    relay.receiveStation({ type: 'applicationBatch', watchId: watch.id, requestId, updates }, now)
  }, 100)

  relay.sync({ peer: station, version: 17 }, [{ sessionId: 'one', peer: browser }], performance.now())
  client.open()
  return { client, relay, stop: () => clearInterval(tick), closed: () => closed, setQueryMs: (ms: number) => { queryMs = ms } }
}

const query = (client: ApplicationClient, command: string, collection: string): Promise<unknown> =>
  client.invoke(command, { collection, cursor: null, search: '', unconfirmed: false, after: null }).catch(() => {})

/** BrowserApplication's own readers, at its own cadences, for five simulated minutes. */
async function run(options: { queryMs: number; starveAfterMs?: number; minutes?: number }) {
  const { client, relay, stop, closed } = harness(options)
  await vi.advanceTimersByTimeAsync(10)
  expect(client.getPhase()).toBe('ready')
  let worst = 0
  const read = () => { for (const topic of TOPICS) void client.invoke(topic).catch(() => {}) }
  read()
  const timers = [
    setInterval(read, 2000),                                                          // the bootstrap load
    setInterval(() => { void client.invoke('get_snapshot').catch(() => {}); void query(client, QUERY_COMMAND, 'decodes') }, 1000),
    setInterval(() => { void query(client, QUERY_COMMAND, 'needs') }, 30_000),        // useNeedAlerts
    setInterval(() => { void query(client, POUNCE_COMMAND, 'pounce') }, 15_000),      // useRareDxAlerts
    setInterval(() => { void query(client, OTA_COMMAND, 'ota') }, 120_000),           // usePotaAlerts
    setInterval(() => { worst = Math.max(worst, client.age('get_snapshot')) }, 500),  // BrowserApplication's own tick
    // The room's alarm (remote/src/room.ts:324) drives the relay's own expiry. Polling it is at
    // least as prompt as scheduling on nextDeadline(), so the relay's timeout is not modelled away.
    setInterval(() => relay.expire(performance.now()), 100),
  ]
  await vi.advanceTimersByTimeAsync((options.minutes ?? 5) * 60_000)
  for (const timer of timers) clearInterval(timer)
  stop()
  return { worst, closed: closed(), phase: client.getPhase() }
}

it('alert polls never age get_snapshot into `stale`, and the control proves that is measured', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  // Five minutes with all four pollers on the query lane and a slow (400 ms) board read: the
  // snapshot stays inside its own 500 ms publish interval, so no control is ever disabled.
  const busy = await run({ queryMs: 400 })
  expect(busy.closed).toBe(false)
  expect(busy.worst).toBeLessThan(STREAM_INTERVAL.get_snapshot + 200)

  // POSITIVE CONTROL: the same run with the station withholding `get_snapshot` alone. If this
  // did not trip, the assertion above would be measuring nothing.
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  const starved = await run({ queryMs: 400, starveAfterMs: 10_000 })
  expect(starved.worst).toBeGreaterThanOrEqual(APPLICATION_TIMEOUT_MS)
})

// A board read that outruns APPLICATION_TIMEOUT_MS used to end the whole session: the query lane's
// deadline was `fail`, which ApplicationClient wires to `disconnected()` + close, and the relay's
// own query expiry closed the browser socket with 1008. Either one leaves `supports()` false and
// phase off 'ready', which disables every station control and drops the audio lease - for a station
// that is answering the instrument stream perfectly well. 1.13.0 tripled the reads exposed to it.
it('a slow board read fails that read and leaves the session operating', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  const slow = await run({ queryMs: APPLICATION_TIMEOUT_MS + 100 })
  expect(slow.closed).toBe(false)
  expect(slow.phase).toBe('ready')
  // The snapshot kept arriving throughout, so no control was ever disabled.
  expect(slow.worst).toBeLessThan(STREAM_INTERVAL.get_snapshot + 200)
})

it('the slow read itself fails, and the lane keeps serving after it', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  // POSITIVE CONTROL for the test above: surviving is only the right answer if the read in fact
  // failed and the lane in fact recovered. A session that quietly stopped reading would pass the
  // assertions above and fail these.
  const { client, relay, stop, closed, setQueryMs } = harness({ queryMs: APPLICATION_TIMEOUT_MS + 100 })
  const alarm = setInterval(() => relay.expire(performance.now()), 100)
  const feed = setInterval(() => { for (const topic of TOPICS) void client.invoke(topic).catch(() => {}) }, 500)
  await vi.advanceTimersByTimeAsync(1000)

  let refused: unknown = 'never settled'
  void client.invoke(POUNCE_COMMAND, { collection: 'pounce', cursor: null, search: '', unconfirmed: false, after: null })
    .then(() => { refused = 'resolved' }, (error: Error) => { refused = error.message })
  await vi.advanceTimersByTimeAsync(APPLICATION_TIMEOUT_MS + 2000)
  expect(refused).toBe('applicationUnavailable')
  expect(closed()).toBe(false)

  // ...and the next read on the same lane still gets an answer.
  setQueryMs(100)
  let next: unknown = 'never settled'
  void client.invoke(OTA_COMMAND, { collection: 'ota', cursor: null, search: '', unconfirmed: false, after: null })
    .then(() => { next = 'resolved' }, (error: Error) => { next = error.message })
  await vi.advanceTimersByTimeAsync(1000)
  expect(next).toBe('resolved')
  expect(client.getPhase()).toBe('ready')
  clearInterval(feed); clearInterval(alarm); stop()
})
