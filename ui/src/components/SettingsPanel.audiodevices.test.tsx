// @vitest-environment jsdom
//
// The audio-device pickers show a device's DESCRIPTION and store its NAME.
//
// A Kubuntu 26.04 operator reported "none of my audio devices are detected": his `aplay -l`
// listed 8 cards including `card 7: CODEC [USB AUDIO CODEC]` — his FTDX10 — and Nexus offered
// one card, under raw ALSA PCM names (`hw:CARD=Device,DEV=0`, `dsnoop:…`, `iec958:…`). Nothing
// in the list matched anything he saw in his own system, KDE, pavucontrol or WSJT-X.
//
// The backend now sends `{name, label}` per device. These tests pin the two halves of that:
// the operator reads `label`, and the value that reaches settings is always `name` — plus the
// Windows/macOS case, where the two are the same string and every rendered option must
// therefore be byte-identical to what shipped before. Losing a Windows operator's device list
// would be a far worse outcome than the bug being fixed.
//
// NON-VACUITY, measured (2026-08-05) — each was broken and this file re-run, 5 tests green
// at baseline: option text back to the raw name (= the reported bug) → 3 red; the two label
// maps merged into one → 1 red; the "— not detected" branch removed → 1 red; the option
// VALUE switched to the label (a label reaching settings) → 2 red.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const VERBS = [
    'clearCloudlogKey', 'clearClublogPassword', 'clearEqslPassword', 'clearHamqthPassword',
    'clearHrdlogCode', 'clearLotwPassword', 'clearQrzLogbookKey', 'clearQrzPassword', 'detectRigs',
    'downloadEqslReport', 'downloadLotwReport', 'getAllRigModels', 'getAudioDevices', 'getBandPlan',
    'getRigModels', 'getSerialPortsDetailed', 'getSettings', 'setCloudlogKey', 'setClublogPassword',
    'setEqslPassword', 'setHamqthPassword', 'setHrdlogCode', 'setLotwPassword', 'setQrzLogbookKey',
    'setQrzPassword', 'setRepeaterbookToken', 'setRxGain', 'setSettings', 'setTxLevel', 'addRadio',
    'removeRadio', 'renameRadio', 'setActiveRadio', 'setRadioBands', 'updateRadioProfile', 'testCat',
    'probeCatPorts', 'qrzTestConnection', 'syncQrz', 'n3fjpTestConnection', 'getConnectionLog',
    'getCredentialsStatus', 'fetchLotwUsers', 'getLotwUsersStatus', 'fetchFccStates',
    'getFccStatesStatus', 'getTleStatus', 'fetchTlesNow', 'importTles', 'discoverFlex',
    'civDiagnosticLog', 'civDiagnosticStatus', 'allTxtLocation', 'revealAllTxt', 'recordingsLocation', 'revealRecordings', 'appVersion',
    'getSpectrumRow', 'setFrequency', 'getWatchlist', 'setWatchlist', 'openPanelWindow',
    'getAssistanceJournal', 'setUnassistedMode',
  ]
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get, VERBS }
})

vi.mock('../api', () => {
  const mod: Record<string, unknown> = {}
  for (const v of api.VERBS) mod[v] = api.get(v)
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

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

/** His real machine, after pruning: 6 cards + the PipeWire routing endpoints. */
const LINUX = {
  input: [
    { name: 'plughw:CARD=Generic,DEV=0', label: 'HD-Audio Generic' },
    { name: 'plughw:CARD=Device,DEV=0', label: 'USB Audio Device' },
    { name: 'plughw:CARD=CODEC,DEV=0', label: 'USB AUDIO CODEC' },
    { name: 'pipewire', label: 'PipeWire Sound Server' },
  ],
  output: [
    { name: 'plughw:CARD=Generic,DEV=0', label: 'HD-Audio Generic, ALC1220 Analog' },
    { name: 'plughw:CARD=CODEC,DEV=0', label: 'USB AUDIO CODEC' },
    { name: 'pipewire', label: 'PipeWire Sound Server' },
  ],
}

/** Windows/macOS: cpal already reports the OS's friendly name, so label === name. */
const WINDOWS = {
  input: [
    { name: 'Microphone (USB Audio CODEC)', label: 'Microphone (USB Audio CODEC)' },
    { name: 'Microphone Array (Intel Smart Sound)', label: 'Microphone Array (Intel Smart Sound)' },
  ],
  output: [
    { name: 'Speakers (USB Audio CODEC)', label: 'Speakers (USB Audio CODEC)' },
    { name: 'Speakers (Realtek(R) Audio)', label: 'Speakers (Realtek(R) Audio)' },
  ],
}

let settings: Record<string, unknown>

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.0.0'))
  settings = { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' }
  api.get('getSettings').mockImplementation(() => Promise.resolve(settings as never))
})
afterEach(cleanup)

const openRadioTab = async () =>
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))

/** The <select> whose field carries this <span class="settings-label"> text. The pickers sit
 *  inside a <label> that also holds a long hint, so the accessible name is not the caption. */
const picker = async (name: string) => {
  const caption = await screen.findByText(name, { selector: '.settings-label' })
  const sel = caption.closest('label')?.querySelector('select')
  if (!sel) throw new Error(`no <select> under the "${name}" field`)
  return sel as HTMLSelectElement
}

describe('Linux: the picker reads like the operator’s own system', () => {
  beforeEach(() => {
    api.get('getAudioDevices').mockImplementation(() => Promise.resolve(LINUX))
  })

  it('shows the card DESCRIPTION and stores the ALSA PCM name', async () => {
    renderPanel()
    await openRadioTab()
    const rx = await picker('Input Device (RX)')
    const opts = [...rx.options]
    // What he reads is what `aplay -l`, KDE, pavucontrol and WSJT-X show him...
    expect(opts.map((o) => o.textContent?.trim())).toEqual([
      'System default',
      'HD-Audio Generic',
      'USB Audio Device',
      'USB AUDIO CODEC',
      'PipeWire Sound Server',
    ])
    // ...and NONE of those strings is what gets stored. The value is the address.
    expect(opts.map((o) => o.value)).toEqual([
      '',
      'plughw:CARD=Generic,DEV=0',
      'plughw:CARD=Device,DEV=0',
      'plughw:CARD=CODEC,DEV=0',
      'pipewire',
    ])
  })

  it('picking the rig by its description puts the PCM name into the form', async () => {
    renderPanel()
    await openRadioTab()
    const rx = await picker('Input Device (RX)')
    fireEvent.change(rx, { target: { value: 'plughw:CARD=CODEC,DEV=0' } })
    // The select's value — the thing that reaches settings — is never the label.
    expect(rx.value).toBe('plughw:CARD=CODEC,DEV=0')
  })

  it('labels every one of the four pickers, not just RX and TX', async () => {
    // The monitor and voice-mic pickers are the two nobody thinks to check; they read the
    // same enumerated lists and were just as unreadable.
    renderPanel()
    await openRadioTab()
    for (const name of ['Input Device (RX)', 'Output Device (TX)', 'Monitor Output Device']) {
      const sel = await picker(name)
      expect([...sel.options].map((o) => o.textContent?.trim())).toContain('USB AUDIO CODEC')
    }
    fireEvent.click(await screen.findByRole('tab', { name: 'Modes' }))
    const mic = await picker('Voice mic (recording)')
    expect([...mic.options].map((o) => o.textContent?.trim())).toContain('USB AUDIO CODEC')
  })
})

describe('Windows / macOS render exactly what they always did', () => {
  it('every option’s text is cpal’s own device name', async () => {
    // The device-list DTO widening is the one part of this change that is NOT Linux-only.
    // On these hosts label === name, so each rendered string must equal its stored value.
    api.get('getAudioDevices').mockImplementation(() => Promise.resolve(WINDOWS))
    renderPanel()
    await openRadioTab()
    for (const name of ['Input Device (RX)', 'Output Device (TX)']) {
      const sel = await picker(name)
      for (const o of [...sel.options].slice(1)) {
        expect(o.textContent?.trim()).toBe(o.value)
      }
    }
    const rx = await picker('Input Device (RX)')
    expect([...rx.options].map((o) => o.textContent?.trim())).toEqual([
      'System default',
      'Microphone (USB Audio CODEC)',
      'Microphone Array (Intel Smart Sound)',
    ])
  })
})

describe('a saved device that is not in the offered list', () => {
  it('stays selectable and says only what we actually know', async () => {
    // ⚠️ THIS USED TO SAY "— not detected", AND THAT ASSERTED KNOWLEDGE WE DO NOT HAVE.
    // The flag is computed against the list the picker OFFERS, which on Linux is the PRUNED
    // ALSA hint set (one entry per card, `plughw` preferred, `dmix`/`dsnoop`/`front`/`iec958`
    // dropped). But a device is OPENED through cpal, whose own enumeration is a probe-open
    // sweep — a different set, and neither is a subset of the other. So a perfectly working
    // device that our pruning simply does not offer was being labelled missing: proven live
    // against a synthetic ALSA config, `saved dsnoop` → offered_in_picker=false,
    // opens_at_runtime=true.
    //
    // We cannot tell from here whether a stored name will open. What we CAN say is that it is
    // not one of the entries we are offering — which is the useful hint (a renumbered card, a
    // moved USB port) without the false claim. The authoritative answer already exists and is
    // accurate: an unresolvable device is a visible error at open time, naming the device.
    settings = { ...settings, audioIn: 'plughw:CARD=GONE,DEV=0' }
    api.get('getAudioDevices').mockImplementation(() => Promise.resolve(LINUX))
    renderPanel()
    await openRadioTab()
    const rx = await picker('Input Device (RX)')
    expect(rx.value).toBe('plughw:CARD=GONE,DEV=0')
    const texts = [...rx.options].map((o) => o.textContent?.trim())
    expect(texts).toContain('plughw:CARD=GONE,DEV=0 — saved, not in the list')
    expect(
      texts.some((t) => t?.includes('not detected')),
      'must not assert a device is absent — we only know it is not one we offer',
    ).toBe(false)
    // The enumerated entries are NOT flagged — a warning on everything says nothing.
    expect(
      [...rx.options].filter((o) => o.textContent?.includes('not in the list')),
    ).toHaveLength(1)
  })
})
