// @vitest-environment jsdom
// Tuning from the docked band-activity strip (#96).
//
// The pop-out band map wheel-tunes (#39); the docked strip is the same frequency scale in the
// same cockpits — Phone and CW both render this one component — and stayed silent. The strip
// attaches the SAME `useWheelTune` the map, the readout digits and the waterfall use, so the
// step, the sensitivity, the coalescer and the band-edge handling are the ones the operator
// already knows. The hook's own plumbing is pinned in `useWheelTune.test.ts`; this pins the
// WIRING — that a wheel event on the strip's track actually issues the tune, with the
// configured step and direction, and that a strip not given tuning stays read-only.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { BandStrip } from './BandStrip'
import { setFrequency } from '../api'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)) }))

const mockSetFreq = setFrequency as unknown as ReturnType<typeof vi.fn>

const base = {
  band: '20m',
  dialMhz: 14.2,
  txAllowed: true,
  spots: [],
  onWorkSpot: vi.fn(),
}

function wheel(el: HTMLElement, init: WheelEventInit): WheelEvent {
  const e = new WheelEvent('wheel', { cancelable: true, bubbles: true, ...init })
  el.dispatchEvent(e)
  return e
}

beforeEach(() => {
  // Timers AND `performance` faked together — the hook measures burst idleness against
  // `performance.now()`, and mixing a fake setTimeout with the real wall clock splits one
  // intended burst in two on a slow run (see the note in useWheelTune.test.ts).
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
  mockSetFreq.mockClear()
})
afterEach(() => {
  vi.useRealTimers()
  cleanup()
})

describe('wheel-tune on the band strip (#96)', () => {
  it('wheel up on the track tunes UP by the configured step', () => {
    const { container } = render(
      <BandStrip {...base} sideband="USB" tuneEnabled stepHz={500} />,
    )
    const track = container.querySelector('.bandstrip-track') as HTMLElement
    wheel(track, { deltaY: -100 })
    expect(mockSetFreq).not.toHaveBeenCalled() // coalesced — nothing until the flush window
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    const [mhz, band, sb] = mockSetFreq.mock.calls[0] as [number, string, string]
    expect(mhz).toBeCloseTo(14.2005, 6) // +500 Hz — the operator's step, not a hardcoded one
    expect(band).toBe('20m')
    expect(sb, 'a strip tune must never flip the mode').toBe('USB')
  })

  it('wheel down tunes DOWN', () => {
    const { container } = render(
      <BandStrip {...base} sideband="USB" tuneEnabled stepHz={500} />,
    )
    const track = container.querySelector('.bandstrip-track') as HTMLElement
    wheel(track, { deltaY: 100 })
    vi.advanceTimersByTime(120)
    expect((mockSetFreq.mock.calls[0] as [number])[0]).toBeCloseTo(14.1995, 6) // -500 Hz
  })

  it('a strip not given tuning is read-only and leaves the page scroll alone', () => {
    const { container } = render(<BandStrip {...base} />)
    const track = container.querySelector('.bandstrip-track') as HTMLElement
    const e = wheel(track, { deltaY: -100 })
    vi.advanceTimersByTime(120)
    expect(mockSetFreq).not.toHaveBeenCalled()
    expect(e.defaultPrevented, 'page scroll must survive a read-only strip').toBe(false)
  })
})
