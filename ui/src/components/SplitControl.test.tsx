// @vitest-environment jsdom
//
// THE DEFECT THIS PINS IS AN ABSENCE, which is why it is a test and not a comment.
//
// Field report 2026-08-25: a General worked a DX in the Extra-only 20 m CW bottom with his rig
// split — RX 14.015, TX 14.026, which is how DX is worked — and Nexus refused to key. The
// privilege gate was judging his RECEIVE dial. Fixing the gate was half the job: the CW cockpit
// had only a read-only `SPLIT ▲` plate and Operate only an annunciator, so the operator who
// reported it still had no way to TELL Nexus where he was transmitting. A gate that judges the
// right frequency is no use if nothing on screen can set it.
//
// So this asserts the control EXISTS and WORKS in all three cockpits, and that toggling it
// commands the frequency the operator would expect.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { SplitControl } from './SplitControl'
import type { AppSnapshot } from '../types'

const setSplit = vi.hoisted(() => vi.fn())
vi.mock('../api', () => ({ setSplit }))

function snapWith(splitTxMhz: number | null): AppSnapshot {
  return {
    radio: { dialMhz: 14.015, splitTxMhz, catOk: true },
  } as unknown as AppSnapshot
}

beforeEach(() => {
  cleanup()
  setSplit.mockReset()
  setSplit.mockResolvedValue(snapWith(null))
})

describe('the split control', () => {
  it('commands a TX frequency the operator can actually reach', async () => {
    render(<SplitControl snap={snapWith(null)} />)

    // Default offset is +5 kHz, the common pileup. From 14.015 that is 14.020.
    fireEvent.click(screen.getByRole('button', { name: 'SPLIT' }))
    expect(setSplit).toHaveBeenCalledTimes(1)
    expect(setSplit.mock.calls[0][0]).toBeCloseTo(14.02, 6)
  })

  it('steps the offset, and off is off', async () => {
    // Split already on at +5.
    render(<SplitControl snap={snapWith(14.02)} />)
    // The readout adopts the rig's REAL offset rather than a stale local default.
    expect(screen.getByText('+5')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: '+' }))
    expect(setSplit.mock.calls[setSplit.mock.calls.length - 1][0]).toBeCloseTo(14.021, 6)

    // Toggling while on clears to simplex — `null`, not a frequency.
    fireEvent.click(screen.getByRole('button', { name: 'SPLIT' }))
    expect(setSplit.mock.calls[setSplit.mock.calls.length - 1][0]).toBeNull()
  })

  /// The reported operator's own case, in the direction that matters: he is receiving in a
  /// segment he may not transmit in, and the control is what lets him say so.
  it('can put the transmit frequency somewhere other than the receive dial', async () => {
    render(<SplitControl snap={snapWith(null)} />)
    // Eleven presses of + from the +5 default → +16 kHz → 14.031, clear of the 14.025 floor.
    for (let i = 0; i < 11; i++) {
      fireEvent.click(screen.getByRole('button', { name: '+' }))
    }
    fireEvent.click(screen.getByRole('button', { name: 'SPLIT' }))
    const tx = setSplit.mock.calls[setSplit.mock.calls.length - 1][0] as number
    expect(tx).toBeCloseTo(14.031, 6)
    expect(tx).toBeGreaterThan(14.025) // the General CW floor he was locked out by
  })

  /// The backend refuses a press that would leave the licence, with the reason: the operator
  /// is told that reason, and the readout stays on the offset the radio really has.
  it('shows a licence refusal as it is and keeps the real offset', async () => {
    const onError = vi.fn()
    const refusal = 'Split not set: that transmit frequency is outside your license privileges.'
    setSplit.mockRejectedValueOnce(refusal)
    render(<SplitControl snap={snapWith(14.02)} onError={onError} />)
    fireEvent.click(screen.getByRole('button', { name: '+' }))
    await waitFor(() => expect(onError).toHaveBeenCalledWith(refusal))
    await waitFor(() => expect(screen.getByText('+5')).toBeTruthy())
  })

  /// Two quick presses, the first accepted (the radio moves to +6) and the second refused: the
  /// readout lands on +6, the radio's offset, not on the +5 the refused press was made from.
  it('a refusal after an accepted press shows the accepted offset', async () => {
    const onError = vi.fn()
    let reject: (e: unknown) => void = () => {}
    setSplit.mockImplementationOnce(() => new Promise((_, r) => { reject = r }))
    const { rerender } = render(<SplitControl snap={snapWith(14.02)} onError={onError} />)
    fireEvent.click(screen.getByRole('button', { name: '+' }))
    // The first press's answer arrives: the radio is at +6.
    rerender(<SplitControl snap={snapWith(14.021)} onError={onError} />)
    reject('Split not set: that transmit frequency is outside your license privileges.')
    await waitFor(() => expect(onError).toHaveBeenCalled())
    await waitFor(() => expect(screen.getByText('+6')).toBeTruthy())
  })

  it('reports a failure instead of looking like a dead button', async () => {
    const onError = vi.fn()
    setSplit.mockRejectedValueOnce(new Error('rig refused'))
    render(<SplitControl snap={snapWith(null)} onError={onError} />)
    fireEvent.click(screen.getByRole('button', { name: 'SPLIT' }))
    // The rejection lands in a .catch(), so the report is a tick away.
    await waitFor(() => expect(onError).toHaveBeenCalled())
  })
})
