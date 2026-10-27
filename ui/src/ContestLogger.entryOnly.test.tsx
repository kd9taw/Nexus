// @vitest-environment jsdom
//
// THE ENTRY ALONE: the contest logger window's `entry` layout (features/contestLoggerLayout) — the bar and
// the entry (call, exchange, dupe warning, Log) with no last contacts and no contest screen, for an
// operator who finds the screen cluttered. It is the same `?panel=contestlog` window, so the entry is the
// shared one: what is typed goes to the engine's shared contact, and Enter logs it once. Mocked at the
// bridge under the real window, the idiom of ContestLogger.satellite.test.tsx.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { t } from './i18n'
import { LOGGER_LAYOUT_KEY, loggerLayout } from './features/contestLoggerLayout'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

const T0 = 1_792_342_800

const snapshot = () => ({
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 14.25, band: '20m', sideband: 'USB', operatingMode: 'phone', txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay: {
    running: false,
    state: 'Idle',
    qsoCount: 1,
    sections: 0,
    points: 0,
    event: 'arrlfd',
    log: [{ call: 'K9AAA', class: '3A', section: 'IL', band: '20m', mode: 'PH', submode: 'SSB', whenUnix: T0 }],
    receives: [
      { key: 'CLASS', kind: 'fdClass', required: true },
      { key: 'SECTION', kind: 'oneOf', required: true, domains: ['fd_sections'] },
    ],
    composing: [
      { key: 'CLASS', raw: '3A' },
      { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
    ],
    composingText: '3A IL',
    sentExchange: '3A IL',
    assistanceOn: [],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    eventStartUnix: T0 - 3600,
    eventEndUnix: T0 + 3600,
  },
  contestEntry: { rev: 1, call: '', fields: {}, marks: null },
})

let calls: { cmd: string; args?: Record<string, unknown> }[] = []

function respond(cmd: string): unknown {
  if (cmd === 'get_snapshot') return snapshot()
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'arrlfd' }
  if (cmd === 'get_window_on_top') return { supported: true, on: false }
  if (cmd === 'contest_entry_put') return 2
  if (cmd === 'contest_log_manual') return snapshot()
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|resolve_entity|contest_place|get_call_history|get_sat_transponder)$/.test(cmd)) return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  calls = []
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ cmd, args })
      return respond(cmd) as T
    },
  }
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

const wait = (ms: number) => act(async () => new Promise((r) => setTimeout(r, ms)))
async function mountLogger(): Promise<void> {
  render(<DetachedPanel panel="contestlog" />)
  await wait(400)
  await wait(0)
}
const entry = () => screen.getByRole('region', { name: t('contestLogger.entry.aria') })
const toggle = () => within(entry()).getByRole('button', { name: t('contestLogger.layout.label') })
/** What the window shows under its entry: the last contacts and the contest screen, or neither. */
const below = () => ({
  recent: document.querySelector('.contest-logger-recent') != null,
  screen: document.querySelector('.contest-logger > main.layout.single') != null,
})

describe("the logger window's entry-only layout", () => {
  it('opened for the entry alone: the entry and its bar, no last contacts, no contest screen', async () => {
    localStorage.setItem(LOGGER_LAYOUT_KEY, 'entry')
    await mountLogger()
    expect(entry().querySelector('input.le-fd-input-call')).not.toBeNull()
    expect(within(entry()).getByRole('button', { name: t('contestLogger.pin.label') })).toBeTruthy()
    expect(toggle().getAttribute('aria-pressed')).toBe('true')
    expect(below()).toEqual({ recent: false, screen: false })
  })

  it('the toggle switches between the whole window and the entry alone, and remembers it', async () => {
    await mountLogger()
    expect(toggle().getAttribute('aria-pressed')).toBe('false')
    expect(below()).toEqual({ recent: true, screen: true })
    fireEvent.click(toggle())
    expect(below()).toEqual({ recent: false, screen: false })
    expect(localStorage.getItem(LOGGER_LAYOUT_KEY)).toBe('entry')
    fireEvent.click(toggle())
    expect(below()).toEqual({ recent: true, screen: true })
    expect(localStorage.getItem(LOGGER_LAYOUT_KEY)).toBe('full')
  })

  it('an open window follows the main window asking for the other layout', async () => {
    await mountLogger()
    act(() => {
      window.dispatchEvent(new StorageEvent('storage', { key: LOGGER_LAYOUT_KEY, newValue: 'entry' }))
    })
    expect(below()).toEqual({ recent: false, screen: false })
    act(() => {
      window.dispatchEvent(new StorageEvent('storage', { key: LOGGER_LAYOUT_KEY, newValue: 'full' }))
    })
    expect(below()).toEqual({ recent: true, screen: true })
  })

  it('shares the entry: the call goes to the shared contact, and Enter logs it under the shared claim', async () => {
    localStorage.setItem(LOGGER_LAYOUT_KEY, 'entry')
    await mountLogger()
    const call = within(entry()).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' })
    fireEvent.change(call, { target: { value: 'W1AW' } })
    fireEvent.change(within(entry()).getByLabelText('Class'), { target: { value: '2A' } })
    fireEvent.change(within(entry()).getByLabelText('Section'), { target: { value: 'WI' } })
    await wait(300)
    expect(calls.some((c) => c.cmd === 'contest_entry_put'), 'the typed call went to the shared contact').toBe(true)
    calls = []
    fireEvent.keyDown(call, { key: 'Enter' })
    await wait(50)
    const logged = calls.filter((c) => c.cmd === 'contest_log_manual')
    expect(logged).toHaveLength(1)
    expect(logged[0].args?.call).toBe('W1AW')
    // …carrying the shared contact's claim: the engine logs a claimed contact once, whichever window's
    // Enter reaches it first (the claim itself is the engine's, tested there).
    expect(logged[0].args?.entry, 'the log goes through the shared entry claim').toBeDefined()
  })

  it('reads anything but "entry" as the whole window', () => {
    expect([loggerLayout('entry'), loggerLayout('full'), loggerLayout(null), loggerLayout('ENTRY'), loggerLayout('')]).toEqual([
      'entry',
      'full',
      'full',
      'full',
      'full',
    ])
  })
})
