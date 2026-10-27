// ⭐ EDITING A LOGGED CONTEST CONTACT — the one place the contest screen's Edit and the strip's
// Ctrl+Y read, so both open a contact, judge an edit and word the answer the same way.
//
// The ENGINE decides (`contest_edit`): it names the contact by its call and time — a county line
// is one contact, one row per county — keeps its time, serial and number, and refuses one that is
// no longer there or no longer reads as the edit opened it, or any edit while club sync runs. This turns a contact's rows into the form's
// values and the form back into the edit, judging each box with the strip's own functions
// (`features/contestSlots`), so an edit cannot write what the strip would refuse to log.
//
// Nothing here keys, unkeys or stops a transmission.

import type { ContestContactEdit, ContestEditAnswer } from '../api'
import type { ContestFieldSpec, FieldDayQso } from '../types'
import { t } from '../i18n'
import { fdFieldOk, fdSlotValue, fdVerdict } from './contestSlots'
import { countyLineDomain, isCountyLine, readCountyLine } from './countyLine'
import { SERVICE_NAMES, contactLabel } from './contestRemoval'

/** Ctrl+Y (edit the last contact): Ctrl alone — never with Alt, Shift or Cmd.
 *  A held key's auto-repeat is the caller's to ignore. */
export function isEditKey(e: KeyboardEvent): boolean {
  return e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey && e.key.toLowerCase() === 'y'
}

/** What a contact's edit can change on its rows, as one string to compare: the fields `editForm`
 *  and `editOf` read and write. The engine compares the same ones (`contest_edit`). */
export function contactText(rows: readonly FieldDayQso[]): string {
  return JSON.stringify(rows.map((q) => [q.call, q.band, q.mode, q.submode, q.class, q.section, q.rcvd]))
}

/** What a host says when the edit's contact is no longer in its window by the call and time
 *  it opened with: every edit keeps a contact's time, so one still at that time had its call
 *  changed elsewhere, and with none at it the contact was removed. */
export function closedText(log: readonly FieldDayQso[], whenUnix: number): string {
  return log.some((q) => q.whenUnix === whenUnix) ? t('contest.edit.changed') : t('contest.edit.gone')
}

/** The modes on the air the form offers besides the contact's own — ADIF mode names, tokens. */
export const EDIT_MODES = ['CW', 'USB', 'LSB', 'FM', 'AM', 'RTTY', 'FT8', 'FT4'] as const

/** What the form holds: the call, each received slot's box by slot id, the band, and the mode
 *  picked (`''` keeps the contact's). */
export interface EditForm {
  call: string
  fields: Record<string, string>
  band: string
  mode: string
}

/** What row `q` received in slot `key`: Field Day's class and section ride the row, every other
 *  contest's slots `rcvd`, in the session's receive order. */
function received(q: FieldDayQso, receives: readonly ContestFieldSpec[], key: string): string {
  if (!q.rcvd?.length) return key === 'CLASS' ? q.class : key === 'SECTION' ? q.section : ''
  const i = receives.findIndex((f) => f.key === key)
  return i < 0 ? '' : (q.rcvd[i] ?? '')
}

/** The form a contact opens with: its call, each received slot (a county line's counties joined
 *  by `/`, in row order, as the strip takes one), its band, and its own mode. */
export function editForm(rows: readonly FieldDayQso[], receives: readonly ContestFieldSpec[]): EditForm {
  const fields: Record<string, string> = {}
  for (const f of receives) {
    const values = rows.map((q) => received(q, receives, f.key))
    fields[f.key] = new Set(values).size > 1 ? values.join('/') : (values[0] ?? '')
  }
  return { call: rows[0]?.call ?? '', fields, band: rows[0]?.band ?? '', mode: '' }
}

/** The modes the form offers: the contact's own first, as the log table names it, whose value
 *  `''` keeps it; then each of `EDIT_MODES` it is not. */
export function modeOptions(q: FieldDayQso | undefined): { value: string; label: string }[] {
  const own = (q?.submode || q?.mode || '').toUpperCase()
  return [
    { value: '', label: own },
    ...EDIT_MODES.filter((m) => m !== own).map((m) => ({ value: m, label: m })),
  ]
}

/** The bands the form offers: the contact's own, then the contest's. */
export function bandOptions(q: FieldDayQso | undefined, bands: readonly string[] | undefined): string[] {
  return [...new Set([q?.band ?? '', ...(bands ?? [])])].filter((b) => b !== '')
}

/** The edit the form makes of `rows` — or why it cannot be saved: the first box the strip would
 *  refuse to log, in the order the boxes are shown and named as they are captioned for `role`,
 *  and a county line given another number of counties than the contact has. `entity` is the
 *  country file's answer for the form's call, which the strip's rule for a QSO party's QTH box
 *  reads. */
export function editOf(
  form: EditForm,
  rows: readonly FieldDayQso[],
  receives: readonly ContestFieldSpec[],
  role?: string,
  entity?: string | null,
): { edit: ContestContactEdit } | { why: string } {
  const call = form.call.replace(/\s+/g, '').toUpperCase()
  if (call === '') return { why: t('contest.edit.needCall') }
  const values: [string, string][] = []
  let line: { key: string; counties: string[] } | undefined
  for (const f of receives) {
    const raw = form.fields[f.key] ?? ''
    const domain = countyLineDomain(f.domains)
    if (!fdFieldOk(f, raw, domain, entity)) return { why: fdVerdict(f, raw, domain, role) }
    const value = fdSlotValue(f, raw, domain)
    if (domain && isCountyLine(value)) {
      const read = readCountyLine(domain, value)
      if (read.ok) line = { key: f.key, counties: read.counties }
    }
    values.push([f.key, value])
  }
  if ((line?.counties.length ?? 1) !== rows.length) {
    return { why: t('contest.edit.lineCount', { count: rows.length }) }
  }
  const at = line
  const vectors = at
    ? at.counties.map((county) =>
        values.map(([k, v]) => [k, k === at.key ? county : v] as [string, string]),
      )
    : [values]
  return { edit: { call, rows: vectors, band: form.band, mode: form.mode } }
}

/** What the screen says after an edit: the contact as it is now, every place it had already gone
 *  (each keeps what it said), and a logbook copy merged before it — or why nothing was edited. */
export function editText(answer: ContestEditAnswer): string {
  if (answer.outcome === 'refused') {
    switch (answer.refusal) {
      case 'incomplete':
        return t('contest.edit.incomplete')
      case 'rows':
        return t('contest.edit.rows')
      case 'satellite':
        return t('contest.edit.satellite')
      case 'club':
        return t('contest.edit.club')
      case 'gone':
        return t('contest.edit.gone')
      default:
        return t('contest.edit.changed')
    }
  }
  const was = answer.before[0]?.call ?? ''
  const parts = [t('contest.edit.done', { was, contact: contactLabel(answer.after) })]
  const s = answer.sentTo
  const places = [
    s.n3fjp ? t('logEntry.remove.sent.n3fjp', { host: s.n3fjp }) : '',
    s.n1mm ? t('logEntry.remove.sent.n1mm') : '',
    s.wsjtx ? t('logEntry.remove.sent.wsjtx') : '',
  ].filter((p) => p)
  if (places.length > 0) parts.push(t('contest.edit.sentTo', { places: places.join(', ') }))
  if (s.logbook) {
    const services = s.uploaded.map((id) => SERVICE_NAMES[id] ?? id)
    parts.push(
      services.length > 0
        ? t('contest.edit.logbookUploaded', { call: was, services: services.join(', ') })
        : t('contest.edit.logbook', { call: was }),
    )
  }
  return parts.join(' ')
}
