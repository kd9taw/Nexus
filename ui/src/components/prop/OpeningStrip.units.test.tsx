// @vitest-environment jsdom
//
// The opening strip states how far the opening reaches in the units the operator chose. Its
// catalog sentence carried a literal "km" and the raw kilometres whatever Settings ▸ Units said.
import { describe, it, expect, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { OpeningStrip } from './OpeningStrip'
import { setUnitsMirror } from '../../units'
import type { OpeningView } from '../../types'

afterEach(() => {
  cleanup()
  localStorage.clear()
})

// 965 km is 600 mi.
const OPENING: OpeningView = {
  band: '6m',
  mode: 'Sporadic-E',
  octant: 'SW',
  bearingDeg: 225,
  maxKm: 965,
  probability: 0.9,
  stations: 5,
  confidence: 'Strong',
  confidenceScore: 0.9,
  reciprocalPairs: 0,
  anomalyZ: 12,
  onsetSecs: 0,
  isNew: false,
  note: '',
}

function detail(units: 'metric' | 'imperial'): string {
  setUnitsMirror(units)
  const { container } = render(<OpeningStrip openings={[OPENING]} />)
  return container.querySelector('.opening-detail')?.textContent ?? ''
}

describe('the opening strip follows the units setting', () => {
  it('Imperial: the reach reads in miles', () => {
    expect(detail('imperial')).toBe('point SW · ~600 mi · 5 stations · Strong')
  })

  it('Metric: the reach reads in kilometres', () => {
    expect(detail('metric')).toBe('point SW · ~965 km · 5 stations · Strong')
  })
})
