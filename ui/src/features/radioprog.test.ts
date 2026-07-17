import { describe, expect, it } from 'vitest'
import {
  autoRadiusMi,
  bandOfMhz,
  deriveNames,
  freqTail,
  sanitizeName,
} from './radioprog'

describe('freqTail', () => {
  it('drops trailing zeros from the kHz fraction', () => {
    expect(freqTail(146.94)).toBe('94')
    expect(freqTail(147.255)).toBe('255')
    expect(freqTail(442.725)).toBe('725')
    expect(freqTail(146.0)).toBe('0')
    expect(freqTail(443.4375)).toBe('438') // rounds to kHz
  })
})

describe('sanitizeName', () => {
  it('uppercases, strips, squeezes, caps', () => {
    expect(sanitizeName('w9abc', 7)).toBe('W9ABC')
    expect(sanitizeName('WB9COW/R  HUB', 12)).toBe('WB9COW/R HUB')
    expect(sanitizeName('Café—Tower', 8)).toBe('CAFTOWER')
  })
})

describe('deriveNames', () => {
  it('uses the bare callsign when unique and fitting', () => {
    const names = deriveNames(
      [
        { callsign: 'W9ABC', city: 'Janesville', outputMhz: 146.94 },
        { callsign: 'K9XYZ', city: 'Beloit', outputMhz: 147.255 },
      ],
      7,
    )
    expect(names).toEqual(['W9ABC', 'K9XYZ'])
  })

  it('resolves a club collision with the frequency nickname', () => {
    const names = deriveNames(
      [
        { callsign: 'W9ABC', city: 'Janesville', outputMhz: 146.94 },
        { callsign: 'W9ABC', city: 'Janesville', outputMhz: 442.725 },
      ],
      7,
    )
    expect(names[0]).toBe('W9AB 94')
    expect(names[1]).toBe('W9A 725')
    expect(new Set(names).size).toBe(2)
    expect(names.every((n) => n.length <= 7)).toBe(true)
  })

  it('strips /R suffixes before naming', () => {
    const names = deriveNames(
      [{ callsign: 'WB9COW/R', city: 'Burlington', outputMhz: 442.8375 }],
      7,
    )
    expect(names[0]).toBe('WB9COW')
  })

  it('falls back to squeezed city + tail when the callsign is blank', () => {
    const names = deriveNames(
      [{ callsign: '', city: 'Gatlinburg', outputMhz: 146.94 }],
      7,
    )
    expect(names[0]).toBe('GTLNB94')
    expect(names[0].length).toBeLessThanOrEqual(7)
  })

  it('suffixes true duplicates so radios never show two identical channels', () => {
    const names = deriveNames(
      [
        { callsign: 'W9ABC', city: 'A', outputMhz: 146.94 },
        { callsign: 'W9ABC', city: 'B', outputMhz: 146.94 },
      ],
      7,
    )
    expect(new Set(names).size).toBe(2)
  })

  it('honors wider caps without inventing tails', () => {
    const names = deriveNames(
      [{ callsign: 'KD9PPX', city: 'Spring Grove', outputMhz: 146.67 }],
      16,
    )
    expect(names[0]).toBe('KD9PPX')
  })
})

describe('bandOfMhz / autoRadiusMi', () => {
  it('classifies the chip bands', () => {
    expect(bandOfMhz(146.52)).toBe('2m')
    expect(bandOfMhz(442.725)).toBe('70cm')
    expect(bandOfMhz(52.525)).toBe('6m')
    expect(bandOfMhz(28.4)).toBe('10m')
    expect(bandOfMhz(223.5)).toBe('1.25m')
    expect(bandOfMhz(902.1)).toBe('')
  })

  it('auto radius takes the widest selected band', () => {
    expect(autoRadiusMi(['2m'])).toBe(50)
    expect(autoRadiusMi(['70cm'])).toBe(25)
    expect(autoRadiusMi(['2m', '70cm'])).toBe(50)
    expect(autoRadiusMi(['6m', '70cm'])).toBe(75)
    expect(autoRadiusMi([])).toBe(50)
  })
})
