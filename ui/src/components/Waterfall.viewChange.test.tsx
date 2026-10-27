// @vitest-environment jsdom
//
// THE STRIP TELLS ITS HOST WHICH AUDIO WINDOW IT SHOWS (`onViewChange`). JTTY's decoder searches
// its fixed channels only inside the range WSJT-X's wide graph shows, so the JTTY cockpit hands
// this window to the engine: once when the strip mounts, and again whenever it moves — a zoom
// pick, or the RX marker paging a zoomed view. Not on a re-render that moves nothing.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { Waterfall } from './Waterfall'
import { t } from '../i18n'

vi.mock('../api', () => ({
  getSpectrumRow: () => Promise.resolve({ row: [], loHz: 200, hiHz: 4000 }),
}))

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const strip = (rx: number, onViewChange: (lo: number, hi: number) => void) => (
  <Waterfall transmitting={false} rxOffsetHz={rx} txOffsetHz={0} theme="dark" onViewChange={onViewChange} />
)

describe('the waterfall reports the audio window it shows', () => {
  it('on mount, after a zoom pick, and when the RX marker pages a zoomed view — not otherwise', () => {
    const seen = vi.fn()
    const { rerender } = render(strip(1500, seen))
    // The stock view: 200 Hz to 3 kHz.
    expect(seen.mock.calls).toEqual([[200, 3000]])
    rerender(strip(1500, seen))
    expect(seen, 'a re-render that moves nothing tells nothing').toHaveBeenCalledTimes(1)

    fireEvent.change(screen.getByRole('combobox', { name: t('waterfall.zoom.aria') }), { target: { value: '600' } })
    expect(seen).toHaveBeenLastCalledWith(1200, 1800)

    // Rx inside the window: the slice holds still. Rx outside it: the window pages over it.
    rerender(strip(1400, seen))
    expect(seen).toHaveBeenCalledTimes(2)
    rerender(strip(2500, seen))
    expect(seen).toHaveBeenCalledTimes(3)
    const [lo, hi] = seen.mock.calls[2]
    expect(hi - lo).toBe(600)
    expect(lo <= 2500 && 2500 <= hi).toBe(true)
  })
})
