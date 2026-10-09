// ⭐ REMOVING THE NEWEST CONTEST CONTACT — the one place the contest strip (Ctrl+D, Remove last)
// and the contest screen (Remove on the newest row, the Removed list) read, so both name a
// contact, and word an answer, the same way.
//
// The ENGINE decides; this only names. The newest contact is the log's trailing rows sharing
// the last row's call and time — a county line is ONE contact, one row per county — which is
// the engine's own rule applied to the rows the snapshot already carries. The engine refuses a
// removal naming a contact that is no longer the newest (`changed`), so a stale snapshot can
// never take out the wrong one.
//
// Nothing here keys, unkeys or stops a transmission, and nothing here moves focus.

import type { ContestRemovalAnswer } from '../api'
import type { FieldDayQso } from '../types'
import { t } from '../i18n'

/** The press-twice window: a second Ctrl+D or click within it removes the contact named. */
export const REMOVE_CONFIRM_MS = 5000

/** The services a merged copy can be uploaded to, by the engine's ids — product names, never
 *  translated. */
const SERVICE_NAMES: Record<string, string> = {
  lotw: 'LoTW',
  qrz: 'QRZ',
  clublog: 'Club Log',
  eqsl: 'eQSL',
}

export interface NewestContact {
  call: string
  whenUnix: number
  rows: FieldDayQso[]
}

/** The newest contact in a contest log as the snapshot carries it, or null for an empty log. */
export function newestContact(log: readonly FieldDayQso[] | undefined): NewestContact | null {
  const last = log?.[log.length - 1]
  if (!log || !last) return null
  let first = log.length - 1
  while (first > 0 && log[first - 1].call === last.call && log[first - 1].whenUnix === last.whenUnix) first--
  return { call: last.call, whenUnix: last.whenUnix ?? 0, rows: log.slice(first) }
}

/** HH:MM, UTC — a contest log's own clock. */
export function utcHhmm(unix: number): string {
  const d = new Date(unix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`
}

/** What a contact received, as the log table shows it: Field Day's class and section, or every
 *  received slot in order — and for a county line each slot's counties joined the way the
 *  station sent them (`599 COOK/DUPG`). Wire values, never translated. */
function received(rows: readonly FieldDayQso[]): string {
  const first = rows[0]
  if (!first) return ''
  if (!first.rcvd?.length) return [first.class, first.section].filter((v) => v).join(' ')
  return first.rcvd
    .map((_, i) => [...new Set(rows.map((r) => r.rcvd?.[i] ?? ''))].filter((v) => v).join('/'))
    .filter((v) => v)
    .join(' ')
}

/** A contact as the strip names it before anything happens: "K9AAA · 20m CW · 14:32 · 599 COOK". */
export function contactLabel(rows: readonly FieldDayQso[]): string {
  const q = rows[0]
  if (!q) return ''
  const exchange = received(rows)
  const values = {
    call: q.call,
    band: q.band,
    mode: q.submode || q.mode || '',
    time: q.whenUnix ? utcHhmm(q.whenUnix) : '',
  }
  return exchange
    ? t('logEntry.remove.contact', { ...values, exchange })
    : t('logEntry.remove.contactBare', values)
}

/** Ctrl+D, as N1MM binds it: Ctrl alone — never with Alt, Shift or Cmd. A held key's auto-repeat
 *  is the caller's to ignore. */
export function isRemoveKey(e: KeyboardEvent): boolean {
  return e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey && e.key.toLowerCase() === 'd'
}

/** A bare modifier: pressing Ctrl again for the second Ctrl+D is not "any other key". */
export function isModifierKey(e: KeyboardEvent): boolean {
  return e.key === 'Control' || e.key === 'Shift' || e.key === 'Alt' || e.key === 'Meta'
}

/** What the strip says after Ctrl+D pressed twice: the contact removed and every place it had
 *  already gone — Nexus takes it back from none of them — or why nothing was removed. */
export function removalText(answer: ContestRemovalAnswer, label: string): string {
  if (answer.outcome === 'refused') {
    switch (answer.refusal) {
      case 'empty':
        return t('logEntry.remove.empty')
      case 'clubSync':
        return t('logEntry.remove.clubSync')
      default:
        return t('logEntry.remove.changed')
    }
  }
  const parts = [t('logEntry.remove.done', { contact: label })]
  if (answer.outcome === 'removed') {
    const s = answer.sentTo
    const places = [
      s.n3fjp ? t('logEntry.remove.sent.n3fjp', { host: s.n3fjp }) : '',
      s.n1mm ? t('logEntry.remove.sent.n1mm') : '',
      s.wsjtx ? t('logEntry.remove.sent.wsjtx') : '',
    ].filter((p) => p)
    if (places.length > 0) parts.push(t('logEntry.remove.sentTo', { places: places.join(', ') }))
    if (s.logbook) {
      const services = s.uploaded.map((id) => SERVICE_NAMES[id] ?? id)
      parts.push(
        services.length > 0
          ? t('logEntry.remove.logbookUploaded', { services: services.join(', ') })
          : t('logEntry.remove.logbook'),
      )
    }
  }
  return parts.join(' ')
}

/** What the contest screen says after a Restore. */
export function restoreText(answer: ContestRemovalAnswer, call: string): string {
  if (answer.outcome !== 'refused') return t('fieldDay.removed.restored', { call })
  switch (answer.refusal) {
    case 'workedAgain':
      return t('fieldDay.removed.workedAgain', { call })
    case 'clubSync':
      return t('fieldDay.removed.clubSync')
    default:
      return t('fieldDay.removed.notRemoved')
  }
}
