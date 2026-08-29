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
  /** Work / call this station (enters QSO answering it). Same positional signature as the
   * cockpit's shared handler, exactly like the roster table's — the card passes the station's
   * GRID so the contact can log with one the DX never sent, and its LAST-HEARD offset in the
   * `freq` slot so RX/TX move onto them like a Band Activity double-click (the engine then
   * applies the Hold-Tx rule). `message`/`snr` stay undefined: there is no clicked decode
   * line behind a roster row. The tier goes last, where the shared handler takes it, and is
   * what routes a Tempo contact to the conversation instead of the FT8 call sequence.
   *
   * It is deliberately the SHARED signature rather than a card-shaped one. #183 was caused by
   * a narrower re-typing here — the card fed an adapter that dropped everything past the
   * tier, so Work started the QSO and left the markers behind while Band Activity and the
   * roster table moved them. Matching the handler leaves nothing in between to drop. */
  onCall: (
    call: string,
    grid?: string,
    message?: string,
    snr?: number,
    freqHz?: number,
    tier?: Tier | null,
  ) => void
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
  // ONE definition of "work this station" for both gestures the card offers — the Work button
  // and the double-click. They were two separate argument lists before, which is how they
  // could have drifted apart; a caller that gets one right and the other wrong is exactly the
  // bug #183 was, one level down.
  const workThisStation = () =>
    onCall(
      station.call,
      station.grid ?? undefined,
      undefined,
      undefined,
      station.freqHz ?? undefined,
      station.tier,
    )
  return (
    <div
      className={`station-card${selected ? ' selected' : ''}${station.worked ? ' worked' : ''}${
        chip ? ` needed need-${chip.cls}` : ''
      }`}
      onDoubleClick={() => workThisStation()}
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
        onClick={() => workThisStation()}
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
