// The Passes pane — "be on the radio at 1412Z pointing NW": upcoming amateur-
// satellite passes over the operator's QTH from get_satellites. Self-fetching
// (like the DXpeditions windows poll) on a 60 s cadence while mounted. Chased
// birds (⭐, persisted) sort first; the rest rank by next AOS. The ★/All chip
// filters to the chase set (default ★; zero stars = inert) — a choice the 2-D
// map and globe satellite layers share via ONE surface-scoped key (satChase).
// Honesty: null data (no/stale elements) → the pane renders nothing and
// PaneFrame falls back to the Basic line; elements older than 14 days carry a
// stale badge. Geometry only — a pass says the bird is above your horizon, not
// that its transponder is on — so a bird SatNOGS calls dead is MARKED on its
// row, and a ★ bird the view could not place at all is NAMED under the list
// rather than dropped (an absent bird reads as "no pass this window", which is
// a different and much more encouraging claim than "no current elements").
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The
// words come from the catalog; the bird names, the Z clock times, the compass
// octants, the elevations and the durations are data and stay here.
import { useEffect, useState } from 'react'
import { t } from '../../i18n'
import type { SatView } from '../../types'
import { getSatellites } from '../../api'
import { satBirdHealth, satExcludedHealth } from '../../features/satHealth'
import { elementsMostlyPastLine, elementsPastLineLabel } from '../../features/elementBands'
import {
  filterSatsToChased,
  isSatChased,
  satChaseKeys,
  satFavOnly,
  setSatFavOnly,
  toggleSatChasing,
  SAT_CHASE_EVENT,
  type SatChaseKeys,
} from '../../features/satChase'

/** Compass octant for an azimuth. */
function octant(az: number): string {
  const names = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW']
  return names[Math.round((((az % 360) + 360) % 360) / 45) % 8]
}

function timeLabel(unix: number, now: number): string {
  const mins = Math.round((unix - now) / 60)
  if (mins <= 0) return t('sat.passesPane.now')
  if (mins < 60) return t('sat.passesPane.inMins', { mins })
  const d = new Date(unix * 1000)
  return `${String(d.getUTCHours()).padStart(2, '0')}${String(d.getUTCMinutes()).padStart(2, '0')}Z`
}

/** One plain sentence for the Basic projection (exported for paneFormat). */
export function satPassesLine(sats: SatView | null): string {
  if (!sats) return t('sat.passesPane.noElements')
  const now = Date.now() / 1000
  const next = sats.passes.find((p) => p.losUnix > now)
  if (!next) return t('sat.passesPane.noPasses')
  return t('sat.passesPane.line', {
    name: next.name,
    when: timeLabel(next.aosUnix, now),
    maxEl: Math.round(next.maxElDeg),
    aos: octant(next.aosAzDeg),
    los: octant(next.losAzDeg),
  })
}

export function SatPassesPane() {
  const [sats, setSats] = useState<SatView | null>(null)
  const [keys, setKeys] = useState<SatChaseKeys>(() => satChaseKeys())
  const [favOnly, setFavOnly] = useState<boolean>(() => satFavOnly())
  useEffect(() => {
    let live = true
    const load = () =>
      getSatellites()
        .then((s) => {
          if (!live) return
          setSats(s)
          // Stars flipped in the Satellites section land in storage with no
          // same-window event — re-read on the poll tick so they reflect here
          // without a remount (the map re-reads per draw for the same reason).
          setKeys(satChaseKeys())
        })
        .catch(() => {})
    load()
    const id = window.setInterval(load, 60_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [])
  // Chip flips + star toggles on the OTHER surfaces (map/globe Layers chip,
  // Satellites section) announce themselves — follow instantly, not on the
  // next poll (the poll re-read above stays as the belt to this suspender).
  useEffect(() => {
    const onChange = () => {
      setFavOnly(satFavOnly())
      setKeys(satChaseKeys())
    }
    window.addEventListener(SAT_CHASE_EVENT, onChange)
    return () => window.removeEventListener(SAT_CHASE_EVENT, onChange)
  }, [])

  if (!sats) return null // PaneFrame falls back to the honest Basic line
  const now = Date.now() / 1000
  const upcoming = sats.passes.filter((p) => p.losUnix > now)
  // ★ birds the view could not place at all. STARRED ONLY: the catalog knows
  // hundreds of unplaceable birds and none of them are this operator's
  // problem until they star one.
  const missing = (sats.excluded ?? []).filter((e) => isSatChased(e.name, e.norad, keys))
  // A ★ bird with nothing but an exclusion is exactly the case that used to
  // disappear — the pane returned null and PaneFrame showed the Basic line.
  if (upcoming.length === 0 && missing.length === 0) return null

  // ★/All chip — the choice is shared with the map + globe satellite layers
  // (one surface-scoped key; see satFavOnly). Zero stars = the filter is inert.
  const shown = favOnly ? filterSatsToChased(upcoming, keys) : upcoming

  // Chased birds first (each bird's next pass), then everything by AOS.
  const rows = [...shown].sort((a, b) => {
    const ac = isSatChased(a.name, a.norad, keys) ? 0 : 1
    const bc = isSatChased(b.name, b.norad, keys) ? 0 : 1
    return ac - bc || a.aosUnix - b.aosUnix
  })

  return (
    <section className="sat-pane panel">
      <div className="sat-filterbar">
        <button
          type="button"
          className={`sat-fav-toggle${favOnly ? ' on' : ''}`}
          aria-label={t('sat.passesPane.favFilter.aria')}
          aria-pressed={favOnly}
          title={
            favOnly ? t('sat.passesPane.favFilter.on') : t('sat.passesPane.favFilter.off')
          }
          onClick={() => {
            const next = !favOnly
            setSatFavOnly(next)
            setFavOnly(next)
          }}
        >
          {favOnly ? '★' : t('sat.passesPane.favFilter.all')}
        </button>
      </div>
      {/* Same 14 d line, same set-wide median, as the Satellites chip — this
          pane polls get_satellites itself, so it has to agree with it.
          Failing that, the shape the median cannot see: most of the catalog
          past the line while the typical bird is current. This pane is where
          the operator PLANS, so a set that has quietly gone off belongs here —
          but only a genuinely degraded one. The catalog permanently holds a
          slow-cadence tail back, and a planning pane that always carries a
          warning is a pane nobody reads (the majority test lives in
          elementBands.ts, shared with the Now-Bar lane). */}
      {sats.tleAgeDays > 14 ? (
        <p className="sat-stale" title={t('sat.passesPane.stale.title')}>
          {t('sat.passesPane.stale', { days: Math.round(sats.tleAgeDays) })}
        </p>
      ) : elementsMostlyPastLine(sats) ? (
        <p className="sat-stale" title={t('sat.passesPane.mostlyStale.title')}>
          {t('sat.passesPane.mostlyStale', { label: elementsPastLineLabel(sats) })}
        </p>
      ) : null}
      {rows.length === 0 ? (
        <p className="sat-fav-empty">{t('sat.passesPane.favEmpty')}</p>
      ) : (
      <ul className="sat-list">
        {rows.slice(0, 14).map((p) => {
          const isChased = isSatChased(p.name, p.norad, keys)
          // Schedule rows carry a status but no transmitter answer, so this
          // can report dead/re-entered and never "silent" (satHealth.ts).
          const health = satBirdHealth(p.status)
          return (
            <li key={`${p.name}-${p.aosUnix}`} className={`sat-row${isChased ? ' chased' : ''}`}>
              <button
                type="button"
                className={`wn-chase${isChased ? ' active' : ''}`}
                onClick={() => {
                  toggleSatChasing(p.name, p.norad)
                  setKeys(satChaseKeys())
                }}
                title={isChased ? t('sat.passesPane.chase.on') : t('sat.passesPane.chase.off')}
                aria-pressed={isChased}
              >
                {isChased ? '★' : '☆'}
              </button>
              <span className="sat-name">{p.name}</span>
              {health && (
                <span className={`sat-chip ${health.tone}`} title={health.title}>
                  {health.label}
                </span>
              )}
              <span className="sat-when">{timeLabel(p.aosUnix, now)}</span>
              <span
                className="sat-el"
                title={t('sat.passesPane.peakEl.title', { el: p.maxElDeg.toFixed(0) })}
              >
                {Math.round(p.maxElDeg)}°
              </span>
              <span className="sat-arc" title={t('sat.passesPane.arc.title')}>
                {octant(p.aosAzDeg)}→{octant(p.losAzDeg)}
              </span>
              <span className="sat-dur">
                {t('sat.passesPane.duration', {
                  mins: Math.max(1, Math.round((p.losUnix - p.aosUnix) / 60)),
                })}
              </span>
            </li>
          )
        })}
      </ul>
      )}
      {missing.length > 0 && (
        <ul className="sat-missing">
          {missing.slice(0, 4).map((e) => {
            const h = satExcludedHealth(e.reason)
            return (
              <li key={e.norad} title={h.title}>
                ★ {e.name} — {h.label}
              </li>
            )
          })}
        </ul>
      )}
    </section>
  )
}
