// @vitest-environment jsdom
//
// #441: THE TUNE NOTE IN PHONE, ON A NEWCAT YAESU, IN THE REAL APP. Phone keys plain USB/LSB, so these
// radios take their transmit audio from the SSB modulation source menu (MIC from the factory: the front
// jack only), and Tune's tone over USB audio then goes out as no RF. App reads the radio from Settings;
// Phone hands the TX strip a line beside Tune saying where the radio's menu is. Text only: Nexus never
// changes a radio menu, and Tune keys exactly as before. Harness: App.flexDaxNote.test.tsx's.
import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest'
import { act, cleanup, render, waitFor } from '@testing-library/react'

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
import { EN } from './i18n/en'

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

async function mountOn(view: string, over: Partial<Settings>): Promise<void> {
  vi.mocked(api.getSettings).mockResolvedValue({ ...(settingsFixture as unknown as Settings), ...over })
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), WAIT)
  await act(async () => {})
}
const NOTE = EN['phone.tune.usbAudioNote']
/** The cockpit each screen mounts (the app keeps the others mounted, hidden). */
const COCKPIT: Record<string, string> = { phone: 'main.phone-cockpit', cw: 'main.cw-cockpit' }
const tuneIn = (view: string) =>
  [...document.querySelectorAll(`${COCKPIT[view]} .cockpit-txstrip button`)].find((b) => /^tune$/i.test(b.textContent?.trim() ?? ''))
/** The note, where it must be: in this screen's TX strip, the one that holds its Tune. */
function noteIn(view: string): Element | null {
  const strip = tuneIn(view)?.closest('.cockpit-txstrip')
  return [...(strip?.querySelectorAll('[role="note"]') ?? [])].find((n) => n.textContent === NOTE) ?? null
}
/** Mount `view` on the radio `over`, let the settings land, and read the note in its strip. */
async function noteOn(view: string, over: Partial<Settings>): Promise<Element | null> {
  await mountOn(view, over)
  await waitFor(() => expect(vi.mocked(api.getSettings)).toHaveBeenCalled(), WAIT)
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
  await waitFor(() => expect(tuneIn(view), `no Tune on the ${view} screen`).toBeTruthy(), WAIT)
  return noteIn(view)
}

describe('the Tune note on a newcat Yaesu (#441)', () => {
  it('shows beside Tune in Phone for an FTDX10 on serial CAT', async () => {
    await noteOn('phone', { rigModel: 1042, rigConn: 'serial' })
    await waitFor(() => expect(noteIn('phone'), 'no note beside Tune').not.toBeNull(), WAIT)
  })

  it('does not show for another radio, a TCI program, or outside Phone', async () => {
    expect(await noteOn('phone', { rigModel: 3073, rigConn: 'serial' }), 'an IC-7300').toBeNull()
    cleanup()
    expect(await noteOn('phone', { rigModel: 1051, rigConn: 'serial' }), 'the FTX-1, not newcat').toBeNull()
    cleanup()
    expect(await noteOn('phone', { rigModel: 1042, rigConn: 'tci' }), 'TCI').toBeNull()
    cleanup()
    expect(await noteOn('cw', { rigModel: 1042, rigConn: 'serial' }), 'the CW screen').toBeNull()
  })
})
