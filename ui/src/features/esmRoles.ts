// ENTER SENDS MESSAGE — which F-key sends each step. The role tables of the built-in sets, the
// voice-slot convention, the CW contest sets re-laid out in the contest layout RTTY's contest set
// already uses, and the role picker's model: the operator's own macros mapped to ESM's steps.
// Pure: what a cockpit sends is still its own send path's business.
//
// ⚠️ THE CW LAYOUT BELOW IS A DRAFT, AND IT IS NOT ON THE AIR. Its texts would go out on the CW
// F-keys of every contest that uses the contest sets, so they are the operator's to sign first.
// Until then the CW cockpit keeps sending its own sets (`DEFAULT_CONTEST_MACROS` and
// `DEFAULT_CONTEST_NO_REPORT_MACROS` in `CwCockpit.tsx`), and `cwBuiltInRoles` gives those no
// role table, so ESM could not send a key of theirs by this layout's positions even if it were
// wired: today's F3 there is the exchange, not TU.
//
// Every character of a macro text goes on the air and is invariant: never translated.
import type { EsmRefusal, EsmRole } from './esm'
import type { BuiltinMacro, MacroKey, MacroSetId } from './macroSets'

/** Which F-keys send each step, in order. Two keys go out as ONE message, their texts joined —
 *  a set laid out as N1MM's CW default is (his call on F5, the exchange alone on F2) runs its
 *  "his call and my exchange" as F5 followed by F2. A step with no keys has no message. */
export type EsmRoleMap = Partial<Record<EsmRole, readonly MacroKey[]>>

/** The contest layout — N1MM's, and RTTY's contest set's as it ships: F1 CQ · F2 his call and my
 *  exchange · F3 TU · F4 my call · F5 his call · F6 my S&P exchange · F7 AGN · F8 QSO B4. ESM
 *  sends six of the eight; F5 and F8 stay keys the operator presses. */
export const CONTEST_LAYOUT_ROLES: EsmRoleMap = {
  cq: ['F1'],
  callExch: ['F2'],
  tu: ['F3'],
  myCall: ['F4'],
  exch: ['F6'],
  again: ['F7'],
}

/** The voice keyer's slots while ESM is on: F1 CQ · F2 my exchange · F3 TU · F4 my call · F5 AGN.
 *  A recording cannot say a callsign, so no slot holds "his call and my exchange" — running, the
 *  operator says it — and F6 stays the operator's own. An empty slot sends nothing, and the strip
 *  names the slot to record. */
export const VOICE_SLOT_ROLES: EsmRoleMap = {
  cq: ['F1'],
  exch: ['F2'],
  tu: ['F3'],
  myCall: ['F4'],
  again: ['F5'],
}

/** RTTY's built-in sets: the contest set is the contest layout; Everyday has no ESM steps. A key
 *  the operator has given his own text keeps its place, and sends his text. */
export const RTTY_SET_ROLES: Record<MacroSetId, EsmRoleMap | null> = {
  everyday: null,
  contest: CONTEST_LAYOUT_ROLES,
}

/** DRAFT — not on the air. The CW contest set for an exchange WITH a signal report (the QSO
 *  parties, CQ WW, CQ WPX) in the contest layout: today's CQ and AGN, today's his-call text moved
 *  to F5, and {RST} before {EXCH} wherever the exchange goes, as today's set has it. */
export const CW_CONTEST_LAYOUT_DRAFT: BuiltinMacro[] = [
  { key: 'F1', label: 'CQ TEST', text: 'CQ TEST DE {MYCALL} {MYCALL} K' },
  { key: 'F2', labelKey: 'cw.macro.exch.label', text: '! {RST} {EXCH}' },
  { key: 'F3', label: 'TU', text: 'TU {MYCALL}' },
  { key: 'F4', labelKey: 'cw.macro.myCall.label', text: '{MYCALL}' },
  { key: 'F5', labelKey: 'cw.macro.hisCall.label', text: '! ' },
  // RTTY's caption says the same words; CW gets its own key in the catalogs when the set goes
  // on the air.
  { key: 'F6', labelKey: 'rtty.macro.spExch.label', text: 'TU {RST} {EXCH}' },
  { key: 'F7', label: 'AGN', text: 'AGN AGN' },
  { key: 'F8', label: 'B4', text: '! QSO B4' },
]

/** DRAFT — not on the air. The same for an exchange with NO signal report (Sweepstakes, the
 *  California QSO Party, the ARRL VHF contests): {RST} left out, because a 5NN there is a wrong
 *  exchange — Sweepstakes would copy it as the serial. */
export const CW_CONTEST_NO_REPORT_LAYOUT_DRAFT: BuiltinMacro[] = [
  { key: 'F1', label: 'CQ TEST', text: 'CQ TEST DE {MYCALL} {MYCALL} K' },
  { key: 'F2', labelKey: 'cw.macro.exch.label', text: '! {EXCH}' },
  { key: 'F3', label: 'TU', text: 'TU {MYCALL}' },
  { key: 'F4', labelKey: 'cw.macro.myCall.label', text: '{MYCALL}' },
  { key: 'F5', labelKey: 'cw.macro.hisCall.label', text: '! ' },
  { key: 'F6', labelKey: 'rtty.macro.spExch.label', text: 'TU {EXCH}' },
  { key: 'F7', label: 'AGN', text: 'AGN AGN' },
  { key: 'F8', label: 'B4', text: '! QSO B4' },
]

/** The role table for the CW set in use: the contest layout when the set IS one of the two
 *  layout sets above, key for key and text for text, and none otherwise. Today's built-in sets
 *  (casual, Field Day, contest) have no Run TU message, and a set of the operator's own has the
 *  roles the operator maps — matching on the texts means ESM can never send a key by this
 *  layout's position on a set laid out some other way. */
export function cwBuiltInRoles(macros: readonly { key: string; text: string }[]): EsmRoleMap | null {
  const isLayout = (set: readonly BuiltinMacro[]) =>
    set.every((b) => macros.some((m) => m.key === b.key && m.text === b.text))
  return isLayout(CW_CONTEST_LAYOUT_DRAFT) || isLayout(CW_CONTEST_NO_REPORT_LAYOUT_DRAFT) ? CONTEST_LAYOUT_ROLES : null
}

/** The roles for a press: the operator's own mapping, role by role, over the built-in table of
 *  the set in use (null for a set of the operator's own). A role the operator left unmapped keeps
 *  the built-in's key. */
export function esmRoles(builtIn: EsmRoleMap | null, own?: EsmRoleMap): EsmRoleMap {
  const roles: EsmRoleMap = { ...builtIn }
  for (const [role, keys] of Object.entries(own ?? {}) as [EsmRole, readonly MacroKey[] | undefined][]) {
    if (keys && keys.length > 0) roles[role] = keys
  }
  return roles
}

/** One F-key as ESM reads it: the key and what it sends, as the dock shows it. A voice slot's
 *  `text` names its recording, and is empty while nothing is recorded. */
export interface EsmSlot {
  key: string
  text: string
}

/** `role`'s message in `slots`: its keys, and their texts joined into the one message they send —
 *  or why there is none, naming the step (and the key), so the strip can say which. */
export function resolveEsmRole(
  roles: EsmRoleMap,
  role: EsmRole,
  slots: readonly EsmSlot[],
): { keys: MacroKey[]; text: string } | Extract<EsmRefusal, { why: 'unmapped' | 'empty' }> {
  const keys = roles[role]
  if (!keys || keys.length === 0) return { why: 'unmapped', role }
  const texts: string[] = []
  for (const key of keys) {
    const text = slots.find((s) => s.key === key)?.text.trim() ?? ''
    if (!text) return { why: 'empty', role, key }
    texts.push(text)
  }
  return { keys: [...keys], text: texts.join(' ') }
}
