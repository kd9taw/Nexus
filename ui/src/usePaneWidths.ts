import { useCallback, useEffect, useRef, useState } from 'react'
import { surfaceGet, surfaceSet } from './features/windowScope'

// Pane width bounds (px). Defaults match the original fixed grid columns.
export const RIGHT_MIN = 260
export const RIGHT_DEFAULT = 360
export const LEFT_MIN = 220
export const LEFT_DEFAULT = 300
/** The conversation between the rails never gets less than this from them (layout L1): a phone's
 *  width, where its composer row and a message still read. Each rail used to be clamped against the
 *  window alone, so the two could add up to all of it: a 900 / 1400 px pair stored on a wide
 *  monitor and opened at 1366×768 left the conversation 0 px wide. */
export const CENTER_MIN = 360
/** Everything across the window at md and up that is neither rail nor conversation, in CSS px:
 *  the navigation rail (`.mode-nav`, 88 px with its border inside: the sheet sizes every box
 *  border-box), the three-pane layout's padding either side and the four gaps between its five
 *  tracks (`--gap`, 12 px; the two dividers' own tracks are 0 wide, their 8 px taken back by their
 *  margins). Measured in Chrome at 1366×768 and 1920×1080: the three panes share exactly
 *  effective width − 160. */
export const RAIL_CHROME = 88 + 2 * 12 + 4 * 12

const KEY_RIGHT = 'tempo-right-rail-w'
const KEY_LEFT = 'tempo-left-rail-w'
// Which rail the operator set LAST: the one that keeps its width when the pair must give.
const KEY_LAST = 'tempo-rail-last'

/** Effective (zoom-adjusted) content width in CSS px. The rails live inside the
 * zoomed `.app`, so their share of the screen must be measured against
 * `innerWidth / --ui-zoom`, not the raw window width — otherwise the drag ceiling
 * (and proportional defaults) are off by the zoom factor. */
function effWidth(): number {
  const raw = getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')
  const z = parseFloat(raw)
  const zoom = Number.isFinite(z) && z > 0 ? z : 1
  return window.innerWidth / zoom
}

export type RailSide = 'left' | 'right'
/** The stored widths (null = never set: the proportional default) and the rail set last. */
export interface RailPrefs {
  left: number | null
  right: number | null
  last?: RailSide
}

const MIN: Record<RailSide, number> = { left: LEFT_MIN, right: RIGHT_MIN }
const SHARE: Record<RailSide, number> = { left: 0.4, right: 0.6 }
const DEFAULT_SHARE: Record<RailSide, number> = { left: 0.18, right: 0.22 }
const otherSide = (side: RailSide): RailSide => (side === 'left' ? 'right' : 'left')

/** One rail on its own: ≥ its floor, ≤ its share of the effective width. */
function ownClamp(side: RailSide, px: number, ew: number): number {
  return Math.max(MIN[side], Math.min(Math.round(ew * SHARE[side]), px))
}

/** First-run / reset rail widths proportional to the screen (clamped), so a fresh
 * install on a 1366×768 laptop doesn't start with 4K-sized rails that starve the
 * center pane. */
function defaultRail(side: RailSide, ew: number): number {
  return ownClamp(side, Math.round(ew * DEFAULT_SHARE[side]), ew)
}

/** The width the two rails may share: the window less its chrome and the conversation's floor,
 *  rounded DOWN so the pair can never overrun it. */
export function railRoom(ew: number): number {
  return Math.floor(ew - RAIL_CHROME - CENTER_MIN)
}

/**
 * Fit the preferences into a window: load, every resize, every zoom change (the shape of
 * connectRails.fitRails). Each rail first gets its own limits; if the pair then fits the room,
 * that is what shows. If it does not:
 *   · the rail the operator set LAST (or the only one ever stored) keeps its width, down to
 *     leaving the other its floor, and the other takes the rest — so a reload after a move shows
 *     the moved rail exactly as moved;
 *   · otherwise both shrink in proportion, each held at its own floor;
 *   · and if even the two floors do not fit (a window below the supported minimum), both hold
 *     their floors and the conversation gives: every divider stays grabbable, nothing is trapped.
 * index.html's preseed carries a copy of this, held to it by index-preseed.test.ts.
 */
export function fitRails(pref: RailPrefs, ew: number): { left: number; right: number } {
  const want: Record<RailSide, number> = {
    left: ownClamp('left', pref.left ?? defaultRail('left', ew), ew),
    right: ownClamp('right', pref.right ?? defaultRail('right', ew), ew),
  }
  const room = railRoom(ew)
  if (want.left + want.right <= room) return want
  if (room < LEFT_MIN + RIGHT_MIN) return { left: LEFT_MIN, right: RIGHT_MIN }
  const anchor: RailSide | null =
    pref.last && pref[pref.last] != null
      ? pref.last
      : pref.left != null && pref.right == null
        ? 'left'
        : pref.right != null && pref.left == null
          ? 'right'
          : null
  if (anchor) {
    const o = otherSide(anchor)
    const a = Math.min(want[anchor], room - MIN[o])
    const b = Math.max(MIN[o], Math.min(want[o], room - a))
    return anchor === 'left' ? { left: a, right: b } : { left: b, right: a }
  }
  const k = room / (want.left + want.right)
  let left = Math.floor(want.left * k)
  let right = Math.floor(want.right * k)
  if (left < LEFT_MIN) {
    left = LEFT_MIN
    right = Math.min(want.right, room - LEFT_MIN)
  } else if (right < RIGHT_MIN) {
    right = RIGHT_MIN
    left = Math.min(want.left, room - RIGHT_MIN)
  }
  return { left, right }
}

/** The widest `side` may be in this window beside the other rail as it is shown: its own share
 *  of the window, or what the room leaves, whichever is less — never below its floor. */
export function railMax(side: RailSide, otherPx: number, ew: number): number {
  return Math.max(MIN[side], Math.min(Math.round(ew * SHARE[side]), railRoom(ew) - otherPx))
}

// PER-SURFACE: a width in px, clamped against THIS window's innerWidth. A narrow pop-out
// sharing the key would overwrite the main window's rails with its own ceiling.
function readWidth(key: string): number | null {
  const v = Number(surfaceGet(key))
  return Number.isFinite(v) && v > 0 ? v : null
}
function storeWidth(key: string, px: number | null): void {
  surfaceSet(key, String(px))
}
function loadPrefs(): RailPrefs {
  const pref: RailPrefs = { left: readWidth(KEY_LEFT), right: readWidth(KEY_RIGHT) }
  const last = surfaceGet(KEY_LAST)
  if (last === 'left' || last === 'right') pref.last = last
  return pref
}

/**
 * Persisted, drag-resizable pane widths, applied as the `--left-rail-w` /
 * `--right-rail-w` CSS custom properties on <html> (mirroring the theme hook).
 * The divider's drag writes the CSS var directly for 60 fps; `commit*` clamps +
 * persists + syncs React state once, on pointer-up (and on each key step).
 *
 * Pass the current UI `scale` (like useViewport) so the fit re-runs when the zoom
 * changes — every limit is defined against effWidth = innerWidth / zoom, so both a
 * resize and a zoom change move them.
 */
export function usePaneWidths(scale?: number) {
  // The operator's PREFERRED widths as stored — never mutated by a re-fit, so a rail
  // sized for the big monitor comes back when the room does. Published state below is
  // the FITTED view of it for THIS window box.
  const prefRef = useRef<RailPrefs | null>(null)
  if (prefRef.current === null) prefRef.current = loadPrefs()
  const [view, setView] = useState(() => {
    const ew = effWidth()
    return { ew, ...fitRails(prefRef.current!, ew) }
  })
  const viewRef = useRef(view)
  viewRef.current = view

  // Publish ONLY. These effects used to also persist, which re-anchored whatever was
  // published on every mount — a stale over-wide value never self-healed, and clamping
  // here would clobber the big-monitor preference. Storage writes now happen solely in
  // commit*/reset* (explicit operator actions).
  useEffect(() => {
    document.documentElement.style.setProperty('--right-rail-w', `${view.right}px`)
  }, [view.right])
  useEffect(() => {
    document.documentElement.style.setProperty('--left-rail-w', `${view.left}px`)
  }, [view.left])

  // Re-fit on resize and on zoom change. rAF-debounced, mirroring useViewport — and
  // deferred a frame for the same reason: a just-changed --ui-zoom must be committed
  // to <html> before effWidth() reads it.
  useEffect(() => {
    let raf = 0
    const apply = () => {
      const ew = effWidth()
      const fit = fitRails(prefRef.current!, ew)
      setView((v) => (v.ew === ew && v.left === fit.left && v.right === fit.right ? v : { ew, ...fit }))
    }
    const onResize = () => {
      cancelAnimationFrame(raf)
      raf = requestAnimationFrame(apply)
    }
    raf = requestAnimationFrame(apply)
    window.addEventListener('resize', onResize)
    return () => {
      window.removeEventListener('resize', onResize)
      cancelAnimationFrame(raf)
    }
  }, [scale])

  /** Show and store one rail, leaving the other exactly as it is shown. */
  const place = useCallback((side: RailSide, px: number, pref: RailPrefs) => {
    prefRef.current = pref
    storeWidth(side === 'left' ? KEY_LEFT : KEY_RIGHT, pref[side])
    const next = { ...viewRef.current, ew: effWidth(), [side]: px }
    viewRef.current = next
    setView(next)
  }, [])

  /** A ONE-RAIL move (a key's step, a drag's release): clamped against its own limits and the
   *  room the other rail leaves as it is shown. It becomes the rail set last. */
  const commit = useCallback(
    (side: RailSide, px: number) => {
      const w = Math.max(MIN[side], Math.min(railMax(side, viewRef.current[otherSide(side)], effWidth()), px))
      place(side, w, { ...prefRef.current!, [side]: w, last: side })
      surfaceSet(KEY_LAST, side)
    },
    [place],
  )
  /** One rail back to its default (its divider's double-click / Backspace), as far as the room
   *  beside the other rail allows; the other stays. It is no longer the rail set last. */
  const reset = useCallback(
    (side: RailSide) => {
      const ew = effWidth()
      const d = defaultRail(side, ew)
      const w = Math.max(MIN[side], Math.min(railMax(side, viewRef.current[otherSide(side)], ew), d))
      const { last, ...pref } = prefRef.current!
      if (last === side) surfaceSet(KEY_LAST, '')
      place(side, w, { ...pref, ...(last && last !== side ? { last } : {}), [side]: d })
    },
    [place],
  )
  const commitRight = useCallback((px: number) => commit('right', px), [commit])
  const commitLeft = useCallback((px: number) => commit('left', px), [commit])
  const resetRight = useCallback(() => reset('right'), [reset])
  const resetLeft = useCallback(() => reset('left'), [reset])
  // Both (Reset layout): the defaults, which always fit together, and no rail set last.
  const resetWidths = useCallback(() => {
    const ew = effWidth()
    const pref: RailPrefs = { left: defaultRail('left', ew), right: defaultRail('right', ew) }
    if (prefRef.current!.last) surfaceSet(KEY_LAST, '')
    prefRef.current = pref
    storeWidth(KEY_LEFT, pref.left)
    storeWidth(KEY_RIGHT, pref.right)
    const next = { ew, ...fitRails(pref, ew) }
    viewRef.current = next
    setView(next)
  }, [])

  return {
    rightW: view.right,
    leftW: view.left,
    rightMax: railMax('right', view.left, view.ew),
    leftMax: railMax('left', view.right, view.ew),
    commitRight,
    commitLeft,
    resetRight,
    resetLeft,
    resetWidths,
  }
}
