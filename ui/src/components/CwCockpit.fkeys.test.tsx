// @vitest-environment jsdom
//
// ONE PRESS OF AN F-KEY IS ONE SEND, AND A MODIFIED F-KEY IS NOT A MACRO.
//
// The CW cockpit's window listener sent the macro on every `keydown` of its key: the
// auto-repeat of a held F3 queued the exchange again on every repeat, and Ctrl+F3, Alt+F3 or
// Cmd+F3 sent it too — Alt+F4, the window manager's close, sent F4's macro. RTTY and PSK have
// the guard (`RttyCockpit.tsx`'s keydown): a repeat is swallowed, and a key with Alt, Ctrl or
// Cmd held is left to the system. CW now has the same one.
//
// Controls: a plain press sends once; Shift is not one of the three (RTTY's rule); PageUp
// held still steps the speed on every repeat; Esc still stops, held or not.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot } from '../types'
import { sendCw, setCwWpm, stopCw } from '../api'

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 }, rigModel: 0 })),
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    cwDecode: vi.fn(async () => ({
      text: '', wpm: 22, sent: [], keyerError: null, candidates: [], state: 'listening', headline: '',
      prompt: '', recommended: null, workedCall: null, rst: null, name: null,
    })),
    selectPeer: vi.fn(async () => null),
    previewCw: vi.fn(async (t: string) => t),
    setCwKeyer: vi.fn(async () => null),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

const mockSendCw = sendCw as unknown as ReturnType<typeof vi.fn>
const mockSetCwWpm = setCwWpm as unknown as ReturnType<typeof vi.fn>
const mockStopCw = stopCw as unknown as ReturnType<typeof vi.fn>

function snap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false,
      txEnabled: true, txAllowed: true, cwWpm: 22, cwKeyer: 'cat', filterWidthHz: 500,
      splitTxMhz: null, smeterDb: null,
    },
  } as unknown as AppSnapshot
}

async function flush() {
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve()
  })
}

async function renderCockpit() {
  render(<CwCockpit snap={snap()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
  await flush()
}

/** A keydown on the window; true when nothing cancelled its default. */
const press = (key: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(window, { key, ...init })

beforeEach(() => {
  mockSendCw.mockClear()
  mockSetCwWpm.mockClear()
  mockStopCw.mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('CW F-keys: one press, one send', () => {
  it('a held F3 sends the exchange once, whatever its auto-repeat does', async () => {
    await renderCockpit()
    press('F3')
    for (let i = 0; i < 5; i++) press('F3', { repeat: true })
    await flush()
    expect(mockSendCw).toHaveBeenCalledTimes(1)
  })

  it('Ctrl+F3, Alt+F3 and Cmd+F3 send nothing', async () => {
    await renderCockpit()
    press('F3', { ctrlKey: true })
    press('F3', { altKey: true })
    press('F3', { metaKey: true })
    await flush()
    expect(mockSendCw).toHaveBeenCalledTimes(0)
  })

  it('Alt+F4 sends nothing and is left to the window manager', async () => {
    await renderCockpit()
    const untouched = press('F4', { altKey: true })
    await flush()
    expect(mockSendCw).toHaveBeenCalledTimes(0)
    expect(untouched, 'Alt+F4 belongs to the window manager').toBe(true)
  })

  // ── CONTROLS ──────────────────────────────────────────────────────────────────────────

  it('a plain F3 sends once, and keeps it from the browser', async () => {
    await renderCockpit()
    expect(press('F3'), 'F3 reached the browser').toBe(false)
    await flush()
    expect(mockSendCw).toHaveBeenCalledTimes(1)
    // Shift is not one of the three: RTTY's rule.
    press('F2', { shiftKey: true })
    await flush()
    expect(mockSendCw).toHaveBeenCalledTimes(2)
  })

  it('PageUp held still steps the speed on every repeat', async () => {
    await renderCockpit()
    press('PageUp')
    press('PageUp', { repeat: true })
    expect(mockSetCwWpm.mock.calls.filter((c) => c[1] === false).map((c) => c[0])).toEqual([24, 26])
  })

  it('Esc still stops, held or not', async () => {
    await renderCockpit()
    press('Escape')
    press('Escape', { repeat: true })
    expect(mockStopCw).toHaveBeenCalledTimes(2)
  })
})
