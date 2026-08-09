// @vitest-environment jsdom
//
// The Tx Power (drive) slider: a square-law position<->level curve and a throttled live-apply.
//
// Two bugs, found chasing a multi-second lag between moving this slider and the rig's ALC
// meter actually settling: (1) this control only committed to the live radio on
// pointerUp/keyUp, so dragging gave zero feedback until release, then jumped in one step; and
// (2) once made live on every drag tick, the real hardware-usable drive range (0 up to just
// past where ALC engages) turned out to sit in only the bottom ~15-20% of a linear slider,
// leaving almost no travel for the region an operator actually needs to set precisely.
//
// Fix: apply live on every onChange, throttled to ~60ms (not gated on release) so it stays
// responsive without hammering disk (setTxLevel persists the whole settings.json on every
// call); pointerUp/keyUp force an unthrottled apply so the settled value is never dropped.
// And a square-law curve (slider position² -> tx_level, √tx_level -> position) so the
// ALC-critical bottom of the range gets most of the slider's travel. What tx_level itself
// means to the engine is unchanged — only how far the slider travels to reach a given level.
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

/** A serial-CAT radio with tx_level = 0.25, so its slider POSITION under the curve is 0.5. */
function serialRadio() {
  const radio = {
    id: 0,
    name: 'Radio 1',
    enabled: true,
    serialPort: 'COM3',
    baud: 38400,
    rigModel: 0,
    rigModelName: '',
    rigConn: 'serial',
    rigAddr: '',
    rigctldPort: 4532,
    rotctldPort: 4533,
    icomNativeCat: false,
    audioIn: 'in-0',
    audioOut: 'out-0',
    txLevel: 0.25,
    rxGain: 1,
    pttMethod: 'cat',
    rotatorModel: 0,
    rotatorPort: '',
    rotatorBaud: 9600,
    rotatorHost: '',
    nativeScope: 'auto',
    bands: [],
  }
  return {
    ...defaultSettings,
    ...radio,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [radio],
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
  api.get('getSettings').mockImplementation(() => Promise.resolve(serialRadio()))
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.0.1'))
})
afterEach(cleanup)

async function openRadioTab() {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await screen.findByLabelText('Transmit drive level')
}

const slider = () => screen.getByLabelText('Transmit drive level') as HTMLInputElement

describe('Tx Power slider: square-law curve', () => {
  it('shows slider POSITION as √level, not the raw level', async () => {
    await openRadioTab()
    // txLevel is 0.25 -> position is √0.25 = 0.5.
    expect(Number(slider().value)).toBeCloseTo(0.5, 5)
  })

  it('onChange squares the slider position back to a level and applies it live', async () => {
    await openRadioTab()
    fireEvent.change(slider(), { target: { value: '0.7' } })
    expect(api.spies.setTxLevel).toHaveBeenCalledWith(0.7 ** 2)
  })
})

describe('Tx Power slider: throttled live-apply', () => {
  it('applies the FIRST change immediately (no gate on release)', async () => {
    await openRadioTab()
    fireEvent.change(slider(), { target: { value: '0.6' } })
    expect(api.spies.setTxLevel).toHaveBeenCalledTimes(1)
    expect(api.spies.setTxLevel).toHaveBeenCalledWith(0.6 ** 2)
  })

  it('suppresses a second change inside the throttle window, but pointerUp force-applies', async () => {
    // FAKE TIMERS, and they are not optional. The throttle compares `Date.now()`, so this case
    // used to rely on two fireEvents landing inside a ~60ms real-time window — which holds on an
    // idle machine and does NOT under load: seen failing twice during parallel builds on
    // 2026-08-08, passing alone every time. A timing test that only passes when the box is quiet
    // is a test that eventually fails in CI for no reason anyone can reproduce.
    // ...and enabled AFTER the async setup: testing-library's findBy* polls on timers, so
    // faking them before `openRadioTab` hangs it until the 5s test timeout.
    await openRadioTab()
    vi.useFakeTimers()
    try {
      fireEvent.change(slider(), { target: { value: '0.4' } })
      expect(api.spies.setTxLevel).toHaveBeenCalledTimes(1)

      // No time has passed at all now, so this is unambiguously inside the window.
      fireEvent.change(slider(), { target: { value: '0.9' } })
      expect(api.spies.setTxLevel).toHaveBeenCalledTimes(1)

      // Release must still land the final value even though the throttle window is open.
      fireEvent.pointerUp(slider(), { target: { value: '0.9' } } as never)
      expect(api.spies.setTxLevel).toHaveBeenCalledTimes(2)
      expect(api.spies.setTxLevel).toHaveBeenLastCalledWith(0.9 ** 2)
    } finally {
      vi.useRealTimers()
    }
  })
})
