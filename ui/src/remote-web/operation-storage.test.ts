import { expect, it, vi } from 'vitest'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
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
  const held = new Set<string>()
  const lock: ReceiptLock = async (key, action) => {
    if (held.has(key)) throw Error('remoteBusy')
    held.add(key)
    try {
      return await action()
    } finally {
      held.delete(key)
    }
  }
  return {
    lock,
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
      pendingLogStorage(() => disk, stationId, disk.lock)
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
    pendingLogStorage(() => disk, stationId, disk.lock)
  )
  expect(second.getSnapshot().unresolved).toBe(id)
  expect(second.getSnapshot().pendingDraft).toEqual(record)
  expect(sent.filter((v) => v.request.type === 'logManual')).toHaveLength(1)
  second.disconnected()
})
it('refuses another tab overwrite and prevents an old receipt from erasing a newer draft', async () => {
  const disk = storage(),
    station = crypto.randomUUID(),
    first = pendingLogStorage(() => disk, station, disk.lock),
    second = pendingLogStorage(() => disk, station, disk.lock),
    a = crypto.randomUUID(),
    b = crypto.randomUUID()
  first.read()
  second.read()
  await first.exclusive!(() => first.write(a, record))
  await expect(
    second.exclusive!(() => second.write(b, { ...record, call: 'K2ABC' }))
  ).rejects.toThrow()
  expect(second.read()).toBe(a)
  await first.exclusive!(() => first.write(null))
  await first.exclusive!(() => first.write(b, { ...record, call: 'K2ABC' }))
  await second.exclusive!(() => second.write(null))
  const reopened = pendingLogStorage(() => disk, station, disk.lock)
  expect(reopened.read()).toBe(b)
  expect(reopened.readDraft?.()?.call).toBe('K2ABC')
  const other = pendingLogStorage(() => disk, crypto.randomUUID(), disk.lock)
  expect(other.read()).toBeNull()
})
it('retains malformed or inaccessible storage and refuses writes instead of discarding it', async () => {
  const disk = storage(),
    station = crypto.randomUUID(),
    key = `nexus.remote.pending-log.${station}`,
    saved = pendingLogStorage(() => disk, station, disk.lock)
  disk.setItem(key, 'damaged retained draft')
  expect(() => saved.read()).toThrow()
  await expect(saved.exclusive!(() => saved.write(crypto.randomUUID(), record))).rejects.toThrow()
  expect(disk.getItem(key)).toBe('damaged retained draft')
  const denied = pendingLogStorage(
    () => {
      throw Error('storage denied')
    },
    station,
    disk.lock
  )
  await expect(denied.exclusive!(() => denied.write(crypto.randomUUID(), record))).rejects.toThrow(
    'storage denied'
  )
})

it('refuses overlapping cross-tab receipt changes instead of queuing a later write', async () => {
  const disk = storage(),
    station = crypto.randomUUID(),
    first = pendingLogStorage(() => disk, station, disk.lock),
    second = pendingLogStorage(() => disk, station, disk.lock),
    another = pendingLogStorage(() => disk, crypto.randomUUID(), disk.lock),
    a = crypto.randomUUID(),
    b = crypto.randomUUID()
  await first.exclusive!(async () => {
    first.write(a, record)
    second.read()
    await expect(second.exclusive!(() => second.write(null))).rejects.toThrow('remoteBusy')
    await expect(second.exclusive!(() => second.write(b, record))).rejects.toThrow('remoteBusy')
    // The lock belongs to one station; a different station remains independent.
    await another.exclusive!(() => another.write(crypto.randomUUID(), record))
    first.write(null)
    first.write(b, { ...record, call: 'K2ABC' })
  })
  await Promise.resolve()
  const reopened = pendingLogStorage(() => disk, station, disk.lock)
  expect(reopened.read()).toBe(b)
  expect(reopened.readDraft!()?.call).toBe('K2ABC')
  await second.exclusive!(() => second.write(null))
  expect(reopened.read()).toBe(b)
  expect(() => first.write(null)).toThrow('receiptStorageUnavailable')
})
it('does not mutate receipts when the browser cannot provide an exclusive lock', async () => {
  const disk = storage(),
    station = crypto.randomUUID()
  vi.stubGlobal('navigator', {})
  try {
    const saved = pendingLogStorage(() => disk, station)
    await expect(saved.exclusive!(() => saved.write(crypto.randomUUID(), record))).rejects.toThrow(
      'receiptStorageUnavailable'
    )
    expect(disk.values.size).toBe(0)
  } finally {
    vi.unstubAllGlobals()
  }
})
