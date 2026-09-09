// @vitest-environment jsdom
//
// The per-radio Edit flow (0.17.0) decoupled "which radio the rig form edits" from "which radio
// is active", but only Save was taught to route accordingly. Test CAT and Auto-test still sent
// the whole flat form, and the backend folds a flat payload into the profile named by its
// `activeRadio` — so configuring radio 2 and pressing either button stamped radio 2's COM port,
// model and audio devices onto radio 1's profile, persisted. Operator report, 2026-07-25: with
// two radios configured, both ended up on one set of comm ports.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  // SettingsPanel pulls ~50 verbs from ../api. They all resolve null, which is enough for a
  // mount — the assertions here are about WHICH save verb gets called, not what it returns.
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module rather than a hand-kept list.
//
// The list was the problem: a verb missing from it made the panel THROW ON MOUNT ("No export is
// defined on the mock"), which presents as a behaviour regression in whichever test happened to
// run -- not as the out-of-date mock it actually is. Reading the real module's export names makes
// that failure impossible by construction.
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
}
const IC9700 = {
  ...FTDX10,
  id: 1,
  name: 'IC-9700',
  serialPort: 'COM7',
  baud: 115200,
  rigModel: 3081,
  rigModelName: 'Icom IC-9700',
  rigctldPort: 4534,
  rotctldPort: 4535,
  icomNativeCat: true,
  audioIn: 'in-1',
  audioOut: 'out-1',
}

/** Settings with two radios, FTDX10 active, flat mirror = the FTDX10.
 * Built on the real `Settings::default()` (dumped from Rust) so the panel renders for real
 * rather than against a hand-guessed subset that drifts. */
function twoRadioSettings() {
  return {
    ...defaultSettings,
    ...FTDX10,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10, IC9700],
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
  // Reset in place — the mocked module captured these exact spy objects at import time, so
  // replacing them here would leave the component calling the old ones.
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.17.12'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(twoRadioSettings()))
  api.get('probeCatPorts').mockImplementation(() =>
    Promise.resolve({
      found: true,
      detail: 'Icom IC-9700 on COM7 @ 115200 baud',
      portName: 'COM7',
      baud: 115200,
      model: 3081,
      modelName: 'Icom IC-9700',
      freqMhz: 144.174,
      modelSeeded: false,
    }),
  )
  api.get('testCat').mockImplementation(() => Promise.resolve({ ok: true, detail: 'ok' }))
})
afterEach(cleanup)

/** Click "Edit" on the non-active radio (the IC-9700) so the flat rig form describes it. */
async function editTheNonActiveRadio() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  // Only the NON-active radio's card offers Edit — the active one is already loaded in the form.
  fireEvent.click(await screen.findByRole('button', { name: 'Edit' }))
}

describe('rig form writes go to the radio they describe', () => {
  it('Test CAT while editing a non-active radio patches THAT radio, never the active one', async () => {
    renderPanel()
    await editTheNonActiveRadio()

    fireEvent.click(await screen.findByRole('button', { name: /test cat/i }))

    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    const [id, patch] = api.get('updateRadioProfile').mock.calls[0] as [number, { serialPort: string }]
    expect(id).toBe(1)
    expect(patch.serialPort).toBe('COM7')
    // ⛔ DO NOT RESTORE `expect(setSettings).not.toHaveBeenCalled()` HERE. That assertion was
    // #173 written down as a guarantee, and it is why this seam survived six data-loss
    // bugs: routing the WHOLE save through the per-radio verb discarded every
    // station-wide setting while the panel reported success, and any correct fix turned
    // four assertions like this one red and read as a regression.
    // WHAT IT GUARANTEES NOW: a whole-settings save built from the EDITED form is what
    // clobbered radio 0 — the payload's identity, not the save's existence, is the invariant.
    // Assert it describes the FTDX10.
    const saved = lastCall('setSettings')?.[0] as
      | { activeRadio: number; serialPort: string }
      | undefined
    if (saved) {
      expect(saved.activeRadio).toBe(0)
      expect(saved.serialPort).toBe('COM3') // the active FTDX10's, never the IC-9700's COM7
    }
  })

  it('Test CAT does not report a green tick earned by the OTHER radio', async () => {
    renderPanel()
    await editTheNonActiveRadio()
    fireEvent.click(await screen.findByRole('button', { name: /test cat/i }))

    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    // test_cat has no radio argument — it reports the ACTIVE radio. Running it here would hand
    // back a pass earned by the FTDX10 for an IC-9700 config that was never tested.
    expect(api.get('testCat')).not.toHaveBeenCalled()
    expect(await screen.findByText(/make .* active to test it/i)).toBeTruthy()
  })

  it('Auto-test probes on behalf of the radio being configured, and saves to it', async () => {
    renderPanel()
    await editTheNonActiveRadio()

    fireEvent.click(await screen.findByRole('button', { name: /auto-test/i }))

    // Radio-blind probing seeded every port with the ACTIVE radio's Hamlib model, and an Icom
    // answers only at its own CI-V address — so radio 2's port could never answer.
    await waitFor(() => expect(api.get('probeCatPorts')).toHaveBeenCalledWith(1))
    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    // ⛔ DO NOT RESTORE `expect(setSettings).not.toHaveBeenCalled()` HERE. That assertion was
    // #173 written down as a guarantee, and it is why this seam survived six data-loss
    // bugs: routing the WHOLE save through the per-radio verb discarded every
    // station-wide setting while the panel reported success, and any correct fix turned
    // four assertions like this one red and read as a regression.
    // WHAT IT GUARANTEES NOW: as above — the station-wide write is allowed and must carry the
    // ACTIVE radio's config.
    const saved = lastCall('setSettings')?.[0] as
      | { activeRadio: number; serialPort: string }
      | undefined
    if (saved) {
      expect(saved.activeRadio).toBe(0)
      expect(saved.serialPort).toBe('COM3')
    }
  })
})
