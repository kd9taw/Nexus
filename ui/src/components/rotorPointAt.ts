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
// ⚠️ THE TOAST ALSO SAYS WHAT THE BEARING WAS TAKEN TO. Until 2026-09-29 every point-at-call aimed
// at the centre of the station's country, 20° wide from the Netherlands to Galicia, and a tester
// lining the beam up for a pile-up had no way to tell. It now aims at the station's own grid or
// callbook position when Nexus knows one, and the toast says which it got (`pointedTo`).
import { pointRotatorAtCall } from '../api'
import { controlFailureMessage } from '../remote-web/control-failure'
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

/** The strip's `onPointAt` for a cockpit. `control` is the cockpit's `useStationControl()`: a
 *  browser gets no bearing back and reports a refusal in the Remote's own words. */
export function rotorPointAt(control: boolean): (call: string, longPath?: boolean) => void {
  return (call, longPath = false) => {
    pointRotatorAtCall(call, longPath)
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
          control ? t('cw.rotator.failed', { error: e instanceof Error ? e.message : String(e) }) : controlFailureMessage(e),
          'error',
        ),
      )
  }
}
