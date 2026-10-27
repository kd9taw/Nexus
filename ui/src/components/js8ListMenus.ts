// The JS8 lists' menus, as JS8Call 3.0.3 builds them: each list's ⋯ (its View menu and header menu:
// Sort By…, Show Columns…, Show Band Heartbeats and ACKs, Clear, Clear All Lists) and a row's ⋯ (its
// right-click menu: Jump to …Hz, Center Filter at …Hz, Jump to … speed, Show Message Inbox, Add New
// Station or Group…, Remove, Block, Remove Activity). UI_Constructor.cpp:781-1335 and mainwindow.cpp:4669-4800, read as
// the specification. Entries only: what each one does is the cockpit's, handed in.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts): every word is in the catalog.
import { t } from '../i18n'
import type { Js8Speed } from '../types'
import { JS8_BAND_SORTS, JS8_CALL_SORTS, speedName, type Js8BandSort, type Js8CallSort } from '../features/js8Lists'
import { JS8_BAND_COLUMNS, JS8_CALL_COLUMNS, type Js8BandColumn, type Js8CallColumn, type Js8ViewPrefs } from '../features/js8View'
import type { Js8MenuEntry } from './Js8ListMenu'
import type { MenuItem } from './ui/Menu'

/** Literal keys per order, so the orphan guard sees each referenced. */
function bandSortLabel(sort: Js8BandSort): string {
  switch (sort) {
    case 'offset':
      return t('js8.view.sort.offset')
    case 'timestamp':
      return t('js8.view.sort.timestamp')
    case '-timestamp':
      return t('js8.view.sort.timestampRecent')
    case 'snr':
      return t('js8.view.sort.snr')
    case '-snr':
      return t('js8.view.sort.snrStrong')
    case 'submode':
      return t('js8.view.sort.submode')
    case '-submode':
      return t('js8.view.sort.submodeFast')
  }
}
function callSortLabel(sort: Js8CallSort): string {
  switch (sort) {
    case 'callsign':
      return t('js8.view.sort.callsign')
    case 'ackTimestamp':
      return t('js8.view.sort.ackTimestamp')
    case 'distance':
      return t('js8.view.sort.distance')
    case '-distance':
      return t('js8.view.sort.distanceFar')
    case 'azimuth':
      return t('js8.view.sort.azimuth')
    default:
      return bandSortLabel(sort)
  }
}
function bandColumnLabel(c: Js8BandColumn): string {
  switch (c) {
    case 'offset':
      return t('js8.view.col.offset')
    case 'timestamp':
      return t('js8.view.col.timestamp')
    case 'snr':
      return t('js8.view.col.snr')
    case 'tdrift':
      return t('js8.view.col.tdrift')
    case 'submode':
      return t('js8.view.col.submode')
  }
}
function callColumnLabel(c: Js8CallColumn): string {
  switch (c) {
    case 'callsign':
      return t('js8.view.col.callsign')
    case 'grid':
      return t('js8.view.col.grid')
    case 'distance':
      return t('js8.view.col.distance')
    case 'azimuth':
      return t('js8.view.col.azimuth')
    case 'log':
      return t('js8.view.col.log')
    case 'logName':
      return t('js8.view.col.logName')
    case 'logComment':
      return t('js8.view.col.logComment')
    default:
      return bandColumnLabel(c)
  }
}

/** "Clear" one list and "Clear All Lists…" (UI_Constructor.cpp:783-800, :913-916, :1053-1056). */
export function js8ClearEntries(clear: () => void, clearAll: () => void): Js8MenuEntry[] {
  return [
    { kind: 'item', label: t('js8.lists.clear'), onSelect: clear },
    { kind: 'item', label: t('js8.lists.clearAll'), onSelect: clearAll },
  ]
}

/** JS8Call's "Show Columns…" (mainwindow.cpp:4669-4734): the labels, then each column. */
function columnEntries(table: 'band' | 'call', view: Js8ViewPrefs, change: (v: Js8ViewPrefs) => void): Js8MenuEntry[] {
  return [
    { kind: 'check', label: t('js8.view.labels'), checked: view.labels, onChange: (on) => change({ ...view, labels: on }) },
    { kind: 'sep' },
    ...(table === 'band'
      ? JS8_BAND_COLUMNS.map((c) => ({
          kind: 'check' as const,
          label: bandColumnLabel(c),
          checked: view.band.columns[c],
          onChange: (on: boolean) => change({ ...view, band: { ...view.band, columns: { ...view.band.columns, [c]: on } } }),
        }))
      : JS8_CALL_COLUMNS.map((c) => ({
          kind: 'check' as const,
          label: callColumnLabel(c),
          checked: view.call.columns[c],
          onChange: (on: boolean) => change({ ...view, call: { ...view.call, columns: { ...view.call.columns, [c]: on } } }),
        }))),
  ]
}

/** Band activity's ⋯: Sort By, Show Columns, Show Band Heartbeats and ACKs, then the clears. */
export function js8BandViewEntries(view: Js8ViewPrefs, change: (v: Js8ViewPrefs) => void, clears: Js8MenuEntry[]): Js8MenuEntry[] {
  return [
    {
      kind: 'sub',
      label: t('js8.view.sortBy'),
      entries: [
        {
          kind: 'radio',
          value: view.band.sort,
          options: JS8_BAND_SORTS.map((v) => ({ value: v, label: bandSortLabel(v) })),
          onChange: (v) => change({ ...view, band: { ...view.band, sort: v as Js8BandSort } }),
        },
      ],
    },
    { kind: 'sub', label: t('js8.view.showColumns'), entries: columnEntries('band', view, change) },
    { kind: 'check', label: t('js8.view.showHb'), checked: view.showHb, onChange: (on) => change({ ...view, showHb: on }) },
    { kind: 'sep' },
    ...clears,
  ]
}

/** The call list's ⋯: Sort By, Show Columns, Add New Station or Group…, then the clears. */
export function js8CallViewEntries(view: Js8ViewPrefs, change: (v: Js8ViewPrefs) => void, onAdd: () => void, clears: Js8MenuEntry[]): Js8MenuEntry[] {
  return [
    {
      kind: 'sub',
      label: t('js8.view.sortBy'),
      entries: [
        {
          kind: 'radio',
          value: view.call.sort,
          options: JS8_CALL_SORTS.map((v) => ({ value: v, label: callSortLabel(v) })),
          onChange: (v) => change({ ...view, call: { ...view.call, sort: v as Js8CallSort } }),
        },
      ],
    },
    { kind: 'sub', label: t('js8.view.showColumns'), entries: columnEntries('call', view, change) },
    { kind: 'item', label: t('js8.lists.add'), onSelect: onAdd },
    { kind: 'sep' },
    ...clears,
  ]
}

/** What a row's "Jump to …" entries need: where it is, and whether a jump may happen now. */
export interface Js8Jump {
  /** The row's offset, or null for a row with none. */
  hz: number | null
  speed: Js8Speed | null
  /** My transmit speed: no "Jump to … speed" to the one I am on (UI_Constructor.cpp:964). */
  current: Js8Speed | null
  /** JS8Call refuses to move the offset while a message is queued or going out, unless "Allow TX
   *  offset changes while sending" is on (`changeFreq`, mainwindow.cpp:5381-5392). */
  offsetLocked: boolean
  /** JS8Call's "Center filter at …Hz": the decode filter centred on the row's offset, offered only
   *  while the filter is on (`js8CentreFilterItem`); null for none. */
  centreFilter: MenuItem | null
  onJump: (hz: number) => void
  onSpeed: (speed: Js8Speed) => void
}

/** "Jump to …Hz", "Center filter at …Hz" and "Jump to … speed" (UI_Constructor.cpp:942-988,
 *  :1231-1263). */
export function js8JumpEntries(j: Js8Jump): Js8MenuEntry[] {
  const out: Js8MenuEntry[] = []
  if (j.hz != null) {
    const hz = j.hz
    out.push({ kind: 'item', label: t('js8.lists.jumpOffset', { offset: hz }), onSelect: () => j.onJump(hz), disabled: j.offsetLocked })
    const f = j.centreFilter
    if (f) out.push({ kind: 'item', label: f.label, onSelect: f.onSelect, disabled: f.disabled })
  }
  if (j.speed && j.current && j.speed !== j.current) {
    const speed = j.speed
    out.push({ kind: 'item', label: t('js8.lists.jumpSpeed', { speed: speedName(speed) }), onSelect: () => j.onSpeed(speed) })
  }
  return out
}

/** A Band activity row's menu: the jumps, Remove Activity, the clears. */
export function js8BandRowEntries(jump: Js8Jump, onRemove: () => void, clears: Js8MenuEntry[]): Js8MenuEntry[] {
  return [...js8JumpEntries(jump), { kind: 'sep' }, { kind: 'item', label: t('js8.lists.removeActivity'), onSelect: onRemove }, { kind: 'sep' }, ...clears]
}

/** A call list row's menu (UI_Constructor.cpp:1225-1335): for a station the jumps, its messages, the
 *  pin, add, remove and block; for @ALLCALL or a group its messages, add and remove; the clears. */
export function js8CallRowEntries(o: {
  group: boolean
  /** Null for a row with nothing heard (added by hand, or heard through a relay). */
  jump: Js8Jump | null
  hasMessages: boolean
  onShowInbox: () => void
  pinned: boolean
  onPin: () => void
  onAdd: () => void
  canRemove: boolean
  onRemove: () => void
  blocked: boolean
  canBlock: boolean
  onBlock: () => void
  clears: Js8MenuEntry[]
}): Js8MenuEntry[] {
  return [
    ...(o.jump ? [...js8JumpEntries(o.jump), { kind: 'sep' as const }] : []),
    { kind: 'item', label: t('js8.lists.showInbox'), onSelect: o.onShowInbox, disabled: !o.hasMessages },
    ...(o.group ? [] : [{ kind: 'item' as const, label: o.pinned ? t('js8.lists.unpin') : t('js8.lists.pin'), onSelect: o.onPin }]),
    { kind: 'sep' },
    { kind: 'item', label: t('js8.lists.add'), onSelect: o.onAdd },
    { kind: 'item', label: o.group ? t('js8.lists.removeGroup') : t('js8.lists.removeStation'), onSelect: o.onRemove, disabled: !o.canRemove },
    ...(o.group
      ? []
      : [{ kind: 'item' as const, label: o.blocked ? t('js8.lists.unblock') : t('js8.lists.block'), onSelect: o.onBlock, disabled: !o.canBlock }]),
    { kind: 'sep' },
    ...o.clears,
  ]
}
