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
// THE LARGE FACE (`large`, the TX meters pane, TxMeters `TxMetersPane`): the same reading and the same
// dial, but the whole face is one drawing, its scale's numbers, finer ticks, name and value in the
// viewBox, so all of it grows with the pane and nothing can ever be ellipsized. THE DIAL IS HALF the face's
// dial of 1.24.0 (operator, 1.24.0: "the dials ... are way too large, reduce them by 50%", and the pane gives
// the room back): it is drawn at LARGE_DIAL, standing on the band, the band, the name and the value keep
// their size in the drawing, and the drawing ends at the top of the half-size dial. So the face is 35.5
// units high where 1.24.0's was 59, and the pane's stock height is that much less (its weight in each
// cockpit, and its low floor `--txmeters-pane-floor` in cockpit-panes.css): on the same screen the drawing
// stands at 1.24.0's scale, the dial half 1.24.0's and the name and value as large as 1.24.0's.
//
// A READOUT: nothing here keys, gates or sets anything. Drawn in SVG on a display well (dark in
// both themes, styles.css DISPLAY WELLS), so it scales with `--ui-zoom` and the text size like the
// bars' type does; its size is its host's, in styles.css's NEEDLE METERS block.
import { useLayoutEffect, useRef, useState, type CSSProperties } from 'react'
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

// THE DIAL, in viewBox units: a window over a shallow arc, the box the scale itself needs, whose pivot
// sits BELOW the window, as a panel meter's does. So the arc fills a strip's short wide dial, and in a
// taller one (narrower, or drawn whole: styles.css) `xMidYMin` keeps it at the top and the needle runs
// on down what is left of the box, to the band that carries the meter's name and value.
const W = 100
const H = 20
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
/** The large face's finer ticks, between the numbered ones. */
const FINE_TICKS = Array.from({ length: 21 }, (_, i) => i / 20).filter((f) => !TICKS.includes(f))
// The large face's drawing, in the same units as the dial: the band with the name and value at the
// foot, and the dial standing on it, drawn at LARGE_DIAL about the middle of the band's top edge so its
// needle still runs in beneath the band. The drawing starts at the top of that dial: 1.24.0's full-size
// dial began at y −9, room above the arc for the scale's numbers, so the half-size one begins halfway
// from there to the band, and nothing is left above it.
const LARGE_BAND_TOP = 38
const LARGE_BOTTOM = 50
/** The dial's size in the face, against 1.24.0's dial: half as wide and half as high. */
export const LARGE_DIAL = 0.5
const LARGE_TOP = LARGE_BAND_TOP - (LARGE_BAND_TOP + 9) * LARGE_DIAL
const LARGE_ASPECT = W / (LARGE_BOTTOM - LARGE_TOP)
/** A row of large faces never goes under this: `.txmeters-pane`'s track floor (cockpit-panes.css), which
 *  TxMetersPane.test.tsx holds to it. About 1.24.0's 7em times the face's height against 1.24.0's
 *  (35.5 / 59, 4.21em), so where a stacking cockpit gives the pane its floor the drawing stands at 1.24.0's
 *  scale, a hair over. */
export const LARGE_FACE_FLOOR_EM = 4.3

/** How many columns draw the three large faces biggest in this much room: the one choice CSS cannot make,
 *  since wrapping looks at the width alone, and put SWR under a scroll in a short wide pane. `gap` is
 *  the space between faces. `floor` is the least a row may be high: a shape whose rows would not fit it is
 *  no choice, since it would scroll SWR away where a row of three keeps all of them in view. A box not
 *  measured yet has all three in a row. */
export function largeFaceColumns(width: number, height: number, gap = 4, floor = 0): 1 | 2 | 3 {
  if (!(width > 0) || !(height > 0)) return 3
  const rowsAt = (cols: number) => Math.ceil(3 / cols)
  const fits = (cols: number) => rowsAt(cols) * floor + gap * (rowsAt(cols) - 1) <= height
  const faceWidthAt = (cols: number) => {
    const rows = rowsAt(cols)
    return Math.min((width - gap * (cols - 1)) / cols, ((height - gap * (rows - 1)) / rows) * LARGE_ASPECT)
  }
  // The fewest rows wins a tie, so all three stay side by side wherever a face is no bigger for it.
  return ([3, 2, 1] as const).reduce((best, cols) => (fits(cols) && faceWidthAt(cols) > faceWidthAt(best) + 0.5 ? cols : best))
}

/** The columns for the box it is given, followed as the pane is dragged. A keep-alive host reports 0×0
 *  while hidden: that leaves the last answer alone (`largeFaceColumns` would say 3, and the face would
 *  jump on the way back). */
function useLargeFaceColumns(on: boolean) {
  const ref = useRef<HTMLDivElement>(null)
  const [cols, setCols] = useState<1 | 2 | 3>(3)
  useLayoutEffect(() => {
    const el = ref.current
    if (!on || !el) return
    const measure = () => {
      if (el.clientWidth > 0 && el.clientHeight > 0) {
        const floor = LARGE_FACE_FLOOR_EM * parseFloat(getComputedStyle(el).fontSize)
        setCols(largeFaceColumns(el.clientWidth, el.clientHeight, undefined, floor))
      }
    }
    measure()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [on])
  return { ref, cols }
}

/** The numbers on a large face, at the five numbered ticks: the inverse of the bar functions that
 *  place the needle (TxMeters' `poBar`, `alcBar`, `swrBar`, held to them by TxMetersPane.test.tsx). None on
 *  an SWR scale Nexus cannot stand behind: TxMeters' `SWR?` says so of the reading, and numbers on the
 *  scale would claim what its missing colours decline to. */
export function needleScale(key: NeedleKey, opts: { ratedWatts: number; swrVerified: boolean }): { frac: number; text: string }[] {
  if (key === 'swr' && !opts.swrVerified) return []
  const full = opts.ratedWatts > 0 ? opts.ratedWatts : 100
  const at = (f: number): number => (key === 'swr' ? 1 + 2 * f : key === 'alc' ? 100 * f : full * f)
  return TICKS.map((frac) => ({ frac, text: String(Math.round(at(frac) * 100) / 100) }))
}

export function TxNeedles({
  rows,
  live,
  swrVerified,
  alcZoneTop,
  unreported,
  large,
}: {
  /** PO, ALC and SWR as TxMeters built them from this poll (not its held copy). */
  rows: NeedleRow[]
  /** On the air with a reading: the needle follows. Otherwise it rests and the marker holds. */
  live: boolean
  swrVerified: boolean
  alcZoneTop: number
  /** Meters the rig stayed silent on through a whole over (TxMeters' ⊘ verdict, which it owns). */
  unreported: readonly NeedleKey[]
  /** The large face, as the pane draws it, with the radio's rated output for the power scale's numbers. */
  large?: { ratedWatts: number }
}) {
  // Refs, as TxMeters' own memory is: the poll re-renders us, and holding a peak must never itself
  // cause a render.
  const largeBox = useLargeFaceColumns(large != null)
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
  if (large) {
    return (
      <div
        ref={largeBox.ref}
        className={`txmeters-pane${live ? '' : ' txmeters-pane-idle'}`}
        role="group"
        aria-label={t('meters.tx.aria')}
        style={{ '--txmeters-pane-cols': largeBox.cols } as CSSProperties}
      >
        {NEEDLE_KEYS.map((key) => {
          const r = rows.find((x) => x.key === key)
          if (!r) return null
          const held = live ? peaks.current.get(key) ?? null : overMax.current.get(key) ?? null
          const peak = live ? peakAt(held, now) : held?.frac ?? 0
          const needle = live ? r.bar?.frac ?? 0 : 0
          const silent = unreported.includes(key)
          const value = live ? r.bar?.value ?? '—' : held?.value ?? '—'
          return (
            <div key={key} className="txmeters-pane-meter ph-txneedle-face well" data-meter={key} title={`${r.title}\n${peakTitle}`}>
              <div className="ph-txneedle-dial">
                <svg
                  className="ph-txneedle-svg"
                  viewBox={`0 ${LARGE_TOP} ${W} ${LARGE_BOTTOM - LARGE_TOP}`}
                  preserveAspectRatio="xMidYMax meet"
                  role="img"
                  aria-label={`${r.label} ${silent ? t('phone.unavail.mark') : value}`}
                >
                  {/* The dial, at LARGE_DIAL about the middle of the band's top edge. */}
                  <g className="txmeters-pane-dial" transform={`translate(${CX * (1 - LARGE_DIAL)} ${LARGE_BAND_TOP * (1 - LARGE_DIAL)}) scale(${LARGE_DIAL})`}>
                  <path className="ph-txneedle-scale" d={arc(R, 0, 1)} />
                  {needleZones(key, { swrVerified, alcZoneTop }).map(([f0, f1, zone]) => (
                    <path key={zone} className="ph-txneedle-zone" data-zone={zone} data-from={f0} data-to={f1} d={arc(R + 1.5, f0, f1)} style={{ stroke: ZONE_INK[zone] }} />
                  ))}
                  {FINE_TICKS.map((f) => (
                    <path key={f} className="ph-txneedle-tick txmeters-pane-finetick" d={radial(R - 2, R + 0.5, f)} />
                  ))}
                  {TICKS.map((f) => (
                    <path key={f} className="ph-txneedle-tick" d={radial(R - 4, R + 0.5, f)} />
                  ))}
                  {needleScale(key, { ratedWatts: large.ratedWatts, swrVerified }).map(({ frac, text }) => {
                    const [x, y] = at(R + 4.5, frac)
                    return (
                      <text key={frac} className="txmeters-pane-scale" x={x.toFixed(2)} y={y.toFixed(2)} textAnchor="middle">
                        {text}
                      </text>
                    )
                  })}
                  {held && <path className="ph-txneedle-peak" data-frac={peak.toFixed(3)} d={radial(R - 7, R + 2.5, peak)} />}
                  <line
                    className="ph-txneedle-needle"
                    data-frac={needle.toFixed(3)}
                    x1={CX}
                    y1={CY}
                    x2={CX}
                    y2={CY - R - 2}
                    transform={`rotate(${needleAngle(needle).toFixed(2)} ${CX} ${CY})`}
                  />
                  </g>
                  {/* The band, past the drawing on every side: a wide face has it edge to edge, and the
                      needle's foot, which runs on to the pivot, never shows under it. */}
                  <rect className="txmeters-pane-band" x={-W * 5} y={LARGE_BAND_TOP} width={W * 11} height={W} />
                  <text className="txmeters-pane-label" x={2} y={LARGE_BOTTOM - 3}>
                    {r.label}
                  </text>
                  <text className="txmeters-pane-value" x={W - 2} y={LARGE_BOTTOM - 3} textAnchor="end">
                    {silent ? '⊘' : value}
                  </text>
                </svg>
              </div>
            </div>
          )
        })}
      </div>
    )
  }
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
                <svg className="ph-txneedle-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="xMidYMin meet" aria-hidden="true">
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
                    {/* The glyph alone on the face (the readout has no room for the words); the words
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
