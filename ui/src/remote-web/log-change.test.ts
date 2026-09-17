import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import { logChange, logChangeCapabilities, logChangeCapability, logRowCanonical, logTarget, operationRequest, operationValue, type OperationState } from './operation-protocol'

const id = () => crypto.randomUUID()
const target = { call: 'W1AW', whenUnix: 1788940800, key: 'a'.repeat(64) }
const record = { call: 'W1AW', grid: 'FN31', country: null, state: null, band: '20m', freqMhz: 14.25, mode: 'SSB',
  rstSent: '59', rstRcvd: '57', name: null, qth: null, comment: null, notes: null, whenUnix: 1788940800,
  confirmed: false as const, awardConfirmed: false as const }
const envelope = (change: unknown) => ({ type: 'logChange', requestId: id(), stationBootId: id(), leaseId: id(),
  expectedRevision: 1, commandWindowId: id(), clientSequence: 1, change })
const state = (capabilities: string[]): OperationState => ({ stationBootId: id(), allowed: true, phase: 'controlling',
  leaseId: id(), revision: 1, commandWindowId: id(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'],
  txArmed: false, transmitEpoch: null,
  controls: { context: { radioId: 1, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities } as OperationState['controls'] })
afterEach(() => vi.useRealTimers())

it('closes the log change grammar around a row key and a complete edit', () => {
  expect(logChange({ kind: 'delete', target })).toEqual({ kind: 'delete', target })
  expect(logChange({ kind: 'edit', target, record })).toEqual({ kind: 'edit', target, record })
  expect(operationRequest(envelope({ kind: 'delete', target })).type).toBe('logChange')
  for (const bad of [
    { kind: 'purge', target },
    { kind: 'toString', target },
    { kind: 'delete', target, index: 3 },
    { kind: 'delete', target: { ...target, key: 'A'.repeat(64) } },
    { kind: 'delete', target: { ...target, key: 'a'.repeat(63) } },
    { kind: 'delete', target: { ...target, index: 0 } },
    { kind: 'delete', target: { ...target, call: '' } },
    { kind: 'edit', target, record: { ...record, whenUnix: null } },
    { kind: 'edit', target, record: { ...record, confirmed: true } },
    { kind: 'edit', target }
  ])
    expect(() => operationRequest(envelope(bad))).toThrow()
  // The park program is whatever the log holds — WWFF is stored verbatim from ADIF SIG — and is
  // never narrowed to POTA/SOTA; lower case, empty and free text are still refused.
  for (const theirProgram of ['POTA', 'SOTA', 'WWFF'])
    expect(logChange({ kind: 'edit', target, record: { ...record, ota: { theirProgram, theirRef: 'DLFF-0001' } } })).toBeTruthy()
  for (const theirProgram of ['pota', '', 'W', 'A'.repeat(17), 'WW FF', 'WWFF-'])
    expect(() => logChange({ kind: 'edit', target, record: { ...record, ota: { theirProgram, theirRef: 'DLFF-0001' } } })).toThrow()
})

it('marks a QSL sent only as B, D, E or a withdrawal, and a card only as received or not', () => {
  for (const change of [
    { kind: 'qslSent', target, via: 'B' }, { kind: 'qslSent', target, via: 'E' }, { kind: 'qslSent', target, via: null },
    { kind: 'qslCard', target, received: true }, { kind: 'qslCard', target, received: false }
  ])
    expect(logChange(change)).toEqual(change)
  // The empty string is the menu's placeholder, never a withdrawal; only null withdraws.
  for (const bad of [
    { kind: 'qslSent', target, via: '' }, { kind: 'qslSent', target, via: 'b' }, { kind: 'qslSent', target, via: 'X' },
    { kind: 'qslSent', target }, { kind: 'qslCard', target, received: 'yes' }, { kind: 'qslCard', target },
    { kind: 'qslCard', target, received: true, via: 'B' }
  ])
    expect(() => logChange(bad)).toThrow()
  expect(logChangeCapability({ kind: 'qslCard', target, received: true })).toBe('qslMarks')
  expect(logChangeCapability({ kind: 'qslSent', target, via: null })).toBe('qslMarks')
})

it('hunts a POTA or SOTA activator with no row target, and reports station state', () => {
  const hunt = { kind: 'hunt', call: 'W1AW', program: 'POTA', reference: 'US-0002' }
  for (const change of [hunt, { ...hunt, program: 'SOTA', reference: 'W7A/MN-001' }, { kind: 'clearHunt' }])
    expect(logChange(change)).toEqual(change)
  for (const bad of [
    { ...hunt, call: 'w1aw' }, { ...hunt, call: 'W1' }, { ...hunt, program: 'WWFF' }, { ...hunt, reference: 'US 0002' },
    { ...hunt, reference: '' }, { ...hunt, target }, { kind: 'hunt', call: 'W1AW', reference: 'US-0002' },
    { kind: 'clearHunt', target }, { kind: 'clearHunt', call: 'W1AW' }
  ])
    expect(() => logChange(bad)).toThrow()
  expect(logChangeCapability({ kind: 'hunt', call: 'W1AW', program: 'POTA', reference: 'US-0002' })).toBe('otaHunt')
  expect(logChangeCapability({ kind: 'clearHunt' })).toBe('otaHunt')
  const operationId = id()
  for (const value of [
    { operation: 'logChange', operationId, outcome: 'applied', evidence: 'stationState' },
    { operation: 'logChange', operationId, outcome: 'rejected', reason: 'invalidChange' }
  ])
    expect(operationValue(value)).toEqual(value)
})

it('starts or ends your own activation with no row target', () => {
  const start = { kind: 'activation', program: 'POTA', reference: 'US-0005' }
  for (const change of [start, { ...start, program: 'SOTA', reference: 'W7A/MN-001' }, { kind: 'clearActivation' }])
    expect(logChange(change)).toEqual(change)
  for (const bad of [
    { ...start, program: 'WWFF' }, { ...start, reference: 'US 0005' }, { ...start, reference: '' }, { ...start, target },
    { ...start, call: 'W1AW' }, { kind: 'activation', reference: 'US-0005' }, { kind: 'clearActivation', target },
    { kind: 'clearActivation', reference: 'US-0005' }
  ])
    expect(() => logChange(bad)).toThrow()
  expect(logChangeCapability({ kind: 'activation', program: 'POTA', reference: 'US-0005' })).toBe('otaActivation')
  expect(logChangeCapability({ kind: 'clearActivation' })).toBe('otaActivation')
})

it('self-spots only with the activation reference and dial the operator confirmed', () => {
  const spot = { kind: 'selfSpot', reference: 'US-0001', dialHz: 14_285_000 }
  expect(logChange(spot)).toEqual(spot)
  for (const bad of [
    { ...spot, dialHz: 0 }, { ...spot, dialHz: 14.285 }, { ...spot, dialHz: 250_000_000_001 }, { ...spot, reference: 'US 0001' },
    { ...spot, call: 'W1AW' }, { ...spot, comment: 'QRV' }, { kind: 'selfSpot', reference: 'US-0001' }, { ...spot, target }
  ])
    expect(() => logChange(bad)).toThrow()
  expect(logChangeCapability(spot as never)).toBe('selfSpot')
  const operationId = id()
  // The receipt names what each target did, so one failure never hides the other.
  const posted = { operation: 'logChange', operationId, outcome: 'applied', evidence: 'spotPosted', spot: { pota: 'posted', cluster: 'queued' } }
  const refused = { operation: 'logChange', operationId, outcome: 'rejected', reason: 'spotNotPosted', spot: { pota: 'loginRequired', cluster: 'unavailable' } }
  for (const value of [posted, refused]) expect(operationValue(value)).toEqual(value)
  for (const value of [
    { ...posted, spot: undefined }, { operation: 'logChange', operationId, outcome: 'applied', evidence: 'spotPosted' },
    { ...posted, spot: { pota: 'posted' } }, { ...posted, spot: { pota: 'posted', cluster: 'queued', extra: 1 } },
    { ...posted, spot: { pota: 'maybe', cluster: 'queued' } }, { ...refused, spot: { pota: 'failed', cluster: 'sent' } },
    { ...posted, evidence: 'fileSynced' }, { ...refused, reason: 'contextChanged' },
    { operation: 'logChange', operationId, outcome: 'applied', evidence: 'spotQueued' },
    // `clusterUnavailable` belongs to a spot of ANOTHER station and carries no per-target report;
    // a self-spot's two results never ride it.
    { operation: 'logChange', operationId, outcome: 'rejected', reason: 'clusterUnavailable', spot: { pota: 'posted', cluster: 'queued' } }
  ])
    expect(() => operationValue(value)).toThrow()
})

it('spots another station under station control, with nothing read from the log', () => {
  const spot = { kind: 'spot', call: 'JA2DEF/P', freqMhz: 14.0765, comment: 'FT8 up 2' }
  expect(logChange(spot)).toEqual(spot)
  expect(logChange({ ...spot, comment: '' })).toBeTruthy()
  for (const bad of [
    { ...spot, call: 'ja2def' }, { ...spot, call: 'W1' }, { ...spot, call: 'A'.repeat(33) },
    { ...spot, freqMhz: 0 }, { ...spot, freqMhz: -14 }, { ...spot, freqMhz: 250_001 }, { ...spot, freqMhz: '14.0765' },
    { ...spot, comment: 'x'.repeat(31) }, { ...spot, comment: 'up\n2' },
    { kind: 'spot', call: 'JA2DEF', freqMhz: 14.0765 }, { ...spot, target }, { ...spot, reference: 'US-0001' }
  ])
    expect(() => logChange(bad)).toThrow()
  // It posts publicly from the station's cluster login, so it is station control, not logging.
  expect(logChangeCapability(spot as never)).toBe('postSpot')
  expect(logChangeCapabilities(spot as never)).toEqual(['postSpot'])
  const operationId = id()
  const queued = { operation: 'logChange', operationId, outcome: 'applied', evidence: 'clusterQueued' }
  const noNode = { operation: 'logChange', operationId, outcome: 'rejected', reason: 'clusterUnavailable' }
  for (const value of [queued, noNode]) expect(operationValue(value)).toEqual(value)
  for (const value of [
    { ...queued, spot: { pota: 'posted', cluster: 'queued' } },
    { ...queued, evidence: 'clusterPosted' }, { ...noNode, reason: 'noCluster' }
  ])
    expect(() => operationValue(value)).toThrow()
})

it('accepts only the bounded change outcomes a station reports', () => {
  const operationId = id()
  for (const value of [
    { operation: 'logChange', operationId, outcome: 'applied', evidence: 'fileSynced' },
    { operation: 'logChange', operationId, outcome: 'rejected', reason: 'contextChanged' },
    { operation: 'logChange', operationId, outcome: 'unknown', reason: 'persistenceUnconfirmed' }
  ])
    expect(operationValue(value)).toEqual(value)
  for (const value of [
    { operation: 'logChange', operationId, outcome: 'applied', evidence: 'radioReadback' },
    { operation: 'logChange', operationId, outcome: 'unknown', reason: 'contextChanged' },
    { operation: 'logChange', operationId, outcome: 'pending' },
    { operation: 'logChange', operationId, outcome: 'applied', evidence: 'fileSynced', index: 2 }
  ])
    expect(() => operationValue(value)).toThrow()
})

// The station finds a row again by this digest. Both sides must build the same bytes from the same
// JSON row; src-tauri's logging tests assert this exact string and digest (the cross-language pair).
const VECTOR = '{"call":"W1AW","freqMhz":14.074,"txPower":-0.5,"whenUnix":1788940800,"name":"José 日本","qth":"a\\"b}","grid":null,"confirmed":false,"ota":{"theirRef":"US-0001","iota":null},"credit":["DXCC",7],"big":12345678901234567}'
it('keys a row from its exact values, independent of key order', async () => {
  const row = JSON.parse(VECTOR)
  expect(logRowCanonical(row)).toBe('o11{s3:bign12345678901234567741440;s4:calls4:W1AWs9:confirmedfs6:credita2[s4:DXCCn7000000;]s7:freqMhzn14074000;s4:gridzs4:names12:José 日本s3:otao2{s4:iotazs8:theirRefs7:US-0001}s3:qths4:a"b}s7:txPowern-500000;s8:whenUnixn1788940800000000;}')
  const keyed = await logTarget(row)
  expect(keyed).toEqual({ call: 'W1AW', whenUnix: 1788940800, key: 'd33eff984a6c2d66e74a3a7fcb19d04b318508cd7cc6dcfcdb77d80ea6121fb3' })
  const reordered = Object.fromEntries(Object.entries(row).reverse()) as typeof row
  expect((await logTarget(reordered)).key).toBe(keyed.key)
  expect((await logTarget({ ...row, notes: 'changed' })).key).not.toBe(keyed.key)
  // An undefined property is absent, as JSON.stringify would send it — the same key, never a throw.
  expect((await logTarget({ ...row, upload: undefined })).key).toBe(keyed.key)
})

it('relays a change only as a versioned v4 mutation', () => {
  const station = { send: vi.fn<(s: string) => void>(), close: vi.fn() }
  const browser = { send: vi.fn<(s: string) => void>(), close: vi.fn() }
  const sessionId = id(), deviceId = id(), relay = new OperationRelay()
  relay.sync({ peer: station, supported: true, operationVersion: 4 }, [{ sessionId, deviceId, commandUntil: Number.MAX_SAFE_INTEGER, peer: browser }], 100)
  const request = envelope({ kind: 'delete', target })
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 3, request }, 101)
  expect(JSON.parse(browser.send.mock.lastCall![0]).error).toBe('stationUnsupported')
  expect(station.send).not.toHaveBeenCalled()
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4, request }, 102)
  expect(JSON.parse(station.send.mock.lastCall![0])).toMatchObject({ type: 'operationRequest', operationVersion: 4, request })
  // One write at a time: a second change cannot pass one still in flight.
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4, request: envelope({ kind: 'delete', target }) }, 103)
  expect(JSON.parse(browser.send.mock.lastCall![0]).error).toBe('remoteBusy')
  const value = { operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'fileSynced' }
  relay.receiveStation({ type: 'operationResponse', sessionId, requestId: request.requestId, value })
  expect(JSON.parse(browser.send.mock.lastCall![0]).value).toEqual(value)
  // An older station negotiates v3 and never receives a request it cannot parse.
  relay.sync({ peer: station, supported: true, operationVersion: 3 }, [{ sessionId, deviceId, commandUntil: Number.MAX_SAFE_INTEGER, peer: browser }], 200)
  station.send.mockClear()
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4, request: envelope({ kind: 'delete', target }) }, 201)
  expect(JSON.parse(browser.send.mock.lastCall![0]).error).toBe('stationUnsupported')
  expect(station.send).not.toHaveBeenCalled()
  // Positive control: an unversioned change is a malformed peer.
  relay.receiveBrowser(sessionId, { type: 'operationRequest', request: envelope({ kind: 'delete', target }) }, 202)
  expect(browser.close).toHaveBeenCalledWith(1008, 'invalidOperation')
})

function client(version: 3 | 4 = 4) {
  vi.useFakeTimers()
  const sent: Record<string, any>[] = [], saved: { id: string | null } = { id: null }
  let now = 1000
  const c = new OperationClient(s => sent.push(JSON.parse(s)), true, () => now,
    { read: () => saved.id, write: id => { saved.id = id } }, version)
  c.open()
  const reply = (value: unknown) => c.receive({ type: 'operationResponse', requestId: sent[sent.length - 1]!.request.requestId, value })
  const poll = async (value: unknown) => { now += 1000; await vi.advanceTimersByTimeAsync(250); reply(value) }
  return { c, sent, saved, reply, poll }
}

it('sends a change only with the station capability and keeps its receipt until checked', async () => {
  const { c, sent, saved, reply, poll } = client()
  reply(state(['qsoLogging']))
  await expect(c.change({ kind: 'delete', target })).rejects.toThrow('notController')
  expect(sent.filter(m => m.request.type === 'logChange')).toHaveLength(0)
  await poll(state(['qsoLogging', 'logEdit']))
  const result = c.change({ kind: 'delete', target }), request = sent[sent.length - 1]!.request
  expect(request).toMatchObject({ type: 'logChange', change: { kind: 'delete', target } })
  expect(saved.id).toBe(request.requestId)
  await expect(c.change({ kind: 'delete', target })).rejects.toThrow('operationUnknown')
  await expect(c.log({ ...record, whenUnix: null })).rejects.toThrow('operationUnknown')
  reply({ operation: 'logChange', operationId: request.requestId, outcome: 'unknown', reason: 'persistenceUnconfirmed' })
  expect((await result).outcome).toBe('unknown')
  expect(c.getSnapshot().unresolved).toBe(request.requestId)
  expect(c.getSnapshot().pendingDraft).toBeNull()
  const checked = c.resolve(), check = sent[sent.length - 1]!.request
  expect(check).toMatchObject({ type: 'result', operationId: request.requestId })
  reply({ operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'fileSynced' })
  expect((await checked).outcome).toBe('applied')
  expect(saved.id).toBeNull()
  expect(sent.filter(m => m.request.type === 'logChange')).toHaveLength(1)
  c.disconnected()
})

it('never sends a change to a station older than v4, and a dropped reply stays unknown', async () => {
  const old = client(3)
  old.reply(state(['logEdit']))
  await expect(old.c.change({ kind: 'delete', target })).rejects.toThrow('stationUnsupported')
  expect(old.sent.filter(m => m.request.type === 'logChange')).toHaveLength(0)
  old.c.disconnected()
  vi.useRealTimers()
  const { c, sent, saved, reply } = client()
  reply(state(['logEdit']))
  const result = c.change({ kind: 'edit', target, record }).catch(e => e.message)
  const request = sent[sent.length - 1]!.request
  c.disconnected()
  expect(await result).toBe('operationUnknown')
  expect(saved.id).toBe(request.requestId)
})
