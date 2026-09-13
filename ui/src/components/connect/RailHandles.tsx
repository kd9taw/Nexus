// Connect's rail handles (close + resize, 2026-09-13): the separators that size the two pane
// rails and split each rail between its two panes — "make the map bigger".
//
// Same discipline as Splitter / SplitterSeam: a pointer drag paints CSS variables LIVE (no
// React render per move — the map is a sibling of every rail) and commits once on release. It
// differs from both in two ways the brief required:
//   · both handles are FOCUSABLE `role="separator"`s with arrow-key steps, Home/End and a
//     double-click reset — a resize only a mouse can reach is not accessible;
//   · a stored width is CLAMPED against the current box on load, on every resize of the grid,
//     and on every step — never only at drag time (features/connectRails).
//
// Neither handle takes a track or a flex slot: each is absolutely positioned in a gap the grid
// already has, so the unconfigured layout's rectangles are exactly what they were before.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The separators'
// accessible names arrive from ConnectView; the tooltip sentence they share is here.
import { useCallback, useLayoutEffect, useRef, useState } from 'react'
import type { KeyboardEvent, PointerEvent as ReactPointerEvent, RefObject } from 'react'
import { t } from '../../i18n'
import { elZoom } from '../Splitter'
import { MIN_SHARE, seamShares } from '../../features/panelState'
import {
  RAIL_MAX,
  RAIL_MIN,
  RAIL_STEP,
  RAIL_STEP_BIG,
  clampRail,
  fitRails,
  loadRailWidths,
  railRoom,
  saveRailWidths,
  type RailSide,
  type RailWidths,
} from '../../features/connectRails'

/** The sheet's fallback when no tier sets `--cn-rail` (md and up). */
const DEFAULT_RAIL_PX = 300

/** The grid box in CSS px. Rect is VISUAL px (zoom-multiplied); padding, gap and the tier
 *  default come from computed style, which is already CSS px. */
function geometry(el: HTMLElement) {
  const cs = getComputedStyle(el)
  const def = parseFloat(cs.getPropertyValue('--cn-rail'))
  return {
    w: el.getBoundingClientRect().width / elZoom(el),
    pad: parseFloat(cs.paddingLeft),
    gap: parseFloat(cs.columnGap),
    defaultPx: Number.isFinite(def) && def > 0 ? def : DEFAULT_RAIL_PX,
  }
}

export interface RailWidthsApi {
  /** What the grid renders per side: a fitted stored width, or null for the tier default. */
  applied: RailWidths
  /** The width a side renders at right now. */
  widthOf: (side: RailSide) => number
  /** The widest `side` may be in the current box, given the other rail. */
  maxOf: (side: RailSide) => number
  /** A candidate width for `side`, clamped against the box as it is NOW (a drag paints this). */
  clamp: (side: RailSide, px: number) => number
  /** Clamp, apply and persist (a keyboard step; a drag's release). */
  commit: (side: RailSide, px: number) => void
  /** Back to the tier default for one side / both. */
  reset: (side: RailSide) => void
  resetAll: () => void
}

export function useRailWidths(
  gridRef: RefObject<HTMLElement | null>,
  present: { left: boolean; right: boolean },
): RailWidthsApi {
  // The operator's PREFERENCE, as stored. A re-clamp never writes it.
  const prefRef = useRef<RailWidths | null>(null)
  if (prefRef.current === null) prefRef.current = loadRailWidths()
  const [view, setView] = useState<{ applied: RailWidths; room: number; defaultPx: number }>({
    applied: { left: null, right: null },
    room: Infinity,
    defaultPx: DEFAULT_RAIL_PX,
  })
  const viewRef = useRef(view)
  viewRef.current = view
  const presentRef = useRef(present)
  presentRef.current = present

  const measure = useCallback(() => {
    const el = gridRef.current
    if (!el) return null
    const g = geometry(el)
    const p = presentRef.current
    return { room: railRoom(g.w, g.pad, g.gap, 1 + (p.left ? 1 : 0) + (p.right ? 1 : 0)), defaultPx: g.defaultPx }
  }, [gridRef])

  const fit = useCallback(() => {
    const m = measure()
    if (!m) return
    const applied = fitRails(prefRef.current!, { room: m.room, present: presentRef.current, defaultPx: m.defaultPx })
    setView((v) =>
      v.room === m.room &&
      v.defaultPx === m.defaultPx &&
      v.applied.left === applied.left &&
      v.applied.right === applied.right
        ? v
        : { applied, room: m.room, defaultPx: m.defaultPx },
    )
  }, [measure])

  // Before first paint, and whenever a rail opens or closes (the room changes by a gap and a
  // track even though the box did not move).
  useLayoutEffect(() => {
    fit()
  }, [fit, present.left, present.right])

  // Every resize of the grid — window, zoom, or a pop-out's own box. A 0×0 fire (keep-alive
  // host hidden) measures as unbounded, and the next real fire clamps before that box paints.
  useLayoutEffect(() => {
    const el = gridRef.current
    if (!el || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => fit())
    ro.observe(el)
    return () => ro.disconnect()
  }, [fit, gridRef])

  const widthOf = (side: RailSide) => viewRef.current.applied[side] ?? viewRef.current.defaultPx
  const otherOf = (side: RailSide) => {
    const o: RailSide = side === 'left' ? 'right' : 'left'
    return presentRef.current[o] ? widthOf(o) : 0
  }
  const clamp = (side: RailSide, px: number) => clampRail(px, otherOf(side), measure()?.room ?? viewRef.current.room)
  const apply = (next: RailWidths) => {
    prefRef.current = next
    saveRailWidths(next)
    setView((v) => ({ ...v, applied: { left: next.left, right: next.right } }))
  }

  return {
    applied: view.applied,
    widthOf,
    maxOf: (side) => clampRail(Infinity, otherOf(side), view.room),
    clamp,
    commit: (side, px) => {
      const v = clamp(side, px)
      apply({ ...prefRef.current!, [side]: v })
    },
    reset: (side) => apply({ ...prefRef.current!, [side]: null }),
    resetAll: () => apply({ left: null, right: null }),
  }
}

/** Remove the window listeners a drag installed and the body state it set. */
function endDrag(listeners: Array<[string, (ev: PointerEvent) => void]>, bodyClass: string) {
  for (const [type, fn] of listeners) window.removeEventListener(type, fn as EventListener)
  document.body.classList.remove(bodyClass)
}

/** The handle on a rail's INNER edge that sets its width. */
export function RailWidthHandle({
  side,
  label,
  api,
  gridRef,
}: {
  side: RailSide
  label: string
  api: RailWidthsApi
  gridRef: RefObject<HTMLElement | null>
}) {
  const varName = side === 'left' ? '--cn-rail-l' : '--cn-rail-r'
  // The left rail's handle is on its right edge and the right rail's on its left edge, so the
  // arrow that WIDENS a rail is the one pointing away from it — the separator moves with the key.
  const grow = side === 'left' ? 'ArrowRight' : 'ArrowLeft'
  const shrink = side === 'left' ? 'ArrowLeft' : 'ArrowRight'

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? RAIL_STEP_BIG : RAIL_STEP
    const cur = api.widthOf(side)
    const next =
      e.key === grow ? cur + step : e.key === shrink ? cur - step : e.key === 'Home' ? RAIL_MIN : e.key === 'End' ? RAIL_MAX : null
    if (next === null) return
    e.preventDefault()
    api.commit(side, next)
  }

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    const grid = gridRef.current
    if (!grid || e.button !== 0) return
    e.preventDefault()
    try {
      e.currentTarget.setPointerCapture(e.pointerId)
    } catch {
      /* no active pointer to capture (synthetic event) — the window listeners still track it */
    }
    // Relative to where the drag STARTED, so grabbing the handle off-centre never jumps the rail.
    const z = elZoom(grid)
    const startX = e.clientX
    const startW = api.widthOf(side)
    let moved = false
    const pxFor = (ev: PointerEvent) =>
      api.clamp(side, startW + ((side === 'left' ? 1 : -1) * (ev.clientX - startX)) / z)
    const move = (ev: PointerEvent) => {
      moved = true
      grid.style.setProperty(varName, `${pxFor(ev)}px`)
    }
    const up = (ev: PointerEvent) => {
      endDrag(listeners, 'resizing')
      // A click without a move (half of a double-click) changes nothing and stores nothing.
      if (moved) api.commit(side, pxFor(ev))
    }
    const cancel = () => {
      endDrag(listeners, 'resizing')
      const a = api.applied[side]
      if (a == null) grid.style.removeProperty(varName)
      else grid.style.setProperty(varName, `${a}px`)
    }
    const listeners: Array<[string, (ev: PointerEvent) => void]> = [
      ['pointermove', move],
      ['pointerup', up],
      ['pointercancel', cancel],
    ]
    document.body.classList.add('resizing')
    for (const [type, fn] of listeners) window.addEventListener(type, fn as EventListener)
  }

  return (
    <div
      className="connect-sep"
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={Math.round(api.widthOf(side))}
      aria-valuemin={RAIL_MIN}
      aria-valuemax={Math.round(api.maxOf(side))}
      data-edge={side === 'left' ? 'end' : 'start'}
      title={t('connect.rail.handle.title', { label })}
      onKeyDown={onKeyDown}
      onPointerDown={onPointerDown}
      onDoubleClick={() => api.reset(side)}
    />
  )
}

/** The seam between a rail's two panes. `fraction` is the top pane's share of the rail. */
export function RailSplitHandle({
  label,
  fraction,
  onCommit,
}: {
  label: string
  fraction: number
  /** Persist the two shares (panelState.setShares, one undoable step). */
  onCommit: (above: number, below: number) => void
}) {
  const commitAt = (f: number) => {
    const [a, b] = seamShares(Math.min(1, Math.max(0, f)))
    onCommit(a, b)
  }

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? 0.15 : 0.05
    const f =
      e.key === 'ArrowDown' ? fraction + step : e.key === 'ArrowUp' ? fraction - step : e.key === 'Home' ? 0 : e.key === 'End' ? 1 : null
    if (f === null) return
    e.preventDefault()
    commitAt(f)
  }

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    const rail = e.currentTarget.parentElement
    if (!rail || e.button !== 0) return
    const frames = rail.querySelectorAll<HTMLElement>(':scope > .pane-frame')
    if (frames.length !== 2) return
    const rect = rail.getBoundingClientRect()
    const z = elZoom(rail)
    const gap = parseFloat(getComputedStyle(rail).rowGap)
    const g = Number.isFinite(gap) ? gap : 0
    const span = rect.height / z - g
    if (!(span > 0)) return // collapsed/hidden rail — never divide by it
    e.preventDefault()
    try {
      e.currentTarget.setPointerCapture(e.pointerId)
    } catch {
      /* synthetic event */
    }
    // The seam's centre sits at (span · f) + gap/2 from the rail's top.
    const fFor = (ev: PointerEvent) => ((ev.clientY - rect.top) / z - g / 2) / span
    const paint = (f: number) => {
      const [a, b] = seamShares(Math.min(1, Math.max(0, f)))
      frames[0].style.setProperty('--connect-share', String(a))
      frames[1].style.setProperty('--connect-share', String(b))
      rail.style.setProperty('--connect-split', String(a / (a + b)))
    }
    let moved = false
    const move = (ev: PointerEvent) => {
      moved = true
      paint(fFor(ev))
    }
    const up = (ev: PointerEvent) => {
      endDrag(listeners, 'resizing-row')
      if (moved) commitAt(fFor(ev))
    }
    const cancel = () => {
      endDrag(listeners, 'resizing-row')
      paint(fraction)
    }
    const listeners: Array<[string, (ev: PointerEvent) => void]> = [
      ['pointermove', move],
      ['pointerup', up],
      ['pointercancel', cancel],
    ]
    document.body.classList.add('resizing-row')
    for (const [type, fn] of listeners) window.addEventListener(type, fn as EventListener)
  }

  return (
    <div
      className="connect-sep"
      role="separator"
      tabIndex={0}
      aria-orientation="horizontal"
      aria-label={label}
      aria-valuenow={Math.round(fraction * 100)}
      aria-valuemin={Math.round((MIN_SHARE / 2) * 100)}
      aria-valuemax={Math.round((1 - MIN_SHARE / 2) * 100)}
      title={t('connect.rail.handle.title', { label })}
      onKeyDown={onKeyDown}
      onPointerDown={onPointerDown}
      onDoubleClick={() => onCommit(1, 1)}
    />
  )
}
