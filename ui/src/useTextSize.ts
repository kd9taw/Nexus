import { useCallback, useEffect, useState } from 'react'

/**
 * TEXT SIZE (#215) — how big the words are: Normal / Large / Larger, +0 / +12 / +25 %
 * (operator, 2026-09-26), on all text, the decode and log lists included.
 *
 * Distinct from the two axes beside it: UI scale (`useScale`, `--ui-zoom`) magnifies the
 * WHOLE interface, so a bigger scale fits less on the screen; density (`useDensity`) is how
 * tightly rows pack. This one grows the text and leaves the rest at its size. Applied as the
 * `data-text-size` attribute on `<html>`; styles.css turns it into `--text-scale`, which every
 * font size in the sheet multiplies (styles-text-size.test.ts holds that to the picks).
 *
 * Per machine and not in the durable store, the same classification as density: a fact about
 * this screen and the eyes in front of it. index.html's preseed carries the first-paint copy of
 * this reading and index-preseed.test.ts holds the two to parity — a mismatch would paint the
 * whole app at one size and then jump to another.
 */
export type TextSize = 'normal' | 'large' | 'larger'

export const TEXT_SIZE_STORAGE_KEY = 'nexus-text-size'

/** A pixel font size that follows Text size, for the few components that size text inline —
 *  the stylesheet writes the same `calc(<n>px * var(--text-scale))`. At Normal it is `<n>px`. */
export function textPx(px: number): string {
  return `calc(${px}px * var(--text-scale))`
}

function readInitial(): TextSize {
  try {
    const saved = localStorage.getItem(TEXT_SIZE_STORAGE_KEY)
    return saved === 'large' || saved === 'larger' ? saved : 'normal'
  } catch {
    return 'normal'
  }
}

export function useTextSize(): [TextSize, (s: TextSize) => void] {
  const [size, setSizeState] = useState<TextSize>(readInitial)

  useEffect(() => {
    document.documentElement.setAttribute('data-text-size', size)
    try {
      localStorage.setItem(TEXT_SIZE_STORAGE_KEY, size)
    } catch {
      /* storage unavailable — the attribute still applies for this session */
    }
  }, [size])

  const setSize = useCallback((s: TextSize) => setSizeState(s), [])
  return [size, setSize]
}
