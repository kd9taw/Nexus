import { useCallback, useEffect, useState } from 'react'

/**
 * Information **density** (row heights / padding compactness) — distinct from
 * `useScale` (`--ui-zoom`, whole-UI magnification). Applied as the `data-density`
 * attribute on `<html>`; CSS maps it to `--density-scale`. The two compose.
 *
 * - `guided`   — roomy (Settings calls it Comfortable)
 * - `standard` — the default
 * - `dense`    — compact, for contest / DX-chase power use
 * - `touch`    — Comfortable plus finger-sized targets. It writes `data-density='guided'`
 *                (so every Comfortable rule applies unchanged) AND `data-touch`, which
 *                styles.css turns into `--touch` 48px and bigger chips. An explicit
 *                attribute, never `@media (pointer: coarse)`. index.html's preseed carries
 *                the first-paint copy of this mapping (index-preseed.test.ts).
 */
export type Density = 'guided' | 'standard' | 'dense' | 'touch'
export const DENSITY_STEPS: Density[] = ['guided', 'standard', 'dense', 'touch']

const STORAGE_KEY = 'nexus-density'

function readInitial(): Density {
  const saved = localStorage.getItem(STORAGE_KEY)
  return saved === 'guided' || saved === 'standard' || saved === 'dense' || saved === 'touch'
    ? saved
    : 'standard'
}

export function useDensity(): [Density, (d: Density) => void] {
  const [density, setDensityState] = useState<Density>(readInitial)

  useEffect(() => {
    const d = document.documentElement
    d.setAttribute('data-density', density === 'touch' ? 'guided' : density)
    if (density === 'touch') d.setAttribute('data-touch', '1')
    else d.removeAttribute('data-touch')
    localStorage.setItem(STORAGE_KEY, density)
  }, [density])

  const setDensity = useCallback((d: Density) => setDensityState(d), [])
  return [density, setDensity]
}
