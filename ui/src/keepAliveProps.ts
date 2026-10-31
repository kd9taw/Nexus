// The comparator that lets a hidden keep-alive cockpit sit out a snapshot poll that changed nothing
// it shows.
//
// WHY. App polls `get_snapshot` every 300 ms and sets the decoded JSON as it comes: a brand-new
// object graph each time, equal or not. The seven cockpits App keeps mounted behind `hidden` hosts
// (Operate, RTTY, PSK, JTTY, SSTV, APRS, JS8 — mounted so their decoders and histories keep
// accumulating) re-rendered their whole trees on every one of those polls while nobody could see
// them. `display:none` skips paint, not reconciliation.
//
// ⚠️ WHY THE SNAPSHOT ITSELF IS NOT SHARED. Handing back the last poll's objects for unchanged parts
// (structural sharing) would save more, but it changes what the screen on show does: the FT decode
// panes re-ingest the decode list whenever its array is a new one, and that is what brings the
// last period's rows back on the next poll after Erase or a UDP Clear. A shared, unchanged list
// never re-ingests, so the cleared rows stayed gone until the list changed. The snapshot App holds
// is therefore exactly the one the bridge decoded, and only the hidden cockpits compare by value.
//
// WHAT A HIDDEN COCKPIT SKIPS. `keepAlivePropsEqual` is the `React.memo` comparator for those
// seven hosts. On screen (or crossing on/off screen) it is React's own shallow compare, so the
// visible cockpit renders exactly as before. Hidden on both sides, a render is skipped when every
// prop is unchanged in VALUE (fresh-but-equal objects and elements included) apart from the
// per-poll meter fields in `LIVE_FIELDS`. Nothing under the seven hosts reads those but the FT
// cockpit's slot-countdown anchor, which only draws on screen and re-anchors when the cockpit is
// shown (the `active` flip renders it). Functions compare by identity, never by value: a skipped
// render keeps the cockpit's handlers, so a handler it holds must be the one App would pass now.

import { isValidElement } from 'react'

/** Snapshot fields that change on EVERY poll while the audio engine runs (the audio loop writes
 *  them each pass), whether or not anything else did. A hidden cockpit ignores them; see above. */
export const LIVE_FIELDS: ReadonlySet<string> = new Set(['nextSlotMs', 'rxLevel', 'smeterDb', 'clockAgeSecs'])

/** How deep the hidden-cockpit compare looks; past it, a value that is not the same object counts
 *  as CHANGED (a render, never a skip). The deepest snapshot part a cockpit is handed is under its
 *  box source: boxes → otaBoard → snap → conversations → [i] → messages → [j] → incomplete (8). */
const MAX_DEPTH = 12

const hasOwn = (o: object, k: string) => Object.prototype.hasOwnProperty.call(o, k)

function isPlain(v: unknown): v is Record<string, unknown> {
  if (v === null || typeof v !== 'object' || Array.isArray(v)) return false
  const proto = Object.getPrototypeOf(v)
  return proto === Object.prototype || proto === null
}

/** Equal in value for a hidden cockpit: identity, or the same plain object / array / element
 *  shape with equal parts, `LIVE_FIELDS` aside. Functions and class instances by identity only. */
function quietEqual(a: unknown, b: unknown, depth: number): boolean {
  if (Object.is(a, b)) return true
  if (depth >= MAX_DEPTH) return false
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return false
    for (let i = 0; i < a.length; i++) if (!quietEqual(a[i], b[i], depth + 1)) return false
    return true
  }
  if (isValidElement(a) && isValidElement(b)) {
    const ref = (e: unknown) => (e as { ref?: unknown }).ref
    return a.type === b.type && a.key === b.key && ref(a) === ref(b) && quietEqual(a.props, b.props, depth + 1)
  }
  if (isPlain(a) && isPlain(b)) {
    const keys = Object.keys(a)
    if (keys.length !== Object.keys(b).length) return false
    for (const k of keys) {
      if (!hasOwn(b, k)) return false
      if (!LIVE_FIELDS.has(k) && !quietEqual(a[k], b[k], depth + 1)) return false
    }
    return true
  }
  return false
}

/**
 * The `React.memo` comparator for a keep-alive cockpit host (true = skip the render).
 *
 * - On screen before or after (`active`): React's default shallow compare, so the cockpit on
 *   screen renders on every poll exactly as it did, and showing or hiding one always renders it.
 * - Hidden before and after: skip unless some prop changed in value, `LIVE_FIELDS` aside.
 */
export function keepAlivePropsEqual<P extends { active?: boolean }>(prev: Readonly<P>, next: Readonly<P>): boolean {
  if (prev.active || next.active) {
    const keys = Object.keys(prev) as (keyof P)[]
    if (keys.length !== Object.keys(next).length) return false
    return keys.every((k) => hasOwn(next, k as string) && Object.is(prev[k], next[k]))
  }
  return quietEqual(prev, next, 0)
}
