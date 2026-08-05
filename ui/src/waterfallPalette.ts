import { useCallback, useEffect, useState } from 'react'

/**
 * The waterfall/scope palette store.
 *
 * The MASTER value is a single operator choice that every signal visualization reads —
 * the CW and Phone scopes, and the RTTY/SSTV waterfalls. Changing it in any of those
 * recolors them all at once.
 *
 * Persisted in localStorage (survives restarts) and broadcast on a same-window custom
 * event so every mounted scope re-syncs live — the native `storage` event only fires in
 * OTHER tabs, so it can't keep two scopes in the same window in step. `'auto'` rides the
 * active theme (see `resolveColormap`).
 *
 * # Scopes, and why FT has one (operator, 2026-08-04: "FT modes should start on Turbo")
 *
 * Turbo has been the fresh-install default since the master store was written, and nothing
 * overrides it on the FT surfaces — so an operator asking for this has an explicit older
 * pick saved under the one global key, made in some other mode, silently applying to FT too.
 * The picker is the only writer, so a stored value is always a real choice: resetting it
 * everywhere to satisfy this request would take away a setting the operator made.
 *
 * A SCOPE is therefore its own key (`<master>.<scope>`) with its own default, and it does
 * NOT fall back to the master — falling back is precisely the behavior being fixed. Only the
 * FT surfaces pass one. Everything else keeps reading and writing the bare master key, so
 * existing choices on those surfaces survive untouched and still move together.
 */
export const WF_PALETTE_KEY = 'nexus.waterfall.palette'
const WF_PALETTE_EVENT = 'nexus:wf-palette'

/** The FT8/FT4 waterfall's palette scope. Its default is Turbo, independent of the master. */
export const FT_PALETTE_SCOPE = 'ft'

/** Storage key for a scope (`undefined` = the shared master value, the bare key). */
function keyFor(scope?: string): string {
  return scope ? `${WF_PALETTE_KEY}.${scope}` : WF_PALETTE_KEY
}

export function getWaterfallPalette(scope?: string): string {
  // Default is Turbo (operator preference) for the master value AND for every scope — a
  // fresh/unset install shows Turbo; an install that has picked something keeps it, per key.
  // `'auto'` (ride the theme) is still available in the picker.
  try {
    return localStorage.getItem(keyFor(scope)) ?? 'turbo'
  } catch {
    return 'turbo'
  }
}

export function setWaterfallPalette(value: string, scope?: string): void {
  try {
    localStorage.setItem(keyFor(scope), value)
  } catch {
    /* storage blocked — the palette still applies this session via the event below */
  }
  // Carry the scope so only the surfaces reading that key re-sync: without it, picking a
  // palette in Operate would repaint the CW and Phone scopes too, which is the coupling
  // this scope exists to break.
  window.dispatchEvent(
    new CustomEvent(WF_PALETTE_EVENT, { detail: { value, scope: scope ?? '' } }),
  )
}

/**
 * Subscribe to a palette; returns `[palette, setPalette]`. Every consumer of the SAME scope —
 * each picker and each scope surface — updates together the instant any picker calls the
 * setter. Pass no scope for the shared master value.
 */
export function useWaterfallPalette(scope?: string): [string, (value: string) => void] {
  const [palette, setPalette] = useState<string>(() => getWaterfallPalette(scope))
  useEffect(() => {
    // Re-read on a scope change: the state was seeded from the first scope this component
    // ever had, and a stale value here shows the wrong palette until the next pick.
    setPalette(getWaterfallPalette(scope))
    const mine = scope ?? ''
    const onEvent = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (!detail || typeof detail !== 'object') return
      if ((detail.scope ?? '') !== mine) return // another scope's pick — not ours
      setPalette(typeof detail.value === 'string' ? detail.value : getWaterfallPalette(scope))
    }
    const onStorage = (e: StorageEvent) => {
      if (e.key === keyFor(scope)) setPalette(getWaterfallPalette(scope))
    }
    window.addEventListener(WF_PALETTE_EVENT, onEvent)
    window.addEventListener('storage', onStorage)
    return () => {
      window.removeEventListener(WF_PALETTE_EVENT, onEvent)
      window.removeEventListener('storage', onStorage)
    }
  }, [scope])
  const set = useCallback((value: string) => setWaterfallPalette(value, scope), [scope])
  return [palette, set]
}
