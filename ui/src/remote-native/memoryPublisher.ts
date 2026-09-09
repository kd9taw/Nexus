import { getRemoteStationStatus, publishRemoteMemoryBank } from '../api'
import { memoriesStore } from '../features/memories'
import { memoryBank } from './memoryBank'

// Started only by the installed main window, after loadDurable. Never imported
// by the hosted entry or a detached panel, and never writes either data store.
export function startMemoryPublisher(
  status = getRemoteStationStatus,
  publish = publishRemoteMemoryBank,
): () => void {
  let live = true, pending = false, lastGeneration: string | null = null
  let lastBank: ReturnType<typeof memoriesStore.get> | null = null, lastAt = -Infinity
  async function tick() {
    if (!live || pending) return
    pending = true
    try {
      const current = await status()
      if (!live) return
      const generation = current.observationGeneration
      if (!generation || !['connecting', 'connected', 'reconnecting'].includes(current.phase)) {
        lastGeneration = null; lastBank = null; return
      }
      const bank = memoriesStore.peek()
      if (generation === lastGeneration && bank === lastBank && performance.now() - lastAt < 20_000) return
      // A too-large or unsupported bank explicitly clears the last publication.
      // The native consumer independently enforces the same closed envelope.
      const payload = bank && memoryBank(bank) ? JSON.stringify(bank) : null
      const accepted = await publish(generation, payload)
      if (live && accepted) { lastBank = bank; lastGeneration = generation; lastAt = performance.now() }
    } catch { /* Failure expires at the native/browser TTL; no silent fallback. */ }
    finally { pending = false }
  }
  // Changes can be rapid (drag reorder). The steady poll coalesces them and also
  // notices enable/reconnect without coupling Remote to the Settings component.
  const timer = setInterval(() => void tick(), 5000)
  void tick()
  return () => { live = false; clearInterval(timer) }
}
