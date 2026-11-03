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
//
// #346 — A ROW'S MENU HIDES ONE CALL MORE NARROWLY: on this band only, and for an hour, two hours
// or the rest of the UTC day. Those are SCOPED hides, a list of their own (`HIDE_CALLS_SCOPED_KEY`)
// that the same panes read beside the list above, and that the chip's popover lists with a ✕ each,
// so a call hidden from a menu can always be found and brought back. A hide on every band with no
// end is exactly an entry of the list above, so the menu adds it there. Display only, like the rest.

import { useCallback, useEffect, useState } from 'react'

export const HIDE_CALLS_KEY = 'nexus.decodes.hideCalls'
export const HIDE_CALLS_SCOPED_KEY = 'nexus.decodes.hideCallsScoped'
const HIDE_CALLS_EVENT = 'nexus:hide-calls'

/** One call hidden from a row's menu, narrower than the plain list: on one band (`band`, the band's
 *  label, e.g. '20m') or on every band (null), until a moment (`until`, Unix ms) or for good (null). */
export interface ScopedHide {
  call: string
  band: string | null
  until: number | null
}

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

/** The stored scoped hides, without any that has run out by `now` or that is not well formed. */
export function parseScopedHides(raw: string | null | undefined, now: number): ScopedHide[] {
  if (!raw) return []
  try {
    const arr: unknown = JSON.parse(raw)
    if (!Array.isArray(arr)) return []
    return arr.flatMap((h): ScopedHide[] => {
      if (!h || typeof h !== 'object') return []
      const { call, band, until } = h as Record<string, unknown>
      if (typeof call !== 'string' || call.trim() === '') return []
      if (band !== null && typeof band !== 'string') return []
      if (until !== null && (typeof until !== 'number' || !Number.isFinite(until))) return []
      if (until !== null && until <= now) return []
      return [{ call: call.trim().toUpperCase(), band, until }]
    })
  } catch {
    return []
  }
}

export function loadScopedHides(now: number = Date.now()): ScopedHide[] {
  try {
    return parseScopedHides(window.localStorage.getItem(HIDE_CALLS_SCOPED_KEY), now)
  } catch {
    return []
  }
}

function saveScopedHides(list: ScopedHide[]): void {
  try {
    window.localStorage.setItem(HIDE_CALLS_SCOPED_KEY, JSON.stringify(list))
  } catch {
    /* full/unavailable — the hide still applies this session via the event below */
  }
  window.dispatchEvent(new Event(HIDE_CALLS_EVENT))
}

/** The end of today's UTC day (the next 00:00 UTC), Unix ms: "Rest of today" in a row's menu. */
export function endOfUtcDay(now: number): number {
  const d = new Date(now)
  return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + 1)
}

/**
 * #346 — hide one call from a row's menu. On every band for good it joins the plain list (the one
 * the chip's text field edits); anything narrower is a scoped hide, and hiding the same call on the
 * same scope again replaces the old one's end rather than stacking a second.
 */
export function hideCall(call: string, band: string | null, until: number | null): void {
  const c = call.trim().toUpperCase()
  if (!c) return
  if (band === null && until === null) {
    const list = loadHideCalls()
    if (!list.includes(c)) saveHideCalls([...list, c])
    return
  }
  const rest = loadScopedHides().filter((h) => !(h.call === c && h.band === band))
  saveScopedHides([...rest, { call: c, band, until }])
}

/** Bring back one scoped hide (the ✕ beside it in the chip's popover). */
export function unhideScoped(call: string, band: string | null): void {
  const list = loadScopedHides()
  const next = list.filter((h) => !(h.call === call && h.band === band))
  saveScopedHides(next)
}

/** Is this call hidden by a scoped hide here — on `band` (this surface's band; unknown = only the
 *  every-band ones apply) at `now`? */
export function isCallHiddenScoped(
  call: string | null | undefined,
  band: string | null | undefined,
  scoped: readonly ScopedHide[],
  now: number,
): boolean {
  if (!call || scoped.length === 0) return false
  const c = call.trim().toUpperCase()
  return scoped.some(
    (h) => h.call === c && (h.band === null || (band != null && h.band === band)) && (h.until === null || h.until > now),
  )
}

/** Subscribe to the wildcard-hide list and the scoped hides; every mounted consumer moves together.
 *  A scoped hide that runs out lifts by itself: the hook reads the list again at the earliest end. */
export function useHideCalls(): {
  entries: string[]
  setEntries: (raw: string) => void
  scoped: ScopedHide[]
} {
  const [entries, setEntriesState] = useState<string[]>(loadHideCalls)
  const [scoped, setScoped] = useState<ScopedHide[]>(() => loadScopedHides())
  useEffect(() => {
    const reread = () => {
      setEntriesState(loadHideCalls())
      setScoped(loadScopedHides())
    }
    const onStorage = (e: StorageEvent) => {
      if (e.key === HIDE_CALLS_KEY || e.key === HIDE_CALLS_SCOPED_KEY) reread()
    }
    window.addEventListener(HIDE_CALLS_EVENT, reread)
    window.addEventListener('storage', onStorage)
    return () => {
      window.removeEventListener(HIDE_CALLS_EVENT, reread)
      window.removeEventListener('storage', onStorage)
    }
  }, [])
  // The earliest end among the timed hides: read the list again just after it, so that hide lifts
  // on time even on a quiet band where nothing else redraws the pane.
  useEffect(() => {
    const ends = scoped.flatMap((h) => (h.until === null ? [] : [h.until]))
    if (ends.length === 0) return
    const wait = Math.max(0, Math.min(...ends) - Date.now()) + 50
    const id = window.setTimeout(() => setScoped(loadScopedHides()), Math.min(wait, 2 ** 31 - 1))
    return () => window.clearTimeout(id)
  }, [scoped])
  const setEntries = useCallback((raw: string) => saveHideCalls(parseHideCalls(raw)), [])
  return { entries, setEntries, scoped }
}
