import { useCallback, useEffect, useState } from 'react'

// FIELD MODE — the outdoor/POTA treatment (field report, 2026-08-09).
//
// One boolean, two effects: `data-contrast='high'` on <html> (the token block in styles.css —
// contrast is what daylight destroys first) and a larger auto-fit in useScale (the size half).
// This hook owns only the boolean and the attribute; useScale takes the boolean as an argument
// so the scale half stays a pure-function input swap.
//
// GLOBAL, not per-surface: being outdoors is a fact about the station, the same class of fact
// as the theme — a pop-out follows it (DetachedPanel wires this hook like it wires useTheme).
// NOT in the durable store: cosmetic, same classification as nexus-density — losing it on a
// reinstall costs one tap, and a backup restoring "outdoors" onto an indoor session would be
// wrong more often than right.

const STORAGE_KEY = 'nexus-field-mode'

function readInitial(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

export function useFieldMode(): [boolean, (on: boolean) => void] {
  const [field, setFieldState] = useState<boolean>(readInitial)

  useEffect(() => {
    if (field) {
      document.documentElement.setAttribute('data-contrast', 'high')
    } else {
      document.documentElement.removeAttribute('data-contrast')
    }
    try {
      localStorage.setItem(STORAGE_KEY, field ? '1' : '0')
    } catch {
      /* storage unavailable — the attribute still applies for this session */
    }
  }, [field])

  const setField = useCallback((on: boolean) => setFieldState(on), [])
  return [field, setField]
}
