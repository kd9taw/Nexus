// @vitest-environment jsdom
//
// SPOT SOURCES ▸ CLUSTER NODES — the automatic choice shows what Nexus is doing, and the operator's
// own list comes back exactly as it was saved.
//
// Rendered through the whole Settings panel, not the component alone: the seam worth testing is
// the form's `clusterNodesAuto` and `clusterHosts` reaching the rows, and a stub would only prove
// that props arrive.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { ClusterNodes } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module.
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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** An operator's own list, deliberately not a list Nexus ever shipped. */
const MY_LIST = ['dx.example.net:7300', 'cluster.example.org:23']

const IN_A_DAY = Math.floor(Date.now() / 1000) + 86_400

function standings(auto: boolean): ClusterNodes {
  return {
    auto,
    nodes: [
      { host: 'dxspots.com:7300', callsign: 'AE5E', software: 'ccCluster', running: true, connected: true },
      { host: 'dx.w1nr.net:23', callsign: 'W1NR', software: 'dxSpider', running: false, connected: false },
      {
        host: 'nd4x.com:7373',
        callsign: 'ND4X',
        software: 'ccCluster',
        running: false,
        connected: false,
        failure: 'noPrompt',
        skippedUntilUnix: IN_A_DAY,
      },
      { host: 'cluster.n2wq.com:7373', callsign: 'N2WQ', software: 'ccCluster', running: true, connected: false },
      ...MY_LIST.map((host) => ({ host, running: false, connected: false })),
    ],
  }
}

function setUp(auto: boolean) {
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.13.0'))
  api.get('getClusterNodes').mockImplementation(() => Promise.resolve(standings(auto)))
  api
    .get('getSettings')
    .mockImplementation(() =>
      Promise.resolve({ ...defaultSettings, mycall: 'W1AW', mygrid: 'FN31', clusterNodesAuto: auto, clusterHosts: MY_LIST } as never),
    )
}

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

async function openSpotSources() {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Logging & Connectors' }))
  return screen.findByRole('radiogroup', { name: 'Phone/SSB cluster nodes' })
}

beforeEach(() => setUp(true))
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('cluster nodes, automatic', () => {
  it('shows each built-in node with what Nexus is doing with it (T14)', async () => {
    const group = await openSpotSources()
    expect(
      (within(group).getByRole('radio', { name: 'Pick working nodes automatically (recommended)' }) as HTMLInputElement)
        .checked,
    ).toBe(true)
    const row = async (host: string) => (await screen.findByText(host)).closest('li')!
    expect((await row('dxspots.com:7300')).textContent).toContain('In use')
    expect((await row('dx.w1nr.net:23')).textContent).toContain('Standby')
    expect((await row('cluster.n2wq.com:7373')).textContent).toContain('Connecting')
    expect((await row('nd4x.com:7373')).textContent).toMatch(/Not answering \(no login prompt\), skipped until \d\d:\d\d/)
    // Automatic rows are what Nexus is doing, not a list to edit: none of the operator's own
    // nodes, and no text boxes.
    expect(screen.queryByText('dx.example.net:7300')).toBeNull()
    expect(screen.queryByDisplayValue('dx.example.net:7300')).toBeNull()
  })

  it('switching to "Use my list" shows the saved list exactly as saved (T14)', async () => {
    const group = await openSpotSources()
    fireEvent.click(within(group).getByRole('radio', { name: 'Use my list' }))
    const boxes = await screen.findAllByDisplayValue(/example\.(net|org)/)
    expect(boxes.map((b) => (b as HTMLInputElement).value)).toEqual(MY_LIST)
    // The standings describe the SAVED automatic choice, so an unsaved switch shows none.
    expect(screen.queryByText('In use')).toBeNull()
    // Nothing was saved just by looking.
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('offers the built-in nodes as the known-node presets', async () => {
    setUp(false)
    await openSpotSources()
    const picker = await screen.findByRole('option', { name: '+ Add a known node…' })
    const options = () =>
      within(picker.closest('select')!).getAllByRole('option').map((o) => o.textContent)
    await waitFor(() => expect(options()).toContain('W1NR — DXSpider, port 23'))
    expect(options()).toContain('AE5E — CC Cluster, port 7300')
    expect(options().some((o) => o?.includes('example'))).toBe(false)
  })
})

describe('cluster nodes, my list', () => {
  it('shows each saved node with how it is doing, and never offers a switch of its own', async () => {
    setUp(false)
    api.get('getClusterNodes').mockImplementation(() =>
      Promise.resolve({
        auto: false,
        nodes: [
          { host: 'dx.example.net:7300', running: true, connected: true },
          { host: 'cluster.example.org:23', running: true, connected: false, failure: 'noGreeting' },
        ],
      } satisfies ClusterNodes),
    )
    const group = await openSpotSources()
    expect((within(group).getByRole('radio', { name: 'Use my list' }) as HTMLInputElement).checked).toBe(true)
    const rowOf = async (host: string) => (await screen.findByDisplayValue(host)).closest('.cluster-node-row')!
    await waitFor(async () => expect((await rowOf('dx.example.net:7300')).textContent).toContain('In use'))
    expect((await rowOf('cluster.example.org:23')).textContent).toContain('Not answering (sends nothing)')
    expect((await rowOf('cluster.example.org:23')).textContent).not.toContain('skipped')
  })
})
