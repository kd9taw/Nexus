// The manual element-refresh result, spoken OPERATOR. One composer for both
// surfaces — the Update-now toast (Settings + the Satellites refresh
// controls) and the Settings "Last refresh" line — so the wording can never
// drift between them. Pure: the backend supplies typed facts
// (`lastErrorKind` + the currency fields), this composes the copy; the raw
// error text is passed through as tooltip material and is NEVER the
// headline (an operator mid-pass needs "what now", not an HTTP code).

import type { TleStatus } from '../api'
import type { ToastKind } from '../toast'

export interface TleRefreshMessage {
  /** The headline — operator-voiced, never HTTP. */
  text: string
  /** How it lands: toast tier / the line's tone. */
  kind: ToastKind
  /** The raw failure text, for a title/tooltip — debugging, never the headline. */
  raw?: string
}

/** Elements within the 14 d stale line — the same threshold every other
 * currency surface (badge, lane, arm-confirm) uses. */
const current = (s: TleStatus) =>
  s.usableCount > 0 && s.elementAgeDays != null && s.elementAgeDays <= 14

export function tleRefreshMessage(s: TleStatus): TleRefreshMessage {
  if (s.lastError == null) {
    // The attempt landed. Celestrak as the serving source means the mirror
    // could not deliver and the same-flight escalation did — Celestrak is
    // only ever reached while the mirror is failing, and saying so tells
    // the operator the pipe healed itself.
    return s.source === 'celestrak'
      ? { text: `Mirror unreachable — fetched from Celestrak: ${s.count} birds`, kind: 'success' }
      : { text: `Orbital elements updated — ${s.count} birds (${s.source})`, kind: 'success' }
  }
  const raw = s.lastError
  if (s.lastErrorKind === 'celestrakBlocked') {
    // The established blocked message (the satLane chip says the same) —
    // the HTTP code that triggered it lives in the tooltip.
    return {
      text: 'Celestrak refused direct element fetches — direct attempts are stopped for 24 h; the mirror keeps retrying.',
      kind: 'error',
      raw,
    }
  }
  if (s.lastErrorKind === 'mirrorUnreachable') {
    if (current(s)) {
      // THE PRE-LAUNCH REALITY: the mirror endpoint 404s until the next
      // site release ships it. With current elements there is nothing to
      // fix — say so calmly, with the age as proof.
      return {
        text: `The element mirror isn't reachable (it goes live with the next release); your elements are ${s.elementAgeDays!.toFixed(1)} d old — current.`,
        kind: 'info',
        raw,
      }
    }
    return s.usableCount > 0 && s.elementAgeDays != null
      ? {
          text: `The element mirror is unreachable and your elements are ${Math.round(s.elementAgeDays)} d old — import a fresh element file or retry later.`,
          kind: 'error',
          raw,
        }
      : {
          text: 'The element mirror is unreachable and no usable elements are cached — import an element file to get the satellite surfaces running.',
          kind: 'error',
          raw,
        }
  }
  // 'failed' — and, defensively, any kind this build doesn't know.
  return {
    text: 'Element update failed — no source delivered a usable set; retry shortly or import an element file.',
    kind: 'error',
    raw,
  }
}
