// THE KEYBOARD MODES' F-KEY MACRO SETS — the parts that are the same in every mode: two named
// built-in sets with the operator's saved entries folded over them, the edits that write one key
// or reset one set, and the brace-token scan the editor refuses on.
//
// Extracted from `rttyMacros.ts` when PSK gained the same editor (#316). ⚠️ WHAT BELONGS HERE is
// only what a mode cannot sensibly differ on. The BUILT-IN TEXTS, the token values and the
// on-air framing are each mode's own and live in its module — RTTY is Baudot upper case and
// cannot send a brace at all, PSK is mixed-case full ASCII and can — so this file knows a macro
// set's SHAPE and nothing about what goes on the air.

import { EN, textInEveryLocale, type MessageKey } from '../i18n'
import type { KeyboardMacroProfile } from '../types'

/** The eight F-keys a keyboard cockpit's dock binds, in dock order. */
export const MACRO_KEYS = ['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8'] as const
export type MacroKey = (typeof MACRO_KEYS)[number]

/** The two built-in sets, by the id the mode's `active*Profile` setting stores. */
export type MacroSetId = 'everyday' | 'contest'

/** A built-in slot. `label` is on-air shorthand and an invariant token (CQ, 73, TU); `labelKey`
 *  is a catalog caption for a word, resolved when the row renders. Neither is an empty slot.
 *  Every character of `text` goes on the air and is invariant. */
export interface BuiltinMacro {
  key: MacroKey
  label?: string
  labelKey?: MessageKey
  text: string
}

/** One key as the dock renders it. `custom` — the operator's saved entry replaced the built-in,
 *  so "Reset this button" has something to reset. */
export interface MacroSlot {
  key: MacroKey
  label: string
  text: string
  custom: boolean
}

/** The set a stored `active*Profile` names: `contest`, or Everyday for anything else. */
export function macroSetId(active: string | null | undefined): MacroSetId {
  return active === 'contest' ? 'contest' : 'everyday'
}

/** The eight keys of `set`: the operator's saved entry for a key where there is one — their own
 *  words, never translated — else the built-in, whose caption `translate` resolves. */
export function resolveMacroSet(
  profiles: KeyboardMacroProfile[] | undefined,
  set: MacroSetId,
  builtins: Record<MacroSetId, BuiltinMacro[]>,
  translate: (key: MessageKey) => string,
): MacroSlot[] {
  const saved = profiles?.find((p) => p.name === set)?.macros ?? []
  return builtins[set].map((b) => {
    const own = saved.find((m) => m.key === b.key)
    if (own) return { key: b.key, label: own.label, text: own.text, custom: true }
    const label = b.labelKey ? translate(b.labelKey) : (b.label ?? '')
    return { key: b.key, label, text: b.text, custom: false }
  })
}

/** `profiles` with `key`'s saved entry in `set` replaced — or removed, for `null`, which puts the
 *  built-in back. Every other set, and every other key (unknown ones included), is left as it
 *  was; a set's entries stay in F-key order so the file reads the way the dock does. */
export function withMacroEntry(
  profiles: KeyboardMacroProfile[],
  set: MacroSetId,
  key: MacroKey,
  entry: { label: string; text: string } | null,
): KeyboardMacroProfile[] {
  const rank = (k: string) => {
    const i = (MACRO_KEYS as readonly string[]).indexOf(k)
    return i < 0 ? MACRO_KEYS.length : i
  }
  const edit = (macros: KeyboardMacroProfile['macros']) =>
    [...macros.filter((m) => m.key !== key), ...(entry ? [{ key, ...entry }] : [])].sort(
      (a, b) => rank(a.key) - rank(b.key),
    )
  return profiles.some((p) => p.name === set)
    ? profiles.map((p) => (p.name === set ? { name: p.name, macros: edit(p.macros) } : p))
    : [...profiles, { name: set, macros: edit([]) }]
}

/** `profiles` with `set` back to its built-ins: the empty list. */
export function withMacroSetReset(
  profiles: KeyboardMacroProfile[],
  set: MacroSetId,
): KeyboardMacroProfile[] {
  return profiles.some((p) => p.name === set)
    ? profiles.map((p) => (p.name === set ? { name: p.name, macros: [] } : p))
    : profiles
}

/** The matcher for a mode's token list — `{MYCALL}` and the rest, without regard to case. */
export function knownTokenPattern(tokens: readonly string[]): RegExp {
  const names = tokens.map((tok) => tok.replace(/[{}]/g, ''))
  return new RegExp(`\\{(${names.join('|')})\\}`, 'gi')
}

/** What `text` holds that the expander cannot fill, once each, in order: every `{…}` the mode
 *  does not know, and a stray `{` or `}` left over. */
export function unknownTokens(text: string, known: RegExp): string[] {
  const rest = text.replace(known, ' ')
  const found = rest.match(/\{[^{}]*\}|[{}]/g) ?? []
  return [...new Set(found)]
}

/** Why a macro cannot be sent as it stands. */
export type MacroRefusal = { missing: 'mycall' | 'call' | 'exch' } | { unknown: string }

/** The words that read as a STOP — stop, halt, abort, cancel, Esc — in each language the app
 *  ships, by locale. Every installed catalog also gives its own word for a stop (below), so a new
 *  language needs no list here; these are the rest — the Esc keys, the halt and abort words, the
 *  words a catalog never writes alone — and ALL the Remote page has, since it installs no
 *  catalog. Reviewed 2026-10-04 against each catalog's own captions for a stop, which
 *  `macroSets.test.ts` holds them to: German says "beenden" as often as "stoppen"
 *  ("Mithören beenden" is Stop listening), and Japanese uses 終了 for a Stop. */
export const STOP_WORDS: Record<string, readonly string[]> = {
  en: ['stop', 'halt', 'abort', 'cancel', 'esc', 'escape'],
  de: ['stopp', 'stoppen', 'halt', 'anhalten', 'abbrechen', 'abbruch', 'beenden'],
  es: ['parar', 'pare', 'parada', 'detener', 'detén', 'abortar', 'anular', 'cancelar', 'dejar de'],
  fr: ['arrêter', 'arrête', 'arrêt', 'interrompre', 'abandonner', 'annuler', 'échap', 'échappement'],
  ja: ['停止', '停波', '中止', '中断', '止め', '止まれ', 'ストップ', 'キャンセル', '取消', '取り消', '終了', 'エスケープ'],
}

/** For matching only: lower case, one space between words, compatibility forms folded (full-width
 *  ＳＴＯＰ is STOP, half-width ｽﾄｯﾌﾟ is ストップ) and Latin accents dropped, because French capitals
 *  often go without (ARRET). */
const fold = (text: string) =>
  text.normalize('NFKD').replace(/[\u0300-\u036f]/g, '').replace(/\s+/g, ' ').toLowerCase()

const REVIEWED = [...new Set(Object.values(STOP_WORDS).flat().map(fold))]

/** A caption's letters, without the ■ or the arrow around them. */
const bare = (text: string) => text.replace(/^\P{L}+|\P{L}+$/gu, '')

/** The catalog keys whose English is a bare stop word (Stop, ■ STOP, Cancel), found on first use:
 *  an installed catalog's text for them is that language's own word for a stop. */
let stopKeys: MessageKey[] | undefined

/** Every installed catalog's own word for a stop, so a language is covered the moment main.tsx
 *  installs its catalog (Portuguese's "Parar" as it ships), with no list here to extend. */
function catalogStopWords(): string[] {
  stopKeys ??= (Object.keys(EN) as MessageKey[]).filter((key) => {
    const text = EN[key]
    return typeof text === 'string' && STOP_WORDS.en.includes(bare(text).toLowerCase())
  })
  return stopKeys.flatMap(textInEveryLocale).map((text) => fold(bare(text))).filter(Boolean)
}

/** Japanese and Chinese run their words together, so a word in those scripts matches anywhere in
 *  a caption: 送信停止 is a stop, at the price of refusing ストップウォッチ where Stopwatch passes.
 *  Every other script spaces its words, so there a word must stand alone, with no letter of a
 *  spaced script either side: Stopwatch and Descent are not stops, "Stopボタン" is. */
const UNSPACED = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]/u
const LETTER = /[\p{L}\p{M}]/u
const extendsWord = (c: string | undefined) => c !== undefined && LETTER.test(c) && !UNSPACED.test(c)

/** `word` in the folded `text` — as a word of its own, where its script has words. */
function readsAs(text: string, word: string): boolean {
  if (UNSPACED.test(word)) return text.includes(word)
  for (let at = text.indexOf(word); at >= 0; at = text.indexOf(word, at + 1))
    if (!extendsWord(text[at - 1]) && !extendsWord(text[at + word.length])) return true
  return false
}

/** A caption that reads as a STOP, in any shipped language, whichever is on screen. The macros
 *  are senders, and a sender captioned Stop, Esc or Abort is the one control an operator reaching
 *  for a stop would press — so the editor refuses it. A caption is the operator's own words and
 *  is never translated, so "Stopp" reads as a stop to anyone at the station who reads German. A
 *  dock's real Esc/Stop is fixed and outside the editable set. */
export function isStopLikeLabel(label: string): boolean {
  const text = fold(label)
  return [...REVIEWED, ...catalogStopWords()].some((word) => readsAs(text, word))
}

/** A slot with neither caption nor message: clicking it opens the editor instead of sending. */
export function isEmptyMacroSlot(slot: MacroSlot): boolean {
  return !slot.label.trim() && !slot.text.trim()
}
