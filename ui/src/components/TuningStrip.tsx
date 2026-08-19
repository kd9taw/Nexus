// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts): the prose is in
// the catalog under `cockpit.tuning.*`. Nothing here transmits — every control moves the RX
// dial or a clarifier — so nothing is deferred.
//
// The units rule lands on the STEP: every step in Hz, the RIT/XIT offsets, the dial and the
// VFO letters are invariant and stay in the code, as do the four button names below.
import { useRef, useState } from 'react'
import type { AppSnapshot } from '../types'
import { setFrequency, setRit, setXit, setVfo } from '../api'
import { bandLabelForMhz, sidebandForQsy } from '../band'
import { FrequencyReadout } from './FrequencyReadout'
import { useWheelTune } from '../useWheelTune'
import { t } from '../i18n'

/** The rig's own vocabulary on these buttons: the two clarifiers and the two VFOs. Named so
 *  the catalog guard reads them as the deliberate tokens they are. */
const RIT = 'RIT'
const XIT = 'XIT'
const VFO_A = 'A'
const VFO_B = 'B'

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
  const dial = snap.radio.dialMhz
  const catOk = snap.radio.catOk === true
  const [stepInternal, setStepInternal] = useState(100)
  const step = stepProp ?? stepInternal
  const setStep = onStep ?? setStepInternal
  const rit = snap.radio.ritHz ?? 0
  const xit = snap.radio.xitHz ?? 0
  const vfo = snap.radio.activeVfo || 'A'
  const apply = (p: Promise<AppSnapshot>) => void p.then((s) => s && onSnap?.(s)).catch(() => {})
  const fmtOffset = (hz: number) => (hz > 0 ? `+${hz}` : `${hz}`)

  const tuneTo = async (mhz: number) => {
    // A dial has to BE one. The band lookup used to double as this check (it answers '' for a
    // NaN or an absurd value), so it stays here on its own now that band membership no longer
    // refuses anything.
    if (!Number.isFinite(mhz) || mhz <= 0) return
    // An EMPTY band label is not a refusal: listening off the ham bands is first-class (operator,
    // 2026-08-13) — WWV, a shortwave broadcaster, the gap between two allocations. This used to
    // toast "outside the band plan" and return, which is what made every ◄/► nudge and every
    // typed entry stop dead at a band edge.
    // In-band keeps the current sideband; crossing 10 MHz follows the band convention (#45).
    const s = await setFrequency(mhz, bandLabelForMhz(mhz), sidebandForQsy(mhz, snap.radio.dialMhz, snap.radio.sideband)).catch(() => null)
    if (s) onSnap?.(s)
  }
  // Round to the nearest Hz to avoid float drift accumulating on repeated nudges.
  const nudge = (deltaHz: number) => void tuneTo(Math.round((dial + deltaHz / 1e6) * 1e6) / 1e6)

  // Mouse-wheel tuning over the big frequency read-out itself (operator request) — same coalesced
  // CAT path + selected step (Shift = ×10) as the scope wheel-tune, for hunting CW/phone signals
  // that have no agreed default frequency. Disabled while transmitting or CAT-down.
  const readoutRef = useRef<HTMLSpanElement>(null)
  useWheelTune(readoutRef, {
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
        disabled={!catOk}
        onClick={() => nudge(-step * 10)}
        title={t('cockpit.tuning.down.title', { hz: step * 10 })}
        aria-label={t('cockpit.tuning.down.aria', { hz: step * 10 })}
      >
        ◄◄
      </button>
      <button
        type="button"
        className="tuning-nudge"
        disabled={!catOk}
        onClick={() => nudge(-step)}
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
            disabled={!catOk}
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
        disabled={!catOk}
        onClick={() => nudge(step)}
        title={t('cockpit.tuning.up.title', { hz: step })}
        aria-label={t('cockpit.tuning.up.aria', { hz: step })}
      >
        ►
      </button>
      <button
        type="button"
        className="tuning-nudge"
        disabled={!catOk}
        onClick={() => nudge(step * 10)}
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
          disabled={!catOk}
          onClick={() => apply(setVfo(VFO_A))}
          title={t('cockpit.tuning.vfo.title', { vfo: VFO_A })}
        >
          {VFO_A}
        </button>
        <button
          type="button"
          className={vfo === VFO_B ? 'active' : ''}
          disabled={!catOk}
          onClick={() => apply(setVfo(VFO_B))}
          title={t('cockpit.tuning.vfo.title', { vfo: VFO_B })}
        >
          {VFO_B}
        </button>
      </span>
      <span className={`tuning-clar${rit !== 0 ? ' on' : ''}`}>
        <button type="button" disabled={!catOk} onClick={() => apply(setRit(0))} title={t('cockpit.tuning.rit.title')}>
          {RIT}
        </button>
        <button type="button" disabled={!catOk} onClick={() => apply(setRit(rit - 10))} aria-label={t('cockpit.tuning.rit.down.aria')}>
          −
        </button>
        <span className="tuning-clar-val mono">{fmtOffset(rit)}</span>
        <button type="button" disabled={!catOk} onClick={() => apply(setRit(rit + 10))} aria-label={t('cockpit.tuning.rit.up.aria')}>
          +
        </button>
      </span>
      <span className={`tuning-clar${xit !== 0 ? ' on' : ''}`}>
        <button type="button" disabled={!catOk} onClick={() => apply(setXit(0))} title={t('cockpit.tuning.xit.title')}>
          {XIT}
        </button>
        <button type="button" disabled={!catOk} onClick={() => apply(setXit(xit - 10))} aria-label={t('cockpit.tuning.xit.down.aria')}>
          −
        </button>
        <span className="tuning-clar-val mono">{fmtOffset(xit)}</span>
        <button type="button" disabled={!catOk} onClick={() => apply(setXit(xit + 10))} aria-label={t('cockpit.tuning.xit.up.aria')}>
          +
        </button>
      </span>
    </div>
  )
}
