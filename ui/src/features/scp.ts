// Super Check Partial — which calls in the downloaded contest list match what is typed in the
// contest strip's Call box. Pure: no React, no IO. The list arrives once per strip mount
// (`features/contestLists`), and each keystroke is a plain scan over it: about a millisecond on
// 50,000 calls in V8, measured, so no index is kept.
//
// A call that is not in the list is not an error. New calls exist, and the list covers two years.

/** One call on the SCP line, and why it is there. */
export interface ScpHit {
  call: string
  /** 'worked': already in this contest's log. 'partial': contains what is typed.
   *  'near': one character away from what is typed. */
  kind: 'worked' | 'partial' | 'near'
}

/** Matching starts at the third typed character: two characters match thousands of calls. */
export const SCP_MIN_CHARS = 3
/** One-character-different matches start at the fourth, where they stop being noise. */
export const NEAR_MIN_CHARS = 4
/** The most the line is given; it shows what fits. */
export const SCP_MAX_HITS = 40

/** The calls that CONTAIN what is typed, in list order. */
export function partial(typed: string, calls: readonly string[], limit = SCP_MAX_HITS): string[] {
  const q = typed.trim().toUpperCase()
  const out: string[] = []
  if (q.length < SCP_MIN_CHARS) return out
  for (const c of calls) {
    if (c.includes(q) && out.push(c) >= limit) break
  }
  return out
}

/** Is `a` exactly one substitution, insertion or deletion away from `b`? */
function oneApart(a: string, b: string): boolean {
  if (a.length === b.length) {
    let diff = 0
    for (let i = 0; i < a.length; i++) {
      if (a.charCodeAt(i) !== b.charCodeAt(i) && ++diff > 1) return false
    }
    return diff === 1
  }
  if (Math.abs(a.length - b.length) !== 1) return false
  const [short, long] = a.length < b.length ? [a, b] : [b, a]
  let skipped = false
  for (let i = 0, j = 0; i < short.length; j++) {
    if (short.charCodeAt(i) === long.charCodeAt(j)) i++
    else if (skipped) return false
    else skipped = true
  }
  return true
}

/** The calls one character away from what is typed (one substituted, inserted or deleted),
 *  never the typed call itself. */
export function near(typed: string, calls: readonly string[], limit = SCP_MAX_HITS): string[] {
  const q = typed.trim().toUpperCase()
  const out: string[] = []
  if (q.length < NEAR_MIN_CHARS) return out
  for (const c of calls) {
    if (oneApart(q, c) && out.push(c) >= limit) break
  }
  return out
}

/** The SCP line: calls already in this log first, then the list's partial matches, then its
 *  one-character-different ones, each call once. */
export function scpLine(
  typed: string,
  calls: readonly string[],
  worked: readonly string[],
  limit = SCP_MAX_HITS,
): ScpHit[] {
  const q = typed.trim().toUpperCase()
  if (q.length < SCP_MIN_CHARS) return []
  const seen = new Set<string>()
  const out: ScpHit[] = []
  const add = (call: string, kind: ScpHit['kind']) => {
    if (out.length < limit && !seen.has(call)) {
      seen.add(call)
      out.push({ call, kind })
    }
  }
  for (const c of worked) if (c.includes(q)) add(c, 'worked')
  for (const c of partial(q, calls, limit)) add(c, 'partial')
  for (const c of near(q, calls, limit)) add(c, 'near')
  return out
}
