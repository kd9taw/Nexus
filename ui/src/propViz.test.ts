import { describe, it, expect } from 'vitest'
import {
  workabilityVar,
  tierVar,
  needMeta,
  heatColor,
  fmtZ,
  sfiImpact,
  kpImpact,
  xrayImpact,
} from './propViz'

describe('propViz', () => {
  it('maps workability words to tokens (good=open, closed=closed)', () => {
    expect(workabilityVar('Excellent')).toBe('var(--band-open)')
    expect(workabilityVar('Good')).toBe('var(--band-open)')
    expect(workabilityVar('Fair')).toBe('var(--band-marginal)')
    expect(workabilityVar('Closed')).toBe('var(--band-closed)')
  })

  it('maps activity tiers to tokens (Quiet/Closed are calm neutrals, not red)', () => {
    expect(tierVar('Active')).toBe('var(--band-open)')
    expect(tierVar('Moderate')).toBe('var(--band-marginal)')
    expect(tierVar('Quiet')).toBe('var(--text-dim)')
    expect(tierVar('Closed')).toBe('var(--text-faint)')
  })

  it('maps need tiers to a glyph + token (color + glyph, never color alone)', () => {
    expect(needMeta('Atno').glyph).toBe('★')
    expect(needMeta('Atno').cssVar).toBe('--status-new-entity')
    expect(needMeta('Confirm').glyph).toBe('✓')
  })

  it('heatColor returns an rgb() string; brighter for higher score', () => {
    expect(heatColor(0)).toMatch(/^rgb\(\d+, \d+, \d+\)$/)
    const lum = (s: string) =>
      (s.match(/\d+/g) || []).map(Number).reduce((a, b) => a + b, 0)
    expect(lum(heatColor(0.9))).toBeGreaterThan(lum(heatColor(0.1)))
  })

  it('formats UTC hours and clamps/wraps', () => {
    expect(fmtZ(14)).toBe('14Z')
    expect(fmtZ(0)).toBe('00Z')
    expect(fmtZ(25)).toBe('01Z')
  })

  it('space-weather impacts cross thresholds with sane severity', () => {
    expect(sfiImpact(160).sev).toBe('active')
    expect(sfiImpact(70).sev).toBe('quiet')
    expect(kpImpact(6).sev).toBe('warn')
    expect(kpImpact(2).sev).toBe('quiet')
    expect(xrayImpact('M1').sev).toBe('warn')
    expect(xrayImpact('A0').sev).toBe('quiet')
  })
})
