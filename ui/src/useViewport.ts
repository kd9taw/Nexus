import { useEffect, useMemo, useSyncExternalStore } from 'react'

// Responsive size-class driver.
//
// The UI zoom is applied as `zoom: var(--ui-zoom)` on `.app` (a non-root element),
// which magnifies content WITHOUT changing the layout viewport — so plain
// `@media (max-width: …)` queries fire against the *unzoomed* window width and
// mis-fire at every zoom level. The fix: compute the EFFECTIVE content width
// (`innerWidth / zoom`) in JS and publish it as a `data-viewport` size class on
// <html>. CSS keys off `[data-viewport='xs'|'sm'|…]` instead of raw-px media
// queries, so breakpoints are correct at any zoom.

export type ViewportClass = 'xs' | 'sm' | 'md' | 'lg' | 'xl'

/** Map an EFFECTIVE (zoom-adjusted) CSS width to a size class. Pure + testable. */
export function classifyViewport(effW: number): ViewportClass {
  if (effW < 768) return 'xs'
  if (effW < 1100) return 'sm'
  if (effW < 1600) return 'md'
  if (effW < 2400) return 'lg'
  return 'xl'
}

const VIEWPORT_CLASSES: readonly string[] = ['xs', 'sm', 'md', 'lg', 'xl']

function readViewportClass(): ViewportClass | null {
  const v = document.documentElement.getAttribute('data-viewport')
  return v != null && VIEWPORT_CLASSES.includes(v) ? (v as ViewportClass) : null
}

function subscribeViewportClass(onChange: () => void): () => void {
  if (typeof MutationObserver === 'undefined') return () => {}
  const mo = new MutationObserver(onChange)
  mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-viewport'] })
  return () => mo.disconnect()
}

/**
 * The size class `useViewport` publishes on <html>, as a value a component can decide from — the
 * dashboard rail renders nothing below `lg`. It READS the published class and never computes its
 * own, so a component and the stylesheet cannot disagree about the window; null until the first stamp.
 */
export function useViewportClass(): ViewportClass | null {
  return useSyncExternalStore(subscribeViewportClass, readViewportClass, () => null)
}

function readViewportSize(): string {
  const style = document.documentElement.style
  return `${style.getPropertyValue('--vw-eff')} ${style.getPropertyValue('--vh-eff')}`
}

function subscribeViewportSize(onChange: () => void): () => void {
  if (typeof MutationObserver === 'undefined') return () => {}
  const mo = new MutationObserver(onChange)
  mo.observe(document.documentElement, { attributes: true, attributeFilter: ['style'] })
  return () => mo.disconnect()
}

/**
 * The effective size `useViewport` publishes on <html> (`--vw-eff` × `--vh-eff`), for a layout that turns on
 * the height as well as the width: a phone on its side is `sm` by its 915 px of width and has 412 px of height.
 * Read, never computed again, as `useViewportClass` reads the class; null until the first stamp.
 */
export function useViewportSize(): { width: number; height: number } | null {
  const raw = useSyncExternalStore(subscribeViewportSize, readViewportSize, () => '')
  return useMemo(() => {
    const [width, height] = raw.split(' ').map(parseFloat)
    return Number.isFinite(width) && Number.isFinite(height) ? { width, height } : null
  }, [raw])
}

/** Read the live `--ui-zoom` (defaults to 1 if unset/invalid). */
function currentZoom(): number {
  const raw = getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')
  const z = parseFloat(raw)
  return Number.isFinite(z) && z > 0 ? z : 1
}

/**
 * Keep `data-viewport` (and `--vh-eff` / `--vw-eff`) on <html> in sync with the
 * effective viewport, live on resize (rAF-debounced). One listener for the whole app.
 *
 * Pass the current UI `scale` so the size class is recomputed when the operator
 * changes zoom (the effective width shifts even though the window didn't resize).
 */
export function useViewport(scale?: number, visibleArea = false): void {
  useEffect(() => {
    let raf = 0
    const apply = () => {
      const zoom = currentZoom()
      // Standalone phone observers opt into the area above an on-screen keyboard.
      // Existing desktop cockpits retain their layout-viewport behavior.
      // A pinch shrinks the visual viewport too, by its scale, and the page must not
      // follow that: it shrank the stream page into a corner of the zoomed view. Times
      // its scale the area is the one at the page's own scale, so the keyboard still
      // counts and a pinch does not (at scale 1 this is the area exactly as before).
      const viewport = visibleArea ? window.visualViewport : null
      const width = viewport ? viewport.width * viewport.scale : window.innerWidth
      const height = viewport ? viewport.height * viewport.scale : window.innerHeight
      const effW = width / zoom
      const effH = height / zoom
      const d = document.documentElement
      d.setAttribute('data-viewport', classifyViewport(effW))
      d.style.setProperty('--vh-eff', `${effH}px`)
      d.style.setProperty('--vw-eff', `${effW}px`)
      // Fill-to-bottom correction: measure the app shell's RENDERED height and fix
      // any shortfall with an explicit pixel height. `zoom` × percentage-height
      // semantics vary across engine versions (a static calc() left a dead band at
      // the bottom of every view on some builds); measuring the real box and
      // correcting in layout units is right regardless of which semantic applies.
      const app = document.querySelector<HTMLElement>('.app')
      if (app) {
        app.style.height = '' // re-measure the stylesheet's natural 100% first
        const visual = app.getBoundingClientRect().height // post-zoom visual px
        const gap = height - visual
        if (Math.abs(gap) > 1) {
          const layoutH = parseFloat(getComputedStyle(app).height) // layout px
          if (Number.isFinite(layoutH)) {
            app.style.height = `${layoutH + gap / zoom}px`
          }
        }
      }
    }
    const onResize = () => {
      cancelAnimationFrame(raf)
      raf = requestAnimationFrame(apply)
    }
    // Defer one frame so a just-changed --ui-zoom is committed before we read it.
    raf = requestAnimationFrame(apply)
    window.addEventListener('resize', onResize)
    if (visibleArea) window.visualViewport?.addEventListener('resize', onResize)
    return () => {
      window.removeEventListener('resize', onResize)
      if (visibleArea) window.visualViewport?.removeEventListener('resize', onResize)
      cancelAnimationFrame(raf)
    }
  }, [scale, visibleArea])
}
