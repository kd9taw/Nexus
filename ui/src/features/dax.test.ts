import { describe, it, expect } from 'vitest'
import { findDaxDevices, flexDaxAudioMissing, isDaxPaired } from './dax'

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

  it('prefers the live bare "DAX TX" endpoint over "DAX Audio TX" when both exist (real-6400M truth)', () => {
    const pair = findDaxDevices(
      ['DAX Audio RX 1 (FlexRadio DAX)'],
      ['DAX Audio TX (FlexRadio DAX)', 'DAX TX (FlexRadio DAX)'],
    )
    expect(pair?.output).toBe('DAX TX (FlexRadio DAX)')
    // Older installs with only the classic name still pair.
    expect(findDaxDevices(['DAX Audio RX 1'], ['DAX Audio TX'])?.output).toBe('DAX Audio TX')
  })

  it('isDaxPaired: any manual DAX-on-both-sides choice counts (never fight the operator)', () => {
    expect(isDaxPaired('DAX Audio RX 1', 'DAX TX (FlexRadio DAX)')).toBe(true)
    expect(isDaxPaired('DAX Audio RX 1', 'Speakers')).toBe(false)
    expect(isDaxPaired('', 'DAX TX')).toBe(false)
  })

  it('returns null when either side is missing (no half-pairing)', () => {
    expect(findDaxDevices(['DAX Audio RX 1'], ['Speakers'])).toBeNull()
    expect(findDaxDevices(['Microphone'], ['DAX Audio TX'])).toBeNull()
    expect(findDaxDevices([], [])).toBeNull()
  })
})

// #522: with Nexus's own Flex client on, the radio's native DAX audio is a separate switch that is
// off by default, so the waterfall hears whatever sound card Settings picks (a laptop's mic).
describe('flexDaxAudioMissing', () => {
  const reporter = {
    rigModel: 2036,
    rigConn: 'network',
    flexRadioIp: '192.0.2.10',
    flexNativeCat: true,
    flexNativeAudio: false,
    audioIn: 'Microphone Array (Realtek)',
  }

  it("is true for the reporter's setup: the client on, native audio off, a microphone picked", () => {
    expect(flexDaxAudioMissing(reporter)).toBe(true)
    expect(flexDaxAudioMissing({ ...reporter, rigModel: 23005 })).toBe(true)
    expect(flexDaxAudioMissing({ ...reporter, rigConn: 'serial' })).toBe(true)
    // System default is no DAX device either.
    expect(flexDaxAudioMissing({ ...reporter, audioIn: '' })).toBe(true)
  })

  it('is false once the audio comes from the radio: native audio on, or a DAX input picked', () => {
    expect(flexDaxAudioMissing({ ...reporter, flexNativeAudio: true })).toBe(false)
    expect(flexDaxAudioMissing({ ...reporter, audioIn: 'DAX Audio RX 1 (FlexRadio DAX)' })).toBe(false)
  })

  it('is false where the Flex client does not run', () => {
    expect(flexDaxAudioMissing({ ...reporter, flexNativeCat: false })).toBe(false)
    expect(flexDaxAudioMissing({ ...reporter, flexRadioIp: ' ' })).toBe(false)
    expect(flexDaxAudioMissing({ ...reporter, rigModel: 1035 })).toBe(false)
    for (const rigConn of ['omnirig', 'icomlan', 'tci', 'OmniRig']) {
      expect(flexDaxAudioMissing({ ...reporter, rigConn }), rigConn).toBe(false)
    }
  })
})
