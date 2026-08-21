// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT: the band names, which are both the option LABEL
// and the option VALUE here — the value is what `pickBand` sends the engine, so neither may move.
//
// The 🔒 chip is a READOUT of `txAllowed`, not a transmit control: it says the engine is already
// blocking transmit here. Nothing on this surface keys, unkeys or stops a transmission.
import { useEffect, useState } from 'react'
import type { AppSnapshot, BandChannel } from '../types'
import { getLicensedBandPlan, pickBand } from '../api'
import { bandColor } from '../bandColors'
import { t } from '../i18n'

interface Props {
  snap: AppSnapshot
  /** The cockpit's operating mode ('phone' | 'cw') — drives which licensed segments/bands to
   * show. Passed explicitly so it can't race the async engine operating-mode set on entry. */
  mode: string
  /** Apply the snapshot returned by the QSY immediately (else it lands on the next poll). */
  onSnap?: (snap: AppSnapshot) => void
}

/**
 * Licensed-band dropdown for the CW/Phone cockpits: lists only the bands the operator may use
 * in the current mode (per their license class). Selecting one names the BAND to the engine
 * (pickBand) rather than computing a dial here, so the pick can consult the per-(band, mode)
 * dial memory: the VFO lands on the last dial used on that band in this mode THIS SESSION,
 * and on the START of the licensed segment (CW-segment start in CW, phone-segment start in
 * Phone) when there is none — the same dial this dropdown always sent. Explicit MHz entry
 * (setFrequency) stays authoritative. Plus a TX-LOCK chip when the current dial/mode is
 * outside privileges (transmit hard-blocked by the engine). Open-mode operators see all bands
 * and never the lock.
 */
export function BandPicker({ snap, mode, onSnap }: Props) {
  const [plan, setPlan] = useState<BandChannel[]>([])
  useEffect(() => {
    void getLicensedBandPlan(mode).then(setPlan).catch(() => {})
  }, [mode])

  const onPick = (band: string) => {
    if (!plan.some((c) => c.band === band)) return
    void pickBand(band, mode)
      .then((s) => onSnap?.(s))
      .catch(() => {})
  }

  // If the operator has manually tuned to a band that's not a licensed jump target (or off
  // the plan), still show it as the selected option so the control reflects the real dial.
  const known = plan.some((c) => c.band === snap.radio.band)

  // The band is a PRIMARY operating fact — color the control with the active band's color
  // (shared with the map's spot dots) so "what band am I on" reads across the room.
  const col = bandColor(snap.radio.band)

  return (
    <div className="band-picker">
      <span className="band-picker-dot" style={{ background: col }} aria-hidden="true" />
      <select
        className="band-picker-select"
        value={snap.radio.band}
        onChange={(e) => onPick(e.target.value)}
        title={t('bandPicker.select.title')}
        style={{ color: col, borderColor: col, boxShadow: `0 0 0 1px ${col}55, 0 0 10px ${col}33` }}
      >
        {!known && <option value={snap.radio.band}>{snap.radio.band}</option>}
        {plan.map((c) => (
          <option key={c.band} value={c.band}>
            {c.band}
          </option>
        ))}
      </select>
      {!snap.radio.txAllowed && (
        <span className="tx-lock" title={t('bandPicker.txLock.title')}>
          {t('bandPicker.txLock.label')}
        </span>
      )}
    </div>
  )
}
