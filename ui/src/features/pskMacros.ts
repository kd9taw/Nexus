// THE PSK F-KEY MACROS (#316) — the two built-in sets and the token expander. RTTY's
// `rttyMacros.ts`, for PSK: the SET MODEL is the shared `features/macroSets.ts`, and what is
// below is only what PSK does differently on the air. Pure — the cockpit renders the slots,
// sends through its own `send()` (the one path to `psk_send` a click and an F-key share), and
// saves through `setPskMacros`.
//
// ⚠️ MIXED CASE IS THE POINT, and it is why this is not RTTY's table with a different name.
// Baudot has one case, so RTTY shouts; PSK31's varicode is full ASCII, and its frequency-ordered
// code table makes lower-case letters the SHORT ones — so mixed-case text is both the mode's
// convention and literally faster on the wire. The callsigns are the exception: those go up,
// which is the convention everywhere and what the cockpit's Call box already stores.
//
// ⚠️ NO ON-AIR FRAMING. RTTY frames an F-key message onto a line of its own ending in a space
// (`frameForAir`), a contest convention its receivers' parsers depend on. PSK has no equivalent
// and has never had one, so a macro goes out as its expanded text and nothing else — the same
// bytes a click on these keys has always sent.

import type { KeyboardMacroProfile } from '../types'
import {
  MACRO_KEYS,
  knownTokenPattern,
  macroSetId,
  resolveMacroSet,
  unknownTokens,
  withMacroEntry,
  withMacroSetReset,
  type BuiltinMacro,
  type MacroKey,
  type MacroRefusal,
  type MacroSetId,
  type MacroSlot,
} from './macroSets'

export const PSK_MACRO_KEYS = MACRO_KEYS
export type PskMacroKey = MacroKey

/** The two built-in sets, by the id `macros.activePskProfile` stores. */
export type PskSetId = MacroSetId

/** EVERYDAY — the casual set the cockpit shipped with (F1–F4, texts unchanged, so an operator
 *  who never opens the editor sees exactly the dock they had), and four empty keys for their
 *  own. */
const EVERYDAY: BuiltinMacro[] = [
  { key: 'F1', label: 'CQ', text: 'CQ CQ CQ de {MYCALL} {MYCALL} pse k' },
  { key: 'F2', labelKey: 'psk.macro.answer.label', text: '{CALL} de {MYCALL} {MYCALL} k' },
  {
    key: 'F3',
    labelKey: 'psk.macro.exchange.label',
    text: '{CALL} de {MYCALL} ur 599 599 btu k',
  },
  { key: 'F4', label: '73', text: '{CALL} de {MYCALL} tnx qso 73 sk' },
  { key: 'F5', text: '' },
  { key: 'F6', text: '' },
  { key: 'F7', text: '' },
  { key: 'F8', text: '' },
]

/** CONTEST — the run/S&P set, RTTY's key ASSIGNMENTS (F1 CQ · F2 exchange · F3 TU · F4 my call ·
 *  F5 his call · F6 S&P exchange · F7 AGN · F8 B4) so an operator who works both modes does not
 *  relearn the keyboard, in PSK's own wording. The message shapes are the published digital
 *  contest conventions: a CQ starts AND ends with CQ, so a station tuning in mid-message still
 *  reads it as one; the exchange goes twice; the run station's TU asks for the next caller; the
 *  S&P exchange leaves out the runner's call. Kept SHORTER than RTTY's — PSK31 runs at about 31
 *  baud, so every repeated word is paid for in seconds on the air. */
const CONTEST: BuiltinMacro[] = [
  { key: 'F1', label: 'CQ', text: 'cq test {MYCALL} {MYCALL} cq' },
  { key: 'F2', labelKey: 'psk.macro.exch.label', text: '{CALL} 599 {EXCH} {EXCH}' },
  { key: 'F3', label: 'TU', text: 'tu {MYCALL} cq' },
  { key: 'F4', labelKey: 'psk.macro.myCall.label', text: '{MYCALL} {MYCALL}' },
  { key: 'F5', labelKey: 'psk.macro.hisCall.label', text: '{CALL}' },
  { key: 'F6', labelKey: 'psk.macro.spExch.label', text: 'tu 599 {EXCH} {EXCH}' },
  { key: 'F7', label: 'AGN', text: 'agn? agn?' },
  { key: 'F8', label: 'B4', text: '{CALL} qso b4 tu {MYCALL}' },
]

const PSK_BUILTINS: Record<PskSetId, BuiltinMacro[]> = { everyday: EVERYDAY, contest: CONTEST }

/** The set `macros.activePskProfile` names: `contest`, or Everyday for anything else. */
export const pskSetId = macroSetId

/** One key as the dock renders it. */
export type PskMacroSlot = MacroSlot

/** The eight keys of `set`: the operator's saved entry for a key where there is one — their own
 *  words, never translated — else the built-in, whose caption `translate` resolves. */
export function resolvePskSet(
  profiles: KeyboardMacroProfile[] | undefined,
  set: PskSetId,
  translate: Parameters<typeof resolveMacroSet>[3],
): PskMacroSlot[] {
  return resolveMacroSet(profiles, set, PSK_BUILTINS, translate)
}

/** `profiles` with `key`'s saved entry in `set` replaced — or removed, for `null`, which puts the
 *  built-in back. Every other set and every other key is left as it was. */
export function withPskEntry(
  profiles: KeyboardMacroProfile[],
  set: PskSetId,
  key: PskMacroKey,
  entry: { label: string; text: string } | null,
): KeyboardMacroProfile[] {
  return withMacroEntry(profiles, set, key, entry)
}

/** `profiles` with `set` back to its built-ins: the empty list. */
export function withPskSetReset(
  profiles: KeyboardMacroProfile[],
  set: PskSetId,
): KeyboardMacroProfile[] {
  return withMacroSetReset(profiles, set)
}

/** The tokens the expander fills, as the editor lists them. RTTY's four, because they are the
 *  four facts a keyboard QSO is made of and an operator working both modes types the same
 *  message in each. */
export const PSK_TOKENS = ['{MYCALL}', '{CALL}', '{RST}', '{EXCH}'] as const

const KNOWN = knownTokenPattern(PSK_TOKENS)

/** What `text` holds that the expander cannot fill, once each, in order: every `{…}` it does not
 *  know, and a stray `{` or `}` left over. Token names are matched without regard to case.
 *
 *  ⚠️ The editor refuses these, and PSK's REASON is not RTTY's. RTTY cannot encode a brace at
 *  all, so an unknown token silently loses its braces and puts a bare word on the air. PSK sends
 *  the braces perfectly — `{MYCAL}` would go out reading `{MYCAL}` — which is not a silent
 *  failure but is never what the operator meant. Refused either way, and the editor says why in
 *  PSK's own words. The COMPOSE BAR is untouched by this: text typed there has always gone out
 *  literally, braces included, and still does. */
export function unknownPskTokens(text: string): string[] {
  return unknownTokens(text, KNOWN)
}

/** Why a macro cannot be sent as it stands. */
export type PskRefusal = MacroRefusal

/** `text` with every token filled, or the reason it cannot be.
 *
 *  `{RST}` is 599 — PSK's own convention, and the report the cockpit's built-in exchange has
 *  always sent. `{CALL}` goes up (the Call box already stores it that way); `{MYCALL}` is passed
 *  through as the station holds it, which is what a click on these keys has always sent and is
 *  not this change's to alter. `exch` is the running contest's sent exchange WITHOUT the report,
 *  and null outside a contest, where `{EXCH}` is refused exactly as `{CALL}` is with no call. */
export function expandPskMacro(
  text: string,
  ctx: { mycall: string; call: string; exch: string | null },
): { text: string } | PskRefusal {
  const unknown = unknownPskTokens(text)
  if (unknown.length > 0) return { unknown: unknown[0] }
  const uses = (token: string) => new RegExp(`\\{${token}\\}`, 'i').test(text)
  const mycall = ctx.mycall.trim()
  const call = ctx.call.trim().toUpperCase()
  const exch = (ctx.exch ?? '').trim().toUpperCase()
  if (uses('MYCALL') && !mycall) return { missing: 'mycall' }
  if (uses('CALL') && !call) return { missing: 'call' }
  if (uses('EXCH') && !exch) return { missing: 'exch' }
  const value: Record<string, string> = { MYCALL: mycall, CALL: call, RST: '599', EXCH: exch }
  return { text: text.replace(KNOWN, (_, token: string) => value[token.toUpperCase()]) }
}
