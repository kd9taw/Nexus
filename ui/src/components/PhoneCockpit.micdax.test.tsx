// @vitest-environment jsdom
//
// THE PHONE OPERATOR'S MICROPHONE (2026-08-17 Flex audit, completeness-critic gap #6).
//
// Switching on **Flex native DAX audio** sends `transmit set dax=1` to the radio. That is a
// RADIO-WIDE setting, not a Nexus one: while it stands, the Flex's modulator takes its audio
// from DAX and IGNORES the physical microphone — on every slice, in every program, SmartSDR's
// own MOX included. The toggle exists for FT8, and for FT8 it is exactly right.
//
// The harm is one screen over. An operator who turned it on for digital, then picks up the mic
// to work someone on SSB, transmits SILENCE: PTT is accepted, the meters read whatever they
// read, nothing errors, and the first evidence is nobody coming back. The audit filed the false
// "RX only" LABEL (#48/1045/1011) and that was corrected — but the CONSEQUENCE was never
// surfaced anywhere an operator would meet it.
//
// So Phone says it, in the chip vocabulary already beside the frequency, and the message names
// the remedy rather than the mechanism (the operator does not care about `transmit set dax`;
// they care that the switch is in Settings ▸ Radio ▸ Rig & CAT).
//
// Both directions are asserted. A warning that is always up says nothing, so the negative cases
// — native DAX off, and the field simply absent from an older snapshot — matter as much as the
// positive one.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  setPtt: vi.fn(async () => {}),
  setTxEnabled: vi.fn(async () => ({})),
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
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

// The header stub RENDERS ITS CHILDREN — the chips are passed to CockpitHeader as children, so
// a stub that drops them would make this whole suite pass on nothing. (The sibling Phone suites
// stub it closed on purpose: they assert the shell, not the chips.)
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { children?: React.ReactNode }) => (
    <header className="cockpit-header">{p.children}</header>
  ),
}))
// …and its other child is the rotator strip, which polls rotctld on mount. Not this suite's
// subject, and its poll would be the only thing this file ever timed out on.
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

afterEach(cleanup)

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

const micChip = () =>
  Array.from(document.querySelectorAll('.ph-mode-mismatch')).find((el) =>
    /mic off/i.test(el.textContent ?? ''),
  ) as HTMLElement | undefined

describe('Phone says when native DAX has taken the operator’s microphone', () => {
  it('warns while native DAX audio is live', () => {
    renderPhone({ flexDaxTx: true })
    const chip = micChip()
    expect(chip, 'nothing on the Phone screen says the mic is disconnected').toBeTruthy()
    expect(chip?.textContent).toMatch(/DAX/)
  })

  it('names the remedy, not the mechanism', () => {
    // An operator who reads "transmit set dax=1" learns nothing they can act on. The tooltip
    // has to name the switch AND where it lives, because there is no other way back — the
    // radio keeps this setting until something sends the restore.
    renderPhone({ flexDaxTx: true })
    const title = micChip()?.getAttribute('title') ?? ''
    expect(title, 'the tooltip does not say to turn native DAX audio off').toMatch(
      /turn\s+off\s+flex\s+native\s+dax\s+audio/i,
    )
    expect(title, 'the tooltip does not say where that switch is').toMatch(/settings/i)
    expect(title, 'the tooltip understates the blast radius — this is radio-wide').toMatch(
      /every\s+(slice|program)/i,
    )
  })

  it('says nothing when native DAX audio is not running', () => {
    // The toggle can be ON with no worker at all (no radio address, a failed start), and the
    // worker can be dropped by the RX starvation guard — in every one of those cases
    // `transmit set dax=1` is NOT standing and the mic is the operator's. The snapshot flag
    // mirrors the installed TX tee for exactly that reason, so false here means false there.
    renderPhone({ flexDaxTx: false })
    expect(micChip(), 'a mic warning fired with native DAX audio off').toBeUndefined()
  })

  it('says nothing on a snapshot that has never heard of the field', () => {
    // Everything that is not a Flex on native audio — which is nearly every station.
    renderPhone()
    expect(micChip(), 'the warning fires by default').toBeUndefined()
  })
})
