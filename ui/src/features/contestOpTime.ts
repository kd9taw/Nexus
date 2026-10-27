/**
 * ⭐ **Operating time** — how long a contest station was on, read off its log's timestamps by the
 * sponsors' own rule: a break of `minOffSec` or more between two consecutive contacts is OFF time,
 * and the rest of the span from the first contact to the last is ON time. The minimum is the
 * contest's own (`offTimeRule`), 30 minutes where its rules name none.
 *
 * Every row with a time counts, a dupe and a contact in a banned mode included: the station was on
 * the air for them. A removed contact is not in the log and counts for nothing.
 */
export const OFF_TIME_MIN_SEC = 30 * 60

/**
 * ⭐ THE MINIMUM BREAK EACH CONTEST'S OWN RULES NAME, by contest id. The rules data the station
 * downloads carries no off-time field, and republishing that file is its own decision, so the values
 * live here, each read off the sponsor's published rules (2026-10-26):
 *
 * - CQ WW CW, SSB and RTTY — 60 min. cqww.com/rules and cqwwrtty.com/rules, V.B.1 (Classic
 *   Overlay): "off times are a minimum of 60 minutes during which no QSO is logged". It is the only
 *   off time those rules define; a Single Operator entry otherwise has "no limit on operating time".
 * - CQ WPX CW and SSB — 60 min. cqwpx.com/rules, II (Period of Operation): "Single Operator stations
 *   may operate 36 of the 48 hours – off times must be a minimum of 60 minutes during which no QSO is
 *   logged."
 * - ARRL Sweepstakes CW and Phone — 30 min. ARRL November Sweepstakes Rules, Special Rule 1.1: "All
 *   stations may operate no more than 24 of the 30 hours … Off times must be at least 30 minutes
 *   long." (contests.arrl.org/ContestRules/SS-Rules.pdf).
 * - ARRL RTTY Roundup — 30 min. RTTY-RU-Rules.pdf v3.0b, Special Rule 1.1: "Single Operator
 *   stations may operate up to 24 hours out of the 30-hour contest period. … each off time must be
 *   at least 30 minutes long until 24 hours have been completed." (1.2's "one or two breaks" is not
 *   counted here.)
 *
 * Every other contest the station runs (both Field Days, the ARRL VHF contests, the QSO parties) has
 * no minimum on record here and uses `OFF_TIME_MIN_SEC`, and a surface that states the rule says so.
 */
const OFF_TIME_RULES: Readonly<Record<string, number>> = {
  cqww_cw: 60 * 60,
  cqww_ssb: 60 * 60,
  cqww_rtty: 60 * 60,
  cqwpx_cw: 60 * 60,
  cqwpx_ssb: 60 * 60,
  arrlss_cw: 30 * 60,
  arrlss_ssb: 30 * 60,
  arrlrtty: 30 * 60,
}

/** The break that counts as off time in contest `event`, and whether its rules name it. */
export function offTimeRule(event: string | undefined): { minOffSec: number; fromRules: boolean } {
  const sec = event ? OFF_TIME_RULES[event] : undefined
  return sec === undefined ? { minOffSec: OFF_TIME_MIN_SEC, fromRules: false } : { minOffSec: sec, fromRules: true }
}

export interface OperatingTime {
  onSec: number
  offSec: number
  /** How many breaks made the off time. */
  breaks: number
}

/** `null` while the log holds no contact with a time. */
export function operatingTime(
  log: readonly { whenUnix?: number }[],
  minOffSec: number = OFF_TIME_MIN_SEC,
): OperatingTime | null {
  const times = log
    .map((q) => q.whenUnix ?? 0)
    .filter((t) => t > 0)
    .sort((a, b) => a - b)
  if (times.length === 0) return null
  let onSec = 0
  let offSec = 0
  let breaks = 0
  for (let i = 1; i < times.length; i++) {
    const gap = times[i] - times[i - 1]
    if (gap >= minOffSec) {
      offSec += gap
      breaks++
    } else {
      onSec += gap
    }
  }
  return { onSec, offSec, breaks }
}

/** `H:MM`, whole minutes, hours unbounded (a 48-hour contest reads `36:07`). */
export function formatOpTime(sec: number): string {
  const mins = Math.floor(sec / 60)
  return `${Math.floor(mins / 60)}:${String(mins % 60).padStart(2, '0')}`
}
