import { useState } from 'react'
import type { AppSnapshot, BandChannel, Tier } from '../types'
import { bandLabelForMhz } from '../band'
import { CockpitHeader } from './CockpitHeader'
import { FrequencyControl } from './FrequencyControl'
import { TuningStrip } from './TuningStrip'

/** Tempo tiers for the header mode indicator (parallels FT8's FT8/FT4 tiles). */
const TEMPO_TIERS = [
  { tier: 'FT1' as Tier, label: 'FT1', slot: 'Fast', title: 'FT1 — fast Tempo tier' },
  { tier: 'DX1' as Tier, label: 'DX1', slot: 'Robust', title: 'DX1 — robust weak-signal tier' },
]

interface Props {
  snap: AppSnapshot
  onSnap?: (s: AppSnapshot) => void
  tier: Tier
  onTierChange: (t: Tier) => void
  bandPlan: BandChannel[]
  onSetFrequency: (dialMhz: number, band: string, mode: string) => void
  onSetTxLevel: (level: number) => void
}

/**
 * Tempo (FT1/DX1 chat) cockpit header — the same shared CockpitHeader the CW /
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
}: Props) {
  const [tuneStep, setTuneStep] = useState(100)
  const commitDial = (mhz: number) => {
    const band = bandLabelForMhz(mhz)
    if (!band) return
    onSetFrequency(mhz, band, snap.radio.sideband || 'USB')
  }
  return (
    <CockpitHeader
      snap={snap}
      onSnap={onSnap}
      modeIndicator={
        <div className="cockpit-modes" role="group" aria-label="Tempo tier">
          {TEMPO_TIERS.map((m) => (
            <button
              key={m.tier}
              type="button"
              className={`cockpit-mode${tier === m.tier ? ' active' : ''}`}
              aria-pressed={tier === m.tier}
              onClick={() => onTierChange(m.tier)}
              title={m.title}
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
        label: 'Pwr',
        title: "TX drive (Pwr) — trim down until your rig's ALC is just zero",
      }}
      txActiveLabel="▲ TX"
    />
  )
}
