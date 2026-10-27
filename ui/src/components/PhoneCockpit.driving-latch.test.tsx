// @vitest-environment jsdom
//
// THE TX LATCH IN PHONE IS A BUTTON ON A COMPUTER DRIVING A STATION. At the station, entering Phone
// arms TX and the latch beside Tune is read-only. From afar, entering a section arms nothing, so the
// latch read "TX Off" and did nothing when clicked. There it is now the latch every other screen
// has: one click is one `set_tx_enabled`, through the station's own guards (licence, sign-in hold,
// watchdog), and a refusal is shown. CONTROL: at the station it stays read-only.
//
// TUNE AND ATU NEVER FAIL SILENTLY: a refused Tune or ATU press says why.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, fireEvent, act, screen } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'

const { setPtt, setTxEnabled, setTune, atuTune, pushToast, driving } = vi.hoisted(() => ({
  setPtt: vi.fn(async (_on: boolean) => ({})),
  setTxEnabled: vi.fn(async (_on: boolean): Promise<unknown> => ({})),
  setTune: vi.fn(async (_on: boolean): Promise<unknown> => ({})),
  atuTune: vi.fn(async (): Promise<unknown> => ({})),
  pushToast: vi.fn(),
  driving: { value: true },
}))

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({})),
  setPtt,
  holdPttLock: vi.fn(async () => ({ ask: false })),
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
  setTune,
  atuTune,
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
vi.mock('../features/durableStore', async (actual) => ({
  ...(await actual<typeof import('../features/durableStore')>()),
  drivesAnotherStation: () => driving.value,
}))

vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeEach(() => {
  driving.value = true
})

afterEach(() => {
  cleanup()
  for (const f of [setPtt, setTxEnabled, setTune, atuTune, pushToast]) f.mockClear()
  setTxEnabled.mockImplementation(async () => ({}))
  setTune.mockImplementation(async () => ({}))
  atuTune.mockImplementation(async () => ({}))
})

function makeSnap(txEnabled: boolean): AppSnapshot {
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
      txEnabled,
      txAllowed: true,
      tuning: false,
      atu: false,
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
    },
  } as unknown as AppSnapshot
}

const renderPhone = (txEnabled = false) =>
  render(<PhoneCockpit snap={makeSnap(txEnabled)} theme="dark" onWorkSpot={() => {}} spots={[]} />)

const latchButton = () => screen.queryByRole('button', { name: /^tx on$|^tx off$/i })

async function click(el: HTMLElement) {
  await act(async () => {
    fireEvent.click(el)
  })
}

describe('the Phone TX latch on a computer driving a station', () => {
  it('is a button, and one click arms TX once', async () => {
    renderPhone(false)
    const latch = latchButton()
    expect(latch, 'the latch beside Tune is a button from afar').not.toBeNull()
    await click(latch!)
    expect(setTxEnabled.mock.calls).toEqual([[true]])
    expect(setPtt, 'arming keys nothing').not.toHaveBeenCalled()
  })

  it('one click disarms TX once', async () => {
    renderPhone(true)
    await click(latchButton()!)
    expect(setTxEnabled.mock.calls).toEqual([[false]])
  })

  it('shows the station’s refusal', async () => {
    setTxEnabled.mockImplementation(async () => {
      throw 'Transmit is held until this device signs in.'
    })
    renderPhone(false)
    await click(latchButton()!)
    expect(pushToast).toHaveBeenCalledWith('Transmit is held until this device signs in.', 'error')
  })
})

describe('CONTROL: the Phone TX latch at the station', () => {
  it('stays read-only', () => {
    driving.value = false
    renderPhone(false)
    expect(latchButton()).toBeNull()
    expect(document.querySelector('.cockpit-txstrip .op-btn.monitor.readonly')?.textContent).toBe('TX Off')
  })
})

describe('Tune and ATU in Phone', () => {
  for (const at of ['from afar', 'at the station'] as const) {
    it(`show a refused Tune ${at}`, async () => {
      driving.value = at === 'from afar'
      setTune.mockImplementation(async () => {
        throw 'No stop road to the station.'
      })
      renderPhone(true)
      await click(screen.getByRole('button', { name: /^tune$/i }))
      expect(setTune).toHaveBeenCalledWith(true)
      expect(pushToast).toHaveBeenCalledWith('No stop road to the station.', 'error')
    })

    it(`show a refused ATU ${at}`, async () => {
      driving.value = at === 'from afar'
      atuTune.mockImplementation(async () => {
        throw 'TX is off — enable TX first'
      })
      renderPhone(false)
      await click(screen.getByRole('button', { name: /^atu$/i }))
      expect(atuTune).toHaveBeenCalled()
      expect(pushToast).toHaveBeenCalledWith('TX is off — enable TX first', 'error')
    })
  }
})
