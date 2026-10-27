// @vitest-environment jsdom
//
// The Spot button on the Phone cockpit toolbar, as Operate's header has long had it. One
// click opens the reviewable cluster-spot popup (SpotDialog), seeded with the call being logged.
// It brings the phone/SSB operator to parity with Operate/DXped, where the button already lives.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { PHONE_PANELS, usePanelLayout } from '../features/panelState'
import type { AppSnapshot } from '../types'

vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => ({
  getSettings: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => {}),
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
  setTxEnabled: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  openQrzPage: vi.fn(async () => {}),
  readRotator: vi.fn(async () => null),
  pointRotator: vi.fn(async () => {}),
  // RotorStrip fetches magnetic declination once a target call is set (the toolbar's seed path).
  getDeclination: vi.fn(async () => null),
  stopVoice: vi.fn(async () => ({})),
  playVoiceMessage: vi.fn(async () => ({})),
  // SpotDialog posts through this — only ever on submit, which these tests do not do.
  postSpot: vi.fn(async () => {}),
}))
vi.mock('../api', () => api)
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

// The header renders its children (the toolbar) — the one thing this suite needs to see. SpotDialog
// is deliberately NOT mocked: the press must really open it.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ children, actions }: { children?: unknown; actions?: unknown }) => (
    <header className="cockpit-header">
      {actions as never}
      {children as never}
    </header>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
// A button stands in for a call settling in the log form: pressing it hands the typed call up
// through `onCallChange`, which is exactly what the toolbar Spot button seeds from.
vi.mock('./LogEntry', () => ({
  LogEntry: (p: { onCallChange?: (c: string) => void }) => (
    <div data-testid="log-stub">
      <button type="button" onClick={() => p.onCallChange?.('JA2DEF')}>seed-call</button>
    </div>
  ),
}))

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
})
afterEach(() => {
  cleanup()
  localStorage.clear()
  sessionStorage.clear()
  for (const f of Object.values(api)) f.mockClear()
})

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: {
      dialMhz: 14.25,
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
      nrLevel: null,
      agc: null,
      nb: null,
      nr: null,
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

function Live() {
  const panels = usePanelLayout(PHONE_PANELS)
  return <PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} panels={panels} />
}

const spotBtn = () => screen.getByRole('button', { name: 'Spot a callsign to the DX cluster' })

describe('Phone cockpit toolbar Spot button', () => {
  it('renders a Spot button on the toolbar', () => {
    render(<Live />)
    expect(spotBtn()).toBeTruthy()
  })

  it('opens the reviewable cluster-spot popup, seeded with the dial and the logged call', () => {
    render(<Live />)
    // No popup until the button is pressed.
    expect(screen.queryByRole('dialog')).toBeNull()
    // A call settles in the log form, then the operator presses Spot.
    fireEvent.click(screen.getByText('seed-call'))
    fireEvent.click(spotBtn())
    const dialog = screen.getByRole('dialog')
    const inputs = dialog.querySelectorAll('input.settings-input')
    expect((inputs[0] as HTMLInputElement).value).toBe('JA2DEF')
    expect((inputs[1] as HTMLInputElement).value).toBe('14.250')
  })
})
