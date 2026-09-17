// THE RECEIPT LOCK, AS THE BROWSER HAS IT. `browserReceiptLock` asks the Web Locks API for the key
// with `ifAvailable: true`: held, the request fails at once with `remoteBusy` — it never queues.
// A test double that simply runs its action (`exclusive: run => run()`) has no lock at all, and it
// let a pushed outcome's receipt clear ship broken: the real `pendingControlStorage.write` refuses
// outside the lock, and nothing in a lock-less test can refuse. This one holds and refuses exactly
// as the browser's does, so `pendingControlStorage` under test is the shipped store.
import type { ReceiptLock } from '../operation-storage'

export function ifAvailableLock(): ReceiptLock {
  const held = new Set<string>()
  return async (key, action) => {
    if (held.has(key)) throw Error('remoteBusy')
    held.add(key)
    try { return await action() } finally { held.delete(key) }
  }
}
