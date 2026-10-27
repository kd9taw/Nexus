// @vitest-environment jsdom
//
// A call nobody answers runs until the TX watchdog turns transmit off. The strip used to show
// the bare sequencer state ("AwaitReport") beside TX off, with nothing saying the call had been
// stopped or why. WSJT-X turns its TX status red and names the watchdog, and alerts its window;
// so does this strip, from the snapshot's `watchdogStopped`.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'

const api = vi.hoisted(() => ({ attention: 0 }))
vi.mock('../api', async (orig) => ({
  ...(await orig<typeof import('../api')>()),
  requestWindowAttention: vi.fn(async () => {
    api.attention += 1
  }),
}))

import { OperateQsoStrip } from './OperateQsoStrip'
import type { QsoStatus, RadioStatus } from '../types'

afterEach(() => {
  cleanup()
  api.attention = 0
})

const calling = {
  running: true,
  cqRunning: false,
  state: 'AwaitReport',
  dxcall: 'W9XYZ',
  txNow: 'W9XYZ K2DEF FN31',
  rxReport: null,
  txCount: 9,
} as unknown as QsoStatus
const stopped = { ...calling, watchdogStopped: true } as QsoStatus
const radio = { txEnabled: false, tuning: false, holdTxFreq: false } as unknown as RadioStatus

function el(qso: QsoStatus) {
  return (
    <OperateQsoStrip
      qso={qso}
      radio={radio}
      onSetMode={vi.fn()}
      onCallCq={vi.fn()}
      onResend={vi.fn()}
      onFreetext={vi.fn()}
      onLog={vi.fn()}
      onSetTxEnabled={vi.fn()}
      onSetTune={vi.fn()}
      onHaltTx={vi.fn()}
      onSetHoldTxFreq={vi.fn()}
    />
  )
}

/** The sequencer-state readout: the strip's `.cq-state` status. */
const stateEl = (c: HTMLElement) => c.querySelector('.cq-state') as HTMLElement

describe('OperateQsoStrip after the TX watchdog stops a call', () => {
  it('says the watchdog stopped the call, in place of the bare state, marked for red', () => {
    const { container } = render(el(stopped))
    const s = stateEl(container)
    expect(s.textContent).toBe('TX watchdog stopped the call')
    expect(s.classList.contains('watchdog')).toBe(true)
    expect(s.title).toBe(
      'The TX watchdog turned transmit off before this call was answered. Turn TX On to call again.',
    )
  })

  it('CONTROL: still calling, the state reads as ever and is not marked', () => {
    const { container } = render(el(calling))
    const s = stateEl(container)
    expect(s.textContent).toBe('AwaitReport')
    expect(s.classList.contains('watchdog')).toBe(false)
  })

  it('flashes the window once, at the trip, and not again while it stands', async () => {
    const { rerender } = render(el(calling))
    await Promise.resolve()
    expect(api.attention).toBe(0)
    rerender(el(stopped))
    await Promise.resolve()
    expect(api.attention).toBe(1)
    // Later snapshots of the same trip.
    rerender(el({ ...stopped, txCount: 10 } as QsoStatus))
    rerender(el({ ...stopped } as QsoStatus))
    await Promise.resolve()
    expect(api.attention).toBe(1)
    // TX On clears it; the next trip flashes again.
    rerender(el(calling))
    rerender(el(stopped))
    await Promise.resolve()
    expect(api.attention).toBe(2)
  })

  it('does not flash for a trip that was already standing when the strip appeared', async () => {
    render(el(stopped))
    await Promise.resolve()
    expect(api.attention).toBe(0)
    expect(screen.getByText('TX watchdog stopped the call')).toBeTruthy()
  })
})
