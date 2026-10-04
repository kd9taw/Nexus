// A MACRO MAY NOT BE CAPTIONED AS A STOP — in every language the app ships, whichever one is on
// screen. An operator's caption is their own words and is never translated, so a key captioned
// "Stopp" reads as a stop to anyone at the station who reads German, in every UI language.
//
// The stop words come from two places, and each is checked where it is all there is:
//   · every installed catalog's own word for Stop and Cancel, so a language is covered as it
//     ships with nothing to extend. This file FINDS every catalog in the tree rather than listing
//     them, installs each as main.tsx does, and fails if any catalog's own captions for a stop
//     (every short English caption that says stop, halt, abort, cancel or Esc, translated) get
//     through. That is also how "beenden" and 終了 reached the reviewed lists: the catalogs say
//     "Mithören beenden" for Stop listening and 終了 for an activation's Stop;
//   · the reviewed lists in macroSets.ts, which are everything the Remote page has, because it
//     installs no catalog. They are checked first, before anything is installed here.
// Every built-in macro caption, in every language, must still be allowed.

import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { EN, availableLocales, installCatalog, type MessageKey, type PartialCatalog } from '../i18n'
import { STOP_WORDS, isStopLikeLabel, type MacroSetId } from './macroSets'
import { resolvePskSet } from './pskMacros'
import { resolveRttySet } from './rttyMacros'

/** Every UI catalog in the tree, by locale — found, not listed, so a new one (Portuguese's pt.ts,
 *  as its kit writes it) is checked the moment it exists. */
const CATALOGS: [string, PartialCatalog][] = Object.entries(
  import.meta.glob<Record<string, unknown>>(['../i18n/[a-z][a-z].ts', '../i18n/[a-z][a-z]-[A-Z][A-Z].ts'], {
    eager: true,
  }),
).map(([path, module]) => {
  const catalogs = Object.values(module).filter((value) => typeof value === 'object' && value !== null)
  if (catalogs.length !== 1) throw new Error(`${path} should export one catalog, not ${catalogs.length}`)
  return [path.replace(/^.*\/|\.ts$/g, ''), catalogs[0] as PartialCatalog]
})

/** A catalog's text for `key`, or the English it falls back to — what the screen would show. */
const textIn = (catalog: PartialCatalog) => (key: MessageKey) => {
  const text = catalog[key]
  return typeof text === 'string' && text ? text : (EN[key] as string)
}

/** What an operator reaching for a stop would read as one, per language. */
const REFUSED: Record<string, string[]> = {
  en: ['Stop', 'Stop TX', 'Esc', 'Abort', 'Escape', 'Halt', 'Cancel', 'Cancel TX', '■ STOP', 'ＳＴＯＰ'],
  de: ['Stopp', 'STOPP', 'TX stoppen', 'TX-Stopp', 'Halt', 'Anhalten', 'Abbrechen', 'Abbruch', 'Beenden'],
  es: ['Parar', 'PARAR', 'Parar TX', 'Pare', 'Parada', 'Detener', 'Detén', 'Abortar', 'Anular', 'Cancelar', 'Dejar de emitir'],
  fr: ['Arrêter', 'ARRÊTER', 'ARRETER', 'Arrête !', 'Arrêt', "Arrêt d'urgence", 'Interrompre', 'Abandonner', 'Annuler', 'Échap'],
  ja: ['停止', '送信停止', 'TX停止', '停波', '中止', '中断', '止めて', '止まれ', 'ストップ', 'ｽﾄｯﾌﾟ', 'キャンセル', '取り消し', '終了', 'エスケープ', 'Stopボタン', 'ＥＳＣ'],
}

/** On-air shorthand is the same in every language — an invariant token, never translated — and
 *  QRT is what its key SENDS, not a control. FT, RTTY, CW and SSTV captions alike. */
const SHORTHAND = ['CQ', 'CQ TEST', 'CQ POTA', 'CQ SSTV', 'TU', 'TU 73', '73', 'RR73', 'RRR', 'R-12', 'Tx1', 'Tx6', 'AGN?', 'B4', 'QRZ?', 'QSL', 'QRT', 'SK', 'KN', 'BK', '5NN', '599', 'NR?', 'DUPE', 'NIL', 'TNX 73 GL', '{CALL} 599', '']

/** Ordinary captions an operator writes, per language. A few merely CONTAIN a stop word's letters
 *  (Stopwatch, Descent, Stoppuhr, Unterhaltung, Comparar): the word boundary's own control. */
const ALLOWED: Record<string, string[]> = {
  en: ['Answer', 'Exchange', 'My call', 'His call', 'Reply', 'Name', 'QTH', 'Rig', 'Thanks', 'Repeat', 'Contest', 'Sign off', 'Stopwatch', 'Descent', 'Rescue'],
  de: ['Antwort', 'Austausch', 'Mein Rufz.', 'Rufen', 'Danke 73', 'Wiederholen', 'Nochmal', 'Rapport', 'Gruß', 'Wetter', 'Stoppuhr', 'Unterhaltung'],
  es: ['Responder', 'Intercambio', 'Mi ind.', 'Llamar', 'Gracias 73', 'Repetir', 'Otra vez', 'Nombre', 'Saludo', 'Despedida', 'Info para QSL', 'Comparar'],
  fr: ['Répondre', 'Échange', 'Mon IND.', 'Appel', 'Merci 73', 'Répéter', 'Encore', 'Nom', 'Bonjour', 'Au revoir', 'Fin'],
  ja: ['応答', 'ナンバー', '自局コール', '相手コール', '呼出', 'ありがとう', '再送', 'もう一度', '名前', '挨拶', 'コンテスト', '最後'],
}

/** The catalogs' own captions for a stop. Chosen by English words, not by the function under
 *  test, so a word dropped from the lists cannot also drop the obligation. 16 is the editor's
 *  caption length: a caption it could hold. */
const SAYS_STOP = /\b(stop|halt|abort|cancel|esc|escape)\b/i
const STOP_CAPTIONS = (Object.keys(EN) as MessageKey[]).filter((k) => {
  const text = EN[k]
  return typeof text === 'string' && text.length <= 16 && SAYS_STOP.test(text)
})
/** Which of `catalog`'s own captions for a stop the check lets through, as `key: text`. */
const missed = (catalog: PartialCatalog) =>
  STOP_CAPTIONS.filter((k) => typeof catalog[k] === 'string' && !isStopLikeLabel(catalog[k] as string)).map(
    (k) => `${k}: ${catalog[k]}`,
  )

describe('the reviewed words alone, as on the Remote page, which installs no catalog', () => {
  it('runs with nothing but English installed (control: this block runs first)', () => {
    expect(availableLocales()).toEqual(['en'])
  })

  it.each(Object.entries(REFUSED))('refuse a stop in %s', (_, captions) => {
    expect(captions.filter((c) => !isStopLikeLabel(c))).toEqual([])
  })

  it("refuse each listed language's own captions for a stop", () => {
    const listed = CATALOGS.filter(([locale]) => locale in STOP_WORDS)
    expect(listed.length, 'control: every list has its catalog').toBe(Object.keys(STOP_WORDS).length)
    expect(listed.flatMap(([locale, catalog]) => missed(catalog).map((m) => `${locale} ${m}`))).toEqual([])
  })
})

describe('every catalog in the tree, installed as main.tsx installs one', () => {
  beforeAll(() => {
    for (const [locale, catalog] of CATALOGS) if (locale !== 'en') installCatalog(locale, catalog)
  })

  it('finds every catalog main.tsx installs (control: the search does not come back empty)', () => {
    const main = readFileSync(new URL('../main.tsx', import.meta.url), 'utf8')
    const installed = [...main.matchAll(/installCatalog\(\s*'([a-z-]+)'/g)].map((m) => m[1])
    expect(installed.length, 'control: main.tsx really does install catalogs').toBeGreaterThan(0)
    expect(CATALOGS.map(([locale]) => locale)).toEqual(expect.arrayContaining(['en', ...installed]))
  })

  it.each(CATALOGS)('refuses the %s catalog’s own captions for a stop', (_, catalog) => {
    expect(STOP_CAPTIONS.some((k) => typeof catalog[k] === 'string'), 'control: it has some').toBe(true)
    expect(missed(catalog)).toEqual([])
  })

  it('allows on-air shorthand', () => {
    expect(SHORTHAND.filter(isStopLikeLabel)).toEqual([])
  })

  it.each(Object.entries(ALLOWED))('allows ordinary captions in %s', (_, captions) => {
    expect(captions.filter(isStopLikeLabel)).toEqual([])
  })

  it.each(CATALOGS)('allows every built-in caption in %s', (_, catalog) => {
    const sets: MacroSetId[] = ['everyday', 'contest']
    const captions = sets.flatMap((set) => [
      ...resolveRttySet(undefined, set, textIn(catalog)),
      ...resolvePskSet(undefined, set, textIn(catalog)),
    ]).map((slot) => slot.label)
    const cw = (Object.keys(EN) as MessageKey[]).filter((k) => /^cw\.macro\.\w+\.label$/.test(k)).map(textIn(catalog))
    expect(cw.length, 'control: the CW macro captions were found').toBeGreaterThan(0)
    expect(captions.filter(Boolean).length, 'control: the built-ins have captions').toBeGreaterThan(0)
    expect([...captions, ...cw].filter(isStopLikeLabel)).toEqual([])
  })
})

describe('a language no list names', () => {
  // Made-up words in a made-up locale: what Portuguese would be if the Spanish list did not
  // already hold its Parar, and no real catalog will ever contain them.
  it('is covered the moment its catalog is installed, with nothing to extend', () => {
    expect(['Zorp', 'Quux'].filter(isStopLikeLabel), 'control: nothing knows them yet').toEqual([])
    installCatalog('zz', { 'ota.activation.stop.label': 'Zorp', 'logbook.purge.cancel': 'Quux' })
    expect(['Zorp', 'ZORP TX', '■ Quux'].filter((c) => !isStopLikeLabel(c))).toEqual([])
    expect(isStopLikeLabel('Zorpig'), 'a word that merely begins with one').toBe(false)
  })
})
