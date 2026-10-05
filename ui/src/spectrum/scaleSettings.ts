// The scope's display settings, one record per cockpit, persisted and CLAMPED ON LOAD: the averaging
// time constant, the detector, the operator's G and Z, and the analysis window (the FFT size).
//
// PER COCKPIT, shared by every window that shows that cockpit. Phone and CW keep their own (a CW
// operator reading keying and a phone operator watching syllables want different averaging), and so do
// the digital cockpits, which today all move one app-wide G/Z between them. Not per window: G and Z are
// a contrast calibration against the station's own noise floor, and re-calibrating per window is the
// surprise (Waterfall.tsx's note on its G/Z keys), so a torn-off FT waterfall shares the docked one's.
//
// WEBVIEW-LOCAL, like every other scope and waterfall display setting (the palettes, zoom, 3D, flow):
// not a settings.json key. How Nexus looks stays per computer (operator, 2026-09-26), and the hosted
// Remote page's settings parser requires every key it knows.
//
// CLAMPED ON LOAD, field by field. A value that is out of range is pulled into it; one that is not a
// value at all (NaN, a string, a foreign shape, unparseable JSON) reads as if it had never been stored.
// The averaging time is taken to the nearest step the control offers, as the waterfall's zoom is
// (Waterfall.tsx `loadZoom`): a stored value the control cannot show is one the operator cannot see.
//
// WHAT WAS STORED BEFORE THIS RECORD EXISTED is read until the record is first written, so the first
// load shows what the operator already chose: the rig scope's window (`nexus.phonescope.win`) for
// Phone and CW, and the digital waterfall's app-wide G and Z (`nexus.waterfall.gain`/`.zero`) for every
// cockpit that draws it. The rig scope's G and Z were never persisted.
//
// A field never changes meaning; a new meaning takes a new name. A record written by a newer build keeps
// the fields this one does not know: a save merges onto what is stored instead of replacing it.

import { useCallback, useRef, useState } from 'react'
import type { ScopeWindow } from '../api'
import { surfaceGet } from '../features/windowScope'
import { AVERAGE_MAX_MS, AVERAGE_STEPS_MS } from './scaleAverage'
import type { Detector } from './types'

/** The surfaces a scale record belongs to: the rig scope's two cockpits, the digital waterfall's, and
 *  the RF scope pane (`rfpan`) — one record for the pane in all five digital cockpits, because it is
 *  one picture wherever it shows: the radio's own panadapter, never the audio FFT the waterfalls draw. */
export type ScopeCockpit = 'phone' | 'cw' | 'operate' | 'js8' | 'rtty' | 'psk' | 'sstv' | 'tempo' | 'rfpan'

export interface ScaleSettings {
  /** Log-recursive averaging time constant, ms (one of AVERAGE_STEPS_MS); 0 = off. */
  averageMs: number
  /** How a pixel covering several bins reduces them. */
  detector: Detector
  /** The operator's G and Z (`applyGainZero`), each −1..1; 0 = the automatic range as it is. */
  gain: number
  zero: number
  /** The analysis window (FFT size): Fast 1024, Balanced 2048, Sharp 4096. */
  window: ScopeWindow
}

/** The storage key of each cockpit's record. Shared by every window (see the header). */
export const SCALE_KEYS: Readonly<Record<ScopeCockpit, string>> = {
  phone: 'nexus.scope.phone',
  cw: 'nexus.scope.cw',
  operate: 'nexus.scope.operate',
  js8: 'nexus.scope.js8',
  rtty: 'nexus.scope.rtty',
  psk: 'nexus.scope.psk',
  sstv: 'nexus.scope.sstv',
  tempo: 'nexus.scope.tempo',
  rfpan: 'nexus.scope.rfpan',
}

/** What was stored before the records: the rig scope's window (per window), the waterfall's G and Z. */
const PHSCOPE_WIN_KEY = 'nexus.phonescope.win'
const WF_GAIN_KEY = 'nexus.waterfall.gain'
const WF_ZERO_KEY = 'nexus.waterfall.zero'

const WINDOWS: readonly ScopeWindow[] = ['fast', 'balanced', 'sharp']
const DETECTORS: readonly Detector[] = ['peak', 'average']

/** The averaging default, deskHPSDR's 250 ms, on every cockpit but Phone and CW. */
export const AVERAGE_DEFAULT_MS = 250
/** Phone's averaging default: 250 ms. Set 2026-10-04 ahead of the operator's ruling on it. */
export const PHONE_AVERAGE_DEFAULT_MS = 250
/**
 * CW's averaging default: off. Set 2026-10-04 ahead of the operator's ruling on it. CW's trace
 * already has its own fast hold (TRACE_HOLD_MS.fast, 120 ms), and a 250 ms log-recursive average on
 * top flattens 25 WPM keying: of a 40 dB keyed carrier, 6.6 dB of swing is left at the producer's
 * 20 ms frames (scaleAverage.test.ts), the near-static bar the fast hold was chosen to remove.
 */
export const CW_AVERAGE_DEFAULT_MS = 0

/**
 * A cockpit's settings before the operator touches anything. The averaging default is each
 * cockpit's named constant above.
 *
 * The detector defaults to PEAK, which is what every scope and waterfall draws today (`resampleRow`);
 * deskHPSDR's default is AVERAGE.
 */
export function scaleDefaults(cockpit: ScopeCockpit): ScaleSettings {
  return {
    averageMs: cockpit === 'cw' ? CW_AVERAGE_DEFAULT_MS : cockpit === 'phone' ? PHONE_AVERAGE_DEFAULT_MS : AVERAGE_DEFAULT_MS,
    detector: 'peak',
    gain: 0,
    zero: 0,
    window: 'balanced',
  }
}

/** The nearest averaging step to a time, after clamping it to 0..AVERAGE_MAX_MS. */
function averageStep(ms: number): number {
  const c = Math.min(AVERAGE_MAX_MS, Math.max(0, ms))
  let best: number = AVERAGE_STEPS_MS[0]
  for (const s of AVERAGE_STEPS_MS) if (Math.abs(s - c) < Math.abs(best - c)) best = s
  return best
}

/** A G or Z knob: −1..1, and never −0 (JSON writes it as 0, so a −0 would not survive a round trip). */
function knob(v: unknown): number | undefined {
  return typeof v === 'number' && Number.isFinite(v) ? Math.min(1, Math.max(-1, v)) + 0 : undefined
}

/**
 * `raw` read as settings, field by field: a field that is a value is clamped into range, one that is
 * missing or is not a value takes `base`'s. `raw` may be anything (whatever was stored, parsed).
 */
export function clampScaleSettings(raw: unknown, base: ScaleSettings): ScaleSettings {
  const r = raw !== null && typeof raw === 'object' && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {}
  const avg = r.averageMs
  return {
    averageMs: typeof avg === 'number' && Number.isFinite(avg) ? averageStep(avg) : base.averageMs,
    detector: DETECTORS.includes(r.detector as Detector) ? (r.detector as Detector) : base.detector,
    gain: knob(r.gain) ?? base.gain,
    zero: knob(r.zero) ?? base.zero,
    window: WINDOWS.includes(r.window as ScopeWindow) ? (r.window as ScopeWindow) : base.window,
  }
}

/** A stored string parsed as JSON, or undefined (absent, blocked, or not JSON). */
function storedObject(key: string): unknown {
  try {
    const s = window.localStorage.getItem(key)
    return s == null ? undefined : JSON.parse(s)
  } catch {
    return undefined
  }
}

/** A stored G or Z knob, read as the digital waterfall reads it (`parseFloat`), or undefined. */
function storedKnob(key: string): number | undefined {
  try {
    return knob(parseFloat(window.localStorage.getItem(key) ?? ''))
  } catch {
    return undefined
  }
}

/** The defaults with what this cockpit's controls stored before its record existed. */
function legacyBase(cockpit: ScopeCockpit): ScaleSettings {
  const base = scaleDefaults(cockpit)
  if (cockpit === 'phone' || cockpit === 'cw') {
    const w = surfaceGet(PHSCOPE_WIN_KEY)
    if (WINDOWS.includes(w as ScopeWindow)) base.window = w as ScopeWindow
    // Not the RF scope pane's: it is new, and its picture is the radio's panadapter, so the audio
    // waterfall's old G and Z — a calibration against another axis — are no starting point for it.
  } else if (cockpit !== 'rfpan') {
    base.gain = storedKnob(WF_GAIN_KEY) ?? base.gain
    base.zero = storedKnob(WF_ZERO_KEY) ?? base.zero
  }
  return base
}

/** A cockpit's settings as stored, clamped. Storage-safe: blocked storage reads as nothing stored. */
export function loadScaleSettings(cockpit: ScopeCockpit): ScaleSettings {
  return clampScaleSettings(storedObject(SCALE_KEYS[cockpit]), legacyBase(cockpit))
}

/** Store a cockpit's settings, keeping any field a newer build wrote that this one does not know. */
export function saveScaleSettings(cockpit: ScopeCockpit, s: ScaleSettings): void {
  const key = SCALE_KEYS[cockpit]
  const stored = storedObject(key)
  const kept = stored !== null && typeof stored === 'object' && !Array.isArray(stored) ? stored : {}
  try {
    window.localStorage.setItem(key, JSON.stringify({ ...kept, ...s }))
  } catch {
    /* full or unavailable: the value still applies for this session */
  }
}

/**
 * A cockpit's settings for a host to draw with, and the one way to change them: `update` clamps the
 * change, stores it and re-renders. A host binds its strip (ScaleStrip.tsx) and its G/Z sliders here.
 */
export function useScaleSettings(cockpit: ScopeCockpit): [ScaleSettings, (patch: Partial<ScaleSettings>) => void] {
  const [state, setState] = useState(() => ({ cockpit, settings: loadScaleSettings(cockpit) }))
  let shown = state
  if (state.cockpit !== cockpit) {
    // A host that changes cockpit shows the new one's record, and never writes the old one's there.
    shown = { cockpit, settings: loadScaleSettings(cockpit) }
    setState(shown)
  }
  const latest = useRef(shown)
  latest.current = shown
  const update = useCallback((patch: Partial<ScaleSettings>) => {
    const { cockpit: c, settings: s } = latest.current
    const next = clampScaleSettings({ ...s, ...patch }, s)
    saveScaleSettings(c, next)
    latest.current = { cockpit: c, settings: next }
    setState(latest.current)
  }, [])
  return [shown.settings, update]
}
