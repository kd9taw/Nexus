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

/**
 * ⭐ **Every contest the picker offers, in the order it offers them** — the rules-file
 * event id (`settings.fdEvent`) and the sponsor's own name for the event.
 *
 * ⚠️ INVARIANT — never translated, for the same reason `FD_EVENT_NAMES` is not: an
 * operator enters "Ohio QSO Party", submits a Cabrillo log to it and reads those words
 * on ohqp.org. A translated event name names nothing.
 *
 * ⚠️ **The ids are the rules file's, and this list is a MENU, not a source of truth.**
 * Rust looks each one up through `fd_rules::ruleset_by_id`, which returns `None` for a
 * ruleset a downloaded rules file dropped — so an id here that the data does not carry
 * costs the operator that contest and nothing else. Adding a contest is a rules-file
 * row plus a line here; nothing else in the UI knows a contest by name.
 *
 * `badge` is the mode badge's own form where the short one has a word too wide for the rail
 * (`contestBadgeName`).
 *
 * `submit` is where the sponsor takes the Cabrillo log, its own page or address, read off its site
 * (2026-10-26). The end-of-contest checklist SHOWS it to copy; Nexus never opens it or sends a log
 * anywhere. Absent where the sponsor names none Nexus could read.
 */
export const CONTESTS: { id: string; name: string; short: string; badge?: string; submit?: string }[] = [
  { id: 'arrlfd', name: FD_EVENT_NAMES.arrlfd, short: 'Field Day', submit: 'https://field-day.arrl.org/fdentry.php' },
  { id: 'wfd', name: FD_EVENT_NAMES.wfd, short: 'WFD' },
  // ⚠️ Sweepstakes is TWO contests, not one with two modes: ARRL runs the CW weekend
  // and the Phone weekend separately, scores them separately and gives each its own
  // Cabrillo token. The sponsor's own names, so an operator reads the same words here
  // and on arrl.org.
  { id: 'arrlss_cw', name: 'ARRL November Sweepstakes (CW)', short: 'SS CW', submit: 'https://contest-log-submission.arrl.org/' },
  { id: 'arrlss_ssb', name: 'ARRL November Sweepstakes (Phone)', short: 'SS Phone', submit: 'https://contest-log-submission.arrl.org/' },
  // ⚠️ ARRL VHF is THREE contests, not one row with three dates. The January running
  // prices 902/1296 MHz and 2.3 GHz-and-up differently from June and September
  // (VHF-Rules.pdf v1.2 §5.1 vs §5.2), each running has its own ADIF id and Cabrillo
  // token, and each is on its own weekend in its own month. ARRL's own current names,
  // as they appear on the contest calendar.
  { id: 'arrlvhf_jan', name: 'ARRL January VHF Contest', short: 'January VHF', badge: 'Jan VHF', submit: 'https://contest-log-submission.arrl.org/' },
  { id: 'arrlvhf_jun', name: 'ARRL June VHF Contest', short: 'June VHF', badge: 'Jun VHF', submit: 'https://contest-log-submission.arrl.org/' },
  { id: 'arrlvhf_sep', name: 'ARRL September VHF Contest', short: 'September VHF', badge: 'Sep VHF', submit: 'https://contest-log-submission.arrl.org/' },
  // The ARRL RTTY Roundup, by the name on arrl.org/rtty-roundup. RTTY only: every other mode is
  // in the rules data's banned list, so a contact in one is logged and scores nothing.
  { id: 'arrlrtty', name: 'ARRL RTTY Roundup', short: 'RTTY Roundup', badge: 'RTTY RU', submit: 'https://contest-log-submission.arrl.org/' },
  // CQ WW and CQ WPX are likewise two contests each, on separate weekends with separate
  // Cabrillo tokens. The sponsors' own names, from cqww.com and cqwpx.com.
  // NB the RTTY runnings of both are a DIFFERENT sponsor's contests (CQ/RJ, with the RTTY
  // Journal) and have their own rules, so they are not arms of these rows.
  { id: 'cqww_cw', name: 'CQ World-Wide DX Contest (CW)', short: 'CQ WW CW', submit: 'https://cqww.com/logcheck/' },
  { id: 'cqww_ssb', name: 'CQ World-Wide DX Contest (SSB)', short: 'CQ WW SSB', submit: 'https://cqww.com/logcheck/' },
  // CQ WW RTTY, therefore, is a row of its own (the sponsor's name, from cqwwrtty.com).
  { id: 'cqww_rtty', name: 'CQ World-Wide RTTY DX Contest', short: 'CQ WW RTTY', submit: 'https://cqwwrtty.com/logcheck/' },
  { id: 'cqwpx_cw', name: 'CQ World-Wide WPX Contest (CW)', short: 'CQ WPX CW', submit: 'https://cqwpx.com/logcheck/' },
  { id: 'cqwpx_ssb', name: 'CQ World-Wide WPX Contest (SSB)', short: 'CQ WPX SSB', submit: 'https://cqwpx.com/logcheck/' },
  { id: 'cqp', name: 'California QSO Party', short: 'CQP', submit: 'https://cqp.org/logsubmit-main.html' },
  // The sponsor is the Western Illinois Amateur Radio Club; the name is the one its own
  // rules and its sample log use ("Announcing the 2025 Illinois QSO Party").
  { id: 'ilqp', name: 'Illinois QSO Party', short: 'ILQP', submit: 'n9jf@arrl.net' },
  // The sponsor is the NYQP committee (nyqp.org); the name is the one its own rules use
  // ("2026 New York QSO Party Rules").
  { id: 'nyqp', name: 'New York QSO Party', short: 'NYQP', submit: 'https://nyqp.contesting.com/nyqpsubmitlog.php' },
  { id: 'ohqp', name: 'Ohio QSO Party', short: 'OhQP', submit: 'https://www.ohqp.org/index.php/submit-log/' },
  { id: 'tnqp', name: 'Tennessee QSO Party', short: 'TNQP', submit: 'https://tnqp.contesting.com/' },
  { id: 'txqp', name: 'Texas QSO Party', short: 'TXQP', submit: 'https://txqp.contesting.com/txqpsubmitlog.php' },
]

/** The sponsor's name for a contest by its rules-file id — the id itself for one this menu
 *  does not list (a contest a downloaded rules file added), which is a token rather than a
 *  guess. */
export function contestName(id: string | undefined): string {
  return CONTESTS.find((c) => c.id === id)?.name ?? id ?? ''
}

/** The short form of that name, for the contest view's header (`short` above — every one
 *  an invariant token, never translated). */
export function contestShortName(id: string | undefined): string {
  return CONTESTS.find((c) => c.id === id)?.short ?? id ?? ''
}

/** The name the rail's mode badge shows: the short form, but `badge` where that has a word too wide
 *  for the rail's 88 px ("September", and "January" at the larger text sizes, measured in Chrome). */
export function contestBadgeName(id: string | undefined): string {
  return CONTESTS.find((c) => c.id === id)?.badge ?? contestShortName(id)
}

/** Is this one of the two Field Day events, whose class + section Settings ▸ Contesting ▸ Field Day
 *  asks for? */
export const isFieldDay = (id: string | undefined): boolean =>
  !id || id === 'arrlfd' || id === 'wfd'

/**
 * ⭐ **The contests whose exchange SENDS the station's power**, by rules-file id — the only ones
 * Settings ▸ Contesting ▸ Your station data shows **Power sent** for. None of the shipped ones does
 * (the contests that send power, ARRL International DX among them, are not on the picker), so the
 * box is shown for none of them; a value already stored is kept and is still saved.
 *
 * ⚠️ A MIRROR, like `CLUB_SYNC_REFUSED` below: `fd_rules.rs` reads this list back and compares it
 * with every seeded ruleset whose sent exchange reads `contest_power`, both ways, so a contest that
 * starts sending power is a red test rather than a box nobody can find.
 */
export const CONTESTS_SENDING_POWER: readonly string[] = []

/** The contest's part of an export's file name — `ilqp-log-<date>.cbr`, `arrlfd-summary-<date>.txt`:
 *  its rules-file id, blank meaning ARRL Field Day as it does everywhere else. Every exported file
 *  was named `fd-…` whatever the contest. An id a downloaded rules file added may carry characters
 *  no file name can (a `/` turns the name into a path, and Downloads keeps only its last part), so
 *  each run of them becomes one `-`; every shipped id passes through unchanged. */
export function contestFileStem(id: string | undefined): string {
  return (id?.trim() || 'arrlfd').replace(/[^A-Za-z0-9_-]+/g, '-')
}

/** ⭐ The side of a contest the operator's location puts them on, in words — never the rules
 *  data's role id, which Settings and the log strip printed ("Running as in_state", "Sending
 *  in_state 599 COOK"). In-state and out-of-state are words, and translated; `W/VE` and `DX` are
 *  the sponsors' own names for those sides ("W/VE stations", "DX stations") and invariant tokens,
 *  like the contest names above. A role id this build has no words for is shown as itself, a
 *  token rather than a guess, as `contestName` does for a contest it does not list. */
export function contestRoleWords(role: string): string {
  if (role === 'in_state') return t('contest.role.inState')
  if (role === 'out_of_state') return t('contest.role.outOfState')
  if (role === 'w_ve') return 'W/VE'
  if (role === 'dx') return 'DX'
  return role
}

/** Why club sync cannot run a contest — the three reasons the engine refuses one for. */
export type ClubSyncRefusal = 'serial' | 'transmitter' | 'unknown'

/**
 * ⭐ **The contests club sync cannot run, and why**, by rules-file id.
 *
 * The club log runs every other contest under that contest's own rules — its exchange,
 * dupe key, multipliers and Cabrillo. The two reasons it cannot are facts about a
 * sponsor's rules, which the engine reads off each ruleset (`club_refusal` in
 * `crates/tempo-app/src/fdevent.rs`): a serial-number exchange must run in ONE sequence
 * for the whole entry while every position gives out its own, and a Cabrillo template
 * with a transmitter column must say which transmitter made each contact, which club
 * sync does not number.
 *
 * ⚠️ A MIRROR, held to the engine by a Rust test that reads this table back and compares
 * it with every seeded ruleset (`the_screen_mirrors_the_contests_club_sync_refuses`), so a
 * contest added to the rules seed without its answer here is a red test rather than a
 * screen that disagrees with the sockets. One line per contest, `id: 'reason',`.
 */
export const CLUB_SYNC_REFUSED: Record<string, ClubSyncRefusal> = {
  cqp: 'serial',
  arrlss_cw: 'serial',
  arrlss_ssb: 'serial',
  arrlrtty: 'serial',
  cqwpx_cw: 'serial',
  cqwpx_ssb: 'serial',
  cqww_cw: 'transmitter',
  cqww_ssb: 'transmitter',
  cqww_rtty: 'transmitter',
}

/**
 * ⭐ **Club sync is switched on for a contest it cannot run, so the engine refuses it** —
 * and why, or `null` when it runs (or is not switched on at all).
 *
 * Hosting the Illinois QSO Party used to build an ARRL Field Day club log in silence; the
 * club now runs whatever contest is picked, except the ones above
 * (`Engine::club_sync_refusal`). This reads the same settings the engine does, the trim
 * and the blank default (ARRL Field Day) included, so the screen saying why cannot
 * disagree with what the sockets do.
 */
export function clubSyncRefusal(
  s: { fdHostEnable?: boolean; fdJoinAddr?: string; fdEvent?: string } | null | undefined,
  preview?: { event: string } | null,
): ClubSyncRefusal | null {
  if (!s) return null
  const configured = s.fdHostEnable === true || (s.fdJoinAddr ?? '').trim() !== ''
  return configured ? contestClubRefusal(s.fdEvent, preview) : null
}

/**
 * Whether the contest rules the station LOADED carry `fdEvent` — `false` when a rules file
 * it downloaded dropped the contest, which the engine refuses club sync for
 * (`ClubRefusal::NoRuleset`, code `unknown`). Read off the station's preview of the picked
 * contest (`get_fd_ruleset`), which resolves an id its rules do not carry to the ARRL Field
 * Day that mode entry falls back to. `null` without a preview: nothing is known yet.
 *
 * ⚠️ The preview answers for the SAVED pick, so compare it with the saved pick: an unsaved
 * choice held against a preview of the last one would name the wrong contest.
 */
export function rulesCarry(fdEvent: string | undefined, preview: { event: string } | null | undefined): boolean | null {
  if (!preview) return null
  return preview.event === (fdEvent?.trim() || 'arrlfd')
}

/**
 * Why club sync cannot run this contest at all, switched on or not — `null` when it can.
 * The rules first, as the engine asks: a contest the loaded rules do not carry is refused
 * for that, whatever this table says of it.
 */
export function contestClubRefusal(
  fdEvent: string | undefined,
  preview?: { event: string } | null,
): ClubSyncRefusal | null {
  if (rulesCarry(fdEvent, preview) === false) return 'unknown'
  const id = fdEvent?.trim() || 'arrlfd'
  return Object.prototype.hasOwnProperty.call(CLUB_SYNC_REFUSED, id) ? CLUB_SYNC_REFUSED[id] : null
}

/** The sentence a refusal shows — one place, for the three surfaces that show it. */
export function clubSyncRefusalText(why: ClubSyncRefusal, contest: string): string {
  switch (why) {
    case 'serial':
      return t('fieldDay.club.refused.serial', { contest })
    case 'transmitter':
      return t('fieldDay.club.refused.transmitter', { contest })
    case 'unknown':
      return t('fieldDay.club.refused.unknown', { contest })
  }
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
  /** The event's name when it is not one of the two Field Days (`contestName`). */
  label?: string,
): FdEvent | null {
  if (!startUnix || !endUnix) return null
  return {
    kind,
    startUnix,
    endUnix,
    label: label ?? FD_EVENT_NAMES[kind],
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
