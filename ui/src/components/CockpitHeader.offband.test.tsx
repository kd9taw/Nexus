// @vitest-environment jsdom
//
// THE RED READOUT IS A PRIVILEGE ANSWER, NOT A BAND-TABLE MISS.
//
// `txBlocked` paints the hero frequency in the TX colour, which reads as "you must not key
// here". It was derived from the UI's OWN band table (`!bandLabelForMhz(dial)`), so listening
// off the ham bands — WWV, shortwave, CB, marine, the gaps between band edges, a first-class
// supported use case per the operator ruling of 2026-08-13 — alarm-coloured the readout for the
// whole session. The snapshot already carries the authority: `radio.txAllowed` is the backend's
// licence-privilege answer for the current dial AND mode, which a frequency table cannot give
// (it also knows the non-US `Open` class, and the privilege gaps INSIDE a band that the table
// says nothing about).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

// Capture the options CockpitHeader hands the wheel-tune hook, so the edge callback can be
// driven directly — the hook's own scroll plumbing is tested in `useWheelTune.test.ts`.
type WheelOpts = { onEdge?: (mhz: number) => void }
const wheelOptions: WheelOpts[] = []
vi.mock('../useWheelTune', () => ({
  useWheelTune: (_ref: unknown, opts: WheelOpts) => {
    wheelOptions.push(opts)
  },
}))

const snapWith = (over: Record<string, unknown> = {}) =>
  ({
    radio: {
      dialMhz: 14.074,
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: false,
      tuning: false,
      txAllowed: true,
      ...over,
    },
  }) as unknown as AppSnapshot

function mount(snap: AppSnapshot) {
  return render(
    <CockpitHeader snap={snap} modeIndicator={<span>FT8</span>} bandControl={<span>—</span>} />,
  )
}

const blocked = (root: ParentNode) => root.querySelector('.readout')?.classList.contains('blocked')

afterEach(cleanup)

describe('the off-band dial is not an alarm', () => {
  it('WWV on 5 MHz with transmit permitted reads normally, not TX-red', () => {
    // 5.000 MHz is off every amateur allocation, so the band table returns nothing — the
    // whole of the old condition. The backend still says transmit is allowed here (an `Open`
    // class operator), and it is the one that knows.
    const { container } = mount(snapWith({ dialMhz: 5.0, txAllowed: true }))
    expect(blocked(container)).toBe(false)
  })

  it('…and the same off the band EDGE, where a listener spends most of their time', () => {
    const { container } = mount(snapWith({ dialMhz: 13.9, txAllowed: true }))
    expect(blocked(container)).toBe(false)
  })

  it('a real privilege block still paints red — off band (the control that must fire)', () => {
    const { container } = mount(snapWith({ dialMhz: 5.0, txAllowed: false }))
    expect(blocked(container)).toBe(true)
  })

  it('…and INSIDE a band, which the band-table derivation could never see', () => {
    // A General on 14.010 (Extra-only CW) is genuinely blocked, and the old condition read
    // "20m" and called it fine. Same wire, both directions now honest.
    const { container } = mount(snapWith({ dialMhz: 14.01, txAllowed: false }))
    expect(blocked(container)).toBe(true)
  })

  it('an ordinary in-band dial with privileges is unchanged', () => {
    const { container } = mount(snapWith({ dialMhz: 14.074, txAllowed: true }))
    expect(blocked(container)).toBe(false)
  })
})

// THE BAND-EDGE TOAST IS NOW A PARK, NOT A REFUSAL.
//
// The wheel used to REFUSE a target off the band plan, so "outside the band plan" at `error`
// severity was an honest description of what had just happened. Since the 2026-08-13 ruling the
// wheel tunes off the bands like anywhere else: a burst merely PARKS on the edge (the runaway
// guard), and one more deliberate gesture goes straight past it. Calling a supported
// destination an error — and speaking it into the assertive live region, which is what `error`
// does — tells the operator the app refused them when it did nothing of the kind.
describe('the band-edge toast', () => {
  it('reports a park, not a refusal', async () => {
    const { pushToast } = await import('../toast')
    const mockPush = vi.mocked(pushToast)
    mockPush.mockClear()
    mount(snapWith({ dialMhz: 7.3 })) // parked on the top edge of 40 m
    const onEdge = wheelOptions[wheelOptions.length - 1]?.onEdge
    expect(onEdge, 'harness: CockpitHeader must still install an onEdge handler').toBeTypeOf(
      'function',
    )
    onEdge?.(7.4)
    expect(mockPush).toHaveBeenCalledTimes(1)
    const [message, kind] = mockPush.mock.calls[0] as [string, string]
    expect(kind, `an edge park is not an error: "${message}"`).not.toBe('error')
    expect(message).not.toContain('outside the band plan')
    // …and it must still NAME THE EDGE, which is the whole reason the toast exists.
    expect(message).toContain('7.3000')
  })

  it('still names the edge it parked on (the control: the message is not blank)', async () => {
    const { pushToast } = await import('../toast')
    const mockPush = vi.mocked(pushToast)
    mockPush.mockClear()
    mount(snapWith({ dialMhz: 14.35 }))
    wheelOptions[wheelOptions.length - 1]?.onEdge?.(15.074)
    const [message] = mockPush.mock.calls[0] as [string]
    expect(message).toContain('20m')
    expect(message.length).toBeGreaterThan(10)
  })
})
