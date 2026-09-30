// The Layers panel's frame on BOTH map surfaces — the 2-D map (MapView) and the 3-D globe
// (Globe3D). One component so the two cannot drift: the same place (an overlay pinned top-left of
// the map; the reasons are on `.map-layers` in styles.css) and the same fold as the Conditions rail
// on the opposite edge (prop/MapInsightRail) — a chevron to collapse, a pill to bring it back,
// remembered per window.
//
// The rows are the caller's: the two surfaces toggle different layer sets.
import { useState, type ReactNode } from 'react'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { t } from '../i18n'

/** PER-SURFACE, like `nexus.connect.insights.collapsed`: folding a panel to reclaim map in THIS
 *  window is pure layout. ONE record for the 2-D and 3-D panels — they sit in the same place, so
 *  the fold follows the operator across the 2D/3D toggle. */
const COLLAPSE_KEY = 'nexus.connect.layersPanel.collapsed'

/** The narrowest map (layout px) that holds this panel OPEN and the Conditions rail side by side:
 *  the panel's place and width (8 + 200), a gap (8), the rail's least width and its margin
 *  (248 + 12), as styles.css draws `.map-layers` and `.map-insights`
 *  (MapLayersPanel.narrow.test.tsx holds the two together). At the 1024×768 floor the map is
 *  about 445 px wide. */
export const OVERLAYS_SIDE_BY_SIDE_PX = 8 + 200 + 8 + 248 + 12

export function MapLayersPanel({
  className,
  title,
  children,
  narrow = false,
}: {
  /** The surface's own class (`map-layers` / `globe3d-layers`) — both share the overlay rule. */
  className: string
  /** The panel's heading, which is also its accessible name and the pill's label. */
  title: string
  children: ReactNode
  /** The map is narrower than OVERLAYS_SIDE_BY_SIDE_PX while the Conditions rail is on it. This
   *  panel stacks above the rail on purpose, so open it would cover the left edge of the rail's
   *  band list (at 1024×768 it covered four band names). While the operator has no fold of their
   *  own on record, the panel is folded here, and opens by itself again on a wider map. */
  narrow?: boolean
}) {
  // What the operator chose, or null: nothing on record. Only then does `narrow` decide.
  const [chosen, setChosen] = useState<boolean | null>(() => {
    const stored = surfaceGet(COLLAPSE_KEY)
    return stored === '1' ? true : stored === '0' ? false : null
  })
  const collapsed = chosen ?? narrow
  const toggle = () => {
    const nv = !collapsed
    surfaceSet(COLLAPSE_KEY, nv ? '1' : '0')
    setChosen(nv)
  }

  if (collapsed) {
    return (
      <button
        type="button"
        className={`${className} collapsed`}
        onClick={toggle}
        aria-expanded={false}
        title={t('map.layers.expand.title')}
      >
        <ChevronRight size={14} aria-hidden="true" />
        <span className="ml-pill-label">{title}</span>
      </button>
    )
  }

  return (
    <aside className={className} aria-label={title}>
      <div className="ml-head">
        <span className="ml-title">{title}</span>
        <button
          type="button"
          className="ml-collapse"
          onClick={toggle}
          aria-expanded={true}
          aria-label={t('map.layers.collapse')}
          title={t('map.layers.collapse')}
        >
          <ChevronLeft size={14} aria-hidden="true" />
        </button>
      </div>
      {children}
    </aside>
  )
}
