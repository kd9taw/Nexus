// UTC date and time for the hand-logging forms — the Logbook's edit form and the log strip's
// "Log a contact from another radio" override.
//
// A log is UTC and 24-hour. These forms used native `datetime-local` / `time` inputs, and
// WebView2 draws those in the OS locale: on a 12-hour Windows PC a contact at 00:58 UTC read
// "12:58 AM", the operator "corrected" it, and a right record moved 12 hours (#280). So the time
// is a plain text box, and everything here is string arithmetic in UTC — no locale formatter, and
// no `Date` parse of a zoneless string, which would read it as LOCAL time and shift it by the
// operator's offset.

/** What the time box asks for. Format tokens: the same letters in every language. */
export const UTC_TIME_FORMATS = { short: 'HH:MM', long: 'HH:MM:SS' } as const

/** A 24-hour UTC time as the box takes it: H:MM, HH:MM or HH:MM:SS — hours 0–23, minutes and
 *  seconds 0–59. Null for anything else: 25:00, 12:60, 1:5, "12:58 AM". */
export function parseUtcTime(v: string): { h: number; m: number; s: number | null } | null {
  const t = /^(\d{1,2}):(\d{2})(?::(\d{2}))?$/.exec(v.trim())
  if (!t) return null
  const h = +t[1]
  const m = +t[2]
  const s = t[3] === undefined ? null : +t[3]
  if (h > 23 || m > 59 || (s !== null && s > 59)) return null
  return { h, m, s }
}

/** Unix seconds for a UTC date (YYYY-MM-DD) and a time `parseUtcTime` takes, or null when
 *  either is not a real one. A time given without seconds is on the minute. */
export function utcDateTimeToUnix(date: string, time: string): number | null {
  const d = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date.trim())
  const t = parseUtcTime(time)
  if (!d || !t) return null
  const [y, mo, day] = [+d[1], +d[2], +d[3]]
  const ms = Date.UTC(y, mo - 1, day, t.h, t.m, t.s ?? 0)
  // Date.UTC rolls an impossible date forward (Feb 30 → Mar 2): refuse it rather than log a day
  // nobody typed.
  const back = new Date(ms)
  if (back.getUTCFullYear() !== y || back.getUTCMonth() !== mo - 1 || back.getUTCDate() !== day) return null
  return Math.floor(ms / 1000)
}

const pad = (n: number) => String(n).padStart(2, '0')

/** An instant's UTC date as YYYY-MM-DD — the date box's value. */
export function utcDate(whenUnix: number): string {
  const d = new Date(whenUnix * 1000)
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`
}

/** An instant's UTC time for the box: HH:MM, or HH:MM:SS when the seconds are not zero — so an
 *  edit that leaves the time alone saves the instant it opened with. */
export function utcTime(whenUnix: number): string {
  const d = new Date(whenUnix * 1000)
  const hm = `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`
  return d.getUTCSeconds() === 0 ? hm : `${hm}:${pad(d.getUTCSeconds())}`
}
