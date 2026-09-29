// THE BASE CALL, THE ENGINE'S WAY. `tempo_core::message::base_call` decides whether a logged
// contact is the hunted activator (`same_call`, the park auto-tag), so every place the UI asks "is
// this the same station?" has to give its answer. Three hand-rolled splits gave three other
// answers: `.split('/').pop()` made `KE7G/P` into `P`, so a portable activator never matched and
// any two `/P` stations did; `.split('/')[0]` made `VE7/KE7G` into `VE7`.
//
// The first block is the Rust unit test `base_call_strips_portable_affixes`, case for case.
import { describe, expect, it } from 'vitest'
import { baseCall, sameCall } from './callsign'

describe('baseCall', () => {
  it('matches the engine’s own cases', () => {
    expect(baseCall('W9XYZ')).toBe('W9XYZ')
    expect(baseCall('w9xyz')).toBe('W9XYZ')
    expect(baseCall('W9XYZ/P')).toBe('W9XYZ')
    expect(baseCall('W9XYZ/4')).toBe('W9XYZ')
    expect(baseCall('KD9TAW/MM')).toBe('KD9TAW')
    expect(baseCall('KH8/W1AW')).toBe('W1AW')
    expect(baseCall('VP2E/AA9A')).toBe('AA9A')
  })

  it('takes the call from every prefix and suffix form', () => {
    expect(baseCall('KE7G/P')).toBe('KE7G')
    expect(baseCall('KE7G/7')).toBe('KE7G')
    expect(baseCall('VE7/W7ABC')).toBe('W7ABC')
    expect(baseCall('W7ABC/P')).toBe('W7ABC')
    expect(baseCall('W7ABC/QRP')).toBe('W7ABC')
    expect(baseCall('DL/W7ABC/P')).toBe('W7ABC')
    expect(baseCall(' ke7g/p ')).toBe('KE7G')
  })

  it('unwraps an FT hashed call, as the engine does', () => {
    expect(baseCall('<W9XYZ>')).toBe('W9XYZ')
    expect(baseCall('<KH8/W1AW>')).toBe('W1AW')
  })

  it('falls back to the longest segment, the last of equals, when none looks like a call', () => {
    expect(baseCall('ABC/DE')).toBe('ABC')
    expect(baseCall('AB/CD')).toBe('CD')
    expect(baseCall('')).toBe('')
  })
})

describe('sameCall', () => {
  it('is one station across a portable affix and case', () => {
    expect(sameCall('KD9TAW', 'kd9taw/p')).toBe(true)
    expect(sameCall('KE7G/P', 'KE7G')).toBe(true)
    expect(sameCall('VE7/KE7G', 'KE7G')).toBe(true)
  })

  // THE CONTROL: an affix is not an identity. Two portable stations are two stations.
  it('is two stations when only the affix is shared', () => {
    expect(sameCall('KE7G/P', 'KF7XYZ/P')).toBe(false)
    expect(sameCall('VE7/KE7G', 'VE7/W7ABC')).toBe(false)
    expect(sameCall('KD9TAW', 'KD9TAX')).toBe(false)
  })
})
