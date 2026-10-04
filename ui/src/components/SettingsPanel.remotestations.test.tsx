// @vitest-environment jsdom
//
// The Remote stations window's entry in Settings ▸ Station ▸ Remote access (the operator,
// 2026-10-04, "Stream client window"): the PC an operator works FROM opens the Remote page in a
// Nexus window of its own. Pinned here: it is offered on Windows only, at the END of the card (the
// beta line and the streaming switch stay first, as ordered on 2026-10-02), one click asks Rust to
// open the window once, a failure says so in words, and the Remote page's own copy of Settings
// never shows it — the page cannot open windows in this app, and must not appear to.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import { navigationPages } from '../remote-web/__fixtures__/navigation-page'
import configuration from '../remote-web/__fixtures__/configuration-settings.json'

const platform = vi.hoisted(() => ({ windows: true }))
vi.mock('../platform', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  return {
    ...actual,
    get IS_WINDOWS() {
      return platform.windows
    },
  }
})

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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel(remote = false) {
  const page = vi.fn(async () => {
    const pages = navigationPages('settings', configuration)
    return { ...pages[0], rows: pages.flatMap((p) => p.rows), nextCursor: null }
  })
  const panel = (
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
    />
  )
  return render(
    remote ? (
      <RemoteCollectionsContext.Provider value={{ page } as unknown as RemoteCollections}>
        {panel}
      </RemoteCollectionsContext.Provider>
    ) : (
      panel
    ),
  )
}

async function openStationTab() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Station' }))
  await waitFor(() => expect(document.getElementById('settings-remote-access')).toBeTruthy())
}

const card = () => document.getElementById('settings-remote-access')!
const theButton = () => screen.queryByRole('button', { name: EN['settings.remoteStations.open'] })
const opens = () => api.get('openRemoteStationsWindow')

beforeEach(() => {
  platform.windows = true
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.16.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' }),
  )
})
afterEach(cleanup)

describe('on Windows', () => {
  it('is offered at the end of Remote access, after the streaming switch, and says what F11 and Esc do', async () => {
    renderPanel()
    await openStationTab()
    const button = await waitFor(() => theButton()!)
    expect(card().contains(button)).toBe(true)
    const streaming = screen.getByRole('switch', { name: EN['settings.remoteStream.label'] })
    expect(streaming.compareDocumentPosition(button) & Node.DOCUMENT_POSITION_FOLLOWING, 'after the switch').toBeTruthy()
    const hint = screen.getByText(EN['settings.remoteStations.hint'])
    expect(hint.closest('.settings-field')?.contains(button)).toBe(true)
    expect(hint.textContent).toMatch(/F11 switches the window to full screen and back, and Esc over the picture still stops transmitting/)
  })

  it('asks Rust to open the window once per click, and holds the button while it does', async () => {
    let finish: () => void = () => {}
    opens().mockImplementation(() => new Promise<void>((resolve) => { finish = resolve }))
    renderPanel()
    await openStationTab()
    const button = await waitFor(() => theButton()!)
    fireEvent.click(button)
    expect(opens()).toHaveBeenCalledTimes(1)
    expect(opens().mock.calls[0]).toEqual([])
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(true))
    finish()
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false))
    expect(screen.queryByText(EN['settings.remoteStations.failed'])).toBeNull()
  })

  it('says so in words when the window could not be opened, and lets the operator try again', async () => {
    opens().mockImplementation(() => Promise.reject(new Error('remoteUnreachable')))
    renderPanel()
    await openStationTab()
    const button = await waitFor(() => theButton()!)
    fireEvent.click(button)
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe(EN['settings.remoteStations.failed'])
    expect((button as HTMLButtonElement).disabled).toBe(false)
    opens().mockImplementation(() => Promise.resolve(null))
    fireEvent.click(button)
    await waitFor(() => expect(screen.queryByText(EN['settings.remoteStations.failed'])).toBeNull())
    expect(opens()).toHaveBeenCalledTimes(2)
  })
})

describe('where the window is not offered', () => {
  it('says to use a browser on Linux and macOS, with no button', async () => {
    platform.windows = false
    renderPanel()
    await openStationTab()
    await waitFor(() => expect(screen.getByText(EN['settings.remoteStations.unavailable'])).toBeTruthy())
    expect(card().contains(screen.getByText(EN['settings.remoteStations.unavailable']))).toBe(true)
    expect(theButton()).toBeNull()
  })

  it('is never on the Remote page', async () => {
    renderPanel(true)
    await openStationTab()
    // CONTROL for the absence: the section rendered, with the Remote page's own note.
    await waitFor(() => expect(screen.getByText(EN['remote.configurationLocal'])).toBeTruthy())
    expect(theButton()).toBeNull()
    expect(screen.queryByText(EN['settings.remoteStations.label'])).toBeNull()
    expect(screen.queryByText(EN['settings.remoteStations.unavailable'])).toBeNull()
  })
})
