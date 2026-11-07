// A filter list that outlives the app: the Spots view's hidden modes and picked bands. Kept per window
// (`surfaceGet`/`surfaceSet`, as the Needed board's filter record is), so the Pop out has a copy of its own and
// neither window moves the other's. storage-scope.test.ts classifies both keys as this seam's.
import { useEffect, useState } from 'react'
import { surfaceGet, surfaceSet } from './features/windowScope'
import { useSessionState } from './useSessionState'

/** What is stored for `key`, checked item by item. A stored list only ever narrows the board, so anything that
 *  fails the check is dropped rather than kept to empty the board later: junk reads as "not picked". */
function loadList(key: string, isValid: (item: string) => boolean): string[] {
  let raw = surfaceGet(key)
  // An older build kept this list in the session. It moves across once, and never over a remembered one. Written
  // here, not left to the effect, so that a second call of this initializer reads the same answer.
  try {
    const old = sessionStorage.getItem(key)
    if (old != null) {
      if (raw == null) {
        raw = old
        surfaceSet(key, old)
      }
      sessionStorage.removeItem(key)
    }
  } catch {
    /* storage refused: what localStorage answered stands */
  }
  if (raw == null) return []
  try {
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return [...new Set(parsed.filter((item): item is string => typeof item === 'string' && isValid(item)))]
  } catch {
    return []
  }
}

/** The view (`host` omitted) remembers its list across restarts; a pane (`host` its scope) keeps its session
 *  copy, and `null` is a host that keeps no such list. `useSessionState` has both of those rules. */
export function useRememberedList(
  key: string,
  host: string | null | undefined,
  isValid: (item: string) => boolean,
): [string[], React.Dispatch<React.SetStateAction<string[]>>] {
  const remembered = host === undefined
  const session = useSessionState<string[]>(key, [], remembered ? null : host)
  const [list, setList] = useState<string[]>(() => (remembered ? loadList(key, isValid) : []))
  useEffect(() => {
    if (remembered) surfaceSet(key, JSON.stringify(list))
  }, [remembered, key, list])
  return remembered ? [list, setList] : session
}
