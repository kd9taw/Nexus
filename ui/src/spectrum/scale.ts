// What a frame's values MEAN, in one place: the axis a host labels, the dB per unit the renderer's
// average detector and the auto range read, and the step from the backend's frame to the renderer's.
//
// THE ONE CONVERSION. The backend says what a row's 0..1 values mean in `scale` (dto.rs
// `SpectrumScale`): `dbfs {loDb, hiDb}` on the audio feed, `relative` on every radio's own scope. The
// renderer wants a number, `dbPerUnit` (types.ts). `axisOf` is the only place one becomes the other,
// and `valueToDb`/`dbToValue` are the only arithmetic between a value and a dB, so a host never has to
// choose between WF_DB_SPAN, a scale's span and a guess:
//
//   dB(v) = topDb + (v − 1) · dbPerUnit          v(dB) = 1 + (dB − topDb) / dbPerUnit
//
// On dBFS, `topDb` is `hiDb` (0, full scale) and `dbPerUnit` is `hiDb − loDb` (120): the audio axis,
// exact at every window length (tempo_core::spectrum `power_to_display`, pinned in Rust by
// `the_absolute_db_axis_reads_the_same_at_every_window_length`). On a relative scale `topDb` is 0, the
// top of the radio's own display, and the dB below it come from what the maker publishes about that
// display, source by source (`RELATIVE` below).
//
// NO dBm, AND THAT IS STRUCTURAL. Flex publishes levels against the pan's `min_dbm`/`max_dbm`, but how
// Nexus maps the radio's bins is unconfirmed, and the FT-710's display line is calibrated to nothing.
// The backend therefore marks every radio `relative`, and `ScaleAxis.kind` has no dBm member to print:
// when a source is confirmed, its scale gains a kind on the wire (wire-consistency.test.ts then makes
// `SpectrumScale` in types.ts list it), and the exhaustive switch in `absoluteAxis` stops compiling until
// the axis says what that kind is. An unknown or malformed scale reads as relative, never
// as absolute: a label we cannot back is worse than none.
//
// THE AXIS IS THE TRACE'S OWN MAPPING. `traceY` is the y both renderers draw a value at in the trace
// band (canvas2d.ts `drawTrace`, webgl2.ts `traceY`: `h − strength · (h − 1)`), and the ticks are placed
// through it, so a tone that reads −20 dBFS sits exactly on the −20 tick.

import type { SpectrumFrameWire, SpectrumScale } from '../types'
import { WF_DB_SPAN } from '../waterfall'
import { safeDbPerUnit, strengthIn } from './aggregate'
import type { DisplayRange, SpectrumFrame } from './types'

export interface ScaleAxis {
  /** `dbfs`: absolute, against digital full scale. `relative`: dB below the top of the radio's own
   *  display. There is deliberately no dBm member (see the header). */
  kind: 'dbfs' | 'relative'
  /** dB at value 1: full scale on dBFS (0), the top of the radio's display on a relative axis (0). */
  topDb: number
  /** dB one unit of value spans. */
  dbPerUnit: number
  /** Whether dB graduations may be printed. False where the source's dB per unit is a stand-in. */
  graduated: boolean
}

/**
 * dB a RELATIVE source's 0..1 spans, and whether that figure is the maker's own.
 *
 * - `civ`: the waveform is 0–160 across the scope's display (`civ::scope` POINT_MAX), and Icom gives
 *   that display 80 dB (IC-7300: "waveform display area (vertical axis) 80 dB", REF −20 to +20 dB; the
 *   IC-9700 and IC-705 scopes are the same design). The REF shifts the trace, not the span, so the axis
 *   reads dB below the display's top and the host labels the REF beside it when it knows it.
 *
 * Every other relative source has no published dB per unit, so it keeps the 120 dB every reading on
 * screen assumes today (WF_DB_SPAN) and prints no numbers:
 * - `flex`: the bins are levels between the pan's `min_dbm` and `max_dbm`, but Nexus maps them as
 *   `bin / 65535` and has never seen a radio. The SmartSDR API sizes the pan's display in `ypixels` and
 *   gives `min_dbm`/`max_dbm` as its y-pixel limits, and a working client (AetherSDR) decodes a bin as a
 *   y pixel counted down from `max_dbm`. Neither is that mapping, so until it is confirmed the window is
 *   not a dB figure this axis may print.
 * - `yaesu`: the FT-710's inverted display line; nothing calibrates its bytes.
 */
const RELATIVE: ReadonlyMap<string, { dbPerUnit: number; graduated: boolean }> = new Map([
  ['civ', { dbPerUnit: 80, graduated: true }],
])

/** A relative source with no published figure. */
const UNPUBLISHED = { dbPerUnit: WF_DB_SPAN, graduated: false }

/** The absolute axis a scale states, or null when it states none. */
function absoluteAxis(scale: SpectrumScale): ScaleAxis | null {
  switch (scale.kind) {
    case 'dbfs': {
      const { loDb, hiDb } = scale
      // A span the backend could never send is not an axis anybody may print.
      if (!(Number.isFinite(loDb) && Number.isFinite(hiDb) && hiDb > loDb)) return null
      return { kind: 'dbfs', topDb: hiDb, dbPerUnit: hiDb - loDb, graduated: true }
    }
    case 'relative':
      return null
    default: {
      // A kind this build does not know (a newer backend's) reads as relative. The `never` makes a new
      // kind in `SpectrumScale` a compile error here until the axis knows what to say about it.
      const unknown: never = scale
      void unknown
      return null
    }
  }
}

/** The axis a frame's values are on: from its `scale`, and for a relative scale its `source`. */
export function axisOf(scale: SpectrumScale | null | undefined, source: string): ScaleAxis {
  const absolute = scale ? absoluteAxis(scale) : null
  if (absolute) return absolute
  const r = RELATIVE.get(source) ?? UNPUBLISHED
  return { kind: 'relative', topDb: 0, dbPerUnit: r.dbPerUnit, graduated: r.graduated }
}

/** A value on `axis` in dB (dBFS, or dB below the radio display's top). */
export function valueToDb(axis: ScaleAxis, v: number): number {
  return axis.topDb + (v - 1) * axis.dbPerUnit
}

/** The value `db` dB is on `axis`: the inverse of `valueToDb`. */
export function dbToValue(axis: ScaleAxis, db: number): number {
  return 1 + (db - axis.topDb) / axis.dbPerUnit
}

/** The backend's frame as the renderer draws it, its `dbPerUnit` taken from its own `scale`. */
export function rendererFrame(f: SpectrumFrameWire): SpectrumFrame {
  return {
    seq: f.seq,
    tMs: f.tMs,
    loHz: f.loHz,
    hiHz: f.hiHz,
    bins: f.bins,
    dbPerUnit: safeDbPerUnit(axisOf(f.scale, f.source).dbPerUnit),
  }
}

/** The y (device px from the top of the trace band) a value is drawn at in a trace band `h` high,
 *  exactly as both renderers draw it: `range.ceil` at y = 1, `range.floor` at y = h. */
export function traceY(v: number, range: DisplayRange, h: number): number {
  return h - strengthIn(v, range.floor, range.ceil) * (h - 1)
}

/** The dB the axis reads at height `y` of a trace band `h` high: the inverse of `traceY` inside the
 *  range (a y above or below the band reads the range's own end). */
export function dbAtY(axis: ScaleAxis, range: DisplayRange, h: number, y: number): number {
  const t = h > 1 ? Math.min(1, Math.max(0, (h - y) / (h - 1))) : 0
  return valueToDb(axis, range.floor + t * (range.ceil - range.floor))
}

/** One graduation: its dB and where it sits in the trace band. */
export interface AxisTick {
  db: number
  y: number
}

/** dB steps an axis may be graduated in, finest first. */
const TICK_STEPS_DB = [1, 2, 5, 10, 20, 50] as const

/**
 * The graduations of a trace band `h` high drawn in `range`: every multiple of the finest step that
 * leaves at least `minGapPx` between neighbours, inside the range, top first. None on an axis whose dB
 * per unit is a stand-in (`graduated` false): a relative scale with no published figure gets its unit
 * and no numbers.
 */
export function axisTicks(axis: ScaleAxis, range: DisplayRange, h: number, minGapPx = 16): AxisTick[] {
  if (!axis.graduated || h <= 1 || !(range.ceil > range.floor)) return []
  const dbTop = valueToDb(axis, range.ceil)
  const dbBottom = valueToDb(axis, range.floor)
  const pxPerDb = (h - 1) / (dbTop - dbBottom)
  const step = TICK_STEPS_DB.find((s) => s * pxPerDb >= minGapPx) ?? TICK_STEPS_DB[TICK_STEPS_DB.length - 1]
  const ticks: AxisTick[] = []
  for (let db = Math.floor(dbTop / step) * step; db >= dbBottom; db -= step) {
    // `+ 0` turns a −0 into 0, so the top of a dBFS axis is labelled 0, not −0.
    ticks.push({ db: db + 0, y: traceY(dbToValue(axis, db), range, h) })
  }
  return ticks
}

/** The unit and qualifier tokens the axis is labelled with: measurements, not prose. */
const DBFS = 'dBFS'
const DB_REL = 'dB rel'
const REL = 'rel'
const REF = 'REF'
const DB = 'dB'

/** The axis's unit: `dBFS`, `dB rel` (dB below the radio display's top), or `rel` (no dB figure). */
export function axisUnit(axis: ScaleAxis): string {
  if (axis.kind === 'dbfs') return DBFS
  return axis.graduated ? DB_REL : REL
}

/** The radio's REF setting, labelled beside a relative axis when the host knows it (a CI-V REF the
 *  operator set from Nexus); null when unknown. `+0.0` is shown as such: a REF of zero is a setting. */
export function refLabel(refDb: number | null | undefined): string | null {
  if (refDb == null || !Number.isFinite(refDb)) return null
  const sign = refDb < 0 ? '−' : '+'
  return `${REF} ${sign}${Math.abs(refDb).toFixed(1)} ${DB}`
}
