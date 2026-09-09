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
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The
// WORDS come from the catalog (`sat.*` in i18n/en.ts); everything a radio or a
// log is set from does not. Staying here as data or as named constants: bird
// names, NORAD ids, TLE ages, every uplink/downlink frequency and offset, the
// SatNOGS transponder descriptions and their per-leg mode names, azimuths,
// elevations, ranges, altitudes, the compass letters, the mode names the radio
// binding prints (MODE_FM/MODE_SSB below), and the sky dome's own plate text —
// those plates are SIZED from the string by the viewBox arithmetic this file
// documents at length, so they are instrument tick labels, not prose.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type {
  AppSnapshot,
  NeedTag,
  SatBinding,
  SatDetail,
  SatExcluded,
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
  setPegLock,
  confirmSatUplink,
  setSatTransponder,
  getSatTransponder,
  startSatTrack,
  stopSatTrack,
  getSatTrackStatus,
  fetchTlesNow,
} from '../api'
import { NEED_CHIP } from '../features/needVisuals'
import { SAT_VFO_MAPS, satVfoLabel, satVfoPair } from '../features/satVfo'
import { isSatChased, satChaseKeys, satChasingSet, toggleSatChasing } from '../features/satChase'
import { satBirdHealth, satExcludedHealth } from '../features/satHealth'
import { elementBandParts, elementBandSummary } from '../features/elementBands'
import {
  ackSatSeedNotice,
  satSeedNoticeOpen,
  satSeedRecord,
  seedSatFavorites,
  type SatSeedRecord,
} from '../features/satSeed'
import { SAT_ICON_RECTS, SAT_ICON_TILT_DEG } from '../features/satIcon'
import { satAlarmMap, toggleSatAlarm, setSatAlarmLead } from '../features/satAlarm'
import {
  DISCOVERY_ROW_CAP,
  modePillWord,
  passAdmission,
  primaryClass,
  rollPassesToBirds,
} from '../features/satDiscovery'
import { tleRefreshMessage } from '../features/tleMessages'
import { heatPulse } from '../features/pulse'
import { pushToast } from '../toast'
import { t } from '../i18n'
import { T } from '../i18n/T'
import { MapView } from './MapView'
import { LogEntry } from './LogEntry'
import { Dialog } from './ui/Dialog'
import { useTheme } from '../useTheme'

interface Props {
  /** Bird to select (map click hand-off). The section follows changes. */
  focusSat?: string | null
  /** The live engine snapshot — the log strip's dial/band/mode source, exactly
   * as the Phone and CW cockpits feed it. Nullable (and absent in the older
   * section tests) because the pop-out window polls it on its own 300 ms
   * cadence and has none for the first tick; the strip simply doesn't render
   * until it lands. */
  snap?: AppSnapshot | null
  onPopOut?: () => void
  /** Open the Logbook filtered to a callsign (#192) — handed to the log strip's recall card,
   *  whose previous-contact rows become clickable when it is present. Omitted ⇒ inert rows. */
  onOpenLogbook?: (call: string) => void
}

const SCHEDULE_HOURS = 48

/** The station's mode, folded to the closed ADIF Mode enumeration for the radio
 * binding line. Mode names are invariant technical tokens — the same two
 * letters on every rig's display, in every language — so they live here rather
 * than in the catalog (see the invariant-token rule in `i18n/index.ts`). */
const MODE_FM = 'FM'
const MODE_SSB = 'SSB'
/** The satellite's catalog number, as its own catalogs name it. */
const NORAD_LABEL = 'NORAD'
/** The passband plot's centre tick — a bare unit, not a sentence about one. */
const PB_ZERO_TICK = '0 kHz'

/** How many ALIVE transmitter cards the chooser shows before a "show N more"
 * disclosure — the DISCOVERY_ROW_CAP idiom, for the same reason.
 *
 * The chooser is a fit-content block at the bottom of a bounded planning
 * column, so its height comes straight out of the schedule's row count above
 * it. Most amateur birds list one or two workable transmitters; a handful (the
 * multi-mode cubesats) list eight or more, and without a bound one of those
 * would quietly cost the operator half his schedule. A ROW COUNT rather than a
 * max-height, because a max-height here would be a second scroller inside a
 * column that already has its owner. The cap sits on the ALIVE cards only —
 * dead ones already collapse behind "show N inactive". */
const TP_ALIVE_CAP = 4

/** The station's COMMANDED mode, folded to the closed ADIF Mode enumeration.
 *
 * Same rule and same field the Phone cockpit logs on (`FM ? 'FM' : 'SSB'` over
 * the commanded sideband), widened only by the two other ADIF Modes that field
 * can legitimately hold outside a phone cockpit — the Satellites section is
 * reachable with the rig on CW (linear-transponder CW is ordinary satellite
 * work) or AM. `snap.radio.sideband` is the mode the app COMMANDED, not the
 * display-only `rigMode` read-back (which some Hamlib backends answer from
 * cache, and which is `None` until the rig reports).
 *
 * USB/LSB deliberately fold to SSB: they are ADIF SUBMODEs, and `<MODE>USB`
 * gets the whole record rejected on LoTW upload — the same closed-enumeration
 * trap LogEntry's LOG_MODES table documents. Nothing about the satellite feeds
 * this: not the bird, not the transponder, not the downlink.
 *
 * ⭐ TIER-AWARE ON A DIGITAL SECTION (the sat-FT batch, 2026-08-10 — this used
 * to be the disclosed "records SSB on a digital tier" defect). The section is
 * reachable on FT8/FT4/FT2/Q65/JT65/MSK144/WSPR/FST4/Tempo, and the sideband those
 * tiers are generated on says "USB" — an analogue voice mode standing in for a
 * data mode on a permanent record. When the active operating SECTION is
 * digital, the honest value is the TIER's own registered name, and the
 * snapshot's tier strings are exactly the engine's own log-mode mapping (FT8,
 * FT4, Q65 without the display submode, FST4W as itself, TempoFast/TempoDeep)
 * — so the tier string is used verbatim rather than re-mapped here where the
 * two tables could drift.
 *
 * The SECTION gate matters: `link.tier` always names a tier (the decoder is
 * always set to something), so folding by tier unconditionally would label a
 * voice pass worked from the Phone cockpit as FT8. Only the digital section
 * says the operator is actually working the tier.
 *
 * Pinned by `records the tier's mode on a digital section` in
 * SatellitesView.logentry.test.tsx. */
function adifModeFromStation(
  sideband: string | null | undefined,
  tier?: string | null,
  operatingMode?: string | null,
): string {
  if ((operatingMode ?? '') === 'digital' && tier && tier.trim() !== '') {
    return tier.trim()
  }
  const m = (sideband ?? '').trim().toUpperCase()
  if (m === 'FM') return 'FM'
  if (m === 'CW') return 'CW'
  if (m === 'AM') return 'AM'
  return 'SSB'
}

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
  if (p.aosUnix <= nowSecs && nowSecs <= p.losUnix) return t('sat.countdown.now')
  const min = Math.round((p.aosUnix - nowSecs) / 60)
  return min < 90
    ? t('sat.countdown.mins', { mins: Math.max(1, min) })
    : t('sat.countdown.hours', { hours: (min / 60).toFixed(1) })
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

/** The pass-quality ladder — the section's ONE quality vocabulary. The
 * discovery band reuses it (as a tooltip on the elevation cell) so there is
 * one language for "how good is this pass", never a second dialect. */
const qualityWord = (el: number) =>
  el >= 60
    ? t('sat.quality.overhead')
    : el >= 30
      ? t('sat.quality.high')
      : el >= 15
        ? t('sat.quality.workable')
        : t('sat.quality.low')

/** Plain-language "why" line for the Next/Best strip. aosClamped passes get
 * the backscan honesty (report what was DONE): "already up", never the scan
 * window's edge dressed as a rise time — and the duration is then the lower
 * bound it is, marked +. */
function whyLine(p: SatPass, nowSecs: number): string {
  const el = Math.round(p.maxElDeg)
  const dur = Math.max(1, Math.round((p.losUnix - p.aosUnix) / 60))
  const status =
    p.status === 'alive'
      ? t('sat.why.alive')
      : p.status === 'dead' || p.status === 're-entered'
        ? // The upstream status word, printed as it arrived (uppercased for the
          // line's voice) — the source's report, not ours to reword.
          t('sat.why.reported', { status: p.status.toUpperCase() })
        : ''
  return t('sat.why', {
    when: p.aosClamped ? t('sat.why.alreadyUp') : `${hhmm(p.aosUnix)} ${countdown(p, nowSecs)}`,
    el,
    quality: qualityWord(el),
    dur: `${dur}${p.aosClamped ? '+' : ''}`,
    aos: wind8(p.aosAzDeg),
    los: wind8(p.losAzDeg),
    status,
  })
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
          title={t('sat.earn.entities.title', {
            count: earn.newEntities,
            names: earn.entitySample.join(', '),
            more: entityMore > 0 ? t('sat.earn.more', { count: entityMore }) : '',
          })}
        >
          {NEED_CHIP.NewEntity.label}
          {earn.newEntities > 1 ? ` ×${earn.newEntities}` : ''}
        </span>
      )}
      {earn.newGrids > 0 && (
        <span
          className={`need-chip need-${NEED_CHIP.NewGrid.cls}`}
          title={t('sat.earn.grids.title', {
            count: earn.newGrids,
            grids: earn.gridSample.join(' '),
            more: gridMore > 0 ? t('sat.earn.more', { count: gridMore }) : '',
          })}
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

/* ---- The bird's persistent az/el tag ------------------------------------- *
 * A MANUAL az/el rotator is turned by hand off these two numbers. Hover-only
 * meant holding a mouse on a moving dot for the length of a pass while the
 * other hand is on the mast, so both numbers ride on the marker itself and stay
 * there for the whole pass. (The <title> stays for assistive tech, and the
 * readout list below the dome stays as the DOM text equivalent.)
 *
 * Geometry lives here rather than in CSS because the plate is sized to the text
 * it holds: the mono glyph box is 0.6 em, and the font size is emitted as an
 * attribute right beside this arithmetic so the two cannot drift apart. Ink —
 * fill, family, weight — stays in styles.css.
 *
 * ⚠️ THESE NUMBERS WENT UP SO THE TEXT GOT SMALLER. Read the arithmetic before
 * "correcting" one of them back. They are viewBox units, so what an operator
 * actually sees is
 *
 *     rendered px = unit × (rendered dome px ÷ 248)
 *
 * and the dome's rendered size is set in CSS (`.sat-dome`). Operator,
 * 2026-08-03: "the actual aos, los and az, el text could be made smaller by
 * 25%". At the supported 1024×768 floor the dome used to render 458 px wide, so
 * TAG_FS 10 drew these at 18.5 effective px — the LARGEST type in the section,
 * bigger than the <h1>. That is what he was looking at.
 *
 * The same pass rebuild caps the dome at 0.31·--vh-eff = 280 px there (he asked
 * for the dome to come down in the same breath), and 10 × 280/248 = 11.3 px
 * would be a 39% cut — more than he asked for, and heading toward the app's
 * 11 px size floor on the number a manual-rotor operator reads mid-turn. So the
 * constants are RAISED, and the pair lands on the quarter he asked for:
 *
 *     12.3 × 280/248 = 13.9 px   against today's 18.5 px   → −24.8%
 *
 * ⚠️ 12.3 IS A GEOMETRIC CEILING, not a taste. tagSize() makes the plate
 * 7 × TAG_FS × 0.6 + 2·TAG_PAD wide for "az 143°", and `tagPlace` flips it to
 * the bird's LEFT the moment its right edge would pass the horizon box
 * (x = 224). At the mid-pass fixture az 143 / el 47 the bird sits at x = 152.8,
 * so the plate has 62.2 u to work in; 12.3 spends 60.3 of them. TAG_FS 14.5 was
 * tried first and flipped that plate to the wrong side —
 * SatellitesView.skydome.test.tsx's "rides on the bird's right when that side
 * is free" caught it, and the constant was wrong, not the test. TAG_LINE and
 * TAG_PAD scale with TAG_FS (×1.15 and ×0.35) so the plate stays proportioned
 * to the text it holds.
 *
 * Stated honestly, because it is not −25% everywhere: above the floor today's
 * dome grew with the column (25.5 px at 1366×768, 40.6 px at 3440×1440) while
 * the capped one does not, so the reduction there is larger — which is the
 * other half of the same instruction. The floor is where the promise is made. */
const TAG_FS = 12.3
const TAG_LINE = 14.1
const TAG_PAD = 4.3
/** Offset from the bird, clear of the glyph's tilted panel tip (~5.7 u). */
const TAG_GAP = 9
/** Keeps the plate off the very edge of the viewBox. */
const TAG_MARGIN = 2

/** Plate size for the widest line, in viewBox units. */
function tagSize(lines: string[]): [number, number] {
  const cols = Math.max(...lines.map((l) => l.length))
  return [cols * TAG_FS * 0.6 + TAG_PAD * 2, lines.length * TAG_LINE + TAG_PAD * 2]
}

/** Where the tag sits relative to the bird.
 *
 * It rides on the bird's right and flips left when the right would push it past
 * the horizon's box — the eastern rim, i.e. exactly where a pass ends — or when
 * the rotator ghost is sitting there, because burying the ghost would hide the
 * gap that IS the tracking error. The horizon box, not the viewBox, is the
 * bound on BOTH axes: outside it live the compass letters and the ring labels,
 * and the plate is far narrower than the dome, so one side always fits. Low on
 * the northern or southern horizon the tag hangs off the bird rather than
 * losing its second line — an az/el rotator needs the elevation as much as the
 * azimuth. Inside the box the plate is opaque, so where it crosses the pass
 * track the numbers still win: the track is context, these are what the mast is
 * turned by, and they change every second. */
function tagPlace(
  bird: [number, number],
  ghost: [number, number] | null,
  w: number,
  h: number,
): [number, number] {
  const rimLo = DOME_C - DOME_R
  const rimHi = DOME_C + DOME_R
  const right = bird[0] + TAG_GAP
  const left = bird[0] - TAG_GAP - w
  const flip = right + w > rimHi || (left >= rimLo && ghost != null && ghost[0] >= bird[0])
  const x = flip ? left : right
  return [
    Math.min(DOME_C * 2 - TAG_MARGIN - w, Math.max(TAG_MARGIN, x)),
    Math.min(rimHi - h, Math.max(rimLo, bird[1] - h / 2)),
  ]
}

/* ---- The rise and set marks' bearings ------------------------------------ *
 * The same operator and the same problem as the bird's tag above. A manual
 * rotator is PRE-POINTED at the rise bearing before a pass and swung to the set
 * bearing to hold the end of one, and both numbers lived in a <title>. The set
 * mark is drawn hollow — an SVG path with no fill answers the pointer on its
 * 1.4 u outline and nowhere else — so "hard to mouse over" was literally true of
 * that one mark and not of its filled twin.
 *
 * Both plates print an AZIMUTH and no elevation. These two marks sit ON the
 * horizon by construction, so "el 0°" would restate the geometry rather than
 * report a reading, and inventing a number where there is none is the mistake
 * the az-only ghost below exists to avoid.
 *
 * Each plate also names WHICH mark it belongs to, in words and by the ▲/▼ the
 * readout under the dome already uses for these two. The pair is read to decide
 * which way to turn a mast: two bare bearings sitting near each other would be
 * ambiguous, and that is worse than the hover they replace. */

/** Clearance between a horizon mark and the near face of its plate. */
const RIM_GAP = 8
/** How much further out a plate sits on each successive ring of candidates. */
const RIM_STEP = 6
/** Directions to try, in degrees off the mark's inward radius, in preference
 * order: straight in first, then fanning to either side. Past ±90 the plate
 * would lean back out over the rim, where the compass letters live. */
const RIM_FAN = [0, 30, -30, 60, -60, 90, -90]
/** How many rings of candidates before giving up and taking the least-bad. */
const RIM_RINGS = 3

/** An axis-aligned box in viewBox units: x, y, w, h. */
type Box = [number, number, number, number]

/** Area two boxes share; 0 when they are clear of each other. */
function overlapArea(a: Box, b: Box): number {
  const w = Math.min(a[0] + a[2], b[0] + b[2]) - Math.max(a[0], b[0])
  const h = Math.min(a[1] + a[3], b[1] + b[3]) - Math.max(a[1], b[1])
  return w > 0 && h > 0 ? w * h : 0
}

/** A square obstacle centred on a mark. */
const boxAt = (p: [number, number], r: number): Box => [p[0] - r, p[1] - r, 2 * r, 2 * r]

/** Where a horizon mark's plate sits.
 *
 * It hangs INSIDE the dome — outside the rim there are only 24 u of margin and
 * the compass letters have them, so a plate out there would cover a letter and
 * then run off the viewBox. The gap is measured from the plate's near FACE (its
 * own reach along the offset direction), because a plate merely centred on an
 * offset point buries its own mark wherever the box is widest along that
 * offset — east and west, for the straight-in case.
 *
 * `taken` is everything already on the dome that has to stay readable: both
 * marks, the bird, its live tag, the rotator ghost. Candidates are tried in
 * preference order and the first CLEAR one wins, so a plate stays as close to
 * its own mark as the traffic allows.
 *
 * The fan is why the candidates are a fan and not a ladder straight inward. On
 * a low grazing pass the two marks sit within a few tens of degrees of each
 * other, so their inward radii are near enough parallel that stepping both
 * deeper marches them inward TOGETHER and they never separate — which is
 * exactly the pass where the operator most needs to tell the rise bearing from
 * the set one. Fanning sideways separates them on the first ring.
 *
 * Crossing the pass track is fine and deliberate: the plate is opaque, and at
 * the rim the track is at its thinnest and faintest anyway. */
function rimTagPlace(azDeg: number, w: number, h: number, taken: Box[]): Box {
  const rad = (azDeg * Math.PI) / 180
  const [ux, uy] = [-Math.sin(rad), Math.cos(rad)] // rim point → dome centre
  const [px, py] = skyPt(azDeg, 0)
  const lo = DOME_C - DOME_R
  const hi = DOME_C + DOME_R
  let best: Box = [px, py, w, h]
  let bestCost = Infinity
  for (let ring = 0; ring < RIM_RINGS; ring++) {
    for (const fan of RIM_FAN) {
      const f = (fan * Math.PI) / 180
      const dx = ux * Math.cos(f) - uy * Math.sin(f)
      const dy = ux * Math.sin(f) + uy * Math.cos(f)
      const d = RIM_GAP + (Math.abs(dx) * w + Math.abs(dy) * h) / 2 + ring * (h + RIM_STEP)
      const box: Box = [
        Math.min(hi - w, Math.max(lo, px + dx * d - w / 2)),
        Math.min(hi - h, Math.max(lo, py + dy * d - h / 2)),
        w,
        h,
      ]
      const cost = taken.reduce((sum, t) => sum + overlapArea(box, t), 0)
      if (cost === 0) return box
      if (cost < bestCost) {
        bestCost = cost
        best = box
      }
    }
  }
  return best
}

/** One horizon mark's plate — shared by both so the rise and set bearings can
 * never drift into two different-looking readings of the same kind of number.
 * Pointer-transparent: it must never take the hover off the mark it labels. */
function RimTag({ box, lines, testid }: { box: Box; lines: string[]; testid: string }) {
  return (
    <g className="sat-dome-rimtag" data-testid={testid} pointerEvents="none">
      <rect
        className="sat-dome-tag-plate"
        x={box[0].toFixed(1)}
        y={box[1].toFixed(1)}
        width={box[2].toFixed(1)}
        height={box[3].toFixed(1)}
        rx={2.5}
      />
      {lines.map((line, i) => (
        <text
          key={line}
          className="sat-dome-rimtag-line"
          x={(box[0] + TAG_PAD).toFixed(1)}
          y={(box[1] + TAG_PAD + TAG_FS * 0.8 + i * TAG_LINE).toFixed(1)}
          fontSize={TAG_FS}
        >
          {line}
        </text>
      ))}
    </g>
  )
}

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

  // The az/el tag that rides on the bird. Both numbers, because a manual az/EL
  // rotator needs both; placement rules in tagPlace.
  const tagLines = bird ? [`az ${deg(bird.az)}`, `el ${deg(bird.el)}`] : null
  const tagWH = tagLines ? tagSize(tagLines) : null
  const tagXY = birdPt && tagWH ? tagPlace(birdPt, ghostPt, tagWH[0], tagWH[1]) : null

  // The rise and set bearings, on their marks. Placed AFTER the bird's tag and
  // handed it as an obstacle: at AOS the bird sits on the rise mark and the two
  // readouts want the same patch of dome, so the second-by-second number holds
  // its place and the fixed reference bearing yields. The identity line carries
  // the ▲/▼ rather than being sized by it — the shape glyphs come from a
  // fallback face whose advance width is not the mono cell tagSize() assumes,
  // and putting them on the shorter line keeps that guess off the plate width.
  const aosLines = ['▲ AOS', `${deg(pass.aosAzDeg)} ${wind8(pass.aosAzDeg)}`]
  const losLines = ['▼ LOS', `${deg(pass.losAzDeg)} ${wind8(pass.losAzDeg)}`]
  const aosWH = tagSize(aosLines)
  const losWH = tagSize(losLines)
  const taken: Box[] = [boxAt(aosPt, 6), boxAt(losPt, 6)]
  if (birdPt) taken.push(boxAt(birdPt, 8))
  if (tagXY && tagWH) taken.push([tagXY[0], tagXY[1], tagWH[0], tagWH[1]])
  if (ghostPt) taken.push(boxAt(ghostPt, 8))
  if (ghostRim) taken.push(boxAt(ghostRim, 6))
  const aosBox = rimTagPlace(pass.aosAzDeg, aosWH[0], aosWH[1], taken)
  const losBox = rimTagPlace(pass.losAzDeg, losWH[0], losWH[1], [...taken, aosBox])

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
        ? t('sat.dome.antenna.noCommand')
        : cmdEl == null
          ? t('sat.dome.antenna.azOnly', { az: deg(cmdAz) })
          : // The Δ tail is pure geometry — a symbol, a number and a degree
            // sign — so it is appended here rather than made into words.
            `${t('sat.dome.azEl', { az: deg(cmdAz), el: deg(cmdEl) })}${errDeg != null ? ` · Δ ${errDeg.toFixed(1)}°` : ''}`

  const label = bird
    ? t('sat.dome.aria.bird', {
        name,
        az: Math.round(bird.az),
        el: Math.round(bird.el),
      })
    : t('sat.dome.aria.track', {
        name,
        aos: Math.round(pass.aosAzDeg),
        los: Math.round(pass.losAzDeg),
        maxEl: Math.round(pass.maxElDeg),
      })

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
          <title>
            {t('sat.dome.aos.title', {
              az: deg(pass.aosAzDeg),
              wind: wind8(pass.aosAzDeg),
              time: hhmm(pass.aosUnix),
            })}
          </title>
        </path>
        {/* Hollow on purpose — shape, not colour, tells rise from set. But an
            SVG path with no fill hit-tests its 1.4 u outline and nothing else,
            which is what made THIS mark, alone of the two, hard to mouse over.
            `all` hit-tests the whole triangle, so the title below is reachable
            without threading a mouse onto a hairline. */}
        <path d={triPath(losPt[0], losPt[1], 5, false)} className="sat-dome-los" pointerEvents="all">
          <title>
            {t('sat.dome.los.title', {
              az: deg(pass.losAzDeg),
              wind: wind8(pass.losAzDeg),
              time: hhmm(pass.losUnix),
            })}
          </title>
        </path>
        {/* The rise and set bearings, ON the marks. Drawn after the track so the
            opaque plate wins where it crosses it, and before the ghost and the
            bird so the live marks stay on top of a reference readout. */}
        <RimTag testid="sat-aos-tag" box={aosBox} lines={aosLines} />
        <RimTag testid="sat-los-tag" box={losBox} lines={losLines} />
        {/* THE ROTATOR GHOST. A second marker at what the antenna was told, not
            where it is: the gap to the bird IS the tracking error, and drawing
            it is what stops a legitimate deadband from looking like a fault. */}
        {ghostPt && (
          <g className="sat-dome-ghost" data-testid="sat-ghost">
            <title>{t('sat.dome.ghost.title')}</title>
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
            <title>{t('sat.dome.ghost.azOnly.title', { az: deg(cmdAz) })}</title>
            <line x1={DOME_C} y1={DOME_C} x2={ghostRim[0]} y2={ghostRim[1]} className="sat-dome-azspoke" />
          </g>
        )}
        {birdPt && bird && (
          <g className="sat-dome-bird" data-testid="sat-bird">
            <title>
              {t('sat.dome.bird.title', { name, az: deg(bird.az), el: deg(bird.el) })}
            </title>
            {live && (
              <circle
                cx={birdPt[0]}
                cy={birdPt[1]}
                r={(5.5 + 5 * breath).toFixed(1)}
                className="sat-dome-bird-halo"
                opacity={(0.32 * breath).toFixed(2)}
              />
            )}
            {/* The spacecraft itself, drawn from the shape the world map draws
                (features/satIcon — one glyph, two renderers): the object the
                operator picks out on the map is the object on the dome. */}
            <g
              className="sat-dome-bird-icon"
              transform={`translate(${birdPt[0].toFixed(1)},${birdPt[1].toFixed(1)}) rotate(${SAT_ICON_TILT_DEG})`}
            >
              {SAT_ICON_RECTS.map((r, i) => (
                <rect key={i} x={r.x} y={r.y} width={r.w} height={r.h} />
              ))}
            </g>
            {tagLines && tagWH && tagXY && (
              <g className="sat-dome-tag" data-testid="sat-bird-tag">
                <rect
                  className="sat-dome-tag-plate"
                  x={tagXY[0].toFixed(1)}
                  y={tagXY[1].toFixed(1)}
                  width={tagWH[0].toFixed(1)}
                  height={tagWH[1].toFixed(1)}
                  rx={2.5}
                />
                {tagLines.map((line, i) => (
                  <text
                    key={line}
                    className="sat-dome-tag-line"
                    x={(tagXY[0] + TAG_PAD).toFixed(1)}
                    y={(tagXY[1] + TAG_PAD + TAG_FS * 0.8 + i * TAG_LINE).toFixed(1)}
                    fontSize={TAG_FS}
                  >
                    {line}
                  </text>
                ))}
              </g>
            )}
          </g>
        )}
      </svg>
      {/* The text equivalent. An SVG instrument is invisible to a screen
          reader, and these are the numbers an operator reads off the dome
          anyway — so it is shown, not hidden. Absent values are absent rows. */}
      <dl className="sat-dome-readout">
        {bird && (
          <div>
            <dt>{t('sat.dome.readout.satellite')}</dt>
            <dd>{t('sat.dome.azEl', { az: deg(bird.az), el: deg(bird.el) })}</dd>
          </div>
        )}
        {rotor?.rangeKm != null && (
          <div>
            <dt title={t('sat.dome.readout.range.title')}>{t('sat.dome.readout.range')}</dt>
            <dd>
              {t('sat.dome.readout.km', { km: Math.round(rotor.rangeKm) })}
              {rotor.rangeRateKmS != null &&
                t('sat.dome.readout.rangeRate', {
                  rate: `${rotor.rangeRateKmS >= 0 ? '+' : ''}${rotor.rangeRateKmS.toFixed(2)}`,
                  trend:
                    rotor.rangeRateKmS < 0
                      ? t('sat.dome.readout.closing')
                      : t('sat.dome.readout.opening'),
                })}
            </dd>
          </div>
        )}
        {/* Directly under the range, because the pair is only useful read
            together and only unambiguous when both are NAMED: range is the
            distance from the shack, altitude the height above the earth. The
            second one is what says whether this is a ten-minute LEO screamer
            or a bird loitering near apogee — which is the difference between
            Doppler running away from you and barely moving. Absent before AOS,
            like the range above it: no row rather than 0 km. */}
        {rotor?.altKm != null && (
          <div>
            <dt title={t('sat.dome.readout.altitude.title')}>
              {t('sat.dome.readout.altitude')}
            </dt>
            <dd>{t('sat.dome.readout.km', { km: Math.round(rotor.altKm) })}</dd>
          </div>
        )}
        {ghostText && (
          <div>
            <dt title={t('sat.dome.readout.antenna.title')}>{t('sat.dome.readout.antenna')}</dt>
            <dd className={ghost === 'az-only' ? 'partial' : undefined}>{ghostText}</dd>
          </div>
        )}
        <div>
          <dt>{t('sat.dome.readout.riseSet')}</dt>
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
        <span>{t('sat.timeline.aos', { time: hhmm(pass.aosUnix) })}</span>
        <span className="sat-tl-tca-label">
          {/* TCA leads the peak when the track knows one — one label, with the
              optional half interpolated, never two glued fragments. */}
          {t('sat.timeline.peak', {
            tca: tcaUnix != null ? t('sat.timeline.tca', { time: hhmm(tcaUnix) }) : '',
            maxEl: Math.round(pass.maxElDeg),
          })}
        </span>
        <span>{t('sat.timeline.los', { time: hhmm(pass.losUnix) })}</span>
      </div>
      <div className="sat-tl-state">
        {live
          ? t('sat.state.inPass', { mins: minsLeft })
          : t('sat.timeline.nextPass', { countdown: countdown(pass, nowSecs) })}
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
                ? t('sat.earn.more', { count: earn.newGrids - earn.gridSample.length })
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
 * that leg" — which is now routine rather than exceptional, since the downlink
 * is corrected automatically while the uplink waits for a mapping confirmed
 * for the radio in play. No zeros, no placeholder dashes — an absent row, plus
 * one line saying WHY when there is nothing at all (the reasons are all things
 * this component actually knows: the off switch, the track's own per-leg
 * consent, whether a transponder is held, and whether the pass has started). */
function DopplerReadout({
  rotor,
  dopplerOn,
  held,
  uplinkOnly,
  undrivableUplink,
  txMode,
}: {
  rotor: SatTrackStatus
  dopplerOn: boolean
  held: boolean
  /** The mapping in force is Uplink only — the one mapping under which the
   * dial is never Doppler's, so the pre-hold promise must name the uplink
   * instead (round 3, defect 6). */
  uplinkOnly: boolean
  /** The held bird has no uplink leg an uplink-only mapping could drive — a
   * one-channel (simplex) transponder or a beacon. Splits the no-legs reason:
   * "not confirmed for this radio" is the wrong sentence (and the wrong fix)
   * when confirmation would change nothing on this bird. */
  undrivableUplink: boolean
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
      ? t('sat.doppler.none.off')
      : !held
        ? uplinkOnly
          ? // Confirmation-aware (round 4), same rule as the rail's waiting
            // copy: an unconfirmed mapping must not promise the drive.
            rotor.uplinkOffer === 'confirm-mapping'
            ? t('sat.doppler.none.pickConfirm')
            : t('sat.doppler.none.pickUplink')
          : t('sat.doppler.none.pick')
        : !rotor.dopplerDownlink && !rotor.dopplerUplink
          ? undrivableUplink
            ? t('sat.uplinkOnly.noLeg')
            : t('sat.doppler.none.unconfirmed')
          : rotor.state !== 'tracking'
            ? t('sat.doppler.none.beforeAos')
            : t('sat.doppler.none.noTuning')
    return <div className="sat-doppler none">{why}</div>
  }
  return (
    <div className="sat-doppler">
      <div className="sat-dop-head">
        <span className="sat-dop-title">{t('sat.doppler.head')}</span>
        {rotor.transponder && <span className="sat-dop-tp">{rotor.transponder}</span>}
        {rotor.inverting && (
          <span className="sat-invert" title={t('sat.inverting.title')}>
            {t('sat.inverting.label')}
          </span>
        )}
        {/* Lock on used to live here, beside these numbers. It moved out — see
            SatLockOn: the numbers are exactly what is missing in the two states
            where the operator needs the way back most. */}
      </div>
      <dl className="sat-dop-legs">
        {rotor.downlinkHz != null && (
          <div>
            <dt>{t('sat.leg.downlink')}</dt>
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
            <dt>{t('sat.leg.uplink')}</dt>
            <dd>
              {fmtMHz(rotor.uplinkHz)}
              {rotor.uplinkShiftHz != null && (
                <span className="sat-dop-shift"> {fmtShift(rotor.uplinkShiftHz)}</span>
              )}
              {txMode != null && (
                <span
                  className="sat-dop-txmode"
                  title={t('sat.doppler.txMode.title', { mode: txMode })}
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
  const name = down ? t('sat.passband.lane.downlink') : t('sat.passband.lane.uplink')
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
        {/* ~24 px of hit target for a 4 px mark — 26 viewBox units. The plot is
            capped at 300 px wide, so a viewBox unit is never more than ~0.94 px
            and this box never shrinks below about 24 px of real target. */}
        <rect className="sat-pb-hit" x={-13} y={down ? 2 : 40} width={26} height={32}>
          <title>
            {t('sat.passband.mark.title', {
              glyph,
              leg: name,
              freq: freqHz != null ? t('sat.passband.mark.freq', { hz: freqHz }) : '',
              offset: fmtShift(offsetHz),
              clamped: clamped ? t('sat.passband.mark.clamped') : '',
            })}
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
      // The exact dial in Hz — a bare reading, printed as it is.
      freq != null ? `${freq} Hz` : null,
      shift != null ? t('sat.passband.readout.doppler', { shift: fmtShift(shift) }) : null,
      t('sat.passband.readout.offset', { offset: fmtShift(off) }),
    ]
      .filter((part) => part != null)
      .join(' · ')
  // ONE sentence: three glued fragments could not be reordered by a language
  // that words "either side of centre" differently.
  const label = t('sat.passband.aria', {
    mode: rotor.inverting
      ? t('sat.passband.inverting.word')
      : t('sat.passband.nonInverting.word'),
    half: pbKHz(half),
    down: fmtShift(offset),
    up: fmtShift(upOffset),
  })
  return (
    <div className="sat-pb" data-testid="sat-passband">
      <div className="sat-pb-head">
        <span className="sat-pb-title">{t('sat.passband.head')}</span>
        {/* The mirror is drawn, but it is also said in words — a picture is a
            poor place to learn a rule nobody has told you. */}
        <span className={`sat-pb-mode${rotor.inverting ? ' inv' : ''}`}>
          {rotor.inverting ? t('sat.passband.inverting') : t('sat.passband.nonInverting')}
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
            {PB_ZERO_TICK}
          </text>
          <text className="sat-pb-ticklabel" x={PB_W - PB_PAD} y={88} textAnchor="end">
            +{pbKHz(half)}
          </text>
          <text className="sat-pb-axistitle" x={PB_CX} y={99} textAnchor="middle">
            {t('sat.passband.axis.title')}
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
          {t('sat.passband.noWidth')}
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
            <dt>{l.down ? t('sat.leg.downlink') : t('sat.leg.uplink')}</dt>
            <dd>{legText(l.freq, l.shift, l.off)}</dd>
          </div>
        ))}
        {half > 0 && (
          <div>
            <dt>{t('sat.passband.head')}</dt>
            <dd>{t('sat.passband.readout.width', { half: pbKHz(half) })}</dd>
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
    ? t('sat.kind.beacon')
    : k === 'Transponder'
      ? t('sat.kind.linear')
      : k === 'Transceiver'
        ? t('sat.kind.fmRepeater')
        : null

/* ======================= the readiness rail (top-5 ①) =======================
 * The arming chain rendered AS a chain: five gates, always five rows, each
 * not-ready row carrying its own fix instead of prose pointing at another
 * section. Filled ● / hollow ○ carries ready-state by SHAPE (the codebase
 * idiom — it survives greyscale and colourblindness). The fifth row —
 * Elements — is the physics everything above runs on: the age of the set this
 * track FROZE at arm (per-pass freeze, now visible), hollow past the 14 d
 * stale line with a refresh fix.
 *
 * The two Settings switches (satDopplerOff, satVfoMap) are MIRRORED here live —
 * Settings ▸ Radio stays canonical, writes go read-modify-write through
 * getSettings → setSettings (the OperateCockpit precedent) so nothing else in
 * the settings object is clobbered. The rail never flips a default itself:
 * every change is the operator's click on this pass, right now.
 *
 * The DOPPLER row reads the TRACK, not those mirrors, for what is being
 * driven: the two legs are separately consented (the downlink is automatic
 * once a transponder is held; the uplink needs a mapping confirmed for the rig
 * actually under the split, which routing can change mid-pass), and only the
 * backend knows which rig that is. The mirrors stay for the CONTROLS — the off
 * switch and the mapping select — because those write settings.
 *
 * Layout: the rail lives in the full-width ARM BAR (row 2 of .sats-view), not
 * in the detail column, and lays its five gates out ACROSS the bar rather than
 * down a stack. Content-height and content-width both: it is a strip, never a
 * grower, and the bar is a non-scrolling grid row. */
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
  simplex,
  autoPicked,
  canPick,
  nowSecs,
  onStop,
  onDopplerOn,
  onVfoMap,
  onGoToPicker,
  onRefreshElements,
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
  /** The held transponder is a SIMPLEX channel (uplink == downlink, the 145.825
   * APRS class) — one dial carries both legs, which is what the Doppler row has
   * to say instead of leaving the operator to wonder where their uplink went.
   * From the ENGINE's binding, never re-derived here: two copies of a rule that
   * decides where the radio transmits is a wrong-uplink generator. */
  simplex: boolean
  /** The hold came from "Work this pass", not a hand pick — disclosed. */
  autoPicked: boolean
  /** The bird has a transponder chooser to go to (it renders only when the
   * bird lists transmitters) — without one the pick button is disabled WITH
   * its reason, never an enabled control that silently does nothing. */
  canPick: boolean
  nowSecs: number
  onStop: () => void
  onDopplerOn: () => void
  /** Write the mapping + confirmation. `radio` is the RadioProfile.id the
   * grant lands on — the rail passes the DTO's uplinkRadioId (the rig its
   * copy names); omitted, the writer falls back to the active radio. `v`
   * undefined = confirm the mapping IN FORCE, resolved by the backend at
   * write time (round 4): the DTO's copy of it is poll-time state. */
  onVfoMap: (v: SatVfoMap | undefined, radio?: number) => void
  onGoToPicker: () => void
  /** The Elements row's fix — the same manual refresh the stale chip fires. */
  onRefreshElements: () => void
  scrollRef: React.RefObject<HTMLDivElement>
}) {
  // The rotor half of the track is fixed at arm time (DTO `mode`) except for a
  // rotator that stops answering, which demotes it mid-pass (`rotorLost` — the
  // track keeps the dial and runs to LOS); the Doppler row below reports the
  // two legs the engine re-reads each tick — so a change here moves both the
  // behaviour and the row together.
  const rotorInTrack = track.mode === 'rotor+doppler' || track.mode === 'rotor-only'
  const dopplerLive = track.dopplerDownlink || track.dopplerUplink
  const minsToAos = Math.max(1, Math.round((track.aosUnix - nowSecs) / 60))
  const minsToLos = Math.max(0, Math.round((track.losUnix - nowSecs) / 60))
  const passText =
    track.state === 'armed'
      ? nowSecs < track.aosUnix
        ? t('sat.rail.pass.armedIn', { mins: minsToAos })
        : t('sat.rail.pass.armed')
      : track.state === 'prepositioning'
        ? t('sat.state.prepositioning')
        : t('sat.state.inPass', { mins: minsToLos })
  // THE DOPPLER ROW's sentence. Six states, so it is computed rather than
  // nested into the JSX: what is being driven right now (from the track, never
  // re-derived here), and — when the transmit leg is not ours — what it would
  // take, in the words of the radio the split would actually land on.
  const offerRig = track.uplinkRadio.trim() || t('sat.rail.doppler.yourRadio')
  const offerPair = track.uplinkOfferMap ? satVfoPair(track.uplinkOfferMap) : null
  // Three confirmable offers, and the copy must not conflate them: 'confirm'
  // is a mapping DERIVED from the rig model; 'confirm-mapping' is the mapping
  // ALREADY IN FORCE, unconfirmed for this radio (a second rig, a reused id,
  // or an upgraded uplink-only file) — confirming keeps the operator's
  // choice. Without its own affordance that state was unrecoverable: the
  // select already shows the mapping, and a same-value re-pick fires no
  // change event (round 3, defect 4).
  //
  // 'switch-mapping' is the third and the only one that appears over a mapping
  // the operator already chose AND confirmed: the backend says it cannot carry
  // this pass on this radio while a mapping that can is known. Offering a
  // correction to a choice that cannot work is not overwriting that choice —
  // the click is still the operator's, and a mapping that works is never
  // proposed against (the backend answers 'none' there).
  const switchMapping = track.uplinkOffer === 'switch-mapping'
  const canConfirm =
    (track.uplinkOffer === 'confirm' ||
      track.uplinkOffer === 'confirm-mapping' ||
      switchMapping) &&
    offerPair != null
  const uplinkNext =
    track.uplinkOffer === 'confirm' && offerPair != null
      ? t('sat.rail.uplink.confirm', { rig: offerRig, pair: offerPair })
      : track.uplinkOffer === 'confirm-mapping' && track.uplinkOfferMap != null
        ? t('sat.rail.uplink.confirmMapping', {
            mapping: satVfoLabel(track.uplinkOfferMap),
            rig: offerRig,
          })
        : switchMapping && offerPair != null
          ? t('sat.rail.uplink.switchMapping', { rig: offerRig, pair: offerPair })
          : track.uplinkOffer === 'ask'
            ? t('sat.rail.uplink.ask')
            : t('sat.rail.uplink.yours')
  // SIMPLEX before the leg pairs: a one-channel bird has no uplink leg, so the
  // honest wire reports dopplerUplink false there even under a confirmed
  // mapping — and the offer sentence must never render (confirming would
  // record a permanent consent for a split the engine refuses on this bird).
  const dopplerText = !dopplerOn
    ? t('sat.rail.doppler.off')
    : heldDesc == null
      ? vfoMap === 'uplink-only'
        ? // Confirmation-aware (round 4): while the mapping in force is not
          // confirmed for the radio in play (the DTO's offer says so), "then
          // Doppler tunes your uplink" promises an unconsented drive.
          track.uplinkOffer === 'confirm-mapping'
          ? t('sat.rail.doppler.waitingConfirm')
          : t('sat.rail.doppler.waitingUplink')
        : t('sat.rail.doppler.waiting')
      : simplex
        ? track.dopplerDownlink
          ? t('sat.rail.doppler.simplex')
          : t('sat.rail.doppler.simplexUplinkOnly')
        : // `dopplerUplink` says the engine is CORRECTING the uplink, which it
          // is — and under 'switch-mapping' the split apply then refuses to
          // write it, every tick. The row reports what is DONE, so the uplink
          // is not claimed here; the pass rail's note and the CAT status carry
          // the refusal itself.
          track.dopplerDownlink && track.dopplerUplink && !switchMapping
          ? t('sat.rail.doppler.both')
          : track.dopplerDownlink
            ? t('sat.rail.doppler.downlink', { next: uplinkNext })
            : track.dopplerUplink && !switchMapping
              ? t('sat.rail.doppler.uplink')
              : t('sat.rail.doppler.uplinkOnly', { next: uplinkNext })
  const rotorText = track.rotorLost
    ? // It QUIT — a different fact from "not in this track", and the row must
      // not let the demoted mode blur the two. Says what the operator now owns
      // (the pointing) and what he still does not have to think about.
      t('sat.rail.rotor.lost')
    : !rotorInTrack
    ? rotorOn
      ? t('sat.rail.rotor.notInTrack')
      : t('sat.rail.rotor.none')
    : track.azDeg != null
      ? t('sat.rail.rotor.tracking', {
          az: deg(track.azDeg),
          // An az-only rotor was never sent an elevation and must not print
          // one; with a real one the pair is a bare reading.
          el: track.elDeg == null ? t('sat.rail.rotor.azOnly') : ` el ${deg(track.elDeg)}`,
        })
      : t('sat.rail.rotor.armed')
  return (
    <div className="sat-rail" data-testid="sat-rail" ref={scrollRef}>
      <div className="sat-rail-row">
        {railDot(true)}
        <span className="sat-rail-name">{t('sat.rail.gate.pass')}</span>
        {/* The gate cells ellipsize in the arm bar (styles.css .sat-rail-state),
            so every state sentence carries itself on `title`. Nothing is
            unreachable — the ● / ○ is what has to survive at a glance. */}
        <span className="sat-rail-state" title={passText}>
          {passText}
        </span>
        <button className="sat-rail-fix" onClick={onStop} title={t('sat.rail.stop.title')}>
          {t('sat.track.stop')}
        </button>
      </div>
      <div className="sat-rail-row">
        {railDot(rotorInTrack, !track.rotorLost && !rotorInTrack && !rotorOn)}
        <span className="sat-rail-name">{t('sat.rail.gate.rotor')}</span>
        <span className="sat-rail-state" title={rotorText}>
          {rotorText}
        </span>
      </div>
      <div className="sat-rail-row">
        {railDot(heldDesc != null)}
        <span className="sat-rail-name">{t('sat.rail.gate.transponder')}</span>
        <span className="sat-rail-state" title={heldDesc ?? t('sat.rail.transponder.none')}>
          {heldDesc != null ? (
            <>
              {heldDesc}
              {heldInverting && <span className="sat-invert">{t('sat.inverting.label')}</span>}
              {autoPicked && (
                <span className="sat-rail-auto">{t('sat.rail.transponder.auto')}</span>
              )}
            </>
          ) : (
            t('sat.rail.transponder.none')
          )}
        </span>
        <button
          className="sat-rail-fix"
          onClick={onGoToPicker}
          disabled={!canPick}
          title={
            canPick
              ? t('sat.rail.transponder.goTo')
              : t('sat.rail.transponder.nothingToPick')
          }
        >
          {heldDesc != null ? t('sat.rail.transponder.change') : t('sat.rail.transponder.pick')}
        </button>
      </div>
      <div className="sat-rail-row">
        {railDot(dopplerLive)}
        <span className="sat-rail-name">{t('sat.rail.gate.doppler')}</span>
        {/* The CURRENT mapping is deliberately not named here. The select at
            the end of this row is both the live value and the control for it,
            so spelling it out in the state text printed the same sentence
            twice on one line ("on — VFO A = uplink, VFO B = downlink", then
            the identical option). What the text owns is what the select cannot
            say: which legs are actually being driven, and — when the transmit
            leg is not ours — what one confirmation would hand over, named for
            the radio that would receive it. */}
        <span className="sat-rail-state" title={dopplerText}>
          {dopplerText}
        </span>
        {!dopplerOn && (
          <button
            className="sat-rail-fix"
            onClick={onDopplerOn}
            title={t('sat.rail.doppler.turnOn.title')}
          >
            {t('sat.rail.doppler.turnOn')}
          </button>
        )}
        {/* The one-time confirmation, where the operator already is. Rendered
            only while there is something derived to confirm, the uplink is
            not already ours, and the held bird HAS an uplink leg to hand over
            (never on simplex — the backend suppresses the offer there too;
            this is the belt to that suspender). Confirming records the
            mapping FOR THE RADIO THE DTO NAMES (uplinkRadioId — the same rig
            the title names), so a radio switch between the poll and the
            click can never grant a rig the operator did not see named. For
            the mapping ALREADY IN FORCE ('confirm-mapping') no map is sent
            at all — the DTO's copy of it is poll-time state, and the backend
            resolves "the mapping in force" at write time exactly as it
            resolves the radio (round 4). The derived offer ('confirm') must
            still send its map: that mapping is NOT in force yet — and neither
            is the correction ('switch-mapping'), which sends its map for the
            same reason.

            'switch-mapping' is also the one offer rendered while the uplink IS
            ours: the operator consented to a mapping whose split the engine
            refuses for this pass, so `!dopplerUplink` — "not already ours" —
            is exactly the wrong question there. It is the only exception, and
            the backend gates it on a REFUSED write plus a mapping that can be
            driven, so the button never appears over a mapping that works. */}
        {dopplerOn && (!track.dopplerUplink || switchMapping) && canConfirm && !simplex && (
          <button
            className="sat-rail-fix"
            onClick={() =>
              onVfoMap(
                track.uplinkOffer === 'confirm-mapping'
                  ? undefined
                  : (track.uplinkOfferMap as SatVfoMap),
                track.uplinkRadioId,
              )
            }
            title={
              switchMapping
                ? t('sat.rail.uplink.switch.title', { rig: offerRig, pair: offerPair ?? '' })
                : track.uplinkOffer === 'confirm'
                  ? t('sat.rail.uplink.confirm.title', {
                      pair: offerPair ?? '',
                      rig: offerRig,
                    })
                  : t('sat.rail.uplink.confirmMapping.title', {
                      mapping: satVfoLabel(track.uplinkOfferMap ?? 'off'),
                      rig: offerRig,
                    })
            }
          >
            {switchMapping
              ? t('sat.rail.uplink.switch.label')
              : t('sat.rail.uplink.confirm.label')}
          </button>
        )}
        <select
          className="sat-rail-vfo"
          value={vfoMap}
          onChange={(e) => onVfoMap(e.target.value as SatVfoMap, track.uplinkRadioId)}
          aria-label={t('sat.rail.vfoMap.aria')}
          title={t('sat.rail.vfoMap.title')}
        >
          {SAT_VFO_MAPS.map((m) => (
            <option key={m.value} value={m.value}>
              {m.label}
            </option>
          ))}
        </select>
      </div>
      {/* ELEMENTS — the physics the four gates above run on. The DTO's age is
          FROZEN at arm (the per-pass freeze: one element set drives the whole
          pass), so the fix refreshes the CACHE for the next arm and says so —
          it must not claim it can move this track. Ready at ≤14 d: the one
          STALE number, worn as a gate. */}
      <div className="sat-rail-row">
        {railDot(track.elementAgeDays <= 14)}
        <span className="sat-rail-name">{t('sat.rail.gate.elements')}</span>
        <span
          className="sat-rail-state"
          title={
            track.elementAgeDays <= 14
              ? t('sat.rail.elements.current', { age: track.elementAgeDays.toFixed(1) })
              : t('sat.rail.elements.stale', { days: Math.round(track.elementAgeDays) })
          }
        >
          {track.elementAgeDays <= 14
            ? t('sat.rail.elements.current', { age: track.elementAgeDays.toFixed(1) })
            : t('sat.rail.elements.stale', { days: Math.round(track.elementAgeDays) })}
        </span>
        {track.elementAgeDays > 14 && (
          <button
            className="sat-rail-fix"
            onClick={onRefreshElements}
            title={t('sat.rail.elements.refresh.title')}
          >
            {t('sat.rail.elements.refresh')}
          </button>
        )}
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
 * TopBar RadioSwitcher's 🔒 — reachable where the operator is, written through
 * its own backend verb (see `writePegged`), not read-modify-write like the two
 * Doppler switches on the rail below.
 *
 * Layout: content-height, never a grower (ui-layout §2). Shares the rail's box
 * treatment, beside it in the arm bar. */
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
        <span className="sat-rail-name">{t('sat.binding.name')}</span>
        <span className="sat-rail-state">
          {/* No RIG = a refusal that returned before routing resolved one, so
              the reason stands alone — never "this radio · · SSB" beside a rig
              that was never picked.

              Keyed on radioId, not on the band. An absent BAND no longer means
              "nothing was resolved": a downlink the band table cannot name
              (QO-100, the microwave birds) routes and tunes like any other, and
              hiding the rig there would print frequencies beside no radio at
              all. The band chip is dropped on its own instead — absent is
              absent, and it is the one thing here we genuinely do not know. */}
          {binding.radioId != null && (
            <>
              {binding.radioName || t('sat.binding.thisRadio')}
              <span className="sat-bind-why">
                {binding.band !== '' ? ` · ${binding.band}` : ''} ·{' '}
                {binding.fm ? MODE_FM : MODE_SSB}
              </span>
            </>
          )}
          {/* A note can accompany surviving legs (e.g. the split refused while
              the dial landed) — print both rather than letting either win. The
              note itself is the ENGINE's prose, interpolated as a value. */}
          {legs.length > 0
            ? t('sat.binding.legs', {
                legs: legs.join(' · '),
                note: binding.note ? t('sat.binding.note', { note: binding.note }) : '',
              })
            : binding.radioId != null
              ? t('sat.binding.note', { note: binding.note ?? '' })
              : (binding.note ?? '')}
        </span>
        <button
          className={`sat-rail-fix${pegged ? ' on' : ''}`}
          aria-pressed={pegged}
          onClick={() => onTogglePeg(!pegged)}
          title={pegged ? t('sat.binding.pegged.title') : t('sat.binding.unpegged.title')}
        >
          {pegged ? t('sat.binding.pegged') : t('sat.binding.unpegged')}
        </button>
      </div>
    </div>
  )
}

/* ==================== lock on (the way back onto the bird) ==================
 * Operator report: "should we introduce a button to lock on to the implied
 * frequency if I move the dial on my own?"
 *
 * Nexus already adopts a knob move made INSIDE the passband as your position
 * and mirrors the uplink to it. What had no way back was leaving the passband
 * — by hand, or because the rig came back on a different frequency — after
 * which the dial is somewhere the pass does not describe. The transponder
 * cards cannot fix that either: they are radio inputs, and clicking the one
 * already selected fires no change event, so the pick that would re-assert
 * everything was literally unreachable while it was the pick in force.
 *
 * WHAT IT DOES: re-runs that pick — `setSatTransponder(name, index)`, the same
 * command the card runs — so routing, the band, the commanded mode and both
 * legs all come with it, rather than shoving a frequency at the rig behind the
 * bookkeeping's back. It re-centres in the passband, which is what "the implied
 * frequency" means; a position you tuned to by hand is not something to
 * preserve while asking to be put back on the bird.
 *
 * WHY IT IS NOT IN THE DOPPLER READOUT (where it shipped first, and where it
 * was unreachable — operator report twice). The readout is where the
 * frequencies are, so it looked like the obvious home. But that block renders
 * only once a pass is ARMED, and its head — the half holding the button — only
 * once Doppler has REPORTED a tuning, which it does not do until AOS. So the
 * recovery control did not exist in either state where the dial most often
 * gets away from you: a transponder picked with no pass armed (the pick tunes
 * the radio immediately — that dial is live), and an armed pass waiting for
 * AOS. A control whose whole job is "put me back" cannot be gated on the
 * numbers it would put you back ON; those numbers arriving is not the event
 * that makes the dial wrong.
 *
 * So it sits at the RADIO BINDING's position instead — with the line that says
 * which rig is being driven, which renders from the moment of the pick. Its
 * subject is the dial, not the pass. ONE control, ONE place: it is rendered
 * here and nowhere else, gated on the same hold `heldT` resolves (engine truth
 * first, local pick second) and on nothing more.
 *
 * Never present without a hold: with nothing held there is no pick to re-run,
 * and a button that picked "whatever looks right" would be choosing a
 * transponder — a choice of uplink — for the operator.
 *
 * Layout: content-height row in the arm bar, sharing the rail/binding box
 * treatment (ui-layout §2). */
function SatLockOn({ onLockOn }: { onLockOn: () => void }) {
  return (
    <div className="sat-lockon" data-testid="sat-lockon">
      <div className="sat-rail-row">
        <span className="sat-rail-name">{t('sat.lockOn.name')}</span>
        <span className="sat-rail-state">{t('sat.lockOn.state')}</span>
        {/* The rail's own fix pill — this IS a fix ("the dial is off the bird;
            put it back"), so it wears the same affordance as the gate fixes
            directly below it rather than inventing a second button style. */}
        <button
          type="button"
          className="sat-rail-fix"
          onClick={onLockOn}
          title={t('sat.lockOn.title')}
        >
          {t('sat.lockOn.label')}
        </button>
      </div>
    </div>
  )
}

export function SatellitesView({ focusSat, snap, onPopOut, onOpenLogbook }: Props) {
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
  // Past TP_ALIVE_CAP workable transmitters, the rest are behind "show all N";
  // reset per bird alongside showDead.
  const [tpAll, setTpAll] = useState(false)
  const railRef = useRef<HTMLDivElement>(null)
  const pickerRef = useRef<HTMLDivElement>(null)
  // The rail's "pick"/"change" fix. It used to be a bare scrollIntoView, which
  // was the right idiom while the chooser sat far down a scrolling column. The
  // chooser is in the planning quadrant now — permanently on screen at md+ — so
  // that scroll is a NO-OP there, and a fix button that visibly does nothing
  // reads as broken. It still scrolls (at sm/xs the view really is the page
  // scroller, and the quadrant is genuinely off-screen), and it now also lands
  // the operator ON the control: focus goes to the held card if there is one,
  // else the first. `preventScroll` + `scrollIntoView({block:'nearest'})` is the
  // layout contract's pairing — never let a focus() yank the column.
  const goToPicker = useCallback(() => {
    const box = pickerRef.current
    if (box == null) return
    box.scrollIntoView?.({ block: 'nearest' })
    const target =
      box.querySelector<HTMLInputElement>('input[type="radio"]:checked') ??
      box.querySelector<HTMLInputElement>('input[type="radio"]')
    target?.focus?.({ preventScroll: true })
  }, [])
  // "Work this pass" wants the rail scrolled into view once it exists.
  const wantRailScroll = useRef(false)
  const [dopplerOn, setDopplerOn] = useState(false)
  const [vfoMap, setVfoMap] = useState<SatVfoMap>('off')
  // A manual element refresh (chip / rail fix / arm-confirm) is in flight —
  // the affordances disable rather than queue a second one behind the
  // backend's single-flight refusal.
  const [tleRefreshing, setTleRefreshing] = useState(false)
  // The >14 d arm confirm: `pass` re-runs the chain after a refresh, `proceed`
  // is the held tail of the work-pass chain ("Arm anyway"). Null = no confirm
  // pending.
  const [armConfirm, setArmConfirm] = useState<{
    pass: SatPass
    ageDays: number
    proceed: () => void
  } | null>(null)
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
  // The one-time seed's notice: open from the moment the seed runs until the
  // operator dismisses it (persisted — a notice that vanished on restart
  // would leave them wondering where the stars came from).
  const [seedNotice, setSeedNotice] = useState<SatSeedRecord | null>(() =>
    satSeedNoticeOpen() ? satSeedRecord() : null,
  )

  // THE DISCOVERY BAND's disclosure. null = follow the default (open only on
  // the zero-favourites path at md+; collapsed everywhere else — the calm
  // default is today's screen plus one chip). Deliberately NOT persisted:
  // the disclosure plus a live count is the whole control surface this needs.
  const [discOpen, setDiscOpen] = useState<boolean | null>(null)
  // Past the row cap, "show all N" — reset when the band collapses.
  const [discAll, setDiscAll] = useState(false)

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
  // Star DISPLAY matches NORAD-first like every Connect surface (isSatChased) —
  // after an upstream rename the star lives under the old name, and a name-only
  // `favs.has()` here showed ☆ while Connect showed ★ for the same bird. The
  // schedule fetch above stays name-driven (favKey — the backend contract).
  // Recomputed off favKey: every toggle mutates the name set, so the memo can
  // never hold a stale NORAD record.
  const chaseKeys = useMemo(() => satChaseKeys(), [favKey])
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
    setShowDead(false) // both collapses are per bird
    setTpAll(false)
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
        setDopplerOn(!s.satDopplerOff)
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
    // briefly shows a dead pass as "live". Answers carry their issue number;
    // a straggler that lost the race is dropped, never applied. (The LOS
    // handback notice this guard once protected now fires from the app-wide
    // watcher — features/satPassAlert.ts — so it lands even when this
    // section is closed at LOS.)
    let issued = 0
    let applied = 0
    const load = () => {
      const seq = ++issued
      getSatTrackStatus()
        .then((t) => {
          if (!live || seq < applied) return
          applied = seq
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
            setDopplerOn(!s.satDopplerOff)
            setVfoMap(s.satVfoMap ?? 'off')
            setPegged(!!s.radioPegged)
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

  // ONE-TIME FAVORITES SEEDING. A first-run operator lands on an empty
  // schedule and a 300-bird list with no way to know which of them are worth
  // starring; this stars the most-workable ACTIVE birds over their grid, once,
  // and then never again (satSeed.ts owns every guard — the marker, the
  // already-starred operator, the operator who deliberately cleared their
  // set). Idempotent, so running it on every snapshot needs no gate here.
  //
  // `gridSet` is optimistic until Settings loads, which is safe on its own
  // terms: seeding also requires a workable PASS, and passes only exist once
  // the backend resolved an observer from the grid. Both must hold.
  useEffect(() => {
    const rec = seedSatFavorites(view, gridSet)
    if (rec == null) return
    setFavs(satChasingSet())
    setSeedNotice(rec)
  }, [view, gridSet])

  // `norad` (when the caller's row knows it) keeps the name→NORAD record
  // current — the UI half of rename survival (satChase.ts, phase 4).
  const onToggleFav = (name: string, norad?: number | null) => {
    toggleSatChasing(name, norad)
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
    // Filtered against the CURRENT ★ set, not just the fetch that produced
    // it: `schedule` lags a favourites change by one effect pass, and for
    // that frame the stale rows kept rendering Next-up + fav rows after the
    // last unstar. A row for a bird that is not ★ right now is never honest
    // here, whatever state it arrived in. Steady-state this is a no-op — the
    // fetch is favourites-only by construction.
    const rows = schedule.filter(
      (p) => p.losUnix > nowSecs && isSatChased(p.name, p.norad, chaseKeys),
    )
    rows.sort((a, b) => {
      const va = val(a)
      const vb = val(b)
      const c = typeof va === 'number' && typeof vb === 'number' ? va - vb : String(va).localeCompare(String(vb))
      // Tiebreak: soonest pass first, always.
      return (schedSort.asc ? c : -c) || a.aosUnix - b.aosUnix
    })
    return rows
  }, [schedule, nowSecs, schedSort, chaseKeys])
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
        title={t('sat.schedule.sortBy', { label })}
      >
        {label}
        {schedSort.key === key ? (schedSort.asc ? ' ▲' : ' ▼') : ''}
      </button>
    </th>
  )
  // THE NEXT/BEST STRIP (operator ruling, 2026-08 field report: "Next up"
  // ranked by pass QUALITY while claiming to be next, and read the ★ set
  // only, so a nearer workable pass on an unstarred bird could never
  // surface). Two labelled pairs over ALL admitted birds — candidates come
  // through the discovery band's OWN admission (passAdmission: ONE rule,
  // never two — what the band refuses cannot surface here either), from the
  // same `view.passes` snapshot, windowed to 24 h.
  const strip = useMemo(() => {
    type StripRow = { pass: SatPass; fav: boolean }
    const empty = { next: [] as StripRow[], best: [] as StripRow[] }
    if (view == null) return empty
    const { admits } = passAdmission(view, nowSecs)
    const horizon = nowSecs + 24 * 3600
    const cands: StripRow[] = view.passes
      .filter((p) => p.aosUnix < horizon && admits(p))
      .map((p) => {
        // ★ rows: the favourites schedule fetch knows more than the element
        // snapshot — the live SatNOGS status and the stamped `earn` (matched
        // by bird + AOS, detailEarn's ±180 s tolerance). Non-★ rows carry
        // NEITHER: earn is stamped only by get_sat_pass_needs (the
        // declined-cost ruling) and the snapshot's status reads 'alive' for
        // every admitted bird (the discovery band's no-status-chip refusal,
        // same reason) — so no claim, never a guessed one.
        const fav = isSatChased(p.name, p.norad, chaseKeys)
        const live = fav
          ? schedule.find((s) => s.name === p.name && Math.abs(s.aosUnix - p.aosUnix) <= 180)
          : undefined
        return { pass: { ...p, status: live?.status ?? null, earn: live?.earn ?? null }, fav }
      })
    // "Next": the 2 soonest workable passes, clock order. An in-progress
    // pass's AOS is already behind the clock (clamped or not), so plain
    // ascending AOS leads with it — the pass overhead right now is the most
    // actionable thing on the board.
    const next = cands
      .slice()
      .sort((a, b) => a.pass.aosUnix - b.pass.aosUnix)
      .slice(0, 2)
    // "Best 24 h": the existing passScore rank, computed on the DECORATED
    // pass (a favourite the live overlay calls dead sinks instead of being
    // crowned). A pass already shown in Next renders once — Best backfills
    // with the next-ranked, never a duplicate.
    const shown = new Set(next.map((c) => `${c.pass.name}|${c.pass.aosUnix}`))
    const best = cands
      .sort((a, b) => passScore(b.pass) - passScore(a.pass))
      .filter((c) => !shown.has(`${c.pass.name}|${c.pass.aosUnix}`))
      .slice(0, 2)
    return { next, best }
  }, [view, nowSecs, chaseKeys, schedule])

  // The Birds list rows. Two sources, because existence used to be
  // elements-only: a bird nothing carries current elements for has no `birds`
  // row, so a ★ bird that lost its elements simply VANISHED from the list the
  // operator stars from — no row, no reason, no way to unstar it.
  //
  // Unplaceable birds are therefore listed when they are STARRED (the
  // operator asked about that one) or when a SEARCH names them. Never all of
  // them: the catalog knows ~280, and a default list five times its real
  // length is its own dishonesty. The search clause is what keeps the star a
  // two-way door — without it, unstarring an element-less bird deleted the
  // only row that could ever star it again.
  const allBirds = useMemo(() => {
    const q = search.trim().toUpperCase()
    const rows = (view?.birds ?? [])
      .filter((b) => q === '' || b.name.toUpperCase().includes(q))
      .map((b) => ({
        name: b.name,
        norad: b.norad ?? null,
        status: b.status ?? null,
        amateur: b.amateur,
        // How high it is RIGHT NOW (the subpoint's height), which is how an
        // operator tells a ten-minute LEO screamer from a bird loitering near
        // apogee before ever opening it. Null for the excluded rows below —
        // no elements, no subpoint, so no number.
        altKm: b.altKm as number | null,
        reason: null as SatExcluded['reason'] | null,
      }))
    for (const e of view?.excluded ?? []) {
      const wanted =
        q === ''
          ? isSatChased(e.name, e.norad, chaseKeys)
          : e.name.toUpperCase().includes(q)
      if (!wanted) continue
      rows.push({
        name: e.name,
        norad: e.norad,
        status: e.status ?? null,
        // The catalog's transmitter answer when it has one — a bird whose
        // last amateur transmitter went quiet is why it has no elements, and
        // the row should say that, not just "no elements". `undefined` where
        // the catalog does not know the bird is "not asked", which satHealth
        // must never read as "no transmitters".
        amateur: e.amateur ?? undefined,
        altKm: null,
        reason: e.reason,
      })
    }
    rows.sort((a, b) => a.name.localeCompare(b.name))
    return rows
  }, [view, search, chaseKeys])

  // `tleAgeDays` is the MEDIAN age of the sets inside the 30 d ceiling (never
  // the oldest one — the ceiling pins that just under itself), so this reads
  // "the typical bird in my catalog is past the line", which is what the chip
  // claims. Individual old birds are named in the Birds list's excluded rows;
  // how many there are is the band note beside the chip.
  const tleStale = view != null && view.tleAgeDays > 14

  // What the median does NOT cover, stated where the operator already is: a
  // catalog with a slow-cadence tail and one that is quietly going off both
  // reported nothing here (an excluded bird gets a row only when it is
  // starred or matched by the search box, so by default it appeared nowhere).
  const bandNote =
    view == null || elementBandParts(view).length === 0 ? null : elementBandSummary(view)
  // THE DISCOVERY ROLLUP — one row per non-★ bird with a workable pass in the
  // snapshot's 24 h window, worth-sorted (satDiscovery.ts owns the volume law
  // and every admission rule). Computed from `view.passes` alone: the array
  // is already fetched at 60 s for the seed and was otherwise unread here —
  // zero new IPC. ★ exclusion is NORAD-first (isSatChased), so a promoted
  // bird leaves the band the moment it is starred.
  const discovery = useMemo(
    () =>
      view == null
        ? []
        : rollPassesToBirds(view, nowSecs, (n, no) => isSatChased(n, no, chaseKeys)),
    [view, nowSecs, chaseKeys],
  )
  // Collapsed at sm/xs ALWAYS (there .sats-view is the page scroller and
  // twelve extra rows push the Birds list further away — the exact failure
  // this design fixes); open by default only on the zero-favourites path,
  // where the band IS the answer and is learned before it must be found.
  const vpClass = document.documentElement.getAttribute('data-viewport')
  const smallViewport = vpClass === 'sm' || vpClass === 'xs'
  const discoveryOpen = discOpen ?? (favs.size === 0 && !smallViewport)
  // THE LATCH: a null default that has RENDERED open behaves as an explicit
  // open until the operator toggles. Without it the FIRST ★ flips favs.size
  // to 1, the null default re-evaluates to collapsed, and all twelve rows
  // vanish under the operator's cursor — punishing exactly the promotion act
  // the band exists for.
  useEffect(() => {
    if (discOpen == null && discoveryOpen) setDiscOpen(true)
  }, [discOpen, discoveryOpen])
  const discShown = discAll ? discovery : discovery.slice(0, DISCOVERY_ROW_CAP)

  // Mode pills: the per-bird class, when the catalog can say (types.ts
  // `classes`, reduced by primaryClass — absent renders NO pill). NORAD
  // first, name fallback, the same identity order every ★ surface uses.
  const birdTypeByKey = useMemo(() => {
    const m = new Map<string, string>()
    for (const b of view?.birds ?? []) {
      const cls = primaryClass(b.classes)
      if (cls == null) continue
      if (b.norad != null) m.set(`n${b.norad}`, cls)
      m.set(b.name.toUpperCase(), cls)
    }
    return m
  }, [view])
  const pillFor = (name: string, norad?: number | null) =>
    modePillWord(
      (norad != null ? birdTypeByKey.get(`n${norad}`) : undefined) ??
        birdTypeByKey.get(name.toUpperCase()),
    )

  // ESCAPE closes the detail — the missing navigation primitive (nothing in
  // this file ever passed setSelected(null) before). Guarded on the arm
  // confirm: the Radix Dialog owns Escape while it is open, and closing the
  // detail underneath it would yank the ground out from under the confirm.
  // Closing is a NAVIGATION act, never a disarm — the track, the rotor and
  // the transponder hold are untouched.
  useEffect(() => {
    if (selected == null) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || armConfirm != null || e.defaultPrevented) return
      // Escape INSIDE a text or select control belongs to that control (clear
      // the field, close the native dropdown) — a window-level close here
      // would yank the detail out from under the alarm-lead <select> or the
      // Birds-list search mid-use. Buttons and the detail body still close.
      const t = e.target
      if (t instanceof Element && t.closest('input, select, textarea')) return
      setSelected(null)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [selected, armConfirm])

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
  // The workable cards, and the first TP_ALIVE_CAP of them. The HELD card is
  // always in the shown set even when it sits past the cap: a chooser that
  // hides the selection is a chooser that reads as "None", and the wire index
  // is the RAW list index, so nothing about the cap reaches the backend.
  const aliveRows = useMemo(() => tpRows.filter((r) => r.t.alive), [tpRows])
  const aliveShown = useMemo(() => {
    if (tpAll || aliveRows.length <= TP_ALIVE_CAP) return aliveRows
    const head = aliveRows.slice(0, TP_ALIVE_CAP)
    if (heldIndex != null && !head.some((r) => r.aliveIndex === heldIndex)) {
      const heldRow = aliveRows.find((r) => r.aliveIndex === heldIndex)
      if (heldRow) return [...head.slice(0, TP_ALIVE_CAP - 1), heldRow]
    }
    return head
  }, [tpAll, aliveRows, heldIndex])
  // Will a pick actually move the radio? The downlink follows any pick unless
  // the operator switched correction off (or mapped their one VFO to the
  // uplink), so saying nothing here would leave a live control looking dead.
  const dopplerLive = dopplerOn && vfoMap !== 'uplink-only'

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
  // The held transponder INDEX — engine truth first (the DTO index is what the
  // engine holds, and it covers a pass armed before this section opened, where
  // the local click-state never saw a click), local pick second (it covers the
  // window between the pick and the arm). Both index the same list
  // get_sat_detail returned, which is the list set_sat_transponder indexes, so
  // this is also the wire index Lock on re-sends.
  const heldPickIndex = detailTrack?.transponderIndex ?? heldIndex
  // …and the RECORD at that index.
  const heldT =
    detail == null || heldPickIndex == null
      ? null
      : (detail.transmitters[heldPickIndex] ?? null)
  // The held bird is a ONE-CHANNEL (simplex) transponder — from the ENGINE's
  // binding, gated on the hold being THIS bird (the binding is global), and
  // never re-derived from the SatNOGS record: two copies of a rule that
  // decides where the radio transmits is a wrong-uplink generator. Feeds the
  // chooser state line and the Doppler readout's reason.
  const heldSimplex = !!(detail != null && binding?.simplex && tuned?.name === detail.name)
  // The ADIF mode the section's log strip records a contact as, and the report
  // format that follows from it. Station state only — see adifModeFromStation.
  const logMode = adifModeFromStation(
    snap?.radio.sideband,
    snap?.link.tier,
    snap?.radio.operatingMode,
  )
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
      .then((armed) => {
        setTrack(armed)
        if (armed) {
          // The toast says what THIS track drives — the DTO's `mode` is the
          // engine's own answer, so a rotor-less arm never claims a rotor.
          const doing =
            armed.state === 'armed'
              ? armed.mode === 'rotor+doppler' || armed.mode === 'rotor-only'
                ? t('sat.toast.track.rotor')
                : armed.mode === 'doppler-only'
                  ? t('sat.toast.track.doppler')
                  : t('sat.toast.track.passOnly')
              : armed.state === 'prepositioning'
                ? t('sat.state.prepositioning')
                : t('sat.toast.track.following')
          pushToast(t('sat.toast.track', { name: armed.name, doing }), 'success', 5000)
        } else pushToast(t('sat.toast.track.nothing'), 'info', 6000)
      })
      .catch((e) =>
        pushToast(t('sat.toast.track.failed', { error: `${e instanceof Error ? e.message : e}` }), 'error'),
      )
  }
  const disarmTrack = () => {
    stopSatTrack()
      .then(() => {
        setTrack(null)
        // Stopping a live track hands the dial back backend-side (the stop
        // command releases the transponder hold) — mirror it now rather than
        // waiting out the next 2 s read-back.
        setTuned(null)
      })
      .catch(() => {})
  }

  /** The manual element refresh (phase 2) — the stale chip, the rail's
   * Elements fix and the arm-confirm's Refresh all run THIS. Awaits the
   * backend's one attempt (mirror-first, escalating to Celestrak in the same
   * flight when the mirror can't deliver; the Celestrak floor and 403 stop
   * are not operator-waivable). A failed attempt RESOLVES with the typed
   * failure in the status — `tleRefreshMessage` composes the one
   * operator-voiced toast either way — and only a landed attempt re-pulls
   * the view so the chip clears the moment fresh elements land. Always
   * resolves — a caller chaining on it never needs its own catch. */
  const refreshTles = () => {
    setTleRefreshing(true)
    return fetchTlesNow()
      .then((s) => {
        const m = tleRefreshMessage(s)
        pushToast(m.text, m.kind, m.kind === 'success' ? 5000 : 8000)
        if (s.lastError != null) return
        return getSatellites()
          .then((v) => setView(v))
          .catch(() => {})
      })
      .catch((e) =>
        // Only "couldn't attempt at all" rejects (a refresh already in
        // flight) — already operator words.
        pushToast(`${e instanceof Error ? e.message : e}`, 'error'),
      )
      .finally(() => setTleRefreshing(false))
  }

  /** Hand a transponder to the Doppler engine, or `null` to hand the dial back.
   * `index` is the RAW index into the list `get_sat_detail` returned (dead
   * entries included) — what the backend indexes. The selection is only shown
   * once the call succeeds: a refused pick must not leave the operator
   * believing the radio is under Doppler control. `auto` = picked by "Work
   * this pass", disclosed in the toast and on the card. */
  const pickTransponder = (name: string, index: number | null, label = '', auto = false) => {
    pickBusy.current = true
    // `auto` travels to the backend as well as into the local mirror: it is the
    // one thing the engine cannot work out for itself, and while a pass is
    // engaged it decides whether a pick naming a different row is honored (an
    // operator changing transponder) or refused (the chain re-running).
    return setSatTransponder(name, index, auto)
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
          pushToast(t('sat.toast.transponder.cleared'), 'success', 4000)
        } else {
          const working = t('sat.toast.transponder.working', {
            name,
            label,
            auto: auto ? t('sat.toast.transponder.working.auto') : '',
          })
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
            setDopplerOn(!s.satDopplerOff)
            setVfoMap(s.satVfoMap ?? 'off')
          })
          .catch(() => {})
      })
      .catch((e) =>
        pushToast(
          t('sat.toast.transponder.failed', { error: `${e instanceof Error ? e.message : e}` }),
          'error',
        ),
      )
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
      .then((s: Settings) => setSettings({ ...s, satDopplerOff: false }))
      .then(() => setDopplerOn(true))
      .catch((e) =>
        pushToast(t('sat.toast.doppler.failed', { error: `${e instanceof Error ? e.message : e}` }), 'error'),
      )
      .finally(() => {
        settingsWriteBusy.current = false
      })
  }
  /** Peg-lock: the app-wide "don't auto-switch radios" override. Pinning does
   * not re-tune — it changes where the NEXT pick lands, and the line re-reads.
   *
   * Written through the backend verb (`setPegLock`, api.ts), NOT read-modify-
   * write like the two Doppler switches beside it: `radio_pegged` is part of
   * the LIVE dual-radio roster state, which `apply_settings` captures before an
   * incoming payload moves in and restores after it (engine.rs `live_pegged`,
   * the same protection the roster, the routing rules and the uplink consent
   * pair get). A whole-settings save therefore cannot carry this field — it
   * landed nowhere, the mirror flipped to 🔒 on the resolved promise, and the
   * rail's next 2 s read-back returned the untouched value and flipped it back
   * to 🔓. That is the "goes pinned, then goes unpinned" field report. */
  const writePegged = (on: boolean) => {
    settingsWriteBusy.current = true
    setPegLock(on)
      .then(() => setPegged(on))
      .catch((e) =>
        pushToast(t('sat.toast.peg.failed', { error: `${e instanceof Error ? e.message : e}` }), 'error'),
      )
      .finally(() => {
        settingsWriteBusy.current = false
      })
  }
  /** The uplink mapping — and, in the same write, the CONFIRMATION that it is
   * the mapping for the radio Doppler is driving. The two are one operator
   * act: choosing (or confirming an offered) mapping here is them stating how
   * this station is wired. The write goes through the backend verb
   * (`confirmSatUplink`, api.ts) — the consent pair is engine-owned live
   * state that a whole-settings payload cannot carry, so a stale snapshot
   * can never re-point the mapping or resurrect a pruned consent.
   *
   * `radio` = the RadioProfile.id the rail's copy NAMED (the DTO's
   * uplinkRadioId — the rig that would receive the split). The grant is
   * recorded for that rig, not for whichever radio is active when the click
   * lands: if the two differ, consenting the active one would authorize a
   * radio the operator never saw named. */
  const writeVfoMap = (v: SatVfoMap | undefined, radio?: number) => {
    settingsWriteBusy.current = true
    confirmSatUplink(v, radio)
      // `v` undefined = confirming the mapping already in force (round 4):
      // the mapping did not change, so the mirror has nothing to learn.
      .then(() => v != null && setVfoMap(v))
      .catch((e) =>
        pushToast(t('sat.toast.vfoMap.failed', { error: `${e instanceof Error ? e.message : e}` }), 'error'),
      )
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
        // The chain's tail (auto-pick → arm), deferrable: past the 14 d STALE
        // line it waits on the operator's confirm instead of running.
        const proceed = () =>
          Promise.resolve()
            .then(() => {
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
        // ARMING PAST 14 d ASKS FIRST (the one STALE number, its second
        // consequence): pointing and Doppler drift with element age, and the
        // operator chooses Refresh / Arm anyway / Cancel. ≤14 d arms exactly
        // as before — the click is the consent either way.
        if (d.elementAgeDays != null && d.elementAgeDays > 14) {
          setArmConfirm({ pass: p, ageDays: d.elementAgeDays, proceed })
          return
        }
        return proceed()
      })
      // Detail unavailable (SatNOGS offline — or the 30 d refusal): still ask
      // the backend to arm. Its own gate answers with the named reason, which
      // the armTrack toast surfaces — never a silent dead button. (This is
      // the pre-confirm fall-through, kept.)
      .catch(() => armTrack(p.name, p.aosUnix))
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
  const workTitle = rotorOn ? t('sat.work.title.rotor') : t('sat.work.title.noRotor')

  return (
    <div className="sats-view">
      <header className="sats-head">
        <h1>{t('sat.head.title')}</h1>
        {/* ONE LINE (2026-08-03 pass rebuild). The header used to wrap to two
            lines at the 1024×768 floor — and it wrapped EXACTLY when a pass went
            live, because the tracking badge pushed it over. The badge moved to
            the arm bar below, and the provenance sentence ("modelled from
            Celestrak elements, N d old") folded into the refresh chip's own
            tooltip: FT8-console technique 7, density won by changing the
            control type rather than the type size. */}
        <span className="sats-sub">{t('sat.head.sub')}</span>
        {/* The birds the age does not speak for — drifting, or held back past
            30 d — as a share of the catalog, so "30 of 367 sit out" cannot
            read like "51 of 100". Dim: it is a fact about a working catalog,
            not a warning. */}
        {bandNote && (
          <span
            className="sats-sub"
            data-testid="sat-element-bands"
            title={t('sat.head.bands.title')}
          >
            {bandNote}
          </span>
        )}
        {/* Stale elements stop being a dim parenthetical (the appliance's own
            failure mode — it buries `tledate`): an amber chip the eye lands
            on — and the chip IS the manual refresh now (fetch_tles_now,
            phase 2). The old "there is no manual refresh" tooltip line is
            dead because the claim is. */}
        {view && tleStale && (
          <button
            type="button"
            className="sat-chip stale"
            disabled={tleRefreshing}
            onClick={() => void refreshTles()}
            title={t('sat.head.stale.title', { days: view.tleAgeDays.toFixed(1) })}
          >
            {/* Unit spelled out: the chip voice is uppercase, and "9 d" would
                render as the wrong unit "9 D". Always plural — stale starts
                past 14 days. */}
            {t('sat.head.stale.chip', { days: Math.round(view.tleAgeDays) })}{' '}
            {tleRefreshing ? t('sat.head.stale.refreshing') : t('sat.head.stale.refresh')}
          </button>
        )}
        {/* THE SECTION'S MANUAL REFRESH when nothing is wrong. The amber chip
            above IS the refresh, but under median semantics it correctly
            disappears on a healthy catalog — which used to take the section's
            only refresh affordance with it (leaving the arm-confirm, the armed
            rail past 14 d, and Settings ▸ Radio ▸ Orbital elements). It is
            here with no elements at all, too: that is when fetching them is
            the only thing to do. Quiet by construction — it states no age and
            makes no claim, so it can never be the warning that lied. Exactly
            one of the two chips renders. */}
        {!tleStale && (
          <button
            type="button"
            className="sat-chip quiet"
            disabled={tleRefreshing}
            onClick={() => void refreshTles()}
            title={
              view
                ? t('sat.head.quiet.title', {
                    days: view.tleAgeDays.toFixed(1),
                    sets: view.usableCount,
                  })
                : t('sat.head.quiet.titleEmpty')
            }
          >
            {tleRefreshing ? t('sat.head.quiet.refreshing') : t('sat.head.quiet.refresh')}
          </button>
        )}
        {onPopOut && (
          <button className="pane-popout" onClick={onPopOut} title={t('sat.head.popOut.title')}>⧉</button>
        )}
        {/* THE SEED NOTICE. The app starred birds on the operator's behalf, so
            it says so — a silent mutation of their ★ set would be the app
            having an opinion it never admitted to. Lives INSIDE the header
            (flex-basis:100%, its own line) rather than as a new grid child:
            .sats-view's rows are `auto auto minmax(0,1fr)` and an extra
            column-1 item would push the schedule out of the grower row. */}
        {seedNotice && (
          <div className="sats-seed-notice">
            <span>{t('sat.head.seed.notice', { count: seedNotice.names.length })}</span>
            <button
              type="button"
              // The glyph is not a name a screen reader can use, and this is
              // the notice's only control.
              aria-label={t('sat.head.seed.dismiss.aria')}
              title={t('sat.head.seed.dismiss.title')}
              onClick={() => {
                ackSatSeedNotice()
                setSeedNotice(null)
              }}
            >
              ✕
            </button>
          </div>
        )}
      </header>

      {/* ================= THE ARM BAR (2026-08-03 pass rebuild) =============
          Everything that says WHAT IS ARMED AND WHAT IT IS DRIVING, on one
          full-width row above both columns: the bird's identity, which rig is
          bound, the way back onto the dial, the five readiness gates, the live
          tracking badge and the ✕.

          WHY IT IS A ROW AND NOT A STACK. These were six separately-bordered
          blocks stacked down the 486-px detail column — 175 px of box for what
          is really one instrument, plus a 25 px sticky heading, in a column
          whose whole budget is 713 px. Spanning both columns they cost one
          wrapping row and hand the reclaimed height to the pass surfaces, which
          is the arithmetic that makes "no scrolling for normal operation"
          close at all. It is the FT8 console's merged `.cockpit-qso` strip
          applied here: nothing was deleted, the duplicated FRAMES were.

          AND IT RETIRES A CASCADE TRAP AT THE ROOT. The bird's name used to be
          `.sats-detail > h2`, whose descendant form out-cascaded `.log-entry h2`
          and pinned the wrong heading (styles.css:15086 records it). With the
          identity here there is no heading inside `.sats-detail` at all, and
          the log strip is no longer nested in that card either.

          RENDER CONDITION: a bird is open, OR a track is running. `selected ==
          null && track != null` is a real state — ✕ closes the detail and the
          track keeps running — and the badge is the way back in. */}
      {(track != null || (selected != null && detail != null)) && (
        <div className="sats-armbar" data-testid="sats-armbar">
          {selected != null && detail != null && (
            <div className="sats-arm-id">
              <b>{detail.name}</b>
              {detail.norad != null && (
                <span className="sat-norad">
                  {' · '}
                  {NORAD_LABEL} {detail.norad}
                </span>
              )}
              {detail.status && (
                <span className={`sat-chip ${detail.status === 'alive' ? 'alive' : 'dead'}`}>
                  {detail.status}
                </span>
              )}
            </div>
          )}
          {/* WHICH RIG WILL MOVE — not gated on a track: a pick tunes the moment
              it is made, long before a pass is armed, and "which radio?" is
              exactly the question then. Gated on the HELD bird (the binding is
              engine-global, one hold at a time): RS-44's rig and frequencies
              must never render under AO-91's name. */}
          {binding && detail != null && tuned?.name === detail.name && (
            <SatRadioBinding binding={binding} pegged={pegged} onTogglePeg={writePegged} />
          )}
          {/* LOCK ON — gated on the HOLD, not on the binding line beside it: the
              engine can hold a transponder this section never saw picked, and
              the way back must not disappear with the read-back that named the
              rig. See SatLockOn for why it is not in the Doppler readout. */}
          {heldPickIndex != null && detail != null && (
            <SatLockOn
              onLockOn={() =>
                pickTransponder(
                  detail.name,
                  heldPickIndex,
                  heldT?.description ?? detailTrack?.transponder ?? '',
                )
              }
            />
          )}
          {/* ⚠️ THE BADGE AND THE ✕ ARE EMITTED BEFORE THE RAIL, AND THAT IS A
              HEIGHT DECISION, NOT A READING-ORDER PREFERENCE. `.sat-rail` is
              `flex: 1 1 100%` — a chosen wrap point — so anything after it in
              DOM order lands on a THIRD bar line of its own. Measured at the
              1024×768 floor with a track armed: 99.2 eff px and four lines with
              these two last, 79.9 and three with them here. That ~19 px is
              spent by BOTH columns (the bar spans them), which is why it is
              worth a reorder rather than a comment.
              They still read as a trailing group: `.sats-tracking-badge` carries
              the bar's one `margin-left: auto` and the ✕ gives its own up when it
              follows the badge (styles.css records why two auto margins on one
              line was the bug), so both go flush right and the ■ stop stops
              sliding under the cursor on the 2 s poll. */}
          {track && (
          <span
            className="sats-tracking-badge"
            title={
              // The claim must match the DTO's `mode` — a rotor-less track
              // never borrows the rotor wording. A rotator that QUIT is asked
              // first: its mode is demoted, so nothing below can tell it apart
              // from a track that never had one.
              track.rotorLost
                ? t('sat.badge.rotorLost.title')
                : track.mode === 'pass-only'
                ? t('sat.badge.passOnly.title')
                : track.mode === 'doppler-only'
                  ? // Dial ownership keys on the DOWNLINK leg — the leg that
                    // writes the dial. An uplink-only track drives only the
                    // TX (split) VFO, and claiming the dial for it would
                    // contradict the rail's own "the dial stays yours" on
                    // the same screen (round 3, defect 5).
                    t('sat.badge.dopplerOnly.title', {
                      what: track.dopplerDownlink
                        ? track.downlinkHz != null
                          ? t('sat.badge.dopplerOnly.dialNow')
                          : t('sat.badge.dopplerOnly.dialAtAos')
                        : track.uplinkHz != null
                          ? t('sat.badge.dopplerOnly.splitNow')
                          : t('sat.badge.dopplerOnly.splitAtAos'),
                    })
                  : track.azDeg == null
                    ? t('sat.badge.rotorWaiting.title')
                    : t('sat.badge.rotorDriving.title')
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
            {/* THE WAY BACK to a live pass: the badge text is a button doing
                setSelected(track.name) — the one control that never scrolls
                away once the detail's ✕ exists. Round trip: ✕ → browse →
                badge → back on the dome. Navigation only; the ■ beside it
                stays the separate stop control, untouched. */}
            <button
              className="sats-badge-open"
              onClick={() => setSelected(track.name)}
              title={t('sat.badge.open.title')}
            >
              ⟳ {track.state === 'armed' ? t('sat.badge.armed') : t('sat.badge.tracking')}{' '}
              {track.name}
              {/* The MODE word, only when a surface is missing: the full
                  rotor+doppler track is the unmarked case; every partial one
                  names what it actually drives. A rotator that QUIT says so
                  instead — the operator armed a track with a mast, and the
                  demoted mode alone would read as though he never had one. */}
              {track.rotorLost
                ? t('sat.badge.mode.rotorLost')
                : track.mode === 'doppler-only'
                  ? t('sat.badge.mode.dopplerOnly')
                  : track.mode === 'pass-only'
                    ? t('sat.badge.mode.passOnly')
                    : track.mode === 'rotor-only'
                      ? t('sat.badge.mode.rotorOnly')
                      : ''}{' '}
              ·{' '}
              {track.azDeg == null
                ? t('sat.badge.risesAz', { az: Math.round(track.aosAzDeg) })
                : t('sat.badge.cmdAz', {
                    az: Math.round(track.azDeg),
                    // Never an invented elevation: an az-only rotor was sent none.
                    el:
                      track.elDeg == null
                        ? t('sat.badge.azOnly')
                        : `el ${Math.round(track.elDeg)}°`,
                  })}
            </button>
            <button
              onClick={disarmTrack}
              title={
                track.mode === 'rotor+doppler' || track.mode === 'rotor-only'
                  ? t('sat.track.stop.rotor.title')
                  : t('sat.track.stop.noRotor.title')
              }
            >
              {t('sat.track.stop')}
            </button>
          </span>
          )}
          {selected != null && detail != null && (
            <button
              type="button"
              className="sats-detail-close"
              aria-label={t('sat.detail.close.aria')}
              title={t('sat.detail.close.title')}
              onClick={() => setSelected(null)}
            >
              ✕
            </button>
          )}
          {/* THE READINESS RAIL: the arming chain AS a chain, each gate fixable
              in place. Content-height, and now content-WIDTH too — the five
              gates lay out across the bar instead of down a column. */}
          {detailTrack && (
            <TrackRail
              track={detailTrack}
              rotorOn={rotorOn}
              dopplerOn={dopplerOn}
              vfoMap={vfoMap}
              heldDesc={detailTrack.transponder ?? heldT?.description ?? null}
              heldInverting={detailTrack.inverting || !!heldT?.invert}
              simplex={!!(binding?.simplex && detail != null && tuned?.name === detail.name)}
              autoPicked={!!(tuned?.auto && detail != null && tuned.name === detail.name)}
              canPick={(detail?.transmitters.length ?? 0) > 0}
              nowSecs={nowSecs}
              onStop={disarmTrack}
              onDopplerOn={writeDopplerOn}
              onVfoMap={writeVfoMap}
              onGoToPicker={goToPicker}
              onRefreshElements={() => void refreshTles()}
              scrollRef={railRef}
            />
          )}
        </div>
      )}

      {/* THE GATE LADDER, reordered (2026-08 schedule rethink): only !gridSet
          short-circuits — no grid, no passes, nothing honest to show. The old
          favs.size===0 branch replaced the entire planning column with one
          sentence while ~1,800 all-bird pass rows sat unread in memory; an
          empty ★ set is an inline line INSIDE the schedule now, with the
          discovery band open beneath it carrying the actual answer. */}
      {!gridSet ? (
        <div className="sats-empty">{t('sat.noGrid')}</div>
      ) : (
        /* THE PLANNING COLUMN — a bounded flex shell: the Next/Best strip
           (fit), the schedule (THE grower, and the section's one column-1
           scroll owner lives inside it), and the radio quadrant (fit) pinned
           to the bottom.

           WHY THE RADIO QUADRANT IS HERE AND NOT IN THE PASS COLUMN. Operator,
           2026-08-03: the frequencies and "the slections of what to select"
           belong on the first screen "as a prominent feature". `.sats-side`
           already spanned every row below the header, so capping the schedule
           on its own handed height to a column with nothing to spend it on —
           the reclaimed height had to be spent on a NAMED thing or the cap
           would rot. This is that thing, and it is what the automated-rotor
           operator watches while the mast looks after itself.

           IT ALSO BOUNDS THE SCHEDULE WITHOUT A PIXEL OR A ROW CAP. The
           schedule scroller keeps `flex: 1 1 auto` and no max-height at all
           (panel-overflow.test.ts's "the discovery band is bounded by ROW COUNT,
           never a pixel height" passes verbatim) — what stops it eating the page
           is the fit-content block underneath it in a bounded column, and the
           number of rows is set by the window rather than by a constant.

           MEASURED, because a round-1 comment here claimed "15 at the floor,
           more than 30 in portrait" and neither figure was ever produced: it is
           5 rows at the 1024×768 floor, 7–8 at 1366×768 and 1280×800, 16 at
           1920×1080, 24 in a 1200×1390 portrait and 30 at 3440×1440. The
           operator's "like the first 10 lines" arrives at about 900 px of window
           height. The per-size statement lives in the CHANGELOG and the guide;
           styles.css's `.sats-plan` comment carries the full table. */
        <section className="sats-plan">
          {/* THE NEXT/BEST STRIP — still the primary action surface
              (litigation ①: a pass is WORKED from here, not merely opened),
              re-ruled 2026-08 after the field report: the old "Next up" was a
              quality rank wearing a clock label, and read favorites-only.
              Two labelled pairs over ALL admitted birds now — "Next" = the 2
              soonest workable passes in clock order (in-progress first, with
              the backscan's already-up honesty), "Best 24 h" = the 2 highest
              passScore passes, deduped against Next. ★ marks a favourite, the
              mode pill rides every classified row, and ▶ Work + the open
              click run the same chain for any bird — an unstarred linear
              bird can finally surface. Side tags spend width, not height:
              four content rows, within one row-height of the old three. */}
          {strip.next.length > 0 && (
          <section className="sats-best">
            {(
              [
                [t('sat.strip.next.label'), t('sat.strip.next.why'), strip.next],
                [t('sat.strip.best.label'), t('sat.strip.best.why'), strip.best],
              ] as const
            )
              .filter(([, , rows]) => rows.length > 0)
              .map(([label, why, rows]) => (
                <div key={label} className="sats-best-group">
                  <h2 className="sats-best-tag" title={why}>
                    {label}
                  </h2>
                  <div className="sats-best-rows">
                    {rows.map(({ pass: p, fav }) => {
                      const pill = pillFor(p.name, p.norad)
                      return (
                        <div
                          key={`${p.name}-${p.aosUnix}`}
                          className={`sats-best-row${p.aosUnix <= nowSecs ? ' live' : ''}`}
                        >
                          <PassArcMini pass={p} />
                          <button
                            className="sats-best-open"
                            onClick={() => setSelected(p.name)}
                            /* ONE LINE, ellipsized (styles.css). The why-line is
                               a sentence, and at the planning column's width it
                               wrapped to two or three lines — measured, the
                               four-row strip cost 236 px of a 719 px column and
                               left the schedule three rows. It carries itself on
                               `title`, and the numbers an operator picks a pass
                               by (time, countdown, elevation) are at the front
                               of the sentence where the ellipsis never reaches. */
                            title={t('sat.strip.row.title', {
                              name: p.name,
                              why: whyLine(p, nowSecs),
                            })}
                          >
                            {fav && (
                              <span className="sat-fav-mark" title={t('sat.strip.fav.title')}>
                                ★
                              </span>
                            )}
                            <b>{p.name}</b>
                            {pill && <span className="sat-mode-pill">{pill}</span>}{' '}
                            {whyLine(p, nowSecs)} <EarnChips earn={p.earn} />
                          </button>
                          <button
                            className="sat-work"
                            onClick={() => workPass(p)}
                            title={workTitle}
                          >
                            {t('sat.work.label')}
                          </button>
                        </div>
                      )
                    })}
                  </div>
                </div>
              ))}
          </section>
          )}

          <section className="sats-sched">
            {/* The control strip NEVER scrolls (fit=content; the table below
                lives in the section's ONE scroller). The disclosure chip
                lives here so it stays findable under a 126-row schedule —
                and its live count is the discoverability mechanism: a number
                that moves is what peripheral vision catches. */}
            <div className="sats-sched-strip">
              <h2>{t('sat.schedule.head', { hours: SCHEDULE_HOURS })}</h2>
              <button
                type="button"
                className="sats-more-chip"
                aria-expanded={discoveryOpen}
                onClick={() => {
                  setDiscOpen(!discoveryOpen)
                  if (discoveryOpen) setDiscAll(false)
                }}
                title={t('sat.discovery.chip.title')}
              >
                {t('sat.discovery.chip', { count: discovery.length })}{' '}
                {discoveryOpen ? '▴' : '▾'}
              </button>
            </div>
            <div className="sats-sched-scroll">
            <table>
              <thead>
                <tr>
                  <th>★</th>
                  {schedTh(t('sat.schedule.column.bird'), 'bird')}
                  {schedTh(t('sat.schedule.column.aos'), 'aos')}
                  <th></th>
                  {schedTh(t('sat.schedule.column.maxEl'), 'el')}
                  {schedTh(t('sat.schedule.column.dur'), 'dur')}
                  <th>{t('sat.schedule.column.path')}</th>
                  {schedTh(t('sat.schedule.column.status'), 'status')}
                  {/* Phase 2: a SORT KEY the operator clicks — never a silent
                      reorder. Default order stays soonest-AOS. */}
                  {schedTh(t('sat.schedule.column.needed'), 'need')}
                  <th>⏰</th>
                  {/* The work column renders for EVERYONE — the rotor gate on
                      this column hid the entire tracking arc from the
                      rotor-less operator (the largest satellite population). */}
                  <th></th>
                </tr>
              </thead>
              <tbody className="fav">
                {/* The ladder's old middle branches, inline: the schedule
                    frame (and the discovery band below it) stay on screen,
                    and the words stay exactly as honest as the branches were. */}
                {upcoming.length === 0 && (
                  <tr className="sats-inline-empty">
                    <td colSpan={11}>
                      {favs.size === 0
                        ? t('sat.schedule.empty.noFavs')
                        : t('sat.schedule.empty.noPasses', {
                            hours: SCHEDULE_HOURS,
                            why:
                              view == null
                                ? t('sat.schedule.empty.noElements')
                                : t('sat.schedule.empty.excluded'),
                          })}
                    </td>
                  </tr>
                )}
                {upcoming.map((p) => {
                  const inPass = p.aosUnix <= nowSecs
                  const armed = p.name.toUpperCase() in alarms
                  const pill = pillFor(p.name, p.norad)
                  return (
                    <tr
                      key={`${p.name}-${p.aosUnix}`}
                      className={`${selected === p.name ? 'sel' : ''}${inPass ? ' live' : ''}`}
                      onClick={() => setSelected(p.name)}
                    >
                      <td>
                        <button
                          className={`sat-star${isSatChased(p.name, p.norad, chaseKeys) ? ' on' : ''}`}
                          onClick={(e) => {
                            e.stopPropagation()
                            onToggleFav(p.name, p.norad)
                          }}
                          title={t('sat.schedule.unstar.title')}
                        >
                          ★
                        </button>
                      </td>
                      <td className="sat-name">
                        {p.name}
                        {/* The mode pill ([FM voice]/[Linear SSB/CW]/…), when
                            the catalog can say — absent class, no pill. */}
                        {pill && <span className="sat-mode-pill">{pill}</span>}
                      </td>
                      <td>{hhmm(p.aosUnix)}</td>
                      <td className="sat-count">{countdown(p, nowSecs)}</td>
                      <td>{Math.round(p.maxElDeg)}°</td>
                      <td>{Math.max(1, Math.round((p.losUnix - p.aosUnix) / 60))} m</td>
                      <td>{wind8(p.aosAzDeg)}→{wind8(p.losAzDeg)}</td>
                      <td>
                        {p.status === 'alive' && (
                          <span className="sat-chip alive" title={t('sat.schedule.alive.title')}>
                            {t('sat.schedule.alive.label')}
                          </span>
                        )}
                        {(p.status === 'dead' || p.status === 're-entered') && (
                          <span className="sat-chip dead" title={t('sat.schedule.dead.title')}>
                            {p.status}
                          </span>
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
                          title={armed ? t('sat.schedule.alarm.on') : t('sat.schedule.alarm.off')}
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
                            title={t('sat.schedule.alarm.lead.title')}
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
                            title={t('sat.schedule.stop.title')}
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
                            {t('sat.work.short')}
                          </button>
                        )}
                      </td>
                    </tr>
                  )
                })}
              </tbody>
              {/* THE DISCOVERY BAND — the second tbody in the SAME table, so
                  its rows share the schedule's columns and read as the same
                  schedule, never a second list to mentally merge. Collapsed =
                  ZERO rows in the DOM. One row per BIRD (its best 24 h pass),
                  worth-sorted, capped at DISCOVERY_ROW_CAP with the remainder
                  stated — satDiscovery.ts owns the volume law. Starring a row
                  PROMOTES the bird into the ★ tbody above, where earn chips
                  and the ⏰ alarm live (both are withheld here on purpose —
                  the module header says why). */}
              <tbody className="more">
                {discoveryOpen && (
                  <>
                    <tr className="sats-more-bar">
                      <td colSpan={11}>{t('sat.discovery.bar')}</td>
                    </tr>
                    {discShown.map((b) => {
                      const p = b.best
                      const pill = modePillWord(b.birdType)
                      return (
                        <tr
                          key={`d-${b.norad ?? b.name}`}
                          className={`sats-disc${selected === b.name ? ' sel' : ''}${p.aosUnix <= nowSecs ? ' live' : ''}`}
                          onClick={() => setSelected(b.name)}
                        >
                          <td>
                            <button
                              className="sat-star"
                              onClick={(e) => {
                                e.stopPropagation()
                                onToggleFav(b.name, b.norad)
                              }}
                              title={t('sat.discovery.star.title')}
                            >
                              ★
                            </button>
                          </td>
                          <td className="sat-name">
                            {b.name}
                            {pill && <span className="sat-mode-pill">{pill}</span>}
                            <span
                              className="sats-disc-note"
                              title={t('sat.discovery.workable.title')}
                            >
                              {t('sat.discovery.workable', { count: b.workable })}
                            </span>
                            {b.altKm != null && (
                              <span
                                className="sats-disc-note"
                                title={t('sat.discovery.altitude.title')}
                              >
                                {t('sat.discovery.altitude', { km: Math.round(b.altKm) })}
                              </span>
                            )}
                          </td>
                          {/* aosClamped: the pass out-lasted the backend's
                              6 h backscan, so aosUnix is the scan window's
                              edge — a clock time the sky never saw. Say what
                              was DONE ("already up"), never the fabrication;
                              the duration is then a lower bound, marked +. */}
                          <td>
                            {p.aosClamped ? (
                              <span title={t('sat.discovery.clamped.title')}>
                                {t('sat.why.alreadyUp')}
                              </span>
                            ) : (
                              hhmm(p.aosUnix)
                            )}
                          </td>
                          <td className="sat-count">{countdown(p, nowSecs)}</td>
                          <td title={qualityWord(p.maxElDeg)}>{Math.round(p.maxElDeg)}°</td>
                          <td>
                            {Math.max(1, Math.round((p.losUnix - p.aosUnix) / 60))}
                            {p.aosClamped ? '+' : ''} m
                          </td>
                          <td>
                            {wind8(p.aosAzDeg)}→{wind8(p.losAzDeg)}
                          </td>
                          {/* Status: EMPTY on purpose. These rows carry the
                              bundled catalog's status, which reads 'alive'
                              for every bird admitted here — including birds
                              the live SatNOGS overlay (favorites-only) knows
                              are dead. A constant chip that is sometimes
                              false is false hope, not information. */}
                          <td></td>
                          {/* Needed: earn is stamped only by
                              get_sat_pass_needs — a non-★ row shows worth-to-
                              work, never a guessed earn. Absent, not zero. */}
                          <td className="sat-need"></td>
                          {/* ⏰ withheld: the alarm map is name-keyed through
                              the schedule's alias resolution; these rows
                              carry the element file's CURRENT name, and an
                              alarm armed under it on a renamed bird would
                              silently never fire. Star first — the promoted
                              row above carries the bell. */}
                          <td></td>
                          <td>
                            <button
                              className="sat-track"
                              onClick={(e) => {
                                e.stopPropagation()
                                workPass(p)
                              }}
                              title={workTitle}
                            >
                              {t('sat.work.short')}
                            </button>
                          </td>
                        </tr>
                      )
                    })}
                    {!discAll && discovery.length > DISCOVERY_ROW_CAP && (
                      <tr className="sats-more-all">
                        <td colSpan={11}>
                          {/* The cap sits on the worth-sorted list, so it can
                              never hide the best thing — and the remainder is
                              always on screen. */}
                          <button type="button" onClick={() => setDiscAll(true)}>
                            {t('sat.discovery.showAll', { count: discovery.length })}
                          </button>
                        </td>
                      </tr>
                    )}
                    {discovery.length === 0 && (
                      <tr className="sats-inline-empty">
                        <td colSpan={11}>{t('sat.discovery.empty')}</td>
                      </tr>
                    )}
                  </>
                )}
              </tbody>
            </table>
            </div>
          </section>

          {/* ============== THE RADIO QUADRANT — frequencies + transponder =====
              Two instruments the operator uses together, side by side and
              permanently on screen: what the radio is tuned to right now (the
              Doppler legs and the passband strip) and what to point it at (the
              transponder chooser). They were 1,100–1,800 px down a scrolling
              column; a pick is the most consequential control in the section
              (a wrong pick is a wrong uplink) and it was never on the first
              screen.

              fit-content, never a grower: whatever this costs comes out of the
              schedule's row count above it, which is exactly where a bird with
              an unusual number of transmitters should show up. */}
          <section className="sats-radio">
            <div className="sats-radio-cell">
              <h2 className="sats-radio-title">{t('sat.radio.frequencies.head')}</h2>
              {detailTrack ? (
                <>
                  <DopplerReadout
                    rotor={detailTrack}
                    dopplerOn={dopplerOn}
                    held={heldIndex != null || detailTrack.transponder != null}
                    uplinkOnly={vfoMap === 'uplink-only'}
                    // Simplex from the engine's binding; a beacon from the
                    // held record having no uplink at all.
                    undrivableUplink={heldSimplex || (heldT != null && heldT.uplinkLowHz == null)}
                    txMode={detailTrack.txMode ?? null}
                  />
                  {/* The strip goes under the readout: the readout says what the
                      radio is tuned to, the strip says where that puts the
                      operator inside the passband. */}
                  <PassbandStrip rotor={detailTrack} />
                </>
              ) : (
                <p className="sats-radio-idle">
                  {detail == null ? t('sat.radio.idle.noBird') : t('sat.radio.idle.noTrack')}
                </p>
              )}
            </div>
            <div className="sats-radio-cell">
              <h2 className="sats-radio-title">{t('sat.transponder.head')}</h2>
              {detail != null && detail.transmitters.length > 0 ? (
                <>
                  {/* THE TRANSPONDER CHOOSER — the most consequential control in
                      the section (a wrong pick is a wrong uplink), so it is on
                      the first screen beside the frequencies it decides, never
                      scrolled away. Dead entries collapse behind "show N
                      inactive". The wire index stays the RAW index of the full
                      list (dead included): the backend indexes the very list
                      get_sat_detail returned and refuses dead picks by name. */}
                  <div
                    className="sat-tp-list"
                    data-testid="sat-tp-list"
                    role="radiogroup"
                    aria-label={t('sat.transponder.list.aria')}
                    ref={pickerRef}
                  >
                    <label className="sat-tp-card">
                      <input
                        type="radio"
                        name="sat-transponder"
                        checked={heldIndex == null}
                        onChange={() => pickTransponder(detail.name, null)}
                        aria-label={t('sat.transponder.none.aria')}
                      />
                      <span className="sat-tp-desc">{t('sat.transponder.none.label')}</span>
                    </label>
                    {aliveShown.map(({ t: tx, aliveIndex }) => (
                        <label
                          key={aliveIndex ?? tx.description}
                          className={`sat-tp-card${heldIndex === aliveIndex ? ' held' : ''}`}
                        >
                          <input
                            type="radio"
                            name="sat-transponder"
                            checked={heldIndex === aliveIndex}
                            onChange={() =>
                              aliveIndex != null &&
                              pickTransponder(detail.name, aliveIndex, tx.description)
                            }
                            aria-label={t('sat.transponder.card.aria', {
                              description: tx.description,
                            })}
                          />
                          <span className="sat-tp-main">
                            <span className="sat-tp-desc">
                              {tx.description}
                              {tx.invert && (
                                <span className="sat-invert" title={t('sat.inverting.title')}>
                                  {t('sat.inverting.label')}
                                </span>
                              )}
                              {kindWord(tx.kind) && (
                                <span className="sat-tp-kind">{kindWord(tx.kind)}</span>
                              )}
                              {tx.downlinkMode == null &&
                                tx.uplinkMode == null &&
                                tx.mode != null && <span className="sat-tp-kind">{tx.mode}</span>}
                            </span>
                            <span className="sat-tp-legs">
                              <span className="sat-tp-leg">
                                ↓ <b>{fmtLeg(tx.downlinkLowHz, tx.downlinkHighHz)}</b>
                                {tx.downlinkMode ? ` ${tx.downlinkMode}` : ''}
                              </span>
                              <span className="sat-tp-leg">
                                ↑ <b>{fmtLeg(tx.uplinkLowHz, tx.uplinkHighHz)}</b>
                                {tx.uplinkMode ? ` ${tx.uplinkMode}` : ''}
                              </span>
                            </span>
                            {heldIndex === aliveIndex && tuned?.auto && (
                              <span className="sat-tp-auto">{t('sat.transponder.auto')}</span>
                            )}
                          </span>
                        </label>
                      ))}
                    {!tpAll && aliveRows.length > TP_ALIVE_CAP && (
                      <button
                        type="button"
                        className="sat-tp-more"
                        onClick={() => setTpAll(true)}
                        title={t('sat.transponder.showAll.title')}
                      >
                        {t('sat.transponder.showAll', { count: aliveRows.length })}
                      </button>
                    )}
                    {tpRows.some((r) => !r.t.alive) &&
                      (showDead ? (
                        tpRows
                          .filter((r) => !r.t.alive)
                          .map(({ t: tx }, k) => (
                            <div key={`dead-${k}`} className="sat-tp-card off">
                              <span className="sat-tp-deadmark" aria-hidden>
                                ○
                              </span>
                              <span className="sat-tp-main">
                                <span className="sat-tp-desc">
                                  {tx.description}
                                  <span className="sat-tp-kind">{t('sat.transponder.dead')}</span>
                                </span>
                                <span className="sat-tp-legs">
                                  <span className="sat-tp-leg">
                                    ↓ <b>{fmtLeg(tx.downlinkLowHz, tx.downlinkHighHz)}</b>
                                  </span>
                                  <span className="sat-tp-leg">
                                    ↑ <b>{fmtLeg(tx.uplinkLowHz, tx.uplinkHighHz)}</b>
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
                          title={t('sat.transponder.showDead.title')}
                        >
                          {t('sat.transponder.showDead', {
                            count: tpRows.filter((r) => !r.t.alive).length,
                          })}
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
                          <T
                            k="sat.transponder.txMode.commanded"
                            tags={{ b: <b /> }}
                            vals={{
                              tx: detailTrack.txMode,
                              down: heldT.downlinkMode ?? '',
                            }}
                          />
                        ) : (
                          // The component KNOWS the cause — name the live one
                          // instead of an enumeration whose first entry
                          // ("Doppler off") is false in the default state, one
                          // row under "correcting the downlink".
                          t('sat.transponder.txMode.notCommanded', {
                            up: txSwapMode,
                            down: heldT.downlinkMode ?? '',
                            why: !dopplerOn
                              ? t('sat.transponder.txMode.why.dopplerOff')
                              : !detailTrack.dopplerUplink
                                ? t('sat.transponder.txMode.why.notDriving')
                                : t('sat.transponder.txMode.why.shared'),
                          })
                        )
                      ) : (
                        t('sat.transponder.txMode.forecast', {
                          up: txSwapMode,
                          down: heldT.downlinkMode ?? '',
                        })
                      )}
                    </div>
                  )}
                  {/* The uplink-only branches key on the TRACK's per-leg truth
                      (the engine's own legs), never the settings mirror alone:
                      "only the transmit VFO is tuned" was rendered while the
                      mapping was unconfirmed for the radio in play — a present-
                      tense claim directly under the engine's own "nothing was
                      tuned" binding note. With no live track the line states
                      the mechanism conditionally instead of claiming a write. */}
                  {heldIndex != null && (
                    <div className={`sat-tp-state${dopplerLive ? '' : ' warn'}`}>
                      {!dopplerOn
                        ? t('sat.transponder.state.dopplerOff')
                        : vfoMap === 'uplink-only'
                          ? heldSimplex || heldT?.uplinkLowHz == null
                            ? t('sat.uplinkOnly.noLeg')
                            : detailTrack != null
                              ? detailTrack.dopplerUplink
                                ? t('sat.transponder.state.uplinkOnly.driving')
                                : t('sat.transponder.state.uplinkOnly.unconfirmed')
                              : t('sat.transponder.state.uplinkOnly.pending')
                          : t('sat.transponder.state.tuning')}
                    </div>
                  )}
                  {tuned && tuned.name !== detail.name && (
                    <div className="sat-tp-state warn">
                      {t('sat.transponder.otherBird', { name: tuned.name })}
                    </div>
                  )}
                  <div className="sats-credit">{t('sat.credit.satnogs')}</div>
                </>
              ) : (
                <div className="sats-credit">
                  {detail == null
                    ? t('sat.credit.noBird')
                    : detail.dataFetchedAt == null
                    ? t('sat.credit.noData')
                    : t('sat.credit.noTransmitters')}
                </div>
              )}
            </div>
          </section>
        </section>
      )}

      <aside className="sats-side">
        {selected && detail && (
          <section className="sats-detail">
            {/* THE TWO MAPS, SIDE BY SIDE. Operator, 2026-08-03: "It also would
                be great to bring the other map to the main screen, the main
                screen that shows now with los overall could overall be reduced
                in size." The globe was already in this section — it was just
                ~1,860 px below the top of a 713 px column, i.e. two screens of
                scrolling at every window size. It is not new machinery; it is
                the same MapView in embedded mode, moved up and made small,
                beside a dome that came down.

                THIS ONE MOVE IS WHY THE HEIGHT BUDGET CLOSES. Stacked, the two
                square graphics cost more than the whole column. Abreast, the
                row is the height of the taller one — and both are capped
                against --vh-eff, so the row is close to width-invariant.

                The dome keeps the left (it is the instrument a mast is turned
                by, and it is read first); the globe takes the right, where
                "whose grids does the footprint cross" is a glance rather than a
                journey. */}
            <div className="sats-pass-graphics" data-testid="sats-pass-graphics">
              {detail.pass ? (
                <SkyDome
                  name={detail.name}
                  pass={detail.pass}
                  track={detail.passTrack}
                  rotor={detailTrack}
                  nowSecs={nowSecs}
                />
              ) : nextBeyond ? (
                <div className="sat-passline">
                  {t('sat.passline.beyond', {
                    time: hhmm(nextBeyond.aosUnix),
                    countdown: countdown(nextBeyond, nowSecs),
                  })}
                </div>
              ) : (
                <div className="sat-passline">{t('sat.passline.none')}</div>
              )}
              <div
                /* The ground-track globe, PROMOTED to the dome's side (it used
                   to be the last block in a 2,300 px card). Sizing lives in
                   styles.css (.sat-globe-box) with the rest of the section — an
                   inline style block is invisible to every cascade-computing
                   guard, and capping HEIGHT alone let the box go 2:1 wide at
                   3440 px. Its old hard `min-height: 260px` is now written to
                   yield, because this box lives under a bounded parent. */
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
            </div>
            {detail.pass && (
              <PassTimeline
                pass={detail.pass}
                tcaUnix={tcaUnix}
                nowSecs={nowSecs}
                earn={detailEarn}
              />
            )}
          </section>
        )}
        {/* LOG THIS QSO — the section's missing half (operator, 0.27.3: "I
            dont have a spot to log within the satellites section to log my
            sat qso's. Lets not reinvent anything, logging sections exist in
            the phone area, can we drop that in?").

            Dropped in, not rebuilt: this is the SAME shared LogEntry the
            Phone and CW cockpits render, with the same props they pass, and
            it logs the SAME ORDINARY CONTACT they log. It is NOT a
            satellite-aware log — nothing about the bird reaches the record.
            Satellite tagging (ADIF PROP_MODE/SAT_NAME, which LoTW needs for
            satellite credit — BOTH: TQSL refuses to sign a record carrying
            only one of the pair, in either direction, so a half-tag costs
            the whole QSO, see `Engine::log_qso`) is NOT YET BUILT —
            deferred, not designed away; see the note line
            below, docs/guide/satellites.md and the CHANGELOG, all of which
            say so plainly so nobody waits on credit that isn't coming. Two
            further consequences of dropping in the shared strip UNCHANGED
            are documented in the guide rather than fixed here: during Field
            Day this strip logs to the ordinary log while the Phone and CW
            strips route to the contest log (App.tsx passes those two
            `fieldDay`; each cockpit then supplies its own literal `fdMode` —
            "PH" in PhoneCockpit, "CW" in CwCockpit — so wiring this section
            up means BOTH, and there is no third mode literal to reuse), and
            the recorded MODE is folded from the sideband, which is wrong on
            a digital tier (see `adifModeFromStation`).

            WHY IT IS A SIBLING OF `.sats-detail` AND NOT INSIDE IT — and
            this half is a BUG FIX, not a placement preference. LogEntry
            holds every field in its own local state, so while it was nested
            in `{selected && detail && …}` (SatellitesView.tsx:3194 before
            this change) hitting ✕, pressing Escape or clicking a different
            bird unmounted it and destroyed a half-typed contact. Mid-pass,
            between overs, that is the worst possible moment to lose a
            callsign. It now renders from `snap` alone — the operator's own
            radio state — so nothing about which bird is open, whether a
            track is armed, or whether AOS has happened can take the form
            away. `SatellitesView.remount.test.tsx` is the guard.

            WHY HERE IN THE COLUMN. Directly under the pass block: the sky
            dome and the pass timeline stay on screen while he types, and
            the frequencies and the transponder chooser are in the OTHER
            column entirely now, so nothing he needs mid-pass is behind the
            form. Below it is the Birds catalog, which is the section's one
            deliberately below-the-fold surface. No new scroller, no sticky,
            no fixed positioning: a plain in-flow block in the column that
            already owns the overflow. Document order is pinned by a test. */}
        {snap && (
          <div className="sats-log">
            {/* `exchange="satellite"`: what the two stations pass each other
                through a bird. It decides two fields and only two — a Grid
                box beside the reports (grid-for-grid IS the exchange here),
                and no POTA/SOTA row (operator, 0.28.1: "that section still
                has a pota/sota section, which is shouldnt"). One prop, not a
                fork of the shared component. Net effect on this column,
                measured: the strip is 48 px SHORTER than 0.28.1's, which is
                what he asked for one message earlier — "make it smaller so
                the logging and az/el map are visable together". */}
            <LogEntry
              onOpenLogbook={onOpenLogbook}
              snap={snap}
              mode={logMode}
              defaultRst={logMode === 'CW' ? '599' : '59'}
              exchange="satellite"
            />
            <p
              className="sats-log-note"
              /* Clamped to ONE line in styles.css (`line-clamp: 1`) — the full
                 sentence stays in the DOM for a screen reader, and is here for a
                 pointer. */
              title={t('sat.log.note.title')}
            >
              {/* ONE sentence-set with its emphasis as markers: the four <b>
                  spans sit mid-sentence, and a language that orders the clause
                  differently cannot be served by gluing five fragments. */}
              <T k="sat.log.note" tags={{ b: <b /> }} />
            </p>
          </div>
        )}

        <section className="sats-favmgr">
          <h2>{t('sat.birds.head', { count: allBirds.length })}</h2>
          <input
            className="sats-search"
            type="text"
            placeholder={t('sat.birds.search.placeholder')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            spellCheck={false}
          />
          <ul>
            {allBirds.map((b) => {
              // What the CATALOG said (dead / re-entered / alive-but-silent),
              // and — for a ★ bird with no row of its own — why it could not
              // be placed. Both can be true at once; both are shown.
              const health = satBirdHealth(b.status, b.amateur)
              const gap = b.reason == null ? null : satExcludedHealth(b.reason)
              return (
                <li
                  key={`${b.norad ?? ''}-${b.name}`}
                  className={selected === b.name ? 'sel' : ''}
                >
                  <button
                    className={`sat-star${isSatChased(b.name, b.norad, chaseKeys) ? ' on' : ''}`}
                    onClick={() => onToggleFav(b.name, b.norad)}
                    title={t('sat.birds.star.title')}
                  >
                    ★
                  </button>
                  <button className="sat-pick" onClick={() => setSelected(b.name)}>
                    {b.name}
                  </button>
                  {health && (
                    <span className={`sat-chip ${health.tone}`} title={health.title}>
                      {health.label}
                    </span>
                  )}
                  {gap && (
                    <span className={`sat-chip ${gap.tone}`} title={gap.title}>
                      {gap.label}
                    </span>
                  )}
                  {/* How high it is right now. "alt" is not decoration: the
                      other km figure an operator reads on these surfaces is
                      RANGE (distance from the shack), and the two are wildly
                      different numbers for the same bird. A row with no
                      elements has no subpoint and so shows nothing at all —
                      never 0 km. */}
                  {b.altKm != null && (
                    <span className="sat-row-alt" title={t('sat.birds.alt.title')}>
                      {t('sat.birds.alt', { km: Math.round(b.altKm) })}
                    </span>
                  )}
                </li>
              )
            })}
            {view == null && <li className="sats-empty">{t('sat.birds.empty')}</li>}
          </ul>
        </section>
      </aside>

      {/* THE >14 d ARM CONFIRM (phase 3): the STALE threshold's second
          consequence. Three honest exits — Refresh (fetch fresh elements,
          then re-run the whole work-pass chain against them), Arm anyway
          (the held chain tail runs as clicked), Cancel (nothing arms).
          Portaled ui/Dialog — the one sanctioned modal primitive. */}
      <Dialog
        open={armConfirm != null}
        onOpenChange={(o) => {
          if (!o) setArmConfirm(null)
        }}
        title={t('sat.armConfirm.title')}
        description={
          armConfirm
            ? t('sat.armConfirm.body', {
                name: armConfirm.pass.name,
                days: Math.round(armConfirm.ageDays),
              })
            : ''
        }
      >
        <div className="sat-armconfirm-actions">
          <button
            type="button"
            className="settings-save"
            disabled={tleRefreshing}
            onClick={() => {
              const c = armConfirm
              setArmConfirm(null)
              if (c == null) return
              // Refresh, then re-run the chain from the top: the fresh set
              // re-fetches detail (fresh age — no second confirm) and the
              // arm matches the same schedule row (±3 min tolerance covers
              // the refreshed elements nudging AOS by seconds). If the
              // refresh fails, its toast says why and the confirm simply
              // reappears on the next work click — never a silent arm.
              void refreshTles().then(() => workPass(c.pass))
            }}
          >
            {tleRefreshing ? t('sat.armConfirm.refreshing') : t('sat.armConfirm.refresh')}
          </button>
          <button
            type="button"
            onClick={() => {
              const c = armConfirm
              setArmConfirm(null)
              c?.proceed()
            }}
          >
            {t('sat.armConfirm.armAnyway')}
          </button>
          <button type="button" onClick={() => setArmConfirm(null)}>
            {t('sat.armConfirm.cancel')}
          </button>
        </div>
      </Dialog>
    </div>
  )
}
