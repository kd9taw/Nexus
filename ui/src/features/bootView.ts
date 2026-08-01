// Boot-restore clamps — the pure decisions behind "reopen where the operator left off".
//
// Everything persisted that picks what the window OPENS ON is validated here against
// what this build actually knows how to render, on every launch (the app-wide
// clamp-on-load discipline, same as sizes/positions). The 0.24.6 black-screen field
// report made this test-pinned: a broken restore target is re-restored EVERY launch,
// so a bad value here is not one bad session — it is a restart loop the operator
// cannot escape from inside the app. Note the limit of a clamp, pinned in the tests:
// it rejects ids this build cannot render; a VALID id whose content crashes (the
// 0.24.6 case — `connect` with a throwing pane) is the error boundary's job.
//
// Pure (no React, no storage, no window) so it unit-tests in node — the registry
// pattern. App.tsx owns reading the hash/localStorage and passes the raw values in.
import type { View } from './registry'

/**
 * Resolve the view the main window boots on. Precedence (each step validated):
 *  1. a `#hash` deeplink — an explicit hash is a stronger intent than a restore;
 *  2. legacy deeplinks for merged sections (`propagation` / `map` → `connect`);
 *  3. the persisted `nexus.view` from last session;
 *  4. the active profile's landing view.
 * A hash or persisted id is honored only if it is a section THIS build renders
 * (`sectionIds` — the live registry) and the feature is enabled; anything else —
 * an id removed in an update, a disabled section, or plain garbage — falls through.
 */
export function resolveBootView(
  hash: string,
  persisted: string | null,
  sectionIds: readonly string[],
  isEnabled: (id: string) => boolean,
  landing: View,
): View {
  if (sectionIds.includes(hash) && isEnabled(hash)) return hash as View
  if (hash === 'propagation' || hash === 'map') return 'connect'
  if (persisted && sectionIds.includes(persisted) && isEnabled(persisted)) return persisted as View
  return landing
}

/** Clamp the persisted `nexus.workspace` operate mode. Unknown values — including
 * the retired 'connect' area (Connect is a global view now) — open on FT8/FT4. */
export function coerceArea(v: string | null): 'dx' | 'msg' {
  return v === 'dx' || v === 'msg' ? v : 'dx'
}
