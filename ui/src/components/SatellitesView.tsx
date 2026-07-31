// Satellites — the top-level section: WHEN to try WHICH bird, favorites first.
// Modeled on the workflow of the field's standard tools (CSN S.A.T., SatPC32,
// Look4Sat): a favorites set drives everything (declutter + prediction focus),
// a ranked "your best passes" strip answers the when/which question in one
// line, the 48 h schedule carries countdowns + ⏰ pass alarms, and the detail
// zone shows the pass on the SKY DOME (hero when a pass is live) with SatNOGS
// frequencies/status (community-measured truth — absent when offline, never
// guessed). The Connect "Satellite Passes" pane stays as the compact glance
// view; this is the planning surface. Rotor auto-track arms here when a rotor
// is configured.
import { useCallback, useEffect, useMemo, useState } from 'react'
import type {
  NeedTag,
  SatDetail,
  SatPass,
  SatTrackStatus,
  SatVfoMap,
  SatView,
  Settings,
  Station,
} from '../types'
import {
  getSatellites,
  getSatSchedule,
  getSatDetail,
  getSettings,
  setSatTransponder,
  startSatTrack,
  stopSatTrack,
  getSatTrackStatus,
} from '../api'
import { satChasingSet, toggleSatChasing } from '../features/satChase'
import { satAlarmMap, toggleSatAlarm, setSatAlarmLead } from '../features/satAlarm'
import { heatPulse } from '../features/pulse'
import { pushToast } from '../toast'
import { MapView } from './MapView'
import { useTheme } from '../useTheme'

interface Props {
  /** Bird to select (map click hand-off). The section follows changes. */
  focusSat?: string | null
  onPopOut?: () => void
}

const SCHEDULE_HOURS = 48

/** 8-wind compass label for a pass direction ("NW→SE"). */
function wind8(az: number): string {
  const w = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW']
  return w[Math.round(((az % 360) + 360) % 360 / 45) % 8]
}

const hhmm = (unix: number) => {
  const d = new Date(unix * 1000)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

/** "in 38 min" / "in 3.2 h" / "NOW" for a pass relative to now (secs). */
function countdown(p: SatPass, nowSecs: number): string {
  if (p.aosUnix <= nowSecs && nowSecs <= p.losUnix) return 'NOW'
  const min = Math.round((p.aosUnix - nowSecs) / 60)
  return min < 90 ? `in ${Math.max(1, min)} min` : `in ${(min / 60).toFixed(1)} h`
}

/** Geometry-first pass quality: max elevation dominates (a 70° pass is a
 * different sport from a 12° horizon-scrape), duration breaks ties, a bird
 * SatNOGS calls dead sinks to the bottom. Score is for RANKING only — the UI
 * never shows a made-up number. */
function passScore(p: SatPass): number {
  const dead = p.status === 'dead' || p.status === 're-entered'
  const durMin = (p.losUnix - p.aosUnix) / 60
  return (dead ? -1000 : 0) + p.maxElDeg + Math.min(durMin, 15) * 0.8 + (p.status === 'alive' ? 8 : 0)
}

/** Plain-language "why" line for the best-passes strip. */
function whyLine(p: SatPass, nowSecs: number): string {
  const el = Math.round(p.maxElDeg)
  const dur = Math.max(1, Math.round((p.losUnix - p.aosUnix) / 60))
  const quality = el >= 60 ? 'overhead pass' : el >= 30 ? 'high pass' : el >= 15 ? 'workable pass' : 'low horizon pass'
  const status =
    p.status === 'alive'
      ? ' · reported alive (SatNOGS)'
      : p.status === 'dead' || p.status === 're-entered'
        ? ` · reported ${p.status.toUpperCase()} (SatNOGS)`
        : ''
  return `${hhmm(p.aosUnix)} ${countdown(p, nowSecs)} — ${el}° ${quality}, ${dur} min, ${wind8(p.aosAzDeg)}→${wind8(p.losAzDeg)}${status}`
}

/* ======================= the sky dome (hero instrument) =====================
 * The one view every satellite operator reads instinctively. SVG on purpose:
 * crisp at any size, themeable from CSS variables, and every mark can carry a
 * <title> — a canvas instrument would be a black box to a screen reader.
 * Geometry is N-up az/el: el 90° = centre, el 0° = rim, exactly as Gpredict /
 * Look4Sat draw it. */

/** Rim radius and centre in viewBox units; the 24 u margin holds the compass
 * letters and ring labels. */
const DOME_R = 100
const DOME_C = 124

/** N-up az/el → SVG point. */
function skyPt(azDeg: number, elDeg: number): [number, number] {
  const r = (DOME_R * (90 - Math.min(90, Math.max(0, elDeg)))) / 90
  const a = (azDeg * Math.PI) / 180
  return [DOME_C + r * Math.sin(a), DOME_C - r * Math.cos(a)]
}

/** Ring radius for an elevation (0 = rim). */
const ringR = (elDeg: number) => (DOME_R * (90 - elDeg)) / 90

interface LookAngle {
  az: number
  el: number
}

/** Where the bird is at `tSecs` by interpolating the computed pass track;
 * null outside the track. Azimuth interpolates the SHORT way, so a pass that
 * crosses north doesn't sweep backwards through 358°. */
function trackAt(track: [number, number, number][], tSecs: number): LookAngle | null {
  if (track.length < 2) return null
  if (tSecs < track[0][0] || tSecs > track[track.length - 1][0]) return null
  for (let i = 1; i < track.length; i++) {
    if (tSecs <= track[i][0]) {
      const [t0, az0, el0] = track[i - 1]
      const [t1, az1, el1] = track[i]
      const f = (tSecs - t0) / Math.max(1, t1 - t0)
      let dAz = az1 - az0
      if (dAz > 180) dAz -= 360
      if (dAz < -180) dAz += 360
      return { az: (az0 + f * dAz + 360) % 360, el: el0 + f * (el1 - el0) }
    }
  }
  return null
}

/** True angular separation between two look angles, in degrees. This is the
 * number a beam cares about; "az off by 6°, el off by 1°" is not the same
 * thing (6° of azimuth error at el 80° is barely a degree of sky). */
function pointingError(a: LookAngle, b: LookAngle): number {
  const rad = Math.PI / 180
  const c =
    Math.sin(a.el * rad) * Math.sin(b.el * rad) +
    Math.cos(a.el * rad) * Math.cos(b.el * rad) * Math.cos((a.az - b.az) * rad)
  return Math.acos(Math.min(1, Math.max(-1, c))) / rad
}

/** What the rotator ghost is allowed to claim. The backend reports the
 * commanded pair as absent rather than zero, so this reads the answer instead
 * of inferring it — a UI cannot tell a real "commanded el 0" (prepositioning on
 * the horizon) from "no elevation was ever sent" (an az-only rotor) by looking
 * at the number.
 *  - `none`   — nothing has been commanded: the loop deliberately drives
 *               nothing while armed, and holds fire until 5 min before AOS.
 *  - `az-only`— the rotator took azimuth but refused elevation; we know the
 *               commanded azimuth and genuinely do not know an elevation.
 *  - `full`   — az AND el were commanded; draw both and the error to the bird. */
type GhostKind = 'none' | 'az-only' | 'full'
function ghostKind(t: SatTrackStatus | null): GhostKind {
  if (t == null || t.azDeg == null) return 'none'
  return t.elDeg == null ? 'az-only' : 'full'
}

/** Triangle marker path (up = rising/AOS, down = setting/LOS). Shape, not
 * colour, carries the distinction — it survives greyscale and colourblindness. */
function triPath(x: number, y: number, r: number, up: boolean): string {
  const h = up ? -r : r
  return `M${(x).toFixed(1)},${(y + h).toFixed(1)} L${(x + r * 0.95).toFixed(1)},${(y - h * 0.8).toFixed(1)} L${(x - r * 0.95).toFixed(1)},${(y - h * 0.8).toFixed(1)} Z`
}

const deg = (v: number) => `${Math.round(v)}°`

function SkyDome({
  name,
  pass,
  track,
  rotor,
  nowSecs,
}: {
  name: string
  pass: SatPass
  /** (unix, az, el) samples across the pass. */
  track: [number, number, number][]
  /** Auto-track status for THIS bird (already filtered), or null. */
  rotor: SatTrackStatus | null
  nowSecs: number
}) {
  const live = nowSecs >= pass.aosUnix && nowSecs <= pass.losUnix
  // MOTION MEANS THE PASS IS LIVE. One clock: the shared pulse module is fed
  // LIVE wall time (features/pulse.ts — the frozen-sine bug was a pulse driven
  // off a slow tick, which parks the sine on an arbitrary value), and the 1 s
  // tick that forces the redraw exists ONLY while a pass is in progress and the
  // tab is visible. Nothing here animates for decoration.
  const [beatMs, setBeatMs] = useState(() => Date.now())
  useEffect(() => {
    if (!live) return
    const id = window.setInterval(() => {
      if (!document.hidden) setBeatMs(Date.now())
    }, 1_000)
    return () => window.clearInterval(id)
  }, [live])

  // Where the bird is. While the tracker is actually following the pass we take
  // the backend's own look angle: it is computed in the same tick as the
  // commanded pair below, so the gap the dome draws is the REAL tracking error
  // and not an artefact of interpolating a 30 s track sample.
  const bird: LookAngle | null =
    rotor != null && rotor.satAzDeg != null && rotor.satElDeg != null
      ? { az: rotor.satAzDeg, el: rotor.satElDeg }
      : trackAt(track, live ? beatMs / 1000 : nowSecs)

  const ghost = ghostKind(rotor)
  // The COMMANDED pair, absent until one was genuinely sent. Held as locals so
  // "we have this number" is checked once, where it is decided.
  const cmdAz = rotor?.azDeg ?? null
  const cmdEl = rotor?.elDeg ?? null
  const birdPt = bird ? skyPt(bird.az, bird.el) : null
  const ghostPt = cmdAz != null && cmdEl != null ? skyPt(cmdAz, cmdEl) : null
  const ghostRim = cmdAz != null && ghost === 'az-only' ? skyPt(cmdAz, 0) : null
  const aosPt = skyPt(pass.aosAzDeg, 0)
  const losPt = skyPt(pass.losAzDeg, 0)
  const errDeg =
    bird && cmdAz != null && cmdEl != null ? pointingError(bird, { az: cmdAz, el: cmdEl }) : null

  // Track drawn segment-by-segment so stroke weight + opacity can ramp with
  // elevation: the high, workable part of the pass reads first at a glance.
  // One hue (--accent) rather than an invented colour scale — it stays legible
  // in both themes and in greyscale, and it never collides with the app's
  // semantic palette (green = open, amber = marginal).
  const segs = useMemo(
    () =>
      track.slice(1).map(([, az1, el1], i) => {
        const [, az0, el0] = track[i]
        const [x0, y0] = skyPt(az0, el0)
        const [x1, y1] = skyPt(az1, el1)
        const f = Math.min(1, Math.max(0, (el0 + el1) / 2 / 90))
        return (
          <line
            key={i}
            x1={x0.toFixed(1)}
            y1={y0.toFixed(1)}
            x2={x1.toFixed(1)}
            y2={y1.toFixed(1)}
            className="sat-dome-track"
            strokeWidth={(1.3 + 2.5 * f).toFixed(2)}
            strokeOpacity={(0.5 + 0.5 * f).toFixed(2)}
          />
        )
      }),
    [track],
  )

  // The breath: 0.4–1.0 on a ~2.8 s period, from the shared clock.
  const breath = live ? heatPulse(beatMs) : 1

  const ghostText =
    rotor == null
      ? null
      : cmdAz == null
        ? 'armed — no rotor command sent yet'
        : cmdEl == null
          ? `az ${deg(cmdAz)} · elevation not commanded (az-only rotator)`
          : `az ${deg(cmdAz)} el ${deg(cmdEl)}${errDeg != null ? ` · Δ ${errDeg.toFixed(1)}°` : ''}`

  const label = bird
    ? `Sky dome for ${name}, north up. Satellite at azimuth ${Math.round(bird.az)} degrees, elevation ${Math.round(bird.el)} degrees.`
    : `Sky dome for ${name}, north up. Pass track from azimuth ${Math.round(pass.aosAzDeg)} to ${Math.round(pass.losAzDeg)} degrees, maximum elevation ${Math.round(pass.maxElDeg)} degrees.`

  return (
    <div className={`sat-sky${live ? ' live' : ''}`}>
      <svg viewBox={`0 0 ${DOME_C * 2} ${DOME_C * 2}`} className="sat-dome" role="img" aria-label={label}>
        <circle cx={DOME_C} cy={DOME_C} r={DOME_R} className="sat-dome-face" />
        {[30, 60].map((el) => (
          <circle key={el} cx={DOME_C} cy={DOME_C} r={ringR(el)} className="sat-dome-ring" />
        ))}
        <line x1={DOME_C} y1={DOME_C - DOME_R} x2={DOME_C} y2={DOME_C + DOME_R} className="sat-dome-ring" />
        <line x1={DOME_C - DOME_R} y1={DOME_C} x2={DOME_C + DOME_R} y2={DOME_C} className="sat-dome-ring" />
        {/* The horizon last of the rings, and heavier: it is the line the bird
            rises through, and everything outside it is unworkable. */}
        <circle cx={DOME_C} cy={DOME_C} r={DOME_R} className="sat-dome-horizon" />
        {[30, 60].map((el) => (
          <text key={el} x={DOME_C + 5} y={DOME_C - ringR(el) + 11} className="sat-dome-ringlabel">
            {el}°
          </text>
        ))}
        <text x={DOME_C} y={DOME_C - DOME_R - 6} className="sat-dome-compass" textAnchor="middle">N</text>
        <text x={DOME_C + DOME_R + 10} y={DOME_C + 4} className="sat-dome-compass" textAnchor="middle">E</text>
        <text x={DOME_C} y={DOME_C + DOME_R + 14} className="sat-dome-compass" textAnchor="middle">S</text>
        <text x={DOME_C - DOME_R - 10} y={DOME_C + 4} className="sat-dome-compass" textAnchor="middle">W</text>
        {segs}
        <path d={triPath(aosPt[0], aosPt[1], 5, true)} className="sat-dome-aos">
          <title>AOS — rises at {deg(pass.aosAzDeg)} ({wind8(pass.aosAzDeg)}) {hhmm(pass.aosUnix)}</title>
        </path>
        <path d={triPath(losPt[0], losPt[1], 5, false)} className="sat-dome-los">
          <title>LOS — sets at {deg(pass.losAzDeg)} ({wind8(pass.losAzDeg)}) {hhmm(pass.losUnix)}</title>
        </path>
        {/* THE ROTATOR GHOST. A second marker at what the antenna was told, not
            where it is: the gap to the bird IS the tracking error, and drawing
            it is what stops a legitimate deadband from looking like a fault. */}
        {ghostPt && (
          <g className="sat-dome-ghost" data-testid="sat-ghost">
            <title>Antenna: commanded az/el (not a rotator read-back)</title>
            {birdPt && (
              <line x1={ghostPt[0]} y1={ghostPt[1]} x2={birdPt[0]} y2={birdPt[1]} className="sat-dome-err" />
            )}
            <circle cx={ghostPt[0]} cy={ghostPt[1]} r={7} className="sat-dome-ghost-ring" />
            <circle cx={ghostPt[0]} cy={ghostPt[1]} r={1.8} className="sat-dome-ghost-hub" />
          </g>
        )}
        {/* Az-only rotator: we know the azimuth it was told and NOTHING about
            an elevation it was never sent. A spoke along that azimuth says
            exactly that — a ghost dot would invent an elevation. */}
        {ghostRim && cmdAz != null && (
          <g className="sat-dome-ghost az-only" data-testid="sat-ghost-az">
            <title>Antenna: azimuth {deg(cmdAz)} commanded — az-only rotator, no elevation sent</title>
            <line x1={DOME_C} y1={DOME_C} x2={ghostRim[0]} y2={ghostRim[1]} className="sat-dome-azspoke" />
          </g>
        )}
        {birdPt && bird && (
          <g className="sat-dome-bird" data-testid="sat-bird">
            <title>{name} — az {deg(bird.az)} el {deg(bird.el)}</title>
            {live && (
              <circle
                cx={birdPt[0]}
                cy={birdPt[1]}
                r={(5.5 + 5 * breath).toFixed(1)}
                className="sat-dome-bird-halo"
                opacity={(0.32 * breath).toFixed(2)}
              />
            )}
            <circle cx={birdPt[0]} cy={birdPt[1]} r={5} />
          </g>
        )}
      </svg>
      {/* The text equivalent. An SVG instrument is invisible to a screen
          reader, and these are the numbers an operator reads off the dome
          anyway — so it is shown, not hidden. Absent values are absent rows. */}
      <dl className="sat-dome-readout">
        {bird && (
          <div>
            <dt>Satellite</dt>
            <dd>
              az {deg(bird.az)} el {deg(bird.el)}
            </dd>
          </div>
        )}
        {rotor?.rangeKm != null && (
          <div>
            <dt>Range</dt>
            <dd>
              {Math.round(rotor.rangeKm)} km
              {rotor.rangeRateKmS != null &&
                ` · ${rotor.rangeRateKmS >= 0 ? '+' : ''}${rotor.rangeRateKmS.toFixed(2)} km/s ${
                  rotor.rangeRateKmS < 0 ? 'closing' : 'opening'
                }`}
            </dd>
          </div>
        )}
        {ghostText && (
          <div>
            <dt title="What the rotator was COMMANDED — not a read-back. Δ is the true angular gap to the bird.">
              Antenna
            </dt>
            <dd className={ghost === 'az-only' ? 'partial' : undefined}>{ghostText}</dd>
          </div>
        )}
        <div>
          <dt>Rise / set</dt>
          <dd>
            ▲ {deg(pass.aosAzDeg)} {wind8(pass.aosAzDeg)} · ▼ {deg(pass.losAzDeg)} {wind8(pass.losAzDeg)}
          </dd>
        </div>
      </dl>
    </div>
  )
}

/** AOS · TCA · LOS on one rail, with the live position marked. Replaces the
 * flat "next pass …" line for the bird being watched: a pass is an interval,
 * and where you are IN it is the question the numbers don't answer at a
 * glance. TCA comes from the computed track (the sample with the highest
 * elevation) — omitted, not guessed, when there is no track. */
function PassTimeline({
  pass,
  tcaUnix,
  nowSecs,
}: {
  pass: SatPass
  tcaUnix: number | null
  nowSecs: number
}) {
  const span = Math.max(1, pass.losUnix - pass.aosUnix)
  const pct = (t: number) => `${Math.min(100, Math.max(0, ((t - pass.aosUnix) / span) * 100)).toFixed(1)}%`
  const live = nowSecs >= pass.aosUnix && nowSecs <= pass.losUnix
  const minsLeft = Math.max(0, Math.round((pass.losUnix - nowSecs) / 60))
  return (
    <div className={`sat-timeline${live ? ' live' : ''}`}>
      <div className="sat-tl-rail" aria-hidden="true">
        {live && <div className="sat-tl-done" style={{ width: pct(nowSecs) }} />}
        {tcaUnix != null && <div className="sat-tl-tca" style={{ left: pct(tcaUnix) }} />}
        {live && <div className="sat-tl-now" style={{ left: pct(nowSecs) }} />}
      </div>
      <div className="sat-tl-labels">
        <span>AOS {hhmm(pass.aosUnix)}</span>
        <span className="sat-tl-tca-label">
          {tcaUnix != null ? `TCA ${hhmm(tcaUnix)} · ` : ''}max {Math.round(pass.maxElDeg)}°
        </span>
        <span>LOS {hhmm(pass.losUnix)}</span>
      </div>
      <div className="sat-tl-state">
        {live ? `IN PASS — ${minsLeft} min to LOS` : `next pass ${countdown(pass, nowSecs)}`}
      </div>
    </div>
  )
}

/** Hz → MHz at 10 Hz resolution (what an SSB operator needs to see move). */
const fmtMHz = (hz: number) => `${(hz / 1e6).toFixed(5)} MHz`
/** Signed Doppler correction, kHz once it is big enough to matter. */
const fmtShift = (hz: number) => {
  const a = Math.abs(hz)
  const sign = hz < 0 ? '-' : '+'
  return a >= 1000 ? `${sign}${(a / 1000).toFixed(2)} kHz` : `${sign}${Math.round(a)} Hz`
}

/** What Doppler has the radio tuned to, per leg, straight from the engine.
 *
 * HONESTY: every frequency here is nullable and null means "we are not tuning
 * that leg". No zeros, no placeholder dashes — an absent row, plus one line
 * saying WHY when there is nothing at all (the reasons are all things this
 * component actually knows: the two opt-in switches, whether a transponder is
 * held, and whether the pass has started). */
function DopplerReadout({
  rotor,
  dopplerOn,
  vfoMap,
  held,
}: {
  rotor: SatTrackStatus
  dopplerOn: boolean
  vfoMap: SatVfoMap
  held: boolean
}) {
  const any = rotor.downlinkHz != null || rotor.uplinkHz != null
  if (!any) {
    const why = !dopplerOn
      ? 'Doppler is off — nothing is being tuned (Settings ▸ Radio ▸ Satellite Doppler).'
      : vfoMap === 'off'
        ? 'VFO mapping is Off — Doppler is not writing to the radio.'
        : !held
          ? 'No transponder selected — pick one below to put the dial under Doppler.'
          : rotor.state !== 'tracking'
            ? 'Doppler corrects from AOS — nothing to correct until the bird is up.'
            : 'Doppler has not reported a tuning for this pass yet.'
    return <div className="sat-doppler none">{why}</div>
  }
  return (
    <div className="sat-doppler">
      <div className="sat-dop-head">
        <span className="sat-dop-title">Doppler</span>
        {rotor.transponder && <span className="sat-dop-tp">{rotor.transponder}</span>}
        {rotor.inverting && (
          <span
            className="sat-invert"
            title="Inverting linear transponder: tune the downlink UP and your uplink goes DOWN, and the sidebands swap (LSB up, USB down)."
          >
            INVERTING
          </span>
        )}
      </div>
      <dl className="sat-dop-legs">
        {rotor.downlinkHz != null && (
          <div>
            <dt>↓ Downlink</dt>
            <dd>
              {fmtMHz(rotor.downlinkHz)}
              {rotor.downlinkShiftHz != null && (
                <span className="sat-dop-shift"> {fmtShift(rotor.downlinkShiftHz)}</span>
              )}
            </dd>
          </div>
        )}
        {rotor.uplinkHz != null && (
          <div>
            <dt>↑ Uplink</dt>
            <dd>
              {fmtMHz(rotor.uplinkHz)}
              {rotor.uplinkShiftHz != null && (
                <span className="sat-dop-shift"> {fmtShift(rotor.uplinkShiftHz)}</span>
              )}
            </dd>
          </div>
        )}
      </dl>
    </div>
  )
}

/* ==================== the transponder passband strip ========================
 * ONE axis, shared by both legs: offset from the passband CENTRE, in kHz.
 * The legs sit on different bands (145 MHz up, 435 MHz down), so plotting them
 * against absolute frequency would be a two-scale chart whose alignment is
 * arbitrary — it would invent a relationship. The offset is the coordinate
 * they genuinely share, and the one an inverting transponder mirrors.
 *
 * The lesson IS the geometry: the downlink sits at +offset and the uplink at
 * −offset while the transponder inverts, so tuning up the band walks the two
 * marks apart in opposite directions. Nothing animates to explain that.
 *
 * Doppler lives in the NUMBERS, never in the marks. Correction moves the DIAL
 * so the operator's place inside the passband stays where they put it —
 * sliding the band would animate a thing that is not moving.
 *
 * Colour: --rx on the downlink mark, --tx on the uplink, the transmit/receive
 * tokens every operator already reads. In the LIGHT theme that pair separates
 * by only ΔE 6.1 under deuteranopia, which is good enough ONLY because three
 * secondary encodings carry the same distinction: the legs are on separate
 * rows, each row carries a text label, and each carries a direction glyph
 * (↓ downlink / ↑ uplink) in both the label text and the SHAPE of its mark.
 * Drop any one of the three and this instrument stops working for a red-green
 * colourblind operator — SatellitesView.passband.test.tsx pins all three.
 */

/** Plot box in viewBox units. The height INCLUDES the axis label band: a box
 * sized to the marks alone puts a nested scrollbar inside the card. */
const PB_W = 320
const PB_H = 102
const PB_PAD = 8
const PB_CX = PB_W / 2
/** Half the drawable width: the axis runs −halfWidth … +halfWidth. */
const PB_SPAN = PB_CX - PB_PAD
const PB_TRACK_H = 10
/** Where the axis sits under both rows. */
const PB_AXIS_Y = 76

/** viewBox x for a signed offset. Clamped for DRAWING only — a mark may never
 * leave the instrument, and every number printed beside it stays true. */
const pbX = (hz: number, half: number) =>
  PB_CX + (Math.max(-half, Math.min(half, hz)) / half) * PB_SPAN

/** kHz with trailing zeros dropped ("12.5", "5", "3.25"). */
const pbKHz = (hz: number) => `${+(hz / 1000).toFixed(2)}`

/** The cursor mark, drawn around x = 0 so the lane can translate it into
 * place: a stem through the track plus a pointer aimed at it from outside.
 * SHAPE carries the leg as well as colour does — it survives greyscale and the
 * light theme's marginal red/green separation. */
function pbMark(down: boolean, trackY: number) {
  const y0 = trackY - 1.5
  const y1 = trackY + PB_TRACK_H + 1.5
  const apex = down ? y0 : y1
  const base = down ? y0 - 4.5 : y1 + 4.5
  return (
    <>
      <path d={`M-4,${base} L4,${base} L0,${apex} Z`} />
      <rect x={-2} y={y0} width={4} height={y1 - y0} rx={2} />
    </>
  )
}

/** One leg's row: the recessive passband track, the direct label (glyph, leg,
 * live dial frequency, signed Doppler shift) and the saturated cursor. */
function PassbandLane({
  down,
  freqHz,
  shiftHz,
  offsetHz,
  halfHz,
}: {
  /** Downlink lane (on top — what you hear) or uplink (below — what you send). */
  down: boolean
  /** The live dial frequency, or null when that leg is not being tuned. */
  freqHz: number | null
  shiftHz: number | null
  /** THIS leg's true signed offset from the passband centre — already negated
   * for the uplink when the transponder inverts. */
  offsetHz: number
  halfHz: number
}) {
  const labelY = down ? 8 : 42
  const trackY = down ? 19 : 53
  const glyph = down ? '↓' : '↑'
  const name = down ? 'Downlink' : 'Uplink'
  const x = pbX(offsetHz, halfHz)
  // Outside the passband the mark parks on the edge; the tooltip says so
  // rather than letting the picture quietly disagree with the numbers.
  const clamped = Math.abs(offsetHz) > halfHz
  const mark = pbMark(down, trackY)
  return (
    <g>
      <rect
        className="sat-pb-track"
        x={PB_PAD}
        y={trackY}
        width={PB_W - 2 * PB_PAD}
        height={PB_TRACK_H}
        rx={4}
      />
      {/* 0 kHz, SOLID — a dashed line reads as a threshold and centre is not
          one. Drawn per ROW rather than as one line down the whole strip: a
          full-height line runs straight through the row labels (it landed in
          the space between "+770" and "Hz"). The two segments are collinear
          and the lower one runs into the axis tick, so they still read as the
          one shared centre. Painted after the track and before the mark, so
          the mark's gap knocks it out where the cursor sits. */}
      <line
        className="sat-pb-centre"
        x1={PB_CX}
        y1={down ? trackY - 8 : trackY - 6}
        x2={PB_CX}
        y2={down ? trackY + PB_TRACK_H + 4 : PB_AXIS_Y}
      />
      {/* The row label IS the legend — glyph, leg, dial frequency, shift. */}
      <text className="sat-pb-label" x={PB_PAD} y={labelY}>
        {glyph} {name}
        {freqHz != null && <tspan> {fmtMHz(freqHz)}</tspan>}
        {shiftHz != null && <tspan className="sat-pb-shift"> {fmtShift(shiftHz)}</tspan>}
      </text>
      <g transform={`translate(${x.toFixed(2)},0)`} data-testid={down ? 'sat-pb-down' : 'sat-pb-up'}>
        {/* A gap of surface colour under the mark instead of a border around
            it: the same outline, stroked in the card's own background. */}
        <g className="sat-pb-gap">{mark}</g>
        <g className={`sat-pb-mark ${down ? 'rx' : 'tx'}`}>{mark}</g>
        {/* ~24 px of hit target for a 4 px mark — 26 viewBox units, because the
            narrowest the sidebar column ever gets (340 px) draws this box at
            about 0.95 units to the pixel. */}
        <rect className="sat-pb-hit" x={-13} y={down ? 2 : 40} width={26} height={32}>
          <title>
            {`${glyph} ${name}${freqHz != null ? ` — ${freqHz} Hz` : ''} — offset ${fmtShift(
              offsetHz,
            )} from passband centre${
              clamped ? ' (outside the passband — the mark is parked on the edge)' : ''
            }`}
          </title>
        </rect>
      </g>
    </g>
  )
}

/** Where the operator sits inside the transponder, both legs on one axis.
 *
 * HONESTY: no `offsetHz` means Doppler is not tuning anything, and the readout
 * above already explains why — this renders nothing rather than saying it
 * twice. No `halfWidthHz` means the passband width is genuinely unknown (many
 * SatNOGS transmitter records carry none): one line saying so and no axis,
 * because an axis you cannot size is a made-up one. */
function PassbandStrip({ rotor }: { rotor: SatTrackStatus }) {
  const offset = rotor.offsetHz
  if (offset == null) return null
  const half = rotor.halfWidthHz ?? 0
  // Under inversion the uplink genuinely sits at the negated offset. That is
  // not a drawing trick — it is where the signal comes out.
  const upOffset = rotor.inverting ? -offset : offset
  const legs = [
    { down: true, freq: rotor.downlinkHz, shift: rotor.downlinkShiftHz, off: offset },
    { down: false, freq: rotor.uplinkHz, shift: rotor.uplinkShiftHz, off: upOffset },
  ]
  /** Exact numbers for the text equivalent: Hz, unrounded, absent when absent. */
  const legText = (freq: number | null, shift: number | null, off: number) =>
    [
      freq != null ? `${freq} Hz` : null,
      shift != null ? `Doppler ${fmtShift(shift)}` : null,
      `offset ${fmtShift(off)}`,
    ]
      .filter((part) => part != null)
      .join(' · ')
  const label =
    `Transponder passband, ${rotor.inverting ? 'inverting' : 'non-inverting'}, ` +
    `±${pbKHz(half)} kHz either side of centre. ` +
    `Downlink ${fmtShift(offset)} from centre, uplink ${fmtShift(upOffset)} from centre.`
  return (
    <div className="sat-pb" data-testid="sat-passband">
      <div className="sat-pb-head">
        <span className="sat-pb-title">Passband</span>
        {/* The mirror is drawn, but it is also said in words — a picture is a
            poor place to learn a rule nobody has told you. */}
        <span className={`sat-pb-mode${rotor.inverting ? ' inv' : ''}`}>
          {rotor.inverting
            ? 'inverting — tune up, transmit down'
            : 'non-inverting — both legs move the same way'}
        </span>
      </div>
      {half > 0 ? (
        <svg viewBox={`0 0 ${PB_W} ${PB_H}`} className="sat-pb-plot" role="img" aria-label={label}>
          {legs.map((l) => (
            <PassbandLane
              key={l.down ? 'down' : 'up'}
              down={l.down}
              freqHz={l.freq}
              shiftHz={l.shift}
              offsetHz={l.off}
              halfHz={half}
            />
          ))}
          <line className="sat-pb-axis" x1={PB_PAD} y1={PB_AXIS_Y} x2={PB_W - PB_PAD} y2={PB_AXIS_Y} />
          {[PB_PAD, PB_CX, PB_W - PB_PAD].map((x) => (
            <line className="sat-pb-axis" key={x} x1={x} y1={PB_AXIS_Y} x2={x} y2={PB_AXIS_Y + 3} />
          ))}
          {/* End labels are anchored INWARD, so the axis extent cannot spill
              out of the box at any rendered width. */}
          <text className="sat-pb-ticklabel" x={PB_PAD} y={88}>
            −{pbKHz(half)}
          </text>
          <text className="sat-pb-ticklabel" x={PB_CX} y={88} textAnchor="middle">
            0 kHz
          </text>
          <text className="sat-pb-ticklabel" x={PB_W - PB_PAD} y={88} textAnchor="end">
            +{pbKHz(half)}
          </text>
          <text className="sat-pb-axistitle" x={PB_CX} y={99} textAnchor="middle">
            kHz from passband centre
          </text>
        </svg>
      ) : (
        <div className="sat-pb-nowidth">
          {/* Zero width has TWO causes and the UI cannot tell them apart, so it
              must not name one: an FM repeater or beacon genuinely has no
              passband to tune inside (SO-50 and the rest of the easy birds are
              all like this), and a linear transponder whose SatNOGS record
              carries no upper edge looks identical here. Blaming the database
              for a channel would be wrong for the satellites most people
              work. */}
          No passband to tune inside — this is a single channel, or SatNOGS carries no width for it.
          There is no axis to draw; the offsets below are still exact.
        </div>
      )}
      {/* The text equivalent, the same way the sky dome carries one: an SVG
          instrument is invisible to a screen reader, and these are the exact
          numbers behind the marks. Deliberately NOT an aria-live region — they
          change on every poll, and announcing them would talk over the operator
          for the whole pass. It is here to be read on demand. */}
      <dl className="sat-pb-readout">
        {legs.map((l) => (
          <div key={l.down ? 'down' : 'up'}>
            <dt>{l.down ? '↓ Downlink' : '↑ Uplink'}</dt>
            <dd>{legText(l.freq, l.shift, l.off)}</dd>
          </div>
        ))}
        {half > 0 && (
          <div>
            <dt>Passband</dt>
            <dd>±{pbKHz(half)} kHz from centre</dd>
          </div>
        )}
      </dl>
    </div>
  )
}

/** One leg of a transponder in MHz. A linear transponder is a BAND, so when
 * SatNOGS gives both edges we show both — the centre alone hides where in the
 * passband you can actually work. */
const fmtLeg = (lowHz: number | null, highHz: number | null) => {
  if (lowHz == null) return '—'
  const low = (lowHz / 1e6).toFixed(3)
  return highHz != null && highHz > lowHz ? `${low}–${(highHz / 1e6).toFixed(3)}` : low
}

/** Per-leg modes, compact ("USB↓/LSB↑"). An inverting transponder is USB down
 * and LSB up; the single `mode` field cannot say that, so the legs win when
 * SatNOGS has them. */
const legModes = (t: SatDetail['transmitters'][number]) => {
  const down = t.downlinkMode
  const up = t.uplinkMode
  if (down && up) return down === up ? down : `${down}↓/${up}↑`
  return down ?? up ?? t.mode ?? '—'
}

export function SatellitesView({ focusSat, onPopOut }: Props) {
  const [view, setView] = useState<SatView | null>(null)
  const [favs, setFavs] = useState<Set<string>>(() => satChasingSet())
  const [schedule, setSchedule] = useState<SatPass[]>([])
  const [selected, setSelected] = useState<string | null>(focusSat ?? null)
  const [detail, setDetail] = useState<SatDetail | null>(null)
  const [alarms, setAlarms] = useState(() => satAlarmMap())
  const [rotorOn, setRotorOn] = useState(false)
  const [gridSet, setGridSet] = useState(true) // optimistic until settings load
  const [myGrid, setMyGrid] = useState('') // for the embedded detail globe's center
  const [theme] = useTheme()
  const [track, setTrack] = useState<SatTrackStatus | null>(null)
  const [search, setSearch] = useState('')
  // The transponder handed to the Doppler engine, and which bird it belongs to.
  // NOTE: there is no backend read-back of the engine's current selection, so
  // this is what the OPERATOR chose here — a pass that runs to LOS clears the
  // hold backend-side and this keeps showing the last pick until they change it.
  const [tuned, setTuned] = useState<{ name: string; index: number } | null>(null)
  const [dopplerOn, setDopplerOn] = useState(false)
  const [vfoMap, setVfoMap] = useState<SatVfoMap>('off')
  const [nowTick, setNowTick] = useState(() => Date.now())
  const nowSecs = Math.floor(nowTick / 1000)

  // Map click hand-off: follow later clicks too, not just the mount value.
  useEffect(() => {
    if (focusSat) setSelected(focusSat)
  }, [focusSat])

  // Countdown re-render cadence. 10 s keeps "in N min" honest without churn.
  // (The sky dome runs its own 1 s tick while a pass is live — a 1 s tick here
  // would re-render the whole 48 h schedule for a marker that moved 2 px.)
  useEffect(() => {
    const id = window.setInterval(() => setNowTick(Date.now()), 10_000)
    return () => window.clearInterval(id)
  }, [])

  // All birds (favorites manager + fallback next-pass data): 60 s poll of the
  // same snapshot the map uses.
  useEffect(() => {
    let live = true
    const load = () => getSatellites().then((v) => live && setView(v)).catch(() => {})
    load()
    const id = window.setInterval(load, 60_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [])

  // Favorites schedule: recompute on favorites change + a 5 min poll (geometry
  // barely moves in minutes; TLE refreshes are half-daily).
  const favKey = useMemo(() => [...favs].sort().join(','), [favs])
  useEffect(() => {
    let live = true
    const names = favKey === '' ? [] : favKey.split(',')
    if (names.length === 0) {
      setSchedule([])
      return
    }
    const load = () =>
      getSatSchedule(names, SCHEDULE_HOURS)
        .then((p) => live && setSchedule(p))
        .catch(() => {})
    load()
    const id = window.setInterval(load, 300_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [favKey])

  // Selected-bird detail (SatNOGS + polar track): refresh each minute while open.
  useEffect(() => {
    if (!selected) {
      setDetail(null)
      return
    }
    let live = true
    const load = () =>
      getSatDetail(selected)
        .then((d) => live && setDetail(d))
        .catch(() => live && setDetail(null))
    load()
    const id = window.setInterval(load, 60_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [selected])

  // Rotor: configured? (model-launched rotctld OR advanced host override), and
  // the live auto-track status while the section is open.
  useEffect(() => {
    let live = true
    getSettings()
      .then((s: Settings) => {
        if (!live) return
        setRotorOn((s.rotatorModel ?? 0) > 0 || s.rotatorHost.trim() !== '')
        setGridSet(s.mygrid.trim().length >= 4) // passes need a real locator
        setMyGrid(s.mygrid)
        setDopplerOn(!!s.satDoppler)
        setVfoMap(s.satVfoMap ?? 'off')
      })
      .catch(() => {})
    return () => {
      live = false
    }
  }, [])
  useEffect(() => {
    if (!rotorOn) return
    let live = true
    const load = () => getSatTrackStatus().then((t) => live && setTrack(t)).catch(() => {})
    load()
    const id = window.setInterval(load, 2000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [rotorOn])

  const onToggleFav = (name: string) => {
    toggleSatChasing(name)
    setFavs(satChasingSet())
  }
  const onToggleAlarm = (name: string) => {
    toggleSatAlarm(name)
    setAlarms(satAlarmMap())
  }

  // Sortable schedule (sortable-everywhere principle, 2026-07-21): default = soonest
  // AOS, click a header to rank by elevation / duration / bird instead.
  type SchedSortKey = 'bird' | 'aos' | 'el' | 'dur' | 'status'
  const [schedSort, setSchedSort] = useState<{ key: SchedSortKey; asc: boolean }>({ key: 'aos', asc: true })
  const upcoming = useMemo(() => {
    const val = (p: (typeof schedule)[number]): string | number => {
      switch (schedSort.key) {
        case 'bird':
          return p.name.toUpperCase()
        case 'aos':
          return p.aosUnix
        case 'el':
          return p.maxElDeg
        case 'dur':
          return p.losUnix - p.aosUnix
        case 'status':
          return p.status ?? ''
      }
    }
    const rows = schedule.filter((p) => p.losUnix > nowSecs)
    rows.sort((a, b) => {
      const va = val(a)
      const vb = val(b)
      const c = typeof va === 'number' && typeof vb === 'number' ? va - vb : String(va).localeCompare(String(vb))
      // Tiebreak: soonest pass first, always.
      return (schedSort.asc ? c : -c) || a.aosUnix - b.aosUnix
    })
    return rows
  }, [schedule, nowSecs, schedSort])
  const schedTh = (label: string, key: SchedSortKey) => (
    <th
      aria-sort={schedSort.key === key ? (schedSort.asc ? 'ascending' : 'descending') : undefined}
    >
      <button
        type="button"
        className="sats-th-btn"
        onClick={() =>
          setSchedSort((s0) =>
            s0.key === key ? { key, asc: !s0.asc } : { key, asc: key === 'bird' || key === 'status' },
          )
        }
        title={`Sort by ${label}`}
      >
        {label}
        {schedSort.key === key ? (schedSort.asc ? ' ▲' : ' ▼') : ''}
      </button>
    </th>
  )
  // "Your best passes": next 24 h, ranked by quality, top 3.
  const best = useMemo(
    () =>
      upcoming
        .filter((p) => p.aosUnix < nowSecs + 24 * 3600)
        .slice()
        .sort((a, b) => passScore(b) - passScore(a))
        .slice(0, 3),
    [upcoming, nowSecs],
  )

  const allBirds = useMemo(() => {
    const names = (view?.birds ?? []).map((b) => b.name).sort()
    const q = search.trim().toUpperCase()
    return q === '' ? names : names.filter((n) => n.toUpperCase().includes(q))
  }, [view, search])

  const tleStale = view != null && view.tleAgeDays > 14

  // Stable empty inputs for the embedded detail globe (it shows only the birds —
  // no stations, spots, or needs), so MapView's per-tick projections don't rebuild.
  const noStations = useMemo(() => [] as Station[], [])
  const noNeeds = useMemo(() => new Map<string, NeedTag>(), [])
  const noSelectCall = useCallback(() => {}, [])
  const selectSatInBox = useCallback((n: string) => setSelected(n), [])

  // The wire index IS the row index: set_sat_transponder indexes the very list
  // get_sat_detail returned (dead entries included) and refuses a dead pick by
  // name. It briefly filtered `alive` on its side, which silently selected a
  // different transponder — a different uplink — on any bird listing a dead
  // transmitter first. Fixed at the backend rather than compensated here: two
  // layers agreeing on a hidden filter is not a contract.
  const tpRows = useMemo(
    () => (detail?.transmitters ?? []).map((t, i) => ({ t, aliveIndex: t.alive ? i : null })),
    [detail],
  )
  // Which of the SELECTED bird's transponders was handed to the engine; null =
  // none of this bird's (a hold on another bird is called out under the table).
  const heldIndex = tuned && detail && tuned.name === detail.name ? tuned.index : null
  // Will a pick actually move the radio? Both switches are operator opt-ins and
  // both default off, so saying nothing here would look like a dead control.
  const dopplerLive = dopplerOn && vfoMap !== 'off'

  // Auto-track status, but only when it belongs to the bird on screen — a badge
  // for a different bird must never decorate this one's sky dome.
  const detailTrack = track != null && detail != null && track.name === detail.name ? track : null
  // TCA from the computed track (highest-elevation sample). No track, no tick —
  // the pass midpoint would be a guess dressed as a measurement.
  const tcaUnix = useMemo(() => {
    const t = detail?.passTrack ?? []
    if (t.length === 0) return null
    let bestSample = t[0]
    for (const s of t) if (s[2] > bestSample[2]) bestSample = s
    return bestSample[0]
  }, [detail])

  const armTrack = (name: string, aosUnix: number) => {
    startSatTrack(name, aosUnix)
      .then((t) => {
        setTrack(t)
        if (t) {
          const doing =
            t.state === 'armed'
              ? 'armed — the rotor stays yours until 5 min before AOS'
              : t.state === 'prepositioning'
                ? 'slewing to the AOS azimuth'
                : 'following the pass'
          pushToast(`Rotor track ${t.name}: ${doing}`, 'success', 5000)
        } else pushToast('Nothing to track — no rotor answering or no matching pass', 'info', 6000)
      })
      .catch((e) => pushToast(`Track failed: ${e instanceof Error ? e.message : e}`, 'error'))
  }
  const disarmTrack = () => {
    stopSatTrack()
      .then(() => setTrack(null))
      .catch(() => {})
  }

  /** Hand a transponder to the Doppler engine, or `null` to hand the dial back.
   * `index` counts the bird's ALIVE transmitters — what the backend indexes.
   * The selection is only shown once the call succeeds: a refused pick must not
   * leave the operator believing the radio is under Doppler control. */
  const pickTransponder = (name: string, index: number | null, label = '') => {
    setSatTransponder(name, index)
      .then(() => {
        setTuned(index == null ? null : { name, index })
        // Re-read the two switches that decide whether this tunes anything —
        // Settings may have changed since this section mounted.
        getSettings()
          .then((s: Settings) => {
            setDopplerOn(!!s.satDoppler)
            setVfoMap(s.satVfoMap ?? 'off')
          })
          .catch(() => {})
        pushToast(
          index == null ? 'Transponder cleared — the dial is yours' : `Working ${name} ${label}`,
          'success',
          4000,
        )
      })
      .catch((e) => pushToast(`Transponder not selected: ${e instanceof Error ? e.message : e}`, 'error'))
  }

  return (
    <div className="sats-view">
      <header className="sats-head">
        <h1>Satellites</h1>
        <span className="sats-sub">
          passes over your grid — modelled from Celestrak elements
          {view ? ` (${view.tleAgeDays.toFixed(1)} d old${tleStale ? ' — STALE' : ''})` : ''}
        </span>
        {track && (
          <span
            className="sats-tracking-badge"
            title={
              track.azDeg == null
                ? 'The rotor has NOT been commanded yet — auto-track takes it 5 min before AOS'
                : 'Auto-track is driving the rotor — angles shown are what was COMMANDED (rotctld read-back lives on the rotor strip/pane)'
            }
          >
            {/* TWO SEPARATE FACTS, and they must be read from two separate
                fields. The PHASE comes from `state`, which the backend derives
                from the clock and always knows. Whether a command has been SENT
                comes from `azDeg` being present. Deriving the phase word from
                the angle instead conflates them — and they genuinely come
                apart: arming a pass that is already under way reports
                "tracking" with nothing commanded yet, and a rotor that stops
                answering mid-pass keeps its phase while the command goes
                stale. Printing "armed" in either case would be flatly wrong.

                With no command to show, the rise azimuth answers the question
                the operator actually has — where to look — without dressing it
                up as a command we withheld on purpose. An az-only rotor is
                never sent an elevation either, and must not print one. */}
            ⟳ {track.state === 'armed' ? 'armed' : 'tracking'} {track.name} ·{' '}
            {track.azDeg == null
              ? `rises az ${Math.round(track.aosAzDeg)}°`
              : `cmd az ${Math.round(track.azDeg)}° ${track.elDeg == null ? '(az only)' : `el ${Math.round(track.elDeg)}°`}`}
            <button onClick={disarmTrack} title="Stop auto-tracking (rotor halts)">■ stop</button>
          </span>
        )}
        {onPopOut && (
          <button className="pane-popout" onClick={onPopOut} title="Open in its own window">⧉</button>
        )}
      </header>

      {!gridSet ? (
        <div className="sats-empty">
          Set your grid square (Settings ▸ Station) first — passes are computed over
          YOUR location, and without a locator there is nothing honest to show.
        </div>
      ) : favs.size === 0 ? (
        <div className="sats-empty">
          No favorites yet — star birds in the list on the right; the schedule, best-pass
          picks, and alarms all run off your ★ set (the S.A.T. workflow).
        </div>
      ) : upcoming.length === 0 ? (
        <div className="sats-empty">
          No upcoming passes for your favorites in the next {SCHEDULE_HOURS} h
          {view == null
            ? ' — waiting for orbital elements (first fetch needs the network once)'
            : ' (birds whose elements are older than 30 days are excluded until a refresh)'}
          .
        </div>
      ) : (
        <>
          <section className="sats-best">
            <h2>Your best passes (24 h)</h2>
            {best.map((p) => (
              <button
                key={`${p.name}-${p.aosUnix}`}
                className={`sats-best-row${p.aosUnix <= nowSecs ? ' live' : ''}`}
                onClick={() => setSelected(p.name)}
                title="Open this bird's detail"
              >
                <b>{p.name}</b> {whyLine(p, nowSecs)}
              </button>
            ))}
          </section>

          <section className="sats-sched">
            <h2>Schedule — favorites, next {SCHEDULE_HOURS} h</h2>
            <table>
              <thead>
                <tr>
                  <th>★</th>
                  {schedTh('Bird', 'bird')}
                  {schedTh('AOS local', 'aos')}
                  <th></th>
                  {schedTh('Max el', 'el')}
                  {schedTh('Dur', 'dur')}
                  <th>Path</th>
                  {schedTh('Status', 'status')}
                  <th>⏰</th>{rotorOn && <th></th>}
                </tr>
              </thead>
              <tbody>
                {upcoming.map((p) => {
                  const inPass = p.aosUnix <= nowSecs
                  const armed = p.name.toUpperCase() in alarms
                  return (
                    <tr
                      key={`${p.name}-${p.aosUnix}`}
                      className={`${selected === p.name ? 'sel' : ''}${inPass ? ' live' : ''}`}
                      onClick={() => setSelected(p.name)}
                    >
                      <td>
                        <button
                          className={`sat-star${favs.has(p.name.toUpperCase()) ? ' on' : ''}`}
                          onClick={(e) => {
                            e.stopPropagation()
                            onToggleFav(p.name)
                          }}
                          title="Unstar removes the bird from this schedule and disarms its alarm"
                        >
                          ★
                        </button>
                      </td>
                      <td className="sat-name">{p.name}</td>
                      <td>{hhmm(p.aosUnix)}</td>
                      <td className="sat-count">{countdown(p, nowSecs)}</td>
                      <td>{Math.round(p.maxElDeg)}°</td>
                      <td>{Math.max(1, Math.round((p.losUnix - p.aosUnix) / 60))} m</td>
                      <td>{wind8(p.aosAzDeg)}→{wind8(p.losAzDeg)}</td>
                      <td>
                        {p.status === 'alive' && <span className="sat-chip alive" title="SatNOGS community reports it transmitting">alive</span>}
                        {(p.status === 'dead' || p.status === 're-entered') && (
                          <span className="sat-chip dead" title="SatNOGS reports it silent/re-entered — geometry still shown, working it is unlikely">{p.status}</span>
                        )}
                      </td>
                      <td>
                        <button
                          className={`sat-bell${armed ? ' on' : ''}`}
                          onClick={(e) => {
                            e.stopPropagation()
                            onToggleAlarm(p.name)
                          }}
                          title={armed ? 'Alarm armed — click to disarm' : 'Wake me before this bird rises (per-bird, survives restarts)'}
                        >
                          ⏰
                        </button>
                        {armed && (
                          <select
                            className="sat-lead"
                            value={alarms[p.name.toUpperCase()].leadMin}
                            onClick={(e) => e.stopPropagation()}
                            onChange={(e) => {
                              setSatAlarmLead(p.name, Number(e.target.value))
                              setAlarms(satAlarmMap())
                            }}
                            title="Lead time before AOS"
                          >
                            {[5, 15, 30, 60].map((m) => (
                              <option key={m} value={m}>−{m}m</option>
                            ))}
                          </select>
                        )}
                      </td>
                      {rotorOn && (
                        <td>
                          {track?.name === p.name && Math.abs(track.aosUnix - p.aosUnix) <= 180 ? (
                            <button className="sat-track on" onClick={(e) => { e.stopPropagation(); disarmTrack() }}>■</button>
                          ) : (
                            <button
                              className="sat-track"
                              onClick={(e) => {
                                e.stopPropagation()
                                armTrack(p.name, p.aosUnix)
                              }}
                              title="Arm auto-track for THIS pass: 5 min before AOS the rotor slews to the rise azimuth, then follows az/el until LOS"
                            >
                              ⟳ track
                            </button>
                          )}
                        </td>
                      )}
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </section>
        </>
      )}

      <aside className="sats-side">
        {selected && detail && (
          <section className="sats-detail">
            <h2>
              {detail.name}
              {detail.norad != null && <span className="sat-norad"> · NORAD {detail.norad}</span>}
              {detail.status && <span className={`sat-chip ${detail.status === 'alive' ? 'alive' : 'dead'}`}>{detail.status}</span>}
            </h2>
            {/* THE PASS, FIRST. The sky dome is the instrument an operator reads
                during a pass, so it sits above the globe and grows to the full
                column while the bird is up; the globe stays the "where is it
                over the earth" view underneath. */}
            {detail.pass ? (
              <>
                <SkyDome
                  name={detail.name}
                  pass={detail.pass}
                  track={detail.passTrack}
                  rotor={detailTrack}
                  nowSecs={nowSecs}
                />
                <PassTimeline pass={detail.pass} tcaUnix={tcaUnix} nowSecs={nowSecs} />
                {detailTrack && (
                  <>
                    <DopplerReadout
                      rotor={detailTrack}
                      dopplerOn={dopplerOn}
                      vfoMap={vfoMap}
                      held={heldIndex != null || detailTrack.transponder != null}
                    />
                    {/* The strip goes under the readout: the readout says what
                        the radio is tuned to, the strip says where that puts
                        the operator inside the passband. */}
                    <PassbandStrip rotor={detailTrack} />
                  </>
                )}
              </>
            ) : (
              <div className="sat-passline">no pass over you in the next 24 h</div>
            )}
            <div
              /* Square-ish and growing with the column rather than pinned at
                 260px: a globe is only readable at size, and this is the one
                 the operator opens to read a single pass. Cap in --vh-eff, not
                 raw vh: this renders INSIDE .app's zoom:var(--ui-zoom), where
                 raw viewport units are wrong by the zoom factor. */
              style={{
                width: '100%',
                aspectRatio: '1 / 1',
                maxHeight: 'calc(0.52 * var(--vh-eff, 100vh))',
                minHeight: 260,
                borderRadius: 8,
                overflow: 'hidden',
                border: '1px solid var(--border)',
              }}
            >
              <MapView
                embedded={{ focusSat: detail.name }}
                myGrid={myGrid}
                theme={theme}
                stations={noStations}
                prop={null}
                selectedCall={null}
                onSelectCall={noSelectCall}
                needByCall={noNeeds}
                onSelectSat={selectSatInBox}
              />
            </div>
            {detail.transmitters.length > 0 ? (
              <>
                <table className="sat-freqs">
                  <thead>
                    <tr>
                      <th>Work</th>
                      <th>Transponder</th>
                      <th>Down MHz</th>
                      <th>Up MHz</th>
                      <th>Mode</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr>
                      <td>
                        <input
                          type="radio"
                          name="sat-transponder"
                          checked={heldIndex == null}
                          onChange={() => pickTransponder(detail.name, null)}
                          aria-label="Work no transponder — leave the dial to me"
                        />
                      </td>
                      <td colSpan={4}>None — leave the dial to me</td>
                    </tr>
                    {tpRows.map(({ t, aliveIndex }, i) => (
                      <tr key={i} className={t.alive ? '' : 'off'}>
                        <td>
                          {aliveIndex != null && (
                            <input
                              type="radio"
                              name="sat-transponder"
                              checked={heldIndex === aliveIndex}
                              onChange={() => pickTransponder(detail.name, aliveIndex, t.description)}
                              aria-label={`Work ${t.description}`}
                            />
                          )}
                        </td>
                        <td title={t.description}>
                          {t.alive ? '●' : '○'} {t.description}
                          {t.invert && (
                            <span
                              className="sat-invert"
                              title="Inverting linear transponder: tune the downlink UP and your uplink goes DOWN, and the sidebands swap (LSB up, USB down)."
                            >
                              INVERTING
                            </span>
                          )}
                        </td>
                        <td>{fmtLeg(t.downlinkLowHz, t.downlinkHighHz)}</td>
                        <td>{fmtLeg(t.uplinkLowHz, t.uplinkHighHz)}</td>
                        <td>{legModes(t)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {heldIndex != null && (
                  <div className={`sat-tp-state${dopplerLive ? '' : ' warn'}`}>
                    {!dopplerOn
                      ? 'Doppler is off, so nothing is being tuned. Turn it on in Settings ▸ Radio ▸ Satellite Doppler.'
                      : vfoMap === 'off'
                        ? 'VFO mapping is Off, so nothing is being tuned. Pick a mapping in Settings ▸ Radio ▸ Satellite Doppler.'
                        : 'Doppler tunes this transponder while auto-track is following the pass.'}
                  </div>
                )}
                {tuned && tuned.name !== detail.name && (
                  <div className="sat-tp-state warn">
                    Doppler holds a transponder on {tuned.name}. Picking one here takes the dial
                    from it.
                  </div>
                )}
                <div className="sats-credit">frequencies & status: SatNOGS DB (CC-BY-SA 4.0)</div>
              </>
            ) : (
              <div className="sats-credit">
                {detail.dataFetchedAt == null
                  ? 'no transponder data yet — fetched from SatNOGS DB when online'
                  : 'no transmitters listed for this bird (SatNOGS DB)'}
              </div>
            )}
          </section>
        )}

        <section className="sats-favmgr">
          <h2>Birds ({allBirds.length})</h2>
          <input
            className="sats-search"
            type="text"
            placeholder="search…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            spellCheck={false}
          />
          <ul>
            {allBirds.map((n) => (
              <li key={n} className={selected === n ? 'sel' : ''}>
                <button
                  className={`sat-star${favs.has(n.toUpperCase()) ? ' on' : ''}`}
                  onClick={() => onToggleFav(n)}
                  title="★ favorites drive the schedule, the map emphasis, and alarms"
                >
                  ★
                </button>
                <button className="sat-pick" onClick={() => setSelected(n)}>{n}</button>
              </li>
            ))}
            {view == null && <li className="sats-empty">no elements yet — first fetch needs the network once</li>}
          </ul>
        </section>
      </aside>
    </div>
  )
}
