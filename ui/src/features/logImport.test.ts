import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { describe, it, expect } from 'vitest'
import { t } from '../i18n'
import { EN } from '../i18n/en'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'
import {
  IMPORT_MAX_BYTES,
  IMPORT_TOO_MANY_NAMES,
  droppedCount,
  importFailure,
  importTooLarge,
  noteLabel,
  reasonLabel,
  reportText,
  unmappedLabel,
  type DropReason,
  type ImportNoteKind,
  type ImportReport,
  type UnmappedReason,
} from './logImport'

const here = dirname(fileURLToPath(import.meta.url))
/** The engine's import module, whose constants and enums the UI states and names. */
const ENGINE = readFileSync(resolve(here, '../../../crates/tempo-core/src/logbook/import.rs'), 'utf8')

const REPORT: ImportReport = {
  format: 'cabrillo',
  template: 'ARRL-SS-CW',
  records: 2,
  contacts: 2,
  added: 1,
  duplicates: 1,
  updated: 0,
  newOnly: true,
  fields: [
    { field: 'BAND', occurrences: 2, stored: 2, normalized: 0, passthrough: 0, dropped: 0, reasons: [], examples: [] },
    { field: 'CALL', occurrences: 2, stored: 2, normalized: 0, passthrough: 0, dropped: 0, reasons: [], examples: [] },
    {
      field: 'FREQ', occurrences: 2, stored: 1, normalized: 0, passthrough: 0, dropped: 1,
      reasons: [{ reason: 'invalid', count: 1 }], examples: ['abc'],
    },
    { field: 'APP_X_Y', occurrences: 2, stored: 0, normalized: 0, passthrough: 2, dropped: 0, reasons: [], examples: [] },
  ],
  unmapped: [{ what: 'PREC', side: 'rcvd', count: 2, reason: 'exchangeText' }],
  notes: [{ kind: 'excludedQso', count: 1, examples: [{ kind: 'excludedQso', at: 7, text: 'K8MM 2024-11-02 2100' }] }],
  accounted: true,
}

describe('the no-loss report as a text file', () => {
  const text = reportText(REPORT, 'ss.log', new Date(Date.UTC(2026, 9, 25, 12, 0, 0)))

  it('names every field of the file, with each of its four counts', () => {
    for (const f of REPORT.fields) {
      expect(text).toContain(
        `${f.field}: ${f.occurrences} = ${f.stored} ${t('logbook.import.report.col.stored')} + ${f.normalized} ${t(
          'logbook.import.report.col.normalized',
        )} + ${f.passthrough} ${t('logbook.import.report.col.passthrough')} + ${f.dropped} ${t('logbook.import.report.col.dropped')}`,
      )
    }
  })

  it('says why a value was dropped, and shows it', () => {
    expect(text).toContain(`${t('logbook.import.report.col.dropped')} 1: ${t('logbook.import.reason.invalid')}`)
    expect(text).toContain('    abc')
  })

  it('lists what has no field of a contact, and the problems, by line', () => {
    expect(text).toContain(`${t('logbook.import.side.rcvd', { what: 'PREC' })} ×2: ${t('logbook.import.unmapped.exchangeText')}`)
    expect(text).toContain(`${t('logbook.import.at.line', { n: 7 })}: K8MM 2024-11-02 2100`)
  })

  it('carries the file, the time, the template and the new-only note', () => {
    expect(text).toContain(`${t('logbook.import.report.file')}: ss.log`)
    expect(text).toContain('2026-10-25 12:00:00Z')
    expect(text).toContain(`${t('logbook.import.report.template')}: ARRL-SS-CW`)
    expect(text).toContain(t('logbook.import.report.newOnly'))
    expect(text).toContain(t('logbook.import.report.accounted'))
    expect(droppedCount(REPORT)).toBe(1)
  })
})

describe('a file too large to import', () => {
  it('is refused with its size and the limit, before it is read', () => {
    expect(importTooLarge(IMPORT_MAX_BYTES)).toBeNull()
    const why = importTooLarge(IMPORT_MAX_BYTES + 500_001)
    expect(why).toBe(t('logbook.import.tooLarge', { size: '200.5', max: 200 }))
    expect(why).toContain('200.5')
  })

  it('is the engine’s own limit', () => {
    const engine = ENGINE.match(/pub const MAX_BYTES: usize = ([0-9_]+);/)
    expect(engine?.[1].replace(/_/g, '')).toBe(String(IMPORT_MAX_BYTES))
  })
})

describe('the field limit a record is read with', () => {
  it('is stated in every language as the engine’s own', () => {
    const max = ENGINE.match(/pub const MAX_FIELDS: usize = ([0-9_]+);/)?.[1].replace(/_/g, '')
    expect(max).toMatch(/^[0-9]+$/)
    const catalogs: Record<string, Record<string, unknown>> = { EN, DE, ES, FR, JA }
    for (const [lang, catalog] of Object.entries(catalogs)) {
      for (const key of ['logbook.import.note.tooManyFields', 'logbook.import.reason.tooManyFields']) {
        const text = String(catalog[key] ?? '')
        // Every number in the sentence, its digit grouping (2,000 · 2.000 · 2 000) taken out.
        const numbers = (text.match(/\d(?:[\d.,\u00a0\u202f ]*\d)?/g) ?? []).map((n) => n.replace(/\D/g, ''))
        expect(numbers.length, `${lang} ${key}`).toBeGreaterThan(0)
        for (const n of numbers) expect(n, `${lang} ${key}: ${text}`).toBe(max)
      }
    }
  })
})

describe('a file naming too many fields', () => {
  it('is refused in every language with the engine’s own limit', () => {
    const max = ENGINE.match(/pub const MAX_NAMES: usize = ([0-9_]+);/)?.[1].replace(/_/g, '')
    expect(max).toMatch(/^[0-9]+$/)
    const catalogs: Record<string, Record<string, unknown>> = { EN, DE, ES, FR, JA }
    for (const [lang, catalog] of Object.entries(catalogs)) {
      const text = String(catalog['logbook.import.tooManyNames'] ?? '')
      const numbers = (text.match(/\d(?:[\d.,\u00a0\u202f ]*\d)?/g) ?? []).map((n) => n.replace(/\D/g, ''))
      expect(numbers.length, lang).toBeGreaterThan(0)
      for (const n of numbers) expect(n, `${lang}: ${text}`).toBe(max)
    }
  })

  it('is told by the engine’s own refusal, and any other failure keeps its detail', () => {
    expect(ENGINE).toContain(`pub const TOO_MANY_NAMES: &str = "${IMPORT_TOO_MANY_NAMES}";`)
    expect(importFailure('Failed', IMPORT_TOO_MANY_NAMES)).toBe(t('logbook.import.tooManyNames'))
    expect(importFailure('Failed', 'disk full')).toBe('Failed: disk full')
  })
})

/** A Rust enum's variants as serde names them on the wire (`rename_all = "camelCase"`). */
function serdeNames(name: string): string[] {
  const body = ENGINE.match(new RegExp(`#\\[serde\\(rename_all = "camelCase"\\)\\]\\npub enum ${name} \\{\\n([\\s\\S]*?)\\n\\}`))?.[1]
  const variants = [...(body ?? '').matchAll(/^ {4}([A-Z]\w*),$/gm)].map((m) => m[1])
  return variants.map((v) => v[0].toLowerCase() + v.slice(1))
}

/** The members of a TS string-literal union, as the module declares it. */
function unionMembers(name: string): string[] {
  const source = readFileSync(resolve(here, 'logImport.ts'), 'utf8')
  const body = source.match(new RegExp(`export type ${name} =([^]*?)\\n\\n`))?.[1] ?? ''
  return [...body.matchAll(/'(\w+)'/g)].map((m) => m[1])
}

describe('the engine’s note kinds and reasons', () => {
  it.each([
    ['NoteKind', 'ImportNoteKind', (k: string) => noteLabel(k as ImportNoteKind)],
    ['DropReason', 'DropReason', (k: string) => reasonLabel(k as DropReason)],
    ['UnmappedReason', 'UnmappedReason', (k: string) => unmappedLabel(k as UnmappedReason)],
  ])('are the UI’s %s, every one with a label', (rust, ts, label) => {
    const names = serdeNames(rust)
    expect(names.length, rust).toBeGreaterThan(3)
    expect([...unionMembers(ts)].sort()).toEqual([...names].sort())
    for (const k of names) {
      const text = label(k)
      expect(text, k).toBeTruthy()
      expect(text, k).not.toMatch(/^logbook\./)
    }
  })
})
