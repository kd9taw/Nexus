// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { ModelBasis, modelBasisLines } from './ModelBasis'

describe('what a P.533 answer rests on', () => {
  it('names the power and where it came from', () => {
    expect(modelBasisLines('p533', { watts: 50, source: 'setting', amplifier: false })).toEqual([
      'Assumes 50 W, your station power setting.',
    ])
    expect(modelBasisLines('p533', { watts: 39.81, source: 'rig', amplifier: false })).toEqual([
      "Assumes about 40 W, the radio's output on your last transmission.",
    ])
    expect(modelBasisLines('p533', { watts: 501, source: 'amplifier', amplifier: true })).toEqual([
      "Assumes about 501 W, the amplifier's output on your last transmission.",
    ])
    expect(modelBasisLines('p533', { watts: 5, source: 'setting', amplifier: false })[0]).toContain('5 W')
  })

  it('says 100 W is a default, and that an amplifier may put more on the air', () => {
    expect(modelBasisLines('p533', { watts: 100, source: 'default', amplifier: false })).toEqual([
      'Assumes 100 W: no station power is set and the radio has not reported its output.',
    ])
    expect(modelBasisLines('p533', { watts: 100, source: 'default', amplifier: true })).toHaveLength(2)
  })

  it('says when a storm correction is applied, and when later hours had no Kp to apply', () => {
    expect(modelBasisLines('p533', undefined, { kp: 7, hoursWithoutKp: false })).toEqual([
      'Adjusted for a geomagnetic storm, Kp up to 7.0.',
    ])
    expect(modelBasisLines('p533', undefined, { kp: 6.67, hoursWithoutKp: true })).toEqual([
      'Adjusted for a geomagnetic storm, Kp up to 6.7.',
      'Hours with no Kp forecast are not adjusted.',
    ])
  })

  it('says nothing for the heuristic, which ignores power, and nothing on a quiet sky with no power', () => {
    expect(modelBasisLines('heuristic', { watts: 100, source: 'default', amplifier: false })).toEqual([])
    const { container } = render(<ModelBasis engine="p533" />)
    expect(container.textContent).toBe('')
  })
})
