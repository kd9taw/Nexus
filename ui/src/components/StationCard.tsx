// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; a hardcoded one fails CI. What does NOT come from it: the
// callsign, grid, country, distance, bearing and SNR the card prints (data), the `B4` shorthand
// and the `QRZ` button label (invariant tokens — see QRZ_LABEL below and the rule in
// `i18n/index.ts`), and the need-chip words, which live in `features/needVisuals.ts` and are
// migrated with that registry, not here.

import type { NeedTag, Station, Tier } from '../types'
import { openQrzPage } from '../api'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import { azimuthLabel, azimuthTitle, azimuthTo, distanceLabel } from '../grid'
import { useEntityCentroids } from '../features/entityCentroids'
import { useUnits } from '../units'
import { RarityChip } from './RarityChip'
import { NEED_CHIP } from '../features/needVisuals'

interface Props {
  station: Station
  myGrid: string
  currentSlot: number
  selected: boolean
  unread: number
  /** Top award-need tier for this call (null = nothing needed / not resolvable) —
   * drives the row's dominant colour. */
  need: NeedTag | null
  /** EVERY need form for this call — one chip each, matching the decode feed /
   * GridTracker roster (so the roster isn't missing pills the decodes show). */
  needAll: NeedTag[]
  onSelect: (call: string) => void
  /** Work / call this station (enters QSO answering it). */
  onCall: (call: string, tier?: Tier | null) => void
}


/** The callbook's name, printed on the button. A proper noun, not a word to translate. */
const QRZ_LABEL = 'QRZ'

/** Worked-before shorthand — the same two characters on every band and in every language. */
const B4_LABEL = 'B4'

function lastHeardLabel(lastHeardSlot: number, currentSlot: number): string {
  const slots = currentSlot - lastHeardSlot
  if (slots <= 0) return t('roster.card.heard.now')
  if (slots < 60) return t('roster.card.heard.slots', { count: slots })
  return t('roster.card.heard.minutes', { count: Math.round(slots / 4) })
}

export function StationCard({
  station,
  myGrid,
  currentSlot,
  selected,
  unread,
  need,
  needAll,
  onSelect,
  onCall,
}: Props) {
  const units = useUnits()
  const centroids = useEntityCentroids()
  const dist = distanceLabel(myGrid, station.grid, units)
  // Distance still needs a real grid — an entity centroid would claim a precision
  // it does not have on a number the operator reads as a measurement. A BEARING
  // degrades gracefully to "roughly that way" and is marked `~`, so it falls back.
  const az = azimuthTo(myGrid, station.grid, station.country, centroids)
  // Top need drives the row's dominant colour; needAll drives the chips.
  const chip = need ? NEED_CHIP[need] : null
  return (
    <div
      className={`station-card${selected ? ' selected' : ''}${station.worked ? ' worked' : ''}${
        chip ? ` needed need-${chip.cls}` : ''
      }`}
      onDoubleClick={() => onCall(station.call, station.tier)}
      title={t('roster.card.doubleClick', { call: station.call })}
    >
      <button
        type="button"
        className="station-open"
        onClick={() => onSelect(station.call)}
        title={t('roster.card.open', { call: station.call })}
      >
        <span className={`presence-dot ${station.presence}`} aria-hidden />
        <span className="station-main">
          <span className="station-line1">
            <span className="station-call">{station.call}</span>
            {/* One chip per need form (new-DXCC, band, zone, …) — matches the decode
                feed so the roster no longer looks emptier than Band Activity. */}
            {needAll.map((t) => {
              const c = NEED_CHIP[t]
              return c ? (
                <span key={t} className={`need-chip need-${c.cls}`} title={c.title}>
                  {c.short}
                </span>
              ) : null
            })}
            {station.worked && (
              <span
                className={`b4-chip${station.workedBand ? ' b4-band' : ''}`}
                title={
                  station.workedBand
                    ? t('roster.card.b4.sameBand')
                    : t('roster.card.b4.otherBand')
                }
              >
                {B4_LABEL}
              </span>
            )}
            {/* Loud on the PRIMARY line (with need/B4/unread) so an ultra-rare grid
                is unmistakable — the tiny line-2 gem was too easy to miss. */}
            <RarityChip rarity={station.gridRarity} />
            {unread > 0 && <span className="unread-badge">{unread}</span>}
          </span>
          <span className="station-line2">
            {station.country && <span className="station-country">{station.country}</span>}
            {station.country && ' · '}
            {station.grid ?? '—'}
            {dist && <span className="station-dist"> · {dist}</span>}
            {az && (
              <span className="station-bearing" title={azimuthTitle(az, station.country)}>
                {' · '}
                {azimuthLabel(az)}
              </span>
            )}
            <span className="station-heard"> · {lastHeardLabel(station.lastHeardSlot, currentSlot)}</span>
          </span>
        </span>
        <span className={`snr-badge ${snrClass(station.snr)}`}>{fmtSnr(station.snr)}</span>
      </button>
      <button
        type="button"
        className="station-work"
        onClick={() => onCall(station.call, station.tier)}
        title={t('roster.card.work.title', { call: station.call })}
      >
        {t('roster.card.work.label')}
      </button>
      <button
        type="button"
        className="station-qrz"
        onClick={(e) => {
          e.stopPropagation()
          void withErrorToast(
            () => openQrzPage(station.call),
            t('callbook.qrzPage.failed', { call: station.call }),
          )
        }}
        title={t('callbook.qrzPage.title', { call: station.call })}
      >
        {QRZ_LABEL}
      </button>
    </div>
  )
}

function fmtSnr(snr: number): string {
  return `${snr > 0 ? '+' : ''}${snr}`
}

function snrClass(snr: number): string {
  if (snr >= -10) return 'good'
  if (snr >= -18) return 'ok'
  return 'weak'
}
