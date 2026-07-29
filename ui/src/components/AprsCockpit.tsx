import { MapView } from './MapView'
import type { NeedTag, Station } from '../types'
import type { Theme } from '../useTheme'
import { useEffect, useMemo, useRef, useState } from 'react'

import {
  aprsArm,
  aprsSendBeacon,
  aprsSendMessage,
  getAprsHeard,
  getAprsHealth,
  getSettings,
  type AprsHealth,
  type AprsHeard,
} from '../api'
import { bearingDeg, gridToLatLon, haversineKm, type LatLon } from '../grid'

const COMPASS = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW']
function compass(deg: number): string {
  return COMPASS[Math.round(deg / 45) % 8]
}

/** The regional 2 m FM APRS frequencies (MHz) — all AFSK-1200, what this decoder handles. */
const APRS_FREQS: [number, string][] = [
  [144.39, 'N. America'],
  [144.8, 'Europe / Africa'],
  [145.175, 'Australia'],
  [144.575, 'New Zealand'],
  [144.66, 'Japan'],
  [144.93, 'Argentina'],
  [145.57, 'Brazil'],
]

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

/** Below this peak the tap is carrying nothing — even an open squelch on a quiet channel sits
 * well above it. Digital silence from a codec is exactly 0. */
const SILENT_PEAK = 0.001

export type AprsDecodeState = 'off' | 'deaf' | 'listening' | 'unreadable' | 'decoding'

/**
 * Turn decoder health into what the operator should be told.
 *
 * THE BUG THIS EXISTS FOR: an empty APRS screen looked the same whether the app was listening to
 * the wrong sound card, hearing a channel whose every frame failed the checksum, or sitting on a
 * quiet band. Only successfully-decoded frames ever reached the UI, so "nothing here" was the
 * single answer to three completely different questions — and the first two are faults the
 * operator can fix in seconds once told.
 */
export function aprsDecodeStatus(
  health: AprsHealth | null,
  nowSec: number,
): { state: AprsDecodeState; label: string; detail: string } {
  if (!health || !health.armed) {
    return {
      state: 'off',
      label: 'Monitor off',
      detail: 'The APRS decoder is not running. Arm Monitor to decode the RX audio.',
    }
  }
  if (health.audioPeak < SILENT_PEAK) {
    return {
      state: 'deaf',
      label: 'No audio',
      detail:
        'Armed, but no audio is reaching the decoder. Check that Settings → Audio input is the ' +
        'radio (not a microphone or a disconnected device) — what you hear on the speaker does ' +
        'not tell you what the app is capturing.',
    }
  }
  if (health.framesDecoded > 0) {
    const age = health.lastDecodeUnix != null ? Math.max(0, nowSec - health.lastDecodeUnix) : null
    return {
      state: 'decoding',
      label: `${health.framesDecoded} decoded`,
      detail:
        age == null
          ? `${health.framesDecoded} packets decoded.`
          : `${health.framesDecoded} packets decoded, last ${ageLabel(nowSec - age, nowSec)} ago.`,
    }
  }
  if (health.framesSeen > 0) {
    return {
      state: 'unreadable',
      label: `${health.framesSeen} failed CRC`,
      detail:
        `${health.framesSeen} packets were heard but none passed the checksum. The signal is ` +
        'arriving corrupted: check that the rig is on 144.390 in FM, and that the RX audio is ' +
        'not so hot that it is clipping.',
    }
  }
  return {
    state: 'listening',
    label: 'Listening',
    detail: 'Audio is reaching the decoder and no packets have been heard yet — a quiet channel.',
  }
}

/**
 * APRS cockpit — monitor decoded packets and send a position beacon. RX-first: arming starts the
 * AFSK-1200 decoder; a beacon is an explicit, gated one-shot send (never automatic).
 */
export function AprsCockpit({
  active,
  onTune,
  radio,
  onSetTxEnabled,
  theme,
  myGrid = '',
}: {
  active: boolean
  /** Palette for the embedded APRS map. */
  theme: Theme
  /** Operator's grid — centers the map on the station. */
  myGrid?: string
  /** QSY to an APRS dial (MHz): 2 m FM simplex, auto-routing to the 2 m-capable radio. */
  onTune?: (dialMhz: number) => void
  /** Live rig readout (dial/band/mode + TX-enable) — the TopBar's is hidden on this view. */
  radio?: { dialMhz: number; band: string; sideband: string; txEnabled: boolean }
  /** Arm/disarm TX (the TopBar's Enable-Tx is hidden here, so APRS carries its own — otherwise a
   * beacon/message is gated off with no way to turn TX on). */
  onSetTxEnabled?: (on: boolean) => void
}) {
  const [armed, setArmed] = useState(false)
  // One selection shared by the list and the map — clicking either highlights both.
  const [selected, setSelected] = useState<string | null>(null)
  const [freq, setFreq] = useState(144.39)
  const [heard, setHeard] = useState<AprsHeard[]>([])
  const [health, setHealth] = useState<AprsHealth | null>(null)
  const [lat, setLat] = useState('')
  const [lon, setLon] = useState('')
  const [comment, setComment] = useState('Nexus APRS')
  const [symbol, setSymbol] = useState('>')
  // Stable empty inputs for the embedded map (it plots APRS only — no decode
  // stations, spots or needs), so MapView's per-tick projections don't rebuild.
  const noStations = useMemo(() => [] as Station[], [])
  const noNeeds = useMemo(() => new Map<string, NeedTag>(), [])
  const noSelectCall = useMemo(() => () => {}, [])
  // How many heard stations actually carry a position — status and message
  // packets carry none, so "nothing on the map" is a normal state worth naming.
  const positioned = useMemo(
    () => heard.filter((h) => h.lat != null && h.lon != null).length,
    [heard],
  )
  const [path, setPath] = useState('WIDE1-1,WIDE2-1')
  const [msgTo, setMsgTo] = useState('')
  const [msgText, setMsgText] = useState('')
  const [status, setStatus] = useState<string | null>(null)
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  const [me, setMe] = useState<LatLon | null>(null)
  const prefilled = useRef(false)
  const autoTuned = useRef(false)

  // Default to the APRS radio on ENTERING the view: hand off to the 2 m-capable rig, land on the
  // selected APRS frequency in FM. This is the operator's "hitting APRS should default to the 9700"
  // — done once per entry (rising edge of `active`), never on every render, and never keys TX.
  useEffect(() => {
    if (!active) {
      autoTuned.current = false
      return
    }
    if (!autoTuned.current && onTune) {
      autoTuned.current = true
      onTune(freq)
    }
  }, [active, onTune, freq])

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

  // Poll the heard list + decoder health (and tick the age clock) while the cockpit is visible.
  useEffect(() => {
    if (!active) return
    let alive = true
    const tick = () => {
      setNow(Math.floor(Date.now() / 1000))
      void getAprsHeard()
        .then((h) => alive && setHeard(h))
        .catch(() => {})
      void getAprsHealth()
        .then((h) => alive && setHealth(h))
        .catch(() => {})
    }
    tick()
    const id = window.setInterval(tick, 2000)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [active])

  const decode = useMemo(() => aprsDecodeStatus(health, now), [health, now])

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

  const sendMessage = () => {
    const to = msgTo.trim()
    const text = msgText.trim()
    if (!to || !text) {
      setStatus('Enter a callsign and a message first.')
      return
    }
    setStatus('Sending message…')
    void aprsSendMessage(to, text)
      .then(() => {
        setStatus(`Message to ${to.toUpperCase()} queued — keying now.`)
        setMsgText('')
      })
      .catch((e) => setStatus(String(e)))
  }

  // Messages get their OWN chronological list (newest first) — never collapsed by source, so a
  // conversation of several lines from one station all show. Positions are the roster below.
  const messages = useMemo(
    () => heard.filter((h) => h.kind === 'message').slice().reverse(),
    [heard],
  )

  // Collapse the POSITION stream to ONE row per station (latest wins — `heard` is oldest→newest),
  // newest first, with distance + bearing from the operator's grid. Messages are excluded (above).
  const rows = useMemo(() => {
    const bySource = new Map<string, AprsHeard>()
    for (const h of heard) {
      if (h.kind === 'message') continue
      bySource.set(h.source, h)
    }
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
        {onTune && (
          <>
            <select
              className="np-chip aprs-freq"
              value={freq}
              onChange={(e) => {
                // Selecting a frequency retunes the rig immediately (band-picker behavior) — no
                // separate Tune click needed. Switches to the 2 m radio + FM simplex via onTune.
                const f = Number(e.target.value)
                setFreq(f)
                onTune(f)
              }}
              title="APRS frequency by region — selecting one tunes the rig (2 m FM, AFSK-1200)"
            >
              {APRS_FREQS.map(([f, region]) => (
                <option key={f} value={f}>
                  {f.toFixed(3)} · {region}
                </option>
              ))}
            </select>
            <button
              type="button"
              className="np-chip"
              onClick={() => onTune(freq)}
              title="Re-tune the rig to the selected APRS frequency (2 m FM simplex; switches to your 2 m radio)"
            >
              Re-tune
            </button>
          </>
        )}
        {radio && (
          <span
            className="aprs-dial"
            title="The rig's current dial / band / mode (this view hides the top bar's readout)"
          >
            {radio.dialMhz.toFixed(3)} MHz · {radio.band} · {radio.sideband || '—'}
          </span>
        )}
        {onSetTxEnabled && radio && (
          <button
            type="button"
            className={`np-chip${radio.txEnabled ? ' active' : ''}`}
            aria-pressed={radio.txEnabled}
            onClick={() => onSetTxEnabled(!radio.txEnabled)}
            title={
              radio.txEnabled
                ? 'Transmit ENABLED — beacons/messages will go out. Click to disable.'
                : 'Transmit is OFF — enable it before a beacon or message can send.'
            }
          >
            {radio.txEnabled ? 'TX On' : 'TX Off'}
          </button>
        )}
        <button
          type="button"
          className={`np-chip${armed ? ' active' : ''}`}
          aria-pressed={armed}
          onClick={toggleArm}
          title="Arm the APRS decoder on the RX audio"
        >
          {armed ? '● Monitoring' : 'Monitor'}
        </button>
        {/* Decode health. An empty APRS screen used to be one answer to three different
            questions — deaf app, unreadable channel, quiet band — so it never told the
            operator which of the two fixable ones they were looking at. */}
        <span
          className={`aprs-health aprs-health-${decode.state}`}
          role="status"
          title={decode.detail}
        >
          {decode.label}
        </span>
      </div>

      {/* ⭐ APRS IS A GEOGRAPHIC MODE AND HAD NO MAP. Everything lived in one
          vertical stack, so on any real window the controls bunched into the
          top-left and the rest of the section was empty — operator, 2026-07-29:
          "all information is in a small area in the top left, and you still have
          three-quarters of a black box of items."
          The controls and lists become a left rail; the map takes the space they
          were not using. Positions were already in the packets (AprsHeard carries
          lat/lon, course and speed) — nothing new is decoded for this. */}
      <div className="aprs-body">
        <div className="aprs-rail">
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

      <div className="aprs-beacon aprs-message-compose">
        <span className="aprs-beacon-title">Message</span>
        <label>
          To
          <input
            value={msgTo}
            onChange={(e) => setMsgTo(e.target.value)}
            placeholder="callsign"
            size={9}
          />
        </label>
        <label className="aprs-beacon-comment">
          Text
          <input
            value={msgText}
            onChange={(e) => setMsgText(e.target.value)}
            maxLength={67}
            placeholder="up to 67 chars"
            onKeyDown={(e) => e.key === 'Enter' && sendMessage()}
          />
        </label>
        <span className="aprs-msg-count">{msgText.length}/67</span>
        <button type="button" className="np-chip aprs-beacon-send" onClick={sendMessage}>
          Send message
        </button>
      </div>

      {messages.length > 0 && (
        <div className="aprs-messages">
          <span className="aprs-beacon-title">Messages</span>
          <ul className="aprs-msg-list">
            {messages.map((m, i) => (
              <li key={`${m.source}-${m.msgId ?? i}-${m.atUnix}`} className="aprs-msg-row">
                <span className="aprs-age">{ageLabel(m.atUnix, now)}</span>
                <span className="aprs-from">{m.source}</span>
                <span className="aprs-msg-arrow">→</span>
                <span className="aprs-msg-to">{m.addressee ?? '?'}</span>
                {m.msgId && <span className="aprs-msg-id">#{m.msgId}</span>}
                <span className="aprs-msg-text">
                  {m.text.replace(/^→[^:]*:\s*/, '')}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {rows.length === 0 ? (
        <div className="np-empty">{decode.detail}</div>
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
              // Selecting here selects on the map and vice versa — one selection,
              // two views of it. Clicking the same row again clears it.
              <tr
                key={h.source}
                className={selected === h.source ? 'sel' : undefined}
                onClick={() => setSelected(selected === h.source ? null : h.source)}
                title={
                  h.lat != null && h.lon != null
                    ? `Highlight ${h.source} on the map`
                    : `${h.source} reported no position — nothing to highlight`
                }
              >
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
        </div>
        <div className="aprs-map">
          <MapView
            embedded={{ aprs: true }}
            aprs={heard}
            selectedAprs={selected}
            onSelectAprs={setSelected}
            myGrid={myGrid}
            theme={theme}
            stations={noStations}
            prop={null}
            selectedCall={null}
            onSelectCall={noSelectCall}
            needByCall={noNeeds}
          />
          {positioned === 0 && (
            <div className="aprs-map-empty">
              {decode.state === 'decoding'
                ? 'No positions heard yet — status and message packets carry none.'
                : decode.detail}
            </div>
          )}
        </div>
      </div>
    </main>
  )
}
