// THE CW DOCK'S F-KEYS, AS ITS EDITOR SAVES THEM — what the shared macro editor needs to know about
// CW that `rttyMacros.ts` and `pskMacros.ts` say about theirs: which tokens the expander fills, and
// how one key is written into the store Settings ▸ CW ▸ Macros edits.
//
// ⚠️ CW'S STORE IS NOT RTTY'S SHAPE. RTTY and PSK keep two built-in sets with only the keys the
// operator changed folded over them (`macroSets.ts`). CW keeps named PROFILES
// (`settings.macros.cwProfiles`, the active one by index in `activeCwProfile`), and a profile holds
// either all of its keys or none: an empty list is the cockpit's built-in set, which is the running
// contest's (or Field Day's) set during one and the everyday set otherwise (`CwCockpit.tsx`). So a
// key saved from the dock of a profile on the built-ins first makes that profile the operator's own,
// holding the keys the dock showed, as Customize does in Settings, with the one key changed.
//
// THE TOKENS ARE THE ENGINE'S (`tempo_core::cw::expand`), matched as it matches them: with case. A
// `{mycall}` is no token there, and the keyer skips the braces, so the bare word would go out.
import { EN, type MessageKey } from '../i18n'
import { cwBuiltInRoles, type EsmRoleMap } from './esmRoles'
import { unknownTokens } from './macroSets'

/** Every brace token the CW expander fills. `!`, the call being worked, is the other one. */
export const CW_TOKENS = [
  '{MYCALL}',
  '{NAME}',
  '{MYGRID}',
  '{MYSTATE}',
  '{RST}',
  '{HISNAME}',
  '{HISSTATE}',
  '{CLASS}',
  '{SECTION}',
  '{EXCH}',
  '{MYPARK}',
  '{HISPARK}',
] as const

const KNOWN = new RegExp(`\\{(${CW_TOKENS.map((tok) => tok.slice(1, -1)).join('|')})\\}`, 'g')

/** What a CW macro holds that the expander cannot fill, once each, in order: every `{…}` it does
 *  not know (with case, as the expander matches) and a stray brace. */
export function unknownCwTokens(text: string): string[] {
  return unknownTokens(text, KNOWN)
}

/** One key as a CW profile stores it. */
export interface CwStoredMacro {
  key: string
  label: string
  text: string
}

/** A CW profile as Settings holds it. */
export interface CwProfile {
  name: string
  macros: CwStoredMacro[]
  esmRoles?: EsmRoleMap
}

/** A built-in key as a profile stores it, the way Customize in Settings ▸ CW ▸ Macros writes the
 *  everyday set: on-air shorthand as it stands, and a caption that is a word in English, because a
 *  saved label is the operator's from then on and a translated one would go stale the day the
 *  language changes. */
export function cwStoredBuiltIn(b: { key: string; label?: string; labelKey?: MessageKey; text: string }): CwStoredMacro {
  const label = b.labelKey ? EN[b.labelKey] : b.label
  return { key: b.key, label: typeof label === 'string' ? label : '', text: b.text }
}

/** `profiles` with `key` saved as `entry` in the active profile, and the index that profile is at.
 *
 *  A profile on the built-ins (an empty list) first takes the keys the dock shows (`builtIn`,
 *  already in stored form), and with them the Enter Sends Message steps it was sending: ESM reads
 *  the built-in contest and Field Day layouts by their TEXTS (`cwBuiltInRoles`), so a set that is no
 *  longer one of them word for word would lose every step at the first edit. A profile of the
 *  operator's own that is still a layout word for word, with no mapping of its own, keeps its steps
 *  the same way; one with a mapping keeps that. Every other profile and every other key is left as
 *  it was. With no profile at all, or an index out of range, it does what the engine's load does
 *  (`Macros::migrate_cw_profiles`): one profile named "Default", and the index clamped to it. */
export function withCwEntry(
  profiles: readonly CwProfile[],
  active: number,
  builtIn: readonly CwStoredMacro[],
  key: string,
  entry: { label: string; text: string },
): { profiles: CwProfile[]; active: number } {
  const list: CwProfile[] = profiles.length ? [...profiles] : [{ name: 'Default', macros: [] }]
  const at = active >= 0 && active < list.length ? active : 0
  const { esmRoles: mapped, ...profile } = list[at]
  const own = profile.macros.length > 0
  const before = own ? profile.macros : builtIn
  const roles = (own ? mapped : undefined) ?? cwBuiltInRoles(before) ?? undefined
  const macros = before.some((m) => m.key === key)
    ? before.map((m) => (m.key === key ? { key, ...entry } : { ...m }))
    : [...before.map((m) => ({ ...m })), { key, ...entry }]
  list[at] = roles ? { ...profile, macros, esmRoles: roles } : { ...profile, macros }
  return { profiles: list, active: at }
}
