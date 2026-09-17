import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import type { OperationState } from './operation-protocol'
import { pendingControlStorage } from './control-storage'
import { ifAvailableLock } from './__fixtures__/receipt-lock'

// Operation v5: the station PUSHES a control's outcome (`operationEvent`) with the fresh state,
// where v4 left the browser to discover both by polling - a `state` read, then a `result` read,
// each a relay round trip, with a one-second back-off between them. The real browser client runs
// against the real relay; only the station is scripted, and its radio loop takes CAT_MS to land
// the target. Every test here is about round trips, so `forwarded` records what left the browser.

afterEach(() => vi.useRealTimers())

const CAT_MS = 80

/** `page` is what the compiled browser asks at (from the Worker's /config), `station` what the
 * station advertised at admission; the relay negotiates their minimum, and the station names
 * `outcomePush` only when that minimum is 5 - so the page learns whether THIS station pushes from
 * the state it holds, never from its own version. */
type Options = { page: 4 | 5; station: 4 | 5; push: boolean }
function connected({ page, station, push }: Options) {
  vi.useFakeTimers({ now: 1000 })
  const relay = new OperationRelay(), sessionId = crypto.randomUUID()
  const errors: string[] = [], forwarded: { at: number; type: string; version?: number }[] = []
  const negotiated = Math.min(page, station)
  const state: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'available', leaseId: null,
    revision: 1, commandWindowId: null, nextSequence: null, leaseRemainingMs: null,
    actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 },
      capabilities: ['frequency', ...(negotiated >= 5 ? ['outcomePush' as const] : [])] }
  }
  // THE SHIPPED RECEIPT STORE, on the browser's own lock discipline (`receipt-lock.ts`). A stub
  // that merely remembered the receipt (`write: v => { stored = v }`, `exclusive: run => run()`)
  // hid that the real store refuses a write outside its lock — which a pushed outcome's clear was,
  // so on a real page the receipt outlived every pushed event by a second.
  const memory = new Map<string, string>()
  const receipts = pendingControlStorage(() => ({ getItem: k => memory.get(k) ?? null, setItem: (k, v) => { memory.set(k, v) }, removeItem: k => { memory.delete(k) } }),
    'push-completion', ifAvailableLock())
  let leaseUntil = 0
  // The station's receipts: what a `result` read answers, and what the event carries.
  const settled = new Map<string, boolean>()
  const client = new OperationClient(wire => relay.receiveBrowser(sessionId, JSON.parse(wire), Date.now()), true,
    Date.now, undefined, page, receipts)
  const outcome = (operationId: string) => settled.get(operationId)
    ? { operation: 'stationControl', operationId, outcome: 'applied', evidence: 'radioReadback' }
    : { operation: 'stationControl', operationId, outcome: 'pending' }
  relay.sync({ supported: true, operationVersion: station, peer: {
    close: () => { throw Error('station closed') },
    send: raw => {
      const { request, operationVersion } = JSON.parse(raw)
      forwarded.push({ at: Date.now(), type: request.type, version: operationVersion })
      if (state.leaseId && Date.now() >= leaseUntil) Object.assign(state, { phase: 'available', leaseId: null,
        commandWindowId: null, nextSequence: null, leaseRemainingMs: null })
      if (request.type === 'acquire') Object.assign(state, { phase: 'controlling', leaseId: crypto.randomUUID(),
        commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000 })
      if (request.type === 'acquire' || request.type === 'heartbeat' && request.leaseId === state.leaseId) leaseUntil = Date.now() + 5000
      if (state.leaseId) state.leaseRemainingMs = leaseUntil - Date.now()
      // A state read after a control issues a new window, the control having spent the old one.
      if ((request.type === 'state' || request.type === 'heartbeat') && state.leaseId && !state.commandWindowId) state.commandWindowId = crypto.randomUUID()
      if (request.type === 'stationControl') {
        if (!state.leaseId || request.leaseId !== state.leaseId || request.commandWindowId !== state.commandWindowId) throw Error('station refused the window')
        state.nextSequence!++; state.revision++; state.commandWindowId = null
        settled.set(request.requestId, false)
        // The radio loop: the readback lands CAT_MS later, the receipt settles and - at v5 - the
        // station says so, carrying the state a `state` read would return at that moment.
        setTimeout(() => {
          settled.set(request.requestId, true)
          state.revision++
          state.commandWindowId = crypto.randomUUID()
          if (negotiated >= 5 && push) relay.receiveStation({ type: 'operationEvent', sessionId, operationId: request.requestId,
            value: outcome(request.requestId), state: structuredClone(state) })
        }, CAT_MS)
      }
      const value = request.type === 'stationControl' ? outcome(request.requestId)
        : request.type === 'result' ? outcome(request.operationId) : structuredClone(state)
      queueMicrotask(() => relay.receiveStation({ type: 'operationResponse', sessionId, requestId: request.requestId, value }))
    }
  } }, [{ sessionId, deviceId: crypto.randomUUID(), commandUntil: Number.MAX_SAFE_INTEGER, peer: {
    close: () => { throw Error('browser closed') }, send: raw => {
      const message = JSON.parse(raw)
      if (message.error) errors.push(message.error)
      if (message.type === 'operationEvent') client.receiveEvent(message)
      else client.receive(message)
    }
  } }], Date.now())
  return { client, forwarded, errors, state, stored: () => receipts.read() }
}

async function controlling(h: ReturnType<typeof connected>) {
  h.client.open(); await vi.advanceTimersByTimeAsync(0)
  const acquire = h.client.acquire(); await vi.advanceTimersByTimeAsync(0); await acquire
  for (let ticks = 0; ticks < 12 && (!h.client.getSnapshot().fresh || h.client.getSnapshot().busy); ticks++)
    await vi.advanceTimersByTimeAsync(250)
  expect(h.client.getSnapshot()).toMatchObject({ fresh: true, busy: false })
}
const tune = { action: 'radio.frequency' as const, dialMhz: 7.074, band: '40m', sideband: 'USB' as const }

it('v5: the outcome reaches the browser when the radio lands it, with no state or result round trip', async () => {
  const h = connected({ page: 5, station: 5, push: true })
  await controlling(h)
  const sentAt = Date.now(), mark = h.forwarded.length
  const action = h.client.control(tune)
  await vi.advanceTimersByTimeAsync(0)
  expect(h.forwarded.slice(mark).map(f => f.type)).toEqual(['stationControl'])
  await vi.advanceTimersByTimeAsync(CAT_MS)
  expect(await action).toMatchObject({ outcome: 'applied', evidence: 'radioReadback' })
  // THE LATENCY WIN: confirmed exactly when the readback landed, not a tick later - and the
  // browser asked for nothing after the control.
  expect(Date.now() - sentAt).toBe(CAT_MS)
  expect(h.forwarded.slice(mark).map(f => f.type)).toEqual(['stationControl'])
  // The carried state is installed as fresh, on the new window: no re-read, no grey-out.
  const view = h.client.getSnapshot()
  expect(view).toMatchObject({ fresh: true, busy: false, controlPending: null, controlRefreshing: false })
  expect(view.state?.commandWindowId).toBe(h.state.commandWindowId)
  expect(view.state?.revision).toBe(h.state.revision)
  expect(h.stored()).toBeNull()
  // ...and that window is usable at once: the next gesture goes out on it with nothing in between.
  const next = h.client.control(tune)
  await vi.advanceTimersByTimeAsync(0)
  expect(h.forwarded.slice(mark).map(f => f.type)).toEqual(['stationControl', 'stationControl'])
  await vi.advanceTimersByTimeAsync(CAT_MS)
  expect(await next).toMatchObject({ outcome: 'applied' })
  expect(h.errors).toEqual([])
})

it('positive control - v4: the same control needs a state read and a result read, and waits for the tick', async () => {
  const h = connected({ page: 4, station: 4, push: true })
  await controlling(h)
  const sentAt = Date.now(), mark = h.forwarded.length
  const action = h.client.control(tune)
  await vi.advanceTimersByTimeAsync(CAT_MS)
  // The readback has landed at the station. The browser does not know: it has to ask.
  let resolved = false
  void action.then(() => { resolved = true })
  await vi.advanceTimersByTimeAsync(0)
  expect(resolved).toBe(false)
  for (let ticks = 0; ticks < 20 && !resolved; ticks++) await vi.advanceTimersByTimeAsync(250)
  expect(await action).toMatchObject({ outcome: 'applied' })
  const after = h.forwarded.slice(mark + 1).map(f => f.type)
  expect(after).toContain('result')
  expect(after.some(type => type === 'state' || type === 'heartbeat')).toBe(true)
  expect(Date.now() - sentAt).toBeGreaterThan(CAT_MS)
  // No event ever reached a v4 page - the station pushes only for a control sent at v5.
  expect(h.errors).toEqual([])
})

it('a v5 page on a v4 station keeps the old path exactly: the relay negotiates 4, the page re-reads at once', async () => {
  // The rollout pair - the site updated, the desktop not yet. The station never names `outcomePush`,
  // so the page must not wait for a push that will never come: the state re-read goes out on the
  // very next tick after the control's reply, as it always did, and the outcome comes by `result`.
  const h = connected({ page: 5, station: 4, push: true })
  await controlling(h)
  expect(h.forwarded.every(f => f.version === 4)).toBe(true)
  expect(h.client.getSnapshot().state?.controls?.capabilities).not.toContain('outcomePush')
  const sentAt = Date.now(), mark = h.forwarded.length
  const action = h.client.control(tune)
  await vi.advanceTimersByTimeAsync(0)
  expect(h.forwarded.slice(mark).map(f => f.type)).toEqual(['stationControl'])
  // The reply (pending) has landed. Within one tick the page asks for the state again - the old,
  // immediate re-read - not a second later.
  await vi.advanceTimersByTimeAsync(250)
  expect(h.forwarded.slice(mark).map(f => f.type)).toEqual(['stationControl', 'heartbeat'])
  let resolved = false
  void action.then(() => { resolved = true })
  for (let ticks = 0; ticks < 20 && !resolved; ticks++) await vi.advanceTimersByTimeAsync(250)
  expect(await action).toMatchObject({ outcome: 'applied' })
  expect(h.forwarded.slice(mark).map(f => f.type)).toContain('result')
  expect(Date.now() - sentAt).toBeLessThanOrEqual(1500)
  expect(h.client.getSnapshot()).toMatchObject({ fresh: true, controlPending: null })
  expect(h.errors).toEqual([])
})

it('v5: a lost event strands nothing - the outcome and a fresh state still arrive by the old polls', async () => {
  const h = connected({ page: 5, station: 5, push: false })
  await controlling(h)
  const sentAt = Date.now(), mark = h.forwarded.length
  const action = h.client.control(tune)
  await vi.advanceTimersByTimeAsync(CAT_MS)
  let resolved = false
  void action.then(() => { resolved = true })
  for (let ticks = 0; ticks < 20 && !resolved; ticks++) await vi.advanceTimersByTimeAsync(250)
  expect(await action).toMatchObject({ outcome: 'applied' })
  expect(Date.now() - sentAt).toBeLessThanOrEqual(2500)
  const after = h.forwarded.slice(mark + 1).map(f => f.type)
  expect(after).toContain('result')
  for (let ticks = 0; ticks < 8 && !h.client.getSnapshot().fresh; ticks++) await vi.advanceTimersByTimeAsync(250)
  expect(h.client.getSnapshot()).toMatchObject({ fresh: true, controlPending: null })
  expect(h.client.getSnapshot().state?.commandWindowId).toBe(h.state.commandWindowId)
  expect(h.errors).toEqual([])
})

it('v5: an event and a poll never disagree - the newer station revision wins whichever lane brought it', async () => {
  const h = connected({ page: 5, station: 5, push: true })
  await controlling(h)
  const held = h.client.getSnapshot().state!
  const event = (revision: number, commandWindowId: string) => {
    const operationId = crypto.randomUUID()
    return { type: 'operationEvent', operationId, value: { operation: 'stationControl', operationId, outcome: 'applied', evidence: 'radioReadback' },
      state: { ...structuredClone(held), revision, commandWindowId } }
  }
  // An event computed BEFORE the state the browser holds (one revision behind) is discarded ...
  h.client.receiveEvent(event(held.revision - 1, crypto.randomUUID()))
  expect(h.client.getSnapshot().state).toEqual(held)
  // ... an equal revision is the later word on the same state, and a newer one is newer.
  const same = crypto.randomUUID(), newer = crypto.randomUUID()
  h.client.receiveEvent(event(held.revision, same))
  expect(h.client.getSnapshot().state?.commandWindowId).toBe(same)
  h.client.receiveEvent(event(held.revision + 1, newer))
  expect(h.client.getSnapshot()).toMatchObject({ fresh: true, state: { revision: held.revision + 1, commandWindowId: newer } })
  // The poll lane obeys the same rule: a heartbeat reply carrying an OLDER revision than the event
  // installed (computed before it, delivered after) is discarded, and the browser keeps the
  // event's window; once the station's own revision is ahead again, the reply is installed.
  const old = crypto.randomUUID()
  Object.assign(h.state, { revision: held.revision, commandWindowId: old })
  const heartbeats = () => h.forwarded.filter(f => f.type === 'heartbeat').length
  for (let n = heartbeats(); heartbeats() === n;) await vi.advanceTimersByTimeAsync(250)
  await vi.advanceTimersByTimeAsync(0)
  expect(h.client.getSnapshot()).toMatchObject({ busy: false, state: { revision: held.revision + 1, commandWindowId: newer } })
  const current = crypto.randomUUID()
  Object.assign(h.state, { revision: held.revision + 2, commandWindowId: current })
  for (let n = heartbeats(); heartbeats() === n;) await vi.advanceTimersByTimeAsync(250)
  await vi.advanceTimersByTimeAsync(0)
  expect(h.client.getSnapshot()).toMatchObject({ fresh: true, state: { revision: held.revision + 2, commandWindowId: current } })
  expect(h.errors).toEqual([])
})

it('an event is refused by a page that never negotiated v5, and by the relay for a station that did not advertise it', () => {
  const state: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'available', leaseId: null,
    revision: 1, commandWindowId: null, nextSequence: null, leaseRemainingMs: null, actions: [], txArmed: false
  }
  const operationId = crypto.randomUUID()
  const event = { type: 'operationEvent', operationId, value: { operation: 'stationControl', operationId, outcome: 'applied', evidence: 'radioReadback' }, state }
  const page = new OperationClient(() => {}, true, Date.now, undefined, 4)
  expect(() => page.receiveEvent(event)).toThrow('invalidOperation')
  const peer = () => ({ frames: [] as unknown[], send(s: string) { this.frames.push(JSON.parse(s)) }, close: vi.fn() })
  for (const stationVersion of [4, 5] as const) {
    const relay = new OperationRelay(), station = peer(), browser = peer(), sessionId = crypto.randomUUID()
    relay.sync({ peer: station, supported: true, operationVersion: stationVersion }, [{ peer: browser, sessionId, deviceId: crypto.randomUUID(), commandUntil: Number.MAX_SAFE_INTEGER }], 1000)
    relay.receiveStation({ sessionId, ...event })
    if (stationVersion === 5) {
      expect(station.close).not.toHaveBeenCalled()
      expect(browser.frames).toEqual([event])
    } else {
      expect(station.close).toHaveBeenCalledWith(1008, 'invalidOperation')
      expect(browser.frames).toEqual([])
    }
  }
  // Malformed - a pending "event", or an outcome for a different operation - closes the station.
  for (const bad of [
    { ...event, value: { operation: 'stationControl', operationId, outcome: 'pending' } },
    { ...event, value: { ...event.value, operationId: crypto.randomUUID() } },
    { ...event, state: { ...state, revision: -1 } },
    { ...event, extra: true }
  ]) {
    const relay = new OperationRelay(), station = peer(), browser = peer(), sessionId = crypto.randomUUID()
    relay.sync({ peer: station, supported: true, operationVersion: 5 }, [{ peer: browser, sessionId, deviceId: crypto.randomUUID(), commandUntil: Number.MAX_SAFE_INTEGER }], 1000)
    relay.receiveStation({ sessionId, ...bad })
    expect(station.close).toHaveBeenCalledWith(1008, 'invalidOperation')
    expect(browser.frames).toEqual([])
  }
})
