/**
 * The APRS station detail card — everything known about one station, in one place.
 *
 * Clicking a station used to highlight it and nothing more, which wasted the merged station record
 * sitting behind it. This is that record, rendered: who, what, where, how it reached us, what it
 * said, and what the weather is if it is a weather station.
 *
 * ⭐ THE HONESTY LINE. "Heard" and "reported" are different claims, and the card states them
 * separately with a per-source age: your own receiver last heard this station four minutes ago;
 * APRS-IS twenty seconds ago. Collapsing those into one "last heard" would hide the only fact that
 * says anything about your own antenna.
 *
 * Reads only the station record it is handed — no fetching, no state of its own beyond what is
 * expanded — so the same card serves a click on the map and a click in the list.
 */
import { useEffect, useRef, useState } from 'react'
import { openQrzPage, type AprsStation } from '../api'
import { withErrorToast } from '../toast'
import { latLonToGrid, bearingDeg, haversineKm, type LatLon } from '../grid'
import {
  CATEGORY_VAR,
  GLYPH_PATHS,
  resolveSymbol,
  symbolCategory,
} from '../aprsSymbols'

/** Compass point for a bearing, for the distance line. */
const COMPASS = ['N', 'NNE', 'NE', 'ENE', 'E', 'ESE', 'SE', 'SSE', 'S', 'SSW', 'SW', 'WSW', 'W', 'WNW', 'NW', 'NNW']
const compass = (deg: number) => COMPASS[Math.round((((deg % 360) + 360) % 360) / 22.5) % 16]

/**
 * Altitude in feet from an `/A=nnnnnn` token in the comment.
 *
 * Parsed here rather than in Rust because it is a display affordance over text the UI already has,
 * not a new wire field — the token is a documented six-digit comment extension, and the comment is
 * already carried verbatim. Negative altitudes (below sea level) are legal and rare.
 */
export function altitudeFt(comment: string): number | null {
  // The spec form is exactly six digits. Below sea level appears in the wild as a sign plus five,
  // keeping the field six characters wide — so match those two shapes and nothing looser, or a
  // truncated token would read as a real altitude.
  const m = /\/A=(-\d{5}|\d{6})/.exec(comment)
  if (!m) return null
  const v = Number(m[1])
  return Number.isFinite(v) ? v : null
}

/** "4 min", "20 s", "2 h" — the compact age used throughout the card. */
export function ageText(fromUnix: number, nowSec: number): string {
  const s = Math.max(0, nowSec - fromUnix)
  if (s < 60) return `${s} s`
  if (s < 3600) return `${Math.round(s / 60)} min`
  if (s < 86400) return `${Math.round(s / 3600)} h`
  return `${Math.round(s / 86400)} d`
}

/**
 * The per-source honesty line.
 *
 * Exported and pure so the wording is testable: this is the sentence that keeps "my antenna hears
 * this" from being confused with "a server told me about it".
 */
export function sourceLines(
  st: AprsStation,
  nowSec: number,
): { label: string; detail: string }[] {
  const out: { label: string; detail: string }[] = []
  if (st.lastRfUnix != null) {
    out.push({
      label: 'Heard on RF',
      detail: `your receiver decoded this station ${ageText(st.lastRfUnix, nowSec)} ago`,
    })
  }
  if (st.lastInetUnix != null) {
    out.push({
      label: 'Via APRS-IS',
      detail: `the internet feed reported it ${ageText(st.lastInetUnix, nowSec)} ago`,
    })
  }
  if (out.length === 0) {
    out.push({ label: 'Source unknown', detail: 'no reception recorded for this station' })
  }
  return out
}

/** Was the packet digipeated, or did it arrive direct? The `*` marks a token that repeated it. */
export function pathSummary(path: string[]): string {
  const repeated = path.filter((p) => p.trim().endsWith('*'))
  if (path.length === 0) return 'direct — no digipeaters in the path'
  if (repeated.length === 0) return `direct — requested ${path.join(', ')}, none used`
  return `digipeated via ${repeated.join(', ')}`
}

/** Weather readings as label/value rows, omitting anything the station has no sensor for. */
export function wxRows(wx: NonNullable<AprsStation['wx']>): [string, string][] {
  const rows: [string, string][] = []
  if (wx.tempF != null) rows.push(['Temperature', `${wx.tempF} °F`])
  if (wx.windDirDeg != null || wx.windMph != null) {
    const dir = wx.windDirDeg != null ? `${compass(wx.windDirDeg)} ${wx.windDirDeg}°` : 'unknown'
    const spd = wx.windMph != null ? `${wx.windMph} mph` : ''
    rows.push(['Wind', `${dir}${spd ? ` at ${spd}` : ''}`])
  }
  if (wx.gustMph != null) rows.push(['Gust', `${wx.gustMph} mph`])
  if (wx.humidityPct != null) rows.push(['Humidity', `${wx.humidityPct}%`])
  if (wx.pressureTenthHpa != null) {
    rows.push(['Pressure', `${(wx.pressureTenthHpa / 10).toFixed(1)} hPa`])
  }
  if (wx.rain1hIn100 != null) rows.push(['Rain, last hour', `${(wx.rain1hIn100 / 100).toFixed(2)} in`])
  if (wx.rain24hIn100 != null) rows.push(['Rain, 24 h', `${(wx.rain24hIn100 / 100).toFixed(2)} in`])
  return rows
}

export function AprsStationCard({
  station: st,
  nowSec,
  me,
  onClose,
}: {
  station: AprsStation
  nowSec: number
  /** The operator's own position, for distance and bearing. Null when no grid is set. */
  me: LatLon | null
  onClose: () => void
}) {
  const [rawOpen, setRawOpen] = useState(false)
  const cardRef = useRef<HTMLDivElement | null>(null)
  const sym = resolveSymbol(st.symbolTable, st.symbolCode)
  const colour = `var(${CATEGORY_VAR[symbolCategory(sym.glyph)]})`
  const there = st.lat != null && st.lon != null ? { lat: st.lat, lon: st.lon } : null
  const alt = altitudeFt(st.text)

  // Escape closes, and focus moves to the card on open so a keyboard operator is not left behind on
  // the map canvas. Matches the app's dialog handling. The take is SILENT (preventScroll — no
  // ancestor reveal walk over the map), and where focus came FROM is remembered so close can hand
  // it back — the old code let it fall to <body>, restarting Tab at the top of the document after
  // every card. Never record the card itself as the origin: this effect re-fires per station while
  // the card is already open (clicking through the heard-stations table).
  const restoreRef = useRef<HTMLElement | null>(null)
  useEffect(() => {
    const prev = document.activeElement
    if (prev instanceof HTMLElement && !cardRef.current?.contains(prev)) {
      restoreRef.current = prev
    }
    cardRef.current?.focus({ preventScroll: true })
  }, [st.call])
  // Close restores focus to where it came from. All close paths (✕, Escape, a selection change)
  // unmount the card, so the unmount cleanup covers every one; focusing a since-removed element
  // is a harmless no-op.
  useEffect(
    () => () => {
      restoreRef.current?.focus({ preventScroll: true })
    },
    [],
  )
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        onClose()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  return (
    <div
      className="aprs-card"
      role="dialog"
      aria-label={`Station ${st.call}`}
      tabIndex={-1}
      ref={cardRef}
    >
      <div className="aprs-card-head">
        <span className="aprs-card-glyph" style={{ color: colour }} aria-hidden="true">
          <svg viewBox="0 0 24 24" width="34" height="34" focusable="false">
            <path d={GLYPH_PATHS[sym.glyph]} fill="currentColor" />
          </svg>
          {sym.overlay && <span className="aprs-card-overlay">{sym.overlay}</span>}
        </span>
        <span className="aprs-card-id">
          <strong>{st.call}</strong>
          <span className="aprs-card-symname" style={{ color: colour }}>
            {sym.known ? sym.label : 'Unrecognised symbol'}
          </span>
        </span>
        <button type="button" className="aprs-card-close" onClick={onClose} aria-label="Close">
          ✕
        </button>
      </div>

      <dl className="aprs-card-rows">
        {/* ⭐ Per-source, never collapsed — see the module header. */}
        {sourceLines(st, nowSec).map((s) => (
          <div key={s.label} className="aprs-card-row">
            <dt>{s.label}</dt>
            <dd>{s.detail}</dd>
          </div>
        ))}

        {there ? (
          <>
            <div className="aprs-card-row">
              <dt>Position</dt>
              <dd>
                {there.lat.toFixed(4)}, {there.lon.toFixed(4)} · {latLonToGrid(there.lat, there.lon)}
              </dd>
            </div>
            {me && (
              <div className="aprs-card-row">
                <dt>From you</dt>
                <dd>
                  {Math.round(haversineKm(me, there))} km {compass(bearingDeg(me, there))} ·{' '}
                  {Math.round(bearingDeg(me, there))}°
                </dd>
              </div>
            )}
          </>
        ) : (
          <div className="aprs-card-row">
            <dt>Position</dt>
            <dd className="aprs-card-none">
              none reported — heard, but nothing to plot
            </dd>
          </div>
        )}

        {(st.speedKnots != null || alt != null) && (
          <div className="aprs-card-row">
            <dt>Motion</dt>
            <dd>
              {st.speedKnots != null
                ? `${st.speedKnots} kn${st.courseDeg != null ? ` ${compass(st.courseDeg)} ${st.courseDeg}°` : ''}`
                : 'stationary'}
              {alt != null && ` · ${alt.toLocaleString()} ft`}
            </dd>
          </div>
        )}

        {st.text && (
          <div className="aprs-card-row">
            <dt>Comment</dt>
            <dd className="aprs-card-comment">{st.text}</dd>
          </div>
        )}

        <div className="aprs-card-row">
          <dt>Path</dt>
          <dd>{pathSummary(st.path)}</dd>
        </div>

        <div className="aprs-card-row">
          <dt>Packets</dt>
          <dd>
            {st.packets} since {ageText(st.firstHeardUnix, nowSec)} ago
          </dd>
        </div>
      </dl>

      {st.wx && wxRows(st.wx).length > 0 && (
        <div className="aprs-card-wx">
          <span className="aprs-card-section">Weather</span>
          <dl className="aprs-card-rows">
            {wxRows(st.wx).map(([k, v]) => (
              <div key={k} className="aprs-card-row">
                <dt>{k}</dt>
                <dd>{v}</dd>
              </div>
            ))}
          </dl>
        </div>
      )}

      {st.raw && (
        <div className="aprs-card-raw">
          <button
            type="button"
            className="aprs-card-rawtoggle"
            aria-expanded={rawOpen}
            onClick={() => setRawOpen(!rawOpen)}
          >
            {rawOpen ? '▾' : '▸'} Raw packet
          </button>
          {rawOpen && <pre className="aprs-card-rawtext">{st.raw}</pre>}
        </div>
      )}

      <div className="aprs-card-actions">
        <button type="button" onClick={() => void withErrorToast(() => openQrzPage(st.call), `Could not open ${st.call} on QRZ`)}>
          QRZ
        </button>
        {/* aprs.fi is a third-party site (Heikki Hannikainen, OH7LZB); this only opens its page for
            the callsign in the operator's browser — nothing is sent to it from here. */}
        <a
          href={`https://aprs.fi/#!call=${encodeURIComponent(st.call)}`}
          target="_blank"
          rel="noreferrer noopener"
          title="Open this station on aprs.fi (third-party site) in your browser"
        >
          aprs.fi
        </a>
      </div>
    </div>
  )
}
