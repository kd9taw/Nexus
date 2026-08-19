import type { ReactNode } from 'react'
import type { CatTestResult } from '../types'
import { rxLevelDb } from './LevelMeter'

/** One health dot. A `<button>` when it can navigate to the fix, a `<span>` when it cannot —
 * never a button that does nothing, which reads as broken (`GettingStartedGuide.test.tsx`
 * refuses inert-looking controls for the same reason). */
function HealthItem({
  className,
  title,
  onGoTo,
  children,
}: {
  className: string
  title: string
  onGoTo?: (() => void) | false
  children: ReactNode
}) {
  if (!onGoTo) {
    return (
      <span className={className} title={title}>
        {children}
      </span>
    )
  }
  return (
    <button type="button" className={`${className} health-item-link`} title={title} onClick={onGoTo}>
      {children}
    </button>
  )
}

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
  onGoTo,
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
  /** Jump to the Settings section that fixes a red light (a registry section id).
   *
   * The strip's whole job is to say what is broken, and it used to end its sentences with
   * "below" — "check the audio device below" pointed 1,870 lines and six fieldsets down a
   * scroller with no anchors in it. A diagnosis that cannot reach its own remedy is half a
   * feature. Omitted (the wizard's rig step) = the dots stay plain text, as before. */
  onGoTo?: (section: string) => void
}) {
  const rigOk = catResult ? catResult.ok : radio?.catOk
  const rigDetail = catResult ? catResult.detail : radio?.catDetail
  const rxDb = radio ? Math.round(rxLevelDb(radio.rxLevel)) : null
  // rxLevelDb is the WSJT-X-style 0..90 scale and returns 0 for silence — the old `> -60`
  // was a dBFS threshold that EVERY value on this scale passed, so the dot could never say
  // "No RX audio" while a radio existed (mac QA audit merged[46]; in every shipped mac DMG,
  // where a denied mic delivers exactly that permanent 0). Real capture always carries a
  // nonzero noise floor (the meter's own zones call <15 dB merely "too quiet"), so 0 = dead.
  const rxLive = rxDb != null && rxDb > 0 && !radio?.audioError
  const cls = (ok?: boolean | null) => (ok === true ? 'ok' : ok === false ? 'bad' : 'unknown')
  const tuning = !!radio?.tuning
  const watts = radio?.txPower ?? null
  // While keying: green once forward power registers (RF is being made → CAT/PTT/rig all work).
  const txClass = tuning ? (watts != null && watts > 0 ? 'ok' : 'bad') : 'unknown'
  return (
    <div className="setup-health" role="status" aria-label="Setup health">
      <span className="setup-health-title">Setup health</span>
      {/* Each dot is a BUTTON when the host can navigate (Settings) and plain text when it
          cannot (the wizard, which already has the controls on screen). The wording drops
          "below" in the button form — it now goes there. */}
      <HealthItem
        className={`health-item ${cls(rigOk)}`}
        title={rigDetail || (onGoTo ? 'CAT not tested yet — open Rig Control' : 'CAT not tested yet — use Test CAT below')}
        onGoTo={onGoTo && (() => onGoTo('rig-control'))}
      >
        <span className="health-dot" /> Rig{' '}
        {rigOk === true ? 'responding' : rigOk === false ? 'not answering' : 'untested'}
      </HealthItem>
      <HealthItem
        className={`health-item ${radio?.audioError ? 'bad' : rxLive ? 'ok' : 'unknown'}`}
        title={
          radio?.audioError ||
          (rxLive
            ? 'Receiving audio'
            : onGoTo
              ? 'No RX audio — open the audio device settings'
              : 'No RX audio — check the audio device below')
        }
        onGoTo={onGoTo && (() => onGoTo('audio'))}
      >
        <span className="health-dot" /> RX audio{' '}
        {radio?.audioError ? 'error' : rxDb != null ? `${rxDb} dB` : '—'}
      </HealthItem>
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
