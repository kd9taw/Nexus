import type { DecodeRow, NeedAlert, NeedTag } from '../types'
import { NEED_PRECEDENCE, NEED_VISUALS, type NeedCat } from './needVisuals'
import {
  isRareGrid,
  needScopeMhz,
  needScopeOk,
  tagsForSurface,
  type NeedBandScopes,
} from './needs'

const TAG_TO_CAT: Record<NeedTag, NeedCat> = {
  NewEntity: 'entity',
  NewZone: 'zone',
  NewBand: 'band',
  NewMode: 'mode',
  NewGrid: 'grid',
  NewState: 'state',
  Confirm: 'confirm',
  Dxped: 'dxped',
  Pota: 'pota',
  Sota: 'sota',
  Wanted: 'wanted',
}

export interface DecodeNeeds {
  /** Applicable need categories, ordered by precedence — the micro-icon cluster. */
  cats: NeedCat[]
  /** The need class that should colour the row (highest-precedence NON-icon-only cat),
   * or null. `dxped/pota/sota` never colour a row; `confirm` colours but the caller
   * ranks it below CQ. */
  rowNeed: string | null
}

/**
 * Resolve a decode's need context from its own engine-computed flags plus the live
 * NeedAlerts for that callsign. Pure + side-effect-free; returns empty needs when no
 * alerts are supplied (the Tempo rail / detached panel pass none), so tagging degrades
 * gracefully.
 *
 * `scopes` are the operator's per-type band scopes (Settings ▸ Spots & Alerts). They are
 * needed HERE and not only on the alert set because this feed's entity/grid icons come from
 * the decode's OWN flags — gating the NeedAlerts alone left the GRID icon exactly where the
 * operator reported it. Omit them and nothing is withheld.
 */
export function resolveDecodeNeeds(
  d: DecodeRow,
  band: string,
  alerts: NeedAlert[],
  feedMode = 'Digital',
  scopes?: NeedBandScopes,
): DecodeNeeds {
  const set = new Set<NeedCat>()
  // Decode-native flags (engine-computed against the worked-entity/grid indices). Every
  // decode here is on the surface's own band, so that band answers the scope.
  const mhz = needScopeMhz(band)
  if (d.newDxcc && needScopeOk('dxcc', mhz, scopes)) set.add('entity')
  if (d.newGrid && needScopeOk(isRareGrid(d.gridRarity) ? 'rareGrid' : 'grid', mhz, scopes))
    set.add('grid')
  // Alert-derived tags: `tagsForSurface` owns the band/mode policy (band-agnostic tags
  // apply anywhere; per-band claims need a band match; NewMode/Confirm additionally need
  // the feed's mode class, so a CW need can't be closed on the FT8 feed). It lives in
  // features/needs.ts because the roster and station list need the SAME gate — they were
  // missing it entirely, which is how a CW need painted a MODE chip on a 30m FT8 screen.
  for (const a of alerts) {
    for (const t of tagsForSurface(a, band, feedMode)) {
      const cat = TAG_TO_CAT[t]
      if (cat) set.add(cat)
    }
  }
  const cats = NEED_PRECEDENCE.filter((c) => set.has(c))
  const rowCat = cats.find((c) => !NEED_VISUALS[c].iconOnly)
  return { cats, rowNeed: rowCat ? NEED_VISUALS[rowCat].cls : null }
}

/** The award-grade need classes that out-rank a CQ for row colour (everything except
 * the worked-but-unconfirmed `need-confirm`, which ranks BELOW a CQ). */
export function isAwardNeed(rowNeed: string | null): boolean {
  return rowNeed != null && rowNeed !== 'need-confirm'
}
