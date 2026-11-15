// ---------------------------------------------------------------------------
// ⭐ THE OPERATOR'S OWN CONTESTS, AS THE SCREEN KNOWS THEM — the list the station installed
// (`list_custom_contests`), held once for every surface that names a contest.
//
// The station is the source of truth: it compiles, validates, stores and exports each definition
// (`crates/tempo-app/src/custom_contests.rs`). This module only remembers its last answer so the
// sync name helpers in `fdEvent.ts` (`contestName`…) can say "Club Sprint" rather than
// `custom_club_sprint`, and re-reads it after anything that changes it (a save, a delete, an
// import). Nothing here decides a rule.
//
// ⚠️ A TYPED VALUE (an age, a name) IS NEVER PART OF A DEFINITION. It lives in
// `Settings.contestEntered[contestId][slot]`, on this computer only, so a shared file carries none.
// ---------------------------------------------------------------------------
import { useSyncExternalStore } from 'react'
import { t } from '../i18n'
import {
  listCustomContests,
  type CustomContest,
  type CustomContestDef,
  type CustomContestFieldId,
  type CustomContestMode,
  type FtActivity,
} from '../api'

/** The prefix every operator-built contest id carries (`custom::ID_PREFIX`). */
export const CUSTOM_PREFIX = 'custom_'
export const isCustomId = (id: string | undefined): boolean => !!id && id.trim().startsWith(CUSTOM_PREFIX)

let list: readonly CustomContest[] = []
const subscribers = new Set<() => void>()

/** The installed contests, as last read. */
export function customContests(): readonly CustomContest[] {
  return list
}
/** One installed contest by id, as last read. */
export function customContest(id: string | undefined): CustomContest | undefined {
  const key = id?.trim()
  return key ? list.find((c) => c.def.id === key) : undefined
}
/** Replace the remembered list (a read's answer) and tell every surface showing it. */
export function setCustomContests(next: readonly CustomContest[]): void {
  list = next
  for (const notify of subscribers) notify()
}
/** Read the station's list again. A station that cannot answer (an older one, a host with no
 *  bridge) leaves the last answer in place: a name shown as its id is the worst that follows. */
export async function reloadCustomContests(): Promise<readonly CustomContest[]> {
  try {
    const next = await listCustomContests()
    if (Array.isArray(next)) setCustomContests(next)
  } catch {
    // keep the last answer
  }
  return list
}
function subscribe(notify: () => void): () => void {
  subscribers.add(notify)
  return () => subscribers.delete(notify)
}
/** The installed contests, re-rendering when they change. */
export function useCustomContests(): readonly CustomContest[] {
  return useSyncExternalStore(subscribe, customContests, customContests)
}

/** ⭐ A new contest's id from its name — `custom::id_for_name`'s rule, which the station checks
 *  again on save: `custom_` + the name lower-cased, each run of other characters one `_`, at most 40,
 *  then `_2`, `_3`… while it is taken. */
export function customIdForName(name: string, taken: (id: string) => boolean): string {
  let slug = ''
  for (const c of name.trim()) {
    if (/^[A-Za-z0-9]$/.test(c)) slug += c.toLowerCase()
    else if (slug !== '' && !slug.endsWith('_')) slug += '_'
  }
  slug = slug.replace(/_+$/, '').slice(0, 40).replace(/_+$/, '')
  const base = `${CUSTOM_PREFIX}${slug || 'contest'}`
  if (!taken(base)) return base
  for (let n = 2; ; n++) if (!taken(`${base}_${n}`)) return `${base}_${n}`
}

/** The modes a contest may list (`custom::MODES`), in the builder's order. Mode names are tokens. */
export const CUSTOM_MODES: readonly CustomContestMode[] = ['CW', 'SSB', 'FM', 'RTTY', 'PSK', 'FT8', 'FT4']
/** The exchange menu (`custom::FIELDS`), in the builder's order. */
export const CUSTOM_FIELDS: readonly CustomContestFieldId[] = [
  'rst',
  'serial',
  'state_province',
  'fd_class',
  'arrl_section',
  'cq_zone',
  'itu_zone',
  'power',
  'grid4',
  'grid6',
  'age',
  'name',
  'text',
]
/** The most fields an exchange has (`custom::MAX_FIELDS`). */
export const MAX_EXCHANGE_FIELDS = 5
/** The longest a text field's caption may be (`custom::MAX_LABEL_CHARS`). */
export const MAX_CAPTION_CHARS = 20
/** A Cabrillo contest name the station accepts: 1 to 32 of A-Z, 0-9 and -. */
export const TOKEN_RE = /^[A-Z0-9-]{1,32}$/

/** What each menu field is called in the builder. */
const FIELD_WORDS: Record<CustomContestFieldId, () => string> = {
  rst: () => t('customContest.field.rst'),
  serial: () => t('customContest.field.serial'),
  state_province: () => t('customContest.field.stateProvince'),
  fd_class: () => t('customContest.field.fdClass'),
  arrl_section: () => t('customContest.field.arrlSection'),
  cq_zone: () => t('customContest.field.cqZone'),
  itu_zone: () => t('customContest.field.ituZone'),
  power: () => t('customContest.field.power'),
  grid4: () => t('customContest.field.grid4'),
  grid6: () => t('customContest.field.grid6'),
  age: () => t('customContest.field.age'),
  name: () => t('customContest.field.name'),
  text: () => t('customContest.field.text'),
}
export function customFieldName(field: CustomContestFieldId): string {
  return FIELD_WORDS[field]()
}

/** ⚠️ WSJT-X's own names for its contest messages (Settings ▸ Advanced ▸ Special operating
 *  activity) — never translated: the operator finds the same words in WSJT-X. */
export const FT_ACTIVITY_NAMES: Record<FtActivity, string> = {
  field_day: 'ARRL Field Day',
  na_vhf: 'NA VHF',
  ww_digi: 'WW Digi',
  rtty_roundup: 'ARRL RTTY Roundup',
  eu_vhf: 'EU VHF',
}

/** A definition to start building from: a weekend contest, RST + serial, once per band. */
export function blankCustomDef(): CustomContestDef {
  return {
    format: 'nexus-custom-contest',
    version: 1,
    id: '',
    name: '',
    short: '',
    cabrillo_contest: '',
    modes: ['CW'],
    window: { kind: 'weekend', month: 1, weekend: 'nth_full', n: 1, start_hour_utc: 0, duration_hours: 24 },
    exchange: [{ field: 'rst' }, { field: 'serial' }],
    dupe: 'band',
    points: { CW: 1, PH: 1, DIG: 1 },
  }
}

/** ⭐ What a TYPED slot is called (`AGE`, `NAME`, `TXT1`): a text field by the caption the operator
 *  gave it, else by its number. `contest` is the contest the slot is in. */
export function enteredSlotLabel(contest: string | undefined, slot: string): string {
  if (slot === 'AGE') return t('customContest.entered.age')
  if (slot === 'NAME') return t('customContest.entered.name')
  const nth = /^TXT([12])$/.exec(slot)
  if (nth) return customTextCaption(contest, slot) || t('customContest.entered.text', { n: Number(nth[1]) })
  return slot
}

/** The caption the operator gave text field `TXT1`/`TXT2` of an installed contest, or `''`. The
 *  caption is never sent; it is what the boxes are called. */
export function customTextCaption(contest: string | undefined, slot: string): string {
  const nth = /^TXT([12])$/.exec(slot)
  const def = customContest(contest)?.def
  if (!nth || !def) return ''
  const texts = def.exchange.filter((f) => f.field === 'text')
  return texts[Number(nth[1]) - 1]?.label?.trim() ?? ''
}
