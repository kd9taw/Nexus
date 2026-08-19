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
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The only prose it
// owns is the handle's tooltip; the separator's accessible name is the caller's `label`.
import { useEffect } from 'react'
import { t } from '../i18n'
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
  /** Clamps for the panel being sized — the ends of the drag. See SplitClamp: a bare
   *  number is CSS px, a function is resolved against the live geometry so a clamp the
   *  SHEET writes in em / --vh-eff can be declared here in the same terms. */
  min: SplitClamp
  max: SplitClamp
  /** Default size as a percentage of the container (used until first drag). */
  defaultPct: number
  /** Accessible label for the separator. */
  label: string
}

/** The geometry a clamp may be expressed against. `fontPx` is the container's computed
 *  font size (what an `em` in the sheet resolves to) and `vhEff` is `--vh-eff` in CSS px
 *  (what the sheet's `calc(f * var(--vh-eff))` caps resolve to). */
export interface SplitGeom {
  fontPx: number
  vhEff: number
}

/** A drag end: CSS px, or a function of the live geometry.
 *
 *  The function form exists because the CLAMPS THAT ACTUALLY BIND ARE IN THE SHEET, and
 *  they are not px. `.phone-cockpit .ph-scope-panel` floors at `8em` and caps at
 *  `calc(0.45 * var(--vh-eff))`; the call sites declared 100 px and 420 px. Both ends of
 *  the drag were therefore DEAD TRAVEL — below 112 px and above 0.45·--vh-eff the
 *  pointer moved the variable and the panel did not move at all, which reads to the
 *  operator as a broken handle. A clamp written in the sheet's own units cannot drift
 *  out of agreement with it; cockpit-shells.test.ts computes both sides and fails when
 *  they do. */
export type SplitClamp = number | ((g: SplitGeom) => number)

export function resolveClamp(c: SplitClamp, g: SplitGeom): number {
  return typeof c === 'function' ? c(g) : c
}

/** The clamps of the CW/Phone band-scope strip, in the sheet's units — kept here, beside
 *  the resolver, because both cockpits declare the identical pair and the sheet rules
 *  (`.cw-cockpit .ph-scope-panel` / `.phone-cockpit .ph-scope-panel`) are their only
 *  other definition. */
export const SCOPE_SPLIT_MIN: SplitClamp = (g) => 8 * g.fontPx
export const SCOPE_SPLIT_MAX: SplitClamp = (g) => 0.45 * g.vhEff

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

/** The geometry `el` currently lives in, for resolving a SplitClamp. `--vh-eff` is
 *  published on <html> by useViewport; the fallback matches its own formula so a
 *  splitter still clamps sanely before the first stamp (and in tests). */
function splitGeom(el: HTMLElement): SplitGeom {
  const fontPx = parseFloat(getComputedStyle(el).fontSize)
  const vh = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--vh-eff'))
  return {
    fontPx: Number.isFinite(fontPx) && fontPx > 0 ? fontPx : 16,
    vhEff: Number.isFinite(vh) && vh > 0 ? vh : window.innerHeight / elZoom(el),
  }
}

export function Splitter({ axis, varName, target, storageKey, min, max, defaultPct, label }: Props) {
  // Apply the persisted (or default) size once the target exists — CLAMPED against the
  // CURRENT container box. The mount used to replay the stored % raw, which is how a
  // scope dragged to max at one geometry reopened past its own drag cap at another
  // (the stored value itself is left alone — it re-applies where it is legal again).
  useEffect(() => {
    const el = target.current
    if (!el) return
    const rect = el.getBoundingClientRect()
    const span = (axis === 'y' ? rect.height : rect.width) / elZoom(el)
    const g = splitGeom(el)
    const pct = clampSplitPct(
      loadPct(storageKey) ?? defaultPct,
      span,
      resolveClamp(min, g),
      resolveClamp(max, g),
    )
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
    // Resolved once per drag: --vh-eff and the font size cannot change mid-gesture.
    const g = splitGeom(el)
    const minCss = resolveClamp(min, g)
    const maxCss = resolveClamp(max, g)
    const pctFor = (ev: PointerEvent) => {
      const px = (axis === 'y' ? ev.clientY - rect.top : ev.clientX - rect.left) / z
      return clampSplitPct((px / span) * 100, span, minCss, maxCss)
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
      title={t('splitter.title', { label })}
      onPointerDown={start}
    />
  )
}
