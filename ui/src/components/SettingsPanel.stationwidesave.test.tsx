// @vitest-environment jsdom
//
// #173 — A SETTINGS SAVE SILENTLY DISCARDED EVERY STATION-WIDE FIELD.
//
// `persistRadioForm` routed the WHOLE save through `updateRadioProfile(id, radioPatch(next))`
// whenever the panel was editing a non-active radio (which is what `handleConfigureRadio` puts
// it into, and it stays there — nothing sends the operator back to the active radio). But
// `radioPatch()` carries per-radio CAT/audio/PTT/rotator fields ONLY. So an operator who had
// clicked Edit on their second rig, then went to Logging & Connectors and turned on QRZ
// auto-upload, watched the panel say "Saved" while the setting never left the browser.
//
// THIS IS A SEAM, NOT A FIELD. The same seam has now eaten `pttSerialPort`, the Flex three,
// `omnirigSlot`, `icomDataMode`, `ampModel`/`ampPort` — each fixed by adding one more name to
// `radioPatch`. Station-wide settings can never be fixed that way: they do not belong in the
// patch at all. The save has to send BOTH writes.
//
// The guard below is therefore written against a field with no business being in `radioPatch`,
// so it cannot be satisfied by adding a name to it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
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
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10, SECOND],
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
    qrzLogbookUpload: false, // the reporter's field, off to begin with
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

/** Put the panel into the state the report came from: editing the SECOND, non-active radio. */
async function editTheSecondRadio() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  const edits = await screen.findAllByRole('button', { name: 'Edit' })
  fireEvent.click(edits[0])
  expect(await screen.findByText(/Editing IC-7300/)).toBeTruthy()
}

/** The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons. */
function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}

describe('a station-wide setting survives Save while a non-active radio is being edited (#173)', () => {
  it('sends the station-wide write at all — it used to be dropped on the floor', async () => {
    renderPanel()
    await editTheSecondRadio()

    // Now go somewhere station-wide and change something. This is the reporter's exact move:
    // nothing on this tab belongs to a radio, and nothing on screen said the save was aimed
    // at one.
    fireEvent.click(screen.getByRole('tab', { name: 'Logging & Connectors' }))
    fireEvent.click(await screen.findByRole('switch', { name: /Auto-upload QSOs to QRZ/ }))
    clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const payload = lastCall('setSettings')![0] as Record<string, unknown>
    expect(payload.qrzLogbookUpload).toBe(true)
  })

  it('and that write carries the ACTIVE radio\'s config, never the edited one\'s', async () => {
    // The reason the whole save was funnelled through the per-radio verb in the first place
    // (2026-07-25): the backend folds a payload's flat rig fields into whichever radio
    // `activeRadio` names, so a payload built from the EDITED form stamped radio 2's COM port
    // onto radio 1. `withActiveRadioConfig` is what makes the fold a no-op.
    renderPanel()
    await editTheSecondRadio()
    fireEvent.click(screen.getByRole('tab', { name: 'Logging & Connectors' }))
    fireEvent.click(await screen.findByRole('switch', { name: /Auto-upload QSOs to QRZ/ }))
    clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const payload = lastCall('setSettings')![0] as Record<string, unknown>
    expect(payload.activeRadio).toBe(0)
    expect(payload.serialPort).toBe('COM3') // the FTDX10's, not the IC-7300's COM9
    expect(payload.rigModel).toBe(1042)
    expect(payload.audioIn).toBe('in-0')
  })

  it('still writes the edited radio\'s own per-radio config', async () => {
    // Both halves, or the fix trades one silent loss for another.
    renderPanel()
    await editTheSecondRadio()
    fireEvent.click(screen.getByRole('tab', { name: 'Logging & Connectors' }))
    fireEvent.click(await screen.findByRole('switch', { name: /Auto-upload QSOs to QRZ/ }))
    clickSave()

    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    const [id, patch] = lastCall('updateRadioProfile')! as [
      number,
      Record<string, unknown>,
    ]
    expect(id).toBe(1)
    expect(patch.serialPort).toBe('COM9')
    expect(patch.rigModel).toBe(3073)
  })

  it('orders the two writes station-wide FIRST — the settings payload carries a stale radios array', async () => {
    // `setSettings` persists `radios` wholesale, and the form's copy predates the per-radio
    // patch. Sent second it would undo it, which is the same silent loss wearing a new hat.
    const order: string[] = []
    api.get('setSettings').mockImplementation(() => {
      order.push('setSettings')
      return Promise.resolve(null)
    })
    api.get('updateRadioProfile').mockImplementation(() => {
      order.push('updateRadioProfile')
      return Promise.resolve(null)
    })
    renderPanel()
    await editTheSecondRadio()
    clickSave()

    await waitFor(() => expect(order).toEqual(['setSettings', 'updateRadioProfile']))
  })
})
