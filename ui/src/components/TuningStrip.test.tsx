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
import { setFrequency, setVfo, swapVfo, setXit } from '../api'
import { pushToast } from '../toast'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  setFrequency: vi.fn(() => Promise.resolve(null)),
  setRit: vi.fn(() => Promise.resolve(null)),
  setXit: vi.fn(() => Promise.resolve(null)),
  setVfo: vi.fn(() => Promise.resolve(null)),
  swapVfo: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const mockSetFreq = setFrequency as unknown as ReturnType<typeof vi.fn>
const mockToast = pushToast as unknown as ReturnType<typeof vi.fn>
const mockSwapVfo = swapVfo as unknown as ReturnType<typeof vi.fn>
const mockSetVfo = setVfo as unknown as ReturnType<typeof vi.fn>

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
  mockSwapVfo.mockClear()
  mockSetVfo.mockClear()
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

// #273: the first ◄/► from a dial between steps rounds to the step; after that, whole steps.
describe('#273 the nudges round to the step first', () => {
  const at = (dialMhz: number) => render(<TuningStrip snap={snapWith({ dialMhz, band: '17m' })} step={1000} />)
  const last = () => mockSetFreq.mock.calls[mockSetFreq.mock.calls.length - 1]

  it('► from 18.110.250 at 1 kHz lands on 18.111.000', () => {
    at(18.11025)
    fireEvent.click(screen.getByRole('button', { name: 'Tune up 1000 Hz' }))
    expect(last()[0]).toBeCloseTo(18.111, 6)
    expect(last()[1]).toBe('17m')
  })

  it('◄ from 18.110.250 lands on 18.110.000', () => {
    at(18.11025)
    fireEvent.click(screen.getByRole('button', { name: 'Tune down 1000 Hz' }))
    expect(last()[0]).toBeCloseTo(18.11, 6)
  })

  it('►► and ◄◄ round first, then take the other nine steps', () => {
    at(18.11025)
    fireEvent.click(screen.getByRole('button', { name: 'Tune up 10000 Hz' }))
    expect(last()[0]).toBeCloseTo(18.12, 6)
    fireEvent.click(screen.getByRole('button', { name: 'Tune down 10000 Hz' }))
    expect(last()[0]).toBeCloseTo(18.101, 6)
  })

  it('POSITIVE CONTROL — an on-grid dial moves exactly one step', () => {
    at(18.111)
    fireEvent.click(screen.getByRole('button', { name: 'Tune up 1000 Hz' }))
    expect(last()[0]).toBeCloseTo(18.112, 6)
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

// ── A⇄B (2026-09-22) ─────────────────────────────────────────────────────────────────
//
// The A and B buttons have always been here and `activeVfo` has always been read back; the
// one thing missing was the SWAP, and `swapVfo` sat in api.ts with no caller at all. It is
// the gesture a split operator makes constantly — listen on B, work on A, put them back.
//
// ⛔ SWAP ONLY. A=B (copy the active VFO onto the other) is a DIFFERENT rig verb, it
// OVERWRITES a dial rather than exchanging two, and it is not offered here (operator,
// 2026-09-22). The census below is what holds that, rather than a grep for a name.
//
// ⚠️ jsdom NEVER LAYS OUT, so nothing here reads geometry: whether three buttons still fit
// the strip's row at 1024 is a browser question and is not answered in this file.
describe('the A⇄B swap', () => {
  const swap = () => screen.getByRole('button', { name: 'Swap VFO A and B' }) as HTMLButtonElement

  it('swaps the two VFOs — and does NOT select one, which is the other verb', () => {
    // The discriminating pair. `setVfo('A')` would also "do something about the VFO" and
    // would leave a station that was on B still on B; only `swapVfo` exchanges them, so the
    // assertion has to name both or it cannot tell a swap from a select.
    mount(snapWith({ activeVfo: 'B' }))
    fireEvent.click(swap())
    expect(mockSwapVfo, 'the swap button did not command a swap').toHaveBeenCalledTimes(1)
    expect(mockSetVfo, 'the swap selected a VFO instead of exchanging them').not.toHaveBeenCalled()
  })

  it('sits with the A and B buttons, and those three are the WHOLE VFO group', () => {
    // Placement is the point of the control — beside the pair it acts on. The count is the
    // A=B guard: a fourth button in this group is a copy control that nobody approved, and
    // this fails by NUMBER rather than by looking for a name a copy might not use.
    mount(snapWith({ activeVfo: 'A' }))
    const group = screen.getByRole('group', { name: 'Active VFO' })
    const names = [...group.querySelectorAll('button')].map((b) => b.getAttribute('aria-label') ?? b.textContent)
    expect(names, 'the VFO group is no longer exactly A, B and the swap').toEqual(['A', 'B', 'Swap VFO A and B'])
  })

  it('CAT DOWN: it is disabled and commands nothing — a swap over a dead link is a lie', () => {
    mount(snapWith({ catOk: false }))
    expect(swap().disabled, 'the swap was offered with no CAT link').toBe(true)
    fireEvent.click(swap())
    expect(mockSwapVfo).not.toHaveBeenCalled()
  })

  it('POSITIVE CONTROL — with CAT up it is live, and so are the A/B buttons beside it', () => {
    // Without this the disabled case above passes against a button that is ALWAYS dead.
    mount(snapWith({ catOk: true }))
    expect(swap().disabled).toBe(false)
    expect((screen.getByRole('button', { name: 'A' }) as HTMLButtonElement).disabled).toBe(false)
  })
})

// ── XIT ON A RADIO THAT HAS NONE (2026-09-23) ────────────────────────────────────────
//
// The IC-9700 has no XIT: Icom's CI-V reference for it lists RIT and no ΔTX. The engine says
// so on the snapshot (`xitUnsupported`), and this strip, which Phone, CW, Operate and the
// browser all draw, reads that one answer rather than a model name of its own.
describe('XIT follows the radio', () => {
  const xitButtons = () => ['XIT', 'XIT down', 'XIT up'].map((name) => screen.queryByRole('button', { name }))

  it('a radio with no XIT gets no XIT buttons, and RIT beside them stays', () => {
    mount(snapWith({ xitUnsupported: true }))
    expect(xitButtons(), 'XIT was offered on a radio that has none').toEqual([null, null, null])
    // The control: the clarifier half of the strip is still drawn, so this cannot pass on a
    // strip that failed to render.
    expect(screen.getByRole('button', { name: 'RIT up' })).not.toBeNull()
  })

  it('POSITIVE CONTROL — a radio that has it (or a station too old to say) keeps XIT', () => {
    mount(snapWith({ xitHz: 0 }))
    for (const b of xitButtons()) expect(b, 'XIT vanished from a radio that has it').not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'XIT up' }))
    expect(setXit).toHaveBeenCalledWith(10)
  })
})
