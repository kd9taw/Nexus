// @vitest-environment jsdom
//
// THE RIG'S OWN ATU (discussion #19, N8GB, FTdx10).
//
// Nexus's Tune emits OUR carrier through the sound card; what the operator asked for is the
// radio's built-in antenna tuner, which WSJT-X fires from a right-click on Tune. Two things about
// that button are load-bearing enough to pin here:
//
//  1. it is shown ONLY when the radio actually reports a tuner — a deliberate EXCEPTION to
//     the 2026-09-20 disabled-with-reason ruling, argued in the case below;
//  2. it lives with the TRANSMIT controls, beside Tune, and carries Tune's licence lockout. An
//     ATU tune-up keys the transmitter; it is not a receive filter like NB/NR/Notch, and it must
//     not be reachable when transmitting here is not permitted.
//
// Since 2026-10-01 the button is the TX strip's (CockpitTxStrip, FT's cluster
// under the scope on every screen), no longer the cockpit header's; these cases moved with it.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, act } from '@testing-library/react'
import { CockpitTxStrip } from './CockpitTxStrip'
import { ATU_LONG_PRESS_MS } from './AtuSwitchOutMenu'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ haltTx: vi.fn(() => Promise.resolve(null)) }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

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

function mount(
  snap: AppSnapshot,
  onAtuTune: (() => void) | undefined = () => {},
  onAtuBypass?: () => void,
) {
  return render(
    <CockpitTxStrip
      radio={snap.radio}
      onTune={() => {}}
      onAtuTune={onAtuTune}
      onAtuBypass={onAtuBypass}
    />,
  )
}

// "ATU", or "ATU ON" while the radio reports its tuner in line (#344).
const atuButton = () => screen.queryByRole('button', { name: /^ATU( ON)?$/ })

afterEach(cleanup)

describe("the rig's own ATU control", () => {
  // ⭐ AN EXCEPTION TO THE DISABLED-WITH-REASON RULING, ruled on the same day as the ruling
  // and recorded here because it looks exactly like a site the sweep missed.
  //
  // That ruling replaced gating-by-absence with disabled-and-said across the Phone cockpit:
  // a control that vanishes is indistinguishable from one that was never built. Its home
  // ground is "the rig can do it, THIS PATH cannot" — a control Nexus built, unreachable
  // through the operator's transport. That case is alive and well on this very button: see
  // the `atuStartTuneUnsupported` case at the foot of this file, where the radio HAS a tuner
  // Hamlib will not start, and the button stays and says so.
  //
  // ABSENT HARDWARE IS NOT A PATH PROBLEM, and this control KEYS THE TRANSMITTER. An ATU
  // button on a barefoot rig is a tune-up affordance over a radio with nothing to tune, and
  // that precaution outranks the discoverability a ⊘ mark would buy.
  //
  // ⚠️ IT WAS BRIEFLY IMPLEMENTED THE OTHER WAY while the ruling was being swept, and
  // reverted. If you are here to "finish" the sweep: this is the one that stays.
  it('is absent when the radio does not report a tuner', () => {
    mount(snapWith({ atu: null }))
    expect(atuButton()).toBeNull()
    // …and the control it sits beside is still there, so this is measuring the ATU button and
    // not a header that failed to render at all.
    expect(screen.getByRole('button', { name: 'Tune' })).toBeTruthy()
  })

  it('CONTROL: absent, and not a dead button with a reason either', () => {
    // The exception is ABSENCE. A ⊘ mark in the button's place is what the ruling would have
    // produced and what was briefly shipped, so it is checked for by name — and checked on
    // BOTH rigs, because "no mark" on one that has a tuner would pass against a header that
    // marks the barefoot one.
    mount(snapWith({ atu: null }))
    expect(document.querySelector('.cq-txctl .ph-unavail'), 'the reverted ⊘ mark came back').toBeNull()
    cleanup()
    mount(snapWith({ atu: false }))
    expect(atuButton()!.hasAttribute('disabled')).toBe(false)
    expect(document.querySelector('.cq-txctl .ph-unavail')).toBeNull()
  })

  it('appears once the radio reports one, bypassed or in-line', () => {
    mount(snapWith({ atu: false }))
    expect(atuButton()).toBeTruthy()
    cleanup()
    mount(snapWith({ atu: true }))
    expect(atuButton()).toBeTruthy()
  })

  it('is not offered at all on a strip that has no Tune control either', () => {
    // A screen that passes no `onAtuTune` gets no ATU, even over a radio with a tuner. The ATU is
    // a sibling of Tune, so it appears exactly where Tune does and nowhere else.
    render(<CockpitTxStrip radio={snapWith({ atu: true }).radio} />)
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
  // ⭐ #344 (RaptorFlight): "after running ATU maybe have it change to ATU ON (and then you could
  // click to turn it off)". Operator ruling: the label follows the radio's tuner, a CLICK always
  // runs a tune-up (exactly as before, whatever the tuner state), and switching the tuner out is a
  // separate action in the button's menu — right-click, or a long press on touch — offered only
  // where Nexus can switch it out (`atuSwitchOut`: never its own Flex client, never native Icom).
  it('reads ATU ON while the tuner is in line, and ATU while it is out', () => {
    mount(snapWith({ atu: true }), () => {}, () => {})
    expect(screen.getByRole('button', { name: 'ATU ON' }).getAttribute('aria-pressed')).toBe('true')
    cleanup()
    mount(snapWith({ atu: false }), () => {}, () => {})
    expect(screen.getByRole('button', { name: 'ATU' }).getAttribute('aria-pressed')).toBe('false')
  })

  it('a click runs a tune-up in both states and never switches the tuner out', () => {
    const tune = vi.fn()
    const out = vi.fn()
    mount(snapWith({ atu: true, atuSwitchOut: true }), tune, out)
    fireEvent.click(screen.getByRole('button', { name: 'ATU ON' }))
    expect([tune.mock.calls.length, out.mock.calls.length]).toEqual([1, 0])
    cleanup()
    mount(snapWith({ atu: false, atuSwitchOut: true }), tune, out)
    fireEvent.click(screen.getByRole('button', { name: 'ATU' }))
    expect([tune.mock.calls.length, out.mock.calls.length]).toEqual([2, 0])
  })

  it('a right-click offers "Switch tuner out", which switches it out and runs no tune-up', async () => {
    const tune = vi.fn()
    const out = vi.fn()
    mount(snapWith({ atu: true, atuSwitchOut: true }), tune, out)
    const btn = screen.getByRole('button', { name: 'ATU ON' })
    expect(btn.getAttribute('aria-haspopup')).toBe('menu')
    fireEvent.contextMenu(btn)
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Switch tuner out' }))
    expect([out.mock.calls.length, tune.mock.calls.length]).toEqual([1, 0])
  })

  it('a long press on touch opens the same menu, and the press does not tune', () => {
    vi.useFakeTimers()
    try {
      const tune = vi.fn()
      const out = vi.fn()
      mount(snapWith({ atu: true, atuSwitchOut: true }), tune, out)
      const btn = screen.getByRole('button', { name: 'ATU ON' })
      fireEvent.pointerDown(btn, { pointerType: 'touch' })
      act(() => {
        vi.advanceTimersByTime(ATU_LONG_PRESS_MS + 50)
      })
      fireEvent.pointerUp(btn, { pointerType: 'touch' })
      fireEvent.click(btn) // the click a touch press ends with
      expect(screen.getByRole('menuitem', { name: 'Switch tuner out' })).toBeTruthy()
      expect(tune).not.toHaveBeenCalled()
      // CONTROL: a short tap is a click, and tunes.
      cleanup()
      mount(snapWith({ atu: true, atuSwitchOut: true }), tune, out)
      const tap = screen.getByRole('button', { name: 'ATU ON' })
      fireEvent.pointerDown(tap, { pointerType: 'touch' })
      act(() => {
        vi.advanceTimersByTime(100)
      })
      fireEvent.pointerUp(tap, { pointerType: 'touch' })
      fireEvent.click(tap)
      expect(tune).toHaveBeenCalledTimes(1)
      expect(screen.queryByRole('menuitem')).toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })

  it('the menu is offered only where the tuner can be switched out, and only while it is in', () => {
    const cases: [Record<string, unknown>, boolean, boolean][] = [
      [{ atu: true, atuSwitchOut: true }, true, true], // POSITIVE
      [{ atu: true, atuSwitchOut: false }, true, false], // the path cannot (Flex client, old station)
      [{ atu: true }, true, false], // a station older than the field
      [{ atu: false, atuSwitchOut: true }, true, false], // the tuner is already out
      [{ atu: true, atuSwitchOut: true }, false, false], // no switch-out wired on this screen
    ]
    for (const [over, wired, offered] of cases) {
      mount(snapWith(over), () => {}, wired ? () => {} : undefined)
      const btn = atuButton()!
      fireEvent.contextMenu(btn)
      expect(
        [btn.getAttribute('aria-haspopup'), screen.queryByRole('menuitem') != null],
        JSON.stringify({ over, wired }),
      ).toEqual([offered ? 'menu' : null, offered])
      cleanup()
    }
  })

  it('on a Flex run by Nexus after a matched cycle: ATU ON, a click still tunes, no switch-out', () => {
    // M1: the Flex client reports its tuner in line after a cycle that matched. The button must keep
    // running tune-ups (an antenna change, a stale band memory) and offer no switch-out it has not got.
    const tune = vi.fn()
    const out = vi.fn()
    mount(
      snapWith({ atu: true, atuSwitchOut: false, flexAtu: { tune: {}, status: 'TUNE_SUCCESSFUL', refused: null } }),
      tune,
      out,
    )
    const btn = screen.getByRole('button', { name: 'ATU ON' })
    fireEvent.click(btn)
    fireEvent.contextMenu(btn)
    expect([tune.mock.calls.length, out.mock.calls.length]).toEqual([1, 0])
    expect(screen.queryByRole('menuitem')).toBeNull()
  })

  it('the switch-out keys nothing, so a locked-out tune-up still leaves the menu reachable', async () => {
    // Outside privileges, or on a CAT path that cannot START a tune: the tune-up stays locked (the
    // click does nothing), and switching the tuner OUT is still offered — it puts nothing on the air.
    for (const over of [{ txAllowed: false }, { atuStartTuneUnsupported: true }]) {
      const tune = vi.fn()
      const out = vi.fn()
      mount(snapWith({ atu: true, atuSwitchOut: true, ...over }), tune, out)
      const btn = screen.getByRole('button', { name: 'ATU ON' })
      expect([btn.hasAttribute('disabled'), btn.getAttribute('aria-disabled')]).toEqual([false, 'true'])
      fireEvent.click(btn)
      expect(tune, JSON.stringify(over)).not.toHaveBeenCalled()
      fireEvent.contextMenu(btn)
      fireEvent.click(await screen.findByRole('menuitem', { name: 'Switch tuner out' }))
      expect(out).toHaveBeenCalledTimes(1)
      cleanup()
      // CONTROL: with no menu to offer the button is disabled outright, as before.
      mount(snapWith({ atu: false, atuSwitchOut: true, ...over }), tune, out)
      expect(screen.getByRole('button', { name: 'ATU' }).hasAttribute('disabled')).toBe(true)
      cleanup()
    }
  })

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
