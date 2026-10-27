// @vitest-environment jsdom
//
// THE RTTY TUNING SCOPE — what an operator can do with it, and when they cannot.
//
// The nudges net through `rtty_net`, which moves the AFSK TRANSMIT tone as well as the decoder, so
// the load-bearing assertions are the keyed ones: a nudge is refused while keyed, and a hold that is
// already repeating stops the moment an over starts — before its next step can move the tone.
// Everything is asserted BY VALUE (the exact hertz asked for, the exact words shown).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import * as api from '../api'
import type { RttyScope } from '../types'
import {
  HOLD_DELAY_MS,
  HOLD_REPEAT_MS,
  NUDGE_HZ,
  POLL_MS,
  RttyTuningScope,
  STATUS_MS,
  tuningWords,
} from './RttyTuningScope'

vi.mock('../api', () => ({ getRttyScope: vi.fn() }))
const getRttyScope = api.getRttyScope as ReturnType<typeof vi.fn>

const frame = (offsetHz: number | null, live = true): RttyScope => ({
  live,
  seq: 1,
  peak: 0.4,
  xy: [127, 10, -127, -10],
  offsetHz,
})

type Props = Parameters<typeof RttyTuningScope>[0]
const base: Props = {
  active: true,
  armed: true,
  keyed: false,
  markHz: 2125,
  spaceHz: 2295,
  onNet: () => {},
}

beforeEach(() => {
  getRttyScope.mockReset().mockResolvedValue(frame(null))
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('the tuning words', () => {
  it('name the state, the offset and its direction by value', () => {
    expect(tuningWords(frame(-20), false, false)).toBe('Arm the decoder to tune')
    expect(tuningWords(frame(-20), true, true)).toBe('Paused while we send')
    expect(tuningWords(null, true, false, true)).toBe('Not on this link')
    expect(tuningWords(null, true, false)).toBe('No signal')
    expect(tuningWords(frame(null), true, false)).toBe('No signal')
    expect(tuningWords(frame(12, false), true, false)).toBe('No signal')
    expect(tuningWords(frame(4.6), true, false)).toBe('Tuned')
    expect(tuningWords(frame(-5), true, false)).toBe('Tuned')
    expect(tuningWords(frame(20.4), true, false)).toBe('Signal 20 Hz high')
    expect(tuningWords(frame(-12.6), true, false)).toBe('Signal 13 Hz low')
  })
})

describe('the picture has a text twin', () => {
  it('names the scope and its state, and says it in a status line', async () => {
    getRttyScope.mockResolvedValue(frame(-20))
    const { container } = render(<RttyTuningScope {...base} />)
    expect(await screen.findByRole('img', { name: 'Tuning scope: Signal 20 Hz low' })).toBeTruthy()
    expect(container.querySelector('[aria-live="polite"]')?.textContent).toBe('Signal 20 Hz low')
    expect(screen.getByText('M 2125')).toBeTruthy()
  })

  it('changes its words at once for a new kind of state, and a bare number at most once a second', async () => {
    vi.useFakeTimers()
    getRttyScope.mockResolvedValue(frame(-20))
    const { container } = render(<RttyTuningScope {...base} />)
    const words = () => container.querySelector('[aria-live="polite"]')?.textContent
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS)
    })
    expect(words()).toBe('Signal 20 Hz low')
    getRttyScope.mockResolvedValue({ ...frame(-14), seq: 2 })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2 * POLL_MS)
    })
    expect(words(), 'only the number moved: held').toBe('Signal 20 Hz low')
    await act(async () => {
      await vi.advanceTimersByTimeAsync(STATUS_MS)
    })
    expect(words()).toBe('Signal 14 Hz low')
    getRttyScope.mockResolvedValue({ ...frame(1), seq: 3 })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2 * POLL_MS)
    })
    expect(words(), 'a new kind: at once').toBe('Tuned')
  })

  it('carries its own ✕ when it has a head, named after the pane', () => {
    const onRemove = vi.fn()
    render(<RttyTuningScope {...base} closeProps={{ onRemove }} />)
    fireEvent.click(screen.getByRole('button', { name: 'Hide Tuning' }))
    expect(onRemove).toHaveBeenCalledTimes(1)
  })
})

describe('the nudges', () => {
  it('net the centre down and up by 5 Hz from the keyboard', () => {
    const onNet = vi.fn()
    render(<RttyTuningScope {...base} onNet={onNet} />)
    // A keyboard press is a click with no pointer (detail 0).
    fireEvent.click(screen.getByRole('button', { name: 'Tune down 5 Hz' }), { detail: 0 })
    fireEvent.click(screen.getByRole('button', { name: 'Tune up 5 Hz' }), { detail: 0 })
    expect(onNet.mock.calls).toEqual([[2210 - NUDGE_HZ], [2210 + NUDGE_HZ]])
  })

  it('repeat while held, each step from the last one asked for, and stop on release', () => {
    vi.useFakeTimers()
    const onNet = vi.fn()
    render(<RttyTuningScope {...base} onNet={onNet} />)
    const up = screen.getByRole('button', { name: 'Tune up 5 Hz' })
    fireEvent.pointerDown(up, { button: 0 })
    expect(onNet.mock.calls).toEqual([[2215]])
    act(() => vi.advanceTimersByTime(HOLD_DELAY_MS))
    act(() => vi.advanceTimersByTime(HOLD_REPEAT_MS))
    expect(onNet.mock.calls).toEqual([[2215], [2220], [2225]])
    fireEvent.pointerUp(up)
    act(() => vi.advanceTimersByTime(10 * HOLD_REPEAT_MS))
    expect(onNet).toHaveBeenCalledTimes(3)
  })

  it('are disabled while keyed and without an armed decoder', () => {
    const { rerender } = render(<RttyTuningScope {...base} keyed />)
    const buttons = () => [
      screen.getByRole('button', { name: 'Tune down 5 Hz' }),
      screen.getByRole('button', { name: 'Tune up 5 Hz' }),
    ]
    for (const b of buttons()) expect((b as HTMLButtonElement).disabled).toBe(true)
    rerender(<RttyTuningScope {...base} armed={false} />)
    for (const b of buttons()) expect((b as HTMLButtonElement).disabled).toBe(true)
    rerender(<RttyTuningScope {...base} />)
    for (const b of buttons()) expect((b as HTMLButtonElement).disabled).toBe(false)
  })

  it('stop a hold the instant an over starts, before the next step can move the tone', () => {
    vi.useFakeTimers()
    const onNet = vi.fn()
    // Not polling (`active` off), so the only timer is the hold's own.
    const props = { ...base, active: false, onNet }
    const { rerender } = render(<RttyTuningScope {...props} />)
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Tune down 5 Hz' }), { button: 0 })
    act(() => vi.advanceTimersByTime(HOLD_DELAY_MS))
    expect(onNet.mock.calls).toEqual([[2205], [2200]])
    // The button is still held (no pointerup) when the station keys.
    rerender(<RttyTuningScope {...props} keyed />)
    expect(vi.getTimerCount(), 'the repeat is still scheduled after the over started').toBe(0)
    act(() => vi.advanceTimersByTime(20 * HOLD_REPEAT_MS))
    expect(onNet).toHaveBeenCalledTimes(2)
  })
})

describe('the poll is its cost', () => {
  it('asks only while visible and armed', async () => {
    vi.useFakeTimers()
    const { rerender } = render(<RttyTuningScope {...base} active={false} />)
    act(() => vi.advanceTimersByTime(10 * POLL_MS))
    rerender(<RttyTuningScope {...base} armed={false} />)
    act(() => vi.advanceTimersByTime(10 * POLL_MS))
    expect(getRttyScope).not.toHaveBeenCalled()
    rerender(<RttyTuningScope {...base} />)
    await act(async () => {
      vi.advanceTimersByTime(3 * POLL_MS)
    })
    expect(getRttyScope.mock.calls.length).toBeGreaterThanOrEqual(1)
  })

  it('says so when the link carries no scope', async () => {
    getRttyScope.mockRejectedValue(new Error('nexusClient:unknown'))
    render(<RttyTuningScope {...base} />)
    expect(await screen.findByRole('img', { name: 'Tuning scope: Not on this link' })).toBeTruthy()
  })
})
