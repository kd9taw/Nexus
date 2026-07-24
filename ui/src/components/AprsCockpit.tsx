import { useEffect, useMemo, useRef, useState } from 'react'

import { aprsArm, aprsSendBeacon, getAprsHeard, getSettings, type AprsHeard } from '../api'
import { bearingDeg, gridToLatLon, haversineKm, type LatLon } from '../grid'

const COMPASS = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW']
function compass(deg: number): string {
  return COMPASS[Math.round(deg / 45) % 8]
}

/** Common APRS symbols (primary table `/`): [code, label]. */
const SYMBOLS: [string, string][] = [
  ['>', 'Car'],
  ['-', 'House'],
  ['[', 'Person'],
  ['b', 'Bicycle'],
  ['j', 'Jeep'],
  ['<', 'Motorcycle'],
  ['k', 'Truck'],
  ['.', 'Dot'],
]

function ageLabel(atUnix: number, nowSec: number): string {
  const s = Math.max(0, nowSec - atUnix)
  if (s < 60) return `${s}s`
  if (s < 3600) return `${Math.floor(s / 60)}m`
  return `${Math.floor(s / 3600)}h`
}

/**
 * APRS cockpit — monitor decoded packets and send a position beacon. RX-first: arming starts the
 * AFSK-1200 decoder; a beacon is an explicit, gated one-shot send (never automatic).
 */
export function AprsCockpit({ active }: { active: boolean }) {
  const [armed, setArmed] = useState(false)
  const [heard, setHeard] = useState<AprsHeard[]>([])
  const [lat, setLat] = useState('')
  const [lon, setLon] = useState('')
  const [comment, setComment] = useState('Nexus APRS')
  const [symbol, setSymbol] = useState('>')
  const [path, setPath] = useState('WIDE1-1,WIDE2-1')
  const [status, setStatus] = useState<string | null>(null)
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  const [me, setMe] = useState<LatLon | null>(null)
  const prefilled = useRef(false)

  // Prefill the beacon lat/lon from the operator's grid (and remember it for distance/bearing), once.
  useEffect(() => {
    if (prefilled.current) return
    prefilled.current = true
    void getSettings()
      .then((s) => {
        const ll = gridToLatLon(s.mygrid || '')
        if (ll) {
          setLat(ll.lat.toFixed(4))
          setLon(ll.lon.toFixed(4))
          setMe(ll)
        }
      })
      .catch(() => {})
  }, [])

  // Poll the heard list (and tick the age clock) while the cockpit is visible.
  useEffect(() => {
    if (!active) return
    let alive = true
    const tick = () => {
      setNow(Math.floor(Date.now() / 1000))
      void getAprsHeard()
        .then((h) => alive && setHeard(h))
        .catch(() => {})
    }
    tick()
    const id = window.setInterval(tick, 2000)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [active])

  const toggleArm = () => {
    const next = !armed
    setArmed(next)
    void aprsArm(next)
      .then(setHeard)
      .catch((e) => setStatus(String(e)))
  }

  const sendBeacon = () => {
    const la = Number.parseFloat(lat)
    const lo = Number.parseFloat(lon)
    if (!Number.isFinite(la) || !Number.isFinite(lo)) {
      setStatus('Enter a valid latitude and longitude first.')
      return
    }
    const digis = path
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
    setStatus('Sending beacon…')
    void aprsSendBeacon(la, lo, '/', symbol, comment, digis)
      .then(() => setStatus('Beacon queued — keying now.'))
      .catch((e) => setStatus(String(e)))
  }

  // Collapse the packet stream to ONE row per station (latest packet wins — `heard` is
  // oldest→newest), newest first, with distance + bearing from the operator's grid.
  const rows = useMemo(() => {
    const bySource = new Map<string, AprsHeard>()
    for (const h of heard) bySource.set(h.source, h)
    return [...bySource.values()]
      .sort((a, b) => b.atUnix - a.atUnix)
      .map((h) => {
        const hasPos = h.lat != null && h.lon != null
        const there = hasPos ? { lat: h.lat as number, lon: h.lon as number } : null
        return {
          h,
          dist: me && there ? haversineKm(me, there) : null,
          brg: me && there ? bearingDeg(me, there) : null,
        }
      })
  }, [heard, me])

  return (
    <main className="layout single needed-panel aprs-cockpit">
      <div className="np-head">
        <h2>APRS</h2>
        <span className="np-count">{rows.length}</span>
        {heard.length !== rows.length && (
          <span className="np-count np-count-filtered">{heard.length} pkts</span>
        )}
        <span className="np-hint">AFSK-1200 packet — decode positions/messages, send a beacon</span>
        <button
          type="button"
          className={`np-chip${armed ? ' active' : ''}`}
          aria-pressed={armed}
          onClick={toggleArm}
          title="Arm the APRS decoder on the RX audio (144.390 FM in North America)"
        >
          {armed ? '● Monitoring' : 'Monitor'}
        </button>
      </div>

      <div className="aprs-beacon">
        <span className="aprs-beacon-title">Position beacon</span>
        <label>
          Lat
          <input value={lat} onChange={(e) => setLat(e.target.value)} inputMode="decimal" size={9} />
        </label>
        <label>
          Lon
          <input value={lon} onChange={(e) => setLon(e.target.value)} inputMode="decimal" size={9} />
        </label>
        <label>
          Symbol
          <select value={symbol} onChange={(e) => setSymbol(e.target.value)}>
            {SYMBOLS.map(([code, name]) => (
              <option key={code} value={code}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label className="aprs-beacon-comment">
          Comment
          <input value={comment} onChange={(e) => setComment(e.target.value)} maxLength={43} />
        </label>
        <label>
          Path
          <input value={path} onChange={(e) => setPath(e.target.value)} size={14} />
        </label>
        <button type="button" className="np-chip aprs-beacon-send" onClick={sendBeacon}>
          Send beacon
        </button>
        {status && <span className="aprs-status">{status}</span>}
      </div>

      {rows.length === 0 ? (
        <div className="np-empty">
          {armed ? 'Listening… decoded packets will appear here.' : 'Monitor is off — arm it to decode APRS.'}
        </div>
      ) : (
        <table className="aprs-table">
          <thead>
            <tr>
              <th>Age</th>
              <th>From</th>
              <th>Type</th>
              <th>Position</th>
              <th>Dist</th>
              <th>Info</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ h, dist, brg }) => (
              <tr key={h.source}>
                <td className="aprs-age">{ageLabel(h.atUnix, now)}</td>
                <td className="aprs-from">{h.source}</td>
                <td className={`aprs-kind aprs-kind-${h.kind}`}>{h.kind}</td>
                <td className="aprs-pos">
                  {h.lat != null && h.lon != null
                    ? `${h.lat.toFixed(4)}, ${h.lon.toFixed(4)}${
                        h.speedKnots ? ` · ${h.speedKnots}kt ${h.courseDeg}°` : ''
                      }`
                    : '—'}
                </td>
                <td className="aprs-dist">
                  {dist != null ? `${Math.round(dist)} km ${brg != null ? compass(brg) : ''}` : ''}
                </td>
                <td className="aprs-info">{h.text}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </main>
  )
}
