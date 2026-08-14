// @vitest-environment jsdom
//
// THE TUNING STRIP MAY LEAVE THE BAND PLAN (operator, 2026-08-13).
//
// Two separate refusals lived in this one strip, and between them an operator could not reach
// WWV, a shortwave broadcaster, or anything in the gap between two band edges from here:
//
//   1. `tuneTo` derived a band from the TARGET and, finding none, pushed an error toast and
//      returned. That is the route every ◄/► nudge and every typed entry takes, so the strip refused
//      the frequency instead of commanding it.
//   2. the read-out was painted TX-red (`.blocked`) on `!bandLabelForMhz(dial)` — the UI's own
//      band table standing in for the engine's privilege answer. Off-band RX is legal listening;
//      it is not a transmit block, and `snap.radio.txAllowed` is the authority that knows the
//      difference.
//
// The REAL band table is used here, not a stub: the frequencies below have to be genuinely
// off-plan for the test to mean anything.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { TuningStrip } from './TuningStrip'
import { setFrequency } from '../api'
import { pushToast } from '../toast'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  setFrequency: vi.fn(() => Promise.resolve(null)),
  setRit: vi.fn(() => Promise.resolve(null)),
  setXit: vi.fn(() => Promise.resolve(null)),
  setVfo: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const mockSetFreq = setFrequency as unknown as ReturnType<typeof vi.fn>
const mockToast = pushToast as unknown as ReturnType<typeof vi.fn>

const snapWith = (over: Record<string, unknown> = {}) =>
  ({
    radio: {
      dialMhz: 14.2,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: false,
      tuning: false,
      txAllowed: true,
      ...over,
    },
  }) as unknown as AppSnapshot

const mount = (snap = snapWith()) => render(<TuningStrip snap={snap} />)

/** Type a frequency into the hero read-out and commit it with Enter — the operator's route in. */
function typeDial(mhz: string) {
  fireEvent.click(screen.getByRole('button', { name: /megahertz|MHz|Dial/i }))
  const input = screen.getByLabelText('Dial frequency (MHz)')
  fireEvent.change(input, { target: { value: mhz } })
  fireEvent.keyDown(input, { key: 'Enter' })
}

beforeEach(() => {
  mockSetFreq.mockClear()
  mockToast.mockClear()
})
afterEach(cleanup)

describe('typed entry', () => {
  it('an OFF-BAND frequency tunes there — 5.000 MHz is a receiver frequency, not an error', () => {
    mount()
    typeDial('5.000')
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    // Empty band label: the engine routes a bandless dial. Crossing 10 MHz downward follows the
    // sideband convention (#45), so USB on 20 m becomes LSB down here.
    expect(mockSetFreq.mock.calls[0]).toEqual([5, '', 'LSB'])
    expect(mockToast).not.toHaveBeenCalled()
  })

  it('POSITIVE CONTROL — an in-band entry is unchanged, band label and all', () => {
    mount()
    typeDial('14.250')
    expect(mockSetFreq.mock.calls[0]).toEqual([14.25, '20m', 'USB'])
  })
})

describe('the ◄/► nudges', () => {
  it('step an off-band dial, instead of refusing every press', () => {
    mount(snapWith({ dialMhz: 9.6, sideband: 'USB' })) // a shortwave broadcaster
    fireEvent.click(screen.getByRole('button', { name: 'Tune up 100 Hz' }))
    expect(mockSetFreq).toHaveBeenCalledTimes(1)
    expect(mockSetFreq.mock.calls[0]).toEqual([9.6001, '', 'USB'])
    expect(mockToast).not.toHaveBeenCalled()
  })

  it('POSITIVE CONTROL — with CAT down they are disabled and nothing is commanded', () => {
    mount(snapWith({ dialMhz: 9.6, catOk: false }))
    const up = screen.getByRole('button', { name: 'Tune up 100 Hz' }) as HTMLButtonElement
    expect(up.disabled).toBe(true)
    fireEvent.click(up)
    expect(mockSetFreq).not.toHaveBeenCalled()
  })
})

describe('the read-out’s TX-blocked paint', () => {
  it('an off-band dial the operator MAY transmit on is not painted TX-red', () => {
    // 9.6 MHz names no band, and the strip used to read that as "transmit blocked". The band
    // table does not know about privileges; `txAllowed` is the engine's answer and the only one.
    const { container } = mount(snapWith({ dialMhz: 9.6, txAllowed: true }))
    expect(container.querySelector('.readout.blocked')).toBeNull()
  })

  it('POSITIVE CONTROL — txAllowed=false still paints it, in band and out', () => {
    const inBand = mount(snapWith({ dialMhz: 14.2, txAllowed: false }))
    expect(inBand.container.querySelector('.readout.blocked')).not.toBeNull()
    cleanup()
    const outOfBand = mount(snapWith({ dialMhz: 9.6, txAllowed: false }))
    expect(outOfBand.container.querySelector('.readout.blocked')).not.toBeNull()
  })
})
