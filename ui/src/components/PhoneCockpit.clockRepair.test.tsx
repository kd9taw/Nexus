// @vitest-environment jsdom
//
// WHILE A CLOCK REPAIR HOLDS TRANSMIT, THE PTT BUTTON SAYS SO (the operator's ruling, 2026-10-06:
// "Yes, hold TX until it finishes"). From the press of Repair clock until the repair ends, the
// engine refuses every key (`Engine::hold_tx_for_clock_repair`). The cockpit must not draw ON AIR
// over a transmitter nobody keyed (#81's lie), nor offer to turn TX back on, which the engine
// refuses as well. It says why, and the same press with no repair running keys (the control).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { EN } from '../i18n/en'

const { setPtt, setTxEnabled, pushToast } = vi.hoisted(() => ({
  setPtt: vi.fn(async () => ({})),
  setTxEnabled: vi.fn(async () => ({})),
  pushToast: vi.fn(),
}))

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({})),
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

async function press() {
  await act(async () => {
    fireEvent.pointerDown(ptt())
  })
}

describe('the Phone PTT while a clock repair holds transmit', () => {
  for (const txEnabled of [true, false]) {
    it(`says why, keys nothing and draws no ON AIR (TX ${txEnabled ? 'on' : 'off'})`, async () => {
      renderPhone({ clockRepairTxHeld: true, txEnabled })
      await press()
      expect(setPtt, 'a key the engine refuses went to the wire').not.toHaveBeenCalledWith(true)
      expect(setTxEnabled, 'it offered to turn TX on, which the engine refuses too').not.toHaveBeenCalled()
      expect(ptt().textContent, 'ON AIR over a transmitter nobody keyed').not.toMatch(/ON AIR/i)
      expect(ptt().classList.contains('keyed')).toBe(false)
      expect(pushToast).toHaveBeenCalledWith(EN['phone.tx.clockRepair'], 'info', 4000)
    })
  }

  it('control: with no repair running, the same press keys', async () => {
    renderPhone({ clockRepairTxHeld: false })
    await press()
    expect(setPtt).toHaveBeenCalledWith(true)
    expect(ptt().textContent).toMatch(/ON AIR/i)
    expect(pushToast).not.toHaveBeenCalledWith(EN['phone.tx.clockRepair'], 'info', 4000)
  })
})
