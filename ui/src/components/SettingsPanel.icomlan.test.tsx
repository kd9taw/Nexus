// @vitest-environment jsdom
//
// The Icom network connection (LAN / Wi-Fi, Beta) in Settings ▸ Radio ▸ Rig & CAT.
//
// Pinned here: the choice is offered for the six network Icoms only and never picked for the
// operator (USB stays the default); picking it swaps Serial Port and Baud for the radio address,
// the network user, the password and the control port; the password goes to the keychain verb
// alone, never into a settings save; and the per-radio fields travel in the radio patch, the seam
// that has dropped per-radio fields on Save before.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel, radioPatch } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module.
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

/** A made-up password: an obvious fake. */
const PASSWORD = 'not-a-password'

const IC7760 = {
  id: 0,
  name: 'IC-7760',
  enabled: true,
  serialPort: 'COM3',
  baud: 38400,
  rigModel: 3092,
  rigModelName: 'Icom IC-7760',
  rigConn: 'serial',
  rigAddr: '',
  omnirigSlot: 1,
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
  icomLanHost: '',
  icomLanUser: '',
  icomLanPort: 50001,
}

/** The NON-ACTIVE radio: the same model, already on the network connection. */
const LAN = {
  ...IC7760,
  id: 1,
  name: 'Shack 7760',
  rigConn: 'icomlan',
  icomLanHost: '192.0.2.10',
  icomLanUser: 'test-user',
  icomLanPort: 50011,
  rigctldPort: 4534,
  rotctldPort: 4535,
}

const YAESU = { ...IC7760, rigModel: 1042, rigModelName: 'Yaesu FTDX10' }

function settingsWith(active: typeof IC7760, radios: (typeof IC7760)[]) {
  return {
    ...defaultSettings,
    ...active,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
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

function renderPanel() {
  return render(
    <SettingsPanel
      activeRadioId={0}
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

const labels = () => Array.from(document.querySelectorAll('.settings-label')).map((n) => n.textContent)

function connectionSelect(): HTMLSelectElement {
  const label = Array.from(document.querySelectorAll('.settings-label')).find(
    (n) => n.textContent === 'Connection',
  )
  const select = label?.parentElement?.querySelector('select')
  expect(select, 'precondition: the Connection control is on screen').toBeTruthy()
  return select as HTMLSelectElement
}

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
  api.get('icomLanPasswordSaved').mockImplementation(() => Promise.resolve(false))
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith(IC7760, [IC7760, LAN])))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('the Icom network connection is offered for the six network Icoms', () => {
  it('is offered for an IC-7760 and USB stays the selected default', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const option = (await screen.findByRole('option', { name: /Icom network/ })) as HTMLOptionElement
    expect(option.value).toBe('icomlan')
    expect(connectionSelect().value, 'nothing picks it for the operator').toBe('serial')
  })

  it('is not offered for a radio with no network server', async () => {
    api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith(YAESU, [YAESU])))
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    // The control: the Connection control is there, with its usual choices.
    await screen.findByRole('option', { name: /Windows only/i })
    const values = Array.from(connectionSelect().options).map((o) => o.value)
    expect(values).toEqual(expect.arrayContaining(['serial', 'network', 'omnirig']))
    expect(values).not.toContain('icomlan')
  })

  it('swaps Serial Port and Baud for the network fields', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    // Control first: on serial, the serial fields are there and the network ones are not.
    expect(labels()).toContain('Serial Port')
    expect(labels()).not.toContain('Radio address')
    fireEvent.change(connectionSelect(), { target: { value: 'icomlan' } })
    await waitFor(() => expect(labels()).toContain('Radio address'))
    expect(labels()).toEqual(
      expect.arrayContaining(['Network user', 'Network password', 'Control port (UDP)']),
    )
    expect(labels()).not.toContain('Serial Port')
    expect(labels()).not.toContain('Baud')
  })
})

describe('the network password', () => {
  it('goes to the keychain verb alone, never into a settings save', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    fireEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    expect(await screen.findByText(/Editing Shack 7760/)).toBeTruthy()
    expect(await screen.findByText(/No password is saved for this radio yet/)).toBeTruthy()
    const field = document.querySelector('input[type="password"][maxlength="16"]') as HTMLInputElement
    expect(field, 'precondition: the network password field').toBeTruthy()
    fireEvent.change(field, { target: { value: PASSWORD } })
    const set = screen.getAllByRole('button', { name: 'Set' }).find((b) => !(b as HTMLButtonElement).disabled)!
    fireEvent.click(set)
    await waitFor(() => expect(api.get('setIcomLanPassword')).toHaveBeenCalledWith(1, PASSWORD))
    expect(await screen.findByText(/A password is saved for this radio/)).toBeTruthy()
    expect(field.value, 'the field empties once it is stored').toBe('')
    // A Save of the form now carries no password anywhere.
    const save = screen
      .getAllByRole('button', { name: 'Save' })
      .find((b) => (b as HTMLButtonElement).type === 'submit')!
    fireEvent.click(save)
    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    for (const verb of ['updateRadioProfile', 'setSettings']) {
      for (const call of api.get(verb).mock.calls) {
        expect(JSON.stringify(call), verb).not.toContain(PASSWORD)
      }
    }
    // The control: the same search finds the password where it does travel.
    expect(JSON.stringify(api.get('setIcomLanPassword').mock.calls)).toContain(PASSWORD)
  })
})

describe('the network fields travel with the radio they belong to', () => {
  it('radioPatch carries them, with the standard port when unset', () => {
    const p = radioPatch({ rigConn: 'icomlan', icomLanHost: '192.0.2.10', icomLanUser: 'u', icomLanPort: 50011 })
    expect([p.icomLanHost, p.icomLanUser, p.icomLanPort]).toEqual(['192.0.2.10', 'u', 50011])
    expect(radioPatch({}).icomLanPort).toBe(50001)
    expect(radioPatch({}).rigConn).toBe('serial')
  })

  it('saving a NON-ACTIVE network radio keeps its address, user and port', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    fireEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    expect(await screen.findByText(/Editing Shack 7760/)).toBeTruthy()
    const save = screen
      .getAllByRole('button', { name: 'Save' })
      .find((b) => (b as HTMLButtonElement).type === 'submit')!
    fireEvent.click(save)
    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    const calls = api.get('updateRadioProfile').mock.calls
    const [id, patch] = calls[calls.length - 1] as [number, Record<string, unknown>]
    expect(id).toBe(1)
    expect([patch.rigConn, patch.icomLanHost, patch.icomLanUser, patch.icomLanPort]).toEqual([
      'icomlan',
      '192.0.2.10',
      'test-user',
      50011,
    ])
  })
})
