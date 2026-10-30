// @vitest-environment jsdom
//
// THE DECODERS A SCREEN STARTED STOP WHEN THE OPERATOR GOES TO ANOTHER SECTION. Opening RTTY, PSK,
// JTTY, SSTV or APRS starts its decoder; nothing stopped them, so a look at each and a return to FT8
// left them all running on the receive audio for the session. In the real App, by the operator's
// own clicks: entering another section tells the engine the screen was left
// (`leave_decoder_screen`); a glance at another screen does not.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, fireEvent, render, waitFor } from '@testing-library/react'

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
import defaultSettings from './components/__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 30_000 })

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, sstv: true, aprs: true, connect: true }

async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await act(async () => {})
}

/** ModeNav's button for a section, by its visible label. */
async function navTo(label: string): Promise<void> {
  const btn = [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')].find(
    (b) => b.querySelector('.mode-label')?.textContent === label,
  )
  expect(btn, `no navigation button labelled ${label}`).toBeTruthy()
  fireEvent.click(btn!)
  await act(async () => {})
}

const left = () => vi.mocked(api.leaveDecoderScreen).mock.calls.map(([s]) => s)

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  localStorage.setItem('nexus.connect.autopop', 'off')
  localStorage.setItem('nexus.needed.autopop', 'off')
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  vi.mocked(api.getSettings).mockImplementation(
    async () => ({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' }) as never,
  )
  // The FT button re-asserts the tier and APRS parks the radio on its channel; each answers with
  // the station, as the backend does.
  for (const verb of [api.setTier, api.aprsTune, api.tuneChannel, api.setFrequency]) {
    vi.mocked(verb).mockResolvedValue(APP_SNAPSHOT as never)
  }
  vi.mocked(api.leaveDecoderScreen).mockReset()
  vi.mocked(api.leaveDecoderScreen).mockResolvedValue(false)
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})

describe('leaving a decoder screen', () => {
  it('the tour: each decoder screen is left when the next section opens, and FT8 leaves the last', async () => {
    await mountOn('operate')
    for (const label of ['RTTY', 'PSK', 'SSTV', 'APRS']) await navTo(label)
    expect(left(), 'each screen left by the next').toEqual(['rtty', 'psk', 'sstv'])
    await navTo('FT')
    expect(left(), 'back on FT8, APRS is left too').toEqual(['rtty', 'psk', 'sstv', 'aprs'])
  })

  it('a glance at another screen leaves nothing; the next section does', async () => {
    await mountOn('rtty')
    await navTo('Conditions')
    await waitFor(() => expect(document.title).toContain('Conditions'))
    expect(left(), 'a glance is not a section change').toEqual([])
    await navTo('Phone')
    expect(left()).toEqual(['rtty'])
  })

  it('a screen the engine keeps is asked about again at the next section', async () => {
    vi.mocked(api.leaveDecoderScreen).mockResolvedValueOnce(true)
    await mountOn('sstv')
    await navTo('Phone')
    await waitFor(() => expect(left()).toEqual(['sstv']))
    await act(async () => {})
    await navTo('CW')
    expect(left(), 'kept once, asked again').toEqual(['sstv', 'sstv'])
  })
})
