// What one snapshot poll costs the React tree: structural sharing on arrival, and the comparator
// that lets a hidden keep-alive cockpit sit out a poll that changed nothing it shows.
//
// WHY. App polls `get_snapshot` every 300 ms and used to `setSnap` the decoded JSON as it came:
// a brand-new object graph each time, equal or not. Every `useMemo`/`useEffect` keyed on a slice
// of it (`snap.recentDecodes`, `snap.conversations`, `snap.radio`, …) therefore re-ran on every
// poll, and the seven cockpits App keeps mounted behind `hidden` hosts (Operate, RTTY, PSK, JTTY,
// SSTV, APRS, JS8 — mounted so their decoders and histories keep accumulating) re-rendered their
// whole trees 3.3 times a second while nobody could see them. `display:none` skips paint, not
// reconciliation.
//
// WHAT IS SHARED. `shareSnapshot` walks the new snapshot against the last one and hands back the
// last one's object for every part that is deep-equal, so identity means "unchanged" from the
// root down. Plain JSON only (what the IPC bridge decodes): plain objects, arrays, primitives.
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

/** How deep the hidden-cockpit compare looks before falling back to identity. The deepest value
 *  it has to see through is a box source's board → snapshot → radio → field. */
const MAX_DEPTH = 6

const hasOwn = (o: object, k: string) => Object.prototype.hasOwnProperty.call(o, k)

function isPlain(v: unknown): v is Record<string, unknown> {
  if (v === null || typeof v !== 'object' || Array.isArray(v)) return false
  const proto = Object.getPrototypeOf(v)
  return proto === Object.prototype || proto === null
}

/**
 * `next`, with every part deep-equal to the same part of `prev` replaced by `prev`'s own object.
 * Returns `prev` itself when the two are deep-equal. Neither argument is mutated; a changed part is
 * a new object or array, never an edited old one.
 */
export function shareSnapshot<T>(prev: unknown, next: T): T {
  if (Object.is(prev, next)) return prev as T
  if (Array.isArray(prev) && Array.isArray(next)) {
    let out: unknown[] | null = prev.length === next.length ? null : new Array(next.length)
    for (let i = 0; i < next.length; i++) {
      const v = i < prev.length ? shareSnapshot(prev[i], next[i]) : next[i]
      if (out === null && v !== prev[i]) out = prev.slice(0, i)
      if (out !== null) out[i] = v
    }
    return (out ?? prev) as T
  }
  if (isPlain(prev) && isPlain(next)) {
    const keys = Object.keys(next)
    // A key that left `prev` makes the result new even if every remaining value is shared.
    let out: Record<string, unknown> | null = keys.length === Object.keys(prev).length ? null : {}
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i]
      const had = hasOwn(prev, k)
      const v = had ? shareSnapshot(prev[k], next[k]) : next[k]
      if (out === null && (!had || v !== prev[k])) {
        out = {}
        for (let j = 0; j < i; j++) out[keys[j]] = prev[keys[j]]
      }
      if (out !== null) out[k] = v
    }
    return (out ?? prev) as T
  }
  return next
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
