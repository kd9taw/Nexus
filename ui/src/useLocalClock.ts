import { useCallback, useEffect, useState } from 'react'

// #253 — THE OPTIONAL LOCAL-TIME CLOCK beside UTC in the top bar (Settings ▸ Workspace).
//
// OFF BY DEFAULT. UTC is the station clock — logs, spots and FT slots all run on it — and a second
// clock is only worth its width to an operator who asked for it.
//
// PER MACHINE, NOT IN THE DURABLE STORE, the same classification as nexus-density and
// nexus-field-mode: local time is a fact about THIS computer's time zone, so a backup restoring
// "show local time" onto a laptop in another zone would carry nothing true with it, and losing it
// on a reinstall costs one tap.

export const LOCAL_CLOCK_STORAGE_KEY = 'nexus-local-clock'

function readInitial(): boolean {
  try {
    return localStorage.getItem(LOCAL_CLOCK_STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

export function useLocalClock(): [boolean, (on: boolean) => void] {
  const [on, setOnState] = useState<boolean>(readInitial)

  useEffect(() => {
    try {
      localStorage.setItem(LOCAL_CLOCK_STORAGE_KEY, on ? '1' : '0')
    } catch {
      /* storage unavailable — the choice still applies for this session */
    }
  }, [on])

  const setOn = useCallback((next: boolean) => setOnState(next), [])
  return [on, setOn]
}
