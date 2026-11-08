// @vitest-environment jsdom
//
// #485: PEAK HOLD ON THE RECEIVE S-METER. The TX needles already hold a peak (TxNeedles'
// `peakStep` / `peakAt`: a fast attack, a 1 s hold, then a fall); the S-meter gets the same
// behaviour from the same two functions, drawn as one marked segment past the lit ones.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { SMeter, sFrac } from './SMeter'
import type { RadioStatus } from '../types'

const SEGMENTS = 28
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(1_000_000)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const radio = (over: Record<string, unknown>) => ({ ...over }) as unknown as RadioStatus
const segs = () => [...document.querySelectorAll('.ph-smeter-seg')]
const peakIdx = () => segs().findIndex((s) => s.classList.contains('peak'))
const lastLit = () => segs().map((s) => s.classList.contains('lit')).lastIndexOf(true)
/** The segment a reading's peak belongs on: the last one it lights. */
const segFor = (db: number) => Math.floor(sFrac(db) * SEGMENTS - 0.5)

describe('the S-meter holds its peak (#485)', () => {
  it('a burst to S9+20 stays marked after the reading falls back', () => {
    const { rerender } = render(<SMeter radio={radio({ smeterDb: -12 })} />)
    rerender(<SMeter radio={radio({ smeterDb: 20 })} />)
    vi.advanceTimersByTime(300)
    rerender(<SMeter radio={radio({ smeterDb: -30 })} />)
    expect(peakIdx()).toBe(segFor(20))
    expect(peakIdx()).toBeGreaterThan(lastLit())
  })

  it('then falls back once the hold is over', () => {
    const { rerender } = render(<SMeter radio={radio({ smeterDb: 20 })} />)
    vi.advanceTimersByTime(5000)
    rerender(<SMeter radio={radio({ smeterDb: -30 })} />)
    expect(peakIdx() === -1 || peakIdx() <= lastLit()).toBe(true)
  })

  it('CONTROL: a steady reading draws no separate marker', () => {
    const { rerender } = render(<SMeter radio={radio({ smeterDb: -12 })} />)
    rerender(<SMeter radio={radio({ smeterDb: -12 })} />)
    expect(peakIdx()).toBe(-1)
  })

  it('no marker while keyed or with no reading: the meter is paused, not measuring', () => {
    const { rerender } = render(<SMeter radio={radio({ smeterDb: 20 })} />)
    rerender(<SMeter radio={radio({ smeterDb: 20, rigKeyed: true })} />)
    expect(peakIdx()).toBe(-1)
    rerender(<SMeter radio={radio({ smeterDb: null })} />)
    expect(peakIdx()).toBe(-1)
  })
})
