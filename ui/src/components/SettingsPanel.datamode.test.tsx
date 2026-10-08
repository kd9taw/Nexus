// @vitest-environment jsdom
//
// THE D1/D2/D3 PICKER IS FOR THE ONE ICOM THAT HAS A CHOICE.
//
// `1A 06` selects DATA1, DATA2 or DATA3 on the IC-7610. The IC-7300, IC-9700, IC-705 and IC-905
// have one DATA mode, and Nexus's own CI-V connection now sends it whatever is saved (tempo-audio's
// `commands::data_mode_count`). So a D2 or D3 saved on one of them, while the picker was still
// offered there, no longer reaches the radio, and a picker kept on screen for it would be a control
// that does nothing. These render the panel and read what the operator sees.
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
  rigctldPort: 4532,
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

/** One radio on Nexus's own CI-V connection, as the backend returns it (flat mirror included). */
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

/** Open Radio ▸ Rig & CAT ▸ Advanced, where the native CI-V controls live, and hand back the
 *  Data mode picker, or null when it is not offered. */
async function pickerOnScreen(): Promise<HTMLSelectElement | null> {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  const grp = document.querySelector('#settings-rig-advanced .settings-group-toggle') as HTMLButtonElement | null
  expect(grp, 'the Advanced group exists in Rig & CAT').toBeTruthy()
  if (grp!.getAttribute('aria-expanded') === 'false') fireEvent.click(grp!)
  expect(
    [...document.querySelectorAll('.settings-label')].some((e) => /Native Icom CI-V/.test(e.textContent ?? '')),
    'the native CI-V controls are on screen, so an absent picker is an answer',
  ).toBe(true)
  const label = [...document.querySelectorAll('.settings-label')].find((e) => e.textContent?.trim() === 'Data mode')
  return (label?.parentElement?.querySelector('select') as HTMLSelectElement | null) ?? null
}

describe('the D1/D2/D3 picker', () => {
  it('is not offered on an IC-9700, even with a D2 or D3 saved there', async () => {
    for (const d of [2, 3]) {
      renderPanel(settingsFor({ icomDataMode: d }))
      expect(await pickerOnScreen(), `IC-9700 holding D${d}`).toBeNull()
      cleanup()
    }
  })

  it('is offered on an IC-7610, showing the saved choice', async () => {
    renderPanel(settingsFor({ rigModel: 3078, rigModelName: 'Icom IC-7610', icomDataMode: 2 }))
    const picker = await pickerOnScreen()
    expect(picker, 'IC-7610').not.toBeNull()
    expect(picker!.value).toBe('2')
  })
})
