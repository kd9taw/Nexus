import { expect, it, vi } from 'vitest'
import { ApplicationClient } from './application-client'
import { ApplicationRelay } from './application-relay'
import { applicationCommands } from './application-capabilities'
import { queryRequest } from './application-query-protocol'
import type { QueryPage } from './application-query-protocol'
import { parseConfirmations } from './confirmations'

const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn() })
const last = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0])
const command = 'get_remote_confirmations'
const args = { collection: 'confirmations', cursor: null, search: '', unconfirmed: false, after: null }
const request = () => ({ ...args, type: 'applicationQuery', requestId: crypto.randomUUID() })
const report = () => ({
  diagnoses: [{ index: 3, award: 'DXCC/WAS', status: 'needsAction', reasons: [
    { code: 'r3', confidence: 'confident', explanation: 'K1ABC is confirmed on a non-award source (eQSL/QRZ) only.', action: { kind: 'uploadToLotw' } },
    { code: 'r1', confidence: 'likely', explanation: 'Never pushed to QRZ.', action: { kind: 'reUpload', source: 'QRZ', detail: 'Rejected.' } },
  ] }, { index: 9, award: 'DXCC/WAS', status: 'needsAction', reasons: [
    { code: 'r7', confidence: 'likely', explanation: 'A possible duplicate.', action: { kind: 'mergeDuplicate', otherIndex: 2 } },
  ] }],
  buckets: [{ kind: 'Upload to LoTW', count: 12, qsoIndices: [] }],
  oneAway: [{ entity: 'Japan', bands: ['20m', '40m'], newEntity: false }],
  waitingOnPartner: 1, pendingLag: 2, logCount: 2301,
})
const page = (source: unknown = report(), patch: Partial<QueryPage> = {}): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(),
  collection: 'confirmations', snapshotId: crypto.randomUUID(), offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 5, rows: [],
  meta: { capturedAgeMs: 100, source: source as QueryPage['meta'] }, ...patch })

it('adds an argument-free diagnostics read to v15 only', () => {
  expect(applicationCommands(15)).toContain(command)
  for (let v = 1; v < 15; v++) expect(applicationCommands(v)).not.toContain(command)
  expect(queryRequest(request(), 15)).toMatchObject(args)
  for (const v of [3, 6, 14]) expect(() => queryRequest(request(), v)).toThrow()
  for (const patch of [{ search: 'K1ABC' }, { unconfirmed: true }, { after: 1 }, { cursor: `${crypto.randomUUID()}:1` }, { upload: true }, { indices: [0] }]) {
    expect(() => queryRequest({ ...request(), ...patch }, 15)).toThrow()
  }
})

it('negotiates all 225 pairs and never asks an older station for diagnostics', async () => {
  for (let stationVersion = 1; stationVersion <= 15; stationVersion++) for (let browserVersion = 1; browserVersion <= 15; browserVersion++) {
    const station = peer(), browser = peer(), relay = new ApplicationRelay()
    relay.sync({ peer: station, version: stationVersion }, [{ sessionId: 'one', peer: browser }], 0)
    const client = new ApplicationClient(s => relay.receiveBrowser('one', JSON.parse(s), 0), vi.fn(), browserVersion)
    client.open(); client.receive(last(browser))
    const supported = stationVersion === 15 && browserVersion === 15
    expect(client.supports(command)).toBe(supported)
    if (!supported) {
      await expect(client.invoke(command, args)).rejects.toThrow()
      expect(station.send).not.toHaveBeenCalled()
    } else {
      await expect(client.invoke(command, { ...args, collection: 'parks' })).rejects.toThrow('applicationUnsupported')
      const pending = client.invoke(command, args)
      expect(last(station).collection).toBe('confirmations')
      client.disconnected(); await expect(pending).rejects.toThrow('applicationUnavailable')
    }
    client.disconnected()
  }
})

it('validates the complete bounded report and keeps everything the panel shows', () => {
  const parsed = parseConfirmations(page())
  const { logCount, ...expected } = report()
  expect(parsed).toEqual({ report: expected, logCount, capturedAgeMs: 105 })
  const mutate = (change: (r: ReturnType<typeof report>) => void) => { const r = report(); change(r); return page(r) }
  for (const bad of [
    mutate(r => { r.diagnoses = Array.from({ length: 51 }, () => r.diagnoses[0]) }),
    mutate(r => { r.diagnoses[0].reasons = Array.from({ length: 9 }, () => r.diagnoses[0].reasons[0]) }),
    mutate(r => { r.buckets[0].qsoIndices = [1] as never[] }),
    mutate(r => { (r.diagnoses[0].reasons[0].action as Record<string, unknown>).token = 'secret' }),
    mutate(r => { (r.diagnoses[0].reasons[0].action as Record<string, unknown>).kind = '' }),
    mutate(r => { (r.diagnoses[0].reasons[1].action as Record<string, unknown>).otherIndex = 1.5 }),
    mutate(r => { r.diagnoses[0].reasons[0].explanation = 'x'.repeat(1025) }),
    mutate(r => { r.oneAway[0].bands = Array.from({ length: 65 }, () => '20m') }),
    mutate(r => { (r as Record<string, unknown>).account = 'W1AW' }),
    mutate(r => { r.pendingLag = -1 }),
    page(report(), { collection: 'awards' }),
    page(report(), { rows: [{}], total: 1, retained: 1 }),
    page(report(), { meta: { capturedAgeMs: 60_000, source: report() as unknown as QueryPage['meta'] } }),
  ]) expect(() => parseConfirmations(bad)).toThrow('invalidConfirmations')
})
