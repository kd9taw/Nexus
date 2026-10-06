// The Connect boxes' Basic sentences follow the units setting, exactly as their Expert renders do.
//
// Each of these sentences carried a literal "km" in its catalog entry and was handed the raw
// kilometres, so an Imperial operator read kilometres in the one-line view of the Getting Out box
// and of the opening boxes. The box context carries the resolved units (`PaneContext.units`, read
// once per window by `usePaneContext`), and every distance reaches its sentence already formatted
// with its unit by `fmtDistanceKm`.
import { describe, it, expect } from 'vitest'
import { esNowcastLine, getoutLine, openingsLine } from './paneFormat'
import type { PaneContext } from './paneContext'
import type { GettingOut, HeardMe, OpeningView } from '../../types'
import type { Units } from '../../units'

// 965 km is 600 mi; 644 km is 400 mi.
const heard = (call: string, octant: string, km: number): HeardMe => ({
  call,
  grid: null,
  band: '20m',
  snr: -12,
  bearingDeg: 0,
  km,
  octant,
  ageSecs: 60,
})

const opening = (over: Partial<OpeningView> = {}): OpeningView => ({
  band: '6m',
  mode: 'Sporadic-E',
  octant: 'SW',
  bearingDeg: 225,
  maxKm: 965,
  probability: 0.9,
  stations: 5,
  confidence: 'Strong',
  confidenceScore: 0.9,
  reciprocalPairs: 2,
  anomalyZ: 12,
  onsetSecs: 120,
  isNew: false,
  note: '',
  ...over,
})

const ctx = (units: Units, over: Partial<PaneContext>): PaneContext =>
  ({ units, prop: null, getout: null, ...over }) as unknown as PaneContext

describe('the Getting Out Basic line', () => {
  const getout: GettingOut = { count: 2, maxKm: 965, reports: [heard('DL1AA', 'NE', 965), heard('G4XYZ', 'E', 644)] }

  it('puts the direction summary’s distance in miles under Imperial and km under Metric', () => {
    expect(getoutLine(ctx('imperial', { getout }))).toBe(
      '2 hearing you — strongest toward NE (~600 mi); little/nothing to the N/SE/S/SW/W/NW.',
    )
    expect(getoutLine(ctx('metric', { getout }))).toBe(
      '2 hearing you — strongest toward NE (~965 km); little/nothing to the N/SE/S/SW/W/NW.',
    )
  })

  it('says how far the furthest receiver is in the chosen unit when no direction can be named', () => {
    // No report carries a usable octant, so there is no direction summary and the line falls back
    // to the box's own furthest distance.
    const furthestOnly: GettingOut = { count: 2, maxKm: 965, reports: [] }
    expect(getoutLine(ctx('imperial', { getout: furthestOnly }))).toBe('2 hearing you — furthest 600 mi.')
    expect(getoutLine(ctx('metric', { getout: furthestOnly }))).toBe('2 hearing you — furthest 965 km.')
  })
})

describe('the opening Basic lines', () => {
  const prop = { openings: [opening()] } as unknown as PaneContext['prop']

  it('Openings: the reach of the opening in the chosen unit', () => {
    expect(openingsLine(ctx('imperial', { prop }))).toBe('6m OPEN SW — ~600 mi, 5 stns.')
    expect(openingsLine(ctx('metric', { prop }))).toBe('6m OPEN SW — ~965 km, 5 stns.')
  })

  it('Es nowcast: the reach of the Es opening in the chosen unit', () => {
    expect(esNowcastLine(ctx('imperial', { prop }))).toBe('6m OPEN SW — ~600 mi Sporadic-E, 5 stns.')
    expect(esNowcastLine(ctx('metric', { prop }))).toBe('6m OPEN SW — ~965 km Sporadic-E, 5 stns.')
  })
})
