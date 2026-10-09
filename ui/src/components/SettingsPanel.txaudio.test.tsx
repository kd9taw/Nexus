// @vitest-environment jsdom
//
// #381 (a TS-590S with a SignaLink on its rear ACC2 jack): "Transmit audio source (CAT PTT):
// Front/Mic | Rear/Data", per radio.
//
// Nexus keyed every CAT radio with rigctld `T 1`, Hamlib's RIG_PTT_ON, which a TS-590S sends as
// `TX;` — "SEND (normal transmission using the MIC input)" in Kenwood's own reference — so the
// SignaLink was never transmitted. Rear/Data keys `T 3`, DATA SEND, instead (the radio loop's
// half is in tempo-audio). These hold the Settings half: where the choice is offered, what it
// saves on both save paths, and that a settings file from before it loads as Front/Mic.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel, radioPatch } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "is offered for CAT PTT on a TS-590S, and a settings…", takes
// 0.33 s and 0.29 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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

const TS590 = {
  id: 0,
  name: 'TS-590S',
  enabled: true,
  serialPort: 'COM3',
  baud: 115200,
  rigModel: 2031,
  rigModelName: 'Kenwood TS-590S',
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
}
/** A second radio, NOT the active one — the per-radio patch is how its edits are saved. */
const TS590SG = {
  ...TS590,
  id: 1,
  name: 'TS-590SG',
  serialPort: 'COM9',
  rigModel: 2037,
  rigModelName: 'Kenwood TS-590SG',
  rigctldPort: 4534,
  rotctldPort: 4535,
}

/** Settings as the backend returns them, with the ACTIVE radio's fields in the flat mirror. A
 *  `txAudioSource` of `undefined` is a file from before the setting: the key is not there. */
function settingsFixture(active: Record<string, unknown>, txAudioSource?: string) {
  const s: Record<string, unknown> = {
    ...defaultSettings,
    ...TS590,
    ...active,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [{ ...TS590, ...active }, TS590SG],
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
  }
  delete s.txAudioSource
  if (txAudioSource !== undefined) s.txAudioSource = txAudioSource
  return s as never
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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
  // The backend's answer, cut to the two radios these tests use (the real one is measured
  // from the bundled Hamlib: `rigmodels::PTT_MIC_DATA_RIGS`).
  api.get('getPttMicDataRigModels').mockImplementation(() => Promise.resolve([2031, 2037]))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const LABEL = /Transmit audio source \(CAT PTT\)/

/** Open Radio and wait until the rig form AND the backend's model rule are both in, so an
 *  absence below is an answer and not a render that has not happened yet. */
async function openRadio() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await screen.findByLabelText(/PTT Method/)
  await waitFor(() => expect(api.get('getPttMicDataRigModels')).toHaveBeenCalled())
  await new Promise((r) => setTimeout(r, 0))
}

const choice = () => screen.queryByLabelText(LABEL) as HTMLSelectElement | null

function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}

function lastCall(name: string): unknown[] {
  const calls = api.get(name).mock.calls
  return calls[calls.length - 1] as unknown[]
}

describe('Transmit audio source (CAT PTT) — where it is offered', () => {
  it('is offered for CAT PTT on a TS-590S, and a settings file from before it reads Front/Mic', async () => {
    renderPanel(settingsFixture({}))
    await openRadio()
    const sel = choice()
    expect(sel, 'offered on a mic/data-PTT radio keyed by CAT').not.toBeNull()
    expect(Array.from(sel!.options).map((o) => [o.value, o.textContent])).toEqual([
      ['front', 'Front/Mic'],
      ['rear', 'Rear/Data'],
    ])
    expect(sel!.value, 'no key in the file: Front/Mic, as every release keyed').toBe('front')
  })

  it('shows a stored Rear/Data as Rear/Data, read the way the radio loop reads it', async () => {
    renderPanel(settingsFixture({}, 'rear'))
    await openRadio()
    expect(choice()!.value).toBe('rear')
    cleanup()
    // A hand-edited file: Rust's `tx_audio_source_is_rear` ignores case and spaces, so the box
    // must too, or it would show Front/Mic for a radio that keys its DATA input.
    renderPanel(settingsFixture({}, ' Rear '))
    await openRadio()
    expect(choice()!.value).toBe('rear')
  })

  it('follows the PTT method: not offered on VOX, offered once CAT keys the radio', async () => {
    renderPanel(settingsFixture({ pttMethod: 'vox' }))
    await openRadio()
    expect(choice(), 'VOX: Nexus sends no key-down at all').toBeNull()
    fireEvent.change(screen.getByLabelText(/PTT Method/), { target: { value: 'cat' } })
    expect(choice(), 'control: the same render offers it under CAT').not.toBeNull()
    fireEvent.change(screen.getByLabelText(/PTT Method/), { target: { value: 'rts' } })
    expect(choice(), 'a serial keying line has no audio source to choose').toBeNull()
  })

  it('follows the radio: not offered on a model whose Hamlib driver cannot choose', async () => {
    renderPanel(settingsFixture({}))
    await openRadio()
    expect(choice(), 'control: offered on the TS-590S').not.toBeNull()
    const model = screen.getByLabelText('Enter a Hamlib rig model number directly')
    fireEvent.change(model, { target: { value: '1042' } }) // Yaesu FTDX10: RIG_PTT_RIG only
    expect(choice(), 'FTDX10').toBeNull()
    fireEvent.change(model, { target: { value: '2037' } }) // TS-590SG: mic/data
    expect(choice(), 'TS-590SG').not.toBeNull()
  })

  it('is not offered through OmniRig, which keys the radio from its own rig file', async () => {
    renderPanel(settingsFixture({ rigConn: 'omnirig' }))
    await openRadio()
    expect(choice()).toBeNull()
    fireEvent.change(screen.getByLabelText(/^Connection/), { target: { value: 'serial' } })
    expect(choice(), 'control: the same radio over its serial port').not.toBeNull()
  })

  it('is not offered when the backend rule cannot be read, so every radio keys as it always has', async () => {
    for (const unread of [
      () => Promise.resolve(null),
      () => Promise.reject(new Error('applicationUnsupported')),
    ]) {
      api.get('getPttMicDataRigModels').mockImplementation(unread)
      renderPanel(settingsFixture({}))
      await openRadio()
      expect(choice()).toBeNull()
      cleanup()
    }
  })
})

describe('Transmit audio source (CAT PTT) — what it saves', () => {
  it('Rear/Data on the radio being operated rides the settings save as "rear"', async () => {
    renderPanel(settingsFixture({}))
    await openRadio()
    fireEvent.change(choice()!, { target: { value: 'rear' } })
    expect(choice()!.value).toBe('rear')
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect((lastCall('setSettings')[0] as Record<string, unknown>).txAudioSource).toBe('rear')
  })

  it('Rear/Data on a radio being edited, not operated, rides the per-radio patch', async () => {
    // The seam that has dropped per-radio fields five times before: an edit of a non-active radio
    // is saved by `updateRadioProfile(id, radioPatch(form))`, so a field `radioPatch` omits is lost.
    renderPanel(settingsFixture({}))
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const edits = await screen.findAllByRole('button', { name: 'Edit' })
    fireEvent.click(edits[0])
    expect(await screen.findByText(/Editing TS-590SG/)).toBeTruthy()
    await waitFor(() => expect(choice()).not.toBeNull())
    fireEvent.change(choice()!, { target: { value: 'rear' } })
    clickSave()
    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    const [id, patch] = lastCall('updateRadioProfile') as [number, Record<string, unknown>]
    expect(id).toBe(1)
    expect(patch.txAudioSource).toBe('rear')
  })

  it('the per-radio patch carries it, and an absent one as Front/Mic', () => {
    expect(radioPatch({}).txAudioSource).toBe('front')
    expect(radioPatch({ txAudioSource: 'rear' }).txAudioSource).toBe('rear')
    expect(radioPatch({ txAudioSource: 'front' }).txAudioSource).toBe('front')
  })
})
