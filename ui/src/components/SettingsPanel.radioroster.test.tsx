// @vitest-environment jsdom
//
// The radio roster's Remove flow, from a Mac field report (2026-08-17): "the IC-9700 won't
// delete". The 9700 was the ACTIVE radio, and the Remove button was simply NOT RENDERED for
// the active card — no disabled state, no tooltip, nothing anywhere saying removal requires
// making another radio active first. A silent-refusal UX bug that presents as a persistence
// bug. The button is now always rendered when the roster has >1 radio, disabled with a
// teaching title on the active card.
//
// Second half: deleting the radio the rig form is EDITING left `editingRadioId` dangling,
// so every later Save routed through `update_radio_profile(<deleted id>)` — a silent no-op —
// and station-wide settings (callsign, credentials) stopped persisting with no error. The
// form retargets to the active radio on removal.
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

// Mock EVERY export of `../api`, derived from the real module rather than a hand-kept list —
// a verb missing from a hand-kept list makes the panel throw on mount, which presents as a
// behaviour regression instead of the stale mock it is (see the radiorouting test).
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

function settingsWith(radios: (typeof FTDX10)[]) {
  return {
    ...defaultSettings,
    ...FTDX10,
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
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith([FTDX10, IC9700])))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('the roster Remove button', () => {
  it('is rendered DISABLED on the active card, with a title that teaches the rule', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))

    // Scope to the roster cards by their titles — other sections have Remove buttons too.
    const removes = (await screen.findAllByRole('button', { name: 'Remove' })).filter((b) =>
      /roster|operating radio/i.test((b as HTMLButtonElement).title),
    )
    // Two radios, two Remove buttons — absence was the bug ("the 9700 won't delete").
    expect(removes).toHaveLength(2)
    const disabled = removes.filter((b) => (b as HTMLButtonElement).disabled)
    expect(disabled).toHaveLength(1)
    expect(disabled[0].title).toMatch(/make another radio active first/i)
    const enabled = removes.filter((b) => !(b as HTMLButtonElement).disabled)
    expect(enabled).toHaveLength(1)
    expect(enabled[0].title).toMatch(/remove this radio/i)
  })

  it('removing the radio being EDITED retargets the form instead of leaving a dangling id', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    api.get('removeRadio').mockImplementation(() => Promise.resolve({ ok: true }))
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))

    // Edit the non-active IC-9700 — the "Editing …" banner marks the retargeted form.
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }))
    expect(await screen.findByText(/Editing IC-9700/)).toBeTruthy()

    // Delete it. The re-pull after removal serves the one-radio roster.
    api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith([FTDX10])))
    const enabled = (await screen.findAllByRole('button', { name: 'Remove' })).find(
      (b) =>
        /roster/i.test((b as HTMLButtonElement).title) && !(b as HTMLButtonElement).disabled,
    )!
    fireEvent.click(enabled)

    await waitFor(() => expect(api.get('removeRadio')).toHaveBeenCalledWith(1))
    // The form must fall back to the active radio: with the id left dangling the banner
    // stays up ("Editing another radio") and every Save no-ops against the deleted profile.
    await waitFor(() => expect(screen.queryByText(/Editing /)).toBeNull())
  })
})
