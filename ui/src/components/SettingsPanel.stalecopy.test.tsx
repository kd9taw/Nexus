// @vitest-environment jsdom
//
// A SETTINGS SAVE SENDS WHAT THE OPERATOR CHANGED, OVER THE LIVE SETTINGS. The form holds every
// field, as it was when Settings opened. Saving it whole wrote each field the operator had NOT
// touched back to that value, over whatever had changed since: "use one radio" chosen in the launch
// picker (Settings can be the view restored under it at launch), a seat swap in the pop-out
// scoreboard, the dial the rig had been tuned to — which the radio loop then commands, so the rig
// jumped back. Four saves send the form: Save, on the radio being operated and while another radio
// is being edited, and the two confirmation downloads, which save first so the download uses the
// account on screen. The backend here is a live one: a read returns what it holds NOW, and a save
// replaces it, so "changed elsewhere" is a change to `station` after the form has loaded.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within, type ByRoleMatcher } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import { SETTINGS_SECTIONS, SETTINGS_TABS } from '../settings/registry'
import type { FeaturesApi } from '../useFeatures'
import type { Settings } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "Save: 'use one radio' chosen after Settings opened…", takes 0.52 s
// and 0.48 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one
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
  flexRadioIp: '',
  flexNativePan: false,
  flexNativeAudio: false,
}
const IC7300 = {
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

let station: Record<string, unknown> = {}

/** Open Settings on `held`: the settings the form loads, and holds from then on. */
function openOn(held: Partial<Settings> = {}) {
  station = structuredClone({
    ...defaultSettings,
    ...FTDX10, // the flat mirror describes the ACTIVE radio
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10, IC7300],
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
    qrzLogbookUpload: false,
    lotwUsername: 'KD9TAW',
    eqslUsername: 'KD9TAW',
    ...held,
  })
  const features = {
    enabled: () => true,
    setEnabled: vi.fn(),
    all: () => [],
    profile: 'full',
    setProfile: vi.fn(),
  } as unknown as FeaturesApi
  render(
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

/** Something outside Settings changes the station's settings after the form has loaded. */
const changedElsewhere = (change: Partial<Settings>) => {
  station = { ...station, ...structuredClone(change) }
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
  api.get('getAssistanceJournal').mockImplementation(() => Promise.resolve([]))
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station)))
  api.get('setSettings').mockImplementation((s: Record<string, unknown>) => {
    station = structuredClone(s)
    return Promise.resolve(null)
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** The form has loaded: the Station tab shows the station's callsign. */
const loaded = () => screen.findAllByDisplayValue('KD9TAW')

/** The operator's own edit, on a tab with no radio in it: QRZ auto-upload on. */
async function turnOnQrzUpload() {
  fireEvent.click(screen.getByRole('tab', { name: 'Logging & Connectors' }))
  fireEvent.click(await screen.findByRole('switch', { name: /Auto-upload QSOs to QRZ/ }))
}

/** The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons. */
async function save() {
  const writes = api.get('setSettings').mock.calls.length
  fireEvent.click(
    screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
  )
  await waitFor(() => expect(api.get('setSettings').mock.calls.length).toBe(writes + 1))
}

const saves: [string, () => Promise<void>][] = [
  ['Save', async () => {
    await turnOnQrzUpload()
    await save()
  }],
  ['Save while another radio is being edited', async () => {
    fireEvent.click(screen.getByRole('tab', { name: 'Radio' }))
    fireEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    await screen.findByText(/Editing IC-7300/)
    await turnOnQrzUpload()
    await save()
  }],
  ['Download confirmations (LoTW)', async () => {
    await turnOnQrzUpload()
    const writes = api.get('setSettings').mock.calls.length
    fireEvent.click(await screen.findByRole('button', { name: 'Download confirmations' }))
    await waitFor(() => expect(api.get('setSettings').mock.calls.length).toBe(writes + 1))
  }],
  ['Sync eQSL now', async () => {
    await turnOnQrzUpload()
    const writes = api.get('setSettings').mock.calls.length
    fireEvent.click(await screen.findByRole('button', { name: 'Sync eQSL now' }))
    await waitFor(() => expect(api.get('setSettings').mock.calls.length).toBe(writes + 1))
  }],
]

describe('a save of the form keeps what changed elsewhere since Settings opened', () => {
  it.each(saves)("%s: 'use one radio' chosen after Settings opened stays chosen", async (_, saveTheForm) => {
    openOn({ simultaneousRadios: true })
    await loaded()
    changedElsewhere({ simultaneousRadios: false })
    await saveTheForm()
    expect(station.qrzLogbookUpload).toBe(true)
    expect(station.simultaneousRadios).toBe(false)
  })

  it.each(saves)('%s: the frequency the radio was tuned to since stays where it is', async (_, saveTheForm) => {
    openOn({ dialMhz: 14.074, band: '20m', sideband: 'USB' })
    await loaded()
    changedElsewhere({ dialMhz: 7.03, band: '40m', sideband: 'LSB' })
    await saveTheForm()
    expect(station.qrzLogbookUpload).toBe(true)
    expect([station.dialMhz, station.band, station.sideband]).toEqual([7.03, '40m', 'LSB'])
  })

  it('a seat swap made in the pop-out scoreboard keeps the new operator', async () => {
    openOn({ fdOperator: 'K1ABC' })
    await loaded()
    changedElsewhere({ fdOperator: 'W9XYZ' })
    await turnOnQrzUpload()
    await save()
    expect(station.qrzLogbookUpload).toBe(true)
    expect(station.fdOperator).toBe('W9XYZ')
  })
})

describe('what the operator changed in the form is what is saved', () => {
  it('over a change made elsewhere to the same field', async () => {
    // Three different values, so the save can be told apart from both the form's load and the
    // change made elsewhere.
    openOn({ fdOperator: 'K1ABC' })
    await loaded()
    changedElsewhere({ fdOperator: 'W9XYZ' })
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    fireEvent.change(await screen.findByDisplayValue('K1ABC'), { target: { value: 'N0AAA' } })
    await save()
    expect(station.fdOperator).toBe('N0AAA')
  })

  it('and when the radio being operated changed under the form, the form goes whole', async () => {
    // The flat rig fields describe the radio `activeRadio` names: the backend folds them into
    // that radio's profile. After a switch the form has not caught up with, the live flat fields
    // describe the OTHER radio, and a mix of the two would stamp one radio's ports onto the
    // other. So the form is sent as it stands, and the backend's own rule for a form that
    // describes a radio other than the active one applies.
    openOn()
    await loaded()
    changedElsewhere({ ...IC7300, activeRadio: 1 })
    await turnOnQrzUpload()
    await save()
    expect(station.qrzLogbookUpload).toBe(true)
    expect([station.activeRadio, station.serialPort, station.rigModel]).toEqual([0, 'COM3', 1042])
  })
})

// EVERY SECTION'S EDIT STILL SAVES. Save now sends only what the form changed, so an edit made
// somewhere the comparison cannot see would never reach the station. One real control per section
// of the settings registry, edited the way the operator edits it, while the station's value of the
// same field is changed elsewhere as well: the save must carry the EDIT, not what the form loaded
// and not what the station holds. A switch has two values, so there the edit differs from the
// station's value only because it was left where the form loaded it. The sections that never save
// through the form are named with the writer they use instead, and every registry section is in
// exactly one of the two lists.
describe('an edit made in any section of Settings is saved', () => {
  /** The section's own element: its fieldset, or a group's box. */
  const inSection = async (id: string) =>
    within(
      await waitFor(() => {
        const el = document.getElementById(`settings-${id}`)
        if (!el) throw new Error(`section ${id} is not on screen`)
        return el
      }),
    )
  const clickIn = (id: string, role: ByRoleMatcher, name: string | RegExp) => async () => {
    fireEvent.click((await inSection(id)).getByRole(role, { name }))
  }
  const setIn = (id: string, role: ByRoleMatcher, name: string | RegExp, value: string) => async () => {
    fireEvent.change((await inSection(id)).getByRole(role, { name }), { target: { value } })
  }
  /** A group that renders its fields only once opened. */
  const opened = (id: string, edit: () => Promise<void>) => async () => {
    fireEvent.click((await inSection(id)).getByRole('button', { expanded: false }))
    await edit()
  }

  interface SectionEdit {
    section: string
    /** The settings the form loads, and the change made elsewhere before Save. */
    load: Partial<Settings>
    elsewhere: Partial<Settings>
    edit: () => Promise<void>
    /** Where the edit lands, and what it must be. */
    saved: (s: Record<string, unknown>) => unknown
    expected: unknown
  }
  /** One field: as loaded, as changed elsewhere, as the edit must save it. */
  const field = (key: keyof Settings, loaded: unknown, live: unknown, expected: unknown) => ({
    load: { [key]: loaded } as Partial<Settings>,
    elsewhere: { [key]: live } as Partial<Settings>,
    saved: (s: Record<string, unknown>) => s[key],
    expected,
  })
  const macros = defaultSettings.macros as unknown as Settings['macros']

  const EDITS: SectionEdit[] = [
    { section: 'operator-radio', ...field('mygrid', 'EN52', 'EN61', 'FN31'), edit: setIn('operator-radio', 'textbox', /^Grid/, 'FN31') },
    { section: 'radios', ...field('simultaneousRadios', false, false, true), edit: clickIn('radios', 'checkbox', 'Run both radios at the same time') },
    { section: 'rig-control', ...field('wheelTuneSensitivity', 1, 0.5, 1.5), edit: setIn('rig-control', 'slider', 'Mouse-wheel tuning sensitivity', '1.5') },
    { section: 'rig-advanced', ...field('dataModesPlainSsb', false, false, true), edit: opened('rig-advanced', clickIn('rig-advanced', 'switch', /^Data modes use plain SSB/)) },
    { section: 'audio', ...field('rxGain', 1, 2, 3), edit: setIn('audio', 'slider', 'RX capture gain', '3') },
    { section: 'headphone-monitor', ...field('monitorEnabled', false, false, true), edit: clickIn('headphone-monitor', 'checkbox', 'Play receive audio on this computer') },
    { section: 'satellite-doppler', ...field('satDopplerOff', false, false, true), edit: clickIn('satellite-doppler', 'checkbox', 'Enable satellite Doppler correction') },
    {
      section: 'rotator',
      ...field('rotatorPort', '', 'COM5', 'COM7'),
      load: { rotatorModel: 601, rotatorPort: '' }, // the port field shows once a rotator is chosen
      edit: setIn('rotator', 'textbox', 'Rotator serial port', 'COM7'),
    },
    { section: 'amplifier', ...field('ampModel', '', 'kpa', 'spe'), edit: setIn('amplifier', 'combobox', 'Amplifier', 'spe') },
    { section: 'transmit-limits', ...field('bandEdgeTones', true, true, false), edit: clickIn('transmit-limits', 'switch', /^Band-edge tones/) },
    { section: 'digital-ft8-ft4', ...field('txWatchdogMin', 5, 7, 9), edit: setIn('digital-ft8-ft4', 'spinbutton', /^Tx Watchdog \(min\)/, '9') },
    { section: 'jt65', ...field('jt65Submode', 0, 1, 2), edit: setIn('jt65', 'combobox', /^Submode \(tone spacing\)/, '2') },
    { section: 'msk144', ...field('msk144PeriodS', 15, 10, 30), edit: setIn('msk144', 'combobox', /^T\/R period/, '30') },
    { section: 'beacons-wspr-fst4w', ...field('beaconPowerDbm', 23, 27, 30), edit: setIn('beacons-wspr-fst4w', 'spinbutton', /^Transmit power \(dBm\)/, '30') },
    { section: 'fst4', ...field('fst4PeriodS', 120, 60, 300), edit: setIn('fst4', 'combobox', /^T\/R period/, '300') },
    { section: 'q65', ...field('q65PeriodS', 60, 30, 120), edit: setIn('q65', 'combobox', /^T\/R period/, '120') },
    {
      section: 'quick-reply-macros',
      load: { macros: { ...macros, chat: ['73'] } },
      elsewhere: { macros: { ...macros, chat: ['GL'] } },
      saved: (s) => (s.macros as Settings['macros']).chat,
      expected: ['TNX', '73'],
      edit: setIn('quick-reply-macros', 'textbox', /^Chat/, 'TNX, 73'),
    },
    { section: 'phone', ...field('phoneMode', 'ssb', 'ssb', 'fm'), edit: setIn('phone', 'combobox', /^Phone mode/, 'fm') },
    { section: 'cw', ...field('cwPitchHz', 600, 650, 700), edit: setIn('cw', 'spinbutton', /^Sidetone pitch \(Hz\)/, '700') },
    { section: 'rtty', ...field('rttyRxAutoArm', true, true, false), edit: clickIn('rtty', 'switch', /^Start receiving when RTTY opens/) },
    { section: 'psk', ...field('pskRxAutoArm', true, true, false), edit: clickIn('psk', 'switch', /^Start receiving when PSK opens/) },
    { section: 'js8', ...field('js8Speed', 1, 2, 3), edit: setIn('js8', 'combobox', /^Transmit speed/, '3') },
    { section: 'sstv', ...field('sstvRxAutoArm', true, true, false), edit: clickIn('sstv', 'switch', /^Start receiving when SSTV opens/) },
    { section: 'aprs', ...field('aprsComment', 'Nexus APRS', 'Mobile', 'Portable'), edit: setIn('aprs', 'textbox', /^Beacon comment/, 'Portable') },
    {
      section: 'working-frequencies',
      ...field('workingFrequencies', [], [{ band: '40m', mode: 'FT8', mhz: 7.074 }], [{ band: '20m', mode: 'FT8', mhz: 14.074 }]),
      edit: clickIn('working-frequencies', 'button', 'Add override'),
    },
    { section: 'pounce', ...field('pounceThreshold', 'off', 'atno', 'atnoOrZone'), edit: setIn('pounce', 'combobox', /^Alert me for/, 'atnoOrZone') },
    { section: 'alerts', ...field('alertMyCall', true, true, false), edit: clickIn('alerts', 'switch', /^My call/) },
    { section: 'accessibility', ...field('announceVerbosity', 'needed', 'off', 'all'), edit: setIn('accessibility', 'combobox', /^Announce decodes/, 'all') },
    { section: 'connections-b4', ...field('b4MatchMode', false, false, true), edit: clickIn('connections-b4', 'switch', /^Match mode too/) },
    { section: 'integrations-feeds', ...field('wsjtxUdp', false, false, true), edit: clickIn('integrations-feeds', 'switch', /^WSJT-X UDP API/) },
    { section: 'antenna-gain', ...field('antTxGainDbi', 0, 3, 6), edit: opened('antenna-gain', setIn('antenna-gain', 'spinbutton', 'TX antenna gain (dBi)', '6')) },
    { section: 'dxkeeper', ...field('dxkeeperHost', '', '10.0.0.2', '10.0.0.9'), edit: setIn('dxkeeper', 'textbox', /^DXKeeper host/, '10.0.0.9') },
    { section: 'n3fjp', ...field('n3fjpHost', '', '10.0.0.2', '10.0.0.9'), edit: setIn('n3fjp', 'textbox', /^N3FJP host/, '10.0.0.9') },
    { section: 'n1mm', ...field('n1mmAddr', '', '10.0.0.2:12060', '10.0.0.9:12060'), edit: setIn('n1mm', 'textbox', /^N1MM contact broadcast address/, '10.0.0.9:12060') },
    { section: 'lotw-users', ...field('lotwMaxAgeDays', 365, 90, 180), edit: setIn('lotw-users', 'spinbutton', /^Count as a LoTW user/, '180') },
    { section: 'confirmations', ...field('lotwUsername', 'KD9TAW', 'KD9TAW/P', 'KD9TAW/M'), edit: setIn('confirmations', 'textbox', /^LoTW username/, 'KD9TAW/M') },
    { section: 'contest-pick', ...field('contestEmail', '', 'a@example.org', 'b@example.org'), edit: setIn('contest-pick', 'textbox', /^Email for contest logs/, 'b@example.org') },
    { section: 'contest-station', ...field('contestQthState', '', 'WI', 'IL'), edit: setIn('contest-station', 'textbox', /^State or province/, 'il') },
    { section: 'contest-category', ...field('unassistedMode', false, false, true), edit: clickIn('contest-category', 'switch', 'Declare an unassisted contest entry') },
    { section: 'scp-call-history', ...field('scpEnabled', true, true, false), edit: clickIn('scp-call-history', 'switch', 'Super Check Partial') },
    { section: 'contest-keys', ...field('contestEsmCw', false, false, true), edit: clickIn('contest-keys', 'switch', 'ESM in the CW cockpit') },
    { section: 'field-day', ...field('fdClass', '1A', '3A', '2A'), edit: setIn('field-day', 'textbox', /^FD Class/, '2a') },
    { section: 'field-day-identity', ...field('fdOperator', 'K1ABC', 'W9XYZ', 'N0AAA'), edit: setIn('field-day-identity', 'textbox', /^Operator at the key/, 'N0AAA') },
    {
      section: 'field-day-club',
      ...field('fdEventName', '', 'Club FD', 'Nexus FD'),
      // No accessible name on this input (its label is a <span> in a <div>): found by its placeholder.
      edit: async () => {
        const input = (await inSection('field-day-club')).getByPlaceholderText('W9ABC Field Day')
        fireEvent.change(input, { target: { value: 'Nexus FD' } })
      },
    },
    { section: 'connect-web', ...field('connectWeb', false, false, true), edit: clickIn('connect-web', 'switch', 'Serve Conditions on the local network') },
    { section: 'features', ...field('fdActive', false, false, true), edit: clickIn('features', 'switch', 'Enable Field Day mode') },
  ]

  /** Sections whose controls never save through the form, and what they save through instead. */
  const NOT_THE_FORM: Record<string, string> = {
    'remote-access': 'its own pairing and access verbs',
    profiles: 'named configurations kept on this computer; Load merges one over a fresh read',
    'orbital-elements': 'the orbital-element download and import verbs',
    connections: 'the credential verbs; the rest is connection status',
    'callsign-state': 'a download verb',
    'country-file': 'a download verb',
    workspace: 'display preferences kept on this computer',
    theme: 'display preferences kept on this computer',
    colours: 'display preferences kept on this computer',
    'waterfall-scopes': 'display preferences kept on this computer',
    'map-globe': 'display preferences kept on this computer',
    performance: 'display preferences kept on this computer',
    'app-updates': 'setBetaUpdates, the beta channel’s one writer',
    'start-at-sign-in': 'setLaunchAtLogin, the login entry’s one writer',
    'data-folder': 'setDataFolder',
    configurations: 'the backup, restore and reset verbs',
  }

  it('names every section of the registry exactly once', () => {
    const named = [...EDITS.map((e) => e.section), ...Object.keys(NOT_THE_FORM)].sort()
    expect(named).toEqual(SETTINGS_SECTIONS.map((s) => s.id).sort())
  })

  it.each(EDITS)('$section', async ({ section, load, elsewhere, edit, saved, expected }) => {
    openOn(load)
    await loaded()
    changedElsewhere(elsewhere)
    const tab = SETTINGS_SECTIONS.find((s) => s.id === section)!.tab
    fireEvent.click(screen.getByRole('tab', { name: SETTINGS_TABS.find((t) => t.id === tab)!.label }))
    await edit()
    await save()
    expect(saved(station)).toEqual(expected)
  })
})
