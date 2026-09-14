/** Structural sharing for station samples (operator decision 2026-09-14, "steady cached view").
 *
 * Returns `next`, reusing every part of `previous` that is deep-equal to its counterpart: an unchanged
 * sample is `previous` itself, and a changed one is a new object whose unchanged branches are the old
 * ones. React then renders nothing for a sample that did not change (setState with the same object
 * bails out) and nothing below a memoized consumer whose branch did not change.
 *
 * Station samples are JSON: plain objects, arrays and primitives. Anything else (a class instance, a
 * typed array from a parser) is returned as it came. Shared samples are read-only by contract; nothing
 * in the workspace mutates one. */
export function shareStructure<T>(previous: unknown, next: T): T {
  if (Object.is(previous, next)) return previous as T
  if (Array.isArray(next)) {
    if (!Array.isArray(previous)) return next
    let same = previous.length === next.length
    const out = next.map((value, i) => {
      const shared = shareStructure(previous[i], value)
      if (!Object.is(shared, previous[i])) same = false
      return shared
    })
    return (same ? previous : out) as T
  }
  if (plain(next)) {
    if (!plain(previous)) return next
    const keys = Object.keys(next)
    let same = keys.length === Object.keys(previous).length
    const out: Record<string, unknown> = {}
    for (const key of keys) {
      const shared = shareStructure(previous[key], next[key])
      out[key] = shared
      if (!Object.prototype.hasOwnProperty.call(previous, key) || !Object.is(shared, previous[key])) same = false
    }
    return (same ? previous : out) as T
  }
  return next
}

function plain(value: unknown): value is Record<string, unknown> {
  if (!value || typeof value !== 'object') return false
  const proto = Object.getPrototypeOf(value)
  return proto === Object.prototype || proto === null
}
