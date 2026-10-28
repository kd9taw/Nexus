// AWARD STATUS — Submitted / Granted per contact, and the "Ready to submit" list per award.
//
// The status lives on the contact in the ADIF fields made for it, CREDIT_SUBMITTED and
// CREDIT_GRANTED, by the award's ADIF Credit code (DXCC, WAS, CQWAZ_MIXED, IOTA, USACA). The
// operator sets it from the contact card and Bulk edit, both through the Bulk edit's own change
// (`bulk_edit_preview` / `bulk_edit_apply`): one run of the history, undone as one, refused unless
// it is made on exactly the contacts the operator confirmed. Only the one code moves — another
// award's, and the award's band credit (DXCC_BAND), stay.
//
// The ready list and its counts come from the station (`get_award_status`), which reads each
// contact by the award fold's own rules; nothing here decides what counts.
import type { AwardKind, AwardStatus, AwardStatusField, ReadyRow } from '../api'
import type { LoggedQso } from '../types'

/** One award whose status the log keeps: its kind, the Bulk-edit field that sets it, its ADIF
 *  Credit code, and its programme's name (an invariant token: DXCC is DXCC in every language). */
export interface StatusAward {
  kind: AwardKind
  field: AwardStatusField
  code: string
  name: string
}

export const STATUS_AWARDS: readonly StatusAward[] = [
  { kind: 'dxcc', field: 'CREDIT_DXCC', code: 'DXCC', name: 'DXCC' },
  { kind: 'was', field: 'CREDIT_WAS', code: 'WAS', name: 'WAS' },
  { kind: 'waz', field: 'CREDIT_CQWAZ_MIXED', code: 'CQWAZ_MIXED', name: 'WAZ' },
  { kind: 'iota', field: 'CREDIT_IOTA', code: 'IOTA', name: 'IOTA' },
  { kind: 'usaca', field: 'CREDIT_USACA', code: 'USACA', name: 'USA-CA' },
]

/** The award a Bulk-edit field sets the status of, or undefined for another field. */
export function statusAwardOf(field: string): StatusAward | undefined {
  return STATUS_AWARDS.find((a) => a.field === field)
}

export type CreditStatus = 'SUBMITTED' | 'GRANTED'

/** A contact's status for an award, by the award's exact Credit code: Granted, Submitted, or null. */
export function statusOf(q: Pick<LoggedQso, 'creditGranted' | 'creditSubmitted'>, code: string): CreditStatus | null {
  const holds = (codes?: string[]) => (codes ?? []).some((c) => c.toUpperCase() === code)
  if (holds(q.creditGranted)) return 'GRANTED'
  if (holds(q.creditSubmitted)) return 'SUBMITTED'
  return null
}

const p2 = (n: number) => String(n).padStart(2, '0')

/** ADIF's QSO_DATE (YYYYMMDD) and TIME_ON (HHMM), UTC. */
function adifWhen(whenUnix: number): [string, string] {
  const d = new Date(whenUnix * 1000)
  return [
    `${d.getUTCFullYear()}${p2(d.getUTCMonth() + 1)}${p2(d.getUTCDate())}`,
    `${p2(d.getUTCHours())}${p2(d.getUTCMinutes())}`,
  ]
}

/** One CSV cell: quoted where it holds a comma, a quote or a line break. */
function cell(v: string): string {
  return /[",\r\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v
}

/** The ready list as CSV for an application: a header of ADIF-style tokens (the same in every
 *  language, as a spreadsheet or an award desk reads them), then one line per entity. */
export function readyCsv(rows: readonly ReadyRow[]): string {
  const lines = ['ENTITY,CALL,QSO_DATE,TIME_ON,BAND,MODE,CONFIRMED_BY']
  for (const r of rows) {
    const [date, time] = adifWhen(r.whenUnix)
    lines.push([r.entity, r.call, date, time, r.band, r.mode, r.confirmedBy.join('+')].map(cell).join(','))
  }
  return lines.join('\r\n') + '\r\n'
}

function esc(v: string): string {
  return v.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

/** The words a printable list is headed with, in the operator's language. */
export interface PrintLabels {
  title: string
  summary: string
  entity: string
  call: string
  date: string
  band: string
  mode: string
  confirmedBy: string
  /** A confirmation channel ('card', 'lotw', 'eqsl') by name. */
  channel: (c: string) => string
}

/** The ready list as a page to print: a self-contained HTML document (no script, no network)
 *  with a table sized for paper. */
export function readyHtml(rows: readonly ReadyRow[], labels: PrintLabels): string {
  const head = [labels.entity, labels.call, labels.date, labels.band, labels.mode, labels.confirmedBy]
  const body = rows
    .map((r) => {
      const d = new Date(r.whenUnix * 1000)
      const when = `${d.getUTCFullYear()}-${p2(d.getUTCMonth() + 1)}-${p2(d.getUTCDate())} ${p2(d.getUTCHours())}:${p2(d.getUTCMinutes())}Z`
      const cells = [r.entity, r.call, when, r.band, r.mode, r.confirmedBy.map(labels.channel).join(' + ')]
      return `<tr>${cells.map((c) => `<td>${esc(c)}</td>`).join('')}</tr>`
    })
    .join('\n')
  return `<!doctype html>
<html><head><meta charset="utf-8"><title>${esc(labels.title)}</title>
<style>
body{font:11pt/1.35 system-ui,sans-serif;margin:16mm;color:#000;background:#fff}
h1{font-size:14pt;margin:0 0 4pt}p{margin:0 0 10pt}
table{border-collapse:collapse;width:100%}th,td{border:1px solid #888;padding:2pt 5pt;text-align:left}
th{background:#eee}tr{break-inside:avoid}
@page{margin:12mm}
</style></head><body>
<h1>${esc(labels.title)}</h1>
<p>${esc(labels.summary)}</p>
<table><thead><tr>${head.map((h) => `<th>${esc(h)}</th>`).join('')}</tr></thead>
<tbody>
${body}
</tbody></table>
</body></html>
`
}

/** A file name for an award's list: `nexus-<award>-ready-<YYYYMMDD>.<ext>`. */
export function readyFileName(award: AwardKind, ext: 'csv' | 'html', nowUnix: number): string {
  return `nexus-${award}-ready-${adifWhen(nowUnix)[0]}.${ext}`
}

/** The ids the list names, once each, in list order: what "Mark submitted" changes. */
export function readyIds(status: Pick<AwardStatus, 'ready'>): string[] {
  return [...new Set(status.ready.map((r) => r.id))]
}
