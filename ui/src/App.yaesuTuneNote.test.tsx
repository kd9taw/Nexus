// @vitest-environment jsdom
//
// #441: THE TUNE NOTE IN PHONE, ON A YAESU WITH BUILT-IN USB AUDIO, IN THE REAL APP. Phone keys plain
// USB/LSB, so these radios take their transmit audio from the SSB MOD SOURCE menu (MIC from the factory:
// the front jack only), and Tune's tone over USB audio then goes out as no RF. App reads the radio from
// Settings; Phone watches a Tune's measured power and, once a Tune reads about none for 2 s, shows a line
// beside Tune for the session (dismissible), or an ⓘ that holds it where the radio reports no power. Text
// only: Nexus never changes a radio menu, and Tune keys exactly as before. Harness: App.flexDaxNote's.
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
import { resetTuneNote } from './features/yaesuTuneNote'

let push: ((s: AppSnapshot) => void) | null = null

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
  vi.mocked(api.subscribeSnapshot).mockImplementation((next: (s: AppSnapshot) => void) => {
    push = next
    return () => {}
  })
  localStorage.removeItem('nexus.phone.yaesuTuneNote')
  resetTuneNote()
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
/** The note's line, where it must be: in this screen's TX strip, the one that holds its Tune. */
function noteIn(view: string): Element | null {
  const strip = tuneIn(view)?.closest('.cockpit-txstrip')
  return [...(strip?.querySelectorAll('[role="note"]') ?? [])].find((n) => n.textContent?.startsWith(NOTE)) ?? null
}
const infoIn = (view: string) => tuneIn(view)?.closest('.cockpit-txstrip')?.querySelector('button.tune-note-info') ?? null
/** Mount `view` on the radio `over` and let the settings land. */
async function open(view: string, over: Partial<Settings>): Promise<void> {
  await mountOn(view, over)
  await waitFor(() => expect(vi.mocked(api.getSettings)).toHaveBeenCalled(), WAIT)
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
  await waitFor(() => expect(tuneIn(view), `no Tune on the ${view} screen`).toBeTruthy(), WAIT)
}
/** The station reports a Tune keyed with `txPoW` watts (undefined: no reading), for `ms`. */
async function tuneFor(ms: number, txPoW?: number) {
  await waitFor(() => expect(push).not.toBeNull(), WAIT)
  const radio = { ...TX_ON.radio, tuning: true, txBusyReason: 'tune', ...(txPoW === undefined ? {} : { txPoW }) }
  await act(async () => push!({ ...TX_ON, radio } as AppSnapshot))
  await act(async () => {
    await new Promise((r) => setTimeout(r, ms))
  })
}

describe('the Tune note on a Yaesu with USB audio (#441)', () => {
  it('is not there before a Tune; a Tune that reads no power for 2 s brings the line, and Dismiss takes it away', async () => {
    await open('phone', { rigModel: 1042, rigConn: 'serial' })
    expect(noteIn('phone'), 'a permanent line').toBeNull()
    expect(infoIn('phone')).toBeNull()
    await tuneFor(2600, 0)
    await waitFor(() => expect(noteIn('phone'), 'no note beside Tune after a Tune with no power').not.toBeNull(), WAIT)
    const dismiss = [...noteIn('phone')!.querySelectorAll('button')].find((b) => b.textContent === EN['common.dismiss'])!
    await act(async () => dismiss.click())
    expect(noteIn('phone')).toBeNull()
    expect(localStorage.getItem('nexus.phone.yaesuTuneNote')).toBe('1')
  })

  it('a Tune that reads power brings nothing', async () => {
    await open('phone', { rigModel: 1042, rigConn: 'serial' })
    await tuneFor(2600, 40)
    expect(noteIn('phone')).toBeNull()
    expect(infoIn('phone')).toBeNull()
  })

  it('with no power reading, an ⓘ beside Tune holds the note', async () => {
    await open('phone', { rigModel: 1035, rigConn: 'serial' })
    await tuneFor(2600)
    const info = await waitFor(() => {
      const b = infoIn('phone')
      expect(b, 'no ⓘ beside Tune').not.toBeNull()
      return b as HTMLButtonElement
    }, WAIT)
    expect(noteIn('phone'), 'the line before the ⓘ was pressed').toBeNull()
    expect(info.getAttribute('title')).toBe(NOTE)
    await act(async () => info.click())
    expect(noteIn('phone')).not.toBeNull()
    expect(info.getAttribute('aria-expanded')).toBe('true')
  })

  it('never for another radio, a TCI program, or outside Phone', async () => {
    for (const [view, over, why] of [
      ['phone', { rigModel: 3073, rigConn: 'serial' }, 'an IC-7300'],
      ['phone', { rigModel: 1036, rigConn: 'serial' }, 'the FT-891 (USB for CAT only)'],
      ['phone', { rigModel: 1042, rigConn: 'tci' }, 'TCI'],
      ['cw', { rigModel: 1042, rigConn: 'serial' }, 'the CW screen'],
    ] as const) {
      resetTuneNote()
      await open(view, over)
      await tuneFor(2600, 0)
      expect(noteIn(view), why).toBeNull()
      expect(infoIn(view), why).toBeNull()
      cleanup()
    }
  })
})
