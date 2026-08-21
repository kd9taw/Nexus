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
 *
 * ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The callsign, the
 * symbol table and code, the digipeater path, the position and grid, the distance, bearing,
 * speed, altitude and every weather reading are data and stay in the code — as do the two
 * service names below, which those services spell for themselves. `units.ts` still chooses
 * metric or imperial for the readings; that is a DISPLAY conversion, not a translation.
 */
import { useEffect, useRef, useState } from 'react'
import { openQrzPage, type AprsStation } from '../api'
import { withErrorToast } from '../toast'
import { t } from '../i18n'
import { fmtTempF, fmtSpeedMph, fmtRainIn, fmtDistanceKm, useUnits, type Units } from '../units'
import { latLonToGrid, bearingDeg, haversineKm, type LatLon } from '../grid'
import {
  CATEGORY_VAR,
  GLYPH_PATHS,
  resolveSymbol,
  symbolCategory,
} from '../aprsSymbols'

/** The two services' own names — a name is a token, not prose. */
const QRZ = 'QRZ'
const APRS_FI = 'aprs.fi'

/** Unit symbols printed beside a reading — knots on the wire, feet from the /A= extension.
 * A unit is a token, and the guard is told so by these constants. */
const KNOTS_UNIT = 'kn'
const FEET_UNIT = 'ft'

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
  // The unit rides inside the message with its number, so a translation can never separate
  // the two.
  if (s < 60) return t('aprs.card.age.secs', { secs: s })
  if (s < 3600) return t('aprs.card.age.mins', { mins: Math.round(s / 60) })
  if (s < 86400) return t('aprs.card.age.hours', { hours: Math.round(s / 3600) })
  return t('aprs.card.age.days', { days: Math.round(s / 86400) })
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
      label: t('aprs.card.source.rf.label'),
      detail: t('aprs.card.source.rf.detail', { age: ageText(st.lastRfUnix, nowSec) }),
    })
  }
  if (st.lastInetUnix != null) {
    out.push({
      label: t('aprs.card.source.inet.label'),
      detail: t('aprs.card.source.inet.detail', { age: ageText(st.lastInetUnix, nowSec) }),
    })
  }
  if (out.length === 0) {
    out.push({
      label: t('aprs.card.source.unknown.label'),
      detail: t('aprs.card.source.unknown.detail'),
    })
  }
  return out
}

/** Was the packet digipeated, or did it arrive direct? The `*` marks a token that repeated it. */
export function pathSummary(path: string[]): string {
  const repeated = path.filter((p) => p.trim().endsWith('*'))
  if (path.length === 0) return t('aprs.card.path.direct')
  if (repeated.length === 0) return t('aprs.card.path.requested', { path: path.join(', ') })
  return t('aprs.card.path.digipeated', { path: repeated.join(', ') })
}

/** Weather readings as label/value rows, omitting anything the station has no sensor for.
 *  APRS transmits °F/mph/inches natively (the wire values are untouched); `units` only
 *  chooses how they display (F4MQS). Pressure is always hPa. */
export function wxRows(
  wx: NonNullable<AprsStation['wx']>,
  units: Units = 'imperial',
): [string, string][] {
  const rows: [string, string][] = []
  if (wx.tempF != null) rows.push([t('aprs.card.wx.temperature'), fmtTempF(wx.tempF, units)])
  if (wx.windDirDeg != null || wx.windMph != null) {
    const dir =
      wx.windDirDeg != null
        ? `${compass(wx.windDirDeg)} ${wx.windDirDeg}°`
        : t('aprs.card.wx.wind.dirUnknown')
    const spd = wx.windMph != null ? fmtSpeedMph(wx.windMph, units) : ''
    rows.push([
      t('aprs.card.wx.wind'),
      spd ? t('aprs.card.wx.wind.atSpeed', { dir, speed: spd }) : dir,
    ])
  }
  if (wx.gustMph != null) rows.push([t('aprs.card.wx.gust'), fmtSpeedMph(wx.gustMph, units)])
  if (wx.humidityPct != null) rows.push([t('aprs.card.wx.humidity'), `${wx.humidityPct}%`])
  if (wx.pressureTenthHpa != null) {
    rows.push([t('aprs.card.wx.pressure'), `${(wx.pressureTenthHpa / 10).toFixed(1)} hPa`])
  }
  if (wx.rain1hIn100 != null) {
    rows.push([t('aprs.card.wx.rain1h'), fmtRainIn(wx.rain1hIn100 / 100, units)])
  }
  if (wx.rain24hIn100 != null) {
    rows.push([t('aprs.card.wx.rain24h'), fmtRainIn(wx.rain24hIn100 / 100, units)])
  }
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
  const units = useUnits()
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
      aria-label={t('aprs.card.aria', { call: st.call })}
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
            {sym.known ? sym.label : t('aprs.card.symbol.unknown')}
          </span>
        </span>
        <button
          type="button"
          className="aprs-card-close"
          onClick={onClose}
          aria-label={t('aprs.card.close')}
        >
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
              <dt>{t('aprs.card.position.label')}</dt>
              <dd>
                {there.lat.toFixed(4)}, {there.lon.toFixed(4)} · {latLonToGrid(there.lat, there.lon)}
              </dd>
            </div>
            {me && (
              <div className="aprs-card-row">
                <dt>{t('aprs.card.fromYou.label')}</dt>
                <dd>
                  {fmtDistanceKm(haversineKm(me, there), units)} {compass(bearingDeg(me, there))} ·{' '}
                  {Math.round(bearingDeg(me, there))}°
                </dd>
              </div>
            )}
          </>
        ) : (
          <div className="aprs-card-row">
            <dt>{t('aprs.card.position.label')}</dt>
            <dd className="aprs-card-none">{t('aprs.card.position.none')}</dd>
          </div>
        )}

        {(st.speedKnots != null || alt != null) && (
          <div className="aprs-card-row">
            <dt>{t('aprs.card.motion.label')}</dt>
            <dd>
              {st.speedKnots != null
                ? `${st.speedKnots} ${KNOTS_UNIT}${st.courseDeg != null ? ` ${compass(st.courseDeg)} ${st.courseDeg}°` : ''}`
                : t('aprs.card.motion.stationary')}
              {alt != null && ` · ${alt.toLocaleString()} ${FEET_UNIT}`}
            </dd>
          </div>
        )}

        {st.text && (
          <div className="aprs-card-row">
            <dt>{t('aprs.card.comment.label')}</dt>
            <dd className="aprs-card-comment">{st.text}</dd>
          </div>
        )}

        <div className="aprs-card-row">
          <dt>{t('aprs.card.path.label')}</dt>
          <dd>{pathSummary(st.path)}</dd>
        </div>

        <div className="aprs-card-row">
          <dt>{t('aprs.card.packets.label')}</dt>
          <dd>
            {t('aprs.card.packets.value', {
              count: st.packets,
              age: ageText(st.firstHeardUnix, nowSec),
            })}
          </dd>
        </div>
      </dl>

      {st.wx && wxRows(st.wx, units).length > 0 && (
        <div className="aprs-card-wx">
          <span className="aprs-card-section">{t('aprs.card.wx.title')}</span>
          <dl className="aprs-card-rows">
            {wxRows(st.wx, units).map(([k, v]) => (
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
            {rawOpen ? t('aprs.card.raw.hide') : t('aprs.card.raw.show')}
          </button>
          {rawOpen && <pre className="aprs-card-rawtext">{st.raw}</pre>}
        </div>
      )}

      <div className="aprs-card-actions">
        <button
          type="button"
          onClick={() =>
            void withErrorToast(
              () => openQrzPage(st.call),
              t('aprs.card.qrz.error', { call: st.call }),
            )
          }
        >
          {QRZ}
        </button>
        {/* aprs.fi is a third-party site (Heikki Hannikainen, OH7LZB); this only opens its page for
            the callsign in the operator's browser — nothing is sent to it from here. */}
        <a
          href={`https://aprs.fi/#!call=${encodeURIComponent(st.call)}`}
          target="_blank"
          rel="noreferrer noopener"
          title={t('aprs.card.aprsfi.title')}
        >
          {APRS_FI}
        </a>
      </div>
    </div>
  )
}
