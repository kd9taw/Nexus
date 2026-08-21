// The CI-V model list must be the ENGINE's list, not a second opinion.
//
// The screen used to decide whether to offer native CI-V by pattern-matching the model NAME
// (`/IC-?\s?(7300|7610|9700|705|905)\b/i`) while the engine decided by model NUMBER. A radio
// stored as `Icom 7610`, `IC-7610M`, or with an empty model name passed the engine's test and
// failed the screen's, so the toggle disappeared for a radio Nexus fully supports — reported by
// an IC-7610 operator, 2026-08-19.
//
// This reads the Rust source and fails if the two lists drift, which is the only way a mirrored
// constant stays true.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { NATIVE_CIV_MODELS, nativeCivBlockedReason } from './rigFormChecks'

const RUST = fileURLToPath(
  new URL('../../crates/tempo-audio/src/rigmodels.rs', import.meta.url),
)

describe('native CI-V model list', () => {
  it('matches icom_scope_model in the engine', () => {
    const src = readFileSync(RUST, 'utf8')
    const fn = src.slice(src.indexOf('fn icom_scope_model'))
    const body = fn.slice(0, fn.indexOf('\n}'))
    const models = [...body.matchAll(/^\s*(\d+)\s*=>/gm)].map((m) => Number(m[1])).sort((a, b) => a - b)
    expect(models.length, 'control: the Rust arm list was actually parsed').toBeGreaterThan(3)
    expect([...NATIVE_CIV_MODELS].sort((a, b) => a - b)).toEqual(models)
  })

  it('answers WHY, so the screen can say it instead of hiding the control', () => {
    expect(nativeCivBlockedReason(3078, 'serial')).toBeNull() // IC-7610 on USB — offered
    expect(nativeCivBlockedReason(3078, 'network')).toBe('network')
    expect(nativeCivBlockedReason(3078, 'omnirig')).toBe('omnirig')
    expect(nativeCivBlockedReason(1042, 'serial')).toBe('not-supported') // a Yaesu
    // The shapes that broke the old name-matching gate are all fine now, because the model
    // number does not care what the name says.
    expect(nativeCivBlockedReason(3073, 'serial')).toBeNull()
    expect(nativeCivBlockedReason(3090, 'serial')).toBeNull()
  })
})
