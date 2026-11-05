// @vitest-environment jsdom
//
// "Accept requests from other computers" in Settings ▸ Services ▸ Integrations & Feeds: beside the
// WSJT-X UDP address, off as it ships, usable only with the UDP API on, carried by a Save, and its
// hint says plainly what it lets in. Search and the section's deep link reach it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { Settings } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'
import { searchSettings } from '../settings/registry'

// The house budget: a loaded full suite has run cases far slower than one core.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module.
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

/** What the station holds: the shipped settings (the CAT broker on 4532, the TCI server off on
 *  40001) with a callsign, and whatever a case puts over them. */
let stored: Partial<Settings> = {}

beforeEach(() => {
  stored = {}
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.19.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', ...stored } as never),
  )
  // The panel scrolls to the section it opens at; jsdom has no layout to scroll.
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(cleanup)

async function openFeeds(over: Partial<Settings> = {}): Promise<HTMLElement> {
  stored = over
  render(
    <SettingsPanel
      target="integrations-feeds"
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
  return screen.findByRole('switch', { name: EN['settings.integrations.wsjtxUdpAccept.label'] })
}

const saved = () => api.get('setSettings').mock.lastCall?.[0] as Settings | undefined

describe('Accept requests from other computers', () => {
  it('sits in the section with the UDP address, off, and locked until the UDP API is on', async () => {
    const accept = await openFeeds()
    expect(accept.getAttribute('aria-checked')).toBe('false')
    expect((accept as HTMLButtonElement).disabled).toBe(true)
    expect(document.querySelector('#settings-integrations-feeds')?.contains(accept)).toBe(true)
    expect(document.querySelector('[data-setting="wsjtxUdpAcceptOtherComputers"]')).toBe(accept)
  })

  it('is saved with the form, and its hint says what it lets in and that a sender can be faked', async () => {
    const accept = await openFeeds({ wsjtxUdp: true })
    expect((accept as HTMLButtonElement).disabled).toBe(false)
    const hint = EN['settings.integrations.wsjtxUdpAccept.hint']
    expect(screen.getByText(hint)).toBeTruthy()
    expect(hint).toMatch(/can be faked/)
    expect(hint).toMatch(/turn TX on/)
    fireEvent.click(accept)
    expect(accept.getAttribute('aria-checked')).toBe('true')
    const save = (await screen.findAllByRole('button', { name: EN['settings.panel.save'] })).find(
      (b) => (b as HTMLButtonElement).type === 'submit',
    )!
    fireEvent.click(save)
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(saved()?.wsjtxUdpAcceptOtherComputers).toBe(true)
  })

  it('is found by search', () => {
    for (const query of ['other computers', 'accept requests', 'another computer']) {
      expect(searchSettings(query).map((h) => h.section.id)).toContain('integrations-feeds')
    }
  })
})
