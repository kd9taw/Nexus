// @vitest-environment jsdom
//
// THE WATERFALL SOURCE IS EACH RADIO'S OWN CHOICE (#417).
//
// On an Icom whose own scope Nexus streams over its CI-V engine, the operator picks which waterfall
// that radio draws: the radio's scope (the default, as every release has) or the sound card's audio
// waterfall, finer close in. It is stored per radio (`nativeScope`, "auto" | "audio"), so these
// render the panel with two radios and read what each Save sends: the radio in use through the flat
// form, a radio opened with Edit through its own patch, and the other radio never touched.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// The house budget for a SettingsPanel render under a loaded suite (see SettingsPanel.datamode.test.tsx).
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module.
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

function lastCall(name: string): unknown[] | undefined {
  const calls = api.get(name).mock.calls
  return calls.length ? (calls[calls.length - 1] as unknown[]) : undefined
}

const IC7300 = {
  id: 0,
  name: 'IC-7300',
  enabled: true,
  serialPort: 'COM7',
  baud: 115200,
  rigModel: 3073,
  rigModelName: 'Icom IC-7300',
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

const IC9700 = {
  ...IC7300,
  id: 1,
  name: 'IC-9700',
  serialPort: 'COM9',
  rigModel: 3081,
  rigModelName: 'Icom IC-9700',
  rigctldPort: 4536,
  rotctldPort: 4537,
}

/** Two radios, the first in use; the flat mirror describes it, as the backend returns it. */
function settingsFor(active: Record<string, unknown>, other: Record<string, unknown> = IC9700) {
  const r = { ...IC7300, ...active }
  return {
    ...defaultSettings,
    ...r,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [r, other],
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.21.0'))
})
afterEach(cleanup)

/** Open Radio ▸ Rig & CAT ▸ Advanced and hand back the Waterfall source picker, or null when it is
 *  not offered. The SSTV switch is always rendered there, so an absent picker is an answer. */
async function pickerOnScreen(): Promise<HTMLSelectElement | null> {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  const grp = document.querySelector('#settings-rig-advanced .settings-group-toggle') as HTMLButtonElement | null
  expect(grp, 'the Advanced group exists in Rig & CAT').toBeTruthy()
  if (grp!.getAttribute('aria-expanded') === 'false') fireEvent.click(grp!)
  expect(
    [...document.querySelectorAll('.settings-label')].some((e) => /Hold the data mode while SSTV/.test(e.textContent ?? '')),
    'the Advanced group rendered, so an absent picker is an answer',
  ).toBe(true)
  const label = [...document.querySelectorAll('.settings-label')].find((e) => e.textContent?.trim() === 'Waterfall source')
  return (label?.parentElement?.querySelector('select') as HTMLSelectElement | null) ?? null
}

function footerSave() {
  return screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
}

describe('the Waterfall source picker', () => {
  it("shows the radio's scope by default, and an older radio with no choice saved reads the same", async () => {
    for (const radio of [IC7300, { ...IC7300, nativeScope: undefined }]) {
      renderPanel(settingsFor(radio))
      const picker = await pickerOnScreen()
      expect(picker, 'offered on an IC-7300 on native CI-V').not.toBeNull()
      expect(picker!.value).toBe('auto')
      expect(picker!.disabled).toBe(false)
      expect([...picker!.options].map((o) => o.textContent)).toEqual(["Radio's scope", 'Audio (sound card)'])
      cleanup()
    }
  })

  it('choosing audio on the radio in use saves it for that radio only', async () => {
    renderPanel(settingsFor({}))
    const picker = (await pickerOnScreen())!
    fireEvent.change(picker, { target: { value: 'audio' } })
    fireEvent.click(footerSave())
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const saved = lastCall('setSettings')![0] as { nativeScope?: string; radios: { id: number; nativeScope: string }[] }
    expect(saved.nativeScope, 'the flat form carries the radio in use').toBe('audio')
    expect(saved.radios.find((r) => r.id === 1)?.nativeScope, 'the second radio is unaffected').toBe('auto')
  })

  it('choosing audio on a radio opened with Edit saves it through that radio, not the one in use', async () => {
    renderPanel(settingsFor({}))
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const edits = await screen.findAllByRole('button', { name: 'Edit' })
    fireEvent.click(edits[edits.length - 1])
    expect(await screen.findByText(/Editing IC-9700/)).toBeTruthy()
    const picker = (await pickerOnScreen())!
    expect(picker.value).toBe('auto')
    fireEvent.change(picker, { target: { value: 'audio' } })
    fireEvent.click(footerSave())
    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    const [id, patch] = lastCall('updateRadioProfile') as [number, Record<string, unknown>]
    expect(id).toBe(1)
    expect(patch.nativeScope, 'the edited radio takes the choice').toBe('audio')
    const saved = lastCall('setSettings')![0] as Record<string, unknown>
    expect(saved.nativeScope, 'the radio in use keeps its own').toBe('auto')
  })

  it('is greyed with the reason while the native CI-V switch is off', async () => {
    renderPanel(settingsFor({ icomNativeCat: false }))
    const picker = await pickerOnScreen()
    expect(picker, 'still offered, so the operator can find it').not.toBeNull()
    expect(picker!.disabled).toBe(true)
    expect(picker!.parentElement?.querySelector('.settings-hint')?.textContent).toMatch(/native CI-V connection/)
  })

  it('is offered on the Icom network connection, where the CI-V engine always drives the radio', async () => {
    renderPanel(settingsFor({ rigConn: 'icomlan', icomNativeCat: false, nativeScope: 'audio' }))
    const picker = await pickerOnScreen()
    expect(picker).not.toBeNull()
    expect(picker!.disabled).toBe(false)
    expect(picker!.value).toBe('audio')
  })

  it('is not offered on a radio whose scope Nexus does not stream this way', async () => {
    renderPanel(settingsFor({ rigModel: 1042, rigModelName: 'Yaesu FTDX10', icomNativeCat: false }))
    expect(await pickerOnScreen()).toBeNull()
  })
})
