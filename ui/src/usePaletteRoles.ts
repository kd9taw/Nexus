import { useCallback, useEffect, useState, useSyncExternalStore } from 'react'
import {
  PALETTE_ROLES,
  attrValueOf,
  presetIdOf,
  type PaletteRoleId,
  type PaletteSelection,
} from './features/paletteRoles'

// THE COLOUR ROLES AT RUNTIME — two halves, one writer.
//
// `usePaletteRoles` is the OWNER, called once per document (App, and a pop-out's DetachedPanel,
// the way both call useTheme): it holds the operator's picks, and it is the ONLY writer of the
// `data-<role>` attributes in its document. Two owners in one document would race on mount the
// way two contrast writers did (useFieldMode.ts), so Settings is handed the picks and the setter
// as props rather than calling this itself.
//
// `usePaletteKey` is for READERS that cache a token's value in a canvas — MapView's memo, the
// waterfall's marker inks. A CSS custom property changing under them says nothing to React, so
// they key their caches on this string, which changes exactly when the attributes do (the role
// attributes, and Night's, which retunes tokens the same way).
//
// PER MACHINE, webview-local, like the theme and field mode: a colour is a preference about this
// screen and the eyes in front of it, not station data a backup should carry to another shack.
// A DEFAULT IS NO ATTRIBUTE AND NO KEY — the default is the theme blocks themselves.
// index.html seeds the attributes before first paint (index-preseed.test.ts executes that copy
// against features/paletteRoles.ts). Like the theme, an open pop-out picks up a change the next
// time it opens.
//
// ⚠️ THE KEYS COME FROM THE ROLE TABLE, so storage-scope.test.ts's literal scan cannot see these
// calls; it classifies every `storage` key in PALETTE_ROLES directly instead, which is the same
// guarantee from the table's side.

/** Fired on window after the attributes change. */
export const PALETTE_EVENT = 'nexus-palette'

function readStored(): PaletteSelection {
  const out: Record<string, string> = {}
  for (const r of PALETTE_ROLES) {
    let stored: string | null = null
    try {
      stored = localStorage.getItem(r.storage)
    } catch {
      /* storage unavailable — the defaults */
    }
    out[r.id] = presetIdOf(r, stored)
  }
  return out as PaletteSelection
}

export interface PalettePrefs {
  /** The preset each role is on. */
  palette: PaletteSelection
  /** Pick a preset; the role's default is its Reset. */
  setPreset: (role: PaletteRoleId, presetId: string) => void
}

export function usePaletteRoles(): PalettePrefs {
  const [palette, setPalette] = useState<PaletteSelection>(readStored)

  useEffect(() => {
    const root = document.documentElement
    for (const r of PALETTE_ROLES) {
      const value = attrValueOf(r, palette[r.id])
      if (value === null) root.removeAttribute(r.attr)
      else root.setAttribute(r.attr, value)
    }
    window.dispatchEvent(new Event(PALETTE_EVENT))
  }, [palette])

  // Storage is written by the pick, never by the mount: opening a window must not rewrite a
  // choice made in another one.
  const setPreset = useCallback((role: PaletteRoleId, presetId: string) => {
    const r = PALETTE_ROLES.find((x) => x.id === role)
    if (!r) return
    const id = presetIdOf(r, presetId)
    try {
      if (attrValueOf(r, id) === null) localStorage.removeItem(r.storage)
      else localStorage.setItem(r.storage, id)
    } catch {
      /* storage unavailable — the attribute still applies for this session */
    }
    setPalette((prev) => (prev[role] === id ? prev : { ...prev, [role]: id }))
  }, [])

  return { palette, setPreset }
}

const subscribe = (onChange: () => void) => {
  window.addEventListener(PALETTE_EVENT, onChange)
  return () => window.removeEventListener(PALETTE_EVENT, onChange)
}
// Night (useNight.ts) retunes the same kind of token without being a role, and fires the same
// event, so it is part of the key: a canvas that cached a day colour must repaint at dusk.
const snapshot = () =>
  [...PALETTE_ROLES.map((r) => r.attr), 'data-night'].map((a) => document.documentElement.getAttribute(a) ?? '').join('|')

/** A string that changes exactly when a colour attribute on <html> does: a colour role, or Night. */
export function usePaletteKey(): string {
  return useSyncExternalStore(subscribe, snapshot, snapshot)
}
