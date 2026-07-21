// Window/surface identity — the one place that decides which storage keys are shared
// between windows and which are private to this one.
//
// A SURFACE is (view, instance); a WINDOW hosts exactly one surface. `instance` is an
// opaque VALIDATED TOKEN: `main` (the docked surface in the main window), `w<n>` (an
// extra unbound surface — a torn-off panel or a second-monitor board) or `r<id>` (bound
// to a RadioProfile.id; multi-radio, not reachable yet). Nothing outside this file ever
// interprets it — it is a key component only.
//
// Panel VISIBILITY is SURFACE-scoped, deliberately. An app-global flag is exactly what
// makes a docked and a popped-out copy of the same view fight over one value — the
// `nexus.waterfall.detached` defect this replaces.

/** `main` | `w<n>` | `r<id>`. Rejected on mismatch, never silently repaired. */
const INSTANCE_RE = /^(main|w[0-9]{1,3}|r[0-9]{1,9})$/

export function isInstanceToken(v: unknown): v is string {
  return typeof v === 'string' && INSTANCE_RE.test(v)
}

/**
 * This window's instance token, read from the URL. An explicit `?instance=` wins; a
 * torn-off window (`?panel=…`) without one is `w1`; the main window is `main`.
 * `instance` is its OWN parameter — never baked into the panel slug, which the Rust
 * side alnum-filters (so `operate-2` / `operate:2` / `operate2` would all collapse).
 */
export function windowInstance(): string {
  const q = new URLSearchParams(window.location.search)
  const raw = q.get('instance')
  if (isInstanceToken(raw)) return raw
  return q.get('panel') ? 'w1' : 'main'
}

export type KeyScope = 'global' | 'surface' | 'radio'

/**
 * Scope a storage key.
 * - `global`  — shared by every window (an app-wide preference).
 * - `surface` — private to this window's (view, instance).
 * - `radio`   — shared by every surface driving the same rig. Only an `r<id>` window is
 *   radio-bound, so every other surface shares the primary rig's key.
 */
export function scopedKey(base: string, scope: KeyScope, instance: string = windowInstance()): string {
  if (scope === 'global') return base
  if (scope === 'surface') return `${base}.${instance}`
  return `${base}.${instance.startsWith('r') ? instance : 'main'}`
}
