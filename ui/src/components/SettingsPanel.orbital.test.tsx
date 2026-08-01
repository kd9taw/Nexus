// @vitest-environment jsdom
//
// Settings ▸ Radio ▸ Orbital elements — the manual refresh speaks OPERATOR.
//
// What is pinned here (the honest-manual-refresh rework):
//
//  - The "Last refresh" line never speaks HTTP. While the mirror is
//    pre-launch-dead (404) and the elements are current, the line explains
//    calmly instead of alarming; the raw fetch error survives in the line's
//    title attribute for debugging — tooltip material, never the headline.
//  - The Update-now toast rides the same composer: a manual click whose
//    mirror leg died but whose Celestrak escalation landed announces
//    "fetched from Celestrak"; a genuinely failed attempt RESOLVES (the
//    command no longer rejects for a failed attempt) and toasts what failed
//    and what to do — again without HTTP in the headline.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import { pushToast } from '../toast'
import type { FeaturesApi } from '../useFeatures'
import type { TleStatus } from '../api'
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
    'civDiagnosticLog', 'civDiagnosticStatus', 'allTxtLocation', 'revealAllTxt', 'appVersion',
    'getSpectrumRow', 'setFrequency', 'getWatchlist', 'setWatchlist', 'openPanelWindow',
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

const NOW = Math.floor(Date.now() / 1000)

function tleStatus(over: Partial<TleStatus> = {}): TleStatus {
  return {
    count: 97,
    usableCount: 97,
    fetchedAt: NOW - 3600,
    source: 'mirror',
    importedCount: 0,
    elementAgeDays: 0.3,
    blockedUntil: 0,
    ...over,
  }
}

/** The pre-launch state: mirror 404s, elements current. */
const mirror404 = () =>
  tleStatus({
    lastError: 'TLE mirror fetch failed: HTTP 404',
    lastErrorKind: 'mirrorUnreachable',
  })

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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.24.5'))
  api.get('getTleStatus').mockImplementation(() => Promise.resolve(tleStatus()))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
  vi.mocked(pushToast).mockClear()
})
afterEach(cleanup)

const openRadio = async () => fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))

describe('the Last refresh line', () => {
  it('speaks operator for the pre-launch 404 mirror, raw error demoted to the tooltip', async () => {
    api.get('getTleStatus').mockImplementation(() => Promise.resolve(mirror404()))
    renderPanel()
    await openRadio()
    const line = await screen.findByText(/Last refresh:/)
    expect(line.textContent).toContain(
      "The element mirror isn't reachable (it goes live with the next release); your elements are 0.3 d old — current.",
    )
    // The headline never speaks HTTP…
    expect(line.textContent).not.toMatch(/HTTP|404/i)
    // …but the raw error survives for debugging, as the line's tooltip.
    expect(line.getAttribute('title')).toBe('TLE mirror fetch failed: HTTP 404')
  })
})

describe('the Update-now result', () => {
  it('announces a landed Celestrak escalation as such', async () => {
    api.get('fetchTlesNow').mockImplementation(() =>
      Promise.resolve(tleStatus({ source: 'celestrak', fetchedAt: NOW })),
    )
    renderPanel()
    await openRadio()
    fireEvent.click(await screen.findByRole('button', { name: 'Update now' }))
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(
        'Mirror unreachable — fetched from Celestrak: 97 birds',
        'success',
        expect.anything(),
      ),
    )
  })

  it('a failed attempt RESOLVES and toasts operator words, not HTTP', async () => {
    api.get('fetchTlesNow').mockImplementation(() =>
      Promise.resolve(
        tleStatus({
          lastError: 'TLE mirror fetch failed: HTTP 404; Celestrak TLE fetch failed: network: timeout',
          lastErrorKind: 'failed',
        }),
      ),
    )
    renderPanel()
    await openRadio()
    fireEvent.click(await screen.findByRole('button', { name: 'Update now' }))
    await waitFor(() => expect(pushToast).toHaveBeenCalled())
    const calls = vi.mocked(pushToast).mock.calls
    const [text, kind] = calls[calls.length - 1]
    expect(kind).toBe('error')
    expect(text).toBe(
      'Element update failed — no source delivered a usable set; retry shortly or import an element file.',
    )
    expect(text).not.toMatch(/HTTP/i)
  })

  it('the pre-launch 404 with current elements toasts calm info, not an error', async () => {
    api.get('fetchTlesNow').mockImplementation(() => Promise.resolve(mirror404()))
    renderPanel()
    await openRadio()
    fireEvent.click(await screen.findByRole('button', { name: 'Update now' }))
    await waitFor(() => expect(pushToast).toHaveBeenCalled())
    const calls = vi.mocked(pushToast).mock.calls
    const [text, kind] = calls[calls.length - 1]
    expect(kind).toBe('info')
    expect(text).toContain("The element mirror isn't reachable")
  })
})
