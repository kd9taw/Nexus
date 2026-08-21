// The manual element-refresh result, spoken OPERATOR. One composer for both
// surfaces — the Update-now toast (Settings + the Satellites refresh
// controls) and the Settings "Last refresh" line — so the wording can never
// drift between them. Pure: the backend supplies typed facts
// (`lastErrorKind` + the currency fields), this composes the copy; the raw
// error text is passed through as tooltip material and is NEVER the
// headline (an operator mid-pass needs "what now", not an HTTP code).

//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Each
// headline is ONE catalog entry with its numbers interpolated — never a
// sentence assembled from fragments. The band sentence appended by `bands()`
// below is a separate WHOLE sentence (elementBands.ts owns its wording), which
// is a different thing from gluing clauses together mid-sentence.

import type { TleStatus } from '../api'
import type { ToastKind } from '../toast'
import { elementBandSentence } from './elementBands'
import { t } from '../i18n'

export interface TleRefreshMessage {
  /** The headline — operator-voiced, never HTTP. */
  text: string
  /** How it lands: toast tier / the line's tone. */
  kind: ToastKind
  /** The raw failure text, for a title/tooltip — debugging, never the headline. */
  raw?: string
}

/** Elements within the 14 d stale line — the same threshold every other
 * currency surface (badge, lane, arm-confirm) uses, applied to the same
 * scalar: `elementAgeDays` is the MEDIAN of the usable sets, so a handful of
 * legitimately old birds cannot decide this. */
const current = (s: TleStatus) =>
  s.usableCount > 0 && s.elementAgeDays != null && s.elementAgeDays <= 14

/** The birds outside the current band, said out loud — their own fact, beside
 * the age rather than folded into it. Without them "your elements are 0.2 d
 * old" leaves the missing birds unexplained and the drifting ones unmentioned;
 * with them the operator can tell "current, a few sit out" from "the set is
 * going off". Empty when there is nothing to report — a zero is not news.
 *
 * EVERY branch below appends it, not just the ones today's pre-launch mirror
 * happens to take: a count that is named only while a particular error is set
 * is not a count the operator can rely on. */
const bands = (headline: string, s: TleStatus) => {
  const sentence = elementBandSentence(s)
  if (sentence === '') return headline // the clean case keeps its exact wording
  return `${headline.endsWith('.') ? headline : `${headline}.`}${sentence}`
}

export function tleRefreshMessage(s: TleStatus): TleRefreshMessage {
  if (s.lastError == null) {
    // The attempt landed. Celestrak as the serving source means the mirror
    // could not deliver and the same-flight escalation did — Celestrak is
    // only ever reached while the mirror is failing, and saying so tells
    // the operator the pipe healed itself.
    return s.source === 'celestrak'
      ? {
          text: bands(t('sat.refresh.mirrorDown', { count: s.count }), s),
          kind: 'success',
        }
      : {
          text: bands(t('sat.refresh.ok', { count: s.count, source: s.source }), s),
          kind: 'success',
        }
  }
  const raw = s.lastError
  if (s.lastErrorKind === 'celestrakBlocked') {
    // The established blocked message (the satLane chip says the same) —
    // the HTTP code that triggered it lives in the tooltip.
    return {
      text: bands(t('sat.refresh.blocked'), s),
      kind: 'error',
      raw,
    }
  }
  if (s.lastErrorKind === 'mirrorUnreachable') {
    if (current(s)) {
      // THE PRE-LAUNCH REALITY: the mirror endpoint 404s until the next
      // site release ships it. With current elements there is nothing to
      // fix — say so calmly, with the age as proof, and account for the
      // birds sitting out so their absence needs no second explanation.
      // (Field report 2026-08-01: this branch was unreachable while the age
      // was a max over the usable set, so a fine catalog got the error line.)
      return {
        text: bands(
          t('sat.refresh.mirrorUnreachableCurrent', { age: s.elementAgeDays!.toFixed(1) }),
          s,
        ),
        kind: 'info',
        raw,
      }
    }
    return s.usableCount > 0 && s.elementAgeDays != null
      ? {
          text: bands(
            t('sat.refresh.mirrorUnreachableStale', { days: Math.round(s.elementAgeDays) }),
            s,
          ),
          kind: 'error',
          raw,
        }
      : {
          text: bands(t('sat.refresh.mirrorUnreachableEmpty'), s),
          kind: 'error',
          raw,
        }
  }
  // 'failed' — and, defensively, any kind this build doesn't know.
  return {
    text: bands(t('sat.refresh.failed'), s),
    kind: 'error',
    raw,
  }
}
