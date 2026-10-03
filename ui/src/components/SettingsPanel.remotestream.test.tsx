// @vitest-environment jsdom
//
// Remote as a stream's switch in Settings ▸ Station ▸ Remote access — a SIGNED-OFF transmit-path
// feature (operator, 2026-09-27, "Session permit"), OFF unless the operator turns it on. What is
// pinned here is what the operator sees and what Save writes: the switch ships off, it writes
// `remoteStream`, a station that cannot capture its window is told so instead of being offered a
// switch that would do nothing, and the Remote page never shows it (no browser may turn it on).
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

function station(extra: Record<string, unknown> = {}) {
  return { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', ...extra } as never
}

function renderPanel(remote = false) {
  // The Remote page is the panel inside a RemoteCollectionsContext (useNavigation's `remote`),
  // reading the station's Settings document instead of the local settings.
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
  // The fieldset the switch lives in, so an absent switch is not an absent tab.
  await waitFor(() => expect(document.getElementById('settings-remote-access')).toBeTruthy())
}

const theSwitch = () => screen.queryByRole('switch', { name: EN['settings.remoteStream.label'] })
// Only what the capture code shows: a minimized (or hidden) window gives no picture, and closing
// Nexus ends the capture. What a locked session or a missing display does is for the bench.
const DISPLAY_NOTE =
  'The stream is this window as Windows draws it on this computer, so keep Nexus open, and not minimized, while a browser streams it.'

function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}

const lastSaved = () => {
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0] as Record<string, unknown>
}

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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.15.0'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(station()))
})
afterEach(cleanup)

describe('on a Windows station', () => {
  it('offers the switch in Remote access, OFF, and says what it sends and what stops', async () => {
    renderPanel()
    await openStationTab()
    const sw = await waitFor(() => {
      const s = theSwitch()
      expect(s, 'the switch is not in Remote access').toBeTruthy()
      return s!
    })
    expect(sw.getAttribute('aria-checked')).toBe('false')
    expect(document.getElementById('settings-remote-access')!.contains(sw)).toBe(true)
    expect(screen.getByText(EN['settings.remoteStream.hint']).textContent).toMatch(/can operate it as you would here, transmit included/)
  })

  it('says beside the switch that the stream is this window as Windows draws it, so Nexus stays open and not minimized', async () => {
    renderPanel()
    await openStationTab()
    const sw = await waitFor(() => theSwitch()!)
    const note = screen.getByText(DISPLAY_NOTE)
    expect(note.closest('.settings-field')?.contains(sw) ?? false, 'the note is not beside the switch').toBe(true)
  })

  it('writes remoteStream: true on Save once it is turned on', async () => {
    renderPanel()
    await openStationTab()
    const sw = await waitFor(() => theSwitch()!)
    fireEvent.click(sw)
    expect(sw.getAttribute('aria-checked')).toBe('true')
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(lastSaved().remoteStream).toBe(true)
  })

  it('reads a station that sends no remoteStream key as OFF', async () => {
    const settings = station() as Record<string, unknown>
    delete settings.remoteStream
    api.get('getSettings').mockImplementation(() => Promise.resolve(settings))
    renderPanel()
    await openStationTab()
    await waitFor(() => expect(theSwitch()?.getAttribute('aria-checked')).toBe('false'))
  })

  it('shows a saved ON as on', async () => {
    api.get('getSettings').mockImplementation(() => Promise.resolve(station({ remoteStream: true })))
    renderPanel()
    await openStationTab()
    await waitFor(() => expect(theSwitch()?.getAttribute('aria-checked')).toBe('true'))
  })
})

// Remote streaming is a beta (the operator, 2026-10-02: "we need a clear warning that this is a beta
// feature in nexus and access could be revoked at any time"): a visible mark and one plain line, in the
// Remote access card beside the streaming switch, never behind a click.
describe('the beta line', () => {
  const BETA = `${EN['remote.beta.mark']} ${EN['remote.beta.notice']}`
  const line = () => document.getElementById('settings-remote-access')?.querySelector('.remote-beta') ?? null
  it('is in the Remote access card, in words, just before the streaming switch', async () => {
    renderPanel()
    await openStationTab()
    const sw = await waitFor(() => theSwitch()!)
    expect(line()?.textContent).toBe(BETA)
    expect(line()!.compareDocumentPosition(sw) & Node.DOCUMENT_POSITION_FOLLOWING, 'the line comes before the switch').toBeTruthy()
    // Nothing between them but the switch's own row: it is beside the switch, not elsewhere in the card.
    expect(line()!.nextElementSibling?.contains(sw)).toBe(true)
  })
  it('is there on a station that cannot stream yet, too', async () => {
    platform.windows = false
    renderPanel()
    await openStationTab()
    await waitFor(() => expect(screen.getByText(EN['settings.remoteStream.unavailable'])).toBeTruthy())
    expect(line()?.textContent).toBe(BETA)
  })
  it('is not on the Remote page\'s copy of Settings, which streams nothing (the site says it on each station)', async () => {
    renderPanel(true)
    await openStationTab()
    await waitFor(() => expect(screen.getByText(EN['remote.configurationLocal'])).toBeTruthy())
    expect(screen.getByText(EN['remote.configurationLocal']).textContent).toMatch(/in Nexus at the station, in person or streamed to this browser/)
    expect(line()).toBeNull()
  })
})

describe('where the switch is not offered', () => {
  it('explains on a station that cannot capture its window, instead of a dead switch', async () => {
    platform.windows = false
    renderPanel()
    await openStationTab()
    await waitFor(() =>
      expect(screen.getByText(EN['settings.remoteStream.unavailable'])).toBeTruthy(),
    )
    expect(theSwitch()).toBeNull()
    expect(screen.queryByText(DISPLAY_NOTE), 'a note about a stream this station cannot send').toBeNull()
  })

  it('is never on the Remote page: no browser may turn streaming on', async () => {
    renderPanel(true)
    await openStationTab()
    // CONTROL for the absence: the section itself rendered, with the Remote page's own note.
    await waitFor(() => expect(screen.getByText(EN['remote.configurationLocal'])).toBeTruthy())
    expect(theSwitch()).toBeNull()
    expect(screen.queryByText(EN['settings.remoteStream.unavailable'])).toBeNull()
    expect(screen.queryByText(DISPLAY_NOTE)).toBeNull()
  })
})
