import { useEffect, useRef } from 'react'
import { getDisplayMetrics, isTauri } from './api'
import { surfaceId } from './features/windowScope'
import { capNeverChosen, dpiSeedCap, type Scale } from './useScale'

/**
 * FIRST LAUNCH ONLY: give a high-density display an auto-fit ceiling it can actually use.
 *
 * The problem this closes is Linux-shaped. `DEFAULT_CAP` is 100, so auto-fit can shrink the
 * UI but never grow it, and on X11 nothing else grows it either — the scale factor is 1
 * whatever the panel is, and GNOME's text-scaling-factor was measured not to reach web
 * content at all. A 4K panel, or a small sharp laptop screen, therefore renders Nexus at 1:1
 * physical pixels and reads half the size the same build does on Windows. The arithmetic that
 * fixes it lives in `dpiSeedCap`; this hook is only the wiring, and every line of it is a
 * guard:
 *
 *  - MAIN SURFACE ONLY. The cap is app-global; a pop-out has no business writing it.
 *  - ONCE PER INSTALL, gated on the ABSENCE of the key (`capNeverChosen`). The moment the
 *    seed writes, the gate is shut for good — an upgrade cannot re-seed, and a scale the
 *    operator chose is never moved under them. That is the difference between a good default
 *    and a change that arrives with an update.
 *  - RAISE ONLY. `dpiSeedCap` returns nothing at or below the current default, so nobody's
 *    UI is ever made smaller by it. Shrinking is auto-fit's job.
 *  - OVERRIDABLE AND STICKY. It writes the same key Settings ▸ Appearance writes, so the
 *    operator can move it afterwards and their value wins permanently, by the same gate.
 *  - SILENT ON FAILURE. No bridge, no answer, a bad answer: change nothing.
 *
 * On a 96-dpi display — every ordinary desktop monitor — the probe comes back near the
 * reference, `dpiSeedCap` returns null and NOTHING HAPPENS. That is also what happens on
 * Windows and macOS, where the OS already sized the CSS pixel correctly and the backend
 * returns no dpi at all.
 */
export function useDpiScaleSeed(setCap: (c: Scale) => void): void {
  // One attempt per window, independent of render count: `capNeverChosen` only flips after
  // the write lands, so a re-render in between must not queue a second probe.
  const tried = useRef(false)

  useEffect(() => {
    if (tried.current) return
    if (surfaceId() !== 'main') return
    if (!capNeverChosen()) return
    if (!isTauri()) return
    tried.current = true

    let cancelled = false
    getDisplayMetrics()
      .then((m) => {
        if (cancelled) return
        // The page's OWN devicePixelRatio is the right divisor — it is what actually sized
        // the CSS pixel. The window's scale factor is the fallback for the theoretical case
        // of a webview that reports neither.
        const dpr = window.devicePixelRatio || m.scaleFactor || 1
        const seed = dpiSeedCap(m.physicalDpi, dpr)
        // Re-check the gate: an operator who opened Settings and set a cap while the probe
        // was in flight has answered the question, and their answer wins.
        if (seed != null && capNeverChosen()) setCap(seed)
      })
      .catch(() => {
        /* no metrics, no change — the default cap is a perfectly good answer */
      })
    return () => {
      cancelled = true
    }
  }, [setCap])
}
