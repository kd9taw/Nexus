// A drag handle that resizes a sub-panel inside a section — the generalization of the
// workspace rail resizer (App.tsx startResize / usePaneWidths): pointer-events (mouse/
// touch/pen), writes a CSS variable LIVE during the drag (no React re-render), commits
// to localStorage on release. Two deliberate differences from the rail resizer:
// - the variable is written to a TARGET CONTAINER element, not the document root, so a
//   splitter in a detached window (or the kept-alive Operate host) resizes its own
//   panel and never a twin in another window;
// - the persisted value is a PERCENTAGE of the container, so it survives window
//   resizes. NB only the RATIO is zoom-invariant: this header used to claim the whole
//   scheme "needs no correction", but minPx/maxPx mean CSS px while pointer positions
//   and getBoundingClientRect are VISUAL px (zoom-multiplied) — so "max 420px" silently
//   meant 646 CSS px at zoom 65. Every measurement is divided by the current zoom
//   before clamping, and the stored % is re-clamped against the CURRENT box at mount
//   (it may come from another window size, another zoom, or an inherited surface).
import { useEffect } from 'react'
import { surfaceGet, surfaceSet } from '../features/windowScope'

interface Props {
  /** 'y' = the handle drags a HEIGHT (row-resize); 'x' drags a width. */
  axis: 'x' | 'y'
  /** CSS variable the drag drives, e.g. "--cockpit-wf-h". */
  varName: string
  /** The container the variable is scoped to and measured against. */
  target: React.RefObject<HTMLElement | null>
  /** localStorage key (nexus.split.<section>.<id>). PER-SURFACE — scoped here rather
   *  than at the three call sites, so a split fraction can never be shared between a
   *  window and a pop-out with a different aspect. */
  storageKey: string
  /** Pixel clamps for the panel being sized. */
  minPx: number
  maxPx: number
  /** Default size as a percentage of the container (used until first drag). */
  defaultPct: number
  /** Accessible label for the separator. */
  label: string
}

/** Load a persisted split percentage (NaN-safe; null = never customized). */
function loadPct(storageKey: string): number | null {
  const v = parseFloat(surfaceGet(storageKey) ?? '')
  return Number.isFinite(v) && v > 0 && v < 100 ? v : null
}

/** Effective zoom on `el`: `currentCSSZoom` where the engine provides it (Chromium
 *  126+), else the `--ui-zoom` var the app publishes on <html>; 1 when neither reads. */
function elZoom(el: HTMLElement): number {
  const z = (el as HTMLElement & { currentCSSZoom?: number }).currentCSSZoom
  if (typeof z === 'number' && Number.isFinite(z) && z > 0) return z
  const raw = getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')
  const p = parseFloat(raw)
  return Number.isFinite(p) && p > 0 ? p : 1
}

/** Clamp a split percentage against a container span, ALL in CSS px: the panel stays
 *  within [minPx, min(maxPx, 90% of span)] — the drag's own bounds — so a stored %
 *  from another geometry re-enters range at apply time. span ≤ 0 (hidden container)
 *  returns the input unchanged. Pure + testable. */
export function clampSplitPct(pct: number, spanCss: number, minPx: number, maxPx: number): number {
  if (!(spanCss > 0)) return pct
  const px = (pct / 100) * spanCss
  const clamped = Math.min(Math.min(maxPx, spanCss * 0.9), Math.max(minPx, px))
  return (clamped / spanCss) * 100
}

export function Splitter({ axis, varName, target, storageKey, minPx, maxPx, defaultPct, label }: Props) {
  // Apply the persisted (or default) size once the target exists — CLAMPED against the
  // CURRENT container box. The mount used to replay the stored % raw, which is how a
  // scope dragged to max at one geometry reopened past its own drag cap at another
  // (the stored value itself is left alone — it re-applies where it is legal again).
  useEffect(() => {
    const el = target.current
    if (!el) return
    const rect = el.getBoundingClientRect()
    const span = (axis === 'y' ? rect.height : rect.width) / elZoom(el)
    const pct = clampSplitPct(loadPct(storageKey) ?? defaultPct, span, minPx, maxPx)
    el.style.setProperty(varName, `${pct}%`)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const start = (e: React.PointerEvent<HTMLDivElement>) => {
    const el = target.current
    if (!el) return
    e.preventDefault()
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
    document.body.classList.add('resizing')
    const rect = el.getBoundingClientRect()
    // Rect + clientX/Y are VISUAL px; minPx/maxPx are CSS px — divide the measurements
    // by the zoom so the clamps mean CSS px at every scale step (see header note).
    const z = elZoom(el)
    const span = (axis === 'y' ? rect.height : rect.width) / z
    if (span <= 0) return // hidden/zero-size container — never divide by it
    const pctFor = (ev: PointerEvent) => {
      const px = (axis === 'y' ? ev.clientY - rect.top : ev.clientX - rect.left) / z
      return clampSplitPct((px / span) * 100, span, minPx, maxPx)
    }
    const move = (ev: PointerEvent) => {
      el.style.setProperty(varName, `${pctFor(ev)}%`)
    }
    const up = (ev: PointerEvent) => {
      const pct = pctFor(ev)
      el.style.setProperty(varName, `${pct}%`)
      surfaceSet(storageKey, String(pct))
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      document.body.classList.remove('resizing')
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  return (
    <div
      className={`pane-splitter ${axis === 'y' ? 'horizontal' : 'vertical-inline'}`}
      role="separator"
      aria-orientation={axis === 'y' ? 'horizontal' : 'vertical'}
      aria-label={label}
      title={`Drag to resize (${label})`}
      onPointerDown={start}
    />
  )
}
