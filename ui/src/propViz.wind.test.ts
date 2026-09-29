// The Space Wx box's solar-wind speed gauge and the insight feed must agree about the same wind.
//
// The insight layer (crates/propagation/src/insight.rs) says "Fast solar-wind stream arriving"
// from a threshold written in Rust. The gauge colours its bar and words its line from
// FAST_WIND_KMS. If the two ever drift, the box calls a wind ordinary while the feed beside it
// warns about it, or the other way round — so the threshold is read out of the Rust source and
// compared, not copied into a comment.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { FAST_WIND_KMS, windSpeedImpact } from './propViz'

const INSIGHT_RS = readFileSync(
  fileURLToPath(new URL('../../crates/propagation/src/insight.rs', import.meta.url)),
  'utf8',
)

describe('windSpeedImpact', () => {
  it("turns at the insight layer's own fast-stream threshold", () => {
    const found = [...INSIGHT_RS.matchAll(/speed_kms\s*>=\s*([0-9.]+)/g)].map((m) => Number(m[1]))
    expect(found, 'insight.rs compares the wind speed against exactly one threshold').toHaveLength(1)
    expect(FAST_WIND_KMS).toBe(found[0])
  })

  it('is ordinary just below the threshold and fast from it', () => {
    expect(windSpeedImpact(FAST_WIND_KMS - 1).sev).toBe('quiet')
    expect(windSpeedImpact(FAST_WIND_KMS).sev).toBe('warn')
    expect(windSpeedImpact(900).sev).toBe('warn')
  })

  it('words the two states differently, and never blank', () => {
    const slow = windSpeedImpact(400).text
    const fast = windSpeedImpact(700).text
    expect(slow.length).toBeGreaterThan(0)
    expect(fast.length).toBeGreaterThan(0)
    expect(slow).not.toBe(fast)
  })
})
