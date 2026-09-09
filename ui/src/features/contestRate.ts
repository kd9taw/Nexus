// ---------------------------------------------------------------------------
// THE OPERATOR'S RATE METER — contacts per hour, from the contest log's timestamps.
//
// A contester watches rate to make one decision: keep running this frequency, or go
// search-and-pounce. Three readings are conventional, and N1MM Logger+ — the logger
// this will be compared against — shows four of them in its Info window: "the rate for
// the last 10 QSOs, the last 100 QSOs, the last hour, and the interval since the start
// of the current clock hour" (n1mmwp.hamdocs.com/manual-windows/info-window/, read
// 2026-09-09).
//
// ⚠️ **N1MM DOES NOT PUBLISH ITS ARITHMETIC.** The manual names the four readings and
// stops. The only open implementation of them is `not1mm` (K6GTE's re-implementation),
// whose rate window computes, for each window, the minutes between the OLDEST and
// NEWEST contact in it and then `(60 / timespan) * count`. That is a re-implementation
// by a different author, not a specification, so it is a READING of the convention and
// not the convention itself — and its arithmetic has two properties this module
// deliberately does not copy:
//
//   1. **`count / span` counts a boundary twice.** Ten contacts one minute apart span
//      nine minutes and contain NINE completed intervals, not ten. `count / span`
//      therefore reports 66.7/h where the operator worked 60/h — about +11% at the
//      10-contact window (+1% at 100, where it hardly matters). This module divides by
//      the INTERVALS, so the sentence behind the number is true: "nine contacts since
//      the oldest of your last ten, in nine minutes, is 60 an hour."
//
//   2. **A span between two logged contacts never reaches NOW, so it freezes.** An
//      operator who runs at 90/h and then stops for two hours would keep reading 90/h
//      off a frozen span — a rate meter saying the run is alive while the band is dead.
//      Every window here is measured from its oldest contact TO NOW, so the denominator
//      keeps growing while nothing is logged and the reading decays on its own. That is
//      the whole point of watching the meter: it is supposed to tell you the run is
//      dying.
//
// The reading, stated once: **since your Nth most recent contact you have logged N-1
// more; project that forward an hour.** Same definition for both windows, and the
// rolling-hour tile is the same statement with the window fixed at 60 minutes — where
// it needs no projection at all, because a count of contacts in the last 60 minutes IS
// a rate per hour.
//
// ⚠️ WHY A ROLLING HOUR AND NOT THE CLOCK HOUR. N1MM offers both. A clock-hour count
// reads 7 at five past the hour for an operator running 90/h, which is the shape of
// misleading this whole module is written to avoid; the rolling hour always describes a
// full hour of real operating time. The clock-hour reading is the one an operator uses
// to fill in a paper rate sheet, which Nexus does not have.
//
// This is pure: the caller supplies `nowUnix`, so the readings are testable and the
// component owns the clock.
// ---------------------------------------------------------------------------

import type { FieldDayQso } from '../types'

const HOUR_SECS = 3600

/** The two contact-count windows, in display order. Nominal sizes — a log shorter than
 *  one of them is read over what it has, and says so (`sample`). */
export const RATE_WINDOWS = [10, 100] as const

export interface RateWindow {
  /** How many contacts actually backed this reading. **The label shows THIS**, not the
   *  nominal window: "Last 3" over three contacts is a true statement, where "Last 10"
   *  over three contacts is a fabricated one, and a fabricated rate is worse than none. */
  sample: number
  /** Contacts per hour, or `null` when the window cannot answer honestly — fewer than
   *  two timestamped contacts (there is no interval to measure), or a `now` that does
   *  not come after the oldest of them. */
  perHour: number | null
}

export interface RateReadings {
  /** One reading per entry of `RATE_WINDOWS`, in the same order. */
  windows: RateWindow[]
  /** Contacts logged in the rolling last 60 minutes. A plain count, never a projection,
   *  so `0` is a real answer and not a missing one. */
  lastHour: number
}

/**
 * Timestamps from the log, oldest first.
 *
 * Rows with no `whenUnix` are SKIPPED, not defaulted: the field is absent on rows logged
 * before it existed, and a missing timestamp treated as 0 (or as now) would drag a window
 * to 1970 or fake a contact in this second. The sort is not decoration — a club merge
 * interleaves logs from several hosts, so append order is not time order.
 */
function stampsAscending(log: readonly FieldDayQso[]): number[] {
  const out: number[] = []
  for (const q of log) {
    const t = q.whenUnix
    if (typeof t === 'number' && Number.isFinite(t) && t > 0) out.push(t)
  }
  return out.sort((a, b) => a - b)
}

/** One window's reading. `size` is nominal; a shorter log is read over what it has. */
function windowRate(ascending: readonly number[], size: number, nowUnix: number): RateWindow {
  const slice = ascending.slice(Math.max(0, ascending.length - size))
  const sample = slice.length
  // One contact is not a rate. Two contacts one second apart are, and the number is
  // enormous and honest — it decays within seconds because the denominator is `now`.
  if (sample < 2) return { sample, perHour: null }
  const elapsed = nowUnix - slice[0]
  // A non-positive elapsed means the oldest of the window is at or after `now` — a clock
  // that stepped back, or a row stamped in the future. Report nothing rather than a
  // negative or infinite rate.
  if (elapsed <= 0) return { sample, perHour: null }
  return { sample, perHour: ((sample - 1) * HOUR_SECS) / elapsed }
}

/** Contacts in the rolling hour ending at `nowUnix`. Future-stamped rows are excluded
 *  for the same reason `windowRate` refuses them: they are not contacts made this hour. */
function rollingHour(ascending: readonly number[], nowUnix: number): number {
  const cut = nowUnix - HOUR_SECS
  let n = 0
  for (const t of ascending) if (t > cut && t <= nowUnix) n++
  return n
}

/** The three readings, from the contest log and the current time (Unix seconds). */
export function contestRate(log: readonly FieldDayQso[], nowUnix: number): RateReadings {
  const ascending = stampsAscending(log)
  return {
    windows: RATE_WINDOWS.map((size) => windowRate(ascending, size, nowUnix)),
    lastHour: rollingHour(ascending, nowUnix),
  }
}
