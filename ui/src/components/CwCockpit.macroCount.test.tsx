// @vitest-environment jsdom
//
// THE CW DOCK AT 8, 12 AND 16 MACRO KEYS, AND THE SETTING THAT CHOOSES IT (Settings ▸ CW ▸ Macro
// keys, `macros.cwMacroCount`; RTTY's and PSK's choices are in the same panel and covered here too).
// RTTY's and PSK's docks are `KeyboardDock.macroCount.test.tsx`; the model `features/macroCount.test.ts`.
// Every assertion is on a seam: `send_cw` for what goes on the air, `set_settings` for what is saved.
//   · the dock shows the profile's keys up to the count, F9–F12 keyed through the one send path,
//     keys 13–16 by a click only;
//   · Settings ▸ CW ▸ Macros edits every key that shows, by key, and the count is the form's to write:
//     absent at 8, so an old file saves back unchanged, and choosing fewer keeps the keys it hides.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, waitFor } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import { SettingsPanel } from './SettingsPanel'
import type { AppSnapshot } from '../types'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(async () => ({}))
    return spies[name]
  }
  return { spies, get }
})
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => {
    try {
      return await action()
    } catch {
      return null
    }
  }),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

const sendCw = () => api.get('sendCw')
const setSettings = () => api.get('setSettings')

/** The operator's own profile, all sixteen keys, each saying which it is. */
const SIXTEEN = {
  name: 'Mine',
  macros: Array.from({ length: 16 }, (_, i) => ({ key: `F${i + 1}`, label: `K${i + 1}`, text: `MSG ${i + 1}` })),
}
const EIGHT = { name: 'Mine', macros: SIXTEEN.macros.slice(0, 8) }

let stored: Record<string, unknown> & { macros: Record<string, unknown> }
const withMacros = (macros: Record<string, unknown>) => {
  stored = structuredClone({ ...defaultSettings, mycall: 'W9XYZ', macros: { ...defaultSettings.macros, ...macros } })
}
const lastSaved = () => {
  const calls = setSettings().mock.calls
  return calls[calls.length - 1][0] as typeof stored
}

const snap = {
  mycall: 'W9XYZ',
  radio: {
    dialMhz: 14.05, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false, tuning: false,
    txEnabled: true, txAllowed: true, cwWpm: 22, cwKeyer: 'cat', filterWidthHz: 500, splitTxMhz: null, smeterDb: null,
  },
} as unknown as AppSnapshot

async function flush() {
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}
async function renderCockpit() {
  const r = render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
  await flush()
  return r
}
const slots = () => [...document.querySelectorAll('[data-cw-macro]')].map((el) => el.getAttribute('data-cw-macro'))
const key = (k: string) => document.querySelector(`[data-cw-macro="${k}"] .cw-macro`) as HTMLButtonElement
const press = (k: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(window, { key: k, ...init })

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockReset()
    spy.mockImplementation(async () => ({}))
  }
  withMacros({ cwProfiles: [SIXTEEN], activeCwProfile: 0 })
  api.get('getSettings').mockImplementation(async () => structuredClone(stored))
  api.get('setSettings').mockImplementation(async (s: unknown) => {
    stored = structuredClone(s as typeof stored)
    return {}
  })
  api.get('getCatCwUnprovenRigModels').mockImplementation(async () => [])
  api.get('cwDecode').mockImplementation(async () => ({
    text: '', wpm: 22, sent: [], keyerError: null, candidates: [], state: 'listening', headline: '',
    prompt: '', recommended: null, workedCall: null, rst: null, name: null,
  }))
  api.get('selectPeer').mockImplementation(async () => null)
  api.get('previewCw').mockImplementation(async (text: string) => text)
  api.get('setCwKeyer').mockImplementation(async () => null)
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe("the CW dock's keys at each count", () => {
  it('shows F1–F8 with no count saved, as the grid it always drew, and keeps the keys past them', async () => {
    await renderCockpit()
    expect(slots()).toEqual(['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8'])
    const group = screen.getByRole('group', { name: 'CW macros' })
    expect(group.className).toBe('cw-macros')
    expect(press('F9'), 'F9 was taken at 8').toBe(true)
    await flush()
    expect(sendCw()).not.toHaveBeenCalled()
    press('F8')
    await waitFor(() => expect(sendCw()).toHaveBeenCalledWith('MSG 8'))
  })

  it('shows 12 at 12 in groups of four; F9–F12 send keys 9–12 through the one send path', async () => {
    withMacros({ cwProfiles: [SIXTEEN], activeCwProfile: 0, cwMacroCount: 12 })
    await renderCockpit()
    expect(slots()).toEqual(SIXTEEN.macros.slice(0, 12).map((m) => m.key))
    const group = screen.getByRole('group', { name: 'CW macros' })
    expect(group.querySelectorAll(':scope > .macro-quad')).toHaveLength(3)
    for (const k of ['F9', 'F10', 'F11', 'F12']) expect(press(k), `${k} reached the webview`).toBe(false)
    await waitFor(() => expect(sendCw().mock.calls.map((c) => c[0])).toEqual(['MSG 9', 'MSG 10', 'MSG 11', 'MSG 12']))
    // Shift+F10 is the system's context menu, and Alt/Ctrl/Cmd chords are the system's.
    sendCw().mockClear()
    expect(press('F10', { shiftKey: true })).toBe(true)
    expect(press('F11', { altKey: true })).toBe(true)
    await flush()
    expect(sendCw()).not.toHaveBeenCalled()
  })

  it('shows 16 at 16: keys 13–16 carry no key name and are sent by a click, never by a key', async () => {
    withMacros({ cwProfiles: [SIXTEEN], activeCwProfile: 0, cwMacroCount: 16 })
    await renderCockpit()
    expect(slots()).toHaveLength(16)
    expect(key('F12').querySelector('.cw-macro-key')!.textContent).toBe('F12')
    expect(key('F13').querySelector('.cw-macro-key')!.textContent!.trim()).toBe('')
    expect(key('F13').title).toContain('Click to send')
    for (const k of ['F13', 'F14', 'F15', 'F16']) expect(press(k)).toBe(true)
    await flush()
    expect(sendCw()).not.toHaveBeenCalled()
    fireEvent.click(key('F14'))
    await waitFor(() => expect(sendCw()).toHaveBeenCalledWith('MSG 14'))
  })

  it('on the built-ins at 12, F9–F12 start empty, and the editor saves key 12 into the profile', async () => {
    withMacros({ cwProfiles: [{ name: 'Default', macros: [] }], activeCwProfile: 0, cwMacroCount: 12 })
    await renderCockpit()
    expect(slots()).toHaveLength(12)
    expect(key('F12').querySelector('.cw-macro-label')!.textContent).toBe('(empty)')
    fireEvent.click(key('F12'))
    fireEvent.change(screen.getByLabelText(/^title$/i), { target: { value: 'QRL' } })
    fireEvent.change(screen.getByLabelText(/^message$/i), { target: { value: 'QRL?' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
    await flush()
    const saved = (lastSaved().macros.cwProfiles as { macros: { key: string; label: string; text: string }[] }[])[0].macros
    expect(saved.map((m) => m.key)).toEqual(['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F12'])
    expect(saved[8]).toEqual({ key: 'F12', label: 'QRL', text: 'QRL?' })
    expect(lastSaved().macros.cwMacroCount, 'the cockpit save kept the count').toBe(12)
    expect(sendCw(), 'Save keyed the rig').not.toHaveBeenCalled()
  })
})

describe('Settings: the Macro keys choice, and the CW grid that shows what it chose', () => {
  const settingsFeatures = { enabled: () => true, setEnabled: vi.fn(), all: () => [], profile: 'full', setProfile: vi.fn() } as unknown as FeaturesApi
  async function openSettings(tab: 'CW' | 'Digital') {
    for (const name of ['getRigModels', 'getAllRigModels', 'getSerialPortsDetailed', 'getBandPlan', 'getCredentialsStatus', 'getConnectionLog', 'detectRigs'])
      api.get(name).mockImplementation(async () => [])
    api.get('getAudioDevices').mockImplementation(async () => ({ input: [], output: [] }))
    api.get('appVersion').mockImplementation(async () => '0.0.0')
    render(
      <SettingsPanel
        fieldDay={null as never}
        activeRadioId={0}
        scale={1 as never}
        scaleMode={'auto' as never}
        scaleCap={1 as never}
        onScaleModeChange={() => {}}
        onScaleCapChange={() => {}}
        density={'comfortable' as never}
        onDensityChange={() => {}}
        onResetLayout={() => {}}
        features={settingsFeatures}
      />,
    )
    fireEvent.click(await screen.findByRole('tab', { name: tab }))
    await screen.findAllByRole('combobox', { name: /^Macro keys/ })
  }
  // The label wraps the choice and its hint, so the hint is part of the name.
  const countSelects = () => screen.getAllByRole('combobox', { name: /^Macro keys/ }) as HTMLSelectElement[]
  const save = async () => {
    fireEvent.click(await screen.findByRole('button', { name: /^Save$/i }))
    await waitFor(() => expect(setSettings()).toHaveBeenCalled())
  }

  it('offers 8, 12 and 16, at 8 for a file with no count, and writes 16 when chosen', async () => {
    withMacros({ cwProfiles: [EIGHT], activeCwProfile: 0 })
    await openSettings('CW')
    const [cw] = countSelects()
    expect([...cw.options].map((o) => [o.value, o.textContent])).toEqual([
      ['8', '8 keys'],
      ['12', '12 keys'],
      ['16', '16 keys'],
    ])
    expect(cw.value).toBe('8')
    expect(screen.getAllByLabelText(/^F\d+ text$/)).toHaveLength(8)
    fireEvent.change(cw, { target: { value: '16' } })
    // The grid shows every key the dock will: 16 rows, keys 13–16 named without an F-key.
    expect(screen.getAllByLabelText(/^(F\d+|#\d+) text$/)).toHaveLength(16)
    fireEvent.change(screen.getByLabelText('#13 text'), { target: { value: 'QRZ?' } })
    await save()
    expect(lastSaved().macros.cwMacroCount).toBe(16)
    const mine = (lastSaved().macros.cwProfiles as { macros: { key: string; text: string }[] }[])[0].macros
    expect(mine.map((m) => m.key)).toEqual(['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F13'])
    expect(mine[8]).toMatchObject({ key: 'F13', text: 'QRZ?' })
  })

  it('a save at 8 writes no count — the file saves back as it was', async () => {
    withMacros({ cwProfiles: [EIGHT], activeCwProfile: 0 })
    await openSettings('CW')
    fireEvent.change(screen.getByLabelText('F2 text'), { target: { value: 'edited' } })
    await save()
    expect('cwMacroCount' in lastSaved().macros).toBe(false)
  })

  it('choosing 8 after 16 hides keys 9–16 and keeps them in the profile', async () => {
    withMacros({ cwProfiles: [SIXTEEN], activeCwProfile: 0, cwMacroCount: 16 })
    await openSettings('CW')
    expect(countSelects()[0].value).toBe('16')
    fireEvent.change(countSelects()[0], { target: { value: '8' } })
    expect(screen.getAllByLabelText(/^(F\d+|#\d+) text$/)).toHaveLength(8)
    await save()
    expect('cwMacroCount' in lastSaved().macros, '8 is written as no count').toBe(false)
    expect((lastSaved().macros.cwProfiles as { macros: unknown[] }[])[0].macros).toEqual(SIXTEEN.macros)
  })

  it('RTTY and PSK each have their own choice, written to their own field', async () => {
    withMacros({})
    await openSettings('Digital')
    const [rtty, psk] = countSelects()
    expect([rtty.value, psk.value]).toEqual(['8', '8'])
    fireEvent.change(rtty, { target: { value: '12' } })
    fireEvent.change(psk, { target: { value: '16' } })
    await save()
    expect(lastSaved().macros.rttyMacroCount).toBe(12)
    expect(lastSaved().macros.pskMacroCount).toBe(16)
    expect('cwMacroCount' in lastSaved().macros).toBe(false)
  })
})
