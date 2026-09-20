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
import type { DupeRule, FieldDayStatus } from '../types'

/** What a station older than `FieldDayStatus.dupeRule` meant: the `(call, band, mode class)`
 *  triple every surface hardcoded then. Absence must behave exactly as that build did — the
 *  same reasoning, and the same value, as `DupeRuleDto::Default` on the Rust side. */
const LEGACY_TRIPLE: DupeRule = {
  byCall: true,
  byBand: true,
  byModeClass: true,
  byFields: [],
  bySentFields: [],
  modeClassGroups: [],
}

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
  const rule = fieldDay.dupeRule ?? LEGACY_TRIPLE
  // ⚠️ **A RULE THAT KEYS ON AN EXCHANGE SLOT CANNOT BE JUDGED FROM THESE THREE**, so it
  // declines rather than guessing — the same answer, for the same reason, that
  // `FieldDayLog::worked_key` gives on the Rust side.
  //
  // The two directions are not symmetric, and `contest::dupe` states which one hurts:
  // "Under-reporting a dupe costs one duplicate contact that scores zero; over-reporting
  // refuses a legal contact." Eight of the seventeen shipped rulesets key on a slot — five
  // QSO parties on QTH, three ARRL VHF runnings on GRID, each in BOTH directions (working
  // someone else's mobile, and being one). Comparing the triple there answered from a
  // PREFIX of the key: a rover reappearing from a new grid read as already-worked and the
  // operator skipped a contact that would have scored.
  //
  // A caller that HAS the exchange can get the exact verdict instead, by comparing a key it
  // builds against each row's `dkey` and the club's `dkeys` — both ride the snapshot. No
  // caller does yet: the FT cockpit answers a roster click before anything is copied, and
  // the log strip's typed boxes are not plumbed here. Until one is, silence is the honest
  // answer and it is the safe one.
  if (rule.byFields.length > 0 || rule.bySentFields.length > 0) return 'none'
  // `dupeModeGroups` rather than `rule.modeClassGroups`: they are the same data from the same
  // field, but the flat one also reaches a station too old to send a rule at all.
  const fold = (m: string): string =>
    (fieldDay.dupeModeGroups ?? []).find((g) => g.includes(m))?.[0] ?? m
  // Only the components the rule actually names. Sweepstakes works a station ONCE (rule 2.2
  // — neither band nor mode class is in its key); CQ WW and WPX count one contact per band
  // either mode. Requiring all three made both of those answer "new" for a contact the log
  // was about to refuse.
  const own = (fieldDay.log ?? []).some(
    (q) =>
      (!rule.byCall || q.call.trim().toUpperCase() === typed) &&
      (!rule.byBand || (q.band ?? '') === band) &&
      (!rule.byModeClass || fold(q.mode ?? '') === fold(modeClass)),
  )
  if (own) return 'own'
  // ⚠️ The CLUB key is matched RAW, without the mode fold — the club-sync block ships each
  // dupe key as the sending position recorded it. This is the comparison the log strip has
  // always made; it is preserved verbatim here rather than quietly widened, because folding
  // the club side too would change which contacts warn, in every club-synced contest, as a
  // side effect of extracting a function.
  //
  // It is left on the legacy triple deliberately, and that costs nothing today: `fdsync`
  // sends `dupes` ONLY while the club runs Field Day and empties it otherwise, and Field
  // Day names all three components — so the flags above could not change this answer. The
  // club's generalised `dkeys` now ride the snapshot beside it, which is what a correct
  // club verdict for the other sixteen rulesets would be built from.
  const club = (fieldDay.club?.dupes ?? []).some(
    ([c, b, m]) => c === typed && b === band && m === modeClass,
  )
  return club ? 'club' : 'none'
}
