// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The tier NAMES
// (TempoFast / TempoDeep, and their Fast / Deep slots) are the modes' own and stay here; the
// ▲ TX indicator is the transmit-state token. Everything else is prose in the catalog.
import { useState } from 'react'
import { t, type MessageKey } from '../i18n'
import type { AppSnapshot, BandChannel, Tier } from '../types'
import { bandLabelForMhz } from '../band'
import { CockpitHeader } from './CockpitHeader'
import { FrequencyControl } from './FrequencyControl'
import { TuningStrip } from './TuningStrip'

/** Tempo tiers for the header mode indicator (parallels FT8's FT8/FT4 tiles). */
const TEMPO_TIERS: { tier: Tier; label: string; slot: string; titleKey: MessageKey }[] = [
  {
    tier: 'TempoFast',
    label: 'TempoFast',
    slot: 'Fast',
    titleKey: 'tempo.header.tier.fast.title',
  },
  {
    tier: 'TempoDeep',
    label: 'TempoDeep',
    slot: 'Deep',
    titleKey: 'tempo.header.tier.deep.title',
  },
]

interface Props {
  snap: AppSnapshot
  onSnap?: (s: AppSnapshot) => void
  tier: Tier
  onTierChange: (t: Tier) => void
  bandPlan: BandChannel[]
  onSetFrequency: (dialMhz: number, band: string, mode: string) => void
  onSetTxLevel: (level: number) => void
  /** Wheel sensitivity (Settings) — how much scroll one tuning step costs. */
  wheelSensitivity?: number
  /** Toggle the CQ RUN (keep calling every idle TX slot). */
  onToggleCqRun: () => void
  /** Resume a paused run immediately. */
  onResumeCqRun: () => void
}

/**
 * Tempo (TempoFast/TempoDeep chat) cockpit header — the same shared CockpitHeader the CW /
 * Phone / FT8 cockpits use, giving Tempo the base rig controls (tier · frequency
 * readout + the FT8-style frequency dropdown · drive power · CAT) in the
 * consistent position. Tune / Stop / Enable-Tx stay in the TopBar transmit
 * cluster (Tempo's existing model), like FT8 keeps its TX cluster in the QSO
 * strip. Rendered full-width above the three-pane Tempo workspace.
 */
export function TempoHeader({
  snap,
  onSnap,
  tier,
  onTierChange,
  bandPlan,
  onSetFrequency,
  onSetTxLevel,
  wheelSensitivity,
  onToggleCqRun,
  onResumeCqRun,
}: Props) {
  const cq = snap.chatCq ?? 'off'
  const [tuneStep, setTuneStep] = useState(100)
  const commitDial = (mhz: number) => {
    // An EMPTY band label is not a refusal: listening off the ham bands is first-class (operator,
    // 2026-08-13), so a typed WWV/shortwave/inter-band frequency tunes there. This used to
    // discard the typed value in SILENCE — the worst of the six, because nothing said why.
    onSetFrequency(mhz, bandLabelForMhz(mhz), snap.radio.sideband || 'USB')
  }
  return (
    <CockpitHeader
      snap={snap}
      onSnap={onSnap}
      modeIndicator={
        <div className="cockpit-modes" role="group" aria-label={t('tempo.header.tier.aria')}>
          {TEMPO_TIERS.map((m) => (
            <button
              key={m.tier}
              type="button"
              className={`cockpit-mode${tier === m.tier ? ' active' : ''}`}
              aria-pressed={tier === m.tier}
              onClick={() => onTierChange(m.tier)}
              title={t(m.titleKey)}
            >
              <span className="cm-name">{m.label}</span>
              <span className="cm-slot">{m.slot}</span>
            </button>
          ))}
        </div>
      }
      bandControl={
        <FrequencyControl
          channels={bandPlan}
          dialMhz={snap.radio.dialMhz}
          band={snap.radio.band}
          mode={snap.radio.sideband}
          variant="compact"
          showReadout={false}
          showModeToggle={false}
          onSet={onSetFrequency}
        />
      }
      onCommitDial={commitDial}
      // Per-digit wheel tuning, the same as the other five main dials. Tempo was the one cockpit
      // rendering this header without it, so its readout was the only one that did not respond to
      // a scroll. Digit-only (no uniform `wheelTune`), matching Operate/RTTY/SSTV: Tempo works
      // agreed calling frequencies, so wheeling the whole readout is not the gesture — Phone and
      // CW have that because they hunt.
      digitTune
      wheelSensitivity={wheelSensitivity}
      frequencyExtras={
        <TuningStrip
          snap={snap}
          onSnap={onSnap}
          step={tuneStep}
          onStep={setTuneStep}
          showReadout={false}
        />
      }
      power={{
        value: snap.radio.txLevel,
        unit: 'drive',
        onChange: onSetTxLevel,
        // A CONFIGURATION control on the transmit path is not a transmit control — the
        // batch-13 ruling, where the drive slider moved and Prove TX did not.
        label: t('tempo.header.power.label'),
        title: t('tempo.header.power.title'),
      }}
      txActiveLabel="▲ TX"
    >
      {/* CQ RUN — the persistent keep-calling control (the one-shot Call CQ button's
          dead-end fix): reachable from the header in every chat view, with the run
          state always visible. Paused = someone answered (sequential policy). */}
      <div className="cq-run" role="group" aria-label={t('tempo.header.cqRun.aria')}>
        <button
          type="button"
          className={`cq-run-btn${cq !== 'off' ? ' on' : ''}${cq === 'paused' ? ' paused' : ''}`}
          aria-pressed={cq !== 'off'}
          onClick={onToggleCqRun}
          title={
            cq === 'off'
              ? t('tempo.header.cqRun.off.title')
              : cq === 'paused'
                ? t('tempo.header.cqRun.paused.title')
                : t('tempo.header.cqRun.on.title')
          }
        >
          {cq === 'off'
            ? t('tempo.header.cqRun.off')
            : cq === 'paused'
              ? t('tempo.header.cqRun.paused')
              : t('tempo.header.cqRun.on')}
        </button>
        {cq === 'paused' && (
          <button
            type="button"
            className="cq-run-btn resume"
            onClick={onResumeCqRun}
            title={t('tempo.header.cqRun.resume.title')}
          >
            {t('tempo.header.cqRun.resume')}
          </button>
        )}
      </div>
    </CockpitHeader>
  )
}
