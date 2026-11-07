// @vitest-environment jsdom
//
// A recall from the Memories pop-out reopens the recorded screen too. For a section that is the same
// `work_spot` nav hint the main window always followed. SSTV's and APRS's tunes tick no hint, so the
// pop-out asks, and only once the tune has answered.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import * as api from './api'
import { APP_SNAPSHOT } from './appCockpits.testkit'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import { addMemory, emptyBank, memoriesStore, type Memory } from './features/memories'
import { MEMORY_OPEN_SCREEN_KEY, openScreenRequest } from './features/memoryRecall'
import { pushToast } from './toast'

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
}))

const asked = () => openScreenRequest(localStorage.getItem(MEMORY_OPEN_SCREEN_KEY))

async function recall(over: Partial<Memory> & { rxMhz: number; mode: string }): Promise<void> {
  memoriesStore.set(addMemory(emptyBank(), over))
  const { container } = render(<DetachedPanel panel="memories" />)
  const tune = await waitFor(() => {
    const b = container.querySelector<HTMLButtonElement>('.mv-row-tune')
    expect(b).toBeTruthy()
    return b!
  })
  fireEvent.click(tune)
  await act(async () => {
    for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0))
  })
}

beforeEach(() => {
  localStorage.clear()
  vi.mocked(pushToast).mockClear()
  vi.mocked(api.getSnapshot).mockResolvedValue(APP_SNAPSHOT as never)
  vi.mocked(api.getSettings).mockImplementation(async () => ({ ...defaultSettings }) as never)
  for (const verb of [api.workSpot, api.sstvTuneRecall, api.aprsTune, api.setSidebandOverride]) {
    vi.mocked(verb).mockReset()
    vi.mocked(verb).mockResolvedValue(APP_SNAPSHOT as never)
  }
})
afterEach(cleanup)

describe('the pop-out’s recall', () => {
  it('SSTV: tunes with the SSTV verb and asks the main window to open it', async () => {
    await recall({ rxMhz: 144.8875, mode: 'FM', screen: 'sstv' })
    expect(api.sstvTuneRecall).toHaveBeenCalledExactlyOnceWith(144.8875, '2m', 'FM')
    expect(api.workSpot).not.toHaveBeenCalled()
    expect(asked()).toBe('sstv')
  })

  it('SSTV on a sideband takes the sideband as Phone does', async () => {
    await recall({ rxMhz: 3.845, mode: 'USB', screen: 'sstv' })
    expect(api.setSidebandOverride).toHaveBeenCalledExactlyOnceWith('USB')
  })

  it('APRS: tunes with its own verb and asks', async () => {
    await recall({ rxMhz: 144.39, mode: 'FM', screen: 'aprs' })
    expect(api.aprsTune).toHaveBeenCalledExactlyOnceWith(144.39)
    expect(asked()).toBe('aprs')
  })

  it('a refused tune asks nothing, marks nothing, and says why', async () => {
    vi.mocked(api.aprsTune).mockRejectedValue(new Error('This radio doesn’t cover 144.390 MHz'))
    await recall({ rxMhz: 144.39, mode: 'FM', screen: 'aprs' })
    expect(asked()).toBeNull()
    expect(memoriesStore.get().memories[0].lastUsedUtc).toBeUndefined()
    expect(vi.mocked(pushToast).mock.calls[0][0]).toContain('144.390')
    expect(vi.mocked(pushToast).mock.calls[0][1]).toBe('error')
  })

  it('a section uses work_spot, whose hint the main window follows, and asks nothing', async () => {
    await recall({ rxMhz: 14.08, mode: 'RTTY', screen: 'rtty' })
    expect(api.workSpot).toHaveBeenCalledExactlyOnceWith('rtty', 14.08, '20m', undefined, undefined)
    expect(asked()).toBeNull()
  })

  it('a memory with no screen goes as it always did', async () => {
    await recall({ rxMhz: 146.52, mode: 'FM' })
    expect(api.workSpot).toHaveBeenCalledExactlyOnceWith('phone', 146.52, '2m', undefined, undefined)
    expect(asked()).toBeNull()
  })

  it('a recorded screen switched off falls back, and asks nothing', async () => {
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { sstv: false } }))
    await recall({ rxMhz: 146.52, mode: 'FM', screen: 'sstv' })
    expect(api.sstvTuneRecall).not.toHaveBeenCalled()
    expect(api.workSpot).toHaveBeenCalledExactlyOnceWith('phone', 146.52, '2m', undefined, undefined)
    expect(asked()).toBeNull()
  })
})
