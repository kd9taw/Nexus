// #480: the listening level's slider mapping. The saved level is a plain gain (0..1 as it always
// was, above 1 up to +20 dB of boost); the slider's left half is the old range, its right half the
// boost in even decibels.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { LEVEL_MAX, SLIDER_MAX, boostDb, levelText, levelToSlider, sliderToLevel } from './listeningLevel'

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

  it('a position whose boost rounds to 0 dB saves exactly unity, so it reads what it saves', () => {
    // The whole span of the right half that reads "100%": every one of them saves 1, not a hair more.
    for (let p = 1; p <= 1.05; p += 0.001) {
      const level = sliderToLevel(p)
      if (boostDb(level) === 0) expect(level, `position ${p}`).toBe(1)
    }
    expect(sliderToLevel(1.02)).toBe(1)
    // The first position that reads a boost still saves one.
    expect(levelText(sliderToLevel(1.03))).toBe('+1 dB')
    expect(sliderToLevel(1.03)).toBeGreaterThan(1)
  })
})

// The UI's top and the backend's must be one number: a UI top above the backend's would show a
// boost the audio clamps away, one below would hide the boost the backend allows.
describe("the listening level's top is the backend's (tempo-audio MONITOR_LEVEL_MAX)", () => {
  const monitorRs = readFileSync(
    fileURLToPath(new URL('../../../crates/tempo-audio/src/monitor.rs', import.meta.url)),
    'utf8',
  )

  it('LEVEL_MAX equals MONITOR_LEVEL_MAX', () => {
    const m = /pub const MONITOR_LEVEL_MAX\s*:\s*f32\s*=\s*([0-9._]+)\s*;/.exec(monitorRs)
    expect(m, 'monitor.rs declares MONITOR_LEVEL_MAX').not.toBeNull()
    expect(LEVEL_MAX).toBe(Number(m![1].replace(/_/g, '')))
    // And the slider's top reaches it.
    expect(sliderToLevel(SLIDER_MAX)).toBe(LEVEL_MAX)
  })
})
