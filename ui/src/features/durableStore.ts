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
  // WHICH MODES THE OPERATOR RUNS. Turning CW, SSTV, RTTY and the rest off is a deliberate
  // setup of the station, not a preference — an operator who has pruned the rail to the four
  // things he actually uses should not find all fifteen back after an upgrade. Reported
  // 2026-08-21 against 1.7.5, alongside the panel layout below: settings.json survived and
  // these did not, which is exactly the split this file exists to correct.
  'nexus.features.v1',
]

/** The MAIN window's pane layout is durable; a detached panel's is not.
 *
 *  Panel keys are `nexus.panels.<view>.<instance>` ([`panelStorageKey`]), and the instance is
 *  what decides. Which panes you have hidden in the main window is a deliberate arrangement
 *  that takes real time to rebuild — the same operator report that moved `nexus.features.v1`
 *  above lost it on an upgrade. A POPPED-OUT panel's layout is genuinely per-surface chrome,
 *  which is what this file's header means by "the right home for a collapsed-panel flag", and
 *  it stays in localStorage where a second window's arrangement cannot fight the first's.
 *
 *  A prefix rule rather than an entry per view, because the alternative is a list that must be
 *  edited every time a cockpit is added — and the cockpit that gets forgotten is the one whose
 *  operator loses their layout. */
function isMainWindowPanelLayout(key: string): boolean {
  return key.startsWith('nexus.panels.') && key.endsWith('.main')
}

const durable = new Set(DURABLE_KEYS)

/** In-memory mirror of `ui-state.json`. `null` until `loadDurable()` has run — distinct from an
 *  empty map, which is a real and normal first-run answer. */
let cache: Record<string, string> | null = null
/** Writes and removes that arrived BEFORE `loadDurable` filled the cache (`null` = remove).
 *  Without this buffer such a write reached `localStorage` only — the cache was `null` — and
 *  the loaded file then SHADOWED it for the whole session, because `durableGet` prefers the
 *  cache: the stale file value was served, and every flush re-persisted it. That is #205 to
 *  the letter: boot hygiene re-docks a stale popped-out pane before the store loads, and the
 *  waterfall came up "popped out" with no window on every launch, forever. The load applies
 *  these LAST, so a this-session write beats the file copy — the same
 *  whichever-copy-is-newer-wins contract every post-load write already has. */
let preLoad: Map<string, string | null> | null = null
let flushTimer: ReturnType<typeof setTimeout> | null = null
let flushing = false

/** Is this a key we promise to keep? */
export function isDurable(key: string): boolean {
  return durable.has(key) || isMainWindowPanelLayout(key)
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
    // No bridge, or the store could not be read. Fall back to localStorage entirely — but
    // this-session pre-load writes still land in the cache, so reads keep seeing them.
    cache = {}
    if (preLoad) {
      for (const [k, v] of preLoad) if (v !== null) cache[k] = v
      preLoad = null
    }
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
  // The panel layouts cannot be walked from DURABLE_KEYS — they are matched by PREFIX, and
  // the set of views is not known here. So the migration walks localStorage itself for them.
  // Without this the promotion would be worthless to every EXISTING operator: their layout is
  // sitting in localStorage right now, and a rule that only protects future writes protects
  // nobody who already has one.
  for (const key of safeLocalKeys()) {
    if (!isMainWindowPanelLayout(key) || key in loaded) continue
    const local = safeLocalGet(key)
    if (local !== null) {
      loaded[key] = local
      migrated = true
    }
  }
  // This-session writes that arrived before the load beat the file copy (see `preLoad`) —
  // applied AFTER the migration walk, so a pre-load remove also wins over a localStorage
  // copy the migration would otherwise adopt.
  if (preLoad) {
    for (const [k, v] of preLoad) {
      if (v === null) delete loaded[k]
      else loaded[k] = v
    }
    preLoad = null
    migrated = true
  }
  cache = loaded
  if (migrated) scheduleFlush()
}

/** Every `localStorage` key, or an empty list where storage is unavailable (private modes
 *  throw on access — the same guard `safeLocalGet` carries). */
function safeLocalKeys(): string[] {
  try {
    return Object.keys(window.localStorage)
  } catch {
    return []
  }
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
  // ⚠️ NON-DURABLE KEYS STOP HERE, and that check belongs in this function rather than at the
  // call sites. Since the panel layouts became durable BY PREFIX, one module now calls this
  // with keys that are durable (the main window) and keys that are deliberately not (a
  // detached panel). Without this line, `durableSet` would quietly promote whatever it was
  // handed — which is how per-surface chrome ends up in a per-profile store, fighting the
  // scoping it was given on purpose. `isDurable` is the single authority; callers just call.
  if (!isDurable(key)) return
  if (cache) {
    cache[key] = value
    scheduleFlush()
  } else {
    ;(preLoad ??= new Map()).set(key, value)
  }
}

/** Remove a durable key from both stores, so a delete actually sticks. */
export function durableRemove(key: string): void {
  try {
    globalThis.localStorage.removeItem(key)
  } catch {
    /* see durableSet */
  }
  if (cache) {
    if (key in cache) {
      delete cache[key]
      scheduleFlush()
    }
  } else if (isDurable(key)) {
    ;(preLoad ??= new Map()).set(key, null)
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
  preLoad = null
  if (flushTimer) clearTimeout(flushTimer)
  flushTimer = null
  flushing = false
}
