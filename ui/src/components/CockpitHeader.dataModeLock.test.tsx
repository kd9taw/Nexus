// @vitest-environment jsdom
//
// The red readout names the rule when the data MODE is the whole licence lock (`dataModeRefusal`):
// Q65-30E or Q65-15D wider than 2.8 kHz below 28.3 MHz, MSK144 faster than 300 baud on 2200 m and
// 630 m, or MSK144's signal outside the licence at its fixed 1500 Hz centre while the TX marker is
// inside. The frequency would key for another mode, so the red readout alone pointed at the wrong
// remedy. Every other lock keeps the red readout and its tuning hint, as before.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import type { AppSnapshot } from '../types'

afterEach(cleanup)

const readoutTitle = (radio: Record<string, unknown>, digitTune = false) => {
  const snap = {
    activeRadioId: 0,
    radio: {
      dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
      txEnabled: true, tuning: false, txAllowed: false, txBusyReason: null, rigKeyed: false,
      ...radio,
    },
  } as unknown as AppSnapshot
  const { container } = render(
    <CockpitHeader snap={snap} modeIndicator={<span>Q65</span>} bandControl={<span>—</span>}
      digitTune={digitTune} />,
  )
  return container.querySelector('.ch-readout')?.getAttribute('title') ?? undefined
}

describe('the readout names a data-mode licence lock', () => {
  it('names a Q65 submode wider than 2.8 kHz', () => {
    expect(readoutTitle({ dataModeRefusal: { rule: 'tooWide', mode: 'Q65-30E' } })).toBe(
      'Q65-30E is wider than 2.8 kHz, and below 28.3 MHz Nexus holds a US license’s data to that — transmit is blocked. Pick a narrower Q65 submode in Settings.',
    )
  })

  it('names MSK144 over 300 baud on 2200 m and 630 m', () => {
    expect(readoutTitle({ dialMhz: 0.4745, band: '630m', dataModeRefusal: { rule: 'tooFast', mode: 'MSK144-15' } })).toBe(
      'MSK144-15 keys at 2000 baud, and US data on 2200 m and 630 m may not exceed 300 baud — transmit is blocked. Pick another mode.',
    )
  })

  it('names MSK144 transmitting at 1500 Hz whatever the marker says', () => {
    expect(readoutTitle({ dialMhz: 50.0965, band: '6m', dataModeRefusal: { rule: 'fixedCentre', mode: 'MSK144-15' } }, true)).toBe(
      'MSK144-15 always transmits 1500 Hz above the dial, whatever the TX marker says, and its signal there is outside your license privileges — transmit is blocked. Move the dial further into the segment.',
    )
  })

  it('keeps the tuning hint for every other lock', () => {
    expect(readoutTitle({}, true)).toBe('Scroll a digit to tune it')
    expect(readoutTitle({ txAllowed: true })).toBeUndefined()
  })
})
