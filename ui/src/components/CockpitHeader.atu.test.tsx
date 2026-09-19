// @vitest-environment jsdom
//
// THE RIG'S OWN ATU (discussion #19, N8GB, FTdx10).
//
// Nexus's Tune emits OUR carrier through the sound card; what the operator asked for is the
// radio's built-in antenna tuner, which WSJT-X fires from a right-click on Tune. Two things about
// that button are load-bearing enough to pin here:
//
//  1. it is shown ONLY when the radio actually reports a tuner. Offering an ATU control to a rig
//     that has no ATU is worse than not having the control — the operator presses it, nothing
//     happens, and they cannot tell whether the app or the radio is at fault;
//  2. it lives with the TRANSMIT controls, beside Tune, and carries Tune's licence lockout. An
//     ATU tune-up keys the transmitter; it is not a receive filter like NB/NR/Notch, and it must
//     not be reachable when transmitting here is not permitted.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
vi.mock('../useWheelTune', () => ({ useWheelTune: () => undefined }))

const snapWith = (over: Record<string, unknown> = {}) =>
  ({
    radio: {
      dialMhz: 14.29,
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: true,
      tuning: false,
      txAllowed: true,
      ...over,
    },
  }) as unknown as AppSnapshot

function mount(snap: AppSnapshot, onAtuTune: (() => void) | undefined = () => {}) {
  return render(
    <CockpitHeader
      snap={snap}
      modeIndicator={<span>Phone</span>}
      bandControl={<span>20m</span>}
      onTune={() => {}}
      onAtuTune={onAtuTune}
    />,
  )
}

const atuButton = () => screen.queryByRole('button', { name: 'ATU' })

afterEach(cleanup)

describe("the rig's own ATU control", () => {
  it('is absent when the radio does not report a tuner', () => {
    mount(snapWith({ atu: null }))
    expect(atuButton()).toBeNull()
    // …and the control it sits beside is still there, so this is measuring the ATU button and
    // not a header that failed to render at all.
    expect(screen.getByRole('button', { name: 'Tune' })).toBeTruthy()
  })

  it('appears once the radio reports one, bypassed or in-line', () => {
    mount(snapWith({ atu: false }))
    expect(atuButton()).toBeTruthy()
    cleanup()
    mount(snapWith({ atu: true }))
    expect(atuButton()).toBeTruthy()
  })

  it('is not offered at all in a cockpit that has no Tune control either', () => {
    // Operate/RTTY pass no `onAtuTune` (they carry no Tune button in the header). The ATU is a
    // sibling of Tune, so it appears exactly where Tune does and nowhere else.
    render(
      <CockpitHeader
        snap={snapWith({ atu: true })}
        modeIndicator={<span>FT8</span>}
        bandControl={<span>20m</span>}
      />,
    )
    expect(atuButton()).toBeNull()
  })

  it('is locked out with Tune when transmitting here is not permitted', () => {
    // The licence gate — the same one Tune carries. The backend refuses this case too (with a
    // reason); the disabled button is so the operator can see it before they press.
    mount(snapWith({ atu: true, txAllowed: false }))
    expect(atuButton()?.hasAttribute('disabled')).toBe(true)
    // Control: permitted → live.
    cleanup()
    mount(snapWith({ atu: true, txAllowed: true }))
    expect(atuButton()?.hasAttribute('disabled')).toBe(false)
  })

  it('runs the tuner when pressed', () => {
    const fired = vi.fn()
    mount(snapWith({ atu: true }), fired)
    atuButton()?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    expect(fired).toHaveBeenCalledTimes(1)
  })

  // ⭐ Operator ruling (2026-09-19): "Say it can't, add it natively." On Hamlib's Icom and
  // Kenwood backends the start-tune command is clamped to "switch the tuner in line", so the
  // press tunes nothing. The button STAYS — the tuner and its in-line state are real, and this
  // is where the operator reads them — and says what to do instead.
  it('stays, disabled and explaining itself, where the CAT path cannot start a tune', () => {
    mount(snapWith({ atu: true, atuStartTuneUnsupported: true }))
    const atu = atuButton()
    expect(atu).toBeTruthy()
    expect(atu?.hasAttribute('disabled')).toBe(true)
    expect(atu?.getAttribute('title')).toContain('TUNER on the radio itself')
    // …and it still reports the tuner state, which is the other thing this button is for.
    expect(atu?.getAttribute('title')).toContain('switched in')

    // CONTROL: the same radio on a path that CAN start a tune is live, with no such warning.
    cleanup()
    mount(snapWith({ atu: true }))
    expect(atuButton()?.hasAttribute('disabled')).toBe(false)
    expect(atuButton()?.getAttribute('title')).not.toContain('TUNER on the radio itself')
  })
})
