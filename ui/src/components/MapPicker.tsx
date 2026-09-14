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
import type { MapChoice } from '../features/intentMapSettings'
import { t } from '../i18n'

/** The WebGL renderer's own name — the same two characters in every language. */
const THREE_D = '3D'

/** Connect's four choices, in the operator's order: the default first. */
export const ALL_MAP_CHOICES: readonly MapChoice[] = ['globe', '3d', 'world', 'aeqd']
/** A 2-D-only map's choices (no WebGL renderer behind it). */
export const FLAT_MAP_CHOICES: readonly MapChoice[] = ['globe', 'world', 'aeqd']

const label = (c: MapChoice): string =>
  c === 'globe'
    ? t('map.projection.globe.label')
    : c === '3d'
      ? THREE_D
      : c === 'world'
        ? t('map.projection.world.label')
        : t('map.projection.beam.label')

const title = (c: MapChoice): string =>
  c === 'globe'
    ? t('map.projection.globe.title')
    : c === '3d'
      ? t('map.projection.webgl.title')
      : c === 'world'
        ? t('map.projection.world.title')
        : t('map.projection.beam.title')

export function MapPicker({
  choices,
  value,
  onPick,
  threeDUnavailable = false,
}: {
  choices: readonly MapChoice[]
  value: MapChoice
  onPick: (choice: MapChoice) => void
  /** This machine's GPU cannot run the WebGL globe: 3D stays in the row, announced as unavailable,
   *  with the reason as its tooltip — `aria-disabled` rather than `disabled`, so it can still be
   *  focused and its explanation read. */
  threeDUnavailable?: boolean
}) {
  return (
    <div className="map-proj map-picker" role="group" aria-label={t('map.projection.aria')}>
      {choices.map((c) => {
        const off = c === '3d' && threeDUnavailable
        return (
          <button
            key={c}
            type="button"
            className={value === c ? 'active' : ''}
            aria-pressed={value === c}
            aria-disabled={off || undefined}
            title={off ? t('globe.unsupported') : title(c)}
            onClick={() => {
              if (!off) onPick(c)
            }}
          >
            {label(c)}
          </button>
        )
      })}
    </div>
  )
}
