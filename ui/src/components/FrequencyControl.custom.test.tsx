// @vitest-environment jsdom
//
// #334, #184: the band pill read "80m (custom)" and nothing on screen said what that meant. It
// means only that the dial is not on a listed channel for the mode; the pill now says so, on the
// pill (its title) and at the top of its menu.
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import type { BandChannel } from '../types'
import { FrequencyControl } from './FrequencyControl'
import { EN } from '../i18n/en'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const channels: BandChannel[] = [
  { band: '80m', dialMhz: 3.573, group: 'HF', mode: 'USB', label: '80 m · FT8', note: 'FT8 calling', tx: true },
]
const WHY = EN['freq.channel.custom.title']

function pill(dialMhz: number) {
  render(<FrequencyControl channels={channels} dialMhz={dialMhz} band="80m" mode="USB" showReadout={false} showModeToggle={false} onSet={() => {}} />)
  return screen.getByRole('button', { name: /channel/i })
}

describe('"(custom)" says what it means', () => {
  it('off a listed channel, the pill title explains it and the menu opens with the same line', () => {
    const b = pill(3.575)
    expect(b.textContent).toContain('80m (custom)')
    expect(WHY).toMatch(/not on a listed channel/)
    // #511/#454: "or type a frequency" sent an operator to type into this TOOLTIP. It now says
    // WHERE (the big MHz readout) and HOW (type, then Enter).
    expect(WHY).toMatch(/click the large MHz frequency/i)
    expect(WHY).toMatch(/press Enter/)
    expect(b.getAttribute('title')).toBe(WHY)
    fireEvent.pointerDown(b, { button: 0, ctrlKey: false })
    expect(screen.getByText(WHY)).toBeTruthy()
  })

  it('on a channel, the pill keeps the channel note and the menu has no such line', () => {
    const b = pill(3.573)
    expect(b.getAttribute('title')).toBe('FT8 calling')
    fireEvent.pointerDown(b, { button: 0, ctrlKey: false })
    expect(screen.queryByText(WHY)).toBeNull()
  })
})
