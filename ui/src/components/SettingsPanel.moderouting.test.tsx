// @vitest-environment jsdom
//
// Band+mode radio routing, from the Settings side. The operator's shack has THREE radios and two of
// them (IC-9700, FT-991A) both cover 2 m — so band coverage alone cannot decide which one gets a
// 2 m action, and the MODE has to. This pins the editor's contract:
//   * a rule edit goes through the LIVE verb `setRoutingRules`, never the settings form. The rig
//     form's Save routes through `updateRadioProfile` while a non-active radio is being edited and
//     drops the form entirely, so a form-state rules table would silently vanish (the exact bug
//     class SettingsPanel.radiorouting.test.tsx exists for).
//   * list ORDER is the first-match-wins precedence, so reordering must be reachable.
//   * the "test" affordance asks the BACKEND resolver — never a second copy of the rule logic in
//     the UI, which would be free to drift from what the radio loop actually does.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
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
    'getFccStatesStatus', 'getTleStatus', 'fetchTlesNow', 'importTles', 'discoverFlex', 'civDiagnosticLog', 'civDiagnosticStatus',
    'allTxtLocation', 'revealAllTxt', 'appVersion', 'getSpectrumRow', 'setFrequency',
    'getWatchlist', 'setWatchlist', 'openPanelWindow',
    'setRoutingRules', 'setDefaultRadio', 'routePreview',
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

const FTDX10 = {
  id: 0,
  name: 'FTdx10',
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
  pttSerialPort: '',
  dataModesPlainSsb: false,
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
  rigModel: 3081,
  rigModelName: 'Icom IC-9700',
  rigctldPort: 4534,
  rotctldPort: 4535,
  audioIn: 'in-1',
  audioOut: 'out-1',
  bands: ['2m', '70cm'],
}
const FT991A = {
  ...FTDX10,
  id: 2,
  name: 'FT-991A',
  serialPort: 'COM9',
  rigModel: 1035,
  rigModelName: 'Yaesu FT-991A',
  rigctldPort: 4536,
  rotctldPort: 4537,
  audioIn: 'in-2',
  audioOut: 'out-2',
  bands: ['2m', '70cm'],
}

/** The operator's real shack: HF rig + a VHF weak-signal-digital rig + a VHF FM/APRS rig, with the
 * band+mode rules that split 2 m between the last two. FTdx10 active (where an HF day starts). */
function threeRadioSettings() {
  return {
    ...defaultSettings,
    ...FTDX10,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10, IC9700, FT991A],
    routingRules: [
      { bands: ['2m', '70cm'], mode: 'fm', radio: 2 },
      { bands: ['2m', '70cm'], mode: 'digital', radio: 1 },
    ],
    defaultRadio: 0,
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
  }
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.3'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(threeRadioSettings() as never))
  api.get('routePreview').mockImplementation(() => Promise.resolve({ radio: 2, name: 'FT-991A' }))
})
afterEach(cleanup)

const openRadioTab = async () => {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
}

describe('band+mode routing rules editor', () => {
  it('renders the operator three-radio table with every rule reachable', async () => {
    renderPanel()
    await openRadioTab()
    // Two rules, each with its own mode + radio selector — three radios all offered as targets.
    const rule1Mode = (await screen.findByLabelText('Rule 1 mode')) as HTMLSelectElement
    const rule1Radio = (await screen.findByLabelText('Rule 1 radio')) as HTMLSelectElement
    expect(rule1Mode.value).toBe('fm')
    expect(rule1Radio.value).toBe('2') // → the FT-991A
    const rule2Mode = (await screen.findByLabelText('Rule 2 mode')) as HTMLSelectElement
    const rule2Radio = (await screen.findByLabelText('Rule 2 radio')) as HTMLSelectElement
    expect(rule2Mode.value).toBe('digital')
    expect(rule2Radio.value).toBe('1') // → the IC-9700
    expect(
      [...rule1Radio.options].map((o) => o.textContent),
    ).toEqual(['FTdx10', 'IC-9700', 'FT-991A'])
  })

  it('editing a rule goes through the LIVE verb, never a whole-settings save', async () => {
    renderPanel()
    await openRadioTab()

    fireEvent.change(await screen.findByLabelText('Rule 1 radio'), { target: { value: '1' } })

    await waitFor(() => expect(api.get('setRoutingRules')).toHaveBeenCalled())
    const [rules] = api.get('setRoutingRules').mock.calls[0] as [
      { bands: string[]; mode: string | null; radio: number }[],
    ]
    expect(rules).toHaveLength(2)
    expect(rules[0]).toEqual({ bands: ['2m', '70cm'], mode: 'fm', radio: 1 })
    // A form save while the rig form points at another radio never sends the form at all, so a
    // form-state rules table would be dropped.
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('reordering rules is reachable, because ORDER is the precedence', async () => {
    renderPanel()
    await openRadioTab()

    // Rule 1 can't move up (it is already first) and rule 2 can't move down.
    expect((await screen.findByLabelText('Move rule 1 up')).hasAttribute('disabled')).toBe(true)
    expect((await screen.findByLabelText('Move rule 2 down')).hasAttribute('disabled')).toBe(true)

    fireEvent.click(await screen.findByLabelText('Move rule 2 up'))
    await waitFor(() => expect(api.get('setRoutingRules')).toHaveBeenCalled())
    const [rules] = api.get('setRoutingRules').mock.calls[0] as [{ mode: string | null }[]]
    expect(rules.map((r) => r.mode)).toEqual(['digital', 'fm'])
  })

  it('a new rule starts as any-band / any-mode rather than guessing', async () => {
    renderPanel()
    await openRadioTab()
    fireEvent.click(await screen.findByRole('button', { name: '+ Add routing rule' }))
    await waitFor(() => expect(api.get('setRoutingRules')).toHaveBeenCalled())
    const [rules] = api.get('setRoutingRules').mock.calls[0] as [
      { bands: string[]; mode: string | null; radio: number }[],
    ]
    expect(rules).toHaveLength(3)
    expect(rules[2]).toEqual({ bands: [], mode: null, radio: 0 })
  })

  it('band chips toggle onto the rule that owns them', async () => {
    renderPanel()
    await openRadioTab()
    // Each rule carries its own chip row; the second rule's 2 m chip is already on, so clicking
    // it must REMOVE 2 m from rule 2 and leave rule 1 alone.
    const chips = await screen.findAllByRole('button', { name: '2m' })
    // …the rows are: radio-card coverage (x3), then one per rule.
    fireEvent.click(chips[chips.length - 1])
    await waitFor(() => expect(api.get('setRoutingRules')).toHaveBeenCalled())
    const [rules] = api.get('setRoutingRules').mock.calls[0] as [{ bands: string[] }[]]
    expect(rules[1].bands).toEqual(['70cm'])
    expect(rules[0].bands).toEqual(['2m', '70cm'])
  })

  it('the default radio is its own live verb and can be cleared back to "stay put"', async () => {
    renderPanel()
    await openRadioTab()
    const sel = (await screen.findByLabelText('Default radio')) as HTMLSelectElement
    expect(sel.value).toBe('0')
    fireEvent.change(sel, { target: { value: '' } })
    await waitFor(() => expect(api.get('setDefaultRadio')).toHaveBeenCalledWith(null))
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('the test affordance asks the BACKEND resolver and names the radio', async () => {
    renderPanel()
    await openRadioTab()

    fireEvent.change(await screen.findByLabelText('Test band'), { target: { value: '2m' } })
    fireEvent.change(await screen.findByLabelText('Test mode'), { target: { value: 'fm' } })
    fireEvent.click(await screen.findByRole('button', { name: /where would this go/i }))

    // The UI must not re-implement the rule logic — a second copy is free to drift from the
    // resolver the radio loop actually calls.
    await waitFor(() => expect(api.get('routePreview')).toHaveBeenCalledWith('2m', 'fm'))
    // Assert on the result line specifically — "FT-991A" also appears as a radio name and as a
    // <select> option, so a bare text query would pass without the answer ever rendering.
    await waitFor(() => {
      const result = document.querySelector('.routing-test-result')
      expect(result?.textContent).toMatch(/2m FM & APRS.*FT-991A/)
    })
  })

  it('a single-radio station is never shown the rules editor', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({
        ...threeRadioSettings(),
        radios: [FTDX10],
        routingRules: [],
        defaultRadio: null,
      } as never),
    )
    renderPanel()
    await openRadioTab()
    // Wait for the roster to render before asserting on ABSENCE, else the test would pass simply
    // because nothing had mounted yet.
    await screen.findByRole('button', { name: '+ Add radio' })
    expect(screen.queryByRole('button', { name: '+ Add routing rule' })).toBeNull()
    expect(screen.queryByLabelText('Default radio')).toBeNull()
  })
})
