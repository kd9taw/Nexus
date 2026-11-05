// @vitest-environment jsdom
//
// A MEMORY REOPENS ON THE MODE SCREEN IT WAS SAVED FROM, IN THE REAL APP. 144.8875 FM saved for
// SSTV recalled into Phone. Here by the operator's own clicks: go to the screen, save, go elsewhere,
// press Tune. The pure rules are in features/memories.screen.test.ts; this is the wiring, and above
// all that nothing behind the recall moves the operator back off the screen it opened.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, cleanup, fireEvent, render, waitFor } from '@testing-library/react'

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
import { pushToast } from './toast'
import { t } from './i18n'
import App from './App'
import { APP_SNAPSHOT } from './appCockpits.testkit'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import { emptyBank, memoriesStore, addMemory, type Memory } from './features/memories'
import { MEMORY_OPEN_SCREEN_KEY } from './features/memoryRecall'

vi.setConfig({ testTimeout: 30_000 })

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, sstv: true, aprs: true, js8: true, jtty: true, memories: true, connect: true }

// Every api answer in the test kit is this one object, so the station is changed in it.
const RADIO_AT_REST = { ...APP_SNAPSHOT.radio }
const stationRadio = (over: Record<string, unknown>) => Object.assign(APP_SNAPSHOT.radio, over)
/** The reported channel, held in FM with the data submode the screen can hold. */
const ON_CHANNEL = { dialMhz: 144.8875, band: '2m', sideband: 'USB', rigMode: 'PKTFM' }

function features(enabled: Record<string, boolean>): void {
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled }))
}

async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await act(async () => {})
}

async function settle(): Promise<void> {
  await act(async () => {
    for (let i = 0; i < 20; i++) await new Promise((r) => setTimeout(r, 0))
  })
}

async function navTo(label: string): Promise<void> {
  const btn = [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')].find(
    (b) => b.querySelector('.mode-label')?.textContent === label,
  )
  expect(btn, `no navigation button labelled ${label}`).toBeTruthy()
  fireEvent.click(btn!)
  await settle()
}

const saved = (over: Partial<Memory> & { rxMhz: number; mode: string }): Memory => {
  memoriesStore.update((b) => addMemory(b, over))
  return memoriesStore.get().memories[memoriesStore.get().memories.length - 1]
}

/** The Memories section's Save button, wherever the toolbar has it. */
async function pressSave(): Promise<void> {
  const btn = [...document.querySelectorAll<HTMLButtonElement>('.mv-tool')].find((b) => b.textContent?.includes('MHz') || b.textContent?.includes('Save'))
  expect(btn, 'no Save button in the Memories toolbar').toBeTruthy()
  fireEvent.click(btn!)
  await settle()
}

/** Tune on the only row. */
async function pressTune(): Promise<void> {
  const btn = document.querySelector<HTMLButtonElement>('.mv-row-tune')
  expect(btn, 'no Tune button on the row').toBeTruthy()
  fireEvent.click(btn!)
  await settle()
}

beforeEach(() => {
  cleanup()
  localStorage.clear()
  features(SECTIONS_ON)
  localStorage.setItem('nexus.connect.autopop', 'off')
  localStorage.setItem('nexus.needed.autopop', 'off')
  memoriesStore.set(emptyBank())
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  vi.mocked(api.getSettings).mockImplementation(async () => ({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' }) as never)
  // Every verb answers with the station, as the backend does.
  for (const k of Object.keys(APP_SNAPSHOT.radio)) delete (APP_SNAPSHOT.radio as Record<string, unknown>)[k]
  stationRadio(RADIO_AT_REST)
  for (const verb of [api.setTier, api.setFrequency, api.tuneChannel, api.setOperatingMode]) {
    vi.mocked(verb).mockResolvedValue(APP_SNAPSHOT as never)
  }
  for (const verb of [api.workSpot, api.sstvTune, api.aprsTune, api.setSettings, api.setSidebandOverride]) {
    vi.mocked(verb).mockReset()
    vi.mocked(verb).mockResolvedValue(APP_SNAPSHOT as never)
  }
  vi.mocked(api.leaveDecoderScreen).mockResolvedValue(false)
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})

describe('saving from the Memories section names the mode screen the operator came from', () => {
  it('the report: SSTV, on a channel held in FM with the data submode, saves FM on SSTV and reopens there', async () => {
    stationRadio(ON_CHANNEL)
    await mountOn('operate')
    await navTo('SSTV')
    await navTo('Memories')
    await pressSave()
    const [row] = memoriesStore.get().memories
    expect(row).toMatchObject({ rxMhz: 144.8875, mode: 'FM', screen: 'sstv' })

    await navTo('FT')
    await navTo('Memories')
    vi.mocked(api.workSpot).mockClear()
    vi.mocked(api.setOperatingMode).mockClear()
    await pressTune()

    expect(api.sstvTune).toHaveBeenCalledExactlyOnceWith(144.8875, '2m', 'FM')
    expect(api.workSpot, 'work_spot’s nav hint would pull the window to Phone').not.toHaveBeenCalled()
    expect(document.title).toContain('SSTV')
  })

  it('a launch straight into Memories has no screen to name, and the save recalls by its mode', async () => {
    stationRadio({ ...ON_CHANNEL, rigMode: 'FM' })
    await mountOn('memories')
    await pressSave()
    expect(memoriesStore.get().memories[0]).not.toHaveProperty('screen')
  })

  it('RTTY: a glance at the logbook on the way keeps the screen', async () => {
    await mountOn('operate')
    await navTo('RTTY')
    await navTo('Logbook')
    await navTo('Memories')
    await pressSave()
    expect(memoriesStore.get().memories[0]).toMatchObject({ mode: 'RTTY', screen: 'rtty' })
  })
})

describe('the strip’s ＋ names the cockpit that hosts it', () => {
  it.each([
    ['phone', 'phone'],
    ['cw', 'cw'],
  ])('%s', async (view, screen) => {
    await mountOn(view)
    const plus = document.querySelector<HTMLButtonElement>('.mem-strip-save')
    expect(plus, 'no strip on this cockpit').toBeTruthy()
    fireEvent.click(plus!)
    await settle()
    expect(memoriesStore.get().memories[0].screen).toBe(screen)
  })
})

describe('recall', () => {
  it('a memory with no screen tunes and opens exactly as it always did', async () => {
    saved({ rxMhz: 146.52, mode: 'FM' })
    await mountOn('memories')
    await pressTune()
    expect(api.workSpot).toHaveBeenCalledExactlyOnceWith('phone', 146.52, '2m', undefined, undefined)
    expect(api.sstvTune).not.toHaveBeenCalled()
    expect(document.title).toContain('Phone')
  })

  it('a recorded screen that is switched off falls back to where the memory would go without one', async () => {
    features({ ...SECTIONS_ON, sstv: false })
    saved({ rxMhz: 146.52, mode: 'FM', screen: 'sstv' })
    await mountOn('memories')
    await pressTune()
    expect(api.workSpot).toHaveBeenCalledExactlyOnceWith('phone', 146.52, '2m', undefined, undefined)
    expect(api.sstvTune).not.toHaveBeenCalled()
    expect(document.title).toContain('Phone')
  })

  it('switched off with its fallback off too, it says so and tunes nothing', async () => {
    features({ ...SECTIONS_ON, sstv: false, phone: false })
    saved({ rxMhz: 146.52, mode: 'FM', screen: 'sstv' })
    await mountOn('memories')
    await pressTune()
    expect(api.workSpot).not.toHaveBeenCalled()
    expect(api.sstvTune).not.toHaveBeenCalled()
  })

  it.each([
    ['rtty', 'RTTY', 14.08, 'rtty', undefined, 'RTTY'],
    ['psk', 'PSK31', 14.07, 'keyboard', undefined, 'PSK'],
    ['jtty', 'JTTY', 14.09, 'jtty', undefined, 'JTTY'],
    ['js8', 'JS8', 14.078, 'js8', 'JS8', 'JS8'],
  ] as const)('%s opens on its own section’s work_spot', async (screen, mode, rxMhz, section, tier, title) => {
    saved({ rxMhz, mode, screen })
    await mountOn('memories')
    vi.mocked(api.setOperatingMode).mockClear()
    await pressTune()
    expect(api.workSpot).toHaveBeenCalledExactlyOnceWith(section, rxMhz, '20m', undefined, tier)
    expect(document.title).toContain(title)
    // The dial was recalled exactly: entering the screen must not re-home it.
    for (const [, followFreq] of vi.mocked(api.setOperatingMode).mock.calls) expect(followFreq).toBe(false)
  })

  it('an SSTV sideband memory takes its sideband as Phone’s does, and leaves the phone mode alone', async () => {
    saved({ rxMhz: 3.845, mode: 'USB', screen: 'sstv' })
    await mountOn('memories')
    await pressTune()
    expect(api.sstvTune).toHaveBeenCalledExactlyOnceWith(3.845, '80m', 'USB')
    expect(api.setSidebandOverride).toHaveBeenCalledExactlyOnceWith('USB')
    expect(api.setSettings).not.toHaveBeenCalled()
  })

  it('an SSTV FM machine writes its shift and tone before the tune, and no phone mode', async () => {
    saved({ rxMhz: 145.5, mode: 'FM', offsetDir: 'minus', offsetMhz: 0.6, toneMode: 'tone', ctcssEncHz: 100, screen: 'sstv' })
    await mountOn('memories')
    await pressTune()
    expect(api.setSettings).toHaveBeenCalledTimes(1)
    const written = vi.mocked(api.setSettings).mock.calls[0][0] as unknown as Record<string, unknown>
    expect(written).toMatchObject({ rptrShift: 'minus', rptrOffsetOverrideHz: 600000, ctcssToneHz: 100 })
    expect(written.phoneMode).toBe(defaultSettings.phoneMode)
    expect(vi.mocked(api.setSettings).mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(api.sstvTune).mock.invocationCallOrder[0])
  })

  it('APRS tunes with its own verb and opens APRS', async () => {
    saved({ rxMhz: 144.39, mode: 'FM', screen: 'aprs' })
    await mountOn('memories')
    await pressTune()
    expect(api.aprsTune).toHaveBeenCalledWith(144.39)
    expect(api.workSpot).not.toHaveBeenCalled()
    expect(api.setSettings).not.toHaveBeenCalled()
    expect(document.title).toContain('APRS')
  })

  it('during a transmission it adds no gate and no second verb: the engine’s tune path answers, as today', async () => {
    stationRadio({ transmitting: true, txEnabled: true })
    saved({ rxMhz: 144.8875, mode: 'FM', screen: 'sstv' })
    saved({ rxMhz: 146.52, mode: 'FM' })
    await mountOn('memories')
    const tunes = document.querySelectorAll<HTMLButtonElement>('.mv-row-tune')
    fireEvent.click(tunes[0])
    await settle()
    expect(api.sstvTune).toHaveBeenCalledTimes(1)
    expect(api.workSpot).not.toHaveBeenCalled()
    expect(api.setTxEnabled, 'a recall never touches the TX latch').not.toHaveBeenCalled()
    await navTo('Memories')
    fireEvent.click(document.querySelectorAll<HTMLButtonElement>('.mv-row-tune')[1])
    await settle()
    expect(api.workSpot).toHaveBeenCalledTimes(1)
    expect(api.setTxEnabled).not.toHaveBeenCalled()
  })

  it('SSTV owns no rig mode: the Phone tab afterwards still asks to keep the dial, as after a pick from its strip', async () => {
    saved({ rxMhz: 144.8875, mode: 'FM', screen: 'sstv' })
    await mountOn('operate')
    await navTo('Memories')
    await pressTune()
    expect(document.title).toContain('SSTV')
    vi.mocked(api.setOperatingMode).mockClear()
    await navTo('Phone')
    expect(api.setOperatingMode).toHaveBeenCalledExactlyOnceWith('phone', true, true)
  })

  it.each([
    ['sstv', 'AM', 'shell.recall.done.setMode'],
    ['sstv', 'NFM', 'shell.recall.done'],
    ['sstv', 'FM', 'shell.recall.done'],
    ['phone', 'AM', 'shell.recall.done.setMode'],
  ] as const)('%s %s says %s', async (screen, mode, key) => {
    saved({ rxMhz: 146.52, mode, name: 'CH', screen })
    await mountOn('memories')
    await pressTune()
    expect(pushToast).toHaveBeenCalledWith(t(key, { name: 'CH', freq: '146.520', mode }), 'success', 2500)
  })

  it('a failed tune stays on the screen the operator was on', async () => {
    vi.mocked(api.sstvTune).mockRejectedValue(new Error('no'))
    saved({ rxMhz: 144.8875, mode: 'FM', screen: 'sstv' })
    await mountOn('memories')
    await pressTune()
    expect(document.title).not.toContain('SSTV')
    expect(memoriesStore.get().memories[0].lastUsedUtc).toBeUndefined()
  })
})

describe('the Memories pop-out asks the main window to open SSTV or APRS', () => {
  const ask = (screen: string) =>
    act(async () => {
      window.dispatchEvent(new StorageEvent('storage', { key: MEMORY_OPEN_SCREEN_KEY, newValue: JSON.stringify({ screen, at: 1 }) }))
    })

  it('opens the screen it names, and sends the rig nothing', async () => {
    await mountOn('memories')
    await ask('sstv')
    await settle()
    expect(document.title).toContain('SSTV')
    expect(api.workSpot).not.toHaveBeenCalled()
    expect(api.sstvTune).not.toHaveBeenCalled()
  })

  it('a screen switched off stays shut, and any other word opens nothing', async () => {
    features({ ...SECTIONS_ON, sstv: false })
    await mountOn('memories')
    await ask('sstv')
    await ask('settings')
    await settle()
    expect(document.title).toContain('Memories')
  })
})
