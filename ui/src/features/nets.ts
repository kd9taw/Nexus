// Net reminder scheduling — pure time math over a memory's NetInfo.
//
// Nets meet on fixed UTC days + time (DST-proof). Reminders are OPT-IN PER NET
// (net.alertEnabled) — never a firehose — and fire once, a lead time before the
// net starts. All math is in UTC; the UI renders local.

import { type Memory, type NetInfo } from './memories'

const DAY_MS = 86_400_000

/** The next UTC epoch-ms at which this net starts, at or after `fromMs`.
 * null when the schedule is empty or malformed. */
export function nextNetStart(net: NetInfo, fromMs: number): number | null {
  if (!net.days.length) return null
  const match = /^(\d{1,2}):(\d{2})$/.exec(net.utcTime)
  if (!match) return null
  const hh = Number(match[1])
  const mm = Number(match[2])
  if (hh > 23 || mm > 59) return null
  // Walk up to 8 days forward (covers "later today" through "same day next week").
  for (let d = 0; d < 8; d++) {
    const probe = new Date(fromMs + d * DAY_MS)
    if (!net.days.includes(probe.getUTCDay())) continue
    const start = Date.UTC(
      probe.getUTCFullYear(),
      probe.getUTCMonth(),
      probe.getUTCDate(),
      hh,
      mm,
      0,
      0,
    )
    if (start >= fromMs) return start
  }
  return null
}

export interface NetReminder {
  memory: Memory
  /** UTC epoch-ms the net starts. */
  startMs: number
}

/** The nets to remind about right now: alert-enabled, with a start within the
 * next `lead` minutes (per-net). A stable per-occurrence key lets the caller
 * fire each reminder exactly once. */
export function dueNetReminders(memories: Memory[], nowMs: number): NetReminder[] {
  const due: NetReminder[] = []
  for (const m of memories) {
    if (!m.net?.alertEnabled) continue
    const start = nextNetStart(m.net, nowMs)
    if (start == null) continue
    const leadMs = Math.max(0, m.net.alertLeadMin ?? 10) * 60_000
    if (start - nowMs <= leadMs) due.push({ memory: m, startMs: start })
  }
  return due
}

/** Stable identity for a single net occurrence — dedupe key so a 30 s poll fires
 * one reminder per meeting, not one every tick. */
export function reminderKey(r: NetReminder): string {
  return `${r.memory.id}:${r.startMs}`
}

/** "in 8 min" / "now" — friendly lead phrasing for the reminder toast. */
export function untilPhrase(startMs: number, nowMs: number): string {
  const min = Math.round((startMs - nowMs) / 60_000)
  if (min <= 0) return 'now'
  if (min === 1) return 'in 1 min'
  return `in ${min} min`
}
