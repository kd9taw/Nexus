import { useCallback, useEffect, useState } from 'react'

// Pane width bounds (px). Defaults match the original fixed grid columns.
export const RIGHT_MIN = 260
export const RIGHT_DEFAULT = 360
export const LEFT_MIN = 220
export const LEFT_DEFAULT = 300

const KEY_RIGHT = 'tempo-right-rail-w'
const KEY_LEFT = 'tempo-left-rail-w'

/** Clamp the right (waterfall) rail width: ≥ RIGHT_MIN, ≤ 60% of the window. */
export function clampRight(px: number): number {
  const max = Math.round(window.innerWidth * 0.6)
  return Math.max(RIGHT_MIN, Math.min(max, px))
}
/** Clamp the left (stations) rail width: ≥ LEFT_MIN, ≤ 40% of the window. */
export function clampLeft(px: number): number {
  const max = Math.round(window.innerWidth * 0.4)
  return Math.max(LEFT_MIN, Math.min(max, px))
}

function readNum(key: string, fallback: number): number {
  const v = Number(localStorage.getItem(key))
  return Number.isFinite(v) && v > 0 ? v : fallback
}

/**
 * Persisted, drag-resizable pane widths, applied as the `--left-rail-w` /
 * `--right-rail-w` CSS custom properties on <html> (mirroring the theme hook).
 * The splitter drag writes the CSS var directly for 60 fps; `commit*` clamps +
 * persists + syncs React state once, on pointer-up.
 */
export function usePaneWidths() {
  const [rightW, setRightW] = useState(() => readNum(KEY_RIGHT, RIGHT_DEFAULT))
  const [leftW, setLeftW] = useState(() => readNum(KEY_LEFT, LEFT_DEFAULT))

  useEffect(() => {
    document.documentElement.style.setProperty('--right-rail-w', `${rightW}px`)
    localStorage.setItem(KEY_RIGHT, String(rightW))
  }, [rightW])
  useEffect(() => {
    document.documentElement.style.setProperty('--left-rail-w', `${leftW}px`)
    localStorage.setItem(KEY_LEFT, String(leftW))
  }, [leftW])

  const commitRight = useCallback((px: number) => setRightW(clampRight(px)), [])
  const commitLeft = useCallback((px: number) => setLeftW(clampLeft(px)), [])
  const resetWidths = useCallback(() => {
    setRightW(RIGHT_DEFAULT)
    setLeftW(LEFT_DEFAULT)
  }, [])

  return { rightW, leftW, commitRight, commitLeft, resetWidths }
}
