// The Passes pane — "be on the radio at 1412Z pointing NW": upcoming amateur-
// satellite passes over the operator's QTH from get_satellites. Self-fetching
// (like the DXpeditions windows poll) on a 60 s cadence while mounted. Chased
// birds (⭐, persisted) sort first; the rest rank by next AOS. The ★/All chip
// filters to the chase set (default ★; zero stars = inert) — a choice the 2-D
// map and globe satellite layers share via ONE surface-scoped key (satChase).
// Honesty: null data (no/stale elements) → the pane renders nothing and
// PaneFrame falls back to the Basic line; elements older than 14 days carry a
// stale badge. Geometry only — a pass says the bird is above your horizon, not
// that its transponder is on.
import { useEffect, useState } from 'react'
import type { SatView } from '../../types'
import { getSatellites } from '../../api'
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
  if (mins <= 0) return 'now'
  if (mins < 60) return `in ${mins} min`
  const d = new Date(unix * 1000)
  return `${String(d.getUTCHours()).padStart(2, '0')}${String(d.getUTCMinutes()).padStart(2, '0')}Z`
}

/** One plain sentence for the Basic projection (exported for paneFormat). */
export function satPassesLine(sats: SatView | null): string {
  if (!sats) return 'No orbital elements yet — satellite data loads once online.'
  const now = Date.now() / 1000
  const next = sats.passes.find((p) => p.losUnix > now)
  if (!next) return 'No passes over your QTH in the next 24 h (set your grid in Settings?).'
  return `Next: ${next.name} ${timeLabel(next.aosUnix, now)}, max ${Math.round(next.maxElDeg)}° ${octant(next.aosAzDeg)}→${octant(next.losAzDeg)}.`
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
  if (upcoming.length === 0) return null

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
          aria-label="Filter to ★ birds"
          aria-pressed={favOnly}
          title={
            favOnly
              ? 'Showing your ★ birds (map + globe follow) — click to show all satellites'
              : 'Showing all satellites — click to show only your ★ birds (map + globe follow)'
          }
          onClick={() => {
            const next = !favOnly
            setSatFavOnly(next)
            setFavOnly(next)
          }}
        >
          {favOnly ? '★' : 'All'}
        </button>
      </div>
      {sats.tleAgeDays > 14 && (
        <p className="sat-stale" title="Orbital elements decay; pass times drift as they age">
          stale elements ({Math.round(sats.tleAgeDays)} d) — times are approximate
        </p>
      )}
      {rows.length === 0 ? (
        <p className="sat-fav-empty">No passes for your ★ birds in the next 24 h.</p>
      ) : (
      <ul className="sat-list">
        {rows.slice(0, 14).map((p) => {
          const isChased = isSatChased(p.name, p.norad, keys)
          return (
            <li key={`${p.name}-${p.aosUnix}`} className={`sat-row${isChased ? ' chased' : ''}`}>
              <button
                type="button"
                className={`wn-chase${isChased ? ' active' : ''}`}
                onClick={() => {
                  toggleSatChasing(p.name, p.norad)
                  setKeys(satChaseKeys())
                }}
                title={isChased ? 'Chasing — sorts first, footprint ring on the map. Click to stop.' : 'Chase this bird — sort its passes first + draw its footprint on the map'}
                aria-pressed={isChased}
              >
                {isChased ? '★' : '☆'}
              </button>
              <span className="sat-name">{p.name}</span>
              <span className="sat-when">{timeLabel(p.aosUnix, now)}</span>
              <span
                className="sat-el"
                title={`Peak elevation ${p.maxElDeg.toFixed(0)}° — higher = longer, stronger pass`}
              >
                {Math.round(p.maxElDeg)}°
              </span>
              <span className="sat-arc" title="Rise → set compass directions">
                {octant(p.aosAzDeg)}→{octant(p.losAzDeg)}
              </span>
              <span className="sat-dur">
                {Math.max(1, Math.round((p.losUnix - p.aosUnix) / 60))} min
              </span>
            </li>
          )
        })}
      </ul>
      )}
    </section>
  )
}
