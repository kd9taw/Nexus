// @vitest-environment jsdom
//
// #334, part B: "switch back to radio 1 → it has changed to 80m custom". With two radios, the one
// not in use is polled over CAT, and a rig running FT8 answers in its DATA submode — Hamlib's
// PKTUSB (PKTFM on an FM data channel). Switching back adopts that live mode, so the snapshot's
// sideband read "PKTUSB" against a band-plan channel that says "USB", the exact-string match
// failed, and a radio sitting on its FT8 channel showed "80m (custom)". The channel match now
// compares the SIDE — a data submode is on the side of the mode it modulates — and still demands
// the exact dial, so CW, AM or LSB on an FT8 dial is still custom.
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import type { BandChannel } from '../types'
import { FrequencyControl } from './FrequencyControl'

beforeAll(() => {
  // Radix Popper observes its elements with a ResizeObserver jsdom lacks.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const channels: BandChannel[] = [
  { band: '80m', dialMhz: 3.573, group: 'HF', mode: 'USB', label: '80 m · FT8', note: '', tx: true },
  { band: '2m', dialMhz: 144.174, group: 'VHF', mode: 'USB', label: '2 m · FT8', note: '', tx: true },
  { band: '2m-fm', dialMhz: 145.56, group: 'VHF', mode: 'FM', label: '2 m · FM simplex (HT)', note: '', tx: true },
]

/** What the band control says it is on, for a radio reporting `mode` at `dialMhz`. */
function shown(dialMhz: number, band: string, mode: string): string {
  render(
    <FrequencyControl channels={channels} dialMhz={dialMhz} band={band} mode={mode} showReadout={false} showModeToggle={false} onSet={() => {}} />,
  )
  const text = screen.getByRole('button', { name: /channel/i }).textContent ?? ''
  cleanup()
  return text
}

describe('the band control finds the channel a radio in its data submode is on (#334)', () => {
  it('a rig answering PKTUSB on its FT8 dial is on that channel, not "custom"', () => {
    expect(shown(3.573, '80m', 'PKTUSB')).toContain('80 m · FT8')
    expect(shown(144.174, '2m', 'PKTUSB')).toContain('2 m · FT8')
  })

  it('PKTFM on an FM channel is that channel', () => {
    expect(shown(145.56, '2m', 'PKTFM')).toContain('2 m · FM simplex (HT)')
  })

  it('plain USB still matches — the control', () => {
    expect(shown(3.573, '80m', 'USB')).toContain('80 m · FT8')
  })

  it('still needs the exact dial', () => {
    expect(shown(3.575, '80m', 'PKTUSB')).toContain('80m (custom)')
  })

  it('a different mode on the FT8 dial is still custom: CW, AM, LSB, and FM on a USB channel', () => {
    for (const mode of ['CW', 'AM', 'LSB', 'PKTLSB']) {
      expect(shown(3.573, '80m', mode), mode).toContain('80m (custom)')
    }
    expect(shown(144.174, '2m', 'FM'), 'FM on the 2 m FT8 dial').toContain('2m (custom)')
  })
})
