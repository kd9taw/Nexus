// @vitest-environment jsdom
//
// The scope source in Settings ▸ Radio: the radio's own panadapter as it comes, or an SDRplay
// receiver through SDRconnect. Pinned here: the radio is the default and an unknown stored word
// reads as it; choosing SDRconnect shows its address, port and follow mode with SDRplay's defaults
// and the line about feeding the receiver safely; and Save writes all four as the operator set them.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
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

/** The control a `.settings-label` names. */
function control<T extends HTMLElement>(label: string, selector = 'select'): T {
  const at = Array.from(document.querySelectorAll('.settings-label')).find((n) => n.textContent === label)
  const el = at?.parentElement?.querySelector(selector)
  expect(el, `precondition: the ${label} control is on screen`).toBeTruthy()
  return el as T
}

const labels = () => Array.from(document.querySelectorAll('.settings-label')).map((n) => n.textContent)

const SDRCONNECT_LABELS = ['SDRconnect computer', 'WebSocket API port', 'Follow the dial']

async function openRadioTab() {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await screen.findByText('Draw the scope from')
}

/** Save, and the whole settings it saved. */
async function saved(): Promise<Record<string, unknown>> {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
  await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0] as Record<string, unknown>
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
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith([FTDX10])))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('Settings ▸ Radio ▸ Scope source', () => {
  it('is the radio as it comes, with nothing of SDRconnect on screen', async () => {
    await openRadioTab()
    expect(control<HTMLSelectElement>('Draw the scope from').value).toBe('radio')
    for (const label of SDRCONNECT_LABELS) expect(labels(), label).not.toContain(label)
    expect(document.querySelector('[data-sdrconnect-rf-safety]')).toBeNull()
  })

  it('a word this build does not know shows as the radio, as the backend reads it', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({ ...(settingsWith([FTDX10]) as object), scopeSource: 'tci' } as never),
    )
    await openRadioTab()
    expect(control<HTMLSelectElement>('Draw the scope from').value).toBe('radio')
  })

  it('SDRconnect shows its address, port and follow mode with their defaults, and the RF line', async () => {
    await openRadioTab()
    fireEvent.change(control('Draw the scope from'), { target: { value: 'sdrconnect' } })
    await waitFor(() => expect(labels()).toEqual(expect.arrayContaining(SDRCONNECT_LABELS)))
    expect(control<HTMLInputElement>('SDRconnect computer', 'input').value).toBe('127.0.0.1')
    expect(control<HTMLInputElement>('WebSocket API port', 'input').value).toBe('5454')
    expect(control<HTMLSelectElement>('Follow the dial').value).toBe('centre')
    const rf = document.querySelector('[data-sdrconnect-rf-safety]')?.textContent ?? ''
    expect(rf).toMatch(/RX OUT/)
    expect(rf).toMatch(/0 dBm/)
  })

  it('saves the source, the address, the port and the follow mode the operator set', async () => {
    await openRadioTab()
    fireEvent.change(control('Draw the scope from'), { target: { value: 'sdrconnect' } })
    await waitFor(() => expect(labels()).toContain('WebSocket API port'))
    fireEvent.change(control('SDRconnect computer', 'input'), { target: { value: 'shack-pc' } })
    fireEvent.change(control('WebSocket API port', 'input'), { target: { value: '5455' } })
    fireEvent.change(control('Follow the dial'), { target: { value: 'scroll' } })
    const s = await saved()
    expect([s.scopeSource, s.sdrconnectHost, s.sdrconnectPort, s.sdrconnectFollow]).toEqual([
      'sdrconnect',
      'shack-pc',
      5455,
      'scroll',
    ])
  })
})
