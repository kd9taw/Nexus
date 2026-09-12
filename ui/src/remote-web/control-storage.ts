// Retain a submitted operating intent until its native result is known or the
// operator explicitly reviews it. No lease or reusable command window is saved.
import { operationId } from './operation-protocol'
import { browserReceiptLock, type ReceiptLock } from './operation-storage'
import { stationAction, type StationAction } from './station-operation'
export type PendingControl = { operationId: string; action: StationAction }
export type ControlStorage = {
  exclusive: <T>(action: () => T | Promise<T>) => Promise<T>
  read: () => PendingControl | null
  write: (entry: PendingControl | null) => void
}
export function pendingControlStorage(
  storage: () => Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>,
  stationId: string,
  lock: ReceiptLock = browserReceiptLock
): ControlStorage {
  const key = `nexus.remote.pending-control.${stationId}`
  // Share the existing logging lock so an operation cannot race a QSO receipt.
  const lockKey = `nexus.remote.pending-log.${stationId}`
  let held = false, owned: string | null = null
  function read(): PendingControl | null {
    const raw = storage().getItem(key)
    if (raw === null) return null
    if (new TextEncoder().encode(raw).length > 4096) throw Error('receiptStorageUnavailable')
    const e = JSON.parse(raw)
    if (!e || typeof e !== 'object' || Array.isArray(e) || Object.keys(e).sort().join(',') !== 'action,operationId,version' || e.version !== 1 || !operationId(e.operationId)) throw Error('receiptStorageUnavailable')
    return { operationId: e.operationId, action: stationAction(e.action) }
  }
  return {
    exclusive: action => lock(lockKey, async () => {
      held = true
      try { return await action() } finally { held = false }
    }),
    read: () => { const entry = read(); owned = entry?.operationId ?? null; return entry },
    write: entry => {
      if (!held) throw Error('receiptStorageUnavailable')
      const current = read()
      if (entry === null) {
        if (owned && current?.operationId === owned) storage().removeItem(key)
        owned = null
        return
      }
      if (storage().getItem(lockKey) !== null) throw Error('operationUnknown')
      if (!operationId(entry.operationId) || (current && current.operationId !== entry.operationId)) throw Error('receiptStorageUnavailable')
      const raw = JSON.stringify({ version: 1, operationId: entry.operationId, action: stationAction(entry.action) })
      if (new TextEncoder().encode(raw).length > 4096) throw Error('receiptStorageUnavailable')
      storage().setItem(key, raw)
      owned = entry.operationId
    }
  }
}
