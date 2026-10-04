// THE SPOTS A SCOPE TAGS, built by its host: BandMap's set (the scope's mode on its band) and BandMap's
// mark for each. Apart from `overlays.ts` so the scope, which only draws them, never loads the need
// palette (and its icons) that the mark is read from.
import { NEED_CHIP } from '../features/needVisuals'
import type { NeedTag, SpotRow } from '../types'
import type { ScopeSpot } from './overlays'

/** BandMap's mark for a spot, as the CSS token its `need-*` / `pota-dim` class paints: never on a
 *  beacon (a one-way transmission is never a need), else its top need's colour, else the dim POTA
 *  colour for a POTA activator no need colours, else none. */
export function spotInk(
  s: SpotRow,
  needByCall?: Map<string, NeedTag>,
  typeByCall?: Map<string, 'Pota' | 'Sota' | 'Dxped'>,
): string | null {
  if (s.beacon) return null
  const cu = s.call.toUpperCase()
  const need = needByCall?.get(cu)
  if (need) return `--need-${NEED_CHIP[need].cls}`
  return typeByCall?.get(cu) === 'Pota' ? '--pota-dim' : null
}

/** The spots a scope tags: BandMap's set — this mode on this band — each with its mark. */
export function scopeSpots(
  spots: readonly SpotRow[],
  mode: 'Phone' | 'CW',
  band: string,
  needByCall?: Map<string, NeedTag>,
  typeByCall?: Map<string, 'Pota' | 'Sota' | 'Dxped'>,
): ScopeSpot[] {
  return spots
    .filter((s) => s.mode === mode && s.band === band)
    .map((s) => ({ spot: s, ink: spotInk(s, needByCall, typeByCall) }))
}
