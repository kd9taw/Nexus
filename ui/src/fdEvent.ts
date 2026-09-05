/**
 * Field Day event LABEL helpers.
 *
 * ⚠️ NO DATE MATH LIVES HERE ANYMORE. The event window (which weekend, start
 * hour, 27 h SFD / 30 h WFD duration) is computed in Rust from the fd-rules
 * data and arrives on `FieldDayStatus.eventStartUnix/eventEndUnix` — this file
 * only turns a window into words. The deleted TS calendar walk hardcoded a
 * 24-hour duration for both events, so the banner declared WFD over with six
 * hours still on the clock and counted down a year to the next one; keeping a
 * second date computation here is how the two surfaces diverged in the first
 * place (single source of truth: FdRuleset::next_or_running).
 */

import { t } from './i18n'

export type FdKind = 'arrlfd' | 'wfd'

/**
 * ⚠️ INVARIANT — the events' own names, never translated.
 *
 * These are what the ARRL and the WFDA call their events: an operator enters "ARRL Field Day",
 * submits a Cabrillo log to it and reads the same words on the sponsor's site. A translated
 * event name names nothing, exactly as a translated award name would. Shared by the header,
 * the export summary and the Settings event picker so the three cannot drift.
 */
export const FD_EVENT_NAMES: Record<FdKind, string> = {
  arrlfd: 'ARRL Field Day',
  wfd: 'Winter Field Day',
}

export interface FdEvent {
  kind: FdKind
  /** UTC start of the event (Saturday 1800 UTC for ARRL FD; 1600 UTC for WFD). */
  startUnix: number
  /** UTC end of the event (SFD: +27 h; WFD: +30 h — from the rules data). */
  endUnix: number
  /** Human label, e.g. "ARRL Field Day" or "Winter Field Day". */
  label: string
  /** Year the event is in. */
  year: number
}

/**
 * Build the [FdEvent] the label helpers below consume from the Rust-computed
 * window (`FieldDayStatus.eventStartUnix/eventEndUnix`). Null when the DTO
 * carries no window (an older backend) — the banner then shows the event name
 * without a countdown rather than inventing dates client-side.
 */
export function fdEventFromWindow(
  kind: FdKind,
  startUnix: number | undefined,
  endUnix: number | undefined,
): FdEvent | null {
  if (!startUnix || !endUnix) return null
  return {
    kind,
    startUnix,
    endUnix,
    label: FD_EVENT_NAMES[kind],
    year: new Date(startUnix * 1000).getUTCFullYear(),
  }
}

/**
 * Format a countdown string like "starts in 18 days" or "in 2 hours".
 * Returns null when the event is currently running ("active").
 */
export function fdCountdownLabel(now: Date, event: FdEvent): string | null {
  const nowUnix = Math.floor(now.getTime() / 1000)
  if (nowUnix >= event.startUnix && nowUnix < event.endUnix) return null // active

  const secsUntil = event.startUnix - nowUnix
  if (secsUntil <= 0) return null

  const days = Math.floor(secsUntil / 86400)
  const hours = Math.floor((secsUntil % 86400) / 3600)

  if (days >= 2) return t('fieldDay.countdown.days', { count: days })
  if (days === 1) return t('fieldDay.countdown.tomorrow')
  if (hours >= 1) return t('fieldDay.countdown.hours', { count: hours })
  return t('fieldDay.countdown.soon')
}

/**
 * Format the event header subtitle, e.g.:
 *   "ARRL Field Day: Jun 28–29 · starts in 18 days"
 *   "Winter Field Day: Jan 24–25 · active"
 */
export function fdHeaderSubtitle(now: Date, event: FdEvent): string {
  const start = new Date(event.startUnix * 1000)
  const end = new Date(event.endUnix * 1000)

  // Month abbreviations are date formatting, not catalog prose — they stay here with the rest
  // of the date handling (the same line the DXpedition calendar draws).
  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
  const sm = months[start.getUTCMonth()]
  const sd = start.getUTCDate()
  const em = months[end.getUTCMonth()]
  const ed = end.getUTCDate()

  const dateRange = sm === em
    ? `${sm} ${sd}–${ed}`
    : `${sm} ${sd}–${em} ${ed}`

  // ONE sentence with three slots rather than a label plus a glued-on suffix: the countdown
  // was a fragment in the middle of it, and a language that leads with the status has nowhere
  // to put a fragment.
  return t('fieldDay.subtitle', {
    event: event.label,
    dates: dateRange,
    status: fdCountdownLabel(now, event) ?? t('fieldDay.status.active'),
  })
}
