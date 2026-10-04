// A MACRO MAY NOT BE CAPTIONED AS A STOP — in every language the app ships, whichever one is on
// screen. An operator's caption is their own words and is never translated, so a key captioned
// "Stopp" reads as a stop to anyone at the station who reads German, in every UI language.
//
// The hand-kept lists below are examples. What keeps the word lists honest is derived:
//   · every short English caption in the catalogs that says stop, halt, abort, cancel or Esc is a
//     caption the app ITSELF uses for a stop, so its translation in each shipped catalog must be
//     refused too (that is how "beenden" and 終了 are on the lists: the catalogs say
//     "Mithören beenden" for Stop listening and 終了 for an activation's Stop);
//   · every built-in macro caption, in every language, must still be allowed;
//   · main.tsx is where a language ships, and each one it installs must have a word list and be
//     checked here.

import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { EN, type MessageKey, type PartialCatalog } from '../i18n'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'
import { STOP_WORDS, isStopLikeLabel, type MacroSetId } from './macroSets'
import { resolvePskSet } from './pskMacros'
import { resolveRttySet } from './rttyMacros'

/** Every shipped translation, by the locale main.tsx installs it under. */
const CATALOGS: [string, PartialCatalog][] = [
  ['de', DE],
  ['es', ES],
  ['fr', FR],
  ['ja', JA],
]
const WITH_EN: [string, PartialCatalog][] = [['en', EN], ...CATALOGS]

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

describe('a macro caption that reads as a stop is refused in every shipped language', () => {
  it.each(Object.entries(REFUSED))('%s', (_, captions) => {
    expect(captions.filter((c) => !isStopLikeLabel(c))).toEqual([])
  })
})

describe('an ordinary macro caption is allowed in every shipped language', () => {
  it('on-air shorthand', () => {
    expect(SHORTHAND.filter(isStopLikeLabel)).toEqual([])
  })

  it.each(Object.entries(ALLOWED))('%s', (_, captions) => {
    expect(captions.filter(isStopLikeLabel)).toEqual([])
  })

  it.each(WITH_EN)('every built-in caption, in %s', (_, catalog) => {
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

describe("each catalog's own captions for a stop are refused", () => {
  // Chosen by English words, not by the function under test, so a word dropped from the lists
  // cannot also drop the obligation. 16 is the editor's caption length: a caption it could hold.
  const SAYS_STOP = /\b(stop|halt|abort|cancel|esc|escape)\b/i
  const stopCaptions = (Object.keys(EN) as MessageKey[]).filter((k) => {
    const text = EN[k]
    return typeof text === 'string' && text.length <= 16 && SAYS_STOP.test(text)
  })

  it('finds them (control: the stops the catalogs are known to hold)', () => {
    for (const k of ['quit.logbook.stopTx', 'ota.activation.stop.label', 'rotor.pane.stop.label', 'remote.audio.stop', 'logbook.purge.cancel'])
      expect(stopCaptions, k).toContain(k)
  })

  it.each(WITH_EN)('%s', (_, catalog) => {
    const translated = stopCaptions.filter((k) => typeof catalog[k] === 'string')
    expect(translated.length, 'control: this catalog translates some of them').toBeGreaterThan(0)
    expect(translated.filter((k) => !isStopLikeLabel(catalog[k] as string)).map((k) => `${k}: ${catalog[k]}`)).toEqual([])
  })
})

describe('the word lists cover every language the app ships', () => {
  it('one list per locale main.tsx installs, and every installed catalog is checked above', () => {
    const main = readFileSync(new URL('../main.tsx', import.meta.url), 'utf8')
    const installed = [...main.matchAll(/installCatalog\(\s*'([a-z-]+)'/g)].map((m) => m[1]).sort()
    expect(installed.length, 'control: main.tsx really does install catalogs').toBeGreaterThan(0)
    expect(CATALOGS.map(([l]) => l).sort()).toEqual(installed)
    expect(Object.keys(STOP_WORDS).sort()).toEqual(['en', ...installed].sort())
  })
})
