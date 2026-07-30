import { describe, expect, it } from 'vitest'
import type { ProgChannel } from '../types'
import {
  autoRadiusMi,
  bandOfMhz,
  deriveNames,
  favoriteName,
  freqTail,
  repeaterMemory,
  rigRepeaterParams,
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

// ── the rig path: shift / offset / tone, and the Memory a machine becomes ──────

const chan = (over: Partial<ProgChannel> = {}): ProgChannel => ({
  id: 'rb:55-1',
  name: 'W9ABC',
  rxMhz: 146.94,
  duplex: 'minus',
  offsetMhz: 0.6,
  toneMode: 'tone',
  rtoneHz: 103.5,
  ctoneHz: 103.5,
  dtcsCode: 23,
  mode: 'fm',
  comment: 'Janesville',
  source: { source: 'repeaterbook', sourceId: '55-1', callsign: 'W9ABC' },
  ...over,
})

describe('rigRepeaterParams', () => {
  it('passes a conventional shift through with its EXACT magnitude in Hz', () => {
    expect(rigRepeaterParams(chan())).toEqual({ shift: 'minus', offsetHz: 600_000, toneHz: 103.5 })
    // An odd split on 2 m: 1 MHz, NOT the 600 kHz band convention. Rounding to the
    // nearest Hz matters — 0.6 * 1e6 is 600000.0000000001 in binary floating point.
    expect(rigRepeaterParams(chan({ duplex: 'plus', offsetMhz: 1.0, rxMhz: 145.11 }))).toEqual({
      shift: 'plus',
      offsetHz: 1_000_000,
      toneHz: 103.5,
    })
  })

  it('derives direction and magnitude for a split, which stores the ABSOLUTE TX', () => {
    // memchan.rs puts the absolute input in offsetMhz when duplex is split.
    expect(rigRepeaterParams(chan({ duplex: 'split', offsetMhz: 445.5 }))).toEqual({
      shift: 'plus',
      offsetHz: 298_560_000,
      toneHz: 103.5,
    })
    expect(rigRepeaterParams(chan({ duplex: 'split', rxMhz: 445.5, offsetMhz: 146.94 }))).toMatchObject({
      shift: 'minus',
      offsetHz: 298_560_000,
    })
  })

  it('sends a tone only for the tone modes that transmit one', () => {
    expect(rigRepeaterParams(chan({ toneMode: 'tsql' })).toneHz).toBe(103.5)
    expect(rigRepeaterParams(chan({ toneMode: 'none' })).toneHz).toBe(0)
    // DCS is a different squelch scheme — sending its code as a CTCSS frequency
    // would key a tone the machine isn't listening for.
    expect(rigRepeaterParams(chan({ toneMode: 'dtcs' })).toneHz).toBe(0)
  })

  it('zeroes the offset for simplex whatever offsetMhz happens to hold', () => {
    expect(rigRepeaterParams(chan({ duplex: 'simplex', rxMhz: 146.52 }))).toMatchObject({
      shift: 'simplex',
      offsetHz: 0,
    })
  })
})

describe('favoriteName', () => {
  it('is what operators say out loud — call plus the frequency nickname', () => {
    expect(favoriteName(chan())).toBe('W9ABC 94')
    // A club's two machines stay apart in the cockpit strip.
    expect(favoriteName(chan({ rxMhz: 442.725, id: 'rb:55-2' }))).toBe('W9ABC 725')
    // /R directory suffixes aren't how the machine is referred to.
    expect(
      favoriteName(chan({ source: { source: 'hearham', sourceId: '9', callsign: 'WB9COW/R' } })),
    ).toBe('WB9COW 94')
  })

  it('falls back to the record name when the directory has no callsign', () => {
    expect(favoriteName(chan({ name: 'Janesville', source: null }))).toBe('Janesville')
  })
})

describe('repeaterMemory', () => {
  it('carries the shift, tone and site so a favorite can retune AND be located', () => {
    const m = repeaterMemory(chan(), 'W9ABC 94', { lat: 42.68, lon: -89.02 })
    expect(m).toMatchObject({
      name: 'W9ABC 94',
      rxMhz: 146.94,
      mode: 'FM',
      kind: 'repeater',
      offsetDir: 'minus',
      offsetMhz: 0.6,
      toneMode: 'tone',
      ctcssEncHz: 103.5,
      callsign: 'W9ABC',
      lat: 42.68,
      lon: -89.02,
      source: 'program',
    })
  })

  it('classifies a toneless simplex channel as simplex, not a repeater', () => {
    const m = repeaterMemory(chan({ duplex: 'simplex', toneMode: 'none', rxMhz: 146.52 }), 'CALL')
    expect(m.kind).toBe('simplex')
    expect(m.offsetDir).toBe('simplex')
    expect(m.offsetMhz).toBeUndefined()
    expect(m.toneMode).toBe('none')
    expect(m.ctcssEncHz).toBeUndefined()
  })

  it('is a repeater when it has a tone even on a simplex shift (tone-access machine)', () => {
    expect(repeaterMemory(chan({ duplex: 'simplex' }), 'CALL').kind).toBe('repeater')
  })

  it('leaves the site undefined when the caller has no record (a reloaded list)', () => {
    const m = repeaterMemory(chan(), 'W9ABC 94')
    expect(m.lat).toBeUndefined()
    expect(m.lon).toBeUndefined()
  })
})
