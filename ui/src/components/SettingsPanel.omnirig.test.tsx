// @vitest-environment jsdom
//
// OmniRig as a connection kind (2026-08-18).
//
// Two things are pinned here, and the second is the one that has cost this project real
// operator data twice already:
//
//   1. The CONTROL. OmniRig is offered on every platform and disabled off Windows, with the
//      reason in its own label; picking it hides Serial Port and Baud (OmniRig owns the rig
//      type, the port and the baud) and reveals the RIG 1 / RIG 2 selector.
//   2. The PATCH. Every save of the rig form while editing a NON-ACTIVE radio routes through
//      `updateRadioProfile(radioPatch(form))`, so a per-radio field `radioPatch` does not
//      return is silently dropped on Save while the panel reports success — the `pttSerialPort`
//      bug (0.18.0) and the Flex three (2026-08-17). `omnirigSlot` is per-radio: it names WHICH
//      radio inside OmniRig this profile drives.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel, radioPatch, omnirigChoiceFor } from './SettingsPanel'
import { checkRigForm, blocks } from '../rigFormChecks'
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

// Mock EVERY export of `../api`, derived from the real module — a hand-kept list makes the panel
// throw on mount when a verb is added, which reads as a behaviour regression.
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
}

/** The NON-ACTIVE radio: an IC-7300 driven through OmniRig's RIG 2 slot. */
const OMNI = {
  ...FTDX10,
  id: 1,
  name: 'Shack IC-7300',
  serialPort: '',
  rigModel: 0, // the rig type lives in OmniRig, not here
  rigModelName: 'None / VOX',
  rigConn: 'omnirig',
  omnirigSlot: 2,
  rigctldPort: 4534,
  rotctldPort: 4535,
  audioIn: 'in-1',
  audioOut: 'out-1',
}

function settingsWith(radios: (typeof FTDX10)[]) {
  return {
    ...defaultSettings,
    ...FTDX10, // the flat mirror describes the ACTIVE radio (id 0)
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
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith([FTDX10, OMNI])))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('OmniRig is offered as a connection kind', () => {
  // BOTH directions, computed. jsdom is never Windows, so a component-only test could see the
  // disabled half and nothing else — and "the option is greyed out" would then be indis-
  // tinguishable from "the option is permanently broken".
  it('is enabled on Windows and honestly disabled everywhere else', () => {
    const win = omnirigChoiceFor(true)
    expect(win.disabled).toBe(false)
    expect(win.label).toMatch(/OmniRig/)

    const other = omnirigChoiceFor(false)
    expect(other.disabled).toBe(true)
    expect(other.label).toMatch(/Windows only/i)
  })

  it('renders the option in the Connection control, with the reason on a non-Windows box', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const option = (await screen.findByRole('option', {
      name: /Windows only/i,
    })) as HTMLOptionElement
    expect(option.value).toBe('omnirig')
    // jsdom's user agent is not Windows, so this is the disabled half — and the label says why.
    expect(option.disabled).toBe(true)
    expect(option.textContent).toMatch(/Windows only/i)
  })

  it('hides the fields OmniRig owns and shows the RIG 1 / RIG 2 picker', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    // The FIELD LABELS, not any prose that happens to name them: a hint elsewhere on the page
    // says "your Rig Model and Serial Port", and matching that would make this test pass while
    // the field itself was gone.
    const labels = () =>
      Array.from(document.querySelectorAll('.settings-label')).map((n) => n.textContent)
    // Control first: on serial, Serial Port and Baud are there and the slot picker is not.
    expect(labels()).toContain('Serial Port')
    expect(labels()).toContain('Baud')
    expect(screen.queryByLabelText('OmniRig rig slot')).toBeNull()

    // Editing the OmniRig radio retargets the form at it.
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }))
    expect(await screen.findByText(/Editing Shack IC-7300/)).toBeTruthy()

    const slot = (await screen.findByLabelText('OmniRig rig slot')) as HTMLSelectElement
    expect(slot.value).toBe('2')
    // The two fields OmniRig owns are gone — asking for them would be asking the operator to
    // configure the same radio twice.
    expect(labels()).not.toContain('Serial Port')
    expect(labels()).not.toContain('Baud')
    // …and the OmniRig field IS one, so this is a swap rather than a disappearance.
    expect(labels()).toContain('OmniRig radio')
  })
})

describe('the OmniRig slot travels with the radio it belongs to', () => {
  it('radioPatch carries omnirigSlot — the field set a per-radio Save is built from', () => {
    expect(radioPatch({ rigConn: 'omnirig', omnirigSlot: 2 }).omnirigSlot).toBe(2)
    // An unset radio gets RIG 1, never `undefined` (the backend field is non-optional, and
    // `undefined` would serialize the key away and land as 0 on the Rust side).
    expect(radioPatch({}).omnirigSlot).toBe(1)
    expect(radioPatch({}).rigConn).toBe('serial')
  })

  it('saving a NON-ACTIVE OmniRig radio keeps its slot instead of resetting it to RIG 1', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }))
    expect(await screen.findByText(/Editing Shack IC-7300/)).toBeTruthy()

    // Save WITHOUT touching anything — the exact shape of the two prior data-loss bugs.
    const save = screen
      .getAllByRole('button', { name: 'Save' })
      .find((b) => (b as HTMLButtonElement).type === 'submit')!
    fireEvent.click(save)

    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    const calls = api.get('updateRadioProfile').mock.calls
    const [id, patch] = calls[calls.length - 1] as [number, Record<string, unknown>]
    expect(id).toBe(1)
    expect(patch.rigConn).toBe('omnirig')
    expect(patch.omnirigSlot).toBe(2)
    // The station-wide write must NOT be the path taken — that is what stamped the edited
    // radio's config onto the active one (the 2026-07-25 report).
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })
})

describe('the pre-save checks leave OmniRig alone', () => {
  it('does not demand a serial port or a rig model that OmniRig owns', () => {
    const omni = {
      serialPort: '',
      rigConn: 'omnirig',
      pttMethod: 'cat', // the normal OmniRig setup: CAT keying through OmniRig
      rigModel: 0, // …and no Nexus rig model, because OmniRig has the rig type
    }
    expect(checkRigForm(omni, [], [0, 1, 2, 3, 4])).toEqual([])
    expect(blocks(checkRigForm(omni, [], [0, 1, 2, 3, 4]))).toBe(false)
    // The positive control, twice over: the SAME form on serial IS blocked for BOTH of the
    // reasons OmniRig is exempt from — so this is the exemption doing the work, not a pair of
    // checks that never fire.
    const noPort = { ...omni, rigConn: 'serial', rigModel: 1042 }
    expect(blocks(checkRigForm(noPort, [], [0, 1, 2, 3, 4]))).toBe(true)
    const catNoModel = { ...omni, rigConn: 'serial', serialPort: 'COM3' }
    expect(blocks(checkRigForm(catNoModel, ['COM3'], [0, 1, 2, 3, 4]))).toBe(true)
  })
})
