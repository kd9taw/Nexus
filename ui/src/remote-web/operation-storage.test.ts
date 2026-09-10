import { expect, it } from 'vitest'
import { pendingLogStorage } from './operation-storage'
import { OperationClient } from './operation-client'
import type { ManualRecord } from './operation-protocol'
const record: ManualRecord = {
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
  notes: 'The only copy of this contact note',
  whenUnix: null,
  confirmed: false,
  awardConfirmed: false
}
function storage() {
  const values = new Map<string, string>()
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value)
    },
    removeItem: (key: string) => {
      values.delete(key)
    }
  }
}
it('retains the exact submitted draft across a fresh client without resubmission', async () => {
  const disk = storage(),
    stationId = crypto.randomUUID(),
    sent: Record<string, any>[] = [],
    first = new OperationClient(
      (raw) => sent.push(JSON.parse(raw)),
      true,
      () => 1000,
      pendingLogStorage(() => disk, stationId)
    )
  first.open()
  first.receive({
    type: 'operationResponse',
    requestId: sent[0].request.requestId,
    value: {
      stationBootId: crypto.randomUUID(),
      allowed: true,
      phase: 'controlling',
      leaseId: crypto.randomUUID(),
      revision: 1,
      commandWindowId: crypto.randomUUID(),
      nextSequence: 1,
      leaseRemainingMs: 5000,
      actions: ['log.manual'],
      txArmed: false
    }
  })
  const result = first.log(record).catch((e) => e.message),
    id = sent[sent.length - 1].request.requestId
  first.disconnected()
  expect(await result).toBe('operationUnknown')
  const second = new OperationClient(
    () => {
      throw Error('must not send')
    },
    true,
    () => 1000,
    pendingLogStorage(() => disk, stationId)
  )
  expect(second.getSnapshot().unresolved).toBe(id)
  expect(second.getSnapshot().pendingDraft).toEqual(record)
  expect(sent.filter((v) => v.request.type === 'logManual')).toHaveLength(1)
  second.disconnected()
})
it('refuses another tab overwrite and prevents an old receipt from erasing a newer draft', () => {
  const disk = storage(),
    station = crypto.randomUUID(),
    first = pendingLogStorage(() => disk, station),
    second = pendingLogStorage(() => disk, station),
    a = crypto.randomUUID(),
    b = crypto.randomUUID()
  first.read()
  second.read()
  first.write(a, record)
  expect(() => second.write(b, { ...record, call: 'K2ABC' })).toThrow()
  expect(second.read()).toBe(a)
  first.write(null)
  first.write(b, { ...record, call: 'K2ABC' })
  second.write(null)
  const reopened = pendingLogStorage(() => disk, station)
  expect(reopened.read()).toBe(b)
  expect(reopened.readDraft?.()?.call).toBe('K2ABC')
  const other = pendingLogStorage(() => disk, crypto.randomUUID())
  expect(other.read()).toBeNull()
})
it('retains malformed or inaccessible storage and refuses writes instead of discarding it', () => {
  const disk = storage(),
    station = crypto.randomUUID(),
    key = `nexus.remote.pending-log.${station}`,
    saved = pendingLogStorage(() => disk, station)
  disk.setItem(key, 'damaged retained draft')
  expect(() => saved.read()).toThrow()
  expect(() => saved.write(crypto.randomUUID(), record)).toThrow()
  expect(disk.getItem(key)).toBe('damaged retained draft')
  const denied = pendingLogStorage(() => {
    throw Error('storage denied')
  }, station)
  expect(() => denied.write(crypto.randomUUID(), record)).toThrow()
})
