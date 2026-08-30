// @vitest-environment jsdom
//
// THE POSITION NAME IS MANDATORY ONCE CLUB SYNC IS CONFIGURED.
//
// Club Field Day report (2026-08-30): "I started with no name, then added it but it's still
// displaying the old name/number id. Can't we make the station name a mandatory field?" Two
// halves answer that. The wire half (the name now rides every presence report, so a rename
// propagates live) is pinned in Rust. THIS is the other half: an operator who hosts or joins a
// club event with the name box empty is told, on the tab that holds the fix, instead of watching
// the club band board show something meaningless.
//
// Scoped deliberately: the refusal fires only when club sync is actually configured. A station
// that never joins a club event owes nobody a tent name, and a save it does not need must not be
// blocked by a field it will never use.
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

const RADIO = {
  id: 0,
  name: 'FTDX10',
  enabled: true,
  serialPort: '',
  baud: 38400,
  rigModel: 0,
  rigModelName: 'None',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: false,
  audioIn: '',
  audioOut: '',
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

/** `club` = how this station is wired into the club event; the rest is a saveable station. */
function settingsFixture(club: Record<string, unknown>) {
  return {
    ...defaultSettings,
    ...RADIO,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [RADIO],
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
    ...club,
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
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons. */
async function clickSave() {
  const save = (await screen.findAllByRole('button', { name: 'Save' })).find(
    (b) => (b as HTMLButtonElement).type === 'submit',
  )!
  fireEvent.click(save)
}

describe('a blank position name refuses a club-sync save', () => {
  /** The Contesting tab has to be on screen before its controls can be driven. */
  async function openContesting() {
    fireEvent.click(await screen.findByRole('tab', { name: /contesting/i }))
  }

  it('refuses the save that TURNS HOSTING ON with the name box empty, and marks the box', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdHostEnable: false, fdPositionName: '' })),
    )
    renderPanel()
    await openContesting()
    // The CHANGE is the trigger: this save is the one that starts hosting.
    fireEvent.click(await screen.findByRole('switch', { name: 'Enable club event hosting' }))
    await clickSave()

    expect(await screen.findByText(/Name this position on the Contesting tab/)).toBeTruthy()
    // Refused, not saved-then-warned: nobody starts hosting as a nameless row on the board.
    expect(api.get('setSettings')).not.toHaveBeenCalled()
    // …and the operator is told WHICH box, not just which tab. The callsign refusal has always
    // marked its field; a refusal that only names a tab is half an answer.
    const box = screen.getByLabelText(/Position name/) as HTMLInputElement
    expect(box.getAttribute('aria-invalid')).toBe('true')
  })

  it('refuses the save that SETS A JOIN ADDRESS with the name box empty', async () => {
    api.get('getSettings').mockImplementation(() =>
      // Not hosting — the join box is disabled while this position hosts its own event.
      Promise.resolve(
        settingsFixture({ fdHostEnable: false, fdJoinAddr: '', fdPositionName: '   ' }),
      ),
    )
    renderPanel()
    await openContesting()
    // By placeholder, not by label: that field is a <div class="settings-field">, so its
    // "Join event at" text is a plain <span> with no label association to the input.
    fireEvent.change(await screen.findByPlaceholderText('192.168.1.10:42073'), {
      target: { value: '192.168.1.10:42073' },
    })
    await clickSave()

    expect(await screen.findByText(/Name this position on the Contesting tab/)).toBeTruthy()
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('refuses CLEARING a name a position already had', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdHostEnable: true, fdPositionName: 'CW tent' })),
    )
    renderPanel()
    await openContesting()
    fireEvent.change(await screen.findByLabelText(/Position name/), { target: { value: '  ' } })
    await clickSave()

    expect(await screen.findByText(/Name this position on the Contesting tab/)).toBeTruthy()
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('⭐ REGRESSION: an existing host who never named the position can still save anything else', async () => {
    // ⚠️ THE BUG THIS RULE NEARLY SHIPPED. `fdPositionName` starts empty and nothing backfills
    // it — no migration, no wizard step — so a club host running for months without one had
    // EVERY save refused by a rule that read the STATE instead of the CHANGE. The scenario is
    // 02:00 mid-event: the USB audio interface drops, the operator picks a new input device on
    // the Radio tab, presses Save, and is thrown to Contesting with the fix unsaved. It also
    // buys nothing, because an unnamed position already calls itself by its callsign on the
    // wire. This save touches nothing about club sync and must go straight through.
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdHostEnable: true, fdPositionName: '' })),
    )
    renderPanel()
    await clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(screen.queryByText(/Name this position on the Contesting tab/)).toBeNull()
  })

  it('POSITIVE CONTROL: a named position turning hosting on saves', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdHostEnable: false, fdPositionName: 'CW tent' })),
    )
    renderPanel()
    await openContesting()
    fireEvent.click(await screen.findByRole('switch', { name: 'Enable club event hosting' }))
    await clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(screen.queryByText(/Name this position on the Contesting tab/)).toBeNull()
  })

  it('POSITIVE CONTROL: a station with no club sync at all is not asked for one', async () => {
    // The scope of the rule. Blocking this save would make a Field-Day-only field mandatory for
    // every operator who never runs one.
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(
        settingsFixture({ fdHostEnable: false, fdJoinAddr: '', fdPositionName: '' }),
      ),
    )
    renderPanel()
    await clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(screen.queryByText(/Name this position on the Contesting tab/)).toBeNull()
  })
})
