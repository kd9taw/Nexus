// THE KEYBOARD MODES' F-KEY MACRO SETS — the parts that are the same in every mode: two named
// built-in sets with the operator's saved entries folded over them, the edits that write one key
// or reset one set, and the brace-token scan the editor refuses on.
//
// Extracted from `rttyMacros.ts` when PSK gained the same editor (#316). ⚠️ WHAT BELONGS HERE is
// only what a mode cannot sensibly differ on. The BUILT-IN TEXTS, the token values and the
// on-air framing are each mode's own and live in its module — RTTY is Baudot upper case and
// cannot send a brace at all, PSK is mixed-case full ASCII and can — so this file knows a macro
// set's SHAPE and nothing about what goes on the air.

import type { MessageKey } from '../i18n'
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

/** A caption that reads as a STOP. The macros are senders, and a sender captioned Stop, Esc or
 *  Abort is the one control an operator reaching for a stop would press — so the editor refuses
 *  it. A dock's real Esc/Stop is fixed and outside the editable set. */
export function isStopLikeLabel(label: string): boolean {
  return /\b(stop|esc|escape|abort)\b/i.test(label)
}

/** A slot with neither caption nor message: clicking it opens the editor instead of sending. */
export function isEmptyMacroSlot(slot: MacroSlot): boolean {
  return !slot.label.trim() && !slot.text.trim()
}
