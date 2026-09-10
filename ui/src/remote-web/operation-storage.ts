// This browser retains the one submitted QSO until its outcome is resolved.
// The relay's hibernation checkpoint continues to contain routing metadata only.
import { manualRecord, operationId, type ManualRecord } from './operation-protocol'
export type ReceiptLock = <T>(key: string, action: () => T | Promise<T>) => Promise<T>
export const browserReceiptLock: ReceiptLock = async (key, action) => {
  const locks = globalThis.navigator?.locks
  if (!locks) throw Error('receiptStorageUnavailable')
  return locks.request(key, { mode: 'exclusive', ifAvailable: true }, async (lock) => {
    if (!lock) throw Error('remoteBusy')
    return action()
  })
}
export type ReceiptStorage = {
  exclusive?: <T>(action: () => T | Promise<T>) => Promise<T>
  read: () => string | null
  readDraft?: () => ManualRecord | null
  write: (id: string | null, record?: ManualRecord) => void
}
type Entry = { version: 1; operationId: string; record: ManualRecord | null }
export function pendingLogStorage(
  storage: () => Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>,
  stationId: string,
  lock: ReceiptLock = browserReceiptLock
): ReceiptStorage {
  const key = `nexus.remote.pending-log.${stationId}`
  let owned: string | null = null
  let held = false
  function current(): Entry | null {
    const raw = storage().getItem(key)
    if (raw === null) return null
    // Preserve the earlier pilot's receipt-only marker until it is checked.
    if (operationId(raw as unknown)) return { version: 1, operationId: raw, record: null }
    if (new TextEncoder().encode(raw).length > 6144) throw Error('receiptStorageUnavailable')
    const e = JSON.parse(raw)
    if (
      !e ||
      typeof e !== 'object' ||
      Array.isArray(e) ||
      Object.keys(e).sort().join(',') !== 'operationId,record,version' ||
      e.version !== 1 ||
      !operationId(e.operationId)
    )
      throw Error('receiptStorageUnavailable')
    return {
      version: 1,
      operationId: e.operationId,
      record: e.record === null ? null : manualRecord(e.record)
    }
  }
  return {
    exclusive: (action) =>
      lock(key, async () => {
        held = true
        try {
          return await action()
        } finally {
          held = false
        }
      }),
    read: () => {
      const e = current()
      owned = e?.operationId ?? null
      return owned
    },
    readDraft: () => {
      const e = current()
      return e?.operationId === owned ? e.record : null
    },
    write: (id, record) => {
      // localStorage has no read/modify/write transaction across tabs. Every
      // mutation is protected by the same origin/station Web Lock, including
      // resolving and dismissing old receipts. Contention refuses, never queues.
      if (!held) throw Error('receiptStorageUnavailable')
      const e = current()
      if (id === null) {
        // Another tab may have started a later contact. An old result must never
        // erase that submission or its draft.
        if (owned && e?.operationId === owned) storage().removeItem(key)
        owned = null
        return
      }
      if (storage().getItem(`nexus.remote.pending-control.${stationId}`) !== null) throw Error('operationUnknown')
      if (!operationId(id) || (e && e.operationId !== id)) throw Error('receiptStorageUnavailable')
      const draft = record ? manualRecord(record) : (e?.record ?? null)
      if (!draft) throw Error('receiptStorageUnavailable')
      const encoded = JSON.stringify({ version: 1, operationId: id, record: draft })
      if (new TextEncoder().encode(encoded).length > 6144) throw Error('receiptStorageUnavailable')
      storage().setItem(key, encoded)
      owned = id
    }
  }
}
