import { describe, it, expect } from 'vitest'
import { openingToastSpec } from './openingAlert'
import type { OpeningView } from './types'

function opening(over: Partial<OpeningView>): OpeningView {
  return {
    band: '6m',
    mode: 'Sporadic-E',
    octant: 'SE',
    bearingDeg: 135,
    maxKm: 1316,
    probability: 0.9,
    stations: 14,
    confidence: 'Strong',
    confidenceScore: 0.9,
    reciprocalPairs: 6,
    anomalyZ: 56,
    onsetSecs: 60,
    isNew: true,
    note: '',
    ...over,
  }
}

describe('openingToastSpec', () => {
  it('goes loud for a strong Sporadic-E opening', () => {
    const s = openingToastSpec(opening({}), 'metric')
    expect(s.prominent).toBe(true)
    expect(s.beepHz).toBe(760)
    expect(s.message).toContain('SPORADIC-E')
    expect(s.message).toContain('NOW')
  })

  // REGRESSION (operator 2026-08-05): "the 6m opening detection is too liberal…
  // it's misfiring on openings where I tune and hear nothing."
  //
  // The gate fix (opening.rs `raw_open`) suppresses the openings that were not
  // real. This is the other half: an opening that IS real in the data can still
  // be inaudible tuning by ear, because every input to the detector is a
  // reception report from a decoder — PSK Reporter, the operator's own FT8
  // roster, CW/RTTY RBN — and a −22 dB FT8 decode is not a signal you can hear.
  // The backend already scores that honestly and the toast threw the score away:
  // the identical drop-everything wording fired at confidence 0.97 (14 stations)
  // and at 0.67 (2 stations). Only Strong confidence earns "point SE NOW".
  it('does not shout about a thin-evidence opening', () => {
    const s = openingToastSpec(
      opening({ stations: 2, anomalyZ: 4.2, confidence: 'Likely', confidenceScore: 0.5 }),
      'metric',
    )
    expect(s.prominent).toBe(false)
    expect(s.beepHz).toBeNull()
    expect(s.message).not.toContain('NOW')
    // …and it says WHY it is hedged, rather than just being quieter.
    expect(s.message).toContain('2 stns')
    expect(s.message.toLowerCase()).toContain('ear')
    // The opening is still announced — this is a label, not a filter. The gate
    // is what suppresses non-openings.
    expect(s.message).toContain('6m')
    expect(s.message).toContain('Sporadic-E')
  })

  it('hedges F2 and Aurora on thin evidence too, by the same rule', () => {
    for (const mode of ['F2', 'Aurora']) {
      const s = openingToastSpec(opening({ mode, confidenceScore: 0.4, stations: 2 }), 'metric')
      expect(s.prominent, mode).toBe(false)
      expect(s.beepHz, mode).toBeNull()
    }
  })

  it('leaves the already-quiet tiers alone', () => {
    // Tropo is capped at Marginal by the backend (geometry-only v1), so gating
    // its wording on confidence would silence EVERY tropo opening. It is already
    // a quiet, informative toast and must keep its own text.
    const s = openingToastSpec(opening({ mode: 'Tropo', confidenceScore: 0.5, maxKm: 900 }), 'metric')
    expect(s.prominent).toBe(false)
    expect(s.message).toContain('tropo opening')
    expect(s.message).toContain('900 km')
  })

  // Every tier that names a distance names it in the operator's units. The toasts used to carry
  // a literal "km" and the raw kilometres whatever Settings ▸ Units said. 900 km is 559 mi.
  it('states the distance in the units the operator chose, on every tier that names one', () => {
    const cases: [string, Partial<OpeningView>][] = [
      ['Sporadic-E', {}],
      ['F2', { mode: 'F2' }],
      ['Tropo', { mode: 'Tropo', confidenceScore: 0.5 }],
      ['thin evidence', { confidenceScore: 0.4, stations: 2 }],
    ]
    for (const [name, over] of cases) {
      const o = opening({ maxKm: 900, ...over })
      const miles = openingToastSpec(o, 'imperial').message
      expect(miles, name).toContain('~559 mi')
      expect(miles, name).not.toContain('km')
      expect(openingToastSpec(o, 'metric').message, name).toContain('~900 km')
    }
  })
})
