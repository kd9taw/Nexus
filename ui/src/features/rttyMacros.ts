// THE RTTY F-KEY MACROS — the two built-in sets, the operator's saved entries folded over them,
// and the token expander. Pure: the cockpit renders the slots, sends through its own `send()`
// (the one path to `rtty_send` a click and an F-key share), and saves through `setRttyMacros`.
//
// ⚠️ NOT `cw::expand`, and not a copy of it. The CW expander sends `!` as the worked call and
// `{RST}` as 5NN — both CW conventions. RTTY reports 599 in figures, and `!` is a character an
// RTTY operator types.
//
// ⚠️ A TOKEN THIS DOES NOT KNOW IS REFUSED, NEVER SENT. RTTY has no braces — ITA2 cannot encode
// them — so an unknown `{TOKEN}` would not fail anywhere downstream: the keyboard filter drops
// the braces and the bare word goes on the air.

import type { MessageKey } from '../i18n'
import type { RttyMacroProfile } from '../types'

export const RTTY_MACRO_KEYS = ['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8'] as const
export type RttyMacroKey = (typeof RTTY_MACRO_KEYS)[number]

/** The two built-in sets, by the id `macros.activeRttyProfile` stores. */
export type RttySetId = 'everyday' | 'contest'

/** A built-in slot. `label` is on-air shorthand and an invariant token (CQ, 73, TU); `labelKey`
 *  is a catalog caption for a word, resolved when the row renders. Neither is an empty slot.
 *  Every character of `text` goes on the air and is invariant. */
interface BuiltinMacro {
  key: RttyMacroKey
  label?: string
  labelKey?: MessageKey
  text: string
}

/** EVERYDAY — the casual set the cockpit shipped with (F1–F4, texts unchanged), and four empty
 *  keys for the operator's own. */
const EVERYDAY: BuiltinMacro[] = [
  { key: 'F1', label: 'CQ', text: 'CQ CQ CQ DE {MYCALL} {MYCALL} K' },
  { key: 'F2', labelKey: 'rtty.macro.answer.label', text: '{CALL} DE {MYCALL} {MYCALL} K' },
  { key: 'F3', labelKey: 'rtty.macro.exchange.label', text: '{CALL} DE {MYCALL} UR 599 599 K' },
  { key: 'F4', label: '73', text: '{CALL} DE {MYCALL} TU 73 SK' },
  { key: 'F5', text: '' },
  { key: 'F6', text: '' },
  { key: 'F7', text: '' },
  { key: 'F8', text: '' },
]

/** CONTEST — the N1MM-convention run/S&P set: F1 CQ · F2 exchange · F3 TU · F4 my call · F5 his
 *  call. Checked against the published RTTY contest message sets (rttycontesting.com, the N1MM
 *  RTTY guides): a CQ starts AND ends with CQ, so a station tuning in mid-message still reads it
 *  as one; the exchange goes twice; the run station's TU ends with CQ to ask for the next caller;
 *  and the S&P exchange leaves out the runner's call. */
const CONTEST: BuiltinMacro[] = [
  { key: 'F1', label: 'CQ', text: 'CQ TEST {MYCALL} {MYCALL} CQ' },
  { key: 'F2', labelKey: 'rtty.macro.exch.label', text: '{CALL} 599 {EXCH} {EXCH}' },
  { key: 'F3', label: 'TU', text: 'TU {MYCALL} CQ' },
  { key: 'F4', labelKey: 'rtty.macro.myCall.label', text: '{MYCALL} {MYCALL}' },
  { key: 'F5', labelKey: 'rtty.macro.hisCall.label', text: '{CALL}' },
  { key: 'F6', labelKey: 'rtty.macro.spExch.label', text: 'TU 599 {EXCH} {EXCH}' },
  { key: 'F7', label: 'AGN', text: 'AGN? AGN?' },
  { key: 'F8', label: 'B4', text: '{CALL} QSO B4 TU {MYCALL}' },
]

/** The set `macros.activeRttyProfile` names: `contest`, or Everyday for anything else. */
export function rttySetId(active: string | null | undefined): RttySetId {
  return active === 'contest' ? 'contest' : 'everyday'
}

/** One key as the dock renders it. `custom` — the operator's saved entry replaced the built-in,
 *  so "Reset this button" has something to reset. */
export interface RttyMacroSlot {
  key: RttyMacroKey
  label: string
  text: string
  custom: boolean
}

/** The eight keys of `set`: the operator's saved entry for a key where there is one — their own
 *  words, never translated — else the built-in, whose caption `translate` resolves. */
export function resolveRttySet(
  profiles: RttyMacroProfile[] | undefined,
  set: RttySetId,
  translate: (key: MessageKey) => string,
): RttyMacroSlot[] {
  const saved = profiles?.find((p) => p.name === set)?.macros ?? []
  return (set === 'contest' ? CONTEST : EVERYDAY).map((b) => {
    const own = saved.find((m) => m.key === b.key)
    if (own) return { key: b.key, label: own.label, text: own.text, custom: true }
    const label = b.labelKey ? translate(b.labelKey) : (b.label ?? '')
    return { key: b.key, label, text: b.text, custom: false }
  })
}

/** `profiles` with `key`'s saved entry in `set` replaced — or removed, for `null`, which puts the
 *  built-in back. Every other set, and every other key (unknown ones included), is left as it
 *  was; a set's entries stay in F-key order so the file reads the way the dock does. */
export function withRttyEntry(
  profiles: RttyMacroProfile[],
  set: RttySetId,
  key: RttyMacroKey,
  entry: { label: string; text: string } | null,
): RttyMacroProfile[] {
  const rank = (k: string) => {
    const i = (RTTY_MACRO_KEYS as readonly string[]).indexOf(k)
    return i < 0 ? RTTY_MACRO_KEYS.length : i
  }
  const edit = (macros: RttyMacroProfile['macros']) =>
    [...macros.filter((m) => m.key !== key), ...(entry ? [{ key, ...entry }] : [])].sort(
      (a, b) => rank(a.key) - rank(b.key),
    )
  return profiles.some((p) => p.name === set)
    ? profiles.map((p) => (p.name === set ? { name: p.name, macros: edit(p.macros) } : p))
    : [...profiles, { name: set, macros: edit([]) }]
}

/** `profiles` with `set` back to its built-ins: the empty list. */
export function withRttySetReset(profiles: RttyMacroProfile[], set: RttySetId): RttyMacroProfile[] {
  return profiles.some((p) => p.name === set)
    ? profiles.map((p) => (p.name === set ? { name: p.name, macros: [] } : p))
    : profiles
}

/** The tokens the expander fills, as the editor lists them. */
export const RTTY_TOKENS = ['{MYCALL}', '{CALL}', '{RST}', '{EXCH}'] as const

const KNOWN = /\{(MYCALL|CALL|RST|EXCH)\}/gi

/** What `text` holds that the expander cannot fill, once each, in order: every `{…}` it does not
 *  know, and a stray `{` or `}` left over. Token names are matched without regard to case. */
export function unknownRttyTokens(text: string): string[] {
  const rest = text.replace(KNOWN, ' ')
  const found = rest.match(/\{[^{}]*\}|[{}]/g) ?? []
  return [...new Set(found)]
}

/** Why a macro cannot be sent as it stands. */
export type RttyRefusal =
  | { missing: 'mycall' | 'call' | 'exch' }
  | { unknown: string }

/** `text` with every token filled — `{RST}` is always 599, the RTTY report — or the reason it
 *  cannot be. `exch` is the running contest's sent exchange WITHOUT the report, and null outside
 *  a contest, where `{EXCH}` is refused exactly as `{CALL}` is with no call. */
export function expandRttyMacro(
  text: string,
  ctx: { mycall: string; call: string; exch: string | null },
): { text: string } | RttyRefusal {
  const unknown = unknownRttyTokens(text)
  if (unknown.length > 0) return { unknown: unknown[0] }
  const uses = (token: string) => new RegExp(`\\{${token}\\}`, 'i').test(text)
  const mycall = ctx.mycall.trim().toUpperCase()
  const call = ctx.call.trim().toUpperCase()
  const exch = (ctx.exch ?? '').trim().toUpperCase()
  if (uses('MYCALL') && !mycall) return { missing: 'mycall' }
  if (uses('CALL') && !call) return { missing: 'call' }
  if (uses('EXCH') && !exch) return { missing: 'exch' }
  const value: Record<string, string> = { MYCALL: mycall, CALL: call, RST: '599', EXCH: exch }
  return { text: text.replace(KNOWN, (_, token: string) => value[token.toUpperCase()]) }
}

/** A caption that reads as a STOP. The macros are senders, and a sender captioned Stop, Esc or
 *  Abort is the one control an operator reaching for a stop would press — so the editor refuses
 *  it. The dock's real Esc/Stop is fixed and outside the editable set. */
export function isStopLikeLabel(label: string): boolean {
  return /\b(stop|esc|escape|abort)\b/i.test(label)
}

/** How an F-key message goes on the air: on a line of its own and ending in a space — the
 *  published RTTY contest convention. The line break puts the call at the start of a line on the
 *  other station's screen; the space terminates the last word, which a receiver's parser
 *  (ours included — seq.rs drops an unterminated trailing token) otherwise holds back. */
export function frameForAir(text: string): string {
  return `\r\n${text.trim()} `
}
