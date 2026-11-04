// THE NEEDLE TX METERS (#343): Power, ALC and SWR drawn as an analog meter face, each with a
// PEAK MARKER — Settings ▸ Workspace ▸ TX meters ▸ Needle (`txMeterStyle.ts`). TxMeters draws them
// in place of its PO, ALC and SWR bars; COMP (with Phone's processor controls) and the temperature
// rows stay bars beside them.
//
// The same readings as the bars, never a second source: TxMeters hands over the rows it built
// (each already scaled by `poBar` / `alcBar` / `swrBar`), and the faces' coloured arcs are those
// functions' own zones (`needleZones`, held to them by TxNeedles.test.tsx).
//
// PEAK HOLD, the one thing a needle adds: a fast attack and a slow fall. The marker jumps to any
// reading above it at once, holds it for `PEAK_HOLD_MS`, then falls at `PEAK_FALL_PER_S` of full
// scale a second until a reading catches it (`peakStep` / `peakAt`). Between overs the needle rests
// at zero and the marker and the value hold the LAST OVER'S highest reading — the number an
// operator nudging ALC or reading SWR after a short over wants — until the next over starts.
//
// A READOUT: nothing here keys, gates or sets anything. Drawn in SVG on a display well (dark in
// both themes, styles.css DISPLAY WELLS), so it scales with `--ui-zoom` and the text size like the
// bars' type does; its size is the variant's, in styles.css's TX meters block.
import { useRef } from 'react'
import { t } from '../i18n'

/** The three faces, in the order a rig's meter switch has them. */
export type NeedleKey = 'po' | 'alc' | 'swr'
export const NEEDLE_KEYS: readonly NeedleKey[] = ['po', 'alc', 'swr']

type Zone = 'ok' | 'warn' | 'hot' | 'unknown'

/** One meter as TxMeters built it: a bar fraction, the formatted value, and its zone. */
export interface NeedleRow {
  key: NeedleKey
  label: string
  title: string
  bar: { frac: number; value: string; zone: Zone } | null
}

/** How long the peak marker holds a reading before it starts to fall. */
export const PEAK_HOLD_MS = 1000
/** How fast it falls once the hold is over, as a share of full scale per second. */
export const PEAK_FALL_PER_S = 0.5

/** A held peak: the bar fraction, when it was caught, and the value as the meter printed it. */
export interface Peak {
  frac: number
  at: number
  value: string
}

/** Where the marker stands at `now`: the held reading through the hold, then falling. */
export function peakAt(p: Peak | null, now: number): number {
  if (!p) return 0
  const falling = now - p.at - PEAK_HOLD_MS
  return falling <= 0 ? p.frac : Math.max(0, p.frac - (PEAK_FALL_PER_S * falling) / 1000)
}

/** The fast attack: a reading at or above where the marker stands takes it, and restarts the hold. */
export function peakStep(p: Peak | null, frac: number, value: string, now: number): Peak {
  return p == null || frac >= peakAt(p, now) ? { frac, at: now, value } : p
}

/** The coloured arcs on a face, as [from, to, zone] in bar fractions — the bar functions' own
 *  thresholds: SWR amber from 1.5:1 and red from 2:1 (`swrBar`, 1:1 → 0, 3:1 → full), none at all
 *  where the rig's SWR scale is unverified (TxMeters' `SWR?`); ALC amber from 80 % of the radio's
 *  ALC zone and red from 95 % of it (`alcBar`, `zoneTop` the radio's `txAlcZone`); power none. */
export function needleZones(key: NeedleKey, opts: { swrVerified: boolean; alcZoneTop: number }): [number, number, Zone][] {
  if (key === 'swr') return opts.swrVerified ? [[0.25, 0.5, 'warn'], [0.5, 1, 'hot']] : []
  if (key === 'alc') {
    const top = opts.alcZoneTop > 0 ? opts.alcZoneTop : 1
    return [[Math.min(1, 0.8 * top), Math.min(1, 0.95 * top), 'warn'], [Math.min(1, 0.95 * top), 1, 'hot']]
  }
  return []
}

const ZONE_INK: Record<Zone, string> = {
  ok: 'var(--state-good)',
  warn: 'var(--alert-warning)',
  hot: 'var(--state-weak)',
  unknown: 'var(--state-pending)',
}

// THE DIAL, in viewBox units: a window over a shallow arc in its top half, whose pivot sits BELOW the
// window, as a panel meter's does. The face's bottom band (styles.css), which carries the meter's name
// and its value, lies over the lower half like a meter's pivot cover: the needle runs in under it, so
// no reading is ever drawn under the needle, and the face is no taller than the dial.
const W = 100
const H = 38
const CX = 50
const CY = 63
const R = 60
/** The needle's swing either side of upright, in degrees. */
const SWING = 40

/** The needle's angle from upright for a bar fraction, in degrees. */
export function needleAngle(frac: number): number {
  return -SWING + 2 * SWING * Math.max(0, Math.min(1, frac))
}
function at(r: number, frac: number): [number, number] {
  const a = (needleAngle(frac) * Math.PI) / 180
  return [CX + r * Math.sin(a), CY - r * Math.cos(a)]
}
function arc(r: number, f0: number, f1: number): string {
  const [x0, y0] = at(r, f0)
  const [x1, y1] = at(r, f1)
  return `M${x0.toFixed(2)} ${y0.toFixed(2)}A${r} ${r} 0 0 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`
}
function radial(r0: number, r1: number, frac: number): string {
  const [x0, y0] = at(r0, frac)
  const [x1, y1] = at(r1, frac)
  return `M${x0.toFixed(2)} ${y0.toFixed(2)}L${x1.toFixed(2)} ${y1.toFixed(2)}`
}
const TICKS = [0, 0.25, 0.5, 0.75, 1]

export function TxNeedles({
  rows,
  live,
  swrVerified,
  alcZoneTop,
  unreported,
}: {
  /** PO, ALC and SWR as TxMeters built them from this poll (not its held copy). */
  rows: NeedleRow[]
  /** On the air with a reading: the needle follows. Otherwise it rests and the marker holds. */
  live: boolean
  swrVerified: boolean
  alcZoneTop: number
  /** Meters the rig stayed silent on through a whole over (TxMeters' ⊘ verdict, which it owns). */
  unreported: readonly NeedleKey[]
}) {
  // Refs, as TxMeters' own memory is: the poll re-renders us, and holding a peak must never itself
  // cause a render.
  const peaks = useRef(new Map<NeedleKey, Peak>())
  const overMax = useRef(new Map<NeedleKey, Peak>())
  const wasLive = useRef(false)
  const now = Date.now()
  if (live && !wasLive.current) {
    // A new over: the last over's peaks have said what they had to say.
    peaks.current.clear()
    overMax.current.clear()
  }
  wasLive.current = live
  if (live) {
    for (const r of rows) {
      if (!r.bar) continue
      peaks.current.set(r.key, peakStep(peaks.current.get(r.key) ?? null, r.bar.frac, r.bar.value, now))
      const m = overMax.current.get(r.key)
      if (!m || r.bar.frac > m.frac) overMax.current.set(r.key, { frac: r.bar.frac, at: now, value: r.bar.value })
    }
  }

  const peakTitle = t('meters.tx.needle.peak.title')
  return (
    <div className="ph-txneedles">
      {NEEDLE_KEYS.map((key) => {
        const r = rows.find((x) => x.key === key)
        if (!r) return null
        const held = live ? peaks.current.get(key) ?? null : overMax.current.get(key) ?? null
        const peak = live ? peakAt(held, now) : held?.frac ?? 0
        const needle = live ? r.bar?.frac ?? 0 : 0
        const value = live ? r.bar?.value ?? '—' : held?.value ?? '—'
        return (
          <div
            key={key}
            className="ph-txneedle"
            data-meter={key}
            title={`${r.title}\n${peakTitle}`}
          >
            <div className="ph-txneedle-face well">
              <div className="ph-txneedle-dial">
                <svg className="ph-txneedle-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="xMidYMax meet" aria-hidden="true">
                  <path className="ph-txneedle-scale" d={arc(R, 0, 1)} />
                  {needleZones(key, { swrVerified, alcZoneTop }).map(([f0, f1, zone]) => (
                    <path
                      key={zone}
                      className="ph-txneedle-zone"
                      data-zone={zone}
                      data-from={f0}
                      data-to={f1}
                      d={arc(R + 1.5, f0, f1)}
                      style={{ stroke: ZONE_INK[zone] }}
                    />
                  ))}
                  {TICKS.map((f) => (
                    <path key={f} className="ph-txneedle-tick" d={radial(R - 4, R + 0.5, f)} />
                  ))}
                  {held && <path className="ph-txneedle-peak" data-frac={peak.toFixed(3)} d={radial(R - 7, R + 2.5, peak)} />}
                  {/* Turned by the SVG attribute, not a CSS transform: the attribute's pivot is
                      read alike by every webview Nexus ships in (WebView2, WebKitGTK, WKWebView). */}
                  <line
                    className="ph-txneedle-needle"
                    data-frac={needle.toFixed(3)}
                    x1={CX}
                    y1={CY}
                    x2={CX}
                    y2={CY - R - 2}
                    transform={`rotate(${needleAngle(needle).toFixed(2)} ${CX} ${CY})`}
                  />
                </svg>
              </div>
              <div className="ph-txneedle-band">
                <span className="ph-txneedle-label">{r.label}</span>
                {unreported.includes(key) ? (
                  <span className="ph-unavail" role="note" title={t('phone.unavail.meter', { meter: r.label })}>
                    {/* The glyph alone on the face (the band has no room for the words); the words
                        are its title and what a screen reader reads. */}
                    <span aria-hidden="true">⊘</span>
                    <span className="sr-only">{t('phone.unavail.mark')}</span>
                  </span>
                ) : (
                  <span className="ph-txneedle-value">{value}</span>
                )}
              </div>
            </div>
          </div>
        )
      })}
    </div>
  )
}
