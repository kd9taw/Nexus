// @vitest-environment jsdom
//
// THE RADIO'S SCOPE STREAM, MEASURED, READ ONLY in Radio ▸ Rig & CAT ▸ Advanced, beside the CI-V bus
// diagnostic log and on the same radios. While the stream runs it says the sweeps a second, how many were not
// drawn and the radio's scope SPEED as the radio's menu names it; with nothing measured it says only when it
// is measured. These render the panel and read what the operator sees.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { RadioStatus } from '../types'

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

function renderPanel(settings: never, radio?: RadioStatus) {
  api.get('getSettings').mockImplementation(() => Promise.resolve(settings))
  return render(
    <SettingsPanel
      radio={radio}
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

/** Open Radio ▸ Rig & CAT ▸ Advanced and return the scope-stream readout, or null when it is not there. The
 *  SSTV switch, which every radio gets, must be on screen first, so an absent readout is an answer. */
async function readout(): Promise<string | null> {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  const grp = document.querySelector('#settings-rig-advanced .settings-group-toggle') as HTMLButtonElement | null
  expect(grp, 'the Advanced group exists in Rig & CAT').toBeTruthy()
  if (grp!.getAttribute('aria-expanded') === 'false') fireEvent.click(grp!)
  const labels = [...document.querySelectorAll('.settings-label')].map((e) => e.textContent?.trim() ?? '')
  expect(labels).toContain('Hold the data mode while SSTV is receiving')
  const el = document.querySelector('[data-testid="scope-rate"]')
  return el ? (el.textContent ?? '') : null
}

const streaming = (scopeRate: RadioStatus['scopeRate']) => ({ scopeRate }) as unknown as RadioStatus

describe('the radio scope stream readout', () => {
  it('says the measured rate, the sweeps not drawn and the radio\'s SPEED in its own word', async () => {
    renderPanel(settingsFor({}), streaming({ sweepsPerSec: 9.64, dropped: 3, speed: 'fast' }))
    const text = await readout()
    expect(text).toContain('Radio scope stream')
    expect(text).toContain('9.6 sweeps a second · 3 not drawn · scope SPEED FAST on the radio')
  })

  it('says the SPEED was not read when the radio did not answer', async () => {
    renderPanel(settingsFor({}), streaming({ sweepsPerSec: 3, dropped: 0, speed: null }))
    expect(await readout()).toContain('3.0 sweeps a second · 0 not drawn · scope SPEED not read on the radio')
  })

  it('says only when it is measured while nothing is streaming', async () => {
    renderPanel(settingsFor({}), streaming(null))
    const text = await readout()
    expect(text).toContain("Measured while the radio's own scope streams to Nexus")
    expect(text).not.toContain('sweeps a second')
  })

  it('is not offered where no CI-V engine drives the radio', async () => {
    renderPanel(settingsFor({ icomNativeCat: false }), streaming({ sweepsPerSec: 9, dropped: 0, speed: 'mid' }))
    expect(await readout()).toBeNull()
  })
})
