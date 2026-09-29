// THE APRS STATION RAIL'S WIDTH DIVIDER (layout L7): the boundary between the rail (the beacon and
// message forms and the station list) and the map. Connect's rail handle generalised: a PaneSeam
// of the VALUE kind, so it is focusable, answers the arrows, Home/End and Backspace, announces
// where it stands, paints live during a drag and commits once on release. The host stores the
// width; the LAYOUT clamps it (features/aprsRail: 260 px … half the body, never less than the stock
// 420 px), and this divider measures the rail the layout drew, so the keys and the drag stop
// exactly where the rail does.
//
// WHERE IT SITS: an absolutely positioned child of the body, placed by `grid-column` on the second
// track and pulled back over the gap before it — the column-divider technique of cockpit-panes.css
// `.cockpit-colseam`, on APRS's own grid. It takes no track and no cell, so the body lays out
// exactly as it did without it. With the map on the left the rail is the second track: the
// divider is in the same gap, and moving it LEFT widens the rail.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): the accessible name is the
// caller's `label`.
import { useCallback, useEffect, useState, type RefObject } from 'react'
import { PaneSeam, elZoom } from './PaneSeam'
import { aprsRailRange, aprsRailValue } from '../features/aprsRail'

export interface AprsRailSeamProps {
  /** The body grid (`.aprs-body`): the divider's container and the width token's owner. */
  body: RefObject<HTMLElement | null>
  /** The rail (`.aprs-rail`): what the divider sizes, measured. */
  rail: RefObject<HTMLElement | null>
  /** The stored width, CSS px, or null for the stock width. */
  stored: number | null
  /** The map is on the left, so the rail is the second track. */
  mapLeft: boolean
  /** Store a width (a key's step, a drag's release), or null for the stock width back. */
  onCommit: (px: number | null) => void
  label: string
}

export function AprsRailSeam({ body, rail, stored, mapLeft, onCommit, label }: AprsRailSeamProps) {
  // Where it stands, measured: the rail's rendered width and the range the layout honours for it.
  // null while nothing is laid out (a keep-alive host's hidden box).
  const [view, setView] = useState<{ value: number; min: number; max: number } | null>(null)
  const settle = useCallback(() => {
    const b = body.current
    const r = rail.current
    if (!b || !r) return
    // Both from rects, in CSS px: the body's `clientWidth` is rounded to a whole pixel, and the
    // half of it the layout pays out is not (at 1024×768 the rail stopped 0.6 px short of the
    // divider's announced end).
    const z = elZoom(b)
    const w = r.getBoundingClientRect().width / z
    const bw = b.getBoundingClientRect().width / z
    if (!(w > 0) || !(bw > 0)) return
    const { min, max } = aprsRailRange(bw)
    setView((v) => (v && v.value === w && v.min === min && v.max === max ? v : { value: w, min, max }))
  }, [body, rail])

  // On mount, on every resize of the body (a window, a zoom) and of the rail (a commit, the
  // layout's own clamp). A PASSIVE effect: the divider is a child of the body, and a component's
  // layout effects run before an ANCESTOR's ref is attached. Only the announced values, the keys
  // and a drag wait for it; where the divider sits is the sheet's.
  useEffect(() => {
    settle()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => settle())
    if (body.current) ro.observe(body.current)
    if (rail.current) ro.observe(rail.current)
    return () => ro.disconnect()
  }, [body, rail, settle, mapLeft])

  return (
    <PaneSeam
      axis="x"
      className="aprs-railseam"
      label={label}
      value={view?.value ?? null}
      min={view?.min ?? 0}
      max={view?.max ?? 0}
      grows={mapLeft ? -1 : 1}
      onPaint={(px) => body.current?.style.setProperty('--aprs-rail-w', aprsRailValue(px))}
      // Back to what the host paints: the stored width, or no token at all (the stock width).
      onCancel={() => {
        const b = body.current
        if (!b) return
        if (stored != null) b.style.setProperty('--aprs-rail-w', aprsRailValue(stored))
        else b.style.removeProperty('--aprs-rail-w')
      }}
      onCommit={(px: number) => {
        onCommit(px)
        // The rail renders exactly this (the layout clamps it the same way), so the next key steps
        // from here without waiting for the resize to be observed.
        setView((v) => (v ? { ...v, value: Math.round(px) } : v))
      }}
      onReset={() => onCommit(null)}
    />
  )
}
