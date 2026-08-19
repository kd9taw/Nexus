// AOS/LOS alerts for the ARMED pass track ("Work this pass"). The operator
// arms a pass from the Satellites section and then works around the shack —
// hands on the rotor, eyes elsewhere — so the moment the bird rises (and sets)
// must be HEARD (distinct rising/falling earcons, alerts.ts) and seen
// (a prominent toast), wherever in the app they are. Ticked by an app-wide 2 s
// poll of get_sat_track_status (App.tsx), with module state that edge-detects
// the DTO's state TRANSITIONS — never wall-clock timers, which drift from the
// track's truth when the app is suspended: a transition observed late fires
// once, honestly ("pass in progress"), and one missed entirely (LOS seen past
// the 5 min window) fires nothing.
//
// NOTIFY, NEVER ACT: this module calls nothing but pushToast and the earcons.
// The track loop backend-side already did whatever was consented (Doppler,
// rotor, the LOS handback) — the alerts report what happened.
//
// This is a SEPARATE channel from the ⏰ per-pass alarm (satAlarm.ts): that one
// is "wake me before any favourite rises", this one is scoped to the one track
// "Work this pass" created. The AOS alert marks the pass fired in the ⏰
// system (markSatPassFired) so the two can never both fire AT AOS — a ⏰ lead
// alarm that fired minutes earlier is a different moment and stays untouched.
//
// The popup is a PROMINENT TOAST, deliberately not a Dialog: the toast stack
// is a live region that never steals keyboard focus and never blocks the UI —
// the operator needs the sky dome the moment the alert lands — while a
// dialog's modality is exactly what an alert must not have. 30 s outlives a
// glance away without becoming a banner the operator has to come dismiss.

//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every
// alert is a whole sentence in the catalog; the bird's name, the bearings, the
// elevation and the minutes are the facts being reported. The LOS report is a
// head plus up to two further COMPLETE sentences about what came back, joined
// with a space — each stands on its own, so a translator never has to reorder
// half a sentence around a conditional.

import { aosEarcon, losEarcon } from '../alerts'
import { pushToast } from '../toast'
import { markSatPassFired } from './satAlarm'
import { t } from '../i18n'
import type { SatTrackStatus } from '../types'

/** The Settings facts a tick needs at fire time — read live by the caller
 * (App.tsx keeps a settings mirror); the module holds no settings state. */
export interface SatPassAlertOpts {
  /** Settings.satPassAlertSoundOff — absent/false = tones ON (the operator
   * asked for audible). Silences only the earcons, never the popups. */
  soundOff?: boolean
  /** Settings.rotPostPass ("stop" | "park" | "ready") for the LOS handback
   * line — at LOS the mast may be about to move on its own. */
  rotPostPass?: string
}

/** A LIVE track that vanishes more than this far past its LOS is a wake from
 * suspension, not an event — the moment has passed; say nothing stale. The
 * same window the section's LOS-handback toast used before it moved here. */
const LOS_WINDOW_SECS = 300

/** Past this long after AOS, an observed rise is reported as "in progress" —
 * the transition happened while nobody watched (suspension, app start), and
 * "AOS now" would be a stale claim about a moment that already went by. */
const AOS_LATE_SECS = 60

// Module state: the previous poll's answer (the edge detector) and the
// per-pass fired keys — belt and braces so a straggler answer re-seeding a
// dead pass as live, or a disarm/re-arm flap, can never fire the same moment
// twice. In-session only: this channel's job is "the moment it happened",
// which does not survive a restart (the mid-pass-wake alert covers that).
let prev: SatTrackStatus | null = null
const fired = new Set<string>()

/** Test hook — forget the previous track and the per-pass fired keys. */
export function resetSatPassAlerts(): void {
  prev = null
  fired.clear()
}

/** One pass, one moment: `aos:` / `los:` / `rotor:` per (bird, exact frozen
 * AOS). */
function passMoment(track: SatTrackStatus, phase: 'aos' | 'los' | 'rotor'): string {
  return `${phase}:${track.name}|${track.aosUnix}`
}

/**
 * The rotator quit mid-pass. The track deliberately does NOT end — it drops to
 * driving the radio alone and runs to a real LOS — so without a word the
 * operator's only clue is the badge quietly losing its commanded angles, and
 * an antenna that has silently stopped following the bird is the difference
 * between a worked pass and a lost one.
 *
 * Reports, acts on nothing: the demotion already happened backend-side. Once
 * per pass, and only for a rotator that WAS being driven — a track armed
 * without one never had a mast to lose.
 */
function fireRotorLost(track: SatTrackStatus): void {
  const key = passMoment(track, 'rotor')
  if (fired.has(key)) return
  fired.add(key)
  pushToast(t('sat.passAlert.rotorLost', { name: track.name }), 'error', 30_000, {
    prominent: true,
  })
}

function fireAos(track: SatTrackStatus, nowSecs: number, opts: SatPassAlertOpts): void {
  const key = passMoment(track, 'aos')
  if (fired.has(key)) return
  fired.add(key)
  // Cross-channel dedupe: ⏰'s late-wake path must never add a second popup
  // at/after AOS for the pass the operator is already being told about.
  markSatPassFired(track.name, track.aosUnix)
  if (!opts.soundOff) aosEarcon()
  const late = nowSecs - track.aosUnix > AOS_LATE_SECS
  const msg = late
    ? // Observed late: the rise already happened — report the truth of NOW.
      t('sat.passAlert.inProgress', {
        name: track.name,
        mins: Math.max(1, Math.round((track.losUnix - nowSecs) / 60)),
        maxEl: Math.round(track.maxElDeg),
      })
    : // The facts the operator needs with hands on the rotor: where the bird
      // rises, how high it gets, how long they have.
      t('sat.passAlert.aos', {
        name: track.name,
        az: Math.round(track.aosAzDeg),
        maxEl: Math.round(track.maxElDeg),
        mins: Math.max(1, Math.round((track.losUnix - track.aosUnix) / 60)),
      })
  pushToast(msg, 'success', 30_000, { prominent: true })
}

function fireLos(ended: SatTrackStatus, opts: SatPassAlertOpts): void {
  const key = passMoment(ended, 'los')
  if (fired.has(key)) return
  fired.add(key)
  if (!opts.soundOff) losEarcon()
  const rotorHalf = ended.mode === 'rotor+doppler' || ended.mode === 'rotor-only'
  const rotPostPass = opts.rotPostPass ?? 'stop'
  const parts = [t('sat.passAlert.los', { name: ended.name })]
  // What came back keys on the LEGS that were driven: the downlink leg held the
  // dial; an uplink-only track only ever held the TX split.
  if (ended.dopplerDownlink) parts.push(t('sat.passAlert.los.dial'))
  else if (ended.dopplerUplink) parts.push(t('sat.passAlert.los.split'))
  // Only when the mast is about to move ON ITS OWN — the "stop" policy leaves
  // it where the pass finished, and a stationary rotor needs no announcement.
  if (rotorHalf && rotPostPass === 'park') parts.push(t('sat.passAlert.los.rotorPark'))
  if (rotorHalf && rotPostPass === 'ready') parts.push(t('sat.passAlert.los.rotorReady'))
  pushToast(parts.join(' '), 'info', 12_000, { prominent: true })
}

/**
 * The edge-detecting tick — call with each get_sat_track_status answer.
 * Fires exactly one alert+tone at AOS (or one "pass in progress" on a
 * mid-pass wake), one if the rotator quits mid-pass, and one at LOS carrying
 * the handback facts. Everything else — armed waits, manual stops, stale
 * wakes, repeated polls — is silence.
 */
export function tickSatPassAlert(
  track: SatTrackStatus | null,
  nowSecs: number,
  opts: SatPassAlertOpts = {},
): void {
  const before = prev
  prev = track

  if (track != null) {
    // A replaced track (new bird / new pass — arming stops the old loop) is a
    // fresh edge, not a continuation: judge the transition per pass.
    const samePass =
      before != null && before.name === track.name && before.aosUnix === track.aosUnix
    if (track.state === 'tracking' && (!samePass || before.state !== 'tracking')) {
      // AOS: the observed edge into "tracking" — a first observation that IS
      // tracking (mid-pass wake / restart) included; the per-pass key makes
      // repeated polls and re-arms of the same pass silent.
      fireAos(track, nowSecs, opts)
    }
    // A rotator that quit is a SECOND moment inside the same live track, so it
    // is its own key rather than a state transition: the track's `state` keeps
    // running through it, which is the point.
    if (track.rotorLost === true) fireRotorLost(track)
    return
  }

  // LOS HANDBACK (the release happened backend-side — this reports what was
  // DONE, it acts on nothing): a live track that vanishes just past its LOS
  // ended by running out of pass, not by a click. A manual stop vanishes with
  // `nowSecs` still short of losUnix, and a wake from suspension past the
  // window has missed the moment — both stay silent.
  if (before != null && nowSecs >= before.losUnix && nowSecs - before.losUnix < LOS_WINDOW_SECS) {
    fireLos(before, opts)
  }
}
