// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Its words come from
// the catalog, and — exactly as `features/needVisuals.ts` does — the two badge TABLES resolve
// theirs through GETTERS: they are module constants that BandStrip and BandMap read during
// render, so resolving at import time would freeze whichever locale loaded first. The record
// shape is unchanged, so neither consumer had to move.
import { NEED_CHIP } from '../features/needVisuals'
import { t } from '../i18n'
import type { BeaconKind, NeedTag } from '../types'

/** The programmes' own names — the same four letters in every language. */
const POTA_PROGRAM = 'POTA'
const SOTA_PROGRAM = 'SOTA'

/** Single-letter activity-type badge (POTA park / SOTA summit / DXpedition) — the badge
 * glyph + colour class shared by the band strip and band map. */
export const TYPE_BADGE: Record<'Pota' | 'Sota' | 'Dxped', { ch: string; cls: string; word: string }> = {
  Pota: { ch: 'P', cls: 'type-pota', word: POTA_PROGRAM },
  Sota: { ch: 'S', cls: 'type-sota', word: SOTA_PROGRAM },
  Dxped: {
    ch: '✈',
    cls: 'type-dxped',
    get word() {
      return t('spots.type.dxped')
    },
  },
}

/** Badge for a ONE-WAY transmission — an NCDXF/IARU beacon slot or a W1AW bulletin. These
 * are shown (an audible beacon is genuine propagation evidence) but never scored as a need,
 * so the badge is deliberately OUTLINED and muted rather than carrying a need colour: it
 * must never read as "worth working". Mirrors `propagation::beacons::BeaconKind`. */
export const BEACON_BADGE: Record<BeaconKind, { ch: string; cls: string; word: string }> = {
  ncdxf: {
    ch: 'B',
    cls: 'type-beacon',
    get word() {
      return t('spots.beacon.ncdxf')
    },
  },
  w1aw: {
    ch: 'W',
    cls: 'type-beacon',
    get word() {
      return t('spots.beacon.w1aw')
    },
  },
}

// The need tiers worth explaining in a compact key (award-grade first). `Confirm` last —
// it's the "worked, needs a QSL" grey. Dxped/Pota/Sota are shown as TYPE badges below, not
// here, since they ride as badges independent of the colour.
// Key order mirrors the backend NeedTag::tier() descending (same as NEED_PRECEDENCE), so all
// three surfaces — decode feed, Needed board, and this legend — read as one system.
const LEGEND_NEEDS: NeedTag[] = [
  'Wanted',
  'NewEntity',
  'NewZone',
  'NewState',
  'NewGrid',
  'NewBand',
  'NewMode',
  'Confirm',
]

/**
 * Shared key for the band strip + band map. Two vocabularies in one row:
 *  • COLOUR = the need tier (why the station is worth working) — the same palette the
 *    Needed board uses, so the two views read as one system.
 *  • P / S / ✈ BADGE = the activity type (POTA park / SOTA summit / DXpedition), shown
 *    independently so a park that's ALSO a new band still flags as a park.
 */
export function SpotLegend() {
  return (
    <div className="spot-legend" role="group" aria-label={t('spots.legend.aria')}>
      {LEGEND_NEEDS.map((tag) => (
        <span
          key={tag}
          className={`spot-legend-item need-${NEED_CHIP[tag].cls}`}
          title={NEED_CHIP[tag].title}
        >
          <span className="spot-legend-dot" aria-hidden />
          {NEED_CHIP[tag].short}
        </span>
      ))}
      <span className="spot-legend-div" aria-hidden />
      <span className="spot-legend-item" title={t('spots.legend.pota.title')}>
        <span className="spot-type-badge type-pota" aria-hidden>
          P
        </span>
        {POTA_PROGRAM}
      </span>
      <span className="spot-legend-item" title={t('spots.legend.sota.title')}>
        <span className="spot-type-badge type-sota" aria-hidden>
          S
        </span>
        {SOTA_PROGRAM}
      </span>
      <span className="spot-legend-item" title={t('spots.legend.dxped.title')}>
        <span className="spot-type-badge type-dxped" aria-hidden>
          ✈
        </span>
        {t('spots.legend.dxped.label')}
      </span>
      <span className="spot-legend-item" title={t('spots.legend.beacon.title')}>
        <span className="spot-type-badge type-beacon" aria-hidden>
          B
        </span>
        {t('spots.legend.beacon.label')}
      </span>
    </div>
  )
}
