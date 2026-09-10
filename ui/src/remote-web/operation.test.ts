import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import { manualRecord, operationRequest, operationValue } from './operation-protocol'
import type { OperationState } from './operation-protocol'
const id = () => crypto.randomUUID()
const record = () => ({
  call: 'W1AW',
  grid: 'FN31',
  country: null,
  state: null,
  band: '20m',
  freqMhz: 14.25,
  mode: 'SSB',
  rstSent: '59',
  rstRcvd: '57',
  name: null,
  qth: null,
  comment: null,
  notes: 'Keep this contact',
  whenUnix: 1700000000,
  confirmed: false as const,
  awardConfirmed: false as const
})
const state = (controlling = false): OperationState => ({
  stationBootId: id(),
  allowed: true,
  phase: controlling ? 'controlling' : 'available',
  leaseId: controlling ? id() : null,
  revision: 1,
  commandWindowId: controlling ? id() : null,
  nextSequence: controlling ? 1 : null,
  leaseRemainingMs: controlling ? 5000 : null,
  actions: ['log.manual'],
  txArmed: false
})
afterEach(() => vi.useRealTimers())
it('closes the logging grammar around plain QSO fields and no arbitrary action', () => {
  expect(manualRecord(record())).toEqual(record())
  for (const change of [
    { call: 'W1AW\n' },
    { confirmed: true },
    { awardConfirmed: true },
    { freqMhz: NaN },
    { notes: 'x'.repeat(1025) },
    { upload: 'lotw' },
    { stationCall: 'K2ABC' },
    { ota: { theirProgram: 'POTA', theirRef: '../../file' } }
  ])
    expect(() => manualRecord({ ...record(), ...change })).toThrow()
  for (const type of ['set_frequency', 'set_settings', 'arm_tx', 'invoke', 'log_qso'])
    expect(() => operationRequest({ type, requestId: id() })).toThrow()
  const s = state(true)
  expect(operationValue(s)).toEqual(s)
  for (const change of [
    { txArmed: true },
    { leaseRemainingMs: 5001 },
    { phase: 'available' },
    { actions: ['tx'] }
  ])
    expect(() => operationValue({ ...s, ...change })).toThrow()
})
function setup() {
  vi.useFakeTimers()
  const sent: Record<string, any>[] = [],
    saved: { id: string | null } = { id: null }
  let now = 1000
  const c = new OperationClient(
    (s) => sent.push(JSON.parse(s)),
    true,
    () => now,
    {
      read: () => saved.id,
      write: (id) => {
        saved.id = id
      }
    }
  )
  c.open()
  const reply = (value: unknown) =>
    c.receive({
      type: 'operationResponse',
      requestId: sent[sent.length - 1]!.request.requestId,
      value
    })
  reply(state())
  return {
    c,
    sent,
    saved,
    reply,
    advance: (ms: number) => {
      now += ms
    }
  }
}
it('requires an explicit acquisition and rejects overlap while preserving the operation id', async () => {
  const { c, sent, saved, reply } = setup()
  await expect(c.log(record())).rejects.toThrow('notController')
  expect(sent.map((m) => m.request.type)).toEqual(['state'])
  const acquiring = c.acquire()
  reply(state(true))
  await acquiring
  const result = c.log(record()),
    request = sent[sent.length - 1]!.request
  expect(saved.id).toBe(request.requestId)
  await expect(c.log(record())).rejects.toThrow('operationUnknown')
  expect(sent.filter((m) => m.request.type === 'logManual')).toHaveLength(1)
  reply({
    outcome: 'applied',
    evidence: 'fileSynced',
    uploads: 'stationPipeline',
    operationId: request.requestId
  })
  expect((await result).outcome).toBe('applied')
  expect(saved.id).toBeNull()
  c.disconnected()
})
it('keeps an unknown receipt across disconnect and a fresh client, never resubmitting', async () => {
  const { c, sent, saved, reply } = setup()
  const a = c.acquire()
  reply(state(true))
  await a
  const action = c.log(record()).catch((e) => e.message),
    operationId = sent[sent.length - 1]!.request.requestId
  c.disconnected()
  expect(await action).toBe('operationUnknown')
  expect(saved.id).toBe(operationId)
  const nextSent: Record<string, any>[] = [],
    next = new OperationClient(
      (s) => nextSent.push(JSON.parse(s)),
      true,
      () => 2000,
      {
        read: () => saved.id,
        write: (id) => {
          saved.id = id
        }
      }
    )
  next.open()
  next.receive({
    type: 'operationResponse',
    requestId: nextSent[nextSent.length - 1]!.request.requestId,
    value: state()
  })
  await expect(next.log(record())).rejects.toThrow('operationUnknown')
  const check = next.resolve()
  expect(nextSent[nextSent.length - 1]!.request).toMatchObject({ type: 'result', operationId })
  next.receive({
    type: 'operationResponse',
    requestId: nextSent[nextSent.length - 1]!.request.requestId,
    value: { outcome: 'applied', evidence: 'fileSynced', uploads: 'stationPipeline', operationId }
  })
  expect((await check).outcome).toBe('applied')
  expect(saved.id).toBeNull()
  expect(nextSent.some((m) => m.request.type === 'logManual')).toBe(false)
  next.disconnected()
})
it('expires the browser command window and makes a timed-out write uncertain', async () => {
  const { c, sent, reply, advance } = setup()
  const a = c.acquire()
  reply(state(true))
  await a
  advance(1300)
  await vi.advanceTimersByTimeAsync(250)
  await expect(c.log(record())).rejects.toThrow('notController')
  // A new explicit station state makes a later action available, not a retry.
  reply(state(true))
  const action = c.log(record()).catch((e) => e.message)
  const operationId = sent[sent.length - 1]!.request.requestId
  await vi.advanceTimersByTimeAsync(7500)
  expect(await action).toBe('operationUnknown')
  expect(c.getSnapshot().unresolved).toBe(operationId)
  expect(sent.filter((m) => m.request.type === 'logManual')).toHaveLength(1)
  c.disconnected()
})
it('refuses to send a mutation when its receipt id cannot be retained', async () => {
  vi.useFakeTimers()
  const sent: Record<string, any>[] = [],
    c = new OperationClient(
      (s) => sent.push(JSON.parse(s)),
      true,
      () => 1000,
      {
        read: () => null,
        write: () => {
          throw Error('storage denied')
        }
      }
    )
  c.open()
  c.receive({
    type: 'operationResponse',
    requestId: sent[sent.length - 1]!.request.requestId,
    value: state(true)
  })
  await expect(c.log(record())).rejects.toThrow('receiptStorageUnavailable')
  expect(sent.some((m) => m.request.type === 'logManual')).toBe(false)
  c.disconnected()
})
function relaySetup() {
  const station = { send: vi.fn<(s: string) => void>(), close: vi.fn() },
    browser = { send: vi.fn<(s: string) => void>(), close: vi.fn() },
    sessionId = id(),
    deviceId = id(),
    relay = new OperationRelay()
  const peers = [{ sessionId, deviceId, peer: browser }]
  relay.sync({ peer: station, supported: true }, peers, 100)
  return { station, browser, sessionId, deviceId, relay, peers }
}
it('routes only an admitted device, replaces client identity and survives hibernation without QSO data', () => {
  const { station, browser, sessionId, deviceId, relay, peers } = relaySetup()
  const request = {
    type: 'logManual',
    requestId: id(),
    stationBootId: id(),
    leaseId: id(),
    expectedRevision: 1,
    commandWindowId: id(),
    clientSequence: 1,
    record: record()
  }
  relay.receiveBrowser(sessionId, { type: 'operationRequest', request }, 101)
  expect(JSON.parse(station.send.mock.lastCall![0])).toEqual({
    type: 'operationRequest',
    sessionId,
    deviceId,
    request
  })
  const saved = relay.checkpoint(sessionId)
  expect(JSON.stringify(saved)).not.toMatch(/W1AW|Keep this contact|leaseId|stationBootId/)
  expect(JSON.stringify(saved).length).toBeLessThan(200)
  const next = new OperationRelay()
  next.sync({ peer: station, supported: true }, peers, 102)
  next.restore(sessionId, saved)
  next.receiveStation({
    type: 'operationResponse',
    requestId: request.requestId,
    sessionId,
    value: {
      outcome: 'applied',
      evidence: 'fileSynced',
      uploads: 'stationPipeline',
      operationId: request.requestId
    }
  })
  expect(JSON.parse(browser.send.mock.lastCall![0]).value.outcome).toBe('applied')
  expect(next.checkpoint(sessionId).pending).toEqual([])
  relay.receiveBrowser(sessionId, { type: 'operationRequest', request, deviceId: id() }, 103)
  expect(browser.close).toHaveBeenCalledWith(1008, 'invalidOperation')
})
it('makes interrupted writes unknown, never forwards pending actions to a replacement station', () => {
  const { station, browser, sessionId, relay, peers } = relaySetup()
  const request = {
    type: 'logManual',
    requestId: id(),
    stationBootId: id(),
    leaseId: id(),
    expectedRevision: 1,
    commandWindowId: id(),
    clientSequence: 1,
    record: record()
  }
  relay.receiveBrowser(sessionId, { type: 'operationRequest', request }, 101)
  const replacement = { send: vi.fn(), close: vi.fn() }
  relay.sync({ peer: replacement, supported: true }, peers, 102)
  expect(JSON.parse(browser.send.mock.lastCall![0]).error).toBe('operationUnknown')
  expect(replacement.send).not.toHaveBeenCalled()
  relay.sync({ peer: replacement, supported: true }, [], 103)
  expect(JSON.parse(replacement.send.mock.lastCall![0]).type).toBe('operationDisconnect')
  expect(station.close).not.toHaveBeenCalled()
})
it('bounds pending requests, expires outcomes and refuses unsupported stations', () => {
  const { station, browser, sessionId, relay, peers } = relaySetup()
  for (let i = 0; i < 3; i++)
    relay.receiveBrowser(
      sessionId,
      { type: 'operationRequest', request: { type: 'state', requestId: id() } },
      101
    )
  expect(station.send).toHaveBeenCalledTimes(2)
  expect(JSON.parse(browser.send.mock.lastCall![0]).error).toBe('remoteBusy')
  relay.expire(7101)
  expect(relay.nextDeadline()).toBeNull()
  relay.sync({ peer: station, supported: false }, peers, 8000)
  relay.receiveBrowser(
    sessionId,
    { type: 'operationRequest', request: { type: 'state', requestId: id() } },
    8001
  )
  expect(JSON.parse(browser.send.mock.lastCall![0]).error).toBe('stationUnsupported')
})

it('coordinates a click with its in-flight heartbeat without changing context or retrying', async () => {
  const { c, sent, reply, advance } = setup()
  const a = c.acquire(),
    owned = state(true)
  reply(owned)
  await a
  advance(1000)
  await vi.advanceTimersByTimeAsync(250)
  expect(sent[sent.length - 1]!.request.type).toBe('heartbeat')
  const action = c.log(record())
  await expect(c.log(record())).rejects.toThrow('remoteBusy')
  expect(sent.filter((m) => m.request.type === 'logManual')).toHaveLength(0)
  reply({ ...owned, commandWindowId: id() })
  await Promise.resolve()
  const request = sent[sent.length - 1]!.request
  expect(request.type).toBe('logManual')
  expect(request.commandWindowId).toBe(owned.commandWindowId)
  reply({
    outcome: 'applied',
    evidence: 'fileSynced',
    uploads: 'stationPipeline',
    operationId: request.requestId
  })
  expect((await action).outcome).toBe('applied')
  c.disconnected()
})
it('refuses a heartbeat wait that crosses a native context change or the click deadline', async () => {
  for (const change of ['context', 'deadline']) {
    const { c, sent, reply, advance } = setup()
    const a = c.acquire(),
      owned = state(true)
    reply(owned)
    await a
    advance(1000)
    await vi.advanceTimersByTimeAsync(250)
    const action = c.log(record()).catch((e) => e.message)
    if (change === 'context') reply({ ...owned, revision: owned.revision + 1 })
    else {
      advance(201)
      await vi.advanceTimersByTimeAsync(201)
    }
    expect(await action).toBe(change === 'context' ? 'staleContext' : 'windowExpired')
    expect(sent.some((m) => m.request.type === 'logManual')).toBe(false)
    c.disconnected()
  }
})

it('finishes revocation when disconnected peers can no longer accept close notifications', () => {
  const { station, browser, sessionId, relay, peers } = relaySetup()
  relay.receiveBrowser(
    sessionId,
    { type: 'operationRequest', request: { type: 'state', requestId: id() } },
    101
  )
  station.send.mockImplementation(() => {
    throw Error('closed')
  })
  browser.send.mockImplementation(() => {
    throw Error('closed')
  })
  expect(() => relay.sync(null, [], 102)).not.toThrow()
  expect(relay.nextDeadline()).toBeNull()
  relay.sync({ peer: station, supported: true }, peers, 103)
  expect(() => relay.sync({ peer: station, supported: true }, [], 104)).not.toThrow()
  expect(station.close).toHaveBeenCalledWith(1011, 'stationUnavailable')
})
