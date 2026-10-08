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
import { dataModePickerShown, MULTI_DATA_MODE_ICOMS, NATIVE_CIV_MODELS, nativeCivBlockedReason } from './rigFormChecks'

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

// THE D1/D2/D3 PICKER BELONGS TO THE RADIOS THAT HAVE D2 AND D3. Each radio's own CI-V reference, "Data mode
// with filter width settings" (`1A 06`), first data byte: the IC-7610 has DATA1, DATA2 and DATA3 (A7380-7EX-4,
// PDF p. 13); the IC-9700 (A7508-3EX-4, PDF p. 19), IC-705 (A7560-8EX-6, PDF p. 23), IC-905 (A7711-9EX-2,
// PDF p. 24) and IC-7300 (Full Manual A7292-4EX-12, PDF p. 168) have "Data mode ON" and nothing more. The
// picker was offered on the 9700, 705 and 905 too, where a D2 or D3 is a value Icom does not define.
describe('the D1/D2/D3 picker', () => {
  it('is offered on the IC-7610 alone, the one native Icom with more than one DATA mode', () => {
    expect([...MULTI_DATA_MODE_ICOMS]).toEqual([3078])
    expect(NATIVE_CIV_MODELS.filter((m) => dataModePickerShown(m, 1))).toEqual([3078])
    expect([1, 2, 3].every((d) => dataModePickerShown(3078, d)), 'the 7610 keeps it on every choice').toBe(true)
  })

  // The daemon still sends a saved D2 or D3, so hiding the picker with one in force would leave the operator no
  // way to put the radio back on D1. It stays until they do.
  it('stays on a one-DATA Icom only while a D2 or D3 saved there earlier is still in force', () => {
    for (const m of [3073, 3081, 3085, 3090]) {
      expect(dataModePickerShown(m, 1), `${m} on D1`).toBe(false)
      expect(dataModePickerShown(m, 2), `${m} on D2`).toBe(true)
      expect(dataModePickerShown(m, 3), `${m} on D3`).toBe(true)
    }
    expect(dataModePickerShown(1042, 2), 'never on a radio Nexus does not drive natively').toBe(false)
  })
})
