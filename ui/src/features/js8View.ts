// THE JS8 LISTS' VIEW CHOICES — what JS8Call keeps in its window state rather than its Settings
// dialog: the columns each list shows, how each sorts, whether column labels show and whether they are
// its minimal ones (mainwindow.cpp:4657-4665, :4890-4898), whether Band
// activity shows heartbeats and their ACKs (View ▸ "Show Band Heartbeats and ACKs"), and what was
// cleared or removed from the lists. JS8Call 3.0.3's defaults (mainwindow.cpp:513-514, :4669-4686,
// :4772-4800) for every one.
//
// Kept in this browser's storage beside the ★ pins (features/js8Pins), SHARED across windows for the
// same reason: how this operator likes the lists drawn is a fact about the operator, not one window.
// Not durable: one click to remake. A value storage refuses still applies for the session, held in
// the cockpit's state. Everything read is checked; anything unknown falls back to the default.
import { JS8_BAND_SORTS, JS8_CALL_SORTS, type Js8BandSort, type Js8CallSort } from './js8Lists'

export const JS8_VIEW_KEY = 'nexus.js8.view'
export const JS8_LISTS_KEY = 'nexus.js8.lists'

/** Band activity's columns (its message column always shows). */
export const JS8_BAND_COLUMNS = ['offset', 'timestamp', 'snr', 'tdrift', 'submode'] as const
export type Js8BandColumn = (typeof JS8_BAND_COLUMNS)[number]
/** The call list's columns, in JS8Call's order (its mark column shows whenever a row has a mark). */
export const JS8_CALL_COLUMNS = [
  'callsign',
  'timestamp',
  'snr',
  'offset',
  'tdrift',
  'submode',
  'grid',
  'distance',
  'azimuth',
  'log',
  'logName',
  'logComment',
] as const
export type Js8CallColumn = (typeof JS8_CALL_COLUMNS)[number]

export interface Js8ViewPrefs {
  band: { columns: Record<Js8BandColumn, boolean>; sort: Js8BandSort }
  call: { columns: Record<Js8CallColumn, boolean>; sort: Js8CallSort }
  /** "Show Column Labels" (on). */
  labels: boolean
  /** "Minimal Column Labels" (off): JS8Call's short headings, the cells' units dropped. One switch for
   *  both lists, as JS8Call keys it to "all" (:4706-4712). */
  minimal: boolean
  /** "Show Band Heartbeats and ACKs" (on). */
  showHb: boolean
}

/** JS8Call's `showColumn` defaults: everything on but Time Delta, Mode Speed, Grid, Distance and
 *  Azimuth (`defaultOverride`, mainwindow.cpp:4678-4680). */
const OFF_BY_DEFAULT: readonly string[] = ['tdrift', 'submode', 'grid', 'distance', 'azimuth']

export function defaultJs8ViewPrefs(): Js8ViewPrefs {
  const on = <K extends string>(keys: readonly K[]) =>
    Object.fromEntries(keys.map((k) => [k, !OFF_BY_DEFAULT.includes(k)])) as Record<K, boolean>
  return {
    band: { columns: on(JS8_BAND_COLUMNS), sort: 'offset' },
    call: { columns: on(JS8_CALL_COLUMNS), sort: 'callsign' },
    labels: true,
    minimal: false,
    showHb: true,
  }
}

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)

/** Parse what storage holds, keeping each value that is the right kind and defaulting the rest. */
export function parseJs8ViewPrefs(raw: string | null | undefined): Js8ViewPrefs {
  const out = defaultJs8ViewPrefs()
  let v: unknown
  try {
    v = raw ? JSON.parse(raw) : null
  } catch {
    return out
  }
  if (!isRecord(v)) return out
  const columns = <K extends string>(into: Record<K, boolean>, from: unknown) => {
    if (!isRecord(from)) return
    for (const k of Object.keys(into) as K[]) if (typeof from[k] === 'boolean') into[k] = from[k] as boolean
  }
  if (isRecord(v.band)) {
    columns(out.band.columns, v.band.columns)
    if ((JS8_BAND_SORTS as readonly unknown[]).includes(v.band.sort)) out.band.sort = v.band.sort as Js8BandSort
  }
  if (isRecord(v.call)) {
    columns(out.call.columns, v.call.columns)
    if ((JS8_CALL_SORTS as readonly unknown[]).includes(v.call.sort)) out.call.sort = v.call.sort as Js8CallSort
  }
  if (typeof v.labels === 'boolean') out.labels = v.labels
  if (typeof v.minimal === 'boolean') out.minimal = v.minimal
  if (typeof v.showHb === 'boolean') out.showHb = v.showHb
  return out
}

/** What was cleared, removed or added: the time of each (ms), so a station or offset heard after
 *  it is back, as JS8Call's lists fill again from the next decode. */
export interface Js8ListMarks {
  cleared: { rx: number; band: number; call: number }
  /** "Remove Activity": an offset → when. */
  removedOffsets: Record<string, number>
  /** "Remove This Station": a call → when. */
  removedCalls: Record<string, number>
  /** "Add New Station or Group…" with a callsign: the call → when. */
  added: Record<string, number>
}

export function emptyJs8ListMarks(): Js8ListMarks {
  return { cleared: { rx: 0, band: 0, call: 0 }, removedOffsets: {}, removedCalls: {}, added: {} }
}

/** At most this many removals or additions are kept: the oldest go first. */
const KEEP = 500

function times(v: unknown): Record<string, number> {
  if (!isRecord(v)) return {}
  const kept = Object.entries(v).filter(([k, t]) => k.length <= 32 && Number.isFinite(t) && (t as number) > 0) as [string, number][]
  return Object.fromEntries(kept.sort((a, b) => b[1] - a[1]).slice(0, KEEP))
}

export function parseJs8ListMarks(raw: string | null | undefined): Js8ListMarks {
  const out = emptyJs8ListMarks()
  let v: unknown
  try {
    v = raw ? JSON.parse(raw) : null
  } catch {
    return out
  }
  if (!isRecord(v)) return out
  if (isRecord(v.cleared)) {
    for (const k of ['rx', 'band', 'call'] as const) {
      const t = v.cleared[k]
      if (typeof t === 'number' && Number.isFinite(t) && t > 0) out.cleared[k] = t
    }
  }
  out.removedOffsets = times(v.removedOffsets)
  out.removedCalls = times(v.removedCalls)
  out.added = times(v.added)
  return out
}

function load<T>(key: string, parse: (raw: string | null) => T): T {
  try {
    return parse(window.localStorage.getItem(key))
  } catch {
    return parse(null)
  }
}

function save(key: string, value: unknown): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(value))
  } catch {
    /* full or unavailable: the choice still holds for this session, in the cockpit's state */
  }
}

export const loadJs8ViewPrefs = (): Js8ViewPrefs => load(JS8_VIEW_KEY, parseJs8ViewPrefs)
export const saveJs8ViewPrefs = (p: Js8ViewPrefs): void => save(JS8_VIEW_KEY, p)
export const loadJs8ListMarks = (): Js8ListMarks => load(JS8_LISTS_KEY, parseJs8ListMarks)
export const saveJs8ListMarks = (m: Js8ListMarks): void => save(JS8_LISTS_KEY, m)

/** What "Clear" of one list, or "Clear All Lists", leaves (mainwindow.cpp:2970-3031): the lists'
 *  stamps at `nowMs`, the removals and additions of the call list gone with it. */
export function clearJs8Lists(m: Js8ListMarks, which: 'rx' | 'band' | 'call' | 'all', nowMs: number): Js8ListMarks {
  const cleared = { ...m.cleared }
  if (which === 'rx' || which === 'all') cleared.rx = nowMs
  if (which === 'band' || which === 'all') cleared.band = nowMs
  if (which === 'call' || which === 'all') cleared.call = nowMs
  const band = which === 'band' || which === 'all'
  const call = which === 'call' || which === 'all'
  return {
    cleared,
    removedOffsets: band ? {} : m.removedOffsets,
    removedCalls: call ? {} : m.removedCalls,
    added: call ? {} : m.added,
  }
}
