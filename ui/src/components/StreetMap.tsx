// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Its sentences come from
// the catalog; "© OpenStreetMap" is the data's own credit and is never translated.
//
// STREET: the downloadable street-level map, MapLibre GL drawing an installed pack, offline.
//
// A THIRD RENDERER beside the d3 flat map and the three.js globe, LAZY-loaded the way Globe3D is
// (`lazy(() => import('./StreetMap'))`): MapLibre, pmtiles and the basemap style download only when
// an operator picks Street. Mount it only while it is shown: one MapLibre instance, so one WebGL
// context, per mount, and `map.remove()` on unmount releases the context. A keep-alive host must
// unmount it, never just hide it.
//
// NEVER A BLANK. Three states replace the map with a sentence:
//   · no WebGL2 (MapLibre v6 needs it; software GL is fine): the gate in gpu.ts, and MapLibre's own
//     refusal to start;
//   · the GPU context lost and not restored within RESTORE_GRACE_MS: "paused", with Reload. The dead
//     map is removed at once, and Reload builds a fresh one;
//   · the pack itself cannot be read (its header failed to load).
//
// THE CAMERA, for Nexus's overlays (the caller draws them above this; nothing here does):
// `onCamera` hears the centre, MapLibre's zoom, the bearing and MapLibre's own `project` at once,
// and again on every move, resize and rendered frame, so an overlay re-syncs in the frame MapLibre
// draws; it hears null when the map goes (unmounted, paused, unreadable). It also hears whether the
// map is still moving (a drag, an animated pan or zoom, the drift after a drag), and once more when it
// stops (`moveend`), so an overlay can hold its picture through the motion and redraw at the end.
// Rotation and pitch are off, and the world is not repeated, so a Mercator locked to the camera lines
// up with the map exactly (features/streetOverlay.ts).
//
// INPUT. MapLibre owns drag and zoom. `onPointer` hands the caller MapLibre's own pointer events in
// the map's CSS px, for the overlays' hit test; `preventDefault()` on a double-click keeps MapLibre
// from zooming, so a double-click on one of the caller's targets keeps the caller's meaning. `cursor`
// replaces MapLibre's grab hand while the pointer is over such a target.
//
// ZERO NETWORK: the style, glyphs, sprites and tiles all come through features/streetPack.ts, and
// MapLibre's worker is a file of this build. The © OpenStreetMap credit is always on screen: the
// attribution control is never compact, so it has no button that folds it away.

import 'maplibre-gl/dist/maplibre-gl.css'
import './StreetMap.css'
import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { AttributionControl, GPUInitializationError, MapLibreMap, setWorkerUrl, type MapMouseEvent } from 'maplibre-gl'
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url'
import { webgl2Available } from '../gpu'
import { t } from '../i18n'
import { useLocale } from '../i18n/useLocale'
import { openStreetPack, packKey, type StreetPack } from '../features/streetPack'
import { labelLanguage, readStreetLook, STREET_SOURCE, streetStyle } from '../features/streetStyle'

// MapLibre finds its worker next to its own module only when that is an http(s) URL. Bundled, it is
// neither next to it nor (on Linux and macOS, under tauri://) http, so name the built file.
setWorkerUrl(workerUrl)

/** How long a lost GPU context may stay lost before the map is shown paused. */
export const RESTORE_GRACE_MS = 10_000
/** Where a map opens when the caller has no view for it: the pack's centre at this zoom. */
const OPEN_ZOOM = 12
/** The data's credit, linked to its licence; externalLinks.ts opens it in the system browser. */
const OSM_CREDIT =
  '<a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noopener">© OpenStreetMap</a>'

/** What an overlay needs to draw in step with the map. */
export interface StreetCamera {
  /** [longitude, latitude] in degrees. */
  center: [number, number]
  /** MapLibre's zoom: the world is 512 · 2^zoom CSS px wide. */
  zoom: number
  /** Degrees. Always 0 while rotation is off; carried so an overlay never has to assume it. */
  bearing: number
  /** A position's CSS px from the map's top-left corner: MapLibre's own projection. */
  project: (lonLat: [number, number]) => [number, number]
  /** MapLibre is mid-gesture or mid-animation (`isMoving`). The camera is heard again, not moving,
   *  when it stops. */
  moving: boolean
}

/** One of MapLibre's pointer events, where it happened in the map's CSS px. */
export interface StreetPointer {
  type: 'move' | 'out' | 'click' | 'dblclick'
  x: number
  y: number
  /** On a double-click: MapLibre does not zoom. */
  preventDefault: () => void
}

/** What the caller can ask of the open map. */
export interface StreetMapHandle {
  /** Zoom in (positive) or out (negative) by whole steps, animated, about the centre. */
  zoomBy: (steps: number) => void
}

export interface StreetMapProps {
  /** The installed pack to draw: one entry of `street_map_packs()`. */
  pack: StreetPack
  /** Where to open, [longitude, latitude]; the pack's centre when absent. Read once, at mount. */
  center?: [number, number]
  /** The zoom to open at; 12 when absent. Read once, at mount. */
  zoom?: number
  onCamera?: (camera: StreetCamera | null) => void
  onPointer?: (e: StreetPointer) => void
  /** A CSS cursor over the map instead of MapLibre's own; absent = MapLibre's. */
  cursor?: string
}

type Shown = 'map' | 'noWebgl2' | 'paused' | 'unreadable'

function cameraOf(map: MapLibreMap): StreetCamera {
  const { lng, lat } = map.getCenter()
  return {
    center: [lng, lat],
    zoom: map.getZoom(),
    bearing: map.getBearing(),
    project: (lonLat) => {
      const p = map.project(lonLat)
      return [p.x, p.y]
    },
    moving: map.isMoving(),
  }
}

const packCentre = ({ bbox: [w, s, e, n] }: StreetPack): [number, number] => [(w + e) / 2, (s + n) / 2]

/** The theme's tokens live on <html>, and every appearance setting (theme, Night, contrast, the
 *  built-in themes, the colour roles) is an attribute there: re-read the look when one changes. */
function subscribeLook(onChange: () => void): () => void {
  const mo = new MutationObserver(onChange)
  mo.observe(document.documentElement, { attributes: true })
  return () => mo.disconnect()
}
const lookSnapshot = () => JSON.stringify(readStreetLook())

export default forwardRef<StreetMapHandle, StreetMapProps>(function StreetMap(
  { pack, center, zoom, onCamera, onPointer, cursor },
  ref,
) {
  const locale = useLocale()
  const look = useSyncExternalStore(subscribeLook, lookSnapshot)
  // A pack is the same pack while its id and sha256 are: `street_map_packs()` answers with new
  // objects every time it is asked, and listing the packs again must not rebuild the map.
  const key = packKey(pack)
  const style = useMemo(() => streetStyle(pack, JSON.parse(look), labelLanguage(locale)), [key, look, locale])
  const [shown, setShown] = useState<Shown>(() => (webgl2Available() ? 'map' : 'noWebgl2'))

  const hostRef = useRef<HTMLDivElement>(null)
  const mapRef = useRef<MapLibreMap | null>(null)
  // The latest style and callback, for the map the effect below builds once per pack.
  const styleRef = useRef(style)
  styleRef.current = style
  const appliedRef = useRef(style)
  const onCameraRef = useRef(onCamera)
  onCameraRef.current = onCamera
  const onPointerRef = useRef(onPointer)
  onPointerRef.current = onPointer

  useImperativeHandle(ref, () => ({ zoomBy: (steps) => mapRef.current?.zoomTo(mapRef.current.getZoom() + steps) }), [])

  useEffect(() => {
    const host = hostRef.current
    if (shown !== 'map' || !host) return
    const close = openStreetPack(pack)
    let map: MapLibreMap
    try {
      map = new MapLibreMap({
        container: host,
        style: styleRef.current,
        center: center ?? packCentre(pack),
        zoom: zoom ?? OPEN_ZOOM,
        attributionControl: false,
        dragRotate: false,
        pitchWithRotate: false,
        touchPitch: false,
        maxPitch: 0,
        renderWorldCopies: false,
        locale: { 'Map.Title': t('map.street.aria') },
      })
    } catch (e) {
      close()
      if (!(e instanceof GPUInitializationError)) throw e
      setShown('noWebgl2')
      return
    }
    appliedRef.current = styleRef.current
    mapRef.current = map
    map.touchZoomRotate.disableRotation()
    map.keyboard.disableRotation()
    map.addControl(new AttributionControl({ compact: false, customAttribution: OSM_CREDIT }), 'bottom-right')

    let lost: number | undefined
    map.on('webglcontextlost', () => {
      window.clearTimeout(lost)
      lost = window.setTimeout(() => setShown('paused'), RESTORE_GRACE_MS)
    })
    map.on('webglcontextrestored', () => window.clearTimeout(lost))
    map.on('error', (e) => {
      // A tile that fails carries `tile`; the pack's own header failing carries only the source.
      const { sourceId, tile } = e as unknown as { sourceId?: string; tile?: unknown }
      if (sourceId === STREET_SOURCE && tile === undefined) setShown('unreadable')
    })
    const report = () => onCameraRef.current?.(cameraOf(map))
    map.on('move', report)
    map.on('moveend', report)
    map.on('resize', report)
    map.on('render', report)
    report()
    const pointer = (type: StreetPointer['type']) => (e: MapMouseEvent) =>
      onPointerRef.current?.({ type, x: e.point.x, y: e.point.y, preventDefault: () => e.preventDefault() })
    map.on('mousemove', pointer('move'))
    map.on('mouseout', pointer('out'))
    map.on('click', pointer('click'))
    map.on('dblclick', pointer('dblclick'))

    // Device pixels per layout pixel, so the map stays sharp under the app's CSS zoom: the
    // MapView/Waterfall pattern. MapLibre's own default, devicePixelRatio, cannot see zoom.
    let ro: ResizeObserver | undefined
    if (typeof ResizeObserver === 'function') {
      ro = new ResizeObserver(([entry]) => {
        const dev = entry?.devicePixelContentBoxSize?.[0]
        const css = entry?.contentBoxSize?.[0]
        if (!dev || !css || css.inlineSize <= 0) return
        const ratio = dev.inlineSize / css.inlineSize
        if (Number.isFinite(ratio) && Math.abs(map.getPixelRatio() - ratio) > 1e-3) map.setPixelRatio(ratio)
      })
      try {
        ro.observe(host, { box: 'device-pixel-content-box' })
      } catch {
        /* an engine without that box keeps MapLibre's devicePixelRatio */
      }
    }

    return () => {
      ro?.disconnect()
      window.clearTimeout(lost)
      mapRef.current = null
      map.remove()
      close()
      onCameraRef.current?.(null)
    }
    // `center` and `zoom` open the map; they never move one that is already open.
  }, [shown, key])

  // The caller's cursor wins over MapLibre's grab hand only while it names one.
  useEffect(() => {
    const map = mapRef.current
    if (map) map.getCanvasContainer().style.cursor = cursor ?? ''
  }, [cursor, shown, key])

  // A theme, Night, contrast or language change repaints the open map in place.
  useEffect(() => {
    const map = mapRef.current
    if (!map || appliedRef.current === style) return
    appliedRef.current = style
    map.setStyle(style)
  }, [style])

  if (shown === 'map') {
    return (
      <div className="street-map">
        <div ref={hostRef} className="street-map-canvas" />
      </div>
    )
  }
  return (
    <div className="street-map street-map-note" role="status">
      {shown === 'noWebgl2' && <p>{t('map.street.noWebgl2')}</p>}
      {shown === 'unreadable' && <p>{t('map.street.unreadable')}</p>}
      {shown === 'paused' && (
        <>
          <p>{t('map.street.paused')}</p>
          <button type="button" onClick={() => setShown('map')}>
            {t('map.street.reload')}
          </button>
        </>
      )}
    </div>
  )
})
