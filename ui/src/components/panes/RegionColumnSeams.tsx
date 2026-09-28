// THE COLUMN DIVIDERS OF A GRID COCKPIT'S PANE REGION (layout L2). Phone, CW and JS8 render them
// through this one component, so the three cannot drift apart.
//
// What each tier gets (useRegionCols' `cols`, the rendered track count):
//   · three columns — a split divider between the two feed columns (their fr shares, the record's
//     `cols.a` / `cols.b`, painted on the region as `--cockpit-col-a/-b`), and a width divider on
//     the last column's left edge (`cols.log`, CSS px, `--cockpit-col-log`);
//   · two columns — the width divider alone, between the feed column and the last one;
//   · one — none. That is the stacking flow (or a region the ⊞ menu emptied down to one column),
//     and a stack cannot be divided sideways.
//
// WHERE A DIVIDER SITS: an absolutely positioned child of the region, placed by `grid-column` over
// the gap before its track (cockpit-panes.css `.cockpit-colseam`). Not a grid child that would take
// a track or a cell, and not inside a column, whose overflow would clip it.
//
// Every divider here is a PaneSeam: focusable, arrows/Home/End/Backspace, announced values, a live
// paint during a drag and ONE commit on release through `setCols` — one undoable step in the
// section's panel record, which ⊞ Undo and Reset layout cover like any other change. The log
// width's clamp (24em … half the region) is the layout's own (features/paneColumns); the divider
// measures the column it sizes, so the keys and the drag stop exactly where the column does.
//
// THE STOP LINE is not near any of this: a divider moves the boundary between columns and nothing
// else. No pane changes parent (the columns keep their keys), no control moves between the header,
// the region and the TX dock, and nothing here has an id in a pane vocabulary.
import { useCallback, useEffect, useState, type RefObject } from 'react'
import { PaneSeam, elZoom } from '../PaneSeam'
import { logColRange, logColValue } from '../../features/paneColumns'
import type { PanelColId, PanelCols } from '../../features/panelState'
import type { RegionCols } from '../../useRegionCols'

type Box = RefObject<HTMLElement | null>
type SetCols = (updates: Partial<Record<PanelColId, number | null>>) => void

export interface RegionColumnSeamsProps {
  /** The pane region (`.cockpit-panes`): the dividers' container and the tokens' owner. */
  region: Box
  /** The rendered track count. */
  cols: RegionCols
  /** The rendered columns, in track order. */
  tracks: readonly Box[]
  /** The section's stored column widths. */
  stored: PanelCols | undefined
  setCols: SetCols
  /** Accessible name of the divider between the two feed columns (three columns only). */
  splitLabel: string
  /** Accessible name of the divider that sets the last column's width. */
  widthLabel: string
}

export function RegionColumnSeams({ region, cols, tracks, stored, setCols, splitLabel, widthLabel }: RegionColumnSeamsProps) {
  if (cols === 1 || tracks.length < cols) return null
  return (
    <>
      {cols === 3 && (
        <PaneSeam
          above={tracks[0]}
          below={tracks[1]}
          axis="x"
          columnsOn={region}
          varName="--cockpit-col"
          className="cockpit-colseam cockpit-colseam-2"
          onCommit={(a, b) => setCols({ a, b })}
          onReset={() => setCols({ a: null, b: null })}
          label={splitLabel}
        />
      )}
      <WidthSeam region={region} track={tracks[cols - 1]} at={cols} stored={stored?.log} setCols={setCols} label={widthLabel} />
    </>
  )
}

/** The divider on the last column's left edge: it sizes that column, so moving it LEFT grows it. */
function WidthSeam({
  region,
  track,
  at,
  stored,
  setCols,
  label,
}: {
  region: Box
  track: Box
  at: 2 | 3
  stored: number | undefined
  setCols: SetCols
  label: string
}) {
  // Where it stands, measured: the column's rendered width, and the range the layout honours for
  // it (24em … half the region). null while nothing is laid out (a hidden keep-alive host).
  const [view, setView] = useState<{ value: number; min: number; max: number } | null>(null)
  const settle = useCallback(() => {
    const r = region.current
    const t = track.current
    if (!r || !t) return
    const z = elZoom(r)
    const w = t.getBoundingClientRect().width / z
    if (!(w > 0) || !(r.clientWidth > 0)) return
    const font = parseFloat(getComputedStyle(r).fontSize)
    const { min, max } = logColRange(r.clientWidth, Number.isFinite(font) && font > 0 ? font : 16)
    setView((v) => (v && v.value === w && v.min === min && v.max === max ? v : { value: w, min, max }))
  }, [region, track])

  // On mount, on every resize of the region (a window, a zoom, a scope drag), and on every change
  // of the column's own width (a commit re-renders the tokens; the tier moves the divider).
  // A PASSIVE effect, not a layout one: this divider is a child of the region, and a component's
  // layout effects run before an ANCESTOR's ref is attached. Only the announced values, the keys and
  // a drag wait for it, and not past the first frame: where the divider sits is the sheet's.
  useEffect(() => {
    settle()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => settle())
    if (region.current) ro.observe(region.current)
    if (track.current) ro.observe(track.current)
    return () => ro.disconnect()
  }, [region, track, at, settle])

  return (
    <PaneSeam
      axis="x"
      className={`cockpit-colseam cockpit-colseam-${at}`}
      label={label}
      value={view?.value ?? null}
      min={view?.min ?? 0}
      max={view?.max ?? 0}
      grows={-1}
      onPaint={(px) => region.current?.style.setProperty('--cockpit-col-log', logColValue(px))}
      // Back to what the record paints: its width, or no token at all (the sheet's default).
      onCancel={() => {
        const r = region.current
        if (!r) return
        if (stored != null) r.style.setProperty('--cockpit-col-log', logColValue(stored))
        else r.style.removeProperty('--cockpit-col-log')
      }}
      onCommit={(px: number) => {
        setCols({ log: px })
        // The column renders exactly this (the layout clamps it the same way), so the next key
        // steps from here without waiting for the resize to be observed.
        setView((v) => (v ? { ...v, value: Math.round(px) } : v))
      }}
      onReset={() => setCols({ log: null })}
    />
  )
}
