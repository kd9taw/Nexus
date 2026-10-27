// @vitest-environment jsdom
//
// PHONE'S AND CW'S OWN RIG CONTROLS, REFUSED. AGC, the RF power slider and CW's speed dropped a
// failed call (power and speed with no catch at all), so the slider went on showing a value the rig
// never took. Now a refused power or speed goes back to what the rig reports, wherever it runs, and
// on a computer driving a station the refusal is shown in the words it came with. At the station a
// refused AGC or power stays quiet, as it was: the next read-back shows what the rig took.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import type { ReactNode } from 'react'
import { render, cleanup, fireEvent, act, screen } from '@testing-library/react'
import type { AppSnapshot } from '../types'

const h = vi.hoisted(() => ({ driving: false, n: 0, text: () => `Not connected to the station. (${h.n})` }))

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const refuse = () => Promise.reject(h.text())
  return {
    ...auto,
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
    cwDecode: vi.fn(async () => ({ text: '', wpm: 0, sent: [], candidates: [], state: 'listening' })),
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    setAgc: vi.fn(refuse),
    setRfPower: vi.fn(refuse),
    setCwWpm: vi.fn(refuse),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('../features/durableStore', async (actual) => ({
  ...(await actual<typeof import('../features/durableStore')>()),
  drivesAnotherStation: () => h.driving,
}))
// The header's power slider, as a number box: its value and its onChange are what is under test.
// Its children are the cockpit's own (CW's speed slider), kept.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ power, children }: { power?: { value: number; onChange: (v: number) => void }; children?: ReactNode }) => (
    <header>
      {power && (
        <input aria-label="power" type="number" value={power.value} onChange={(e) => power.onChange(Number(e.target.value))} />
      )}
      {children}
    </header>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

import { pushToast } from '../toast'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'

const toast = pushToast as unknown as ReturnType<typeof vi.fn>
const snap = {
  mycall: 'KD9TAW',
  radio: {
    dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB',
    transmitting: false, txEnabled: false, txAllowed: true, tuning: false, atu: false, qsoRecording: false,
    rfPower: 0.5, micGain: null, nrLevel: 0.3, agc: 'fast', nb: true, nr: true, notch: null, comp: null,
    vox: null, filterWidthHz: null, splitTxMhz: null, smeterDb: null, rxLevel: 0, phoneSegLo: null,
    phoneSegHi: null, cwWpm: 25,
  },
} as unknown as AppSnapshot
const settle = () => act(async () => { await new Promise((r) => setTimeout(r, 0)) })

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  toast.mockClear()
  h.n++
})
afterEach(() => {
  cleanup()
  h.driving = false
})

describe.each([
  ['driving a station', true],
  ['at the station', false],
])('a refused rig control, %s', (_where, driving) => {
  beforeEach(() => {
    h.driving = driving
  })
  const said = () =>
    driving ? expect(toast).toHaveBeenCalledWith(h.text(), 'error') : expect(toast).not.toHaveBeenCalled()

  it("Phone's RF power goes back to what the rig reports", async () => {
    render(<PhoneCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    const power = screen.getByLabelText('power') as HTMLInputElement
    expect(power.value, 'control: the rig reads 50 %').toBe('50')
    fireEvent.change(power, { target: { value: '20' } })
    expect(power.value, 'control: the slider moved').toBe('20')
    await settle()
    expect(power.value).toBe('50')
    said()
  })

  it("Phone's AGC", async () => {
    render(<PhoneCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    fireEvent.click(screen.getByRole('button', { name: 'Slow' }))
    await settle()
    said()
  })

  it("CW's speed goes back to what the rig reports", async () => {
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} />)
    await settle()
    const wpm = screen.getByRole('slider', { name: 'CW keyer speed (WPM)' }) as HTMLInputElement
    expect(wpm.value, 'control: the keyer reads 25').toBe('25')
    fireEvent.change(wpm, { target: { value: '30' } })
    await settle()
    expect(wpm.value).toBe('25')
    said()
  })
})
