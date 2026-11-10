// @vitest-environment jsdom
//
// The amplifier TX delay: how long after the key the radio loop holds Nexus's audio back so an
// amplifier keyed from the radio has switched. It is per radio, so it must ride both the
// station-wide save and the per-radio patch (a per-radio field a save drops is reset on the
// radio), and the form may only ever hold what the radio loop takes: whole 10 ms steps, 0 to 500.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel, ampTxDelayFromInput, radioPatch } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "sends the station-wide write at all — it used to be…", takes
// 0.73 s and 0.74 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module (the flexperradio pattern — a
// hand-kept list makes the panel throw on mount when a verb is added).
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

/** The ACTIVE radio. Its config is what a station-wide write must carry in the flat mirror. */
/** Newest call to a mocked api verb. (`Array.prototype.at` is outside this project's lib
 *  target, so index explicitly.) */
function lastCall(name: string): unknown[] | undefined {
  const calls = api.get(name).mock.calls
  return calls.length ? (calls[calls.length - 1] as unknown[]) : undefined
}

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
  ampModel: 'kpa',
  ampPort: '',
  ampFollowBand: false,
}
/** The NON-ACTIVE radio the operator clicks Edit on — the state the bug lives in. */
const SECOND = {
  ...FTDX10,
  id: 1,
  name: 'IC-7300',
  serialPort: 'COM9',
  rigModel: 3073,
  rigModelName: 'Icom IC-7300',
  rigctldPort: 4534,
  rotctldPort: 4535,
  audioIn: 'in-1',
  audioOut: 'out-1',
}

function settingsFixture() {
  return {
    ...defaultSettings,
    ...FTDX10, // the flat mirror describes the ACTIVE radio
    mycall: 'N0CALL',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10, SECOND],
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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsFixture()))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons. */
function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}


function slider() {
  return screen.findByRole('slider', { name: 'Amplifier TX delay' }) as Promise<HTMLInputElement>
}

describe('Settings: the amplifier TX delay', () => {
  it('shows 0 for a station that never set it, whatever the amplifier model', async () => {
    const plain = { ...FTDX10, ampModel: '' }
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({ ...(settingsFixture() as object), ...plain, radios: [plain, SECOND] } as never),
    )
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const input = await slider()
    expect(input.value).toBe('0')
    expect(input.min).toBe('0')
    expect(input.max).toBe('500')
    expect(input.step).toBe('10')
  })

  it('saves the chosen delay with the station-wide save', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    fireEvent.change(await slider(), { target: { value: '120' } })
    expect(screen.getByText('120 ms')).toBeTruthy()
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const payload = lastCall('setSettings')![0] as Record<string, unknown>
    expect(payload.ampTxDelayMs).toBe(120)
  })

  it('rides the per-radio patch, 0 when the form has none', () => {
    expect(radioPatch({ ampTxDelayMs: 200 }).ampTxDelayMs).toBe(200)
    expect(radioPatch({}).ampTxDelayMs).toBe(0)
  })

  it('holds a typed value to whole 10 ms steps from 0 to 500', () => {
    expect(ampTxDelayFromInput('0')).toBe(0)
    expect(ampTxDelayFromInput('123')).toBe(120)
    expect(ampTxDelayFromInput('125')).toBe(130)
    expect(ampTxDelayFromInput('9000')).toBe(500)
    expect(ampTxDelayFromInput('-50')).toBe(0)
    expect(ampTxDelayFromInput('abc')).toBe(0)
  })
})
