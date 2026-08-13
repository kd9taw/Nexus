// @vitest-environment jsdom
//
// SSTV was the only mode with a cockpit and no Settings section. Everything an operator chose
// for it — transmit mode, drive — lived in `useState` inside SstvView and died on every restart,
// and its ONE persisted setting (the ISS pass auto-arm) was filed inside the Rig & CAT fieldset
// on the Radio tab, where nothing about it is a rig model, port, baud, framing or keying.
//
// These tests pin the section and the move. The same 2026-07-29 discoverability ruling that put
// APRS beside RTTY applies: a setting nobody can find is a setting that does not exist.
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.2'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
})
afterEach(cleanup)

const openTab = async (name: string) =>
  fireEvent.click(await screen.findByRole('tab', { name }))

/** The fieldset a control sits in, by its <legend>. */
const sectionOf = (el: HTMLElement) =>
  el.closest('fieldset')?.querySelector('legend')?.textContent ?? null

describe('the SSTV section', () => {
  it('renders every control under the SSTV legend on the Digital tab', async () => {
    renderPanel()
    await openTab('Digital')
    for (const label of [
      'Start receiving when SSTV opens',
      'ISS SSTV auto-arm',
      'Transmit mode',
      'Transmit power',
    ]) {
      const el = await screen.findByText(label)
      expect(sectionOf(el), `"${label}" must sit under the SSTV legend`).toBe('SSTV')
    }
  })

  it('the ISS auto-arm is NOT in Rig & CAT any more', async () => {
    // It spent its whole life inside the Rig & CAT fieldset, where nothing else is about a
    // satellite pass. Finding it back on the Radio tab means the move was reverted.
    renderPanel()
    await openTab('Radio')
    expect(screen.queryByText('ISS SSTV auto-arm')).toBeNull()
  })

  it('SSTV sits beside the other per-mode sections, between RTTY and APRS', async () => {
    renderPanel()
    await openTab('Digital')
    const legends = [...document.querySelectorAll('fieldset legend')].map((l) => l.textContent)
    expect(legends).toContain('SSTV')
    expect(legends).toContain('RTTY')
    expect(legends).toContain('APRS')
    // Order is load-bearing: registry.test.ts compares the registry array against the panel's
    // source order per tab, so a JSX move without a registry move publishes a wrong manual.
    expect(legends.indexOf('RTTY')).toBeLessThan(legends.indexOf('SSTV'))
    expect(legends.indexOf('SSTV')).toBeLessThan(legends.indexOf('APRS'))
  })

  it('the receive switch defaults ON, and the ISS one OFF', async () => {
    // ⚠️ THE DEFAULTS ARE THE WHOLE POINT OF THIS ONE. Starting the receiver on view entry is
    // the fix for the field bug where the ordinary way to use SSTV decoded nothing; an absent
    // key must read as ON (`!== false`, never `!!`). The ISS arm touches the rig, so it stays
    // opt-in.
    renderPanel()
    await openTab('Digital')
    const rx = (await screen.findByText('Start receiving when SSTV opens'))
      .closest('label')
      ?.querySelector('button')
    expect(rx!.getAttribute('aria-checked')).toBe('true')
    const iss = (await screen.findByText('ISS SSTV auto-arm')).closest('label')?.querySelector('button')
    expect(iss!.getAttribute('aria-checked')).toBe('false')
  })

  it('the transmit defaults are "leave it alone": Automatic mode and a blank power', async () => {
    // A power default of 100 would take an operator whose rig sits at 20 W for a reason to full
    // power on their first Send after upgrading. Blank means Nexus never issues the command.
    renderPanel()
    await openTab('Digital')
    const mode = (await screen.findByText('Transmit mode'))
      .closest('label')
      ?.querySelector('select') as HTMLSelectElement
    expect(mode.value).toBe('auto')
    // All 15 transmittable modes are offered beside it, grouped by family.
    expect(mode.querySelectorAll('option')).toHaveLength(16) // 15 + Automatic
    expect(mode.querySelectorAll('optgroup')).toHaveLength(4)

    const power = (await screen.findByText('Transmit power'))
      .closest('label')
      ?.querySelector('input') as HTMLInputElement
    expect(power.value).toBe('')
  })
})
