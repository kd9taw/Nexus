// @vitest-environment jsdom
//
// THE HIGH-SWR CUTOFF'S SETTINGS CONTROL (operator request + ruling, 2026-09-14).
//
// The engine holds the guarantee — it refuses the cutoff outright on a radio whose SWR scale is
// not verified, whatever the UI does. What this file checks is the OPERATOR'S HALF: that the
// control is off out of the box, that it is not offered at all where the number cannot be
// trusted, and that when it is not offered the panel says WHY rather than just greying out.
//
// #292 is the reason it matters. A Xiegu speaks CI-V, so it takes Icom's meter curve, and one
// reading 1.2:1 on its own meter was reported as 6:1 here. Offered a 2.5:1 cutoff, that operator
// would be unkeyed on every over they ever sent, with nothing on screen explaining it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { RadioStatus } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module — see SettingsPanel.txlevel.test.tsx
// for why a hand-kept list is the wrong shape here.
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

function settingsDoc() {
  const radio = {
    id: 0,
    name: 'Radio 1',
    enabled: true,
    serialPort: 'COM3',
    baud: 38400,
    rigModel: 3081,
    rigModelName: 'Icom IC-9700',
    rigConn: 'serial',
    rigAddr: '',
    rigctldPort: 4532,
    rotctldPort: 4533,
    icomNativeCat: true,
    audioIn: 'in-0',
    audioOut: 'out-0',
    txLevel: 0.25,
    rxGain: 1,
    pttMethod: 'cat',
    rotatorModel: 0,
    rotatorPort: '',
    rotatorBaud: 9600,
    rotatorHost: '',
    nativeScope: 'auto',
    bands: [],
  }
  return {
    ...defaultSettings,
    ...radio,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [radio],
  } as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** The live radio status the panel reads the verdict from. `undefined` is the wizard's case. */
function radioStatus(swrScaleVerified: boolean): RadioStatus {
  return { swrScaleVerified } as unknown as RadioStatus
}

function renderPanel(radio?: RadioStatus) {
  return render(
    <SettingsPanel
      activeRadioId={0}
      radio={radio}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'comfortable' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
    />,
  )
}

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsDoc()))
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.0.1'))
})
afterEach(cleanup)

const LABEL = 'Stop transmitting when SWR is high'

async function openRadioTab(radio?: RadioStatus) {
  renderPanel(radio)
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  return await screen.findByRole('switch', { name: LABEL })
}

/** jest-dom is not installed here — `disabled` is read off the element itself. */
const off = (el: Element) => (el as HTMLButtonElement | HTMLInputElement).disabled

/** The threshold box lives beside the switch inside the same field. */
const thresholdBox = () =>
  screen
    .getByRole('switch', { name: LABEL })
    .closest('.settings-field')!
    .querySelector('input[type="number"]') as HTMLInputElement

describe('the high-SWR cutoff is offered only where the number can be trusted', () => {
  it('on a verified radio it is present, switchable, and OFF out of the box', async () => {
    const sw = await openRadioTab(radioStatus(true))
    expect(off(sw)).toBe(false)
    expect(sw.getAttribute('aria-checked'), 'the shipped default is off').toBe('false')
    // The threshold is dead until the operator turns the cutoff on — a number they can edit
    // while the feature does nothing reads as a setting that is in force.
    expect(off(thresholdBox())).toBe(true)

    fireEvent.click(sw)
    expect(screen.getByRole('switch', { name: LABEL }).getAttribute('aria-checked')).toBe('true')
    expect(off(thresholdBox())).toBe(false)
    expect(thresholdBox().value, 'the default threshold').toBe('2.5')
  })

  it('on an unverified radio it is disabled and the panel says why', async () => {
    const sw = await openRadioTab(radioStatus(false))
    expect(off(sw), 'a cutoff on an unscaled reading would unkey for nothing').toBe(true)
    expect(off(thresholdBox())).toBe(true)
    const hint = sw.closest('.settings-field')!.querySelector('.settings-hint')!.textContent ?? ''
    expect(hint, 'the operator is told this is about THIS radio').toContain("isn't verified yet")
    expect(hint, 'and what Nexus does trust').toContain('CI-V')
  })

  it('with no live radio at all it stays disabled — unknown falls on the safe side', async () => {
    // The setup wizard and the Remote projection both render this panel with no `radio`.
    const sw = await openRadioTab(undefined)
    expect(off(sw)).toBe(true)
  })
})
