// THE CALL LIST, as JS8Call 3.0.3's call activity (displayCallActivity.cpp): @ALLCALL and my groups
// on top, then a row per station with one mark — ✕ blocked, ⚑ a message for me, ★ hearing me, ☎ a CQ
// in the last five minutes, ⚟ heard through a relay — and the columns the operator picked (Sort By
// and Show Columns are in the pane's ⋯ menu; JS8Call's defaults: callsign, age, SNR, offset, ✓, name,
// comment). Each row has JS8Call's right-click menu, also behind its ⋯.
//
// A display and a selection: a click on a call puts it in the To box. What the row's ⋯ offers is the
// cockpit's (`rowMenu`), and so is the last cell (`actions`: the one-click asks, which send, and the
// chips), so every transmit path stays where it was.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts): calls, grids, numbers and the mark glyphs
// are data; every word is in the catalog.
import { useState, type ReactNode } from 'react'
import { t } from '../i18n'
import { HZ, ageLabel, dtLabel, fmtSnr, utcClock } from '../js8Vocab'
import { azimuthLabel, azimuthTitle, azimuthTo, distanceLabel } from '../grid'
import type { Units } from '../units'
import type { Js8CallMark, Js8CallRow } from '../features/js8Lists'
import { JS8_MARK_GLYPH, speedMark, speedName } from '../features/js8Lists'
import type { Js8CallColumn } from '../features/js8View'
import { Js8ListMenu, type Js8MenuEntry } from './Js8ListMenu'

/** The logbook's answer about a call: the source of ✓, Name and Comment, and a grid fallback. */
export interface Js8LogDetail {
  count: number
  lastUnix: number | null
  grid: string
  name: string
  comment: string
}

/** Literal keys per mark, so the orphan guard sees each referenced. */
function markTitle(mark: Js8CallMark, row: Js8CallRow, nowMs: number): string {
  const since = row.markAtMs != null ? ageLabel(nowMs - row.markAtMs) : ''
  switch (mark) {
    case 'blocked':
      return t('js8.calls.mark.blocked')
    case 'message':
      return t('js8.station.unread.title', { call: row.call })
    case 'ack':
      return t('js8.calls.mark.ack', { since })
    case 'cq':
      return t('js8.calls.mark.cq', { since })
    case 'relay':
      return t('js8.calls.mark.relay', { through: row.through ?? '' })
  }
}

export function Js8CallList({
  groups,
  rows,
  columns,
  labels,
  minimal = false,
  nowMs,
  selectedCall,
  pins,
  myGrid,
  units,
  logDetail,
  logPending,
  highlight,
  onSelect,
  rowMenu,
  actions,
  header,
}: {
  /** @ALLCALL (while I take part) and my groups, each ⚑ while it holds an unread message. */
  groups: readonly { call: string; unread: boolean }[]
  /** The station rows, sorted. */
  rows: readonly Js8CallRow[]
  columns: Readonly<Record<Js8CallColumn, boolean>>
  labels: boolean
  /** JS8Call's Minimal Column Labels: its short headings, and no units in the cells. */
  minimal?: boolean
  nowMs: number
  selectedCall: string
  /** The operator's pinned calls (features/js8Pins): a pinned row is tinted. */
  pins: readonly string[]
  myGrid: string
  units: Units
  logDetail: ReadonlyMap<string, Js8LogDetail>
  /** Why a call's ✓ is not known yet ("—" with this title), or null once the log has answered. */
  logPending: (call: string) => string | null
  highlight: (call: string) => 'primary' | 'secondary' | null
  onSelect: (call: string) => void
  /** The row's menu (JS8Call's right-click menu), built when it opens. */
  rowMenu: (call: string, row: Js8CallRow | null) => readonly Js8MenuEntry[]
  /** The cockpit's own last cell of a station row: its one-click asks and its chips. */
  actions: (row: Js8CallRow) => ReactNode
  /** Above the table (the Remote page's history status). */
  header?: ReactNode
}) {
  const [menuFor, setMenuFor] = useState<string | null>(null)
  const anyMark = rows.some((r) => r.mark != null) || groups.some((g) => g.unread)
  const shownCols = (Object.keys(columns) as Js8CallColumn[]).filter((c) => columns[c])
  const span = (anyMark ? 1 : 0) + shownCols.length + 2
  const menu = (call: string, row: Js8CallRow | null) => (
    <Js8ListMenu
      className="js8-row-menu"
      label={t('js8.calls.rowMenu.aria', { call })}
      open={menuFor === call}
      onOpenChange={(o) => setMenuFor(o ? call : null)}
      entries={() => rowMenu(call, row)}
    />
  )
  const openMenu = (call: string) => (e: React.MouseEvent) => {
    e.preventDefault()
    setMenuFor(call)
  }
  const head: Record<Js8CallColumn, string> = {
    callsign: minimal ? t('js8.calls.col.callsign.minimal', { count: rows.length }) : t('js8.calls.col.callsign', { count: rows.length }),
    timestamp: t('js8.col.age'),
    snr: minimal ? t('js8.col.snr.minimal') : t('js8.col.snr'),
    offset: minimal ? t('js8.col.offset.minimal') : t('js8.col.offset'),
    tdrift: minimal ? t('js8.col.tdrift.minimal') : t('js8.col.tdrift'),
    submode: minimal ? t('js8.col.speed.minimal') : t('js8.col.speed'),
    grid: t('js8.calls.col.grid'),
    distance: minimal ? t('js8.calls.col.distance.minimal') : t('js8.calls.col.distance'),
    azimuth: t('js8.calls.col.azimuth'),
    log: t('js8.calls.col.log'),
    logName: t('js8.calls.col.logName'),
    logComment: t('js8.calls.col.logComment'),
  }
  return (
    <div className="js8-stations">
      {header}
      {rows.length === 0 && groups.length === 0 ? (
        <div className="cw-decode-idle">{t('js8.station.empty')}</div>
      ) : (
        <table className="js8-table js8-call-table">
          {labels && (
            <thead>
              <tr>
                {anyMark && <th scope="col" aria-label={t('js8.calls.col.mark')} />}
                {shownCols.map((c) => (
                  <th scope="col" key={c}>
                    {head[c]}
                  </th>
                ))}
                <th scope="col" aria-label={t('js8.calls.col.actions')} />
                <th scope="col" aria-label={t('js8.calls.col.menu')} />
              </tr>
            </thead>
          )}
          <tbody>
            {groups.map((g) => (
              <tr
                key={`group-${g.call}`}
                className={`js8-station js8-group${g.call === selectedCall ? ' selected' : ''}`}
                onContextMenu={openMenu(g.call)}
              >
                {anyMark && (
                  <td className="js8-cell js8-mark">
                    {g.unread && (
                      <span className="js8-chip" title={t('js8.calls.mark.groupMessage', { group: g.call })}>
                        {JS8_MARK_GLYPH.message}
                      </span>
                    )}
                  </td>
                )}
                <td className="js8-cell" colSpan={span - (anyMark ? 2 : 1)}>
                  <button type="button" className="js8-station-call" onClick={() => onSelect(g.call)} title={t('js8.station.select.title', { call: g.call })}>
                    {g.call}
                  </button>
                </td>
                <td className="js8-cell js8-row-acts">{menu(g.call, null)}</td>
              </tr>
            ))}
            {rows.map((r) => {
              const h = r.heard
              const det = logDetail.get(r.call)
              const grid = (h?.grid ?? '').trim() || det?.grid || ''
              const az = azimuthTo(myGrid, grid || null, null, null)
              const dist = distanceLabel(myGrid, grid || null, units)
              const pending = logPending(r.call)
              const hl = highlight(r.call)
              const pinned = pins.includes(r.call.toUpperCase())
              const cqTint = r.mark === 'cq'
              const cell: Record<Js8CallColumn, ReactNode> = {
                callsign: (
                  <td className="js8-cell" key="callsign">
                    <button
                      type="button"
                      className="js8-station-call"
                      onClick={() => onSelect(r.call)}
                      title={t('js8.station.select.title', { call: r.call })}
                    >
                      {r.call}
                    </button>
                  </td>
                ),
                timestamp: (
                  <td className="js8-cell js8-age" key="timestamp" title={r.lastMs != null ? utcClock(r.lastMs) : undefined}>
                    {r.lastMs != null ? ageLabel(nowMs - r.lastMs) : ''}
                  </td>
                ),
                snr: (
                  <td className="js8-cell js8-snr" key="snr">
                    {h ? fmtSnr(h.snrDb) : ''}
                  </td>
                ),
                // JS8Call's call activity prints the offset held in an int, so truncated
                // (`cd.offset`): the Band activity pane's rule.
                offset: (
                  <td className="js8-cell" key="offset">
                    {h ? (minimal ? `${Math.trunc(h.freqHz)}` : `${Math.trunc(h.freqHz)} ${HZ}`) : ''}
                  </td>
                ),
                tdrift: (
                  <td className={`js8-cell${r.dtS != null ? ' js8-dt' : ''}`} key="tdrift" title={t('js8.panel.offsets.dt.title')}>
                    {r.dtS != null ? dtLabel(r.dtS, !minimal) : ''}
                  </td>
                ),
                submode: (
                  <td className="js8-cell js8-speed" key="submode" title={h ? speedName(h.speed) : undefined}>
                    {h ? speedMark(h.speed) : ''}
                  </td>
                ),
                // The grid falls back to the one in the log when the station has not sent one, as
                // JS8Call's does, and shows four characters with the whole locator in the tooltip.
                grid: (
                  <td className="js8-cell js8-grid" key="grid" title={grid || undefined}>
                    {grid.slice(0, 4)}
                  </td>
                ),
                distance: dist ? (
                  <td className="js8-cell js8-dist" key="distance" title={t('js8.station.distance.title', { grid })}>
                    {dist}
                  </td>
                ) : (
                  <td className="js8-cell" key="distance" />
                ),
                // No entity centroid here: a heard station carries no country, so one with no grid
                // gets NO bearing rather than a rough one.
                azimuth:
                  az && azimuthLabel(az) ? (
                    <td className="js8-cell js8-az" key="azimuth" title={azimuthTitle(az)}>
                      {azimuthLabel(az)}
                    </td>
                  ) : (
                    <td className="js8-cell" key="azimuth" />
                  ),
                log: pending ? (
                  <td className="js8-cell js8-b4" key="log" title={pending}>
                    —
                  </td>
                ) : det ? (
                  <td
                    className="js8-cell js8-b4"
                    key="log"
                    title={t('js8.station.worked.title', {
                      count: det.count,
                      when: det.lastUnix ? new Date(det.lastUnix * 1000).toISOString().slice(0, 10) : '',
                    })}
                  >
                    ✓
                  </td>
                ) : (
                  <td className="js8-cell" key="log" />
                ),
                logName: det?.name ? (
                  <td className="js8-cell js8-opname" key="logName" title={t('js8.station.name.title')}>
                    {det.name}
                  </td>
                ) : (
                  <td className="js8-cell" key="logName" />
                ),
                logComment: det?.comment ? (
                  <td className="js8-cell js8-opcomment" key="logComment" title={t('js8.station.comment.title')}>
                    {det.comment}
                  </td>
                ) : (
                  <td className="js8-cell" key="logComment" />
                ),
              }
              return (
                <tr
                  key={r.call}
                  className={`js8-station${r.call === selectedCall ? ' selected' : ''}${pinned ? ' pinned' : ''}${
                    cqTint ? ' cq' : ''
                  }${hl ? ` hl-${hl}` : ''}${r.mark === 'message' ? ' unread' : ''}`}
                  onContextMenu={openMenu(r.call)}
                >
                  {anyMark && (
                    <td className="js8-cell js8-mark">
                      {r.mark && (
                        <span className={`js8-chip js8-mark-${r.mark}`} title={markTitle(r.mark, r, nowMs)}>
                          {JS8_MARK_GLYPH[r.mark]}
                        </span>
                      )}
                    </td>
                  )}
                  {shownCols.map((c) => cell[c])}
                  <td className="js8-cell js8-acts-cell">
                    <span className="js8-station-acts">{actions(r)}</span>
                  </td>
                  <td className="js8-cell js8-row-acts">{menu(r.call, r)}</td>
                </tr>
              )
            })}
          </tbody>
        </table>
      )}
    </div>
  )
}
