// @vitest-environment jsdom
//
// THE CW PITCH WENT BACK TO THE OLD ONE. A user set the CW cockpit's pitch to 650 and soon after saw
// 700, the pitch they had before. The engine kept 650 (only `set_cw_keyer` writes the pitch), but the
// box is seeded from App's copy of the settings, which App reads at start-up and after a Settings
// save, and which the cockpit's own pitch change never touched. App unmounts the CW cockpit on a page
// switch, so the next time it was drawn it showed — and its scope marker sat on — the old pitch.
// Here, in the real App and by the operator's own path: set 650, go to Conditions and back.
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
import { COCKPIT_MAIN } from './appCockpits.testkit'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

// Each case mounts the real App and navigates it twice; under the full suite's load that outruns
// vitest's default 5 s per test, which is a budget, not a claim about the app.
vi.setConfig({ testTimeout: 30_000 })

let enginePitch = 700

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, js8: true, connect: true }
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

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  localStorage.setItem('nexus.connect.autopop', 'off')
  localStorage.setItem('nexus.needed.autopop', 'off')
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  // A stand-in engine: `set_cw_keyer` stores the pitch, and `get_settings` reads it back, so
  // anything that re-reads the settings sees the pitch the engine really holds.
  enginePitch = 700
  vi.mocked(api.getSettings).mockImplementation(
    async () => ({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', cwPitchHz: enginePitch }) as never,
  )
  vi.mocked(api.setCwKeyer).mockImplementation(async (_backend, pitch = 0) => {
    if (pitch > 0) enginePitch = pitch
    return null as never
  })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})

const pitchBox = () =>
  document.querySelector<HTMLInputElement>(`${COCKPIT_MAIN.cw} input.cw-pitch`)

describe('CW pitch', () => {
  it('a pitch set in the cockpit is still the pitch after Conditions and back', async () => {
    await mountOn('cw')
    expect(document.querySelector('.view-crash')).toBeNull()
    await waitFor(() => expect(pitchBox()?.value).toBe('700'))
    const box = pitchBox()!
    fireEvent.change(box, { target: { value: '650' } })
    fireEvent.blur(box)
    await act(async () => {})
    expect(enginePitch).toBe(650)

    await navTo('Conditions')
    await waitFor(() => expect(document.title).toContain('Conditions'))
    expect(document.querySelector(COCKPIT_MAIN.cw), 'premise: CW unmounted on the page switch').toBeNull()
    await navTo('CW')
    await waitFor(() => expect(pitchBox()).not.toBeNull())
    expect(pitchBox()!.value, 'the CW cockpit came back on the old pitch').toBe('650')
  })
})
