// Filter state that lives for the session — through a view switch, gone at app exit — for the Spots
// board and the Needed board's search, which keep the same lifetime (sessionStorage is per window,
// so a pop-out keeps its own). Moved out of SpotsPanel.tsx when the Needed board took its search.
// storage-scope.test.ts lists and scans every key passed to it.
import { useEffect, useState } from 'react'

/** View-session state: the Spots panel unmounts on every view switch, which wiped all
 * filters mid-session (operator report 2026-07-21: "Leaving SPOT and returning resets
 * all filters"). sessionStorage survives the remount and clears on app exit — exactly
 * "retain them until application exit". Falls back to plain state if storage throws.
 *
 * `host` picks WHOSE copy: omitted, the Spots view's own `key`; a pane's scope, that pane's copy
 * (`key.<scope>`); `null`, a filter this host does not keep at all (plain state, nothing stored).
 * The key stays the FIRST argument and a literal: storage-scope.test.ts classifies every one by
 * scanning for exactly that. */
export function useSessionState<T>(
  key: string,
  init: T,
  host?: string | null,
): [T, React.Dispatch<React.SetStateAction<T>>] {
  const stored = host === null ? null : host ? `${key}.${host}` : key
  const [v, setV] = useState<T>(() => {
    if (stored == null) return init
    try {
      const raw = sessionStorage.getItem(stored)
      if (raw != null) return JSON.parse(raw) as T
    } catch {
      /* ignore */
    }
    return init
  })
  useEffect(() => {
    if (stored == null) return
    try {
      sessionStorage.setItem(stored, JSON.stringify(v))
    } catch {
      /* ignore */
    }
  }, [stored, v])
  return [v, setV]
}

