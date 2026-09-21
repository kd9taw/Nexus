// @vitest-environment jsdom
//
// THE ANALOG METER — one instrument, two roles.
//
// The operator could not find the meters at all: Phone's S reading was a 10-px bar inside the
// scope header and its TX meters were four bar rows that render BLANK until the first over, so
// on receive there was nothing meter-shaped on the cockpit. This is the instrument that answers
// that, and these are the claims it has to keep.
//
// ⚠️ jsdom NEVER LAYS OUT, so nothing here asserts geometry. The face is SVG and its arithmetic
// is testable directly — `readingFor` and `facePoint` are exported pure functions for exactly
// that reason — so the numbers are asserted on the functions and only STRUCTURE (which elements
// exist, what text, what disabled state) is asserted on the render.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { AnalogMeter, readingFor, facePoint, sFrac, sLabel } from './AnalogMeter'
import type { RadioStatus } from '../types'

afterEach(cleanup)

const radio = (over: Record<string, unknown> = {}) => ({ ...over }) as unknown as RadioStatus

describe('what the needle reads', () => {
  it('reads S-units on receive and the picked transmit scale when keyed', () => {
    const r = radio({ smeterDb: -12, txPoW: 42 })
    // Receive: the S scale, and S7 is -12 dB (6 dB per unit below S9).
    const rx = readingFor(r, false, 'po', 100)
    expect(rx.plate).toBe('S')
    expect(rx.value).toBe('S7')
    // Keyed: the same radio, the same instant, a different quantity.
    const tx = readingFor(r, true, 'po', 100)
    expect(tx.plate).toBe('PO')
    expect(tx.value).toBe('42 W')
  })

  it('puts S9 just under half scale, which is what leaves room for the +dB zone', () => {
    // The whole reason a real face looks the way it does. S1..S9 spans -48..0 dB and the
    // over-S9 range runs to +60, so the right half of the sweep is the red zone.
    expect(sFrac(0)).toBeCloseTo(0.474, 3)
    expect(sFrac(-48)).toBeLessThan(0.1)
    expect(sFrac(60)).toBe(1)
    // Clamped, so a rig reporting nonsense cannot drive the needle off the face.
    expect(sFrac(999)).toBe(1)
    expect(sFrac(-999)).toBe(0)
  })

  it('never rounds an over-S9 reading UP — that would overstate a signal', () => {
    expect(sLabel(0)).toBe('S9')
    expect(sLabel(20)).toBe('S9+20')
    expect(sLabel(-12)).toBe('S7')
  })

  it('scales power against THIS radio, not a hardcoded 100 W', () => {
    // The bars used watts/100, so a 5 W QRP set never left the first tick. Full scale is the
    // radio's rated output, so the same 5 W reads half on a 10 W rig and full on a 5 W one.
    expect(readingFor(radio({ txPoW: 5 }), true, 'po', 10).frac).toBeCloseTo(0.5, 3)
    expect(readingFor(radio({ txPoW: 5 }), true, 'po', 5).frac).toBe(1)
    expect(readingFor(radio({ txPoW: 50 }), true, 'po', 100).frac).toBeCloseTo(0.5, 3)
    // And the printed numbers follow the rating rather than staying 25/50/100.
    expect(readingFor(radio({ txPoW: 3 }), true, 'po', 10).ticks.map((t) => t.label)).toEqual([
      '0',
      '3',
      '5',
      '10',
    ])
  })

  it('shows a dash and NO needle when the rig reports nothing', () => {
    // Absent is not zero. A needle resting at the left is a READING of "no signal"; this is
    // "no answer", and they must not look the same.
    const r = readingFor(radio({}), false, 'po', 100)
    expect(r.frac).toBeNull()
    expect(r.value).toBe('—')
    render(<AnalogMeter radio={radio({})} keyed={false} />)
    expect(screen.queryByTestId('meter-needle')).toBeNull()
  })
})

describe('the SWR scale does not claim more than Nexus can stand behind', () => {
  it('drops its numbers on a rig whose scale is unverified, and keeps the needle', () => {
    // The bug this guards is #292: a Xiegu reads 1.2:1 on its own front panel and 6:1 here, and
    // the bars were fixed on 2026-09-20 to stop advising "keep it under 2:1" over it. A needle
    // on a numbered arc makes that claim HARDER, not softer.
    const un = readingFor(radio({ txSwr: 2.4, swrScaleVerified: false }), true, 'swr', 100)
    expect(un.ticks, 'an uncalibrated arc must carry no numbers').toEqual([])
    expect(un.plate).toBe('SWR?')
    expect(un.frac, 'the needle still moves — that part is true').toBeGreaterThan(0)

    const ok = readingFor(radio({ txSwr: 2.4, swrScaleVerified: true }), true, 'swr', 100)
    expect(ok.ticks.length).toBeGreaterThan(0)
    expect(ok.plate).toBe('SWR')
  })

  it('treats an ABSENT verification flag as unverified', () => {
    // `!== true`, the same direction the bars and Settings take. Resolving a missing flag to
    // "verified" is the one default that puts the confident wrong number back.
    const r = readingFor(radio({ txSwr: 2.0 }), true, 'swr', 100)
    expect(r.ticks).toEqual([])
    expect(r.plate).toBe('SWR?')
  })
})

describe('the face', () => {
  it('sweeps left to right, and clamps', () => {
    const lo = facePoint(100, 0)
    const mid = facePoint(100, 0.5)
    const hi = facePoint(100, 1)
    expect(lo.x).toBeLessThan(mid.x)
    expect(mid.x).toBeLessThan(hi.x)
    // Symmetric about the centre, and the middle of the sweep is the top of the arc.
    expect(mid.y).toBeLessThan(lo.y)
    expect(facePoint(100, 2).x).toBeCloseTo(hi.x, 6)
  })

  it('prints all three scale arcs whatever the needle reads — a rig prints its whole face', () => {
    const { container } = render(<AnalogMeter radio={radio({ smeterDb: -20 })} keyed={false} />)
    expect(container.querySelectorAll('.ph-meter-arc')).toHaveLength(3)
  })

  it('offers the meter switch only while keyed, disabled rather than hidden on receive', () => {
    // DISABLED, NOT HIDDEN: this sits above a bottom-anchored dock, and a row that appeared on
    // key-down would move the PTT button under a held pointer.
    const { rerender } = render(<AnalogMeter radio={radio({ smeterDb: -20 })} keyed={false} />)
    const po = () => screen.getByRole('button', { name: /^PO$/ }) as HTMLButtonElement
    expect(po().disabled).toBe(true)
    rerender(<AnalogMeter radio={radio({ txPoW: 10 })} keyed />)
    expect(po().disabled).toBe(false)
  })

  it('switches what the needle reads when the operator picks another scale', () => {
    render(<AnalogMeter radio={radio({ txPoW: 42, txAlc: 0.5 })} keyed />)
    expect(screen.getByTestId('meter-value').textContent).toBe('42 W')
    fireEvent.click(screen.getByRole('button', { name: /^ALC$/ }))
    expect(screen.getByTestId('meter-value').textContent).toBe('50%')
  })

  it('marks the power cap on the PO scale, and only there', () => {
    const { rerender } = render(
      <AnalogMeter radio={radio({ txPoW: 42 })} keyed capFrac={0.75} />,
    )
    expect(screen.getByTestId('meter-cap')).toBeTruthy()
    // The cap is a fraction of THIS rig's output, so it is placeable on PO and meaningless on
    // an SWR or ALC scale.
    rerender(<AnalogMeter radio={radio({ txSwr: 1.4 })} keyed capFrac={0.75} />)
    fireEvent.click(screen.getByRole('button', { name: /^SWR$/ }))
    expect(screen.queryByTestId('meter-cap')).toBeNull()
  })
})

// ⚠️ THE NEAR-MISS THIS GUARDS. Phone hides the scope header's S reading because the analog
// meter shows the same number on the same screen — but the class on that row is
// `ph-scope-smeter` and the row is NOT the S-meter: it also carries the RF-source badge, the
// dynamic-range readout and the G/Z gain control. Gating the row instead of the three S
// elements would have taken the gain control away from Phone, which is a different and much
// worse change than the one asked for. It was written that way first.
describe('hiding the S reading takes nothing else with it', () => {
  it('drops the S label, track and value — and KEEPS the gain control', async () => {
    const { PhoneScope } = await import('./PhoneScope')
    const props = {
      transmitting: false,
      theme: 'dark' as const,
      smeterDb: -20,
      active: false,
    }
    const { container, rerender } = render(<PhoneScope {...props} />)
    // Control first: with the strip shown, both the reading and the gain control are present.
    expect(container.querySelector('.ph-scope-smeter-track')).not.toBeNull()
    expect(container.querySelector('.ph-scope-gz')).not.toBeNull()

    rerender(<PhoneScope {...props} hideSmeter />)
    expect(
      container.querySelector('.ph-scope-smeter-track'),
      'the S reading is what Phone hides',
    ).toBeNull()
    expect(
      container.querySelector('.ph-scope-gz'),
      'the gain control lives in the same row and must survive',
    ).not.toBeNull()
  })
})

