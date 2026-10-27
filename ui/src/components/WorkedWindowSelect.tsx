// How far back "worked" reaches for Hide worked: the window, the rule that judges a row against
// it, and the picker. The Spots panel and the Call Roster share all three, so one station reads
// as worked, or not, the same way on both.
import type { SpotRow } from '../types'
import { t } from '../i18n'

/**
 * How far back "worked" reaches for Hide worked. `'utcDay'` is since 0000Z on the STATION's
 * clock — the row's own `workedTodayUtc`, never a browser's idea of the date — and a number is
 * plain seconds. The ladder is the operator's (2026-09-17): the UTC day by default, because that
 * is the day POTA credits and the day a spotted station is "already in the log today", and the
 * hour/day/week rungs for events like 13 Colonies and Route 66 where one callsign is on the air
 * all week and a UTC day is far too coarse.
 */
export type WorkedWindow = 'utcDay' | 3600 | 14400 | 86400 | 604800
export const WORKED_WINDOWS: readonly WorkedWindow[] = ['utcDay', 3600, 14400, 86400, 604800]
export function isWorkedWindow(v: unknown): v is WorkedWindow {
  return (WORKED_WINDOWS as readonly unknown[]).includes(v)
}

/**
 * Was this row's station worked within `window`?
 *
 * A row carrying NO flags — an older station, or one that never answered — is worked in no
 * window at all. A pane must never hide a row it cannot judge, which is the same fail-open
 * posture `spotterLocal === false` takes on the Spots panel.
 */
export function workedWithin(
  s: Pick<SpotRow, 'workedAgoSecs' | 'workedTodayUtc'>,
  window: WorkedWindow,
): boolean {
  if (window === 'utcDay') return s.workedTodayUtc === true
  return s.workedAgoSecs != null && s.workedAgoSecs < window
}

interface Props {
  className: string
  value: WorkedWindow | null
  onChange: (window: WorkedWindow | null) => void
  /** Offer "Any time" (`null`) as the first choice. The Call Roster's Hide worked has always
   *  reached back over the whole log; the Spots panel's has not. */
  anyTime?: boolean
}

export function WorkedWindowSelect({ className, value, onChange, anyTime }: Props) {
  return (
    <select
      className={className}
      value={value == null ? '' : String(value)}
      onChange={(e) => {
        const v = e.target.value
        onChange(v === '' ? null : v === 'utcDay' ? 'utcDay' : (Number(v) as WorkedWindow))
      }}
      aria-label={t('spots.filter.workedWindow.aria')}
      title={t('spots.filter.workedWindow.title')}
    >
      {/* The <option> VALUES are the persisted window; only the labels are prose. */}
      {anyTime && <option value="">{t('spots.filter.workedWindow.any')}</option>}
      <option value="utcDay">{t('spots.filter.workedWindow.utcDay')}</option>
      <option value="3600">{t('spots.filter.workedWindow.hour1')}</option>
      <option value="14400">{t('spots.filter.workedWindow.hours4')}</option>
      <option value="86400">{t('spots.filter.workedWindow.hours24')}</option>
      <option value="604800">{t('spots.filter.workedWindow.days7')}</option>
    </select>
  )
}
