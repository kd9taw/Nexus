// @vitest-environment jsdom
//
// THREE SETTINGS THAT EXISTED IN RUST WITH NO WAY TO SET THEM. `tunePowerPct`,
// `catSerialHandshake` and `catPttLineState` all landed backend-first with live consumers
// (tempo-audio/src/service.rs and rigctld_proc.rs), and no control anywhere in the panel — so
// every one of them was inert: the operator could not reach the feature, and the only way to
// change one was to hand-edit settings.json. A backend field with no UI is not shipped.
//
// The two CAT declarations are the #145 fix — a rig that KEYS THE TRANSMITTER AT APP LAUNCH —
// and they carry a real warning rather than a neutral hint, because Hamlib's `rig_open` refuses
// `<line>_state` on the line it keys with and THE REFUSAL IS SILENT: it returns -RIG_ECONF,
// rigctld does not exit, and it goes on serving a rig it never opened. So a non-auto value can
// fix a keyed-at-launch rig on one backend and cost CAT entirely on another, and which one it
// does cannot be determined from here — there is no serial rig on this box and CI cannot watch
// a pin. The hint has to say so; the tests below pin that it does.
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

const RIG = {
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

function settingsFixture() {
  return {
    ...defaultSettings,
    ...RIG,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [RIG],
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

function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}

/** The saved payload — these are all STATION-WIDE fields, so they ride `setSettings`. */
async function savedPayload(): Promise<Record<string, unknown>> {
  await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0] as Record<string, unknown>
}

describe('Tune power — the fixed low level a tune-up keys at', () => {
  it('is reachable on the Digital tab and saves as a percent', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
    const input = await screen.findByLabelText(/Tune power/i)
    fireEvent.change(input, { target: { value: '10' } })
    clickSave()
    expect((await savedPayload()).tunePowerPct).toBe(10)
  })

  it('clearing it means "never touch my power" — null, not zero', async () => {
    // `Option<u8>`, and `None` is load-bearing: the loop declines to act rather than guessing,
    // because leaving a rig at 10 W for the rest of a session is worse than the bug it fixes.
    // A generic numeric coercion would have saved 0 here, which reads as "tune at zero power".
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
    const input = await screen.findByLabelText(/Tune power/i)
    fireEvent.change(input, { target: { value: '10' } })
    fireEvent.change(input, { target: { value: '' } })
    clickSave()
    expect((await savedPayload()).tunePowerPct).toBeNull()
  })

  it('says plainly that it can only turn the rig DOWN', async () => {
    // The loop commands min(tune_pct, current_power). An operator who reads this as "tune at
    // 50%" while running 25% would key at 25% and think the setting was broken — or worse,
    // set it high believing it raises power for a tune.
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
    const field = (await screen.findByLabelText(/Tune power/i)).closest('.settings-field')!
    expect(field.textContent).toMatch(/lower|never raise|only.*down/i)
  })
})

describe('the two CAT declarations (#145 — a rig that keys at launch)', () => {
  async function openAdvancedCat() {
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    // They live in the Advanced disclosure, which is collapsed by default — correct for an
    // opt-in declaration that replaces an inference.
    const adv = await screen.findByRole('button', { name: /Advanced/i })
    fireEvent.click(adv)
  }

  it('serial handshake offers exactly the four values the backend parses', async () => {
    renderPanel()
    await openAdvancedCat()
    const sel = (await screen.findByLabelText(/Serial handshake/i)) as HTMLSelectElement
    expect(Array.from(sel.options).map((o) => o.value)).toEqual([
      'auto',
      'none',
      'hardware',
      'xonxoff',
    ])
    expect(sel.value).toBe('auto') // today's behaviour, to the byte
  })

  it('keying line offers exactly the four values the backend parses', async () => {
    renderPanel()
    await openAdvancedCat()
    const sel = (await screen.findByLabelText(/Keying line at startup/i)) as HTMLSelectElement
    expect(Array.from(sel.options).map((o) => o.value)).toEqual([
      'auto',
      'untouched',
      'low',
      'high',
    ])
    expect(sel.value).toBe('auto')
  })

  it('both save the value the backend parses', async () => {
    renderPanel()
    await openAdvancedCat()
    fireEvent.change(await screen.findByLabelText(/Serial handshake/i), {
      target: { value: 'none' },
    })
    fireEvent.change(await screen.findByLabelText(/Keying line at startup/i), {
      target: { value: 'low' },
    })
    clickSave()
    const payload = await savedPayload()
    expect(payload.catSerialHandshake).toBe('none')
    expect(payload.catPttLineState).toBe('low')
  })

  it('WARNS rather than describes — the operator is the only one who can watch the line', async () => {
    // This is the whole reason these are opt-in declarations and not ordinary preferences.
    // A neutral hint would invite an operator to "try" a value and silently lose CAT.
    renderPanel()
    await openAdvancedCat()
    for (const label of [/Serial handshake/i, /Keying line at startup/i]) {
      const field = (await screen.findByLabelText(label)).closest('.settings-field')!
      const text = field.textContent ?? ''
      expect(text, `${label} must say to leave it on Auto`).toMatch(/leave it on Auto/i)
      expect(text, `${label} must name the keys-at-launch symptom`).toMatch(/keys at launch/i)
      expect(text, `${label} must say to put it back if CAT stops`).toMatch(/put it back/i)
    }
  })

  it('says upgrading changes nothing — the default is today\'s behaviour', async () => {
    renderPanel()
    await openAdvancedCat()
    const field = (await screen.findByLabelText(/Serial handshake/i)).closest('.settings-field')!
    expect(field.textContent).toMatch(/changes nothing|today's behaviour|behaves exactly/i)
  })
})
