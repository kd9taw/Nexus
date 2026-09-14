// Whether the Logbook shows its 3-D globe band (D#278 — "is there a way to remove it, not just
// reduce its size"). Default ON, so nobody who likes the globe loses it on upgrade.
//
// APP-GLOBAL, bare localStorage (NOT surfaceGet/surfaceSet), the same classification as the
// units preference: it is a standing statement about how this operator wants the Logbook to
// look, not a property of one window. The switch lives in Settings ▸ Appearance ▸ Workspace
// and the Logbook reads it, and those are different components that can both be mounted
// (keep-alive views), so a change is announced in-window as well — a `storage` event only
// reaches OTHER windows.

import { useEffect, useState } from 'react'

export const LOGBOOK_GLOBE_KEY = 'nexus.logbook.globe'
const CHANGED_EVENT = 'nexus-logbook-globe-changed'

function readShown(): boolean {
  try {
    return localStorage.getItem(LOGBOOK_GLOBE_KEY) !== 'off'
  } catch {
    return true // blocked storage: keep the default
  }
}

/** Show or hide the Logbook globe, and tell every mounted reader in this window at once. The
 *  value rides in the event, so a blocked store (where the write is lost) still flips the view. */
export function setLogbookGlobeShown(shown: boolean): void {
  try {
    localStorage.setItem(LOGBOOK_GLOBE_KEY, shown ? 'on' : 'off')
  } catch {
    /* blocked storage: the change still applies for this session */
  }
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent<boolean>(CHANGED_EVENT, { detail: shown }))
  }
}

/** The current setting, re-rendering when it changes here or in another window. */
export function useLogbookGlobe(): [boolean, (shown: boolean) => void] {
  const [shown, setShown] = useState<boolean>(readShown)
  useEffect(() => {
    const onChange = (e: Event) => setShown((e as CustomEvent<boolean>).detail !== false)
    const onStorage = (e: StorageEvent) => {
      if (e.key === LOGBOOK_GLOBE_KEY) setShown(readShown())
    }
    window.addEventListener(CHANGED_EVENT, onChange)
    window.addEventListener('storage', onStorage)
    return () => {
      window.removeEventListener(CHANGED_EVENT, onChange)
      window.removeEventListener('storage', onStorage)
    }
  }, [])
  return [shown, setLogbookGlobeShown]
}
