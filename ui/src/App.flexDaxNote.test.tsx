// @vitest-environment jsdom
//
// THE FLEX DAX NOTE IN THE REAL APP: App reads the active radio from Settings and the Phone cockpit
// hands the voice keyer whether a Flex on SmartSDR CAT or Hamlib is the radio. The keyer's own test
// (components/VoiceKeyer.flexDaxNote.test.tsx) covers once-only and dismissal; this one covers the
// wiring, which no component test can see.
import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest'
import { act, cleanup, render, waitFor, fireEvent } from '@testing-library/react'

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  return { ...auto, ...appApiAnswers() }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import * as api from './api'
import App from './App'
import { APP_SNAPSHOT } from './appCockpits.testkit'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
import type { AppSnapshot, Settings, VoiceMessage } from './types'

// The real App mounts per case; under the full suite's load that outruns vitest's default 5 s.
vi.setConfig({ testTimeout: 30_000 })
const WAIT = { timeout: 10_000 }

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, sstv: true, aprs: true, js8: true, connect: true }
const TX_ON = { ...APP_SNAPSHOT, radio: { ...APP_SNAPSHOT.radio, txEnabled: true } } as unknown as AppSnapshot
/** Every slot recorded, so any F-key that reached the keyer would play. */
const RECORDED: VoiceMessage[] = [1, 2, 3, 4, 5, 6, 8].map((slot) => ({ slot, label: `M${slot}`, file: `/voice/slot${slot}.wav` }))

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
  // Settings as a fresh install has them; each case puts its radio on top.
  vi.mocked(api.getSettings).mockResolvedValue(settingsFixture as unknown as Settings)
  vi.mocked(api.getVoiceMessages).mockResolvedValue(RECORDED)
  // TX on, so a key that reached the keyer would play rather than be refused. Every command that
  // answers with a snapshot answers with this one, or the first section switch turns TX off again.
  for (const answer of [api.getSnapshot, api.setOperatingMode, api.setArea, api.selectPeer]) {
    vi.mocked(answer as Mock).mockImplementation(async () => TX_ON)
  }
  vi.mocked(api.subscribeSnapshot).mockImplementation(() => () => {})
  vi.mocked(api.playVoiceMessage).mockClear()
})

async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), WAIT)
  await act(async () => {})
}
const played = () => vi.mocked(api.playVoiceMessage).mock.calls.map((c) => c[0])

/** Mount the Phone screen with `over` on the station's settings and press F1 once. */
async function playF1With(over: Partial<Settings>): Promise<Element | null> {
  vi.mocked(api.getSettings).mockResolvedValue({ ...(settingsFixture as unknown as Settings), ...over })
  await mountOn('phone')
  await waitFor(() => expect(document.querySelectorAll('.vk-slot')).toHaveLength(RECORDED.length), WAIT)
  await act(async () => {
    fireEvent.keyDown(document.body, { key: 'F1' })
  })
  expect(played(), 'F1 played slot 1').toEqual([1])
  return document.querySelector('.vk-flex-dax-note')
}

describe('the Flex DAX note in the app', () => {
  it('shows on the first message for a Flex on SmartSDR CAT', async () => {
    const note = await playF1With({ rigModel: 2036, rigConn: 'network', rigAddr: '127.0.0.1:5002' })
    expect(note).not.toBeNull()
  })

  it('does not show for Nexus’s own Flex client, or for another radio', async () => {
    expect(
      await playF1With({ rigModel: 2036, rigConn: 'network', flexNativeCat: true, flexRadioIp: '192.168.1.50' }),
      'the native Flex client switches to DAX itself',
    ).toBeNull()
    cleanup()
    vi.mocked(api.playVoiceMessage).mockClear()
    expect(await playF1With({ rigModel: 3073, rigConn: 'serial' }), 'an IC-7300').toBeNull()
  })
})
