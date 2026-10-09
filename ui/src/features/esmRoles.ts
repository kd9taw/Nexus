// ENTER SENDS MESSAGE — which F-key sends each step. The role tables of the built-in sets, the
// voice-slot convention, the CW contest and Field Day sets re-laid out in the contest layout
// RTTY's contest set already uses, and the role picker's model: the operator's own macros mapped
// to ESM's steps. Pure: what a cockpit sends is still its own send path's business.
//
// ⚠️ THE CW LAYOUT BELOW IS SIGNED (operator, 2026-10-09), AND IT IS NOT ON THE AIR YET. Its
// texts go out on the CW F-keys of every contest and Field Day that uses the built-in sets, and
// they go on the air WITH Enter Sends Message, never before it. Until `CW_LAYOUT_ON_AIR` is
// switched on, the CW cockpit keeps sending its own sets (`DEFAULT_CONTEST_MACROS`,
// `DEFAULT_CONTEST_NO_REPORT_MACROS` and `DEFAULT_FD_MACROS` in `CwCockpit.tsx`), and
// `cwBuiltInRoles` gives those no role table, so ESM could not send a key of theirs by this
// layout's positions even if it were wired: today's F3 there is the exchange, not TU.
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

/** The voice keyer's six slots, the keys a Phone step may be mapped to. */
export const VOICE_KEYS: readonly MacroKey[] = ['F1', 'F2', 'F3', 'F4', 'F5', 'F6']

/** The voice keyer's slots as ESM reads them: each named by its label once it holds a
 *  recording, and empty while it holds none, so a step on an empty slot is refused by name. */
export function esmVoiceSlots(messages: readonly { slot: number; label: string; file: string }[]): EsmSlot[] {
  return messages.map((m) => ({ key: `F${m.slot}`, text: m.file ? m.label.trim() || `F${m.slot}` : '' }))
}

/** RTTY's built-in sets: the contest set is the contest layout; Everyday has no ESM steps. A key
 *  the operator has given his own text keeps its place, and sends his text. */
export const RTTY_SET_ROLES: Record<MacroSetId, EsmRoleMap | null> = {
  everyday: null,
  contest: CONTEST_LAYOUT_ROLES,
}

/** ⛔ THE ON-AIR SWITCH for the signed CW layout below, and it is OFF. While it is off, the CW
 *  cockpit's contest and Field Day keys send today's sets; switched on, they send the three
 *  layout sets. It is switched on in the change that wires Enter to send, never before, and
 *  three things move with it: `docs/manual/CW.md`'s three tables (`docs-match-code.test.ts`
 *  compares the manual with whichever sets this selects), the tests that pin today's texts
 *  (`CwCockpit.contestmacros.test.tsx`, `esmRoles.test.ts`), and the CHANGELOG, which says that
 *  F3 is now TU. */
export const CW_LAYOUT_ON_AIR: boolean = false

/** SIGNED — on the air only with `CW_LAYOUT_ON_AIR`. The CW contest set for an exchange WITH a
 *  signal report (the QSO parties, CQ WW, CQ WPX) in the contest layout: today's CQ and AGN,
 *  today's his-call text moved to F5, and {RST} before {EXCH} wherever the exchange goes, as
 *  today's set has it. */
export const CW_CONTEST_LAYOUT: BuiltinMacro[] = [
  { key: 'F1', label: 'CQ TEST', text: 'CQ TEST DE {MYCALL} {MYCALL} K' },
  { key: 'F2', labelKey: 'cw.macro.exch.label', text: '! {RST} {EXCH}' },
  { key: 'F3', label: 'TU', text: 'TU {MYCALL}' },
  { key: 'F4', labelKey: 'cw.macro.myCall.label', text: '{MYCALL}' },
  { key: 'F5', labelKey: 'cw.macro.hisCall.label', text: '! ' },
  { key: 'F6', labelKey: 'cw.macro.spExch.label', text: 'TU {RST} {EXCH}' },
  { key: 'F7', label: 'AGN', text: 'AGN AGN' },
  { key: 'F8', label: 'B4', text: '! QSO B4' },
]

/** SIGNED — on the air only with `CW_LAYOUT_ON_AIR`. The same for an exchange with NO signal
 *  report (Sweepstakes, the California QSO Party, the ARRL VHF contests): {RST} left out,
 *  because a 5NN there is a wrong exchange — Sweepstakes would copy it as the serial. */
export const CW_CONTEST_NO_REPORT_LAYOUT: BuiltinMacro[] = [
  { key: 'F1', label: 'CQ TEST', text: 'CQ TEST DE {MYCALL} {MYCALL} K' },
  { key: 'F2', labelKey: 'cw.macro.exch.label', text: '! {EXCH}' },
  { key: 'F3', label: 'TU', text: 'TU {MYCALL}' },
  { key: 'F4', labelKey: 'cw.macro.myCall.label', text: '{MYCALL}' },
  { key: 'F5', labelKey: 'cw.macro.hisCall.label', text: '! ' },
  { key: 'F6', labelKey: 'cw.macro.spExch.label', text: 'TU {EXCH}' },
  { key: 'F7', label: 'AGN', text: 'AGN AGN' },
  { key: 'F8', label: 'B4', text: '! QSO B4' },
]

/** SIGNED — on the air only with `CW_LAYOUT_ON_AIR`. The CW Field Day set (ARRL Field Day and
 *  Winter Field Day) in the same layout: Field Day's own CQ, and its exchange, class and
 *  section, which carries no report. */
export const CW_FIELD_DAY_LAYOUT: BuiltinMacro[] = [
  { key: 'F1', label: 'CQ FD', text: 'CQ FD DE {MYCALL} {MYCALL} K' },
  { key: 'F2', labelKey: 'cw.macro.exch.label', text: '! {EXCH}' },
  { key: 'F3', label: 'TU', text: 'TU {MYCALL}' },
  { key: 'F4', labelKey: 'cw.macro.myCall.label', text: '{MYCALL}' },
  { key: 'F5', labelKey: 'cw.macro.hisCall.label', text: '! ' },
  { key: 'F6', labelKey: 'cw.macro.spExch.label', text: 'TU {EXCH}' },
  { key: 'F7', label: 'AGN', text: 'AGN AGN' },
  { key: 'F8', label: 'B4', text: '! QSO B4' },
]

/** The role table for the CW set in use: the contest layout when the set IS one of the three
 *  layout sets above, key for key and text for text, and none otherwise. Today's built-in sets
 *  (casual, Field Day, contest) have no Run TU message, and a set of the operator's own has the
 *  roles the operator maps — matching on the texts means ESM can never send a key by this
 *  layout's position on a set laid out some other way. */
export function cwBuiltInRoles(macros: readonly { key: string; text: string }[]): EsmRoleMap | null {
  const isLayout = (set: readonly BuiltinMacro[]) =>
    set.every((b) => macros.some((m) => m.key === b.key && m.text === b.text))
  return [CW_CONTEST_LAYOUT, CW_CONTEST_NO_REPORT_LAYOUT, CW_FIELD_DAY_LAYOUT].some(isLayout)
    ? CONTEST_LAYOUT_ROLES
    : null
}

/** What ESM finds in the CW cockpit's built-in contest and Field Day sets: the contest layout's
 *  steps once those sets are the layout (`CW_LAYOUT_ON_AIR`), and none before, because today's
 *  sets have no Run TU. Settings shows it, read-only, for a CW profile on the built-in sets. The
 *  everyday set has no steps either way: ESM runs only in a contest. */
export const CW_BUILT_IN_ROLES: EsmRoleMap | null = CW_LAYOUT_ON_AIR ? CONTEST_LAYOUT_ROLES : null

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
