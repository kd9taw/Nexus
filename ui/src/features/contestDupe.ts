// THE CONTEST DUPE VERDICT — one implementation, every surface.
//
// Two implementations of a dupe verdict is the defect this file exists to prevent. The
// contest log strip (LogEntry's FD variant) and the FT cockpit's callsign card ask the same
// question about the same contact, and a cockpit that answered it differently would tell the
// operator to call a station the log is about to refuse.
//
// ZERO IPC. The full own log rides every snapshot and the club-sync block ships club-ONLY
// keys, so both checks are plain lookups on data already in hand. That is what lets a surface
// ask this per keystroke, and per roster click, without a round trip.
import type { FieldDayStatus } from '../types'

/** What the contest log says about one call on one band in one mode class.
 *
 *  'own'  — THIS position already worked them: the hard block `contestLogManual` will refuse,
 *           on the same key (call, band, mode class).
 *  'club' — another position already worked them. N3FJP semantics: a WARNING only, logging
 *           proceeds and the host keeps both rows.
 *  'none' — no contest running, no call to check, or a contact that is genuinely fresh. */
export type ContestDupeVerdict = 'none' | 'own' | 'club'

/**
 * The verdict for `call` on `band` in `modeClass`, against the contest log in `fieldDay`.
 *
 * `fieldDay` absent (no contest running) is 'none' — the caller does not need to pre-check.
 *
 * ⭐ MODE CLASSES THIS CONTEST COUNTS AS ONE. ILQP works a station "once per band and mode
 * (phone and CW/digital)", so CW and RTTY are one mode there and three separate classes
 * everywhere else. Folding through `dupeModeGroups` is what keeps every badge and the
 * engine's refusal asking the same question — without it the operator calls a station the
 * log is about to reject, and finds out after the over.
 */
export function contestDupe(
  fieldDay: FieldDayStatus | null | undefined,
  call: string,
  band: string,
  modeClass: string,
): ContestDupeVerdict {
  const typed = call.trim().toUpperCase()
  if (fieldDay == null || typed === '') return 'none'
  const fold = (m: string): string =>
    (fieldDay.dupeModeGroups ?? []).find((g) => g.includes(m))?.[0] ?? m
  const own = (fieldDay.log ?? []).some(
    (q) => q.call.toUpperCase() === typed && q.band === band && fold(q.mode ?? '') === fold(modeClass),
  )
  if (own) return 'own'
  // ⚠️ The CLUB key is matched RAW, without the mode fold — the club-sync block ships each
  // dupe key as the sending position recorded it. This is the comparison the log strip has
  // always made; it is preserved verbatim here rather than quietly widened, because folding
  // the club side too would change which contacts warn, in every club-synced contest, as a
  // side effect of extracting a function.
  const club = (fieldDay.club?.dupes ?? []).some(
    ([c, b, m]) => c === typed && b === band && m === modeClass,
  )
  return club ? 'club' : 'none'
}
