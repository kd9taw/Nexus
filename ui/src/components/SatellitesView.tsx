// Satellites — the top-level section: WHEN to try WHICH bird, favorites first.
// Modeled on the workflow of the field's standard tools (CSN S.A.T., SatPC32,
// Look4Sat): a favorites set drives everything (declutter + prediction focus),
// a ranked "your best passes" strip answers the when/which question in one
// line, the 48 h schedule carries countdowns + ⏰ pass alarms, and the detail
// zone shows the pass on the SKY DOME (hero when a pass is live) with SatNOGS
// frequencies/status (community-measured truth — absent when offline, never
// guessed). The Connect "Satellite Passes" pane stays as the compact glance
// view; this is the planning surface.
//
// 2026-07-31 (UX litigation): "Work this pass" is ONE control that runs the
// whole chain (select → auto-pick a workable transponder → arm), the readiness
// rail renders the chain AS a chain (four gates, each fixable in place), and
// tracking is rotor-LESS capable — the backend runs the pass clock + Doppler
// without a rotator, and the arm affordance renders for everyone. Schedule
// rows carry Phase 2 `earn` (needed grids/entities) in the app's one need-chip
// vocabulary. Arming stays the operator's click, every time: nothing here
// moves a rotor or takes a dial on its own.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type {
  NeedTag,
  SatBinding,
  SatDetail,
  SatPass,
  SatPassEarn,
  SatTrackStatus,
  SatVfoMap,
  SatView,
  Settings,
  Station,
} from '../types'
import {
  getSatellites,
  getSatPassNeeds,
  getSatDetail,
  getSettings,
  setSettings,
  setSatTransponder,
  getSatTransponder,
  startSatTrack,
  stopSatTrack,
  getSatTrackStatus,
} from '../api'
import { NEED_CHIP } from '../features/needVisuals'
import { SAT_VFO_MAPS, satVfoLabel } from '../features/satVfo'
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

/** What a pass would EARN, in the app's one need-chip vocabulary (NEED_CHIP +
 * the --need-* palette) so the Satellites section reads like the Needed board.
 * Counts are complete; the sample names (≤8, backend-capped) live in the
 * tooltip. A pass that earns nothing renders NOTHING — absent, not zero. */
function EarnChips({ earn }: { earn?: SatPassEarn | null }) {
  if (!earn || (earn.newEntities === 0 && earn.newGrids === 0)) return null
  const gridMore = earn.newGrids - earn.gridSample.length
  const entityMore = earn.newEntities - earn.entitySample.length
  return (
    <span className="sat-earn" data-testid="sat-earn">
      {earn.newEntities > 0 && (
        <span
          className={`need-chip need-${NEED_CHIP.NewEntity.cls}`}
          title={`${earn.newEntities} never-worked ${earn.newEntities === 1 ? 'entity' : 'entities'} reachable through this pass's footprint: ${earn.entitySample.join(', ')}${entityMore > 0 ? ` +${entityMore} more` : ''}`}
        >
          {NEED_CHIP.NewEntity.label}
          {earn.newEntities > 1 ? ` ×${earn.newEntities}` : ''}
        </span>
      )}
      {earn.newGrids > 0 && (
        <span
          className={`need-chip need-${NEED_CHIP.NewGrid.cls}`}
          title={`${earn.newGrids} new Satellite VUCC grid${earn.newGrids === 1 ? '' : 's'} reachable through this pass's footprint: ${earn.gridSample.join(' ')}${gridMore > 0 ? ` +${gridMore} more` : ''}`}
        >
          {NEED_CHIP.NewGrid.label} ×{earn.newGrids}
        </span>
      )}
    </span>
  )
}

/** A 28 px SKETCH of the pass's shape (Look4Sat's pick-by-shape idea): rim,
 * plus a quadratic arc whose endpoints (AOS/LOS azimuth) and apex (max el on
 * the mid-azimuth) are real; the curve between them is interpolation, which is
 * why this is aria-hidden decoration beside the why-line's real numbers — the
 * detail dome draws the SGP4 truth. */
function PassArcMini({ pass }: { pass: SatPass }) {
  const R = 12
  const C = 14
  const pt = (azDeg: number, elDeg: number): [number, number] => {
    const r = (R * (90 - Math.max(0, Math.min(90, elDeg)))) / 90
    const a = (azDeg * Math.PI) / 180
    return [C + r * Math.sin(a), C - r * Math.cos(a)]
  }
  const [x0, y0] = pt(pass.aosAzDeg, 0)
  const [x1, y1] = pt(pass.losAzDeg, 0)
  let dAz = pass.losAzDeg - pass.aosAzDeg
  if (dAz > 180) dAz -= 360
  if (dAz < -180) dAz += 360
  const [mx, my] = pt(pass.aosAzDeg + dAz / 2, pass.maxElDeg)
  // Quadratic control point that puts the curve THROUGH the apex sample.
  const cx = 2 * mx - (x0 + x1) / 2
  const cy = 2 * my - (y0 + y1) / 2
  return (
    <svg className="sat-arcmini" width={28} height={28} viewBox="0 0 28 28" aria-hidden>
      <circle cx={C} cy={C} r={R} className="sat-arcmini-rim" />
      <path
        className="sat-arcmini-arc"
        d={`M${x0.toFixed(1)},${y0.toFixed(1)} Q${cx.toFixed(1)},${cy.toFixed(1)} ${x1.toFixed(1)},${y1.toFixed(1)}`}
      />
      <circle cx={x0} cy={y0} r={1.8} className="sat-arcmini-aos" />
    </svg>
  )
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

  // The Antenna row exists only when this track HAS a rotor half — the DTO
  // `mode` is the engine's own answer. A doppler-only/pass-only track will
  // never command a rotor: "armed — no rotor command sent yet" there promised
  // an antenna event that cannot come (the rail's Rotor row already carries
  // the "no rotor in this track" story). Absent is absent, never a claim.
  const ghostText =
    rotor == null || (rotor.mode !== 'rotor+doppler' && rotor.mode !== 'rotor-only')
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
  earn,
}: {
  pass: SatPass
  tcaUnix: number | null
  nowSecs: number
  /** Phase 2 earn for THIS pass (matched by bird + AOS); absent = no lane. */
  earn?: SatPassEarn | null
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
      {/* Phase 2's visual home (sat-visual-design §3.4): what the pass is WORTH
          — the needed grids its footprint crosses — on the timeline itself, not
          buried in a sort order. Sample names print (backend caps at 8), the
          remainder is said honestly, and a pass that earns nothing has no lane. */}
      {earn && (earn.newGrids > 0 || earn.newEntities > 0) && (
        <div className="sat-tl-earn" data-testid="sat-tl-earn">
          <EarnChips earn={earn} />
          {earn.gridSample.length > 0 && (
            <span className="sat-tl-earn-grids">
              {earn.gridSample.join(' ')}
              {earn.newGrids > earn.gridSample.length
                ? ` +${earn.newGrids - earn.gridSample.length} more`
                : ''}
            </span>
          )}
        </div>
      )}
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
  txMode,
}: {
  rotor: SatTrackStatus
  dopplerOn: boolean
  vfoMap: SatVfoMap
  held: boolean
  /** The sideband the ENGINE declares for the TX (split) leg — the DTO's
   * `txMode`, i.e. `Engine::sat_tx_mode`'s own per-tick answer, so this shows
   * exactly what the radio loop writes and nothing else. Null = the engine
   * commands nothing (legs share a mode, a mapping without the uplink,
   * Doppler off, or the operator took the mode back) — and then nothing is
   * claimed. Never derived from the SatNOGS record here: a second derivation
   * of the command is how a display claims a write the radio never gets. */
  txMode?: string | null
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
              {txMode != null && (
                <span
                  className="sat-dop-txmode"
                  title={`The TX (split) VFO's sideband — this bird's uplink runs ${txMode} while the downlink does not, and the radio's TX leg is set to match. Commanded by the engine with the Doppler tuning; shown here so a swapped sideband is never a surprise.`}
                >
                  {' '}
                  {txMode}
                </span>
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

/** SatNOGS `type` in operator words, for the chooser cards. Unknown = no chip
 * — never guess a kind. */
const kindWord = (k: string | null) =>
  k === 'Transmitter'
    ? 'beacon'
    : k === 'Transponder'
      ? 'linear'
      : k === 'Transceiver'
        ? 'FM repeater'
        : null

/* ======================= the readiness rail (top-5 ①) =======================
 * The arming chain rendered AS a chain: four gates, always four rows, each
 * not-ready row carrying its own fix instead of prose pointing at another
 * section. Filled ● / hollow ○ carries ready-state by SHAPE (the codebase
 * idiom — it survives greyscale and colourblindness).
 *
 * The two Settings switches (satDoppler, satVfoMap) are MIRRORED here live —
 * Settings ▸ Radio stays canonical, writes go read-modify-write through
 * getSettings → setSettings (the OperateCockpit precedent) so nothing else in
 * the settings object is clobbered. The rail never flips a default itself:
 * every change is the operator's click on this pass, right now.
 *
 * Layout: content-height (fit) inside .sats-detail — the .sats-side scroller
 * owns overflow; the rail is never a grower. */
/** ● ready · ○ not ready (fixable) · — absent (nothing to be ready ABOUT).
 * ○ promises "this gate can be passed"; a station with no rotator can never
 * pass the Rotor gate and never needs to, and a hollow circle there reads as
 * permanently broken. Absence gets an absent mark — the sentinel lesson,
 * applied to a glyph. */
function railDot(ok: boolean, absent = false) {
  return (
    <span className={`sat-rail-dot${ok ? ' ok' : ''}`} aria-hidden>
      {absent ? '—' : ok ? '●' : '○'}
    </span>
  )
}

function TrackRail({
  track,
  rotorOn,
  dopplerOn,
  vfoMap,
  heldDesc,
  heldInverting,
  autoPicked,
  canPick,
  nowSecs,
  onStop,
  onDopplerOn,
  onVfoMap,
  onGoToPicker,
  scrollRef,
}: {
  /** The live track for THIS bird (already name-filtered by the caller). */
  track: SatTrackStatus
  /** A rotator is configured in Settings (model or host). */
  rotorOn: boolean
  dopplerOn: boolean
  vfoMap: SatVfoMap
  /** The held transponder's description (engine truth first), or null. */
  heldDesc: string | null
  heldInverting: boolean
  /** The hold came from "Work this pass", not a hand pick — disclosed. */
  autoPicked: boolean
  /** The bird has a transponder chooser to go to (it renders only when the
   * bird lists transmitters) — without one the pick button is disabled WITH
   * its reason, never an enabled control that silently does nothing. */
  canPick: boolean
  nowSecs: number
  onStop: () => void
  onDopplerOn: () => void
  onVfoMap: (v: SatVfoMap) => void
  onGoToPicker: () => void
  scrollRef: React.RefObject<HTMLDivElement>
}) {
  // The rotor half of the track is fixed at arm time (DTO `mode`); the Doppler
  // rows below mirror the LIVE settings pair, which is what the engine re-reads
  // each tick — so a toggle here moves both the behaviour and the row together.
  const rotorInTrack = track.mode === 'rotor+doppler' || track.mode === 'rotor-only'
  const dopplerLive = dopplerOn && vfoMap !== 'off'
  const minsToAos = Math.max(1, Math.round((track.aosUnix - nowSecs) / 60))
  const minsToLos = Math.max(0, Math.round((track.losUnix - nowSecs) / 60))
  const passText =
    track.state === 'armed'
      ? nowSecs < track.aosUnix
        ? `armed — AOS in ${minsToAos} min`
        : 'armed'
      : track.state === 'prepositioning'
        ? 'slewing to the AOS azimuth'
        : `IN PASS — ${minsToLos} min to LOS`
  const rotorText = !rotorInTrack
    ? rotorOn
      ? 'not in this track — re-arm to take the rotor'
      : 'no rotator configured — Settings ▸ Rig Control'
    : track.azDeg != null
      ? `tracking · cmd az ${deg(track.azDeg)}${track.elDeg == null ? ' (az only)' : ` el ${deg(track.elDeg)}`}`
      : 'armed — takes the rotor 5 min before AOS'
  return (
    <div className="sat-rail" data-testid="sat-rail" ref={scrollRef}>
      <div className="sat-rail-row">
        {railDot(true)}
        <span className="sat-rail-name">Pass</span>
        <span className="sat-rail-state">{passText}</span>
        <button
          className="sat-rail-fix"
          onClick={onStop}
          title="Stop this track (rotor halts if it holds one; the dial is handed back)"
        >
          ■ stop
        </button>
      </div>
      <div className="sat-rail-row">
        {railDot(rotorInTrack, !rotorInTrack && !rotorOn)}
        <span className="sat-rail-name">Rotor</span>
        <span className="sat-rail-state">{rotorText}</span>
      </div>
      <div className="sat-rail-row">
        {railDot(heldDesc != null)}
        <span className="sat-rail-name">Transponder</span>
        <span className="sat-rail-state">
          {heldDesc != null ? (
            <>
              {heldDesc}
              {heldInverting && <span className="sat-invert">INVERTING</span>}
              {autoPicked && <span className="sat-rail-auto"> picked for you</span>}
            </>
          ) : (
            'none — the dial stays yours'
          )}
        </span>
        <button
          className="sat-rail-fix"
          onClick={onGoToPicker}
          disabled={!canPick}
          title={
            canPick
              ? 'Go to the transponder chooser below'
              : 'No transmitters listed for this bird (SatNOGS) — nothing to pick'
          }
        >
          {heldDesc != null ? 'change' : 'pick'}
        </button>
      </div>
      <div className="sat-rail-row">
        {railDot(dopplerLive)}
        <span className="sat-rail-name">Doppler</span>
        <span className="sat-rail-state">
          {dopplerLive
            ? `on — ${satVfoLabel(vfoMap)}`
            : !dopplerOn
              ? 'off — nothing is being tuned'
              : 'VFO map is Off — Doppler writes nothing to the radio'}
        </span>
        {!dopplerOn && (
          <button
            className="sat-rail-fix"
            onClick={onDopplerOn}
            title="Turn Satellite Doppler on (the same switch as Settings ▸ Radio ▸ Satellite Doppler)"
          >
            turn on
          </button>
        )}
        <select
          className="sat-rail-vfo"
          value={vfoMap}
          onChange={(e) => onVfoMap(e.target.value as SatVfoMap)}
          aria-label="Satellite VFO mapping"
          title="Match this to how your radio is wired. A wrong mapping transmits on your own downlink — into the satellite's output passband, on top of everyone else working the bird. Off is the default and writes nothing to the radio."
        >
          {SAT_VFO_MAPS.map((m) => (
            <option key={m.value} value={m.value}>
              {m.label}
            </option>
          ))}
        </select>
      </div>
    </div>
  )
}

/* ==================== which rig will move (the radio binding) ===============
 * Operator field report: "I have my 9700 selected, but in the sat area, it's
 * not selecting the radio." The section showed nothing about radios at all —
 * and behind it, nothing in the satellite path ever ROUTED, so Doppler drove
 * whichever rig happened to be active. A pick now routes on band+mode class
 * exactly as a repeater tune does; this is the one line that says where it went.
 *
 * Reports what was DONE — and "done" means the RIG ACKNOWLEDGED it, not that
 * the engine queued it: a confirmed leg prints plain, a leg still awaiting the
 * radio loop's wire acknowledgment prints with a trailing "…" (the honest
 * tuning-in-flight state the 2 s read-back resolves), and the honest reason
 * takes their place when nothing moved. Ready-state by SHAPE (filled ● /
 * hollow ○), the codebase idiom — filled only once every requested leg is
 * confirmed on the wire.
 *
 * The override is the app's OWN one — peg-lock, the same switch behind the
 * TopBar RadioSwitcher's 🔒 — reachable where the operator is, written
 * read-modify-write like the two Doppler switches on the rail below.
 *
 * Layout: content-height, never a grower (ui-layout §2), shares the rail's box. */
function SatRadioBinding({
  binding,
  pegged,
  onTogglePeg,
}: {
  binding: SatBinding
  pegged: boolean
  onTogglePeg: (on: boolean) => void
}) {
  const leg = (confirmed: number | null, pending: number | null, arrow: string) =>
    confirmed != null
      ? `${confirmed.toFixed(3)} ${arrow}`
      : pending != null
        ? `${pending.toFixed(3)} ${arrow} …`
        : null
  const legs = [
    leg(binding.downlinkMhz, binding.pendingDownlinkMhz, '↓'),
    leg(binding.uplinkMhz, binding.pendingUplinkMhz, '↑'),
  ].filter((s) => s != null)
  const confirmed = binding.downlinkMhz != null || binding.uplinkMhz != null
  const pending = binding.pendingDownlinkMhz != null || binding.pendingUplinkMhz != null
  return (
    <div className="sat-bind" data-testid="sat-radio-binding">
      <div className="sat-rail-row">
        {railDot(confirmed && !pending)}
        <span className="sat-rail-name">Radio</span>
        <span className="sat-rail-state">
          {/* An empty band = a refusal that returned BEFORE routing (band-plan
              miss): no rig was resolved and no class chosen, so the reason
              stands alone — never "this radio · · SSB" beside a rig that was
              never picked. */}
          {binding.band !== '' && (
            <>
              {binding.radioName || 'this radio'}
              <span className="sat-bind-why">
                {' '}
                · {binding.band} · {binding.fm ? 'FM' : 'SSB'}
              </span>
            </>
          )}
          {/* A note can accompany surviving legs (e.g. the split refused while
              the dial landed) — print both rather than letting either win. */}
          {legs.length > 0
            ? ` — ${legs.join(' · ')} MHz${binding.note ? ` — ${binding.note}` : ''}`
            : binding.band !== ''
              ? ` — ${binding.note ?? ''}`
              : (binding.note ?? '')}
        </span>
        <button
          className={`sat-rail-fix${pegged ? ' on' : ''}`}
          aria-pressed={pegged}
          onClick={() => onTogglePeg(!pegged)}
          title={
            pegged
              ? 'Peg-lock is ON — this bird stays on the active radio; band+mode routing will not hand it to another rig. Click to unlock.'
              : 'Peg-lock is OFF — a pick routes to the radio that owns the band and mode class. Click to pin the active radio instead.'
          }
        >
          {pegged ? '🔒 pinned' : '🔓 pin this radio'}
        </button>
      </div>
    </div>
  )
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
  // The transponder handed to the Doppler engine, and which bird it belongs
  // to. MIRRORS the engine via the get_sat_transponder read-back on the 2 s
  // poll below — the hold is released backend-side (LOS handback, live-track
  // stop), and trusting only the last local click kept the Transponder gate
  // green while the next "Work this pass" skipped its re-pick and armed a
  // pass that tuned nothing. `auto` = the hold came from "Work this pass",
  // disclosed on the card (local-only; the wire doesn't carry it).
  const [tuned, setTuned] = useState<{ name: string; index: number; auto?: boolean } | null>(null)
  // Birds the operator explicitly said "None — leave the dial to me" for.
  // "Work this pass" must never re-take a dial that was deliberately handed
  // back — the None pick is a consent statement, not an empty slot. Per BIRD:
  // picking a transponder on one bird withdraws nothing said about another.
  const [dialOptOut, setDialOptOut] = useState<Set<string>>(() => new Set())
  // A set_sat_transponder call is in flight: the 2 s read-back must not race
  // it (a poll answered pre-pick would briefly erase the fresh selection).
  const pickBusy = useRef(false)
  // A Settings write (rail fix buttons) is in flight: same reason, for the
  // dopplerOn/vfoMap mirrors.
  const settingsWriteBusy = useRef(false)
  // Dead transmitters are collapsed behind "show N inactive"; reset per bird.
  const [showDead, setShowDead] = useState(false)
  const railRef = useRef<HTMLDivElement>(null)
  const pickerRef = useRef<HTMLDivElement>(null)
  // The previous poll's track, for the LOS-handback notice: the backend
  // releases the hold and the track at LOS, and before this the rail, binding
  // line and header badge simply vanished on the next 2 s poll — the one
  // ownership change in the section with zero notification. Nulled on a manual
  // stop (the operator did that; there is nothing to announce).
  const lastTrack = useRef<SatTrackStatus | null>(null)
  // Post-pass rotor policy mirror (read with the settings the poll already
  // fetches): at LOS the mast may be about to move on its own — park/ready —
  // and the handback notice must say so.
  const rotPostPassRef = useRef('stop')
  // "Work this pass" wants the rail scrolled into view once it exists.
  const wantRailScroll = useRef(false)
  const [dopplerOn, setDopplerOn] = useState(false)
  const [vfoMap, setVfoMap] = useState<SatVfoMap>('off')
  // Which rig the engine's held pick bound to, and what it actually wrote.
  // ENGINE truth off the same read-back the hold uses — a binding drawn from
  // the last local click would name a rig the engine no longer drives (the
  // hold is released backend-side at LOS and on a live-track stop).
  const [binding, setBinding] = useState<SatBinding | null>(null)
  // Peg-lock mirror: the app-wide routing override, surfaced on the binding
  // line because that is where the operator asks "why THAT radio?".
  const [pegged, setPegged] = useState(false)
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
  // barely moves in minutes; TLE refreshes are half-daily). Fetched through
  // get_sat_pass_needs — the same rows get_sat_schedule returns (same names,
  // hours, backscan) with Phase 2 `earn` stamped on each; computed on demand
  // backend-side, so this poll is the only thing that pays for it.
  const favKey = useMemo(() => [...favs].sort().join(','), [favs])
  useEffect(() => {
    let live = true
    const names = favKey === '' ? [] : favKey.split(',')
    if (names.length === 0) {
      setSchedule([])
      return
    }
    const load = () =>
      getSatPassNeeds(names, SCHEDULE_HOURS)
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
    setShowDead(false) // the collapse is per bird
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
  // The live track status — polled UNCONDITIONALLY while the section is open.
  // It used to be gated on a configured rotor, which hid the whole tracking arc
  // (pass clock, Doppler, geometry) from the rotor-less operator the backend
  // now serves; the DTO's `mode` says honestly which surfaces are driven.
  //
  // The same 2 s tick also carries two READ-BACKS this section must not drift
  // from: the engine's transponder hold (released backend-side at LOS and on a
  // live-track stop — a stale local mirror showed a green Transponder gate for
  // a hold that no longer existed) and the two Settings ▸ Radio switches the
  // readiness rail mirrors (a pop-out Settings window can flip them mid-pass;
  // a mount-time snapshot showed "off — nothing is being tuned" beside a
  // header badge saying Doppler was live).
  useEffect(() => {
    let live = true
    // The 2 s tick can lap a slow answer, and a lapped answer applied late
    // re-seeds `lastTrack` with a dead pass as "live" — the next null poll
    // would announce the SAME handback twice. Answers carry their issue
    // number; a straggler that lost the race is dropped, never applied.
    let issued = 0
    let applied = 0
    const load = () => {
      const seq = ++issued
      getSatTrackStatus()
        .then((t) => {
          if (!live || seq < applied) return
          applied = seq
          const prev = lastTrack.current
          lastTrack.current = t
          // LOS HANDBACK (the release happened backend-side — this reports
          // what was DONE, it acts on nothing): a live track that vanishes
          // just past its LOS ended by running out of pass, not by a click.
          // A manual stop nulls the ref before this comparison can fire, and
          // an armed track dropped before AOS has a future losUnix.
          if (prev != null && t == null) {
            const now = Math.floor(Date.now() / 1000)
            if (now >= prev.losUnix && now - prev.losUnix < 300) {
              const rotorHalf = prev.mode === 'rotor+doppler' || prev.mode === 'rotor-only'
              pushToast(
                `${prev.name} pass complete — LOS.` +
                  (prev.transponder != null ? ' Dial handed back.' : '') +
                  // Only when the mast is about to move ON ITS OWN — the
                  // "stop" policy leaves it where the pass finished, and a
                  // stationary rotor needs no announcement.
                  (rotorHalf && rotPostPassRef.current === 'park' ? ' Rotor parking.' : '') +
                  (rotorHalf && rotPostPassRef.current === 'ready'
                    ? ' Rotor moving to the ready position.'
                    : ''),
                'info',
                6000,
              )
            }
          }
          setTrack(t)
        })
        .catch(() => {})
      getSatTransponder()
        .then((h) => {
          if (!live || pickBusy.current) return
          setBinding(h?.binding ?? null)
          setTuned((cur) => {
            if (h == null || h.index == null) return null
            if (cur && cur.name === h.name && cur.index === h.index) return cur // keep `auto`
            return { name: h.name, index: h.index }
          })
        })
        .catch(() => {})
      if (!settingsWriteBusy.current) {
        getSettings()
          .then((s: Settings) => {
            if (!live || settingsWriteBusy.current) return
            setDopplerOn(!!s.satDoppler)
            setVfoMap(s.satVfoMap ?? 'off')
            setPegged(!!s.radioPegged)
            rotPostPassRef.current = s.rotPostPass ?? 'stop'
          })
          .catch(() => {})
      }
    }
    load()
    const id = window.setInterval(load, 2000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [])

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
  type SchedSortKey = 'bird' | 'aos' | 'el' | 'dur' | 'status' | 'need'
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
        case 'need':
          // The Phase 2 sort key (an ATNO outranks any number of grids —
          // backend-documented). A row with no earn sinks, never NaNs.
          return p.earn?.score ?? -1
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
  // Phase 2 earn for the pass on screen: matched against the schedule row by
  // bird + AOS (±180 s, the same tolerance the track-row match uses — TLE
  // refreshes nudge a modelled AOS by seconds, not minutes). No match = no
  // lane; earn is stamped only by get_sat_pass_needs and never guessed here.
  const detailEarn = useMemo(() => {
    const pass = detail?.pass
    if (pass == null) return null
    const row = schedule.find(
      (p) => p.name === detail!.name && Math.abs(p.aosUnix - pass.aosUnix) <= 180,
    )
    return row?.earn ?? null
  }, [schedule, detail])
  // HORIZON MISMATCH FALLBACK: get_sat_detail's pass window is 24 h; the
  // schedule sees SCHEDULE_HOURS (48 h). A clicked 24–48 h row lands here with
  // detail.pass null, and "no pass over you in the next 24 h" directly under a
  // row promising that exact pass reads as a contradiction. Until the two
  // horizons share one backend constant, the passline cites the schedule's own
  // AOS — known data, never a guess — and makes no 24 h claim beside it:
  // detail refreshes a minute behind the schedule, so at the window boundary
  // that claim briefly argues with its own countdown. (Favorites only; other
  // birds fall back to the plain line, which is then simply true.)
  const nextBeyond = useMemo(() => {
    if (detail == null || detail.pass != null) return null
    return (
      schedule
        .filter((p) => p.name === detail.name && p.aosUnix > nowSecs)
        .sort((a, b) => a.aosUnix - b.aosUnix)[0] ?? null
    )
  }, [schedule, detail, nowSecs])
  // The held transponder RECORD — engine truth first (the DTO index is what
  // the engine holds; the local pick covers the pre-arm window).
  const heldT =
    detail == null
      ? null
      : detailTrack?.transponderIndex != null
        ? (detail.transmitters[detailTrack.transponderIndex] ?? null)
        : heldIndex != null
          ? (detail.transmitters[heldIndex] ?? null)
          : null
  // The RECORD's per-leg sidebands differing (SatNOGS data) — used only to
  // decide whether the chooser's TX-sideband note renders at all, and for the
  // pre-arm FORECAST wording. What the engine actually commands is the DTO's
  // `txMode` (its own answer), never re-derived from the record: the two
  // disagree exactly where it matters (CW/data downlinks, a downlink-only
  // mapping, an operator take-back), and the display must not claim a
  // command the radio never gets.
  const txSwapMode =
    heldT?.uplinkMode != null &&
    heldT.downlinkMode != null &&
    heldT.uplinkMode !== heldT.downlinkMode
      ? heldT.uplinkMode
      : null

  const armTrack = (name: string, aosUnix: number) => {
    startSatTrack(name, aosUnix)
      .then((t) => {
        setTrack(t)
        if (t) {
          // The toast says what THIS track drives — the DTO's `mode` is the
          // engine's own answer, so a rotor-less arm never claims a rotor.
          const doing =
            t.state === 'armed'
              ? t.mode === 'rotor+doppler' || t.mode === 'rotor-only'
                ? 'armed — the rotor stays yours until 5 min before AOS'
                : t.mode === 'doppler-only'
                  ? 'armed — no rotor in this track; Doppler takes the dial at AOS'
                  : 'armed — pass timing only; the dial stays yours'
              : t.state === 'prepositioning'
                ? 'slewing to the AOS azimuth'
                : 'following the pass'
          pushToast(`Pass track ${t.name}: ${doing}`, 'success', 5000)
        } else pushToast('Nothing to track — no matching pass to arm', 'info', 6000)
      })
      .catch((e) => pushToast(`Track failed: ${e instanceof Error ? e.message : e}`, 'error'))
  }
  const disarmTrack = () => {
    stopSatTrack()
      .then(() => {
        // The operator stopped this track — there is no handback to announce.
        lastTrack.current = null
        setTrack(null)
        // Stopping a live track hands the dial back backend-side (the stop
        // command releases the transponder hold) — mirror it now rather than
        // waiting out the next 2 s read-back.
        setTuned(null)
      })
      .catch(() => {})
  }

  /** Hand a transponder to the Doppler engine, or `null` to hand the dial back.
   * `index` is the RAW index into the list `get_sat_detail` returned (dead
   * entries included) — what the backend indexes. The selection is only shown
   * once the call succeeds: a refused pick must not leave the operator
   * believing the radio is under Doppler control. `auto` = picked by "Work
   * this pass", disclosed in the toast and on the card. */
  const pickTransponder = (name: string, index: number | null, label = '', auto = false) => {
    pickBusy.current = true
    return setSatTransponder(name, index)
      .then(() => {
        setTuned(index == null ? null : { name, index, auto })
        // An explicit None is a consent statement "Work this pass" must honor
        // — PER BIRD: an explicit pick re-grants consent for that bird only,
        // never withdrawing a None said about a different one.
        setDialOptOut((prev) => {
          const next = new Set(prev)
          if (index == null) next.add(name)
          else next.delete(name)
          return next
        })
        // What the pick actually TUNED, read straight back: the tune happens
        // backend-side inside set_sat_transponder, and waiting out the 2 s poll
        // to learn whether the radio moved is exactly the uncertainty this line
        // exists to remove. Clearing the pick clears the binding with it.
        //
        // The TOAST rides the same read-back: a pick the engine REFUSED (rig
        // can't reach the band, mapping off, …) used to fire the green
        // "Working …" toast anyway — the command returns Ok on every refusal,
        // its honesty lives in the binding — so the field report's dead pick
        // was announced as a success. The binding note is the toast now.
        if (index == null) {
          setBinding(null)
          pushToast('Transponder cleared — the dial is yours', 'success', 4000)
        } else {
          const working = `Working ${name} ${label}${auto ? ' (picked for you — change below)' : ''}`
          getSatTransponder()
            .then((h) => {
              const b = h?.binding ?? null
              setBinding(b)
              if (b?.note) pushToast(b.note, 'info', 6000)
              else pushToast(working, 'success', 4000)
            })
            // Read-back unavailable: the pick itself succeeded, say that much.
            .catch(() => pushToast(working, 'success', 4000))
        }
        // Re-read the two switches that decide whether this tunes anything —
        // Settings may have changed since this section mounted.
        getSettings()
          .then((s: Settings) => {
            setDopplerOn(!!s.satDoppler)
            setVfoMap(s.satVfoMap ?? 'off')
          })
          .catch(() => {})
      })
      .catch((e) => pushToast(`Transponder not selected: ${e instanceof Error ? e.message : e}`, 'error'))
      .finally(() => {
        pickBusy.current = false
      })
  }

  // Live mirrors of the two Settings ▸ Radio switches the readiness rail can
  // fix in place. Read-modify-write (getSettings → spread → setSettings, the
  // OperateCockpit precedent): the one field changes, nothing else rides along
  // wrong. Local state updates only after the write succeeds — the rail must
  // never show a switch position the store refused.
  const writeDopplerOn = () => {
    settingsWriteBusy.current = true
    getSettings()
      .then((s: Settings) => setSettings({ ...s, satDoppler: true }))
      .then(() => setDopplerOn(true))
      .catch((e) => pushToast(`Doppler setting: ${e instanceof Error ? e.message : e}`, 'error'))
      .finally(() => {
        settingsWriteBusy.current = false
      })
  }
  /** Peg-lock: the app-wide "don't auto-switch radios" override, written the
   * same read-modify-write way as the two switches beside it. Pinning does not
   * re-tune — it changes where the NEXT pick lands, and the line re-reads. */
  const writePegged = (on: boolean) => {
    settingsWriteBusy.current = true
    getSettings()
      .then((s: Settings) => setSettings({ ...s, radioPegged: on }))
      .then(() => setPegged(on))
      .catch((e) => pushToast(`Peg-lock: ${e instanceof Error ? e.message : e}`, 'error'))
      .finally(() => {
        settingsWriteBusy.current = false
      })
  }
  const writeVfoMap = (v: SatVfoMap) => {
    settingsWriteBusy.current = true
    getSettings()
      .then((s: Settings) => setSettings({ ...s, satVfoMap: v }))
      .then(() => setVfoMap(v))
      .catch((e) => pushToast(`VFO mapping: ${e instanceof Error ? e.message : e}`, 'error'))
      .finally(() => {
        settingsWriteBusy.current = false
      })
  }

  /** "WORK THIS PASS" — the one control that runs the chain (litigation ①):
   * open the bird, auto-pick a workable transponder, arm the pass, bring the
   * readiness rail into view. The click IS the consent for the arm; the two
   * fail-safe Settings switches are NOT flipped here — the rail makes them
   * operable where the operator is, one deliberate click each.
   *
   * Auto-pick rules: only an alive Transponder/Transceiver ever qualifies —
   * never a beacon (downlink-only, nothing to work), never a dead entry, and
   * never over the operator's explicit "None" or an existing hold. */
  const workPass = (p: SatPass) => {
    setSelected(p.name)
    wantRailScroll.current = true
    Promise.all([
      getSatDetail(p.name),
      // The ENGINE's hold, not this session's last click: the hold is
      // released backend-side at LOS and on a live-track stop, so on a
      // bird's NEXT pass (LEO repeats every ~100 min) a stale local mirror
      // here skipped the re-pick — arming a pass whose Doppler had nothing
      // to tune while the rail showed every gate green.
      getSatTransponder().catch(() => null),
    ])
      .then(([d, held]) => {
        setDetail(d) // seed the pane now; the selected-effect keeps it fresh
        const alreadyHeld = held != null && held.name === d.name
        if (alreadyHeld || dialOptOut.has(d.name)) return
        const alive = d.transmitters.map((t, i) => ({ t, i })).filter((x) => x.t.alive)
        const workable = alive.filter(
          (x) => x.t.kind === 'Transponder' || x.t.kind === 'Transceiver',
        )
        const pick = workable[0] ?? null
        // Land the pick BEFORE the arm: the arm's initial DTO — and the
        // consent toast built from it — must describe the pass as it will
        // actually run (its `mode` label needs the held transponder).
        if (pick) return pickTransponder(d.name, pick.i, pick.t.description, true)
      })
      .catch(() => {})
      .then(() => armTrack(p.name, p.aosUnix))
  }
  // The rail scroll: once the armed track's rail exists, bring it into view
  // (nearest — never yank the whole column). jsdom has no scrollIntoView.
  useEffect(() => {
    if (wantRailScroll.current && railRef.current != null) {
      wantRailScroll.current = false
      railRef.current.scrollIntoView?.({ block: 'nearest' })
    }
  })

  /** Title for the work affordances — honest about what an arm will drive. */
  const workTitle = rotorOn
    ? 'Work this pass: opens the bird, picks its transponder, arms rotor auto-track + the pass clock (Doppler tunes when its switches are on)'
    : 'Work this pass: opens the bird, picks its transponder, starts the pass clock + Doppler (no rotator configured — nothing will move)'

  return (
    <div className="sats-view">
      <header className="sats-head">
        <h1>Satellites</h1>
        <span className="sats-sub">
          passes over your grid — modelled from Celestrak elements
          {view && !tleStale ? ` (${view.tleAgeDays.toFixed(1)} d old)` : ''}
        </span>
        {/* Stale elements stop being a dim parenthetical (the appliance's own
            failure mode — it buries `tledate`): an amber chip the eye lands
            on. No "refresh now" — no manual-refresh command exists; the 12 h
            TTL refetch is the only honest story to tell. */}
        {view && tleStale && (
          <span
            className="sat-chip stale"
            title={`Orbital elements are ${view.tleAgeDays.toFixed(1)} days old — pass times and Doppler drift with element age. They refresh automatically (12 h cadence) when the network allows; there is no manual refresh.`}
          >
            {/* Unit spelled out: the chip voice is uppercase, and "9 d" would
                render as the wrong unit "9 D". Always plural — stale starts
                past 14 days. */}
            TLE {Math.round(view.tleAgeDays)} days — STALE
          </span>
        )}
        {track && (
          <span
            className="sats-tracking-badge"
            title={
              // The claim must match the DTO's `mode` — a rotor-less track
              // never borrows the rotor wording.
              track.mode === 'pass-only'
                ? 'Pass timing only — nothing is driven: no rotor in this track, and Doppler is not driving the dial (off, no VFO mapping, or no transponder held). The pass clock and geometry still run.'
                : track.mode === 'doppler-only'
                  ? `No rotator in this track — Doppler ${
                      track.downlinkHz != null || track.uplinkHz != null
                        ? 'is steering the radio dial'
                        : 'takes the radio dial at AOS'
                    }; nothing moves an antenna`
                  : track.azDeg == null
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
            ⟳ {track.state === 'armed' ? 'armed' : 'tracking'} {track.name}
            {/* The MODE word, only when a surface is missing: the full
                rotor+doppler track is the unmarked case; every partial one
                names what it actually drives. */}
            {track.mode === 'doppler-only'
              ? ' · Doppler only'
              : track.mode === 'pass-only'
                ? ' · pass timing only'
                : track.mode === 'rotor-only'
                  ? ' · rotor only'
                  : ''}{' '}
            ·{' '}
            {track.azDeg == null
              ? `rises az ${Math.round(track.aosAzDeg)}°`
              : `cmd az ${Math.round(track.azDeg)}° ${track.elDeg == null ? '(az only)' : `el ${Math.round(track.elDeg)}°`}`}
            <button
              onClick={disarmTrack}
              title={
                track.mode === 'rotor+doppler' || track.mode === 'rotor-only'
                  ? 'Stop auto-tracking (rotor halts)'
                  : 'Stop this track (no rotor involved; the dial is handed back)'
              }
            >
              ■ stop
            </button>
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
          No favorites yet — star birds in the Birds list; the schedule, best-pass
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
          {/* "NEXT UP" — the primary action surface (litigation ①): a pass is
              WORKED from here, not merely opened. The mini arc is Look4Sat's
              pick-by-shape idea; the why-line keeps the plain-language case;
              the earn chips say what the pass is worth (Phase 2); and ▶ runs
              the whole chain in one consented click. */}
          <section className="sats-best">
            <h2>Next up (24 h)</h2>
            {best.map((p) => (
              <div
                key={`${p.name}-${p.aosUnix}`}
                className={`sats-best-row${p.aosUnix <= nowSecs ? ' live' : ''}`}
              >
                <PassArcMini pass={p} />
                <button
                  className="sats-best-open"
                  onClick={() => setSelected(p.name)}
                  title="Open this bird's detail"
                >
                  <b>{p.name}</b> {whyLine(p, nowSecs)} <EarnChips earn={p.earn} />
                </button>
                <button className="sat-work" onClick={() => workPass(p)} title={workTitle}>
                  ▶ Work this pass
                </button>
              </div>
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
                  {/* Phase 2: a SORT KEY the operator clicks — never a silent
                      reorder. Default order stays soonest-AOS. */}
                  {schedTh('Needed', 'need')}
                  <th>⏰</th>
                  {/* The work column renders for EVERYONE — the rotor gate on
                      this column hid the entire tracking arc from the
                      rotor-less operator (the largest satellite population). */}
                  <th></th>
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
                      {/* Earn chips: absent when the pass earns nothing. */}
                      <td className="sat-need">
                        <EarnChips earn={p.earn} />
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
                      <td>
                        {track?.name === p.name && Math.abs(track.aosUnix - p.aosUnix) <= 180 ? (
                          <button
                            className="sat-track on"
                            onClick={(e) => { e.stopPropagation(); disarmTrack() }}
                            title="Stop this track"
                          >
                            ■
                          </button>
                        ) : (
                          <button
                            className="sat-track"
                            onClick={(e) => {
                              e.stopPropagation()
                              workPass(p)
                            }}
                            title={workTitle}
                          >
                            ▶ Work
                          </button>
                        )}
                      </td>
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
            {/* THE READINESS RAIL, directly under the bird's name: once a pass
                is armed the chain is visible AS a chain, each gate fixable in
                place. Content-height — never a grower. */}
            {/* WHICH RIG WILL MOVE — at the rail's position, but not gated on a
                track: a pick tunes the moment it is made, long before a pass is
                armed, and "which radio?" is exactly the question then. Gated on
                the HELD bird (the binding is engine-global, one hold at a time):
                RS-44's rig and frequencies must never render under AO-91's
                heading — the chooser's cross-bird warning covers that case. */}
            {binding && tuned?.name === detail.name && (
              <SatRadioBinding binding={binding} pegged={pegged} onTogglePeg={writePegged} />
            )}
            {detailTrack && (
              <TrackRail
                track={detailTrack}
                rotorOn={rotorOn}
                dopplerOn={dopplerOn}
                vfoMap={vfoMap}
                heldDesc={detailTrack.transponder ?? heldT?.description ?? null}
                heldInverting={detailTrack.inverting || !!heldT?.invert}
                autoPicked={!!(tuned?.auto && tuned.name === detail.name)}
                canPick={detail.transmitters.length > 0}
                nowSecs={nowSecs}
                onStop={disarmTrack}
                onDopplerOn={writeDopplerOn}
                onVfoMap={writeVfoMap}
                onGoToPicker={() => pickerRef.current?.scrollIntoView?.({ block: 'nearest' })}
                scrollRef={railRef}
              />
            )}
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
                <PassTimeline
                  pass={detail.pass}
                  tcaUnix={tcaUnix}
                  nowSecs={nowSecs}
                  earn={detailEarn}
                />
                {detailTrack && (
                  <>
                    <DopplerReadout
                      rotor={detailTrack}
                      dopplerOn={dopplerOn}
                      vfoMap={vfoMap}
                      held={heldIndex != null || detailTrack.transponder != null}
                      txMode={detailTrack.txMode ?? null}
                    />
                    {/* The strip goes under the readout: the readout says what
                        the radio is tuned to, the strip says where that puts
                        the operator inside the passband. */}
                    <PassbandStrip rotor={detailTrack} />
                  </>
                )}
              </>
            ) : nextBeyond ? (
              <div className="sat-passline">
                next pass over you rises {hhmm(nextBeyond.aosUnix)} (
                {countdown(nextBeyond, nowSecs)})
              </div>
            ) : (
              <div className="sat-passline">no pass over you in the next 24 h</div>
            )}
            {detail.transmitters.length > 0 ? (
              <>
                {/* THE TRANSPONDER CHOOSER — the most consequential control in
                    the section (a wrong pick is a wrong uplink), so it lives
                    WITH the tuning instruments, above the globe, as a card
                    list rather than a radio column at the bottom of a
                    scroller. Dead entries collapse behind "show N inactive".
                    The wire index stays the RAW index of the full list (dead
                    included): the backend indexes the very list get_sat_detail
                    returned and refuses dead picks by name. */}
                <div
                  className="sat-tp-list"
                  data-testid="sat-tp-list"
                  role="radiogroup"
                  aria-label="Transponder — where Doppler puts the dial"
                  ref={pickerRef}
                >
                  <label className="sat-tp-card">
                    <input
                      type="radio"
                      name="sat-transponder"
                      checked={heldIndex == null}
                      onChange={() => pickTransponder(detail.name, null)}
                      aria-label="Work no transponder — leave the dial to me"
                    />
                    <span className="sat-tp-desc">None — leave the dial to me</span>
                  </label>
                  {tpRows
                    .filter((r) => r.t.alive)
                    .map(({ t, aliveIndex }) => (
                      <label
                        key={aliveIndex ?? t.description}
                        className={`sat-tp-card${heldIndex === aliveIndex ? ' held' : ''}`}
                      >
                        <input
                          type="radio"
                          name="sat-transponder"
                          checked={heldIndex === aliveIndex}
                          onChange={() =>
                            aliveIndex != null &&
                            pickTransponder(detail.name, aliveIndex, t.description)
                          }
                          aria-label={`Work ${t.description}`}
                        />
                        <span className="sat-tp-main">
                          <span className="sat-tp-desc">
                            {t.description}
                            {t.invert && (
                              <span
                                className="sat-invert"
                                title="Inverting linear transponder: tune the downlink UP and your uplink goes DOWN, and the sidebands swap (LSB up, USB down)."
                              >
                                INVERTING
                              </span>
                            )}
                            {kindWord(t.kind) && (
                              <span className="sat-tp-kind">{kindWord(t.kind)}</span>
                            )}
                            {t.downlinkMode == null && t.uplinkMode == null && t.mode != null && (
                              <span className="sat-tp-kind">{t.mode}</span>
                            )}
                          </span>
                          <span className="sat-tp-legs">
                            <span className="sat-tp-leg">
                              ↓ <b>{fmtLeg(t.downlinkLowHz, t.downlinkHighHz)}</b>
                              {t.downlinkMode ? ` ${t.downlinkMode}` : ''}
                            </span>
                            <span className="sat-tp-leg">
                              ↑ <b>{fmtLeg(t.uplinkLowHz, t.uplinkHighHz)}</b>
                              {t.uplinkMode ? ` ${t.uplinkMode}` : ''}
                            </span>
                          </span>
                          {heldIndex === aliveIndex && tuned?.auto && (
                            <span className="sat-tp-auto">
                              picked for you — change it here if this is not the one
                            </span>
                          )}
                        </span>
                      </label>
                    ))}
                  {tpRows.some((r) => !r.t.alive) &&
                    (showDead ? (
                      tpRows
                        .filter((r) => !r.t.alive)
                        .map(({ t }, k) => (
                          <div key={`dead-${k}`} className="sat-tp-card off">
                            <span className="sat-tp-deadmark" aria-hidden>
                              ○
                            </span>
                            <span className="sat-tp-main">
                              <span className="sat-tp-desc">
                                {t.description}
                                <span className="sat-tp-kind">
                                  reported dead (SatNOGS) — not workable
                                </span>
                              </span>
                              <span className="sat-tp-legs">
                                <span className="sat-tp-leg">
                                  ↓ <b>{fmtLeg(t.downlinkLowHz, t.downlinkHighHz)}</b>
                                </span>
                                <span className="sat-tp-leg">
                                  ↑ <b>{fmtLeg(t.uplinkLowHz, t.uplinkHighHz)}</b>
                                </span>
                              </span>
                            </span>
                          </div>
                        ))
                    ) : (
                      <button
                        type="button"
                        className="sat-tp-more"
                        onClick={() => setShowDead(true)}
                        title="Transmitters SatNOGS reports dead/re-entered — shown for the record, never workable"
                      >
                        show {tpRows.filter((r) => !r.t.alive).length} inactive
                      </button>
                    ))}
                </div>
                {/* Display only — the engine owns the command. With a live
                    track the ENGINE's declared answer (DTO txMode) is the
                    only thing phrased as a command; when it says nothing, so
                    do we (a downlink-only mapping, a CW/data downlink or an
                    operator take-back all mean no X write, whatever the
                    SatNOGS record's per-leg modes say). With no track there
                    is no command to claim yet — the record data reads as a
                    forecast, conditions stated. */}
                {heldT != null && txSwapMode != null && (
                  <div className="sat-tp-txmode" data-testid="sat-tp-txmode">
                    {detailTrack != null ? (
                      detailTrack.txMode != null ? (
                        <>
                          TX sideband: the uplink (split) VFO is set to{' '}
                          <b>{detailTrack.txMode}</b> — the downlink stays{' '}
                          {heldT.downlinkMode} while Doppler runs this pass.
                        </>
                      ) : (
                        <>
                          This bird lists {txSwapMode} up / {heldT.downlinkMode} down
                          (SatNOGS) — the TX sideband is not being commanded for this
                          pass (Doppler off, a mapping that does not drive the uplink,
                          or the mode is yours).
                        </>
                      )
                    ) : (
                      <>
                        TX sideband: this bird runs {txSwapMode} up /{' '}
                        {heldT.downlinkMode} down (SatNOGS). With Doppler on and a VFO
                        mapping that drives the uplink, the TX (split) VFO is set to
                        match while a tracked pass runs (Settings ▸ Radio).
                      </>
                    )}
                  </div>
                )}
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
            <div
              /* The globe, DEMOTED below the chooser (it used to sit between
                 the passband strip and the transponder picker — physically
                 separating the two controls used together). Square-ish and
                 growing with the column rather than pinned at 260px: a globe
                 is only readable at size. Sizing lives in styles.css
                 (.sat-globe-box) with the rest of the section — an inline
                 style block is invisible to every cascade-computing guard,
                 and capping HEIGHT alone let the box go 2:1 wide at 3440px
                 (the width cap there keeps it genuinely square). */
              data-testid="sat-globe-box"
              className="sat-globe-box"
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
