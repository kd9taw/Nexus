import { useCallback, useEffect, useRef, useState } from 'react'
import { surfaceGet, surfaceSet } from './features/windowScope'

// Pane width bounds (px). Defaults match the original fixed grid columns.
export const RIGHT_MIN = 260
export const RIGHT_DEFAULT = 360
export const LEFT_MIN = 220
export const LEFT_DEFAULT = 300

const KEY_RIGHT = 'tempo-right-rail-w'
const KEY_LEFT = 'tempo-left-rail-w'

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

/** The widest each rail may be in this window — the ceilings its divider announces. */
function rightMax(): number {
  return Math.round(effWidth() * 0.6)
}
function leftMax(): number {
  return Math.round(effWidth() * 0.4)
}

/** Clamp the right (waterfall) rail width: ≥ RIGHT_MIN, ≤ 60% of the effective width. */
export function clampRight(px: number): number {
  return Math.max(RIGHT_MIN, Math.min(rightMax(), px))
}
/** Clamp the left (stations) rail width: ≥ LEFT_MIN, ≤ 40% of the effective width. */
export function clampLeft(px: number): number {
  return Math.max(LEFT_MIN, Math.min(leftMax(), px))
}

/** First-run / reset rail widths proportional to the screen (clamped), so a fresh
 * install on a 1366×768 laptop doesn't start with 4K-sized rails that starve the
 * center pane. */
function defaultLeft(): number {
  return clampLeft(Math.round(effWidth() * 0.18))
}
function defaultRight(): number {
  return clampRight(Math.round(effWidth() * 0.22))
}

// PER-SURFACE: a width in px, clamped against THIS window's innerWidth. A narrow pop-out
// sharing the key would overwrite the main window's rails with its own ceiling.
function readNum(key: string, fallback: () => number): number {
  const v = Number(surfaceGet(key))
  return Number.isFinite(v) && v > 0 ? v : fallback()
}

/**
 * Persisted, drag-resizable pane widths, applied as the `--left-rail-w` /
 * `--right-rail-w` CSS custom properties on <html> (mirroring the theme hook).
 * The splitter drag writes the CSS var directly for 60 fps; `commit*` clamps +
 * persists + syncs React state once, on pointer-up.
 *
 * Pass the current UI `scale` (like useViewport) so the clamps re-run when the zoom
 * changes — clampLeft/clampRight ceilings are defined against effWidth = innerWidth
 * / zoom, so both a resize and a zoom change move them.
 */
export function usePaneWidths(scale?: number) {
  // The operator's PREFERRED widths as stored — never mutated by a re-clamp, so a rail
  // sized for the big monitor comes back when the room does. Published state below is
  // the CLAMPED view of it for THIS window box.
  const prefRef = useRef<{ right: number; left: number } | null>(null)
  if (prefRef.current === null) {
    prefRef.current = {
      right: readNum(KEY_RIGHT, defaultRight),
      left: readNum(KEY_LEFT, defaultLeft),
    }
  }
  const [rightW, setRightW] = useState(() => clampRight(prefRef.current!.right))
  const [leftW, setLeftW] = useState(() => clampLeft(prefRef.current!.left))
  // The ceilings, re-read at the same moments as the clamps above (they move together).
  const [maxW, setMaxW] = useState(() => ({ right: rightMax(), left: leftMax() }))

  // Publish ONLY. These effects used to also persist, which re-anchored whatever was
  // published on every mount — a stale over-wide value never self-healed, and clamping
  // here would clobber the big-monitor preference. Storage writes now happen solely in
  // commit*/resetWidths (explicit operator actions).
  useEffect(() => {
    document.documentElement.style.setProperty('--right-rail-w', `${rightW}px`)
  }, [rightW])
  useEffect(() => {
    document.documentElement.style.setProperty('--left-rail-w', `${leftW}px`)
  }, [leftW])

  // Re-clamp on resize and on zoom change. rAF-debounced, mirroring useViewport — and
  // deferred a frame for the same reason: a just-changed --ui-zoom must be committed
  // to <html> before effWidth() reads it.
  useEffect(() => {
    let raf = 0
    const apply = () => {
      setRightW(clampRight(prefRef.current!.right))
      setLeftW(clampLeft(prefRef.current!.left))
      const right = rightMax()
      const left = leftMax()
      setMaxW((m) => (m.right === right && m.left === left ? m : { right, left }))
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

  const commitRight = useCallback((px: number) => {
    const w = clampRight(px)
    prefRef.current!.right = w
    surfaceSet(KEY_RIGHT, String(w))
    setRightW(w)
  }, [])
  const commitLeft = useCallback((px: number) => {
    const w = clampLeft(px)
    prefRef.current!.left = w
    surfaceSet(KEY_LEFT, String(w))
    setLeftW(w)
  }, [])
  // One rail back to its default (its divider's double-click / Backspace); the other rail stays.
  const resetRight = useCallback(() => {
    const r = defaultRight()
    prefRef.current!.right = r
    surfaceSet(KEY_RIGHT, String(r))
    setRightW(r)
  }, [])
  const resetLeft = useCallback(() => {
    const l = defaultLeft()
    prefRef.current!.left = l
    surfaceSet(KEY_LEFT, String(l))
    setLeftW(l)
  }, [])
  // Both (Reset layout).
  const resetWidths = useCallback(() => {
    resetRight()
    resetLeft()
  }, [resetRight, resetLeft])

  return {
    rightW,
    leftW,
    rightMax: maxW.right,
    leftMax: maxW.left,
    commitRight,
    commitLeft,
    resetRight,
    resetLeft,
    resetWidths,
  }
}
