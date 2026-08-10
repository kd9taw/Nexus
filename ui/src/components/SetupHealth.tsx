import type { CatTestResult } from '../types'
import { rxLevelDb } from './LevelMeter'

/** Setup Health — "is the station actually working?" made visible, so setup stops running on
 * faith (0.17.0). Reads live snapshot state: Rig (CAT responding), RX audio (level/error), and
 * whether TX is armed. A live Test-CAT result, when present, overrides the passive CAT state.
 * SHARED (extracted from SettingsPanel, 2026-08-09): Settings and the wizard's rig step render
 * the same strip — its own comment promised "the wizard finale … can render the same strip
 * later", and the wizard's verify stage is that later. */
export function SetupHealth({
  radio,
  catResult,
  onProveTx,
}: {
  radio?: {
    catOk?: boolean | null
    catDetail?: string
    rxLevel: number
    audioError?: string | null
    txEnabled: boolean
    tuning?: boolean
    txPower?: number | null
  }
  catResult: CatTestResult | null
  /** Key a bounded tune carrier to prove the CAT→PTT→RF path (behind a confirm dialog). */
  onProveTx?: () => void
}) {
  const rigOk = catResult ? catResult.ok : radio?.catOk
  const rigDetail = catResult ? catResult.detail : radio?.catDetail
  const rxDb = radio ? Math.round(rxLevelDb(radio.rxLevel)) : null
  const rxLive = rxDb != null && rxDb > -60 && !radio?.audioError
  const cls = (ok?: boolean | null) => (ok === true ? 'ok' : ok === false ? 'bad' : 'unknown')
  const tuning = !!radio?.tuning
  const watts = radio?.txPower ?? null
  // While keying: green once forward power registers (RF is being made → CAT/PTT/rig all work).
  const txClass = tuning ? (watts != null && watts > 0 ? 'ok' : 'bad') : 'unknown'
  return (
    <div className="setup-health" role="status" aria-label="Setup health">
      <span className="setup-health-title">Setup health</span>
      <span
        className={`health-item ${cls(rigOk)}`}
        title={rigDetail || 'CAT not tested yet — use Test CAT below'}
      >
        <span className="health-dot" /> Rig{' '}
        {rigOk === true ? 'responding' : rigOk === false ? 'not answering' : 'untested'}
      </span>
      <span
        className={`health-item ${radio?.audioError ? 'bad' : rxLive ? 'ok' : 'unknown'}`}
        title={
          radio?.audioError || (rxLive ? 'Receiving audio' : 'No RX audio — check the audio device below')
        }
      >
        <span className="health-dot" /> RX audio{' '}
        {radio?.audioError ? 'error' : rxDb != null ? `${rxDb} dB` : '—'}
      </span>
      <span
        className={`health-item ${txClass}`}
        title={
          tuning
            ? 'Keying a tune carrier — forward power confirms the CAT → PTT → RF path'
            : radio?.txEnabled
              ? 'Transmit is enabled'
              : 'Transmit is off'
        }
      >
        <span className="health-dot" /> TX{' '}
        {tuning
          ? `keying${watts != null ? ` · ${watts.toFixed(0)} W` : '…'}`
          : radio?.txEnabled
            ? 'on'
            : 'off'}
      </span>
      {onProveTx && !tuning && (
        <button
          type="button"
          className="np-chip health-prove"
          onClick={() => {
            if (
              window.confirm(
                'Prove the transmit path?\n\nThis keys your transmitter for ~2 seconds at your tune ' +
                  'power. Make sure an antenna or dummy load is connected.',
              )
            )
              onProveTx()
          }}
          title="Key a 2 s tune carrier to verify CAT → PTT → RF (asks first, every time)"
        >
          Prove TX
        </button>
      )}
    </div>
  )
}
