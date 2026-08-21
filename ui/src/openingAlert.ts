// Tiered VHF/HF opening alerts, keyed by the classified propagation mode
// (OpeningView.mode = the backend PropMode label). Tier philosophy:
//   - Sporadic-E / F2 / Aurora are the drop-everything events — rare, fleeting
//     (2m Es lasts minutes), and aurora needs a different operating technique —
//     so they go LOUD (prominent + beep + long TTL) with concrete guidance.
//   - Tropo lifts are real openings but last hours — an informative quiet toast.
//   - Anything unclassified keeps the old generic one-liner, quiet.
// Local/scatter activity never reaches here at all: on VHF all three rungs of the
// detector's open gate carry a DX distance (≥500/700 km), so groundwave and
// routine scatter are dropped before an opening exists — see `raw_open` in
// crates/propagation/src/opening.rs. That claim was false until 2026-08-05, and
// it took two passes to make true: first two 6 m decodes at 111 km and 199 km
// satisfied a distance-blind station-count rung in `op_gate`, and after that was
// removed `regional_gate` was still a pure census — twelve stations, three local
// ears, two-way pairs, band-specificity, and not one question about distance.
// Both are fixed; this file cheerfully told the operator to point SE right now
// for as long as either stood.
import type { OpeningView } from './types'
import { t } from './i18n'

/// The backend's `Confidence::Strong` cut (propagation/src/engine.rs
/// `confidence_word`). Mirrored rather than re-derived so the toast's tone and
/// the confidence WORD the rest of the UI shows can never disagree.
const STRONG_CONFIDENCE = 0.66

export interface OpeningToastSpec {
  message: string
  kind: 'info' | 'success'
  ttlMs: number
  prominent: boolean
  /** Double-beep frequency; null = silent (quiet tiers). */
  beepHz: number | null
}

/** Build the toast spec for a newly-opened band (o.isNew edge). Pure. */
export function openingToastSpec(o: OpeningView): OpeningToastSpec {
  const spec = tierSpec(o)
  // HONESTY GATE (operator 2026-08-05: "misfiring on openings where I tune and
  // hear nothing"). Suppressing the openings that were not real is the gate's
  // job and the gate now does it. This is the remaining, separate problem: an
  // opening that IS real in the data can still be inaudible tuning by ear,
  // because every input to the detector is a report from a DECODER — PSK
  // Reporter, the operator's own FT8 roster, CW/RTTY RBN — and a −22 dB FT8
  // decode is not a signal you can hear on SSB.
  //
  // The backend already scores exactly that, and this file was throwing the
  // score away: the identical "rare & brief, point SE NOW" fired at confidence
  // 0.97 (14 stations, z 56) and at 0.67 (2 stations, z 4). Only Strong
  // confidence earns the drop-everything wording; below it the SAME opening is
  // announced quietly and says what the evidence actually was, so the operator
  // can decide whether it is worth leaving the chair.
  //
  // This is a LABEL, not a filter — the row still appears, still names the mode
  // and the bearing. Only the already-loud tiers are affected; Tropo is capped
  // at Marginal by the backend (geometry-only v1), so hedging on confidence
  // would silence every tropo opening there has ever been.
  if (spec.prominent && o.confidenceScore < STRONG_CONFIDENCE) {
    const km = Math.round(o.maxKm)
    return {
      message: t('prop.openingAlert.thin', {
        band: o.band,
        mode: o.mode,
        stations: o.stations,
        km,
        octant: o.octant,
      }),
      kind: 'info',
      ttlMs: 12000,
      prominent: false,
      beepHz: null,
    }
  }
  return spec
}

/** The per-mode tier, before the confidence hedge above. */
function tierSpec(o: OpeningView): OpeningToastSpec {
  const km = Math.round(o.maxKm)
  switch (o.mode) {
    case 'Sporadic-E':
      return {
        message: t('prop.openingAlert.sporadicE', {
          band: o.band,
          octant: o.octant,
          km,
          stations: o.stations,
        }),
        kind: 'success',
        ttlMs: 20000,
        prominent: true,
        beepHz: 760,
      }
    case 'Aurora':
      return {
        message: t('prop.openingAlert.aurora', { band: o.band }),
        kind: 'success',
        ttlMs: 20000,
        prominent: true,
        beepHz: 590,
      }
    case 'F2':
      return {
        message: t('prop.openingAlert.f2', {
          band: o.band,
          octant: o.octant,
          km,
          stations: o.stations,
        }),
        kind: 'success',
        ttlMs: 20000,
        prominent: true,
        beepHz: 700,
      }
    case 'Tropo':
      return {
        message: t('prop.openingAlert.tropo', {
          band: o.band,
          km,
          octant: o.octant,
          stations: o.stations,
        }),
        kind: 'info',
        ttlMs: 10000,
        prominent: false,
        beepHz: null,
      }
    default:
      return {
        message: t('prop.openingAlert.generic', {
          band: o.band,
          octant: o.octant,
          stations: o.stations,
        }),
        kind: 'success',
        ttlMs: 8000,
        prominent: false,
        beepHz: null,
      }
  }
}
