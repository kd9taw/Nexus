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

// THE BUDGET (2026-10-09). The slowest case here, "is not offered on an IC-9700, even with a D2 or D3…", takes 0.35 s
// and 0.38 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one
// core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
 *  Data mode picker, or null when it is not offered. `rendered` is a label that must be on screen
 *  first, so an absent picker is an answer and not a missing render: the native CI-V switch, or for
 *  a radio that has none (the IC-7760 and IC-7300MK2), the SSTV switch beside it. */
async function pickerOnScreen(rendered: RegExp = /Native Icom CI-V/): Promise<HTMLSelectElement | null> {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  const grp = document.querySelector('#settings-rig-advanced .settings-group-toggle') as HTMLButtonElement | null
  expect(grp, 'the Advanced group exists in Rig & CAT').toBeTruthy()
  if (grp!.getAttribute('aria-expanded') === 'false') fireEvent.click(grp!)
  expect(
    [...document.querySelectorAll('.settings-label')].some((e) => rendered.test(e.textContent ?? '')),
    `${rendered} is on screen, so an absent picker is an answer`,
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

// THE ICOM NETWORK CONNECTION DRIVES THE RADIO WITH NEXUS'S OWN CI-V, whatever the native switch says, and
// it applies the saved D1/D2/D3 there too. So the picker follows each radio's own DATA count (the IC-7760
// has three, A7788-8EX-2, PDF p. 23; the IC-7300MK2 one, rev 0, PDF p. 22) and can be changed on that
// connection. Over USB, Hamlib drives an IC-7760 and always selects D1, and the radio has no native switch
// to turn on, so there the picker says which connection makes it work.
describe('the D1/D2/D3 picker on the Icom network connection', () => {
  const SSTV = /Hold the data mode while SSTV/
  const net = { rigConn: 'icomlan', icomNativeCat: false }

  it('is offered on an IC-7760 and an IC-7610, showing the saved choice, and can be changed', async () => {
    for (const [rigModel, rigModelName] of [[3092, 'Icom IC-7760'], [3078, 'Icom IC-7610']] as const) {
      renderPanel(settingsFor({ ...net, rigModel, rigModelName, icomDataMode: 2 }))
      const picker = await pickerOnScreen(SSTV)
      expect(picker, rigModelName).not.toBeNull()
      expect(picker!.value, rigModelName).toBe('2')
      expect(picker!.disabled, `${rigModelName}: the network connection applies it`).toBe(false)
      cleanup()
    }
  })

  it('is not offered on an IC-7300MK2, even with a D2 or D3 saved there', async () => {
    for (const d of [2, 3]) {
      renderPanel(settingsFor({ ...net, rigModel: 3094, rigModelName: 'Icom IC-7300MK2', icomDataMode: d }))
      expect(await pickerOnScreen(SSTV), `IC-7300MK2 holding D${d}`).toBeNull()
      cleanup()
    }
  })

  it('is greyed on an IC-7760 over USB, naming the connection that applies it, even with a native switch left on', async () => {
    renderPanel(settingsFor({ rigModel: 3092, rigModelName: 'Icom IC-7760', rigConn: 'serial', icomNativeCat: true, icomDataMode: 2 }))
    const picker = await pickerOnScreen(SSTV)
    expect(picker, 'IC-7760 over USB').not.toBeNull()
    expect(picker!.disabled, 'Hamlib drives it over USB and always selects D1').toBe(true)
    expect(picker!.parentElement?.querySelector('.settings-hint')?.textContent).toMatch(/Icom network connection/)
  })
})
