// THE RTTY GRAB — double-click a word in Decoded text and it lands where it belongs: a callsign
// in the Call box and the log strip, and (inside a contest) a zone or a QTH in the exchange.
// This is the pure half: a string, an offset into it, and a verdict. The DOM half — which
// character the pointer is over — lives in the cockpit, because it needs a real layout.
//
// ⚠️ WHY THE STRING AND NEVER THE SELECTION. Native word selection breaks at `/`, so a
// double-click on `VE3/K1ABC` selects `VE3` or `K1ABC` and a selection-based grab fills half a
// compound call. And the transcript re-renders every 500 ms from a FRONT-TRIMMED 4000-character
// ring, so any offset stored across a render points at a different character by the next poll.
// The cockpit resolves the offset at event time against the text on screen at that instant, and
// everything below derives the token from that string alone.
//
// THE RULES MIRROR THE SEQUENCER (`crates/tempo-core/src/rtty/seq.rs`), so the grab and the
// auto-sequencer never disagree about what a word is: the same edge punctuation, the same
// keyword list, the same RST normalization (5NN and the letters-plane TOO are both 599). Two
// places part company with it, deliberately:
//   · A call's BASE part must end in a letter here (see `BASE_CALL`) — the segment carrying
//     prefix, digit and suffix, so a portable `W1AW/7` or `K1ABC/QRP` is a call while `TU73`
//     and `NR001` are not. `plausible_call` only asks for a digit and two letters, so it would
//     take RR73; nobody double-clicks RR73 meaning a call.
//   · A FIGS-damaged call is ACCEPTED. `plausible_call` requires a digit, and a call whose
//     figures shift was lost has none: W1AW prints WQAW. The sequencer can afford to wait for
//     clean copy; an operator pointing at the one garbled call they need cannot, so the grab
//     fills it as copied and they correct one letter. The cost is stated rather than hidden:
//     an ordinary word shaped like that (HERE, GOOD) is filled too if the operator
//     double-clicks it — a double-click is their own claim that the word is a call.

/** Stripped from both ends before a token is judged — seq.rs `EDGE_PUNCT`, verbatim. `/` is
 *  not in it: compound calls keep their slash. */
const EDGE_PUNCT = new Set(['?', '.', ',', ':', ';', '!', '"', "'", '(', ')'])

/** Prosigns and abbreviations that are never a callsign or an exchange value — seq.rs
 *  `KEYWORDS`, verbatim. */
const KEYWORDS = new Set([
  'DE', 'CQ', 'QRZ', 'UR', 'RST', 'TU', 'QSL', 'PSE', 'AGN', 'RPT', 'K', 'KN', 'SK', 'BK', 'R',
  'ES', 'HW', 'FB', 'OM', 'GL', 'GM', 'GA', 'GE', '73', '88', 'TEST', 'FD', 'WFD', 'NR', 'NAME',
  'QTH', 'BTU', 'EE', 'UE', 'NW', 'NOW',
])

/** The ITA2 shared codes: a letter a digit prints as when the figures shift is lost (the
 *  QWERTYUIOP row over 1234567890) — seq.rs `garble_digit`. */
const GARBLE_DIGIT: Record<string, string> = {
  Q: '1', W: '2', E: '3', R: '4', T: '5', Y: '6', U: '7', I: '8', O: '9', P: '0',
}

/** The whitespace-delimited token under `offset`. A caret offset is an insertion point, so one
 *  sitting just past a word's last character (a click on its right half) still names that
 *  word; one between two whitespace characters names nothing. */
export function tokenAt(text: string, offset: number): string {
  const at = Math.max(0, Math.min(Math.trunc(offset), text.length))
  const ws = (i: number) => /\s/.test(text[i])
  let pivot: number
  if (at < text.length && !ws(at)) pivot = at
  else if (at > 0 && !ws(at - 1)) pivot = at - 1
  else return ''
  let start = pivot
  while (start > 0 && !ws(start - 1)) start--
  let end = pivot + 1
  while (end < text.length && !ws(end)) end++
  return text.slice(start, end)
}

/** A token as the sequencer's tokenizer sees it: uppercased, control characters dropped (a
 *  FIGS-S garble prints BEL), edge punctuation stripped. */
function clean(raw: string): string {
  let t = Array.from(raw.toUpperCase())
    .filter((c) => c >= '!' && c <= '~')
    .join('')
  while (t && EDGE_PUNCT.has(t[t.length - 1])) t = t.slice(0, -1)
  while (t && EDGE_PUNCT.has(t[0])) t = t.slice(1)
  return t
}

/** seq.rs `normalize_rst(t, 3)`: three positions, each a digit, the cut `N` (9) or a
 *  letters-plane garble; readability 1–5, no other position 0. */
function isRst(t: string): boolean {
  if (t.length !== 3) return false
  let out = ''
  for (const c of t) {
    const d = c >= '0' && c <= '9' ? c : c === 'N' ? '9' : GARBLE_DIGIT[c]
    if (!d) return false
    out += d
  }
  return out[0] >= '1' && out[0] <= '5' && out[1] !== '0' && out[2] !== '0'
}

/** A 4- or 6-character Maidenhead square. */
const GRID = /^[A-R]{2}[0-9]{2}([A-X]{2})?$/

/** The shape of a callsign's BASE part: a letter somewhere before a digit, and a letter last.
 *  That is what separates K1ABC from 100W, 20M, RR73 and every number in an exchange. It is
 *  asked of each `/` segment, never of the whole token: the other segments are a prefix (`VE3`)
 *  or a designator (`7`, `P`, `MM`, `QRP`), and a US call worked outside its area ends `/7`. */
const BASE_CALL = /^[A-Z0-9]*[A-Z][A-Z0-9]*[0-9][A-Z0-9]*[A-Z]$/

function isCall(t: string): boolean {
  if (t.length < 3 || t.length > 14 || !/^[A-Z0-9]+(\/[A-Z0-9]+)*$/.test(t)) return false
  if (GRID.test(t)) return false
  const parts = t.split('/')
  if (/[0-9]/.test(t)) return parts.some((p) => BASE_CALL.test(p))
  // FIGS-damaged: no digit survived. Some part must become a call with ONE of its interior
  // letters read back as the digit it garbles from (WQAW → W1AW). One is enough: a lost figures
  // shift garbles one run of digits, and a call's digits sit together.
  return parts.some((p) =>
    Array.from(p).some((c, i) => {
      const d = GARBLE_DIGIT[c]
      return d !== undefined && BASE_CALL.test(p.slice(0, i) + d + p.slice(i + 1))
    }),
  )
}

/** A number read through the letters-plane garble: `05` sent by a station with unshift-on-space
 *  off prints `PT` here, because the space unshifts our decoder. Up to four digits, so a serial
 *  is readable too; what makes a number an EXCHANGE value is a slot whose bounds accept it. */
function numberOf(t: string): number | null {
  if (t.length < 1 || t.length > 4) return null
  let digits = ''
  for (const c of t) {
    const d = c >= '0' && c <= '9' ? c : GARBLE_DIGIT[c]
    if (!d) return null
    digits += d
  }
  return Number(digits)
}

export type GrabResult =
  | { kind: 'call'; value: string }
  /** A value for one RECEIVED exchange slot, named by its slot id (`ZN`, `QTH`, …). */
  | { kind: 'exchange'; slot: string; value: string }

/** One received exchange slot, as the session publishes it — `snap.fieldDay.receives`, the
 *  ruleset's own shape (`ContestFieldSpec`). Nothing about a contest is written here: the
 *  bounds of a zone and the values of a QTH domain are the RULESET's, so the same grab works
 *  for the next contest without a line changing. */
export interface GrabSlot {
  key: string
  kind: string
  min?: number
  max?: number
  domain?: string
}

/** What the grab knows about the session. Without `slots` there is no contest, and only calls
 *  are grabbed. */
export interface GrabContext {
  slots?: GrabSlot[]
  /**
   * Is `value` a KNOWN member of `domain`?
   *
   * ⚠️ NOT the strip's `inDomain`, which answers TRUE for a domain this build carries no values
   * for. That is right for a while-typing verdict — an absent value set is not evidence that
   * what the operator typed is wrong — and wrong here: it would make every word the operator
   * double-clicked, callsigns included, a QTH. A grab fills only what can be recognised.
   */
  knownCode?: (domain: string | undefined, value: string) => boolean
}

/** Classify one token. `null` is "not something the grab fills" — the caller does nothing and
 *  says nothing. */
export function classifyGrab(raw: string, ctx: GrabContext = {}): GrabResult | null {
  const t = clean(raw)
  if (!t || KEYWORDS.has(t) || isRst(t)) return null
  const slots = ctx.slots ?? []
  if (slots.length > 0) {
    const n = numberOf(t)
    const digits = /^[0-9]+$/.test(t)
    // A number fills a numeric slot only when EXACTLY ONE of them accepts it: two slots that
    // both could take it is an ambiguity, and a grab guessing between them would file the
    // operator's zone as a serial. A garbled number needs the slot to carry bounds — without
    // them any word off the letters plane would be a number.
    const numeric = (s: GrabSlot) =>
      (s.kind === 'number' || s.kind === 'serial') &&
      (digits || s.min !== undefined || s.max !== undefined) &&
      (s.min === undefined || (n ?? -1) >= s.min) &&
      (s.max === undefined || (n ?? -1) <= s.max)
    // Plain digits first: nothing else an exchange carries is a bare number.
    if (n !== null && digits) {
      const fits = slots.filter(numeric)
      if (fits.length === 1) return { kind: 'exchange', slot: fits[0].key, value: String(n) }
    }
    // The domain before the garble: WI is Wisconsin before it is zone 28 on the letters plane.
    const codes = slots.filter((s) => s.kind === 'enum' && (ctx.knownCode?.(s.domain, t) ?? false))
    if (codes.length === 1) return { kind: 'exchange', slot: codes[0].key, value: t }
    if (n !== null && !digits) {
      const fits = slots.filter(numeric)
      if (fits.length === 1) return { kind: 'exchange', slot: fits[0].key, value: String(n) }
    }
  }
  return isCall(t) ? { kind: 'call', value: t } : null
}

/** The token under `offset` in `text`, classified. */
export function grabAt(text: string, offset: number, ctx?: GrabContext): GrabResult | null {
  return classifyGrab(tokenAt(text, offset), ctx)
}
