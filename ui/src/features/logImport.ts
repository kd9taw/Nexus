// ---------------------------------------------------------------------------
// THE LOGBOOK'S IMPORT, LOOKED AT FIRST AND ACCOUNTED FOR AFTER — the wire shapes of the engine's
// preview and no-loss report (`tempo_core::logbook::import`), their words, and the report as the
// text file the operator saves.
//
// The engine reads the file (ADIF, ADX or Cabrillo) and measures what becomes of every field; this
// module only words it. Field names, values, contest ids and counts are invariant tokens and are
// never translated; each kind of note, reason and heading is a catalog entry, named by a literal key
// in a switch so the catalog guard can read every call site.
// ---------------------------------------------------------------------------
import { t } from '../i18n'
import { failureText } from '../toast'
import type { ImportStats } from '../types'
import { fmtUtc } from './logQuery'

export type ImportFormat = 'adif' | 'adx' | 'cabrillo'

export type ImportNoteKind =
  | 'noCall'
  | 'unterminated'
  | 'repeated'
  | 'cutValue'
  | 'badDate'
  | 'badTime'
  | 'badValue'
  | 'unknownBand'
  | 'unknownMode'
  | 'unreadableLine'
  | 'templateMismatch'
  | 'unknownContest'
  | 'modeUnresolved'
  | 'excludedQso'
  | 'badXml'
  | 'tooManyFields'

export interface ImportNote {
  kind: ImportNoteKind
  /** 1-based: the record (ADIF, ADX) or the line (Cabrillo); 0 for the file's header. */
  at: number
  /** The file's own text the note is about. */
  text: string
}

export interface ImportNoteGroup {
  kind: ImportNoteKind
  count: number
  examples: ImportNote[]
}

export type UnmappedReason = 'header' | 'exchangeText' | 'transmitterId' | 'fieldName'

export interface ImportUnmapped {
  what: string
  side: 'sent' | 'rcvd' | null
  count: number
  reason: UnmappedReason
}

export interface ImportPreview {
  format: ImportFormat
  template: string | null
  records: number
  contacts: number
  firstUnix: number | null
  lastUnix: number | null
  new: number
  duplicates: number
  updates: number
  notes: ImportNoteGroup[]
  coverage: { field: string; count: number }[]
  unmapped: ImportUnmapped[]
}

export type DropReason =
  | 'noContact'
  | 'unterminated'
  | 'repeated'
  | 'empty'
  | 'invalid'
  | 'default'
  | 'rederived'
  | 'notWritten'
  | 'tooManyFields'

export interface FieldAccount {
  field: string
  occurrences: number
  stored: number
  normalized: number
  passthrough: number
  dropped: number
  reasons: { reason: DropReason; count: number }[]
  examples: string[]
}

export interface ImportReport {
  format: ImportFormat
  template: string | null
  records: number
  contacts: number
  added: number
  duplicates: number
  updated: number
  newOnly: boolean
  fields: FieldAccount[]
  unmapped: ImportUnmapped[]
  notes: ImportNoteGroup[]
  accounted: boolean
}

export interface ImportDone {
  stats: ImportStats
  report: ImportReport
}

/** The largest file an import reads, in bytes: the engine's own limit (`import::MAX_BYTES`), checked
 *  here as well so that a larger file is refused before the webview reads it into memory. */
export const IMPORT_MAX_BYTES = 200_000_000

/** Why a file of `size` bytes is not imported, or null when it can be. */
export function importTooLarge(size: number): string | null {
  if (size <= IMPORT_MAX_BYTES) return null
  return t('logbook.import.tooLarge', { size: (size / 1e6).toFixed(1), max: IMPORT_MAX_BYTES / 1e6 })
}

/** The engine's refusal of a file naming more distinct fields than it reads (`import::TOO_MANY_NAMES`). */
export const IMPORT_TOO_MANY_NAMES = 'tooManyNames'

/** What a failed preview or import says: the engine's refusal in the operator's words, else `fallback` with its detail. */
export function importFailure(fallback: string, err: unknown): string {
  return err === IMPORT_TOO_MANY_NAMES ? t('logbook.import.tooManyNames') : failureText(fallback, err)
}

export function formatName(f: ImportFormat): string {
  switch (f) {
    case 'adif':
      return t('logbook.import.format.adif')
    case 'adx':
      return t('logbook.import.format.adx')
    case 'cabrillo':
      return t('logbook.import.format.cabrillo')
  }
}

export function noteLabel(k: ImportNoteKind): string {
  switch (k) {
    case 'noCall':
      return t('logbook.import.note.noCall')
    case 'unterminated':
      return t('logbook.import.note.unterminated')
    case 'repeated':
      return t('logbook.import.note.repeated')
    case 'cutValue':
      return t('logbook.import.note.cutValue')
    case 'badDate':
      return t('logbook.import.note.badDate')
    case 'badTime':
      return t('logbook.import.note.badTime')
    case 'badValue':
      return t('logbook.import.note.badValue')
    case 'unknownBand':
      return t('logbook.import.note.unknownBand')
    case 'unknownMode':
      return t('logbook.import.note.unknownMode')
    case 'unreadableLine':
      return t('logbook.import.note.unreadableLine')
    case 'templateMismatch':
      return t('logbook.import.note.templateMismatch')
    case 'unknownContest':
      return t('logbook.import.note.unknownContest')
    case 'modeUnresolved':
      return t('logbook.import.note.modeUnresolved')
    case 'excludedQso':
      return t('logbook.import.note.excludedQso')
    case 'badXml':
      return t('logbook.import.note.badXml')
    case 'tooManyFields':
      return t('logbook.import.note.tooManyFields')
  }
}

export function unmappedLabel(r: UnmappedReason): string {
  switch (r) {
    case 'header':
      return t('logbook.import.unmapped.header')
    case 'exchangeText':
      return t('logbook.import.unmapped.exchangeText')
    case 'transmitterId':
      return t('logbook.import.unmapped.transmitterId')
    case 'fieldName':
      return t('logbook.import.unmapped.fieldName')
  }
}

export function reasonLabel(r: DropReason): string {
  switch (r) {
    case 'noContact':
      return t('logbook.import.reason.noContact')
    case 'unterminated':
      return t('logbook.import.reason.unterminated')
    case 'repeated':
      return t('logbook.import.reason.repeated')
    case 'empty':
      return t('logbook.import.reason.empty')
    case 'invalid':
      return t('logbook.import.reason.invalid')
    case 'default':
      return t('logbook.import.reason.default')
    case 'rederived':
      return t('logbook.import.reason.rederived')
    case 'notWritten':
      return t('logbook.import.reason.notWritten')
    case 'tooManyFields':
      return t('logbook.import.reason.tooManyFields')
  }
}

/** Where a note is, in the file's own terms: a record, a line, or the header. */
export function noteAt(format: ImportFormat, at: number): string {
  if (at === 0) return t('logbook.import.at.header')
  return format === 'cabrillo' ? t('logbook.import.at.line', { n: at }) : t('logbook.import.at.record', { n: at })
}

/** An unmapped item's name: an exchange column carries its side. */
export function unmappedName(u: ImportUnmapped): string {
  if (u.side === 'sent') return t('logbook.import.side.sent', { what: u.what })
  if (u.side === 'rcvd') return t('logbook.import.side.rcvd', { what: u.what })
  return u.what
}

/** The UTC day of a Unix time. */
export const utcDay = (unix: number): string => fmtUtc(unix).slice(0, 10)

/** How many field values the import did not write back. */
export const droppedCount = (r: ImportReport): number => r.fields.reduce((n, f) => n + f.dropped, 0)

/** The report as the text file the operator saves: every field, and what became of it. */
export function reportText(r: ImportReport, file: string, when: Date): string {
  const L: string[] = []
  const kv = (label: string, value: string | number) => L.push(`${label}: ${value}`)
  L.push(t('logbook.import.report.heading'))
  L.push('')
  kv(t('logbook.import.report.file'), file)
  kv(t('logbook.import.report.when'), when.toISOString().replace('T', ' ').slice(0, 19) + 'Z')
  kv(t('logbook.import.report.format'), formatName(r.format))
  if (r.template) kv(t('logbook.import.report.template'), r.template)
  kv(t('logbook.import.report.records'), r.records)
  kv(t('logbook.import.report.contacts'), r.contacts)
  kv(t('logbook.import.report.added'), r.added)
  kv(t('logbook.import.report.duplicates'), r.duplicates)
  kv(t('logbook.import.report.updated'), r.updated)
  if (r.newOnly) L.push(t('logbook.import.report.newOnly'))
  L.push('')
  L.push(r.accounted ? t('logbook.import.report.accounted') : t('logbook.import.report.notAccounted'))
  L.push(t('logbook.import.report.basis'))
  L.push('')
  L.push(`== ${t('logbook.import.report.fields')} ==`)
  L.push(t('logbook.import.report.legend'))
  for (const f of r.fields) {
    L.push(
      `${f.field}: ${f.occurrences} = ${f.stored} ${t('logbook.import.report.col.stored')} + ${f.normalized} ${t(
        'logbook.import.report.col.normalized',
      )} + ${f.passthrough} ${t('logbook.import.report.col.passthrough')} + ${f.dropped} ${t(
        'logbook.import.report.col.dropped',
      )}`,
    )
    for (const reason of f.reasons) L.push(`    ${t('logbook.import.report.col.dropped')} ${reason.count}: ${reasonLabel(reason.reason)}`)
    for (const e of f.examples) L.push(`    ${e}`)
  }
  if (r.unmapped.length) {
    L.push('')
    L.push(`== ${t('logbook.import.report.unmapped')} ==`)
    for (const u of r.unmapped) L.push(`${unmappedName(u)} ×${u.count}: ${unmappedLabel(u.reason)}`)
  }
  if (r.notes.length) {
    L.push('')
    L.push(`== ${t('logbook.import.report.notes')} ==`)
    for (const g of r.notes) {
      L.push(`${noteLabel(g.kind)}: ${g.count}`)
      for (const e of g.examples) L.push(`    ${noteAt(r.format, e.at)}: ${e.text}`)
    }
  }
  return L.join('\n') + '\n'
}
