// @vitest-environment jsdom
//
// THE PTT BUTTON MUST NEVER LIE ABOUT WHY IT DID NOT KEY THE RIG (issue #81).
//
// The engine drops a PTT request on TWO separate conditions — `Engine::set_ptt` is
// `on && self.tx_enabled && self.tx_allowed()`. The cockpit only ever showed ONE of them.
// With `tx_enabled` false and privileges fine, the button still read "PUSH TO TALK", the
// click went to the wire, the wire threw it away, and the rig stayed silent with nothing on
// screen to say why — reported as "Clicking on Push to Talk does not trigger TX" on an FTdx10
// that was working perfectly. Worse, `key()` set its LOCAL `keyed` state before the wire
// refused, so the button then lit up red and read "ON AIR — release to stop" over a
// transmitter that was not keyed. An operator cannot tell that apart from broken hardware.
//
// The three states asserted here are the three answers the control owes:
//   1. permitted + enabled  → PUSH TO TALK, and a press really keys.
//   2. permitted + TX off   → say TX is OFF (not "locked", which is a different fact and a
//                             different remedy), never claim ON AIR, never fake a key, and
//                             offer the way back — in Phone there is NO other Enable-Tx
//                             affordance on screen (the TopBar's is hidden with the digital
//                             chrome, and this cockpit's header pill is display-only), so a
//                             message naming a switch that is not there would strand him.
//   3. not permitted        → 🔒 TX LOCKED, disabled — the pre-existing licence lock, which
//                             must NOT be widened to cover state 2.
//
// This suite asserts what the operator is TOLD. It says nothing about what is allowed: the
// engine's gate is untouched and arming keys nothing by itself (`set_operating_mode` already
// arms TX on entering Phone).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'

const { setPtt, setTxEnabled, pushToast } = vi.hoisted(() => ({
  setPtt: vi.fn(async () => ({})),
  setTxEnabled: vi.fn(async () => ({})),
  pushToast: vi.fn(),
}))

vi.mock('../api', () => ({
  setPtt,
  setTxEnabled,
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
}))
vi.mock('../toast', () => ({
  pushToast,
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

afterEach(() => {
  cleanup()
  setPtt.mockClear()
  setTxEnabled.mockClear()
  pushToast.mockClear()
})

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      sidebandOverride: null,
      rigMode: 'USB',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      qsoRecording: false,
      rfPower: null,
      micGain: null,
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      comp: null,
      vox: null,
      filterWidthHz: null,
      splitTxMhz: null,
      smeterDb: null,
      rxLevel: 0,
      phoneSegLo: null,
      phoneSegHi: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

const renderPhone = (over: Record<string, unknown> = {}) =>
  render(<PhoneCockpit snap={makeSnap(over)} theme="dark" onWorkSpot={() => {}} spots={[]} />)

const ptt = () => document.querySelector('.ph-ptt') as HTMLButtonElement

describe('the Phone PTT button tells the operator WHICH gate stopped him (#81)', () => {
  it('state 1 — permitted and TX enabled: PUSH TO TALK, and the press reaches the wire', async () => {
    renderPhone()
    const b = ptt()
    expect(b.textContent).toMatch(/PUSH TO TALK/i)
    expect(b.disabled).toBe(false)

    await act(async () => {
      fireEvent.pointerDown(b)
    })
    expect(setPtt).toHaveBeenCalledWith(true)
    expect(ptt().textContent, 'a real key must show ON AIR').toMatch(/ON AIR/i)
  })

  it('state 2 — permitted but TX is OFF: it says so, does not fake ON AIR, and offers the way back', async () => {
    // THE #81 STATE. Stop TX / the TX watchdog / a UDP HaltTx all leave tx_enabled false
    // while privileges are fine, and nothing in the Phone cockpit said a word about it.
    renderPhone({ txEnabled: false })
    const b = ptt()

    // (a) The label names the real reason. Not "PUSH TO TALK" (a promise the wire will
    //     break) and not the licence lock (a different fact with a different remedy).
    expect(b.textContent, 'the button still promises to talk while TX is off').not.toMatch(
      /PUSH TO TALK/i,
    )
    expect(b.textContent, 'the button does not say TX is off').toMatch(/TX (is )?OFF/i)
    expect(b.textContent, 'TX-off is being reported as a licence lock').not.toMatch(/LOCKED/i)

    // (b) The title says what to do about it, and the mic sentence survives (structure test).
    const title = b.getAttribute('title') ?? ''
    expect(title, 'the title does not name the TX switch the operator has to flip').toMatch(
      /enable|turn (it |tx )?on|arm/i,
    )

    // (c) Pressing it must not lie: no phantom ON AIR, and no key request the wire will
    //     silently drop.
    await act(async () => {
      fireEvent.pointerDown(b)
    })
    expect(ptt().textContent, 'the button claimed ON AIR over a rig that is not keyed').not.toMatch(
      /ON AIR/i,
    )
    expect(ptt().classList.contains('keyed'), 'the button lit up keyed while TX is off').toBe(false)
    expect(setPtt, 'a doomed key request went to the wire').not.toHaveBeenCalledWith(true)

    // (d) RECOVERABLE. Phone has no other Enable-Tx control on screen, so the press that
    //     found TX off must arm it — the operator presses again and talks.
    expect(setTxEnabled, 'TX off in Phone is a dead end — nothing on screen re-arms it').toHaveBeenCalledWith(
      true,
    )
    expect(pushToast, 'nothing told the operator what just happened').toHaveBeenCalled()

    await act(async () => {
      fireEvent.pointerUp(b)
    })
  })

  it('state 3 — not permitted: the licence lock is unchanged and stays distinct', () => {
    renderPhone({ txAllowed: false })
    const b = ptt()
    expect(b.textContent).toMatch(/TX LOCKED/i)
    expect(b.disabled, 'the licence lock must stay a disabled control').toBe(true)
    expect(b.getAttribute('title') ?? '').toMatch(/privileg/i)
  })

  it('the three states are three different sentences — none of them is reused', () => {
    // The gap #81 fell through was one label doing duty for two states. Recompute it here
    // rather than trusting the three cases above to stay different by accident.
    const labels = [
      renderPhone().container && ptt().textContent,
      (cleanup(), renderPhone({ txEnabled: false }).container && ptt().textContent),
      (cleanup(), renderPhone({ txAllowed: false }).container && ptt().textContent),
    ]
    expect(new Set(labels).size, `two states share a label: ${JSON.stringify(labels)}`).toBe(3)
  })
})
