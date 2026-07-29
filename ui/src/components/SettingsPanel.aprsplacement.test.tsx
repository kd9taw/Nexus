// @vitest-environment jsdom
//
// The APRS-IS / iGate settings first shipped under Logging & Connectors ▸ Integrations & Feeds,
// alongside the other network feeds. That is where they belong by TYPE — and it is not where
// anybody looked for them. The first operator to go hunting went to the APRS settings, did not
// find them, and reported them missing (2026-07-29 ruling).
//
// They now live in Modes ▸ APRS, beside RTTY / CW / Phone, because a setting nobody can find is a
// setting that does not exist. These tests pin the placement so a later tidy-up cannot quietly
// file them back under the heading that reads better but does not get found.
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
    'getFccStatesStatus', 'discoverFlex', 'civDiagnosticLog', 'civDiagnosticStatus',
    'allTxtLocation', 'revealAllTxt', 'appVersion', 'getSpectrumRow', 'setFrequency',
    'getWatchlist', 'setWatchlist', 'openPanelWindow',
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
  // The Logging tab renders these two as lists; the default `null` throws there but nowhere else.
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

describe('APRS-IS settings live where operators look for them', () => {
  it('every one of the nine controls is on the Modes tab under APRS', async () => {
    // The whole group moves together — a half-move that stranded the iGate toggle behind the old
    // heading would be worse than not moving at all.
    renderPanel()
    await openTab('Modes')
    for (const label of [
      'APRS-IS feed',
      'Server',
      'Port',
      'Radius (km)',
      'Watched calls',
      'Weather stations',
      'Objects & items',
      'Messages',
      'Receive-only iGate',
    ]) {
      const el = await screen.findByText(label)
      expect(sectionOf(el), `"${label}" must sit under the APRS legend`).toBe('APRS')
    }
  })

  it('they are NOT under Logging & Connectors any more', async () => {
    renderPanel()
    await openTab('Logging & Connectors')
    // The old home. Finding the iGate toggle here again means the move was reverted.
    expect(screen.queryByText('Receive-only iGate')).toBeNull()
    expect(screen.queryByText('APRS-IS feed')).toBeNull()
  })

  it('the APRS section sits with the other per-mode settings, not off on its own', async () => {
    // Discoverability was the entire complaint: an operator scanning the Modes tab for APRS should
    // meet it in the same list as the modes they already know.
    renderPanel()
    await openTab('Modes')
    const legends = [...document.querySelectorAll('fieldset legend')].map((l) => l.textContent)
    expect(legends).toContain('APRS')
    expect(legends).toContain('RTTY')
    expect(legends).toContain('CW')
  })

  it('the enable-gate survived the move — dependent controls stay disabled until the feed is on', async () => {
    // `disabled={!form.aprsIsEnabled}` on eight controls is easy to drop when re-indenting a
    // 180-line block. The default is feed-off, so every dependent control must start disabled.
    renderPanel()
    await openTab('Modes')
    const server = (await screen.findByText('Server')).closest('label')?.querySelector('input')
    expect(server).toBeTruthy()
    expect(server!.disabled).toBe(true)

    const igate = (await screen.findByText('Receive-only iGate'))
      .closest('label')
      ?.querySelector('button')
    expect(igate).toBeTruthy()
    expect(igate!.disabled).toBe(true)

    // ...and the master switch itself is never gated, or nothing could ever be turned on.
    const master = (await screen.findByText('APRS-IS feed'))
      .closest('label')
      ?.querySelector('button')
    expect(master!.disabled).toBe(false)
    expect(master!.getAttribute('aria-checked')).toBe('false')
  })
})
