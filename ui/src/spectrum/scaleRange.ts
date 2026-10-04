// The display range a frame is drawn in: an automatic one from the frame itself, then the operator's
// G and Z on top, in the frame's own dB.
//
// AUTO RANGE is deskHPSDR's automatic waterfall level, as its source has it (`waterfall.c`,
// `waterfall_update`, under `waterfall_automatic`): the black point 5 dB under the MEAN of the row's dB
// values, the top 55 dB above the black point, per frame, with no smoothing of its own. The mean is of
// the values the operator is looking at, so the host passes the bins inside its view, the way the rig
// scope's AGC already measures only the visible window (a loud signal outside the view must not move
// what is shown). Averaging the frame first (scaleAverage.ts) is what steadies it.
//
// Measured on the harness's own fixtures (ui/spectrum-harness/scaleRange.test.ts), and two things the
// rule is not:
// - Carriers land where the rig scope draws them today (within two palette steps of its median + 50 dB
//   window), but the noise does not: the black point sits 5 dB under the mean, so the noise median is
//   drawn about 9% up the palette (index 20–22) where today's scope draws it at 0.
// - It lights the noise the FT waterfall keeps dark. On the FT8 fixture's quiet rows 2.4% of the field is
//   black under this rule and 78% under the parked floor (`parkFloor`, the operator's "the back is dark
//   and not over noisy", 2026-08-05), so it is not a drop-in for the digital waterfall's range.
// - deskHPSDR's PANADAPTER autoscale is a different rule (the 60th percentile plus 3 dB, smoothed once a
//   second); this is its waterfall's, which the specification names.
//
// G AND Z are today's sliders with today's meaning (`applyGainZero`): Z trims the black point by up to
// ±WF_ZERO_TRIM_DB, G narrows or widens the window. Only the unit changes: their dB are the frame's own
// (`dbPerUnit`), so Z moves the black point 5 dB on a CI-V scope as on the audio feed.

import { applyGainZero } from '../waterfall'
import { safeDbPerUnit } from './aggregate'
import type { DisplayRange } from './types'

/** dB the automatic black point sits under the mean of the row's values. */
export const AUTO_UNDER_MEAN_DB = 5
/** dB from the automatic black point to the top of the range. */
export const AUTO_SPAN_DB = 55

/**
 * The automatic range of the finite values `values[from..to)` on an axis of `dbPerUnit`: the black
 * point `AUTO_UNDER_MEAN_DB` under their mean, the top `AUTO_SPAN_DB` above it. A row with no finite
 * value gets the range a row of zeros would, so a host always has one to commit.
 */
export function autoRange(values: ArrayLike<number>, dbPerUnit: number, from = 0, to = values.length): DisplayRange {
  const d = safeDbPerUnit(dbPerUnit)
  let sum = 0
  let n = 0
  const end = Math.min(to, values.length)
  for (let i = Math.max(0, from); i < end; i++) {
    const v = values[i]
    if (Number.isFinite(v)) {
      sum += v
      n++
    }
  }
  const floor = (n > 0 ? sum / n : 0) - AUTO_UNDER_MEAN_DB / d
  return { floor, ceil: floor + AUTO_SPAN_DB / d }
}

/** The range a frame is drawn in: `auto` with the operator's G and Z on top, in the frame's dB. */
export function displayRange(auto: DisplayRange, gain: number, zero: number, dbPerUnit: number): DisplayRange {
  return applyGainZero(auto.floor, auto.ceil, gain, zero, safeDbPerUnit(dbPerUnit))
}
