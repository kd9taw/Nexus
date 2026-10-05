// What a 3-D globe does with its WebGL context. Globe3D (Connect) and QsoGlobe (the Logbook) are the
// only two, and both call `useGlobeWebgl` and render `GlobePaused`.
//
// HANDED BACK WHEN THE GLOBE GOES. A browser keeps a handful of live WebGL contexts per page (Chromium
// keeps 16) and, past that, takes the OLDEST away, which can be the waterfall's or the scope's. A closed
// globe used to keep its context for good. OrbitControls hangs a keydown listener on the canvas's root
// node and looks that root up again to take it off, and globe.gl disposes the controls only after React
// has detached the canvas, so the lookup found the detached subtree and the listener stayed on the
// document, holding the controls, the renderer, its context and its textures. So the controls are
// disconnected in a LAYOUT cleanup, which runs while the canvas is still in the document, and the
// context is handed back explicitly in the passive cleanup: the renderer disposed, then
// `forceContextLoss()` (the spectrum renderer's `destroy()` does the same). One more hold outlived
// that: three.js gives every Sprite ONE shared geometry, and a renderer that drew a sprite stays
// registered on it until that geometry is disposed, which nothing else ever does. That kept each
// closed Connect globe's renderer, its DOM and its 4096 x 2048 basemap canvas, so the release disposes
// it too (the next renderer to draw a sprite uploads it again).
//
// SURVIVING A LOSS WHILE SHOWN (a driver reset, sleep, the browser's context cap), the way the spectrum
// renderer does (spectrum/index.ts). `preventDefault()` on `webglcontextlost` asks for the context back;
// the scene is hidden while it is away (Chrome paints a lost canvas white); on `webglcontextrestored`
// three.js rebuilds its own GL state and the globe draws again. With no restore within RESTORE_WAIT_MS
// the globe says so, "3D view paused" with a Reload that mounts it afresh, never a silent blank.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): the words are in the catalog under
// `globe.paused.*`, one set for both globes.
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react'
import { Sprite } from 'three'
import type { GlobeMethods } from 'react-globe.gl'
import { t } from '../i18n'

/** How long a lost context may take to come back before the globe says it is paused. */
export const RESTORE_WAIT_MS = 10_000

/**
 * Hand the globe's WebGL context back when the globe goes, and survive a loss while it is shown.
 * True while the globe is paused: its context lost and not back within RESTORE_WAIT_MS.
 *
 * ⚠️ Call it AFTER the globe's other effects. React runs a component's effect cleanups in the order the
 * effects were declared, and this one's hands the context back, so it must run last: a cleanup after it
 * that drew a frame (QsoGlobe's resumes the loop) would draw into a released context.
 *
 * `redraw` draws the scene again once a lost context is back, for a globe whose loop sleeps while
 * nothing changes (Globe3D's).
 */
export function useGlobeWebgl(
  globe: RefObject<GlobeMethods | undefined>,
  ready: boolean,
  redraw?: () => void,
): boolean {
  const [paused, setPaused] = useState(false)
  const redrawRef = useRef(redraw)
  useEffect(() => {
    redrawRef.current = redraw
  })

  // The controls, disconnected while the canvas is still in the document (see the header). `connect`
  // disconnects first, so the first run changes nothing, and a run after a Suspense boundary hid and
  // revealed the globe wires the controls back up.
  useLayoutEffect(() => {
    const g = ready ? globe.current : undefined
    if (!g) return
    const controls = g.controls()
    controls.connect(g.renderer().domElement)
    return () => controls.disconnect()
  }, [globe, ready])

  useEffect(() => {
    const g = ready ? globe.current : undefined
    if (!g) return
    const renderer = g.renderer()
    const canvas = renderer.domElement
    // The scene's own box (the canvas and the HTML spots over it), hidden while the context is away so
    // nothing of the scene floats over a blank.
    const scene = canvas.parentElement ?? canvas
    let wait: number | undefined
    const onLost = (e: Event) => {
      e.preventDefault() // asks for the context back: without it there is never a restore
      scene.style.visibility = 'hidden'
      window.clearTimeout(wait)
      wait = window.setTimeout(() => setPaused(true), RESTORE_WAIT_MS)
    }
    const onRestored = () => {
      window.clearTimeout(wait)
      scene.style.visibility = ''
      setPaused(false)
      redrawRef.current?.()
    }
    canvas.addEventListener('webglcontextlost', onLost)
    canvas.addEventListener('webglcontextrestored', onRestored)
    return () => {
      // Stop listening first, so the loss this cleanup causes is not taken for a real one.
      canvas.removeEventListener('webglcontextlost', onLost)
      canvas.removeEventListener('webglcontextrestored', onRestored)
      window.clearTimeout(wait)
      g.pauseAnimation()
      renderer.dispose()
      new Sprite().geometry.dispose() // the shared one: every renderer that drew a sprite lets go
      // A context the browser has already taken is not ours to hand back.
      if (!renderer.getContext().isContextLost()) renderer.forceContextLoss()
    }
  }, [globe, ready])

  return paused
}

/** Over a paused globe: what happened, and the Reload that mounts the globe again. */
export function GlobePaused({ onReload }: { onReload: () => void }) {
  return (
    <div className="globe-paused" role="status">
      <span>{t('globe.paused.text')}</span>
      <button type="button" onClick={onReload} title={t('globe.paused.reload.title')}>
        {t('globe.paused.reload')}
      </button>
    </div>
  )
}
