// @vitest-environment jsdom
//
// The N1MM contact broadcast only spoke during a Field Day event, which made it useless for what a
// tester actually wanted it for: OpenHamClock listening for N1MM contact datagrams and plotting
// every QSO on its map as it is logged (2026-07-29 request). "Broadcast every QSO" is that standing
// switch. These tests pin the two things that decide whether an operator can find and use it: it
// lives in the N1MM+ section on the tab where they went looking, and it starts OFF.
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
    'getFccStatesStatus', 'getTleStatus', 'fetchTlesNow', 'importTles', 'discoverFlex', 'civDiagnosticLog', 'civDiagnosticStatus',
    'allTxtLocation', 'revealAllTxt', 'recordingsLocation', 'revealRecordings', 'appVersion', 'getSpectrumRow', 'setFrequency',
    'getWatchlist', 'setWatchlist', 'openPanelWindow', 'getAssistanceJournal',
    'setUnassistedMode',
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.3'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
})
afterEach(cleanup)

const openLogging = async () =>
  fireEvent.click(await screen.findByRole('tab', { name: 'Logging & Connectors' }))

/** The fieldset a control sits in, by its <legend>. */
const sectionOf = (el: HTMLElement) =>
  el.closest('fieldset')?.querySelector('legend')?.textContent ?? null

const switchFor = async (label: string) =>
  (await screen.findByText(label)).closest('label')?.querySelector('button') as HTMLButtonElement

const addressField = async () =>
  (await screen.findByText('N1MM contact broadcast address'))
    .closest('label')
    ?.querySelector('input') as HTMLInputElement

describe('the N1MM broadcast can be switched on independently of Field Day', () => {
  it('the switch sits with the N1MM address, on the tab the tester went to', async () => {
    // Filed anywhere else it is a feature nobody finds — the whole point of the request.
    renderPanel()
    await openLogging()
    const el = await screen.findByText('Broadcast every QSO')
    expect(sectionOf(el)).toBe('N1MM+ Integration')
  })

  it('starts OFF — an upgrade never begins putting an operator on the network', async () => {
    renderPanel()
    await openLogging()
    expect((await switchFor('Broadcast every QSO')).getAttribute('aria-checked')).toBe('false')
    expect((await addressField()).value).toBe('')
  })

  it('turning it on with a blank address fills in the local target', async () => {
    // Otherwise the switch reads as broken: on, and silently sending nowhere.
    renderPanel()
    await openLogging()
    fireEvent.click(await switchFor('Broadcast every QSO'))
    expect((await switchFor('Broadcast every QSO')).getAttribute('aria-checked')).toBe('true')
    expect((await addressField()).value).toBe('127.0.0.1:12060')
  })

  it('says plainly that an address alone sends nothing outside Field Day', async () => {
    // The reported bug was a SILENT failure: the address sat there looking like a standing
    // integration beside HRD, which worked, and emitted nothing. A configured-but-silent
    // integration has to say so on the screen where it was configured.
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({
        ...defaultSettings,
        mycall: 'KD9TAW',
        n1mmAddr: '127.0.0.1:12061',
        n1mmUpload: false,
      } as never),
    )
    renderPanel()
    await openLogging()
    const hint = (await screen.findByText('N1MM contact broadcast address'))
      .closest('label')
      ?.querySelector('.settings-hint')
    expect(hint?.textContent).toMatch(/sends nothing outside a Field Day event/i)

    // ...and once it IS a standing output, it says that instead.
    fireEvent.click(await switchFor('Broadcast every QSO'))
    expect(hint?.textContent).toMatch(/Sending for every logged QSO/i)
  })

  it("never overwrites an address the operator already set", async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({
        ...defaultSettings,
        mycall: 'KD9TAW',
        n1mmAddr: '192.168.1.50:12060',
      } as never),
    )
    renderPanel()
    await openLogging()
    fireEvent.click(await switchFor('Broadcast every QSO'))
    expect((await addressField()).value).toBe('192.168.1.50:12060')
  })
})
