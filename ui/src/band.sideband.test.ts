// #45 (patpell, FT-710): "I tried to select 20m SSTV. In the menu, it stated 14.230 USB, but it
// tuned the radio to 14.230 LSB." Transmitting on the wrong sideband is as bad as transmitting on
// the wrong frequency — nobody hears you and you do not know why.
import { describe, it, expect } from 'vitest'
import { sidebandForMhz, sidebandForQsy } from './band'

describe('the sideband convention', () => {
  it('is LSB below 10 MHz and USB at or above', () => {
    expect(sidebandForMhz(7.2)).toBe('LSB')
    expect(sidebandForMhz(3.75)).toBe('LSB')
    expect(sidebandForMhz(14.23)).toBe('USB')
    expect(sidebandForMhz(10.0)).toBe('USB') // the boundary itself is USB
    expect(sidebandForMhz(50.125)).toBe('USB')
  })
})

describe('choosing a sideband for a QSY', () => {
  it('corrects when the move crosses the boundary — the reported bug', () => {
    // 40 m (LSB) to the 20 m SSTV calling frequency.
    expect(sidebandForQsy(14.23, 7.171, 'LSB')).toBe('USB')
    // ...and the other way.
    expect(sidebandForQsy(7.171, 14.23, 'USB')).toBe('LSB')
  })

  it('KEEPS a deliberate sideband when the move stays on one side', () => {
    // An operator on USB down on 40 m has chosen that; an ordinary retune must not revert it.
    expect(sidebandForQsy(7.074, 7.171, 'USB')).toBe('USB')
    expect(sidebandForQsy(3.573, 7.171, 'LSB')).toBe('LSB')
    expect(sidebandForQsy(21.074, 14.074, 'USB')).toBe('USB')
  })

  it('passes a non-sideband mode straight through', () => {
    // FM, AM and the DATA submodes are a class, not a side. This convention does not govern
    // them, and rewriting one to USB would command the rig out of the mode it is working.
    expect(sidebandForQsy(14.23, 7.171, 'FM')).toBe('FM')
    expect(sidebandForQsy(14.23, 7.171, 'PKTUSB')).toBe('PKTUSB')
    expect(sidebandForQsy(14.23, 7.171, 'AM')).toBe('AM')
  })

  it('falls back to the convention when nothing is known', () => {
    expect(sidebandForQsy(14.23, 7.171, '')).toBe('USB')
    expect(sidebandForQsy(7.171, 14.23, null)).toBe('LSB')
  })
})
