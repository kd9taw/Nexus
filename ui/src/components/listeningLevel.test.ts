// #480: the listening level's slider mapping. The saved level is a plain gain (0..1 as it always
// was, above 1 up to +20 dB of boost); the slider's left half is the old range, its right half the
// boost in even decibels.
import { describe, it, expect } from 'vitest'
import { LEVEL_MAX, boostDb, levelText, levelToSlider, sliderToLevel } from './listeningLevel'

describe('the listening level (#480)', () => {
  it('levels saved before the boost keep their place and their value', () => {
    for (const l of [0, 0.25, 0.5, 0.8, 1]) {
      expect(levelToSlider(l)).toBe(l)
      expect(sliderToLevel(levelToSlider(l))).toBe(l)
      expect(boostDb(l)).toBe(0)
    }
    expect(levelText(0.5)).toBe('50%')
    expect(levelText(1)).toBe('100%')
  })

  it('the right half is 0..+20 dB of boost, evenly', () => {
    expect(sliderToLevel(2)).toBe(LEVEL_MAX)
    expect(boostDb(sliderToLevel(1.5))).toBe(10)
    expect(boostDb(LEVEL_MAX)).toBe(20)
    expect(levelText(LEVEL_MAX)).toBe('+20 dB')
    expect(levelToSlider(LEVEL_MAX)).toBeCloseTo(2)
  })

  it('a saved value out of range or not a number is clamped', () => {
    expect(levelToSlider(50)).toBeCloseTo(2)
    expect(levelToSlider(-1)).toBe(0)
    expect(levelToSlider(Number.NaN)).toBe(0)
    expect(sliderToLevel(3)).toBe(LEVEL_MAX)
  })
})
