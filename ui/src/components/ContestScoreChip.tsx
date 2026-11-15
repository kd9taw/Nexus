// ---------------------------------------------------------------------------
// ⭐ THE SCORE CHIP — the running contest at a glance in every contest log strip
// ("ILQP · 212 pts · 48 Q · 19 mults · 41/hr"), so the cockpit answers "am I counting?"
// without a trip to the Contest screen.
//
// THE SAME NUMBERS THE CONTEST SCREEN SHOWS, FROM THE SAME PLACES — never a second scoring
// path. Each is read off the session the engine serialises, exactly as the scoreboard's
// tiles read it (`FieldDayScoreboard`, ContestView.tsx):
//   • points — `totalScore`, the engine's claimed total. LEFT OUT when the session carries
//     none (a Remote view of an older station): the screen then falls back to working one
//     out from settings this strip does not hold, and a total that could disagree with the
//     screen is worse than no total.
//   • Q — `qsoCount`.
//   • mults — `multCount` when the ruleset counts multipliers; otherwise the worked
//     `sections`, which is the second tile the screen shows for both Field Days.
//   • /hr — the scoreboard's FIRST rate tile: `contestRate`'s last-ten window, per hour,
//     measured to now on the same five-second clock, so it decays while nothing is logged;
//     the screen's em dash when there is no reading yet.
// The contest is named as the screen's header names it (`contestShortName`).
//
// Only while a contest runs: every host renders it only while it holds a session.
//
// ⚠️ IT NEVER MOVES THE STRIP, and it has TWO PLACES because the strip has two widths:
//   • 'head' — a cockpit that frames its contest strip in a Log pane (CW, Phone, RTTY, PSK)
//     shows it in that pane's head, beside the one-word title. That strip sits in a log
//     column of 24–44em, where its own header row has no room left at all.
//   • 'strip' — a host with no frame (Satellites, the contest logger window) shows it in the
//     strip's own header row as a ZERO-COST SLOT (`.le-fd-score`): every other item in that
//     row lays out exactly as it did without it, and the chip takes only the room left.
// Either face clips with an ellipsis rather than wrap or grow — the whole reading is its
// title. Both are measurements in headless Chrome; jsdom lays out nothing.
// ---------------------------------------------------------------------------
import { useEffect, useMemo, useState } from 'react'
import { t } from '../i18n'
import type { FieldDayStatus } from '../types'
import { contestRate } from '../features/contestRate'
import { contestShortName, countsOnly, scoresOwnPoints } from '../fdEvent'

/** No rate yet — the scoreboard tile's own symbol (`RATE_NONE`, ContestView.tsx): one contact
 *  is not a rate, and "0/hr" would claim one. A symbol, not prose. */
const RATE_NONE = '—'
/** Between readings. A separator, not prose: the strip's hint uses the same one. */
const SEP = ' · '

/** The scoreboard's cadence (`useNowUnix`, ContestView.tsx): every window is measured TO NOW,
 *  so the reading has to move when nothing is logged. */
function useNowUnix(): number {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  useEffect(() => {
    const id = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 5000)
    return () => clearInterval(id)
  }, [])
  return now
}

/** The chip's reading, in the screen's order: contest · points · Q · mults · rate. */
export function scoreChipText(fieldDay: FieldDayStatus, nowUnix: number): string {
  // A blank event is ARRL Field Day everywhere else in the app, so it is here.
  const parts = [contestShortName(fieldDay.event || 'arrlfd')]
  // A contact-count-only contest (`countsOnly`) shows no points and no sections: its total is a
  // count, not its score, and sections are Field Day's.
  const count = countsOnly(fieldDay)
  if (typeof fieldDay.totalScore === 'number' && (!count || scoresOwnPoints(fieldDay))) {
    parts.push(t('logEntry.score.points', { count: fieldDay.totalScore }))
  }
  parts.push(t('logEntry.score.qsos', { count: fieldDay.qsoCount }))
  const mults = fieldDay.multCount
  if (typeof mults === 'number') parts.push(t('logEntry.score.mults', { count: mults }))
  else if (!count) parts.push(t('logEntry.score.sections', { count: fieldDay.sections }))
  const perHour = contestRate(fieldDay.log ?? [], nowUnix).windows[0].perHour
  parts.push(t('logEntry.score.rate', { rate: perHour === null ? RATE_NONE : Math.round(perHour) }))
  return parts.join(SEP)
}

export function ContestScoreChip({
  fieldDay,
  place = 'strip',
}: {
  fieldDay: FieldDayStatus
  /** Where it sits: the strip's own header row, or the head of the Log pane framing it. */
  place?: 'strip' | 'head'
}) {
  const nowUnix = useNowUnix()
  const text = useMemo(() => scoreChipText(fieldDay, nowUnix), [fieldDay, nowUnix])
  const title = `${text}\n${t('logEntry.score.title')}`
  return place === 'head' ? (
    <span className="contest-score-chip" title={title}>
      <span className="contest-score-chip-val">{text}</span>
    </span>
  ) : (
    <span className="le-fd-score" title={title}>
      <span className="le-fd-score-val">{text}</span>
    </span>
  )
}
