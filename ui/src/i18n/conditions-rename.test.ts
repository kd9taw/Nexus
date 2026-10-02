// CONNECT IS NOW "CONDITIONS" (step 5). The operator's scope: "Display strings only (nav label, titles,
// manual/docs, the TV-mode setting's wording); code identifiers, settings keys and saved layouts
// unchanged; all five languages + the pt-BR kit". And: "The nav button reads "Conditions"; its tooltip and
// the view's title say "(formerly Connect)"", there and nowhere else (2026-10-02): "Tooltip, window title,
// CHANGELOG and release notes only. … Settings and the website just say "Conditions"." So the feature's
// label, which Settings, the "now on" announcement, a crash message and the website's feature list read,
// is the plain name, and the window's title has a key of its own (App: `shell.windowTitle` over
// `features.connect.windowTitle` while Conditions is open; App.conditionsName.test.tsx mounts it).
//
// Read off the catalogs and the kit's CSV themselves: a key renamed, a language missed, a mention of the
// old name left in a hint, or a kit row left on the old English all fail here.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { EN } from './en'
import { DE } from './de'
import { ES } from './es'
import { FR } from './fr'
import { JA } from './ja'

type Catalog = Readonly<Record<string, unknown>>
const LOCALES: Record<string, { cat: Catalog; name: string; title: string; formerly: string; dflt: string }> = {
  en: { cat: EN, name: 'Conditions', title: 'Conditions (formerly Connect)', formerly: '(formerly Connect)', dflt: '(default)' },
  de: { cat: DE, name: 'Bedingungen', title: 'Bedingungen (früher Connect)', formerly: '(früher Connect)', dflt: '(voreingestellt)' },
  es: { cat: ES, name: 'Condiciones', title: 'Condiciones (antes Connect)', formerly: '(antes Connect)', dflt: '(predeterminada)' },
  fr: { cat: FR, name: 'Conditions', title: 'Conditions (anciennement Connect)', formerly: '(anciennement Connect)', dflt: '(par défaut)' },
  ja: { cat: JA, name: 'コンディション', title: 'コンディション（旧コネクト）', formerly: '（旧コネクト）', dflt: '（既定）' },
}

/** The view's old name as a word, in either script: "Connecté", "Connectors", "コネクター" and "connect" are
 *  not it. Its edges are Latin letters only: Japanese runs words together ("テレビでConnect"). */
const OLD = /(?<![\p{Script=Latin}\p{N}])Connect(?![\p{Script=Latin}\p{N}])|コネクト(?![ー\p{Script=Katakana}])/u
/** The two English strings where "Connect" is the verb, opening a sentence. */
const VERB = new Set(['program.chirp.step.upload', 'settings.amplifier.note'])
const text = (v: unknown) => (typeof v === 'string' ? v : JSON.stringify(v))

/** Every display string the rename touched, and the picker's words the default changed. */
const CHANGED = [
  'nav.connect.label',
  'nav.connect.title',
  'features.connect.label',
  'features.connect.windowTitle',
  'settings.connectWeb.legend',
  'settings.connectWeb.label',
  'settings.connectWeb.hint',
  'settings.connectWeb.aria.enable',
  'settings.connectWeb.aria.disable',
  'settings.amplifier.model.hint',
  'settings.rotator.model.hint',
  'settings.integrations.clusterSpots.hint',
  'settings.mapGlobe.note',
  'dxped.showOnMap.title',
  'connect.popOut.title',
  'nowbar.band.title.connect',
  'nowbar.rail.off.title',
  'profiles.vhf.blurb',
  'connect.layout.standard.title',
  'connect.layout.frameBar.label',
  'connect.layout.button.title',
  'connect.layout.kept.label',
  'connect.layout.kept.title',
] as const

describe('Connect reads "Conditions" in all five languages', () => {
  for (const [loc, l] of Object.entries(LOCALES)) {
    it(`${loc}: the nav button and the feature's label read ${l.name}; its tooltip and the window's title say ${l.formerly}`, () => {
      expect(l.cat['nav.connect.label']).toBe(l.name)
      expect(l.cat['features.connect.label']).toBe(l.name)
      expect(text(l.cat['nav.connect.title']).startsWith(l.title)).toBe(true)
      expect(l.cat['features.connect.windowTitle']).toBe(l.title)
    })

    it(`${loc}: no other string calls the view by its old name`, () => {
      const left = Object.entries(l.cat)
        .filter(([k, v]) => !VERB.has(k) && OLD.test(text(v).split(l.formerly).join('')))
        .map(([k]) => k)
      expect(left).toEqual([])
    })

    it(`${loc}: the picker names Frame + bar the default, and offers the earlier layout in this language`, () => {
      expect(text(l.cat['connect.layout.frameBar.label']).endsWith(l.dflt)).toBe(true)
      expect(text(l.cat['connect.layout.button.title']).endsWith(l.dflt)).toBe(true)
      expect(typeof l.cat['connect.layout.kept.label']).toBe('string')
      expect(typeof l.cat['connect.layout.kept.title']).toBe('string')
    })
  }

  it("the TV page's tab says it too, as the main and dashboard windows' titles do", () => {
    const page = readFileSync(fileURLToPath(new URL('../../connect-tv.html', import.meta.url)), 'utf8')
    expect(page).toContain(`<title>Nexus ${LOCALES.en.title}</title>`)
  })

  it('POSITIVE CONTROL — the scan sees the old name in the forms it took, and passes over the verb and its kin', () => {
    for (const s of ['Open Connect in its own window', 'テレビでConnect', 'コネクトを開く', 'eine Spalte mit Connect-Bereichen'])
      expect(OLD.test(s), s).toBe(true)
    for (const s of ['Connecté au service', 'SET > Connectors > USB AF', 'How does the radio connect?', 'Connection log', 'USBコネクター'])
      expect(OLD.test(s), s).toBe(false)
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
  const english = new Map(body.map((r) => [r[head.indexOf('key')], r[head.indexOf('english')]]))

  it('has a row for every changed string, with its English word for word', () => {
    const wrong = CHANGED.filter((k) => typeof EN[k] !== 'string' || english.get(k) !== EN[k])
    expect(wrong).toEqual([])
  })

  it('names the view by its old name nowhere in its English column', () => {
    const left = [...english].filter(([k, v]) => !VERB.has(k) && OLD.test(v.split('(formerly Connect)').join(''))).map(([k]) => k)
    expect(left).toEqual([])
  })
})
