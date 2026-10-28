// BAND ACTIVITY, as JS8Call 3.0.3's (`tableWidgetRXAll`, displayBandActivity.cpp): one row per
// frequency offset holding every frame heard there, joined, with the end mark after each message and
// the missing-frame mark where one did not come (features/js8Lists `js8BandRows`). The columns and
// the order are the operator's (the pane's ⋯ menu; JS8Call's defaults: offset, age, SNR, by offset),
// and so is whether heartbeats and their ACKs show. CQ rows, traffic to me and the two highlight
// lists are tinted, as JS8Call colours them.
//
// A display and the RX cursor: a double-click moves RX to the row's offset; the row's menu (its ⋯ or
// a right-click) is the cockpit's (`rowMenu`). Nothing here keys.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts): offsets, ages, reports and the received
// text are data; every word is in the catalog.
import { useState } from 'react'
import { t } from '../i18n'
import { HZ, ageLabel, dtLabel, fmtSnr, utcClock } from '../js8Vocab'
import { speedMark, speedName, type Js8BandRow } from '../features/js8Lists'
import type { Js8BandColumn } from '../features/js8View'
import { Js8ListMenu, type Js8MenuEntry } from './Js8ListMenu'

export function Js8BandActivity({
  rows,
  columns,
  labels,
  minimal = false,
  nowMs,
  selectedHz,
  onDoubleClick,
  rowMenu,
}: {
  /** The rows, sorted. */
  rows: readonly Js8BandRow[]
  columns: Readonly<Record<Js8BandColumn, boolean>>
  labels: boolean
  /** JS8Call's Minimal Column Labels: its short headings, and no units in the cells. */
  minimal?: boolean
  nowMs: number
  /** RX's offset: its row is the selected one. */
  selectedHz: number | null
  onDoubleClick: (offsetHz: number) => void
  rowMenu: (row: Js8BandRow) => readonly Js8MenuEntry[]
}) {
  const [menuFor, setMenuFor] = useState<number | null>(null)
  const head: Record<Js8BandColumn, string> = {
    offset: minimal ? t('js8.col.offset.minimal') : t('js8.col.offset'),
    timestamp: t('js8.col.age'),
    snr: minimal ? t('js8.col.snr.minimal') : t('js8.col.snr'),
    tdrift: minimal ? t('js8.col.tdrift.minimal') : t('js8.col.tdrift'),
    submode: minimal ? t('js8.col.speed.minimal') : t('js8.col.speed'),
  }
  const shown = (Object.keys(columns) as Js8BandColumn[]).filter((c) => columns[c])
  return (
    <div className="js8-offsets" title={t('js8.band.title')}>
      {rows.length === 0 ? (
        <div className="cw-decode-idle">{t('js8.panel.offsets.empty')}</div>
      ) : (
        <table className="js8-table js8-band-table">
          {labels && (
            <thead>
              <tr>
                {shown.map((c) => (
                  <th scope="col" key={c}>
                    {head[c]}
                  </th>
                ))}
                <th scope="col">{t('js8.band.col.text')}</th>
                <th scope="col" aria-label={t('js8.calls.col.menu')} />
              </tr>
            </thead>
          )}
          <tbody>
            {rows.map((r) => (
              <tr
                key={r.offsetHz}
                className={`js8-offset-row${r.offsetHz === selectedHz ? ' selected' : ''}${r.directed ? ' directed' : ''}${
                  r.cq ? ' cq' : ''
                }${r.highlight ? ` hl-${r.highlight}` : ''}${r.lowConf ? ' low' : ''}`}
                onDoubleClick={() => onDoubleClick(r.offsetHz)}
                onContextMenu={(e) => {
                  e.preventDefault()
                  setMenuFor(r.offsetHz)
                }}
                title={t('js8.panel.offsets.row.title')}
              >
                {columns.offset && (
                  <td className="js8-cell js8-freq">{minimal ? r.offsetHz : `${r.offsetHz} ${HZ}`}</td>
                )}
                {columns.timestamp && (
                  <td className="js8-cell js8-age" title={utcClock(r.atMs)}>
                    {ageLabel(nowMs - r.atMs)}
                  </td>
                )}
                {columns.snr && <td className="js8-cell js8-snr">{fmtSnr(r.snrDb)}</td>}
                {columns.tdrift && (
                  <td className="js8-cell js8-dt" title={t('js8.panel.offsets.dt.title')}>
                    {dtLabel(r.dtS, !minimal)}
                  </td>
                )}
                {columns.submode && (
                  <td className="js8-cell js8-speed" title={speedName(r.speed)}>
                    {speedMark(r.speed)}
                  </td>
                )}
                <td className="js8-cell js8-text" title={r.text}>
                  {r.text}
                </td>
                <td className="js8-cell js8-row-acts">
                  <Js8ListMenu
                    className="js8-row-menu"
                    label={t('js8.band.rowMenu.aria', { offset: r.offsetHz })}
                    open={menuFor === r.offsetHz}
                    onOpenChange={(o) => setMenuFor(o ? r.offsetHz : null)}
                    entries={() => rowMenu(r)}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}
