// The trace peak-hold each scope takes when its caller passes none — pinned by VALUE, so a revert of
// the phone default (back to the shared `normal` 250 ms) fails here rather than passing silently.
import { describe, it, expect } from 'vitest'
import { defaultTraceHoldMs } from './PhoneScope'
import { TRACE_HOLD_MS, traceHoldDecay } from '../waterfall'

describe('defaultTraceHoldMs', () => {
  it('gives the phone scope its own 150 ms hold', () => {
    expect(defaultTraceHoldMs('phone')).toBe(150)
  })

  it('leaves the RF pane, and a scope with no cockpit, on the normal preset', () => {
    expect(defaultTraceHoldMs('rfpan')).toBe(TRACE_HOLD_MS.normal)
    expect(defaultTraceHoldMs(undefined)).toBe(TRACE_HOLD_MS.normal)
  })

  it('sits between the CW and normal presets, and does not strobe at the 30 Hz poll', () => {
    const phone = defaultTraceHoldMs('phone')
    expect(phone).toBeGreaterThan(TRACE_HOLD_MS.fast)
    expect(phone).toBeLessThan(TRACE_HOLD_MS.normal)
    // One 33 ms row gap must keep most of the trace (the anti-strobe bound the presets share).
    expect(traceHoldDecay(33, phone)).toBeGreaterThan(0.5)
  })
})
