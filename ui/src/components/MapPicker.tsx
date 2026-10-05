// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). `3D` is the WebGL renderer's own name,
// a token like the intent chips' `POTA/SOTA` (see ConnectView.tsx); everything else is catalog.
//
// THE MAP PICKER — the one control for "what map am I looking at" (operator decision 2026-09-13,
// late). It replaced two: Connect's 2D/3D header toggle and the 2-D map's Globe / Beam / World
// buttons, which lived inside the map and unmounted with it whenever 3D was on.
//
// Connect hosts it ONCE, in a bar at the top of the map cell (ConnectView), so it is the same node
// whatever renders beneath it. A map with no 3-D renderer to offer — the dedicated POTA map pop-out
// — puts the same component in its own toolbar without the 3D choice.
//
// STREET (operator ruling 2026-10-04, D5) joins Connect's row only once the street map is offered
// (features/streetMaps.ts: hidden until its maps are hosted). Until a pack is installed it carries a
// download badge, and a press opens the download sheet instead of picking it; while a download runs
// it carries the percent instead.
import { Download } from 'lucide-react'
import type { MapChoice } from '../features/intentMapSettings'
import { t } from '../i18n'

/** The WebGL renderer's own name — the same two characters in every language. */
const THREE_D = '3D'

/** Connect's four choices, in the operator's order: the default first. */
export const ALL_MAP_CHOICES: readonly MapChoice[] = ['globe', '3d', 'world', 'aeqd']
/** …and with the street map offered, Street after them, so the four keep their places. */
export const STREET_MAP_CHOICES: readonly MapChoice[] = [...ALL_MAP_CHOICES, 'street']
/** A 2-D-only map's choices (no WebGL renderer behind it). */
export const FLAT_MAP_CHOICES: readonly MapChoice[] = ['globe', 'world', 'aeqd']

const label = (c: MapChoice): string =>
  c === 'globe'
    ? t('map.projection.globe.label')
    : c === '3d'
      ? THREE_D
      : c === 'world'
        ? t('map.projection.world.label')
        : c === 'street'
          ? t('map.projection.street.label')
          : t('map.projection.beam.label')

const title = (c: MapChoice): string =>
  c === 'globe'
    ? t('map.projection.globe.title')
    : c === '3d'
      ? t('map.projection.webgl.title')
      : c === 'world'
        ? t('map.projection.world.title')
        : c === 'street'
          ? t('map.projection.street.title')
          : t('map.projection.beam.title')

export function MapPicker({
  choices,
  value,
  onPick,
  threeDUnavailable = false,
  street,
}: {
  choices: readonly MapChoice[]
  value: MapChoice
  onPick: (choice: MapChoice) => void
  /** This machine's GPU cannot run the WebGL globe: 3D stays in the row, announced as unavailable,
   *  with the reason as its tooltip — `aria-disabled` rather than `disabled`, so it can still be
   *  focused and its explanation read. */
  threeDUnavailable?: boolean
  /** The Street choice, where it is offered: whether a pack is installed, a running download's
   *  percent, and why this machine cannot draw it (no WebGL2), which makes it unavailable as 3D is. */
  street?: { installed: boolean; percent: number | null; unavailable: string | null }
}) {
  return (
    <div className="map-proj map-picker" role="group" aria-label={t('map.projection.aria')}>
      {choices.map((c) => {
        const offWhy = c === '3d' && threeDUnavailable ? t('globe.unsupported') : c === 'street' ? (street?.unavailable ?? null) : null
        const needsPack = c === 'street' && street != null && !street.installed
        const percent = c === 'street' ? (street?.percent ?? null) : null
        return (
          <button
            key={c}
            type="button"
            className={value === c ? 'active' : ''}
            aria-pressed={value === c}
            aria-disabled={offWhy != null || undefined}
            title={offWhy ?? (needsPack ? t('map.projection.street.download') : title(c))}
            onClick={() => {
              if (offWhy == null) onPick(c)
            }}
          >
            {label(c)}
            {percent != null ? (
              <span className="map-picker-chip">{t('map.street.chip', { pct: percent })}</span>
            ) : (
              needsPack && <Download className="map-picker-badge" size={12} aria-hidden="true" />
            )}
          </button>
        )
      })}
    </div>
  )
}
