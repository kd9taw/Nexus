// The sun disc with NOAA's numbered active regions — where on the sun a flare can come from. In
// the unfolded flare chip, on the 2-D map and the 3-D globe alike.
//
// It draws only what `sunRegions.regionsShown` allows (a list the shack holds, before its `until`
// on this clock); otherwise one dim line says why nothing is drawn. Every drawing names its source
// and day ("NOAA active regions · 29 Oct"), and the hover says the regions are drawn where NOAA
// observed them that day, not moved on to now. Solar north up, east on the left, as NOAA's images
// show the sun.

import { t } from '../../i18n'
import { limbSide, regionDiscXY, regionsDayLabel, regionsShown, regionsWhy } from '../../sunRegions'
import type { SolarRegion, SolarRegionsView } from '../../types'

/** Drawn size (SVG user units; the CSS sets the size on screen). */
const R = 30
const PAD = 6

/** "4564 N05E68" — NOAA's number and its own location string (or the position, when it sent none). */
export function regionLabel(r: SolarRegion): string {
  const ns = `${r.latDeg >= 0 ? 'N' : 'S'}${String(Math.round(Math.abs(r.latDeg))).padStart(2, '0')}`
  const ew = `${r.lonDeg >= 0 ? 'E' : 'W'}${String(Math.round(Math.abs(r.lonDeg))).padStart(2, '0')}`
  return `${r.region} ${r.location ?? ns + ew}`
}

/** One region with its limb word, for a list ("4564 N05E68 (near the east limb)"). */
export function regionWithLimb(r: SolarRegion): string {
  const side = limbSide(r)
  const limb = side === 'east' ? t('map.sun.regions.eastLimb') : side === 'west' ? t('map.sun.regions.westLimb') : null
  return limb ? `${regionLabel(r)} (${limb})` : regionLabel(r)
}

export function SunRegionsDisc({ view, nowMs }: { view: SolarRegionsView | null | undefined; nowMs: number }) {
  const regions = regionsShown(view, nowMs)
  if (regions == null) {
    const why = regionsWhy(view, nowMs)
    const day = view?.observedDay != null ? regionsDayLabel(view.observedDay) : ''
    return (
      <div className="flare-chip-line flare-chip-dim sun-regions-none" data-why={why}>
        {why === 'stale'
          ? t('map.sun.regions.stale', { day })
          : why === 'ahead'
            ? t('map.sun.regions.ahead', { day })
            : why === 'unknown'
              ? t('map.sun.regions.unknown')
              : t('map.sun.regions.none')}
      </div>
    )
  }
  const day = regionsDayLabel(view!.observedDay!)
  const title = t('map.sun.regions.title', { day })
  return (
    <div className="flare-chip-line sun-regions" title={title}>
      <div className="flare-chip-dim">{t('map.sun.regions.head', { day })}</div>
      <svg
        className="sun-regions-disc"
        viewBox={`${-R - PAD} ${-R - PAD} ${2 * (R + PAD)} ${2 * (R + PAD)}`}
        role="img"
        aria-label={t('map.sun.regions.aria', { day })}
      >
        <title>{title}</title>
        <circle className="sun-regions-face" cx={0} cy={0} r={R} />
        {regions.map((r) => {
          const p = regionDiscXY(r)
          return (
            <g key={r.region} data-region={r.region}>
              <circle className="sun-regions-dot" cx={p.x * R} cy={-p.y * R} r={1.8} />
              {/* Sized in the disc's own viewBox units, so it scales with the drawing. */}
              <text x={p.x * R} y={-p.y * R - 3} textAnchor="middle" fontSize={6} fill="currentColor">
                {r.region}
              </text>
            </g>
          )
        })}
      </svg>
      <div className="flare-chip-dim">
        {regions.length > 0 ? regions.map(regionWithLimb).join(' · ') : t('map.sun.regions.empty', { day })}
      </div>
    </div>
  )
}
