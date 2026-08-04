import { useLayoutEffect, useRef, useState } from 'react'

// COLUMN TIER for a cockpit pane region (2026-07-30 layout assessment, design3 §3).
//
// Why the REGION and not the window: the tier must react to the width the panes actually
// get, which a scope-splitter drag, a popped-out panel or (later) a rail changes with the
// window untouched. `data-viewport` (useViewport.ts) keys off the effective WINDOW width
// and cannot see any of that, so the two vocabularies are deliberately separate: viewport
// classes drive app chrome, `data-cols` drives one pane region.
//
// Why an observed attribute and not `@container`: container queries would measure this for
// free, but they are unverified on WebKitGTK (a shipped deb target), historically buggy
// with CSS `zoom` in Chromium (this app zooms `.app`), and untestable in vitest/jsdom.
// A ResizeObserver-stamped attribute is deterministic, testable, and already proven in
// Operate (`.cockpit-lower[data-cols]`). Container queries stay the documented later
// simplification, once verified live on both engines.
//
// Measured with `clientWidth`, NOT getBoundingClientRect(): under `zoom` a client rect is
// engine-inconsistent (visual vs layout px), while clientWidth is the element's own
// layout box in its own CSS px — the same space as `--vw-eff` and the same space the
// grid templates are written in. So OS display scaling and UI zoom are invisible here by
// construction; the thresholds mean what they say.

/** The three pane-region tiers. Also the literal `data-cols` value. */
export type RegionCols = 1 | 2 | 3

/**
 * Column tier for a pane region `width` CSS px wide. Pure + total, so the thresholds are
 * unit-testable without layout.
 *
 * The boundaries come from the assessment's scenario table: a 1200×750 window at the
 * fitter's 80% lands the region at ~1470 (2-col, the default-window case), a 1366×768
 * laptop at 85% lands ~1570 (2-col — deliberately the SAME layout), and 3440 fullscreen
 * lands ~3390 (3-col). Below ~1080 the 2-col split (a 40%-bounded log column beside the
 * feed — cockpit-panes.css) leaves the log under ~432px and the feed under ~640px,
 * neither comfortably usable, so the region becomes a single scrolling stack instead.
 */
export function classifyRegionCols(width: number): RegionCols {
  if (width < 1080) return 1
  if (width < 1700) return 2
  return 3
}

/**
 * Observe a pane region and keep `data-cols` + `data-flow` on it in sync with its own width.
 *
 * TWO ATTRIBUTES, BECAUSE THERE ARE TWO FACTS, and conflating them was a shipped bug:
 *
 *   `data-cols` is a TRACK COUNT — min(measured tier, `maxCols`). It drives
 *     `grid-template-columns` and nothing else, and it is a CONTENT budget, so it can be 1
 *     on a 3440-wide window whenever the ⊞ Panels menu empties the other columns.
 *   `data-flow` is the WIDTH CLAIM — 'stack' only when the region MEASURED narrow, 'fill'
 *     otherwise. Everything the sheet does *because the region is narrow* hangs off this:
 *     content-height rows, the region owning the scrollbar, `--cockpit-pane-flex: 0 0 auto`
 *     collapsing fill panes to content height.
 *
 * They used to be one attribute, and the tier-1 rule justified itself as "the region is
 * content-height and scrolls" — i.e. tier 1 == narrow, which is NOT what this function
 * computes. Untick CW's decode/sent/aux on an ultrawide and `maxCols` drove the region to
 * tier 1: the log pane went content-height and the whole surplus was left blank. So the
 * cap now narrows the TEMPLATE and never the flow. ('stack' implies the measurement was
 * tier 1, so cols = min(1, maxCols) = 1 — there is no wide 'stack' state.)
 *
 * The hook OWNS both attributes — it writes them imperatively so the first paint is already
 * correct (useLayoutEffect runs before paint) and so no consumer can stamp a state that
 * disagrees with the measurement. Consumers must NOT render either themselves; they use the
 * returned `cols` for the parts of the layout that live in TSX (how many `.cockpit-col`
 * groups to render).
 *
 * `maxCols` is how many column groups the cockpit CAN fill right now — Phone with no rig
 * connected (or with the rig panes hidden from the ⊞ Panels menu) has nothing to put in the
 * aux column, and a 3-track template with an empty middle is the "band of empty black"
 * complaint rebuilt. Capping here rather than in CSS keeps the invariant the grid depends
 * on: the number of `.cockpit-col` groups never exceeds `cols`. Same collapse Operate
 * ships as `dataCols: 'one' | 'two'` (features/panelHost.ts).
 *
 * Resize bursts are collapsed to one measurement per frame; a hidden or mid-layout region
 * (0×0 — the keep-alive host, a view switch) keeps the tier it last had, so being hidden
 * never costs a single-column flash on the way back.
 */
export function useRegionCols<T extends HTMLElement>(
  maxCols: RegionCols = 3,
): {
  ref: React.RefObject<T>
  cols: RegionCols
} {
  const ref = useRef<T>(null)
  const [cols, setCols] = useState<RegionCols>(1)
  // The MEASURED tier, kept across effect re-runs (a maxCols change must not forget the
  // last measurement and re-stamp 1 while the region happens to be hidden).
  const measured = useRef<RegionCols>(1)

  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    let raf = 0

    const measure = () => {
      const w = el.clientWidth
      // 0×0 = hidden or mid-layout, not "narrow". Keep the last real tier (the guard
      // idiom in PhoneScope.tsx's resize()).
      if (w >= 2) measured.current = classifyRegionCols(w)
      const next = Math.min(measured.current, maxCols) as RegionCols
      el.setAttribute('data-cols', String(next))
      // The MEASUREMENT, uncapped: `maxCols` may narrow the template, never the flow.
      el.setAttribute('data-flow', measured.current === 1 ? 'stack' : 'fill')
      setCols(next) // same value ⇒ React bails out, no re-render
    }

    measure()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => {
      cancelAnimationFrame(raf)
      raf = requestAnimationFrame(measure)
    })
    ro.observe(el)
    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
    }
  }, [maxCols])

  return { ref, cols }
}
