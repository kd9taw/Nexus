// @vitest-environment jsdom
//
// Settings ▸ Licenses: the license texts of the npm packages built into the desktop app's
// interface and of the Rust crates the app is built from, which the desktop build emits as
// THIRD-PARTY.txt and rust/THIRD-PARTY.txt (ui/vite.config.ts). Pinned here: the button sits in
// the Settings header beside Check for updates on the desktop and is absent from the Remote page's
// copy of Settings (that page links its own file); nothing is read until it is pressed; the dialog
// shows both files as they came and gives the keyboard back to the button when it closes; and a
// build without either file says so in words instead of showing whatever a dev server answered
// with, or half of the texts as if they were all of them.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsLicenses } from './SettingsLicenses'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import { navigationPages } from '../remote-web/__fixtures__/navigation-page'
import configuration from '../remote-web/__fixtures__/configuration-settings.json'

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

/** What fetch() answers: the file itself, or what a dev server sends for a path it lacks. */
function answer(body: string, contentType: string, status = 200) {
  return { ok: status >= 200 && status < 300, status, headers: new Headers({ 'content-type': contentType }), text: async () => body }
}

const TEXT = 'Nexus — third-party notices for the app\'s interface\n\nreact 18.3.1\nMIT License\n'
const RUST = 'Nexus — third-party notices for the app\'s Rust crates\n\nserde 1.0.228 — MIT OR Apache-2.0\n'
const files = async (url: string) => answer(url === 'rust/THIRD-PARTY.txt' ? RUST : TEXT, 'text/plain')
const fetchSpy = vi.fn(files)
const theButton = () => screen.queryByRole('button', { name: EN['settings.licenses.button'] })

beforeEach(() => {
  fetchSpy.mockClear()
  fetchSpy.mockImplementation(files)
  vi.stubGlobal('fetch', fetchSpy)
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
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('in the Settings header', () => {
  it('sits beside Check for updates on the desktop', async () => {
    renderPanel()
    const update = await screen.findByRole('button', { name: EN['settings.panel.update.label'] })
    const header = update.closest('.panel-header')!
    expect(header.contains(theButton())).toBe(true)
    expect(update.nextElementSibling).toBe(theButton())
  })

  it('is not on the Remote page, which links its own license file', async () => {
    renderPanel(true)
    await screen.findByRole('button', { name: EN['settings.panel.update.label'] })
    expect(theButton()).toBeNull()
  })
})

describe('the dialog', () => {
  it('reads nothing until the button is pressed, then shows both files as they came', async () => {
    render(<SettingsLicenses />)
    expect(fetchSpy).not.toHaveBeenCalled()
    fireEvent.click(theButton()!)
    const dialog = await screen.findByRole('dialog', { name: EN['settings.licenses.title'] })
    await waitFor(() => expect(dialog.querySelector('pre')?.textContent).toBe(`${TEXT}\n\n${RUST}`))
    expect(fetchSpy).toHaveBeenCalledTimes(2)
    expect(fetchSpy).toHaveBeenCalledWith('THIRD-PARTY.txt')
    expect(fetchSpy).toHaveBeenCalledWith('rust/THIRD-PARTY.txt')
  })

  it('says the build has no texts when the answer is a page, not the file', async () => {
    fetchSpy.mockImplementation(async () => answer('<!doctype html><title>Nexus</title>', 'text/html'))
    render(<SettingsLicenses />)
    fireEvent.click(theButton()!)
    const dialog = await screen.findByRole('dialog', { name: EN['settings.licenses.title'] })
    await waitFor(() => expect(dialog.textContent).toContain(EN['settings.licenses.unavailable']))
    expect(dialog.querySelector('pre')).toBeNull()
  })

  it('gives the keyboard back to the button when it closes', async () => {
    render(<SettingsLicenses />)
    const opener = theButton()!
    act(() => opener.focus())
    fireEvent.click(opener)
    await screen.findByRole('dialog', { name: EN['settings.licenses.title'] })
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await waitFor(() => expect(document.activeElement).toBe(opener))
  })

  it('says the same when the file is missing', async () => {
    fetchSpy.mockImplementation(async () => answer('', 'text/plain', 404))
    render(<SettingsLicenses />)
    fireEvent.click(theButton()!)
    const dialog = await screen.findByRole('dialog', { name: EN['settings.licenses.title'] })
    await waitFor(() => expect(dialog.textContent).toContain(EN['settings.licenses.unavailable']))
  })

  it('shows neither file when only the interface\'s is there', async () => {
    fetchSpy.mockImplementation(async (url: string) => url === 'rust/THIRD-PARTY.txt' ? answer('', 'text/plain', 404) : answer(TEXT, 'text/plain'))
    render(<SettingsLicenses />)
    fireEvent.click(theButton()!)
    const dialog = await screen.findByRole('dialog', { name: EN['settings.licenses.title'] })
    await waitFor(() => expect(dialog.textContent).toContain(EN['settings.licenses.unavailable']))
    expect(dialog.querySelector('pre')).toBeNull()
  })
})
