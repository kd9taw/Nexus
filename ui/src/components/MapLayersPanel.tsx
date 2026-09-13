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

export function MapLayersPanel({
  className,
  title,
  children,
}: {
  /** The surface's own class (`map-layers` / `globe3d-layers`) — both share the overlay rule. */
  className: string
  /** The panel's heading, which is also its accessible name and the pill's label. */
  title: string
  children: ReactNode
}) {
  const [collapsed, setCollapsed] = useState(() => surfaceGet(COLLAPSE_KEY) === '1')
  const toggle = () =>
    setCollapsed((v) => {
      const nv = !v
      surfaceSet(COLLAPSE_KEY, nv ? '1' : '0')
      return nv
    })

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
