import { useStationCapability, useStationControl } from '../stationAccess'
// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts): the prose is in
// the catalog under `cockpit.tuning.*`. Nothing here transmits — every control moves the RX
// dial or a clarifier — so nothing is deferred.
//
// The units rule lands on the STEP: every step in Hz, the RIT/XIT offsets, the dial and the
// VFO letters are invariant and stay in the code, as do the four button names below.
import { useContext, useRef, useState } from 'react'
import type { AppSnapshot } from '../types'
import { setFrequency, setRit, setXit, setVfo, swapVfo } from '../api'
import { bandLabelForMhz, sidebandForQsy } from '../band'
import { FrequencyReadout } from './FrequencyReadout'
import { useWheelTune } from '../useWheelTune'
import { stepFrom } from '../wheelTuningPolicy'
import { t } from '../i18n'
import { pushToast } from '../toast'
import { controlFailureMessage } from '../remote-web/control-failure'
import { OperationFailure } from '../remote-web/operation-client'
import { RemoteWheelTuningContext } from '../remote-web/wheel-tuning-context'
import { useRemoteStation } from '../remote-web/amplifier-observation'

/** The rig's own vocabulary on these buttons: the two clarifiers and the two VFOs. Named so
 *  the catalog guard reads them as the deliberate tokens they are. */
const RIT = 'RIT'
const XIT = 'XIT'
const VFO_A = 'A'
const VFO_B = 'B'
/** The swap arrows, the same mark the rigs put on the A⇄B key. A SYMBOL, not prose — its
 *  name is `cockpit.tuning.vfo.swap.aria`, like the clarifiers' − and + beside it. */
const VFO_SWAP = '⇄'

/** Tuning steps (Hz). The `×10` buttons jump ten of the selected step. The labels are
 *  measurements, so they stay written here. */
const STEPS = [
  { hz: 10, label: '10 Hz' },
  { hz: 100, label: '100 Hz' },
  { hz: 1000, label: '1 kHz' },
  { hz: 5000, label: '5 kHz' },
] as const

/**
 * Compact RX tuning strip for the Phone/CW cockpits — the missing "tune from here" control. Live
 * frequency read-out, VFO up/down step-tuning (selectable step), and direct MHz entry. All routes
 * through the existing `set_frequency` CAT path (which keeps the band-correct sideband), so tuning
 * within a band never changes mode.
 */
export function TuningStrip({
  snap,
  onSnap,
  step: stepProp,
  onStep,
  sensitivity,
  showReadout = true,
}: {
  snap: AppSnapshot
  onSnap?: (s: AppSnapshot) => void
  /** Controlled tuning step (Hz), shared with scope wheel-tuning; falls back to internal state. */
  step?: number
  onStep?: (hz: number) => void
  /** Wheel-tune sensitivity (from Settings), applied to the readout wheel. */
  sensitivity?: number
  /** Render the big frequency readout in the strip (default). Set false when a parent (the shared
   * CockpitHeader) already owns the readout and this strip only supplies nudges/step/VFO/RIT/XIT. */
  showReadout?: boolean
}) {
  const control = useStationControl(), frequency = useStationCapability('frequency')
  const remoteTuning = useContext(RemoteWheelTuningContext), observation = useRemoteStation(snap.activeRadioId)
  const frequencyAllowed = control || (frequency && snap.radio.source === 'native' && !snap.radio.txEnabled &&
    !snap.radio.txBusyReason && !snap.radio.transmitting && !snap.radio.rigKeyed && !snap.radio.tuning)
  // A browser holds VFO + XIT under splitTuning (they move the transmitter) and RIT under ritTuning.
  const splitTuning = useStationCapability('splitTuning'), ritTuning = useStationCapability('ritTuning')
  const vfoXitAllowed = control || splitTuning, ritAllowed = control || ritTuning
  const dial = snap.radio.dialMhz
  const catOk = snap.radio.catOk === true
  const [stepInternal, setStepInternal] = useState(100)
  const step = stepProp ?? stepInternal
  const setStep = onStep ?? setStepInternal
  const rit = snap.radio.ritHz ?? 0
  const xit = snap.radio.xitHz ?? 0
  const vfo = snap.radio.activeVfo || 'A'
  const apply = (p: Promise<AppSnapshot>) => void p.then((s) => s && onSnap?.(s)).catch((error) => {
    // A refused browser change is said, never swallowed: the value it shows stays the station's.
    if (!control) pushToast(controlFailureMessage(error), 'error')
  })
  const fmtOffset = (hz: number) => (hz > 0 ? `+${hz}` : `${hz}`)

  const tuneTo = async (mhz: number) => {
    // A dial has to BE one. The band lookup used to double as this check (it answers '' for a
    // NaN or an absurd value), so it stays here on its own now that band membership no longer
    // refuses anything.
    if (!frequencyAllowed || !Number.isFinite(mhz) || mhz <= 0) return
    // An EMPTY band label is not a refusal: listening off the ham bands is first-class (operator,
    // 2026-08-13) — WWV, a shortwave broadcaster, the gap between two allocations. This used to
    // toast "outside the band plan" and return, which is what made every ◄/► nudge and every
    // typed entry stop dead at a band edge.
    // In-band keeps the current sideband; crossing 10 MHz follows the band convention (#45).
    const s = await setFrequency(mhz, bandLabelForMhz(mhz), sidebandForQsy(mhz, snap.radio.dialMhz, snap.radio.sideband)).catch(() => null)
    if (s) onSnap?.(s)
  }
  // #273: `steps` whole steps, the first rounding to the step (18.110.250 → 18.111.000 at 1 kHz),
  // in integer Hz so float drift never accumulates on repeated nudges.
  //
  // A BROWSER'S ARROWS STEP THE WHEEL'S OWN BURST, and the desktop's command the dial directly as
  // they always have. `dial` is the sample this strip draws, a poll behind the stream and further
  // behind a wheel command already in flight; an absolute dial built from it walked back what the
  // wheel had just asked for, or was refused as a second command while the digits showed neither.
  // `nudgeSteps` keeps the rounding above and hands the press to the one pipeline (wheel-tuning.ts).
  const nudge = (steps: number) => {
    if (!control && remoteTuning) {
      // A PRESS IS ONE GESTURE, SO IT SAYS SOMETHING. The burst refuses a wheel spin quietly on
      // purpose — ten notches must not raise ten toasts — but an arrow that does nothing and says
      // nothing reads as a broken button, and the state that gets here is a real one (the
      // observation this browser is looking at is not current, so the pipeline will not build a
      // command on it). The button stays offered; the press carries the refusal, in the same words
      // as every other gesture refused before anything left the browser.
      if (!remoteTuning.nudgeSteps(steps, step, { dialMhz: dial, sideband: snap.radio.sideband || 'USB', context: observation.context }))
        pushToast(controlFailureMessage(new OperationFailure('notController', false)), 'error')
      return
    }
    void tuneTo(stepFrom(Math.round(dial * 1e6), steps, step) / 1e6)
  }

  // Mouse-wheel tuning over the big frequency read-out itself (operator request) — same coalesced
  // CAT path + selected step (Shift = ×10) as the scope wheel-tune, for hunting CW/phone signals
  // that have no agreed default frequency. Disabled while transmitting or CAT-down.
  const readoutRef = useRef<HTMLSpanElement>(null)
  useWheelTune(readoutRef, {
    remoteFrequency: true,
    dialMhz: dial,
    sideband: snap.radio.sideband || 'USB',
    enabled: catOk && !snap.radio.txBusyReason && !snap.radio.transmitting,
    stepHz: step,
    sensitivity,
    onSnap,
  })

  return (
    <div className="tuning-strip" role="group" aria-label={t('cockpit.tuning.aria')}>
      <button
        type="button"
        className="tuning-nudge"
        disabled={!frequencyAllowed || !catOk}
        onClick={() => nudge(-10)}
        title={t('cockpit.tuning.down.title', { hz: step * 10 })}
        aria-label={t('cockpit.tuning.down.aria', { hz: step * 10 })}
      >
        ◄◄
      </button>
      <button
        type="button"
        className="tuning-nudge"
        disabled={!frequencyAllowed || !catOk}
        onClick={() => nudge(-1)}
        title={t('cockpit.tuning.down.title', { hz: step })}
        aria-label={t('cockpit.tuning.down.aria', { hz: step })}
      >
        ◄
      </button>
      {showReadout && (
        <span
          ref={readoutRef}
          className="tuning-readout-wheel"
          title={catOk ? t('cockpit.tuning.wheel.title', { hz: step }) : undefined}
        >
          <FrequencyReadout
            dialMhz={dial}
            size="hero"
            editable
            remoteFrequency
            disabled={!frequencyAllowed || !catOk}
            onCommit={(mhz) => void tuneTo(mhz)}
            // The ENGINE's privilege answer, not the UI's band table. Off-band RX is legal
            // listening, not a transmit block, and `!bandLabelForMhz(dial)` painted every
            // out-of-band dial TX-red on the strength of a band-plan miss (operator, 2026-08-13).
            txBlocked={!snap.radio.txAllowed}
          />
        </span>
      )}
      <button
        type="button"
        className="tuning-nudge"
        disabled={!frequencyAllowed || !catOk}
        onClick={() => nudge(1)}
        title={t('cockpit.tuning.up.title', { hz: step })}
        aria-label={t('cockpit.tuning.up.aria', { hz: step })}
      >
        ►
      </button>
      <button
        type="button"
        className="tuning-nudge"
        disabled={!frequencyAllowed || !catOk}
        onClick={() => nudge(10)}
        title={t('cockpit.tuning.up.title', { hz: step * 10 })}
        aria-label={t('cockpit.tuning.up.aria', { hz: step * 10 })}
      >
        ►►
      </button>
      <select
        className="tuning-step"
        value={step}
        onChange={(e) => setStep(Number(e.target.value))}
        title={t('cockpit.tuning.step.label')}
        aria-label={t('cockpit.tuning.step.label')}
      >
        {STEPS.map((s) => (
          <option key={s.hz} value={s.hz}>
            {s.label}
          </option>
        ))}
      </select>
      <span className="tuning-vfo" role="group" aria-label={t('cockpit.tuning.vfo.aria')}>
        <button
          type="button"
          className={vfo === VFO_A ? 'active' : ''}
          disabled={!vfoXitAllowed || !catOk}
          onClick={() => apply(setVfo(VFO_A))}
          title={t('cockpit.tuning.vfo.title', { vfo: VFO_A })}
        >
          {VFO_A}
        </button>
        <button
          type="button"
          className={vfo === VFO_B ? 'active' : ''}
          disabled={!vfoXitAllowed || !catOk}
          onClick={() => apply(setVfo(VFO_B))}
          title={t('cockpit.tuning.vfo.title', { vfo: VFO_B })}
        >
          {VFO_B}
        </button>
        {/* SWAP ONLY, and A=B is deliberately not here (operator, 2026-09-22): a copy
            OVERWRITES the other dial, needs a rig verb this app does not have, and wants a
            bench pass before it is offered. A swap is reversible by pressing it again.

            It carries the SAME gate as the two buttons it sits beside — `vfoXitAllowed`
            because it moves the transmitter's VFO as well as the receiver's, and `catOk`
            because with no CAT link there is nothing to swap. A control offered over a dead
            link would be a button that lies. */}
        <button
          type="button"
          disabled={!vfoXitAllowed || !catOk}
          onClick={() => apply(swapVfo())}
          title={t('cockpit.tuning.vfo.swap.title')}
          aria-label={t('cockpit.tuning.vfo.swap.aria')}
        >
          {VFO_SWAP}
        </button>
      </span>
      <span className={`tuning-clar${rit !== 0 ? ' on' : ''}`}>
        <button type="button" disabled={!ritAllowed || !catOk} onClick={() => apply(setRit(0))} title={t('cockpit.tuning.rit.title')}>
          {RIT}
        </button>
        <button type="button" disabled={!ritAllowed || !catOk} onClick={() => apply(setRit(rit - 10))} aria-label={t('cockpit.tuning.rit.down.aria')}>
          −
        </button>
        <span className="tuning-clar-val mono">{fmtOffset(rit)}</span>
        <button type="button" disabled={!ritAllowed || !catOk} onClick={() => apply(setRit(rit + 10))} aria-label={t('cockpit.tuning.rit.up.aria')}>
          +
        </button>
      </span>
      <span className={`tuning-clar${xit !== 0 ? ' on' : ''}`}>
        <button type="button" disabled={!vfoXitAllowed || !catOk} onClick={() => apply(setXit(0))} title={t('cockpit.tuning.xit.title')}>
          {XIT}
        </button>
        <button type="button" disabled={!vfoXitAllowed || !catOk} onClick={() => apply(setXit(xit - 10))} aria-label={t('cockpit.tuning.xit.down.aria')}>
          −
        </button>
        <span className="tuning-clar-val mono">{fmtOffset(xit)}</span>
        <button type="button" disabled={!vfoXitAllowed || !catOk} onClick={() => apply(setXit(xit + 10))} aria-label={t('cockpit.tuning.xit.up.aria')}>
          +
        </button>
      </span>
    </div>
  )
}
