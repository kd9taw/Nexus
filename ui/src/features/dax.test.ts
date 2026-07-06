import { describe, it, expect } from 'vitest'
import { findDaxDevices } from './dax'

describe('findDaxDevices', () => {
  it('pairs DAX RX 1 with DAX TX, preferring RX 1 over other RX channels', () => {
    const pair = findDaxDevices(
      ['Microphone (USB)', 'DAX Audio RX 2 (FlexRadio)', 'DAX Audio RX 1 (FlexRadio)'],
      ['Speakers', 'DAX Audio TX (FlexRadio)'],
    )
    expect(pair).toEqual({ input: 'DAX Audio RX 1 (FlexRadio)', output: 'DAX Audio TX (FlexRadio)' })
  })

  it('falls back to any DAX device when RX 1 is absent', () => {
    const pair = findDaxDevices(['DAX RESERVED AUDIO RX 3'], ['DAX Audio TX'])
    expect(pair?.input).toBe('DAX RESERVED AUDIO RX 3')
  })

  it('returns null when either side is missing (no half-pairing)', () => {
    expect(findDaxDevices(['DAX Audio RX 1'], ['Speakers'])).toBeNull()
    expect(findDaxDevices(['Microphone'], ['DAX Audio TX'])).toBeNull()
    expect(findDaxDevices([], [])).toBeNull()
  })
})
