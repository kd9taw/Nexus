// PROGRAM IS NOW "REPEATERS" (the operator, 2026-10-02: "Repeaters everywhere, no "(formerly Program)"."). Display
// strings only: the view id `program`, the `program.*`/`nav.program.*`/`features.program.*` keys, the settings keys and
// the saved lists (radioprog.json, "My channels") keep their names. All five languages and the pt-BR kit, and no note:
// the nav button, its tooltip, the view's title and the feature's label (Settings, the window title, the "now on"
// announcement, the website's feature list) all read the new name and nothing says what it used to be called.
//
// Read off the catalogs and the kit's CSV themselves: a key renamed, a language missed, a mention of the old name left
// in a hint, a "(formerly …)" note, or a kit row left on the old English all fail here.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { EN } from './en'
import { DE } from './de'
import { ES } from './es'
import { FR } from './fr'
import { JA } from './ja'

type Catalog = Readonly<Record<string, unknown>>
/** Each language's word for the view, and the forms its old name took there (the English name too: three of the four
 *  translations quoted it in their RepeaterBook hints). Edges are letters of any script, except in Japanese, which
 *  runs words together. */
const LOCALES: Record<string, { cat: Catalog; name: string; old: RegExp; formerly: RegExp }> = {
  en: { cat: EN, name: 'Repeaters', old: /(?<![\p{L}\p{N}])Program(?![\p{L}\p{N}])/u, formerly: /formerly/i },
  de: { cat: DE, name: 'Relais', old: /(?<![\p{L}\p{N}])(Program|Programmieren)(?![\p{L}\p{N}])|„Programm“|Bereich Programm\b/u, formerly: /früher|ehemals/i },
  es: { cat: ES, name: 'Repetidores', old: /(?<![\p{L}\p{N}])(Program|Programar)(?![\p{L}\p{N}])/u, formerly: /antes|anteriormente/i },
  fr: { cat: FR, name: 'Relais', old: /(?<![\p{L}\p{N}])(Program|Programmation)(?![\p{L}\p{N}])/u, formerly: /anciennement|autrefois/i },
  ja: { cat: JA, name: 'レピータ', old: /プログラムセクション|<b>プログラム<\/b>|^プログラム(\s|$)|までプログラムは|場合、プログラムは/u, formerly: /旧|以前の/u },
}
/** Strings where the word is NOT the view: a verb ("Program your radios", "Programar con CHIRP") or an on-the-air
 *  awards programme (POTA/SOTA). Each is checked to still be what it is, so this list cannot hide a missed rename. */
const OTHER_SENSE: Record<string, Record<string, RegExp>> = {
  en: { 'features.program.oneLine': /^Program your radios — /, 'ota.program.aria': /^Program$/ },
  de: {},
  es: { 'program.chirp.title': /^Programar con CHIRP$/ },
  fr: {},
  ja: { 'ota.program.aria': /^プログラム$/ },
}
const text = (v: unknown) => (typeof v === 'string' ? v : JSON.stringify(v))

/** Every display string the rename touched. */
const CHANGED = [
  'nav.program.label',
  'nav.program.title',
  'features.program.label',
  'program.title',
  'memories.empty.hint',
  'program.projectsFile.keptInPlace',
  'settings.connections.repeaterbook.token.saved',
  'settings.connections.repeaterbook.token.cleared',
  'settings.confirmations.repeaterbook.token.hint',
] as const
/** The sentences that name the view inside a longer string. */
const SENTENCES = CHANGED.filter((k) => !['nav.program.label', 'features.program.label', 'program.title'].includes(k))

describe('Program reads "Repeaters" in all five languages, with no note', () => {
  for (const [loc, l] of Object.entries(LOCALES)) {
    it(`${loc}: the nav button, the view's title and the feature's label read ${l.name}; the tooltip leads with it`, () => {
      expect(l.cat['nav.program.label']).toBe(l.name)
      expect(l.cat['features.program.label']).toBe(l.name)
      expect(l.cat['program.title']).toBe(l.name)
      expect(text(l.cat['nav.program.title']).startsWith(`${l.name} — `)).toBe(true)
    })

    it(`${loc}: every sentence that names the view names it ${l.name}`, () => {
      const missing = SENTENCES.filter((k) => !text(l.cat[k]).includes(l.name))
      expect(missing).toEqual([])
    })

    it(`${loc}: nothing says what the view used to be called`, () => {
      const noted = CHANGED.filter((k) => l.formerly.test(text(l.cat[k])) && l.old.test(text(l.cat[k])))
      expect(noted).toEqual([])
      expect(l.formerly.test(text(l.cat['nav.program.title']))).toBe(false)
    })

    it(`${loc}: no other string calls the view by its old name`, () => {
      const other = OTHER_SENSE[loc]
      const left = Object.entries(l.cat)
        .filter(([k, v]) => !(k in other) && l.old.test(text(v)))
        .map(([k]) => k)
      expect(left).toEqual([])
      for (const [k, still] of Object.entries(other)) expect(text(l.cat[k]), `${loc} ${k}`).toMatch(still)
    })
  }

  it('POSITIVE CONTROL — each scan sees the old name in the forms it took, and passes over the other senses', () => {
    const sees: Record<string, string[]> = {
      en: ['Program', 'the Program section falls back', 'the <b>Program</b> section', 'so Program will not save'],
      de: ['der Bereich „Program“ nutzt', 'Programmieren', 'aus dem Bereich Programmieren hierher', 'Bereich Programm'],
      es: ['la sección Programar', 'la sección Program vuelve', 'Programar'],
      fr: ['la section Programmation', 'la section Program se rabat', 'Programmation'],
      ja: ['プログラムセクションはRepeaterBookを使用します', '<b>プログラム</b>セクション', 'プログラム', 'までプログラムは保存しません'],
    }
    const passes: Record<string, string[]> = {
      en: ['programs', 'programming cable', 'the radio-programming workbench', 'Programmer'],
      de: ['ein Windows-Programm', 'im anderen Programm einfügen', 'Funkgeräte programmieren', 'On-the-Air-Programm'],
      es: ['Programa tus equipos', 'programar', 'Programas'],
      fr: ['Programmez vos radios', 'Programmer avec CHIRP', 'programmation'],
      ja: ['他のプログラムと共有', 'Windows用プログラムで', 'オンエアプログラム'],
    }
    for (const [loc, l] of Object.entries(LOCALES)) {
      for (const s of sees[loc]) expect(l.old.test(s), `${loc} sees "${s}"`).toBe(true)
      for (const s of passes[loc]) expect(l.old.test(s), `${loc} passes "${s}"`).toBe(false)
    }
  })

  it('the guide is titled with the new name, and the feature list the website reads names it too', () => {
    const guide = readFileSync(fileURLToPath(new URL('../../../docs/guide/program.md', import.meta.url)), 'utf8')
    expect(guide.split('\n')[0]).toBe('# Repeaters')
    const shipped = JSON.parse(readFileSync(fileURLToPath(new URL('../../../docs/shipped.json', import.meta.url)), 'utf8'))
    const labels = JSON.stringify(shipped)
    expect(labels).toContain('"id":"program","label":"Repeaters"')
  })
})

/** RFC 4180, as the kit is written (quoted fields, "" for a quote, line breaks inside quotes). */
function parseCsv(src: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let field = ''
  let quoted = false
  for (let i = 0; i < src.length; i++) {
    const c = src[i]
    if (quoted) {
      if (c !== '"') field += c
      else if (src[i + 1] === '"') {
        field += '"'
        i++
      } else quoted = false
    } else if (c === '"') quoted = true
    else if (c === ',') {
      row.push(field)
      field = ''
    } else if (c === '\n') {
      row.push(field)
      rows.push(row)
      row = []
      field = ''
    } else if (c !== '\r') field += c
  }
  if (field || row.length) rows.push([...row, field])
  return rows
}

describe('the pt-BR kit carries the rename', () => {
  const [head, ...body] = parseCsv(
    readFileSync(fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)), 'utf8'),
  )
  const col = (name: string) => head.indexOf(name)
  const english = new Map(body.map((r) => [r[col('key')], r[col('english')]]))
  const notes = new Map(body.map((r) => [r[col('key')], r[col('notes')]]))

  it('has a row for every changed string, with its English word for word', () => {
    const wrong = CHANGED.filter((k) => typeof EN[k] !== 'string' || english.get(k) !== EN[k])
    expect(wrong).toEqual([])
  })

  it('names the view by its old name nowhere in its English column, and its notes call the view Repeaters', () => {
    const left = [...english].filter(([k, v]) => !(k in OTHER_SENSE.en) && LOCALES.en.old.test(v)).map(([k]) => k)
    expect(left).toEqual([])
    const stale = [...notes].filter(([, v]) => /Program's|Program \(radio|in Program\b|Program section/.test(v)).map(([k]) => k)
    expect(stale).toEqual([])
  })
})
