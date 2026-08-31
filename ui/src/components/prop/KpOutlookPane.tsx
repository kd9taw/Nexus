// The three-day planetary-K outlook — the one propagation question a nowcast cannot
// answer: WHEN DOES IT GET BETTER?
//
// Everything else in Connect describes the ionosphere as it is now. This says what
// SWPC expects it to do next, which is what decides whether you sit down tonight or
// wait for Thursday.
//
// ⚠️ MEASURED AND MODELLED ARE DRAWN DIFFERENTLY, ON PURPOSE. SWPC labels every
// sample (`observed` / `estimated` / `predicted`) and only `observed` is a
// measurement — `estimated` is its own fill for a period whose observations are not
// final. Bars past the measured edge are hollow and the boundary is marked, so a
// forecast can never be read as a reading. Same rule the rest of the propagation
// stack follows.
import { useEffect, useState } from 'react'
import { getKpForecast } from '../../api'
import type { KpForecast, KpPoint } from '../../types'
import { t, type MessageKey } from '../../i18n'

/** Storm threshold. Kp 5 is the G1 boundary — below it HF is workable, at or above
 *  it the high bands start closing and the aurora oval comes south. */
const STORM_KP = 5

/** Bars to draw: 24 measured hours behind, the whole forecast ahead. At 3 h a
 *  sample that is 8 back, and the feed carries about 24 forward. */
const LOOKBACK = 8

const isMeasured = (p: KpPoint) => p.kind === 'observed'

/** SWPC's word for how a sample was arrived at, spelled out. A literal per kind
 *  rather than a computed key: a template literal is invisible to the catalog's
 *  orphan check, which would then offer all three to a translator as unused. The
 *  `labelKey` shape is what that check looks for — same idiom as `LAYER_LABEL`. */
const KIND_LABEL: Record<KpPoint['kind'], { labelKey: MessageKey }> = {
  observed: { labelKey: 'connect.kp.kind.observed' },
  estimated: { labelKey: 'connect.kp.kind.estimated' },
  predicted: { labelKey: 'connect.kp.kind.predicted' },
}

/** Kp colour bands, matching the NOAA G-scale the rest of the app uses: quiet,
 *  unsettled/active, then storm. */
function kpTone(kp: number): string {
  if (kp >= STORM_KP) return 'var(--state-bad, #e5484d)'
  if (kp >= 4) return 'var(--state-warn, #f5a524)'
  return 'var(--state-good, #30a46c)'
}

/** "Tue 15z" — the period label. UTC, because that is what the feed and the operator
 *  both work in. */
function slot(unix: number): string {
  const d = new Date(unix * 1000)
  const day = d.toLocaleDateString(undefined, { weekday: 'short', timeZone: 'UTC' })
  return `${day} ${String(d.getUTCHours()).padStart(2, '0')}z`
}

export function KpOutlookPane() {
  const [fc, setFc] = useState<KpForecast | null>(null)
  useEffect(() => {
    let live = true
    const load = () =>
      getKpForecast()
        .then((v) => live && setFc(v))
        .catch(() => {})
    load()
    // Matches the server's 15-min cache; SWPC republishes every 30 min.
    const id = window.setInterval(load, 900_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [])

  // No series at all = we have never had the feed. Say so; a flat line at zero would
  // read as "quiet", which is a forecast we do not have.
  if (!fc || fc.points.length === 0) {
    return <div className="kp-outlook empty">{t('connect.kp.unavailable')}</div>
  }

  const measured = fc.points.filter(isMeasured)
  const ahead = fc.points.filter((p) => !isMeasured(p))
  const bars = [...measured.slice(-LOOKBACK), ...ahead]
  const max = Math.max(STORM_KP + 1, ...bars.map((b) => b.kp))
  const peak = ahead.reduce<KpPoint | null>((a, b) => (!a || b.kp > a.kp ? b : a), null)
  const now = measured.length ? measured[measured.length - 1] : undefined
  // "When does it get bad" is the FIRST crossing ahead, not the worst one.
  const onset = ahead.find((p) => p.kp >= STORM_KP)
  // Only meaningful while it is actually disturbed now.
  const relief = now && now.kp >= STORM_KP ? ahead.find((p) => p.kp < STORM_KP) : undefined

  return (
    <div className="kp-outlook">
      <div className="kp-bars" role="img" aria-label={t('connect.kp.chart.aria')}>
        {bars.map((b) => (
          <div
            key={b.timeUnix}
            className={`kp-bar${isMeasured(b) ? ' measured' : ' modelled'}`}
            style={{ height: `${Math.max(4, (b.kp / max) * 100)}%`, ['--kp-tone' as string]: kpTone(b.kp) }}
            title={t('connect.kp.bar.title', {
              when: slot(b.timeUnix),
              kp: b.kp.toFixed(2),
              kind: t(KIND_LABEL[b.kind].labelKey),
              scale: b.noaaScale ? ` · ${b.noaaScale}` : '',
            })}
          />
        ))}
      </div>
      <div className="kp-lines">
        {now && (
          <div className="kp-line">
            {t('connect.kp.now', { kp: now.kp.toFixed(2), when: slot(now.timeUnix) })}
          </div>
        )}
        {/* The headline: the worst period still ahead. */}
        {peak && (
          <div className="kp-line">
            {t('connect.kp.peak', { kp: peak.kp.toFixed(2), when: slot(peak.timeUnix) })}
          </div>
        )}
        {onset && (
          <div className="kp-line warn">
            {t('connect.kp.onset', { when: slot(onset.timeUnix), kp: STORM_KP })}
          </div>
        )}
        {relief && <div className="kp-line good">{t('connect.kp.relief', { when: slot(relief.timeUnix) })}</div>}
        {!peak && <div className="kp-line">{t('connect.kp.noForward')}</div>}
      </div>
    </div>
  )
}

/** The Basic one-liner: the worst period ahead, or the current value when the feed
 *  carried nothing forward. */
export function kpOutlookLine(fc: KpForecast | null): string {
  if (!fc || fc.points.length === 0) return t('connect.kp.unavailable')
  const ahead = fc.points.filter((p) => !isMeasured(p))
  const peak = ahead.reduce<KpPoint | null>((a, b) => (!a || b.kp > a.kp ? b : a), null)
  if (!peak) return t('connect.kp.noForward')
  return t('connect.kp.peak', { kp: peak.kp.toFixed(2), when: slot(peak.timeUnix) })
}
