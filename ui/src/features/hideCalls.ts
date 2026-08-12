// Wildcard call-hide list (F4MQS asked for "a list of prefixes" — the honest version is
// call-SHAPE hiding, not raw prefix-to-country, which gets portables and Réunion wrong).
//
// DISPLAY-ONLY: this hides rows in the decode panes and the roster, and nothing else. It is
// deliberately NOT the engine-side block list (Alt-double-click), which the auto-responder
// honours — this is the "I don't want to LOOK at these" filter, so it lives entirely in the
// UI layer with the country exclude, app-global (one standing preference, every window).
//
// Entries are exact calls or trailing-`*` prefixes ("VP8*" → any VP8…), the same idiom the
// wanted watch list uses (needalert.rs::wanted_entry_matches), so an operator who knows one
// knows the other.

import { useCallback, useEffect, useState } from 'react'

export const HIDE_CALLS_KEY = 'nexus.decodes.hideCalls'
const HIDE_CALLS_EVENT = 'nexus:hide-calls'

/** Parse the stored blob (a space/comma/newline-separated string) into normalized entries. */
export function parseHideCalls(raw: string | null | undefined): string[] {
  if (!raw) return []
  const seen = new Set<string>()
  return raw
    .split(/[\s,;]+/)
    .map((c) => c.trim().toUpperCase())
    .filter((c) => c.length > 0 && !seen.has(c) && (seen.add(c), true))
}

export function loadHideCalls(): string[] {
  try {
    return parseHideCalls(window.localStorage.getItem(HIDE_CALLS_KEY))
  } catch {
    return []
  }
}

export function saveHideCalls(entries: string[]): void {
  try {
    window.localStorage.setItem(HIDE_CALLS_KEY, entries.join(' '))
  } catch {
    /* full/unavailable — the choice still applies this session via the event below */
  }
  window.dispatchEvent(new Event(HIDE_CALLS_EVENT))
}

/** Does `entry` match `callUpper` (already trimmed+uppercased)? Trailing `*` is a prefix
 *  wildcard; a bare "*" matches NOTHING (a stray wildcard must never hide the whole pane). */
export function hideEntryMatches(entry: string, callUpper: string): boolean {
  if (entry.endsWith('*')) {
    const prefix = entry.slice(0, -1)
    return prefix.length > 0 && callUpper.startsWith(prefix)
  }
  return entry.length > 0 && callUpper === entry
}

/** Is this call hidden by any entry in the list? */
export function isCallHidden(call: string | null | undefined, entries: string[]): boolean {
  if (!call || entries.length === 0) return false
  const c = call.trim().toUpperCase()
  return entries.some((e) => hideEntryMatches(e, c))
}

/** Subscribe to the wildcard-hide list; every mounted consumer moves together. */
export function useHideCalls(): { entries: string[]; setEntries: (raw: string) => void } {
  const [entries, setEntriesState] = useState<string[]>(loadHideCalls)
  useEffect(() => {
    const reread = () => setEntriesState(loadHideCalls())
    const onStorage = (e: StorageEvent) => {
      if (e.key === HIDE_CALLS_KEY) reread()
    }
    window.addEventListener(HIDE_CALLS_EVENT, reread)
    window.addEventListener('storage', onStorage)
    return () => {
      window.removeEventListener(HIDE_CALLS_EVENT, reread)
      window.removeEventListener('storage', onStorage)
    }
  }, [])
  const setEntries = useCallback((raw: string) => saveHideCalls(parseHideCalls(raw)), [])
  return { entries, setEntries }
}
