import { useCallback, useEffect, useState } from 'react'

/** Global UI scale (percent). Applied as the `--ui-zoom` factor on <html>; CSS
 * `.app { zoom: var(--ui-zoom) }` scales the whole interface crisply. */
export type Scale = 90 | 100 | 110 | 125
export const SCALE_STEPS: Scale[] = [90, 100, 110, 125]

const STORAGE_KEY = 'tempo-ui-scale'

function readInitial(): Scale {
  const saved = Number(localStorage.getItem(STORAGE_KEY))
  // 125% is the default base level — far more readable than 100% on typical shack
  // displays. A saved preference (any of SCALE_STEPS) still wins.
  return (SCALE_STEPS as number[]).includes(saved) ? (saved as Scale) : 125
}

export function useScale(): [Scale, (s: Scale) => void] {
  const [scale, setScaleState] = useState<Scale>(readInitial)

  useEffect(() => {
    document.documentElement.style.setProperty('--ui-zoom', String(scale / 100))
    localStorage.setItem(STORAGE_KEY, String(scale))
  }, [scale])

  const setScale = useCallback((s: Scale) => setScaleState(s), [])
  return [scale, setScale]
}
