// @vitest-environment jsdom
// Tuning from the band map (#39, kr4fqg).
//
// The map was always a frequency scale — `yOf` places every tick — but you could not act on a
// position on it, while the readout digits and the waterfall both tuned by wheel. That is what
// made it read as an inconsistency rather than a missing feature.
//
// The failure worth guarding is not "does it tune": it is tuning to the WRONG frequency, and
// specifically tuning when the operator meant to work a spot. A spot's LABEL is deliberately
// de-collided away from its true frequency so a dense cluster stays readable, so a click that
// lands on a label and is treated as a position would QSY to a frequency that is visibly not
// the one on the label. Every case below is paired with the state that must not happen.
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, fireEvent, within, cleanup } from '@testing-library/react'
import { BandMap } from './BandMap'
import type { SpotRow } from '../types'

const setFrequency = vi.fn(() => Promise.resolve(null))
vi.mock('../api', () => ({
  setFrequency: (...a: unknown[]) => setFrequency(...(a as [])),
}))

const spots: SpotRow[] = [
  {
    call: 'G0ABC',
    band: '20m',
    mode: 'Phone',
    freqMhz: 14.2,
    spotter: 'K1AAA',
    ageSecs: 10,
    comment: '',
  } as SpotRow,
]

/** The track reports a real box; jsdom gives everything zero height otherwise, and a zero-height
 *  track is the one case the handler must refuse rather than divide by.
 *
 *  Scoped to the render's OWN container, never `document`: without that this reached the first
 *  band map in the document, which on the second test onward was a previous render's — one that
 *  still had tuning enabled and its handlers live. Two cases passed alone and failed in the
 *  suite, which is the signature of exactly that. */
function sizeTrack(container: HTMLElement, height = 400, top = 0) {
  const track = container.querySelector('.bandmap-track') as HTMLElement
  vi.spyOn(track, 'getBoundingClientRect').mockReturnValue({
    top,
    height,
    left: 0,
    width: 200,
    bottom: top + height,
    right: 200,
    x: 0,
    y: top,
    toJSON: () => ({}),
  } as DOMRect)
  return track
}

const base = {
  band: '20m',
  dialMhz: 14.2,
  txAllowed: true,
  spots,
  spotMode: 'Phone' as const,
  onWorkSpot: vi.fn(),
}

beforeEach(() => {
  cleanup() // belt and braces: no previous render's handlers left in the document
  setFrequency.mockClear()
  base.onWorkSpot.mockClear()
})

describe('click to tune', () => {
  it('tunes to the frequency under the pointer', () => {
    const { container } = render(<BandMap {...base} sideband="USB" tuneEnabled />)
    const track = sizeTrack(container)
    // A quarter of the way down from the top. The top of the track is the HIGH edge.
    fireEvent.click(track, { clientY: 100 })
    expect(setFrequency).toHaveBeenCalledTimes(1)
    const [mhz, band, sideband] = setFrequency.mock.calls[0] as unknown as [number, string, string]
    expect(band).toBe('20m')
    expect(sideband, 'a map tune must never flip the mode').toBe('USB')
    // Exact value depends on the zoom window, but it must be inside the band and in the upper
    // half — a handler that inverted the axis would land in the lower half and still "pass" a
    // looser assertion.
    expect(mhz).toBeGreaterThan(14.0)
    expect(mhz).toBeLessThan(14.35)
  })

  it('clicking higher tunes HIGHER — the axis is not inverted', () => {
    const { container } = render(<BandMap {...base} sideband="USB" tuneEnabled />)
    const track = sizeTrack(container)
    fireEvent.click(track, { clientY: 40 })
    const high = (setFrequency.mock.calls[0] as unknown as [number])[0]
    setFrequency.mockClear()
    fireEvent.click(track, { clientY: 360 })
    const low = (setFrequency.mock.calls[0] as unknown as [number])[0]
    expect(high, 'top of the track is the high edge').toBeGreaterThan(low)
  })

  it('does NOT tune when the click lands on a spot — that works the station', () => {
    const { container } = render(<BandMap {...base} sideband="USB" tuneEnabled />)
    sizeTrack(container)
    const spot = within(container).getByRole('button', { name: /G0ABC/ })
    fireEvent.click(spot, { clientY: 200 })
    expect(base.onWorkSpot, 'clicking a spot works it').toHaveBeenCalledTimes(1)
    // The label is de-collided away from the true frequency, so tuning to the CLICK position
    // would QSY to a frequency that is visibly not the one on the label.
    expect(setFrequency, 'a spot click must not also tune to the label position').not.toHaveBeenCalled()
  })
})

describe('when tuning is not available', () => {
  it('a read-only map ignores clicks entirely', () => {
    const { container } = render(<BandMap {...base} />)
    const track = sizeTrack(container)
    fireEvent.click(track, { clientY: 100 })
    expect(setFrequency).not.toHaveBeenCalled()
  })

  it('says nothing about tuning in its cursor or its tooltip', () => {
    const { container } = render(<BandMap {...base} />)
    const track = container.querySelector('.bandmap-track')!
    expect(track.className, 'a crosshair over a dead scale invites a click that does nothing')
      .not.toContain('tunable')
    expect(track.getAttribute('title')).not.toMatch(/click to tune/i)
  })

  it('offers the affordance once tuning is live', () => {
    const { container } = render(<BandMap {...base} tuneEnabled />)
    const track = container.querySelector('.bandmap-track')!
    expect(track.className).toContain('tunable')
    expect(track.getAttribute('title')).toMatch(/click to tune/i)
  })

  it('refuses a zero-height track rather than dividing by it', () => {
    const { container } = render(<BandMap {...base} tuneEnabled />)
    const track = sizeTrack(container, 0)
    fireEvent.click(track, { clientY: 0 })
    expect(setFrequency).not.toHaveBeenCalled()
  })
})
