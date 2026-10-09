// @vitest-environment jsdom
//
// THE CI-V BUS DIAGNOSTIC LOG IS OFFERED BY THE MODEL NUMBER, like the native CI-V switch above it.
//
// The log records Nexus's own CI-V engine on the bus, so it belongs where that engine drives the radio: a
// native model (`NATIVE_CIV_MODELS`, the engine's `icom_scope_model`) with the native switch on, on a
// connection the engine can open, or any radio on the Icom network connection. It was offered by the model
// NAME instead (`/IC-?\s?(7300|7610|9700|705|905)\b/i`), so an IC-7610 stored as `Icom 7610`, `IC-7610M` or
// with no name could turn native CI-V on and never see the log, and a radio NAMED like one of those was
// offered a log with no CI-V engine to record. These render the panel and read what the operator sees.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
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

// Every export of `../api`, derived from the real module (a hand-kept list throws on mount the
// day a verb is added).
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

const IC9700 = {
  id: 0,
  name: 'IC-9700',
  enabled: true,
  serialPort: 'COM7',
  baud: 115200,
  rigModel: 3081,
  rigModelName: 'Icom IC-9700',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4534,
  rotctldPort: 4533,
  icomNativeCat: true,
  icomDataMode: 1,
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
}

/** One radio, as the backend returns it (flat mirror included). */
function settingsFor(radio: Record<string, unknown>) {
  const r = { ...IC9700, ...radio }
  return {
    ...defaultSettings,
    ...r,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [r],
  } as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel(settings: never) {
  api.get('getSettings').mockImplementation(() => Promise.resolve(settings))
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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.17.0'))
})
afterEach(cleanup)

/** Open Radio ▸ Rig & CAT ▸ Advanced and say whether the CI-V log switch is there. The SSTV switch, which
 *  every radio gets, must be on screen first, so an absent log switch is an answer and not a missing render. */
async function logOffered(): Promise<boolean> {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  const grp = document.querySelector('#settings-rig-advanced .settings-group-toggle') as HTMLButtonElement | null
  expect(grp, 'the Advanced group exists in Rig & CAT').toBeTruthy()
  if (grp!.getAttribute('aria-expanded') === 'false') fireEvent.click(grp!)
  const labels = [...document.querySelectorAll('.settings-label')].map((e) => e.textContent?.trim() ?? '')
  expect(labels, 'the SSTV switch is on screen, so an absent log switch is an answer').toContain(
    'Hold the data mode while SSTV is receiving',
  )
  return labels.includes('CI-V bus diagnostic log')
}

/** Render one radio and read the answer, then clear the screen for the next. */
async function offeredFor(radio: Record<string, unknown>): Promise<boolean> {
  renderPanel(settingsFor(radio))
  const offered = await logOffered()
  cleanup()
  return offered
}

describe('the CI-V bus diagnostic log', () => {
  it('is offered on each radio the native switch drives, under its own model name', async () => {
    for (const [rigModel, rigModelName] of [
      [3073, 'Icom IC-7300'],
      [3078, 'Icom IC-7610'],
      [3081, 'Icom IC-9700'],
      [3085, 'Icom IC-705'],
      [3090, 'Icom IC-905'],
    ] as const) {
      expect(await offeredFor({ rigModel, rigModelName }), rigModelName).toBe(true)
    }
  })

  it.each(['Icom 7610', 'IC-7610M', ''])('is offered on an IC-7610 stored under the name "%s"', async (rigModelName) => {
    expect(await offeredFor({ rigModel: 3078, rigModelName })).toBe(true)
  })

  it.each([
    // A Yaesu whose stored name says IC-7300, with the native switch left on: there is no CI-V engine to record.
    ['an FTDX10 stored under the name IC-7300', { rigModel: 1042, rigModelName: 'IC-7300' }],
    // OmniRig holds the COM port, so the CI-V engine never opens it, switch or no switch.
    ['an IC-9700 through OmniRig', { rigConn: 'omnirig' }],
    ['an IC-9700 on Network', { rigConn: 'network', rigAddr: '192.168.1.50:4532' }],
    ['an IC-9700 with the native switch off', { icomNativeCat: false }],
  ])('is not offered on %s', async (_what, radio) => {
    expect(await offeredFor(radio)).toBe(false)
  })

  it('is offered on the Icom network connection, which always drives the radio with the CI-V engine', async () => {
    expect(
      await offeredFor({ rigConn: 'icomlan', rigModel: 3092, rigModelName: 'Icom IC-7760', icomNativeCat: false }),
      'IC-7760 on the Icom network connection',
    ).toBe(true)
  })
})
