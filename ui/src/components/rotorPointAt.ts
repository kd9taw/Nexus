// RotorStrip's `onPointAt` as a cockpit wires it: slew the antenna toward the station on the path
// the operator picked, then say where it went. One handler for every cockpit that carries the
// strip, so no host writes its own.
//
// ⚠️ THE PATH IS FORWARDED, NEVER DROPPED. The strip calls `onPointAt(call, false)` from → CALL
// and `onPointAt(call, true)` from LP (#338). Every cockpit used to wire
// `(call) => pointRotatorAtCall(call)`, and TypeScript accepts a one-argument handler for the
// two-argument prop. So in Phone, CW and FT, LP turned the beam the SHORT way while its tooltip
// promised the reciprocal. The LP toast names the long path, for the reason `grid.ts::azimuthTitle`
// gives: a heading with no path is half an answer.
//
// Every key below is written out literally, with its params as an object literal, so the catalog
// scanners can read each call site.
//
// ⚠️ A POINT ENDS THE TRACKER THAT IS STEERING THE MAST, and only that one
// (`stopSteeringTracker`): a Moon follow, or a satellite pass prepositioning or tracking, is
// stopped first (`stopSatTrack`, as the Rotor pane's manual moves and ■ STOP do), and only then is
// the point sent, so the stop's halt can never land after it. A Moon follow left running kept its
// badge and swung the antenna back to the Moon minutes later. An ARMED pass, still waiting for AOS,
// is not steering: it promises the rotor is free for HF until 5 min before AOS, so it stays armed,
// staged transponder and all. `point_rotator_at_call` applies the same rule on the station's side
// for every caller, including the FT cockpit's, App's and the pop-out's own point handlers, which
// do not come through here (they get the station's disarm, without the halt first).
//
// ⚠️ THE TOAST ALSO SAYS WHAT THE BEARING WAS TAKEN TO. Until 2026-09-29 every point-at-call aimed
// at the centre of the station's country, 20° wide from the Netherlands to Galicia, and a tester
// lining the beam up for a pile-up had no way to tell. It now aims at the station's own grid or
// callbook position when Nexus knows one, and the toast says which it got (`pointedTo`).
import { getMoonStatus, getSatTrackStatus, pointRotatorAtCall, stopSatTrack } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'
import type { PointedAt } from '../types'

/** What a point-at-call's bearing was taken to, as the closing clause of its toast: the station's
 *  own grid, the position its callbook gives, or only the centre of its country. */
export function pointedTo(pointed: PointedAt): string {
  if (pointed.to === 'grid') return t('rotor.pointed.to.grid', { grid: pointed.grid ?? '' })
  if (pointed.to === 'position') return t('rotor.pointed.to.position')
  return t('rotor.pointed.to.country', { country: pointed.country ?? '' })
}

/** Stop the tracker steering the mast, if one is: a Moon follow, or a pass prepositioning or
 *  tracking. An armed pass is left armed — unless it lost its rotor, which a pass can only do by
 *  steering it: one that lost it while prepositioning reads "armed" until AOS, and takes the mast
 *  back off the station within 15 s once the rotator answers. A read that fails cannot tell, so
 *  the stop is made, as it always was before a tracker's state was asked (the Needed board's Go
 *  has no station-side check behind it). */
export async function stopSteeringTracker(): Promise<void> {
  let unknown = false
  const [sat, moon] = await Promise.all([
    getSatTrackStatus().catch(() => {
      unknown = true
      return null
    }),
    getMoonStatus(null).catch(() => {
      unknown = true
      return null
    }),
  ])
  const steering =
    unknown || (sat != null && (sat.state !== 'armed' || sat.rotorLost === true)) || moon?.track != null
  if (steering) await stopSatTrack()
}

/** The strip's `onPointAt` for a cockpit. */
export function rotorPointAt(): (call: string, longPath?: boolean) => void {
  return (call, longPath = false) => {
    stopSteeringTracker()
      .then(() => pointRotatorAtCall(call, longPath))
      .then((pointed: PointedAt | null | undefined) =>
        // A browser gets nothing back: the station resolves it.
        pushToast(
          pointed == null
            ? t('remote.b1.rotatorPointing', { call })
            : longPath
              ? t('shell.rotator.pointedLong', {
                  bearing: Math.round(pointed.bearing),
                  call,
                  to: pointedTo(pointed),
                })
              : t('cw.rotator.pointed', {
                  call,
                  bearing: Math.round(pointed.bearing),
                  to: pointedTo(pointed),
                }),
          'info',
        ),
      )
      .catch((e) =>
        pushToast(
          t('cw.rotator.failed', { error: e instanceof Error ? e.message : String(e) }),
          'error',
        ),
      )
  }
}
