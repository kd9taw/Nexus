// @vitest-environment jsdom
//
// The TCI connection (Beta) in Settings ▸ Radio ▸ Rig & CAT: an SDR program's own TCI server.
//
// Pinned here: the choice is offered for every radio and never picked for the operator; picking it
// shows the server's address, port, transceiver and program, the three switches and Find TCI
// server, and puts away the Rig Model, Serial Port and Baud; it stores TCI's own model number and
// leaving it takes that number off again; the section says plainly what works on it and what is
// not ready yet; Find TCI server asks for the address in the form and fills in the port it found;
// and every field travels in the radio patch, the seam that has dropped per-radio fields on Save
// before.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { TciProbeResult } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'

// The house budget: a loaded full suite has run cases far slower than one core.
vi.setConfig({ testTimeout: 15_000 })

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
  tciHost: '127.0.0.1',
  tciPort: 50001,
  tciTrx: 0,
  tciDialect: 'auto',
  tciAudio: true,
  tciPan: false,
  tciSpots: false,
}

/** The NON-ACTIVE radio the tests edit: an SDR program reached over its CAT server today. */
const HL2 = {
  ...FTDX10,
  id: 1,
  name: 'HL2',
  rigModel: 2054,
  rigModelName: 'Thetis (Hermes Lite 2 / ANAN / HPSDR)',
  rigConn: 'network',
  rigAddr: '127.0.0.1:13013',
  serialPort: '',
  rigctldPort: 4534,
  rotctldPort: 4535,
}

/** The same radio already on TCI, as a save of this form stores it. */
const HL2_TCI = { ...HL2, rigConn: 'tci', rigModel: 7, rigModelName: 'TCI', tciPort: 40001, tciDialect: 'pihpsdr' }

function settingsWith(radios: (typeof FTDX10)[]) {
  return {
    ...defaultSettings,
    ...FTDX10,
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

/** The control a `.settings-label` names. */
function control<T extends HTMLElement>(label: string, selector = 'select'): T {
  const at = Array.from(document.querySelectorAll('.settings-label')).find((n) => n.textContent === label)
  const el = at?.parentElement?.querySelector(selector)
  expect(el, `precondition: the ${label} control is on screen`).toBeTruthy()
  return el as T
}

/** Open the Radio tab and edit the non-active radio. */
async function editHl2() {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  fireEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
  expect(await screen.findByText(/Editing HL2/)).toBeTruthy()
}

/** Save, and the patch the edited radio was saved with. */
async function savedPatch(): Promise<Record<string, unknown>> {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
  await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
  const calls = api.get('updateRadioProfile').mock.calls
  const [id, patch] = calls[calls.length - 1] as [number, Record<string, unknown>]
  expect(id).toBe(1)
  return patch
}

const TCI_LABELS = [
  'SDR program address',
  'TCI port',
  'TCI server',
  'Transceiver',
  'SDR program',
  'Audio over TCI',
  'SDR panadapter in the scope',
  'Spots on the SDR panadapter',
]

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
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith([FTDX10, HL2])))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('the TCI connection is offered for every radio', () => {
  it('is offered, and Serial stays the selected default', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const option = (await screen.findByRole('option', { name: /^TCI/ })) as HTMLOptionElement
    expect(option.value).toBe('tci')
    expect(control<HTMLSelectElement>('Connection').value, 'nothing picks it for the operator').toBe('serial')
  })

  it('swaps the model, Serial Port and Baud for the server and its switches', async () => {
    await editHl2()
    // The control: on Network, the model is asked and none of the TCI fields shows.
    expect(labels()).toContain('Rig Model')
    for (const label of TCI_LABELS) expect(labels(), label).not.toContain(label)
    fireEvent.change(control('Connection'), { target: { value: 'tci' } })
    await waitFor(() => expect(labels()).toContain('SDR program address'))
    expect(labels()).toEqual(expect.arrayContaining(TCI_LABELS))
    for (const gone of ['Rig Model', 'Serial Port', 'Baud', 'Network Address']) {
      expect(labels(), gone).not.toContain(gone)
    }
  })

  it('says that Nexus transmits on it, and what is not ready yet', async () => {
    await editHl2()
    fireEvent.change(control('Connection'), { target: { value: 'tci' } })
    const hint = await waitFor(() => {
      const el = document.querySelector('[data-tci-hint]')
      expect(el).toBeTruthy()
      return el!.textContent ?? ''
    })
    expect(hint).toMatch(/Beta/)
    expect(hint).toMatch(/It reads and sets the dial and mode and transmits through the program\./)
    expect(hint).toMatch(/Not ready yet: CW through the program's own keyer \(use the Soundcard keyer\)\./)
    // Nexus's spots on the program's panadapter work, so the hint no longer lists them.
    expect(hint).not.toMatch(/panadapter/)
    expect(hint).not.toMatch(/does not transmit/)
  })
})

describe('the model number', () => {
  it('a radio saved on TCI stores TCI’s own model, with the server’s defaults', async () => {
    await editHl2()
    fireEvent.change(control('Connection'), { target: { value: 'tci' } })
    await waitFor(() => expect(labels()).toContain('SDR program address'))
    const patch = await savedPatch()
    expect([patch.rigConn, patch.rigModel, patch.rigModelName]).toEqual(['tci', 7, 'TCI'])
    expect([patch.tciHost, patch.tciPort, patch.tciTrx, patch.tciDialect]).toEqual(['127.0.0.1', 50001, 0, 'auto'])
    expect([patch.tciAudio, patch.tciPan, patch.tciSpots]).toEqual([true, false, false])
  })

  it('leaving TCI takes the number off, so the model is asked again', async () => {
    api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith([FTDX10, HL2_TCI])))
    await editHl2()
    expect(labels(), 'precondition: on TCI the model is not asked').not.toContain('Rig Model')
    fireEvent.change(control('Connection'), { target: { value: 'network' } })
    await waitFor(() => expect(labels()).toContain('Rig Model'))
    const patch = await savedPatch()
    expect([patch.rigConn, patch.rigModel, patch.rigModelName]).toEqual(['network', 0, ''])
  })

  it('a model that is not TCI’s stays when the connection changes', async () => {
    await editHl2()
    fireEvent.change(control('Connection'), { target: { value: 'serial' } })
    await waitFor(() => expect(labels()).toContain('Serial Port'))
    const patch = await savedPatch()
    expect([patch.rigConn, patch.rigModel]).toEqual(['serial', 2054])
  })
})

describe('the TCI fields travel with the radio they belong to', () => {
  it('each control writes its own field', async () => {
    api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith([FTDX10, HL2_TCI])))
    await editHl2()
    expect(control<HTMLSelectElement>('SDR program').value, 'the stored program').toBe('pihpsdr')
    expect(control<HTMLInputElement>('TCI port', 'input').value).toBe('40001')
    fireEvent.change(control<HTMLInputElement>('SDR program address', 'input'), { target: { value: '192.168.1.30' } })
    fireEvent.change(control('Transceiver'), { target: { value: '1' } })
    fireEvent.click(control<HTMLInputElement>('Audio over TCI', 'input'))
    fireEvent.click(control<HTMLInputElement>('SDR panadapter in the scope', 'input'))
    fireEvent.click(control<HTMLInputElement>('Spots on the SDR panadapter', 'input'))
    const patch = await savedPatch()
    expect([patch.tciHost, patch.tciPort, patch.tciTrx, patch.tciDialect]).toEqual(['192.168.1.30', 40001, 1, 'pihpsdr'])
    expect([patch.tciAudio, patch.tciPan, patch.tciSpots]).toEqual([false, true, true])
  })

  it('naming a program fills in the port it comes set to, and Automatic leaves the port alone', async () => {
    await editHl2()
    fireEvent.change(control('Connection'), { target: { value: 'tci' } })
    const port = () => control<HTMLInputElement>('TCI port', 'input').value
    await waitFor(() => expect(port()).toBe('50001'))
    fireEvent.change(control('SDR program'), { target: { value: 'pihpsdr' } })
    expect(port()).toBe('40001')
    fireEvent.change(control('SDR program'), { target: { value: 'auto' } })
    expect(port(), 'Automatic says nothing about the port').toBe('40001')
    fireEvent.change(control<HTMLInputElement>('TCI port', 'input'), { target: { value: '12345' } })
    fireEvent.change(control('SDR program'), { target: { value: 'thetis' } })
    expect(port(), 'a port the operator typed is theirs').toBe('12345')
  })

  it('the Transceiver choice shows TCI’s numbers from 1', async () => {
    await editHl2()
    fireEvent.change(control('Connection'), { target: { value: 'tci' } })
    await waitFor(() => expect(labels()).toContain('Transceiver'))
    const options = Array.from(control<HTMLSelectElement>('Transceiver').options)
    expect(options[0]).toMatchObject({ value: '0', textContent: '1' })
    expect(options).toHaveLength(8)
  })
})

describe('Find TCI server', () => {
  const status = () => document.querySelector('[data-tci-find-status]')?.textContent ?? ''

  it('asks for the address in the form, and fills in the port it found', async () => {
    const answer: TciProbeResult = {
      host: '127.0.0.1',
      resolved: true,
      tried: [
        { port: 50001, outcome: 'noAnswer' },
        { port: 40001, outcome: 'found' },
      ],
      found: { port: 40001, program: 'ExpertSDR3', version: '2.0', device: 'SunSDR2QRP', trxCount: 1, dialect: 'expertsdr3' },
    }
    api.get('tciProbe').mockImplementation(() => Promise.resolve(answer))
    await editHl2()
    fireEvent.change(control('Connection'), { target: { value: 'tci' } })
    fireEvent.click(await screen.findByRole('button', { name: 'Find TCI server' }))
    await waitFor(() => expect(status()).toContain('Found a TCI server at 127.0.0.1:40001'))
    expect(api.get('tciProbe')).toHaveBeenCalledWith('127.0.0.1', 50001)
    expect(status()).toContain('ExpertSDR3 2.0, SunSDR2QRP')
    expect(status()).toContain('TCI port is now 40001')
    expect(control<HTMLInputElement>('TCI port', 'input').value).toBe('40001')
    const patch = await savedPatch()
    expect(patch.tciPort).toBe(40001)
  })

  it('says so when nothing answers, and changes nothing', async () => {
    api.get('tciProbe').mockImplementation(() =>
      Promise.resolve({
        host: '192.168.1.30',
        resolved: true,
        tried: [
          { port: 50001, outcome: 'noAnswer' },
          { port: 40001, outcome: 'noAnswer' },
        ],
        found: null,
      } satisfies TciProbeResult),
    )
    await editHl2()
    fireEvent.change(control('Connection'), { target: { value: 'tci' } })
    fireEvent.change(control<HTMLInputElement>('SDR program address', 'input'), { target: { value: ' 192.168.1.30 ' } })
    fireEvent.click(await screen.findByRole('button', { name: 'Find TCI server' }))
    await waitFor(() => expect(status()).toContain('No TCI server answered at 192.168.1.30 (ports tried: 50001, 40001)'))
    expect(api.get('tciProbe')).toHaveBeenCalledWith('192.168.1.30', 50001)
    expect(control<HTMLInputElement>('TCI port', 'input').value).toBe('50001')
  })

  it('says why when it could not look at all', async () => {
    api.get('tciProbe').mockImplementation(() => Promise.reject(new Error('refused from afar')))
    await editHl2()
    fireEvent.change(control('Connection'), { target: { value: 'tci' } })
    fireEvent.click(await screen.findByRole('button', { name: 'Find TCI server' }))
    await waitFor(() => expect(status()).toBe('Could not look for a TCI server: refused from afar'))
  })
})
