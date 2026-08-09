// Durable storage for the browser-storage keys that hold REAL OPERATOR DATA (#28).
//
// The problem, from the audit: 37 keys lived in WebView2 `localStorage`, and while most are
// genuinely cosmetic, some are not — the radio memory channels, the watchlist, the satellite and
// DXpedition chase sets and their armed alarms, the profile list, and the UI scale, which is an
// ACCESSIBILITY setting and so not cosmetic at all. `localStorage` is invisible to the operator,
// does not sit beside settings.json, is not covered by any backup of it, is not per-profile, and
// is origin-scoped — so it is lost by a WebView2 data reset, an uninstall-then-reinstall rather
// than an upgrade in place, or a change of asset protocol. And because WebView2 keys its data
// folder on the bundle identifier, a rename of `com.kd9taw.tempo` would wipe the lot on every
// installed machine with no error and no migration.
//
// So these keys are mirrored into `ui-state.json`, a sibling of settings.json in the current
// profile's config directory. Durable, per-profile, and backed up with everything else.
//
// ── The contract, and why it is shaped like this ────────────────────────────────────────────
// `localStorage` is SYNCHRONOUS and every existing call site depends on that; the Tauri bridge is
// async. Rewriting a dozen modules to be async would be a far larger and riskier change than the
// bug warrants, so instead:
//
//   - `loadDurable()` runs ONCE at boot, before the UI reads anything, and fills an in-memory
//     cache. Call sites keep their synchronous reads.
//   - Writes go to `localStorage` AND to the cache, then schedule an async flush of the whole map.
//     `localStorage` therefore stays a live, current fallback rather than a stale one — which is
//     what makes a failed or slow flush harmless.
//   - On first run the cache is empty and `localStorage` holds everything, so the load MIGRATES:
//     any durable key present in `localStorage` and absent from the store is adopted. Nothing is
//     deleted from `localStorage` — the old copy is a free rollback, and the keys are tiny.
//
// The store is deliberately the SECOND source consulted on read, not the first: whichever copy is
// newer wins by construction, because every write updates both.

import { uiStateLoad, uiStateSave } from '../api'

/** The keys that hold operator data rather than preference. Everything NOT on this list stays in
 *  `localStorage`, which is the right home for a collapsed-panel flag or a chosen tab.
 *
 *  ⚠️ Base keys only — anything routed through `windowScope` is per-surface *chrome*, which is
 *  exactly what should NOT be durable. `storage-durability.test.ts` pins this list against the
 *  audit and fails if a key appears in both classifications. */
export const DURABLE_KEYS: readonly string[] = [
  'nexus.memory.bank.v2',
  'nexus.memory.bank.v1', // the orphaned predecessor: carried so an old install is not stranded
  'nexus.watchlist',
  'nexus.sats.chasing',
  'nexus.dxped.chasing',
  'nexus.sats.alarms',
  'nexus.sats.alarms.fired',
  'nexus.dxped.alarms',
  'nexus.dxped.alarms.fired',
  'nexus.profiles',
  'nexus.navOrder',
  // Accessibility, not decoration. An operator who needs a larger UI should not have to find
  // this setting again because a reinstall cleared a browser store they never knew existed.
  //
  // ⚠️ The CAP only. `nexus-ui-scale-mode` is in `PER_SURFACE` — a detached panel deliberately
  // carries its own scale mode — so a single durable global copy would fight the per-surface
  // scoping and its guard. The audit in #28 named both; the codebase has since split them, and
  // the split wins. The cap is the station-wide ceiling and is global.
  'nexus-ui-scale-cap',
]

const durable = new Set(DURABLE_KEYS)

/** In-memory mirror of `ui-state.json`. `null` until `loadDurable()` has run — distinct from an
 *  empty map, which is a real and normal first-run answer. */
let cache: Record<string, string> | null = null
let flushTimer: ReturnType<typeof setTimeout> | null = null
let flushing = false

/** Is this a key we promise to keep? */
export function isDurable(key: string): boolean {
  return durable.has(key)
}

/**
 * Load the durable store and migrate anything still only in `localStorage`.
 *
 * Call once, early, and `await` it before the first UI read. Never throws: a bridge that is not
 * there (a test, a plain browser) leaves the cache empty and every call site falls back to
 * `localStorage`, which is exactly the pre-#28 behaviour and is safe.
 */
export async function loadDurable(): Promise<void> {
  let loaded: Record<string, string> = {}
  try {
    loaded = (await uiStateLoad()) ?? {}
  } catch {
    // No bridge, or the store could not be read. Fall back to localStorage entirely.
    cache = {}
    return
  }
  // Migration: adopt what is only in localStorage. Absent-from-store is the test, NOT
  // empty-store — a key legitimately deleted by the operator must not come back from the dead
  // every launch just because localStorage still has it.
  let migrated = false
  for (const key of DURABLE_KEYS) {
    if (key in loaded) continue
    const local = safeLocalGet(key)
    if (local !== null) {
      loaded[key] = local
      migrated = true
    }
  }
  cache = loaded
  if (migrated) scheduleFlush()
}

/** Read a durable key: the store first, then `localStorage`. */
export function durableGet(key: string): string | null {
  if (cache && key in cache) return cache[key]
  return safeLocalGet(key)
}

/** Write a durable key to BOTH stores. `localStorage` stays current so it is a live fallback. */
export function durableSet(key: string, value: string): void {
  try {
    globalThis.localStorage.setItem(key, value)
  } catch {
    // Quota or a disabled store — the durable copy below is then the only one, which is fine.
  }
  if (cache) {
    cache[key] = value
    scheduleFlush()
  }
}

/** Remove a durable key from both stores, so a delete actually sticks. */
export function durableRemove(key: string): void {
  try {
    globalThis.localStorage.removeItem(key)
  } catch {
    /* see durableSet */
  }
  if (cache && key in cache) {
    delete cache[key]
    scheduleFlush()
  }
}

/** Coalesce writes: a memory-bank edit rewrites the whole bank, and a drag reorder fires many. */
function scheduleFlush(): void {
  if (flushTimer) clearTimeout(flushTimer)
  flushTimer = setTimeout(() => {
    flushTimer = null
    void flushDurable()
  }, 250)
}

/** Write the whole map. Exported so a test can flush deterministically instead of waiting. */
export async function flushDurable(): Promise<boolean> {
  if (!cache || flushing) return false
  flushing = true
  try {
    return await uiStateSave(cache)
  } catch {
    return false // localStorage still holds the value; nothing is lost by a failed flush.
  } finally {
    flushing = false
  }
}

// `globalThis`, not `window`: the node-environment suites shim `globalThis.localStorage` with
// no `window` at all, and reaching through `window` there throws ReferenceError into these
// try/catches — which silently swallowed every write and made four profile tests fail.
function safeLocalGet(key: string): string | null {
  try {
    return globalThis.localStorage.getItem(key)
  } catch {
    return null
  }
}

/** Test seam: forget everything loaded, so a case can start from a known state. */
export function __resetDurableForTest(): void {
  cache = null
  if (flushTimer) clearTimeout(flushTimer)
  flushTimer = null
  flushing = false
}
