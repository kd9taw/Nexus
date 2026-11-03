// Park names for the Logbook's Park name column (#437), read from the station's park list
// (`lookupPark`) and held here for the window's life, so a page of contacts in one park asks the
// station ONCE: a lookup on its way is shared by every cell that wants it, and the answer, a
// miss included, is kept. A park list downloaded or imported (`parksChanged`, called by the
// POTA/SOTA screen) forgets them all, and every cell shown asks again.
import { lookupPark, parksCount } from '../api'

/** The answer for each reference asked: a name, or null for one the list does not hold. */
const settled = new Map<string, string | null>()
/** The lookups on their way. */
const asking = new Map<string, Promise<void>>()
/** The park list's count, as last asked (null: not asked since the list last changed). */
let count: Promise<number | null> | null = null
/** Bumped when the list changes, so a lookup that was on its way keeps nothing. */
let generation = 0
const listeners = new Set<() => void>()

/** The name kept for `ref`: a name, null for a miss, undefined when not yet asked. */
export function keptParkName(ref: string): string | null | undefined {
  return settled.get(ref)
}

/** Ask the station for every reference in `refs` not yet kept or on its way; resolves once all
 *  of them are settled. A failed lookup is not kept (it is asked again next time). */
export function resolveParkNames(refs: readonly string[]): Promise<void> {
  const gen = generation
  return Promise.all(
    refs.map((ref) => {
      if (settled.has(ref)) return undefined
      let p = asking.get(ref)
      if (!p) {
        p = lookupPark(ref)
          .then((park) => {
            if (gen === generation) settled.set(ref, park?.name?.trim() || null)
          })
          .catch(() => {})
          .finally(() => {
            if (asking.get(ref) === p) asking.delete(ref)
          })
        asking.set(ref, p)
      }
      return p
    }),
  ).then(() => {})
}

/** How many parks the station's list holds (0: none loaded yet), asked once until it changes;
 *  null when the station could not say. */
export function parkListCount(): Promise<number | null> {
  count ??= parksCount().catch(() => null)
  return count
}

/** The park list was downloaded or imported: forget every name and the count, and tell every
 *  cell shown. */
export function parksChanged(): void {
  generation += 1
  settled.clear()
  asking.clear()
  count = null
  for (const l of [...listeners]) l()
}

/** Call `l` whenever the park list changes; returns the unsubscribe. */
export function onParksChanged(l: () => void): () => void {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}
