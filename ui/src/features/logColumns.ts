// THE LOGBOOK'S COLUMNS — one model the header, every row and the column chooser are all built from.
//
// The table used to be two hand-written CSS grid templates (13 tracks, or 21 with "More columns")
// and a header and a row that each listed their cells by hand. Three lists had to agree, and the
// guard that held them together counted cells against tracks after the fact. Here there is one
// list: a column is shown or not, and the template, the header and the row are all cut from the
// same `visibleColumns` answer, so a cell without a track (the action cluster wrapped onto a second
// grid row under the callsign, 2026-08-23) cannot be written.
//
// ⭐ THE DEFAULTS ARE TODAY'S TABLE. With nothing saved, the columns, their order and every track
// are exactly the compact table's; an operator who had "More columns" on gets that table, read from
// the same browser key it was always kept under. Nothing changes for anyone until they open the
// chooser.
//
// What is saved (`settings.json` → `logbookColumns`, through its one writer `set_logbook_columns`):
// the default columns turned OFF, the other columns turned ON, the width of each column the
// operator dragged, and — once a header has been dragged to another place — the order. Deltas, not
// a list: a column added to the defaults later still shows for an operator who customised before
// it existed, and a column the saved order does not name takes its place beside its default
// neighbour. A choice saved before the order existed has no `order` and loads exactly as it did.
//
// ⚠️ A SAVED WIDTH IS CLAMPED ON LOAD (the layout contract: anything persisted that encodes a size
// is clamped against the current window, not only at drag time). A width dragged out on a 4K
// monitor is cut to a share of the window it is read on, so it cannot push the action cluster off
// a 1366 px laptop.

import { t, type MessageKey } from '../i18n'
import type { LoggedQso, UploadStatus } from '../types'
import type { LogSortKey } from './logQuery'

export type LogColumnGroup = 'contact' | 'location' | 'qsl' | 'uploads' | 'contest' | 'station'

export interface LogColumn {
  id: string
  /** The header's catalog key. */
  labelKey: MessageKey
  /** The track with no width saved: the compact table's own share. */
  track: string
  /** The track in the fitted narrow table (`sm`/`xs`), where it differs. */
  narrowTrack?: string
  /** The narrowest a drag may make it, in px. */
  minPx: number
  group: LogColumnGroup
  /** In today's compact table. */
  base?: boolean
  /** In the old "More columns" table. */
  more?: boolean
  /** Kept by the fitted narrow table at `sm`/`xs`. */
  narrow?: boolean
  /** Cannot be hidden: the callsign names the row, the actions are its controls. */
  fixed?: boolean
  /** The engine's sort for it. Every column that shows data sorts; only the action cluster does
   *  not. A passthrough tag (county, zones, continent, distance, the received dates) is read by
   *  the passthrough's own key, and an upload mark or an exchange by its table's, one lookup per
   *  contact, as the search reads the SUBMODE. */
  sort?: LogSortKey
  /** The catalog parameter the label takes, if any (the upload columns name their service). */
  labelVars?: Record<string, string>
}

/** The row's action cluster: ✎ Edit and the ⋯ menu that holds everything else the row can do. Its
 *  width is MEASURED (ui/layout-harness/measure-logbook-actions.sh, guarded by
 *  styles-logbook-actions.test.tsx): two 32 px targets 8 px apart need 72 px on every face installed
 *  (the glyphs sit inside the buttons' 32 px floor, so there is no spread between faces); 80 leaves
 *  room to spare. Fixed, not a share: the width the cluster does not need goes to the columns that
 *  show the contact. Never resizable and never hidden. */
export const ACTIONS_TRACK = 'minmax(80px, 80px)'

/** The px a column's text needs to be read whole at the default size — a callsign with a portable
 *  prefix or suffix, a full `2026-10-28`, a full `14:06:22` — so a narrow window takes width from
 *  Country and Notes, which cut to a tooltip, and never from what a contact is scanned by. Read in
 *  real Chrome at 1024×768 (the supported floor) by `scripts/browser-probe --desktop logbook`. */
const CALL_TRACK = 'minmax(96px, 1fr)'
const DATE_TRACK = 'minmax(84px, 1fr)'
const TIME_TRACK = 'minmax(64px, 0.6fr)'

/** Every column, in the order the table shows them. */
export const LOG_COLUMNS: readonly LogColumn[] = [
  { id: 'call', labelKey: 'logbook.column.call', track: CALL_TRACK, minPx: 64, group: 'contact', base: true, narrow: true, fixed: true, sort: 'call' },
  { id: 'country', labelKey: 'logbook.column.country', track: '1.3fr', minPx: 48, group: 'location', base: true, narrow: true, sort: 'country' },
  { id: 'band', labelKey: 'logbook.column.band', track: '0.7fr', minPx: 40, group: 'contact', base: true, narrow: true, sort: 'band' },
  { id: 'freq', labelKey: 'logbook.column.freq', track: '1fr', minPx: 48, group: 'contact', base: true, sort: 'freq' },
  { id: 'mode', labelKey: 'logbook.column.mode', track: '0.7fr', minPx: 40, group: 'contact', base: true, sort: 'mode' },
  { id: 'sent', labelKey: 'logbook.column.sent', track: '0.6fr', minPx: 32, group: 'contact', base: true, sort: 'sent' },
  { id: 'rcvd', labelKey: 'logbook.column.rcvd', track: '0.6fr', minPx: 32, group: 'contact', base: true, sort: 'rcvd' },
  { id: 'date', labelKey: 'logbook.column.date', track: DATE_TRACK, minPx: 56, group: 'contact', base: true, sort: 'time' },
  { id: 'time', labelKey: 'logbook.column.time', track: TIME_TRACK, minPx: 40, group: 'contact', base: true, sort: 'time' },
  { id: 'park', labelKey: 'logbook.column.park', track: '0.9fr', minPx: 40, group: 'location', base: true, sort: 'park' },
  { id: 'qsl', labelKey: 'logbook.column.qsl', track: '0.7fr', minPx: 32, group: 'qsl', base: true, narrow: true, sort: 'qsl' },
  // The old "More columns" block, in its order.
  { id: 'end', labelKey: 'logbook.column.end', track: '0.9fr', minPx: 40, group: 'contact', more: true, sort: 'end' },
  { id: 'myGrid', labelKey: 'logbook.column.myGrid', track: '0.8fr', minPx: 40, group: 'station', more: true, sort: 'myGrid' },
  { id: 'myRig', labelKey: 'logbook.column.myRig', track: '1.1fr', minPx: 40, group: 'station', more: true, sort: 'myRig' },
  { id: 'name', labelKey: 'logbook.column.name', track: '1fr', minPx: 40, group: 'contact', more: true, sort: 'name' },
  { id: 'qth', labelKey: 'logbook.column.qth', track: '1fr', minPx: 40, group: 'location', more: true, sort: 'qth' },
  { id: 'state', labelKey: 'logbook.column.state', track: '0.5fr', minPx: 32, group: 'location', more: true, sort: 'state' },
  { id: 'power', labelKey: 'logbook.column.power', track: '0.6fr', minPx: 32, group: 'station', more: true, sort: 'power' },
  { id: 'operator', labelKey: 'logbook.column.operator', track: '0.8fr', minPx: 40, group: 'station', more: true, sort: 'operator' },
  // What the log stores and the table never showed. Off until the operator turns one on.
  { id: 'grid', labelKey: 'logbook.column.grid', track: '0.7fr', minPx: 40, group: 'location', sort: 'grid' },
  { id: 'iota', labelKey: 'logbook.column.iota', track: '0.7fr', minPx: 40, group: 'location', sort: 'iota' },
  { id: 'county', labelKey: 'logbook.column.county', track: '1fr', minPx: 40, group: 'location', sort: 'county' },
  { id: 'cqz', labelKey: 'logbook.column.cqz', track: '0.5fr', minPx: 28, group: 'location', sort: 'cqz' },
  { id: 'ituz', labelKey: 'logbook.column.ituz', track: '0.5fr', minPx: 28, group: 'location', sort: 'ituz' },
  { id: 'cont', labelKey: 'logbook.column.cont', track: '0.5fr', minPx: 28, group: 'location', sort: 'cont' },
  { id: 'distance', labelKey: 'logbook.column.distance', track: '0.7fr', minPx: 40, group: 'location', sort: 'distance' },
  { id: 'qslSentDate', labelKey: 'logbook.column.qslSentDate', track: '0.9fr', minPx: 56, group: 'qsl', sort: 'qslSentDate' },
  { id: 'qslRcvdDate', labelKey: 'logbook.column.qslRcvdDate', track: '0.9fr', minPx: 56, group: 'qsl', sort: 'qslRcvdDate' },
  { id: 'lotwRcvdDate', labelKey: 'logbook.column.lotwRcvdDate', track: '0.9fr', minPx: 56, group: 'qsl', sort: 'lotwRcvdDate' },
  { id: 'eqslRcvdDate', labelKey: 'logbook.column.eqslRcvdDate', track: '0.9fr', minPx: 56, group: 'qsl', sort: 'eqslRcvdDate' },
  { id: 'upLotw', labelKey: 'logbook.column.upload', labelVars: { service: 'LoTW' }, track: '0.6fr', minPx: 28, group: 'uploads', sort: 'upLotw' },
  { id: 'upEqsl', labelKey: 'logbook.column.upload', labelVars: { service: 'eQSL' }, track: '0.6fr', minPx: 28, group: 'uploads', sort: 'upEqsl' },
  { id: 'upQrz', labelKey: 'logbook.column.upload', labelVars: { service: 'QRZ' }, track: '0.6fr', minPx: 28, group: 'uploads', sort: 'upQrz' },
  { id: 'upClublog', labelKey: 'logbook.column.upload', labelVars: { service: 'Club Log' }, track: '0.6fr', minPx: 28, group: 'uploads', sort: 'upClublog' },
  { id: 'contest', labelKey: 'logbook.column.contest', track: '1fr', minPx: 48, group: 'contest', sort: 'contest' },
  { id: 'exchSent', labelKey: 'logbook.column.exchSent', track: '1fr', minPx: 40, group: 'contest', sort: 'exchSent' },
  { id: 'exchRcvd', labelKey: 'logbook.column.exchRcvd', track: '1fr', minPx: 40, group: 'contest', sort: 'exchRcvd' },
  // The tail: the notes, then the controls.
  { id: 'notes', labelKey: 'logbook.column.notes', track: '1.4fr', narrowTrack: '1fr', minPx: 48, group: 'contact', base: true, narrow: true, sort: 'notes' },
  { id: 'actions', labelKey: 'logbook.column.actions', track: ACTIONS_TRACK, minPx: 80, group: 'contact', base: true, narrow: true, fixed: true },
]

const BY_ID: ReadonlyMap<string, LogColumn> = new Map(LOG_COLUMNS.map((c) => [c.id, c]))

/** A column's name, as its header and the chooser print it. */
export function columnName(c: LogColumn): string {
  return t(c.labelKey, c.labelVars)
}

/** A chooser group's heading. */
export function groupName(g: LogColumnGroup): string {
  switch (g) {
    case 'contact':
      return t('logbook.columns.group.contact')
    case 'location':
      return t('logbook.columns.group.location')
    case 'qsl':
      return t('logbook.columns.group.qsl')
    case 'uploads':
      return t('logbook.columns.group.uploads')
    case 'contest':
      return t('logbook.columns.group.contest')
    case 'station':
      return t('logbook.columns.group.station')
  }
}

export function logColumn(id: string): LogColumn | undefined {
  return BY_ID.get(id)
}

/** The choice as `settings.json` keeps it (`logbookColumns`): deltas from the defaults. */
export interface LogColumnsSaved {
  /** Columns shown that the default table does not show. */
  shown: string[]
  /** Default columns the operator turned off. */
  hidden: string[]
  /** Dragged widths, px, by column id. */
  widths: Record<string, number>
  /** The operator's column order, every column id (shown or not), the action cluster aside — it is
   *  always last. Absent while the table is in its default order. */
  order?: string[]
}

export const DEFAULT_LOG_COLUMNS: LogColumnsSaved = { shown: [], hidden: [], widths: {} }

/** #239 "More columns", as this browser kept it before the column chooser. Read only while the
 *  operator has saved no column choice, so the table they had is the table they keep; any
 *  failure reads as off. */
const MORE_COLUMNS_KEY = 'nexus.logbook.moreColumns'

export function loadLegacyMoreColumns(): boolean {
  try {
    return window.localStorage.getItem(MORE_COLUMNS_KEY) === '1'
  } catch {
    return false
  }
}

/** Turn the old switch off, so a reset to the default table does not bring "More columns" back
 *  the next time nothing is saved. */
export function forgetLegacyMoreColumns(): void {
  try {
    window.localStorage.setItem(MORE_COLUMNS_KEY, '0')
  } catch {
    /* unavailable — nothing to forget */
  }
}

/** Nothing saved yet: the default table, or the old "More columns" one for an operator who had
 *  that switch on (kept in this browser, as it always was). */
export function legacyLogColumns(moreColumns: boolean): LogColumnsSaved {
  return moreColumns
    ? { shown: LOG_COLUMNS.filter((c) => c.more).map((c) => c.id), hidden: [], widths: {} }
    : DEFAULT_LOG_COLUMNS
}

/** The widest a saved column may be on a window `windowPx` wide: half of it. */
export function maxColumnPx(windowPx: number | null): number {
  return windowPx && windowPx > 0 ? Math.floor(windowPx / 2) : Infinity
}

const clampWidth = (c: LogColumn, px: number, windowPx: number | null): number =>
  Math.round(Math.max(c.minPx, Math.min(px, Math.max(c.minPx, maxColumnPx(windowPx)))))

/**
 * A saved choice made safe to show on a window `windowPx` effective CSS px wide (null: not known,
 * and only the floors apply): unknown ids dropped, a fixed column never hidden, the action
 * cluster never sized, every width a finite number within `[floor, half the window]`. Anything
 * that is not the saved shape at all reads as nothing saved.
 */
export function clampLogColumns(raw: unknown, windowPx: number | null): LogColumnsSaved {
  if (!raw || typeof raw !== 'object') return DEFAULT_LOG_COLUMNS
  const r = raw as Partial<Record<keyof LogColumnsSaved, unknown>>
  const ids = (v: unknown, keep: (c: LogColumn) => boolean): string[] =>
    Array.isArray(v)
      ? [...new Set(v.filter((id): id is string => typeof id === 'string'))].filter((id) => {
          const c = BY_ID.get(id)
          return !!c && keep(c)
        })
      : []
  const widths: Record<string, number> = {}
  if (r.widths && typeof r.widths === 'object') {
    for (const [id, px] of Object.entries(r.widths as Record<string, unknown>)) {
      const c = BY_ID.get(id)
      if (!c || c.id === 'actions' || typeof px !== 'number' || !Number.isFinite(px)) continue
      widths[id] = clampWidth(c, px, windowPx)
    }
  }
  return withOrder(
    {
      shown: ids(r.shown, (c) => !c.base),
      hidden: ids(r.hidden, (c) => !!c.base && !c.fixed),
      widths,
    },
    Array.isArray(r.order) ? columnOrder({ order: r.order }) : DEFAULT_ORDER,
  )
}

/** Every column the operator can move, in the default order: all but the action cluster. */
const DEFAULT_ORDER: readonly string[] = LOG_COLUMNS.filter((c) => c.id !== 'actions').map((c) => c.id)

/** The full column order a choice holds: its saved order (known ids, once each, never the
 *  actions), with every column it does not name placed after its nearest default predecessor. */
function columnOrder(saved: { order?: unknown }): string[] {
  const raw = Array.isArray(saved.order) ? saved.order : []
  const out = [...new Set(raw.filter((id): id is string => typeof id === 'string' && id !== 'actions' && BY_ID.has(id)))]
  if (!out.length) return [...DEFAULT_ORDER]
  DEFAULT_ORDER.forEach((id, i) => {
    if (out.includes(id)) return
    let at = 0
    for (let j = i - 1; j >= 0; j--) {
      const k = out.indexOf(DEFAULT_ORDER[j])
      if (k >= 0) {
        at = k + 1
        break
      }
    }
    out.splice(at, 0, id)
  })
  return out
}

/** `saved` with `order`, kept only when it is not the default one (so a table moved back to where
 *  it began saves exactly what it did before it was ever moved). */
function withOrder(saved: LogColumnsSaved, order: readonly string[]): LogColumnsSaved {
  const { order: _drop, ...rest } = saved
  const isDefault = order.length === DEFAULT_ORDER.length && order.every((id, i) => id === DEFAULT_ORDER[i])
  return isDefault ? rest : { ...rest, order: [...order] }
}

/** The columns a saved choice shows, in table order: the operator's, then the action cluster. */
export function visibleColumns(saved: LogColumnsSaved): LogColumn[] {
  const shown = new Set(saved.shown)
  const hidden = new Set(saved.hidden)
  const on = (c: LogColumn) => c.fixed || (c.base ? !hidden.has(c.id) : shown.has(c.id))
  return [...columnOrder(saved).map((id) => BY_ID.get(id)!), BY_ID.get('actions')!].filter(on)
}

/** The choice with column `id` moved to just `side` of column `anchor`. Dropped on the action
 *  cluster, it goes last before it; the cluster itself does not move. */
export function withColumnMoved(saved: LogColumnsSaved, id: string, anchor: string, side: 'before' | 'after'): LogColumnsSaved {
  if (id === 'actions' || id === anchor || !BY_ID.has(id) || !BY_ID.has(anchor)) return saved
  const order = columnOrder(saved).filter((x) => x !== id)
  const at = anchor === 'actions' ? order.length : order.indexOf(anchor) + (side === 'after' ? 1 : 0)
  order.splice(at, 0, id)
  return withOrder(saved, order)
}

/** The choice with column `id` moved one place left (-1) or right (1) among the columns `cols`
 *  renders — past its neighbour on screen, never past the action cluster. Unchanged at an end. */
export function withColumnStep(saved: LogColumnsSaved, cols: readonly LogColumn[], id: string, dir: -1 | 1): LogColumnsSaved {
  const movable = cols.filter((c) => c.id !== 'actions')
  const i = movable.findIndex((c) => c.id === id)
  const next = movable[i + dir]
  if (i < 0 || !next) return saved
  return withColumnMoved(saved, id, next.id, dir === 1 ? 'after' : 'before')
}

/** Whether the choice shows any column the default table does not. Such a table keeps every
 *  column it shows at every window size and scrolls sideways, as "More columns" always did; the
 *  default one fits the window and drops to its narrow columns at `sm`/`xs`. */
export function isWideTable(saved: LogColumnsSaved): boolean {
  return visibleColumns(saved).some((c) => !c.base)
}

/** The columns actually rendered: the choice, cut to the narrow columns when the window is `sm` or
 *  `xs` and the table is a fitted one. Header, rows and template all take THIS list. */
export function renderedColumns(saved: LogColumnsSaved, narrowWindow: boolean): LogColumn[] {
  const cols = visibleColumns(saved)
  return narrowWindow && !isWideTable(saved) ? cols.filter((c) => c.narrow) : cols
}

/** The `grid-template-columns` for `cols`: a dragged width in px, otherwise the column's share. */
export function logGridTemplate(cols: readonly LogColumn[], saved: LogColumnsSaved, narrowWindow: boolean): string {
  // The narrow tracks belong to the fitted narrow table only; a wide one keeps its own at any size.
  const fitted = narrowWindow && !isWideTable(saved)
  return cols
    .map((c) => {
      if (c.id === 'actions') return ACTIONS_TRACK
      const px = saved.widths[c.id]
      if (px !== undefined) return `${px}px`
      return (fitted && c.narrowTrack) || c.track
    })
    .join(' ')
}

/** rem of minimum table width per `fr` of share in a wide table: "More columns" held its 21 tracks
 *  (18 fr) at 123rem, so the old wide table comes out at exactly the width it always had. */
const WIDE_REM_PER_FR = 123 / 18

/** The wide table's minimum width (it scrolls sideways under it), or null for a fitted table. */
export function logTableMinWidth(cols: readonly LogColumn[], saved: LogColumnsSaved): string | null {
  if (!cols.some((c) => !c.base)) return null
  let fr = 0
  let px = 0
  for (const c of cols) {
    // The action cluster's track is a fixed width, not a share.
    const w = c.id === 'actions' ? c.minPx : saved.widths[c.id]
    if (w !== undefined) px += w
    else fr += parseFloat(/([\d.]+)fr\)?$/.exec(c.track)?.[1] ?? '0')
  }
  const rem = Math.round(fr * WIDE_REM_PER_FR * 100) / 100
  return px ? `calc(${rem}rem + ${px}px)` : `${rem}rem`
}

/** The choice with column `id` shown or hidden. A fixed column does not move. */
export function withColumnShown(saved: LogColumnsSaved, id: string, on: boolean): LogColumnsSaved {
  const c = BY_ID.get(id)
  if (!c || c.fixed) return saved
  const list = c.base ? 'hidden' : 'shown'
  const want = c.base ? !on : on
  const without = saved[list].filter((x) => x !== id)
  return { ...saved, [list]: want ? [...without, id] : without }
}

/** The choice with column `id` at `px` wide (clamped as a saved width is), or back to its share
 *  when `px` is null. The action cluster has no width to set. */
export function withColumnWidth(saved: LogColumnsSaved, id: string, px: number | null, windowPx: number | null): LogColumnsSaved {
  const c = BY_ID.get(id)
  if (!c || c.id === 'actions') return saved
  const widths = { ...saved.widths }
  if (px === null || !Number.isFinite(px)) delete widths[id]
  else widths[id] = clampWidth(c, px, windowPx)
  return { ...saved, widths }
}

// ── the values the new columns print ─────────────────────────────────────────────────────────

/** An ADIF tag the record keeps verbatim (`extra`), by name, as the import spelled it. */
export function extraTag(q: LoggedQso, tag: string): string | null {
  for (const [k, v] of q.extra ?? []) if (k.toUpperCase() === tag && v.trim()) return v.trim()
  return null
}

/** A passthrough tag's value as a number (ADIF numbers are decimal), or null when it is not one.
 *  `Number()` would read `0x10` as 16; the engine (`query.rs` `tag_num`) reads it as text, and so
 *  does this. */
export function adifNumber(v: string): number | null {
  if (/^[+-]?0[xob]/i.test(v.trim())) return null
  const n = Number(v)
  return Number.isFinite(n) ? n : null
}

/** An ADIF date (`YYYYMMDD`) as the table prints dates; anything else as it came. */
export function adifDate(v: string | null): string | null {
  if (!v) return null
  const m = /^(\d{4})(\d{2})(\d{2})$/.exec(v)
  return m ? `${m[1]}-${m[2]}-${m[3]}` : v
}

/** The sent and received exchange of a contest contact: the exchange as logged, else the ADIF
 *  strings, else the serial. Null on an ordinary contact. */
export function contestExchange(q: LoggedQso, side: 'sent' | 'rcvd'): string | null {
  const c = q.contest
  if (!c) return null
  const slots = side === 'sent' ? c.sent : c.rcvd
  const joined = (slots ?? []).map(([, v]) => v).filter(Boolean).join(' ')
  if (joined) return joined
  const str = side === 'sent' ? c.stxString : c.srxString
  if (str) return str
  const serial = side === 'sent' ? c.stx : c.srx
  return serial != null ? String(serial) : null
}

export type UploadMark = 'done' | 'waiting' | 'failed' | 'none'

/** An upload mark in words, for a tooltip and the detail view. */
export function uploadWord(m: UploadMark): string {
  switch (m) {
    case 'done':
      return t('logbook.upload.done')
    case 'waiting':
      return t('logbook.upload.waiting')
    case 'failed':
      return t('logbook.upload.failed')
    case 'none':
      return t('logbook.upload.none')
  }
}

/** One service's upload stamp as the table marks it. */
export function uploadMark(s: UploadStatus | undefined): UploadMark {
  if (!s) return 'none'
  if (s.outcome === 'accepted' || s.outcome === 'duplicate') return 'done'
  if (s.outcome === 'pending') return 'waiting'
  return 'failed'
}

/** The upload stamp a column shows. */
export function uploadOf(q: LoggedQso, id: string): UploadStatus | undefined {
  const u = q.upload
  switch (id) {
    case 'upLotw':
      return u?.lotw
    case 'upEqsl':
      return u?.eqsl
    case 'upQrz':
      return u?.qrz
    case 'upClublog':
      return u?.clublog
    default:
      return undefined
  }
}

/** The text an added column prints for `q`; null for nothing recorded. The upload columns print
 *  a mark, so they are not here. */
export function addedColumnText(q: LoggedQso, id: string): string | null {
  switch (id) {
    case 'grid':
      return q.grid?.trim() || null
    case 'iota':
      return q.ota?.iota?.trim() || null
    case 'county':
      return extraTag(q, 'CNTY')
    case 'cqz':
      return extraTag(q, 'CQZ')
    case 'ituz':
      return extraTag(q, 'ITUZ')
    case 'cont':
      return extraTag(q, 'CONT')
    case 'distance': {
      const v = extraTag(q, 'DISTANCE')
      const n = v === null ? null : adifNumber(v)
      return n !== null ? String(Math.round(n)) : v
    }
    case 'qslRcvdDate':
      return adifDate(extraTag(q, 'QSLRDATE'))
    case 'lotwRcvdDate':
      return adifDate(extraTag(q, 'LOTW_QSLRDATE'))
    case 'eqslRcvdDate':
      return adifDate(extraTag(q, 'EQSL_QSLRDATE'))
    case 'qslSentDate':
      return q.qslSent?.dateUnix ? new Date(q.qslSent.dateUnix * 1000).toISOString().slice(0, 10) : null
    case 'contest':
      return q.contest?.contestId?.trim() || null
    case 'exchSent':
      return contestExchange(q, 'sent')
    case 'exchRcvd':
      return contestExchange(q, 'rcvd')
    default:
      return null
  }
}
