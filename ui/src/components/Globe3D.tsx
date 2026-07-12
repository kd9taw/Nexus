// The opt-in WebGL 3-D globe (react-globe.gl → globe.gl → three.js) for higher-end
// machines. The 2-D Canvas globe (MapView) stays the universal default; this mode is
// lazy-loaded, so a low-end shack PC never downloads three.js unless the operator turns
// it on. It reuses the SAME propagation data as MapView — spots, the operator's QTH —
// but renders them on a real textured sphere with atmosphere + an animated QTH ping.
//
// First cut: textured globe + atmosphere + a "you are here" ring at the QTH, auto-framed
// on the operator's grid. Spot points and great-circle arcs wire in next.
import { useEffect, useMemo, useRef, useState } from 'react'
import Globe, { type GlobeMethods } from 'react-globe.gl'
import earthUrl from '../assets/earth-relief.webp'
import { gridToLatLon } from '../grid'

interface Props {
  /** The operator's Maidenhead grid — places + frames the QTH. */
  myGrid: string
}

/** react-globe.gl wants { lat, lng }; the app's grid helper yields { lat, lon }. */
interface GlobePoint {
  lat: number
  lng: number
}

export default function Globe3D({ myGrid }: Props) {
  const wrapRef = useRef<HTMLDivElement>(null)
  const globeRef = useRef<GlobeMethods | undefined>(undefined)
  const [size, setSize] = useState({ w: 0, h: 0 })

  // Track the container size (the globe canvas is sized in px, not %).
  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const ro = new ResizeObserver(() => {
      setSize({ w: el.clientWidth, h: el.clientHeight })
    })
    ro.observe(el)
    setSize({ w: el.clientWidth, h: el.clientHeight })
    return () => ro.disconnect()
  }, [])

  const qth = useMemo<GlobePoint | null>(() => {
    const ll = gridToLatLon(myGrid)
    return ll ? { lat: ll.lat, lng: ll.lon } : null
  }, [myGrid])

  // Frame the globe on the operator's QTH and start a slow auto-rotate.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !qth) return
    g.pointOfView({ lat: qth.lat, lng: qth.lng, altitude: 2.2 }, 0)
    const controls = g.controls() as { autoRotate: boolean; autoRotateSpeed: number }
    controls.autoRotate = true
    controls.autoRotateSpeed = 0.35
  }, [qth])

  // The QTH "you are here" ping — an expanding ring at the operator's grid.
  const rings = qth ? [qth] : []

  return (
    <div ref={wrapRef} style={{ width: '100%', height: '100%', position: 'relative' }}>
      <Globe
        ref={globeRef}
        width={size.w || undefined}
        height={size.h || undefined}
        backgroundColor="rgba(0,0,0,0)"
        globeImageUrl={earthUrl}
        showAtmosphere
        atmosphereColor="#68a8e2"
        atmosphereAltitude={0.18}
        ringsData={rings}
        ringColor={() => '#4ea1ff'}
        ringMaxRadius={4}
        ringPropagationSpeed={1.4}
        ringRepeatPeriod={1400}
      />
    </div>
  )
}
