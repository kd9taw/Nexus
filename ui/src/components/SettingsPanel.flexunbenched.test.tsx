// @vitest-environment jsdom
//
// THE FLEX CLIENT'S BETA SETTING FOR CW, TUNE AND THE ATU (operator ruling, 2026-10-14: "A Beta
// setting, off by default"). Nexus's own Flex client refuses those three starts until a tester's
// bench confirms them; this per-radio switch lets the operator turn them on before then.
//
// What the panel owes it, pinned here: the switch shows wherever the client can run (a Flex
// profile, the client switched on, the radio's address set — on any CAT connection, Serial
// included, since the client talks to the radio at its own address) and is hidden everywhere else; it reads off for a radio that has never stated it; a click turns it on
// and Save sends it, for the radio in use and for one being edited; and it is changed only at the
// shack, never by a press that comes through the Remote stream. The presses through the stream come
// from the REAL stream dispatcher, as LanStation's test makes them, each paired with the same press
// at the shack, which goes through.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel, radioPatch } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import defaultSettings from './__fixtures__/defaultSettings.json'

// The house budget for a panel render on a loaded box (see SettingsPanel.flexperradio.test.tsx).
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module, as the panel's other suites do.
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
const toast = vi.hoisted(() => ({ pushToast: vi.fn() }))
vi.mock('../toast', () => ({
  pushToast: toast.pushToast,
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

const LABEL = 'Let Nexus send CW, Tune and ATU through the native Flex client (not yet bench-tested)'

const FTDX10 = {
  id: 0,
  name: 'FTDX10',
  enabled: true,
  serialPort: 'COM3',
  baud: 38400,
  rigModel: 1042,
  rigModelName: 'Yaesu FTDX10',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: false,
  audioIn: 'in-0',
  audioOut: 'out-0',
  txLevel: 1,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
  flexRadioIp: '',
  flexNativePan: false,
  flexNativeAudio: false,
  flexNativeCat: false,
}
/** A Flex the client serves: on the network, the client switched on, its address set. The Beta
 *  setting is not stated at all, as on every radio saved before it existed. */
const FLEX = {
  ...FTDX10,
  id: 1,
  name: 'FLEX-6400',
  serialPort: '',
  rigModel: 2036,
  rigModelName: 'FlexRadio FLEX-6xxx (SmartSDR CAT)',
  rigConn: 'network',
  rigAddr: '127.0.0.1:5002',
  rigctldPort: 4534,
  rotctldPort: 4535,
  audioIn: 'in-1',
  audioOut: 'out-1',
  flexRadioIp: '192.0.2.77',
  flexNativeCat: true,
}

/** The station with `active` in use (the flat mirror describes it) and both radios in the roster. */
function settingsWith(active: typeof FLEX, flex: typeof FLEX = FLEX) {
  const radios = active.id === 0 ? [active, flex] : [FTDX10, active]
  return {
    ...defaultSettings,
    ...active,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: active.id,
    radios,
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
  } as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** The panel on Radio ▸ Rig & CAT with its Advanced group open, where the Flex switches live. */
async function renderAdvanced(settings: unknown, activeRadioId: number) {
  api.get('getSettings').mockImplementation(() => Promise.resolve(settings))
  render(
    <SettingsPanel
      activeRadioId={activeRadioId}
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
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await screen.findByText('Advanced')
  const group = document.getElementById('settings-rig-advanced')
  expect(group, 'the Rig & CAT Advanced group renders').not.toBeNull()
  fireEvent.click(group!.querySelector('button.settings-group-toggle')!)
  // The control every visibility case below is measured against: the group really is open.
  await screen.findByText('rigctld TCP Port')
}

const theSwitch = () => {
  const label = screen.queryByText(LABEL)
  return (label?.closest('label')?.querySelector('[role="switch"]') ?? null) as HTMLButtonElement | null
}

/** The footer Save (`type="submit"`); other sections carry their own "Save …" buttons. */
const save = () =>
  fireEvent.click(
    screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
  )

// jsdom never lays out: `elementFromPoint` does not exist. Each streamed press says what is under it.
let under: Element | null = null
let stream: StreamInputDispatcher | null = null
const pointer = (action: 'down' | 'up') =>
  ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
    modifiers: 0, pointerType: 'mouse', clicks: 1 })

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  toast.pushToast.mockClear()
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
  under = null
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
  stream = new StreamInputDispatcher(window)
})
afterEach(() => {
  stream?.dispose()
  stream = null
  cleanup()
  delete (document as { elementFromPoint?: unknown }).elementFromPoint
  vi.restoreAllMocks()
})

describe("the Flex client's Beta setting for CW, Tune and the ATU", () => {
  it('shows only where the client is the radio\'s connection, off for a radio that never stated it', async () => {
    await renderAdvanced(settingsWith(FLEX), 1)
    const sw = theSwitch()
    expect(sw, 'shown on a Flex the client serves').not.toBeNull()
    expect(sw!.getAttribute('aria-checked')).toBe('false')
  })

  it.each([
    ['a radio that is not a Flex', { ...FTDX10, flexNativeCat: true, flexRadioIp: '192.0.2.77' }, 0],
    [
      'a network radio that is not a Flex',
      { ...FTDX10, rigModel: 3073, rigConn: 'network', rigAddr: '192.0.2.5:4532', flexNativeCat: true, flexRadioIp: '192.0.2.77' },
      0,
    ],
    ['a Flex on SmartSDR CAT (the client off)', { ...FLEX, flexNativeCat: false }, 1],
    ['a Flex with the client on and no radio address', { ...FLEX, flexRadioIp: '  ' }, 1],
    ['a Flex on Serial with the client off', { ...FLEX, rigConn: 'serial', flexNativeCat: false }, 1],
  ])('is hidden on %s', async (_name, radio, id) => {
    await renderAdvanced(settingsWith(radio as typeof FLEX), id as number)
    expect(theSwitch()).toBeNull()
  })

  // The client runs on a Serial (SmartSDR CAT COM port) profile too, and honours this setting
  // there (`flex_client_unbenched` has no connection term). Hidden, a setting turned on over the
  // network could never be turned off again from Serial.
  it('shows on a Flex on Serial where the client runs, and turns off there', async () => {
    await renderAdvanced(settingsWith({ ...FLEX, rigConn: 'serial', rigAddr: '', flexNativeUnbenched: true } as typeof FLEX), 1)
    const sw = theSwitch()
    expect(sw, 'shown on a Flex on Serial with the client on').not.toBeNull()
    expect(sw!.disabled).toBe(false)
    expect(sw!.getAttribute('aria-checked')).toBe('true')
    fireEvent.click(sw!)
    expect(theSwitch()!.getAttribute('aria-checked')).toBe('false')
  })

  it('a click at the shack turns it on, and Save sends it for the radio in use', async () => {
    await renderAdvanced(settingsWith(FLEX), 1)
    fireEvent.click(theSwitch()!)
    expect(theSwitch()!.getAttribute('aria-checked')).toBe('true')
    save()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const calls = api.get('setSettings').mock.calls
    const saved = calls[calls.length - 1][0] as Record<string, unknown>
    expect(saved.activeRadio).toBe(1)
    expect(saved.flexNativeUnbenched).toBe(true)
  })

  it('saving a Flex that is not in use sends its own setting in the per-radio patch', async () => {
    const stated = { ...FLEX, flexNativeUnbenched: true }
    await renderAdvanced(settingsWith(FTDX10 as typeof FLEX, stated), 0)
    expect(theSwitch(), 'precondition: the FTDX10 in use shows no switch').toBeNull()
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }))
    expect(await screen.findByText(/Editing FLEX-6400/)).toBeTruthy()
    expect(theSwitch()!.getAttribute('aria-checked')).toBe('true')
    fireEvent.click(theSwitch()!)
    expect(theSwitch()!.getAttribute('aria-checked')).toBe('false')
    save()
    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    const calls = api.get('updateRadioProfile').mock.calls
    const [id, patch] = calls[calls.length - 1] as [number, Record<string, unknown>]
    expect(id).toBe(1)
    expect(patch.flexNativeUnbenched).toBe(false)
  })

  it('radioPatch states it both ways, and off for a radio that never stated it', () => {
    expect(radioPatch({ flexNativeUnbenched: true }).flexNativeUnbenched).toBe(true)
    expect(radioPatch({ flexNativeUnbenched: false }).flexNativeUnbenched).toBe(false)
    expect(radioPatch({}).flexNativeUnbenched).toBe(false)
  })

  it('only at the shack: a press through the Remote stream changes nothing, and says why', async () => {
    await renderAdvanced(settingsWith(FLEX), 1)
    under = theSwitch()
    act(() => {
      stream!.handle(pointer('down') as never)
      stream!.handle(pointer('up') as never)
    })
    await act(async () => {
      await Promise.resolve()
    })
    expect(theSwitch()!.getAttribute('aria-checked'), 'a streamed press turned it on').toBe('false')
    expect(toast.pushToast).toHaveBeenCalledWith(
      'Only at the station itself: this can’t be done through a stream.',
      'error',
    )
    // CONTROL: the same press at the shack goes through.
    fireEvent.click(theSwitch()!)
    expect(theSwitch()!.getAttribute('aria-checked')).toBe('true')
    // And turning it off is the shack's alone too.
    toast.pushToast.mockClear()
    under = theSwitch()
    act(() => {
      stream!.handle(pointer('down') as never)
      stream!.handle(pointer('up') as never)
    })
    expect(theSwitch()!.getAttribute('aria-checked'), 'a streamed press turned it off').toBe('true')
    expect(toast.pushToast).toHaveBeenCalledTimes(1)
  })
})
