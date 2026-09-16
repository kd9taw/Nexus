import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import {
  ACTIVATION_EXPORT_CHUNK_BYTES,
  ACTIVATION_EXPORT_MAX_BYTES,
  LOG_CAPABILITIES,
  OPERATION_EXPORT_RESPONSE_BYTES,
  activationSelection,
  operationRequest,
  operationValue,
  type OperationState
} from './operation-protocol'
import { downloadActivation } from './activation-export'

const id = () => crypto.randomUUID()
/** 2026-09-09 00:00:00 UTC. */
const DAY = 1788912000
const selection = { reference: 'US-1234', dayStartUnix: DAY, callsign: 'W9XYZ' }
const entry = { program: 'POTA', reference: 'US-1234', dayStartUnix: DAY, date: '2026-09-09', callsign: 'W9XYZ', qsos: 12 }
const hex = (b: ArrayBuffer) => [...new Uint8Array(b)].map(x => x.toString(16).padStart(2, '0')).join('')
const exportRequest = (over: Record<string, unknown> = {}) =>
  ({ type: 'activationExport', requestId: id(), stationBootId: id(), leaseId: id(), selection, index: 0, ...over })
const state = (capabilities: string[]): OperationState => ({ stationBootId: id(), allowed: true, phase: 'controlling', leaseId: id(),
  revision: 1, commandWindowId: id(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false,
  transmitEpoch: null,
  controls: { context: { radioId: 1, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities } as OperationState['controls'] })
afterEach(() => vi.useRealTimers())

type Station = (request: Record<string, any>) => unknown
/** A v4 client holding a logging lease. `station` answers each export (a string is an error code),
 * and heartbeats are answered with the same capabilities. */
function open(capabilities: string[], station?: Station) {
  const clock = { now: 1000 }, sent: Record<string, any>[] = []
  const client: OperationClient = new OperationClient(s => {
    const message = JSON.parse(s)
    sent.push(message)
    const answer = message.request.type === 'heartbeat' ? () => state(capabilities)
      : message.request.type === 'activationExport' && station ? () => station(message.request) : null
    if (answer) queueMicrotask(() => {
      const value = answer()
      client.receive({ type: 'operationResponse', requestId: message.request.requestId, ...(typeof value === 'string' ? { error: value } : { value }) })
    })
  }, true, () => clock.now, { read: () => null, write: () => {} }, 4)
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0]!.request.requestId, value: state(capabilities) })
  return { client, sent, clock, exports: () => sent.filter(m => m.request.type === 'activationExport').map(m => m.request) }
}
const chunkOf = (bytes: Uint8Array, sha256: string, index: number, file: Record<string, unknown> = {}) => ({
  operation: 'activationExport', index,
  file: { byteLength: bytes.length, sha256, chunks: Math.ceil(bytes.length / ACTIVATION_EXPORT_CHUNK_BYTES), ...file },
  base64: Buffer.from(bytes.subarray(index * ACTIVATION_EXPORT_CHUNK_BYTES, (index + 1) * ACTIVATION_EXPORT_CHUNK_BYTES)).toString('base64')
})

it('names exactly one activation by park, UTC day and callsign, and nothing else from the log', () => {
  expect(activationSelection(selection)).toEqual(selection)
  expect(activationSelection({ ...selection, callsign: null })).toEqual({ ...selection, callsign: null })
  expect(activationSelection({ ...selection, reference: 'W7A/MN-001' }).reference).toBe('W7A/MN-001')
  expect(operationRequest(exportRequest()).type).toBe('activationExport')
  // No selection asks for the list of activations, and only its first page.
  expect(operationRequest(exportRequest({ selection: null })).type).toBe('activationExport')
  for (const bad of [
    { ...selection, from: 0 }, { ...selection, to: DAY }, { ...selection, search: 'K1' }, { ...selection, all: true },
    { reference: 'US-1234', dayStartUnix: DAY }, { ...selection, reference: '' }, { ...selection, reference: 'us-1234' },
    { ...selection, reference: 'US 1234' }, { ...selection, reference: 'US-1234,US-5678' }, { ...selection, dayStartUnix: DAY + 1 },
    { ...selection, dayStartUnix: -86400 }, { ...selection, callsign: 'w9xyz' }, { ...selection, callsign: '' }
  ])
    expect(() => operationRequest(exportRequest({ selection: bad })), JSON.stringify(bad)).toThrow()
  for (const bad of [{ index: -1 }, { index: 32 }, { index: 1.5 }, { selection: null, index: 1 }, { index: undefined },
    { cursor: null }, { search: '' }, { from: 0 }, { expectedRevision: 1 }, { commandWindowId: id() }])
    expect(() => operationRequest(exportRequest(bad)), JSON.stringify(bad)).toThrow()
  const { selection: _, ...unselected } = exportRequest()
  expect(() => operationRequest(unselected)).toThrow()
})

it('reads the list, one bounded chunk with its digest, or a refusal, and nothing wider', () => {
  const file = { byteLength: 5, sha256: 'a'.repeat(64), chunks: 1 }
  for (const value of [
    { operation: 'activationExport', activations: [entry, { ...entry, program: null, callsign: null }] },
    { operation: 'activationExport', activations: [] },
    { operation: 'activationExport', file, index: 0, base64: 'SGVsbG8=' },
    { operation: 'activationExport', refused: 'notFound' },
    { operation: 'activationExport', refused: 'tooLarge' }
  ])
    expect(operationValue(value)).toEqual(value)
  for (const bad of [
    { operation: 'activationExport', activations: Array(129).fill(entry) },
    { operation: 'activationExport', activations: [{ ...entry, rows: [] }] },
    { operation: 'activationExport', activations: [{ ...entry, reference: 'US 1234' }] },
    { operation: 'activationExport', file: { ...file, byteLength: ACTIVATION_EXPORT_MAX_BYTES + 1, chunks: 33 }, index: 0, base64: '' },
    { operation: 'activationExport', file: { ...file, chunks: 2 }, index: 0, base64: 'SGVsbG8=' },
    { operation: 'activationExport', file, index: 1, base64: 'SGVsbG8=' },
    { operation: 'activationExport', file, index: 0, base64: 'not base64!' },
    { operation: 'activationExport', file, index: 0, base64: 'A'.repeat(43696) },
    { operation: 'activationExport', file, index: 0, base64: 'SGVsbG8=', adif: '<EOR>' },
    { operation: 'activationExport', refused: 'forbidden' },
    { operation: 'activationExport' }
  ])
    expect(() => operationValue(bad), JSON.stringify(bad).slice(0, 80)).toThrow()
})

it('downloads one activation file in whole chunks, checks its digest, and yields to the heartbeat between chunks', async () => {
  const text = Array.from({ length: 2400 }, (_, i) => `<CALL:5>K${String(i).padStart(4, '0')}<MY_SIG_INFO:7>US-1234<EOR>\n`).join('')
  const bytes = new TextEncoder().encode(text), sha256 = hex(await crypto.subtle.digest('SHA-256', bytes))
  const chunks = Math.ceil(bytes.length / ACTIVATION_EXPORT_CHUNK_BYTES)
  expect(chunks, 'positive control: the file spans several chunks').toBeGreaterThan(2)
  const { client, exports, clock } = open(['qsoLogging', 'activationExport'], r => chunkOf(bytes, sha256, r.index))
  const wait = vi.fn(async (ms: number) => { clock.now += ms })
  const blob = await downloadActivation(client, selection, wait)
  expect(await blob.text()).toBe(text)
  expect(exports().map(r => r.index)).toEqual([...Array(chunks).keys()])
  expect(exports().every(r => JSON.stringify(r.selection) === JSON.stringify(selection))).toBe(true)
  expect(wait.mock.calls.length).toBeGreaterThanOrEqual(chunks - 1)
  client.disconnected()
})

it('refuses a file whose digest, index, chunk length or description does not hold', async () => {
  const bytes = new TextEncoder().encode('<EOR>\n'.repeat(10000)), sha256 = hex(await crypto.subtle.digest('SHA-256', bytes))
  const stations: Station[] = [
    r => chunkOf(bytes, sha256, r.index, { sha256: 'f'.repeat(64) }),
    // The log changed between chunks: the second describes a different file.
    r => r.index === 0 ? chunkOf(bytes, sha256, 0) : chunkOf(bytes, sha256, 1, { sha256: 'e'.repeat(64) }),
    r => ({ ...chunkOf(bytes, sha256, r.index), index: 0 }),
    r => r.index === 0 ? { ...chunkOf(bytes, sha256, 0), base64: Buffer.from(bytes.subarray(0, 100)).toString('base64') } : chunkOf(bytes, sha256, 1)
  ]
  // Positive control: the same station answering honestly delivers the file.
  const honest = open(['activationExport'], r => chunkOf(bytes, sha256, r.index))
  expect((await (await downloadActivation(honest.client, selection, async ms => { honest.clock.now += ms })).arrayBuffer()).byteLength).toBe(bytes.length)
  honest.client.disconnected()
  for (const station of stations) {
    const { client, clock } = open(['activationExport'], station)
    await expect(downloadActivation(client, selection, async ms => { clock.now += ms })).rejects.toThrow('invalidActivationFile')
    client.disconnected()
  }
  for (const refused of ['notFound', 'tooLarge']) {
    const { client } = open(['activationExport'], () => ({ operation: 'activationExport', refused }))
    await expect(downloadActivation(client, selection, async () => {})).rejects.toThrow(refused)
    client.disconnected()
  }
})

it('waits out a busy station without giving up the download or the lease', async () => {
  const bytes = new TextEncoder().encode('<EOR>\n'), sha256 = hex(await crypto.subtle.digest('SHA-256', bytes))
  let busy = 2
  const { client, exports, clock } = open(['activationExport'], r => busy-- > 0 ? 'stationBusy' : chunkOf(bytes, sha256, r.index))
  const blob = await downloadActivation(client, selection, async ms => { clock.now += ms })
  expect(await blob.text()).toBe('<EOR>\n')
  expect(exports()).toHaveLength(3)
  // A busy reply to a read is not a lost lease: the station state is kept.
  expect(client.getSnapshot().state?.phase).toBe('controlling')
  client.disconnected()
})

it('never asks a station that does not offer an export, so an older desktop is never sent one', async () => {
  for (const capabilities of [[], ['qsoLogging', 'logEdit', 'qslMarks', 'otaHunt', 'otaActivation']]) {
    const { client, exports } = open(capabilities, () => { throw Error('an older station must never be asked') })
    await expect(client.activationExport(null)).rejects.toMatchObject({ message: 'notController', sent: false })
    await expect(downloadActivation(client, selection, async () => {})).rejects.toThrow('notController')
    expect(exports()).toEqual([])
    client.disconnected()
  }
  const old = new OperationClient(() => { throw Error('never sent') }, true, () => 1000, undefined, 3)
  await expect(old.activationExport(null)).rejects.toMatchObject({ message: 'stationUnsupported', sent: false })
})

it('an older hosted page tolerates the new station hint, a bounded token it drops', () => {
  for (const hint of ['activationExport']) {
    expect(LOG_CAPABILITIES).toContain(hint)
    // The token rule every hosted page since operation v3 applies before it filters unknown hints.
    expect(hint).toMatch(/^[a-z][a-zA-Z0-9]{0,31}$/)
  }
  expect((operationValue(state(['qsoLogging', 'activationExportNextYear'])) as OperationState).controls!.capabilities).toEqual(['qsoLogging'])
})

it('accepts a reply over the 4 KiB operation bound only for the export it asked for', async () => {
  const { client, sent } = open(['activationExport'])
  const pending = client.activationExport(null), request = sent[sent.length - 1]!.request
  const reply = { type: 'operationResponse', requestId: request.requestId, value: { operation: 'activationExport', activations: [entry] } }
  expect(() => client.receive(reply, OPERATION_EXPORT_RESPONSE_BYTES + 1)).toThrow('invalidOperation')
  client.receive(reply, 40_000)
  await expect(pending).resolves.toEqual(reply.value)
  client.disconnected()
  // Positive control: the same size on an ordinary state reply is still refused.
  const plain: Record<string, any>[] = []
  const other = new OperationClient(s => plain.push(JSON.parse(s)), true, () => 1000, { read: () => null, write: () => {} }, 4)
  other.open()
  expect(() => other.receive({ type: 'operationResponse', requestId: plain[0]!.request.requestId, value: state([]) }, 5000)).toThrow('invalidOperation')
  other.disconnected()
})

it('the relay routes an export only as a versioned v4 read, and keeps an older station off its wire', () => {
  const station = { send: vi.fn<(s: string) => void>(), close: vi.fn() }
  const browser = { send: vi.fn<(s: string) => void>(), close: vi.fn() }
  const sessionId = id(), deviceId = id(), relay = new OperationRelay()
  relay.sync({ peer: station, supported: true, operationVersion: 4 }, [{ sessionId, deviceId, commandUntil: Number.MAX_SAFE_INTEGER, peer: browser }], 100)
  const request = exportRequest()
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 3, request }, 101)
  expect(JSON.parse(browser.send.mock.lastCall![0]).error).toBe('stationUnsupported')
  expect(station.send).not.toHaveBeenCalled()
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4, request }, 102)
  expect(JSON.parse(station.send.mock.lastCall![0])).toMatchObject({ type: 'operationRequest', operationVersion: 4, request })
  // A read, not a write: a log change may still be sent beside it.
  const change = { type: 'logChange', requestId: id(), stationBootId: id(), leaseId: id(), expectedRevision: 1, commandWindowId: id(),
    clientSequence: 1, change: { kind: 'clearHunt' } }
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4, request: change }, 103)
  expect(JSON.parse(station.send.mock.lastCall![0]).request.type).toBe('logChange')
  const chunk = { operation: 'activationExport', file: { byteLength: 40000, sha256: 'a'.repeat(64), chunks: 2 }, index: 0,
    base64: Buffer.alloc(ACTIVATION_EXPORT_CHUNK_BYTES).toString('base64') }
  relay.receiveStation({ type: 'operationResponse', sessionId, requestId: request.requestId, value: chunk })
  expect(JSON.parse(browser.send.mock.lastCall![0]).value).toEqual(chunk)
  relay.sync({ peer: station, supported: true, operationVersion: 3 }, [{ sessionId, deviceId, commandUntil: Number.MAX_SAFE_INTEGER, peer: browser }], 2200)
  station.send.mockClear()
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4, request: exportRequest() }, 2201)
  expect(JSON.parse(browser.send.mock.lastCall![0]).error).toBe('stationUnsupported')
  expect(station.send).not.toHaveBeenCalled()
  // Positive control: an unversioned export is a malformed peer.
  relay.receiveBrowser(sessionId, { type: 'operationRequest', request: exportRequest() }, 2202)
  expect(browser.close).toHaveBeenCalledWith(1008, 'invalidOperation')
})
