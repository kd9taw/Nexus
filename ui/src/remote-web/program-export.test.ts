// The Program view's CHIRP/CSV export, read from the station in bounded chunks. The station holds
// the only writer; this browser holds only the reader, and the reader is the SAME one the activation
// ADIF uses (`chunked-file`), so neither export can grow a looser check than the other.
import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import {
  EXPORT_CHUNK_BYTES,
  EXPORT_MAX_BYTES,
  LOG_CAPABILITIES,
  operationRequest,
  operationValue,
  type OperationState
} from './operation-protocol'
import { downloadProgramExport, programExportName } from './program-export'

const id = () => crypto.randomUUID()
const hex = (b: ArrayBuffer) => [...new Uint8Array(b)].map(x => x.toString(16).padStart(2, '0')).join('')
const exportRequest = (over: Record<string, unknown> = {}) =>
  ({ type: 'programExport', requestId: id(), stationBootId: id(), leaseId: id(), format: 'chirp', nameCap: 7, index: 0, ...over })
const state = (capabilities: string[]): OperationState => ({ stationBootId: id(), allowed: true, phase: 'controlling', leaseId: id(),
  revision: 1, commandWindowId: id(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false, transmitEpoch: null,
  controls: { context: { radioId: 1, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities } as OperationState['controls'] })
afterEach(() => vi.useRealTimers())

type Station = (request: Record<string, any>) => unknown
/** A v4 client holding a lease. `station` answers each export (a string is an error code). */
function open(capabilities: string[], station?: Station) {
  const clock = { now: 1000 }, sent: Record<string, any>[] = []
  const client: OperationClient = new OperationClient(s => {
    const message = JSON.parse(s)
    sent.push(message)
    const answer = message.request.type === 'heartbeat' ? () => state(capabilities)
      : message.request.type === 'programExport' && station ? () => station(message.request) : null
    if (answer) queueMicrotask(() => {
      const value = answer()
      client.receive({ type: 'operationResponse', requestId: message.request.requestId, ...(typeof value === 'string' ? { error: value } : { value }) })
    })
  }, true, () => clock.now, { read: () => null, write: () => {} }, 4)
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0]!.request.requestId, value: state(capabilities) })
  return { client, sent, clock, exports: () => sent.filter(m => m.request.type === 'programExport').map(m => m.request) }
}
const chunkOf = (bytes: Uint8Array, sha256: string, index: number, file: Record<string, unknown> = {}) => ({
  operation: 'programExport', index,
  file: { byteLength: bytes.length, sha256, chunks: Math.ceil(bytes.length / EXPORT_CHUNK_BYTES), ...file },
  base64: Buffer.from(bytes.subarray(index * EXPORT_CHUNK_BYTES, (index + 1) * EXPORT_CHUNK_BYTES)).toString('base64')
})

it('asks for one of the two formats at a cap a rig has, and nothing wider', () => {
  expect(operationRequest(exportRequest()).type).toBe('programExport')
  expect(operationRequest(exportRequest({ format: 'csv', nameCap: 16 })).type).toBe('programExport')
  for (const bad of [
    // Not a format the deliver row offers, or not a format at all.
    { format: 'adif' }, { format: 'CSV' }, { format: '' }, { format: null }, { format: undefined },
    // A cap no rig has, or not an integer.
    { nameCap: 3 }, { nameCap: 17 }, { nameCap: 7.5 }, { nameCap: '7' }, { nameCap: undefined },
    { index: -1 }, { index: 32 }, { index: 1.5 }, { index: undefined },
    // Nothing else may ride along: no selection, no search, no path, no whole-file ask.
    { selection: null }, { project: 'working' }, { path: '/tmp/x.csv' }, { attribution: 'hearham' },
    { expectedRevision: 1 }, { commandWindowId: id() }
  ])
    expect(() => operationRequest(exportRequest(bad)), JSON.stringify(bad)).toThrow()
})

it('reads one bounded chunk with its digest, or a refusal, and nothing wider', () => {
  const file = { byteLength: 5, sha256: 'a'.repeat(64), chunks: 1 }
  for (const value of [
    { operation: 'programExport', file, index: 0, base64: 'SGVsbG8=' },
    { operation: 'programExport', refused: 'notFound' },
    { operation: 'programExport', refused: 'tooLarge' }
  ])
    expect(operationValue(value)).toEqual(value)
  for (const bad of [
    // The list shape belongs to the activation export; this one has no list.
    { operation: 'programExport', activations: [] },
    { operation: 'programExport', file: { ...file, byteLength: EXPORT_MAX_BYTES + 1, chunks: 33 }, index: 0, base64: '' },
    { operation: 'programExport', file: { ...file, chunks: 2 }, index: 0, base64: 'SGVsbG8=' },
    { operation: 'programExport', file, index: 1, base64: 'SGVsbG8=' },
    { operation: 'programExport', file, index: 0, base64: 'not base64!' },
    { operation: 'programExport', file, index: 0, base64: 'SGVsbG8=', csv: 'Location,Name' },
    { operation: 'programExport', refused: 'forbidden' },
    { operation: 'programExport' }
  ])
    expect(() => operationValue(bad), JSON.stringify(bad).slice(0, 80)).toThrow()
})

it('is offered only by a station that advertises it', () => {
  expect(LOG_CAPABILITIES).toContain('programExport')
})

it('downloads the channel file in whole chunks, checks its digest, and carries the format and cap', async () => {
  const text = 'Location,Name,Frequency\n' +
    Array.from({ length: 1800 }, (_, i) => `${i + 1},REPEAT${i % 10},146.${String(i % 1000).padStart(3, '0')}000\n`).join('')
  const bytes = new TextEncoder().encode(text), sha256 = hex(await crypto.subtle.digest('SHA-256', bytes))
  const chunks = Math.ceil(bytes.length / EXPORT_CHUNK_BYTES)
  expect(chunks, 'positive control: the file spans several chunks').toBeGreaterThan(1)
  const { client, exports, clock } = open(['programExport'], r => chunkOf(bytes, sha256, r.index))
  const wait = vi.fn(async (ms: number) => { clock.now += ms })
  const blob = await downloadProgramExport(client, 'chirp', 9, wait)
  expect(await blob.text()).toBe(text)
  expect(exports().map(r => r.index)).toEqual([...Array(chunks).keys()])
  // The operator's format and rig cap travel with EVERY chunk, or the station could render the
  // second half of the file differently from the first.
  expect(exports().every(r => r.format === 'chirp' && r.nameCap === 9)).toBe(true)
  client.disconnected()
})

it('refuses a file whose digest or description does not hold, and throws the station word otherwise', async () => {
  const bytes = new TextEncoder().encode('Location,Name\n'.repeat(4000)), sha256 = hex(await crypto.subtle.digest('SHA-256', bytes))
  const wait = async () => {}
  const stations: Station[] = [
    // A digest that does not match the bytes.
    r => chunkOf(bytes, sha256, r.index, { sha256: 'f'.repeat(64) }),
    // The file description moving mid-download — a channel list that changed under the reader.
    r => chunkOf(bytes, r.index ? 'b'.repeat(64) : sha256, r.index),
    // A chunk answering for a different index than the one asked for.
    r => chunkOf(bytes, sha256, r.index === 0 ? 0 : 0)
  ]
  for (const station of stations) {
    const { client } = open(['programExport'], station)
    await expect(downloadProgramExport(client, 'csv', 7, wait)).rejects.toThrow('invalidProgramFile')
    client.disconnected()
  }
  // POSITIVE CONTROL: the same harness with an honest station resolves, so the three rejections
  // above are the checks firing rather than the fixture never working at all.
  const honest = open(['programExport'], r => chunkOf(bytes, sha256, r.index))
  expect((await downloadProgramExport(honest.client, 'csv', 7, wait)).size).toBe(bytes.length)
  honest.client.disconnected()
  // A station refusal is thrown by its own word, so the view can say which one happened.
  for (const refused of ['notFound', 'tooLarge']) {
    const { client } = open(['programExport'], () => ({ operation: 'programExport', refused }))
    await expect(downloadProgramExport(client, 'chirp', 7, wait)).rejects.toThrow(refused)
    client.disconnected()
  }
})

it('never asks a station that does not offer it', async () => {
  const { client, exports } = open(['qsoLogging', 'activationExport'], r => chunkOf(new Uint8Array([65]), 'a'.repeat(64), r.index))
  await expect(downloadProgramExport(client, 'chirp', 7, async () => {})).rejects.toThrow('notController')
  expect(exports()).toEqual([])
  client.disconnected()
})

it('names the file the way the desktop names it', () => {
  const when = new Date('2026-09-15T23:30:00Z')
  expect(programExportName('chirp', when)).toBe('nexus-chirp-2026-09-15.csv')
  expect(programExportName('csv', when)).toBe('nexus-channels-2026-09-15.csv')
})
