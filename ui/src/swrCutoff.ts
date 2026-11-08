import { useEffect, useState } from 'react'
import type { Settings } from './types'

// #476 — THE HIGH-SWR CUTOFF THE TX METERS' TOOLTIP NAMES. The meters render in eight cockpits, a
// pop-out and Settings, none of which is handed the settings object, so the two fields are
// mirrored here the way the units setting is (units.ts): App writes the mirror whenever its
// settings copy changes, the meters read it synchronously through `useSwrCutoff()`, and every
// window sees the value through the `storage` event.
//
// DISPLAY ONLY. The engine's cutoff reads `Settings::swr_stop_ratio` and nothing here; this copy
// can only change a tooltip. Whether the rig's scale is verified (the other half of "is the
// cutoff in force") is the meter's own `radio.swrScaleVerified`, not stored here.
//
// CLAMPED ON LOAD to the engine's 1-99, so a hand-edited value reads the way the cutoff reads it.

const KEY = 'nexus-swr-cutoff'
const CHANGED = 'nexus-swr-cutoff-changed'
/** `default_swr_stop_threshold` in settings.rs: what the engine uses for a non-finite value. */
const DEFAULT_THRESHOLD = 2.5

/** The stored cutoff ratio, or null when the operator has the cutoff switched off. */
export function readSwrCutoff(): number | null {
  let raw: string | null
  try {
    raw = localStorage.getItem(KEY)
  } catch {
    return null
  }
  if (raw == null || raw === '') return null
  const n = Number(raw)
  if (!Number.isFinite(n)) return null
  return Math.min(99, Math.max(1, n))
}

/** Mirror the two settings fields; tells this window's meters too (`storage` reaches only others). */
export function setSwrCutoffMirror(
  s: Pick<Settings, 'swrStopEnabled' | 'swrStopThreshold'> | null | undefined,
): void {
  const t = s?.swrStopThreshold
  const v = s?.swrStopEnabled === true ? String(Number.isFinite(t) ? t : DEFAULT_THRESHOLD) : ''
  try {
    if (localStorage.getItem(KEY) === v) return
    localStorage.setItem(KEY, v)
  } catch {
    return
  }
  window.dispatchEvent(new Event(CHANGED))
}

/** The cutoff ratio, re-rendering when it changes here or in another window. */
export function useSwrCutoff(): number | null {
  const [v, setV] = useState<number | null>(readSwrCutoff)
  useEffect(() => {
    const on = () => setV(readSwrCutoff())
    window.addEventListener('storage', on)
    window.addEventListener(CHANGED, on)
    return () => {
      window.removeEventListener('storage', on)
      window.removeEventListener(CHANGED, on)
    }
  }, [])
  return v
}
