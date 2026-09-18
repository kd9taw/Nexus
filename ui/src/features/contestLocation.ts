// ---------------------------------------------------------------------------
// THE W/VE LOCATION WARNING, in words.
//
// The engine decides WHETHER to warn and says WHAT is wrong (`ContestLocationWarning`: what the
// operator typed as their contest state, and the listed codes it most likely means). It fires
// when this station's own call is in the USA or Canada, the contest has a W/VE side, and the
// state it was given puts it on the DX side anyway — so it would send the DX exchange with no
// QTH. It is a WARNING and never a refusal: a US call operating from abroad really is DX.
//
// One wording for the three places that show it (the contest strip, Settings, and the notice
// when a contest starts), so they cannot drift apart. The codes are invariant tokens.
// ---------------------------------------------------------------------------

import { t } from '../i18n'
import type { AppSnapshot, ContestLocationWarning } from '../types'

/** The warning as the operator reads it. Several hints are ambiguous (`NL` is both `NF` and
 *  `LB`), so all are offered and none is chosen. */
export function locationWarningText(w: ContestLocationWarning): string {
  const head =
    w.typed === ''
      ? t('logEntry.contest.location.blank')
      : t('logEntry.contest.location.unlisted', { typed: w.typed })
  return w.hints.length > 0
    ? `${head} ${t('logEntry.contest.location.hint', { codes: w.hints.join(' / ') })}`
    : head
}

/** The notice to show when `mode` has just started a contest whose session carries the
 *  warning — `null` for any other mode, or a contest without it. */
export function contestStartWarning(
  mode: string,
  snap: AppSnapshot | null | undefined,
): string | null {
  if (!mode.startsWith('fieldday')) return null
  const w = snap?.fieldDay?.locationWarning
  return w ? locationWarningText(w) : null
}
